mod pst;

use cozy_chess::*;
use crate::types::*;

// integer sqrt table, Q8 fixed point: sqrt(i) * 256
#[rustfmt::skip]
const ISQRT_Q8: [i32; 65] = [
    0, 256, 362, 443, 512, 572, 626, 676, 724, 768, 810, 849, 887, 922, 957, 989,
    1024, 1055, 1086, 1116, 1145, 1173, 1200, 1227, 1253, 1280, 1305, 1330, 1355, 1379, 1403, 1426,
    1448, 1471, 1493, 1515, 1536, 1557, 1578, 1598, 1619, 1638, 1658, 1677, 1696, 1715, 1734, 1752,
    1770, 1788, 1806, 1824, 1841, 1858, 1875, 1892, 1909, 1925, 1941, 1957, 1973, 1989, 2005, 2020,
    2048,
];

// sqrt(i * 5.5) * 256 for opponent mobility estimate
#[rustfmt::skip]
const XMOB_SQRT_Q8: [i32; 17] = [
    0, 600, 849, 1039, 1200, 1341, 1469, 1588, 1697, 1800, 1897, 1990, 2078, 2163, 2245, 2323, 2400,
];

// passed pawn base values by distance to promotion
const PASSED_BASE: [Score; 8] = [0, 350, 150, 80, 40, 20, 10, 5];

// king zone attack weights per piece type
const KATK_W: [i32; 6] = [0, 2, 2, 3, 5, 0]; // P, N, B, R, Q, K

// safe check weights - knight checks can't be interposed so worth more
const KCHECK_W: [i32; 5] = [0, 3, 2, 4, 6]; // P(unused), N, B, R, Q

#[derive(Clone, Copy, Default)]
struct PawnEntry {
    key: u64,
    score: Score,
}

pub struct PawnTable {
    entries: Vec<PawnEntry>,
    mask: usize,
}

impl PawnTable {
    pub fn new(size_kb: usize) -> Self {
        let count = ((size_kb * 1024) / std::mem::size_of::<PawnEntry>()).next_power_of_two();
        Self { entries: vec![PawnEntry::default(); count], mask: count - 1 }
    }

    fn probe(&self, key: u64) -> Option<Score> {
        let e = &self.entries[key as usize & self.mask];
        if e.key == key { Some(e.score) } else { None }
    }

    fn store(&mut self, key: u64, score: Score) {
        let idx = key as usize & self.mask;
        self.entries[idx] = PawnEntry { key, score };
    }
}

pub struct Evaluator {
    pub pawn_table: PawnTable,
}

impl Evaluator {
    pub fn new() -> Self {
        Self { pawn_table: PawnTable::new(256) }
    }
}

#[inline]
fn chebyshev(a: Square, b: Square) -> i32 {
    let dr = (a.rank() as i32 - b.rank() as i32).abs();
    let df = (a.file() as i32 - b.file() as i32).abs();
    dr.max(df)
}

#[inline]
fn square_attacked_by(board: &Board, sq: Square, color: Color) -> bool {
    let occ = board.occupied();
    let pieces = board.colors(color);
    !((get_pawn_attacks(sq, !color) & pieces & board.pieces(Piece::Pawn))
        | (get_knight_moves(sq) & pieces & board.pieces(Piece::Knight))
        | (get_bishop_moves(sq, occ) & pieces & (board.pieces(Piece::Bishop) | board.pieces(Piece::Queen)))
        | (get_rook_moves(sq, occ) & pieces & (board.pieces(Piece::Rook) | board.pieces(Piece::Queen)))
        | (get_king_moves(sq) & pieces & board.pieces(Piece::King)))
        .is_empty()
}

#[inline]
fn has_no_pieces(board: &Board, color: Color) -> bool {
    let non_pk = board.colors(color) & !board.pieces(Piece::Pawn) & !board.pieces(Piece::King);
    non_pk.is_empty()
}

