//! Throwaway kernel for ticket 09.
//!
//! Sized to prove three things on the deployed host and nothing else: a
//! `wasm32-unknown-unknown` module arrives with the right MIME type, a module
//! worker can `instantiateStreaming` it, and its exports do real integer work.
//! The real kernel is ADR-0001's business, not this file's.

/// Bumped with any ABI change, per ADR-0001's typed-ABI rule.
#[no_mangle]
pub extern "C" fn abi_version() -> u32 {
    1
}

#[no_mangle]
pub extern "C" fn add(a: i32, b: i32) -> i32 {
    a.wrapping_add(b)
}

/// Column-word bit work: builds `boards` pseudo-random 20-bit words and sums
/// their low 20 bits. Deterministic for a given seed, so the page can print a
/// value that a rerun can be compared against.
#[no_mangle]
pub extern "C" fn probe_bits(boards: u32, seed: u32) -> u32 {
    let mut state = seed | 1;
    let mut total: u32 = 0;
    for _ in 0..boards {
        let mut word: u32 = 0;
        for _ in 0..10 {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            word = (word << 2) | (state >> 30);
        }
        total = total.wrapping_add(word & 0x000F_FFFF);
    }
    total
}
