//! The seven tetrominoes and their placement table (§5.2, normative).
//!
//! Rotation lists and widths are pinned from `tetromino_data.py`'s `_RAW_SHAPES`
//! so the Generator, the beam and a replay simulator all agree on what a piece
//! is. Rotation *order* is contract: the beam breaks score ties by insertion
//! order, and insertion order is `(piece, rotation, column)` ascending.

/// The canonical order of §5.2. The discriminant is the ABI's `piece_index`
/// (§4.1.1), so this order is not an implementation detail either.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum Piece {
    I = 0,
    J = 1,
    L = 2,
    O = 3,
    S = 4,
    T = 5,
    Z = 6,
}

pub const PIECES: [Piece; 7] = [
    Piece::I,
    Piece::J,
    Piece::L,
    Piece::O,
    Piece::S,
    Piece::T,
    Piece::Z,
];

/// One rotation of one piece: a box of cells, each column word holding that
/// column's cells as bits with bit 0 at the box's bottom row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rotation {
    pub width: u8,
    pub height: u8,
    pub cols: [u8; 4],
}

/// The cell maps of `_RAW_SHAPES`, transcribed: one string per row, top first.
type Rows = &'static [&'static str];

const SHAPES: &[&[Rows]] = &[
    // I
    &[&["1111"], &["1", "1", "1", "1"]],
    // J
    &[
        &["100", "111"],
        &["11", "10", "10"],
        &["111", "001"],
        &["01", "01", "11"],
    ],
    // L
    &[
        &["001", "111"],
        &["10", "10", "11"],
        &["111", "100"],
        &["11", "01", "01"],
    ],
    // O
    &[&["11", "11"]],
    // S
    &[&["011", "110"], &["10", "11", "01"]],
    // T
    &[
        &["010", "111"],
        &["10", "11", "10"],
        &["111", "010"],
        &["01", "11", "01"],
    ],
    // Z
    &[&["110", "011"], &["01", "11", "10"]],
];

/// Rotation counts, in `Piece` order: §5.2's "rotations" column.
pub const N_ROTATIONS: [usize; 7] = [2, 4, 4, 1, 2, 4, 2];

const fn build_rotations() -> [[Rotation; 4]; 7] {
    // Const-built so the table is data, not lazy allocation: the engine holds no
    // state and nothing here can differ between the native and wasm builds.
    let mut table = [[Rotation {
        width: 0,
        height: 0,
        cols: [0; 4],
    }; 4]; 7];
    let mut piece = 0;
    while piece < 7 {
        let rotations = SHAPES[piece];
        let mut index = 0;
        while index < rotations.len() {
            let rows = rotations[index];
            let height = rows.len() as u8;
            let width = rows[0].len() as u8;
            let mut cols = [0u8; 4];
            let mut y = 0;
            while y < rows.len() {
                let line = rows[y].as_bytes();
                let mut x = 0;
                while x < line.len() {
                    if line[x] == b'1' {
                        cols[x] |= 1u8 << (height as usize - 1 - y);
                    }
                    x += 1;
                }
                y += 1;
            }
            table[piece][index] = Rotation {
                width,
                height,
                cols,
            };
            index += 1;
        }
        piece += 1;
    }
    table
}

static ROTATIONS: [[Rotation; 4]; 7] = build_rotations();

impl Piece {
    /// The ABI's `piece_index` (§4.1.1).
    #[inline]
    pub const fn index(self) -> u8 {
        self as u8
    }

    /// From a `piece_index`; `None` past the seventh piece.
    #[inline]
    pub const fn from_index(index: u8) -> Option<Piece> {
        match index {
            0 => Some(Piece::I),
            1 => Some(Piece::J),
            2 => Some(Piece::L),
            3 => Some(Piece::O),
            4 => Some(Piece::S),
            5 => Some(Piece::T),
            6 => Some(Piece::Z),
            _ => None,
        }
    }

    /// The Sequence letter of §4.1, `I J L O S T Z`.
    #[inline]
    pub const fn letter(self) -> u8 {
        match self {
            Piece::I => b'I',
            Piece::J => b'J',
            Piece::L => b'L',
            Piece::O => b'O',
            Piece::S => b'S',
            Piece::T => b'T',
            Piece::Z => b'Z',
        }
    }

    /// The Sequence letter as a piece; a Sequence byte is validated here.
    #[inline]
    pub const fn from_letter(letter: u8) -> Option<Piece> {
        match letter {
            b'I' => Some(Piece::I),
            b'J' => Some(Piece::J),
            b'L' => Some(Piece::L),
            b'O' => Some(Piece::O),
            b'S' => Some(Piece::S),
            b'T' => Some(Piece::T),
            b'Z' => Some(Piece::Z),
            _ => None,
        }
    }

    /// §5.2's rotation list, in §5.2's order.
    #[inline]
    pub fn rotations(self) -> &'static [Rotation] {
        &ROTATIONS[self.index() as usize][..N_ROTATIONS[self.index() as usize]]
    }
}

impl Rotation {
    /// How many of this rotation's cells land in `rows` when it rests `shift`
    /// cells above the floor. `rows` is a mask over bit positions, bit 0 the
    /// bottom row (§5.1); the column the piece was dropped in does not matter,
    /// because a row bit means the same thing in every column word.
    ///
    /// This is the `b` of `eroded_cells`: the search multiplies it by the Lines
    /// the placement cleared, which is the published product (§5.4).
    #[inline]
    pub fn cells_in_rows(&self, shift: u32, rows: u32) -> u32 {
        let mut cells = 0;
        let mut i = 0;
        while i < self.width as usize {
            cells += ((self.cols[i] as u32) << shift & rows).count_ones();
            i += 1;
        }
        cells
    }

    /// Centre of mass of this rotation's cells above the floor when it rests
    /// `shift` cells above it: `1..=20`, the landing height of §5.4.
    ///
    /// Centre of mass, not the top or the bottom of the piece and not the middle
    /// of its bounding box: the paper's "height where the last piece is added"
    /// does not say which, the readings differ by up to two rows and only for the
    /// corner-footed orientations, and a fixture that pinned one of them by
    /// accident would be a different evaluator. `+1` so a flat landing on the
    /// floor is 1, matching a column height of 1 for a filled bottom row.
    #[inline]
    pub fn landing_height(&self, shift: u32) -> i32 {
        let mut sum = 0;
        let mut cells = 0;
        let mut i = 0;
        while i < self.width as usize {
            let mut word = self.cols[i] as u32;
            while word != 0 {
                let bit = word.trailing_zeros();
                sum += bit + shift + 1;
                cells += 1;
                word &= word - 1;
            }
            i += 1;
        }
        (sum / cells) as i32
    }
}
