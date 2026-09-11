// D-FREESTAND-CORE1=A: the canonical no-OS Core closure.
//
// This source is deliberately heap-free.  It owns the checked carrier,
// arithmetic policy adapters, source-location stop boundary, and the target
// formatting macros.  Target I/O and failure are supplied by TargetAdapters;
// no host runtime, synchronization, filesystem, or UI code belongs here.

use core::fmt;
use core::fmt::Write;
use core::sync::atomic::{AtomicI32, Ordering};
use crate::RuntimeDiagnosticCore::{
    JetRuntimeDiagnosticRow, JetRuntimeStopContext, jet_runtime_stop_status,
    jet_write_runtime_stop,
};

pub type JetOutcome<T, E> = Result<T, E>;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct JetAbsent;

/// Errors crossing the portable entry boundary expose bytes without allocating.
pub trait JetPortableError {
    fn jet_error_bytes(&self) -> &[u8];

    fn jet_error_status(&self) -> i32 {
        1
    }
}

impl JetPortableError for &'static str {
    fn jet_error_bytes(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl JetPortableError for &'static [u8] {
    fn jet_error_bytes(&self) -> &[u8] {
        self
    }
}

pub static __JET_LAST_TARGET_STATUS: AtomicI32 = AtomicI32::new(0);


#[inline(always)]
fn jet_stop_rich<M: fmt::Display>(
    code: &'static str,
    context: JetRuntimeStopContext<'_>,
    message: &dyn fmt::Display,
    locals: Option<M>,
) -> ! {
    let row = jet_runtime_diagnostic_row(code);
    jet_target_failure_render(jet_runtime_stop_status(row), |out| {
        jet_write_runtime_stop(
            out, row, code, context, message,
            locals.as_ref().map(|value| value as &dyn fmt::Display),
        )
    })
}

#[inline(always)]
pub fn jet_runtime_boundary<F, T>(run: F) -> T
where
    F: FnOnce() -> T,
{
    run()
}
/// Report a top-level `Result` failure without constructing a hosted `String`.
/// The MIR emitter uses this carrier for no-OS entries, where formatting must
/// stay in `core` and the selected target report provider owns the bytes.
#[inline(never)]
pub fn jet_entry_error_exit<E: fmt::Debug>(error: E) -> ! {
    let row = jet_runtime_diagnostic_row("E3001");
    jet_target_failure_render(jet_runtime_stop_status(row), |out| {
        jet_write_runtime_stop(
            out,
            row,
            "E3001",
            JetRuntimeStopContext {
                file: "",
                line: 0,
                function: "",
                source_line: "",
                column: 1,
                caret_len: 1,
                expected_type: "",
            },
            &format_args!("{error:?}"),
            None,
        )
    })
}


#[inline(never)]
pub fn jet_runtime_stop(code: &'static str, file: &str, line: u32, message: &str) -> ! {
    jet_stop_rich(
        code,
        JetRuntimeStopContext {
            file, line, function: "", source_line: "", column: 1, caret_len: 1,
            expected_type: message.rsplit_once(" — expected ").map(|(_, expected)| expected).unwrap_or(message),
        },
        &message,
        Option::<fmt::Arguments<'_>>::None,
    )
}

