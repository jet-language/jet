// Shared Decimal/Fraction route kernels.
//
// The including module supplies `ExactBigInt`, `ExactDecimal`, and
// `ExactFraction` aliases.  The host bridge and emitted Core wrappers both
// marshal into these carriers before invoking the same Foundation policy.

pub(crate) fn decimal_from_str(value: &str) -> Result<ExactDecimal, ()> {
    ExactDecimal::from_str(value).map_err(|_| ())
}

pub(crate) fn decimal_from_int(value: ExactBigInt) -> ExactDecimal {
    ExactDecimal::from_str(&value.to_string_rep()).expect("exact Int must fit Decimal")
}

pub(crate) fn decimal_from_float(value: f64) -> Option<ExactDecimal> {
    ExactDecimal::from_float(value)
}

pub(crate) fn decimal_from_fraction(value: &ExactFraction) -> Option<ExactDecimal> {
    ExactDecimal::from_fraction(value)
}

pub(crate) fn decimal_add(left: &ExactDecimal, right: &ExactDecimal) -> ExactDecimal {
    left.add(right)
}

pub(crate) fn decimal_sub(left: &ExactDecimal, right: &ExactDecimal) -> ExactDecimal {
    left.sub(right)
}

pub(crate) fn decimal_mul(left: &ExactDecimal, right: &ExactDecimal) -> ExactDecimal {
    left.mul(right)
}

pub(crate) fn decimal_equal(left: &ExactDecimal, right: &ExactDecimal) -> bool {
    left.equal(right)
}

pub(crate) fn decimal_compare(
    left: &ExactDecimal,
    right: &ExactDecimal,
) -> core::cmp::Ordering {
    left.cmp(right)
}

pub(crate) fn fraction_equal(left: &ExactFraction, right: &ExactFraction) -> bool {
    left == right
}

pub(crate) fn fraction_compare(
    left: &ExactFraction,
    right: &ExactFraction,
) -> core::cmp::Ordering {
    left.cmp(right)
}

pub(crate) fn fraction_numerator(value: &ExactFraction) -> ExactBigInt {
    value.numerator.clone()
}

pub(crate) fn fraction_denominator(value: &ExactFraction) -> ExactBigInt {
    value.denominator.clone()
}

pub(crate) fn decimal_div(left: &ExactDecimal, right: &ExactDecimal) -> Option<ExactFraction> {
    left.div(right)
}

pub(crate) fn decimal_round(value: &ExactDecimal) -> ExactDecimal {
    value.round()
}

pub(crate) fn decimal_floor(value: &ExactDecimal) -> ExactDecimal {
    value.floor()
}

pub(crate) fn decimal_ceil(value: &ExactDecimal) -> ExactDecimal {
    value.ceil()
}

pub(crate) fn decimal_to_int(value: &ExactDecimal) -> Option<ExactBigInt> {
    value.to_int_exact()
}

pub(crate) fn decimal_to_fraction(value: &ExactDecimal) -> Option<ExactFraction> {
    value.to_fraction()
}

pub(crate) fn decimal_to_string(value: &ExactDecimal) -> String {
    value.to_string_rep()
}

pub(crate) fn decimal_to_float(value: &ExactDecimal) -> f64 {
    value.to_f64()
}

pub(crate) fn fraction_to_string(value: &ExactFraction) -> String {
    value.to_string_rep()
}

pub(crate) fn fraction_to_float(value: &ExactFraction) -> f64 {
    value.to_float()
}

pub(crate) fn fraction_is_zero(value: &ExactFraction) -> bool {
    value.is_zero()
}

pub(crate) fn fraction_new(
    numerator: ExactBigInt,
    denominator: ExactBigInt,
) -> Option<ExactFraction> {
    ExactFraction::from_bigints(numerator, denominator)
}

pub(crate) fn fraction_from_int(value: ExactBigInt) -> Option<ExactFraction> {
    ExactFraction::from_bigint(value)
}

pub(crate) fn fraction_from_float(value: f64) -> Option<ExactFraction> {
    ExactFraction::from_float(value)
}

pub(crate) fn fraction_from_decimal(value: &ExactDecimal) -> Option<ExactFraction> {
    ExactFraction::from_decimal(value)
}

pub(crate) fn fraction_add(left: &ExactFraction, right: &ExactFraction) -> Option<ExactFraction> {
    left.add(right)
}

pub(crate) fn fraction_sub(left: &ExactFraction, right: &ExactFraction) -> Option<ExactFraction> {
    left.sub(right)
}

pub(crate) fn fraction_mul(left: &ExactFraction, right: &ExactFraction) -> Option<ExactFraction> {
    left.mul(right)
}

pub(crate) fn fraction_div(left: &ExactFraction, right: &ExactFraction) -> Option<ExactFraction> {
    left.div(right)
}

pub(crate) fn fraction_to_int(value: &ExactFraction) -> Option<ExactBigInt> {
    value.to_int_exact()
}

pub(crate) fn fraction_to_decimal(value: &ExactFraction) -> Option<ExactDecimal> {
    value.to_decimal()
}

pub(crate) fn failure_message(type_name: &str, function: &str) -> &'static str {
    match (type_name, function) {
        ("Decimal", "from_str") => "invalid Decimal string",
        ("Decimal", "from_int") => "invalid exact Int",
        ("Decimal", "from_float") => "Float is not finite",
        ("Decimal", "from_fraction") => "Fraction has a repeating expansion",
        ("Decimal", "div") => "Decimal quotient does not fit Fraction, or divided by zero",
        ("Decimal", "to_int") => "Decimal is not an integer",
        ("Decimal", "to_fraction") => "Decimal does not fit Fraction",
        ("Fraction", "new" | "from_parts") => "invalid exact quotient",
        ("Fraction", "from_int") => "exact Int does not fit Fraction",
        ("Fraction", "from_float") => "Float has no word-sized exact Fraction",
        ("Fraction", "from_decimal") => "Decimal does not fit Fraction",
        ("Fraction", "add") => "this sum of ratios overflows the value type",
        ("Fraction", "sub") => "this difference of ratios overflows the value type",
        ("Fraction", "mul") => "this product of ratios overflows the value type",
        ("Fraction", "div") => "divided by zero",
        ("Fraction", "to_int") => "Fraction is not an integer",
        ("Fraction", "to_decimal") => "Fraction has a repeating expansion",
        _ => "precise numeric operation failed",
    }
}
