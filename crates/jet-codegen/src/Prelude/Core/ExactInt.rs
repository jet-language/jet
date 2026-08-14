// D-INTBIG1: the Wasm adapter for default `Int`.
//
// Wasm exports cannot carry an arbitrary-size value in one scalar ABI slot.
// Internal Wasm functions therefore use this small, owned limb carrier, while
// the JS boundary uses the existing packed UTF-8 ownership rail. This is a
// marshalling adapter only: the operations mirror the exact `Int` Prelude
// algorithms and preserve the same sign, division, and bit semantics.
#[derive(Clone, Debug, PartialEq, Eq)]
struct JetWasmInt {
    negative: bool,
    limbs: Vec<u32>,
}

const JET_WASM_INT_BASE: u64 = 1_000_000_000;

impl JetWasmInt {
    fn zero() -> Self {
        Self {
            negative: false,
            limbs: vec![0],
        }
    }

    fn from_u64(mut value: u64) -> Self {
        if value == 0 {
            return Self::zero();
        }
        let mut limbs = Vec::new();
        while value != 0 {
            limbs.push((value % JET_WASM_INT_BASE) as u32);
            value /= JET_WASM_INT_BASE;
        }
        Self {
            negative: false,
            limbs,
        }
    }

    fn from_i64(value: i64) -> Self {
        if value < 0 {
            let magnitude = (value as i128).wrapping_neg() as u64;
            Self {
                negative: true,
                limbs: Self::from_u64(magnitude).limbs,
            }
        } else {
            Self::from_u64(value as u64)
        }
    }

