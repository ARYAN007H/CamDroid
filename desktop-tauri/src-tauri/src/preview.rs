//! Streams the rendered preview to the webview.
//!
//! Frames arrive from the core's [`PreviewHub`] as RGB, are JPEG-encoded
//! (about 20x smaller), and are sent over a binary IPC channel. The webview
//! decodes each message into a canvas. Only the newest frame is ever sent, so
//! a slow UI costs frames, never latency.

use anyhow::{Context, Result};
use camdroid_client::{PreviewFrame, PreviewHub, PreviewWait};
use jpeg_encoder::{ColorType, Encoder};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tauri::ipc::{Channel, InvokeResponseBody};

const JPEG_QUALITY: u8 = 82;
const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// Encodes an RGB preview frame as JPEG into `out` (which is cleared first).
pub fn encode_jpeg(frame: &PreviewFrame, out: &mut Vec<u8>) -> Result<()> {
    out.clear();
    let width = u16::try_from(frame.width).context("preview is too wide for JPEG")?;
    let height = u16::try_from(frame.height).context("preview is too tall for JPEG")?;
    Encoder::new(&mut *out, JPEG_QUALITY)
        .encode(&frame.rgb, width, height, ColorType::Rgb)
        .context("JPEG encoding failed")
}

/// A running preview stream; stopping it joins the worker thread.
pub struct PreviewPump {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl PreviewPump {
    pub fn start(hub: PreviewHub, channel: Channel<InvokeResponseBody>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let handle = thread::Builder::new()
            .name("preview-pump".into())
            .spawn(move || pump(&hub, &channel, &worker_stop))
            .expect("spawning the preview thread");
        Self {
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for PreviewPump {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn pump(hub: &PreviewHub, channel: &Channel<InvokeResponseBody>, stop: &AtomicBool) {
    let mut last_sequence = 0;
    let mut jpeg = Vec::new();
    while !stop.load(Ordering::Relaxed) {
        match hub.wait_for_frame(last_sequence, POLL_INTERVAL) {
            PreviewWait::Frame(frame) => {
                last_sequence = frame.sequence;
                if let Err(error) = encode_jpeg(&frame, &mut jpeg) {
                    log::warn!("Skipping a preview frame that could not be encoded: {error:#}");
                    continue;
                }
                // An error means the webview is gone; there is nobody to stream to.
                if channel
                    .send(InvokeResponseBody::Raw(std::mem::take(&mut jpeg)))
                    .is_err()
                {
                    break;
                }
            }
            PreviewWait::TimedOut => {}
            PreviewWait::Closed => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(width: u32, height: u32) -> PreviewFrame {
        let mut rgb = Vec::new();
        for y in 0..height {
            for x in 0..width {
                rgb.extend_from_slice(&[(x * 255 / width) as u8, (y * 255 / height) as u8, 128]);
            }
        }
        PreviewFrame {
            width,
            height,
            rgb,
            sequence: 1,
        }
    }

    #[test]
    fn encoding_produces_a_valid_jpeg_envelope() {
        let mut out = Vec::new();
        encode_jpeg(&gradient(64, 48), &mut out).unwrap();

        assert_eq!(&out[..2], &[0xFF, 0xD8], "JPEG must start with SOI");
        assert_eq!(
            &out[out.len() - 2..],
            &[0xFF, 0xD9],
            "JPEG must end with EOI"
        );
    }

    #[test]
    fn jpeg_is_far_smaller_than_the_raw_frame() {
        let frame = gradient(480, 270);
        let mut out = Vec::new();
        encode_jpeg(&frame, &mut out).unwrap();

        assert!(
            out.len() * 5 < frame.rgb.len(),
            "{} vs {}",
            out.len(),
            frame.rgb.len()
        );
    }

    #[test]
    fn the_output_buffer_is_reused_between_frames() {
        let mut out = Vec::new();
        encode_jpeg(&gradient(64, 48), &mut out).unwrap();
        let first = out.clone();
        encode_jpeg(&gradient(64, 48), &mut out).unwrap();
        assert_eq!(
            out, first,
            "stale bytes from a previous frame must not leak in"
        );
    }

    #[test]
    fn a_frame_with_inconsistent_dimensions_is_an_error_not_a_panic() {
        let mut frame = gradient(64, 48);
        frame.rgb.truncate(10);
        let mut out = Vec::new();
        assert!(encode_jpeg(&frame, &mut out).is_err());
    }
}
