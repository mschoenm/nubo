//! The browser's localStorage, used by the browser build in place of files.

use web_sys::Storage;

/// Put in front of every key, so Nubo's entries don't clash with other
/// apps on the same site.
pub const KEY_PREFIX: &str = "nubo:";

/// The page's localStorage, which can be missing (for example when the user
/// blocks site data).
pub fn local_storage() -> Result<Storage, String> {
    web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .ok_or_else(|| String::from("localStorage is unavailable"))
}
