//! The ABI: §4's exports and nothing else.
//!
//! Hand-written `#[no_mangle] pub extern "C"` functions, no glue generator, no
//! `wasm-bindgen`: Cargo emits these into the export section directly.
//!
//! The failure contract is §4.2's. Every *expected* failure is a typed status —
//! never a panic, an `unwrap` or an `assert!`, because `panic = "abort"` in the
//! release profile turns any of those into a trap that costs a worker. A bug is
//! allowed to abort; the worker respawns and logs it.
//!
//! `solve_batch` is the last of §4's six exports and arrives in a later ticket **at
//! this same `abi_version`**. The loader honours the version and must not probe for
//! it.

use engine::generator::{generate_game, DomainError};
use engine::search::{
    solve as engine_solve, solve_traced as engine_solve_traced, SolveConfig, Status,
};
use engine::trace::Trace;
use engine::{shipped, Board, SolveResult};

pub mod buffers;

use buffers::{
    forget_sequence, read_board, read_sequence, remember_trace_region, sequence_len,
    trace_record_capacity, trace_region, write_board, write_moves, write_sequence, write_trace,
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
    let offset = ptr as usize as u32;
    // §4.4 has `solve_traced` write into "the trace buffer allocated through
    // `alloc`" and §4's signatures carry no pointer, so the region is the *first*
    // allocation this instance makes: the loader allocates the trace once at boot
    // and before anything else (`buffers::remember_trace_region`).
    remember_trace_region(offset, size as u32);
    offset
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
/// No trace: that is `solve_traced`.
#[no_mangle]
pub extern "C" fn solve(goal: u32, beam_width: u32) -> i64 {
    solve_and_pack(goal, beam_width, None)
}

/// §4 export 5: as `solve`, and §4.4's trace into the `alloc`'d trace region.
///
/// `band` bounds the records and never the search, so the packed result is
/// bit-identical to `solve`'s for the same `(goal, beam_width)` — that equality is
/// asserted natively (`engine/tests/trace.rs`) and through this export
/// (`web/test/solve-traced.test.mjs`). At most `band` records are kept per Layer,
/// and the count word at word 0 is the record count, written last, so a trap in
/// the middle of a trace cannot present a half-written one as complete.
#[no_mangle]
pub extern "C" fn solve_traced(goal: u32, beam_width: u32, band: u32) -> i64 {
    solve_and_pack(goal, beam_width, Some(band))
}

fn solve_and_pack(goal: u32, beam_width: u32, band: Option<u32>) -> i64 {
    let Some(sequence) = read_sequence(sequence_len()) else {
        // Status 4: no game in the buffers, or a sequence byte that is not a
        // piece. Both mean the loader used the ABI out of order — and an empty
        // trace, because the previous solve's records must not be read as this
        // one's.
        write_empty_trace();
        return SolveResult {
            status: Status::IllegalAbiState,
            won: false,
            lines: 0,
            evaluations: 0,
            moves: Vec::new(),
            tail_truncated: false,
        }
        .packed();
    };

    let board: Board = read_board();
    let cfg = SolveConfig::new(goal, beam_width);
    let result = match band {
        None => engine_solve(&board, sequence.as_slice(), &cfg, shipped()),
        Some(band) => {
            let (ptr, capacity) = trace_region();
            // The engine is handed the region's own record ceiling, so it stops at
            // a whole Layer instead of writing past the buffer it was given. This
            // is what makes "a trace can never overrun" structural rather than a
            // bound on a multiplication.
            let mut trace = Trace::with_capacity(trace_record_capacity(capacity));
            let result = engine_solve_traced(
                &board,
                sequence.as_slice(),
                &cfg,
                shipped(),
                band,
                &mut trace,
            );
            let words = trace.words();
            write_trace(ptr, &words, capacity);
            result
        }
    };
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

/// Word 0 of a region nobody is writing records into.
fn write_empty_trace() {
    let (ptr, capacity) = trace_region();
    if ptr == 0 || capacity < 4 {
        return;
    }
    write_trace(ptr, &[0], 4);
}
