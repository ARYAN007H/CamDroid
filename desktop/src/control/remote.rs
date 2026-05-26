// Remote camera control via interactive CLI.
//
// Provides a command-line interface for adjusting camera settings on the phone
// while streaming. Commands are serialized as JSON and sent as ControlCmd packets.

use crate::connection::WriterHandle;
use crate::protocol::{ControlCommand, Packet};
use anyhow::{Context, Result};
use log::{error, info, warn};
use std::io::{self, BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

/// Spawn a thread that reads commands from stdin and sends them to the phone.
///
/// # Arguments
/// * `writer` - Handle for sending packets to the phone
/// * `shutdown` - Shutdown signal (set to true when "quit" is entered)
pub fn spawn_control_thread(
    writer: Arc<WriterHandle>,
    shutdown: Arc<AtomicBool>,
) -> Result<thread::JoinHandle<()>> {
    let handle = thread::Builder::new()
        .name("remote-control".into())
        .spawn(move || {
            if let Err(e) = control_loop(writer, shutdown) {
                error!("Control thread error: {:#}", e);
            }
            info!("Control thread exiting");
        })
        .context("Failed to spawn control thread")?;

    Ok(handle)
}

/// Main control loop — reads commands from stdin, parses them, and sends control packets.
fn control_loop(writer: Arc<WriterHandle>, shutdown: Arc<AtomicBool>) -> Result<()> {
    let stdin = io::stdin();
    let reader = stdin.lock();
    let mut lines = reader.lines();

    // Print help on start
    print_help();

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        // Print prompt
        print!("\x1b[36mcamdroid>\x1b[0m ");
        io::stdout().flush().ok();

        // Read a line from stdin
        let line = match lines.next() {
            Some(Ok(line)) => line,
            Some(Err(e)) => {
                warn!("stdin read error: {}", e);
                break;
            }
            None => {
                // EOF — stdin closed
                info!("stdin closed");
                break;
            }
        };

        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        // Parse and execute the command
        match parse_and_send(&line, &writer) {
            Ok(should_quit) => {
                if should_quit {
                    info!("Quit command received, shutting down...");
                    shutdown.store(true, Ordering::Relaxed);
                    break;
                }
            }
            Err(e) => {
                eprintln!("\x1b[31mError: {}\x1b[0m", e);
            }
        }
    }

    Ok(())
}

