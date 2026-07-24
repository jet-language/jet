//! D-ENCBASE-STRICT1=A edition-2027 strict RFC 4648 decoders with named allowances.

const WS: [u8; 6] = [0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x20];

fn is_ws(byte: u8) -> bool {
    WS.contains(&byte)
}

fn err(kind: &str, offset: usize, reason: impl Into<String>) -> String {
    format!("invalid {kind} at byte {offset}: {}", reason.into())
}

fn std_b64(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn url_b64(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'-' => Some(62),
        b'_' => Some(63),
        _ => None,
    }
}

fn b32(byte: u8, allow_lowercase: bool) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' if allow_lowercase => Some(byte - b'a'),
        b'2'..=b'7' => Some(byte - b'2' + 26),
        _ => None,
    }
}

struct Scanned {
    symbols: Vec<u8>,
    origins: Vec<usize>,
    pad_start: Option<usize>,
    pad_count: usize,
}

fn scan(
    text: &str,
    kind: &str,
    alphabet: &str,
    url: bool,
    allow_whitespace: bool,
    allow_padding: bool,
    allow_lowercase: bool,
) -> Result<Scanned, String> {
    let raw = text.as_bytes();
    let mut symbols = Vec::new();
    let mut origins = Vec::new();
    let mut pad_start = None;
    let mut pad_count = 0usize;
    let digit = |b: u8| if url { url_b64(b) } else { std_b64(b) };
    for (offset, &byte) in raw.iter().enumerate() {
        if is_ws(byte) {
            if !allow_whitespace {
                return Err(err(kind, offset, "ASCII whitespace is not allowed"));
            }
            continue;
        }
        if byte == b'=' {
            if !allow_padding {
                return Err(err(kind, offset, "padding is not allowed"));
            }
            if pad_start.is_none() {
                pad_start = Some(offset);
            }
            pad_count += 1;
            continue;
        }
        if pad_start.is_some() {
            return Err(err(kind, offset, "padding may appear only at the end"));
        }
        let mapped = if url {
            byte
        } else if allow_lowercase {
            byte
        } else {
            byte
        };
        let value = if kind == "base32" {
            b32(mapped, allow_lowercase)
        } else {
            digit(mapped)
        };
        match value {
            Some(v) => {
                symbols.push(v);
                origins.push(offset);
            }
            None => {
                return Err(err(
                    kind,
                    offset,
                    format!("byte 0x{byte:02X} is not in the {alphabet} alphabet"),
                ));
            }
        }
    }
    Ok(Scanned {
        symbols,
        origins,
        pad_start,
        pad_count,
    })
}

fn decode_quanta(
    kind: &str,
    scanned: &Scanned,
    allow_missing_padding: bool,
    radix: u32,
    _mask: u32,
) -> Result<Vec<u8>, String> {
    let sym_len = scanned.symbols.len();
    let pad = scanned.pad_count;
    let eof = scanned
        .pad_start
        .unwrap_or_else(|| scanned.origins.last().copied().unwrap_or(0).saturating_add(1));
    if sym_len == 0 && pad == 0 {
        return Ok(Vec::new());
    }
    let quantum = if radix == 64 { 4 } else { 8 };
    let total = sym_len + pad;
    let rem = total % quantum;
    if rem == 1 || (!allow_missing_padding && rem != 0) {
        if pad == 0 && allow_missing_padding && matches!(rem, 0 | 2 | 3 | 5 | 7) && radix == 64 {
            // allowed unpadded remainder for base64
        } else if pad == 0 && allow_missing_padding && matches!(rem, 0 | 2 | 4 | 5 | 7) && radix == 32 {
            // allowed unpadded remainder for base32
        } else if rem == 1 {
            return Err(err(kind, eof, "encoded length cannot represent whole bytes"));
        } else if !allow_missing_padding {
            return Err(err(
                kind,
                eof,
                format!("expected {} padding characters", quantum - rem),
            ));
        }
    }
    if pad > 0 {
        let expected = match (radix, sym_len % quantum) {
            (64, 2) => 2,
            (64, 3) => 1,
            (64, 0) if pad > 0 => return Err(err(kind, scanned.pad_start.unwrap(), "unexpected padding")),
            (32, 2) => 6,
            (32, 4) => 4,
            (32, 5) => 3,
            (32, 7) => 1,
            (32, 0) if pad > 0 => return Err(err(kind, scanned.pad_start.unwrap(), "unexpected padding")),
            _ => {
                return Err(err(
                    kind,
                    scanned.pad_start.unwrap(),
                    format!("expected {} padding characters", 0),
                ));
            }
        };
        if pad != expected {
            return Err(err(
                kind,
                scanned.pad_start.unwrap(),
                if pad > expected {
                    "unexpected padding".to_string()
                } else {
                    format!("expected {expected} padding characters")
                },
            ));
        }
    } else if !allow_missing_padding {
        let need = match sym_len % quantum {
            0 => 0,
            2 => if radix == 64 { 2 } else { 6 },
            3 => 1,
            4 => if radix == 32 { 4 } else { return Err(err(kind, eof, "encoded length cannot represent whole bytes")) },
            5 => 3,
            7 => 1,
            _ => return Err(err(kind, eof, "encoded length cannot represent whole bytes")),
        };
        if need > 0 {
            return Err(err(kind, eof, format!("expected {need} padding characters")));
        }
    }
    let mut out = Vec::new();
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for (index, &value) in scanned.symbols.iter().enumerate() {
        buffer = (buffer << (if radix == 64 { 6 } else { 5 })) | value as u32;
        bits += if radix == 64 { 6 } else { 5 };
        while bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xff) as u8);
        }
        if index + 1 == scanned.symbols.len() {
            let unused = bits;
            let trailing = buffer & ((1u32 << bits) - 1);
            if unused > 0 && trailing != 0 {
                let origin = scanned.origins[index];
                return Err(err(kind, origin, "non-zero unused bits"));
            }
        }
    }
    Ok(out)
}

pub fn decode_base64(
    text: &str,
    allow_whitespace: bool,
    allow_missing_padding: bool,
) -> Result<Vec<u8>, String> {
    let scanned = scan(
        text,
        "base64",
        "standard base64",
        false,
        allow_whitespace,
        true,
        false,
    )?;
    decode_quanta("base64", &scanned, allow_missing_padding, 64, 0x3f)
}

pub fn decode_base64url(
    text: &str,
    allow_whitespace: bool,
    allow_padding: bool,
) -> Result<Vec<u8>, String> {
    let scanned = scan(
        text,
        "base64url",
        "URL-safe base64",
        true,
        allow_whitespace,
        allow_padding,
        false,
    )?;
    decode_quanta("base64url", &scanned, !allow_padding, 64, 0x3f)
}

pub fn decode_base32(
    text: &str,
    allow_whitespace: bool,
    allow_missing_padding: bool,
    allow_lowercase: bool,
) -> Result<Vec<u8>, String> {
    let scanned = scan(
        text,
        "base32",
        "base32",
        false,
        allow_whitespace,
        true,
        allow_lowercase,
    )?;
    decode_quanta("base32", &scanned, allow_missing_padding, 32, 0x1f)
}
