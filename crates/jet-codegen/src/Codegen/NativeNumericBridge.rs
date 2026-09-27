//! Native numeric adapter for checked MIR Prelude rows.
//!
//! This module is deliberately a child of `NativePreludeBridge`: the parent
//! owns the route-row ABI and the shared diagnostic constructor, while this
//! file owns only the closed numeric symbol set.  The adapter does not
//! implement a second arithmetic policy.  Fixed-width operations enter the
//! shared `Core/FixedArithmetic.rs` and numeric Prelude kernels.  Exact and
//! precise values marshal through those same source kernels; this layer only
//! validates row facts, marshals MIR values, and projects kernel
//! outcomes back onto the MIR failure carrier.

use super::{bridge_error, NativePreludeRoute};
use crate::Comptime::AmbientMirPreludeResult;
use crate::Diagnostics::{Diagnostic, Span};
use jet_foundation::AST::CtValue;
use jet_foundation::MIR::{MirRuntimeValue, MirType, MirTypeKind};
use jet_foundation::Numeric::{CtBigInt, CtDecimal, CtFraction};
use std::cmp::Ordering;

const BORROW_NONE_1: &[bool] = &[false];
const BORROW_NONE_2: &[bool] = &[false, false];
const BORROW_NONE_4: &[bool] = &[false, false, false, false];
const BORROW_ALL_1: &[bool] = &[true];
const BORROW_ALL_2: &[bool] = &[true, true];
#[allow(dead_code)]
mod exact_kernel {
    use jet_foundation::Numeric::CtBigInt as ExactBigInt;
    include!("../Prelude/Core/ExactNumeric.rs");
}
#[allow(dead_code)]
mod precise_kernel {
    use jet_foundation::Numeric::{
        CtBigInt as ExactBigInt, CtDecimal as ExactDecimal, CtFraction as ExactFraction,
    };
    include!("../Prelude/Core/PreciseNumeric.rs");
}
#[allow(dead_code)]
mod fixed_kernel {
    // Keep one operation/mode/bounds implementation for every execution tier.
    // The adapter below is intentionally a thin call into this source.
    include!("../Prelude/Core/FixedArithmetic.rs");
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FixedMode {
    Trap,
    Checked,
    Wrapping,
    Saturating,
}

impl FixedMode {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "trap" => Some(Self::Trap),
            "checked" => Some(Self::Checked),
            "wrapping" => Some(Self::Wrapping),
            "saturating" => Some(Self::Saturating),
            _ => None,
        }
    }

    fn kernel_mode(self) -> i64 {
        match self {
            Self::Trap => fixed_kernel::JET_FIXED_MODE_TRAP,
            Self::Checked => fixed_kernel::JET_FIXED_MODE_CHECKED,
            Self::Wrapping => fixed_kernel::JET_FIXED_MODE_WRAPPING,
            Self::Saturating => fixed_kernel::JET_FIXED_MODE_SATURATING,
        }
    }
}

#[derive(Clone, Copy)]
struct FixedRoute {
    signed: bool,
    bits: u8,
    member_mode: FixedMode,
    operation: i64,
}

fn exact_integer_member(symbol: &str) -> Option<&'static str> {
    match symbol {
        "jet_std::jet_int_add" => Some("add"),
        "jet_std::jet_int_sub" => Some("sub"),
        "jet_std::jet_int_mul" => Some("mul"),
        "jet_std::jet_int_div" => Some("div"),
        "jet_std::jet_int_rem" => Some("rem"),
        "jet_std::jet_int_div_euclid" => Some("div_euclid"),
        "jet_std::jet_int_rem_euclid" => Some("rem_euclid"),
        "jet_std::jet_int_floor_div" => Some("floor_div"),
        "jet_std::jet_int_mod" => Some("mod"),
        "jet_std::jet_int_pow" => Some("pow"),
        "jet_std::jet_int_shl" => Some("shl"),
        "jet_std::jet_int_shr" => Some("shr"),
        "jet_std::jet_int_bit_and" => Some("bit_and"),
        "jet_std::jet_int_bit_or" => Some("bit_or"),
        "jet_std::jet_int_bit_xor" => Some("bit_xor"),
        _ => None,
    }
}

fn exact_integer_unary_member(symbol: &str) -> Option<&'static str> {
    match symbol {
        "jet_std::jet_int_neg" => Some("neg"),
        "jet_std::jet_int_not" => Some("not"),
        _ => None,
    }
}

fn exact_integer_arity(member: &str) -> usize {
    if matches!(
        member,
        "div"
            | "rem"
            | "div_euclid"
            | "rem_euclid"
            | "floor_div"
            | "mod"
            | "pow"
            | "shl"
            | "shr"
    ) {
        4
    } else {
        2
    }
}

fn exact_integer_value(value: CtBigInt) -> MirRuntimeValue {
    // The resident exact-Int carrier reserves the outer tag bits.  Values in
    // the signed 63-bit interval remain scalar only when they fit the inline
    // payload; all other values cross as canonical decimal BigInt text.
    const MIN_SMALL_INT: i64 = -(1_i64 << 62);
    const MAX_SMALL_INT: i64 = (1_i64 << 62) - 1;
    match value.try_i64() {
        Some(value) if (MIN_SMALL_INT..=MAX_SMALL_INT).contains(&value) => {
            MirRuntimeValue::Int(value)
        }
        _ => MirRuntimeValue::BigInt(value.to_string_rep()),
    }
}

fn exact_integer(
    args: &[MirRuntimeValue],
    index: usize,
    span: Span,
) -> Result<CtBigInt, Diagnostic> {
    match args.get(index) {
        Some(MirRuntimeValue::Int(value)) => Ok(CtBigInt::from_int(*value)),
        Some(MirRuntimeValue::BigInt(value)) => CtBigInt::from_str(value)
            .map_err(|_| bridge_error(span, "native numeric route received an invalid exact integer")),
        _ => Err(bridge_error(
            span,
            format!("native numeric argument {index} requires an exact integer"),
        )),
    }
}

fn integer(
    args: &[MirRuntimeValue],
    index: usize,
    span: Span,
) -> Result<i64, Diagnostic> {
    match args.get(index) {
        Some(MirRuntimeValue::Int(value)) => Ok(*value),
        _ => Err(bridge_error(
            span,
            format!("native numeric argument {index} requires an integer"),
        )),
    }
}

fn float(
    args: &[MirRuntimeValue],
    index: usize,
    span: Span,
) -> Result<f64, Diagnostic> {
    match args.get(index) {
        Some(MirRuntimeValue::Float { value, .. }) => Ok(*value),
        _ => Err(bridge_error(
            span,
            format!("native numeric argument {index} requires a float"),
        )),
    }
}

fn boolean(
    args: &[MirRuntimeValue],
    index: usize,
    span: Span,
) -> Result<bool, Diagnostic> {
    match args.get(index) {
        Some(MirRuntimeValue::Bool(value)) => Ok(*value),
        _ => Err(bridge_error(
            span,
            format!("native numeric argument {index} requires a boolean"),
        )),
    }
}

fn text<'a>(
    args: &'a [MirRuntimeValue],
    index: usize,
    span: Span,
) -> Result<&'a str, Diagnostic> {
    match args.get(index) {
        Some(MirRuntimeValue::String(value)) => Ok(value),
        _ => Err(bridge_error(
            span,
            format!("native numeric argument {index} requires text"),
        )),
    }
}

fn source_line(
    args: &[MirRuntimeValue],
    file_index: usize,
    line_index: usize,
    span: Span,
) -> Result<(&str, u32), Diagnostic> {
    let file = text(args, file_index, span)?;
    let line = u32::try_from(integer(args, line_index, span)?).map_err(|_| {
        bridge_error(
            span,
            "native numeric source location line is not a nonnegative u32",
        )
    })?;
    Ok((file, line))
}

fn runtime_stop(file: &str, line: u32, message: &str, span: Span) -> Diagnostic {
    let report = jet_foundation::Outcome::jet_render_runtime_stop(
        "E3010", file, line, "", "", 1, 1, message, "",
    );
    Diagnostic::error(
        report.code,
        report.what,
        report.why,
        report.fix,
        Some(span),
    )
}

