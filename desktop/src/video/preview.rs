//! Live preview plumbing: shrink a YUYV frame, convert it to RGB, and hand
//! the newest result to whichever consumer is listening.
//!
//! The preview is deliberately small (about 960 px wide). It exists so the
//! user can frame themselves and judge a filter; the full-resolution frame
//! continues to the virtual camera untouched by this path.

use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// Widest preview we produce. Wider sources are decimated by an integer step.
pub const PREVIEW_MAX_WIDTH: usize = 960;

/// One RGB24 preview image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewFrame {
    pub width: u32,
    pub height: u32,
    /// Tightly packed RGB, `width * height * 3` bytes.
    pub rgb: Vec<u8>,
    /// Monotonically increasing; lets consumers skip frames they have seen.
    pub sequence: u64,
}

/// Result of waiting on the hub.
#[derive(Debug)]
pub enum PreviewWait {
    Frame(Arc<PreviewFrame>),
    TimedOut,
    Closed,
}

#[derive(Debug, Default)]
struct Slot {
    frame: Option<Arc<PreviewFrame>>,
    sequence: u64,
    closed: bool,
}

/// Single-slot mailbox: producers overwrite, consumers read the newest.
/// A slow consumer therefore never causes a backlog or adds latency.
#[derive(Debug, Clone, Default)]
pub struct PreviewHub(Arc<(Mutex<Slot>, Condvar)>);

fn locked(mutex: &Mutex<Slot>) -> MutexGuard<'_, Slot> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl PreviewHub {
    pub fn publish(&self, width: u32, height: u32, rgb: Vec<u8>) {
        let (mutex, condvar) = &*self.0;
        let mut slot = locked(mutex);
        slot.sequence += 1;
        let sequence = slot.sequence;
        slot.frame = Some(Arc::new(PreviewFrame {
            width,
            height,
            rgb,
            sequence,
        }));
        condvar.notify_all();
    }

    /// Blocks until a frame newer than `after_sequence` exists, the hub is
    /// closed, or `timeout` elapses.
    pub fn wait_for_frame(&self, after_sequence: u64, timeout: Duration) -> PreviewWait {
        let (mutex, condvar) = &*self.0;
        let guard = locked(mutex);
        let (guard, _) = condvar
            .wait_timeout_while(guard, timeout, |slot| {
                !slot.closed && slot.sequence <= after_sequence
            })
            .unwrap_or_else(PoisonError::into_inner);
        if guard.sequence > after_sequence {
            if let Some(frame) = &guard.frame {
                return PreviewWait::Frame(Arc::clone(frame));
            }
        }
        if guard.closed {
            PreviewWait::Closed
        } else {
            PreviewWait::TimedOut
        }
    }

    pub fn close(&self) {
        let (mutex, condvar) = &*self.0;
        locked(mutex).closed = true;
        condvar.notify_all();
    }
}

/// Integer decimation geometry for a source frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewGeometry {
    pub source_width: usize,
    pub source_height: usize,
    pub step: usize,
    pub width: usize,
    pub height: usize,
}

impl PreviewGeometry {
    pub fn for_source(source_width: usize, source_height: usize) -> Self {
        let step = source_width.div_ceil(PREVIEW_MAX_WIDTH).max(1);
        // YUYV packs two pixels per chroma pair, so the width must stay even.
        let width = (source_width / step) & !1;
        let height = source_height / step;
        Self {
            source_width,
            source_height,
            step,
            width,
            height,
        }
    }

    pub fn yuyv_len(&self) -> usize {
        self.width * self.height * 2
    }
}

/// Nearest-neighbour decimation of a packed YUYV frame into `out`.
///
/// `out` is resized to the preview geometry. Chroma is taken from the source
/// pair that contains the first pixel of each output pair.
pub fn decimate_yuyv(source: &[u8], geometry: PreviewGeometry, out: &mut Vec<u8>) {
    out.clear();
    out.resize(geometry.yuyv_len(), 0x80);
    let source_stride = geometry.source_width * 2;
    if source.len() < source_stride * geometry.source_height {
        return;
    }

    for out_y in 0..geometry.height {
        let source_row = &source[out_y * geometry.step * source_stride..][..source_stride];
        let out_row = &mut out[out_y * geometry.width * 2..][..geometry.width * 2];
        for (pair_index, pair) in out_row.chunks_exact_mut(4).enumerate() {
            let x0 = pair_index * 2 * geometry.step;
            let x1 = x0 + geometry.step;
            let chroma = (x0 / 2) * 4;
            pair[0] = source_row[x0 * 2];
            pair[1] = source_row[chroma + 1];
            pair[2] = source_row[x1 * 2];
            pair[3] = source_row[chroma + 3];
        }
    }
}