/// Parse a command string and send the corresponding control packet.
/// Returns `Ok(true)` if the user wants to quit.
fn parse_and_send(input: &str, writer: &WriterHandle) -> Result<bool> {
    let parts: Vec<&str> = input.split_whitespace().collect();
    if parts.is_empty() {
        return Ok(false);
    }

    let cmd = match parts[0].to_lowercase().as_str() {
        "zoom" => {
            let value = parts
                .get(1)
                .context("Usage: zoom <value> (e.g., zoom 2.5)")?
                .parse::<f32>()
                .context("Zoom value must be a number")?;
            ControlCommand::SetZoom { value }
        }

        "focus" => {
            let mode = parts
                .get(1)
                .context("Usage: focus auto | focus manual <distance>")?
                .to_string();
            match mode.as_str() {
                "auto" => ControlCommand::SetFocus {
                    mode: "auto".to_string(),
                    distance: None,
                    x: None,
                    y: None,
                },
                "manual" => {
                    let distance = parts
                        .get(2)
                        .context("Usage: focus manual <distance> (e.g., focus manual 1.5)")?
                        .parse::<f32>()
                        .context("Focus distance must be a number")?;
                    ControlCommand::SetFocus {
                        mode: "manual".to_string(),
                        distance: Some(distance),
                        x: None,
                        y: None,
                    }
                }
                _ => {
                    anyhow::bail!("Unknown focus mode: {}. Use 'auto' or 'manual'", mode);
                }
            }
        }

        "exposure" | "exp" => {
            let comp = parts
                .get(1)
                .context("Usage: exposure <compensation> (e.g., exposure 2)")?
                .parse::<i32>()
                .context("Exposure compensation must be an integer")?;
            ControlCommand::SetExposure { compensation: comp }
        }

        "wb" | "whitebalance" => {
            let mode = parts
                .get(1)
                .context("Usage: wb <auto|daylight|tungsten|fluorescent|cloudy>")?
                .to_string();
            match mode.as_str() {
                "auto" | "daylight" | "tungsten" | "fluorescent" | "cloudy" => {
                    ControlCommand::SetWhiteBalance { mode }
                }
                _ => {
                    anyhow::bail!(
                        "Unknown WB mode: {}. Use: auto, daylight, tungsten, fluorescent, cloudy",
                        mode
                    );
                }
            }
        }

        "flash" => {
            let state = parts
                .get(1)
                .context("Usage: flash on|off")?
                .to_lowercase();
            let enabled = match state.as_str() {
                "on" | "true" | "1" => true,
                "off" | "false" | "0" => false,
                _ => anyhow::bail!("Usage: flash on|off"),
            };
            ControlCommand::SetFlash { enabled }
        }

        "mirror" | "flip" => {
            // Toggle — we don't track state here, just send the command
            ControlCommand::SetMirror { enabled: true }
        }

        "camera" | "switch" => ControlCommand::SwitchCamera,

        "resolution" | "res" => {
            let value = parts
                .get(1)
                .context("Usage: resolution <1080p|1440p|4k>")?
                .to_string();
            match value.as_str() {
                "1080p" | "1440p" | "4k" | "2k" => {
                    ControlCommand::SetResolution { value }
                }
                _ => anyhow::bail!("Unknown resolution: {}. Use: 1080p, 1440p, 4k", value),
            }
        }

        "fps" => {
            let value = parts
                .get(1)
                .context("Usage: fps <30|60>")?
                .parse::<u32>()
                .context("FPS must be a number")?;
            if value != 30 && value != 60 {
                anyhow::bail!("FPS must be 30 or 60");
            }
            ControlCommand::SetFps { value }
        }

        "codec" => {
            let value = parts
                .get(1)
                .context("Usage: codec <h264|h265|mjpeg>")?
                .to_lowercase();
            match value.as_str() {
                "h264" | "h265" | "hevc" | "mjpeg" => {
                    ControlCommand::SetCodec { value }
                }
                _ => anyhow::bail!("Unknown codec: {}. Use: h264, h265, mjpeg", value),
            }
        }

        "bitrate" | "br" => {
            let value = parts
                .get(1)
                .context("Usage: bitrate <bps> (e.g., bitrate 8000000, or bitrate 0 for auto)")?
                .parse::<u64>()
                .context("Bitrate must be a number")?;
            ControlCommand::SetBitrate { value }
        }

        "keyframe" | "idr" => ControlCommand::RequestKeyframe,

        "status" | "stats" => {
            println!("\x1b[33m[Status request — see log output for current stats]\x1b[0m");
            return Ok(false);
        }

        "help" | "?" => {
            print_help();
            return Ok(false);
        }

        "quit" | "exit" | "q" => {
            // Send stop command to the phone
            let cmd = ControlCommand::Stop;
            let packet = Packet::control_cmd(&cmd)?;
            writer.send(packet).ok(); // Best effort
            return Ok(true);
        }

        _ => {
            eprintln!(
                "\x1b[31mUnknown command: '{}'. Type 'help' for available commands.\x1b[0m",
                parts[0]
            );
            return Ok(false);
        }
    };

    // Serialize and send the control command
    let packet = Packet::control_cmd(&cmd)?;
    writer.send(packet)?;
    println!("\x1b[32m✓ Command sent\x1b[0m");

    Ok(false)
}

/// Print the help message showing all available commands.
fn print_help() {
    println!(
        r#"
┌─────────────────────────────────────────────────────────────┐
│                   CamDroid Remote Control                    │
├─────────────────────────────────────────────────────────────┤
│  Camera Controls:                                           │
│    zoom <value>        Set zoom level (e.g., zoom 2.5)      │
│    focus auto          Auto focus                           │
│    focus manual <dist> Manual focus distance (diopters)      │
│    exposure <ev>       Exposure compensation (-4 to +4)      │
│    wb <mode>           White balance: auto/daylight/         │
│                        tungsten/fluorescent/cloudy           │
│    flash on|off        Toggle flashlight/torch              │
│    mirror              Toggle horizontal flip               │
│    camera              Switch front/rear camera             │
│                                                             │
│  Stream Controls:                                           │
│    resolution <res>    Change: 1080p / 1440p / 4k           │
│    fps <value>         Change: 30 / 60                      │
│    codec <codec>       Change: h264 / h265 / mjpeg          │
│    bitrate <bps>       Set bitrate (0 = auto adaptive)      │
│    keyframe            Request I-frame                      │
│                                                             │
│  Other:                                                     │
│    status              Show stream statistics               │
│    help                Show this help                       │
│    quit                Disconnect and exit                  │
└─────────────────────────────────────────────────────────────┘
"#
    );
}
