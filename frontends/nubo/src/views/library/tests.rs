//! Tests for moving through the library with the D-pad.

use super::*;

/// A window 700 pixels wide fits three cards side by side.
const WIDTH: f32 = 700.0;

fn entries(count: usize) -> Vec<LibraryEntry> {
    (0..count)
        .map(|index| LibraryEntry::example(&format!("Game {index}.gb")))
        .collect()
}

/// Two recently played games, then five games in three columns:
///
/// ```text
/// 0 1
/// 2 3 4
/// 5 6
/// ```
fn shelves<'a>(recent: &'a [LibraryEntry], games: &'a [LibraryEntry]) -> Shelves<'a> {
    Shelves {
        recent: recent.iter().collect(),
        games: games.iter().collect(),
    }
}

#[test]
fn the_grid_fits_as_many_columns_as_iced_does() {
    assert_eq!(columns(WIDTH), 3);
    // Like Iced's grid, a partly filled column still counts, so the cards
    // shrink rather than leave a gap.
    assert_eq!(columns(300.0), 2);
    assert_eq!(columns(200.0), 1);
}

#[test]
fn the_starting_window_fits_a_whole_continue_playing_row() {
    assert_eq!(columns(FULL_ROW_WINDOW_WIDTH), STARTING_COLUMNS);
    // The cards are at their full size.
    let full_height = CARD_MAX_WIDTH * RECENT_CARD_ASPECT.1 / RECENT_CARD_ASPECT.0;
    assert!((card_height(FULL_ROW_WINDOW_WIDTH, true) - full_height).abs() < 0.01);
}

#[test]
fn left_and_right_move_one_card_and_stop_at_the_ends() {
    let (recent, games) = (entries(2), entries(5));
    let shelves = shelves(&recent, &games);

    assert_eq!(shelves.step(0, Action::Left, WIDTH, false), 0);
    assert_eq!(shelves.step(1, Action::Right, WIDTH, false), 2);
    assert_eq!(shelves.step(6, Action::Right, WIDTH, false), 6);
}

#[test]
fn down_moves_from_continue_playing_to_all_games() {
    let (recent, games) = (entries(2), entries(5));
    let shelves = shelves(&recent, &games);

    assert_eq!(shelves.step(0, Action::Down, WIDTH, false), 2);
    assert_eq!(shelves.step(1, Action::Down, WIDTH, false), 3);
    assert_eq!(shelves.step(2, Action::Down, WIDTH, false), 5);
    // There is no card below 4, so down goes to the last card of the next row.
    assert_eq!(shelves.step(4, Action::Down, WIDTH, false), 6);
    assert_eq!(shelves.step(5, Action::Down, WIDTH, false), 5);
}

#[test]
fn up_moves_back_to_continue_playing() {
    let (recent, games) = (entries(2), entries(5));
    let shelves = shelves(&recent, &games);

    assert_eq!(shelves.step(5, Action::Up, WIDTH, false), 2);
    assert_eq!(shelves.step(2, Action::Up, WIDTH, false), 0);
    assert_eq!(shelves.step(4, Action::Up, WIDTH, false), 1);
    assert_eq!(shelves.step(0, Action::Up, WIDTH, false), 0);
}

#[test]
fn a_fresh_press_wraps_around_the_ends() {
    let (recent, games) = (entries(2), entries(5));
    let shelves = shelves(&recent, &games);

    assert_eq!(shelves.step(0, Action::Left, WIDTH, true), 6);
    assert_eq!(shelves.step(6, Action::Right, WIDTH, true), 0);
    // Up from the top row goes to the bottom row, in the same column.
    assert_eq!(shelves.step(0, Action::Up, WIDTH, true), 5);
    assert_eq!(shelves.step(1, Action::Up, WIDTH, true), 6);
    // Down from the bottom row goes to the top row, in the same column.
    assert_eq!(shelves.step(5, Action::Down, WIDTH, true), 0);
    assert_eq!(shelves.step(6, Action::Down, WIDTH, true), 1);
    // Away from the ends, wrapping changes nothing.
    assert_eq!(shelves.step(4, Action::Up, WIDTH, true), 1);
    assert_eq!(shelves.step(4, Action::Down, WIDTH, true), 6);
}

#[test]
fn down_from_continue_playing_wraps_when_all_games_is_empty() {
    // 0 1 2
    // 3 4
    let recent = entries(5);
    let shelves = shelves(&recent, &[]);

    assert_eq!(shelves.step(3, Action::Down, WIDTH, true), 0);
    assert_eq!(shelves.step(4, Action::Down, WIDTH, true), 1);
    // Held down, the selection stays at the bottom.
    assert_eq!(shelves.step(3, Action::Down, WIDTH, false), 3);
    // There is no card under 2 in the bottom row, so down goes to its last card.
    assert_eq!(shelves.step(2, Action::Down, WIDTH, true), 4);
    assert_eq!(shelves.step(2, Action::Down, WIDTH, false), 4);
}

#[test]
fn wrapping_without_continue_playing_finds_the_nearest_card() {
    // 0 1 2
    // 3 4
    let games = entries(5);
    let shelves = shelves(&[], &games);

    // There is no card under 2 in the bottom row, so up goes to its last card.
    assert_eq!(shelves.step(2, Action::Up, WIDTH, true), 4);
    assert_eq!(shelves.step(4, Action::Down, WIDTH, true), 1);
    assert_eq!(shelves.step(3, Action::Down, WIDTH, true), 0);
}

#[test]
fn lower_cards_sit_lower_on_the_page() {
    let (recent, games) = (entries(2), entries(5));
    let shelves = shelves(&recent, &games);

    let (first_top, first_bottom) = shelves.card_bounds(0, WIDTH);
    let (games_top, _) = shelves.card_bounds(2, WIDTH);
    let (second_row_top, _) = shelves.card_bounds(5, WIDTH);

    assert!(first_top < first_bottom);
    assert!(games_top > first_bottom);
    assert!(second_row_top > games_top);
    assert_eq!(shelves.card_bounds(3, WIDTH), shelves.card_bounds(2, WIDTH));
}

#[test]
fn continue_playing_shows_a_row_of_the_widest_window_at_most() {
    assert_eq!(most_recent_shown(), 5);
    assert_eq!(columns(1280.0), most_recent_shown());
}

#[test]
fn down_from_a_full_row_of_five_lands_under_the_same_card() {
    let recent = entries(5);
    let games = entries(5);
    let shelves = shelves(&recent, &games);

    assert_eq!(shelves.step(4, Action::Down, 1280.0, false), 9);
    assert_eq!(shelves.step(9, Action::Up, 1280.0, false), 4);
}

#[test]
fn a_resize_keeps_the_same_game_selected() {
    let games = entries(6);
    let wide = Shelves {
        recent: games[..5].iter().collect(),
        games: games.iter().collect(),
    };
    let narrow = Shelves {
        recent: games[..3].iter().collect(),
        games: games.iter().collect(),
    };

    // A game still in "Continue playing" stays there.
    assert_eq!(narrow.same_card(&wide, 1), Some(1));
    // One that no longer fits moves to the same game on All games.
    assert_eq!(narrow.same_card(&wide, 4), Some(3 + 4));
    // A card on All games stays on the same game there.
    assert_eq!(narrow.same_card(&wide, 5 + 2), Some(3 + 2));
    assert_eq!(wide.same_card(&narrow, 3 + 2), Some(5 + 2));
}
