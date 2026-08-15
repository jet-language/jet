// D-FAIL-TIER1: one runtime predicate and one report shape for every engine.
//
// The AOT emitter calls these functions directly. The interpreter and
// Cranelift hosts include this same source and only marshal values into it.
// Keep the contract message, arithmetic facts, and the rich runtime reports
// here; callers own only control flow and transport.
#[inline]
pub(crate) fn jet_contract_check(condition: bool) -> bool {
    condition
}

#[inline]
pub(crate) fn jet_contract_report(
    clause_kw: &str,
    msg: &str,
    file: &str,
    line: u32,
) -> JetRuntimeDiagnostic {
    let message = format!("#{} contract failed: {}", clause_kw, msg);
    jet_render_runtime_stop("E3005", file, line, "", "", 1, 1, &message, "")
}

// D-FAIL-ARITH1: engines marshal an operation fact to this one arithmetic
// boundary. The code and wording do not live in a host adapter.
pub(crate) const JET_ARITHMETIC_CODE: &str = "E3010";
pub(crate) const JET_ARITHMETIC_DIVIDE_ZERO: &str = "divided by zero";
pub(crate) const JET_ARITHMETIC_DIVISION_ERROR: &str =
    "this division can't be done (dividing by zero, or overflow)";
pub(crate) const JET_ARITHMETIC_DIVIDE_OVERFLOW: &str =
    "this division overflows the value's type (the result is outside its range)";
pub(crate) const JET_ARITHMETIC_POWER_NEGATIVE: &str =
    "a negative exponent has no whole-number result (make the base a Float to raise it to a negative power)";
pub(crate) const JET_ARITHMETIC_POWER_OVERFLOW: &str =
    "this power overflows the value's type (the result is outside its range)";
pub(crate) const JET_ARITHMETIC_REMAINDER_OVERFLOW: &str =
    "attempt to calculate the remainder with overflow";

pub(crate) fn jet_arithmetic_message(kind: &str) -> &'static str {
    match kind {
        "add" => "this addition overflows the value's type (the result is outside its range)",
        "sub" => "this subtraction overflows the value's type (the result is outside its range)",
        "mul" => "this multiplication overflows the value's type (the result is outside its range)",
        "div" => JET_ARITHMETIC_DIVISION_ERROR,
        "divide_zero" => JET_ARITHMETIC_DIVIDE_ZERO,
        "divide_overflow" => JET_ARITHMETIC_DIVIDE_OVERFLOW,
        "pow_negative" => JET_ARITHMETIC_POWER_NEGATIVE,
        "pow" => JET_ARITHMETIC_POWER_OVERFLOW,
        "remainder" => JET_ARITHMETIC_REMAINDER_OVERFLOW,
        _ => "this operation overflows the value's type (the result is outside its range)",
    }
}

pub(crate) fn jet_arithmetic_shift_message(
    direction: &str,
    count: i128,
    bits: u8,
) -> Option<String> {
    if (0..i128::from(bits)).contains(&count) {
        return None;
    }
    Some(format!(
        "shifting {direction} by {count} bits is out of range (this type is {bits} bits wide)"
    ))
}
