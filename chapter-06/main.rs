#![allow(unused)]
// Chapter 6 — The TLB and Paging
//
// Copy this file into src/main.rs of a fresh `cargo new` project.
// Run with:  cargo run --release
//
// The book's exercise: allocate 1 GiB, write 1 GiB of bytes, scan it.
// Two variants:
//
//   1. lazy()      — pages are faulted in as the write loop touches them.
//                    The fault cost is interleaved with the writes.
//   2. prefaulted() — every 4 KB page is touched once before the timed
//                    write loop, so the faults happen in one batch,
//                    outside the measurement.
//
// Switch variants by commenting/uncommenting one line in `main`.
//
// Both variants write the same 1 GiB of the same bytes and scan the same
// committed buffer. The only difference is *when* the minor faults
// happen: interleaved with the write loop (lazy) or batched before it
// (prefaulted). The scan times should be nearly identical; the write
// times should differ by the fault cost.
//
// The TLB side of the chapter (dTLB-load-misses, huge pages) is measured
// with `perf` and `madvise`, not with `cargo run`. See the "measuring
// with perf" block at the bottom of this file.

use std::time::Instant;

const N: usize = 1 << 30; // 1 GiB
const PAGE: usize = 4096;

// ---------------------------------------------------------------------------
// The scan. Identical in both variants. Perfectly sequential, perfectly
// prefetcher-friendly. Any timing difference between the two variants
// comes from the pages, not from the data.
//
// Note: the scan does not pay fault cost in either variant. The pages
// were committed by the write loop. The scan's cost is translation
// pressure in the TLB, which you measure with `perf stat -e
// dTLB-loads,dTLB-load-misses`, not with a timer. See the block at the
// bottom of this file.
// ---------------------------------------------------------------------------

fn sum_bytes(data: &[u8]) -> u64 {
    let mut sum = 0u64;
    for &b in data {
        sum = sum.wrapping_add(b as u64);
    }
    sum
}

// ---------------------------------------------------------------------------
// Variant 1: lazy.
//
// We reserve 1 GiB of virtual address space with with_capacity. The
// kernel does not commit physical pages yet. Each write to a fresh page
// triggers a minor fault: the program stops, the kernel finds a physical
// frame, updates the page table, and resumes.
//
// A 1 GiB buffer is 262,144 pages. The faults are interleaved with the
// writes — for every 4 KB of writing, one fault. The kernel cannot batch
// them, because it doesn't know which page we'll touch next.
//
// The scan that follows runs over committed pages. No faults during the
// scan.
// ---------------------------------------------------------------------------

fn lazy() {
    let mut data: Vec<u8> = Vec::with_capacity(N);

    let start = Instant::now();

    // Write every byte. `(i & 0xFF) as u8` gives a repeating 0..=255
    // pattern, so the sum is deterministic and comparable across variants.
    //
    // SAFETY: we reserved N bytes with with_capacity, so writing to
    // indices 0..N is within the reserved capacity. We use raw pointer
    // writes because we want to write past the Vec's current length
    // without going through push() one byte at a time. After the loop,
    // set_len(N) tells the Vec that all N bytes are initialized.
    unsafe {
        let ptr = data.as_mut_ptr();
        for i in 0..N {
            std::ptr::write(ptr.add(i), (i & 0xFF) as u8);
        }
        data.set_len(N);
    }

    let write_elapsed = start.elapsed();

    let start = Instant::now();
    let sum = sum_bytes(&data);
    let scan_elapsed = start.elapsed();

    println!(
        "lazy:        write {:>7.1} ms   scan {:>7.1} ms   sum = {}",
        write_elapsed.as_secs_f64() * 1000.0,
        scan_elapsed.as_secs_f64() * 1000.0,
        sum
    );
}

// ---------------------------------------------------------------------------
// Variant 2: prefaulted.
//
// Same allocation, same writes, same scan — but every 4 KB page is
// touched once before the timed write loop. The pre-fault writes one
// byte per page, which forces the kernel to commit every page upfront.
//
// The pre-fault loop is outside the timer. It's the cost we're moving,
// not eliminating: the faults still happen, we just pay for them before
// the measured region instead of during it.
//
// The timed write loop then runs over committed pages and pays no fault
// cost. The scan that follows is identical to the lazy variant's scan.
// ---------------------------------------------------------------------------

