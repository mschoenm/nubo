//! Tests for the app's small helpers, for resetting the settings in a
//! `--fresh` app, which keeps nothing, and for shortcuts while the settings
//! are open.

use super::settings_window::reset_summary;
use super::{App, GameMessage, Message, SettingsMessage, cycle};
use crate::command_line::Options;
use crate::dialog::{DialogKind, ResetChoices};
use crate::input::KeyInput;
use crate::settings::Appearance;
use crate::views::settings::SettingsTab;
use iced::keyboard::{self, Modifiers};
use nubo_core::{ControllerButton, ControllerState};
use nubo_frontend::gamepads::{ControllerEvent, Controllers};
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
    app.remembered = Some(Rom {
        path: PathBuf::from("game.gb"),
        bytes: Arc::from(vec![0; 0x8000]),
    });

    let _ = app.update(GameMessage::PlayRemembered.into());
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
    app.remembered = Some(Rom {
        path: PathBuf::from("game.gb"),
        bytes: Arc::from(vec![0; 3]),
    });

    let _ = app.update(GameMessage::PlayRemembered.into());
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
    app.remembered = Some(Rom {
        path: PathBuf::from("game.gb"),
        bytes: Arc::from(vec![0; 0x8000]),
    });
    let _ = app.update(GameMessage::PlayRemembered.into());
    let pad = |buttons: &[ControllerButton]| {
        let mut state = ControllerState::new();
        for &button in buttons {
            state.set_button(button, true);
        }
        Message::Controller(ControllerEvent::Changed(Controllers::from(vec![state])))
    };
    let rewinding = |app: &App| app.game.as_ref().unwrap().session.is_rewinding();

    let _ = app.update(SettingsMessage::Open.into());
    let _ = app.update(pad(&[ControllerButton::West]));
    assert!(!rewinding(&app));
    let _ = app.update(pad(&[ControllerButton::LeftShoulder]));
    assert!(!app.game.as_ref().unwrap().is_paused());
    let _ = app.update(pad(&[]));

    let _ = app.update(SettingsMessage::Close.into());
    let _ = app.update(pad(&[ControllerButton::West]));
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
    app.remembered = Some(Rom {
        path: PathBuf::from("game.gb"),
        bytes: Arc::from(vec![0; 0x8000]),
    });
    let _ = app.update(GameMessage::PlayRemembered.into());
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
