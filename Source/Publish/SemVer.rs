// ──────────────────────────────────────────────
// SemVer (no external crates — I6)
// ──────────────────────────────────────────────

/// A parsed SemVer version (major.minor.patch), with optional pre-release and
/// build metadata stripped. Pre-release is stored but does not influence range
/// matching (matching is exact for registry deps, range for SemVer checking).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemVer {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// Pre-release identifier, e.g. `alpha.1`. Stored for display only.
    pub pre: Option<String>,
}

impl SemVer {
    /// Parse `"major.minor.patch[-pre]"`. Returns `None` on any parse failure.
    pub fn parse(s: &str) -> Option<Self> {
        // Strip a leading `v` (common in tags).
        let s = s.strip_prefix('v').unwrap_or(s);
        // Split off pre-release.
        let (version_part, pre) = if let Some((v, p)) = s.split_once('-') {
            (v, Some(p.to_string()))
        } else {
            (s, None)
        };
        let parts: Vec<&str> = version_part.splitn(3, '.').collect();
        if parts.len() < 3 {
            return None;
        }
        let major = parts[0].parse::<u64>().ok()?;
        let minor = parts[1].parse::<u64>().ok()?;
        let patch = parts[2].parse::<u64>().ok()?;
        Some(Self { major, minor, patch, pre })
    }

    /// `true` when `other` is API-compatible under SemVer (same major, >=
    /// minor.patch). This is what `^major.0` means: any `major.x.y >= major.0.0`.
    pub fn is_compatible_with(&self, other: &SemVer) -> bool {
        self.major == other.major
            && (self.minor > other.minor
                || (self.minor == other.minor && self.patch >= other.patch))
    }
}

impl std::fmt::Display for SemVer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(pre) = &self.pre {
            write!(f, "-{}", pre)?;
        }
        Ok(())
    }
}

impl PartialOrd for SemVer {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SemVer {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.major.cmp(&other.major)
            .then(self.minor.cmp(&other.minor))
            .then(self.patch.cmp(&other.patch))
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

/// Parse a SemVer version-requirement like `^1.2`, `>=1.0.0 <2.0.0`, or `1.2.3`.
/// For M8 we only implement `^` (caret) and `*` (any); exact versions are also accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionReq {
    /// `^major[.minor[.patch]]` — compatible range.
    /// `precision` records how many components were specified (1, 2, or 3).
    Caret { floor: SemVer, precision: u8 },
    /// Exact `major.minor.patch` match.
    Exact(SemVer),
    /// `*` or empty — any version.
    Any,
}

impl VersionReq {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if s == "*" || s.is_empty() {
            return Some(VersionReq::Any);
        }
        if let Some(inner) = s.strip_prefix('^') {
            let inner = inner.trim();
            let precision = inner.splitn(3, '.').count() as u8;
            let padded = pad_semver(inner);
            return SemVer::parse(&padded).map(|sv| VersionReq::Caret { floor: sv, precision });
        }
        SemVer::parse(s).map(VersionReq::Exact)
    }

    /// Does `candidate` satisfy this requirement?
    pub fn matches(&self, candidate: &SemVer) -> bool {
        match self {
            VersionReq::Any => true,
            VersionReq::Exact(v) => candidate == v,
            VersionReq::Caret { floor, precision } => {
                // Semantics (Cargo/npm compatible):
                //   ^1         (precision=1) → 1.x.y  (any >=1.0.0 <2.0.0)
                //   ^1.2       (precision=2) → 1.2.x  when major>0; 0.2.x when major=0
                //   ^1.2.3     (precision=3) → same major (or same minor if major=0, or same minor+patch if 0.0.x)
                if *precision == 1 {
                    // ^N → same major, any minor/patch
                    candidate.major == floor.major && *candidate >= *floor
                } else if *precision == 2 {
                    if floor.major == 0 {
                        // ^0.N → same minor (0.N.x)
                        candidate.major == 0 && candidate.minor == floor.minor && candidate.patch >= floor.patch
                    } else {
                        // ^M.N → same major, minor >= N
                        candidate.major == floor.major && *candidate >= *floor
                    }
                } else {
                    // precision == 3
                    if floor.major == 0 && floor.minor == 0 {
                        // ^0.0.P → exact match on patch
                        candidate.major == 0 && candidate.minor == 0 && candidate.patch >= floor.patch
                    } else if floor.major == 0 {
                        // ^0.M.P → same minor
                        candidate.major == 0 && candidate.minor == floor.minor && candidate.patch >= floor.patch
                    } else {
                        // ^M.N.P → same major
                        candidate.major == floor.major && *candidate >= *floor
                    }
                }
            }
        }
    }
}

fn pad_semver(s: &str) -> String {
    let parts = s.splitn(3, '.').count();
    match parts {
        1 => format!("{}.0.0", s),
        2 => format!("{}.0", s),
        _ => s.to_string(),
    }
}
