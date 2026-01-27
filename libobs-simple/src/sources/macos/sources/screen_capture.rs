//! Screen capture source for macOS
//!
//! Bindings to OBS's `screen_capture` source which internally uses ScreenCaptureKit.
//! This source captures the entire screen, a specific application, or a specific window.
//!
//! # Capture Types
//!
//! The macOS screen capture source supports three capture modes:
//! - **Display** (type=0): Captures an entire display
//! - **Window** (type=1): Captures a specific window by window ID
//! - **Application** (type=2): Captures all visible windows of a specific application
//!
//! # Example
//!
//! ```ignore
//! // Application capture
//! let source = context
//!     .source_builder::<ScreenCaptureSourceBuilder, _>("app_capture")?
//!     .set_capture_type(ScreenCaptureType::Application as i64)
//!     .set_application("com.apple.Safari")
//!     .set_display_uuid(&display_uuid)
//!     .set_show_cursor(true)
//!     .set_hide_obs(true)
//!     .add_to_scene(&mut scene)?;
//! ```

use libobs_simple_macro::obs_object_impl;
use libobs_wrapper::data::ObsObjectBuilder;
use libobs_wrapper::sources::ObsSourceRef;

use crate::sources::macro_helper::define_object_manager;

/// Screen capture type for macOS ScreenCaptureKit source
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(i64)]
pub enum ScreenCaptureType {
    /// Capture an entire display
    #[default]
    Display = 0,
    /// Capture a specific window
    Window = 1,
    /// Capture all visible windows of a specific application
    Application = 2,
}

impl ScreenCaptureType {
    /// Convert to i64 for OBS property setting
    pub fn as_i64(self) -> i64 {
        self as i64
    }
}

define_object_manager!(
    /// Builder for the screen capture source on macOS.
    ///
    /// Supports three capture modes via `capture_type`:
    /// - Display capture (type=0): Captures entire display specified by `display_uuid`
    /// - Window capture (type=1): Captures specific window specified by `window`
    /// - Application capture (type=2): Captures all windows of app specified by `application`
    #[derive(Debug)]
    struct ScreenCaptureSource("screen_capture") for ObsSourceRef {
        #[obs_property(type_t = "int", settings_key = "type")]
        /// Capture type: 0=Display, 1=Window, 2=Application
        capture_type: i64,

        #[obs_property(type_t = "string", settings_key = "display_uuid")]
        /// UUID of the display to capture (for display and application capture modes)
        display_uuid: String,

        #[obs_property(type_t = "string", settings_key = "application")]
        /// Bundle identifier of the application to capture (e.g., "com.apple.Safari")
        /// Used when capture_type is Application (2)
        application: String,

        #[obs_property(type_t = "int", settings_key = "window")]
        /// Window ID to capture (used when capture_type is Window)
        window: i64,

        #[obs_property(type_t = "int", settings_key = "display")]
        /// The display ID to capture (legacy, prefer display_uuid)
        display: i64,

        #[obs_property(type_t = "bool")]
        /// Whether to show the cursor in the capture
        show_cursor: bool,

        #[obs_property(type_t = "bool")]
        /// Whether to capture audio (macOS 13+)
        audio_capture: bool,

        #[obs_property(type_t = "bool")]
        /// Whether to hide OBS windows from capture
        hide_obs: bool,

        #[obs_property(type_t = "bool")]
        /// Whether to show hidden windows in the capture
        show_hidden_windows: bool,

        #[obs_property(type_t = "bool")]
        /// Whether to show windows with empty names
        show_empty_names: bool,
    }
);

#[obs_object_impl]
impl ScreenCaptureSource {
    // Helper methods can be added here
}

impl libobs_wrapper::sources::ObsSourceBuilder for ScreenCaptureSourceBuilder {
    fn add_to_scene(
        self,
        scene: &mut libobs_wrapper::scenes::ObsSceneRef,
    ) -> Result<libobs_wrapper::sources::ObsSourceRef, libobs_wrapper::utils::ObsError>
    where
        Self: Sized,
    {
        let source = self.build()?;
        scene.add_source(source)
    }
}