fn validate_signature(
    route: NativePreludeRoute<'_>,
    args: &[MirRuntimeValue],
    arity: usize,
    borrow_mask: &[bool],
    span: Span,
) -> Result<(), Diagnostic> {
    if route.arity != arity
        || route.max_arity != arity
        || args.len() != arity
        || route.borrow_mask != borrow_mask
    {
        return Err(bridge_error(
            span,
            format!(
                "native numeric route `{}.{}` has an invalid checked signature",
                route.module, route.member
            ),
        ));
    }
    Ok(())
}

fn validate_metadata(
    route: NativePreludeRoute<'_>,
    family_overflow: bool,
    infallible: bool,
    effect_name: Option<&str>,
    span: Span,
) -> Result<(), Diagnostic> {
    if route.family_overflow != family_overflow
        || !route.abi_value
        || route.infallible != infallible
        || route.effect_name != effect_name
    {
        return Err(bridge_error(
            span,
            format!(
                "native numeric route `{}.{}` has non-canonical checked metadata",
                route.module, route.member
            ),
        ));
    }
    Ok(())
}

fn require_exact_int_result(
    result_ty: Option<&MirType>,
    span: Span,
) -> Result<(), Diagnostic> {
    if result_ty.is_some_and(|ty| matches!(&ty.kind, MirTypeKind::Int)) {
        Ok(())
    } else {
        Err(bridge_error(
            span,
            "native exact integer route has a non-Int checked result type",
        ))
    }
}

fn fixed_symbol_parts(symbol: &str) -> Option<(&'static str, bool, u8, &str, &str)> {
    let (width, signed, bits, rest) = if let Some(rest) = symbol.strip_prefix("jet_i8_") {
        ("i8", true, 8, rest)
    } else if let Some(rest) = symbol.strip_prefix("jet_i16_") {
        ("i16", true, 16, rest)
    } else if let Some(rest) = symbol.strip_prefix("jet_i32_") {
        ("i32", true, 32, rest)
    } else if let Some(rest) = symbol.strip_prefix("jet_i64_") {
        ("i64", true, 64, rest)
    } else if let Some(rest) = symbol.strip_prefix("jet_u8_") {
        ("u8", false, 8, rest)
    } else if let Some(rest) = symbol.strip_prefix("jet_u16_") {
        ("u16", false, 16, rest)
    } else if let Some(rest) = symbol.strip_prefix("jet_u32_") {
        ("u32", false, 32, rest)
    } else if let Some(rest) = symbol.strip_prefix("jet_u64_") {
        ("u64", false, 64, rest)
    } else {
        return None;
    };
    let (symbol_mode, operation_name) = if let Some(rest) = rest.strip_prefix("trap_") {
        ("trap", rest)
    } else if let Some(rest) = rest.strip_prefix("checked_") {
        ("checked", rest)
    } else if let Some(rest) = rest.strip_prefix("wrapping_") {
        ("wrapping", rest)
    } else if let Some(rest) = rest.strip_prefix("saturating_") {
        ("saturating", rest)
    } else if rest == "rotate_left" {
        ("rotate_left", "rotate")
    } else if rest == "rotate_right" {
        ("rotate_right", "rotate")
    } else {
        return None;
    };
    Some((width, signed, bits, symbol_mode, operation_name))
}

fn fixed_operation(mode: &str, operation: &str) -> Option<i64> {
    Some(match (mode, operation) {
        ("rotate_left", "rotate") => fixed_kernel::JET_FIXED_OP_ROTATE_LEFT,
        ("rotate_right", "rotate") => fixed_kernel::JET_FIXED_OP_ROTATE_RIGHT,
        (_, "add") => fixed_kernel::JET_FIXED_OP_ADD,
        (_, "sub") => fixed_kernel::JET_FIXED_OP_SUB,
        (_, "mul") => fixed_kernel::JET_FIXED_OP_MUL,
        (_, "div") => fixed_kernel::JET_FIXED_OP_DIV,
        (_, "rem") => fixed_kernel::JET_FIXED_OP_REM,
        (_, "floor_div") => fixed_kernel::JET_FIXED_OP_FLOOR_DIV,
        (_, "mod") => fixed_kernel::JET_FIXED_OP_MOD,
        (_, "pow") => fixed_kernel::JET_FIXED_OP_POW,
        (_, "shl") => fixed_kernel::JET_FIXED_OP_SHL,
        (_, "shr") => fixed_kernel::JET_FIXED_OP_SHR,
        _ => return None,
    })
}

fn fixed_member_parts(member: &str) -> Option<(&str, &str, &str)> {
    let mut parts = member.split('.');
    let width = parts.next()?;
    let mode = parts.next()?;
    let operation = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    Some((width, mode, operation))
}

fn fixed_operation_allowed(mode: &str, operation: &str) -> bool {
    match mode {
        "trap" => matches!(
            operation,
            "add"
                | "sub"
                | "mul"
                | "div"
                | "rem"
                | "floor_div"
                | "mod"
                | "pow"
                | "shl"
                | "shr"
        ),
        "checked" => matches!(operation, "add" | "sub" | "mul" | "div" | "rem" | "pow"),
        "checked_policy" => matches!(operation, "add" | "sub" | "mul" | "div" | "rem" | "pow"),
        "wrapping" | "saturating" => {
            matches!(operation, "add" | "sub" | "mul" | "div" | "pow")
        }
        "rotate_left" | "rotate_right" => operation == "rotate",
        _ => false,
    }
}

fn fixed_route(
    route: NativePreludeRoute<'_>,
    symbol: &str,
    args: &[MirRuntimeValue],
    result_ty: Option<&MirType>,
    span: Span,
) -> Result<FixedRoute, Diagnostic> {
    if route.module != "core.numeric" || route.symbol != symbol {
        return Err(bridge_error(
            span,
            "native fixed route has non-canonical identity",
        ));
    }
    let Some((symbol_width, signed, bits, symbol_mode, symbol_operation)) =
        fixed_symbol_parts(symbol)
    else {
        return Err(bridge_error(span, "native fixed route has an unknown kernel symbol"));
    };
    let Some(operation) = fixed_operation(symbol_mode, symbol_operation) else {
        return Err(bridge_error(
            span,
            "native fixed route has an unsupported kernel operation",
        ));
    };
    let Some((member_width, member_mode_name, member_operation)) =
        fixed_member_parts(route.member)
    else {
        return Err(bridge_error(
            span,
            "native fixed route has a malformed checked member",
        ));
    };
    let member_mode = match member_mode_name {
        "checked_policy" | "rotate_left" | "rotate_right" => {
            // `checked_policy` is a source-level trap policy and rotations use
            // the trap result path in the shared fixed kernel.
            FixedMode::Trap
        }
        _ => FixedMode::from_name(member_mode_name).ok_or_else(|| {
            bridge_error(
                span,
                "native fixed route has an unsupported overflow mode",
            )
        })?,
    };
    if member_width != symbol_width
        || member_operation != symbol_operation
        || !fixed_operation_allowed(member_mode_name, member_operation)
    {
        return Err(bridge_error(
            span,
            "native fixed route member and symbol disagree",
        ));
    }
    if member_mode_name == "checked_policy" && symbol_mode != "trap" {
        return Err(bridge_error(
            span,
            "native checked-policy route does not use the trap kernel",
        ));
    }
    if member_mode_name != "checked_policy" && symbol_mode != member_mode_name {
        return Err(bridge_error(
            span,
            "native fixed route overflow mode disagrees with its kernel symbol",
        ));
    }
    let expected_arity = if matches!(
        member_mode_name,
        "trap" | "checked_policy" | "rotate_left" | "rotate_right"
    ) || (matches!(member_mode, FixedMode::Wrapping | FixedMode::Saturating)
        && matches!(member_operation, "div" | "pow"))
    {
        4
    } else {
        2
    };
    let expected_effect = (expected_arity == 4).then_some("Panic");
    validate_metadata(route, true, member_mode != FixedMode::Checked, expected_effect, span)?;
    let borrow_mask = match expected_arity {
        2 => BORROW_NONE_2,
        4 => BORROW_NONE_4,
        _ => unreachable!(),
    };
    validate_signature(route, args, expected_arity, borrow_mask, span)?;
    let success_ty = if member_mode == FixedMode::Checked {
        let result_ty = result_ty.ok_or_else(|| {
            bridge_error(span, "checked fixed route has no checked Option result type")
        })?;
        result_ty.option_inner().ok_or_else(|| {
            bridge_error(span, "checked fixed route does not return an Option")
        })?
    } else {
        result_ty.ok_or_else(|| bridge_error(span, "fixed route has no checked result type"))?
    };
    if success_ty.fixed_int() != Some((signed, bits)) {
        return Err(bridge_error(
            span,
            "native fixed route result width disagrees with its checked type",
        ));
    }
    Ok(FixedRoute {
        signed,
        bits,
        member_mode,
        operation,
    })
}

