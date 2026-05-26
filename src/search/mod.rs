use cozy_chess::*;
use arrayvec::ArrayVec;
use std::time::Instant;

use crate::types::*;
use crate::tt::*;
use crate::eval::{self, Evaluator};
use crate::uci::uci_send;

pub struct SearchResult {
    pub best_move: Option<Move>,
    pub score: Score,
    pub depth: i32,
    pub nodes: u64,
    pub time_ms: u64,
    pub pv: Vec<Move>,
}

// continuation history: [prev_piece][prev_to][cur_piece][cur_to]
// captures 2-ply move correlations for quiet ordering
type ContHistory = Box<[[[[i16; 64]; 6]; 64]; 6]>;

fn new_cont_history() -> ContHistory {
    Box::new([[[[0i16; 64]; 6]; 64]; 6])
}

pub struct Searcher {
    pub nodes: u64,
    pub tt: TranspositionTable,
    pub evaluator: Evaluator,
    root_history: Vec<u64>,
    killers: [[Option<Move>; 2]; MAX_PLY],
    history: [[[i16; 64]; 64]; 2],
    cont_history: ContHistory,
    countermove: [[[Option<Move>; 64]; 64]; 2],
    move_stack: [(usize, usize); MAX_PLY],
    hash_stack: [u64; MAX_PLY],
    eval_stack: [Score; MAX_PLY],
    start_time: Instant,
    time_limit_ms: u64,
    stop: bool,
}

impl Searcher {
    pub fn new(tt_mb: usize) -> Self {
        Self {
            nodes: 0,
            tt: TranspositionTable::new(tt_mb),
            evaluator: Evaluator::new(),
            root_history: Vec::new(),
            killers: [[None; 2]; MAX_PLY],
            history: [[[0i16; 64]; 64]; 2],
            cont_history: new_cont_history(),
            countermove: [[[None; 64]; 64]; 2],
            move_stack: [(0, 0); MAX_PLY],
            hash_stack: [0u64; MAX_PLY],
            eval_stack: [0; MAX_PLY],
            start_time: Instant::now(),
            time_limit_ms: u64::MAX,
            stop: false,
        }
    }

    pub fn set_root_history(&mut self, history: Vec<u64>) {
        self.root_history = history;
    }

    pub fn search(&mut self, board: &Board, max_depth: i32, time_limit_ms: u64) -> SearchResult {
        self.nodes = 0;
        self.start_time = Instant::now();
        self.time_limit_ms = time_limit_ms;
        self.stop = false;
        self.tt.new_search();
        // history gravity: halve so recent moves dominate
        for c in 0..2 {
            for f in 0..64 {
                for t in 0..64 {
                    self.history[c][f][t] /= 2;
                }
            }
        }

        let mut best_result = SearchResult {
            best_move: None, score: 0, depth: 0, nodes: 0, time_ms: 0, pv: vec![],
        };
        let mut prev_score: Score = 0;
        let mut prev_prev_score: Score = 0;
        let mut prev_best_move: Option<Move> = None;
        let mut best_move_changes: u32 = 0;

        for depth in 1..=max_depth {
            let mut pv = Vec::new();
            let mut score;

            if depth >= 4 {
                // aspiration window widens with score volatility between iterations
                let volatility = if depth >= 5 {
                    (prev_score - prev_prev_score).unsigned_abs() as Score / 4
                } else { 0 };
                let mut delta = ASP_WINDOW + volatility;
                let mut a = prev_score - delta;
                let mut b = prev_score + delta;
                score = prev_score;
                loop {
                    pv.clear();
                    let s = self.alpha_beta(board, depth, a, b, &mut pv, 0, 0, None, None);
                    if self.stop { break; }
                    if s > a && s < b {
                        score = s;
                        break;
                    }
                    if s <= a {
                        delta *= 2;
                        a = if delta > 300 { -INF_SCORE } else { prev_score - delta };
                    } else {
                        delta *= 2;
                        b = if delta > 300 { INF_SCORE } else { prev_score + delta };
                    }
                }
            } else {
                score = self.alpha_beta(board, depth, -INF_SCORE, INF_SCORE, &mut pv, 0, 0, None, None);
            }

            if self.stop { break; }

            prev_prev_score = prev_score;
            prev_score = score;
            let elapsed = self.start_time.elapsed().as_millis() as u64;

            if !pv.is_empty() {
                // Track move instability: did the best move change?
                if depth >= 4 {
                    if prev_best_move.is_some() && prev_best_move != Some(pv[0]) {
                        best_move_changes += 1;
                    }
                    prev_best_move = Some(pv[0]);
                }

                best_result = SearchResult {
                    best_move: Some(pv[0]),
                    score,
                    depth,
                    nodes: self.nodes,
                    time_ms: elapsed,
                    pv: pv.clone(),
                };

                // Print UCI info (convert castling to standard notation)
                let nps = if elapsed > 0 { self.nodes * 1000 / elapsed } else { 0 };
                let mut pv_board = board.clone();
                let pv_str: String = pv.iter().map(|m| {
                    let uci = crate::types::move_to_uci(*m, &pv_board);
                    pv_board.play_unchecked(*m);
                    uci
                }).collect::<Vec<_>>().join(" ");
                uci_send(&format!("info depth {} score cp {} nodes {} time {} nps {} pv {}",
                    depth, score, self.nodes, elapsed, nps, pv_str));
            }

            // Soft time limit with move instability adjustment:
            // Stable best move → stop earlier (70% of limit)
            // Unstable best move → think longer (150% of limit)
            if time_limit_ms != u64::MAX {
                let stability_pct = if best_move_changes == 0 && depth >= 5 {
                    70  // stable: use 70% of time
                } else if best_move_changes >= 2 {
                    150 // unstable: use 150% of time
                } else {
                    85  // default: use 85% of time (was hardcoded before)
                };
                if elapsed > time_limit_ms.saturating_mul(stability_pct) / 100 {
                    break;
                }
            }
        }

        best_result
    }

