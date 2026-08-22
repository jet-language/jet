use crate::Diagnostics::Diagnostic;
use crate::Lock::{LockFile, LockSource, LockedPackage};
use std::collections::BTreeSet;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use super::SemVer::{SemVer, VersionReq};

// ──────────────────────────────────────────────
// Signed advisory feed
// ──────────────────────────────────────────────

/// Advisory severity (D-SUPPLY1). `jet inspect audit` exits nonzero only when a
/// `Critical` advisory matches; lower severities are advisory and exit 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    /// Parse a severity word (case-insensitive). Unknown / empty → `Medium`,
    /// so a database that omits the field is treated as advisory, not fatal.
    pub fn parse(s: &str) -> Severity {
        match s.trim().to_ascii_lowercase().as_str() {
            "low" => Severity::Low,
            "high" => Severity::High,
            "critical" | "crit" => Severity::Critical,
            _ => Severity::Medium,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Severity::Low => "low",
            Severity::Medium => "medium",
            Severity::High => "high",
            Severity::Critical => "critical",
        }
    }
}

/// One advisory entry inside the canonical signed feed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Advisory {
    /// Unique identifier, e.g. `JET-2026-0001` or a CVE ID.
    pub id: String,
    pub package: String,
    /// Version range where the vulnerability is present.
    pub affected: VersionReq,
    /// First version where the fix is available, if known.
    pub fixed: Option<SemVer>,
    pub title: String,
    /// Advisory severity — only `Critical` makes `jet inspect audit` exit nonzero.
    pub severity: Severity,
}

impl Advisory {
    /// Does `version` fall within the affected range?
    pub fn affects(&self, version: &SemVer) -> bool {
        self.affected.matches(version) && self.fixed.as_ref().map(|f| version < f).unwrap_or(true)
    }
}

// Signed offline feed and package policy.

pub const ADVISORY_FEED_MAGIC: &str = "jet-advisory-feed-v1";
pub const DEFAULT_MATURITY_SECONDS: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SourceClass {
    ThirdParty,
    FirstParty,
    Workspace,
}

