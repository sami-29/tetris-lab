//! §4.1's three static buffers, and the ABI state the export signatures do not
//! carry.
//!
//! **The offsets are not statics.** A `static mut [u32; 10]` would be given an
//! address by the linker (0x100018 for a `[u32; 200]` in `.bss` on this
//! toolchain), so the loader reading `0x00` would see zeros while the engine
//! wrote elsewhere. Instead the buffers are constants plus pointer arithmetic.
//!
//! **Every access is volatile.** Offset `0x00` is the null pointer in Rust's
//! model: a plain store there is undefined behaviour. It has been observed as a
//! compile error, as an `unreachable` trap, and — the reason for the canary test
//! in `web/test/abi.test.mjs` — as a store the optimiser silently deleted, which
//! made `gen_game` return success while the board read back all zero. Volatile
//! reads and writes are cheap and also stop buffer traffic being reordered across
//! the call boundary.

use core::sync::atomic::{AtomicU32, Ordering};

use engine::board::{Board, W};
use engine::generator::Sequence;
use engine::search::Move;

/// The board: ten `u32` column words, little-endian (§4.1).
pub const BOARD_OFF: u32 = 0x00;
pub const BOARD_BYTES: u32 = (W * 4) as u32;

/// The sequence: one ASCII letter per piece (§4.1).
pub const SEQ_OFF: u32 = 0x40;
pub const SEQ_BYTES: u32 = 200;

/// The moves: 200 `u32` in the §4.1.1 encoding.
pub const MOVES_OFF: u32 = 0x140;
pub const MOVES_BYTES: u32 = 800;
pub const MAX_MOVES: usize = (MOVES_BYTES / 4) as usize;

/// The first byte no §4 buffer may touch. Data segments and the shadow stack must
/// stay above this; `web/test/abi.test.mjs` asserts it, and a small
/// `-zstack-size` is the plausible change that would break it.
pub const ABI_END: u32 = MOVES_OFF + MOVES_BYTES; // 0x460

// ── board ──────────────────────────────────────────────────────────────────

#[inline]
fn board_ptr() -> *mut u32 {
    BOARD_OFF as usize as *mut u32
}

/// Write the Board into the buffer at `0x00`.
pub fn write_board(board: &Board) {
    let ptr = board_ptr();
    for (index, word) in board.0.iter().enumerate() {
        // SAFETY: 0x00..0x28 is inside the module's linear memory and is the
        // buffer §4.1 defines; volatile keeps LLVM from reasoning about the
        // null address.
        unsafe { core::ptr::write_volatile(ptr.add(index), *word) };
    }
}

/// Read the Board back out of the buffer at `0x00`. Bits above row 0 are the
/// caller's business: the engine reports them as status 3.
pub fn read_board() -> Board {
    let ptr = board_ptr() as *const u32;
    let mut board = Board::EMPTY;
    for index in 0..W {
        // SAFETY: as `write_board`.
        board.0[index] = unsafe { core::ptr::read_volatile(ptr.add(index)) };
    }
    board
}

// ── sequence ───────────────────────────────────────────────────────────────

#[inline]
fn seq_ptr() -> *mut u8 {
    SEQ_OFF as usize as *mut u8
}

/// Write the Sequence's letters into the buffer at `0x40` and record its length:
/// `solve` takes no length argument, so the piece count is ABI state (§4).
pub fn write_sequence(sequence: &Sequence) {
    let ptr = seq_ptr();
    for (index, piece) in sequence.as_slice().iter().enumerate() {
        // SAFETY: the Generator bounds the piece count to 200, the buffer's size.
        unsafe { core::ptr::write_volatile(ptr.add(index), piece.letter()) };
    }
    SEQ_LEN.store(sequence.len() as u32, Ordering::Relaxed);
}

/// Forget any game in the buffers. Called when a `gen_game` fails, so a failed
/// call cannot leave a half-written game behind for `solve` to read.
pub fn forget_sequence() {
    SEQ_LEN.store(0, Ordering::Relaxed);
}

/// The piece count of the game currently in the buffers; 0 means "no game".
pub fn sequence_len() -> usize {
    SEQ_LEN.load(Ordering::Relaxed) as usize
}