    fn check_time(&mut self) {
        if self.nodes & 4095 == 0 {
            if self.start_time.elapsed().as_millis() as u64 > self.time_limit_ms {
                self.stop = true;
            }
        }
    }

    /// In-search repetition detection: check if position hash appeared before
    /// in the current search path OR in the root game history.
    /// Step by 2 through search path (same side to move) for correctness.
    fn is_repetition(&self, key: u64, ply: usize) -> bool {
        // Check search path: positions at ply-2, ply-4, ... (same color)
        if ply >= 2 {
            let mut i = ply - 2;
            loop {
                if self.hash_stack[i] == key { return true; }
                if i < 2 { break; }
                i -= 2;
            }
        }
        // Check root game history (positions before the search started)
        // A match here means the search path recreated a position from the game
        for &h in self.root_history.iter().rev() {
            if h == key { return true; }
        }
        false
    }

    fn is_en_passant_capture(&self, board: &Board, mv: Move) -> bool {
        board.piece_on(mv.from) == Some(Piece::Pawn)
            && board.piece_on(mv.to).is_none()
            && mv.from.file() != mv.to.file()
            && board.en_passant() == Some(mv.to.file())
    }

    fn is_capture(&self, board: &Board, mv: Move) -> bool {
        board.piece_on(mv.to).is_some() || self.is_en_passant_capture(board, mv)
    }

    fn has_non_pawn_material(&self, board: &Board, color: Color) -> bool {
        !(board.colored_pieces(color, Piece::Knight)
            | board.colored_pieces(color, Piece::Bishop)
            | board.colored_pieces(color, Piece::Rook)
            | board.colored_pieces(color, Piece::Queen))
            .is_empty()
    }

    fn attackers_to(&self, board: &Board, sq: Square, occupied: BitBoard, color: Color) -> BitBoard {
        let pieces = board.colors(color);
        let pawns = get_pawn_attacks(sq, !color) & pieces & board.pieces(Piece::Pawn);
        let knights = get_knight_moves(sq) & pieces & board.pieces(Piece::Knight);
        let bishops = get_bishop_moves(sq, occupied)
            & pieces
            & (board.pieces(Piece::Bishop) | board.pieces(Piece::Queen));
        let rooks = get_rook_moves(sq, occupied)
            & pieces
            & (board.pieces(Piece::Rook) | board.pieces(Piece::Queen));
        let kings = get_king_moves(sq) & pieces & board.pieces(Piece::King);
        pawns | knights | bishops | rooks | kings
    }

    fn least_attacker(&self, board: &Board, attackers: BitBoard) -> Option<(Square, Piece)> {
        for &piece in &[Piece::Pawn, Piece::Knight, Piece::Bishop, Piece::Rook, Piece::Queen, Piece::King] {
            let bb = attackers & board.pieces(piece);
            if let Some(sq) = bb.into_iter().next() {
                return Some((sq, piece));
            }
        }
        None
    }

