//! D-INTBIG1 / D-DECIMAL1 / D-NUMTYPE1: precise numeric host shims for Cranelift JIT.
//! Exact `Int` values use the packed `JetArena` carrier; spilled values are
//! opaque i64 handles into that same carrier.
//! `Decimal` / `Fraction` values are opaque i64 handles into side tables on
//! `JitRuntime`. All reuse foundation algorithms (`CtBigInt` / `CtDecimal` /
//! `CtFraction`) — same semantics AOT Prelude calls, not a third policy copy.

use super::{Concurrency, JitRuntime};
use crate::MathExtra::math_rt::JetComplex;
use crate::runtime_host::fixed_arithmetic_kernel::{
    self, JetFixedArithmeticResult, JET_FIXED_MODE_TRAP, JET_FIXED_MODE_WRAPPING,
    JET_FIXED_MODE_SATURATING, JET_FIXED_MODE_CHECKED,
};
use jet_foundation::Numeric::{CtBigInt, CtDecimal, CtFraction};

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
        rt.decimal_values.get(idx).and_then(|s| s.as_ref()).map(f)
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
        rt.fraction_values.get(idx).and_then(|s| s.as_ref()).map(f)
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

fn jet_jit_int_to_radix(value: i64, radix: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(radix) = rt.heap.int_to_i64(radix) else {
            rt.set_trap("integer radix must fit in Int");
            return 0;
        };
        let Ok(radix) = u32::try_from(radix) else {
            rt.set_trap("integer radix must be between 2 and 36");
            return 0;
        };
        let text = rt.heap.int_to_string(value);
        let rendered = CtBigInt::from_str(&text)
            .and_then(|value| value.to_radix(radix));
        match rendered {
            Ok(rendered) => rt.heap.alloc_string(rendered),
            Err(message) => {
                rt.set_trap(&message);
                0
            }
        }
    })
}

fn jet_jit_int_from_radix(text: i64, radix: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(radix) = rt.heap.int_to_i64(radix) else {
            rt.set_trap("integer radix must fit in Int");
            return 0;
        };
        let Ok(radix) = u32::try_from(radix) else {
            rt.set_trap("integer radix must be between 2 and 36");
            return 0;
        };
        let text = rt.heap.clone_string(text).unwrap_or_default();
        match CtBigInt::from_radix(&text, radix)
            .and_then(|value| rt.heap.int_from_str(&value.to_string_rep()))
        {
            Ok(value) => value,
            Err(message) => {
                rt.set_trap(&message);
                0
            }
        }
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

fn jet_jit_int_div_euclid(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| match rt.heap.int_div_euclid(a, b) {
        Some(value) => value,
        None => {
            rt.set_arithmetic_stop(0, divide_by_zero_message());
            0
        }
    })
}