fn prefaulted() {
    let mut data: Vec<u8> = Vec::with_capacity(N);

    // Pre-fault: one write per page, outside the timed region.
    //
    // SAFETY: same reasoning as above. Each write lands inside the
    // reserved capacity. We are not reading the value; the write alone
    // is what commits the page.
    for i in (0..N).step_by(PAGE) {
        unsafe {
            std::ptr::write(data.as_mut_ptr().add(i), 0);
        }
    }

    let start = Instant::now();

    // Same write pattern as the lazy variant. The pages already exist,
    // so no faults are taken here.
    unsafe {
        let ptr = data.as_mut_ptr();
        for i in 0..N {
            std::ptr::write(ptr.add(i), (i & 0xFF) as u8);
        }
        data.set_len(N);
    }

    let write_elapsed = start.elapsed();

    let start = Instant::now();
    let sum = sum_bytes(&data);
    let scan_elapsed = start.elapsed();

    println!(
        "prefaulted:  write {:>7.1} ms   scan {:>7.1} ms   sum = {}",
        write_elapsed.as_secs_f64() * 1000.0,
        scan_elapsed.as_secs_f64() * 1000.0,
        sum
    );
}

// ---------------------------------------------------------------------------
// main — the switchboard.
// ---------------------------------------------------------------------------

fn main() {
    println!("N = {} bytes ({} MiB)\n", N, N >> 20);

    // The scan times will be nearly identical between variants — both
    // scan a committed buffer. The difference is in the write, and it's
    // the page faults. See the note above `sum_bytes`.

    lazy();
    // prefaulted();
}

// ---------------------------------------------------------------------------
// Measuring with perf (the parts that need a tool, not a cargo run):
//
// 1. TLB miss rate. Run either variant under:
//
//        perf stat -e dTLB-loads,dTLB-load-misses \
//            ./target/release/<name>
//
//    dTLB-load-misses / dTLB-loads is the miss rate. On a sequential
//    scan of 1 GiB with 4 KB pages, this is typically a few percent.
//    The miss rate is what huge pages are meant to reduce.
//
// 2. Minor faults. Run either variant under:
//
//        perf stat -e minor-faults,major-faults \
//            ./target/release/<name>
//
//    Both variants show roughly the same total fault count (~262,144).
//    The faults don't disappear; they move. That's the lesson.
//
// 3. Page walk cost:
//
//        perf stat -e dtlb_load_misses.walk_completed \
//            ./target/release/<name>
//
//    (Event name varies by CPU; check `perf list`.)
//
// 4. Transparent Huge Pages. Requires Linux, THP in madvise mode, and a
//    call to madvise(MADV_HUGEPAGE) on the buffer. That needs `libc` or
//    `nix` as a dependency, so it's not in this file. The book's §7
//    shows the pattern. If you try it, watch the dTLB-load-misses count
//    drop — that's the TLB coverage improvement from 2 MB pages.
//
// ---------------------------------------------------------------------------
// Try changing:
//
// 1. Run both variants and compare the `write` line. The lazy write
//    should be measurably slower, because it pays fault cost interleaved
//    with its work. The prefaulted write pays none. The `scan` lines
//    should be nearly identical — both scan a fully committed buffer.
//
// 2. Reduce N to 1 << 26 (64 MiB). The write-time difference should
//    shrink, because 64 MiB is only 16,384 pages — a much smaller fault
//    bill. The scan times should stay close to each other.
//
// 3. Reduce N to 1 << 20 (1 MiB). The difference should disappear. At
//    1 MiB, the fault cost is negligible compared to the writes.
//
// 4. Compare the *total* time (write + scan) of the two variants. They
//    should be close. The prefaulted version isn't doing less work; it's
//    doing the same work in a different order. The point is that faults
//    on the critical path cost more than faults batched upfront.
//
// 5. Change PAGE to 8192 in the prefaulted variant. The fault count
//    won't change — the kernel still uses 4 KB pages regardless of how
//    often you touch them. This demonstrates that the page size is the
//    kernel's choice, not the program's, unless you explicitly ask for
//    huge pages.
// ---------------------------------------------------------------------------
