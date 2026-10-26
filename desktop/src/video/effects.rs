//! Spatial effects for packed YUYV frames.
//!
//! Colour grading lives in [`crate::video::filters`] and is a pure per-byte
//! table lookup. The "fun" presets additionally need neighbouring pixels
//! (blocks, scanlines, edges), which is what this module provides.
//!
//! Every effect size is expressed relative to the frame width, so the small
//! in-app preview and the full-resolution virtual camera show the same look.

use crate::video::filters::{FilterPreset, FilterSettings};

/// Limited-range luma bounds used by video-range YUV.
const Y_MIN: f32 = 16.0;
const Y_MAX: f32 = 235.0;

/// Reusable scratch space so effects do not allocate per frame.
#[derive(Debug, Default)]
pub struct EffectEngine {
    luma: Vec<u8>,
    row: Vec<u8>,
}

impl EffectEngine {
    /// Applies the spatial component of `settings` to `frame` in place.
    ///
    /// Presets without a spatial component, a zero strength, or a buffer that
    /// does not match `width` x `height` leave the frame untouched.
    pub fn apply(
        &mut self,
        settings: &FilterSettings,
        frame: &mut [u8],
        width: usize,
        height: usize,
    ) {
        let expected = width * height * 2;
        if width < 4 || height < 2 || width & 1 != 0 || frame.len() < expected {
            return;
        }
        let strength = settings.strength.clamp(0.0, 1.0);
        if strength <= 0.0 {
            return;
        }
        let frame = &mut frame[..expected];
        match settings.preset {
            FilterPreset::Pixel => self.pixelate(frame, width, height, strength),
            FilterPreset::Vhs => self.vhs(frame, width, height, strength),
            FilterPreset::NeonEdge => self.neon_edge(frame, width, height, strength),
            FilterPreset::Cartoon => self.cartoon(frame, width, height, strength),
            FilterPreset::Natural
            | FilterPreset::WarmFilm
            | FilterPreset::TealOrange
            | FilterPreset::Noir
            | FilterPreset::Vintage => {}
        }
    }

    /// Replaces each block with the colour at its centre.
    fn pixelate(&mut self, frame: &mut [u8], width: usize, height: usize, strength: f32) {
        // Even block sizes keep every block aligned to whole YUYV pairs.
        let block = ((width as f32 / 90.0 * strength).round() as usize) & !1;
        if block < 2 {
            return;
        }
        let stride = width * 2;
        self.row.resize(stride, 0);

        let mut top = 0;
        while top < height {
            let rows = block.min(height - top);
            let sample_row = (top + rows / 2).min(height - 1) * stride;

            let mut left = 0;
            while left < width {
                let columns = block.min(width - left);
                let sample_x = ((left + columns / 2) & !1).min(width - 2);
                let source = sample_row + sample_x * 2;
                let (luma, u, v) = (frame[source], frame[source + 1], frame[source + 3]);
                for pair in self.row[left * 2..(left + columns) * 2].chunks_exact_mut(4) {
                    pair.copy_from_slice(&[luma, u, luma, v]);
                }
                left += columns;
            }

            for row in 0..rows {
                frame[(top + row) * stride..][..stride].copy_from_slice(&self.row);
            }
            top += rows;
        }
    }

    /// Dims alternate scanlines and smears chroma to the right, like a worn
    /// analogue tape.
    fn vhs(&mut self, frame: &mut [u8], width: usize, height: usize, strength: f32) {
        let stride = width * 2;
        let pairs = width / 2;
        let smear = ((width as f32 / 160.0 * strength).round() as usize).min(pairs - 1);
        let dim = 1.0 - 0.22 * strength;
        let mut dim_lut = [0_u8; 256];
        for (value, slot) in dim_lut.iter_mut().enumerate() {
            *slot = ((value as f32 - Y_MIN) * dim + Y_MIN).clamp(0.0, 255.0) as u8;
        }
        self.row.resize(stride, 0);

        for y in 0..height {
            let row = &mut frame[y * stride..][..stride];
            if smear > 0 {
                self.row.copy_from_slice(row);
                for pair in 0..pairs {
                    let source = pair.saturating_sub(smear) * 4;
                    row[pair * 4 + 1] = self.row[source + 1];
                    row[pair * 4 + 3] = self.row[source + 3];
                }
            }
            if y % 2 == 1 {
                for pair in row.chunks_exact_mut(4) {
                    pair[0] = dim_lut[usize::from(pair[0])];
                    pair[2] = dim_lut[usize::from(pair[2])];
                }
            }
        }
    }

