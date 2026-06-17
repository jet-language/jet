//! E2-M8 — packages and enterprise supply chain.
//!
//! Owns:
//!   - SemVer parsing and comparison (no external crates, I6).
//!   - Public API extraction from parsed Jet AST items.
//!   - API diff → E2601 (breaking change under non-breaking version bump).
//!   - PubGrub-style conflict detection → E2602.
//!   - Advisory database format + check → E2603.
//!   - Artifact integrity verification → E2604.
//!   - SBOM emission (SPDX 2.3 tag-value format from a lockfile).
//!   - `jet vendor` (copy resolved deps into a `vendor/` tree).
//!   - Private / mirror registry configuration.

use crate::diag::Diagnostic;
use crate::lock::{LockFile, LockedPackage, LockSource};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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

// ──────────────────────────────────────────────
// Public API surface extraction
// ──────────────────────────────────────────────

/// An item in a package's public API. Two `ApiItem`s are "compatible" when they
/// have the same `kind`, `name`, and `signature` (a textual canonical form).
/// We store the signature as a string because full AST comparison is brittle;
/// the canonical form gives false-negative safety (we might miss a breaking
/// change in a complex generic; that is acceptable for v1).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ApiItem {
    /// "fn", "struct", "enum", "trait", "const"
    pub kind: String,
    pub name: String,
    /// Textual canonical form of the signature (param/field types, return type).
    /// Does not include the body. Whitespace-normalised.
    pub signature: String,
}

/// Extract the public API surface from a parsed Jet source file.
/// Only `pub` items at the top level are included.
pub fn extract_public_api(src: &str, file: &str) -> Vec<ApiItem> {
    use crate::loader;

    let bundle = match loader::load_entry_with_overlay(file, None, true) {
        Ok(b) => b,
        Err(_) => return vec![],
    };
    let _ = src; // bundle already loaded

    let mut out = Vec::new();
    // Entry file items (the main module).
    let entry = &bundle.modules[bundle.entry];
    for item in &entry.items {
        if let Some(api) = public_api_of_item(item) {
            out.push(api);
        }
    }
    out.sort();
    out
}

/// Build an `ApiItem` for a single AST item, or `None` if it is private.
fn public_api_of_item(item: &crate::ast::Item) -> Option<ApiItem> {
    use crate::ast::Item;
    match item {
        Item::Func(f) if f.is_pub => Some(ApiItem {
            kind: "fn".into(),
            name: f.name.clone(),
            signature: format_fn_sig(f),
        }),
        Item::Struct(s) if s.is_pub => Some(ApiItem {
            kind: "struct".into(),
            name: s.name.clone(),
            signature: format_struct_sig(s),
        }),
        Item::Enum(e) if e.is_pub => Some(ApiItem {
            kind: "enum".into(),
            name: e.name.clone(),
            signature: format_enum_sig(e),
        }),
        Item::Trait(t) if t.is_pub => Some(ApiItem {
            kind: "trait".into(),
            name: t.name.clone(),
            signature: format_trait_sig(t),
        }),
        // ConstDef does not carry is_pub in v1 — consts are accessible by name
        // and the pub distinction is enforced at use sites by sema. Skip from
        // public API for now; revisit when const visibility is added to the AST.
        Item::Const(_c) => None,
        _ => None,
    }
}

fn format_type(ty: &crate::ast::Type) -> String {
    ty.show()
}

fn format_fn_sig(f: &crate::ast::Func) -> String {
    use crate::ast::AccessConvention;
    let params: Vec<String> = f
        .params
        .iter()
        .map(|p| {
            let prefix = match p.convention {
                AccessConvention::Read => "",
                AccessConvention::Mutate => "mut ",
                AccessConvention::Move => "take ",
            };
            format!("{}{}: {}", prefix, p.name, format_type(&p.ty))
        })
        .collect();
    let ret = match &f.return_type {
        Some(t) => format!(" -> {}", format_type(t)),
        None => String::new(),
    };
    format!("fn {}({}){}", f.name, params.join(", "), ret)
}

fn format_struct_sig(s: &crate::ast::StructDef) -> String {
    let fields: Vec<String> = s
        .fields
        .iter()
        .map(|f| format!("{}: {}", f.name, format_type(&f.ty)))
        .collect();
    format!("struct {} {{ {} }}", s.name, fields.join("; "))
}

fn format_enum_sig(e: &crate::ast::EnumDef) -> String {
    let variants: Vec<String> = e
        .variants
        .iter()
        .map(|v| v.name.clone())
        .collect();
    format!("enum {} {{ {} }}", e.name, variants.join(", "))
}

