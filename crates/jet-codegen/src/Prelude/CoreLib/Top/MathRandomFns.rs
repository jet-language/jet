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
fn jet_std_math_atan2(y: f64, x: f64) -> f64 {
    y.atan2(x)
}
fn jet_std_math_hypot(a: f64, b: f64) -> f64 {
    a.hypot(b)
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
fn jet_random_state_raw() -> u64 {
    jet_scheduler_world_rng_state().unwrap_or_else(|| JET_RNG.with(|cell| cell.get()))
}

fn jet_random_try_update_raw(expected: u64, replacement: u64) -> bool {
    jet_scheduler_world_rng_try_update(expected, replacement).unwrap_or_else(|| {
        JET_RNG.with(|cell| {
            if cell.get() != expected {
                return false;
            }
            cell.set(replacement);
            true
        })
    })
}

fn jet_std_random_seed(n: i64) {
    if jet_scheduler_world_rng_seed(n) {
        return;
    }
    JET_RNG.with(|cell| cell.set(n as u64));
}
