//! D-INTBIG1 / D-DECIMAL1: arbitrary-precision default `Int` and base-10 `Decimal`.
//! Shared name/method tables for sema and codegen.

use crate::JSONNumber::{json_decimal_lexeme, json_exact_integer_text};
use crate::Syntax;
use crate::AST::{Expr, Marker, Type};

pub const MONEY_LINT_NAMES: &[&str] = &["price", "cost", "amount", "fee", "balance", "tax"];

pub fn is_decimal_type_name(name: &str) -> bool {
    name == Syntax::TYPE_DECIMAL
}

pub fn type_is_decimal(ty: &Type) -> bool {
    matches!(ty, Type::Named(n) if is_decimal_type_name(n))
}

pub fn is_exact_type_name(name: &str) -> bool {
    matches!(name, Syntax::TYPE_DECIMAL | Syntax::TYPE_FRACTION)
}

pub fn exact_type_from_name(name: &str) -> Option<Type> {
    is_exact_type_name(name).then(|| Type::Named(name.to_string()))
}

/// D-SHAPE-CONVERT1=A: exact-number conversion routing. The returned owner
/// and function name describe the shared precise carrier that performs the
/// conversion; source-owned `to_*` spellings never enter this table.
pub fn precise_conversion_route(
    target_name: &str,
    source_name: &str,
) -> Option<(&'static str, &'static str)> {
    match (target_name, source_name) {
        (Syntax::TYPE_DECIMAL, "Int") => Some((Syntax::TYPE_DECIMAL, "from_int")),
        (Syntax::TYPE_DECIMAL, "Float") => Some((Syntax::TYPE_DECIMAL, "from_float")),
        (Syntax::TYPE_DECIMAL, Syntax::TYPE_FRACTION) => {
            Some((Syntax::TYPE_DECIMAL, "from_fraction"))
        }
        (Syntax::TYPE_FRACTION, "Int") => Some((Syntax::TYPE_FRACTION, "from_int")),
        (Syntax::TYPE_FRACTION, "Float") => Some((Syntax::TYPE_FRACTION, "from_float")),
        (Syntax::TYPE_FRACTION, Syntax::TYPE_DECIMAL) => {
            Some((Syntax::TYPE_FRACTION, "from_decimal"))
        }
        ("Float", Syntax::TYPE_DECIMAL) => Some((Syntax::TYPE_DECIMAL, "to_float")),
        ("Float", Syntax::TYPE_FRACTION) => Some((Syntax::TYPE_FRACTION, "to_float")),
        ("Int", Syntax::TYPE_DECIMAL) => Some((Syntax::TYPE_DECIMAL, "to_int")),
        ("Int", Syntax::TYPE_FRACTION) => Some((Syntax::TYPE_FRACTION, "to_int")),
        _ => None,
    }
}

/// Return the exact conversion result shape. Exact-number conversions are
/// explicit and total for representable values; an impossible conversion
/// stops loudly at runtime rather than rounding or changing the type.
pub fn precise_conversion_return(
    target: &Type,
    method: &str,
    nargs: usize,
) -> Option<Option<Type>> {
    if nargs != 1 {
        return None;
    }
    let source_name = Syntax::numeric_conversion_source(method)?;
    let target_name = match target {
        Type::Int => "Int",
        Type::Float => "Float",
        Type::Named(name) if is_exact_type_name(name) => name.as_str(),
        _ => return None,
    };
    precise_conversion_route(target_name, source_name).map(|_| Some(target.clone()))
}

pub fn is_money_like_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    MONEY_LINT_NAMES.iter().any(|m| lower.contains(m))
}

#[cfg(test)]
mod tests {
    use super::{is_money_like_name, CtFraction};

    #[test]
    fn total_is_not_assumed_to_mean_money() {
        assert!(!is_money_like_name("total"));
        assert!(is_money_like_name("total_price"));
    }

