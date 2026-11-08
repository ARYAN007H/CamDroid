//! CamDroid core library.
//!
//! Receives the phone's camera stream, decodes it, applies the chosen look,
//! and delivers it to an in-app preview and, optionally, a virtual webcam.
//! Used by both the command-line client and the Tauri desktop app.

pub mod audio;
pub mod connection;
pub mod control;
pub mod media_queue;
pub mod protocol;
pub mod session;
pub mod telemetry;
pub mod video;

#[cfg(test)]
mod media_queue_tests;
#[cfg(test)]
mod session_tests;

use anyhow::{bail, Context, Result};
use log::{info, warn};
use serde::Serialize;
use session::{
    recommended_bitrate, CapabilitySet, NegotiatedStream, SessionEvent, SessionMachine,
    SessionMessage, StreamRequest,
};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

extern crate ffmpeg_next as ffmpeg;

// Re-export key types for consumers.
pub use connection::{DiscoveredDevice, PacketChannels, WriterHandle};
pub use protocol::{ControlCommand, ServerCapabilities};
pub use telemetry::{Telemetry, TelemetrySnapshot, VirtualCameraStatus};
pub use video::filters::{
    FilterController, FilterPreset, FilterSettings, RendererProbe, RendererStatus,
};
pub use video::output::{OutputController, OutputMode};
pub use video::preview::{PreviewFrame, PreviewHub, PreviewWait};
pub use video::VideoCodec;

/// How long the phone gets to answer each negotiation step.
const NEGOTIATION_TIMEOUT: Duration = Duration::from_secs(10);
/// How long [`CamdroidSession::stop`] waits for worker threads to finish.
const SHUTDOWN_JOIN_BUDGET: Duration = Duration::from_millis(1500);
/// How long a queued `stop` command gets to reach the wire before the socket closes.
const STOP_FLUSH_BUDGET: Duration = Duration::from_millis(250);

/// Configuration for a CamDroid streaming session.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub codec: String,
    pub resolution: String,
    pub fps: u32,
    pub audio: bool,
    /// Where frames go besides the in-app preview. Switchable at runtime.
    pub output: OutputMode,
    /// Display name for the phone when the transport does not supply one.
    pub device_name: Option<String>,
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
            output: OutputMode::VirtualCamera,
            device_name: None,
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

/// What the UI needs to render a connected session.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStatus {
    pub connected: bool,
    pub device_name: String,
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub audio_enabled: bool,
    pub telemetry: TelemetrySnapshot,
}

/// Raises the shutdown flag if negotiation is abandoned part-way, so threads
/// that were already started do not outlive a failed connection attempt.
struct AbortOnDrop {
    shutdown: Arc<AtomicBool>,
    armed: bool,
}

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        if self.armed {
            self.shutdown.store(true, Ordering::Relaxed);
        }
    }
}

/// A live CamDroid streaming session.
///
/// Owns the TCP connection and the decode, render and audio threads. Created
/// via [`CamdroidSession::connect`]; dropping it tears everything down.
pub struct CamdroidSession {
    writer: Arc<WriterHandle>,
    shutdown: Arc<AtomicBool>,
    stream: NegotiatedStream,
    config: SessionConfig,
    device_name: String,
    filters: FilterController,
    output: OutputController,
    preview: PreviewHub,
    telemetry: Telemetry,
    threads: Mutex<Vec<JoinHandle<()>>>,
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

