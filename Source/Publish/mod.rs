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

pub mod Advisory;
mod API;
pub mod ApiFreeze;
mod Diff;
mod Registry;
mod Resolve;
mod SBOM;
mod Schema;
pub mod SemVer;
mod Vendor;

pub use Advisory::*;
pub use API::*;
pub use Diff::*;
pub use Registry::*;
pub use Resolve::*;
pub use SBOM::*;
pub use Schema::*;
pub use SemVer::*;
pub use Vendor::*;

// ──────────────────────────────────────────────
// Unit tests
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Lock::{LockFile, LockedPackage, LockSource};
    use std::collections::BTreeMap;

    fn sv(s: &str) -> SemVer::SemVer {
        SemVer::SemVer::parse(s).expect(s)
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

    // ── Full SemVer 2.0.0 coverage (D-PARSE-1) ──

    #[test]
    fn semver_parses_build_metadata() {
        let v = sv("1.2.3+build.5");
        assert_eq!(v.build.as_deref(), Some("build.5"));
        assert_eq!(v.pre, None);
        // Pre-release then build: 1.2.3-rc.1+exp.sha.
        let v2 = sv("1.2.3-rc.1+exp.sha.5114f85");
        assert_eq!(v2.pre.as_deref(), Some("rc.1"));
        assert_eq!(v2.build.as_deref(), Some("exp.sha.5114f85"));
        // Round-trips through Display.
        assert_eq!(v2.to_string(), "1.2.3-rc.1+exp.sha.5114f85");
    }

    #[test]
    fn semver_build_metadata_ignored_in_precedence() {
        assert_eq!(sv("1.2.3+a"), sv("1.2.3+b"));
        assert_eq!(sv("1.2.3+a").cmp(&sv("1.2.3")), std::cmp::Ordering::Equal);
    }

    #[test]
    fn semver_rejects_leading_zeros() {
        assert!(SemVer::SemVer::parse("01.2.3").is_none());
        assert!(SemVer::SemVer::parse("1.02.3").is_none());
        assert!(SemVer::SemVer::parse("1.2.3-01").is_none()); // numeric pre-release id, leading zero
        assert!(SemVer::SemVer::parse("1.2").is_none()); // not three components
        assert!(SemVer::SemVer::parse("1.2.3.4").is_none());
        // Build metadata may carry leading zeros.
        assert!(SemVer::SemVer::parse("1.2.3+0010").is_some());
    }

    #[test]
    fn semver_prerelease_precedence() {
        // Spec example chain: alpha < alpha.1 < alpha.beta < beta < beta.2 < beta.11 < rc.1 < release.
        let chain = [
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
        ];
        for w in chain.windows(2) {
            assert!(sv(w[0]) < sv(w[1]), "{} should be < {}", w[0], w[1]);
        }
    }

    #[test]
    fn version_req_operators() {
        assert!(VersionReq::parse(">=1.2.0").unwrap().matches(&sv("1.5.0")));
        assert!(!VersionReq::parse(">=1.2.0").unwrap().matches(&sv("1.1.9")));
        assert!(VersionReq::parse("<2.0.0").unwrap().matches(&sv("1.9.9")));
        assert!(!VersionReq::parse("<2.0.0").unwrap().matches(&sv("2.0.0")));
        // AND of two simples (whitespace), with space after operator.
        let r = VersionReq::parse(">=1.2.0 < 2.0.0").unwrap();
        assert!(r.matches(&sv("1.5.0")));
        assert!(!r.matches(&sv("2.0.0")));
        assert!(!r.matches(&sv("1.0.0")));
    }

    #[test]
    fn version_req_tilde() {
        let r = VersionReq::parse("~1.2.3").unwrap();
        assert!(r.matches(&sv("1.2.9")));
        assert!(!r.matches(&sv("1.3.0")));
        assert!(!r.matches(&sv("1.2.2")));
        let r2 = VersionReq::parse("~1").unwrap();
        assert!(r2.matches(&sv("1.9.9")));
        assert!(!r2.matches(&sv("2.0.0")));
    }

    #[test]
    fn version_req_x_ranges() {
        let r = VersionReq::parse("1.2.x").unwrap();
        assert!(r.matches(&sv("1.2.0")));
        assert!(r.matches(&sv("1.2.99")));
        assert!(!r.matches(&sv("1.3.0")));
        let r2 = VersionReq::parse("1").unwrap();
        assert!(r2.matches(&sv("1.9.9")));
        assert!(!r2.matches(&sv("2.0.0")));
    }

    #[test]
    fn version_req_hyphen_and_or() {
        let r = VersionReq::parse("1.2.3 - 2.3.4").unwrap();
        assert!(r.matches(&sv("1.2.3")));
        assert!(r.matches(&sv("2.3.4")));
        assert!(!r.matches(&sv("2.3.5")));
        // Partial high bound: `1.2.3 - 2.3` → <2.4.0.
        let r2 = VersionReq::parse("1.2.3 - 2.3").unwrap();
        assert!(r2.matches(&sv("2.3.9")));
        assert!(!r2.matches(&sv("2.4.0")));
        // OR.
        let r3 = VersionReq::parse("^1.0.0 || ^3.0.0").unwrap();
        assert!(r3.matches(&sv("1.5.0")));
        assert!(r3.matches(&sv("3.2.0")));
        assert!(!r3.matches(&sv("2.0.0")));
    }

    #[test]
    fn version_req_prerelease_rule() {
        // `*` and bare ranges do not match pre-releases…
        assert!(!VersionReq::parse(">=1.0.0").unwrap().matches(&sv("2.0.0-alpha")));
        // …unless a comparator names the same tuple with a pre-release.
        assert!(VersionReq::parse(">=1.2.3-alpha").unwrap().matches(&sv("1.2.3-beta")));
        assert!(!VersionReq::parse(">=1.2.3-alpha").unwrap().matches(&sv("1.2.4-beta")));
    }

    #[test]
    fn version_req_caret_zero() {
        // ^0.2.3 → >=0.2.3 <0.3.0
        let r = VersionReq::parse("^0.2.3").unwrap();
        assert!(r.matches(&sv("0.2.9")));
        assert!(!r.matches(&sv("0.3.0")));
        // ^0.0.3 → >=0.0.3 <0.0.4
        let r2 = VersionReq::parse("^0.0.3").unwrap();
        assert!(r2.matches(&sv("0.0.3")));
        assert!(!r2.matches(&sv("0.0.4")));
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
        let matches = audit_lockfile(&lock, &advisories);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].diagnostic.code, "E2603");
        // No explicit severity field → defaults to Medium (advisory, exit 0).
        assert_eq!(matches[0].severity, Severity::Medium);
    }

    #[test]
    fn audit_severity_parsed_from_db() {
        let lock = make_lock(vec![make_lock_pkg("mylib", "1.0.3", "sha256-aabb")]);
        let db = "ADV-002|mylib|^1.0||Heap overflow|critical\n";
        let advisories = parse_advisory_db(db);
        assert_eq!(advisories[0].severity, Severity::Critical);
        let matches = audit_lockfile(&lock, &advisories);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].severity, Severity::Critical);
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
