//! The trace: the survive-set of each Layer, as §4.4 carries it (§2.3's band).
//!
//! `solve` never writes one — that is `solve_traced`. The type lives here because
//! §4.4's record layout is normative and the engine is where it is produced and
//! tested, not the wasm crate.
//!
//! Two conventions this module owns, both of them tick 11's decisions rather than
//! the spec's words (recorded in the ticket's report):
//!
//! * **Layer 0 is the start position**, one record, `parent_rank == -1` (§4.4 is
//!   satisfiable by either reading, but only this one gives the lineage an
//!   anchor and matches every other artifact that draws this trace);
//! * **`PARENT_UNRECORDED` (`-2`)** means "this state's parent exists in the
//!   previous Layer but was not kept, because the band is narrower than that
//!   Layer's survive-set". It cannot occur when `band >= beam_width`, which is
//!   what the app passes; it exists so a narrower band degrades into a *named*
//!   missing edge instead of a dangling pointer. `-1` stays §4.4's "no parent".

/// §4.4: `parent_rank` at Layer 0. The start position was not placed.
pub const PARENT_NONE: i32 = -1;

/// The parent exists in the previous Layer but the band cut its record.
pub const PARENT_UNRECORDED: i32 = -2;

/// One recorded state: which Layer, its rank inside that Layer, its score, the
/// Lines cleared to reach it, and its parent's rank in the previous Layer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TraceRecord {
    pub layer: u16,
    /// 0-based within the Layer; 0 is the best score.
    pub rank: u16,
    pub score: i32,
    pub lines: u16,
    /// The parent's rank in the previous Layer; [`PARENT_NONE`] at Layer 0 and
    /// [`PARENT_UNRECORDED`] when the band cut it.
    pub parent_rank: i32,
}

/// §4.4: records in layer order, then rank order.
///
/// `capacity` is a hard ceiling on the record count, not just a reservation: the
/// wasm's trace region is a fixed `alloc` at boot, so a solve must be able to
/// fill it without ever writing past it. A layer that does not fit is *not*
/// recorded at all — the trace then ends at the last whole layer, which keeps
/// every `parent_rank` resolvable. `Trace::new()` is unbounded, which is what the
/// native tests and the bench want.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Trace {
    pub records: Vec<TraceRecord>,
    capacity: usize,
}

impl Trace {
    /// Unbounded: every layer is recorded. For native callers.
    pub fn new() -> Trace {
        Trace {
            records: Vec::new(),
            capacity: usize::MAX,
        }
    }

    /// At most `records` records (§4.4's `band x pieces`, i.e. the wasm's region).
    pub fn with_capacity(records: usize) -> Trace {
        Trace {
            records: Vec::with_capacity(records),
            capacity: records,
        }
    }

    /// The most records this trace will ever hold.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// How many records still fit.
    #[inline]
    pub fn remaining(&self) -> usize {
        self.capacity.saturating_sub(self.records.len())
    }

    #[inline]
    pub fn push(&mut self, record: TraceRecord) {
        if self.records.len() < self.capacity {
            self.records.push(record);
        }
    }

    #[inline]
    pub fn clear(&mut self) {
        self.records.clear();
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// The Layers present, as `(first record index, count)`, in Layer order.
    pub fn layers(&self) -> Vec<(usize, usize)> {
        let mut layers: Vec<(usize, usize)> = Vec::new();
        for (index, record) in self.records.iter().enumerate() {
            match layers.last_mut() {
                Some(last) if self.records[last.0].layer == record.layer => last.1 += 1,
                _ => layers.push((index, 1)),
            }
        }
        layers
    }

    /// §4.4's buffer words: a count, then five `i32` per record.
    pub fn words(&self) -> Vec<i32> {
        let mut words = Vec::with_capacity(1 + self.records.len() * 5);
        words.push(self.records.len() as i32);
        for record in &self.records {
            words.push(record.layer as i32);
            words.push(record.rank as i32);
            words.push(record.score);
            words.push(record.lines as i32);
            words.push(record.parent_rank);
        }
        words
    }
}
