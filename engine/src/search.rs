//! The Solve: one pass of a layered beam over a known piece Sequence (§5.3).
//!
//! A Layer is reached after placing piece *i*; Dedup happens inside one Layer and
//! never across Layers; the No-suicide rule drops game-ending placements unless
//! nothing else is legal; the final three pieces are searched exactly; and when
//! the Goal is reached the Move list is reconstructed by parent pointer.
//!
//! Visit order, tie-breaking and Dedup domination are observable contract
//! (ADR 0002): a change to any of them changes results and is a deliberate
//! golden-fingerprint update, never an incidental one.

use crate::board::{Board, W};
use crate::evaluator::{Evaluator, PlacementCtx};
use crate::pieces::Piece;

pub use crate::generator::MAX_PIECES;

/// §5.8: the ABI accepts widths up to 16,384; the site's Width cap is a
/// performance measurement and is deliberately *not* in here.
pub const MAX_WIDTH: u32 = 16_384;

/// The packed result carries lines in eight bits, so a Goal above 255 could
/// never be observed as won; the Domain stops there rather than pretend.
pub const MAX_GOAL: u32 = 255;

/// §5.3: the exact Tail search covers the final three pieces.
pub const TAIL: usize = 3;

/// The most placements one piece can produce: T has four rotations whose widths
/// leave 8 + 9 + 8 + 9 columns. Buffers are sized from this, so neither a Layer
/// nor a Tail node has to grow one.
const MAX_PLACEMENTS_PER_PIECE: usize = 34;

/// One placement, encoded the way the moves buffer carries it (§4.1.1).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Move {
    pub piece_index: u8,
    pub rotation: u8,
    pub column: u8,
}

impl Move {
    /// `(piece_index << 16) | (rotation << 8) | column`.
    #[inline]
    pub fn packed(self) -> u32 {
        ((self.piece_index as u32) << 16) | ((self.rotation as u32) << 8) | (self.column as u32)
    }

    #[inline]
    pub fn unpack(word: u32) -> Move {
        Move {
            piece_index: ((word >> 16) & 0xff) as u8,
            rotation: ((word >> 8) & 0xff) as u8,
            column: (word & 0xff) as u8,
        }
    }
}

/// How a Solve ended. Expected failures are values, not panics (§4.2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Ok,
    OutOfDomain,
    BoundExhausted,
    IllegalBoard,
    IllegalAbiState,
}

impl Status {
    /// The status byte of the packed result (§4.2).
    #[inline]
    pub fn code(self) -> u8 {
        match self {
            Status::Ok => 0,
            Status::OutOfDomain => 1,
            Status::BoundExhausted => 2,
            Status::IllegalBoard => 3,
            Status::IllegalAbiState => 4,
        }
    }
}

/// Everything a Solve is asked. The toggles are §9.4's ablation ladder: a config
/// sweep, not a rebuild.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SolveConfig {
    pub goal: u32,
    pub beam_width: u32,
    pub dedup: bool,
    pub no_suicide: bool,
    pub tail: bool,
}

impl SolveConfig {
    /// The shipped behaviour at a Goal and a Beam width.
    pub const fn new(goal: u32, beam_width: u32) -> SolveConfig {
        SolveConfig {
            goal,
            beam_width,
            dedup: true,
            no_suicide: true,
            tail: true,
        }
    }

    fn parameters_in_domain(&self) -> bool {
        self.goal >= 1
            && self.goal <= MAX_GOAL
            && self.beam_width >= 1
            && self.beam_width <= MAX_WIDTH
    }
}

/// The outcome of one Solve.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SolveResult {
    pub status: Status,
    pub won: bool,
    /// Cumulative Lines cleared by the returned Move list.
    pub lines: u32,
    /// Placements evaluated: spawn-legal placements only, counted where the
    /// shipped counter counts them.
    pub evaluations: u32,
    pub moves: Vec<Move>,
}

impl SolveResult {
    fn failed(status: Status) -> SolveResult {
        SolveResult {
            status,
            won: false,
            lines: 0,
            evaluations: 0,
            moves: Vec::new(),
        }
    }

    /// §4.2's packed `i64`. This function is the only place that bit layout
    /// exists, which is why it is tested natively and not in the loader.
    pub fn packed(&self) -> i64 {
        let mut packed: i64 = 0;
        packed |= (self.won as i64) & 1;
        packed |= ((self.evaluations as i64) & 0xffff_ffff) << 1;
        packed |= ((self.status.code() as i64) & 0xff) << 33;
        packed |= ((self.lines as i64) & 0xff) << 41;
        packed |= ((self.moves.len() as i64) & 0x1ff) << 49;
        packed
    }
}

