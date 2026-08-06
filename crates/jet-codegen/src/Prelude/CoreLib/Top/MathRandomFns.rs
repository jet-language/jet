// Math and random helpers for `core.math` and `core.random`.
//
// These used to live in Top/Process.rs, which only emits for `core.process`
// or the filesystem runtime. A math-only program therefore generated calls to
// symbols that were never included, which rustc rejected as an I2 violation.
// They are gated on `needs_math` here, beside the rest of the math surface.

fn jet_std_math_sqrt(x: f64) -> f64 {
    x.sqrt()
}
fn jet_std_math_pow(a: f64, b: f64) -> f64 {
    a.powf(b)
}
fn jet_std_math_floor(x: f64) -> f64 {
    x.floor()
}
fn jet_std_math_ceil(x: f64) -> f64 {
    x.ceil()
}
fn jet_std_math_round(x: f64) -> i64 {
    x.round() as i64
}
fn jet_std_math_sign(x: f64) -> i64 {
    if x > 0.0 {
        1
    } else if x < 0.0 {
        -1
    } else {
        0
    }
}
fn jet_std_math_checked_pow(base: i64, exp: i64) -> Option<i64> {
    if exp < 0 {
        return None;
    }
    base.checked_pow(exp as u32)
}
fn jet_std_math_int_pow(base: i64, exp: i64) -> i64 {
    if exp < 0 {
        return 0;
    }
    base.saturating_pow(exp as u32)
}
/// The largest whole number whose square is at most `value`, or absent when
/// there is none. A negative number has no whole square root.
fn jet_std_math_isqrt(value: i64) -> Option<i64> {
    if value < 0 {
        return None;
    }
    let mut root = (value as f64).sqrt() as i64;
    // The float square root can land either side on large values, so walk back
    // to the exact answer.
    while root > 0 && root.saturating_mul(root) > value {
        root -= 1;
    }
    while (root + 1).saturating_mul(root + 1) <= value {
        root += 1;
    }
    Some(root)
}

/// The product of every whole number from 1 to `value`, or absent when there is
/// no answer: a negative input, or a result past the range. 21 factorial is
/// already too big.
fn jet_std_math_factorial(value: i64) -> Option<i64> {
    if value < 0 {
        return None;
    }
    let mut total: i64 = 1;
    let mut step: i64 = 2;
    while step <= value {
        total = total.checked_mul(step)?;
        step += 1;
    }
    Some(total)
}

fn jet_std_math_gcd(mut a: i64, mut b: i64) -> i64 {
    a = a.abs();
    b = b.abs();
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a
}
fn jet_std_math_lcm(a: i64, b: i64) -> i64 {
    if a == 0 || b == 0 {
        0
    } else {
        (a / jet_std_math_gcd(a, b)).saturating_mul(b).abs()
    }
}

/// Ways to choose `k` items from `n`, or absent when the product leaves the
/// range or either side is negative.
fn jet_std_math_binomial(n: i64, k: i64) -> Option<i64> {
    if n < 0 || k < 0 || k > n {
        return None;
    }
    let k = k.min(n - k);
    let mut out: i64 = 1;
    let mut i: i64 = 1;
    while i <= k {
        out = out.checked_mul(n - k + i)?.checked_div(i)?;
        i += 1;
    }
    Some(out)
}

fn jet_std_math_digits(value: i64) -> i64 {
    if value == 0 {
        return 1;
    }
    let mut n = value.unsigned_abs();
    let mut count = 0i64;
    while n > 0 {
        count += 1;
        n /= 10;
    }
    count
}

fn jet_std_math_leading_ones(value: i64) -> i64 {
    (value as u64).leading_ones() as i64
}

fn jet_std_math_trailing_ones(value: i64) -> i64 {
    (value as u64).trailing_ones() as i64
}

fn jet_std_math_cmp(a: f64, b: f64) -> i64 {
    match a.partial_cmp(&b) {
        Some(std::cmp::Ordering::Less) => -1,
        Some(std::cmp::Ordering::Equal) => 0,
        Some(std::cmp::Ordering::Greater) => 1,
        None => {
            // NaN sorts after every finite value, matching a stable total order
            // for audit output: NaN vs NaN is equal; NaN vs number is greater.
            if a.is_nan() && b.is_nan() {
                0
            } else if a.is_nan() {
                1
            } else {
                -1
            }
        }
    }
}

