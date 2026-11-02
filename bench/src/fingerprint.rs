//! §6.1, and nothing else: the Dataset fingerprint is semantic FNV-1a-64.
//!
//! Per game in seed order: the seed as 4 bytes little-endian, then the ten
//! column words as 4 bytes little-endian each, then the sequence letters as one
//! ASCII byte each. Reformatting a file cannot move it; any change to any game
//! must. It is compared for equality and never defended against an adversary,
//! which is why it is 64-bit non-cryptographic and not a `sha2` dependency.
//!
//! Deliberately *not* hashed: the header, the manifest, the piece count, the cap,
//! the seed range, the generator version, and the zero-filled tail of the §4.1
//! sequence buffer. The first group is prose; the tail is whatever the buffer
//! happened to hold.

use crate::dataset::GameLine;

/// FNV-1a-64's offset basis.
pub const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a-64's prime.
pub const PRIME: u64 = 0x0000_0100_0000_01b3;

/// The running hash. One byte at a time: folding a whole `u32` in one step is
/// not FNV-1a and gives a different (wrong) value.
#[derive(Clone, Copy, Debug)]
pub struct Fnv(u64);

impl Fnv {
    pub const fn new() -> Fnv {
        Fnv(OFFSET_BASIS)
    }

    /// `h ^= byte; h *= prime`, wrapping.
    #[inline]
    pub fn byte(&mut self, byte: u8) {
        self.0 ^= byte as u64;
        self.0 = self.0.wrapping_mul(PRIME);
    }

    /// Explicitly little-endian: a shift, not a memory cast, so a big-endian
    /// host cannot change the Dataset's identity.
    #[inline]
    pub fn u32_le(&mut self, word: u32) {
        for byte in word.to_le_bytes() {
            self.byte(byte);
        }
    }

    #[inline]
    pub fn game(&mut self, game: &GameLine) {
        self.u32_le(game.seed);
        for word in &game.board {
            self.u32_le(*word);
        }
        for letter in &game.sequence {
            self.byte(*letter);
        }
    }

    #[inline]
    pub fn finish(self) -> u64 {
        self.0
    }
}

impl Default for Fnv {
    fn default() -> Fnv {
        Fnv::new()
    }
}

/// The fingerprint of a set of games, in the order given (seed order).
pub fn fingerprint(games: &[GameLine]) -> u64 {
    let mut hash = Fnv::new();
    for game in games {
        hash.game(game);
    }
    hash.finish()
}

/// Sixteen lowercase hex digits: the form the manifest stores and an archive
/// file is named after.
pub fn hex(value: u64) -> String {
    format!("{value:016x}")
}

/// Parse the manifest's fingerprint string back to a number (for reporting).
pub fn from_hex(text: &str) -> Option<u64> {
    if text.len() != 16 {
        return None;
    }
    u64::from_str_radix(text, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a64_matches_its_published_vectors() {
        // The canonical FNV-1a-64 test vectors: "" -> offset basis,
        // "a" -> af63dc4c8601ec8c, "foobar" -> 85944171f73967e8.
        assert_eq!(Fnv::new().finish(), OFFSET_BASIS);
        let mut a = Fnv::new();
        a.byte(b'a');
        assert_eq!(hex(a.finish()), "af63dc4c8601ec8c");
        let mut foobar = Fnv::new();
        for byte in b"foobar" {
            foobar.byte(*byte);
        }
        assert_eq!(hex(foobar.finish()), "85944171f73967e8");
    }

    #[test]
    fn the_stream_is_little_endian_and_per_byte() {
        // 0x00000001 as four little-endian bytes is 01 00 00 00.
        let mut little = Fnv::new();
        little.u32_le(1);
        let mut bytewise = Fnv::new();
        for byte in [1u8, 0, 0, 0] {
            bytewise.byte(byte);
        }
        assert_eq!(little.finish(), bytewise.finish());

        // Folding the word in one step is a different (wrong) value.
        let mut wrong = Fnv::new();
        wrong.byte(0); // placeholder so the type is used identically
        let mut one_step = Fnv::new();
        one_step.0 ^= 1;
        one_step.0 = one_step.0.wrapping_mul(PRIME);
        assert_ne!(one_step.finish(), little.finish());
        let _ = wrong;
    }

    #[test]
    fn one_game_hashes_its_seed_words_and_letters() {
        let game = GameLine {
            seed: 1,
            board: [0; 10],
            sequence: b"IJLOSTZ".to_vec(),
        };
        let mut expected = Fnv::new();
        expected.u32_le(1);
        for _ in 0..10 {
            expected.u32_le(0);
        }
        for letter in b"IJLOSTZ" {
            expected.byte(*letter);
        }
        assert_eq!(fingerprint(&[game]), expected.finish());
        assert_eq!(hex(fingerprint(&[])), hex(OFFSET_BASIS));
    }
}
