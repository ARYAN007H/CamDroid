// ADB (Android Debug Bridge) integration for USB connections.
//
// Detects connected Android devices, sets up TCP port forwarding over USB,
// and provides RAII cleanup of the forwarding rule on drop.

use anyhow::{bail, Context, Result};
use log::{debug, error, info, warn};
use std::process::Command;

/// Default port used by CamDroid for streaming.
const DEFAULT_PORT: u16 = 4747;

/// Manages ADB port forwarding for a USB-connected Android device.
///
/// When dropped, automatically removes the port forwarding rule.
pub struct AdbForward {
    serial: String,
    local_port: u16,
}

impl AdbForward {
    /// The localhost address to connect to after forwarding is established.
    pub fn local_addr(&self) -> String {
        format!("127.0.0.1:{}", self.local_port)
    }
}

impl Drop for AdbForward {
    fn drop(&mut self) {
        info!(
            "Removing ADB port forwarding for device {}",
            self.serial
        );
        let result = Command::new("adb")
            .args([
                "-s",
                &self.serial,
                "forward",
                "--remove",
                &format!("tcp:{}", self.local_port),
            ])
            .output();

        match result {
            Ok(output) if output.status.success() => {
                debug!("ADB forward removed successfully");
            }
            Ok(output) => {
                warn!(
                    "ADB forward removal returned non-zero: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            Err(e) => {
                error!("Failed to remove ADB forward: {}", e);
            }
        }
    }
}

/// Check whether the `adb` binary is available in PATH.
pub fn is_adb_available() -> bool {
    Command::new("adb")
        .arg("version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// List connected Android devices with USB debugging enabled.
///
/// Parses the output of `adb devices -l` to extract device serial numbers.
pub fn list_devices() -> Result<Vec<String>> {
    let output = Command::new("adb")
        .args(["devices", "-l"])
        .output()
        .context("Failed to run 'adb devices'. Is ADB installed and in PATH?")?;

    if !output.status.success() {
        bail!(
            "adb devices failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let devices: Vec<String> = stdout
        .lines()
        .skip(1) // Skip the "List of devices attached" header
        .filter(|line| !line.trim().is_empty())
        .filter(|line| line.contains("device") && !line.contains("unauthorized"))
        .filter_map(|line| {
            line.split_whitespace()
                .next()
                .map(|s| s.to_string())
        })
        .collect();

    Ok(devices)
}

/// Set up ADB TCP port forwarding for a specific device.
///
/// Forwards `local_port` on localhost to `remote_port` on the Android device.
/// Returns an `AdbForward` guard that automatically cleans up on drop.
pub fn forward_port(serial: &str, local_port: u16, remote_port: u16) -> Result<AdbForward> {
    info!(
        "Setting up ADB port forwarding: localhost:{} → {}:{}",
        local_port, serial, remote_port
    );

    let output = Command::new("adb")
        .args([
            "-s",
            serial,
            "forward",
            &format!("tcp:{}", local_port),
            &format!("tcp:{}", remote_port),
        ])
        .output()
        .context("Failed to run adb forward")?;

    if !output.status.success() {
        bail!(
            "adb forward failed for {}: {}",
            serial,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    info!(
        "Port forwarding established: localhost:{} → {}:{}",
        local_port, serial, remote_port
    );

    Ok(AdbForward {
        serial: serial.to_string(),
        local_port,
    })
}

/// Auto-detect a USB device and set up port forwarding.
///
/// Uses the first connected device found. Returns the `AdbForward` guard
/// and the local address to connect to (`127.0.0.1:4747`).
pub fn setup_usb_connection() -> Result<AdbForward> {
    if !is_adb_available() {
        bail!(
            "ADB is not installed or not in PATH.\n\
             Install it with: sudo apt install android-tools-adb"
        );
    }

    let devices = list_devices().context("Failed to list ADB devices")?;

    if devices.is_empty() {
        bail!(
            "No Android devices found via USB.\n\
             Make sure:\n\
             1. Your phone is connected via USB cable\n\
             2. USB Debugging is enabled (Settings → Developer Options → USB Debugging)\n\
             3. You have authorized the computer on the phone's prompt"
        );
    }

    if devices.len() > 1 {
        info!("Multiple devices found, using first: {}", devices[0]);
        for (i, dev) in devices.iter().enumerate() {
            info!("  [{}] {}", i, dev);
        }
    }

    let serial = &devices[0];
    info!("Using USB device: {}", serial);

    forward_port(serial, DEFAULT_PORT, DEFAULT_PORT)
}
