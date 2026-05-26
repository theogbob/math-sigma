use crate::types::Score;
use cozy_chess::Move;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    Exact,
    Lower,
    Upper,
}

#[derive(Clone, Copy)]
pub struct TtEntry {
    pub key: u64,
    pub score: Score,
    pub static_eval: Score,
    pub depth: i8,
    pub bound: Bound,
    pub best_move: Option<Move>,
    pub generation: u8,
}

impl Default for TtEntry {
    fn default() -> Self {
        Self { key: 0, score: 0, static_eval: 0, depth: -1, bound: Bound::Exact, best_move: None, generation: 0 }
    }
}

pub struct TranspositionTable {
    entries: Vec<TtEntry>,
    mask: usize,
    generation: u8,
}

impl TranspositionTable {
    pub fn new(size_mb: usize) -> Self {
        let entry_size = std::mem::size_of::<TtEntry>();
        let count = ((size_mb * 1024 * 1024) / entry_size).next_power_of_two();
        Self {
            entries: vec![TtEntry::default(); count],
            mask: count - 1,
            generation: 0,
        }
    }

    pub fn new_search(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    #[inline]
    pub fn probe(&self, key: u64) -> Option<&TtEntry> {
        let idx = key as usize & self.mask;
        let entry = &self.entries[idx];
        if entry.key == key && entry.depth >= 0 {
            Some(entry)
        } else {
            None
        }
    }

    #[inline]
    pub fn store(&mut self, key: u64, score: Score, static_eval: Score, depth: i8, bound: Bound, best_move: Option<Move>) {
        let idx = key as usize & self.mask;
        let entry = &mut self.entries[idx];
        let dominated = entry.key != key
            && (entry.generation != self.generation || depth >= entry.depth);
        if entry.key == key || dominated || entry.depth < 0 {
            let mv = if best_move.is_some() {
                best_move
            } else if entry.key == key {
                entry.best_move
            } else {
                None
            };
            *entry = TtEntry { key, score, static_eval, depth, bound, best_move: mv, generation: self.generation };
        }
    }

    pub fn clear(&mut self) {
        self.generation = 0;
        for entry in self.entries.iter_mut() {
            *entry = TtEntry::default();
        }
    }
}
