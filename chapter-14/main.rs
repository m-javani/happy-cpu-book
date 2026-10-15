#![allow(unused)]
// Chapter 14 — When the Compiler Gets in the Way
//
// Copy this file into src/main.rs of a fresh `cargo new` project.
// Run with:  cargo run --release
//
// Two experiments on the compiler's choices. Both are about the same
// question: did the compiler do what you expected?
//
//   1. Inlining. Two functions that do the same thing, one marked
//      #[inline(always)] and one #[inline(never)]. Compare their
//      assembly with `cargo asm`. The noinline version exists as a
//      symbol; the inlined version doesn't, because it was folded into
//      its callers.
//
//   2. Target features. An element-wise f32 loop, built with four
//      different compiler configurations. The instruction width
//      changes with the flag: SSE (xmm), AVX2 (ymm), or AVX-512 (zmm).
//
// No external crates. Just the default toolchain and cargo-asm.
//
//     cargo install cargo-asm
//
// ---------------------------------------------------------------------------
// Experiment 1: inlining.
//
//     cargo build --release
//     cargo asm sample::hot_add_noinline
//     cargo asm sample::hot_add_inlined
//
// The first command prints a small function. The second says "not
// found." That's the answer, not an error. `hot_add_inlined` was
// inlined into its callers, so there's no separate function to find.
// The presence or absence of the symbol is the whole check — you don't
// need to read any instructions.
//
// ---------------------------------------------------------------------------
// Experiment 2: target features.
//
//     cargo clean
//     cargo build --release
//     objdump -d target/release/<name> | grep -A 40 'scale_add'
//
//     cargo clean
//     RUSTFLAGS="-C target-feature=+avx2" cargo build --release
//     objdump -d target/release/<name> | grep -A 40 'scale_add'
//
//     cargo clean
//     RUSTFLAGS="-C target-cpu=native" cargo build --release
//     objdump -d target/release/<name> | grep -A 40 'scale_add'
//
//     cargo clean
//     RUSTFLAGS="-C target-feature=+avx512f" cargo build --release
//     objdump -d target/release/<name> | grep -A 40 'scale_add'
//
// The `cargo clean` between builds matters — without it, Cargo's
// fingerprint cache can skip the recompile and the flag won't take
// effect.
//
// For each build, look at three things in the `scale_add` loop:
//
//   - Register names. xmm is 128-bit. ymm is 256-bit. zmm is 512-bit.
//   - Instruction prefixes. addps/mulps is SSE. vaddps/vmulps is AVX.
//   - The `add $0x..., %rax` at the bottom of the loop. That number
//     is bytes processed per iteration: 0x10 is 16, 0x20 is 32,
//     0x40 is 64.
//
// You should see the register width increase across the builds: xmm,
// then ymm, then zmm. Note that target-cpu=native may stop at ymm even
// on an AVX-512 machine — that's the compiler's cost model declining
// to use 512-bit by default.
//
// Only run the +avx512f build if your CPU supports AVX-512. Otherwise
// the binary will crash on the first AVX-512 instruction. Check with
// `lscpu | grep avx512f` on Linux, or `sysctl -a | grep avx512f` on
// macOS. If your machine doesn't have it, skip that build and note the
// result for the other three.
//
// `objdump` is used instead of `cargo asm` for this experiment because
// `cargo-asm` caches its disassembly, and stale output is a real hazard
// when rebuilding with different flags. `objdump` always reads the
// current binary.

use std::hint::black_box;
use std::time::Instant;

// Sizes for the two demos.
const ADD_N: usize = 1_000_000;
const SCALE_N: usize = 1 << 20; // 1 Mi f32 = 4 MiB

// ---------------------------------------------------------------------------
// Experiment 1: inlining control.
//
// These two functions do the same thing. The attributes force opposite
// decisions. In a microbenchmark like this one, the noinline version is
// measurably slower, because each call has real overhead. Whether the
// plain and inline(always) versions differ depends on whether the
// compiler inlined the plain one on its own — it often does for a
// function this small.
//
// The timer shows that there's a difference. `cargo asm` shows what the
// difference is.
// ---------------------------------------------------------------------------

#[inline(always)]
fn hot_add_inlined(a: u64, b: u64) -> u64 {
    a.wrapping_add(b)
}

#[inline(never)]
fn hot_add_noinline(a: u64, b: u64) -> u64 {
    a.wrapping_add(b)
}

// Two drivers, so the compiler has a reason to keep both functions.
// The loops are identical from the source's point of view; the only
// difference is which function gets called.

fn sum_with_inlined(data: &[u64]) -> u64 {
    let mut total = 0u64;
    for &x in data {
        total = hot_add_inlined(total, x);
    }
    total
}

fn sum_with_noinline(data: &[u64]) -> u64 {
    let mut total = 0u64;
    for &x in data {
        total = hot_add_noinline(total, x);
    }
    total
}