// closed-form solvers for simple endgames (<=5 pieces)
fn endgame_solver(board: &Board) -> Option<Score> {
    let occ = board.occupied();
    let n = occ.len();
    if n > 5 { return None; }

    let wn = board.colored_pieces(Color::White, Piece::Knight).len();
    let bn = board.colored_pieces(Color::Black, Piece::Knight).len();
    let wb = board.colored_pieces(Color::White, Piece::Bishop).len();
    let bb = board.colored_pieces(Color::Black, Piece::Bishop).len();
    let wr = board.colored_pieces(Color::White, Piece::Rook).len();
    let br = board.colored_pieces(Color::Black, Piece::Rook).len();
    let wq = board.colored_pieces(Color::White, Piece::Queen).len();
    let bq = board.colored_pieces(Color::Black, Piece::Queen).len();
    let wp_count = board.colored_pieces(Color::White, Piece::Pawn).len();
    let bp_count = board.colored_pieces(Color::Black, Piece::Pawn).len();

    let edge_distance = |sq: Square| -> i32 {
        let r = sq.rank() as i32;
        let f = sq.file() as i32;
        r.min(7 - r).min(f).min(7 - f)
    };

    // KBN mate needs to force king to the correct corner based on bishop color
    let corner_distance_bn = |sq: Square, bishop_sq: Square| -> i32 {
        let r = sq.rank() as i32;
        let f = sq.file() as i32;
        let bishop_light = (bishop_sq.rank() as i32 + bishop_sq.file() as i32) % 2 == 0;
        if bishop_light {
            (r - f).abs().min((7 - r - (7 - f)).abs())
                .min(((r) + (f)).min((7 - r) + (7 - f)))
        } else {
            ((r) + (f)).min((7 - r) + (7 - f))
                .min((r - f).abs().min((7 - r - (7 - f)).abs()))
        }
    };

    for &strong in &[Color::White, Color::Black] {
        let weak = !strong;
        let sign: Score = if strong == Color::White { 1 } else { -1 };

        let s_q = if strong == Color::White { wq } else { bq };
        let s_r = if strong == Color::White { wr } else { br };
        let s_b = if strong == Color::White { wb } else { bb };
        let s_n = if strong == Color::White { wn } else { bn };
        let s_p = if strong == Color::White { wp_count } else { bp_count };
        let w_q = if weak == Color::White { wq } else { bq };
        let w_r = if weak == Color::White { wr } else { br };
        let w_b = if weak == Color::White { wb } else { bb };
        let w_n = if weak == Color::White { wn } else { bn };
        let w_p = if weak == Color::White { wp_count } else { bp_count };

        let weak_bare = w_q == 0 && w_r == 0 && w_b == 0 && w_n == 0 && w_p == 0;

        if !weak_bare { continue; }

        let ek = board.king(weak);
        let sk = board.king(strong);
        let edge = edge_distance(ek);
        let king_dist = chebyshev(sk, ek);

        // KQK
        if s_q == 1 && s_r == 0 && s_b == 0 && s_n == 0 && s_p == 0 {
            return Some(sign * (QUEEN_VALUE + 200 + 50 * (3 - edge) + 20 * (7 - king_dist)));
        }

        // KRK
        if s_r == 1 && s_q == 0 && s_b == 0 && s_n == 0 && s_p == 0 {
            return Some(sign * (ROOK_VALUE + 150 + 50 * (3 - edge) + 20 * (7 - king_dist)));
        }

        // KBNK
        if s_b == 1 && s_n == 1 && s_q == 0 && s_r == 0 && s_p == 0 {
            let bsq = board.colored_pieces(strong, Piece::Bishop).into_iter().next().unwrap();
            let corner_d = corner_distance_bn(ek, bsq);
            return Some(sign * (BISHOP_VALUE + KNIGHT_VALUE + 100 + 40 * (3 - corner_d).max(0) + 20 * (7 - king_dist)));
        }

        // KBBK
        if s_b == 2 && s_q == 0 && s_r == 0 && s_n == 0 && s_p == 0 {
            return Some(sign * (2 * BISHOP_VALUE + 100 + 50 * (3 - edge) + 20 * (7 - king_dist)));
        }

        // KPK
        if s_p == 1 && s_q == 0 && s_r == 0 && s_b == 0 && s_n == 0 {
            let psq = board.colored_pieces(strong, Piece::Pawn).into_iter().next().unwrap();
            let pf = psq.file() as i32;
            let pr = psq.rank() as i32;
            let promo_rank = if strong == Color::White { 7 } else { 0 };
            let pawn_dist = (promo_rank - pr).unsigned_abs() as i32;
            let promo_sq = Square::index(promo_rank as usize * 8 + pf as usize);
            let sk_dist = chebyshev(sk, promo_sq);
            let ek_dist = chebyshev(ek, promo_sq);

            // rook pawn draws if defending king reaches the corner
            if pf == 0 || pf == 7 {
                if ek_dist <= pawn_dist + 1 {
                    return Some(DRAW_SCORE);
                }
            }

            // simplified key-square rule
            let pawn_adv = if strong == Color::White { pr - 1 } else { 6 - pr };
            if sk_dist <= 1 && ek_dist > pawn_dist + 1 {
                return Some(sign * (QUEEN_VALUE - PAWN_VALUE + 100 - 10 * pawn_dist));
            }
            if ek_dist <= pawn_dist && sk_dist > pawn_dist {
                if pawn_adv <= 3 {
                    return Some(DRAW_SCORE);
                }
            }
        }

        // KBP vs K - wrong bishop + rook pawn is drawn
        if s_b == 1 && s_p == 1 && s_q == 0 && s_r == 0 && s_n == 0 {
            let bsq = board.colored_pieces(strong, Piece::Bishop).into_iter().next().unwrap();
            let psq = board.colored_pieces(strong, Piece::Pawn).into_iter().next().unwrap();
            let pf = psq.file() as i32;
            if pf == 0 || pf == 7 {
                let promo_rank = if strong == Color::White { 7 } else { 0 };
                let promo_sq = Square::index(promo_rank as usize * 8 + pf as usize);
                let bishop_color = (bsq.rank() as i32 + bsq.file() as i32) % 2;
                let promo_color = (promo_sq.rank() as i32 + promo_sq.file() as i32) % 2;
                if bishop_color != promo_color {
                    let ek_dist = chebyshev(ek, promo_sq);
                    if ek_dist <= 2 {
                        return Some(DRAW_SCORE);
                    }
                    return Some(sign * 30); // very drawish even with king far away
                }
            }
        }

        // KQKP - handled by KQK above, skip
        if s_q == 1 && s_r == 0 && s_b == 0 && s_n == 0 && s_p == 0 && w_p == 0 {
        }
    }

    // KQ vs KP (pawn-on-7th fortress)
    for &q_side in &[Color::White, Color::Black] {
        let p_side = !q_side;
        let sign: Score = if q_side == Color::White { 1 } else { -1 };
        let qs_q = if q_side == Color::White { wq } else { bq };
        let qs_r = if q_side == Color::White { wr } else { br };
        let qs_b = if q_side == Color::White { wb } else { bb };
        let qs_n = if q_side == Color::White { wn } else { bn };
        let qs_p = if q_side == Color::White { wp_count } else { bp_count };
        let ps_q = if p_side == Color::White { wq } else { bq };
        let ps_r = if p_side == Color::White { wr } else { br };
        let ps_b = if p_side == Color::White { wb } else { bb };
        let ps_n = if p_side == Color::White { wn } else { bn };
        let ps_p = if p_side == Color::White { wp_count } else { bp_count };

        if qs_q != 1 || qs_r != 0 || qs_b != 0 || qs_n != 0 || qs_p != 0 { continue; }
        if ps_q != 0 || ps_r != 0 || ps_b != 0 || ps_n != 0 || ps_p != 1 { continue; }

        let psq = board.colored_pieces(p_side, Piece::Pawn).into_iter().next().unwrap();
        let pk = board.king(p_side);
        let _qk = board.king(q_side);
        let pf = psq.file() as i32;
        let pr = psq.rank() as i32;
        let promo_rank = if p_side == Color::White { 7 } else { 0 };
        let pawn_dist = (promo_rank - pr).unsigned_abs() as i32;

        // pawn on 7th with king support - fortress on a/c/f/h files
        if pawn_dist <= 1 && chebyshev(pk, psq) <= 1 {
            if pf == 0 || pf == 7 {
                return Some(DRAW_SCORE);
            }
            // c/f-file stalemate fortress
            if (pf == 2 || pf == 5) && pawn_dist == 1 {
                return Some(sign * 50);
            }
        }
        if pawn_dist >= 2 {
            return Some(sign * (QUEEN_VALUE - PAWN_VALUE + 100));
        }
    }

    None
}