/// Read the Sequence out of the buffer at `0x40`. `None` if a byte is not one of
/// the seven letters, which would be an illegal ABI state rather than a panic.
pub fn read_sequence(len: usize) -> Option<Sequence> {
    if len == 0 || len > SEQ_BYTES as usize {
        return None;
    }
    let ptr = SEQ_OFF as usize as *const u8;
    let mut letters = [0u8; SEQ_BYTES as usize];
    for (index, slot) in letters.iter_mut().enumerate().take(len) {
        // SAFETY: len <= 200, the buffer's size.
        *slot = unsafe { core::ptr::read_volatile(ptr.add(index)) };
    }
    Sequence::from_letters(&letters[..len])
}

/// `solve` has no length argument, so the last successful `gen_game`'s piece
/// count is ABI state. 0 means the buffers hold no game → status 4.
static SEQ_LEN: AtomicU32 = AtomicU32::new(0);

// ── moves ──────────────────────────────────────────────────────────────────

#[inline]
fn moves_ptr() -> *mut u32 {
    MOVES_OFF as usize as *mut u32
}

/// Write the Move list into the buffer at `0x140`. Returns how many entries are
/// valid, i.e. §4.2's `move_count`.
pub fn write_moves(moves: &[Move]) -> u32 {
    let ptr = moves_ptr();
    let count = moves.len().min(MAX_MOVES);
    for (index, mv) in moves.iter().enumerate().take(count) {
        // SAFETY: count <= 200, the buffer's size.
        unsafe { core::ptr::write_volatile(ptr.add(index), mv.packed()) };
    }
    count as u32
}

// ── the trace region (§4.4) ────────────────────────────────────────────────
//
// §4.4 says the trace is written into "the trace buffer allocated through
// `alloc`", and §4's export signature carries no pointer — so the region is the
// **first** allocation this instance makes. That is a convention, not a
// transcription of the spec, and it is the only one that survives the other
// variable-size buffers §4 grows later: the loader allocates the trace once at
// boot (`loader.ts`'s one `alloc`), before anything else, so "first" is stable
// and testable, while "last" would silently become the batch summary when
// `solve_batch` lands.
//
// Unlike the §4.1 buffers these writes are **plain**, not volatile: an `alloc`'d
// region is above the data base (0x100000 on this toolchain), not inside the
// shadow stack where offset 0x00 lives, so the null-pointer reasoning that makes
// `write_board` volatile does not apply here.

/// Remember `ptr`/`bytes` if the instance has not recorded a region yet.
pub fn remember_trace_region(ptr: u32, bytes: u32) {
    if bytes > 0 && TRACE_BYTES.load(Ordering::Relaxed) == 0 {
        TRACE_PTR.store(ptr, Ordering::Relaxed);
        TRACE_BYTES.store(bytes, Ordering::Relaxed);
    }
}

/// The trace region, as `(byte offset, capacity)`. `(0, 0)` means "never
/// allocated": a `solve_traced` then solves and records nothing.
pub fn trace_region() -> (u32, u32) {
    (
        TRACE_PTR.load(Ordering::Relaxed),
        TRACE_BYTES.load(Ordering::Relaxed),
    )
}

static TRACE_PTR: AtomicU32 = AtomicU32::new(0);
static TRACE_BYTES: AtomicU32 = AtomicU32::new(0);

/// `(capacity - 4) / 20`: how many §4.4 records fit in a region of `bytes`.
#[inline]
pub fn trace_record_capacity(bytes: u32) -> usize {
    if bytes < 4 {
        0
    } else {
        ((bytes - 4) / 20) as usize
    }
}

/// Write §4.4's words — a count, then five `i32` per record — into the region.
/// Returns the bytes written, which never exceeds the capacity the loader
/// reserved, because the engine is handed that capacity as the trace's ceiling.
pub fn write_trace(ptr: u32, words: &[i32], capacity: u32) -> u32 {
    if ptr == 0 || words.is_empty() {
        return 0;
    }
    let max_words = (capacity / 4) as usize;
    let count = words.len().min(max_words);
    let base = ptr as usize as *mut i32;
    for (index, word) in words.iter().enumerate().take(count) {
        // SAFETY: the region came from `alloc` in this module and `count` words
        // fit inside the capacity the caller reserved.
        unsafe { core::ptr::write_volatile(base.add(index), *word) };
    }
    (count * 4) as u32
}
