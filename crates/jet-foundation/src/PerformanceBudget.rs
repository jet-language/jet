//! D-PERFBUDGET-REPORT1: shared, deterministic performance-budget math.
//!
//! This module is deliberately in `jet-foundation`: producers (`jet budget`),
//! proof readers (`jet prove`), and future dossier readers must call the same
//! implementation.  It contains no rendering or filesystem policy.

use crate::SHA256;
use std::cmp::Ordering;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalJson {
    Null,
    Bool(bool),
    Integer(String),
    String(String),
    Array(Vec<CanonicalJson>),
    Object(BTreeMap<String, CanonicalJson>),
}

impl CanonicalJson {
    pub fn integer(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if !valid_integer(&value) {
            return Err(format!("non-canonical integer `{value}`"));
        }
        Ok(Self::Integer(value))
    }

    pub fn object(fields: impl IntoIterator<Item = (String, CanonicalJson)>) -> Result<Self, String> {
        let mut out = BTreeMap::new();
        for (key, value) in fields {
            if out.insert(key.clone(), value).is_some() {
                return Err(format!("duplicate JSON key `{key}`"));
            }
        }
        Ok(Self::Object(out))
    }

    pub fn bytes(&self) -> Vec<u8> {
        let mut out = String::new();
        encode_json(self, &mut out);
        out.push('\n');
        out.into_bytes()
    }

    pub fn sha256(&self) -> String {
        SHA256::sha256_hex(&self.bytes())
    }

    pub fn parse_canonical(bytes: &[u8]) -> Result<Self, String> {
        let text = std::str::from_utf8(bytes).map_err(|_| "canonical JSON is not UTF-8")?;
        if text.as_bytes().starts_with(&[0xef, 0xbb, 0xbf]) { return Err("canonical JSON has a BOM".into()); }
        if !text.ends_with('\n') || text[..text.len() - 1].ends_with('\n') { return Err("canonical JSON must end in exactly one LF".into()); }
        let mut parser = JSONParser { bytes: &text.as_bytes()[..text.len() - 1], at: 0 };
        let value = parser.value()?;
        if parser.at != parser.bytes.len() { return Err("trailing bytes after canonical JSON value".into()); }
        if value.bytes() != bytes { return Err("JSON is valid but not A-canonical".into()); }
        Ok(value)
    }
}

struct JSONParser<'a> { bytes: &'a [u8], at: usize }
impl<'a> JSONParser<'a> {
    fn peek(&self) -> Option<u8> { self.bytes.get(self.at).copied() }
    fn take(&mut self) -> Option<u8> { let byte = self.peek()?; self.at += 1; Some(byte) }
    fn value(&mut self) -> Result<CanonicalJson, String> {
        match self.peek() {
            Some(b'n') => { self.literal(b"null")?; Ok(CanonicalJson::Null) }
            Some(b't') => { self.literal(b"true")?; Ok(CanonicalJson::Bool(true)) }
            Some(b'f') => { self.literal(b"false")?; Ok(CanonicalJson::Bool(false)) }
            Some(b'"') => Ok(CanonicalJson::String(self.string()?)),
            Some(b'[') => self.array(), Some(b'{') => self.object(),
            Some(b'-' | b'0'..=b'9') => self.integer(),
            Some(_) => Err("invalid canonical JSON token".into()), None => Err("unexpected end of JSON".into()),
        }
    }
    fn literal(&mut self, literal: &[u8]) -> Result<(), String> {
        if self.bytes.get(self.at..self.at + literal.len()) != Some(literal) { return Err("invalid JSON literal".into()); }
        self.at += literal.len(); Ok(())
    }
    fn integer(&mut self) -> Result<CanonicalJson, String> {
        let start = self.at;
        if self.peek() == Some(b'-') { self.at += 1; }
        while matches!(self.peek(), Some(b'0'..=b'9')) { self.at += 1; }
        let text = std::str::from_utf8(&self.bytes[start..self.at]).map_err(|_| "invalid integer")?;
        CanonicalJson::integer(text)
    }
    fn string(&mut self) -> Result<String, String> {
        if self.take() != Some(b'"') { return Err("expected JSON string".into()); }
        let mut raw = Vec::new();
        loop {
            match self.take() {
                Some(b'"') => return String::from_utf8(raw).map_err(|_| "JSON string is not UTF-8".into()),
                Some(b'\\') => match self.take() {
                    Some(b'"') => raw.push(b'"'), Some(b'\\') => raw.push(b'\\'),
                    Some(b'b') => raw.push(8), Some(b'f') => raw.push(12), Some(b'n') => raw.push(b'\n'),
                    Some(b'r') => raw.push(b'\r'), Some(b't') => raw.push(b'\t'),
                    Some(b'u') => {
                        let digits = self.bytes.get(self.at..self.at + 4).ok_or("short Unicode escape")?;
                        self.at += 4;
                        let hex = std::str::from_utf8(digits).map_err(|_| "invalid Unicode escape")?;
                        let scalar = u32::from_str_radix(hex, 16).map_err(|_| "invalid Unicode escape")?;
                        if (0xd800..=0xdfff).contains(&scalar) { return Err("unpaired surrogate in JSON string".into()); }
                        let ch = char::from_u32(scalar).ok_or("invalid Unicode scalar")?;
                        let mut encoded = [0; 4]; raw.extend_from_slice(ch.encode_utf8(&mut encoded).as_bytes());
                    }
                    _ => return Err("invalid JSON string escape".into()),
                },
                Some(byte) if byte < 0x20 => return Err("unescaped control in JSON string".into()),
                Some(byte) => raw.push(byte), None => return Err("unterminated JSON string".into()),
            }
        }
    }
    fn array(&mut self) -> Result<CanonicalJson, String> {
        self.at += 1; let mut values = Vec::new();
        if self.peek() == Some(b']') { self.at += 1; return Ok(CanonicalJson::Array(values)); }
        loop {
            values.push(self.value()?);
            match self.take() { Some(b',') => {}, Some(b']') => return Ok(CanonicalJson::Array(values)), _ => return Err("invalid JSON array".into()) }
        }
    }
    fn object(&mut self) -> Result<CanonicalJson, String> {
        self.at += 1; let mut values = BTreeMap::new();
        if self.peek() == Some(b'}') { self.at += 1; return Ok(CanonicalJson::Object(values)); }
        loop {
            let key = self.string()?;
            if self.take() != Some(b':') { return Err("missing JSON object colon".into()); }
            let value = self.value()?;
            if values.insert(key.clone(), value).is_some() { return Err(format!("duplicate JSON key `{key}`")); }
            match self.take() { Some(b',') => {}, Some(b'}') => return Ok(CanonicalJson::Object(values)), _ => return Err("invalid JSON object".into()) }
        }
    }
}

fn valid_integer(value: &str) -> bool {
    if value == "0" {
        return true;
    }
    let digits = value.strip_prefix('-').unwrap_or(value);
    !digits.is_empty()
        && !digits.starts_with('0')
        && digits.bytes().all(|byte| byte.is_ascii_digit())
}

fn encode_json(value: &CanonicalJson, out: &mut String) {
    match value {
        CanonicalJson::Null => out.push_str("null"),
        CanonicalJson::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        CanonicalJson::Integer(value) => out.push_str(value),
        CanonicalJson::String(value) => encode_string(value, out),
        CanonicalJson::Array(values) => {
            out.push('[');
            for (index, value) in values.iter().enumerate() {
                if index != 0 { out.push(','); }
                encode_json(value, out);
            }
            out.push(']');
        }
        CanonicalJson::Object(fields) => {
            out.push('{');
            for (index, (key, value)) in fields.iter().enumerate() {
                if index != 0 { out.push(','); }
                encode_string(key, out);
                out.push(':');
                encode_json(value, out);
            }
            out.push('}');
        }
    }
}

fn encode_string(value: &str, out: &mut String) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch <= '\u{1f}' => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
}

/// A signed, arbitrary precision wire integer. Limbs are little-endian base 1e9.
/// Zero has sign 0 and no limbs; all other values have sign -1 or 1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BigInt { sign: i8, limbs: Vec<u32> }

