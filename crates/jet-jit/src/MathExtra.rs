//! Extra `core.math` hosts (#1464 / I9). Algorithms from Prelude `MathLibPure`
//! via build.rs extract — marshalling only.

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.
#![allow(dead_code)]

use super::Concurrency;
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_module::Module;

pub(crate) mod math_rt {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!(concat!(env!("OUT_DIR"), "/math_rt.rs"));
}

fn jet_jit_math_abs_i64(value: i64) -> i64 {
    math_rt::jet_std_math_abs_i64(value)
}
fn jet_jit_math_abs_f64(value: f64) -> f64 {
    math_rt::jet_std_math_abs_f64(value)
}
fn jet_jit_math_abs_f32(value: f64) -> f64 {
    math_rt::jet_std_math_abs_f32(value as f32) as f64
}
fn jet_jit_math_to_bits(value: f64) -> i64 {
    math_rt::jet_std_math_to_bits(value)
}
fn jet_jit_math_from_bits(value: i64) -> f64 {
    math_rt::jet_std_math_from_bits(value)
}
fn jet_jit_math_round(value: f64) -> i64 {
    math_rt::jet_std_math_round(value)
}

fn opt_i64(v: Option<i64>) -> i64 {
    match v {
        Some(n) => n.wrapping_add(1),
        None => 0,
    }
}

fn jet_jit_math_min_i64(left: i64, right: i64) -> i64 {
    math_rt::jet_std_math_min_i64(left, right)
}
fn jet_jit_math_max_i64(left: i64, right: i64) -> i64 {
    math_rt::jet_std_math_max_i64(left, right)
}
fn jet_jit_math_clamp_i64(value: i64, low: i64, high: i64) -> i64 {
    math_rt::jet_std_math_clamp_i64(value, low, high)
}
fn jet_jit_math_min_f64(left: f64, right: f64) -> f64 {
    math_rt::jet_std_math_min_f64(left, right)
}
fn jet_jit_math_max_f64(left: f64, right: f64) -> f64 {
    math_rt::jet_std_math_max_f64(left, right)
}
fn jet_jit_math_clamp_f64(value: f64, low: f64, high: f64) -> f64 {
    math_rt::jet_std_math_clamp_f64(value, low, high)
}
fn jet_jit_math_min_f32(left: f64, right: f64) -> f64 {
    math_rt::jet_std_math_min_f32(left as f32, right as f32) as f64
}
fn jet_jit_math_max_f32(left: f64, right: f64) -> f64 {
    math_rt::jet_std_math_max_f32(left as f32, right as f32) as f64
}
fn jet_jit_math_clamp_f32(value: f64, low: f64, high: f64) -> f64 {
    math_rt::jet_std_math_clamp_f32(value as f32, low as f32, high as f32) as f64
}

fn pair_ff(a: f64, b: f64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let h = rt.heap.alloc_record(2);
        let _ = rt.heap.record_set_float(h, 0, a);
        let _ = rt.heap.record_set_float(h, 1, b);
        h
    })
}

fn pair_fi(a: f64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let h = rt.heap.alloc_record(2);
        let _ = rt.heap.record_set_float(h, 0, a);
        let _ = rt.heap.record_set_int(h, 1, b);
        h
    })
}

fn pair_ii(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let h = rt.heap.alloc_record(2);
        let _ = rt.heap.record_set_int(h, 0, a);
        let _ = rt.heap.record_set_int(h, 1, b);
        h
    })
}

// ── Prelude-backed (#1464) ───────────────────────────────────────────────────

