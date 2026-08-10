// Shared core.math helpers (I9). Included by AOT prelude, JIT math_rt, and comptime ambient.
// Keep std-only; no jet_std / host types.

/// Numeric Core bounds shared by AOT, JIT, and comptime. Engines only marshal
/// the typed call to these Prelude functions.
pub fn jet_std_math_abs_i64(value: i64) -> i64 {
    value.abs()
}

pub fn jet_std_math_min_i64(left: i64, right: i64) -> i64 {
    if left <= right { left } else { right }
}

pub fn jet_std_math_max_i64(left: i64, right: i64) -> i64 {
    if left >= right { left } else { right }
}

pub fn jet_std_math_clamp_i64(value: i64, low: i64, high: i64) -> i64 {
    jet_std_math_min_i64(jet_std_math_max_i64(value, low), high)
}

fn jet_std_math_intn_parts(value: i64, signed: i64, bits: i64) -> (i64, u64) {
    let width = bits.clamp(1, 64) as u32;
    let mask = if width == 64 {
        u64::MAX
    } else {
        (1u64 << width) - 1
    };
    let raw = value as u64 & mask;
    let decoded = if signed != 0 && width < 64 && raw & (1u64 << (width - 1)) != 0 {
        (raw | !mask) as i64
    } else {
        raw as i64
    };
    (decoded, raw)
}

pub fn jet_std_math_abs_intn(value: i64, signed: i64, bits: i64) -> i64 {
    let (decoded, raw) = jet_std_math_intn_parts(value, signed, bits);
    if signed == 0 { raw as i64 } else { decoded.wrapping_abs() }
}

pub fn jet_std_math_min_intn(left: i64, right: i64, signed: i64, bits: i64) -> i64 {
    let (left_signed, left_raw) = jet_std_math_intn_parts(left, signed, bits);
    let (right_signed, right_raw) = jet_std_math_intn_parts(right, signed, bits);
    if signed == 0 {
        if left_raw <= right_raw { left_raw as i64 } else { right_raw as i64 }
    } else if left_signed <= right_signed {
        left_signed
    } else {
        right_signed
    }
}

pub fn jet_std_math_max_intn(left: i64, right: i64, signed: i64, bits: i64) -> i64 {
    let (left_signed, left_raw) = jet_std_math_intn_parts(left, signed, bits);
    let (right_signed, right_raw) = jet_std_math_intn_parts(right, signed, bits);
    if signed == 0 {
        if left_raw >= right_raw { left_raw as i64 } else { right_raw as i64 }
    } else if left_signed >= right_signed {
        left_signed
    } else {
        right_signed
    }
}

pub fn jet_std_math_clamp_intn(
    value: i64,
    low: i64,
    high: i64,
    signed: i64,
    bits: i64,
) -> i64 {
    jet_std_math_min_intn(
        jet_std_math_max_intn(value, low, signed, bits),
        high,
        signed,
        bits,
    )
}

pub fn jet_std_math_abs_f32(value: f32) -> f32 {
    value.abs()
}

pub fn jet_std_math_min_f32(left: f32, right: f32) -> f32 {
    left.min(right)
}

pub fn jet_std_math_max_f32(left: f32, right: f32) -> f32 {
    left.max(right)
}

pub fn jet_std_math_clamp_f32(value: f32, low: f32, high: f32) -> f32 {
    value.clamp(low, high)
}

pub fn jet_std_math_abs_f64(value: f64) -> f64 {
    value.abs()
}

pub fn jet_std_math_min_f64(left: f64, right: f64) -> f64 {
    left.min(right)
}

pub fn jet_std_math_max_f64(left: f64, right: f64) -> f64 {
    left.max(right)
}

pub fn jet_std_math_clamp_f64(value: f64, low: f64, high: f64) -> f64 {
    value.clamp(low, high)
}

/// The largest whole number whose square is at most `value`, or absent when
/// there is none. A negative number has no whole square root.
pub fn jet_std_math_isqrt(value: i64) -> Option<i64> {
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
pub fn jet_std_math_factorial(value: i64) -> Option<i64> {
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

pub fn jet_std_math_gcd(mut a: i64, mut b: i64) -> i64 {
    a = a.abs();
    b = b.abs();
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a
}
pub fn jet_std_math_lcm(a: i64, b: i64) -> i64 {
    if a == 0 || b == 0 {
        0
    } else {
        (a / jet_std_math_gcd(a, b)).saturating_mul(b).abs()
    }
}

/// Ways to choose `k` items from `n`, or absent when the product leaves the
/// range or either side is negative.
pub fn jet_std_math_binomial(n: i64, k: i64) -> Option<i64> {
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

pub fn jet_std_math_digits(value: i64) -> i64 {
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

pub fn jet_std_math_leading_ones(value: i64) -> i64 {
    (value as u64).leading_ones() as i64
}

pub fn jet_std_math_trailing_ones(value: i64) -> i64 {
    (value as u64).trailing_ones() as i64
}

pub fn jet_std_math_cmp(a: f64, b: f64) -> i64 {
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

pub fn jet_std_math_ulp(x: f64) -> f64 {
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

pub fn jet_std_math_significand(x: f64) -> f64 {
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

pub fn jet_std_math_ilogb(x: f64) -> Option<i64> {
    if x == 0.0 || !x.is_finite() {
        return None;
    }
    Some(x.abs().log2().floor() as i64)
}

pub fn jet_std_math_logb(x: f64) -> f64 {
    match jet_std_math_ilogb(x) {
        Some(e) => e as f64,
        None if x == 0.0 => f64::NEG_INFINITY,
        None => f64::INFINITY,
    }
}

pub fn jet_std_math_ldexp(x: f64, exp: i64) -> f64 {
    if !x.is_finite() || x == 0.0 || exp == 0 {
        return x;
    }
    x * 2f64.powi(exp.clamp(-2099, 2099) as i32)
}

pub fn jet_std_math_next_after(x: f64, toward: f64) -> f64 {
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
pub fn jet_std_math_erf(x: f64) -> f64 {
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

pub fn jet_std_math_erfc(x: f64) -> f64 {
    1.0 - jet_std_math_erf(x)
}

/// Lanczos approximation for Γ(x) on positive reals; reflected for (0,1).
pub fn jet_std_math_gamma(x: f64) -> f64 {
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

pub fn jet_std_math_lgamma(x: f64) -> f64 {
    let g = jet_std_math_gamma(x);
    if g.is_nan() || g <= 0.0 {
        f64::NAN
    } else {
        g.ln()
    }
}
