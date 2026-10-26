//! The render stage: the last hop between the decoder and the screen.
//!
//! One thread takes the newest decoded frame, applies the user's filter, and
//! delivers it to up to two places:
//!
//! * the **live preview** shown inside the app, throttled to ~30 fps, and
//! * the **virtual camera** (v4l2loopback), when the output mode asks for it.
//!
//! Work is skipped when nobody needs it: in standalone mode the grade runs on
//! the small preview image rather than the full frame, and frames between two
//! preview ticks are discarded without being touched.

use crate::telemetry::{Telemetry, VirtualCameraStatus};
use crate::video::effects::EffectEngine;
use crate::video::filters::{ColorLut, FilterController, FilterSettings};
use crate::video::output::{OutputController, OutputMode};
use crate::video::preview::{decimate_yuyv, yuyv_to_rgb, PreviewGeometry, PreviewHub};
use crate::video::v4l2_output::V4l2Sink;
use anyhow::{Context, Result};
use crossbeam_channel::Receiver;
use log::{info, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Minimum gap between preview frames (~30 fps).
const PREVIEW_INTERVAL: Duration = Duration::from_millis(33);
/// How long to wait before retrying a virtual camera that failed to open.
const SINK_RETRY_INTERVAL: Duration = Duration::from_secs(2);
/// Window over which the render frame rate is averaged.
const FPS_WINDOW: Duration = Duration::from_secs(1);
/// Neutral YUYV byte used to pad short frames.
const NEUTRAL: u8 = 0x80;

/// Geometry and device for the render stage.
#[derive(Debug, Clone)]
pub struct RenderConfig {
    pub device_path: String,
    pub width: u32,
    pub height: u32,
}

/// Shared handles the render stage reads from and reports to.
#[derive(Debug, Clone)]
pub struct RenderTargets {
    pub filters: FilterController,
    pub output: OutputController,
    pub preview: PreviewHub,
    pub telemetry: Telemetry,
}

/// Spawns the render thread. It exits when `shutdown` is set or the frame
/// channel closes, and always closes the preview hub on the way out.
pub fn spawn_render_thread(
    config: RenderConfig,
    frame_rx: Receiver<Vec<u8>>,
    targets: RenderTargets,
    shutdown: Arc<AtomicBool>,
) -> Result<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name("render".into())
        .spawn(move || {
            render_loop(config, frame_rx, targets, shutdown);
            info!("Render thread exiting");
        })
        .context("Failed to spawn render thread")
}

