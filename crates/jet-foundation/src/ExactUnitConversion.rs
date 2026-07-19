// Exact runtime proof for unit conversions, shared by JIT and emitted AOT code.

#[derive(Clone)]
struct Integer { sign: i8, limbs: Vec<u32> }

impl Integer {
    const BASE: u64 = 1_000_000_000;
    fn zero() -> Self { Self { sign: 0, limbs: Vec::new() } }
    fn one() -> Self { Self::from_u64(1) }
    fn from_u64(mut value: u64) -> Self {
        if value == 0 { return Self::zero(); }
        let mut limbs = Vec::new();
        while value != 0 { limbs.push((value % Self::BASE) as u32); value /= Self::BASE; }
        Self { sign: 1, limbs }
    }
    fn parse(text: &str) -> Option<Self> {
        let (sign, digits) = text.strip_prefix('-').map_or((1, text), |digits| (-1, digits));
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) { return None; }
        let mut value = Self::zero();
        for byte in digits.bytes() {
            value = value.mul_small(10).add(&Self::from_u64(u64::from(byte - b'0')));
        }
        if !value.is_zero() { value.sign = sign; }
        Some(value)
    }
    fn is_zero(&self) -> bool { self.sign == 0 }
    fn is_odd(&self) -> bool { self.limbs.first().is_some_and(|limb| limb % 2 == 1) }
    fn neg(mut self) -> Self { self.sign = -self.sign; self }
    fn normalize(&mut self) {
        while self.limbs.last() == Some(&0) { self.limbs.pop(); }
        if self.limbs.is_empty() { self.sign = 0; }
    }
    fn abs(&self) -> Self { let mut value = self.clone(); if !value.is_zero() { value.sign = 1; } value }
    fn abs_cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.limbs.len().cmp(&other.limbs.len())
            .then_with(|| self.limbs.iter().rev().cmp(other.limbs.iter().rev()))
    }
    fn abs_add(&self, other: &Self) -> Self {
        let mut carry = 0;
        let mut limbs = Vec::with_capacity(self.limbs.len().max(other.limbs.len()) + 1);
        for index in 0..self.limbs.len().max(other.limbs.len()) {
            let value = u64::from(*self.limbs.get(index).unwrap_or(&0))
                + u64::from(*other.limbs.get(index).unwrap_or(&0)) + carry;
            limbs.push((value % Self::BASE) as u32); carry = value / Self::BASE;
        }
        if carry != 0 { limbs.push(carry as u32); }
        Self { sign: 1, limbs }
    }
    fn abs_sub(&self, other: &Self) -> Self {
        let mut borrow = 0;
        let mut limbs = Vec::with_capacity(self.limbs.len());
        for index in 0..self.limbs.len() {
            let mut value = i64::from(self.limbs[index])
                - i64::from(*other.limbs.get(index).unwrap_or(&0)) - borrow;
            if value < 0 { value += Self::BASE as i64; borrow = 1; } else { borrow = 0; }
            limbs.push(value as u32);
        }
        let mut value = Self { sign: 1, limbs }; value.normalize(); value
    }
    fn add(&self, other: &Self) -> Self {
        if self.is_zero() { return other.clone(); }
        if other.is_zero() { return self.clone(); }
        if self.sign == other.sign {
            let mut value = self.abs_add(other); value.sign = self.sign; value
        } else {
            match self.abs_cmp(other) {
                std::cmp::Ordering::Greater => { let mut value = self.abs_sub(other); value.sign = self.sign; value }
                std::cmp::Ordering::Less => { let mut value = other.abs_sub(self); value.sign = other.sign; value }
                std::cmp::Ordering::Equal => Self::zero(),
            }
        }
    }
    fn mul_small(&self, other: u32) -> Self {
        if other == 0 || self.is_zero() { return Self::zero(); }
        let mut carry = 0;
        let mut limbs = Vec::with_capacity(self.limbs.len() + 1);
        for limb in &self.limbs {
            let value = u64::from(*limb) * u64::from(other) + carry;
            limbs.push((value % Self::BASE) as u32); carry = value / Self::BASE;
        }
        if carry != 0 { limbs.push(carry as u32); }
        Self { sign: self.sign, limbs }
    }
    fn mul(&self, other: &Self) -> Self {
        if self.is_zero() || other.is_zero() { return Self::zero(); }
        let mut limbs = vec![0u64; self.limbs.len() + other.limbs.len()];
        for (left_index, left) in self.limbs.iter().enumerate() {
            let mut carry = 0;
            for (right_index, right) in other.limbs.iter().enumerate() {
                let index = left_index + right_index;
                let value = limbs[index] + u64::from(*left) * u64::from(*right) + carry;
                limbs[index] = value % Self::BASE; carry = value / Self::BASE;
            }
            limbs[left_index + other.limbs.len()] += carry;
        }
        let mut value = Self { sign: self.sign * other.sign,
            limbs: limbs.into_iter().map(|limb| limb as u32).collect() };
        value.normalize(); value
    }
    fn mul_pow2(mut self, exponent: u32) -> Self {
        for _ in 0..exponent { self = self.mul_small(2); }
        self
    }
    fn div_rem_abs(&self, other: &Self) -> Option<(Self, Self)> {
        if other.is_zero() { return None; }
        let divisor = other.abs();
        let dividend = self.abs();
        if dividend.abs_cmp(&divisor) == std::cmp::Ordering::Less {
            return Some((Self::zero(), dividend));
        }
        let mut quotient = vec![0; dividend.limbs.len()];
        let mut remainder = Self::zero();
        for index in (0..dividend.limbs.len()).rev() {
            remainder.limbs.insert(0, dividend.limbs[index]); remainder.sign = 1; remainder.normalize();
            let (mut low, mut high) = (0, (Self::BASE - 1) as u32);
            while low < high {
                let middle = low + (high - low) / 2 + 1;
                if divisor.mul_small(middle).abs_cmp(&remainder) != std::cmp::Ordering::Greater {
                    low = middle;
                } else { high = middle - 1; }
            }
            quotient[index] = low;
            if low != 0 { remainder = remainder.abs_sub(&divisor.mul_small(low)); }
        }
        let mut quotient = Self { sign: 1, limbs: quotient }; quotient.normalize();
        Some((quotient, remainder))
    }
    fn decimal(&self) -> String {
        if self.is_zero() { return "0".to_string(); }
        let mut limbs = self.limbs.iter().rev();
        let mut text = limbs.next().expect("nonzero integer has limbs").to_string();
        for limb in limbs { text.push_str(&format!("{limb:09}")); }
        if self.sign < 0 { text.insert(0, '-'); }
        text
    }
}

