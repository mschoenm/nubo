//! Opening a web page in the player's browser, and a folder in their file
//! manager. The desktop build asks the system to open them (with the
//! `opener` crate); the browser build opens web pages in a new tab and has
//! no folders to show.

use std::path::Path;

/// Whether this build can show a folder: only the desktop build can.
pub const CAN_SHOW_FOLDERS: bool = cfg!(feature = "desktop");

/// Opens the web page at `url` in the player's browser, or says why it
/// couldn't.
pub fn open(url: &str) -> Result<(), String> {
    #[cfg(feature = "desktop")]
    {
        opener::open_browser(url).map_err(|error| format!("Couldn't open {url}: {error}"))
    }
    #[cfg(all(feature = "web", not(feature = "desktop")))]
    {
        let window = web_sys::window().ok_or("Couldn't open the page: there is no window")?;
        window
            .open_with_url_and_target(url, "_blank")
            .map(|_| ())
            .map_err(|_| format!("Couldn't open {url}"))
    }
    #[cfg(not(any(feature = "desktop", feature = "web")))]
    {
        Err(format!(
            "Couldn't open {url}: this build can't open web pages"
        ))
    }
}

/// Opens the folder `path` is in, or `path` itself if it is a folder, in the
/// player's file manager, or says why it couldn't.
pub fn show_folder(path: &Path) -> Result<(), String> {
    let folder = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };
    #[cfg(feature = "desktop")]
    {
        opener::open(folder).map_err(|error| format!("Couldn't open {}: {error}", folder.display()))
    }
    #[cfg(not(feature = "desktop"))]
    {
        Err(format!(
            "Couldn't open {}: this build can't show folders",
            folder.display()
        ))
    }
}
