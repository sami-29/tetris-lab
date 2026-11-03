//! One game as an ASCII board on stdout -- the `--pretty` the spec mentions
//! (§6). It lives here, not on `export`, so that no exporter flag can produce a
//! non-canonical `games.txt`.

use crate::dataset::GameLine;
use engine::board::{Board, H, W};

/// The board, top row first, with the seed and the sequence underneath so a
/// reported game can be pasted into a ticket.
pub fn ascii(game: &GameLine) -> String {
    let board = Board(game.board);
    let mut out = String::with_capacity(21 * (W + 3) + game.sequence.len() + 32);
    out.push_str(&format!(
        "seed {}  tallest {}  letters {}\n",
        game.seed,
        board.tallest(),
        String::from_utf8_lossy(&game.sequence)
    ));
    out.push('+');
    for _ in 0..W {
        out.push('-');
    }
    out.push_str("+\n");
    for row in 0..H {
        out.push('|');
        for column in 0..W {
            out.push(if board.filled(column, row) { '#' } else { '.' });
        }
        out.push_str("|\n");
    }
    out.push('+');
    for _ in 0..W {
        out.push('-');
    }
    out.push_str("+\n");
    out
}
