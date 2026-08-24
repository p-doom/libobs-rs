#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

mod error;
#[cfg_attr(coverage_nightly, coverage(off))]
mod options;
pub mod status_handler;

pub use error::ObsBootstrapError;
pub use options::{ObsBootstrapperOptions, ObsBundleManifest};

use crate::status_handler::ObsBootstrapStatusHandler;

#[cfg(all(not(feature = "__ci"), target_os = "linux"))]
compile_error!("libobs-bootstrapper is not supported on Linux.");

/// Disabled runtime bootstrap entry points retained for migration diagnostics.
pub struct ObsBootstrapper {}

pub enum ObsBootstrapperResult {
    None,
    Restart,
}

impl ObsBootstrapper {
    /// Returns the pre-execution architecture error without inspecting an installation.
    pub fn is_valid_installation_with_options(
        _options: &ObsBootstrapperOptions,
    ) -> Result<bool, ObsBootstrapError> {
        Err(ObsBootstrapError::RuntimeBootstrapDisabled)
    }

    /// Returns the pre-execution architecture error without starting runtime bootstrap.
    pub async fn bootstrap(
        _options: &ObsBootstrapperOptions,
    ) -> Result<ObsBootstrapperResult, ObsBootstrapError> {
        Err(ObsBootstrapError::RuntimeBootstrapDisabled)
    }

    /// Returns the pre-execution architecture error without invoking the handler.
    pub async fn bootstrap_with_handler<E: Send + Sync + 'static + std::error::Error>(
        _options: &ObsBootstrapperOptions,
        _handler: Box<dyn ObsBootstrapStatusHandler<Error = E>>,
    ) -> Result<ObsBootstrapperResult, ObsBootstrapError> {
        Err(ObsBootstrapError::RuntimeBootstrapDisabled)
    }
}
