//! Bitboard beam search — a faithful port of `TetrisSolver.py` plus the live
//! terms of `tetris_engine.evaluate`, over column bitboards instead of numpy
//! arrays.
//!
//! Faithfulness is the point: this spike must spend its evaluations in the same
//! places the Python solver does, so the speed ratio means something. Kept
//! identical to the Python code:
//!
//! * beam schedule (1, 3, 7, 25), narrowest first, first success wins;
//! * iteration order state -> rotation -> column, ascending columns;
//! * stable sort by score, descending, trimmed to the beam width;
//! * the dead `_W_LINES` term (zero unless the `lines_live` feature is on);
//! * the eval counter increments once per spawn-legal placement, exactly where
//!   `TetrisSolver` increments its `evaluations`.
//!
//! Deliberately NOT ported (out of scope for the spike): dedup, the
//! Dellacherie-Thierry evaluator and the exact tail search from ticket 04.

use crate::pieces::Rot;

pub const BOARD_W: usize = 10;
pub const BOARD_H: i32 = 20;

pub const W_HEIGHT: i32 = -51;
pub const W_HOLES: i32 = -35;
pub const W_BUMPINESS: i32 = -18;
pub const W_LINES: i32 = 1000;

/// Column-word operations the kernel needs. Implemented for u32 and u64 so the
/// u32-versus-u64 question is measured rather than argued.
pub trait Word: Copy + PartialEq {
    const BITS: u32;
    fn zero() -> Self;
    fn one() -> Self;
    fn from_u32(x: u32) -> Self;
    fn lz(self) -> u32;
    fn popcount(self) -> u32;
    fn shl(self, n: u32) -> Self;
    fn shr(self, n: u32) -> Self;
    fn and(self, o: Self) -> Self;
    fn or(self, o: Self) -> Self;
    fn andnot(self, o: Self) -> Self;
    fn sub(self, o: Self) -> Self;

    /// Score a board the way `tetris_engine.evaluate` does.
    #[inline(always)]
    fn evaluate(board: &[Self; BOARD_W], lines_this_move: u32) -> i32 {
        evaluate_scalar(board, lines_this_move)
    }
}

macro_rules! impl_word {
    ($t:ty) => {
        impl Word for $t {
            const BITS: u32 = <$t>::BITS;
            #[inline(always)]
            fn zero() -> Self {
                0
            }
            #[inline(always)]
            fn one() -> Self {
                1
            }
            #[inline(always)]
            fn from_u32(x: u32) -> Self {
                x as $t
            }
            #[inline(always)]
            fn lz(self) -> u32 {
                self.leading_zeros()
            }
            #[inline(always)]
            fn popcount(self) -> u32 {
                self.count_ones()
            }
            #[inline(always)]
            fn shl(self, n: u32) -> Self {
                self << n
            }
            #[inline(always)]
            fn shr(self, n: u32) -> Self {
                self >> n
            }
            #[inline(always)]
            fn and(self, o: Self) -> Self {
                self & o
            }
            #[inline(always)]
            fn or(self, o: Self) -> Self {
                self | o
            }
            #[inline(always)]
            fn andnot(self, o: Self) -> Self {
                self & !o
            }
            #[inline(always)]
            fn sub(self, o: Self) -> Self {
                self - o
            }
        }
    };
}

impl_word!(u64);

