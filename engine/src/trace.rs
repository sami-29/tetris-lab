//! The trace: the survive-set of each Layer, as §4.4 carries it (§2.3's band).
//!
//! `solve` never writes one — that is `solve_traced`, ticket 05's export. The
//! type lives here now because §4.4's record layout is normative and the engine
//! is where it is produced and tested, not the wasm crate.

/// One recorded state: which Layer, its rank inside that Layer, its score, the
/// Lines cleared to reach it, and its parent's rank in the previous Layer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TraceRecord {
    pub layer: u16,
    /// 0-based within the Layer; 0 is the best score.
    pub rank: u16,
    pub score: i32,
    pub lines: u16,
    /// The parent's rank in the previous Layer; -1 at Layer 0.
    pub parent_rank: i32,
}

/// §4.4: records in layer order, then rank order.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Trace {
    pub records: Vec<TraceRecord>,
}

impl Trace {
    pub fn new() -> Trace {
        Trace {
            records: Vec::new(),
        }
    }

    /// Reserve room for `records` records: §4.4 allocates `band * pieces` of them.
    pub fn with_capacity(records: usize) -> Trace {
        Trace {
            records: Vec::with_capacity(records),
        }
    }

    #[inline]
    pub fn push(&mut self, record: TraceRecord) {
        self.records.push(record);
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
