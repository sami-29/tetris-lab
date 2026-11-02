//! SIMD128 evaluator for the u32 column kernel (wasm32 only).
//!
//! Vectorises the three aggregate terms of `tetris_engine.evaluate` across the
//! ten columns: heights (leading zeros), holes (popcount under a bitlen mask)
//! and bumpiness (abs diff between neighbouring columns).
//!
//! Heights cost ~20 vector ops per four columns because SIMD128 has no `clz`:
//! this kernel is clz- and popcount-heavy, not lane-parallel arithmetic, which
//! is why the measured gain is small (see the findings file).

use core::arch::wasm32::*;

use crate::kernel::{evaluate_scalar, BOARD_W, W_BUMPINESS, W_HEIGHT, W_HOLES, W_LINES};

/// Vectorised leading-zero count for four u32 lanes (unsigned, values < 2^20).
#[inline(always)]
fn clz_u32x4(x: v128) -> v128 {
    let mut n = i32x4_splat(0);
    let mut v = x;
    // Binary search: if the top `k` bits are empty, charge `k` leading zeros
    // and shift the word up so the next probe looks at the next `k/2` bits.
    macro_rules! step {
        ($k:literal, $probe:literal) => {{
            let top_empty = i32x4_eq(i32x4_shr(u32x4_shr(v, $probe), 0), i32x4_splat(0));
            n = i32x4_add(n, v128_and(top_empty, i32x4_splat($k)));
            v = v128_bitselect(i32x4_shl(v, $k), v, top_empty);
        }};
    }
    step!(16, 16);
    step!(8, 24);
    step!(4, 28);
    step!(2, 30);
    step!(1, 31);
    // v is 0 or 1 here; all-zero lanes need clz == 32.
    let all_zero = i32x4_eq(v, i32x4_splat(0));
    v128_bitselect(i32x4_splat(32), n, all_zero)
}

/// Load the ten column words into three 4-lane chunks (lanes 10, 11 padded).
#[inline(always)]
fn load3(board: &[u32; BOARD_W]) -> [v128; 3] {
    [
        u32x4(board[0], board[1], board[2], board[3]),
        u32x4(board[4], board[5], board[6], board[7]),
        u32x4(board[8], board[9], 0, 0),
    ]
}

/// Score one board with SIMD128.
///
/// # Safety
/// Safe with this crate's build settings: the whole crate is compiled with
/// `-Ctarget-feature=+simd128` and only shipped to runtimes that validate
/// SIMD128 (the `simd` cargo feature is off for the fallback build).
#[target_feature(enable = "simd128")]
pub fn components(board: &[u32; BOARD_W]) -> (i32, i32, i32) {
    let c = load3(board);

    // Heights: 32 - clz(w).
    let h0 = i32x4_sub(i32x4_splat(32), clz_u32x4(c[0]));
    let h1 = i32x4_sub(i32x4_splat(32), clz_u32x4(c[1]));
    let h2 = i32x4_sub(i32x4_splat(32), clz_u32x4(c[2]));

    // Holes: h - popcount(w) per lane (all set bits sit inside the h-bit prefix).
    let holes_lane = |w: v128, h: v128| {
        let bytes = i8x16_popcnt(w);
        let pairs = i16x8_extadd_pairwise_i8x16(bytes);
        i32x4_sub(h, i32x4_extadd_pairwise_i16x8(pairs))
    };
    let ho = i32x4_add(
        i32x4_add(holes_lane(c[0], h0), holes_lane(c[1], h1)),
        holes_lane(c[2], h2),
    );

    // Bumpiness: |h[i] - h[i+1]| for i in 0..8. Shift each chunk by one lane,
    // pulling the neighbour from the following chunk.
    let shift1 = |a: v128, b: v128| {
        i8x16_shuffle::<4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19>(a, b)
    };
    let d0 = i32x4_abs(i32x4_sub(h0, shift1(h0, h1))); // (0,1) (1,2) (2,3) (3,4)
    let d1 = i32x4_abs(i32x4_sub(h1, shift1(h1, h2))); // (4,5) (5,6) (6,7) (7,8)
    let d2 = i32x4_abs(i32x4_sub(h2, shift1(h2, h2))); // (8,9) + 3 padding lanes
                                                       // Only lane 0 of d2 is a real column pair; the rest are padding artefacts.
    let bump = i32x4_add(i32x4_add(d0, d1), v128_and(d2, i32x4(-1, 0, 0, 0)));

    let sum4 = |v: v128| -> i32 {
        i32x4_extract_lane::<0>(v)
            + i32x4_extract_lane::<1>(v)
            + i32x4_extract_lane::<2>(v)
            + i32x4_extract_lane::<3>(v)
    };
    // Lanes 10 and 11 are zero words: height 0, no holes, no contribution.
    let sum_h = sum4(h0) + sum4(h1) + sum4(h2);
    let holes = sum4(ho);
    let bumpiness = sum4(bump);
    (sum_h, holes, bumpiness)
}

/// Score one board with SIMD128, including the lines term the Python weight
/// expects.
#[target_feature(enable = "simd128")]
pub fn evaluate(board: &[u32; BOARD_W], lines_this_move: u32) -> i32 {
    let (sum_h, holes, bumpiness) = components(board);
    let mut score = W_HEIGHT * sum_h + W_HOLES * holes + W_BUMPINESS * bumpiness;
    if cfg!(feature = "lines_live") {
        score += W_LINES * lines_this_move as i32;
    }
    score
}

/// Scalar reference used by the wasm-side parity check.
#[inline(always)]
pub fn evaluate_reference(board: &[u32; BOARD_W], lines_this_move: u32) -> i32 {
    evaluate_scalar(board, lines_this_move)
}
