//! §9.2 gate 3's independent simulator: a Move list, a Board, and nothing from
//! the Search.
//!
//! Deliberately *not* `Board::rest_shift`, `Board::place` or
//! `Board::clear_full_rows`. This module keeps the field as a 20 × 10 grid of
//! `bool`s, drops each piece one row at a time under gravity, and clears a row by
//! rebuilding the grid without it. If the two implementations agree, the claim
//! "every returned Move list replays to the claimed Lines" is a check rather than
//! a restatement: a bug in the beam's placement arithmetic that also happened to
//! be in the board code would still be caught here, because the arithmetic is not
//! shared.
//!
//! What *is* shared, and has to be: the §5.2 rotation table (`Piece::rotations`)
//! and §5.3's game-over rule (a piece that cannot enter at row 0 ends the game).
//! Those are contract data, not an implementation, and a second transcription of
//! the tetrominoes would test the transcription rather than the Search.

use engine::{Board, Move, Piece};

/// The field's width, in cells.
pub const W: usize = 10;
/// The field's height, in cells. Row 0 is the top row (§5.1).
pub const H: usize = 20;

/// What a replay actually did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    /// Move words consumed: equal to the list's length when the replay succeeded.
    pub moves: usize,
    /// Lines cleared by those placements, counted by this simulator's own row
    /// clears.
    pub lines: u32,
    /// Full rows left on the field afterwards. Must be zero: every completed row
    /// is cleared as it completes.
    pub full_rows: u32,
}

/// Why a Move list does not replay. Every variant names the move (1-based, as a
/// human counts them) so the gate's failure points at one placement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The list is longer than the Sequence: the game cannot have that many
    /// placements.
    TooManyMoves { moves: usize, pieces: usize },
    /// `piece_index` is not the piece the Sequence holds at this position.
    PieceIndex { at: usize, said: u8, wanted: u8 },
    /// A rotation the piece does not have.
    Rotation {
        at: usize,
        said: u8,
        rotations: usize,
    },
    /// The placement's columns leave the field.
    OffField { at: usize, column: u8, width: u8 },
    /// No legal position at all: the piece overlaps the stack even at row 0, so
    /// the game was already over and this placement cannot exist.
    Blocked { at: usize },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::TooManyMoves { moves, pieces } => write!(
                f,
                "move list has {moves} moves, more moves than pieces ({pieces})"
            ),
            Error::PieceIndex { at, said, wanted } => write!(
                f,
                "move {at}: piece_index {said} is not the piece the Sequence holds at that position ({wanted})"
            ),
            Error::Rotation {
                at,
                said,
                rotations,
            } => write!(
                f,
                "move {at}: rotation {said} does not exist ({rotations} rotations)"
            ),
            Error::OffField {
                at,
                column,
                width,
            } => write!(
                f,
                "move {at}: column {column} with rotation width {width} leaves the field"
            ),
            Error::Blocked { at } => write!(
                f,
                "move {at}: the piece cannot enter at row 0 (spawn blocked), so the position was already lost"
            ),
        }
    }
}

impl std::error::Error for Error {}

/// Replay `moves` from `board`, consuming `sequence` in order, and report the
/// Lines this simulator cleared.
pub fn replay(board: &Board, sequence: &[Piece], moves: &[Move]) -> Result<Report, Error> {
    let mut grid = [[false; W]; H];
    for (row, cells) in grid.iter_mut().enumerate() {
        for (column, cell) in cells.iter_mut().enumerate() {
            *cell = board.filled(column, row);
        }
    }
    if moves.len() > sequence.len() {
        return Err(Error::TooManyMoves {
            moves: moves.len(),
            pieces: sequence.len(),
        });
    }

    let mut lines = 0u32;
    for (index, mv) in moves.iter().enumerate() {
        let at = index + 1;
        let wanted = sequence[index];
        if mv.piece_index != wanted.index() {
            return Err(Error::PieceIndex {
                at,
                said: mv.piece_index,
                wanted: wanted.index(),
            });
        }
        let piece = Piece::from_index(mv.piece_index).expect("piece_index was just compared");
        let rotations = piece.rotations();
        let rotation = rotations.get(mv.rotation as usize).ok_or(Error::Rotation {
            at,
            said: mv.rotation,
            rotations: rotations.len(),
        })?;
        let width = rotation.width as usize;
        let height = rotation.height as usize;
        let column = mv.column as usize;
        if column + width > W {
            return Err(Error::OffField {
                at,
                column: mv.column,
                width: rotation.width,
            });
        }
        if height > H {
            return Err(Error::Blocked { at });
        }

        // The piece's cells, in box coordinates with y = 0 the box's top row.
        let mut cells: Vec<(usize, usize)> = Vec::with_capacity(4);
        for x in 0..width {
            for y in 0..height {
                if rotation.cols[x] & (1 << (height - 1 - y)) != 0 {
                    cells.push((x, y));
                }
            }
        }

        // Gravity, one row at a time from the highest legal entry (the box's top
        // row at row 0) down. A piece that overlaps at the entry has nowhere to
        // come in: that is §5.3's game over, and a Move list cannot contain it.
        let mut shift = H - height;
        if !clear(&grid, &cells, column, shift, height) {
            return Err(Error::Blocked { at });
        }
        while shift > 0 && clear(&grid, &cells, column, shift - 1, height) {
            shift -= 1;
        }
        for (x, y) in &cells {
            grid[H - shift - height + y][column + x] = true;
        }
        lines += clear_full_rows(&mut grid);
    }

    let mut full_rows = 0u32;
    for row in &grid {
        if row.iter().all(|filled| *filled) {
            full_rows += 1;
        }
    }

    Ok(Report {
        moves: moves.len(),
        lines,
        full_rows,
    })
}

