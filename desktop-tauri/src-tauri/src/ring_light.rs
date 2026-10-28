//! A software ring light.
//!
//! A borderless, transparent, always-on-top, click-through window covers the
//! monitor and paints a soft glowing frame around the edges, lighting the
//! user's face with the screen itself. The overlay window is a plain web page
//! (`ring-light.html`) that only listens for settings updates.

use crate::error::{CommandError, CommandResult};
use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder,
};

pub const WINDOW_LABEL: &str = "ring-light";
pub const UPDATE_EVENT: &str = "ring-light:update";

const BRIGHTNESS: std::ops::RangeInclusive<f32> = 0.1..=1.0;
const WARMTH: std::ops::RangeInclusive<f32> = -1.0..=1.0;
const THICKNESS: std::ops::RangeInclusive<f32> = 0.03..=0.25;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RingLightSettings {
    /// Light intensity, 0.1 (dim) to 1.0 (full).
    pub brightness: f32,
    /// Colour temperature: -1 is cool white, +1 is warm amber.
    pub warmth: f32,
    /// Ring width as a fraction of the screen's shorter side.
    pub thickness: f32,
}

impl Default for RingLightSettings {
    fn default() -> Self {
        Self {
            brightness: 0.85,
            warmth: 0.1,
            thickness: 0.09,
        }
    }
}

impl RingLightSettings {
    /// Pulls every value into its valid range; non-finite values reset to default.
    pub fn clamped(self) -> Self {
        let defaults = Self::default();
        let fix = |value: f32, range: &std::ops::RangeInclusive<f32>, fallback: f32| {
            if value.is_finite() {
                value.clamp(*range.start(), *range.end())
            } else {
                fallback
            }
        };
        Self {
            brightness: fix(self.brightness, &BRIGHTNESS, defaults.brightness),
            warmth: fix(self.warmth, &WARMTH, defaults.warmth),
            thickness: fix(self.thickness, &THICKNESS, defaults.thickness),
        }
    }
}

/// Shows the overlay (creating it on first use) with the given look.
pub fn show(app: &AppHandle, settings: RingLightSettings) -> CommandResult<()> {
    let window = match app.get_webview_window(WINDOW_LABEL) {
        Some(window) => window,
        None => {
            WebviewWindowBuilder::new(app, WINDOW_LABEL, WebviewUrl::App("ring-light.html".into()))
                .title("CamDroid Ring Light")
                .decorations(false)
                .transparent(true)
                .shadow(false)
                .resizable(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .focused(false)
                .visible(false)
                .build()
                .map_err(|error| {
                    CommandError::Window(format!("Could not create the ring light: {error}"))
                })?
        }
    };

    // Cover whichever monitor the studio window is on.
    if let Some(main) = app.get_webview_window("main") {
        if let Ok(Some(monitor)) = main.current_monitor() {
            let PhysicalPosition { x, y } = *monitor.position();
            let PhysicalSize { width, height } = *monitor.size();
            window.set_position(PhysicalPosition::new(x, y))?;
            window.set_size(PhysicalSize::new(width, height))?;
        }
    }

    // Input must pass straight through to whatever is underneath.
    window.set_ignore_cursor_events(true)?;
    window.show()?;
    app.emit_to(WINDOW_LABEL, UPDATE_EVENT, settings.clamped())?;
    Ok(())
}

pub fn hide(app: &AppHandle) -> CommandResult<()> {
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        window.hide()?;
    }
    Ok(())
}

pub fn is_visible(app: &AppHandle) -> bool {
    app.get_webview_window(WINDOW_LABEL)
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false)
}

/// Pushes new settings to a visible overlay. A hidden one picks them up when shown.
pub fn push_update(app: &AppHandle, settings: RingLightSettings) -> CommandResult<()> {
    if is_visible(app) {
        app.emit_to(WINDOW_LABEL, UPDATE_EVENT, settings.clamped())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_already_valid() {
        let defaults = RingLightSettings::default();
        assert_eq!(defaults, defaults.clamped());
    }

    #[test]
    fn values_are_clamped_into_range() {
        let wild = RingLightSettings {
            brightness: 5.0,
            warmth: -9.0,
            thickness: 0.0,
        };
        let fixed = wild.clamped();
        assert_eq!(fixed.brightness, 1.0);
        assert_eq!(fixed.warmth, -1.0);
        assert_eq!(fixed.thickness, 0.03);
    }

    #[test]
    fn non_finite_values_fall_back_to_defaults() {
        let broken = RingLightSettings {
            brightness: f32::NAN,
            warmth: f32::INFINITY,
            thickness: f32::NEG_INFINITY,
        };
        assert_eq!(broken.clamped(), RingLightSettings::default());
    }

    #[test]
    fn missing_fields_in_stored_json_use_defaults() {
        let parsed: RingLightSettings = serde_json::from_str(r#"{"brightness":0.5}"#).unwrap();
        assert_eq!(parsed.brightness, 0.5);
        assert_eq!(parsed.thickness, RingLightSettings::default().thickness);
    }
}