fn raw_integer(
    args: &[MirRuntimeValue],
    index: usize,
    span: Span,
) -> Result<u64, Diagnostic> {
    Ok(exact_integer(args, index, span)?.wrapping_u64())
}

fn fixed_left(value: &CtBigInt, signed: bool, span: Span) -> Result<i64, Diagnostic> {
    if signed {
        i64::try_from(value.try_i128().ok_or_else(|| {
            bridge_error(span, "native fixed signed operand exceeds its carrier width")
        })?)
        .map_err(|_| bridge_error(span, "native fixed signed operand exceeds its carrier width"))
    } else {
        Ok(value.wrapping_u64() as i64)
    }
}

fn fixed_right(value: &CtBigInt, signed: bool, span: Span) -> Result<i128, Diagnostic> {
    if signed {
        value
            .try_i128()
            .ok_or_else(|| bridge_error(span, "native fixed signed operand exceeds its carrier width"))
    } else {
        Ok(value.wrapping_u64() as i128)
    }
}

fn fixed_result_value(value: i64) -> MirRuntimeValue {
    // The fixed kernel returns the resident raw word.  In particular, U64
    // values in the high half intentionally remain a negative MIR `Int` word.
    MirRuntimeValue::Int(value)
}

fn dispatch_fixed(
    route: NativePreludeRoute<'_>,
    symbol: &str,
    args: Vec<MirRuntimeValue>,
    result_ty: Option<MirType>,
    span: Span,
) -> Result<AmbientMirPreludeResult, Diagnostic> {
    let fixed = fixed_route(route, symbol, &args, result_ty.as_ref(), span)?;
    let left = exact_integer(&args, 0, span)?;
    let right = exact_integer(&args, 1, span)?;
    let left = fixed_left(&left, fixed.signed, span)?;
    let right = fixed_right(&right, fixed.signed, span)?;
    let result = fixed_kernel::jet_fixed_arithmetic(
        left,
        right,
        fixed.operation,
        fixed.member_mode.kernel_mode(),
        fixed.signed,
        fixed.bits,
        fixed.signed,
    );
    let value = match result {
        fixed_kernel::JetFixedArithmeticResult::Value(value) => {
            let value = fixed_result_value(value);
            if fixed.member_mode == FixedMode::Checked {
                MirRuntimeValue::Present(Box::new(value))
            } else {
                value
            }
        }
        fixed_kernel::JetFixedArithmeticResult::Absent => {
            let element = result_ty
                .as_ref()
                .and_then(MirType::option_inner)
                .ok_or_else(|| {
                    bridge_error(span, "checked fixed kernel returned absent without an Option type")
                })?
                .clone();
            MirRuntimeValue::Absent { element }
        }
        fixed_kernel::JetFixedArithmeticResult::Trap(error) => {
            let (file, line) = source_line(&args, 2, 3, span)?;
            return Err(runtime_stop(file, line, &error.to_string(), span));
        }
    };
    Ok(AmbientMirPreludeResult::Value(value))
}

fn dispatch_exact_binary(
    route: NativePreludeRoute<'_>,
    member: &str,
    args: Vec<MirRuntimeValue>,
    result_ty: Option<MirType>,
    span: Span,
) -> Result<AmbientMirPreludeResult, Diagnostic> {
    let arity = exact_integer_arity(member);
    validate_metadata(route, true, true, (arity == 4).then_some("Panic"), span)?;
    let borrow_mask = if arity == 2 {
        BORROW_NONE_2
    } else {
        BORROW_NONE_4
    };
    validate_signature(route, &args, arity, borrow_mask, span)?;
    require_exact_int_result(result_ty.as_ref(), span)?;
    let left = exact_integer(&args, 0, span)?;
    let right = exact_integer(&args, 1, span)?;
    let value = match member {
        "add" => Some(exact_kernel::add(&left, &right)),
        "sub" => Some(exact_kernel::sub(&left, &right)),
        "mul" => Some(exact_kernel::mul(&left, &right)),
        "bit_and" => Some(exact_kernel::bit_and(&left, &right)),
        "bit_or" => Some(exact_kernel::bit_or(&left, &right)),
        "bit_xor" => Some(exact_kernel::bit_xor(&left, &right)),
        "div" => exact_kernel::div_rem(&left, &right).map(|(quotient, _)| quotient),
        "rem" => exact_kernel::div_rem(&left, &right).map(|(_, remainder)| remainder),
        "div_euclid" => exact_kernel::div_rem_euclid(&left, &right)
            .map(|(quotient, _)| quotient),
        "rem_euclid" => exact_kernel::div_rem_euclid(&left, &right)
            .map(|(_, remainder)| remainder),
        "floor_div" => exact_kernel::floor_div(&left, &right),
        "mod" => exact_kernel::modulo(&left, &right),
        "pow" => exact_kernel::pow(&left, &right),
        "shl" => exact_kernel::shl(&left, &right),
        "shr" => exact_kernel::shr(&left, &right),
        _ => None,
    };
    let value = match value {
        Some(value) => exact_integer_value(value),
        None => {
            let (file, line) = source_line(&args, 2, 3, span)?;
            return Err(runtime_stop(
                file,
                line,
                exact_kernel::failure_message(member),
                span,
            ));
        }
    };
    Ok(AmbientMirPreludeResult::Value(value))
}

fn dispatch_exact_unary(
    route: NativePreludeRoute<'_>,
    member: &str,
    args: Vec<MirRuntimeValue>,
    result_ty: Option<MirType>,
    span: Span,
) -> Result<AmbientMirPreludeResult, Diagnostic> {
    validate_metadata(route, true, true, None, span)?;
    validate_signature(route, &args, 1, &[false], span)?;
    require_exact_int_result(result_ty.as_ref(), span)?;
    let value = exact_integer(&args, 0, span)?;
    let value = match member {
        "neg" => exact_kernel::neg(&value),
        "not" => exact_kernel::bit_not(&value),
        _ => {
            return Err(bridge_error(
                span,
                "native exact integer unary route has no canonical kernel",
            ));
        }
    };
    Ok(AmbientMirPreludeResult::Value(exact_integer_value(value)))
}

fn is_nominal(ty: &MirType, expected: &str) -> bool {
    ty.identity.is_some()
        && matches!(
            &ty.kind,
            MirTypeKind::Apply { name, args }
                if name.name == expected && args.is_empty()
        )
}

fn require_bool_result(ty: &MirType, span: Span) -> Result<(), Diagnostic> {
    if matches!(&ty.kind, MirTypeKind::Bool) {
        Ok(())
    } else {
        Err(bridge_error(span, "native comparison route has a non-Bool result type"))
    }
}

fn ordering_value(ordering: Ordering) -> MirRuntimeValue {
    let variant = match ordering {
        Ordering::Less => "Less",
        Ordering::Equal => "Equal",
        Ordering::Greater => "Greater",
    };
    MirRuntimeValue::Enum {
        type_name: "Ordering".to_string(),
        variant: variant.to_string(),
        args: Vec::new(),
    }
}