fn format_trait_sig(t: &crate::ast::TraitDef) -> String {
    let methods: Vec<String> = t
        .methods
        .iter()
        .map(|m| m.name.clone())
        .collect();
    format!("trait {} {{ {} }}", t.name, methods.join(", "))
}

// ──────────────────────────────────────────────
// API diff → E2601
// ──────────────────────────────────────────────

/// Compare old and new public API surfaces and return a list of breaking
/// changes. A change is breaking when an item is removed or its signature
/// changes (any method removed from a trait, any field removed from a pub
/// struct, any function's parameter list or return type changed).
#[derive(Debug, Clone)]
pub struct BreakingChange {
    /// Human-readable description of the broken item.
    pub description: String,
    /// The item name for the diagnostic span label.
    pub item_name: String,
}

pub fn diff_public_api(old: &[ApiItem], new: &[ApiItem]) -> Vec<BreakingChange> {
    let old_set: BTreeMap<(&str, &str), &ApiItem> = old
        .iter()
        .map(|i| ((i.kind.as_str(), i.name.as_str()), i))
        .collect();
    let new_set: BTreeMap<(&str, &str), &ApiItem> = new
        .iter()
        .map(|i| ((i.kind.as_str(), i.name.as_str()), i))
        .collect();

    let mut changes = Vec::new();

    // Removed items.
    for ((kind, name), old_item) in &old_set {
        if !new_set.contains_key(&(*kind, *name)) {
            changes.push(BreakingChange {
                description: format!(
                    "pub {} `{}` was removed\n   | {}\n   | (removed)",
                    kind, name, old_item.signature
                ),
                item_name: name.to_string(),
            });
        }
    }

    // Changed signatures.
    for ((kind, name), old_item) in &old_set {
        if let Some(new_item) = new_set.get(&(*kind, *name)) {
            if old_item.signature != new_item.signature {
                changes.push(BreakingChange {
                    description: format!(
                        "pub {} `{}` changed signature\n   | was: {}\n   | now: {}",
                        kind, name, old_item.signature, new_item.signature
                    ),
                    item_name: name.to_string(),
                });
            }
        }
    }

    changes
}

/// E2601 — publishing would break SemVer.
pub fn e2601(
    version: &str,
    bump_kind: BumpKind,
    change: &BreakingChange,
    next_major: u64,
) -> Diagnostic {
    let bump_str = match bump_kind {
        BumpKind::Minor => "minor",
        BumpKind::Patch => "patch",
        _ => "non-breaking",
    };
    Diagnostic::error(
        "E2601",
        format!(
            "this release is tagged {} but removes public API",
            version
        ),
        format!(
            "{} is a {} bump, which promises no breaking changes. Callers pinned to ^{}.0 would stop compiling.\n  {}",
            version,
            bump_str,
            // Extract the major from version
            version.split('.').next().unwrap_or("?"),
            change.description,
        ),
        format!(
            "bump to {}.0.0, or restore `{}` (a deprecated shim counts). Use `jet publish --force` to override with a warning banner.",
            next_major,
            change.item_name,
        ),
        None,
    )
}

// ──────────────────────────────────────────────
// Registry resolver (PubGrub-style conflict detection)
// ──────────────────────────────────────────────

/// A single version constraint from one dependent.
#[derive(Debug, Clone)]
pub struct VersionConstraint {
    pub package: String,
    pub req: VersionReq,
    /// Where this constraint comes from (package name and version).
    pub from: String,
}

/// E2602 — resolver cannot satisfy two conflicting constraints.
pub fn e2602(
    package: &str,
    req_a: &str,
    from_a: &str,
    req_b: &str,
    from_b: &str,
) -> Diagnostic {
    Diagnostic::error(
        "E2602",
        format!("dependency resolver conflict: `{}` has incompatible version requirements", package),
        format!(
            "`{}` requires `{}` from `{}`, but `{}` from `{}`; no version satisfies both.",
            package, req_a, from_a, req_b, from_b,
        ),
        format!(
            "upgrade or downgrade one of the conflicting dependents so their `{}` constraints overlap, or ask the authors to release a compatible version.",
            package,
        ),
        None,
    )
}

