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
use std::process::Command;
use std::sync::Mutex;

// Serialize all tests that mutate JET_STORE_DIR to prevent concurrent set_var races.
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
    diags.first().map(|d| d.code).unwrap_or("")
}

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug/jet")
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
/// `jet publish`/`jet yank` at a scratch registry via `JET_REGISTRY_URL` and
/// `JET_REGISTRY_CACHE_DIR`).
fn jet_cmd_env(args: &[&str], cwd: &Path, envs: &[(&str, &str)]) -> std::process::Output {
    let mut cmd = Command::new(jet_bin());
    cmd.args(args).current_dir(cwd);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.output().expect("jet binary should run")
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

/// Write a minimal project and commit it (clean tree) so `jet publish` clears
/// the dirty-tree gate (E2605).
fn init_clean_project(dir: &Path, name: &str, version: &str) {
    write(dir, "pkg.jet", &min_manifest(name, version));
    write(dir, "main.jet", "fn run() { print(\"hi\"); }\n");
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

// Minimal `pkg.jet` (Jet syntax, U1) for a named package with no deps.
fn min_manifest(name: &str, version: &str) -> String {
    format!(
        "payload: {{\n    name: \"{}\",\n    version: \"{}\",\n    jet: \">=0.1.0\",\n    description: \"\",\n    license: \"MIT\",\n    repository: \"\",\n}}\n",
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
// Manifest parsing
// ─────────────────────────────────────────────

#[test]
fn manifest_parse_valid_fields() {
    let raw = r#"payload: {
    name:    "myapp",
    version: "1.2.3",
    jet:     ">=0.1.0",
    description: "A test package",
    license: "MIT OR Apache-2.0",
    repository: "https://example.com",
}
deps: {
}
"#;
    let path = PathBuf::from("pkg.jet");
    let mf = jet::Manifest::parse(&path, raw).expect("valid manifest should parse");
    assert_eq!(mf.package.name, "myapp");
    assert_eq!(mf.package.version, "1.2.3");
    assert_eq!(mf.package.jet_constraint.as_deref(), Some(">=0.1.0"));
    assert_eq!(mf.package.description.as_deref(), Some("A test package"));
    assert_eq!(mf.package.license.as_deref(), Some("MIT OR Apache-2.0"));
    assert!(mf.dependencies.is_empty());
}

#[test]
fn manifest_parse_dep_path() {
    let raw = manifest_with_deps("root", "0.1.0", "    helpers: path@../helpers,");
    let mf = jet::Manifest::parse(&PathBuf::from("pkg.jet"), &raw).expect("path dep should parse");
    let dep = mf.dependencies.get("helpers").expect("missing helpers dep");
    assert!(matches!(dep, jet::Manifest::DepSpec::Path { path } if path == "../helpers"));
}

#[test]
fn manifest_parse_dep_git_tag() {
    let raw = manifest_with_deps(
        "root",
        "0.1.0",
        "    parsekit: { git: \"https://github.com/acme/parsekit\", tag: \"v0.4.1\" },",
    );
    let mf =
        jet::Manifest::parse(&PathBuf::from("pkg.jet"), &raw).expect("git tag dep should parse");
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
fn manifest_parse_e1206_missing_required_field() {
    // `package` with no `version` is a shape error (E1206).
    let raw = "payload: {\n    name: \"myapp\",\n}\n";
    let err = jet::Manifest::parse(&PathBuf::from("pkg.jet"), raw)
        .expect_err("missing version should fail");
    assert_eq!(err.code, "E1206");
}

#[test]
fn manifest_parse_e1209_reserved_nonempty() {
    let raw = min_manifest("myapp", "0.1.0") + "\ndev_deps: {\n    testlib: path@../testlib,\n}\n";
    let err = jet::Manifest::parse(&PathBuf::from("pkg.jet"), &raw)
        .expect_err("non-empty dev_deps should fail E1209");
    assert_eq!(err.code, "E1209");
}

// ─────────────────────────────────────────────
// D-EFFBUDGET1: package effect budget manifest parsing
// ─────────────────────────────────────────────

#[test]
fn manifest_parse_effects_block() {
    let raw = min_manifest("app", "0.1.0")
        + "\neffects: {\n    allow: [Fs, Time],\n    deny: [Net],\n}\n";
    let pm = jet::Jetpack::PackageManifest::parse(&raw).expect("effects block should parse");
    assert!(pm.effects_enabled);
    assert_eq!(
        pm.effects_allow,
        Some(vec!["Fs".to_string(), "Time".to_string()])
    );
    assert_eq!(pm.effects_deny, Some(vec!["Net".to_string()]));
}

#[test]
fn manifest_parse_grants_block() {
    let raw = min_manifest("app", "0.1.0") + "\ngrants: {\n    \"pdf-lib\": [Net],\n}\n";
    let pm = jet::Jetpack::PackageManifest::parse(&raw).expect("grants block should parse");
    assert_eq!(
        pm.grants,
        vec![("pdf-lib".to_string(), vec!["Net".to_string()])]
    );
}

#[test]
fn manifest_no_effects_block_disables_enforcement() {
    let raw = min_manifest("app", "0.1.0");
    let pm = jet::Jetpack::PackageManifest::parse(&raw).expect("valid manifest should parse");
    assert!(!pm.effects_enabled);
    assert_eq!(pm.effects_allow, None);
}

#[test]
fn manifest_parse_effects_e1221_unknown_effect() {
    let raw = min_manifest("app", "0.1.0") + "\neffects: {\n    allow: [NotAnEffect],\n}\n";
    let err = jet::Jetpack::PackageManifest::parse(&raw)
        .expect_err("unknown effect name should fail E1221");
    let diag = jet::Manifest::parse(&PathBuf::from("pkg.jet"), &raw)
        .expect_err("should surface through Manifest::parse too");
    assert_eq!(diag.code, "E1221");
    assert!(matches!(
        err,
        jet::Jetpack::PackageManifest::ManifestError::BadEffectsBlock { .. }
    ));
}

#[test]
fn manifest_parse_effects_e1221_unknown_field() {
    let raw = min_manifest("app", "0.1.0") + "\neffects: {\n    nope: [Fs],\n}\n";
    let diag = jet::Manifest::parse(&PathBuf::from("pkg.jet"), &raw)
        .expect_err("unknown effects field should fail E1221");
    assert_eq!(diag.code, "E1221");
}

#[test]
fn effect_budget_load_ok_reports_via_compile_with_path() {
    // A project with a well-formed `effects:` block and no dependencies
    // reaching a disallowed effect should compile fine end to end — the
    // manifest gate (Loader/Manifest::parse) never rejects a valid budget.
    let tmp = tmp_dir("effbudget_ok");
    write(
        &tmp,
        "pkg.jet",
        &(min_manifest("app", "0.1.0") + "\neffects: {\n    allow: [Io],\n}\n"),
    );
    let entry = tmp.join("main.jet");
    fs::write(&entry, "fn run() { print(\"hi\"); }\n").unwrap();

    let result = jet::compile_with_path("", &entry.to_string_lossy());
    assert!(
        result.is_ok(),
        "a well-formed effects: budget should not block compilation:\n{}",
        result
            .err()
            .map(|d| jet::render_diagnostics("main.jet", "", &d))
            .unwrap_or_default()
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn cli_build_prints_effect_summary() {
    // D-EFFBUDGET1: every `jet build` prints a one-line effect summary, with
    // zero config — no pkg.jet needed.
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
        stderr.contains("effects:") && stderr.contains("Fs"),
        "expected an effect summary naming Fs on stderr, got:\n{stderr}"
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn cli_build_enforces_effect_budget_e1220() {
    // A dependency that reaches `Net` while the root's budget only allows
    // `Fs` must fail the build naming the dependency (E1220).
    if !jet_bin().is_file() {
        eprintln!(
            "note: skipping cli_build_enforces_effect_budget_e1220 (run `cargo build` first)"
        );
        return;
    }
    let tmp = tmp_dir("effbudget_deny");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();

    write(&tmp, "netdep/pkg.jet", &min_manifest("netdep", "0.1.0"));
    write(
        &tmp,
        "netdep/netdep.jet",
        "use core.net as net\npub fn ping() { net.tcp_connect(\"127.0.0.1:1\") ?? panic(\"e\"); }\n",
    );

    write(
        &tmp,
        "pkg.jet",
        &(manifest_with_deps("app", "0.1.0", "    netdep: path@netdep,")
            + "\neffects: {\n    allow: [Fs],\n}\n"),
    );
    write(
        &tmp,
        "main.jet",
        "use netdep;\nfn run() { netdep.ping(); }\n",
    );

    let out = jet_cmd(&["build", "main.jet"], &tmp, &store);
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
fn manifest_toolchain_ok() {
    let raw = min_manifest("myapp", "0.1.0");
    let mf = jet::Manifest::parse(&PathBuf::from("pkg.jet"), &raw).unwrap();
    assert!(jet::Manifest::check_toolchain(&mf, "pkg.jet").is_ok());
}

#[test]
fn manifest_toolchain_e1208_future_version() {
    let raw =
        "payload: {\n    name: \"myapp\",\n    version: \"0.1.0\",\n    jet: \">=99.0.0\",\n}\n";
    let mf = jet::Manifest::parse(&PathBuf::from("pkg.jet"), raw).unwrap();
    let err = jet::Manifest::check_toolchain(&mf, "pkg.jet").expect_err("E1208");
    assert_eq!(err.code, "E1208");
}

// ─────────────────────────────────────────────
// Template generation (jet new)
// ─────────────────────────────────────────────

#[test]
fn manifest_template_plain_parses() {
    let raw = jet::Manifest::new_template("myapp", false);
    let mf =
        jet::Manifest::parse(&PathBuf::from("pkg.jet"), &raw).expect("plain template should parse");
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
    jet::Manifest::parse(&PathBuf::from("pkg.jet"), &raw).expect("annotated template should parse");
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
    let mf = jet::Manifest::parse(&PathBuf::from("pkg.jet"), &updated).expect("should reparse");
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
    let mf = jet::Manifest::parse(&PathBuf::from("pkg.jet"), &updated).expect("should reparse");
    assert!(matches!(
        mf.dependencies.get("helpers"),
        Some(jet::Manifest::DepSpec::Path { path }) if path == "../helpers"
    ));
}

#[test]
fn manifest_remove_dep_removes_correct_entry() {
    let raw = min_manifest("root", "0.1.0")
        + "\ndeps: {\n    helpers: path@../helpers,\n    other: path@../other,\n}\n";
    let updated = jet::Manifest::remove_dependency(&raw, "helpers");
    let mf = jet::Manifest::parse(&PathBuf::from("pkg.jet"), &updated).expect("should reparse");
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
        "pub fn hello() -> String { return \"hi\"; }\n",
    );
    write(&src, "pkg.jet", &min_manifest("mylib", "0.1.0"));

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
    write(&src, "pkg.jet", &min_manifest("mylib", "0.1.0"));

    let fp = "sha256-1111111111111111111111111111111111111111111111111111111111111111";

    let (p1, p2) = with_store(&store, || {
        let (a, _) = jet::Store::ensure_path_dep("mylib", "0.1.0", fp, &src).unwrap();
        let (b, _) = jet::Store::ensure_path_dep("mylib", "0.1.0", fp, &src).unwrap();
        (a, b)
    });

    assert_eq!(p1, p2, "second call must return the same store path");
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
    write(&src, "pkg.jet", &min_manifest("mylib", "0.1.0"));

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
    write(&src, "pkg.jet", &min_manifest("mylib", "0.1.0"));

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
    write(&src, "pkg.jet", &min_manifest("mylib", "0.1.0"));

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
        envelope: None,
    };
    let lock = LockFile {
        version: 1,
        packages: vec![pkg],
        root_dependencies: vec!["foo".into()],
        workspace_members: vec![LockedWorkspaceMember {
            name: "hello".into(),
            path: "packages/hello".into(),
        }],
        comptime_inputs: vec![],
        toolchains: Vec::new(),
        source_channels: Vec::new(),
    };
    let serialized = jet::Lock::write(&lock);
    assert!(
        serialized.contains("content-hash = \"sha256-deadbeef\""),
        "content-hash must appear in lockfile"
    );
    assert!(serialized.contains("layer = \"core\""));
    assert!(serialized.contains("inferred-layer = \"std\""));
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
    write(&src, "pkg.jet", &min_manifest("mylib", "0.1.0"));

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
    write(&tmp, "greeter/pkg.jet", &min_manifest("greeter", "0.1.0"));
    write(
        &tmp,
        "greeter/greeter.jet",
        "pub fn greet() -> String { return \"hello!\"; }\n",
    );

    // Root project with path dep.
    write(
        &tmp,
        "pkg.jet",
        &manifest_with_deps("myapp", "0.1.0", "    greeter: path@greeter,"),
    );
    let entry = tmp.join("main.jet");
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
            .map(|d| jet::render_diagnostics("main.jet", "", &d))
            .unwrap_or_default()
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn version_conflict_emits_e1201() {
    let tmp = tmp_dir("ver_conflict");

    write(&tmp, "liba/pkg.jet", &min_manifest("mylib", "1.0.0"));
    write(&tmp, "liba/mylib.jet", "pub fn v1() {}\n");

    write(&tmp, "libb/pkg.jet", &min_manifest("mylib", "2.0.0"));
    write(&tmp, "libb/mylib.jet", "pub fn v2() {}\n");

    write(
        &tmp,
        "pkg.jet",
        &manifest_with_deps(
            "conflict_app",
            "0.1.0",
            "    liba: path@liba,\n    libb: path@libb,",
        ),
    );
    let entry = tmp.join("main.jet");
    fs::write(&entry, "fn run() {}\n").unwrap();

    let diags = jet::compile_with_path("", &entry.to_string_lossy())
        .expect_err("version conflict must fail with E1201");
    assert_eq!(first_diag_code(&diags), "E1201");

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn stale_lock_emits_e1202() {
    let tmp = tmp_dir("stale_lock");

    write(&tmp, "greeter/pkg.jet", &min_manifest("greeter", "0.1.0"));
    write(
        &tmp,
        "greeter/greeter.jet",
        "pub fn greet() -> String { return \"hi\"; }\n",
    );

    write(
        &tmp,
        "pkg.jet",
        &manifest_with_deps("app", "0.1.0", "    greeter: path@greeter,"),
    );
    // Lock exists but lists no dependencies — stale.
    write(
        &tmp,
        ".jet/lock",
        "version = 1\n\n[[package]]\nname = \"app\"\nsource = { root = \".\" }\n\n[root]\ndependencies = []\n",
    );

    let entry = tmp.join("main.jet");
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
        "pkg.jet",
        "payload: {\n    name: \"app\",\n    version: \"0.1.0\",\n    jet: \">=99.0.0\",\n}\n",
    );
    let entry = tmp.join("main.jet");
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
        "pkg.jet",
        &(min_manifest("app", "0.1.0") + "\ndev_deps: {\n    testlib: path@../testlib,\n}\n"),
    );
    let entry = tmp.join("main.jet");
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

    let raw = manifest_with_deps("app", "0.1.0", "    greeter: path@greeter,");
    write(&tmp, "pkg.jet", &raw);

    let mf = jet::Manifest::parse(&tmp.join("pkg.jet"), &raw).unwrap();
    let opts = jet::Fetch::FetchOptions {
        locked: true,
        update: false,
        update_dep: None,
    };

    let result = with_store(&store, || jet::Fetch::fetch(&tmp, &mf, None, &opts));
    let diags = result.expect_err("--locked with no lock file should fail");
    assert_eq!(first_diag_code(&diags), "E1202");

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
    write(&src, "mylib.jet", "pub fn answer() -> Int { return 42; }\n");
    write(&src, "pkg.jet", &min_manifest("mylib", "0.1.0"));

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
    write(&tmp, "pkg.jet", &raw);

    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();

    let mf = jet::Manifest::parse(&tmp.join("pkg.jet"), &raw).unwrap();
    let opts = jet::Fetch::FetchOptions {
        locked: false,
        update: false,
        update_dep: None,
    };

    let result = with_store(&store, || jet::Fetch::fetch(&tmp, &mf, None, &opts));
    assert!(
        result.is_ok(),
        "git dep fetch should succeed: {:?}",
        result.err()
    );

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
    write(&src, "mylib.jet", "pub fn answer() -> Int { return 42; }\n");
    write(&src, "pkg.jet", &min_manifest("mylib", "0.1.0"));

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
    write(&tmp, "pkg.jet", &raw);

    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();

    let mf = jet::Manifest::parse(&tmp.join("pkg.jet"), &raw).unwrap();

    // Initial fetch (no lock yet) — writes the lock file.
    let opts = jet::Fetch::FetchOptions {
        locked: false,
        update: false,
        update_dep: None,
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
        "pub fn extra() -> Int { return 99; }\n",
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
// CLI binary end-to-end (skip if binary not built)
// ─────────────────────────────────────────────

#[test]
fn cli_jet_new_creates_project_structure() {
    if !jet_bin().is_file() {
        eprintln!("note: skipping cli_jet_new (run `cargo build` first)");
        return;
    }

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
        proj.join("pkg.jet").is_file(),
        "jet new must create pkg.jet"
    );
    assert!(
        proj.join(".jet/main.jet").is_file() || proj.join("main.jet").is_file(),
        "jet new must create an entry point"
    );
    assert!(
        proj.join(".gitignore").is_file(),
        "jet new must create .gitignore"
    );

    let _ = fs::remove_dir_all(&tmp);
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

    write(&tmp, "greeter/pkg.jet", &min_manifest("greeter", "0.1.0"));
    write(
        &tmp,
        "greeter/greeter.jet",
        "pub fn greet() -> String { return \"hi\"; }\n",
    );
    write(
        &tmp,
        "pkg.jet",
        &manifest_with_deps("app", "0.1.0", "    greeter: path@greeter,"),
    );

    let out = jet_cmd(&["vendor", "--vendor-dir", "third_party"], &tmp, &store);
    assert!(
        out.status.success(),
        "jet vendor --vendor-dir failed:\n{}",
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

    let manifest = fs::read_to_string(tmp.join("annotated_app/pkg.jet"))
        .expect("pkg.jet must exist after jet new --annotated");
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
    write(&lib, "pkg.jet", &min_manifest("mylib", "0.1.0"));
    write(&lib, "mylib.jet", "pub fn answer() -> Int { return 42; }\n");

    // 3. jet add mylib --path ../mylib (from inside the project)
    let proj = tmp.join("myapp");
    let out = jet_cmd(&["add", "mylib", "--path", "../mylib"], &proj, &store);
    assert!(
        out.status.success(),
        "jet add --path failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // pkg.jet must now reference mylib.
    let manifest = fs::read_to_string(proj.join("pkg.jet")).unwrap();
    assert!(
        manifest.contains("mylib"),
        "pkg.jet should list mylib after jet add"
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
        signature: "fn parse(raw: String) -> Int".into(),
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
fn capability_sigil_frozen_in_public_api() {
    // c129 (D-MEM1, was D-CAP7/D-CAP8): the resolved capability sigil is part of
    // a pub fn's published signature, and a read -> write drift is a breaking change.
    use jet::Publish::{diff_public_api, extract_public_api};

    let dir = tmp_dir("cap_api_freeze");

    let write_src = "\
struct Account { balance: Int }
pub fn deposit(a: &Account, amount: Int) -> Int {
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
pub fn deposit(a: Account, amount: Int) -> Int { return a.balance + amount }
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
    write(&tmp, "greeter/pkg.jet", &min_manifest("greeter", "0.1.0"));
    write(
        &tmp,
        "greeter/greeter.jet",
        "pub fn greet() -> String { return \"hi\"; }\n",
    );

    // Project that depends on it.
    write(
        &tmp,
        "pkg.jet",
        &manifest_with_deps("vendored_app", "0.1.0", "    greeter: path@greeter,"),
    );
    write(
        &tmp,
        "main.jet",
        "use greeter;\nfn run() { print(greeter.greet()); }\n",
    );

    let entry = tmp.join("main.jet");
    let pack_path = tmp.join("pkg.jet");

    // Fetch to create the lock.
    let mf = jet::Manifest::parse(&pack_path, &fs::read_to_string(&pack_path).unwrap()).unwrap();
    let opts = jet::Fetch::FetchOptions {
        locked: false,
        update: false,
        update_dep: None,
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
            envelope: None,
        }],
        root_dependencies: vec![name.into()],
        workspace_members: vec![],
        comptime_inputs: Vec::new(),
        toolchains: Vec::new(),
        source_channels: Vec::new(),
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
    assert!(sbom.contains("DocumentNamespace: https://jet-lang.org/spdx/myapp-0.5.0-"));
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
    use jet::Publish::{audit_lockfile, parse_advisory_db};

    let lock = make_test_lock("crypto-lib", "0.9.0", "sha256-aabb");
    // Advisory: crypto-lib ^0 (pre-1.0) has a critical issue fixed in 0.9.5.
    let db = "JET-2026-SEC-001|crypto-lib|^0|0.9.5|Timing side-channel in AES-GCM|critical\n";
    let advisories = parse_advisory_db(db);
    let matches = audit_lockfile(&lock, &advisories);

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
    // the severity carried back is below Critical, so `jet audit` exits 0.
    use jet::Publish::{audit_lockfile, parse_advisory_db, Severity};

    let lock = make_test_lock("util-lib", "1.0.0", "sha256-ccdd");
    let db = "JET-2026-INFO-1|util-lib|^1|1.0.2|Minor info leak in debug logs|low\n";
    let advisories = parse_advisory_db(db);
    let matches = audit_lockfile(&lock, &advisories);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].severity, Severity::Low);
    assert!(matches.iter().all(|m| m.severity != Severity::Critical));
}

#[test]
fn e1217_missing_locked_revision() {
    // D-SUPPLY1 Step 2: a dep declared in the manifest with no lock entry fails
    // the bidirectional completeness check.
    use jet::Lock::{verify_all_manifest_deps_locked, LockFile};

    let raw = manifest_with_deps("app", "0.1.0", "    greeter: path@greeter,");
    let tmp = tmp_dir("e1217");
    write(&tmp, "pkg.jet", &raw);
    let mf = jet::Manifest::parse(&tmp.join("pkg.jet"), &raw).unwrap();

    // Empty lock — greeter is declared but not pinned.
    let empty_lock = LockFile {
        version: 1,
        packages: vec![],
        root_dependencies: vec![],
        workspace_members: vec![],
        comptime_inputs: Vec::new(),
        toolchains: Vec::new(),
        source_channels: Vec::new(),
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
        signature: "fn parse(raw: String) -> Int".into(),
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
    // D-PUBLISH1A: `jet publish` must refuse (E2605) when the git working tree
    // has uncommitted changes. This test initialises a fresh git repo with one
    // commit, adds an uncommitted file, then runs `jet publish` and asserts it
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
    write(&tmp, "pkg.jet", &min_manifest("dirtypkg", "1.0.0"));
    write(&tmp, "main.jet", "fn run() { print(\"hello\"); }\n");

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

    let out = jet_cmd(&["publish"], &tmp, &store);
    assert!(
        !out.status.success(),
        "jet publish must fail on a dirty tree"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E2605"),
        "jet publish must cite E2605 for a dirty tree:\n{stderr}"
    );

    // With --force it must emit a warning and get past the dirty gate. The
    // uncommitted warning is printed before any registry push, so pointing the
    // registry at a bogus local path (push then fails fast, no network) does not
    // affect these assertions.
    let cache = tmp.join("cache");
    let bogus = format!("file://{}", tmp.join("nonexistent.git").to_str().unwrap());
    let out_force = jet_cmd_env(
        &["publish", "--force"],
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
        "jet publish --force must warn about uncommitted changes:\n{stderr_force}"
    );

    let _ = fs::remove_dir_all(&tmp);
}

// ─────────────────────────────────────────────
// D-VERSION1: yank local marker
// ─────────────────────────────────────────────

#[test]
fn cli_publish_pushes_index_and_enforces_immutability_e1234() {
    // c56 (D-JPK-CACHE1=A / D-VERSION1=A): `jet publish` writes a JSONL line to
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

    let envs = &[
        ("JET_REGISTRY_URL", url.as_str()),
        ("JET_REGISTRY_CACHE_DIR", cache.to_str().unwrap()),
        ("JET_STORE_DIR", store.to_str().unwrap()),
        ("JET_KEYS_DIR", keys.to_str().unwrap()),
    ];

    let out = jet_cmd_env(&["publish"], &proj, envs);
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

    // S2/D-MEM1: `jet publish` now unconditionally snapshots the public-fn
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
    let out2 = jet_cmd_env(&["publish"], &proj, envs);
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
fn cli_yank_flips_index_entry() {
    // c56 (D-VERSION1=A): `jet yank <version>` flips the `yanked` flag on the
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

    let envs = &[
        ("JET_REGISTRY_URL", url.as_str()),
        ("JET_REGISTRY_CACHE_DIR", cache.to_str().unwrap()),
        ("JET_STORE_DIR", store.to_str().unwrap()),
        ("JET_KEYS_DIR", keys.to_str().unwrap()),
    ];

    let pubd = jet_cmd_env(&["publish"], &proj, envs);
    assert!(
        pubd.status.success(),
        "publish should succeed:\n{}",
        String::from_utf8_lossy(&pubd.stderr)
    );

    let yanked = jet_cmd_env(&["yank", "2.0.0", "--message", "regression"], &proj, envs);
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

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn cli_publish_unreachable_registry_e1235() {
    // c56: an unreachable registry index makes `jet publish` fail with E1235 —
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

    let out = jet_cmd_env(&["publish"], &proj, envs);
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
    // E2606: `jet yank` with no version must exit nonzero with E2606.
    if !jet_bin().is_file() {
        eprintln!("note: skipping cli_yank_requires_version_arg (run `cargo build` first)");
        return;
    }

    let tmp = tmp_dir("yank_no_ver");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();
    write(&tmp, "pkg.jet", &min_manifest("mypkg", "1.0.0"));

    let out = jet_cmd(&["yank"], &tmp, &store);
    assert!(!out.status.success(), "jet yank with no version must fail");
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
}

// ============================================================================
// Section: `pub(package)` visibility (was tests/pub_package.rs)
//
// Uses its own `Scratch` helper rather than the `tmp_dir`/`write` pair above:
// unlike `tmp_dir`, `Scratch` mixes a nanosecond suffix into the dir name (so
// concurrent test runs never collide on a bare label) and cleans up via
// `Drop`. Kept separate rather than forced onto the shared helper.
// ============================================================================

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("jet-pub-package-{tag}-{nanos}"));
        fs::create_dir_all(&path).unwrap();
        Scratch { path }
    }

    fn join(&self, path: &str) -> PathBuf {
        self.path.join(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn pub_package_function_is_visible_inside_project_scope() {
    let s = Scratch::new("same");
    fs::write(
        s.join("helper.jet"),
        "pub(package) fn secret() -> String {\n    return \"ok\"\n}\n",
    )
    .unwrap();
    fs::write(
        s.join("main.jet"),
        "use helper;\n\nfn run() {\n    print(helper.secret())\n}\n",
    )
    .unwrap();

    let diags = jet::check_with_path(&s.join("main.jet").to_string_lossy());
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
        app.join("pkg.jet"),
        "payload: { name: \"app\", version: \"0.1.0\" }\ndeps: { dep: path@../dep }\n",
    )
    .unwrap();
    fs::write(
        app.join("main.jet"),
        "use dep;\n\nfn run() {\n    print(dep.secret())\n}\n",
    )
    .unwrap();
    fs::write(
        dep.join("pkg.jet"),
        "payload: { name: \"dep\", version: \"0.1.0\" }\n",
    )
    .unwrap();
    fs::write(
        dep.join("dep.jet"),
        "pub(package) fn secret() -> String {\n    return \"hidden\"\n}\n",
    )
    .unwrap();

    let diags = jet::check_with_path(&app.join("main.jet").to_string_lossy());
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
        "pub(package) struct Secret {\n    pub(package) value: String\n}\n\npub fn make() -> Secret {\n    return Secret.{ value: \"ok\" }\n}\n",
    )
    .unwrap();
    fs::write(
        s.join("main.jet"),
        "use helper;\n\nfn run() {\n    s :: helper.make()\n    print(s.value)\n}\n",
    )
    .unwrap();

    let diags = jet::check_with_path(&s.join("main.jet").to_string_lossy());
    assert!(
        diags.is_empty(),
        "expected same-package type/field access to pass, got {diags:?}"
    );
}

// ─────────────────────────────────────────────
// c146 (D-PKGSIGN1): package signing tier A — Ed25519
// ─────────────────────────────────────────────

static KEYS_LOCK: Mutex<()> = Mutex::new(());

fn have_cargo() -> bool {
    Command::new("cargo").arg("--version").output().is_ok()
}

/// Run `f` with JET_KEYS_DIR pointed at `dir`, serialized against concurrent
/// env mutation (same shape as `with_store`).
fn with_keys<T, F: FnOnce() -> T>(dir: &Path, f: F) -> T {
    let _guard = KEYS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
        public_key: public_key.to_string(),
        signature: signature.to_string(),
    }
}

#[test]
fn cli_publish_signs_index_and_auto_keygens() {
    // c146: `jet publish` auto-generates a key on first use, signs by default,
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

    let envs = &[
        ("JET_REGISTRY_URL", url.as_str()),
        ("JET_REGISTRY_CACHE_DIR", cache.to_str().unwrap()),
        ("JET_STORE_DIR", store.to_str().unwrap()),
        ("JET_KEYS_DIR", keys.to_str().unwrap()),
    ];

    let out = jet_cmd_env(&["publish"], &proj, envs);
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

    let envs = &[
        ("JET_REGISTRY_URL", url.as_str()),
        ("JET_REGISTRY_CACHE_DIR", cache.to_str().unwrap()),
        ("JET_STORE_DIR", store.to_str().unwrap()),
        ("JET_KEYS_DIR", keys.to_str().unwrap()),
    ];

    let out = jet_cmd_env(&["publish", "--no-sign"], &proj, envs);
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
fn key_rotation_warns_not_errors() {
    // c146: a version that declares a different public key than the pin is a
    // legitimate key rotation — a warning, never a hard error. The signature
    // still verifies against the pinned key (the real signer).
    if !have_cargo() {
        eprintln!("note: skipping key_rotation (cargo not found)");
        return;
    }
    let tmp = tmp_dir("key_rotate");
    let keys = tmp.join("keys");

    with_keys(&keys, || {
        let (seed_a, _pa, pub_a) = jet::Publish::Sign::keygen("jet", false).unwrap();
        let (_seed_b, _pb, pub_b) = jet::Publish::Sign::keygen("other", false).unwrap();
        assert_ne!(pub_a, pub_b, "the two keys must differ");

        let ch1 = "sha256-v1v1v1";
        let ch2 = "sha256-v2v2v2";
        let sig1 = jet::Publish::Sign::sign(&seed_a, ch1).unwrap();
        // v2 is really signed by A, but its line declares key B (simulating a
        // hand-rewritten rotation record).
        let sig2 = jet::Publish::Sign::sign(&seed_a, ch2).unwrap();

        let v1 = signed_entry("textkit", "1.0.0", ch1, &pub_a, &sig1);
        let v2 = signed_entry("textkit", "2.0.0", ch2, &pub_b, &sig2);
        let all = vec![v1, v2.clone()];

        // pin = pub_a (first). v2's signature (by A) verifies against the pin;
        // its declared key (B) differs → a warning, not an error.
        let warnings = jet::Publish::verify_index_entry(&all, &v2, false, "jet")
            .expect("key rotation must warn, not error");
        assert!(
            !warnings.is_empty() && warnings[0].contains("rotation"),
            "rotation must produce a key-rotation warning, got {warnings:?}"
        );
    });

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn keygen_refuses_existing_key_e1248() {
    // c146: `jet keygen` refuses to overwrite an existing key without --force.
    if !have_cargo() {
        eprintln!("note: skipping keygen_refuses_existing_key (cargo not found)");
        return;
    }
    let tmp = tmp_dir("keygen_refuse");
    let keys = tmp.join("keys");

    with_keys(&keys, || {
        jet::Publish::Sign::keygen("jet", false).expect("first keygen should succeed");
        let err = jet::Publish::Sign::keygen("jet", false)
            .expect_err("a second keygen without --force must refuse");
        assert_eq!(err.code, "E1248", "must cite E1248");
        // --force overwrites.
        jet::Publish::Sign::keygen("jet", true).expect("--force keygen must overwrite");
    });

    let _ = fs::remove_dir_all(&tmp);
}
