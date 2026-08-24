#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

use std::{env, path::PathBuf};

#[cfg(target_os = "macos")]
use std::path::Path;

#[cfg(not(target_os = "macos"))]
use std::process;

use async_stream::stream;
use download::DownloadStatus;
use extract::ExtractStatus;
use futures_core::Stream;
use futures_util::{StreamExt, pin_mut};

#[cfg(not(target_os = "macos"))]
use tokio::{fs::File, io::AsyncWriteExt, process::Command};

#[cfg_attr(coverage_nightly, coverage(off))]
mod download;
mod error;
#[cfg_attr(coverage_nightly, coverage(off))]
mod extract;
#[cfg_attr(coverage_nightly, coverage(off))]
mod options;
pub mod status_handler;
mod version;

pub use error::ObsBootstrapError;

pub use options::{ObsBootstrapperOptions, ObsBundleManifest};

use crate::status_handler::{ObsBootstrapConsoleHandler, ObsBootstrapStatusHandler};

#[cfg(all(not(feature = "__ci"), target_os = "linux"))]
compile_error!("libobs-bootstrapper is not supported on Linux.");

pub enum BootstrapStatus {
    /// Downloading status (first is progress from 0.0 to 1.0 and second is message)
    Downloading(f32, String),

    /// Extracting status (first is progress from 0.0 to 1.0 and second is message)
    Extracting(f32, String),
    Error(ObsBootstrapError),
    /// The application must be restarted to use the new version of OBS.
    /// This is because the obs.dll file is in use by the application and can not be replaced while running.
    /// Therefore, the "updater" is spawned to watch for the application to exit and rename the "obs_new.dll" file to "obs.dll".
    /// The updater will start the application again with the same arguments as the original application.
    RestartRequired,
    /// Bootstrap completed successfully without requiring a restart.
    /// This is used on macOS where files can be moved immediately.
    Done,
}

/// A struct for bootstrapping OBS Studio.
///
/// This struct downloads and installs the one OBS bundle named by the caller's
/// exact manifest.
///
/// If you want to use this bootstrapper to also install required OBS binaries at runtime,
/// do the following:
/// - On Windows, enable the default `install_dummy_dll` feature to place the crate's vendored
///   placeholder `obs.dll` in the build output. It will be replaced by the verified bundle.
/// - Parse an `ObsBundleManifest`, then call `ObsBootstrapper::bootstrap()` at startup.
/// - If BootstrapStatus::RestartRequired is returned, you'll need to restart your application. A updater process has been spawned to watch for the application to exit and rename the `obs_new.dll` file to `obs.dll`.
/// - Exit the application. The updater process will wait for the application to exit and rename the `obs_new.dll` file to `obs.dll` and restart your application with the same arguments as before.
///
/// [Example project](https://github.com/libobs-rs/libobs-rs/tree/main/examples/download-at-runtime)
pub struct ObsBootstrapper {}

pub const UPDATER_SCRIPT: &str = include_str!("./updater.ps1");

fn resolve_install_dir(options: &ObsBootstrapperOptions) -> Result<PathBuf, ObsBootstrapError> {
    if let Some(dir) = options.get_install_dir() {
        return Ok(dir.clone());
    }

    #[cfg(not(target_os = "linux"))]
    let executable =
        env::current_exe().map_err(|e| ObsBootstrapError::IoError("Getting current exe", e))?;
    #[cfg(not(target_os = "linux"))]
    let parent = executable.parent().ok_or_else(|| {
        ObsBootstrapError::IoError(
            "Failed to get parent directory",
            std::io::Error::from(std::io::ErrorKind::InvalidInput),
        )
    })?;

    #[cfg(not(target_os = "linux"))]
    {
        return Ok(parent.to_path_buf());
    }

    #[cfg(target_os = "linux")]
    {
        let _ = options;
        unreachable!("libobs-bootstrapper is not supported on Linux.");
    }
}

fn get_obs_dll_path(options: &ObsBootstrapperOptions) -> Result<PathBuf, ObsBootstrapError> {
    #[cfg(not(target_os = "linux"))]
    let install_dir = resolve_install_dir(options)?;

    #[cfg(target_os = "macos")]
    {
        // macOS: Check for libobs.framework
        Ok(install_dir.join("libobs.framework/Versions/A/libobs"))
    }

    #[cfg(target_os = "windows")]
    {
        // Windows: Check for obs.dll
        Ok(install_dir.join("obs.dll"))
    }

    #[cfg(target_os = "linux")]
    {
        let _ = options;
        unreachable!("libobs-bootstrapper is not supported on Linux.");
    }
}

