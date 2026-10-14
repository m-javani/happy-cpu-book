use crate::matrix::{EllMatrix, Matrix};
use wide::f32x4;

/// Classic CSR SpMV
#[inline(never)]
pub fn spmv_csr(a: &Matrix, x: &[f32], y: &mut [f32]) {
    let n = a.n as usize;
    assert_eq!(x.len(), n);
    assert_eq!(y.len(), n);

    for i in 0..n {
        let start = a.row_ptr[i] as usize;
        let end = a.row_ptr[i + 1] as usize;
        let mut sum = 0.0f32;

        // SAFETY: CSR invariants guarantee the ranges and indices are valid
        unsafe {
            for k in start..end {
                let j = *a.col_idx.get_unchecked(k) as usize;
                sum += *a.values.get_unchecked(k) * *x.get_unchecked(j);
            }
        }
        y[i] = sum;
    }
}

/// ELLPACK SpMV (scalar)
#[inline(never)]
pub fn spmv_ell(a: &EllMatrix, x: &[f32], y: &mut [f32]) {
    let n = a.n as usize;
    let width = a.width as usize;
    assert_eq!(x.len(), n);
    assert_eq!(y.len(), n);

    for i in 0..n {
        let row_base = i * width;
        let mut sum = 0.0f32;

        unsafe {
            for k in 0..width {
                let j = *a.col_idx.get_unchecked(row_base + k) as usize;
                sum += *a.values.get_unchecked(row_base + k) * *x.get_unchecked(j);
            }
        }
        y[i] = sum;
    }
}

/// ELLPACK SpMV with 4-wide SIMD (via `wide`)
#[inline(never)]
pub fn spmv_ell_simd(a: &EllMatrix, x: &[f32], y: &mut [f32]) {
    let n = a.n as usize;
    let width = a.width as usize;
    assert_eq!(x.len(), n);
    assert_eq!(y.len(), n);

    let width_simd = width / 4 * 4;

    for i in 0..n {
        let row_base = i * width;
        let mut sum = f32x4::ZERO;

        let mut k = 0;
        while k < width_simd {
            unsafe {
                let j0 = *a.col_idx.get_unchecked(row_base + k) as usize;
                let j1 = *a.col_idx.get_unchecked(row_base + k + 1) as usize;
                let j2 = *a.col_idx.get_unchecked(row_base + k + 2) as usize;
                let j3 = *a.col_idx.get_unchecked(row_base + k + 3) as usize;

                let x_vec = f32x4::new([
                    *x.get_unchecked(j0),
                    *x.get_unchecked(j1),
                    *x.get_unchecked(j2),
                    *x.get_unchecked(j3),
                ]);

                let v_vec = f32x4::new([
                    *a.values.get_unchecked(row_base + k),
                    *a.values.get_unchecked(row_base + k + 1),
                    *a.values.get_unchecked(row_base + k + 2),
                    *a.values.get_unchecked(row_base + k + 3),
                ]);

                sum = v_vec.mul_add(x_vec, sum);
            }
            k += 4;
        }

        let arr = sum.to_array();
        let mut total = arr[0] + arr[1] + arr[2] + arr[3];

        for k in width_simd..width {
            unsafe {
                let j = *a.col_idx.get_unchecked(row_base + k) as usize;
                total += *a.values.get_unchecked(row_base + k) * *x.get_unchecked(j);
            }
        }
        y[i] = total;
    }
}
