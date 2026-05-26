use cozy_chess::*;
use std::io::{self, BufRead, Write};

use crate::search::Searcher;

// broken pipe safe - gui may close the pipe at any time
#[inline]
pub fn uci_send(msg: &str) {
    let stdout = io::stdout();
    let mut lock = stdout.lock();
    if writeln!(lock, "{}", msg).is_err() {
        std::process::exit(0);
    }
    let _ = lock.flush();
}

pub fn run_loop() {
    let stdin = io::stdin();
    let mut searcher = Searcher::new(64);
    let mut board = Board::default();
    let mut root_history = vec![board.hash()];

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let tokens: Vec<&str> = line.trim().split_whitespace().collect();
        if tokens.is_empty() { continue; }

        match tokens[0] {
            "uci" => {
                uci_send("id name MATH-Sigma");
                uci_send("id author Spenc");
                uci_send("uciok");
            }
            "isready" => {
                uci_send("readyok");
            }
            "ucinewgame" => {
                searcher = Searcher::new(64);
                board = Board::default();
                root_history = vec![board.hash()];
            }
            "position" => {
                root_history = parse_position(&tokens, &mut board);
            }
            "go" => {
                let (depth, time_ms) = parse_go(&tokens, &board);
                searcher.set_root_history(root_history.clone());
                let result = searcher.search(&board, depth, time_ms);
                if let Some(mv) = result.best_move {
                    uci_send(&format!("bestmove {}", crate::types::move_to_uci(mv, &board)));
                } else {
                    // fallback: pick first legal move
                    let mut moves = Vec::new();
                    board.generate_moves(|mvs| { moves.extend(mvs); false });
                    if let Some(mv) = moves.first() {
                        uci_send(&format!("bestmove {}", crate::types::move_to_uci(*mv, &board)));
                    } else {
                        uci_send("bestmove 0000");
                    }
                }
            }
            "quit" => break,
            _ => {}
        }
    }
}

fn parse_position(tokens: &[&str], board: &mut Board) -> Vec<u64> {
    let mut idx = 1;
    if idx >= tokens.len() { return vec![board.hash()]; }

    if tokens[idx] == "startpos" {
        *board = Board::default();
        idx += 1;
    } else if tokens[idx] == "fen" {
        idx += 1;
        let mut fen_parts = Vec::new();
        while idx < tokens.len() && tokens[idx] != "moves" {
            fen_parts.push(tokens[idx]);
            idx += 1;
        }
        let fen = fen_parts.join(" ");
        if let Ok(b) = fen.parse::<Board>() {
            *board = b;
        }
    }

    let mut history = vec![board.hash()];
    if idx < tokens.len() && tokens[idx] == "moves" {
        idx += 1;
        while idx < tokens.len() {
            if let Ok(mv) = cozy_chess::util::parse_uci_move(board, tokens[idx]) {
                // validate legality before playing
                let mut legal = false;
                board.generate_moves(|mvs| {
                    for m in mvs {
                        if m == mv {
                            legal = true;
                            return true;
                        }
                    }
                    false
                });
                if legal {
                    board.play_unchecked(mv);
                    history.push(board.hash());
                }
            }
            idx += 1;
        }
    }
    history
}

struct TimeControl {
    soft_limit: u64,
    hard_limit: u64,
}

