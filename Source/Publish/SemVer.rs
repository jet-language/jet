// ──────────────────────────────────────────────
// SemVer 2.0.0 — full native parser (no external crates — I6)
// ──────────────────────────────────────────────
//
// Implements the complete Semantic Versioning 2.0.0 grammar:
//   - major.minor.patch with strict numeric identifiers (no leading zeros)
//   - dot-separated pre-release identifiers with spec precedence
//   - `+build` metadata (parsed, stored, ignored in precedence)
// and the node-semver range grammar for `VersionReq`:
//   - operators `=` `>` `>=` `<` `<=`
//   - caret `^`, tilde `~`, x-ranges (`1`, `1.2`, `1.x`, `*`)
//   - hyphen ranges (`1.2.3 - 2.3.4`)
//   - whitespace-AND inside a range, `||`-OR across ranges
//
// Everything normalizes to a set (OR) of comparator-sets (AND of `(op, version)`),
// which is the form node-semver canonicalizes to.

use std::cmp::Ordering;

/// A parsed SemVer version. `pre`/`build` keep their original dot-joined text;
/// precedence and equality follow the spec (build ignored, pre-release ordered).
#[derive(Debug, Clone)]
pub struct SemVer {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// Pre-release identifiers joined by `.`, e.g. `alpha.1`. `None` = a release.
    pub pre: Option<String>,
    /// Build metadata joined by `.`, e.g. `build.5`. Ignored in precedence.
    pub build: Option<String>,
}

impl SemVer {
    /// Parse a full `major.minor.patch[-pre][+build]` version.
    /// A leading `v` (common in tags) is tolerated. Returns `None` on any
    /// deviation from the SemVer 2.0.0 grammar.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let s = s.strip_prefix('v').unwrap_or(s);
        // Split build metadata (after the first `+`); build cannot contain `+`.
        let (rest, build) = match s.split_once('+') {
            Some((r, b)) => (r, Some(b)),
            None => (s, None),
        };
        // Split pre-release (after the first `-`); a pre-release identifier may
        // itself contain `-`, so split on the FIRST hyphen only.
        let (core, pre) = match rest.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (rest, None),
        };
        let mut parts = core.split('.');
        let major = parse_numeric_id(parts.next()?)?;
        let minor = parse_numeric_id(parts.next()?)?;
        let patch = parse_numeric_id(parts.next()?)?;
        if parts.next().is_some() {
            return None; // more than three core components
        }
        let pre = match pre {
            Some(p) => Some(validate_pre(p)?),
            None => None,
        };
        let build = match build {
            Some(b) => Some(validate_build(b)?),
            None => None,
        };
        Some(Self {
            major,
            minor,
            patch,
            pre,
            build,
        })
    }

    /// `true` when `self` is API-compatible with (an upgrade from) `other`
    /// under SemVer: same major and `self >= other` on minor.patch. This is the
    /// `^major` notion used for the toolchain-compat shorthand.
    pub fn is_compatible_with(&self, other: &SemVer) -> bool {
        self.major == other.major
            && (self.minor > other.minor
                || (self.minor == other.minor && self.patch >= other.patch))
    }

    /// SemVer precedence ordering (build metadata ignored; pre-release ordered
    /// below the matching release).
    fn precedence(&self, other: &SemVer) -> Ordering {
        self.major
            .cmp(&other.major)
            .then(self.minor.cmp(&other.minor))
            .then(self.patch.cmp(&other.patch))
            .then_with(|| cmp_pre(self.pre.as_deref(), other.pre.as_deref()))
    }
}

/// Parse a core numeric identifier: ASCII digits, no leading zeros (except `0`).
fn parse_numeric_id(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if s.len() > 1 && s.starts_with('0') {
        return None; // leading zero
    }
    s.parse::<u64>().ok()
}

/// A pre-release identifier is non-empty `[0-9A-Za-z-]`; numeric ones carry no
/// leading zeros. Returns the validated text unchanged.
fn validate_pre(s: &str) -> Option<String> {
    for id in s.split('.') {
        if id.is_empty() || !id.bytes().all(is_id_byte) {
            return None;
        }
        // Numeric identifier → no leading zeros.
        if id.bytes().all(|b| b.is_ascii_digit()) && id.len() > 1 && id.starts_with('0') {
            return None;
        }
    }
    Some(s.to_string())
}

