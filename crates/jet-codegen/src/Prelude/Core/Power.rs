// D-EXPOP1=A / D-EXPSEM1=A: the one power surface for `^` and `^=`.
//
// Every tier that runs generated Rust — the native build and the wasm module —
// includes this same file, so a power means the same thing everywhere. The
// only tier-local part is `jet_panic`, which reports the trap in the way that
// tier reports every other trap.
//
// `^` on whole numbers is exact. A result outside the type's range traps the
// way a multiplication does. A negative exponent has no whole-number answer,
// so it traps too; make the base a Float to raise it to a negative power.
// `^` on floats is the ordinary floating-point power.
//
// The exponent arrives as an `i128` so an exponent of any integer width,
// signed or unsigned, reaches here losslessly.
const JET_POW_NEGATIVE: &str =
    "a negative exponent has no whole-number result (make the base a Float to raise it to a negative power)";
const JET_POW_OVERFLOW: &str = "this power overflows the value's type (the result is outside its range)";

trait JetPow: Copy {
    fn jet_pow(self, exponent: i128, file: &str, line: u32) -> Self;
}
macro_rules! jet_pow_impl {
    ($($t:ty),*) => { $(
        impl JetPow for $t {
            fn jet_pow(self, exponent: i128, file: &str, line: u32) -> Self {
                if exponent < 0 {
                    jet_panic(file, line, JET_POW_NEGATIVE);
                }
                if exponent > u32::MAX as i128 {
                    jet_panic(file, line, JET_POW_OVERFLOW);
                }
                self.checked_pow(exponent as u32)
                    .unwrap_or_else(|| jet_panic(file, line, JET_POW_OVERFLOW))
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
