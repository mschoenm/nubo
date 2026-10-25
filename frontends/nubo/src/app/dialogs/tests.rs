//! Tests for what the dialogs offer.

use super::*;

/// Every box the reset can offer.
const EVERYTHING: ResetOffers = ResetOffers {
    delete_battery_saves: true,
    delete_save_states: true,
    remove_folders: true,
    clear_recently_played: true,
    game_open: false,
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

/// The button that does the reset, the last choice.
fn reset_button(content: &DialogContent) -> &Choice {
    content.choices.last().unwrap()
}

#[test]
fn starting_over_ticks_only_the_settings_and_puts_the_boxes_above_the_buttons() {
    let content = reset_question(ResetChoices::default(), EVERYTHING);

    let first_button = content.choices.iter().position(|choice| !choice.checkbox);
    assert_eq!(first_button, Some(6));
    assert_eq!(
        labels(&content).0[..2],
        ["Reset settings", "Reset controls"]
    );
    let ticked: Vec<bool> = content
        .choices
        .iter()
        .map(|choice| choice.checked)
        .collect();
    assert_eq!(
        ticked,
        [true, false, false, false, false, false, false, false]
    );
    assert_eq!(labels(&content).1, ["Cancel", "Reset"]);
    assert!(reset_button(&content).enabled);
    assert!(!content.notice.unwrap().warning);
}

#[test]
fn only_what_is_offered_gets_a_box() {
    let offers = ResetOffers {
        delete_battery_saves: false,
        delete_save_states: false,
        ..EVERYTHING
    };

    let content = reset_question(ResetChoices::default(), offers);

    assert_eq!(
        labels(&content).0,
        [
            "Reset settings",
            "Reset controls",
            "Remove game folders",
            "Clear Continue playing"
        ]
    );
}

#[test]
fn an_open_game_grays_out_deleting_saves_and_says_why() {
    let offers = ResetOffers {
        game_open: true,
        ..EVERYTHING
    };

    let content = reset_question(ResetChoices::default(), offers);

    assert_eq!(
        labels(&content).0,
        [
            "Reset settings",
            "Reset controls",
            "Delete in-game saves",
            "Delete save states",
            "Remove game folders",
            "Clear Continue playing"
        ]
    );
    let enabled: Vec<bool> = content.choices[..6]
        .iter()
        .map(|choice| choice.enabled)
        .collect();
    assert_eq!(enabled, [true, true, false, false, true, true]);
    for choice in &content.choices[2..4] {
        assert!(!choice.checked);
        assert!(
            choice
                .description
                .as_deref()
                .unwrap()
                .contains("Close the game first")
        );
    }
}

#[test]
fn ticking_a_box_changes_only_its_choice() {
    let content = reset_question(ResetChoices::default(), EVERYTHING);

    let DialogAction::Stay(Message::Dialog(DialogMessage::ResetChoicesChanged(changed))) =
        &content.choices[4].action
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

    assert!(content.choices[2].checked);
    assert_eq!(labels(&content).1, ["Cancel", "Reset and delete"]);
    let notice = content.notice.unwrap();
    assert!(notice.warning);
    assert_eq!(notice.text, "Deleted saves can't be brought back.");
}

#[test]
fn the_button_says_what_it_does_without_the_settings() {
    let only_saves = ResetChoices {
        reset_settings: false,
        delete_save_states: true,
        ..ResetChoices::default()
    };
    let only_recent = ResetChoices {
        reset_settings: false,
        clear_recently_played: true,
        ..ResetChoices::default()
    };

    assert_eq!(
        reset_button(&reset_question(only_saves, EVERYTHING)).label,
        "Delete"
    );
    assert_eq!(
        reset_button(&reset_question(only_recent, EVERYTHING)).label,
        "Clear"
    );
}

#[test]
fn the_controls_alone_can_be_reset() {
    let only_controls = ResetChoices {
        reset_settings: false,
        reset_controls: true,
        ..ResetChoices::default()
    };

    let content = reset_question(only_controls, EVERYTHING);

    assert!(content.choices[1].checked);
    assert_eq!(labels(&content).1, ["Cancel", "Reset"]);
    assert!(reset_button(&content).enabled);
    assert!(!content.notice.unwrap().warning);
}

#[test]
fn nothing_ticked_grays_the_button_out() {
    let nothing = ResetChoices {
        reset_settings: false,
        ..ResetChoices::default()
    };

    let content = reset_question(nothing, EVERYTHING);

    assert!(!reset_button(&content).enabled);
    assert!(content.choices[..7].iter().all(|choice| choice.enabled));
}

#[test]
fn a_ticked_box_no_longer_offered_deletes_nothing() {
    let choices = ResetChoices {
        delete_battery_saves: true,
        ..ResetChoices::default()
    };
    let offers = ResetOffers {
        game_open: true,
        ..EVERYTHING
    };

    let content = reset_question(choices, offers);

    assert!(!content.choices[2].checked);
    assert_eq!(labels(&content).1, ["Cancel", "Reset"]);
    assert!(!content.notice.unwrap().warning);
}

#[test]
fn the_other_model_question_names_the_consoles_and_stays_first() {
    let content = other_model_question(Model::Sgb, Model::Dmg, true);

    assert_eq!(content.title, "Switch to the Game Boy?");
    assert_eq!(
        content.text.as_deref(),
        Some(
            "You last played this game on the Super Game Boy, but it's now set to the Game \
             Boy.\n\nSwitching starts the game at the title screen and replaces your automatic \
             save. Your in-game saves stay.\n\nStaying continues where you left off and keeps \
             the game on the Super Game Boy."
        )
    );
    let (_, buttons) = labels(&content);
    assert_eq!(
        buttons,
        ["Stay on the Super Game Boy", "Switch to the Game Boy"]
    );
    assert_eq!(content.choices[0].style, ChoiceStyle::Primary);
}

#[test]
fn the_other_model_question_names_revisions_of_one_console() {
    let content = other_model_question(Model::CgbC, Model::CgbE, true);

    assert_eq!(content.title, "Switch to the CGB-E?");
    let (_, buttons) = labels(&content);
    assert_eq!(buttons, ["Stay on the CGB-C", "Switch to the CGB-E"]);
}

#[test]
fn the_other_model_question_says_nothing_about_keeping_a_model_from_the_command_line() {
    let content = other_model_question(Model::Sgb, Model::Dmg, false);

    assert!(
        content
            .text
            .unwrap()
            .ends_with("Staying continues where you left off.")
    );
}
