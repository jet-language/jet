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
#[path = "tir_support/mod.rs"]
mod tir_support;
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

struct GzipBits {
    bytes: Vec<u8>,
    bit: u8,
}

impl GzipBits {
    fn write(&mut self, value: u32, bits: u8) {
        for offset in 0..bits {
            if self.bit == 0 {
                self.bytes.push(0);
            }
            if value & (1 << offset) != 0 {
                let last = self.bytes.len() - 1;
                self.bytes[last] |= 1 << self.bit;
            }
            self.bit = (self.bit + 1) % 8;
        }
    }
}

fn gzip_reverse_bits(mut code: u32, bits: u8) -> u32 {
    let mut reversed = 0;
    for _ in 0..bits {
        reversed = (reversed << 1) | (code & 1);
        code >>= 1;
    }
    reversed
}

fn gzip_fixed_code(symbol: usize) -> (u32, u8) {
    let (code, bits) = match symbol {
        0..=143 => (0x30 + symbol as u32, 8),
        144..=255 => (0x190 + (symbol - 144) as u32, 9),
        256..=279 => ((symbol - 256) as u32, 7),
        280..=287 => (0xc0 + (symbol - 280) as u32, 8),
        _ => unreachable!("fixed DEFLATE symbol"),
    };
    (gzip_reverse_bits(code, bits), bits)
}

fn gzip_length_code(length: usize) -> (usize, u32, u8) {
    const BASE: [usize; 29] = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83,
        99, 115, 131, 163, 195, 227, 258,
    ];
    const EXTRA: [u8; 29] = [
        0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5,
        5, 5, 0,
    ];
    for (index, (&base, &extra)) in BASE.iter().zip(EXTRA.iter()).enumerate() {
        let max = base + ((1usize << extra) - 1);
        if length <= max {
            return (257 + index, (length - base) as u32, extra);
        }
    }
    unreachable!("DEFLATE match length")
}

fn gzip_crc32_repeated(byte: u8, length: usize) -> u32 {
    let mut table = [0u32; 256];
    for (index, slot) in table.iter_mut().enumerate() {
        let mut crc = index as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
        *slot = crc;
    }

    let mut crc = !0u32;
    for _ in 0..length {
        crc = (crc >> 8) ^ table[((crc as u8) ^ byte) as usize];
    }
    !crc
}

/// A compact fixed-DEFLATE stream that expands through the shared codec cap.
fn gzip_fixed_repeat_bomb(output_len: usize, byte: u8) -> Vec<u8> {
    assert!(output_len > 0);
    let mut frame = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 255];
    let mut bits = GzipBits {
        bytes: Vec::new(),
        bit: 0,
    };
    bits.write(1, 1); // final block
    bits.write(1, 2); // fixed Huffman block

    let (literal, literal_bits) = gzip_fixed_code(byte as usize);
    bits.write(literal, literal_bits);
    let mut remaining = output_len - 1;
    while remaining >= 3 {
        let length = remaining.min(258);
        let (symbol, extra, extra_bits) = gzip_length_code(length);
        let (code, code_bits) = gzip_fixed_code(symbol);
        bits.write(code, code_bits);
        bits.write(extra, extra_bits);
        bits.write(0, 5); // distance symbol 0: one byte backwards
        remaining -= length;
    }
    for _ in 0..remaining {
        bits.write(literal, literal_bits);
    }
    let (end, end_bits) = gzip_fixed_code(256);
    bits.write(end, end_bits);
    frame.extend_from_slice(&bits.bytes);
    let checksum = gzip_crc32_repeated(byte, output_len);
    frame.extend_from_slice(&checksum.to_le_bytes());
    frame.extend_from_slice(&(output_len as u32).to_le_bytes());
    frame
}

#[test]
fn runtime_compressors_reject_output_over_the_shared_budget() {
    const OUTPUT_LIMIT: usize = 64 * 1024 * 1024;
    let temp = TempTree::new("jet_archive_codec_limits");
    let gzip_path = temp.0.join("oversized.gz");
    let zstd_path = temp.0.join("oversized.zst");
    let gzip = gzip_fixed_repeat_bomb(OUTPUT_LIMIT + 1, b'x');
    let zstd = zstd_rle_frame(OUTPUT_LIMIT + 1, b'x');
    assert!(gzip.len() < 1024 * 1024, "gzip bomb must stay compact");
    assert!(zstd.len() < 1024 * 1024, "zstd bomb must stay compact");
    fs::write(&gzip_path, gzip).unwrap();
    fs::write(&zstd_path, zstd).unwrap();

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
    _ :: zstd.decompress(zstd_bytes) ?? {
        print("zstd rejected")
        return
    }
    print("zstd accepted")
}
"#
    .replace("__GZIP__", &escape(&gzip_path))
    .replace("__ZSTD__", &escape(&zstd_path));
    tir_support::assert_tiers_agree(
        "runtime_compressors_output_budget",
        &source,
        "gzip rejected\nzstd rejected\n",
    );
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