fn dispatch_compare(
    route: NativePreludeRoute<'_>,
    args: Vec<MirRuntimeValue>,
    result_ty: Option<MirType>,
    span: Span,
) -> Result<AmbientMirPreludeResult, Diagnostic> {
    if route.module != "core.compare" || route.symbol != "jet_int_compare" {
        return Err(bridge_error(span, "native comparison route has non-canonical identity"));
    }
    if !matches!(route.member, "eq" | "ne" | "lt" | "gt" | "le" | "ge" | "compare") {
        return Err(bridge_error(span, "native comparison route has an unknown member"));
    }
    validate_metadata(route, false, true, None, span)?;
    validate_signature(route, &args, 2, &[true, true], span)?;
    let result_ty = result_ty
        .as_ref()
        .ok_or_else(|| bridge_error(span, "native comparison route has no checked result type"))?;
    let left = exact_integer(&args, 0, span)?;
    let right = exact_integer(&args, 1, span)?;
    let ordering = exact_kernel::compare(&left, &right);
    let value = if route.member == "compare" {
        if !is_nominal(result_ty, "Ordering") {
            return Err(bridge_error(
                span,
                "native comparison route has a non-Ordering result type",
            ));
        }
        ordering_value(ordering)
    } else {
        require_bool_result(result_ty, span)?;
        let value = match route.member {
            "eq" => ordering == Ordering::Equal,
            "ne" => ordering != Ordering::Equal,
            "lt" => ordering == Ordering::Less,
            "gt" => ordering == Ordering::Greater,
            "le" => ordering != Ordering::Greater,
            "ge" => ordering != Ordering::Less,
            _ => unreachable!(),
        };
        MirRuntimeValue::Bool(value)
    };
    Ok(AmbientMirPreludeResult::Value(value))
}

fn precise_symbol(symbol: &str) -> Option<(&'static str, &str)> {
    if let Some(func) = symbol.strip_prefix("jet_decimal_") {
        return Some(("Decimal", func));
    }
    if let Some(func) = symbol.strip_prefix("jet_fraction_") {
        return Some(("Fraction", func));
    }
    None
}

fn is_conversion_symbol(symbol: &str) -> bool {
    matches!(
        symbol,
        "jet_numeric_float_to_int"
            | "jet_numeric_float_narrow"
            | "jet_numeric_try_from_fixed"
            | "jet_std::jet_int_try_from_checked"
            | "jet_std::jet_int_checked_fixed"
            | "jet_std::jet_int_checked_widen"
            | "jet_numeric_checked_widen_at"
    )
}

/// Fast ownership gate used by the parent bridge before it moves argument
/// carriers into this numeric adapter.
pub(super) fn owns_symbol(symbol: &str) -> bool {
    exact_integer_member(symbol).is_some()
        || exact_integer_unary_member(symbol).is_some()
        || symbol == "jet_int_compare"
        || fixed_symbol_parts(symbol).is_some()
        || precise_symbol(symbol).is_some()
        || is_conversion_symbol(symbol)
}

fn precise_expected(type_name: &str, function: &str) -> Option<(&'static str, usize)> {
    match (type_name, function) {
        ("Decimal", "from_str") => Some(("Decimal", 1)),
        ("Decimal", "from_int" | "from_float" | "from_fraction") => Some(("Decimal", 1)),
        ("Decimal", "add" | "sub" | "mul" | "round" | "floor" | "ceil") => Some((
            "Decimal",
            if matches!(function, "add" | "sub" | "mul") { 2 } else { 1 },
        )),
        ("Decimal", "div" | "to_fraction") => Some((
            "Fraction",
            if function == "div" { 2 } else { 1 },
        )),
        ("Decimal", "equal") => Some(("Bool", 2)),
        ("Decimal", "compare") => Some(("Ordering", 2)),
        ("Decimal", "to_string") => Some(("String", 1)),
        ("Decimal", "to_float") => Some(("Float", 1)),
        ("Decimal", "to_int") => Some(("Int", 1)),
        ("Fraction", "new") => Some(("Fraction", 2)),
        ("Fraction", "from_parts" | "from_int" | "from_float" | "from_decimal") => Some((
            "Fraction",
            if function == "from_parts" { 2 } else { 1 },
        )),
        ("Fraction", "add" | "sub" | "mul" | "div") => Some(("Fraction", 2)),
        ("Fraction", "equal") => Some(("Bool", 2)),
        ("Fraction", "numerator" | "denominator" | "to_int") => Some(("Int", 1)),
        ("Fraction", "to_string") => Some(("String", 1)),
        ("Fraction", "to_float") => Some(("Float", 1)),
        ("Fraction", "is_zero") => Some(("Bool", 1)),
        ("Fraction", "to_decimal") => Some(("Decimal", 1)),
        _ => None,
    }
}

fn success_type<'a>(result_ty: &'a MirType, span: Span) -> Result<&'a MirType, Diagnostic> {
    if let Some(inner) = result_ty.option_inner() {
        return Ok(inner);
    }
    if let Some((ok, _)) = result_ty.result_parts() {
        return Ok(ok);
    }
    Ok(result_ty)
}

fn precise_type_matches(ty: &MirType, expected: &str) -> bool {
    match expected {
        "Decimal" | "Fraction" | "Ordering" => is_nominal(ty, expected),
        "Bool" => matches!(&ty.kind, MirTypeKind::Bool),
        "Float" => matches!(&ty.kind, MirTypeKind::Float | MirTypeKind::Float32),
        "String" => matches!(&ty.kind, MirTypeKind::String),
        "Int" => matches!(&ty.kind, MirTypeKind::Int),
        _ => false,
    }
}

fn precise_success(
    route: NativePreludeRoute<'_>,
    result_ty: &MirType,
    value: MirRuntimeValue,
    expected: &str,
    span: Span,
) -> Result<MirRuntimeValue, Diagnostic> {
    let inner = success_type(result_ty, span)?;
    if !precise_type_matches(inner, expected) {
        return Err(bridge_error(
            span,
            "native precise route result carrier disagrees with its checked type",
        ));
    }
    if route.infallible {
        if result_ty.option_inner().is_some() || result_ty.result_parts().is_some() {
            return Err(bridge_error(
                span,
                "infallible precise route unexpectedly has a failure carrier",
            ));
        }
        Ok(value)
    } else if result_ty.option_inner().is_some() || result_ty.result_parts().is_some() {
        Ok(MirRuntimeValue::Present(Box::new(value)))
    } else {
        Err(bridge_error(
            span,
            "fallible precise route has no MIR failure carrier",
        ))
    }
}

fn precise_failure(
    route: NativePreludeRoute<'_>,
    result_ty: &MirType,
    message: &str,
    span: Span,
) -> Result<MirRuntimeValue, Diagnostic> {
    if route.infallible {
        return Err(bridge_error(span, message));
    }
    if let Some(inner) = result_ty.option_inner() {
        return Ok(MirRuntimeValue::Absent {
            element: inner.clone(),
        });
    }
    if result_ty.result_parts().is_some() {
        return Ok(MirRuntimeValue::FailedTold(Box::new(
            MirRuntimeValue::String(message.to_string()),
        )));
    }
    Err(bridge_error(
        span,
        "fallible precise route has no MIR failure carrier",
    ))
}


fn decimal_input(value: &MirRuntimeValue, span: Span) -> Result<CtDecimal, Diagnostic> {
    let MirRuntimeValue::Struct { type_name, fields } = value else {
        return Err(bridge_error(span, "native Decimal route requires a Decimal carrier"));
    };
    if type_name != "Decimal" {
        return Err(bridge_error(span, "native Decimal route received a non-canonical type"));
    }
    let mut negative = None;
    let mut digits = None;
    let mut scale = None;
    for (name, value) in fields {
        match (name.as_str(), value) {
            ("negative", MirRuntimeValue::Bool(value)) if negative.is_none() => {
                negative = Some(*value)
            }
            ("digits", MirRuntimeValue::String(value)) if digits.is_none() => {
                digits = Some(value.clone())
            }
            ("scale", MirRuntimeValue::Int(value)) if scale.is_none() => {
                scale = Some(u32::try_from(*value).map_err(|_| {
                    bridge_error(span, "native Decimal scale is not a nonnegative u32")
                })?)
            }
            ("negative" | "digits" | "scale", _) => {
                return Err(bridge_error(span, "native Decimal carrier has a duplicate or malformed field"));
            }
            _ => return Err(bridge_error(span, "native Decimal carrier has an unknown field")),
        }
    }
    let negative = negative.ok_or_else(|| bridge_error(span, "native Decimal carrier has no sign"))?;
    let digits = digits.ok_or_else(|| bridge_error(span, "native Decimal carrier has no digits"))?;
    let scale = scale.ok_or_else(|| bridge_error(span, "native Decimal carrier has no scale"))?;
    CtDecimal::from_value(&CtValue::Struct {
        type_name: "Decimal".to_string(),
        fields: vec![
            ("negative".to_string(), CtValue::Bool(negative)),
            ("digits".to_string(), CtValue::Str(digits)),
            ("scale".to_string(), CtValue::Int(i64::from(scale))),
        ],
    })
    .map_err(|message| bridge_error(span, message))
}

