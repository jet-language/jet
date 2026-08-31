// core.archive's single audited ABI kernel (D-CORE-COMPRESS1=A, D-BFS1).
//
// This file is the dependency-free audited ABI kernel for the ordinary-Jet
// package in archive.jet. It has no filesystem, process, or host-tool access.
// Public calls reach this file only through the package source's internal ABI
// boundary; it is not a compiler template or an engine-specific public path.

use std::path::{Component, Path};

pub const JET_CORE_ARCHIVE_ABI_VERSION: &str = "core.archive.abi.v2";

const TAR_BLOCK: usize = 512;
const MAX_OUTPUT: usize = 64 * 1024 * 1024;
const MAX_ENTRIES: usize = 4096;
const ZIP_READER_STATE: &[u8] = b"JZR1";
const ZIP_WRITER_STATE: &[u8] = b"JZW1";

#[derive(Default)]
struct ArchiveAllocationBudget {
    used: usize,
}

impl ArchiveAllocationBudget {
    fn reserve(&mut self, bytes: usize) -> bool {
        let Some(used) = self.used.checked_add(bytes) else {
            return false;
        };
        if used > MAX_OUTPUT {
            return false;
        }
        self.used = used;
        true
    }

    fn release(&mut self, bytes: usize) {
        self.used = self.used.saturating_sub(bytes);
    }
}

fn replace_archive_entry(
    entries: &mut [(String, Vec<u8>)],
    index: usize,
    name: &str,
    data: &[u8],
) {
    let old_name = std::mem::take(&mut entries[index].0);
    drop(old_name);
    let old_data = std::mem::take(&mut entries[index].1);
    drop(old_data);
    entries[index] = (name.to_owned(), data.to_vec());
}

pub fn jet_archive_zip_compress(name: &str, data: &[u8]) -> Vec<u8> {
    let Some((local_size, central_size, _)) = zip_archive_size(std::iter::once((name, data)))
    else {
        return Vec::new();
    };
    let Some(size) = local_size
        .checked_add(central_size)
        .and_then(|size| size.checked_add(22))
    else {
        return Vec::new();
    };
    if size > MAX_OUTPUT {
        return Vec::new();
    }
    zip_write_all(&[(name.to_string(), data.to_vec())])
}

pub fn jet_archive_zip_decompress(data: &[u8]) -> Vec<u8> {
    zip_read_all(data)
        .and_then(|entries| entries.into_iter().next().map(|(_, data)| data))
        .unwrap_or_default()
}

pub fn jet_archive_crc32(data: &[u8]) -> i64 {
    i64::from(crc32(data))
}

pub fn jet_archive_adler32(data: &[u8]) -> i64 {
    let mut a = 1u32;
    let mut b = 0u32;
    for chunk in data.chunks(5_552) {
        for byte in chunk {
            a = (a + u32::from(*byte)) % 65_521;
            b = (b + a) % 65_521;
        }
    }
    i64::from((b << 16) | a)
}

pub fn jet_archive_deflate(data: &[u8]) -> Vec<u8> {
    if data.len() > MAX_OUTPUT || deflate_fixed_len(data).is_none_or(|size| size > MAX_OUTPUT) {
        return Vec::new();
    }
    deflate_fixed(data)
}

pub fn jet_archive_inflate(data: &[u8]) -> Vec<u8> {
    inflate_any(data).unwrap_or_default()
}

pub fn jet_archive_zip_names_json(data: &[u8]) -> String {
    let entries = zip_read_all(data).unwrap_or_default();
    names_json(entries.iter().map(|(name, _)| name.as_str()).collect()).unwrap_or_default()
}

pub fn jet_archive_zip_open(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        let mut state = ZIP_WRITER_STATE.to_vec();
        state.extend_from_slice(&zip_write_all(&[]));
        return state;
    }
    if zip_read_all(data).is_some() {
        let mut state = ZIP_READER_STATE.to_vec();
        state.extend_from_slice(data);
        state
    } else {
        Vec::new()
    }
}

pub fn jet_archive_zip_next(reader: &[u8], index: i64) -> String {
    let Ok(index) = usize::try_from(index) else {
        return String::new();
    };
    zip_reader_entries(reader)
        .and_then(|entries| entries.into_iter().nth(index).map(|(name, _)| name))
        .unwrap_or_default()
}

pub fn jet_archive_zip_read(reader: &[u8], name: &str) -> Vec<u8> {
    zip_reader_entries(reader)
        .and_then(|entries| entries.into_iter().find(|(entry, _)| entry == name).map(|(_, data)| data))
        .unwrap_or_default()
}

