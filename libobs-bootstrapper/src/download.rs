use std::{env::temp_dir, path::PathBuf};

use async_stream::stream;
use futures_core::Stream;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::{fs::File, io::AsyncWriteExt};
use uuid::Uuid;

use crate::{ObsBootstrapError, ObsBundleManifest};

pub enum DownloadStatus {
    Error(ObsBootstrapError),
    Progress(f32, String),
    Done(PathBuf),
}

pub(crate) async fn download_obs(
    manifest: &ObsBundleManifest,
) -> Result<impl Stream<Item = DownloadStatus>, ObsBootstrapError> {
    let client = reqwest::ClientBuilder::new()
        .user_agent("libobs-bootstrapper")
        .build()
        .map_err(|error| ObsBootstrapError::DownloadError("Building HTTP client", error))?;
    let response = client
        .get(manifest.url().clone())
        .send()
        .await
        .map_err(|error| ObsBootstrapError::DownloadError("Fetching OBS bundle", error))?
        .error_for_status()
        .map_err(|error| ObsBootstrapError::DownloadError("Fetching OBS bundle", error))?;
    if response
        .content_length()
        .is_some_and(|size| size != manifest.size())
    {
        return Err(ObsBootstrapError::InvalidFormatError(format!(
            "bundle content length does not match manifest: expected {}, got {}",
            manifest.size(),
            response.content_length().unwrap()
        )));
    }

    let mut bytes_stream = response.bytes_stream();
    let path = temp_dir().join(format!(
        "libobs-{}.{}",
        Uuid::new_v4(),
        manifest.archive_extension()
    ));
    let mut file = File::create_new(&path)
        .await
        .map_err(|error| ObsBootstrapError::IoError("Creating bundle file", error))?;
    let expected_size = manifest.size();
    let expected_sha256 = *manifest.sha256();

    Ok(stream! {
        let mut size = 0_u64;
        let mut hasher = Sha256::new();
        yield DownloadStatus::Progress(0.0, "Downloading OBS".to_string());
        while let Some(chunk) = bytes_stream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    yield DownloadStatus::Error(ObsBootstrapError::DownloadError("Receiving OBS bundle", error));
                    return;
                }
            };
            size = match size.checked_add(chunk.len() as u64) {
                Some(size) if size <= expected_size => size,
                _ => {
                    yield DownloadStatus::Error(ObsBootstrapError::InvalidFormatError(
                        "bundle exceeds manifest size".to_string(),
                    ));
                    return;
                }
            };
            hasher.update(&chunk);
            if let Err(error) = file.write_all(&chunk).await {
                yield DownloadStatus::Error(ObsBootstrapError::IoError("Writing bundle file", error));
                return;
            }
            yield DownloadStatus::Progress(
                size as f32 / expected_size as f32,
                "Downloading OBS".to_string(),
            );
        }
        if size != expected_size {
            yield DownloadStatus::Error(ObsBootstrapError::InvalidFormatError(format!(
                "bundle size does not match manifest: expected {expected_size}, got {size}",
            )));
            return;
        }
        if hasher.finalize().as_slice() != expected_sha256 {
            yield DownloadStatus::Error(ObsBootstrapError::HashMismatchError);
            return;
        }
        if let Err(error) = file.sync_all().await {
            yield DownloadStatus::Error(ObsBootstrapError::IoError("Syncing bundle file", error));
            return;
        }
        yield DownloadStatus::Done(path);
    })
}
