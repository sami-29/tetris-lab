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