pub fn evaluate(board: &Board, eval: &mut Evaluator) -> Score {
    let occ = board.occupied();
    if occ.len() == 2 { return DRAW_SCORE; }

    if let Some(s) = endgame_solver(board) {
        return s;
    }

    let wp = board.colored_pieces(Color::White, Piece::Pawn);
    let bp = board.colored_pieces(Color::Black, Piece::Pawn);
    let wk_sq = board.king(Color::White);
    let bk_sq = board.king(Color::Black);

    // material + PST + phase
    let mut mg: Score = 0;
    let mut eg: Score = 0;
    let mut phase: i32 = 0;
    let mut w_bishops = 0u32;
    let mut b_bishops = 0u32;
    let mut w_pawn_files = [0u8; 8];
    let mut b_pawn_files = [0u8; 8];
    let mut total_pawns = 0u32;

    for &color in &[Color::White, Color::Black] {
        let sign: Score = if color == Color::White { 1 } else { -1 };
        for &piece in &[Piece::Pawn, Piece::Knight, Piece::Bishop, Piece::Rook, Piece::Queen, Piece::King] {
            let val = piece_value(piece);
            let pw = phase_weight(piece);
            let bb = board.colored_pieces(color, piece);
            for sq in bb {
                let pst_sq = if color == Color::White { sq as usize } else { sq as usize ^ 56 };
                mg += sign * (val + pst::pst_mg_idx(piece, pst_sq));
                eg += sign * (val + pst::pst_eg_idx(piece, pst_sq));
                phase += pw;

                if piece == Piece::Bishop {
                    if color == Color::White { w_bishops += 1; } else { b_bishops += 1; }
                } else if piece == Piece::Pawn {
                    let f = sq.file() as usize;
                    total_pawns += 1;
                    if color == Color::White { w_pawn_files[f] += 1; } else { b_pawn_files[f] += 1; }
                }
            }
        }
    }

    let phase = phase.min(TOTAL_PHASE);
    let mg256 = (phase * 256 / TOTAL_PHASE) as i32;
    let eg256 = 256 - mg256;

    // lazy eval: skip mobility/king-attack when material is decisive
    const LAZY_MARGIN: Score = 400;
    let rough_score = (mg * mg256 + eg * eg256) >> 8;
    if rough_score.abs() >= LAZY_MARGIN {
        let pawn_key = wp.0 ^ bp.0.wrapping_mul(0x9e3779b97f4a7c15);
        let pawn_score = if let Some(s) = eval.pawn_table.probe(pawn_key) {
            s
        } else {
            let s = evaluate_pawn_structure(&w_pawn_files, &b_pawn_files, board);
            eval.pawn_table.store(pawn_key, s);
            s
        };
        let total_mg = mg + pawn_score;
        let total_eg = eg + pawn_score;
        let total = (total_mg * mg256 + total_eg * eg256) >> 8;
        let total = endgame_progress(board, total, phase);
        let total = endgame_scale(board, total);
        return rook_endgame_geometry(board, total);
    }

    // mobility + king attack (symmetric, both sides)
    let mut w_mob: u32 = 0;
    let mut b_mob: u32 = 0;

    let mut w_kz_attacks: i32 = 0;
    let mut w_kz_attackers: i32 = 0;
    let bk_ring = get_king_moves(bk_sq) | BitBoard::from(bk_sq);
    let mut b_kz_attacks: i32 = 0;
    let mut b_kz_attackers: i32 = 0;
    let wk_ring = get_king_moves(wk_sq) | BitBoard::from(wk_sq);
    let mut w_hanging: i32 = 0;
    let mut b_hanging: i32 = 0;
    let mut w_attacks = BitBoard::EMPTY;
    let mut b_attacks = BitBoard::EMPTY;
    let blockers = occ;

    for &piece in &[Piece::Knight, Piece::Bishop, Piece::Rook, Piece::Queen] {
        let aw = KATK_W[piece as usize];

        for sq in board.colored_pieces(Color::White, piece) {
            let attacks = match piece {
                Piece::Knight => get_knight_moves(sq),
                Piece::Bishop => get_bishop_moves(sq, blockers),
                Piece::Rook => get_rook_moves(sq, blockers),
                Piece::Queen => get_bishop_moves(sq, blockers) | get_rook_moves(sq, blockers),
                _ => BitBoard::EMPTY,
            };
            w_mob += attacks.len() as u32;
            w_attacks = BitBoard(w_attacks.0 | attacks.0);
            let ring_hits = (attacks & bk_ring).len() as i32;
            if ring_hits > 0 {
                w_kz_attacks += ring_hits * aw;
                w_kz_attackers += 1;
            }
        }

        for sq in board.colored_pieces(Color::Black, piece) {
            let attacks = match piece {
                Piece::Knight => get_knight_moves(sq),
                Piece::Bishop => get_bishop_moves(sq, blockers),
                Piece::Rook => get_rook_moves(sq, blockers),
                Piece::Queen => get_bishop_moves(sq, blockers) | get_rook_moves(sq, blockers),
                _ => BitBoard::EMPTY,
            };
            b_mob += attacks.len() as u32;
            b_attacks = BitBoard(b_attacks.0 | attacks.0);
            let ring_hits = (attacks & wk_ring).len() as i32;
            if ring_hits > 0 {
                b_kz_attacks += ring_hits * aw;
                b_kz_attackers += 1;
            }
        }
    }

    // pawn attacks via bitboard shifts
    let not_a = 0xfefefefefefefefe_u64;
    let not_h = 0x7f7f7f7f7f7f7f7f_u64;
    let w_pawn_atk = BitBoard(((wp.0 & not_a) << 7) | ((wp.0 & not_h) << 9));
    let b_pawn_atk = BitBoard(((bp.0 & not_h) >> 7) | ((bp.0 & not_a) >> 9));
    w_attacks = BitBoard(w_attacks.0 | w_pawn_atk.0);
    b_attacks = BitBoard(b_attacks.0 | b_pawn_atk.0);

    // hanging pieces: attacked by enemy, not defended
    for &piece in &[Piece::Knight, Piece::Bishop, Piece::Rook, Piece::Queen] {
        let pv = piece_value(piece);
        let w_pieces_bb = board.colored_pieces(Color::White, piece);
        let b_attacks_w = BitBoard(b_attacks.0 & w_pieces_bb.0 & !w_attacks.0);
        w_hanging += b_attacks_w.len() as i32 * pv / 4;
        let b_pieces_bb = board.colored_pieces(Color::Black, piece);
        let w_attacks_b = BitBoard(w_attacks.0 & b_pieces_bb.0 & !b_attacks.0);
        b_hanging += w_attacks_b.len() as i32 * pv / 4;
    }

    let mob_coeff = 3 * 256 + 4 * mg256;
    let w_sqrt = ISQRT_Q8[w_mob.min(64) as usize];
    let b_sqrt = ISQRT_Q8[b_mob.min(64) as usize];
    let mob_score = mob_coeff * (w_sqrt - b_sqrt) >> 16;
    let hanging_score = (b_hanging - w_hanging) * mg256 / 256;

    // pawn threats on minors (~15cp each, middlegame only)
    let mut threat_score: Score = 0;
    if mg256 > 64 {
        let b_minors = board.colored_pieces(Color::Black, Piece::Knight)
            | board.colored_pieces(Color::Black, Piece::Bishop);
        let w_threats = (w_pawn_atk & b_minors).len() as i32;
        let w_minors = board.colored_pieces(Color::White, Piece::Knight)
            | board.colored_pieces(Color::White, Piece::Bishop);
        let b_threats = (b_pawn_atk & w_minors).len() as i32;
        threat_score = (w_threats - b_threats) * 15 * mg256 / 256;
    }

    // quadratic king attack: danger = units^2 / 64
    // superlinear because each attacker exponentially reduces king escape squares
    let mut king_attack: Score = 0;
    let bk_check_n = get_knight_moves(bk_sq);
    let bk_check_b = get_bishop_moves(bk_sq, blockers);
    let bk_check_r = get_rook_moves(bk_sq, blockers);
    let wk_check_n = get_knight_moves(wk_sq);
    let wk_check_b = get_bishop_moves(wk_sq, blockers);
    let wk_check_r = get_rook_moves(wk_sq, blockers);

    // white attacking black king
    {
        let mut units: i32 = w_kz_attacks;
        let safe_mask = BitBoard(!b_attacks.0);

        let n_checks = (bk_check_n & w_attacks & safe_mask
            & BitBoard(!board.colors(Color::White).0))
            .len() as i32;
        units += n_checks.min(2) * KCHECK_W[1];

        let bq_checks = (bk_check_b & w_attacks & safe_mask
            & BitBoard(!board.colors(Color::White).0))
            .len() as i32;
        units += bq_checks.min(2) * KCHECK_W[2];

        let rq_checks = (bk_check_r & w_attacks & safe_mask
            & BitBoard(!board.colors(Color::White).0))
            .len() as i32;
        units += rq_checks.min(2) * KCHECK_W[3];

        // missing shield pawns add 3 units each
        let mut shield = 0i32;
        let ek_f = bk_sq.file() as i8;
        let ek_r = bk_sq.rank() as i8;
        for df in -1..=1i8 {
            let sf = ek_f + df;
            let sr = ek_r - 1;
            if (0..8).contains(&sf) && (0..8).contains(&sr) {
                let ssq = Square::index(sr as usize * 8 + sf as usize);
                if board.piece_on(ssq) == Some(Piece::Pawn) && board.color_on(ssq) == Some(Color::Black) {
                    shield += 1;
                }
            }
        }
        units += (3 - shield) * 3;

        if units >= 4 {
            king_attack += (units * units / 64) * mg256 / 256;
        }
    }

    // black attacking white king
    {
        let mut units: i32 = b_kz_attacks;

        let safe_mask = BitBoard(!w_attacks.0);

        let n_checks = (wk_check_n & b_attacks & safe_mask
            & BitBoard(!board.colors(Color::Black).0))
            .len() as i32;
        units += n_checks.min(2) * KCHECK_W[1];

        let bq_checks = (wk_check_b & b_attacks & safe_mask
            & BitBoard(!board.colors(Color::Black).0))
            .len() as i32;
        units += bq_checks.min(2) * KCHECK_W[2];

        let rq_checks = (wk_check_r & b_attacks & safe_mask
            & BitBoard(!board.colors(Color::Black).0))
            .len() as i32;
        units += rq_checks.min(2) * KCHECK_W[3];

        let mut shield = 0i32;
        let ek_f = wk_sq.file() as i8;
        let ek_r = wk_sq.rank() as i8;
        for df in -1..=1i8 {
            let sf = ek_f + df;
            let sr = ek_r + 1;
            if (0..8).contains(&sf) && (0..8).contains(&sr) {
                let ssq = Square::index(sr as usize * 8 + sf as usize);
                if board.piece_on(ssq) == Some(Piece::Pawn) && board.color_on(ssq) == Some(Color::White) {
                    shield += 1;
                }
            }
        }
        units += (3 - shield) * 3;

        if units >= 4 {
            king_attack -= (units * units / 64) * mg256 / 256;
        }
    }

    // space: squares in enemy half we control exclusively
    let mut space_score: Score = 0;
    if mg256 > 64 {
        let w_only = BitBoard(w_attacks.0 & !b_attacks.0);
        let b_only = BitBoard(b_attacks.0 & !w_attacks.0);
        let black_half = 0xFFFFFFFF00000000_u64;
        let white_half = 0x00000000FFFFFFFF_u64;
        let w_space = (BitBoard(w_only.0 & black_half)).len() as i32;
        let b_space = (BitBoard(b_only.0 & white_half)).len() as i32;
        space_score = (w_space - b_space) * mg256 / 256;
    }

    // knight outposts: pawn-defended, no enemy pawns can challenge on adjacent files
    let mut outpost_score: Score = 0;
    for sq in board.colored_pieces(Color::White, Piece::Knight) {
        let r = sq.rank() as usize;
        let f = sq.file() as usize;
        if r >= 4 {
            let pawn_defends = !(get_pawn_attacks(sq, Color::Black) & wp).is_empty();
            if pawn_defends {
                let mut can_be_attacked = false;
                for df in [-1i32, 1] {
                    let nf = f as i32 + df;
                    if !(0..8).contains(&nf) { continue; }
                    for pr in (r + 1)..=6 {
                        let psq = Square::index(pr * 8 + nf as usize);
                        if board.piece_on(psq) == Some(Piece::Pawn) && board.color_on(psq) == Some(Color::Black) {
                            can_be_attacked = true;
                            break;
                        }
                    }
                    if can_be_attacked { break; }
                }
                if !can_be_attacked {
                    outpost_score += 15 + 5 * (r as Score - 4);
                }
            }
        }
    }
    for sq in board.colored_pieces(Color::Black, Piece::Knight) {
        let r = sq.rank() as usize;
        let f = sq.file() as usize;
        if r <= 3 {
            let pawn_defends = !(get_pawn_attacks(sq, Color::White) & bp).is_empty();
            if pawn_defends {
                let mut can_be_attacked = false;
                for df in [-1i32, 1] {
                    let nf = f as i32 + df;
                    if !(0..8).contains(&nf) { continue; }
                    for pr in 1..r {
                        let psq = Square::index(pr * 8 + nf as usize);
                        if board.piece_on(psq) == Some(Piece::Pawn) && board.color_on(psq) == Some(Color::White) {
                            can_be_attacked = true;
                            break;
                        }
                    }
                    if can_be_attacked { break; }
                }
                if !can_be_attacked {
                    outpost_score -= 15 + 5 * (3 - r as Score);
                }
            }
        }
    }

    // bad bishop: penalty for pawns on same color complex
    let mut bishop_complex_score: Score = 0;
    if w_bishops == 1 {
        let bsq = board.colored_pieces(Color::White, Piece::Bishop).into_iter().next().unwrap();
        let b_light = (bsq.rank() as i32 + bsq.file() as i32) % 2;
        let mut bad_pawns = 0i32;
        for psq in wp {
            let p_light = (psq.rank() as i32 + psq.file() as i32) % 2;
            if p_light == b_light { bad_pawns += 1; }
        }
        bishop_complex_score += -3 * bad_pawns;
    }
    if b_bishops == 1 {
        let bsq = board.colored_pieces(Color::Black, Piece::Bishop).into_iter().next().unwrap();
        let b_light = (bsq.rank() as i32 + bsq.file() as i32) % 2;
        let mut bad_pawns = 0i32;
        for psq in bp {
            let p_light = (psq.rank() as i32 + psq.file() as i32) % 2;
            if p_light == b_light { bad_pawns += 1; }
        }
        bishop_complex_score -= -3 * bad_pawns;
    }

    // bishop pair bonus scales with board openness
    let open_files = (0..8u8).filter(|&f| w_pawn_files[f as usize] == 0 && b_pawn_files[f as usize] == 0).count() as i32;
    let openness256 = ((16 - total_pawns as i32 + open_files) * 256 / 24) as i32;
    let mut bp_score: Score = 0;
    if w_bishops >= 2 { bp_score += 20 + (48 * openness256 >> 8); }
    if b_bishops >= 2 { bp_score -= 20 + (48 * openness256 >> 8); }

    // rook on open/semi-open files + 7th rank
    let mut rook_score: Score = 0;
    for &color in &[Color::White, Color::Black] {
        let sign: Score = if color == Color::White { 1 } else { -1 };
        let own_pf = if color == Color::White { &w_pawn_files } else { &b_pawn_files };
        let enemy_pf = if color == Color::White { &b_pawn_files } else { &w_pawn_files };
        let ek = board.king(!color);
        let ek_file = ek.file() as i32;

        for sq in board.colored_pieces(color, Piece::Rook) {
            let f = sq.file() as usize;
            let r = sq.rank() as usize;

            if own_pf[f] == 0 {
                let base = if enemy_pf[f] == 0 { 15 } else { 10 };
                let fd = (f as i32 - ek_file).abs();
                let kf = 256 + (2 - fd).max(0) * mg256 / 2; // Q8
                rook_score += sign * (base * kf >> 8);
            }

            let seventh = if color == Color::White { 6 } else { 1 };
            if r == seventh {
                let enemy_back = if color == Color::White { 7usize } else { 0 };
                let kob = if ek.rank() as usize == enemy_back { 1 } else { 0 };
                let targets = board.colored_pieces(!color, Piece::Pawn).into_iter()
                    .filter(|s| s.rank() as usize == seventh).count() as i32;
                let r7 = 10 + 5 * targets + 10 * kob;
                rook_score += sign * (r7 * mg256 >> 8);
            }
        }
    }

    // pawn structure (cached)
    let pawn_key = wp.0 ^ bp.0.wrapping_mul(0x9e3779b97f4a7c15);
    let pawn_score = if let Some(s) = eval.pawn_table.probe(pawn_key) {
        s
    } else {
        let s = evaluate_pawn_structure(&w_pawn_files, &b_pawn_files, board);
        eval.pawn_table.store(pawn_key, s);
        s
    };
    // passer bonuses depend on pieces/kings so can't be cached
    let passer_score = evaluate_passers(board, &w_pawn_files, &b_pawn_files, phase);

    let king_safety = eval_king_safety(board, mg256);

    // phase interpolation (everything is from white's perspective)
    let total_mg = mg + mob_score + bp_score + rook_score + pawn_score + passer_score + king_safety + king_attack + hanging_score + threat_score + space_score + outpost_score + bishop_complex_score;
    let eg_mob = mob_score * eg256 / mg256.max(64) / 2;
    let total_eg = eg + eg_mob + bp_score + rook_score + pawn_score + passer_score + outpost_score + bishop_complex_score;
    let total = (total_mg * mg256 + total_eg * eg256) >> 8;

    // 50-move rule: scale eval toward draw in the last 10 moves
    let halfmove = board.halfmove_clock() as i32;
    let total = if halfmove >= 90 {
        total * (100 - halfmove) / 10
    } else if halfmove >= 80 {
        total * (110 - halfmove) / 30
    } else {
        total
    };

    let total = endgame_progress(board, total, phase);
    let total = endgame_scale(board, total);
    rook_endgame_geometry(board, total)
}

