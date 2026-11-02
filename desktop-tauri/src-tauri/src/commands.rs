// CamDroid Desktop GUI — Tauri Commands
//
// IPC command handlers that bridge the TypeScript frontend
// with the camdroid_client core library.

use camdroid_client::{
    control::brightness::DisplayBrightnessController, CamdroidSession, ConnectionMode,
    ControlCommand, DiscoveredDevice, SessionConfig, SessionStatus, SharedPreviewFrame,
};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::State;

/// Application state — holds the active streaming session and native display brightness controller.
pub struct AppState {
    pub session: Mutex<Option<CamdroidSession>>,
    pub brightness: DisplayBrightnessController,
}

/// A discovered device formatted for the frontend.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceInfo {
    pub name: String,
    pub ip: String,
    pub port: u16,
    pub model: String,
    pub codecs: String,
    pub resolution: String,
}

impl From<DiscoveredDevice> for DeviceInfo {
    fn from(d: DiscoveredDevice) -> Self {
        Self {
            name: d.name,
            ip: d.ip.to_string(),
            port: d.port,
            model: d.device_model,
            codecs: d.codecs,
            resolution: d.resolution,
        }
    }
}

/// Parameters for starting a stream connection.
#[derive(Debug, Deserialize)]
pub struct ConnectParams {
    pub mode: String,       // "wifi", "usb", or "discover"
    pub ip: Option<String>, // Required if mode == "wifi"
    pub port: Option<u16>,  // Required if mode == "wifi"
    pub codec: String,
    pub resolution: String,
    pub fps: u32,
    pub audio: bool,
}

// ── Tauri Commands ──

/// Discover CamDroid devices on the local network via mDNS + USB ADB.
#[tauri::command]
pub fn discover_devices(timeout: Option<u64>) -> Result<Vec<DeviceInfo>, String> {
    let timeout = timeout.unwrap_or(2);
    CamdroidSession::discover_devices(timeout)
        .map(|devices| devices.into_iter().map(DeviceInfo::from).collect())
        .map_err(|e| format!("{:#}", e))
}

/// Connect to a phone and start the streaming pipeline.
#[tauri::command]
pub fn connect_and_stream(
    state: State<'_, AppState>,
    params: ConnectParams,
) -> Result<SessionStatus, String> {
    // Drop any existing session first
    {
        let mut session_lock = state.session.lock().map_err(|e| e.to_string())?;
        if let Some(old) = session_lock.take() {
            let _ = old.stop();
        }
    }

    let mode = match params.mode.as_str() {
        "wifi" => {
            let ip = params.ip.ok_or("IP address required for WiFi mode")?;
            let port = params.port.unwrap_or(4747);
            ConnectionMode::Direct(format!("{}:{}", ip, port))
        }
        "usb" => ConnectionMode::Usb,
        "discover" => ConnectionMode::Discover,
        _ => return Err(format!("Unknown connection mode: {}", params.mode)),
    };

    let config = SessionConfig {
        codec: params.codec,
        resolution: params.resolution,
        fps: params.fps,
        audio: params.audio,
        ..SessionConfig::default()
    };

    let session = CamdroidSession::connect(mode, config).map_err(|e| format!("{:#}", e))?;
    let status = session.status();

    let mut session_lock = state.session.lock().map_err(|e| e.to_string())?;
    *session_lock = Some(session);

    Ok(status)
}

/// Send a camera control command to the connected phone.
#[tauri::command]
pub fn send_control(state: State<'_, AppState>, command: String) -> Result<(), String> {
    let session_lock = state.session.lock().map_err(|e| e.to_string())?;
    let session = session_lock
        .as_ref()
        .ok_or("Not connected — start a stream first")?;

    let cmd: ControlCommand = serde_json::from_str(&command)
        .map_err(|e| format!("Invalid control command: {}", e))?;

    session.send_control(&cmd).map_err(|e| format!("{:#}", e))
}

/// Get the current session status enriched with 1Hz phone telemetry.
#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> Result<SessionStatus, String> {
    let session_lock = state.session.lock().map_err(|e| e.to_string())?;
    let session = session_lock.as_ref().ok_or("Not connected")?;

    Ok(session.status())
}

/// Fetch the latest RGBA preview frame for the standalone desktop viewfinder.
#[tauri::command]
pub fn get_latest_frame(state: State<'_, AppState>) -> Result<Option<SharedPreviewFrame>, String> {
    let session_lock = state.session.lock().map_err(|e| e.to_string())?;
    let session = session_lock.as_ref().ok_or("Not connected")?;
    Ok(session.latest_frame())
}

/// Stop the current streaming session and disconnect.
#[tauri::command]
pub fn stop_stream(state: State<'_, AppState>) -> Result<(), String> {
    let mut session_lock = state.session.lock().map_err(|e| e.to_string())?;
    if let Some(session) = session_lock.take() {
        session.stop().map_err(|e| format!("{:#}", e))?;
    }
    Ok(())
}

/// Set zoom level on the phone camera (supports 0.5x Ultra-Wide to 10x Telephoto).
#[tauri::command]
pub fn set_zoom(state: State<'_, AppState>, value: f32) -> Result<(), String> {
    let session_lock = state.session.lock().map_err(|e| e.to_string())?;
    let session = session_lock.as_ref().ok_or("Not connected")?;
    session
        .send_control(&ControlCommand::SetZoom { value })
        .map_err(|e| format!("{:#}", e))
}