    fn see(&self, board: &Board, mv: Move) -> Score {
        let Some(attacker) = board.piece_on(mv.from) else { return 0; };
        let mut gains = [0; 32];
        let mut depth = 0usize;
        let mut occupied = board.occupied();

        gains[0] = if let Some(victim) = board.piece_on(mv.to) {
            piece_value(victim)
        } else if self.is_en_passant_capture(board, mv) {
            PAWN_VALUE
        } else {
            0
        };

        if let Some(promo) = mv.promotion {
            gains[0] += piece_value(promo) - PAWN_VALUE;
        }

        occupied.0 &= !mv.from.bitboard().0;
        if board.piece_on(mv.to).is_none() {
            occupied.0 |= mv.to.bitboard().0;
        }
        if self.is_en_passant_capture(board, mv) {
            let ep_pawn = if board.side_to_move() == Color::White {
                mv.to.try_offset(0, -1)
            } else {
                mv.to.try_offset(0, 1)
            };
            if let Some(ep_sq) = ep_pawn {
                occupied.0 &= !ep_sq.bitboard().0;
            }
        }

        let mut side = !board.side_to_move();
        let mut target_value = mv.promotion.map(piece_value).unwrap_or_else(|| piece_value(attacker));

        loop {
            let attackers = self.attackers_to(board, mv.to, occupied, side) & occupied;
            let Some((from, piece)) = self.least_attacker(board, attackers) else { break; };
            depth += 1;
            if depth >= gains.len() {
                break;
            }
            gains[depth] = target_value - gains[depth - 1];
            occupied.0 &= !from.bitboard().0;
            target_value = piece_value(piece);
            side = !side;
        }

        while depth > 0 {
            depth -= 1;
            gains[depth] = -(-gains[depth]).max(gains[depth + 1]);
        }

        gains[0]
    }

    fn capture_score(&self, board: &Board, mv: Move) -> Score {
        let victim = board.piece_on(mv.to).map(piece_value).unwrap_or_else(|| {
            if self.is_en_passant_capture(board, mv) { PAWN_VALUE } else { 0 }
        });
        let attacker = board.piece_on(mv.from).map(piece_value).unwrap_or(0);
        let promo = mv.promotion.map(piece_value).unwrap_or(0);
        10_000 + self.see(board, mv) * 16 + victim * 4 + promo - attacker / 8
    }

    fn losing_capture_reduction(&self, board: &Board, mv: Move, depth: i32, move_index: usize) -> i32 {
        if depth < 3 || move_index < 4 || mv.promotion.is_some() {
            return 0;
        }
        let see_loss = -self.see(board, mv);
        if see_loss <= 0 {
            return 0;
        }

        let attacker = board.piece_on(mv.from).map(piece_value).unwrap_or(PAWN_VALUE).max(PAWN_VALUE);
        let victim = board.piece_on(mv.to).map(piece_value).unwrap_or_else(|| {
            if self.is_en_passant_capture(board, mv) { PAWN_VALUE } else { 0 }
        });

        // A losing capture is only reduced when the static exchange loss is a
        // meaningful fraction of the moving piece and larger than the captured
        // material's tactical forcing value. This keeps ordinary sacrifices and
        // equal exchanges searchable while pushing obvious blunders later.
        let material_margin = (attacker / 2).max(victim).max(PAWN_VALUE);
        if see_loss < material_margin {
            return 0;
        }

        let depth_pressure = depth / 4;
        let loss_pressure = (see_loss / PAWN_VALUE).min(2);
        (1 + depth_pressure + loss_pressure).clamp(1, depth - 2)
    }

    fn root_repetition_score(&mut self, board: &Board, child_hash: u64) -> Option<Score> {
        let repeats = self.root_history.iter().filter(|&&h| h == child_hash).count();
        if repeats == 0 {
            return None;
        }
        if repeats >= 2 {
            return Some(DRAW_SCORE);
        }

        let mut static_eval = eval::evaluate(board, &mut self.evaluator);
        if board.side_to_move() == Color::Black {
            static_eval = -static_eval;
        }

        if static_eval > PAWN_VALUE {
            Some(DRAW_SCORE - static_eval.min(QUEEN_VALUE))
        } else if static_eval < -PAWN_VALUE {
            Some(DRAW_SCORE)
        } else {
            Some(-PAWN_VALUE / 2)
        }
    }