pub fn jet_archive_zip_write(writer: &[u8], name: &str, data: &[u8]) -> Vec<u8> {
    if !zip_name_valid(name) || data.len() > MAX_OUTPUT || u16::try_from(name.len()).is_err() {
        return Vec::new();
    }
    let mut entries = zip_writer_entries(writer).unwrap_or_default();
    let replacement = entries.iter().position(|(entry, _)| entry == name);
    if replacement.is_none() && entries.len() >= MAX_ENTRIES {
        return Vec::new();
    }
    let (local_size, central_size, _) = if let Some(index) = replacement {
        let Some(sizes) = zip_archive_size(entries.iter().enumerate().map(|(entry_index, (entry, payload))| {
            if entry_index == index {
                (name, data)
            } else {
                (entry.as_str(), payload.as_slice())
            }
        })) else {
            return Vec::new();
        };
        sizes
    } else {
        let Some(sizes) = zip_archive_size(
            entries
                .iter()
                .map(|(entry, payload)| (entry.as_str(), payload.as_slice()))
                .chain(std::iter::once((name, data))),
        ) else {
            return Vec::new();
        };
        sizes
    };
    let Some(size) = local_size
        .checked_add(central_size)
        .and_then(|size| size.checked_add(22))
    else {
        return Vec::new();
    };
    if size > MAX_OUTPUT {
        return Vec::new();
    }
    if let Some(index) = replacement {
        replace_archive_entry(&mut entries, index, name, data);
    } else {
        entries.push((name.to_string(), data.to_vec()));
    }
    let mut state = ZIP_WRITER_STATE.to_vec();
    state.extend_from_slice(&zip_write_all(&entries));
    state
}

pub fn jet_archive_zip_close(writer: &[u8]) -> Vec<u8> {
    zip_writer_entries(writer).map(|entries| zip_write_all(&entries)).unwrap_or_default()
}

pub fn jet_archive_zip_extract(data: &[u8], name: &str) -> Vec<u8> {
    jet_archive_zip_read(&jet_archive_zip_open(data), name)
}

pub fn jet_archive_unzip(data: &[u8], name: &str) -> Vec<u8> {
    jet_archive_zip_extract(data, name)
}

fn zip_read_all(data: &[u8]) -> Option<Vec<(String, Vec<u8>)>> {
    let eocd = find_eocd(data)?;
    let count = usize::from(read_u16_at(data, eocd, 10)?);
    let central_size = usize::try_from(read_u32_at(data, eocd, 12)?).ok()?;
    let central = usize::try_from(read_u32_at(data, eocd, 16)?).ok()?;
    let central_end = central.checked_add(central_size)?;
    if central_end > eocd || read_u16_at(data, eocd, 4)? != 0 || read_u16_at(data, eocd, 6)? != 0 {
        return None;
    }
    if count > MAX_ENTRIES {
        return None;
    }
    let mut entries = Vec::with_capacity(count);
    let mut offset = central;
    let mut budget = ArchiveAllocationBudget::default();
    for _ in 0..count {
        let fixed = data.get(offset..offset.checked_add(46)?)?;
        if read_u32(fixed, 0)? != 0x0201_4b50 {
            return None;
        }
        if read_u16(fixed, 6)? != 20 || read_u16(fixed, 8)? != 0 {
            return None;
        }
        let method = read_u16(fixed, 10)?;
        if method != 0 && method != 8 {
            return None;
        }
        let crc = read_u32(fixed, 16)?;
        let compressed_len = usize::try_from(read_u32(fixed, 20)?).ok()?;
        let expected_len = usize::try_from(read_u32(fixed, 24)?).ok()?;
        if expected_len > MAX_OUTPUT {
            return None;
        }
        let name_len = usize::from(read_u16(fixed, 28)?);
        let extra_len = usize::from(read_u16(fixed, 30)?);
        let comment_len = usize::from(read_u16(fixed, 32)?);
        let name_start = offset.checked_add(46)?;
        let name_end = name_start.checked_add(name_len)?;
        let extra_end = name_end.checked_add(extra_len)?;
        let record_end = extra_end.checked_add(comment_len)?;
        if record_end > central_end {
            return None;
        }
        let name_bytes = data.get(name_start..name_end)?;
        let name_text = std::str::from_utf8(name_bytes).ok()?;
        if !zip_name_valid(name_text) {
            return None;
        }
        let local = usize::try_from(read_u32(fixed, 42)?).ok()?;
        let local_fixed = data.get(local..local.checked_add(30)?)?;
        if read_u32(local_fixed, 0)? != 0x0403_4b50
            || read_u16(local_fixed, 6)? != 0
            || read_u16(local_fixed, 8)? != method
        {
            return None;
        }
        let local_name_len = usize::from(read_u16(local_fixed, 26)?);
        let local_extra_len = usize::from(read_u16(local_fixed, 28)?);
        let payload_start = local
            .checked_add(30)?
            .checked_add(local_name_len)?
            .checked_add(local_extra_len)?;
        let payload_end = payload_start.checked_add(compressed_len)?;
        let payload = data.get(payload_start..payload_end)?;
        let name_and_data = name_len.checked_add(expected_len)?;
        if !budget.reserve(name_and_data) {
            return None;
        }
        let name = name_text.to_owned();
        let bytes = match method {
            0 => (compressed_len == expected_len).then(|| payload.to_vec())?,
            8 => inflate(payload, expected_len)?,
            _ => return None,
        };
        if bytes.len() != expected_len || crc32(&bytes) != crc {
            return None;
        }
        entries.push((name, bytes));
        offset = record_end;
    }
    (offset == central_end).then_some(entries)
}

