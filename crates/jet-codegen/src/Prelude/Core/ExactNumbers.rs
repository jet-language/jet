// D-TYPE2-DEFAULT1: exact numeric carriers for the Wasm adapter.
//
// These are the Wasm representation of the same Prelude values used by the
// native tiers. The adapter changes the carrier, not the exact arithmetic law.

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetWasmFraction {
    numerator: JetWasmInt,
    denominator: JetWasmInt,
}

impl JetWasmFraction {
    fn new(mut numerator: JetWasmInt, mut denominator: JetWasmInt) -> Option<Self> {
        if denominator.is_zero() {
            return None;
        }
        if denominator.negative {
            numerator = numerator.neg_ref();
            denominator = denominator.neg_ref();
        }

        let mut a = numerator.abs_ref();
        let mut b = denominator.clone();
        while !b.is_zero() {
            let remainder = a.div_rem_ref(&b)?.1;
            a = b;
            b = remainder;
        }
        Some(Self {
            numerator: numerator.div_rem_ref(&a)?.0,
            denominator: denominator.div_rem_ref(&a)?.0,
        })
    }

    fn add(&self, other: &Self) -> Option<Self> {
        Self::new(
            self.numerator
                .mul_ref(&other.denominator)
                .add_ref(&other.numerator.mul_ref(&self.denominator)),
            self.denominator.mul_ref(&other.denominator),
        )
    }

    fn sub(&self, other: &Self) -> Option<Self> {
        Self::new(
            self.numerator
                .mul_ref(&other.denominator)
                .sub_ref(&other.numerator.mul_ref(&self.denominator)),
            self.denominator.mul_ref(&other.denominator),
        )
    }

    fn mul(&self, other: &Self) -> Option<Self> {
        Self::new(
            self.numerator.mul_ref(&other.numerator),
            self.denominator.mul_ref(&other.denominator),
        )
    }

    fn div(&self, other: &Self) -> Option<Self> {
        Self::new(
            self.numerator.mul_ref(&other.denominator),
            self.denominator.mul_ref(&other.numerator),
        )
    }

    fn to_string_rep(&self) -> String {
        match (self.numerator.to_i64(), self.denominator.to_i64()) {
            (Some(numerator), Some(denominator)) => {
                finite_fraction_decimal(numerator, denominator)
                    .unwrap_or_else(|| format!("{numerator}/{denominator}"))
            }
            _ => format!("{}/{}", self.numerator, self.denominator),
        }
    }
}

fn finite_fraction_decimal(numerator: i64, denominator: i64) -> Option<String> {
    if denominator <= 0 {
        return None;
    }
    if numerator == 0 {
        return Some("0".to_string());
    }

    let mut factors = denominator as u64;
    let mut twos = 0u32;
    while factors % 2 == 0 {
        factors /= 2;
        twos += 1;
    }
    let mut fives = 0u32;
    while factors % 5 == 0 {
        factors /= 5;
        fives += 1;
    }
    if factors != 1 {
        return None;
    }

    let scale = twos.max(fives);
    let denominator = denominator as u128;
    let magnitude = numerator.unsigned_abs() as u128;
    let mut remainder = magnitude % denominator;
    let whole = magnitude / denominator;
    let sign = if numerator < 0 { "-" } else { "" };
    if scale == 0 {
        return Some(format!("{sign}{whole}"));
    }

    let mut fraction = String::with_capacity(scale as usize);
    for _ in 0..scale {
        remainder *= 10;
        fraction.push(char::from(b'0' + (remainder / denominator) as u8));
        remainder %= denominator;
    }
    while fraction.ends_with('0') {
        fraction.pop();
    }
    if fraction.is_empty() {
        Some(format!("{sign}{whole}"))
    } else {
        Some(format!("{sign}{whole}.{fraction}"))
    }
}

impl JetDisplay for JetWasmFraction {
    fn jet_display(&self) -> String {
        self.to_string_rep()
    }
}

impl JetDebug for JetWasmFraction {
    fn jet_debug(&self) -> String {
        self.to_string_rep()
    }
}

impl std::fmt::Display for JetWasmFraction {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.to_string_rep())
    }
}

fn jet_wasm_fraction_from_parts(
    numerator: JetWasmInt,
    denominator: JetWasmInt,
) -> JetWasmFraction {
    JetWasmFraction::new(numerator, denominator)
        .unwrap_or_else(|| jet_panic("", 0, "", "", "invalid exact quotient"))
}

