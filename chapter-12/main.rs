#![allow(unused)]
// Chapter 12 — Zero-Copy: Making the Reader Cheap
//
// Copy this file into src/main.rs of a fresh `cargo new` project.
// Run with:  cargo run --release
//
// Three demos, each showing one of the patterns from the chapter:
//
//   1. get_words_string()  vs get_words_slice()  — Vec<String> vs Vec<&str>
//   2. parse_line_owned()  vs parse_line_borrowed() — HTTP request line
//   3. normalize_owned()   vs normalize_cow()    — Cow<str>
//
// The counting allocator from Chapter 11 is installed globally, so
// every demo reports its own allocation count alongside the wall time.
//
// You need memchr for demo 2. Add:
//
//     memchr = "2"
//
// Demos 1 and 3 don't need any external crate.

use std::alloc::{GlobalAlloc, Layout, System};
use std::borrow::Cow;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

// ---------------------------------------------------------------------------
// Counting allocator (from Chapter 11).
// ---------------------------------------------------------------------------

struct CountingAllocator;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
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

// ---------------------------------------------------------------------------
// Timing helper: prints wall time, allocation delta, and checksum.
// ---------------------------------------------------------------------------

fn time_it<F: FnOnce() -> u64>(label: &str, f: F) -> u64 {
    let allocs_before = alloc_count();
    let start = Instant::now();

    let result = f();

    let elapsed = start.elapsed();
    let allocs = alloc_count() - allocs_before;

    println!(
        "{:<24} {:>8.2} ms   {:>9} allocs   (checksum {})",
        label,
        elapsed.as_secs_f64() * 1000.0,
        allocs,
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

// ---------------------------------------------------------------------------
// Build a synthetic text buffer: N words, each 3-12 bytes, space-separated.
//
// We generate the buffer once and reuse it across both variants, so the
// comparison is honest: same input, same number of words, only the
// extraction strategy differs.
// ---------------------------------------------------------------------------

fn make_text(target_bytes: usize) -> String {
    let mut rng = Rng::new(0xDEADBEEF);
    let mut s = String::with_capacity(target_bytes + 32);
    while s.len() < target_bytes {
        let word_len = 3 + (rng.next() % 10) as usize;
        for _ in 0..word_len {
            // Lowercase ASCII letters.
            let c = b'a' + (rng.next() % 26) as u8;
            s.push(c as char);
        }
        s.push(' ');
    }
    s
}

// ===========================================================================
// Demo 1: word extraction — Vec<String> vs Vec<&str>
// ===========================================================================

fn get_words_string(text: &str) -> Vec<String> {
    text.split_whitespace().map(|s| s.to_string()).collect()
}

fn get_words_slice(text: &str) -> Vec<&str> {
    text.split_whitespace().collect()
}

fn demo1_owned(text: &str) {
    println!("Demo 1a: Vec<String>, {} bytes of text", text.len());
    time_it("get_words_string", || {
        let words = get_words_string(text);
        // Checksum: total length of all words, so the compiler can't
        // optimize the collection away.
        words.iter().map(|w| w.len() as u64).sum()
    });
    println!();
}

fn demo1_borrowed(text: &str) {
    println!("Demo 1b: Vec<&str>, {} bytes of text", text.len());
    time_it("get_words_slice", || {
        let words = get_words_slice(text);
        words.iter().map(|w| w.len() as u64).sum()
    });
    println!();
}

// ===========================================================================
// Demo 2: HTTP request-line parsing — owned vs borrowed
//
// The request line is "METHOD PATH VERSION\r\n". We generate a million
// of them into one buffer, then parse the whole buffer two ways.
//
// The owned version allocates a String for each field. The borrowed
// version returns &str slices into the buffer.
// ===========================================================================

#[derive(Debug)]
struct RequestLineOwned {
    method: String,
    path: String,
    version: String,
}

#[derive(Debug)]
struct RequestLineBorrowed<'a> {
    method: &'a str,
    path: &'a str,
    version: &'a str,
}

fn parse_line_owned(line: &[u8]) -> RequestLineOwned {
    let space1 = memchr::memchr(b' ', line).unwrap();
    let method = String::from_utf8(line[..space1].to_vec()).unwrap();

    let rest = &line[space1 + 1..];
    let space2 = memchr::memchr(b' ', rest).unwrap();
    let path = String::from_utf8(rest[..space2].to_vec()).unwrap();

    let version_bytes = &rest[space2 + 1..];
    // Strip trailing CRLF.
    let version_end = version_bytes
        .iter()
        .position(|&b| b == b'\r' || b == b'\n')
        .unwrap_or(version_bytes.len());
    let version = String::from_utf8(version_bytes[..version_end].to_vec()).unwrap();

    RequestLineOwned {
        method,
        path,
        version,
    }
}

fn parse_line_borrowed(line: &[u8]) -> RequestLineBorrowed<'_> {
    let space1 = memchr::memchr(b' ', line).unwrap();
    let method = std::str::from_utf8(&line[..space1]).unwrap();

    let rest = &line[space1 + 1..];
    let space2 = memchr::memchr(b' ', rest).unwrap();
    let path = std::str::from_utf8(&rest[..space2]).unwrap();

    let version_bytes = &rest[space2 + 1..];
    let version_end = version_bytes
        .iter()
        .position(|&b| b == b'\r' || b == b'\n')
        .unwrap_or(version_bytes.len());
    let version = std::str::from_utf8(&version_bytes[..version_end]).unwrap();

    RequestLineBorrowed {
        method,
        path,
        version,
    }
}