    /// Dark scene with glowing cyan (vertical) and magenta (horizontal) edges.
    fn neon_edge(&mut self, frame: &mut [u8], width: usize, height: usize, strength: f32) {
        self.extract_luma(frame, width);
        let luma = &self.luma;
        let stride = width * 2;

        for y in 0..height {
            let row = &mut frame[y * stride..][..stride];
            for (pair_index, pair) in row.chunks_exact_mut(4).enumerate() {
                let x0 = pair_index * 2;
                let (mag0, vertical0) = edge_at(luma, width, height, x0, y);
                let (mag1, vertical1) = edge_at(luma, width, height, x0 + 1, y);
                let (magnitude, vertical) = if mag0 >= mag1 {
                    (mag0, vertical0)
                } else {
                    (mag1, vertical1)
                };

                let glow = (magnitude as f32 * 4.0).min(255.0) / 255.0;
                let (du, dv) = if vertical {
                    (64.0, -56.0)
                } else {
                    (40.0, 90.0)
                };
                let neon_u = 128.0 + (f32::from(pair[1]) - 128.0) * 0.35 * (1.0 - glow) + glow * du;
                let neon_v = 128.0 + (f32::from(pair[3]) - 128.0) * 0.35 * (1.0 - glow) + glow * dv;
                let neon_y = |source: u8, edge: i32| -> f32 {
                    let base = (f32::from(source) - Y_MIN) * 0.22 + Y_MIN;
                    let lit = (edge as f32 * 4.0).min(255.0) * 0.85;
                    (base + lit).clamp(Y_MIN, Y_MAX)
                };

                pair[0] = mix(pair[0], neon_y(pair[0], mag0), strength);
                pair[2] = mix(pair[2], neon_y(pair[2], mag1), strength);
                pair[1] = mix(pair[1], neon_u.clamp(0.0, 255.0), strength);
                pair[3] = mix(pair[3], neon_v.clamp(0.0, 255.0), strength);
            }
        }
    }

    /// Flat colour bands with dark ink outlines.
    fn cartoon(&mut self, frame: &mut [u8], width: usize, height: usize, strength: f32) {
        const OUTLINE_THRESHOLD: i32 = 36;
        const LUMA_STEP: f32 = 32.0;
        const CHROMA_STEP: f32 = 20.0;

        self.extract_luma(frame, width);
        let luma = &self.luma;
        let stride = width * 2;
        let posterize_luma = |value: u8| -> f32 {
            ((f32::from(value) / LUMA_STEP).floor() * LUMA_STEP + LUMA_STEP / 2.0)
                .clamp(Y_MIN, Y_MAX)
        };
        let posterize_chroma = |value: u8| -> f32 {
            128.0 + ((f32::from(value) - 128.0) / CHROMA_STEP).round() * CHROMA_STEP
        };

        for y in 0..height {
            let row = &mut frame[y * stride..][..stride];
            for (pair_index, pair) in row.chunks_exact_mut(4).enumerate() {
                let x0 = pair_index * 2;
                let ink0 = edge_at(luma, width, height, x0, y).0 > OUTLINE_THRESHOLD;
                let ink1 = edge_at(luma, width, height, x0 + 1, y).0 > OUTLINE_THRESHOLD;

                let y0 = if ink0 { Y_MIN } else { posterize_luma(pair[0]) };
                let y1 = if ink1 { Y_MIN } else { posterize_luma(pair[2]) };
                let (u, v) = if ink0 && ink1 {
                    (128.0, 128.0)
                } else {
                    (posterize_chroma(pair[1]), posterize_chroma(pair[3]))
                };

                pair[0] = mix(pair[0], y0, strength);
                pair[2] = mix(pair[2], y1, strength);
                pair[1] = mix(pair[1], u.clamp(0.0, 255.0), strength);
                pair[3] = mix(pair[3], v.clamp(0.0, 255.0), strength);
            }
        }
    }