fn zip_write_all(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    if entries.len() > MAX_ENTRIES {
        return Vec::new();
    }
    let Some((local_size, central_size, _)) = zip_archive_size(
        entries
            .iter()
            .map(|(name, data)| (name.as_str(), data.as_slice())),
    ) else {
        return Vec::new();
    };
    let Some(final_size) = local_size
        .checked_add(central_size)
        .and_then(|size| size.checked_add(22))
    else {
        return Vec::new();
    };
    if final_size > MAX_OUTPUT {
        return Vec::new();
    }
    let Ok(count) = u16::try_from(entries.len()) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(final_size);
    for (name, data) in entries {
        let Ok(name_len) = u16::try_from(name.len()) else {
            return Vec::new();
        };
        let Some(compressed_len) = deflate_fixed_len(data) else {
            return Vec::new();
        };
        let Ok(compressed_len_u32) = u32::try_from(compressed_len) else {
            return Vec::new();
        };
        let Ok(size) = u32::try_from(data.len()) else {
            return Vec::new();
        };
        put_u32(&mut out, 0x0403_4b50);
        put_u16(&mut out, 20);
        put_u16(&mut out, 0);
        put_u16(&mut out, 8);
        put_u16(&mut out, 0);
        put_u16(&mut out, 0);
        put_u32(&mut out, crc32(data));
        put_u32(&mut out, compressed_len_u32);
        put_u32(&mut out, size);
        put_u16(&mut out, name_len);
        put_u16(&mut out, 0);
        out.extend_from_slice(name.as_bytes());
        let compressed_start = out.len();
        deflate_fixed_into(data, &mut out);
        debug_assert_eq!(out.len() - compressed_start, compressed_len);
    }
    let Ok(central_offset) = u32::try_from(out.len()) else {
        return Vec::new();
    };
    let Ok(central_size) = u32::try_from(central_size) else {
        return Vec::new();
    };
    let mut local_offset = 0usize;
    for (name, data) in entries {
        let Ok(name_len) = u16::try_from(name.len()) else {
            return Vec::new();
        };
        let Some(compressed_len) = deflate_fixed_len(data) else {
            return Vec::new();
        };
        let Ok(compressed_len_u32) = u32::try_from(compressed_len) else {
            return Vec::new();
        };
        let Ok(size) = u32::try_from(data.len()) else {
            return Vec::new();
        };
        let Ok(local_offset_u32) = u32::try_from(local_offset) else {
            return Vec::new();
        };
        put_u32(&mut out, 0x0201_4b50);
        put_u16(&mut out, 20);
        put_u16(&mut out, 20);
        put_u16(&mut out, 0);
        put_u16(&mut out, 8);
        put_u16(&mut out, 0);
        put_u16(&mut out, 0);
        put_u32(&mut out, crc32(data));
        put_u32(&mut out, compressed_len_u32);
        put_u32(&mut out, size);
        put_u16(&mut out, name_len);
        put_u16(&mut out, 0);
        put_u16(&mut out, 0);
        put_u16(&mut out, 0);
        put_u16(&mut out, 0);
        put_u32(&mut out, 0);
        put_u32(&mut out, local_offset_u32);
        out.extend_from_slice(name.as_bytes());
        let Some((local_len, _)) = zip_entry_wire_lengths(name, data) else {
            return Vec::new();
        };
        local_offset = match local_offset.checked_add(local_len) {
            Some(offset) => offset,
            None => return Vec::new(),
        };
    }
    put_u32(&mut out, 0x0605_4b50);
    put_u16(&mut out, 0);
    put_u16(&mut out, 0);
    put_u16(&mut out, count);
    put_u16(&mut out, count);
    put_u32(&mut out, central_size);
    put_u32(&mut out, central_offset);
    put_u16(&mut out, 0);
    out
}