// u32 gets the same ops plus a SIMD scoring path when the crate is built for
// wasm32 with the `simd` feature. One trait impl only: the dispatch is a cfg
// inside the impl, not a second impl.
impl Word for u32 {
    const BITS: u32 = u32::BITS;
    #[inline(always)]
    fn zero() -> Self {
        0
    }
    #[inline(always)]
    fn one() -> Self {
        1
    }
    #[inline(always)]
    fn from_u32(x: u32) -> Self {
        x
    }
    #[inline(always)]
    fn lz(self) -> u32 {
        self.leading_zeros()
    }
    #[inline(always)]
    fn popcount(self) -> u32 {
        self.count_ones()
    }
    #[inline(always)]
    fn shl(self, n: u32) -> Self {
        self << n
    }
    #[inline(always)]
    fn shr(self, n: u32) -> Self {
        self >> n
    }
    #[inline(always)]
    fn and(self, o: Self) -> Self {
        self & o
    }
    #[inline(always)]
    fn or(self, o: Self) -> Self {
        self | o
    }
    #[inline(always)]
    fn andnot(self, o: Self) -> Self {
        self & !o
    }
    #[inline(always)]
    fn sub(self, o: Self) -> Self {
        self - o
    }

    #[cfg(all(target_arch = "wasm32", feature = "simd"))]
    #[inline(always)]
    fn evaluate(board: &[Self; BOARD_W], lines_this_move: u32) -> i32 {
        crate::simd::evaluate(board, lines_this_move)
    }
}

/// Stack height of a column: bits used, i.e. `BITS - clz`.
#[inline(always)]
pub fn bitlen<W: Word>(w: W) -> u32 {
    if w == W::zero() {
        0
    } else {
        W::BITS - w.lz()
    }
}

/// The three aggregate terms the shipped heuristic scores on.
#[inline(always)]
pub fn components_scalar<W: Word>(board: &[W; BOARD_W]) -> (i32, i32, i32) {
    let mut sum_h: i32 = 0;
    let mut holes: i32 = 0;
    let mut bump: i32 = 0;
    let mut prev_h: i32 = -1;
    for w in board.iter() {
        let h = bitlen(*w) as i32;
        sum_h += h;
        holes += h - W::popcount(*w) as i32;
        if prev_h >= 0 {
            bump += (prev_h - h).abs();
        }
        prev_h = h;
    }
    (sum_h, holes, bump)
}

#[inline(always)]
pub fn evaluate_scalar<W: Word>(board: &[W; BOARD_W], lines_this_move: u32) -> i32 {
    let mut sum_h: i32 = 0;
    let mut holes: i32 = 0;
    let mut bump: i32 = 0;
    let mut prev_h: i32 = -1;
    for w in board.iter() {
        let h = bitlen(*w) as i32;
        sum_h += h;
        // Cells below the stack top that are empty: all set bits lie inside the
        // `h`-bit prefix, so holes == h - popcount(w).
        holes += h - W::popcount(*w) as i32;
        if prev_h >= 0 {
            bump += (prev_h - h).abs();
        }
        prev_h = h;
    }
    let mut score = W_HEIGHT * sum_h + W_HOLES * holes + W_BUMPINESS * bump;
    if cfg!(feature = "lines_live") {
        score += W_LINES * lines_this_move as i32;
    }
    score
}

/// A u32 kernel built with the `simd` feature on wasm32 scores through
/// `simd`; every other configuration stays scalar (the trait default).
#[inline(always)]
fn clear_rows<W: Word>(mut w: W, full: W) -> W {
    // Remove each full row, highest first so lower bits never shift twice.
    let mut b = W::BITS;
    while b > 0 {
        b -= 1;
        if W::and(full, W::one().shl(b)) != W::zero() {
            let lower = if b == 0 {
                W::zero()
            } else {
                W::and(w, W::one().shl(b).sub(W::one()))
            };
            let upper = W::shl(W::shr(w, b + 1), b);
            w = W::or(lower, upper);
        }
    }
    w
}

#[derive(Clone, Copy)]
struct Cand<W> {
    score: i32,
    lines: u32,
    board: [W; BOARD_W],
}

#[derive(Clone, Copy)]
struct State<W> {
    lines: u32,
    board: [W; BOARD_W],
}

/// One game: pre-filled start board plus the known piece sequence.
#[derive(Clone)]
pub struct Game<W> {
    pub seed: u32,
    pub board: [W; BOARD_W],
    pub seq: Vec<u8>,
}