    fn extract_luma(&mut self, frame: &[u8], width: usize) {
        debug_assert_eq!(width % 2, 0);
        self.luma.clear();
        self.luma
            .extend(frame.chunks_exact(2).map(|sample| sample[0]));
    }
}

/// Gradient magnitude at a pixel and whether the edge is mostly vertical
/// (a horizontal luma change). Uses central differences with clamped borders.
fn edge_at(luma: &[u8], width: usize, height: usize, x: usize, y: usize) -> (i32, bool) {
    let at = |px: usize, py: usize| i32::from(luma[py * width + px]);
    let gx = at((x + 1).min(width - 1), y) - at(x.saturating_sub(1), y);
    let gy = at(x, (y + 1).min(height - 1)) - at(x, y.saturating_sub(1));
    (gx.abs() + gy.abs(), gx.abs() >= gy.abs())
}

fn mix(original: u8, effect: f32, strength: f32) -> u8 {
    (f32::from(original) + (effect - f32::from(original)) * strength)
        .round()
        .clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: usize = 360;
    const H: usize = 8;

    /// A frame whose luma ramps with x and whose chroma encodes x too.
    fn ramp() -> Vec<u8> {
        let mut frame = Vec::with_capacity(W * H * 2);
        for _row in 0..H {
            for pair in 0..W / 2 {
                let x = pair * 2;
                frame.extend_from_slice(&[
                    (x % 200 + 20) as u8,
                    (100 + pair % 50) as u8,
                    ((x + 1) % 200 + 20) as u8,
                    (140 + pair % 50) as u8,
                ]);
            }
        }
        frame
    }

    fn settings(preset: FilterPreset, strength: f32) -> FilterSettings {
        FilterSettings::with_strength(preset, strength).unwrap()
    }

    fn luma_at(frame: &[u8], x: usize, y: usize) -> u8 {
        frame[(y * W + x) * 2]
    }

    #[test]
    fn presets_without_a_spatial_component_do_not_touch_the_frame() {
        let mut engine = EffectEngine::default();
        for preset in [
            FilterPreset::Natural,
            FilterPreset::WarmFilm,
            FilterPreset::TealOrange,
            FilterPreset::Noir,
            FilterPreset::Vintage,
        ] {
            let mut frame = ramp();
            engine.apply(&settings(preset, 1.0), &mut frame, W, H);
            assert_eq!(frame, ramp(), "{preset:?} must be colour-only");
        }
    }

    #[test]
    fn zero_strength_and_mismatched_buffers_are_ignored() {
        let mut engine = EffectEngine::default();
        let mut frame = ramp();
        engine.apply(&settings(FilterPreset::Pixel, 0.0), &mut frame, W, H);
        assert_eq!(frame, ramp());

        let mut short = vec![0x80; 16];
        engine.apply(&settings(FilterPreset::Pixel, 1.0), &mut short, W, H);
        assert_eq!(short, vec![0x80; 16]);
    }

    #[test]
    fn pixelate_produces_uniform_blocks_that_repeat_vertically() {
        let mut engine = EffectEngine::default();
        let mut frame = ramp();
        engine.apply(&settings(FilterPreset::Pixel, 1.0), &mut frame, W, H);

        // 360 / 90 = a 4 px block.
        for block_start in (0..W).step_by(4) {
            let reference = luma_at(&frame, block_start, 0);
            for dx in 1..4 {
                assert_eq!(luma_at(&frame, block_start + dx, 0), reference);
            }
        }
        assert_eq!(
            &frame[..W * 2],
            &frame[W * 2..W * 4],
            "rows 0 and 1 share a block"
        );
    }

    #[test]
    fn pixelate_keeps_the_frame_length_and_chroma_alignment() {
        let mut engine = EffectEngine::default();
        let mut frame = ramp();
        engine.apply(&settings(FilterPreset::Pixel, 1.0), &mut frame, W, H);
        assert_eq!(frame.len(), W * H * 2);
    }

    #[test]
    fn vhs_darkens_odd_scanlines_only() {
        let mut engine = EffectEngine::default();
        let mut frame = vec![0_u8; W * H * 2];
        for pair in frame.chunks_exact_mut(4) {
            pair.copy_from_slice(&[180, 128, 180, 128]);
        }
        engine.apply(&settings(FilterPreset::Vhs, 1.0), &mut frame, W, H);

        assert_eq!(luma_at(&frame, 10, 0), 180);
        assert_eq!(luma_at(&frame, 10, 2), 180);
        assert!(luma_at(&frame, 10, 1) < 180);
        assert!(luma_at(&frame, 10, 3) < 180);
    }

    #[test]
    fn vhs_smears_chroma_to_the_right() {
        let mut engine = EffectEngine::default();
        let mut frame = vec![0_u8; W * H * 2];
        for (index, pair) in frame.chunks_exact_mut(4).enumerate() {
            let column = index % (W / 2);
            let (u, v) = if column < 40 { (200, 60) } else { (128, 128) };
            pair.copy_from_slice(&[120, u, 120, v]);
        }
        engine.apply(&settings(FilterPreset::Vhs, 1.0), &mut frame, W, H);

        // The saturated region now bleeds past its original right edge (pair 40).
        let u_after_edge = frame[40 * 4 + 1];
        assert_ne!(u_after_edge, 128, "chroma should have bled into pair 40");
    }

    fn step_edge_frame() -> Vec<u8> {
        let mut frame = Vec::with_capacity(W * H * 2);
        for _row in 0..H {
            for pair in 0..W / 2 {
                let bright = pair * 2 >= W / 2;
                let y = if bright { 200 } else { 60 };
                frame.extend_from_slice(&[y, 128, y, 128]);
            }
        }
        frame
    }

    #[test]
    fn neon_edge_darkens_flat_areas_and_lights_up_edges() {
        let mut engine = EffectEngine::default();
        let mut frame = step_edge_frame();
        engine.apply(&settings(FilterPreset::NeonEdge, 1.0), &mut frame, W, H);

        let flat = luma_at(&frame, 20, 3);
        let edge = luma_at(&frame, W / 2, 3);
        assert!(flat < 60, "flat region should be dimmed, got {flat}");
        assert!(
            edge > flat + 100,
            "edge ({edge}) should glow over flat ({flat})"
        );
    }

    #[test]
    fn neon_edge_tints_vertical_edges_cyan() {
        let mut engine = EffectEngine::default();
        let mut frame = step_edge_frame();
        engine.apply(&settings(FilterPreset::NeonEdge, 1.0), &mut frame, W, H);

        let pair = (W / 2 / 2) * 4;
        assert!(frame[pair + 1] > 160, "cyan has high U");
        assert!(frame[pair + 3] < 100, "cyan has low V");
    }

    #[test]
    fn cartoon_quantises_luma_into_flat_bands() {
        let mut engine = EffectEngine::default();
        let mut frame = ramp();
        engine.apply(&settings(FilterPreset::Cartoon, 1.0), &mut frame, W, H);

        let mut levels: Vec<u8> = frame.chunks_exact(2).map(|sample| sample[0]).collect();
        levels.sort_unstable();
        levels.dedup();
        assert!(levels.len() <= 9, "expected few luma bands, got {levels:?}");
    }

    #[test]
    fn cartoon_inks_outlines_on_strong_edges() {
        let mut engine = EffectEngine::default();
        let mut frame = step_edge_frame();
        engine.apply(&settings(FilterPreset::Cartoon, 1.0), &mut frame, W, H);

        assert_eq!(luma_at(&frame, W / 2, 3), Y_MIN as u8);
    }

    #[test]
    fn partial_strength_blends_toward_the_effect() {
        let mut engine = EffectEngine::default();
        let mut full = step_edge_frame();
        let mut half = step_edge_frame();
        engine.apply(&settings(FilterPreset::NeonEdge, 1.0), &mut full, W, H);
        engine.apply(&settings(FilterPreset::NeonEdge, 0.5), &mut half, W, H);

        let original = luma_at(&step_edge_frame(), 20, 3);
        let flat_full = luma_at(&full, 20, 3);
        let flat_half = luma_at(&half, 20, 3);
        assert!(flat_full < flat_half && flat_half < original);
    }
}
