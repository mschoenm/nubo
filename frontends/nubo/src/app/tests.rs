//! Tests for the app's small helpers, for resetting the settings in a
//! `--fresh` app, which keeps nothing, for shortcuts while the settings
//! are open or while Ctrl plays a Game Boy button, for dialogs whose
//! content is gone, for a state that fails to load on another model, and
//! for the fixed keys and buttons of the menus.

use super::playing::{PendingStart, ResumeAnswer, UndoPoint};
use super::settings_window::reset_summary;
use super::{App, CAPTURE_TIMEOUT, GameMessage, LibraryMessage, Message, SettingsMessage, cycle};
use crate::command_line::Options;
use crate::dialog::{Dialog, DialogKind, GameMenuPage, ResetChoices};
use crate::game::PauseMenu;
use crate::input::KeyInput;
use crate::settings::Appearance;
use crate::views::settings::SettingsTab;
use iced::keyboard::{self, Modifiers};
use nubo_core::{ControllerButton, ControllerState, Emulator, Model};
use nubo_frontend::controls::{Action, Controls, Device, KeyCode, MenuButtons, PadButton};
use nubo_frontend::gamepads::{ControllerEvent, Controllers};
use nubo_frontend::library::{Cache, Library};
use nubo_frontend::model::{Console, ModelPreference};
use nubo_frontend::rom::Rom;
use nubo_frontend::save_states::{self, SaveState, Slot};
use nubo_frontend::storage::files::use_folder;
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
    assert_eq!(app.settings, changed.reset());
    assert_eq!(app.settings.appearance, Appearance::System);
    assert!((app.settings.shared.volume - 1.0).abs() < f32::EPSILON);

    let _ = app.update(GameMessage::Undo.into());
    assert_eq!(app.settings, changed);
}

#[test]
fn undoing_a_reset_puts_back_the_model_picked_on_the_command_line() {
    let (mut app, _) = App::new(
        None,
        &Options {
            fresh: true,
            model: Some(Model::Sgb),
            ..Options::default()
        },
    );

    let _ = app.update(SettingsMessage::Reset(ResetChoices::default()).into());
    assert_eq!(app.model_override, None);

    let _ = app.update(GameMessage::Undo.into());
    assert_eq!(app.model_override, Some(Model::Sgb));
}

