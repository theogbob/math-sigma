# MATH-Sigma

A chess engine where every evaluation constant is derived from mathematics. No neural networks, no training data, no learned weights, no tuning against games. Every number in the evaluation comes from the rules of chess, board geometry, or mathematical first principles.

## The Constraint

Traditional chess engines tune thousands of parameters against millions of games. NNUE engines use neural networks trained on billions of positions. MATH-Sigma does neither.

Every constant is derived from three axioms about the structure of chess:

- **b = 35** - the average branching factor (a property of legal move generation)
- **b_eff ~ 7.4** - the effective branching factor after move ordering (TT + killers + good captures)
- **T = 78cp** - the tempo value, derived as `P * ln(b) / (1 + ln(b))`

From these, all search parameters follow: aspiration windows, null move reduction depths, LMR coefficients, futility margins, reverse futility margins, and probcut thresholds. The evaluation uses piece values from standard exchange ratios (P=100, N=320, B=330, R=500, Q=900), piece-square tables derived from board geometry, and positional features (mobility, king safety, pawn structure, passed pawns) weighted through mathematical relationships between the features and game-tree properties.

Nothing was tuned by playing games. If a constant appears in the code, there is a derivation for why it has that value.

## Strength

Tested against Stockfish at fixed depth limits, 1 second per move for MATH-Sigma, 8 games per depth:

| Opponent | Result | Elo Estimate |
|----------|--------|--------------|
| Stockfish depth 6 (~2100) | 5W 1D 2L | ~2237 |
| Stockfish depth 7 (~2250) | 3W 1D 4L | ~2206 |
| Stockfish depth 8 (~2400) | 1W 0D 7L | ~2062 |

Realistic playing strength is around **2200 Elo**, which corresponds to strong amateur or National Master level. The engine wins tactically at shorter time controls but struggles in long endgames where Stockfish's deeper search and superior eval take over.

For context: no documented chess engine has reached above ~2500 Elo using purely mathematically derived constants. Empirically tuned hand-crafted evaluation engines have reached ~2800-2900 (Fruit, Crafty, Rebel). NNUE engines exceed 3500. The measurable gap between "derived from math" and "tuned against games" appears to be roughly 600-800 Elo.

Note: 8-game matches carry wide confidence intervals (~+-200 Elo). These numbers establish a range, not a precise rating.

## Architecture

~3200 lines of Rust.

```
src/
  main.rs        entry point + benchmark harness (10 positions)
  lib.rs         module declarations
  types.rs       piece values, derived constants, score types
  tt.rs          transposition table (64MB, generation-based replacement)
  uci.rs         UCI protocol handler + time management
  eval/
    mod.rs       evaluation function (~1350 lines)
    pst.rs       piece-square tables
  search/
    mod.rs       alpha-beta search engine (~1000 lines)
```

### Evaluation

The evaluation is a hand-crafted, integer-only function with midgame/endgame interpolation on a 24-point phase system. All arithmetic on the hot path uses integers or Q8 fixed-point to avoid floating-point overhead.

Features:
- Material and piece-square tables (phase-interpolated)
- Symmetric mobility computation for both sides (sqrt-scaled, phase-weighted)
- Quadratic king attack model (attack units squared, safe check bonuses, pawn shield penalties)
- Pawn structure with hash caching (isolated, doubled, connected, passed pawns)
- Passed pawn evaluation (blockade detection, rook behind passer, king proximity, rule of the square, promotion corridor control)
- Bishop pair bonus scaled by position openness
- Rook bonuses on open/semi-open files with enemy king proximity weighting
- Knight outposts, bishop color complex penalty, space advantage, hanging piece detection, pawn-on-minor threats
- Closed-form endgame solvers: KQK, KRK, KBNK, KBBK, KPK, KBP (wrong bishop), KQ vs KP (fortress)
- Endgame scaling: opposite-color bishops, KR+minor vs KR, mating potential assessment
- Rook endgame geometry: R vs P(s), R+P vs R with Lucena/Philidor pattern recognition
- Lazy evaluation cutoff when material imbalance exceeds 400cp

### Search

- Alpha-beta with principal variation search, iterative deepening
- Volatility-adaptive aspiration windows (base window + inter-iteration score instability)
- Null move pruning with zugzwang detection and verification search at high depth
- Late move reductions with history and continuation-history adjustment
- Reverse futility pruning, futility pruning, razoring (all margins derived from tempo and capture probability)
- ProbCut with SEE-positive captures only
- Singular extensions, recapture extensions, pawn-to-7th extensions, check extensions
- Late move pruning and history-based pruning applied before board clone (pre-clone pruning)
- Losing capture reduction based on static exchange evaluation
- Move ordering: TT move, SEE-scored captures, killer moves, countermove heuristic, history + continuation history for quiets
- Quiescence search with SEE-ordered captures, delta pruning, and volatility-gated quiet checks
- In-search repetition detection with root game history
- Time management with move instability detection (stable best move exits early, unstable extends)

## Building

Requires Rust and a CPU with PEXT support (most modern x86-64 processors).

```bash
RUSTFLAGS="-C target-cpu=native" cargo build --release
```

Run the engine (UCI protocol):
```bash
./target/release/superchess-rs
```

Benchmark (10 positions, depth 8):
```bash
./target/release/superchess-rs --bench
```

Run tests:
```bash
RUSTFLAGS="-C target-cpu=native" cargo test
```

## Usage

MATH-Sigma speaks the UCI protocol. Connect it to any UCI-compatible GUI:

- [Arena](http://www.playwitharena.de/)
- [CuteChess](https://cutechess.com/)
- [Banksia](https://banksiagui.com/)

Or from the command line:
```bash
echo -e "uci\nisready\nposition startpos\ngo depth 10\nquit" | ./target/release/superchess-rs
```

## Findings

Over 35 experiments were tested during development across multiple sessions. Most were rejected. The surviving findings:

**Speed beats complexity.** The only change that consistently improved match results was making the evaluation faster (lazy eval cutoff), not adding more evaluation terms. At ~1M nodes per second, the Elo gained from an extra ply of search depth outweighs the Elo from more nuanced positional scoring.

**Evaluation noise is the binding constraint.** In an additive linear evaluation (score = sum of weighted features), each new term contributes both signal and noise. Past roughly 100 features, marginal terms add noise approximately equal to signal. This appears to be an inherent property of the architecture, not something better mathematics can resolve within the same framework.

**Mathematical derivation works well for material and search parameters.** Piece values, pruning margins, reduction formulas, and aspiration windows all derive cleanly from branching factor analysis and game-tree properties. The derivations break down for positional evaluation weights, where the mapping from board features to centipawn values is fundamentally empirical.

**~2200 Elo appears to be the ceiling for this approach.** Not because the mathematics is wrong, but because a linear additive evaluation cannot represent the positional patterns that separate 2200 from 2800. Reaching higher would likely require a conditional or nonlinear evaluation architecture, not better parameter values within the current one.

## Dependencies

- [cozy-chess](https://crates.io/crates/cozy-chess) - move generation with PEXT bitboards
- [arrayvec](https://crates.io/crates/arrayvec) - stack-allocated move lists
