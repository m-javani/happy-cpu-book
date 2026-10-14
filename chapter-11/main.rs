#![allow(unused)]
// Chapter 11 — Allocation Is Not Free
//
// Copy this file into src/main.rs of a fresh `cargo new` project.
// Run with:  cargo run --release
//
// Five demos, each showing one of the patterns from the chapter:
//
//   1. push_growth()      — Vec::new() + push vs Vec::with_capacity
//   2. scratch_per_iter() — allocate per iteration vs reuse + clear
//   3. boxed_tree()       — Box<Node> tree vs bumpalo arena
//   4. remove_vs_swap()   — Vec::remove (O(n)) vs Vec::swap_remove (O(1))
//   5. counting allocator — wraps every demo, counts allocations
//
// Switch demos by commenting/uncommenting lines in `main`.
//
// The counting allocator is installed globally, so every allocation the
// program makes — including Vec growth, String construction, and
// everything else — is counted. The demos print their own counts at
// the end of each run.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

// ---------------------------------------------------------------------------
// Counting allocator.
//
// Wraps the system allocator. Increments ALLOCATIONS on every alloc, and
// ALLOC_BYTES by the requested size. dealloc just forwards to the system
// allocator — we're counting allocations, not frees.
//
// This is global: install it once and every allocation in the program
// is counted, regardless of which module made it.
// ---------------------------------------------------------------------------

struct CountingAllocator;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static ALLOC_BYTES: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

fn alloc_count() -> usize {
    ALLOCATIONS.load(Ordering::Relaxed)
}

fn alloc_bytes() -> usize {
    ALLOC_BYTES.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Timing and reporting helper. Prints wall time and delta in
// allocation count since the start of the timed region.
// ---------------------------------------------------------------------------

fn time_it<F: FnOnce() -> u64>(label: &str, f: F) -> u64 {
    let allocs_before = alloc_count();
    let bytes_before = alloc_bytes();
    let start = Instant::now();

    let result = f();

    let elapsed = start.elapsed();
    let allocs_after = alloc_count();
    let bytes_after = alloc_bytes();
    let allocs = allocs_after - allocs_before;
    let bytes = bytes_after - bytes_before;

    println!(
        "{:<26} {:>8.2} ms   {:>8} allocs   {:>10} bytes   (checksum {})",
        label,
        elapsed.as_secs_f64() * 1000.0,
        allocs,
        bytes,
        result
    );
    result
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

// ===========================================================================
// Demo 1: Vec growth — push vs with_capacity
// ===========================================================================

const DEMO1_N: usize = 1_000_000;

fn demo1_push_growth() {
    println!("Demo 1a: Vec::new() + push, {} elements", DEMO1_N);
    time_it("push_growth", || {
        let mut v: Vec<u64> = Vec::new();
        for i in 0..DEMO1_N {
            v.push(i as u64);
        }
        v.iter().sum()
    });
    println!();
}

fn demo1_with_capacity() {
    println!("Demo 1b: Vec::with_capacity, {} elements", DEMO1_N);
    time_it("with_capacity", || {
        let mut v: Vec<u64> = Vec::with_capacity(DEMO1_N);
        for i in 0..DEMO1_N {
            v.push(i as u64);
        }
        v.iter().sum()
    });
    println!();
}

// ===========================================================================
// Demo 2: scratch buffer per iteration vs reuse
// ===========================================================================

const DEMO2_ITERS: usize = 100_000;
const DEMO2_SCRATCH: usize = 64;

fn demo2_scratch_per_iter() {
    println!(
        "Demo 2a: fresh scratch per iteration, {} iters",
        DEMO2_ITERS
    );
    time_it("scratch_per_iter", || {
        let mut checksum = 0u64;
        for i in 0..DEMO2_ITERS {
            let mut scratch: Vec<u64> = Vec::new();
            for j in 0..DEMO2_SCRATCH {
                scratch.push((i * DEMO2_SCRATCH + j) as u64);
            }
            checksum = checksum.wrapping_add(scratch.iter().sum::<u64>());
        }
        checksum
    });
    println!();
}

fn demo2_reused_scratch() {
    println!("Demo 2b: reused scratch buffer, {} iters", DEMO2_ITERS);
    time_it("reused_scratch", || {
        let mut scratch: Vec<u64> = Vec::with_capacity(DEMO2_SCRATCH);
        let mut checksum = 0u64;
        for i in 0..DEMO2_ITERS {
            scratch.clear();
            for j in 0..DEMO2_SCRATCH {
                scratch.push((i * DEMO2_SCRATCH + j) as u64);
            }
            checksum = checksum.wrapping_add(scratch.iter().sum::<u64>());
        }
        checksum
    });
    println!();
}

// ===========================================================================
// Demo 3: boxed tree vs bumpalo arena
//
// Build a small tree of nodes. Each Box<Node> is a separate allocation.
// Each Node in the arena is a pointer bump with no per-object bookkeeping.
//
// We build a tree of depth 20 with two children per node, then sum all
// the values. The node count is 2^21 - 1 ≈ 2 million.
// ===========================================================================

const TREE_DEPTH: usize = 20;

struct Node {
    value: u64,
    children: Vec<Box<Node>>,
}

fn build_tree(depth: usize, seed: u64) -> Box<Node> {
    let mut children = Vec::new();
    if depth > 0 {
        children.push(build_tree(depth - 1, seed.wrapping_mul(2)));
        children.push(build_tree(depth - 1, seed.wrapping_mul(2).wrapping_add(1)));
    }
    Box::new(Node {
        value: seed,
        children,
    })
}

fn sum_tree(node: &Node) -> u64 {
    let mut total = node.value;
    for child in &node.children {
        total = total.wrapping_add(sum_tree(child));
    }
    total
}

fn demo3_boxed_tree() {
    println!("Demo 3a: Box<Node> tree, depth {}", TREE_DEPTH);
    time_it("boxed_tree", || {
        let root = build_tree(TREE_DEPTH, 1);
        sum_tree(&root)
    });
    println!();
}

// The bumpalo version. NodeArena holds references into the Bump, which
// is owned outside. This is the standard bumpalo pattern: the allocator
// outlives everything allocated from it, and dropping it frees the
// whole tree at once.
//
// We need to add bumpalo to Cargo.toml:
//
//     bumpalo = "3"
//
// If you don't want to add the dependency, comment out demo3_bump_tree
// and its call site in main.
struct NodeArena<'bump> {
    value: u64,
    children: Vec<&'bump NodeArena<'bump>>,
}

fn build_tree_arena<'bump>(
    bump: &'bump bumpalo::Bump,
    depth: usize,
    seed: u64,
) -> &'bump NodeArena<'bump> {
    let mut children = Vec::new();
    if depth > 0 {
        children.push(build_tree_arena(bump, depth - 1, seed.wrapping_mul(2)));
        children.push(build_tree_arena(
            bump,
            depth - 1,
            seed.wrapping_mul(2).wrapping_add(1),
        ));
    }
    bump.alloc(NodeArena {
        value: seed,
        children,
    })
}

