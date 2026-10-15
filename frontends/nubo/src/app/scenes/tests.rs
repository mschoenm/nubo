//! Tests for the names scenes give what is open on screen, and for where the
//! recorder saves scenes.

use super::*;

#[test]
fn every_pause_menu_page_reads_back_from_its_name() {
    for page in [
        PauseMenu::Main,
        PauseMenu::Save,
        PauseMenu::Load,
        PauseMenu::Cheats,
        PauseMenu::SuperGameBoy,
    ] {
        assert_eq!(pause_menu_page(pause_menu_name(page)), Ok(page));
    }
    assert!(pause_menu_page("options").is_err());
}

#[test]
fn every_settings_tab_reads_back_from_its_name() {
    for &tab in SettingsTab::all() {
        assert_eq!(settings_tab(settings_tab_name(tab)), Ok(tab));
    }
    assert!(settings_tab("audio").is_err());
}

#[test]
fn scenes_are_saved_in_a_new_folder_named_after_the_game() {
    let parent = std::env::temp_dir().join(format!("nubo-scenes-test-{}", std::process::id()));
    let first = free_folder(&parent, "Tobu Tobu Girl Deluxe");
    assert_eq!(first, parent.join("tobu-tobu-girl-deluxe-1"));

    std::fs::create_dir_all(&first).unwrap();
    assert_eq!(
        free_folder(&parent, "Tobu Tobu Girl Deluxe"),
        parent.join("tobu-tobu-girl-deluxe-2")
    );
    std::fs::remove_dir_all(&parent).unwrap();
}