    /// Connect to a CamDroid phone, negotiate a stream, and start the pipeline.
    pub fn connect(mode: ConnectionMode, config: SessionConfig) -> Result<Self> {
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut abort = AbortOnDrop {
            shutdown: Arc::clone(&shutdown),
            armed: true,
        };
        let telemetry = Telemetry::default();

        let (connect_addr, adb_guard, discovered_name) = resolve_transport(&mode, &config)?;
        let device_name = discovered_name
            .or_else(|| config.device_name.clone())
            .unwrap_or_else(|| "Android phone".to_string());

        let mut machine = SessionMachine::new();
        let transition = |machine: &mut SessionMachine, event| {
            machine
                .apply(event)
                .map(|_| ())
                .map_err(|error| anyhow::anyhow!(error.message))
        };
        transition(&mut machine, SessionEvent::ConnectRequested)?;

        let connection = connection::connect(&connect_addr, shutdown.clone(), telemetry.clone())?;
        let connection::Connection {
            channels,
            writer,
            keyframes,
            intra_only,
        } = connection;
        let writer = Arc::new(writer);
        transition(&mut machine, SessionEvent::TransportConnected)?;

        let send_message = |message: SessionMessage| -> Result<()> {
            writer.send(protocol::Packet::metadata(&serde_json::to_value(message)?)?)
        };

        send_message(SessionMessage::Hello {
            protocol_versions: vec![2],
            client: "camdroid-desktop".to_string(),
        })?;

        info!("Waiting for phone capabilities...");
        let capabilities: CapabilitySet =
            match channels.metadata_rx.recv_timeout(NEGOTIATION_TIMEOUT) {
                Ok(bytes) => match serde_json::from_slice::<SessionMessage>(&bytes)? {
                    SessionMessage::Capabilities(capabilities) => capabilities,
                    other => bail!("expected capabilities response, received {other:?}"),
                },
                Err(_) => bail!("The phone did not answer. Is the CamDroid app streaming?"),
            };
        transition(
            &mut machine,
            SessionEvent::CapabilitiesReceived(capabilities.clone()),
        )?;

        let (width, height) = parse_resolution(&config.resolution);
        let desired = StreamRequest {
            config: protocol::v2::StreamConfig {
                codec: config.codec.clone(),
                width,
                height,
                fps: config.fps,
                bitrate_bps: recommended_bitrate(width, height, config.fps, &config.codec),
                profile: None,
                level: None,
                audio: None,
            },
            audio_enabled: config.audio,
        };
        let request = capabilities
            .resolve(&desired)
            .map_err(|error| anyhow::anyhow!(error.message))?;
        if request.config != desired.config {
            info!(
                "Phone offers {} {}x{}@{}; using that instead of {} {}x{}@{}",
                request.config.codec,
                request.config.width,
                request.config.height,
                request.config.fps,
                desired.config.codec,
                desired.config.width,
                desired.config.height,
                desired.config.fps
            );
        }

        let video_codec = VideoCodec::from_str(&request.config.codec)?;
        intra_only.store(video_codec == VideoCodec::Mjpeg, Ordering::Relaxed);

        transition(&mut machine, SessionEvent::StartRequested(request.clone()))?;
        send_message(SessionMessage::Start(request))?;

        let stream = match channels.metadata_rx.recv_timeout(NEGOTIATION_TIMEOUT) {
            Ok(bytes) => match serde_json::from_slice::<SessionMessage>(&bytes)? {
                SessionMessage::StreamAccepted(stream) => stream,
                SessionMessage::StreamRejected(error) => {
                    bail!("The phone rejected the stream: {}", error.message)
                }
                other => bail!("expected stream acceptance, received {other:?}"),
            },
            Err(_) => bail!("The phone did not acknowledge the stream request."),
        };
        transition(
            &mut machine,
            SessionEvent::StartAcknowledged(stream.clone()),
        )?;

        info!(
            "Connected to {device_name}: {} {}x{}@{}",
            stream.config.codec, stream.config.width, stream.config.height, stream.config.fps
        );

        let filters = FilterController::default();
        let output = OutputController::new(config.output);
        let preview = PreviewHub::default();
        let mut threads = Vec::new();

        // Decoded frames are tiny in number (the decoder forwards one per
        // batch) so a short queue keeps latency low.
        let (frame_tx, frame_rx) = crossbeam_channel::bounded::<Vec<u8>>(4);
        threads.push(video::decoder::spawn_decoder_thread(
            video::decoder::DecoderConfig {
                codec: video_codec,
                width: stream.config.width,
                height: stream.config.height,
            },
            video::decoder::DecoderChannels {
                video_config_rx: channels.video_config_rx,
                video_rx: channels.video_rx,
                frame_tx,
            },
            telemetry.clone(),
            keyframes,
            shutdown.clone(),
        )?);
        threads.push(video::render::spawn_render_thread(
            video::render::RenderConfig {
                device_path: config.v4l2_device.clone(),
                width: stream.config.width,
                height: stream.config.height,
            },
            frame_rx,
            video::render::RenderTargets {
                filters: filters.clone(),
                output: output.clone(),
                preview: preview.clone(),
                telemetry: telemetry.clone(),
            },
            shutdown.clone(),
        )?);

        let pulse_output = if stream.audio_enabled {
            match start_audio(
                &config.audio_device,
                channels.audio_config_rx,
                channels.audio_rx,
                &shutdown,
            ) {
                Ok((pulse, handles)) => {
                    threads.extend(handles);
                    Some(pulse)
                }
                Err(error) => {
                    // Video is the product; a missing PulseAudio must not block it.
                    warn!("Audio unavailable: {error:#}");
                    telemetry.set_audio_error(format!("{error:#}"));
                    None
                }
            }
        } else {
            info!("Audio disabled");
            None
        };

        abort.armed = false;
        Ok(Self {
            writer,
            shutdown,
            stream,
            config,
            device_name,
            filters,
            output,
            preview,
            telemetry,
            threads: Mutex::new(threads),
            _adb_guard: adb_guard,
            _pulse_output: pulse_output,
        })
    }

    /// Send a control command to the phone (zoom, focus, exposure, etc.).
    pub fn send_control(&self, cmd: &ControlCommand) -> Result<()> {
        self.writer.send(protocol::Packet::control_cmd(cmd)?)
    }