fn fraction_input(value: &MirRuntimeValue, span: Span) -> Result<CtFraction, Diagnostic> {
    let MirRuntimeValue::Struct { type_name, fields } = value else {
        return Err(bridge_error(span, "native Fraction route requires a Fraction carrier"));
    };
    if type_name != "Fraction" {
        return Err(bridge_error(span, "native Fraction route received a non-canonical type"));
    }
    let mut numerator = None;
    let mut denominator = None;
    for (name, value) in fields {
        match name.as_str() {
            "numerator" if numerator.is_none() => numerator = Some(exact_integer_value_ref(value, span)?),
            "denominator" if denominator.is_none() => denominator = Some(exact_integer_value_ref(value, span)?),
            "numerator" | "denominator" => {
                return Err(bridge_error(span, "native Fraction carrier has a duplicate field"));
            }
            _ => return Err(bridge_error(span, "native Fraction carrier has an unknown field")),
        }
    }
    CtFraction::from_bigints(
        numerator.ok_or_else(|| bridge_error(span, "native Fraction carrier has no numerator"))?,
        denominator.ok_or_else(|| bridge_error(span, "native Fraction carrier has no denominator"))?,
    )
    .ok_or_else(|| bridge_error(span, "native Fraction carrier has a zero denominator"))
}

fn exact_integer_value_ref(value: &MirRuntimeValue, span: Span) -> Result<CtBigInt, Diagnostic> {
    match value {
        MirRuntimeValue::Int(value) => Ok(CtBigInt::from_int(*value)),
        MirRuntimeValue::BigInt(value) => CtBigInt::from_str(value)
            .map_err(|_| bridge_error(span, "native precise route received an invalid exact integer")),
        _ => Err(bridge_error(span, "native precise route requires an exact integer field")),
    }
}

fn decimal_value(value: CtDecimal, span: Span) -> Result<MirRuntimeValue, Diagnostic> {
    crate::Comptime::MirBridge::ct_to_mir_value(value.to_value(), span)
}

fn fraction_value(value: CtFraction, span: Span) -> Result<MirRuntimeValue, Diagnostic> {
    crate::Comptime::MirBridge::ct_to_mir_value(value.to_value(), span)
}

fn precise_value(
    type_name: &str,
    function: &str,
    args: &[MirRuntimeValue],
    span: Span,
) -> Result<Result<(MirRuntimeValue, &'static str), String>, Diagnostic> {
    let result = match (type_name, function) {
        ("Decimal", "from_str") => {
            let text = text(args, 0, span)?;
            match precise_kernel::decimal_from_str(text) {
                Ok(value) => decimal_value(value, span).map(|value| Ok((value, "Decimal"))),
                Err(()) => Ok(Err(
                    precise_kernel::failure_message(type_name, function).to_string(),
                )),
            }
        }
        ("Decimal", "from_int") => {
            let value = exact_integer(args, 0, span)?;
            decimal_value(precise_kernel::decimal_from_int(value), span)
                .map(|value| Ok((value, "Decimal")))
        }
        ("Decimal", "from_float") => match precise_kernel::decimal_from_float(
            float(args, 0, span)?,
        ) {
            Some(value) => decimal_value(value, span).map(|value| Ok((value, "Decimal"))),
            None => Ok(Err(
                precise_kernel::failure_message(type_name, function).to_string(),
            )),
        },
        ("Decimal", "from_fraction") => {
            let value = fraction_input(&args[0], span)?;
            match precise_kernel::decimal_from_fraction(&value) {
                Some(value) => decimal_value(value, span).map(|value| Ok((value, "Decimal"))),
                None => Ok(Err(
                    precise_kernel::failure_message(type_name, function).to_string(),
                )),
            }
        }
        ("Decimal", "add" | "sub" | "mul") => {
            let left = decimal_input(&args[0], span)?;
            let right = decimal_input(&args[1], span)?;
            let value = match function {
                "add" => precise_kernel::decimal_add(&left, &right),
                "sub" => precise_kernel::decimal_sub(&left, &right),
                "mul" => precise_kernel::decimal_mul(&left, &right),
                _ => unreachable!(),
            };
            decimal_value(value, span).map(|value| Ok((value, "Decimal")))
        }
        ("Decimal", "div") => {
            let left = decimal_input(&args[0], span)?;
            let right = decimal_input(&args[1], span)?;
            match precise_kernel::decimal_div(&left, &right) {
                Some(value) => fraction_value(value, span).map(|value| Ok((value, "Fraction"))),
                None => Ok(Err(
                    precise_kernel::failure_message(type_name, function).to_string(),
                )),
            }
        }
        ("Decimal", "round" | "floor" | "ceil") => {
            let value = decimal_input(&args[0], span)?;
            let value = match function {
                "round" => precise_kernel::decimal_round(&value),
                "floor" => precise_kernel::decimal_floor(&value),
                "ceil" => precise_kernel::decimal_ceil(&value),
                _ => unreachable!(),
            };
            decimal_value(value, span).map(|value| Ok((value, "Decimal")))
        }
        ("Decimal", "equal") => {
            let left = decimal_input(&args[0], span)?;
            let right = decimal_input(&args[1], span)?;
            Ok(Ok((
                MirRuntimeValue::Bool(precise_kernel::decimal_equal(&left, &right)),
                "Bool",
            )))
        }
        ("Decimal", "compare") => {
            let left = decimal_input(&args[0], span)?;
            let right = decimal_input(&args[1], span)?;
            Ok(Ok((
                ordering_value(precise_kernel::decimal_compare(&left, &right)),
                "Ordering",
            )))
        }
        ("Decimal", "to_string") => {
            let value = decimal_input(&args[0], span)?;
            Ok(Ok((
                MirRuntimeValue::String(precise_kernel::decimal_to_string(&value)),
                "String",
            )))
        }
        ("Decimal", "to_float") => {
            let value = decimal_input(&args[0], span)?;
            Ok(Ok((
                MirRuntimeValue::Float {
                    value: precise_kernel::decimal_to_float(&value),
                    f32: false,
                },
                "Float",
            )))
        }
        ("Decimal", "to_int") => {
            let value = decimal_input(&args[0], span)?;
            match precise_kernel::decimal_to_int(&value) {
                Some(value) => Ok(Ok((exact_integer_value(value), "Int"))),
                None => Ok(Err(
                    precise_kernel::failure_message(type_name, function).to_string(),
                )),
            }
        }
        ("Decimal", "to_fraction") => {
            let value = decimal_input(&args[0], span)?;
            match precise_kernel::decimal_to_fraction(&value) {
                Some(value) => fraction_value(value, span).map(|value| Ok((value, "Fraction"))),
                None => Ok(Err(
                    precise_kernel::failure_message(type_name, function).to_string(),
                )),
            }
        }
        ("Fraction", "new" | "from_parts") => {
            let numerator = exact_integer(args, 0, span)?;
            let denominator = exact_integer(args, 1, span)?;
            match precise_kernel::fraction_new(numerator, denominator) {
                Some(value) => fraction_value(value, span).map(|value| Ok((value, "Fraction"))),
                None => Ok(Err(
                    precise_kernel::failure_message(type_name, function).to_string(),
                )),
            }
        }
        ("Fraction", "from_int") => {
            let value = exact_integer(args, 0, span)?;
            match precise_kernel::fraction_from_int(value) {
                Some(value) => fraction_value(value, span).map(|value| Ok((value, "Fraction"))),
                None => Ok(Err(
                    precise_kernel::failure_message(type_name, function).to_string(),
                )),
            }
        }
        ("Fraction", "from_float") => match precise_kernel::fraction_from_float(
            float(args, 0, span)?,
        ) {
            Some(value) => fraction_value(value, span).map(|value| Ok((value, "Fraction"))),
            None => Ok(Err(
                precise_kernel::failure_message(type_name, function).to_string(),
            )),
        },
        ("Fraction", "from_decimal") => {
            let value = decimal_input(&args[0], span)?;
            match precise_kernel::fraction_from_decimal(&value) {
                Some(value) => fraction_value(value, span).map(|value| Ok((value, "Fraction"))),
                None => Ok(Err(
                    precise_kernel::failure_message(type_name, function).to_string(),
                )),
            }
        }
        ("Fraction", "add" | "sub" | "mul" | "div") => {
            let left = fraction_input(&args[0], span)?;
            let right = fraction_input(&args[1], span)?;
            let value = match function {
                "add" => precise_kernel::fraction_add(&left, &right),
                "sub" => precise_kernel::fraction_sub(&left, &right),
                "mul" => precise_kernel::fraction_mul(&left, &right),
                "div" => precise_kernel::fraction_div(&left, &right),
                _ => unreachable!(),
            };
            match value {
                Some(value) => fraction_value(value, span).map(|value| Ok((value, "Fraction"))),
                None => Ok(Err(
                    precise_kernel::failure_message(type_name, function).to_string(),
                )),
            }
        }
        ("Fraction", "equal") => {
            let left = fraction_input(&args[0], span)?;
            let right = fraction_input(&args[1], span)?;
            Ok(Ok((
                MirRuntimeValue::Bool(precise_kernel::fraction_equal(&left, &right)),
                "Bool",
            )))
        }
        ("Fraction", "numerator" | "denominator") => {
            let value = fraction_input(&args[0], span)?;
            let value = if function == "numerator" {
                precise_kernel::fraction_numerator(&value)
            } else {
                precise_kernel::fraction_denominator(&value)
            };
            Ok(Ok((exact_integer_value(value), "Int")))
        }
        ("Fraction", "to_int") => {
            let value = fraction_input(&args[0], span)?;
            match precise_kernel::fraction_to_int(&value) {
                Some(value) => Ok(Ok((exact_integer_value(value), "Int"))),
                None => Ok(Err(
                    precise_kernel::failure_message(type_name, function).to_string(),
                )),
            }
        }
        ("Fraction", "to_string") => {
            let value = fraction_input(&args[0], span)?;
            Ok(Ok((
                MirRuntimeValue::String(precise_kernel::fraction_to_string(&value)),
                "String",
            )))
        }
        ("Fraction", "to_float") => {
            let value = fraction_input(&args[0], span)?;
            Ok(Ok((
                MirRuntimeValue::Float {
                    value: precise_kernel::fraction_to_float(&value),
                    f32: false,
                },
                "Float",
            )))
        }
        ("Fraction", "is_zero") => {
            let value = fraction_input(&args[0], span)?;
            Ok(Ok((
                MirRuntimeValue::Bool(precise_kernel::fraction_is_zero(&value)),
                "Bool",
            )))
        }
        ("Fraction", "to_decimal") => {
            let value = fraction_input(&args[0], span)?;
            match precise_kernel::fraction_to_decimal(&value) {
                Some(value) => decimal_value(value, span).map(|value| Ok((value, "Decimal"))),
                None => Ok(Err(
                    precise_kernel::failure_message(type_name, function).to_string(),
                )),
            }
        }
        _ => return Err(bridge_error(span, "native precise route has no canonical kernel")),
    };
    result
}