fn jet_jit_int_rem_euclid(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| match rt.heap.int_rem_euclid(a, b) {
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

/// The canonical row set (`Codegen/TIR/routes.rs`, `exact_int_binary_route`)
/// declares the division-family `Int` operators with the AOT Prelude ABI
/// `(value, divisor, file, line)`; the source location travels with the call
/// so a zero divisor, a negative exponent, or a bad shift count stops at the
/// operator's own line exactly as `jet_std::jet_int_div` does on AOT. These
/// adapters are what the row symbols `jet_std::jet_int_{div,rem,floor_div,
/// mod,pow,shl,shr}` resolve to; the two-argument `jet_jit_int_*` hosts above
/// stay for the receiver-method rows that carry no location.
fn located_int_op(
    line: i64,
    failure: &str,
    op: impl FnOnce(&mut JitRuntime) -> Option<i64>,
) -> i64 {
    Concurrency::with_runtime_mut(|rt| match op(rt) {
        Some(value) => value,
        None => {
            rt.set_arithmetic_stop(line as u32, failure);
            0
        }
    })
}

fn jet_jit_int_div_at(a: i64, b: i64, _file: i64, line: i64) -> i64 {
    located_int_op(line, divide_by_zero_message(), |rt| rt.heap.int_div(a, b))
}

fn jet_jit_int_rem_at(a: i64, b: i64, _file: i64, line: i64) -> i64 {
    located_int_op(line, divide_by_zero_message(), |rt| rt.heap.int_rem(a, b))
}

fn jet_jit_int_floor_div_at(a: i64, b: i64, _file: i64, line: i64) -> i64 {
    located_int_op(line, divide_by_zero_message(), |rt| rt.heap.int_floor_div(a, b))
}

fn jet_jit_int_mod_at(a: i64, b: i64, _file: i64, line: i64) -> i64 {
    located_int_op(line, divide_by_zero_message(), |rt| rt.heap.int_mod(a, b))
}

fn jet_jit_int_pow_at(a: i64, b: i64, _file: i64, line: i64) -> i64 {
    located_int_op(line, "Negative default Int exponent", |rt| rt.heap.int_pow(a, b))
}

fn jet_jit_int_shl_at(a: i64, b: i64, _file: i64, line: i64) -> i64 {
    located_int_op(line, "Invalid shift count", |rt| rt.heap.int_shl(a, b))
}

fn jet_jit_int_shr_at(a: i64, b: i64, _file: i64, line: i64) -> i64 {
    located_int_op(line, "Invalid shift count", |rt| rt.heap.int_shr(a, b))
}

fn jet_jit_fixed_trap(
    left: i64,
    right: i64,
    _file: i64,
    line: i64,
    op: i64,
    signed: bool,
    bits: u8,
) -> i64 {
    jet_jit_fixed_arith(left, right, _file, line, op, JET_FIXED_MODE_TRAP, signed, bits)
}

fn jet_jit_fixed_arith(
    left: i64,
    right: i64,
    _file: i64,
    line: i64,
    op: i64,
    mode: i64,
    signed: bool,
    bits: u8,
) -> i64 {
    match fixed_arithmetic_kernel::jet_fixed_arithmetic(
        left,
        right as i128,
        op,
        mode,
        signed,
        bits,
        signed,
    ) {
        JetFixedArithmeticResult::Value(value) => value,
        JetFixedArithmeticResult::Absent => 0,
        JetFixedArithmeticResult::Trap(error) => {
            let message = error.to_string();
            Concurrency::with_runtime_mut(|rt| rt.set_arithmetic_stop(line as u32, &message));
            0
        }
    }
}

macro_rules! fixed_trap_hosts {
    ($( $host:ident => ($op:ident, $signed:expr, $bits:expr) ),+ $(,)?) => {
        $(
            fn $host(left: i64, right: i64, file: i64, line: i64) -> i64 {
                jet_jit_fixed_trap(
                    left,
                    right,
                    file,
                    line,
                    fixed_arithmetic_kernel::$op,
                    $signed,
                    $bits,
                )
            }
        )+
    };
}

fixed_trap_hosts! {
    jet_jit_i8_trap_add => (JET_FIXED_OP_ADD, true, 8),
    jet_jit_i8_trap_sub => (JET_FIXED_OP_SUB, true, 8),
    jet_jit_i8_trap_mul => (JET_FIXED_OP_MUL, true, 8),
    jet_jit_i8_trap_div => (JET_FIXED_OP_DIV, true, 8),
    jet_jit_i8_trap_rem => (JET_FIXED_OP_REM, true, 8),
    jet_jit_u8_trap_add => (JET_FIXED_OP_ADD, false, 8),
    jet_jit_u8_trap_sub => (JET_FIXED_OP_SUB, false, 8),
    jet_jit_u8_trap_mul => (JET_FIXED_OP_MUL, false, 8),
    jet_jit_u8_trap_div => (JET_FIXED_OP_DIV, false, 8),
    jet_jit_u8_trap_rem => (JET_FIXED_OP_REM, false, 8),
    jet_jit_i16_trap_add => (JET_FIXED_OP_ADD, true, 16),
    jet_jit_i16_trap_sub => (JET_FIXED_OP_SUB, true, 16),
    jet_jit_i16_trap_mul => (JET_FIXED_OP_MUL, true, 16),
    jet_jit_i16_trap_div => (JET_FIXED_OP_DIV, true, 16),
    jet_jit_i16_trap_rem => (JET_FIXED_OP_REM, true, 16),
    jet_jit_u16_trap_add => (JET_FIXED_OP_ADD, false, 16),
    jet_jit_u16_trap_sub => (JET_FIXED_OP_SUB, false, 16),
    jet_jit_u16_trap_mul => (JET_FIXED_OP_MUL, false, 16),
    jet_jit_u16_trap_div => (JET_FIXED_OP_DIV, false, 16),
    jet_jit_u16_trap_rem => (JET_FIXED_OP_REM, false, 16),
    jet_jit_i32_trap_add => (JET_FIXED_OP_ADD, true, 32),
    jet_jit_i32_trap_sub => (JET_FIXED_OP_SUB, true, 32),
    jet_jit_i32_trap_mul => (JET_FIXED_OP_MUL, true, 32),
    jet_jit_i32_trap_div => (JET_FIXED_OP_DIV, true, 32),
    jet_jit_i32_trap_rem => (JET_FIXED_OP_REM, true, 32),
    jet_jit_u32_trap_add => (JET_FIXED_OP_ADD, false, 32),
    jet_jit_u32_trap_sub => (JET_FIXED_OP_SUB, false, 32),
    jet_jit_u32_trap_mul => (JET_FIXED_OP_MUL, false, 32),
    jet_jit_u32_trap_div => (JET_FIXED_OP_DIV, false, 32),
    jet_jit_u32_trap_rem => (JET_FIXED_OP_REM, false, 32),
    jet_jit_u32_rotate_left => (JET_FIXED_OP_ROTATE_LEFT, false, 32),
    jet_jit_i64_trap_add => (JET_FIXED_OP_ADD, true, 64),
    jet_jit_i64_trap_sub => (JET_FIXED_OP_SUB, true, 64),
    jet_jit_i64_trap_mul => (JET_FIXED_OP_MUL, true, 64),
    jet_jit_i64_trap_div => (JET_FIXED_OP_DIV, true, 64),
    jet_jit_i64_trap_rem => (JET_FIXED_OP_REM, true, 64),
    jet_jit_u64_trap_add => (JET_FIXED_OP_ADD, false, 64),
    jet_jit_u64_trap_sub => (JET_FIXED_OP_SUB, false, 64),
    jet_jit_u64_trap_mul => (JET_FIXED_OP_MUL, false, 64),
    jet_jit_u64_trap_div => (JET_FIXED_OP_DIV, false, 64),
    jet_jit_u64_trap_rem => (JET_FIXED_OP_REM, false, 64),
    jet_jit_i8_trap_shl => (JET_FIXED_OP_SHL, true, 8),
    jet_jit_i8_trap_shr => (JET_FIXED_OP_SHR, true, 8),
    jet_jit_i8_trap_pow => (JET_FIXED_OP_POW, true, 8),
    jet_jit_i8_trap_floor_div => (JET_FIXED_OP_FLOOR_DIV, true, 8),
    jet_jit_i8_trap_mod => (JET_FIXED_OP_MOD, true, 8),
    jet_jit_u8_trap_shl => (JET_FIXED_OP_SHL, false, 8),
    jet_jit_u8_trap_shr => (JET_FIXED_OP_SHR, false, 8),
    jet_jit_u8_trap_pow => (JET_FIXED_OP_POW, false, 8),
    jet_jit_u8_trap_floor_div => (JET_FIXED_OP_FLOOR_DIV, false, 8),
    jet_jit_u8_trap_mod => (JET_FIXED_OP_MOD, false, 8),
    jet_jit_i16_trap_shl => (JET_FIXED_OP_SHL, true, 16),
    jet_jit_i16_trap_shr => (JET_FIXED_OP_SHR, true, 16),
    jet_jit_i16_trap_pow => (JET_FIXED_OP_POW, true, 16),
    jet_jit_i16_trap_floor_div => (JET_FIXED_OP_FLOOR_DIV, true, 16),
    jet_jit_i16_trap_mod => (JET_FIXED_OP_MOD, true, 16),
    jet_jit_u16_trap_shl => (JET_FIXED_OP_SHL, false, 16),
    jet_jit_u16_trap_shr => (JET_FIXED_OP_SHR, false, 16),
    jet_jit_u16_trap_pow => (JET_FIXED_OP_POW, false, 16),
    jet_jit_u16_trap_floor_div => (JET_FIXED_OP_FLOOR_DIV, false, 16),
    jet_jit_u16_trap_mod => (JET_FIXED_OP_MOD, false, 16),
    jet_jit_i32_trap_shl => (JET_FIXED_OP_SHL, true, 32),
    jet_jit_i32_trap_shr => (JET_FIXED_OP_SHR, true, 32),
    jet_jit_i32_trap_pow => (JET_FIXED_OP_POW, true, 32),
    jet_jit_i32_trap_floor_div => (JET_FIXED_OP_FLOOR_DIV, true, 32),
    jet_jit_i32_trap_mod => (JET_FIXED_OP_MOD, true, 32),
    jet_jit_u32_trap_shl => (JET_FIXED_OP_SHL, false, 32),
    jet_jit_u32_trap_shr => (JET_FIXED_OP_SHR, false, 32),
    jet_jit_u32_trap_pow => (JET_FIXED_OP_POW, false, 32),
    jet_jit_u32_trap_floor_div => (JET_FIXED_OP_FLOOR_DIV, false, 32),
    jet_jit_u32_trap_mod => (JET_FIXED_OP_MOD, false, 32),
    jet_jit_i64_trap_shl => (JET_FIXED_OP_SHL, true, 64),
    jet_jit_i64_trap_shr => (JET_FIXED_OP_SHR, true, 64),
    jet_jit_i64_trap_pow => (JET_FIXED_OP_POW, true, 64),
    jet_jit_i64_trap_floor_div => (JET_FIXED_OP_FLOOR_DIV, true, 64),
    jet_jit_i64_trap_mod => (JET_FIXED_OP_MOD, true, 64),
    jet_jit_u64_trap_shl => (JET_FIXED_OP_SHL, false, 64),
    jet_jit_u64_trap_shr => (JET_FIXED_OP_SHR, false, 64),
    jet_jit_u64_trap_pow => (JET_FIXED_OP_POW, false, 64),
    jet_jit_u64_trap_floor_div => (JET_FIXED_OP_FLOOR_DIV, false, 64),
    jet_jit_u64_trap_mod => (JET_FIXED_OP_MOD, false, 64),
}


/// Packed legacy option ABI: `0` is absent, otherwise payload + 1.
fn jet_jit_int_factorial(a: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| match rt.heap.int_factorial(a) {
        Some(value) => crate::runtime_host::alloc_jit_result(rt, true, value as u64),
        None => crate::runtime_host::alloc_jit_result(rt, false, 0),
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

fn jet_jit_decimal_equal(a: i64, b: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        let left = rt
            .decimal_values
            .get(a.saturating_sub(1) as usize)
            .and_then(|value| value.as_ref());
        let right = rt
            .decimal_values
            .get(b.saturating_sub(1) as usize)
            .and_then(|value| value.as_ref());
        match (left, right) {
            (Some(left), Some(right)) => i8::from(left == right),
            _ => 0,
        }
    })
}

fn jet_jit_decimal_to_string(a: i64) -> i64 {
    let text = with_decimal(a, |d| d.to_string_rep()).unwrap_or_else(|| "0".to_string());
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(text))
}

/// Packed `Option<Fraction>` ABI: `0` = None, else `handle.wrapping_add(1)`.
fn fraction_from_int_handles(numerator: i64, denominator: i64) -> Option<CtFraction> {
    Concurrency::with_runtime_mut(|rt| {
        let numerator = CtBigInt::from_str(&rt.heap.int_to_string(numerator)).ok()?;
        let denominator = CtBigInt::from_str(&rt.heap.int_to_string(denominator)).ok()?;
        CtFraction::from_bigints(numerator, denominator)
    })
}

fn jet_jit_fraction_new(numerator: i64, denominator: i64) -> i64 {
    match fraction_from_int_handles(numerator, denominator) {
        Some(f) => push_fraction(f).wrapping_add(1),
        None => 0,
    }
}

fn jet_jit_fraction_from_parts(numerator: i64, denominator: i64) -> i64 {
    match fraction_from_int_handles(numerator, denominator) {
        Some(f) => push_fraction(f),
        None => {
            trap_fraction("invalid exact quotient");
            0
        }
    }
}

fn zero_fraction() -> CtFraction {
    CtFraction {
        numerator: CtBigInt::from_int(0),
        denominator: CtBigInt::from_int(1),
    }
}

fn jet_jit_fraction_add(a: i64, b: i64) -> i64 {
    let left = with_fraction(a, Clone::clone).unwrap_or_else(zero_fraction);
    let right = with_fraction(b, Clone::clone).unwrap_or_else(zero_fraction);
    match left.add(&right) {
        Some(out) => push_fraction(out),
        None => {
            trap_fraction("this sum of ratios overflows the value type");
            0
        }
    }
}

fn jet_jit_fraction_sub(a: i64, b: i64) -> i64 {
    let left = with_fraction(a, Clone::clone).unwrap_or_else(zero_fraction);
    let right = with_fraction(b, Clone::clone).unwrap_or_else(zero_fraction);
    match left.sub(&right) {
        Some(out) => push_fraction(out),
        None => {
            trap_fraction("this difference of ratios overflows the value type");
            0
        }
    }
}

fn jet_jit_fraction_mul(a: i64, b: i64) -> i64 {
    let left = with_fraction(a, Clone::clone).unwrap_or_else(zero_fraction);
    let right = with_fraction(b, Clone::clone).unwrap_or_else(zero_fraction);
    match left.mul(&right) {
        Some(out) => push_fraction(out),
        None => {
            trap_fraction("this product of ratios overflows the value type");
            0
        }
    }
}

fn jet_jit_fraction_div(a: i64, b: i64) -> i64 {
    let left = with_fraction(a, Clone::clone).unwrap_or_else(zero_fraction);
    let right = with_fraction(b, Clone::clone).unwrap_or_else(zero_fraction);
    match left.div(&right) {
        Some(out) => push_fraction(out),
        None => {
            trap_fraction("divided by zero");
            0
        }
    }
}

fn jet_jit_fraction_equal(a: i64, b: i64) -> i8 {
    let left = with_fraction(a, Clone::clone);
    let right = with_fraction(b, Clone::clone);
    match (left, right) {
        (Some(l), Some(r)) => (l == r) as i8,
        _ => 0,
    }
}

fn jet_jit_fraction_numerator(a: i64) -> i64 {
    let text = with_fraction(a, |f| f.numerator.to_string_rep())
        .unwrap_or_else(|| "0".to_string());
    Concurrency::with_runtime_mut(|rt| rt.heap.int_from_str(&text).unwrap_or(0))
}

fn jet_jit_fraction_denominator(a: i64) -> i64 {
    let text = with_fraction(a, |f| f.denominator.to_string_rep())
        .unwrap_or_else(|| "1".to_string());
    Concurrency::with_runtime_mut(|rt| rt.heap.int_from_str(&text).unwrap_or(1))
}

fn jet_jit_fraction_to_string(a: i64) -> i64 {
    let text = with_fraction(a, |f| f.to_string_rep()).unwrap_or_else(|| "0/1".to_string());
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(text))
}

