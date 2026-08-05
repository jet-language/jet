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

pub fn jet_archive_zip_compress(name: &str, data: &[u8]) -> Vec<u8> {
    if data.len() > MAX_OUTPUT {
        return Vec::new();
    }
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
    put_u16(&mut out, 0);
    put_u16(&mut out, 0);
    put_u16(&mut out, 0x21);
    put_u32(&mut out, crc);
    put_u32(&mut out, size);
    put_u32(&mut out, size);
    put_u16(&mut out, name_len);
    put_u16(&mut out, 0);
    out.extend_from_slice(name);
    out.extend_from_slice(data);

    let central_offset = u32::try_from(out.len()).unwrap_or(0);
    put_u32(&mut out, 0x0201_4b50);
    put_u16(&mut out, 0x031e);
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

    let central_size = u32::try_from(out.len().saturating_sub(central_offset as usize)).unwrap_or(0);
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

pub fn jet_archive_zip_decompress(data: &[u8]) -> Vec<u8> {
    let Some(eocd) = find_eocd(data) else {
        return Vec::new();
    };
    if read_u16(data, eocd + 8) != Some(1) || read_u16(data, eocd + 10) != Some(1) {
        return Vec::new();
    }
    let Some(central) = read_u32(data, eocd + 16).map(|n| n as usize) else {
        return Vec::new();
    };
    if read_u32(data, central) != Some(0x0201_4b50) {
        return Vec::new();
    }
    let flags = read_u16(data, central + 8).unwrap_or(u16::MAX);
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
    if expected_len > MAX_OUTPUT {
        return Vec::new();
    }
    let Some(expected_crc) = read_u32(data, central + 16) else {
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
    if out.len() == expected_len && crc32(&out) == expected_crc {
        out
    } else {
        Vec::new()
    }
}

pub fn jet_archive_tar_add(archive: &[u8], name: &str, data: &[u8]) -> Vec<u8> {
    if data.len() > MAX_OUTPUT {
        return Vec::new();
    }
    let mut entries = tar_read_all(archive);
    if let Some(index) = entries.iter().position(|(entry, _)| entry == name) {
        entries[index] = (name.to_string(), data.to_vec());
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
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    let mut offset = 0usize;
    let mut long_name = None;
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
            long_name = Some(trim_nul(payload));
        } else if kind == b'x' {
            long_name = Some(pax_path(payload).unwrap_or_default());
        } else if kind != b'g' {
            let name = long_name.take().unwrap_or_else(|| tar_header_name(header));
            if !tar_name_valid(&name) {
                return Vec::new();
            }
            let existing: usize = out.iter().map(|(_, bytes)| bytes.len()).sum();
            if existing.saturating_add(payload.len()) > MAX_OUTPUT {
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
    let mut out = Vec::new();
    for (name, data) in entries {
        if !tar_name_valid(name) || data.len() > MAX_OUTPUT {
            continue;
        }
        if split_ustar_name(name).is_none() {
            let mut long = name.as_bytes().to_vec();
            long.push(0);
            append_tar_entry(&mut out, "././#LongLink", &long, b'L');
            if out.len() > MAX_OUTPUT {
                return Vec::new();
            }
        }
        append_tar_entry(&mut out, name, data, b'0');
        if out.len() > MAX_OUTPUT {
            return Vec::new();
        }
    }
    if out.len().saturating_add(TAR_BLOCK * 2) > MAX_OUTPUT {
        return Vec::new();
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
    if prefix.is_empty() {
        name
    } else {
        format!("{prefix}/{name}")
    }
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
            return (out.len() == expected_len).then_some(out);
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
                lengths.resize(lengths.len().checked_add(bits.read(2)? as usize + 3)?, previous);
            }
            17 => lengths.resize(lengths.len().checked_add(bits.read(3)? as usize + 3)?, 0),
            18 => lengths.resize(lengths.len().checked_add(bits.read(7)? as usize + 11)?, 0),
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
                if distance == 0 || distance > out.len() || out.len().checked_add(length)? > expected_len {
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
