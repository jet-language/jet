//! D-BIGINT1 / D-DECIMAL1 / D-NUMTYPE1: precise numeric host shims for Cranelift JIT.
//! `BigInt` values are opaque i64 handles into the shared `JetArena` heap.
//! `Decimal` / `Fraction` values are opaque i64 handles into side tables on
//! `JitRuntime`. All reuse foundation algorithms (`CtBigInt` / `CtDecimal` /
//! `CtFraction`) — same semantics AOT Prelude calls, not a third policy copy.

use super::Concurrency;
use jet_foundation::Numeric::{CtDecimal, CtFraction};

/// Record an invalid `BigInt(...)` literal as a trap (mirrors AOT's
/// `JetBigInt::from_str(...).expect(...)` panic, but as a JIT-safe trap
/// instead of a Rust panic unwinding through a JIT frame — I1).
fn trap_bigint(msg: &str) {
    Concurrency::with_runtime_mut(|rt| {
        rt.set_trap(msg);
    });
}

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

fn push_decimal(d: CtDecimal) -> i64 {
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

extern "C" fn jet_jit_bigint_from_int(n: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_bigint_from_int(n))
}

extern "C" fn jet_jit_bigint_from_str(str_id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let s = rt.heap.clone_string(str_id).unwrap_or_default();
        match rt.heap.alloc_bigint_from_str(&s) {
            Ok(id) => id,
            Err(_) => {
                trap_bigint("invalid BigInt string");
                0
            }
        }
    })
}

extern "C" fn jet_jit_bigint_add(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .bigint_add(a, b)
            .expect("jit bigint add: bad handle")
    })
}

extern "C" fn jet_jit_bigint_sub(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .bigint_sub(a, b)
            .expect("jit bigint sub: bad handle")
    })
}

extern "C" fn jet_jit_bigint_mul(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .bigint_mul(a, b)
            .expect("jit bigint mul: bad handle")
    })
}

extern "C" fn jet_jit_bigint_eq(a: i64, b: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .bigint_eq(a, b)
            .expect("jit bigint eq: bad handle") as i8
    })
}

extern "C" fn jet_jit_bigint_neg(a: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.bigint_neg(a).expect("jit bigint neg: bad handle"))
}

extern "C" fn jet_jit_bigint_to_string(a: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let text = rt
            .heap
            .bigint_to_string(a)
            .expect("jit bigint to_string: bad handle");
        rt.heap.alloc_string(text)
    })
}

extern "C" fn jet_jit_decimal_from_str(str_id: i64) -> i64 {
    let s = Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(str_id).unwrap_or_default());
    match CtDecimal::from_str(&s) {
        Ok(d) => push_decimal(d),
        Err(_) => {
            trap_decimal("invalid Decimal string");
            0
        }
    }
}

extern "C" fn jet_jit_decimal_add(a: i64, b: i64) -> i64 {
    let left = with_decimal(a, |d| d.clone()).unwrap_or_else(|| CtDecimal::from_str("0").unwrap());
    let right = with_decimal(b, |d| d.clone()).unwrap_or_else(|| CtDecimal::from_str("0").unwrap());
    push_decimal(left.add(&right))
}

extern "C" fn jet_jit_decimal_sub(a: i64, b: i64) -> i64 {
    let left = with_decimal(a, |d| d.clone()).unwrap_or_else(|| CtDecimal::from_str("0").unwrap());
    let right = with_decimal(b, |d| d.clone()).unwrap_or_else(|| CtDecimal::from_str("0").unwrap());
    push_decimal(left.sub(&right))
}

extern "C" fn jet_jit_decimal_mul(a: i64, b: i64) -> i64 {
    let left = with_decimal(a, |d| d.clone()).unwrap_or_else(|| CtDecimal::from_str("0").unwrap());
    let right = with_decimal(b, |d| d.clone()).unwrap_or_else(|| CtDecimal::from_str("0").unwrap());
    push_decimal(left.mul(&right))
}

extern "C" fn jet_jit_decimal_to_string(a: i64) -> i64 {
    let text = with_decimal(a, |d| d.to_string_rep()).unwrap_or_else(|| "0".to_string());
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(text))
}

/// Packed `Option<Fraction>` ABI: `0` = None, else `handle.wrapping_add(1)`.
extern "C" fn jet_jit_fraction_new(numerator: i64, denominator: i64) -> i64 {
    match CtFraction::new(numerator, denominator) {
        Some(f) => push_fraction(f).wrapping_add(1),
        None => 0,
    }
}

extern "C" fn jet_jit_fraction_add(a: i64, b: i64) -> i64 {
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

extern "C" fn jet_jit_fraction_sub(a: i64, b: i64) -> i64 {
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

extern "C" fn jet_jit_fraction_mul(a: i64, b: i64) -> i64 {
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

extern "C" fn jet_jit_fraction_div(a: i64, b: i64) -> i64 {
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

extern "C" fn jet_jit_fraction_equal(a: i64, b: i64) -> i8 {
    let left = with_fraction(a, |f| *f);
    let right = with_fraction(b, |f| *f);
    match (left, right) {
        (Some(l), Some(r)) => (l == r) as i8,
        _ => 0,
    }
}

extern "C" fn jet_jit_fraction_numerator(a: i64) -> i64 {
    with_fraction(a, |f| f.numerator).unwrap_or(0)
}

extern "C" fn jet_jit_fraction_denominator(a: i64) -> i64 {
    with_fraction(a, |f| f.denominator).unwrap_or(1)
}

extern "C" fn jet_jit_fraction_to_string(a: i64) -> i64 {
    let text = with_fraction(a, |f| f.to_string_rep()).unwrap_or_else(|| "0/1".to_string());
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(text))
}

extern "C" fn jet_jit_fraction_to_float(a: i64) -> f64 {
    with_fraction(a, |f| f.numerator as f64 / f.denominator as f64).unwrap_or(0.0)
}

extern "C" fn jet_jit_fraction_is_zero(a: i64) -> i8 {
    with_fraction(a, |f| f.numerator == 0).unwrap_or(false) as i8
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
        let mut sig_unary_bool = Signature::new(cc);
        sig_unary_bool.params.push(AbiParam::new(types::I64));
        sig_unary_bool.returns.push(AbiParam::new(types::I8));
        let mut sig_unary_f64 = Signature::new(cc);
        sig_unary_f64.params.push(AbiParam::new(types::I64));
        sig_unary_f64.returns.push(AbiParam::new(types::F64));


    }
    bigint_from_int: "jet_jit_bigint_from_int" => jet_jit_bigint_from_int: sig_unary;
    bigint_from_str: "jet_jit_bigint_from_str" => jet_jit_bigint_from_str: sig_unary;
    bigint_add: "jet_jit_bigint_add" => jet_jit_bigint_add: sig_binary;
    bigint_sub: "jet_jit_bigint_sub" => jet_jit_bigint_sub: sig_binary;
    bigint_mul: "jet_jit_bigint_mul" => jet_jit_bigint_mul: sig_binary;
    bigint_eq: "jet_jit_bigint_eq" => jet_jit_bigint_eq: sig_compare;
    bigint_neg: "jet_jit_bigint_neg" => jet_jit_bigint_neg: sig_unary;
    bigint_to_string: "jet_jit_bigint_to_string" => jet_jit_bigint_to_string: sig_unary;
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
}





