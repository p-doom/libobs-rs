# libobs-bootstrapper

[![Crates.io](https://img.shields.io/crates/v/libobs-bootstrapper.svg)](https://crates.io/crates/libobs-bootstrapper)
[![Documentation](https://docs.rs/libobs-bootstrapper/badge.svg)](https://docs.rs/libobs-bootstrapper)

A utility crate for automatically downloading and installing OBS (Open Broadcaster Software) Studio binaries at runtime. This crate is part of the libobs-rs ecosystem and is designed to make distributing OBS-based applications easier by handling the setup of OBS binaries.

Note: This crate currently supports Windows and MacOS platforms. Refer to the libobs-wrapper documentation for Linux setup instructions [here](../libobs-wrapper/README.md).

## Features

- **Exact OBS Download**: Downloads the single bundle named by a caller-supplied manifest
- **Cross-Platform**: Supports Windows (7z), macOS (DMG)
- **Progress Tracking**: Built-in progress reporting for downloads and extraction
- **Provenance Enforcement**: Requires an application-anchored manifest identity and verifies the archive and staged runtime files
- **Custom Status Handlers**: Flexible progress reporting via custom handlers
- **Async Support**: Built on Tokio for async operations
- **Error Handling**: Comprehensive error types for reliable error handling

## Usage

Add the crate to your dependencies:

```toml
[dependencies]
libobs-bootstrapper = "0.2.0"
```

### Basic Example

Here's a simple example using the default console handler:

```rust
use libobs_bootstrapper::{
    ObsBootstrapper, ObsBootstrapperOptions, ObsBootstrapperResult, ObsBundleManifest
};
use libobs_wrapper::{context::ObsContext, utils::StartupInfo};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    const MANIFEST: &[u8] = include_bytes!("../obs-bundle-manifest.json");
    const MANIFEST_SHA256: &str = env!("LIBOBS_BUNDLE_MANIFEST_SHA256");
    let manifest = ObsBundleManifest::from_json(MANIFEST, MANIFEST_SHA256)?;
    let options = ObsBootstrapperOptions::new(manifest);
    
    // Run bootstrap with default console handler
    match ObsBootstrapper::bootstrap(&options).await? {
        ObsBootstrapperResult::None => {
            println!("OBS is already installed and up to date!");
        }
        ObsBootstrapperResult::Restart => {
            // This only happens on Windows - macOS moves files immediately
            println!("OBS has been updated. Restarting application...");
            std::process::exit(0);
        }
    }

    let context = ObsContext::new(StartupInfo::default()).unwrap();

    println!("Done");
    // Use the context here
    // For example creating new obs data
    context.data().unwrap();
    
    Ok(())
}
```

### Custom Progress Handler

You can implement your own progress handler for custom UI integration:

```rust
use indicatif::{ProgressBar, ProgressStyle};
use libobs_bootstrapper::status_handler::ObsBootstrapStatusHandler;
use std::{sync::Arc, time::Duration};

#[derive(Debug, Clone)]
struct CustomProgressHandler(Arc<ProgressBar>);

impl CustomProgressHandler {
    pub fn new() -> Self {
        let bar = ProgressBar::new(200).with_style(
            ProgressStyle::default_bar()
                .template("{msg}\n{wide_bar} {pos}/{len}")
                .unwrap(),
        );

        bar.set_message("Initializing bootstrapper...");
        Self(Arc::new(bar))
    }
}

impl ObsBootstrapStatusHandler for CustomProgressHandler {
    fn handle_downloading(&mut self, prog: f32, msg: String) -> anyhow::Result<()> {
        self.0.set_message(msg);
        self.0.set_position((prog * 100.0) as u64);
        Ok(())
    }

    fn handle_extraction(&mut self, prog: f32, msg: String) -> anyhow::Result<()> {
        self.0.set_message(msg);
        self.0.set_position(100 + (prog * 100.0) as u64);
        Ok(())
    }
}
```

### Setup Steps

1. **Windows only:** Keep the default `install_dummy_dll` feature enabled. It places
   the crate's vendored placeholder in the build output; the verified bundle replaces it.

2. Embed exactly one `libobs-bootstrap-manifest-v1` document and its independently
   recorded SHA-256, then call `ObsBootstrapper::bootstrap()` before loading OBS. The
   strict document binds `platform`, `arch`, `obs_abi`, `implementation_id`, the exact
   OBS and libobs-rs commits and trees, generated bindings, build recipe, immutable
   builder image, dependency source materials, shipped file identities, and one bundle
   `url`/`size`/`sha256`. Unknown fields and non-canonical identities are rejected.

3. Handle the result based on platform:
   - **Windows**: If `ObsBootstrapperResult::Restart` is returned, exit the application and the updater will restart it automatically
   - **macOS**: Bootstrap completes immediately, no restart needed (`ObsBootstrapperResult::None` is returned after successful installation)

### Platform-Specific Notes

- **Windows**: Downloads and extracts the manifest's verified 7z archive
  - Requires application restart to complete installation
  - An updater script moves files from `obs_new/` to the executable directory after restart
- **macOS**: Downloads and verifies the manifest's exact DMG before extracting frameworks, plugins, and data
  - **No restart required** - files are moved immediately after extraction
  - Dylibs can be replaced while the application is running

### Advanced Options

The manifest is required. The only path option is an explicit install root:

```rust
let manifest = ObsBundleManifest::from_json(&manifest_bytes, expected_manifest_sha256)?;
let options = ObsBootstrapperOptions::new(manifest)
    .set_install_dir("/tmp/my-obs-runtime");    // Custom install root
```

## Error Handling

The crate provides the `ObsBootstrapError` enum for error handling:

- `GeneralError`: Generic bootstrapper errors
- `DownloadError`: Issues during OBS binary download
- `ExtractError`: Problems extracting downloaded files

## License

This project is licensed under the MIT License - see the LICENSE file for details.
