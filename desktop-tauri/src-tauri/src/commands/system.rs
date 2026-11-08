//! Host environment checks and application lifecycle.

use crate::commands::session::blocking;
use crate::error::CommandResult;
use crate::state::AppState;
use serde::Serialize;
#[cfg(target_os = "linux")]
use std::path::Path;
use tauri::{AppHandle, State};

#[cfg(target_os = "linux")]
const VIRTUAL_CAMERA_DEVICE: &str = "/dev/video10";
#[cfg(not(target_os = "linux"))]
const VIRTUAL_CAMERA_DEVICE: &str = "Unavailable on this platform";

/// What is (and is not) available on this machine.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemCheck {
    pub adb_available: bool,
    pub virtual_camera_device: &'static str,
    pub virtual_camera_ready: bool,
    /// False on Windows/macOS, where v4l2loopback does not exist.
    pub virtual_camera_supported: bool,
}

#[tauri::command]
pub async fn system_check() -> CommandResult<SystemCheck> {
    let adb_available =
        blocking(|| Ok(camdroid_client::connection::adb::is_adb_available())).await?;
    Ok(SystemCheck {
        adb_available,
        virtual_camera_device: VIRTUAL_CAMERA_DEVICE,
        #[cfg(target_os = "linux")]
        virtual_camera_ready: Path::new(VIRTUAL_CAMERA_DEVICE).exists(),
        #[cfg(not(target_os = "linux"))]
        virtual_camera_ready: false,
        virtual_camera_supported: cfg!(target_os = "linux"),
    })
}

/// Stops the session cleanly (so the phone is told and the virtual camera is
/// released), saves settings, and exits.
#[tauri::command]
pub async fn quit_app(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    if let Some(session) = state.take_session() {
        let _ = blocking(move || session.stop()).await;
    }
    state.settings.flush();
    app.exit(0);
    Ok(())
}
