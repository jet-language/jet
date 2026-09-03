use std::env;
use std::time::Instant;

fn fft(n: usize) {
    let mut re = vec![0.0f64; n];
    let mut im = vec![0.0f64; n];
    re[0] = 1.0;
    let start = Instant::now();
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n / 2;
        while bit > 0 {
            if j >= bit {
                j -= bit;
                bit /= 2;
            } else {
                break;
            }
        }
        j += bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut length = 2usize;
    let mut half_f = 1.0f64;
    while length <= n {
        let half = length / 2;
        let angle = -std::f64::consts::PI / half_f;
        let step_re = angle.cos();
        let step_im = angle.sin();
        let mut w_re = 1.0f64;
        let mut w_im = 0.0f64;
        for offset in 0..half {
            let mut i = offset;
            while i < n {
                let k = i + half;
                let odd_re = re[k];
                let odd_im = im[k];
                let t_re = w_re * odd_re - w_im * odd_im;
                let t_im = w_re * odd_im + w_im * odd_re;
                let even_re = re[i];
                let even_im = im[i];
                re[i] = even_re + t_re;
                im[i] = even_im + t_im;
                re[k] = even_re - t_re;
                im[k] = even_im - t_im;
                i += length;
            }
            let next_re = w_re * step_re - w_im * step_im;
            let next_im = w_re * step_im + w_im * step_re;
            w_re = next_re;
            w_im = next_im;
        }
        length *= 2;
        half_f *= 2.0;
    }
    let checksum: f64 = re.iter().sum::<f64>() + im.iter().map(|x| x.abs()).sum::<f64>();
    println!("fft_n:{} ns:{} checksum:{} first:{} last:{}", n, start.elapsed().as_nanos(), checksum, re[0], re[n - 1]);
}

fn matmul(n: usize) {
    let count = n * n;
    let a = vec![1.0f64; count];
    let b = vec![2.0f64; count];
    let start = Instant::now();
    let mut c = Vec::with_capacity(count);
    for i in 0..n {
        let base_a = i * n;
        for j in 0..n {
            let mut acc = 0.0f64;
            let mut idx_a = base_a;
            let mut idx_b = j;
            for _ in 0..n {
                acc += a[idx_a] * b[idx_b];
                idx_a += 1;
                idx_b += n;
            }
            c.push(acc);
        }
    }
    let checksum: f64 = c.iter().sum();
    println!("matmul_n:{} ns:{} checksum:{} first:{} last:{}", n, start.elapsed().as_nanos(), checksum, c[0], c[count - 1]);
}

fn sparse(rows: usize, entries_per_row: usize) {
    let nnz = rows * entries_per_row;
    let mut data = Vec::with_capacity(nnz);
    let mut indices = Vec::with_capacity(nnz);
    let mut indptr = Vec::with_capacity(rows + 1);
    let x = vec![1.0f64; 1000];
    indptr.push(0);
    for _ in 0..rows {
        for k in 0..entries_per_row {
            data.push(1.0);
            indices.push(k);
        }
        indptr.push(data.len());
    }
    let start = Instant::now();
    let mut y = Vec::with_capacity(rows);
    for row in 0..rows {
        let mut acc = 0.0f64;
        for p in indptr[row]..indptr[row + 1] {
            acc += data[p] * x[indices[p]];
        }
        y.push(acc);
    }
    let checksum: f64 = y.iter().sum();
    println!("csr_rows:{} cols:1000 nnz:{} ns:{} checksum:{} first:{} last:{}", rows, nnz, start.elapsed().as_nanos(), checksum, y[0], y[rows - 1]);
}

fn stencil(n: usize) {
    let count = n * n;
    let grid = vec![1.0f64; count];
    let mut output = vec![0.0f64; count];
    let start = Instant::now();
    for row in 1..n - 1 {
        for col in 1..n - 1 {
            let idx = row * n + col;
            output[idx] = (grid[idx] + grid[idx - n] + grid[idx + n] + grid[idx - 1] + grid[idx + 1]) / 5.0;
        }
    }
    let checksum: f64 = output.iter().sum();
    println!("stencil_n:{} ns:{} checksum:{} first:{} center:{} last:{}", n, start.elapsed().as_nanos(), checksum, output[0], output[(n / 2) * n + n / 2], output[count - 1]);
}

fn main() {
    match env::args().nth(1).as_deref() {
        Some("fft4096") => fft(4096),
        Some("fft65536") => fft(65536),
        Some("matmul") => matmul(512),
        Some("sparse") => sparse(100_000, 10),
        Some("stencil") => stencil(1024),
        _ => {
            fft(4096);
            fft(65536);
            matmul(512);
            sparse(100_000, 10);
            stencil(1024);
        }
    }
}
