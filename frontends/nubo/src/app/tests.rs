//! Tests for the app's small helpers, for resetting the settings in a
//! `--fresh` app, which keeps nothing, for shortcuts while the settings
//! are open, for dialogs whose content is gone, and for the fixed keys and
//! buttons of the menus.

use super::playing::{PendingStart, ResumeAnswer};
use super::settings_window::reset_summary;
use super::{App, CAPTURE_TIMEOUT, GameMessage, LibraryMessage, Message, SettingsMessage, cycle};
use crate::command_line::Options;
use crate::dialog::{Dialog, DialogKind, ResetChoices};
use crate::input::KeyInput;
use crate::settings::Appearance;
use crate::views::settings::SettingsTab;
use iced::keyboard::{self, Modifiers};
use nubo_core::{ControllerButton, ControllerState, Emulator, Model};
use nubo_frontend::controls::{Action, Device, KeyCode, MenuButtons, PadButton};
use nubo_frontend::gamepads::{ControllerEvent, Controllers};
use nubo_frontend::model::ModelPreference;
use nubo_frontend::rom::Rom;
use std::path::PathBuf;
use std::sync::Arc;

const OPTIONS: [u8; 3] = [1, 2, 3];

#[test]
fn cycling_stops_at_the_ends_without_wrapping() {
    assert_eq!(cycle(&OPTIONS, 1, -1, false), 1);
    assert_eq!(cycle(&OPTIONS, 2, 1, false), 3);
    assert_eq!(cycle(&OPTIONS, 3, 1, false), 3);
}

#[test]
fn cycling_starts_over_with_wrapping() {
    assert_eq!(cycle(&OPTIONS, 3, 1, true), 1);
    assert_eq!(cycle(&OPTIONS, 1, -1, true), 3);
}

#[test]
fn cycling_through_no_options_stays_put() {
    assert_eq!(cycle(&[], 2, 1, false), 2);
    assert_eq!(cycle(&[], 2, -1, true), 2);
}

#[test]
fn a_reset_asks_first_puts_the_settings_back_and_can_be_undone() {
    let (mut app, _) = App::new(
        None,
        &Options {
            fresh: true,
            ..Options::default()
        },
    );
    app.settings.appearance = Appearance::Dark;
    app.settings.shared.volume = 0.3;
    let changed = app.settings.clone();

    let _ = app.update(SettingsMessage::AskReset.into());
    let dialog = app.dialog.clone().unwrap();
    assert_eq!(
        dialog.kind,
        DialogKind::ResetSettings(ResetChoices::default())
    );
    let content = app.dialog_content().unwrap();
    assert_eq!(content.choices[dialog.selection].label, "Cancel");

    let _ = app.update(SettingsMessage::Reset(ResetChoices::default()).into());
    assert_eq!(app.settings, changed.reset(false));
    assert_eq!(app.settings.appearance, Appearance::System);
    assert!((app.settings.shared.volume - 1.0).abs() < f32::EPSILON);

    let _ = app.update(GameMessage::Undo.into());
    assert_eq!(app.settings, changed);
}

#[test]
fn a_game_menu_closes_when_its_game_is_gone() {
    let (mut app, _) = App::new(
        None,
        &Options {
            fresh: true,
            ..Options::default()
        },
    );

    let _ = app.update(LibraryMessage::OpenGameMenu(PathBuf::from("gone.gb")).into());

    assert!(app.dialog.is_none());
}

#[test]
fn a_reset_whose_deletions_are_not_offered_can_still_be_undone() {
    let (mut app, _) = App::new(
        None,
        &Options {
            fresh: true,
            ..Options::default()
        },
    );
    app.settings.appearance = Appearance::Dark;

    let _ = app.update(
        SettingsMessage::Reset(ResetChoices {
            delete_battery_saves: true,
            ..ResetChoices::default()
        })
        .into(),
    );
    let _ = app.update(GameMessage::Undo.into());

    assert_eq!(app.settings.appearance, Appearance::Dark);
}