fn float_ratio(value: f64) -> Option<(Integer, Integer)> {
    if !value.is_finite() { return None; }
    let bits = value.to_bits();
    let exponent_bits = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1_u64 << 52) - 1);
    let (significand, exponent) = if exponent_bits == 0 {
        (fraction, -1074)
    } else { ((1_u64 << 52) | fraction, exponent_bits - 1023 - 52) };
    let mut numerator = Integer::from_u64(significand);
    if bits >> 63 != 0 && !numerator.is_zero() { numerator.sign = -1; }
    let mut denominator = Integer::one();
    if exponent >= 0 { numerator = numerator.mul_pow2(exponent as u32); }
    else { denominator = denominator.mul_pow2((-exponent) as u32); }
    Some((numerator, denominator))
}

fn converted_ratio(
    value: f64,
    scale_num: &str,
    scale_den: &str,
    offset_num: &str,
    offset_den: &str,
) -> Option<(Integer, Integer)> {
    let (value_num, value_den) = float_ratio(value)?;
    let scale_num = Integer::parse(scale_num)?;
    let scale_den = Integer::parse(scale_den)?;
    let offset_num = Integer::parse(offset_num)?;
    let offset_den = Integer::parse(offset_den)?;
    if scale_den.is_zero() || offset_den.is_zero() { return None; }
    let mut numerator = value_num.mul(&scale_num).mul(&offset_den)
        .add(&offset_num.mul(&value_den).mul(&scale_den));
    let mut denominator = value_den.mul(&scale_den).mul(&offset_den);
    if denominator.sign < 0 {
        numerator = numerator.neg();
        denominator = denominator.neg();
    }
    Some((numerator, denominator))
}

