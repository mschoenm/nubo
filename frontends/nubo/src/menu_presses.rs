//! Controller buttons pressed in the menus (the library, the pause menu or
//! the settings) don't reach the game until they are released. Otherwise the
//! A that picks "Resume" would still be held when the game goes on, and the
//! game would see a fresh press of A. Keyboard keys already work this way,
//! since the app only passes key presses on while the game runs.
//!
//! Each controller's presses are held back on their own, since on a Super
//! Game Boy each controller can play a player of its own: a button the
//! second player still holds from the menus mustn't hide the same button
//! from the first.

use nubo_core::{ControllerButton, ControllerState, Key};

/// The controller buttons and stick directions that went down on each
/// controller while the game wasn't running and are still held.
#[derive(Debug, Default)]
pub struct MenuPresses {
    /// What each controller holds back, the first controller's first.
    each: Vec<HeldBack>,
}

impl MenuPresses {
    /// Notes how each controller went from `previous` to `states` (both the
    /// first controller's first): what goes down while `in_menus` is held
    /// back from the game, and whatever is released stops being held back.
    /// A controller that wasn't there before counts as holding nothing.
    pub fn update(
        &mut self,
        previous: &[ControllerState],
        states: &[ControllerState],
        in_menus: bool,
    ) {
        self.each.resize_with(states.len(), HeldBack::default);
        let released = ControllerState::new();
        for (index, (held_back, state)) in self.each.iter_mut().zip(states).enumerate() {
            held_back.update(previous.get(index).unwrap_or(&released), state, in_menus);
        }
    }

    /// What the game gets to see of `state`, the state of the controller at
    /// `index`: everything that controller doesn't hold back.
    pub fn hide_from(&self, index: usize, state: &ControllerState) -> ControllerState {
        match self.each.get(index) {
            Some(held_back) => held_back.hide_from(state),
            None => *state,
        }
    }
}

/// The buttons and stick directions one controller holds back.
#[derive(Debug, Default)]
struct HeldBack {
    buttons: Vec<ControllerButton>,
    directions: Vec<Key>,
}

impl HeldBack {
    /// Notes how the controller went from `previous` to `state`.
    fn update(&mut self, previous: &ControllerState, state: &ControllerState, in_menus: bool) {
        let held: Vec<Key> = state.stick_directions().collect();
        self.buttons.retain(|&button| state.is_pressed(button));
        self.directions.retain(|direction| held.contains(direction));

        if !in_menus {
            return;
        }

        for button in ControllerButton::ALL {
            if state.is_pressed(button)
                && !previous.is_pressed(button)
                && !self.buttons.contains(&button)
            {
                self.buttons.push(button);
            }
        }

        let held_before: Vec<Key> = previous.stick_directions().collect();
        for direction in held {
            if !held_before.contains(&direction) && !self.directions.contains(&direction) {
                self.directions.push(direction);
            }
        }
    }

    /// What the game gets to see of `state`: everything that isn't held back.
    fn hide_from(&self, state: &ControllerState) -> ControllerState {
        let mut visible = ControllerState::new();

        for button in ControllerButton::ALL {
            if state.is_pressed(button) && !self.buttons.contains(&button) {
                visible.set_button(button, true);
            }
        }

        // The controller thread reports the stick as fully pushed or
        // centered, so its directions say all there is to know about it.
        let pushed = |direction| {
            !self.directions.contains(&direction)
                && state.stick_directions().any(|held| held == direction)
        };
        let axis =
            |positive, negative| f32::from(i8::from(pushed(positive)) - i8::from(pushed(negative)));
        visible.set_left_stick(axis(Key::Right, Key::Left), axis(Key::Up, Key::Down));

        visible
    }
}

#[cfg(test)]
mod tests;