/// A simplified resolver: given a set of constraints for each package name,
/// check whether any package has two mutually-incompatible constraints
/// (i.e. no candidate version in the registry satisfies all of them).
/// In v1 without a live registry, we detect the syntactic contradiction
/// (e.g. `^1.0` vs `^2.0`).
pub fn check_conflicts(
    constraints: &[VersionConstraint],
    available: &BTreeMap<String, Vec<SemVer>>,
) -> Vec<Diagnostic> {
    // Group constraints by package.
    let mut by_pkg: BTreeMap<&str, Vec<&VersionConstraint>> = BTreeMap::new();
    for c in constraints {
        by_pkg.entry(c.package.as_str()).or_default().push(c);
    }

    let mut diags = Vec::new();
    for (pkg, reqs) in &by_pkg {
        if reqs.len() < 2 {
            continue;
        }
        // Find any version that satisfies ALL constraints.
        let candidates = available.get(*pkg).map(|v| v.as_slice()).unwrap_or(&[]);
        let any_ok = candidates
            .iter()
            .any(|v| reqs.iter().all(|r| r.req.matches(v)));
        if !any_ok && !candidates.is_empty() {
            // Report first two conflicting.
            let a = reqs[0];
            let b = reqs.iter().skip(1).find(|r| {
                !candidates
                    .iter()
                    .any(|v| r.req.matches(v) && a.req.matches(v))
            });
            if let Some(b) = b {
                let req_a_str = format!("{:?}", a.req).replace("VersionReq::", "");
                let req_b_str = format!("{:?}", b.req).replace("VersionReq::", "");
                diags.push(e2602(pkg, &req_a_str, &a.from, &req_b_str, &b.from));
            }
        }
        // When no candidates at all: surface if two constraints' ranges exclude each other.
        if candidates.is_empty() && reqs.len() >= 2 {
            for i in 0..reqs.len() {
                for j in (i + 1)..reqs.len() {
                    if ranges_disjoint(&reqs[i].req, &reqs[j].req) {
                        let req_a_str = req_display(&reqs[i].req);
                        let req_b_str = req_display(&reqs[j].req);
                        diags.push(e2602(pkg, &req_a_str, &reqs[i].from, &req_b_str, &reqs[j].from));
                    }
                }
            }
        }
    }
    diags
}

fn req_display(r: &VersionReq) -> String {
    match r {
        VersionReq::Any => "*".into(),
        VersionReq::Exact(v) => v.to_string(),
        VersionReq::Caret { floor, .. } => format!("^{}", floor),
    }
}

fn ranges_disjoint(a: &VersionReq, b: &VersionReq) -> bool {
    match (a, b) {
        (VersionReq::Caret { floor: fa, precision: pa }, VersionReq::Caret { floor: fb, precision: pb }) => {
            // ^1.x and ^2.x are disjoint (different majors, or major=0 different minors).
            if fa.major == 0 && fb.major == 0 && *pa >= 2 && *pb >= 2 {
                fa.minor != fb.minor
            } else {
                fa.major != fb.major
            }
        }
        (VersionReq::Exact(va), VersionReq::Exact(vb)) => va != vb,
        (VersionReq::Exact(v), VersionReq::Caret { floor, precision }) => {
            !VersionReq::Caret { floor: floor.clone(), precision: *precision }.matches(v)
        }
        (VersionReq::Caret { floor, precision }, VersionReq::Exact(v)) => {
            !VersionReq::Caret { floor: floor.clone(), precision: *precision }.matches(v)
        }
        _ => false,
    }
}

// ──────────────────────────────────────────────
// Advisory database
// ──────────────────────────────────────────────

/// One advisory entry. In v1 the database is a list of these structs, loaded
/// from a plain text format (one JSON-like record per advisory).
#[derive(Debug, Clone)]
pub struct Advisory {
    /// Unique identifier, e.g. `JET-2026-0001` or a CVE ID.
    pub id: String,
    pub package: String,
    /// Version range where the vulnerability is present.
    pub affected: VersionReq,
    /// First version where the fix is available, if known.
    pub fixed: Option<SemVer>,
    pub title: String,
}

impl Advisory {
    /// Does `version` fall within the affected range?
    pub fn affects(&self, version: &SemVer) -> bool {
        self.affected.matches(version)
            && self.fixed.as_ref().map(|f| version < f).unwrap_or(true)
    }
}

