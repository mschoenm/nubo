//! Game controller (gamepad) input, independent of any platform.
//!
//! Frontends read the real controller, fill in a [`ControllerState`] and pass
//! it to the emulator, which uses a [`ControllerMapping`] to turn it into Game
//! Boy button presses.

use crate::joypad::Key;

/// How far the stick has to be pushed, from 0 (center) to 1 (edge), before it
/// counts as pressing a direction.
const STICK_DEAD_ZONE: f32 = 0.5;

/// A button on a modern controller.
///
/// The four face buttons are named after their position, so the names fit
/// any brand: on an Xbox controller South is A and East is B, on a Nintendo
/// controller South is B and East is A.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerButton {
    /// The bottom face button.
    South,
    /// The right face button.
    East,
    /// The left face button.
    West,
    /// The top face button.
    North,
    /// Up on the directional pad.
    DPadUp,
    /// Down on the directional pad.
    DPadDown,
    /// Left on the directional pad.
    DPadLeft,
    /// Right on the directional pad.
    DPadRight,
    /// The Start (or Menu, or +) button.
    Start,
    /// The Select (or View, or -) button.
    Select,
    /// The upper button on the left shoulder (L, L1 or LB).
    LeftShoulder,
    /// The upper button on the right shoulder (R, R1 or RB).
    RightShoulder,
    /// The trigger under the left shoulder (ZL, L2 or LT).
    LeftTrigger,
    /// The trigger under the right shoulder (ZR, R2 or RT).
    RightTrigger,
    /// Pressing the left stick in (L3).
    LeftStick,
    /// Pressing the right stick in (R3).
    RightStick,
}

impl ControllerButton {
    /// Every button, in the order a settings screen would list them.
    pub const ALL: [ControllerButton; 16] = [
        ControllerButton::South,
        ControllerButton::East,
        ControllerButton::West,
        ControllerButton::North,
        ControllerButton::DPadUp,
        ControllerButton::DPadDown,
        ControllerButton::DPadLeft,
        ControllerButton::DPadRight,
        ControllerButton::Start,
        ControllerButton::Select,
        ControllerButton::LeftShoulder,
        ControllerButton::RightShoulder,
        ControllerButton::LeftTrigger,
        ControllerButton::RightTrigger,
        ControllerButton::LeftStick,
        ControllerButton::RightStick,
    ];

    fn mask(self) -> u16 {
        1 << self as u16
    }
}

/// A snapshot of a controller: which buttons are held and where the left
/// stick points.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ControllerState {
    /// One bit per [`ControllerButton`], set while the button is held.
    buttons: u16,
    left_stick_right: f32,
    left_stick_up: f32,
}

impl ControllerState {
    /// A controller with no buttons held and the stick centered.
    pub fn new() -> Self {
        Self::default()
    }

    /// Marks `button` as held or released.
    pub fn set_button(&mut self, button: ControllerButton, pressed: bool) -> &mut Self {
        if pressed {
            self.buttons |= button.mask();
        } else {
            self.buttons &= !button.mask();
        }
        self
    }

    /// Whether `button` is held.
    pub fn is_pressed(&self, button: ControllerButton) -> bool {
        self.buttons & button.mask() != 0
    }

    /// Sets the left stick position. Each axis goes from -1 to 1, where
    /// positive `right` points right and positive `up` points up.
    pub fn set_left_stick(&mut self, right: f32, up: f32) -> &mut Self {
        self.left_stick_right = right;
        self.left_stick_up = up;
        self
    }

    /// The D-pad directions the left stick is pushed toward, beyond the dead
    /// zone. Frontends can use them to move through menus with the stick.
    pub fn stick_directions(&self) -> impl Iterator<Item = Key> + use<> {
        [
            (self.left_stick_up > STICK_DEAD_ZONE, Key::Up),
            (self.left_stick_up < -STICK_DEAD_ZONE, Key::Down),
            (self.left_stick_right < -STICK_DEAD_ZONE, Key::Left),
            (self.left_stick_right > STICK_DEAD_ZONE, Key::Right),
        ]
        .into_iter()
        .filter_map(|(active, key)| active.then_some(key))
    }
}

