// Math and random helpers for `core.math` and `core.math.random`.
//
// These used to live in Top/Process.rs, which only emits for `core.process`
// or the filesystem runtime. A math-only program therefore generated calls to
// symbols that were never included, which rustc rejected as an I2 violation.
// They are gated on `needs_math` here, beside the rest of the math surface.

#[inline(always)]
fn jet_std_math_sqrt(x: f64) -> f64 {
    x.sqrt()
}
fn jet_std_math_sin(x: f64) -> f64 {
    x.sin()
}
fn jet_std_math_cos(x: f64) -> f64 {
    x.cos()
}
fn jet_std_math_tan(x: f64) -> f64 {
    x.tan()
}
fn jet_std_math_asin(x: f64) -> f64 {
    x.asin()
}
fn jet_std_math_acos(x: f64) -> f64 {
    x.acos()
}
fn jet_std_math_atan(x: f64) -> f64 {
    x.atan()
}
fn jet_std_math_sinh(x: f64) -> f64 {
    x.sinh()
}
fn jet_std_math_cosh(x: f64) -> f64 {
    x.cosh()
}
fn jet_std_math_tanh(x: f64) -> f64 {
    x.tanh()
}
fn jet_std_math_ln(x: f64) -> f64 {
    x.ln()
}
fn jet_std_math_log10(x: f64) -> f64 {
    x.log10()
}
fn jet_std_math_log2(x: f64) -> f64 {
    x.log2()
}
fn jet_std_math_trunc(x: f64) -> f64 {
    x.trunc()
}
fn jet_std_math_fract(x: f64) -> f64 {
    x.fract()
}
fn jet_std_math_atan2(y: f64, x: f64) -> f64 {
    y.atan2(x)
}
fn jet_std_math_hypot(a: f64, b: f64) -> f64 {
    a.hypot(b)
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
    // D-TEST-WORLD1=A: ordinary non-cryptographic random draws use the
    // active world's isolated stream; production ambient behavior remains the
    // fallback outside a world.
    if let Some(value) = jet_scheduler_world_rng_next() {
        return value;
    }
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
    if jet_scheduler_world_rng_seed(n) {
        return;
    }
    JET_RNG.with(|cell| cell.set(n as u64));
}
fn jet_std_random_getrandbits(k: i64) -> i64 {
    let bits = k.clamp(0, 63) as u32;
    if bits == 0 {
        0
    } else {
        (jet_rng_next() >> (64 - bits)) as i64
    }
}
fn jet_std_random_randrange(start: i64, stop: i64) -> i64 {
    if stop <= start {
        return start;
    }
    jet_std_random_int(start, stop - 1)
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
fn jet_std_random_choices<T: Clone>(xs: &Vec<T>, n: i64) -> Vec<T> {
    let mut out = Vec::new();
    if xs.is_empty() {
        return out;
    }
    for _ in 0..n.max(0) {
        if let Some(value) = jet_std_random_pick(xs) {
            out.push(value);
        }
    }
    out
}
fn jet_std_random_triangular(low: f64, high: f64, mode: f64) -> f64 {
    if high <= low {
        return low;
    }
    let peak = mode.clamp(low, high);
    let u = jet_std_random_float();
    let cut = (peak - low) / (high - low);
    if u <= cut {
        low + (u * (high - low) * (peak - low)).sqrt()
    } else {
        high - ((1.0 - u) * (high - low) * (high - peak)).sqrt()
    }
}
fn jet_std_random_gamma(alpha: f64, beta: f64) -> f64 {
    if alpha <= 0.0 || beta <= 0.0 {
        return 0.0;
    }
    if alpha < 1.0 {
        return jet_std_random_gamma(alpha + 1.0, beta)
            * (jet_std_random_float_open().ln() / alpha).exp();
    }
    let d = alpha - 1.0 / 3.0;
    let c = 1.0 / (9.0 * d).sqrt();
    loop {
        let x = jet_std_random_normal(0.0, 1.0);
        let v0 = 1.0 + c * x;
        if v0 <= 0.0 {
            continue;
        }
        let v = v0 * v0 * v0;
        let u = jet_std_random_float_open();
        if u < 1.0 - 0.0331 * x.powi(4)
            || u.ln() < 0.5 * x * x + d * (1.0 - v + v.ln())
        {
            return beta * d * v;
        }
    }
}
fn jet_std_random_beta(alpha: f64, beta: f64) -> f64 {
    if alpha <= 0.0 || beta <= 0.0 {
        return 0.0;
    }
    let a = jet_std_random_gamma(alpha, 1.0);
    let b = jet_std_random_gamma(beta, 1.0);
    if a + b <= 0.0 { 0.0 } else { a / (a + b) }
}
fn jet_std_random_lognormal(mean: f64, sigma: f64) -> f64 {
    jet_std_random_normal(mean, sigma).exp()
}
fn jet_std_random_pareto(alpha: f64) -> f64 {
    if alpha <= 0.0 {
        0.0
    } else {
        (-jet_std_random_float_open().ln() / alpha).exp()
    }
}
fn jet_std_random_weibull(alpha: f64, beta: f64) -> f64 {
    if alpha <= 0.0 || beta <= 0.0 {
        0.0
    } else {
        alpha * (-jet_std_random_float_open().ln()).powf(1.0 / beta)
    }
}
fn jet_std_random_vonmises(mu: f64, kappa: f64) -> f64 {
    if kappa <= 0.0 {
        std::f64::consts::TAU * jet_std_random_float()
    } else {
        mu + jet_std_random_normal(0.0, 1.0 / kappa.sqrt())
    }
}
fn jet_std_random_binomial(n: i64, p: f64) -> i64 {
    if n <= 0 || p <= 0.0 {
        return 0;
    }
    if p >= 1.0 {
        return n;
    }
    (0..n)
        .filter(|_| jet_std_random_float() < p)
        .count() as i64
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
