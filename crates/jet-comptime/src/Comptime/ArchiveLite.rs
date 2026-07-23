//! Std-only archive and stream-codec evaluator used by comptime, REPL, and
//! interpreter.
//!
//! ZIP writes the same uncompressed single-entry shape as AOT's
//! `ZipWriter::start_file(FileOptions::default())`; reads accept stored and
//! DEFLATE entries. TAR reads ordinary ustar/GNU archives and writes GNU
//! archives. GZIP writes stored DEFLATE and reads stored/fixed/dynamic DEFLATE.
//! Zstandard writes ordinary raw-block frames.
//! Invalid inputs stay bounded and follow each public API's existing
//! empty/`Err` contract.

use std::path::{Component, Path};

const TAR_BLOCK: usize = 512;
const MAX_CODEC_OUTPUT: usize = 64 * 1024 * 1024;
const ZSTD_BLOCK_MAX: usize = 128 * 1024;
const ZSTD_WINDOW_MAX: u64 = 128 * 1024 * 1024;
const U32_MAX_U64: u64 = u32::MAX as u64;

/// Emit a standards-valid Zstandard frame without bringing the native `zstd`
/// dependency into the compiler seam. Compression ratio is deliberately zero;
/// AOT's full decoder accepts the resulting raw blocks.
pub(super) fn zstd_compress(data: &[u8]) -> Vec<u8> {
    let mut out =
        Vec::with_capacity(data.len().saturating_add(16 + data.len() / ZSTD_BLOCK_MAX * 3));
    zstd_frame_header(&mut out, data.len() as u64);

    if data.is_empty() {
        put_u24(&mut out, 1); // last raw block, zero bytes
    } else {
        let mut blocks = data.chunks(ZSTD_BLOCK_MAX).peekable();
        while let Some(block) = blocks.next() {
            let header = ((block.len() as u32) << 3) | u32::from(blocks.peek().is_none());
            put_u24(&mut out, header);
            out.extend_from_slice(block);
        }
    }
    out
}

// Dictionaryless, std-only Zstandard decoder used by the resident evaluator.
pub(super) fn zstd_decompress(data: &[u8]) -> Result<Vec<u8>, String> {
    let fail = |reason: &str| format!("compress.zstd.decompress: {reason}");
    let mut input = 0usize;
    let mut out = Vec::new();
    while input < data.len() {
        let magic = read_u32(data, input).ok_or_else(|| fail("invalid zstd data"))?;
        input += 4;
        if (0x184d_2a50..=0x184d_2a5f).contains(&magic) {
            let size = read_u32(data, input).ok_or_else(|| fail("invalid zstd data"))? as usize;
            input = input
                .checked_add(4 + size)
                .filter(|end| *end <= data.len())
                .ok_or_else(|| fail("invalid zstd data"))?;
            continue;
        }
        if magic != 0xfd2f_b528 {
            return Err(fail("invalid zstd data"));
        }
        let frame_start = out.len();
        let descriptor = *data.get(input).ok_or_else(|| fail("invalid zstd data"))?;
        input += 1;
        if descriptor & 0x08 != 0 {
            return Err(fail("invalid zstd data"));
        }
        let single_segment = descriptor & 0x20 != 0;
        let checksum = descriptor & 0x04 != 0;
        let dict_size = [0usize, 1, 2, 4][usize::from(descriptor & 3)];
        let fcs_size = match (descriptor >> 6, single_segment) {
            (0, false) => 0,
            (0, true) => 1,
            (1, _) => 2,
            (2, _) => 4,
            _ => 8,
        };
        let window = if single_segment {
            None
        } else {
            let byte = *data.get(input).ok_or_else(|| fail("invalid zstd data"))?;
            input += 1;
            let base = 1u64 << (10 + u32::from(byte >> 3));
            Some(base + base / 8 * u64::from(byte & 7))
        };
        let dictionary = read_le(data, input, dict_size).ok_or_else(|| fail("invalid zstd data"))?;
        input += dict_size;
        if dictionary != 0 {
            return Err(fail("dictionary frames are unsupported"));
        }
        let mut expected = read_le(data, input, fcs_size).ok_or_else(|| fail("invalid zstd data"))?;
        input += fcs_size;
        if fcs_size == 2 {
            expected += 256;
        }
        let expected = (fcs_size != 0).then_some(expected);
        let window = if single_segment {
            expected.ok_or_else(|| fail("invalid zstd data"))?
        } else {
            window.ok_or_else(|| fail("invalid zstd data"))?
        };
        if window > ZSTD_WINDOW_MAX {
            return Err(fail("frame exceeds the 128 MiB window limit"));
        }
        if expected.is_some_and(|n| n > MAX_CODEC_OUTPUT as u64) {
            return Err(fail("frame exceeds the 64 MiB output limit"));
        }
        let block_max = window.min(ZSTD_BLOCK_MAX as u64) as usize;
        let mut huffman = super::ZstdEntropy::HuffmanState::default();
        let mut sequences = super::ZstdEntropy::SequenceState::default();
        loop {
            let header = read_le(data, input, 3).ok_or_else(|| fail("invalid zstd data"))? as u32;
            input += 3;
            let last = header & 1 != 0;
            let kind = (header >> 1) & 3;
            let size = (header >> 3) as usize;
            if size > block_max
                || (kind != 2
                    && out
                        .len()
                        .checked_add(size)
                        .is_none_or(|len| len > MAX_CODEC_OUTPUT))
            {
                return Err(fail("frame exceeds the 64 MiB output limit"));
            }
            match kind {
                0 => {
                    let end = input
                        .checked_add(size)
                        .filter(|end| *end <= data.len())
                        .ok_or_else(|| fail("invalid zstd data"))?;
                    out.extend_from_slice(&data[input..end]);
                    input = end;
                }
                1 => {
                    let byte = *data.get(input).ok_or_else(|| fail("invalid zstd data"))?;
                    input += 1;
                    out.resize(out.len() + size, byte);
                }
                2 => {
                    let end = input
                        .checked_add(size)
                        .filter(|end| *end <= data.len())
                        .ok_or_else(|| fail("invalid zstd data"))?;
                    let block = &data[input..end];
                    let (literals, used) = super::ZstdEntropy::literals(block, &mut huffman)
                        .ok_or_else(|| fail("invalid compressed literals"))?;
                    super::ZstdEntropy::sequences(
                        block.get(used..).ok_or_else(|| fail("invalid compressed sequences"))?,
                        &literals,
                        &mut sequences,
                        &mut out,
                        frame_start,
                        window as usize,
                        block_max,
                        MAX_CODEC_OUTPUT,
                    )
                    .ok_or_else(|| fail("invalid compressed sequences"))?;
                    input = end;
                }
                _ => return Err(fail("invalid zstd data")),
            }
            if last {
                break;
            }
        }
        let frame = &out[frame_start..];
        if expected.is_some_and(|size| size != frame.len() as u64) {
            return Err(fail("frame content size mismatch"));
        }
        if checksum {
            let stored = read_u32(data, input).ok_or_else(|| fail("invalid zstd data"))?;
            input += 4;
            if stored != xxh64(frame) as u32 {
                return Err(fail("checksum mismatch"));
            }
        }
    }
    if data.is_empty() {
        Err(fail("invalid zstd data"))
    } else {
        Ok(out)
    }
}

