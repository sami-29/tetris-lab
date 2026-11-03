//! The Generator: the one function turning `(seed, pieces, height_cap)` into a
//! Board and a Sequence (§5.5–5.8, ADR 0003).
//!
//! It is a frozen interface. Every constant, draw order and bound here is
//! contract: change one and every committed game moves, which the Dataset
//! fingerprint gate exists to catch.

use crate::board::Board;
use crate::pieces::{Piece, N_ROTATIONS, PIECES};

/// §5.7: the leading 64-bit fractions of pi, one per purpose.
pub const C_FILL: u64 = 0x243F_6A88_85A3_08D3;
pub const C_SEQUENCE: u64 = 0x1319_8A2E_0370_7344;

/// §5.8's Domain.
pub const MIN_PIECES: u32 = 1;
pub const MAX_PIECES: u32 = 200;
pub const MIN_CAP: u32 = 2;
pub const MAX_CAP: u32 = 19;

/// §5.8: the pre-fill loop is bounded, then reports instead of hanging. The
/// measured worst over 2,000 seeds at cap 7 is 12 placements, so this is ~80x.
pub const MAX_ATTEMPTS: u32 = 1_000;
pub const DRAWS_PER_ATTEMPT: u32 = 3;
pub const MAX_DRAWS: u32 = 3_000;

/// §5.7: the S/Z re-shuffle at the bag seam is bounded, then typed-error.
pub const MAX_SEAM_RESHUFFLES: u32 = 64;

/// How a request can fail without being a bug (§4.2 statuses 1 and 2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DomainError {
    /// Status 1: a parameter outside §5.8.
    OutOfDomain,
    /// Status 2: the pre-fill or the S/Z re-shuffle bound was exhausted.
    BoundExhausted,
}

/// splitmix64 (§5.6). The engine's only randomness, and it is seeded, never
/// sampled: the same seed draws the same numbers in every build.
#[derive(Clone, Copy, Debug)]
pub struct Rng {
    state: u64,
}

impl Rng {
    #[inline]
    pub const fn new(state: u64) -> Rng {
        Rng { state }
    }

    /// The stream: bump, mix, mix, mix.
    #[inline]
    pub fn draw(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Lemire multiply-shift with rejection: unbiased for `1 <= n <= 2^32`.
    pub fn bounded(&mut self, n: u32) -> u32 {
        debug_assert!(n >= 1);
        let n = n as u64;
        let mut x = self.draw();
        let mut product = (x as u128) * (n as u128);
        let mut low = product as u64;
        if low < n {
            // Reject the top of the range that would bias the multiply.
            let threshold = (u64::MAX - n + 1) % n;
            while low < threshold {
                x = self.draw();
                product = (x as u128) * (n as u128);
                low = product as u64;
            }
        }
        (product >> 64) as u32
    }
}

/// One game's piece order: one letter per piece, in the order the beam consumes
/// them. A Sequence is a bag of seven repeated, never a free draw.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Sequence(pub Vec<Piece>);

impl Sequence {
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[inline]
    pub fn as_slice(&self) -> &[Piece] {
        &self.0
    }

    /// The §4.1 sequence buffer's bytes.
    pub fn letters(&self) -> Vec<u8> {
        self.0.iter().map(|p| p.letter()).collect()
    }

    /// From the §4.1 sequence buffer's bytes; a bad byte is an error, not a panic.
    pub fn from_letters(bytes: &[u8]) -> Option<Sequence> {
        let mut pieces = Vec::with_capacity(bytes.len());
        for byte in bytes {
            pieces.push(Piece::from_letter(*byte)?);
        }
        Some(Sequence(pieces))
    }
}

/// One generated game: what the board buffer and the sequence buffer carry.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Game {
    pub seed: u32,
    pub board: Board,
    pub sequence: Sequence,
}

impl Game {
    /// §5.7's assertable property: the crossing piece is never placed, so no
    /// column is taller than the cap.
    pub fn tallest(&self) -> u32 {
        self.board.tallest()
    }
}

