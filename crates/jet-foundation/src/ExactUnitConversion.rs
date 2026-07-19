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

fn approximate(num: &str, den: &str) -> Option<f64> {
    let value = num.parse::<f64>().ok()? / den.parse::<f64>().ok()?;
    value.is_finite().then_some(value)
}

/// Converts a Float only when exact rational arithmetic proves the result integral.
pub fn jet_unit_conversion_exact(value: f64, scale_num: &str, scale_den: &str, offset_num: &str, offset_den: &str) -> Option<f64> {
    let (value_num, value_den) = float_ratio(value)?;
    let scale_num_integer = Integer::parse(scale_num)?;
    let scale_den_integer = Integer::parse(scale_den)?;
    let offset_num_integer = Integer::parse(offset_num)?;
    let offset_den_integer = Integer::parse(offset_den)?;
    if scale_den_integer.is_zero() || offset_den_integer.is_zero() { return None; }
    let numerator = value_num.mul(&scale_num_integer).mul(&offset_den_integer)
        .add(&offset_num_integer.mul(&value_den).mul(&scale_den_integer));
    let denominator = value_den.mul(&scale_den_integer).mul(&offset_den_integer);
    if !numerator.div_rem_abs(&denominator)?.1.is_zero() { return None; }
    let converted = value * approximate(scale_num, scale_den)? + approximate(offset_num, offset_den)?;
    converted.is_finite().then_some(converted)
}

/// Applies the declared rational conversion, then rounds ties to even.
pub fn jet_unit_conversion_rounded(value: f64, scale_num: &str, scale_den: &str, offset_num: &str, offset_den: &str) -> Option<f64> {
    let converted = value * approximate(scale_num, scale_den)? + approximate(offset_num, offset_den)?;
    converted.is_finite().then(|| converted.round_ties_even())
}
