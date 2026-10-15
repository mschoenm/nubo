//! Keys and controller buttons: picking one for an action in the settings,
//! the app's shortcuts, moving through the menus, and playing.

use super::window_events::toggle_fullscreen;
use super::{App, GameMessage, Message};
use crate::input::{self, KeyInput};
use iced::Task;
use iced::keyboard::{self, key::Named};
use nubo_core::{ControllerButton, ControllerState, Key};
use nubo_frontend::controls::{Action, Device, PadButton};
use nubo_frontend::gamepads::ControllerEvent;
use nubo_frontend::save_states::Slot;

impl App {
    /// What the game sees of each controller: everything but the buttons
    /// held back because they were pressed in the menus.
    pub(super) fn controller_states_for_game(&self) -> Vec<ControllerState> {
        let controllers = &self.controller_states;
        controllers
            .ids()
            .iter()
            .zip(controllers.each())
            .map(|(&id, state)| self.menu_presses.hide_from(id, state))
            .collect()
    }

    /// Handles a key going down or up: picking a key in the settings, the
    /// fixed shortcuts, and then the player's controls. A window that shows
    /// only the game (`--scale`) has no shortcuts.
    pub(super) fn handle_key(&mut self, input: &KeyInput) -> Task<Message> {
        if input.pressed && !input.repeat && self.screen_scale.is_none() {
            if let Some((device, action)) = self.capture {
                match input.key.as_ref() {
                    keyboard::Key::Named(Named::Escape) => self.capture = None,
                    keyboard::Key::Named(Named::Backspace) => {
                        self.capture = None;
                        self.change_controls(|controls| controls.clear(device, action));
                    }
                    _ => {
                        if device == Device::Keyboard
                            && let Some(key) = input.code
                        {
                            self.capture = None;
                            self.change_controls(|controls| {
                                controls.keyboard.bind(action, Some(key));
                            });
                        }
                    }
                }
                return Task::none();
            }

            if let Some(message) = input::fixed_shortcut(&input.key, input.modifiers) {
                return self.update(message);
            }
        }

        if input.pressed && input.modifiers.command() {
            return Task::none();
        }

        let Some(action) = input
            .code
            .and_then(|key| self.settings.shared.controls.keyboard.action(key))
        else {
            return Task::none();
        };

        if input.pressed && action.navigates() && self.in_menus() {
            // Holding a direction keeps moving, like in other apps.
            if input.repeat && !action.is_direction() {
                return Task::none();
            }
            return self.navigate(action);
        }

        if input.repeat {
            return Task::none();
        }
        self.perform(action, input.pressed)
    }

