//! E2-M8 — packages and enterprise supply chain.
//!
//! Owns:
//!   - SemVer parsing and comparison (no external crates, I6).
//!   - Public API extraction from parsed Jet AST items.
//!   - API diff → E2601 (breaking change under non-breaking version bump).
//!   - PubGrub-style conflict detection → E2602.
//!   - Signed advisory feed and policy check → E2603/E2609/E2610.
//!   - Artifact integrity verification → E2604.
//!   - Deterministic SBOM emission and OCI referrer evidence for registry
//!     publication (SBOM, signature, provenance, reproducibility).
//!   - `jet registry vendor` (copy resolved deps into a `vendor/` tree).
//!   - Private / mirror registry configuration.

mod API;
pub mod Advisory;
// ApiFreeze: pure sema types from Sema::ApiFreeze; driver-level write_api_snapshot_for_entry
// lives in Publish/ApiFreeze.rs which re-exports the pure items and adds the loader call.
pub mod ApiFreeze;
// Schema: pure types from Sema::Schema; write_schema_snapshots_for_entry in Publish/Schema.rs.
pub use crate::Sema::Schema::{
    load_all_snapshots, load_snapshot, save_snapshot, schema_cache_dir, snapshot_from_struct,
    SchemaSnapshot, SnapshotField, SNAPSHOT_VERSION,
};
mod Schema;
pub use Schema::write_schema_snapshots_for_entry;
mod Diff;
pub mod Index;
mod NamePolicy;
pub mod Policy;
mod Registry;
mod Resolve;
mod SBOM;
pub mod SemVer;
pub mod Tier;
mod Tuf;
// c146 (D-PKGSIGN1): Ed25519 author signing via the hidden crypto bridge helper.
pub mod Sign;
mod Vendor;

pub use Advisory::*;
pub use Diff::*;
pub use Index::IndexEntry;
pub use NamePolicy::*;
pub use Policy::*;
pub use Registry::*;
pub(crate) use Registry::{
    read_registry_package_metadata, RegistryDependency, RegistryPackageMetadata,
};
pub use Resolve::*;
pub use SemVer::*;
pub use Tier::*;
pub use Tuf::*;
pub use Vendor::*;
pub use API::*;
pub use SBOM::*;