impl SourceClass {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "third-party" => Some(Self::ThirdParty),
            "first-party" => Some(Self::FirstParty),
            "workspace" => Some(Self::Workspace),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::ThirdParty => "third-party",
            Self::FirstParty => "first-party",
            Self::Workspace => "workspace",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvisoryRelease {
    pub package: String,
    pub version: SemVer,
    pub first_seen: u64,
    pub source_class: SourceClass,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvisoryException {
    pub package: String,
    pub version: SemVer,
    pub reason: String,
    pub reviewer: String,
    pub expires_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvisoryFeed {
    pub sequence: u64,
    pub issued_at: u64,
    pub expires_at: u64,
    pub maturity_seconds: u64,
    pub key_id: String,
    pub public_key: String,
    pub signature: String,
    pub releases: Vec<AdvisoryRelease>,
    pub advisories: Vec<Advisory>,
    pub exceptions: Vec<AdvisoryException>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AdvisoryTrustRoot {
    pub public_key: String,
    pub min_sequence: u64,
    pub accepted_digest: Option<String>,
    pub revoked_keys: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvisoryReceipt {
    pub sequence: u64,
    pub digest: String,
    pub key_id: String,
    pub issued_at: u64,
    pub expires_at: u64,
    pub maturity_seconds: u64,
}

#[derive(Debug)]
pub struct PolicyAudit {
    pub receipt: AdvisoryReceipt,
    pub matches: Vec<AuditMatch>,
    pub maturity: Vec<Diagnostic>,
}

/// The verified offline policy snapshot used by package resolution. The
/// resolver receives this only after the feed, pinned key, sequence, digest,
/// clock, and signature have all passed verification.
#[derive(Debug, Clone)]
pub struct AdvisoryPolicy {
    pub feed: AdvisoryFeed,
    pub trust: AdvisoryTrustRoot,
    pub receipt: AdvisoryReceipt,
    pub now: u64,
}

/// Load the project-local advisory policy, if one is configured. An absent
/// feed keeps ordinary local development usable; once a feed is present, every
/// read failure or trust failure is fatal rather than silently downgrading to
/// an unaudited resolution.
pub fn load_advisory_policy(project_root: &Path) -> Result<Option<AdvisoryPolicy>, Diagnostic> {
    let feed_path = std::env::var_os("JET_ADVISORY_DB")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            let path = project_root.join(".jet").join("advisories.db");
            path.is_file().then_some(path)
        });
    let Some(feed_path) = feed_path else {
        return Ok(None);
    };
    let feed_text = std::fs::read_to_string(&feed_path).map_err(|error| {
        e2610(
            "advisory database",
            &format!("could not read `{}`: {error}", feed_path.display()),
        )
    })?;
    let feed = parse_advisory_feed(&feed_text)?;
    let trust_path = std::env::var_os("JET_ADVISORY_TRUST")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| project_root.join(".jet").join("advisory-trust"));
    let trust = if let Some(public_key) = std::env::var_os("JET_ADVISORY_PUBLIC_KEY") {
        AdvisoryTrustRoot {
            public_key: public_key.to_string_lossy().trim().to_string(),
            ..Default::default()
        }
    } else {
        let trust_text = std::fs::read_to_string(&trust_path).map_err(|error| {
            e2610(
                "advisory trust root",
                &format!("could not read `{}`: {error}", trust_path.display()),
            )
        })?;
        parse_advisory_trust(&trust_text)?
    };
    let now = advisory_now();
    let receipt = verify_advisory_feed(&feed, &trust, now)?;
    Ok(Some(AdvisoryPolicy {
        feed,
        trust,
        receipt,
        now,
    }))
}

/// Admit one newly selected registry version under an already verified feed.
/// Existing exact locks bypass this function: freshness never moves an
/// already locked or realized environment.
pub fn authorize_registry_candidate(
    policy: &AdvisoryPolicy,
    package: &str,
    version: &str,
) -> Result<(), Diagnostic> {
    let version = SemVer::parse(version).ok_or_else(|| {
        e2610(
            "registry package",
            &format!("{package} has an invalid selected version `{version}`"),
        )
    })?;
    let Some(release) = policy
        .feed
        .releases
        .iter()
        .find(|release| release.package == package && release.version == version)
    else {
        return Err(e2610(
            "advisory feed",
            &format!("no trusted release record exists for {package}#{version}"),
        ));
    };
    let required = match release.source_class {
        SourceClass::ThirdParty => policy.receipt.maturity_seconds,
        SourceClass::FirstParty | SourceClass::Workspace => 0,
    };
    let mature_at = release.first_seen.saturating_add(required);
    let excepted = policy.feed.exceptions.iter().any(|exception| {
        exception.package == package
            && exception.version == version
            && exception.expires_at > policy.now
    });
    if policy.now < mature_at && !excepted {
        return Err(e2609(package, &version.to_string(), mature_at, release.source_class));
    }
    if let Some(advisory) = policy
        .feed
        .advisories
        .iter()
        .find(|advisory| advisory.package == package && advisory.affects(&version))
    {
        return Err(e2603(
            &advisory.id,
            package,
            &version.to_string(),
            &advisory.title,
            advisory.severity,
            advisory.fixed.as_ref(),
        ));
    }
    Ok(())
}

pub fn parse_advisory_feed(text: &str) -> Result<AdvisoryFeed, Diagnostic> {
    let mut lines = text.lines().enumerate().filter_map(|(number, raw)| {
        let line = raw.trim();
        (!line.is_empty() && !line.starts_with('#')).then_some((number + 1, line))
    });
    let Some((magic_line, magic)) = lines.next() else {
        return Err(e2607("advisory feed", "the signed feed is empty"));
    };
    if magic != ADVISORY_FEED_MAGIC {
        return Err(e2607(
            "advisory feed",
            &format!("line {magic_line} must start with {ADVISORY_FEED_MAGIC}"),
        ));
    }
    let Some((header_line, header)) = lines.next() else {
        return Err(e2607("advisory feed", "the signed feed has no feed header"));
    };
    let fields: Vec<_> = header.split('|').collect();
    if fields.len() != 8 || fields[0] != "feed" {
        return Err(e2607(
            "advisory feed",
            &format!("line {header_line} must contain eight pipe-separated feed fields"),
        ));
    }
    let parse_u64 = |value: &str, label: &str| {
        value.trim().parse::<u64>().map_err(|_| {
            e2607(
                "advisory feed",
                &format!("line {header_line} has an invalid {label}"),
            )
        })
    };
    let sequence = parse_u64(fields[1], "sequence")?;
    let issued_at = parse_u64(fields[2], "issued timestamp")?;
    let expires_at = parse_u64(fields[3], "expiry timestamp")?;
    let maturity_seconds = parse_u64(fields[4], "maturity window")?;
    if sequence == 0 || issued_at == 0 || expires_at <= issued_at {
        return Err(e2607(
            "advisory feed",
            &format!("line {header_line} has a non-monotonic sequence or time range"),
        ));
    }
    if fields[5].trim().is_empty() || fields[6].trim().is_empty() || fields[7].trim().is_empty() {
        return Err(e2607(
            "advisory feed",
            &format!("line {header_line} has an empty trust or signature field"),
        ));
    }
    let feed = parse_feed_records(
        sequence,
        issued_at,
        expires_at,
        maturity_seconds,
        fields[5].trim().to_string(),
        fields[6].trim().to_string(),
        fields[7].trim().to_string(),
        lines,
    )?;
    validate_feed_identity(&feed)?;
    Ok(feed)
}

fn parse_feed_records<'a>(
    sequence: u64,
    issued_at: u64,
    expires_at: u64,
    maturity_seconds: u64,
    key_id: String,
    public_key: String,
    signature: String,
    lines: impl Iterator<Item = (usize, &'a str)>,
) -> Result<AdvisoryFeed, Diagnostic> {
    let mut releases = Vec::new();
    let mut advisories = Vec::new();
    let mut exceptions = Vec::new();
    let mut release_keys = BTreeSet::new();
    let mut advisory_ids = BTreeSet::new();
    let mut exception_keys = BTreeSet::new();
    for (line_number, line) in lines {
        let fields: Vec<_> = line.split('|').collect();
        match fields.first().copied() {
            Some("release") if fields.len() == 4 => {
                let (package, version) = parse_exact_target(fields[1], line_number)?;
                let first_seen = fields[2].trim().parse::<u64>().map_err(|_| {
                    e2607(
                        "advisory feed",
                        &format!("line {line_number} has an invalid release timestamp"),
                    )
                })?;
                let source_class = SourceClass::parse(fields[3]).ok_or_else(|| {
                    e2607(
                        "advisory feed",
                        &format!("line {line_number} has an unknown source class"),
                    )
                })?;
                let key = format!("{package}#{version}");
                if !release_keys.insert(key) || first_seen == 0 || first_seen > issued_at {
                    return Err(e2607(
                        "advisory feed",
                        &format!("line {line_number} repeats a release or has no first-seen time"),
                    ));
                }
                releases.push(AdvisoryRelease {
                    package,
                    version,
                    first_seen,
                    source_class,
                });
            }
            Some("advisory") if fields.len() == 7 => {
                let id = fields[1].trim();
                let package = fields[2].trim();
                let title = fields[5].trim();
                if id.is_empty() || package.is_empty() || title.is_empty() {
                    return Err(e2607(
                        "advisory feed",
                        &format!("line {line_number} has an empty advisory identity"),
                    ));
                }
                if !advisory_ids.insert(id.to_string()) {
                    return Err(e2607(
                        "advisory feed",
                        &format!("line {line_number} repeats advisory {id}"),
                    ));
                }
                let affected = VersionReq::parse(fields[3].trim()).ok_or_else(|| {
                    e2607(
                        "advisory feed",
                        &format!("line {line_number} has an invalid affected version range"),
                    )
                })?;
                let fixed = if fields[4].trim().is_empty() {
                    None
                } else {
                    Some(SemVer::parse(fields[4].trim()).ok_or_else(|| {
                        e2607(
                            "advisory feed",
                            &format!("line {line_number} has an invalid fixed version"),
                        )
                    })?)
                };
                let severity = match fields[6].trim().to_ascii_lowercase().as_str() {
                    "low" => Severity::Low,
                    "medium" => Severity::Medium,
                    "high" => Severity::High,
                    "critical" => Severity::Critical,
                    _ => {
                        return Err(e2607(
                            "advisory feed",
                            &format!("line {line_number} has an unknown severity"),
                        ));
                    }
                };
                advisories.push(Advisory {
                    id: id.to_string(),
                    package: package.to_string(),
                    affected,
                    fixed,
                    title: title.to_string(),
                    severity,
                });
            }
            Some("exception") if fields.len() == 5 => {
                let (package, version) = parse_exact_target(fields[1], line_number)?;
                let reason = fields[2].trim();
                let reviewer = fields[3].trim();
                let expires_at = fields[4].trim().parse::<u64>().map_err(|_| {
                    e2607(
                        "advisory feed",
                        &format!("line {line_number} has an invalid exception expiry"),
                    )
                })?;
                let key = format!("{package}#{version}");
                if reason.is_empty()
                    || reviewer.is_empty()
                    || expires_at <= issued_at
                    || !exception_keys.insert(key)
                {
                    return Err(e2607(
                        "advisory feed",
                        &format!("line {line_number} has an invalid or repeated exact exception"),
                    ));
                }
                exceptions.push(AdvisoryException {
                    package,
                    version,
                    reason: reason.to_string(),
                    reviewer: reviewer.to_string(),
                    expires_at,
                });
            }
            _ => {
                return Err(e2607(
                    "advisory feed",
                    &format!("line {line_number} has an unknown record or field count"),
                ));
            }
        }
    }
    Ok(AdvisoryFeed {
        sequence,
        issued_at,
        expires_at,
        maturity_seconds,
        key_id,
        public_key,
        signature,
        releases,
        advisories,
        exceptions,
    })
}

fn parse_exact_target(raw: &str, line_number: usize) -> Result<(String, SemVer), Diagnostic> {
    if raw.contains('@') {
        return Err(e2607(
            "advisory feed",
            &format!("line {line_number} uses @; exact policy targets use package#version"),
        ));
    }
    let Some((package, version)) = raw.trim().split_once('#') else {
        return Err(e2607(
            "advisory feed",
            &format!("line {line_number} needs an exact package#version target"),
        ));
    };
    let package = package.trim();
    let version = SemVer::parse(version.trim()).ok_or_else(|| {
        e2607(
            "advisory feed",
            &format!("line {line_number} has an invalid exact version"),
        )
    })?;
    if package.is_empty() {
        return Err(e2607(
            "advisory feed",
            &format!("line {line_number} has an empty exact package name"),
        ));
    }
    Ok((package.to_string(), version))
}

fn validate_feed_identity(feed: &AdvisoryFeed) -> Result<(), Diagnostic> {
    let key = feed.public_key.trim();
    let Some(expected_id) = advisory_key_id(key) else {
        return Err(e2610(
            "advisory feed",
            "the publisher public key is malformed",
        ));
    };
    if feed.key_id != expected_id {
        return Err(e2610(
            "advisory feed",
            "the key id does not match the publisher key",
        ));
    }
    Ok(())
}

/// Return the stable identity for a 32-byte Ed25519 public key written as
/// lowercase or uppercase hexadecimal text.
pub fn advisory_key_id(public_key: &str) -> Option<String> {
    let public_key = public_key.trim();
    if public_key.len() != 64 {
        return None;
    }
    let mut bytes = [0u8; 32];
    for (index, pair) in public_key.as_bytes().chunks_exact(2).enumerate() {
        let high = hex_nibble(pair[0])?;
        let low = hex_nibble(pair[1])?;
        bytes[index] = (high << 4) | low;
    }
    Some(format!("sha256-{}", crate::SHA256::sha256_hex(&bytes)))
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub fn advisory_feed_payload(feed: &AdvisoryFeed) -> String {
    let mut out = format!(
        "{}\nfeed|{}|{}|{}|{}|{}|{}\n",
        ADVISORY_FEED_MAGIC,
        feed.sequence,
        feed.issued_at,
        feed.expires_at,
        feed.maturity_seconds,
        feed.key_id,
        feed.public_key,
    );
    let mut releases = feed.releases.clone();
    releases.sort_by(|left, right| {
        left.package
            .cmp(&right.package)
            .then(left.version.cmp(&right.version))
    });
    for release in releases {
        out.push_str(&format!(
            "release|{}#{}|{}|{}\n",
            release.package,
            release.version,
            release.first_seen,
            release.source_class.label()
        ));
    }
    let mut advisories = feed.advisories.clone();
    advisories.sort_by(|left, right| left.id.cmp(&right.id));
    for advisory in advisories {
        out.push_str(&format!(
            "advisory|{}|{}|{}|{}|{}|{}\n",
            advisory.id,
            advisory.package,
            advisory.affected.raw,
            advisory
                .fixed
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default(),
            advisory.title,
            advisory.severity.label(),
        ));
    }
    let mut exceptions = feed.exceptions.clone();
    exceptions.sort_by(|left, right| {
        left.package
            .cmp(&right.package)
            .then(left.version.cmp(&right.version))
    });
    for exception in exceptions {
        out.push_str(&format!(
            "exception|{}#{}|{}|{}|{}\n",
            exception.package,
            exception.version,
            exception.reason,
            exception.reviewer,
            exception.expires_at,
        ));
    }
    out
}

/// Render the canonical on-disk feed after `feed.signature` has been filled.
/// The signature is part of the signed feed header, but not part of the bytes
/// passed to `sign_advisory_feed`.
pub fn advisory_feed_text(feed: &AdvisoryFeed) -> String {
    let payload = advisory_feed_payload(feed);
    let mut parts = payload.splitn(3, '\n');
    let magic = parts.next().unwrap_or(ADVISORY_FEED_MAGIC);
    let header = parts.next().unwrap_or_default();
    let records = parts.next().unwrap_or_default();
    format!("{magic}\n{header}|{}\n{records}", feed.signature)
}

pub fn sign_advisory_feed(feed: &AdvisoryFeed, seed: &Path) -> Result<String, Diagnostic> {
    let digest = crate::SHA256::sha256_hex(advisory_feed_payload(feed).as_bytes());
    super::Sign::sign(seed, &digest)
}

fn advisory_feed_digest(feed: &AdvisoryFeed) -> String {
    format!(
        "sha256-{}",
        crate::SHA256::sha256_hex(advisory_feed_text(feed).as_bytes())
    )
}

pub fn verify_advisory_feed(
    feed: &AdvisoryFeed,
    trust: &AdvisoryTrustRoot,
    now: u64,
) -> Result<AdvisoryReceipt, Diagnostic> {
    validate_feed_identity(feed)?;
    if trust.public_key.trim().is_empty() || feed.public_key != trust.public_key.trim() {
        return Err(e2610(
            "advisory feed",
            "the feed key is not pinned by the local trust root",
        ));
    }
    if trust.revoked_keys.contains(feed.public_key.trim()) {
        return Err(e2610(
            "advisory feed",
            "the feed key is revoked by local policy",
        ));
    }
    if feed.sequence < trust.min_sequence {
        return Err(e2610(
            "advisory feed",
            "the feed sequence rolled back below the locally accepted sequence",
        ));
    }
    let digest = advisory_feed_digest(feed);
    if feed.sequence == trust.min_sequence
        && trust
            .accepted_digest
            .as_deref()
            .is_some_and(|accepted| accepted != digest)
    {
        return Err(e2610(
            "advisory feed",
            "the feed forked at an already accepted sequence",
        ));
    }
    if feed.issued_at > now.saturating_add(300) {
        return Err(e2610("advisory feed", "the feed is issued in the future"));
    }
    if feed.expires_at <= now {
        return Err(e2610(
            "advisory feed",
            "the signed advisory metadata is stale or expired",
        ));
    }
    let signed_digest = crate::SHA256::sha256_hex(advisory_feed_payload(feed).as_bytes());
    if !super::Sign::verify(&feed.public_key, &signed_digest, &feed.signature)? {
        return Err(e2610("advisory feed", "the feed signature does not verify"));
    }
    Ok(AdvisoryReceipt {
        sequence: feed.sequence,
        digest,
        key_id: feed.key_id.clone(),
        issued_at: feed.issued_at,
        expires_at: feed.expires_at,
        maturity_seconds: if feed.maturity_seconds == 0 {
            DEFAULT_MATURITY_SECONDS
        } else {
            feed.maturity_seconds
        },
    })
}

pub fn parse_advisory_trust(text: &str) -> Result<AdvisoryTrustRoot, Diagnostic> {
    let mut root = AdvisoryTrustRoot::default();
    let mut seen = BTreeSet::new();
    for (line_number, raw) in text.lines().enumerate() {
        let line_number = line_number + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(e2607(
                "advisory trust root",
                &format!("line {line_number} is not key=value"),
            ));
        };
        let key = key.trim();
        if key != "revoked_key" && !seen.insert(key.to_string()) {
            return Err(e2607(
                "advisory trust root",
                &format!("line {line_number} repeats field {key}"),
            ));
        }
        match key {
            "public_key" if root.public_key.is_empty() => {
                root.public_key = value.trim().to_string()
            }
            "min_sequence" => {
                root.min_sequence = value.trim().parse().map_err(|_| {
                    e2607(
                        "advisory trust root",
                        &format!("line {line_number} has an invalid minimum sequence"),
                    )
                })?;
            }
            "accepted_digest" if root.accepted_digest.is_none() => {
                root.accepted_digest = Some(value.trim().to_string())
            }
            "revoked_key" => {
                let revoked = value.trim();
                if revoked.is_empty() {
                    return Err(e2607(
                        "advisory trust root",
                        &format!("line {line_number} has an empty revoked key"),
                    ));
                }
                root.revoked_keys.insert(revoked.to_string());
            }
            _ => {
                return Err(e2607(
                    "advisory trust root",
                    &format!("line {line_number} has an unknown or repeated field"),
                ));
            }
        }
    }
    if root.public_key.is_empty() {
        return Err(e2610(
            "advisory trust root",
            "no publisher public key is pinned",
        ));
    }
    if root.accepted_digest.is_some() && root.min_sequence == 0 {
        return Err(e2610(
            "advisory trust root",
            "an accepted digest needs its monotonic minimum sequence",
        ));
    }
    Ok(root)
}

pub fn audit_advisory_feed(
    lock: &LockFile,
    feed: &AdvisoryFeed,
    trust: &AdvisoryTrustRoot,
    now: u64,
) -> Result<PolicyAudit, Diagnostic> {
    let receipt = verify_advisory_feed(feed, trust, now)?;
    if let Some(requirement) = lock
        .authority
        .as_ref()
        .and_then(|authority| authority.trust.as_ref())
        .and_then(|trust| trust.require)
    {
        crate::Lock::enforce_provenance_requirement(lock, requirement)
            .map_err(|error| e2610("lock provenance", &error))?;
    }
    let mut maturity = Vec::new();
    for package in &lock.packages {
        let Some(version) = SemVer::parse(&package.version) else {
            continue;
        };
        let Some(release) = feed
            .releases
            .iter()
            .find(|release| release.package == package.name && release.version == version)
        else {
            if !matches!(&package.source, LockSource::Root | LockSource::Path(_)) {
                maturity.push(e2610(
                    "advisory feed",
                    &format!(
                        "no trusted release record exists for {}#{}",
                        package.name, package.version
                    ),
                ));
            }
            continue;
        };
        let required = match release.source_class {
            SourceClass::ThirdParty => receipt.maturity_seconds,
            SourceClass::FirstParty | SourceClass::Workspace => 0,
        };
        let mature_at = release.first_seen.saturating_add(required);
        if now < mature_at
            && !feed.exceptions.iter().any(|exception| {
                exception.package == package.name
                    && exception.version == version
                    && exception.expires_at > now
            })
        {
            maturity.push(e2609(
                &package.name,
                &package.version,
                mature_at,
                release.source_class,
            ));
        }
    }
    Ok(PolicyAudit {
        receipt,
        matches: audit_lockfile(lock, &feed.advisories),
        maturity,
    })
}

fn current_unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub fn advisory_now() -> u64 {
    std::env::var("JET_ADVISORY_NOW")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(current_unix_time)
}

pub fn e2609(
    package: &str,
    version: &str,
    mature_at: u64,
    source_class: SourceClass,
) -> Diagnostic {
    Diagnostic::error(
        "E2609",
        format!("{package}#{version} is not mature under the advisory policy"),
        format!("the signed feed classifies this {} release as new until Unix time {mature_at}", source_class.label()),
        format!("wait for the maturity window, or add a reviewed exact exception for {package}#{version} with a reason, reviewer, and expiry"),
        None,
    )
}

pub fn e2610(source: &str, detail: &str) -> Diagnostic {
    Diagnostic::error(
        "E2610",
        format!("{source} was rejected: {detail}"),
        "advisory policy is security-sensitive; Jet will not use unsigned, stale, rolled-back, forked, or downgraded evidence.".to_string(),
        "refresh the signed offline feed and trust root, or repair the lock provenance before retrying.".to_string(),
        None,
    )
}

pub fn e2611(input: &str, fix: &str) -> Diagnostic {
    Diagnostic::error(
        "E2611",
        format!("jet inspect audit needs {input}"),
        "an audit without its lock or advisory database could report a false clean result.".to_string(),
        fix.to_string(),
        None,
    )
}

/// E2607 — a supply-chain metadata parser rejected malformed input.
pub fn e2607(source: &str, detail: &str) -> Diagnostic {
    Diagnostic::error(
        "E2607",
        format!("{source} is malformed: {detail}"),
        "supply-chain metadata is security-sensitive, so Jet rejects ambiguous or partial records instead of silently skipping them."
            .to_string(),
        format!(
            "fix the malformed {source} record and retry; use the documented parser contract and UTF-8 text."
        ),
        None,
    )
}

/// One advisory that matched a locked package, paired with its severity so the
/// caller can decide the exit code (`jet inspect audit` exits nonzero on CRITICAL).
#[derive(Debug, Clone)]
pub struct AuditMatch {
    pub severity: Severity,
    pub diagnostic: Diagnostic,
}

/// Check a set of locked packages against verified advisory records.
/// Returns one match (severity + E2603) per advisory that applies.
pub fn audit_lockfile(lock: &LockFile, advisories: &[Advisory]) -> Vec<AuditMatch> {
    let mut matches = Vec::new();
    for pkg in &lock.packages {
        let ver = match SemVer::parse(&pkg.version) {
            Some(v) => v,
            None => continue,
        };
        for adv in advisories {
            if adv.package == pkg.name && adv.affects(&ver) {
                matches.push(AuditMatch {
                    severity: adv.severity,
                    diagnostic: e2603(
                        &adv.id,
                        &pkg.name,
                        &pkg.version,
                        &adv.title,
                        adv.severity,
                        adv.fixed.as_ref(),
                    ),
                });
            }
        }
    }
    matches
}

/// E2603 — advisory match.
pub fn e2603(
    id: &str,
    package: &str,
    version: &str,
    title: &str,
    severity: Severity,
    fixed: Option<&SemVer>,
) -> Diagnostic {
    let fix_msg = match fixed {
        Some(v) => format!("upgrade `{}` to >= {}. Run `jet inspect audit --explain {}` for details.", package, v, id),
        None => format!("no fixed version is known; monitor `{}` for a patch. Run `jet inspect audit --explain {}` for details.", package, id),
    };
    Diagnostic::error(
        "E2603",
        format!("[{}] advisory {} matches `{}` {}: {}", severity.label(), id, package, version, title),
        format!(
            "the advisory database flags `{}` {} as having a known vulnerability, exposed interface, or supply-chain risk.",
            package, version
        ),
        fix_msg,
        None,
    )
}

// ──────────────────────────────────────────────
// Integrity verification → E2604
// ──────────────────────────────────────────────

/// E2604 — integrity check failed.
pub fn e2604(package: &str, version: &str, expected: &str, actual: &str) -> Diagnostic {
    Diagnostic::error(
        "E2604",
        format!("integrity check failed for `{}` {}", package, version),
        format!(
            "expected hash {}, got {}. The artifact changed after it was locked — this may indicate accidental or deliberate tampering.",
            expected, actual
        ),
        format!(
            "re-run `jet store fetch` after cleaning stale Jetpack hangar entries (`jet clean`). If the problem persists, the upstream source may have been altered; audit the change before proceeding."
        ),
        None,
    )
}

/// Verify a locked package's store entry against its recorded hash.
pub fn verify_package_integrity(pkg: &LockedPackage, store_entry: &Path) -> Result<(), Diagnostic> {
    use crate::SHA256::tree_hash;
    let actual = tree_hash(store_entry);
    if actual != pkg.fingerprint {
        return Err(e2604(&pkg.name, &pkg.version, &pkg.fingerprint, &actual));
    }
    Ok(())
}
