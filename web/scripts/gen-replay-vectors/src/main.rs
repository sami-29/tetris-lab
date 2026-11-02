//! The golden fixture for the browser's playback replay: `web/test/fixtures/replay-vectors.json`.
//!
//! ```
//! cd web/scripts/gen-replay-vectors && cargo run --release   # writes the fixture
//! ```
//!
//! The page cannot ask wasm for "the board after move k" — §4 has no export that
//! returns one, and `solve` leaves the board buffer untouched — so the browser
//! replays `(starting Board, Move list)` itself (`web/src/ui/timeline.ts`). That
//! makes the page a second implementation of `rest_shift` / `place` /
//! `clear_full_rows`, and a second implementation silently drifts.
//!
//! This binary is the leash's other end: it links **the engine crate itself** (a
//! path dependency, so it is the same code the wasm artifact and `cargo test`
//! use — not a transcription of it), replays each fixture through
//! `Board::rest_shift` / `Board::place` / `Board::clear_full_rows`, and writes the
//! ten board words after *every* Move. `web/test/timeline.test.mjs` compares the
//! JavaScript replay against those words one at a time.
//!
//! The fixture is generated, never hand-edited. If the engine's arithmetic
//! changes, regenerate it — and expect the JS mirror to need the same change.

use std::fmt::Write as _;
use std::fs;

use engine::{generate_game, solve, shipped, Board, Piece, SolveConfig};

const W: usize = 10;
const H: usize = 20;

/// The four-row clear, one-row clear, mid-stack clear and non-contiguous clear
/// fixtures, plus three real solved games. Each entry is `(name, board words,
/// moves)`; the moves are `(piece_index, rotation, column)` as §4.1.1 words.
fn built_cases() -> Vec<(&'static str, [u32; W], Vec<(u8, u8, u8)>)> {
    vec![
        // An empty Board and two pieces that do not clear anything: the drop is
        // the whole event, including an O on the floor.
        (
            "empty-no-clear",
            [0; W],
            vec![(3, 0, 4), (5, 0, 6)],
        ),
        // Rows 16..19 are full once a vertical I lands in the one empty column:
        // the common four-row clear, and the heaviest collapse there is.
        (
            "four-row",
            [15, 15, 15, 15, 15, 15, 15, 15, 15, 0],
            vec![(0, 1, 9)],
        ),
        // A single row, completed at the bottom.
        (
            "one-row",
            [1, 1, 1, 1, 1, 1, 1, 1, 1, 0],
            vec![(0, 1, 9)],
        ),
        // One row cleared with a stack standing *above* it: the surviving columns
        // must fall one row, which is the case a flash cannot draw.
        (
            "stack-above",
            [17, 17, 17, 17, 17, 17, 17, 17, 17, 0],
            vec![(0, 1, 9)],
        ),
        // Two full rows with empty cells *below* them: the rows under a clear must
        // not move.
        (
            "mid-stack",
            [13, 13, 13, 13, 13, 12, 12, 12, 12, 0],
            vec![(0, 1, 9)],
        ),
        // The case a single falling band cannot draw: the piece completes rows 14
        // and 17 while rows 15 and 16 stay unfilled. Three surviving segments fall
        // 2, 1 and 0 rows.
        (
            "non-contiguous",
            [36, 36, 36, 36, 36, 36, 36, 36, 36, 3],
            vec![(0, 1, 9)],
        ),
    ]
}

/// Replay a Move list on the engine's own Board, recording every intermediate.
struct Replayed {
    /// The board words after each Move.
    frames: Vec<[u32; W]>,
    /// Rows that were full after the placement, top row first.
    cleared: Vec<Vec<usize>>,
    /// Cumulative Lines cleared.
    lines: Vec<u32>,
    /// The resting shift the engine used.
    shifts: Vec<u32>,
    /// How far the piece fell, from §5.3's spawn row.
    drops: Vec<u32>,
}

fn replay(board0: &Board, moves: &[(u8, u8, u8)]) -> Replayed {
    let mut board = *board0;
    let mut out = Replayed {
        frames: Vec::new(),
        cleared: Vec::new(),
        lines: Vec::new(),
        shifts: Vec::new(),
        drops: Vec::new(),
    };
    let mut total = 0;
    for (index, &(piece_index, rotation, column)) in moves.iter().enumerate() {
        let piece = Piece::from_index(piece_index)
            .unwrap_or_else(|| panic!("move {index}: piece_index {piece_index} is not a piece"));
        let rot = piece
            .rotations()
            .get(rotation as usize)
            .unwrap_or_else(|| panic!("move {index}: {piece:?} has no rotation {rotation}"));
        let shift = board
            .rest_shift(rot, column as usize)
            .unwrap_or_else(|| panic!("move {index}: not spawn-legal at column {column}"));
        board.place(rot, column as usize, shift);
        let full = board.full_rows();
        let rows: Vec<usize> = (0..H)
            .filter(|row| full & (1 << (H - 1 - row)) != 0)
            .collect();
        let cleared = board.clear_full_rows();
        assert_eq!(cleared as usize, rows.len(), "move {index}: clear count");
        total += cleared;
        out.frames.push(board.0);
        out.cleared.push(rows);
        out.lines.push(total);
        out.shifts.push(shift);
        out.drops.push(H as u32 - rot.height as u32 - shift);
    }
    out
}

