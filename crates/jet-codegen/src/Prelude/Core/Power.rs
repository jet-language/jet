// D-EXPOP1=A / D-EXPSEM1=A: the one power surface for `^` and `^=`.
//
// Every tier that runs generated Rust — the native build and the wasm module —
// includes this same file, so a power means the same thing everywhere. The
// the Prelude's shared arithmetic stop carries the report code and wording.
//
// `^` on whole numbers is exact. A result outside the type's range traps the
// way a multiplication does. A written negative exponent is lowered to the
// exact Fraction carrier before this helper; a dynamic negative exponent has
// no whole-number answer and traps. `^` on floats is the ordinary
// floating-point power.
//
// The exponent arrives as an `i128` so an exponent of any integer width,
// signed or unsigned, reaches here losslessly.
trait JetPow: Copy {
    fn jet_pow(self, exponent: i128, file: &str, line: u32) -> Self;
    fn jet_wrapping_pow(self, exponent: i128, file: &str, line: u32) -> Self;
    fn jet_saturating_pow(self, exponent: i128, file: &str, line: u32) -> Self;
}

fn jet_power_fixed_value(result: JetFixedArithmeticResult, file: &str, line: u32) -> i64 {
    match result {
        JetFixedArithmeticResult::Value(value) => value,
        JetFixedArithmeticResult::Absent => jet_arithmetic_stop(
            file,
            line,
            "This checked fixed-width operation has no result",
        ),
        JetFixedArithmeticResult::Trap(error) => {
            let message = error.message();
            jet_arithmetic_stop(file, line, &message)
        }
    }
}

macro_rules! jet_pow_impl {
    ($($t:ty),*) => { $(
        impl JetPow for $t {
            fn jet_pow(self, exponent: i128, file: &str, line: u32) -> Self {
                jet_power_fixed_value(jet_fixed_arithmetic(
                    self as i64,
                    exponent,
                    JET_FIXED_OP_POW,
                    JET_FIXED_MODE_TRAP,
                    <$t>::MIN < 0,
                    <$t>::BITS as u8,
                    true,
                ), file, line) as $t
            }
            fn jet_wrapping_pow(self, exponent: i128, file: &str, line: u32) -> Self {
                jet_power_fixed_value(jet_fixed_arithmetic(
                    self as i64,
                    exponent,
                    JET_FIXED_OP_POW,
                    JET_FIXED_MODE_WRAPPING,
                    <$t>::MIN < 0,
                    <$t>::BITS as u8,
                    true,
                ), file, line) as $t
            }
            fn jet_saturating_pow(self, exponent: i128, file: &str, line: u32) -> Self {
                jet_power_fixed_value(jet_fixed_arithmetic(
                    self as i64,
                    exponent,
                    JET_FIXED_OP_POW,
                    JET_FIXED_MODE_SATURATING,
                    <$t>::MIN < 0,
                    <$t>::BITS as u8,
                    true,
                ), file, line) as $t
            }
        }
    )* };
}
jet_pow_impl!(i8, i16, i32, i64, u8, u16, u32, u64);

trait JetPowFloat: Copy {
    fn jet_pow(self, exponent: Self) -> Self;
}
impl JetPowFloat for f64 {
    fn jet_pow(self, exponent: f64) -> f64 {
        self.powf(exponent)
    }
}
impl JetPowFloat for f32 {
    fn jet_pow(self, exponent: f32) -> f32 {
        self.powf(exponent)
    }
}
