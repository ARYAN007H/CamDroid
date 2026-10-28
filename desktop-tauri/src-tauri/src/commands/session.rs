//! Connecting to a phone and managing the live session.

use crate::error::{CommandError, CommandResult};
use crate::preview::PreviewPump;
use crate::settings::{StreamPreferences, WifiTarget};
use crate::state::AppState;
use camdroid_client::{
    CamdroidSession, ConnectionMode, DiscoveredDevice, SessionConfig, SessionStatus,
};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::State;

/// Runs blocking work off the UI thread and flattens the error layers.
pub(crate) async fn blocking<T: Send + 'static>(
    job: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> CommandResult<T> {
    tauri::async_runtime::spawn_blocking(job)
        .await
        .map_err(|error| CommandError::Window(format!("A background task failed: {error}")))?
        .map_err(CommandError::from)
}

/// A phone found on the local network.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub name: String,
    pub ip: String,
    pub port: u16,
    pub model: String,
    pub codecs: String,
    pub resolution: String,
}

impl From<DiscoveredDevice> for DeviceInfo {
    fn from(device: DiscoveredDevice) -> Self {
        Self {
            name: device.name,
            ip: device.ip.to_string(),
            port: device.port,
            model: device.device_model,
            codecs: device.codecs,
            resolution: device.resolution,
        }
    }
}

/// How to reach the phone.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ConnectTarget {
    Discover,
    Usb,
    Wifi { host: String, port: u16 },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectRequest {
    pub target: ConnectTarget,
    pub stream: StreamPreferences,
}

/// Builds a `host:port` string, bracketing IPv6 literals.
pub fn direct_address(host: &str, port: u16) -> CommandResult<String> {
    let host = host.trim();
    if host.is_empty() {
        return Err(CommandError::Invalid(
            "Enter the phone's IP address.".into(),
        ));
    }
    if port == 0 {
        return Err(CommandError::Invalid(
            "The port must be between 1 and 65535.".into(),
        ));
    }
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V6(address)) => Ok(format!("[{address}]:{port}")),
        Ok(IpAddr::V4(address)) => Ok(format!("{address}:{port}")),
        Err(_) => {
            let looks_like_hostname = host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
                && !host.starts_with(['.', '-'])
                && !host.ends_with(['.', '-']);
            if looks_like_hostname {
                Ok(format!("{host}:{port}"))
            } else {
                Err(CommandError::Invalid(format!(
                    "'{host}' is not a valid address."
                )))
            }
        }
    }
}

impl ConnectTarget {
    fn to_mode(&self) -> CommandResult<ConnectionMode> {
        Ok(match self {
            Self::Discover => ConnectionMode::Discover,
            Self::Usb => ConnectionMode::Usb,
            Self::Wifi { host, port } => ConnectionMode::Direct(direct_address(host, *port)?),
        })
    }
}

#[tauri::command]
pub async fn discover_devices(timeout_seconds: Option<u64>) -> CommandResult<Vec<DeviceInfo>> {
    let timeout = timeout_seconds.unwrap_or(5).clamp(1, 30);
    let devices = blocking(move || CamdroidSession::discover_devices(timeout)).await?;
    Ok(devices.into_iter().map(DeviceInfo::from).collect())
}

