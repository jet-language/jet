//! D-INTBIG1 / D-DECIMAL1 / D-NUMTYPE1: precise numeric host shims for Cranelift JIT.
//! Exact `Int` values use the packed `JetArena` carrier; spilled values are
//! opaque i64 handles into that same carrier.
//! `Decimal` / `Fraction` values are opaque i64 handles into side tables on
//! `JitRuntime`. All reuse foundation algorithms (`CtBigInt` / `CtDecimal` /
//! `CtFraction`) — same semantics AOT Prelude calls, not a third policy copy.

use super::{Concurrency, JitRuntime};
use jet_foundation::Numeric::{CtDecimal, CtFraction};
use crate::MathExtra::math_rt::JetComplex;

fn trap_decimal(msg: &str) {
    Concurrency::with_runtime_mut(|rt| {
        rt.set_trap(msg);
    });
}

fn trap_fraction(msg: &str) {
    Concurrency::with_runtime_mut(|rt| {
        rt.set_trap(msg);
    });
}

/// D-FAIL-ARITH1 / I9: a zero divisor is ONE arithmetic boundary, so both the
/// diagnostic code and the sentence come from the shared Prelude table
/// (`Prelude/Core/Contracts.rs`, reached through
/// `runtime_host::contract_kernel`), never from a host adapter's own literal.
///
/// `JitHeap::int_div_rem` answers `None` only when the divisor is zero
/// (`jet-rt/src/lib.rs`), so that is the exact fact these hosts marshal. They
/// used to raise `set_trap`'s generic `E3001` "division by zero" while the
/// fixed-width path beside them raised `E3010` "divided by zero" through this
/// same table (`jit/runtime_host.rs`, `lower_intn_values`) — one operator, two
/// reports, and the plain-`Int` one carried the invented wording.
fn divide_by_zero_message() -> &'static str {
    super::runtime_host::contract_kernel::jet_arithmetic_message("divide_zero")
}

pub(crate) fn push_decimal(d: CtDecimal) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.decimal_values.push(Some(d));
        rt.decimal_values.len() as i64
    })
}

fn with_decimal<R>(handle: i64, f: impl FnOnce(&CtDecimal) -> R) -> Option<R> {
    Concurrency::with_runtime_mut(|rt| {
        let idx = handle.saturating_sub(1) as usize;
        rt.decimal_values
            .get(idx)
            .and_then(|s| s.as_ref())
            .map(f)
    })
}

fn push_fraction(f: CtFraction) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.fraction_values.push(Some(f));
        rt.fraction_values.len() as i64
    })
}

fn with_fraction<R>(handle: i64, f: impl FnOnce(&CtFraction) -> R) -> Option<R> {
    Concurrency::with_runtime_mut(|rt| {
        let idx = handle.saturating_sub(1) as usize;
        rt.fraction_values
            .get(idx)
            .and_then(|s| s.as_ref())
            .map(f)
    })
}

fn push_complex(value: JetComplex) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.complex_values.push(Some(value));
        rt.complex_values.len() as i64
    })
}

fn with_complex<R>(handle: i64, f: impl FnOnce(&JetComplex) -> R) -> Option<R> {
    Concurrency::with_runtime_mut(|rt| {
        let idx = handle.saturating_sub(1) as usize;
        rt.complex_values
            .get(idx)
            .and_then(|value| value.as_ref())
            .map(f)
    })
}

// D-INTBIG1: default `Int` keeps a signed 63-bit payload inline and spills
// through the same JetArena/CtBigInt implementation only when required.
fn jet_jit_int_from_int(n: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.int_from_i64(n))
}

fn jet_jit_int_from_u64(n: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.int_from_u64(n as u64))
}

fn jet_jit_int_from_str(str_id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let s = rt.heap.clone_string(str_id).unwrap_or_default();
        match rt.heap.int_from_str(&s) {
            Ok(id) => id,
            Err(_) => {
                rt.set_trap("invalid default Int literal");
                0
            }
        }
    })
}

