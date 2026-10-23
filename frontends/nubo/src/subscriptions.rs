//! Iced subscriptions for what the app's other parts and `nubo-frontend`
//! report without knowing Iced: they send over `futures` channels, call
//! back, or are asked once per frame in the browser, and these turn that
//! into subscriptions for `App::subscription`, as well as the Unix signals
//! that ask the app to stop.

#[cfg(not(target_arch = "wasm32"))]
use crate::game::Game;
use iced::Subscription;
#[cfg(not(target_arch = "wasm32"))]
use iced::futures::{SinkExt, executor};
use nubo_frontend::gamepads::{self, ControllerEvent};
use nubo_frontend::library::WatchEvent;
use nubo_frontend::settings::LibraryFolder;
#[cfg(unix)]
use signal_hook::{consts::TERM_SIGNALS, flag, iterator::Signals};
use std::path::PathBuf;
#[cfg(unix)]
use std::sync::{Arc, atomic::AtomicBool};

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

/// Changes to the ROMs in the library `folders` and to the files of
/// `games`, the recently played games outside them, which the folder
/// watcher (`nubo_frontend::library::watch`) reports from a thread of its
/// own. It starts over when the folders or those games change, and watching
/// stops when the subscription ends, which drops the watcher.
#[cfg(not(target_arch = "wasm32"))]
pub fn library_changes(folders: &[LibraryFolder], games: Vec<PathBuf>) -> Subscription<WatchEvent> {
    Subscription::run_with((folders.to_vec(), games), |(folders, games)| {
        let (folders, games) = (folders.clone(), games.clone());
        iced::stream::channel(16, async move |mut output| {
            // The watcher's thread may wait for room in the channel; this
            // task, which empties it, must not.
            let changes = output.clone();
            let watching = nubo_frontend::library::watch(&folders, &games, move |event| {
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
pub fn library_changes(
    _folders: &[LibraryFolder],
    _games: Vec<PathBuf>,
) -> Subscription<WatchEvent> {
    Subscription::none()
}

/// Produces an item when the system asks the app to stop with a Unix
/// signal: SIGTERM (sent by `kill` and by logging out or shutting down),
/// SIGINT (Ctrl+C in the terminal it was started from) or SIGQUIT. The app
/// then closes as if its window were closed, which saves where the player
/// left off. A second signal ends the process at once, in case closing gets
/// stuck.
#[cfg(unix)]
pub fn stop_requests() -> Subscription<()> {
    Subscription::run(|| {
        iced::stream::channel(1, async |mut output| {
            let asked_before = Arc::new(AtomicBool::new(false));
            for &signal in TERM_SIGNALS {
                // The shutdown goes first, so it only fires once the flag
                // is set by an earlier signal.
                let _ = flag::register_conditional_shutdown(signal, 1, Arc::clone(&asked_before));
                let _ = flag::register(signal, Arc::clone(&asked_before));
            }
            let Ok(mut signals) = Signals::new(TERM_SIGNALS) else {
                return;
            };
            let started = std::thread::Builder::new()
                .name(String::from("signals"))
                .spawn(move || {
                    for _ in signals.forever() {
                        if executor::block_on(output.send(())).is_err() {
                            break;
                        }
                    }
                });
            if started.is_ok() {
                std::future::pending::<()>().await;
            }
        })
    })
}

/// Windows and the browser have no Unix signals: Windows asks a program to
/// stop by closing its window.
#[cfg(not(unix))]
pub fn stop_requests() -> Subscription<()> {
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