fn zstd_frame_header(out: &mut Vec<u8>, len: u64) {
    out.extend_from_slice(&0xfd2f_b528u32.to_le_bytes());
    out.push(match len {
        0..=255 => 0,
        256..=65_791 => 0x40,
        65_792..=U32_MAX_U64 => 0x80,
        _ => 0xc0,
    });
    out.push(0x38); // fixed 128 KiB decode window
    match len {
        0..=255 => {} // streaming frame, no content-size field
        256..=65_791 => put_u16(out, (len - 256) as u16),
        65_792..=U32_MAX_U64 => put_u32(out, len as u32),
        _ => out.extend_from_slice(&len.to_le_bytes()),
    }
}

pub(super) fn gzip_compress(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 255];
    if data.is_empty() {
        out.extend_from_slice(&[1, 0, 0, 255, 255]);
    } else {
        let mut blocks = data.chunks(u16::MAX as usize).peekable();
        while let Some(block) = blocks.next() {
            out.push(u8::from(blocks.peek().is_none()));
            let len = block.len() as u16;
            put_u16(&mut out, len);
            put_u16(&mut out, !len);
            out.extend_from_slice(block);
        }
    }
    put_u32(&mut out, crc32(data));
    put_u32(&mut out, data.len() as u32);
    out
}

pub(super) fn gzip_decompress(data: &[u8]) -> Result<Vec<u8>, String> {
    let fail = || "compress.gzip.decompress: invalid gzip data".to_string();
    if data.len() < 18 || data[..3] != [0x1f, 0x8b, 8] {
        return Err(fail());
    }
    let flags = data[3];
    if flags & 0xe0 != 0 {
        return Err(fail());
    }
    let trailer = data.len() - 8;
    let mut offset = 10usize;
    if flags & 4 != 0 {
        let len = read_u16(data, offset).ok_or_else(&fail)? as usize;
        offset = offset
            .checked_add(2 + len)
            .filter(|end| *end <= trailer)
            .ok_or_else(&fail)?;
    }
    for mask in [8, 16] {
        if flags & mask != 0 {
            let len = data
                .get(offset..trailer)
                .and_then(|rest| rest.iter().position(|byte| *byte == 0))
                .ok_or_else(&fail)?;
            offset = offset
                .checked_add(len + 1)
                .filter(|end| *end <= trailer)
                .ok_or_else(&fail)?;
        }
    }
    if flags & 2 != 0 {
        let header_crc = read_u16(data, offset).ok_or_else(&fail)?;
        if header_crc != crc32(&data[..offset]) as u16 {
            return Err(fail());
        }
        offset = offset
            .checked_add(2)
            .filter(|end| *end <= trailer)
            .ok_or_else(&fail)?;
    }
    let expected_size = read_u32(data, trailer + 4).ok_or_else(&fail)?;
    let expected = expected_size as usize;
    if expected > MAX_CODEC_OUTPUT || offset > trailer {
        return Err(fail());
    }
    let (out, consumed) =
        inflate_with_consumed(&data[offset..trailer], expected).ok_or_else(&fail)?;
    if consumed != trailer - offset {
        return Err(fail());
    }
    if out.len() as u32 != expected_size {
        return Err(fail());
    }
    if crc32(&out) != read_u32(data, trailer).ok_or_else(&fail)? {
        return Err(fail());
    }
    Ok(out)
}

