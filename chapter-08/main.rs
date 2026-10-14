#![allow(unused)]
// Chapter 8 — The CPU Pipeline
//
// Copy this file into src/main.rs of a fresh `cargo new` project.
// Run with:  cargo run --release
//
// One experiment on the pipeline: the dependency chain.
//
//   sum_one_f64()  — single accumulator; one chain
//   sum_four_f64() — four accumulators; four independent chains
//
// The two functions do the same number of additions on the same data.
// The only difference is the shape of the dependency graph, and the
// four-accumulator version should be roughly 3x faster.

use std::hint::black_box;
use std::time::Instant;

// Working set.
//
// 16,384 f64 = 128 KiB. Fits in L2 on essentially every modern CPU
// (L2 is 256 KiB to 1 MiB), does not fit in L1 (32-64 KiB). The loop
// streams through L2 while the accumulator chain runs. If the working
// set doesn't fit in L2, both variants become DRAM-bound and the chain
// effect is invisible.
//
// We scan it REPEATS times so the measurement is above the noise floor
// of Instant::now() and scheduler jitter.
const SUM_N: usize = 1 << 14; // 16,384 f64 = 128 KiB
const SUM_REPEATS: usize = 2000;

// ---------------------------------------------------------------------------
// A tiny seeded RNG, same as previous chapters.
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
// Why f64 and not u64
//
// We use f64 sums, not integer sums, because LLVM will not reassociate
// or vectorize floating-point additions. IEEE 754 addition is not
// associative: (a + b) + c is not always equal to a + (b + c). LLVM
// respects this by default, so a source-level f64 sum stays a serial
// chain of `addsd` instructions.
//
// An integer sum would be vectorized. LLVM would turn `total += x` over
// u64 into SIMD adds across independent lanes, and the source-level
// "single accumulator" would become four parallel SIMD lanes in the
// machine code. The dependency chain would disappear, and the two
// variants would run at the same speed. We tried that first; both
// versions compiled to identical `paddq` loops and produced identical
// timings.
//
// f64 forces the chain to survive compilation. That's the point of the
// experiment.
//
// ---------------------------------------------------------------------------
// Why #[inline(never)]
//
// Two reasons.
//
// 1. It keeps each function as its own symbol in the binary, so
//    `cargo asm sample::sum_one_f64` can find it. Without it, LLVM
//    inlines these small functions into main and there is no symbol to
//    inspect.
//
// 2. It prevents inlining from changing the experiment. If `sum_one_f64`
//    were inlined into main, LLVM could in principle see the caller's
//    full context and transform the loop in ways it wouldn't otherwise.
//    Keeping the function opaque means we measure the function as
//    written, not as inlined-and-reoptimized.
//
// This is a benchmark-only attribute. In real code you usually want the
// opposite — let the compiler inline — unless you've measured that
// inlining hurts.
// ---------------------------------------------------------------------------

#[inline(never)]
fn sum_one_f64(data: &[f64], repeats: usize) -> f64 {
    let mut total = 0.0;
    for _ in 0..repeats {
        for &x in data {
            total += x;
        }
    }
    total
}

#[inline(never)]
fn sum_four_f64(data: &[f64], repeats: usize) -> f64 {
    let (mut s0, mut s1, mut s2, mut s3) = (0.0, 0.0, 0.0, 0.0);

    let chunks = data.chunks_exact(4);
    let rem = chunks.remainder();

    for _ in 0..repeats {
        for chunk in data.chunks_exact(4) {
            s0 += chunk[0];
            s1 += chunk[1];
            s2 += chunk[2];
            s3 += chunk[3];
        }
    }

    let mut tail = 0.0;
    for &x in rem {
        tail += x;
    }

    s0 + s1 + s2 + s3 + tail
}

// ---------------------------------------------------------------------------
// Timing helper.
// ---------------------------------------------------------------------------