// ──────────────────────────────────────────────
// Unit tests
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Lock::{LockFile, LockSource, LockedPackage};
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
    fn semver_hostile_numbers_and_ambiguous_ranges_never_panic_or_match() {
        let huge_pre = format!("1.0.0-{}", "9".repeat(200));
        let parsed = SemVer::SemVer::parse(&huge_pre).expect("SemVer permits unbounded pre ids");
        assert!(parsed > sv("1.0.0-2"));

        for range in [
            "1 ||",
            "|| 1",
            "1 || || 2",
            "^18446744073709551615",
            "~1.18446744073709551615",
            "18446744073709551615.x",
            ">=",
            ">",
            "<",
            "=",
            "^",
            "~",
            "~>",
            "~>1.2",
        ] {
            assert!(
                VersionReq::parse(range).is_none(),
                "accepted hostile range: {range}"
            );
        }
    }

    #[test]
    fn semver_leading_v_is_consistent_across_versions_and_ranges() {
        assert_eq!(SemVer::SemVer::parse("v1.2.3"), Some(sv("1.2.3")));
        for range in ["v1.2", "=v1.2.3", ">=v1.2.3", "^v1.2.3", "~v1.2.3"] {
            assert!(
                VersionReq::parse(range).unwrap().matches(&sv("1.2.3")),
                "leading v failed in {range}"
            );
        }
        assert!(VersionReq::parse("v1.2.3 - v2.0.0")
            .unwrap()
            .matches(&sv("1.5.0")));
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
        assert!(!VersionReq::parse(">=1.0.0")
            .unwrap()
            .matches(&sv("2.0.0-alpha")));
        // …unless a comparator names the same tuple with a pre-release.
        assert!(VersionReq::parse(">=1.2.3-alpha")
            .unwrap()
            .matches(&sv("1.2.3-beta")));
        assert!(!VersionReq::parse(">=1.2.3-alpha")
            .unwrap()
            .matches(&sv("1.2.4-beta")));
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
        assert!(
            !diags.is_empty(),
            "disjoint caret ranges should be a conflict"
        );
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
        assert!(
            diags.is_empty(),
            "compatible ranges with a valid candidate should not conflict"
        );
    }

    #[test]
    fn advisory_parse_and_match() {
        let public_key = "00".repeat(32);
        let key_id = advisory_key_id(&public_key).unwrap();
        let feed = parse_advisory_feed(&format!(
            "{ADVISORY_FEED_MAGIC}\nfeed|1|100|1000|86400|{key_id}|{public_key}|sig\nadvisory|JET-2026-0001|mylib|^1.0|1.0.5|Remote code execution via parse|medium\n"
        ))
        .unwrap();
        assert_eq!(feed.advisories.len(), 1);
        let adv = &feed.advisories[0];
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
            content_hash: None,
            source: LockSource::Path("/tmp/placeholder".into()),
            locked: None,
            dependencies: vec![],
            layer: None,
            inferred_layer: None,
            effects: Vec::new(),
            effect_grants: Vec::new(),
            required_effects: Vec::new(),
            granted_effects: Vec::new(),
            denied_effects: Vec::new(),
            effect_authority: None,
            envelope: None,
            receipt: Default::default(),
            provenance: None,
        }
    }

    fn make_lock(pkgs: Vec<LockedPackage>) -> LockFile {
        LockFile {
            version: 1,
            packages: pkgs,
            root_dependencies: vec![],
            authority: None,
            workspace_members: Vec::new(),
            workspace_source_digest: None,
            workspace_overlay_policy: Default::default(),
            comptime_inputs: Vec::new(),
            toolchains: Vec::new(),
            browsers: Vec::new(),
            source_channels: Vec::new(),
            build_stamp: None,
            build_contributions: Vec::new(),
        }
    }

    #[test]
    fn audit_lockfile_emits_e2603() {
        let lock = make_lock(vec![make_lock_pkg("mylib", "1.0.3", "sha256-aabb")]);
        let advisories = vec![Advisory::Advisory {
            id: "ADV-001".into(),
            package: "mylib".into(),
            affected: VersionReq::parse("^1.0").unwrap(),
            fixed: Some(sv("1.0.5")),
            title: "XSS in template engine".into(),
            severity: Severity::Medium,
        }];
        let matches = audit_lockfile(&lock, &advisories).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].diagnostic.code, "E2603");
        // No explicit severity field → defaults to Medium (advisory, exit 0).
        assert_eq!(matches[0].severity, Severity::Medium);
    }

    #[test]
    fn audit_severity_parsed_from_db() {
        let lock = make_lock(vec![make_lock_pkg("mylib", "1.0.3", "sha256-aabb")]);
        let advisories = vec![Advisory::Advisory {
            id: "ADV-002".into(),
            package: "mylib".into(),
            affected: VersionReq::parse("^1.0").unwrap(),
            fixed: None,
            title: "Heap overflow".into(),
            severity: Severity::Critical,
        }];
        assert_eq!(advisories[0].severity, Severity::Critical);
        let matches = audit_lockfile(&lock, &advisories).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].severity, Severity::Critical);
    }

    #[test]
    fn advisory_parser_rejects_partial_ambiguous_and_invalid_records() {
        let public_key = "00".repeat(32);
        let key_id = advisory_key_id(&public_key).unwrap();
        for record in [
            "missing|fields|only",
            "advisory|ADV||^1.0||title|critical",
            "release|pkg@1.0.0|100|third-party",
            "advisory|ADV|pkg|^1.0|not-a-version|title|critical",
            "advisory|ADV|pkg|^1.0||title|critical|extra",
        ] {
            let text = format!(
                "{ADVISORY_FEED_MAGIC}\nfeed|1|100|1000|86400|{key_id}|{public_key}|sig\n{record}\n"
            );
            let diagnostic = parse_advisory_feed(&text).unwrap_err();
            assert_eq!(diagnostic.code, "E2607");
        }
    }

    #[test]
    fn signed_advisory_feed_parser_binds_exact_policy_targets() {
        let public_key = "00".repeat(32);
        let key_id = advisory_key_id(&public_key).unwrap();
        let text = format!(
            "{ADVISORY_FEED_MAGIC}\nfeed|7|100|100000|86400|{key_id}|{public_key}|signature\nrelease|mylib#1.0.3|100|third-party\nadvisory|ADV-1|mylib|^1.0|1.0.5|security fix|high\nexception|mylib#1.0.3|incident response|security-team|200000\n"
        );
        let feed = parse_advisory_feed(&text).expect("signed feed records should parse");
        assert_eq!(feed.sequence, 7);
        assert_eq!(feed.releases[0].source_class, SourceClass::ThirdParty);
        assert_eq!(feed.exceptions[0].package, "mylib");
        assert!(advisory_feed_payload(&feed).contains("mylib#1.0.3"));

        let at_target = text.replace("mylib#1.0.3", "mylib@1.0.3");
        assert_eq!(parse_advisory_feed(&at_target).unwrap_err().code, "E2607");
    }

    #[test]
    fn advisory_trust_and_policy_fail_closed() {
        let duplicate = "public_key=00\npublic_key=11\n";
        assert_eq!(parse_advisory_trust(duplicate).unwrap_err().code, "E2607");
        let revoked = parse_advisory_trust("public_key=00\nrevoked_key=00\n").unwrap();
        assert!(revoked.revoked_keys.contains("00"));
        assert_eq!(
            parse_advisory_trust("public_key=00\nmin_sequence=1\naccepted_digest=bad\n")
                .unwrap_err()
                .code,
            "E2607"
        );

        let diagnostic = e2609("mylib", "1.0.3", 86_500, SourceClass::ThirdParty);
        assert_eq!(diagnostic.code, "E2609");
        assert!(diagnostic.what.contains("mylib#1.0.3"));
        assert!(diagnostic.fix.contains("policy.exceptions"));
        assert!(diagnostic.fix.contains("mylib#1.0.3"));
    }

    #[test]
    fn audit_lockfile_rejects_invalid_locked_versions() {
        let lock = make_lock(vec![make_lock_pkg("mylib", "not-semver", "sha256-aabb")]);
        let error = audit_lockfile(&lock, &[]).expect_err("invalid lock versions must fail closed");
        assert_eq!(error.code, "E2610");
        assert!(error.what.contains("mylib"));
        assert!(error.what.contains("not-semver"));
    }

    #[test]
    fn malformed_supply_metadata_diagnostic_is_pinned() {
        let diagnostic = e2607("advisory feed", "line 2 has an invalid fixed version");
        assert_eq!(diagnostic.code, "E2607");
        assert_eq!(
            diagnostic.what,
            "advisory feed is malformed: line 2 has an invalid fixed version"
        );
        assert_eq!(
            diagnostic.why,
            "supply-chain metadata is security-sensitive, so Jet rejects ambiguous or partial records instead of silently skipping them."
        );
        assert_eq!(
            diagnostic.fix,
            "fix the malformed advisory feed record and retry; use the documented parser contract and UTF-8 text."
        );
    }

    #[test]
    fn spdx_sbom_has_required_fields() {
        let lock = make_lock(vec![make_lock_pkg("helpers", "1.0.0", "sha256-abcd1234")]);
        let sbom = emit_spdx(&lock, "myapp", "0.1.0");
        assert_eq!(sbom, emit_spdx(&lock, "myapp", "0.1.0"));
        assert!(
            sbom.contains("SPDXVersion: SPDX-2.3"),
            "must have version header"
        );
        assert!(
            sbom.contains("PackageName: helpers"),
            "must list dependency"
        );
        assert!(sbom.contains("PackageVersion: 1.0.0"));
        assert!(sbom.contains("SHA256: abcd1234"));
        assert!(sbom.contains("DEPENDS_ON"), "must have relationship");
    }

    #[test]
    fn registry_publish_writes_and_rejects_tampered_oci_referrers() {
        let root = std::env::temp_dir().join(format!(
            "jet_registry_referrers_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let repo = root.join("registry");
        let source = root.join("source");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(source.join(".jet")).unwrap();
        std::fs::write(source.join("package.jet"), "package bytes\n").unwrap();
        let lock = make_lock(vec![make_lock_pkg("helpers", "1.0.0", "sha256-abcd1234")]);
        std::fs::write(source.join(".jet").join("lock"), crate::Lock::write(&lock)).unwrap();

        let content_hash = crate::SHA256::tree_hash(&source);
        Registry::publish_artifact(&repo, &source, "ref-kit", "1.0.0", &content_hash).unwrap();
        let entry = IndexEntry {
            name: "ref-kit".into(),
            version: "1.0.0".into(),
            content_hash: content_hash.clone(),
            fingerprint: "sha256-fingerprint".into(),
            yanked: false,
            tier: RegistryTier::Core,
            gate_status: GateStatus::core_reviewed(),
            public_key: String::new(),
            signature: String::new(),
        };
        Index::write_index_entry(&repo, &entry).unwrap();
        Registry::verify_oci_referrers(&repo, &entry).unwrap();

        let blobs = repo.join("referrers").join(&content_hash).join("blobs");
        let blob = std::fs::read_dir(&blobs)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let mut bytes = std::fs::read(&blob).unwrap();
        bytes.push(b'\n');
        std::fs::write(blob, bytes).unwrap();
        let error = Registry::verify_oci_referrers(&repo, &entry)
            .expect_err("a tampered OCI referrer must fail closed");
        assert!(
            error.to_string().contains("digest") || error.to_string().contains("size"),
            "tampering must fail on a bound OCI blob: {error}"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn registry_publish_rejects_missing_sbom_before_index_exposure() {
        let root = std::env::temp_dir().join(format!(
            "jet_registry_missing_sbom_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let repo = root.join("registry");
        let source = root.join("source");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("package.jet"), "package bytes\n").unwrap();

        let content_hash = crate::SHA256::tree_hash(&source);
        Registry::publish_artifact(&repo, &source, "ref-kit", "1.0.0", &content_hash).unwrap();
        let pending = repo
            .join("referrers")
            .join(&content_hash)
            .join(".sbom.pending");
        std::fs::remove_file(&pending).unwrap();
        let entry = IndexEntry {
            name: "ref-kit".into(),
            version: "1.0.0".into(),
            content_hash,
            fingerprint: "sha256-fingerprint".into(),
            yanked: false,
            tier: RegistryTier::Core,
            gate_status: GateStatus::core_reviewed(),
            public_key: String::new(),
            signature: String::new(),
        };

        let error = Index::write_index_entry(&repo, &entry)
            .expect_err("a publication without its staged SBOM must fail closed");
        assert!(error.to_string().contains("SBOM evidence was not staged"));
        assert!(Index::find_entry(&repo, "ref-kit", "1.0.0")
            .unwrap()
            .is_none());
        std::fs::remove_dir_all(root).unwrap();
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
            signature: "fn parse(raw: String) Int".into(),
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
            signature: "fn parse(raw: String) Int".into(),
        }];
        let new = vec![ApiItem {
            kind: "fn".into(),
            name: "parse".into(),
            signature: "fn parse(raw: String) Float".into(),
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
        env.insert(
            "JET_REGISTRY_PRIVATE_URL".into(),
            "https://my.company/jet".into(),
        );
        env.insert("JET_REGISTRY_PRIVATE_MIRROR".into(), "true".into());
        let regs = parse_registries_from_env(&env);
        assert!(!regs.is_empty());
        let r = &regs[0];
        assert_eq!(r.name, "private");
        assert!(r.mirror);
    }
}