    fn alpha_beta(
        &mut self, board: &Board, mut depth: i32,
        mut alpha: Score, beta: Score,
        pv: &mut Vec<Move>, ply: usize, mut extensions: i32,
        recapture_sq: Option<Square>, prev_move: Option<Move>,
    ) -> Score {
        self.nodes += 1;
        self.check_time();
        if self.stop { return 0; }

        const MAX_EXT: i32 = 3;
        let is_pv = beta - alpha > 1;

        // Store previous move info for continuation history lookups
        if let Some(pm) = prev_move {
            if ply > 0 && ply < MAX_PLY {
                let piece_idx = board.piece_on(pm.to).map(|p| p as usize).unwrap_or(0);
                self.move_stack[ply - 1] = (piece_idx, pm.to as usize);
            }
        }

        // Terminal
        let status = board.status();
        if status == GameStatus::Drawn {
            return DRAW_SCORE;
        }
        if status == GameStatus::Won {
            return -(MATE_SCORE - ply as Score);
        }

        // Record position hash for repetition detection
        let key = board.hash();
        if ply < MAX_PLY {
            self.hash_stack[ply] = key;
        }

        // In-search repetition: 2-fold = draw
        // (Root handled separately by root_repetition_score with contempt logic)
        if ply > 0 && ply < MAX_PLY && self.is_repetition(key, ply) {
            return DRAW_SCORE;
        }

        // TT probe
        let mut tt_move: Option<Move> = None;
        let mut tt_score: Option<Score> = None;
        let mut tt_depth: i32 = -1;
        let mut tt_eval: Option<Score> = None; // cached static eval from previous search
        if let Some(entry) = self.tt.probe(key) {
            let score = score_from_tt(entry.score, ply);
            tt_depth = entry.depth as i32;
            tt_score = Some(score);
            if entry.static_eval != 0 {
                tt_eval = Some(entry.static_eval);
            }
            if tt_depth >= depth {
                match entry.bound {
                    Bound::Exact if ply > 0 => return score,
                    Bound::Lower if score >= beta => return score,
                    Bound::Upper if score <= alpha => return score,
                    _ => {}
                }
            }
            tt_move = entry.best_move;
        }

        let in_check = !board.checkers().is_empty();

        // Internal iterative reduction: no TT move at deep nodes → reduce 1
        if tt_move.is_none() && depth >= 4 && !in_check {
            depth -= 1;
        }

        // Leaf
        if depth <= 0 {
            if in_check && extensions < MAX_EXT {
                depth = 1;
                extensions += 1;
            } else {
                return self.quiesce(board, alpha, beta, 0);
            }
        }

        // Check extension
        if in_check && depth > 0 && extensions < MAX_EXT {
            depth += 1;
            extensions += 1;
        }

        // Static eval for pruning decisions
        // Always compute fresh eval — TT-cached eval can be from a different search context
        // and pruning decisions are sensitive to eval accuracy
        let mut static_eval: Score = 0;
        let need_static = !in_check;
        if need_static {
            static_eval = eval::evaluate(board, &mut self.evaluator);
            if board.side_to_move() == Color::Black {
                static_eval = -static_eval;
            }
            if ply < MAX_PLY {
                self.eval_stack[ply] = static_eval;
            }
        }

        // "Improving" heuristic: is our static eval better than 2 plies ago?
        // If improving, we prune less aggressively; if not, we prune more.
        let improving = if need_static && ply >= 2 {
            static_eval > self.eval_stack[ply - 2]
        } else {
            false
        };

        // Reverse futility (skip at root) — derived from tempo + capture probability
        let rfp_depth = if improving { 3 } else { 4 };
        if need_static && depth <= rfp_depth && ply > 0 && !is_pv {
            let margin = if improving { RFP_IMPROVING } else { RFP_NOT_IMPROVING };
            if static_eval - margin * depth >= beta {
                return static_eval;
            }
        }

        // Futility flag — derived from RFP + safety buffer (sigma_cap)
        let fut_margin = if improving { FUT_IMPROVING } else { FUT_NOT_IMPROVING };
        let futility = need_static && depth <= 2 && !is_pv && static_eval + fut_margin * depth < alpha;

        // Razoring: if eval + large margin < alpha, verify with qsearch
        // Margin derived from: 3 * tempo + capture swing ≈ 3*78 + 40 ≈ 274 → ~3*T per ply
        if need_static && depth <= 4 && !futility && ply > 0 && !is_pv {
            if static_eval + (3 * TEMPO + V_CAP_REALIZED / 3) * depth < alpha {
                let q = self.quiesce(board, alpha - 1, alpha, 0);
                if q < alpha { return q; }
            }
        }

        // Null move pruning — skip in zugzwang-prone positions
        // Zugzwang risk: few pieces, no queens, at most one non-pawn piece per side
        let stm = board.side_to_move();
        let zugzwang_prone = board.occupied().len() <= 6
            && board.pieces(Piece::Queen).is_empty()
            && (board.colored_pieces(stm, Piece::Rook).len()
                + board.colored_pieces(stm, Piece::Knight).len()
                + board.colored_pieces(stm, Piece::Bishop).len()) <= 1;
        if depth >= 3
            && !in_check
            && ply > 0
            && self.has_non_pawn_material(board, stm)
            && static_eval >= beta
            && !zugzwang_prone
        {
            // R derived from: sqrt(d) base + d/(2*ln(b_eff)) depth term + eval surplus/(2*T)
            let r = 3 + depth / 4 + ((static_eval - beta) / (2 * TEMPO)).min(3);
            let null_board = match board.null_move() {
                Some(b) => b,
                None => { board.clone() },
            };
            let null_score = -self.alpha_beta(&null_board, depth - 1 - r, -beta, -beta + 1, &mut vec![], ply + 1, extensions, None, None);
            if null_score >= beta {
                // Verification at high depths
                if depth >= 12 {
                    let v = self.alpha_beta(board, depth - r, beta - 1, beta, &mut vec![], ply, extensions, recapture_sq, prev_move);
                    if v >= beta { return beta; }
                } else {
                    return beta;
                }
            }
        }

        // ProbCut: if a shallow search with a raised beta still exceeds beta,
        // the position is almost certainly above beta at full depth.
        // Only try with good captures/promotions to avoid false positives.
        if depth >= 5 && !in_check && ply > 0 && !is_pv && need_static {
            let probcut_beta = beta + PROBCUT_MARGIN;
            let probcut_depth = depth - 4;
            let mut probcut_moves = ArrayVec::<Move, 64>::new();
            board.generate_moves(|mvs| {
                for mv in mvs {
                    if self.is_capture(board, mv) && self.see(board, mv) > 0 {
                        probcut_moves.push(mv);
                    }
                }
                false
            });
            for mv in &probcut_moves {
                let mut child = board.clone();
                child.play_unchecked(*mv);
                let score = -self.alpha_beta(&child, probcut_depth, -probcut_beta, -probcut_beta + 1, &mut vec![], ply + 1, extensions, None, Some(*mv));
                if score >= probcut_beta {
                    return score;
                }
            }
        }

        // Staged move picking — TT move → captures → killers → countermove → quiets by history+cont
        let cm = self.get_countermove(board, prev_move);
        let prev_info = if ply > 0 { Some(self.move_stack[ply - 1]) } else { None };
        let moves = self.pick_moves(board, ply, tt_move, cm, prev_info);

        let mut best_score = -INF_SCORE;
        let mut best_move: Option<Move> = None;
        let mut bound = Bound::Upper;
        let mut quiets_tried = 0;

        // Singular extension detection: if we have a TT move at sufficient depth,
        // check if it's significantly better than all alternatives
        let singular_candidate = !in_check
            && extensions < MAX_EXT
            && depth >= 8
            && tt_move.is_some()
            && tt_depth >= depth - 3
            && tt_score.is_some();

        for (i, &mv) in moves.iter().enumerate() {
            let is_capture = self.is_capture(board, mv);
            let is_promotion = mv.promotion.is_some();

            // --- Pre-clone pruning: skip moves before paying the board.clone() cost ---
            // These checks don't need gives_check, so we can do them cheaply.

            // LMP: prune late quiet moves at shallow depth (no gives_check needed —
            // we accept the small risk of pruning a checking move, which LMP at d≤3
            // rarely matters for since checks are usually captures or promotions)
            if !is_pv && !in_check && depth <= 3 && !is_capture && !is_promotion && ply > 0 && i > 0 {
                let lmp_threshold = if improving {
                    3 + depth * depth
                } else {
                    2 + depth * depth / 2
                };
                if quiets_tried >= lmp_threshold {
                    if !is_capture && !is_promotion { quiets_tried += 1; }
                    continue;
                }
            }

            // History-based pruning: skip quiet moves with terrible history
            if !is_pv && !in_check && depth <= 4 && !is_capture && !is_promotion && ply > 0 && i > 0 {
                let color_idx = if board.side_to_move() == Color::White { 0 } else { 1 };
                let hist = self.history[color_idx][mv.from as usize][mv.to as usize] as i32;
                if hist < -(depth as i32 * 1024) {
                    if !is_capture && !is_promotion { quiets_tried += 1; }
                    continue;
                }
            }

            // --- Now clone the board (only for moves that survived pre-clone pruning) ---
            let mut child = board.clone();
            child.play_unchecked(mv);
            let gives_check = !child.checkers().is_empty();

            // Futility: skip quiet non-checking moves
            if futility && !is_capture && !is_promotion && !gives_check && i > 0 {
                continue;
            }

            if !is_capture && !is_promotion {
                quiets_tried += 1;
            }

            // Singular extension
            let mut singular_ext = 0;
            if singular_candidate && mv == tt_move.unwrap() {
                let s_beta = tt_score.unwrap() - depth * TEMPO / BRANCHING_FACTOR as Score;
                let s_score = self.singular_search(board, depth / 2 - 1, s_beta, ply, extensions, recapture_sq, prev_move, mv);
                if s_score < s_beta {
                    singular_ext = 1;
                }
            }

            // LMR
            let mut reduction = 0;
            if ply > 0 && i >= 3 && depth >= 3 && !gives_check && !is_capture && !is_promotion && !in_check {
                // LMR: 1/ln(b_eff) * ln(depth) * ln(move_index) — information-theoretic
                reduction = ((depth as f64).ln() * ((i + 1) as f64).ln() * LMR_COEFF) as i32;
                // Reduce more when not improving
                if !improving { reduction += 1; }
                // Reduce less in PV nodes
                if is_pv { reduction = (reduction - 1).max(0); }
                // History-based LMR: reduce less for good history, more for bad
                // History saturates at ~16384 (gravity denominator), /8192 maps to [-2,+2] adjustment
                let color_idx = if board.side_to_move() == Color::White { 0 } else { 1 };
                let hist = self.history[color_idx][mv.from as usize][mv.to as usize] as i32;
                let ch = if ply > 0 {
                    let (prev_piece, prev_to) = self.move_stack[ply - 1];
                    board.piece_on(mv.from)
                        .map(|p| self.cont_history[prev_piece][prev_to][p as usize][mv.to as usize] as i32)
                        .unwrap_or(0)
                } else { 0 };
                reduction -= (hist + ch) / 8192;
                reduction = reduction.clamp(0, depth - 2);
            }
            if ply > 0 && is_capture && !gives_check && !in_check {
                reduction = reduction.max(self.losing_capture_reduction(board, mv, depth, i));
            }

            let recapture_extension = if is_capture
                && recapture_sq == Some(mv.to)
                && depth <= MAX_EXT + 2
                && extensions < MAX_EXT
            {
                1
            } else {
                0
            };
            // Pawn push to 7th rank extension: near-promotion pushes in endgames
            // are critical moves that determine the game outcome
            let pawn_push_ext = if extensions < MAX_EXT
                && !is_capture
                && board.piece_on(mv.from) == Some(Piece::Pawn)
            {
                let to_rank = mv.to.rank() as usize;
                let promo_dist = if board.side_to_move() == Color::White { 7 - to_rank } else { to_rank };
                if promo_dist <= 1 { 1 } else { 0 }
            } else { 0 };
            let total_ext = recapture_extension + singular_ext + pawn_push_ext;
            let child_extensions = extensions + total_ext;
            let child_depth = depth - 1 + total_ext;
            let next_recapture_sq = if is_capture { Some(mv.to) } else { None };
            let mut child_pv = Vec::new();
            let score;

            let root_repeat_score = if ply == 0 {
                self.root_repetition_score(board, child.hash())
            } else {
                None
            };

            if let Some(repetition_score) = root_repeat_score {
                score = repetition_score;
            } else if i == 0 {
                score = -self.alpha_beta(&child, child_depth, -beta, -alpha, &mut child_pv, ply + 1, child_extensions, next_recapture_sq, Some(mv));
            } else {
                let reduced_depth = (child_depth - reduction).max(0);
                let s = -self.alpha_beta(&child, reduced_depth, -alpha - 1, -alpha, &mut child_pv, ply + 1, child_extensions, next_recapture_sq, Some(mv));
                if s > alpha {
                    child_pv.clear();
                    score = -self.alpha_beta(&child, child_depth, -beta, -alpha, &mut child_pv, ply + 1, child_extensions, next_recapture_sq, Some(mv));
                } else {
                    score = s;
                }
            }

            if self.stop { return 0; }

            if score > best_score {
                best_score = score;
                best_move = Some(mv);
                if score > alpha {
                    alpha = score;
                    bound = Bound::Exact;
                    pv.clear();
                    pv.push(mv);
                    pv.extend_from_slice(&child_pv);
                }
            }

            if alpha >= beta {
                bound = Bound::Lower;
                if !is_capture && !is_promotion {
                    self.store_killer(mv, ply);
                    self.update_history(board.side_to_move(), mv, depth);
                    // Store countermove
                    if let Some(pm) = prev_move {
                        self.store_countermove(board, pm, mv);
                    }
                    // Update continuation history for the cutoff move
                    if ply > 0 {
                        let (prev_piece, prev_to) = self.move_stack[ply - 1];
                        if let Some(piece) = board.piece_on(mv.from) {
                            let bonus = (depth * depth) as i32;
                            let entry = &mut self.cont_history[prev_piece][prev_to][piece as usize][mv.to as usize];
                            let val = *entry as i32;
                            let new_val = val + bonus - val * bonus.abs() / 16384;
                            *entry = new_val.clamp(-16384, 16384) as i16;
                        }
                    }
                }
                break;
            }
        }

        if let Some(bm) = best_move {
            self.tt.store(key, score_to_tt(best_score, ply), static_eval, depth as i8, bound, Some(bm));
        }

        best_score
    }

