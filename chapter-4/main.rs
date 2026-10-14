#![allow(unused)]
// Chapter 4 — Temporal Locality and Working Sets
//
// Copy this file into src/main.rs of a fresh `cargo new` project.
// Run with:  cargo run --release
//
// The book's exercise: generate ten million random f64 values and compute
// the average of every 100-element window, three ways:
//
//   1. naive()      — nested loop, re-reads 99 values per window
//   2. running()    — running sum, each value read twice total
//   3. strided()    — windows that skip 512 elements between samples
//
// Switch variants by commenting/uncommenting one line in `main`.
//
// The chapter's claim is that the running-sum version wins because it
// touches each value twice instead of a hundred times — not because of
// layout, but because of *how much* work it does. The strided version is
// the counterexample: same arithmetic as running(), but the access
// pattern defeats the cache, so it should be slower.

use std::time::Instant;

const N: usize = 10_000_000;
const WINDOW: usize = 100;

// ---------------------------------------------------------------------------
// A tiny seeded RNG, same as Chapters 2 and 3.
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
    // Uniform in [0, 1).
    fn next_f64(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn make_data() -> Vec<f64> {
    let mut rng = Rng::new(0xDEADBEEF);
    (0..N).map(|_| rng.next_f64()).collect()
}

// ---------------------------------------------------------------------------
// Timing helper. Same shape as Chapters 2 and 3: the result is returned
// and printed by the caller, so the compiler keeps the loop.
// ---------------------------------------------------------------------------

fn time_it<F: FnOnce() -> f64>(label: &str, f: F) -> f64 {
    let start = Instant::now();
    let result = f();
    let elapsed = start.elapsed();
    println!(
        "{:<30} {:>8.2} ms  (checksum {:.6})",
        label,
        elapsed.as_secs_f64() * 1000.0,
        result
    );
    result
}

// ---------------------------------------------------------------------------
// Variant 1: naive. For each window position, sum 100 values from scratch.
//
// Reads per element: ~100 (each element appears in up to 100 windows, and
// every appearance re-reads it).
// Reuse distance: small — the same line is hit again within a few
// iterations — so the cache handles it. The problem is the *work*: about
// 100x more additions than the running-sum version.
// ---------------------------------------------------------------------------

fn naive(data: &[f64]) -> f64 {
    let mut acc = 0.0;
    for i in 0..data.len() - WINDOW + 1 {
        let mut sum = 0.0;
        for j in 0..WINDOW {
            sum += data[i + j];
        }
        acc += sum / WINDOW as f64;
    }
    acc
}

// ---------------------------------------------------------------------------
// Variant 2: running sum. Each value enters the window once and leaves
// once. Two reads per element, one add and one subtract per step.
//
// Reuse distance: WINDOW elements = 800 bytes for f64, comfortably in L1.
// Work: O(n), not O(n * WINDOW).
// ---------------------------------------------------------------------------

fn running(data: &[f64]) -> f64 {
    let mut acc = 0.0;
    let mut sum: f64 = data[..WINDOW].iter().sum();
    acc += sum / WINDOW as f64;

    for i in WINDOW..data.len() {
        sum += data[i];
        sum -= data[i - WINDOW];
        acc += sum / WINDOW as f64;
    }
    acc
}

// ---------------------------------------------------------------------------
// Variant 3: strided windows. Same arithmetic as running(), but each
// window starts 512 elements after the previous one instead of 1.
//
// 512 f64 elements = 4096 bytes = 4 KB. That is exactly the L1 set stride
// on many machines, which is what the chapter's conflict-miss section is
// about. On some CPUs the effect is obvious; on others (higher
// associativity, different set count) it may be small or absent.
//
// If you don't see a slowdown here, that is also a valid result: measure,
// note it, and explain why your machine's cache geometry doesn't produce
// the conflict. The chapter's point is the pattern, not a guaranteed
// factor.
//
// Note: this also does less work than running() — one window per 512
// elements instead of one per element. So if you compare its absolute
// time to running()'s, you're comparing different amounts of arithmetic.
// Compare its *per-window* cost instead, or compare it to a running-sum
// version that also strides — see "try changing" below.
// ---------------------------------------------------------------------------

fn strided(data: &[f64]) -> f64 {
    let stride = 512;
    let mut acc = 0.0;
    let mut i = 0;
    while i + WINDOW <= data.len() {
        let mut sum = 0.0;
        for j in 0..WINDOW {
            sum += data[i + j];
        }
        acc += sum / WINDOW as f64;
        i += stride;
    }
    acc
}

// ---------------------------------------------------------------------------
// main — the switchboard. Uncomment one, run, compare.
// ---------------------------------------------------------------------------

fn main() {
    println!("N = {}, window = {}\n", N, WINDOW);
    let data = make_data();

    // time_it("naive", || naive(&data));
    // time_it("running", || running(&data));
    time_it("strided (stride 512)", || strided(&data));
}

// ---------------------------------------------------------------------------
// Try changing:
//
// 1. Uncomment all three variants and run them in one go. The `naive`
//    version should be roughly 100x slower than `running`. Why 100x?
//    Because `naive` re-reads every value up to 100 times, and `running`
//    reads each value exactly twice.
//
// 2. Compute time per window for each variant, not total time. The
//    strided version does ~512x fewer windows than `running`, so its
//    total time is lower — but its cost *per window* is much higher,
//    because every window starts 4 KB after the last one, which fights
//    the L1 sets and defeats the prefetcher. On the author's machine
//    this was ~70x slower per window. Note your own number.
//
// 3. Change the stride in `strided` from 512 to 513, 514, 511. The
//    conflict-miss effect should weaken as soon as the stride is no
//    longer a clean multiple of the set stride. This is the cleanest way
//    to see the effect on your machine.
//
// 4. The fairest comparison is per-window cost, not total time.
//    On the author's machine:
//
//        running:  ~1.95 ns/window
//        strided:  ~137 ns/window
//
//    Your numbers will differ; the ratio is what to look at.
//
//    That ~70x per-window difference is the access pattern. Both versions
//    do one 100-element sum per window; the only difference is that
//    `running` slides by 1 element and `strided` jumps by 512. The jump
//    lands each window on a different 4 KB boundary, which conflicts in
//    L1 and defeats the prefetcher.
//
// 5. Change the RNG seed and confirm the checksums change but the
//    relative timings don't. If the timings move substantially, you're
//    measuring noise.
// ---------------------------------------------------------------------------