/// A build identifier is non-empty `[0-9A-Za-z-]` (leading zeros allowed).
fn validate_build(s: &str) -> Option<String> {
    for id in s.split('.') {
        if id.is_empty() || !id.bytes().all(is_id_byte) {
            return None;
        }
    }
    Some(s.to_string())
}

fn is_id_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-'
}

/// Compare two optional pre-release strings under SemVer rules: no pre-release
/// (a release) outranks any pre-release; otherwise compare identifier lists.
fn cmp_pre(a: Option<&str>, b: Option<&str>) -> Ordering {
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater, // release > pre-release
        (Some(_), None) => Ordering::Less,
        (Some(a), Some(b)) => cmp_pre_ids(a, b),
    }
}

fn cmp_pre_ids(a: &str, b: &str) -> Ordering {
    let mut ai = a.split('.');
    let mut bi = b.split('.');
    loop {
        match (ai.next(), bi.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less, // shorter set < longer
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                let xn = x.bytes().all(|c| c.is_ascii_digit());
                let yn = y.bytes().all(|c| c.is_ascii_digit());
                let ord = match (xn, yn) {
                    // SemVer numeric pre-release identifiers are unbounded.
                    // Leading zeroes are already rejected, so length then
                    // lexical order compares them without integer overflow.
                    (true, true) => x.len().cmp(&y.len()).then_with(|| x.cmp(y)),
                    (true, false) => Ordering::Less, // numeric < alphanumeric
                    (false, true) => Ordering::Greater,
                    (false, false) => x.cmp(y),
                };
                if ord != Ordering::Equal {
                    return ord;
                }
            }
        }
    }
}

impl std::fmt::Display for SemVer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(pre) = &self.pre {
            write!(f, "-{}", pre)?;
        }
        if let Some(build) = &self.build {
            write!(f, "+{}", build)?;
        }
        Ok(())
    }
}

// Equality and ordering follow precedence (build metadata ignored).
impl PartialEq for SemVer {
    fn eq(&self, other: &Self) -> bool {
        self.precedence(other) == Ordering::Equal
    }
}
impl Eq for SemVer {}
impl PartialOrd for SemVer {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for SemVer {
    fn cmp(&self, other: &Self) -> Ordering {
        self.precedence(other)
    }
}

/// What kind of version bump is this (old → new)?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BumpKind {
    Major,
    Minor,
    Patch,
    Same,
}

/// Classify the bump from `old` to `new`.
pub fn classify_bump(old: &SemVer, new: &SemVer) -> BumpKind {
    if new.major > old.major {
        BumpKind::Major
    } else if new.minor > old.minor {
        BumpKind::Minor
    } else if new.patch > old.patch {
        BumpKind::Patch
    } else {
        BumpKind::Same
    }
}

// ──────────────────────────────────────────────
// Version requirements (ranges)
// ──────────────────────────────────────────────

/// Comparison operator in a single comparator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Lt,
    Lte,
    Gt,
    Gte,
    Eq,
}

/// A single comparator, e.g. `>=1.2.0`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comparator {
    pub op: Op,
    pub version: SemVer,
}

impl Comparator {
    fn matches(&self, v: &SemVer) -> bool {
        let ord = v.cmp(&self.version);
        match self.op {
            Op::Eq => ord == Ordering::Equal,
            Op::Lt => ord == Ordering::Less,
            Op::Lte => ord != Ordering::Greater,
            Op::Gt => ord == Ordering::Greater,
            Op::Gte => ord != Ordering::Less,
        }
    }
}

/// A version requirement: an OR of comparator-sets (each inner set is an AND).
/// An empty inner set matches every release. `raw` is the original text, used
/// for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionReq {
    pub sets: Vec<Vec<Comparator>>,
    pub raw: String,
}

impl VersionReq {
    /// Parse a node-semver-style range string. Returns `None` if any range in
    /// the set is malformed.
    pub fn parse(s: &str) -> Option<Self> {
        let raw = s.trim().to_string();
        if !raw.is_empty() && raw.split("||").any(|range| range.trim().is_empty()) {
            return None;
        }
        let mut sets = Vec::new();
        for range in raw.split("||") {
            sets.push(parse_range(range)?);
        }
        Some(VersionReq { sets, raw })
    }

    /// Does `candidate` satisfy this requirement?
    pub fn matches(&self, candidate: &SemVer) -> bool {
        self.sets.iter().any(|set| set_matches(set, candidate))
    }

