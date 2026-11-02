//! Ticket 06 spike — bitboard beam-search kernel, native bench binary and wasm
//! module. Throwaway code: not wired into the site, not production quality.

pub mod games;
pub mod kernel;
pub mod pieces;

#[cfg(all(target_arch = "wasm32", feature = "simd"))]
pub mod simd;

#[cfg(target_arch = "wasm32")]
mod wasm_api;

#[cfg(all(feature = "word32", not(feature = "word64")))]
pub type W = u32;
#[cfg(all(feature = "word64", not(feature = "word32")))]
pub type W = u64;

/// Human-readable kernel variant, e.g. `u32+simd`.
pub fn variant() -> String {
    let mut s = String::new();
    if cfg!(feature = "word32") {
        s.push_str("u32");
    } else {
        s.push_str("u64");
    }
    if cfg!(feature = "simd") {
        s.push_str("+simd");
    }
    if cfg!(feature = "lines_live") {
        s.push_str("+lines_live");
    }
    s
}

use std::sync::OnceLock;

use pieces::Rot;

/// Piece tables built once per process/instance (the wasm batch path calls
/// this per entry point, so it must not allocate).
pub fn rots() -> &'static [Vec<Rot>] {
    static ROTS: OnceLock<Vec<Vec<Rot>>> = OnceLock::new();
    ROTS.get_or_init(pieces::rot_table)
}
