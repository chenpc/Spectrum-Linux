//! Grid/list navigation. Kept as a pure index function so it can be tested without a display:
//! GTK4 has no in-process key injection, so the key controller is only a thin wrapper around this.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
}

impl Direction {
    /// Command-registry spelling (`{"direction": "down"}`).
    pub fn parse(name: &str) -> Option<Direction> {
        Some(match name {
            "up" => Direction::Up,
            "down" => Direction::Down,
            "left" => Direction::Left,
            "right" => Direction::Right,
            "home" => Direction::Home,
            "end" => Direction::End,
            _ => return None,
        })
    }

    pub fn arg(self) -> &'static str {
        match self {
            Direction::Up => "up",
            Direction::Down => "down",
            Direction::Left => "left",
            Direction::Right => "right",
            Direction::Home => "home",
            Direction::End => "end",
        }
    }
}

/// Next display position, or None when the move would leave the model.
///
/// `columns` is the number of columns the grid is actually using (1 for a single-column list).
/// Left/Right move one item; Up/Down move one row. No wrapping: moving past the first/last item
/// returns None so the selection stays where it is.
pub fn next_index(current: usize, n: usize, columns: usize, dir: Direction) -> Option<usize> {
    if n == 0 {
        return None;
    }
    let columns = columns.max(1);
    let next = match dir {
        Direction::Home => 0,
        Direction::End => n - 1,
        Direction::Left => current.checked_sub(1)?,
        Direction::Right => current + 1,
        Direction::Up => current.checked_sub(columns)?,
        Direction::Down => current + columns,
    };
    if next < n {
        Some(next)
    } else {
        None
    }
}

/// Row/column of a position, for the status line.
pub fn row_col(pos: usize, columns: usize) -> (usize, usize) {
    let columns = columns.max(1);
    (pos / columns, pos % columns)
}