fn jet_jit_int_add(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.int_add(a, b))
}

fn jet_jit_int_sub(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.int_sub(a, b))
}

fn jet_jit_int_mul(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.int_mul(a, b))
}

fn jet_jit_int_bit_and(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.int_bit_and(a, b))
}

fn jet_jit_int_bit_or(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.int_bit_or(a, b))
}

fn jet_jit_int_bit_xor(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.int_bit_xor(a, b))
}

fn jet_jit_int_compare(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.int_compare(a, b))
}

fn jet_jit_int_neg(a: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.int_neg(a))
}

fn jet_jit_int_abs(a: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.int_abs(a))
}

fn jet_jit_int_not(a: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let negated = rt.heap.int_neg(a);
        let one = rt.heap.int_from_i64(1);
        rt.heap.int_sub(negated, one)
    })
}

fn jet_jit_int_shl(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| match rt.heap.int_shl(a, b) {
        Some(value) => value,
        None => {
            rt.set_trap("invalid shift count");
            0
        }
    })
}

fn jet_jit_int_shr(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| match rt.heap.int_shr(a, b) {
        Some(value) => value,
        None => {
            rt.set_trap("invalid shift count");
            0
        }
    })
}

fn jet_jit_int_to_string(a: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let text = rt.heap.int_to_string(a);
        rt.heap.alloc_string(text)
    })
}

fn jet_jit_int_to_f64(a: i64) -> f64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.int_to_f64(a))
}

fn jet_jit_int_div(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| match rt.heap.int_div(a, b) {
        Some(value) => value,
        None => {
            rt.set_arithmetic_stop(0, divide_by_zero_message());
            0
        }
    })
}

fn jet_jit_int_floor_div(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| match rt.heap.int_floor_div(a, b) {
        Some(value) => value,
        None => {
            rt.set_arithmetic_stop(0, divide_by_zero_message());
            0
        }
    })
}

fn jet_jit_int_mod(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| match rt.heap.int_mod(a, b) {
        Some(value) => value,
        None => {
            rt.set_arithmetic_stop(0, divide_by_zero_message());
            0
        }
    })
}

fn jet_jit_int_rem(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| match rt.heap.int_rem(a, b) {
        Some(value) => value,
        None => {
            rt.set_arithmetic_stop(0, divide_by_zero_message());
            0
        }
    })
}

fn jet_jit_int_pow(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| match rt.heap.int_pow(a, b) {
        Some(value) => value,
        None => {
            rt.set_trap("invalid default Int exponent");
            0
        }
    })
}

/// Packed legacy option ABI: `0` is absent, otherwise payload + 1.
fn jet_jit_int_factorial(a: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        match rt.heap.int_factorial(a) {
            Some(value) => crate::runtime_host::alloc_jit_result(rt, true, value as u64),
            None => crate::runtime_host::alloc_jit_result(rt, false, 0),
        }
    })
}

fn int_option(rt: &mut JitRuntime, value: Option<i64>) -> i64 {
    match value {
        Some(value) => crate::runtime_host::alloc_jit_result(rt, true, value as u64),
        None => crate::runtime_host::alloc_jit_result(rt, false, 0),
    }
}

fn int_pair(rt: &mut JitRuntime, quotient: i64, remainder: i64) -> i64 {
    let handle = rt.heap.alloc_record(2);
    let _ = rt.heap.record_set_int(handle, 0, quotient);
    let _ = rt.heap.record_set_int(handle, 1, remainder);
    handle
}

fn jet_jit_int_is_even(value: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| i8::from(rt.heap.int_is_even(value)))
}

fn jet_jit_int_is_odd(value: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| i8::from(rt.heap.int_is_odd(value)))
}

fn jet_jit_int_isqrt(value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let result = rt.heap.int_isqrt(value);
        int_option(rt, result)
    })
}

