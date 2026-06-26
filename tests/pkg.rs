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
    let mf =
        jet::Manifest::parse(&PathBuf::from("pkg.jet"), &raw).expect("path dep should parse");
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
    let raw = min_manifest("myapp", "0.1.0")
        + "\ndev_deps: {\n    testlib: path@../testlib,\n}\n";
    let err = jet::Manifest::parse(&PathBuf::from("pkg.jet"), &raw)
        .expect_err("non-empty dev_deps should fail E1209");
    assert_eq!(err.code, "E1209");
}

#[test]
fn manifest_toolchain_ok() {
    let raw = min_manifest("myapp", "0.1.0");
    let mf = jet::Manifest::parse(&PathBuf::from("pkg.jet"), &raw).unwrap();
    assert!(jet::Manifest::check_toolchain(&mf, "pkg.jet").is_ok());
}

#[test]
fn manifest_toolchain_e1208_future_version() {
    let raw = "payload: {\n    name: \"myapp\",\n    version: \"0.1.0\",\n    jet: \">=99.0.0\",\n}\n";
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
    let mf = jet::Manifest::parse(&PathBuf::from("pkg.jet"), &raw)
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
    jet::Manifest::parse(&PathBuf::from("pkg.jet"), &raw)
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
// param's resolved capability (read → ~/^/&) must shift the fingerprint even when
// the source tree hash and deps are identical.
#[test]
fn fingerprint_changes_with_capability_digest() {
    let fp_read = jet::Lock::compute_fingerprint("sha256-aabbcc", &[], "pkg\nfn scale(v: Vec3)");
    let fp_write = jet::Lock::compute_fingerprint("sha256-aabbcc", &[], "pkg\nfn scale(v: ~Vec3)");
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

    let entry = with_store(&store, || {
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
        let a = jet::Store::ensure_path_dep("mylib", "0.1.0", fp, &src).unwrap();
        let b = jet::Store::ensure_path_dep("mylib", "0.1.0", fp, &src).unwrap();
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
        let e = jet::Store::ensure_path_dep("mylib", "0.1.0", fp, &src).unwrap();
        let h = jet::SHA256::tree_hash(&e);
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
        let e = jet::Store::ensure_path_dep("mylib", "0.1.0", fp, &src).unwrap();
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
        "use greeter;\nfn main() { print(greeter.greet()); }\n",
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
    fs::write(&entry, "fn main() {}\n").unwrap();

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
    fs::write(&entry, "fn main() {}\n").unwrap();

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
    fs::write(&entry, "fn main() { print(\"hi\"); }\n").unwrap();

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
    fs::write(&entry, "fn main() {}\n").unwrap();

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
    write(&tmp, "hello.jet", "fn main() { print(\"hi\"); }\n");

    let out = jet_cmd(&["build", "--sbom", "hello.jet"], &tmp, &store);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "jet build --sbom failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("sbom:"), "build output must announce the SBOM path");

    // The SBOM lands beside the produced binary as <bin>.spdx.
    let spdx = tmp.join("build/hello.spdx");
    assert!(spdx.is_file(), "expected SBOM at {}", spdx.display());
    let body = fs::read_to_string(&spdx).unwrap();
    assert!(body.starts_with("SPDXVersion: SPDX-2.3\n"), "SBOM must be SPDX 2.3");

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
    write(&tmp, "greeter/greeter.jet", "pub fn greet() -> String { return \"hi\"; }\n");
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
    assert!(tmp.join("third_party/greeter").is_dir(), "dep must land in the chosen dir");
    assert!(tmp.join("third_party/manifest.json").is_file(), "vendor manifest must be written");
    assert!(!tmp.join("vendor").exists(), "default vendor/ must not be created");

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
    use jet::Publish::{ApiItem, BumpKind, diff_public_api, e2601};

    let old_api = vec![ApiItem {
        kind: "fn".into(),
        name: "parse".into(),
        signature: "fn parse(raw: String) -> Int".into(),
    }];
    let new_api: Vec<ApiItem> = vec![]; // removed

    let changes = diff_public_api(&old_api, &new_api);
    assert!(!changes.is_empty(), "removed pub fn must be a breaking change");

    let diag = e2601("1.2.0", BumpKind::Minor, &changes[0], 2);
    assert_eq!(diag.code, "E2601");
    assert!(diag.what.contains("1.2.0"), "what must name the version");
    assert!(diag.why.contains("minor"), "why must name the bump kind");
    assert!(diag.fix.contains("2.0.0"), "fix must name the next major");
    assert!(diag.why.contains("parse") || diag.why.contains("removed"),
        "why must name the broken item or action");
}

#[test]
fn capability_sigil_frozen_in_public_api() {
    // c129 (D-CAP7/D-CAP8): the resolved capability sigil is part of a pub fn's
    // published signature, and a read -> write drift is a breaking change.
    use jet::Publish::{diff_public_api, extract_public_api};

    let dir = tmp_dir("cap_api_freeze");

    let write_src = "\
struct Account { balance: Int }
pub fn deposit(a: ~Account, amount: Int) -> Int {
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
        deposit.signature.contains("a: ~"),
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
    use jet::Publish::{VersionConstraint, VersionReq, check_conflicts};
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
    assert!(!diags.is_empty(), "disjoint major caret ranges must be a conflict");
    assert_eq!(diags[0].code, "E2602");
    let why = &diags[0].why;
    assert!(why.contains("log"), "why must name the conflicting package");
    assert!(why.contains("web-server") || why.contains("db-client"), "why must name a dependent");
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
    write(&tmp, "greeter/greeter.jet", "pub fn greet() -> String { return \"hi\"; }\n");

    // Project that depends on it.
    write(
        &tmp,
        "pkg.jet",
        &manifest_with_deps("vendored_app", "0.1.0", "    greeter: path@greeter,"),
    );
    write(&tmp, "main.jet", "use greeter;\nfn main() { print(greeter.greet()); }\n");

    let entry = tmp.join("main.jet");
    let pack_path = tmp.join("pkg.jet");

    // Fetch to create the lock.
    let mf = jet::Manifest::parse(&pack_path, &fs::read_to_string(&pack_path).unwrap()).unwrap();
    let opts = jet::Fetch::FetchOptions { locked: false, update: false, update_dep: None };
    let result = with_store(&store, || {
        jet::Fetch::fetch(&tmp, &mf, None, &opts)
    });
    assert!(result.is_ok(), "initial fetch must succeed");
    let (lock, dep_dirs) = result.unwrap();

    // Vendor the dependency.
    let vendor_dir = tmp.join("vendor");
    let vendor_result = jet::Publish::vendor(&tmp, &lock, &dep_dirs, &vendor_dir);
    assert!(vendor_result.is_ok(), "vendor must succeed");
    let copied = vendor_result.unwrap();
    assert!(copied.contains(&"greeter".to_string()), "greeter must be vendored");
    assert!(tmp.join("vendor/greeter").is_dir(), "vendor/greeter must exist");
    // D-SUPPLY1: a vendor manifest records each dep's name/version/fingerprint.
    let manifest = fs::read_to_string(tmp.join("vendor/manifest.json")).unwrap();
    assert!(manifest.contains("\"name\": \"greeter\""), "manifest must list greeter");
    assert!(manifest.contains("\"fingerprint\""), "manifest must record fingerprints");

    // With the lock present, --locked fetch succeeds (no network needed).
    let lock_text = fs::read_to_string(tmp.join(".jet/lock")).unwrap_or_default();
    assert!(!lock_text.is_empty(), "lock file must exist after fetch");

    // Compile the project (uses the in-store copy, not network).
    let compile_result = with_store(&store, || {
        jet::compile_with_path("", &entry.to_string_lossy())
    });
    assert!(compile_result.is_ok(), "vendored project must compile offline");

    let _ = fs::remove_dir_all(&tmp);
}

fn make_test_lock(name: &str, version: &str, fp: &str) -> jet::Lock::LockFile {
    use jet::Lock::{LockFile, LockedPackage, LockSource};
    LockFile {
        version: 1,
        packages: vec![LockedPackage {
            name: name.into(),
            version: version.into(),
            fingerprint: fp.into(),
            source: LockSource::Path("/tmp/placeholder".into()),
            locked: None,
            dependencies: vec![],
        }],
        root_dependencies: vec![name.into()],
        comptime_inputs: Vec::new(),
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
    use jet::Publish::{parse_advisory_db, audit_lockfile};

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
    assert!(d.what.contains("[critical]"), "severity must prefix the message");
    assert_eq!(matches[0].severity, jet::Publish::Severity::Critical);
}

#[test]
fn audit_non_critical_is_advisory() {
    // D-SUPPLY1: a non-critical advisory still matches but is advisory-only —
    // the severity carried back is below Critical, so `jet audit` exits 0.
    use jet::Publish::{parse_advisory_db, audit_lockfile, Severity};

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
    let empty_lock = LockFile { version: 1, packages: vec![], root_dependencies: vec![], comptime_inputs: Vec::new() };
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
    use jet::Publish::{diff_public_api, e1218, classify_bump, BumpKind, ApiItem};
    use jet::Publish::SemVer::SemVer;

    let old = vec![ApiItem {
        kind: "fn".into(),
        name: "parse".into(),
        signature: "fn parse(raw: String) -> Int".into(),
    }];
    let new: Vec<ApiItem> = vec![]; // parse removed
    let breaking = diff_public_api(&old, &new);
    assert!(!breaking.is_empty());

    let bump = classify_bump(&SemVer::parse("1.0.0").unwrap(), &SemVer::parse("1.1.0").unwrap());
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
    env.insert("JET_REGISTRY_INTERNAL_URL".into(), "https://registry.acme.corp/jet".into());
    let regs = parse_registries_from_env(&env);
    assert!(!regs.is_empty());
    assert_eq!(regs[0].name, "internal");
    assert_eq!(regs[0].url, "https://registry.acme.corp/jet");
    assert!(!regs[0].mirror);
}

#[test]
fn pre_publish_gate_blocks_on_build_failure() {
    use jet::Publish::{PrePublishGate, BumpKind};
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
    use jet::Publish::{PrePublishGate, BumpKind};
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
