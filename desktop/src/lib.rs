// CamDroid Core Library
//
// Exposes the streaming pipeline as a reusable library for both
// the CLI binary and the Tauri desktop GUI.

pub mod audio;
pub mod connection;
pub mod control;
pub mod protocol;
pub mod video;

use anyhow::{bail, Context, Result};
use log::{info, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

extern crate ffmpeg_next as ffmpeg;

// Re-export key types for consumers
pub use connection::{DiscoveredDevice, PacketChannels, WriterHandle};
pub use protocol::{ControlCommand, ServerCapabilities};
pub use video::VideoCodec;

/// Configuration for a CamDroid streaming session.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub codec: String,
    pub resolution: String,
    pub fps: u32,
    pub audio: bool,
    pub v4l2_device: String,
    pub audio_device: String,
    pub discovery_timeout: u64,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            codec: "h264".to_string(),
            resolution: "1080p".to_string(),
            fps: 60,
            audio: true,
            v4l2_device: "/dev/video10".to_string(),
            audio_device: "CamDroid".to_string(),
            discovery_timeout: 5,
        }
    }
}

/// Connection mode — how to reach the phone.
#[derive(Debug, Clone)]
pub enum ConnectionMode {
    /// Direct TCP connection to IP:PORT
    Direct(String),
    /// USB via ADB port forwarding
    Usb,
    /// Auto-discover via mDNS
    Discover,
}

/// Session status reported to the GUI.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionStatus {
    pub connected: bool,
    pub streaming: bool,
    pub device_name: String,
    pub codec: String,
    pub resolution: String,
    pub fps: u32,
    pub battery: u32,
}

/// A live CamDroid streaming session.
///
/// Owns the TCP connection, video/audio decode pipelines, and v4l2/pulse
/// output threads. Created via `CamdroidSession::connect()`.
pub struct CamdroidSession {
    writer: Arc<WriterHandle>,
    shutdown: Arc<AtomicBool>,
    capabilities: ServerCapabilities,
    config: SessionConfig,
    _adb_guard: Option<connection::AdbForward>,
    _pulse_output: Option<audio::PulseOutput>,
}

impl CamdroidSession {
    /// Initialize FFmpeg. Must be called once before creating any sessions.
    pub fn init() -> Result<()> {
        ffmpeg::init().context("Failed to initialize FFmpeg")?;
        Ok(())
    }

    /// Discover CamDroid devices on the local network via mDNS.
    pub fn discover_devices(timeout_secs: u64) -> Result<Vec<DiscoveredDevice>> {
        connection::discovery::discover_devices(timeout_secs)
    }