/// Result of one solve: did the beam reach the goal, and how many candidate
/// placements did it score (same definition as the Python counter).
#[derive(Clone, Copy, Debug, Default)]
pub struct Solve {
    pub success: bool,
    pub evals: u32,
}

impl<W: Word> Game<W> {
    /// Search one beam width, mirroring `TetrisSolver._beam_search`.
    #[inline]
    fn beam(&self, rots: &[Vec<Rot>], width: usize, goal: u32) -> (bool, u32) {
        let mut evals: u32 = 0;
        let mut beam: Vec<State<W>> = vec![State {
            lines: 0,
            board: self.board,
        }];
        let mut next: Vec<State<W>> = Vec::with_capacity(width + 64);
        let mut cands: Vec<Cand<W>> = Vec::with_capacity(width * 34 + 64);
        let goal_board_len = self.seq.len();

        for piece in self.seq.iter() {
            let rotations = &rots[*piece as usize];
            let mut cands_len = 0usize;

            for state in beam.iter() {
                for (rot_idx, rot) in rotations.iter().enumerate() {
                    let _ = rot_idx;
                    let w = rot.width as usize;
                    let s_start = BOARD_H - rot.height as i32;
                    for col in 0..=(BOARD_W - w) {
                        // Lowest legal shift: s >= h(c+i) - tz(p_i) for every column.
                        let mut s_min: i32 = 0;
                        for i in 0..w {
                            let tz = (rot.cols[i] as u32).trailing_zeros() as i32;
                            let need = bitlen(state.board[col + i]) as i32 - tz;
                            if need > s_min {
                                s_min = need;
                            }
                        }
                        // The piece must also fit at the spawn row; such columns
                        // are excluded by `can_place(.., 0, col)` in Python and
                        // never reach the eval counter there.
                        if s_min > s_start {
                            continue;
                        }
                        evals += 1;
                        let s = s_min as u32;

                        let mut nb = state.board;
                        for i in 0..w {
                            nb[col + i] =
                                W::or(nb[col + i], W::from_u32(rot.cols[i] as u32).shl(s));
                        }

                        let mut full = nb[0];
                        for word in nb.iter().skip(1) {
                            full = W::and(full, *word);
                        }
                        let mut lines = 0u32;
                        if full != W::zero() {
                            lines = full.popcount();
                            for w2 in nb.iter_mut() {
                                *w2 = clear_rows(*w2, full);
                            }
                        }

                        let new_lines = state.lines + lines;
                        if new_lines >= goal {
                            return (true, evals);
                        }

                        let cand = Cand {
                            score: W::evaluate(&nb, lines),
                            lines: new_lines,
                            board: nb,
                        };
                        if cands_len < cands.len() {
                            cands[cands_len] = cand;
                        } else {
                            cands.push(cand);
                        }
                        cands_len += 1;
                    }
                }
            }

            if cands_len == 0 {
                return (false, evals);
            }
            let slice = &mut cands[..cands_len];
            if cands_len > width {
                // Stable sort by score, descending: same tie order as Python.
                slice.sort_by_key(|c| std::cmp::Reverse(c.score));
            }
            let keep = if cands_len > width { width } else { cands_len };
            next.clear();
            for c in slice[..keep].iter() {
                next.push(State {
                    lines: c.lines,
                    board: c.board,
                });
            }
            core::mem::swap(&mut beam, &mut next);
            let _ = goal_board_len;
        }
        (false, evals)
    }

    /// Solve with the app's progressive schedule, first success wins.
    pub fn solve(&self, rots: &[Vec<Rot>], goal: u32) -> Solve {
        let mut total = 0u32;
        for &width in &[1usize, 3, 7, 25] {
            let (ok, evals) = self.beam(rots, width, goal);
            total += evals;
            if ok {
                return Solve {
                    success: true,
                    evals: total,
                };
            }
        }
        Solve {
            success: false,
            evals: total,
        }
    }
}
