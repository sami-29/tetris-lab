//! Seam 1, the Generator: the frozen interface of §5.6–5.8.
//!
//! Order matters here. The splitmix64 reference vectors are asserted **before**
//! anything plays a game, because a port that gets the PRNG wrong produces games
//! that look plausible and are not the committed ones.

use engine::board::Board;
use engine::generator::{generate_game, DomainError, Rng, C_FILL, C_SEQUENCE};
use engine::pieces::Piece;

/// `include_str!` rather than a path read: the vectors must not depend on the
/// test runner's working directory.
const VECTORS: &str = include_str!("fixtures/splitmix64.txt");
const GAMES: &str = include_str!("fixtures/games.txt");

fn data_lines(text: &'static str) -> impl Iterator<Item = &'static str> {
    text.lines().filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
}

#[test]
fn splitmix64_matches_the_committed_vectors() {
    // §5.6: state starts at the zero-extended seed with no pre-scramble, and each
    // draw bumps the state before mixing.
    let mut seen = 0;
    for line in data_lines(VECTORS) {
        let f: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(f.len(), 5, "vector line: {line}");
        let seed: u64 = f[0].parse().expect("seed");
        let mut rng = Rng::new(seed);
        for (i, want) in f[1..].iter().enumerate() {
            let want = u64::from_str_radix(want, 16).expect("hex draw");
            assert_eq!(rng.draw(), want, "seed {seed} draw {i}");
        }
        seen += 1;
    }
    assert_eq!(seen, 2, "the fixture must carry seeds 0 and 42");
}

#[test]
fn the_stream_constants_are_normative() {
    // §5.7: the leading 64-bit fractions of pi.
    assert_eq!(C_FILL, 0x243F_6A88_85A3_08D3);
    assert_eq!(C_SEQUENCE, 0x1319_8A2E_0370_7344);
    assert_ne!(C_FILL, C_SEQUENCE);
}

#[test]
fn golden_games_regenerate_bit_for_bit() {
    for line in data_lines(GAMES) {
        let f: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(f.len(), 14, "game line: {line}");
        let seed: u32 = f[0].parse().expect("seed");
        let pieces: u32 = f[1].parse().expect("pieces");
        let cap: u32 = f[2].parse().expect("cap");

        let mut want_board = Board::EMPTY;
        for (c, word) in f[3..13].iter().enumerate() {
            want_board.0[c] = u32::from_str_radix(word, 16).expect("hex column word");
        }
        let want_sequence = f[13].as_bytes();

        let game = generate_game(seed, pieces, cap).expect("in domain");
        assert_eq!(game.seed, seed);
        assert_eq!(game.board, want_board, "board for seed {seed}");
        assert_eq!(
            game.sequence.letters(),
            want_sequence,
            "sequence for seed {seed}"
        );
        assert_eq!(game.sequence.len(), pieces as usize);
    }
}

#[test]
fn raising_the_cap_keeps_the_sequence_and_changes_the_board() {
    // §5.7: the two streams are purpose-separated, so the height cap must not
    // reach the piece stream.
    let low = generate_game(0, 40, 7).expect("in domain");
    let high = generate_game(0, 40, 10).expect("in domain");
    assert_eq!(low.sequence.letters(), high.sequence.letters());
    assert_ne!(low.board, high.board);
}

#[test]
fn a_longer_sequence_extends_the_shorter_one() {
    // §5.7: a 100-piece sequence begins with the 40-piece sequence for the same
    // seed, which is what makes goal 8/40 and goal 12/100 a paired experiment.
    let short = generate_game(42, 40, 7).expect("in domain");
    let long = generate_game(42, 100, 7).expect("in domain");
    assert_eq!(long.sequence.letters()[..40], short.sequence.letters()[..]);
    assert_eq!(long.board, short.board, "the cap alone fixes the board");
}

#[test]
fn the_domain_is_typed_not_a_panic() {
    // §5.8: outside the domain the Generator names no game and never panics.
    for (pieces, cap) in [(0u32, 7u32), (201, 7), (40, 1), (40, 20), (0, 1)] {
        assert_eq!(
            generate_game(0, pieces, cap).unwrap_err(),
            DomainError::OutOfDomain,
            "pieces {pieces} cap {cap}"
        );
    }
    // The edges of the domain are inside it.
    assert!(generate_game(u32::MAX, 1, 2).is_ok());
    assert!(generate_game(0, 200, 19).is_ok());
}

#[test]
fn prefill_stops_before_the_crossing_piece() {
    // §5.7's assertable property: no column is ever taller than the cap, because
    // the piece that would cross it is never placed.
    for cap in [2u32, 4, 7, 10, 19] {
        for seed in 0..120u32 {
            let game = generate_game(seed, 40, cap).expect("in domain");
            let tallest = (0..engine::board::W)
                .map(|c| game.board.height(c))
                .max()
                .expect("ten columns");
            assert!(tallest <= cap, "seed {seed} cap {cap} tallest {tallest}");
        }
    }
}

#[test]
fn a_generated_board_is_legal_by_the_solver_definition() {
    // §5.5: the Generator uses the solver's own drop and clear code, so every
    // generated board must pass the solver's legality test and have no full rows
    // left standing (the pre-fill clears as it goes).
    for seed in 0..120u32 {
        let game = generate_game(seed, 40, 7).expect("in domain");
        assert!(game.board.is_legal(), "seed {seed} produced an illegal board");
        assert_eq!(game.board.full_rows(), 0, "seed {seed} left a full row");
        assert!(game.board.height(0) <= engine::board::H as u32);
    }
}

#[test]
fn the_sequence_is_a_bag_of_seven() {
    // §5.7: 7-bags, Fisher-Yates descending, the final bag possibly truncated.
    let game = generate_game(7, 200, 7).expect("in domain");
    let letters = game.sequence.letters();
    assert_eq!(letters.len(), 200);
    for (i, chunk) in letters.chunks(7).enumerate() {
        if chunk.len() < 7 {
            break; // the truncated final bag
        }
        let mut sorted = chunk.to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, b"IJLOSTZ".to_vec(), "bag {i} is not one of each");
    }
    // No S/Z adjacency across a bag seam: §5.7's rule, wherever it lands.
    for w in letters.windows(2) {
        let (a, b) = (w[0], w[1]);
        if (a == b'S' || a == b'Z') && a == b {
            panic!(
                "S/S or Z/Z adjacency at byte {:?} of the sequence",
                String::from_utf8_lossy(&letters)
            );
        }
    }
}

#[test]
fn every_sequence_letter_names_a_piece() {
    let game = generate_game(99, 100, 7).expect("in domain");
    for piece in game.sequence.as_slice() {
        assert_eq!(Piece::from_letter(piece.letter()), Some(*piece));
        assert!((*piece as u8) < 7, "piece index is the ABI's piece_index");
    }
}