    /// Connect to a CamDroid phone and start the streaming pipeline.
    pub fn connect(mode: ConnectionMode, config: SessionConfig) -> Result<Self> {
        let shutdown = Arc::new(AtomicBool::new(false));

        // Determine the connection address
        let (connect_addr, adb_guard) = match &mode {
            ConnectionMode::Direct(addr) => {
                if !addr.contains(':') {
                    bail!(
                        "Invalid address format: '{}'. Expected IP:PORT (e.g., 192.168.1.5:4747)",
                        addr
                    );
                }
                (addr.clone(), None)
            }
            ConnectionMode::Usb => {
                let forward = connection::adb::setup_usb_connection()?;
                let addr = forward.local_addr();
                (addr.to_string(), Some(forward))
            }
            ConnectionMode::Discover => {
                let devices = connection::discovery::discover_devices(config.discovery_timeout)?;
                if devices.is_empty() {
                    bail!(
                        "No CamDroid devices found on the network.\n\
                         Make sure:\n\
                         1. The CamDroid app is running on your phone\n\
                         2. Phone and PC are on the same WiFi network"
                    );
                }
                let device = &devices[0];
                info!(
                    "Auto-discovered: {} at {}:{}",
                    device.device_model, device.ip, device.port
                );
                (format!("{}:{}", device.ip, device.port), None)
            }
        };

        info!("Connecting to: {}", connect_addr);

        // Establish TCP connection
        let (channels, writer) = if adb_guard.is_some() {
            // For USB mode, the connect_addr is already set from the ADB forward
            connection::tcp_client::connect(&*connect_addr, shutdown.clone())?
        } else {
            connection::tcp_client::connect(&*connect_addr, shutdown.clone())?
        };

        let writer = Arc::new(writer);

        // Send handshake
        let handshake = protocol::ClientHandshake {
            version: "2.0".to_string(),
            client: "camdroid-desktop".to_string(),
        };
        let handshake_json = serde_json::to_value(&handshake)?;
        writer.send(protocol::Packet::metadata(&handshake_json)?)?;

        // Wait for server capabilities
        info!("Waiting for server capabilities...");
        let capabilities: ServerCapabilities = match channels
            .metadata_rx
            .recv_timeout(std::time::Duration::from_secs(10))
        {
            Ok(meta_bytes) => serde_json::from_slice(&meta_bytes).unwrap_or_else(|e| {
                warn!("Failed to parse server capabilities: {}", e);
                ServerCapabilities::default()
            }),
            Err(_) => {
                warn!("No server capabilities received, using defaults");
                ServerCapabilities::default()
            }
        };

        info!(
            "Connected to: {} (v{}, codecs: {:?}, battery: {}%)",
            capabilities.device, capabilities.version, capabilities.codecs, capabilities.battery
        );

        // Send start command
        let start_cmd = ControlCommand::Start {
            codec: config.codec.clone(),
            resolution: config.resolution.clone(),
            fps: config.fps,
            audio: config.audio,
        };
        writer.send(protocol::Packet::control_cmd(&start_cmd)?)?;
        info!("Stream start requested");

        let (width, height) = parse_resolution(&config.resolution);
        let video_codec = VideoCodec::from_str(&config.codec)?;

        // Create channel for decoded YUYV frames → v4l2loopback
        let (frame_tx, frame_rx) = crossbeam_channel::bounded::<Vec<u8>>(8);

        // Spawn video decoder thread
        let _video_decoder_handle = video::decoder::spawn_decoder_thread(
            video_codec,
            channels.video_config_rx,
            channels.video_rx,
            frame_tx,
            width,
            height,
            shutdown.clone(),
        )?;

        // Spawn v4l2loopback writer thread
        let _v4l2_handle = video::v4l2_output::spawn_v4l2_writer_thread(
            config.v4l2_device.clone(),
            frame_rx,
            width,
            height,
            config.fps,
            shutdown.clone(),
        )?;

        info!(
            "Video pipeline started: {} → FFmpeg decoder → {}",
            config.codec, config.v4l2_device
        );

        // Set up audio pipeline if enabled
        let pulse_output = if config.audio {
            let pulse = audio::PulseOutput::new(&config.audio_device)?;
            let sink_name = pulse.sink_name().to_string();

            let (pcm_tx, pcm_rx) = crossbeam_channel::bounded::<Vec<u8>>(240);

            let _audio_decoder_handle = audio::aac_decoder::spawn_audio_decoder_thread(
                channels.audio_config_rx,
                channels.audio_rx,
                pcm_tx,
                shutdown.clone(),
            )?;

            let _pulse_handle = audio::pulse_output::spawn_pulse_writer_thread(
                pcm_rx,
                sink_name,
                shutdown.clone(),
            )?;

            info!(
                "Audio pipeline started: AAC → FFmpeg decoder → PulseAudio ({})",
                config.audio_device
            );
            Some(pulse)
        } else {
            info!("Audio disabled");
            None
        };

        Ok(Self {
            writer,
            shutdown,
            capabilities,
            config,
            _adb_guard: adb_guard,
            _pulse_output: pulse_output,
        })
    }

    /// Send a control command to the phone (zoom, focus, exposure, etc.).
    pub fn send_control(&self, cmd: &ControlCommand) -> Result<()> {
        self.writer.send(protocol::Packet::control_cmd(cmd)?)
    }

    /// Get the server capabilities reported at handshake time.
    pub fn capabilities(&self) -> &ServerCapabilities {
        &self.capabilities
    }

    /// Get the current session config.
    pub fn config(&self) -> &SessionConfig {
        &self.config
    }

    /// Get the current session status.
    pub fn status(&self) -> SessionStatus {
        SessionStatus {
            connected: !self.shutdown.load(Ordering::Relaxed),
            streaming: !self.shutdown.load(Ordering::Relaxed),
            device_name: self.capabilities.device.clone(),
            codec: self.config.codec.clone(),
            resolution: self.config.resolution.clone(),
            fps: self.config.fps,
            battery: self.capabilities.battery,
        }
    }

    /// Check if the session is still alive.
    pub fn is_alive(&self) -> bool {
        !self.shutdown.load(Ordering::Relaxed)
    }

    /// Get a clone of the shutdown flag for external coordination.
    pub fn shutdown_flag(&self) -> Arc<AtomicBool> {
        self.shutdown.clone()
    }

    /// Get a reference to the writer handle for sending raw packets.
    pub fn writer(&self) -> Arc<WriterHandle> {
        self.writer.clone()
    }

    /// Gracefully stop the streaming session.
    pub fn stop(&self) -> Result<()> {
        info!("Stopping CamDroid session...");
        // Send stop command to the phone
        let _ = self.send_control(&ControlCommand::Stop);
        // Signal all threads to shut down
        self.shutdown.store(true, Ordering::Relaxed);
        // Give threads a moment to finish
        std::thread::sleep(std::time::Duration::from_millis(300));
        info!("CamDroid session stopped");
        Ok(())
    }
}

impl Drop for CamdroidSession {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

/// Parse a resolution string into (width, height).
pub fn parse_resolution(res: &str) -> (u32, u32) {
    match res {
        "1080p" => (1920, 1080),
        "1440p" => (2560, 1440),
        "4k" => (3840, 2160),
        _ => (1920, 1080),
    }
}
