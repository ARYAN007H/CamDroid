//! Shared desktop colour/effect state and a deterministic YUYV fallback.
//!
//! The V4L2 sink currently consumes CPU-addressable YUYV frames.  This module
//! keeps filter semantics independent from the eventual GPU upload/readback
//! implementation, while providing a low-overhead fallback on every machine.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterPreset {
    Natural,
    WarmFilm,
    TealOrange,
    Noir,
    Vintage,
    Pixel,
    Vhs,
    NeonEdge,
    Cartoon,
}

impl FilterPreset {
    pub const ALL: [Self; 9] = [
        Self::Natural,
        Self::WarmFilm,
        Self::TealOrange,
        Self::Noir,
        Self::Vintage,
        Self::Pixel,
        Self::Vhs,
        Self::NeonEdge,
        Self::Cartoon,
    ];

    pub fn is_fun(self) -> bool {
        matches!(
            self,
            Self::Pixel | Self::Vhs | Self::NeonEdge | Self::Cartoon
        )
    }
}

/// Accepted ranges for the user-adjustable grade parameters.
pub const TEMPERATURE_RANGE: std::ops::RangeInclusive<f32> = -1.0..=1.0;
pub const SATURATION_RANGE: std::ops::RangeInclusive<f32> = 0.0..=2.5;
pub const CONTRAST_RANGE: std::ops::RangeInclusive<f32> = 0.5..=2.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterSettings {
    pub preset: FilterPreset,
    /// Inclusive opacity/intensity of the selected look.
    pub strength: f32,
    /// Relative white-balance shift: negative is cool, positive is warm.
    pub temperature: f32,
    pub saturation: f32,
    pub contrast: f32,
}

impl FilterSettings {
    pub fn for_preset(preset: FilterPreset) -> Self {
        let (temperature, saturation, contrast) = match preset {
            FilterPreset::Natural => (0.0, 1.0, 1.0),
            FilterPreset::WarmFilm => (0.24, 1.10, 1.06),
            FilterPreset::TealOrange => (0.08, 1.16, 1.12),
            FilterPreset::Noir => (0.0, 0.0, 1.20),
            FilterPreset::Vintage => (0.16, 0.76, 0.90),
            FilterPreset::Pixel => (0.0, 1.08, 1.05),
            FilterPreset::Vhs => (0.10, 1.24, 0.94),
            FilterPreset::NeonEdge => (-0.12, 1.38, 1.28),
            FilterPreset::Cartoon => (0.02, 1.30, 1.18),
        };
        Self {
            preset,
            strength: 1.0,
            temperature,
            saturation,
            contrast,
        }
    }

    pub fn with_strength(preset: FilterPreset, strength: f32) -> Result<Self> {
        if !strength.is_finite() || !(0.0..=1.0).contains(&strength) {
            bail!("filter strength must be a finite value from 0.0 through 1.0");
        }
        Ok(Self {
            strength,
            ..Self::for_preset(preset)
        })
    }

    pub fn validate(self) -> Result<Self> {
        Self::with_strength(self.preset, self.strength)?;
        if !self.temperature.is_finite()
            || !self.saturation.is_finite()
            || !self.contrast.is_finite()
        {
            bail!("filter settings must be finite");
        }
        if !TEMPERATURE_RANGE.contains(&self.temperature) {
            bail!("temperature must be between -1.0 and 1.0");
        }
        if !SATURATION_RANGE.contains(&self.saturation) {
            bail!("saturation must be between 0.0 and 2.5");
        }
        if !CONTRAST_RANGE.contains(&self.contrast) {
            bail!("contrast must be between 0.5 and 2.0");
        }
        Ok(self)
    }

    /// Applies the colour component of a filter to a YUYV pixel pair.
    /// YUYV pairs are `[Y0, U, Y1, V]`; spatial fun effects are applied in
    /// `apply_yuyv_frame` where neighboring pixels are available.
    pub fn apply_yuyv(self, source: [u8; 4]) -> [u8; 4] {
        if self.preset == FilterPreset::Natural || self.strength == 0.0 {
            return source;
        }

        let grade = |y: u8, u: u8, v: u8| -> (u8, u8, u8) {
            let y = ((y as f32 - 128.0) * self.contrast + 128.0).clamp(0.0, 255.0);
            let u = ((u as f32 - 128.0) * self.saturation + 128.0).clamp(0.0, 255.0);
            let v = ((v as f32 - 128.0) * self.saturation + 128.0).clamp(0.0, 255.0);
            let warmth = self.temperature * 24.0;
            let mut out_y = y;
            let mut out_u = (u - warmth * 0.55).clamp(0.0, 255.0);
            let mut out_v = (v + warmth).clamp(0.0, 255.0);

            match self.preset {
                FilterPreset::Noir => {
                    out_u = 128.0;
                    out_v = 128.0;
                }
                FilterPreset::TealOrange => {
                    out_u = (out_u + 5.0).clamp(0.0, 255.0);
                    out_v = (out_v + 5.0).clamp(0.0, 255.0);
                }
                FilterPreset::Vintage => out_y = (out_y + 7.0).clamp(0.0, 255.0),
                FilterPreset::Vhs => {
                    out_u = (out_u + 8.0).clamp(0.0, 255.0);
                    out_v = (out_v - 4.0).clamp(0.0, 255.0);
                }
                FilterPreset::NeonEdge | FilterPreset::Cartoon => {
                    out_y = ((out_y - 128.0) * 1.12 + 128.0).clamp(0.0, 255.0);
                }
                FilterPreset::Natural | FilterPreset::WarmFilm | FilterPreset::Pixel => {}
            }
            (out_y as u8, out_u as u8, out_v as u8)
        };

        let (y0, u, v) = grade(source[0], source[1], source[3]);
        let (y1, _, _) = grade(source[2], source[1], source[3]);
        let mix = |original: u8, filtered: u8| -> u8 {
            (original as f32 + (filtered as f32 - original as f32) * self.strength).round() as u8
        };
        [
            mix(source[0], y0),
            mix(source[1], u),
            mix(source[2], y1),
            mix(source[3], v),
        ]
    }

