//! wasm ABI for the ticket 06 spike (wasm32-unknown-unknown, raw exports).
//!
//! The JS side loads the whole games file once, then asks for games one at a
//! time (`run_game`, for per-game timing) or all at once (`solve_all`, the batch
//! path ticket 05 proposes). Memory is a plain exported `Vec` buffer so Node can
//! write the file in and read results out without any glue tooling.

use std::sync::Mutex;

use crate::kernel::Game;
use crate::W;

static GAMES: Mutex<Vec<Game<W>>> = Mutex::new(Vec::new());

#[no_mangle]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

#[no_mangle]
pub extern "C" fn dealloc(ptr: *mut u8, len: usize) {
    unsafe {
        drop(Vec::from_raw_parts(ptr, 0, len));
    }
}

/// Parse the games file already copied into wasm memory. Returns the game count.
#[no_mangle]
pub extern "C" fn load_games(ptr: *const u8, len: usize) -> u32 {
    let text = unsafe { std::slice::from_raw_parts(ptr, len) };
    let text = std::str::from_utf8(text).expect("games file is utf-8");
    let games = crate::games::parse::<W>(text);
    let n = games.len() as u32;
    *GAMES.lock().expect("games mutex") = games;
    n
}

#[no_mangle]
pub extern "C" fn game_count() -> u32 {
    GAMES.lock().expect("games mutex").len() as u32
}

/// Solve game `idx`. Returns `success | evals << 1`.
#[no_mangle]
pub extern "C" fn run_game(idx: u32, goal: u32) -> u64 {
    let rots = crate::rots();
    let games = GAMES.lock().expect("games mutex");
    let game = &games[idx as usize];
    let r = game.solve(&rots, goal);
    (r.success as u64) | ((r.evals as u64) << 1)
}

/// Batch entry point: solve up to `max` loaded games, writing `[success, evals]`
/// per game into `out`. Returns the number of games solved.
#[no_mangle]
pub extern "C" fn solve_all(goal: u32, out: *mut u32, max: u32) -> u32 {
    let rots = crate::rots();
    let games = GAMES.lock().expect("games mutex");
    let n = (max as usize).min(games.len());
    let out = unsafe { std::slice::from_raw_parts_mut(out, n * 2) };
    for (i, game) in games.iter().take(n).enumerate() {
        let r = game.solve(&rots, goal);
        out[i * 2] = r.success as u32;
        out[i * 2 + 1] = r.evals;
    }
    n as u32
}

/// Correctness gate for the SIMD evaluator: score `n` boards with both the
/// vector and scalar paths, return the mismatch count. `boards` is `n * 10` u32
/// column words.
#[cfg(feature = "simd")]
#[no_mangle]
pub extern "C" fn parity_check(boards: *const u32, n: u32) -> u32 {
    let flat = unsafe { std::slice::from_raw_parts(boards, n as usize * crate::kernel::BOARD_W) };
    let mut bad = 0u32;
    for i in 0..n as usize {
        let mut board = [0u32; crate::kernel::BOARD_W];
        board.copy_from_slice(&flat[i * crate::kernel::BOARD_W..(i + 1) * crate::kernel::BOARD_W]);
        for lines in [0u32, 1, 2, 4] {
            let a = crate::simd::evaluate(&board, lines);
            let b = crate::simd::evaluate_reference(&board, lines);
            if a != b {
                bad += 1;
            }
        }
    }
    bad
}

/// Scalar-only builds still export the symbol so the harness need not branch.
#[cfg(not(feature = "simd"))]
#[no_mangle]
pub extern "C" fn parity_check(_boards: *const u32, _n: u32) -> u32 {
    0
}

/// Which kernel this module is: bit 0 u32, bit 1 u64, bit 2 simd, bit 3 lines_live.
#[no_mangle]
pub extern "C" fn variant_marker() -> u32 {
    (cfg!(feature = "word32") as u32)
        | ((cfg!(feature = "word64") as u32) << 1)
        | ((cfg!(feature = "simd") as u32) << 2)
        | ((cfg!(feature = "lines_live") as u32) << 3)
}

/// Debug aid: write `[simd_sum_h, simd_holes, simd_bump, simd_score,
/// scalar_sum_h, scalar_holes, scalar_bump, scalar_score]` for one board.
#[cfg(feature = "simd")]
#[no_mangle]
pub extern "C" fn debug_components(boards: *const u32, out: *mut i32) {
    let flat = unsafe { std::slice::from_raw_parts(boards, crate::kernel::BOARD_W) };
    let mut board = [0u32; crate::kernel::BOARD_W];
    board.copy_from_slice(flat);
    let (a, b, c) = crate::simd::components(&board);
    let (d, e, f) = crate::kernel::components_scalar(&board);
    let out = unsafe { std::slice::from_raw_parts_mut(out, 8) };
    out[0] = a;
    out[1] = b;
    out[2] = c;
    out[3] = crate::simd::evaluate(&board, 0);
    out[4] = d;
    out[5] = e;
    out[6] = f;
    out[7] = crate::kernel::evaluate_scalar(&board, 0);
}