#[inline(never)]
pub fn jet_panic(file: &str, line: u32, message: &str) -> ! {
    jet_panic_rich(file, line, "", "", 1, 1, message, None::<fmt::Arguments<'_>>)
}

#[inline(never)]
pub fn jet_arithmetic_stop(file: &str, line: u32, message: &str) -> ! {
    jet_runtime_stop("E3010", file, line, message)
}

#[inline(never)]
pub fn jet_todo_stop(file: &str, line: u32, expected_type: &str) -> ! {
    jet_runtime_stop(
        "E3011",
        file,
        line,
        expected_type,
    )
}

#[inline(never)]
pub fn jet_panic_rich<M: fmt::Display>(
    file: &str,
    line: u32,
    fn_name: &str,
    source_line: &str,
    col: u32,
    caret_len: u32,
    message: &str,
    locals: Option<M>,
) -> ! {
    jet_stop_rich(
        "E3001",
        JetRuntimeStopContext {
            file, line, function: fn_name, source_line, column: col, caret_len,
            expected_type: message,
        },
        &message,
        locals,
    )
}

#[inline(never)]
pub fn jet_require<M: fmt::Display>(
    condition: bool,
    message: &str,
    file: &str,
    line: u32,
    fn_name: &str,
    source_line: &str,
    col: u32,
    caret_len: u32,
    locals: Option<M>,
) {
    if !condition {
        jet_panic_rich(
            file,
            line,
            fn_name,
            source_line,
            col,
            caret_len,
            message,
            locals,
        );
    }
}

pub fn jet_require_eq<L: fmt::Display, R: fmt::Display, M: fmt::Display>(
    condition: bool,
    left_debug: L,
    right_debug: R,
    file: &str,
    line: u32,
    fn_name: &str,
    source_line: &str,
    col: u32,
    caret_len: u32,
    locals: Option<M>,
) {
    if !condition {
        jet_stop_rich(
            "E3001",
            JetRuntimeStopContext {
                file, line, function: fn_name, source_line, column: col, caret_len,
                expected_type: "",
            },
            &format_args!("expected: {right_debug}, got: {left_debug}"),
            locals,
        );
    }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        $crate::jet_target_print(core::format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! println {
    () => {{
        $crate::jet_target_println(core::format_args!(""));
    }};
    ($($arg:tt)*) => {{
        $crate::jet_target_println(core::format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! eprint {
    ($($arg:tt)*) => {{
        $crate::jet_target_report(core::format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! eprintln {
    () => {{
        $crate::jet_target_reportln(core::format_args!(""));
    }};
    ($($arg:tt)*) => {{
        $crate::jet_target_reportln(core::format_args!($($arg)*));
    }};
}
const JET_ARITHMETIC_CODE: &str = "E3010";
const JET_ARITHMETIC_ADD_OVERFLOW: &str =
    "This addition overflows the value's type (the result is outside its range)";
const JET_ARITHMETIC_SUB_OVERFLOW: &str =
    "This subtraction overflows the value's type (the result is outside its range)";
const JET_ARITHMETIC_MUL_OVERFLOW: &str =
    "This multiplication overflows the value's type (the result is outside its range)";
const JET_ARITHMETIC_DIVIDE_ZERO: &str = "divided by zero";
const JET_ARITHMETIC_DIVIDE_OVERFLOW: &str =
    "This division overflows the value's type (the result is outside its range)";
const JET_ARITHMETIC_POWER_NEGATIVE: &str =
    "A negative exponent has no whole-number result (make the base a Float to raise it to a negative power)";
const JET_ARITHMETIC_POWER_OVERFLOW: &str =
    "This power overflows the value's type (the result is outside its range)";
const JET_ARITHMETIC_ROTATE_NEGATIVE: &str = "A rotation count cannot be negative";
const JET_ARITHMETIC_REMAINDER_OVERFLOW: &str =
    "Attempt to calculate the remainder with overflow";

// D-INTBIG1/D-NUMOPS1: operation selection and overflow semantics remain in
// Core/FixedArithmetic.rs.  This file supplies only the no-allocation adapter.
trait JetArith: Copy {
    fn jet_add(self, rhs: Self, file: &str, line: u32) -> Self;
    fn jet_sub(self, rhs: Self, file: &str, line: u32) -> Self;
    fn jet_mul(self, rhs: Self, file: &str, line: u32) -> Self;
    fn jet_wrapping_add(self, rhs: Self) -> Self;
    fn jet_wrapping_sub(self, rhs: Self) -> Self;
    fn jet_wrapping_mul(self, rhs: Self) -> Self;
    fn jet_saturating_add(self, rhs: Self) -> Self;
    fn jet_saturating_sub(self, rhs: Self) -> Self;
    fn jet_saturating_mul(self, rhs: Self) -> Self;
    fn jet_div(self, rhs: Self, file: &str, line: u32) -> Self;
    fn jet_rem(self, rhs: Self, file: &str, line: u32) -> Self;
    fn jet_shl(self, bits: i128, file: &str, line: u32) -> Self;
    fn jet_shr(self, bits: i128, file: &str, line: u32) -> Self;
    fn jet_rotate_left(self, bits: i128, file: &str, line: u32) -> Self;
    fn jet_rotate_right(self, bits: i128, file: &str, line: u32) -> Self;
}

#[inline(always)]
fn jet_fixed_value(result: JetFixedArithmeticResult, file: &str, line: u32) -> i64 {
    match result {
        JetFixedArithmeticResult::Value(value) => value,
        JetFixedArithmeticResult::Absent => {
            jet_arithmetic_stop(file, line, "This checked fixed-width operation has no result")
        }
        JetFixedArithmeticResult::Trap(error) => jet_fixed_error_stop(error, file, line),
    }
}

#[inline(never)]
fn jet_fixed_error_stop(error: JetFixedArithmeticError, file: &str, line: u32) -> ! {
    jet_stop_rich(
        JET_ARITHMETIC_CODE,
        JetRuntimeStopContext {
            file,
            line,
            function: "",
            source_line: "",
            column: 1,
            caret_len: 1,
            expected_type: "",
        },
        &error,
        None::<fmt::Arguments<'_>>,
    )
}


macro_rules! jet_arith_impl {
    ($(($t:ty, $signed:expr)),*) => { $(
        impl JetArith for $t {
            #[inline(always)]
            fn jet_add(self, rhs: Self, file: &str, line: u32) -> Self {
                self.checked_add(rhs).unwrap_or_else(|| jet_arithmetic_stop(file, line, JET_ARITHMETIC_ADD_OVERFLOW))
            }
            #[inline(always)]
            fn jet_sub(self, rhs: Self, file: &str, line: u32) -> Self {
                self.checked_sub(rhs).unwrap_or_else(|| jet_arithmetic_stop(file, line, JET_ARITHMETIC_SUB_OVERFLOW))
            }
            #[inline(always)]
            fn jet_mul(self, rhs: Self, file: &str, line: u32) -> Self {
                self.checked_mul(rhs).unwrap_or_else(|| jet_arithmetic_stop(file, line, JET_ARITHMETIC_MUL_OVERFLOW))
            }
            #[inline(always)] fn jet_wrapping_add(self, rhs: Self) -> Self { self.wrapping_add(rhs) }
            #[inline(always)] fn jet_wrapping_sub(self, rhs: Self) -> Self { self.wrapping_sub(rhs) }
            #[inline(always)] fn jet_wrapping_mul(self, rhs: Self) -> Self { self.wrapping_mul(rhs) }
            #[inline(always)] fn jet_saturating_add(self, rhs: Self) -> Self { self.saturating_add(rhs) }
            #[inline(always)] fn jet_saturating_sub(self, rhs: Self) -> Self { self.saturating_sub(rhs) }
            #[inline(always)] fn jet_saturating_mul(self, rhs: Self) -> Self { self.saturating_mul(rhs) }
            #[inline(always)]
            fn jet_div(self, rhs: Self, file: &str, line: u32) -> Self {
                jet_fixed_value(jet_fixed_arithmetic(self as i64, rhs as i128, JET_FIXED_OP_DIV, JET_FIXED_MODE_TRAP, $signed, <$t>::BITS as u8, $signed), file, line) as $t
            }
            #[inline(always)]
            fn jet_rem(self, rhs: Self, file: &str, line: u32) -> Self {
                jet_fixed_value(jet_fixed_arithmetic(self as i64, rhs as i128, JET_FIXED_OP_REM, JET_FIXED_MODE_TRAP, $signed, <$t>::BITS as u8, $signed), file, line) as $t
            }
            #[inline(always)]
            fn jet_shl(self, bits: i128, file: &str, line: u32) -> Self {
                jet_fixed_value(jet_fixed_arithmetic(self as i64, bits, JET_FIXED_OP_SHL, JET_FIXED_MODE_TRAP, $signed, <$t>::BITS as u8, true), file, line) as $t
            }
            #[inline(always)]
            fn jet_shr(self, bits: i128, file: &str, line: u32) -> Self {
                jet_fixed_value(jet_fixed_arithmetic(self as i64, bits, JET_FIXED_OP_SHR, JET_FIXED_MODE_TRAP, $signed, <$t>::BITS as u8, true), file, line) as $t
            }
            #[inline(always)]
            fn jet_rotate_left(self, bits: i128, file: &str, line: u32) -> Self {
                jet_fixed_value(jet_fixed_arithmetic(self as i64, bits, JET_FIXED_OP_ROTATE_LEFT, JET_FIXED_MODE_TRAP, $signed, <$t>::BITS as u8, true), file, line) as $t
            }
            #[inline(always)]
            fn jet_rotate_right(self, bits: i128, file: &str, line: u32) -> Self {
                jet_fixed_value(jet_fixed_arithmetic(self as i64, bits, JET_FIXED_OP_ROTATE_RIGHT, JET_FIXED_MODE_TRAP, $signed, <$t>::BITS as u8, true), file, line) as $t
            }
        }
    )* };
}
jet_arith_impl!((i8, true), (i16, true), (i32, true), (i64, true), (u8, false), (u16, false), (u32, false), (u64, false));

#[inline(always)]
fn jet_fixed_option<T>(result: JetFixedArithmeticResult, map: impl FnOnce(i64) -> T) -> Option<T> {
    match result {
        JetFixedArithmeticResult::Value(value) => Some(map(value)),
        JetFixedArithmeticResult::Absent | JetFixedArithmeticResult::Trap(_) => None,
    }
}

// Concrete route items are the checked Prelude ABI used by MIR.  The
// operation, width, and overflow mode are encoded by each function item.
macro_rules! jet_fixed_route_kernels {
    ($t:ty, $signed:expr, $bits:expr;
     trap: {$($trap:ident),+};
     wrapping: {$($wrap:ident),+};
     saturating: {$($sat:ident),+};
     checked: {$($checked:ident),+};
     rotate: {$($rotate:ident),+}) => {
        jet_fixed_route_kernels!(@trap $t, $signed, $bits; $($trap),+);
        jet_fixed_route_kernels!(@wrapping $t, $signed, $bits; $($wrap),+);
        jet_fixed_route_kernels!(@saturating $t, $signed, $bits; $($sat),+);
        jet_fixed_route_kernels!(@checked $t, $signed, $bits; $($checked),+);
        jet_fixed_route_kernels!(@rotate $t, $signed, $bits; $($rotate),+);
    };
    (@trap $t:ty, $signed:expr, $bits:expr; $add:ident, $sub:ident, $mul:ident, $div:ident, $rem:ident, $floor:ident, $mod:ident, $pow:ident, $shl:ident, $shr:ident) => {
        #[inline(always)] pub(crate) fn $add(a:$t,b:$t,f:&str,l:u32)->$t { <$t as JetArith>::jet_add(a,b,f,l) }
        #[inline(always)] pub(crate) fn $sub(a:$t,b:$t,f:&str,l:u32)->$t { <$t as JetArith>::jet_sub(a,b,f,l) }
        #[inline(always)] pub(crate) fn $mul(a:$t,b:$t,f:&str,l:u32)->$t { <$t as JetArith>::jet_mul(a,b,f,l) }
        #[inline(always)] pub(crate) fn $div(a:$t,b:$t,f:&str,l:u32)->$t { <$t as JetArith>::jet_div(a,b,f,l) }
        #[inline(always)] pub(crate) fn $rem(a:$t,b:$t,f:&str,l:u32)->$t { <$t as JetArith>::jet_rem(a,b,f,l) }
        #[inline(always)] pub(crate) fn $floor(a:$t,b:$t,f:&str,l:u32)->$t { jet_fixed_value(jet_fixed_arithmetic(a as i64,b as i128,JET_FIXED_OP_FLOOR_DIV,JET_FIXED_MODE_TRAP,$signed,$bits,$signed),f,l) as $t }
        #[inline(always)] pub(crate) fn $mod(a:$t,b:$t,f:&str,l:u32)->$t { jet_fixed_value(jet_fixed_arithmetic(a as i64,b as i128,JET_FIXED_OP_MOD,JET_FIXED_MODE_TRAP,$signed,$bits,$signed),f,l) as $t }
        #[inline(always)] pub(crate) fn $pow(a:$t,b:$t,f:&str,l:u32)->$t { <$t as JetPow>::jet_pow(a,b as i128,f,l) }
        #[inline(always)] pub(crate) fn $shl(a:$t,b:$t,f:&str,l:u32)->$t { <$t as JetArith>::jet_shl(a,b as i128,f,l) }
        #[inline(always)] pub(crate) fn $shr(a:$t,b:$t,f:&str,l:u32)->$t { <$t as JetArith>::jet_shr(a,b as i128,f,l) }
    };
    (@wrapping $t:ty, $signed:expr, $bits:expr; $add:ident, $sub:ident, $mul:ident, $div:ident, $pow:ident) => {
        #[inline(always)] pub(crate) fn $add(a:$t,b:$t)->$t { <$t as JetArith>::jet_wrapping_add(a,b) }
        #[inline(always)] pub(crate) fn $sub(a:$t,b:$t)->$t { <$t as JetArith>::jet_wrapping_sub(a,b) }
        #[inline(always)] pub(crate) fn $mul(a:$t,b:$t)->$t { <$t as JetArith>::jet_wrapping_mul(a,b) }
        #[inline(always)] pub(crate) fn $div(a:$t,b:$t,f:&str,l:u32)->$t { jet_fixed_value(jet_fixed_arithmetic(a as i64,b as i128,JET_FIXED_OP_DIV,JET_FIXED_MODE_WRAPPING,$signed,$bits,$signed),f,l) as $t }
        #[inline(always)] pub(crate) fn $pow(a:$t,b:$t,f:&str,l:u32)->$t { <$t as JetPow>::jet_wrapping_pow(a,b as i128,f,l) }
    };
    (@saturating $t:ty, $signed:expr, $bits:expr; $add:ident, $sub:ident, $mul:ident, $div:ident, $pow:ident) => {
        #[inline(always)] pub(crate) fn $add(a:$t,b:$t)->$t { <$t as JetArith>::jet_saturating_add(a,b) }
        #[inline(always)] pub(crate) fn $sub(a:$t,b:$t)->$t { <$t as JetArith>::jet_saturating_sub(a,b) }
        #[inline(always)] pub(crate) fn $mul(a:$t,b:$t)->$t { <$t as JetArith>::jet_saturating_mul(a,b) }
        #[inline(always)] pub(crate) fn $div(a:$t,b:$t,f:&str,l:u32)->$t { jet_fixed_value(jet_fixed_arithmetic(a as i64,b as i128,JET_FIXED_OP_DIV,JET_FIXED_MODE_SATURATING,$signed,$bits,$signed),f,l) as $t }
        #[inline(always)] pub(crate) fn $pow(a:$t,b:$t,f:&str,l:u32)->$t { <$t as JetPow>::jet_saturating_pow(a,b as i128,f,l) }
    };
    (@checked $t:ty, $signed:expr, $bits:expr; $add:ident, $sub:ident, $mul:ident, $div:ident, $rem:ident, $pow:ident) => {
        #[inline(always)] pub(crate) fn $add(a:$t,b:$t)->Option<$t> { jet_fixed_option(jet_fixed_arithmetic(a as i64,b as i128,JET_FIXED_OP_ADD,JET_FIXED_MODE_CHECKED,$signed,$bits,$signed),|v|v as $t) }
        #[inline(always)] pub(crate) fn $sub(a:$t,b:$t)->Option<$t> { jet_fixed_option(jet_fixed_arithmetic(a as i64,b as i128,JET_FIXED_OP_SUB,JET_FIXED_MODE_CHECKED,$signed,$bits,$signed),|v|v as $t) }
        #[inline(always)] pub(crate) fn $mul(a:$t,b:$t)->Option<$t> { jet_fixed_option(jet_fixed_arithmetic(a as i64,b as i128,JET_FIXED_OP_MUL,JET_FIXED_MODE_CHECKED,$signed,$bits,$signed),|v|v as $t) }
        #[inline(always)] pub(crate) fn $div(a:$t,b:$t)->Option<$t> { jet_fixed_option(jet_fixed_arithmetic(a as i64,b as i128,JET_FIXED_OP_DIV,JET_FIXED_MODE_CHECKED,$signed,$bits,$signed),|v|v as $t) }
        #[inline(always)] pub(crate) fn $rem(a:$t,b:$t)->Option<$t> { jet_fixed_option(jet_fixed_arithmetic(a as i64,b as i128,JET_FIXED_OP_REM,JET_FIXED_MODE_CHECKED,$signed,$bits,$signed),|v|v as $t) }
        #[inline(always)] pub(crate) fn $pow(a:$t,b:$t)->Option<$t> { jet_fixed_option(jet_fixed_arithmetic(a as i64,b as i128,JET_FIXED_OP_POW,JET_FIXED_MODE_CHECKED,$signed,$bits,$signed),|v|v as $t) }
    };
    (@rotate $t:ty, $signed:expr, $bits:expr; $left:ident, $right:ident) => {
        #[inline(always)] pub(crate) fn $left(a:$t,b:$t,f:&str,l:u32)->$t { <$t as JetArith>::jet_rotate_left(a,b as i128,f,l) }
        #[inline(always)] pub(crate) fn $right(a:$t,b:$t,f:&str,l:u32)->$t { <$t as JetArith>::jet_rotate_right(a,b as i128,f,l) }
    };
}

jet_fixed_route_kernels!(i8,true,8; trap:{jet_i8_trap_add,jet_i8_trap_sub,jet_i8_trap_mul,jet_i8_trap_div,jet_i8_trap_rem,jet_i8_trap_floor_div,jet_i8_trap_mod,jet_i8_trap_pow,jet_i8_trap_shl,jet_i8_trap_shr}; wrapping:{jet_i8_wrapping_add,jet_i8_wrapping_sub,jet_i8_wrapping_mul,jet_i8_wrapping_div,jet_i8_wrapping_pow}; saturating:{jet_i8_saturating_add,jet_i8_saturating_sub,jet_i8_saturating_mul,jet_i8_saturating_div,jet_i8_saturating_pow}; checked:{jet_i8_checked_add,jet_i8_checked_sub,jet_i8_checked_mul,jet_i8_checked_div,jet_i8_checked_rem,jet_i8_checked_pow}; rotate:{jet_i8_rotate_left,jet_i8_rotate_right});
jet_fixed_route_kernels!(i16,true,16; trap:{jet_i16_trap_add,jet_i16_trap_sub,jet_i16_trap_mul,jet_i16_trap_div,jet_i16_trap_rem,jet_i16_trap_floor_div,jet_i16_trap_mod,jet_i16_trap_pow,jet_i16_trap_shl,jet_i16_trap_shr}; wrapping:{jet_i16_wrapping_add,jet_i16_wrapping_sub,jet_i16_wrapping_mul,jet_i16_wrapping_div,jet_i16_wrapping_pow}; saturating:{jet_i16_saturating_add,jet_i16_saturating_sub,jet_i16_saturating_mul,jet_i16_saturating_div,jet_i16_saturating_pow}; checked:{jet_i16_checked_add,jet_i16_checked_sub,jet_i16_checked_mul,jet_i16_checked_div,jet_i16_checked_rem,jet_i16_checked_pow}; rotate:{jet_i16_rotate_left,jet_i16_rotate_right});
jet_fixed_route_kernels!(i32,true,32; trap:{jet_i32_trap_add,jet_i32_trap_sub,jet_i32_trap_mul,jet_i32_trap_div,jet_i32_trap_rem,jet_i32_trap_floor_div,jet_i32_trap_mod,jet_i32_trap_pow,jet_i32_trap_shl,jet_i32_trap_shr}; wrapping:{jet_i32_wrapping_add,jet_i32_wrapping_sub,jet_i32_wrapping_mul,jet_i32_wrapping_div,jet_i32_wrapping_pow}; saturating:{jet_i32_saturating_add,jet_i32_saturating_sub,jet_i32_saturating_mul,jet_i32_saturating_div,jet_i32_saturating_pow}; checked:{jet_i32_checked_add,jet_i32_checked_sub,jet_i32_checked_mul,jet_i32_checked_div,jet_i32_checked_rem,jet_i32_checked_pow}; rotate:{jet_i32_rotate_left,jet_i32_rotate_right});
jet_fixed_route_kernels!(i64,true,64; trap:{jet_i64_trap_add,jet_i64_trap_sub,jet_i64_trap_mul,jet_i64_trap_div,jet_i64_trap_rem,jet_i64_trap_floor_div,jet_i64_trap_mod,jet_i64_trap_pow,jet_i64_trap_shl,jet_i64_trap_shr}; wrapping:{jet_i64_wrapping_add,jet_i64_wrapping_sub,jet_i64_wrapping_mul,jet_i64_wrapping_div,jet_i64_wrapping_pow}; saturating:{jet_i64_saturating_add,jet_i64_saturating_sub,jet_i64_saturating_mul,jet_i64_saturating_div,jet_i64_saturating_pow}; checked:{jet_i64_checked_add,jet_i64_checked_sub,jet_i64_checked_mul,jet_i64_checked_div,jet_i64_checked_rem,jet_i64_checked_pow}; rotate:{jet_i64_rotate_left,jet_i64_rotate_right});
jet_fixed_route_kernels!(u8,false,8; trap:{jet_u8_trap_add,jet_u8_trap_sub,jet_u8_trap_mul,jet_u8_trap_div,jet_u8_trap_rem,jet_u8_trap_floor_div,jet_u8_trap_mod,jet_u8_trap_pow,jet_u8_trap_shl,jet_u8_trap_shr}; wrapping:{jet_u8_wrapping_add,jet_u8_wrapping_sub,jet_u8_wrapping_mul,jet_u8_wrapping_div,jet_u8_wrapping_pow}; saturating:{jet_u8_saturating_add,jet_u8_saturating_sub,jet_u8_saturating_mul,jet_u8_saturating_div,jet_u8_saturating_pow}; checked:{jet_u8_checked_add,jet_u8_checked_sub,jet_u8_checked_mul,jet_u8_checked_div,jet_u8_checked_rem,jet_u8_checked_pow}; rotate:{jet_u8_rotate_left,jet_u8_rotate_right});
jet_fixed_route_kernels!(u16,false,16; trap:{jet_u16_trap_add,jet_u16_trap_sub,jet_u16_trap_mul,jet_u16_trap_div,jet_u16_trap_rem,jet_u16_trap_floor_div,jet_u16_trap_mod,jet_u16_trap_pow,jet_u16_trap_shl,jet_u16_trap_shr}; wrapping:{jet_u16_wrapping_add,jet_u16_wrapping_sub,jet_u16_wrapping_mul,jet_u16_wrapping_div,jet_u16_wrapping_pow}; saturating:{jet_u16_saturating_add,jet_u16_saturating_sub,jet_u16_saturating_mul,jet_u16_saturating_div,jet_u16_saturating_pow}; checked:{jet_u16_checked_add,jet_u16_checked_sub,jet_u16_checked_mul,jet_u16_checked_div,jet_u16_checked_rem,jet_u16_checked_pow}; rotate:{jet_u16_rotate_left,jet_u16_rotate_right});
jet_fixed_route_kernels!(u32,false,32; trap:{jet_u32_trap_add,jet_u32_trap_sub,jet_u32_trap_mul,jet_u32_trap_div,jet_u32_trap_rem,jet_u32_trap_floor_div,jet_u32_trap_mod,jet_u32_trap_pow,jet_u32_trap_shl,jet_u32_trap_shr}; wrapping:{jet_u32_wrapping_add,jet_u32_wrapping_sub,jet_u32_wrapping_mul,jet_u32_wrapping_div,jet_u32_wrapping_pow}; saturating:{jet_u32_saturating_add,jet_u32_saturating_sub,jet_u32_saturating_mul,jet_u32_saturating_div,jet_u32_saturating_pow}; checked:{jet_u32_checked_add,jet_u32_checked_sub,jet_u32_checked_mul,jet_u32_checked_div,jet_u32_checked_rem,jet_u32_checked_pow}; rotate:{jet_u32_rotate_left,jet_u32_rotate_right});
jet_fixed_route_kernels!(u64,false,64; trap:{jet_u64_trap_add,jet_u64_trap_sub,jet_u64_trap_mul,jet_u64_trap_div,jet_u64_trap_rem,jet_u64_trap_floor_div,jet_u64_trap_mod,jet_u64_trap_pow,jet_u64_trap_shl,jet_u64_trap_shr}; wrapping:{jet_u64_wrapping_add,jet_u64_wrapping_sub,jet_u64_wrapping_mul,jet_u64_wrapping_div,jet_u64_wrapping_pow}; saturating:{jet_u64_saturating_add,jet_u64_saturating_sub,jet_u64_saturating_mul,jet_u64_saturating_div,jet_u64_saturating_pow}; checked:{jet_u64_checked_add,jet_u64_checked_sub,jet_u64_checked_mul,jet_u64_checked_div,jet_u64_checked_rem,jet_u64_checked_pow}; rotate:{jet_u64_rotate_left,jet_u64_rotate_right});