#[test]
fn a_reset_keeps_the_controls_unless_their_box_is_ticked() {
    let (mut app, _) = App::new(
        None,
        &Options {
            fresh: true,
            ..Options::default()
        },
    );
    app.settings.shared.controls.menu_buttons = MenuButtons::Xbox;
    let changed = app.settings.clone();

    let _ = app.update(SettingsMessage::Reset(ResetChoices::default()).into());
    assert_eq!(app.settings.shared.controls, changed.shared.controls);

    let _ = app.update(
        SettingsMessage::Reset(ResetChoices {
            reset_settings: false,
            reset_controls: true,
            ..ResetChoices::default()
        })
        .into(),
    );
    assert_eq!(app.settings.shared.controls, Controls::default());

    let _ = app.update(GameMessage::Undo.into());
    assert_eq!(app.settings.shared.controls, changed.shared.controls);
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
        GameMessage::RomPicked(Some(Ok(Rom {
            path: PathBuf::from("game.gb"),
            bytes: Arc::from(vec![0; 0x8000]),
        })))
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
        GameMessage::RomPicked(Some(Ok(Rom {
            path: PathBuf::from("game.gb"),
            bytes: Arc::from(vec![0; 3]),
        })))
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
        GameMessage::RomPicked(Some(Ok(Rom {
            path: PathBuf::from("game.gb"),
            bytes: Arc::from(vec![0; 0x8000]),
        })))
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
        GameMessage::RomPicked(Some(Ok(Rom {
            path: PathBuf::from("game.gb"),
            bytes: Arc::from(vec![0; 0x8000]),
        })))
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

/// `key` going down with Ctrl held, as the left Ctrl key reports it.
#[cfg(not(target_os = "macos"))]
fn ctrl_press(key: keyboard::Key, code: Option<KeyCode>) -> Message {
    Message::Key(KeyInput {
        code,
        key,
        modifiers: Modifiers::CTRL,
        pressed: true,
        repeat: false,
    })
}

/// A game playing with B on the left Ctrl key and Rewind on R.
#[cfg(not(target_os = "macos"))]
fn app_with_b_on_ctrl() -> App {
    let mut app = playing_app();
    let keyboard = &mut app.settings.shared.controls.keyboard;
    keyboard.bind(Action::B, Some(KeyCode::ControlLeft));
    keyboard.bind(Action::Rewind, Some(KeyCode::KeyR));
    app
}

#[cfg(not(target_os = "macos"))]
#[test]
fn ctrl_bound_to_a_game_boy_button_lets_the_other_keys_through() {
    let mut app = app_with_b_on_ctrl();
    let rewinding = |app: &App| app.game.as_ref().unwrap().session.is_rewinding();

    let _ = app.update(ctrl_press(
        keyboard::Key::Named(keyboard::key::Named::Control),
        Some(KeyCode::ControlLeft),
    ));
    // Ctrl+R rewinds instead of restarting the game.
    let _ = app.update(ctrl_press(
        keyboard::Key::Character("r".into()),
        Some(KeyCode::KeyR),
    ));
    assert!(rewinding(&app));
    // Ctrl+P is P, which isn't bound, so the game keeps running.
    let _ = app.update(ctrl_press(keyboard::Key::Character("p".into()), None));
    assert!(!app.game.as_ref().unwrap().is_paused());
    // Esc still opens the pause menu.
    let _ = app.update(ctrl_press(
        keyboard::Key::Named(keyboard::key::Named::Escape),
        None,
    ));
    assert!(app.game.as_ref().unwrap().is_paused());
}

#[cfg(not(target_os = "macos"))]
#[test]
fn ctrl_bound_to_a_game_boy_button_is_a_shortcut_again_in_the_pause_menu() {
    let mut app = app_with_b_on_ctrl();
    let _ = app.update(GameMessage::TogglePause.into());

    let _ = app.update(ctrl_press(keyboard::Key::Character("p".into()), None));
    assert!(!app.game.as_ref().unwrap().is_paused());
}

#[cfg(not(target_os = "macos"))]
#[test]
fn ctrl_bound_to_a_shortcut_still_drops_the_other_keys() {
    let mut app = playing_app();
    let keyboard = &mut app.settings.shared.controls.keyboard;
    keyboard.bind(Action::FastForward, Some(KeyCode::ControlLeft));
    keyboard.bind(Action::Rewind, Some(KeyCode::KeyX));

    let _ = app.update(ctrl_press(
        keyboard::Key::Character("x".into()),
        Some(KeyCode::KeyX),
    ));
    assert!(!app.game.as_ref().unwrap().session.is_rewinding());
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
        GameMessage::RomPicked(Some(Ok(Rom {
            path: PathBuf::from("other.gb"),
            bytes: Arc::from(vec![0; 0x8000]),
        })))
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
fn a_question_the_model_question_replaced_comes_back_after_the_answer() {
    for answer in [
        ResumeAnswer::Continue,
        ResumeAnswer::StartOver,
        ResumeAnswer::Cancel,
    ] {
        let mut app = waiting_over_box_art();
        let _ = app.update(GameMessage::ResumeAnswered(answer).into());

        assert_eq!(
            app.dialog.map(|dialog| dialog.kind),
            Some(DialogKind::BoxArt)
        );
    }
}

#[test]
fn a_library_menu_the_model_question_replaced_comes_back_only_on_cancel() {
    for (answer, kind) in [
        (ResumeAnswer::Continue, None),
        (ResumeAnswer::Cancel, Some(DialogKind::SortMenu)),
    ] {
        let mut app = waiting_app();
        app.pending_start.as_mut().unwrap().replaced = Some(Dialog::new(DialogKind::SortMenu));
        let _ = app.update(GameMessage::ResumeAnswered(answer).into());

        assert_eq!(app.dialog.map(|dialog| dialog.kind), kind);
    }
}

#[test]
fn another_game_starting_puts_back_what_the_waiting_one_replaced() {
    let mut app = waiting_over_box_art();
    start_other_game(&mut app);

    assert_eq!(
        app.dialog.map(|dialog| dialog.kind),
        Some(DialogKind::BoxArt)
    );
    assert!(app.pending_start.is_none());
}

#[test]
fn the_model_question_keeps_the_open_question_through_another_waiting_game() {
    let mut app = waiting_app();
    let pending = app.pending_start.take().unwrap();
    app.dialog = Some(Dialog::new(DialogKind::Updates));

    app.ask_which_model(
        pending.rom.clone(),
        pending.header.clone(),
        pending.state.clone(),
        Model::Dmg,
    );
    app.ask_which_model(pending.rom, pending.header, pending.state, Model::Dmg);
    assert!(matches!(
        app.dialog.as_ref().map(|dialog| &dialog.kind),
        Some(DialogKind::OtherModel { .. })
    ));

    let _ = app.update(GameMessage::ResumeAnswered(ResumeAnswer::Cancel).into());
    assert_eq!(
        app.dialog.map(|dialog| dialog.kind),
        Some(DialogKind::Updates)
    );
}

/// Starts `other.gb`, an empty game, as a dropped file or Ctrl+O would.
fn start_other_game(app: &mut App) {
    let _ = app.update(
        GameMessage::RomPicked(Some(Ok(Rom {
            path: PathBuf::from("other.gb"),
            bytes: Arc::from(vec![0; 0x8000]),
        })))
        .into(),
    );
}

#[test]
fn a_game_starting_closes_the_library_and_settings_dialogs() {
    for kind in [
        DialogKind::SortMenu,
        DialogKind::GameMenu(PathBuf::from("game.gb"), GameMenuPage::Main),
        DialogKind::FolderMenu(0),
        DialogKind::DeleteSaveStates(PathBuf::from("game.gb")),
        DialogKind::RemoveGame(PathBuf::from("game.gb")),
        DialogKind::ResetSettings(ResetChoices::default()),
    ] {
        let mut app = playing_app();
        app.dialog = Some(Dialog::new(kind.clone()));
        start_other_game(&mut app);

        assert!(app.dialog.is_none(), "{kind:?} stayed open");
        assert!(!app.game.as_ref().unwrap().is_paused());
    }
}

#[test]
fn a_game_starting_keeps_a_question_that_has_nothing_to_do_with_it() {
    for kind in [DialogKind::BoxArt, DialogKind::Updates] {
        let mut app = playing_app();
        app.dialog = Some(Dialog::new(kind.clone()));
        start_other_game(&mut app);

        assert_eq!(app.dialog.map(|dialog| dialog.kind), Some(kind));
    }
}

#[test]
fn a_game_starting_stops_waiting_for_a_key() {
    let mut app = playing_app();
    let _ = app.update(SettingsMessage::Open.into());
    let _ = app.update(SettingsMessage::CaptureBinding(Device::Keyboard, Action::A).into());
    assert!(app.capture.is_some());
    start_other_game(&mut app);

    assert_eq!(app.capture, None);
    assert_eq!(app.settings_focus, None);
    assert_eq!(app.settings_tab, None);
}

#[test]
fn a_game_continuing_after_the_model_question_stops_waiting_for_a_key() {
    let mut app = waiting_app();
    let _ = app.update(SettingsMessage::Open.into());
    let _ = app.update(SettingsMessage::CaptureBinding(Device::Keyboard, Action::A).into());
    let _ = app.update(GameMessage::ResumeAnswered(ResumeAnswer::Continue).into());

    assert!(app.game.is_some());
    assert_eq!(app.capture, None);
}

#[test]
fn continuing_keeps_a_game_on_the_settings_when_they_pick_its_model() {
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
        Some(ModelPreference::Fixed(Model::Dmg))
    );
    assert_eq!(preference(&app, Model::Sgb), None);
    assert_eq!(
        preference(&app, Model::CgbE),
        Some(ModelPreference::Fixed(Model::CgbE))
    );

    app.settings.shared.model = ModelPreference::Fixed(Model::Mgb);
    assert_eq!(preference(&app, Model::Mgb), None);
}

#[test]
fn continuing_sets_the_saves_model_only_once_the_game_has_continued() {
    let id = std::process::id();
    let folder = std::env::temp_dir().join(format!("nubo-continue-library-{id}"));
    // A name no player's game has, since the state that doesn't load is
    // kept aside in the save states of a game with this file name.
    let rom_path = PathBuf::from(format!("nubo-continue-test-{id}.gb"));
    for (loads, kept) in [
        (false, None),
        (true, Some(ModelPreference::Fixed(Model::Sgb))),
    ] {
        let _ = std::fs::remove_dir_all(&folder);
        let mut app = waiting_app();
        app.model_override = None;
        let pending = app.pending_start.as_mut().unwrap();
        pending.rom.path.clone_from(&rom_path);
        if !loads {
            pending.state.data = vec![0; 16];
        }
        let (mut library, _) = Library::load_from(Cache::in_folder(folder.clone()));
        library.record_played(&rom_path, &pending.header).unwrap();
        app.library = library;

        let _ = app.update(GameMessage::ResumeAnswered(ResumeAnswer::Continue).into());

        assert_eq!(
            app.game.as_ref().map(|game| game.session.model()),
            Some(Model::Sgb)
        );
        assert_eq!(app.library.entry(&rom_path).unwrap().preference(), kept);
    }
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_state_that_fails_on_another_model_puts_the_game_back() {
    let mut app = playing_app();
    let game = app.game.as_mut().unwrap();
    game.show_menu(Some(PauseMenu::Load));
    let model = game.session.model();
    let rom_path = game.session.rom().path.clone();
    let other = if model == Model::Sgb {
        Model::Dmg
    } else {
        Model::Sgb
    };
    app.undo = Some(UndoPoint::Game {
        rom_path: rom_path.clone(),
        state: SaveState {
            model: other,
            data: vec![1, 2, 3],
        },
    });

    let _ = app.update(GameMessage::Undo.into());

    let game = app.game.as_mut().unwrap();
    assert_eq!(game.session.model(), model);
    assert_eq!(game.session.rom().path, rom_path);
    assert_eq!(game.menu(), Some(PauseMenu::Load));
    assert!(game.session.snapshot().is_ok());
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
        replaced: None,
    });
    app
}

