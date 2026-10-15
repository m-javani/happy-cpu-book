#![allow(unused)]
// Chapter 5 — Sharing Memory Between Cores
//
// Copy this file into src/main.rs of a fresh `cargo new` project.
// Run with:  cargo run --release
//
// The book's exercise: eight threads, each incrementing its own counter
// ten million times. Two versions:
//
//   1. unpadded()  — Vec<AtomicU64>, all eight shards in one cache line
//   2. padded()    — Vec<Padded>, each shard on its own cache line
//
// Switch variants by commenting/uncommenting one line in `main`.
//
// The two versions do the same arithmetic — the same number of atomic
// increments — but the padded version should be several times faster,
// because the unpadded version's cache line ping-pongs between cores on
// every increment. That's false sharing.

use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Instant;

const THREADS: usize = 8;
const ITERS: usize = 10_000_000;

// ---------------------------------------------------------------------------
// Version 1: unpadded shards.
//
// AtomicU64 is 8 bytes. Eight of them are exactly 64 bytes — one cache
// line on x86-64. So all eight shards share a line, even though each
// thread only ever touches its own shard.
//
// When thread 0 writes shards[0], it must take exclusive ownership of the
// whole line. Thread 1's copy of that line is invalidated, even though
// thread 1 was only using shards[1]. The line ping-pongs between cores on
// every increment.
// ---------------------------------------------------------------------------

fn unpadded() -> Vec<AtomicU64> {
    (0..THREADS).map(|_| AtomicU64::new(0)).collect()
}

// ---------------------------------------------------------------------------
// Version 2: padded shards.
//
// #[repr(align(64))] does two things:
//
//   - It forces every Padded to *start* at a 64-byte boundary.
//   - It forces size_of::<Padded>() to be 64, not 8. The compiler adds
//     56 bytes of padding after the AtomicU64.
//
// So a Vec<Padded> of length 8 occupies 512 bytes — eight separate cache
// lines. Thread 0's writes touch line 0 only; thread 1's touch line 1
// only. No invalidation, no ping-pong.
//
// The important part is that `align(64)` is on the *element* type, not on
// the container. Putting it on the container would align the Vec's base
// pointer but not space out the elements, and the shards would still
// share lines. This is the most common mistake with this technique.
//
// Cache line size is 64 bytes on x86-64 and most ARM. On Apple Silicon
// it's 128. If you need this to work everywhere, use
// `crossbeam_utils::CachePadded<T>`, which picks the right constant per
// target.
// ---------------------------------------------------------------------------

#[repr(align(64))]
struct Padded(AtomicU64);

fn padded() -> Vec<Padded> {
    (0..THREADS).map(|_| Padded(AtomicU64::new(0))).collect()
}

// ---------------------------------------------------------------------------
// Run THREADS threads against the given shard vector.
//
// Each thread increments only its own shard. `load` on the shard tells
// the compiler the value is used, so the loop can't be optimized away.
// ---------------------------------------------------------------------------

fn run(shards: &[impl Shard]) -> u64 {
    let start = Instant::now();

    thread::scope(|s| {
        for t in 0..THREADS {
            let shards = &shards;
            s.spawn(move || {
                for _ in 0..ITERS {
                    shards[t].inc();
                }
            });
        }
    });

    let elapsed = start.elapsed();
    let total: u64 = (0..THREADS).map(|i| shards[i].get()).sum();

    println!(
        "  {:>6.1} ms  (total = {}, expected = {})",
        elapsed.as_secs_f64() * 1000.0,
        total,
        (THREADS * ITERS) as u64
    );

    total
}

// A tiny trait so `run` can take either shard type.
trait Shard: Sync {
    fn inc(&self);
    fn get(&self) -> u64;
}

impl Shard for AtomicU64 {
    fn inc(&self) {
        self.fetch_add(1, Ordering::Relaxed);
    }
    fn get(&self) -> u64 {
        self.load(Ordering::Relaxed)
    }
}

impl Shard for Padded {
    fn inc(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
    fn get(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

// ---------------------------------------------------------------------------
// main — the switchboard.
// ---------------------------------------------------------------------------

fn main() {
    println!("threads = {}, iters/thread = {}\n", THREADS, ITERS);

    println!("unpadded (8 shards in one cache line):");
    run(&unpadded());

    // println!("padded (one shard per cache line):");
    // run(&padded());
}

// ---------------------------------------------------------------------------
// Try changing:
//
// 1. Uncomment both blocks and run them back to back. The padded version
//    should be several times faster. On the author's machine the
//    difference was roughly 8x. On yours it may be more or less,
//    depending on core count and cache topology.
//
// 2. Comment out the `#[repr(align(64))]` on `Padded` (leave the struct
//    definition). Run again. The padded version should slow down to
//    roughly the unpadded version's speed — because now each Padded is
//    8 bytes again, and eight of them fit in one cache line.
//
// 3. Change THREADS to 2. The gap shrinks, because with fewer threads
//    there are fewer cores fighting over the line. False sharing gets
//    worse as core count goes up.
//
// 4. If you have `perf` on Linux, run both under:
//
//        perf stat -e mem_load_l3_hit_retired.xsnp_fwd ./target/release/<name>
//
//    This counts "snoop hit modified" events — the number of times a core
//    had to pull a line from another core's modified cache. In the
//    unpadded version the count will be enormous; in the padded version
//    it should be near zero. That difference is the fingerprint of false
//    sharing.
//
// 5. Change `run` to use `Ordering::Relaxed` everywhere (it already does).
//    Try `Ordering::SeqCst`. The gap between padded and unpadded will
//    widen, because SeqCst adds memory fences that amplify the coherence
//    traffic. Relaxed is the right choice here; nothing needs
//    synchronization, only atomicity.
//
// 6. Verify that your threads actually ran on different cores. The
//    false-sharing effect requires it — if all eight threads had landed
//    on one core, padding would make no difference. Run:
//
//        perf stat -e task-clock ./target/release/<name>
//
//    and check that task-clock is close to 8x the wall time. If it isn't,
//    the scheduler packed your threads, and the numbers you measured are
//    a mix of real contention and scheduling artifacts.
// ---------------------------------------------------------------------------