pub(super) fn zip_compress(name: &str, data: &[u8]) -> Vec<u8> {
    let name = name.as_bytes();
    let Ok(name_len) = u16::try_from(name.len()) else {
        return Vec::new();
    };
    let Ok(size) = u32::try_from(data.len()) else {
        return Vec::new();
    };
    let crc = crc32(data);
    let mut out = Vec::with_capacity(30 + name.len() + data.len() + 46 + name.len() + 22);

    put_u32(&mut out, 0x0403_4b50);
    put_u16(&mut out, 10);
    put_u16(&mut out, 0);
    put_u16(&mut out, 0); // stored
    put_u16(&mut out, 0);
    put_u16(&mut out, 0x21); // 1980-01-01
    put_u32(&mut out, crc);
    put_u32(&mut out, size);
    put_u32(&mut out, size);
    put_u16(&mut out, name_len);
    put_u16(&mut out, 0);
    out.extend_from_slice(name);
    out.extend_from_slice(data);

    let central_offset = out.len() as u32;
    put_u32(&mut out, 0x0201_4b50);
    put_u16(&mut out, 0x031e); // Unix, ZIP 3.0
    put_u16(&mut out, 10);
    put_u16(&mut out, 0);
    put_u16(&mut out, 0);
    put_u16(&mut out, 0);
    put_u16(&mut out, 0x21);
    put_u32(&mut out, crc);
    put_u32(&mut out, size);
    put_u32(&mut out, size);
    put_u16(&mut out, name_len);
    put_u16(&mut out, 0);
    put_u16(&mut out, 0);
    put_u16(&mut out, 0);
    put_u16(&mut out, 0);
    put_u32(&mut out, 0);
    put_u32(&mut out, 0);
    out.extend_from_slice(name);

    let central_size = out.len() as u32 - central_offset;
    put_u32(&mut out, 0x0605_4b50);
    put_u16(&mut out, 0);
    put_u16(&mut out, 0);
    put_u16(&mut out, 1);
    put_u16(&mut out, 1);
    put_u32(&mut out, central_size);
    put_u32(&mut out, central_offset);
    put_u16(&mut out, 0);
    out
}

pub(super) fn zip_decompress(data: &[u8]) -> Vec<u8> {
    let Some(eocd) = find_eocd(data) else {
        return Vec::new();
    };
    if read_u16(data, eocd + 10).unwrap_or(0) == 0 {
        return Vec::new();
    }
    let Some(central) = read_u32(data, eocd + 16).map(|n| n as usize) else {
        return Vec::new();
    };
    if read_u32(data, central) != Some(0x0201_4b50) {
        return Vec::new();
    }
    let flags = read_u16(data, central + 8).unwrap_or(1);
    if flags & 1 != 0 {
        return Vec::new();
    }
    let method = read_u16(data, central + 10).unwrap_or(u16::MAX);
    let Some(compressed_len) = read_u32(data, central + 20).map(|n| n as usize) else {
        return Vec::new();
    };
    let Some(expected_len) = read_u32(data, central + 24).map(|n| n as usize) else {
        return Vec::new();
    };
    let Some(local) = read_u32(data, central + 42).map(|n| n as usize) else {
        return Vec::new();
    };
    if read_u32(data, local) != Some(0x0403_4b50) {
        return Vec::new();
    }
    let Some(name_len) = read_u16(data, local + 26).map(|n| n as usize) else {
        return Vec::new();
    };
    let Some(extra_len) = read_u16(data, local + 28).map(|n| n as usize) else {
        return Vec::new();
    };
    let Some(start) = local.checked_add(30 + name_len + extra_len) else {
        return Vec::new();
    };
    let Some(end) = start.checked_add(compressed_len) else {
        return Vec::new();
    };
    let Some(payload) = data.get(start..end) else {
        return Vec::new();
    };
    let out = match method {
        0 => payload.to_vec(),
        8 => inflate(payload, expected_len).unwrap_or_default(),
        _ => Vec::new(),
    };
    if out.len() == expected_len { out } else { Vec::new() }
}

pub(super) fn tar_add(archive: &[u8], name: &str, data: &[u8]) -> Vec<u8> {
    let mut entries = tar_read_all(archive);
    if let Some(index) = entries.iter().position(|(entry, _)| entry == name) {
        entries[index] = (name.to_string(), data.to_vec());
    } else {
        entries.push((name.to_string(), data.to_vec()));
    }
    tar_write_all(&entries)
}

pub(super) fn tar_get(archive: &[u8], name: &str) -> Vec<u8> {
    tar_read_all(archive)
        .into_iter()
        .find_map(|(entry, data)| (entry == name).then_some(data))
        .unwrap_or_default()
}

pub(super) fn tar_names_json(archive: &[u8]) -> String {
    let mut out = String::from("[");
    for (index, (name, _)) in tar_read_all(archive).iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('"');
        for ch in name.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                ch => out.push(ch),
            }
        }
        out.push('"');
    }
    out.push(']');
    out
}

fn tar_read_all(data: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    let mut long_name = None;
    while let Some(header) = data.get(offset..offset.saturating_add(TAR_BLOCK)) {
        if header.iter().all(|byte| *byte == 0) || !tar_checksum_ok(header) {
            break;
        }
        let Some(size) = tar_number(&header[124..136]).and_then(|n| usize::try_from(n).ok()) else {
            break;
        };
        let Some(payload_start) = offset.checked_add(TAR_BLOCK) else {
            break;
        };
        let Some(payload_end) = payload_start.checked_add(size) else {
            break;
        };
        let Some(payload) = data.get(payload_start..payload_end) else {
            break;
        };
        let kind = header[156];
        if kind == b'L' {
            long_name = Some(trim_nul(payload));
        } else if kind == b'x' {
            if let Some(path) = pax_path(payload) {
                long_name = Some(path);
            }
        } else if kind != b'g' {
            let name = long_name.take().unwrap_or_else(|| tar_header_name(header));
            out.push((name, payload.to_vec()));
        }
        let padded = size.saturating_add(TAR_BLOCK - 1) / TAR_BLOCK * TAR_BLOCK;
        let Some(next) = payload_start.checked_add(padded) else {
            break;
        };
        offset = next;
    }
    out
}