    fn from_decimal(text: &str) -> Result<Self, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("empty integer".to_string());
        }
        let (negative, digits) = if let Some(rest) = text.strip_prefix('-') {
            (true, rest)
        } else if let Some(rest) = text.strip_prefix('+') {
            (false, rest)
        } else {
            (false, text)
        };
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(format!("invalid integer `{text}`"));
        }
        let mut value = Self::zero();
        for byte in digits.bytes() {
            value = value.mul_small(10).add_small(u32::from(byte - b'0'));
        }
        value.negative = negative && !value.is_zero();
        Ok(value)
    }

    fn normalize(mut self) -> Self {
        while self.limbs.len() > 1 && self.limbs.last() == Some(&0) {
            self.limbs.pop();
        }
        if self.is_zero() {
            self.negative = false;
        }
        self
    }

    fn is_zero(&self) -> bool {
        self.limbs.len() == 1 && self.limbs[0] == 0
    }

    fn with_sign(self, negative: bool) -> Self {
        Self {
            negative: negative && !self.is_zero(),
            limbs: self.limbs,
        }
    }

    fn mul_small(&self, multiplier: u32) -> Self {
        let mut carry = 0u64;
        let mut limbs = Vec::with_capacity(self.limbs.len() + 1);
        for &limb in &self.limbs {
            let product = u64::from(limb) * u64::from(multiplier) + carry;
            limbs.push((product % JET_WASM_INT_BASE) as u32);
            carry = product / JET_WASM_INT_BASE;
        }
        if carry != 0 {
            limbs.push(carry as u32);
        }
        Self {
            negative: self.negative,
            limbs,
        }
        .normalize()
    }

    fn add_small(&self, value: u32) -> Self {
        self.add_ref(&Self::from_u64(u64::from(value)))
    }

    fn cmp_abs(&self, other: &Self) -> std::cmp::Ordering {
        match self.limbs.len().cmp(&other.limbs.len()) {
            std::cmp::Ordering::Equal => self
                .limbs
                .iter()
                .rev()
                .cmp(other.limbs.iter().rev()),
            ordering => ordering,
        }
    }

    fn sub_abs(&self, other: &Self) -> Self {
        let mut borrow = 0i64;
        let mut limbs = Vec::with_capacity(self.limbs.len());
        for index in 0..self.limbs.len() {
            let left = i64::from(self.limbs[index]);
            let right = i64::from(*other.limbs.get(index).unwrap_or(&0));
            let mut value = left - right - borrow;
            if value < 0 {
                value += JET_WASM_INT_BASE as i64;
                borrow = 1;
            } else {
                borrow = 0;
            }
            limbs.push(value as u32);
        }
        Self {
            negative: false,
            limbs,
        }
        .normalize()
    }

    fn add_ref(&self, other: &Self) -> Self {
        if self.negative == other.negative {
            let mut carry = 0u64;
            let len = self.limbs.len().max(other.limbs.len());
            let mut limbs = Vec::with_capacity(len + 1);
            for index in 0..len {
                let left = u64::from(*self.limbs.get(index).unwrap_or(&0));
                let right = u64::from(*other.limbs.get(index).unwrap_or(&0));
                let value = left + right + carry;
                limbs.push((value % JET_WASM_INT_BASE) as u32);
                carry = value / JET_WASM_INT_BASE;
            }
            if carry != 0 {
                limbs.push(carry as u32);
            }
            return Self {
                negative: self.negative,
                limbs,
            }
            .normalize();
        }
        match self.cmp_abs(other) {
            std::cmp::Ordering::Equal => Self::zero(),
            std::cmp::Ordering::Greater => self.sub_abs(other).with_sign(self.negative),
            std::cmp::Ordering::Less => other.sub_abs(self).with_sign(other.negative),
        }
    }

    fn sub_ref(&self, other: &Self) -> Self {
        self.add_ref(&other.neg_ref())
    }

    fn neg_ref(&self) -> Self {
        self.clone().with_sign(!self.negative)
    }

    fn abs_ref(&self) -> Self {
        self.clone().with_sign(false)
    }

    fn mul_ref(&self, other: &Self) -> Self {
        let mut out = Self::zero();
        for (offset, &limb) in other.limbs.iter().enumerate() {
            if limb == 0 {
                continue;
            }
            let mut part = self.mul_small(limb);
            for _ in 0..offset {
                part = part.mul_small(JET_WASM_INT_BASE as u32);
            }
            out = out.add_ref(&part);
        }
        Self {
            negative: self.negative != other.negative,
            limbs: out.limbs,
        }
        .normalize()
    }

    fn div_rem_small(&self, divisor: u32) -> (Self, u32) {
        let divisor = u64::from(divisor);
        let mut remainder = 0u64;
        let mut limbs = vec![0u32; self.limbs.len()];
        for index in (0..self.limbs.len()).rev() {
            let value = remainder * JET_WASM_INT_BASE + u64::from(self.limbs[index]);
            limbs[index] = (value / divisor) as u32;
            remainder = value % divisor;
        }
        (
            Self {
                negative: false,
                limbs,
            }
            .normalize(),
            remainder as u32,
        )
    }

    fn div_rem_ref(&self, other: &Self) -> Option<(Self, Self)> {
        if other.is_zero() {
            return None;
        }
        let divisor = other.abs_ref();
        let dividend = self.abs_ref();
        if dividend.cmp_abs(&divisor) == std::cmp::Ordering::Less {
            return Some((Self::zero(), self.clone()));
        }
        let mut quotient = vec![0u32; dividend.limbs.len()];
        let mut remainder = Self::zero();
        for index in (0..dividend.limbs.len()).rev() {
            remainder.limbs.insert(0, dividend.limbs[index]);
            remainder = remainder.normalize();
            let mut low = 0u32;
            let mut high = (JET_WASM_INT_BASE - 1) as u32;
            while low < high {
                let middle = low + (high - low) / 2 + 1;
                if divisor.mul_small(middle).cmp_abs(&remainder)
                    != std::cmp::Ordering::Greater
                {
                    low = middle;
                } else {
                    high = middle - 1;
                }
            }
            quotient[index] = low;
            if low != 0 {
                remainder = remainder.sub_abs(&divisor.mul_small(low));
            }
        }
        let quotient = Self {
            negative: self.negative != other.negative,
            limbs: quotient,
        }
        .normalize();
        remainder.negative = self.negative && !remainder.is_zero();
        Some((quotient, remainder.normalize()))
    }

    fn bit_width(&self) -> usize {
        let mut value = self.abs_ref();
        let mut width = 0usize;
        while !value.is_zero() {
            let (next, _) = value.div_rem_small(2);
            value = next;
            width += 1;
        }
        width
    }

    fn unsigned_bits(&self, width: usize) -> Vec<bool> {
        let mut value = self.abs_ref();
        let mut bits = Vec::with_capacity(width);
        for _ in 0..width {
            let (next, remainder) = value.div_rem_small(2);
            bits.push(remainder != 0);
            value = next;
        }
        bits
    }

    fn from_unsigned_bits(bits: &[bool]) -> Self {
        let mut value = Self::zero();
        for bit in bits.iter().rev() {
            value = value.mul_small(2);
            if *bit {
                value = value.add_small(1);
            }
        }
        value
    }

    fn twos_complement(&self, width: usize) -> Vec<bool> {
        let mut bits = self.unsigned_bits(width);
        if self.negative {
            for bit in &mut bits {
                *bit = !*bit;
            }
            let mut carry = true;
            for bit in &mut bits {
                if !carry {
                    break;
                }
                if *bit {
                    *bit = false;
                } else {
                    *bit = true;
                    carry = false;
                }
            }
        }
        bits
    }

    fn from_twos_complement(mut bits: Vec<bool>) -> Self {
        let negative = bits.last().copied().unwrap_or(false);
        if !negative {
            return Self::from_unsigned_bits(&bits);
        }
        for bit in &mut bits {
            *bit = !*bit;
        }
        let mut carry = true;
        for bit in &mut bits {
            if !carry {
                break;
            }
            if *bit {
                *bit = false;
            } else {
                *bit = true;
                carry = false;
            }
        }
        Self::from_unsigned_bits(&bits).neg_ref()
    }

    fn bitwise_ref(&self, other: &Self, op: fn(bool, bool) -> bool) -> Self {
        let width = self.bit_width().max(other.bit_width()).saturating_add(1);
        let left = self.twos_complement(width);
        let right = other.twos_complement(width);
        Self::from_twos_complement(
            left.into_iter()
                .zip(right)
                .map(|(left, right)| op(left, right))
                .collect(),
        )
    }

    fn shift_count(&self) -> Option<usize> {
        if self.negative {
            return None;
        }
        self.to_i64().and_then(|value| usize::try_from(value).ok())
    }

    fn shl_ref(&self, count: &Self) -> Option<Self> {
        let count = count.shift_count()?;
        let mut value = self.clone();
        for _ in 0..count {
            value = value.mul_small(2);
        }
        Some(value)
    }

    fn shr_ref(&self, count: &Self) -> Option<Self> {
        let count = count.shift_count()?;
        let mut value = self.clone();
        for _ in 0..count {
            let (quotient, remainder) = value.abs_ref().div_rem_small(2);
            value = if self.negative && remainder != 0 {
                quotient.add_small(1).neg_ref()
            } else {
                quotient.with_sign(self.negative)
            };
        }
        Some(value)
    }

    fn pow_ref(&self, exponent: &Self) -> Option<Self> {
        if exponent.negative {
            return None;
        }
        let mut exponent = exponent.clone();
        let mut base = self.clone();
        let mut result = Self::from_u64(1);
        while !exponent.is_zero() {
            let (next, bit) = exponent.div_rem_small(2);
            if bit != 0 {
                result = result.mul_ref(&base);
            }
            exponent = next;
            if !exponent.is_zero() {
                base = base.mul_ref(&base);
            }
        }
        Some(result)
    }

    fn to_i64(&self) -> Option<i64> {
        let mut value = 0u128;
        for &limb in self.limbs.iter().rev() {
            value = value.checked_mul(JET_WASM_INT_BASE as u128)?;
            value = value.checked_add(u128::from(limb))?;
        }
        let value = i128::try_from(value).ok()?;
        i64::try_from(if self.negative { -value } else { value }).ok()
    }

    fn to_f64(&self) -> f64 {
        self.to_string().parse::<f64>().unwrap_or_else(|_| {
            if self.negative {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            }
        })
    }

    fn to_string_rep(&self) -> String {
        let top = *self.limbs.last().unwrap_or(&0);
        let mut text = top.to_string();
        for limb in self.limbs.iter().rev().skip(1) {
            text.push_str(&format!("{limb:09}"));
        }
        if self.negative && !self.is_zero() {
            format!("-{text}")
        } else {
            text
        }
    }
}