fn exact_integer(numerator: &Integer, denominator: &Integer) -> Option<Integer> {
    let (mut quotient, remainder) = numerator.div_rem_abs(denominator)?;
    if !remainder.is_zero() { return None; }
    if !quotient.is_zero() { quotient.sign = numerator.sign * denominator.sign; }
    Some(quotient)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i64)]
pub enum UnitRoundingMode {
    TowardZero,
    Floor,
    Ceiling,
    NearestEven,
}

pub const UNIT_ROUNDING_NEGATIVE_DIGITS: &str =
    "rounded unit conversion needs nonnegative digits";
pub const UNIT_ROUNDING_UNREPRESENTABLE: &str =
    "unit conversion overflows its runtime representation";

fn rounded_integer(
    numerator: &Integer,
    denominator: &Integer,
    mode: UnitRoundingMode,
) -> Option<Integer> {
    let (mut quotient, remainder) = numerator.div_rem_abs(denominator)?;
    let sign = numerator.sign * denominator.sign;
    let increment = match mode {
        UnitRoundingMode::TowardZero => false,
        UnitRoundingMode::Floor => sign < 0 && !remainder.is_zero(),
        UnitRoundingMode::Ceiling => sign > 0 && !remainder.is_zero(),
        UnitRoundingMode::NearestEven => {
            let comparison = remainder.mul_small(2).abs_cmp(&denominator.abs());
            comparison == std::cmp::Ordering::Greater
                || (comparison == std::cmp::Ordering::Equal && quotient.is_odd())
        }
    };
    if increment {
        quotient = quotient.add(&Integer::one());
    }
    if !quotient.is_zero() { quotient.sign = sign; }
    Some(quotient)
}

fn decimal_float(integer: &Integer, digits: usize) -> Option<f64> {
    let negative = integer.sign < 0;
    let mut decimal = integer.abs().decimal();
    if digits != 0 {
        if decimal.len() <= digits {
            decimal.insert_str(0, &"0".repeat(digits + 1 - decimal.len()));
        }
        decimal.insert(decimal.len() - digits, '.');
    }
    if negative { decimal.insert(0, '-'); }
    let value = decimal.parse::<f64>().ok()?;
    value.is_finite().then_some(value)
}

fn exactly_represented_float(integer: &Integer) -> Option<f64> {
    let value = integer.decimal().parse::<f64>().ok()?;
    if !value.is_finite() { return None; }
    let (float_num, float_den) = float_ratio(value)?;
    float_num
        .add(&integer.mul(&float_den).neg())
        .is_zero()
        .then_some(value)
}

/// Converts a Float only when exact rational arithmetic proves the result integral.
pub fn jet_unit_conversion_exact(value: f64, scale_num: &str, scale_den: &str, offset_num: &str, offset_den: &str) -> Option<f64> {
    let (numerator, denominator) =
        converted_ratio(value, scale_num, scale_den, offset_num, offset_den)?;
    exactly_represented_float(&exact_integer(&numerator, &denominator)?)
}

/// Applies the declared rational conversion, then rounds to destination decimal places.
pub fn jet_unit_conversion_rounded(
    value: f64,
    scale_num: &str,
    scale_den: &str,
    offset_num: &str,
    offset_den: &str,
    mode: UnitRoundingMode,
    digits: i64,
) -> Result<f64, &'static str> {
    let digits = usize::try_from(digits).map_err(|_| UNIT_ROUNDING_NEGATIVE_DIGITS)?;
    let (mut numerator, denominator) =
        converted_ratio(value, scale_num, scale_den, offset_num, offset_den)
            .ok_or(UNIT_ROUNDING_UNREPRESENTABLE)?;
    for _ in 0..digits { numerator = numerator.mul_small(10); }
    let integer = rounded_integer(&numerator, &denominator, mode)
        .ok_or(UNIT_ROUNDING_UNREPRESENTABLE)?;
    decimal_float(&integer, digits).ok_or(UNIT_ROUNDING_UNREPRESENTABLE)
}
