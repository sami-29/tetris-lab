//! Piece tables for the ticket 06 spike.
//!
//! The shape literals below are copied verbatim from the repo's
//! `tetromino_data.py` (same pieces, same rotation order) so the search visits
//! placements in exactly the same order as the numpy solver. Rotation order
//! matters: beam ties are broken by insertion order.
//!
//! Column words use bit 0 = bottom of the piece's bounding box.

pub const PIECE_NAMES: &[u8; 7] = b"IJLOSTZ";

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

/// One rotation: the piece's width in columns, its box height in rows, and the
/// vertical bit pattern of each column (bit 0 = bottom row of the box).
#[derive(Clone, Copy, Debug)]
pub struct Rot {
    pub width: u32,
    pub height: u32,
    pub cols: [u8; 4],
}

pub fn rot_table() -> Vec<Vec<Rot>> {
    let mut out = Vec::with_capacity(7);
    for rotations in SHAPES {
        let mut rots = Vec::with_capacity(rotations.len());
        for rows in *rotations {
            let height = rows.len() as u32;
            let width = rows[0].len() as u32;
            let mut cols = [0u8; 4];
            for (r, line) in rows.iter().enumerate() {
                assert_eq!(line.len() as u32, width, "ragged shape literal");
                for (c, ch) in line.bytes().enumerate() {
                    if ch == b'1' {
                        // Box row r counting from the top -> bit (height-1-r).
                        cols[c] |= 1 << (height as usize - 1 - r);
                    }
                }
            }
            rots.push(Rot {
                width,
                height,
                cols,
            });
        }
        out.push(rots);
    }
    out
}

/// Piece index from the sequence letter used in the games file.
pub fn piece_index(name: u8) -> u8 {
    PIECE_NAMES
        .iter()
        .position(|&p| p == name)
        .unwrap_or_else(|| panic!("unknown piece '{}'", name as char)) as u8
}