fn tar_write_all(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, data) in entries {
        if !tar_name_valid(name) {
            continue;
        }
        if split_ustar_name(name).is_none() {
            let mut long = name.as_bytes().to_vec();
            long.push(0);
            append_tar_entry(&mut out, "././#LongLink", &long, b'L');
        }
        append_tar_entry(&mut out, name, data, b'0');
    }
    out.resize(out.len() + TAR_BLOCK * 2, 0);
    out
}

fn tar_name_valid(name: &str) -> bool {
    !name.is_empty()
        && !name.as_bytes().contains(&0)
        && !Path::new(name).components().any(|component| {
            matches!(
                component,
                Component::Prefix(_) | Component::RootDir | Component::ParentDir
            )
        })
}

fn append_tar_entry(out: &mut Vec<u8>, name: &str, data: &[u8], kind: u8) {
    let mut header = [0u8; TAR_BLOCK];
    let (path, prefix) = split_ustar_name(name).unwrap_or_else(|| {
        let bytes = name.as_bytes();
        (&bytes[..bytes.len().min(100)], &[][..])
    });
    header[..path.len()].copy_from_slice(path);
    header[345..345 + prefix.len()].copy_from_slice(prefix);
    put_tar_octal(&mut header[100..108], 0o644);
    put_tar_octal(&mut header[108..116], 0);
    put_tar_octal(&mut header[116..124], 0);
    put_tar_octal(&mut header[124..136], data.len() as u64);
    put_tar_octal(&mut header[136..148], 0);
    header[148..156].fill(b' ');
    header[156] = kind;
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    let checksum = header.iter().map(|byte| *byte as u64).sum();
    put_tar_checksum(&mut header[148..156], checksum);
    out.extend_from_slice(&header);
    out.extend_from_slice(data);
    out.resize(out.len().div_ceil(TAR_BLOCK) * TAR_BLOCK, 0);
}

fn split_ustar_name(name: &str) -> Option<(&[u8], &[u8])> {
    let bytes = name.as_bytes();
    if bytes.len() <= 100 {
        return Some((bytes, &[]));
    }
    bytes
        .iter()
        .enumerate()
        .rev()
        .find(|(index, byte)| **byte == b'/' && *index <= 155 && bytes.len() - index - 1 <= 100)
        .map(|(index, _)| (&bytes[index + 1..], &bytes[..index]))
}

fn tar_header_name(header: &[u8]) -> String {
    let name = trim_nul(&header[..100]);
    let prefix = trim_nul(&header[345..500]);
    if prefix.is_empty() { name } else { format!("{prefix}/{name}") }
}

fn trim_nul(bytes: &[u8]) -> String {
    String::from_utf8(
        bytes[..bytes.iter().position(|byte| *byte == 0).unwrap_or(bytes.len())].to_vec(),
    )
    .unwrap_or_default()
}

fn pax_path(payload: &[u8]) -> Option<String> {
    let mut offset = 0;
    while offset < payload.len() {
        let space = payload[offset..].iter().position(|byte| *byte == b' ')? + offset;
        let len = std::str::from_utf8(&payload[offset..space]).ok()?.parse::<usize>().ok()?;
        let end = offset.checked_add(len)?;
        let record = payload.get(space + 1..end.checked_sub(1)?)?;
        if let Some(path) = record.strip_prefix(b"path=") {
            return String::from_utf8(path.to_vec()).ok();
        }
        offset = end;
    }
    None
}

fn tar_checksum_ok(header: &[u8]) -> bool {
    let Some(stored) = tar_number(&header[148..156]) else {
        return false;
    };
    let actual = header
        .iter()
        .enumerate()
        .map(|(index, byte)| if (148..156).contains(&index) { b' ' } else { *byte })
        .map(u64::from)
        .sum::<u64>();
    stored == actual
}

fn tar_number(field: &[u8]) -> Option<u64> {
    if field.first().is_some_and(|byte| byte & 0x80 != 0) {
        let mut value = u64::from(field[0] & 0x7f);
        for byte in &field[1..] {
            value = value.checked_mul(256)?.checked_add(u64::from(*byte))?;
        }
        return Some(value);
    }
    let text = std::str::from_utf8(field).ok()?.trim_matches(['\0', ' ']);
    if text.is_empty() { Some(0) } else { u64::from_str_radix(text, 8).ok() }
}

fn put_tar_octal(field: &mut [u8], value: u64) {
    field.fill(b'0');
    field[field.len() - 1] = 0;
    let digits = format!("{value:o}");
    if digits.len() < field.len() {
        let start = field.len() - 1 - digits.len();
        field[start..start + digits.len()].copy_from_slice(digits.as_bytes());
    }
}

fn put_tar_checksum(field: &mut [u8], value: u64) {
    field.fill(b' ');
    let digits = format!("{value:06o}");
    field[..6].copy_from_slice(&digits.as_bytes()[digits.len().saturating_sub(6)..]);
    field[6] = 0;
}

fn find_eocd(data: &[u8]) -> Option<usize> {
    let start = data.len().saturating_sub(65_557);
    (start..=data.len().checked_sub(22)?)
        .rev()
        .find(|offset| {
            read_u32(data, *offset) == Some(0x0605_4b50)
                && read_u16(data, *offset + 20)
                    .is_some_and(|comment| *offset + 22 + usize::from(comment) == data.len())
        })
}

