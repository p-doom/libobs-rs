# How libobs-rs Works

`libobs-rs` is a set of crates that provide safe Rust bindings to the OBS Studio C API (`libobs`).

## Architecture

### 1. `libobs` (sys crate)
- Raw FFI bindings to `obs.dll`.
- Generated using `bindgen`.
- Unsafe and difficult to use directly.

### 2. `libobs-wrapper`
- Safe wrapper around `libobs`.
- Handles memory management, threading, and context.
- Provides a more Rust-idiomatic API.

### 3. `libobs-simple`
- High-level abstraction.
- Simplifies common tasks like recording and streaming.

### 4. Native runtime package
- Contains the exact OBS libraries, plugins, data, and helper binaries.
- Is authenticated and installed before the application process starts.

## Data Flow

1. **Initialization**: The operating-system loader resolves the packaged OBS runtime, then `libobs-wrapper` initializes the core context.
2. **Configuration**: You create scenes, sources, and encoders using `libobs-simple` or `libobs-wrapper`.
3. **Execution**: `libobs` runs the video/audio pipeline in background threads.
4. **Output**: Encoded data is written to file or stream.