fn dispatch_precise(
    route: NativePreludeRoute<'_>,
    type_name: &str,
    function: &str,
    args: Vec<MirRuntimeValue>,
    result_ty: Option<MirType>,
    span: Span,
) -> Result<AmbientMirPreludeResult, Diagnostic> {
    let Some((expected, arity)) = precise_expected(type_name, function) else {
        return Err(bridge_error(span, "native precise route has an unsupported function"));
    };
    let symbol_matches = match type_name {
        "Decimal" => route
            .symbol
            .strip_prefix("jet_decimal_")
            .is_some_and(|name| name == function),
        "Fraction" => route
            .symbol
            .strip_prefix("jet_fraction_")
            .is_some_and(|name| name == function),
        _ => false,
    };
    if route.module != type_name || route.member != function || !symbol_matches {
        return Err(bridge_error(span, "native precise route has non-canonical identity"));
    }
    if route.family_overflow || !route.abi_value || route.effect_name.is_some() {
        return Err(bridge_error(
            span,
            "native precise route has non-canonical checked metadata",
        ));
    }
    let by_value = function == "new"
        || function == "from_parts"
        || (function.starts_with("from_") && function != "from_str");
    let borrow_mask = match (by_value, arity) {
        (true, 1) => BORROW_NONE_1,
        (true, 2) => BORROW_NONE_2,
        (false, 1) => BORROW_ALL_1,
        (false, 2) => BORROW_ALL_2,
        _ => {
            return Err(bridge_error(
                span,
                "native precise route has an unsupported borrow-mask arity",
            ));
        }
    };
    validate_signature(route, &args, arity, borrow_mask, span)?;
    let result_ty = result_ty
        .as_ref()
        .ok_or_else(|| bridge_error(span, "native precise route has no checked result type"))?;
    let has_failure_carrier = result_ty.option_inner().is_some() || result_ty.result_parts().is_some();
    if route.infallible == has_failure_carrier {
        return Err(bridge_error(
            span,
            "native precise route fallibility disagrees with its checked result type",
        ));
    }
    let value = precise_value(type_name, function, &args, span)?;
    let (value, value_type) = match value {
        Ok(value) => value,
        Err(message) => return Ok(AmbientMirPreludeResult::Value(precise_failure(route, result_ty, &message, span)?)),
    };
    if value_type != expected {
        return Err(bridge_error(span, "native precise kernel returned the wrong carrier kind"));
    }
    Ok(AmbientMirPreludeResult::Value(precise_success(
        route, result_ty, value, expected, span,
    )?))
}

fn conversion_success_type<'a>(
    result_ty: &'a MirType,
    span: Span,
) -> Result<&'a MirType, Diagnostic> {
    success_type(result_ty, span)
}

fn conversion_failure(
    route: NativePreludeRoute<'_>,
    result_ty: &MirType,
    message: &str,
    span: Span,
) -> Result<MirRuntimeValue, Diagnostic> {
    if route.infallible {
        return Err(bridge_error(span, message));
    }
    if let Some(inner) = result_ty.option_inner() {
        return Ok(MirRuntimeValue::Absent {
            element: inner.clone(),
        });
    }
    if result_ty.result_parts().is_some() {
        return Ok(MirRuntimeValue::FailedTold(Box::new(
            MirRuntimeValue::String(message.to_string()),
        )));
    }
    Err(bridge_error(
        span,
        "fallible numeric conversion has no MIR failure carrier",
    ))
}

fn validate_conversion_route(
    route: NativePreludeRoute<'_>,
    member: &str,
    symbol: &str,
    arity: usize,
    borrow_mask: &[bool],
    infallible: bool,
    args: &[MirRuntimeValue],
    span: Span,
) -> Result<(), Diagnostic> {
    if route.module != "core.numeric" || route.member != member || route.symbol != symbol {
        return Err(bridge_error(span, "native numeric conversion route has non-canonical identity"));
    }
    validate_metadata(route, false, infallible, None, span)?;
    validate_signature(route, args, arity, borrow_mask, span)
}

fn kind_shape(kind: i64) -> Option<(bool, u8)> {
    Some(match kind {
        0 => (true, 8),
        1 => (true, 16),
        2 => (true, 32),
        3 => (true, 64),
        4 => (false, 8),
        5 => (false, 16),
        6 => (false, 32),
        7 => (false, 64),
        _ => return None,
    })
}

fn require_fixed_success(
    result_ty: &MirType,
    kind: i64,
    span: Span,
) -> Result<(), Diagnostic> {
    let (signed, bits) = kind_shape(kind).ok_or_else(|| {
        bridge_error(span, "numeric conversion received an unknown fixed-width kind")
    })?;
    if result_ty.fixed_int() == Some((signed, bits)) {
        Ok(())
    } else {
        Err(bridge_error(
            span,
            "numeric conversion result width disagrees with its checked type",
        ))
    }
}

fn require_float_success(result_ty: &MirType, f32: bool, span: Span) -> Result<(), Diagnostic> {
    let valid = if f32 {
        matches!(&result_ty.kind, MirTypeKind::Float32)
    } else {
        matches!(&result_ty.kind, MirTypeKind::Float)
    };
    if valid {
        Ok(())
    } else {
        Err(bridge_error(
            span,
            "numeric conversion result float width disagrees with its checked type",
        ))
    }
}