// ---------------------------------------------------------------------------
// Experiment 2: element-wise f32 multiply-add.
//
// Pure element-wise: no reduction, no dependency chain, no branches.
// This is the loop shape the vectorizer handles most aggressively, and
// it's the one where the target-feature flags are most likely to change
// the emitted code.
//
// `#[inline(never)]` so objdump finds it as a named symbol. In
// production code you wouldn't add this; the compiler usually does the
// right thing with inlining on its own.
// ---------------------------------------------------------------------------

#[inline(never)]
fn scale_add(a: &[f32], b: &[f32], out: &mut [f32]) {
    for i in 0..a.len() {
        out[i] = a[i] * b[i] + 1.0;
    }
}

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
        "{:<22} {:>8.2} ms   (checksum {})",
        label,
        elapsed.as_secs_f64() * 1000.0,
        result
    );
    result
}

// ---------------------------------------------------------------------------
// main.
// ---------------------------------------------------------------------------

fn main() {
    // ---- Inlining demo ----
    println!("Inlining: {} u64, two drivers\n", ADD_N);

    let mut rng = Rng::new(0xDEADBEEF);
    let data: Vec<u64> = (0..ADD_N).map(|_| rng.next() & 0xFFFF).collect();

    time_it("sum_with_inlined", || sum_with_inlined(&data));
    time_it("sum_with_noinline", || sum_with_noinline(&data));
    println!();

    // ---- scale_add ----
    println!("scale_add: {} f32 elements\n", SCALE_N);

    let a: Vec<f32> = (0..SCALE_N).map(|i| (i % 100) as f32 * 0.5).collect();
    let b: Vec<f32> = (0..SCALE_N).map(|i| (i % 73) as f32 * 0.25).collect();
    let mut out: Vec<f32> = vec![0.0; SCALE_N];

    // Warm-up. Not timed.
    scale_add(&a, &b, &mut out);

    let start = Instant::now();
    scale_add(&a, &b, &mut out);
    let elapsed = start.elapsed();

    // Sum the output so the loop can't be optimized away.
    let checksum: u64 = out.iter().map(|&x| x as u64).sum();
    println!(
        "{:<22} {:>8.2} ms   (checksum {})",
        "scale_add",
        elapsed.as_secs_f64() * 1000.0,
        checksum
    );

    black_box(&data);
    black_box(&a);
    black_box(&b);
    black_box(&out);
}

// ---------------------------------------------------------------------------
// What to expect:
//
// Inlining:
//
// The timer will show a difference between the inlined and noinline
// drivers. A one-instruction function called a million times pays a
// lot of call overhead, and inlining eliminates all of it. The exact
// ratio depends on the loop, but a small function called in a tight
// loop is where inlining matters most.
//
// The noinline version exists as a symbol; the inlined version doesn't.
// That's the observable difference, and it's the same on every machine.
//
// Target features:
//
// The four builds produce different loops for scale_add. The default
// build uses SSE (xmm registers, addps/mulps, 4 floats per instruction).
// The +avx2 build uses AVX2 (ymm registers, vmulps, 8 floats). The
// +avx512f build uses AVX-512 (zmm registers, vmulps, 16 floats).
// The native build typically stops at AVX2, because LLVM's default
// cost model prefers 256-bit vectors on most Intel microarchitectures
// even when 512-bit is available.
//
// Your results may differ depending on your CPU. The point is not to
// match a specific output — it's to observe your own, and to see that
// "the feature is enabled" and "the compiler used it" are two
// different facts.
//
// ---------------------------------------------------------------------------
// Try changing:
//
// 1. In the inlining demo, look at the assembly for both functions:
//
//        cargo asm sample::hot_add_noinline
//        cargo asm sample::hot_add_inlined
//
//    The first has a symbol. The second doesn't, because it was
//    inlined. That's the observable effect of the attribute.
//
// 2. Build the file four ways and dump the assembly for scale_add each
//    time (see the commands in the header comment). For each build, note:
//
//        - which registers appear (xmm, ymm, zmm)
//        - which instruction prefixes appear (addps vs. vaddps)
//        - the byte count in the loop counter (add $0x..., %rax)
//
//    Write down what each build produced.
//
// 3. Run the timer in each build. Note whether the wall time changed
//    when the assembly changed. On a compute-bound loop like scale_add,
//    wider registers usually help — but on a memory-bound loop, they
//    wouldn't, and the width wouldn't matter.
//
// 4. Try the same four builds on the count_above function from Chapter
//    10. It's a byte-counting loop, and it may produce the same SSE2
//    code under every build, because the loop is memory-bound and the
//    compiler's cost model decides wider instructions aren't worth it.
//    Both outcomes are valid — the flag expands what's permitted, and
//    the compiler decides whether to use the permission.
//
// 5. Pick a loop from your own code that you suspect is vectorizable.
//    Build it with the same four configurations and compare. If the
//    compiler doesn't vectorize it at all, look at why: the loop
//    shape, a dependency, aliasing, or the type of arithmetic
//    involved. Chapter 10 covers the common cases.
// ---------------------------------------------------------------------------
