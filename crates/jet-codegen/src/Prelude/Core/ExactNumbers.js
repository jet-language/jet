// D-NUMTYPE1 / I9: exact-number adapters for the JS tier.
// The emitters only select these Prelude doors. Whole-number parts stay BigInt;
// no operation crosses JavaScript Number unless the language asks for Float.

function jet_fraction_gcd(left, right) {
  let a = left < 0n ? -left : left;
  let b = right < 0n ? -right : right;
  while (b !== 0n) {
    const remainder = a % b;
    a = b;
    b = remainder;
  }
  return a === 0n ? 1n : a;
}

function jet_fraction_value(numerator, denominator) {
  let n = BigInt(numerator);
  let d = BigInt(denominator);
  if (d === 0n) throw new Error("invalid exact quotient");
  if (d < 0n) {
    n = -n;
    d = -d;
  }
  const divisor = jet_fraction_gcd(n, d);
  n /= divisor;
  d /= divisor;
  const value = {
    __jet_type: "Fraction",
    numerator: n,
    denominator: d,
    toString() { return jet_fraction_to_string(this); },
  };
  return value;
}

function jet_fraction_from_parts(numerator, denominator) {
  return jet_fraction_value(numerator, denominator);
}

function jet_fraction_add(left, right) {
  return jet_fraction_value(
    left.numerator * right.denominator + right.numerator * left.denominator,
    left.denominator * right.denominator,
  );
}

function jet_fraction_sub(left, right) {
  return jet_fraction_value(
    left.numerator * right.denominator - right.numerator * left.denominator,
    left.denominator * right.denominator,
  );
}

function jet_fraction_mul(left, right) {
  return jet_fraction_value(
    left.numerator * right.numerator,
    left.denominator * right.denominator,
  );
}

function jet_fraction_div(left, right) {
  return jet_fraction_value(
    left.numerator * right.denominator,
    left.denominator * right.numerator,
  );
}

function jet_fraction_equal(left, right) {
  return left.numerator === right.numerator && left.denominator === right.denominator;
}

function jet_fraction_numerator(value) { return value.numerator; }
function jet_fraction_denominator(value) { return value.denominator; }
function jet_fraction_to_float(value) {
  return Number(value.numerator) / Number(value.denominator);
}
function jet_fraction_is_zero(value) { return value.numerator === 0n; }

function jet_fraction_to_string(value) {
  const numerator = value.numerator;
  const denominator = value.denominator;
  if (numerator === 0n) return "0";
  let factors = denominator;
  let twos = 0;
  while (factors % 2n === 0n) { factors /= 2n; twos += 1; }
  let fives = 0;
  while (factors % 5n === 0n) { factors /= 5n; fives += 1; }
  if (factors !== 1n) return `${numerator}/${denominator}`;
  const scale = Math.max(twos, fives);
  const magnitude = numerator < 0n ? -numerator : numerator;
  const whole = magnitude / denominator;
  if (scale === 0) return `${numerator < 0n ? "-" : ""}${whole}`;
  let remainder = magnitude % denominator;
  let fraction = "";
  for (let index = 0; index < scale; index += 1) {
    remainder *= 10n;
    fraction += (remainder / denominator).toString();
    remainder %= denominator;
  }
  fraction = fraction.replace(/0+$/, "");
  return fraction.length === 0
    ? `${numerator < 0n ? "-" : ""}${whole}`
    : `${numerator < 0n ? "-" : ""}${whole}.${fraction}`;
}

function jet_complex_value(real, imaginary) {
  const value = { real: Number(real), imaginary: Number(imaginary) };
  value.toString = () => `${value.real} ${value.imaginary < 0 ? "-" : "+"} ${Math.abs(value.imaginary)}i`;
  return value;
}

function jet_complex_from_parts(real, imaginary) { return jet_complex_value(real, imaginary); }
function jet_complex_add(left, right) {
  return jet_complex_value(left.real + right.real, left.imaginary + right.imaginary);
}
function jet_complex_sub(left, right) {
  return jet_complex_value(left.real - right.real, left.imaginary - right.imaginary);
}
function jet_complex_mul(left, right) {
  return jet_complex_value(
    left.real * right.real - left.imaginary * right.imaginary,
    left.real * right.imaginary + left.imaginary * right.real,
  );
}
function jet_complex_div(left, right) {
  const denominator = right.real * right.real + right.imaginary * right.imaginary;
  return jet_complex_value(
    (left.real * right.real + left.imaginary * right.imaginary) / denominator,
    (left.imaginary * right.real - left.real * right.imaginary) / denominator,
  );
}
function jet_complex_abs(value) { return Math.hypot(value.real, value.imaginary); }
function jet_complex_to_string(value) { return value.toString(); }

function jet_decimal_value(negative, digits, scale) {
  let value = { negative, digits: BigInt(digits), scale };
  while (value.scale > 0 && value.digits !== 0n && value.digits % 10n === 0n) {
    value.digits /= 10n;
    value.scale -= 1;
  }
  if (value.digits === 0n) value = { negative: false, digits: 0n, scale: 0 };
  value.toString = () => jet_decimal_to_string(value);
  return value;
}

function jet_decimal_from_str(text) {
  const trimmed = String(text).trim();
  if (trimmed.length === 0) throw new Error("empty Decimal string");
  const negative = trimmed.startsWith("-");
  const body = trimmed.replace(/^[+-]/, "");
  const parts = body.split(".");
  if (parts.length > 2 || !parts.every((part) => /^\d*$/.test(part)) || parts.every((part) => part.length === 0)) {
    throw new Error(`invalid Decimal string \`${text}\``);
  }
  const whole = parts[0] || "0";
  const fraction = parts[1] || "";
  return jet_decimal_value(negative, BigInt(whole + fraction), fraction.length);
}

function jet_decimal_signed(value) { return value.negative ? -value.digits : value.digits; }
function jet_decimal_add(left, right) {
  const scale = Math.max(left.scale, right.scale);
  const a = jet_decimal_signed(left) * 10n ** BigInt(scale - left.scale);
  const b = jet_decimal_signed(right) * 10n ** BigInt(scale - right.scale);
  const sum = a + b;
  return jet_decimal_value(sum < 0n, sum < 0n ? -sum : sum, scale);
}
function jet_decimal_sub(left, right) {
  return jet_decimal_add(left, jet_decimal_value(!right.negative, right.digits, right.scale));
}
function jet_decimal_mul(left, right) {
  const product = jet_decimal_signed(left) * jet_decimal_signed(right);
  return jet_decimal_value(product < 0n, product < 0n ? -product : product, left.scale + right.scale);
}
function jet_decimal_equal(left, right) {
  const scale = Math.max(left.scale, right.scale);
  return jet_decimal_signed(left) * 10n ** BigInt(scale - left.scale)
    === jet_decimal_signed(right) * 10n ** BigInt(scale - right.scale);
}
function jet_decimal_to_string(value) {
  if (value.scale === 0) return `${value.negative ? "-" : ""}${value.digits}`;
  let digits = value.digits.toString();
  if (digits.length <= value.scale) digits = digits.padStart(value.scale + 1, "0");
  const split = digits.length - value.scale;
  return `${value.negative ? "-" : ""}${digits.slice(0, split)}.${digits.slice(split)}`;
}
