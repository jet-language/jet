//! D-CORE-COMPRESS1=A — stream codecs and archive containers have one home each.
//!
//! `core.archive.gzip` / `.zstd` deliver streams; `core.archive` delivers zip/tar
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
    let canonical =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("corelib/core.archive/pkgs/archive/src/lib.rs");
    let source_package =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("corelib/core.archive/pkgs/archive/archive.jet");
    let retired_copy =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/jet-pkg-model/src/Prelude/Archive.rs");

    assert!(canonical.is_file(), "canonical archive source is missing");
    let source = fs::read_to_string(&source_package).unwrap();
    for function in [
        "pub fn zip_compress",
        "pub fn zip_decompress",
        "pub fn tar_add",
        "pub fn tar_get",
        "pub fn tar_names_json",
    ] {
        assert!(
            source.contains(function),
            "source package is missing `{function}`"
        );
    }
    assert!(
        ffi.contains("../../../corelib/core.archive/pkgs/archive/src/lib.rs"),
        "the ABI bridge must include the canonical ring-package kernel"
    );
    assert!(
        !retired_copy.exists(),
        "a second archive runtime source would allow the two build paths to drift"
    );

    let temp = TempTree::new("jet_archive_source_boundary");
    let entry = temp.0.join("main.jet");
    let entry_source = "use core.archive as ar\nfn run() { ar.zip_compress(\"x\", [U8]{}) }\n";
    fs::write(&entry, entry_source).unwrap();
    let output = jet::compile_with_path(entry_source, entry.to_str().unwrap())
        .expect("Core source package must compile through the normal frontend");
    assert!(
        output.rust.contains("mod __jet_core_archive"),
        "reachable archive source module must be emitted"
    );
    assert!(
        output
            .rust
            .contains("__jet_core_archive::__jet_zip_compress"),
        "public archive calls must target the emitted source module"
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
    bytes :: [U8]{ 1, 2, 3 }
    ar.gzip_compress(bytes)
}
"#;
    let diags = jet::compile(src).expect_err("D-CORE-COMPRESS1=A removes gzip from core.archive");
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
    assert!(
        out.ffi.is_some(),
        "Core codec/container call must produce an FFI bridge"
    );
    let user_rust = common::strip_vetted_prelude_modules(&out.rust);
    assert!(
        user_rust
            .lines()
            .all(|line| common::unsafe_keyword_columns(line).is_empty()),
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
        "archive program failed at runtime:\nstderr :: {}",
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
        rustc
            .arg("-L")
            .arg(format!("dependency={}", deps_dir.display()));
    }
    rustc.output().unwrap()
}

#[test]
fn gzip_round_trip_uses_core_compress() {
    if !have_toolchain() {
        eprintln!("note: cargo/rustc not found; skipping archive codec integration test");
        return;
    }
    let src = r#"
use core.archive.gzip as gz

fn run() {
    original :: [U8]{ 72, 101, 108, 108, 111 }
    compressed :: gz.compress(original)
    print((compressed.len() > 5))
    restored :: gz.decompress(compressed) ?? panic("bad gzip")
    print((restored == original))
}
"#;
    let out = run_core_bridge(src);
    assert_eq!(out, "true\ntrue\n", "gzip round-trip failed: {out:?}");
}

fn zstd_rle_frame(output_len: usize, byte: u8) -> Vec<u8> {
    let mut frame = vec![0x28, 0xb5, 0x2f, 0xfd, 0xe0];
    frame.extend_from_slice(&(output_len as u64).to_le_bytes());
    let mut remaining = output_len;
    while remaining > 0 {
        let block_len = remaining.min(128 * 1024);
        let last = block_len == remaining;
        let header = ((block_len as u32) << 3) | (1 << 1) | u32::from(last);
        frame.extend_from_slice(&header.to_le_bytes()[..3]);
        frame.push(byte);
        remaining -= block_len;
    }
    frame
}