impl BigInt {
    pub fn zero() -> Self { Self { sign: 0, limbs: Vec::new() } }
    pub fn one() -> Self { Self::from_i128(1) }
    pub fn from_i128(value: i128) -> Self {
        Self::parse(&value.to_string()).expect("i128 decimal is canonical")
    }
    pub fn parse(text: &str) -> Result<Self, String> {
        if !valid_integer(text) { return Err(format!("non-canonical integer `{text}`")); }
        let (sign, digits) = match text.strip_prefix('-') { Some(v) => (-1, v), None => (1, text) };
        if digits == "0" { return Ok(Self::zero()); }
        let mut limbs = Vec::new();
        let mut end = digits.len();
        while end != 0 {
            let start = end.saturating_sub(9);
            limbs.push(digits[start..end].parse::<u32>().map_err(|_| "invalid integer")?);
            end = start;
        }
        Ok(Self { sign, limbs })
    }
    pub fn parse_source(text: &str) -> Result<Self, String> {
        let (negative, unsigned) = match text.strip_prefix('-') {
            Some(value) => (true, value),
            None => (false, text),
        };
        let (radix, digits) = if let Some(value) = unsigned.strip_prefix("0x") {
            (16, value)
        } else if let Some(value) = unsigned.strip_prefix("0o") {
            (8, value)
        } else if let Some(value) = unsigned.strip_prefix("0b") {
            (2, value)
        } else {
            (10, unsigned)
        };
        let mut value = Self::zero();
        let mut saw_digit = false;
        for ch in digits
            .chars()
            .filter(|ch| *ch != crate::Syntax::DIGIT_SEPARATOR)
        {
            let digit = ch
                .to_digit(radix)
                .ok_or_else(|| format!("invalid base-{radix} integer `{text}`"))?;
            value = value
                .mul_small(radix)
                .add(&Self::from_i128(i128::from(digit)));
            saw_digit = true;
        }
        if !saw_digit {
            return Err(format!("invalid integer `{text}`"));
        }
        Ok(if negative { value.neg() } else { value })
    }
    pub fn is_zero(&self) -> bool { self.sign == 0 }
    pub fn is_negative(&self) -> bool { self.sign < 0 }
    pub fn abs(&self) -> Self { let mut out = self.clone(); if out.sign < 0 { out.sign = 1; } out }
    pub fn neg(&self) -> Self { let mut out = self.clone(); out.sign = -out.sign; out }
    fn normalize(&mut self) { while self.limbs.last() == Some(&0) { self.limbs.pop(); } if self.limbs.is_empty() { self.sign = 0; } }
    fn abs_cmp(&self, rhs: &Self) -> Ordering {
        self.limbs.len().cmp(&rhs.limbs.len()).then_with(|| self.limbs.iter().rev().cmp(rhs.limbs.iter().rev()))
    }
    fn abs_add(&self, rhs: &Self) -> Self {
        const B: u64 = 1_000_000_000;
        let mut out = Vec::with_capacity(self.limbs.len().max(rhs.limbs.len()) + 1); let mut carry = 0;
        for i in 0..self.limbs.len().max(rhs.limbs.len()) { let n = u64::from(*self.limbs.get(i).unwrap_or(&0)) + u64::from(*rhs.limbs.get(i).unwrap_or(&0)) + carry; out.push((n % B) as u32); carry = n / B; }
        if carry != 0 { out.push(carry as u32); } Self { sign: 1, limbs: out }
    }
    fn abs_sub(&self, rhs: &Self) -> Self { // |self| >= |rhs|
        const B: i64 = 1_000_000_000; let mut out = Vec::with_capacity(self.limbs.len()); let mut borrow = 0;
        for i in 0..self.limbs.len() { let mut n = i64::from(self.limbs[i]) - i64::from(*rhs.limbs.get(i).unwrap_or(&0)) - borrow; if n < 0 { n += B; borrow = 1; } else { borrow = 0; } out.push(n as u32); }
        let mut value = Self { sign: 1, limbs: out }; value.normalize(); value
    }
    pub fn add(&self, rhs: &Self) -> Self {
        if self.sign == 0 { return rhs.clone(); } if rhs.sign == 0 { return self.clone(); }
        if self.sign == rhs.sign { let mut out = self.abs_add(rhs); out.sign = self.sign; out }
        else { match self.abs_cmp(rhs) { Ordering::Greater => { let mut out = self.abs_sub(rhs); out.sign = self.sign; out }, Ordering::Less => { let mut out = rhs.abs_sub(self); out.sign = rhs.sign; out }, Ordering::Equal => Self::zero() } }
    }
    pub fn sub(&self, rhs: &Self) -> Self { self.add(&rhs.neg()) }
    pub fn mul(&self, rhs: &Self) -> Self {
        const B: u64 = 1_000_000_000; if self.is_zero() || rhs.is_zero() { return Self::zero(); }
        let mut out = vec![0u64; self.limbs.len() + rhs.limbs.len()];
        for (i, &a) in self.limbs.iter().enumerate() { let mut carry = 0u64; for (j, &b) in rhs.limbs.iter().enumerate() { let at = i+j; let n = out[at] + u64::from(a)*u64::from(b) + carry; out[at] = n%B; carry=n/B; } out[i+rhs.limbs.len()] += carry; }
        let mut value = Self { sign: self.sign * rhs.sign, limbs: out.into_iter().map(|v| v as u32).collect() }; value.normalize(); value
    }
    fn mul_small(&self, rhs: u32) -> Self { if rhs == 0 || self.is_zero() { return Self::zero(); } let mut out=Vec::with_capacity(self.limbs.len()+1); let mut carry=0u64; for &a in &self.limbs { let n=u64::from(a)*u64::from(rhs)+carry; out.push((n%1_000_000_000) as u32); carry=n/1_000_000_000; } if carry>0 { out.push(carry as u32); } Self { sign: 1, limbs: out } }
    pub fn div_rem_abs(&self, rhs: &Self) -> Result<(Self, Self), String> {
        if rhs.is_zero() { return Err("integer division by zero".into()); } let divisor=rhs.abs(); let dividend=self.abs();
        if dividend.abs_cmp(&divisor)==Ordering::Less { return Ok((Self::zero(), dividend)); }
        let mut quotient=vec![0u32; dividend.limbs.len()]; let mut remainder=Self::zero();
        for i in (0..dividend.limbs.len()).rev() { remainder.limbs.insert(0, dividend.limbs[i]); remainder.sign=1; remainder.normalize(); let mut lo=0u32; let mut hi=999_999_999u32; while lo<hi { let mid=lo+(hi-lo)/2+1; if divisor.mul_small(mid).abs_cmp(&remainder)!=Ordering::Greater { lo=mid; } else { hi=mid-1; } } quotient[i]=lo; if lo!=0 { remainder=remainder.abs_sub(&divisor.mul_small(lo)); } }
        let mut q=Self { sign: if quotient.iter().all(|x|*x==0){0}else{1}, limbs:quotient }; q.normalize(); Ok((q,remainder))
    }
    pub fn exact_div(&self, rhs: &Self) -> Result<Self, String> { let (mut q,r)=self.div_rem_abs(rhs)?; if !r.is_zero(){return Err("integer division is not exact".into());} q.sign=self.sign*rhs.sign; q.normalize(); Ok(q) }
    pub fn gcd(mut left: Self, mut right: Self) -> Result<Self,String> { left=left.abs(); right=right.abs(); while !right.is_zero() { let (_,r)=left.div_rem_abs(&right)?; left=right; right=r; } Ok(left) }
}
impl std::fmt::Display for BigInt { fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result { if self.sign<0 { write!(f,"-")?; } if self.sign==0 { return write!(f,"0"); } let mut it=self.limbs.iter().rev(); write!(f,"{}",it.next().unwrap())?; for limb in it { write!(f,"{limb:09}")?; } Ok(()) } }
impl Ord for BigInt { fn cmp(&self,rhs:&Self)->Ordering { self.sign.cmp(&rhs.sign).then_with(|| if self.sign<0 { self.abs_cmp(rhs).reverse() } else { self.abs_cmp(rhs) }) } }
impl PartialOrd for BigInt { fn partial_cmp(&self,rhs:&Self)->Option<Ordering>{Some(self.cmp(rhs))} }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rational { pub num: BigInt, pub den: BigInt }

impl Rational {
    pub fn new(num: i128, den: i128) -> Result<Self, String> {
        Self::from_bigints(BigInt::from_i128(num), BigInt::from_i128(den))
    }
    pub fn parse(num:&str,den:&str)->Result<Self,String>{Self::from_bigints(BigInt::parse(num)?,BigInt::parse(den)?)}
    pub fn parse_source(num: &str, den: &str) -> Result<Self, String> {
        Self::from_bigints(BigInt::parse_source(num)?, BigInt::parse_source(den)?)
    }
    pub fn from_bigints(mut num:BigInt,mut den:BigInt)->Result<Self,String>{if den.is_zero(){return Err("rational denominator is zero".into());}if den.is_negative(){num=num.neg();den=den.neg();}if num.is_zero(){return Ok(Self::zero());}let divisor=BigInt::gcd(num.clone(),den.clone())?;Ok(Self{num:num.exact_div(&divisor)?,den:den.exact_div(&divisor)?})}
    pub fn zero()->Self{Self{num:BigInt::zero(),den:BigInt::one()}}
    pub fn integer(value: i128) -> Self { Self { num: BigInt::from_i128(value), den: BigInt::one() } }
    pub fn add(&self,rhs:&Self)->Result<Self,String>{Self::from_bigints(self.num.mul(&rhs.den).add(&rhs.num.mul(&self.den)),self.den.mul(&rhs.den))}
    pub fn sub(&self,rhs:&Self)->Result<Self,String>{self.add(&rhs.neg())}
    pub fn mul(&self,rhs:&Self)->Result<Self,String>{Self::from_bigints(self.num.mul(&rhs.num),self.den.mul(&rhs.den))}
    pub fn div(&self,rhs:&Self)->Result<Self,String>{if rhs.num.is_zero(){return Err("rational division by zero".into());}Self::from_bigints(self.num.mul(&rhs.den),self.den.mul(&rhs.num))}
    pub fn neg(&self)->Self{Self{num:self.num.neg(),den:self.den.clone()}}
    pub fn abs(&self)->Self{Self{num:self.num.abs(),den:self.den.clone()}}
    pub fn max_zero(&self)->Self{if self.num.is_negative(){Self::zero()}else{self.clone()}}
    pub fn to_json(&self) -> CanonicalJson {
        CanonicalJson::object([
            ("den".into(), CanonicalJson::Integer(self.den.to_string())), ("num".into(), CanonicalJson::Integer(self.num.to_string())),
        ]).expect("unique rational keys")
    }
}

impl Ord for Rational {
    fn cmp(&self, rhs: &Self) -> Ordering {
        self.num.mul(&rhs.den).cmp(&rhs.num.mul(&self.den))
    }
}
impl PartialOrd for Rational { fn partial_cmp(&self, rhs: &Self) -> Option<Ordering> { Some(self.cmp(rhs)) } }

