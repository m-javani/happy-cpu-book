#![allow(unused)]
// Chapter 3 — Layout Is the Highest-Leverage Knob
//
// Copy this file into src/main.rs of a fresh `cargo new` project.
// Run with:  cargo run --release
//
// The book's exercise: store a million 24-byte records, then run two
// passes over them (read the first field; read the third). Compare three
// layouts:
//
//   1. Vec<Record>          — interleaved, dense
//   2. three Vecs           — separated, dense
//   3. Vec<Box<Record>>     — contiguous pointers, records scattered
//
// Switch layouts by commenting/uncommenting one line in `main`.

use std::time::Instant;

const N: usize = 1_000_000;

// ---------------------------------------------------------------------------
// The record. Three fields, 24 bytes on a 64-bit machine.
//
//   first  : u64   (8)
//   second : u64   (8)
//   third  : u64   (8)
//
// A 64-byte cache line holds two records (48 bytes) plus 16 bytes of a
// third. So a pass that reads one field gets 16 useful bytes per line.
//
// The separated layout stores each field in its own dense Vec, so a
// 64-byte line holds eight of the same field, all useful.
//
// The boxed layout stores eight pointers per line in the outer Vec, but
// each Record lives in its own heap allocation. Following the pointer is
// a cache-line fetch with no spatial pattern for the prefetcher.
// ---------------------------------------------------------------------------

struct Record {
    first: u64,
    second: u64,
    third: u64,
}

// ---------------------------------------------------------------------------
// A tiny seeded RNG. Same as Chapter 2 — we don't want a `rand` dependency.
// ---------------------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed | 1)
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
// Time one call and print ns/entry. Result is returned and printed by the
// caller, so the compiler keeps the loop. Same helper as Chapter 2.
// ---------------------------------------------------------------------------

fn time_it<F: FnOnce() -> u64>(label: &str, f: F) -> u64 {
    let start = Instant::now();
    let result = f();
    let elapsed = start.elapsed();
    let ns_per_entry = elapsed.as_nanos() as f64 / N as f64;
    println!("{:<34} {:>8.2} ns/entry", label, ns_per_entry);
    result
}

// ---------------------------------------------------------------------------
// Layout 1: Vec<Record>. Two passes, each reading one field.
// ---------------------------------------------------------------------------

fn run_interleaved() {
    let mut rng = Rng::new(0xDEADBEEF);
    let mut records = Vec::with_capacity(N);
    for _ in 0..N {
        records.push(Record {
            first: rng.next(),
            second: rng.next(),
            third: rng.next(),
        });
    }

    let a = time_it("interleaved: read first", || {
        let mut sum = 0u64;
        for r in &records {
            sum = sum.wrapping_add(r.first);
        }
        sum
    });

    let b = time_it("interleaved: read third", || {
        let mut sum = 0u64;
        for r in &records {
            sum = sum.wrapping_add(r.third);
        }
        sum
    });

    println!("checksums: {} {}\n", a, b);
}

// ---------------------------------------------------------------------------
// Layout 2: three Vecs. Same two passes, now over dense arrays.
// ---------------------------------------------------------------------------

fn run_separated() {
    let mut rng = Rng::new(0xDEADBEEF);
    let mut firsts = Vec::with_capacity(N);
    let mut seconds = Vec::with_capacity(N);
    let mut thirds = Vec::with_capacity(N);
    for _ in 0..N {
        firsts.push(rng.next());
        seconds.push(rng.next());
        thirds.push(rng.next());
    }

    let a = time_it("separated: read first", || {
        let mut sum = 0u64;
        for &v in &firsts {
            sum = sum.wrapping_add(v);
        }
        sum
    });

    let b = time_it("separated: read third", || {
        let mut sum = 0u64;
        for &v in &thirds {
            sum = sum.wrapping_add(v);
        }
        sum
    });

    println!("checksums: {} {}\n", a, b);
}

// ---------------------------------------------------------------------------
// Layout 3: Vec<Box<Record>>. Contiguous pointers, scattered records.
// ---------------------------------------------------------------------------

fn run_boxed() {
    let mut rng = Rng::new(0xDEADBEEF);
    let mut records = Vec::with_capacity(N);
    for _ in 0..N {
        records.push(Box::new(Record {
            first: rng.next(),
            second: rng.next(),
            third: rng.next(),
        }));
    }

    let a = time_it("boxed: read first", || {
        let mut sum = 0u64;
        for r in &records {
            sum = sum.wrapping_add(r.first);
        }
        sum
    });

    let b = time_it("boxed: read third", || {
        let mut sum = 0u64;
        for r in &records {
            sum = sum.wrapping_add(r.third);
        }
        sum
    });

    println!("checksums: {} {}\n", a, b);
}

// ---------------------------------------------------------------------------
// main — the switchboard. Uncomment one, run, compare.
// ---------------------------------------------------------------------------

fn main() {
    println!("N = {} records\n", N);

    run_interleaved();
    // run_separated();
    // run_boxed();
}

// ---------------------------------------------------------------------------
// Note on the numbers:
//
// You will not see the full byte-count ratio between layouts, because the
// prefetcher hides part of the cost of the worse ones. The interleaved
// and boxed walks are both sequential at the outer level, so the
// prefetcher runs ahead and overlaps DRAM latency with the current work.
//
// The boxed layout is the interesting case: it has *two* costs. The
// pointer array is dense and prefetchable, but each Record lives in its
// own heap allocation, so following the pointer is an unpredictable
// fetch. Watch whether "boxed: read first" is meaningfully slower than
// "interleaved: read first" — that gap is the pointer chase.
//
// ---------------------------------------------------------------------------
// Try changing:
//
// 1. Change N to 100 (small enough to fit in L1). Do the layouts still
//    differ? At small sizes, everything fits in cache and the layout
//    stops mattering. The lesson is about scale.
//
// 2. Add a second field to each pass: read `first` and `second` together
//    in the interleaved layout. Does the separated layout still win? What
//    if the pass reads *all three* fields?
//
// 3. Replace the per-field passes with one pass that reads all three
//    fields and sums them. In the interleaved layout, every byte of the
//    cache line is now useful. In the separated layout, three dense
//    scans are still dense. Which wins, and why?
//
// 4. Change the RNG seed and confirm the checksums change but the
//    relative order of the layouts does not. If it flips, the earlier
//    result was noise.
// ---------------------------------------------------------------------------
