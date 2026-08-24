use std::{
    collections::HashSet,
    path::{Component, Path},
};

use libobs::{LIBOBS_API_MAJOR_VER, LIBOBS_API_MINOR_VER, LIBOBS_API_PATCH_VER};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::ObsBootstrapError;

const MANIFEST_SCHEMA: &str = "libobs-bootstrap-manifest-v1";
const MAX_MANIFEST_BYTES: usize = 4 * 1024 * 1024;
const MAX_BUNDLE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const MAX_FILE_COUNT: usize = 65_536;
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_TOTAL_FILE_BYTES: u64 = 16 * 1024 * 1024 * 1024;
const MAX_PATH_BYTES: usize = 1024;
const MAX_PATH_COMPONENTS: usize = 32;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestDocument {
    schema: String,
    platform: String,
    arch: String,
    obs_abi: String,
    implementation_id: String,
    provenance: ProvenanceDocument,
    files: Vec<FileDocument>,
    bundle: BundleDocument,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProvenanceDocument {
    obs: ObsProvenanceDocument,
    libobs_rs: LibobsRsProvenanceDocument,
    build: BuildProvenanceDocument,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObsProvenanceDocument {
    upstream_commit: String,
    upstream_tree: String,
    patch_commit: String,
    patched_tree: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LibobsRsProvenanceDocument {
    commit: String,
    tree: String,
    generated_bindings: Vec<FileDocument>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BuildProvenanceDocument {
    recipe: FileDocument,
    builder_image: BuilderImageDocument,
    dependencies: Vec<DependencyDocument>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BuilderImageDocument {
    reference: String,
    digest: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DependencyDocument {
    name: String,
    material: SourceMaterialDocument,
    vcs: Option<VcsDocument>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceMaterialDocument {
    url: String,
    size: u64,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VcsDocument {
    repository: String,
    commit: String,
    tree: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileDocument {
    path: String,
    size: u64,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleDocument {
    url: String,
    sha256: String,
    size: u64,
}

#[derive(Debug, Clone)]
pub struct ObsBundleManifest {
    identity: String,
    platform: String,
    arch: String,
    obs_abi: String,
    implementation_id: String,
    url: url::Url,
    sha256: [u8; 32],
    size: u64,
}

impl ObsBundleManifest {
    pub fn from_json(bytes: &[u8], expected_identity: &str) -> Result<Self, ObsBootstrapError> {
        Self::from_json_for_target(
            bytes,
            expected_identity,
            std::env::consts::OS,
            std::env::consts::ARCH,
        )
    }

    fn from_json_for_target(
        bytes: &[u8],
        expected_identity: &str,
        target_platform: &str,
        target_arch: &str,
    ) -> Result<Self, ObsBootstrapError> {
        if bytes.is_empty() || bytes.len() > MAX_MANIFEST_BYTES {
            return Err(invalid("bundle manifest is outside its byte limit"));
        }
        let expected_identity = decode_sha256("manifest", expected_identity)?;
        let actual_identity: [u8; 32] = Sha256::digest(bytes).into();
        if actual_identity != expected_identity {
            return Err(invalid(
                "bundle manifest SHA-256 does not match the application identity",
            ));
        }

        let document: ManifestDocument = serde_json::from_slice(bytes)
            .map_err(|error| invalid(format!("invalid bundle manifest: {error}")))?;
        Self::from_document(document, actual_identity, target_platform, target_arch)
    }

    fn from_document(
        document: ManifestDocument,
        identity: [u8; 32],
        target_platform: &str,
        target_arch: &str,
    ) -> Result<Self, ObsBootstrapError> {
        if document.schema != MANIFEST_SCHEMA {
            return Err(invalid(format!(
                "unsupported bundle manifest schema: {}",
                document.schema
            )));
        }
        if !matches!(document.platform.as_str(), "windows" | "macos") {
            return Err(invalid(format!(
                "unsupported bundle platform: {}",
                document.platform
            )));
        }
        if document.platform != target_platform || document.arch != target_arch {
            return Err(invalid(format!(
                "bundle target {}/{} does not match runtime {target_platform}/{target_arch}",
                document.platform, document.arch
            )));
        }

        let expected_abi =
            format!("{LIBOBS_API_MAJOR_VER}.{LIBOBS_API_MINOR_VER}.{LIBOBS_API_PATCH_VER}");
        if document.obs_abi != expected_abi {
            return Err(invalid(format!(
                "bundle OBS ABI {} does not match linked ABI {expected_abi}",
                document.obs_abi
            )));
        }
        validate_identifier("implementation ID", &document.implementation_id)?;
        validate_provenance(&document.provenance)?;
        validate_files("bundle files", &document.files)?;
        if document.platform == "windows" {
            validate_windows_file_paths(&document.files)?;
        }
        let required_runtime = match document.platform.as_str() {
            "windows" => "obs.dll",
            "macos" => "libobs.framework/Versions/A/libobs",
            _ => unreachable!("platform validated above"),
        };
        if !document
            .files
            .iter()
            .any(|file| file.path == required_runtime)
        {
            return Err(invalid(format!(
                "bundle files do not contain the required runtime: {required_runtime}"
            )));
        }
        if document.files.len() > MAX_FILE_COUNT {
            return Err(invalid("bundle file count exceeds its limit"));
        }
        let mut total_size = 0_u64;
        for file in &document.files {
            if file.size > MAX_FILE_BYTES {
                return Err(invalid(format!(
                    "bundle file exceeds its byte limit: {}",
                    file.path
                )));
            }
            total_size = total_size
                .checked_add(file.size)
                .filter(|size| *size <= MAX_TOTAL_FILE_BYTES)
                .ok_or_else(|| invalid("bundle files exceed their total byte limit"))?;
        }

        if document.bundle.size == 0 || document.bundle.size > MAX_BUNDLE_BYTES {
            return Err(invalid("bundle size is outside its byte limit"));
        }
        let sha256 = decode_sha256("bundle", &document.bundle.sha256)?;
        let url = validate_https_url("bundle", &document.bundle.url)?;

        Ok(Self {
            identity: hex::encode(identity),
            platform: document.platform,
            arch: document.arch,
            obs_abi: document.obs_abi,
            implementation_id: document.implementation_id,
            url,
            sha256,
            size: document.bundle.size,
        })
    }

    pub fn identity(&self) -> &str {
        &self.identity
    }

    pub fn platform(&self) -> &str {
        &self.platform
    }

    pub fn arch(&self) -> &str {
        &self.arch
    }

    pub fn obs_abi(&self) -> &str {
        &self.obs_abi
    }

    pub fn implementation_id(&self) -> &str {
        &self.implementation_id
    }

    pub fn url(&self) -> &url::Url {
        &self.url
    }

    pub fn sha256(&self) -> &[u8; 32] {
        &self.sha256
    }

    pub fn size(&self) -> u64 {
        self.size
    }
}

fn validate_provenance(provenance: &ProvenanceDocument) -> Result<(), ObsBootstrapError> {
    for (label, value) in [
        ("OBS upstream commit", &provenance.obs.upstream_commit),
        ("OBS upstream tree", &provenance.obs.upstream_tree),
        ("OBS patch commit", &provenance.obs.patch_commit),
        ("OBS patched tree", &provenance.obs.patched_tree),
        ("libobs-rs commit", &provenance.libobs_rs.commit),
        ("libobs-rs tree", &provenance.libobs_rs.tree),
    ] {
        validate_git_hash(label, value)?;
    }
    validate_files(
        "generated bindings",
        &provenance.libobs_rs.generated_bindings,
    )?;
    validate_file("build recipe", &provenance.build.recipe)?;

    let builder_digest = provenance
        .build
        .builder_image
        .digest
        .strip_prefix("sha256:")
        .ok_or_else(|| invalid("builder image digest must use sha256"))?;
    decode_sha256("builder image", builder_digest)?;
    let Some((image_name, image_digest)) = provenance.build.builder_image.reference.split_once('@')
    else {
        return Err(invalid("builder image reference must contain its digest"));
    };
    if image_name.is_empty()
        || image_name.contains('@')
        || !image_name.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'.' | b'/' | b':' | b'_' | b'-')
        })
        || image_digest != provenance.build.builder_image.digest
    {
        return Err(invalid(
            "builder image reference must name one exact digest",
        ));
    }

    if provenance.build.dependencies.is_empty() {
        return Err(invalid("build dependencies must not be empty"));
    }
    let mut dependency_names = HashSet::new();
    for dependency in &provenance.build.dependencies {
        validate_identifier("dependency name", &dependency.name)?;
        if !dependency_names.insert(dependency.name.as_str()) {
            return Err(invalid(format!(
                "duplicate build dependency: {}",
                dependency.name
            )));
        }
        if dependency.material.size == 0 {
            return Err(invalid(format!(
                "dependency material size must be non-zero: {}",
                dependency.name
            )));
        }
        validate_https_url("dependency material", &dependency.material.url)?;
        decode_sha256("dependency material", &dependency.material.sha256)?;
        if let Some(vcs) = &dependency.vcs {
            validate_https_url("dependency repository", &vcs.repository)?;
            validate_git_hash("dependency commit", &vcs.commit)?;
            validate_git_hash("dependency tree", &vcs.tree)?;
        }
    }
    Ok(())
}

fn validate_files(label: &str, files: &[FileDocument]) -> Result<(), ObsBootstrapError> {
    if files.is_empty() {
        return Err(invalid(format!("{label} must not be empty")));
    }
    let mut paths = HashSet::new();
    for file in files {
        validate_file(label, file)?;
        if !paths.insert(file.path.as_str()) {
            return Err(invalid(format!("duplicate {label} path: {}", file.path)));
        }
    }
    Ok(())
}

fn validate_file(label: &str, file: &FileDocument) -> Result<(), ObsBootstrapError> {
    let path = Path::new(&file.path);
    if file.size == 0
        || file.path.is_empty()
        || file.path.len() > MAX_PATH_BYTES
        || file.path.contains('\\')
        || file.path.split('/').any(str::is_empty)
        || path.components().count() > MAX_PATH_COMPONENTS
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(invalid(format!("invalid {label}: {}", file.path)));
    }
    decode_sha256(label, &file.sha256)?;
    Ok(())
}

fn validate_windows_file_paths(files: &[FileDocument]) -> Result<(), ObsBootstrapError> {
    let mut folded_paths = HashSet::with_capacity(files.len());
    for file in files {
        let mut folded = String::with_capacity(file.path.len());
        for component in file.path.split('/') {
            if component.ends_with([' ', '.'])
                || !component.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric()
                        || matches!(byte, b' ' | b'.' | b'_' | b'(' | b')' | b'+' | b'-')
                })
            {
                return Err(invalid(format!(
                    "invalid Windows bundle file path: {}",
                    file.path
                )));
            }
            let stem = component
                .split_once('.')
                .map_or(component, |(stem, _)| stem)
                .to_ascii_lowercase();
            if matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
                || (stem.len() == 4
                    && matches!(&stem[..3], "com" | "lpt")
                    && matches!(stem.as_bytes()[3], b'1'..=b'9'))
            {
                return Err(invalid(format!(
                    "reserved Windows bundle file path: {}",
                    file.path
                )));
            }
            if !folded.is_empty() {
                folded.push('/');
            }
            folded.push_str(&component.to_ascii_lowercase());
        }
        if !folded_paths.insert(folded) {
            return Err(invalid(format!(
                "case-colliding Windows bundle file path: {}",
                file.path
            )));
        }
    }
    Ok(())
}

fn validate_identifier(label: &str, value: &str) -> Result<(), ObsBootstrapError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(invalid(format!("invalid {label}: {value}")));
    }
    Ok(())
}

fn validate_git_hash(label: &str, value: &str) -> Result<(), ObsBootstrapError> {
    if !matches!(value.len(), 40 | 64)
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid(format!(
            "{label} must be a canonical Git object ID"
        )));
    }
    Ok(())
}

