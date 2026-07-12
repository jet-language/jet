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
        let mut parser = JsonParser { bytes: &text.as_bytes()[..text.len() - 1], at: 0 };
        let value = parser.value()?;
        if parser.at != parser.bytes.len() { return Err("trailing bytes after canonical JSON value".into()); }
        if value.bytes() != bytes { return Err("JSON is valid but not A-canonical".into()); }
        Ok(value)
    }
}

struct JsonParser<'a> { bytes: &'a [u8], at: usize }
impl<'a> JsonParser<'a> {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rational {
    pub num: i128,
    pub den: i128,
}

impl Rational {
    pub const ZERO: Rational = Rational { num: 0, den: 1 };

    pub fn new(num: i128, den: i128) -> Result<Self, String> {
        if den == 0 { return Err("rational denominator is zero".into()); }
        let (num, den) = if den < 0 { (-num, -den) } else { (num, den) };
        if num == 0 { return Ok(Self::ZERO); }
        let divisor = gcd(num.unsigned_abs(), den as u128) as i128;
        Ok(Self { num: num / divisor, den: den / divisor })
    }

    pub fn integer(value: i128) -> Self { Self { num: value, den: 1 } }
    pub fn add(self, rhs: Self) -> Result<Self, String> {
        let left = self.num.checked_mul(rhs.den).ok_or("rational addition overflow")?;
        let right = rhs.num.checked_mul(self.den).ok_or("rational addition overflow")?;
        let den = self.den.checked_mul(rhs.den).ok_or("rational addition overflow")?;
        Self::new(left.checked_add(right).ok_or("rational addition overflow")?, den)
    }
    pub fn sub(self, rhs: Self) -> Result<Self, String> { self.add(rhs.neg()) }
    pub fn mul(self, rhs: Self) -> Result<Self, String> {
        Self::new(self.num.checked_mul(rhs.num).ok_or("rational multiplication overflow")?, self.den.checked_mul(rhs.den).ok_or("rational multiplication overflow")?)
    }
    pub fn div(self, rhs: Self) -> Result<Self, String> {
        if rhs.num == 0 { return Err("rational division by zero".into()); }
        Self::new(self.num.checked_mul(rhs.den).ok_or("rational division overflow")?, self.den.checked_mul(rhs.num).ok_or("rational division overflow")?)
    }
    pub fn neg(self) -> Self { Self { num: -self.num, den: self.den } }
    pub fn abs(self) -> Self { if self.num < 0 { self.neg() } else { self } }
    pub fn max_zero(self) -> Self { if self.num < 0 { Self::ZERO } else { self } }
    pub fn to_json(self) -> CanonicalJson {
        CanonicalJson::object([
            ("den".into(), CanonicalJson::Integer(self.den.to_string())),
            ("num".into(), CanonicalJson::Integer(self.num.to_string())),
        ]).expect("unique rational keys")
    }
}

impl Ord for Rational {
    fn cmp(&self, rhs: &Self) -> Ordering {
        match (self.num < 0, rhs.num < 0) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (false, false) => compare_unsigned_fractions(self.num as u128, self.den as u128, rhs.num as u128, rhs.den as u128),
            (true, true) => compare_unsigned_fractions(self.num.unsigned_abs(), self.den as u128, rhs.num.unsigned_abs(), rhs.den as u128).reverse(),
        }
    }
}
impl PartialOrd for Rational { fn partial_cmp(&self, rhs: &Self) -> Option<Ordering> { Some(self.cmp(rhs)) } }

fn gcd(mut left: u128, mut right: u128) -> u128 {
    while right != 0 { let remainder = left % right; left = right; right = remainder; }
    left
}

