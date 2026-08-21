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

impl std::fmt::Display for JetWasmFraction {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.to_string_rep())
    }
}

fn jet_wasm_fraction_from_parts(
    numerator: JetWasmInt,
    denominator: JetWasmInt,
) -> JetWasmFraction {
    JetWasmFraction::new(numerator, denominator).expect("invalid exact quotient")
}

fn jet_wasm_fraction_add(a: &JetWasmFraction, b: &JetWasmFraction) -> JetWasmFraction {
    a.add(b).expect("exact ratio overflow")
}

fn jet_wasm_fraction_sub(a: &JetWasmFraction, b: &JetWasmFraction) -> JetWasmFraction {
    a.sub(b).expect("exact ratio overflow")
}

fn jet_wasm_fraction_mul(a: &JetWasmFraction, b: &JetWasmFraction) -> JetWasmFraction {
    a.mul(b).expect("exact ratio overflow")
}

fn jet_wasm_fraction_div(a: &JetWasmFraction, b: &JetWasmFraction) -> JetWasmFraction {
    a.div(b).expect("divided by zero")
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetWasmDecimal {
    negative: bool,
    digits: Vec<u8>,
    scale: u32,
}

impl JetWasmDecimal {
    fn from_str(s: &str) -> Result<Self, String> {
        let text = s.trim();
        if text.is_empty() {
            return Err("empty Decimal string".to_string());
        }
        let (negative, body) = if let Some(rest) = text.strip_prefix('-') {
            (true, rest)
        } else if let Some(rest) = text.strip_prefix('+') {
            (false, rest)
        } else {
            (false, text)
        };
        let parts: Vec<&str> = body.split('.').collect();
        if parts.len() > 2 {
            return Err(format!("invalid Decimal string `{s}`"));
        }
        let int_part = parts[0];
        let frac_part = parts.get(1).copied().unwrap_or("");
        if int_part.is_empty() && frac_part.is_empty()
            || !int_part.chars().all(|c| c.is_ascii_digit())
            || !frac_part.chars().all(|c| c.is_ascii_digit())
        {
            return Err(format!("invalid Decimal string `{s}`"));
        }

        let mut digits: Vec<u8> = int_part
            .chars()
            .chain(frac_part.chars())
            .map(|c| c as u8 - b'0')
            .collect();
        while digits.len() > 1 && digits.first() == Some(&0) {
            digits.remove(0);
        }
        if digits.is_empty() {
            digits.push(0);
        }
        Ok(Self {
            negative,
            digits,
            scale: frac_part.len() as u32,
        }
        .normalize())
    }

    fn normalize(mut self) -> Self {
        while self.scale > 0 && self.digits.len() > 1 && self.digits.last() == Some(&0) {
            self.digits.pop();
            self.scale -= 1;
        }
        if self.digits == [0] {
            self.negative = false;
            self.scale = 0;
        }
        self
    }

    fn align_scales(a: &Self, b: &Self) -> (Self, Self) {
        let scale = a.scale.max(b.scale);
        let mut left = a.clone();
        let mut right = b.clone();
        while left.scale < scale {
            left.digits.push(0);
            left.scale += 1;
        }
        while right.scale < scale {
            right.digits.push(0);
            right.scale += 1;
        }
        (left, right)
    }

    fn to_bigint(&self) -> JetWasmInt {
        let digits: String = self
            .digits
            .iter()
            .map(|digit| char::from(b'0' + *digit))
            .collect();
        JetWasmInt::from_decimal(&digits).expect("normalized Decimal digits")
    }

    fn from_bigint(value: JetWasmInt, scale: u32, negative: bool) -> Self {
        let text = value.to_string_rep();
        let body = text.strip_prefix('-').unwrap_or(&text);
        let digits = body.bytes().map(|byte| byte - b'0').collect();
        Self {
            negative,
            digits,
            scale,
        }
        .normalize()
    }

    fn add(&self, other: &Self) -> Self {
        let (a, b) = Self::align_scales(self, other);
        let left = a.to_bigint();
        let right = b.to_bigint();
        let negative = if a.negative == b.negative {
            a.negative
        } else if left.cmp_abs(&right) != std::cmp::Ordering::Less {
            a.negative
        } else {
            b.negative
        };
        if a.negative == b.negative {
            Self::from_bigint(left.add_ref(&right), a.scale, negative)
        } else if left.cmp_abs(&right) != std::cmp::Ordering::Less {
            Self::from_bigint(left.sub_abs(&right), a.scale, negative)
        } else {
            Self::from_bigint(right.sub_abs(&left), a.scale, negative)
        }
    }

    fn sub(&self, other: &Self) -> Self {
        let mut negated = other.clone();
        negated.negative = !negated.negative;
        self.add(&negated)
    }

    fn mul(&self, other: &Self) -> Self {
        Self::from_bigint(
            self.to_bigint().mul_ref(&other.to_bigint()),
            self.scale + other.scale,
            self.negative != other.negative,
        )
    }

    fn to_string_rep(&self) -> String {
        if self.digits == [0] {
            return "0".to_string();
        }
        let mut digits = self.digits.clone();
        let fraction_len = self.scale as usize;
        let sign = if self.negative { "-" } else { "" };
        if fraction_len == 0 {
            let whole: String = digits
                .iter()
                .map(|digit| char::from(b'0' + *digit))
                .collect();
            return format!("{sign}{whole}");
        }
        if digits.len() <= fraction_len {
            let padding = fraction_len - digits.len() + 1;
            digits.splice(0..0, std::iter::repeat_n(0, padding));
        }
        let split = digits.len() - fraction_len;
        let whole: String = digits[..split]
            .iter()
            .map(|digit| char::from(b'0' + *digit))
            .collect();
        let fraction: String = digits[split..]
            .iter()
            .map(|digit| char::from(b'0' + *digit))
            .collect();
        format!("{sign}{whole}.{fraction}")
    }
}

impl JetDisplay for JetWasmDecimal {
    fn jet_display(&self) -> String {
        self.to_string_rep()
    }
}

impl std::fmt::Display for JetWasmDecimal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.to_string_rep())
    }
}

fn jet_wasm_decimal_from_str(s: &String) -> JetWasmDecimal {
    JetWasmDecimal::from_str(s).expect("invalid Decimal string")
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
    a == b
}

fn jet_wasm_decimal_to_string(a: &JetWasmDecimal) -> String {
    a.to_string_rep()
}