// D-TYPE2-DEFAULT1: the JIT half of the exact-to-approximate crossing. Both
// exact carriers cross at the irrational-result math functions, so the JIT
// needs Decimal here for the same reason it needs Fraction.
fn jet_jit_decimal_to_float(a: i64) -> f64 {
    with_decimal(a, |d| d.to_string_rep().parse::<f64>().unwrap_or(f64::NAN)).unwrap_or(f64::NAN)
}

fn jet_jit_fraction_to_float(a: i64) -> f64 {
    with_fraction(a, CtFraction::to_float).unwrap_or(0.0)
}

fn jet_jit_fraction_is_zero(a: i64) -> i8 {
    with_fraction(a, CtFraction::is_zero).unwrap_or(false) as i8
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
    let text =
        with_complex(a, |value| value.to_string_rep()).unwrap_or_else(|| "0 + 0i".to_string());
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(text))
}

macro_rules! fixed_wrap_hosts {
    ($( $host:ident => ($op:ident, $signed:expr, $bits:expr) ),+ $(,)?) => {
        $(
            fn $host(left: i64, right: i64) -> i64 {
                jet_jit_fixed_arith(left, right, 0, 0, fixed_arithmetic_kernel::$op, JET_FIXED_MODE_WRAPPING, $signed, $bits)
            }
        )+
    };
}


macro_rules! fixed_sat_hosts {
    ($( $host:ident => ($op:ident, $signed:expr, $bits:expr) ),+ $(,)?) => {
        $(
            fn $host(left: i64, right: i64) -> i64 {
                jet_jit_fixed_arith(left, right, 0, 0, fixed_arithmetic_kernel::$op, JET_FIXED_MODE_SATURATING, $signed, $bits)
            }
        )+
    };
}


macro_rules! fixed_checked_hosts {
    ($( $host:ident => ($op:ident, $signed:expr, $bits:expr) ),+ $(,)?) => {
        $(
            fn $host(left: i64, right: i64) -> i64 {
                jet_jit_fixed_arith(left, right, 0, 0, fixed_arithmetic_kernel::$op, JET_FIXED_MODE_CHECKED, $signed, $bits)
            }
        )+
    };
}

fixed_sat_hosts! {
    jet_jit_i8_saturating_add => (JET_FIXED_OP_ADD, true, 8),
    jet_jit_i8_saturating_sub => (JET_FIXED_OP_SUB, true, 8),
    jet_jit_i8_saturating_mul => (JET_FIXED_OP_MUL, true, 8),
    jet_jit_u8_saturating_add => (JET_FIXED_OP_ADD, false, 8),
    jet_jit_u8_saturating_sub => (JET_FIXED_OP_SUB, false, 8),
    jet_jit_u8_saturating_mul => (JET_FIXED_OP_MUL, false, 8),
    jet_jit_i16_saturating_add => (JET_FIXED_OP_ADD, true, 16),
    jet_jit_i16_saturating_sub => (JET_FIXED_OP_SUB, true, 16),
    jet_jit_i16_saturating_mul => (JET_FIXED_OP_MUL, true, 16),
    jet_jit_u16_saturating_add => (JET_FIXED_OP_ADD, false, 16),
    jet_jit_u16_saturating_sub => (JET_FIXED_OP_SUB, false, 16),
    jet_jit_u16_saturating_mul => (JET_FIXED_OP_MUL, false, 16),
    jet_jit_i32_saturating_add => (JET_FIXED_OP_ADD, true, 32),
    jet_jit_i32_saturating_sub => (JET_FIXED_OP_SUB, true, 32),
    jet_jit_i32_saturating_mul => (JET_FIXED_OP_MUL, true, 32),
    jet_jit_u32_saturating_add => (JET_FIXED_OP_ADD, false, 32),
    jet_jit_u32_saturating_sub => (JET_FIXED_OP_SUB, false, 32),
    jet_jit_u32_saturating_mul => (JET_FIXED_OP_MUL, false, 32),
    jet_jit_i64_saturating_add => (JET_FIXED_OP_ADD, true, 64),
    jet_jit_i64_saturating_sub => (JET_FIXED_OP_SUB, true, 64),
    jet_jit_i64_saturating_mul => (JET_FIXED_OP_MUL, true, 64),
    jet_jit_u64_saturating_add => (JET_FIXED_OP_ADD, false, 64),
    jet_jit_u64_saturating_sub => (JET_FIXED_OP_SUB, false, 64),
    jet_jit_u64_saturating_mul => (JET_FIXED_OP_MUL, false, 64),
}