/// Parse advisories from the line-based format:
/// `id|package|affected_req|fixed_version_or_empty|title`
pub fn parse_advisory_db(text: &str) -> Vec<Advisory> {
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(5, '|').collect();
            if parts.len() < 5 {
                return None;
            }
            let id = parts[0].trim().to_string();
            let package = parts[1].trim().to_string();
            let affected = VersionReq::parse(parts[2].trim())?;
            let fixed = if parts[3].trim().is_empty() {
                None
            } else {
                SemVer::parse(parts[3].trim())
            };
            let title = parts[4].trim().to_string();
            Some(Advisory { id, package, affected, fixed, title })
        })
        .collect()
}

/// Check a set of locked packages against the advisory database.
/// Returns one E2603 per match.
pub fn audit_lockfile(
    lock: &LockFile,
    advisories: &[Advisory],
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for pkg in &lock.packages {
        let ver = match SemVer::parse(&pkg.version) {
            Some(v) => v,
            None => continue,
        };
        for adv in advisories {
            if adv.package == pkg.name && adv.affects(&ver) {
                diags.push(e2603(&adv.id, &pkg.name, &pkg.version, &adv.title, adv.fixed.as_ref()));
            }
        }
    }
    diags
}

/// E2603 — advisory match.
pub fn e2603(id: &str, package: &str, version: &str, title: &str, fixed: Option<&SemVer>) -> Diagnostic {
    let fix_msg = match fixed {
        Some(v) => format!("upgrade `{}` to >= {}. Run `jet audit --explain {}` for details.", package, v, id),
        None => format!("no fixed version is known; monitor `{}` for a patch. Run `jet audit --explain {}` for details.", package, id),
    };
    Diagnostic::error(
        "E2603",
        format!("advisory {} matches `{}` {}: {}", id, package, version, title),
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
            "re-run `jet fetch` after removing the corrupt store entry (`jet gc --force`). If the problem persists, the upstream source may have been altered; audit the change before proceeding."
        ),
        None,
    )
}

/// Verify a locked package's store entry against its recorded hash.
pub fn verify_package_integrity(
    pkg: &LockedPackage,
    store_entry: &Path,
) -> Result<(), Diagnostic> {
    use crate::sha256::tree_hash;
    let actual = tree_hash(store_entry);
    if actual != pkg.fingerprint {
        return Err(e2604(&pkg.name, &pkg.version, &pkg.fingerprint, &actual));
    }
    Ok(())
}

// ──────────────────────────────────────────────
// SBOM generation (SPDX 2.3 tag-value)
// ──────────────────────────────────────────────

/// Generate an SPDX 2.3 tag-value SBOM from a lockfile.
///
/// Format: https://spdx.github.io/spdx-spec/v2.3/ (tag-value subset)
/// We emit the mandatory fields plus packages. The document namespace
/// is `https://jet-lang.org/spdx/<root-package>-<timestamp>`.
pub fn emit_spdx(lock: &LockFile, root_name: &str, root_version: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let mut out = String::new();

    // Document creation information.
    out.push_str("SPDXVersion: SPDX-2.3\n");
    out.push_str("DataLicense: CC0-1.0\n");
    out.push_str(&format!(
        "SPDXID: SPDXRef-DOCUMENT\n"
    ));
    out.push_str(&format!(
        "DocumentNamespace: https://jet-lang.org/spdx/{}-{}-{}\n",
        root_name, root_version, ts
    ));
    out.push_str(&format!(
        "DocumentName: {}-{}\n",
        root_name, root_version
    ));
    out.push_str("Creator: Tool: jet\n");
    out.push_str(&format!("Created: {}\n", spdx_timestamp(ts)));
    out.push_str("\n");

    // Root package.
    out.push_str("##### Root package\n\n");
    out.push_str(&format!("PackageName: {}\n", root_name));
    out.push_str("SPDXID: SPDXRef-root\n");
    out.push_str(&format!("PackageVersion: {}\n", root_version));
    out.push_str("FilesAnalyzed: false\n");
    out.push_str("PackageChecksum: NOASSERTION\n");
    out.push_str("PackageDownloadLocation: NOASSERTION\n");
    out.push_str("\n");

    // One package block per locked dependency.
    for (i, pkg) in lock.packages.iter().enumerate() {
        let spdx_id = format!("SPDXRef-pkg-{}", i);
        out.push_str(&format!("##### {}\n\n", pkg.name));
        out.push_str(&format!("PackageName: {}\n", pkg.name));
        out.push_str(&format!("SPDXID: {}\n", spdx_id));
        out.push_str(&format!("PackageVersion: {}\n", pkg.version));
        out.push_str("FilesAnalyzed: false\n");
        // The fingerprint is sha256-<hex>; SPDX uses SHA256: <hex>.
        let checksum = pkg
            .fingerprint
            .strip_prefix("sha256-")
            .map(|h| format!("SHA256: {}", h))
            .unwrap_or_else(|| "NOASSERTION".to_string());
        out.push_str(&format!("PackageChecksum: {}\n", checksum));
        out.push_str("PackageDownloadLocation: NOASSERTION\n");
        out.push_str("\n");

        // DESCRIBES relationship from root.
        out.push_str(&format!(
            "Relationship: SPDXRef-root DEPENDS_ON {}\n\n",
            spdx_id
        ));
    }

    out
}

