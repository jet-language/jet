//! Integration tests for M12.1 — package manager (path deps, lock, store).
//!
//! Tests that need real file I/O create temp dirs under `std::env::temp_dir()`
//! and clean up on exit. Tests that need the `jet` binary or `git` subprocess
//! are skipped (not failed) if the tool is unavailable.
//!
//! Exit criteria (M12.1):
//!   path dep, git dep (local bare), lock verify, --locked CI mode,
//!   version conflict E1201, tamper E1204, two projects sharing one store inode,
//!   @latest update rewrites lock, reserved section E1209, toolchain mismatch E1208,
//!   end-to-end jet new → jet add --path, jet new --annotated snapshot.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

mod common;
use common::Scratch;

// Serialize tests that mutate process-global package environment or helper selection.
static STORE_LOCK: Mutex<()> = Mutex::new(());

// ─────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────

fn tmp_dir(label: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("jet_pkg_{}_{}", label, std::process::id()));
    fs::create_dir_all(&base).unwrap();
    base
}

fn write(dir: &Path, rel: &str, content: &str) {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

fn first_diag_code(diags: &[jet::Diagnostics::Diagnostic]) -> &str {
    diags.first().map(|d| d.code.as_str()).unwrap_or("")
}

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn have_git() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}

fn jet_cmd(args: &[&str], cwd: &Path, store_dir: &Path) -> std::process::Output {
    Command::new(jet_bin())
        .args(args)
        .current_dir(cwd)
        .env("JET_STORE_DIR", store_dir)
        .output()
        .expect("jet binary should run")
}

/// Run the jet binary with an explicit set of env vars (used to point
/// `jet registry publish`/`jet registry yank` at a scratch registry via `JET_REGISTRY_URL` and
/// `JET_REGISTRY_CACHE_DIR`).
fn jet_cmd_env(args: &[&str], cwd: &Path, envs: &[(&str, &str)]) -> std::process::Output {
    let mut cmd = Command::new(jet_bin());
    cmd.args(args).current_dir(cwd);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.output().expect("jet binary should run")
}

#[test]
fn shape6_registry_routes_and_retired_bare_snapshots() {
    let tmp = tmp_dir("shape6_registry");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();

    let key = jet_cmd(&["registry", "key"], &tmp, &store);
    assert!(!key.status.success());
    assert_eq!(
        String::from_utf8(key.stderr).unwrap(),
        "Error [E2104]: `jet registry key` needs a subcommand — try `jet registry key backup`.\n Why: Jet needs valid command input before it can run this command\n Fix: correct the named argument or input, then run the command again\n"
    );

    for verb in ["publish", "keygen", "key", "yank"] {
        let bare = jet_cmd(&[verb, "sentinel"], &tmp, &store);
        assert_eq!(bare.status.code(), Some(2), "bare jet {verb}");
        assert_eq!(
            String::from_utf8(bare.stderr).unwrap(),
            format!(
                "Error [E2101]: `{verb}` moved under `jet registry`.\n Why: infrequent commands live in a named area so daily Jet commands stay easy to scan.\n Fix: run `jet registry {verb} sentinel`.\n"
            )
        );
    }

    fs::remove_dir_all(tmp).unwrap();
}

// #2075: every bridge key's built artifacts live in ONE Cargo target dir shared
// per (toolchain, target, profile) — `<home>/.cache/jet/ffi/deps/<build
// hash>/<triple>/release/` — with the key in each file NAME
// (`jet-crypto-helper-<key>`, `libjet_ffi_<key>.rlib`, and the digest sidecar
// `jet_ffi_<key>.sha256` beside them). `cached_crypto_helper_path()` (called
// against the *test process's own* real HOME) returns that helper path for the
// current deps/target key — the key is HOME-independent, so the tail below the
// cache root is safe to re-root under an isolated `home`.
fn isolated_crypto_helper_paths(home: &Path) -> (PathBuf, PathBuf) {
    let real_helper = jetpack::FFI::cached_crypto_helper_path();
    let real_root = real_helper
        .ancestors()
        .find(|dir| dir.ends_with("ffi"))
        .expect("the crypto helper lives under the FFI cache root");
    let relative = real_helper.strip_prefix(real_root).unwrap();
    let helper = home.join(".cache/jet/ffi").join(relative);
    let cache_key = common::ffi_bridge_key(&helper);
    let rlib = helper
        .parent()
        .unwrap()
        .join(format!("libjet_ffi_{cache_key}.rlib"));
    (rlib, helper)
}

/// Install a fake crypto helper AND its cache-verification sidecar
/// (`artifacts.sha256`) so `ensure_bridge_helper()`'s cache-hit fast path
/// (`bridge_cache_verified`) accepts these files instead of falling through to
/// a real `cargo build` that would silently bypass the fake helper.
#[cfg(unix)]
fn install_closed_status_crypto_helper(home: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let (rlib, helper) = isolated_crypto_helper_paths(home);
    let release = helper.parent().unwrap();
    fs::create_dir_all(release).unwrap();

    let rlib_bytes = b"test cache sentinel".to_vec();
    fs::write(&rlib, &rlib_bytes).unwrap();

    let crate_stem = rlib.file_stem().unwrap().to_str().unwrap().to_string();
    let cdylib = release.join(format!("{crate_stem}.{}", std::env::consts::DLL_EXTENSION));
    let cdylib_bytes = b"test cache cdylib sentinel".to_vec();
    fs::write(&cdylib, &cdylib_bytes).unwrap();

    let signature = "00".repeat(64);
    let helper_bytes = format!(
        "#!/bin/sh\nIFS= read -r command\ncase \"$command\" in\n  keygen*) printf 'secret helper output' ; printf 'raw OS status and dependency text' >&2 ; exit 75 ;;\n  sign*) printf '%s\\n' '{signature}' ;;\n  verify*) exit 0 ;;\n  *) exit 1 ;;\nesac\n"
    )
    .into_bytes();
    fs::write(&helper, &helper_bytes).unwrap();
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();

    // The sidecar sits beside the artifacts it blesses, one row per file, each
    // row's path relative to that shared release dir — i.e. a bare file name.
    let crate_name = crate_stem
        .strip_prefix("lib")
        .expect("bridge rlib is `lib*`");
    let mut manifest = String::from("jet-ffi-artifacts-v2\n");
    for (bytes, path) in [
        (&rlib_bytes, &rlib),
        (&cdylib_bytes, &cdylib),
        (&helper_bytes, &helper),
    ] {
        let relative = path.file_name().unwrap().to_str().unwrap();
        manifest.push_str(&format!("{} {relative}\n", jet::SHA256::sha256_hex(bytes)));
    }
    fs::write(release.join(format!("{crate_name}.sha256")), manifest).unwrap();
}

/// Init an empty bare git repo standing in for a registry index. Returns its
/// `file://` URL.
fn bare_registry(dir: &Path) -> String {
    Command::new("git")
        .args(["init", "--bare", dir.to_str().unwrap()])
        .output()
        .unwrap();
    format!("file://{}", dir.to_str().unwrap())
}

/// Seed the maintainer-owned receipt required for a core-tier publish.
fn seed_core_review(dir: &Path, package: &str, version: &str) {
    let work = dir.with_extension(format!("review-{}", std::process::id()));
    let url = format!("file://{}", dir.to_str().unwrap());
    Command::new("git")
        .args([
            "--git-dir",
            dir.to_str().unwrap(),
            "symbolic-ref",
            "HEAD",
            "refs/heads/main",
        ])
        .status()
        .unwrap();
    Command::new("git")
        .args(["clone", url.as_str(), work.to_str().unwrap()])
        .status()
        .unwrap();
    let review = work
        .join("reviews")
        .join(package)
        .join(format!("{version}.review"));
    fs::create_dir_all(review.parent().unwrap()).unwrap();
    fs::write(
        &review,
        format!(
            "jet-registry-core-review-v1\npackage={package}\nversion={version}\nreviewer=test-maintainer\ndecision=approved\n"
        ),
    )
    .unwrap();
    for args in [
        vec!["config", "user.email", "test@jet.test"],
        vec!["config", "user.name", "Jet Test"],
        vec!["add", "."],
        vec!["commit", "-m", "approve core package"],
        vec!["push", "origin", "HEAD:main"],
    ] {
        Command::new("git")
            .args(&args)
            .current_dir(&work)
            .status()
            .unwrap();
    }
    fs::remove_dir_all(work).unwrap();
}

/// Read `index/<name>/<name>.jsonl` out of a bare registry via `git show`.
fn read_index_file(bare: &Path, name: &str) -> Option<String> {
    let spec = format!("HEAD:index/{name}/{name}.jsonl");
    let out = Command::new("git")
        .args(["--git-dir", bare.to_str().unwrap(), "show", &spec])
        .output()
        .unwrap();
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        None
    }
}

/// Write a minimal project and commit it (clean tree) so `jet registry publish` clears
/// the dirty-tree gate (E2605).
fn init_clean_project(dir: &Path, name: &str, version: &str) {
    write(dir, "package.jet", &min_manifest(name, version));
    write(
        dir,
        "run.jet",
        "#Test(\"smoke\") { expect(1 == 1) }\nfn run() { print(\"hi\"); }\n",
    );
    for args in &[
        vec!["init", "-b", "main"],
        vec!["config", "user.email", "test@jet.test"],
        vec!["config", "user.name", "Jet Test"],
        vec!["add", "."],
        vec!["commit", "-m", "init"],
    ] {
        Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
    }
}

/// Run `f` with JET_STORE_DIR set to `store_dir`, serializing concurrent calls.
fn with_store<T, F: FnOnce() -> T>(store_dir: &Path, f: F) -> T {
    let _guard = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let prev = std::env::var("JET_STORE_DIR").ok();
    std::env::set_var("JET_STORE_DIR", store_dir);
    let result = f();
    match prev {
        Some(v) => std::env::set_var("JET_STORE_DIR", v),
        None => std::env::remove_var("JET_STORE_DIR"),
    }
    result
}

// Minimal `package.jet` (Jet syntax, U1) for a named package with no deps.
fn min_manifest(name: &str, version: &str) -> String {
    format!(
        "name: \"{}\"\nversion: \"{}\"\njet: \">=0.1.0\"\ndescription: \"\"\nlicense: \"MIT\"\nrepository: \"\"\n",
        name, version
    )
}

// `min_manifest` plus a `deps: { … }` block holding the given entry lines.
fn manifest_with_deps(name: &str, version: &str, dep_lines: &str) -> String {
    format!(
        "{}\ndeps: {{\n{}\n}}\n",
        min_manifest(name, version),
        dep_lines
    )
}

// ─────────────────────────────────────────────
// E4 Jetpack: strict graph, semantic lock, importers, federated providers
// ─────────────────────────────────────────────

fn graph_with_app() -> jetpack::PackageGraph::PackageGraph {
    use jetpack::PackageGraph::{PackageGraph, PackageNode};
    let mut graph = PackageGraph::new();
    graph.add_package(
        PackageNode::new("app")
            .with_deps(&["ui"])
            .with_transitives(&["log"]),
    );
    graph.add_package(PackageNode::new("ui").with_deps(&["log"]));
    graph.add_package(PackageNode::new("worker").with_transitives(&["log"]));
    graph
}

fn semantic_record(owner: &str, key: &str, exact: &str) -> jetpack::SemanticLock::SemanticRecord {
    use jetpack::SemanticLock::{LockIdentity, LockRationale, LockRecordKind, SemanticRecord};
    SemanticRecord::new(
        LockIdentity {
            kind: LockRecordKind::Package,
            key: key.to_string(),
            exact: exact.to_string(),
            hash: format!("sha256-{exact}"),
            platform: "x86_64-linux".to_string(),
        },
        LockRationale {
            owner_package: owner.to_string(),
            reason: format!("{owner} declared {key}"),
            source_ref: format!("catalog:{key}"),
            provider: "core".to_string(),
            channel_input: "stable".to_string(),
            exact_output: exact.to_string(),
            policy_fingerprint: "policy-1".to_string(),
            recipe_id: String::new(),
            adapter_id: String::new(),
            signature: "ed25519:sig".to_string(),
            cache_provenance: "hangar".to_string(),
            update_command: format!("jet update {key}"),
        },
    )
}

fn semantic_record_with_hash(
    owner: &str,
    key: &str,
    exact: &str,
    hash: &str,
) -> jetpack::SemanticLock::SemanticRecord {
    let mut record = semantic_record(owner, key, exact);
    record.identity.hash = hash.to_string();
    record
}

fn replacement_identity(
    provider: &str,
    name: &str,
    version: &str,
) -> jetpack::Replacement::PackageIdentity {
    jetpack::Replacement::PackageIdentity::new(provider, name, version)
}

fn replacement_surface(
    provider: &str,
    name: &str,
    version: &str,
) -> jetpack::Replacement::CompatibilitySurface {
    use jetpack::Replacement::{
        CompatibilitySurface, GoldenFixture, PackageIdentity, PublicSymbol,
    };
    let mut surface = CompatibilitySurface::new(PackageIdentity::new(provider, name, version));
    surface.public_symbols = vec![
        PublicSymbol::new("pad_left", "fn(String, Int) String")
            .with_effects(&["pure"])
            .with_errors(&["ValueError"]),
        PublicSymbol::new("trim", "fn(String) String").with_effects(&["pure"]),
    ];
    surface.examples = vec!["examples/replacement/pad.jet".to_string()];
    surface.goldens = vec![GoldenFixture::new("pad_left_basic", "  hi\n")];
    surface.platforms = vec!["x86_64-linux".to_string()];
    surface
}

fn replacement_passed_candidate() -> jetpack::Replacement::ReplacementCandidate {
    let foreign = replacement_surface("npm", "left-pad", "1.0.0");
    let native = replacement_surface("core", "core.text.pad", "1.0.0");
    let report = jetpack::Replacement::run_proof(&foreign, &native, "x86_64-linux");
    report.candidate("MIT", vec!["x86_64-linux".to_string()])
}

#[test]
fn strict_graph_rejects_transitive_import() {
    let graph = graph_with_app();
    let err = graph
        .check_visible("app", "log")
        .expect_err("transitive dep must not be visible");
    assert!(err.reason.contains("direct deps"));
    assert_eq!(err.requested, "log");
}

#[test]
fn strict_graph_accepts_direct_dep() {
    let graph = graph_with_app();
    let edge = graph
        .check_visible("app", "ui")
        .expect("direct dep visible");
    assert_eq!(edge.dependency, "ui");
    assert_eq!(edge.kind, jetpack::PackageGraph::VisibleEdgeKind::DirectDep);
}

#[test]
fn catalog_edge_behaves_like_direct_dep_after_selection() {
    use jetpack::PackageGraph::{CatalogEntry, VisibleEdgeKind};
    let mut graph = graph_with_app();
    graph.add_catalog(CatalogEntry {
        logical_name: "log".to_string(),
        provider_ref: "core.log@1.2.3".to_string(),
        version_rule: "1.2".to_string(),
        allowed_packages: vec!["app".to_string()],
        owner_workspace: "root".to_string(),
        rationale: "workspace selected shared logging version".to_string(),
    });
    let edge = graph
        .check_visible("app", "log")
        .expect("catalog edge visible");
    assert_eq!(edge.kind, VisibleEdgeKind::Catalog);
    assert_eq!(edge.provider_ref, "core.log@1.2.3");
}

#[test]
fn lock_records_catalog_owner_and_rationale() {
    use jetpack::SemanticLock::{parse, write, SemanticLockFile};
    let lock = SemanticLockFile {
        records: vec![semantic_record("app", "core.log", "1.2.3")],
        ..Default::default()
    };
    let text = write(&lock);
    assert!(text.contains("owner-package = \"app\""));
    assert!(text.contains("reason = \"app declared core.log\""));
    let parsed = parse(&text);
    assert_eq!(parsed.records[0].rationales[0].owner_package, "app");
}

#[test]
fn catalog_merge_conflict_names_owner_package() {
    use jetpack::SemanticLock::{merge, SemanticLockFile};
    let left = SemanticLockFile {
        records: vec![semantic_record("app", "core.log", "1.2.3")],
        ..Default::default()
    };
    let right = SemanticLockFile {
        records: vec![semantic_record("app", "core.log", "1.3.0")],
        ..Default::default()
    };
    let out = merge(&SemanticLockFile::default(), &left, &right);
    assert_eq!(out.conflicts.len(), 1);
    assert_eq!(out.conflicts[0].owner_package, "app");
    assert_eq!(out.conflicts[0].semantic_key, "package:core.log");
}

#[test]
fn missing_dep_fix_prefers_direct_add_for_single_package() {
    let graph = graph_with_app();
    let err = graph.check_visible("app", "only-app").unwrap_err();
    assert!(err
        .fix_text()
        .contains("jet add only-app --path ../only-app"));
}

#[test]
fn missing_dep_fix_does_not_catalog_without_hidden_use_evidence() {
    let mut graph = graph_with_app();
    graph
        .add_package(jetpack::PackageGraph::PackageNode::new("direct-only").with_deps(&["shared"]));
    let err = graph.check_visible("app", "shared").unwrap_err();
    assert!(matches!(
        err.fix,
        jetpack::PackageGraph::MissingDependencyFix::DirectAddPath { .. }
    ));
}

#[test]
fn missing_dep_fix_prefers_catalog_for_workspace_reuse() {
    let graph = graph_with_app();
    let err = graph.check_visible("app", "log").unwrap_err();
    assert!(err.fix_text().contains("catalog data"));
    assert!(err.fix_text().contains("app"));
    assert!(err.fix_text().contains("worker"));
}

#[test]
fn lsp_completion_hides_transitive_deps() {
    let graph = graph_with_app();
    let visible: Vec<String> = graph
        .visible_edges("app")
        .into_iter()
        .map(|edge| edge.dependency)
        .collect();
    assert_eq!(visible, vec!["ui".to_string()]);
    assert!(!visible.contains(&"log".to_string()));
}

#[test]
fn lock_record_kinds_roundtrip_unknown_future_fields() {
    use jetpack::SemanticLock::{parse, write, LockRecordKind, SemanticLockFile};
    let raw = "semantic-lock-version = 1\n\n[[semantic_record]]\nkind = \"future-kind\"\nkey = \"k\"\nexact = \"e\"\nhash = \"h\"\nplatform = \"p\"\nfuture-key = \"future-value\"\n";
    let parsed = parse(raw);
    assert!(matches!(
        parsed.records[0].identity.kind,
        LockRecordKind::Future(_)
    ));
    assert_eq!(
        parsed.records[0].future_fields.get("future-key"),
        Some(&"future-value".to_string())
    );
    let reparsed = parse(&write(&SemanticLockFile {
        records: parsed.records,
        ..Default::default()
    }));
    assert_eq!(
        reparsed.records[0].future_fields.get("future-key"),
        Some(&"future-value".to_string())
    );
}

#[test]
fn lock_rationale_preserves_exact_identity() {
    use jetpack::SemanticLock::SemanticLockFile;
    let a = SemanticLockFile {
        records: vec![semantic_record("app", "core.log", "1.2.3")],
        ..Default::default()
    };
    let mut b = a.clone();
    b.records[0].rationales[0].reason = "human text changed".to_string();
    assert_eq!(
        a.records[0].identity.machine_identity(),
        b.records[0].identity.machine_identity()
    );
}

#[test]
fn lock_merge_independent_additions() {
    use jetpack::SemanticLock::{merge, SemanticLockFile};
    let left = SemanticLockFile {
        records: vec![semantic_record("app", "core.log", "1.2.3")],
        ..Default::default()
    };
    let right = SemanticLockFile {
        records: vec![semantic_record("app", "core.http", "2.0.0")],
        ..Default::default()
    };
    let out = merge(&SemanticLockFile::default(), &left, &right);
    assert!(out.conflicts.is_empty());
    assert_eq!(out.merged.records.len(), 2);
}

#[test]
fn lock_merge_same_identity_two_owners() {
    use jetpack::SemanticLock::{merge, SemanticLockFile};
    let left = SemanticLockFile {
        records: vec![semantic_record("app", "core.log", "1.2.3")],
        ..Default::default()
    };
    let right = SemanticLockFile {
        records: vec![semantic_record("worker", "core.log", "1.2.3")],
        ..Default::default()
    };
    let out = merge(&SemanticLockFile::default(), &left, &right);
    assert!(out.conflicts.is_empty());
    assert_eq!(out.merged.records[0].rationales.len(), 2);
}

#[test]
fn lock_merge_conflicting_identity_diagnostic() {
    use jetpack::SemanticLock::{merge, SemanticLockFile};
    let left = SemanticLockFile {
        records: vec![semantic_record("app", "core.log", "1.2.3")],
        ..Default::default()
    };
    let right = SemanticLockFile {
        records: vec![semantic_record("app", "core.log", "1.3.0")],
        ..Default::default()
    };
    let out = merge(&SemanticLockFile::default(), &left, &right);
    assert_eq!(out.conflicts[0].left_reason, "app declared core.log");
    assert_eq!(out.conflicts[0].right_reason, "app declared core.log");
}

#[test]
fn lock_merge_conflicts_on_same_version_different_hash() {
    use jetpack::SemanticLock::{merge, SemanticLockFile};
    let left = SemanticLockFile {
        records: vec![semantic_record_with_hash(
            "app",
            "core.log",
            "1.2.3",
            "sha256-left",
        )],
        ..Default::default()
    };
    let right = SemanticLockFile {
        records: vec![semantic_record_with_hash(
            "app",
            "core.log",
            "1.2.3",
            "sha256-right",
        )],
        ..Default::default()
    };
    let out = merge(&SemanticLockFile::default(), &left, &right);
    assert_eq!(out.conflicts.len(), 1);
}

#[test]
fn lock_merge_accepts_one_sided_identity_change_from_base() {
    use jetpack::SemanticLock::{merge, SemanticLockFile};
    let base = SemanticLockFile {
        records: vec![semantic_record("app", "core.log", "1.2.3")],
        ..Default::default()
    };
    let left = base.clone();
    let right = SemanticLockFile {
        records: vec![semantic_record("app", "core.log", "1.3.0")],
        ..Default::default()
    };
    let out = merge(&base, &left, &right);
    assert!(out.conflicts.is_empty());
    assert_eq!(out.merged.records[0].identity.exact, "1.3.0");
}

#[test]
fn lock_merge_platform_specific_records() {
    use jetpack::SemanticLock::{merge, SemanticLockFile};
    let left = SemanticLockFile {
        records: vec![semantic_record("app", "bin.tool:x86_64-linux", "1")],
        ..Default::default()
    };
    let mut right_rec = semantic_record("app", "bin.tool:aarch64-macos", "1");
    right_rec.identity.platform = "aarch64-macos".to_string();
    let right = SemanticLockFile {
        records: vec![right_rec],
        ..Default::default()
    };
    let out = merge(&SemanticLockFile::default(), &left, &right);
    assert!(out.conflicts.is_empty());
    assert_eq!(out.merged.records.len(), 2);
}

#[test]
fn lock_explain_names_owner_policy_provider_platform() {
    use jetpack::SemanticLock::{explain, SemanticLockFile};
    let lock = SemanticLockFile {
        records: vec![semantic_record("app", "core.log", "1.2.3")],
        ..Default::default()
    };
    let fact = explain(&lock, "package:core.log").expect("explain fact");
    assert_eq!(fact.owners, vec!["app".to_string()]);
    assert_eq!(fact.provider, "core");
    assert_eq!(fact.platform, "x86_64-linux");
    assert_eq!(fact.policy_fingerprint, "policy-1");
    assert_eq!(fact.contenders.len(), 1);
}

#[test]
fn lock_satisfied_offline_verbs_do_not_touch_network() {
    use jetpack::ProviderGraph::{
        AuthorityGraph, FetchDecision, ProviderFamily, ProviderObject, ProviderRequest,
    };
    let obj = ProviderObject {
        family: ProviderFamily::Npm,
        ref_key: "left-pad".to_string(),
        exact_identity: "npm:left-pad@1.0.0".to_string(),
        hash: "sha256-aa".to_string(),
        platform: "any".to_string(),
        signature: String::new(),
        audit: Vec::new(),
        sandbox_effects: Vec::new(),
        build_effects: Vec::new(),
    };
    let mut graph = AuthorityGraph::default();
    graph.add_locked(obj);
    let decision = graph.fetch_allowed(&ProviderRequest {
        family: ProviderFamily::Npm,
        ref_key: "left-pad".to_string(),
        exact_identity: "npm:left-pad@1.0.0".to_string(),
        hash: "sha256-aa".to_string(),
        platform: "any".to_string(),
        offline: true,
    });
    assert_eq!(decision, FetchDecision::AllowedOfflineSatisfied);
}

#[test]
fn read_only_verbs_do_not_rewrite_lock() {
    use jetpack::SemanticLock::{parse, write, SemanticLockFile};
    let lock = SemanticLockFile {
        records: vec![semantic_record("app", "core.log", "1.2.3")],
        ..Default::default()
    };
    let before = write(&lock);
    let after = write(&parse(&before));
    assert_eq!(before, after);
}

#[test]
fn nix_import_emits_role_modules_and_todos() {
    let plan = jetpack::MigrationImport::import_nix_facts(
        "flake.nix",
        r#"{"name":"app","packages":["ripgrep"],"shellHook":"echo hi"}"#,
    );
    assert!(plan.generated_files.iter().any(|f| f.path == "env.jet"));
    assert!(plan.todos.iter().any(|t| t.message.contains("shellHook")));
}

#[test]
fn nix_import_emits_exact_refs_and_retains_native_facts() {
    let plan = jetpack::MigrationImport::import_nix_facts(
        "flake.lock",
        r#"{"name":"app","version":"1.0.0","source":"nixpkgs","packages":[{"name":"ripgrep","version":"14.1.1","drvPath":"/nix/store/hash-ripgrep-14.1.1.drv","buildInputs":["openssl"],"system":"x86_64-linux","license":"BSD-3-Clause"}]}"#,
    );
    assert_eq!(plan.deps[0].provider_ref, "ripgrep@nixpkgs#version=14.1.1");
    assert_eq!(plan.deps[0].locked_version, "14.1.1");
    assert!(plan
        .emit_pkg_jet()
        .contains("ripgrep: ripgrep@nixpkgs#version=14.1.1"));
    let facts = &plan.provider_facts["ripgrep@nixpkgs#version=14.1.1"];
    facts.validate().expect("exact Nix import is lossless");
    assert_eq!(facts.native_format, "flake-facts.json");
    assert!(facts.native_document.contains("drvPath"));
    assert!(facts.facts.contains_key("provider.nix.import.drvPath"));
    assert!(facts.facts.contains_key("provider.nix.dependency.build"));
    assert!(facts.facts.contains_key("provider.nix.variant.system"));
    assert!(facts.facts.contains_key("provider.nix.license"));
    assert!(facts.facts.contains_key("provider.nix.source.drv_path"));
}

#[test]
fn nix_import_keeps_mutable_refs_out_of_generated_source() {
    let plan = jetpack::MigrationImport::import_nix_facts(
        "flake.nix",
        r#"{"name":"app","packages":["ripgrep"]}"#,
    );
    assert_eq!(plan.deps[0].provider_ref, "ripgrep@nixpkgs");
    assert!(!plan.emit_pkg_jet().contains("ripgrep: ripgrep@nixpkgs"));
    assert!(plan.provider_facts["ripgrep@nixpkgs"]
        .losses
        .iter()
        .any(|loss| loss.reason.contains("no exact version")));
    assert!(plan
        .todos
        .iter()
        .any(|todo| todo.message.contains("migration remains unresolved")));
}

#[test]
fn nix_import_reports_malformed_package_facts_without_generating_them() {
    let plan = jetpack::MigrationImport::import_nix_facts(
        "flake.lock",
        r#"{"name":"app","version":"1.0.0","packages":[{"name":"broken","version":"1.0.0","drvPath":1,"buildInputs":false}]}"#,
    );
    let facts = &plan.provider_facts["broken@nixpkgs#version=1.0.0"];
    assert!(facts
        .losses
        .iter()
        .any(|loss| loss.reason.contains("drvPath")));
    assert!(facts
        .losses
        .iter()
        .any(|loss| loss.reason.contains("buildInputs")));
    assert!(!plan
        .emit_pkg_jet()
        .contains("broken: broken@nixpkgs#version=1.0.0"));
}

#[test]
fn cargo_import_preserves_locked_versions() {
    let plan = jetpack::MigrationImport::import_cargo(
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n[dependencies]\nserde = \"1\"\n",
        "[[package]]\nname = \"serde\"\nversion = \"1.0.200\"\n",
    );
    assert_eq!(plan.deps[0].locked_version, "1.0.200");
    assert_eq!(plan.deps[0].provider_ref, "serde@cargo#version=1.0.200");
    assert!(plan
        .emit_pkg_jet()
        .contains("serde: serde@cargo#version=1.0.200"));
    assert!(plan.provider_facts["serde#version=1.0.200@cargo"]
        .validate()
        .is_ok());
    assert!(plan.ffi_stubs.iter().any(|s| s.symbol == "serde"));
}

#[test]
fn npm_import_turns_scripts_into_legacy_build_actions() {
    let plan = jetpack::MigrationImport::import_npm(
        r#"{"name":"web","version":"1.0.0","dependencies":{"vite":"5"},"scripts":{"build":"vite build"}}"#,
    );
    assert!(plan.deps.iter().any(|d| d.name == "vite"));
    assert_eq!(plan.deps[0].provider_ref, "vite@npm");
    assert!(!plan.emit_pkg_jet().contains("vite: vite@npm"));
    assert!(plan
        .todos
        .iter()
        .any(|t| t.message.contains("legacy build action")));
    assert!(plan.provider_facts.values().any(|facts| facts
        .losses
        .iter()
        .any(|loss| loss.reason.contains("not an exact lock"))));
}

#[test]
fn npm_import_emits_exact_provider_refs_and_retains_requests() {
    let plan = jetpack::MigrationImport::import_npm(
        r#"{"name":"web","version":"1.0.0","dependencies":{"vite":"5.4.0"}}"#,
    );
    assert_eq!(plan.deps[0].provider_ref, "vite@npm#version=5.4.0");
    assert_eq!(plan.deps[0].locked_version, "5.4.0");
    assert!(plan.emit_pkg_jet().contains("vite: vite@npm#version=5.4.0"));
    assert!(plan.provider_facts["vite@npm#version=5.4.0"]
        .validate()
        .is_ok());
}

#[test]
fn cargo_import_reports_missing_lock_without_generating_mutable_ref() {
    let plan = jetpack::MigrationImport::import_cargo(
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n[dependencies]\nserde = \"1\"\n",
        "",
    );
    assert_eq!(plan.deps[0].provider_ref, "serde@cargo");
    assert!(!plan.emit_pkg_jet().contains("serde: serde@cargo"));
    assert!(plan
        .todos
        .iter()
        .any(|todo| todo.message.contains("unresolved")));
    assert!(plan.provider_facts["serde@cargo"]
        .losses
        .iter()
        .any(|loss| loss.reason.contains("no exact version")));
}

#[test]
fn python_import_marks_dynamic_metadata_todo() {
    let plan = jetpack::MigrationImport::import_python_metadata("pkg", &["version"]);
    assert!(plan.todos[0].message.contains("dynamic Python metadata"));
    assert!(jetpack::MigrationImport::todo_diagnostics_need_ballot());
}

#[test]
fn swiftpm_import_emits_provider_refs() {
    let plan = jetpack::MigrationImport::import_swiftpm("swift-log", "abc123");
    assert_eq!(
        plan.deps[0].provider_ref,
        "swift-log#revision=abc123@swiftpm"
    );
    assert_eq!(plan.deps[0].locked_version, "abc123");
    assert!(plan.provider_facts["swift-log#revision=abc123@swiftpm"]
        .validate()
        .is_ok());
}

#[test]
fn import_idempotent_without_user_edits() {
    use jetpack::MigrationImport::{merge_generated_file, GeneratedFile};
    let file = GeneratedFile {
        path: "env.jet".to_string(),
        contents: "module env.dev\n".to_string(),
        owned: true,
    };
    let merged = merge_generated_file(Some("module env.dev\n"), &file).expect("stable");
    assert_eq!(merged, "module env.dev\n");
}

#[test]
fn import_conflict_preserves_user_edit() {
    use jetpack::MigrationImport::{merge_generated_file, GeneratedFile, ImportConflict};
    let file = GeneratedFile {
        path: "env.jet".to_string(),
        contents: "module env.dev\n".to_string(),
        owned: true,
    };
    let err = merge_generated_file(Some("# jet-import: edited\nmodule env.dev\n"), &file)
        .expect_err("user edit must conflict");
    assert_eq!(
        err,
        ImportConflict::UserEditedGeneratedLine {
            path: "env.jet".to_string()
        }
    );
}

#[test]
fn import_conflicts_on_any_generated_file_drift() {
    use jetpack::MigrationImport::{merge_generated_file, GeneratedFile};
    let file = GeneratedFile {
        path: "env.jet".to_string(),
        contents: "module env.dev\n".to_string(),
        owned: true,
    };
    let err = merge_generated_file(Some("module env.dev\n// user change\n"), &file)
        .expect_err("any drift from generated baseline must conflict");
    assert_eq!(
        err,
        jetpack::MigrationImport::ImportConflict::UserEditedGeneratedLine {
            path: "env.jet".to_string()
        }
    );
}

#[test]
fn migration_status_feeds_lock_explain() {
    use jetpack::MigrationImport::MigrationStatus;
    let status = MigrationStatus::AdapterWrapped {
        name: "left-pad".to_string(),
    };
    let mut record = semantic_record("app", "left-pad", "1.0.0");
    record.rationales[0].adapter_id = match status {
        MigrationStatus::AdapterWrapped { name } => format!("adapter:{name}"),
        _ => String::new(),
    };
    assert_eq!(record.rationales[0].adapter_id, "adapter:left-pad");
}

#[test]
fn provider_contract_covers_core_nix_path_github() {
    use jetpack::ProviderGraph::{built_in_contracts, ProviderFamily};
    let contracts = built_in_contracts();
    let families: Vec<ProviderFamily> = contracts.iter().map(|c| c.family.clone()).collect();
    assert!(families.contains(&ProviderFamily::Core));
    assert!(families.contains(&ProviderFamily::Nix));
    assert!(families.contains(&ProviderFamily::Path));
    assert!(families.contains(&ProviderFamily::Github));
    assert!(
        !contracts
            .iter()
            .find(|contract| contract.family == ProviderFamily::Homebrew)
            .expect("Homebrew contract")
            .fetches_bytes
    );
    assert!(
        !contracts
            .iter()
            .find(|contract| contract.family == ProviderFamily::Github)
            .expect("GitHub contract")
            .fetches_bytes
    );
    assert!(
        contracts
            .iter()
            .find(|contract| contract.family == ProviderFamily::Binary)
            .expect("binary contract")
            .fetches_bytes
    );
}

#[test]
fn npm_metadata_normalizes_deps_scripts_bins() {
    let facts = jetpack::ProviderGraph::normalize_npm(
        r#"{"name":"web","version":"1.0.0","license":"MIT","dependencies":{"vite":"5"},"scripts":{"build":"vite build"},"bin":{"web":"cli.js"}}"#,
    );
    assert_eq!(facts.dependencies, vec!["vite".to_string()]);
    assert_eq!(facts.scripts, vec!["build".to_string()]);
    assert_eq!(facts.bins, vec!["web".to_string()]);
}