fn zip_reader_entries(state: &[u8]) -> Option<Vec<(String, Vec<u8>)>> {
    state
        .strip_prefix(ZIP_READER_STATE)
        .and_then(zip_read_all)
}

fn zip_writer_entries(state: &[u8]) -> Option<Vec<(String, Vec<u8>)>> {
    state
        .strip_prefix(ZIP_WRITER_STATE)
        .and_then(zip_read_all)
}

fn zip_name_valid(name: &str) -> bool {
    !name.is_empty()
        && !name.as_bytes().contains(&0)
        && !name.contains('\\')
        && !Path::new(name).components().any(|component| {
            matches!(
                component,
                Component::Prefix(_) | Component::RootDir | Component::ParentDir
            )
        })
}

fn zip_entry_wire_lengths(name: &str, data: &[u8]) -> Option<(usize, usize)> {
    let name_len = usize::from(u16::try_from(name.len()).ok()?);
    let compressed_len = deflate_fixed_len(data)?;
    let local_len = 30usize
        .checked_add(name_len)?
        .checked_add(compressed_len)?;
    let central_len = 46usize
        .checked_add(name_len)?;
    Some((local_len, central_len))
}

fn zip_archive_size<'a, I>(entries: I) -> Option<(usize, usize, usize)>
where
    I: IntoIterator<Item = (&'a str, &'a [u8])>,
{
    let mut local_size = 0usize;
    let mut central_size = 0usize;
    let mut budget = ArchiveAllocationBudget::default();
    for (name, data) in entries {
        if !zip_name_valid(name) || data.len() > MAX_OUTPUT {
            return None;
        }
        let materialized = name.len().checked_add(data.len())?;
        if !budget.reserve(materialized) {
            return None;
        }
        let (local_len, central_len) = zip_entry_wire_lengths(name, data)?;
        local_size = local_size.checked_add(local_len)?;
        central_size = central_size.checked_add(central_len)?;
    }
    Some((local_size, central_size, budget.used))
}

fn names_json(names: Vec<&str>) -> Option<String> {
    let mut size = 2usize;
    for (index, name) in names.iter().enumerate() {
        let name_size = name.chars().try_fold(2usize, |size, ch| {
            let escaped = match ch {
                '"' | '\\' | '\u{0008}' | '\u{000c}' | '\n' | '\r' | '\t' => 2,
                ch if (ch as u32) < 0x20 => 6,
                ch => ch.len_utf8(),
            };
            size.checked_add(escaped)
        })?;
        size = size
            .checked_add(if index > 0 { 1 } else { 0 })?
            .checked_add(name_size)?;
        if size > MAX_OUTPUT {
            return None;
        }
    }
    let mut out = String::with_capacity(size);
    out.push('[');
    for (index, name) in names.into_iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('"');
        for ch in name.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\u{0008}' => out.push_str("\\b"),
                '\u{000c}' => out.push_str("\\f"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
                ch => out.push(ch),
            }
        }
        out.push('"');
    }
    out.push(']');
    Some(out)
}

struct DeflateBits<'a> {
    out: &'a mut Vec<u8>,
    bit: u8,
}

impl DeflateBits<'_> {
    fn write(&mut self, mut value: u32, count: u8) {
        for _ in 0..count {
            if self.bit == 0 {
                self.out.push(0);
            }
            if value & 1 != 0 {
                let last = self.out.len() - 1;
                self.out[last] |= 1 << self.bit;
            }
            value >>= 1;
            self.bit = (self.bit + 1) % 8;
        }
    }

}

fn deflate_fixed(data: &[u8]) -> Vec<u8> {
    let Some(size) = deflate_fixed_len(data) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(size);
    deflate_fixed_into(data, &mut out);
    out
}

fn deflate_fixed_into(data: &[u8], out: &mut Vec<u8>) {
    let mut bits = DeflateBits { out, bit: 0 };
    bits.write(1, 1);
    bits.write(1, 2);
    for byte in data {
        let (code, count) = fixed_code(usize::from(*byte));
        bits.write(code, count);
    }
    let (code, count) = fixed_code(256);
    bits.write(code, count);
}