/// Generate a CycloneDX 1.5 JSON SBOM from a lockfile.
pub fn emit_cyclonedx(lock: &LockFile, root_name: &str, root_version: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let mut components = Vec::new();
    for (i, pkg) in lock.packages.iter().enumerate() {
        let hash_val = pkg
            .fingerprint
            .strip_prefix("sha256-")
            .unwrap_or(&pkg.fingerprint);
        components.push(format!(
            r#"    {{
      "type": "library",
      "bom-ref": "pkg-{i}",
      "name": "{name}",
      "version": "{version}",
      "hashes": [{{ "alg": "SHA-256", "content": "{hash}" }}]
    }}"#,
            i = i,
            name = json_escape(&pkg.name),
            version = json_escape(&pkg.version),
            hash = json_escape(hash_val),
        ));
    }

    format!(
        r#"{{
  "bomFormat": "CycloneDX",
  "specVersion": "1.5",
  "serialNumber": "urn:uuid:jet-{ts}",
  "version": 1,
  "metadata": {{
    "timestamp": "{timestamp}",
    "tools": [{{ "name": "jet" }}],
    "component": {{
      "type": "library",
      "name": "{root_name}",
      "version": "{root_version}"
    }}
  }},
  "components": [
{components}
  ]
}}
"#,
        ts = ts,
        timestamp = iso8601(ts),
        root_name = json_escape(root_name),
        root_version = json_escape(root_version),
        components = components.join(",\n"),
    )
}

fn spdx_timestamp(secs: u64) -> String {
    // Simple ISO8601: 2026-01-01T00:00:00Z (we don't have chrono — I6)
    iso8601(secs)
}