#[test]
fn a_game_waits_for_every_reset_deleting_saves() {
    let (mut app, _) = App::new(
        None,
        &Options {
            fresh: true,
            ..Options::default()
        },
    );
    app.deletions_running = 2;
    let _ = app.update(
        GameMessage::RomPicked(Some(Rom {
            path: PathBuf::from("game.gb"),
            bytes: Arc::from(vec![0; 0x8000]),
        }))
        .into(),
    );
    let _ = app.update(
        SettingsMessage::SavesDeleted {
            done: Vec::new(),
            deleted: vec!["save states deleted"],
            errors: Vec::new(),
        }
        .into(),
    );

    assert!(app.game.is_none());
    assert!(app.start_after_deleting.is_some());
    assert_eq!(app.deletions_running, 1);
}

#[test]
fn starting_a_game_takes_away_a_resets_undo() {
    let (mut app, _) = App::new(
        None,
        &Options {
            fresh: true,
            ..Options::default()
        },
    );
    app.settings.appearance = Appearance::Dark;
    let _ = app.update(SettingsMessage::Reset(ResetChoices::default()).into());
    let _ = app.update(
        GameMessage::RomPicked(Some(Rom {
            path: PathBuf::from("game.gb"),
            bytes: Arc::from(vec![0; 3]),
        }))
        .into(),
    );
    let _ = app.update(GameMessage::Undo.into());

    assert_eq!(app.settings.appearance, Appearance::System);
}

#[test]
fn starting_over_without_the_settings_box_keeps_the_settings() {
    let (mut app, _) = App::new(
        None,
        &Options {
            fresh: true,
            ..Options::default()
        },
    );
    app.settings.appearance = Appearance::Dark;

    let _ = app.update(
        SettingsMessage::Reset(ResetChoices {
            reset_settings: false,
            ..ResetChoices::default()
        })
        .into(),
    );

    assert_eq!(app.settings.appearance, Appearance::Dark);
    assert!(app.undo.is_none());
}

#[test]
fn the_reset_toast_names_what_was_done() {
    assert_eq!(reset_summary(&["settings reset"]), "Settings reset");
    assert_eq!(
        reset_summary(&["Continue playing cleared", "save states deleted"]),
        "Continue playing cleared and save states deleted"
    );
    assert_eq!(
        reset_summary(&[
            "settings reset",
            "game folders removed",
            "in-game saves deleted"
        ]),
        "Settings reset, game folders removed and in-game saves deleted"
    );
}

#[test]
fn picking_another_settings_tab_scrolls_its_page_to_the_top() {
    let (mut app, _) = App::new(
        None,
        &Options {
            fresh: true,
            ..Options::default()
        },
    );
    let _ = app.update(SettingsMessage::Open.into());

    let task = app.update(SettingsMessage::TabSelected(SettingsTab::Controls).into());
    assert_eq!(app.settings_tab, Some(SettingsTab::Controls));
    assert_eq!(task.units(), 1);

    let task = app.update(SettingsMessage::TabSelected(SettingsTab::Controls).into());
    assert_eq!(task.units(), 0);
}

#[test]
fn controller_shortcuts_wait_while_the_settings_are_open() {
    let (mut app, _) = App::new(
        None,
        &Options {
            fresh: true,
            ..Options::default()
        },
    );
    let _ = app.update(
        GameMessage::RomPicked(Some(Rom {
            path: PathBuf::from("game.gb"),
            bytes: Arc::from(vec![0; 0x8000]),
        }))
        .into(),
    );
    let rewinding = |app: &App| app.game.as_ref().unwrap().session.is_rewinding();

    let _ = app.update(SettingsMessage::Open.into());
    let _ = app.update(pad(&[ControllerButton::RightShoulder]));
    assert!(!rewinding(&app));
    let _ = app.update(pad(&[ControllerButton::LeftShoulder]));
    assert!(!app.game.as_ref().unwrap().is_paused());
    let _ = app.update(pad(&[]));

    let _ = app.update(SettingsMessage::Close.into());
    let _ = app.update(pad(&[ControllerButton::RightShoulder]));
    assert!(rewinding(&app));
    let _ = app.update(SettingsMessage::Open.into());
    let _ = app.update(pad(&[]));
    assert!(!rewinding(&app));
}