fn compare_unsigned_fractions(mut an: u128, mut ad: u128, mut bn: u128, mut bd: u128) -> Ordering {
    let mut reverse = false;
    loop {
        let aq = an / ad;
        let bq = bn / bd;
        if aq != bq { return if reverse { bq.cmp(&aq) } else { aq.cmp(&bq) }; }
        let ar = an % ad;
        let br = bn % bd;
        match (ar == 0, br == 0) {
            (true, true) => return Ordering::Equal,
            (true, false) => return if reverse { Ordering::Greater } else { Ordering::Less },
            (false, true) => return if reverse { Ordering::Less } else { Ordering::Greater },
            (false, false) => { an = ad; ad = ar; bn = bd; bd = br; reverse = !reverse; }
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
    let mut sum = Rational::ZERO;
    for sample in &sorted { sum = sum.add(*sample)?; }
    let mean = sum.div(Rational::integer(sorted.len() as i128))?;
    let mut deviations = sorted.iter().map(|sample| sample.sub(p50).map(Rational::abs)).collect::<Result<Vec<_>, _>>()?;
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
    Ok(sorted[rank.max(1) - 1])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Percentile { P50, P90, P95, P99, P999 }
pub fn estimator(samples: &[Rational], percentile: Option<Percentile>) -> Result<Rational, String> {
    if samples.len() == 1 && percentile.is_none() { return Ok(samples[0]); }
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
    RelativeTo { limit_basis_points: i128, goal: RelativeGoal },
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
            let point = match limit_direction { LimitDirection::AtMost => candidate_point.sub(*limit)?, LimitDirection::AtLeast => limit.sub(candidate_point)? };
            let policy = policy.ok_or("AbsoluteFrom requires a measurement policy")?;
            let values = bootstrap_values(evidence_id, context_key, baseline_report_ids, candidate, &[], percentile, comparison, direction, policy)?;
            (point, values)
        }
        Comparison::RelativeTo { limit_basis_points, goal } => {
            if baseline.is_empty() { return Err("RelativeTo baseline samples are empty".into()); }
            let baseline_point = estimator(baseline, percentile)?;
            if baseline_point == Rational::ZERO { return Ok(finish(Rational::ZERO, None, None, Evidence::Unavailable, enforcement, Vec::new())); }
            let point = relative_stat(candidate_point, baseline_point, *limit_basis_points, *goal, direction)?;
            let policy = policy.ok_or("RelativeTo requires a measurement policy")?;
            let values = bootstrap_values(evidence_id, context_key, baseline_report_ids, candidate, baseline, percentile, comparison, direction, policy)?;
            (point, values)
        }
    };
    let policy = policy.expect("statistical branches require policy");
    if policy.lower_rank == 0 || policy.upper_rank == 0 || policy.lower_rank > bootstrap.len() || policy.upper_rank > bootstrap.len() {
        return Err("bootstrap ranks are outside the replicate set".into());
    }
    let lower = bootstrap[policy.lower_rank - 1];
    let upper = bootstrap[policy.upper_rank - 1];
    let evidence = if upper <= Rational::ZERO { Evidence::Pass } else if lower > Rational::ZERO { Evidence::Regression } else { Evidence::Inconclusive };
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
            Comparison::AbsoluteFrom { limit, direction } => match direction { LimitDirection::AtMost => candidate_estimator.sub(*limit)?, LimitDirection::AtLeast => limit.sub(candidate_estimator)? },
            Comparison::RelativeTo { limit_basis_points, goal } => {
                let baseline_resample = resample(baseline, baseline.len(), &mut stream)?;
                let baseline_estimator = estimator(&baseline_resample, percentile)?;
                if baseline_estimator == Rational::ZERO { return Err("RelativeTo bootstrap baseline estimator is zero".into()); }
                relative_stat(candidate_estimator, baseline_estimator, *limit_basis_points, *goal, direction)?
            }
            Comparison::Absolute { .. } => return Err("Absolute does not bootstrap".into()),
        };
        values.push(value);
    }
    values.sort();
    Ok(values)
}

fn relative_stat(candidate: Rational, baseline: Rational, limit_basis_points: i128, goal: RelativeGoal, direction: Direction) -> Result<Rational, String> {
    let delta = match (goal, direction) {
        (RelativeGoal::RegressionAtMost, Direction::LowerIsBetter) | (RelativeGoal::ImprovementAtLeast, Direction::HigherIsBetter) => candidate.sub(baseline)?.max_zero(),
        (RelativeGoal::RegressionAtMost, Direction::HigherIsBetter) | (RelativeGoal::ImprovementAtLeast, Direction::LowerIsBetter) => baseline.sub(candidate)?.max_zero(),
    };
    let basis_points = delta.mul(Rational::integer(10_000))?.div(baseline)?;
    match goal { RelativeGoal::RegressionAtMost => basis_points.sub(Rational::integer(limit_basis_points)), RelativeGoal::ImprovementAtLeast => Rational::integer(limit_basis_points).sub(basis_points) }
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
    (0..count).map(|_| stream.index(values.len()).map(|index| values[index])).collect()
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
            let numerator = match direction { Direction::LowerIsBetter => estimators[left].sub(estimators[right])?, Direction::HigherIsBetter => estimators[right].sub(estimators[left])? };
            slopes.push(numerator.div(Rational::integer((right - left) as i128))?);
        }
    }
    slopes.sort();
    let score = nearest_rank(&slopes, 500, 1000)?;
    let label = if score > Rational::ZERO { TrendLabel::Improving } else if score < Rational::ZERO { TrendLabel::Regressing } else { TrendLabel::Stable };
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
    let claimed = match fields.get("report_id") { Some(CanonicalJson::String(id)) => id, _ => return Err("budget report_id is not text".into()) };
    if claimed.len() != 64 || !claimed.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) { return Err("budget report_id is not lowercase Hex64".into()); }
    verify_stable_id(content, claimed)?;
    Ok(report)
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
        let comparison = Comparison::RelativeTo { limit_basis_points: 500, goal: RelativeGoal::RegressionAtMost };
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
        assert_eq!(verify_budget_report(&bytes).unwrap(), report);
        let spaced = String::from_utf8(bytes.clone()).unwrap().replace("{\"content\"", "{ \"content\"");
        assert!(verify_budget_report(spaced.as_bytes()).is_err());
        let corrupt = String::from_utf8(bytes).unwrap().replace("\"pass\"", "\"fail\"");
        assert!(verify_budget_report(corrupt.as_bytes()).is_err());
    }
}