fn make_request_lines(count: usize) -> Vec<u8> {
    let methods = ["GET", "POST", "PUT", "DELETE", "HEAD"];
    let paths = [
        "/",
        "/index.html",
        "/api/v1/users",
        "/api/v1/users/12345",
        "/static/css/main.css",
        "/favicon.ico",
    ];
    let mut rng = Rng::new(0xBEEFCAFE);
    let mut buf = Vec::with_capacity(count * 40);
    for _ in 0..count {
        let method = methods[(rng.next() % methods.len() as u64) as usize];
        let path = paths[(rng.next() % paths.len() as u64) as usize];
        buf.extend_from_slice(method.as_bytes());
        buf.push(b' ');
        buf.extend_from_slice(path.as_bytes());
        buf.extend_from_slice(b" HTTP/1.1\r\n");
    }
    buf
}

fn split_lines(buf: &[u8]) -> Vec<&[u8]> {
    // Find each line's start and end by scanning for \r\n.
    let mut lines = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i + 1 < buf.len() {
        if buf[i] == b'\r' && buf[i + 1] == b'\n' {
            lines.push(&buf[start..i]);
            i += 2;
            start = i;
        } else {
            i += 1;
        }
    }
    lines
}

fn demo2_owned(buf: &[u8]) {
    let lines = split_lines(buf);
    println!("Demo 2a: owned request lines, {} lines", lines.len());
    time_it("parse_line_owned", || {
        let mut checksum = 0u64;
        for line in &lines {
            let req = parse_line_owned(line);
            checksum = checksum
                .wrapping_add(req.method.len() as u64)
                .wrapping_add(req.path.len() as u64)
                .wrapping_add(req.version.len() as u64);
        }
        checksum
    });
    println!();
}

fn demo2_borrowed(buf: &[u8]) {
    let lines = split_lines(buf);
    println!("Demo 2b: borrowed request lines, {} lines", lines.len());
    time_it("parse_line_borrowed", || {
        let mut checksum = 0u64;
        for line in &lines {
            let req = parse_line_borrowed(line);
            checksum = checksum
                .wrapping_add(req.method.len() as u64)
                .wrapping_add(req.path.len() as u64)
                .wrapping_add(req.version.len() as u64);
        }
        checksum
    });
    println!();
}

// ===========================================================================
// Demo 3: normalize — always owned vs Cow
//
// The transformation is "replace '_' with '-'". Half the inputs already
// have no underscores (borrowed case), half have one or more (owned case).
// ===========================================================================

fn normalize_owned(input: &str) -> String {
    if input.contains('_') {
        input.replace('_', "-")
    } else {
        input.to_string()
    }
}

fn normalize_cow(input: &str) -> Cow<'_, str> {
    if input.contains('_') {
        Cow::Owned(input.replace('_', "-"))
    } else {
        Cow::Borrowed(input)
    }
}

fn make_normalize_inputs(count: usize) -> Vec<String> {
    let mut rng = Rng::new(0xFACEFEED);
    let mut inputs = Vec::with_capacity(count);
    for i in 0..count {
        let len = 8 + (rng.next() % 16) as usize;
        let mut s = String::with_capacity(len);
        for _ in 0..len {
            let c = b'a' + (rng.next() % 26) as u8;
            s.push(c as char);
        }
        // Half the inputs have an underscore; half don't.
        if i % 2 == 0 {
            s.insert(s.len() / 2, '_');
        }
        inputs.push(s);
    }
    inputs
}

fn demo3_owned(inputs: &[String]) {
    println!("Demo 3a: always owned, {} inputs", inputs.len());
    time_it("normalize_owned", || {
        let mut checksum = 0u64;
        for input in inputs {
            let n = normalize_owned(input);
            checksum = checksum.wrapping_add(n.len() as u64);
        }
        checksum
    });
    println!();
}

