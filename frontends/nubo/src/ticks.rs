//! Wake-ups at a fixed interval for the desktop build, which has no async
//! timer, so a thread of its own sleeps between them. The window only
//! redraws for new frames and input, so whatever changes with time alone
//! (toasts expiring, the fullscreen bar hiding, problems reported from
//! other threads) needs these to be noticed.

use iced::Subscription;
use iced::futures::{SinkExt, executor};
use std::time::Duration;

/// Produces an item every `interval` until the subscription is dropped.
pub fn every(interval: Duration) -> Subscription<()> {
    Subscription::run_with(interval, |interval| {
        let interval = *interval;
        iced::stream::channel(1, async move |mut output| {
            let started = std::thread::Builder::new()
                .name(String::from("ticks"))
                .spawn(move || {
                    loop {
                        std::thread::sleep(interval);
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