fn parse_go(tokens: &[&str], board: &Board) -> (i32, u64) {
    let mut depth = 64i32;
    let mut wtime: Option<u64> = None;
    let mut btime: Option<u64> = None;
    let mut winc: u64 = 0;
    let mut binc: u64 = 0;
    let mut movestogo: Option<u64> = None;
    let mut movetime: Option<u64> = None;
    let mut infinite = false;

    let mut i = 1;
    while i < tokens.len() {
        match tokens[i] {
            "depth" => {
                if i + 1 < tokens.len() {
                    depth = tokens[i + 1].parse().unwrap_or(64);
                    i += 1;
                }
            }
            "movetime" => {
                if i + 1 < tokens.len() {
                    movetime = Some(tokens[i + 1].parse().unwrap_or(u64::MAX));
                    i += 1;
                }
            }
            "wtime" => {
                if i + 1 < tokens.len() {
                    wtime = Some(tokens[i + 1].parse().unwrap_or(60000));
                    i += 1;
                }
            }
            "btime" => {
                if i + 1 < tokens.len() {
                    btime = Some(tokens[i + 1].parse().unwrap_or(60000));
                    i += 1;
                }
            }
            "winc" => {
                if i + 1 < tokens.len() {
                    winc = tokens[i + 1].parse().unwrap_or(0);
                    i += 1;
                }
            }
            "binc" => {
                if i + 1 < tokens.len() {
                    binc = tokens[i + 1].parse().unwrap_or(0);
                    i += 1;
                }
            }
            "movestogo" => {
                if i + 1 < tokens.len() {
                    movestogo = Some(tokens[i + 1].parse().unwrap_or(30));
                    i += 1;
                }
            }
            "infinite" => {
                infinite = true;
            }
            _ => {}
        }
        i += 1;
    }

    if let Some(mt) = movetime {
        return (depth, mt);
    }

    if infinite {
        return (depth, u64::MAX);
    }

    let our_time = if board.side_to_move() == Color::White {
        wtime.unwrap_or(60000)
    } else {
        btime.unwrap_or(60000)
    };
    let our_inc = if board.side_to_move() == Color::White { winc } else { binc };

    let moves_left = movestogo.unwrap_or(30u64);

    let base = our_time / moves_left.max(1) + our_inc * 9 / 10;
    let hard = (base * 3).min(our_time * 3 / 10).max(50);
    (depth, hard)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_position_accepts_standard_uci_white_castling() {
        let mut board = Board::default();
        let tokens = [
            "position", "startpos", "moves",
            "e2e4", "e7e5", "g1f3", "b8c6", "f1e2", "g8f6", "e1g1",
        ];

        parse_position(&tokens, &mut board);

        assert_eq!(board.piece_on("g1".parse().unwrap()), Some(Piece::King));
        assert_eq!(board.color_on("g1".parse().unwrap()), Some(Color::White));
        assert_eq!(board.piece_on("f1".parse().unwrap()), Some(Piece::Rook));
        assert_eq!(board.color_on("f1".parse().unwrap()), Some(Color::White));
        assert_eq!(board.piece_on("e1".parse().unwrap()), None);
        assert_eq!(board.piece_on("h1".parse().unwrap()), None);
    }

    #[test]
    fn parse_position_accepts_standard_uci_black_castling() {
        let mut board = Board::default();
        let tokens = [
            "position", "startpos", "moves",
            "e2e4", "e7e5", "g1f3", "b8c6", "f1e2", "g8f6",
            "e1g1", "f8e7", "d2d3", "e8g8",
        ];

        parse_position(&tokens, &mut board);

        assert_eq!(board.piece_on("g8".parse().unwrap()), Some(Piece::King));
        assert_eq!(board.color_on("g8".parse().unwrap()), Some(Color::Black));
        assert_eq!(board.piece_on("f8".parse().unwrap()), Some(Piece::Rook));
        assert_eq!(board.color_on("f8".parse().unwrap()), Some(Color::Black));
        assert_eq!(board.piece_on("e8".parse().unwrap()), None);
        assert_eq!(board.piece_on("h8".parse().unwrap()), None);
    }

    #[test]
    fn parse_position_accepts_standard_uci_queenside_castling() {
        let mut board = Board::default();
        let tokens = [
            "position", "startpos", "moves",
            "d2d4", "d7d5", "b1c3", "b8c6", "c1e3", "c8e6",
            "d1d2", "d8d7", "e1c1", "e8c8",
        ];

        parse_position(&tokens, &mut board);

        assert_eq!(board.piece_on("c1".parse().unwrap()), Some(Piece::King));
        assert_eq!(board.color_on("c1".parse().unwrap()), Some(Color::White));
        assert_eq!(board.piece_on("d1".parse().unwrap()), Some(Piece::Rook));
        assert_eq!(board.color_on("d1".parse().unwrap()), Some(Color::White));
        assert_eq!(board.piece_on("c8".parse().unwrap()), Some(Piece::King));
        assert_eq!(board.color_on("c8".parse().unwrap()), Some(Color::Black));
        assert_eq!(board.piece_on("d8".parse().unwrap()), Some(Piece::Rook));
        assert_eq!(board.color_on("d8".parse().unwrap()), Some(Color::Black));
    }
}
