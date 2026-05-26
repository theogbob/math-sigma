mod types;
mod tt;
mod eval;
mod search;
mod uci;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "--bench" {
        run_bench();
        return;
    }
    uci::run_loop();
}

fn run_bench() {
    use cozy_chess::Board;
    use std::time::Instant;

    let positions = [
        ("Starting", "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"),
        ("Kiwipete", "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1"),
        ("SF +7.42", "r3k1nr/pppqn2p/5p2/b5p1/3PP3/1BP2QBP/PP1N2P1/R3K2R w KQkq - 3 14"),
        ("Italian", "r1bqkb1r/pppp1ppp/2n2n2/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4"),
        ("Castled", "r1bq1rk1/ppp2ppp/2np1n2/2b1p3/2B1P3/2NP1N2/PPP2PPP/R1BQ1RK1 w - - 0 7"),
        ("French Lock", "rnbqkbnr/pp3ppp/2p1p3/3pP3/3P4/2N5/PPP2PPP/R1BQKBNR w KQkq - 0 5"),
        ("Passed Race", "8/2p5/3p4/1P3p2/5P1k/4P1p1/6P1/4K3 w - - 0 1"),
        ("Rook vs Pawn", "8/8/8/1P6/K7/8/6k1/7r w - - 0 1"),
        ("Queen Tension", "r2q1rk1/ppp2ppp/2n2n2/2bpp3/4P3/2PP1N1P/PP3PP1/RNBQ1RK1 w - - 0 9"),
        ("Endgame", "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1"),
    ];

    let mut searcher = search::Searcher::new(64);
    let mut total_nodes: u64 = 0;
    let total_start = Instant::now();

    for (name, fen) in &positions {
        let board: Board = fen.parse().unwrap();
        searcher.tt.clear();
        let start = Instant::now();
        let result = searcher.search(&board, 8, 30_000);
        let elapsed = start.elapsed();
        let nps = if elapsed.as_millis() > 0 {
            result.nodes * 1000 / elapsed.as_millis() as u64
        } else { 0 };
        let mv = result.best_move.map(|m| types::move_to_uci(m, &board)).unwrap_or("none".into());
        println!("{:>12}: {:+6}cp d{} {:>8} nodes {:>8} nps {:.1}s  move={}",
            name, result.score, result.depth, result.nodes, nps,
            elapsed.as_secs_f64(), mv);
        total_nodes += result.nodes;
    }

    let total_elapsed = total_start.elapsed();
    let total_nps = if total_elapsed.as_millis() > 0 {
        total_nodes * 1000 / total_elapsed.as_millis() as u64
    } else { 0 };
    println!("\nTotal: {} nodes in {:.1}s ({} nps)",
        total_nodes, total_elapsed.as_secs_f64(), total_nps);
}
