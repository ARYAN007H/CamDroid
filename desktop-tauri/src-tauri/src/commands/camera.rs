//! Remote camera controls: typed commands from the UI, validated and mapped to
//! the phone's control protocol.

use crate::error::{CommandError, CommandResult};
use crate::state::AppState;
use camdroid_client::ControlCommand;
use serde::Deserialize;
use tauri::State;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WhiteBalance {
    Auto,
    Daylight,
    Tungsten,
    Fluorescent,
    Cloudy,
}

impl WhiteBalance {
    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Daylight => "daylight",
            Self::Tungsten => "tungsten",
            Self::Fluorescent => "fluorescent",
            Self::Cloudy => "cloudy",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CameraCommand {
    Zoom { value: f32 },
    FocusAuto,
    FocusManual { distance: f32 },
    FocusTap { x: f32, y: f32 },
    Exposure { compensation: i32 },
    WhiteBalance { mode: WhiteBalance },
    Flash { enabled: bool },
    SwitchCamera,
    RequestKeyframe,
}

fn invalid(message: &str) -> CommandError {
    CommandError::Invalid(message.to_string())
}

impl CameraCommand {
    /// Validates the parameters and builds the wire command.
    pub fn into_control(self) -> CommandResult<ControlCommand> {
        Ok(match self {
            Self::Zoom { value } => {
                if !(1.0..=20.0).contains(&value) {
                    return Err(invalid("Zoom must be between 1x and 20x."));
                }
                ControlCommand::SetZoom { value }
            }
            Self::FocusAuto => focus("auto", None, None, None),
            Self::FocusManual { distance } => {
                if !(0.0..=10.0).contains(&distance) {
                    return Err(invalid("Focus distance must be between 0 and 10."));
                }
                focus("manual", Some(distance), None, None)
            }
            Self::FocusTap { x, y } => {
                if !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) {
                    return Err(invalid("Tap-to-focus position must be within the frame."));
                }
                focus("tap", None, Some(x), Some(y))
            }
            Self::Exposure { compensation } => {
                if !(-4..=4).contains(&compensation) {
                    return Err(invalid("Exposure must be between -4 and +4 EV."));
                }
                ControlCommand::SetExposure { compensation }
            }
            Self::WhiteBalance { mode } => ControlCommand::SetWhiteBalance {
                mode: mode.as_str().to_string(),
            },
            Self::Flash { enabled } => ControlCommand::SetFlash { enabled },
            Self::SwitchCamera => ControlCommand::SwitchCamera,
            Self::RequestKeyframe => ControlCommand::RequestKeyframe,
        })
    }
}

fn focus(mode: &str, distance: Option<f32>, x: Option<f32>, y: Option<f32>) -> ControlCommand {
    ControlCommand::SetFocus {
        mode: mode.to_string(),
        distance,
        x,
        y,
    }
}

#[tauri::command]
pub fn camera_command(state: State<'_, AppState>, command: CameraCommand) -> CommandResult<()> {
    let control = command.into_control()?;
    state
        .with_session(|session| session.send_control(&control))?
        .map_err(CommandError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> CameraCommand {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn frontend_payloads_deserialise_into_commands() {
        assert_eq!(
            parse(r#"{"type":"zoom","value":2.5}"#),
            CameraCommand::Zoom { value: 2.5 }
        );
        assert_eq!(parse(r#"{"type":"focus_auto"}"#), CameraCommand::FocusAuto);
        assert_eq!(
            parse(r#"{"type":"white_balance","mode":"tungsten"}"#),
            CameraCommand::WhiteBalance {
                mode: WhiteBalance::Tungsten
            }
        );
        assert!(
            serde_json::from_str::<CameraCommand>(r#"{"type":"white_balance","mode":"neon"}"#)
                .is_err()
        );
        assert!(serde_json::from_str::<CameraCommand>(r#"{"type":"self_destruct"}"#).is_err());
    }

    #[test]
    fn valid_commands_map_to_the_wire_protocol() {
        let exposure = CameraCommand::Exposure { compensation: -2 };
        match exposure.into_control().unwrap() {
            ControlCommand::SetExposure { compensation } => assert_eq!(compensation, -2),
            other => panic!("unexpected {other:?}"),
        }
        let tap = CameraCommand::FocusTap { x: 0.25, y: 0.75 };
        match tap.into_control().unwrap() {
            ControlCommand::SetFocus {
                mode,
                x,
                y,
                distance,
            } => {
                assert_eq!(
                    (mode.as_str(), x, y, distance),
                    ("tap", Some(0.25), Some(0.75), None)
                );
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn out_of_range_values_are_rejected_before_reaching_the_phone() {
        for command in [
            CameraCommand::Zoom { value: 0.5 },
            CameraCommand::Zoom { value: 50.0 },
            CameraCommand::Zoom { value: f32::NAN },
            CameraCommand::FocusManual { distance: -1.0 },
            CameraCommand::FocusTap { x: 1.5, y: 0.5 },
            CameraCommand::Exposure { compensation: 9 },
        ] {
            assert!(
                command.into_control().is_err(),
                "{command:?} should be rejected"
            );
        }
    }
}