fn jet_std_math_ulp(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x.is_infinite() {
        return f64::INFINITY;
    }
    let x = x.abs();
    if x == 0.0 {
        return f64::from_bits(1);
    }
    let bits = x.to_bits();
    let next = f64::from_bits(bits + 1);
    next - x
}

fn jet_std_math_significand(x: f64) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return x;
    }
    let bits = x.to_bits();
    let exp = ((bits >> 52) & 0x7ff) as i32;
    if exp == 0 {
        // Subnormal: scale into the normal significand range.
        let mut v = x.abs();
        while v < 1.0 {
            v *= 2.0;
        }
        return if x.is_sign_negative() { -v } else { v };
    }
    let frac_bits = bits & ((1u64 << 52) - 1);
    let sig = f64::from_bits((0x3ffu64 << 52) | frac_bits);
    if x.is_sign_negative() {
        -sig
    } else {
        sig
    }
}

fn jet_std_math_ilogb(x: f64) -> Option<i64> {
    if x == 0.0 || !x.is_finite() {
        return None;
    }
    Some(x.abs().log2().floor() as i64)
}

fn jet_std_math_logb(x: f64) -> f64 {
    match jet_std_math_ilogb(x) {
        Some(e) => e as f64,
        None if x == 0.0 => f64::NEG_INFINITY,
        None => f64::INFINITY,
    }
}

fn jet_std_math_ldexp(x: f64, exp: i64) -> f64 {
    if !x.is_finite() || x == 0.0 || exp == 0 {
        return x;
    }
    x * 2f64.powi(exp.clamp(-2099, 2099) as i32)
}

fn jet_std_math_next_after(x: f64, toward: f64) -> f64 {
    if x.is_nan() || toward.is_nan() {
        return f64::NAN;
    }
    if x == toward {
        return toward;
    }
    if x == 0.0 {
        return if toward > 0.0 {
            f64::from_bits(1)
        } else {
            -f64::from_bits(1)
        };
    }
    let bits = x.to_bits();
    let next = if (toward > x) == x.is_sign_positive() {
        bits + 1
    } else {
        bits - 1
    };
    f64::from_bits(next)
}

/// Abramowitz & Stegun 7.1.26 — max error under 1.5e-7 on the real line.
fn jet_std_math_erf(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x.is_infinite() {
        return x.signum();
    }
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let ax = x.abs();
    let t = 1.0 / (1.0 + 0.3275911 * ax);
    let poly = t
        * (0.254829592
            + t * (-0.284496736
                + t * (1.421413741 + t * (-1.453152027 + t * 1.061405429))));
    sign * (1.0 - poly * (-ax * ax).exp())
}

fn jet_std_math_erfc(x: f64) -> f64 {
    1.0 - jet_std_math_erf(x)
}

/// Lanczos approximation for Γ(x) on positive reals; reflected for (0,1).
fn jet_std_math_gamma(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x <= 0.0 {
        if x == x.floor() {
            return f64::NAN;
        }
        // Reflection: Γ(z)Γ(1−z) = π / sin(πz)
        return std::f64::consts::PI / ((std::f64::consts::PI * x).sin() * jet_std_math_gamma(1.0 - x));
    }
    // Lanczos g=7, n=9
    const G: f64 = 7.0;
    const C: [f64; 9] = [
        0.99999999999980993,
        676.5203681218851,
        -1259.1392167224028,
        771.32342877765313,
        -176.61502916214059,
        12.507343278686905,
        -0.13857109526572012,
        9.984369654078991e-6,
        1.5056327351493116e-7,
    ];
    let mut z = x;
    if z < 0.5 {
        return std::f64::consts::PI / ((std::f64::consts::PI * z).sin() * jet_std_math_gamma(1.0 - z));
    }
    z -= 1.0;
    let mut xacc = C[0];
    for i in 1..9 {
        xacc += C[i] / (z + i as f64);
    }
    let t = z + G + 0.5;
    (2.0 * std::f64::consts::PI).sqrt() * t.powf(z + 0.5) * (-t).exp() * xacc
}

fn jet_std_math_lgamma(x: f64) -> f64 {
    let g = jet_std_math_gamma(x);
    if g.is_nan() || g <= 0.0 {
        f64::NAN
    } else {
        g.ln()
    }
}

