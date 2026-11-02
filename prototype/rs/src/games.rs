//! Parser for the games file written by `prototype/py/baseline.py`.
//!
//! Format, per game: a `seed N` line, 20 board rows of 10 chars (row 0 = top,
//! '1' = filled), then one sequence line of piece letters.

use crate::kernel::{Game, Word, BOARD_H, BOARD_W};
use crate::pieces::piece_index;

pub fn parse<W: Word>(text: &str) -> Vec<Game<W>> {
    let mut out = Vec::new();
    let mut lines = text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty());

    while let Some(seed_line) = lines.next() {
        let seed: u32 = seed_line
            .trim()
            .trim_start_matches("seed")
            .trim()
            .parse()
            .expect("seed line");

        let mut board = [W::zero(); BOARD_W];
        for r in 0..BOARD_H as usize {
            let row = lines.next().expect("board row");
            for (c, ch) in row.bytes().enumerate() {
                if ch == b'1' {
                    // Bit 0 is the bottom row; row 0 in the file is the top.
                    board[c] = W::or(board[c], W::one().shl((BOARD_H as u32 - 1) - r as u32));
                }
            }
        }

        let seq_line = lines.next().expect("sequence line");
        let seq: Vec<u8> = seq_line.bytes().map(piece_index).collect();
        out.push(Game { seed, board, seq });
    }
    out
}
