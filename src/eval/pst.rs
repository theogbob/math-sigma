// piece-square tables, PeSTO-style mg + eg
// indexed from white's perspective (A1=0, H8=63)
use cozy_chess::{Piece, Color, Square};

use crate::types::Score;

#[rustfmt::skip]
const PAWN_MG: [Score; 64] = [
     0,  0,  0,  0,  0,  0,  0,  0,
     5, 10, 10,-20,-20, 10, 10,  5,
     5, -5,-10,  0,  0,-10, -5,  5,
     0,  0,  0, 20, 20,  0,  0,  0,
     5,  5, 10, 25, 25, 10,  5,  5,
    10, 10, 20, 30, 30, 20, 10, 10,
    50, 50, 50, 50, 50, 50, 50, 50,
     0,  0,  0,  0,  0,  0,  0,  0,
];

#[rustfmt::skip]
const PAWN_EG: [Score; 64] = [
     0,  0,  0,  0,  0,  0,  0,  0,
    10, 10, 10, 10, 10, 10, 10, 10,
    10, 10, 10, 10, 10, 10, 10, 10,
    20, 20, 20, 20, 20, 20, 20, 20,
    30, 30, 30, 30, 30, 30, 30, 30,
    50, 50, 50, 50, 50, 50, 50, 50,
    80, 80, 80, 80, 80, 80, 80, 80,
     0,  0,  0,  0,  0,  0,  0,  0,
];

#[rustfmt::skip]
const KNIGHT_MG: [Score; 64] = [
   -50,-40,-30,-30,-30,-30,-40,-50,
   -40,-20,  0,  5,  5,  0,-20,-40,
   -30,  5, 10, 15, 15, 10,  5,-30,
   -30,  0, 15, 20, 20, 15,  0,-30,
   -30,  5, 15, 20, 20, 15,  5,-30,
   -30,  0, 10, 15, 15, 10,  0,-30,
   -40,-20,  0,  0,  0,  0,-20,-40,
   -50,-40,-30,-30,-30,-30,-40,-50,
];

#[rustfmt::skip]
const KNIGHT_EG: [Score; 64] = [
   -40,-20,-15,-15,-15,-15,-20,-40,
   -20,  0, 10, 10, 10, 10,  0,-20,
   -15, 10, 20, 25, 25, 20, 10,-15,
   -15, 10, 25, 30, 30, 25, 10,-15,
   -15, 10, 25, 30, 30, 25, 10,-15,
   -15, 10, 20, 25, 25, 20, 10,-15,
   -20,  0, 10, 10, 10, 10,  0,-20,
   -40,-20,-15,-15,-15,-15,-20,-40,
];

#[rustfmt::skip]
const BISHOP_MG: [Score; 64] = [
   -20,-10,-10,-10,-10,-10,-10,-20,
   -10,  5,  0,  0,  0,  0,  5,-10,
   -10, 10, 10, 10, 10, 10, 10,-10,
   -10,  0, 10, 10, 10, 10,  0,-10,
   -10,  5,  5, 10, 10,  5,  5,-10,
   -10,  0,  5, 10, 10,  5,  0,-10,
   -10,  0,  0,  0,  0,  0,  0,-10,
   -20,-10,-10,-10,-10,-10,-10,-20,
];

#[rustfmt::skip]
const BISHOP_EG: [Score; 64] = [
   -20,-10,-10,-10,-10,-10,-10,-20,
   -10,  0,  0,  0,  0,  0,  0,-10,
   -10,  0, 10, 10, 10, 10,  0,-10,
   -10,  0, 10, 10, 10, 10,  0,-10,
   -10,  0, 10, 10, 10, 10,  0,-10,
   -10,  0, 10, 10, 10, 10,  0,-10,
   -10,  0,  0,  0,  0,  0,  0,-10,
   -20,-10,-10,-10,-10,-10,-10,-20,
];

#[rustfmt::skip]
const ROOK_MG: [Score; 64] = [
     0,  0,  0,  5,  5,  0,  0,  0,
    -5,  0,  0,  0,  0,  0,  0, -5,
    -5,  0,  0,  0,  0,  0,  0, -5,
    -5,  0,  0,  0,  0,  0,  0, -5,
    -5,  0,  0,  0,  0,  0,  0, -5,
    -5,  0,  0,  0,  0,  0,  0, -5,
     5, 10, 10, 10, 10, 10, 10,  5,
     0,  0,  0,  0,  0,  0,  0,  0,
];

#[rustfmt::skip]
const ROOK_EG: [Score; 64] = [
     0,  0,  0,  0,  0,  0,  0,  0,
     0,  0,  0,  0,  0,  0,  0,  0,
     0,  0,  0,  0,  0,  0,  0,  0,
     5,  5,  5,  5,  5,  5,  5,  5,
     5,  5,  5,  5,  5,  5,  5,  5,
    10, 10, 10, 10, 10, 10, 10, 10,
    20, 20, 20, 20, 20, 20, 20, 20,
    10, 10, 10, 10, 10, 10, 10, 10,
];