    /// The stream the phone agreed to, which may differ from what was asked for.
    pub fn negotiated(&self) -> &NegotiatedStream {
        &self.stream
    }

    /// Get the current session config.
    pub fn config(&self) -> &SessionConfig {
        &self.config
    }

    /// The live filter selection applied to the preview and virtual camera.
    pub fn filters(&self) -> FilterController {
        self.filters.clone()
    }

    /// Switches between app-only and virtual-camera output while streaming.
    pub fn output(&self) -> OutputController {
        self.output.clone()
    }

    /// Newest rendered frames for display inside an application window.
    pub fn preview(&self) -> PreviewHub {
        self.preview.clone()
    }

    pub fn telemetry(&self) -> Telemetry {
        self.telemetry.clone()
    }

    /// Get the current session status.
    pub fn status(&self) -> SessionStatus {
        SessionStatus {
            connected: self.is_alive(),
            device_name: self.device_name.clone(),
            codec: self.stream.config.codec.clone(),
            width: self.stream.config.width,
            height: self.stream.config.height,
            fps: self.stream.config.fps,
            audio_enabled: self.stream.audio_enabled,
            telemetry: self.telemetry.snapshot(),
        }
    }

    /// True until the user stops the session or the connection is lost.
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

    /// Gracefully stops the session: tells the phone, closes the socket, and
    /// waits (briefly) for the worker threads. Safe to call more than once.
    pub fn stop(&self) -> Result<()> {
        if self.is_alive() {
            info!("Stopping CamDroid session...");
            let _ = self.send_control(&ControlCommand::Stop);
            self.writer.drain(STOP_FLUSH_BUDGET);
        }
        self.halt();
        self.join_workers();
        Ok(())
    }

    /// Stops all pipeline activity without waiting for threads.
    fn halt(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
        self.writer.close();
        self.preview.close();
    }

    fn join_workers(&self) {
        let handles =
            std::mem::take(&mut *self.threads.lock().unwrap_or_else(PoisonError::into_inner));
        let deadline = Instant::now() + SHUTDOWN_JOIN_BUDGET;
        for handle in handles {
            while !handle.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            if handle.is_finished() {
                let _ = handle.join();
            } else {
                warn!("A worker thread did not stop in time; detaching it");
            }
        }
    }
}

impl Drop for CamdroidSession {
    fn drop(&mut self) {
        self.halt();
    }
}

/// Works out the socket address (and a friendly device name when known).
fn resolve_transport(
    mode: &ConnectionMode,
    config: &SessionConfig,
) -> Result<(String, Option<connection::AdbForward>, Option<String>)> {
    match mode {
        ConnectionMode::Direct(addr) => {
            if !addr.contains(':') {
                bail!("Invalid address '{addr}'. Expected IP:PORT, for example 192.168.1.5:4747");
            }
            Ok((addr.clone(), None, None))
        }
        ConnectionMode::Usb => {
            let forward = connection::adb::setup_usb_connection()?;
            let address = forward.local_addr();
            let name = forward.device_model();
            Ok((address, Some(forward), name))
        }
        ConnectionMode::Discover => {
            let devices = connection::discovery::discover_devices(config.discovery_timeout)?;
            let Some(device) = devices.first() else {
                bail!(
                    "No CamDroid devices found on the network.\n\
                     Make sure:\n\
                     1. The CamDroid app is running on your phone\n\
                     2. Phone and PC are on the same WiFi network"
                );
            };
            info!(
                "Auto-discovered: {} at {}:{}",
                device.device_model, device.ip, device.port
            );
            Ok((
                format!("{}:{}", device.ip, device.port),
                None,
                Some(device.device_model.clone()),
            ))
        }
    }
}

/// Creates the virtual microphone and the threads that feed it.
fn start_audio(
    sink_name: &str,
    audio_config_rx: crossbeam_channel::Receiver<Vec<u8>>,
    audio_rx: crossbeam_channel::Receiver<media_queue::TimedMediaFrame>,
    shutdown: &Arc<AtomicBool>,
) -> Result<(audio::PulseOutput, Vec<JoinHandle<()>>)> {
    let pulse = audio::PulseOutput::new(sink_name)?;
    let (pcm_tx, pcm_rx) = crossbeam_channel::bounded::<Vec<u8>>(240);
    let decoder = audio::aac_decoder::spawn_audio_decoder_thread(
        audio_config_rx,
        audio_rx,
        pcm_tx,
        shutdown.clone(),
    )?;
    let writer = audio::pulse_output::spawn_pulse_writer_thread(
        pcm_rx,
        pulse.sink_name().to_string(),
        shutdown.clone(),
    )?;
    info!("Audio pipeline started: AAC -> FFmpeg -> PulseAudio ({sink_name})");
    Ok((pulse, vec![decoder, writer]))
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