#[test]
fn provider_report_round_trips_through_lock_and_explain() {
    use jetpack::ProviderGraph::{normalize_provider_document, ProviderFamily};
    let native = r#"{"name":"web","version":"1.0.0","license":"MIT","dependencies":{"vite":"5"}}"#;
    let report = normalize_provider_document(ProviderFamily::Npm, native);
    report.validate().expect("lossless npm report");
    let shared = report.shared_facts();
    assert_eq!(shared.qualified_reference(), "web#version=1.0.0@npm");
    assert_eq!(shared.native_document, native);
    assert!(shared
        .explain_lines()
        .iter()
        .any(|line| line == "native json: retained"));
    let round_trip = jetpack::ProviderFacts::from_json(&report.export_json())
        .expect("shared provider facts JSON");
    assert_eq!(round_trip, shared);
    let lock = report
        .lock_record("app", "web@npm#1.0.0", "any")
        .expect("provider semantic lock");
    assert_eq!(lock.identity.exact, "web#version=1.0.0@npm");
    let digest = shared.digest();
    assert_eq!(
        lock.future_fields.get("provider-facts-digest"),
        Some(&digest)
    );
}

#[test]
fn provider_report_surfaces_missing_identity_as_loss() {
    use jetpack::ProviderGraph::{normalize_provider_document, ProviderFamily};
    let report = normalize_provider_document(ProviderFamily::Npm, r#"{"name":"web"}"#);
    assert!(!report.is_lossless());
    let error = report.validate().expect_err("missing provider version");
    assert!(
        error.contains("lossy") || error.contains("exact"),
        "{error}"
    );
    let shared = report.shared_facts_for("web@npm");
    assert!(shared
        .losses
        .iter()
        .any(|loss| loss.reason.contains("exact version")));
}

#[test]
fn provider_report_preserves_alias_resolution_and_typed_native_facts() {
    use jetpack::ProviderGraph::{normalize_provider_document, ProviderFamily};
    use jetpack::{ProviderFactValue, ProviderFacts};
    let native = r#"{"name":"web","version":"1.0.0","yanked":false}"#;
    let report = normalize_provider_document(ProviderFamily::Npm, native);
    let shared = report.shared_facts_for("web@catalog");
    shared.validate().expect("catalog alias remains lossless");
    assert_eq!(shared.qualified_reference(), "web#version=1.0.0@catalog");
    assert_eq!(shared.native_document, native);
    assert_eq!(
        shared.facts.get("provider.resolved_selector"),
        Some(&ProviderFactValue::Text("#version=1.0.0".to_string()))
    );
    assert_eq!(
        shared.facts.get("provider.npm.native.yanked"),
        Some(&ProviderFactValue::List(vec![ProviderFactValue::Bool(
            false
        )]))
    );

    let lock = report
        .lock_record("app", "web@catalog", "any")
        .expect("alias lock retains source authority");
    assert_eq!(lock.identity.exact, "web#version=1.0.0@catalog");
    let locked = ProviderFacts::from_json(
        lock.future_fields
            .get("provider-facts")
            .expect("provider facts in lock"),
    )
    .expect("locked provider facts");
    locked.validate().expect("locked alias facts");
    assert_eq!(locked.reference, "web@catalog");
}

#[test]
fn swiftpm_v2_report_keeps_revision_and_native_bytes() {
    use jetpack::ProviderGraph::{normalize_provider_document, ProviderFamily};
    let revision = "0123456789abcdef0123456789abcdef01234567";
    let native = format!(
        "{{\"pins\":[{{\"identity\":\"swift-log\",\"state\":{{\"revision\":\"{revision}\",\"version\":\"1.5.4\"}}}}]}}"
    );
    let report = normalize_provider_document(ProviderFamily::SwiftPM, &native);
    report.validate().expect("SwiftPM v2 pin is lossless");
    let shared = report.shared_facts();
    assert_eq!(
        shared.qualified_reference(),
        format!("swift-log#revision={revision}@swiftpm")
    );
    assert_eq!(shared.native_document, native);
    assert!(shared
        .facts
        .get("provider.revision")
        .is_some_and(|value| format!("{value:?}").contains(revision)));
}

#[test]
fn homebrew_report_reads_stable_version_and_dependencies() {
    use jetpack::ProviderGraph::{normalize_provider_document, ProviderFamily};
    let native = r#"{"name":"jq","versions":{"stable":"1.7.1"},"license":"MIT","dependencies":["oniguruma"]}"#;
    let report = normalize_provider_document(ProviderFamily::Homebrew, native);
    report
        .validate()
        .expect("Homebrew formula identity is lossless");
    assert_eq!(report.facts.version, "1.7.1");
    assert_eq!(report.facts.dependencies, vec!["oniguruma".to_string()]);
    assert_eq!(report.shared_facts().native_document, native);
}

#[test]
fn homebrew_conformance_retains_bottle_source_and_hook_facts() {
    use jetpack::ProviderGraph::{normalize_provider_document, ProviderFamily};
    let native = r##"{"name":"jq","full_name":"jq","version":"1.7.1","versions":{"stable":"1.7.1","head":"HEAD"},"license":"MIT","tap":"homebrew/core","dependencies":["oniguruma"],"build_dependencies":["pkg-config"],"test_dependencies":["bats-core"],"recommended_dependencies":[{"name":"less","version":"1.0"}],"source":{"url":"https://example.invalid/jq.tar.gz","sha256":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"},"bottle":{"stable":{"files":{"arm64_sonoma":{"url":"https://example.invalid/jq.arm64.bottle.tar.gz","sha256":"abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"}}}},"relocatable":true,"test":"system \"#{bin}/jq\", \"--version\"","deprecated":false}"##;
    let report = normalize_provider_document(ProviderFamily::Homebrew, native);
    report.validate().expect("Homebrew formula is lossless");
    assert_eq!(report.facts.dependencies, vec!["oniguruma".to_string()]);
    assert_eq!(
        report.facts.build_dependencies,
        vec!["pkg-config".to_string()]
    );
    assert_eq!(report.facts.dev_dependencies, vec!["bats-core".to_string()]);
    assert!(report
        .facts
        .typed
        .contains_key("provider.homebrew.bottle.arm64_sonoma.sha256"));
    assert!(report
        .shared_facts()
        .facts
        .contains_key("provider.homebrew.hook.test"));
    let shared = report.shared_facts();
    let lock = report
        .lock_record("app", "jq@homebrew#1.7.1", "x86_64-linux")
        .expect("Homebrew provider lock");
    let locked = jetpack::ProviderFacts::from_json(
        lock.future_fields
            .get("provider-facts")
            .expect("provider facts in Homebrew lock"),
    )
    .expect("Homebrew provider facts JSON");
    assert_eq!(locked, shared);
    assert_eq!(locked.native_document, native);
}

#[test]
fn homebrew_conformance_reports_conflicting_identity_facts() {
    use jetpack::ProviderGraph::{normalize_provider_document, ProviderFamily};
    let report = normalize_provider_document(
        ProviderFamily::Homebrew,
        r#"{"name":"jq","version":"1.7.1","versions":{"stable":"1.7.2"}}"#,
    );
    assert!(report
        .conflicts
        .iter()
        .any(|conflict| conflict.contains("conflicting version")));
    assert!(report.validate().is_err());
}

#[test]
fn github_conformance_retains_release_assets_source_and_status() {
    use jetpack::ProviderGraph::{normalize_provider_document, ProviderFamily};
    let digest = "sha256-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let revision = "0123456789abcdef0123456789abcdef01234567";
    let native = format!(
        r#"{{"name":"tool","tag_name":"v1.2.3","target_commitish":"{revision}","license":"MIT","repository":{{"full_name":"acme/tool"}},"draft":false,"prerelease":false,"assets":[{{"name":"tool-linux","platform":"x86_64-linux","digest":"{digest}","browser_download_url":"https://example.invalid/tool"}}],"signature":{{"key":"pk","value":"sig"}},"advisories":["CVE-0000-0000"],"hooks":{{"build":"reviewed"}}}}"#
    );
    let report = normalize_provider_document(ProviderFamily::Github, &native);
    report.validate().expect("GitHub release is lossless");
    assert_eq!(report.facts.version, "v1.2.3");
    assert!(report.facts.platforms.contains(&"x86_64-linux".to_string()));
    assert!(report.facts.typed.contains_key("provider.github.revision"));
    assert!(report
        .shared_facts()
        .facts
        .contains_key("provider.github.asset.tool-linux.browser_download_url"));
    let shared = report.shared_facts();
    let lock = report
        .lock_record("app", "tool@github#v1.2.3", "x86_64-linux")
        .expect("GitHub provider lock");
    let locked = jetpack::ProviderFacts::from_json(
        lock.future_fields
            .get("provider-facts")
            .expect("provider facts in GitHub lock"),
    )
    .expect("GitHub provider facts JSON");
    assert_eq!(locked, shared);
    assert_eq!(locked.native_document, native);
}

#[test]
fn github_conformance_reports_unhashed_release_assets() {
    use jetpack::ProviderGraph::{normalize_provider_document, ProviderFamily};
    let report = normalize_provider_document(
        ProviderFamily::Github,
        r#"{"name":"tool","tag_name":"v1.2.3","assets":[{"name":"tool-linux","browser_download_url":"https://example.invalid/tool"}]}"#,
    );
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.contains("no content digest")));
    assert!(report.validate().is_err());
}

#[test]
fn binary_conformance_retains_platform_signature_and_provenance() {
    use jetpack::ProviderGraph::{normalize_provider_document, ProviderFamily};
    let digest = "sha256-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let native = format!(
        r#"{{"name":"tool","version":"2.0.0","hash":"{digest}","platforms":["x86_64-linux"],"license":"MIT","url":"https://example.invalid/tool","signature":{{"key":"pk","value":"sig"}},"provenance":{{"builder":"ci"}},"sbom":{{"format":"spdx"}},"variants":{{"debug":{{"features":["trace"]}}}}}}"#
    );
    let report = normalize_provider_document(ProviderFamily::Binary, &native);
    report
        .validate()
        .expect("binary provider facts are lossless");
    assert_eq!(report.shared_facts().selector.digest, digest);
    assert!(report
        .shared_facts()
        .facts
        .contains_key("provider.binary.signature"));
    assert!(report
        .shared_facts()
        .facts
        .contains_key("provider.binary.provenance"));
    assert_eq!(report.shared_facts().native_document, native);
}

#[test]
fn binary_conformance_reports_missing_platform_and_weak_hash() {
    use jetpack::ProviderGraph::{normalize_provider_document, ProviderFamily};
    let report = normalize_provider_document(
        ProviderFamily::Binary,
        r#"{"name":"tool","hash":"sha256-aa"}"#,
    );
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.contains("not an exact digest")));
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.contains("no target platform")));
    assert!(report.validate().is_err());
}

