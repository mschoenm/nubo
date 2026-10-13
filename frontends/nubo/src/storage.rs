//! Where the app keeps data between runs: files in the user's folders on the
//! desktop, localStorage in the browser.

#[cfg(not(target_arch = "wasm32"))]
pub mod files;
#[cfg(target_arch = "wasm32")]
pub mod local_storage;
