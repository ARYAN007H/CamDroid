//! Where finished frames go.
//!
//! The app can show video on its own ("standalone") or additionally publish
//! it as a system webcam through v4l2loopback. The mode is switchable while a
//! session is live, so it is a shared atomic rather than a start-up option.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputMode {
    /// Frames are shown in the app only; no kernel device is touched.
    #[default]
    Standalone,
    /// Frames are also written to the v4l2loopback device for OBS, Zoom, etc.
    VirtualCamera,
}

impl OutputMode {
    fn to_bits(self) -> u8 {
        match self {
            Self::Standalone => 0,
            Self::VirtualCamera => 1,
        }
    }

    fn from_bits(bits: u8) -> Self {
        match bits {
            1 => Self::VirtualCamera,
            _ => Self::Standalone,
        }
    }
}

/// Live, lock-free handle to the active [`OutputMode`].
#[derive(Debug, Clone)]
pub struct OutputController(Arc<AtomicU8>);

impl OutputController {
    pub fn new(mode: OutputMode) -> Self {
        Self(Arc::new(AtomicU8::new(mode.to_bits())))
    }

    pub fn mode(&self) -> OutputMode {
        OutputMode::from_bits(self.0.load(Ordering::Relaxed))
    }

    pub fn set(&self, mode: OutputMode) {
        self.0.store(mode.to_bits(), Ordering::Relaxed);
    }
}

impl Default for OutputController {
    fn default() -> Self {
        Self::new(OutputMode::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_changes_are_visible_through_every_clone() {
        let controller = OutputController::default();
        let observer = controller.clone();
        assert_eq!(observer.mode(), OutputMode::Standalone);

        controller.set(OutputMode::VirtualCamera);
        assert_eq!(observer.mode(), OutputMode::VirtualCamera);
    }

    #[test]
    fn mode_serialises_as_snake_case_for_the_frontend() {
        let json = serde_json::to_string(&OutputMode::VirtualCamera).unwrap();
        assert_eq!(json, "\"virtual_camera\"");
        let parsed: OutputMode = serde_json::from_str("\"standalone\"").unwrap();
        assert_eq!(parsed, OutputMode::Standalone);
    }
}
