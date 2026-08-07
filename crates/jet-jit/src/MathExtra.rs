//! Extra `core.math` hosts (#1464 / I9). Algorithms from Prelude `MathLibPure`
//! via build.rs extract — marshalling only.

use super::Concurrency;
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};

pub(crate) mod math_rt {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!(concat!(env!("OUT_DIR"), "/math_rt.rs"));
}

fn opt_i64(v: Option<i64>) -> i64 {
    match v {
        Some(n) => n.wrapping_add(1),
        None => 0,
    }
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

extern "C" fn jet_jit_math_erf(x: f64) -> f64 {
    math_rt::jet_std_math_erf(x)
}
extern "C" fn jet_jit_math_erfc(x: f64) -> f64 {
    math_rt::jet_std_math_erfc(x)
}
extern "C" fn jet_jit_math_gamma(x: f64) -> f64 {
    math_rt::jet_std_math_gamma(x)
}
extern "C" fn jet_jit_math_lgamma(x: f64) -> f64 {
    math_rt::jet_std_math_lgamma(x)
}
extern "C" fn jet_jit_math_ulp(x: f64) -> f64 {
    math_rt::jet_std_math_ulp(x)
}
extern "C" fn jet_jit_math_significand(x: f64) -> f64 {
    math_rt::jet_std_math_significand(x)
}
extern "C" fn jet_jit_math_logb(x: f64) -> f64 {
    math_rt::jet_std_math_logb(x)
}
extern "C" fn jet_jit_math_ldexp(x: f64, exp: i64) -> f64 {
    math_rt::jet_std_math_ldexp(x, exp)
}
extern "C" fn jet_jit_math_next_after(x: f64, toward: f64) -> f64 {
    math_rt::jet_std_math_next_after(x, toward)
}
extern "C" fn jet_jit_math_cmp(a: f64, b: f64) -> i64 {
    math_rt::jet_std_math_cmp(a, b)
}
extern "C" fn jet_jit_math_ilogb(x: f64) -> i64 {
    opt_i64(math_rt::jet_std_math_ilogb(x))
}
extern "C" fn jet_jit_math_isqrt(v: i64) -> i64 {
    opt_i64(math_rt::jet_std_math_isqrt(v))
}
extern "C" fn jet_jit_math_factorial(v: i64) -> i64 {
    opt_i64(math_rt::jet_std_math_factorial(v))
}
extern "C" fn jet_jit_math_binomial(n: i64, k: i64) -> i64 {
    opt_i64(math_rt::jet_std_math_binomial(n, k))
}
extern "C" fn jet_jit_math_digits(v: i64) -> i64 {
    math_rt::jet_std_math_digits(v)
}
extern "C" fn jet_jit_math_leading_ones(v: i64) -> i64 {
    math_rt::jet_std_math_leading_ones(v)
}
extern "C" fn jet_jit_math_trailing_ones(v: i64) -> i64 {
    math_rt::jet_std_math_trailing_ones(v)
}

// ── AOT-inlined f64/i64 methods (same semantics, marshall only) ──────────────

extern "C" fn jet_jit_math_asinh(x: f64) -> f64 {
    x.asinh()
}
extern "C" fn jet_jit_math_acosh(x: f64) -> f64 {
    x.acosh()
}
extern "C" fn jet_jit_math_atanh(x: f64) -> f64 {
    x.atanh()
}
extern "C" fn jet_jit_math_atan(x: f64) -> f64 {
    x.atan()
}
extern "C" fn jet_jit_math_asin(x: f64) -> f64 {
    x.asin()
}
extern "C" fn jet_jit_math_acos(x: f64) -> f64 {
    x.acos()
}
extern "C" fn jet_jit_math_tan(x: f64) -> f64 {
    x.tan()
}
extern "C" fn jet_jit_math_sinh(x: f64) -> f64 {
    x.sinh()
}
extern "C" fn jet_jit_math_cosh(x: f64) -> f64 {
    x.cosh()
}
extern "C" fn jet_jit_math_tanh(x: f64) -> f64 {
    x.tanh()
}
extern "C" fn jet_jit_math_cbrt(x: f64) -> f64 {
    x.cbrt()
}
extern "C" fn jet_jit_math_exp2(x: f64) -> f64 {
    x.exp2()
}
extern "C" fn jet_jit_math_exp_m1(x: f64) -> f64 {
    x.exp_m1()
}
extern "C" fn jet_jit_math_ln_1p(x: f64) -> f64 {
    x.ln_1p()
}
extern "C" fn jet_jit_math_log(x: f64, base: f64) -> f64 {
    x.log(base)
}
extern "C" fn jet_jit_math_copysign(x: f64, y: f64) -> f64 {
    x.copysign(y)
}
extern "C" fn jet_jit_math_signum(x: f64) -> f64 {
    x.signum()
}
extern "C" fn jet_jit_math_fma(a: f64, b: f64, c: f64) -> f64 {
    a.mul_add(b, c)
}
extern "C" fn jet_jit_math_is_even(n: i64) -> i8 {
    i8::from(n % 2 == 0)
}
extern "C" fn jet_jit_math_is_odd(n: i64) -> i8 {
    i8::from(n % 2 != 0)
}
extern "C" fn jet_jit_math_checked_abs(n: i64) -> i64 {
    opt_i64(n.checked_abs())
}
extern "C" fn jet_jit_math_checked_neg(n: i64) -> i64 {
    opt_i64(n.checked_neg())
}
extern "C" fn jet_jit_math_checked_div(a: i64, b: i64) -> i64 {
    opt_i64(a.checked_div(b))
}
extern "C" fn jet_jit_math_checked_rem(a: i64, b: i64) -> i64 {
    opt_i64(a.checked_rem(b))
}
extern "C" fn jet_jit_math_is_normal(x: f64) -> i8 {
    i8::from(x.is_normal())
}
extern "C" fn jet_jit_math_is_subnormal(x: f64) -> i8 {
    i8::from(x.is_subnormal())
}
extern "C" fn jet_jit_math_is_canonical(x: f64) -> i8 {
    i8::from(x.is_finite() || x.is_nan())
}
extern "C" fn jet_jit_math_is_signed(x: f64) -> i8 {
    i8::from(x.is_sign_negative())
}
extern "C" fn jet_jit_math_is_zero_f(x: f64) -> i8 {
    i8::from(x == 0.0)
}
extern "C" fn jet_jit_math_is_integer(x: f64) -> i8 {
    i8::from(x.is_finite() && x.fract() == 0.0)
}
extern "C" fn jet_jit_math_next_up(x: f64) -> f64 {
    x.next_up()
}
extern "C" fn jet_jit_math_next_down(x: f64) -> f64 {
    x.next_down()
}
extern "C" fn jet_jit_math_cot(x: f64) -> f64 {
    1.0 / x.tan()
}
extern "C" fn jet_jit_math_inv(x: f64) -> f64 {
    1.0 / x
}
extern "C" fn jet_jit_math_sin_cos(x: f64) -> i64 {
    let (s, c) = x.sin_cos();
    pair_ff(s, c)
}
extern "C" fn jet_jit_math_modf(x: f64) -> i64 {
    pair_ff(x.fract(), x.trunc())
}
extern "C" fn jet_jit_math_frexp(x: f64) -> i64 {
    let exp = math_rt::jet_std_math_ilogb(x).unwrap_or(0);
    let frac = if x == 0.0 || !x.is_finite() {
        x
    } else {
        math_rt::jet_std_math_ldexp(x, -exp)
    };
    pair_fi(frac, exp)
}
extern "C" fn jet_jit_math_div_mod(a: i64, b: i64) -> i64 {
    if b == 0 {
        return pair_ii(0, 0);
    }
    pair_ii(a.div_euclid(b), a.rem_euclid(b))
}
extern "C" fn jet_jit_math_div_rem(a: i64, b: i64) -> i64 {
    if b == 0 {
        return pair_ii(0, 0);
    }
    pair_ii(a / b, a % b)
}

pub(crate) struct MathExtraHostFns {
    pub erf: FuncId,
    pub erfc: FuncId,
    pub gamma: FuncId,
    pub lgamma: FuncId,
    pub ulp: FuncId,
    pub significand: FuncId,
    pub logb: FuncId,
    pub ldexp: FuncId,
    pub next_after: FuncId,
    pub cmp: FuncId,
    pub ilogb: FuncId,
    pub isqrt: FuncId,
    pub factorial: FuncId,
    pub binomial: FuncId,
    pub digits: FuncId,
    pub leading_ones: FuncId,
    pub trailing_ones: FuncId,
    pub asinh: FuncId,
    pub acosh: FuncId,
    pub atanh: FuncId,
    pub atan: FuncId,
    pub asin: FuncId,
    pub acos: FuncId,
    pub tan: FuncId,
    pub sinh: FuncId,
    pub cosh: FuncId,
    pub tanh: FuncId,
    pub cbrt: FuncId,
    pub exp2: FuncId,
    pub exp_m1: FuncId,
    pub ln_1p: FuncId,
    pub log: FuncId,
    pub copysign: FuncId,
    pub signum: FuncId,
    pub fma: FuncId,
    pub is_even: FuncId,
    pub is_odd: FuncId,
    pub checked_abs: FuncId,
    pub checked_neg: FuncId,
    pub checked_div: FuncId,
    pub checked_rem: FuncId,
    pub is_normal: FuncId,
    pub is_subnormal: FuncId,
    pub is_canonical: FuncId,
    pub is_signed: FuncId,
    pub is_zero_f: FuncId,
    pub is_integer: FuncId,
    pub next_up: FuncId,
    pub next_down: FuncId,
    pub cot: FuncId,
    pub inv: FuncId,
    pub sin_cos: FuncId,
    pub modf: FuncId,
    pub frexp: FuncId,
    pub div_mod: FuncId,
    pub div_rem: FuncId,
}

pub(crate) fn register_math_extra_symbols(builder: &mut JITBuilder) {
    macro_rules! sym {
        ($n:expr, $f:expr) => {
            builder.symbol($n, $f as *const u8)
        };
    }
    sym!("jet_jit_math_erf", jet_jit_math_erf);
    sym!("jet_jit_math_erfc", jet_jit_math_erfc);
    sym!("jet_jit_math_gamma", jet_jit_math_gamma);
    sym!("jet_jit_math_lgamma", jet_jit_math_lgamma);
    sym!("jet_jit_math_ulp", jet_jit_math_ulp);
    sym!("jet_jit_math_significand", jet_jit_math_significand);
    sym!("jet_jit_math_logb", jet_jit_math_logb);
    sym!("jet_jit_math_ldexp", jet_jit_math_ldexp);
    sym!("jet_jit_math_next_after", jet_jit_math_next_after);
    sym!("jet_jit_math_cmp", jet_jit_math_cmp);
    sym!("jet_jit_math_ilogb", jet_jit_math_ilogb);
    sym!("jet_jit_math_isqrt", jet_jit_math_isqrt);
    sym!("jet_jit_math_factorial", jet_jit_math_factorial);
    sym!("jet_jit_math_binomial", jet_jit_math_binomial);
    sym!("jet_jit_math_digits", jet_jit_math_digits);
    sym!("jet_jit_math_leading_ones", jet_jit_math_leading_ones);
    sym!("jet_jit_math_trailing_ones", jet_jit_math_trailing_ones);
    sym!("jet_jit_math_asinh", jet_jit_math_asinh);
    sym!("jet_jit_math_acosh", jet_jit_math_acosh);
    sym!("jet_jit_math_atanh", jet_jit_math_atanh);
    sym!("jet_jit_math_atan", jet_jit_math_atan);
    sym!("jet_jit_math_asin", jet_jit_math_asin);
    sym!("jet_jit_math_acos", jet_jit_math_acos);
    sym!("jet_jit_math_tan", jet_jit_math_tan);
    sym!("jet_jit_math_sinh", jet_jit_math_sinh);
    sym!("jet_jit_math_cosh", jet_jit_math_cosh);
    sym!("jet_jit_math_tanh", jet_jit_math_tanh);
    sym!("jet_jit_math_cbrt", jet_jit_math_cbrt);
    sym!("jet_jit_math_exp2", jet_jit_math_exp2);
    sym!("jet_jit_math_exp_m1", jet_jit_math_exp_m1);
    sym!("jet_jit_math_ln_1p", jet_jit_math_ln_1p);
    sym!("jet_jit_math_log", jet_jit_math_log);
    sym!("jet_jit_math_copysign", jet_jit_math_copysign);
    sym!("jet_jit_math_signum", jet_jit_math_signum);
    sym!("jet_jit_math_fma", jet_jit_math_fma);
    sym!("jet_jit_math_is_even", jet_jit_math_is_even);
    sym!("jet_jit_math_is_odd", jet_jit_math_is_odd);
    sym!("jet_jit_math_checked_abs", jet_jit_math_checked_abs);
    sym!("jet_jit_math_checked_neg", jet_jit_math_checked_neg);
    sym!("jet_jit_math_checked_div", jet_jit_math_checked_div);
    sym!("jet_jit_math_checked_rem", jet_jit_math_checked_rem);
    sym!("jet_jit_math_is_normal", jet_jit_math_is_normal);
    sym!("jet_jit_math_is_subnormal", jet_jit_math_is_subnormal);
    sym!("jet_jit_math_is_canonical", jet_jit_math_is_canonical);
    sym!("jet_jit_math_is_signed", jet_jit_math_is_signed);
    sym!("jet_jit_math_is_zero_f", jet_jit_math_is_zero_f);
    sym!("jet_jit_math_is_integer", jet_jit_math_is_integer);
    sym!("jet_jit_math_next_up", jet_jit_math_next_up);
    sym!("jet_jit_math_next_down", jet_jit_math_next_down);
    sym!("jet_jit_math_cot", jet_jit_math_cot);
    sym!("jet_jit_math_inv", jet_jit_math_inv);
    sym!("jet_jit_math_sin_cos", jet_jit_math_sin_cos);
    sym!("jet_jit_math_modf", jet_jit_math_modf);
    sym!("jet_jit_math_frexp", jet_jit_math_frexp);
    sym!("jet_jit_math_div_mod", jet_jit_math_div_mod);
    sym!("jet_jit_math_div_rem", jet_jit_math_div_rem);
}

pub(crate) fn declare_math_extra_host_fns(module: &mut JITModule) -> Result<MathExtraHostFns, String> {
    let cc = module.target_config().default_call_conv;
    let mut f64_f64 = Signature::new(cc);
    f64_f64.params.push(AbiParam::new(types::F64));
    f64_f64.returns.push(AbiParam::new(types::F64));
    let mut f64_f64_f64 = Signature::new(cc);
    f64_f64_f64.params.push(AbiParam::new(types::F64));
    f64_f64_f64.params.push(AbiParam::new(types::F64));
    f64_f64_f64.returns.push(AbiParam::new(types::F64));
    let mut f64_i64_f64 = Signature::new(cc);
    f64_i64_f64.params.push(AbiParam::new(types::F64));
    f64_i64_f64.params.push(AbiParam::new(types::I64));
    f64_i64_f64.returns.push(AbiParam::new(types::F64));
    let mut f64_i64 = Signature::new(cc);
    f64_i64.params.push(AbiParam::new(types::F64));
    f64_i64.returns.push(AbiParam::new(types::I64));
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
    let mut f64_handle = Signature::new(cc);
    f64_handle.params.push(AbiParam::new(types::F64));
    f64_handle.returns.push(AbiParam::new(types::I64));
    let mut import = |name: &str, sig: &Signature| {
        module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|e| e.to_string())
    };
    Ok(MathExtraHostFns {
        erf: import("jet_jit_math_erf", &f64_f64)?,
        erfc: import("jet_jit_math_erfc", &f64_f64)?,
        gamma: import("jet_jit_math_gamma", &f64_f64)?,
        lgamma: import("jet_jit_math_lgamma", &f64_f64)?,
        ulp: import("jet_jit_math_ulp", &f64_f64)?,
        significand: import("jet_jit_math_significand", &f64_f64)?,
        logb: import("jet_jit_math_logb", &f64_f64)?,
        ldexp: import("jet_jit_math_ldexp", &f64_i64_f64)?,
        next_after: import("jet_jit_math_next_after", &f64_f64_f64)?,
        cmp: import("jet_jit_math_cmp", &f64_f64_i64)?,
        ilogb: import("jet_jit_math_ilogb", &f64_i64)?,
        isqrt: import("jet_jit_math_isqrt", &i64_i64)?,
        factorial: import("jet_jit_math_factorial", &i64_i64)?,
        binomial: import("jet_jit_math_binomial", &i64_i64_i64)?,
        digits: import("jet_jit_math_digits", &i64_i64)?,
        leading_ones: import("jet_jit_math_leading_ones", &i64_i64)?,
        trailing_ones: import("jet_jit_math_trailing_ones", &i64_i64)?,
        asinh: import("jet_jit_math_asinh", &f64_f64)?,
        acosh: import("jet_jit_math_acosh", &f64_f64)?,
        atanh: import("jet_jit_math_atanh", &f64_f64)?,
        atan: import("jet_jit_math_atan", &f64_f64)?,
        asin: import("jet_jit_math_asin", &f64_f64)?,
        acos: import("jet_jit_math_acos", &f64_f64)?,
        tan: import("jet_jit_math_tan", &f64_f64)?,
        sinh: import("jet_jit_math_sinh", &f64_f64)?,
        cosh: import("jet_jit_math_cosh", &f64_f64)?,
        tanh: import("jet_jit_math_tanh", &f64_f64)?,
        cbrt: import("jet_jit_math_cbrt", &f64_f64)?,
        exp2: import("jet_jit_math_exp2", &f64_f64)?,
        exp_m1: import("jet_jit_math_exp_m1", &f64_f64)?,
        ln_1p: import("jet_jit_math_ln_1p", &f64_f64)?,
        log: import("jet_jit_math_log", &f64_f64_f64)?,
        copysign: import("jet_jit_math_copysign", &f64_f64_f64)?,
        signum: import("jet_jit_math_signum", &f64_f64)?,
        fma: import("jet_jit_math_fma", &fma)?,
        is_even: import("jet_jit_math_is_even", &i64_i8)?,
        is_odd: import("jet_jit_math_is_odd", &i64_i8)?,
        checked_abs: import("jet_jit_math_checked_abs", &i64_i64)?,
        checked_neg: import("jet_jit_math_checked_neg", &i64_i64)?,
        checked_div: import("jet_jit_math_checked_div", &i64_i64_i64)?,
        checked_rem: import("jet_jit_math_checked_rem", &i64_i64_i64)?,
        is_normal: import("jet_jit_math_is_normal", &f64_i8)?,
        is_subnormal: import("jet_jit_math_is_subnormal", &f64_i8)?,
        is_canonical: import("jet_jit_math_is_canonical", &f64_i8)?,
        is_signed: import("jet_jit_math_is_signed", &f64_i8)?,
        is_zero_f: import("jet_jit_math_is_zero_f", &f64_i8)?,
        is_integer: import("jet_jit_math_is_integer", &f64_i8)?,
        next_up: import("jet_jit_math_next_up", &f64_f64)?,
        next_down: import("jet_jit_math_next_down", &f64_f64)?,
        cot: import("jet_jit_math_cot", &f64_f64)?,
        inv: import("jet_jit_math_inv", &f64_f64)?,
        sin_cos: import("jet_jit_math_sin_cos", &f64_handle)?,
        modf: import("jet_jit_math_modf", &f64_handle)?,
        frexp: import("jet_jit_math_frexp", &f64_handle)?,
        div_mod: import("jet_jit_math_div_mod", &i64_i64_i64)?,
        div_rem: import("jet_jit_math_div_rem", &i64_i64_i64)?,
    })
}