// when winning an endgame, reward driving the losing king to the edge
fn endgame_progress(board: &Board, score: Score, phase: i32) -> Score {
    if phase > TOTAL_PHASE / 2 || score.abs() < PAWN_VALUE {
        return score;
    }

    let winning_color = if score > 0 { Color::White } else { Color::Black };
    let losing_color = !winning_color;
    let sign: Score = if winning_color == Color::White { 1 } else { -1 };

    let losing_king = board.king(losing_color);
    let winning_king = board.king(winning_color);

    let edge_dist = (losing_king.rank() as i32).min(7 - losing_king.rank() as i32)
        .min(losing_king.file() as i32).min(7 - losing_king.file() as i32);
    let edge_bonus = sign * (3 - edge_dist) * 10;
    let king_dist = chebyshev(winning_king, losing_king);
    let proximity_bonus = sign * (7 - king_dist) * 5;
    let eg_factor = (TOTAL_PHASE - phase) * 256 / TOTAL_PHASE;
    let progress = (edge_bonus + proximity_bonus) * eg_factor / 256;

    score + progress
}

fn evaluate_pawn_structure(w_files: &[u8; 8], b_files: &[u8; 8], board: &Board) -> Score {
    let mut score: Score = 0;

    for &color in &[Color::White, Color::Black] {
        let sign: Score = if color == Color::White { 1 } else { -1 };
        let own_files = if color == Color::White { w_files } else { b_files };
        let enemy_pawns = board.colored_pieces(!color, Piece::Pawn);
        let mut doubled_seen = 0u8;

        for sq in board.colored_pieces(color, Piece::Pawn) {
            let f = sq.file() as usize;
            let r = sq.rank() as usize;

            let mut is_passed = true;
            for ep in enemy_pawns {
                let ef = ep.file() as usize;
                let er = ep.rank() as usize;
                if (ef as i32 - f as i32).unsigned_abs() <= 1 {
                    if color == Color::White && er > r { is_passed = false; break; }
                    if color == Color::Black && er < r { is_passed = false; break; }
                }
            }
            if is_passed {
                let dist = if color == Color::White { 7 - r } else { r };
                let dist = dist.clamp(1, 7);
                score += sign * PASSED_BASE[dist];
            }

            let has_nb = (f > 0 && own_files[f - 1] > 0) || (f < 7 && own_files[f + 1] > 0);
            if !has_nb { score -= sign * 20; } // isolated

            if own_files[f] > 1 && (doubled_seen & (1 << f)) == 0 { // doubled
                doubled_seen |= 1 << f;
                score -= sign * 12 * (own_files[f] as Score - 1);
            }

            for other in board.colored_pieces(color, Piece::Pawn) { // connected
                if other == sq { continue; }
                if (other.file() as i32 - f as i32).abs() == 1 && (other.rank() as i32 - r as i32).abs() <= 1 {
                    score += sign * 7;
                    break;
                }
            }
        }
    }
    score
}

