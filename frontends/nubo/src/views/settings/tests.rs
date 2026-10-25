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
        newer_version: None,
        layout: SettingsLayout::new(936.0),
        keeps_library: true,
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
                    for window_width in [936.0, 620.0, 480.0] {
                        let state = PageState {
                            layout: SettingsLayout::new(window_width),
                            ..state(restart_needed, device, box_art_problem)
                        };
                        assert_eq!(
                            drawn_rows(tab, &settings, &state),
                            rows(tab, &settings, &state),
                            "{tab:?} with restart_needed {restart_needed}, {device:?}, \
                             box_art_problem {box_art_problem} and a window \
                             {window_width} wide"
                        );
                    }
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
            SettingsRow::Folder(0),
            SettingsRow::Folder(1),
            SettingsRow::AddFolder,
            SettingsRow::BoxArt
        ]
    );
}

#[test]
fn every_tab_has_rows() {
    let settings = Settings::default();

    for &tab in SettingsTab::all() {
        let tab_rows = rows(tab, &settings, &state(false, Device::Keyboard, false));
        assert!(!tab_rows.is_empty(), "{tab:?}");
    }
}

#[test]
fn the_focus_stays_on_the_last_row_when_the_row_under_it_goes_away() {
    let settings = Settings::default();
    let with_try_again = rows(
        SettingsTab::Library,
        &settings,
        &state(false, Device::Keyboard, true),
    );
    let without = rows(
        SettingsTab::Library,
        &settings,
        &state(false, Device::Keyboard, false),
    );
    let focus = with_try_again.len() - 1;
    assert!(focus >= without.len());

    assert_eq!(
        focused_row(&without, focus),
        Some((without.len() - 1, without[without.len() - 1]))
    );
    assert_eq!(focused_row(&[], 0), None);
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

#[test]
fn the_tabs_move_above_the_page_in_a_narrow_window() {
    let usual = SettingsLayout::new(936.0);
    assert!(usual.has_sidebar());
    assert!(usual.names_every_tab());
    assert!(!usual.stacks_wide_controls());

    assert!(
        SettingsLayout::new(0.0).has_sidebar(),
        "width not known yet"
    );
    assert!(SettingsLayout::new(760.0).has_sidebar());

    let medium = SettingsLayout::new(700.0);
    assert!(!medium.has_sidebar());
    assert!(medium.names_every_tab());
    assert!(!medium.stacks_wide_controls());

    let narrow = SettingsLayout::new(620.0);
    assert!(!narrow.has_sidebar());
    assert!(!narrow.names_every_tab());
    assert!(!narrow.stacks_wide_controls());

    let smallest = SettingsLayout::new(480.0);
    assert!(!smallest.has_sidebar());
    assert!(!smallest.names_every_tab());
    assert!(smallest.stacks_wide_controls());
}

#[test]
fn the_d_pad_follows_the_tabs_along_the_sidebar_or_the_row() {
    let sidebar = SettingsLayout::new(936.0);
    assert_eq!(sidebar.tab_step(Action::Up), Some(-1));
    assert_eq!(sidebar.tab_step(Action::Down), Some(1));
    assert_eq!(sidebar.tab_step(Action::Left), None);
    assert!(sidebar.enters_page(Action::Right));
    assert!(!sidebar.enters_page(Action::Down));
    assert!(sidebar.enters_page(Action::A));
    assert!(!sidebar.up_leaves_page());

    let row = SettingsLayout::new(480.0);
    assert_eq!(row.tab_step(Action::Left), Some(-1));
    assert_eq!(row.tab_step(Action::Right), Some(1));
    assert_eq!(row.tab_step(Action::Up), None);
    assert!(row.enters_page(Action::Down));
    assert!(!row.enters_page(Action::Right));
    assert!(row.enters_page(Action::Start));
    assert!(row.up_leaves_page());
}

#[test]
fn a_run_that_keeps_no_library_offers_no_game_folders() {
    let mut settings = Settings::default();
    settings.shared.library_folders.push(LibraryFolder {
        path: "a".into(),
        include_subfolders: true,
    });
    let state = PageState {
        keeps_library: false,
        ..state(false, Device::Keyboard, false)
    };

    let rows = rows(SettingsTab::Library, &settings, &state);
    assert!(
        !rows
            .iter()
            .any(|row| matches!(row, SettingsRow::Folder(_) | SettingsRow::AddFolder)),
        "{rows:?}"
    );
    assert_eq!(drawn_rows(SettingsTab::Library, &settings, &state), rows);
}