fn deflate_fixed_len(data: &[u8]) -> Option<usize> {
    let mut bits = 3usize;
    for byte in data {
        bits = bits.checked_add(usize::from(fixed_code(usize::from(*byte)).1))?;
    }
    bits = bits.checked_add(usize::from(fixed_code(256).1))?;
    bits.checked_add(7).map(|bits| bits / 8)
}

fn fixed_code(symbol: usize) -> (u32, u8) {
    let (code, count) = match symbol {
        0..=143 => (symbol as u32 + 0x30, 8),
        144..=255 => (symbol as u32 - 144 + 0x190, 9),
        256..=279 => (symbol as u32 - 256, 7),
        _ => (symbol as u32 - 280 + 0xc0, 8),
    };
    (reverse_bits(code, count), count as u8)
}

pub fn jet_archive_tar_add(archive: &[u8], name: &str, data: &[u8]) -> Vec<u8> {
    if data.len() > MAX_OUTPUT {
        return Vec::new();
    }
    let mut entries = tar_read_all(archive);
    if !tar_name_valid(name) {
        return tar_write_all(&entries);
    }
    let replacement = entries.iter().position(|(entry, _)| entry == name);
    if replacement.is_none() && entries.len() >= MAX_ENTRIES {
        return Vec::new();
    }
    let mut budget = ArchiveAllocationBudget::default();
    for (entry_index, (entry, payload)) in entries.iter().enumerate() {
        let (entry_name, entry_data) = if Some(entry_index) == replacement {
            (name, data)
        } else {
            (entry.as_str(), payload.as_slice())
        };
        let Some(materialized) = entry_name.len().checked_add(entry_data.len()) else {
            return Vec::new();
        };
        if !budget.reserve(materialized) {
            return Vec::new();
        }
    }
    if replacement.is_none() {
        let Some(materialized) = name.len().checked_add(data.len()) else {
            return Vec::new();
        };
        if !budget.reserve(materialized) {
            return Vec::new();
        }
    }
    if let Some(index) = replacement {
        replace_archive_entry(&mut entries, index, name, data);
    } else {
        entries.push((name.to_string(), data.to_vec()));
    }
    tar_write_all(&entries)
}

pub fn jet_archive_tar_get(archive: &[u8], name: &str) -> Vec<u8> {
    tar_read_all(archive)
        .into_iter()
        .find_map(|(entry, data)| (entry == name).then_some(data))
        .unwrap_or_default()
}

pub fn jet_archive_tar_names_json(archive: &[u8]) -> String {
    let entries = tar_read_all(archive);
    names_json(entries.iter().map(|(name, _)| name.as_str()).collect()).unwrap_or_default()
}

fn tar_read_all(data: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    let mut offset = 0usize;
    let mut long_name = None;
    let mut budget = ArchiveAllocationBudget::default();
    let mut entries = 0usize;
    let mut terminated = false;
    while offset < data.len() {
        let Some(header_end) = offset.checked_add(TAR_BLOCK) else {
            return Vec::new();
        };
        let Some(header) = data.get(offset..header_end) else {
            return Vec::new();
        };
        if header.iter().all(|byte| *byte == 0) {
            if !data[offset..].iter().all(|byte| *byte == 0) {
                return Vec::new();
            }
            terminated = true;
            break;
        }
        if !tar_checksum_ok(header) {
            return Vec::new();
        }
        entries = match entries.checked_add(1) {
            Some(entries) if entries <= MAX_ENTRIES => entries,
            _ => return Vec::new(),
        };
        let Some(size) = tar_number(&header[124..136]).and_then(|n| usize::try_from(n).ok()) else {
            return Vec::new();
        };
        if size > MAX_OUTPUT {
            return Vec::new();
        }
        let Some(payload_start) = offset.checked_add(TAR_BLOCK) else {
            return Vec::new();
        };
        let Some(payload_end) = payload_start.checked_add(size) else {
            return Vec::new();
        };
        let Some(payload) = data.get(payload_start..payload_end) else {
            return Vec::new();
        };
        let kind = header[156];
        if kind == b'L' {
            let name = std::str::from_utf8(trim_nul(payload)).unwrap_or("");
            if !set_tar_long_name(&mut long_name, &mut budget, name) {
                return Vec::new();
            }
        } else if kind == b'x' {
            let name = pax_path(payload).unwrap_or("");
            if !set_tar_long_name(&mut long_name, &mut budget, name) {
                return Vec::new();
            }
        } else if kind != b'g' {
            let name = match long_name.take() {
                Some(name) => name,
                None => match tar_header_name(header, &mut budget) {
                    Some(name) => name,
                    None => return Vec::new(),
                },
            };
            if !tar_name_valid(&name) {
                return Vec::new();
            }
            if !budget.reserve(payload.len()) {
                return Vec::new();
            }
            out.push((name, payload.to_vec()));
        }
        let Some(padded) = size
            .checked_add(TAR_BLOCK - 1)
            .and_then(|size| size.checked_div(TAR_BLOCK))
            .and_then(|blocks| blocks.checked_mul(TAR_BLOCK))
        else {
            return Vec::new();
        };
        let Some(next) = payload_start.checked_add(padded) else {
            return Vec::new();
        };
        if next > data.len() {
            return Vec::new();
        }
        offset = next;
    }
    terminated.then_some(out).unwrap_or_default()
}