fn evaluate_passers(board: &Board, w_files: &[u8; 8], b_files: &[u8; 8], phase: i32) -> Score {
    let mut score: Score = 0;
    let promotion_corridor_active = phase <= TOTAL_PHASE / 4;

    for &color in &[Color::White, Color::Black] {
        let sign: Score = if color == Color::White { 1 } else { -1 };
        let enemy_pawns = board.colored_pieces(!color, Piece::Pawn);
        let own_rooks = board.colored_pieces(color, Piece::Rook);

        for sq in board.colored_pieces(color, Piece::Pawn) {
            let f = sq.file() as usize;
            let r = sq.rank() as usize;

            let mut is_passed = true;
            for ep in enemy_pawns {
                let ef = ep.file() as usize;
                let er = ep.rank() as usize;
                if (ef as i32 - f as i32).unsigned_abs() <= 1 {
                    if color == Color::White && er > r { is_passed = false; break; }
                    if color == Color::Black && er < r { is_passed = false; break; }
                }
            }
            if !is_passed { continue; }

            let dist = if color == Color::White { 7 - r } else { r };
            let dist = dist.clamp(1, 7);
            let mut pp: Score = 0;

            let front_r = if color == Color::White { r + 1 } else { r.wrapping_sub(1) };
            if front_r < 8 {
                let front = Square::index(front_r * 8 + f);
                if board.piece_on(front).is_some() && board.color_on(front) != Some(color) {
                    pp -= PASSED_BASE[dist] / 2; // blockade
                }
            }

            for rsq in own_rooks { // rook behind passer
                if rsq.file() as usize == f {
                    let behind = if color == Color::White {
                        (rsq.rank() as usize) < r
                    } else {
                        (rsq.rank() as usize) > r
                    };
                    if behind { pp += PASSED_BASE[dist] * 3 / 10; break; }
                }
            }

            let promo = Square::index(if color == Color::White { 7 * 8 + f } else { f });
            let ksq = board.king(color);
            let eksq = board.king(!color);
            pp += (30 - 5 * chebyshev(ksq, promo)).max(0);
            pp += 5 * chebyshev(eksq, promo);

            let promotion_gain = QUEEN_VALUE - PAWN_VALUE;
            let mut king_support = 0;
            if chebyshev(ksq, sq) == 1 {
                king_support = king_support.max(promotion_gain / (dist as Score + 2));
            }
            if front_r < 8 {
                let front = Square::index(front_r * 8 + f);
                if chebyshev(ksq, front) == 1 {
                    king_support = king_support.max(promotion_gain / (dist as Score + 1));
                }
            }
            pp += king_support;

            // rule of the square: if the defending king can't catch the pawn, it promotes
            {
                let def_pieces = board.colored_pieces(!color, Piece::Knight).len()
                    + board.colored_pieces(!color, Piece::Bishop).len()
                    + board.colored_pieces(!color, Piece::Rook).len()
                    + board.colored_pieces(!color, Piece::Queen).len();
                if def_pieces <= 1 {
                    let pawn_dist_to_promo = dist as i32;
                    let king_dist_to_promo = chebyshev(eksq, promo);
                    let tempo_adj = if board.side_to_move() != color { 1 } else { 0 };
                    let starting_rank = if color == Color::White { 1 } else { 6 };
                    let effective_pawn_dist = pawn_dist_to_promo + tempo_adj
                        - if r == starting_rank { 1 } else { 0 };
                    if king_dist_to_promo > effective_pawn_dist {
                        pp += promotion_gain - pawn_dist_to_promo as Score * 10;
                    }
                }
            }

            if promotion_corridor_active { // promotion corridor control
                let mut corridor_num: Score = 0;
                let corridor_den: Score = (dist as Score * (dist as Score + 1) / 2).max(1);
                for step in 1..=dist {
                    let cr = if color == Color::White { r + step } else { r - step };
                    if cr >= 8 { break; }
                    let csq = Square::index(cr * 8 + f);
                    let own_ctrl = square_attacked_by(board, csq, color);
                    let enemy_ctrl = square_attacked_by(board, csq, !color);
                    let diff = match (own_ctrl, enemy_ctrl) {
                        (true, false) => 1,
                        (false, true) => -1,
                        _ => 0,
                    };
                    corridor_num += diff * step as Score;
                }
                pp += corridor_num * promotion_gain / ((dist as Score + 1) * corridor_den);

                let own_promo_ctrl = square_attacked_by(board, promo, color);
                let enemy_promo_ctrl = square_attacked_by(board, promo, !color);
                if own_promo_ctrl && !enemy_promo_ctrl {
                    pp += promotion_gain / (dist as Score + 1);
                } else if enemy_promo_ctrl && !own_promo_ctrl {
                    pp -= promotion_gain / (dist as Score + 1);
                }
            }

            score += sign * pp;
        }
    }
    score
}

