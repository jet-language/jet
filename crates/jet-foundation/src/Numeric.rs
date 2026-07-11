//! D-BIGINT1 / D-DECIMAL1: arbitrary-precision `BigInt` and base-10 `Decimal`.
//! Shared name/method tables for sema and codegen.

use crate::Syntax;
use crate::AST::{Expr, Marker, Type};

pub const MONEY_LINT_NAMES: &[&str] =
    &["price", "cost", "amount", "total", "fee", "balance", "tax"];

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