fixed_checked_hosts! {
    jet_jit_i8_checked_add => (JET_FIXED_OP_ADD, true, 8),
    jet_jit_i8_checked_sub => (JET_FIXED_OP_SUB, true, 8),
    jet_jit_i8_checked_mul => (JET_FIXED_OP_MUL, true, 8),
    jet_jit_i8_checked_div => (JET_FIXED_OP_DIV, true, 8),
    jet_jit_i8_checked_rem => (JET_FIXED_OP_REM, true, 8),
    jet_jit_u8_checked_add => (JET_FIXED_OP_ADD, false, 8),
    jet_jit_u8_checked_sub => (JET_FIXED_OP_SUB, false, 8),
    jet_jit_u8_checked_mul => (JET_FIXED_OP_MUL, false, 8),
    jet_jit_u8_checked_div => (JET_FIXED_OP_DIV, false, 8),
    jet_jit_u8_checked_rem => (JET_FIXED_OP_REM, false, 8),
    jet_jit_i16_checked_add => (JET_FIXED_OP_ADD, true, 16),
    jet_jit_i16_checked_sub => (JET_FIXED_OP_SUB, true, 16),
    jet_jit_i16_checked_mul => (JET_FIXED_OP_MUL, true, 16),
    jet_jit_i16_checked_div => (JET_FIXED_OP_DIV, true, 16),
    jet_jit_i16_checked_rem => (JET_FIXED_OP_REM, true, 16),
    jet_jit_u16_checked_add => (JET_FIXED_OP_ADD, false, 16),
    jet_jit_u16_checked_sub => (JET_FIXED_OP_SUB, false, 16),
    jet_jit_u16_checked_mul => (JET_FIXED_OP_MUL, false, 16),
    jet_jit_u16_checked_div => (JET_FIXED_OP_DIV, false, 16),
    jet_jit_u16_checked_rem => (JET_FIXED_OP_REM, false, 16),
    jet_jit_i32_checked_add => (JET_FIXED_OP_ADD, true, 32),
    jet_jit_i32_checked_sub => (JET_FIXED_OP_SUB, true, 32),
    jet_jit_i32_checked_mul => (JET_FIXED_OP_MUL, true, 32),
    jet_jit_i32_checked_div => (JET_FIXED_OP_DIV, true, 32),
    jet_jit_i32_checked_rem => (JET_FIXED_OP_REM, true, 32),
    jet_jit_u32_checked_add => (JET_FIXED_OP_ADD, false, 32),
    jet_jit_u32_checked_sub => (JET_FIXED_OP_SUB, false, 32),
    jet_jit_u32_checked_mul => (JET_FIXED_OP_MUL, false, 32),
    jet_jit_u32_checked_div => (JET_FIXED_OP_DIV, false, 32),
    jet_jit_u32_checked_rem => (JET_FIXED_OP_REM, false, 32),
    jet_jit_i64_checked_add => (JET_FIXED_OP_ADD, true, 64),
    jet_jit_i64_checked_sub => (JET_FIXED_OP_SUB, true, 64),
    jet_jit_i64_checked_mul => (JET_FIXED_OP_MUL, true, 64),
    jet_jit_i64_checked_div => (JET_FIXED_OP_DIV, true, 64),
    jet_jit_i64_checked_rem => (JET_FIXED_OP_REM, true, 64),
    jet_jit_u64_checked_add => (JET_FIXED_OP_ADD, false, 64),
    jet_jit_u64_checked_sub => (JET_FIXED_OP_SUB, false, 64),
    jet_jit_u64_checked_mul => (JET_FIXED_OP_MUL, false, 64),
    jet_jit_u64_checked_div => (JET_FIXED_OP_DIV, false, 64),
    jet_jit_u64_checked_rem => (JET_FIXED_OP_REM, false, 64),
}

fixed_wrap_hosts! {
    jet_jit_i8_wrapping_add => (JET_FIXED_OP_ADD, true, 8),
    jet_jit_i8_wrapping_sub => (JET_FIXED_OP_SUB, true, 8),
    jet_jit_i8_wrapping_mul => (JET_FIXED_OP_MUL, true, 8),
    jet_jit_i8_wrapping_div => (JET_FIXED_OP_DIV, true, 8),
    jet_jit_i8_wrapping_rem => (JET_FIXED_OP_REM, true, 8),
    jet_jit_i8_wrapping_shl => (JET_FIXED_OP_SHL, true, 8),
    jet_jit_i8_wrapping_shr => (JET_FIXED_OP_SHR, true, 8),
    jet_jit_u8_wrapping_add => (JET_FIXED_OP_ADD, false, 8),
    jet_jit_u8_wrapping_sub => (JET_FIXED_OP_SUB, false, 8),
    jet_jit_u8_wrapping_mul => (JET_FIXED_OP_MUL, false, 8),
    jet_jit_u8_wrapping_div => (JET_FIXED_OP_DIV, false, 8),
    jet_jit_u8_wrapping_rem => (JET_FIXED_OP_REM, false, 8),
    jet_jit_u8_wrapping_shl => (JET_FIXED_OP_SHL, false, 8),
    jet_jit_u8_wrapping_shr => (JET_FIXED_OP_SHR, false, 8),
    jet_jit_i16_wrapping_add => (JET_FIXED_OP_ADD, true, 16),
    jet_jit_i16_wrapping_sub => (JET_FIXED_OP_SUB, true, 16),
    jet_jit_i16_wrapping_mul => (JET_FIXED_OP_MUL, true, 16),
    jet_jit_i16_wrapping_div => (JET_FIXED_OP_DIV, true, 16),
    jet_jit_i16_wrapping_rem => (JET_FIXED_OP_REM, true, 16),
    jet_jit_i16_wrapping_shl => (JET_FIXED_OP_SHL, true, 16),
    jet_jit_i16_wrapping_shr => (JET_FIXED_OP_SHR, true, 16),
    jet_jit_u16_wrapping_add => (JET_FIXED_OP_ADD, false, 16),
    jet_jit_u16_wrapping_sub => (JET_FIXED_OP_SUB, false, 16),
    jet_jit_u16_wrapping_mul => (JET_FIXED_OP_MUL, false, 16),
    jet_jit_u16_wrapping_div => (JET_FIXED_OP_DIV, false, 16),
    jet_jit_u16_wrapping_rem => (JET_FIXED_OP_REM, false, 16),
    jet_jit_u16_wrapping_shl => (JET_FIXED_OP_SHL, false, 16),
    jet_jit_u16_wrapping_shr => (JET_FIXED_OP_SHR, false, 16),
    jet_jit_i32_wrapping_add => (JET_FIXED_OP_ADD, true, 32),
    jet_jit_i32_wrapping_sub => (JET_FIXED_OP_SUB, true, 32),
    jet_jit_i32_wrapping_mul => (JET_FIXED_OP_MUL, true, 32),
    jet_jit_i32_wrapping_div => (JET_FIXED_OP_DIV, true, 32),
    jet_jit_i32_wrapping_rem => (JET_FIXED_OP_REM, true, 32),
    jet_jit_i32_wrapping_shl => (JET_FIXED_OP_SHL, true, 32),
    jet_jit_i32_wrapping_shr => (JET_FIXED_OP_SHR, true, 32),
    jet_jit_u32_wrapping_add => (JET_FIXED_OP_ADD, false, 32),
    jet_jit_u32_wrapping_sub => (JET_FIXED_OP_SUB, false, 32),
    jet_jit_u32_wrapping_mul => (JET_FIXED_OP_MUL, false, 32),
    jet_jit_u32_wrapping_div => (JET_FIXED_OP_DIV, false, 32),
    jet_jit_u32_wrapping_rem => (JET_FIXED_OP_REM, false, 32),
    jet_jit_u32_wrapping_shl => (JET_FIXED_OP_SHL, false, 32),
    jet_jit_u32_wrapping_shr => (JET_FIXED_OP_SHR, false, 32),
    jet_jit_i64_wrapping_add => (JET_FIXED_OP_ADD, true, 64),
    jet_jit_i64_wrapping_sub => (JET_FIXED_OP_SUB, true, 64),
    jet_jit_i64_wrapping_mul => (JET_FIXED_OP_MUL, true, 64),
    jet_jit_i64_wrapping_div => (JET_FIXED_OP_DIV, true, 64),
    jet_jit_i64_wrapping_rem => (JET_FIXED_OP_REM, true, 64),
    jet_jit_i64_wrapping_shl => (JET_FIXED_OP_SHL, true, 64),
    jet_jit_i64_wrapping_shr => (JET_FIXED_OP_SHR, true, 64),
    jet_jit_u64_wrapping_add => (JET_FIXED_OP_ADD, false, 64),
    jet_jit_u64_wrapping_sub => (JET_FIXED_OP_SUB, false, 64),
    jet_jit_u64_wrapping_mul => (JET_FIXED_OP_MUL, false, 64),
    jet_jit_u64_wrapping_div => (JET_FIXED_OP_DIV, false, 64),
    jet_jit_u64_wrapping_rem => (JET_FIXED_OP_REM, false, 64),
    jet_jit_u64_wrapping_shl => (JET_FIXED_OP_SHL, false, 64),
    jet_jit_u64_wrapping_shr => (JET_FIXED_OP_SHR, false, 64),
}