    /// Could any single version satisfy both `self` and `other`? Used by the
    /// resolver to detect contradictory constraints without a live registry.
    pub fn intersects(&self, other: &VersionReq) -> bool {
        for a in &self.sets {
            for b in &other.sets {
                let mut merged = a.clone();
                merged.extend(b.iter().cloned());
                if set_satisfiable(&merged) {
                    return true;
                }
            }
        }
        false
    }

    /// The original requirement text (for diagnostics).
    pub fn display(&self) -> &str {
        if self.raw.is_empty() {
            "*"
        } else {
            &self.raw
        }
    }
}

/// A version with `*`/`x`/`X` wildcards or missing trailing components.
struct Partial {
    major: Option<u64>,
    minor: Option<u64>,
    patch: Option<u64>,
    pre: Option<String>,
    build: Option<String>,
}

fn parse_partial(s: &str) -> Option<Partial> {
    let s = s.trim();
    if s.is_empty() {
        return Some(Partial {
            major: None,
            minor: None,
            patch: None,
            pre: None,
            build: None,
        });
    }
    let (rest, build) = match s.split_once('+') {
        Some((r, b)) => (r, Some(validate_build(b)?)),
        None => (s, None),
    };
    let (core, pre) = match rest.split_once('-') {
        Some((c, p)) => (c, Some(validate_pre(p)?)),
        None => (rest, None),
    };
    let mut major = None;
    let mut minor = None;
    let mut patch = None;
    let mut seen_wild = false;
    for (i, part) in core.split('.').enumerate() {
        if i > 2 {
            return None;
        }
        let val = if matches!(part, "*" | "x" | "X" | "") {
            seen_wild = true;
            None
        } else {
            if seen_wild {
                return None; // a number after a wildcard, e.g. `1.x.3`
            }
            Some(parse_numeric_id(part)?)
        };
        match i {
            0 => major = val,
            1 => minor = val,
            _ => patch = val,
        }
    }
    Some(Partial {
        major,
        minor,
        patch,
        pre,
        build,
    })
}

/// A fully-specified version from a partial's components (missing → 0).
fn ver(major: u64, minor: u64, patch: u64) -> SemVer {
    SemVer {
        major,
        minor,
        patch,
        pre: None,
        build: None,
    }
}

fn successor(value: u64) -> Option<u64> {
    value.checked_add(1)
}

/// Parse one range (whitespace-AND of simples, or a hyphen range).
fn parse_range(range: &str) -> Option<Vec<Comparator>> {
    let range = range.trim();
    if range.is_empty() {
        return Some(vec![]); // matches any release
    }
    if let Some((lo, hi)) = range.split_once(" - ") {
        return expand_hyphen(lo.trim(), hi.trim());
    }
    let mut comps = Vec::new();
    for simple in split_simples(range) {
        comps.extend(parse_simple(&simple)?);
    }
    Some(comps)
}

/// Split a range into simples, re-attaching a bare operator token to the
/// version that follows it (`>= 1.2.3` → `>=1.2.3`).
fn split_simples(range: &str) -> Vec<String> {
    let toks: Vec<&str> = range.split_whitespace().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let t = toks[i];
        if matches!(t, ">" | ">=" | "<" | "<=" | "=" | "~" | "^") && i + 1 < toks.len() {
            out.push(format!("{}{}", t, toks[i + 1]));
            i += 2;
        } else {
            out.push(t.to_string());
            i += 1;
        }
    }
    out
}

fn parse_simple(s: &str) -> Option<Vec<Comparator>> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix('^') {
        return expand_caret(parse_partial(rest)?);
    }
    if let Some(rest) = s.strip_prefix('~') {
        // `~>1.2` is accepted as an alias for `~1.2`.
        let rest = rest.strip_prefix('>').unwrap_or(rest);
        return expand_tilde(parse_partial(rest)?);
    }
    let (op, rest) = if let Some(r) = s.strip_prefix(">=") {
        (Op::Gte, r)
    } else if let Some(r) = s.strip_prefix("<=") {
        (Op::Lte, r)
    } else if let Some(r) = s.strip_prefix('>') {
        (Op::Gt, r)
    } else if let Some(r) = s.strip_prefix('<') {
        (Op::Lt, r)
    } else if let Some(r) = s.strip_prefix('=') {
        (Op::Eq, r)
    } else {
        (Op::Eq, s)
    };
    expand_op(op, parse_partial(rest)?)
}

