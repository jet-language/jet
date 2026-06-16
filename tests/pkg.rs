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

fn first_diag_code(diags: &[jet::diag::Diagnostic]) -> &str {
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

// Minimal `pack.jet` (Jet syntax, U1) for a named package with no deps.
fn min_manifest(name: &str, version: &str) -> String {
    format!(
        "package: {{\n    name: \"{}\",\n    version: \"{}\",\n    jet: \">=0.1.0\",\n    description: \"\",\n    license: \"MIT\",\n    repository: \"\",\n}}\n",
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
    let raw = r#"package: {
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
    let path = PathBuf::from("pack.jet");
    let mf = jet::manifest::parse(&path, raw).expect("valid manifest should parse");
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
        jet::manifest::parse(&PathBuf::from("pack.jet"), &raw).expect("path dep should parse");
    let dep = mf.dependencies.get("helpers").expect("missing helpers dep");
    assert!(matches!(dep, jet::manifest::DepSpec::Path { path } if path == "../helpers"));
}

#[test]
fn manifest_parse_dep_git_tag() {
    let raw = manifest_with_deps(
        "root",
        "0.1.0",
        "    parsekit: { git: \"https://github.com/acme/parsekit\", tag: \"v0.4.1\" },",
    );
    let mf =
        jet::manifest::parse(&PathBuf::from("pack.jet"), &raw).expect("git tag dep should parse");
    let dep = mf.dependencies.get("parsekit").expect("missing parsekit");
    assert!(matches!(
        dep,
        jet::manifest::DepSpec::Git {
            url,
            selector: jet::manifest::GitSelector::Tag(t)
        } if url.contains("parsekit") && t == "v0.4.1"
    ));
}

#[test]
fn manifest_parse_e1206_missing_required_field() {
    // `package` with no `version` is a shape error (E1206).
    let raw = "package: {\n    name: \"myapp\",\n}\n";
    let err = jet::manifest::parse(&PathBuf::from("pack.jet"), raw)
        .expect_err("missing version should fail");
    assert_eq!(err.code, "E1206");
}

#[test]
fn manifest_parse_e1209_reserved_nonempty() {
    let raw = min_manifest("myapp", "0.1.0")
        + "\ndev_deps: {\n    testlib: path@../testlib,\n}\n";
    let err = jet::manifest::parse(&PathBuf::from("pack.jet"), &raw)
        .expect_err("non-empty dev_deps should fail E1209");
    assert_eq!(err.code, "E1209");
}

#[test]
fn manifest_toolchain_ok() {
    let raw = min_manifest("myapp", "0.1.0");
    let mf = jet::manifest::parse(&PathBuf::from("pack.jet"), &raw).unwrap();
    assert!(jet::manifest::check_toolchain(&mf, "pack.jet").is_ok());
}

#[test]
fn manifest_toolchain_e1208_future_version() {
    let raw = "package: {\n    name: \"myapp\",\n    version: \"0.1.0\",\n    jet: \">=99.0.0\",\n}\n";
    let mf = jet::manifest::parse(&PathBuf::from("pack.jet"), raw).unwrap();
    let err = jet::manifest::check_toolchain(&mf, "pack.jet").expect_err("E1208");
    assert_eq!(err.code, "E1208");
}

// ─────────────────────────────────────────────
// Template generation (jet new)
// ─────────────────────────────────────────────

#[test]
fn manifest_template_plain_parses() {
    let raw = jet::manifest::new_template("myapp", false);
    let mf = jet::manifest::parse(&PathBuf::from("pack.jet"), &raw)
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
    let raw = jet::manifest::new_template("myapp", true);
    assert!(
        raw.contains("// Jet package dependencies:"),
        "annotated template should have dep comment block: {}",
        raw
    );
    // Must still parse cleanly.
    jet::manifest::parse(&PathBuf::from("pack.jet"), &raw)
        .expect("annotated template should parse");
}

// ─────────────────────────────────────────────
// Comment-preserving edit helpers
// ─────────────────────────────────────────────

#[test]
fn manifest_add_dep_inserts_in_existing_table() {
    let raw = min_manifest("root", "0.1.0") + "\ndeps: {\n}\n";
    let updated = jet::manifest::add_dependency(
        &raw,
        "helpers",
        &jet::manifest::DepSpec::Path {
            path: "../helpers".to_string(),
        },
    );
    let mf = jet::manifest::parse(&PathBuf::from("pack.jet"), &updated).expect("should reparse");
    assert!(matches!(
        mf.dependencies.get("helpers"),
        Some(jet::manifest::DepSpec::Path { path }) if path == "../helpers"
    ));
}

#[test]
fn manifest_add_dep_creates_table_when_absent() {
    let raw = min_manifest("root", "0.1.0");
    let updated = jet::manifest::add_dependency(
        &raw,
        "helpers",
        &jet::manifest::DepSpec::Path {
            path: "../helpers".to_string(),
        },
    );
    assert!(updated.contains("deps:"), "should create deps: block");
    let mf = jet::manifest::parse(&PathBuf::from("pack.jet"), &updated).expect("should reparse");
    assert!(matches!(
        mf.dependencies.get("helpers"),
        Some(jet::manifest::DepSpec::Path { path }) if path == "../helpers"
    ));
}

#[test]
fn manifest_remove_dep_removes_correct_entry() {
    let raw = min_manifest("root", "0.1.0")
        + "\ndeps: {\n    helpers: path@../helpers,\n    other: path@../other,\n}\n";
    let updated = jet::manifest::remove_dependency(&raw, "helpers");
    let mf = jet::manifest::parse(&PathBuf::from("pack.jet"), &updated).expect("should reparse");
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
    let hash = jet::sha256::sha256_hex(b"");
    assert_eq!(
        hash,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn sha256_known_vector_matches_self() {
    // The internal sha256.rs unit test asserts a specific hash for "abc".
    // Cross-check here to catch any accidental changes to the implementation.
    let hash = jet::sha256::sha256_hex(b"abc");
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

    let h1 = jet::sha256::tree_hash(&tmp);
    let h2 = jet::sha256::tree_hash(&tmp);
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
    let h1 = jet::sha256::tree_hash(&tmp);

    write(&tmp, "a.jet", "fn foo() { print(\"hello\"); }");
    let h2 = jet::sha256::tree_hash(&tmp);
    assert_ne!(h1, h2, "tree hash must change when file content changes");

    let _ = fs::remove_dir_all(&tmp);
}

// ─────────────────────────────────────────────
// Lock fingerprinting
// ─────────────────────────────────────────────

#[test]
fn fingerprint_is_deterministic() {
    let fp1 = jet::lock::compute_fingerprint("sha256-aabbcc", &["sha256-ddeeff"]);
    let fp2 = jet::lock::compute_fingerprint("sha256-aabbcc", &["sha256-ddeeff"]);
    assert_eq!(fp1, fp2);
    assert!(fp1.starts_with("sha256-"));
}

#[test]
fn fingerprint_changes_with_deps() {
    let fp1 = jet::lock::compute_fingerprint("sha256-aabbcc", &[]);
    let fp2 = jet::lock::compute_fingerprint("sha256-aabbcc", &["sha256-ddeeff"]);
    assert_ne!(
        fp1, fp2,
        "fingerprint must change when dep fingerprints differ"
    );
}

// ─────────────────────────────────────────────
// Store path format
// ─────────────────────────────────────────────

#[test]
fn store_path_format_name_version_fp() {
    let p = jet::store::store_path("mylib", "1.0.0", "sha256-deadbeef");
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
        let with_pfx = jet::store::store_path("mylib", "1.0.0", "sha256-deadbeef");
        let without = jet::store::store_path("mylib", "1.0.0", "deadbeef");
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
    write(&src, "pack.jet", &min_manifest("mylib", "0.1.0"));

    let fp = "sha256-0000000000000000000000000000000000000000000000000000000000000000";

    let entry = with_store(&store, || {
        jet::store::ensure_path_dep("mylib", "0.1.0", fp, &src)
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
    write(&src, "pack.jet", &min_manifest("mylib", "0.1.0"));

    let fp = "sha256-1111111111111111111111111111111111111111111111111111111111111111";

    let (p1, p2) = with_store(&store, || {
        let a = jet::store::ensure_path_dep("mylib", "0.1.0", fp, &src).unwrap();
        let b = jet::store::ensure_path_dep("mylib", "0.1.0", fp, &src).unwrap();
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
    write(&src, "pack.jet", &min_manifest("mylib", "0.1.0"));

    let fp = "sha256-2222222222222222222222222222222222222222222222222222222222222222";

    let (entry, genuine_hash) = with_store(&store, || {
        let e = jet::store::ensure_path_dep("mylib", "0.1.0", fp, &src).unwrap();
        let h = jet::sha256::tree_hash(&e);
        (e, h)
    });

    // Tamper: modify the stored file outside the locked section.
    fs::write(
        entry.join("mylib.jet"),
        "pub fn evil() { /* tampered */ }\n",
    )
    .unwrap();

    let result = jet::store::verify_entry("mylib", &entry, &genuine_hash);
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
    write(&src, "pack.jet", &min_manifest("mylib", "0.1.0"));

    let fp = "sha256-3333333333333333333333333333333333333333333333333333333333333333";

    // link_root must NOT pre-exist — link_into_project checks that.
    let link1 = tmp.join("proj1/deps/mylib");
    let link2 = tmp.join("proj2/deps/mylib");

    let store_entry = with_store(&store, || {
        let e = jet::store::ensure_path_dep("mylib", "0.1.0", fp, &src).unwrap();
        jet::store::link_into_project(&e, &link1).unwrap();
        jet::store::link_into_project(&e, &link2).unwrap();
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
    write(&tmp, "greeter/pack.jet", &min_manifest("greeter", "0.1.0"));
    write(
        &tmp,
        "greeter/greeter.jet",
        "pub fn greet() -> String { return \"hello!\"; }\n",
    );

    // Root project with path dep.
    write(
        &tmp,
        "pack.jet",
        &manifest_with_deps("myapp", "0.1.0", "    greeter: path@greeter,"),
    );
    let entry = tmp.join("main.jet");
    fs::write(
        &entry,
        "import greeter;\nfn main() { print(greeter.greet()); }\n",
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

    write(&tmp, "liba/pack.jet", &min_manifest("mylib", "1.0.0"));
    write(&tmp, "liba/mylib.jet", "pub fn v1() {}\n");

    write(&tmp, "libb/pack.jet", &min_manifest("mylib", "2.0.0"));
    write(&tmp, "libb/mylib.jet", "pub fn v2() {}\n");

    write(
        &tmp,
        "pack.jet",
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

    write(&tmp, "greeter/pack.jet", &min_manifest("greeter", "0.1.0"));
    write(
        &tmp,
        "greeter/greeter.jet",
        "pub fn greet() -> String { return \"hi\"; }\n",
    );

    write(
        &tmp,
        "pack.jet",
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
        "pack.jet",
        "package: {\n    name: \"app\",\n    version: \"0.1.0\",\n    jet: \">=99.0.0\",\n}\n",
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
        "pack.jet",
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
    write(&tmp, "pack.jet", &raw);

    let mf = jet::manifest::parse(&tmp.join("pack.jet"), &raw).unwrap();
    let opts = jet::fetch::FetchOptions {
        locked: true,
        update: false,
        update_dep: None,
    };

    let result = with_store(&store, || jet::fetch::fetch(&tmp, &mf, None, &opts));
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
    write(&src, "pack.jet", &min_manifest("mylib", "0.1.0"));

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
    write(&tmp, "pack.jet", &raw);

    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();

    let mf = jet::manifest::parse(&tmp.join("pack.jet"), &raw).unwrap();
    let opts = jet::fetch::FetchOptions {
        locked: false,
        update: false,
        update_dep: None,
    };

    let result = with_store(&store, || jet::fetch::fetch(&tmp, &mf, None, &opts));
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
    write(&src, "pack.jet", &min_manifest("mylib", "0.1.0"));

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
    write(&tmp, "pack.jet", &raw);

    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();

    let mf = jet::manifest::parse(&tmp.join("pack.jet"), &raw).unwrap();

    // Initial fetch (no lock yet) — writes the lock file.
    let opts = jet::fetch::FetchOptions {
        locked: false,
        update: false,
        update_dep: None,
    };
    let result = with_store(&store, || jet::fetch::fetch(&tmp, &mf, None, &opts));
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
    let existing_lock = jet::lock::parse(&lock_str).expect("initial lock must parse");
    let update_opts = jet::fetch::FetchOptions {
        locked: false,
        update: true,
        update_dep: None,
    };
    let result2 = with_store(&store, || {
        jet::fetch::fetch(&tmp, &mf, Some(&existing_lock), &update_opts)
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
        proj.join("pack.jet").is_file(),
        "jet new must create pack.jet"
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
fn cli_jet_new_annotated_has_dep_comments() {
    if !jet_bin().is_file() {
        eprintln!("note: skipping cli_jet_new_annotated (run `cargo build` first)");
        return;
    }

    let tmp = tmp_dir("cli_annotated");
    let store = tmp.join("store");
    fs::create_dir_all(&store).unwrap();

    jet_cmd(&["new", "annotated_app", "--annotated"], &tmp, &store);

    let manifest = fs::read_to_string(tmp.join("annotated_app/pack.jet"))
        .expect("pack.jet must exist after jet new --annotated");
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
    write(&lib, "pack.jet", &min_manifest("mylib", "0.1.0"));
    write(&lib, "mylib.jet", "pub fn answer() -> Int { return 42; }\n");

    // 3. jet add mylib --path ../mylib (from inside the project)
    let proj = tmp.join("myapp");
    let out = jet_cmd(&["add", "mylib", "--path", "../mylib"], &proj, &store);
    assert!(
        out.status.success(),
        "jet add --path failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // pack.jet must now reference mylib.
    let manifest = fs::read_to_string(proj.join("pack.jet")).unwrap();
    assert!(
        manifest.contains("mylib"),
        "pack.jet should list mylib after jet add"
    );

    let _ = fs::remove_dir_all(&tmp);
}
