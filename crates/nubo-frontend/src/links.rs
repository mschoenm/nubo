//! Opening a web page in the player's browser, and a folder in their file
//! manager. The desktop build asks the system to open them (with the
//! `opener` crate); the browser build opens web pages in a new tab and has
//! no folders to show, but hands files to the browser as downloads
//! (`download`).

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

/// Hands `contents` to the browser as a download called `file_name`, of the
/// media type `media_type` (such as `image/png`), as a link with a
/// `download` attribute that the page clicks.
#[cfg(all(feature = "web", not(feature = "desktop")))]
pub fn download(file_name: &str, media_type: &str, contents: &[u8]) -> Result<(), String> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;

    let fail = |_| String::from("the browser didn't take the download");
    let window = web_sys::window().ok_or("there is no window")?;
    let document = window.document().ok_or("there is no page")?;

    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(contents));
    let options = web_sys::BlobPropertyBag::new();
    options.set_type(media_type);
    let blob =
        web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options).map_err(fail)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(fail)?;

    let link: web_sys::HtmlAnchorElement = document
        .create_element("a")
        .map_err(fail)?
        .dyn_into()
        .map_err(|_| String::from("the page made no link"))?;
    link.set_href(&url);
    link.set_download(file_name);
    link.click();

    // The browser reads the file after the click, so its address is freed a
    // little later.
    let revoke = Closure::once_into_js(move || {
        let _ = web_sys::Url::revoke_object_url(&url);
    });
    let _ = window
        .set_timeout_with_callback_and_timeout_and_arguments_0(revoke.unchecked_ref(), 10_000);
    Ok(())
}