fn read_u16(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        data.get(offset..offset.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        data.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn read_le(data: &[u8], offset: usize, size: usize) -> Option<u64> {
    let bytes = data.get(offset..offset.checked_add(size)?)?;
    (size <= 8).then(|| {
        bytes
            .iter()
            .enumerate()
            .fold(0u64, |value, (shift, byte)| value | (u64::from(*byte) << (shift * 8)))
    })
}

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u24(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes()[..3]);
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

fn xxh64(data: &[u8]) -> u64 {
    const P1: u64 = 11_400_714_785_074_694_791;
    const P2: u64 = 14_029_467_366_897_019_727;
    const P3: u64 = 1_609_587_929_392_839_161;
    const P4: u64 = 9_650_029_242_287_828_579;
    const P5: u64 = 2_870_177_450_012_600_261;
    let round = |acc: u64, lane: u64| {
        acc.wrapping_add(lane.wrapping_mul(P2))
            .rotate_left(31)
            .wrapping_mul(P1)
    };
    let mut offset = 0usize;
    let mut hash = if data.len() >= 32 {
        let mut lanes = [P1.wrapping_add(P2), P2, 0, 0u64.wrapping_sub(P1)];
        while offset + 32 <= data.len() {
            for lane in &mut lanes {
                *lane = round(*lane, u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap()));
                offset += 8;
            }
        }
        let mut hash = lanes[0]
            .rotate_left(1)
            .wrapping_add(lanes[1].rotate_left(7))
            .wrapping_add(lanes[2].rotate_left(12))
            .wrapping_add(lanes[3].rotate_left(18));
        for lane in lanes {
            hash ^= round(0, lane);
            hash = hash.wrapping_mul(P1).wrapping_add(P4);
        }
        hash
    } else {
        P5
    };
    hash = hash.wrapping_add(data.len() as u64);
    while offset + 8 <= data.len() {
        hash ^= round(0, u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap()));
        hash = hash.rotate_left(27).wrapping_mul(P1).wrapping_add(P4);
        offset += 8;
    }
    if offset + 4 <= data.len() {
        hash ^= u64::from(u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()))
            .wrapping_mul(P1);
        hash = hash.rotate_left(23).wrapping_mul(P2).wrapping_add(P3);
        offset += 4;
    }
    for byte in &data[offset..] {
        hash ^= u64::from(*byte).wrapping_mul(P5);
        hash = hash.rotate_left(11).wrapping_mul(P1);
    }
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(P2);
    hash ^= hash >> 29;
    hash = hash.wrapping_mul(P3);
    hash ^ (hash >> 32)
}

struct Bits<'a> {
    data: &'a [u8],
    bit: usize,
}

impl Bits<'_> {
    fn read(&mut self, count: usize) -> Option<u32> {
        let mut value = 0u32;
        for shift in 0..count {
            let byte = *self.data.get(self.bit / 8)?;
            value |= u32::from((byte >> (self.bit % 8)) & 1) << shift;
            self.bit += 1;
        }
        Some(value)
    }

    fn align_byte(&mut self) {
        self.bit = self.bit.div_ceil(8) * 8;
    }
}

struct Huffman(Vec<(u32, usize, u16)>);

impl Huffman {
    fn new(lengths: &[u8]) -> Option<Self> {
        let max = usize::from(*lengths.iter().max()?);
        if max == 0 || max > 15 {
            return None;
        }
        let mut counts = vec![0u32; max + 1];
        for length in lengths.iter().copied().filter(|length| *length != 0) {
            counts[usize::from(length)] += 1;
        }
        let mut next = vec![0u32; max + 1];
        let mut code = 0u32;
        for bits in 1..=max {
            code = (code + counts[bits - 1]) << 1;
            if code + counts[bits] > 1 << bits {
                return None;
            }
            next[bits] = code;
        }
        let mut entries = Vec::new();
        for (symbol, length) in lengths.iter().copied().enumerate() {
            if length == 0 {
                continue;
            }
            let length = usize::from(length);
            let canonical = next[length];
            next[length] += 1;
            entries.push((reverse_bits(canonical, length), length, symbol as u16));
        }
        Some(Self(entries))
    }

    fn symbol(&self, bits: &mut Bits<'_>) -> Option<u16> {
        let mut code = 0u32;
        for length in 1..=15 {
            code |= bits.read(1)? << (length - 1);
            if let Some((_, _, symbol)) = self
                .0
                .iter()
                .find(|(candidate, candidate_len, _)| *candidate_len == length && *candidate == code)
            {
                return Some(*symbol);
            }
        }
        None
    }
}

fn reverse_bits(mut code: u32, count: usize) -> u32 {
    let mut reversed = 0;
    for _ in 0..count {
        reversed = (reversed << 1) | (code & 1);
        code >>= 1;
    }
    reversed
}

fn inflate(data: &[u8], expected_len: usize) -> Option<Vec<u8>> {
    inflate_with_consumed(data, expected_len).map(|(out, _)| out)
}

fn inflate_with_consumed(data: &[u8], expected_len: usize) -> Option<(Vec<u8>, usize)> {
    let mut bits = Bits { data, bit: 0 };
    let mut out = Vec::new();
    loop {
        let final_block = bits.read(1)? != 0;
        match bits.read(2)? {
            0 => {
                bits.align_byte();
                let len = bits.read(16)? as usize;
                if bits.read(16)? as u16 != !(len as u16) {
                    return None;
                }
                for _ in 0..len {
                    if out.len() == expected_len {
                        return None;
                    }
                    out.push(bits.read(8)? as u8);
                }
            }
            1 => {
                let (lit, dist) = fixed_trees()?;
                inflate_codes(&mut bits, &lit, &dist, &mut out, expected_len)?;
            }
            2 => {
                let (lit, dist) = dynamic_trees(&mut bits)?;
                inflate_codes(&mut bits, &lit, &dist, &mut out, expected_len)?;
            }
            _ => return None,
        }
        if final_block {
            return Some((out, bits.bit.div_ceil(8)));
        }
    }
}

fn fixed_trees() -> Option<(Huffman, Huffman)> {
    let mut literal = vec![8; 288];
    literal[144..256].fill(9);
    literal[256..280].fill(7);
    literal[280..].fill(8);
    Some((Huffman::new(&literal)?, Huffman::new(&[5; 32])?))
}