/// "There is no such node": the initial position, or the end of a parent chain.
const NO_NODE: u32 = u32::MAX;

/// One step of a parent chain. The arena is flat and append-only, so a Move list
/// is reconstructed by walking indices, never by re-deriving anything.
#[derive(Clone, Copy)]
struct Node {
    parent: u32,
    mv: Move,
}

/// One state on the beam: the position, the Lines cleared to reach it, and the
/// node that represents it (so the next Layer's candidates can point at it).
#[derive(Clone, Copy)]
struct State {
    board: Board,
    lines: u32,
    node: u32,
}

/// A placement this Layer produced, before Dedup and the top-width cut.
#[derive(Clone, Copy)]
struct Cand {
    board: Board,
    lines: u32,
    /// The node this candidate hangs off, i.e. its parent state's `node`.
    parent: u32,
    score: i64,
    mv: Move,
    suicide: bool,
}

/// Dedup's key is the Board alone: the Board is the whole future and the score is
/// a function of it, so two states with the same Board are the same state (§5.3,
/// ADR 0002).
///
/// Keys live in one flat vector and collide by linear probing rather than in a
/// `HashMap`, because a `HashMap`'s random seed would put an RNG in the engine.
struct BoardIndex {
    /// Index into the kept candidates, plus one: zero means an empty slot.
    slots: Vec<u32>,
    mask: usize,
    len: usize,
}

impl BoardIndex {
    /// `expected` is the most candidates this Layer can produce; the table is
    /// sized for it so a probing chain never runs into a full table.
    fn with_capacity(expected: usize) -> BoardIndex {
        let mut capacity = 16;
        while capacity < expected.saturating_mul(2) {
            capacity <<= 1;
        }
        BoardIndex {
            slots: vec![0; capacity],
            mask: capacity - 1,
            len: 0,
        }
    }