fn eval_king_safety(board: &Board, mg256: i32) -> Score {
    if mg256 < 51 { return 0; }
    let mut score: Score = 0;

    for &color in &[Color::White, Color::Black] {
        let sign: Score = if color == Color::White { 1 } else { -1 };
        let ksq = board.king(color);
        let kr = ksq.rank() as i8;
        let kf = ksq.file() as i8;
        let cr = board.castle_rights(color);
        if cr.short.is_some() { score += sign * 15; }
        if cr.long.is_some() { score += sign * 10; }

        let dir: i8 = if color == Color::White { 1 } else { -1 };
        let mut shield = 0i32;
        for df in -1..=1i8 {
            let f = kf + df;
            if !(0..8).contains(&f) { continue; }
            for dist in 1..=2i8 {
                let r = kr + dir * dist;
                if (0..8).contains(&r) {
                    let ssq = Square::index(r as usize * 8 + f as usize);
                    if board.piece_on(ssq) == Some(Piece::Pawn) && board.color_on(ssq) == Some(color) {
                        shield += if dist == 1 { 2 } else { 1 };
                        break;
                    }
                }
            }
        }
        score += sign * (15 * mg256 * (shield - 3) / 256 >> 1);

        if mg256 > 102 {
            let home: i8 = if color == Color::White { 0 } else { 7 };
            if kr == home && (2..=5).contains(&kf) {
                let has_cr = cr.short.is_some() || cr.long.is_some();
                if !has_cr { score -= sign * (30 * mg256 >> 8); }
            }
        }
    }
    score
}

// mating potential: 0..256 (Q8), how likely this side can force mate
fn mating_potential(board: &Board, color: Color) -> i32 {
    let n = board.colored_pieces(color, Piece::Knight).len() as i32;
    let b = board.colored_pieces(color, Piece::Bishop).len() as i32;
    let r = board.colored_pieces(color, Piece::Rook).len() as i32;
    let q = board.colored_pieces(color, Piece::Queen).len() as i32;
    let p = board.colored_pieces(color, Piece::Pawn).len() as i32;
    let mat = n * KNIGHT_VALUE + b * BISHOP_VALUE + r * ROOK_VALUE + q * QUEEN_VALUE;
    let pieces = n + b + r + q;

    if pieces == 0 && p == 0 { return 0; }
    if r > 0 || q > 0 { return (mat * 256 / 900).min(256); }
    if pieces == 0 { return if p > 0 { (p * 77).min(256) } else { 0 }; }
    if pieces == 1 { return if p > 0 { (77 + p * 64).min(256) } else { 0 }; }
    if pieces == 2 {
        if b >= 1 && n >= 1 { return if p > 0 { (179 + p * 38).min(256) } else { 179 }; }
        if n == 2 { return if p > 0 { (p * 51).min(256) } else { 0 }; }
        if b == 2 { return if p > 0 { (179 + p * 38).min(256) } else { 179 }; }
    }
    (mat * 256 / 900).min(256)
}

