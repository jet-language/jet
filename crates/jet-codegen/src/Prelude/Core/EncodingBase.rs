// D-UUIDENC1=A / D-BYTESDECODE1=A: exact encoding and byte-decoding kernels.

const JET_B64_CHARS: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub(crate) fn jet_std_hex_decode(text: &String) -> Result<Vec<u8>, String> {
    if let Some((offset, _)) = text.char_indices().find(|(_, ch)| ch.is_whitespace()) {
        return Err(format!(
            "hex string contains whitespace at byte offset {offset}"
        ));
    }
    if text.len() % 2 != 0 {
        return Err(format!("hex string has odd length ({})", text.len()));
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    for (pair_index, pair) in text.as_bytes().chunks_exact(2).enumerate() {
        let offset = pair_index * 2;
        let Some(high) = hex_nibble(pair[0]) else {
            return Err(format!(
                "invalid hex at offset {offset}: {:?}",
                String::from_utf8_lossy(pair)
            ));
        };
        let Some(low) = hex_nibble(pair[1]) else {
            return Err(format!(
                "invalid hex at offset {offset}: {:?}",
                String::from_utf8_lossy(pair)
            ));
        };
        out.push((high << 4) | low);
    }
    Ok(out)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn jet_std_hex_encode(bytes: &Vec<u8>) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
pub(crate) fn jet_std_hex_encode_upper(bytes: &Vec<u8>) -> String {
    bytes.iter().map(|byte| format!("{byte:02X}")).collect()
}
pub(crate) fn jet_std_hex_encode_prefixed(bytes: &Vec<u8>) -> String {
    format!("0x{}", jet_std_hex_encode(bytes))
}
pub(crate) fn jet_std_hex_encode_sep(bytes: &Vec<u8>, separator: &String) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(separator)
}
pub(crate) fn jet_std_hex_is_hex(text: &String) -> bool {
    !text.is_empty() && text.len().is_multiple_of(2) && text.bytes().all(|byte| hex_nibble(byte).is_some())
}
pub(crate) fn jet_std_hex_dump(bytes: &Vec<u8>) -> String {
    let mut out = String::new();
    for (offset, chunk) in bytes.chunks(16).enumerate() {
        let mut hex = String::new();
        for index in 0..16 {
            if index > 0 {
                hex.push(' ');
            }
            if let Some(byte) = chunk.get(index) {
                hex.push_str(&format!("{byte:02x}"));
            } else {
                hex.push_str("  ");
            }
        }
        let ascii: String = chunk
            .iter()
            .map(|byte| {
                if (32..=126).contains(byte) {
                    *byte as char
                } else {
                    '.'
                }
            })
            .collect();
        out.push_str(&format!("{offset:08x}  {hex}  {ascii}\n"));
    }
    out
}
pub(crate) fn jet_std_crc_hqx(bytes: &Vec<u8>, value: i64) -> i64 {
    let mut crc = (value & 0xffff) as u16;
    for &byte in bytes {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    i64::from(crc)
}

pub(crate) fn jet_std_crc32(bytes: &Vec<u8>) -> i64 {
    let mut crc = 0xffff_ffffu32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    i64::from((!crc) as i32) & 0xffff_ffff
}

pub(crate) fn jet_std_b2a_qp(bytes: &Vec<u8>) -> String {
    let mut out = String::new();
    for &byte in bytes {
        match byte {
            b'\n' => out.push('\n'),
            33..=60 | 62..=126 | b' ' | b'\t' => out.push(byte as char),
            _ => out.push_str(&format!("={byte:02X}")),
        }
    }
    out
}

pub(crate) fn jet_std_a2b_qp(text: &String) -> Result<Vec<u8>, String> {
    let raw = text.as_bytes();
    let mut out = Vec::new();
    let mut index = 0usize;
    while index < raw.len() {
        if raw[index] == b'=' {
            if index + 1 < raw.len() && raw[index + 1] == b'\n' {
                index += 2;
                continue;
            }
            if index + 2 >= raw.len() {
                return Err("incomplete quoted-printable escape".to_string());
            }
            let Some(high) = hex_nibble(raw[index + 1]) else {
                return Err("invalid quoted-printable escape".to_string());
            };
            let Some(low) = hex_nibble(raw[index + 2]) else {
                return Err("invalid quoted-printable escape".to_string());
            };
            out.push((high << 4) | low);
            index += 3;
        } else {
            out.push(raw[index]);
            index += 1;
        }
    }
    Ok(out)
}

fn uu_char(value: u8) -> u8 {
    let value = value & 0x3f;
    if value == 0 { b'`' } else { value + 32 }
}

fn uu_value(value: u8) -> u8 {
    if value == b'`' { 0 } else { value.wrapping_sub(32) & 0x3f }
}

pub(crate) fn jet_std_b2a_uu(bytes: &Vec<u8>) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(45) {
        out.push(uu_char(chunk.len() as u8) as char);
        for triple in chunk.chunks(3) {
            let a = triple[0];
            let b = triple.get(1).copied().unwrap_or(0);
            let c = triple.get(2).copied().unwrap_or(0);
            out.push(uu_char(a >> 2) as char);
            out.push(uu_char((a << 4) | (b >> 4)) as char);
            out.push(uu_char((b << 2) | (c >> 6)) as char);
            out.push(uu_char(c) as char);
        }
        out.push('\n');
    }
    out
}

pub(crate) fn jet_std_a2b_uu(text: &String) -> Result<Vec<u8>, String> {
    let raw = text.as_bytes();
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    let count = uu_value(raw[0]) as usize;
    let mut out = Vec::with_capacity(count);
    let mut index = 1usize;
    while index + 3 < raw.len() && out.len() < count {
        let a = uu_value(raw[index]);
        let b = uu_value(raw[index + 1]);
        let c = uu_value(raw[index + 2]);
        let d = uu_value(raw[index + 3]);
        out.push((a << 2) | (b >> 4));
        if out.len() < count {
            out.push((b << 4) | (c >> 2));
        }
        if out.len() < count {
            out.push((c << 6) | d);
        }
        index += 4;
    }
    if out.len() == count {
        Ok(out)
    } else {
        Err("truncated uuencoded data".to_string())
    }
}

pub(crate) fn jet_std_b64_encode(bytes: &Vec<u8>) -> String {
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(JET_B64_CHARS[(n >> 18) as usize] as char);
        out.push(JET_B64_CHARS[((n >> 12) & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            JET_B64_CHARS[((n >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            JET_B64_CHARS[(n & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}
/// Encode a packed integer list without materializing an intermediate byte
/// vector. JIT `[U8]` literals use the dense `i64` carrier.
pub(crate) fn jet_std_b64_encode_ints(values: &[i64]) -> String {
    let mut out = String::with_capacity((values.len() + 2) / 3 * 4);
    for chunk in values.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 {
            chunk[1] as u32
        } else {
            0
        };
        let b2 = if chunk.len() > 2 {
            chunk[2] as u32
        } else {
            0
        };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(JET_B64_CHARS[(n >> 18) as usize] as char);
        out.push(JET_B64_CHARS[((n >> 12) & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            JET_B64_CHARS[((n >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            JET_B64_CHARS[(n & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

pub(crate) fn jet_std_b64url_encode(bytes: &Vec<u8>) -> String {
    jet_std_b64_encode(bytes)
        .trim_end_matches('=')
        .replace('+', "-")
        .replace('/', "_")
}
pub(crate) fn jet_std_b64url_encode_padded(bytes: &Vec<u8>) -> String {
    jet_std_b64_encode(bytes)
        .replace('+', "-")
        .replace('/', "_")
}
pub(crate) fn jet_std_b64_pad(text: &String) -> String {
    let remainder = text.len() % 4;
    if remainder == 0 {
        return text.clone();
    }
    format!("{text}{}", "=".repeat(4 - remainder))
}
pub(crate) fn jet_std_b64_unpad(text: &String) -> String {
    text.trim_end_matches('=').to_string()
}
pub(crate) fn jet_std_b64_is_base64(text: &String) -> bool {
    if text.is_empty() {
        return false;
    }
    let mut padding = 0usize;
    for byte in text.bytes() {
        if byte == b'=' {
            padding += 1;
            if padding > 2 {
                return false;
            }
            continue;
        }
        if padding != 0
            || !(byte.is_ascii_uppercase()
                || byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'+' | b'/' | b'-' | b'_'))
        {
            return false;
        }
    }
    text.len().is_multiple_of(4) || padding > 0
}

pub(crate) fn jet_std_base32_is_base32(text: &String) -> bool {
    !text.is_empty()
        && text.bytes().all(|byte| {
            byte.is_ascii_uppercase()
                || byte.is_ascii_lowercase()
                || matches!(byte, b'2'..=b'7' | b'=')
        })
}

const JET_BASE32_CHARS: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
const JET_BASE32HEX_CHARS: &[u8; 32] = b"0123456789ABCDEFGHIJKLMNOPQRSTUV";

fn base32_encode_with_alphabet(bytes: &Vec<u8>, alphabet: &[u8; 32]) -> String {
    let mut out = String::new();
    let mut buffer: u32 = 0;
    let mut bits = 0u8;
    for &byte in bytes {
        buffer = (buffer << 8) | byte as u32;
        bits += 8;
        while bits >= 5 {
            let index = ((buffer >> (bits - 5)) & 31) as usize;
            out.push(alphabet[index] as char);
            bits -= 5;
        }
    }
    if bits > 0 {
        let index = ((buffer << (5 - bits)) & 31) as usize;
        out.push(alphabet[index] as char);
    }
    while out.len() % 8 != 0 {
        out.push('=');
    }
    out
}

pub(crate) fn jet_std_base32_encode(bytes: &Vec<u8>) -> String {
    base32_encode_with_alphabet(bytes, JET_BASE32_CHARS)
}

pub(crate) fn jet_std_base32hex_encode(bytes: &Vec<u8>) -> String {
    base32_encode_with_alphabet(bytes, JET_BASE32HEX_CHARS)
}

const JET_B85_CHARS: &[u8] =
    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz!#$%&()*+-;<=>?@^_`{|}~";

fn base85_word(chunk: &[u8]) -> u32 {
    let mut word = 0u32;
    for &byte in chunk {
        word = (word << 8) | byte as u32;
    }
    word << (8 * (4 - chunk.len()))
}

fn base85_digits(word: u32, alphabet: impl Fn(usize) -> u8) -> [u8; 5] {
    let mut digits = [0u8; 5];
    let mut value = word as u64;
    let mut index = 5usize;
    while index > 0 {
        index -= 1;
        digits[index] = alphabet((value % 85) as usize);
        value /= 85;
    }
    digits
}

pub(crate) fn jet_std_a85_encode(bytes: &Vec<u8>) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(4) {
        let word = base85_word(chunk);
        if chunk.len() == 4 && word == 0 {
            out.push('z');
            continue;
        }
        let digits = base85_digits(word, |index| (33 + index) as u8);
        let count = if chunk.len() == 4 { 5 } else { chunk.len() + 1 };
        for &digit in &digits[..count] {
            out.push(digit as char);
        }
    }
    out
}

pub(crate) fn jet_std_b85_encode(bytes: &Vec<u8>) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(4) {
        let digits = base85_digits(base85_word(chunk), |index| JET_B85_CHARS[index]);
        let count = if chunk.len() == 4 { 5 } else { chunk.len() + 1 };
        for &digit in &digits[..count] {
            out.push(digit as char);
        }
    }
    out
}

fn decode_base85_group(group: &[u8; 5]) -> u32 {
    let mut word = 0u32;
    for &digit in group {
        word = word * 85 + digit as u32;
    }
    word
}

fn is_ascii_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

pub(crate) fn jet_std_a85_decode(text: &String) -> Result<Vec<u8>, String> {
    let mut raw = text.as_bytes();
    let adobe = raw.starts_with(b"<~");
    if adobe {
        if !raw.ends_with(b"~>") {
            return Err("Ascii85 adobe framing must end with ~>".to_string());
        }
        raw = &raw[2..raw.len() - 2];
    }
    let mut out = Vec::new();
    let mut group = [0u8; 5];
    let mut length = 0usize;
    for &byte in raw {
        if is_ascii_space(byte) {
            continue;
        }
        if byte == b'z' {
            if length != 0 {
                return Err("Ascii85 z shortcut must start a group".to_string());
            }
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        if !(33..=117).contains(&byte) {
            return Err("invalid Ascii85 character".to_string());
        }
        group[length] = byte - 33;
        length += 1;
        if length == 5 {
            out.extend_from_slice(&decode_base85_group(&group).to_be_bytes());
            length = 0;
        }
    }
    if length == 1 {
        return Err("Ascii85 final group must contain at least two characters".to_string());
    }
    if length > 1 {
        for digit in &mut group[length..] {
            *digit = 84;
        }
        let bytes = decode_base85_group(&group).to_be_bytes();
        out.extend_from_slice(&bytes[..length - 1]);
    }
    Ok(out)
}

pub(crate) fn jet_std_b85_decode(text: &String) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut group = [0u8; 5];
    let mut length = 0usize;
    for byte in text.bytes() {
        let Some(index) = JET_B85_CHARS.iter().position(|&candidate| candidate == byte) else {
            return Err("invalid Base85 character".to_string());
        };
        group[length] = index as u8;
        length += 1;
        if length == 5 {
            out.extend_from_slice(&decode_base85_group(&group).to_be_bytes());
            length = 0;
        }
    }
    if length == 1 {
        return Err("Base85 final group must contain at least two characters".to_string());
    }
    if length > 1 {
        for digit in &mut group[length..] {
            *digit = 84;
        }
        let bytes = decode_base85_group(&group).to_be_bytes();
        out.extend_from_slice(&bytes[..length - 1]);
    }
    Ok(out)
}

const JET_Z85_CHARS: &[u8] =
    b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ.-:+=^!/*?&<>()[]{}@%$#";

pub(crate) fn jet_std_z85_encode(bytes: &Vec<u8>) -> String {
    if bytes.len() % 4 != 0 {
        return String::new();
    }
    let mut out = String::with_capacity(bytes.len() / 4 * 5);
    for chunk in bytes.chunks_exact(4) {
        let digits = base85_digits(base85_word(chunk), |index| JET_Z85_CHARS[index]);
        for &digit in &digits {
            out.push(digit as char);
        }
    }
    out
}

pub(crate) fn jet_std_z85_decode(text: &String) -> Result<Vec<u8>, String> {
    if text.len() % 5 != 0 {
        return Err("Z85 text length must be a multiple of five".to_string());
    }
    let mut out = Vec::with_capacity(text.len() / 5 * 4);
    let mut group = [0u8; 5];
    for (index, byte) in text.bytes().enumerate() {
        let Some(digit) = JET_Z85_CHARS.iter().position(|&candidate| candidate == byte) else {
            return Err("invalid Z85 character".to_string());
        };
        group[index % 5] = digit as u8;
        if index % 5 == 4 {
            out.extend_from_slice(&decode_base85_group(&group).to_be_bytes());
        }
    }
    Ok(out)
}
pub(crate) fn jet_std_binary_pack_u8(value: i64) -> Vec<u8> {
    vec![value as u8]
}

pub(crate) fn jet_std_binary_pack_i8(value: i64) -> Vec<u8> {
    jet_std_binary_pack_u8(value)
}

pub(crate) fn jet_std_binary_pack_u16le(value: i64) -> Vec<u8> {
    (value as u16).to_le_bytes().to_vec()
}

pub(crate) fn jet_std_binary_pack_u16be(value: i64) -> Vec<u8> {
    (value as u16).to_be_bytes().to_vec()
}

pub(crate) fn jet_std_binary_pack_u32le(value: i64) -> Vec<u8> {
    (value as u32).to_le_bytes().to_vec()
}

pub(crate) fn jet_std_binary_pack_u32be(value: i64) -> Vec<u8> {
    (value as u32).to_be_bytes().to_vec()
}

pub(crate) fn jet_std_binary_pack_u64le(value: i64) -> Vec<u8> {
    (value as u64).to_le_bytes().to_vec()
}

pub(crate) fn jet_std_binary_pack_u64be(value: i64) -> Vec<u8> {
    (value as u64).to_be_bytes().to_vec()
}

fn jet_std_binary_slice<const N: usize>(
    data: &Vec<u8>,
    offset: i64,
) -> Option<[u8; N]> {
    let start = usize::try_from(offset).ok()?;
    let end = start.checked_add(N)?;
    data.get(start..end)?.try_into().ok()
}

pub(crate) fn jet_std_binary_unpack_u8(data: &Vec<u8>, offset: i64) -> Option<i64> {
    let [value] = jet_std_binary_slice::<1>(data, offset)?;
    Some(i64::from(value))
}

pub(crate) fn jet_std_binary_unpack_u16le(data: &Vec<u8>, offset: i64) -> Option<i64> {
    Some(i64::from(u16::from_le_bytes(jet_std_binary_slice(data, offset)?)))
}

pub(crate) fn jet_std_binary_unpack_u16be(data: &Vec<u8>, offset: i64) -> Option<i64> {
    Some(i64::from(u16::from_be_bytes(jet_std_binary_slice(data, offset)?)))
}

pub(crate) fn jet_std_binary_unpack_u32le(data: &Vec<u8>, offset: i64) -> Option<i64> {
    Some(i64::from(u32::from_le_bytes(jet_std_binary_slice(data, offset)?)))
}

pub(crate) fn jet_std_binary_unpack_u32be(data: &Vec<u8>, offset: i64) -> Option<i64> {
    Some(i64::from(u32::from_be_bytes(jet_std_binary_slice(data, offset)?)))
}

pub(crate) fn jet_std_binary_unpack_u64le(data: &Vec<u8>, offset: i64) -> Option<i64> {
    Some(i64::from_ne_bytes(
        u64::from_le_bytes(jet_std_binary_slice(data, offset)?).to_ne_bytes(),
    ))
}

pub(crate) fn jet_std_binary_unpack_u64be(data: &Vec<u8>, offset: i64) -> Option<i64> {
    Some(i64::from_ne_bytes(
        u64::from_be_bytes(jet_std_binary_slice(data, offset)?).to_ne_bytes(),
    ))
}

pub(crate) fn jet_std_binary_sign_extend(value: i64, bits: i64) -> i64 {
    if !(1..=64).contains(&bits) {
        return value;
    }
    let shift = 64 - bits as u32;
    (value << shift) >> shift
}

pub(crate) fn jet_std_binary_pack_f64le(value: f64) -> Vec<u8> {
    value.to_bits().to_le_bytes().to_vec()
}

pub(crate) fn jet_std_binary_pack_f64be(value: f64) -> Vec<u8> {
    value.to_bits().to_be_bytes().to_vec()
}

pub(crate) fn jet_std_binary_unpack_f64le(data: &Vec<u8>, offset: i64) -> Option<f64> {
    Some(f64::from_bits(u64::from_le_bytes(jet_std_binary_slice(data, offset)?)))
}

pub(crate) fn jet_std_binary_unpack_f64be(data: &Vec<u8>, offset: i64) -> Option<f64> {
    Some(f64::from_bits(u64::from_be_bytes(jet_std_binary_slice(data, offset)?)))
}

#[derive(Clone, Copy)]
enum JetStdBinaryEndian {
    Little,
    Big,
}

fn jet_std_binary_format(
    format: &str,
) -> Result<(JetStdBinaryEndian, Vec<(u8, usize, usize)>), String> {
    let bytes = format.as_bytes();
    let (endian, mut cursor) = match bytes.first().copied() {
        Some(b'<') | Some(b'=') | Some(b'@') => (JetStdBinaryEndian::Little, 1),
        Some(b'>') | Some(b'!') => (JetStdBinaryEndian::Big, 1),
        _ => (JetStdBinaryEndian::Little, 0),
    };
    let mut ops = Vec::new();
    while cursor < bytes.len() {
        let mut repeat = 0usize;
        while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
            repeat = repeat
                .checked_mul(10)
                .and_then(|value| value.checked_add((bytes[cursor] - b'0') as usize))
                .ok_or_else(|| "binary format repeat count overflowed".to_string())?;
            cursor += 1;
        }
        if repeat == 0 {
            repeat = 1;
        }
        let code = *bytes
            .get(cursor)
            .ok_or_else(|| "binary format repeat count is missing a type".to_string())?;
        cursor += 1;
        let unit_width = match code {
            b'x' | b'c' | b'b' | b'B' => 1,
            b'h' | b'H' => 2,
            b'i' | b'I' | b'l' | b'L' => 4,
            b'q' | b'Q' => 8,
            _ => return Err(format!("unknown binary format character `{}`", code as char)),
        };
        let width = repeat
            .checked_mul(unit_width)
            .ok_or_else(|| "binary format size overflowed".to_string())?;
        ops.push((code, repeat, width));
    }
    Ok((endian, ops))
}

pub(crate) fn jet_std_binary_calcsize(format: &String) -> Result<i64, String> {
    let (_, ops) = jet_std_binary_format(format)?;
    Ok(ops.iter().map(|(_, _, width)| *width as i64).sum())
}

fn jet_std_binary_pack_one(code: u8, endian: JetStdBinaryEndian, value: i64) -> Vec<u8> {
    match (code, endian) {
        (b'x', _) => vec![0],
        (b'c' | b'b' | b'B', _) => vec![value as u8],
        (b'h' | b'H', JetStdBinaryEndian::Little) => (value as u16).to_le_bytes().to_vec(),
        (b'h' | b'H', JetStdBinaryEndian::Big) => (value as u16).to_be_bytes().to_vec(),
        (b'i' | b'I' | b'l' | b'L', JetStdBinaryEndian::Little) => {
            (value as u32).to_le_bytes().to_vec()
        }
        (b'i' | b'I' | b'l' | b'L', JetStdBinaryEndian::Big) => {
            (value as u32).to_be_bytes().to_vec()
        }
        (b'q' | b'Q', JetStdBinaryEndian::Little) => (value as u64).to_le_bytes().to_vec(),
        (b'q' | b'Q', JetStdBinaryEndian::Big) => (value as u64).to_be_bytes().to_vec(),
        _ => Vec::new(),
    }
}

fn jet_std_binary_unpack_one(
    code: u8,
    endian: JetStdBinaryEndian,
    data: &Vec<u8>,
    offset: usize,
) -> Option<i64> {
    let width = match code {
        b'x' | b'c' | b'b' | b'B' => 1,
        b'h' | b'H' => 2,
        b'i' | b'I' | b'l' | b'L' => 4,
        b'q' | b'Q' => 8,
        _ => return None,
    };
    let raw = data.get(offset..offset.checked_add(width)?)?;
    Some(match (code, endian) {
        (b'x', _) => 0,
        (b'c' | b'b' | b'B', _) => i64::from(raw[0]),
        (b'h' | b'H', JetStdBinaryEndian::Little) => {
            i64::from(u16::from_le_bytes(raw.try_into().ok()?))
        }
        (b'h' | b'H', JetStdBinaryEndian::Big) => {
            i64::from(u16::from_be_bytes(raw.try_into().ok()?))
        }
        (b'i' | b'I' | b'l' | b'L', JetStdBinaryEndian::Little) => {
            i64::from(u32::from_le_bytes(raw.try_into().ok()?))
        }
        (b'i' | b'I' | b'l' | b'L', JetStdBinaryEndian::Big) => {
            i64::from(u32::from_be_bytes(raw.try_into().ok()?))
        }
        (b'q' | b'Q', JetStdBinaryEndian::Little) => {
            i64::from_ne_bytes(u64::from_le_bytes(raw.try_into().ok()?).to_ne_bytes())
        }
        (b'q' | b'Q', JetStdBinaryEndian::Big) => {
            i64::from_ne_bytes(u64::from_be_bytes(raw.try_into().ok()?).to_ne_bytes())
        }
        _ => return None,
    })
}

pub(crate) fn jet_std_binary_pack(
    format: &String,
    values: &Vec<i64>,
) -> Result<Vec<u8>, String> {
    let (endian, ops) = jet_std_binary_format(format)?;
    let expected = ops
        .iter()
        .map(|(code, repeat, _)| if *code == b'x' { 0 } else { *repeat })
        .sum::<usize>();
    if values.len() != expected {
        return Err(format!(
            "binary pack expected {expected} values, got {}",
            values.len()
        ));
    }
    let mut out = Vec::new();
    let mut value_index = 0usize;
    for (code, repeat, _) in ops {
        for _ in 0..repeat {
            let value = if code == b'x' {
                0
            } else {
                let value = values[value_index];
                value_index += 1;
                value
            };
            out.extend(jet_std_binary_pack_one(code, endian, value));
        }
    }
    Ok(out)
}

pub(crate) fn jet_std_binary_unpack(
    format: &String,
    data: &Vec<u8>,
) -> Result<Vec<i64>, String> {
    let (endian, ops) = jet_std_binary_format(format)?;
    let size: usize = ops.iter().map(|(_, _, width)| *width).sum();
    if data.len() < size {
        return Err(format!(
            "binary unpack needs {size} bytes, got {}",
            data.len()
        ));
    }
    let mut out = Vec::new();
    let mut offset = 0usize;
    for (code, repeat, width) in ops {
        let unit_width = width / repeat;
        for _ in 0..repeat {
            if code != b'x' {
                let value = jet_std_binary_unpack_one(code, endian, data, offset)
                    .ok_or_else(|| "binary unpack ran out of bytes".to_string())?;
                out.push(value);
            }
            offset += unit_width;
        }
    }
    Ok(out)
}
pub(crate) fn jet_std_binary_iter_unpack(
    format: &String,
    data: &Vec<u8>,
) -> Result<Vec<Vec<i64>>, String> {
    let size = usize::try_from(jet_std_binary_calcsize(format)?)
        .map_err(|_| "binary iter_unpack format size is invalid".to_string())?;
    if size == 0 {
        return Err("binary iter_unpack needs a positive format size".to_string());
    }
    if data.len() % size != 0 {
        return Err("binary iter_unpack buffer length is not a multiple of the format size".to_string());
    }
    data.chunks_exact(size)
        .map(|chunk| jet_std_binary_unpack(format, &chunk.to_vec()))
        .collect()
}
