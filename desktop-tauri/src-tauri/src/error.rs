//! The single error type returned to the frontend.
//!
//! Commands return `Result<T, CommandError>`; it serialises to a plain,
//! human-readable string, which is what the UI shows in a toast.

use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    /// The request was well-formed but not acceptable (bad port, unknown preset...).
    #[error("{0}")]
    Invalid(String),
    #[error("Not connected to a phone.")]
    NotConnected,
    #[error("{0}")]
    Busy(&'static str),
    #[error("{0:#}")]
    Core(#[from] anyhow::Error),
    #[error("{0}")]
    Window(String),
}

impl Serialize for CommandError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl From<tauri::Error> for CommandError {
    fn from(error: tauri::Error) -> Self {
        Self::Window(error.to_string())
    }
}

pub type CommandResult<T> = Result<T, CommandError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_serialise_as_plain_strings_for_the_ui() {
        let json = serde_json::to_string(&CommandError::NotConnected).unwrap();
        assert_eq!(json, "\"Not connected to a phone.\"");

        let json =
            serde_json::to_string(&CommandError::Invalid("port must be 1-65535".into())).unwrap();
        assert_eq!(json, "\"port must be 1-65535\"");
    }

    #[test]
    fn core_errors_keep_their_full_context_chain() {
        let error = anyhow::anyhow!("inner cause").context("outer step");
        let text = CommandError::from(error).to_string();
        assert!(
            text.contains("outer step") && text.contains("inner cause"),
            "{text}"
        );
    }
}