fn words(values: &[u32]) -> String {
    let mut out = String::from("[");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let _ = write!(out, "{value}");
    }
    out.push(']');
    out
}

fn rows(values: &[usize]) -> String {
    let mut out = String::from("[");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let _ = write!(out, "{value}");
    }
    out.push(']');
    out
}

fn moves_json(moves: &[(u8, u8, u8)]) -> String {
    let mut out = String::from("[");
    for (index, (piece, rotation, column)) in moves.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let _ = write!(out, "[{piece},{rotation},{column}]");
    }
    out.push(']');
    out
}

/// One fixture case, in the shape `web/test/timeline.test.mjs` reads.
fn case_json(
    name: &str,
    seed: Option<u32>,
    sequence: Option<String>,
    board0: &Board,
    moves: &[(u8, u8, u8)],
    replayed: &Replayed,
) -> String {
    let mut out = String::new();
    out.push_str("  {\n");
    let _ = writeln!(out, "    \"name\": \"{name}\",");
    match seed {
        Some(seed) => {
            let _ = writeln!(out, "    \"seed\": {seed},");
        }
        None => out.push_str("    \"seed\": null,\n"),
    }
    if let Some(sequence) = sequence {
        let _ = writeln!(out, "    \"sequence\": \"{sequence}\",");
    } else {
        out.push_str("    \"sequence\": null,\n");
    }
    let _ = writeln!(out, "    \"board0\": {},", words(&board0.0));
    let _ = writeln!(out, "    \"moves\": {},", moves_json(moves));
    let _ = writeln!(out, "    \"shifts\": {},", words(&replayed.shifts));
    let _ = writeln!(out, "    \"drops\": {},", words(&replayed.drops));
    let cleared = replayed
        .cleared
        .iter()
        .map(|set| rows(set))
        .collect::<Vec<_>>()
        .join(",");
    let _ = writeln!(out, "    \"cleared\": [{cleared}],");
    let _ = writeln!(out, "    \"lines\": {},", words(&replayed.lines));
    let frames = replayed
        .frames
        .iter()
        .map(|frame| words(frame))
        .collect::<Vec<_>>()
        .join(",");
    let _ = writeln!(out, "    \"frames\": [{frames}]");
    out.push_str("  }");
    out
}

fn main() {
    let mut cases: Vec<String> = Vec::new();

    for (name, board0, moves) in built_cases() {
        let board = Board(board0);
        let replayed = replay(&board, &moves);
        cases.push(case_json(name, None, None, &board, &moves, &replayed));
    }

    // Real games: the default configuration the page ships with, at the beam width
    // the page solves at. The Move list is the engine's own, so the fixture is not
    // a synthetic arithmetic test only.
    for (seed, goal, pieces, cap, beam) in [(0u32, 8u32, 40u32, 7u32, 25u32), (42, 8, 40, 7, 25), (7, 8, 40, 7, 25)] {
        let game = generate_game(seed, pieces, cap).expect("the fixture's game is in the Domain");
        let result = solve(
            &game.board,
            game.sequence.as_slice(),
            &SolveConfig::new(goal, beam),
            shipped(),
        );
        assert_eq!(result.status, engine::Status::Ok, "seed {seed} solves");
        assert!(result.won, "seed {seed} wins at beam {beam}");
        let moves: Vec<(u8, u8, u8)> = result
            .moves
            .iter()
            .map(|m| (m.piece_index, m.rotation, m.column))
            .collect();
        let replayed = replay(&game.board, &moves);
        // The replay is the search's own board evolution: if this disagrees, the
        // fixture would be certifying the wrong thing.
        assert_eq!(
            *replayed.lines.last().unwrap_or(&0),
            result.lines,
            "seed {seed}: the replay's Lines are the Solve's"
        );
        for (index, &(piece_index, _, _)) in moves.iter().enumerate() {
            assert_eq!(
                piece_index,
                game.sequence.0[index].index(),
                "seed {seed}: move {index} names the Sequence's piece"
            );
        }
        let sequence = String::from_utf8(game.sequence.letters()).expect("ASCII letters");
        cases.push(case_json(
            &format!("game-seed{seed}"),
            Some(seed),
            Some(sequence),
            &game.board,
            &moves,
            &replayed,
        ));
    }

    let mut out = String::new();
    out.push_str("{\n");
    out.push_str("  \"generator\": \"web/scripts/gen-replay-vectors (engine crate, path dependency)\",\n");
    out.push_str(
        "  \"note\": \"Generated, never hand-edited. `frames[k]` is the ten board words after Move k+1,\\n  as engine::Board computes them; `cleared[k]` are the full rows that Move completed, top row\\n  first; `shifts[k]` is the resting shift and `drops[k]` the number of rows the piece fell.\",\n",
    );
    let _ = writeln!(out, "  \"cases\": [");
    out.push_str(&cases.join(",\n"));
    out.push_str("\n  ]\n}\n");

    let target = concat!(env!("CARGO_MANIFEST_DIR"), "/../../test/fixtures/replay-vectors.json");
    let path = std::path::Path::new(target);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("the fixtures directory is writable");
    }
    fs::write(path, out).expect("the fixture is writable");
    println!("wrote {} ({} cases)", path.display(), cases.len());
}