host_fns! {
    struct NumericHostFns;
    register: register_numeric_symbols;
    declare: declare_numeric_host_fns(module) {
        use cranelift_codegen::ir::{types, AbiParam, Signature};
        use cranelift_module::Module;
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
        let mut sig_located = Signature::new(cc);
        sig_located.params.extend([AbiParam::new(types::I64); 4]);
        sig_located.returns.push(AbiParam::new(types::I64));
    }
    // Canonical MIR Prelude rows (`routes.rs` `exact_int_binary_route`,
    // `TNumericOp`, `compare_route`, `precise_builtin_route`) resolve by the
    // exact symbol the row declares. These entries are that resolution; the
    // `jet_jit_*` spellings below stay for Core rows projected through
    // `CoreCallRecord::jit_symbol_candidates`.
    i8_trap_add: "jet_i8_trap_add" => jet_jit_i8_trap_add: sig_located;
    i8_trap_sub: "jet_i8_trap_sub" => jet_jit_i8_trap_sub: sig_located;
    i8_trap_mul: "jet_i8_trap_mul" => jet_jit_i8_trap_mul: sig_located;
    i8_trap_div: "jet_i8_trap_div" => jet_jit_i8_trap_div: sig_located;
    i8_trap_rem: "jet_i8_trap_rem" => jet_jit_i8_trap_rem: sig_located;
    u8_trap_add: "jet_u8_trap_add" => jet_jit_u8_trap_add: sig_located;
    u8_trap_sub: "jet_u8_trap_sub" => jet_jit_u8_trap_sub: sig_located;
    u8_trap_mul: "jet_u8_trap_mul" => jet_jit_u8_trap_mul: sig_located;
    u8_trap_div: "jet_u8_trap_div" => jet_jit_u8_trap_div: sig_located;
    u8_trap_rem: "jet_u8_trap_rem" => jet_jit_u8_trap_rem: sig_located;
    i16_trap_add: "jet_i16_trap_add" => jet_jit_i16_trap_add: sig_located;
    i16_trap_sub: "jet_i16_trap_sub" => jet_jit_i16_trap_sub: sig_located;
    i16_trap_mul: "jet_i16_trap_mul" => jet_jit_i16_trap_mul: sig_located;
    i16_trap_div: "jet_i16_trap_div" => jet_jit_i16_trap_div: sig_located;
    i16_trap_rem: "jet_i16_trap_rem" => jet_jit_i16_trap_rem: sig_located;
    u16_trap_add: "jet_u16_trap_add" => jet_jit_u16_trap_add: sig_located;
    u16_trap_sub: "jet_u16_trap_sub" => jet_jit_u16_trap_sub: sig_located;
    u16_trap_mul: "jet_u16_trap_mul" => jet_jit_u16_trap_mul: sig_located;
    u16_trap_div: "jet_u16_trap_div" => jet_jit_u16_trap_div: sig_located;
    u16_trap_rem: "jet_u16_trap_rem" => jet_jit_u16_trap_rem: sig_located;
    i32_trap_add: "jet_i32_trap_add" => jet_jit_i32_trap_add: sig_located;
    i32_trap_sub: "jet_i32_trap_sub" => jet_jit_i32_trap_sub: sig_located;
    i32_trap_mul: "jet_i32_trap_mul" => jet_jit_i32_trap_mul: sig_located;
    i32_trap_div: "jet_i32_trap_div" => jet_jit_i32_trap_div: sig_located;
    i32_trap_rem: "jet_i32_trap_rem" => jet_jit_i32_trap_rem: sig_located;
    u32_trap_add: "jet_u32_trap_add" => jet_jit_u32_trap_add: sig_located;
    u32_trap_sub: "jet_u32_trap_sub" => jet_jit_u32_trap_sub: sig_located;
    u32_trap_mul: "jet_u32_trap_mul" => jet_jit_u32_trap_mul: sig_located;
    u32_trap_div: "jet_u32_trap_div" => jet_jit_u32_trap_div: sig_located;
    u32_trap_rem: "jet_u32_trap_rem" => jet_jit_u32_trap_rem: sig_located;
    u32_rotate_left: "jet_u32_rotate_left" => jet_jit_u32_rotate_left: sig_located;
    i64_trap_add: "jet_i64_trap_add" => jet_jit_i64_trap_add: sig_located;
    i64_trap_sub: "jet_i64_trap_sub" => jet_jit_i64_trap_sub: sig_located;
    i64_trap_mul: "jet_i64_trap_mul" => jet_jit_i64_trap_mul: sig_located;
    i64_trap_div: "jet_i64_trap_div" => jet_jit_i64_trap_div: sig_located;
    i64_trap_rem: "jet_i64_trap_rem" => jet_jit_i64_trap_rem: sig_located;
    u64_trap_add: "jet_u64_trap_add" => jet_jit_u64_trap_add: sig_located;
    u64_trap_sub: "jet_u64_trap_sub" => jet_jit_u64_trap_sub: sig_located;
    u64_trap_mul: "jet_u64_trap_mul" => jet_jit_u64_trap_mul: sig_located;
    u64_trap_div: "jet_u64_trap_div" => jet_jit_u64_trap_div: sig_located;
    u64_trap_rem: "jet_u64_trap_rem" => jet_jit_u64_trap_rem: sig_located;
    i8_trap_shl: "jet_i8_trap_shl" => jet_jit_i8_trap_shl: sig_located;
    i8_trap_shr: "jet_i8_trap_shr" => jet_jit_i8_trap_shr: sig_located;
    i8_trap_pow: "jet_i8_trap_pow" => jet_jit_i8_trap_pow: sig_located;
    i8_trap_floor_div: "jet_i8_trap_floor_div" => jet_jit_i8_trap_floor_div: sig_located;
    i8_trap_mod: "jet_i8_trap_mod" => jet_jit_i8_trap_mod: sig_located;
    u8_trap_shl: "jet_u8_trap_shl" => jet_jit_u8_trap_shl: sig_located;
    u8_trap_shr: "jet_u8_trap_shr" => jet_jit_u8_trap_shr: sig_located;
    u8_trap_pow: "jet_u8_trap_pow" => jet_jit_u8_trap_pow: sig_located;
    u8_trap_floor_div: "jet_u8_trap_floor_div" => jet_jit_u8_trap_floor_div: sig_located;
    u8_trap_mod: "jet_u8_trap_mod" => jet_jit_u8_trap_mod: sig_located;
    i16_trap_shl: "jet_i16_trap_shl" => jet_jit_i16_trap_shl: sig_located;
    i16_trap_shr: "jet_i16_trap_shr" => jet_jit_i16_trap_shr: sig_located;
    i16_trap_pow: "jet_i16_trap_pow" => jet_jit_i16_trap_pow: sig_located;
    i16_trap_floor_div: "jet_i16_trap_floor_div" => jet_jit_i16_trap_floor_div: sig_located;
    i16_trap_mod: "jet_i16_trap_mod" => jet_jit_i16_trap_mod: sig_located;
    u16_trap_shl: "jet_u16_trap_shl" => jet_jit_u16_trap_shl: sig_located;
    u16_trap_shr: "jet_u16_trap_shr" => jet_jit_u16_trap_shr: sig_located;
    u16_trap_pow: "jet_u16_trap_pow" => jet_jit_u16_trap_pow: sig_located;
    u16_trap_floor_div: "jet_u16_trap_floor_div" => jet_jit_u16_trap_floor_div: sig_located;
    u16_trap_mod: "jet_u16_trap_mod" => jet_jit_u16_trap_mod: sig_located;
    i32_trap_shl: "jet_i32_trap_shl" => jet_jit_i32_trap_shl: sig_located;
    i32_trap_shr: "jet_i32_trap_shr" => jet_jit_i32_trap_shr: sig_located;
    i32_trap_pow: "jet_i32_trap_pow" => jet_jit_i32_trap_pow: sig_located;
    i32_trap_floor_div: "jet_i32_trap_floor_div" => jet_jit_i32_trap_floor_div: sig_located;
    i32_trap_mod: "jet_i32_trap_mod" => jet_jit_i32_trap_mod: sig_located;
    u32_trap_shl: "jet_u32_trap_shl" => jet_jit_u32_trap_shl: sig_located;
    u32_trap_shr: "jet_u32_trap_shr" => jet_jit_u32_trap_shr: sig_located;
    u32_trap_pow: "jet_u32_trap_pow" => jet_jit_u32_trap_pow: sig_located;
    u32_trap_floor_div: "jet_u32_trap_floor_div" => jet_jit_u32_trap_floor_div: sig_located;
    u32_trap_mod: "jet_u32_trap_mod" => jet_jit_u32_trap_mod: sig_located;
    i64_trap_shl: "jet_i64_trap_shl" => jet_jit_i64_trap_shl: sig_located;
    i64_trap_shr: "jet_i64_trap_shr" => jet_jit_i64_trap_shr: sig_located;
    i64_trap_pow: "jet_i64_trap_pow" => jet_jit_i64_trap_pow: sig_located;
    i64_trap_floor_div: "jet_i64_trap_floor_div" => jet_jit_i64_trap_floor_div: sig_located;
    i64_trap_mod: "jet_i64_trap_mod" => jet_jit_i64_trap_mod: sig_located;
    u64_trap_shl: "jet_u64_trap_shl" => jet_jit_u64_trap_shl: sig_located;
    u64_trap_shr: "jet_u64_trap_shr" => jet_jit_u64_trap_shr: sig_located;
    u64_trap_pow: "jet_u64_trap_pow" => jet_jit_u64_trap_pow: sig_located;
    u64_trap_floor_div: "jet_u64_trap_floor_div" => jet_jit_u64_trap_floor_div: sig_located;
    u64_trap_mod: "jet_u64_trap_mod" => jet_jit_u64_trap_mod: sig_binary;
    i8_wrapping_add: "jet_i8_wrapping_add" => jet_jit_i8_wrapping_add: sig_binary;
    i8_wrapping_sub: "jet_i8_wrapping_sub" => jet_jit_i8_wrapping_sub: sig_binary;
    i8_wrapping_mul: "jet_i8_wrapping_mul" => jet_jit_i8_wrapping_mul: sig_binary;
    i8_wrapping_div: "jet_i8_wrapping_div" => jet_jit_i8_wrapping_div: sig_binary;
    i8_wrapping_rem: "jet_i8_wrapping_rem" => jet_jit_i8_wrapping_rem: sig_binary;
    i8_wrapping_shl: "jet_i8_wrapping_shl" => jet_jit_i8_wrapping_shl: sig_binary;
    i8_wrapping_shr: "jet_i8_wrapping_shr" => jet_jit_i8_wrapping_shr: sig_binary;
    u8_wrapping_add: "jet_u8_wrapping_add" => jet_jit_u8_wrapping_add: sig_binary;
    u8_wrapping_sub: "jet_u8_wrapping_sub" => jet_jit_u8_wrapping_sub: sig_binary;
    u8_wrapping_mul: "jet_u8_wrapping_mul" => jet_jit_u8_wrapping_mul: sig_binary;
    u8_wrapping_div: "jet_u8_wrapping_div" => jet_jit_u8_wrapping_div: sig_binary;
    u8_wrapping_rem: "jet_u8_wrapping_rem" => jet_jit_u8_wrapping_rem: sig_binary;
    u8_wrapping_shl: "jet_u8_wrapping_shl" => jet_jit_u8_wrapping_shl: sig_binary;
    u8_wrapping_shr: "jet_u8_wrapping_shr" => jet_jit_u8_wrapping_shr: sig_binary;
    i16_wrapping_add: "jet_i16_wrapping_add" => jet_jit_i16_wrapping_add: sig_binary;
    i16_wrapping_sub: "jet_i16_wrapping_sub" => jet_jit_i16_wrapping_sub: sig_binary;
    i16_wrapping_mul: "jet_i16_wrapping_mul" => jet_jit_i16_wrapping_mul: sig_binary;
    i16_wrapping_div: "jet_i16_wrapping_div" => jet_jit_i16_wrapping_div: sig_binary;
    i16_wrapping_rem: "jet_i16_wrapping_rem" => jet_jit_i16_wrapping_rem: sig_binary;
    i16_wrapping_shl: "jet_i16_wrapping_shl" => jet_jit_i16_wrapping_shl: sig_binary;
    i16_wrapping_shr: "jet_i16_wrapping_shr" => jet_jit_i16_wrapping_shr: sig_binary;
    u16_wrapping_add: "jet_u16_wrapping_add" => jet_jit_u16_wrapping_add: sig_binary;
    u16_wrapping_sub: "jet_u16_wrapping_sub" => jet_jit_u16_wrapping_sub: sig_binary;
    u16_wrapping_mul: "jet_u16_wrapping_mul" => jet_jit_u16_wrapping_mul: sig_binary;
    u16_wrapping_div: "jet_u16_wrapping_div" => jet_jit_u16_wrapping_div: sig_binary;
    u16_wrapping_rem: "jet_u16_wrapping_rem" => jet_jit_u16_wrapping_rem: sig_binary;
    u16_wrapping_shl: "jet_u16_wrapping_shl" => jet_jit_u16_wrapping_shl: sig_binary;
    u16_wrapping_shr: "jet_u16_wrapping_shr" => jet_jit_u16_wrapping_shr: sig_binary;
    i32_wrapping_add: "jet_i32_wrapping_add" => jet_jit_i32_wrapping_add: sig_binary;
    i32_wrapping_sub: "jet_i32_wrapping_sub" => jet_jit_i32_wrapping_sub: sig_binary;
    i32_wrapping_mul: "jet_i32_wrapping_mul" => jet_jit_i32_wrapping_mul: sig_binary;
    i32_wrapping_div: "jet_i32_wrapping_div" => jet_jit_i32_wrapping_div: sig_binary;
    i32_wrapping_rem: "jet_i32_wrapping_rem" => jet_jit_i32_wrapping_rem: sig_binary;
    i32_wrapping_shl: "jet_i32_wrapping_shl" => jet_jit_i32_wrapping_shl: sig_binary;
    i32_wrapping_shr: "jet_i32_wrapping_shr" => jet_jit_i32_wrapping_shr: sig_binary;
    u32_wrapping_add: "jet_u32_wrapping_add" => jet_jit_u32_wrapping_add: sig_binary;
    u32_wrapping_sub: "jet_u32_wrapping_sub" => jet_jit_u32_wrapping_sub: sig_binary;
    u32_wrapping_mul: "jet_u32_wrapping_mul" => jet_jit_u32_wrapping_mul: sig_binary;
    u32_wrapping_div: "jet_u32_wrapping_div" => jet_jit_u32_wrapping_div: sig_binary;
    u32_wrapping_rem: "jet_u32_wrapping_rem" => jet_jit_u32_wrapping_rem: sig_binary;
    u32_wrapping_shl: "jet_u32_wrapping_shl" => jet_jit_u32_wrapping_shl: sig_binary;
    u32_wrapping_shr: "jet_u32_wrapping_shr" => jet_jit_u32_wrapping_shr: sig_binary;
    i64_wrapping_add: "jet_i64_wrapping_add" => jet_jit_i64_wrapping_add: sig_binary;
    i64_wrapping_sub: "jet_i64_wrapping_sub" => jet_jit_i64_wrapping_sub: sig_binary;
    i64_wrapping_mul: "jet_i64_wrapping_mul" => jet_jit_i64_wrapping_mul: sig_binary;
    i64_wrapping_div: "jet_i64_wrapping_div" => jet_jit_i64_wrapping_div: sig_binary;
    i64_wrapping_rem: "jet_i64_wrapping_rem" => jet_jit_i64_wrapping_rem: sig_binary;
    i64_wrapping_shl: "jet_i64_wrapping_shl" => jet_jit_i64_wrapping_shl: sig_binary;
    i64_wrapping_shr: "jet_i64_wrapping_shr" => jet_jit_i64_wrapping_shr: sig_binary;
    u64_wrapping_add: "jet_u64_wrapping_add" => jet_jit_u64_wrapping_add: sig_binary;
    u64_wrapping_sub: "jet_u64_wrapping_sub" => jet_jit_u64_wrapping_sub: sig_binary;
    u64_wrapping_mul: "jet_u64_wrapping_mul" => jet_jit_u64_wrapping_mul: sig_binary;
    u64_wrapping_div: "jet_u64_wrapping_div" => jet_jit_u64_wrapping_div: sig_binary;
    u64_wrapping_rem: "jet_u64_wrapping_rem" => jet_jit_u64_wrapping_rem: sig_binary;
    u64_wrapping_shl: "jet_u64_wrapping_shl" => jet_jit_u64_wrapping_shl: sig_binary;
    u64_wrapping_shr: "jet_u64_wrapping_shr" => jet_jit_u64_wrapping_shr: sig_binary;
    i8_saturating_add: "jet_i8_saturating_add" => jet_jit_i8_saturating_add: sig_binary;
    i8_saturating_sub: "jet_i8_saturating_sub" => jet_jit_i8_saturating_sub: sig_binary;
    i8_saturating_mul: "jet_i8_saturating_mul" => jet_jit_i8_saturating_mul: sig_binary;
    u8_saturating_add: "jet_u8_saturating_add" => jet_jit_u8_saturating_add: sig_binary;
    u8_saturating_sub: "jet_u8_saturating_sub" => jet_jit_u8_saturating_sub: sig_binary;
    u8_saturating_mul: "jet_u8_saturating_mul" => jet_jit_u8_saturating_mul: sig_binary;
    i16_saturating_add: "jet_i16_saturating_add" => jet_jit_i16_saturating_add: sig_binary;
    i16_saturating_sub: "jet_i16_saturating_sub" => jet_jit_i16_saturating_sub: sig_binary;
    i16_saturating_mul: "jet_i16_saturating_mul" => jet_jit_i16_saturating_mul: sig_binary;
    u16_saturating_add: "jet_u16_saturating_add" => jet_jit_u16_saturating_add: sig_binary;
    u16_saturating_sub: "jet_u16_saturating_sub" => jet_jit_u16_saturating_sub: sig_binary;
    u16_saturating_mul: "jet_u16_saturating_mul" => jet_jit_u16_saturating_mul: sig_binary;
    i32_saturating_add: "jet_i32_saturating_add" => jet_jit_i32_saturating_add: sig_binary;
    i32_saturating_sub: "jet_i32_saturating_sub" => jet_jit_i32_saturating_sub: sig_binary;
    i32_saturating_mul: "jet_i32_saturating_mul" => jet_jit_i32_saturating_mul: sig_binary;
    u32_saturating_add: "jet_u32_saturating_add" => jet_jit_u32_saturating_add: sig_binary;
    u32_saturating_sub: "jet_u32_saturating_sub" => jet_jit_u32_saturating_sub: sig_binary;
    u32_saturating_mul: "jet_u32_saturating_mul" => jet_jit_u32_saturating_mul: sig_binary;
    i64_saturating_add: "jet_i64_saturating_add" => jet_jit_i64_saturating_add: sig_binary;
    i64_saturating_sub: "jet_i64_saturating_sub" => jet_jit_i64_saturating_sub: sig_binary;
    i64_saturating_mul: "jet_i64_saturating_mul" => jet_jit_i64_saturating_mul: sig_binary;
    u64_saturating_add: "jet_u64_saturating_add" => jet_jit_u64_saturating_add: sig_binary;
    u64_saturating_sub: "jet_u64_saturating_sub" => jet_jit_u64_saturating_sub: sig_binary;
    u64_saturating_mul: "jet_u64_saturating_mul" => jet_jit_u64_saturating_mul: sig_binary;
    i8_checked_add: "jet_i8_checked_add" => jet_jit_i8_checked_add: sig_binary;
    i8_checked_sub: "jet_i8_checked_sub" => jet_jit_i8_checked_sub: sig_binary;
    i8_checked_mul: "jet_i8_checked_mul" => jet_jit_i8_checked_mul: sig_binary;
    i8_checked_div: "jet_i8_checked_div" => jet_jit_i8_checked_div: sig_binary;
    i8_checked_rem: "jet_i8_checked_rem" => jet_jit_i8_checked_rem: sig_binary;
    u8_checked_add: "jet_u8_checked_add" => jet_jit_u8_checked_add: sig_binary;
    u8_checked_sub: "jet_u8_checked_sub" => jet_jit_u8_checked_sub: sig_binary;
    u8_checked_mul: "jet_u8_checked_mul" => jet_jit_u8_checked_mul: sig_binary;
    u8_checked_div: "jet_u8_checked_div" => jet_jit_u8_checked_div: sig_binary;
    u8_checked_rem: "jet_u8_checked_rem" => jet_jit_u8_checked_rem: sig_binary;
    i16_checked_add: "jet_i16_checked_add" => jet_jit_i16_checked_add: sig_binary;
    i16_checked_sub: "jet_i16_checked_sub" => jet_jit_i16_checked_sub: sig_binary;
    i16_checked_mul: "jet_i16_checked_mul" => jet_jit_i16_checked_mul: sig_binary;
    i16_checked_div: "jet_i16_checked_div" => jet_jit_i16_checked_div: sig_binary;
    i16_checked_rem: "jet_i16_checked_rem" => jet_jit_i16_checked_rem: sig_binary;
    u16_checked_add: "jet_u16_checked_add" => jet_jit_u16_checked_add: sig_binary;
    u16_checked_sub: "jet_u16_checked_sub" => jet_jit_u16_checked_sub: sig_binary;
    u16_checked_mul: "jet_u16_checked_mul" => jet_jit_u16_checked_mul: sig_binary;
    u16_checked_div: "jet_u16_checked_div" => jet_jit_u16_checked_div: sig_binary;
    u16_checked_rem: "jet_u16_checked_rem" => jet_jit_u16_checked_rem: sig_binary;
    i32_checked_add: "jet_i32_checked_add" => jet_jit_i32_checked_add: sig_binary;
    i32_checked_sub: "jet_i32_checked_sub" => jet_jit_i32_checked_sub: sig_binary;
    i32_checked_mul: "jet_i32_checked_mul" => jet_jit_i32_checked_mul: sig_binary;
    i32_checked_div: "jet_i32_checked_div" => jet_jit_i32_checked_div: sig_binary;
    i32_checked_rem: "jet_i32_checked_rem" => jet_jit_i32_checked_rem: sig_binary;
    u32_checked_add: "jet_u32_checked_add" => jet_jit_u32_checked_add: sig_binary;
    u32_checked_sub: "jet_u32_checked_sub" => jet_jit_u32_checked_sub: sig_binary;
    u32_checked_mul: "jet_u32_checked_mul" => jet_jit_u32_checked_mul: sig_binary;
    u32_checked_div: "jet_u32_checked_div" => jet_jit_u32_checked_div: sig_binary;
    u32_checked_rem: "jet_u32_checked_rem" => jet_jit_u32_checked_rem: sig_binary;
    i64_checked_add: "jet_i64_checked_add" => jet_jit_i64_checked_add: sig_binary;
    i64_checked_sub: "jet_i64_checked_sub" => jet_jit_i64_checked_sub: sig_binary;
    i64_checked_mul: "jet_i64_checked_mul" => jet_jit_i64_checked_mul: sig_binary;
    i64_checked_div: "jet_i64_checked_div" => jet_jit_i64_checked_div: sig_binary;
    i64_checked_rem: "jet_i64_checked_rem" => jet_jit_i64_checked_rem: sig_binary;
    u64_checked_add: "jet_u64_checked_add" => jet_jit_u64_checked_add: sig_binary;
    u64_checked_sub: "jet_u64_checked_sub" => jet_jit_u64_checked_sub: sig_binary;
    u64_checked_mul: "jet_u64_checked_mul" => jet_jit_u64_checked_mul: sig_binary;
    u64_checked_div: "jet_u64_checked_div" => jet_jit_u64_checked_div: sig_binary;
    u64_checked_rem: "jet_u64_checked_rem" => jet_jit_u64_checked_rem: sig_binary;
    row_int_add: "jet_std::jet_int_add" => jet_jit_int_add: sig_binary;
    row_int_sub: "jet_std::jet_int_sub" => jet_jit_int_sub: sig_binary;
    row_int_mul: "jet_std::jet_int_mul" => jet_jit_int_mul: sig_binary;
    row_int_bit_and: "jet_std::jet_int_bit_and" => jet_jit_int_bit_and: sig_binary;
    row_int_bit_or: "jet_std::jet_int_bit_or" => jet_jit_int_bit_or: sig_binary;
    row_int_bit_xor: "jet_std::jet_int_bit_xor" => jet_jit_int_bit_xor: sig_binary;
    row_int_div: "jet_std::jet_int_div" => jet_jit_int_div_at: sig_located;
    row_int_rem: "jet_std::jet_int_rem" => jet_jit_int_rem_at: sig_located;
    row_int_floor_div: "jet_std::jet_int_floor_div" => jet_jit_int_floor_div_at: sig_located;
    row_int_mod: "jet_std::jet_int_mod" => jet_jit_int_mod_at: sig_located;
    row_int_pow: "jet_std::jet_int_pow" => jet_jit_int_pow_at: sig_located;
    row_int_shl: "jet_std::jet_int_shl" => jet_jit_int_shl_at: sig_located;
    row_int_shr: "jet_std::jet_int_shr" => jet_jit_int_shr_at: sig_located;
    row_int_div_euclid: "jet_std::jet_int_div_euclid" => jet_jit_int_div_euclid: sig_binary;
    row_int_rem_euclid: "jet_std::jet_int_rem_euclid" => jet_jit_int_rem_euclid: sig_binary;
    row_int_abs: "jet_std::jet_int_abs" => jet_jit_int_abs: sig_unary;
    row_int_compare: "jet_int_compare" => jet_jit_int_compare: sig_compare_i64;
    row_decimal_from_str: "jet_decimal_from_str" => jet_jit_decimal_from_str: sig_unary;
    row_decimal_add: "jet_decimal_add" => jet_jit_decimal_add: sig_binary;
    row_decimal_sub: "jet_decimal_sub" => jet_jit_decimal_sub: sig_binary;
    row_decimal_mul: "jet_decimal_mul" => jet_jit_decimal_mul: sig_binary;
    row_decimal_equal: "jet_decimal_equal" => jet_jit_decimal_equal: sig_compare;
    row_decimal_to_string: "jet_decimal_to_string" => jet_jit_decimal_to_string: sig_unary;
    row_decimal_to_float: "jet_decimal_to_float" => jet_jit_decimal_to_float: sig_unary_f64;
    row_fraction_from_parts: "jet_fraction_from_parts" => jet_jit_fraction_from_parts: sig_binary;
    row_fraction_add: "jet_fraction_add" => jet_jit_fraction_add: sig_binary;
    row_fraction_sub: "jet_fraction_sub" => jet_jit_fraction_sub: sig_binary;
    row_fraction_mul: "jet_fraction_mul" => jet_jit_fraction_mul: sig_binary;
    row_fraction_div: "jet_fraction_div" => jet_jit_fraction_div: sig_binary;
    row_fraction_equal: "jet_fraction_equal" => jet_jit_fraction_equal: sig_compare;
    row_fraction_to_string: "jet_fraction_to_string" => jet_jit_fraction_to_string: sig_unary;
    row_fraction_to_float: "jet_fraction_to_float" => jet_jit_fraction_to_float: sig_unary_f64;
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
    int_to_radix: "jet_jit_int_to_radix" => jet_jit_int_to_radix: sig_binary;
    int_from_radix: "jet_jit_int_from_radix" => jet_jit_int_from_radix: sig_binary;
    int_to_f64: "jet_jit_int_to_f64" => jet_jit_int_to_f64: sig_unary_f64;
    int_div: "jet_jit_int_div" => jet_jit_int_div: sig_binary;
    int_div_euclid: "jet_jit_int_div_euclid" => jet_jit_int_div_euclid: sig_binary;
    int_floor_div: "jet_jit_int_floor_div" => jet_jit_int_floor_div: sig_binary;
    int_mod: "jet_jit_int_mod" => jet_jit_int_mod: sig_binary;
    int_rem: "jet_jit_int_rem" => jet_jit_int_rem: sig_binary;
    int_rem_euclid: "jet_jit_int_rem_euclid" => jet_jit_int_rem_euclid: sig_binary;
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
    int_int_pow: "jet_jit_int_int_pow" => jet_jit_int_int_pow: sig_binary;
    int_gcd: "jet_jit_int_gcd" => jet_jit_int_gcd: sig_binary;
    int_lcm: "jet_jit_int_lcm" => jet_jit_int_lcm: sig_binary;
    int_div_mod: "jet_jit_int_div_mod" => jet_jit_int_div_mod: sig_binary;
    int_div_rem_pair: "jet_jit_int_div_rem_pair" => jet_jit_int_div_rem_pair: sig_binary;
    decimal_from_str: "jet_jit_decimal_from_str" => jet_jit_decimal_from_str: sig_unary;
    decimal_add: "jet_jit_decimal_add" => jet_jit_decimal_add: sig_binary;
    decimal_sub: "jet_jit_decimal_sub" => jet_jit_decimal_sub: sig_binary;
    decimal_mul: "jet_jit_decimal_mul" => jet_jit_decimal_mul: sig_binary;
    decimal_equal: "jet_jit_decimal_equal" => jet_jit_decimal_equal: sig_compare;
    decimal_to_string: "jet_jit_decimal_to_string" => jet_jit_decimal_to_string: sig_unary;
    decimal_to_float: "jet_jit_decimal_to_float" => jet_jit_decimal_to_float: sig_unary_f64;
    fraction_new: "jet_jit_fraction_new" => jet_jit_fraction_new: sig_binary;
    fraction_from_parts: "jet_jit_fraction_from_parts" => jet_jit_fraction_from_parts: sig_binary;
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
    row_complex_from_parts: "jet_complex_from_parts" => jet_jit_complex_from_parts: sig_complex_parts;
    complex_add: "jet_jit_complex_add" => jet_jit_complex_add: sig_binary;
    row_complex_add: "jet_complex_add" => jet_jit_complex_add: sig_binary;
    complex_sub: "jet_jit_complex_sub" => jet_jit_complex_sub: sig_binary;
    row_complex_sub: "jet_complex_sub" => jet_jit_complex_sub: sig_binary;
    complex_mul: "jet_jit_complex_mul" => jet_jit_complex_mul: sig_binary;
    row_complex_mul: "jet_complex_mul" => jet_jit_complex_mul: sig_binary;
    complex_div: "jet_jit_complex_div" => jet_jit_complex_div: sig_binary;
    row_complex_div: "jet_complex_div" => jet_jit_complex_div: sig_binary;
    complex_abs: "jet_jit_complex_abs" => jet_jit_complex_abs: sig_unary_f64;
    row_complex_abs: "jet_complex_abs" => jet_jit_complex_abs: sig_unary_f64;
    complex_to_string: "jet_jit_complex_to_string" => jet_jit_complex_to_string: sig_unary;
    row_complex_to_string: "jet_complex_to_string" => jet_jit_complex_to_string: sig_unary;
}
