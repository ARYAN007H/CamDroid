// CamDroid Desktop Client — CLI Interface
//
// Thin wrapper around the camdroid_client library.
//
// Usage:
//   camdroid-client --discover          # Auto-find phone via mDNS
//   camdroid-client --usb               # Connect via USB/ADB
//   camdroid-client --connect 192.168.1.5:4747  # Direct IP

use anyhow::{Context, Result};
use camdroid_client::{CamdroidSession, ConnectionMode, SessionConfig};
use clap::Parser;
use log::info;
use std::sync::atomic::Ordering;

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

fn main() -> Result<()> {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    // Initialize FFmpeg
    CamdroidSession::init()?;

    let args = Args::parse();

    info!("CamDroid Desktop Client v{}", env!("CARGO_PKG_VERSION"));
    info!(
        "Config: codec={}, resolution={}, fps={}, audio={}",
        args.codec, args.resolution, args.fps, !args.no_audio
    );

    // Determine connection mode
    let mode = if let Some(ref addr) = args.connect {
        ConnectionMode::Direct(addr.clone())
    } else if args.usb {
        ConnectionMode::Usb
    } else {
        ConnectionMode::Discover
    };

    let config = SessionConfig {
        codec: args.codec,
        resolution: args.resolution,
        fps: args.fps,
        audio: !args.no_audio,
        v4l2_device: args.device,
        audio_device: args.audio_device,
        discovery_timeout: args.discovery_timeout,
    };

    // Connect and start streaming
    let session = CamdroidSession::connect(mode, config)?;

    // Register Ctrl+C handler
    let shutdown = session.shutdown_flag();
    ctrlc::set_handler(move || {
        info!("Ctrl+C received, shutting down...");
        shutdown.store(true, Ordering::Relaxed);
    })
    .context("Failed to set Ctrl+C handler")?;

    // Spawn the interactive control CLI
    let writer = session.writer();
    let shutdown_for_control = session.shutdown_flag();
    camdroid_client::control::remote::spawn_control_thread(writer, shutdown_for_control)?;

    // Main thread: wait for shutdown
    while session.is_alive() {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    session.stop()?;
    info!("CamDroid client shut down cleanly. Goodbye!");

    Ok(())
}