pub(crate) fn bootstrap(
    options: &ObsBootstrapperOptions,
) -> Result<Option<impl Stream<Item = BootstrapStatus>>, ObsBootstrapError> {
    let install_dir = resolve_install_dir(options)?;

    log::trace!("Checking exact OBS bundle identity...");
    let should_bootstrap = !ObsBootstrapper::is_valid_installation_with_options(options)?;

    if !should_bootstrap {
        log::debug!("No update needed.");
        return Ok(None);
    }

    #[allow(unused_variables)]
    let options = options.clone();
    let install_dir = install_dir.clone();
    Ok(Some(stream! {
        log::debug!("Downloading OBS manifest {}", options.manifest.identity());
        let download_stream = download::download_obs(&options.manifest).await;
        if let Err(err) = download_stream {
            yield BootstrapStatus::Error(err);
            return;
        }

        let download_stream = download_stream.unwrap();
        pin_mut!(download_stream);

        let mut file = None;
        while let Some(item) = download_stream.next().await {
            match item {
                DownloadStatus::Error(err) => {
                    yield BootstrapStatus::Error(err);
                    return;
                }
                DownloadStatus::Progress(progress, message) => {
                    yield BootstrapStatus::Downloading(progress, message);
                }
                DownloadStatus::Done(path) => {
                    file = Some(path)
                }
            }
        }

        let archive_file = file.ok_or(ObsBootstrapError::InvalidState);
        if let Err(err) = archive_file {
            yield BootstrapStatus::Error(err);
            return;
        }

        log::debug!("Extracting OBS to {:?}", archive_file);
        let archive_file = archive_file.unwrap();
        let extract_stream = extract::extract_obs(&archive_file, &install_dir).await;
        if let Err(err) = extract_stream {
            yield BootstrapStatus::Error(err);
            return;
        }

        let extract_stream = extract_stream.unwrap();
        pin_mut!(extract_stream);

        while let Some(item) = extract_stream.next().await {
            match item {
                ExtractStatus::Error(err) => {
                    yield BootstrapStatus::Error(err);
                    return;
                }
                ExtractStatus::Progress(progress, message) => {
                    yield BootstrapStatus::Extracting(progress, message);
                }
            }
        }

        if let Err(err) = tokio::fs::remove_file(&archive_file).await {
            yield BootstrapStatus::Error(ObsBootstrapError::IoError("Removing downloaded bundle", err));
            return;
        }

        let staged_directory = install_dir.join("obs_new");
        let manifest = options.manifest.clone();
        let directory = staged_directory.clone();
        let verification = tokio::task::spawn_blocking(move || manifest.verify_staged_files(&directory)).await;
        match verification {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                yield BootstrapStatus::Error(err);
                return;
            }
            Err(err) => {
                yield BootstrapStatus::Error(ObsBootstrapError::GeneralError(format!(
                    "staged bundle verification task failed: {err}"
                )));
                return;
            }
        }

        if let Err(err) = options.manifest.write_receipt(&staged_directory) {
            yield BootstrapStatus::Error(err);
            return;
        }

        // Platform-specific post-extraction handling
        #[cfg(target_os = "macos")]
        {
            // On macOS, we can move files immediately since dylibs can be replaced while running
            let r = move_obs_files_macos(&install_dir).await;
            if let Err(err) = r {
                yield BootstrapStatus::Error(err);
                return;
            }
            yield BootstrapStatus::Done;
        }

        #[cfg(not(target_os = "macos"))]
        {
            // On Windows, we need to spawn an updater and restart
            let r = spawn_updater(options.clone()).await;
            if let Err(err) = r {
                yield BootstrapStatus::Error(err);
                return;
            }
            yield BootstrapStatus::RestartRequired;
        }
    }))
}

