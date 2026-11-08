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
use log::{debug, info, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

extern crate ffmpeg_next as ffmpeg;

// Re-export key types for consumers
pub use connection::{DiscoveredDevice, PacketChannels, WriterHandle};
pub use protocol::{ControlCommand, LiveStatusMetadata, ServerCapabilities};
pub use video::{DecodedVideoFrame, LatestFrameHub, SharedPreviewFrame, VideoCodec};

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

/// Session status reported to the GUI (enriched with 1Hz phone telemetry).
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionStatus {
    pub connected: bool,
    pub streaming: bool,
    pub device_name: String,
    pub codec: String,
    pub resolution: String,
    pub fps: u32,
    pub measured_fps: f32,
    pub bitrate_mbps: f32,
    pub battery: u32,
    pub temperature: f32,
    pub dropped_frames: u64,
    pub zoom: f32,
    pub min_zoom: f32,
    pub max_zoom: f32,
    pub mirror: bool,
    pub flash: bool,
    pub v4l2_active: bool,
    pub audio_active: bool,
    pub transport: String,
}

/// A live CamDroid streaming session.
///
/// Owns the TCP connection, video/audio decode pipelines, standalone preview
/// frame buffer, and v4l2/pulse output threads. Created via `CamdroidSession::connect()`.
pub struct CamdroidSession {
    writer: Arc<WriterHandle>,
    shutdown: Arc<AtomicBool>,
    capabilities: ServerCapabilities,
    config: SessionConfig,
    transport: String,
    live_telemetry: Arc<RwLock<LiveStatusMetadata>>,
    preview_hub: Arc<LatestFrameHub>,
    mirror_override: Arc<AtomicBool>,
    v4l2_active: Arc<AtomicBool>,
    audio_active: bool,
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
    /// Also checks USB ADB first if available so plugged-in phones appear immediately.
    pub fn discover_devices(timeout_secs: u64) -> Result<Vec<DiscoveredDevice>> {
        let mut list = Vec::new();
        if connection::adb::is_adb_available() {
            if let Ok(usb_serials) = connection::adb::list_devices() {
                for serial in usb_serials {
                    list.push(DiscoveredDevice {
                        name: format!("USB ({})", serial),
                        ip: std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)),
                        port: 4747,
                        version: "2.0".to_string(),
                        codecs: "h264,h265".to_string(),
                        resolution: "1080p,1440p,4k".to_string(),
                        device_model: format!("Android USB [{}]", serial),
                    });
                }
            }
        }
        if let Ok(mut mdns_devices) = connection::discovery::discover_devices(timeout_secs) {
            list.append(&mut mdns_devices);
        }
        Ok(list)
    }

    /// Connect to a CamDroid phone and start the streaming pipeline.
    pub fn connect(mode: ConnectionMode, config: SessionConfig) -> Result<Self> {
        let shutdown = Arc::new(AtomicBool::new(false));

        // Recommendation #5: Seamless Continuity Auto-Connect (USB ADB priority -> mDNS fallback)
        let (connect_addr, adb_guard, transport) = match &mode {
            ConnectionMode::Direct(addr) => {
                if !addr.contains(':') {
                    bail!(
                        "Invalid address format: '{}'. Expected IP:PORT (e.g., 192.168.1.5:4747)",
                        addr
                    );
                }
                (addr.clone(), None, "wifi".to_string())
            }
            ConnectionMode::Usb => {
                let forward = connection::adb::setup_usb_connection()?;
                let addr = forward.local_addr();
                (addr, Some(forward), "usb".to_string())
            }
            ConnectionMode::Discover => {
                // Try zero-latency USB ADB first if a device is plugged in
                if let Ok(forward) = connection::adb::setup_usb_connection() {
                    let addr = forward.local_addr();
                    info!("Auto-detected USB Android device via ADB at {}", addr);
                    (addr, Some(forward), "usb".to_string())
                } else {
                    let devices = connection::discovery::discover_devices(config.discovery_timeout)?;
                    if devices.is_empty() {
                        bail!(
                            "No CamDroid devices found via USB or Wi-Fi.\n\
                             Make sure:\n\
                             1. The CamDroid app is running on your phone\n\
                             2. Phone is plugged in via USB (with USB Debugging) or on the same Wi-Fi network"
                        );
                    }
                    let device = &devices[0];
                    info!(
                        "Auto-discovered: {} at {}:{}",
                        device.device_model, device.ip, device.port
                    );
                    (format!("{}:{}", device.ip, device.port), None, "wifi".to_string())
                }
            }
        };

        info!("Connecting to: {} ({})", connect_addr, transport);

        // Establish TCP connection
        let (channels, writer) = connection::tcp_client::connect(&connect_addr, shutdown.clone())?;
        let writer = Arc::new(writer);

        // Send handshake
        let handshake = protocol::ClientHandshake {
            version: "2.0".to_string(),
            client: "camdroid-desktop".to_string(),
        };
        let handshake_json = serde_json::to_value(&handshake)?;
        writer.send(protocol::Packet::metadata(&handshake_json)?)?;

        // Wait for server capabilities (Recommendation #1: Two-phase handshake)
        info!("Waiting for server capabilities...");
        let capabilities: ServerCapabilities = match channels
            .metadata_rx
            .recv_timeout(std::time::Duration::from_secs(5))
        {
            Ok(meta_bytes) => serde_json::from_slice(&meta_bytes).unwrap_or_else(|e| {
                warn!("Failed to parse server capabilities: {}", e);
                ServerCapabilities::default()
            }),
            Err(_) => {
                warn!("No server capabilities received within 5s, using defaults");
                ServerCapabilities::default()
            }
        };

        info!(
            "Connected to: {} (v{}, codecs: {:?}, zoom: {:.1}x..{:.1}x, battery: {}%)",
            capabilities.device,
            capabilities.version,
            capabilities.codecs,
            capabilities.min_zoom,
            capabilities.max_zoom,
            capabilities.battery
        );

        // Send start command so phone launches encoder with requested codec/res/fps
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

        let preview_hub = Arc::new(LatestFrameHub::new());
        let mirror_override = Arc::new(AtomicBool::new(false));
        let v4l2_active = Arc::new(AtomicBool::new(false));
        let live_telemetry = Arc::new(RwLock::new(LiveStatusMetadata {
            battery: capabilities.battery,
            fps: config.fps as f32,
            zoom: 1.0,
            ..LiveStatusMetadata::default()
        }));

        // Spawn background metadata listener for 1Hz telemetry from the phone (Recommendation #8)
        {
            let meta_rx = channels.metadata_rx;
            let telemetry_store = live_telemetry.clone();
            let shutdown_meta = shutdown.clone();
            std::thread::Builder::new()
                .name("metadata-telemetry".into())
                .spawn(move || {
                    while !shutdown_meta.load(Ordering::Relaxed) {
                        match meta_rx.recv_timeout(std::time::Duration::from_millis(250)) {
                            Ok(bytes) => {
                                if let Ok(status) = serde_json::from_slice::<LiveStatusMetadata>(&bytes) {
                                    if status.r#type == "status" || status.battery > 0 {
                                        if let Ok(mut guard) = telemetry_store.write() {
                                            *guard = status;
                                        }
                                    }
                                }
                            }
                            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                        }
                    }
                    debug!("Metadata telemetry listener exiting");
                })
                .ok();
        }

        // Create channel for decoded YUYV frames → v4l2loopback (tight 4-frame buffer)
        let (frame_tx, frame_rx) = crossbeam_channel::bounded::<DecodedVideoFrame>(4);

        // Spawn video decoder thread (populates both v4l2loopback and standalone LatestFrameHub)
        let _video_decoder_handle = video::decoder::spawn_decoder_thread(
            video_codec,
            channels.video_config_rx,
            channels.video_rx,
            frame_tx,
            (*preview_hub).clone(),
            mirror_override.clone(),
            width,
            height,
            shutdown.clone(),
        )?;

        // Spawn v4l2loopback writer thread (non-fatal if /dev/video10 is absent)
        let _v4l2_handle = video::v4l2_output::spawn_v4l2_writer_thread(
            config.v4l2_device.clone(),
            frame_rx,
            width,
            height,
            config.fps,
            shutdown.clone(),
            v4l2_active.clone(),
        )?;

        info!(
            "Video pipeline started: {} → FFmpeg low-delay decoder → preview hub + {}",
            config.codec, config.v4l2_device
        );

        // Set up audio pipeline if enabled (non-fatal if PulseAudio / pactl is unavailable)
        let (pulse_output, audio_active) = if config.audio {
            match audio::PulseOutput::new(&config.audio_device) {
                Ok(pulse) => {
                    let sink_name = pulse.sink_name().to_string();
                    let (pcm_tx, pcm_rx) = crossbeam_channel::bounded::<Vec<u8>>(32);

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
                    (Some(pulse), true)
                }
                Err(e) => {
                    warn!(
                        "PulseAudio virtual microphone unavailable ({:#}); continuing with video stream",
                        e
                    );
                    (None, false)
                }
            }
        } else {
            info!("Audio disabled");
            (None, false)
        };

        Ok(Self {
            writer,
            shutdown,
            capabilities,
            config,
            transport,
            live_telemetry,
            preview_hub,
            mirror_override,
            v4l2_active,
            audio_active,
            _adb_guard: adb_guard,
            _pulse_output: pulse_output,
        })
    }

    /// Send a control command to the phone (zoom, focus, exposure, etc.).
    pub fn send_control(&self, cmd: &ControlCommand) -> Result<()> {
        self.writer.send(protocol::Packet::control_cmd(cmd)?)
    }

    /// Request an immediate IDR keyframe from the phone encoder.
    pub fn request_keyframe(&self) -> Result<()> {
        self.send_control(&ControlCommand::RequestKeyframe)
    }

    /// Toggle desktop-side or phone-side horizontal mirror.
    pub fn set_mirror(&self, enabled: bool) -> Result<()> {
        self.mirror_override.store(enabled, Ordering::Relaxed);
        self.send_control(&ControlCommand::SetMirror { enabled })
    }

    /// Fetch the latest RGBA preview frame for the standalone desktop app UI.
    pub fn latest_frame(&self) -> Option<SharedPreviewFrame> {
        self.preview_hub.latest()
    }

    /// Get the server capabilities reported at handshake time.
    pub fn capabilities(&self) -> &ServerCapabilities {
        &self.capabilities
    }

    /// Get the current session config.
    pub fn config(&self) -> &SessionConfig {
        &self.config
    }

    /// Get the current session status enriched with real-time 1Hz phone telemetry.
    pub fn status(&self) -> SessionStatus {
        let alive = !self.shutdown.load(Ordering::Relaxed);
        let telem = self
            .live_telemetry
            .read()
            .map(|g| g.clone())
            .unwrap_or_default();

        SessionStatus {
            connected: alive,
            streaming: alive,
            device_name: self.capabilities.device.clone(),
            codec: if telem.codec.is_empty() {
                self.config.codec.clone()
            } else {
                telem.codec
            },
            resolution: if telem.resolution.is_empty() {
                self.config.resolution.clone()
            } else {
                telem.resolution
            },
            fps: self.config.fps,
            measured_fps: if telem.fps > 0.0 {
                telem.fps
            } else {
                self.config.fps as f32
            },
            bitrate_mbps: telem.bitrate as f32 / 1_000_000.0,
            battery: if telem.battery > 0 {
                telem.battery
            } else {
                self.capabilities.battery
            },
            temperature: telem.temperature,
            dropped_frames: telem.dropped_frames,
            zoom: if telem.zoom > 0.0 { telem.zoom } else { 1.0 },
            min_zoom: self.capabilities.min_zoom,
            max_zoom: self.capabilities.max_zoom,
            mirror: self.mirror_override.load(Ordering::Relaxed) || telem.mirror,
            flash: telem.flash,
            v4l2_active: self.v4l2_active.load(Ordering::Relaxed),
            audio_active: self.audio_active,
            transport: self.transport.clone(),
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
        let _ = self.send_control(&ControlCommand::Stop);
        self.shutdown.store(true, Ordering::Relaxed);
        std::thread::sleep(std::time::Duration::from_millis(200));
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
