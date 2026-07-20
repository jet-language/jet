//! D-CORE-COMPRESS1=A — stream codecs and archive containers have one home each.
//!
//! `core.compress` delivers gzip/zstd streams; `core.archive` delivers zip/tar
//! containers through the hidden FFI bridge. These tests are gated on
//! cargo/rustc availability like the FFI golden tests.
//!
//! D-BFS1: a separate test exercises the build-from-source path — realizing
//! the ring package source (`corelib/core.archive/`) through CoreProvider and
//! verifying the rlib artifact lands in the hangar.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

struct TempTree(PathBuf);

impl TempTree {
    fn new(prefix: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "{prefix}_{}_{}",
            std::process::id(),
            TEMP_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        make_tree_owner_writable(&self.0);
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn make_tree_owner_writable(path: &Path) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata.file_type().is_symlink() {
        return;
    }
    let is_dir = metadata.is_dir();
    let mut permissions = metadata.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        permissions.set_mode(permissions.mode() | if is_dir { 0o700 } else { 0o600 });
    }
    #[cfg(not(unix))]
    permissions.set_readonly(false);
    let _ = fs::set_permissions(path, permissions);
    if is_dir {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                make_tree_owner_writable(&entry.path());
            }
        }
    }
}

#[test]
fn archive_bridge_embeds_the_canonical_ring_source() {
    let ffi = include_str!("../crates/jet-pkg-model/src/FFI.rs");
    let canonical = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("corelib/core.archive/pkgs/archive/src/lib.rs");
    let retired_copy = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("crates/jet-pkg-model/src/Prelude/Archive.rs");

    assert!(canonical.is_file(), "canonical archive source is missing");
    assert!(
        ffi.contains("../../../corelib/core.archive/pkgs/archive/src/lib.rs"),
        "the bridge must include the canonical ring-package implementation"
    );
    assert!(
        !retired_copy.exists(),
        "a second archive runtime source would allow the two build paths to drift"
    );
}

mod common;
use common::have_rustc;

fn have_toolchain() -> bool {
    have_rustc() && Command::new("cargo").arg("--version").output().is_ok()
}

#[test]
fn legacy_archive_gzip_is_rejected() {
    let src = r#"
use core.archive as ar

fn run() {
    bytes: [U8] :: [1, 2, 3]
    ar.gzip_compress(bytes)
}
"#;
    let diags = jet::compile(src)
        .expect_err("D-CORE-COMPRESS1=A removes gzip from core.archive");
    assert!(
        diags.iter().any(|d| d.code == "E1004"),
        "legacy archive gzip should be an ordinary unknown Core item: {diags:?}"
    );
}

