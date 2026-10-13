//! Settings stored as TOML in the browser's localStorage (browser build).

use crate::storage::local_storage::{KEY_PREFIX, local_storage};

/// Reads the saved settings, or returns `None` if there aren't any yet.
pub fn read() -> Result<Option<String>, String> {
    local_storage()?
        .get_item(&key())
        .map_err(|error| format!("{error:?}"))
}

/// Replaces the saved settings.
pub fn write(contents: &str) -> Result<(), String> {
    local_storage()?
        .set_item(&key(), contents)
        .map_err(|error| format!("{error:?}"))
}

fn key() -> String {
    format!("{KEY_PREFIX}settings.toml")
}
