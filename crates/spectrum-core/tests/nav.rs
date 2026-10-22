//! Grid navigation (`nav::next_index`).

use spectrum_core::nav::{next_index, row_col, Direction};

const D: Direction = Direction::Down;

#[test]
fn moves_within_a_three_column_grid() {
    // 10 items, 3 columns:
    //   0 1 2
    //   3 4 5
    //   6 7 8
    //   9
    assert_eq!(next_index(0, 10, 3, Direction::Right), Some(1));
    assert_eq!(next_index(1, 10, 3, D), Some(4));
    assert_eq!(next_index(4, 10, 3, Direction::Up), Some(1));
    assert_eq!(next_index(4, 10, 3, Direction::Left), Some(3));
    assert_eq!(next_index(2, 10, 3, D), Some(5));
}

#[test]
fn no_wrapping_past_the_edges() {
    assert_eq!(next_index(0, 10, 3, Direction::Left), None);
    assert_eq!(next_index(0, 10, 3, Direction::Up), None);
    assert_eq!(next_index(9, 10, 3, Direction::Right), None);
    assert_eq!(next_index(8, 10, 3, D), None); // 8 is the last full row, Down would be 11
    assert_eq!(next_index(9, 10, 3, D), None);
    assert_eq!(next_index(0, 0, 3, Direction::Right), None);
}

#[test]
fn home_and_end() {
    assert_eq!(next_index(5, 10, 3, Direction::Home), Some(0));
    assert_eq!(next_index(5, 10, 3, Direction::End), Some(9));
    assert_eq!(next_index(0, 1, 3, Direction::End), Some(0));
}

#[test]
fn single_column_list_moves_one_item_at_a_time() {
    assert_eq!(next_index(2, 4, 1, D), Some(3));
    assert_eq!(next_index(2, 4, 1, Direction::Up), Some(1));
    assert_eq!(next_index(2, 4, 0, D), Some(3)); // columns.max(1)
}

#[test]
fn row_col_reports_the_visual_position() {
    assert_eq!(row_col(7, 3), (2, 1));
    assert_eq!(row_col(9, 3), (3, 0));
    assert_eq!(row_col(2, 1), (2, 0));
}