fn zip_limit_bomb(
    name: &[u8],
    entry_count: usize,
    method: u16,
    compressed: &[u8],
    uncompressed_len: usize,
    checksum: u32,
) -> Vec<u8> {
    let mut local = Vec::new();
    let mut central = Vec::new();
    for _ in 0..entry_count {
        let local_offset = local.len();
        push_archive_u32(&mut local, 0x0403_4b50);
        push_archive_u16(&mut local, 20);
        push_archive_u16(&mut local, 0);
        push_archive_u16(&mut local, method as usize);
        push_archive_u16(&mut local, 0);
        push_archive_u16(&mut local, 0);
        push_archive_u32(&mut local, checksum as usize);
        push_archive_u32(&mut local, compressed.len());
        push_archive_u32(&mut local, uncompressed_len);
        push_archive_u16(&mut local, name.len());
        push_archive_u16(&mut local, 0);
        local.extend_from_slice(name);
        local.extend_from_slice(compressed);

        push_archive_u32(&mut central, 0x0201_4b50);
        push_archive_u16(&mut central, 20);
        push_archive_u16(&mut central, 20);
        push_archive_u16(&mut central, 0);
        push_archive_u16(&mut central, method as usize);
        push_archive_u16(&mut central, 0);
        push_archive_u16(&mut central, 0);
        push_archive_u32(&mut central, checksum as usize);
        push_archive_u32(&mut central, compressed.len());
        push_archive_u32(&mut central, uncompressed_len);
        push_archive_u16(&mut central, name.len());
        push_archive_u16(&mut central, 0);
        push_archive_u16(&mut central, 0);
        push_archive_u16(&mut central, 0);
        push_archive_u16(&mut central, 0);
        push_archive_u32(&mut central, 0);
        push_archive_u32(&mut central, local_offset);
        central.extend_from_slice(name);
    }

    let central_offset = local.len();
    let central_size = central.len();
    let mut archive = local;
    archive.extend_from_slice(&central);
    push_archive_u32(&mut archive, 0x0605_4b50);
    push_archive_u16(&mut archive, 0);
    push_archive_u16(&mut archive, 0);
    push_archive_u16(&mut archive, entry_count);
    push_archive_u16(&mut archive, entry_count);
    push_archive_u32(&mut archive, central_size);
    push_archive_u32(&mut archive, central_offset);
    push_archive_u16(&mut archive, 0);
    archive
}

fn zip_entry_count_bomb() -> Vec<u8> {
    zip_limit_bomb(b"x", 4097, 0, &[], 0, 0)
}

fn zip_declared_output_bomb() -> Vec<u8> {
    const OUTPUT_LIMIT: usize = 64 * 1024 * 1024;
    let gzip = gzip_fixed_repeat_bomb(OUTPUT_LIMIT + 1, b'x');
    let checksum = u32::from_le_bytes([
        gzip[gzip.len() - 8],
        gzip[gzip.len() - 7],
        gzip[gzip.len() - 6],
        gzip[gzip.len() - 5],
    ]);
    zip_limit_bomb(
        b"x",
        1,
        8,
        &gzip[10..gzip.len() - 8],
        OUTPUT_LIMIT + 1,
        checksum,
    )
}

