// D-UUIDENC1=A / D-BYTESDECODE1=A: exact encoding and byte-decoding kernels.

const JET_B64_CHARS: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `core.encoding.hex.decode`: mixed-case digits, whole bytes only, no
/// whitespace. The length check runs before the digit scan.
pub(crate) fn jet_std_hex_decode(text: &String) -> Result<Vec<u8>, String> {
    if text.len() % 2 != 0 {
        return Err(
            "odd-length hex string; hex text encodes whole bytes, so the length must be even"
                .to_string(),
        );
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    for pair in text.as_bytes().chunks_exact(2) {
        let (Some(high), Some(low)) = (hex_nibble(pair[0]), hex_nibble(pair[1])) else {
            return Err("invalid hex digit; expected 0-9, a-f, or A-F".to_string());
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

fn hex_with_digits(bytes: &[u8], digits: &[u8; 16]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(digits[usize::from(byte >> 4)] as char);
        out.push(digits[usize::from(byte & 0x0f)] as char);
    }
    out
}
pub(crate) fn jet_std_hex_encode(bytes: &Vec<u8>) -> String {
    hex_with_digits(bytes, b"0123456789abcdef")
}
pub(crate) fn jet_std_hex_encode_upper(bytes: &Vec<u8>) -> String {
    hex_with_digits(bytes, b"0123456789ABCDEF")
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
    for (row, chunk) in bytes.chunks(16).enumerate() {
        let offset = row * 16;
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
/// Python `base64.encodebytes`: standard Base64 in 76-character lines, each
/// ending in `\n`.
pub(crate) fn jet_std_b64_encodebytes(bytes: &Vec<u8>) -> String {
    let encoded = jet_std_b64_encode(bytes);
    let mut out = String::with_capacity(encoded.len() + encoded.len() / 76 + 1);
    for line in encoded.as_bytes().chunks(76) {
        out.extend(line.iter().map(|&byte| char::from(byte)));
        out.push('\n');
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
