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
use std::path::Path;
use std::process::Command;

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
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "jet_archive_{}_{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
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
    let user_rust = common::strip_scheduler_native(&common::strip_vetted_module(&out.rust, "jet_atomic_windows"));
    assert!(
        !user_rust.contains("unsafe"),
        "I1: Core bridge output must not contain unsafe"
    );

    let rs = dir.join("archive_test.rs");
    let bin = dir.join("archive_test_bin");
    fs::write(&rs, &out.rust).unwrap();
    let link = out.ffi.as_ref().unwrap();
    let status = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&bin)
        .arg("--extern")
        .arg(format!("{}={}", link.crate_name, link.rlib_path.display()))
        .arg("-L")
        .arg(format!("dependency={}", link.deps_dir.display()))
        .status()
        .unwrap();
    assert!(status.success(), "I2: rustc rejected archive-linked output");

    let run = Command::new(&bin).output().unwrap();
    assert!(
        run.status.success(),
        "archive program failed at runtime:\nstderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
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

    let base = std::env::temp_dir().join(format!(
        "jet-bfs1-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let store = base.join("store");
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

    let _ = fs::remove_dir_all(&base);
}