#[cfg(not(target_os = "macos"))]
pub(crate) async fn spawn_updater(
    options: ObsBootstrapperOptions,
) -> Result<(), ObsBootstrapError> {
    let pid = process::id();
    let args = env::args().collect::<Vec<_>>();
    // Skip the first argument which is the executable path
    let args = args.into_iter().skip(1).collect::<Vec<_>>();

    let updater_path = env::temp_dir().join(format!("libobs-updater-{}.ps1", uuid::Uuid::new_v4()));
    let mut updater_file = File::create_new(&updater_path)
        .await
        .map_err(|e| ObsBootstrapError::IoError("Creating updater script", e))?;

    updater_file
        .write_all(UPDATER_SCRIPT.as_bytes())
        .await
        .map_err(|e| ObsBootstrapError::IoError("Writing updater script", e))?;
    updater_file
        .sync_all()
        .await
        .map_err(|e| ObsBootstrapError::IoError("Syncing updater script", e))?;

    let mut command = Command::new("powershell");
    command
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-NoProfile")
        .arg("-WindowStyle")
        .arg("Hidden")
        .arg("-File")
        .arg(updater_path)
        .arg("-processPid")
        .arg(pid.to_string())
        .arg("-binary")
        .arg(
            env::current_exe()
                .map_err(|e| ObsBootstrapError::IoError("Getting current exe", e))?
                .to_string_lossy()
                .to_string(),
        );

    if options.restart_after_update {
        command.arg("-restart");
    }

    // Encode arguments as hex string (UTF-8, null-separated)
    if !args.is_empty() {
        let joined = args.join("\0");
        let bytes = joined.as_bytes();
        let hex_str = hex::encode(bytes);
        command.arg("-argumentHex");
        command.arg(hex_str);
    }

    command
        .spawn()
        .map_err(|e| ObsBootstrapError::IoError("Spawning updater process", e))?;

    Ok(())
}

#[cfg(target_os = "macos")]
async fn move_obs_files_macos(install_dir: &Path) -> Result<(), ObsBootstrapError> {
    use tokio::fs;

    let obs_new_dir = install_dir.join("obs_new");
    let staged_receipt = obs_new_dir.join(options::RECEIPT_NAME);
    let installed_receipt = install_dir.join(options::RECEIPT_NAME);

    if !obs_new_dir.exists() {
        return Err(ObsBootstrapError::InvalidFormatError(format!(
            "obs_new directory not found at {}",
            obs_new_dir.display()
        )));
    }
    if !staged_receipt.is_file() {
        return Err(ObsBootstrapError::InvalidFormatError(
            "staged install receipt is missing".to_string(),
        ));
    }
    if installed_receipt.exists() {
        fs::remove_file(&installed_receipt)
            .await
            .map_err(|e| ObsBootstrapError::IoError("Removing old install receipt", e))?;
    }

    log::info!(
        "Moving OBS files from {:?} to {:?}",
        obs_new_dir,
        install_dir
    );

    // Read all entries in obs_new
    let mut entries = fs::read_dir(&obs_new_dir)
        .await
        .map_err(|e| ObsBootstrapError::IoError("Reading obs_new directory", e))?;

    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| ObsBootstrapError::IoError("Reading directory entry", e))?
    {
        let src_path = entry.path();
        let file_name = entry.file_name();
        if file_name == options::RECEIPT_NAME {
            continue;
        }
        let dest_path = install_dir.join(&file_name);

        // Remove destination if it exists
        if dest_path.exists() {
            if dest_path.is_dir() {
                fs::remove_dir_all(&dest_path)
                    .await
                    .map_err(|e| ObsBootstrapError::IoError("Removing old directory", e))?;
            } else {
                fs::remove_file(&dest_path)
                    .await
                    .map_err(|e| ObsBootstrapError::IoError("Removing old file", e))?;
            }
        }

        // Move the file/directory
        log::debug!("  Moving {:?} to {:?}", file_name, dest_path);
        fs::rename(&src_path, &dest_path)
            .await
            .map_err(|e| ObsBootstrapError::IoError("Moving file/directory", e))?;
    }

    fs::rename(&staged_receipt, &installed_receipt)
        .await
        .map_err(|e| ObsBootstrapError::IoError("Publishing install receipt", e))?;

    // Remove the now-empty obs_new directory
    fs::remove_dir(&obs_new_dir)
        .await
        .map_err(|e| ObsBootstrapError::IoError("Removing obs_new directory", e))?;

    log::info!("✓ OBS files moved successfully");

    Ok(())
}
pub enum ObsBootstrapperResult {
    /// No action was needed, OBS is already installed and up to date.
    None,
    /// The application must be restarted to complete the installation or update of OBS.
    Restart,
}

/// A convenience type that exposes high-level helpers to detect, update and
/// bootstrap an OBS installation.
///
/// The bootstrapper coordinates version checks and the streaming bootstrap
/// process. It does not itself perform low-level network or extraction work;
/// instead it delegates to internal modules (version checking and the
/// bootstrap stream) and surfaces a simple API for callers.
impl ObsBootstrapper {
    /// Returns true only when the installed OBS ABI and the persisted bundle
    /// receipt both match the caller's manifest.
    pub fn is_valid_installation_with_options(
        options: &ObsBootstrapperOptions,
    ) -> Result<bool, ObsBootstrapError> {
        let install_dir = resolve_install_dir(options)?;
        if !options.manifest.receipt_matches(&install_dir) {
            return Ok(false);
        }
        let installed = version::get_installed_version(&get_obs_dll_path(options)?)?;
        Ok(installed.as_deref() == Some(options.manifest.obs_abi()))
    }

