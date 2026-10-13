//! Game controllers, read with gilrs and combined into one controller
//! state. The app hears about them through a subscription (see
//! `subscriptions.rs`), so controllers work on every screen: to play, to use
//! shortcuts such as the pause menu, and to pick buttons in the settings.
//!
//! The desktop build reads them on a thread of its own, which sleeps until
//! a controller has something to report and sends it over a channel
//! (`read_on_thread`). The browser build has no threads, so there the app
//! reads them on every frame (`next_event`).
//!
//! How much that thread sleeps depends on the system. On Linux (epoll) and
//! macOS, gilrs blocks until a controller sends something. On Windows its
//! default backend (Windows.Gaming.Input) has a thread of its own that polls
//! the controllers every 8 ms whatever this thread does, so a Windows
//! computer wakes up 125 times a second for controllers even when none is
//! connected.

use gilrs::{Axis, Button, Event, EventType, Gamepad, Gilrs, GilrsBuilder};

#[cfg(target_arch = "wasm32")]
pub use frame::next_event;
use nubo_core::{ControllerButton, ControllerState};
#[cfg(not(target_arch = "wasm32"))]
pub use thread::read_on_thread;

/// How far the stick has to be pushed (from 0 to 1) to count as a D-pad
/// press. The stick is reported as fully pushed or centered, so small
/// movements don't flood the app with updates.
const STICK_THRESHOLD: f32 = 0.5;

/// The gilrs buttons and the controller buttons they are.
const BUTTONS: [(Button, ControllerButton); 16] = [
    (Button::South, ControllerButton::South),
    (Button::East, ControllerButton::East),
    (Button::West, ControllerButton::West),
    (Button::North, ControllerButton::North),
    (Button::DPadUp, ControllerButton::DPadUp),
    (Button::DPadDown, ControllerButton::DPadDown),
    (Button::DPadLeft, ControllerButton::DPadLeft),
    (Button::DPadRight, ControllerButton::DPadRight),
    (Button::Start, ControllerButton::Start),
    (Button::Select, ControllerButton::Select),
    (Button::LeftTrigger, ControllerButton::LeftShoulder),
    (Button::RightTrigger, ControllerButton::RightShoulder),
    (Button::LeftTrigger2, ControllerButton::LeftTrigger),
    (Button::RightTrigger2, ControllerButton::RightTrigger),
    (Button::LeftThumb, ControllerButton::LeftStick),
    (Button::RightThumb, ControllerButton::RightStick),
];

/// News about the controllers.
#[derive(Debug, Clone)]
pub enum ControllerEvent {
    Connected(String),
    Disconnected(String),
    /// The buttons held or the stick direction changed, on any controller.
    Changed(ControllerState),
    /// Controller support couldn't start, for this reason.
    Unavailable(String),
}

/// Reads all connected controllers and remembers what it last reported.
struct Reader {
    gilrs: Gilrs,
    last_state: ControllerState,
}

impl Reader {
    /// Starts reading controllers. Force feedback stays off: Nubo doesn't
    /// use it, and gilrs would otherwise run a thread that wakes 20 times a
    /// second for it.
    fn open() -> Result<Reader, String> {
        let gilrs = GilrsBuilder::new()
            .with_force_feedback(false)
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Reader {
            gilrs,
            last_state: ControllerState::new(),
        })
    }

    /// The controllers connected before the app started.
    fn connected(&self) -> Vec<ControllerEvent> {
        self.gilrs
            .gamepads()
            .map(|(_, gamepad)| ControllerEvent::Connected(name(&gamepad)))
            .collect()
    }

    /// Handles what happened since the last call: controllers coming and
    /// going, and a change in the combined state.
    #[cfg(target_arch = "wasm32")]
    fn poll(&mut self) -> Vec<ControllerEvent> {
        self.handle(None)
    }

    /// Waits up to `timeout` for a controller to report something, then
    /// handles it and whatever else happened, like `poll`.
    #[cfg(not(target_arch = "wasm32"))]
    fn wait(&mut self, timeout: std::time::Duration) -> Vec<ControllerEvent> {
        let first = self.gilrs.next_event_blocking(Some(timeout));
        self.handle(first)
    }

    /// Handles `first` (an event already taken from gilrs) and the events
    /// still waiting, then reports a change in the combined state.
    fn handle(&mut self, first: Option<Event>) -> Vec<ControllerEvent> {
        let mut events = Vec::new();
        let mut next = first.or_else(|| self.gilrs.next_event());

        while let Some(event) = next {
            let gamepad = self.gilrs.gamepad(event.id);
            match event.event {
                EventType::Connected => events.push(ControllerEvent::Connected(name(&gamepad))),
                EventType::Disconnected => {
                    events.push(ControllerEvent::Disconnected(name(&gamepad)));
                }
                _ => {}
            }
            next = self.gilrs.next_event();
        }

        let state = self.state();
        if state != self.last_state {
            self.last_state = state;
            events.push(ControllerEvent::Changed(state));
        }

        events
    }

    /// Every connected controller combined: a button counts as held if it
    /// is held on any of them.
    fn state(&self) -> ControllerState {
        let mut state = ControllerState::new();
        let mut stick_right: f32 = 0.0;
        let mut stick_up: f32 = 0.0;

        for (_, gamepad) in self.gilrs.gamepads() {
            for (button, controller_button) in BUTTONS {
                if gamepad.is_pressed(button) {
                    state.set_button(controller_button, true);
                }
            }

            stick_right = strongest(stick_right, gamepad.value(Axis::LeftStickX));
            stick_up = strongest(stick_up, gamepad.value(Axis::LeftStickY));
        }

        state.set_left_stick(snap(stick_right), snap(stick_up));
        state
    }
}

fn name(gamepad: &Gamepad<'_>) -> String {
    gamepad.name().to_owned()
}

/// Returns whichever stick value is further from the center.
fn strongest(current: f32, candidate: f32) -> f32 {
    if candidate.abs() > current.abs() {
        candidate
    } else {
        current
    }
}

/// Reports a stick axis as fully pushed one way, or centered.
fn snap(value: f32) -> f32 {
    if value >= STICK_THRESHOLD {
        1.0
    } else if value <= -STICK_THRESHOLD {
        -1.0
    } else {
        0.0
    }
}

#[cfg(target_arch = "wasm32")]
mod frame;
#[cfg(test)]
mod tests;
#[cfg(not(target_arch = "wasm32"))]
mod thread;
