pub type Score = i32;

pub const MATE_SCORE: Score = 99_999;
pub const DRAW_SCORE: Score = 0;
pub const INF_SCORE: Score = 999_999;

pub const MATE_THRESHOLD: Score = MATE_SCORE - MAX_PLY as Score;

// mate scores: ply-relative to root-relative for TT storage
#[inline]
pub fn score_to_tt(score: Score, ply: usize) -> Score {
    if score > MATE_THRESHOLD {
        score + ply as Score
    } else if score < -MATE_THRESHOLD {
        score - ply as Score
    } else {
        score
    }
}

// reverse: root-relative back to ply-relative
#[inline]
pub fn score_from_tt(score: Score, ply: usize) -> Score {
    if score > MATE_THRESHOLD {
        score - ply as Score
    } else if score < -MATE_THRESHOLD {
        score + ply as Score
    } else {
        score
    }
}

pub const MAX_PLY: usize = 128;

// piece values (centipawns)
pub const PAWN_VALUE: Score = 100;
pub const KNIGHT_VALUE: Score = 320;
pub const BISHOP_VALUE: Score = 330;
pub const ROOK_VALUE: Score = 500;
pub const QUEEN_VALUE: Score = 900;

pub fn piece_value(piece: cozy_chess::Piece) -> Score {
    match piece {
        cozy_chess::Piece::Pawn => PAWN_VALUE,
        cozy_chess::Piece::Knight => KNIGHT_VALUE,
        cozy_chess::Piece::Bishop => BISHOP_VALUE,
        cozy_chess::Piece::Rook => ROOK_VALUE,
        cozy_chess::Piece::Queen => QUEEN_VALUE,
        cozy_chess::Piece::King => 0,
    }
}

// phase weights for mg/eg interpolation
pub fn phase_weight(piece: cozy_chess::Piece) -> i32 {
    match piece {
        cozy_chess::Piece::Pawn => 0,
        cozy_chess::Piece::Knight => 1,
        cozy_chess::Piece::Bishop => 1,
        cozy_chess::Piece::Rook => 2,
        cozy_chess::Piece::Queen => 4,
        cozy_chess::Piece::King => 0,
    }
}

pub const TOTAL_PHASE: i32 = 24;

// derived constants from chess branching factor model:
//   b = 35 (average branching factor)
//   b_eff ~= 7.4 (effective BF after move ordering)
//   T = P * ln(b) / (1 + ln(b)) (tempo value)

pub const BRANCHING_FACTOR: f64 = 35.0;
pub const LN_B: f64 = 3.5553; // ln(35)
pub const B_EFF: f64 = 7.4; // b / (1 + n_priority_moves)
pub const LN_BEFF: f64 = 2.0015; // ln(b_eff)
pub const TEMPO: Score = 78; // 100 * ln(35) / (1 + ln(35))
pub const V_AVG: Score = (PAWN_VALUE + KNIGHT_VALUE + BISHOP_VALUE + ROOK_VALUE + QUEEN_VALUE) / 5;
pub const P_CAPTURE: f64 = 5.0 / BRANCHING_FACTOR;
pub const V_CAP_REALIZED: Score = (PAWN_VALUE + V_AVG) / 2; // avg SEE-positive capture
pub const SIGMA_LEAF: f64 = 50.0; // leaf eval noise, ~half a pawn
pub const LMR_COEFF: f64 = 1.0 / LN_BEFF; // ~0.50
pub const ASP_WINDOW: Score = 30; // 1.65 * sigma / sqrt(b_eff)
pub const PROBCUT_MARGIN: Score = 100; // 97.5% shallow-vs-deep confidence bound
pub const RFP_IMPROVING: Score = 115; // reverse futility (improving)
pub const RFP_NOT_IMPROVING: Score = 97; // reverse futility (not improving)
pub const FUT_IMPROVING: Score = 150; // futility margin (improving)
pub const FUT_NOT_IMPROVING: Score = 120; // futility margin (not improving)

// convert cozy-chess 960 notation (e1h1) to standard uci (e1g1)
pub fn move_to_uci(mv: cozy_chess::Move, board: &cozy_chess::Board) -> String {
    cozy_chess::util::display_uci_move(board, mv).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cozy_chess::*;

    #[test]
    fn mate_score_tt_roundtrip() {
        let ply = 3usize;
        let score = -(MATE_SCORE - 8);
        let stored = score_to_tt(score, ply);
        let recovered = score_from_tt(stored, ply);
        assert_eq!(recovered, score, "Mate score should survive TT roundtrip");
    }

    #[test]
    fn mate_score_tt_different_ply() {
        let score = MATE_SCORE - 10;
        let stored = score_to_tt(score, 3);
        let at_ply5 = score_from_tt(stored, 5);
        assert_eq!(at_ply5, score - 2, "Mate score at different ply should adjust by ply delta");
    }

    #[test]
    fn normal_score_tt_unchanged() {
        let score = 150;
        let stored = score_to_tt(score, 7);
        let recovered = score_from_tt(stored, 7);
        assert_eq!(recovered, score);
        let at_other = score_from_tt(stored, 2);
        assert_eq!(at_other, score, "Normal scores should not change with ply");
    }

    #[test]
    fn move_to_uci_outputs_standard_white_castling() {
        let board: Board = "rnbqkb1r/ppp2ppp/4pn2/3p4/8/5NP1/PPPPPPBP/RNBQK2R w KQkq - 0 4"
            .parse()
            .unwrap();
        let mv: Move = "e1h1".parse().unwrap();

        assert_eq!(move_to_uci(mv, &board), "e1g1");
    }

    #[test]
    fn move_to_uci_outputs_standard_black_castling() {
        let board: Board = "rnbqk2r/ppppbppp/5n2/4p3/4P3/5N2/PPPPBPPP/RNBQ1RK1 b kq - 1 5"
            .parse()
            .unwrap();
        let mv: Move = "e8h8".parse().unwrap();

        assert_eq!(move_to_uci(mv, &board), "e8g8");
    }

    #[test]
    fn move_to_uci_outputs_standard_queenside_castling() {
        let white_board: Board = "r3kbnr/ppp1pppp/2n1b3/3p4/3P4/2N1B3/PPPQPPPP/R3KBNR w KQkq - 4 5"
            .parse()
            .unwrap();
        let black_board: Board = "r3kbnr/ppp1pppp/2n1b3/3p4/3P4/2N1B3/PPPQPPPP/2KR1BNR b kq - 5 5"
            .parse()
            .unwrap();

        assert_eq!(move_to_uci("e1a1".parse().unwrap(), &white_board), "e1c1");
        assert_eq!(move_to_uci("e8a8".parse().unwrap(), &black_board), "e8c8");
    }
}
