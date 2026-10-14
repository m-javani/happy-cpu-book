# Companion Code

Each chapter has a runnable example in its own directory. To try one,
copy the `main.rs` into a fresh `cargo new` project, then:

```
cargo run --release
```

The comments in each file explain what to look for and what to change.

| Chapter | Directory | What it demonstrates |
|---|---|---|
| 02 | [chapter-02/](chapter-02/) | Cache hierarchy: interleaved vs. separated layouts |
| 03 | [chapter-03/](chapter-03/) | Layout: `Vec<T>` vs. `Vec<Box<T>>` |
| 04 | [chapter-04/](chapter-04/) | Temporal locality: naive vs. running-sum windows |
| 05 | [chapter-05/](chapter-05/) | False sharing: padded vs. unpadded counters |
| 06 | [chapter-06/](chapter-06/) | TLB and paging: raw vs. pre-faulted allocation |
| 08 | [chapter-08/](chapter-08/) | Pipeline: single vs. four accumulators |
| 09 | [chapter-09/](chapter-09/) | Top-down: matrix multiply profiling |
| 10 | [chapter-10/](chapter-10/) | SIMD: count, sum, and `memchr` |
| 11 | [chapter-11/](chapter-11/) | Allocation: counting allocator and patterns |
| 12 | [chapter-12/](chapter-12/) | Zero-copy: strings, HTTP, `Cow` |
| 13 | [chapter-13/](chapter-13/) | I/O: raw, buffered, and `mmap` reads |
| 14 | [chapter-14/](chapter-14/) | Compiler: inlining and target features |
| 15 | [chapter-15/spmv/](chapter-15/spmv/) | Case study: SpMV in CSR, ELL, and SIMD |

Chapter 7 (NUMA) has no runnable code. Its exercise asks you to write
your own small program and run it on a multi-socket machine — the
effect the chapter describes can't be demonstrated on a single-node
laptop.

Chapter 9's file is a sandbox for the top-down workflow, not a
standalone exercise. The chapter's exercise asks you to profile a
program *you* care about.

## A note on tooling

Chapters 5, 6, 9, 13, and 14 use external tools:

- **`perf`** — Linux only. Install with your distribution's package
  manager (`linux-tools-*` on Debian/Ubuntu, `perf` on Fedora/Arch).
  Not available on macOS.
- **`strace`** — Linux only. For Chapter 13's syscall counts.
- **`cargo-asm`** — install with `cargo install cargo-asm`. Used in
  Chapters 10 and 14.
- **`numactl`** — Linux only. For Chapter 7, on a multi-socket machine.

The chapters that use these tools explain what to do when they aren't
available.

