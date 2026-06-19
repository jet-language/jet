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

mod advisory;
mod api;
mod diff;
mod registry;
mod resolve;
mod sbom;
mod semver;
mod vendor;

pub use advisory::*;
pub use api::*;
pub use diff::*;
pub use registry::*;
pub use resolve::*;
pub use sbom::*;
pub use semver::*;
pub use vendor::*;

// ──────────────────────────────────────────────
// Unit tests
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::{LockFile, LockedPackage, LockSource};
    use std::collections::BTreeMap;

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
