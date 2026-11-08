//! Host environment checks and application lifecycle.

use crate::commands::session::blocking;
use crate::error::CommandResult;
use crate::state::AppState;
use serde::Serialize;
use std::path::Path;
use tauri::{AppHandle, State};

const VIRTUAL_CAMERA_DEVICE: &str = "/dev/video10";

/// What is (and is not) available on this machine.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemCheck {
    pub adb_available: bool,
    pub virtual_camera_device: &'static str,
    pub virtual_camera_ready: bool,
}

#[tauri::command]
pub async fn system_check() -> CommandResult<SystemCheck> {
    let adb_available =
        blocking(|| Ok(camdroid_client::connection::adb::is_adb_available())).await?;
    Ok(SystemCheck {
        adb_available,
        virtual_camera_device: VIRTUAL_CAMERA_DEVICE,
        virtual_camera_ready: Path::new(VIRTUAL_CAMERA_DEVICE).exists(),
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