#[tauri::command]
pub async fn connect(
    state: State<'_, AppState>,
    request: ConnectRequest,
) -> CommandResult<SessionStatus> {
    let _attempt = state.begin_connect()?;
    request.stream.validate().map_err(CommandError::Invalid)?;
    let mode = request.target.to_mode()?;

    if let Some(previous) = state.take_session() {
        blocking(move || previous.stop()).await?;
    }

    let settings = state.settings.snapshot();
    let config = SessionConfig {
        codec: request.stream.codec.as_str().to_string(),
        resolution: request.stream.resolution.as_str().to_string(),
        fps: request.stream.fps,
        audio: request.stream.audio,
        output: settings.output_mode,
        ..SessionConfig::default()
    };
    let session = blocking(move || CamdroidSession::connect(mode, config)).await?;

    // A new session starts from the user's saved look and framing.
    session.filters().set(settings.filter)?;
    session.output().set_mirrored(settings.mirror);
    let status = session.status();
    state.install_session(session);

    state.settings.update(|stored| {
        stored.stream = request.stream;
        if let ConnectTarget::Wifi { host, port } = &request.target {
            stored.wifi = WifiTarget {
                host: host.trim().to_string(),
                port: *port,
            };
        }
    });
    Ok(status)
}

/// The current session, or `None` if there is none.
///
/// A session whose connection has dropped is reported once, with the reason,
/// and released at the same time so its resources are freed promptly.
#[tauri::command]
pub async fn session_status(state: State<'_, AppState>) -> CommandResult<Option<SessionStatus>> {
    let Some((alive, status)) =
        state.try_with_session(|session| (session.is_alive(), session.status()))
    else {
        return Ok(None);
    };
    if !alive {
        if let Some(ended) = state.take_session() {
            let _ = blocking(move || ended.stop()).await;
        }
    }
    Ok(Some(status))
}

#[tauri::command]
pub async fn disconnect(state: State<'_, AppState>) -> CommandResult<()> {
    if let Some(session) = state.take_session() {
        blocking(move || session.stop()).await?;
    }
    Ok(())
}

/// Begins streaming rendered preview frames (JPEG bytes) into `on_frame`.
#[tauri::command]
pub async fn start_preview(
    state: State<'_, AppState>,
    on_frame: Channel<InvokeResponseBody>,
) -> CommandResult<()> {
    let hub = state.with_session(|session| session.preview())?;
    state.start_preview(PreviewPump::start(hub, on_frame));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv4_and_hostnames_become_host_port_pairs() {
        assert_eq!(
            direct_address("192.168.1.67", 4747).unwrap(),
            "192.168.1.67:4747"
        );
        assert_eq!(
            direct_address("  pixel.local ", 4747).unwrap(),
            "pixel.local:4747"
        );
    }

    #[test]
    fn ipv6_literals_are_bracketed() {
        assert_eq!(direct_address("fe80::1", 4747).unwrap(), "[fe80::1]:4747");
    }

    #[test]
    fn unusable_addresses_are_rejected_with_a_readable_reason() {
        for host in [
            "",
            "   ",
            "192.168.1.1/24",
            "bad host",
            "-leading.dash",
            "http://phone",
        ] {
            let error = direct_address(host, 4747).unwrap_err().to_string();
            assert!(!error.is_empty(), "{host:?} should be rejected");
        }
        assert!(direct_address("192.168.1.2", 0).is_err());
    }

    #[test]
    fn connect_targets_deserialise_from_the_frontend_shape() {
        let wifi: ConnectTarget =
            serde_json::from_str(r#"{"mode":"wifi","host":"10.0.0.5","port":4747}"#).unwrap();
        assert_eq!(
            wifi,
            ConnectTarget::Wifi {
                host: "10.0.0.5".into(),
                port: 4747
            }
        );
        assert_eq!(
            serde_json::from_str::<ConnectTarget>(r#"{"mode":"usb"}"#).unwrap(),
            ConnectTarget::Usb
        );
        assert!(serde_json::from_str::<ConnectTarget>(r#"{"mode":"carrier_pigeon"}"#).is_err());
    }

    #[test]
    fn a_wifi_request_maps_to_a_direct_connection() {
        let target = ConnectTarget::Wifi {
            host: "10.0.0.5".into(),
            port: 4747,
        };
        match target.to_mode().unwrap() {
            ConnectionMode::Direct(address) => assert_eq!(address, "10.0.0.5:4747"),
            other => panic!("unexpected {other:?}"),
        }
    }
}