    #[inline]
    fn hash(board: &Board) -> usize {
        // FNV-1a over the ten column words.
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        let mut i = 0;
        while i < W {
            hash = (hash ^ board.0[i] as u64).wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        hash as usize
    }

    /// The position in `kept` holding this Board, if Dedup has seen it.
    fn find(&self, kept: &[Cand], board: &Board) -> Option<usize> {
        let mut slot = BoardIndex::hash(board) & self.mask;
        loop {
            match self.slots[slot] {
                0 => return None,
                occupied => {
                    let index = (occupied - 1) as usize;
                    if kept[index].board == *board {
                        return Some(index);
                    }
                }
            }
            slot = (slot + 1) & self.mask;
        }
    }

    fn insert(&mut self, kept: &[Cand], index: usize) {
        let board = &kept[index].board;
        let mut slot = BoardIndex::hash(board) & self.mask;
        while self.slots[slot] != 0 {
            let occupied = (self.slots[slot] - 1) as usize;
            if kept[occupied].board == *board {
                self.slots[slot] = index as u32 + 1;
                return;
            }
            slot = (slot + 1) & self.mask;
        }
        self.slots[slot] = index as u32 + 1;
        self.len += 1;
    }

    #[inline]
    fn is_crowded(&self) -> bool {
        // Three quarters full: probing stays short and a rebuild stays rare.
        (self.len + 1) * 4 >= self.slots.len() * 3
    }

    /// Rebuild at twice the size from the states already kept. A full table would
    /// make `find` spin, so this is correctness, not tuning.
    fn grow(&mut self, kept: &[Cand]) {
        self.slots = vec![0; self.slots.len() * 2];
        self.mask = self.slots.len() - 1;
        self.len = 0;
        for index in 0..kept.len() {
            self.insert(kept, index);
        }
    }
}

/// Which of two states that share a Board survives Dedup: the one that is not
/// game-ending (so the No-suicide rule can never lose the only legal state),
/// then the one with more Lines cleared.
#[inline]
fn dominates(new: &Cand, old: &Cand) -> bool {
    match (new.suicide, old.suicide) {
        (false, true) => true,
        (true, false) => false,
        _ => new.lines > old.lines,
    }
}

/// Add a candidate, keeping the Dedup index in step.
#[inline]
fn push_kept(kept: &mut Vec<Cand>, cand: Cand, index: &mut BoardIndex) {
    kept.push(cand);
    if index.is_crowded() {
        index.grow(kept); // rebuilds from `kept`, which already holds this one
    } else {
        let position = kept.len() - 1;
        index.insert(kept, position);
    }
}

/// The one entry point: a Board, its Sequence, the Goal and Beam width, and the
/// Evaluator that scores a Layer's placements.
pub fn solve(
    board: &Board,
    sequence: &[Piece],
    cfg: &SolveConfig,
    evaluator: &dyn Evaluator,
) -> SolveResult {
    if !cfg.parameters_in_domain() {
        return SolveResult::failed(Status::OutOfDomain);
    }
    if !board.is_legal() {
        return SolveResult::failed(Status::IllegalBoard);
    }
    if sequence.len() > MAX_PIECES as usize {
        return SolveResult::failed(Status::OutOfDomain);
    }
    if sequence.is_empty() {
        return SolveResult {
            status: Status::Ok,
            won: false,
            lines: 0,
            evaluations: 0,
            moves: Vec::new(),
        };
    }

    let width = cfg.beam_width as usize;
    let mut arena: Vec<Node> = Vec::new();
    // The start position is a state with no node: nothing placed it.
    let mut beam: Vec<State> = vec![State {
        board: *board,
        lines: 0,
        node: NO_NODE,
    }];
    let mut evaluations: u32 = 0;
    let mut best_lines: u32 = 0;

    for (layer, piece) in sequence.iter().enumerate() {
        // §5.3's exact Tail: from the first of the final three pieces onwards the
        // beam's approximation is no longer worth its speed, so the rest of the
        // game is searched exactly from each surviving state.
        if cfg.tail && sequence.len() - layer <= TAIL {
            let mut tail = Tail {
                sequence,
                cfg,
                evaluations,
                best_lines,
            };
            for state in &beam {
                let mut path = Vec::with_capacity(TAIL);
                if let Some(lines) = tail.descend(state.board, state.lines, layer, &mut path) {
                    let mut moves = reconstruct(&arena, state.node);
                    moves.extend_from_slice(&path);
                    return SolveResult {
                        status: Status::Ok,
                        won: true,
                        lines,
                        evaluations: tail.evaluations,
                        moves,
                    };
                }
            }
            return SolveResult {
                status: Status::Ok,
                won: false,
                lines: tail.best_lines,
                evaluations: tail.evaluations,
                moves: Vec::new(),
            };
        }

        let next_piece = sequence.get(layer + 1).copied();
        let rotations = piece.rotations();
        let mut cands: Vec<Cand> = Vec::with_capacity(beam.len());
        let mut dedup_index = if cfg.dedup {
            // Sized for the most placements this Layer can produce.
            Some(BoardIndex::with_capacity(
                beam.len().saturating_mul(MAX_PLACEMENTS_PER_PIECE),
            ))
        } else {
            None
        };
        let mut any_safe = false;

        for parent in &beam {
            for (rotation_index, rotation) in rotations.iter().enumerate() {
                for column in 0..=(W - rotation.width as usize) {
                    let Some(shift) = parent.board.rest_shift(rotation, column) else {
                        continue; // no legal spawn position: not an evaluation
                    };
                    evaluations = evaluations.saturating_add(1);

                    let mut next_board = parent.board;
                    next_board.place(rotation, column, shift);
                    // The rows this placement completes, read *before* the clear:
                    // the clear is exactly what the evaluator's context has to
                    // remember, because the resulting board has forgotten it.
                    let completed = next_board.full_rows();
                    let cleared = next_board.clear_full_rows();
                    let lines = parent.lines + cleared;
                    if lines > best_lines {
                        best_lines = lines;
                    }

                    let mv = Move {
                        piece_index: piece.index(),
                        rotation: rotation_index as u8,
                        column: column as u8,
                    };

                    // The Goal ends the search. The winning Move list comes back
                    // by parent pointer, which is why the node is pushed before
                    // the result is assembled.
                    if lines >= cfg.goal {
                        let winning = push_node(&mut arena, parent.node, mv);
                        return SolveResult {
                            status: Status::Ok,
                            won: true,
                            lines,
                            evaluations,
                            moves: reconstruct(&arena, winning),
                        };
                    }

                    // Game over is spawn-blocked at row 0 (§5.3).
                    let suicide = cfg.no_suicide
                        && next_piece.is_some_and(|next| !next_board.can_place_anywhere(next));
                    if !suicide {
                        any_safe = true;
                    }
                    let cand = Cand {
                        board: next_board,
                        lines,
                        parent: parent.node,
                        score: evaluator.score(
                            &next_board,
                            &PlacementCtx {
                                lines_cleared: cleared,
                                landing_height: rotation.landing_height(shift),
                                eroded_cells: cleared * rotation.cells_in_rows(shift, completed),
                            },
                        ),
                        mv,
                        suicide,
                    };
                    match &mut dedup_index {
                        Some(index) => match index.find(&cands, &next_board) {
                            Some(position) => {
                                if dominates(&cand, &cands[position]) {
                                    cands[position] = cand;
                                }
                            }
                            None => push_kept(&mut cands, cand, index),
                        },
                        None => cands.push(cand),
                    }
                }
            }
        }

        // §5.3's No-suicide rule is "unless nothing else is legal": game-ending
        // placements only survive a Layer in which every placement ends the game.
        if any_safe && cfg.no_suicide {
            cands.retain(|cand| !cand.suicide);
        }
        if cands.is_empty() {
            return SolveResult {
                status: Status::Ok,
                won: false,
                lines: best_lines,
                evaluations,
                moves: Vec::new(),
            };
        }

        // Stable descending sort: ties keep insertion order, which is
        // `(piece, rotation, column)` ascending.
        cands.sort_by_key(|cand| std::cmp::Reverse(cand.score));
        if cands.len() > width {
            cands.truncate(width);
        }

        beam.clear();
        for cand in &cands {
            let node = push_node(&mut arena, cand.parent, cand.mv);
            beam.push(State {
                board: cand.board,
                lines: cand.lines,
                node,
            });
        }
    }

    SolveResult {
        status: Status::Ok,
        won: false,
        lines: best_lines,
        evaluations,
        moves: Vec::new(),
    }
}

/// One legal placement held on the stack while a Tail node decides what to keep.
#[derive(Clone, Copy)]
struct Slot {
    board: Board,
    lines: u32,
    mv: Move,
    suicide: bool,
}

/// §5.3's exact Tail: all legal placements over the remaining pieces, no trim,
/// depth first in `(piece, rotation, column)` ascending order, so the first
/// combination that reaches the Goal is the one the search returns.
///
/// Depth first rather than a layered expansion on purpose: a Layer of the Tail
/// can hold hundreds of thousands of states, and this searches the same set of
/// placements with flat memory and the same visit order.
struct Tail<'a> {
    sequence: &'a [Piece],
    cfg: &'a SolveConfig,
    evaluations: u32,
    best_lines: u32,
}