/// Expand an operator applied to a (possibly partial) version into comparators,
/// following node-semver's partial-version replacement rules.
fn expand_op(op: Op, p: Partial) -> Option<Vec<Comparator>> {
    let major = match p.major {
        Some(m) => m,
        None => return Some(vec![]), // `*` / `x` → any
    };
    let full = |pre: Option<String>, build: Option<String>| SemVer {
        major,
        minor: p.minor.unwrap_or(0),
        patch: p.patch.unwrap_or(0),
        pre,
        build,
    };
    let comps = match op {
        Op::Eq => match (p.minor, p.patch) {
            (None, _) => vec![
                Comparator {
                    op: Op::Gte,
                    version: ver(major, 0, 0),
                },
                Comparator {
                    op: Op::Lt,
                    version: ver(successor(major)?, 0, 0),
                },
            ],
            (Some(mi), None) => vec![
                Comparator {
                    op: Op::Gte,
                    version: ver(major, mi, 0),
                },
                Comparator {
                    op: Op::Lt,
                    version: ver(major, successor(mi)?, 0),
                },
            ],
            (Some(_), Some(_)) => {
                vec![Comparator {
                    op: Op::Eq,
                    version: full(p.pre.clone(), p.build.clone()),
                }]
            }
        },
        Op::Gt => {
            let c = match (p.minor, p.patch) {
                (None, _) => Comparator {
                    op: Op::Gte,
                    version: ver(successor(major)?, 0, 0),
                },
                (Some(mi), None) => Comparator {
                    op: Op::Gte,
                    version: ver(major, successor(mi)?, 0),
                },
                (Some(mi), Some(pa)) => Comparator {
                    op: Op::Gt,
                    version: SemVer {
                        major,
                        minor: mi,
                        patch: pa,
                        pre: p.pre.clone(),
                        build: p.build.clone(),
                    },
                },
            };
            vec![c]
        }
        Op::Lt => {
            let c = match (p.minor, p.patch) {
                (None, _) => Comparator {
                    op: Op::Lt,
                    version: ver(major, 0, 0),
                },
                (Some(mi), None) => Comparator {
                    op: Op::Lt,
                    version: ver(major, mi, 0),
                },
                (Some(mi), Some(pa)) => Comparator {
                    op: Op::Lt,
                    version: SemVer {
                        major,
                        minor: mi,
                        patch: pa,
                        pre: p.pre.clone(),
                        build: p.build.clone(),
                    },
                },
            };
            vec![c]
        }
        Op::Gte => vec![Comparator {
            op: Op::Gte,
            version: full(p.pre.clone(), p.build.clone()),
        }],
        Op::Lte => {
            let c = match (p.minor, p.patch) {
                (None, _) => Comparator {
                    op: Op::Lt,
                    version: ver(successor(major)?, 0, 0),
                },
                (Some(mi), None) => Comparator {
                    op: Op::Lt,
                    version: ver(major, successor(mi)?, 0),
                },
                (Some(mi), Some(pa)) => Comparator {
                    op: Op::Lte,
                    version: SemVer {
                        major,
                        minor: mi,
                        patch: pa,
                        pre: p.pre.clone(),
                        build: p.build.clone(),
                    },
                },
            };
            vec![c]
        }
    };
    Some(comps)
}

fn expand_caret(p: Partial) -> Option<Vec<Comparator>> {
    let major = match p.major {
        Some(m) => m,
        None => return Some(vec![]),
    };
    let lower = SemVer {
        major,
        minor: p.minor.unwrap_or(0),
        patch: p.patch.unwrap_or(0),
        pre: p.pre.clone(),
        build: None,
    };
    let upper = if major > 0 {
        ver(successor(major)?, 0, 0)
    } else {
        // major == 0
        match p.minor {
            None => ver(1, 0, 0),                    // ^0
            Some(mi) if mi > 0 => ver(0, successor(mi)?, 0), // ^0.2 / ^0.2.3
            Some(mi) => match p.patch {
                Some(pa) => ver(0, mi, successor(pa)?), // ^0.0.3
                None => ver(0, successor(mi)?, 0),      // ^0.0
            },
        }
    };
    Some(vec![
        Comparator {
            op: Op::Gte,
            version: lower,
        },
        Comparator {
            op: Op::Lt,
            version: upper,
        },
    ])
}

fn expand_tilde(p: Partial) -> Option<Vec<Comparator>> {
    let major = match p.major {
        Some(m) => m,
        None => return Some(vec![]),
    };
    let lower = SemVer {
        major,
        minor: p.minor.unwrap_or(0),
        patch: p.patch.unwrap_or(0),
        pre: p.pre.clone(),
        build: None,
    };
    let upper = match p.minor {
        Some(mi) => ver(major, successor(mi)?, 0), // ~1.2 / ~1.2.3 → <1.3.0
        None => ver(successor(major)?, 0, 0),      // ~1 → <2.0.0
    };
    Some(vec![
        Comparator {
            op: Op::Gte,
            version: lower,
        },
        Comparator {
            op: Op::Lt,
            version: upper,
        },
    ])
}