/// Compile, FFI-link, and run a Core bridge program; return stdout.
fn run_core_bridge(src: &str) -> String {
    let temp = TempTree::new("jet_archive");
    let dir = &temp.0;
    let path = dir.join("archive_test.jet");
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy();

    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected Core bridge fixture:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    assert!(out.ffi.is_some(), "Core codec/container call must produce an FFI bridge");
    let user_rust = common::strip_vetted_prelude_modules(&out.rust);
    assert!(
        !user_rust.contains("unsafe"),
        "I1: Core bridge output must not contain unsafe"
    );

    let rs = dir.join("archive_test.rs");
    let bin = dir.join("archive_test_bin");
    fs::write(&rs, &out.rust).unwrap();
    let link = out.ffi.as_ref().unwrap();
    let dependency_dirs: Vec<_> = link.dependency_dirs().collect();
    let built = rustc_bridge(&rs, &bin, link, &dependency_dirs);
    assert!(
        built.status.success(),
        "I2: rustc rejected archive-linked output:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );

    let run = Command::new(&bin).output().unwrap();
    assert!(
        run.status.success(),
        "archive program failed at runtime:\nstderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

fn rustc_bridge(
    rs: &Path,
    bin: &Path,
    link: &jet::FFI::FfiLink,
    dependency_dirs: &[&Path],
) -> Output {
    let mut rustc = Command::new("rustc");
    rustc
        .args(["--edition", "2021"])
        .arg(rs)
        .arg("-o")
        .arg(bin)
        .arg("--extern")
        .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
    for deps_dir in dependency_dirs {
        rustc.arg("-L").arg(format!("dependency={}", deps_dir.display()));
    }
    rustc.output().unwrap()
}

#[test]
fn gzip_round_trip_uses_core_compress() {
    if !have_toolchain() {
        eprintln!("note: cargo/rustc not found; skipping core.compress integration test");
        return;
    }
    let src = r#"
use core.compress.gzip as gz

fn run() {
original: [U8] :: [72, 101, 108, 108, 111]
    compressed :: gz.compress(original)
    print((compressed.len() > 5))
    restored :: gz.decompress(compressed) ?? panic("bad gzip")
    print((restored == original))
}
"#;
    let out = run_core_bridge(src);
    assert_eq!(out, "true\ntrue\n", "gzip round-trip failed: {out:?}");
}

#[test]
fn archive_zip_and_tar_round_trip_bytes() {
    if !have_toolchain() {
        eprintln!("note: cargo/rustc not found; skipping core.archive integration test");
        return;
    }
    let src = r#"
use core.archive as ar

fn run() {
data: [U8] :: [72, 101, 108, 108, 111]
    zipped :: ar.zip_compress("hello.txt", data)
    print((ar.zip_decompress(zipped) == data))
    empty: [U8] :: []
    tarred :: ar.tar_add(empty, "hello.txt", data)
    print((ar.tar_get(tarred, "hello.txt") == data))
    print((ar.tar_names_json(tarred) == "[\"hello.txt\"]"))
}
"#;
    let out = run_core_bridge(src);
    assert_eq!(
        out, "true\ntrue\ntrue\n",
        "zip/tar byte round-trip failed: {out:?}"
    );
}

#[test]
fn archive_direct_rustc_requires_target_and_host_dependency_dirs() {
    if !have_toolchain() {
        eprintln!("note: cargo/rustc not found; skipping archive link-contract test");
        return;
    }
    let temp = TempTree::new("jet_archive_link_contract");
    let jet_path = temp.0.join("archive_link_contract.jet");
    let src = r#"
use core.archive as ar

fn run() {
data: [U8] :: [1, 2, 3]
    zipped :: ar.zip_compress("data.bin", data)
    print((ar.zip_decompress(zipped) == data))
}
"#;
    fs::write(&jet_path, src).unwrap();
    let shown = jet_path.to_string_lossy();
    let out = jet::compile_with_path(src, &shown).unwrap();
    let link = out.ffi.as_ref().expect("core.archive must build an FFI bridge");

    let dirs: Vec<_> = link.dependency_dirs().collect();
    assert_eq!(dirs, [&*link.target_deps_dir, &*link.host_deps_dir]);
    assert_ne!(link.target_deps_dir, link.host_deps_dir);
    assert!(link.target_deps_dir.is_dir());
    assert!(link.host_deps_dir.is_dir());
    assert!(
        fs::read_dir(&link.host_deps_dir).unwrap().flatten().any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .ends_with(std::env::consts::DLL_SUFFIX)
        }),
        "Cargo host dependency directory must contain a proc-macro dynamic library"
    );

    let rs = temp.0.join("archive_link_contract.rs");
    fs::write(&rs, out.rust).unwrap();
    let missing = rustc_bridge(
        &rs,
        &temp.0.join("missing_host_bin"),
        link,
        &[&link.target_deps_dir],
    );
    assert_link_dir_failure("missing host dependency directory", &missing);

    let wrong = temp.0.join("wrong-host-deps");
    fs::create_dir(&wrong).unwrap();
    let wrong_dir = rustc_bridge(
        &rs,
        &temp.0.join("wrong_host_bin"),
        link,
        &[&link.target_deps_dir, &wrong],
    );
    assert_link_dir_failure("wrong host dependency directory", &wrong_dir);

    let complete = rustc_bridge(
        &rs,
        &temp.0.join("complete_bin"),
        link,
        &dirs,
    );
    assert!(
        complete.status.success(),
        "target + host dependency directories must link:\n{}",
        String::from_utf8_lossy(&complete.stderr)
    );
}

fn assert_link_dir_failure(case: &str, output: &Output) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{case} unexpectedly linked");
    assert!(
        stderr.contains("E0463") && stderr.contains("can't find crate"),
        "{case} must fail as an honest missing dependency, got:\n{stderr}"
    );
}

