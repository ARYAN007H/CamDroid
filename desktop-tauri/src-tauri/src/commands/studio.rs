//! Look and output controls: filters, mirroring, output mode and ring light.
//!
//! Every setter updates the live session (when there is one) and the stored
//! settings, so the choice survives reconnects and restarts.

use crate::error::{CommandError, CommandResult};
use crate::ring_light::{self, RingLightSettings};
use crate::settings::{StreamPreferences, WifiTarget};
use crate::state::AppState;
use camdroid_client::{FilterPreset, FilterSettings, OutputMode};
use serde::Serialize;
use tauri::{AppHandle, State};

/// Everything the studio UI needs to draw itself on start-up.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StudioState {
    pub output_mode: OutputMode,
    pub mirror: bool,
    pub filter: FilterSettings,
    pub ring_light: RingLightSettings,
    pub ring_light_visible: bool,
    pub stream: StreamPreferences,
    pub wifi: WifiTarget,
}

/// A selectable look with the settings it starts from.
#[derive(Debug, Serialize)]
pub struct PresetInfo {
    pub id: FilterPreset,
    pub settings: FilterSettings,
    /// True for effects that reshape the picture (pixelate, edges...) rather
    /// than just grading its colour; they cost more CPU at full resolution.
    pub spatial: bool,
}

/// Every preset in display order. The defaults live in the core so the UI
/// never has to duplicate them.
#[tauri::command]
pub fn filter_presets() -> Vec<PresetInfo> {
    FilterPreset::ALL
        .into_iter()
        .map(|id| PresetInfo {
            id,
            settings: FilterSettings::for_preset(id),
            spatial: id.is_fun(),
        })
        .collect()
}

#[tauri::command]
pub fn get_studio_state(app: AppHandle, state: State<'_, AppState>) -> StudioState {
    let settings = state.settings.snapshot();
    StudioState {
        output_mode: settings.output_mode,
        mirror: settings.mirror,
        filter: settings.filter,
        ring_light: settings.ring_light,
        ring_light_visible: ring_light::is_visible(&app),
        stream: settings.stream,
        wifi: settings.wifi,
    }
}

#[tauri::command]
pub fn set_output_mode(state: State<'_, AppState>, mode: OutputMode) {
    state
        .settings
        .update(|settings| settings.output_mode = mode);
    state.try_with_session(|session| session.output().set(mode));
}

#[tauri::command]
pub fn set_mirror(state: State<'_, AppState>, enabled: bool) {
    state.settings.update(|settings| settings.mirror = enabled);
    state.try_with_session(|session| session.output().set_mirrored(enabled));
}

#[tauri::command]
pub fn set_filter(state: State<'_, AppState>, filter: FilterSettings) -> CommandResult<()> {
    let filter = filter
        .validate()
        .map_err(|error| CommandError::Invalid(error.to_string()))?;
    state.settings.update(|settings| settings.filter = filter);
    if let Some(result) = state.try_with_session(|session| session.filters().set(filter)) {
        result?;
    }
    Ok(())
}

#[tauri::command]
pub fn set_ring_light(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: RingLightSettings,
) -> CommandResult<()> {
    let settings = settings.clamped();
    state.settings.update(|stored| stored.ring_light = settings);
    ring_light::push_update(&app, settings)
}

#[tauri::command]
pub async fn set_ring_light_visible(
    app: AppHandle,
    state: State<'_, AppState>,
    visible: bool,
) -> CommandResult<()> {
    if visible {
        ring_light::show(&app, state.settings.snapshot().ring_light)
    } else {
        ring_light::hide(&app)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_listed_in_core_order_with_valid_defaults() {
        let presets = filter_presets();
        assert_eq!(presets.len(), FilterPreset::ALL.len());
        assert_eq!(presets[0].id, FilterPreset::Natural);
        for preset in &presets {
            assert!(preset.settings.validate().is_ok(), "{:?}", preset.id);
        }
    }

    #[test]
    fn only_the_shape_changing_presets_are_marked_spatial() {
        let spatial: Vec<_> = filter_presets()
            .into_iter()
            .filter(|preset| preset.spatial)
            .map(|preset| preset.id)
            .collect();
        assert_eq!(
            spatial,
            vec![
                FilterPreset::Pixel,
                FilterPreset::Vhs,
                FilterPreset::NeonEdge,
                FilterPreset::Cartoon
            ]
        );
    }

    #[test]
    fn preset_ids_serialise_to_the_names_the_frontend_uses() {
        let json = serde_json::to_value(filter_presets()).unwrap();
        assert_eq!(json[1]["id"], "warm_film");
        assert_eq!(json[7]["id"], "neon_edge");
    }
}
