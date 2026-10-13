//! Iced subscriptions for what the app's other parts report without
//! knowing Iced: they send over `futures` channels, or are asked once per
//! frame in the browser, and these turn that into subscriptions for
//! `App::subscription`.

use crate::controllers::{self, ControllerEvent};
#[cfg(not(target_arch = "wasm32"))]
use crate::emulation::FrameListener;
#[cfg(not(target_arch = "wasm32"))]
use crate::game::Game;
use crate::library::WatchEvent;
use iced::Subscription;
use nubo_frontend::settings::LibraryFolder;

/// News about the controllers: from a thread of their own on the desktop,
/// which sleeps until a controller has something to report.
#[cfg(not(target_arch = "wasm32"))]
pub fn controllers() -> Subscription<ControllerEvent> {
    Subscription::run(|| {
        iced::stream::channel(64, async |output| {
            if controllers::read_on_thread(output) {
                std::future::pending::<()>().await;
            }
        })
    })
}

/// News about the controllers: read on every frame in the browser, which
/// has a single thread.
#[cfg(target_arch = "wasm32")]
pub fn controllers() -> Subscription<ControllerEvent> {
    iced::window::frames().filter_map(|_| controllers::next_event())
}

/// Changes to the ROMs in the library `folders`. It starts over when the
/// folders change.
#[cfg(not(target_arch = "wasm32"))]
pub fn library_changes(folders: &[LibraryFolder]) -> Subscription<WatchEvent> {
    Subscription::run_with(folders.to_vec(), |folders| {
        let folders = folders.clone();
        iced::stream::channel(16, async move |output| {
            crate::library::watch(folders, output).await;
        })
    })
}

/// The browser build has no folders to watch.
#[cfg(target_arch = "wasm32")]
pub fn library_changes(_folders: &[LibraryFolder]) -> Subscription<WatchEvent> {
    Subscription::none()
}

/// Produces an item whenever the running game's emulator finishes a new
/// frame (see `Session::frame_listener`). Each game session is a
/// subscription of its own.
#[cfg(not(target_arch = "wasm32"))]
pub fn new_frames(game: &Game) -> Subscription<()> {
    Subscription::run_with(game.frame_listener(), FrameListener::listen)
}
