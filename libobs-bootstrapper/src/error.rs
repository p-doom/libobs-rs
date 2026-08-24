#[derive(Debug)]
pub enum ObsBootstrapError {
    RuntimeBootstrapDisabled,
    InvalidFormatError(String),
}

impl std::fmt::Display for ObsBootstrapError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RuntimeBootstrapDisabled => formatter.write_str(
                "Runtime OBS bootstrap is disabled because it cannot authenticate OBS before process startup",
            ),
            Self::InvalidFormatError(error) => {
                write!(formatter, "Invalid format error: {error:?}")
            }
        }
    }
}

impl std::error::Error for ObsBootstrapError {}