    /// Handles news from the controllers: picking a button in the settings,
    /// the shortcuts on controller buttons, and passing the rest to the game.
    pub(super) fn handle_controller(&mut self, event: ControllerEvent) -> Task<Message> {
        match event {
            ControllerEvent::Connected(name) => {
                self.toasts.info(format!("{name} connected"));
                self.controllers.push(name);
                Task::none()
            }
            ControllerEvent::Disconnected(name) => {
                self.toasts.info(format!("{name} disconnected"));
                if let Some(index) = self.controllers.iter().position(|known| *known == name) {
                    self.controllers.remove(index);
                }
                Task::none()
            }
            ControllerEvent::Unavailable(error) => {
                self.toasts
                    .error(format!("Controllers can't be used: {error}"));
                Task::none()
            }
            ControllerEvent::Changed(controllers) => {
                let state = controllers.combined();
                let previous_states = std::mem::replace(&mut self.controller_states, controllers);
                let previous = std::mem::replace(&mut self.controller_state, state);
                let changed: Vec<(ControllerButton, bool)> = ControllerButton::ALL
                    .into_iter()
                    .filter(|&button| state.is_pressed(button) != previous.is_pressed(button))
                    .map(|button| (button, state.is_pressed(button)))
                    .collect();

                self.menu_presses.update(
                    &previous_states,
                    &self.controller_states,
                    self.in_menus(),
                );
                let states = self.controller_states_for_game();
                if let Some(game) = &mut self.game {
                    game.session.set_controller_states(states);
                }

                if let Some((device, action)) = self.capture {
                    let pressed = changed
                        .iter()
                        .find(|&&(_, pressed)| pressed)
                        .map(|&(button, _)| PadButton(button));

                    match (device, pressed) {
                        (Device::Controller, Some(button)) => {
                            self.capture = None;
                            self.change_controls(|controls| {
                                controls.controller.bind(action, Some(button));
                            });
                        }
                        // Waiting for a keyboard key: the controller's B
                        // cancels, and its other buttons are ignored.
                        (Device::Keyboard, Some(button))
                            if self.settings.shared.controls.controller.action(button)
                                == Some(Action::B) =>
                        {
                            self.capture = None;
                        }
                        _ => {}
                    }
                    return Task::none();
                }

                let mut tasks = Vec::new();

                if self.in_menus() {
                    let held_before: Vec<Key> = previous.stick_directions().collect();
                    let stick = state
                        .stick_directions()
                        .filter(|key| !held_before.contains(key))
                        .filter_map(Action::for_direction);
                    let buttons = changed.iter().filter(|&&(_, pressed)| pressed).filter_map(
                        |&(button, _)| {
                            let action = self
                                .settings
                                .shared
                                .controls
                                .controller
                                .action(PadButton(button))?;
                            action.navigates().then_some(action)
                        },
                    );
                    let steps: Vec<Action> = stick.chain(buttons).collect();

                    for action in steps {
                        tasks.push(self.navigate(action));
                    }
                }

                let shortcuts: Vec<(Action, bool)> = changed
                    .into_iter()
                    .filter_map(|(button, pressed)| {
                        let action = self
                            .settings
                            .shared
                            .controls
                            .controller
                            .action(PadButton(button))?;
                        action.game_boy_key().is_none().then_some((action, pressed))
                    })
                    .collect();

                for (action, pressed) in shortcuts {
                    tasks.push(self.perform(action, pressed));
                }
                Task::batch(tasks)
            }
        }
    }

    /// Moves through the menu on screen (a dialog, the settings window, the
    /// pause menu or the library) with a direction, picks with A or Start, and steps
    /// back with B. These are the keys and controller buttons the player
    /// bound to the Game Boy's D-pad, A, B, Start and Select.
    fn navigate(&mut self, action: Action) -> Task<Message> {
        if self.dialog.is_some() {
            return self.navigate_dialog(action);
        }
        if self.settings_tab.is_some() {
            return self.navigate_settings(action);
        }

        if self.game.is_some() {
            self.navigate_pause_menu(action)
        } else {
            self.navigate_library(action)
        }
    }

    /// Does what a key or controller button is bound to, as it goes down
    /// (`pressed`) or up. Game Boy buttons only go down while the game runs.
    /// A window that shows only the game (`--scale`) only plays it and fast
    /// forwards, so nothing can come up over the game.
    fn perform(&mut self, action: Action, pressed: bool) -> Task<Message> {
        if let Some(key) = action.game_boy_key() {
            let running = self.is_running();
            if let Some(game) = &mut self.game {
                if !pressed {
                    game.session.release(key);
                } else if running {
                    game.session.press(key);
                }
            }
            return Task::none();
        }

        if action == Action::FastForward {
            if let Some(game) = &mut self.game {
                game.session.hold_fast_forward(pressed);
            }
            return Task::none();
        }

        if action == Action::Rewind {
            if let Some(game) = &mut self.game {
                game.session.hold_rewind(pressed);
            }
            return Task::none();
        }

        if !pressed || self.screen_scale.is_some() {
            return Task::none();
        }

        match action {
            Action::PauseMenu => return self.update(GameMessage::TogglePause.into()),
            Action::QuickSave => self.save_state(Slot::Quick),
            Action::QuickLoad => self.load_state(Slot::Quick),
            Action::Fullscreen => return toggle_fullscreen(),
            Action::Screenshot => self.take_screenshot(),
            _ => {}
        }

        Task::none()
    }
}
