# libobs-bootstrapper

Runtime OBS installation is disabled. Every `ObsBootstrapper` cache and
bootstrap entry point returns `ObsBootstrapError::RuntimeBootstrapDisabled`
before reading consumer state or starting network work.

Version 0.3 is a breaking security release. Consumers must remove runtime
bootstrap configuration and package OBS before process startup.

On Windows, applications linked to `obs.dll` load it before Rust `main`, so an
in-process receipt or hash check cannot authorize the library before it is
executed. The complete OBS runtime must instead be authenticated before process
startup by a signed installer or a launcher that does not import OBS. On macOS,
the complete framework, plugins, data, and helper binaries must be included in
the signed application bundle.

`ObsBundleManifest` and `ObsBootstrapperOptions` remain available while
consumers migrate away from runtime bootstrap. They do not enable installation.
