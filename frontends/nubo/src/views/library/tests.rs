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
    assert_eq!(columns(FULL_ROW_WINDOW_WIDTH), RECENT_LIMIT);
    // The cards are at their full size.
    let full_height = CARD_MAX_WIDTH * CARD_ASPECT.1 / CARD_ASPECT.0;
    assert!((card_height(FULL_ROW_WINDOW_WIDTH) - full_height).abs() < 0.01);
}

#[test]
fn left_and_right_move_one_card_and_stop_at_the_ends() {
    let (recent, games) = (entries(2), entries(5));
    let shelves = shelves(&recent, &games);

    assert_eq!(shelves.step(0, Action::Left, WIDTH), 0);
    assert_eq!(shelves.step(1, Action::Right, WIDTH), 2);
    assert_eq!(shelves.step(6, Action::Right, WIDTH), 6);
}

#[test]
fn down_moves_from_continue_playing_to_all_games() {
    let (recent, games) = (entries(2), entries(5));
    let shelves = shelves(&recent, &games);

    assert_eq!(shelves.step(0, Action::Down, WIDTH), 2);
    assert_eq!(shelves.step(1, Action::Down, WIDTH), 3);
    assert_eq!(shelves.step(2, Action::Down, WIDTH), 5);
    // There is no card below 4, so down goes to the last card of the next row.
    assert_eq!(shelves.step(4, Action::Down, WIDTH), 6);
    assert_eq!(shelves.step(5, Action::Down, WIDTH), 5);
}

#[test]
fn up_moves_back_to_continue_playing() {
    let (recent, games) = (entries(2), entries(5));
    let shelves = shelves(&recent, &games);

    assert_eq!(shelves.step(5, Action::Up, WIDTH), 2);
    assert_eq!(shelves.step(2, Action::Up, WIDTH), 0);
    assert_eq!(shelves.step(4, Action::Up, WIDTH), 1);
    assert_eq!(shelves.step(0, Action::Up, WIDTH), 0);
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
