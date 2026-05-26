# MATH-Sigma

A chess engine where every evaluation constant is derived from mathematics. No neural networks, no training data, no learned weights, no tuning against games. Every number in the evaluation comes from the rules of chess, board geometry, or mathematical first principles.

## the constraint

Traditional chess engines tune thousands of parameters against millions of games. NNUE engines use neural networks trained on billions of positions. MATH-Sigma does neither. Every constant is derived from three axioms:

- **b = 35** - average branching factor of chess (property of legal move rules)
- **b_eff = 7.4** - effective branching factor after move ordering
- **T = 78cp** - tempo value, derived as P * ln(b) / (1 + ln(b))

From these, all search parameters follow: aspiration windows, null move reduction, LMR coefficients, futility margins, reverse futility margins, probcut thresholds. The evaluation uses piece values from standard exchange ratios (P=100, N=320, B=330, R=500, Q=900), piece-square tables from board geometry, and positional terms (mobility, king safety, pawn structure, passed pawns) weighted by mathematical relationships.

Nothing was tuned by playing games against itself or other engines. If a constant appears in the code, there's a derivation for why it has that value.

## how strong is it

Tested against Stockfish at fixed depth limits with 1 second per move:

| opponent | result | estimated elo |
|----------|--------|---------------|
| stockfish depth 6 (~2100) | 5W 1D 2L | ~2237 |
| stockfish depth 7 (~2250) | 3W 1D 4L | ~2206 |
| stockfish depth 8 (~2400) | 1W 0D 7L | ~2062 |

Realistic strength is around **2200 Elo** - strong amateur / national master level. The engine can beat Stockfish at shallow depths through tactical play but gets outclassed when Stockfish searches deeper.

For context, no documented chess engine has achieved above ~2500 Elo with purely mathematically derived constants. Empirically tuned hand-crafted engines reach ~2800-2900. NNUE engines reach 3500+. The gap between "derived from math" and "tuned against games" is roughly 600-800 Elo.

## what's in here

```
src/
  main.rs        - entry point + bench harness (10 positions)
  lib.rs         - module declarations
  types.rs       - piece values, derived constants, score types
  tt.rs          - transposition table (64MB, generation-based)
  uci.rs         - UCI protocol handler + time management
  eval/
    mod.rs       - full evaluation function (~1350 lines)
    pst.rs       - piece-square tables
  search/
    mod.rs       - alpha-beta search (~1000 lines)
```

about 3200 lines of Rust total.

### evaluation

- material + piece-square tables with midgame/endgame interpolation (24-point phase system)
- symmetric mobility for both sides (sqrt-scaled, phase-weighted)
- quadratic king attack model (attack units squared, safe check bonuses, pawn shield)
- pawn structure with caching (isolated, doubled, connected, passed pawns)
- passed pawn bonuses (blockade, rook behind passer, king proximity, rule of the square, promotion corridor)
- bishop pair bonus scaled by position openness
- rook on open/semi-open files with king proximity weighting
- knight outposts, bishop color complex, space advantage, hanging pieces, pawn threats
- endgame solvers for KQK, KRK, KBNK, KBBK, KPK, KBP, KQvKP
- endgame scaling for opposite-color bishops, KR+minor vs KR, mating potential
- rook endgame geometry (R vs P, R+P vs R with Lucena/Philidor patterns)
- lazy eval cutoff when material difference exceeds 400cp
- integer-only arithmetic on the hot path (Q8 fixed-point where needed)

### search

- alpha-beta with principal variation search and iterative deepening
- volatility-adaptive aspiration windows
- null move pruning with zugzwang detection and verification search
- late move reductions with history-based adjustment
- reverse futility pruning, futility pruning, razoring
- probcut with good captures only
- singular extensions, recapture extensions, pawn push extensions, check extensions
- late move pruning and history-based pruning (pre-clone)
- losing capture reduction based on SEE
- countermove heuristic, killer moves, continuation history
- quiescence search with SEE-ordered captures, delta pruning, volatility-gated quiet checks
- in-search repetition detection + root game history
- time management with move instability detection

## building

requires Rust and a CPU with PEXT support (most modern x86 processors).

```
RUSTFLAGS="-C target-cpu=native" cargo build --release
```

run the engine (speaks UCI protocol):
```
./target/release/superchess-rs
```

run the benchmark:
```
./target/release/superchess-rs --bench
```

run tests:
```
RUSTFLAGS="-C target-cpu=native" cargo test
```

## playing against it

MATH-Sigma speaks the UCI protocol. Point any UCI-compatible chess GUI at the binary:

- [Arena](http://www.playwitharena.de/)
- [CuteChess](https://cutechess.com/)
- [Banksia](https://banksiagui.com/)

or use it from the command line:
```
echo -e "uci\nisready\nposition startpos\ngo depth 10\nquit" | ./target/release/superchess-rs
```

## what we learned

over 35 experiments were tested during development. the main findings:

- **speed beats complexity.** the only change that consistently improved match results was making the eval faster (lazy eval cutoff), not adding more evaluation terms. at ~1M nodes per second, extra search depth matters more than positional nuance.

- **eval noise limits everything.** in an additive linear evaluation (score = sum of weighted features), each new term adds both signal and noise. past ~100 features, marginal terms add noise roughly equal to signal. this is the architectural ceiling.

- **the math works for material and search.** piece values, search parameters, pruning margins - these all derive cleanly from branching factor and game tree properties. the math breaks down for positional evaluation, where feature weights are fundamentally empirical.

- **~2200 is the ceiling for this approach.** not because the math is wrong, but because the linear additive architecture can't represent the positional patterns that separate 2200 from 2800. breaking through would require a different evaluation architecture (conditional/nonlinear), not better parameters.

## dependencies

- [cozy-chess](https://crates.io/crates/cozy-chess) - move generation with PEXT bitboards
- [arrayvec](https://crates.io/crates/arrayvec) - stack-allocated move lists