fn iso8601(secs: u64) -> String {
    // Minimal ISO 8601 without chrono. We compute date parts from the epoch.
    // Accurate for years 1970–2100 (Gregorian, no leap seconds).
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let mut days = secs / 86400;

    let mut year = 1970u64;
    loop {
        let y_days = if is_leap(year) { 366 } else { 365 };
        if days < y_days {
            break;
        }
        days -= y_days;
        year += 1;
    }
    let months = if is_leap(year) {
        [31u64, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31u64, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 1u64;
    for &md in &months {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }
    let day = days + 1;
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, h, m, s
    )
}

fn is_leap(year: u64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

// ──────────────────────────────────────────────
// `jet vendor` — copy resolved deps into vendor/
// ──────────────────────────────────────────────

/// Copy all resolved dependency store entries into `<project_root>/vendor/<name>/`.
/// After vendoring, `--locked` builds can run offline by reading from vendor/.
pub fn vendor(
    project_root: &Path,
    lock: &LockFile,
    dep_dirs: &std::collections::HashMap<String, PathBuf>,
) -> Result<Vec<String>, Diagnostic> {
    let vendor_dir = project_root.join("vendor");
    std::fs::create_dir_all(&vendor_dir).map_err(|e| Diagnostic::error(
        "E2604",
        format!("couldn't create vendor/ directory: {}", e),
        "vendor/ is where `jet vendor` writes offline copies of dependencies.".into(),
        "check write permissions on the project directory.".into(),
        None,
    ))?;

    let mut copied = Vec::new();
    for (name, src_dir) in dep_dirs {
        let dest = vendor_dir.join(name);
        if dest.exists() {
            // Remove stale copy.
            std::fs::remove_dir_all(&dest).ok();
        }
        copy_dir_recursive(src_dir, &dest).map_err(|e| Diagnostic::error(
            "E2604",
            format!("failed to vendor `{}`: {}", name, e),
            "jet vendor copies dependency source into vendor/ for offline builds.".into(),
            "check that the dependency is correctly fetched first with `jet fetch`.".into(),
            None,
        ))?;
        copied.push(name.clone());
    }
    let _ = lock; // lock used for hash verification in a future pass
    Ok(copied)
}

/// Recursively copy a directory tree.
fn copy_dir_recursive(src: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else {
            std::fs::copy(&src_path, &dest_path)?;
        }
    }
    Ok(())
}

// ──────────────────────────────────────────────
// Private / mirror registry configuration
// ──────────────────────────────────────────────

/// Registry endpoint configuration. Mirrors proxy the public registry;
/// private registries host organisation-internal packages.
/// D-PKGS1: no hard-coded public infrastructure — registries are configured.
#[derive(Debug, Clone)]
pub struct RegistryConfig {
    /// Registry name (used in `deps: { pkg: { registry: "private" } }` etc.)
    pub name: String,
    /// Base URL for the git registry index.
    pub url: String,
    /// If true, this registry mirrors the public one (proxies unknown packages).
    pub mirror: bool,
    /// If true, require signed metadata from this registry.
    pub require_signed: bool,
}

impl RegistryConfig {
    /// Build the default (well-known) public registry config.
    /// In v1, the URL is advisory — the registry ops are deferred (M8 out of scope).
    pub fn public_default() -> Self {
        Self {
            name: "jet".to_string(),
            url: "https://github.com/jet-lang/registry".to_string(),
            mirror: false,
            require_signed: false,
        }
    }

    /// Build a private mirror config.
    pub fn private(name: &str, url: &str, mirror: bool) -> Self {
        Self {
            name: name.to_string(),
            url: url.to_string(),
            mirror,
            require_signed: false,
        }
    }
}

/// Parse registry configs from a simple block in the manifest extra fields.
/// Format: `registries: { name: { url: "...", mirror: true } }`
/// This is the placeholder for the future registry section in payload.jet.
pub fn parse_registries_from_env(env: &std::collections::HashMap<String, String>) -> Vec<RegistryConfig> {
    // Look for JET_REGISTRY_<NAME>_URL env vars.
    let mut result = Vec::new();
    for (key, url) in env {
        if let Some(suffix) = key.strip_prefix("JET_REGISTRY_") {
            if let Some(name) = suffix.strip_suffix("_URL") {
                let name_lc = name.to_lowercase();
                let mirror_key = format!("JET_REGISTRY_{}_MIRROR", name);
                let mirror = env.get(&mirror_key).map(|v| v == "true").unwrap_or(false);
                result.push(RegistryConfig::private(&name_lc, url, mirror));
            }
        }
    }
    if result.is_empty() {
        result.push(RegistryConfig::public_default());
    }
    result
}

// ──────────────────────────────────────────────
// Pre-publish gate (D-PKGS4 amended)
// ──────────────────────────────────────────────

/// Pre-publish gate outcome.
#[derive(Debug)]
pub struct PrePublishGate {
    pub build_ok: bool,
    pub tests_ok: bool,
    /// API breaking changes found (E2601 candidates).
    pub breaking: Vec<BreakingChange>,
    pub version: String,
    pub bump_kind: BumpKind,
    pub next_major: u64,
}

impl PrePublishGate {
    /// `true` when the publish should be blocked (failing gate, or breaking change
    /// under a non-breaking bump).
    pub fn is_blocked(&self) -> bool {
        !self.build_ok
            || !self.tests_ok
            || (!self.breaking.is_empty()
                && matches!(self.bump_kind, BumpKind::Minor | BumpKind::Patch | BumpKind::Same))
    }

    /// Build E2601 diagnostics for every breaking change.
    pub fn semver_errors(&self) -> Vec<Diagnostic> {
        if matches!(self.bump_kind, BumpKind::Major) {
            return vec![];
        }
        self.breaking
            .iter()
            .map(|c| e2601(&self.version, self.bump_kind, c, self.next_major))
            .collect()
    }
}

// ──────────────────────────────────────────────
// Unit tests
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::{LockFile, LockedPackage, LockSource};

    fn sv(s: &str) -> SemVer {
        SemVer::parse(s).expect(s)
    }

    #[test]
    fn semver_parse_basic() {
        let v = sv("1.2.3");
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 2);
        assert_eq!(v.patch, 3);
    }

    #[test]
    fn semver_parse_with_prefix_v() {
        let v = sv("v2.0.0");
        assert_eq!(v.major, 2);
    }

    #[test]
    fn semver_parse_with_pre() {
        let v = sv("1.0.0-alpha.1");
        assert_eq!(v.major, 1);
        assert_eq!(v.pre.as_deref(), Some("alpha.1"));
    }

    #[test]
    fn semver_ordering() {
        assert!(sv("2.0.0") > sv("1.9.9"));
        assert!(sv("1.1.0") > sv("1.0.9"));
        assert_eq!(sv("1.2.3"), sv("1.2.3"));
    }

    #[test]
    fn semver_compatible() {
        let v100 = sv("1.0.0");
        let v120 = sv("1.2.0");
        assert!(v120.is_compatible_with(&v100));
        assert!(!v100.is_compatible_with(&v120)); // not >=
    }

    #[test]
    fn classify_bump_kinds() {
        assert_eq!(classify_bump(&sv("1.0.0"), &sv("2.0.0")), BumpKind::Major);
        assert_eq!(classify_bump(&sv("1.0.0"), &sv("1.1.0")), BumpKind::Minor);
        assert_eq!(classify_bump(&sv("1.0.0"), &sv("1.0.1")), BumpKind::Patch);
        assert_eq!(classify_bump(&sv("1.0.0"), &sv("1.0.0")), BumpKind::Same);
    }

    #[test]
    fn version_req_caret() {
        let req = VersionReq::parse("^1.2").unwrap();
        assert!(req.matches(&sv("1.2.0")));
        assert!(req.matches(&sv("1.5.3")));
        assert!(!req.matches(&sv("2.0.0")));
        assert!(!req.matches(&sv("1.1.9")));
    }

    #[test]
    fn version_req_exact() {
        let req = VersionReq::parse("1.2.3").unwrap();
        assert!(req.matches(&sv("1.2.3")));
        assert!(!req.matches(&sv("1.2.4")));
    }

    #[test]
    fn version_req_any() {
        let req = VersionReq::parse("*").unwrap();
        assert!(req.matches(&sv("99.99.99")));
    }

    #[test]
    fn conflict_detection_disjoint_majors() {
        let constraints = vec![
            VersionConstraint {
                package: "foo".into(),
                req: VersionReq::parse("^1.0").unwrap(),
                from: "bar 0.1.0".into(),
            },
            VersionConstraint {
                package: "foo".into(),
                req: VersionReq::parse("^2.0").unwrap(),
                from: "baz 0.1.0".into(),
            },
        ];
        let diags = check_conflicts(&constraints, &BTreeMap::new());
        assert!(!diags.is_empty(), "disjoint caret ranges should be a conflict");
        assert_eq!(diags[0].code, "E2602");
    }

    #[test]
    fn conflict_compatible_ranges_no_conflict() {
        let constraints = vec![
            VersionConstraint {
                package: "foo".into(),
                req: VersionReq::parse("^1.0").unwrap(),
                from: "bar 0.1.0".into(),
            },
            VersionConstraint {
                package: "foo".into(),
                req: VersionReq::parse("^1.2").unwrap(),
                from: "baz 0.1.0".into(),
            },
        ];
        // Provide candidates that satisfy both.
        let mut avail = BTreeMap::new();
        avail.insert("foo".to_string(), vec![sv("1.2.0"), sv("1.3.0")]);
        let diags = check_conflicts(&constraints, &avail);
        assert!(diags.is_empty(), "compatible ranges with a valid candidate should not conflict");
    }

    #[test]
    fn advisory_parse_and_match() {
        let db = "JET-2026-0001|mylib|^1.0|1.0.5|Remote code execution via parse\n";
        let advisories = parse_advisory_db(db);
        assert_eq!(advisories.len(), 1);
        let adv = &advisories[0];
        assert_eq!(adv.id, "JET-2026-0001");
        assert!(adv.affects(&sv("1.0.3")));
        assert!(!adv.affects(&sv("1.0.5"))); // fixed
        assert!(!adv.affects(&sv("2.0.0"))); // outside ^1.0
    }

    fn make_lock_pkg(name: &str, version: &str, fp: &str) -> LockedPackage {
        LockedPackage {
            name: name.into(),
            version: version.into(),
            fingerprint: fp.into(),
            source: LockSource::Path("/tmp/placeholder".into()),
            locked: None,
            dependencies: vec![],
        }
    }

    fn make_lock(pkgs: Vec<LockedPackage>) -> LockFile {
        LockFile {
            version: 1,
            packages: pkgs,
            root_dependencies: vec![],
        }
    }

    #[test]
    fn audit_lockfile_emits_e2603() {
        let lock = make_lock(vec![make_lock_pkg("mylib", "1.0.3", "sha256-aabb")]);
        let db = "ADV-001|mylib|^1.0|1.0.5|XSS in template engine\n";
        let advisories = parse_advisory_db(db);
        let diags = audit_lockfile(&lock, &advisories);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, "E2603");
    }

    #[test]
    fn spdx_sbom_has_required_fields() {
        let lock = make_lock(vec![make_lock_pkg("helpers", "1.0.0", "sha256-abcd1234")]);
        let sbom = emit_spdx(&lock, "myapp", "0.1.0");
        assert!(sbom.contains("SPDXVersion: SPDX-2.3"), "must have version header");
        assert!(sbom.contains("PackageName: helpers"), "must list dependency");
        assert!(sbom.contains("PackageVersion: 1.0.0"));
        assert!(sbom.contains("SHA256: abcd1234"));
        assert!(sbom.contains("DEPENDS_ON"), "must have relationship");
    }

    #[test]
    fn cyclonedx_sbom_is_valid_json_structure() {
        let lock = make_lock(vec![make_lock_pkg("helpers", "1.0.0", "sha256-abcd1234")]);
        let sbom = emit_cyclonedx(&lock, "myapp", "0.1.0");
        assert!(sbom.contains("\"bomFormat\": \"CycloneDX\""));
        assert!(sbom.contains("\"name\": \"helpers\""));
        assert!(sbom.contains("SHA-256"));
    }

    #[test]
    fn api_diff_detects_removed_fn() {
        let old = vec![ApiItem {
            kind: "fn".into(),
            name: "parse".into(),
            signature: "fn parse(raw: String) -> Int".into(),
        }];
        let new = vec![];
        let changes = diff_public_api(&old, &new);
        assert!(!changes.is_empty());
        assert!(changes[0].description.contains("removed"));
    }

    #[test]
    fn api_diff_detects_changed_signature() {
        let old = vec![ApiItem {
            kind: "fn".into(),
            name: "parse".into(),
            signature: "fn parse(raw: String) -> Int".into(),
        }];
        let new = vec![ApiItem {
            kind: "fn".into(),
            name: "parse".into(),
            signature: "fn parse(raw: String) -> Float".into(),
        }];
        let changes = diff_public_api(&old, &new);
        assert!(!changes.is_empty());
        assert!(changes[0].description.contains("changed"));
    }

    #[test]
    fn api_diff_no_change() {
        let api = vec![ApiItem {
            kind: "fn".into(),
            name: "greet".into(),
            signature: "fn greet(name: String)".into(),
        }];
        let changes = diff_public_api(&api, &api);
        assert!(changes.is_empty());
    }

    #[test]
    fn pre_publish_gate_blocked_on_minor_with_break() {
        let gate = PrePublishGate {
            build_ok: true,
            tests_ok: true,
            breaking: vec![BreakingChange {
                description: "fn `foo` removed".into(),
                item_name: "foo".into(),
            }],
            version: "1.1.0".into(),
            bump_kind: BumpKind::Minor,
            next_major: 2,
        };
        assert!(gate.is_blocked());
        let errs = gate.semver_errors();
        assert!(!errs.is_empty());
        assert_eq!(errs[0].code, "E2601");
    }

    #[test]
    fn pre_publish_gate_passes_major_with_break() {
        let gate = PrePublishGate {
            build_ok: true,
            tests_ok: true,
            breaking: vec![BreakingChange {
                description: "fn `foo` removed".into(),
                item_name: "foo".into(),
            }],
            version: "2.0.0".into(),
            bump_kind: BumpKind::Major,
            next_major: 3,
        };
        assert!(!gate.is_blocked());
        assert!(gate.semver_errors().is_empty());
    }

    #[test]
    fn iso8601_format() {
        // Unix epoch → 1970-01-01T00:00:00Z
        assert_eq!(iso8601(0), "1970-01-01T00:00:00Z");
        // 2000-01-01T00:00:00Z = 946684800
        assert_eq!(iso8601(946684800), "2000-01-01T00:00:00Z");
        // 2024-01-01T00:00:00Z = 1704067200
        let ts = iso8601(1704067200);
        assert!(ts.starts_with("2024-01-01"), "got {}", ts);
    }

    #[test]
    fn registries_from_env() {
        let mut env = std::collections::HashMap::new();
        env.insert("JET_REGISTRY_PRIVATE_URL".into(), "https://my.company/jet".into());
        env.insert("JET_REGISTRY_PRIVATE_MIRROR".into(), "true".into());
        let regs = parse_registries_from_env(&env);
        assert!(!regs.is_empty());
        let r = &regs[0];
        assert_eq!(r.name, "private");
        assert!(r.mirror);
    }
}
