use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Result, Write};

/// Compressed Sparse Row
pub struct Matrix {
    pub n: u32,
    pub row_ptr: Vec<u32>,
    pub col_idx: Vec<u32>,
    pub values: Vec<f32>,
}

/// ELLPACK (fixed-width rows, zero-padded)
pub struct EllMatrix {
    pub n: u32,
    pub width: u32,
    pub col_idx: Vec<u32>,
    pub values: Vec<f32>,
}

impl Matrix {
    pub fn nnz(&self) -> u32 {
        self.col_idx.len() as u32
    }

    pub fn save(&self, path: &str) -> Result<()> {
        let mut out = BufWriter::new(File::create(path)?);
        out.write_all(&self.n.to_le_bytes())?;
        out.write_all(&self.nnz().to_le_bytes())?;
        for &v in &self.row_ptr {
            out.write_all(&v.to_le_bytes())?;
        }
        for &v in &self.col_idx {
            out.write_all(&v.to_le_bytes())?;
        }
        for &v in &self.values {
            out.write_all(&v.to_le_bytes())?;
        }
        out.flush()
    }

    pub fn to_ell(&self) -> EllMatrix {
        let n = self.n as usize;
        let mut max_len = 0;
        for i in 0..n {
            let len = (self.row_ptr[i + 1] - self.row_ptr[i]) as usize;
            if len > max_len {
                max_len = len;
            }
        }

        let mut col_idx = vec![0u32; n * max_len];
        let mut values = vec![0.0f32; n * max_len];

        for i in 0..n {
            let start = self.row_ptr[i] as usize;
            let end = self.row_ptr[i + 1] as usize;
            for (k, src) in (start..end).enumerate() {
                col_idx[i * max_len + k] = self.col_idx[src];
                values[i * max_len + k] = self.values[src];
            }
        }

        EllMatrix {
            n: self.n,
            width: max_len as u32,
            col_idx,
            values,
        }
    }
}

pub fn load_matrix(path: &str) -> Result<Matrix> {
    let mut r = BufReader::new(File::open(path)?);
    let mut buf = [0u8; 4];

    r.read_exact(&mut buf)?;
    let n = u32::from_le_bytes(buf);
    r.read_exact(&mut buf)?;
    let nnz = u32::from_le_bytes(buf);

    let mut row_ptr = vec![0u32; n as usize + 1];
    read_u32_slice(&mut r, &mut row_ptr)?;
    let mut col_idx = vec![0u32; nnz as usize];
    read_u32_slice(&mut r, &mut col_idx)?;
    let mut values = vec![0.0f32; nnz as usize];
    read_f32_slice(&mut r, &mut values)?;

    Ok(Matrix {
        n,
        row_ptr,
        col_idx,
        values,
    })
}

fn read_u32_slice<R: Read>(r: &mut R, out: &mut [u32]) -> Result<()> {
    let mut bytes = vec![0u8; out.len() * 4];
    r.read_exact(&mut bytes)?;
    for (dst, chunk) in out.iter_mut().zip(bytes.chunks_exact(4)) {
        *dst = u32::from_le_bytes(chunk.try_into().unwrap());
    }
    Ok(())
}

fn read_f32_slice<R: Read>(r: &mut R, out: &mut [f32]) -> Result<()> {
    let mut bytes = vec![0u8; out.len() * 4];
    r.read_exact(&mut bytes)?;
    for (dst, chunk) in out.iter_mut().zip(bytes.chunks_exact(4)) {
        *dst = f32::from_le_bytes(chunk.try_into().unwrap());
    }
    Ok(())
}

pub fn generate_matrix(n: u32, nnz_target: u32) -> Matrix {
    let mut row_ptr = Vec::with_capacity(n as usize + 1);
    let mut col_idx = Vec::with_capacity(nnz_target as usize);
    let mut values = Vec::with_capacity(nnz_target as usize);

    row_ptr.push(0);
    let mut current = 0u32;

    for i in 0..n {
        let base = (nnz_target / n) as i32;
        let extra = if i < (nnz_target % n) { 1 } else { 0 };
        let row_nnz = (base + extra).max(1) as u32;

        for k in 0..row_nnz {
            let h = splitmix64((i as u64) << 32 | k as u64);
            col_idx.push((h as u32) % n);
            values.push(0.5 + ((h >> 40) as f32 / u32::MAX as f32));
            current += 1;
        }
        row_ptr.push(current);
    }

    Matrix {
        n,
        row_ptr,
        col_idx,
        values,
    }
}

fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e3779b97f4a7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}
