//! Seam 1, the Board and the placement table: §5.1 and §5.2 are normative and
//! every other layer (generator, beam, replay) is defined in terms of them.

use engine::board::{Board, H, W};
use engine::pieces::Piece;

#[test]
fn bit_n_of_column_c_is_row_19_minus_n() {
    // §5.1: bit 0 is the bottom row (row 19), bit 19 is the top row (row 0).
    let mut b = Board::EMPTY;
    b.set(0, H - 1);
    assert_eq!(b.0[0], 1, "the bottom row is bit 0");
    assert!(b.filled(0, H - 1));
    assert!(!b.filled(0, H - 2));

    let mut top = Board::EMPTY;
    top.set(W - 1, 0);
    assert_eq!(top.0[W - 1], 1 << (H - 1), "row 0 is the highest bit");
    assert!(top.filled(W - 1, 0));
}

#[test]
fn height_and_full_rows_read_the_column_words() {
    let mut b = Board::EMPTY;
    assert_eq!(b.height(3), 0);
    b.set(3, H - 1);
    b.set(3, H - 2);
    assert_eq!(b.height(3), 2);

    assert_eq!(b.full_rows(), 0);
    for c in 0..W {
        b.set(c, H - 1);
    }
    assert_eq!(b.full_rows(), 1, "row 19 is full");
    assert_eq!(b.full_rows().count_ones(), 1);
}

#[test]
fn line_clears_run_bottom_up_with_the_stack_above_shifted_down() {
    // Rows 18 and 19 both full, with a marker at row 17 in column 0 and a
    // marker higher up in column 9. Two clears must drop both markers by two.
    let mut b = Board::EMPTY;
    for c in 0..W {
        b.set(c, H - 1);
        b.set(c, H - 2);
    }
    b.set(0, H - 3);
    b.set(9, H - 6);

    assert_eq!(b.clear_full_rows(), 2);
    assert_eq!(b.full_rows(), 0);
    assert!(b.filled(0, H - 1), "the marker fell to the bottom row");
    assert!(!b.filled(0, H - 3), "and left its old row");
    assert!(b.filled(9, H - 4), "a higher marker fell two rows too");
    assert_eq!(b.0[9].count_ones(), 1);
    assert_eq!(b.0[0].count_ones(), 1);
}

#[test]
fn a_gapped_row_does_not_clear() {
    let mut b = Board::EMPTY;
    for c in 0..W - 1 {
        b.set(c, H - 1);
    }
    assert_eq!(b.full_rows(), 0);
    assert_eq!(b.clear_full_rows(), 0);
    assert_eq!(b.height(0), 1, "nothing moved");
}

#[test]
fn clearing_twice_is_a_no_op_the_second_time() {
    let mut b = Board::EMPTY;
    for c in 0..W {
        b.set(c, H - 1);
    }
    assert_eq!(b.clear_full_rows(), 1);
    assert_eq!(b.clear_full_rows(), 0);
    assert_eq!(b, Board::EMPTY);
}

#[test]
fn the_placement_table_matches_section_5_2() {
    // §5.2 is normative: rotation order is contract because ties break by
    // insertion order.
    let wanted: [(Piece, &[u8]); 7] = [
        (Piece::I, &[4, 1]),
        (Piece::J, &[3, 2, 3, 2]),
        (Piece::L, &[3, 2, 3, 2]),
        (Piece::O, &[2]),
        (Piece::S, &[3, 2]),
        (Piece::T, &[3, 2, 3, 2]),
        (Piece::Z, &[3, 2]),
    ];
    for (piece, widths) in wanted {
        let rots = piece.rotations();
        assert_eq!(rots.len(), widths.len(), "{piece:?} rotation count");
        for (i, w) in widths.iter().enumerate() {
            assert_eq!(rots[i].width, *w, "{piece:?} rotation {i} width");
            let cells: u32 = rots[i].cols.iter().map(|c| c.count_ones()).sum();
            assert_eq!(cells, 4, "{piece:?} rotation {i} must have four cells");
        }
    }
}

#[test]
fn piece_indices_are_the_abi_piece_index() {
    // §4.1.1: piece_index indexes I, J, L, O, S, T, Z.
    let order = [
        Piece::I,
        Piece::J,
        Piece::L,
        Piece::O,
        Piece::S,
        Piece::T,
        Piece::Z,
    ];
    assert_eq!(&b"IJLOSTZ"[..], &order.map(|p| p.letter())[..]);
    for (i, p) in order.iter().enumerate() {
        assert_eq!(p.index(), i as u8);
        assert_eq!(Piece::from_letter(p.letter()), Some(*p));
    }
    assert_eq!(Piece::from_letter(b'X'), None);
    assert_eq!(Piece::from_letter(b'i'), None, "letters are upper case");
}

#[test]
fn a_board_with_bits_above_row_0_is_illegal() {
    assert!(Board::EMPTY.is_legal());
    let mut b = Board::EMPTY;
    b.0[4] = 1 << 20; // 21 rows' worth of bits: above the field
    assert!(!b.is_legal());
}