fn jet_jit_int_binomial(n: i64, k: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let result = rt.heap.int_binomial(n, k);
        int_option(rt, result)
    })
}

fn jet_jit_int_digits(value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.int_digits(value))
}

fn jet_jit_int_leading_ones(value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.int_leading_ones(value))
}

fn jet_jit_int_trailing_ones(value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.int_trailing_ones(value))
}

fn jet_jit_int_checked_abs(value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let result = rt.heap.int_checked_abs(value);
        int_option(rt, result)
    })
}

fn jet_jit_int_checked_neg(value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let result = rt.heap.int_checked_neg(value);
        int_option(rt, result)
    })
}

fn jet_jit_int_checked_add(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let result = rt.heap.int_checked_add(left, right);
        int_option(rt, result)
    })
}

fn jet_jit_int_checked_sub(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let result = rt.heap.int_checked_sub(left, right);
        int_option(rt, result)
    })
}

fn jet_jit_int_checked_mul(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let result = rt.heap.int_checked_mul(left, right);
        int_option(rt, result)
    })
}

fn jet_jit_int_checked_div(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let result = rt.heap.int_checked_div(left, right);
        int_option(rt, result)
    })
}

fn jet_jit_int_checked_rem(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let result = rt.heap.int_checked_rem(left, right);
        int_option(rt, result)
    })
}

fn jet_jit_int_checked_pow(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let result = rt.heap.int_checked_pow(left, right);
        int_option(rt, result)
    })
}

fn jet_jit_int_saturating_add(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.int_saturating_add(left, right))
}

fn jet_jit_int_saturating_sub(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.int_saturating_sub(left, right))
}

fn jet_jit_int_saturating_mul(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.int_saturating_mul(left, right))
}

fn jet_jit_int_wrapping_add(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.int_wrapping_add(left, right))
}

fn jet_jit_int_wrapping_sub(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.int_wrapping_sub(left, right))
}

fn jet_jit_int_wrapping_mul(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.int_wrapping_mul(left, right))
}

fn jet_jit_int_int_pow(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.int_int_pow(left, right))
}

fn jet_jit_int_gcd(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.int_gcd(left, right))
}

fn jet_jit_int_lcm(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.int_lcm(left, right))
}

fn jet_jit_int_div_mod(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| match rt.heap.int_div_mod(left, right) {
        Some((quotient, remainder)) => int_pair(rt, quotient, remainder),
        None => {
            rt.set_arithmetic_stop(0, divide_by_zero_message());
            int_pair(rt, 0, 0)
        }
    })
}

fn jet_jit_int_div_rem_pair(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| match rt.heap.int_div_rem(left, right) {
        Some((quotient, remainder)) => int_pair(rt, quotient, remainder),
        None => {
            rt.set_arithmetic_stop(0, divide_by_zero_message());
            int_pair(rt, 0, 0)
        }
    })
}

fn jet_jit_decimal_from_str(str_id: i64) -> i64 {
    let s = Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(str_id).unwrap_or_default());
    match CtDecimal::from_str(&s) {
        Ok(d) => push_decimal(d),
        Err(_) => {
            trap_decimal("invalid Decimal string");
            0
        }
    }
}

fn jet_jit_decimal_add(a: i64, b: i64) -> i64 {
    let left = with_decimal(a, |d| d.clone()).unwrap_or_else(|| CtDecimal::from_str("0").unwrap());
    let right = with_decimal(b, |d| d.clone()).unwrap_or_else(|| CtDecimal::from_str("0").unwrap());
    push_decimal(left.add(&right))
}

fn jet_jit_decimal_sub(a: i64, b: i64) -> i64 {
    let left = with_decimal(a, |d| d.clone()).unwrap_or_else(|| CtDecimal::from_str("0").unwrap());
    let right = with_decimal(b, |d| d.clone()).unwrap_or_else(|| CtDecimal::from_str("0").unwrap());
    push_decimal(left.sub(&right))
}

