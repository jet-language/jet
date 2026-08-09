// D-UUIDENC1=A: pure base encoding value kernels.

const JET_B64_CHARS: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub(crate) fn jet_std_hex_decode(text: &String) -> Result<Vec<u8>, String> {
    let s = text.trim();
    if s.len() % 2 != 0 {
        return Err(format!("hex string has odd length ({})", s.len()));
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for i in (0..s.len()).step_by(2) {
        match u8::from_str_radix(&s[i..i + 2], 16) {
            Ok(byte) => out.push(byte),
            Err(_) => return Err(format!("invalid hex at offset {}: {:?}", i, &s[i..i + 2])),
        }
    }
    Ok(out)
}

pub(crate) fn jet_std_hex_encode(bytes: &Vec<u8>) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
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

pub(crate) fn jet_std_b64url_encode(bytes: &Vec<u8>) -> String {
    jet_std_b64_encode(bytes)
        .trim_end_matches('=')
        .replace('+', "-")
        .replace('/', "_")
}

const JET_BASE32_CHARS: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

pub(crate) fn jet_std_base32_encode(bytes: &Vec<u8>) -> String {
    let mut out = String::new();
    let mut buffer: u32 = 0;
    let mut bits = 0u8;
    for &byte in bytes {
        buffer = (buffer << 8) | byte as u32;
        bits += 8;
        while bits >= 5 {
            let index = ((buffer >> (bits - 5)) & 31) as usize;
            out.push(JET_BASE32_CHARS[index] as char);
            bits -= 5;
        }
    }
    if bits > 0 {
        let index = ((buffer << (5 - bits)) & 31) as usize;
        out.push(JET_BASE32_CHARS[index] as char);
    }
    while out.len() % 8 != 0 {
        out.push('=');
    }
    out
}