fn sum_tree_arena(node: &NodeArena) -> u64 {
    let mut total = node.value;
    for child in &node.children {
        total = total.wrapping_add(sum_tree_arena(child));
    }
    total
}

fn demo3_bump_tree() {
    println!("Demo 3b: bumpalo tree, depth {}", TREE_DEPTH);
    time_it("bump_tree", || {
        let bump = bumpalo::Bump::new();
        let root = build_tree_arena(&bump, TREE_DEPTH, 1);
        sum_tree_arena(root)
    });
    println!();
}

// ===========================================================================
// Demo 4: Vec::remove vs Vec::swap_remove
// ===========================================================================

const DEMO4_N: usize = 100_000;
const DEMO4_REMOVALS: usize = 10_000;

fn demo4_remove() {
    println!("Demo 4a: Vec::remove, {} removals", DEMO4_REMOVALS);
    time_it("remove", || {
        let mut v: Vec<u64> = (0..DEMO4_N as u64).collect();
        for _ in 0..DEMO4_REMOVALS {
            // Remove from the middle each time.
            v.remove(v.len() / 2);
        }
        v.iter().sum()
    });
    println!();
}

fn demo4_swap_remove() {
    println!("Demo 4b: Vec::swap_remove, {} removals", DEMO4_REMOVALS);
    time_it("swap_remove", || {
        let mut v: Vec<u64> = (0..DEMO4_N as u64).collect();
        for _ in 0..DEMO4_REMOVALS {
            v.swap_remove(v.len() / 2);
        }
        v.iter().sum()
    });
    println!();
}

// ===========================================================================
// main — the switchboard.
// ===========================================================================