/// BT.709 limited-range YUYV to packed RGB24.
pub fn yuyv_to_rgb(yuyv: &[u8], width: usize, height: usize, rgb: &mut Vec<u8>) {
    rgb.clear();
    rgb.resize(width * height * 3, 0);
    let pixel = |y: u8, u: u8, v: u8, out: &mut [u8]| {
        let c = i32::from(y) - 16;
        let d = i32::from(u) - 128;
        let e = i32::from(v) - 128;
        let r = (298 * c + 459 * e + 128) >> 8;
        let g = (298 * c - 55 * d - 136 * e + 128) >> 8;
        let b = (298 * c + 541 * d + 128) >> 8;
        out[0] = r.clamp(0, 255) as u8;
        out[1] = g.clamp(0, 255) as u8;
        out[2] = b.clamp(0, 255) as u8;
    };

    for (pair, out) in yuyv.chunks_exact(4).zip(rgb.chunks_exact_mut(6)) {
        pixel(pair[0], pair[1], pair[3], &mut out[..3]);
        pixel(pair[2], pair[1], pair[3], &mut out[3..]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn geometry_keeps_hd_sources_at_a_sensible_preview_size() {
        let hd = PreviewGeometry::for_source(1920, 1080);
        assert_eq!((hd.step, hd.width, hd.height), (2, 960, 540));

        let uhd = PreviewGeometry::for_source(3840, 2160);
        assert_eq!((uhd.step, uhd.width, uhd.height), (4, 960, 540));

        let small = PreviewGeometry::for_source(640, 360);
        assert_eq!((small.step, small.width, small.height), (1, 640, 360));
    }

    #[test]
    fn geometry_width_is_always_even_for_yuyv_packing() {
        for width in [641, 959, 1281, 1919, 2561] {
            assert_eq!(PreviewGeometry::for_source(width, 100).width % 2, 0);
        }
    }

    #[test]
    fn decimation_samples_the_expected_source_pixels() {
        // 8x2 source, each pixel's Y equals its x so sampling is observable.
        let mut source = Vec::new();
        for _row in 0..2 {
            for pair in 0..4 {
                let x = pair * 2;
                source.extend_from_slice(&[
                    x as u8,
                    100 + pair as u8,
                    (x + 1) as u8,
                    200 + pair as u8,
                ]);
            }
        }
        let geometry = PreviewGeometry {
            source_width: 8,
            source_height: 2,
            step: 2,
            width: 4,
            height: 1,
        };

        let mut out = Vec::new();
        decimate_yuyv(&source, geometry, &mut out);

        // Output pair 0 samples source x=0 and x=2; pair 1 samples x=4 and x=6.
        assert_eq!(out, vec![0, 100, 2, 200, 4, 102, 6, 202]);
    }

    #[test]
    fn decimation_of_a_short_buffer_yields_neutral_grey_instead_of_panicking() {
        let geometry = PreviewGeometry::for_source(1920, 1080);
        let mut out = Vec::new();
        decimate_yuyv(&[0; 16], geometry, &mut out);
        assert_eq!(out.len(), geometry.yuyv_len());
        assert!(out.iter().all(|byte| *byte == 0x80));
    }

    #[test]
    fn rgb_conversion_maps_limited_range_black_white_and_grey() {
        let mut rgb = Vec::new();
        yuyv_to_rgb(&[16, 128, 235, 128], 2, 1, &mut rgb);
        assert_eq!(&rgb[..3], &[0, 0, 0]);
        assert_eq!(&rgb[3..], &[255, 255, 255]);
    }

    #[test]
    fn rgb_conversion_pushes_red_chroma_toward_red() {
        let mut rgb = Vec::new();
        yuyv_to_rgb(&[81, 90, 81, 240], 2, 1, &mut rgb);
        assert!(rgb[0] > 200 && rgb[1] < 60 && rgb[2] < 60, "got {rgb:?}");
    }

    #[test]
    fn hub_delivers_only_frames_newer_than_the_caller_has_seen() {
        let hub = PreviewHub::default();
        assert!(matches!(
            hub.wait_for_frame(0, Duration::from_millis(5)),
            PreviewWait::TimedOut
        ));

        hub.publish(2, 1, vec![1; 6]);
        let PreviewWait::Frame(first) = hub.wait_for_frame(0, Duration::from_millis(5)) else {
            panic!("expected a frame");
        };
        assert_eq!(first.sequence, 1);
        assert!(matches!(
            hub.wait_for_frame(first.sequence, Duration::from_millis(5)),
            PreviewWait::TimedOut
        ));
    }

    #[test]
    fn hub_keeps_only_the_newest_frame_for_a_slow_consumer() {
        let hub = PreviewHub::default();
        hub.publish(2, 1, vec![1; 6]);
        hub.publish(2, 1, vec![2; 6]);
        hub.publish(2, 1, vec![3; 6]);

        let PreviewWait::Frame(frame) = hub.wait_for_frame(0, Duration::from_millis(5)) else {
            panic!("expected a frame");
        };
        assert_eq!(frame.sequence, 3);
        assert_eq!(frame.rgb[0], 3);
    }

    #[test]
    fn closing_the_hub_wakes_a_blocked_consumer() {
        let hub = PreviewHub::default();
        let waiter = hub.clone();
        let handle = thread::spawn(move || waiter.wait_for_frame(0, Duration::from_secs(5)));

        thread::sleep(Duration::from_millis(30));
        hub.close();

        assert!(matches!(handle.join().unwrap(), PreviewWait::Closed));
    }
}
