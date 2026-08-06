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
const JET_TRUNC_REM_OVERFLOW: &str = "attempt to calculate the remainder with overflow";

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

// D-MODSEM1=A: `%` is the floored modulo, the partner of `/%`. Its answer
// takes the divisor's sign, so `-7 % 2` is 1, and for every pair of whole
// numbers `a == b * (a /% b) + a % b`. Rust's `%` is the truncated remainder,
// which Jet spells `%%`, so the floored one is built here.
trait JetMod: Copy {
    fn jet_mod(self, rhs: Self, file: &str, line: u32) -> Self;
}
// Signed: the remainder comes back with the dividend's sign, so add the
// divisor whenever the two signs disagree.
macro_rules! jet_mod_signed {
    ($($t:ty),*) => { $(
        impl JetMod for $t {
            fn jet_mod(self, rhs: Self, file: &str, line: u32) -> Self {
                if rhs == 0 {
                    jet_panic(file, line, JET_FLOORDIV_ZERO);
                }
                let remainder = self.wrapping_rem(rhs);
                if remainder != 0 && (remainder < 0) != (rhs < 0) {
                    remainder.wrapping_add(rhs)
                } else {
                    remainder
                }
            }
        }
    )* };
}
// Unsigned: nothing is ever below zero, so the two remainders agree.
macro_rules! jet_mod_unsigned {
    ($($t:ty),*) => { $(
        impl JetMod for $t {
            fn jet_mod(self, rhs: Self, file: &str, line: u32) -> Self {
                if rhs == 0 {
                    jet_panic(file, line, JET_FLOORDIV_ZERO);
                }
                self % rhs
            }
        }
    )* };
}
jet_mod_signed!(i8, i16, i32, i64);
jet_mod_unsigned!(u8, u16, u32, u64);

// D-MODSEM1=A: `%%` is the truncated remainder, the partner of `/`. Rust's `%`
// already truncates, so this only adds the traps: a zero divisor, and the one
// signed pair whose remainder overflows.
trait JetTruncRem: Copy {
    fn jet_trunc_rem(self, rhs: Self, file: &str, line: u32) -> Self;
}
macro_rules! jet_trunc_rem_impl {
    ($($t:ty),*) => { $(
        impl JetTruncRem for $t {
            fn jet_trunc_rem(self, rhs: Self, file: &str, line: u32) -> Self {
                if rhs == 0 {
                    jet_panic(file, line, JET_FLOORDIV_ZERO);
                }
                match self.checked_rem(rhs) {
                    Some(remainder) => remainder,
                    None => jet_panic(file, line, JET_TRUNC_REM_OVERFLOW),
                }
            }
        }
    )* };
}
jet_trunc_rem_impl!(i8, i16, i32, i64, u8, u16, u32, u64);

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