fn endgame_scale(board: &Board, raw: Score) -> Score {
    if raw.abs() < 10 { return raw; }

    let wp = mating_potential(board, Color::White);
    let bp = mating_potential(board, Color::Black);
    let (win_p, def_p) = if raw > 0 { (wp, bp) } else { (bp, wp) };
    if win_p == 0 { return 0; }

    let scale = if def_p > 128 { win_p } else { win_p * win_p / 256 };

    // KR vs K
    for &color in &[Color::White, Color::Black] {
        let sign: Score = if color == Color::White { 1 } else { -1 };
        let enemy = !color;
        if board.colored_pieces(color, Piece::Rook).len() == 1
            && board.colored_pieces(color, Piece::Queen).is_empty()
            && board.colored_pieces(color, Piece::Knight).is_empty()
            && board.colored_pieces(color, Piece::Bishop).is_empty()
            && board.colored_pieces(color, Piece::Pawn).is_empty()
            && has_no_pieces(board, enemy)
            && board.colored_pieces(enemy, Piece::Pawn).is_empty()
        {
            let ek = board.king(enemy);
            let edge = (ek.rank() as i32).min(7 - ek.rank() as i32)
                      .min(ek.file() as i32).min(7 - ek.file() as i32);
            return sign * (400 + 50 * (3 - edge));
        }
    }

    let scaled = raw * scale / 256;

    // opposite-color bishops are very drawish
    let wb = board.colored_pieces(Color::White, Piece::Bishop);
    let bb_bish = board.colored_pieces(Color::Black, Piece::Bishop);
    if wb.len() == 1 && bb_bish.len() == 1 {
        let wsq = wb.into_iter().next().unwrap();
        let bsq = bb_bish.into_iter().next().unwrap();
        let w_light = (wsq.rank() as i32 + wsq.file() as i32) % 2;
        let b_light = (bsq.rank() as i32 + bsq.file() as i32) % 2;
        if w_light != b_light {
            let wn = board.colored_pieces(Color::White, Piece::Knight).len();
            let bn = board.colored_pieces(Color::Black, Piece::Knight).len();
            let wr = board.colored_pieces(Color::White, Piece::Rook).len();
            let br = board.colored_pieces(Color::Black, Piece::Rook).len();
            let wq = board.colored_pieces(Color::White, Piece::Queen).len();
            let bq = board.colored_pieces(Color::Black, Piece::Queen).len();
            if wn == 0 && bn == 0 && wr == 0 && br == 0 && wq == 0 && bq == 0 {
                return scaled * 77 / 256; // pure OCB: ~30%
            }
            if (wn + bn) <= 1 && wr == 0 && br == 0 && wq == 0 && bq == 0 {
                return scaled * 128 / 256; // OCB + knight: ~50%
            }
        }
    }

    // KR+minor vs KR: theoretically drawn, scale heavily
    for &strong in &[Color::White, Color::Black] {
        let weak = !strong;
        let sr = board.colored_pieces(strong, Piece::Rook).len();
        let sn = board.colored_pieces(strong, Piece::Knight).len();
        let sb = board.colored_pieces(strong, Piece::Bishop).len();
        let sq = board.colored_pieces(strong, Piece::Queen).len();
        let sp = board.colored_pieces(strong, Piece::Pawn).len();
        let wr = board.colored_pieces(weak, Piece::Rook).len();
        let wn = board.colored_pieces(weak, Piece::Knight).len();
        let wb_c = board.colored_pieces(weak, Piece::Bishop).len();
        let wq = board.colored_pieces(weak, Piece::Queen).len();
        let wp_c = board.colored_pieces(weak, Piece::Pawn).len();
        if sr == 1 && (sn + sb) == 1 && sq == 0
            && wr == 1 && wn == 0 && wb_c == 0 && wq == 0
        {
            if sp == 0 && wp_c == 0 {
                return scaled * 40 / 256; // ~15%
            }
            if sp <= 1 && wp_c == 0 {
                return scaled * 100 / 256; // ~40%
            }
        }
    }

    scaled
}