fn jet_jit_decimal_mul(a: i64, b: i64) -> i64 {
    let left = with_decimal(a, |d| d.clone()).unwrap_or_else(|| CtDecimal::from_str("0").unwrap());
    let right = with_decimal(b, |d| d.clone()).unwrap_or_else(|| CtDecimal::from_str("0").unwrap());
    push_decimal(left.mul(&right))
}

fn jet_jit_decimal_to_string(a: i64) -> i64 {
    let text = with_decimal(a, |d| d.to_string_rep()).unwrap_or_else(|| "0".to_string());
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(text))
}

/// Packed `Option<Fraction>` ABI: `0` = None, else `handle.wrapping_add(1)`.
fn jet_jit_fraction_new(numerator: i64, denominator: i64) -> i64 {
    match CtFraction::new(numerator, denominator) {
        Some(f) => push_fraction(f).wrapping_add(1),
        None => 0,
    }
}

fn jet_jit_fraction_add(a: i64, b: i64) -> i64 {
    let left = with_fraction(a, |f| *f).unwrap_or(CtFraction {
        numerator: 0,
        denominator: 1,
    });
    let right = with_fraction(b, |f| *f).unwrap_or(CtFraction {
        numerator: 0,
        denominator: 1,
    });
    match left.add(&right) {
        Some(out) => push_fraction(out),
        None => {
            trap_fraction("this sum of ratios overflows the value type");
            0
        }
    }
}

fn jet_jit_fraction_sub(a: i64, b: i64) -> i64 {
    let left = with_fraction(a, |f| *f).unwrap_or(CtFraction {
        numerator: 0,
        denominator: 1,
    });
    let right = with_fraction(b, |f| *f).unwrap_or(CtFraction {
        numerator: 0,
        denominator: 1,
    });
    match left.sub(&right) {
        Some(out) => push_fraction(out),
        None => {
            trap_fraction("this difference of ratios overflows the value type");
            0
        }
    }
}

fn jet_jit_fraction_mul(a: i64, b: i64) -> i64 {
    let left = with_fraction(a, |f| *f).unwrap_or(CtFraction {
        numerator: 0,
        denominator: 1,
    });
    let right = with_fraction(b, |f| *f).unwrap_or(CtFraction {
        numerator: 0,
        denominator: 1,
    });
    match left.mul(&right) {
        Some(out) => push_fraction(out),
        None => {
            trap_fraction("this product of ratios overflows the value type");
            0
        }
    }
}

fn jet_jit_fraction_div(a: i64, b: i64) -> i64 {
    let left = with_fraction(a, |f| *f).unwrap_or(CtFraction {
        numerator: 0,
        denominator: 1,
    });
    let right = with_fraction(b, |f| *f).unwrap_or(CtFraction {
        numerator: 0,
        denominator: 1,
    });
    match left.div(&right) {
        Some(out) => push_fraction(out),
        None => {
            trap_fraction("divided by zero");
            0
        }
    }
}

fn jet_jit_fraction_equal(a: i64, b: i64) -> i8 {
    let left = with_fraction(a, |f| *f);
    let right = with_fraction(b, |f| *f);
    match (left, right) {
        (Some(l), Some(r)) => (l == r) as i8,
        _ => 0,
    }
}

fn jet_jit_fraction_numerator(a: i64) -> i64 {
    with_fraction(a, |f| f.numerator).unwrap_or(0)
}

fn jet_jit_fraction_denominator(a: i64) -> i64 {
    with_fraction(a, |f| f.denominator).unwrap_or(1)
}

fn jet_jit_fraction_to_string(a: i64) -> i64 {
    let text = with_fraction(a, |f| f.to_string_rep()).unwrap_or_else(|| "0/1".to_string());
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(text))
}

fn jet_jit_fraction_to_float(a: i64) -> f64 {
    with_fraction(a, |f| f.numerator as f64 / f.denominator as f64).unwrap_or(0.0)
}