fn dynamic_trees(bits: &mut Bits<'_>) -> Option<(Huffman, Huffman)> {
    let literal_count = bits.read(5)? as usize + 257;
    let distance_count = bits.read(5)? as usize + 1;
    let code_count = bits.read(4)? as usize + 4;
    let order = [16usize, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
    let mut code_lengths = [0u8; 19];
    for index in 0..code_count {
        code_lengths[order[index]] = bits.read(3)? as u8;
    }
    let code_tree = Huffman::new(&code_lengths)?;
    let total = literal_count + distance_count;
    let mut lengths = Vec::with_capacity(total);
    while lengths.len() < total {
        match code_tree.symbol(bits)? {
            value @ 0..=15 => lengths.push(value as u8),
            16 => {
                let previous = *lengths.last()?;
                let count = bits.read(2)? as usize + 3;
                lengths.resize(lengths.len().checked_add(count)?, previous);
            }
            17 => {
                let count = bits.read(3)? as usize + 3;
                lengths.resize(lengths.len().checked_add(count)?, 0);
            }
            18 => {
                let count = bits.read(7)? as usize + 11;
                lengths.resize(lengths.len().checked_add(count)?, 0);
            }
            _ => return None,
        }
        if lengths.len() > total {
            return None;
        }
    }
    let literal = Huffman::new(&lengths[..literal_count])?;
    let distance = Huffman::new(&lengths[literal_count..]).or_else(|| Huffman::new(&[1]))?;
    Some((literal, distance))
}

fn inflate_codes(
    bits: &mut Bits<'_>,
    literal: &Huffman,
    distance: &Huffman,
    out: &mut Vec<u8>,
    expected_len: usize,
) -> Option<()> {
    const LENGTH_BASE: [usize; 29] = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83,
        99, 115, 131, 163, 195, 227, 258,
    ];
    const LENGTH_EXTRA: [usize; 29] = [
        0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4,
        5, 5, 5, 5, 0,
    ];
    const DIST_BASE: [usize; 30] = [
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769,
        1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
    ];
    const DIST_EXTRA: [usize; 30] = [
        0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10,
        11, 11, 12, 12, 13, 13,
    ];
    loop {
        match literal.symbol(bits)? {
            byte @ 0..=255 => {
                if out.len() == expected_len {
                    return None;
                }
                out.push(byte as u8);
            }
            256 => return Some(()),
            symbol @ 257..=285 => {
                let index = usize::from(symbol - 257);
                let length = LENGTH_BASE[index] + bits.read(LENGTH_EXTRA[index])? as usize;
                let distance_symbol = usize::from(distance.symbol(bits)?);
                let base = *DIST_BASE.get(distance_symbol)?;
                let extra = *DIST_EXTRA.get(distance_symbol)?;
                let distance = base + bits.read(extra)? as usize;
                if distance == 0 || distance > out.len() {
                    return None;
                }
                if out.len().checked_add(length)? > expected_len {
                    return None;
                }
                for _ in 0..length {
                    out.push(out[out.len() - distance]);
                }
            }
            _ => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stock_zstd(plain: &[u8]) -> Vec<u8> {
        stock_zstd_with(plain, &["--no-check"])
    }

    fn stock_zstd_with(plain: &[u8], args: &[&str]) -> Vec<u8> {
        use std::io::Write as _;
        use std::process::{Command, Stdio};

        let mut child = Command::new("zstd")
            .args(["-q", "-c"])
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("zstd must be available in the Jet test environment");
        child.stdin.take().unwrap().write_all(plain).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        output.stdout
    }

    #[test]
    fn stored_zip_and_tar_round_trip() {
        let zip = zip_compress("hello.txt", b"hello");
        assert_eq!(&zip[..4], b"PK\x03\x04");
        assert_eq!(zip_decompress(&zip), b"hello");

        let tar = tar_add(&[], "hello.txt", b"hello");
        assert_eq!(&tar[257..263], b"ustar\0");
        assert_eq!(tar_get(&tar, "hello.txt"), b"hello");
        assert_eq!(tar_names_json(&tar), "[\"hello.txt\"]");
    }

    #[test]
    fn tar_rejects_invalid_names_without_dropping_valid_entries() {
        for name in ["", "../x", "/x"] {
            let valid = tar_add(&[], "keep.txt", b"keep");
            let tar = tar_add(&valid, name, b"invalid");
            assert_eq!(tar_names_json(&tar), "[\"keep.txt\"]");
            assert_eq!(tar_get(&tar, "keep.txt"), b"keep");
            assert_eq!(tar_get(&tar, name), b"");
        }
    }

    #[test]
    fn fixed_huffman_deflate_decodes() {
        assert_eq!(
            inflate(&[0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x07, 0x00], 5),
            Some(b"hello".to_vec())
        );
    }

    #[test]
    fn dynamic_huffman_deflate_decodes() {
        let compressed = [
            173, 140, 87, 21, 128, 48, 12, 69, 173, 60, 5, 24, 64, 77, 11, 233, 96, 52, 221,
            5, 212, 147, 131, 6, 190, 239, 168, 142, 144, 154, 95, 118, 232, 204, 35, 192,
            240, 133, 173, 157, 177, 128, 59, 101, 84, 193, 135, 122, 110, 172, 108, 103, 68,
            37, 222, 121, 67, 139, 52, 124, 117, 48, 190, 147, 160, 135, 2, 14, 159, 26, 103,
            105, 109, 153, 190, 236, 255, 235, 11,
        ];
        let expected = b"the quick brown fox jumps over the lazy dog; pack my box with five dozen liquor jugs. "
            .repeat(2);
        assert_eq!(inflate(&compressed, expected.len()), Some(expected));
    }

    #[test]
    fn gzip_round_trip_with_bounded_frames() {
        for data in [&[][..], b"hello", &vec![42; 200_000]] {
            assert_eq!(gzip_decompress(&gzip_compress(data)), Ok(data.to_vec()));
        }
    }

    #[test]
    fn python_gzip_golden_decodes() {
        // Python 3: gzip.compress(b"hello", mtime=0). Not produced by this
        // encoder, and its fixed-Huffman payload exercises a different path.
        let gzip = [
            31, 139, 8, 0, 0, 0, 0, 0, 2, 3, 203, 72, 205, 201, 201, 7, 0, 134, 166, 16,
            54, 5, 0, 0, 0,
        ];
        assert_eq!(gzip_decompress(&gzip), Ok(b"hello".to_vec()));
    }

    #[test]
    fn gzip_rejects_truncation_corruption_and_oversized_frames() {
        let mut gzip = gzip_compress(b"hello");
        gzip[14] ^= 1;
        assert!(gzip_decompress(&gzip).is_err());
        assert!(gzip_decompress(&[31, 139, 8]).is_err());
        let mut trailing = gzip_compress(b"hello");
        trailing.insert(trailing.len() - 8, 0);
        assert!(gzip_decompress(&trailing).is_err());
        let mut wrong_size = gzip_compress(b"hello");
        let trailer = wrong_size.len() - 4;
        wrong_size[trailer..].copy_from_slice(&6u32.to_le_bytes());
        assert!(gzip_decompress(&wrong_size).is_err());
        let mut with_hcrc = gzip_compress(b"hello");
        with_hcrc[3] |= 2;
        let hcrc = (crc32(&with_hcrc[..10]) as u16).to_le_bytes();
        with_hcrc.splice(10..10, hcrc);
        assert_eq!(gzip_decompress(&with_hcrc), Ok(b"hello".to_vec()));
        with_hcrc[10] ^= 1;
        assert!(gzip_decompress(&with_hcrc).is_err());
    }

    #[test]
    fn zstd_encoder_uses_standard_raw_frame_layout() {
        assert_eq!(
            zstd_compress(b"hello"),
            [40, 181, 47, 253, 0, 56, 41, 0, 0, 104, 101, 108, 108, 111]
        );

        let frame = zstd_compress(&vec![42; ZSTD_BLOCK_MAX + 1]);
        assert_eq!(&frame[..6], &[40, 181, 47, 253, 128, 56]);
        assert_eq!(
            &frame[6..10],
            &((ZSTD_BLOCK_MAX + 1) as u32).to_le_bytes()
        );
        assert_eq!(&frame[10..13], &[0, 0, 16]); // non-final 128 KiB raw block
        let second = 13 + ZSTD_BLOCK_MAX;
        assert_eq!(&frame[second..second + 3], &[9, 0, 0]); // final one-byte raw block
    }

    #[test]
    fn zstd_frame_header_boundaries_keep_a_bounded_window() {
        let header = |len| {
            let mut out = Vec::new();
            zstd_frame_header(&mut out, len);
            out
        };
        assert_eq!(header(0), [40, 181, 47, 253, 0, 56]);
        assert_eq!(header(255), [40, 181, 47, 253, 0, 56]);
        assert_eq!(header(256), [40, 181, 47, 253, 64, 56, 0, 0]);
        assert_eq!(header(65_791), [40, 181, 47, 253, 64, 56, 255, 255]);
        assert_eq!(header(65_792), [40, 181, 47, 253, 128, 56, 0, 1, 1, 0]);
        assert_eq!(
            header(u64::from(u32::MAX)),
            [40, 181, 47, 253, 128, 56, 255, 255, 255, 255]
        );
        assert_eq!(
            header(u64::from(u32::MAX) + 1),
            [40, 181, 47, 253, 192, 56, 0, 0, 0, 0, 1, 0, 0, 0]
        );
    }

    #[test]
    fn zstd_default_decoder_accepts_frame_larger_than_128_mib() {
        use std::io::Write as _;
        use std::process::{Command, Stdio};

        let frame = zstd_compress(&vec![42; 128 * 1024 * 1024 + 1]);
        let mut child = Command::new("zstd")
            .args(["-q", "-t"])
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("zstd must be available in the Jet test environment");
        let written = child.stdin.take().unwrap().write_all(&frame);
        let output = child.wait_with_output().unwrap();
        assert!(
            written.is_ok() && output.status.success(),
            "default zstd decoder rejected bounded-window frame: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn zstd_raw_rle_skippable_and_concatenated_frames_decode() {
        assert_eq!(zstd_decompress(&zstd_compress(b"hello")), Ok(b"hello".to_vec()));

        let rle = [40, 181, 47, 253, 32, 10, 83, 0, 0, b'a'];
        assert_eq!(zstd_decompress(&rle), Ok(vec![b'a'; 10]));

        // zstd 1.5.7: `printf hello | zstd --no-check -c`. Unlike our encoder,
        // this independently produced frame carries a window descriptor.
        let external = [40, 181, 47, 253, 0, 88, 41, 0, 0, 104, 101, 108, 108, 111];
        assert_eq!(zstd_decompress(&external), Ok(b"hello".to_vec()));

        let mut stream = [80, 42, 77, 24, 3, 0, 0, 0, b'a', b'b', b'c'].to_vec();
        stream.extend(zstd_compress(b"one"));
        stream.extend(zstd_compress(b"two"));
        assert_eq!(zstd_decompress(&stream), Ok(b"onetwo".to_vec()));
    }

    #[test]
    fn zstd_raw_decoder_checks_headers_sizes_and_checksum() {
        // zstd 1.5.7's independently produced checksum-bearing `hello` frame.
        let checksummed = [
            40, 181, 47, 253, 4, 88, 41, 0, 0, 104, 101, 108, 108, 111, 163, 109, 159, 136,
        ];
        assert_eq!(zstd_decompress(&checksummed), Ok(b"hello".to_vec()));
        let mut corrupt = checksummed;
        corrupt[14] ^= 1;
        assert!(zstd_decompress(&corrupt).unwrap_err().contains("checksum mismatch"));

        let wrong_size = [40, 181, 47, 253, 32, 6, 41, 0, 0, 104, 101, 108, 108, 111];
        assert!(zstd_decompress(&wrong_size)
            .unwrap_err()
            .contains("content size mismatch"));
        let dictionary = [40, 181, 47, 253, 33, 1, 1, 1, 0, 0];
        assert!(zstd_decompress(&dictionary)
            .unwrap_err()
            .contains("dictionary frames are unsupported"));
        let oversized = [40, 181, 47, 253, 160, 1, 0, 0, 4];
        assert!(zstd_decompress(&oversized).unwrap_err().contains("64 MiB"));
        let max_window = [40, 181, 47, 253, 0, 136, 41, 0, 0, 104, 101, 108, 108, 111];
        assert_eq!(zstd_decompress(&max_window), Ok(b"hello".to_vec()));
        let oversized_window =
            [40, 181, 47, 253, 0, 137, 41, 0, 0, 104, 101, 108, 108, 111];
        assert!(zstd_decompress(&oversized_window)
            .unwrap_err()
            .contains("128 MiB"));
        assert!(zstd_decompress(&[]).is_err());
        assert!(zstd_decompress(&[40, 181, 47]).is_err());
    }

    #[test]
    fn zstd_stock_compressed_sequence_golden_decodes() {
        // zstd 1.5.7 compressed-block frame for a repeated pangram.
        let compressed = [
            40, 181, 47, 253, 0, 88, 181, 1, 0, 180, 2, 116, 104, 101, 32, 113, 117, 105,
            99, 107, 32, 98, 114, 111, 119, 110, 32, 102, 111, 120, 32, 106, 117, 109, 112,
            115, 32, 111, 118, 101, 114, 32, 116, 104, 101, 32, 108, 97, 122, 121, 32, 100,
            111, 103, 2, 0, 253, 169, 4, 6, 194, 44, 3,
        ];
        assert_eq!(
            zstd_decompress(&compressed),
            Ok(b"the quick brown fox jumps over the lazy dog ".repeat(4))
        );
    }

    #[test]
    fn zstd_stock_multiblock_sequences_use_prior_output() {
        let mut value = 1u32;
        let base = (0..120_000)
            .map(|_| {
                value ^= value << 13;
                value ^= value >> 17;
                value ^= value << 5;
                value as u8
            })
            .collect::<Vec<_>>();
        let plain = base.repeat(3);
        let frame = stock_zstd(&plain);
        assert_eq!(zstd_decompress(&frame), Ok(plain));

        let mut value = 1u32;
        let plain = (0..300_000)
            .map(|_| {
                value = value.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                b'A' + (value % 3) as u8
            })
            .collect::<Vec<_>>();
        let frame = stock_zstd(&plain);
        assert_eq!(zstd_decompress(&frame), Ok(plain));

        let mut value = 1u32;
        let base = (0..ZSTD_BLOCK_MAX)
            .map(|_| {
                value = value.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                (value % 3) as u8
            })
            .collect::<Vec<_>>();
        let mut plain = Vec::with_capacity(ZSTD_BLOCK_MAX * 3);
        for shift in [0, 4, 8] {
            plain.extend(base.iter().map(|byte| byte + shift));
        }
        let frame = stock_zstd_with(&plain, &["--no-check", "-19"]);
        assert_eq!(zstd_decompress(&frame), Ok(plain));
    }

    #[test]
    fn zstd_stock_levels_checksums_fcs_and_concatenation_decode() {
        let corpora = [
            Vec::new(),
            b"z".to_vec(),
            b"the quick brown fox jumps over the lazy dog ".repeat(200),
            (0..20_000).map(|index| (index * 37) as u8).collect::<Vec<_>>(),
        ];
        for plain in corpora {
            for level in ["-1", "-5", "-19"] {
                let size = format!("--stream-size={}", plain.len());
                let frame = stock_zstd_with(&plain, &[level, &size]);
                assert_eq!(zstd_decompress(&frame), Ok(plain.clone()), "level {level}");
            }
        }

        let one = b"checked compressed frame".repeat(200);
        let two = b"second checked frame".repeat(300);
        let size_one = format!("--stream-size={}", one.len());
        let size_two = format!("--stream-size={}", two.len());
        let mut stream = stock_zstd_with(&one, &["-5", &size_one]);
        stream.extend([80, 42, 77, 24, 3, 0, 0, 0, b'x', b'y', b'z']);
        stream.extend(stock_zstd_with(&two, &["-19", &size_two]));
        assert_eq!(zstd_decompress(&stream), Ok([one, two].concat()));
    }

    #[test]
    fn zstd_mutated_compressed_frames_are_bounded_and_never_panic() {
        let plain = b"mutation probe mutation probe mutation probe".repeat(4);
        let frame = stock_zstd_with(&plain, &["-5"]);
        for end in 0..frame.len() {
            assert!(std::panic::catch_unwind(|| zstd_decompress(&frame[..end])).is_ok());
        }
        for index in 0..frame.len() {
            for bit in 0..8 {
                let mut mutated = frame.clone();
                mutated[index] ^= 1 << bit;
                assert!(std::panic::catch_unwind(|| zstd_decompress(&mutated)).is_ok());
            }
        }
    }
}