/// Which controller button presses which Game Boy button.
///
/// The default mapping uses the D-pad, Start and Select as you'd expect, and
/// puts A and B where they sit on a Game Boy: East is A, South is B. The left
/// stick always acts as the D-pad and can't be remapped.
#[derive(Debug, Clone, PartialEq)]
pub struct ControllerMapping {
    bindings: Vec<(ControllerButton, Key)>,
}

impl Default for ControllerMapping {
    fn default() -> Self {
        ControllerMapping {
            bindings: vec![
                (ControllerButton::DPadUp, Key::Up),
                (ControllerButton::DPadDown, Key::Down),
                (ControllerButton::DPadLeft, Key::Left),
                (ControllerButton::DPadRight, Key::Right),
                (ControllerButton::East, Key::A),
                (ControllerButton::South, Key::B),
                (ControllerButton::Start, Key::Start),
                (ControllerButton::Select, Key::Select),
            ],
        }
    }
}

impl ControllerMapping {
    /// A mapping where no button presses anything; only the left stick still
    /// acts as the D-pad. Frontends that let users pick every button start
    /// from this.
    pub fn empty() -> Self {
        ControllerMapping {
            bindings: Vec::new(),
        }
    }

    /// Makes `button` press `key`, replacing what `button` did before.
    /// Several buttons may press the same key.
    pub fn bind(&mut self, button: ControllerButton, key: Key) -> &mut Self {
        self.bindings
            .retain(|(bound_button, _)| *bound_button != button);
        self.bindings.push((button, key));
        self
    }

    /// The Game Boy keys held by the buttons and stick in `state`. A key may
    /// appear more than once.
    pub(crate) fn pressed_keys<'a>(
        &'a self,
        state: &'a ControllerState,
    ) -> impl Iterator<Item = Key> + 'a {
        self.bindings
            .iter()
            .filter(|(button, _)| state.is_pressed(*button))
            .map(|(_, key)| *key)
            .chain(state.stick_directions())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pressed_keys(mapping: &ControllerMapping, state: &ControllerState) -> Vec<Key> {
        mapping.pressed_keys(state).collect()
    }

    #[test]
    fn default_mapping_uses_nintendo_button_positions() {
        let mut state = ControllerState::new();
        state.set_button(ControllerButton::East, true);
        state.set_button(ControllerButton::South, true);

        let keys = pressed_keys(&ControllerMapping::default(), &state);

        assert_eq!(keys, vec![Key::A, Key::B]);
    }

    #[test]
    fn released_buttons_are_not_reported() {
        let mut state = ControllerState::new();
        state.set_button(ControllerButton::Start, true);
        state.set_button(ControllerButton::Start, false);

        assert!(pressed_keys(&ControllerMapping::default(), &state).is_empty());
    }

    #[test]
    fn stick_inside_dead_zone_presses_nothing() {
        let mut state = ControllerState::new();
        state.set_left_stick(0.4, -0.4);

        assert!(pressed_keys(&ControllerMapping::default(), &state).is_empty());
    }

    #[test]
    fn stick_outside_dead_zone_presses_directions() {
        let mut state = ControllerState::new();
        state.set_left_stick(0.9, -0.8);

        let keys = pressed_keys(&ControllerMapping::default(), &state);

        assert_eq!(keys, vec![Key::Down, Key::Right]);
    }

    #[test]
    fn an_empty_mapping_only_uses_the_stick() {
        let mut state = ControllerState::new();
        state.set_button(ControllerButton::South, true);
        state.set_left_stick(-1.0, 0.0);

        assert_eq!(
            pressed_keys(&ControllerMapping::empty(), &state),
            vec![Key::Left]
        );
    }

    #[test]
    fn every_button_has_its_own_bit() {
        let mut state = ControllerState::new();
        for button in ControllerButton::ALL {
            assert!(!state.is_pressed(button));
            state.set_button(button, true);
        }
        assert!(
            ControllerButton::ALL
                .iter()
                .all(|&button| state.is_pressed(button))
        );
    }

    #[test]
    fn bind_replaces_existing_binding_for_button() {
        let mut mapping = ControllerMapping::default();
        mapping.bind(ControllerButton::South, Key::A);
        let mut state = ControllerState::new();
        state.set_button(ControllerButton::South, true);

        assert_eq!(pressed_keys(&mapping, &state), vec![Key::A]);
    }
}