fn jet_jit_fraction_is_zero(a: i64) -> i8 {
    with_fraction(a, |f| f.numerator == 0).unwrap_or(false) as i8
}

fn jet_jit_complex_from_parts(real: f64, imaginary: f64) -> i64 {
    push_complex(JetComplex::from_parts(real, imaginary))
}

fn jet_jit_complex_add(a: i64, b: i64) -> i64 {
    let left = with_complex(a, |value| *value).unwrap_or_else(|| JetComplex::from_parts(0.0, 0.0));
    let right = with_complex(b, |value| *value).unwrap_or_else(|| JetComplex::from_parts(0.0, 0.0));
    push_complex(left.add(&right))
}

fn jet_jit_complex_sub(a: i64, b: i64) -> i64 {
    let left = with_complex(a, |value| *value).unwrap_or_else(|| JetComplex::from_parts(0.0, 0.0));
    let right = with_complex(b, |value| *value).unwrap_or_else(|| JetComplex::from_parts(0.0, 0.0));
    push_complex(left.sub(&right))
}

fn jet_jit_complex_mul(a: i64, b: i64) -> i64 {
    let left = with_complex(a, |value| *value).unwrap_or_else(|| JetComplex::from_parts(0.0, 0.0));
    let right = with_complex(b, |value| *value).unwrap_or_else(|| JetComplex::from_parts(0.0, 0.0));
    push_complex(left.mul(&right))
}

fn jet_jit_complex_div(a: i64, b: i64) -> i64 {
    let left = with_complex(a, |value| *value).unwrap_or_else(|| JetComplex::from_parts(0.0, 0.0));
    let right = with_complex(b, |value| *value).unwrap_or_else(|| JetComplex::from_parts(0.0, 0.0));
    push_complex(left.div(&right))
}

fn jet_jit_complex_abs(a: i64) -> f64 {
    with_complex(a, |value| value.abs()).unwrap_or(0.0)
}

fn jet_jit_complex_to_string(a: i64) -> i64 {
    let text = with_complex(a, |value| value.to_string_rep())
        .unwrap_or_else(|| "0 + 0i".to_string());
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(text))
}

