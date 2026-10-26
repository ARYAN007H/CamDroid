use super::filters::{
    FilterController, FilterPreset, FilterSettings, RenderBackend, RendererProbe,
};

#[test]
fn cinematic_preset_has_a_deterministic_warm_grade() {
    let settings = FilterSettings::for_preset(FilterPreset::WarmFilm);

    assert_eq!(settings.preset, FilterPreset::WarmFilm);
    assert!(settings.temperature > 0.0);
    assert!(settings.saturation > 1.0);
}

#[test]
fn filter_settings_reject_an_invalid_strength() {
    assert!(FilterSettings::with_strength(FilterPreset::Noir, 1.01).is_err());
    assert!(FilterSettings::with_strength(FilterPreset::Noir, 0.75).is_ok());
}

#[test]
fn warm_film_changes_a_yuyv_fixture_but_natural_does_not() {
    let source = [100_u8, 128, 150, 128];
    let natural = FilterSettings::for_preset(FilterPreset::Natural);
    let warm = FilterSettings::for_preset(FilterPreset::WarmFilm);

    assert_eq!(natural.apply_yuyv(source), source);
    assert_ne!(warm.apply_yuyv(source), source);
}

#[test]
fn unavailable_gpu_reports_a_truthful_cpu_fallback() {
    let status = RendererProbe::Unavailable("no render device".into()).select_backend();

    assert_eq!(status.backend, RenderBackend::CpuFallback);
    assert!(!status.gpu_active);
    assert!(status.detail.contains("no render device"));
}

#[test]
fn controller_keeps_the_last_valid_selection() {
    let controller = FilterController::new(FilterSettings::for_preset(FilterPreset::Natural));
    controller
        .set(FilterSettings::for_preset(FilterPreset::Vintage))
        .unwrap();

    assert_eq!(controller.settings().preset, FilterPreset::Vintage);
}

#[test]
fn lut_is_bit_identical_to_the_per_pixel_grade_for_every_preset() {
    // Deterministic pseudo-random YUYV pairs covering the whole byte range.
    let mut state = 0x2545_F491_u32;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        (state >> 8) as u8
    };
    let pairs: Vec<[u8; 4]> = (0..4096)
        .map(|_| [next(), next(), next(), next()])
        .collect();

    for preset in FilterPreset::ALL {
        for strength in [0.0, 0.35, 1.0] {
            let settings = FilterSettings::with_strength(preset, strength).unwrap();
            let lut = settings.lut();
            for pair in &pairs {
                let mut via_lut = *pair;
                lut.apply(&mut via_lut);
                assert_eq!(
                    via_lut,
                    settings.apply_yuyv(*pair),
                    "{preset:?} at {strength} diverged for {pair:?}"
                );
            }
        }
    }
}

#[test]
fn natural_lut_is_reported_as_identity_so_callers_can_skip_the_pass() {
    assert!(FilterSettings::for_preset(FilterPreset::Natural)
        .lut()
        .is_identity());
    assert!(FilterSettings::with_strength(FilterPreset::Noir, 0.0)
        .unwrap()
        .lut()
        .is_identity());
    assert!(!FilterSettings::for_preset(FilterPreset::Noir)
        .lut()
        .is_identity());
}

#[test]
fn settings_outside_the_supported_ranges_are_rejected() {
    let base = FilterSettings::for_preset(FilterPreset::Natural);
    for invalid in [
        FilterSettings {
            temperature: 1.5,
            ..base
        },
        FilterSettings {
            saturation: -0.1,
            ..base
        },
        FilterSettings {
            saturation: 3.0,
            ..base
        },
        FilterSettings {
            contrast: 0.2,
            ..base
        },
        FilterSettings {
            contrast: f32::NAN,
            ..base
        },
    ] {
        assert!(
            invalid.validate().is_err(),
            "{invalid:?} should be rejected"
        );
    }
    for preset in FilterPreset::ALL {
        assert!(FilterSettings::for_preset(preset).validate().is_ok());
    }
}

#[test]
fn noir_removes_all_chroma_regardless_of_input() {
    let mut frame = vec![90_u8, 40, 150, 220, 60, 200, 30, 10];
    FilterSettings::for_preset(FilterPreset::Noir).apply_yuyv_frame(&mut frame);

    assert_eq!([frame[1], frame[3], frame[5], frame[7]], [128; 4]);
}