fn tar_write_all(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    if entries.len() > MAX_ENTRIES {
        return Vec::new();
    }
    let mut wire_size = TAR_BLOCK * 2;
    let mut budget = ArchiveAllocationBudget::default();
    for (name, data) in entries {
        if !tar_name_valid(name) || data.len() > MAX_OUTPUT {
            continue;
        }
        let Some(materialized) = name.len().checked_add(data.len()) else {
            return Vec::new();
        };
        if !budget.reserve(materialized) {
            return Vec::new();
        }
        let Some(data_size) = tar_entry_wire_len(data.len()) else {
            return Vec::new();
        };
        wire_size = match wire_size.checked_add(data_size) {
            Some(size) if size <= MAX_OUTPUT => size,
            _ => return Vec::new(),
        };
        if split_ustar_name(name).is_none() {
            let Some(long_size) = name.len().checked_add(1).and_then(tar_entry_wire_len) else {
                return Vec::new();
            };
            wire_size = match wire_size.checked_add(long_size) {
                Some(size) if size <= MAX_OUTPUT => size,
                _ => return Vec::new(),
            };
        }
    }
    let mut out = Vec::with_capacity(wire_size);
    for (name, data) in entries {
        if !tar_name_valid(name) || data.len() > MAX_OUTPUT {
            continue;
        }
        if split_ustar_name(name).is_none() {
            if !append_tar_long_name(&mut out, name) {
                return Vec::new();
            }
        }
        if !append_tar_entry(&mut out, name, data, b'0') {
            return Vec::new();
        }
        if out.len() > MAX_OUTPUT {
            return Vec::new();
        }
    }
    out.resize(wire_size, 0);
    out
}

