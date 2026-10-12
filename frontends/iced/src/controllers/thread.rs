//! The desktop build: a thread waits for the controllers to report
//! something and sends what changed to the app.

use super::{ControllerEvent, Reader};
use iced::Subscription;
use iced::futures::channel::mpsc::Sender;
use iced::futures::{SinkExt, executor};
use std::time::Duration;

/// The longest the thread waits for controller events before checking
/// whether the app still listens.
const LONGEST_WAIT: Duration = Duration::from_secs(1);

pub fn subscription() -> Subscription<ControllerEvent> {
    Subscription::run(|| {
        iced::stream::channel(64, async |output| {
            let started = std::thread::Builder::new()
                .name(String::from("controllers"))
                .spawn(move || read_forever(output));

            if started.is_ok() {
                std::future::pending::<()>().await;
            }
        })
    })
}

/// Reads the controllers until the app stops listening.
fn read_forever(mut output: Sender<ControllerEvent>) {
    let mut reader = match Reader::open() {
        Ok(reader) => reader,
        Err(error) => {
            send(&mut output, ControllerEvent::Unavailable(error));
            return;
        }
    };

    for event in reader.connected() {
        if !send(&mut output, event) {
            return;
        }
    }

    while !output.is_closed() {
        for event in reader.wait(LONGEST_WAIT) {
            if !send(&mut output, event) {
                return;
            }
        }
    }
}

/// Sends `event` to the app, returning whether it still listens.
fn send(output: &mut Sender<ControllerEvent>, event: ControllerEvent) -> bool {
    executor::block_on(output.send(event)).is_ok()
}