impl std::fmt::Display for JetWasmInt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_string_rep())
    }
}

impl PartialOrd for JetWasmInt {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for JetWasmInt {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self.negative, other.negative) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            (false, false) => self.cmp_abs(other),
            (true, true) => other.cmp_abs(self),
        }
    }
}

impl std::ops::Add for JetWasmInt {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        self.add_ref(&rhs)
    }
}

impl std::ops::Sub for JetWasmInt {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        self.sub_ref(&rhs)
    }
}

impl std::ops::Mul for JetWasmInt {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self::Output {
        self.mul_ref(&rhs)
    }
}

impl std::ops::BitAnd for JetWasmInt {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self::Output {
        self.bitwise_ref(&rhs, |left, right| left & right)
    }
}

impl std::ops::BitOr for JetWasmInt {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self::Output {
        self.bitwise_ref(&rhs, |left, right| left | right)
    }
}

impl std::ops::BitXor for JetWasmInt {
    type Output = Self;
    fn bitxor(self, rhs: Self) -> Self::Output {
        self.bitwise_ref(&rhs, |left, right| left ^ right)
    }
}

impl std::ops::Neg for JetWasmInt {
    type Output = Self;
    fn neg(self) -> Self::Output {
        self.neg_ref()
    }
}

impl std::ops::Not for JetWasmInt {
    type Output = Self;
    fn not(self) -> Self::Output {
        self.neg_ref().sub_ref(&Self::from_u64(1))
    }
}

