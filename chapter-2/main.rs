#![allow(unused)]
// Chapter 2 — Cache Hierarchy
//
// Copy this file into src/main.rs of a fresh `cargo new` project.
// Run with:  cargo run --release
//
// The book's exercise: generate ten million log records, run two passes
// over them (count entries above a timestamp cutoff; count error-level
// entries), then repeat with the same data laid out as three separate
// Vecs. Compare.
//
// Two layouts are provided. In `main`, leave one uncommented at a time.
// Compare the printed ns/entry numbers between them.

use std::time::Instant;

const N: usize = 10_000_000;
const CUTOFF: u64 = 5_000_000_000;
const ERROR_LEVEL: u8 = 3;

// ---------------------------------------------------------------------------
// A tiny seeded RNG. We don't want a `rand` dependency — the reader
// shouldn't have to touch Cargo.toml for a layout experiment. The values
// don't matter; only their distribution (uniform here) does.
// ---------------------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed | 1) // must be nonzero
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

// ---------------------------------------------------------------------------
// The data, in two layouts.
//
// `LogEntry` is the natural way to write this: one struct per record.
// It is 24 bytes on a 64-bit machine:
//     timestamp  : u64   (8)
//     level      : u8    (1, padded to 8 for alignment)
//     message_id : u64   (8)
//     8 + 8 + 8 = 24
//
// A 64-byte cache line holds 2 entries (48 bytes) plus 16 bytes of a third.
// So a pass that reads only `timestamp` gets 16 useful bytes per line;
// a pass that reads only `level` gets 2 useful bytes per line.
//
// The separated layout stores each field in its own dense Vec, so a
// 64-byte line holds 8 timestamps (all useful) or 64 levels (all useful).
// ---------------------------------------------------------------------------

struct LogEntry {
    timestamp: u64,
    level: u8,
    message_id: u64,
}

// ---------------------------------------------------------------------------
// Layout 1: interleaved. One Vec<LogEntry>.
//
// Both passes walk the same Vec. Each cache line carries all three fields,
// but the pass only uses one of them.
// ---------------------------------------------------------------------------

fn count_above_interleaved(entries: &[LogEntry]) -> u64 {
    let mut count = 0;
    for e in entries {
        if e.timestamp > CUTOFF {
            count += 1;
        }
    }
    count
}

fn count_errors_interleaved(entries: &[LogEntry]) -> u64 {
    let mut count = 0;
    for e in entries {
        if e.level == ERROR_LEVEL {
            count += 1;
        }
    }
    count
}

// ---------------------------------------------------------------------------
// Layout 2: separated. Three dense Vecs, one per field.
//
// Same two passes, same data, same comparisons — but each pass now reads
// a dense Vec of exactly the field it wants. A 64-byte line holds 8
// timestamps or 64 levels, all of them used.
// ---------------------------------------------------------------------------

fn count_above_separated(timestamps: &[u64]) -> u64 {
    let mut count = 0;
    for &t in timestamps {
        if t > CUTOFF {
            count += 1;
        }
    }
    count
}

fn count_errors_separated(levels: &[u8]) -> u64 {
    let mut count = 0;
    for &l in levels {
        if l == ERROR_LEVEL {
            count += 1;
        }
    }
    count
}

// ---------------------------------------------------------------------------
// Time one call and print ns/entry.
//
// Note on the numbers:
//
// You will not see a 32× difference between the two layouts, even though
// the separated version uses ~32× more of each cache line. The reason is
// the prefetcher: the CPU watches your access pattern and fetches ahead,
// hiding part of the interleaved layout's waste. The gap is real and
// consistent, just smaller than the byte-count ratio. See "Why the
// machine helps" in this chapter; we return to the prefetcher later.
// ---------------------------------------------------------------------------

fn time_it<F: FnOnce() -> u64>(label: &str, f: F) -> u64 {
    let start = Instant::now();
    let result = f();
    let elapsed = start.elapsed();
    let ns_per_entry = elapsed.as_nanos() as f64 / N as f64;
    println!("{:<36} {:>8.2} ns/entry", label, ns_per_entry);
    result
}

// ---------------------------------------------------------------------------
// main — the switchboard.
//
// Uncomment exactly one of the two blocks, then `cargo run --release`.
// Run them in either order; the point is to compare the printed numbers.
// ---------------------------------------------------------------------------

fn run_interleaved() {
    let mut rng = Rng::new(0xDEADBEEF);
    let mut entries = Vec::with_capacity(N);
    for _ in 0..N {
        entries.push(LogEntry {
            timestamp: rng.next() % 10_000_000_000,
            level: (rng.next() % 5) as u8,
            message_id: rng.next(),
        });
    }

    let a = time_it("interleaved: count_above", || {
        count_above_interleaved(&entries)
    });
    let b = time_it("interleaved: count_errors", || {
        count_errors_interleaved(&entries)
    });
    println!("checksums: {} {}\n", a, b);
}

fn run_separated() {
    let mut rng = Rng::new(0xDEADBEEF);
    let mut timestamps = Vec::with_capacity(N);
    let mut levels = Vec::with_capacity(N);
    let mut message_ids = Vec::with_capacity(N);
    for _ in 0..N {
        timestamps.push(rng.next() % 10_000_000_000);
        levels.push((rng.next() % 5) as u8);
        message_ids.push(rng.next());
    }

    let a = time_it("separated: count_above", || {
        count_above_separated(&timestamps)
    });
    let b = time_it("separated: count_errors", || {
        count_errors_separated(&levels)
    });
    println!("checksums: {} {}\n", a, b);
}

fn main() {
    println!("N = {} entries\n", N);
    run_interleaved();
    run_separated();
}

// ---------------------------------------------------------------------------
// Try changing (the book's exercise, restated for the keyboard):
//
// 1. Add a third pass that reads `message_id` in each layout. Which layout
//    wins now?
//
// 2. Drop `message_id` from `LogEntry` and re-run. The struct is now 16
//    bytes, so a 64-byte line holds exactly four entries. The interleaved
//    version should get noticeably faster on both passes. Why?
//
// 3. Change the RNG seed (0xDEADBEEF, near the top of each run_*
//    function) and re-run. The two layouts' numbers should each move
//    by a little, but the gap *between* layouts should stay about the
//    same. If changing the seed flips which layout wins, the earlier
//    result was noise, not layout.
//
// 4. Notice that both `checksums` lines must print the same two numbers.
//    If they don't, the two layouts aren't holding equivalent data, and
//    the comparison is meaningless.
// ---------------------------------------------------------------------------