    fn quiesce(&mut self, board: &Board, mut alpha: Score, beta: Score, depth: i32) -> Score {
        self.nodes += 1;
        if self.stop { return 0; }

        let in_check = !board.checkers().is_empty();

        if in_check {
            if depth <= -12 {
                let mut s = eval::evaluate(board, &mut self.evaluator);
                if board.side_to_move() == Color::Black { s = -s; }
                return s;
            }
            let mut best = -INF_SCORE;
            let mut moves = ArrayVec::<Move, 256>::new();
            board.generate_moves(|mvs| { moves.extend(mvs); false });
            if moves.is_empty() { return -(MATE_SCORE - depth.unsigned_abs() as Score); }
            for mv in &moves {
                let mut child = board.clone();
                child.play_unchecked(*mv);
                let score = -self.quiesce(&child, -beta, -alpha, depth - 1);
                if score > best { best = score; }
                if score >= beta { return beta; }
                if score > alpha { alpha = score; }
            }
            return best;
        }

        // Stand pat
        let mut stand_pat = eval::evaluate(board, &mut self.evaluator);
        if board.side_to_move() == Color::Black { stand_pat = -stand_pat; }

        if depth <= -8 { return stand_pat; }
        if stand_pat >= beta { return beta; }
        if stand_pat > alpha { alpha = stand_pat; }

        // Generate captures + promotions only (skip quiet moves entirely)
        let mut captures = ArrayVec::<(Score, Move), 64>::new();
        let mut quiet_checks = ArrayVec::<Move, 32>::new();
        let mut capture_count: i32 = 0;

        // Precompute check squares to avoid board clones in quiet-check detection
        let occ = board.occupied();
        let ek = board.king(!board.side_to_move());
        let check_knight = get_knight_moves(ek);
        let check_bishop = get_bishop_moves(ek, occ);
        let check_rook = get_rook_moves(ek, occ);

        board.generate_moves(|mvs| {
            for mv in mvs {
                if self.is_capture(board, mv) || mv.promotion.is_some() {
                    captures.push((self.capture_score(board, mv), mv));
                    capture_count += 1;
                } else if depth >= -1 {
                    // Cheap check detection: does the destination square
                    // attack the enemy king for this piece type?
                    if let Some(piece) = board.piece_on(mv.from) {
                        let lands_check = match piece {
                            Piece::Knight => !(check_knight & BitBoard::from(mv.to)).is_empty(),
                            Piece::Bishop => !(check_bishop & BitBoard::from(mv.to)).is_empty(),
                            Piece::Rook => !(check_rook & BitBoard::from(mv.to)).is_empty(),
                            Piece::Queen => !((check_bishop | check_rook) & BitBoard::from(mv.to)).is_empty(),
                            // Pawn: check if it attacks the king from destination
                            Piece::Pawn => !(get_pawn_attacks(mv.to, board.side_to_move()) & BitBoard::from(ek)).is_empty(),
                            _ => false,
                        };
                        if lands_check {
                            quiet_checks.push(mv);
                        }
                    }
                }
            }
            false
        });
        captures.sort_unstable_by(|a, b| b.0.cmp(&a.0));

        for &(see_score, mv) in &captures {
            // Delta pruning
            if let Some(victim) = board.piece_on(mv.to) {
                let delta = stand_pat + piece_value(victim) + 200;
                if delta < alpha && mv.promotion.is_none() {
                    continue;
                }
            }
            if see_score < 8_400 && mv.promotion.is_none() {
                continue;
            }

            let mut child = board.clone();
            child.play_unchecked(mv);
            let score = -self.quiesce(&child, -beta, -alpha, depth - 1);
            if score >= beta { return beta; }
            if score > alpha { alpha = score; }
        }

        // Volatility-gated quiet checks: only at first qsearch ply when position is tense
        // Gate: multiple captures available OR stand-pat is close to the window
        if depth >= -1 && !quiet_checks.is_empty() {
            let volatile = capture_count >= 3
                || (stand_pat - alpha).abs() < PAWN_VALUE * 2;

            if volatile {
                let max_checks: usize = if depth >= 0 { 3 } else { 1 };
                for (i, &mv) in quiet_checks.iter().enumerate() {
                    if i >= max_checks { break; }
                    // SEE filter: skip checks that lose material
                    if self.see(board, mv) < 0 { continue; }
                    let mut child = board.clone();
                    child.play_unchecked(mv);
                    let score = -self.quiesce(&child, -beta, -alpha, depth - 1);
                    if score >= beta { return beta; }
                    if score > alpha { alpha = score; }
                }
            }
        }

        alpha
    }