impl Tail<'_> {
    /// Play out `board` from Tail piece `layer`. On a win, `path` holds the Moves
    /// from that piece to the winning one and the value is the Lines cleared by
    /// the whole game.
    fn descend(
        &mut self,
        board: Board,
        lines: u32,
        layer: usize,
        path: &mut Vec<Move>,
    ) -> Option<u32> {
        let piece = self.sequence[layer];
        let next_piece = self.sequence.get(layer + 1).copied();
        let rotations = piece.rotations();

        let empty = Slot {
            board,
            lines,
            mv: Move::default(),
            suicide: false,
        };
        let mut slots = [empty; MAX_PLACEMENTS_PER_PIECE];
        let mut count = 0usize;

        for (rotation_index, rotation) in rotations.iter().enumerate() {
            for column in 0..=(W - rotation.width as usize) {
                let Some(shift) = board.rest_shift(rotation, column) else {
                    continue;
                };
                self.evaluations = self.evaluations.saturating_add(1);

                let mut next_board = board;
                next_board.place(rotation, column, shift);
                let cleared = next_board.clear_full_rows();
                let next_lines = lines + cleared;
                if next_lines > self.best_lines {
                    self.best_lines = next_lines;
                }
                let mv = Move {
                    piece_index: piece.index(),
                    rotation: rotation_index as u8,
                    column: column as u8,
                };

                // A placement that reaches the Goal wins, whatever comes next.
                if next_lines >= self.cfg.goal {
                    path.push(mv);
                    return Some(next_lines);
                }
                let suicide = self.cfg.no_suicide
                    && next_piece.is_some_and(|next| !next_board.can_place_anywhere(next));
                slots[count] = Slot {
                    board: next_board,
                    lines: next_lines,
                    mv,
                    suicide,
                };
                count += 1;
            }
        }

        // The last Tail piece has nothing to recurse into.
        if layer + 1 >= self.sequence.len() {
            return None;
        }
        // No win at this node: recurse, and let the No-suicide rule drop a
        // game-ending placement only when something else was legal.
        let any_safe = slots[..count].iter().any(|slot| !slot.suicide);
        for slot in &slots[..count] {
            if self.cfg.no_suicide && any_safe && slot.suicide {
                continue;
            }
            path.push(slot.mv);
            if let Some(lines) = self.descend(slot.board, slot.lines, layer + 1, path) {
                return Some(lines);
            }
            path.pop();
        }
        None
    }
}

/// Walk a parent chain back to the start position and read it forwards.
fn reconstruct(arena: &[Node], last: u32) -> Vec<Move> {
    let mut moves = Vec::new();
    let mut cursor = last;
    while cursor != NO_NODE {
        let node = arena[cursor as usize];
        moves.push(node.mv);
        cursor = node.parent;
    }
    moves.reverse();
    moves
}

#[inline]
fn push_node(arena: &mut Vec<Node>, parent: u32, mv: Move) -> u32 {
    arena.push(Node { parent, mv });
    (arena.len() - 1) as u32
}
