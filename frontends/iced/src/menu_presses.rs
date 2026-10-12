//! Controller buttons pressed in the menus (the library, the pause menu or
//! the settings) don't reach the game until they are released. Otherwise the
//! A that picks "Resume" would still be held when the game goes on, and the
//! game would see a fresh press of A. Keyboard keys already work this way,
//! since the app only passes key presses on while the game runs.

use nubo_core::{ControllerButton, ControllerState, Key};

/// The controller buttons and stick directions that went down while the game
/// wasn't running and are still held.
#[derive(Debug, Default)]
pub struct MenuPresses {
    buttons: Vec<ControllerButton>,
    directions: Vec<Key>,
}

impl MenuPresses {
    /// Notes how the controllers went from `previous` to `state`: what goes
    /// down while `in_menus` is held back from the game, and whatever is
    /// released stops being held back.
    pub fn update(&mut self, previous: &ControllerState, state: &ControllerState, in_menus: bool) {
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
    pub fn hide_from(&self, state: &ControllerState) -> ControllerState {
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