fn jet_wasm_fraction_add(a: &JetWasmFraction, b: &JetWasmFraction) -> JetWasmFraction {
    a.add(b)
        .unwrap_or_else(|| jet_panic("", 0, "", "", "exact ratio overflow"))
}

fn jet_wasm_fraction_sub(a: &JetWasmFraction, b: &JetWasmFraction) -> JetWasmFraction {
    a.sub(b)
        .unwrap_or_else(|| jet_panic("", 0, "", "", "exact ratio overflow"))
}

fn jet_wasm_fraction_mul(a: &JetWasmFraction, b: &JetWasmFraction) -> JetWasmFraction {
    a.mul(b)
        .unwrap_or_else(|| jet_panic("", 0, "", "", "exact ratio overflow"))
}

fn jet_wasm_fraction_div(a: &JetWasmFraction, b: &JetWasmFraction) -> JetWasmFraction {
    a.div(b)
        .unwrap_or_else(|| jet_panic("", 0, "", "", "divided by zero"))
}

fn jet_wasm_fraction_equal(a: &JetWasmFraction, b: &JetWasmFraction) -> bool {
    a == b
}

fn jet_wasm_fraction_numerator(a: &JetWasmFraction) -> JetWasmInt {
    a.numerator.clone()
}

fn jet_wasm_fraction_denominator(a: &JetWasmFraction) -> JetWasmInt {
    a.denominator.clone()
}

fn jet_wasm_fraction_to_string(a: &JetWasmFraction) -> String {
    a.to_string_rep()
}

fn jet_wasm_fraction_to_float(a: &JetWasmFraction) -> f64 {
    a.numerator.to_f64() / a.denominator.to_f64()
}

fn jet_wasm_fraction_is_zero(a: &JetWasmFraction) -> bool {
    a.numerator.is_zero()
}

impl JetWasmInt {
    fn from_u128(mut value: u128) -> Self {
        if value == 0 {
            return Self::zero();
        }
        let base = u128::from(JET_WASM_INT_BASE);
        let mut limbs = Vec::new();
        while value != 0 {
            limbs.push((value % base) as u32);
            value /= base;
        }
        Self {
            negative: false,
            limbs,
        }
    }

    fn from_i128(value: i128) -> Self {
        let negative = value < 0;
        let magnitude = if negative {
            value.wrapping_neg() as u128
        } else {
            value as u128
        };
        Self::from_u128(magnitude).with_sign(negative)
    }

    fn from_decimal_digits(digits: &[u8]) -> Self {
        let mut value = Self::zero();
        for &digit in digits {
            value = value.mul_small(10).add_small(u32::from(digit));
        }
        value
    }

    fn to_i128(&self) -> Option<i128> {
        let base = u128::from(JET_WASM_INT_BASE);
        let mut magnitude = 0u128;
        for &limb in self.limbs.iter().rev() {
            magnitude = magnitude.checked_mul(base)?;
            magnitude = magnitude.checked_add(u128::from(limb))?;
        }
        if !self.negative {
            return i128::try_from(magnitude).ok();
        }
        let minimum = 1u128 << 127;
        if magnitude > minimum {
            None
        } else if magnitude == minimum {
            Some(i128::MIN)
        } else {
            Some(-(magnitude as i128))
        }
    }

    fn mul_pow10(mut self, scale: u32) -> Self {
        for _ in 0..scale {
            self = self.mul_small(10);
        }
        self
    }

    fn decimal_len(&self) -> usize {
        let mut top = *self.limbs.last().unwrap_or(&0);
        let mut digits = 1usize;
        while top >= 10 {
            top /= 10;
            digits += 1;
        }
        digits + self.limbs.len().saturating_sub(1) * 9
    }

