//! Tests for the rows the D-pad moves through in the settings.

use super::*;

/// The rows the page of `tab` draws, in the order it draws them.
fn drawn_rows(
    tab: SettingsTab,
    settings: &Settings,
    restart_needed: bool,
    device: Device,
) -> Vec<SettingsRow> {
    let library = Library::default();
    let controls_page = ControlsPage {
        device,
        capture: None,
        controllers: &[],
    };
    let rings = Rings::new(None);

    let _ = page(
        tab,
        settings,
        &library,
        restart_needed,
        &controls_page,
        &rings,
    );
    rings.drawn.into_inner()
}

#[test]
fn every_page_draws_its_rows_in_the_order_the_d_pad_moves_through_them() {
    let mut settings = Settings::default();
    for name in ["a", "b"] {
        settings.library_folders.push(LibraryFolder {
            path: name.into(),
            include_subfolders: true,
        });
    }

    for &tab in SettingsTab::all() {
        for restart_needed in [false, true] {
            for device in Device::ALL {
                assert_eq!(
                    drawn_rows(tab, &settings, restart_needed, device),
                    rows(tab, &settings, restart_needed, device),
                    "{tab:?} with restart_needed {restart_needed} and {device:?}"
                );
            }
        }
    }
}

#[test]
fn restart_now_is_only_a_row_when_it_shows() {
    let settings = Settings::default();

    let without = rows(SettingsTab::General, &settings, false, Device::Keyboard);
    let with = rows(SettingsTab::General, &settings, true, Device::Keyboard);

    assert!(!without.contains(&SettingsRow::RestartNow));
    assert_eq!(with.len(), without.len() + 1);
}

#[test]
fn the_controls_rows_follow_the_device() {
    let settings = Settings::default();

    let keyboard = rows(SettingsTab::Controls, &settings, false, Device::Keyboard);
    let controller = rows(SettingsTab::Controls, &settings, false, Device::Controller);

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
        settings.library_folders.push(LibraryFolder {
            path: name.into(),
            include_subfolders: true,
        });
    }

    let rows = rows(SettingsTab::Library, &settings, false, Device::Keyboard);

    assert_eq!(rows[..2], [SettingsRow::Folder(0), SettingsRow::Folder(1)]);
    assert_eq!(rows[2], SettingsRow::AddFolder);
}
