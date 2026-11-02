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

use crate::board::{Board, H, W};
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

/// The most states one Tail layer may carry.
///
/// A Tail layer is `states x placements`, and a state is 64 bytes, so at
/// `MAX_WIDTH` an uncapped last layer is hundreds of megabytes of frontier: the
/// failure mode is a wasm out-of-memory trap, which the ABI defines as "respawn
/// the worker and report it" (§4.2) — a bug, not a bad result. 131,072 states is
/// 8.4 MB per layer and bounds the last layer to 131,072 x 34 expansions.
///
/// Trimming is a property of the *frontier*, not of a state's placements, and it
/// keeps the best-scoring prefix of the layer on the same stable key the beam
/// trims with, so it never reorders a tie. Moving this constant changes the
/// Evaluations of every game whose Tail runs, which makes it
/// golden-fingerprint surface: a deliberate update, never an incidental one
/// (§5.3, §9.5).
pub const TAIL_STATES_MAX: usize = 131_072;

/// How far past [`TAIL_STATES_MAX`] a Tail layer may grow before it is trimmed.
/// The cap bounds memory; the slack bounds how often it has to re-sort and
/// re-index a layer, which is the cost that would otherwise dominate a wide Tail.
/// Four thousand states is ~34k candidates, ~2 MB.
const TAIL_TRIM_SLACK: usize = 4096 * MAX_PLACEMENTS_PER_PIECE;

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

/// §9.4's three search decisions, in one place: an Ablation is a configuration
/// change, never a second engine build.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Toggles {
    /// §5.3's per-Layer board Dedup, in the beam and in every Tail layer.
    pub dedup: bool,
    /// §5.3's No-suicide rule: drop placements that end the game unless nothing
    /// else is legal.
    pub no_suicide: bool,
    /// §5.3's exact Tail search over the final [`TAIL`] pieces.
    pub tail: bool,
}

impl Toggles {
    /// The shipped row: every search decision on. The one construction site the
    /// shipped paths use, so the page, the ABI and the bench cannot drift apart.
    pub const SHIPPED: Toggles = Toggles {
        dedup: true,
        no_suicide: true,
        tail: true,
    };

    /// The shipped row with exactly one decision off: §9.4's ladder row.
    pub const fn without(self, feature: Feature) -> Toggles {
        match feature {
            Feature::Dedup => Toggles {
                dedup: false,
                no_suicide: self.no_suicide,
                tail: self.tail,
            },
            Feature::NoSuicide => Toggles {
                dedup: self.dedup,
                no_suicide: false,
                tail: self.tail,
            },
            Feature::Tail => Toggles {
                dedup: self.dedup,
                no_suicide: self.no_suicide,
                tail: false,
            },
        }
    }
}

/// One of the three decisions §9.4's Ablation switches off, one at a time.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Feature {
    Dedup,
    NoSuicide,
    Tail,
}

/// Everything a Solve is asked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SolveConfig {
    pub goal: u32,
    pub beam_width: u32,
    /// The three search decisions of §9.4, in one place.
    pub toggles: Toggles,
}