fn marshal_fixed_i128(
    value: i128,
    result_ty: &MirType,
    span: Span,
) -> Result<MirRuntimeValue, Diagnostic> {
    let Some((signed, bits)) = result_ty.fixed_int() else {
        return Err(bridge_error(span, "numeric conversion result is not a fixed integer"));
    };
    let raw = if signed {
        i64::try_from(value)
            .map_err(|_| bridge_error(span, "numeric conversion result exceeds signed carrier width"))?
    } else {
        u64::try_from(value)
            .map_err(|_| bridge_error(span, "numeric conversion result exceeds unsigned carrier width"))?
            as i64
    };
    let _ = bits;
    Ok(MirRuntimeValue::Int(raw))
}

fn wrap_conversion_success(
    route: NativePreludeRoute<'_>,
    result_ty: &MirType,
    value: MirRuntimeValue,
    span: Span,
) -> Result<MirRuntimeValue, Diagnostic> {
    if route.infallible {
        if result_ty.option_inner().is_some() || result_ty.result_parts().is_some() {
            return Err(bridge_error(
                span,
                "infallible numeric conversion unexpectedly has a failure carrier",
            ));
        }
        Ok(value)
    } else if result_ty.option_inner().is_some() || result_ty.result_parts().is_some() {
        Ok(MirRuntimeValue::Present(Box::new(value)))
    } else {
        Err(bridge_error(
            span,
            "fallible numeric conversion has no MIR failure carrier",
        ))
    }
}

fn dispatch_conversion(
    route: NativePreludeRoute<'_>,
    symbol: &str,
    args: Vec<MirRuntimeValue>,
    result_ty: Option<MirType>,
    span: Span,
) -> Result<AmbientMirPreludeResult, Diagnostic> {
    let result_ty = result_ty
        .as_ref()
        .ok_or_else(|| bridge_error(span, "native numeric conversion has no checked result type"))?;
    let success_ty = conversion_success_type(result_ty, span)?;
    let value = match symbol {
        "jet_numeric_float_to_int" => {
            validate_conversion_route(route, "float_to_int", symbol, 2, &[false, false], false, &args, span)?;
            let kind = integer(&args, 1, span)?;
            require_fixed_success(success_ty, kind, span)?;
            match jet_foundation::NumericConversion::jet_numeric_float_to_int(float(&args, 0, span)?, kind) {
                Ok(value) => marshal_fixed_i128(value, success_ty, span),
                Err(error) => return Ok(AmbientMirPreludeResult::Value(conversion_failure(route, result_ty, error, span)?)),
            }
        }
        "jet_numeric_float_narrow" => {
            validate_conversion_route(route, "float_narrow", symbol, 1, &[false], false, &args, span)?;
            require_float_success(success_ty, true, span)?;
            match jet_foundation::NumericConversion::jet_numeric_float_narrow(float(&args, 0, span)?) {
                Ok(value) => Ok(MirRuntimeValue::Float { value: f64::from(value), f32: true }),
                Err(error) => return Ok(AmbientMirPreludeResult::Value(conversion_failure(route, result_ty, error, span)?)),
            }
        }
        "jet_numeric_try_from_fixed" => {
            validate_conversion_route(route, "fixed_try_from", symbol, 3, &[false, false, false], false, &args, span)?;
            let kind = integer(&args, 2, span)?;
            require_fixed_success(success_ty, kind, span)?;
            match jet_foundation::NumericConversion::jet_numeric_try_from_fixed(
                raw_integer(&args, 0, span)?,
                boolean(&args, 1, span)?,
                kind,
            ) {
                Ok(value) => marshal_fixed_i128(value, success_ty, span),
                Err(error) => return Ok(AmbientMirPreludeResult::Value(conversion_failure(route, result_ty, error, span)?)),
            }
        }
        "jet_std::jet_int_try_from_checked" => {
            validate_conversion_route(route, "int_try_from", symbol, 2, &[false, false], false, &args, span)?;
            let kind = integer(&args, 1, span)?;
            require_fixed_success(success_ty, kind, span)?;
            let value = exact_integer(&args, 0, span)?
                .try_i128()
                .and_then(|value| jet_foundation::NumericConversion::jet_numeric_fixed_from_i128(value, kind));
            match value {
                Some(value) => marshal_fixed_i128(value, success_ty, span),
                None => {
                    return Ok(AmbientMirPreludeResult::Value(conversion_failure(
                        route,
                        result_ty,
                        jet_foundation::NumericConversion::JET_NUMERIC_CONVERSION_ERROR,
                        span,
                    )?));
                }
            }
        }
        "jet_std::jet_int_checked_fixed" => {
            validate_conversion_route(route, "int_checked_fixed", symbol, 4, &[false, false, true, false], true, &args, span)?;
            let kind = integer(&args, 1, span)?;
            require_fixed_success(success_ty, kind, span)?;
            let value = exact_integer(&args, 0, span)?
                .try_i128()
                .and_then(|value| jet_foundation::NumericConversion::jet_numeric_fixed_from_i128(value, kind));
            match value {
                Some(value) => marshal_fixed_i128(value, success_ty, span),
                None => {
                    let (file, line) = source_line(&args, 2, 3, span)?;
                    return Err(runtime_stop(
                        file,
                        line,
                        jet_foundation::NumericConversion::JET_NUMERIC_CONVERSION_TRAP,
                        span,
                    ));
                }
            }
        }
        "jet_std::jet_int_checked_widen" => {
            validate_conversion_route(route, "checked_widen", symbol, 4, &[false, false, true, false], true, &args, span)?;
            let target_f32 = boolean(&args, 1, span)?;
            require_float_success(success_ty, target_f32, span)?;
            let value = exact_integer(&args, 0, span)?.checked_widen(target_f32);
            match value {
                Some(value) => Ok(MirRuntimeValue::Float { value, f32: target_f32 }),
                None => {
                    let (file, line) = source_line(&args, 2, 3, span)?;
                    return Err(runtime_stop(
                        file,
                        line,
                        jet_foundation::NumericConversion::JET_NUMERIC_WIDEN_TRAP,
                        span,
                    ));
                }
            }
        }
        "jet_numeric_checked_widen_at" => {
            validate_conversion_route(route, "checked_widen", symbol, 5, &[false, false, false, true, false], true, &args, span)?;
            let target_f32 = boolean(&args, 2, span)?;
            require_float_success(success_ty, target_f32, span)?;
            let value = jet_foundation::NumericConversion::jet_numeric_checked_widen(
                raw_integer(&args, 0, span)?,
                boolean(&args, 1, span)?,
                target_f32,
            );
            match value {
                Some(value) => Ok(MirRuntimeValue::Float { value, f32: target_f32 }),
                None => {
                    let (file, line) = source_line(&args, 3, 4, span)?;
                    return Err(runtime_stop(
                        file,
                        line,
                        jet_foundation::NumericConversion::JET_NUMERIC_WIDEN_TRAP,
                        span,
                    ));
                }
            }
        }
        _ => return Err(bridge_error(span, "native numeric conversion has no canonical kernel")),
    }?;
    Ok(AmbientMirPreludeResult::Value(wrap_conversion_success(
        route, result_ty, value, span,
    )?))
}

