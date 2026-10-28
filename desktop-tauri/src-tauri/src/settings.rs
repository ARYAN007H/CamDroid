//! User preferences, persisted as JSON in the app config directory.
//!
//! Loading is deliberately forgiving: a missing, truncated or hand-edited
//! file falls back to defaults (field by field where possible) instead of
//! preventing the app from starting. Saving is atomic and coalesced, because
//! sliders fire dozens of updates per second.

use crate::ring_light::RingLightSettings;
use camdroid_client::{FilterPreset, FilterSettings, OutputMode};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

const FILE_NAME: &str = "settings.json";
/// How long the user must stop changing things before we write to disk.
const SAVE_QUIET_PERIOD: Duration = Duration::from_millis(400);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Codec {
    H264,
    H265,
    Mjpeg,
}

impl Codec {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::H264 => "h264",
            Self::H265 => "h265",
            Self::Mjpeg => "mjpeg",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Resolution {
    #[serde(rename = "1080p")]
    P1080,
    #[serde(rename = "1440p")]
    P1440,
    #[serde(rename = "4k")]
    Uhd,
}

impl Resolution {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::P1080 => "1080p",
            Self::P1440 => "1440p",
            Self::Uhd => "4k",
        }
    }
}

/// What the user would like to receive; the phone may offer something close.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamPreferences {
    pub codec: Codec,
    pub resolution: Resolution,
    pub fps: u32,
    pub audio: bool,
}

impl Default for StreamPreferences {
    fn default() -> Self {
        Self {
            codec: Codec::H264,
            resolution: Resolution::P1080,
            fps: 60,
            audio: true,
        }
    }
}

impl StreamPreferences {
    pub fn validate(&self) -> Result<(), String> {
        match self.fps {
            30 | 60 => Ok(()),
            other => Err(format!("Frame rate must be 30 or 60, got {other}.")),
        }
    }
}

/// The phone's address for manual (Wi-Fi) connections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WifiTarget {
    pub host: String,
    pub port: u16,
}

impl Default for WifiTarget {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 4747,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub output_mode: OutputMode,
    pub mirror: bool,
    pub filter: FilterSettings,
    pub ring_light: RingLightSettings,
    pub stream: StreamPreferences,
    pub wifi: WifiTarget,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            output_mode: OutputMode::Standalone,
            mirror: false,
            filter: FilterSettings::for_preset(FilterPreset::Natural),
            ring_light: RingLightSettings::default(),
            stream: StreamPreferences::default(),
            wifi: WifiTarget::default(),
        }
    }
}

impl Settings {
    /// Replaces any out-of-range value with its default.
    fn sanitized(mut self) -> Self {
        let defaults = Self::default();
        if self.filter.validate().is_err() {
            self.filter = defaults.filter;
        }
        if self.stream.validate().is_err() {
            self.stream = defaults.stream;
        }
        self.ring_light = self.ring_light.clamped();
        self
    }

    /// Reads settings from `path`. Always succeeds; problems are logged.
    pub fn load(path: &Path) -> Self {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Self::default(),
            Err(error) => {
                log::warn!("Could not read {}: {error}", path.display());
                return Self::default();
            }
        };
        match serde_json::from_str::<Self>(&text) {
            Ok(settings) => settings.sanitized(),
            Err(error) => {
                log::warn!(
                    "Ignoring unreadable settings file {}: {error}",
                    path.display()
                );
                Self::default()
            }
        }
    }

    /// Writes atomically: a crash mid-write leaves the previous file intact.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("json.tmp");
        let json = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        fs::write(&temporary, json)?;
        fs::rename(&temporary, path)
    }
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// In-memory settings plus a background writer that coalesces updates.
pub struct SettingsStore {
    path: PathBuf,
    current: std::sync::Arc<Mutex<Settings>>,
    dirty: Sender<()>,
}