impl SolveConfig {
    /// The shipped behaviour at a Goal and a Beam width.
    pub const fn new(goal: u32, beam_width: u32) -> SolveConfig {
        SolveConfig {
            goal,
            beam_width,
            toggles: Toggles::SHIPPED,
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
    /// Whether the Tail search's frontier cap bound, i.e. whether the last pieces
    /// were searched approximately rather than exactly (§5.3). Native only: the
    /// packed result's reserved bits stay zero (§4.2), so the ABI does not carry
    /// it.
    pub tail_truncated: bool,
}

impl SolveResult {
    fn failed(status: Status) -> SolveResult {
        SolveResult {
            status,
            won: false,
            lines: 0,
            evaluations: 0,
            moves: Vec::new(),
            tail_truncated: false,
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

/// What a Solve has counted so far and the Tail inherits, so one Solve has one
/// Evaluations count across both paths (§4.2) and one Lines-cleared high-water
/// mark for a lost game.
#[derive(Clone, Copy, Default)]
struct Tally {
    evaluations: u32,
    best_lines: u32,
}

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
            tail_truncated: false,
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
        if cfg.toggles.tail && sequence.len() - layer <= TAIL {
            return tail_search(
                &beam,
                &arena,
                sequence,
                layer,
                cfg,
                evaluator,
                Tally {
                    evaluations,
                    best_lines,
                },
            );
        }

        let next_piece = sequence.get(layer + 1).copied();
        let rotations = piece.rotations();
        let mut cands: Vec<Cand> = Vec::with_capacity(beam.len());
        let mut dedup_index = if cfg.toggles.dedup {
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
                            tail_truncated: false,
                        };
                    }

                    // Game over is spawn-blocked at row 0 (§5.3).
                    let suicide = cfg.toggles.no_suicide
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
        if any_safe && cfg.toggles.no_suicide {
            cands.retain(|cand| !cand.suicide);
        }
        if cands.is_empty() {
            return SolveResult {
                status: Status::Ok,
                won: false,
                lines: best_lines,
                evaluations,
                moves: Vec::new(),
                tail_truncated: false,
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
        tail_truncated: false,
    }
}

/// The exact Tail search of §5.3, entered at the first of the final [`TAIL`]
/// pieces with the states the beam kept after piece `layer - 1`.
///
/// Every surviving state is expanded by every legal placement of the next piece,
/// and those placements are expanded in turn, layer by layer, with no trim of a
/// state's placements: the first combination that reaches the Goal wins. "First"
/// is decidable because the Tail continues the beam's order rather than starting a
/// depth-first walk — Layers in order, then frontier rank, then rotation, then
/// column — so a win one piece from the end beats a win three pieces from the end,
/// and ties still break by insertion order.
///
/// Two prunings make it affordable, and the ticket's notes measure both:
///
/// * an **admissible Lines-to-go bound** per Tail state ([`lines_bound`]). A state
///   whose board cannot still reach the Goal with the pieces it has left is
///   skipped, and that is the difference between a hopeless Tail costing
///   microseconds and it costing the whole `beam x 34 x 34 x 34` expansion. It is
///   admissible, so it never costs a win;
/// * a **cap on a Tail layer's frontier** ([`TAIL_STATES_MAX`]), so a wide beam
///   cannot allocate a frontier bigger than the heap. It binds only where the
///   bound could not prove hopelessness and the search is genuinely wide, and it
///   is reported ([`SolveResult::tail_truncated`]) rather than quietly pretending
///   the search was exact.
fn tail_search(
    beam: &[State],
    arena: &[Node],
    sequence: &[Piece],
    layer: usize,
    cfg: &SolveConfig,
    evaluator: &dyn Evaluator,
    tally: Tally,
) -> SolveResult {
    let Tally {
        mut evaluations,
        mut best_lines,
    } = tally;
    let pieces_left = sequence.len() - layer;
    debug_assert!((1..=TAIL).contains(&pieces_left));

    // The entry set is the beam itself: in it, `parent` is a beam index, and a
    // Tail node's parent is an index into the layer before it.
    let mut entry: Vec<Cand> = Vec::with_capacity(beam.len());
    for (rank, state) in beam.iter().enumerate() {
        entry.push(Cand {
            board: state.board,
            lines: state.lines,
            parent: rank as u32,
            score: 0,
            mv: Move::default(),
            suicide: false,
        });
    }

    // One layer's worth of Tail candidates, kept only so a win can be walked back
    // to the position it started from.
    let mut layers: Vec<Vec<Cand>> = Vec::with_capacity(pieces_left);
    let mut truncated = false;

    for ply in 0..pieces_left {
        let piece = sequence[layer + ply];
        let next_piece = sequence.get(layer + ply + 1).copied();
        let rotations = piece.rotations();
        let remaining = pieces_left - ply;

        let mut cands: Vec<Cand> = Vec::new();
        let mut dedup_index = if cfg.toggles.dedup {
            Some(BoardIndex::with_capacity(
                beam.len()
                    .saturating_mul(MAX_PLACEMENTS_PER_PIECE)
                    .min(TAIL_STATES_MAX),
            ))
        } else {
            None
        };
        let mut any_safe = false;

        {
            let previous: &[Cand] = if ply == 0 { &entry } else { &layers[ply - 1] };
            for (rank, state) in previous.iter().enumerate() {
                // The bound: with `remaining` pieces left, this board cannot clear
                // the Lines the Goal still needs, so neither can any placement of
                // it. Skipping the state skips its whole subtree.
                if state.lines + lines_bound(&state.board, remaining as u32) < cfg.goal {
                    continue;
                }
                for (rotation_index, rotation) in rotations.iter().enumerate() {
                    for column in 0..=(W - rotation.width as usize) {
                        let Some(shift) = state.board.rest_shift(rotation, column) else {
                            continue; // no legal spawn position: not an evaluation
                        };
                        evaluations = evaluations.saturating_add(1);

                        let mut next_board = state.board;
                        next_board.place(rotation, column, shift);
                        // Same context the beam path builds: one definition of the
                        // placement, because E1 scores both.
                        let completed = next_board.full_rows();
                        let cleared = next_board.clear_full_rows();
                        let lines = state.lines + cleared;
                        if lines > best_lines {
                            best_lines = lines;
                        }
                        let mv = Move {
                            piece_index: piece.index(),
                            rotation: rotation_index as u8,
                            column: column as u8,
                        };

                        // The first combination that reaches the Goal wins.
                        if lines >= cfg.goal {
                            let root = tail_root(&layers, ply, rank);
                            let mut whole = reconstruct(arena, beam[root].node);
                            whole.extend(tail_moves(&layers, ply, rank));
                            whole.push(mv);
                            return SolveResult {
                                status: Status::Ok,
                                won: true,
                                lines,
                                evaluations,
                                moves: whole,
                                tail_truncated: truncated,
                            };
                        }

                        let suicide = cfg.toggles.no_suicide
                            && next_piece.is_some_and(|next| !next_board.can_place_anywhere(next));
                        if !suicide {
                            any_safe = true;
                        }
                        let cand = Cand {
                            board: next_board,
                            lines,
                            parent: rank as u32,
                            score: evaluator.score(
                                &next_board,
                                &PlacementCtx {
                                    lines_cleared: cleared,
                                    landing_height: rotation.landing_height(shift),
                                    eroded_cells: cleared
                                        * rotation.cells_in_rows(shift, completed),
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
                // Cap the frontier as it is built, not after it is built, so a
                // wide beam cannot allocate a layer bigger than the heap. The
                // slack is what keeps the cap affordable: trimming on every
                // placement past the cap would re-sort and re-index a full layer
                // once per state, which turns a wide hopeless Tail from
                // milliseconds into minutes.
                if cands.len() > TAIL_STATES_MAX + TAIL_TRIM_SLACK {
                    trim_frontier(&mut cands, &mut dedup_index);
                    truncated = true;
                }
            }
        }

        // §5.3's No-suicide rule holds in the Tail exactly as in the beam:
        // game-ending placements survive only a layer in which every placement
        // ends the game.
        if any_safe && cfg.toggles.no_suicide {
            cands.retain(|cand| !cand.suicide);
        }
        if cands.is_empty() {
            return SolveResult {
                status: Status::Ok,
                won: false,
                lines: best_lines,
                evaluations,
                moves: Vec::new(),
                tail_truncated: truncated,
            };
        }
        layers.push(cands);
    }

    SolveResult {
        status: Status::Ok,
        won: false,
        lines: best_lines,
        evaluations,
        moves: Vec::new(),
        tail_truncated: truncated,
    }
}

/// An admissible upper bound on the Lines `board` can still clear with `pieces`
/// pieces left, counting cells rather than playing them out.
///
/// The row at height index `k` needs a cell in every column whose height is at
/// most `k`, and one of them must be filled before that row can clear. Pieces add
/// four cells each, no cell ever changes column, and the rows' costs are
/// non-decreasing in `k`, so spending a `4 * pieces` budget on the cheapest rows in
/// order counts **at least** the Lines that are actually reachable.
///
/// Sound rather than tight on purpose: a board with holes above a row counts those
/// rows as reachable, which can only over-state the bound, and an over-stated bound
/// costs work but never a win.
fn lines_bound(board: &Board, pieces: u32) -> u32 {
    let mut by_height = [0u32; H + 1];
    for column in 0..W {
        by_height[board.height(column) as usize] += 1;
    }
    let mut budget = 4 * pieces;
    let mut short = 0u32; // columns whose height is at most the current index
    let mut lines = 0u32;
    for columns_with_height in by_height.iter().take(H) {
        short += columns_with_height;
        if short > budget {
            break; // every row above costs at least as much as this one
        }
        budget -= short;
        lines += 1;
    }
    lines
}

/// Keep at most [`TAIL_STATES_MAX`] candidates, on the same stable score key the
/// beam trims with, and rebuild the Dedup index over the survivors.
///
/// Trimming is where the Tail stops being exact, which is why it is counted
/// (`tail_truncated`) instead of hidden.
fn trim_frontier(cands: &mut Vec<Cand>, dedup_index: &mut Option<BoardIndex>) {
    cands.sort_by_key(|cand| std::cmp::Reverse(cand.score));
    cands.truncate(TAIL_STATES_MAX);
    if dedup_index.is_some() {
        let mut rebuilt = BoardIndex::with_capacity(cands.len());
        for position in 0..cands.len() {
            rebuilt.insert(cands, position);
        }
        *dedup_index = Some(rebuilt);
    }
}

/// The Tail Moves that reach the node at `rank` of Tail level `ply`, oldest first.
/// Level zero is the entry set, which the beam's own Move list already reaches.
fn tail_moves(layers: &[Vec<Cand>], ply: usize, rank: usize) -> Vec<Move> {
    let mut moves = Vec::with_capacity(ply);
    let mut level = ply;
    let mut cursor = rank;
    while level > 0 {
        let node = &layers[level - 1][cursor];
        moves.push(node.mv);
        cursor = node.parent as usize;
        level -= 1;
    }
    moves.reverse();
    moves
}

/// The entry-set index the node at `rank` of Tail level `ply` hangs off: which
/// beam state the winning combination started from.
fn tail_root(layers: &[Vec<Cand>], ply: usize, rank: usize) -> usize {
    let mut level = ply;
    let mut cursor = rank;
    while level > 0 {
        cursor = layers[level - 1][cursor].parent as usize;
        level -= 1;
    }
    cursor
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
