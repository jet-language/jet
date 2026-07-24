//! D-BIGINT1 / D-DECIMAL1: arbitrary-precision `BigInt` and base-10 `Decimal`.
//! Shared name/method tables for sema and codegen.

use crate::Syntax;
use crate::AST::{Expr, Marker, Type};

pub const MONEY_LINT_NAMES: &[&str] =
    &["price", "cost", "amount", "fee", "balance", "tax"];

pub fn is_bigint_type_name(name: &str) -> bool {
    name == Syntax::TYPE_BIGINT
}

pub fn is_decimal_type_name(name: &str) -> bool {
    name == Syntax::TYPE_DECIMAL
}

pub fn is_precise_numeric_type_name(name: &str) -> bool {
    is_bigint_type_name(name) || is_decimal_type_name(name)
}

pub fn type_is_bigint(ty: &Type) -> bool {
    matches!(ty, Type::Named(n) if is_bigint_type_name(n))
}

pub fn type_is_decimal(ty: &Type) -> bool {
    matches!(ty, Type::Named(n) if is_decimal_type_name(n))
}

pub fn is_money_like_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    MONEY_LINT_NAMES.iter().any(|m| lower.contains(m))
}

#[cfg(test)]
mod tests {
    use super::is_money_like_name;

    #[test]
    fn total_is_not_assumed_to_mean_money() {
        assert!(!is_money_like_name("total"));
        assert!(is_money_like_name("total_price"));
    }
}

/// D-DECIMAL1: `#[allow(float_money)]` suppresses the default-on money lint.
pub fn allows_float_money(markers: &[Marker]) -> bool {
    markers.iter().any(|m| {
        m.name == "allow"
            && m.args
                .iter()
                .any(|a| matches!(a, Expr::Ident(s, _) if s == "float_money"))
    })
}

pub fn bigint_method_return(method: &str, nargs: usize) -> Option<Option<Type>> {
    let bigint = || Type::Named(Syntax::TYPE_BIGINT.to_string());
    match (method, nargs) {
        ("add" | "sub" | "mul", 1) => Some(Some(bigint())),
        ("neg", 0) => Some(Some(bigint())),
        ("to_string", 0) => Some(Some(Type::String)),
        _ => None,
    }
}

pub fn decimal_method_return(method: &str, nargs: usize) -> Option<Option<Type>> {
    let decimal = || Type::Named(Syntax::TYPE_DECIMAL.to_string());
    match (method, nargs) {
        ("add" | "sub" | "mul", 1) => Some(Some(decimal())),
        ("to_string", 0) => Some(Some(Type::String)),
        _ => None,
    }
}

// ── CtBigInt: comptime/REPL tier-0 arbitrary-precision integer ──────────────
//
// Mirrors `JetBigInt` in
// `crates/jet-codegen/src/Prelude/CoreLib/JetStd/CommonTypes.rs` limb-for-limb
// (sign-magnitude, little-endian base 10^9) so a `BigInt` computed at comptime
// prints byte-identical to the same expression run through the AOT path
// (R12 parity). Kept here (not in `jet-comptime`) because `CtValue` — shared
// by every seam crate — needs the type in its own definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CtBigInt {
    pub negative: bool,
    pub limbs: Vec<u32>, // little-endian base 10^9
}

const CTBI_BASE: u64 = 1_000_000_000;

impl CtBigInt {
    pub fn from_int(n: i64) -> Self {
        if n == 0 {
            return CtBigInt {
                negative: false,
                limbs: vec![0],
            };
        }
        let negative = n < 0;
        let mut v = if negative {
            (n as i128).unsigned_abs() as u64
        } else {
            n as u64
        };
        let mut limbs = Vec::new();
        while v > 0 {
            limbs.push((v % CTBI_BASE) as u32);
            v /= CTBI_BASE;
        }
        CtBigInt { negative, limbs }
    }