    /// Staged move picker — avoids sorting all moves when early cutoff happens.
    /// Stage 1: TT move (if legal)
    /// Stage 2: Captures + promotions sorted by MVV-LVA
    /// Stage 3: Killers (if not already tried)
    /// Stage 3b: Countermove (if not already tried)
    /// Stage 4: Remaining quiets sorted by history
    fn pick_moves(&self, board: &Board, ply: usize, tt_move: Option<Move>, countermove: Option<Move>, prev_move_info: Option<(usize, usize)>) -> ArrayVec<Move, 256> {
        let mut result = ArrayVec::<Move, 256>::new();
        let mut all_moves = ArrayVec::<Move, 256>::new();
        board.generate_moves(|mvs| { all_moves.extend(mvs); false });

        let color_idx = if board.side_to_move() == Color::White { 0 } else { 1 };
        let mut used = [false; 256]; // track which moves are already emitted

        // Stage 1: TT move
        if let Some(ttm) = tt_move {
            for (i, &mv) in all_moves.iter().enumerate() {
                if mv == ttm { used[i] = true; result.push(mv); break; }
            }
        }

        // Stage 2: Captures + promotions (scored by SEE + MVV-LVA via capture_score)
        // capture_score already returns 10_000 + see*16 + victim*4 + promo - attacker/8
        // (previously had redundant vv - av/10 and promo bonus on top — removed)
        let mut caps: ArrayVec<(i32, usize), 64> = ArrayVec::new();
        for (i, &mv) in all_moves.iter().enumerate() {
            if used[i] { continue; }
            let is_cap = self.is_capture(board, mv);
            let is_promo = mv.promotion.is_some();
            if is_cap || is_promo {
                let score = self.capture_score(board, mv);
                caps.push((score, i));
            }
        }
        caps.sort_unstable_by(|a, b| b.0.cmp(&a.0));
        for &(_, i) in &caps { used[i] = true; result.push(all_moves[i]); }

        // Stage 3: Killers
        if ply < MAX_PLY {
            for k in &self.killers[ply] {
                if let Some(km) = k {
                    for (i, &mv) in all_moves.iter().enumerate() {
                        if !used[i] && mv == *km { used[i] = true; result.push(mv); break; }
                    }
                }
            }
        }

        // Stage 3b: Countermove
        if let Some(cm) = countermove {
            for (i, &mv) in all_moves.iter().enumerate() {
                if !used[i] && mv == cm { used[i] = true; result.push(mv); break; }
            }
        }

        // Stage 4: Remaining quiets sorted by history + continuation history
        let mut quiets: ArrayVec<(i32, usize), 256> = ArrayVec::new();
        for (i, &mv) in all_moves.iter().enumerate() {
            if used[i] { continue; }
            let h = self.history[color_idx][mv.from as usize][mv.to as usize] as i32;
            // Add continuation history bonus if previous move info available
            let ch = if let Some((prev_piece, prev_to)) = prev_move_info {
                if let Some(piece) = board.piece_on(mv.from) {
                    self.cont_history[prev_piece][prev_to][piece as usize][mv.to as usize] as i32
                } else { 0 }
            } else { 0 };
            quiets.push((h + ch, i));
        }
        quiets.sort_unstable_by(|a, b| b.0.cmp(&a.0));
        for &(_, i) in &quiets { result.push(all_moves[i]); }

        result
    }

