//! Shared test machinery: an independent replay simulator and the fixture
//! reader. Both are deliberately written without the engine's bitboard code, so
//! a Move list the engine returns is checked against plain gravity rather than
//! against itself (§9.2 gate 3).
//!
//! A few test crates share this module and each uses a subset, so dead code is
//! expected rather than suspected.
#![allow(dead_code)]

use engine::board::{Board, H, W};
use engine::pieces::{Piece, Rotation};

/// Data lines of a committed fixture: comments and blank lines are not data.
pub fn data_lines(text: &'static str) -> impl Iterator<Item = &'static str> {
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
}

pub struct Sim {
    cells: [[bool; H]; W],
}

impl Sim {
    pub fn from_board(board: &Board) -> Sim {
        let mut cells = [[false; H]; W];
        for (c, word) in board.0.iter().enumerate() {
            for (bit, cell) in cells[c].iter_mut().enumerate() {
                *cell = word & (1 << bit) != 0;
            }
        }
        Sim { cells }
    }

    /// Drop `rot` down column `column` under gravity; place it and clear rows.
    /// Returns the number of lines cleared, or `None` if the placement is
    /// illegal at spawn or overlaps anything.
    pub fn play(&mut self, piece: Piece, rotation: u8, column: u8) -> Option<u32> {
        let rots = piece.rotations();
        let rot: &Rotation = rots.get(rotation as usize)?;
        let (w, h) = (rot.width as usize, rot.height as usize);
        let col = column as usize;
        if col + w > W {
            return None;
        }
        // Cell offsets from the piece's lowest occupied bit: `Rotation.cols` is a
        // bit per row with bit 0 at the box's *bottom* row, so box row `r` is
        // offset `h - 1 - r` from that bit.
        let mut cells = Vec::new();
        for dc in 0..w {
            for row in 0..h {
                if (rot.cols[dc] >> (h - 1 - row)) & 1 == 1 {
                    cells.push((dc, h - 1 - row));
                }
            }
        }
        if cells.is_empty() {
            return None;
        }
        // Gravity: enter at the top row and fall while the position is free, so
        // the landing is the lowest base reached with every base above it free.
        // A piece that cannot enter at the top row has no legal placement.
        let mut landing: Option<usize> = None;
        for base in (0..=(H - h)).rev() {
            let fits = cells
                .iter()
                .all(|(dc, offset)| !self.cells[col + dc][base + offset]);
            if !fits {
                break;
            }
            landing = Some(base);
        }
        let base = landing?;
        for (dc, offset) in &cells {
            self.cells[col + dc][base + offset] = true;
        }

        // Clear full rows bottom-up. `bit` is a bit index, so clearing the row at
        // `bit` shifts everything above it down one place; the row that falls into
        // `bit` has to be re-checked, hence `bit += 1`.
        let mut cleared = 0u32;
        let mut bit = H;
        while bit > 0 {
            bit -= 1;
            if (0..W).all(|c| self.cells[c][bit]) {
                for c in 0..W {
                    for k in bit..(H - 1) {
                        self.cells[c][k] = self.cells[c][k + 1];
                    }
                    self.cells[c][H - 1] = false;
                }
                cleared += 1;
                bit += 1;
            }
        }
        Some(cleared)
    }

    pub fn as_board(&self) -> Board {
        let mut board = Board::EMPTY;
        for c in 0..W {
            for r in 0..H {
                if self.cells[c][r] {
                    board.0[c] |= 1 << r;
                }
            }
        }
        board
    }
}

/// Replay a Move list on the independent simulator and return the Lines it
/// cleared. Panics with the offending Move when a placement is not legal, which
/// is §9.2 gate 3's "no overlap and a legal board".
pub fn replay(board: &Board, sequence: &[Piece], moves: &[engine::search::Move]) -> u32 {
    let mut sim = Sim::from_board(board);
    let mut lines = 0;
    for (i, mv) in moves.iter().enumerate() {
        let piece = sequence[i];
        assert_eq!(mv.piece_index, piece.index(), "move {i} names its piece");
        let before = sim.as_board();
        let cleared = sim.play(piece, mv.rotation, mv.column).unwrap_or_else(|| {
            panic!(
                "move {i} ({mv:?}) is not legal; board {:?}",
                before.0
            )
        });
        lines += cleared;
    }
    assert!(sim.as_board().is_legal(), "the replayed board stays legal");
    lines
}