#[rustfmt::skip]
const QUEEN_MG: [Score; 64] = [
   -20,-10,-10, -5, -5,-10,-10,-20,
   -10,  0,  5,  0,  0,  0,  0,-10,
   -10,  5,  5,  5,  5,  5,  0,-10,
     0,  0,  5,  5,  5,  5,  0, -5,
    -5,  0,  5,  5,  5,  5,  0, -5,
   -10,  0,  5,  5,  5,  5,  0,-10,
   -10,  0,  0,  0,  0,  0,  0,-10,
   -20,-10,-10, -5, -5,-10,-10,-20,
];

#[rustfmt::skip]
const QUEEN_EG: [Score; 64] = [
   -20,-10,-10, -5, -5,-10,-10,-20,
   -10,  0,  0,  0,  0,  0,  0,-10,
   -10,  0,  5,  5,  5,  5,  0,-10,
    -5,  0,  5, 10, 10,  5,  0, -5,
    -5,  0,  5, 10, 10,  5,  0, -5,
   -10,  0,  5,  5,  5,  5,  0,-10,
   -10,  0,  0,  0,  0,  0,  0,-10,
   -20,-10,-10, -5, -5,-10,-10,-20,
];

#[rustfmt::skip]
const KING_MG: [Score; 64] = [
    20, 30, 10,  0,  0, 10, 30, 20,
    20, 20,  0,  0,  0,  0, 20, 20,
   -10,-20,-20,-20,-20,-20,-20,-10,
   -20,-30,-30,-40,-40,-30,-30,-20,
   -30,-40,-40,-50,-50,-40,-40,-30,
   -30,-40,-40,-50,-50,-40,-40,-30,
   -30,-40,-40,-50,-50,-40,-40,-30,
   -30,-40,-40,-50,-50,-40,-40,-30,
];

#[rustfmt::skip]
const KING_EG: [Score; 64] = [
   -50,-30,-30,-30,-30,-30,-30,-50,
   -30,-20,-10,  0,  0,-10,-20,-30,
   -30,-10, 20, 30, 30, 20,-10,-30,
   -30,-10, 30, 40, 40, 30,-10,-30,
   -30,-10, 30, 40, 40, 30,-10,-30,
   -30,-10, 20, 30, 30, 20,-10,-30,
   -30,-30,  0,  0,  0,  0,-30,-30,
   -50,-30,-30,-30,-30,-30,-30,-50,
];

#[inline(always)]
fn pst_index(sq: Square, color: Color) -> usize {
    let idx = sq as usize;
    if color == Color::White { idx } else { idx ^ 56 }
}

#[inline]
pub fn pst_mg(piece: Piece, sq: Square, color: Color) -> Score {
    let idx = pst_index(sq, color);
    match piece {
        Piece::Pawn => PAWN_MG[idx],
        Piece::Knight => KNIGHT_MG[idx],
        Piece::Bishop => BISHOP_MG[idx],
        Piece::Rook => ROOK_MG[idx],
        Piece::Queen => QUEEN_MG[idx],
        Piece::King => KING_MG[idx],
    }
}

#[inline]
pub fn pst_eg(piece: Piece, sq: Square, color: Color) -> Score {
    let idx = pst_index(sq, color);
    match piece {
        Piece::Pawn => PAWN_EG[idx],
        Piece::Knight => KNIGHT_EG[idx],
        Piece::Bishop => BISHOP_EG[idx],
        Piece::Rook => ROOK_EG[idx],
        Piece::Queen => QUEEN_EG[idx],
        Piece::King => KING_EG[idx],
    }
}

// caller already computed the mirrored index
#[inline(always)]
pub fn pst_mg_idx(piece: Piece, idx: usize) -> Score {
    match piece {
        Piece::Pawn => PAWN_MG[idx],
        Piece::Knight => KNIGHT_MG[idx],
        Piece::Bishop => BISHOP_MG[idx],
        Piece::Rook => ROOK_MG[idx],
        Piece::Queen => QUEEN_MG[idx],
        Piece::King => KING_MG[idx],
    }
}

#[inline(always)]
pub fn pst_eg_idx(piece: Piece, idx: usize) -> Score {
    match piece {
        Piece::Pawn => PAWN_EG[idx],
        Piece::Knight => KNIGHT_EG[idx],
        Piece::Bishop => BISHOP_EG[idx],
        Piece::Rook => ROOK_EG[idx],
        Piece::Queen => QUEEN_EG[idx],
        Piece::King => KING_EG[idx],
    }
}
