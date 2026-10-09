// D-UUIDENC1=A / D-BYTESDECODE1=A: exact encoding and byte-decoding kernels.


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

pub(crate) fn jet_std_binary_calcsize(format: &str) -> Result<i64, String> {
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
    format: &str,
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
    format: &str,
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
    format: &str,
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