#[test]
fn pausing_with_the_command_key_waits_while_the_settings_are_open() {
    let (mut app, _) = App::new(
        None,
        &Options {
            fresh: true,
            ..Options::default()
        },
    );
    let _ = app.update(
        GameMessage::RomPicked(Some(Rom {
            path: PathBuf::from("game.gb"),
            bytes: Arc::from(vec![0; 0x8000]),
        }))
        .into(),
    );
    let command_p = Message::Key(KeyInput {
        code: None,
        key: keyboard::Key::Character("p".into()),
        modifiers: Modifiers::COMMAND,
        pressed: true,
        repeat: false,
    });

    let _ = app.update(SettingsMessage::Open.into());
    let _ = app.update(command_p.clone());
    assert!(!app.game.as_ref().unwrap().is_paused());

    let _ = app.update(SettingsMessage::Close.into());
    let _ = app.update(command_p);
    assert!(app.game.as_ref().unwrap().is_paused());
}

#[test]
fn a_game_saved_on_another_model_starts_as_the_player_answers() {
    for (answer, model) in [
        (ResumeAnswer::Continue, Some(Model::Sgb)),
        (ResumeAnswer::StartOver, Some(Model::Dmg)),
        (ResumeAnswer::Cancel, None),
    ] {
        let mut app = waiting_app();
        let _ = app.update(GameMessage::ResumeAnswered(answer).into());

        assert_eq!(app.game.as_ref().map(|game| game.session.model()), model);
        assert!(app.pending_start.is_none());
    }
}

#[test]
fn another_game_starting_replaces_the_one_waiting() {
    let mut app = waiting_app();
    let _ = app.update(
        GameMessage::RomPicked(Some(Rom {
            path: PathBuf::from("other.gb"),
            bytes: Arc::from(vec![0; 0x8000]),
        }))
        .into(),
    );

    assert!(app.dialog.is_none());
    assert!(app.pending_start.is_none());
    let _ = app.update(GameMessage::ResumeAnswered(ResumeAnswer::Continue).into());
    assert_eq!(
        app.game.as_ref().unwrap().session.rom().path,
        PathBuf::from("other.gb")
    );
}

#[test]
fn continuing_keeps_a_game_on_automatic_when_automatic_picks_its_model() {
    let (mut app, _) = App::new(
        None,
        &Options {
            fresh: true,
            ..Options::default()
        },
    );
    // An empty cartridge is for the original Game Boy only.
    let header = Emulator::read_rom_header(&[0; 0x8000]).unwrap();
    let preference = |app: &App, model| app.preference_starting_on(&header, model);

    assert_eq!(preference(&app, Model::Dmg), None);
    assert_eq!(
        preference(&app, Model::Sgb),
        Some(ModelPreference::Fixed(Model::Sgb))
    );

    app.settings.shared.model = ModelPreference::Fixed(Model::Sgb);
    assert_eq!(
        preference(&app, Model::Dmg),
        Some(ModelPreference::Automatic)
    );
    assert_eq!(
        preference(&app, Model::CgbE),
        Some(ModelPreference::Fixed(Model::CgbE))
    );

    app.settings.shared.model = ModelPreference::Fixed(Model::Mgb);
    assert_eq!(preference(&app, Model::Mgb), None);
}

/// A `--fresh` app set to start games on the Game Boy, asking whether to
/// continue an empty game from where it was left off on the Super Game Boy.
fn waiting_app() -> App {
    let mut app = playing_app();
    app.model_override = Some(Model::Sgb);
    let _ = app.update(GameMessage::Reset.into());
    let game = app.game.as_mut().unwrap();
    let state = game.session.snapshot().unwrap();
    let rom = game.session.rom().clone();
    app.game = None;
    app.model_override = Some(Model::Dmg);

    app.dialog = Some(Dialog::new(DialogKind::OtherModel {
        saved: Model::Sgb,
        configured: Model::Dmg,
        remembers: false,
    }));
    app.pending_start = Some(PendingStart {
        header: Emulator::read_rom_header(&rom.bytes).unwrap(),
        rom,
        state,
    });
    app
}

/// A `--fresh` app playing an empty game.
fn playing_app() -> App {
    let (mut app, _) = App::new(
        None,
        &Options {
            fresh: true,
            ..Options::default()
        },
    );
    let _ = app.update(
        GameMessage::RomPicked(Some(Rom {
            path: PathBuf::from("game.gb"),
            bytes: Arc::from(vec![0; 0x8000]),
        }))
        .into(),
    );
    app
}