fn decode_sha256(label: &str, value: &str) -> Result<[u8; 32], ObsBootstrapError> {
    let bytes =
        hex::decode(value).map_err(|error| invalid(format!("invalid {label} SHA-256: {error}")))?;
    if bytes.len() != 32 || hex::encode(&bytes) != value {
        return Err(invalid(format!(
            "{label} SHA-256 must be 64 lowercase hexadecimal characters"
        )));
    }
    Ok(bytes.try_into().expect("length checked above"))
}

fn validate_https_url(label: &str, value: &str) -> Result<url::Url, ObsBootstrapError> {
    let url =
        url::Url::parse(value).map_err(|error| invalid(format!("invalid {label} URL: {error}")))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid(format!(
            "{label} URL must be HTTPS without credentials, a query, or a fragment"
        )));
    }
    Ok(url)
}

fn invalid(message: impl Into<String>) -> ObsBootstrapError {
    ObsBootstrapError::InvalidFormatError(message.into())
}

#[derive(Debug, Clone)]
pub struct ObsBootstrapperOptions {
    pub(crate) manifest: ObsBundleManifest,
}

impl ObsBootstrapperOptions {
    pub fn new(manifest: ObsBundleManifest) -> Self {
        Self { manifest }
    }

    pub fn manifest(&self) -> &ObsBundleManifest {
        &self.manifest
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const GIT_1: &str = "1111111111111111111111111111111111111111";
    const GIT_2: &str = "2222222222222222222222222222222222222222";
    const GIT_3: &str = "3333333333333333333333333333333333333333";
    const GIT_4: &str = "4444444444444444444444444444444444444444";
    const GIT_5: &str = "5555555555555555555555555555555555555555";
    const GIT_6: &str = "6666666666666666666666666666666666666666";

    fn manifest(platform: &str, arch: &str, abi: &str, bundle_sha: &str) -> Vec<u8> {
        let obs_sha = hex::encode(Sha256::digest(b"obs"));
        format!(
            r#"{{"schema":"{MANIFEST_SCHEMA}","platform":"{platform}","arch":"{arch}","obs_abi":"{abi}","implementation_id":"crowd-cast-hybrid-mp4-v1","provenance":{{"obs":{{"upstream_commit":"{GIT_1}","upstream_tree":"{GIT_2}","patch_commit":"{GIT_3}","patched_tree":"{GIT_4}"}},"libobs_rs":{{"commit":"{GIT_5}","tree":"{GIT_6}","generated_bindings":[{{"path":"libobs/src/bindings_win.rs","size":1,"sha256":"{SHA}"}}]}},"build":{{"recipe":{{"path":"native/Dockerfile","size":1,"sha256":"{SHA}"}},"builder_image":{{"reference":"ghcr.io/example/builder@sha256:{SHA}","digest":"sha256:{SHA}"}},"dependencies":[{{"name":"ffmpeg","material":{{"url":"https://example.invalid/ffmpeg.tar.xz","size":1,"sha256":"{SHA}"}},"vcs":{{"repository":"https://github.com/FFmpeg/FFmpeg","commit":"{GIT_1}","tree":"{GIT_2}"}}}}]}}}},"files":[{{"path":"obs.dll","size":3,"sha256":"{obs_sha}"}}],"bundle":{{"url":"https://example.invalid/obs.7z","sha256":"{bundle_sha}","size":123}}}}"#
        )
        .into_bytes()
    }

    fn parse(
        bytes: &[u8],
        platform: &str,
        arch: &str,
    ) -> Result<ObsBundleManifest, ObsBootstrapError> {
        let identity = hex::encode(Sha256::digest(bytes));
        ObsBundleManifest::from_json_for_target(bytes, &identity, platform, arch)
    }

    #[test]
    fn exact_manifest_is_accepted() {
        let bytes = manifest("windows", "x86_64", "32.0.2", SHA);
        let parsed = parse(&bytes, "windows", "x86_64").unwrap();
        assert_eq!(parsed.obs_abi(), "32.0.2");
        assert_eq!(parsed.implementation_id(), "crowd-cast-hybrid-mp4-v1");
        assert_eq!(parsed.size(), 123);
        assert_eq!(parsed.identity(), hex::encode(Sha256::digest(&bytes)));
    }

    #[test]
    fn manifest_rejects_unanchored_bytes_target_or_abi_drift() {
        let bytes = manifest("windows", "x86_64", "32.0.2", SHA);
        assert!(ObsBundleManifest::from_json_for_target(&bytes, SHA, "windows", "x86_64").is_err());

        for bytes in [
            manifest("macos", "aarch64", "32.0.2", SHA),
            manifest("windows", "x86_64", "32.0.1", SHA),
        ] {
            assert!(parse(&bytes, "windows", "x86_64").is_err());
        }
    }

    #[test]
    fn manifest_rejects_ambiguous_digest_and_unknown_fields() {
        let uppercase = manifest("windows", "x86_64", "32.0.2", &SHA.to_uppercase());
        assert!(parse(&uppercase, "windows", "x86_64").is_err());

        let mut document: serde_json::Value =
            serde_json::from_slice(&manifest("windows", "x86_64", "32.0.2", SHA)).unwrap();
        document["extra"] = serde_json::Value::Bool(true);
        let bytes = serde_json::to_vec(&document).unwrap();
        assert!(parse(&bytes, "windows", "x86_64").is_err());
    }

    #[test]
    fn manifest_rejects_leakage_aliases_and_unbounded_resources() {
        let original = manifest("windows", "x86_64", "32.0.2", SHA);
        let mut document: serde_json::Value = serde_json::from_slice(&original).unwrap();
        document["bundle"]["url"] = serde_json::Value::String(
            "https://example.invalid/obs.7z?credential=secret".to_string(),
        );
        assert!(parse(&serde_json::to_vec(&document).unwrap(), "windows", "x86_64").is_err());

        for path in [
            "obs.dll:stream",
            "con.dll",
            "folder./obs.dll",
            "data//obs.dll",
            "dáta/obs.dll",
        ] {
            let mut document: serde_json::Value = serde_json::from_slice(&original).unwrap();
            document["files"][0]["path"] = serde_json::Value::String(path.to_string());
            assert!(parse(&serde_json::to_vec(&document).unwrap(), "windows", "x86_64").is_err());
        }

        let mut document: serde_json::Value = serde_json::from_slice(&original).unwrap();
        document["bundle"]["size"] = serde_json::Value::from(MAX_BUNDLE_BYTES + 1);
        assert!(parse(&serde_json::to_vec(&document).unwrap(), "windows", "x86_64").is_err());

        for size in [0, MAX_FILE_BYTES + 1] {
            let mut document: serde_json::Value = serde_json::from_slice(&original).unwrap();
            document["files"][0]["size"] = serde_json::Value::from(size);
            if size == 0 {
                document["files"][0]["sha256"] =
                    serde_json::Value::String(hex::encode(Sha256::digest([])));
            }
            assert!(parse(&serde_json::to_vec(&document).unwrap(), "windows", "x86_64").is_err());
        }

        let mut oversized = original;
        oversized.resize(MAX_MANIFEST_BYTES + 1, b' ');
        let identity = hex::encode(Sha256::digest(&oversized));
        assert!(
            ObsBundleManifest::from_json_for_target(&oversized, &identity, "windows", "x86_64")
                .is_err()
        );
    }

    #[test]
    fn every_runtime_entry_point_fails_before_consumer_state_is_read() {
        use std::{
            convert::Infallible,
            future::Future,
            sync::Arc,
            task::{Context, Poll, Wake, Waker},
        };

        #[derive(Debug)]
        struct PanicHandler;

        impl crate::status_handler::ObsBootstrapStatusHandler for PanicHandler {
            type Error = Infallible;

            fn handle_downloading(
                &mut self,
                _progress: f32,
                _message: String,
            ) -> Result<(), Self::Error> {
                panic!("disabled bootstrap called its status handler")
            }

            fn handle_extraction(
                &mut self,
                _progress: f32,
                _message: String,
            ) -> Result<(), Self::Error> {
                panic!("disabled bootstrap called its status handler")
            }
        }

        struct NoopWake;

        impl Wake for NoopWake {
            fn wake(self: Arc<Self>) {}
        }

        fn ready<F: Future>(future: F) -> F::Output {
            let waker = Waker::from(Arc::new(NoopWake));
            let mut context = Context::from_waker(&waker);
            let mut future = std::pin::pin!(future);
            match future.as_mut().poll(&mut context) {
                Poll::Ready(output) => output,
                Poll::Pending => panic!("disabled bootstrap performed asynchronous work"),
            }
        }

        let bytes = manifest("windows", "x86_64", "32.0.2", SHA);
        let parsed = parse(&bytes, "windows", "x86_64").unwrap();
        let options = ObsBootstrapperOptions::new(parsed);

        assert!(matches!(
            crate::ObsBootstrapper::is_valid_installation_with_options(&options),
            Err(ObsBootstrapError::RuntimeBootstrapDisabled)
        ));
        assert!(matches!(
            ready(crate::ObsBootstrapper::bootstrap(&options)),
            Err(ObsBootstrapError::RuntimeBootstrapDisabled)
        ));
        assert!(matches!(
            ready(crate::ObsBootstrapper::bootstrap_with_handler(
                &options,
                Box::new(PanicHandler)
            )),
            Err(ObsBootstrapError::RuntimeBootstrapDisabled)
        ));
    }
}