#[test]
fn runtime_compressors_reject_output_over_the_shared_budget() {
    if !have_toolchain() {
        eprintln!("note: cargo/rustc not found; skipping hostile codec integration test");
        return;
    }

    const OUTPUT_LIMIT: usize = 64 * 1024 * 1024;
    let temp = TempTree::new("jet_archive_codec_limits");
    let gzip_path = temp.0.join("oversized.gz");
    let zstd_path = temp.0.join("oversized.zst");
    let gzip = jet_foundation::GzipKernel::jet_compress_gzip_compress(&vec![b'x'; OUTPUT_LIMIT + 1]);
    fs::write(&gzip_path, gzip).unwrap();
    fs::write(&zstd_path, zstd_rle_frame(OUTPUT_LIMIT + 1, b'x')).unwrap();

    let escape = |path: &Path| path.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
    let source = r#"
use core.archive.gzip as gzip
use core.archive.zstd as zstd
use core.files as files

fn run() {
    gzip_bytes :: files.read_bytes("__GZIP__") ?? panic("gzip fixture")
    if gzip.decompress(gzip_bytes) == {
        .Ok(_) -> print("gzip accepted")
        .Err(_) -> print("gzip rejected")
        else -> print("gzip unexpected")
    }
    zstd_bytes :: files.read_bytes("__ZSTD__") ?? panic("zstd fixture")
    if zstd.decompress(zstd_bytes) == {
        .Ok(_) -> print("zstd accepted")
        .Err(_) -> print("zstd rejected")
        else -> print("zstd unexpected")
    }
}
"#
    .replace("__GZIP__", &escape(&gzip_path))
    .replace("__ZSTD__", &escape(&zstd_path));
    let out = run_core_bridge(&source);
    assert_eq!(out, "gzip rejected\nzstd rejected\n", "codec budget regression: {out:?}");
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
data :: [U8]{ 72, 101, 108, 108, 111 }
    zipped :: ar.zip_compress("hello.txt", data)
    print((ar.zip_decompress(zipped) == data))
    empty :: [U8]{}
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
fn archive_long_names_preserve_format_semantics_below_limits() {
    let zip_name = "zip-name/".repeat(120) + "file.txt";
    let zip = jet_foundation::CoreArchive::jet_archive_zip_compress(&zip_name, b"zip");
    assert_eq!(
        jet_foundation::CoreArchive::jet_archive_zip_names_json(&zip),
        format!("[\"{zip_name}\"]")
    );
    assert_eq!(
        jet_foundation::CoreArchive::jet_archive_zip_decompress(&zip),
        b"zip"
    );

    let tar_name = "tar-name/".repeat(40) + "file.txt";
    let tar = jet_foundation::CoreArchive::jet_archive_tar_add(&[], &tar_name, b"tar");
    assert_eq!(
        jet_foundation::CoreArchive::jet_archive_tar_names_json(&tar),
        format!("[\"{tar_name}\"]")
    );
    assert_eq!(
        jet_foundation::CoreArchive::jet_archive_tar_get(&tar, &tar_name),
        b"tar"
    );
}

#[test]
fn archive_name_json_escapes_controls() {
    let name = "quote\"line\ncontrol\u{0001}";
    let expected = "[\"quote\\\"line\\ncontrol\\u0001\"]";

    let zip = jet_foundation::CoreArchive::jet_archive_zip_compress(name, b"zip");
    assert_eq!(
        jet_foundation::CoreArchive::jet_archive_zip_names_json(&zip),
        expected
    );

    let tar = jet_foundation::CoreArchive::jet_archive_tar_add(&[], name, b"tar");
    assert_eq!(
        jet_foundation::CoreArchive::jet_archive_tar_names_json(&tar),
        expected
    );
}

fn push_archive_u16(output: &mut Vec<u8>, value: usize) {
    output.extend_from_slice(&(value as u16).to_le_bytes());
}

fn push_archive_u32(output: &mut Vec<u8>, value: usize) {
    output.extend_from_slice(&(value as u32).to_le_bytes());
}

fn zip_names_json_materialization_bomb() -> Vec<u8> {
    const ENTRY_COUNT: usize = 4096;
    const NAME_LEN: usize = 4096;
    let name = vec![1u8; NAME_LEN];
    let mut local = Vec::new();
    let mut central = Vec::new();
    for _ in 0..ENTRY_COUNT {
        let local_offset = local.len();
        push_archive_u32(&mut local, 0x0403_4b50);
        push_archive_u16(&mut local, 20);
        push_archive_u16(&mut local, 0);
        push_archive_u16(&mut local, 0);
        push_archive_u16(&mut local, 0);
        push_archive_u16(&mut local, 0);
        push_archive_u32(&mut local, 0);
        push_archive_u32(&mut local, 0);
        push_archive_u32(&mut local, 0);
        push_archive_u16(&mut local, NAME_LEN);
        push_archive_u16(&mut local, 0);
        local.extend_from_slice(&name);

        push_archive_u32(&mut central, 0x0201_4b50);
        push_archive_u16(&mut central, 20);
        push_archive_u16(&mut central, 20);
        push_archive_u16(&mut central, 0);
        push_archive_u16(&mut central, 0);
        push_archive_u16(&mut central, 0);
        push_archive_u16(&mut central, 0);
        push_archive_u32(&mut central, 0);
        push_archive_u32(&mut central, 0);
        push_archive_u32(&mut central, 0);
        push_archive_u16(&mut central, NAME_LEN);
        push_archive_u16(&mut central, 0);
        push_archive_u16(&mut central, 0);
        push_archive_u16(&mut central, 0);
        push_archive_u16(&mut central, 0);
        push_archive_u32(&mut central, 0);
        push_archive_u32(&mut central, local_offset);
        central.extend_from_slice(&name);
    }

    let central_offset = local.len();
    let central_size = central.len();
    let mut archive = local;
    archive.extend_from_slice(&central);
    push_archive_u32(&mut archive, 0x0605_4b50);
    push_archive_u16(&mut archive, 0);
    push_archive_u16(&mut archive, 0);
    push_archive_u16(&mut archive, ENTRY_COUNT);
    push_archive_u16(&mut archive, ENTRY_COUNT);
    push_archive_u32(&mut archive, central_size);
    push_archive_u32(&mut archive, central_offset);
    push_archive_u16(&mut archive, 0);
    archive
}

fn tar_bomb_octal(field: &mut [u8], value: usize) {
    field.fill(b'0');
    field[field.len() - 1] = 0;
    let digits = format!("{value:o}");
    let start = field.len() - 1 - digits.len();
    field[start..start + digits.len()].copy_from_slice(digits.as_bytes());
}

fn append_tar_bomb_record(output: &mut Vec<u8>, name: &[u8], payload: &[u8], kind: u8) {
    let mut header = [0u8; 512];
    header[..name.len()].copy_from_slice(name);
    tar_bomb_octal(&mut header[100..108], 0o644);
    tar_bomb_octal(&mut header[124..136], payload.len());
    header[148..156].fill(b' ');
    header[156] = kind;
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    let checksum = header.iter().map(|byte| *byte as u64).sum::<u64>();
    let digits = format!("{checksum:06o}");
    header[148..154].copy_from_slice(&digits.as_bytes()[digits.len() - 6..]);
    header[154] = 0;
    output.extend_from_slice(&header);
    output.extend_from_slice(payload);
    let padded = (output.len() + 511) / 512 * 512;
    output.resize(padded, 0);
}

fn tar_names_json_materialization_bomb() -> Vec<u8> {
    const PAIR_COUNT: usize = 2048;
    const NAME_LEN: usize = 8192;
    let name = vec![1u8; NAME_LEN];
    let mut archive = Vec::new();
    for _ in 0..PAIR_COUNT {
        let mut long_name = name.clone();
        long_name.push(0);
        append_tar_bomb_record(&mut archive, b"././#LongLink", &long_name, b'L');
        append_tar_bomb_record(&mut archive, b"x", b"x", b'0');
    }
    archive.extend_from_slice(&[0; 1024]);
    archive
}

#[test]
fn archive_public_zip_names_json_rejects_aggregate_materialization_bomb() {
    let archive = zip_names_json_materialization_bomb();
    assert!(archive.len() < 64 * 1024 * 1024);
    assert!(
        !jet_foundation::CoreArchive::jet_archive_zip_open(&archive).is_empty(),
        "ZIP materialization bomb fixture must parse before JSON sizing"
    );
    assert_eq!(
        jet_foundation::CoreArchive::jet_archive_zip_names_json(&archive),
        ""
    );
}

#[test]
fn archive_public_tar_names_json_rejects_aggregate_materialization_bomb() {
    let archive = tar_names_json_materialization_bomb();
    assert!(archive.len() < 64 * 1024 * 1024);
    let long_name = "\u{0001}".repeat(8192);
    assert_eq!(
        jet_foundation::CoreArchive::jet_archive_tar_get(&archive, &long_name),
        b"x"
    );
    assert_eq!(
        jet_foundation::CoreArchive::jet_archive_tar_names_json(&archive),
        ""
    );
}

#[test]
fn archive_public_tar_reader_rejects_an_entry_count_bomb() {
    const TOO_MANY_ENTRIES: usize = 4097;
    let mut archive = Vec::new();
    for index in 0..TOO_MANY_ENTRIES {
        let name = format!("entry-{index}");
        append_tar_bomb_record(&mut archive, name.as_bytes(), b"x", b'0');
    }
    archive.extend_from_slice(&[0; 1024]);
    assert!(archive.len() < 8 * 1024 * 1024);
    assert_eq!(
        jet_foundation::CoreArchive::jet_archive_tar_get(&archive, "entry-0"),
        Vec::<u8>::new()
    );
    assert_eq!(
        jet_foundation::CoreArchive::jet_archive_tar_names_json(&archive),
        ""
    );
}

#[test]
fn archive_direct_rustc_uses_target_and_host_dependency_dirs() {
    if !have_toolchain() {
        eprintln!("note: cargo/rustc not found; skipping archive link-contract test");
        return;
    }
    let temp = TempTree::new("jet_archive_link_contract");
    let jet_path = temp.0.join("archive_link_contract.jet");
    let src = r#"
use core.archive as ar

fn run() {
data :: [U8]{ 1, 2, 3 }
    zipped :: ar.zip_compress("data.bin", data)
    print((ar.zip_decompress(zipped) == data))
}
"#;
    fs::write(&jet_path, src).unwrap();
    let shown = jet_path.to_string_lossy();
    let out = jet::compile_with_path(src, &shown).unwrap();
    let link = out
        .ffi
        .as_ref()
        .expect("core.archive must build an FFI bridge");

    let dirs: Vec<_> = link.dependency_dirs().collect();
    assert_eq!(dirs, [&*link.target_deps_dir, &*link.host_deps_dir]);
    assert_ne!(link.target_deps_dir, link.host_deps_dir);
    assert!(link.target_deps_dir.is_dir());
    assert!(link.host_deps_dir.is_dir());

    let rs = temp.0.join("archive_link_contract.rs");
    fs::write(&rs, out.rust).unwrap();
    let complete = rustc_bridge(&rs, &temp.0.join("complete_bin"), link, &dirs);
    assert!(
        complete.status.success(),
        "target + host dependency directories must link:\n{}",
        String::from_utf8_lossy(&complete.stderr)
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
        assert!(
            !path.exists(),
            "archive fixture cleanup leaked {}",
            path.display()
        );
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
    // D-JPK-REF1=A: package@source (email order). Old colon form was source:package.
    let spec = classify_in("archive@ring", &table).unwrap();
    let ctx = Ctx {
        fixtures: None,
        store_dir: &store,
        offline: false,
        project_dir: None,
        nix_index: None,
        nix_roots: None,
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
    let error =
        Store::closure_graph(&roots).expect_err("changed output must invalidate closure proof");
    assert!(
        error
            .to_string()
            .contains("has no dependency references or store-validated closure proof"),
        "changed output must fail its content-bound closure proof: {error}"
    );
}