    fn write_decimal(&self, out: &mut String) {
        use std::fmt::Write as _;
        let top = *self.limbs.last().unwrap_or(&0);
        let _ = write!(out, "{top}");
        for &limb in self.limbs.iter().rev().skip(1) {
            let _ = write!(out, "{limb:09}");
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum JetWasmDecimalMagnitude {
    Small(i128),
    Big(JetWasmInt),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetWasmDecimal {
    negative: bool,
    magnitude: JetWasmDecimalMagnitude,
    scale: u32,
}

impl JetWasmDecimal {
    fn from_str(s: &str) -> Result<Self, String> {
        let (negative, digits, scale) = json_decimal_lexeme(s)?;
        Ok(Self::from_digits(negative, digits, scale))
    }

    fn from_digits(negative: bool, digits: Vec<u8>, scale: u32) -> Self {
        let mut small = 0i128;
        let mut fits = true;
        for &digit in &digits {
            let Some(next) = small
                .checked_mul(10)
                .and_then(|value| value.checked_add(i128::from(digit)))
            else {
                fits = false;
                break;
            };
            small = next;
        }
        let magnitude = if fits {
            JetWasmDecimalMagnitude::Small(small)
        } else {
            JetWasmDecimalMagnitude::Big(JetWasmInt::from_decimal_digits(&digits))
        };
        Self {
            negative,
            magnitude,
            scale,
        }
        .normalize()
    }

    fn is_zero(&self) -> bool {
        match &self.magnitude {
            JetWasmDecimalMagnitude::Small(value) => *value == 0,
            JetWasmDecimalMagnitude::Big(value) => value.is_zero(),
        }
    }

    fn normalize(mut self) -> Self {
        match &mut self.magnitude {
            JetWasmDecimalMagnitude::Small(value) => {
                while self.scale > 0 && *value != 0 && *value % 10 == 0 {
                    *value /= 10;
                    self.scale -= 1;
                }
            }
            JetWasmDecimalMagnitude::Big(value) => {
                while self.scale > 0 && !value.is_zero() {
                    let (next, remainder) = value.div_rem_small(10);
                    if remainder != 0 {
                        break;
                    }
                    *value = next;
                    self.scale -= 1;
                }
            }
        }
        if self.is_zero() {
            self.negative = false;
            self.scale = 0;
        }
        self
    }

    fn signed_small(&self) -> Option<i128> {
        let JetWasmDecimalMagnitude::Small(value) = &self.magnitude else {
            return None;
        };
        if self.negative {
            (*value).checked_neg()
        } else {
            Some(*value)
        }
    }

    fn scale_small(value: i128, places: u32) -> Option<i128> {
        if value == 0 {
            return Some(0);
        }
        let mut value = value;
        for _ in 0..places {
            value = value.checked_mul(10)?;
        }
        Some(value)
    }

    fn from_signed_small(value: i128, scale: u32) -> Option<Self> {
        let negative = value < 0;
        let magnitude = if negative {
            value.checked_neg()?
        } else {
            value
        };
        Some(
            Self {
                negative,
                magnitude: JetWasmDecimalMagnitude::Small(magnitude),
                scale,
            }
            .normalize(),
        )
    }

    fn try_add(&self, other: &Self, negate_other: bool) -> Option<Self> {
        let scale = self.scale.max(other.scale);
        let left = Self::scale_small(self.signed_small()?, scale - self.scale)?;
        let mut right = Self::scale_small(other.signed_small()?, scale - other.scale)?;
        if negate_other {
            right = right.checked_neg()?;
        }
        Self::from_signed_small(left.checked_add(right)?, scale)
    }

    fn to_bigint(&self) -> JetWasmInt {
        match &self.magnitude {
            JetWasmDecimalMagnitude::Small(value) => {
                let value = if self.negative {
                    value.checked_neg().expect("small Decimal magnitude is positive")
                } else {
                    *value
                };
                JetWasmInt::from_i128(value)
            }
            JetWasmDecimalMagnitude::Big(value) => value.clone().with_sign(self.negative),
        }
    }

    fn scaled_bigint(&self, scale: u32) -> JetWasmInt {
        self.to_bigint().mul_pow10(scale - self.scale)
    }

    fn from_bigint(value: JetWasmInt, scale: u32, negative: bool) -> Self {
        let magnitude = value.abs_ref();
        let magnitude = if let Some(value) = magnitude.to_i128() {
            JetWasmDecimalMagnitude::Small(value)
        } else {
            JetWasmDecimalMagnitude::Big(magnitude)
        };
        Self {
            negative,
            magnitude,
            scale,
        }
        .normalize()
    }

    fn from_signed_bigint(value: JetWasmInt, scale: u32) -> Self {
        Self::from_bigint(value.abs_ref(), scale, value.negative)
    }

    fn add_with_sign(&self, other: &Self, negate_other: bool) -> Self {
        if let Some(value) = self.try_add(other, negate_other) {
            return value;
        }
        let scale = self.scale.max(other.scale);
        let left = self.scaled_bigint(scale);
        let right = other.scaled_bigint(scale);
        let right = if negate_other {
            right.neg_ref()
        } else {
            right
        };
        Self::from_signed_bigint(left.add_ref(&right), scale)
    }

    fn add(&self, other: &Self) -> Self {
        self.add_with_sign(other, false)
    }

    fn sub(&self, other: &Self) -> Self {
        self.add_with_sign(other, true)
    }

    fn mul(&self, other: &Self) -> Self {
        let scale = self.scale + other.scale;
        if let (Some(left), Some(right)) = (self.signed_small(), other.signed_small()) {
            if let Some(value) = left.checked_mul(right) {
                if let Some(value) = Self::from_signed_small(value, scale) {
                    return value;
                }
            }
        }
        Self::from_signed_bigint(
            self.to_bigint().mul_ref(&other.to_bigint()),
            scale,
        )
    }

    fn equal(&self, other: &Self) -> bool {
        let scale = self.scale.max(other.scale);
        if let (Some(left), Some(right)) = (self.signed_small(), other.signed_small()) {
            let left = Self::scale_small(left, scale - self.scale);
            let right = Self::scale_small(right, scale - other.scale);
            if let (Some(left), Some(right)) = (left, right) {
                return left == right;
            }
            if left.is_some() != right.is_some() {
                return false;
            }
        }
        self.scaled_bigint(scale) == other.scaled_bigint(scale)
    }

    fn write_magnitude(&self, out: &mut String) {
        match &self.magnitude {
            JetWasmDecimalMagnitude::Small(value) => {
                use std::fmt::Write as _;
                let _ = write!(out, "{value}");
            }
            JetWasmDecimalMagnitude::Big(value) => value.write_decimal(out),
        }
    }

    fn magnitude_len(&self) -> usize {
        match &self.magnitude {
            JetWasmDecimalMagnitude::Small(value) => {
                let mut value = *value;
                let mut digits = 1usize;
                while value >= 10 {
                    value /= 10;
                    digits += 1;
                }
                digits
            }
            JetWasmDecimalMagnitude::Big(value) => value.decimal_len(),
        }
    }

    fn to_string_rep(&self) -> String {
        if self.is_zero() {
            return "0".to_string();
        }
        let fraction_len = self.scale as usize;
        let digit_len = self.magnitude_len();
        let mut out = String::with_capacity(
            usize::from(self.negative)
                + digit_len.max(fraction_len.saturating_add(1))
                + usize::from(fraction_len > 0),
        );
        if self.negative {
            out.push('-');
        }
        if fraction_len == 0 {
            self.write_magnitude(&mut out);
            return out;
        }
        if digit_len <= fraction_len {
            out.push('0');
            out.push('.');
            for _ in 0..fraction_len - digit_len {
                out.push('0');
            }
            self.write_magnitude(&mut out);
        } else {
            self.write_magnitude(&mut out);
            let split = out.len() - fraction_len;
            out.insert(split, '.');
        }
        out
    }
}

impl JetDisplay for JetWasmDecimal {
    fn jet_display(&self) -> String {
        self.to_string_rep()
    }
}

impl JetDebug for JetWasmDecimal {
    fn jet_debug(&self) -> String {
        self.to_string_rep()
    }
}

impl std::fmt::Display for JetWasmDecimal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.to_string_rep())
    }
}

fn jet_wasm_decimal_from_str(s: &String) -> JetWasmDecimal {
    JetWasmDecimal::from_str(s)
        .unwrap_or_else(|_| jet_panic("", 0, "", "", "invalid Decimal string"))
}

fn jet_wasm_decimal_add(a: &JetWasmDecimal, b: &JetWasmDecimal) -> JetWasmDecimal {
    a.add(b)
}

fn jet_wasm_decimal_sub(a: &JetWasmDecimal, b: &JetWasmDecimal) -> JetWasmDecimal {
    a.sub(b)
}

fn jet_wasm_decimal_mul(a: &JetWasmDecimal, b: &JetWasmDecimal) -> JetWasmDecimal {
    a.mul(b)
}

fn jet_wasm_decimal_equal(a: &JetWasmDecimal, b: &JetWasmDecimal) -> bool {
    a.equal(b)
}

fn jet_wasm_decimal_to_string(a: &JetWasmDecimal) -> String {
    a.to_string_rep()
}
