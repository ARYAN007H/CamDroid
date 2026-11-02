// Cross-platform Virtual Microphone Output (PulseAudio/PipeWire on Linux, non-fatal fallback on macOS/Windows).

use anyhow::{bail, Context, Result};
use crossbeam_channel::Receiver;
use log::{debug, error, info, warn};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

#[cfg(target_os = "linux")]
use libpulse_binding::sample::{Format, Spec};
#[cfg(target_os = "linux")]
use libpulse_binding::stream::Direction;
#[cfg(target_os = "linux")]
use libpulse_simple_binding::Simple;

pub struct PulseOutput {
    module_id: Option<String>,
    sink_name: String,
}

impl PulseOutput {
    pub fn new(sink_name: &str) -> Result<Self> {
        #[cfg(not(target_os = "linux"))]
        {
            bail!(
                "PulseAudio null-sink is Linux-specific; using native standalone audio monitor for {}",
                sink_name
            );
        }

        #[cfg(target_os = "linux")]
        {
            info!("Creating PulseAudio virtual microphone: {}", sink_name);
            Self::try_cleanup_existing(sink_name);

            let output = Command::new("pactl")
                .args([
                    "load-module",
                    "module-null-sink",
                    &format!("sink_name={}", sink_name),
                    &format!("sink_properties=device.description={}_Microphone", sink_name),
                    "rate=44100",
                    "channels=1",
                    "format=s16le",
                ])
                .output()
                .context("Failed to run pactl. Make sure PulseAudio or PipeWire-PulseAudio is installed.")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                bail!("Failed to create PulseAudio null sink: {}", stderr.trim());
            }

            let module_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Ok(Self {
                module_id: Some(module_id),
                sink_name: sink_name.to_string(),
            })
        }
    }

    #[cfg(target_os = "linux")]
    fn try_cleanup_existing(sink_name: &str) {
        if let Ok(output) = Command::new("pactl").args(["list", "short", "modules"]).output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.contains(sink_name) && line.contains("module-null-sink") {
                    if let Some(id) = line.split_whitespace().next() {
                        debug!("Removing stale PulseAudio module: {}", id);
                        let _ = Command::new("pactl").args(["unload-module", id]).output();
                    }
                }
            }
        }
    }

    pub fn sink_name(&self) -> &str {
        &self.sink_name
    }
}

impl Drop for PulseOutput {
    fn drop(&mut self) {
        if let Some(ref module_id) = self.module_id {
            let _ = Command::new("pactl")
                .args(["unload-module", module_id])
                .output();
        }
    }
}

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

#[cfg(target_os = "linux")]
fn pulse_writer_loop(
    pcm_rx: &Receiver<Vec<u8>>,
    sink_name: &str,
    shutdown: Arc<AtomicBool>,
) -> Result<()> {
    let spec = Spec {
        format: Format::S16le,
        channels: 1,
        rate: 44100,
    };

    let pulse = Simple::new(
        None,
        "CamDroid",
        Direction::Playback,
        Some(sink_name),
        "CamDroid Audio",
        &spec,
        None,
        None,
    )
    .map_err(|e| anyhow::anyhow!("Failed to open PulseAudio connection: {}", e))?;

    let silence_10ms = vec![0u8; 882];

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        while pcm_rx.len() > 8 {
            let _ = pcm_rx.try_recv();
        }

        let pcm_data = match pcm_rx.recv_timeout(std::time::Duration::from_millis(25)) {
            Ok(data) => data,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                let _ = pulse.write(&silence_10ms);
                continue;
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        };

        if let Err(e) = pulse.write(&pcm_data) {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }
            warn!("PulseAudio write error: {}", e);
        }
    }

    let _ = pulse.drain();
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn pulse_writer_loop(
    pcm_rx: &Receiver<Vec<u8>>,
    _sink_name: &str,
    shutdown: Arc<AtomicBool>,
) -> Result<()> {
    while !shutdown.load(Ordering::Relaxed) {
        if pcm_rx
            .recv_timeout(std::time::Duration::from_millis(100))
            .is_err()
        {
            continue;
        }
    }
    Ok(())
}