#[test]
fn a_scale_window_which_cant_ask_continues_on_the_saves_model() {
    let mut app = waiting_app();
    let pending = app.pending_start.take().unwrap();
    app.dialog = None;
    app.screen_scale = Some(1);

    app.start_or_ask(pending.rom, pending.header, Some(pending.state));

    assert!(app.dialog.is_none());
    assert!(app.pending_start.is_none());
    assert_eq!(app.game.as_ref().unwrap().session.model(), Model::Sgb);
}

/// `waiting_app`, its question having replaced the box art question.
fn waiting_over_box_art() -> App {
    let mut app = waiting_app();
    app.pending_start.as_mut().unwrap().replaced = Some(Dialog::new(DialogKind::BoxArt));
    app
}

/// Puts this test run's data, such as save states, in a folder of its own
/// rather than the player's, and returns its data folder. The folder is the
/// same for every test of the run, since only the first `use_folder` counts,
/// so each test writes the states of games named after itself.
fn test_data_folder() -> PathBuf {
    use_folder(&std::env::temp_dir().join(format!("nubo-app-tests-{}", std::process::id())));
    nubo_frontend::storage::files::data_folder().unwrap()
}

#[test]
fn undoing_a_save_over_a_slot_puts_its_picture_file_back_unchanged() {
    let data_folder = test_data_folder();
    let rom_path = PathBuf::from("nubo-undo-save-test.gb");
    let mut app = playing_app();
    let _ = app.update(
        GameMessage::RomPicked(Some(Ok(Rom {
            path: rom_path.clone(),
            bytes: Arc::from(vec![0; 0x8000]),
        })))
        .into(),
    );
    app.game.as_mut().unwrap().keeps_data = true;
    let old_state = SaveState {
        model: Model::Mgb,
        data: Emulator::new(Model::Mgb).save_state(),
    };
    // A picture file that encoding its picture again wouldn't give byte for
    // byte, as one an earlier version wrote may not.
    let pixels = vec![90; 160 * 144 * 4];
    let mut old_picture = nubo_frontend::storage::files::encode_png(160, 144, &pixels).unwrap();
    old_picture.extend_from_slice(b"written by another encoder");
    save_states::save_with_png(&rom_path, Slot::Quick, &old_state, Some(&old_picture)).unwrap();

    let _ = app.update(GameMessage::SaveState(Slot::Quick).into());
    assert_ne!(
        save_states::load(&rom_path, Slot::Quick)
            .unwrap()
            .unwrap()
            .data,
        old_state.data
    );
    let _ = app.update(GameMessage::Undo.into());

    let picture_file = data_folder.join("states").join(&rom_path).join("quick.png");
    assert_eq!(std::fs::read(&picture_file).unwrap(), old_picture);
    assert_eq!(
        save_states::load(&rom_path, Slot::Quick)
            .unwrap()
            .unwrap()
            .data,
        old_state.data
    );
    let _ = std::fs::remove_dir_all(data_folder.join("states").join(&rom_path));
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
        GameMessage::RomPicked(Some(Ok(Rom {
            path: PathBuf::from("game.gb"),
            bytes: Arc::from(vec![0; 0x8000]),
        })))
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

/// An app that keeps nothing else, with a Game Boy Color game in a library
/// kept in a temporary folder named after `name`. Returns the app, the
/// game's path and the folder, for the test to remove.
fn app_with_color_game(name: &str) -> (App, PathBuf, PathBuf) {
    let folder = std::env::temp_dir().join(format!("nubo-console-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    let (mut app, _) = App::new(
        None,
        &Options {
            fresh: true,
            ..Options::default()
        },
    );
    let mut rom = vec![0; 0x8000];
    rom[0x143] = 0x80;
    let header = Emulator::read_rom_header(&rom).unwrap();
    let path = PathBuf::from("Color Game.gbc");
    let (mut library, _) = Library::load_from(Cache::in_folder(folder.clone()));
    library.record_played(&path, &header).unwrap();
    app.library = library;
    (app, path, folder)
}

/// The label of the choice the D-pad is on in the open dialog.
fn selected_label(app: &App) -> String {
    let selection = app.dialog.as_ref().unwrap().selection;
    app.dialog_content().unwrap().choices[selection]
        .label
        .clone()
}

#[test]
fn picking_a_console_closes_the_menu_and_can_be_undone() {
    let (mut app, path, folder) = app_with_color_game("pick");
    app.show_game_menu(path.clone(), GameMenuPage::Main);
    assert_eq!(selected_label(&app), "Play");
    let _ = app.navigate_dialog(Action::Down, false);
    let _ = app.navigate_dialog(Action::A, false);

    assert_eq!(
        app.dialog.as_ref().map(|dialog| &dialog.kind),
        Some(&DialogKind::GameMenu(
            path.clone(),
            GameMenuPage::Consoles(None)
        ))
    );
    let content = app.dialog_content().unwrap();
    let color = &content.choices[app.dialog.as_ref().unwrap().selection];
    assert_eq!(color.label, "Game Boy Color");
    assert!(color.current);
    assert_eq!(color.note.as_deref(), Some("Made for this console"));
    assert_eq!(color.fold.as_ref().unwrap().label, "CGB-E");

    let _ = app.navigate_dialog(Action::Up, false);
    let _ = app.navigate_dialog(Action::Up, false);
    assert_eq!(selected_label(&app), "Game Boy Pocket");
    let _ = app.navigate_dialog(Action::A, false);

    assert!(app.dialog.is_none());
    let preference = |app: &App| app.library.entry(&path).unwrap().preference();
    assert_eq!(preference(&app), Some(ModelPreference::Fixed(Model::Mgb)));
    let _ = app.update(GameMessage::Undo.into());
    assert_eq!(preference(&app), None);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn the_console_row_says_when_the_command_line_picks_the_console() {
    let (mut app, path, folder) = app_with_color_game("override");
    let console_row = |app: &mut App| {
        app.show_game_menu(path.clone(), GameMenuPage::Main);
        let content = app.dialog_content().unwrap();
        let row = content
            .choices
            .into_iter()
            .find(|choice| choice.label == "Console")
            .unwrap();
        (row.hint, row.description)
    };

    assert_eq!(
        console_row(&mut app),
        (Some(String::from("Game Boy Color ›")), None)
    );

    app.model_override = Some(Model::Sgb);
    assert_eq!(
        console_row(&mut app),
        (
            Some(String::from("Game Boy Color ›")),
            Some(String::from(
                "This run starts every game on the Super Game Boy (--model)"
            ))
        )
    );
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn starting_a_game_takes_away_a_console_picks_undo() {
    let (mut app, path, folder) = app_with_color_game("start");
    let _ = app.update(LibraryMessage::GameModelPicked(path.clone(), Model::Mgb).into());
    assert!(matches!(app.undo, Some(UndoPoint::GameModel { .. })));

    let _ = app.update(
        GameMessage::RomPicked(Some(Ok(Rom {
            path: PathBuf::from("game.gb"),
            bytes: Arc::from(vec![0; 3]),
        })))
        .into(),
    );

    assert!(app.undo.is_none());
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn picking_the_console_the_settings_pick_follows_them_again() {
    let (mut app, path, folder) = app_with_color_game("settings");
    let preference = |app: &App| app.library.entry(&path).unwrap().preference();
    let pick = |app: &mut App, model| {
        let _ = app.update(LibraryMessage::GameModelPicked(path.clone(), model).into());
    };

    pick(&mut app, Model::CgbC);
    assert_eq!(preference(&app), Some(ModelPreference::Fixed(Model::CgbC)));
    pick(&mut app, Model::CgbE);
    assert_eq!(preference(&app), None);

    app.settings.shared.model = ModelPreference::Fixed(Model::Mgb);
    pick(&mut app, Model::CgbE);
    assert_eq!(preference(&app), Some(ModelPreference::Fixed(Model::CgbE)));
    pick(&mut app, Model::Mgb);
    assert_eq!(preference(&app), None);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn right_and_left_unfold_and_fold_a_consoles_revisions() {
    let (mut app, path, folder) = app_with_color_game("fold");
    app.show_game_menu(path.clone(), GameMenuPage::Consoles(None));
    assert_eq!(selected_label(&app), "Game Boy Color");

    let _ = app.navigate_dialog(Action::Right, false);
    assert_eq!(
        app.dialog.as_ref().map(|dialog| &dialog.kind),
        Some(&DialogKind::GameMenu(
            path.clone(),
            GameMenuPage::Consoles(Some(Console::Color))
        ))
    );
    assert_eq!(selected_label(&app), "Game Boy Color");
    let _ = app.navigate_dialog(Action::Down, false);
    assert_eq!(selected_label(&app), "CGB-0");

    let _ = app.navigate_dialog(Action::Left, false);
    assert_eq!(
        app.dialog.as_ref().map(|dialog| &dialog.kind),
        Some(&DialogKind::GameMenu(
            path.clone(),
            GameMenuPage::Consoles(None)
        ))
    );
    assert_eq!(selected_label(&app), "Game Boy Color");
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_revision_other_than_the_most_common_shows_unfolded_and_b_goes_back() {
    let (mut app, path, folder) = app_with_color_game("revision");
    app.library
        .set_model(&path, Some(ModelPreference::Fixed(Model::CgbC)))
        .unwrap();
    app.show_game_menu(path.clone(), GameMenuPage::Main);
    let _ = app.navigate_dialog(Action::Down, false);
    let _ = app.navigate_dialog(Action::A, false);

    assert_eq!(selected_label(&app), "CGB-C");
    let content = app.dialog_content().unwrap();
    let common = content
        .choices
        .iter()
        .find(|choice| choice.label == "CGB-E")
        .unwrap();
    assert_eq!(common.badge.as_deref(), Some("Most common"));
    assert!(!common.current);

    let _ = app.navigate_dialog(Action::B, false);
    assert_eq!(
        app.dialog.as_ref().map(|dialog| &dialog.kind),
        Some(&DialogKind::GameMenu(path.clone(), GameMenuPage::Main))
    );
    assert_eq!(selected_label(&app), "Console");
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_games_menu_offers_to_delete_its_save_states_until_they_are_deleted() {
    let (mut app, path, folder) = app_with_color_game("save-states");
    test_data_folder();
    app.fresh = false;
    let offers_deleting = |app: &App| {
        app.dialog_content()
            .unwrap()
            .choices
            .iter()
            .any(|choice| choice.label == "Delete save states…")
    };
    let state = SaveState {
        model: Model::CgbE,
        data: vec![1, 2, 3],
    };

    app.show_game_menu(path.clone(), GameMenuPage::Main);
    assert!(!offers_deleting(&app));

    save_states::save(&path, Slot::Quick, &state, None).unwrap();
    app.show_game_menu(path.clone(), GameMenuPage::Main);
    assert!(offers_deleting(&app));
    let _ = app.update(LibraryMessage::DeleteSaveStates(path.clone()).into());
    assert!(!offers_deleting(&app));

    save_states::save(&path, Slot::Quick, &state, None).unwrap();
    app.show_game_menu(path.clone(), GameMenuPage::Main);
    save_states::delete(&path, Slot::Quick).unwrap();
    assert!(offers_deleting(&app), "worked out again on every redraw");
    let _ = app.update(
        SettingsMessage::SavesDeleted {
            done: Vec::new(),
            deleted: vec!["save states deleted"],
            errors: Vec::new(),
        }
        .into(),
    );
    assert!(!offers_deleting(&app));
    let _ = std::fs::remove_dir_all(&folder);
}
