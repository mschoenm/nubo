//! Tests for the app's small helpers, and for resetting the settings in a
//! `--fresh` app, which keeps nothing.

use super::{App, GameMessage, SettingsMessage, cycle};
use crate::command_line::Options;
use crate::dialog::{DialogKind, ResetChoices};
use crate::settings::Appearance;
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
fn a_reset_that_deletes_takes_away_an_earlier_resets_undo() {
    let (mut app, _) = App::new(
        None,
        &Options {
            fresh: true,
            ..Options::default()
        },
    );
    app.settings.appearance = Appearance::Dark;
    let _ = app.update(SettingsMessage::Reset(ResetChoices::default()).into());
    app.settings.appearance = Appearance::Light;

    let _ = app.update(
        SettingsMessage::Reset(ResetChoices {
            delete_battery_saves: true,
            ..ResetChoices::default()
        })
        .into(),
    );
    let _ = app.update(GameMessage::Undo.into());

    assert_eq!(app.settings.appearance, Appearance::System);
}

#[test]
fn a_game_waits_for_the_saves_a_reset_deletes() {
    let (mut app, _) = App::new(
        None,
        &Options {
            fresh: true,
            ..Options::default()
        },
    );
    app.deleting_saves = true;
    app.remembered = Some(Rom {
        path: PathBuf::from("game.gb"),
        bytes: Arc::from(vec![0; 0x8000]),
    });

    let _ = app.update(GameMessage::PlayRemembered.into());

    assert!(app.game.is_none());
    assert!(app.start_after_deleting.is_some());
}
