#![allow(unused)]
// Chapter 13 — I/O, Syscalls, and the Kernel Boundary
//
// Copy this file into src/main.rs of a fresh `cargo new` project.
// Run with:  cargo run --release -- <mode> <path>
//
// Modes:
//
//   raw       — read the file in 4 KB chunks with File::read
//   buffered  — read the file with BufReader (64 KB buffer)
//   mmap      — map the file into memory and scan it directly
//
// The path must point to a reasonably large file (say, 500 MB) so the
// differences are visible.
//
// To see the syscall counts, run under strace:
//
//     strace -c ./target/release/<name> raw /path/to/file
//
// The raw mode does one read syscall per 4 KB chunk; the buffered mode
// does one per 64 KB; the mmap mode does almost none after the initial
// mmap.
//
// You need memmap2 for the mmap mode. Add:
//
//     memmap2 = "0.9"
//
// The other modes don't need any external crate.
//
// ---------------------------------------------------------------------------
// Making a test file:
//
//     head -c 500M /dev/urandom > /tmp/big.bin
//
// On Linux, if you don't need the bytes to be random, this is faster:
//
//     fallocate -l 500M /tmp/big.bin
//
// 500 MB is enough to see the differences; larger files show them more
// clearly but take longer to produce.
//
// ---------------------------------------------------------------------------
// On the "cold vs warm page cache" demo:
//
// This file does not include one. That demo would require dropping the
// system page cache with:
//
//     sudo sh -c 'echo 3 > /proc/sys/vm/drop_caches'
//
// which degrades performance for every process on the machine until the
// cache refills. It's not worth running for a book demo, and on an NVMe
// SSD the ratio is small enough that the effect is hard to see anyway.
//
// The page cache is real, and it explains why repeated reads of the
// same file are fast. But demonstrating it cleanly requires an isolated
// machine, and the chapter's other lessons don't need it.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Timing helper. Prints wall time and total bytes read.
// ---------------------------------------------------------------------------

fn report(label: &str, elapsed: std::time::Duration, bytes: u64) {
    let seconds = elapsed.as_secs_f64();
    let mb_per_sec = (bytes as f64 / (1024.0 * 1024.0)) / seconds;
    println!(
        "{:<12} {:>8.1} ms   {:>8} bytes   {:>7.1} MB/s",
        label,
        seconds * 1000.0,
        bytes,
        mb_per_sec
    );
}

// ---------------------------------------------------------------------------
// Mode 1: raw read in 4 KB chunks.
//
// Each iteration of the loop calls File::read once, which is one syscall
// (assuming the kernel doesn't coalesce). For a 500 MB file, that's
// 128,000 syscalls. Each syscall is a user-to-kernel transition, and the
// transition cost dominates for reads this small.
//
// This is the "unbuffered" version the chapter describes. It's not what
// you'd write in real code — File::read on Linux won't necessarily
// return the full chunk size, so you'd need a loop anyway. But it
// isolates the per-syscall cost.
// ---------------------------------------------------------------------------

fn mode_raw(path: &Path) -> std::io::Result<()> {
    let mut file = File::open(path)?;
    let mut buf = [0u8; 4096];
    let mut total: u64 = 0;
    let mut checksum: u64 = 0;

    let start = Instant::now();
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        // Touch the data so the compiler can't eliminate the read.
        checksum = checksum.wrapping_add(buf[..n].iter().map(|&b| b as u64).sum::<u64>());
    }
    let elapsed = start.elapsed();

    report("raw", elapsed, total);
    println!("  checksum = {}", checksum);
    Ok(())
}

// ---------------------------------------------------------------------------
// Mode 2: BufReader with a 64 KB buffer.
//
// BufReader reads in 64 KB chunks from the underlying file and serves
// smaller reads from its internal buffer. The syscall count is 1/16th
// of the raw mode, because the same total bytes are transferred in 16
// times fewer syscalls.
//
// The buffer size is the parameter to tune. The default is 8 KB. 64 KB
// is a common choice for high-throughput workloads. The upper limit is
// usually the L2 cache size — beyond that, the buffer stops being
// cheap to serve from.
// ---------------------------------------------------------------------------

fn mode_buffered(path: &Path) -> std::io::Result<()> {
    let file = File::open(path)?;
    let mut reader = BufReader::with_capacity(64 * 1024, file);
    let mut buf = [0u8; 4096];
    let mut total: u64 = 0;
    let mut checksum: u64 = 0;

    let start = Instant::now();
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        checksum = checksum.wrapping_add(buf[..n].iter().map(|&b| b as u64).sum::<u64>());
    }
    let elapsed = start.elapsed();

    report("buffered", elapsed, total);
    println!("  checksum = {}", checksum);
    Ok(())
}