    pub fn from_str(s: &str) -> Result<Self, String> {
        let t = s.trim();
        if t.is_empty() {
            return Err("empty BigInt string".to_string());
        }
        let (negative, body) = if let Some(rest) = t.strip_prefix('-') {
            (true, rest)
        } else if let Some(rest) = t.strip_prefix('+') {
            (false, rest)
        } else {
            (false, t)
        };
        if body.is_empty() || !body.chars().all(|c| c.is_ascii_digit()) {
            return Err(format!("invalid BigInt string `{s}`"));
        }
        let mut acc = CtBigInt {
            negative: false,
            limbs: vec![0],
        };
        for ch in body.chars() {
            let digit = ch.to_digit(10).unwrap();
            acc = acc.mul_small(10).add_small(digit);
        }
        acc.negative = negative && !(acc.limbs.len() == 1 && acc.limbs[0] == 0);
        Ok(acc)
    }

    fn normalize(mut self) -> Self {
        while self.limbs.len() > 1 && *self.limbs.last().unwrap() == 0 {
            self.limbs.pop();
        }
        if self.limbs.len() == 1 && self.limbs[0] == 0 {
            self.negative = false;
        }
        self
    }

    fn mul_small(&self, m: u32) -> Self {
        let mut carry = 0u64;
        let mut limbs = Vec::with_capacity(self.limbs.len() + 1);
        for &limb in &self.limbs {
            let prod = limb as u64 * m as u64 + carry;
            limbs.push((prod % CTBI_BASE) as u32);
            carry = prod / CTBI_BASE;
        }
        if carry > 0 {
            limbs.push(carry as u32);
        }
        CtBigInt {
            negative: self.negative,
            limbs,
        }
        .normalize()
    }

    fn add_small(&self, n: u32) -> Self {
        self.add(&CtBigInt::from_int(n as i64))
    }

    pub fn add(&self, other: &CtBigInt) -> CtBigInt {
        if self.negative == other.negative {
            let mut carry = 0u64;
            let len = self.limbs.len().max(other.limbs.len());
            let mut limbs = Vec::with_capacity(len + 1);
            for i in 0..len {
                let a = *self.limbs.get(i).unwrap_or(&0) as u64;
                let b = *other.limbs.get(i).unwrap_or(&0) as u64;
                let sum = a + b + carry;
                limbs.push((sum % CTBI_BASE) as u32);
                carry = sum / CTBI_BASE;
            }
            if carry > 0 {
                limbs.push(carry as u32);
            }
            CtBigInt {
                negative: self.negative,
                limbs,
            }
            .normalize()
        } else {
            let cmp = self.cmp_abs(other);
            if cmp == 0 {
                CtBigInt::from_int(0)
            } else if cmp > 0 {
                self.sub_abs(other).with_sign(self.negative)
            } else {
                other.sub_abs(self).with_sign(other.negative)
            }
        }
    }

    fn with_sign(self, negative: bool) -> Self {
        CtBigInt {
            negative,
            limbs: self.limbs,
        }
    }

    pub fn sub(&self, other: &CtBigInt) -> CtBigInt {
        let mut neg_other = other.clone();
        neg_other.negative = !neg_other.negative;
        self.add(&neg_other)
    }

    fn sub_abs(&self, other: &CtBigInt) -> CtBigInt {
        let mut borrow = 0i64;
        let len = self.limbs.len();
        let mut limbs = Vec::with_capacity(len);
        for i in 0..len {
            let a = self.limbs[i] as i64;
            let b = *other.limbs.get(i).unwrap_or(&0) as i64;
            let mut cur = a - b - borrow;
            if cur < 0 {
                cur += CTBI_BASE as i64;
                borrow = 1;
            } else {
                borrow = 0;
            }
            limbs.push(cur as u32);
        }
        CtBigInt {
            negative: false,
            limbs,
        }
        .normalize()
    }

    fn cmp_abs(&self, other: &CtBigInt) -> i8 {
        match self.limbs.len().cmp(&other.limbs.len()) {
            std::cmp::Ordering::Greater => 1,
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => {
                for (a, b) in self.limbs.iter().rev().zip(other.limbs.iter().rev()) {
                    match a.cmp(b) {
                        std::cmp::Ordering::Greater => return 1,
                        std::cmp::Ordering::Less => return -1,
                        std::cmp::Ordering::Equal => {}
                    }
                }
                0
            }
        }
    }