fn rook_endgame_geometry(board: &Board, mut score: Score) -> Score {
    // R vs P(s)
    for &rook_color in &[Color::White, Color::Black] {
        let pawn_color = !rook_color;
        if board.colored_pieces(rook_color, Piece::Rook).len() != 1 { continue; }
        if !board.colored_pieces(rook_color, Piece::Queen).is_empty()
            || !board.colored_pieces(rook_color, Piece::Knight).is_empty()
            || !board.colored_pieces(rook_color, Piece::Bishop).is_empty()
            || !board.colored_pieces(rook_color, Piece::Pawn).is_empty() { continue; }
        if !has_no_pieces(board, pawn_color) { continue; }
        let pp = board.colored_pieces(pawn_color, Piece::Pawn);
        if pp.is_empty() { continue; }

        let rk = board.king(rook_color);
        let mut best_adv = -1i32;
        let mut best_sq = Square::A1;
        for psq in pp {
            let adv = if pawn_color == Color::White { psq.rank() as i32 - 1 } else { 6 - psq.rank() as i32 };
            if adv > best_adv { best_adv = adv; best_sq = psq; }
        }
        if best_adv < 0 { continue; }
        let pf = best_sq.file() as usize;
        let promo = Square::index(if pawn_color == Color::White { 7 * 8 + pf } else { pf });
        let rk_dist = chebyshev(rk, promo);
        let pawn_dist = if pawn_color == Color::White { 7 - best_sq.rank() as i32 } else { best_sq.rank() as i32 };

        if rk_dist <= pawn_dist + 1 {
            // King can intercept — draw
            score = score / 10;
            score = score.clamp(-50, 50);
        } else if best_adv >= 4 {
            score = score * 4 / 10;
        } else {
            score = score / 4;
        }
    }

    // R+P vs R (Lucena / Philidor patterns)
    for &strong in &[Color::White, Color::Black] {
        let weak = !strong;
        let sign: Score = if strong == Color::White { 1 } else { -1 };
        let sr = board.colored_pieces(strong, Piece::Rook);
        let sp = board.colored_pieces(strong, Piece::Pawn);
        let wr = board.colored_pieces(weak, Piece::Rook);
        if !(sr.len() == 1 && !sp.is_empty() && wr.len() == 1) { continue; }
        let other = [Piece::Queen, Piece::Knight, Piece::Bishop];
        if other.iter().any(|&pt| !board.colored_pieces(strong, pt).is_empty() || !board.colored_pieces(weak, pt).is_empty()) { continue; }
        if !board.colored_pieces(weak, Piece::Pawn).is_empty() { continue; }

        let sk = board.king(strong);
        let wk = board.king(weak);
        let mut best_dist = 8i32;
        let mut best_sq = Square::A1;
        for psq in sp {
            let d = if strong == Color::White { 7 - psq.rank() as i32 } else { psq.rank() as i32 };
            if d < best_dist { best_dist = d; best_sq = psq; }
        }
        let pf = best_sq.file() as usize;
        let pr = best_sq.rank() as usize;
        let promo = Square::index(if strong == Color::White { 7 * 8 + pf } else { pf });
        let wk_dist = chebyshev(wk, promo);
        let adv = if strong == Color::White { pr as i32 - 1 } else { 6 - pr as i32 };
        let cutoff = (wk.file() as i32 - pf as i32).abs();
        let king_rank_ok = if strong == Color::White { sk.rank() as usize >= pr } else { sk.rank() as usize <= pr };
        let king_file_near = (sk.file() as i32 - pf as i32).abs() <= 1;
        let king_in_front = king_rank_ok && king_file_near;

        if adv >= 5 && king_in_front && cutoff >= 1 {
            score += sign * (200 + 50 * (adv - 4) + 30 * cutoff);
        } else if adv >= 4 && chebyshev(sk, promo) <= 2 && wk_dist >= 3 {
            score += sign * (100 + 30 * (adv - 3));
        } else if wk_dist <= 1 {
            let wr_sq = wr.into_iter().next().unwrap();
            let barrier = if strong == Color::White { 5 } else { 2 };
            if adv <= 3 && wr_sq.rank() as usize == barrier {
                score = score / 5; // Philidor draw
            } else if adv <= 3 {
                score = score / 2;
            }
        }
    }

    score
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endgame_kqk_white_wins() {
        let board: Board = "4k3/8/8/8/8/8/8/4K2Q w - - 0 1".parse().unwrap();
        let s = endgame_solver(&board);
        assert!(s.is_some());
        assert!(s.unwrap() > QUEEN_VALUE, "KQK should return large positive score, got {}", s.unwrap());
    }

    #[test]
    fn endgame_kqk_black_wins() {
        let board: Board = "4k2q/8/8/8/8/8/8/4K3 w - - 0 1".parse().unwrap();
        let s = endgame_solver(&board);
        assert!(s.is_some());
        assert!(s.unwrap() < -QUEEN_VALUE, "KQK Black should return large negative, got {}", s.unwrap());
    }

    #[test]
    fn endgame_krk_corner() {
        let board: Board = "k7/8/8/8/8/8/8/4K2R w - - 0 1".parse().unwrap();
        let s = endgame_solver(&board);
        assert!(s.is_some());
        assert!(s.unwrap() > ROOK_VALUE, "KRK should score above rook value, got {}", s.unwrap());
    }

    #[test]
    fn endgame_kpk_rook_pawn_draw() {
        let board: Board = "8/8/8/8/P7/8/1k6/K7 w - - 0 1".parse().unwrap();
        let s = endgame_solver(&board);
        if let Some(score) = s {
            assert!(score.abs() <= 10, "Rook-pawn KPK with intercepting king should be draw, got {}", score);
        }
    }

    #[test]
    fn evaluate_kqk_white_perspective() {
        let board: Board = "4k3/8/8/8/8/8/8/4K2Q w - - 0 1".parse().unwrap();
        let mut eval = Evaluator::new();
        let s = evaluate(&board, &mut eval);
        assert!(s > 500, "White KQK should be positive, got {}", s);
    }

    #[test]
    fn evaluate_symmetric_mobility_startpos() {
        let board = Board::default();
        let mut eval = Evaluator::new();
        let s = evaluate(&board, &mut eval);
        assert!(s.abs() < 100, "Startpos should be near equal, got {}", s);
    }

    #[test]
    fn knight_outpost_bonus_white() {
        let board: Board = "4k3/8/8/4N3/3P4/8/8/4K3 w - - 0 1".parse().unwrap();
        let mut eval = Evaluator::new();
        let with_outpost = evaluate(&board, &mut eval);
        let board2: Board = "4k3/8/8/8/3P4/8/4N3/4K3 w - - 0 1".parse().unwrap();
        let no_outpost = evaluate(&board2, &mut eval);
        assert!(with_outpost > no_outpost, "Outpost knight should score higher: outpost={}, no_outpost={}", with_outpost, no_outpost);
    }

    #[test]
    fn knight_outpost_bonus_awarded() {
        let outpost: Board = "4k3/8/8/4N3/3P4/8/8/4K3 w - - 0 1".parse().unwrap();
        let no_outpost: Board = "4k3/8/8/8/3P4/8/4N3/4K3 w - - 0 1".parse().unwrap();
        let mut eval = Evaluator::new();
        let s_outpost = evaluate(&outpost, &mut eval);
        let s_no = evaluate(&no_outpost, &mut eval);
        assert!(s_outpost > s_no, "Knight on outpost should score higher: outpost={}, no_outpost={}", s_outpost, s_no);
    }

    #[test]
    fn opposite_color_bishop_draw_scaling() {
        let board: Board = "4k3/4p3/8/3B4/4b3/8/4P3/4K3 w - - 0 1".parse().unwrap();
        let mut eval = Evaluator::new();
        let s = evaluate(&board, &mut eval);
        assert!(s.abs() < 50, "OCB endgame should be near-draw, got {}", s);
    }

    #[test]
    fn bishop_color_complex_penalty() {
        // pawns on same color as bishop (bad) vs different color (good)
        let board_bad: Board = "4k3/8/8/8/8/4B3/1P1P1P2/4K3 w - - 0 1".parse().unwrap();
        let mut eval = Evaluator::new();
        let bad = evaluate(&board_bad, &mut eval);
        let board_good: Board = "4k3/8/8/8/8/4B3/2P1P1P1/4K3 w - - 0 1".parse().unwrap();
        let good = evaluate(&board_good, &mut eval);
        assert!(good >= bad, "Good bishop complex should score at least as well: good={}, bad={}", good, bad);
    }

    #[test]
    fn space_advantage_open_position() {
        // White has pieces deep in Black's territory
        let board: Board = "r1bqkb1r/pppp1ppp/2n2n2/4N3/2B1P3/8/PPPP1PPP/RNBQK2R w KQkq - 0 1".parse().unwrap();
        let mut eval = Evaluator::new();
        let s = evaluate(&board, &mut eval);
        assert!(s > 0, "White with active pieces should have positive eval, got {}", s);
    }
}
