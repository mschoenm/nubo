//! Game controllers (gamepads), read with gilrs (the `gamepads` feature).
//! Each controller's state is reported in the order they connected
//! ([`Controllers`]): the game gives each one to its own player when a Super
//! Game Boy game asks for more than one, and menus and shortcuts use them
//! all combined. The app hears about them
//! whatever it shows (Nubo's app turns them into an Iced subscription), so
//! controllers work on every screen: to play, to use shortcuts such as the
//! pause menu, and to pick buttons in the settings.
//!
//! The desktop build reads them on a thread of its own, which sleeps until
//! a controller has something to report and sends it over a `futures`
//! channel (`read_on_thread`). The browser build has no threads, so there
//! the app reads them on every frame (`next_event`).
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
use nubo_core::{ControllerButton, ControllerState, Key};
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

/// A connected controller's identity, which stays the same while it is
/// connected, even when a controller before it in the list disconnects: the
/// number gilrs gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ControllerId(pub usize);

/// What every connected controller is doing, in the order gilrs lists them
/// (the order they connected in).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Controllers {
    /// Each controller's identity, in the order of `each`.
    ids: Vec<ControllerId>,
    each: Vec<ControllerState>,
}

impl Controllers {
    /// The controllers' states, the first controller's first.
    pub fn each(&self) -> &[ControllerState] {
        &self.each
    }

    /// The controllers' identities, in the order of [`Controllers::each`].
    pub fn ids(&self) -> &[ControllerId] {
        &self.ids
    }

    /// The state of the controller `id`, if it is connected.
    pub fn state(&self, id: ControllerId) -> Option<&ControllerState> {
        let index = self.ids.iter().position(|&known| known == id)?;
        self.each.get(index)
    }

    /// Every controller combined: a button counts as held if it is held on
    /// any of them, and the stick points where the first one pushed points.
    pub fn combined(&self) -> ControllerState {
        let mut combined = ControllerState::new();
        for state in &self.each {
            for button in ControllerButton::ALL {
                if state.is_pressed(button) {
                    combined.set_button(button, true);
                }
            }
        }
        let axis = |positive, negative| {
            self.each
                .iter()
                .find_map(|state| {
                    let pushed = |key| state.stick_directions().any(|held| held == key);
                    match (pushed(positive), pushed(negative)) {
                        (true, false) => Some(1.0),
                        (false, true) => Some(-1.0),
                        _ => None,
                    }
                })
                .unwrap_or(0.0)
        };
        combined.set_left_stick(axis(Key::Right, Key::Left), axis(Key::Up, Key::Down));
        combined
    }
}

impl From<Vec<(ControllerId, ControllerState)>> for Controllers {
    /// Controllers with these identities doing what their states say.
    fn from(controllers: Vec<(ControllerId, ControllerState)>) -> Self {
        let (ids, each) = controllers.into_iter().unzip();
        Controllers { ids, each }
    }
}

impl From<Vec<ControllerState>> for Controllers {
    /// Controllers doing what `each` says, numbered from 0 in its order.
    fn from(each: Vec<ControllerState>) -> Self {
        Controllers {
            ids: (0..each.len()).map(ControllerId).collect(),
            each,
        }
    }
}

/// News about the controllers.
#[derive(Debug, Clone)]
pub enum ControllerEvent {
    /// A controller with this name was connected, or was there when
    /// reading started.
    Connected(String),
    /// The controller with this name was disconnected.
    Disconnected(String),
    /// The buttons held or the stick direction changed, on any controller.
    Changed(Controllers),
    /// Controller support couldn't start, for this reason.
    Unavailable(String),
}

/// Reads all connected controllers and remembers what it last reported.
struct Reader {
    gilrs: Gilrs,
    last_state: Controllers,
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
            last_state: Controllers::default(),
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
            self.last_state = state.clone();
            events.push(ControllerEvent::Changed(state));
        }

        events
    }

    /// Every connected controller's state.
    fn state(&self) -> Controllers {
        let (ids, each) = self
            .gilrs
            .gamepads()
            .map(|(id, gamepad)| {
                let mut state = ControllerState::new();
                for (button, controller_button) in BUTTONS {
                    if gamepad.is_pressed(button) {
                        state.set_button(controller_button, true);
                    }
                }
                state.set_left_stick(
                    snap(gamepad.value(Axis::LeftStickX)),
                    snap(gamepad.value(Axis::LeftStickY)),
                );
                (ControllerId(id.into()), state)
            })
            .unzip();
        Controllers { ids, each }
    }
}

/// The name the controller gives itself, for the player to see.
fn name(gamepad: &Gamepad<'_>) -> String {
    gamepad.name().to_owned()
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