impl std::fmt::Display for Rational {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.den == BigInt::one() {
            write!(f, "{}", self.num)
        } else {
            write!(f, "{}/{}", self.num, self.den)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Statistics {
    pub p50: Rational,
    pub p90: Rational,
    pub p95: Rational,
    pub p99: Rational,
    pub p999: Rational,
    pub mean: Rational,
    pub mad: Rational,
}

pub fn statistics(samples: &[Rational]) -> Result<Statistics, String> {
    if samples.is_empty() { return Err("statistics require at least one sample".into()); }
    let mut sorted = samples.to_vec();
    sorted.sort();
    let p50 = nearest_rank(&sorted, 500, 1000)?;
    let mut sum = Rational::zero();
    for sample in &sorted { sum = sum.add(sample)?; }
    let mean = sum.div(&Rational::integer(sorted.len() as i128))?;
    let mut deviations = sorted.iter().map(|sample| sample.sub(&p50).map(|v| v.abs())).collect::<Result<Vec<_>, _>>()?;
    deviations.sort();
    Ok(Statistics {
        p50,
        p90: nearest_rank(&sorted, 900, 1000)?,
        p95: nearest_rank(&sorted, 950, 1000)?,
        p99: nearest_rank(&sorted, 990, 1000)?,
        p999: nearest_rank(&sorted, 999, 1000)?,
        mean,
        mad: nearest_rank(&deviations, 500, 1000)?,
    })
}

fn nearest_rank(sorted: &[Rational], numerator: usize, denominator: usize) -> Result<Rational, String> {
    if sorted.is_empty() { return Err("rank requires at least one value".into()); }
    let rank = sorted.len().checked_mul(numerator).ok_or("rank overflow")?.div_ceil(denominator);
    Ok(sorted[rank.max(1) - 1].clone())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Percentile { P50, P90, P95, P99, P999 }
pub fn estimator(samples: &[Rational], percentile: Option<Percentile>) -> Result<Rational, String> {
    if samples.len() == 1 && percentile.is_none() { return Ok(samples[0].clone()); }
    let stats = statistics(samples)?;
    Ok(match percentile.unwrap_or(Percentile::P50) {
        Percentile::P50 => stats.p50, Percentile::P90 => stats.p90,
        Percentile::P95 => stats.p95, Percentile::P99 => stats.p99,
        Percentile::P999 => stats.p999,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction { LowerIsBetter, HigherIsBetter }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitDirection { AtMost, AtLeast }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelativeGoal { RegressionAtMost, ImprovementAtLeast }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Evidence { Pass, Regression, Inconclusive, Unavailable }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Enforcement { Warn, Fail }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyOutcome { Pass, Warn, Fail }

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Comparison {
    Absolute { limit: Rational, direction: LimitDirection },
    AbsoluteFrom { limit: Rational, direction: LimitDirection },
    RelativeTo { limit_basis_points: BigInt, goal: RelativeGoal },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeasurementPolicy {
    pub bootstrap_resamples: usize,
    pub lower_rank: usize,
    pub upper_rank: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Evaluation {
    pub point: Rational,
    pub lower95: Option<Rational>,
    pub upper95: Option<Rational>,
    pub evidence: Evidence,
    pub outcome: PolicyOutcome,
    pub bootstrap: Vec<Rational>,
}

#[allow(clippy::too_many_arguments)]
pub fn evaluate(
    evidence_id: &str,
    context_key: &str,
    baseline_report_ids: &[String],
    candidate: &[Rational],
    baseline: &[Rational],
    percentile: Option<Percentile>,
    comparison: &Comparison,
    direction: Direction,
    enforcement: Enforcement,
    policy: Option<&MeasurementPolicy>,
) -> Result<Evaluation, String> {
    if candidate.is_empty() { return Err("candidate samples are empty".into()); }
    let candidate_point = estimator(candidate, percentile)?;
    let (point, bootstrap) = match comparison {
        Comparison::Absolute { limit, direction } => {
            let pass = match direction { LimitDirection::AtMost => candidate_point <= *limit, LimitDirection::AtLeast => candidate_point >= *limit };
            return Ok(finish(candidate_point, None, None, if pass { Evidence::Pass } else { Evidence::Regression }, enforcement, Vec::new()));
        }
        Comparison::AbsoluteFrom { limit, direction: limit_direction } => {
            let point = match limit_direction { LimitDirection::AtMost => candidate_point.sub(limit)?, LimitDirection::AtLeast => limit.sub(&candidate_point)? };
            let policy = policy.ok_or("AbsoluteFrom requires a measurement policy")?;
            let values = bootstrap_values(evidence_id, context_key, baseline_report_ids, candidate, &[], percentile, comparison, direction, policy)?;
            (point, values)
        }
        Comparison::RelativeTo { limit_basis_points, goal } => {
            if baseline.is_empty() { return Err("RelativeTo baseline samples are empty".into()); }
            let baseline_point = estimator(baseline, percentile)?;
            if baseline_point == Rational::zero() { return Ok(finish(Rational::zero(), None, None, Evidence::Unavailable, enforcement, Vec::new())); }
            let point = relative_stat(candidate_point, baseline_point, limit_basis_points.clone(), *goal, direction)?;
            let policy = policy.ok_or("RelativeTo requires a measurement policy")?;
            let values = bootstrap_values(evidence_id, context_key, baseline_report_ids, candidate, baseline, percentile, comparison, direction, policy)?;
            (point, values)
        }
    };
    let policy = policy.expect("statistical branches require policy");
    if policy.lower_rank == 0 || policy.upper_rank == 0 || policy.lower_rank > bootstrap.len() || policy.upper_rank > bootstrap.len() {
        return Err("bootstrap ranks are outside the replicate set".into());
    }
    let lower = bootstrap[policy.lower_rank - 1].clone();
    let upper = bootstrap[policy.upper_rank - 1].clone();
    let evidence = if upper <= Rational::zero() { Evidence::Pass } else if lower > Rational::zero() { Evidence::Regression } else { Evidence::Inconclusive };
    Ok(finish(point, Some(lower), Some(upper), evidence, enforcement, bootstrap))
}

fn finish(point: Rational, lower95: Option<Rational>, upper95: Option<Rational>, evidence: Evidence, enforcement: Enforcement, bootstrap: Vec<Rational>) -> Evaluation {
    let outcome = match (evidence, enforcement) {
        (Evidence::Pass, _) => PolicyOutcome::Pass,
        (_, Enforcement::Warn) => PolicyOutcome::Warn,
        (_, Enforcement::Fail) => PolicyOutcome::Fail,
    };
    Evaluation { point, lower95, upper95, evidence, outcome, bootstrap }
}

#[allow(clippy::too_many_arguments)]
fn bootstrap_values(evidence_id: &str, context_key: &str, baseline_report_ids: &[String], candidate: &[Rational], baseline: &[Rational], percentile: Option<Percentile>, comparison: &Comparison, direction: Direction, policy: &MeasurementPolicy) -> Result<Vec<Rational>, String> {
    if policy.bootstrap_resamples == 0 { return Err("bootstrap requires at least one replicate".into()); }
    let mut values = Vec::with_capacity(policy.bootstrap_resamples);
    for replicate in 0..policy.bootstrap_resamples {
        let mut stream = ShaStream::new(evidence_id, context_key, baseline_report_ids, replicate as u64);
        let candidate_resample = resample(candidate, candidate.len(), &mut stream)?;
        let candidate_estimator = estimator(&candidate_resample, percentile)?;
        let value = match comparison {
            Comparison::AbsoluteFrom { limit, direction } => match direction { LimitDirection::AtMost => candidate_estimator.sub(limit)?, LimitDirection::AtLeast => limit.sub(&candidate_estimator)? },
            Comparison::RelativeTo { limit_basis_points, goal } => {
                let baseline_resample = resample(baseline, baseline.len(), &mut stream)?;
                let baseline_estimator = estimator(&baseline_resample, percentile)?;
                if baseline_estimator == Rational::zero() { return Err("RelativeTo bootstrap baseline estimator is zero".into()); }
                relative_stat(candidate_estimator, baseline_estimator, limit_basis_points.clone(), *goal, direction)?
            }
            Comparison::Absolute { .. } => return Err("Absolute does not bootstrap".into()),
        };
        values.push(value);
    }
    values.sort();
    Ok(values)
}

fn relative_stat(candidate: Rational, baseline: Rational, limit_basis_points: BigInt, goal: RelativeGoal, direction: Direction) -> Result<Rational, String> {
    let delta = match (goal, direction) {
        (RelativeGoal::RegressionAtMost, Direction::LowerIsBetter) | (RelativeGoal::ImprovementAtLeast, Direction::HigherIsBetter) => candidate.sub(&baseline)?.max_zero(),
        (RelativeGoal::RegressionAtMost, Direction::HigherIsBetter) | (RelativeGoal::ImprovementAtLeast, Direction::LowerIsBetter) => baseline.sub(&candidate)?.max_zero(),
    };
    let basis_points = delta.mul(&Rational::integer(10_000))?.div(&baseline)?;
    let limit = Rational::from_bigints(limit_basis_points, BigInt::one())?;
    match goal { RelativeGoal::RegressionAtMost => basis_points.sub(&limit), RelativeGoal::ImprovementAtLeast => limit.sub(&basis_points) }
}

const BOOTSTRAP_DOMAIN: &[u8] = b"jet.performance-budget.bootstrap.v1\0";
struct ShaStream<'a> { evidence_id: &'a str, context_key: &'a str, baseline_ids: &'a [String], replicate: u64, block: u64, words: Vec<u64> }
impl<'a> ShaStream<'a> {
    fn new(evidence_id: &'a str, context_key: &'a str, baseline_ids: &'a [String], replicate: u64) -> Self { Self { evidence_id, context_key, baseline_ids, replicate, block: 0, words: Vec::new() } }
    fn word(&mut self) -> u64 {
        if self.words.is_empty() {
            let mut input = Vec::new();
            input.extend_from_slice(BOOTSTRAP_DOMAIN);
            input.extend_from_slice(self.evidence_id.as_bytes());
            input.extend_from_slice(self.context_key.as_bytes());
            for id in self.baseline_ids { input.extend_from_slice(id.as_bytes()); }
            input.extend_from_slice(&self.replicate.to_be_bytes());
            input.extend_from_slice(&self.block.to_be_bytes());
            self.block += 1;
            self.words = SHA256::sha256(&input).chunks_exact(8).rev().map(|chunk| u64::from_be_bytes(chunk.try_into().expect("eight bytes"))).collect();
        }
        self.words.pop().expect("digest contains four words")
    }
    fn index(&mut self, population: usize) -> Result<usize, String> {
        if population == 0 { return Err("cannot sample an empty population".into()); }
        let k = population as u64;
        let threshold = ((u128::from(u64::MAX) + 1) / u128::from(k) * u128::from(k)) as u128;
        loop { let word = self.word(); if u128::from(word) < threshold { return Ok((word % k) as usize); } }
    }
}
fn resample(values: &[Rational], count: usize, stream: &mut ShaStream<'_>) -> Result<Vec<Rational>, String> {
    (0..count).map(|_| stream.index(values.len()).map(|index| values[index].clone())).collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trend { pub report_ids: Vec<String>, pub estimators: Vec<Rational>, pub score: Option<Rational>, pub label: TrendLabel }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrendLabel { Improving, Stable, Regressing, Insufficient }
pub fn trend(report_ids: &[String], estimators: &[Rational], direction: Direction) -> Result<Trend, String> {
    if report_ids.len() != estimators.len() { return Err("trend ids and estimators differ in length".into()); }
    if estimators.len() < 3 { return Ok(Trend { report_ids: report_ids.to_vec(), estimators: estimators.to_vec(), score: None, label: TrendLabel::Insufficient }); }
    let mut slopes = Vec::new();
    for left in 0..estimators.len() {
        for right in left + 1..estimators.len() {
            let numerator = match direction { Direction::LowerIsBetter => estimators[left].sub(&estimators[right])?, Direction::HigherIsBetter => estimators[right].sub(&estimators[left])? };
            slopes.push(numerator.div(&Rational::integer((right - left) as i128))?);
        }
    }
    slopes.sort();
    let score = nearest_rank(&slopes, 500, 1000)?;
    let label = if score > Rational::zero() { TrendLabel::Improving } else if score < Rational::zero() { TrendLabel::Regressing } else { TrendLabel::Stable };
    Ok(Trend { report_ids: report_ids.to_vec(), estimators: estimators.to_vec(), score: Some(score), label })
}

pub fn stable_id(content: &CanonicalJson) -> String { content.sha256() }
pub fn verify_stable_id(content: &CanonicalJson, claimed: &str) -> Result<(), String> {
    let actual = stable_id(content);
    if actual == claimed { Ok(()) } else { Err(format!("content hash mismatch: claimed {claimed}, recomputed {actual}")) }
}

pub fn budget_report(content: CanonicalJson) -> CanonicalJson {
    let report_id = stable_id(&content);
    CanonicalJson::object([
        ("content".into(), content),
        ("report_id".into(), CanonicalJson::String(report_id)),
        ("schema".into(), CanonicalJson::String("jet.budget-report".into())),
        ("version".into(), CanonicalJson::Integer("1".into())),
    ]).expect("fixed report keys are unique")
}

pub fn verify_budget_report(bytes: &[u8]) -> Result<CanonicalJson, String> {
    let report = CanonicalJson::parse_canonical(bytes)?;
    let fields = match &report { CanonicalJson::Object(fields) => fields, _ => return Err("budget report wrapper is not an object".into()) };
    let expected = ["content", "report_id", "schema", "version"];
    if fields.len() != expected.len() || !expected.iter().all(|key| fields.contains_key(*key)) { return Err("budget report wrapper has missing or unknown keys".into()); }
    if fields.get("schema") != Some(&CanonicalJson::String("jet.budget-report".into())) || fields.get("version") != Some(&CanonicalJson::Integer("1".into())) { return Err("unsupported budget report schema/version".into()); }
    let content = fields.get("content").expect("checked content key");
    validate_report_content(content)?;
    let claimed = match fields.get("report_id") { Some(CanonicalJson::String(id)) => id, _ => return Err("budget report_id is not text".into()) };
    if claimed.len() != 64 || !claimed.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) { return Err("budget report_id is not lowercase Hex64".into()); }
    verify_stable_id(content, claimed)?;
    Ok(report)
}

fn object<'a>(value: &'a CanonicalJson, name: &str, keys: &[&str]) -> Result<&'a BTreeMap<String, CanonicalJson>, String> {
    let fields = match value { CanonicalJson::Object(v) => v, _ => return Err(format!("{name} is not an object")) };
    if fields.len() != keys.len() || !keys.iter().all(|key| fields.contains_key(*key)) { return Err(format!("{name} has missing or unknown fields")); }
    Ok(fields)
}
fn text<'a>(value:&'a CanonicalJson,name:&str)->Result<&'a str,String>{match value{CanonicalJson::String(v)=>Ok(v),_=>Err(format!("{name} is not text"))}}
fn integer(value:&CanonicalJson,name:&str)->Result<BigInt,String>{match value{CanonicalJson::Integer(v)=>BigInt::parse(v).map_err(|_|format!("{name} is not a canonical integer")),_=>Err(format!("{name} is not an integer"))}}
fn unsigned(value:&CanonicalJson,name:&str)->Result<BigInt,String>{let v=integer(value,name)?;if v.is_negative(){Err(format!("{name} is negative"))}else{Ok(v)}}
fn boolean(value:&CanonicalJson,name:&str)->Result<bool,String>{match value{CanonicalJson::Bool(v)=>Ok(*v),_=>Err(format!("{name} is not boolean"))}}
fn array<'a>(value:&'a CanonicalJson,name:&str)->Result<&'a [CanonicalJson],String>{match value{CanonicalJson::Array(v)=>Ok(v),_=>Err(format!("{name} is not an array"))}}
fn hex64<'a>(value:&'a CanonicalJson,name:&str)->Result<&'a str,String>{let v=text(value,name)?;if v.len()==64&&v.bytes().all(|b|b.is_ascii_hexdigit()&&!b.is_ascii_uppercase()){Ok(v)}else{Err(format!("{name} is not lowercase Hex64"))}}
fn nullable(value:&CanonicalJson,validate:impl FnOnce(&CanonicalJson)->Result<(),String>)->Result<(),String>{if matches!(value,CanonicalJson::Null){Ok(())}else{validate(value)}}
fn one_of(value:&CanonicalJson,name:&str,allowed:&[&str])->Result<(),String>{let v=text(value,name)?;if allowed.contains(&v){Ok(())}else{Err(format!("{name} has unknown value `{v}`"))}}

fn validate_rational(value:&CanonicalJson,quantity:bool,name:&str)->Result<Rational,String>{
    let f=object(value,name,&["num","den"])?;let n=integer(&f["num"],&format!("{name}.num"))?;let d=integer(&f["den"],&format!("{name}.den"))?;
    if d.is_zero()||d.is_negative(){return Err(format!("{name}.den must be positive"));}if quantity&&n.is_negative(){return Err(format!("{name} quantity is negative"));}
    let reduced=Rational::from_bigints(n.clone(),d.clone())?;if reduced.num!=n||reduced.den!=d{return Err(format!("{name} is not gcd-reduced"));}Ok(reduced)
}
fn validate_policy(value:&CanonicalJson)->Result<(),String>{let f=object(value,"measurement policy",&["min_candidate_samples","min_baseline_samples","baseline_generations","bootstrap_resamples","lower_rank","upper_rank","stale_after_seconds","trend_generations"])?;for key in f.keys(){unsigned(&f[key],&format!("measurement policy.{key}"))?;}Ok(())}
fn validate_statistics(value:&CanonicalJson)->Result<(),String>{let f=object(value,"statistics",&["count","sorted_samples","p50","p90","p95","p99","p999","mean","mad"])?;let count=unsigned(&f["count"],"statistics.count")?;let raw=array(&f["sorted_samples"],"statistics.sorted_samples")?;let mut samples=Vec::with_capacity(raw.len());for sample in raw{samples.push(validate_rational(sample,true,"statistics sample")?);}if samples.windows(2).any(|w|w[0]>w[1]){return Err("statistics.sorted_samples is not sorted".into());}if count.to_string()!=samples.len().to_string(){return Err("statistics.count does not match sorted_samples".into());}let actual=statistics(&samples)?;for (key,want) in [("p50",actual.p50),("p90",actual.p90),("p95",actual.p95),("p99",actual.p99),("p999",actual.p999),("mean",actual.mean),("mad",actual.mad)]{if validate_rational(&f[key],true,&format!("statistics.{key}"))?!=want{return Err(format!("statistics.{key} does not match independent recomputation"));}}Ok(())}
fn validate_history(value:&CanonicalJson)->Result<(),String>{let f=object(value,"history selection",&["state_id","report_ids"])?;hex64(&f["state_id"],"history.state_id")?;for id in array(&f["report_ids"],"history.report_ids")?{hex64(id,"history report id")?;}Ok(())}
fn validate_baseline(value:&CanonicalJson)->Result<(),String>{let f=object(value,"statistical baseline",&["history","pooled_samples","statistics","policy"])?;validate_history(&f["history"])?;for q in array(&f["pooled_samples"],"baseline.pooled_samples")?{validate_rational(q,true,"baseline sample")?;}validate_statistics(&f["statistics"])?;validate_policy(&f["policy"])}
fn validate_trend(value:&CanonicalJson)->Result<(),String>{let f=object(value,"trend",&["label","report_ids","estimators","score"])?;one_of(&f["label"],"trend.label",&["improving","stable","regressing","insufficient"])?;let ids=array(&f["report_ids"],"trend.report_ids")?;for id in ids{hex64(id,"trend report id")?;}let estimators=array(&f["estimators"],"trend.estimators")?;if ids.len()!=estimators.len(){return Err("trend inputs are not index-aligned".into());}for q in estimators{validate_rational(q,true,"trend estimator")?;}nullable(&f["score"],|v|validate_rational(v,false,"trend.score").map(|_|()))}
fn validate_decision(value:&CanonicalJson)->Result<(),String>{let f=object(value,"decision",&["evidence","reason","point","lower95","upper95","trend","policy_outcome"])?;one_of(&f["evidence"],"decision.evidence",&["pass","regression","inconclusive","unavailable"])?;nullable(&f["reason"],|v|text(v,"decision.reason").map(|_|()))?;nullable(&f["point"],|v|validate_rational(v,false,"decision.point").map(|_|()))?;nullable(&f["lower95"],|v|validate_rational(v,false,"decision.lower95").map(|_|()))?;nullable(&f["upper95"],|v|validate_rational(v,false,"decision.upper95").map(|_|()))?;validate_trend(&f["trend"])?;one_of(&f["policy_outcome"],"decision.policy_outcome",&["pass","warn","fail"])}
fn validate_metric(value:&CanonicalJson)->Result<(),String>{let f=object(value,"metric",&["name","percentile"])?;text(&f["name"],"metric.name")?;nullable(&f["percentile"],|v|one_of(v,"metric.percentile",&["p50","p90","p95","p99","p999"]))}
fn validate_provider(value:&CanonicalJson)->Result<(),String>{let f=object(value,"provider",&["kind","identity","version","isolation","cpu_arch","cpu_model","logical_cpus","memory_bytes","os","kernel","power_governor","hardware_fingerprint"])?;for key in ["kind","identity","version","isolation","cpu_arch","cpu_model","os","kernel","power_governor"]{text(&f[key],&format!("provider.{key}"))?;}unsigned(&f["logical_cpus"],"provider.logical_cpus")?;unsigned(&f["memory_bytes"],"provider.memory_bytes")?;hex64(&f["hardware_fingerprint"],"provider.hardware_fingerprint")?;let mut unhashed=f.clone();unhashed.remove("hardware_fingerprint");verify_stable_id(&CanonicalJson::Object(unhashed),text(&f["hardware_fingerprint"],"provider.hardware_fingerprint")?).map_err(|_|"provider hardware_fingerprint mismatch".into())}
fn validate_comparison(value:&CanonicalJson)->Result<(),String>{let f=match value{CanonicalJson::Object(f)=>f,_=>return Err("comparison is not an object".into())};let kind=text(f.get("kind").ok_or("comparison missing kind")?,"comparison.kind")?;match kind{"absolute"=>{let f=object(value,"absolute comparison",&["kind","limit","direction"])?;validate_rational(&f["limit"],true,"comparison.limit")?;one_of(&f["direction"],"comparison.direction",&["at_most","at_least"])},"absolute_from"=>{let f=object(value,"absolute_from comparison",&["kind","baseline","limit","direction"])?;text(&f["baseline"],"comparison.baseline")?;validate_rational(&f["limit"],true,"comparison.limit")?;one_of(&f["direction"],"comparison.direction",&["at_most","at_least"])},"relative_to"=>{let f=object(value,"relative_to comparison",&["kind","baseline","limit_basis_points","goal","direction"])?;text(&f["baseline"],"comparison.baseline")?;unsigned(&f["limit_basis_points"],"comparison.limit_basis_points")?;one_of(&f["goal"],"comparison.goal",&["regression_at_most","improvement_at_least"])?;one_of(&f["direction"],"comparison.direction",&["lower_is_better","higher_is_better"])},_=>Err(format!("comparison has unknown kind `{kind}`"))}}
fn validate_measurement(value:&CanonicalJson)->Result<(),String>{
    let fields=match value{CanonicalJson::Object(fields)=>fields,_=>return Err("measurement is not an object".into())};
    let required=["budget_id","budget_spec","budget_spec_sha256","source","metric","target_class","unit","direction","provider","comparison","enforcement","context_key","policy","samples","statistics","history","baseline","decision"];
    let has_compile=fields.contains_key("compile");
    if fields.len()!=required.len()+usize::from(has_compile)||!required.iter().all(|key|fields.contains_key(*key))||fields.keys().any(|key|key!="compile"&&!required.contains(&key.as_str())){return Err("measurement has missing or unknown fields".into());}
    text(&fields["budget_id"],"measurement.budget_id")?;if !matches!(fields["budget_spec"],CanonicalJson::Object(_)){return Err("measurement.budget_spec is not an object".into());}hex64(&fields["budget_spec_sha256"],"measurement.budget_spec_sha256")?;verify_stable_id(&fields["budget_spec"],text(&fields["budget_spec_sha256"],"measurement.budget_spec_sha256")?).map_err(|_|"measurement budget_spec_sha256 mismatch".to_string())?;text(&fields["source"],"measurement.source")?;validate_metric(&fields["metric"])?;text(&fields["target_class"],"measurement.target_class")?;text(&fields["unit"],"measurement.unit")?;one_of(&fields["direction"],"measurement.direction",&["lower_is_better","higher_is_better"])?;validate_provider(&fields["provider"])?;validate_comparison(&fields["comparison"])?;one_of(&fields["enforcement"],"measurement.enforcement",&["warn","fail"])?;hex64(&fields["context_key"],"measurement.context_key")?;nullable(&fields["policy"],validate_policy)?;for q in array(&fields["samples"],"measurement.samples")?{validate_rational(q,true,"measurement sample")?;}nullable(&fields["statistics"],validate_statistics)?;nullable(&fields["history"],validate_history)?;nullable(&fields["baseline"],validate_baseline)?;nullable(&fields["decision"],validate_decision)?;
    let provider_kind=match &fields["provider"]{CanonicalJson::Object(provider)=>text(&provider["kind"],"provider.kind")?,_=>unreachable!()};
    if has_compile { let metric=match &fields["metric"]{CanonicalJson::Object(metric)=>metric,_=>unreachable!()};if text(&metric["name"],"metric.name")?!="CompileTime"||provider_kind!="CompilerProbe"||fields["unit"]!=CanonicalJson::String("Duration".into())||matches!(&metric["percentile"],CanonicalJson::Null){return Err("compile metadata requires the CompilerProbe provider, CompileTime percentile, and Duration unit".into());}validate_compile_metadata(fields.get("compile").expect("compile field exists"),&fields["samples"])?;} else if provider_kind=="CompilerProbe" { return Err("CompilerProbe measurement has no compile metadata".into()); }
    Ok(())
}

fn validate_compile_metadata(value:&CanonicalJson,measurement_samples:&CanonicalJson)->Result<(),String>{
    let fields=object(value,"compile metadata",&["backend","cache_state","compiler_digest","core_digest","edit_bytes","edit_sha256","host","linker","phase_totals","profile","sample_records","samples","source_tree_sha256","target","variance","warmups","workload_bytes"])?;
    text(&fields["backend"],"compile.backend")?;one_of(&fields["cache_state"],"compile.cache_state",&["Clean","NoChange","Edit"])?;hex64(&fields["compiler_digest"],"compile.compiler_digest")?;hex64(&fields["core_digest"],"compile.core_digest")?;unsigned(&fields["edit_bytes"],"compile.edit_bytes")?;hex64(&fields["edit_sha256"],"compile.edit_sha256")?;text(&fields["host"],"compile.host")?;text(&fields["linker"],"compile.linker")?;text(&fields["profile"],"compile.profile")?;hex64(&fields["source_tree_sha256"],"compile.source_tree_sha256")?;text(&fields["target"],"compile.target")?;for(key,name)in [("backend","compile.backend"),("host","compile.host"),("linker","compile.linker"),("profile","compile.profile"),("target","compile.target")]{if text(&fields[key],name)?.is_empty(){return Err(format!("{name} is empty"));}}let samples=usize_wire(&fields["samples"],"compile.samples")?;let warmups=usize_wire(&fields["warmups"],"compile.warmups")?;if samples!=20||warmups!=1{return Err("compile metadata does not use the pinned warmup/sample policy".into());}let records=array(&fields["sample_records"],"compile.sample_records")?;if records.len()!=samples{return Err("compile.sample_records count does not match compile.samples".into());}
    let mut elapsed=Vec::with_capacity(records.len());let mut aggregate=BTreeMap::<String,BigInt>::new();for record in records{validate_compile_record(record,fields,&mut aggregate,&mut elapsed)?;}let measured_samples=array(measurement_samples,"measurement.samples")?;if measured_samples.len()!=elapsed.len(){return Err("measurement.samples count does not match compile.sample_records".into());}for(index,sample)in measured_samples.iter().enumerate(){if validate_rational(sample,true,&format!("measurement.samples[{index}]"))?!=elapsed[index]{return Err("measurement.samples does not match compile.sample_records".into());}}let root_phases=validate_phase_totals(&fields["phase_totals"],"compile.phase_totals")?;if root_phases!=aggregate{return Err("compile.phase_totals does not equal the per-sample phase totals".into());}let mean=elapsed.iter().try_fold(Rational::zero(),|sum,value|sum.add(value))?.div(&Rational::integer(elapsed.len()as i128))?;let variance=elapsed.iter().try_fold(Rational::zero(),|sum,value|{let delta=value.sub(&mean)?;sum.add(&delta.mul(&delta)?)})?.div(&Rational::integer(elapsed.len()as i128))?;if validate_rational(&fields["variance"],true,"compile.variance")?!=variance{return Err("compile.variance does not match elapsed samples".into());}Ok(())
}

fn validate_compile_record(value:&CanonicalJson,root:&BTreeMap<String,CanonicalJson>,aggregate:&mut BTreeMap<String,BigInt>,elapsed:&mut Vec<Rational>)->Result<(),String>{
    unsigned(&root["workload_bytes"],"compile.workload_bytes")?;
    let fields=object(value,"compile sample record",&["backend","cache_state","compiler_digest","core_digest","edit_bytes","elapsed_ns","host","linker","phase_totals","profile","source_tree_sha256","target","workload_bytes"])?;for key in ["backend","cache_state","compiler_digest","core_digest","edit_bytes","host","linker","profile","source_tree_sha256","target","workload_bytes"]{if fields[key]!=root[key]{return Err(format!("compile sample record `{key}` does not match its row identity"));}}hex64(&fields["compiler_digest"],"compile sample compiler_digest")?;hex64(&fields["core_digest"],"compile sample core_digest")?;hex64(&fields["source_tree_sha256"],"compile sample source_tree_sha256")?;let ns=unsigned(&fields["elapsed_ns"],"compile sample elapsed_ns")?;elapsed.push(Rational::from_bigints(ns.clone(),BigInt::one())?);let phases=validate_phase_totals(&fields["phase_totals"],"compile sample phase_totals")?;for(name,value)in phases{let slot=aggregate.entry(name).or_insert_with(BigInt::zero);*slot=slot.add(&value);}Ok(())
}

fn validate_phase_totals(value:&CanonicalJson,name:&str)->Result<BTreeMap<String,BigInt>,String>{let entries=array(value,name)?;let mut prior=None;let mut totals=BTreeMap::new();for entry in entries{let fields=object(entry,&format!("{name} entry"),&["name","ns"])?;let phase=text(&fields["name"],&format!("{name}.name"))?;if phase.is_empty()||prior.is_some_and(|previous|previous>=phase){return Err(format!("{name} is not sorted by phase name"));}prior=Some(phase);let ns=unsigned(&fields["ns"],&format!("{name}.ns"))?;if totals.insert(phase.into(),ns).is_some(){return Err(format!("{name} contains a duplicate phase"));}}if totals.is_empty(){return Err(format!("{name} is empty"));}Ok(totals)}
fn validate_report_content(value:&CanonicalJson)->Result<(),String>{
    let f=object(value,"report content",&["subject","toolchain","evidence_id","measurements","summary","privacy"])?;
    let subject=object(&f["subject"],"subject",&["target_id","member_sources","target_triple","target_class","profile","artifact","measured_start","measured_end"])?;for key in ["target_id","target_triple","target_class","profile","measured_start","measured_end"]{text(&subject[key],&format!("subject.{key}"))?;}let mut prior:Option<(&str,&str)>=None;for source in array(&subject["member_sources"],"subject.member_sources")?{let sf=object(source,"member source",&["path","sha256"])?;let path=text(&sf["path"],"member source.path")?;let hash=hex64(&sf["sha256"],"member source.sha256")?;if prior.is_some_and(|p|p>(path,hash)){return Err("member_sources is not sorted".into());}prior=Some((path,hash));}nullable(&subject["artifact"],|v|{let a=object(v,"artifact",&["sha256","bytes"])?;hex64(&a["sha256"],"artifact.sha256")?;unsigned(&a["bytes"],"artifact.bytes")?;Ok(())})?;
    let tool=object(&f["toolchain"],"toolchain",&["jet_version","compiler_build_id","stdlib_id","runner_id","digest"])?;for key in ["jet_version","compiler_build_id","stdlib_id","runner_id"]{text(&tool[key],&format!("toolchain.{key}"))?;}hex64(&tool["digest"],"toolchain.digest")?;let digest_content=CanonicalJson::object(["jet_version","compiler_build_id","stdlib_id","runner_id"].into_iter().map(|k|(k.into(),tool[k].clone())))?;verify_stable_id(&digest_content,text(&tool["digest"],"toolchain.digest")?).map_err(|_|"toolchain digest mismatch".to_string())?;
    let evidence=hex64(&f["evidence_id"],"evidence_id")?;let measurements=array(&f["measurements"],"measurements")?;let mut order=None;for measurement in measurements{validate_measurement(measurement)?;let mf=match measurement{CanonicalJson::Object(v)=>v,_=>unreachable!()};let key=(text(&mf["budget_id"],"budget_id")?,text(&mf["source"],"source")?);if order.is_some_and(|p|p>key){return Err("measurements is not sorted by budget_id then source".into());}order=Some(key);validate_context_key(subject,tool,mf)?;verify_wire_evaluation(evidence,mf)?;}
    let sanitized=CanonicalJson::object([("measurements".into(),CanonicalJson::Array(measurements.iter().map(sanitize_measurement).collect::<Result<Vec<_>,_>>()?)),("subject".into(),f["subject"].clone()),("toolchain".into(),f["toolchain"].clone())])?;verify_stable_id(&sanitized,evidence).map_err(|_|"evidence_id mismatch".to_string())?;
    let summary=object(&f["summary"],"summary",&["outcome","pass","warn","fail"])?;one_of(&summary["outcome"],"summary.outcome",&["pass","warn","fail"])?;for key in ["pass","warn","fail"]{unsigned(&summary[key],&format!("summary.{key}"))?;}
    let privacy=object(&f["privacy"],"privacy",&["schema","workspace_paths_only","retained","excluded"])?;if privacy["schema"]!=CanonicalJson::Integer("1".into())||!boolean(&privacy["workspace_paths_only"],"privacy.workspace_paths_only")?{return Err("unsupported privacy schema/policy".into());}for key in ["retained","excluded"]{let vals=array(&privacy[key],&format!("privacy.{key}"))?;let mut prior=None;for v in vals{let v=text(v,&format!("privacy.{key} item"))?;if prior.is_some_and(|p|p>v){return Err(format!("privacy.{key} is not sorted"));}prior=Some(v);}}
    Ok(())
}

fn sanitize_measurement(value:&CanonicalJson)->Result<CanonicalJson,String>{let mut f=match value{CanonicalJson::Object(v)=>v.clone(),_=>return Err("measurement is not object".into())};for key in ["history","baseline","decision"]{f.insert(key.into(),CanonicalJson::Null);}Ok(CanonicalJson::Object(f))}
fn frame(input:&mut Vec<u8>,value:&str){input.extend_from_slice(&(value.len() as u64).to_be_bytes());input.extend_from_slice(value.as_bytes());}
fn wire_text(value:&CanonicalJson)->String{match value{CanonicalJson::String(v)|CanonicalJson::Integer(v)=>v.clone(),CanonicalJson::Null=>String::new(),_=>String::from_utf8(value.bytes()).expect("canonical JSON UTF-8").trim_end().into()}}
fn validate_context_key(subject:&BTreeMap<String,CanonicalJson>,tool:&BTreeMap<String,CanonicalJson>,measurement:&BTreeMap<String,CanonicalJson>)->Result<(),String>{
    let metric=match &measurement["metric"]{CanonicalJson::Object(v)=>v,_=>return Err("metric is not object".into())};let provider=match &measurement["provider"]{CanonicalJson::Object(v)=>v,_=>return Err("provider is not object".into())};let mut input=b"jet-budget-context-v1\0".to_vec();
    for value in [&subject["target_id"],&metric["name"],&metric["percentile"],&measurement["target_class"],&subject["target_triple"],&subject["profile"]]{frame(&mut input,&wire_text(value));}
    for key in ["jet_version","compiler_build_id","stdlib_id","runner_id","digest"]{frame(&mut input,&wire_text(&tool[key]));}
    for key in ["kind","identity","version","isolation","cpu_arch","cpu_model","logical_cpus","memory_bytes","os","kernel","power_governor","hardware_fingerprint"]{frame(&mut input,&wire_text(&provider[key]));}
    let actual=SHA256::sha256_hex(&input);if text(&measurement["context_key"],"context_key")?!=actual{return Err("measurement context_key mismatch".into());}Ok(())
}

fn usize_wire(value:&CanonicalJson,name:&str)->Result<usize,String>{unsigned(value,name)?.to_string().parse().map_err(|_|format!("{name} exceeds implementation resource limit"))}
fn rational_or_none(value:&CanonicalJson,name:&str)->Result<Option<Rational>,String>{if matches!(value,CanonicalJson::Null){Ok(None)}else{Ok(Some(validate_rational(value,false,name)?))}}
fn parse_percentile(value:&CanonicalJson)->Result<Option<Percentile>,String>{match value{CanonicalJson::Null=>Ok(None),CanonicalJson::String(v)=>match v.as_str(){"p50"=>Ok(Some(Percentile::P50)),"p90"=>Ok(Some(Percentile::P90)),"p95"=>Ok(Some(Percentile::P95)),"p99"=>Ok(Some(Percentile::P99)),"p999"=>Ok(Some(Percentile::P999)),_=>Err("unknown percentile".into())},_=>Err("percentile is not text or null".into())}}
fn parse_direction(value:&CanonicalJson)->Result<Direction,String>{match text(value,"direction")?{"lower_is_better"=>Ok(Direction::LowerIsBetter),"higher_is_better"=>Ok(Direction::HigherIsBetter),_=>Err("unknown direction".into())}}
fn parse_enforcement(value:&CanonicalJson)->Result<Enforcement,String>{match text(value,"enforcement")?{"warn"=>Ok(Enforcement::Warn),"fail"=>Ok(Enforcement::Fail),_=>Err("unknown enforcement".into())}}
fn parse_comparison(value:&CanonicalJson)->Result<Comparison,String>{let f=match value{CanonicalJson::Object(v)=>v,_=>return Err("comparison is not object".into())};match text(&f["kind"],"comparison.kind")?{"absolute"=>Ok(Comparison::Absolute{limit:validate_rational(&f["limit"],true,"comparison.limit")?,direction:match text(&f["direction"],"comparison.direction")?{"at_most"=>LimitDirection::AtMost,"at_least"=>LimitDirection::AtLeast,_=>return Err("unknown limit direction".into())}}),"absolute_from"=>Ok(Comparison::AbsoluteFrom{limit:validate_rational(&f["limit"],true,"comparison.limit")?,direction:match text(&f["direction"],"comparison.direction")?{"at_most"=>LimitDirection::AtMost,"at_least"=>LimitDirection::AtLeast,_=>return Err("unknown limit direction".into())}}),"relative_to"=>Ok(Comparison::RelativeTo{limit_basis_points:unsigned(&f["limit_basis_points"],"comparison.limit_basis_points")?,goal:match text(&f["goal"],"comparison.goal")?{"regression_at_most"=>RelativeGoal::RegressionAtMost,"improvement_at_least"=>RelativeGoal::ImprovementAtLeast,_=>return Err("unknown relative goal".into())}}),_=>Err("unknown comparison kind".into())}}
fn parse_policy(value:&CanonicalJson)->Result<Option<MeasurementPolicy>,String>{if matches!(value,CanonicalJson::Null){return Ok(None);}let f=match value{CanonicalJson::Object(v)=>v,_=>return Err("policy is not object".into())};Ok(Some(MeasurementPolicy{bootstrap_resamples:usize_wire(&f["bootstrap_resamples"],"policy.bootstrap_resamples")?,lower_rank:usize_wire(&f["lower_rank"],"policy.lower_rank")?,upper_rank:usize_wire(&f["upper_rank"],"policy.upper_rank")?}))}
fn quantities(value:&CanonicalJson,name:&str)->Result<Vec<Rational>,String>{array(value,name)?.iter().map(|v|validate_rational(v,true,name)).collect()}
fn history_ids(value:&CanonicalJson)->Result<Vec<String>,String>{if matches!(value,CanonicalJson::Null){return Ok(Vec::new());}let f=match value{CanonicalJson::Object(v)=>v,_=>return Err("history is not object".into())};Ok(array(&f["report_ids"],"history.report_ids")?.iter().map(|v|text(v,"history report id").map(str::to_owned)).collect::<Result<_,_>>()?)}
fn evidence_enum(value:&CanonicalJson)->Result<Evidence,String>{match text(value,"decision.evidence")?{"pass"=>Ok(Evidence::Pass),"regression"=>Ok(Evidence::Regression),"inconclusive"=>Ok(Evidence::Inconclusive),"unavailable"=>Ok(Evidence::Unavailable),_=>Err("unknown evidence".into())}}
fn outcome_enum(value:&CanonicalJson)->Result<PolicyOutcome,String>{match text(value,"decision.policy_outcome")?{"pass"=>Ok(PolicyOutcome::Pass),"warn"=>Ok(PolicyOutcome::Warn),"fail"=>Ok(PolicyOutcome::Fail),_=>Err("unknown policy outcome".into())}}
fn trend_label_enum(value:&CanonicalJson)->Result<TrendLabel,String>{match text(value,"trend.label")?{"improving"=>Ok(TrendLabel::Improving),"stable"=>Ok(TrendLabel::Stable),"regressing"=>Ok(TrendLabel::Regressing),"insufficient"=>Ok(TrendLabel::Insufficient),_=>Err("unknown trend label".into())}}
fn verify_wire_evaluation(evidence_id:&str,m:&BTreeMap<String,CanonicalJson>)->Result<(),String>{
    if matches!(m["decision"],CanonicalJson::Null){return Ok(());}let d=match &m["decision"]{CanonicalJson::Object(v)=>v,_=>unreachable!()};let direction=parse_direction(&m["direction"])?;let enforcement=parse_enforcement(&m["enforcement"])?;let comparison=parse_comparison(&m["comparison"])?;let policy=parse_policy(&m["policy"])?;let candidate=quantities(&m["samples"],"measurement.samples")?;let (baseline,ids)=if let CanonicalJson::Object(b)=&m["baseline"]{(quantities(&b["pooled_samples"],"baseline.pooled_samples")?,history_ids(&b["history"])?)}else{(Vec::new(),history_ids(&m["history"])?)};
    let stored_evidence=evidence_enum(&d["evidence"])?;let stored_outcome=outcome_enum(&d["policy_outcome"])?;
    if matches!(comparison,Comparison::RelativeTo{..})&&baseline.is_empty(){if stored_evidence!=Evidence::Unavailable{return Err("relative decision without baseline must be unavailable".into());}let expected=match enforcement{Enforcement::Warn=>PolicyOutcome::Warn,Enforcement::Fail=>PolicyOutcome::Fail};if stored_outcome!=expected{return Err("unavailable decision policy outcome mismatch".into());}}
    else {let metric=match &m["metric"]{CanonicalJson::Object(v)=>v,_=>unreachable!()};let actual=evaluate(evidence_id,text(&m["context_key"],"context_key")?,&ids,&candidate,&baseline,parse_percentile(&metric["percentile"])?,&comparison,direction,enforcement,policy.as_ref())?;if rational_or_none(&d["point"],"decision.point")?!=Some(actual.point)||rational_or_none(&d["lower95"],"decision.lower95")?!=actual.lower95||rational_or_none(&d["upper95"],"decision.upper95")?!=actual.upper95||stored_evidence!=actual.evidence||stored_outcome!=actual.outcome{return Err("persisted decision does not match independent evaluator recomputation".into());}}
    let t=match &d["trend"]{CanonicalJson::Object(v)=>v,_=>unreachable!()};let tids=history_ids(&CanonicalJson::Object(BTreeMap::from([("state_id".into(),CanonicalJson::String("0".repeat(64))),("report_ids".into(),t["report_ids"].clone())])))?;let estimators=quantities(&t["estimators"],"trend.estimators")?;let actual=trend(&tids,&estimators,direction)?;if trend_label_enum(&t["label"])?!=actual.label||rational_or_none(&t["score"],"trend.score")?!=actual.score{return Err("persisted trend does not match independent recomputation".into());}Ok(())
}

pub fn verify_evaluation(expected: &Evaluation, inputs: EvaluationInputs<'_>) -> Result<(), String> {
    let actual = evaluate(inputs.evidence_id, inputs.context_key, inputs.baseline_report_ids, inputs.candidate, inputs.baseline, inputs.percentile, inputs.comparison, inputs.direction, inputs.enforcement, inputs.policy)?;
    if &actual == expected { Ok(()) } else { Err("persisted budget evaluation does not match independent recomputation".into()) }
}

pub struct EvaluationInputs<'a> {
    pub evidence_id: &'a str,
    pub context_key: &'a str,
    pub baseline_report_ids: &'a [String],
    pub candidate: &'a [Rational],
    pub baseline: &'a [Rational],
    pub percentile: Option<Percentile>,
    pub comparison: &'a Comparison,
    pub direction: Direction,
    pub enforcement: Enforcement,
    pub policy: Option<&'a MeasurementPolicy>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(value: i128) -> Rational { Rational::integer(value) }

    #[test]
    fn canonical_json_sorts_keys_escapes_and_hashes_lf_terminated_content() {
        let value = CanonicalJson::object([
            ("z".into(), CanonicalJson::String("é\n".into())),
            ("a".into(), CanonicalJson::integer("123").unwrap()),
        ]).unwrap();
        assert_eq!(value.bytes(), "{\"a\":123,\"z\":\"é\\n\"}\n".as_bytes());
        assert_eq!(stable_id(&value), SHA256::sha256_hex(b"{\"a\":123,\"z\":\"\xc3\xa9\\n\"}\n"));
        assert!(CanonicalJson::integer("-0").is_err());
        assert!(CanonicalJson::integer("01").is_err());
    }

    #[test]
    fn rational_statistics_are_reduced_and_nearest_rank_exact() {
        assert_eq!(Rational::new(20, -30).unwrap(), Rational::new(-2, 3).unwrap());
        let stats = statistics(&[q(1), q(2), q(3), q(4), q(100)]).unwrap();
        assert_eq!(stats.p50, q(3));
        assert_eq!(stats.p90, q(100));
        assert_eq!(stats.mean, Rational::new(22, 1).unwrap());
        assert_eq!(stats.mad, q(1));
    }

    #[test]
    fn arbitrary_precision_rationals_are_total_and_reduce_exactly() {
        let huge = "999999999999999999999999999999999999999999999999999999999999999999999999";
        let value = Rational::parse(huge, "300000000000000000000000000000000000000000000000000000000000000000000000").unwrap();
        assert_eq!(value.den.to_string(), "100000000000000000000000000000000000000000000000000000000000000000000000");
        assert_eq!(value.num.to_string(), "333333333333333333333333333333333333333333333333333333333333333333333333");
        let squared = value.mul(&value).unwrap();
        assert!(squared.num.to_string().len() > 140);
        assert_eq!(squared.div(&value).unwrap(), value);
        assert_eq!(Rational::parse("-0", "1").unwrap_err(), "non-canonical integer `-0`");
    }

    #[test]
    fn deterministic_absolute_and_trend_follow_direction() {
        let evaluation = evaluate("e", "c", &[], &[q(9)], &[], None, &Comparison::Absolute { limit: q(10), direction: LimitDirection::AtMost }, Direction::LowerIsBetter, Enforcement::Fail, None).unwrap();
        assert_eq!(evaluation.evidence, Evidence::Pass);
        let ids = vec!["old".into(), "mid".into(), "new".into()];
        let got = trend(&ids, &[q(12), q(10), q(8)], Direction::LowerIsBetter).unwrap();
        assert_eq!(got.label, TrendLabel::Improving);
        assert_eq!(got.score, Some(q(2)));
    }

    #[test]
    fn bootstrap_is_deterministic_and_reader_recomputes_every_outcome() {
        let policy = MeasurementPolicy { bootstrap_resamples: 20, lower_rank: 1, upper_rank: 20 };
        let comparison = Comparison::RelativeTo { limit_basis_points: BigInt::from_i128(500), goal: RelativeGoal::RegressionAtMost };
        let ids = vec!["0".repeat(64)];
        let inputs = EvaluationInputs { evidence_id: &"1".repeat(64), context_key: &"2".repeat(64), baseline_report_ids: &ids, candidate: &[q(90), q(91), q(92)], baseline: &[q(100), q(101), q(102)], percentile: Some(Percentile::P50), comparison: &comparison, direction: Direction::LowerIsBetter, enforcement: Enforcement::Fail, policy: Some(&policy) };
        let got = evaluate(inputs.evidence_id, inputs.context_key, inputs.baseline_report_ids, inputs.candidate, inputs.baseline, inputs.percentile, inputs.comparison, inputs.direction, inputs.enforcement, inputs.policy).unwrap();
        assert_eq!(got.evidence, Evidence::Pass);
        verify_evaluation(&got, inputs).unwrap();
        let mut corrupted = got.clone();
        corrupted.point = q(1);
        let inputs = EvaluationInputs { evidence_id: &"1".repeat(64), context_key: &"2".repeat(64), baseline_report_ids: &ids, candidate: &[q(90), q(91), q(92)], baseline: &[q(100), q(101), q(102)], percentile: Some(Percentile::P50), comparison: &comparison, direction: Direction::LowerIsBetter, enforcement: Enforcement::Fail, policy: Some(&policy) };
        assert!(verify_evaluation(&corrupted, inputs).is_err());
    }

    #[test]
    fn report_reader_rejects_noncanonical_unknown_and_hash_corrupt_artifacts() {
        let content = CanonicalJson::object([("summary".into(), CanonicalJson::String("pass".into()))]).unwrap();
        let report = budget_report(content);
        let bytes = report.bytes();
        assert!(verify_budget_report(&bytes).unwrap_err().contains("report content"));
        let spaced = String::from_utf8(bytes.clone()).unwrap().replace("{\"content\"", "{ \"content\"");
        assert!(verify_budget_report(spaced.as_bytes()).is_err());
        let corrupt = String::from_utf8(bytes).unwrap().replace("\"pass\"", "\"fail\"");
        assert!(verify_budget_report(corrupt.as_bytes()).is_err());
    }

    #[test]
    fn typed_schema_rejects_extra_nonreduced_and_forged_statistics() {
        let extra = CanonicalJson::object([
            ("den".into(), CanonicalJson::Integer("2".into())),
            ("num".into(), CanonicalJson::Integer("1".into())),
            ("surprise".into(), CanonicalJson::Null),
        ]).unwrap();
        assert!(validate_rational(&extra, false, "q").unwrap_err().contains("unknown"));
        let nonreduced = CanonicalJson::object([
            ("den".into(), CanonicalJson::Integer("4".into())),
            ("num".into(), CanonicalJson::Integer("2".into())),
        ]).unwrap();
        assert!(validate_rational(&nonreduced, false, "q").unwrap_err().contains("gcd-reduced"));
        let qj = |n:i128| Rational::integer(n).to_json();
        let forged = CanonicalJson::object([
            ("count".into(), CanonicalJson::Integer("3".into())),
            ("sorted_samples".into(), CanonicalJson::Array(vec![qj(1),qj(2),qj(3)])),
            ("p50".into(), qj(3)), ("p90".into(), qj(3)), ("p95".into(), qj(3)),
            ("p99".into(), qj(3)), ("p999".into(), qj(3)), ("mean".into(), qj(2)), ("mad".into(), qj(1)),
        ]).unwrap();
        assert!(validate_statistics(&forged).unwrap_err().contains("p50"));
    }

    #[test]
    fn wire_reader_recomputes_decision_confidence_trend_and_history() {
        let evidence="1".repeat(64);let context="2".repeat(64);let ids=vec!["3".repeat(64)];let samples=vec![q(1),q(10),q(100),q(1000)];
        let comparison=Comparison::AbsoluteFrom{limit:q(50),direction:LimitDirection::AtMost};let policy=MeasurementPolicy{bootstrap_resamples:100,lower_rank:50,upper_rank:51};
        let actual=evaluate(&evidence,&context,&ids,&samples,&[],Some(Percentile::P50),&comparison,Direction::LowerIsBetter,Enforcement::Fail,Some(&policy)).unwrap();
        let ev=match actual.evidence{Evidence::Pass=>"pass",Evidence::Regression=>"regression",Evidence::Inconclusive=>"inconclusive",Evidence::Unavailable=>"unavailable"};let outcome=match actual.outcome{PolicyOutcome::Pass=>"pass",PolicyOutcome::Warn=>"warn",PolicyOutcome::Fail=>"fail"};
        let trend_json=CanonicalJson::object([("label".into(),CanonicalJson::String("insufficient".into())),("report_ids".into(),CanonicalJson::Array(Vec::new())),("estimators".into(),CanonicalJson::Array(Vec::new())),("score".into(),CanonicalJson::Null)]).unwrap();
        let decision=CanonicalJson::object([("evidence".into(),CanonicalJson::String(ev.into())),("reason".into(),CanonicalJson::Null),("point".into(),actual.point.to_json()),("lower95".into(),actual.lower95.as_ref().unwrap().to_json()),("upper95".into(),actual.upper95.as_ref().unwrap().to_json()),("trend".into(),trend_json),("policy_outcome".into(),CanonicalJson::String(outcome.into()))]).unwrap();
        let policy_json=CanonicalJson::object([("min_candidate_samples".into(),CanonicalJson::Integer("1".into())),("min_baseline_samples".into(),CanonicalJson::Integer("1".into())),("baseline_generations".into(),CanonicalJson::Integer("5".into())),("bootstrap_resamples".into(),CanonicalJson::Integer("100".into())),("lower_rank".into(),CanonicalJson::Integer("50".into())),("upper_rank".into(),CanonicalJson::Integer("51".into())),("stale_after_seconds".into(),CanonicalJson::Integer("2592000".into())),("trend_generations".into(),CanonicalJson::Integer("5".into()))]).unwrap();
        let mut m=BTreeMap::from([("decision".into(),decision),("direction".into(),CanonicalJson::String("lower_is_better".into())),("enforcement".into(),CanonicalJson::String("fail".into())),("comparison".into(),CanonicalJson::object([("kind".into(),CanonicalJson::String("absolute_from".into())),("baseline".into(),CanonicalJson::String("main".into())),("limit".into(),q(50).to_json()),("direction".into(),CanonicalJson::String("at_most".into()))]).unwrap()),("policy".into(),policy_json),("samples".into(),CanonicalJson::Array(samples.iter().map(Rational::to_json).collect())),("baseline".into(),CanonicalJson::Null),("history".into(),CanonicalJson::object([("state_id".into(),CanonicalJson::String("4".repeat(64))),("report_ids".into(),CanonicalJson::Array(ids.iter().cloned().map(CanonicalJson::String).collect()))]).unwrap()),("metric".into(),CanonicalJson::object([("name".into(),CanonicalJson::String("BenchTime".into())),("percentile".into(),CanonicalJson::String("p50".into()))]).unwrap()),("context_key".into(),CanonicalJson::String(context))]);
        verify_wire_evaluation(&evidence,&m).unwrap();
        let original=m["decision"].clone();if let CanonicalJson::Object(d)=m.get_mut("decision").unwrap(){d.insert("upper95".into(),q(999).to_json());}assert!(verify_wire_evaluation(&evidence,&m).is_err());m.insert("decision".into(),original.clone());
        if let CanonicalJson::Object(d)=m.get_mut("decision").unwrap(){d.insert("policy_outcome".into(),CanonicalJson::String("warn".into()));}assert!(verify_wire_evaluation(&evidence,&m).is_err());m.insert("decision".into(),original);
        if let CanonicalJson::Object(h)=m.get_mut("history").unwrap(){h.insert("report_ids".into(),CanonicalJson::Array(vec![CanonicalJson::String("forged".into())]));}assert!(validate_history(&m["history"]).is_err());
    }
}
