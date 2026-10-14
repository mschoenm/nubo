//! Iced subscriptions for what the app's other parts and `nubo-frontend`
//! report without knowing Iced: they send over `futures` channels, call
//! back, or are asked once per frame in the browser, and these turn that
//! into subscriptions for `App::subscription`.

#[cfg(not(target_arch = "wasm32"))]
use crate::game::Game;
use iced::Subscription;
#[cfg(not(target_arch = "wasm32"))]
use iced::futures::{SinkExt, executor};
use nubo_frontend::gamepads::{self, ControllerEvent};
use nubo_frontend::library::WatchEvent;
use nubo_frontend::settings::LibraryFolder;

/// News about the controllers: from a thread of their own on the desktop,
/// which sleeps until a controller has something to report.
#[cfg(not(target_arch = "wasm32"))]
pub fn controllers() -> Subscription<ControllerEvent> {
    Subscription::run(|| {
        iced::stream::channel(64, async |output| {
            if gamepads::read_on_thread(output) {
                std::future::pending::<()>().await;
            }
        })
    })
}

/// News about the controllers: read on every frame in the browser, which
/// has a single thread.
#[cfg(target_arch = "wasm32")]
pub fn controllers() -> Subscription<ControllerEvent> {
    iced::window::frames().filter_map(|_| gamepads::next_event())
}

/// Changes to the ROMs in the library `folders`, which the folder watcher
/// (`nubo_frontend::library::watch`) reports from a thread of its own. It
/// starts over when the folders change, and watching stops when the
/// subscription ends, which drops the watcher.
#[cfg(not(target_arch = "wasm32"))]
pub fn library_changes(folders: &[LibraryFolder]) -> Subscription<WatchEvent> {
    Subscription::run_with(folders.to_vec(), |folders| {
        let folders = folders.clone();
        iced::stream::channel(16, async move |mut output| {
            // The watcher's thread may wait for room in the channel; this
            // task, which empties it, must not.
            let changes = output.clone();
            let watching = nubo_frontend::library::watch(&folders, move |event| {
                let _ = executor::block_on(changes.clone().send(event));
            });
            for failure in watching.failures {
                let _ = output.send(WatchEvent::Failed(failure)).await;
            }
            if watching.watcher.is_some() {
                std::future::pending::<()>().await;
            }
        })
    })
}

/// The browser build has no folders to watch.
#[cfg(target_arch = "wasm32")]
pub fn library_changes(_folders: &[LibraryFolder]) -> Subscription<WatchEvent> {
    Subscription::none()
}

/// Produces an item whenever the running game's emulator finishes a new
/// frame (see `GameSession::frame_listener`). Each game session is a
/// subscription of its own.
///
/// The session calls back on the emulator thread for each new frame, and
/// once as the subscription starts. The callback only tries to send into a
/// channel that holds one message, and never waits: calls the app hasn't
/// handled yet merge into one, since the app always takes the newest frame.
#[cfg(not(target_arch = "wasm32"))]
pub fn new_frames(game: &Game) -> Subscription<()> {
    Subscription::run_with(game.session.frame_listener(), |listener| {
        let (mut sender, receiver) = iced::futures::channel::mpsc::channel(0);
        listener.listen(move || {
            let _ = sender.try_send(());
        });
        receiver
    })
}