/// Dispatch one checked numeric Prelude row.
///
/// `None` means that this adapter does not own the symbol.  A recognized
/// symbol with malformed route facts returns `Some(Err(_))`, so callers cannot
/// silently fall back to a different arithmetic implementation.
pub(super) fn dispatch(
    route: super::NativePreludeRoute<'_>,
    args: Vec<MirRuntimeValue>,
    result_ty: Option<MirType>,
    span: Span,
) -> Option<Result<AmbientMirPreludeResult, Diagnostic>> {
    let symbol = route.symbol;
    if let Some(member) = exact_integer_member(symbol) {
        return Some(
            if route.module != "core.numeric" || route.member != member {
                Err(bridge_error(span, "native exact integer route has non-canonical identity"))
            } else {
                dispatch_exact_binary(route, member, args, result_ty, span)
            },
        );
    }
    if let Some(member) = exact_integer_unary_member(symbol) {
        return Some(
            if route.module != "core.numeric" || route.member != member {
                Err(bridge_error(span, "native exact integer unary route has non-canonical identity"))
            } else {
                dispatch_exact_unary(route, member, args, result_ty, span)
            },
        );
    }
    if symbol == "jet_int_compare" {
        return Some(dispatch_compare(route, args, result_ty, span));
    }
    if fixed_symbol_parts(symbol).is_some() {
        return Some(dispatch_fixed(route, symbol, args, result_ty, span));
    }
    if let Some((type_name, function)) = precise_symbol(symbol) {
        return Some(dispatch_precise(
            route,
            type_name,
            function,
            args,
            result_ty,
            span,
        ));
    }
    if is_conversion_symbol(symbol) {
        return Some(dispatch_conversion(route, symbol, args, result_ty, span));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{
        dispatch_exact_binary, exact_kernel, fixed_kernel, fixed_left, fixed_result_value,
        precise_kernel, source_line, NativePreludeRoute,
    };
    use jet_foundation::Diagnostics::{Diagnostic, Span};
    use jet_foundation::MIR::{MirRuntimeValue, MirType, MirTypeKind};
    use jet_foundation::Numeric::CtBigInt;

    const BORROW_NONE_4_TEST: &[bool] = &[false, false, false, false];

    fn exact_route(member: &'static str) -> NativePreludeRoute<'static> {
        NativePreludeRoute {
            module: "Int",
            member,
            symbol: "jet_std::jet_int_exact_test",
            family_overflow: true,
            abi_value: true,
            infallible: true,
            effect_name: Some("Panic"),
            arity: 4,
            max_arity: 4,
            borrow_mask: BORROW_NONE_4_TEST,
        }
    }

    fn exact_args(left: i64, right: i64) -> Vec<MirRuntimeValue> {
        vec![
            MirRuntimeValue::Int(left),
            MirRuntimeValue::Int(right),
            MirRuntimeValue::String("program.jet".to_string()),
            MirRuntimeValue::Int(42),
        ]
    }

    fn exact_failure(member: &'static str, left: i64, right: i64) -> Diagnostic {
        match dispatch_exact_binary(
            exact_route(member),
            exact_args(left, right),
            Some(MirType::from_kind(MirTypeKind::Int)),
            Span::new(8, 13),
        ) {
            Ok(_) => panic!("expected exact route failure"),
            Err(error) => error,
        }
    }

    #[test]
    fn native_numeric_bridge_u64_high_bit_carrier_stays_a_raw_mir_word() {
        let value = CtBigInt::from_str("18446744073709551615").expect("u64 max parses");
        assert_eq!(fixed_left(&value, false, Span::new(0, 0)).unwrap(), -1);
        assert_eq!(fixed_result_value(-1), MirRuntimeValue::Int(-1));
    }

    #[test]
    fn exact_kernel_signed_floor_mod_and_euclidean_remainder_share_policy() {
        let left = CtBigInt::from_int(-7);
        let right = CtBigInt::from_int(3);
        assert_eq!(
            exact_kernel::div_rem(&left, &right),
            Some((CtBigInt::from_int(-2), CtBigInt::from_int(-1)))
        );
        assert_eq!(
            exact_kernel::div_rem_euclid(&left, &right),
            Some((CtBigInt::from_int(-3), CtBigInt::from_int(2)))
        );
        assert_eq!(
            exact_kernel::floor_div(&left, &right),
            Some(CtBigInt::from_int(-3))
        );
        assert_eq!(
            exact_kernel::modulo(&left, &right),
            Some(CtBigInt::from_int(2))
        );
        assert!(exact_kernel::div_rem(&left, &CtBigInt::from_int(0)).is_none());
    }

    #[test]
    fn exact_dispatch_errors_use_shared_messages_and_source_context() {
        let zero = exact_failure("rem", 7, 0);
        let negative_power = exact_failure("pow", 2, -1);
        let invalid_shift = exact_failure("shl", 2, -1);
        let text = |error: &Diagnostic| {
            format!("{} {} {}", error.what, error.why, error.fix)
        };
        assert!(text(&zero).contains(exact_kernel::failure_message("rem")));
        assert!(text(&negative_power).contains(exact_kernel::failure_message("pow")));
        assert!(text(&invalid_shift).contains(exact_kernel::failure_message("shl")));
        assert_eq!(zero.code, "E3010");
        assert_eq!(zero.span, Some(Span::new(8, 13)));
        let args = exact_args(7, 0);
        assert_eq!(
            source_line(&args, 2, 3, Span::new(8, 13)).unwrap(),
            ("program.jet", 42)
        );
    }

    #[test]
    fn exact_kernel_rejects_negative_power_and_shift_counts() {
        let value = CtBigInt::from_int(2);
        let negative = CtBigInt::from_int(-1);
        assert!(exact_kernel::pow(&value, &negative).is_none());
        assert!(exact_kernel::shl(&value, &negative).is_none());
        assert!(exact_kernel::shr(&value, &negative).is_none());
        assert_eq!(
            exact_kernel::failure_message("pow"),
            "Negative default Int exponent"
        );
        assert_eq!(
            exact_kernel::failure_message("shl"),
            "Invalid shift count"
        );
    }

    #[test]
    fn fixed_kernel_modes_preserve_width_and_unsigned_high_bits() {
        use fixed_kernel::{
            jet_fixed_arithmetic, jet_fixed_integer_narrow, jet_fixed_integer_widen,
            JetFixedArithmeticError, JetFixedArithmeticResult, JET_FIXED_MODE_CHECKED,
            JET_FIXED_MODE_SATURATING, JET_FIXED_MODE_TRAP, JET_FIXED_MODE_WRAPPING,
            JET_FIXED_OP_ADD,
        };

        assert_eq!(
            jet_fixed_arithmetic(
                127,
                1,
                JET_FIXED_OP_ADD,
                JET_FIXED_MODE_CHECKED,
                true,
                8,
                true
            ),
            JetFixedArithmeticResult::Absent
        );
        assert_eq!(
            jet_fixed_arithmetic(
                127,
                1,
                JET_FIXED_OP_ADD,
                JET_FIXED_MODE_WRAPPING,
                true,
                8,
                true
            ),
            JetFixedArithmeticResult::Value(-128)
        );
        assert_eq!(
            jet_fixed_arithmetic(
                127,
                1,
                JET_FIXED_OP_ADD,
                JET_FIXED_MODE_SATURATING,
                true,
                8,
                true
            ),
            JetFixedArithmeticResult::Value(127)
        );
        assert_eq!(
            jet_fixed_arithmetic(
                127,
                1,
                JET_FIXED_OP_ADD,
                JET_FIXED_MODE_TRAP,
                true,
                8,
                true
            ),
            JetFixedArithmeticResult::Trap(JetFixedArithmeticError::AddOverflow)
        );
        assert_eq!(jet_fixed_integer_widen(-1, false), i128::from(u64::MAX));
        assert_eq!(jet_fixed_integer_narrow(255, false, 8), 255);
        assert_eq!(
            jet_fixed_arithmetic(
                -1,
                0,
                JET_FIXED_OP_ADD,
                JET_FIXED_MODE_WRAPPING,
                false,
                64,
                false
            ),
            JetFixedArithmeticResult::Value(-1)
        );
    }

    #[test]
    fn precise_kernel_distinguishes_constructor_and_representability_failures() {
        let decimal = precise_kernel::decimal_from_str("1.25").expect("valid Decimal");
        let finite = precise_kernel::fraction_new(
            CtBigInt::from_int(5),
            CtBigInt::from_int(4),
        )
        .expect("finite Fraction");
        let repeating = precise_kernel::fraction_new(
            CtBigInt::from_int(1),
            CtBigInt::from_int(3),
        )
        .expect("nonzero Fraction");
        let invalid_decimal: Result<_, ()> =
            precise_kernel::decimal_from_str("not-a-decimal");
        let invalid_fraction: Option<_> = precise_kernel::fraction_new(
            CtBigInt::from_int(1),
            CtBigInt::from_int(0),
        );

        assert!(invalid_decimal.is_err());
        assert!(invalid_fraction.is_none());
        assert!(precise_kernel::fraction_from_decimal(&decimal).is_some());
        assert!(precise_kernel::decimal_from_fraction(&finite).is_some());
        assert!(precise_kernel::decimal_from_fraction(&repeating).is_none());
        assert!(precise_kernel::decimal_from_float(f64::INFINITY).is_none());
        assert_eq!(
            precise_kernel::failure_message("Fraction", "from_parts"),
            "invalid exact quotient"
        );
        assert_eq!(
            precise_kernel::failure_message("Decimal", "to_fraction"),
            "Decimal does not fit Fraction"
        );
    }
}