fn jet_jit_math_erf(x: f64) -> f64 {
    math_rt::jet_std_math_erf(x)
}
fn jet_jit_math_erfc(x: f64) -> f64 {
    math_rt::jet_std_math_erfc(x)
}
fn jet_jit_math_gamma(x: f64) -> f64 {
    math_rt::jet_std_math_gamma(x)
}
fn jet_jit_math_lgamma(x: f64) -> f64 {
    math_rt::jet_std_math_lgamma(x)
}
fn jet_jit_math_ulp(x: f64) -> f64 {
    math_rt::jet_std_math_ulp(x)
}
fn jet_jit_math_significand(x: f64) -> f64 {
    math_rt::jet_std_math_significand(x)
}
fn jet_jit_math_logb(x: f64) -> f64 {
    math_rt::jet_std_math_logb(x)
}
fn jet_jit_math_ldexp(x: f64, exp: i64) -> f64 {
    math_rt::jet_std_math_ldexp(x, exp)
}
fn jet_jit_math_next_after(x: f64, toward: f64) -> f64 {
    math_rt::jet_std_math_next_after(x, toward)
}
fn jet_jit_math_cmp(a: f64, b: f64) -> i64 {
    math_rt::jet_std_math_cmp(a, b)
}
fn jet_jit_math_ilogb(x: f64) -> i64 {
    opt_i64(math_rt::jet_std_math_ilogb(x))
}
fn jet_jit_math_isqrt(v: i64) -> i64 {
    opt_i64(math_rt::jet_std_math_isqrt(v))
}
fn jet_jit_math_factorial(v: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| match rt.heap.int_factorial(v) {
        Some(value) => value.wrapping_add(1),
        None => 0,
    })
}
fn jet_jit_math_binomial(n: i64, k: i64) -> i64 {
    opt_i64(math_rt::jet_std_math_binomial(n, k))
}
fn jet_jit_math_digits(v: i64) -> i64 {
    math_rt::jet_std_math_digits(v)
}
fn jet_jit_math_leading_ones(v: i64) -> i64 {
    math_rt::jet_std_math_leading_ones(v)
}
fn jet_jit_math_trailing_ones(v: i64) -> i64 {
    math_rt::jet_std_math_trailing_ones(v)
}

// ── AOT-inlined f64/i64 methods (same semantics, marshall only) ──────────────

fn jet_jit_math_asinh(x: f64) -> f64 {
    x.asinh()
}
fn jet_jit_math_acosh(x: f64) -> f64 {
    x.acosh()
}
fn jet_jit_math_atanh(x: f64) -> f64 {
    x.atanh()
}
fn jet_jit_math_atan(x: f64) -> f64 {
    x.atan()
}
fn jet_jit_math_asin(x: f64) -> f64 {
    x.asin()
}
fn jet_jit_math_acos(x: f64) -> f64 {
    x.acos()
}
fn jet_jit_math_tan(x: f64) -> f64 {
    x.tan()
}
fn jet_jit_math_sinh(x: f64) -> f64 {
    x.sinh()
}
fn jet_jit_math_cosh(x: f64) -> f64 {
    x.cosh()
}
fn jet_jit_math_tanh(x: f64) -> f64 {
    x.tanh()
}
fn jet_jit_math_cbrt(x: f64) -> f64 {
    x.cbrt()
}
fn jet_jit_math_exp2(x: f64) -> f64 {
    x.exp2()
}
fn jet_jit_math_exp_m1(x: f64) -> f64 {
    x.exp_m1()
}
fn jet_jit_math_ln_1p(x: f64) -> f64 {
    x.ln_1p()
}
fn jet_jit_math_log(x: f64, base: f64) -> f64 {
    x.log(base)
}
fn jet_jit_math_copysign(x: f64, y: f64) -> f64 {
    x.copysign(y)
}
fn jet_jit_math_signum(x: f64) -> f64 {
    x.signum()
}
fn jet_jit_math_fma(a: f64, b: f64, c: f64) -> f64 {
    a.mul_add(b, c)
}
fn jet_jit_math_is_even(n: i64) -> i8 {
    i8::from(n % 2 == 0)
}
fn jet_jit_math_is_odd(n: i64) -> i8 {
    i8::from(n % 2 != 0)
}
fn jet_jit_math_checked_abs(n: i64) -> i64 {
    opt_i64(n.checked_abs())
}
fn jet_jit_math_checked_neg(n: i64) -> i64 {
    opt_i64(n.checked_neg())
}
fn jet_jit_math_checked_div(a: i64, b: i64) -> i64 {
    opt_i64(a.checked_div(b))
}
fn jet_jit_math_checked_rem(a: i64, b: i64) -> i64 {
    opt_i64(a.checked_rem(b))
}
fn jet_jit_math_is_normal(x: f64) -> i8 {
    i8::from(x.is_normal())
}
fn jet_jit_math_is_subnormal(x: f64) -> i8 {
    i8::from(x.is_subnormal())
}
fn jet_jit_math_is_canonical(x: f64) -> i8 {
    i8::from(x.is_finite() || x.is_nan())
}
fn jet_jit_math_is_signed(x: f64) -> i8 {
    i8::from(x.is_sign_negative())
}
fn jet_jit_math_is_zero_f(x: f64) -> i8 {
    i8::from(x == 0.0)
}
fn jet_jit_math_is_integer(x: f64) -> i8 {
    i8::from(x.is_finite() && x.fract() == 0.0)
}
fn jet_jit_math_next_up(x: f64) -> f64 {
    x.next_up()
}
fn jet_jit_math_next_down(x: f64) -> f64 {
    x.next_down()
}
fn jet_jit_math_cot(x: f64) -> f64 {
    1.0 / x.tan()
}
fn jet_jit_math_inv(x: f64) -> f64 {
    1.0 / x
}
fn jet_jit_math_sin_cos(x: f64) -> i64 {
    let (s, c) = x.sin_cos();
    pair_ff(s, c)
}
fn jet_jit_math_modf(x: f64) -> i64 {
    pair_ff(x.fract(), x.trunc())
}
fn jet_jit_math_frexp(x: f64) -> i64 {
    let exp = math_rt::jet_std_math_ilogb(x).unwrap_or(0);
    let frac = if x == 0.0 || !x.is_finite() {
        x
    } else {
        math_rt::jet_std_math_ldexp(x, -exp)
    };
    pair_fi(frac, exp)
}
fn jet_jit_math_div_mod(a: i64, b: i64) -> i64 {
    if b == 0 {
        return pair_ii(0, 0);
    }
    pair_ii(a.div_euclid(b), a.rem_euclid(b))
}
fn jet_jit_math_div_rem(a: i64, b: i64) -> i64 {
    if b == 0 {
        return pair_ii(0, 0);
    }
    pair_ii(a / b, a % b)
}

