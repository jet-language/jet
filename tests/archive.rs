//! Canonical `core.archive` coverage through Jet source and all hosted tiers.
//!
//! Containers live in `core.archive`; whole-buffer stream codecs live in the
//! nested `core.archive.gzip` and `core.archive.zstd` modules. Every behavioral
//! assertion below runs through `tir_support::assert_tiers_agree` so AOT, the
//! default JIT, and forced interpretation exercise the same source API.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

struct TempTree(PathBuf);

impl TempTree {
    fn new(prefix: &str) -> Self {
        let path = common::test_scratch_root("archive").join(format!(
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
        let _ = fs::remove_dir_all(&self.0);
    }
}

mod common;
#[path = "tir_support/mod.rs"]
mod tir_support;

#[test]
fn archive_containers_round_trip_and_reject_malformed_inputs_on_all_tiers() {
    let source = r#"
use core.archive as archive

fn run() {
    data :: [U8]{72, 101, 108, 108, 111}
    zip :: archive.create("hello.txt", data)
    print(zip.len() > 0)
    print((archive.zip_decompress(zip) ?? [U8]{}) == data)
    print((archive.zip_names_json(zip) ?? "") == "[\"hello.txt\"]")

    tar :: archive.tar_add([U8]{}, "hello.txt", data)
    print((archive.tar_get(tar, "hello.txt") ?? [U8]{}) == data)
    print((archive.tar_names_json(tar) ?? "") == "[\"hello.txt\"]")

    print(archive.zip_decompress([U8]{1, 2, 3}) == .Err(_))
    print(archive.tar_get([U8]{1, 2, 3}, "hello.txt") == .Err(_))
    print(archive.tar_names_json([U8]{}) == .Err(_))
}
"#;
    tir_support::assert_tiers_agree(
        "archive_containers_round_trip_and_malformed",
        source,
        "true\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\n",
    );
}

#[test]
fn archive_names_and_path_rejection_are_source_owned_on_all_tiers() {
    let source = r#"
use core.archive as archive

fn run() {
    zip_name :: "zip-name/".repeat(120) + "file.txt"
    zip :: archive.create(zip_name, [U8]{122, 105, 112})
    print((archive.zip_names_json(zip) ?? "") == "[\"{zip_name}\"]")
    print((archive.zip_decompress(zip) ?? [U8]{}) == [U8]{122, 105, 112})

    tar_name :: "tar-name/".repeat(40) + "file.txt"
    tar :: archive.tar_add([U8]{}, tar_name, [U8]{116, 97, 114})
    print((archive.tar_names_json(tar) ?? "") == "[\"{tar_name}\"]")
    print((archive.tar_get(tar, tar_name) ?? [U8]{}) == [U8]{116, 97, 114})

    valid :: archive.tar_add([U8]{}, "keep.txt", [U8]{1})
    attempted :: archive.tar_add(valid, "../escape", [U8]{2})
    print((archive.tar_names_json(attempted) ?? "") == "[\"keep.txt\"]")
    print((archive.tar_get(attempted, "keep.txt") ?? [U8]{}) == [U8]{1})
    print(archive.tar_get(attempted, "../escape") == .Err(_))
    print(archive.create("../escape", [U8]{2}).len() == 0)
}
"#;
    tir_support::assert_tiers_agree(
        "archive_names_and_path_rejection",
        source,
        "true\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\n",
    );
}

#[test]
fn archive_stream_codecs_round_trip_and_reject_malformed_inputs_on_all_tiers() {
    let source = r#"
use core.archive.gzip as gzip
use core.archive.zstd as zstd

fn run() {
    data :: [U8]{72, 101, 108, 108, 111, 32, 74, 101, 116}

    gz :: gzip.compress(data)
    print(gzip.is_gzip(gz))
    print((gzip.decompress(gz) ?? [U8]{}) == data)

    zst :: zstd.compress(data)
    print(zstd.is_zstd(zst))
    print((zstd.decompress(zst) ?? [U8]{}) == data)

    print(gzip.decompress(gz) == .Ok(_))
    print(zstd.decompress(zst) == .Ok(_))
    print(gzip.decompress([U8]{1, 2, 3}) == .Err(_))
    print(zstd.decompress([U8]{1, 2, 3}) == .Err(_))
}
"#;
    tir_support::assert_tiers_agree(
        "archive_stream_codecs_round_trip_and_malformed",
        source,
        "true\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\n",
    );
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
fn source_compressors_reject_output_over_the_shared_budget_on_all_tiers() {
    const OUTPUT_LIMIT: usize = 64 * 1024 * 1024;
    let temp = TempTree::new("jet_archive_codec_limits");
    let gzip_path = temp.0.join("oversized.gz");
    let zstd_path = temp.0.join("oversized.zst");
    let gzip = gzip_fixed_repeat_bomb(OUTPUT_LIMIT + 1, b'x');
    let zstd = zstd_rle_frame(OUTPUT_LIMIT + 1, b'x');
    fs::write(&gzip_path, gzip).unwrap();
    fs::write(&zstd_path, zstd).unwrap();

    let escape = |path: &Path| path.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
    let source = r#"
use core.archive.gzip as gzip
use core.archive.zstd as zstd
use core.files as files

fn run() {
    gzip_bytes :: files.read_bytes("__GZIP__") ?? panic("gzip fixture")
    print(gzip.decompress(gzip_bytes) == .Err(_))
    zstd_bytes :: files.read_bytes("__ZSTD__") ?? panic("zstd fixture")
    print(zstd.decompress(zstd_bytes) == .Err(_))
}
"#
    .replace("__GZIP__", &escape(&gzip_path))
    .replace("__ZSTD__", &escape(&zstd_path));
    tir_support::assert_tiers_agree(
        "source_compressors_output_budget",
        &source,
        "true\ntrue\n",
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
fn archive_hostile_materialization_limits_are_rejected_by_source_on_all_tiers() {
    let zip_names = zip_names_json_materialization_bomb();
    let zip_entries = zip_entry_count_bomb();
    let zip_output = zip_declared_output_bomb();
    let zip_materialization = zip_materialization_bomb();
    let tar_names = tar_names_json_materialization_bomb();
    let tar_entries = tar_entry_count_bomb();

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
    fs::write(&zip_materialization_path, zip_materialization).unwrap();
    fs::write(&tar_names_path, tar_names).unwrap();
    fs::write(&tar_entries_path, tar_entries).unwrap();

    let escape = |path: &Path| path.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
    let source = r#"
use core.archive as archive
use core.files as files

fn run() {
    zip_names :: files.read_bytes("__ZIP_NAMES__") ?? panic("zip names fixture")
    print(archive.zip_names_json(zip_names) == .Err(_))

    zip_entries :: files.read_bytes("__ZIP_ENTRIES__") ?? panic("zip entries fixture")
    print(archive.zip_open(zip_entries) == .Err(_))

    zip_output :: files.read_bytes("__ZIP_OUTPUT__") ?? panic("zip output fixture")
    print(archive.zip_decompress(zip_output) == .Err(_))

    zip_materialization :: files.read_bytes("__ZIP_MATERIALIZATION__") ?? panic("zip materialization fixture")
    print(archive.zip_decompress(zip_materialization) == .Err(_))

    tar_names :: files.read_bytes("__TAR_NAMES__") ?? panic("tar names fixture")
    print(archive.tar_names_json(tar_names) == .Err(_))

    tar_entries :: files.read_bytes("__TAR_ENTRIES__") ?? panic("tar entries fixture")
    print(archive.tar_get(tar_entries, "entry-0") == .Err(_))
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
        "true\ntrue\ntrue\ntrue\ntrue\ntrue\n",
    );
}
