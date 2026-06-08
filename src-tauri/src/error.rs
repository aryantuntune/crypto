use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("io error: {0}")] Io(#[from] std::io::Error),
    #[error("sqlite error: {0}")] Sqlite(#[from] rusqlite::Error),
    #[error("http error: {0}")] Http(#[from] reqwest::Error),
    #[error("serde error: {0}")] Serde(#[from] serde_json::Error),
    #[error("keyring error: {0}")] Keyring(#[from] keyring::Error),
    #[error("Anthropic API error: {0}")] Anthropic(String),
    #[error("Daily cost cap reached — raise it in Settings or wait until tomorrow")] CostCapReached,
    #[error("No Anthropic API key set — add it in Settings")] NoApiKey,
    #[error("Not found: {0}")] NotFound(String),
    #[error("{0}")] Invalid(String),
    #[error("Something went wrong: {0}")] Internal(String),
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> std::result::Result<S::Ok, S::Error> {
        ser.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    // These strings are shown to the user verbatim in the frontend; pin the
    // actionable wording so a refactor can't silently degrade the UX.
    #[test]
    fn user_facing_messages_are_actionable() {
        assert_eq!(
            AppError::CostCapReached.to_string(),
            "Daily cost cap reached — raise it in Settings or wait until tomorrow"
        );
        assert_eq!(
            AppError::NoApiKey.to_string(),
            "No Anthropic API key set — add it in Settings"
        );
    }

    #[test]
    fn invalid_passes_message_through_unprefixed() {
        assert_eq!(AppError::Invalid("bad thing".into()).to_string(), "bad thing");
    }

    #[test]
    fn serializes_to_display_string() {
        let json = serde_json::to_string(&AppError::NoApiKey).unwrap();
        assert_eq!(json, "\"No Anthropic API key set — add it in Settings\"");
    }
}