    pub fn mul(&self, other: &CtBigInt) -> CtBigInt {
        let mut out = CtBigInt::from_int(0);
        for (i, &limb) in other.limbs.iter().enumerate() {
            if limb == 0 {
                continue;
            }
            let mut part = self.mul_small(limb);
            for _ in 0..i {
                part = part.mul_small(CTBI_BASE as u32);
            }
            out = out.add(&part);
        }
        CtBigInt {
            negative: self.negative != other.negative,
            limbs: out.limbs,
        }
        .normalize()
    }

    pub fn neg(&self) -> CtBigInt {
        if self.limbs.len() == 1 && self.limbs[0] == 0 {
            self.clone()
        } else {
            CtBigInt {
                negative: !self.negative,
                limbs: self.limbs.clone(),
            }
        }
    }

    /// Total order (sign-aware, unlike the private magnitude-only `cmp_abs`).
    pub fn compare(&self, other: &CtBigInt) -> std::cmp::Ordering {
        match (self.negative, other.negative) {
            (false, true) => std::cmp::Ordering::Greater,
            (true, false) => std::cmp::Ordering::Less,
            (false, false) => match self.cmp_abs(other) {
                1 => std::cmp::Ordering::Greater,
                -1 => std::cmp::Ordering::Less,
                _ => std::cmp::Ordering::Equal,
            },
            (true, true) => match self.cmp_abs(other) {
                1 => std::cmp::Ordering::Less,
                -1 => std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Equal,
            },
        }
    }

    pub fn to_string_rep(&self) -> String {
        if self.limbs.len() == 1 && self.limbs[0] == 0 {
            return "0".to_string();
        }
        let mut s = String::new();
        let top = *self.limbs.last().unwrap();
        s.push_str(&top.to_string());
        for &limb in self.limbs.iter().rev().skip(1) {
            s.push_str(&format!("{:09}", limb));
        }
        if self.negative {
            format!("-{s}")
        } else {
            s
        }
    }
}

// ── CtDecimal: comptime/REPL tier-0 exact base-10 decimal ───────────────────
// Mirrors AOT `JetDecimal` (CommonTypes.rs) limb-for-limb so `to_string` and
// arithmetic match across tiers (D-DECIMAL1 / R12).

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CtDecimal {
    pub negative: bool,
    pub digits: Vec<u8>, // big-endian mantissa digits 0-9, no dot
    pub scale: u32,
}

impl CtDecimal {
    pub fn from_str(s: &str) -> Result<Self, String> {
        let t = s.trim();
        if t.is_empty() {
            return Err("empty Decimal string".to_string());
        }
        let (negative, body) = if let Some(rest) = t.strip_prefix('-') {
            (true, rest)
        } else if let Some(rest) = t.strip_prefix('+') {
            (false, rest)
        } else {
            (false, t)
        };
        let parts: Vec<&str> = body.split('.').collect();
        if parts.len() > 2 {
            return Err(format!("invalid Decimal string `{s}`"));
        }
        let (int_part, frac_part) = (parts[0], parts.get(1).copied().unwrap_or(""));
        if int_part.is_empty() && frac_part.is_empty() {
            return Err(format!("invalid Decimal string `{s}`"));
        }
        if !int_part.chars().all(|c| c.is_ascii_digit())
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
        let scale = frac_part.len() as u32;
        Ok(CtDecimal {
            negative,
            digits,
            scale,
        }
        .normalize())
    }

