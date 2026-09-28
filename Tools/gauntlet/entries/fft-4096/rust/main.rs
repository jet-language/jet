const N: usize = 4096;
const SCALAR_COUNT: usize = N * 2;
// Every real and imaginary scalar must be within this absolute tolerance.
const TOLERANCE: f64 = 1.0e-9;

#[derive(Clone, Copy)]
struct Complex {
    re: f64,
    im: f64,
}

#[inline]
fn multiply(a: Complex, b: Complex) -> Complex {
    Complex {
        re: a.re * b.re - a.im * b.im,
        im: a.re * b.im + a.im * b.re,
    }
}

fn fft(input: &[f64]) -> Vec<Complex> {
    let n = input.len();
    let mut spectrum = Vec::with_capacity(n);
    for &value in input {
        spectrum.push(Complex { re: value, im: 0.0 });
    }

    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            spectrum.swap(i, j);
        }
    }

    let mut width = 2usize;
    while width <= n {
        let half = width >> 1;
        let angle = -2.0 * std::f64::consts::PI / width as f64;
        let wlen = Complex { re: angle.cos(), im: angle.sin() };
        let mut start = 0usize;
        while start < n {
            let mut w = Complex { re: 1.0, im: 0.0 };
            for offset in 0..half {
                let left = start + offset;
                let right = left + half;
                let u = spectrum[left];
                let v = multiply(spectrum[right], w);
                spectrum[left] = Complex { re: u.re + v.re, im: u.im + v.im };
                spectrum[right] = Complex { re: u.re - v.re, im: u.im - v.im };
                w = multiply(w, wlen);
            }
            start += width;
        }
        width <<= 1;
    }
    spectrum
}

fn main() {
    let mut input = vec![0.0f64; N];
    input[0] = 1.0;

    let spectrum = fft(&input);
    let mut output = vec![0.0f64; SCALAR_COUNT];
    for i in 0..N {
        output[2 * i] = spectrum[i].re;
        output[2 * i + 1] = spectrum[i].im;
    }

    let mut failures = 0usize;
    if output.len() != SCALAR_COUNT {
        failures += 1;
    } else {
        for i in 0..N {
            if !((output[2 * i] - 1.0).abs() <= TOLERANCE) {
                failures += 1;
            }
            if !(output[2 * i + 1].abs() <= TOLERANCE) {
                failures += 1;
            }
        }
    }
    if failures != 0 {
        eprintln!("fft validation failed: {failures}");
        std::process::exit(1);
    }
    println!("fft4096 checked=8192 tolerance=1e-9");
}