host_fns! {
    struct MathExtraHostFns;
    register: register_math_extra_symbols;
    declare: declare_math_extra_host_fns(module) {
        let cc = module.target_config().default_call_conv;
        let mut f64_f64 = Signature::new(cc);
        f64_f64.params.push(AbiParam::new(types::F64));
        f64_f64.returns.push(AbiParam::new(types::F64));
        let mut f64_f64_f64 = Signature::new(cc);
        f64_f64_f64.params.push(AbiParam::new(types::F64));
        f64_f64_f64.params.push(AbiParam::new(types::F64));
        f64_f64_f64.returns.push(AbiParam::new(types::F64));
        let mut f64_f64_f64_f64 = Signature::new(cc);
        f64_f64_f64_f64.params.push(AbiParam::new(types::F64));
        f64_f64_f64_f64.params.push(AbiParam::new(types::F64));
        f64_f64_f64_f64.params.push(AbiParam::new(types::F64));
        f64_f64_f64_f64.returns.push(AbiParam::new(types::F64));
        let mut f64_i64_f64 = Signature::new(cc);
        f64_i64_f64.params.push(AbiParam::new(types::F64));
        f64_i64_f64.params.push(AbiParam::new(types::I64));
        f64_i64_f64.returns.push(AbiParam::new(types::F64));
        let mut f64_i64 = Signature::new(cc);
        f64_i64.params.push(AbiParam::new(types::F64));
        f64_i64.returns.push(AbiParam::new(types::I64));
        let mut i64_f64 = Signature::new(cc);
        i64_f64.params.push(AbiParam::new(types::I64));
        i64_f64.returns.push(AbiParam::new(types::F64));
        let mut f64_i8 = Signature::new(cc);
        f64_i8.params.push(AbiParam::new(types::F64));
        f64_i8.returns.push(AbiParam::new(types::I8));
        let mut f64_f64_i64 = Signature::new(cc);
        f64_f64_i64.params.push(AbiParam::new(types::F64));
        f64_f64_i64.params.push(AbiParam::new(types::F64));
        f64_f64_i64.returns.push(AbiParam::new(types::I64));
        let mut fma = Signature::new(cc);
        fma.params.push(AbiParam::new(types::F64));
        fma.params.push(AbiParam::new(types::F64));
        fma.params.push(AbiParam::new(types::F64));
        fma.returns.push(AbiParam::new(types::F64));
        let mut i64_i64 = Signature::new(cc);
        i64_i64.params.push(AbiParam::new(types::I64));
        i64_i64.returns.push(AbiParam::new(types::I64));
        let mut i64_i8 = Signature::new(cc);
        i64_i8.params.push(AbiParam::new(types::I64));
        i64_i8.returns.push(AbiParam::new(types::I8));
        let mut i64_i64_i64 = Signature::new(cc);
        i64_i64_i64.params.push(AbiParam::new(types::I64));
        i64_i64_i64.params.push(AbiParam::new(types::I64));
        i64_i64_i64.returns.push(AbiParam::new(types::I64));
        let mut i64_i64_i64_i64 = Signature::new(cc);
        i64_i64_i64_i64.params.push(AbiParam::new(types::I64));
        i64_i64_i64_i64.params.push(AbiParam::new(types::I64));
        i64_i64_i64_i64.params.push(AbiParam::new(types::I64));
        i64_i64_i64_i64.returns.push(AbiParam::new(types::I64));
        let mut f64_handle = Signature::new(cc);
        f64_handle.params.push(AbiParam::new(types::F64));
        f64_handle.returns.push(AbiParam::new(types::I64));


    }
    abs_i64: "jet_jit_math_abs_i64" => jet_jit_math_abs_i64: i64_i64;
    abs_f64: "jet_jit_math_abs_f64" => jet_jit_math_abs_f64: f64_f64;
    abs_f32: "jet_jit_math_abs_f32" => jet_jit_math_abs_f32: f64_f64;
    to_bits: "jet_jit_math_to_bits" => jet_jit_math_to_bits: f64_i64;
    from_bits: "jet_jit_math_from_bits" => jet_jit_math_from_bits: i64_f64;
    round: "jet_jit_math_round" => jet_jit_math_round: f64_i64;
    erf: "jet_jit_math_erf" => jet_jit_math_erf: f64_f64;
    erfc: "jet_jit_math_erfc" => jet_jit_math_erfc: f64_f64;
    gamma: "jet_jit_math_gamma" => jet_jit_math_gamma: f64_f64;
    lgamma: "jet_jit_math_lgamma" => jet_jit_math_lgamma: f64_f64;
    ulp: "jet_jit_math_ulp" => jet_jit_math_ulp: f64_f64;
    significand: "jet_jit_math_significand" => jet_jit_math_significand: f64_f64;
    logb: "jet_jit_math_logb" => jet_jit_math_logb: f64_f64;
    ldexp: "jet_jit_math_ldexp" => jet_jit_math_ldexp: f64_i64_f64;
    next_after: "jet_jit_math_next_after" => jet_jit_math_next_after: f64_f64_f64;
    cmp: "jet_jit_math_cmp" => jet_jit_math_cmp: f64_f64_i64;
    min_i64: "jet_jit_math_min_i64" => jet_jit_math_min_i64: i64_i64_i64;
    max_i64: "jet_jit_math_max_i64" => jet_jit_math_max_i64: i64_i64_i64;
    clamp_i64: "jet_jit_math_clamp_i64" => jet_jit_math_clamp_i64: i64_i64_i64_i64;
    min_f64: "jet_jit_math_min_f64" => jet_jit_math_min_f64: f64_f64_f64;
    max_f64: "jet_jit_math_max_f64" => jet_jit_math_max_f64: f64_f64_f64;
    clamp_f64: "jet_jit_math_clamp_f64" => jet_jit_math_clamp_f64: f64_f64_f64_f64;
    min_f32: "jet_jit_math_min_f32" => jet_jit_math_min_f32: f64_f64_f64;
    max_f32: "jet_jit_math_max_f32" => jet_jit_math_max_f32: f64_f64_f64;
    clamp_f32: "jet_jit_math_clamp_f32" => jet_jit_math_clamp_f32: f64_f64_f64_f64;
    ilogb: "jet_jit_math_ilogb" => jet_jit_math_ilogb: f64_i64;
    isqrt: "jet_jit_math_isqrt" => jet_jit_math_isqrt: i64_i64;
    factorial: "jet_jit_math_factorial" => jet_jit_math_factorial: i64_i64;
    binomial: "jet_jit_math_binomial" => jet_jit_math_binomial: i64_i64_i64;
    digits: "jet_jit_math_digits" => jet_jit_math_digits: i64_i64;
    leading_ones: "jet_jit_math_leading_ones" => jet_jit_math_leading_ones: i64_i64;
    trailing_ones: "jet_jit_math_trailing_ones" => jet_jit_math_trailing_ones: i64_i64;
    asinh: "jet_jit_math_asinh" => jet_jit_math_asinh: f64_f64;
    acosh: "jet_jit_math_acosh" => jet_jit_math_acosh: f64_f64;
    atanh: "jet_jit_math_atanh" => jet_jit_math_atanh: f64_f64;
    atan: "jet_jit_math_atan" => jet_jit_math_atan: f64_f64;
    asin: "jet_jit_math_asin" => jet_jit_math_asin: f64_f64;
    acos: "jet_jit_math_acos" => jet_jit_math_acos: f64_f64;
    tan: "jet_jit_math_tan" => jet_jit_math_tan: f64_f64;
    sinh: "jet_jit_math_sinh" => jet_jit_math_sinh: f64_f64;
    cosh: "jet_jit_math_cosh" => jet_jit_math_cosh: f64_f64;
    tanh: "jet_jit_math_tanh" => jet_jit_math_tanh: f64_f64;
    cbrt: "jet_jit_math_cbrt" => jet_jit_math_cbrt: f64_f64;
    exp2: "jet_jit_math_exp2" => jet_jit_math_exp2: f64_f64;
    exp_m1: "jet_jit_math_exp_m1" => jet_jit_math_exp_m1: f64_f64;
    ln_1p: "jet_jit_math_ln_1p" => jet_jit_math_ln_1p: f64_f64;
    log: "jet_jit_math_log" => jet_jit_math_log: f64_f64_f64;
    copysign: "jet_jit_math_copysign" => jet_jit_math_copysign: f64_f64_f64;
    signum: "jet_jit_math_signum" => jet_jit_math_signum: f64_f64;
    fma: "jet_jit_math_fma" => jet_jit_math_fma: fma;
    is_even: "jet_jit_math_is_even" => jet_jit_math_is_even: i64_i8;
    is_odd: "jet_jit_math_is_odd" => jet_jit_math_is_odd: i64_i8;
    checked_abs: "jet_jit_math_checked_abs" => jet_jit_math_checked_abs: i64_i64;
    checked_neg: "jet_jit_math_checked_neg" => jet_jit_math_checked_neg: i64_i64;
    checked_div: "jet_jit_math_checked_div" => jet_jit_math_checked_div: i64_i64_i64;
    checked_rem: "jet_jit_math_checked_rem" => jet_jit_math_checked_rem: i64_i64_i64;
    is_normal: "jet_jit_math_is_normal" => jet_jit_math_is_normal: f64_i8;
    is_subnormal: "jet_jit_math_is_subnormal" => jet_jit_math_is_subnormal: f64_i8;
    is_canonical: "jet_jit_math_is_canonical" => jet_jit_math_is_canonical: f64_i8;
    is_signed: "jet_jit_math_is_signed" => jet_jit_math_is_signed: f64_i8;
    is_zero_f: "jet_jit_math_is_zero_f" => jet_jit_math_is_zero_f: f64_i8;
    is_integer: "jet_jit_math_is_integer" => jet_jit_math_is_integer: f64_i8;
    next_up: "jet_jit_math_next_up" => jet_jit_math_next_up: f64_f64;
    next_down: "jet_jit_math_next_down" => jet_jit_math_next_down: f64_f64;
    cot: "jet_jit_math_cot" => jet_jit_math_cot: f64_f64;
    inv: "jet_jit_math_inv" => jet_jit_math_inv: f64_f64;
    sin_cos: "jet_jit_math_sin_cos" => jet_jit_math_sin_cos: f64_handle;
    modf: "jet_jit_math_modf" => jet_jit_math_modf: f64_handle;
    frexp: "jet_jit_math_frexp" => jet_jit_math_frexp: f64_handle;
    div_mod: "jet_jit_math_div_mod" => jet_jit_math_div_mod: i64_i64_i64;
    div_rem: "jet_jit_math_div_rem" => jet_jit_math_div_rem: i64_i64_i64;
}