#[test]
fn archive_temp_cleanup_restores_permissions_on_success_and_unwind() {
    for unwind in [false, true] {
        let path = std::env::temp_dir().join(format!(
            "jet_archive_cleanup_{}_{}_{}",
            std::process::id(),
            TEMP_SEQ.fetch_add(1, Ordering::Relaxed),
            unwind
        ));
        let result = std::panic::catch_unwind(|| {
            fs::create_dir_all(path.join("objects/item/src")).unwrap();
            let _cleanup = TempTree(path.clone());
            let file = path.join("objects/item/src/lib.rs");
            fs::write(&file, "readonly").unwrap();
            let mut file_permissions = fs::metadata(&file).unwrap().permissions();
            file_permissions.set_readonly(true);
            fs::set_permissions(&file, file_permissions).unwrap();
            let src = path.join("objects/item/src");
            let mut dir_permissions = fs::metadata(&src).unwrap().permissions();
            dir_permissions.set_readonly(true);
            fs::set_permissions(&src, dir_permissions).unwrap();
            assert!(fs::metadata(&file).unwrap().permissions().readonly());
            assert!(fs::metadata(&src).unwrap().permissions().readonly());
            if unwind {
                panic!("exercise archive fixture unwind cleanup");
            }
        });
        assert_eq!(result.is_err(), unwind);
        assert!(!path.exists(), "archive fixture cleanup leaked {}", path.display());
    }
}

// ── D-BFS1: build-from-source via CoreProvider ────────────────────────────────

/// Verify that `CoreProvider::realize()` compiles a library package that ships
/// a `Cargo.toml` into an rlib artifact cached in the hangar. This is the
/// end-to-end proof of the D-BFS1 compile step.
#[test]
fn core_provider_compiles_ring_package_to_rlib() {
    if !have_toolchain() {
        eprintln!("note: cargo/rustc not found; skipping D-BFS1 build-from-source test");
        return;
    }

    use jetpack::Provider::Ctx;
    use jetpack::RefSpec::{classify_in, ProviderKind, SourceTable};
    use jetpack::Store::{self, Roots};

    // Locate the core.archive ring package from the repo root.
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let ring_repo = repo_root.join("corelib/core.archive");
    if !ring_repo.is_dir() {
        eprintln!(
            "note: corelib/core.archive not found at {}; skipping D-BFS1 test",
            ring_repo.display()
        );
        return;
    }

    let temp = TempTree::new("jet-bfs1");
    let base = temp.0.clone();
    let store = base.join("hangar");
    fs::create_dir_all(&store).unwrap();

    let roots = Roots {
        root: base.clone(),
        dev_mode: true,
    };

    let upstream = format!("path:{}", ring_repo.to_string_lossy());
    let table = SourceTable::from_decls([("ring".to_string(), upstream, ProviderKind::Core)]);
    let spec = classify_in("ring:archive", &table).unwrap();
    let ctx = Ctx {
        fixtures: None,
        store_dir: &store,
        offline: true,
        project_dir: None,
    };

    // Realize the ring package — CoreProvider should compile the Cargo.toml.
    let realized = Store::realize_verified(
        &roots,
        &ctx,
        Store::RealizeRequest::Package {
            spec: &spec,
            table: &table,
        },
    )
    .expect("verified realization should build core.archive from source");
    let r = realized.metadata();

    assert_eq!(r.name, "archive", "realized name should be archive");
    assert!(r.bin.is_empty(), "library package must have no bin");
    assert!(
        !r.rlib.is_empty(),
        "D-BFS1: library with Cargo.toml must produce an rlib (got empty)"
    );

    let rlib_path = Path::new(&r.rlib);
    assert!(
        rlib_path.is_file(),
        "D-BFS1: rlib file must exist at {}",
        r.rlib
    );
    assert!(
        rlib_path.extension().and_then(|e| e.to_str()) == Some("rlib"),
        "D-BFS1: produced artifact must be an rlib, got {}",
        r.rlib
    );

    let listed = Store::list(&roots);
    let found = listed
        .iter()
        .find(|e| e.name == "archive")
        .expect("archive must appear in hangar listing");
    assert_eq!(
        found.rlib, r.rlib,
        "rlib path must be durable in hangar meta.json"
    );

    // Source-built outputs may live in the configured store rather than the
    // Hangar object directory. Their empty dependency set is valid only while
    // the entire output tree still hashes to the committed closure digest.
    let output_root = Path::new(&r.out);
    let mut permissions = fs::metadata(output_root).unwrap().permissions();
    permissions.set_readonly(false);
    fs::set_permissions(output_root, permissions).unwrap();
    fs::write(output_root.join("closure-tamper"), b"changed after commit").unwrap();
    let error = Store::closure_graph(&roots)
        .expect_err("changed output must invalidate closure proof");
    assert!(
        error
            .to_string()
            .contains("has no dependency references or store-validated closure proof"),
        "changed output must fail its content-bound closure proof: {error}"
    );
}