host_fns! {
    struct NumericHostFns;
    register: register_numeric_symbols;
    declare: declare_numeric_host_fns(module) {
        use cranelift_codegen::ir::{types, AbiParam, Signature};
        use cranelift_module::{Linkage, Module};
        let cc = module.target_config().default_call_conv;
        let mut sig_unary = Signature::new(cc);
        sig_unary.params.push(AbiParam::new(types::I64));
        sig_unary.returns.push(AbiParam::new(types::I64));
        let mut sig_binary = Signature::new(cc);
        sig_binary.params.push(AbiParam::new(types::I64));
        sig_binary.params.push(AbiParam::new(types::I64));
        sig_binary.returns.push(AbiParam::new(types::I64));
        let mut sig_compare = Signature::new(cc);
        sig_compare.params.push(AbiParam::new(types::I64));
        sig_compare.params.push(AbiParam::new(types::I64));
        sig_compare.returns.push(AbiParam::new(types::I8));
        let mut sig_compare_i64 = Signature::new(cc);
        sig_compare_i64.params.push(AbiParam::new(types::I64));
        sig_compare_i64.params.push(AbiParam::new(types::I64));
        sig_compare_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_unary_bool = Signature::new(cc);
        sig_unary_bool.params.push(AbiParam::new(types::I64));
        sig_unary_bool.returns.push(AbiParam::new(types::I8));
        let mut sig_unary_f64 = Signature::new(cc);
        sig_unary_f64.params.push(AbiParam::new(types::I64));
        sig_unary_f64.returns.push(AbiParam::new(types::F64));
        let mut sig_complex_parts = Signature::new(cc);
        sig_complex_parts.params.push(AbiParam::new(types::F64));
        sig_complex_parts.params.push(AbiParam::new(types::F64));
        sig_complex_parts.returns.push(AbiParam::new(types::I64));


    }
    int_from_int: "jet_jit_int_from_int" => jet_jit_int_from_int: sig_unary;
    int_from_u64: "jet_jit_int_from_u64" => jet_jit_int_from_u64: sig_unary;
    int_from_str: "jet_jit_int_from_str" => jet_jit_int_from_str: sig_unary;
    int_add: "jet_jit_int_add" => jet_jit_int_add: sig_binary;
    int_sub: "jet_jit_int_sub" => jet_jit_int_sub: sig_binary;
    int_mul: "jet_jit_int_mul" => jet_jit_int_mul: sig_binary;
    int_bit_and: "jet_jit_int_bit_and" => jet_jit_int_bit_and: sig_binary;
    int_bit_or: "jet_jit_int_bit_or" => jet_jit_int_bit_or: sig_binary;
    int_bit_xor: "jet_jit_int_bit_xor" => jet_jit_int_bit_xor: sig_binary;
    int_compare: "jet_jit_int_compare" => jet_jit_int_compare: sig_compare_i64;
    int_neg: "jet_jit_int_neg" => jet_jit_int_neg: sig_unary;
    int_abs: "jet_jit_int_abs" => jet_jit_int_abs: sig_unary;
    int_not: "jet_jit_int_not" => jet_jit_int_not: sig_unary;
    int_shl: "jet_jit_int_shl" => jet_jit_int_shl: sig_binary;
    int_shr: "jet_jit_int_shr" => jet_jit_int_shr: sig_binary;
    int_to_string: "jet_jit_int_to_string" => jet_jit_int_to_string: sig_unary;
    int_to_f64: "jet_jit_int_to_f64" => jet_jit_int_to_f64: sig_unary_f64;
    int_div: "jet_jit_int_div" => jet_jit_int_div: sig_binary;
    int_floor_div: "jet_jit_int_floor_div" => jet_jit_int_floor_div: sig_binary;
    int_mod: "jet_jit_int_mod" => jet_jit_int_mod: sig_binary;
    int_rem: "jet_jit_int_rem" => jet_jit_int_rem: sig_binary;
    int_pow: "jet_jit_int_pow" => jet_jit_int_pow: sig_binary;
    int_factorial: "jet_jit_int_factorial" => jet_jit_int_factorial: sig_unary;
    int_is_even: "jet_jit_int_is_even" => jet_jit_int_is_even: sig_unary_bool;
    int_is_odd: "jet_jit_int_is_odd" => jet_jit_int_is_odd: sig_unary_bool;
    int_isqrt: "jet_jit_int_isqrt" => jet_jit_int_isqrt: sig_unary;
    int_binomial: "jet_jit_int_binomial" => jet_jit_int_binomial: sig_binary;
    int_digits: "jet_jit_int_digits" => jet_jit_int_digits: sig_unary;
    int_leading_ones: "jet_jit_int_leading_ones" => jet_jit_int_leading_ones: sig_unary;
    int_trailing_ones: "jet_jit_int_trailing_ones" => jet_jit_int_trailing_ones: sig_unary;
    int_checked_abs: "jet_jit_int_checked_abs" => jet_jit_int_checked_abs: sig_unary;
    int_checked_neg: "jet_jit_int_checked_neg" => jet_jit_int_checked_neg: sig_unary;
    int_checked_add: "jet_jit_int_checked_add" => jet_jit_int_checked_add: sig_binary;
    int_checked_sub: "jet_jit_int_checked_sub" => jet_jit_int_checked_sub: sig_binary;
    int_checked_mul: "jet_jit_int_checked_mul" => jet_jit_int_checked_mul: sig_binary;
    int_checked_div: "jet_jit_int_checked_div" => jet_jit_int_checked_div: sig_binary;
    int_checked_rem: "jet_jit_int_checked_rem" => jet_jit_int_checked_rem: sig_binary;
    int_checked_pow: "jet_jit_int_checked_pow" => jet_jit_int_checked_pow: sig_binary;
    int_saturating_add: "jet_jit_int_saturating_add" => jet_jit_int_saturating_add: sig_binary;
    int_saturating_sub: "jet_jit_int_saturating_sub" => jet_jit_int_saturating_sub: sig_binary;
    int_saturating_mul: "jet_jit_int_saturating_mul" => jet_jit_int_saturating_mul: sig_binary;
    int_wrapping_add: "jet_jit_int_wrapping_add" => jet_jit_int_wrapping_add: sig_binary;
    int_wrapping_sub: "jet_jit_int_wrapping_sub" => jet_jit_int_wrapping_sub: sig_binary;
    int_wrapping_mul: "jet_jit_int_wrapping_mul" => jet_jit_int_wrapping_mul: sig_binary;
    int_int_pow: "jet_jit_int_int_pow" => jet_jit_int_int_pow: sig_binary;
    int_gcd: "jet_jit_int_gcd" => jet_jit_int_gcd: sig_binary;
    int_lcm: "jet_jit_int_lcm" => jet_jit_int_lcm: sig_binary;
    int_div_mod: "jet_jit_int_div_mod" => jet_jit_int_div_mod: sig_binary;
    int_div_rem_pair: "jet_jit_int_div_rem_pair" => jet_jit_int_div_rem_pair: sig_binary;
    decimal_from_str: "jet_jit_decimal_from_str" => jet_jit_decimal_from_str: sig_unary;
    decimal_add: "jet_jit_decimal_add" => jet_jit_decimal_add: sig_binary;
    decimal_sub: "jet_jit_decimal_sub" => jet_jit_decimal_sub: sig_binary;
    decimal_mul: "jet_jit_decimal_mul" => jet_jit_decimal_mul: sig_binary;
    decimal_to_string: "jet_jit_decimal_to_string" => jet_jit_decimal_to_string: sig_unary;
    fraction_new: "jet_jit_fraction_new" => jet_jit_fraction_new: sig_binary;
    fraction_add: "jet_jit_fraction_add" => jet_jit_fraction_add: sig_binary;
    fraction_sub: "jet_jit_fraction_sub" => jet_jit_fraction_sub: sig_binary;
    fraction_mul: "jet_jit_fraction_mul" => jet_jit_fraction_mul: sig_binary;
    fraction_div: "jet_jit_fraction_div" => jet_jit_fraction_div: sig_binary;
    fraction_equal: "jet_jit_fraction_equal" => jet_jit_fraction_equal: sig_compare;
    fraction_numerator: "jet_jit_fraction_numerator" => jet_jit_fraction_numerator: sig_unary;
    fraction_denominator: "jet_jit_fraction_denominator" => jet_jit_fraction_denominator: sig_unary;
    fraction_to_string: "jet_jit_fraction_to_string" => jet_jit_fraction_to_string: sig_unary;
    fraction_to_float: "jet_jit_fraction_to_float" => jet_jit_fraction_to_float: sig_unary_f64;
    fraction_is_zero: "jet_jit_fraction_is_zero" => jet_jit_fraction_is_zero: sig_unary_bool;
    complex_from_parts: "jet_jit_complex_from_parts" => jet_jit_complex_from_parts: sig_complex_parts;
    complex_add: "jet_jit_complex_add" => jet_jit_complex_add: sig_binary;
    complex_sub: "jet_jit_complex_sub" => jet_jit_complex_sub: sig_binary;
    complex_mul: "jet_jit_complex_mul" => jet_jit_complex_mul: sig_binary;
    complex_div: "jet_jit_complex_div" => jet_jit_complex_div: sig_binary;
    complex_abs: "jet_jit_complex_abs" => jet_jit_complex_abs: sig_unary_f64;
    complex_to_string: "jet_jit_complex_to_string" => jet_jit_complex_to_string: sig_unary;
}
