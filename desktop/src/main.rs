// CamDroid Desktop Client
//
// Receives camera streams from an Android phone and outputs them as a
// virtual webcam (v4l2loopback) and virtual microphone (PulseAudio).
//
// Usage:
//   camdroid-client --discover          # Auto-find phone via mDNS
//   camdroid-client --usb               # Connect via USB/ADB
//   camdroid-client --connect 192.168.1.5:4747  # Direct IP

mod audio;
mod connection;
mod control;
mod protocol;
mod video;

use anyhow::{bail, Context, Result};
use clap::Parser;
use log::{error, info, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

extern crate ffmpeg_next as ffmpeg;

/// CamDroid Desktop Client — use your Android phone as a webcam on Linux.
#[derive(Parser, Debug)]
#[command(name = "camdroid-client", version, about)]
struct Args {
    /// Connect directly to phone at IP:PORT
    #[arg(long, value_name = "IP:PORT")]
    connect: Option<String>,

    /// Auto-detect USB device via ADB port forwarding
    #[arg(long)]
    usb: bool,

    /// Auto-discover phone via mDNS (default if no other mode specified)
    #[arg(long)]
    discover: bool,

    /// v4l2loopback device path
    #[arg(long, default_value = "/dev/video10")]
    device: String,

    /// Preferred video codec
    #[arg(long, default_value = "h264", value_parser = ["h264", "h265", "mjpeg"])]
    codec: String,

    /// Preferred resolution
    #[arg(long, default_value = "1080p", value_parser = ["1080p", "1440p", "4k"])]
    resolution: String,

    /// Preferred FPS
    #[arg(long, default_value_t = 60)]
    fps: u32,

    /// Disable audio streaming
    #[arg(long)]
    no_audio: bool,

    /// PulseAudio sink name for virtual microphone
    #[arg(long, default_value = "CamDroid")]
    audio_device: String,

    /// mDNS discovery timeout in seconds
    #[arg(long, default_value_t = 5)]
    discovery_timeout: u64,
}

/// Parse a resolution string into (width, height).
fn parse_resolution(res: &str) -> (u32, u32) {
    match res {
        "1080p" => (1920, 1080),
        "1440p" => (2560, 1440),
        "4k" => (3840, 2160),
        _ => (1920, 1080), // Default fallback
    }
}

fn main() -> Result<()> {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    // Initialize FFmpeg
    ffmpeg::init().context("Failed to initialize FFmpeg")?;

    let args = Args::parse();

    info!("CamDroid Desktop Client v{}", env!("CARGO_PKG_VERSION"));
    info!(
        "Config: codec={}, resolution={}, fps={}, audio={}",
        args.codec,
        args.resolution,
        args.fps,
        !args.no_audio
    );

    // Global shutdown signal
    let shutdown = Arc::new(AtomicBool::new(false));

    // Register Ctrl+C handler
    let shutdown_ctrlc = shutdown.clone();
    ctrlc::set_handler(move || {
        info!("Ctrl+C received, shutting down...");
        shutdown_ctrlc.store(true, Ordering::Relaxed);
    })
    .context("Failed to set Ctrl+C handler")?;

    // Determine the connection address
    let connect_addr = determine_connection(&args)?;
    info!("Connecting to: {}", connect_addr);

    // Store ADB forward guard to keep it alive for the session duration.
    // When this guard is dropped, the ADB port forwarding is cleaned up.
    let _adb_guard: Option<connection::AdbForward>;

    // Establish TCP connection
    let (channels, writer) = if args.usb {
        let forward = connection::adb::setup_usb_connection()?;
        let addr = forward.local_addr();
        let result = connection::tcp_client::connect(&*addr, shutdown.clone())?;
        _adb_guard = Some(forward);
        result
    } else {
        _adb_guard = None;
        connection::tcp_client::connect(&*connect_addr, shutdown.clone())?
    };

    let writer = Arc::new(writer);

    // Send handshake
    let handshake = protocol::ClientHandshake {
        version: "1.0".to_string(),
        client: "camdroid-desktop".to_string(),
    };
    let handshake_json = serde_json::to_value(&handshake)?;
    writer.send(protocol::Packet::metadata(&handshake_json)?)?;

    // Wait for server capabilities
    info!("Waiting for server capabilities...");
    let capabilities: protocol::ServerCapabilities = match channels
        .metadata_rx
        .recv_timeout(std::time::Duration::from_secs(10))
    {
        Ok(meta_bytes) => {
            serde_json::from_slice(&meta_bytes).unwrap_or_else(|e| {
                warn!("Failed to parse server capabilities: {}", e);
                protocol::ServerCapabilities {
                    version: "unknown".to_string(),
                    device: "unknown".to_string(),
                    codecs: vec!["h264".to_string()],
                    resolutions: vec!["1080p".to_string()],
                    fps: vec![30, 60],
                    audio: true,
                    battery: 100,
                }
            })
        }
        Err(_) => {
            warn!("No server capabilities received, using defaults");
            protocol::ServerCapabilities {
                version: "unknown".to_string(),
                device: "unknown".to_string(),
                codecs: vec!["h264".to_string()],
                resolutions: vec!["1080p".to_string()],
                fps: vec![30, 60],
                audio: true,
                battery: 100,
            }
        }
    };

    info!(
        "Connected to: {} (v{}, codecs: {:?}, battery: {}%)",
        capabilities.device, capabilities.version, capabilities.codecs, capabilities.battery
    );

    // Send start command
    let start_cmd = protocol::ControlCommand::Start {
        codec: args.codec.clone(),
        resolution: args.resolution.clone(),
        fps: args.fps,
        audio: !args.no_audio,
    };
    writer.send(protocol::Packet::control_cmd(&start_cmd)?)?;
    info!("Stream start requested");

    let (width, height) = parse_resolution(&args.resolution);
    let video_codec = video::VideoCodec::from_str(&args.codec)?;

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
        args.device.clone(),
        frame_rx,
        width,
        height,
        args.fps,
        shutdown.clone(),
    )?;

    info!("Video pipeline started: {} → {} → {}", args.codec, "FFmpeg decoder", args.device);

    // Set up audio pipeline if enabled
    let _pulse_output: Option<audio::PulseOutput>;
    if !args.no_audio {
        // Create PulseAudio virtual microphone
        let pulse = audio::PulseOutput::new(&args.audio_device)?;
        let sink_name = pulse.sink_name().to_string();

        // Create channel for decoded PCM → PulseAudio
        let (pcm_tx, pcm_rx) = crossbeam_channel::bounded::<Vec<u8>>(240);

        // Spawn AAC decoder thread
        let _audio_decoder_handle = audio::aac_decoder::spawn_audio_decoder_thread(
            channels.audio_config_rx,
            channels.audio_rx,
            pcm_tx,
            shutdown.clone(),
        )?;

        // Spawn PulseAudio writer thread
        let _pulse_handle = audio::pulse_output::spawn_pulse_writer_thread(
            pcm_rx,
            sink_name,
            shutdown.clone(),
        )?;

        info!("Audio pipeline started: AAC → FFmpeg decoder → PulseAudio ({})", args.audio_device);
        _pulse_output = Some(pulse);
    } else {
        _pulse_output = None;
        info!("Audio disabled");
    }

    // Spawn the interactive control CLI
    let _control_handle = control::remote::spawn_control_thread(writer, shutdown.clone())?;

    // Main thread: wait for shutdown
    // The control thread or Ctrl+C will set the shutdown flag.
    while !shutdown.load(Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    info!("Shutdown initiated, cleaning up...");

    // Give threads a moment to finish
    std::thread::sleep(std::time::Duration::from_millis(500));

    // PulseOutput's Drop impl will clean up the null sink.
    // AdbForward's Drop impl will clean up port forwarding.
    info!("CamDroid client shut down cleanly. Goodbye!");

    Ok(())
}

/// Determine the connection address based on CLI arguments.
fn determine_connection(args: &Args) -> Result<String> {
    if let Some(ref addr) = args.connect {
        // Direct connection — validate format
        if !addr.contains(':') {
            bail!(
                "Invalid address format: '{}'. Expected IP:PORT (e.g., 192.168.1.5:4747)",
                addr
            );
        }
        return Ok(addr.clone());
    }

    if args.usb {
        // USB mode — ADB forwarding will provide 127.0.0.1:4747
        return Ok("127.0.0.1:4747".to_string());
    }

    // Default: mDNS discovery
    info!("No connection mode specified, using mDNS discovery...");
    let devices =
        connection::discovery::discover_devices(args.discovery_timeout)?;

    if devices.is_empty() {
        bail!(
            "No CamDroid devices found on the network.\n\
             Make sure:\n\
             1. The CamDroid app is running on your phone\n\
             2. Phone and PC are on the same WiFi network\n\
             3. Try connecting directly: --connect <phone-ip>:4747"
        );
    }

    let device = &devices[0];
    info!(
        "Auto-discovered: {} at {}:{}",
        device.device_model, device.ip, device.port
    );

    Ok(format!("{}:{}", device.ip, device.port))
}