fn time_it<F: FnOnce() -> f64>(label: &str, f: F) -> f64 {
    let start = Instant::now();
    let result = f();
    let elapsed = start.elapsed();
    println!(
        "{:<20} {:>8.2} ms   (checksum {:.6})",
        label,
        elapsed.as_secs_f64() * 1000.0,
        result
    );
    result
}

// ---------------------------------------------------------------------------
// main — the switchboard.
// ---------------------------------------------------------------------------

fn main() {
    println!(
        "f64 sum over {} elements ({} KiB), {} repeats\n",
        SUM_N,
        SUM_N * 8 / 1024,
        SUM_REPEATS
    );

    let mut rng = Rng::new(0xDEADBEEF);
    let data: Vec<f64> = (0..SUM_N).map(|_| (rng.next() & 0xFFFF) as f64).collect();

    // time_it("sum_one_f64", || sum_one_f64(&data, SUM_REPEATS));
    time_it("sum_four_f64", || sum_four_f64(&data, SUM_REPEATS));

    // Keep the compiler from proving the buffer is dead.
    black_box(&data);
}

// ---------------------------------------------------------------------------
// What to expect:
//
// sum_one_f64 runs at roughly one f64 add per add-latency. On modern
// x86, addsd latency is about 4 cycles, so the loop runs at ~4 cycles
// per element.
//
// sum_four_f64 has four independent chains. Each chain advances once
// per 4 cycles, but the four chains overlap, so the loop runs at ~1
// cycle per element — a 3-4x speedup.
//
// The speedup is 3-4x, not exactly 4x, because the loop is partly
// memory-bound. 128 KiB streams through L2 at ~64 bytes per cycle, so
// the loop needs about 1 cycle per 2 elements to keep the memory side
// fed. That cap applies to both variants and limits the achievable
// speedup slightly.
//
// ---------------------------------------------------------------------------
// Verify with cargo asm:
//
//   cargo build --release
//   cargo asm sample::sum_one_f64
//   cargo asm sample::sum_four_f64
//
// In sum_one_f64, look for a single `addsd` chain:
//
//     addsd xmm0, [mem]
//     addsd xmm0, [mem]
//     addsd xmm0, [mem]
//     ...
//
// Every addsd writes to the same register (xmm0) and reads it back.
// The next addsd depends on the previous one. That's the chain.
//
// In sum_four_f64, look for four independent registers:
//
//     addsd xmm0, [mem]
//     addsd xmm1, [mem]
//     addsd xmm2, [mem]
//     addsd xmm3, [mem]
//     ...
//
// Four different destination registers, four independent chains.
//
// If you see `vaddpd` (SIMD packed f64 add), the compiler vectorized
// the loop anyway, and the experiment is broken. This can happen with
// `-C target-cpu=native` and some LLVM versions. Rebuild without
// target-cpu=native and check again.
//
// ---------------------------------------------------------------------------
// Try changing:
//
// 1. Run both variants. Compare the times. sum_four_f64 should be
//    roughly 3x faster.
//
// 2. Change SUM_N to 1 << 20 (8 MiB). The loop no longer fits in L2,
//    and the gap between sum_one and sum_four should shrink, because
//    the loop is now DRAM-bound. This is the counterexample: multiple
//    accumulators help only when the loop isn't already memory-bound.
//
// 3. Change SUM_N to 1 << 12 (32 KiB). This fits in L1, and the gap
//    should be at its widest. Dependency chains are most visible when
//    memory isn't the bottleneck at all.
//
// 4. Try eight accumulators instead of four. On most cores the gain
//    over four is small, because the core can only sustain a few adds
//    per cycle. But on some microarchitectures it helps. Measure.
//
// 5. Try `u64` instead of `f64`. Watch what happens. Both variants will
//    compile to SIMD, and the timings will be identical. The compiler
//    has done the four-accumulator transformation for you — which is
//    useful to know, but not what this chapter is about. This is why
//    we use f64.
// ---------------------------------------------------------------------------