    fn normalize(mut self) -> Self {
        // Same law as AOT `JetDecimal::normalize` (CommonTypes.rs): trailing
        // fractional zeros drop with scale. Digits-only pop silently shifts
        // the radix point and violates D-DECIMAL1 / R12.
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

    fn align_scales(a: &CtDecimal, b: &CtDecimal) -> (CtDecimal, CtDecimal) {
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

    fn to_bigint(&self) -> CtBigInt {
        let mut s = String::new();
        for &d in &self.digits {
            s.push((b'0' + d) as char);
        }
        CtBigInt::from_str(&s).unwrap()
    }

    fn from_bigint(v: CtBigInt, scale: u32, negative: bool) -> CtDecimal {
        let s = v.to_string_rep();
        let body = if s.starts_with('-') { &s[1..] } else { &s };
        let digits: Vec<u8> = body.bytes().map(|b| b - b'0').collect();
        CtDecimal {
            negative,
            digits,
            scale,
        }
        .normalize()
    }

    pub fn add(&self, other: &CtDecimal) -> CtDecimal {
        let (a, b) = CtDecimal::align_scales(self, other);
        let negative = if a.negative == b.negative {
            a.negative
        } else if a.to_bigint().cmp_abs(&b.to_bigint()) >= 0 {
            a.negative
        } else {
            b.negative
        };
        if a.negative == b.negative {
            CtDecimal::from_bigint(a.to_bigint().add(&b.to_bigint()), a.scale, negative)
        } else {
            let diff = if a.to_bigint().cmp_abs(&b.to_bigint()) >= 0 {
                a.to_bigint().sub_abs(&b.to_bigint())
            } else {
                b.to_bigint().sub_abs(&a.to_bigint())
            };
            CtDecimal::from_bigint(diff, a.scale, negative)
        }
    }

    pub fn sub(&self, other: &CtDecimal) -> CtDecimal {
        let mut neg = other.clone();
        neg.negative = !neg.negative;
        self.add(&neg)
    }

    pub fn mul(&self, other: &CtDecimal) -> CtDecimal {
        CtDecimal::from_bigint(
            self.to_bigint().mul(&other.to_bigint()),
            self.scale + other.scale,
            self.negative != other.negative,
        )
    }

    pub fn to_string_rep(&self) -> String {
        if self.digits == [0] {
            return if self.scale == 0 {
                "0".to_string()
            } else {
                format!("0.{}", "0".repeat(self.scale as usize))
            };
        }
        let mut int_digits = self.digits.clone();
        let frac_len = self.scale as usize;
        let sign = if self.negative { "-" } else { "" };
        if frac_len == 0 {
            let s: String = int_digits.iter().map(|d| (b'0' + *d) as char).collect();
            return format!("{sign}{s}");
        }
        if int_digits.len() <= frac_len {
            let pad = frac_len - int_digits.len() + 1;
            int_digits.splice(0..0, vec![0; pad]);
        }
        let split = int_digits.len() - frac_len;
        let (whole, frac) = int_digits.split_at(split);
        let w: String = whole.iter().map(|d| (b'0' + *d) as char).collect();
        let f: String = frac.iter().map(|d| (b'0' + *d) as char).collect();
        format!("{sign}{w}.{f}")
    }

    pub fn to_value(&self) -> crate::AST::CtValue {
        crate::AST::CtValue::Struct {
            type_name: crate::Syntax::TYPE_DECIMAL.to_string(),
            fields: vec![
                ("negative".to_string(), crate::AST::CtValue::Bool(self.negative)),
                (
                    "digits".to_string(),
                    crate::AST::CtValue::Str(
                        self.digits.iter().map(|d| (b'0' + *d) as char).collect(),
                    ),
                ),
                ("scale".to_string(), crate::AST::CtValue::Int(self.scale as i64)),
            ],
        }
    }

    pub fn from_value(value: &crate::AST::CtValue) -> Result<Self, String> {
        let crate::AST::CtValue::Struct { type_name, fields } = value else {
            return Err("expected Decimal".to_string());
        };
        if type_name != crate::Syntax::TYPE_DECIMAL {
            return Err(format!("expected Decimal, found {type_name}"));
        }
        let mut negative = false;
        let mut digits = String::new();
        let mut scale = 0u32;
        for (name, field) in fields {
            match (name.as_str(), field) {
                ("negative", crate::AST::CtValue::Bool(flag)) => negative = *flag,
                ("digits", crate::AST::CtValue::Str(text)) => digits = text.clone(),
                ("scale", crate::AST::CtValue::Int(n)) if *n >= 0 => scale = *n as u32,
                _ => return Err(format!("malformed Decimal.{name}")),
            }
        }
        if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
            return Err("malformed Decimal.digits".to_string());
        }
        Ok(CtDecimal {
            negative,
            digits: digits.bytes().map(|b| b - b'0').collect(),
            scale,
        }
        .normalize())
    }
}