fn demo3_cow(inputs: &[String]) {
    println!("Demo 3b: Cow<str>, {} inputs", inputs.len());
    time_it("normalize_cow", || {
        let mut checksum = 0u64;
        for input in inputs {
            let n = normalize_cow(input);
            checksum = checksum.wrapping_add(n.len() as u64);
        }
        checksum
    });
    println!();
}

// ===========================================================================
// main — the switchboard.
//
// Run with an argument to pick the demo:
//
//     cargo run --release -- 1a
//     cargo run --release -- 1b
//     ...
//
// Or with no argument, to see this list.
// ===========================================================================

fn main() {
    let which = std::env::args().nth(1).unwrap_or_default();

    match which.as_str() {
        "1a" => {
            let text = make_text(100 * 1024 * 1024); // 100 MiB
            demo1_owned(&text);
        }
        "1b" => {
            let text = make_text(100 * 1024 * 1024);
            demo1_borrowed(&text);
        }
        "2a" => {
            let buf = make_request_lines(1_000_000);
            demo2_owned(&buf);
        }
        "2b" => {
            let buf = make_request_lines(1_000_000);
            demo2_borrowed(&buf);
        }
        "3a" => {
            let inputs = make_normalize_inputs(1_000_000);
            demo3_owned(&inputs);
        }
        "3b" => {
            let inputs = make_normalize_inputs(1_000_000);
            demo3_cow(&inputs);
        }
        _ => {
            eprintln!(
                "usage: {} <1a|1b|2a|2b|3a|3b>",
                std::env::args().next().unwrap()
            );
            eprintln!();
            eprintln!("  Demo 1: word extraction, Vec<String> vs Vec<&str>");
            eprintln!("  Demo 2: HTTP request line, owned vs borrowed");
            eprintln!("  Demo 3: normalize, always owned vs Cow<str>");
        }
    }
}

// ---------------------------------------------------------------------------
// What to expect:
//
// Demo 1 (100 MiB of text, ~10M words):
//
//   get_words_string: ~10M allocations, one per word. Wall time is
//   dominated by the allocator and the memcpy of each word's bytes.
//   On a laptop this can take several hundred milliseconds.
//
//   get_words_slice: 1 allocation (the Vec itself). The words are
//   views into `text`. Wall time is dominated by the scan for word
//   boundaries, which is memory-bandwidth-bound.
//
//   Expect 3-10x wall-time improvement and a 10-million-fold reduction
//   in allocation count. The wall-time ratio depends on how fast the
//   allocator is; the allocation-count ratio is the more striking number.
//
// Demo 2 (1M request lines):
//
//   parse_line_owned: ~3M allocations (method + path + version per line).
//   parse_line_borrowed: 0 allocations (the input buffer is pre-split
//   into lines, and every field is a &str into it).
//
//   Wall-time difference is usually 5-10x in favor of borrowed.
//
// Demo 3 (1M inputs, half with underscores):
//
//   normalize_owned: ~1M allocations (one String per input, even when
//   no transformation is needed).
//
//   normalize_cow: ~500K allocations (only the inputs that need
//   transformation allocate; the other half borrow).
//
//   Wall-time difference is smaller than demos 1-2 because the work
//   per input is tiny, but the allocation count is halved, and the
//   wall time should be 1.5-2x better.
//
// ---------------------------------------------------------------------------
// Try changing:
//
// 1. Run each pair. Note the allocation counts first, then the wall
//    times. The counts are the cleaner signal, because they're not
//    affected by memory bandwidth variation between runs.
//
// 2. In demo 1, reduce the text size to 10 MiB and rerun. The
//    allocation count scales linearly; the wall-time ratio stays
//    similar. The Vec<String> version is 10x more allocations at every
//    size, but small sizes are dominated by fixed costs.
//
// 3. In demo 2, change the number of request lines to 100,000 and then
//    to 10,000,000. The allocation-count ratio stays the same; the
//    wall-time ratio widens with more lines.
//
// 4. In demo 3, change the proportion of inputs that need
//    transformation. If all inputs need it, Cow and owned perform
//    identically (both allocate every time). If none need it, Cow
//    allocates zero and owned allocates every time. The pattern is:
//    Cow wins when the transformation is rarely needed, loses (very
//    slightly) when it's always needed.
//
// 5. Add a demo that uses Bytes from the `bytes` crate to slice a
//    buffer across threads. That's the async-network-buffer pattern
//    from §6. It needs `bytes = "1"` in Cargo.toml. Skip if you don't
//    want the dependency.
// ---------------------------------------------------------------------------
