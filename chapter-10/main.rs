#![allow(unused)]
// Chapter 10 — SIMD and Auto-Vectorization
//
// Copy this file into src/main.rs of a fresh `cargo new` project.
// Run with:  cargo run --release
//
// Four demonstrations, each in its own function, each showing a
// different aspect of SIMD:
//
//   1. count_above_u8()    — simple loop over bytes. The compiler
//                            vectorizes this automatically. Run
//                            `cargo asm sample::count_above_u8` to see.
//
//   2. sum_f64_naive()     — floating-point sum. The compiler does NOT
//                            vectorize this, because IEEE 754 addition
//                            is not associative.
//
//   3. sum_u64_naive()     — integer sum. The compiler DOES vectorize
//                            this, and rewrites the single accumulator
//                            into SIMD lanes.
//
//   4. search_memchr()     — byte search, comparing a hand-written
//                            scalar loop against memchr's SIMD
//                            implementation.
//
// Switch demos by commenting/uncommenting lines in `main`.
//
// For the assembly checks, add `#[inline(never)]` to the functions you
// want to inspect, then:
//
//     cargo build --release
//     cargo asm sample::count_above_u8
//
// The `#[inline(never)]` is already applied to the two sum functions
// and to count_above_u8, so cargo asm can find them.

use std::hint::black_box;
use std::time::Instant;

// Sizes. Big enough to be memory-relevant, small enough to run quickly.
const COUNT_N: usize = 1 << 24; // 16 MiB of u8
const SUM_N: usize = 1 << 20; // 1 Mi elements of f64 / u64
const SEARCH_N: usize = 1 << 27; // 128 MiB of u8 for search
const SEARCH_NEEDLE: u8 = 0xAB;

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
// Timing helper.
// ---------------------------------------------------------------------------

fn time_it<F: FnOnce() -> u64>(label: &str, f: F) -> u64 {
    let start = Instant::now();
    let result = f();
    let elapsed = start.elapsed();
    println!(
        "{:<26} {:>8.2} ms   (checksum {})",
        label,
        elapsed.as_secs_f64() * 1000.0,
        result
    );
    result
}

// ---------------------------------------------------------------------------
// Demo 1: count bytes greater than 128.
//
// This is the loop from §4 of the chapter. It's exactly the same code as
// Chapter 8's count_branchy, and the compiler will produce the same
// result: a fully vectorized loop using pcmpeqb, punpcklbw, and paddq.
//
// Run `cargo asm sample::count_above_u8` to see it. Look for `pcmpeqb`,
// `psubb`, or `paddq` — those are the SIMD instructions doing the
// comparison and count in parallel.
//
// The point of including this here is to give you something concrete to
// look at in the assembly. The compiler does this transformation on its
// own, for loops of this shape. You don't have to write SIMD to get it.
// ---------------------------------------------------------------------------

#[inline(never)]
fn count_above_u8(data: &[u8]) -> u64 {
    let mut count = 0u64;
    for &x in data {
        if x > 128 {
            count += 1;
        }
    }
    count
}

// ---------------------------------------------------------------------------
// Demo 2: floating-point sum, single accumulator.
//
// This is the loop from Chapter 8. The compiler will NOT vectorize it,
// because IEEE 754 addition is not associative and LLVM respects that
// by default. The result is a serial chain of addsd instructions.
//
// Run `cargo asm sample::sum_f64_naive`. You should see `addsd`, not
// `vaddpd`. That's the whole point: floating-point reductions are stuck
// on a single chain unless you give the compiler independent accumulators
// to work with.
// ---------------------------------------------------------------------------

#[inline(never)]
fn sum_f64_naive(data: &[f64]) -> f64 {
    let mut total = 0.0;
    for &x in data {
        total += x;
    }
    total
}

// ---------------------------------------------------------------------------
// Demo 3: integer sum, single accumulator.
//
// Same shape as demo 2, but over u64. The compiler WILL vectorize this,
// because integer addition is associative and LLVM is free to reorder.
// Run `cargo asm sample::sum_u64_naive`. You should see `paddq`, not
// `add`. The single source-level accumulator has been rewritten into
// two or four SIMD lanes.
//
// This is the same phenomenon we hit in Chapter 8, and it's why that
// chapter's experiment had to use f64. Integer arithmetic is easy for
// the vectorizer; floating point isn't.
// ---------------------------------------------------------------------------

#[inline(never)]
fn sum_u64_naive(data: &[u64]) -> u64 {
    let mut total = 0u64;
    for &x in data {
        total = total.wrapping_add(x);
    }
    total
}

// ---------------------------------------------------------------------------
// Demo 4: byte search, scalar vs. memchr.
//
// The scalar version processes one byte at a time. The memchr version
// uses AVX2 to compare 32 bytes at once, and it's the engine behind
// ripgrep's search.
//
// This is a case where the compiler does NOT vectorize the scalar
// version — the loop's shape (`memchr_iter` returns an iterator, and
// scalar code has to check each byte individually) prevents it. The win
// from using memchr is 5-50x depending on the input.
//
// Note: this demo needs memchr in Cargo.toml. Add:
//
//     memchr = "2"
//
// The other three demos don't need any dependency; if you don't want to
// add memchr, comment out this demo's calls in main.
// ---------------------------------------------------------------------------

fn search_scalar(data: &[u8], needle: u8) -> u64 {
    let mut count = 0u64;
    for &b in data {
        if b == needle {
            count += 1;
        }
    }
    count
}

fn search_memchr(data: &[u8], needle: u8) -> u64 {
    memchr::memchr_iter(needle, data).count() as u64
}

