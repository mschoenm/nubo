//! Tests for the rows the D-pad moves through in the settings.

use super::*;

/// The page state with `restart_needed`, `device` and `box_art_problem`.
fn state(restart_needed: bool, device: Device, box_art_problem: bool) -> PageState<'static> {
    PageState {
        restart_needed,
        device,
        capture: None,
        controllers: &[],
        box_art_problem: box_art_problem.then_some("offline"),
    }
}

/// The rows the page of `tab` draws, in the order it draws them.
fn drawn_rows(tab: SettingsTab, settings: &Settings, state: &PageState<'_>) -> Vec<SettingsRow> {
    let library = Library::default();
    let rings = Rings::new(None);

    let _ = page(tab, settings, &library, state, &rings);
    rings.drawn.into_inner()
}

#[test]
fn every_page_draws_its_rows_in_the_order_the_d_pad_moves_through_them() {
    let mut settings = Settings::default();
    for name in ["a", "b"] {
        settings.shared.library_folders.push(LibraryFolder {
            path: name.into(),
            include_subfolders: true,
        });
    }

    for &tab in SettingsTab::all() {
        for restart_needed in [false, true] {
            for device in Device::ALL {
                for box_art_problem in [false, true] {
                    let state = state(restart_needed, device, box_art_problem);
                    assert_eq!(
                        drawn_rows(tab, &settings, &state),
                        rows(tab, &settings, &state),
                        "{tab:?} with restart_needed {restart_needed}, {device:?} and \
                         box_art_problem {box_art_problem}"
                    );
                }
            }
        }
    }
}

#[test]
fn restart_now_is_only_a_row_when_it_shows() {
    let settings = Settings::default();

    let without = rows(
        SettingsTab::Playing,
        &settings,
        &state(false, Device::Keyboard, false),
    );
    let with = rows(
        SettingsTab::Playing,
        &settings,
        &state(true, Device::Keyboard, false),
    );

    assert!(!without.contains(&SettingsRow::RestartNow));
    assert_eq!(with.len(), without.len() + 1);
}

#[test]
fn the_controls_rows_follow_the_device() {
    let settings = Settings::default();

    let keyboard = rows(
        SettingsTab::Controls,
        &settings,
        &state(false, Device::Keyboard, false),
    );
    let controller = rows(
        SettingsTab::Controls,
        &settings,
        &state(false, Device::Controller, false),
    );

    assert_eq!(
        keyboard.len(),
        2 + Action::GAME_BOY.len() + Action::KEYBOARD_SHORTCUTS.len()
    );
    assert!(!keyboard.contains(&SettingsRow::Binding(Action::PauseMenu)));
    assert!(controller.contains(&SettingsRow::Binding(Action::PauseMenu)));
}

#[test]
fn every_library_folder_is_a_row() {
    let mut settings = Settings::default();
    for name in ["a", "b"] {
        settings.shared.library_folders.push(LibraryFolder {
            path: name.into(),
            include_subfolders: true,
        });
    }

    let rows = rows(
        SettingsTab::Library,
        &settings,
        &state(false, Device::Keyboard, false),
    );

    assert_eq!(
        rows[..4],
        [
            SettingsRow::Rescan,
            SettingsRow::Folder(0),
            SettingsRow::Folder(1),
            SettingsRow::AddFolder
        ]
    );
}

#[test]
fn every_tab_but_about_has_rows() {
    let settings = Settings::default();

    for &tab in SettingsTab::all() {
        let tab_rows = rows(tab, &settings, &state(false, Device::Keyboard, false));
        assert_eq!(tab_rows.is_empty(), tab == SettingsTab::About, "{tab:?}");
    }
}

#[test]
fn the_revision_row_shows_for_consoles_with_several_revisions() {
    let mut settings = Settings::default();
    let state = state(false, Device::Keyboard, false);

    for (model, shown) in [
        (ModelPreference::Automatic, false),
        (ModelPreference::Fixed(Model::Mgb), false),
        (ModelPreference::Fixed(Model::CgbC), true),
        (ModelPreference::Fixed(Model::Sgb2), true),
    ] {
        settings.shared.model = model;
        let rows = rows(SettingsTab::Playing, &settings, &state);
        assert_eq!(
            rows.contains(&SettingsRow::ModelRevision),
            shown,
            "{model:?}"
        );
        assert_eq!(
            drawn_rows(SettingsTab::Playing, &settings, &state),
            rows,
            "{model:?}"
        );
    }
}

#[test]
fn picking_a_console_keeps_its_revision_or_picks_the_recommended_one() {
    let color_c = ModelPreference::Fixed(Model::CgbC);

    assert_eq!(
        ModelOption::Console(Console::Color).preference(color_c),
        color_c
    );
    assert_eq!(
        ModelOption::Console(Console::GameBoy).preference(color_c),
        ModelPreference::Fixed(Model::Dmg)
    );
    assert_eq!(
        ModelOption::Automatic.preference(color_c),
        ModelPreference::Automatic
    );
}