/// §5.8's Domain check, as a value.
pub fn in_domain(pieces: u32, height_cap: u32) -> bool {
    (MIN_PIECES..=MAX_PIECES).contains(&pieces) && (MIN_CAP..=MAX_CAP).contains(&height_cap)
}

/// The one entry point: seed, piece count and height cap in, a game out.
pub fn generate_game(seed: u32, pieces: u32, height_cap: u32) -> Result<Game, DomainError> {
    if !in_domain(pieces, height_cap) {
        return Err(DomainError::OutOfDomain);
    }
    let board = pre_fill(seed, height_cap)?;
    let sequence = bag_sequence(seed, pieces)?;
    Ok(Game {
        seed,
        board,
        sequence,
    })
}

/// §5.7's pre-fill: three draws per attempt, in a fixed order, stopping *before*
/// the piece that would rest above the cap. Line clears are part of the rule:
/// skipping them silently produces a different Dataset (~0.3% of games at cap 7).
fn pre_fill(seed: u32, height_cap: u32) -> Result<Board, DomainError> {
    // The fill stream is the seed and nothing else: not the cap, not the pieces.
    let mut rng = Rng::new(seed as u64 ^ C_FILL);
    let mut board = Board::EMPTY;
    let mut attempts = 0u32;
    let mut draws = 0u32;
    loop {
        if attempts >= MAX_ATTEMPTS || draws + DRAWS_PER_ATTEMPT > MAX_DRAWS {
            return Err(DomainError::BoundExhausted);
        }
        attempts += 1;
        draws += DRAWS_PER_ATTEMPT;

        let piece = PIECES[rng.bounded(7) as usize];
        let rotations = piece.rotations();
        let rotation = &rotations[rng.bounded(rotations.len() as u32) as usize];
        let column = rng.bounded(crate::board::W as u32 - rotation.width as u32 + 1) as usize;

        let Some(shift) = board.rest_shift(rotation, column) else {
            continue; // cannot enter at row 0 or does not fit: try again
        };
        // Row the piece's top cell comes to rest at, counted from the top.
        let landing = crate::board::H as u32 - shift - rotation.height as u32;
        if crate::board::H as u32 - landing > height_cap {
            break; // the crossing piece never lands
        }
        board.place(rotation, column, shift);
        board.clear_full_rows();
    }
    Ok(board)
}

/// §5.7's Sequence: 7-bags, Fisher-Yates descending, the S/Z re-shuffle at the
/// seam, the final bag possibly truncated.
fn bag_sequence(seed: u32, pieces: u32) -> Result<Sequence, DomainError> {
    let mut rng = Rng::new(seed as u64 ^ C_SEQUENCE);
    let mut out: Vec<Piece> = Vec::with_capacity(pieces as usize);
    let mut previous_last: Option<Piece> = None;
    while out.len() < pieces as usize {
        let mut bag = PIECES;
        let mut reshuffles = 0u32;
        loop {
            shuffle(&mut bag, &mut rng);
            let seam_is_clean = match previous_last {
                // A bag holds one of each piece, so the shipped rule's test
                // ("bag[i] == bag[i+1]" for S or Z) is unsatisfiable inside a
                // bag; the adjacency it exists to prevent lands at the seam.
                Some(last) => !(bag[0] == last && matches!(last, Piece::S | Piece::Z)),
                None => true,
            };
            if seam_is_clean || reshuffles >= MAX_SEAM_RESHUFFLES {
                break;
            }
            reshuffles += 1;
        }
        previous_last = Some(bag[6]);
        for piece in bag {
            if out.len() < pieces as usize {
                out.push(piece);
            }
        }
    }
    Ok(Sequence(out))
}

fn shuffle(bag: &mut [Piece; 7], rng: &mut Rng) {
    let mut i = 6;
    while i >= 1 {
        let j = rng.bounded(i as u32 + 1) as usize;
        bag.swap(i, j);
        i -= 1;
    }
}

/// Rotation count of a `piece_index`, for callers holding an index rather than a
/// piece (the ABI's move encoding does).
#[inline]
pub fn rotations_of_index(piece_index: u8) -> usize {
    N_ROTATIONS.get(piece_index as usize).copied().unwrap_or(0)
}