/// Whether the piece rests at `shift` without overlapping a filled cell. `shift`
/// is how far the piece's bottom row is above the floor, so the box's top row
/// lands on row `H - shift - height`.
fn clear(
    grid: &[[bool; W]; H],
    cells: &[(usize, usize)],
    column: usize,
    shift: usize,
    height: usize,
) -> bool {
    for (x, y) in cells {
        if grid[H - shift - height + y][column + x] {
            return false;
        }
    }
    true
}

/// Clear every complete row, keeping the surviving rows in order with the empty
/// space above them. Returns how many were cleared.
fn clear_full_rows(grid: &mut [[bool; W]; H]) -> u32 {
    let mut kept: Vec<[bool; W]> = Vec::with_capacity(H);
    let mut cleared = 0u32;
    for row in grid.iter() {
        if row.iter().all(|filled| *filled) {
            cleared += 1;
        } else {
            kept.push(*row);
        }
    }
    if cleared == 0 {
        return 0;
    }
    let mut fresh = [[false; W]; H];
    let offset = H - kept.len();
    for (index, row) in kept.iter().enumerate() {
        fresh[offset + index] = *row;
    }
    *grid = fresh;
    cleared
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::Piece;

    fn sequence(letters: &str) -> Vec<Piece> {
        engine::Sequence::from_letters(letters.as_bytes())
            .expect("test sequences are made of piece letters")
            .0
    }

    fn mv(piece_index: u8, rotation: u8, column: u8) -> Move {
        Move {
            piece_index,
            rotation,
            column,
        }
    }

    /// Five O pieces fill the two bottom rows and clear both: the classic check
    /// that the clear is not a no-op and that the count is per row.
    #[test]
    fn a_completed_row_is_cleared_and_the_lines_are_counted() {
        let pieces = sequence("OOOOO");
        let moves = vec![
            mv(3, 0, 0),
            mv(3, 0, 2),
            mv(3, 0, 4),
            mv(3, 0, 6),
            mv(3, 0, 8),
        ];
        let report = replay(&Board::EMPTY, &pieces, &moves).unwrap();
        assert_eq!(report.lines, 2, "five O's fill two rows of ten columns");
        assert_eq!(report.full_rows, 0);
        assert_eq!(report.moves, 5);
    }

    /// A piece that has nowhere to enter is refused, not placed on top of the
    /// stack: §5.3's spawn-blocked game over.
    #[test]
    fn a_blocked_piece_is_an_error() {
        let mut board = Board::EMPTY;
        for column in 0..W {
            for row in 0..H {
                if row != 0 {
                    board.set(column, row);
                }
            }
        }
        // The top row is empty, so an I piece laid flat fits; an I standing up
        // does not.
        let pieces = sequence("II");
        let flat = replay(&board, &pieces, &[mv(0, 0, 0)]).unwrap();
        assert_eq!(flat.moves, 1);
        let error = replay(&board, &pieces, &[mv(0, 1, 0)]).unwrap_err();
        assert_eq!(error, Error::Blocked { at: 1 });
        assert!(error.to_string().contains("move 1"));
    }

    #[test]
    fn the_sequence_position_is_part_of_the_contract() {
        let pieces = sequence("IT");
        let error = replay(&Board::EMPTY, &pieces, &[mv(5, 0, 0)]).unwrap_err();
        assert_eq!(
            error,
            Error::PieceIndex {
                at: 1,
                said: 5,
                wanted: 0
            }
        );
    }

    #[test]
    fn an_empty_list_replays_to_nothing() {
        let report = replay(&Board::EMPTY, &sequence("I"), &[]).unwrap();
        assert_eq!(report.lines, 0);
        assert_eq!(report.moves, 0);
        assert_eq!(report.full_rows, 0);
    }

    #[test]
    fn a_rotation_out_of_range_is_named() {
        let pieces = sequence("O");
        let error = replay(&Board::EMPTY, &pieces, &[mv(3, 1, 0)]).unwrap_err();
        assert_eq!(
            error,
            Error::Rotation {
                at: 1,
                said: 1,
                rotations: 1
            }
        );
    }
}