fn tar_entry_wire_len(data_len: usize) -> Option<usize> {
    let padded = data_len
        .checked_add(TAR_BLOCK - 1)?
        .checked_div(TAR_BLOCK)?
        .checked_mul(TAR_BLOCK)?;
    TAR_BLOCK.checked_add(padded)
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

fn append_tar_entry(out: &mut Vec<u8>, name: &str, data: &[u8], kind: u8) -> bool {
    let Some(size) = u64::try_from(data.len()).ok() else {
        return false;
    };
    if !append_tar_header(out, name, size, kind) {
        return false;
    }
    out.extend_from_slice(data);
    let Some(padded) = out
        .len()
        .checked_add(TAR_BLOCK - 1)
        .and_then(|size| size.checked_div(TAR_BLOCK))
        .and_then(|blocks| blocks.checked_mul(TAR_BLOCK))
    else {
        return false;
    };
    out.resize(padded, 0);
    true
}

fn append_tar_long_name(out: &mut Vec<u8>, name: &str) -> bool {
    let Some(size) = name.len().checked_add(1).and_then(|size| u64::try_from(size).ok()) else {
        return false;
    };
    if !append_tar_header(out, "././#LongLink", size, b'L') {
        return false;
    }
    out.extend_from_slice(name.as_bytes());
    out.push(0);
    let Some(padded) = out
        .len()
        .checked_add(TAR_BLOCK - 1)
        .and_then(|size| size.checked_div(TAR_BLOCK))
        .and_then(|blocks| blocks.checked_mul(TAR_BLOCK))
    else {
        return false;
    };
    out.resize(padded, 0);
    true
}

fn append_tar_header(out: &mut Vec<u8>, name: &str, size: u64, kind: u8) -> bool {
    let mut header = [0u8; TAR_BLOCK];
    let (path, prefix) = split_ustar_name(name).unwrap_or_else(|| {
        let bytes = name.as_bytes();
        (&bytes[..bytes.len().min(100)], &[][..])
    });
    header[..path.len()].copy_from_slice(path);
    let Some(prefix_end) = 345usize.checked_add(prefix.len()) else {
        return false;
    };
    header[345..prefix_end].copy_from_slice(prefix);
    put_tar_octal(&mut header[100..108], 0o644);
    put_tar_octal(&mut header[108..116], 0);
    put_tar_octal(&mut header[116..124], 0);
    put_tar_octal(&mut header[124..136], size);
    put_tar_octal(&mut header[136..148], 0);
    header[148..156].fill(b' ');
    header[156] = kind;
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    let checksum = header.iter().map(|byte| *byte as u64).sum();
    put_tar_checksum(&mut header[148..156], checksum);
    out.extend_from_slice(&header);
    true
}

fn set_tar_long_name(
    long_name: &mut Option<String>,
    budget: &mut ArchiveAllocationBudget,
    value: &str,
) -> bool {
    if let Some(old) = long_name.take() {
        let old_len = old.len();
        drop(old);
        budget.release(old_len);
    }
    if !budget.reserve(value.len()) {
        return false;
    }
    *long_name = Some(value.to_owned());
    true
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
        .find(|(index, byte)| {
            **byte == b'/'
                && *index <= 155
                && (*index)
                    .checked_add(1)
                    .and_then(|start| bytes.len().checked_sub(start))
                    .is_some_and(|len| len <= 100)
        })
        .and_then(|(index, _)| index.checked_add(1).map(|start| (&bytes[start..], &bytes[..index])))
}

fn tar_header_name(header: &[u8], budget: &mut ArchiveAllocationBudget) -> Option<String> {
    let name = std::str::from_utf8(trim_nul(&header[..100])).unwrap_or("");
    let prefix = std::str::from_utf8(trim_nul(&header[345..500])).unwrap_or("");
    let size = if prefix.is_empty() {
        name.len()
    } else {
        prefix.len().checked_add(1)?.checked_add(name.len())?
    };
    if !budget.reserve(size) {
        return None;
    }
    let mut full = String::with_capacity(size);
    if !prefix.is_empty() {
        full.push_str(prefix);
        full.push('/');
    }
    full.push_str(name);
    Some(full)
}

fn trim_nul(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes.iter().position(|byte| *byte == 0).unwrap_or(bytes.len())]
}

fn pax_path(payload: &[u8]) -> Option<&str> {
    let mut offset = 0;
    while offset < payload.len() {
        let space = offset.checked_add(
            payload[offset..].iter().position(|byte| *byte == b' ')?,
        )?;
        let len = std::str::from_utf8(&payload[offset..space]).ok()?.parse::<usize>().ok()?;
        let end = offset.checked_add(len)?;
        let record_start = space.checked_add(1)?;
        let record = payload.get(record_start..end.checked_sub(1)?)?;
        if let Some(path) = record.strip_prefix(b"path=") {
            return std::str::from_utf8(path).ok();
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
    if text.is_empty() {
        Some(0)
    } else {
        u64::from_str_radix(text, 8).ok()
    }
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
    (start..=data.len().checked_sub(22)?).rev().find(|offset| {
        read_u32(data, *offset) == Some(0x0605_4b50)
            && read_u16_at(data, *offset, 20).is_some_and(|comment| {
                offset
                    .checked_add(22)
                    .and_then(|start| start.checked_add(usize::from(comment)))
                    == Some(data.len())
            })
    })
}

fn read_u16_at(data: &[u8], base: usize, offset: usize) -> Option<u16> {
    read_u16(data, base.checked_add(offset)?)
}

fn read_u32_at(data: &[u8], base: usize, offset: usize) -> Option<u32> {
    read_u32(data, base.checked_add(offset)?)
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

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
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
            self.bit = self.bit.checked_add(1)?;
        }
        Some(value)
    }

    fn align_byte(&mut self) -> Option<()> {
        self.bit = self
            .bit
            .checked_add(7)?
            .checked_div(8)?
            .checked_mul(8)?;
        Some(())
    }
}

struct Huffman(Vec<(u32, usize, u16)>);

