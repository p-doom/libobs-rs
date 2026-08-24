use libobs_bootstrapper::{ObsBootstrapperOptions, ObsBundleManifest};

pub fn options() -> anyhow::Result<ObsBootstrapperOptions> {
    let path = std::env::var_os("LIBOBS_BUNDLE_MANIFEST")
        .ok_or_else(|| anyhow::anyhow!("LIBOBS_BUNDLE_MANIFEST is required"))?;
    let identity = std::env::var("LIBOBS_BUNDLE_MANIFEST_SHA256")
        .map_err(|_| anyhow::anyhow!("LIBOBS_BUNDLE_MANIFEST_SHA256 is required"))?;
    let bytes = std::fs::read(path)?;
    Ok(ObsBootstrapperOptions::new(ObsBundleManifest::from_json(
        &bytes, &identity,
    )?))
}