fn main() {
    let which = std::env::args().nth(1).unwrap_or_default();
    match which.as_str() {
        "1a" => demo1_push_growth(),
        "1b" => demo1_with_capacity(),
        "2a" => demo2_scratch_per_iter(),
        "2b" => demo2_reused_scratch(),
        "3a" => demo3_boxed_tree(),
        "3b" => demo3_bump_tree(),
        "4a" => demo4_remove(),
        "4b" => demo4_swap_remove(),
        _ => eprintln!(
            "usage: {} <1a|1b|2a|2b|3a|3b|4a|4b>",
            std::env::args().next().unwrap()
        ),
    }
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// What to expect:
//
// Demo 1: push_growth makes ~20-30 allocations (Vec doubles on each
// growth). with_capacity makes 1. The wall-time difference is small for
// 1M u64 (a few ms), but the allocation count is stark: 19 vs 1. On
// larger workloads the wall-time gap widens because of the memcpy on
// each doubling.
//
// Demo 2: scratch_per_iter makes 500,000 allocations, not 100,000.
// Each iteration creates a fresh Vec and pushes 64 elements into it.
// The Vec starts empty, so it grows by doubling as it fills: 4, 8, 16,
// 32, 64 — that's ~5 allocations per iteration. 100,000 iterations ×
// ~5 growth steps = 500,000 allocations.
//
// reused_scratch makes 1 allocation total (the initial with_capacity).
// The buffer grows to 64 elements on the first iteration and never
// again; clear() keeps the capacity.
//
// The wall-time difference is usually 2-3x on the scratch loop. Not
// 10-30x — the per-iteration work is small, and the system allocator is
// fast for small sizes, so the allocation overhead is a meaningful but
// not dominant fraction of the time. The allocation *count* is the
// striking number: 500,000 vs 1.
//
// Demo 3: boxed_tree makes ~3 million allocations. Each node is one
// Box::new (one allocation), and the tree at depth 20 has ~2 million
// nodes. The extra ~1 million comes from the Vec<Box<Node>> children
// vectors — each node allocates a Vec for its children, even when it
// has two or fewer.
//
// bump_tree makes ~1 million allocations, not "a few." The arena
// handles the NodeArena structs (one pointer bump per node), but the
// children are still Vec<&NodeArena>, and those Vecs go through the
// system allocator. A fully arena-based tree would use a fixed-size
// child array instead of a Vec and reduce the allocation count to a
// handful (one per Bump chunk).
//
// The wall-time difference is 2-3x. The bigger win for real workloads
// is cache locality: arena nodes are contiguous, boxed nodes are
// scattered across the heap. That's a Chapter 3 effect on top of the
// Chapter 11 effect.
//
// Demo 4: remove is O(n) per call, so 10,000 calls on a 100K vector
// shift ~500M elements total. swap_remove is O(1) per call, so 10,000
// calls do 10,000 moves. Expect remove to be 100x or more slower. The
// two checksums differ by design: remove shifts survivors left,
// swap_remove moves the last element into the hole. Same number of
// removals, different remaining elements, different sum.
//
// ---------------------------------------------------------------------------
// Try changing:
//
// 1. Run each pair back to back. Compare the allocation counts first,
//    then the wall times. The counts are the clearer signal.
//
// 2. In demo 1, reduce DEMO1_N to 100 and rerun. The allocation counts
//    are similar (Vec still grows the same number of times), but the
//    wall-time difference shrinks because there's less to copy.
//
// 3. In demo 2, increase DEMO2_SCRATCH to 1024. The allocation count
//    doesn't change, but the wall-time gap widens, because the per-
//    iteration work is now larger relative to the allocator call.
//
// 4. In demo 3, try TREE_DEPTH = 15 and TREE_DEPTH = 22. The allocation
//    count for the boxed version grows exponentially; the bump version's
//    count grows linearly (one chunk per N bytes).
//
// 5. In demo 4, change DEMO4_REMOVALS to 1 and to 100,000. `remove`'s
//    cost is proportional to the number of elements shifted; swap_remove
//    stays constant. At one removal, the difference is invisible. At
//    100,000, it's enormous.
//
// 6. Swap in mimalloc or jemalloc as the global allocator (see the
//    chapter's §8) and re-run all the demos. Note which patterns are
//    affected and which aren't. The allocation *counts* don't change,
//    but the wall times may.
// ---------------------------------------------------------------------------