// D-FLOATW1 (ratified 2026-06-22): F32 variants — sqrt(F32)->F32, pow(F32,F32)->F32 etc.
// F32 is a real precision choice, not just storage; no silent widening to f64 (I3).
fn jet_std_math_sqrt_f32(x: f32) -> f32 {
    x.sqrt()
}
fn jet_std_math_pow_f32(a: f32, b: f32) -> f32 {
    a.powf(b)
}
fn jet_std_math_floor_f32(x: f32) -> f32 {
    x.floor()
}
fn jet_std_math_ceil_f32(x: f32) -> f32 {
    x.ceil()
}

thread_local! { static JET_RNG: std::cell::Cell<u64> = std::cell::Cell::new(0x4d595df4d0f33173); }
fn jet_rng_next() -> u64 {
    JET_RNG.with(|cell| {
        let mut x = cell.get();
        x ^= x << 7;
        x ^= x >> 9;
        x = x.wrapping_mul(0x9e3779b97f4a7c15);
        cell.set(x);
        x
    })
}
fn jet_std_random_seed(n: i64) {
    JET_RNG.with(|cell| cell.set(n as u64));
}
fn jet_std_random_int(low: i64, high: i64) -> i64 {
    if high <= low {
        return low;
    }
    low + (jet_rng_next() % ((high - low + 1) as u64)) as i64
}
fn jet_std_random_float() -> f64 {
    (jet_rng_next() as f64) / (u64::MAX as f64)
}
fn jet_std_random_float_open() -> f64 {
    let x = jet_std_random_float();
    if x <= 0.0 { f64::MIN_POSITIVE } else { x }
}
fn jet_std_random_float_range(low: f64, high: f64) -> f64 {
    if !(high > low) {
        return low;
    }
    low + (high - low) * jet_std_random_float()
}
fn jet_std_random_bool(p: f64) -> bool {
    if p <= 0.0 || p.is_nan() {
        false
    } else if p >= 1.0 {
        true
    } else {
        jet_std_random_float() < p
    }
}
fn jet_std_random_normal(mean: f64, stddev: f64) -> f64 {
    let u1 = jet_std_random_float_open();
    let u2 = jet_std_random_float();
    let z0 = (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos();
    mean + z0 * stddev.max(0.0)
}
fn jet_std_random_exponential(lambda: f64) -> f64 {
    if lambda <= 0.0 || lambda.is_nan() {
        return 0.0;
    }
    -jet_std_random_float_open().ln() / lambda
}
fn jet_std_random_pick<T: Clone>(xs: &Vec<T>) -> Option<T> {
    if xs.is_empty() {
        None
    } else {
        Some(xs[jet_std_random_int(0, xs.len() as i64 - 1) as usize].clone())
    }
}
fn jet_std_random_weighted_pick<T: Clone>(xs: &Vec<T>, weights: &Vec<f64>) -> Option<T> {
    if xs.is_empty() || xs.len() != weights.len() {
        return None;
    }
    let mut total = 0.0;
    for &w in weights {
        if w.is_finite() && w > 0.0 {
            total += w;
        }
    }
    if total <= 0.0 {
        return None;
    }
    let mut needle = jet_std_random_float_range(0.0, total);
    for (item, &weight) in xs.iter().zip(weights.iter()) {
        let w = if weight.is_finite() && weight > 0.0 { weight } else { 0.0 };
        if needle < w {
            return Some(item.clone());
        }
        needle -= w;
    }
    xs.last().cloned()
}
fn jet_std_random_sample<T: Clone>(xs: &Vec<T>, k: i64) -> Vec<T> {
    let want = (k.max(0) as usize).min(xs.len());
    let mut pool = xs.clone();
    for i in 0..want {
        let j = jet_std_random_int(i as i64, pool.len() as i64 - 1) as usize;
        pool.swap(i, j);
    }
    pool.truncate(want);
    pool
}
fn jet_std_random_shuffle<T>(xs: &mut Vec<T>) {
    let len = xs.len();
    for i in (1..len).rev() {
        let j = jet_std_random_int(0, i as i64) as usize;
        xs.swap(i, j);
    }
}
// D-RANDSPLIT1=A: PRNG bytes via the ambient SplitMix64 state — fast, seedable,
// NOT cryptographically secure. Use for simulation, testing, or shuffles only.
fn jet_std_random_bytes(n: i64) -> Vec<u8> {
    let n = n.max(0) as usize;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(jet_rng_next() as u8);
    }
    out
}
fn jet_std_random_split(seed: i64) -> jet_std::Rng {
    let mixed = (seed as u64) ^ jet_rng_next().rotate_left(17);
    jet_std::Rng { state: mixed }
}