/// Set focus mode (auto/manual) and distance.
#[tauri::command]
pub fn set_focus(
    state: State<'_, AppState>,
    mode: String,
    distance: Option<f32>,
) -> Result<(), String> {
    let session_lock = state.session.lock().map_err(|e| e.to_string())?;
    let session = session_lock.as_ref().ok_or("Not connected")?;
    session
        .send_control(&ControlCommand::SetFocus {
            mode,
            distance,
            x: None,
            y: None,
        })
        .map_err(|e| format!("{:#}", e))
}

/// Trigger tap-to-focus & AE metering at normalized viewfinder coordinates (x, y in 0.0..1.0).
#[tauri::command]
pub fn set_tap_focus(state: State<'_, AppState>, x: f32, y: f32) -> Result<(), String> {
    let session_lock = state.session.lock().map_err(|e| e.to_string())?;
    let session = session_lock.as_ref().ok_or("Not connected")?;
    session
        .send_control(&ControlCommand::SetFocus {
            mode: "tap".to_string(),
            distance: None,
            x: Some(x.clamp(0.0, 1.0)),
            y: Some(y.clamp(0.0, 1.0)),
        })
        .map_err(|e| format!("{:#}", e))
}

/// Set exposure compensation.
#[tauri::command]
pub fn set_exposure(state: State<'_, AppState>, compensation: i32) -> Result<(), String> {
    let session_lock = state.session.lock().map_err(|e| e.to_string())?;
    let session = session_lock.as_ref().ok_or("Not connected")?;
    session
        .send_control(&ControlCommand::SetExposure { compensation })
        .map_err(|e| format!("{:#}", e))
}

/// Set manual ISO and shutter speed (ns).
#[tauri::command]
pub fn set_manual_exposure(
    state: State<'_, AppState>,
    iso: Option<i32>,
    shutter_ns: Option<i64>,
) -> Result<(), String> {
    let session_lock = state.session.lock().map_err(|e| e.to_string())?;
    let session = session_lock.as_ref().ok_or("Not connected")?;
    session
        .send_control(&ControlCommand::SetManualExposure { iso, shutter_ns })
        .map_err(|e| format!("{:#}", e))
}

/// Set white balance mode.
#[tauri::command]
pub fn set_white_balance(state: State<'_, AppState>, mode: String) -> Result<(), String> {
    let session_lock = state.session.lock().map_err(|e| e.to_string())?;
    let session = session_lock.as_ref().ok_or("Not connected")?;
    session
        .send_control(&ControlCommand::SetWhiteBalance { mode })
        .map_err(|e| format!("{:#}", e))
}

/// Toggle flash/torch.
#[tauri::command]
pub fn set_flash(state: State<'_, AppState>, enabled: bool) -> Result<(), String> {
    let session_lock = state.session.lock().map_err(|e| e.to_string())?;
    let session = session_lock.as_ref().ok_or("Not connected")?;
    session
        .send_control(&ControlCommand::SetFlash { enabled })
        .map_err(|e| format!("{:#}", e))
}

/// Toggle horizontal mirror.
#[tauri::command]
pub fn set_mirror(state: State<'_, AppState>, enabled: bool) -> Result<(), String> {
    let session_lock = state.session.lock().map_err(|e| e.to_string())?;
    let session = session_lock.as_ref().ok_or("Not connected")?;
    session.set_mirror(enabled).map_err(|e| format!("{:#}", e))
}

/// Set dynamic encoder bitrate (bps).
#[tauri::command]
pub fn set_bitrate(state: State<'_, AppState>, bitrate: u64) -> Result<(), String> {
    let session_lock = state.session.lock().map_err(|e| e.to_string())?;
    let session = session_lock.as_ref().ok_or("Not connected")?;
    session
        .send_control(&ControlCommand::SetBitrate { value: bitrate })
        .map_err(|e| format!("{:#}", e))
}

/// Request an immediate IDR keyframe from the phone camera encoder.
#[tauri::command]
pub fn request_keyframe(state: State<'_, AppState>) -> Result<(), String> {
    let session_lock = state.session.lock().map_err(|e| e.to_string())?;
    let session = session_lock.as_ref().ok_or("Not connected")?;
    session.request_keyframe().map_err(|e| format!("{:#}", e))
}

/// Switch between front and rear cameras.
#[tauri::command]
pub fn switch_camera(state: State<'_, AppState>) -> Result<(), String> {
    let session_lock = state.session.lock().map_err(|e| e.to_string())?;
    let session = session_lock.as_ref().ok_or("Not connected")?;
    session
        .send_control(&ControlCommand::SwitchCamera)
        .map_err(|e| format!("{:#}", e))
}

/// Enable or disable the Virtual Ring Light (Edge Light) hardware display brightness boost
/// and synchronize phone camera sensor exposure compensation.
#[tauri::command]
pub fn set_ring_light_brightness(
    state: State<'_, AppState>,
    enabled: bool,
    intensity: f32,
) -> Result<u32, String> {
    let target_pct = state
        .brightness
        .set_ring_light_boost(enabled, intensity)
        .map_err(|e| format!("{:#}", e))?;

    // Also send an exposure lift to the connected phone camera if a session is active
    if let Ok(session_lock) = state.session.lock() {
        if let Some(ref session) = *session_lock {
            let ev_boost = if enabled {
                ((intensity.clamp(0.15, 1.0) * 2.0).round() as i32).clamp(1, 2)
            } else {
                0
            };
            let _ = session.send_control(&ControlCommand::SetExposure {
                compensation: ev_boost,
            });
        }
    }

    Ok(target_pct)
}
