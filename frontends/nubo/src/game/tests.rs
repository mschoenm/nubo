use super::moved_selection;

#[test]
fn moves_within_the_rows() {
    assert_eq!(moved_selection(1, 1, 4, true), 2);
    assert_eq!(moved_selection(2, -1, 4, false), 1);
}

#[test]
fn wraps_around_the_ends() {
    assert_eq!(moved_selection(0, -1, 4, true), 3);
    assert_eq!(moved_selection(3, 1, 4, true), 0);
}

#[test]
fn stops_at_the_ends_without_wrapping() {
    assert_eq!(moved_selection(0, -1, 4, false), 0);
    assert_eq!(moved_selection(3, 1, 4, false), 3);
}

#[test]
fn an_empty_page_stays_on_the_first_row() {
    assert_eq!(moved_selection(0, 1, 0, true), 0);
    assert_eq!(moved_selection(0, -1, 0, false), 0);
}

#[test]
fn a_selection_past_the_end_comes_back_to_the_rows() {
    assert_eq!(moved_selection(7, 1, 4, false), 3);
    assert_eq!(moved_selection(7, 1, 4, true), 0);
    assert_eq!(moved_selection(7, -1, 4, true), 3);
}
