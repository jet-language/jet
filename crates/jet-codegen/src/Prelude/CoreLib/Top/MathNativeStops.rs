// AOT-only core.math native rows that stop through the native arithmetic
// report (`jet_fixed_error_stop` in Core.rs / PortableCore.rs). Kept out of
// MathRandomFns.rs because comptime and the JIT `include!` that file without
// the native stop; their twins are the comptime fold (plain_calls.rs) and
// `jet_jit_math_int_pow` (jet-jit CoreHost.rs), which stop with the same
// PowerNegative wording.

// D-EXPNEG1=A: a negative exponent stops with the shared arithmetic report.
fn jet_std_math_int_pow(base: i64, exp: i64) -> i64 {
    if exp < 0 {
        jet_fixed_error_stop(JetFixedArithmeticError::PowerNegative, "<core.math>", 0);
    }
    base.saturating_pow(exp as u32)
}
