//! Application-wide error type.
//!
//! Implements [`Serialize`](serde::Serialize) so Tauri can forward errors to the
//! frontend as a string (the conventional shape expected by `invoke` rejection).

use std::fmt;

/// A stringy error variant that serializes to its display string, allowing
/// arbitrary error sources to cross the IPC boundary.
///
/// Some variants are reserved for phases that haven't been wired yet; they're
/// allowed to be dead code during scaffolding.
#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum AppError {
    #[error("IO error: {0}")]
    Io(String),

    #[error("JSON error: {0}")]
    Json(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Path error: {0}")]
    Path(String),

    #[error("Tauri error: {0}")]
    Tauri(String),

    #[error("{0}")]
    Other(String),
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Json(e.to_string())
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        AppError::Database(e.to_string())
    }
}

impl From<tauri::Error> for AppError {
    fn from(e: tauri::Error) -> Self {
        AppError::Tauri(e.to_string())
    }
}

// Serialize as the display string so the frontend receives a plain message.
impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.to_string().as_ref())
    }
}

/// `Result` alias used throughout the backend.
pub type AppResult<T> = Result<T, AppError>;

/// Helper to attach a context string to an IO error before conversion.
pub fn io_context<C: fmt::Display>(context: C, source: std::io::Error) -> AppError {
    AppError::Io(format!("{context}: {source}"))
}
