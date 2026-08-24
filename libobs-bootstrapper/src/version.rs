use std::path::Path;

use crate::error::ObsBootstrapError;
use libloading::Library;

pub type GetVersionFunc = unsafe extern "C" fn() -> u32;

pub fn get_installed_version(obs_dll: &Path) -> Result<Option<String>, ObsBootstrapError> {
    // The obs.dll should always exist
    let dll_exists = obs_dll.exists() && obs_dll.is_file();
    if !dll_exists {
        log::trace!("obs.dll does not exist at {}", obs_dll.display());
        return Ok(None);
    }

    log::trace!("Getting obs.dll version string");
    unsafe {
        let lib = Library::new(obs_dll)
            .map_err(|e| ObsBootstrapError::LibLoadingError("Opening library", e))?;
        let get_version: libloading::Symbol<GetVersionFunc> = lib
            .get(b"obs_get_version")
            .map_err(|e| ObsBootstrapError::LibLoadingError("Getting version string", e))?;
        let version = get_version();

        if version == 0 {
            lib.close()
                .map_err(|e| ObsBootstrapError::LibLoadingError("Closing lib", e))?;
            log::trace!("obs.dll does not have a version string");
            return Ok(None);
        }

        lib.close()
            .map_err(|e| ObsBootstrapError::LibLoadingError("Closing lib", e))?;

        let version_str = format!(
            "{}.{}.{}",
            (version >> 24) & 0xFF,
            (version >> 16) & 0xFF,
            version & 0xFFFF
        );

        Ok(Some(version_str))
    }
}
