//! Tests for what the dialogs offer.

use super::*;

/// Every box the reset can offer.
const EVERYTHING: ResetOffers = ResetOffers {
    delete_battery_saves: true,
    delete_save_states: true,
    remove_folders: true,
    clear_recently_played: true,
    close_game_first: false,
};

/// The labels of `content`'s checkboxes, then of its buttons.
fn labels(content: &DialogContent) -> (Vec<&str>, Vec<&str>) {
    let (boxes, buttons): (Vec<&Choice>, Vec<&Choice>) =
        content.choices.iter().partition(|choice| choice.checkbox);
    (
        boxes.iter().map(|choice| choice.label.as_str()).collect(),
        buttons.iter().map(|choice| choice.label.as_str()).collect(),
    )
}

#[test]
fn the_reset_starts_with_nothing_ticked_and_its_boxes_above_its_buttons() {
    let content = reset_question(ResetChoices::default(), EVERYTHING);

    let first_button = content.choices.iter().position(|choice| !choice.checkbox);
    assert_eq!(first_button, Some(4));
    assert!(content.choices.iter().all(|choice| !choice.checked));
    assert_eq!(labels(&content).1, ["Cancel", "Reset"]);
    assert!(!content.notice.unwrap().warning);
}

#[test]
fn only_what_is_offered_gets_a_box() {
    let offers = ResetOffers {
        delete_battery_saves: false,
        delete_save_states: false,
        close_game_first: true,
        ..EVERYTHING
    };

    let content = reset_question(ResetChoices::default(), offers);

    assert_eq!(labels(&content).0.len(), 2);
    assert!(
        content
            .text
            .unwrap()
            .contains("To delete saves too, close the game first.")
    );
}

#[test]
fn ticking_a_box_changes_only_its_choice() {
    let content = reset_question(ResetChoices::default(), EVERYTHING);

    let DialogAction::Stay(Message::Dialog(DialogMessage::ResetChoicesChanged(changed))) =
        &content.choices[2].action
    else {
        panic!("a box should keep the dialog open and change the choices");
    };
    assert_eq!(
        *changed,
        ResetChoices {
            remove_folders: true,
            ..ResetChoices::default()
        }
    );
}

#[test]
fn deleting_saves_warns_and_names_the_button_after_it() {
    let choices = ResetChoices {
        delete_battery_saves: true,
        ..ResetChoices::default()
    };

    let content = reset_question(choices, EVERYTHING);

    assert!(content.choices[0].checked);
    assert_eq!(labels(&content).1, ["Cancel", "Reset and delete"]);
    let notice = content.notice.unwrap();
    assert!(notice.warning);
    assert_eq!(notice.text, "Deleted saves can't be brought back.");
}

#[test]
fn a_ticked_box_no_longer_offered_deletes_nothing() {
    let choices = ResetChoices {
        delete_battery_saves: true,
        ..ResetChoices::default()
    };
    let offers = ResetOffers {
        delete_battery_saves: false,
        delete_save_states: false,
        close_game_first: true,
        ..EVERYTHING
    };

    let content = reset_question(choices, offers);

    assert_eq!(labels(&content).1, ["Cancel", "Reset"]);
    assert!(!content.notice.unwrap().warning);
}