// ---------------------------------------------------------------------------
// Mode 3: mmap.
//
// The file is mapped into the process's address space. Reading from it
// is a memory load; the page fault brings the page in from the page
// cache (or disk on the first access). No read syscall per chunk — the
// kernel handles the page faults, which are cheaper per page than a
// syscall.
//
// The catch: mmap works best when the file is already in the page
// cache. On a cold file, each page fault has to fetch from disk, and
// the performance is worse than buffered read. On a warm file, mmap
// is typically the fastest option.
//
// SAFETY: memmap2::Mmap::map is unsafe because the underlying file
// could be modified by another process while we're reading it, which
// would be undefined behavior. For a read-only scan of a file we
// control, this is fine. In general, mmap on files that other
// processes might write is a genuine hazard.
// ---------------------------------------------------------------------------

fn mode_mmap(path: &Path) -> std::io::Result<()> {
    let file = File::open(path)?;
    let mmap = unsafe { memmap2::Mmap::map(&file)? };

    let start = Instant::now();
    // Scan the whole mapping. The reads here are memory loads, not
    // syscalls — the kernel handles page faults behind the scenes.
    let checksum: u64 = mmap.iter().map(|&b| b as u64).sum();
    let elapsed = start.elapsed();

    report("mmap", elapsed, mmap.len() as u64);
    println!("  checksum = {}", checksum);
    Ok(())
}

// ---------------------------------------------------------------------------
// main.
// ---------------------------------------------------------------------------

fn main() -> std::io::Result<()> {
    let mode = std::env::args().nth(1).unwrap_or_default();
    let path = std::env::args().nth(2);

    let path = match path {
        Some(p) => p,
        None => {
            eprintln!(
                "usage: {} <raw|buffered|mmap> <path>",
                std::env::args().next().unwrap()
            );
            eprintln!();
            eprintln!("  raw       read in 4 KB chunks with File::read");
            eprintln!("  buffered  read with BufReader (64 KB)");
            eprintln!("  mmap      map the file and scan it directly");
            eprintln!();
            eprintln!("The path should point to a large file (500 MB or so).");
            eprintln!("See the comment at the top for how to make one.");
            return Ok(());
        }
    };

    let path = Path::new(&path);

    match mode.as_str() {
        "raw" => mode_raw(path)?,
        "buffered" => mode_buffered(path)?,
        "mmap" => mode_mmap(path)?,
        _ => {
            eprintln!("unknown mode: {}", mode);
            eprintln!("expected one of: raw, buffered, mmap");
            return Ok(());
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// What to expect:
//
// Raw: one read syscall per 4 KB chunk. For a 500 MB file, that's
// ~128,000 syscalls. At ~200 ns per syscall (post-Spectre mitigations
// on a modern CPU), the syscall overhead alone is ~25 ms. The data
// transfer itself adds more, but the syscall path is a significant
// fraction of the total.
//
// Buffered: one syscall per 64 KB. That's ~8,000 syscalls for the same
// 500 MB file — a 16x reduction. The wall time is usually 1.5-3x faster
// than raw. The exact ratio depends on how fast your disk is: if the
// file is in the page cache, the syscall count dominates, and buffered
// wins big. If the disk is the bottleneck, both versions are limited
// by disk throughput and the syscall difference matters less.
//
// mmap: after the initial mmap syscall, the reads are memory loads,
// not syscalls. Page faults bring pages in from the page cache (or
// disk on a cold run). On a warm file, mmap is typically the fastest
// of the three modes — often 1.5-2x faster than raw, because it skips
// the kernel-to-user copy entirely.
//
// To see the syscall counts directly, run each mode under strace:
//
//     strace -c ./target/release/<name> raw /tmp/big.bin
//     strace -c ./target/release/<name> buffered /tmp/big.bin
//     strace -c ./target/release/<name> mmap /tmp/big.bin
//
// The "calls" column for read will be ~125,000 for raw, ~8,000 for
// buffered, and near zero for mmap.
//
// ---------------------------------------------------------------------------
// Try changing:
//
// 1. Run all three modes on the same file. Compare the wall times and
//    (if you have strace) the syscall counts.
//
// 2. Change the buffer size in mode_buffered from 64 KB to 8 KB and
//    then to 1 MB. Note the wall-time difference. The default 8 KB is
//    a compromise; larger is usually better for throughput, up to the
//    point where the buffer doesn't fit in L2.
//
// 3. Change the chunk size in mode_raw from 4 KB to 64 KB. This shows
//    that the chunk size, not the buffering strategy, is what
//    determines the syscall count. A raw read with 64 KB chunks is
//    equivalent to a buffered read with 64 KB chunks (minus the
//    BufReader overhead).
//
// 4. If you have a second disk or a network filesystem, run the file
//    from there. The mmap mode is much more sensitive to where the
//    file actually lives than the buffered mode.
// ---------------------------------------------------------------------------