impl Huffman {
    fn new(lengths: &[u8]) -> Option<Self> {
        let max = usize::from(*lengths.iter().max()?);
        if max == 0 || max > 15 {
            return None;
        }
        let mut counts = vec![0u32; max.checked_add(1)?];
        for length in lengths.iter().copied().filter(|length| *length != 0) {
            let count = &mut counts[usize::from(length)];
            *count = count.checked_add(1)?;
        }
        let mut next = vec![0u32; max.checked_add(1)?];
        let mut code = 0u32;
        for bits in 1..=max {
            code = code
                .checked_add(counts[bits - 1])?
                .checked_shl(1)?;
            let limit = 1u32.checked_shl(bits.try_into().ok()?)?;
            if code.checked_add(counts[bits])? > limit {
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
            next[length] = next[length].checked_add(1)?;
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
    (expected_len <= MAX_OUTPUT).then(|| inflate_with_limit(data, Some(expected_len)))?
}

fn inflate_any(data: &[u8]) -> Option<Vec<u8>> {
    inflate_with_limit(data, None)
}

fn inflate_with_limit(data: &[u8], expected_len: Option<usize>) -> Option<Vec<u8>> {
    let mut bits = Bits { data, bit: 0 };
    let mut out = Vec::with_capacity(expected_len.unwrap_or(0).min(MAX_OUTPUT));
    loop {
        let final_block = bits.read(1)? != 0;
        match bits.read(2)? {
            0 => {
                bits.align_byte()?;
                let len = bits.read(16)? as usize;
                if bits.read(16)? as u16 != !(len as u16) {
                    return None;
                }
                for _ in 0..len {
                    if expected_len.is_some_and(|limit| out.len() == limit)
                        || out.len() == MAX_OUTPUT
                    {
                        return None;
                    }
                    out.push(bits.read(8)? as u8);
                }
            }
            1 => {
                let (literal, distance) = fixed_trees()?;
                inflate_codes(&mut bits, &literal, &distance, &mut out, expected_len)?;
            }
            2 => {
                let (literal, distance) = dynamic_trees(&mut bits)?;
                inflate_codes(&mut bits, &literal, &distance, &mut out, expected_len)?;
            }
            _ => return None,
        }
        if final_block {
            return match expected_len {
                None => Some(out),
                Some(limit) if out.len() == limit => Some(out),
                Some(_) => None,
            };
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
    let literal_count = (bits.read(5)? as usize).checked_add(257)?;
    let distance_count = (bits.read(5)? as usize).checked_add(1)?;
    let code_count = (bits.read(4)? as usize).checked_add(4)?;
    let order = [16usize, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
    let mut code_lengths = [0u8; 19];
    for index in 0..code_count {
        code_lengths[order[index]] = bits.read(3)? as u8;
    }
    let code_tree = Huffman::new(&code_lengths)?;
    let total = literal_count.checked_add(distance_count)?;
    let mut lengths = Vec::with_capacity(total);
    while lengths.len() < total {
        match code_tree.symbol(bits)? {
            value @ 0..=15 => lengths.push(value as u8),
            16 => {
                let previous = *lengths.last()?;
                let repeat = (bits.read(2)? as usize).checked_add(3)?;
                lengths.resize(lengths.len().checked_add(repeat)?, previous);
            }
            17 => {
                let repeat = (bits.read(3)? as usize).checked_add(3)?;
                lengths.resize(lengths.len().checked_add(repeat)?, 0)
            }
            18 => {
                let repeat = (bits.read(7)? as usize).checked_add(11)?;
                lengths.resize(lengths.len().checked_add(repeat)?, 0)
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
    expected_len: Option<usize>,
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
                if expected_len.is_some_and(|limit| out.len() == limit)
                    || out.len() == MAX_OUTPUT
                {
                    return None;
                }
                out.push(byte as u8);
            }
            256 => return Some(()),
            symbol @ 257..=285 => {
                let index = usize::from(symbol - 257);
                let length = LENGTH_BASE[index].checked_add(bits.read(LENGTH_EXTRA[index])? as usize)?;
                let distance_symbol = usize::from(distance.symbol(bits)?);
                let base = *DIST_BASE.get(distance_symbol)?;
                let extra = *DIST_EXTRA.get(distance_symbol)?;
                let distance = base.checked_add(bits.read(extra)? as usize)?;
                let end = out.len().checked_add(length)?;
                if distance == 0 || distance > out.len() || end > MAX_OUTPUT || expected_len.is_some_and(|limit| end > limit) {
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

    #[test]
    fn names_json_escapes_controls_and_bounds_expansion() {
        assert_eq!(
            names_json(vec!["quote\"slash\\line\ncontrol\u{0001}"]),
            Some("[\"quote\\\"slash\\\\line\\ncontrol\\u0001\"]".to_string())
        );

        let name = "\u{0001}".repeat(MAX_OUTPUT / 6);
        assert!(names_json(vec![name.as_str()]).is_none());
    }
}
