# OBS Runtime Packaging

Runtime OBS bootstrap is disabled. An in-process hash or receipt check cannot
authorize a Windows `obs.dll` that the operating-system loader executes before
Rust `main`.

For development, use [`cargo-obs-build`](../cargo-obs-build/README.md) to build
the exact native runtime before starting the application.

Production distributions must package the complete reviewed runtime before
process startup:

- Windows installers must contain the exact OBS libraries, plugins, data, and
  helpers inside the signed installer and install them under protected ACLs.
- macOS application bundles must contain the exact framework, plugins, data,
  and helpers before the bundle is signed and notarized.

Do not download, replace, or accept OBS native files from inside the application
process.
