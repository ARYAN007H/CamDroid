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