    #[test]
    fn fractions_use_decimal_display_only_when_finite() {
        assert_eq!(CtFraction::new(7, 2).unwrap().to_string_rep(), "3.5");
        assert_eq!(CtFraction::new(-1, 8).unwrap().to_string_rep(), "-0.125");
        assert_eq!(CtFraction::new(1, 3).unwrap().to_string_rep(), "1/3");
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

pub fn decimal_method_return(method: &str, nargs: usize) -> Option<Option<Type>> {
    let decimal = || Type::Named(Syntax::TYPE_DECIMAL.to_string());
    match (method, nargs) {
        ("add" | "sub" | "mul", 1) => Some(Some(decimal())),
        ("div", 1) => Some(Some(Type::Named(Syntax::TYPE_FRACTION.to_string()))),
        ("round" | "floor" | "ceil", 0) => Some(Some(decimal())),
        ("equal", 1) => Some(Some(Type::Bool)),
        ("to_string", 0) => Some(Some(Type::String)),
        _ => None,
    }
}

/// D-NUMTYPE1=A: an exact ratio. Arithmetic answers another Fraction; the two
/// parts answer whole numbers; comparison answers a yes or no.
pub fn fraction_method_return(method: &str, nargs: usize) -> Option<Option<Type>> {
    let fraction = || Type::Named(Syntax::TYPE_FRACTION.to_string());
    match (method, nargs) {
        ("add" | "sub" | "mul" | "div", 1) => Some(Some(fraction())),
        ("numerator" | "denominator", 0) => Some(Some(Type::Int)),
        ("to_string", 0) => Some(Some(Type::String)),
        ("to_float", 0) => Some(Some(Type::Float)),
        ("is_zero", 0) => Some(Some(Type::Bool)),
        ("equal", 1) => Some(Some(Type::Bool)),
        _ => None,
    }
}

// ── CtFraction: comptime/REPL tier-0 exact ratio ────────────────────────────
//
// Mirrors `JetFraction` in the Prelude so a Fraction computed at comptime
// prints byte-identical to the same expression run through the AOT path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CtFraction {
    pub numerator: i64,
    pub denominator: i64,
}

impl PartialOrd for CtFraction {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CtFraction {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        ((self.numerator as i128) * (other.denominator as i128)).cmp(
            &((other.numerator as i128) * (self.denominator as i128)),
        )
    }
}

impl CtFraction {
    pub fn new(numerator: i64, denominator: i64) -> Option<Self> {
        if denominator == 0 {
            return None;
        }
        let (mut n, mut d) = (numerator, denominator);
        if d < 0 {
            n = n.checked_neg()?;
            d = d.checked_neg()?;
        }
        let mut a = n.checked_abs()?;
        let mut b = d;
        while b != 0 {
            let r = a % b;
            a = b;
            b = r;
        }
        let divisor = if a == 0 { 1 } else { a };
        Some(Self {
            numerator: n / divisor,
            denominator: d / divisor,
        })
    }

    pub fn add(&self, other: &Self) -> Option<Self> {
        let left = self.numerator.checked_mul(other.denominator)?;
        let right = other.numerator.checked_mul(self.denominator)?;
        Self::new(
            left.checked_add(right)?,
            self.denominator.checked_mul(other.denominator)?,
        )
    }

    pub fn sub(&self, other: &Self) -> Option<Self> {
        let left = self.numerator.checked_mul(other.denominator)?;
        let right = other.numerator.checked_mul(self.denominator)?;
        Self::new(
            left.checked_sub(right)?,
            self.denominator.checked_mul(other.denominator)?,
        )
    }

    pub fn mul(&self, other: &Self) -> Option<Self> {
        Self::new(
            self.numerator.checked_mul(other.numerator)?,
            self.denominator.checked_mul(other.denominator)?,
        )
    }

    pub fn div(&self, other: &Self) -> Option<Self> {
        Self::new(
            self.numerator.checked_mul(other.denominator)?,
            self.denominator.checked_mul(other.numerator)?,
        )
    }

    /// Exact conversion from a finite binary64 value. Values whose exact
    /// denominator or numerator does not fit the resident Fraction carrier
    /// stay unavailable; no binary-to-decimal detour is allowed here.
    pub fn from_float(value: f64) -> Option<Self> {
        if !value.is_finite() {
            return None;
        }
        if value == 0.0 {
            return Some(Self {
                numerator: 0,
                denominator: 1,
            });
        }
        let bits = value.to_bits();
        let negative = (bits >> 63) != 0;
        let exponent = ((bits >> 52) & 0x7ff) as i32;
        let fraction = bits & ((1u64 << 52) - 1);
        let (significand, power) = if exponent == 0 {
            (fraction, -1074)
        } else {
            (fraction | (1u64 << 52), exponent - 1023 - 52)
        };
        let magnitude = i128::from(significand);
        let (numerator, denominator) = if power >= 0 {
            let numerator = magnitude.checked_shl(power as u32)?;
            (numerator, 1i128)
        } else {
            let denominator = 1i128.checked_shl((-power) as u32)?;
            (magnitude, denominator)
        };
        let numerator = if negative { numerator.checked_neg()? } else { numerator };
        Self::new(i64::try_from(numerator).ok()?, i64::try_from(denominator).ok()?)
    }

