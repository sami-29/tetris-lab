//! The ABI: four raw exports and nothing else (§4).
//!
//! Hand-written `#[no_mangle] pub extern "C"` functions, no glue generator, no
//! `wasm-bindgen`: Cargo emits these into the export section directly.
//!
//! The failure contract is §4.2's. Every *expected* failure is a typed status —
//! never a panic, an `unwrap` or an `assert!`, because `panic = "abort"` in the
//! release profile turns any of those into a trap that costs a worker. A bug is
//! allowed to abort; the worker respawns and logs it.
//!
//! `solve_traced` and `solve_batch` are the other two of §4's six exports and
//! arrive in later tickets **at this same `abi_version`**. The loader therefore
//! checks the version and must not probe for them.

use engine::generator::{generate_game, DomainError};
use engine::search::{solve as engine_solve, SolveConfig, Status};
use engine::{shipped, Board, SolveResult};

pub mod buffers;

use buffers::{
    forget_sequence, read_board, read_sequence, sequence_len, write_board, write_moves,
    write_sequence,
};

/// §4's `abi_version`: `gen_game` landing is what made it 2.
pub const ABI_VERSION: u32 = 2;

/// The §4.1 buffers, for a caller that wants to assert the layout rather than
/// hard-code it (the canary test does).
#[no_mangle]
pub extern "C" fn abi_version() -> u32 {
    ABI_VERSION
}

/// §4 export 2: allocate `len` bytes in engine memory, return the byte offset.
///
/// Never freed — the contract is that variable-size buffers (the trace, a batch
/// summary) are allocated once at boot and reused, so leaking is the behaviour,
/// not an oversight. Alignment is 16 bytes; the loader must not depend on more.
#[no_mangle]
pub extern "C" fn alloc(len: u32) -> u32 {
    let size = len.max(16) as usize;
    let layout = match std::alloc::Layout::from_size_align(size, 16) {
        Ok(layout) => layout,
        // A layout the allocator cannot express is a caller bug, and statuses
        // cannot be returned from this signature; §4 defines `alloc` as infallible
        // for the sizes the app asks for. Ask for zero and get a valid pointer
        // rather than a trap.
        Err(_) => return 0,
    };
    // SAFETY: the layout has a non-zero size, and the pointer is deliberately
    // leaked: §4 says these buffers are never freed.
    let ptr = unsafe { std::alloc::alloc(layout) };
    if ptr.is_null() {
        return 0; // out of memory is the caller's problem, not a trap
    }
    ptr as usize as u32
}

/// §4 export 3: generate a game into the board and sequence buffers.
///
/// `0` on success, otherwise §4.2's status code (§5.8's Domain failures are 1 and
/// 2). A failed call leaves no game behind, so `solve` reports status 4 instead of
/// solving half a game.
#[no_mangle]
pub extern "C" fn gen_game(seed: u32, pieces: u32, height_cap: u32) -> i32 {
    match generate_game(seed, pieces, height_cap) {
        Ok(game) => {
            write_board(&game.board);
            write_sequence(&game.sequence);
            0
        }
        Err(DomainError::OutOfDomain) => {
            forget_sequence();
            Status::OutOfDomain.code() as i32
        }
        Err(DomainError::BoundExhausted) => {
            forget_sequence();
            Status::BoundExhausted.code() as i32
        }
    }
}

/// §4 export 4: solve the Board and Sequence currently in the buffers.
///
/// Returns §4.2's packed `i64` and writes the Move list into the moves buffer.
/// No trace: that is `solve_traced`, ticket 05.
#[no_mangle]
pub extern "C" fn solve(goal: u32, beam_width: u32) -> i64 {
    let Some(sequence) = read_sequence(sequence_len()) else {
        // Status 4: no game in the buffers, or a sequence byte that is not a
        // piece. Both mean the loader used the ABI out of order.
        return SolveResult {
            status: Status::IllegalAbiState,
            won: false,
            lines: 0,
            evaluations: 0,
            moves: Vec::new(),
        }
        .packed();
    };

    let board: Board = read_board();
    let result = engine_solve(
        &board,
        sequence.as_slice(),
        &SolveConfig::new(goal, beam_width),
        shipped(),
    );
    let written = write_moves(&result.moves);

    // The status precedence is observable: the ABI-state check runs first, so a
    // domain failure on an empty instance reports 4 and not 1.
    let mut packed = result.packed();
    if result.status == Status::Ok {
        // Trust the buffer, not the vector: the packed move count is what the
        // loader will read out of memory.
        packed &= !(0x1ff_i64 << 49);
        packed |= ((written as i64) & 0x1ff) << 49;
    }
    packed
}
