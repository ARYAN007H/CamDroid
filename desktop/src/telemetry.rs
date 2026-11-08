//! Live pipeline health shared between the worker threads and the UI.
//!
//! Counters are lock-free atomics so the hot decode/render paths never block
//! on a reader; the few string fields sit behind short-lived mutexes.

use serde::Serialize;
use std::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

const BATTERY_UNKNOWN: i32 = -1;

/// State of the optional v4l2loopback output.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(tag = "state", content = "detail", rename_all = "snake_case")]
pub enum VirtualCameraStatus {
    /// Output mode is "app only"; the device is not opened.
    #[default]
    Inactive,
    /// Frames are being written to the device.
    Active,
    /// The device could not be opened or written; the message says why.
    Failed(String),
}

/// A point-in-time copy of all counters, ready to serialise for the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetrySnapshot {
    pub frames_decoded: u64,
    pub frames_dropped: u64,
    pub decode_errors: u64,
    pub render_fps: f32,
    pub battery_percent: Option<u8>,
    pub virtual_camera: VirtualCameraStatus,
    pub audio_error: Option<String>,
    pub disconnect_reason: Option<String>,
}

#[derive(Debug, Default)]
struct Inner {
    frames_decoded: AtomicU64,
    frames_dropped: AtomicU64,
    decode_errors: AtomicU64,
    render_fps_centi: AtomicU32,
    battery_percent: AtomicI32,
    virtual_camera: Mutex<VirtualCameraStatus>,
    audio_error: Mutex<Option<String>>,
    disconnect_reason: Mutex<Option<String>>,
}

/// Cheaply cloneable handle to the shared counters.
#[derive(Debug, Clone)]
pub struct Telemetry(Arc<Inner>);

impl Default for Telemetry {
    fn default() -> Self {
        let inner = Inner::default();
        inner
            .battery_percent
            .store(BATTERY_UNKNOWN, Ordering::Relaxed);
        Self(Arc::new(inner))
    }
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panicking worker must not take the UI's status polling down with it.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Telemetry {
    pub fn record_decoded(&self, count: u64) {
        self.0.frames_decoded.fetch_add(count, Ordering::Relaxed);
    }

    pub fn record_dropped(&self, count: u64) {
        self.0.frames_dropped.fetch_add(count, Ordering::Relaxed);
    }

    pub fn record_decode_error(&self) {
        self.0.decode_errors.fetch_add(1, Ordering::Relaxed);
    }

    pub fn set_render_fps(&self, fps: f32) {
        let centi = (fps.max(0.0) * 100.0).round() as u32;
        self.0.render_fps_centi.store(centi, Ordering::Relaxed);
    }

    pub fn set_battery_percent(&self, percent: u8) {
        self.0
            .battery_percent
            .store(i32::from(percent.min(100)), Ordering::Relaxed);
    }

    pub fn set_virtual_camera(&self, status: VirtualCameraStatus) {
        *locked(&self.0.virtual_camera) = status;
    }

    pub fn set_audio_error(&self, message: impl Into<String>) {
        *locked(&self.0.audio_error) = Some(message.into());
    }

    /// Records why the session ended. Only the first reason is kept, because
    /// later failures are usually consequences of the first.
    pub fn set_disconnect_reason(&self, message: impl Into<String>) {
        let mut reason = locked(&self.0.disconnect_reason);
        if reason.is_none() {
            *reason = Some(message.into());
        }
    }

    pub fn snapshot(&self) -> TelemetrySnapshot {
        let battery = self.0.battery_percent.load(Ordering::Relaxed);
        TelemetrySnapshot {
            frames_decoded: self.0.frames_decoded.load(Ordering::Relaxed),
            frames_dropped: self.0.frames_dropped.load(Ordering::Relaxed),
            decode_errors: self.0.decode_errors.load(Ordering::Relaxed),
            render_fps: self.0.render_fps_centi.load(Ordering::Relaxed) as f32 / 100.0,
            battery_percent: u8::try_from(battery).ok(),
            virtual_camera: locked(&self.0.virtual_camera).clone(),
            audio_error: locked(&self.0.audio_error).clone(),
            disconnect_reason: locked(&self.0.disconnect_reason).clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_is_unknown_until_the_phone_reports_it() {
        let telemetry = Telemetry::default();
        assert_eq!(telemetry.snapshot().battery_percent, None);

        telemetry.set_battery_percent(72);
        assert_eq!(telemetry.snapshot().battery_percent, Some(72));
    }

    #[test]
    fn battery_reports_are_clamped_to_a_percentage() {
        let telemetry = Telemetry::default();
        telemetry.set_battery_percent(250);
        assert_eq!(telemetry.snapshot().battery_percent, Some(100));
    }

    #[test]
    fn only_the_first_disconnect_reason_is_kept() {
        let telemetry = Telemetry::default();
        telemetry.set_disconnect_reason("phone went away");
        telemetry.set_disconnect_reason("writer closed");

        assert_eq!(
            telemetry.snapshot().disconnect_reason.as_deref(),
            Some("phone went away")
        );
    }

    #[test]
    fn counters_accumulate_across_clones() {
        let telemetry = Telemetry::default();
        let worker = telemetry.clone();
        worker.record_decoded(3);
        worker.record_dropped(2);
        worker.record_decode_error();
        worker.set_render_fps(59.94);

        let snapshot = telemetry.snapshot();
        assert_eq!(snapshot.frames_decoded, 3);
        assert_eq!(snapshot.frames_dropped, 2);
        assert_eq!(snapshot.decode_errors, 1);
        assert!((snapshot.render_fps - 59.94).abs() < 0.01);
    }

    #[test]
    fn virtual_camera_status_serialises_with_a_stable_tag() {
        let failed = serde_json::to_value(VirtualCameraStatus::Failed("no device".into())).unwrap();
        assert_eq!(failed["state"], "failed");
        assert_eq!(failed["detail"], "no device");

        let active = serde_json::to_value(VirtualCameraStatus::Active).unwrap();
        assert_eq!(active["state"], "active");
    }
}