fn expand_hyphen(lo: &str, hi: &str) -> Option<Vec<Comparator>> {
    let lp = parse_partial(lo)?;
    let hp = parse_partial(hi)?;
    let mut comps = Vec::new();
    // Lower bound: missing components → 0, inclusive.
    let lmajor = lp.major.unwrap_or(0);
    comps.push(Comparator {
        op: Op::Gte,
        version: SemVer {
            major: lmajor,
            minor: lp.minor.unwrap_or(0),
            patch: lp.patch.unwrap_or(0),
            pre: lp.pre.clone(),
            build: None,
        },
    });
    // Upper bound: partial high → exclusive next; full → inclusive.
    let hmajor = hp.major?;
    let upper = match (hp.minor, hp.patch) {
        (None, _) => Comparator {
            op: Op::Lt,
            version: ver(successor(hmajor)?, 0, 0),
        },
        (Some(mi), None) => Comparator {
            op: Op::Lt,
            version: ver(hmajor, successor(mi)?, 0),
        },
        (Some(mi), Some(pa)) => Comparator {
            op: Op::Lte,
            version: SemVer {
                major: hmajor,
                minor: mi,
                patch: pa,
                pre: hp.pre.clone(),
                build: None,
            },
        },
    };
    comps.push(upper);
    Some(comps)
}

/// Does a version satisfy an AND-set? An empty set matches every release; a
/// pre-release version only matches if some comparator names the same
/// `major.minor.patch` tuple with a pre-release (node-semver rule).
fn set_matches(set: &[Comparator], v: &SemVer) -> bool {
    if !set.iter().all(|c| c.matches(v)) {
        return false;
    }
    if v.pre.is_some() {
        let allowed = set.iter().any(|c| {
            c.version.pre.is_some()
                && c.version.major == v.major
                && c.version.minor == v.minor
                && c.version.patch == v.patch
        });
        if !allowed {
            return false;
        }
    }
    true
}

/// Is there any release version satisfying every comparator in `set`?
/// Computed by intersecting the lower/upper bounds (pre-release ignored — used
/// only for resolver disjointness, which reasons about release ranges).
fn set_satisfiable(set: &[Comparator]) -> bool {
    // Lower bound (version, inclusive) and upper bound (version, inclusive).
    let mut lower: Option<(SemVer, bool)> = None;
    let mut upper: Option<(SemVer, bool)> = None;
    let release = |v: &SemVer| SemVer {
        major: v.major,
        minor: v.minor,
        patch: v.patch,
        pre: None,
        build: None,
    };
    for c in set {
        let v = release(&c.version);
        match c.op {
            Op::Gte => tighten_lower(&mut lower, v, true),
            Op::Gt => tighten_lower(&mut lower, v, false),
            Op::Lte => tighten_upper(&mut upper, v, true),
            Op::Lt => tighten_upper(&mut upper, v, false),
            Op::Eq => {
                tighten_lower(&mut lower, v.clone(), true);
                tighten_upper(&mut upper, v, true);
            }
        }
    }
    match (lower, upper) {
        (Some((lo, li)), Some((hi, ui))) => match lo.cmp(&hi) {
            Ordering::Less => true,
            Ordering::Greater => false,
            Ordering::Equal => li && ui,
        },
        _ => true,
    }
}

fn tighten_lower(cur: &mut Option<(SemVer, bool)>, v: SemVer, incl: bool) {
    match cur {
        None => *cur = Some((v, incl)),
        Some((c, ci)) => match v.cmp(c) {
            Ordering::Greater => *cur = Some((v, incl)),
            Ordering::Equal => *ci = *ci && incl, // tighter bound wins (exclusive)
            Ordering::Less => {}
        },
    }
}

fn tighten_upper(cur: &mut Option<(SemVer, bool)>, v: SemVer, incl: bool) {
    match cur {
        None => *cur = Some((v, incl)),
        Some((c, ci)) => match v.cmp(c) {
            Ordering::Less => *cur = Some((v, incl)),
            Ordering::Equal => *ci = *ci && incl,
            Ordering::Greater => {}
        },
    }
}
