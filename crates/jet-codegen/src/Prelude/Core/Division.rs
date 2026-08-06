// D-FLOORDIV1=A: the one floor-division surface for `/%` and `/%=`.
//
// Every tier that runs generated Rust — the native build and the wasm module —
// includes this same file, so `/%` means the same thing everywhere. The only
// tier-local part is `jet_panic`, which reports the trap in the way that tier
// reports every other trap.
//
// `/%` rounds the answer down, toward negative infinity: `7 /% 2` is 3 and
// `-7 /% 2` is -4. Rust's `/` rounds toward zero instead, so a signed answer
// that came out one too high is corrected here. Dividing by zero traps, the
// same as `/` does. On floats `/%` is the ordinary division with the answer
// rounded down.
const JET_FLOORDIV_ZERO: &str = "divided by zero";
const JET_FLOORDIV_OVERFLOW: &str =
    "this division overflows the value's type (the result is outside its range)";

trait JetFloorDiv: Copy {
    fn jet_floordiv(self, rhs: Self, file: &str, line: u32) -> Self;
}
// Signed: the answer rounds toward zero, so subtract one whenever a non-zero
// remainder means the true answer sat below it.
macro_rules! jet_floordiv_signed {
    ($($t:ty),*) => { $(
        impl JetFloorDiv for $t {
            fn jet_floordiv(self, rhs: Self, file: &str, line: u32) -> Self {
                if rhs == 0 {
                    jet_panic(file, line, JET_FLOORDIV_ZERO);
                }
                let quotient = match self.checked_div(rhs) {
                    Some(quotient) => quotient,
                    None => jet_panic(file, line, JET_FLOORDIV_OVERFLOW),
                };
                let remainder = self.wrapping_rem(rhs);
                if remainder != 0 && (remainder < 0) != (rhs < 0) {
                    quotient - 1
                } else {
                    quotient
                }
            }
        }
    )* };
}
// Unsigned: nothing is ever below zero, so rounding down is plain division.
macro_rules! jet_floordiv_unsigned {
    ($($t:ty),*) => { $(
        impl JetFloorDiv for $t {
            fn jet_floordiv(self, rhs: Self, file: &str, line: u32) -> Self {
                if rhs == 0 {
                    jet_panic(file, line, JET_FLOORDIV_ZERO);
                }
                self / rhs
            }
        }
    )* };
}
jet_floordiv_signed!(i8, i16, i32, i64);
jet_floordiv_unsigned!(u8, u16, u32, u64);

trait JetFloorDivFloat: Copy {
    fn jet_floordiv(self, rhs: Self) -> Self;
}
impl JetFloorDivFloat for f64 {
    fn jet_floordiv(self, rhs: f64) -> f64 {
        (self / rhs).floor()
    }
}
impl JetFloorDivFloat for f32 {
    fn jet_floordiv(self, rhs: f32) -> f32 {
        (self / rhs).floor()
    }
}
