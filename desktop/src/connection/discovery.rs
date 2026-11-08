// mDNS service discovery for finding CamDroid phones on the local network.
//
// Browses for `_camdroid._tcp.local.` services and returns discovered devices
// with their IP addresses, ports, and capability metadata from TXT records.

use anyhow::{Context, Result};
use log::{debug, info, warn};
use std::net::IpAddr;
use std::time::Duration;

/// Information about a discovered CamDroid device on the network.
#[derive(Debug, Clone)]
pub struct DiscoveredDevice {
    pub name: String,
    pub ip: IpAddr,
    pub port: u16,
    pub version: String,
    pub codecs: String,
    pub resolution: String,
    pub device_model: String,
}

/// The mDNS service type registered by the CamDroid Android app.
const SERVICE_TYPE: &str = "_camdroid._tcp.local.";

/// Discover CamDroid devices on the local network using mDNS.
///
/// Listens for `timeout_secs` seconds, collecting all devices that respond.
/// Returns the list of discovered devices sorted by name.
pub fn discover_devices(timeout_secs: u64) -> Result<Vec<DiscoveredDevice>> {
    info!(
        "Searching for CamDroid devices on the network ({} seconds)...",
        timeout_secs
    );

    let mdns = mdns_sd::ServiceDaemon::new()
        .context("Failed to create mDNS daemon — is the network interface available?")?;

    let receiver = mdns
        .browse(SERVICE_TYPE)
        .context("Failed to start mDNS browsing")?;

    let mut devices = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
    let mut first_found_at: Option<std::time::Instant> = None;

    loop {
        let now = std::time::Instant::now();
        if now >= deadline {
            break;
        }
        // Recommendation #5: Once at least one CamDroid phone is resolved on the LAN,
        // wait only 250ms more for any secondary devices instead of blocking the UI
        // for the entire 5-second timeout.
        if let Some(found_time) = first_found_at {
            if now.duration_since(found_time) >= Duration::from_millis(250) {
                break;
            }
        }

        let poll_wait = Duration::from_millis(100).min(deadline.saturating_duration_since(now));
        match receiver.recv_timeout(poll_wait) {
            Ok(event) => match event {
                mdns_sd::ServiceEvent::ServiceResolved(info) => {
                    let name = info.get_fullname().to_string();
                    let port = info.get_port();

                    // Extract TXT record properties
                    let props = info.get_properties();
                    let version = props
                        .get("version")
                        .map(|v| v.val_str().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    let codecs = props
                        .get("codecs")
                        .map(|v| v.val_str().to_string())
                        .unwrap_or_else(|| "h264".to_string());
                    let resolution = props
                        .get("resolution")
                        .map(|v| v.val_str().to_string())
                        .unwrap_or_else(|| "1080p".to_string());
                    let device_model = props
                        .get("device")
                        .map(|v| v.val_str().to_string())
                        .unwrap_or_else(|| "Unknown".to_string());

                    for addr in info.get_addresses() {
                        info!(
                            "Found: {} at {}:{} ({})",
                            name, addr, port, device_model
                        );
                        devices.push(DiscoveredDevice {
                            name: name.clone(),
                            ip: *addr,
                            port,
                            version: version.clone(),
                            codecs: codecs.clone(),
                            resolution: resolution.clone(),
                            device_model: device_model.clone(),
                        });
                    }
                    if first_found_at.is_none() && !devices.is_empty() {
                        first_found_at = Some(std::time::Instant::now());
                    }
                }
                mdns_sd::ServiceEvent::SearchStarted(_) => {
                    debug!("mDNS search started");
                }
                mdns_sd::ServiceEvent::ServiceFound(_, _) => {
                    debug!("mDNS service found, waiting for resolution...");
                }
                mdns_sd::ServiceEvent::ServiceRemoved(_, name) => {
                    debug!("mDNS service removed: {}", name);
                }
                mdns_sd::ServiceEvent::SearchStopped(_) => {
                    debug!("mDNS search stopped");
                }
            },
            // Newer mdns-sd no longer re-exports `flume`; the deadline check at the
            // top of the loop bounds this, so treat every recv error as a poll tick.
            Err(_) => continue,
        }
    }

    if let Err(e) = mdns.shutdown() {
        warn!("mDNS shutdown error: {:?}", e);
    }

    devices.sort_by(|a, b| a.name.cmp(&b.name));

    if devices.is_empty() {
        info!("No CamDroid devices found on the network");
    } else {
        info!("Found {} CamDroid device(s)", devices.len());
    }

    Ok(devices)
}