    fn store_killer(&mut self, mv: Move, ply: usize) {
        if ply >= MAX_PLY { return; }
        if self.killers[ply][0] != Some(mv) {
            self.killers[ply][1] = self.killers[ply][0];
            self.killers[ply][0] = Some(mv);
        }
    }

    fn update_history(&mut self, color: Color, mv: Move, depth: i32) {
        let idx = if color == Color::White { 0 } else { 1 };
        let bonus = (depth * depth) as i32;
        let val = self.history[idx][mv.from as usize][mv.to as usize] as i32;
        // Gravity formula in i32 to prevent i16 overflow, then clamp back
        let new_val = val + bonus - val * bonus.abs() / 16384;
        self.history[idx][mv.from as usize][mv.to as usize] = new_val.clamp(-16384, 16384) as i16;
    }

    /// Singular extension validation: search the position excluding a specific move.
    /// Returns the best score found among all non-excluded moves.
    fn singular_search(
        &mut self, board: &Board, depth: i32, s_beta: Score,
        ply: usize, extensions: i32, recapture_sq: Option<Square>,
        prev_move: Option<Move>, excluded: Move,
    ) -> Score {
        let mut best = -INF_SCORE;
        let mut moves = ArrayVec::<Move, 256>::new();
        board.generate_moves(|mvs| { moves.extend(mvs); false });

        for &mv in &moves {
            if mv == excluded { continue; } // skip the TT move
            let mut child = board.clone();
            child.play_unchecked(mv);
            let score = -self.alpha_beta(&child, depth, -s_beta, -s_beta + 1, &mut vec![], ply + 1, extensions, recapture_sq, Some(mv));
            if score > best { best = score; }
            if best >= s_beta { return best; } // fail high — not singular
        }
        best
    }

    fn store_countermove(&mut self, board: &Board, prev: Move, refutation: Move) {
        let idx = if board.side_to_move() == Color::White { 1 } else { 0 }; // prev was opponent's move
        self.countermove[idx][prev.from as usize][prev.to as usize] = Some(refutation);
    }

    fn get_countermove(&self, board: &Board, prev_move: Option<Move>) -> Option<Move> {
        let pm = prev_move?;
        let idx = if board.side_to_move() == Color::White { 1 } else { 0 };
        self.countermove[idx][pm.from as usize][pm.to as usize]
    }
}
