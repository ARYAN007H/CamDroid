// PulseAudio virtual microphone output.
//
// Creates a null sink via `pactl`, writes decoded PCM audio to it, and the
// sink's monitor source appears as a virtual microphone in applications.
//
// RAII cleanup: the null sink module is unloaded when the PulseOutput is dropped.

use anyhow::{bail, Context, Result};
use crossbeam_channel::Receiver;
use libpulse_binding::sample::{Format, Spec};
use libpulse_binding::stream::Direction;
use libpulse_simple_binding::Simple;
use log::{debug, error, info, warn};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

/// Manages a PulseAudio null sink that serves as a virtual microphone.
///
/// When audio is written to the sink, applications can capture it by selecting
/// the sink's monitor as their microphone input.
pub struct PulseOutput {
    module_id: Option<String>,
    sink_name: String,
}

impl PulseOutput {
    /// Create a new PulseAudio null sink that appears as a virtual microphone.
    ///
    /// # Arguments
    /// * `sink_name` - Name for the PulseAudio sink (e.g., "CamDroid")
    pub fn new(sink_name: &str) -> Result<Self> {
        info!("Creating PulseAudio virtual microphone: {}", sink_name);

        // First, try to remove any existing module with the same name
        // (from a previous unclean shutdown)
        Self::try_cleanup_existing(sink_name);

        // Load the module-null-sink module
        let output = Command::new("pactl")
            .args([
                "load-module",
                "module-null-sink",
                &format!("sink_name={}", sink_name),
                &format!(
                    "sink_properties=device.description={}_Microphone",
                    sink_name
                ),
                "rate=44100",
                "channels=1",
                "format=s16le",
            ])
            .output()
            .context(
                "Failed to run pactl. Make sure PulseAudio or PipeWire-PulseAudio is installed.",
            )?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "Failed to create PulseAudio null sink: {}",
                stderr.trim()
            );
        }

        let module_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
        info!(
            "PulseAudio null sink created: {} (module ID: {})",
            sink_name, module_id
        );
        info!(
            "Virtual microphone available as: {}.monitor",
            sink_name
        );
        info!(
            "Select '{}_Microphone' as your microphone in OBS/Zoom/Discord",
            sink_name
        );

        Ok(Self {
            module_id: Some(module_id),
            sink_name: sink_name.to_string(),
        })
    }

    /// Try to remove an existing sink with the same name (from a previous run).
    fn try_cleanup_existing(sink_name: &str) {
        // List loaded modules and find any with our sink name
        if let Ok(output) = Command::new("pactl").args(["list", "short", "modules"]).output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.contains(sink_name) && line.contains("module-null-sink") {
                    if let Some(id) = line.split_whitespace().next() {
                        debug!("Removing stale PulseAudio module: {}", id);
                        let _ = Command::new("pactl")
                            .args(["unload-module", id])
                            .output();
                    }
                }
            }
        }
    }

    /// Get the sink name for writing audio to.
    pub fn sink_name(&self) -> &str {
        &self.sink_name
    }
}

impl Drop for PulseOutput {
    fn drop(&mut self) {
        if let Some(ref module_id) = self.module_id {
            info!(
                "Removing PulseAudio null sink (module {})",
                module_id
            );
            match Command::new("pactl")
                .args(["unload-module", module_id])
                .output()
            {
                Ok(output) if output.status.success() => {
                    info!("PulseAudio module {} removed", module_id);
                }
                Ok(output) => {
                    warn!(
                        "Failed to remove PulseAudio module {}: {}",
                        module_id,
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
                Err(e) => {
                    error!(
                        "Failed to run pactl to remove module {}: {}",
                        module_id, e
                    );
                }
            }
        }
    }
}

/// Spawn a thread that reads decoded PCM audio and writes it to the PulseAudio sink.
///
/// # Arguments
/// * `pcm_rx` - Channel receiving decoded PCM S16LE mono audio data
/// * `sink_name` - Name of the PulseAudio null sink to write to
/// * `shutdown` - Shutdown signal
pub fn spawn_pulse_writer_thread(
    pcm_rx: Receiver<Vec<u8>>,
    sink_name: String,
    shutdown: Arc<AtomicBool>,
) -> Result<thread::JoinHandle<()>> {
    let handle = thread::Builder::new()
        .name("pulse-writer".into())
        .spawn(move || {
            if let Err(e) = pulse_writer_loop(&pcm_rx, &sink_name, shutdown) {
                error!("PulseAudio writer error: {:#}", e);
            }
            info!("PulseAudio writer thread exiting");
        })
        .context("Failed to spawn PulseAudio writer thread")?;

    Ok(handle)
}

/// Main PulseAudio writer loop — opens a Simple playback connection and writes PCM data.
fn pulse_writer_loop(
    pcm_rx: &Receiver<Vec<u8>>,
    sink_name: &str,
    shutdown: Arc<AtomicBool>,
) -> Result<()> {
    info!("PulseAudio writer starting for sink: {}", sink_name);

    let spec = Spec {
        format: Format::S16le,
        channels: 1, // Mono
        rate: 44100,
    };

    assert!(spec.is_valid(), "Invalid PulseAudio sample spec");

    let pulse = Simple::new(
        None,              // Default server
        "CamDroid",        // Application name
        Direction::Playback, // We're writing audio
        Some(sink_name),   // Target the null sink we created
        "CamDroid Audio",  // Stream description
        &spec,
        None, // Default channel map
        None, // Default buffer attributes
    )
    .map_err(|e| anyhow::anyhow!("Failed to open PulseAudio connection: {}", e))?;

    info!("PulseAudio connection established");

    let mut bytes_written: u64 = 0;

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        let pcm_data = match pcm_rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(data) => data,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                // Write silence to prevent underruns
                let silence = vec![0u8; 4410]; // ~50ms of silence at 44100Hz mono S16LE
                if let Err(e) = pulse.write(&silence) {
                    if !shutdown.load(Ordering::Relaxed) {
                        warn!("PulseAudio write error (silence): {}", e);
                    }
                }
                continue;
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                info!("PCM channel disconnected");
                break;
            }
        };

        if let Err(e) = pulse.write(&pcm_data) {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }
            error!("PulseAudio write error: {}", e);
            // Don't break — try to continue writing
            continue;
        }

        bytes_written += pcm_data.len() as u64;

        if bytes_written % (44100 * 2 * 10) < pcm_data.len() as u64 {
            // Log approximately every 10 seconds of audio
            debug!("Audio: {} bytes written to PulseAudio", bytes_written);
        }
    }

    // Drain remaining audio
    if let Err(e) = pulse.drain() {
        debug!("PulseAudio drain error: {}", e);
    }

    info!(
        "PulseAudio writer: {} total bytes written",
        bytes_written
    );
    Ok(())
}