fn render_loop(
    config: RenderConfig,
    frame_rx: Receiver<Vec<u8>>,
    targets: RenderTargets,
    shutdown: Arc<AtomicBool>,
) {
    let preview = targets.preview.clone();
    let telemetry = targets.telemetry.clone();
    let mut renderer = Renderer::new(config, targets);

    while !shutdown.load(Ordering::Relaxed) {
        match frame_rx.recv_timeout(Duration::from_millis(10)) {
            Ok(first) => {
                let (newest, skipped) = take_newest_frame(first, &frame_rx);
                telemetry.record_dropped(skipped);
                renderer.process(newest, Instant::now());
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
    }

    preview.close();
    telemetry.set_virtual_camera(VirtualCameraStatus::Inactive);
}

/// Drains the channel and keeps only the newest frame, so a stall in the
/// output never turns into latency. Returns the frame and how many older
/// frames were discarded.
fn take_newest_frame(mut newest: Vec<u8>, frame_rx: &Receiver<Vec<u8>>) -> (Vec<u8>, u64) {
    let mut skipped = 0;
    while let Ok(next) = frame_rx.try_recv() {
        newest = next;
        skipped += 1;
    }
    (newest, skipped)
}

/// Pads (with neutral grey) or truncates a frame to exactly `expected` bytes.
fn normalize_frame(frame: &mut Vec<u8>, expected: usize) {
    frame.resize(expected, NEUTRAL);
}

struct Renderer {
    config: RenderConfig,
    targets: RenderTargets,
    geometry: PreviewGeometry,
    effects: EffectEngine,
    lut: Option<(FilterSettings, ColorLut)>,
    sink: Option<V4l2Sink>,
    sink_retry_at: Option<Instant>,
    sink_open_attempts: u32,
    last_preview: Option<Instant>,
    small: Vec<u8>,
    rgb: Vec<u8>,
    fps_window_start: Option<Instant>,
    fps_window_frames: u32,
}

impl Renderer {
    fn new(config: RenderConfig, targets: RenderTargets) -> Self {
        let geometry = PreviewGeometry::for_source(config.width as usize, config.height as usize);
        Self {
            config,
            targets,
            geometry,
            effects: EffectEngine::default(),
            lut: None,
            sink: None,
            sink_retry_at: None,
            sink_open_attempts: 0,
            last_preview: None,
            small: Vec::new(),
            rgb: Vec::new(),
            fps_window_start: None,
            fps_window_frames: 0,
        }
    }

    fn process(&mut self, mut frame: Vec<u8>, now: Instant) {
        let expected = (self.config.width as usize) * (self.config.height as usize) * 2;
        if frame.len() != expected {
            warn!(
                "Frame size mismatch: got {} bytes, expected {} ({}x{} YUYV)",
                frame.len(),
                expected,
                self.config.width,
                self.config.height
            );
            normalize_frame(&mut frame, expected);
        }

        let settings = self.targets.filters.settings();
        self.refresh_lut(settings);
        let preview_due = self.preview_due(now);

        let wants_virtual_camera = self.targets.output.mode() == OutputMode::VirtualCamera;
        if wants_virtual_camera && self.ensure_sink(now) {
            self.grade_full_frame(&settings, &mut frame);
            self.write_to_sink(&frame, now);
            if preview_due {
                self.publish_preview(&frame, now);
            }
        } else {
            if !wants_virtual_camera {
                self.release_sink();
            }
            if preview_due {
                self.publish_graded_preview(&settings, &frame, now);
            }
        }

        self.count_frame(now);
    }

    fn refresh_lut(&mut self, settings: FilterSettings) {
        if self.lut.as_ref().map(|(cached, _)| *cached) != Some(settings) {
            self.lut = Some((settings, settings.lut()));
        }
    }

    fn preview_due(&self, now: Instant) -> bool {
        self.last_preview
            .is_none_or(|last| now.duration_since(last) >= PREVIEW_INTERVAL)
    }

    /// Returns true when a writable virtual camera is available, opening it if
    /// needed. Failed opens are reported through telemetry and retried slowly.
    fn ensure_sink(&mut self, now: Instant) -> bool {
        if self.sink.is_some() {
            return true;
        }
        if self.sink_retry_at.is_some_and(|retry_at| now < retry_at) {
            return false;
        }

        self.sink_open_attempts += 1;
        match V4l2Sink::open(
            &self.config.device_path,
            self.config.width,
            self.config.height,
        ) {
            Ok(sink) => {
                self.sink = Some(sink);
                self.sink_retry_at = None;
                self.targets
                    .telemetry
                    .set_virtual_camera(VirtualCameraStatus::Active);
                true
            }
            Err(error) => {
                warn!("Virtual camera unavailable: {error:#}");
                self.sink_retry_at = Some(now + SINK_RETRY_INTERVAL);
                self.targets
                    .telemetry
                    .set_virtual_camera(VirtualCameraStatus::Failed(format!("{error:#}")));
                false
            }
        }
    }

    fn release_sink(&mut self) {
        if self.sink.take().is_some() {
            info!("Virtual camera released");
        }
        self.sink_retry_at = None;
        self.targets
            .telemetry
            .set_virtual_camera(VirtualCameraStatus::Inactive);
    }

    fn write_to_sink(&mut self, frame: &[u8], now: Instant) {
        let Some(sink) = self.sink.as_mut() else {
            return;
        };
        if let Err(error) = sink.write_frame(frame) {
            warn!("Virtual camera write failed: {error}");
            self.sink = None;
            self.sink_retry_at = Some(now + SINK_RETRY_INTERVAL);
            self.targets
                .telemetry
                .set_virtual_camera(VirtualCameraStatus::Failed(format!(
                    "Writing to {} failed: {error}",
                    self.config.device_path
                )));
        }
    }

    fn grade_full_frame(&mut self, settings: &FilterSettings, frame: &mut [u8]) {
        if let Some((_, lut)) = &self.lut {
            lut.apply(frame);
        }
        self.effects.apply(
            settings,
            frame,
            self.config.width as usize,
            self.config.height as usize,
        );
    }

    /// Preview of an already-graded full frame.
    fn publish_preview(&mut self, graded: &[u8], now: Instant) {
        decimate_yuyv(graded, self.geometry, &mut self.small);
        self.convert_and_publish(now);
    }

    /// Preview of a raw frame: shrink first, then grade the small image.
    fn publish_graded_preview(&mut self, settings: &FilterSettings, raw: &[u8], now: Instant) {
        decimate_yuyv(raw, self.geometry, &mut self.small);
        if let Some((_, lut)) = &self.lut {
            lut.apply(&mut self.small);
        }
        self.effects.apply(
            settings,
            &mut self.small,
            self.geometry.width,
            self.geometry.height,
        );
        self.convert_and_publish(now);
    }

    fn convert_and_publish(&mut self, now: Instant) {
        yuyv_to_rgb(
            &self.small,
            self.geometry.width,
            self.geometry.height,
            &mut self.rgb,
        );
        self.targets.preview.publish(
            self.geometry.width as u32,
            self.geometry.height as u32,
            std::mem::take(&mut self.rgb),
        );
        self.last_preview = Some(now);
    }

    fn count_frame(&mut self, now: Instant) {
        let start = *self.fps_window_start.get_or_insert(now);
        self.fps_window_frames += 1;
        let elapsed = now.duration_since(start);
        if elapsed >= FPS_WINDOW {
            self.targets
                .telemetry
                .set_render_fps(self.fps_window_frames as f32 / elapsed.as_secs_f32());
            self.fps_window_start = Some(now);
            self.fps_window_frames = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::filters::FilterPreset;
    use crate::video::preview::PreviewWait;
    use crossbeam_channel::bounded;

    const W: u32 = 64;
    const H: u32 = 4;

    fn targets(mode: OutputMode) -> RenderTargets {
        RenderTargets {
            filters: FilterController::default(),
            output: OutputController::new(mode),
            preview: PreviewHub::default(),
            telemetry: Telemetry::default(),
        }
    }

    fn renderer(targets: &RenderTargets) -> Renderer {
        Renderer::new(
            RenderConfig {
                device_path: "/dev/camdroid-test-missing".into(),
                width: W,
                height: H,
            },
            targets.clone(),
        )
    }

    /// Mid-grey luma with a strong colour cast, so grading is observable.
    fn coloured_frame() -> Vec<u8> {
        [120_u8, 90, 120, 200].repeat((W * H / 2) as usize)
    }

    fn latest(
        targets: &RenderTargets,
        after: u64,
    ) -> Option<std::sync::Arc<crate::video::preview::PreviewFrame>> {
        match targets
            .preview
            .wait_for_frame(after, Duration::from_millis(1))
        {
            PreviewWait::Frame(frame) => Some(frame),
            _ => None,
        }
    }

    #[test]
    fn standalone_mode_publishes_a_preview_and_never_opens_the_device() {
        let targets = targets(OutputMode::Standalone);
        let mut renderer = renderer(&targets);

        renderer.process(coloured_frame(), Instant::now());

        let frame = latest(&targets, 0).expect("preview published");
        assert_eq!((frame.width, frame.height), (W, H));
        assert_eq!(frame.rgb.len(), (W * H * 3) as usize);
        assert_eq!(renderer.sink_open_attempts, 0);
        assert_eq!(
            targets.telemetry.snapshot().virtual_camera,
            VirtualCameraStatus::Inactive
        );
    }

    #[test]
    fn preview_is_throttled_to_about_thirty_frames_per_second() {
        let targets = targets(OutputMode::Standalone);
        let mut renderer = renderer(&targets);
        let start = Instant::now();

        renderer.process(coloured_frame(), start);
        renderer.process(coloured_frame(), start + Duration::from_millis(10));
        assert_eq!(latest(&targets, 0).unwrap().sequence, 1);

        renderer.process(coloured_frame(), start + Duration::from_millis(40));
        assert_eq!(latest(&targets, 0).unwrap().sequence, 2);
    }

    #[test]
    fn the_selected_filter_is_visible_in_the_preview() {
        let targets = targets(OutputMode::Standalone);
        let mut renderer = renderer(&targets);
        let start = Instant::now();

        renderer.process(coloured_frame(), start);
        let natural = latest(&targets, 0).unwrap();
        assert!(natural.rgb[0] != natural.rgb[2], "source has a colour cast");

        targets
            .filters
            .set(FilterSettings::for_preset(FilterPreset::Noir))
            .unwrap();
        renderer.process(coloured_frame(), start + Duration::from_millis(100));
        let noir = latest(&targets, natural.sequence).unwrap();
        assert!(
            noir.rgb
                .chunks_exact(3)
                .all(|p| p[0] == p[1] && p[1] == p[2]),
            "noir must be neutral grey"
        );
    }

    #[test]
    fn a_missing_virtual_camera_is_reported_but_the_preview_keeps_working() {
        let targets = targets(OutputMode::VirtualCamera);
        let mut renderer = renderer(&targets);

        renderer.process(coloured_frame(), Instant::now());

        assert!(matches!(
            targets.telemetry.snapshot().virtual_camera,
            VirtualCameraStatus::Failed(_)
        ));
        assert!(
            latest(&targets, 0).is_some(),
            "preview must survive a failed sink"
        );
    }

    #[test]
    fn a_failed_virtual_camera_is_retried_slowly_not_every_frame() {
        let targets = targets(OutputMode::VirtualCamera);
        let mut renderer = renderer(&targets);
        let start = Instant::now();

        for step in 0..20 {
            renderer.process(coloured_frame(), start + Duration::from_millis(step * 40));
        }
        assert_eq!(
            renderer.sink_open_attempts, 1,
            "no retries inside the backoff window"
        );

        renderer.process(
            coloured_frame(),
            start + SINK_RETRY_INTERVAL + Duration::from_millis(50),
        );
        assert_eq!(renderer.sink_open_attempts, 2);
    }

    #[test]
    fn switching_back_to_standalone_clears_a_previous_failure() {
        let targets = targets(OutputMode::VirtualCamera);
        let mut renderer = renderer(&targets);
        let start = Instant::now();
        renderer.process(coloured_frame(), start);
        assert!(matches!(
            targets.telemetry.snapshot().virtual_camera,
            VirtualCameraStatus::Failed(_)
        ));

        targets.output.set(OutputMode::Standalone);
        renderer.process(coloured_frame(), start + Duration::from_millis(50));

        assert_eq!(
            targets.telemetry.snapshot().virtual_camera,
            VirtualCameraStatus::Inactive
        );
    }

    #[test]
    fn short_and_long_frames_are_normalised_instead_of_panicking() {
        let targets = targets(OutputMode::Standalone);
        let mut renderer = renderer(&targets);
        let start = Instant::now();

        renderer.process(vec![16; 10], start);
        renderer.process(
            vec![16; (W * H * 2) as usize + 500],
            start + Duration::from_millis(100),
        );

        assert_eq!(latest(&targets, 0).unwrap().sequence, 2);
    }

    #[test]
    fn render_fps_is_published_once_per_window() {
        let targets = targets(OutputMode::Standalone);
        let mut renderer = renderer(&targets);
        let start = Instant::now();

        for frame in 0..=60 {
            renderer.process(
                coloured_frame(),
                start + Duration::from_millis(frame * 1000 / 60),
            );
        }

        let fps = targets.telemetry.snapshot().render_fps;
        assert!((fps - 60.0).abs() < 2.0, "expected ~60 fps, got {fps}");
    }

    #[test]
    fn newest_frame_is_selected_before_any_filter_work() {
        let (tx, rx) = bounded(3);
        tx.send(vec![1]).unwrap();
        tx.send(vec![2]).unwrap();
        tx.send(vec![3]).unwrap();

        let (newest, skipped) = take_newest_frame(vec![0], &rx);
        assert_eq!(newest, vec![3]);
        assert_eq!(skipped, 3);
    }

    #[test]
    fn normalize_pads_with_neutral_grey_and_truncates() {
        let mut short = vec![1, 2];
        normalize_frame(&mut short, 4);
        assert_eq!(short, vec![1, 2, NEUTRAL, NEUTRAL]);

        let mut long = vec![1, 2, 3, 4, 5];
        normalize_frame(&mut long, 3);
        assert_eq!(long, vec![1, 2, 3]);
    }
}