fn zip_materialization_bomb() -> Vec<u8> {
    const OUTPUT_LIMIT: usize = 64 * 1024 * 1024;
    const NAME_LEN: usize = 512;
    let name = vec![b'n'; NAME_LEN];
    let gzip = gzip_fixed_repeat_bomb(OUTPUT_LIMIT - NAME_LEN + 1, b'x');
    let checksum = u32::from_le_bytes([
        gzip[gzip.len() - 8],
        gzip[gzip.len() - 7],
        gzip[gzip.len() - 6],
        gzip[gzip.len() - 5],
    ]);
    zip_limit_bomb(
        &name,
        1,
        8,
        &gzip[10..gzip.len() - 8],
        OUTPUT_LIMIT - NAME_LEN + 1,
        checksum,
    )
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

fn tar_entry_count_bomb() -> Vec<u8> {
    const TOO_MANY_ENTRIES: usize = 4097;
    let mut archive = Vec::new();
    for index in 0..TOO_MANY_ENTRIES {
        let name = format!("entry-{index}");
        append_tar_bomb_record(&mut archive, name.as_bytes(), b"x", b'0');
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
    let archive = tar_entry_count_bomb();
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
fn archive_hostile_materialization_limits_match_all_execution_tiers() {
    let zip_names = zip_names_json_materialization_bomb();
    let zip_entries = zip_entry_count_bomb();
    let zip_output = zip_declared_output_bomb();
    let zip_materialization = zip_materialization_bomb();
    let tar_names = tar_names_json_materialization_bomb();
    let tar_entries = tar_entry_count_bomb();

    assert!(zip_names.len() < 64 * 1024 * 1024);
    assert!(
        !jet_foundation::CoreArchive::jet_archive_zip_open(&zip_names).is_empty(),
        "ZIP materialization bomb fixture must parse before JSON sizing"
    );
    assert!(zip_entries.len() < 8 * 1024 * 1024);
    assert!(
        jet_foundation::CoreArchive::jet_archive_zip_open(&zip_entries).is_empty(),
        "ZIP entry-count bomb must be rejected by the public reader"
    );
    assert!(zip_output.len() < 8 * 1024 * 1024);
    assert!(
        jet_foundation::CoreArchive::jet_archive_zip_open(&zip_output).is_empty(),
        "ZIP declared-output bomb must be rejected by the public reader"
    );
    assert!(zip_materialization.len() < 8 * 1024 * 1024);
    assert!(
        jet_foundation::CoreArchive::jet_archive_zip_open(&zip_materialization).is_empty(),
        "ZIP aggregate materialization bomb must be rejected before allocation"
    );

    assert!(tar_names.len() < 64 * 1024 * 1024);
    assert_eq!(
        jet_foundation::CoreArchive::jet_archive_tar_get(&tar_names, &"\u{0001}".repeat(8192)),
        b"x"
    );
    assert!(tar_entries.len() < 8 * 1024 * 1024);
    assert_eq!(
        jet_foundation::CoreArchive::jet_archive_tar_get(&tar_entries, "entry-0"),
        Vec::<u8>::new()
    );

    let temp = TempTree::new("jet_archive_hostile_limits");
    let zip_names_path = temp.0.join("zip_names.bin");
    let zip_entries_path = temp.0.join("zip_entries.bin");
    let zip_output_path = temp.0.join("zip_output.bin");
    let zip_materialization_path = temp.0.join("zip_materialization.bin");
    let tar_names_path = temp.0.join("tar_names.bin");
    let tar_entries_path = temp.0.join("tar_entries.bin");
    fs::write(&zip_names_path, zip_names).unwrap();
    fs::write(&zip_entries_path, zip_entries).unwrap();
    fs::write(&zip_output_path, zip_output).unwrap();
    fs::write(&tar_names_path, tar_names).unwrap();
    fs::write(&zip_materialization_path, zip_materialization).unwrap();
    fs::write(&tar_entries_path, tar_entries).unwrap();

    let escape = |path: &Path| path.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
    let source = r#"
use core.archive as ar
use core.files as files

fn zip_names_case(path: String) {
    bytes :: files.read_bytes(path) ?? panic("zip names fixture")
    if {
        ar.zip_names_json(bytes) == "" -> print("zip-names:rejected")
        else -> print("zip-names:accepted")
    }
}

fn zip_entries_case(path: String) {
    bytes :: files.read_bytes(path) ?? panic("zip entries fixture")
    if {
        ar.zip_open(bytes).len() == 0 -> print("zip-entries:rejected")
        else -> print("zip-entries:accepted")
    }
}

fn zip_output_case(path: String) {
    bytes :: files.read_bytes(path) ?? panic("zip output fixture")
    if {
        ar.zip_decompress(bytes).len() == 0 -> print("zip-output:rejected")
        else -> print("zip-output:accepted")
    }
}

fn zip_materialization_case(path: String) {
    bytes :: files.read_bytes(path) ?? panic("zip materialization fixture")
    if {
        ar.zip_decompress(bytes).len() == 0 -> print("zip-materialization:rejected")
        else -> print("zip-materialization:accepted")
    }
}

fn tar_names_case(path: String) {
    bytes :: files.read_bytes(path) ?? panic("tar names fixture")
    if {
        ar.tar_names_json(bytes) == "" -> print("tar-names:rejected")
        else -> print("tar-names:accepted")
    }
}

fn tar_entries_case(path: String) {
    bytes :: files.read_bytes(path) ?? panic("tar entries fixture")
    if {
        ar.tar_get(bytes, "entry-0").len() == 0 -> print("tar-entries:rejected")
        else -> print("tar-entries:accepted")
    }
}

fn run() {
    zip_names_case("__ZIP_NAMES__")
    zip_entries_case("__ZIP_ENTRIES__")
    zip_output_case("__ZIP_OUTPUT__")
    zip_materialization_case("__ZIP_MATERIALIZATION__")
    tar_names_case("__TAR_NAMES__")
    tar_entries_case("__TAR_ENTRIES__")
}
"#
    .replace("__ZIP_NAMES__", &escape(&zip_names_path))
    .replace("__ZIP_ENTRIES__", &escape(&zip_entries_path))
    .replace("__ZIP_OUTPUT__", &escape(&zip_output_path))
    .replace("__ZIP_MATERIALIZATION__", &escape(&zip_materialization_path))
    .replace("__TAR_NAMES__", &escape(&tar_names_path))
    .replace("__TAR_ENTRIES__", &escape(&tar_entries_path));
    tir_support::assert_tiers_agree(
        "archive_hostile_materialization_limits",
        &source,
        "zip-names:rejected\nzip-entries:rejected\nzip-output:rejected\nzip-materialization:rejected\ntar-names:rejected\ntar-entries:rejected\n",
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