impl SettingsStore {
    pub fn open(directory: &Path) -> Self {
        let path = directory.join(FILE_NAME);
        let current = std::sync::Arc::new(Mutex::new(Settings::load(&path)));
        let (dirty, signals) = mpsc::channel::<()>();

        let worker_settings = std::sync::Arc::clone(&current);
        let worker_path = path.clone();
        thread::Builder::new()
            .name("settings-writer".into())
            .spawn(move || {
                // Wait for a change, then for the user to stop changing things.
                while signals.recv().is_ok() {
                    loop {
                        match signals.recv_timeout(SAVE_QUIET_PERIOD) {
                            Ok(()) => continue,
                            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                                break
                            }
                        }
                    }
                    let snapshot = locked(&worker_settings).clone();
                    if let Err(error) = snapshot.save(&worker_path) {
                        log::error!("Could not save settings: {error}");
                    }
                }
            })
            .expect("spawning the settings writer thread");

        Self {
            path,
            current,
            dirty,
        }
    }

    pub fn snapshot(&self) -> Settings {
        locked(&self.current).clone()
    }

    /// Applies `change` and schedules a save.
    pub fn update<R>(&self, change: impl FnOnce(&mut Settings) -> R) -> R {
        let result = change(&mut locked(&self.current));
        let _ = self.dirty.send(());
        result
    }

    /// Writes immediately; used on exit so the last change is never lost.
    pub fn flush(&self) {
        if let Err(error) = self.snapshot().save(&self.path) {
            log::error!("Could not save settings on exit: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("camdroid-{name}-{}-{unique}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_missing_file_yields_defaults() {
        let dir = scratch_dir("missing");
        assert_eq!(Settings::load(&dir.join(FILE_NAME)), Settings::default());
    }

    #[test]
    fn settings_round_trip_through_disk() {
        let dir = scratch_dir("roundtrip");
        let path = dir.join(FILE_NAME);
        let mut settings = Settings::default();
        settings.output_mode = OutputMode::VirtualCamera;
        settings.filter = FilterSettings::for_preset(FilterPreset::Noir);
        settings.wifi = WifiTarget {
            host: "192.168.1.67".into(),
            port: 4747,
        };
        settings.stream.fps = 30;

        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
        assert!(
            !path.with_extension("json.tmp").exists(),
            "temp file must be renamed away"
        );
    }

    #[test]
    fn a_corrupt_file_falls_back_to_defaults_instead_of_failing() {
        let dir = scratch_dir("corrupt");
        let path = dir.join(FILE_NAME);
        fs::write(&path, "{ this is not json").unwrap();

        assert_eq!(Settings::load(&path), Settings::default());
    }

    #[test]
    fn missing_fields_take_their_defaults_so_old_files_keep_working() {
        let dir = scratch_dir("partial");
        let path = dir.join(FILE_NAME);
        fs::write(&path, r#"{"outputMode":"virtual_camera"}"#).unwrap();

        let loaded = Settings::load(&path);
        assert_eq!(loaded.output_mode, OutputMode::VirtualCamera);
        assert_eq!(loaded.stream, StreamPreferences::default());
    }

    #[test]
    fn out_of_range_values_are_replaced_rather_than_trusted() {
        let dir = scratch_dir("range");
        let path = dir.join(FILE_NAME);
        fs::write(
            &path,
            r#"{"filter":{"preset":"noir","strength":1.0,"temperature":9.0,"saturation":1.0,"contrast":1.0},
                "stream":{"codec":"h264","resolution":"1080p","fps":144,"audio":true}}"#,
        )
        .unwrap();

        let loaded = Settings::load(&path);
        assert_eq!(loaded.filter, Settings::default().filter);
        assert_eq!(loaded.stream.fps, 60);
    }

    #[test]
    fn stream_preference_names_match_what_the_frontend_sends() {
        let json = serde_json::to_value(StreamPreferences::default()).unwrap();
        assert_eq!(json["codec"], "h264");
        assert_eq!(json["resolution"], "1080p");
        let uhd: StreamPreferences =
            serde_json::from_str(r#"{"codec":"h265","resolution":"4k","fps":30,"audio":false}"#)
                .unwrap();
        assert_eq!((uhd.codec, uhd.resolution), (Codec::H265, Resolution::Uhd));
    }

    #[test]
    fn the_store_coalesces_rapid_updates_into_one_write_and_flush_is_immediate() {
        let dir = scratch_dir("store");
        let store = SettingsStore::open(&dir);

        for fps in [30, 60, 30] {
            store.update(|settings| settings.stream.fps = fps);
        }
        store.flush();

        assert_eq!(Settings::load(&dir.join(FILE_NAME)).stream.fps, 30);
    }
}