/// The first controller holding `buttons`.
fn pad(buttons: &[ControllerButton]) -> Message {
    let mut state = ControllerState::new();
    for &button in buttons {
        state.set_button(button, true);
    }
    Message::Controller(ControllerEvent::Changed(Controllers::from(vec![state])))
}

/// `key` going down, with no modifiers.
fn key_press(key: keyboard::Key, code: Option<KeyCode>) -> Message {
    Message::Key(KeyInput {
        code,
        key,
        modifiers: Modifiers::empty(),
        pressed: true,
        repeat: false,
    })
}

#[test]
fn the_menus_take_the_right_and_bottom_buttons_whatever_a_and_b_are_bound_to() {
    let mut app = playing_app();
    app.settings
        .shared
        .controls
        .bind_button(Action::A, PadButton(ControllerButton::North));
    app.settings
        .shared
        .controls
        .bind_button(Action::B, PadButton(ControllerButton::West));
    let paused = |app: &App| app.game.as_ref().unwrap().is_paused();

    let _ = app.update(pad(&[ControllerButton::LeftShoulder]));
    let _ = app.update(pad(&[]));
    assert!(paused(&app));
    // The buttons bound to A and B do nothing in the menu.
    let _ = app.update(pad(&[ControllerButton::North]));
    let _ = app.update(pad(&[ControllerButton::West]));
    let _ = app.update(pad(&[]));
    assert!(paused(&app));
    // The right button picks "Resume", as on a Nintendo console.
    let _ = app.update(pad(&[ControllerButton::East]));
    let _ = app.update(pad(&[]));
    assert!(!paused(&app));

    let _ = app.update(pad(&[ControllerButton::LeftShoulder]));
    let _ = app.update(pad(&[]));
    // The bottom button steps back out of the menu.
    let _ = app.update(pad(&[ControllerButton::South]));
    let _ = app.update(pad(&[]));
    assert!(!paused(&app));
}

#[test]
fn the_menus_take_the_bottom_button_to_pick_when_the_player_picks_xbox() {
    let mut app = playing_app();
    let _ = app.update(SettingsMessage::MenuButtonsSelected(MenuButtons::Xbox).into());
    let paused = |app: &App| app.game.as_ref().unwrap().is_paused();

    let _ = app.update(pad(&[ControllerButton::LeftShoulder]));
    let _ = app.update(pad(&[]));
    assert!(paused(&app));
    // The bottom button picks "Resume".
    let _ = app.update(pad(&[ControllerButton::South]));
    let _ = app.update(pad(&[]));
    assert!(!paused(&app));

    let _ = app.update(pad(&[ControllerButton::LeftShoulder]));
    let _ = app.update(pad(&[]));
    // The right button steps back out of the menu.
    let _ = app.update(pad(&[ControllerButton::East]));
    let _ = app.update(pad(&[]));
    assert!(!paused(&app));
}

#[test]
fn the_menus_take_enter_whatever_the_keys_are_bound_to() {
    let mut app = playing_app();
    let controls = &mut app.settings.shared.controls;
    controls.keyboard.bind(Action::Start, Some(KeyCode::KeyP));
    controls.keyboard.bind(Action::A, Some(KeyCode::KeyK));
    let paused = |app: &App| app.game.as_ref().unwrap().is_paused();

    let _ = app.update(GameMessage::TogglePause.into());
    assert!(paused(&app));
    let _ = app.update(key_press(
        keyboard::Key::Character("k".into()),
        Some(KeyCode::KeyK),
    ));
    assert!(paused(&app));
    let _ = app.update(key_press(
        keyboard::Key::Named(keyboard::key::Named::Enter),
        Some(KeyCode::Enter),
    ));
    assert!(!paused(&app));
}

#[test]
fn waiting_for_a_button_gives_up_after_a_while() {
    let mut app = playing_app();
    let _ = app.update(SettingsMessage::Open.into());
    let _ = app.update(SettingsMessage::CaptureBinding(Device::Controller, Action::A).into());

    let _ = app.update(Message::Tick);
    assert!(app.capture.is_some());

    app.capture_started -= CAPTURE_TIMEOUT;
    let _ = app.update(Message::Tick);
    assert_eq!(app.capture, None);
}
