//! The Board: twenty rows by ten columns, held as ten `u32` column words.
//!
//! Bit *n* of column word *c* is row `H-1-n`, so bit 0 is the bottom row and row
//! 0 is the top row (§5.1). A set bit is a filled cell. The whole Board is 40
//! bytes, which is why it is also the ABI's board buffer (§4.1).

use crate::pieces::Rotation;

pub const W: usize = 10;
pub const H: usize = 20;

/// Every row of the field: twenty bits. Anything above that is not a Board.
const FIELD: u32 = (1 << H) - 1;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Board(pub [u32; W]);

impl Board {
    pub const EMPTY: Board = Board([0u32; W]);

    #[inline]
    pub fn filled(&self, col: usize, row: usize) -> bool {
        self.0[col] & (1u32 << (H - 1 - row)) != 0
    }

    #[inline]
    pub fn set(&mut self, col: usize, row: usize) {
        self.0[col] |= 1u32 << (H - 1 - row);
    }

    /// Stack height of one column in cells: zero when the column is empty.
    #[inline]
    pub fn height(&self, col: usize) -> u32 {
        u32::BITS - self.0[col].leading_zeros()
    }

    /// The tallest column's height: what the height cap is measured against.
    pub fn tallest(&self) -> u32 {
        let mut tallest = 0;
        for c in 0..W {
            let h = self.height(c);
            if h > tallest {
                tallest = h;
            }
        }
        tallest
    }

    /// Rows where every column is filled, as a mask over bit positions.
    #[inline]
    pub fn full_rows(&self) -> u32 {
        let mut mask = self.0[0];
        let mut c = 1;
        while c < W {
            mask &= self.0[c];
            c += 1;
        }
        mask
    }

    /// Clear every full row, bottom-up, shifting the stack above each one down.
    /// Returns the number of Lines cleared by this call.
    pub fn clear_full_rows(&mut self) -> u32 {
        let full = self.full_rows();
        if full == 0 {
            return 0;
        }
        for word in self.0.iter_mut() {
            let mut w = *word;
            let mut bit = H as u32;
            while bit > 0 {
                bit -= 1;
                if full & (1 << bit) != 0 {
                    // Everything below the row stays, everything above drops one.
                    let lower = w & ((1u32 << bit) - 1);
                    let upper = (w >> (bit + 1)) << bit;
                    w = lower | upper;
                }
            }
            *word = w;
        }
        full.count_ones()
    }

    /// A Board may not carry bits above row 0; status 3 covers the ones that do.
    #[inline]
    pub fn is_legal(&self) -> bool {
        let mut c = 0;
        while c < W {
            if self.0[c] & !FIELD != 0 {
                return false;
            }
            c += 1;
        }
        true
    }

    /// How far a rotation must be shifted up from the floor to rest at column
    /// `col`, or `None` when it cannot enter at row 0 (§5.3's game-over rule) or
    /// does not fit horizontally.
    ///
    /// The same primitive serves the Generator's pre-fill and the beam's
    /// expansion, which is what makes a generated Board legal by the solver's
    /// own definition (§5.5).
    #[inline]
    pub fn rest_shift(&self, rot: &Rotation, col: usize) -> Option<u32> {
        let width = rot.width as usize;
        if col + width > W {
            return None;
        }
        let mut shift: i32 = 0;
        for i in 0..width {
            // The column's part of the piece starts `trailing_zeros` above the floor.
            let lowest = rot.cols[i].trailing_zeros() as i32;
            let needed = self.height(col + i) as i32 - lowest;
            if needed > shift {
                shift = needed;
            }
        }
        if shift as u32 > H as u32 - rot.height as u32 {
            return None; // no legal spawn position at row 0
        }
        Some(shift as u32)
    }

    /// Stamp a rotation resting at `shift` onto the Board. Legality is the
    /// caller's: it comes from `rest_shift`.
    #[inline]
    pub fn place(&mut self, rot: &Rotation, col: usize, shift: u32) {
        for i in 0..rot.width as usize {
            self.0[col + i] |= (rot.cols[i] as u32) << shift;
        }
    }

    /// `rest_shift` and `place` and `clear_full_rows` in one call: place the
    /// rotation, let it rest, clear what completed. Returns the Lines cleared,
    /// or `None` when the placement is not legal.
    #[inline]
    pub fn apply(&mut self, rot: &Rotation, col: usize) -> Option<u32> {
        let shift = self.rest_shift(rot, col)?;
        self.place(rot, col, shift);
        Some(self.clear_full_rows())
    }

    /// Whether the piece has any legal placement at all: the "top out" test is
    /// spawn-blocked at row 0 (§5.3).
    pub fn can_place_anywhere(&self, piece: crate::pieces::Piece) -> bool {
        for rot in piece.rotations() {
            for col in 0..=(W - rot.width as usize) {
                if self.rest_shift(rot, col).is_some() {
                    return true;
                }
            }
        }
        false
    }
}
