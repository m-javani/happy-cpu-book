use std::env;
use std::time::Instant;

mod matrix;
mod spmv;

use matrix::load_matrix;
use spmv::{spmv_csr, spmv_ell, spmv_ell_simd};

#[derive(Clone, Copy)]
enum Kernel {
    Csr,
    Ell,
    EllSimd,
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let result: std::io::Result<()> = match args.get(1).map(String::as_str) {
        Some("generate") => {
            let path = args.get(2).expect("usage: spmv generate <path>");
            generate(path)
        }
        Some("run-csr") => {
            let path = args.get(2).expect("usage: spmv run-csr <path>");
            run_one(path, Kernel::Csr)
        }
        Some("run-ell") => {
            let path = args.get(2).expect("usage: spmv run-ell <path>");
            run_one(path, Kernel::Ell)
        }
        Some("run-ell-simd") => {
            let path = args.get(2).expect("usage: spmv run-ell-simd <path>");
            run_one(path, Kernel::EllSimd)
        }
        Some("run-all") => {
            let path = args.get(2).expect("usage: spmv run-all <path>");
            run_all(path)
        }
        _ => {
            eprintln!("usage:");
            eprintln!("  spmv generate <path>");
            eprintln!("  spmv run-csr <path>");
            eprintln!("  spmv run-ell <path>");
            eprintln!("  spmv run-ell-simd <path>");
            eprintln!("  spmv run-all <path>");
            std::process::exit(1);
        }
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn generate(path: &str) -> std::io::Result<()> {
    const N: u32 = 20_000;
    const NNZ_TARGET: u32 = 2_000_000;

    let matrix = matrix::generate_matrix(N, NNZ_TARGET);
    matrix.save(path)?;
    println!(
        "wrote {}×{} matrix with {} non-zeros to {}",
        matrix.n,
        matrix.n,
        matrix.nnz(),
        path
    );
    Ok(())
}

fn run_one(path: &str, kernel: Kernel) -> std::io::Result<()> {
    let matrix = load_matrix(path)?;
    let n = matrix.n as usize;

    let x = vec![1.0f32; n];
    let mut y = vec![0.0f32; n];

    // Prepare both representations once, outside the timed region.
    let ell = matrix.to_ell();

    let name = match kernel {
        Kernel::Csr => "CSR",
        Kernel::Ell => "ELL",
        Kernel::EllSimd => "ELL SIMD",
    };

    // Warm-up
    for _ in 0..5 {
        run_kernel(kernel, &matrix, &ell, &x, &mut y);
    }

    // Timed
    let iters = 100;
    let start = Instant::now();
    for _ in 0..iters {
        run_kernel(kernel, &matrix, &ell, &x, &mut y);
    }
    let elapsed = start.elapsed();

    println!("{} checksum: {:.3}", name, y.iter().sum::<f32>());
    println!("{} avg:      {:?}", name, elapsed / iters);
    Ok(())
}

fn run_all(path: &str) -> std::io::Result<()> {
    let matrix = load_matrix(path)?;
    let n = matrix.n as usize;
    let ell = matrix.to_ell();

    println!("matrix: {}×{}, nnz = {}", matrix.n, matrix.n, matrix.nnz());
    println!("ELL width (max row length): {}", ell.width);

    let x = vec![1.0f32; n];
    let mut y = vec![0.0f32; n];
    let iters = 100;

    // CSR
    for _ in 0..5 {
        spmv_csr(&matrix, &x, &mut y);
    }
    let t = Instant::now();
    for _ in 0..iters {
        spmv_csr(&matrix, &x, &mut y);
    }
    println!("CSR       checksum: {:.3}", y.iter().sum::<f32>());
    println!("CSR       avg:      {:?}", t.elapsed() / iters);

    // ELL scalar
    for _ in 0..5 {
        spmv_ell(&ell, &x, &mut y);
    }
    let t = Instant::now();
    for _ in 0..iters {
        spmv_ell(&ell, &x, &mut y);
    }
    println!("ELL       checksum: {:.3}", y.iter().sum::<f32>());
    println!("ELL       avg:      {:?}", t.elapsed() / iters);

    // ELL + SIMD
    for _ in 0..5 {
        spmv_ell_simd(&ell, &x, &mut y);
    }
    let t = Instant::now();
    for _ in 0..iters {
        spmv_ell_simd(&ell, &x, &mut y);
    }
    println!("ELL SIMD  checksum: {:.3}", y.iter().sum::<f32>());
    println!("ELL SIMD  avg:      {:?}", t.elapsed() / iters);

    Ok(())
}

fn run_kernel(
    kernel: Kernel,
    matrix: &matrix::Matrix,
    ell: &matrix::EllMatrix,
    x: &[f32],
    y: &mut [f32],
) {
    match kernel {
        Kernel::Csr => spmv_csr(matrix, x, y),
        Kernel::Ell => spmv_ell(ell, x, y),
        Kernel::EllSimd => spmv_ell_simd(ell, x, y),
    }
}