    /// Builds the per-channel lookup tables equivalent to [`Self::apply_yuyv`].
    ///
    /// The grade treats Y, U and V independently, so each channel is a pure
    /// function of one byte. Precomputing 3x256 entries turns the per-pixel
    /// float maths into three table reads, which is what keeps 1080p60
    /// grading cheap on modest CPUs. The tables are derived from
    /// `apply_yuyv` itself, so the two can never disagree.
    pub fn lut(self) -> ColorLut {
        let mut lut = ColorLut {
            y: [0; 256],
            u: [0; 256],
            v: [0; 256],
            identity: self.preset == FilterPreset::Natural || self.strength == 0.0,
        };
        for value in 0..=255_u8 {
            let index = usize::from(value);
            lut.y[index] = self.apply_yuyv([value, 128, value, 128])[0];
            lut.u[index] = self.apply_yuyv([128, value, 128, 128])[1];
            lut.v[index] = self.apply_yuyv([128, 128, 128, value])[3];
        }
        lut
    }

    /// Applies only the colour component of the filter to a YUYV frame.
    /// Spatial effects live in [`crate::video::effects`].
    pub fn apply_yuyv_frame(self, frame: &mut [u8]) {
        self.lut().apply(frame);
    }
}

/// Precomputed colour grade; see [`FilterSettings::lut`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorLut {
    y: [u8; 256],
    u: [u8; 256],
    v: [u8; 256],
    identity: bool,
}

impl ColorLut {
    /// True when applying the table would leave every frame unchanged.
    pub fn is_identity(&self) -> bool {
        self.identity
    }

    pub fn apply(&self, frame: &mut [u8]) {
        if self.identity {
            return;
        }
        for pair in frame.chunks_exact_mut(4) {
            pair[0] = self.y[usize::from(pair[0])];
            pair[1] = self.u[usize::from(pair[1])];
            pair[2] = self.y[usize::from(pair[2])];
            pair[3] = self.v[usize::from(pair[3])];
        }
    }
}

#[derive(Debug, Clone)]
pub struct FilterController(Arc<RwLock<FilterSettings>>);

impl FilterController {
    pub fn new(settings: FilterSettings) -> Self {
        Self(Arc::new(RwLock::new(
            settings.validate().expect("default filters are valid"),
        )))
    }

    pub fn settings(&self) -> FilterSettings {
        *self.0.read().expect("filter state lock poisoned")
    }

    pub fn set(&self, settings: FilterSettings) -> Result<()> {
        *self.0.write().expect("filter state lock poisoned") = settings.validate()?;
        Ok(())
    }
}

impl Default for FilterController {
    fn default() -> Self {
        Self::new(FilterSettings::for_preset(FilterPreset::Natural))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderBackend {
    CpuFallback,
    GpuAccelerated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RendererStatus {
    pub backend: RenderBackend,
    pub gpu_active: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RendererProbe {
    Available(String),
    Unavailable(String),
}

impl RendererProbe {
    pub fn probe_system() -> Self {
        let dri = Path::new("/dev/dri");
        let found = std::fs::read_dir(dri)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().starts_with("renderD"));
        if found {
            Self::Unavailable(
                "GPU detected; this build uses the low-latency CPU YUYV renderer until the optional GPU backend is enabled".into(),
            )
        } else {
            Self::Unavailable("no GPU render device detected".into())
        }
    }

    pub fn select_backend(self) -> RendererStatus {
        match self {
            Self::Available(detail) => RendererStatus {
                backend: RenderBackend::GpuAccelerated,
                gpu_active: true,
                detail,
            },
            Self::Unavailable(detail) => RendererStatus {
                backend: RenderBackend::CpuFallback,
                gpu_active: false,
                detail: format!("CPU fallback: {detail}"),
            },
        }
    }
}
