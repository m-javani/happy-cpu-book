#![allow(unused)]
// Chapter 9 — Top-Down: The Four Buckets
//
// Copy this file into src/main.rs of a fresh `cargo new` project.
// Run with:  cargo run --release
//
// This file is not the exercise. The exercise asks you to profile a
// program *you* care about, and no file from the book can substitute
// for that.
//
// What this file is for: to let you see the top-down workflow on a
// known example before you apply it to your own code. It implements
// the matrix multiply from §7, in the two versions the chapter walks
// through:
//
//   1. naive()    — the triple loop, as written
//   2. blocked()  — the same loop, tiled to fit in L1
//
// Run each under `perf stat --topdown` and watch the buckets move.
//
// Switch variants by commenting/uncommenting one line in `main`.
//
// You will need `perf` installed. On Debian/Ubuntu:
//     sudo apt install linux-tools-common linux-tools-generic
// On Fedora:
//     sudo dnf install perf
// On Arch:
//     sudo pacman -S perf
//
// Run under perf like this:
//
//     cargo build --release
//     perf stat --topdown ./target/release/<name>
//
// If the top-down events aren't available on your machine, fall back
// to the cache and TLB events from Chapters 2 and 6:
//
//     perf stat -e cache-misses,cache-references,dTLB-loads,dTLB-load-misses ./target/release/<name>
//
// The naive version should show high cache-miss rates; the blocked
// version should show them drop. That's the same lesson the top-down
// profile teaches, measured a different way.

use std::time::Instant;

const N: usize = 1024;
const BLOCK: usize = 32;

// ---------------------------------------------------------------------------
// Timing helper. We do enough work that the wall time is the interesting
// number here; `perf` gives us the breakdown.
// ---------------------------------------------------------------------------

fn time_it<F: FnOnce() -> f32>(label: &str, f: F) {
    let start = Instant::now();
    let checksum = f();
    let elapsed = start.elapsed();
    println!(
        "{:<12} {:>8.1} ms   (checksum {:.3})",
        label,
        elapsed.as_secs_f64() * 1000.0,
        checksum
    );
}

// ---------------------------------------------------------------------------
// Naive: the triple loop.
//
// The inner loop strides through `b` with a stride of N elements — 4 KB
// for f32 at N=1024. Every access to `b` misses in L1, and by the time
// you come back to the same column, the line has been evicted. This is
// the version that shows up as Backend Bound in the profile, and as a
// high cache-miss rate in the counters.
// ---------------------------------------------------------------------------

fn naive(a: &[f32], b: &[f32], c: &mut [f32]) -> f32 {
    for i in 0..N {
        for j in 0..N {
            let mut acc = 0.0f32;
            for k in 0..N {
                acc += a[i * N + k] * b[k * N + j];
            }
            c[i * N + j] = acc;
        }
    }
    c.iter().sum()
}

// ---------------------------------------------------------------------------
// Blocked: the same arithmetic, tiled to fit in L1.
//
// Each tile is BLOCK x BLOCK = 32 x 32 = 1024 f32 = 4 KB. Three tiles
// (one of a, one of b, one of c) fit comfortably in L1. Within a tile,
// the reuse distance is small: the same row of `b` is reused many times
// before moving on.
//
// The number of multiplications is identical to naive. The order of
// iteration is what changed, and the order is what determines whether
// the working set fits in cache.
// ---------------------------------------------------------------------------

fn blocked(a: &[f32], b: &[f32], c: &mut [f32]) -> f32 {
    for ii in (0..N).step_by(BLOCK) {
        for kk in (0..N).step_by(BLOCK) {
            for jj in (0..N).step_by(BLOCK) {
                for i in ii..ii + BLOCK {
                    for k in kk..kk + BLOCK {
                        let a_ik = a[i * N + k];
                        for j in jj..jj + BLOCK {
                            c[i * N + j] += a_ik * b[k * N + j];
                        }
                    }
                }
            }
        }
    }
    c.iter().sum()
}

// ---------------------------------------------------------------------------
// Data setup. Deterministic so the checksums match across variants.
// ---------------------------------------------------------------------------

fn make_matrices() -> (Vec<f32>, Vec<f32>, Vec<f32>) {
    let a: Vec<f32> = (0..N * N).map(|i| ((i % 17) as f32) * 0.01).collect();
    let b: Vec<f32> = (0..N * N).map(|i| ((i % 13) as f32) * 0.01).collect();
    let c: Vec<f32> = vec![0.0; N * N];
    (a, b, c)
}

// ---------------------------------------------------------------------------
// main — the switchboard.
// ---------------------------------------------------------------------------

fn main() {
    println!("Matrix multiply, N = {}\n", N);

    let (a, b, mut c) = make_matrices();

    // time_it("naive", || naive(&a, &b, &mut c));

    // Reset c and try the next variant.
    //
    // All variants accumulate into c, so if you run them in one process
    // (by uncommenting more than one), you must reset c between calls
    // or the checksums will be wrong.
    //
    // For the top-down workflow, this doesn't matter: you run one
    // variant per `perf stat` invocation, so each process starts with
    // a fresh c.
    //
    // c.fill(0.0);
    time_it("blocked", || blocked(&a, &b, &mut c));
}

// ---------------------------------------------------------------------------
// How to run the profile:
//
//     cargo build --release
//     perf stat --topdown ./target/release/<name>
//
// Expected results:
//
//   naive:     Backend Bound dominates. In the top-level profile, most
//              cycles fall into Backend Bound and Retiring is low.
//              In the cache counters, cache-misses are a large fraction
//              of cache-references. The inner loop strides through b
//              with a 4 KB stride, so every access misses L1.
//
//   blocked:   Retiring jumps substantially. Backend Bound drops. In the
//              cache counters, cache-misses fall and cache-references
//              stay roughly the same. The working set now fits in L1,
//              so most accesses hit.
//
// Exact percentages will differ on your machine. The *direction* is
// what to look at: retiring up, backend down, miss rate down.
//
// If the top-down events aren't available on your machine, use the cache
// counters instead. They tell the same story without needing TMA.
//
// ---------------------------------------------------------------------------
// Try changing:
//
// 1. Run both variants and compare the top-level profile. Write down
//    the four numbers for each. The pattern should be: naive is
//    backend-bound, blocked is much less so.
//
// 2. Run both variants under
//        perf stat -e cache-misses,cache-references
//    and compare the miss rates. The naive version should be much
//    higher.
//
// 3. Add a third variant that multiplies two matrices with N = 128
//    (small enough to fit in L2 entirely). The naive version should now
//    be fast, because there's no memory problem to fix. This is the
//    important counterexample: blocking only helps when the working
//    set doesn't fit in cache.
//
// 4. Change BLOCK to 16 and then to 64. Which is faster on your
//    machine? Why? The answer depends on your L1 size and associativity.
//
// 5. Take the top-down workflow to a program you actually care about.
//    That's the real exercise. This file is just the warm-up.
// ---------------------------------------------------------------------------