#[test]
fn binary_report_uses_digest_identity_without_fabricating_a_version() {
    use jetpack::ProviderGraph::{normalize_provider_document, ProviderFamily};
    let digest = "sha256-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let native = format!("{{\"name\":\"tool\",\"hash\":\"{digest}\",\"platforms\":[\"linux\"]}}");
    let report = normalize_provider_document(ProviderFamily::Binary, &native);
    report
        .validate()
        .expect("binary digest is an exact identity");
    assert_eq!(
        report.shared_facts().qualified_reference(),
        format!("tool#digest={digest}@binary")
    );
    assert_eq!(report.shared_facts().native_document, native);
}

#[test]
fn binary_report_surfaces_an_invalid_digest_as_loss() {
    use jetpack::ProviderGraph::{normalize_provider_document, ProviderFamily};
    let report = normalize_provider_document(
        ProviderFamily::Binary,
        r#"{"name":"tool","hash":"sha256-aa","platforms":["linux"]}"#,
    );
    assert!(!report.is_lossless());
    let shared = report.shared_facts();
    assert!(shared
        .losses
        .iter()
        .any(|loss| loss.reason.contains("exact version, revision, or digest")));
    assert!(report.validate().is_err());
}

#[test]
fn cargo_metadata_normalizes_features_and_build_script_fact() {
    let facts = jetpack::ProviderGraph::normalize_cargo(
        "[package]\nname = \"lib\"\nversion = \"0.1.0\"\nbuild = \"build.rs\"\n[dependencies]\nserde = \"1\"\n[build-dependencies]\ncc = \"1\"\n",
    );
    assert_eq!(facts.dependencies, vec!["serde".to_string()]);
    assert_eq!(facts.build_dependencies, vec!["cc".to_string()]);
    assert_eq!(facts.scripts, vec!["build.rs".to_string()]);
}

#[test]
fn pypi_dynamic_metadata_becomes_todo_fact() {
    let facts = jetpack::ProviderGraph::normalize_pypi("pkg", "1.0.0", true);
    assert!(facts.todos[0].contains("dynamic metadata"));
}

#[test]
fn swiftpm_metadata_locks_exact_revision() {
    let facts = jetpack::ProviderGraph::normalize_swiftpm("swift-log", "abc123");
    assert_eq!(facts.integrity_hash, "abc123");
    assert_eq!(facts.source_identity, "swiftpm:swift-log@abc123");
}

#[test]
fn binary_provider_requires_hash_and_platform() {
    assert!(jetpack::ProviderGraph::binary_object("tool", "", "linux", "").is_err());
    assert!(jetpack::ProviderGraph::binary_object("tool", "sha256-aa", "", "").is_err());
    let obj = jetpack::ProviderGraph::binary_object("tool", "sha256-aa", "linux", "")
        .expect("hash + platform ok");
    assert_eq!(obj.exact_identity, "binary:tool:linux:sha256-aa");
}

#[test]
fn provider_fetch_denied_under_offline_without_lock() {
    use jetpack::ProviderGraph::{AuthorityGraph, FetchDecision, ProviderFamily, ProviderRequest};
    let decision = AuthorityGraph::default().fetch_allowed(&ProviderRequest {
        family: ProviderFamily::Cargo,
        ref_key: "serde".to_string(),
        exact_identity: "cargo:serde@1".to_string(),
        hash: "sha256-aa".to_string(),
        platform: "any".to_string(),
        offline: true,
    });
    assert_eq!(decision, FetchDecision::DeniedOfflineMissingLock);
}

#[test]
fn provider_fetch_allowed_offline_with_satisfied_lock() {
    use jetpack::ProviderGraph::{
        AuthorityGraph, FetchDecision, ProviderFamily, ProviderObject, ProviderRequest,
    };
    let mut graph = AuthorityGraph::default();
    graph.add_locked(ProviderObject {
        family: ProviderFamily::Cargo,
        ref_key: "serde".to_string(),
        exact_identity: "cargo:serde@1".to_string(),
        hash: "sha256-aa".to_string(),
        platform: "any".to_string(),
        signature: String::new(),
        audit: Vec::new(),
        sandbox_effects: Vec::new(),
        build_effects: Vec::new(),
    });
    let decision = graph.fetch_allowed(&ProviderRequest {
        family: ProviderFamily::Cargo,
        ref_key: "serde".to_string(),
        exact_identity: "cargo:serde@1".to_string(),
        hash: "sha256-aa".to_string(),
        platform: "any".to_string(),
        offline: true,
    });
    assert_eq!(decision, FetchDecision::AllowedOfflineSatisfied);
}

#[test]
fn replacement_candidate_visible_but_inactive() {
    use jetpack::ProviderGraph::{normalize_npm, ReplacementOverlay};
    use jetpack::Replacement::{PackageIdentity, ProofStatus};
    let mut facts = normalize_npm(r#"{"name":"left-pad","version":"1.0.0"}"#);
    facts.replacement_candidates.push(ReplacementOverlay {
        foreign_identity: PackageIdentity::new("npm", "left-pad", "1.0.0"),
        native_identity: PackageIdentity::new("core", "core.text.pad", "1.0.0"),
        covered_public_symbols: Vec::new(),
        unsupported_symbols: Vec::new(),
        license: "MIT".to_string(),
        platforms: vec!["x86_64-linux".to_string()],
        proof_status: ProofStatus::Missing,
        proof_digest: String::new(),
        proof_inputs: Vec::new(),
    });
    let record = semantic_record("app", &facts.name, &facts.version);
    assert_eq!(facts.source_identity, "npm:left-pad@1.0.0");
    assert_eq!(record.identity.semantic_key(), "package:left-pad");
    assert_eq!(
        facts.replacement_candidates[0].proof_status,
        ProofStatus::Missing
    );
    assert_eq!(
        facts.replacement_candidates[0].native_identity.ref_string(),
        "core:core.text.pad@1.0.0"
    );
}

#[test]
fn replacement_compat_proof_fails_on_missing_symbol() {
    use jetpack::Replacement::{run_proof, ProofFailureKind};
    let foreign = replacement_surface("npm", "left-pad", "1.0.0");
    let mut native = replacement_surface("core", "core.text.pad", "1.0.0");
    native.public_symbols.retain(|symbol| symbol.name != "trim");
    let report = run_proof(&foreign, &native, "x86_64-linux");
    assert!(report
        .failures
        .iter()
        .any(|f| { f.kind == ProofFailureKind::MissingPublicSymbol && f.name == "trim" }));
}

#[test]
fn replacement_compat_proof_fails_on_effect_mismatch() {
    use jetpack::Replacement::{run_proof, ProofFailureKind};
    let foreign = replacement_surface("npm", "left-pad", "1.0.0");
    let mut native = replacement_surface("core", "core.text.pad", "1.0.0");
    native.public_symbols[0].effects = vec!["fs.read".to_string()];
    let report = run_proof(&foreign, &native, "x86_64-linux");
    assert!(report
        .failures
        .iter()
        .any(|f| { f.kind == ProofFailureKind::EffectMismatch && f.name == "pad_left" }));
}

#[test]
fn replacement_compat_proof_fails_on_error_shape_mismatch() {
    use jetpack::Replacement::{run_proof, ProofFailureKind};
    let foreign = replacement_surface("npm", "left-pad", "1.0.0");
    let mut native = replacement_surface("core", "core.text.pad", "1.0.0");
    native.public_symbols[0].errors = vec!["RangeError".to_string()];
    let report = run_proof(&foreign, &native, "x86_64-linux");
    assert!(report
        .failures
        .iter()
        .any(|f| { f.kind == ProofFailureKind::ErrorShapeMismatch && f.name == "pad_left" }));
}

#[test]
fn replacement_compat_proof_fails_on_golden_output_diff() {
    use jetpack::Replacement::{run_proof, GoldenFixture, ProofFailureKind};
    let foreign = replacement_surface("npm", "left-pad", "1.0.0");
    let mut native = replacement_surface("core", "core.text.pad", "1.0.0");
    native.goldens = vec![GoldenFixture::new("pad_left_basic", "hi\n")];
    let report = run_proof(&foreign, &native, "x86_64-linux");
    assert!(report
        .failures
        .iter()
        .any(|f| { f.kind == ProofFailureKind::GoldenOutputDiff && f.name == "pad_left_basic" }));
}

#[test]
fn replacement_compat_proof_pass_enables_policy_replacement() {
    use jetpack::Replacement::{resolve_replacement, ReplacementDecision, ReplacementPolicy};
    let candidate = replacement_passed_candidate();
    let policy = ReplacementPolicy::allow(
        &candidate.foreign_identity,
        &candidate.native_identity,
        "app",
    );
    let decision = resolve_replacement(&candidate, &policy, "app", "x86_64-linux");
    let ReplacementDecision::Active(active) = decision else {
        panic!("passed proof + allow policy should activate replacement");
    };
    assert_eq!(
        active.native_identity.ref_string(),
        "core:core.text.pad@1.0.0"
    );
    assert_eq!(
        active.lock_record.identity.kind.as_str(),
        "replacement-overlay"
    );
}

#[test]
fn replacement_policy_deny_blocks_replacement() {
    use jetpack::Replacement::{resolve_replacement, ReplacementDecision, ReplacementPolicy};
    let candidate = replacement_passed_candidate();
    let decision = resolve_replacement(
        &candidate,
        &ReplacementPolicy::default(),
        "app",
        "x86_64-linux",
    );
    assert!(matches!(decision, ReplacementDecision::Denied { .. }));
}

#[test]
fn replacement_preserves_foreign_call_site() {
    use jetpack::Replacement::{resolve_replacement, ReplacementDecision, ReplacementPolicy};
    let candidate = replacement_passed_candidate();
    let policy = ReplacementPolicy::prefer(
        &candidate.foreign_identity,
        &candidate.native_identity,
        "app",
    );
    let decision = resolve_replacement(&candidate, &policy, "app", "x86_64-linux");
    let ReplacementDecision::Active(active) = decision else {
        panic!("prefer policy should activate replacement");
    };
    assert_eq!(active.foreign_call_site, "npm:left-pad@1.0.0");
    assert_eq!(
        active.lock_record.identity.key,
        "npm:left-pad@1.0.0@x86_64-linux"
    );
    assert_eq!(
        active.lock_record.identity.exact,
        "core:core.text.pad@1.0.0"
    );
}

#[test]
fn replacement_lock_records_foreign_native_proof_and_policy() {
    let candidate = replacement_passed_candidate();
    let record = jetpack::Replacement::replacement_lock_record(
        &candidate,
        "app",
        "x86_64-linux",
        "policy.replacements:npm:left-pad@1.0.0=>core:core.text.pad@1.0.0:allow",
    );
    assert_eq!(
        record
            .future_fields
            .get("replacement-foreign")
            .map(String::as_str),
        Some("npm:left-pad@1.0.0")
    );
    assert_eq!(
        record
            .future_fields
            .get("replacement-native")
            .map(String::as_str),
        Some("core:core.text.pad@1.0.0")
    );
    assert_eq!(
        record
            .future_fields
            .get("replacement-proof-digest")
            .map(String::as_str),
        Some(candidate.proof_digest.as_str())
    );
    assert_eq!(
        record
            .future_fields
            .get("replacement-proof-inputs")
            .map(|s| s.contains("platform=x86_64-linux")),
        Some(true)
    );
    assert!(record.rationales[0]
        .policy_fingerprint
        .contains("policy.replacements"));
}

#[test]
fn replacement_lock_merge_conflict_names_owners() {
    use jetpack::Replacement::{
        replacement_lock_record, PackageIdentity, ProofStatus, ReplacementCandidate,
    };
    use jetpack::SemanticLock::{merge, SemanticLockFile};
    let foreign = replacement_identity("npm", "left-pad", "1.0.0");
    let mk_candidate = |native: &str| ReplacementCandidate {
        foreign_identity: foreign.clone(),
        native_identity: PackageIdentity::new("core", native, "1.0.0"),
        covered_public_symbols: vec!["pad_left".to_string()],
        unsupported_symbols: Vec::new(),
        license: "MIT".to_string(),
        platforms: vec!["x86_64-linux".to_string()],
        proof_status: ProofStatus::Passed,
        proof_digest: format!("proof-{native}"),
        proof_inputs: vec![format!("platform=x86_64-linux;native={native}")],
    };
    let left = SemanticLockFile {
        records: vec![replacement_lock_record(
            &mk_candidate("core.text.pad"),
            "app",
            "x86_64-linux",
            "policy-left",
        )],
        ..Default::default()
    };
    let right = SemanticLockFile {
        records: vec![replacement_lock_record(
            &mk_candidate("core.string.pad"),
            "app",
            "x86_64-linux",
            "policy-right",
        )],
        ..Default::default()
    };
    let out = merge(&SemanticLockFile::default(), &left, &right);
    assert_eq!(out.conflicts.len(), 1);
    assert_eq!(out.conflicts[0].owner_package, "app");
    assert_eq!(
        out.conflicts[0].semantic_key,
        "replacement-overlay:npm:left-pad@1.0.0@x86_64-linux"
    );
}

#[test]
fn replacement_lock_keys_are_platform_specific() {
    let candidate = replacement_passed_candidate();
    let linux =
        jetpack::Replacement::replacement_lock_record(&candidate, "app", "x86_64-linux", "policy");
    let macos = jetpack::Replacement::replacement_lock_record(
        &candidate,
        "app",
        "aarch64-apple-darwin",
        "policy",
    );
    assert_ne!(linux.identity.semantic_key(), macos.identity.semantic_key());
}

#[test]
fn replacement_importer_reports_replacement_progress() {
    use jetpack::Replacement::{ImporterProgressFact, ImporterReplacementStatus};
    let candidate = replacement_passed_candidate();
    let proof = ImporterProgressFact::from_candidate(&candidate);
    let active = ImporterProgressFact::active(&candidate);
    assert_eq!(proof.status, ImporterReplacementStatus::ProofPassed);
    assert_eq!(active.status, ImporterReplacementStatus::ReplacementActive);
    assert_eq!(active.proof_digest, candidate.proof_digest);
}

// ─────────────────────────────────────────────
// Manifest parsing
// ─────────────────────────────────────────────

#[test]
fn manifest_parse_valid_fields() {
    let raw = r#"name:    "myapp"
version: "1.2.3"
jet:     ">=0.1.0"
description: "A test package"
license: "MIT OR Apache-2.0"
repository: "https://example.com"
deps: .{
}
"#;
    let path = PathBuf::from("package.jet");
    let mf = jet::Manifest::parse(&path, raw).expect("valid manifest should parse");
    assert_eq!(mf.package.name, "myapp");
    assert_eq!(mf.package.version, "1.2.3");
    assert_eq!(mf.package.jet_constraint.as_deref(), Some(">=0.1.0"));
    assert_eq!(mf.package.description.as_deref(), Some("A test package"));
    assert_eq!(mf.package.license.as_deref(), Some("MIT OR Apache-2.0"));
    assert!(mf.dependencies.is_empty());
    assert!(mf.boundaries.is_empty());
}

#[test]
fn manifest_parse_import_boundaries() {
    let raw = min_manifest("app", "0.1.0")
        + r#"
boundaries: {
    deny: [
        { from: "app.ui", to: "app.db" },
        { from: "app.api.*", to: "app.db.*" },
    ],
}
"#;
    let mf = jet::Manifest::parse(&PathBuf::from("package.jet"), &raw)
        .expect("boundary policy should parse");
    assert_eq!(mf.boundaries.len(), 2);
    assert_eq!(mf.boundaries[0].from, "app.ui");
    assert_eq!(mf.boundaries[0].to, "app.db");
    assert!(mf.boundaries[1].matches("app.api.auth", "app.db.users"));
    assert!(!mf.boundaries[1].matches("app.web", "app.db.users"));
}

#[test]
fn manifest_parse_import_boundaries_rejects_malformed_patterns() {
    for (label, pattern) in [
        ("unquoted", "app.ui"),
        ("interior wildcard", "app.*.ui"),
        ("multiple wildcards", "app.**"),
    ] {
        let raw = min_manifest("app", "0.1.0")
            + &format!("boundaries: {{ deny: [{{ from: {pattern}, to: \"app.db\" }}] }}\n");
        let error = jet::Manifest::parse(&PathBuf::from("package.jet"), &raw)
            .expect_err("malformed boundary pattern must use E1206");
        assert_eq!(error.code, "E1206", "{label}");
        assert!(
            error.what.contains("package.jet"),
            "{label}: {}",
            error.what
        );
    }

    for (label, block) in [
        (
            "unknown boundary field",
            "boundaries: { allow: [{ from: \"app.ui\", to: \"app.db\" }] }",
        ),
        ("missing from", "boundaries: { deny: [{ to: \"app.db\" }] }"),
        ("missing to", "boundaries: { deny: [{ from: \"app.ui\" }] }"),
        (
            "duplicate edge field",
            "boundaries: { deny: [{ from: \"app.ui\", from: \"app.api\", to: \"app.db\" }] }",
        ),
    ] {
        let raw = min_manifest("app", "0.1.0") + block + "\n";
        let error = jet::Manifest::parse(&PathBuf::from("package.jet"), &raw)
            .expect_err("malformed boundary record must use E1206");
        assert_eq!(error.code, "E1206", "{label}");
    }
}

#[test]
fn loader_enforces_import_boundaries_and_warns_on_zero_match() {
    let denied = tmp_dir("import_boundary_denied");
    write(
        &denied,
        "package.jet",
        &(min_manifest("app", "0.1.0")
            + "boundaries: { deny: [{ from: \"app.ui\", to: \"app.db\" }] }\n"),
    );
    write(&denied, "ui.jet", "use db;\nfn run() -[IO]> { }\n");
    write(&denied, "db.jet", "pub fn value() Int -> 1\n");
    let error = jet::Loader::load_entry(denied.join("ui.jet").to_str().unwrap())
        .expect_err("a denied import edge must fail during loading");
    assert_eq!(first_diag_code(&error), "E0619");
    assert!(error[0].what.contains("app.ui"));
    assert!(error[0].what.contains("app.db"));
    fs::remove_dir_all(&denied).unwrap();

    let unused = tmp_dir("import_boundary_unused");
    write(
        &unused,
        "package.jet",
        &(min_manifest("app", "0.1.0")
            + "boundaries: { deny: [{ from: \"app.ui\", to: \"app.db\" }] }\n"),
    );
    write(&unused, "ui.jet", "fn run() { }\n");
    let bundle = jet::Loader::load_entry(unused.join("ui.jet").to_str().unwrap())
        .expect("an unmatched boundary is a warning");
    assert!(bundle
        .parse_teaching
        .iter()
        .any(|diagnostic| diagnostic.code == "L0619"));
    fs::remove_dir_all(&unused).unwrap();
}

#[test]
fn loader_records_import_edge_facts_and_erases_boundary_policy_before_codegen() {
    let root = tmp_dir("import_boundary_structure_fact");
    write(
        &root,
        "package.jet",
        &(min_manifest("app", "0.1.0")
            + "boundaries: { deny: [{ from: \"app.ui\", to: \"app.hidden.*\" }] }\n"),
    );
    write(
        &root,
        "ui.jet",
        "use db;\nfn run() -[IO]> { print(db.value()) }\n",
    );
    write(&root, "db.jet", "pub fn value() Int -> 1\n");
    let entry = root.join("ui.jet");
    let shown = entry.to_str().unwrap();
    let mut bundle = jet::Loader::load_entry(shown).expect("allowed edge must load");
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Lint),
        "allowed boundary edge diagnostics: {diagnostics:?}"
    );
    let edge = bundle
        .name_ledger
        .structure_facts()
        .iter()
        .find(|fact| fact.kind == jet_foundation::Names::StructureFactKind::ImportEdge)
        .expect("allowed edge structure fact");
    assert_eq!(edge.subject, "app.ui -> app.db");
    assert_eq!(edge.status, "allowed");
    assert_eq!(edge.gate.as_deref(), Some("manifest rule edit"));

    let gates =
        jet::Sema::GateLedger::GateLedger::collect(&bundle, jet::Policy::GateSet::default());
    assert!(gates.entries().iter().any(|entry| {
        entry.kind == jet::Sema::GateLedger::GateKind::Structure
            && entry.scope == "import-edge"
            && entry.subject == "app.ui -> app.db"
    }));

    let inspected = Command::new(jet_bin())
        .current_dir(&root)
        .args(["inspect", "structure", shown])
        .env("NO_COLOR", "1")
        .output()
        .expect("inspect structure must run");
    assert!(inspected.status.success(), "inspect failed: {inspected:?}");
    let inspected = String::from_utf8(inspected.stdout).expect("inspection is UTF-8");
    assert!(inspected.contains("Structure.ImportEdge"));
    assert!(inspected.contains("app.ui -> app.db"));
    assert!(inspected.contains("manifest rule edit"));

    let output = jet::compile_with_path("", shown).expect("allowed edge must codegen");
    assert!(!output.rust.contains("manifest rule edit"));
    assert!(!output.rust.contains("app.hidden.*"));

    for (label, args) in [
        ("aot", vec!["run", "--release", shown]),
        ("jit", vec!["run", shown]),
        ("interpreter", vec!["run", "--interpret", shown]),
        ("dev", vec!["dev", shown, "--watch=off"]),
    ] {
        let output = Command::new(jet_bin())
            .current_dir(&root)
            .args(args)
            .env("NO_COLOR", "1")
            .env(
                "JET_RUN_CACHE_DIR",
                root.join(".cache").join(label).join("run"),
            )
            .env(
                "JET_CACHE_DIR",
                root.join(".cache").join(label).join("build"),
            )
            .output()
            .expect("tier command must run");
        assert!(
            output.status.success(),
            "{label} failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"1\n", "{label} output");
    }

    let wasm = Command::new("rustc")
        .args([
            "--print",
            "target-libdir",
            "--target",
            "wasm32-unknown-unknown",
        ])
        .output()
        .expect("probe web target");
    if wasm.status.success() {
        let output = Command::new(jet_bin())
            .current_dir(&root)
            .args(["build", "--target=web", shown])
            .env("NO_COLOR", "1")
            .env("JET_CACHE_DIR", root.join(".cache").join("web"))
            .output()
            .expect("web command must run");
        assert!(
            output.status.success(),
            "web failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    } else {
        eprintln!("note: skipping import-boundary web tier proof (wasm target unavailable)");
    }
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn denied_import_boundary_fact_reaches_structure_inspection() {
    let root = tmp_dir("import_boundary_denied_structure_fact");
    write(
        &root,
        "package.jet",
        &(min_manifest("app", "0.1.0")
            + "boundaries: { deny: [{ from: \"app.ui\", to: \"app.db\" }] }\n"),
    );
    write(&root, "ui.jet", "use db;\nfn run() { }\n");
    write(&root, "db.jet", "pub fn value() Int -> 1\n");
    let entry = root.join("ui.jet");
    let shown = entry.to_str().unwrap();

    let diagnostics = jet::Loader::load_entry_with_diagnostics(shown)
        .expect_err("denied edge must retain its loader fact");
    let fact = diagnostics
        .iter()
        .find_map(|entry| entry.structure_fact.as_ref())
        .expect("denied edge structure fact");
    assert_eq!(fact.subject, "app.ui -> app.db");
    assert_eq!(fact.status, "denied");
    assert_eq!(fact.gate.as_deref(), Some("manifest rule edit"));

    let inspected = Command::new(jet_bin())
        .current_dir(&root)
        .args(["inspect", "structure", shown])
        .env("NO_COLOR", "1")
        .output()
        .expect("inspect structure must run");
    assert!(!inspected.status.success(), "denied inspection must fail");
    let stdout = String::from_utf8(inspected.stdout).expect("inspection is UTF-8");
    assert!(stdout.contains("Structure.ImportEdge"));
    assert!(stdout.contains("app.ui -> app.db"));
    assert!(stdout.contains("denied"));
    assert!(stdout.contains("manifest rule edit"));
    let stderr = String::from_utf8(inspected.stderr).expect("diagnostics are UTF-8");
    assert!(
        stderr.contains("E0619"),
        "missing denied-edge diagnostic: {stderr}"
    );

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn manifest_parse_dep_path() {
    let raw = manifest_with_deps("root", "0.1.0", "    helpers: ../helpers,");
    let mf =
        jet::Manifest::parse(&PathBuf::from("package.jet"), &raw).expect("path dep should parse");
    let dep = mf.dependencies.get("helpers").expect("missing helpers dep");
    assert!(matches!(dep, jet::Manifest::DepSpec::Path { path } if path == "../helpers"));
}

#[test]
fn manifest_rejects_traversal_dependency_name() {
    let raw = manifest_with_deps("root", "0.1.0", "    ../escape: \"1.0.0\",");
    let error = jet::Manifest::parse(&PathBuf::from("package.jet"), &raw)
        .expect_err("dependency names must not become path components");
    assert_eq!(error.code, "E1206", "{} {}", error.code, error.what);
}

#[test]
fn manifest_parse_dep_git_tag() {
    let raw = manifest_with_deps(
        "root",
        "0.1.0",
        "    parsekit: { git: \"https://github.com/acme/parsekit\", tag: \"v0.4.1\" },",
    );
    let mf = jet::Manifest::parse(&PathBuf::from("package.jet"), &raw)
        .expect("git tag dep should parse");
    let dep = mf.dependencies.get("parsekit").expect("missing parsekit");
    assert!(matches!(
        dep,
        jet::Manifest::DepSpec::Git {
            url,
            selector: jet::Manifest::GitSelector::Tag(t)
        } if url.contains("parsekit") && t == "v0.4.1"
    ));
}

#[test]
fn manifest_parse_foreign_package_dep() {
    let raw = manifest_with_deps(
        "root",
        "0.1.0",
        "    plotly: js@\"plotly#version=2.35.0@npm\",",
    );
    let mf = jet::Manifest::parse(&PathBuf::from("package.jet"), &raw)
        .expect("foreign package dep should parse");
    assert!(matches!(
        mf.dependencies.get("plotly"),
        Some(jet::Manifest::DepSpec::Foreign {
            language: jet::AST::ForeignLanguage::JS,
            reference,
        }) if reference == "plotly#version=2.35.0@npm"
    ));
}

#[test]
fn foreign_package_provider_fetch_lock_and_locked_round_trip() {
    let _guard = STORE_LOCK.lock().unwrap();
    let project = Scratch::new("foreign_provider_project");
    let stale_project = Scratch::new("foreign_provider_stale");
    let failed_project = Scratch::new("foreign_provider_failure");
    let fixtures = Scratch::new("foreign_provider_fixtures");
    let store = Scratch::new("foreign_provider_store");
    let artifact =
        fixtures.join("foreign/npm/plotly-plotly_version_2_35_0/.jet/bindings/js/plotly.jet");
    fs::create_dir_all(artifact.parent().unwrap()).unwrap();
    let descriptor = jet::AST::binder_descriptor(jet::AST::ForeignLanguage::JS)
        .unwrap()
        .stamp();
    fs::write(
        &artifact,
        format!(
            "// jet-ffi-descriptor={descriptor}\npub fn scatter() Int -> {{\n    return 7\n}}\n"
        ),
    )
    .unwrap();
    let manifest_text = manifest_with_deps(
        "consumer",
        "0.1.0",
        "    plotly: js@\"plotly#version=2.35.0@npm\",",
    );
    // Keep the provider fixture identity and the package declaration identical
    // in both success and fail-closed cases.
    for root in [&project.path, &failed_project.path] {
        fs::write(root.join("package.jet"), &manifest_text).unwrap();
    }
    let main = project.join("main.jet");
    fs::write(
        &main,
        "use js.plotly as plot\nfn run() {\n    print(plot.scatter())\n}\n",
    )
    .unwrap();
    let manifest = jet::Manifest::parse(&project.join("package.jet"), &manifest_text).unwrap();

    let old_root = std::env::var_os("JETPACK_ROOT");
    let old_fixtures = std::env::var_os("JETPACK_FIXTURES");
    std::env::set_var("JETPACK_ROOT", &store.path);
    std::env::set_var("JETPACK_FIXTURES", &fixtures.path);
    fs::write(stale_project.join("package.jet"), &manifest_text).unwrap();
    fs::write(
        &artifact,
        "// jet-ffi-descriptor=stale\npub fn scatter() Int -> {\n    return 7\n}\n",
    )
    .unwrap();
    let stale_manifest =
        jet::Manifest::parse(&stale_project.join("package.jet"), &manifest_text).unwrap();
    let stale = jet::Fetch::fetch(
        &stale_project.path,
        &stale_manifest,
        None,
        &jet::Fetch::FetchOptions {
            locked: false,
            update: false,
            update_dep: None,
            resolution: jet::Publish::ResolveMode::Conservative,
        },
    )
    .expect_err("stale provider binding must fail before ingestion");
    assert!(stale.iter().any(|diagnostic| diagnostic.code == "E1256"));
    fs::write(
        &artifact,
        format!(
            "// jet-ffi-descriptor={descriptor}\npub fn scatter() Int -> {{\n    return 7\n}}\n"
        ),
    )
    .unwrap();
    let fetched = jet::Fetch::fetch(
        &project.path,
        &manifest,
        None,
        &jet::Fetch::FetchOptions {
            locked: false,
            update: false,
            update_dep: None,
            resolution: jet::Publish::ResolveMode::Conservative,
        },
    );
    let (lock, dep_dirs) = fetched.expect("foreign provider fetch should realize the package");
    assert!(dep_dirs.get("plotly").is_some_and(|path| path.is_dir()));
    assert!(jetpack::Foreign::project_binding_path(
        &project.path,
        jet::AST::ForeignLanguage::JS,
        "plotly"
    )
    .is_file());
    let foreign = lock
        .packages
        .iter()
        .find(|package| package.name == "plotly")
        .expect("foreign package must be in lock");
    assert!(matches!(
        &foreign.source,
        jet::Lock::LockSource::Foreign {
            language: jet::AST::ForeignLanguage::JS,
            reference,
            output,
        } if reference == "plotly#version=2.35.0@npm" && !output.is_empty()
    ));
    assert!(foreign
        .envelope
        .as_ref()
        .is_some_and(|envelope| !envelope.output_hash.is_empty()));
    let lock_text = fs::read_to_string(project.join(".jet/lock")).unwrap();
    let round_trip = jet::Lock::parse(&lock_text).expect("foreign lock should parse back");
    assert_eq!(round_trip.packages, lock.packages);
    let source = fs::read_to_string(&main).unwrap();
    jet::compile_with_path(&source, &main.to_string_lossy())
        .expect("compiler must consume the provider-projected binding");

    let locked = jet::Fetch::fetch(
        &project.path,
        &manifest,
        Some(&lock),
        &jet::Fetch::FetchOptions {
            locked: true,
            update: false,
            update_dep: None,
            resolution: jet::Publish::ResolveMode::Conservative,
        },
    )
    .expect("locked foreign provider round-trip should verify");
    assert_eq!(locked.0.packages, lock.packages);

    fs::remove_dir_all(fixtures.join("foreign/npm")).unwrap();
    let failed_manifest =
        jet::Manifest::parse(&failed_project.join("package.jet"), &manifest_text).unwrap();
    let failure = jet::Fetch::fetch(
        &failed_project.path,
        &failed_manifest,
        None,
        &jet::Fetch::FetchOptions {
            locked: false,
            update: false,
            update_dep: None,
            resolution: jet::Publish::ResolveMode::Conservative,
        },
    )
    .expect_err("missing provider artifact must fail before foreign fetch succeeds");
    assert!(failure.iter().any(|diagnostic| diagnostic.code == "E1256"));
    assert!(!failed_project.join(".jet/lock").exists());

    match old_root {
        Some(value) => std::env::set_var("JETPACK_ROOT", value),
        None => std::env::remove_var("JETPACK_ROOT"),
    }
    match old_fixtures {
        Some(value) => std::env::set_var("JETPACK_FIXTURES", value),
        None => std::env::remove_var("JETPACK_FIXTURES"),
    }
}

#[test]
fn manifest_parse_e1206_missing_required_field() {
    // No `name:` at all is a shape error (E1206, D-CONF-NAME1: bare `name`/
    // `version`, `version:` alone is optional).
    let raw = "version: \"0.1.0\"\n";
    let err = jet::Manifest::parse(&PathBuf::from("package.jet"), raw)
        .expect_err("missing name should fail");
    assert_eq!(err.code, "E1206");
}

#[test]
fn manifest_parse_e1206_unknown_field() {
    // The retired `payload:` wrapper is now a normal unknown-field error
    // (D-CONF-PLANE1/D-CONF-NAME1).
    let raw = include_str!("ui/manifest_unknown_field/package.jet");
    let err = jet::Manifest::parse(&PathBuf::from("package.jet"), raw)
        .expect_err("payload: wrapper should fail");
    assert_eq!(err.code, "E1206");
    assert!(err.what.contains("payload"));
}

#[test]
fn manifest_parse_e1209_reserved_nonempty() {
    let raw = min_manifest("myapp", "0.1.0") + "\ndev_deps: {\n    testlib: ../testlib,\n}\n";
    let err = jet::Manifest::parse(&PathBuf::from("package.jet"), &raw)
        .expect_err("non-empty dev_deps should fail E1209");
    assert_eq!(err.code, "E1209");
}

// ─────────────────────────────────────────────
// D-EFFBUDGET1: package effect budget manifest parsing
// ─────────────────────────────────────────────

#[test]
fn manifest_parse_effects_block() {
    let raw = min_manifest("app", "0.1.0")
        + "\nauthority: .{\n    holds: { allow: [FS, Time], deny: [Net, Panic] },\n}\n";
    let pm =
        jetpack::Package::PackageFacts::parse(&raw, "test").expect("authority holds should parse");
    assert!(pm.effects_enabled);
    assert_eq!(
        pm.effects_allow,
        Some(vec!["FS".to_string(), "Time".to_string()])
    );
    assert_eq!(
        pm.effects_deny,
        Some(vec!["Net".to_string(), "Panic".to_string()])
    );
}

#[test]
fn manifest_panic_budget_names_the_dependency_stop_site() {
    let raw = min_manifest("app", "0.1.0") + "\nauthority: .{ holds: { deny: [Panic] } }\n";
    let manifest = jetpack::Package::PackageFacts::parse(&raw, "test")
        .expect("Panic should be a manifest effect root");
    let entries = [jetpack::EffectBudget::PackageEffects {
        name: "panicdep".to_string(),
        effects: jet::Sema::EffectSet::from(["Panic".to_string()]),
        panic_sites: vec!["panicdep::parse_port".to_string()],
        boundary_span: Some(jet::Diagnostics::Span::new(4, 12)),
    }];
    let diagnostics = jetpack::EffectBudget::enforce(&entries, &manifest);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, "E1220");
    assert!(diagnostics[0].what.contains("panicdep"));
    assert!(diagnostics[0].what.contains("parse_port"));
    assert!(diagnostics[0].fix.contains("fallible result"));
    assert!(diagnostics[0].fix.contains("#Pre"));
    assert_eq!(
        diagnostics[0].span,
        Some(jet::Diagnostics::Span::new(4, 12))
    );
}

#[test]
fn manifest_parse_grants_block() {
    let raw = min_manifest("app", "0.1.0") + "\nauthority: .{ grants: { \"pdf-lib\": [Net] } }\n";
    let pm =
        jetpack::Package::PackageFacts::parse(&raw, "test").expect("grants block should parse");
    assert_eq!(
        pm.grants,
        vec![("pdf-lib".to_string(), vec!["Net".to_string()])]
    );
}

#[test]
fn manifest_parse_authority_block_holds_grants_trust_and_providers() {
    let raw = min_manifest("app", "0.1.0")
        + r#"
authority: .{
    holds: { allow: [Net, DB.Read], deny: [Exec] },
    grants: { "image-codec": [FS.Read] },
    trust: { default: prompt, ci: { prompt: deny }, services: { stripe: allow } },
    providers: { nix: { registry: "nixpkgs", deny: ["openssl-1.0"] } },
}
"#;
    let pm = jetpack::Package::PackageFacts::parse(&raw, "test")
        .expect("one authority block should parse");
    assert_eq!(
        pm.authority.holds.allow,
        Some(vec!["Net".to_string(), "DB.Read".to_string()])
    );
    assert_eq!(pm.authority.holds.deny, Some(vec!["Exec".to_string()]));
    assert_eq!(
        pm.authority.grants,
        vec![("image-codec".to_string(), vec!["FS.Read".to_string()])]
    );
    assert_eq!(pm.grants, pm.authority.grants);
    assert_eq!(pm.effects_allow, pm.authority.holds.allow);
    assert_eq!(pm.effects_deny, pm.authority.holds.deny);
    assert_eq!(
        pm.authority.trust.as_ref().and_then(|trust| trust.default),
        Some(jetpack::Package::TrustDecision::Prompt)
    );
    assert_eq!(pm.authority.providers.len(), 1);
    assert_eq!(pm.authority.providers[0].provider, "nix");
    assert_eq!(pm.authority.providers[0].registry, "nixpkgs");
    assert_eq!(
        pm.authority.providers[0].deny,
        vec!["openssl-1.0".to_string()]
    );
}

#[test]
fn manifest_parse_authority_trust_block() {
    let raw = min_manifest("app", "0.1.0")
        + "\nauthority: .{ trust: { default: prompt, ci: { prompt: deny }, services: { postgres: prompt }, require: attested } }\n";
    let pm = jetpack::Package::PackageFacts::parse(&raw, "test")
        .expect("authority.trust block should parse");
    let policy = pm
        .authority
        .trust
        .expect("authority trust policy should be stored");
    assert_eq!(
        policy.default,
        Some(jetpack::Package::TrustDecision::Prompt)
    );
    assert_eq!(
        policy.ci_prompt,
        Some(jetpack::Package::TrustDecision::Deny)
    );
    assert_eq!(
        policy.services,
        vec![(
            "postgres".to_string(),
            jetpack::Package::TrustDecision::Prompt
        )]
    );
    assert_eq!(
        policy.require,
        Some(jetpack::Package::ProvenanceRequirement::Attested)
    );
}

#[test]
fn manifest_authority_trust_rejects_unknown_decision() {
    let raw = min_manifest("app", "0.1.0") + "\nauthority: .{ trust: { default: maybe } }\n";
    let err = jetpack::Package::PackageFacts::parse(&raw, "test")
        .expect_err("unknown trust decision should fail");
    assert!(matches!(
        err,
        jetpack::Package::PackageParseError::BadEffectsBlock(_)
    ));
}

#[test]
fn manifest_no_effects_block_disables_enforcement() {
    let raw = min_manifest("app", "0.1.0");
    let pm =
        jetpack::Package::PackageFacts::parse(&raw, "test").expect("valid manifest should parse");
    assert!(!pm.effects_enabled);
    assert_eq!(pm.effects_allow, None);
}

#[test]
fn manifest_parse_effects_e1221_unknown_effect() {
    let raw = min_manifest("app", "0.1.0") + "\nauthority: .{ holds: { allow: [NotAnEffect] } }\n";
    let err = jetpack::Package::PackageFacts::parse(&raw, "test")
        .expect_err("unknown effect name should fail E1221");
    let diag = jet::Manifest::parse(&PathBuf::from("package.jet"), &raw)
        .expect_err("should surface through Manifest::parse too");
    assert_eq!(diag.code, "E1221");
    assert!(matches!(
        err,
        jetpack::Package::PackageParseError::BadEffectsBlock { .. }
    ));
}

#[test]
fn manifest_parse_effects_e1221_unknown_field() {
    let raw = min_manifest("app", "0.1.0") + "\nauthority: .{ holds: { nope: [FS] } }\n";
    let diag = jet::Manifest::parse(&PathBuf::from("package.jet"), &raw)
        .expect_err("unknown effects field should fail E1221");
    assert_eq!(diag.code, "E1221");
}

#[test]
fn effect_budget_load_ok_reports_via_compile_with_path() {
    // A project with a well-formed `authority.holds` block and no dependencies
    // reaching a disallowed effect should compile fine end to end — the
    // manifest gate (Loader/Manifest::parse) never rejects a valid budget.
    let tmp = tmp_dir("effbudget_ok");
    write(
        &tmp,
        "package.jet",
        &(min_manifest("app", "0.1.0") + "\nauthority: .{ holds: { allow: [IO] } }\n"),
    );
    let entry = tmp.join("run.jet");
    fs::write(&entry, "fn run() { print(\"hi\"); }\n").unwrap();

    let result = jet::compile_with_path("", &entry.to_string_lossy());
    assert!(
        result.is_ok(),
        "a well-formed authority.holds budget should not block compilation:\n{}",
        result
            .err()
            .map(|d| jet::render_diagnostics("run.jet", "", &d))
            .unwrap_or_default()
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn cli_build_prints_effect_summary() {
    // D-EFFBUDGET1: every `jet build` prints a one-line effect summary, with
    // zero config — no package.jet needed.
    if !jet_bin().is_file() {
        eprintln!("note: skipping cli_build_prints_effect_summary (run `cargo build` first)");
        return;
    }
    let tmp = tmp_dir("effbudget_summary");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();
    write(
        &tmp,
        "hello.jet",
        "use core.files as fs\nfn run() { fs.write(\"/tmp/jet_effbudget_test.txt\", \"x\") ?? panic(\"e\"); }\n",
    );

    let out = jet_cmd(&["build", "hello.jet"], &tmp, &store);
    assert!(
        out.status.success(),
        "jet build failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // The summary goes to stderr so program/tool stdout stays clean
    // (U7 / D-DEVMODE1 byte-identity).
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("effects:") && stderr.contains("FS"),
        "expected an effect summary naming FS on stderr, got:\n{stderr}"
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn cli_build_enforces_effect_budget_e1220() {
    // A dependency that reaches `Net` while the root's budget only allows
    // `FS` must fail the build naming the dependency (E1220).
    if !jet_bin().is_file() {
        eprintln!(
            "note: skipping cli_build_enforces_effect_budget_e1220 (run `cargo build` first)"
        );
        return;
    }
    let tmp = tmp_dir("effbudget_deny");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();

    write(&tmp, "netdep/package.jet", &min_manifest("netdep", "0.1.0"));
    write(
        &tmp,
        "netdep/netdep.jet",
        "use core.net as net\npub fn ping() { net.tcp_connect(\"127.0.0.1:1\") ?? panic(\"e\"); }\n",
    );

    write(
        &tmp,
        "package.jet",
        &(manifest_with_deps("app", "0.1.0", "    netdep: ./netdep,")
            + "\nauthority: .{ holds: { allow: [FS] } }\n"),
    );
    write(
        &tmp,
        "run.jet",
        "use netdep;\nfn run() { netdep.ping(); }\n",
    );

    let out = jet_cmd(&["build", "run.jet"], &tmp, &store);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "expected build to fail on an out-of-budget effect, stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("E1220") && stderr.contains("netdep"),
        "expected E1220 naming `netdep`, got:\n{stderr}"
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn cli_build_rejects_undeclared_effect_budget_leaf() {
    if !jet_bin().is_file() {
        eprintln!(
            "note: skipping cli_build_rejects_undeclared_effect_budget_leaf (run `cargo build` first)"
        );
        return;
    }
    let tmp = tmp_dir("effbudget_leaf_typo");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();
    write(
        &tmp,
        "package.jet",
        &(min_manifest("app", "0.1.0") + "\nauthority: .{ holds: { allow: [FS.Raed] } }\n"),
    );
    write(&tmp, "run.jet", "fn run() {}\n");

    let out = jet_cmd(&["build", "run.jet"], &tmp, &store);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{stderr}");
    assert!(
        stderr.contains("E0750") && stderr.contains("FS.Read"),
        "budget typo should resolve against Prelude effect leaves:\n{stderr}"
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn cli_build_lint_never_blocks_by_default() {
    // D-LINTPOLICY1=A (the override law, card #505): warnings never fail a
    // build by default. A money-named `Float` field fires lint L0504, but
    // with no `policy.lints` block in `package.jet` the build still succeeds.
    if !jet_bin().is_file() {
        eprintln!(
            "note: skipping cli_build_lint_never_blocks_by_default (run `cargo build` first)"
        );
        return;
    }
    let tmp = tmp_dir("lintpolicy_default");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();

    write(&tmp, "package.jet", &min_manifest("app", "0.1.0"));
    write(
        &tmp,
        "run.jet",
        "struct Invoice { price: Float }\nfn run() { print(\"hi\"); }\n",
    );

    let out = jet_cmd(&["build", "run.jet"], &tmp, &store);
    let stderr = String::from_utf8_lossy(&out.stderr);
    // A denied lint would fail *before* the effect summary prints (see
    // `cli_build_enforces_lint_policy_e1293`); reaching the summary with no
    // `E1293` proves the L0504 warning never blocked. Asserted this way
    // rather than on overall `jet build` success because the rustc stage has
    // an unrelated pre-existing failure in this environment
    // (`jet_std_env_init`) that would otherwise mask what this test checks.
    assert!(
        stderr.contains("L0504"),
        "expected the L0504 lint to still print as a warning, got:\n{stderr}"
    );
    assert!(
        stderr.contains("effects:"),
        "expected the build to proceed past the lint gate to the effect summary, got:\n{stderr}"
    );
    assert!(
        !stderr.contains("E1293"),
        "no `policy.lints` block was declared — E1293 must never fire, got:\n{stderr}"
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn cli_build_enforces_lint_policy_e1293() {
    // D-LINTPOLICY1=A: a team's own `policy: { lints: { deny: [float_money] } }`
    // in `package.jet` turns that same warning into a build failure (E1293),
    // naming the denied lint. No other `package.jet` gets this behavior — the
    // wall is opt-in, per team (the override law's third clause).
    if !jet_bin().is_file() {
        eprintln!("note: skipping cli_build_enforces_lint_policy_e1293 (run `cargo build` first)");
        return;
    }
    let tmp = tmp_dir("lintpolicy_deny");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();

    write(
        &tmp,
        "package.jet",
        &(min_manifest("app", "0.1.0") + "\npolicy: {\n    lints: { deny: [float_money] },\n}\n"),
    );
    write(
        &tmp,
        "run.jet",
        "struct Invoice { price: Float }\nfn run() { print(\"hi\"); }\n",
    );

    let out = jet_cmd(&["build", "run.jet"], &tmp, &store);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "expected the build to fail once policy.lints denies float_money, stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("E1293") && stderr.contains("L0504"),
        "expected E1293 naming float_money/L0504, got:\n{stderr}"
    );
    assert!(
        !stderr.contains("Warning [L0504]"),
        "a denied lint must not also be printed as a plain warning:\n{stderr}"
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn cli_build_unused_lint_warns_by_default_and_denies_by_policy() {
    if !jet_bin().is_file() {
        eprintln!(
            "note: skipping cli_build_unused_lint_warns_by_default_and_denies_by_policy (run `cargo build` first)"
        );
        return;
    }
    let tmp = tmp_dir("lintpolicy_unused");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();
    write(&tmp, "package.jet", &min_manifest("app", "0.1.0"));
    write(
        &tmp,
        "run.jet",
        "fn run() { unused_binding :: 1; print(\"hi\"); }\n",
    );

    let warning = jet_cmd(&["build", "run.jet"], &tmp, &store);
    let warning_stderr = String::from_utf8_lossy(&warning.stderr);
    assert!(
        warning.status.success(),
        "default warning must not block the build:\n{warning_stderr}"
    );
    assert!(
        warning_stderr.contains("L0101"),
        "expected unused-local warning:\n{warning_stderr}"
    );
    assert!(
        warning_stderr.contains("effects:"),
        "default build did not pass the lint gate:\n{warning_stderr}"
    );
    assert!(
        !warning_stderr.contains("E1293"),
        "default warning must not be denied:\n{warning_stderr}"
    );

    write(
        &tmp,
        "package.jet",
        &(min_manifest("app", "0.1.0")
            + "\npolicy: {\n    lints: { deny: [unused_local_binding] },\n}\n"),
    );
    let denied = jet_cmd(&["build", "run.jet"], &tmp, &store);
    let denied_stderr = String::from_utf8_lossy(&denied.stderr);
    assert!(
        !denied.status.success(),
        "denied lint must fail:\n{denied_stderr}"
    );
    assert!(
        denied_stderr.contains("E1293") && denied_stderr.contains("L0101"),
        "expected E1293 to name unused_local_binding/L0101:\n{denied_stderr}"
    );
    assert!(
        !denied_stderr.contains("Warning [L0101]"),
        "denied lint must not remain a plain warning:\n{denied_stderr}"
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn cli_build_rejects_lint_code_policy_value_with_complete_diagnostic() {
    if !jet_bin().is_file() {
        eprintln!(
            "note: skipping cli_build_rejects_lint_code_policy_value_with_complete_diagnostic (run `cargo build` first)"
        );
        return;
    }
    let tmp = tmp_dir("lintpolicy_code_value");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();

    write(
        &tmp,
        "package.jet",
        include_str!("ui/lint_policy_code_name.jet")
            .lines()
            .find_map(|line| line.strip_prefix("// @lint_policy_config "))
            .expect("lint policy code fixture must carry a manifest sample"),
    );
    write(&tmp, "run.jet", "fn run() { print(\"hi\"); }\n");

    let out = jet_cmd(&["build", "run.jet"], &tmp, &store);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "a lint code is not a policy value:\n{stderr}"
    );
    assert!(
        stderr.contains("Error [E1206]: `package.jet` has a manifest shape error."),
        "missing diagnostic what:\n{stderr}"
    );
    assert!(
        stderr.contains(
            "Why: A manifest field, value, or policy form is outside the current `package.jet` grammar."
        ),
        "missing diagnostic why:\n{stderr}"
    );
    assert!(
        stderr.contains(
            "Fix: For a lint selector, use `same_enum_guard_table` in `policy.lints.deny` instead of diagnostic code `L0302`; otherwise use the current `package.jet` grammar."
        ),
        "missing diagnostic fix:\n{stderr}"
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn manifest_toolchain_ok() {
    let raw = min_manifest("myapp", "0.1.0");
    let mf = jet::Manifest::parse(&PathBuf::from("package.jet"), &raw).unwrap();
    assert!(jet::Manifest::check_toolchain(&mf, "package.jet").is_ok());
}

#[test]
fn manifest_toolchain_e1208_future_version() {
    let raw = "name: \"myapp\"\nversion: \"0.1.0\"\njet: \">=99.0.0\"\n";
    let mf = jet::Manifest::parse(&PathBuf::from("package.jet"), raw).unwrap();
    let err = jet::Manifest::check_toolchain(&mf, "package.jet").expect_err("E1208");
    assert_eq!(err.code, "E1208");
}

// ─────────────────────────────────────────────
// Template generation (jet new)
// ─────────────────────────────────────────────

#[test]
fn manifest_template_plain_parses() {
    let raw = jet::Manifest::new_template("myapp", false);
    let mf = jet::Manifest::parse(&PathBuf::from("package.jet"), &raw)
        .expect("plain template should parse");
    assert_eq!(mf.package.name, "myapp");
    assert_eq!(mf.package.version, "0.1.0");
    assert!(
        mf.package.jet_constraint.is_some(),
        "plain template needs jet constraint"
    );
}

#[test]
fn manifest_template_annotated_has_dep_comments() {
    let raw = jet::Manifest::new_template("myapp", true);
    assert!(
        raw.contains("// Jet package dependencies:"),
        "annotated template should have dep comment block: {}",
        raw
    );
    // Must still parse cleanly.
    jet::Manifest::parse(&PathBuf::from("package.jet"), &raw)
        .expect("annotated template should parse");
}

// ─────────────────────────────────────────────
// Comment-preserving edit helpers
// ─────────────────────────────────────────────

#[test]
fn manifest_add_dep_inserts_in_existing_table() {
    let raw = min_manifest("root", "0.1.0") + "\ndeps: {\n}\n";
    let updated = jet::Manifest::add_dependency(
        &raw,
        "helpers",
        &jet::Manifest::DepSpec::Path {
            path: "../helpers".to_string(),
        },
    );
    let mf = jet::Manifest::parse(&PathBuf::from("package.jet"), &updated).expect("should reparse");
    assert!(matches!(
        mf.dependencies.get("helpers"),
        Some(jet::Manifest::DepSpec::Path { path }) if path == "../helpers"
    ));
}

#[test]
fn manifest_add_dep_creates_table_when_absent() {
    let raw = min_manifest("root", "0.1.0");
    let updated = jet::Manifest::add_dependency(
        &raw,
        "helpers",
        &jet::Manifest::DepSpec::Path {
            path: "../helpers".to_string(),
        },
    );
    assert!(updated.contains("deps:"), "should create deps: block");
    let mf = jet::Manifest::parse(&PathBuf::from("package.jet"), &updated).expect("should reparse");
    assert!(matches!(
        mf.dependencies.get("helpers"),
        Some(jet::Manifest::DepSpec::Path { path }) if path == "../helpers"
    ));
}

#[test]
fn manifest_add_dep_expands_inline_empty_table() {
    let raw = min_manifest("root", "0.1.0") + "\ndeps: .{}\n";
    let updated = jet::Manifest::add_dependency(
        &raw,
        "helpers",
        &jet::Manifest::DepSpec::Path {
            path: "../helpers".to_string(),
        },
    );
    let mf = jet::Manifest::parse(&PathBuf::from("package.jet"), &updated)
        .expect("inline empty deps table should stay parseable");
    assert!(matches!(
        mf.dependencies.get("helpers"),
        Some(jet::Manifest::DepSpec::Path { path }) if path == "../helpers"
    ));
    assert!(updated.contains("deps: .{\n    helpers: ../helpers,\n}\n"));
}

#[test]
fn manifest_remove_dep_removes_correct_entry() {
    let raw = min_manifest("root", "0.1.0")
        + "\ndeps: {\n    helpers: ../helpers,\n    other: ../other,\n}\n";
    let updated = jet::Manifest::remove_dependency(&raw, "helpers");
    let mf = jet::Manifest::parse(&PathBuf::from("package.jet"), &updated).expect("should reparse");
    assert!(
        mf.dependencies.get("helpers").is_none(),
        "helpers should be removed"
    );
    assert!(
        mf.dependencies.get("other").is_some(),
        "other should remain"
    );
}

// ─────────────────────────────────────────────
// SHA-256 (cross-check against implementation's own known vectors)
// ─────────────────────────────────────────────

#[test]
fn sha256_empty_vector() {
    // SHA-256 of the empty string — matches FIPS 180-4 test vector.
    let hash = jet::SHA256::sha256_hex(b"");
    assert_eq!(
        hash,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn sha256_known_vector_matches_self() {
    // The internal sha256.rs unit test asserts a specific hash for "abc".
    // Cross-check here to catch any accidental changes to the implementation.
    let hash = jet::SHA256::sha256_hex(b"abc");
    assert_eq!(hash.len(), 64, "sha256 hex must be exactly 64 chars");
    // NIST FIPS 180-4 first 16 hex chars of SHA-256("abc").
    assert!(
        hash.starts_with("ba7816bf8f01cfea"),
        "sha256(abc) prefix mismatch"
    );
}

#[test]
fn tree_hash_deterministic() {
    let tmp = tmp_dir("tree_hash_det");
    write(&tmp, "a.jet", "fn foo() {}");
    write(&tmp, "b.jet", "fn bar() {}");

    let h1 = jet::SHA256::tree_hash(&tmp);
    let h2 = jet::SHA256::tree_hash(&tmp);
    assert_eq!(h1, h2, "tree hash must be deterministic");
    assert!(
        h1.starts_with("sha256-"),
        "tree hash must have sha256- prefix"
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn tree_hash_changes_on_content_change() {
    let tmp = tmp_dir("tree_hash_chg");
    write(&tmp, "a.jet", "fn foo() {}");
    let h1 = jet::SHA256::tree_hash(&tmp);

    write(&tmp, "a.jet", "fn foo() { print(\"hello\"); }");
    let h2 = jet::SHA256::tree_hash(&tmp);
    assert_ne!(h1, h2, "tree hash must change when file content changes");

    let _ = fs::remove_dir_all(&tmp);
}

// ─────────────────────────────────────────────
// Lock fingerprinting
// ─────────────────────────────────────────────

#[test]
fn fingerprint_is_deterministic() {
    let fp1 = jet::Lock::compute_fingerprint("sha256-aabbcc", &["sha256-ddeeff"], "");
    let fp2 = jet::Lock::compute_fingerprint("sha256-aabbcc", &["sha256-ddeeff"], "");
    assert_eq!(fp1, fp2);
    assert!(fp1.starts_with("sha256-"));
}

#[test]
fn fingerprint_changes_with_deps() {
    let fp1 = jet::Lock::compute_fingerprint("sha256-aabbcc", &[], "");
    let fp2 = jet::Lock::compute_fingerprint("sha256-aabbcc", &["sha256-ddeeff"], "");
    assert_ne!(
        fp1, fp2,
        "fingerprint must change when dep fingerprints differ"
    );
}

// c129: the frozen capability contract is part of the pin — changing a public
// param's resolved capability (read → &/^) must shift the fingerprint even when
// the source tree hash and deps are identical.
#[test]
fn fingerprint_changes_with_capability_digest() {
    let fp_read = jet::Lock::compute_fingerprint("sha256-aabbcc", &[], "pkg\nfn scale(v: Vec3)");
    let fp_write = jet::Lock::compute_fingerprint("sha256-aabbcc", &[], "pkg\nfn scale(v: &Vec3)");
    assert_ne!(
        fp_read, fp_write,
        "fingerprint must change when a public capability changes"
    );
    // An empty digest skips the cap block entirely, so a package with no frozen
    // surface fingerprints identically regardless of the (empty) digest argument.
    let fp_none_a = jet::Lock::compute_fingerprint("sha256-aabbcc", &["sha256-x"], "");
    let fp_none_b = jet::Lock::compute_fingerprint("sha256-aabbcc", &["sha256-x"], "");
    assert_eq!(fp_none_a, fp_none_b);
    assert_ne!(fp_none_a, fp_read, "a frozen digest must differ from none");
}

// ─────────────────────────────────────────────
// Store path format
// ─────────────────────────────────────────────

#[test]
fn store_path_format_name_version_fp() {
    let p = jet::Store::store_path("mylib", "1.0.0", "sha256-deadbeef");
    let name = p.file_name().unwrap().to_str().unwrap();
    assert_eq!(
        name, "mylib-1.0.0-deadbeef",
        "store entry: <name>-<version>-<fp_no_prefix>"
    );
}

#[test]
fn store_path_strips_sha256_prefix() {
    let tmp = tmp_dir("store_path_idem");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();
    with_store(&store, || {
        let with_pfx = jet::Store::store_path("mylib", "1.0.0", "sha256-deadbeef");
        let without = jet::Store::store_path("mylib", "1.0.0", "deadbeef");
        assert_eq!(with_pfx, without);
    });
}

// ─────────────────────────────────────────────
// Store operations (redirect HOME to temp dir)
// ─────────────────────────────────────────────

#[test]
fn store_ensure_path_dep_creates_entry() {
    let tmp = tmp_dir("store_ensure");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();

    let src = tmp.join("mylib_src");
    write(
        &src,
        "mylib.jet",
        "pub fn hello() => String { return \"hi\"; }\n",
    );
    write(&src, "package.jet", &min_manifest("mylib", "0.1.0"));

    let fp = "sha256-0000000000000000000000000000000000000000000000000000000000000000";

    let (entry, _hash) = with_store(&store, || {
        jet::Store::ensure_path_dep("mylib", "0.1.0", fp, &src)
            .expect("ensure_path_dep should succeed")
    });

    assert!(entry.is_dir(), "store entry dir must exist");
    assert!(
        entry.join("mylib.jet").is_file(),
        "source file must be in store"
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn store_ensure_is_idempotent() {
    let tmp = tmp_dir("store_idem");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();

    let src = tmp.join("mylib_src");
    write(&src, "mylib.jet", "pub fn x() {}\n");
    write(&src, "package.jet", &min_manifest("mylib", "0.1.0"));

    let fp = "sha256-1111111111111111111111111111111111111111111111111111111111111111";

    let (p1, p2) = with_store(&store, || {
        let (a, _) = jet::Store::ensure_path_dep("mylib", "0.1.0", fp, &src).unwrap();
        let (b, _) = jet::Store::ensure_path_dep("mylib", "0.1.0", fp, &src).unwrap();
        (a, b)
    });

    assert_eq!(p1, p2, "second call must return the same store path");
    let _ = fs::remove_dir_all(&tmp);
}

#[cfg(unix)]
#[test]
fn store_install_rejects_source_and_destination_symlinks() {
    use std::os::unix::fs::symlink;

    let tmp = tmp_dir("store_symlink_boundary");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();
    let outside = tmp.join("outside");
    fs::write(&outside, "must survive\n").unwrap();
    let source = tmp.join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("package.jet"), "name: \"safe\"\n").unwrap();

    let outcomes = with_store(&store, || {
        let entry = jet::Store::store_path("safe", "0.1.0", "sha256-safe");
        symlink(&outside, &entry).unwrap();
        let destination = jet::Store::ensure_path_dep("safe", "0.1.0", "sha256-safe", &source);
        fs::remove_file(&entry).unwrap();

        symlink(&outside, source.join("leak")).unwrap();
        let source_result =
            jet::Store::ensure_path_dep("safe", "0.1.0", "sha256-source", &source);
        (destination, source_result)
    });

    assert!(outcomes.0.is_err(), "store entry symlink must be refused");
    assert!(outcomes.1.is_err(), "source tree symlink must be refused");
    assert_eq!(fs::read_to_string(&outside).unwrap(), "must survive\n");
    let _ = fs::remove_dir_all(&tmp);
}

// ─────────────────────────────────────────────
// Store verify / tamper detection (E1204)
// ─────────────────────────────────────────────

#[test]
fn store_tamper_detected_e1204() {
    let tmp = tmp_dir("tamper");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();

    let src = tmp.join("mylib_src");
    write(&src, "mylib.jet", "pub fn ok() {}\n");
    write(&src, "package.jet", &min_manifest("mylib", "0.1.0"));

    let fp = "sha256-2222222222222222222222222222222222222222222222222222222222222222";

    let (entry, genuine_hash) = with_store(&store, || {
        let (e, h) = jet::Store::ensure_path_dep("mylib", "0.1.0", fp, &src).unwrap();
        (e, h)
    });

    // Tamper: modify the stored file outside the locked section.
    fs::write(
        entry.join("mylib.jet"),
        "pub fn evil() { /* tampered */ }\n",
    )
    .unwrap();

    let result = jet::Store::verify_entry("mylib", &entry, &genuine_hash);
    let diag = result.expect_err("verify_entry must return E1204 after tampering");
    assert_eq!(diag.code, "E1204");

    let _ = fs::remove_dir_all(&tmp);
}

// ─────────────────────────────────────────────
// D-CASTORE1=A: content-addressed store identity
// ─────────────────────────────────────────────

#[test]
fn content_hash_recorded_at_install() {
    let tmp = tmp_dir("castore_record");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();

    let src = tmp.join("src");
    write(&src, "lib.jet", "pub fn x() {}\n");
    write(&src, "package.jet", &min_manifest("mylib", "0.1.0"));

    let fp = "sha256-aaaa0000000000000000000000000000000000000000000000000000000000aa";

    let (entry, hash) = with_store(&store, || {
        jet::Store::ensure_path_dep("mylib", "0.1.0", fp, &src).unwrap()
    });

    assert!(entry.is_dir());
    // Hash is a non-empty sha256-prefixed string.
    assert!(
        hash.starts_with("sha256-"),
        "content hash must be sha256-prefixed"
    );
    // verify_content_hash succeeds with the correct hash.
    jet::Store::verify_content_hash("mylib", &entry, &hash).expect("fresh install must verify");
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn content_hash_mismatch_after_tamper() {
    let tmp = tmp_dir("castore_tamper");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();

    let src = tmp.join("src");
    write(&src, "lib.jet", "pub fn x() {}\n");
    write(&src, "package.jet", &min_manifest("mylib", "0.1.0"));

    let fp = "sha256-bbbb0000000000000000000000000000000000000000000000000000000000bb";

    let (entry, original_hash) = with_store(&store, || {
        jet::Store::ensure_path_dep("mylib", "0.1.0", fp, &src).unwrap()
    });

    // Tamper with the store entry.
    fs::write(entry.join("lib.jet"), "pub fn evil() {}\n").unwrap();

    let result = jet::Store::verify_content_hash("mylib", &entry, &original_hash);
    let diag = result.expect_err("tampered entry must fail verify_content_hash");
    assert_eq!(diag.code, "E1204");
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn content_hash_covers_non_jet_files_copied_into_store() {
    let tmp = tmp_dir("castore_non_jet_tamper");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();

    let src = tmp.join("src");
    write(&src, "lib.jet", "pub fn x() {}\n");
    write(&src, "package.jet", &min_manifest("mylib", "0.1.0"));
    fs::write(src.join("runtime.data"), b"trusted bytes\n").unwrap();

    let source_hash = jet::SHA256::tree_hash(&src);
    fs::write(src.join("runtime.data"), b"hostile bytes\n").unwrap();
    assert_ne!(
        source_hash,
        jet::SHA256::tree_hash(&src),
        "copied non-.jet files must be part of the source identity"
    );

    fs::write(src.join("runtime.data"), b"trusted bytes\n").unwrap();
    let fp = "sha256-cccc0000000000000000000000000000000000000000000000000000000000cc";
    let (entry, original_hash) = with_store(&store, || {
        jet::Store::ensure_path_dep("mylib", "0.1.0", fp, &src).unwrap()
    });
    fs::write(entry.join("runtime.data"), b"tampered bytes\n").unwrap();

    let error = jet::Store::verify_content_hash("mylib", &entry, &original_hash)
        .expect_err("tampering a copied non-.jet file must fail verification");
    assert_eq!(error.code, "E1204");
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn lock_file_content_hash_roundtrip() {
    use jet::Lock::{LockFile, LockSource, LockedPackage, LockedWorkspaceMember};
    let pkg = LockedPackage {
        name: "foo".into(),
        version: "1.0.0".into(),
        source: LockSource::Path("../foo".into()),
        locked: None,
        fingerprint: "sha256-cccc".into(),
        content_hash: Some("sha256-deadbeef".into()),
        dependencies: vec![],
        layer: Some(jet::Syntax::RuntimeLayer::Core),
        inferred_layer: Some(jet::Syntax::RuntimeLayer::Std),

        effects: vec![],

        effect_grants: vec![],
        required_effects: vec![],
        granted_effects: vec![],
        denied_effects: vec![],
        effect_authority: None,
        envelope: None,
        receipt: None,
        provenance: None,
    };
    let lock = LockFile {
        version: 1,
        packages: vec![pkg],
        root_dependencies: vec!["foo".into()],
        authority: None,
        workspace_members: vec![LockedWorkspaceMember {
            name: "hello".into(),
            path: "packages/hello".into(),
            source_digest: "no-workspace-source".into(),
            canonical_path: "/workspace/packages/hello".into(),
            package_digest: "sha256-package".into(),
        }],
        comptime_inputs: vec![],
        workspace_source_digest: None,
        workspace_overlay_policy: Default::default(),
        toolchains: Vec::new(),
        browsers: Vec::new(),
        source_channels: Vec::new(),
        build_stamp: None,
        build_contributions: Vec::new(),
    };
    let serialized = jet::Lock::write(&lock);
    assert!(
        serialized.contains("content-hash = \"sha256-deadbeef\""),
        "content-hash must appear in lockfile"
    );
    assert!(serialized.contains("layer = \"core\""));
    assert!(serialized.contains("inferred-layer = \"hosted\""));
    let parsed = jet::Lock::parse(&serialized).expect("must parse back");
    assert_eq!(
        parsed.packages[0].content_hash,
        Some("sha256-deadbeef".into())
    );
    assert_eq!(
        parsed.packages[0].layer,
        Some(jet::Syntax::RuntimeLayer::Core)
    );
    assert_eq!(
        parsed.packages[0].inferred_layer,
        Some(jet::Syntax::RuntimeLayer::Std)
    );
    assert_eq!(parsed.workspace_members[0].name, "hello");
    assert_eq!(parsed.workspace_members[0].path, "packages/hello");
}

// ─────────────────────────────────────────────
// Hardlink: two projects share one store inode
// ─────────────────────────────────────────────

#[test]
fn hardlink_projects_share_store_inode() {
    let tmp = tmp_dir("hardlink");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();

    let src = tmp.join("mylib_src");
    write(&src, "mylib.jet", "pub fn hi() {}\n");
    write(&src, "package.jet", &min_manifest("mylib", "0.1.0"));

    let fp = "sha256-3333333333333333333333333333333333333333333333333333333333333333";

    // link_root must NOT pre-exist — link_into_project checks that.
    let link1 = tmp.join("proj1/deps/mylib");
    let link2 = tmp.join("proj2/deps/mylib");

    let store_entry = with_store(&store, || {
        let (e, _) = jet::Store::ensure_path_dep("mylib", "0.1.0", fp, &src).unwrap();
        jet::Store::link_into_project(&e, &link1).unwrap();
        jet::Store::link_into_project(&e, &link2).unwrap();
        e
    });

    // Both link dirs and the store entry must share inodes (hardlinks).
    use std::os::unix::fs::MetadataExt;
    let store_ino = fs::metadata(store_entry.join("mylib.jet")).unwrap().ino();
    let link1_ino = fs::metadata(link1.join("mylib.jet")).unwrap().ino();
    let link2_ino = fs::metadata(link2.join("mylib.jet")).unwrap().ino();

    assert_eq!(store_ino, link1_ino, "link1 must share inode with store");
    assert_eq!(store_ino, link2_ino, "link2 must share inode with store");

    let _ = fs::remove_dir_all(&tmp);
}

// ─────────────────────────────────────────────
// Path-dep compilation (full front-end integration)
// ─────────────────────────────────────────────

#[test]
fn path_dep_compiles_ok() {
    let tmp = tmp_dir("pd_compile");

    // Greeter library.
    write(
        &tmp,
        "greeter/package.jet",
        &min_manifest("greeter", "0.1.0"),
    );
    write(
        &tmp,
        "greeter/greeter.jet",
        "pub fn greet() => String { return \"hello!\"; }\n",
    );

    // Root project with path dep.
    write(
        &tmp,
        "package.jet",
        &manifest_with_deps("myapp", "0.1.0", "    greeter: ./greeter,"),
    );
    let entry = tmp.join("run.jet");
    fs::write(
        &entry,
        "use greeter;\nfn run() { print(greeter.greet()); }\n",
    )
    .unwrap();

    let result = jet::compile_with_path("", &entry.to_string_lossy());
    assert!(
        result.is_ok(),
        "path dep project should compile:\n{}",
        result
            .err()
            .map(|d| jet::render_diagnostics("run.jet", "", &d))
            .unwrap_or_default()
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn transitive_path_dependency_cannot_escape_declaring_package() {
    let tmp = tmp_dir("transitive_path_escape");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();

    write(
        &tmp,
        "outer/package.jet",
        &manifest_with_deps("outer", "0.1.0", "    escape: ../outside,"),
    );
    write(&tmp, "outer/outer.jet", "pub fn outer() {}\n");
    write(
        &tmp,
        "outside/package.jet",
        &min_manifest("outside", "0.1.0"),
    );
    write(&tmp, "outside/outside.jet", "pub fn hostile() {}\n");
    let raw = manifest_with_deps("app", "0.1.0", "    outer: ./outer,");
    write(&tmp, "package.jet", &raw);
    let manifest = jet::Manifest::parse(&tmp.join("package.jet"), &raw).unwrap();
    let options = jet::Fetch::FetchOptions {
        locked: false,
        update: false,
        update_dep: None,
        resolution: jet::Publish::ResolveMode::Conservative,
    };

    let error = with_store(&store, || {
        jet::Fetch::fetch(&tmp, &manifest, None, &options)
            .expect_err("a transitive path escape must be rejected")
    });
    assert_eq!(first_diag_code(&error), "E1206");
    assert!(error[0].what.contains("escapes"));
    assert!(!store.join("outside").exists(), "escaped package was ingested");
    let _ = fs::remove_dir_all(&tmp);
}

#[cfg(unix)]
#[test]
fn transitive_path_dependency_cannot_escape_via_symlinked_directory() {
    use std::os::unix::fs::symlink;

    let tmp = tmp_dir("transitive_path_symlink_escape");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();

    write(
        &tmp,
        "outer/package.jet",
        &manifest_with_deps("outer", "0.1.0", "    escape: ./link/escape,"),
    );
    write(&tmp, "outer/outer.jet", "pub fn outer() {}\n");
    write(
        &tmp,
        "outside/escape/package.jet",
        &min_manifest("escape", "0.1.0"),
    );
    write(
        &tmp,
        "outside/escape/escape.jet",
        "pub fn hostile() {}\n",
    );
    symlink(tmp.join("outside"), tmp.join("outer/link")).unwrap();

    let raw = manifest_with_deps("app", "0.1.0", "    outer: ./outer,");
    write(&tmp, "package.jet", &raw);
    let manifest = jet::Manifest::parse(&tmp.join("package.jet"), &raw).unwrap();
    let options = jet::Fetch::FetchOptions {
        locked: false,
        update: false,
        update_dep: None,
        resolution: jet::Publish::ResolveMode::Conservative,
    };

    let error = with_store(&store, || {
        jet::Fetch::fetch(&tmp, &manifest, None, &options)
            .expect_err("a symlinked transitive path must be rejected")
    });
    assert_eq!(first_diag_code(&error), "E1206");
    assert!(error[0].what.contains("escapes"));
    assert!(!store.join("escape").exists(), "escaped package was ingested");
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn version_conflict_emits_e1201() {
    let tmp = tmp_dir("ver_conflict");

    write(&tmp, "liba/package.jet", &min_manifest("mylib", "1.0.0"));
    write(&tmp, "liba/mylib.jet", "pub fn v1() {}\n");

    write(&tmp, "libb/package.jet", &min_manifest("mylib", "2.0.0"));
    write(&tmp, "libb/mylib.jet", "pub fn v2() {}\n");

    write(
        &tmp,
        "package.jet",
        &manifest_with_deps(
            "conflict_app",
            "0.1.0",
            "    liba: ./liba,\n    libb: ./libb,",
        ),
    );
    let entry = tmp.join("run.jet");
    fs::write(&entry, "fn run() {}\n").unwrap();

    let diags = jet::compile_with_path("", &entry.to_string_lossy())
        .expect_err("version conflict must fail with E1201");
    assert_eq!(first_diag_code(&diags), "E1201");

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn stale_lock_emits_e1202() {
    let tmp = tmp_dir("stale_lock");

    write(
        &tmp,
        "greeter/package.jet",
        &min_manifest("greeter", "0.1.0"),
    );
    write(
        &tmp,
        "greeter/greeter.jet",
        "pub fn greet() => String { return \"hi\"; }\n",
    );

    write(
        &tmp,
        "package.jet",
        &manifest_with_deps("app", "0.1.0", "    greeter: ./greeter,"),
    );
    // Lock exists but lists no dependencies — stale.
    write(
        &tmp,
        ".jet/lock",
        "version = 1\n\n[[package]]\nname = \"app\"\nsource = { root = \".\" }\n\n[root]\ndependencies = []\n",
    );

    let entry = tmp.join("run.jet");
    fs::write(&entry, "fn run() {}\n").unwrap();

    let diags = jet::compile_with_path("", &entry.to_string_lossy())
        .expect_err("stale lock must fail with E1202");
    assert_eq!(first_diag_code(&diags), "E1202");

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn toolchain_mismatch_emits_e1208() {
    let tmp = tmp_dir("tc_mismatch");

    write(
        &tmp,
        "package.jet",
        "name: \"app\"\nversion: \"0.1.0\"\njet: \">=99.0.0\"\n",
    );
    let entry = tmp.join("run.jet");
    fs::write(&entry, "fn run() { print(\"hi\"); }\n").unwrap();

    let diags = jet::compile_with_path("", &entry.to_string_lossy())
        .expect_err("toolchain mismatch must fail with E1208");
    assert_eq!(first_diag_code(&diags), "E1208");

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn reserved_section_emits_e1209() {
    let tmp = tmp_dir("reserved_sec");

    write(
        &tmp,
        "package.jet",
        &(min_manifest("app", "0.1.0") + "\ndev_deps: {\n    testlib: ../testlib,\n}\n"),
    );
    let entry = tmp.join("run.jet");
    fs::write(&entry, "fn run() {}\n").unwrap();

    let diags = jet::compile_with_path("", &entry.to_string_lossy())
        .expect_err("reserved section must fail with E1209");
    assert_eq!(first_diag_code(&diags), "E1209");

    let _ = fs::remove_dir_all(&tmp);
}

// ─────────────────────────────────────────────
// --locked CI mode (via fetch API)
// ─────────────────────────────────────────────

#[test]
fn fetch_locked_rejects_missing_lock() {
    let tmp = tmp_dir("locked_no_lock");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();

    let raw = manifest_with_deps("app", "0.1.0", "    greeter: ./greeter,");
    write(&tmp, "package.jet", &raw);

    let mf = jet::Manifest::parse(&tmp.join("package.jet"), &raw).unwrap();
    let opts = jet::Fetch::FetchOptions {
        locked: true,
        update: false,
        update_dep: None,
        resolution: jet::Publish::ResolveMode::Conservative,
    };

    let result = with_store(&store, || jet::Fetch::fetch(&tmp, &mf, None, &opts));
    let diags = result.expect_err("--locked with no lock file should fail");
    assert_eq!(first_diag_code(&diags), "E1202");

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn compiler_rejects_transitive_path_dependency_escape() {
    let tmp = tmp_dir("compiler_transitive_path_escape");
    let outer = tmp.join("outer");
    let outside = tmp.join("outside");
    write(
        &outer,
        "package.jet",
        &(min_manifest("outer", "0.1.0") + "\ndeps: { escape: ../outside }\n"),
    );
    write(&outer, "outer.jet", "pub fn value() {}\n");
    write(&outside, "package.jet", &min_manifest("outside", "0.1.0"));
    write(&outside, "outside.jet", "pub fn value() {}\n");
    let raw = manifest_with_deps("app", "0.1.0", "    outer: ./outer,");
    write(&tmp, "package.jet", &raw);
    write(&tmp, "run.jet", "fn run() {}\n");

    let entry = tmp.join("run.jet");
    let errors = jet::Driver::compile_bundle_path_build(
        entry.to_str().unwrap(),
        jet::Driver::BuildRunOptions::default(),
    )
    .expect_err("the compiler must not read a transitive manifest outside its package");
    assert_eq!(first_diag_code(&errors), "E1206");
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn fetch_locked_rejects_tampered_path_dependency_source() {
    let tmp = tmp_dir("locked_path_tamper");
    let store = tmp.join("store");
    let dependency = tmp.join("greeter");
    fs::create_dir_all(&store).unwrap();
    write(&dependency, "package.jet", &min_manifest("greeter", "0.1.0"));
    write(&dependency, "greeter.jet", "pub fn greet() {}\n");
    let raw = manifest_with_deps("app", "0.1.0", "    greeter: ./greeter,");
    write(&tmp, "package.jet", &raw);
    let manifest = jet::Manifest::parse(&tmp.join("package.jet"), &raw).unwrap();
    let unlocked = jet::Fetch::FetchOptions {
        locked: false,
        update: false,
        update_dep: None,
        resolution: jet::Publish::ResolveMode::Conservative,
    };
    let (lock, _) = with_store(&store, || {
        jet::Fetch::fetch(&tmp, &manifest, None, &unlocked)
            .expect("initial path fetch should create a content hash")
    });

    write(&dependency, "greeter.jet", "pub fn compromised() {}\n");
    let locked = jet::Fetch::FetchOptions {
        locked: true,
        update: false,
        update_dep: None,
        resolution: jet::Publish::ResolveMode::Conservative,
    };
    let error = with_store(&store, || {
        jet::Fetch::fetch(&tmp, &manifest, Some(&lock), &locked)
            .expect_err("locked fetch must reject a changed path source")
    });
    assert_eq!(first_diag_code(&error), "E1204");
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn locked_build_rejects_tampered_path_dependency_source() {
    let tmp = tmp_dir("locked_build_path_tamper");
    let store = tmp.join("store");
    let dependency = tmp.join("greeter");
    fs::create_dir_all(&store).unwrap();
    write(&dependency, "package.jet", &min_manifest("greeter", "0.1.0"));
    write(&dependency, "greeter.jet", "pub fn greet() {}");
    let raw = manifest_with_deps("app", "0.1.0", "    greeter: ./greeter,");
    write(&tmp, "package.jet", &raw);
    write(&tmp, "run.jet", "fn run() {}\n");
    let manifest = jet::Manifest::parse(&tmp.join("package.jet"), &raw).unwrap();
    let unlocked = jet::Fetch::FetchOptions {
        locked: false,
        update: false,
        update_dep: None,
        resolution: jet::Publish::ResolveMode::Conservative,
    };
    with_store(&store, || {
        jet::Fetch::fetch(&tmp, &manifest, None, &unlocked)
            .expect("initial path fetch should create a lock hash");
    });

    write(&dependency, "greeter.jet", "pub fn compromised() {}\n");
    let mut locked = jet::Driver::BuildRunOptions::default();
    locked.locked = true;
    let entry = tmp.join("run.jet");
    let errors = with_store(&store, || {
        jet::Driver::compile_bundle_path_build(entry.to_str().unwrap(), locked)
            .expect_err("locked build must reject a changed path source before loading it")
    });
    assert_eq!(first_diag_code(&errors), "E1204");
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn registry_dependency_reports_transport_failure() {
    let tmp = tmp_dir("registry_dep_staged");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();
    let raw = manifest_with_deps("app", "0.1.0", "    textkit: textkit#1.2.0,");
    write(&tmp, "package.jet", &raw);
    let mf = jet::Manifest::parse(&tmp.join("package.jet"), &raw).unwrap();
    let opts = jet::Fetch::FetchOptions {
        locked: false,
        update: false,
        update_dep: None,
        resolution: jet::Publish::ResolveMode::Conservative,
    };
    let diags = with_store(&store, || jet::Fetch::fetch(&tmp, &mf, None, &opts))
        .expect_err("registry dependency must report its verified transport diagnostic");
    assert_eq!(first_diag_code(&diags), "E1207");
    let rendered =
        jet::Diagnostics::render_all(&tmp.join("package.jet").to_string_lossy(), &raw, &diags);
    assert!(
        rendered.contains("Error [E1207]:"),
        "unexpected diagnostic:\n{rendered}"
    );
    assert!(
        rendered.contains("Why:"),
        "missing E1207 reason:\n{rendered}"
    );
    assert!(rendered.contains("Fix:"), "missing E1207 fix:\n{rendered}");
    let snapshot = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/jetpack-diagnostics/E1207.stderr");
    if std::env::var_os("UPDATE_EXPECT").is_some() {
        fs::write(&snapshot, &rendered).unwrap();
    }
    assert_eq!(rendered, fs::read_to_string(snapshot).unwrap());
    let _ = fs::remove_dir_all(&tmp);
}

// ─────────────────────────────────────────────
// Git dep (local bare repo — no network needed; skip if no git)
// ─────────────────────────────────────────────

#[test]
fn git_dep_local_bare_repo_fetches_ok() {
    if !have_git() {
        eprintln!("note: skipping git_dep_local_bare_repo (git not found)");
        return;
    }

    let tmp = tmp_dir("git_dep");

    // Create a source directory to commit.
    let src = tmp.join("mylib_src");
    write(&src, "mylib.jet", "pub fn answer() => Int { return 42; }\n");
    write(&src, "package.jet", &min_manifest("mylib", "0.1.0"));

    // Init bare repo.
    let bare = tmp.join("mylib.git");
    Command::new("git")
        .args(["init", "--bare", bare.to_str().unwrap()])
        .output()
        .unwrap();

    // Clone, add files, commit, push tag.
    let clone = tmp.join("mylib_clone");
    Command::new("git")
        .args(["clone", bare.to_str().unwrap(), clone.to_str().unwrap()])
        .output()
        .unwrap();
    for e in fs::read_dir(&src).unwrap().flatten() {
        fs::copy(e.path(), clone.join(e.file_name())).unwrap();
    }
    for (k, v) in [("user.email", "test@jet.test"), ("user.name", "Jet Test")] {
        Command::new("git")
            .args(["config", k, v])
            .current_dir(&clone)
            .output()
            .unwrap();
    }
    Command::new("git")
        .args(["add", "."])
        .current_dir(&clone)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(&clone)
        .output()
        .unwrap();
    Command::new("git")
        .args(["push", "origin", "HEAD:main"])
        .current_dir(&clone)
        .output()
        .unwrap();
    Command::new("git")
        .args(["tag", "v0.1.0"])
        .current_dir(&clone)
        .output()
        .unwrap();
    Command::new("git")
        .args(["push", "origin", "v0.1.0"])
        .current_dir(&clone)
        .output()
        .unwrap();

    let repo_url = format!("file://{}", bare.to_str().unwrap());
    let raw = manifest_with_deps(
        "app",
        "0.1.0",
        &format!("    mylib: {{ git: \"{}\", tag: \"v0.1.0\" }},", repo_url),
    );
    write(&tmp, "package.jet", &raw);
    write(&tmp, "run.jet", "fn run() {}\n");

    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();
    let home = tmp.join("home");
    fs::create_dir_all(&home).unwrap();

    let mf = jet::Manifest::parse(&tmp.join("package.jet"), &raw).unwrap();
    let opts = jet::Fetch::FetchOptions {
        locked: false,
        update: false,
        update_dep: None,
        resolution: jet::Publish::ResolveMode::Conservative,
    };

    let previous_home = std::env::var_os("HOME");
    std::env::set_var("HOME", &home);
    let (lock, _) = with_store(&store, || {
        jet::Fetch::fetch(&tmp, &mf, None, &opts).expect("git dep fetch should succeed")
    });
    match previous_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }

    let revision = lock
        .packages
        .iter()
        .find(|package| package.name == "mylib")
        .and_then(|package| package.locked.as_ref())
        .expect("git lock entry should pin a revision");
    let url_hash = jet::SHA256::sha256_hex(repo_url.as_bytes());
    let revision_prefix: String = revision.rev.chars().take(16).collect();
    let cache_dir = home
        .join(".jet")
        .join("git-cache")
        .join(&url_hash[..16])
        .join(revision_prefix);
    fs::write(cache_dir.join("mylib.jet"), "pub fn compromised() {}\n").unwrap();
    fs::write(
        tmp.join(".jet-build/deps/mylib/mylib.jet"),
        "pub fn compromised() {}\n",
    )
    .unwrap();

    let locked = jet::Fetch::FetchOptions {
        locked: true,
        update: false,
        update_dep: None,
        resolution: jet::Publish::ResolveMode::Conservative,
    };
    let previous_home = std::env::var_os("HOME");
    std::env::set_var("HOME", &home);
    let error = with_store(&store, || {
        jet::Fetch::fetch(&tmp, &mf, Some(&lock), &locked)
            .expect_err("locked fetch must reject a changed git cache")
    });
    match previous_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
    assert_eq!(first_diag_code(&error), "E1204");

    let mut locked_build = jet::Driver::BuildRunOptions::default();
    locked_build.locked = true;
    let entry = tmp.join("run.jet");
    let build_error = with_store(&store, || {
        jet::Driver::compile_bundle_path_build(entry.to_str().unwrap(), locked_build)
            .expect_err("locked build must reject a changed Git dependency link")
    });
    assert_eq!(first_diag_code(&build_error), "E1204");

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn git_revision_with_multibyte_prefix_does_not_panic() {
    if !have_git() {
        eprintln!("note: skipping git_revision_with_multibyte_prefix (git not found)");
        return;
    }

    let tmp = tmp_dir("git_revision_utf8");
    let revision = format!("{}é", "a".repeat(15));
    let raw = manifest_with_deps(
        "app",
        "0.1.0",
        &format!(
            "    broken: {{ git: \"file:///definitely/missing.git\", rev: \"{revision}\" }},"
        ),
    );
    write(&tmp, "package.jet", &raw);
    let manifest = jet::Manifest::parse(&tmp.join("package.jet"), &raw).unwrap();
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();
    let options = jet::Fetch::FetchOptions {
        locked: false,
        update: false,
        update_dep: None,
        resolution: jet::Publish::ResolveMode::Conservative,
    };

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        with_store(&store, || jet::Fetch::fetch(&tmp, &manifest, None, &options))
    }));
    assert!(
        result.is_ok(),
        "a multibyte git revision must return a diagnostic, not panic"
    );
    assert!(result.unwrap().is_err(), "the missing repository must fail");
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn git_dep_rejects_private_transport_before_network_access() {
    if !have_git() {
        eprintln!("note: skipping git_dep_private_transport (git not found)");
        return;
    }

    let tmp = tmp_dir("git_private_transport");
    let raw = manifest_with_deps(
        "app",
        "0.1.0",
        "    private: { git: \"http://127.0.0.1:9/private.git\", rev: \"0000000000000000000000000000000000000000\" },",
    );
    write(&tmp, "package.jet", &raw);
    let mf = jet::Manifest::parse(&tmp.join("package.jet"), &raw).unwrap();
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();
    let opts = jet::Fetch::FetchOptions {
        locked: false,
        update: false,
        update_dep: None,
        resolution: jet::Publish::ResolveMode::Conservative,
    };

    let diags = with_store(&store, || jet::Fetch::fetch(&tmp, &mf, None, &opts))
        .expect_err("private git transport must be rejected before clone");
    assert_eq!(first_diag_code(&diags), "E1203");
    let rendered = jet::Diagnostics::render_all(
        &tmp.join("package.jet").to_string_lossy(),
        &raw,
        &diags,
    );
    assert!(rendered.contains("not allowed"), "unexpected diagnostic:\n{rendered}");
    assert!(!tmp.join(".jet").exists(), "rejected transport created project state");
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn git_dep_rejects_option_revision_before_git_execution() {
    if !have_git() {
        eprintln!("note: skipping git_dep_option_revision (git not found)");
        return;
    }

    let tmp = tmp_dir("git_option_revision");
    let marker = tmp.join("git-option-injection-marker");
    let raw = manifest_with_deps(
        "app",
        "0.1.0",
        &format!(
            "    hostile: {{ git: \"file:///definitely/missing.git\", rev: \"--upload-pack=touch {}\" }},",
            marker.display()
        ),
    );
    write(&tmp, "package.jet", &raw);
    let mf = jet::Manifest::parse(&tmp.join("package.jet"), &raw).unwrap();
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();
    let opts = jet::Fetch::FetchOptions {
        locked: false,
        update: false,
        update_dep: None,
        resolution: jet::Publish::ResolveMode::Conservative,
    };

    let diags = with_store(&store, || jet::Fetch::fetch(&tmp, &mf, None, &opts))
        .expect_err("an option-shaped revision must be rejected before git");
    assert_eq!(first_diag_code(&diags), "E1203");
    assert!(!marker.exists());
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn git_dep_rejects_cache_path_traversal_before_filesystem_access() {
    if !have_git() {
        eprintln!("note: skipping git_dep_cache_path_traversal (git not found)");
        return;
    }

    let tmp = tmp_dir("git_cache_path_traversal");
    let escaped = tmp.join("escaped-cache");
    let raw = manifest_with_deps(
        "app",
        "0.1.0",
        "    hostile: { git: \"file:///definitely/missing.git\", rev: \"../escaped-cache\" },",
    );
    write(&tmp, "package.jet", &raw);
    let manifest = jet::Manifest::parse(&tmp.join("package.jet"), &raw).unwrap();
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();
    let options = jet::Fetch::FetchOptions {
        locked: false,
        update: false,
        update_dep: None,
        resolution: jet::Publish::ResolveMode::Conservative,
    };

    let diagnostics = with_store(&store, || jet::Fetch::fetch(&tmp, &manifest, None, &options))
        .expect_err("a traversal-shaped revision must be rejected");
    assert_eq!(first_diag_code(&diagnostics), "E1203");
    assert!(!escaped.exists(), "rejected revision created an escaped cache");
    let _ = fs::remove_dir_all(&tmp);
}

// ─────────────────────────────────────────────
// @latest / branch update rewrites lock
// ─────────────────────────────────────────────

#[test]
fn git_dep_branch_update_rewrites_lock() {
    if !have_git() {
        eprintln!("note: skipping git_dep_branch_update_rewrites_lock (git not found)");
        return;
    }

    let tmp = tmp_dir("git_branch_upd");

    // Create a source directory to commit.
    let src = tmp.join("mylib_src");
    write(&src, "mylib.jet", "pub fn answer() => Int { return 42; }\n");
    write(&src, "package.jet", &min_manifest("mylib", "0.1.0"));

    // Init a non-bare repo, commit, then mirror to a bare repo (avoids HEAD ambiguity).
    let init_repo = tmp.join("mylib_init");
    Command::new("git")
        .args(["init", "-b", "main", init_repo.to_str().unwrap()])
        .output()
        .unwrap();
    for e in fs::read_dir(&src).unwrap().flatten() {
        fs::copy(e.path(), init_repo.join(e.file_name())).unwrap();
    }
    for (k, v) in [("user.email", "test@jet.test"), ("user.name", "Jet Test")] {
        Command::new("git")
            .args(["config", k, v])
            .current_dir(&init_repo)
            .output()
            .unwrap();
    }
    Command::new("git")
        .args(["add", "."])
        .current_dir(&init_repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(&init_repo)
        .output()
        .unwrap();

    // Create the bare repo from this.
    let bare = tmp.join("mylib.git");
    Command::new("git")
        .args([
            "clone",
            "--bare",
            init_repo.to_str().unwrap(),
            bare.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    // Clone from the bare repo for future pushes.
    let clone = tmp.join("mylib_clone");
    Command::new("git")
        .args(["clone", bare.to_str().unwrap(), clone.to_str().unwrap()])
        .output()
        .unwrap();
    for (k, v) in [("user.email", "test@jet.test"), ("user.name", "Jet Test")] {
        Command::new("git")
            .args(["config", k, v])
            .current_dir(&clone)
            .output()
            .unwrap();
    }

    let repo_url = format!("file://{}", bare.to_str().unwrap());
    let raw = manifest_with_deps(
        "app",
        "0.1.0",
        &format!("    mylib: {{ git: \"{}\", branch: \"main\" }},", repo_url),
    );
    write(&tmp, "package.jet", &raw);

    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();

    let mf = jet::Manifest::parse(&tmp.join("package.jet"), &raw).unwrap();

    // Initial fetch (no lock yet) — writes the lock file.
    let opts = jet::Fetch::FetchOptions {
        locked: false,
        update: false,
        update_dep: None,
        resolution: jet::Publish::ResolveMode::Conservative,
    };
    let result = with_store(&store, || jet::Fetch::fetch(&tmp, &mf, None, &opts));
    assert!(
        result.is_ok(),
        "initial git branch fetch should succeed: {:?}",
        result.err()
    );

    // Capture the initial rev from the lock file.
    let lock_raw = fs::read_to_string(tmp.join(".jet/lock")).expect(".jet/lock must exist");
    let initial_rev = extract_rev_from_lock(&lock_raw);
    assert!(
        !initial_rev.is_empty(),
        "lock must contain a rev after initial fetch"
    );

    // Make a NEW commit to the branch.
    write(
        &clone,
        "extra.jet",
        "pub fn extra() => Int { return 99; }\n",
    );
    Command::new("git")
        .args(["add", "."])
        .current_dir(&clone)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "second commit"])
        .current_dir(&clone)
        .output()
        .unwrap();
    Command::new("git")
        .args(["push", "origin", "HEAD:main"])
        .current_dir(&clone)
        .output()
        .unwrap();

    // Re-fetch with update = true — should re-resolve the branch and update the lock.
    let lock_str = fs::read_to_string(tmp.join(".jet/lock")).unwrap();
    let existing_lock = jet::Lock::parse(&lock_str).expect("initial lock must parse");
    let update_opts = jet::Fetch::FetchOptions {
        locked: false,
        update: true,
        update_dep: None,
        resolution: jet::Publish::ResolveMode::Conservative,
    };
    let result2 = with_store(&store, || {
        jet::Fetch::fetch(&tmp, &mf, Some(&existing_lock), &update_opts)
    });
    assert!(
        result2.is_ok(),
        "update fetch should succeed: {:?}",
        result2.err()
    );

    // Capture the new rev from the lock file.
    let lock_raw2 =
        fs::read_to_string(tmp.join(".jet/lock")).expect(".jet/lock must exist after update");
    let new_rev = extract_rev_from_lock(&lock_raw2);
    assert!(
        !new_rev.is_empty(),
        "lock must contain a rev after update fetch"
    );

    assert_ne!(
        initial_rev, new_rev,
        "lock rev must change after a new commit was pushed"
    );

    let _ = fs::remove_dir_all(&tmp);
}

/// Extract the first `rev = "..."` value from a lock file string.
/// The lock format stores it as: `locked = { rev = "...", tree-hash = "...", ... }`
fn extract_rev_from_lock(lock_raw: &str) -> String {
    for line in lock_raw.lines() {
        // Look for the locked inline table line.
        if !line.contains("rev = \"") {
            continue;
        }
        // Find rev = "..."
        if let Some(after) = line.find("rev = \"") {
            let rest = &line[after + 7..]; // skip past 'rev = "'
            if let Some(end) = rest.find('"') {
                return rest[..end].to_string();
            }
        }
    }
    String::new()
}

// ─────────────────────────────────────────────
// CLI binary end-to-end
// ─────────────────────────────────────────────

#[test]
fn cli_jet_new_creates_project_structure() {
    assert!(
        jet_bin().is_file(),
        "the Cargo-provided jet binary must exist for this production-path test"
    );

    let tmp = tmp_dir("cli_new");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();

    let out = jet_cmd(&["new", "myapp"], &tmp, &store);
    assert!(
        out.status.success(),
        "jet new failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let proj = tmp.join("myapp");
    assert!(
        proj.join("package.jet").is_file(),
        "jet new must create package.jet"
    );
    assert!(
        proj.join("run.jet").is_file(),
        "jet new must create run.jet"
    );
    assert!(
        proj.join(".gitignore").is_file(),
        "jet new must create .gitignore"
    );

    let run = jet_cmd(&["run"], &proj, &store);
    assert!(
        run.status.success(),
        "new project must run without naming a file:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "hello, world\n");

    let check = jet_cmd(&["check"], &proj, &store);
    assert!(
        check.status.success(),
        "new project must check without naming a file:\n{}",
        String::from_utf8_lossy(&check.stderr)
    );

    let edited_source =
        "fn run() { print(\"edited\") }\n#Test(\"scaffold smoke\") { assert(true) }\n";
    write(&proj, "run.jet", edited_source);
    let test = jet_cmd(&["test"], &proj, &store);
    assert!(
        test.status.success(),
        "an edited project with a test must test without naming a file:\n{}",
        String::from_utf8_lossy(&test.stderr)
    );

    write(
        &proj,
        "run.jet",
        "fn run() { print(unknown_scaffold_name) }\n",
    );
    let invalid = jet_cmd(&["check"], &proj, &store);
    assert!(
        !invalid.status.success(),
        "invalid source must fail `jet check`"
    );
    assert!(
        String::from_utf8_lossy(&invalid.stderr).contains("Error ["),
        "invalid source lost its diagnostic:\n{}",
        String::from_utf8_lossy(&invalid.stderr)
    );
    write(&proj, "run.jet", edited_source);

    for args in [["run", "--profile=debug"], ["dev", "--watch=off"]] {
        let output = jet_cmd(&args, &proj, &store);
        assert!(
            output.status.success(),
            "newcomer command {args:?} failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "edited\n",
            "newcomer command {args:?} chose a different entry or tier"
        );
    }

    let update = jet_cmd(&["update", "jet"], &proj, &store);
    assert!(
        update.status.success(),
        "new project toolchain pin must update:\n{}",
        String::from_utf8_lossy(&update.stderr)
    );
    assert!(
        proj.join(".jet/lock").is_file(),
        "toolchain update must write .jet/lock"
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn cli_beginner_onboarding_ordered_workflow_uses_real_binary() {
    assert!(
        jet_bin().is_file(),
        "the Cargo-provided jet binary must exist for this production-path test"
    );

    let tmp = tmp_dir("cli_beginner_onboarding");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();

    let created = jet_cmd(&["new", "hello"], &tmp, &store);
    assert!(
        created.status.success(),
        "jet new failed:\n{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let project = tmp.join("hello");
    assert!(project.join("run.jet").is_file());
    assert!(project.join("package.jet").is_file());

    let first_run = jet_cmd(&["run"], &project, &store);
    assert!(first_run.status.success());
    assert_eq!(String::from_utf8_lossy(&first_run.stdout), "hello, world\n");

    let source = r#"fn greet(name: String) String -> {
    return "hello, {name}"
}

#Test("greet says hello") {
    assert_eq(greet("Jet"), "hello, Jet")
}

fn run() {
    print(greet("Jet"))
}
"#;
    write(&project, "run.jet", source);

    let check = jet_cmd(&["check"], &project, &store);
    assert!(
        check.status.success(),
        "jet check failed:\n{}",
        String::from_utf8_lossy(&check.stderr)
    );

    let tests = jet_cmd(&["test"], &project, &store);
    assert!(
        tests.status.success(),
        "jet test failed:\n{}",
        String::from_utf8_lossy(&tests.stderr)
    );
    let test_stdout = String::from_utf8_lossy(&tests.stdout);
    assert!(
        test_stdout.contains("greet says hello: pass"),
        "{test_stdout}"
    );
    assert!(
        test_stdout.contains("1 passed, 0 failed, 0 skipped"),
        "{test_stdout}"
    );

    let explicit_run = jet_cmd(&["run", "run.jet"], &project, &store);
    assert!(explicit_run.status.success());
    assert_eq!(
        String::from_utf8_lossy(&explicit_run.stdout),
        "hello, Jet\n"
    );

    write(
        &project,
        "run.jet",
        "print(\"before\")\nfn run() { print(\"middle\") }\nprint(\"after\")\n",
    );
    let invalid = jet_cmd(&["check"], &project, &store);
    assert!(!invalid.status.success());
    let invalid_stderr = String::from_utf8_lossy(&invalid.stderr);
    assert!(invalid_stderr.contains("E0621"), "{invalid_stderr}");

    let fixed = jet_cmd(&["fix", "run.jet"], &project, &store);
    assert!(
        fixed.status.success(),
        "jet fix failed:\n{}",
        String::from_utf8_lossy(&fixed.stderr)
    );
    assert!(String::from_utf8_lossy(&fixed.stdout).contains("applied 1 fix"));

    let fixed_check = jet_cmd(&["check"], &project, &store);
    assert!(fixed_check.status.success());

    let explanation = jet_cmd(&["explain", "E0621"], &project, &store);
    assert!(explanation.status.success());
    let explanation_stdout = String::from_utf8_lossy(&explanation.stdout);
    assert!(
        explanation_stdout.contains("What this means:"),
        "{explanation_stdout}"
    );
    assert!(
        explanation_stdout.contains("Why Jet enforces it:"),
        "{explanation_stdout}"
    );
    assert!(
        explanation_stdout.contains("How to fix it:"),
        "{explanation_stdout}"
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn cli_run_migrates_all_retired_entry_layouts() {
    assert!(
        jet_bin().is_file(),
        "the Cargo-provided jet binary must exist"
    );

    let tmp = tmp_dir("cli_run_entry_migration");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();

    for (label, retired, canonical) in [
        ("root", "main.jet", "run.jet"),
        ("src", "src/main.jet", "src/run.jet"),
        ("managed", ".jet/main.jet", "run.jet"),
    ] {
        let project = tmp.join(label);
        fs::create_dir_all(&project).unwrap();
        write(&project, "package.jet", &min_manifest(label, "0.1.0"));
        write(
            &project,
            retired,
            &format!("fn run() {{ print(\"{label}\") }}\n"),
        );

        let out = jet_cmd(&["run"], &project, &store);
        assert!(
            out.status.success(),
            "jet run failed for {label} layout:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&out.stdout), format!("{label}\n"));
        assert!(project.join(canonical).is_file());
        assert!(!project.join(retired).exists());
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("notice: migrated retired entry"),
            "{stderr}"
        );
        assert!(
            stderr.contains("main.jet") && stderr.contains("run.jet"),
            "{stderr}"
        );
    }

    fs::remove_dir_all(&tmp).unwrap();
}

#[test]
fn cli_run_reports_ambiguous_retired_entry_layout() {
    assert!(
        jet_bin().is_file(),
        "the Cargo-provided jet binary must exist"
    );

    let tmp = tmp_dir("cli_run_entry_ambiguous");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();
    write(&tmp, "package.jet", &min_manifest("ambiguous", "0.1.0"));
    write(&tmp, "run.jet", "fn run() { print(\"current\") }\n");
    write(&tmp, "main.jet", "fn run() { print(\"retired\") }\n");

    let out = jet_cmd(&["run"], &tmp, &store);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("ambiguous project entry"), "{stderr}");
    assert!(
        stderr.contains("main.jet") && stderr.contains("run.jet"),
        "{stderr}"
    );
    assert_eq!(stderr.matches("`run.jet`").count(), 1, "{stderr}");
    assert!(tmp.join("main.jet").is_file());
    assert!(tmp.join("run.jet").is_file());

    fs::remove_dir_all(&tmp).unwrap();
}

#[test]
fn cli_run_reports_named_canonical_retired_entry_layout() {
    assert!(
        jet_bin().is_file(),
        "the Cargo-provided jet binary must exist"
    );

    let tmp = tmp_dir("cli_run_named_entry_ambiguous");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();
    write(&tmp, "package.jet", &min_manifest("ambiguous", "0.1.0"));
    write(&tmp, "ambiguous.jet", "fn run() { print(\"current\") }\n");
    write(&tmp, "main.jet", "fn run() { print(\"retired\") }\n");

    let out = jet_cmd(&["run"], &tmp, &store);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("ambiguous project entry"), "{stderr}");
    assert!(
        stderr.contains("main.jet") && stderr.contains("ambiguous.jet"),
        "{stderr}"
    );
    assert!(tmp.join("main.jet").is_file());
    assert!(tmp.join("ambiguous.jet").is_file());

    fs::remove_dir_all(&tmp).unwrap();
}

#[test]
fn cli_run_keeps_explicit_retired_entry_target() {
    assert!(
        jet_bin().is_file(),
        "the Cargo-provided jet binary must exist"
    );

    let tmp = tmp_dir("cli_run_explicit_retired");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();
    write(&tmp, "main.jet", "fn run() { print(\"explicit\") }\n");

    let out = jet_cmd(&["run", "main.jet"], &tmp, &store);
    assert!(
        out.status.success(),
        "explicit source target failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "explicit\n");
    assert!(tmp.join("main.jet").is_file());
    assert!(!tmp.join("run.jet").exists());

    fs::remove_dir_all(&tmp).unwrap();
}

#[test]
fn unsupported_authority_reports_a_host_recovery() {
    let diagnostic = jet::Authority::AuthorityError::Unsupported(
        "descriptor-relative no-follow authority is unavailable on this platform".to_string(),
    )
    .diagnostic();
    assert_eq!(diagnostic.code, "E1334");
    assert!(diagnostic.what.contains("unavailable on this platform"));
    assert!(diagnostic.why.contains("descriptor-relative no-follow"));
    assert!(diagnostic.fix.contains("use a platform"));
}

#[cfg(unix)]
#[test]
fn cli_run_authority_failure_offers_specific_recovery() {
    use std::os::unix::fs::symlink;

    let tmp = tmp_dir("cli_run_authority_recovery");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();
    write(&tmp, "package.jet", &min_manifest("authority", "0.1.0"));
    write(&tmp, "real.jet", "fn run() { print(\"real\") }\n");
    symlink("real.jet", tmp.join("run.jet")).unwrap();

    let out = jet_cmd(&["run"], &tmp, &store);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Error [E1334]:"), "{stderr}");
    assert!(
        stderr.contains("run.jet") && stderr.contains("symlink"),
        "{stderr}"
    );
    assert!(
        stderr.contains("replace the symlink with the expected regular file"),
        "{stderr}"
    );

    fs::remove_dir_all(&tmp).unwrap();
}

#[test]
fn cli_build_sbom_writes_spdx() {
    // D-SUPPLY1: `jet build --sbom` writes an SPDX file next to the binary.
    if !jet_bin().is_file() {
        eprintln!("note: skipping cli_build_sbom (run `cargo build` first)");
        return;
    }
    let tmp = tmp_dir("cli_build_sbom");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();
    write(&tmp, "hello.jet", "fn run() { print(\"hi\"); }\n");

    let out = jet_cmd(&["build", "--sbom", "hello.jet"], &tmp, &store);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "jet build --sbom failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("sbom:"),
        "build output must announce the SBOM path"
    );

    // The SBOM lands beside the produced binary as <bin>.spdx.
    let spdx = tmp.join("build/hello.spdx");
    assert!(spdx.is_file(), "expected SBOM at {}", spdx.display());
    let body = fs::read_to_string(&spdx).unwrap();
    assert!(
        body.starts_with("SPDXVersion: SPDX-2.3\n"),
        "SBOM must be SPDX 2.3"
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn cli_vendor_dir_flag_relocates() {
    // D-SUPPLY1: `--vendor-dir` writes the vendor tree to a chosen location.
    if !jet_bin().is_file() || !have_git() {
        eprintln!("note: skipping cli_vendor_dir (need built binary)");
        return;
    }
    let tmp = tmp_dir("cli_vendor_dir");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();

    write(
        &tmp,
        "greeter/package.jet",
        &min_manifest("greeter", "0.1.0"),
    );
    write(
        &tmp,
        "greeter/greeter.jet",
        "pub fn greet() => String { return \"hi\"; }\n",
    );
    write(
        &tmp,
        "package.jet",
        &manifest_with_deps("app", "0.1.0", "    greeter: ./greeter,"),
    );

    let out = jet_cmd(
        &["registry", "vendor", "--vendor-dir", "third_party"],
        &tmp,
        &store,
    );
    assert!(
        out.status.success(),
        "jet registry vendor --vendor-dir failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        tmp.join("third_party/greeter").is_dir(),
        "dep must land in the chosen dir"
    );
    assert!(
        tmp.join("third_party/manifest.json").is_file(),
        "vendor manifest must be written"
    );
    assert!(
        !tmp.join("vendor").exists(),
        "default vendor/ must not be created"
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn cli_jet_new_annotated_has_dep_comments() {
    if !jet_bin().is_file() {
        eprintln!("note: skipping cli_jet_new_annotated (run `cargo build` first)");
        return;
    }

    let tmp = tmp_dir("cli_annotated");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();

    jet_cmd(&["new", "annotated_app", "--annotated"], &tmp, &store);

    let manifest = fs::read_to_string(tmp.join("annotated_app/package.jet"))
        .expect("package.jet must exist after jet new --annotated");
    assert!(
        manifest.contains("// Jet package dependencies:"),
        "annotated template should have dep comments:\n{}",
        manifest
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn cli_end_to_end_new_then_add_path() {
    if !jet_bin().is_file() {
        eprintln!("note: skipping cli_end_to_end (run `cargo build` first)");
        return;
    }

    let tmp = tmp_dir("e2e");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();

    // 1. jet new myapp
    let out = jet_cmd(&["new", "myapp"], &tmp, &store);
    assert!(
        out.status.success(),
        "jet new failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // 2. Create a local lib for `jet add --path`.
    let lib = tmp.join("mylib");
    write(&lib, "package.jet", &min_manifest("mylib", "0.1.0"));
    write(&lib, "mylib.jet", "pub fn answer() => Int { return 42; }\n");

    // 3. jet add mylib --path ../mylib (from inside the project)
    let proj = tmp.join("myapp");
    let out = jet_cmd(&["add", "mylib", "--path", "../mylib"], &proj, &store);
    assert!(
        out.status.success(),
        "jet add --path failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // package.jet must now reference mylib.
    let manifest = fs::read_to_string(proj.join("package.jet")).unwrap();
    assert!(
        manifest.contains("mylib"),
        "package.jet should list mylib after jet add"
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn cli_add_path_into_inline_empty_deps_table() {
    if !jet_bin().is_file() {
        eprintln!(
            "note: skipping cli_add_path_into_inline_empty_deps_table (run `cargo build` first)"
        );
        return;
    }

    let tmp = tmp_dir("cli_add_inline_deps");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();
    write(
        &tmp,
        "package.jet",
        &(min_manifest("app", "0.1.0") + "\ndeps: .{}\n"),
    );
    let lib = tmp.join("mylib");
    write(&lib, "package.jet", &min_manifest("mylib", "0.1.0"));
    write(&lib, "mylib.jet", "pub fn answer() => Int { return 42; }\n");

    let out = jet_cmd(&["add", "mylib", "--path", "./mylib"], &tmp, &store);
    assert!(
        out.status.success(),
        "jet add --path failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let raw = fs::read_to_string(tmp.join("package.jet")).unwrap();
    let manifest = jet::Manifest::parse(&tmp.join("package.jet"), &raw)
        .expect("jet add must leave an inline deps table parseable");
    assert!(manifest.dependencies.contains_key("mylib"));
    assert!(raw.contains("deps: .{\n    mylib: ./mylib,\n}\n"));

    let fetch = jet_cmd(&["fetch"], &tmp, &store);
    assert!(
        fetch.status.success(),
        "fetch after add failed:\n{}",
        String::from_utf8_lossy(&fetch.stderr)
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn cli_remove_absent_dependency_is_an_error() {
    if !jet_bin().is_file() {
        eprintln!(
            "note: skipping cli_remove_absent_dependency_is_an_error (run `cargo build` first)"
        );
        return;
    }

    let tmp = tmp_dir("cli_remove_absent");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();
    let manifest_path = tmp.join("package.jet");
    let before = min_manifest("app", "0.1.0");
    write(&tmp, "package.jet", &before);

    let out = jet_cmd(&["remove", "json"], &tmp, &store);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "absent dependency removal must fail");
    assert!(
        stderr.contains("json") && stderr.contains("not present"),
        "missing absent-dep diagnostic:\n{stderr}"
    );
    assert!(
        !stdout.contains("removed") && !stdout.contains("fetched"),
        "false success output:\n{stdout}"
    );
    assert_eq!(fs::read_to_string(manifest_path).unwrap(), before);

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn cli_corrupt_manifest_has_one_diagnostic_across_package_commands() {
    if !jet_bin().is_file() {
        eprintln!("note: skipping cli_corrupt_manifest_has_one_diagnostic_across_package_commands (run `cargo build` first)");
        return;
    }

    let tmp = tmp_dir("cli_corrupt_manifest");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();
    write(
        &tmp,
        "package.jet",
        &(min_manifest("app", "0.1.0") + "\ndeps: .{\n}\nunknown: ../broken,\n"),
    );
    write(&tmp, "run.jet", "fn run() { print(\"hi\"); }\n");
    let lib = tmp.join("mylib");
    write(&lib, "package.jet", &min_manifest("mylib", "0.1.0"));

    let outputs = [
        jet_cmd(&["fetch"], &tmp, &store),
        jet_cmd(&["build", "run.jet"], &tmp, &store),
        jet_cmd(&["add", "mylib", "--path", "./mylib"], &tmp, &store),
    ];
    let lines: Vec<String> = outputs
        .iter()
        .map(|out| {
            assert!(
                !out.status.success(),
                "corrupt manifest command unexpectedly succeeded"
            );
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .find(|line| line.starts_with("Error [E1206]:"))
                .unwrap_or("")
                .to_string()
        })
        .collect();
    assert!(
        lines.iter().all(|line| !line.is_empty()),
        "missing E1206 diagnosis: {lines:?}"
    );
    assert!(
        lines.windows(2).all(|pair| pair[0] == pair[1]),
        "inconsistent diagnostics: {lines:?}"
    );

    let _ = fs::remove_dir_all(&tmp);
}

// ─────────────────────────────────────────────
// E2-M8: SemVer + supply-chain tests
// ─────────────────────────────────────────────

#[test]
fn semver_break_e2601() {
    // A minor bump that removes a public API item must produce E2601.
    use jet::Publish::{diff_public_api, e2601, ApiItem, BumpKind};

    let old_api = vec![ApiItem {
        kind: "fn".into(),
        name: "parse".into(),
        signature: "fn parse(raw: String) Int".into(),
    }];
    let new_api: Vec<ApiItem> = vec![]; // removed

    let changes = diff_public_api(&old_api, &new_api);
    assert!(
        !changes.is_empty(),
        "removed pub fn must be a breaking change"
    );

    let diag = e2601("1.2.0", BumpKind::Minor, &changes[0], 2);
    assert_eq!(diag.code, "E2601");
    assert!(diag.what.contains("1.2.0"), "what must name the version");
    assert!(diag.why.contains("minor"), "why must name the bump kind");
    assert!(diag.fix.contains("2.0.0"), "fix must name the next major");
    assert!(
        diag.why.contains("parse") || diag.why.contains("removed"),
        "why must name the broken item or action"
    );
}

#[test]
fn returned_view_source_union_change_feeds_e1218_and_e2601() {
    use jet::Publish::SemVer::SemVer;
    use jet::Publish::{classify_bump, diff_public_api, e1218, e2601, ApiItem, BumpKind};

    let item = |source: &str| ApiItem {
        kind: "fn".into(),
        name: "pick".into(),
        signature: format!("fn pick(left: [Int], right: [Int]) View<Int> ; view_source = {source}"),
    };
    let changes = diff_public_api(
        &[item("parameter:0;access:read;path:range")],
        &[item(
            "one_of(parameter:0;path:range,parameter:1;path:range);access:read",
        )],
    );
    assert_eq!(
        changes.len(),
        1,
        "adding a possible owner to a source union must be breaking"
    );
    assert!(changes[0].description.contains("parameter:0"));
    assert!(changes[0].description.contains("parameter:1"));

    let bump = classify_bump(
        &SemVer::parse("1.0.0").unwrap(),
        &SemVer::parse("1.1.0").unwrap(),
    );
    assert_eq!(bump, BumpKind::Minor);
    assert_eq!(e1218("1.0.0", "1.1.0", bump, &changes[0], 2).code, "E1218");
    assert_eq!(e2601("1.1.0", bump, &changes[0], 2).code, "E2601");
}

#[test]
fn capability_sigil_frozen_in_public_api() {
    // c129 (D-MEM1, was D-CAP7/D-CAP8): the resolved capability sigil is part of
    // a pub fn's published signature, and a read -> write drift is a breaking change.
    use jet::Publish::{diff_public_api, extract_public_api};

    let dir = tmp_dir("cap_api_freeze");

    let write_src = "\
struct Account { balance: Int }
pub fn deposit(a: &Account, amount: Int) Int -> {
    a.balance = a.balance + amount
    return a.balance
}
";
    let f = dir.join("write.jet");
    fs::write(&f, write_src).unwrap();
    let write_api = extract_public_api(write_src, f.to_str().unwrap());
    let deposit = write_api
        .iter()
        .find(|i| i.name == "deposit")
        .expect("deposit must be in the public API");
    assert!(
        deposit.signature.contains("a: &"),
        "the write sigil must be frozen onto the param type in the published signature, got `{}`",
        deposit.signature
    );

    // Same signature, only the capability sigil differs (read instead of write).
    let read_src = "\
struct Account { balance: Int }
pub fn deposit(a: Account, amount: Int) Int -> { return a.balance + amount }
";
    let f2 = dir.join("read.jet");
    fs::write(&f2, read_src).unwrap();
    let read_api = extract_public_api(read_src, f2.to_str().unwrap());

    let changes = diff_public_api(&read_api, &write_api);
    assert!(
        !changes.is_empty(),
        "a read -> write capability drift on a pub fn must be a breaking change"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn cli_typed_default_is_not_frozen_as_positional() {
    let dir = tmp_dir("cli_default_api_freeze");
    let src = "#CLI\npub struct Config {\n    port: Int{3000}\n    required: Int\n}\n";
    let path = dir.join("config.jet");
    fs::write(&path, src).unwrap();
    let api = jet::Publish::extract_public_api_for_package(src, path.to_str().unwrap(), "config");
    let config = api
        .iter()
        .find(|item| item.name == "Config")
        .expect("public CLI struct must be frozen");
    assert_eq!(
        config.signature,
        "struct Config { port: Int; required: Int [positional 0] }"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn physical_unit_api_freeze_and_semver_share_one_canonical_signature() {
    use jet::Publish::{diff_public_api, ApiItem};

    let dir = tmp_dir("physical_unit_api_freeze");
    let current = "#UnitFamily(Length, base: meter) { meter millimeter(scale: 1/1000) }\npub fn distance() Millimeter -> { return Millimeter.from_float(1.0) }\n";
    let current_path = dir.join("current.jet");
    fs::write(&current_path, current).unwrap();
    let current_api = jet::Publish::extract_public_api_for_package(
        current,
        current_path.to_str().unwrap(),
        "physics",
    );
    let mut bundle =
        jet::Loader::load_entry_with_overlay(current_path.to_str().unwrap(), None, true).unwrap();
    let (_, facts) =
        jet::Sema::check_bundle_with_effect_facts(&mut bundle, jet::Sema::CompileMode::Check);
    let entry = &bundle.modules[bundle.entry];
    let frozen = jet::Publish::ApiFreeze::snapshot_from_items_with_effects(
        &entry.items,
        "physics",
        "1.0.0",
        Some(&facts.solved),
        Some(&entry.alias),
    );
    assert_eq!(
        frozen.api_version,
        jet::Publish::ApiFreeze::API_SNAPSHOT_VERSION
    );
    let frozen_api: Vec<ApiItem> = frozen
        .funcs
        .iter()
        .map(|function| ApiItem {
            kind: "fn".into(),
            name: function.name.clone(),
            signature: function.signature.clone(),
        })
        .collect();
    assert!(diff_public_api(&frozen_api, &current_api).is_empty());

    let changed = "#UnitFamily(Length, base: meter) { meter millimeter(scale: 1/100) }\npub fn distance() Millimeter -> { return Millimeter.from_float(1.0) }\n";
    let changed_path = dir.join("changed.jet");
    fs::write(&changed_path, changed).unwrap();
    let changed_api = jet::Publish::extract_public_api_for_package(
        changed,
        changed_path.to_str().unwrap(),
        "physics",
    );
    assert_eq!(diff_public_api(&current_api, &changed_api).len(), 1);
    let foreign_api = jet::Publish::extract_public_api_for_package(
        current,
        current_path.to_str().unwrap(),
        "geometry",
    );
    assert_eq!(diff_public_api(&current_api, &foreign_api).len(), 1);

    let affine = "#UnitFamily(Temperature, base: kelvin) { kelvin celsius(scale: 1, offset: 27315/100) }\npub fn target() CelsiusPoint -> { return CelsiusPoint.from_float(20.0) }\n";
    let affine_path = dir.join("affine.jet");
    fs::write(&affine_path, affine).unwrap();
    let affine_api = jet::Publish::extract_public_api_for_package(
        affine,
        affine_path.to_str().unwrap(),
        "physics",
    );
    let shifted = "#UnitFamily(Temperature, base: kelvin) { kelvin celsius(scale: 1, offset: 27415/100) }\npub fn target() CelsiusPoint -> { return CelsiusPoint.from_float(20.0) }\n";
    let shifted_path = dir.join("shifted.jet");
    fs::write(&shifted_path, shifted).unwrap();
    let shifted_api = jet::Publish::extract_public_api_for_package(
        shifted,
        shifted_path.to_str().unwrap(),
        "physics",
    );
    assert_eq!(diff_public_api(&affine_api, &shifted_api).len(), 1);

    let length_generic =
        "pub fn keep<Q: Quantity<Length, .Linear>>(value: ^Q) Q -> { return value }\n";
    let time_generic = "pub fn keep<Q: Quantity<Time, .Linear>>(value: ^Q) Q -> { return value }\n";
    let length_path = dir.join("length_generic.jet");
    let time_path = dir.join("time_generic.jet");
    fs::write(&length_path, length_generic).unwrap();
    fs::write(&time_path, time_generic).unwrap();
    let length_api = jet::Publish::extract_public_api_for_package(
        length_generic,
        length_path.to_str().unwrap(),
        "physics",
    );
    let time_api = jet::Publish::extract_public_api_for_package(
        time_generic,
        time_path.to_str().unwrap(),
        "physics",
    );
    assert_eq!(
        length_api[0].signature,
        "fn keep<Q: Quantity<Length, .Linear>>(value: ^Q) Q -[]>"
    );
    assert_eq!(diff_public_api(&length_api, &time_api).len(), 1);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn soft_public_names_are_not_semver_promises() {
    use jet::Publish::{diff_public_api, extract_public_api};

    let dir = tmp_dir("soft_public_api");
    let old = "pub fn stable() {}\npub fn _unstable() {}\n";
    let new = "pub fn stable() {}\n";
    let old_path = dir.join("old.jet");
    let new_path = dir.join("new.jet");
    fs::write(&old_path, old).unwrap();
    fs::write(&new_path, new).unwrap();

    let old_api = extract_public_api(old, old_path.to_str().unwrap());
    let new_api = extract_public_api(new, new_path.to_str().unwrap());
    assert!(old_api.iter().all(|item| item.name != "_unstable"));
    assert!(diff_public_api(&old_api, &new_api).is_empty());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn inferred_public_effect_drift_is_breaking() {
    use jet::Publish::{diff_public_api, extract_public_api};

    let dir = tmp_dir("effect_api_drift");
    let pure_path = dir.join("pure.jet");
    let io_path = dir.join("io.jet");
    let pure = "pub fn report() Int -> { return 1 }\n";
    let io = "pub fn report() Int -> { print(\"report\"); return 1 }\n";
    fs::write(&pure_path, pure).unwrap();
    fs::write(&io_path, io).unwrap();

    let pure_api = extract_public_api(pure, pure_path.to_str().unwrap());
    let io_api = extract_public_api(io, io_path.to_str().unwrap());
    assert!(pure_api[0].signature.contains("-[]>"));
    assert!(io_api[0].signature.contains("-[IO]>"));
    assert_eq!(diff_public_api(&pure_api, &io_api).len(), 1);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn inferred_inline_module_effects_are_published() {
    use jet::Publish::{diff_public_api, extract_public_api};

    let dir = tmp_dir("inline_module_effect_api");
    let old_dir = dir.join("old");
    let new_dir = dir.join("new");
    fs::create_dir_all(&old_dir).unwrap();
    fs::create_dir_all(&new_dir).unwrap();
    let old_path = old_dir.join("run.jet");
    let new_path = new_dir.join("run.jet");
    let old = "module files { pub fn report() { print(\"report\"); } }\nmodule bench { pub fn report() {} }\n";
    let new = "module files { pub fn report() { print(\"report\"); } }\nmodule bench { pub fn report() { print(\"bench\"); } }\n";
    fs::write(&old_path, old).unwrap();
    fs::write(&new_path, new).unwrap();

    let old_api = extract_public_api(old, old_path.to_str().unwrap());
    let new_api = extract_public_api(new, new_path.to_str().unwrap());
    let report = old_api
        .iter()
        .find(|item| item.name == "files.report")
        .expect("inline module function is public API");
    assert!(report.signature.contains("-[IO]>"), "{}", report.signature);
    assert!(old_api.iter().any(|item| item.name == "bench.report"));
    assert_eq!(diff_public_api(&old_api, &new_api).len(), 1);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn v2_snapshot_upgrade_matches_duplicate_inline_leaves() {
    use jet::Publish::ApiFreeze::{legacy_api_name, legacy_api_signature, ApiSnapshot};
    use jet::Publish::{diff_public_api, extract_public_api, ApiItem};

    let dir = tmp_dir("v2_inline_api_upgrade");
    let path = dir.join("current.jet");
    let source = "module bench { pub fn report() {} }\nmodule files { pub fn report(x: Int) {} }\n";
    fs::write(&path, source).unwrap();
    let mut current = extract_public_api(source, path.to_str().unwrap())
        .into_iter()
        .filter(|item| item.kind == "fn")
        .collect::<Vec<_>>();
    for item in &mut current {
        item.name = legacy_api_name(&item.name).to_string();
        item.signature = legacy_api_signature(&item.signature);
    }

    let previous = ApiSnapshot::parse(
        "api_version = 2\npackage = demo\npublished_version = 1.0.0\nfn report()\nfn report(x: Int (a whole number))\n",
    )
    .expect("v2 snapshot");
    let mut old = previous
        .funcs
        .iter()
        .map(|function| ApiItem {
            kind: "fn".to_string(),
            name: function.name.clone(),
            signature: function.signature.clone(),
        })
        .collect::<Vec<_>>();
    for item in &mut old {
        item.name = legacy_api_name(&item.name).to_string();
        item.signature = legacy_api_signature(&item.signature);
    }
    assert!(
        diff_public_api(&old, &current).is_empty(),
        "old={old:#?}\ncurrent={current:#?}"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn public_effect_metadata_preserves_symbolic_rows() {
    use jet::Publish::extract_public_api;

    let dir = tmp_dir("effect_api_open_row");
    let path = dir.join("open.jet");
    let source = "pub fn invoke<E>(act: fn() Int -[..E]>) Int -[..E]> { return act(); }\n";
    fs::write(&path, source).unwrap();

    let api = extract_public_api(source, path.to_str().unwrap());
    assert!(api[0].signature.contains("-[..E]>"), "{}", api[0].signature);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn public_trait_effect_contract_drift_is_breaking() {
    use jet::Publish::{diff_public_api, extract_public_api};

    let dir = tmp_dir("trait_effect_api_drift");
    let old_path = dir.join("old.jet");
    let new_path = dir.join("new.jet");
    let old = "pub trait Render { fn draw(self) Int -[IO]>; }\n";
    let new = "pub trait Render { fn draw(self) Int -[GPU]>; }\n";
    fs::write(&old_path, old).unwrap();
    fs::write(&new_path, new).unwrap();

    let old_api = extract_public_api(old, old_path.to_str().unwrap());
    let new_api = extract_public_api(new, new_path.to_str().unwrap());
    assert!(old_api[0].signature.contains("-[IO]>"));
    assert!(new_api[0].signature.contains("-[GPU]>"));
    assert!(
        !diff_public_api(&old_api, &new_api).is_empty(),
        "trait method effect drift must be breaking"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn physical_unit_trait_methods_use_canonical_dimensions() {
    use jet::Publish::extract_public_api;

    let dir = tmp_dir("physical_unit_trait_api");
    let path = dir.join("current.jet");
    // No local `#UnitFamily(Length)` here: D-DIMENSION-OPEN1=D says a
    // same-named local declaration shadows the standard Prelude catalog
    // (`Bundle/Units.rs`'s "Local names shadow Prelude members; physical
    // dimension behavior remains explicit opt-in"), so it would stay
    // nominal and could never carry `core.units::Length`. Referencing
    // `Meter` bare lets the ambient standard-unit prelude supply the real,
    // canonical Length family instead (card #1765/#1769 root cause: the
    // prior fixture redeclared the family and shadowed the very identity
    // it meant to assert on).
    let source = "pub trait Measure { fn scale(value: Meter) Meter; }\n";
    fs::write(&path, source).unwrap();

    let api = extract_public_api(source, path.to_str().unwrap());
    let method = api
        .iter()
        .find(|item| item.name == "Measure.scale")
        .expect("public trait method");
    assert_eq!(
        method.signature,
        "fn Measure.scale(value: Meter{package=core.units; family=Length; base=Meter; dimension=core.units%3A%3ALength:1; scale=1; provenance=Rational; offset=0}) Meter{package=core.units; family=Length; base=Meter; dimension=core.units%3A%3ALength:1; scale=1; provenance=Rational; offset=0}"
    );

    let mut bundle = jet::Loader::load_entry_with_overlay(path.to_str().unwrap(), None, true)
        .expect("trait source bundle");
    let (_, facts) =
        jet::Sema::check_bundle_with_effect_facts(&mut bundle, jet::Sema::CompileMode::Check);
    let entry = &bundle.modules[bundle.entry];
    let frozen = jet::Publish::ApiFreeze::snapshot_from_items_with_effects(
        &entry.items,
        "physics",
        "1.0.0",
        Some(&facts.solved),
        Some(&entry.alias),
    );
    assert_eq!(
        frozen
            .funcs
            .iter()
            .find(|item| item.name == "Measure.scale")
            .expect("frozen public trait method")
            .signature,
        method.signature
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn resolver_conflict_e2602() {
    // Two packages requiring incompatible versions of a shared dep → E2602.
    use jet::Publish::{check_conflicts, VersionConstraint, VersionReq};
    use std::collections::BTreeMap;

    let constraints = vec![
        VersionConstraint {
            package: "log".into(),
            req: VersionReq::parse("^1.0").unwrap(),
            from: "web-server 2.1.0".into(),
        },
        VersionConstraint {
            package: "log".into(),
            req: VersionReq::parse("^2.0").unwrap(),
            from: "db-client 3.0.0".into(),
        },
    ];
    let diags = check_conflicts(&constraints, &BTreeMap::new());
    assert!(
        !diags.is_empty(),
        "disjoint major caret ranges must be a conflict"
    );
    assert_eq!(diags[0].code, "E2602");
    let why = &diags[0].why;
    assert!(why.contains("log"), "why must name the conflicting package");
    assert!(
        why.contains("web-server") || why.contains("db-client"),
        "why must name a dependent"
    );
}

#[test]
fn vendored_offline_locked_build() {
    // --locked on a project with a lock file and a vendored dep must succeed.
    // Without network this verifies the offline path works.
    let tmp = tmp_dir("m8_vendor_locked");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();

    // Create a simple library.
    write(
        &tmp,
        "greeter/package.jet",
        &min_manifest("greeter", "0.1.0"),
    );
    write(
        &tmp,
        "greeter/greeter.jet",
        "pub fn greet() => String { return \"hi\"; }\n",
    );

    // Project that depends on it.
    write(
        &tmp,
        "package.jet",
        &manifest_with_deps("vendored_app", "0.1.0", "    greeter: ./greeter,"),
    );
    write(
        &tmp,
        "run.jet",
        "use greeter;\nfn run() { print(greeter.greet()); }\n",
    );

    let entry = tmp.join("run.jet");
    let pack_path = tmp.join("package.jet");

    // Fetch to create the lock.
    let mf = jet::Manifest::parse(&pack_path, &fs::read_to_string(&pack_path).unwrap()).unwrap();
    let opts = jet::Fetch::FetchOptions {
        locked: false,
        update: false,
        update_dep: None,
        resolution: jet::Publish::ResolveMode::Conservative,
    };
    let result = with_store(&store, || jet::Fetch::fetch(&tmp, &mf, None, &opts));
    assert!(result.is_ok(), "initial fetch must succeed");
    let (lock, dep_dirs) = result.unwrap();

    // Vendor the dependency.
    let vendor_dir = tmp.join("vendor");
    let vendor_result = jet::Publish::vendor(&tmp, &lock, &dep_dirs, &vendor_dir);
    assert!(vendor_result.is_ok(), "vendor must succeed");
    let copied = vendor_result.unwrap();
    assert!(
        copied.contains(&"greeter".to_string()),
        "greeter must be vendored"
    );
    assert!(
        tmp.join("vendor/greeter").is_dir(),
        "vendor/greeter must exist"
    );
    // D-SUPPLY1: a vendor manifest records each dep's name/version/fingerprint.
    let manifest = fs::read_to_string(tmp.join("vendor/manifest.json")).unwrap();
    assert!(
        manifest.contains("\"name\": \"greeter\""),
        "manifest must list greeter"
    );
    assert!(
        manifest.contains("\"fingerprint\""),
        "manifest must record fingerprints"
    );

    // With the lock present, --locked fetch succeeds (no network needed).
    let lock_text = fs::read_to_string(tmp.join(".jet/lock")).unwrap_or_default();
    assert!(!lock_text.is_empty(), "lock file must exist after fetch");

    // Compile the project (uses the in-store copy, not network).
    let compile_result = with_store(&store, || {
        jet::compile_with_path("", &entry.to_string_lossy())
    });
    assert!(
        compile_result.is_ok(),
        "vendored project must compile offline"
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[cfg(unix)]
#[test]
fn vendor_rejects_symlink_sources_and_traversal_names() {
    use std::collections::HashMap;
    use std::os::unix::fs::symlink;

    let tmp = tmp_dir("vendor_symlink_boundary");
    let outside = tmp.join("outside");
    fs::write(&outside, "must survive\n").unwrap();
    let source = tmp.join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("package.jet"), "name: \"dep\"\n").unwrap();
    symlink(&outside, source.join("leak")).unwrap();

    let mut symlink_deps = HashMap::new();
    symlink_deps.insert("dep".to_string(), source.clone());
    let symlink_result = jet::Publish::vendor(
        &tmp,
        &make_test_lock("dep", "0.1.0", "sha256-dep"),
        &symlink_deps,
        &tmp.join("vendor-symlink"),
    );
    assert!(symlink_result.is_err(), "vendor must refuse source symlinks");
    assert_eq!(fs::read_to_string(&outside).unwrap(), "must survive\n");

    let safe_source = tmp.join("safe-source");
    fs::create_dir_all(&safe_source).unwrap();
    fs::write(safe_source.join("package.jet"), "name: \"escape\"\n").unwrap();
    let mut traversal_deps = HashMap::new();
    traversal_deps.insert("../escape".to_string(), safe_source);
    let traversal_result = jet::Publish::vendor(
        &tmp,
        &make_test_lock("../escape", "0.1.0", "sha256-escape"),
        &traversal_deps,
        &tmp.join("vendor-name"),
    );
    assert!(
        traversal_result.is_err(),
        "vendor must refuse traversal-shaped dependency names"
    );
    assert!(!tmp.join("escape").exists());
    let _ = fs::remove_dir_all(&tmp);
}

fn make_test_lock(name: &str, version: &str, fp: &str) -> jet::Lock::LockFile {
    use jet::Lock::{LockFile, LockSource, LockedPackage};
    LockFile {
        version: 1,
        packages: vec![LockedPackage {
            name: name.into(),
            version: version.into(),
            fingerprint: fp.into(),
            content_hash: None,
            source: LockSource::Path("/tmp/placeholder".into()),
            locked: None,
            dependencies: vec![],
            layer: None,
            inferred_layer: None,

            effects: vec![],

            effect_grants: vec![],
            required_effects: vec![],
            granted_effects: vec![],
            denied_effects: vec![],
            effect_authority: None,
            envelope: None,
            receipt: None,
            provenance: None,
        }],
        root_dependencies: vec![name.into()],
        authority: None,
        workspace_members: vec![],
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
fn sbom_spdx_golden() {
    // SBOM emitted from a known lockfile has the expected SPDX structure.
    use jet::Publish::emit_spdx;

    let lock = make_test_lock(
        "logger",
        "1.2.3",
        "sha256-deadbeef00112233445566778899aabbccddeeff00112233445566778899aabb",
    );
    let sbom = emit_spdx(&lock, "myapp", "0.5.0");
    // Golden structure checks (not full byte comparison because Created: timestamp varies).
    assert!(sbom.starts_with("SPDXVersion: SPDX-2.3\n"));
    assert!(sbom.contains("DataLicense: CC0-1.0\n"));
    assert!(sbom.contains("DocumentNamespace: https://jet-lang.dev/spdx/myapp-0.5.0-"));
    assert!(sbom.contains("PackageName: logger\n"));
    assert!(sbom.contains("PackageVersion: 1.2.3\n"));
    assert!(sbom.contains("PackageChecksum: SHA256: deadbeef00112233445566778899aabbccddeeff00112233445566778899aabb\n"));
    assert!(sbom.contains("Relationship: SPDXRef-root DEPENDS_ON SPDXRef-pkg-0\n"));
}

#[test]
fn sbom_cyclonedx_golden() {
    use jet::Publish::emit_cyclonedx;

    let lock = make_test_lock(
        "logger",
        "1.2.3",
        "sha256-deadbeef00112233445566778899aabbccddeeff00112233445566778899aabb",
    );
    let sbom = emit_cyclonedx(&lock, "myapp", "0.5.0");
    assert!(sbom.contains("\"bomFormat\": \"CycloneDX\""));
    assert!(sbom.contains("\"specVersion\": \"1.5\""));
    assert!(sbom.contains("\"name\": \"logger\""));
    assert!(sbom.contains("\"version\": \"1.2.3\""));
    assert!(sbom.contains("\"alg\": \"SHA-256\""));
    assert!(sbom.contains("deadbeef00112233445566778899aabbccddeeff00112233445566778899aabb"));
}

#[test]
fn audit_e2603_on_vulnerable_dep() {
    use jet::Publish::Advisory::Advisory;
    use jet::Publish::SemVer::SemVer;
    use jet::Publish::{audit_lockfile, Severity, VersionReq};

    let lock = make_test_lock("crypto-lib", "0.9.0", "sha256-aabb");
    // Advisory: crypto-lib ^0 (pre-1.0) has a critical issue fixed in 0.9.5.
    let advisories = vec![Advisory {
        id: "JET-2026-SEC-001".into(),
        package: "crypto-lib".into(),
        affected: VersionReq::parse("^0").unwrap(),
        fixed: Some(SemVer::parse("0.9.5").unwrap()),
        title: "Timing side-channel in AES-GCM".into(),
        severity: Severity::Critical,
    }];
    let matches = audit_lockfile(&lock, &advisories).unwrap();

    assert_eq!(matches.len(), 1, "one advisory match expected");
    let d = &matches[0].diagnostic;
    assert_eq!(d.code, "E2603");
    assert!(d.what.contains("JET-2026-SEC-001"));
    assert!(d.what.contains("crypto-lib"));
    assert!(d.what.contains("Timing side-channel"));
    assert!(
        d.what.contains("[critical]"),
        "severity must prefix the message"
    );
    assert_eq!(matches[0].severity, jet::Publish::Severity::Critical);
}

#[test]
fn audit_non_critical_is_advisory() {
    // D-SUPPLY1: a non-critical advisory still matches but is advisory-only —
    // the severity carried back is below Critical, so `jet inspect audit` exits 0.
    use jet::Publish::Advisory::Advisory;
    use jet::Publish::SemVer::SemVer;
    use jet::Publish::{audit_lockfile, Severity, VersionReq};

    let lock = make_test_lock("util-lib", "1.0.0", "sha256-ccdd");
    let advisories = vec![Advisory {
        id: "JET-2026-INFO-1".into(),
        package: "util-lib".into(),
        affected: VersionReq::parse("^1").unwrap(),
        fixed: Some(SemVer::parse("1.0.2").unwrap()),
        title: "Minor info leak in debug logs".into(),
        severity: Severity::Low,
    }];
    let matches = audit_lockfile(&lock, &advisories).unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].severity, Severity::Low);
    assert!(matches.iter().all(|m| m.severity != Severity::Critical));
}

#[test]
fn e1217_missing_locked_revision() {
    // D-SUPPLY1 Step 2: a dep declared in the manifest with no lock entry fails
    // the bidirectional completeness check.
    use jet::Lock::{verify_all_manifest_deps_locked, LockFile};

    let raw = manifest_with_deps("app", "0.1.0", "    greeter: ./greeter,");
    let tmp = tmp_dir("e1217");
    write(&tmp, "package.jet", &raw);
    let mf = jet::Manifest::parse(&tmp.join("package.jet"), &raw).unwrap();

    // Empty lock — greeter is declared but not pinned.
    let empty_lock = LockFile {
        version: 1,
        packages: vec![],
        root_dependencies: vec![],
        authority: None,
        workspace_members: vec![],
        workspace_source_digest: None,
        workspace_overlay_policy: Default::default(),
        comptime_inputs: Vec::new(),
        toolchains: Vec::new(),
        browsers: Vec::new(),
        source_channels: Vec::new(),
        build_stamp: None,
        build_contributions: Vec::new(),
    };
    let err = verify_all_manifest_deps_locked(&mf, &empty_lock)
        .expect_err("missing locked revision must fail");
    assert_eq!(err.code, "E1217");
    assert!(err.what.contains("greeter"));

    // A lock that pins greeter passes.
    let good_lock = make_test_lock("greeter", "0.1.0", "sha256-aabb");
    assert!(verify_all_manifest_deps_locked(&mf, &good_lock).is_ok());

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn e1218_breaking_change_under_minor_bump() {
    // D-SUPPLY1 Step 3: a removed public fn under a minor bump is E1218.
    use jet::Publish::SemVer::SemVer;
    use jet::Publish::{classify_bump, diff_public_api, e1218, ApiItem, BumpKind};

    let old = vec![ApiItem {
        kind: "fn".into(),
        name: "parse".into(),
        signature: "fn parse(raw: String) Int".into(),
    }];
    let new: Vec<ApiItem> = vec![]; // parse removed
    let breaking = diff_public_api(&old, &new);
    assert!(!breaking.is_empty());

    let bump = classify_bump(
        &SemVer::parse("1.0.0").unwrap(),
        &SemVer::parse("1.1.0").unwrap(),
    );
    assert_eq!(bump, BumpKind::Minor);

    let d = e1218("1.0.0", "1.1.0", bump, &breaking[0], 2);
    assert_eq!(d.code, "E1218");
    assert!(d.what.contains("1.1.0"));
    assert!(d.fix.contains("2.0.0"), "fix must suggest a major bump");
}

#[test]
fn integrity_e2604_on_tampered_store() {
    use jet::Publish::e2604;

    let diag = e2604("mylib", "1.0.0", "sha256-expected", "sha256-actual");
    assert_eq!(diag.code, "E2604");
    assert!(diag.what.contains("mylib"));
    assert!(diag.what.contains("1.0.0"));
    assert!(diag.why.contains("sha256-expected"));
    assert!(diag.why.contains("sha256-actual"));
}

#[test]
fn private_registry_from_env() {
    use jet::Publish::parse_registries_from_env;
    let mut env = std::collections::HashMap::new();
    env.insert(
        "JET_REGISTRY_INTERNAL_URL".into(),
        "https://registry.acme.corp/jet".into(),
    );
    let regs = parse_registries_from_env(&env);
    assert!(!regs.is_empty());
    assert_eq!(regs[0].name, "internal");
    assert_eq!(regs[0].url, "https://registry.acme.corp/jet");
    assert!(!regs[0].mirror);
}

#[test]
fn pre_publish_gate_blocks_on_build_failure() {
    use jet::Publish::{BumpKind, PrePublishGate};
    let gate = PrePublishGate {
        build_ok: false,
        tests_ok: true,
        breaking: vec![],
        version: "1.1.0".into(),
        bump_kind: BumpKind::Minor,
        next_major: 2,
    };
    assert!(gate.is_blocked(), "failed build must block publish");
}

#[test]
fn pre_publish_gate_passes_with_no_breaks_and_minor_bump() {
    use jet::Publish::{BumpKind, PrePublishGate};
    let gate = PrePublishGate {
        build_ok: true,
        tests_ok: true,
        breaking: vec![],
        version: "1.1.0".into(),
        bump_kind: BumpKind::Minor,
        next_major: 2,
    };
    assert!(!gate.is_blocked());
    assert!(gate.semver_errors().is_empty());
}

// ─────────────────────────────────────────────
// D-PUBLISH1A: dirty-tree refusal (E2605)
// ─────────────────────────────────────────────

#[test]
fn cli_publish_refuses_dirty_git_tree() {
    // D-PUBLISH1A: `jet registry publish` must refuse (E2605) when the git working tree
    // has uncommitted changes. This test initialises a fresh git repo with one
    // commit, adds an uncommitted file, then runs `jet registry publish` and asserts it
    // exits nonzero with E2605 in stderr.
    if !jet_bin().is_file() {
        eprintln!("note: skipping cli_publish_refuses_dirty_git_tree (run `cargo build` first)");
        return;
    }
    if !have_git() {
        eprintln!("note: skipping cli_publish_refuses_dirty_git_tree (git not found)");
        return;
    }

    let tmp = tmp_dir("pub_dirty");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();

    // Create a minimal project.
    write(&tmp, "package.jet", &min_manifest("dirtypkg", "1.0.0"));
    write(&tmp, "run.jet", "fn run() { print(\"hello\"); }\n");

    // Init git, commit everything (clean tree first).
    for cmd_args in &[
        vec!["init", "-b", "main"],
        vec!["config", "user.email", "test@jet.test"],
        vec!["config", "user.name", "Jet Test"],
        vec!["add", "."],
        vec!["commit", "-m", "initial"],
    ] {
        Command::new("git")
            .args(cmd_args)
            .current_dir(&tmp)
            .output()
            .unwrap();
    }

    // Add a new uncommitted file → dirty tree.
    write(&tmp, "untracked.jet", "// dirty\n");

    let out = jet_cmd(&["registry", "publish"], &tmp, &store);
    assert!(
        !out.status.success(),
        "jet registry publish must fail on a dirty tree"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E2605"),
        "jet registry publish must cite E2605 for a dirty tree:\n{stderr}"
    );

    // With --force it must emit a warning and get past the dirty gate. The
    // uncommitted warning is printed before any registry push, so pointing the
    // registry at a bogus local path (push then fails fast, no network) does not
    // affect these assertions.
    let cache = tmp.join("cache");
    let bogus = format!("file://{}", tmp.join("nonexistent.git").to_str().unwrap());
    let out_force = jet_cmd_env(
        &["registry", "publish", "--force"],
        &tmp,
        &[
            ("JET_STORE_DIR", store.to_str().unwrap()),
            ("JET_REGISTRY_URL", bogus.as_str()),
            ("JET_REGISTRY_CACHE_DIR", cache.to_str().unwrap()),
        ],
    );
    let stderr_force = String::from_utf8_lossy(&out_force.stderr);
    assert!(
        stderr_force.contains("warning") && stderr_force.contains("uncommitted"),
        "jet registry publish --force must warn about uncommitted changes:\n{stderr_force}"
    );

    let _ = fs::remove_dir_all(&tmp);
}

// ─────────────────────────────────────────────
// D-VERSION1: yank local marker
// ─────────────────────────────────────────────

#[test]
fn cli_publish_pushes_index_and_enforces_immutability_e1234() {
    // c56 (D-JPK-CACHE1=A / D-VERSION1=A): `jet registry publish` writes a JSONL line to
    // the git registry index and pushes it; republishing the same version is
    // refused with E1234 (version immutability).
    if !jet_bin().is_file() {
        eprintln!("note: skipping cli_publish_pushes_index (run `cargo build` first)");
        return;
    }
    if !have_git() {
        eprintln!("note: skipping cli_publish_pushes_index (git not found)");
        return;
    }

    let tmp = tmp_dir("pub_push");
    let proj = tmp.join("proj");
    fs::create_dir_all(&proj).unwrap();
    let bare = tmp.join("registry.git");
    let url = bare_registry(&bare);
    let cache = tmp.join("cache");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();
    let keys = tmp.join("keys");
    init_clean_project(&proj, "textkit", "1.2.0");
    seed_core_review(&bare, "textkit", "1.2.0");

    let envs = &[
        ("JET_REGISTRY_URL", url.as_str()),
        ("JET_REGISTRY_CACHE_DIR", cache.to_str().unwrap()),
        ("JET_STORE_DIR", store.to_str().unwrap()),
        ("JET_KEYS_DIR", keys.to_str().unwrap()),
    ];

    let out = jet_cmd_env(&["registry", "publish"], &proj, envs);
    assert!(
        out.status.success(),
        "publish should succeed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let index = read_index_file(&bare, "textkit").expect("index file must exist in registry");
    assert!(
        index.contains("\"name\":\"textkit\""),
        "index line:\n{index}"
    );
    assert!(
        index.contains("\"version\":\"1.2.0\""),
        "index line:\n{index}"
    );
    assert!(index.contains("\"yanked\":false"), "index line:\n{index}");
    let entry = index
        .lines()
        .find_map(jet::Publish::IndexEntry::parse_line)
        .expect("published index line must parse");
    let referrer = format!("referrers/{}/index.json", entry.content_hash);
    let referrer_probe = Command::new("git")
        .args(["--git-dir", bare.to_str().unwrap(), "cat-file", "-e"])
        .arg(format!("HEAD:{referrer}"))
        .output()
        .unwrap();
    assert!(
        referrer_probe.status.success(),
        "publish must commit the OCI referrer index at {referrer}"
    );

    // S2/D-MEM1: `jet registry publish` now unconditionally snapshots the public-fn
    // surface to `.jet/cache/api/<pkg>.api` — a committed, durable interface
    // contract (not a build artifact). Commit it so the tree is clean again
    // before the republish attempt below (matching real usage: generate,
    // review, commit).
    for cmd_args in &[vec!["add", "."], vec!["commit", "-m", "snapshot api"]] {
        Command::new("git")
            .args(cmd_args)
            .current_dir(&proj)
            .output()
            .unwrap();
    }

    // Republish the same version → E1234 immutability.
    let out2 = jet_cmd_env(&["registry", "publish"], &proj, envs);
    assert!(
        !out2.status.success(),
        "republishing an existing version must fail"
    );
    let stderr2 = String::from_utf8_lossy(&out2.stderr);
    assert!(
        stderr2.contains("E1234"),
        "republish must cite E1234:\n{stderr2}"
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn cli_concurrent_publish_keeps_one_immutable_version() {
    if !jet_bin().is_file() || !have_git() {
        eprintln!("note: skipping cli_concurrent_publish (need built binary and git)");
        return;
    }

    let tmp = tmp_dir("pub_concurrent");
    let left = tmp.join("left");
    let right = tmp.join("right");
    let bare = tmp.join("registry.git");
    let url = bare_registry(&bare);
    let cache = tmp.join("cache");
    let keys = tmp.join("keys");
    fs::create_dir_all(&left).unwrap();
    fs::create_dir_all(&right).unwrap();
    init_clean_project(&left, "racekit", "1.0.0");
    init_clean_project(&right, "racekit", "1.0.0");
    write(
        &right,
        "run.jet",
        "#Test(\"smoke\") { expect(1 == 1) }\nfn run() { print(\"different bytes\"); }\n",
    );
    for args in &[vec!["add", "."], vec!["commit", "-m", "different source"]] {
        Command::new("git")
            .args(args)
            .current_dir(&right)
            .output()
            .unwrap();
    }
    seed_core_review(&bare, "racekit", "1.0.0");

    let keygen = jet_cmd_env(
        &["registry", "keygen"],
        &left,
        &[("JET_KEYS_DIR", keys.to_str().unwrap())],
    );
    assert!(
        keygen.status.success(),
        "shared publish keygen failed: {}",
        String::from_utf8_lossy(&keygen.stderr)
    );

    let spawn = |project: &Path, store: &Path| {
        Command::new(jet_bin())
            .args(["registry", "publish"])
            .current_dir(project)
            .env("JET_REGISTRY_URL", &url)
            .env("JET_REGISTRY_CACHE_DIR", &cache)
            .env("JET_KEYS_DIR", &keys)
            .env("JET_STORE_DIR", store)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    };
    let left_child = spawn(&left, &tmp.join("left-store"));
    let right_child = spawn(&right, &tmp.join("right-store"));
    let left_output = left_child.wait_with_output().unwrap();
    let right_output = right_child.wait_with_output().unwrap();
    let outputs = [&left_output, &right_output];
    assert_eq!(
        outputs
            .iter()
            .filter(|output| output.status.success())
            .count(),
        1,
        "exactly one concurrent immutable publish may win: left={:?}, right={:?}",
        left_output.status,
        right_output.status
    );
    let loser = outputs
        .iter()
        .find(|output| !output.status.success())
        .expect("one concurrent publisher must lose");
    assert!(
        String::from_utf8_lossy(&loser.stderr).contains("E1234"),
        "duplicate concurrent publish must report immutable-version error: {}",
        String::from_utf8_lossy(&loser.stderr)
    );

    let index = read_index_file(&bare, "racekit").expect("winning index entry must be published");
    assert_eq!(
        index
            .lines()
            .filter(|line| line.contains("\"version\":\"1.0.0\""))
            .count(),
        1,
        "concurrent publication must leave one immutable index line: {index}"
    );
    for path in [
        "artifacts/racekit/1.0.0/package.jet",
        "metadata/racekit.json",
        "transparency/log",
        "transparency/checkpoint",
    ] {
        let output = Command::new("git")
            .args(["--git-dir", bare.to_str().unwrap(), "cat-file", "-e"])
            .arg(format!("HEAD:{path}"))
            .output()
            .unwrap();
        assert!(output.status.success(), "winning publish omitted {path}");
    }

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn registry_fetch_installs_verified_artifact_in_hangar_and_locked_reuses_it() {
    if !jet_bin().is_file() || !have_git() {
        eprintln!(
            "note: skipping registry_fetch_installs_verified_artifact (need built binary and git)"
        );
        return;
    }

    let tmp = tmp_dir("registry_fetch_hangar");
    let publisher = tmp.join("publisher");
    let consumer = tmp.join("consumer");
    fs::create_dir_all(&publisher).unwrap();
    fs::create_dir_all(&consumer).unwrap();
    let bare = tmp.join("registry.git");
    let url = bare_registry(&bare);
    let cache = tmp.join("registry-cache");
    let store = tmp.join("legacy-store");
    let hangar_root = tmp.join("jetpack-root");
    let keys = tmp.join("keys");
    fs::create_dir_all(&store).unwrap();
    init_clean_project(&publisher, "textkit", "1.2.0");
    seed_core_review(&bare, "textkit", "1.2.0");

    let publish = jet_cmd_env(
        &["registry", "publish"],
        &publisher,
        &[
            ("JET_REGISTRY_URL", url.as_str()),
            ("JET_REGISTRY_CACHE_DIR", cache.to_str().unwrap()),
            ("JET_STORE_DIR", store.to_str().unwrap()),
            ("JET_KEYS_DIR", keys.to_str().unwrap()),
        ],
    );
    assert!(
        publish.status.success(),
        "registry publish failed:\n{}",
        String::from_utf8_lossy(&publish.stderr)
    );

    let raw = format!(
        "{}\npolicy: {{ licenses: .Allow([\"MIT\"]), sources: {{ \"textkit\": [\"jet\"] }} }}\n",
        manifest_with_deps("consumer", "0.1.0", "    textkit: textkit#1.2.0,")
    );
    write(&consumer, "package.jet", &raw);
    let manifest = jet::Manifest::parse(&consumer.join("package.jet"), &raw).unwrap();
    let opts = jet::Fetch::FetchOptions {
        locked: false,
        update: false,
        update_dep: None,
        resolution: jet::Publish::ResolveMode::Conservative,
    };

    let denied_raw = format!(
        "{}\npolicy: {{ licenses: .Allow([\"MIT\"]), sources: {{ \"textkit\": [\"other\"] }} }}\n",
        manifest_with_deps("consumer", "0.1.0", "    textkit: textkit#1.2.0,")
    );
    let denied_manifest = jet::Manifest::parse(&consumer.join("package.jet"), &denied_raw)
        .expect("denied policy manifest parses");
    let denied = with_store(&store, || {
        let previous = [
            ("JET_REGISTRY_URL", std::env::var_os("JET_REGISTRY_URL")),
            (
                "JET_REGISTRY_CACHE_DIR",
                std::env::var_os("JET_REGISTRY_CACHE_DIR"),
            ),
            ("JET_KEYS_DIR", std::env::var_os("JET_KEYS_DIR")),
            ("JETPACK_ROOT", std::env::var_os("JETPACK_ROOT")),
        ];
        std::env::set_var("JET_REGISTRY_URL", &url);
        std::env::set_var("JET_REGISTRY_CACHE_DIR", &cache);
        std::env::set_var("JET_KEYS_DIR", &keys);
        std::env::set_var("JETPACK_ROOT", &hangar_root);
        let result = jet::Fetch::fetch(&consumer, &denied_manifest, None, &opts);
        for (key, value) in previous {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
        result
    })
    .expect_err("wrong source mapping must deny before Hangar ingest");
    assert_eq!(denied[0].code, "E1207");
    assert!(denied[0].what.contains("source authority"));
    assert!(denied[0].what.contains("consumer -> textkit#1.2.0"));
    assert!(denied[0].why.contains("package `consumer`"));
    assert!(denied[0].fix.contains("use an allowed source"));
    assert!(
        jetpack::Store::list(&jetpack::Store::Roots::at(hangar_root.clone()))
            .into_iter()
            .all(|entry| !(entry.name == "textkit" && entry.version == "1.2.0"))
    );

    let (lock, dep_dirs) = with_store(&store, || {
        let previous = [
            ("JET_REGISTRY_URL", std::env::var_os("JET_REGISTRY_URL")),
            (
                "JET_REGISTRY_CACHE_DIR",
                std::env::var_os("JET_REGISTRY_CACHE_DIR"),
            ),
            ("JET_KEYS_DIR", std::env::var_os("JET_KEYS_DIR")),
            ("JETPACK_ROOT", std::env::var_os("JETPACK_ROOT")),
        ];
        std::env::set_var("JET_REGISTRY_URL", &url);
        std::env::set_var("JET_REGISTRY_CACHE_DIR", &cache);
        std::env::set_var("JET_KEYS_DIR", &keys);
        std::env::set_var("JETPACK_ROOT", &hangar_root);
        let result = jet::Fetch::fetch(&consumer, &manifest, None, &opts);
        for (key, value) in previous {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
        result.expect("verified registry fetch should succeed")
    });

    let installed = dep_dirs.get("textkit").expect("registry dep directory");
    assert!(
        installed.starts_with(hangar_root.join("hangar").join("objects")),
        "registry dependency must resolve from Hangar, got {}",
        installed.display()
    );
    assert!(installed.join("package.jet").is_file());
    assert!(
        !installed.starts_with(&store),
        "registry dependency must not use the legacy path store"
    );
    let lock_source = lock
        .packages
        .iter()
        .find(|package| package.name == "textkit")
        .expect("registry package must be locked");
    assert!(
        matches!(&lock_source.source, jet::Lock::LockSource::Registry { .. }),
        "registry dependency must retain registry lock identity"
    );
    let hangar_entry = jetpack::Store::list(&jetpack::Store::Roots::at(hangar_root.clone()))
        .into_iter()
        .find(|entry| entry.name == "textkit" && entry.version == "1.2.0")
        .expect("registry fetch must register immutable Hangar metadata");
    assert_eq!(
        hangar_entry.out.as_str(),
        installed.to_string_lossy().as_ref(),
        "Hangar metadata must point at the resolved output"
    );
    assert_eq!(
        hangar_entry.cache_identity.source_fingerprint,
        lock_source
            .content_hash
            .as_deref()
            .expect("registry lock must retain the source hash")
    );
    assert!(
        hangar_entry
            .envelope
            .provenance
            .contains("package=textkit#1.2.0"),
        "Hangar provenance must retain the immutable registry package identity"
    );
    assert!(
        hangar_entry
            .envelope
            .provenance
            .contains("package-policy=package=textkit#1.2.0;license=MIT;source=jet;source-rule=textkit => [jet];fingerprint=sha256-"),
        "Hangar provenance must retain the package policy receipt"
    );
    assert!(
        hangar_entry
            .cache_identity
            .policy_fingerprint
            .starts_with("sha256-"),
        "Hangar cache identity must bind the package policy"
    );
    assert_eq!(
        hangar_entry.cache_identity.policy_fingerprint,
        jet::Publish::policy_fingerprint(&manifest.policy),
        "Hangar cache identity must use the canonical policy fingerprint"
    );
    assert!(
        hangar_entry
            .envelope
            .provenance
            .contains("oci-referrers=subject="),
        "Hangar provenance must retain the verified OCI referrer receipt"
    );

    let update_opts = jet::Fetch::FetchOptions {
        locked: false,
        update: true,
        update_dep: Some("textkit".to_string()),
        resolution: jet::Publish::ResolveMode::Conservative,
    };
    with_store(&store, || {
        let previous = [
            ("JET_REGISTRY_URL", std::env::var_os("JET_REGISTRY_URL")),
            (
                "JET_REGISTRY_CACHE_DIR",
                std::env::var_os("JET_REGISTRY_CACHE_DIR"),
            ),
            ("JET_KEYS_DIR", std::env::var_os("JET_KEYS_DIR")),
            ("JETPACK_ROOT", std::env::var_os("JETPACK_ROOT")),
        ];
        std::env::set_var("JET_REGISTRY_URL", &url);
        std::env::set_var("JET_REGISTRY_CACHE_DIR", &cache);
        std::env::set_var("JET_KEYS_DIR", &keys);
        std::env::set_var("JETPACK_ROOT", &hangar_root);
        let result = jet::Fetch::fetch(&consumer, &manifest, Some(&lock), &update_opts);
        for (key, value) in previous {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
        result.expect("targeted registry update should preserve the verified package")
    });
    let semantic_lock = fs::read_to_string(consumer.join(".jet/lock")).unwrap();
    assert!(semantic_lock.contains("adapter-id = \"registry.pubgrub\""));
    assert!(semantic_lock.contains("update-command = \"jet update textkit\""));
    assert!(semantic_lock.contains("exact = \"textkit#1.2.0\""));
    assert!(
        semantic_lock.contains(
            "package-policy=package=textkit#1.2.0;license=MIT;source=jet;source-rule=textkit => [jet];fingerprint=sha256-"
        ),
        "semantic lock must retain the matched source rule and policy receipt: {semantic_lock}"
    );
    assert!(semantic_lock.contains("pattern = \"textkit\""));
    assert!(semantic_lock.contains("sources = [\"jet\"]"));

    let locked_opts = jet::Fetch::FetchOptions {
        locked: true,
        update: false,
        update_dep: None,
        resolution: jet::Publish::ResolveMode::Conservative,
    };
    let locked_dirs = with_store(&store, || {
        let previous = [
            ("JET_REGISTRY_URL", std::env::var_os("JET_REGISTRY_URL")),
            (
                "JET_REGISTRY_CACHE_DIR",
                std::env::var_os("JET_REGISTRY_CACHE_DIR"),
            ),
            ("JET_KEYS_DIR", std::env::var_os("JET_KEYS_DIR")),
            ("JETPACK_ROOT", std::env::var_os("JETPACK_ROOT")),
        ];
        std::env::set_var("JET_REGISTRY_URL", &url);
        std::env::set_var("JET_REGISTRY_CACHE_DIR", &cache);
        std::env::set_var("JET_KEYS_DIR", &keys);
        std::env::set_var("JETPACK_ROOT", &hangar_root);
        let result = jet::Fetch::fetch(&consumer, &manifest, Some(&lock), &locked_opts);
        for (key, value) in previous {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
        result
            .expect("locked registry fetch should verify and reuse the published artifact")
            .1
    });
    assert_eq!(
        locked_dirs.get("textkit"),
        Some(installed),
        "locked consumption must resolve the same immutable Hangar object"
    );

    let offline_registry_url = format!("file://{}/must-not-be-read.git", tmp.display());
    let offline = jet_cmd_env(
        &["fetch", "--offline"],
        &consumer,
        &[
            ("JET_REGISTRY_URL", offline_registry_url.as_str()),
            ("JET_REGISTRY_CACHE_DIR", cache.to_str().unwrap()),
            ("JET_STORE_DIR", store.to_str().unwrap()),
            ("JET_KEYS_DIR", keys.to_str().unwrap()),
            ("JETPACK_ROOT", hangar_root.to_str().unwrap()),
        ],
    );
    assert!(
        offline.status.success(),
        "offline registry fetch must use the verified local lock path:\n{}",
        String::from_utf8_lossy(&offline.stderr)
    );

    common::make_tree_writable(&tmp);
    fs::remove_dir_all(tmp).unwrap();
}

#[test]
fn registry_fetch_rejects_tampered_referrer_index_before_hangar_ingest() {
    if !jet_bin().is_file() || !have_git() {
        eprintln!(
            "note: skipping registry_fetch_rejects_tampered_referrer_index (need built binary and git)"
        );
        return;
    }

    let tmp = tmp_dir("registry_referrer_tamper");
    let publisher = tmp.join("publisher");
    let consumer = tmp.join("consumer");
    let tamper = tmp.join("tamper");
    let bare = tmp.join("registry.git");
    let url = bare_registry(&bare);
    let cache = tmp.join("registry-cache");
    let store = tmp.join("legacy-store");
    let hangar_root = tmp.join("jetpack-root");
    let keys = tmp.join("keys");
    fs::create_dir_all(&publisher).unwrap();
    fs::create_dir_all(&consumer).unwrap();
    fs::create_dir_all(&store).unwrap();
    init_clean_project(&publisher, "refkit", "1.2.0");
    seed_core_review(&bare, "refkit", "1.2.0");

    let publish = jet_cmd_env(
        &["registry", "publish"],
        &publisher,
        &[
            ("JET_REGISTRY_URL", url.as_str()),
            ("JET_REGISTRY_CACHE_DIR", cache.to_str().unwrap()),
            ("JET_STORE_DIR", store.to_str().unwrap()),
            ("JET_KEYS_DIR", keys.to_str().unwrap()),
        ],
    );
    assert!(
        publish.status.success(),
        "registry publish failed:\n{}",
        String::from_utf8_lossy(&publish.stderr)
    );
    let entry = read_index_file(&bare, "refkit")
        .and_then(|text| text.lines().find_map(jet::Publish::IndexEntry::parse_line))
        .expect("published refkit entry");

    assert!(Command::new("git")
        .args(["clone", url.as_str(), tamper.to_str().unwrap()])
        .status()
        .unwrap()
        .success());
    let referrer_index = tamper
        .join("referrers")
        .join(&entry.content_hash)
        .join("index.json");
    let mut bytes = fs::read(&referrer_index).unwrap();
    bytes.push(b'\n');
    fs::write(&referrer_index, bytes).unwrap();
    for args in [
        vec!["config", "user.email", "test@jet.test"],
        vec!["config", "user.name", "Jet Test"],
        vec!["add", "."],
        vec!["commit", "-m", "tamper referrer index"],
        vec!["push", "origin", "HEAD:main"],
    ] {
        assert!(
            Command::new("git")
                .args(&args)
                .current_dir(&tamper)
                .status()
                .unwrap()
                .success(),
            "tamper registry command failed: {:?}",
            args
        );
    }

    let raw = manifest_with_deps("ref-consumer", "0.1.0", "    refkit: refkit#1.2.0,");
    write(&consumer, "package.jet", &raw);
    let manifest = jet::Manifest::parse(&consumer.join("package.jet"), &raw).unwrap();
    let opts = jet::Fetch::FetchOptions {
        locked: false,
        update: false,
        update_dep: None,
        resolution: jet::Publish::ResolveMode::Conservative,
    };
    let denied = with_store(&store, || {
        let previous = [
            ("JET_REGISTRY_URL", std::env::var_os("JET_REGISTRY_URL")),
            (
                "JET_REGISTRY_CACHE_DIR",
                std::env::var_os("JET_REGISTRY_CACHE_DIR"),
            ),
            ("JET_KEYS_DIR", std::env::var_os("JET_KEYS_DIR")),
            ("JETPACK_ROOT", std::env::var_os("JETPACK_ROOT")),
        ];
        std::env::set_var("JET_REGISTRY_URL", &url);
        std::env::set_var("JET_REGISTRY_CACHE_DIR", &cache);
        std::env::set_var("JET_KEYS_DIR", &keys);
        std::env::set_var("JETPACK_ROOT", &hangar_root);
        let result = jet::Fetch::fetch(&consumer, &manifest, None, &opts);
        for (key, value) in previous {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
        result
    })
    .expect_err("a referrer index changed outside signed metadata must fail closed");
    assert_eq!(denied[0].code, "E1207");
    assert!(
        denied[0]
            .fix
            .contains("restore the immutable OCI referrer set")
            || denied[0].what.contains("OCI referrer index digest"),
        "tampered referrer recovery must be explicit: {:?}",
        denied[0]
    );
    assert!(
        jetpack::Store::list(&jetpack::Store::Roots::at(hangar_root))
            .into_iter()
            .all(|entry| !(entry.name == "refkit" && entry.version == "1.2.0")),
        "tampered registry evidence must not reach Hangar"
    );

    common::make_tree_writable(&tmp);
    fs::remove_dir_all(tmp).unwrap();
}

#[test]
fn registry_fetch_applies_artifact_dependency_roles_features_and_constraints() {
    if !jet_bin().is_file() || !have_git() {
        eprintln!(
            "note: skipping registry dependency metadata delivery (need built binary and git)"
        );
        return;
    }

    let tmp = tmp_dir("registry_dependency_metadata");
    let bare = tmp.join("registry.git");
    let url = bare_registry(&bare);
    let cache = tmp.join("registry-cache");
    let keys = tmp.join("keys");
    let store = tmp.join("store");
    let hangar_root = tmp.join("jetpack-root");
    let consumer = tmp.join("consumer");
    fs::create_dir_all(&store).unwrap();
    fs::create_dir_all(&consumer).unwrap();

    let publish = |name: &str, version: &str, metadata: Option<&str>| {
        let project = tmp.join(format!("publish-{name}-{version}"));
        fs::create_dir_all(&project).unwrap();
        init_clean_project(&project, name, version);
        if let Some(metadata) = metadata {
            write(&project, "registry.json", metadata);
            for args in &[vec!["add", "."], vec!["commit", "-m", "registry metadata"]] {
                Command::new("git")
                    .args(args)
                    .current_dir(&project)
                    .output()
                    .unwrap();
            }
        }
        seed_core_review(&bare, name, version);
        let output = jet_cmd_env(
            &["registry", "publish"],
            &project,
            &[
                ("JET_REGISTRY_URL", url.as_str()),
                ("JET_REGISTRY_CACHE_DIR", cache.to_str().unwrap()),
                ("JET_STORE_DIR", store.to_str().unwrap()),
                ("JET_KEYS_DIR", keys.to_str().unwrap()),
            ],
        );
        assert!(
            output.status.success(),
            "publishing {name} {version} failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };

    publish("runtime", "1.0.0", None);
    publish("runtime", "1.0.1", None);
    publish("buildtool", "1.0.0", None);
    publish("trace", "1.0.0", None);
    publish(
        "rolekit",
        "1.0.0",
        Some(
            r#"{"name":"rolekit","version":"1.0.0","dependencies":{"runtime":"^1.0"},"build_dependencies":{"buildtool":"1.0.0"},"dev_dependencies":{"devkit":"1.0.0"},"optional_dependencies":{"trace":"1.0.0"},"features":{"default":["trace"]},"constraints":{"runtime":{"require":"^1.0","prefer":"1.0.1","reject":["1.0.1"],"strict":true}}}"#,
        ),
    );

    let raw = manifest_with_deps("consumer", "0.1.0", "    rolekit: rolekit#1.0.0,");
    write(&consumer, "package.jet", &raw);
    let manifest = jet::Manifest::parse(&consumer.join("package.jet"), &raw).unwrap();
    let opts = jet::Fetch::FetchOptions {
        locked: false,
        update: false,
        update_dep: None,
        resolution: jet::Publish::ResolveMode::Latest,
    };
    let (lock, dep_dirs) = with_store(&store, || {
        let previous = [
            ("JET_REGISTRY_URL", std::env::var_os("JET_REGISTRY_URL")),
            (
                "JET_REGISTRY_CACHE_DIR",
                std::env::var_os("JET_REGISTRY_CACHE_DIR"),
            ),
            ("JET_KEYS_DIR", std::env::var_os("JET_KEYS_DIR")),
            ("JETPACK_ROOT", std::env::var_os("JETPACK_ROOT")),
        ];
        std::env::set_var("JET_REGISTRY_URL", &url);
        std::env::set_var("JET_REGISTRY_CACHE_DIR", &cache);
        std::env::set_var("JET_KEYS_DIR", &keys);
        std::env::set_var("JETPACK_ROOT", &hangar_root);
        let result = jet::Fetch::fetch(&consumer, &manifest, None, &opts);
        for (key, value) in previous {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
        result.expect("artifact dependency metadata should resolve through Hangar")
    });

    let rolekit = lock
        .packages
        .iter()
        .find(|package| package.name == "rolekit")
        .expect("rolekit must be locked");
    assert_eq!(rolekit.version, "1.0.0");
    assert!(
        rolekit.dependencies.contains(&"runtime".to_string())
            && rolekit.dependencies.contains(&"buildtool".to_string())
            && rolekit.dependencies.contains(&"trace".to_string()),
        "active normal/build/default-feature edges must enter the lock: {:?}",
        rolekit.dependencies
    );
    assert!(
        !rolekit.dependencies.contains(&"devkit".to_string()),
        "dev dependency must stay outside the production closure"
    );
    assert_eq!(
        lock.packages
            .iter()
            .find(|package| package.name == "runtime")
            .map(|package| package.version.as_str()),
        Some("1.0.0"),
        "reject must exclude 1.0.1 even though prefer requests it"
    );
    assert!(dep_dirs.contains_key("buildtool") && dep_dirs.contains_key("trace"));
    let lock_text = fs::read_to_string(consumer.join(".jet/lock")).unwrap();
    assert!(
        lock_text.contains("dependency-metadata = "),
        "semantic lock must carry the exact artifact-bound dependency meaning"
    );

    let locked_opts = jet::Fetch::FetchOptions {
        locked: true,
        update: false,
        update_dep: None,
        resolution: jet::Publish::ResolveMode::Latest,
    };
    let (locked, locked_dirs) = with_store(&store, || {
        let previous = [
            ("JET_REGISTRY_URL", std::env::var_os("JET_REGISTRY_URL")),
            (
                "JET_REGISTRY_CACHE_DIR",
                std::env::var_os("JET_REGISTRY_CACHE_DIR"),
            ),
            ("JET_KEYS_DIR", std::env::var_os("JET_KEYS_DIR")),
            ("JETPACK_ROOT", std::env::var_os("JETPACK_ROOT")),
        ];
        std::env::set_var("JET_REGISTRY_URL", &url);
        std::env::set_var("JET_REGISTRY_CACHE_DIR", &cache);
        std::env::set_var("JET_KEYS_DIR", &keys);
        std::env::set_var("JETPACK_ROOT", &hangar_root);
        let result = jet::Fetch::fetch(&consumer, &manifest, Some(&lock), &locked_opts);
        for (key, value) in previous {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
        result.expect("locked registry metadata must agree with the dependency lock")
    });
    assert!(locked.packages.iter().any(|package| {
        package.name == "rolekit"
            && package.dependencies.contains(&"runtime".to_string())
            && package.dependencies.contains(&"buildtool".to_string())
            && package.dependencies.contains(&"trace".to_string())
    }));
    assert!(locked_dirs.contains_key("rolekit"));

    // The dependency meaning is part of the immutable artifact identity. A
    // valid metadata edit must fail before a fresh Hangar can ingest it even
    // when the package source and signed referrer evidence are unchanged.
    let tamper_work = tmp.join("tamper-registry");
    assert!(Command::new("git")
        .args(["clone", url.as_str(), tamper_work.to_str().unwrap()])
        .status()
        .unwrap()
        .success());
    let metadata_path = tamper_work
        .join("artifacts")
        .join("rolekit")
        .join("1.0.0")
        .join("registry.json");
    let metadata = fs::read_to_string(&metadata_path).unwrap();
    let tampered_metadata = metadata.replace(
        "\"build_dependencies\":{\"buildtool\":\"1.0.0\"}",
        "\"build_dependencies\":{\"runtime\":\"1.0.0\"}",
    );
    assert_ne!(
        metadata, tampered_metadata,
        "tamper fixture must change meaning"
    );
    fs::write(&metadata_path, tampered_metadata).unwrap();
    for args in [
        vec!["config", "user.email", "test@jet.test"],
        vec!["config", "user.name", "Jet Test"],
        vec!["add", "artifacts/rolekit/1.0.0/registry.json"],
        vec!["commit", "-m", "tamper registry metadata"],
        vec!["push", "origin", "HEAD:main"],
    ] {
        assert!(
            Command::new("git")
                .args(&args)
                .current_dir(&tamper_work)
                .status()
                .unwrap()
                .success(),
            "git command failed: {args:?}"
        );
    }

    let tampered_consumer = tmp.join("consumer-tampered");
    let tampered_cache = tmp.join("registry-cache-tampered");
    let tampered_hangar = tmp.join("jetpack-root-tampered");
    fs::create_dir_all(&tampered_consumer).unwrap();
    let tampered_raw =
        manifest_with_deps("tampered-consumer", "0.1.0", "    rolekit: rolekit#1.0.0,");
    write(&tampered_consumer, "package.jet", &tampered_raw);
    let tampered_manifest =
        jet::Manifest::parse(&tampered_consumer.join("package.jet"), &tampered_raw).unwrap();
    let tampered_result = with_store(&store, || {
        let previous = [
            ("JET_REGISTRY_URL", std::env::var_os("JET_REGISTRY_URL")),
            (
                "JET_REGISTRY_CACHE_DIR",
                std::env::var_os("JET_REGISTRY_CACHE_DIR"),
            ),
            ("JET_KEYS_DIR", std::env::var_os("JET_KEYS_DIR")),
            ("JETPACK_ROOT", std::env::var_os("JETPACK_ROOT")),
        ];
        std::env::set_var("JET_REGISTRY_URL", &url);
        std::env::set_var("JET_REGISTRY_CACHE_DIR", &tampered_cache);
        std::env::set_var("JET_KEYS_DIR", &keys);
        std::env::set_var("JETPACK_ROOT", &tampered_hangar);
        let result = jet::Fetch::fetch(&tampered_consumer, &tampered_manifest, None, &opts);
        for (key, value) in previous {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
        result
    });
    let tampered_error = tampered_result.expect_err("metadata tampering must fail closed");
    assert_eq!(first_diag_code(&tampered_error), "E1207");
    assert!(
        tampered_error
            .iter()
            .any(|diagnostic| diagnostic.what.contains("content hash")),
        "tampered metadata must be rejected as an identity failure: {tampered_error:?}"
    );
    assert!(
        jetpack::Store::list(&jetpack::Store::Roots::at(tampered_hangar.clone()))
            .into_iter()
            .all(|entry| !(entry.name == "rolekit" && entry.version == "1.0.0")),
        "tampered registry metadata must not reach Hangar"
    );

    common::make_tree_writable(&tmp);
    fs::remove_dir_all(tmp).unwrap();
}

#[test]
fn registry_fetch_applies_verified_advisory_freshness_before_hangar_ingest() {
    if !jet_bin().is_file() || !have_git() {
        eprintln!(
            "note: skipping registry_fetch_applies_advisory_freshness (need built binary and git)"
        );
        return;
    }

    let tmp = tmp_dir("registry_fetch_advisory_freshness");
    let publisher = tmp.join("publisher");
    let consumer = tmp.join("consumer");
    let bare = tmp.join("registry.git");
    let url = bare_registry(&bare);
    let cache = tmp.join("registry-cache");
    let store = tmp.join("legacy-store");
    let hangar_root = tmp.join("jetpack-root");
    let keys = tmp.join("keys");
    fs::create_dir_all(&publisher).unwrap();
    fs::create_dir_all(&consumer).unwrap();
    fs::create_dir_all(&store).unwrap();
    init_clean_project(&publisher, "freshlib", "1.2.0");
    seed_core_review(&bare, "freshlib", "1.2.0");

    let publish = jet_cmd_env(
        &["registry", "publish"],
        &publisher,
        &[
            ("JET_REGISTRY_URL", url.as_str()),
            ("JET_REGISTRY_CACHE_DIR", cache.to_str().unwrap()),
            ("JET_STORE_DIR", store.to_str().unwrap()),
            ("JET_KEYS_DIR", keys.to_str().unwrap()),
        ],
    );
    assert!(
        publish.status.success(),
        "registry publish failed:\n{}",
        String::from_utf8_lossy(&publish.stderr)
    );

    let raw = manifest_with_deps("fresh-consumer", "0.1.0", "    freshlib: freshlib#1.2.0,");
    write(&consumer, "package.jet", &raw);
    let manifest = jet::Manifest::parse(&consumer.join("package.jet"), &raw).unwrap();
    let opts = jet::Fetch::FetchOptions {
        locked: false,
        update: false,
        update_dep: None,
        resolution: jet::Publish::ResolveMode::Conservative,
    };

    with_keys(&keys, || {
        let (seed, _public_path, public_key) =
            jet::Publish::Sign::keygen("advisory", false).expect("advisory keygen should succeed");
        let mut feed = jet::Publish::AdvisoryFeed {
            sequence: 1,
            issued_at: 100,
            expires_at: 200_000,
            maturity_seconds: jet::Publish::DEFAULT_MATURITY_SECONDS,
            key_id: jet::Publish::advisory_key_id(&public_key).unwrap(),
            public_key: public_key.clone(),
            signature: String::new(),
            releases: vec![jet::Publish::AdvisoryRelease {
                package: "freshlib".into(),
                version: jet::Publish::SemVer::SemVer::parse("1.2.0").unwrap(),
                first_seen: 100,
                source_class: jet::Publish::SourceClass::ThirdParty,
            }],
            advisories: Vec::new(),
            exceptions: Vec::new(),
        };
        feed.signature = jet::Publish::sign_advisory_feed(&feed, &seed).unwrap();
        fs::create_dir_all(consumer.join(".jet")).unwrap();
        fs::write(
            consumer.join(".jet/advisories.db"),
            jet::Publish::advisory_feed_text(&feed),
        )
        .unwrap();
        fs::write(
            consumer.join(".jet/advisory-trust"),
            format!("public_key={public_key}\nmin_sequence=1\n"),
        )
        .unwrap();

        let env_keys = [
            "JET_REGISTRY_URL",
            "JET_REGISTRY_CACHE_DIR",
            "JET_STORE_DIR",
            "JETPACK_ROOT",
            "JET_ADVISORY_NOW",
            "JET_ADVISORY_DB",
            "JET_ADVISORY_TRUST",
            "JET_ADVISORY_PUBLIC_KEY",
        ];
        let previous = env_keys
            .iter()
            .map(|key| (*key, std::env::var_os(key)))
            .collect::<Vec<_>>();
        std::env::set_var("JET_REGISTRY_URL", &url);
        std::env::set_var("JET_REGISTRY_CACHE_DIR", &cache);
        std::env::set_var("JET_STORE_DIR", &store);
        std::env::set_var("JETPACK_ROOT", &hangar_root);
        std::env::set_var("JET_ADVISORY_NOW", "200");
        std::env::remove_var("JET_ADVISORY_DB");
        std::env::remove_var("JET_ADVISORY_TRUST");
        std::env::remove_var("JET_ADVISORY_PUBLIC_KEY");

        let immature = jet::Fetch::fetch(&consumer, &manifest, None, &opts)
            .expect_err("an immature registry candidate must be rejected");
        assert_eq!(
            first_diag_code(&immature),
            "E2609",
            "diagnostics: {immature:?}"
        );
        assert!(immature[0].what.contains("freshlib#1.2.0"));
        assert!(
            !consumer.join(".jet-build/deps/freshlib").exists(),
            "freshness must fail before a registry artifact is usable"
        );

        let exception_raw = format!(
            "{raw}\npolicy: {{ exceptions: [PolicyException.{{ id: \"JSA-2026-0001\", scope: \"freshlib#1.2.0\", reason: \"urgent security fix\", expires: 9999999999 }}] }}\n"
        );
        let exception_manifest =
            jet::Manifest::parse(&consumer.join("package.jet"), &exception_raw)
                .expect("source exception manifest should parse");
        let (_exception_lock, exception_dirs) =
            jet::Fetch::fetch(&consumer, &exception_manifest, None, &opts)
                .expect("an active exact source exception should admit the fresh release");
        assert!(exception_dirs.contains_key("freshlib"));
        let excepted_entry = jetpack::Store::list(&jetpack::Store::Roots::at(hangar_root.clone()))
            .into_iter()
            .find(|entry| entry.name == "freshlib" && entry.version == "1.2.0")
            .expect("source exception must still use the production Hangar path");
        assert!(
            excepted_entry
                .envelope
                .provenance
                .contains("source-policy-exception=id=JSA-2026-0001;scope=freshlib#1.2.0"),
            "source exception evidence must be visible in provenance: {}",
            excepted_entry.envelope.provenance
        );

        std::env::set_var("JET_ADVISORY_NOW", "100000");
        let (lock, dep_dirs) = jet::Fetch::fetch(&consumer, &manifest, None, &opts)
            .expect("a mature, verified candidate should resolve and ingest");
        assert!(dep_dirs.contains_key("freshlib"));
        assert!(lock
            .packages
            .iter()
            .any(|package| package.name == "freshlib"));
        let entry = jetpack::Store::list(&jetpack::Store::Roots::at(hangar_root.clone()))
            .into_iter()
            .find(|entry| entry.name == "freshlib" && entry.version == "1.2.0")
            .expect("mature candidate must produce a Hangar entry");
        assert!(
            entry
                .envelope
                .provenance
                .contains("advisory-feed=sequence=1"),
            "verified advisory receipt must be visible in Hangar provenance: {}",
            entry.envelope.provenance
        );
        assert!(entry.envelope.provenance.contains("maturity=86400s"));

        let mut stale = feed.clone();
        stale.expires_at = 150;
        stale.signature =
            jet::Publish::sign_advisory_feed(&stale, &seed).expect("stale feed should sign");
        fs::write(
            &consumer.join(".jet/advisories.db"),
            jet::Publish::advisory_feed_text(&stale),
        )
        .unwrap();
        std::env::set_var("JET_ADVISORY_NOW", "200");
        let (_same_lock, locked_dirs) = jet::Fetch::fetch(&consumer, &manifest, Some(&lock), &opts)
            .expect("an exact lock must remain usable when advisory metadata expires");
        assert_eq!(
            locked_dirs.get("freshlib"),
            dep_dirs.get("freshlib"),
            "exact-lock reuse must keep the same Hangar object"
        );
        let reused_entry = jetpack::Store::list(&jetpack::Store::Roots::at(hangar_root.clone()))
            .into_iter()
            .find(|entry| entry.name == "freshlib" && entry.version == "1.2.0")
            .expect("exact-lock reuse must keep the Hangar entry");
        assert!(
            reused_entry
                .envelope
                .provenance
                .contains("advisory-feed=sequence=1"),
            "exact-lock reuse must not erase the verified advisory receipt"
        );

        for (key, value) in previous {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    });

    common::make_tree_writable(&tmp);
    fs::remove_dir_all(tmp).unwrap();
}

#[test]
fn cli_yank_flips_index_entry() {
    // c56 (D-VERSION1=A): `jet registry yank <version>` flips the `yanked` flag on the
    // version's index line in place — it never deletes the line.
    if !jet_bin().is_file() {
        eprintln!("note: skipping cli_yank_flips_index_entry (run `cargo build` first)");
        return;
    }
    if !have_git() {
        eprintln!("note: skipping cli_yank_flips_index_entry (git not found)");
        return;
    }

    let tmp = tmp_dir("yank_flip");
    let proj = tmp.join("proj");
    fs::create_dir_all(&proj).unwrap();
    let bare = tmp.join("registry.git");
    let url = bare_registry(&bare);
    let cache = tmp.join("cache");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();
    let keys = tmp.join("keys");
    init_clean_project(&proj, "mypkg", "2.0.0");
    seed_core_review(&bare, "mypkg", "2.0.0");

    let envs = &[
        ("JET_REGISTRY_URL", url.as_str()),
        ("JET_REGISTRY_CACHE_DIR", cache.to_str().unwrap()),
        ("JET_STORE_DIR", store.to_str().unwrap()),
        ("JET_KEYS_DIR", keys.to_str().unwrap()),
    ];

    let pubd = jet_cmd_env(&["registry", "publish"], &proj, envs);
    assert!(
        pubd.status.success(),
        "publish should succeed:\n{}",
        String::from_utf8_lossy(&pubd.stderr)
    );

    // Publishing records the API snapshot as a durable project contract. The
    // generated test harness is also local build output, so commit the
    // release boundary before exercising yank and immutable-version reuse.
    for cmd_args in &[vec!["add", "."], vec!["commit", "-m", "snapshot api"]] {
        Command::new("git")
            .args(cmd_args)
            .current_dir(&proj)
            .output()
            .unwrap();
    }

    let yanked = jet_cmd_env(
        &["registry", "yank", "2.0.0", "--message", "regression"],
        &proj,
        envs,
    );
    assert!(
        yanked.status.success(),
        "yank should succeed:\n{}",
        String::from_utf8_lossy(&yanked.stderr)
    );

    let index = read_index_file(&bare, "mypkg").expect("index file must exist");
    // The line is flipped, not removed — still exactly one line for 2.0.0.
    assert_eq!(
        index
            .lines()
            .filter(|l| l.contains("\"version\":\"2.0.0\""))
            .count(),
        1,
        "yank must not delete the line:\n{index}"
    );
    assert!(
        index.contains("\"yanked\":true"),
        "version must be flipped to yanked:\n{index}"
    );

    // A yank hides the version from fresh selection but never releases its
    // immutable identity. The real publish path must reject reuse as E1234.
    let republish = jet_cmd_env(&["registry", "publish"], &proj, envs);
    assert!(
        !republish.status.success(),
        "a yanked version must remain reserved"
    );
    let republish_stderr = String::from_utf8_lossy(&republish.stderr);
    assert!(
        republish_stderr.contains("E1234"),
        "republishing a yanked version must cite E1234:\n{republish_stderr}"
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn registry_transport_rejects_and_redacts_embedded_credentials() {
    let registry = jet::Publish::RegistryConfig::private(
        "scratch",
        "https://user:super-secret@example.invalid/registry.git",
        false,
    );
    assert_eq!(
        jet::Publish::redact_registry_url(&registry.url),
        "https://example.invalid/registry.git"
    );
    assert_eq!(
        jet::Publish::redact_registry_url(
            "https://example.invalid/registry.git?access_token=super-secret#fragment"
        ),
        "https://example.invalid/registry.git"
    );
    let diagnostic = jet::Publish::ensure_index_clone(&registry)
        .expect_err("registry transport must reject embedded credentials");
    let rendered = jet::Diagnostics::render_all("package.jet", "", &[diagnostic]);
    assert!(
        rendered.contains("E1235"),
        "unexpected diagnostic:\n{rendered}"
    );
    assert!(
        !rendered.contains("super-secret"),
        "registry credential leaked in diagnostic:\n{rendered}"
    );
    let query_registry = jet::Publish::RegistryConfig::private(
        "scratch",
        "https://example.invalid/registry.git?access_token=super-secret",
        false,
    );
    let query_diagnostic = jet::Publish::ensure_index_clone(&query_registry)
        .expect_err("registry transport must reject credential-bearing URL parameters");
    let query_rendered = jet::Diagnostics::render_all("package.jet", "", &[query_diagnostic]);
    assert!(
        query_rendered.contains("E1235"),
        "unexpected diagnostic:\n{query_rendered}"
    );
    assert!(
        !query_rendered.contains("super-secret"),
        "registry credential leaked in query diagnostic:\n{query_rendered}"
    );

    let file_registry = jet::Publish::RegistryConfig::private(
        "scratch",
        "file:///tmp/registry.git?access_token=super-secret",
        false,
    );
    let file_diagnostic = jet::Publish::ensure_index_clone(&file_registry)
        .expect_err("credential-bearing file URLs must fail before transport");
    let file_rendered = jet::Diagnostics::render_all("package.jet", "", &[file_diagnostic]);
    assert!(
        !file_rendered.contains("super-secret"),
        "credential leaked: {file_rendered}"
    );

    let detail = jet::Publish::e1235(
        "https://example.invalid/registry.git",
        "git helper failed after receiving password super-secret",
    );
    let detail_rendered = jet::Diagnostics::render_all("package.jet", "", &[detail]);
    assert!(
        !detail_rendered.contains("super-secret"),
        "raw credential-bearing transport detail leaked:\n{detail_rendered}"
    );
}

#[test]
fn registry_refresh_failure_preserves_previous_cache() {
    if !have_git() {
        eprintln!(
            "note: skipping registry_refresh_failure_preserves_previous_cache (git not found)"
        );
        return;
    }

    let tmp = tmp_dir("registry_stale_cache");
    let bare = tmp.join("registry.git");
    let url = bare_registry(&bare);
    seed_core_review(&bare, "stale-kit", "1.0.0");
    let cache = tmp.join("cache");
    let registry = jet::Publish::RegistryConfig::private("scratch", &url, false);

    let _guard = STORE_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let previous_cache = std::env::var_os("JET_REGISTRY_CACHE_DIR");
    std::env::set_var("JET_REGISTRY_CACHE_DIR", &cache);
    let repo = jet::Publish::ensure_index_clone(&registry).expect("initial registry clone");
    let before = fs::read(repo.join("reviews/stale-kit/1.0.0.review")).unwrap();

    fs::remove_dir_all(&bare).unwrap();
    let error = jet::Publish::ensure_index_clone(&registry)
        .expect_err("a failed refresh must not replace the existing cache");
    assert_eq!(error.code, "E1235");
    assert_eq!(
        fs::read(repo.join("reviews/stale-kit/1.0.0.review")).unwrap(),
        before
    );
    let parent = repo.parent().unwrap();
    let leftovers = fs::read_dir(parent)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".partial-") || name.contains(".backup-"))
        .collect::<Vec<_>>();
    assert!(
        leftovers.is_empty(),
        "failed refresh left staging paths: {leftovers:?}"
    );

    match previous_cache {
        Some(value) => std::env::set_var("JET_REGISTRY_CACHE_DIR", value),
        None => std::env::remove_var("JET_REGISTRY_CACHE_DIR"),
    }
    drop(_guard);
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn registry_corrupt_artifact_is_rejected_before_install() {
    let tmp = tmp_dir("registry_corrupt_artifact");
    let repo = tmp.join("registry");
    let artifact = repo.join("artifacts").join("corruptkit").join("1.0.0");
    fs::create_dir_all(&artifact).unwrap();
    fs::write(artifact.join("package.jet"), "not-the-published-bytes").unwrap();

    let entry = signed_entry("corruptkit", "1.0.0", "sha256-not-the-tree", "", "");
    let error = jet::Publish::verify_artifact(&repo, &entry)
        .expect_err("a content-addressed registry artifact must fail closed when corrupted");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("failed its content hash"));
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn registry_artifact_hash_covers_auxiliary_files() {
    let tmp = tmp_dir("registry_artifact_auxiliary_hash");
    let repo = tmp.join("registry");
    let source = tmp.join("source");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("package.jet"), "package bytes").unwrap();
    fs::write(
        source.join("registry.json"),
        r#"{"name":"asset-kit","version":"1.0.0"}"#,
    )
    .unwrap();
    fs::write(source.join("embedded.bin"), b"published asset").unwrap();

    let expected_hash = jet::Publish::registry_artifact_hash(&source).unwrap();
    fs::write(source.join("embedded.bin"), b"tampered asset").unwrap();
    let error = jet::Publish::publish_artifact(
        &repo,
        &source,
        "asset-kit",
        "1.0.0",
        &expected_hash,
    )
    .expect_err("mutating a non-Jet artifact file must fail content verification");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("source hash changed"));
    let destination = jet::Publish::artifact_path(&repo, "asset-kit", "1.0.0").unwrap();
    assert!(!destination.exists());
    let _ = fs::remove_dir_all(&tmp);
}

#[cfg(unix)]
#[test]
fn registry_artifact_parent_symlink_is_rejected_before_hashing() {
    use std::os::unix::fs::symlink;

    let tmp = tmp_dir("registry_artifact_parent_symlink");
    let repo = tmp.join("registry");
    let outside = tmp.join("outside");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(outside.join("escape-kit").join("1.0.0")).unwrap();
    fs::write(
        outside.join("escape-kit").join("1.0.0").join("package.jet"),
        "must not be followed",
    )
    .unwrap();
    symlink(&outside, repo.join("artifacts")).unwrap();

    let entry = signed_entry("escape-kit", "1.0.0", "sha256-unchecked", "", "");
    let error = jet::Publish::verify_artifact(&repo, &entry)
        .expect_err("registry verification must not follow an artifact-root symlink");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("not a real directory"));
    assert!(outside.join("escape-kit/1.0.0/package.jet").is_file());
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn registry_feature_cycle_fails_before_staging() {
    let tmp = tmp_dir("registry_feature_cycle");
    let repo = tmp.join("registry");
    let source = tmp.join("source");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("package.jet"), "package bytes").unwrap();
    fs::write(
        source.join("registry.json"),
        r#"{"name":"cycle-kit","version":"1.0.0","features":{"default":["loop"],"loop":["default"]}}"#,
    )
    .unwrap();

    let error = jet::Publish::publish_artifact(&repo, &source, "cycle-kit", "1.0.0", "")
        .expect_err("cyclic default feature closure must fail before publication");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(error
        .to_string()
        .contains("feature closure contains a cycle"));
    let destination = jet::Publish::artifact_path(&repo, "cycle-kit", "1.0.0").unwrap();
    assert!(
        !destination.exists(),
        "invalid metadata installed an artifact"
    );
    let _ = fs::remove_dir_all(&tmp);
}

#[cfg(unix)]
#[test]
fn registry_interrupted_artifact_stage_leaves_no_partial() {
    use std::os::unix::fs::symlink;

    let tmp = tmp_dir("registry_interrupted_artifact");
    let repo = tmp.join("registry");
    let source = tmp.join("source");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("package.jet"), "package bytes").unwrap();
    symlink("package.jet", source.join("linked.jet")).unwrap();
    let expected_hash = jet::SHA256::tree_hash(&source);

    let error =
        jet::Publish::publish_artifact(&repo, &source, "atomic-kit", "1.0.0", &expected_hash)
            .expect_err("unsupported source nodes must abort the staged publication");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    let destination = jet::Publish::artifact_path(&repo, "atomic-kit", "1.0.0").unwrap();
    assert!(
        !destination.exists(),
        "failed publication installed an artifact"
    );
    let package_dir = repo.join("artifacts").join("atomic-kit");
    if package_dir.is_dir() {
        assert!(
            fs::read_dir(package_dir).unwrap().next().is_none(),
            "failed publication left a staging directory"
        );
    }
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn cli_publish_unreachable_registry_e1235() {
    // c56: an unreachable registry index makes `jet registry publish` fail with E1235 —
    // a clean diagnostic, never a raw git stack trace.
    if !jet_bin().is_file() {
        eprintln!("note: skipping cli_publish_unreachable_registry (run `cargo build` first)");
        return;
    }
    if !have_git() {
        eprintln!("note: skipping cli_publish_unreachable_registry (git not found)");
        return;
    }

    let tmp = tmp_dir("pub_unreach");
    let proj = tmp.join("proj");
    fs::create_dir_all(&proj).unwrap();
    let cache = tmp.join("cache");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();
    init_clean_project(&proj, "lonely", "0.1.0");

    // A local path with no repo → git clone fails immediately, no network.
    let bogus = format!("file://{}", tmp.join("nonexistent.git").to_str().unwrap());
    let envs = &[
        ("JET_REGISTRY_URL", bogus.as_str()),
        ("JET_REGISTRY_CACHE_DIR", cache.to_str().unwrap()),
        ("JET_STORE_DIR", store.to_str().unwrap()),
    ];

    let out = jet_cmd_env(&["registry", "publish"], &proj, envs);
    assert!(
        !out.status.success(),
        "publish to an unreachable registry must fail"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E1235"), "must cite E1235:\n{stderr}");

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn cli_yank_requires_version_arg() {
    // E2606: `jet registry yank` with no version must exit nonzero with E2606.
    if !jet_bin().is_file() {
        eprintln!("note: skipping cli_yank_requires_version_arg (run `cargo build` first)");
        return;
    }

    let tmp = tmp_dir("yank_no_ver");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();
    write(&tmp, "package.jet", &min_manifest("mypkg", "1.0.0"));

    let out = jet_cmd(&["registry", "yank"], &tmp, &store);
    assert!(
        !out.status.success(),
        "jet registry yank with no version must fail"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E2606"), "must cite E2606:\n{stderr}");

    let _ = fs::remove_dir_all(&tmp);
}

// ─────────────────────────────────────────────
// D-RESOLVE1: highest-compatible resolver
// ─────────────────────────────────────────────

#[test]
fn resolver_selects_highest_compatible() {
    // D-RESOLVE1=A: given a list of candidates and constraints, the resolver
    // picks the highest version satisfying all constraints.
    use jet::Publish::SemVer::SemVer;
    use jet::Publish::{select_highest_compatible, VersionConstraint, VersionReq};

    fn sv(s: &str) -> SemVer {
        SemVer::parse(s).unwrap()
    }

    let candidates = vec![sv("1.0.0"), sv("1.2.0"), sv("1.5.0"), sv("2.0.0")];

    // ^1.0 && ^1.2 → highest compatible is 1.5.0 (both within major 1).
    let c1 = VersionConstraint {
        package: "log".into(),
        req: VersionReq::parse("^1.0").unwrap(),
        from: "app".into(),
    };
    let c2 = VersionConstraint {
        package: "log".into(),
        req: VersionReq::parse("^1.2").unwrap(),
        from: "lib".into(),
    };
    let winner = select_highest_compatible("log", &[&c1, &c2], &candidates)
        .expect("compatible constraints with candidates should resolve");
    assert_eq!(
        winner.to_string(),
        "1.5.0",
        "must pick the highest satisfying candidate"
    );

    // ^2.0 → 2.0.0 only.
    let c3 = VersionConstraint {
        package: "log".into(),
        req: VersionReq::parse("^2.0").unwrap(),
        from: "other".into(),
    };
    let winner2 = select_highest_compatible("log", &[&c3], &candidates)
        .expect("^2.0 should resolve to 2.0.0");
    assert_eq!(winner2.to_string(), "2.0.0");

    // Conflicting ^1.0 && ^2.0 → E2602.
    let err = select_highest_compatible("log", &[&c1, &c3], &candidates)
        .expect_err("incompatible constraints must fail");
    assert_eq!(err.code, "E2602", "conflict must be E2602");
    let detail = err.detail.as_deref().unwrap_or_default();
    assert!(detail.contains("PubGrub proof tree:"));
    assert!(detail.contains("Smallest fixes:"));
    assert!(detail.contains("app") && detail.contains("other"));
}

#[test]
fn resolver_no_candidates_returns_e2602() {
    // With an empty candidate list and a single constraint, we get E2602.
    use jet::Publish::{select_highest_compatible, VersionConstraint, VersionReq};

    let c = VersionConstraint {
        package: "missing".into(),
        req: VersionReq::parse("^1.0").unwrap(),
        from: "app".into(),
    };
    let err = select_highest_compatible("missing", &[&c], &[])
        .expect_err("no candidates must be an error");
    assert_eq!(err.code, "E2602");
    assert!(err
        .detail
        .as_deref()
        .unwrap_or_default()
        .contains("PubGrub proof tree:"));
}

// ============================================================================
// Section: `pub(package)` visibility (was tests/pub_package.rs)
//
// Uses its own `Scratch` helper rather than the `tmp_dir`/`write` pair above:
// unlike `tmp_dir`, `Scratch` mixes a nanosecond suffix into the dir name (so
// concurrent test runs never collide on a bare label) and cleans up via
// `Drop`. Kept separate rather than forced onto the shared helper.
// ============================================================================

#[test]
fn pub_package_function_is_visible_inside_project_scope() {
    let s = Scratch::new("same");
    fs::write(
        s.join("helper.jet"),
        "pub(package) fn secret() => String {\n    return \"ok\"\n}\n",
    )
    .unwrap();
    fs::write(
        s.join("run.jet"),
        "use helper;\n\nfn run() {\n    print(helper.secret())\n}\n",
    )
    .unwrap();

    let diags = jet::check_with_path(&s.join("run.jet").to_string_lossy());
    assert!(
        diags.is_empty(),
        "expected same-package access to pass, got {diags:?}"
    );
}

#[test]
fn pub_package_function_is_hidden_from_path_dependency_consumer() {
    let s = Scratch::new("dep");
    let app = s.join("app");
    let dep = s.join("dep");
    fs::create_dir_all(&app).unwrap();
    fs::create_dir_all(&dep).unwrap();
    fs::write(
        app.join("package.jet"),
        "name: \"app\"\nversion: \"0.1.0\"\ndeps: .{ dep: ../dep }\n",
    )
    .unwrap();
    fs::write(
        app.join("run.jet"),
        "use dep;\n\nfn run() {\n    print(dep.secret())\n}\n",
    )
    .unwrap();
    fs::write(
        dep.join("package.jet"),
        "name: \"dep\"\nversion: \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        dep.join("dep.jet"),
        "pub(package) fn secret() => String {\n    return \"hidden\"\n}\n",
    )
    .unwrap();

    let diags = jet::check_with_path(&app.join("run.jet").to_string_lossy());
    assert!(
        diags.iter().any(|d| d.code == "E0605"),
        "expected downstream access to report E0605, got {diags:?}"
    );
}

#[test]
fn pub_package_type_and_field_are_visible_inside_project_scope() {
    let s = Scratch::new("type");
    fs::write(
        s.join("helper.jet"),
        "pub(package) struct Secret {\n    pub(package) value: String\n}\n\npub fn make() => Secret {\n    return Secret.{ value: \"ok\" }\n}\n",
    )
    .unwrap();
    fs::write(
        s.join("run.jet"),
        "use helper;\n\nfn run() {\n    s :: helper.make()\n    print(s.value)\n}\n",
    )
    .unwrap();

    let diags = jet::check_with_path(&s.join("run.jet").to_string_lossy());
    assert!(
        diags.is_empty(),
        "expected same-package type/field access to pass, got {diags:?}"
    );
}

// ─────────────────────────────────────────────
// c146 (D-PKGSIGN1): package signing tier A — Ed25519
// ─────────────────────────────────────────────

fn have_cargo() -> bool {
    Command::new("cargo").arg("--version").output().is_ok()
}

/// Run `f` with JET_KEYS_DIR pointed at `dir`, serialized against concurrent
/// env mutation (same shape as `with_store`).
fn with_keys<T, F: FnOnce() -> T>(dir: &Path, f: F) -> T {
    let _guard = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let prev = std::env::var("JET_KEYS_DIR").ok();
    std::env::set_var("JET_KEYS_DIR", dir);
    let result = f();
    match prev {
        Some(v) => std::env::set_var("JET_KEYS_DIR", v),
        None => std::env::remove_var("JET_KEYS_DIR"),
    }
    result
}

fn signed_entry(
    name: &str,
    version: &str,
    content_hash: &str,
    public_key: &str,
    signature: &str,
) -> jet::Publish::IndexEntry {
    jet::Publish::IndexEntry {
        name: name.to_string(),
        version: version.to_string(),
        content_hash: content_hash.to_string(),
        fingerprint: "sha256-fp".to_string(),
        yanked: false,
        tier: jet::Publish::RegistryTier::Core,
        gate_status: jet::Publish::GateStatus::core_reviewed(),
        public_key: public_key.to_string(),
        signature: signature.to_string(),
    }
}

#[test]
fn cli_signed_advisory_feed_receipt_and_tamper_fail_closed() {
    if !have_cargo() {
        eprintln!("note: skipping signed advisory feed audit (cargo not found)");
        return;
    }
    let tmp = tmp_dir("signed_advisory_audit");
    let project = tmp.join("project");
    let keys = tmp.join("keys");
    fs::create_dir_all(project.join(".jet")).unwrap();
    fs::write(project.join("package.jet"), min_manifest("app", "0.1.0")).unwrap();
    fs::write(
        project.join(".jet/lock"),
        jet::Lock::write(&make_test_lock("mylib", "1.0.0", "sha256-locked")),
    )
    .unwrap();

    with_keys(&keys, || {
        let (seed, _public_path, public_key) =
            jet::Publish::Sign::keygen("advisory", false).expect("advisory keygen should succeed");
        let key_id = jet::Publish::advisory_key_id(&public_key).unwrap();
        let mut feed = jet::Publish::AdvisoryFeed {
            sequence: 1,
            issued_at: 100,
            expires_at: 10_000,
            maturity_seconds: 86_400,
            key_id,
            public_key: public_key.clone(),
            signature: String::new(),
            releases: Vec::new(),
            advisories: Vec::new(),
            exceptions: Vec::new(),
        };
        feed.signature = jet::Publish::sign_advisory_feed(&feed, &seed)
            .expect("advisory feed signing should succeed");
        let feed_path = project.join(".jet/advisories.db");
        fs::write(&feed_path, jet::Publish::advisory_feed_text(&feed)).unwrap();
        fs::write(
            project.join(".jet/advisory-trust"),
            format!("public_key={}\nmin_sequence=1\n", public_key),
        )
        .unwrap();
        let audit = || {
            Command::new(jet_bin())
                .args(["inspect", "audit"])
                .current_dir(&project)
                .env("JET_ADVISORY_NOW", "200")
                .env("NO_COLOR", "1")
                .output()
                .unwrap()
        };

        let clean = audit();
        assert_eq!(clean.status.code(), Some(0), "stderr={:?}", clean.stderr);
        let stdout = String::from_utf8_lossy(&clean.stdout);
        assert!(
            stdout.contains("feed sequence 1 verified"),
            "stdout={stdout}"
        );
        assert!(stdout.contains("no advisories found"), "stdout={stdout}");

        fs::remove_file(project.join(".jet/advisory-trust")).unwrap();
        let key_only = Command::new(jet_bin())
            .args(["inspect", "audit"])
            .current_dir(&project)
            .env("JET_ADVISORY_NOW", "200")
            .env("JET_ADVISORY_PUBLIC_KEY", &public_key)
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert_eq!(key_only.status.code(), Some(1));
        assert!(
            String::from_utf8_lossy(&key_only.stderr).contains("advisory trust root"),
            "key-only environment input must not replace the pinned trust root: {}",
            String::from_utf8_lossy(&key_only.stderr)
        );
        fs::write(
            project.join(".jet/advisory-trust"),
            format!("public_key={}\nmin_sequence=1\n", public_key),
        )
        .unwrap();

        let mut weakened = feed.clone();
        weakened.maturity_seconds = 1;
        weakened.signature = jet::Publish::sign_advisory_feed(&weakened, &seed)
            .expect("weakened feed signing should succeed");
        fs::write(&feed_path, jet::Publish::advisory_feed_text(&weakened)).unwrap();
        let weakened_result = audit();
        assert_eq!(weakened_result.status.code(), Some(1));
        assert!(
            String::from_utf8_lossy(&weakened_result.stderr)
                .contains("weaker than the 24-hour default"),
            "a signed feed must not weaken the default maturity policy: {}",
            String::from_utf8_lossy(&weakened_result.stderr)
        );
        assert_eq!(
            String::from_utf8(weakened_result.stderr).unwrap(),
            include_str!("cli/advisory_weakened_maturity_e2610.txt")
        );
        fs::write(&feed_path, jet::Publish::advisory_feed_text(&feed)).unwrap();

        fs::write(
            project.join(".jet/advisory-trust"),
            format!("public_key={}\nmin_sequence=2\n", public_key),
        )
        .unwrap();
        let rollback = audit();
        assert_eq!(rollback.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&rollback.stderr).contains("rolled back"));

        fs::write(
            project.join(".jet/advisory-trust"),
            format!(
                "public_key={}\nmin_sequence=1\nrevoked_key={}\n",
                public_key, public_key
            ),
        )
        .unwrap();
        let compromised = audit();
        assert_eq!(compromised.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&compromised.stderr).contains("revoked"));

        fs::write(
            project.join(".jet/advisory-trust"),
            format!("public_key={}\nmin_sequence=1\n", public_key),
        )
        .unwrap();
        let accepted_digest = format!(
            "sha256-{}",
            jet::SHA256::sha256_hex(jet::Publish::advisory_feed_text(&feed).as_bytes())
        );
        fs::write(
            project.join(".jet/advisory-trust"),
            format!(
                "public_key={}\nmin_sequence=1\naccepted_digest={}\n",
                public_key, accepted_digest
            ),
        )
        .unwrap();
        let mut fork = feed.clone();
        fork.maturity_seconds = 86_401;
        fork.signature = jet::Publish::sign_advisory_feed(&fork, &seed)
            .expect("fork feed signing should succeed");
        fs::write(&feed_path, jet::Publish::advisory_feed_text(&fork)).unwrap();
        let mixed = audit();
        assert_eq!(mixed.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&mixed.stderr).contains("forked"));

        fs::write(
            project.join(".jet/advisory-trust"),
            format!("public_key={}\nmin_sequence=1\n", public_key),
        )
        .unwrap();
        let mut stale = feed.clone();
        stale.expires_at = 150;
        stale.signature = jet::Publish::sign_advisory_feed(&stale, &seed)
            .expect("stale feed signing should succeed");
        fs::write(&feed_path, jet::Publish::advisory_feed_text(&stale)).unwrap();
        let frozen = audit();
        assert_eq!(frozen.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&frozen.stderr).contains("stale or expired"));

        fs::write(
            project.join(".jet/advisory-trust"),
            format!("public_key={}\nmin_sequence=1\n", public_key),
        )
        .unwrap();
        fs::write(&feed_path, jet::Publish::advisory_feed_text(&feed)).unwrap();

        let mut immature_feed = feed.clone();
        immature_feed.releases.push(jet::Publish::AdvisoryRelease {
            package: "mylib".into(),
            version: jet::Publish::SemVer::SemVer::parse("1.0.0").unwrap(),
            first_seen: 100,
            source_class: jet::Publish::SourceClass::ThirdParty,
        });
        immature_feed.signature = jet::Publish::sign_advisory_feed(&immature_feed, &seed)
            .expect("immature feed signing should succeed");
        fs::write(&feed_path, jet::Publish::advisory_feed_text(&immature_feed)).unwrap();
        let immature = audit();
        assert_eq!(
            immature.status.code(),
            Some(1),
            "immature release must fail closed"
        );
        assert_eq!(
            String::from_utf8(immature.stderr).unwrap(),
            include_str!("cli/audit_maturity_e2609.txt")
        );

        fs::write(
            project.join("package.jet"),
            format!(
                "{}\npolicy: {{ exceptions: [PolicyException.{{ id: \"JSA-2026-0001\", scope: \"mylib#1.0.0\", reason: \"urgent security fix\", expires: 1000 }}] }}\n",
                min_manifest("app", "0.1.0")
            ),
        )
        .unwrap();
        let source_excepted = audit();
        assert_eq!(
            source_excepted.status.code(),
            Some(0),
            "the source-owned exact exception should clear only maturity: {}",
            String::from_utf8_lossy(&source_excepted.stderr)
        );
        assert!(
            String::from_utf8_lossy(&source_excepted.stdout)
                .contains("source policy exception applied: id=JSA-2026-0001"),
            "audit must show the source exception evidence: {}",
            String::from_utf8_lossy(&source_excepted.stdout)
        );
        fs::write(project.join("package.jet"), min_manifest("app", "0.1.0")).unwrap();

        let mut excepted_feed = immature_feed;
        excepted_feed
            .exceptions
            .push(jet::Publish::AdvisoryException {
                package: "mylib".into(),
                version: jet::Publish::SemVer::SemVer::parse("1.0.0").unwrap(),
                reason: "urgent security fix".into(),
                reviewer: "security-team".into(),
                expires_at: 1_000,
            });
        excepted_feed.signature = jet::Publish::sign_advisory_feed(&excepted_feed, &seed)
            .expect("exception feed signing should succeed");
        fs::write(&feed_path, jet::Publish::advisory_feed_text(&excepted_feed)).unwrap();
        let excepted = audit();
        assert_eq!(
            excepted.status.code(),
            Some(0),
            "exact exception should allow the release"
        );
        assert!(String::from_utf8_lossy(&excepted.stdout).contains("no advisories found"));

        let tampered = jet::Publish::advisory_feed_text(&jet::Publish::AdvisoryFeed {
            maturity_seconds: 86_401,
            ..feed
        });
        fs::write(feed_path, tampered).unwrap();
        let rejected = audit();
        assert_eq!(rejected.status.code(), Some(1), "tamper must fail closed");
        assert!(
            String::from_utf8_lossy(&rejected.stderr).contains("Error [E2610]:"),
            "stderr={}",
            String::from_utf8_lossy(&rejected.stderr)
        );
        assert_eq!(
            String::from_utf8(rejected.stderr).unwrap(),
            include_str!("cli/audit_tampered_e2610.txt")
        );
    });

    let _ = fs::remove_dir_all(tmp);
}

#[test]
fn cli_publish_signs_index_and_auto_keygens() {
    // c146: `jet registry publish` auto-generates a key on first use, signs by default,
    // and pins the public key + signature into the index line.
    if !jet_bin().is_file() || !have_git() || !have_cargo() {
        eprintln!("note: skipping cli_publish_signs (need built binary, git, cargo)");
        return;
    }

    let tmp = tmp_dir("pub_sign");
    let proj = tmp.join("proj");
    fs::create_dir_all(&proj).unwrap();
    let bare = tmp.join("registry.git");
    let url = bare_registry(&bare);
    let cache = tmp.join("cache");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();
    let keys = tmp.join("keys");
    init_clean_project(&proj, "signedkit", "1.0.0");
    seed_core_review(&bare, "signedkit", "1.0.0");

    let envs = &[
        ("JET_REGISTRY_URL", url.as_str()),
        ("JET_REGISTRY_CACHE_DIR", cache.to_str().unwrap()),
        ("JET_STORE_DIR", store.to_str().unwrap()),
        ("JET_KEYS_DIR", keys.to_str().unwrap()),
    ];

    let out = jet_cmd_env(&["registry", "publish"], &proj, envs);
    assert!(
        out.status.success(),
        "signed publish should succeed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let index = read_index_file(&bare, "signedkit").expect("index file must exist");
    assert!(
        index.contains("\"public_key\":\"") && !index.contains("\"public_key\":\"\""),
        "the first published version must pin a non-empty public key:\n{index}"
    );
    assert!(
        index.contains("\"signature\":\"") && !index.contains("\"signature\":\"\""),
        "a default publish must carry a non-empty signature:\n{index}"
    );
    // Auto-keygen wrote the key files at the documented paths.
    assert!(
        keys.join("jet.ed25519").is_file(),
        "auto-keygen must write the secret seed"
    );
    assert!(
        keys.join("jet.ed25519.pub").is_file(),
        "auto-keygen must write the public key file"
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[cfg(unix)]
#[test]
fn cli_keygen_entropy_failure_is_exact_e1292_and_artifact_free() {
    let _guard = STORE_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let tmp = tmp_dir("keygen_entropy_failure");
    let home = tmp.join("home");
    let keys = tmp.join("keys");
    fs::create_dir_all(&home).unwrap();
    let process_home = std::env::var_os("HOME");
    install_closed_status_crypto_helper(&home);
    assert_eq!(std::env::var_os("HOME"), process_home);

    let out = jet_cmd_env(
        &["registry", "keygen"],
        &tmp,
        &[
            ("HOME", home.to_str().unwrap()),
            ("JET_KEYS_DIR", keys.to_str().unwrap()),
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(
        out.stdout.is_empty(),
        "stdout must stay empty: {:?}",
        out.stdout
    );
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        include_str!("fixtures/jetpack-diagnostics/keygen_entropy_unavailable.stderr")
    );
    assert!(!keys.exists(), "entropy failure created a key directory");

    fs::remove_dir_all(&tmp).unwrap();
}

#[cfg(unix)]
#[test]
fn cli_publish_auto_keygen_entropy_failure_mutates_nothing() {
    if !have_git() {
        eprintln!("note: skipping auto-keygen entropy failure (git not found)");
        return;
    }
    let _guard = STORE_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let tmp = tmp_dir("publish_entropy_failure");
    let home = tmp.join("home");
    let project = tmp.join("project");
    let bare = tmp.join("registry.git");
    let registry_url = bare_registry(&bare);
    let registry_cache = tmp.join("registry-cache");
    let store = tmp.join("store");
    let keys = tmp.join("keys");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&store).unwrap();
    install_closed_status_crypto_helper(&home);
    init_clean_project(&project, "entropyfail", "1.0.0");

    let out = jet_cmd_env(
        &["registry", "publish"],
        &project,
        &[
            ("HOME", home.to_str().unwrap()),
            ("JET_KEYS_DIR", keys.to_str().unwrap()),
            ("JET_REGISTRY_URL", registry_url.as_str()),
            ("JET_REGISTRY_CACHE_DIR", registry_cache.to_str().unwrap()),
            ("JET_STORE_DIR", store.to_str().unwrap()),
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(
        out.stdout.is_empty(),
        "stdout must stay empty: {:?}",
        out.stdout
    );
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        include_str!("fixtures/jetpack-diagnostics/keygen_entropy_unavailable.stderr")
    );
    assert!(!keys.exists(), "auto-keygen failure created key artifacts");
    assert!(
        !registry_cache.exists(),
        "auto-keygen failure cloned an index"
    );
    assert!(
        read_index_file(&bare, "entropyfail").is_none(),
        "auto-keygen failure mutated the registry index"
    );
    assert!(
        !project.join(".jet/cache/api").exists() && !project.join(".jet/cache/schema").exists(),
        "auto-keygen failure created package snapshots"
    );

    fs::remove_dir_all(&tmp).unwrap();
}

#[cfg(unix)]
#[test]
fn cli_publish_existing_key_bypasses_entropy_keygen() {
    if !have_git() {
        eprintln!("note: skipping existing-key entropy bypass (git not found)");
        return;
    }
    let _guard = STORE_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let tmp = tmp_dir("publish_existing_key");
    let home = tmp.join("home");
    let project = tmp.join("project");
    let bare = tmp.join("registry.git");
    let registry_url = bare_registry(&bare);
    let registry_cache = tmp.join("registry-cache");
    let store = tmp.join("store");
    let keys = tmp.join("keys");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&store).unwrap();
    fs::create_dir_all(&keys).unwrap();
    install_closed_status_crypto_helper(&home);
    init_clean_project(&project, "existingkey", "1.0.0");
    seed_core_review(&bare, "existingkey", "1.0.0");
    let seed = vec![0x5au8; 32];
    fs::write(keys.join("jet.ed25519"), &seed).unwrap();
    fs::write(keys.join("jet.ed25519.pub"), "00".repeat(32)).unwrap();

    let out = jet_cmd_env(
        &["registry", "publish"],
        &project,
        &[
            ("HOME", home.to_str().unwrap()),
            ("JET_KEYS_DIR", keys.to_str().unwrap()),
            ("JET_REGISTRY_URL", registry_url.as_str()),
            ("JET_REGISTRY_CACHE_DIR", registry_cache.to_str().unwrap()),
            ("JET_STORE_DIR", store.to_str().unwrap()),
        ],
    );
    assert!(
        out.status.success(),
        "existing-key publish failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(fs::read(keys.join("jet.ed25519")).unwrap(), seed);
    assert!(read_index_file(&bare, "existingkey").is_some());

    fs::remove_dir_all(&tmp).unwrap();
}

#[test]
fn cli_publish_no_sign_leaves_signature_empty() {
    // c146: `--no-sign` opts out; tier-B checksum (content_hash) still present.
    if !jet_bin().is_file() || !have_git() {
        eprintln!("note: skipping cli_publish_no_sign (need built binary, git)");
        return;
    }

    let tmp = tmp_dir("pub_nosign");
    let proj = tmp.join("proj");
    fs::create_dir_all(&proj).unwrap();
    let bare = tmp.join("registry.git");
    let url = bare_registry(&bare);
    let cache = tmp.join("cache");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();
    let keys = tmp.join("keys");
    init_clean_project(&proj, "plainkit", "1.0.0");
    seed_core_review(&bare, "plainkit", "1.0.0");

    let envs = &[
        ("JET_REGISTRY_URL", url.as_str()),
        ("JET_REGISTRY_CACHE_DIR", cache.to_str().unwrap()),
        ("JET_STORE_DIR", store.to_str().unwrap()),
        ("JET_KEYS_DIR", keys.to_str().unwrap()),
    ];

    let out = jet_cmd_env(&["registry", "publish", "--no-sign"], &proj, envs);
    assert!(
        out.status.success(),
        "--no-sign publish should succeed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let index = read_index_file(&bare, "plainkit").expect("index file must exist");
    assert!(
        index.contains("\"signature\":\"\""),
        "--no-sign must leave the signature empty:\n{index}"
    );
    // No key was generated (no signing happened).
    assert!(
        !keys.join("jet.ed25519").is_file(),
        "--no-sign must not auto-generate a key"
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn cli_community_publish_refuses_until_all_named_gates_pass() {
    if !jet_bin().is_file() || !have_git() {
        eprintln!("note: skipping community gate refusal (need built binary and git)");
        return;
    }

    let tmp = tmp_dir("pub_community_closed");
    let proj = tmp.join("proj");
    fs::create_dir_all(&proj).unwrap();
    let bare = tmp.join("registry.git");
    let url = bare_registry(&bare);
    let cache = tmp.join("cache");
    let store = tmp.join("store");
    let keys = tmp.join("keys");
    fs::create_dir_all(&store).unwrap();
    init_clean_project(&proj, "communitykit", "1.0.0");

    let out = jet_cmd_env(
        &["registry", "publish", "--no-sign"],
        &proj,
        &[
            ("JET_REGISTRY_URL", url.as_str()),
            ("JET_REGISTRY_TIER", "community"),
            ("JET_REGISTRY_CACHE_DIR", cache.to_str().unwrap()),
            ("JET_STORE_DIR", store.to_str().unwrap()),
            ("JET_KEYS_DIR", keys.to_str().unwrap()),
        ],
    );
    assert!(!out.status.success(), "community publish must stay closed");
    let stderr = String::from_utf8_lossy(&out.stderr);
    for gate in ["#935", "#431", "#1912", "#1913"] {
        assert!(
            stderr.contains(gate),
            "missing {gate} in refusal:\n{stderr}"
        );
    }
    assert!(read_index_file(&bare, "communitykit").is_none());
    fs::remove_dir_all(tmp).unwrap();
}

#[test]
fn sign_verify_roundtrip_and_tamper_e1246() {
    // c146: a signed entry verifies clean; tampering the content_hash makes the
    // signature fail verification (E1246 — never silently accept tampered bytes).
    if !have_cargo() {
        eprintln!("note: skipping sign_verify_roundtrip (cargo not found)");
        return;
    }
    let tmp = tmp_dir("sign_verify");
    let keys = tmp.join("keys");

    with_keys(&keys, || {
        let (seed_a, _pub_path, pub_a) =
            jet::Publish::Sign::keygen("jet", false).expect("keygen should succeed");
        let content_hash = "sha256-0011223344556677";
        let sig = jet::Publish::Sign::sign(&seed_a, content_hash).expect("sign should succeed");

        let good = signed_entry("textkit", "1.0.0", content_hash, &pub_a, &sig);
        let all = vec![good.clone()];
        jet::Publish::verify_index_entry(&all, &good, false, "jet")
            .expect("an untampered signature must verify");

        // Direct Sign::verify roundtrip too.
        assert!(
            jet::Publish::Sign::verify(&pub_a, content_hash, &sig).unwrap(),
            "Sign::verify must accept a genuine signature"
        );

        // Tamper the content hash → signature no longer matches → E1246.
        let mut bad = good.clone();
        bad.content_hash = "sha256-TAMPEREDdeadbeef".to_string();
        let all_bad = vec![bad.clone()];
        let err = jet::Publish::verify_index_entry(&all_bad, &bad, false, "jet")
            .expect_err("a tampered content hash must fail verification");
        assert_eq!(err.code, "E1246", "tamper must cite E1246");
    });

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn require_signed_unsigned_entry_e1247() {
    // c146: a registry with require_signed = true refuses an entry with no
    // signature (E1247). No crypto needed — the check precedes verification.
    let unsigned = signed_entry("corpkit", "1.0.0", "sha256-abc", "", "");
    let all = vec![unsigned.clone()];
    let err = jet::Publish::verify_index_entry(&all, &unsigned, true, "corp")
        .expect_err("require_signed + no signature must error");
    assert_eq!(err.code, "E1247", "must cite E1247");

    // With require_signed = false the same unsigned entry is allowed.
    jet::Publish::verify_index_entry(&all, &unsigned, false, "corp")
        .expect("an unsigned entry is fine when the registry doesn't require signing");
}

#[test]
fn takeover_requires_review_and_resigning() {
    // #1913: a changed package key is a takeover. The new key must sign the
    // release and the registry must carry an approved maintainer review.
    if !have_cargo() {
        eprintln!("note: skipping takeover (cargo not found)");
        return;
    }

    let tmp = tmp_dir("takeover");
    let keys = tmp.join("keys");
    with_keys(&keys, || {
        let (seed_a, _pa, pub_a) = jet::Publish::Sign::keygen("jet", false).unwrap();
        let original_seed = fs::read(&seed_a).unwrap();
        let (seed_b, _pb, pub_b) = jet::Publish::Sign::keygen("other", false).unwrap();
        assert_ne!(pub_a, pub_b, "the two keys must differ");

        let ch1 = "sha256-v1v1v1";
        let ch2 = "sha256-v2v2v2";
        let sig1 = jet::Publish::Sign::sign(&seed_a, ch1).unwrap();
        let old_key_sig = jet::Publish::Sign::sign(&seed_a, ch2).unwrap();
        let new_key_sig = jet::Publish::Sign::sign(&seed_b, ch2).unwrap();

        let v1 = signed_entry("textkit", "1.0.0", ch1, &pub_a, &sig1);
        let v2_old_key = signed_entry("textkit", "2.0.0", ch2, &pub_b, &old_key_sig);
        let v2 = signed_entry("textkit", "2.0.0", ch2, &pub_b, &new_key_sig);
        let repo = tmp.join("registry");
        fs::create_dir_all(&repo).unwrap();
        jet::Publish::Index::write_index_entry(&repo, &v1).unwrap();

        let missing_review = jet::Publish::Index::write_index_entry(&repo, &v2)
            .expect_err("takeover without review must be refused");
        assert!(
            missing_review
                .to_string()
                .contains("no approved registry review"),
            "missing review must be named: {missing_review}"
        );

        let review = repo.join("reviews/textkit/2.0.0.review");
        fs::create_dir_all(review.parent().unwrap()).unwrap();
        fs::write(
            review,
            "jet-registry-core-review-v1\npackage=textkit\nversion=2.0.0\nreviewer=registry-owner\ndecision=approved\n",
        )
        .unwrap();

        let wrong_signature = jet::Publish::Index::write_index_entry(&repo, &v2_old_key)
            .expect_err("takeover signed by the old key must be refused");
        assert!(
            wrong_signature
                .to_string()
                .contains("signature verification failed"),
            "wrong signer must be named: {wrong_signature}"
        );
        jet::Publish::Index::write_index_entry(&repo, &v2)
            .expect("reviewed takeover signed by the new key must pass");
        let all = jet::Publish::Index::read_entries(&repo, "textkit").unwrap();
        jet::Publish::verify_index_entry(&all, &v2, false, "jet")
            .expect("reviewed takeover signature must verify");
        assert_eq!(fs::read(seed_a).unwrap(), original_seed);
        fs::remove_dir_all(repo).unwrap();
    });
    fs::remove_dir_all(tmp).unwrap();
}

#[test]
fn keygen_refuses_existing_key_e1248() {
    // c146: `jet registry keygen` refuses to overwrite an existing key without --force.
    if !have_cargo() {
        eprintln!("note: skipping keygen_refuses_existing_key (cargo not found)");
        return;
    }
    let tmp = tmp_dir("keygen_refuse");
    let keys = tmp.join("keys");

    with_keys(&keys, || {
        let (seed, public, _) =
            jet::Publish::Sign::keygen("jet", false).expect("first keygen should succeed");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&seed).unwrap().permissions().mode() & 0o7777,
                0o600,
                "private signing seed must remain owner-only"
            );
            assert_eq!(
                fs::metadata(&public).unwrap().permissions().mode() & 0o7777,
                0o644,
                "public signing key keeps its published mode"
            );
        }
        let err = jet::Publish::Sign::keygen("jet", false)
            .expect_err("a second keygen without --force must refuse");
        assert_eq!(err.code, "E1248", "must cite E1248");
        // --force overwrites.
        jet::Publish::Sign::keygen("jet", true).expect("--force keygen must overwrite");
    });

    let _ = fs::remove_dir_all(&tmp);
}