    pub fn to_int_exact(&self) -> Option<i64> {
        (self.denominator == 1).then_some(self.numerator)
    }

    pub fn from_int(value: i64) -> Option<Self> {
        Self::new(value, 1)
    }

    pub fn from_decimal(value: &CtDecimal) -> Option<Self> {
        value.to_fraction()
    }

    pub fn to_decimal(&self) -> Option<CtDecimal> {
        CtDecimal::from_fraction(self)
    }

    pub fn to_string_rep(&self) -> String {
        if let Some(decimal) = finite_decimal(self.numerator, self.denominator) {
            return decimal;
        }
        format!("{}/{}", self.numerator, self.denominator)
    }

    pub fn to_value(&self) -> crate::AST::CtValue {
        crate::AST::CtValue::Struct {
            type_name: Syntax::TYPE_FRACTION.to_string(),
            fields: vec![
                (
                    "numerator".to_string(),
                    crate::AST::CtValue::Int(self.numerator),
                ),
                (
                    "denominator".to_string(),
                    crate::AST::CtValue::Int(self.denominator),
                ),
            ],
        }
    }

    pub fn from_value(value: &crate::AST::CtValue) -> Result<Self, String> {
        let crate::AST::CtValue::Struct { type_name, fields } = value else {
            return Err("expected Fraction".to_string());
        };
        if type_name != Syntax::TYPE_FRACTION {
            return Err(format!("expected Fraction, found {type_name}"));
        }
        let mut numerator = 0i64;
        let mut denominator = 1i64;
        for (name, field) in fields {
            if let crate::AST::CtValue::Int(n) = field {
                if name == "numerator" {
                    numerator = *n;
                } else if name == "denominator" {
                    denominator = *n;
                }
            }
        }
        Ok(Self {
            numerator,
            denominator,
        })
    }
}

/// Render a word-sized ratio as a finite decimal when its reduced denominator
/// has no prime factors other than 2 and 5. Long division keeps the formatter
/// exact without multiplying into a wider-than-word temporary.
fn finite_decimal(numerator: i64, denominator: i64) -> Option<String> {
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
    let mut remainder = numerator.unsigned_abs() as u128 % denominator;
    let whole = numerator.unsigned_abs() as u128 / denominator;
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

// ── CtBigInt: comptime/REPL tier-0 arbitrary-precision integer ──────────────
//
// Mirrors the exact integer carrier in
// `crates/jet-codegen/src/Prelude/CoreLib/JetStd/CommonTypes.rs` limb-for-limb
// (sign-magnitude, little-endian base 10^9) so a spilled `Int` computed at
// comptime prints byte-identical to the same expression run through the AOT path
// (R12 parity). Kept here (not in `jet-comptime`) because `CtValue` — shared
// by every seam crate — needs the type in its own definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CtBigInt {
    pub negative: bool,
    pub limbs: Vec<u32>, // little-endian base 10^9
}

const CTBI_BASE: u64 = 1_000_000_000;

impl CtBigInt {
    pub fn from_u64(mut value: u64) -> Self {
        if value == 0 {
            return Self::from_int(0);
        }
        let mut limbs = Vec::new();
        while value > 0 {
            limbs.push((value % CTBI_BASE) as u32);
            value /= CTBI_BASE;
        }
        Self {
            negative: false,
            limbs,
        }
    }

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
            return Err("empty exact Int string".to_string());
        }
        let (negative, body) = if let Some(rest) = t.strip_prefix('-') {
            (true, rest)
        } else if let Some(rest) = t.strip_prefix('+') {
            (false, rest)
        } else {
            (false, t)
        };
        if body.is_empty() || !body.chars().all(|c| c.is_ascii_digit()) {
            return Err(format!("invalid exact Int string `{s}`"));
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

    /// Project one JSON number token into the arbitrary-precision default
    /// `Int` carrier without passing through `Float`.
    pub fn from_json_number(s: &str) -> Result<Self, String> {
        Self::from_str(&json_exact_integer_text(s)?)
    }

    /// Parse one source integer literal. The lexer keeps the original spelling
    /// so exact `Int` can survive the i64 fast path; this is the one canonical
    /// radix/underscore parser used by sema, comptime, and TIR lowering.
    pub fn from_literal(s: &str) -> Result<Self, String> {
        let text = s.replace('_', "");
        let (negative, body) = if let Some(rest) = text.strip_prefix('-') {
            (true, rest)
        } else if let Some(rest) = text.strip_prefix('+') {
            (false, rest)
        } else {
            (false, text.as_str())
        };
        let (radix, digits) =
            if let Some(rest) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
                (16, rest)
            } else if let Some(rest) = body.strip_prefix("0o").or_else(|| body.strip_prefix("0O")) {
                (8, rest)
            } else if let Some(rest) = body.strip_prefix("0b").or_else(|| body.strip_prefix("0B")) {
                (2, rest)
            } else {
                return Self::from_str(&text);
            };
        if digits.is_empty() {
            return Err(format!("invalid integer literal `{s}`"));
        }
        let mut value = Self::from_int(0);
        for digit in digits.chars() {
            let digit = digit
                .to_digit(radix)
                .ok_or_else(|| format!("invalid integer literal `{s}`"))?;
            value = value.mul_small(radix).add_small(digit);
        }
        value.negative = negative && !value.is_zero();
        Ok(value)
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

    pub fn is_zero(&self) -> bool {
        self.limbs.len() == 1 && self.limbs[0] == 0
    }

    /// Return the value when it fits in the resident machine-word payload.
    /// The tagged runtime representation reserves the outer signed bit, so
    /// callers pass the 63-bit bound explicitly instead of silently narrowing.
    pub fn try_i64(&self) -> Option<i64> {
        let mut value = 0u128;
        for &limb in self.limbs.iter().rev() {
            value = value.checked_mul(CTBI_BASE as u128)?;
            value = value.checked_add(limb as u128)?;
        }
        let signed = if self.negative {
            let magnitude = i128::try_from(value).ok()?;
            -magnitude
        } else {
            i128::try_from(value).ok()?
        };
        i64::try_from(signed).ok()
    }

    pub fn try_i128(&self) -> Option<i128> {
        let mut value = 0u128;
        for &limb in self.limbs.iter().rev() {
            value = value.checked_mul(CTBI_BASE as u128)?;
            value = value.checked_add(limb as u128)?;
        }
        let magnitude = i128::try_from(value).ok()?;
        Some(if self.negative { -magnitude } else { magnitude })
    }

    /// Project the exact integer into the low 64 bits with the same
    /// two's-complement wrap used by the runtime `from_bits` ABI.
    pub fn wrapping_u64(&self) -> u64 {
        let magnitude = self.limbs.iter().rev().fold(0_u64, |value, &limb| {
            value
                .wrapping_mul(CTBI_BASE)
                .wrapping_add(limb as u64)
        });
        if self.negative {
            0_u64.wrapping_sub(magnitude)
        } else {
            magnitude
        }
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

    fn div_rem_small(&self, divisor: u32) -> (CtBigInt, u32) {
        let divisor = u64::from(divisor);
        let mut remainder = 0u64;
        let mut limbs = vec![0u32; self.limbs.len()];
        for index in (0..self.limbs.len()).rev() {
            let current = remainder * CTBI_BASE + u64::from(self.limbs[index]);
            limbs[index] = (current / divisor) as u32;
            remainder = current % divisor;
        }
        (
            CtBigInt {
                negative: false,
                limbs,
            }
            .normalize(),
            remainder as u32,
        )
    }

    fn bit_width(&self) -> usize {
        let mut value = self.abs();
        let mut width = 0usize;
        while !value.is_zero() {
            let (next, _) = value.div_rem_small(2);
            value = next;
            width += 1;
        }
        width
    }

    fn unsigned_bits(&self, width: usize) -> Vec<bool> {
        let mut value = self.abs();
        let mut bits = Vec::with_capacity(width);
        for _ in 0..width {
            let (next, remainder) = value.div_rem_small(2);
            bits.push(remainder != 0);
            value = next;
        }
        bits
    }

    fn from_unsigned_bits(bits: &[bool]) -> CtBigInt {
        let mut value = CtBigInt::from_int(0);
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

    fn from_twos_complement(mut bits: Vec<bool>) -> CtBigInt {
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
        Self::from_unsigned_bits(&bits).neg()
    }

    fn bitwise(&self, other: &CtBigInt, op: impl Fn(bool, bool) -> bool) -> CtBigInt {
        let width = self.bit_width().max(other.bit_width()).saturating_add(1);
        let left = self.twos_complement(width);
        let right = other.twos_complement(width);
        let bits = left
            .into_iter()
            .zip(right)
            .map(|(left, right)| op(left, right))
            .collect();
        Self::from_twos_complement(bits)
    }

    pub fn bit_and(&self, other: &CtBigInt) -> CtBigInt {
        self.bitwise(other, |left, right| left & right)
    }

    pub fn bit_or(&self, other: &CtBigInt) -> CtBigInt {
        self.bitwise(other, |left, right| left | right)
    }

    pub fn bit_xor(&self, other: &CtBigInt) -> CtBigInt {
        self.bitwise(other, |left, right| left ^ right)
    }

    pub fn bit_count(&self, width: u32, method: &str) -> Option<i64> {
        let width = usize::try_from(width).ok()?;
        if width == 0 {
            return None;
        }
        let bits = self.twos_complement(width);
        let ones = bits.iter().filter(|bit| **bit).count();
        let count = match method {
            "count_ones" => ones,
            "count_zeros" => width - ones,
            "leading_zeros" => bits.iter().rev().take_while(|bit| !**bit).count(),
            "trailing_zeros" => bits.iter().take_while(|bit| !**bit).count(),
            _ => return None,
        };
        i64::try_from(count).ok()
    }

    pub fn checked_widen(&self, target_f32: bool) -> Option<f64> {
        let precision = if target_f32 { 24 } else { 53 };
        let width = self.bit_width();
        let mut trailing = 0usize;
        let mut value = self.abs();
        while !value.is_zero() {
            let (next, remainder) = value.div_rem_small(2);
            if remainder != 0 {
                break;
            }
            trailing += 1;
            value = next;
        }
        if width > precision && trailing < width - precision {
            return None;
        }
        let value = self.to_string_rep().parse::<f64>().ok()?;
        if !value.is_finite() {
            return None;
        }
        if target_f32 {
            let value = value as f32;
            value.is_finite().then_some(value as f64)
        } else {
            Some(value)
        }
    }

    fn shift_count(&self) -> Option<usize> {
        let count = self.try_i64()?;
        (count >= 0).then_some(count as usize)
    }

    pub fn shl(&self, count: &CtBigInt) -> Option<CtBigInt> {
        let count = count.shift_count()?;
        let mut value = self.clone();
        for _ in 0..count {
            value = value.mul_small(2);
        }
        Some(value)
    }

    pub fn shr(&self, count: &CtBigInt) -> Option<CtBigInt> {
        let count = count.shift_count()?;
        let mut value = self.clone();
        for _ in 0..count {
            let (quotient, remainder) = value.abs().div_rem_small(2);
            value = if self.negative && remainder != 0 {
                quotient.add_small(1).neg()
            } else {
                quotient.with_sign(self.negative)
            };
        }
        Some(value)
    }

    pub fn is_even(&self) -> bool {
        self.div_rem_small(2).1 == 0
    }

    pub fn is_odd(&self) -> bool {
        !self.is_even()
    }

    pub fn digits(&self) -> i64 {
        let digits = self.to_string_rep().trim_start_matches('-').len();
        i64::try_from(digits).unwrap_or(i64::MAX)
    }

    pub fn leading_ones(&self) -> i64 {
        let width = 64usize.max(self.bit_width().saturating_add(1));
        let count = self
            .twos_complement(width)
            .iter()
            .rev()
            .take_while(|bit| **bit)
            .count();
        i64::try_from(count).unwrap_or(i64::MAX)
    }

    pub fn trailing_ones(&self) -> i64 {
        let width = 64usize.max(self.bit_width().saturating_add(1));
        let count = self
            .twos_complement(width)
            .iter()
            .take_while(|bit| **bit)
            .count();
        i64::try_from(count).unwrap_or(i64::MAX)
    }

    pub fn isqrt(&self) -> Option<CtBigInt> {
        if self.negative {
            return None;
        }
        if self.is_zero() {
            return Some(self.clone());
        }
        let one = CtBigInt::from_int(1);
        let two = CtBigInt::from_int(2);
        let mut root = one.clone();
        for _ in 0..self.bit_width().saturating_add(1) / 2 {
            root = root.mul_small(2);
        }
        loop {
            let quotient = self.div_rem(&root)?.0;
            let next = root.add(&quotient).div_rem(&two)?.0;
            if next.compare(&root) != std::cmp::Ordering::Less {
                break;
            }
            root = next;
        }
        while root.mul(&root).compare(self) == std::cmp::Ordering::Greater {
            root = root.sub(&one);
        }
        loop {
            let next = root.add(&one);
            if next.mul(&next).compare(self) == std::cmp::Ordering::Greater {
                break;
            }
            root = next;
        }
        Some(root)
    }

    pub fn pow(&self, exponent: &CtBigInt) -> Option<CtBigInt> {
        if exponent.negative {
            return None;
        }
        let mut exponent = exponent.clone();
        let mut base = self.clone();
        let mut result = CtBigInt::from_int(1);
        while !exponent.is_zero() {
            let (next, bit) = exponent.div_rem_small(2);
            if bit != 0 {
                result = result.mul(&base);
            }
            exponent = next;
            if !exponent.is_zero() {
                base = base.mul(&base);
            }
        }
        Some(result)
    }

    pub fn gcd(left: &CtBigInt, right: &CtBigInt) -> CtBigInt {
        let mut a = left.abs();
        let mut b = right.abs();
        while !b.is_zero() {
            let (_, remainder) = a.div_rem(&b).expect("gcd divisor is nonzero");
            a = b;
            b = remainder.abs();
        }
        a
    }

    pub fn lcm(left: &CtBigInt, right: &CtBigInt) -> CtBigInt {
        if left.is_zero() || right.is_zero() {
            return CtBigInt::from_int(0);
        }
        let divisor = Self::gcd(left, right);
        let quotient = left.abs().div_rem(&divisor).expect("lcm gcd is nonzero").0;
        quotient.mul(&right.abs())
    }

    pub fn binomial(n: &CtBigInt, k: &CtBigInt) -> Option<CtBigInt> {
        if n.negative || k.negative || k.compare(n) == std::cmp::Ordering::Greater {
            return None;
        }
        let other = n.sub(k);
        let limit = if k.compare(&other) == std::cmp::Ordering::Greater {
            other
        } else {
            k.clone()
        };
        let one = CtBigInt::from_int(1);
        let mut index = one.clone();
        let mut result = one.clone();
        while index.compare(&limit) != std::cmp::Ordering::Greater {
            let numerator = n.sub(&limit).add(&index);
            result = result.mul(&numerator).div_rem(&index)?.0;
            index = index.add(&one);
        }
        Some(result)
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

    /// Truncating quotient and remainder. Both outputs are normalized; the
    /// remainder carries the dividend sign, matching Rust's integer rules.
    pub fn div_rem(&self, other: &CtBigInt) -> Option<(CtBigInt, CtBigInt)> {
        if other.is_zero() {
            return None;
        }
        let divisor = other.abs();
        let dividend = self.abs();
        if dividend.cmp_abs(&divisor) < 0 {
            return Some((CtBigInt::from_int(0), self.clone()));
        }

        let mut quotient = vec![0u32; dividend.limbs.len()];
        let mut remainder = CtBigInt::from_int(0);
        for index in (0..dividend.limbs.len()).rev() {
            remainder.limbs.insert(0, dividend.limbs[index]);
            remainder = remainder.normalize();
            let mut low = 0u32;
            let mut high = (CTBI_BASE - 1) as u32;
            while low < high {
                let middle = low + (high - low) / 2 + 1;
                if divisor.mul_small(middle).cmp_abs(&remainder) <= 0 {
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
        let quotient = CtBigInt {
            negative: self.negative != other.negative,
            limbs: quotient,
        }
        .normalize();
        remainder.negative = self.negative && !remainder.is_zero();
        Some((quotient, remainder.normalize()))
    }

    /// Euclidean quotient and remainder. The remainder is always
    /// non-negative and smaller than the divisor magnitude; the quotient is
    /// adjusted from the truncating pair when the dividend is negative.
    pub fn div_rem_euclid(&self, other: &CtBigInt) -> Option<(CtBigInt, CtBigInt)> {
        let (mut quotient, mut remainder) = self.div_rem(other)?;
        if remainder.negative {
            remainder = remainder.add(&other.abs());
            let one = CtBigInt::from_int(1);
            quotient = if other.negative {
                quotient.add(&one)
            } else {
                quotient.sub(&one)
            };
        }
        Some((quotient, remainder))
    }

    pub fn abs(&self) -> CtBigInt {
        CtBigInt {
            negative: false,
            limbs: self.limbs.clone(),
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
// parity: guard tests/repl.rs::repl_decimal_exact_transcript

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CtDecimal {
    pub negative: bool,
    pub digits: Vec<u8>, // big-endian mantissa digits 0-9, no dot
    pub scale: u32,
}

impl CtDecimal {
    pub fn from_str(s: &str) -> Result<Self, String> {
        if s.trim().is_empty() {
            return Err("empty Decimal string".to_string());
        }
        if s.contains('e') || s.contains('E') {
            return Err(format!("invalid Decimal string `{s}`"));
        }
        let (negative, digits, scale) = json_decimal_lexeme(s)?;
        Ok(CtDecimal {
            negative,
            digits,
            scale,
        }
        .normalize())
    }

    pub fn from_int(value: i64) -> Self {
        Self::from_bigint(CtBigInt::from_int(value), 0, value < 0)
    }

    /// Preserve the exact binary64 value as a finite decimal. A binary value
    /// has a denominator that is a power of two, so multiplying by the same
    /// power of five gives an exact base-10 representation.
    pub fn from_float(value: f64) -> Option<Self> {
        if !value.is_finite() {
            return None;
        }
        if value == 0.0 {
            return Some(Self::from_int(0));
        }
        let bits = value.to_bits();
        let negative = (bits >> 63) != 0;
        let exponent = ((bits >> 52) & 0x7ff) as i32;
        let fraction = bits & ((1u64 << 52) - 1);
        let (significand, power) = if exponent == 0 {
            (fraction, -1074)
        } else {
            (fraction | (1u64 << 52), exponent - 1023 - 52)
        };
        let mut numerator = CtBigInt::from_u64(significand);
        if power >= 0 {
            for _ in 0..power {
                numerator = numerator.mul(&CtBigInt::from_int(2));
            }
            Some(Self::from_bigint(numerator, 0, negative))
        } else {
            let scale = u32::try_from(-power).ok()?;
            for _ in 0..scale {
                numerator = numerator.mul(&CtBigInt::from_int(5));
            }
            Some(Self::from_bigint(numerator, scale, negative))
        }
    }

    /// Project one JSON number token into an exact base-10 value. Unlike the
    /// ordinary constructor, this keeps the token's written scale.
    pub fn from_json_number(s: &str) -> Result<Self, String> {
        let (negative, digits, scale) = json_decimal_lexeme(s)?;
        Ok(CtDecimal {
            negative,
            digits,
            scale,
        })
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

    fn signed_bigint(&self) -> CtBigInt {
        let value = self.to_bigint();
        if self.negative {
            value.neg()
        } else {
            value
        }
    }

    fn scale_factor(scale: u32) -> CtBigInt {
        let ten = CtBigInt::from_int(10);
        let mut factor = CtBigInt::from_int(1);
        for _ in 0..scale {
            factor = factor.mul(&ten);
        }
        factor
    }

    fn from_signed_bigint(value: CtBigInt, scale: u32) -> CtDecimal {
        let negative = value.negative;
        CtDecimal::from_bigint(value.abs(), scale, negative)
    }

    /// Exact quotient in the shared rational carrier. The resident Fraction
    /// keeps word-sized parts, so callers must handle a value that cannot be
    /// represented instead of silently narrowing it.
    pub fn to_fraction(&self) -> Option<CtFraction> {
        let numerator = self.signed_bigint().try_i64()?;
        let denominator = Self::scale_factor(self.scale).try_i64()?;
        CtFraction::new(numerator, denominator)
    }

    pub fn div(&self, other: &CtDecimal) -> Option<CtFraction> {
        self.to_fraction()?.div(&other.to_fraction()?)
    }

    fn quotient_remainder(&self) -> (CtBigInt, CtBigInt, CtBigInt) {
        let denominator = Self::scale_factor(self.scale);
        let (quotient, remainder) = self
            .signed_bigint()
            .div_rem(&denominator)
            .expect("Decimal scale denominator is nonzero");
        (quotient, remainder, denominator)
    }

    /// Exact integral projection. Fractional input has no integral result.
    pub fn to_int_exact(&self) -> Option<CtBigInt> {
        let (quotient, remainder, _) = self.quotient_remainder();
        remainder.is_zero().then_some(quotient)
    }

    /// Floor, ceiling, and nearest rounding all operate on the full decimal
    /// digits. Nearest ties go away from zero, matching `Float.round()`.
    pub fn floor_int(&self) -> CtBigInt {
        let (quotient, remainder, _) = self.quotient_remainder();
        if self.negative && !remainder.is_zero() {
            quotient.sub(&CtBigInt::from_int(1))
        } else {
            quotient
        }
    }

    pub fn ceil_int(&self) -> CtBigInt {
        let (quotient, remainder, _) = self.quotient_remainder();
        if !self.negative && !remainder.is_zero() {
            quotient.add(&CtBigInt::from_int(1))
        } else {
            quotient
        }
    }

    pub fn round_int(&self) -> CtBigInt {
        let (quotient, remainder, denominator) = self.quotient_remainder();
        let magnitude_remainder = remainder.abs();
        let doubled = magnitude_remainder.mul(&CtBigInt::from_int(2));
        let mut magnitude = quotient.abs();
        if doubled.compare(&denominator) != std::cmp::Ordering::Less {
            magnitude = magnitude.add(&CtBigInt::from_int(1));
        }
        if self.negative {
            magnitude.neg()
        } else {
            magnitude
        }
    }

    pub fn floor(&self) -> CtDecimal {
        Self::from_signed_bigint(self.floor_int(), 0)
    }

    pub fn ceil(&self) -> CtDecimal {
        Self::from_signed_bigint(self.ceil_int(), 0)
    }

    pub fn round(&self) -> CtDecimal {
        Self::from_signed_bigint(self.round_int(), 0)
    }

    /// Build the exact finite decimal represented by a reduced Fraction.
    /// A repeating denominator has no finite Decimal value and returns None.
    pub fn from_fraction(fraction: &CtFraction) -> Option<Self> {
        let mut factors = fraction.denominator as u64;
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
        let denominator = CtBigInt::from_int(fraction.denominator);
        let numerator = CtBigInt::from_int(fraction.numerator);
        let scaled = numerator.mul(&Self::scale_factor(scale));
        let (digits, remainder) = scaled.div_rem(&denominator)?;
        if !remainder.is_zero() {
            return None;
        }
        Some(Self::from_signed_bigint(digits, scale))
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

    /// D-TYPE2-DEFAULT1: the one place an exact `Decimal` becomes an
    /// approximate `Float`, at the irrational-result math functions that leave
    /// the exact world. Rounding happens exactly once, from the full digit
    /// string, so no intermediate step loses precision the value still had.
    pub fn to_f64(&self) -> f64 {
        self.to_string_rep().parse::<f64>().unwrap_or(f64::NAN)
    }

    pub fn to_value(&self) -> crate::AST::CtValue {
        crate::AST::CtValue::Struct {
            type_name: crate::Syntax::TYPE_DECIMAL.to_string(),
            fields: vec![
                (
                    "negative".to_string(),
                    crate::AST::CtValue::Bool(self.negative),
                ),
                (
                    "digits".to_string(),
                    crate::AST::CtValue::Str(
                        self.digits.iter().map(|d| (b'0' + *d) as char).collect(),
                    ),
                ),
                (
                    "scale".to_string(),
                    crate::AST::CtValue::Int(self.scale as i64),
                ),
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
        })
    }
}