impl std::ops::Div for JetWasmInt {
    type Output = Self;
    fn div(self, rhs: Self) -> Self::Output {
        self.div_rem_ref(&rhs).expect("division by zero").0
    }
}

impl std::ops::Rem for JetWasmInt {
    type Output = Self;
    fn rem(self, rhs: Self) -> Self::Output {
        self.div_rem_ref(&rhs).expect("division by zero").1
    }
}

fn jet_wasm_int_abs(value: JetWasmInt) -> JetWasmInt {
    value.abs_ref()
}

fn jet_wasm_int_min(left: JetWasmInt, right: JetWasmInt) -> JetWasmInt {
    left.min(right)
}

fn jet_wasm_int_max(left: JetWasmInt, right: JetWasmInt) -> JetWasmInt {
    left.max(right)
}

fn jet_wasm_int_clamp(value: JetWasmInt, low: JetWasmInt, high: JetWasmInt) -> JetWasmInt {
    value.clamp(low, high)
}

fn jet_wasm_int_div(left: JetWasmInt, right: JetWasmInt, file: &str, line: u32) -> JetWasmInt {
    left.div_rem_ref(&right).map(|pair| pair.0).unwrap_or_else(|| {
        jet_arithmetic_stop(file, line, "division by zero")
    })
}

fn jet_wasm_int_floor_div(left: JetWasmInt, right: JetWasmInt, file: &str, line: u32) -> JetWasmInt {
    let (quotient, remainder) = left
        .div_rem_ref(&right)
        .unwrap_or_else(|| jet_arithmetic_stop(file, line, "division by zero"));
    if !remainder.is_zero() && left.negative != right.negative {
        quotient.sub_ref(&JetWasmInt::from_u64(1))
    } else {
        quotient
    }
}

fn jet_wasm_int_mod(left: JetWasmInt, right: JetWasmInt, file: &str, line: u32) -> JetWasmInt {
    let (quotient, remainder) = left
        .div_rem_ref(&right)
        .unwrap_or_else(|| jet_arithmetic_stop(file, line, "division by zero"));
    if !remainder.is_zero() && left.negative != right.negative {
        remainder.add_ref(&right)
    } else {
        let _ = quotient;
        remainder
    }
}

fn jet_wasm_int_rem(left: JetWasmInt, right: JetWasmInt, file: &str, line: u32) -> JetWasmInt {
    left.div_rem_ref(&right).map(|pair| pair.1).unwrap_or_else(|| {
        jet_arithmetic_stop(file, line, "division by zero")
    })
}

fn jet_wasm_int_pow(left: JetWasmInt, right: JetWasmInt, file: &str, line: u32) -> JetWasmInt {
    left.pow_ref(&right).unwrap_or_else(|| {
        jet_arithmetic_stop(file, line, "a negative exponent has no whole-number result")
    })
}

fn jet_wasm_int_shl(left: JetWasmInt, right: JetWasmInt, file: &str, line: u32) -> JetWasmInt {
    left.shl_ref(&right).unwrap_or_else(|| {
        jet_arithmetic_stop(file, line, "invalid shift count")
    })
}

fn jet_wasm_int_shr(left: JetWasmInt, right: JetWasmInt, file: &str, line: u32) -> JetWasmInt {
    left.shr_ref(&right).unwrap_or_else(|| {
        jet_arithmetic_stop(file, line, "invalid shift count")
    })
}