// ---------------------------------------------------------------------------
// main — the switchboard.
// ---------------------------------------------------------------------------

fn main() {
    // ---- Demo 1: count_above_u8 ----
    println!("Demo 1: count bytes > 128 over {} bytes\n", COUNT_N);
    let mut rng = Rng::new(0xDEADBEEF);
    let bytes: Vec<u8> = (0..COUNT_N).map(|_| (rng.next() & 0xFF) as u8).collect();
    time_it("count_above_u8", || count_above_u8(&bytes));
    println!();

    // ---- Demo 2: f64 sum ----
    println!("Demo 2: f64 sum over {} elements\n", SUM_N);
    let mut rng = Rng::new(0xCAFEBABE);
    let floats: Vec<f64> = (0..SUM_N).map(|_| (rng.next() & 0xFFFF) as f64).collect();
    time_it("sum_f64_naive", || {
        // Return a u64 for the timing helper; f64 sum cast to u64 loses
        // precision but the checksum is only a sanity check.
        sum_f64_naive(&floats) as u64
    });
    println!();

    // ---- Demo 3: u64 sum ----
    println!("Demo 3: u64 sum over {} elements\n", SUM_N);
    let mut rng = Rng::new(0xFEEDFACE);
    let ints: Vec<u64> = (0..SUM_N).map(|_| rng.next() & 0xFFFF).collect();
    time_it("sum_u64_naive", || sum_u64_naive(&ints));
    println!();

    // ---- Demo 4: byte search ----
    // Comment this block out if you don't want to add memchr as a
    // dependency.
    println!("Demo 4: byte search over {} bytes\n", SEARCH_N);
    let mut rng = Rng::new(0xBEEFCAFE);
    let haystack: Vec<u8> = (0..SEARCH_N)
        .map(|_| {
            // Skew the distribution so the needle is rare but present.
            if rng.next() & 0xFF < 4 {
                SEARCH_NEEDLE
            } else {
                (rng.next() & 0xFF) as u8
            }
        })
        .collect();
    time_it("search_scalar", || search_scalar(&haystack, SEARCH_NEEDLE));
    time_it("search_memchr", || search_memchr(&haystack, SEARCH_NEEDLE));

    black_box(&bytes);
    black_box(&floats);
    black_box(&ints);
    black_box(&haystack);
}

// ---------------------------------------------------------------------------
// What to check with cargo asm:
//
//     cargo build --release
//
//     cargo asm sample::count_above_u8    # expect SIMD (pcmpeqb, paddq)
//     cargo asm sample::sum_f64_naive     # expect scalar (addsd)
//     cargo asm sample::sum_u64_naive     # expect SIMD (paddq)
//
// The point of these three checks is that the *source* looks almost
// identical in all three cases — a loop with one accumulator — but the
// compiler does completely different things with each:
//
//   - count_above_u8: element-wise operation, no dependency. Vectorized.
//   - sum_u64_naive:  integer reduction, reassociable. Vectorized.
//   - sum_f64_naive:  floating-point reduction, NOT reassociable by
//                     default. Left scalar.
//
// That last one is the lesson. If you want a vectorized float sum, you
// have to write the multi-accumulator version explicitly, and even then
// LLVM won't vectorize it without -ffast-math (which Rust doesn't expose
// directly). For most purposes, the scalar chain is what you get.
//
// ---------------------------------------------------------------------------
// What to expect from the demos:
//
// Demo 1: count_above_u8 on 16 MiB of random bytes. On a modern x86-64
// with SSE2 (the baseline), the compiler's auto-vectorized version
// should do this in a few milliseconds. The exact number depends on how
// well the SIMD path fits your CPU's cache hierarchy.
//
// Demo 2: sum_f64_naive on 8 MiB of f64. This is a serial chain of
// addsd, so it runs at one add per add-latency (about 4 cycles on
// modern x86). Expect something like 40-80 ms depending on your CPU.
//
// Demo 3: sum_u64_naive on 8 MiB of u64. This is vectorized to paddq,
// so it runs at two or four adds per cycle. Should be 3-4x faster than
// demo 2 in wall time, on the same amount of data.
//
// Demo 4: search_scalar vs. search_memchr on 128 MiB. The scalar version
// is a byte-at-a-time loop; the memchr version uses SIMD. Expect memchr
// to be roughly 5-20x faster. This is the biggest single-number win in
// the chapter, and it's the reason the chapter tells you to reach for
// the crate rather than write your own.
//
// ---------------------------------------------------------------------------
// Try changing:
//
// 1. Run cargo asm on count_above_u8, sum_f64_naive, and sum_u64_naive.
//    Compare the three functions' assembly. The source looks nearly
//    identical; the compiled output is completely different.
//
// 2. Add a five-accumulator u64 sum and check its assembly. LLVM may
//    or may not vectorize it — this is one of those cases where the
//    compiler's decision is not obvious from the source.
//
// 3. For the f64 sum, add the -C target-cpu=native flag (via RUSTFLAGS)
//    and re-run. Note whether anything changed. It usually doesn't,
//    because the default float semantics block vectorization regardless
//    of target features.
//
// 4. Comment out the memchr demo's calls in main. Compare what you get
//    from the remaining three demos. Without the search comparison, the
//    chapter's point about "use the crate when it exists" is missing.
//
// 5. Change SEARCH_N to 1 << 20 (1 MiB) and re-run demo 4. At small
//    sizes, the setup cost of the SIMD path dominates, and the gap
//    between scalar and memchr shrinks. This is the "SIMD isn't always
//    faster" note from §9.
// ---------------------------------------------------------------------------