    /// Bootstraps OBS using the provided options and a default console status
    /// handler.
    ///
    /// This is a convenience wrapper around `bootstrap_with_handler` that
    /// supplies an `ObsBootstrapConsoleHandler` as the status consumer.
    ///
    /// # Returns
    ///
    /// - `Ok(ObsBootstrapperResult::None)` if no action was necessary.
    /// - `Ok(ObsBootstrapperResult::Restart)` if the bootstrap completed and a
    ///   restart is required.
    ///
    /// # Errors
    ///
    /// Returns `Err(ObsBootstrapError)` for any failure that prevents the
    /// bootstrap from completing (download failures, extraction failures,
    /// general errors).
    pub async fn bootstrap(
        options: &options::ObsBootstrapperOptions,
    ) -> Result<ObsBootstrapperResult, ObsBootstrapError> {
        ObsBootstrapper::bootstrap_with_handler(
            options,
            Box::new(ObsBootstrapConsoleHandler::default()),
        )
        .await
    }

    /// Bootstraps OBS using the provided options and a custom status handler.
    ///
    /// The handler will receive progress updates as the bootstrap stream emits
    /// statuses. The method drives the bootstrap stream to completion and maps
    /// stream statuses into handler calls or final results:
    ///
    /// - `BootstrapStatus::Downloading(progress, message)` → calls
    ///   `handler.handle_downloading(progress, message)`. Handler errors are
    ///   mapped to `ObsBootstrapError::DownloadError`.
    /// - `BootstrapStatus::Extracting(progress, message)` → calls
    ///   `handler.handle_extraction(progress, message)`. Handler errors are
    ///   mapped to `ObsBootstrapError::ExtractError`.
    /// - `BootstrapStatus::Error(err)` → returns `Err(ObsBootstrapError::GeneralError(_))`.
    /// - `BootstrapStatus::RestartRequired` → returns `Ok(ObsBootstrapperResult::Restart)`.
    ///
    /// If the underlying `bootstrap(options)` call returns `None` there is
    /// nothing to do and the function returns `Ok(ObsBootstrapperResult::None)`.
    ///
    /// # Parameters
    ///
    /// - `options`: configuration that controls download/extraction behavior.
    /// - `handler`: user-provided boxed trait object that receives progress
    ///   notifications; it is called on each progress update and can fail.
    ///
    /// # Returns
    ///
    /// - `Ok(ObsBootstrapperResult::None)` when no work was required or the
    ///   stream completed without requiring a restart.
    /// - `Ok(ObsBootstrapperResult::Restart)` when the bootstrap succeeded and
    ///   a restart is required.
    ///
    /// # Errors
    ///
    /// Returns `Err(ObsBootstrapError)` when:
    /// - the bootstrap pipeline could not be started,
    /// - the handler returns an error while handling a download or extraction
    ///   update (mapped respectively to `DownloadError` / `ExtractError`),
    /// - or when the bootstrap stream yields a general error.
    pub async fn bootstrap_with_handler<E: Send + Sync + 'static + std::error::Error>(
        options: &options::ObsBootstrapperOptions,
        mut handler: Box<dyn ObsBootstrapStatusHandler<Error = E>>,
    ) -> Result<ObsBootstrapperResult, ObsBootstrapError> {
        let stream = bootstrap(options)?;

        if let Some(stream) = stream {
            pin_mut!(stream);

            log::trace!("Waiting for bootstrapper to finish");
            while let Some(item) = stream.next().await {
                match item {
                    BootstrapStatus::Downloading(progress, message) => {
                        handler
                            .handle_downloading(progress, message)
                            .map_err(|e| ObsBootstrapError::Abort(Box::new(e)))?;
                    }
                    BootstrapStatus::Extracting(progress, message) => {
                        handler
                            .handle_extraction(progress, message)
                            .map_err(|e| ObsBootstrapError::Abort(Box::new(e)))?;
                    }
                    BootstrapStatus::Error(err) => {
                        return Err(err);
                    }
                    BootstrapStatus::RestartRequired => {
                        return Ok(ObsBootstrapperResult::Restart);
                    }
                    BootstrapStatus::Done => {
                        return Ok(ObsBootstrapperResult::None);
                    }
                }
            }
        }

        Ok(ObsBootstrapperResult::None)
    }
}
