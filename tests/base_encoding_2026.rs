mod common;

use jet_foundation::XmlPull::base_encoding_2026;

const BASE64: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn digit(byte: u8) -> Option<u8> {
    BASE64.iter().position(|&item| item == byte).map(|n| n as u8)
}

fn frozen_aot_base64(text: &str) -> Option<Vec<u8>> {
    let input: Vec<u8> = text.bytes().filter(|&b| !b.is_ascii_whitespace()).collect();
    if input.len() % 4 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    for chunk in input.chunks(4) {
        let a = digit(chunk[0])?;
        let b = digit(chunk[1])?;
        out.push((a << 2) | (b >> 4));
        if chunk[2] != b'=' {
            let c = digit(chunk[2])?;
            out.push((b << 4) | (c >> 2));
            if chunk[3] != b'=' {
                let d = digit(chunk[3])?;
                out.push((c << 6) | d);
            }
        }
    }
    Some(out)
}

fn frozen_comptime_base64(text: &str) -> Option<Vec<u8>> {
    let input = text.trim_end_matches('=').as_bytes();
    if input.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    for chunk in input.chunks(4) {
        let values: Vec<u8> = chunk
            .iter()
            .map(|&byte| digit(byte))
            .collect::<Option<_>>()?;
        out.push((values[0] << 2) | (values.get(1).copied().unwrap_or(0) >> 4));
        if values.len() > 2 {
            out.push((values[1] << 4) | (values[2] >> 2));
        }
        if values.len() > 3 {
            out.push((values[2] << 6) | values[3]);
        }
    }
    Some(out)
}

fn url_input(text: &str) -> String {
    let mut input = text.trim().replace('-', "+").replace('_', "/");
    while input.len() % 4 != 0 {
        input.push('=');
    }
    input
}

fn frozen_base32(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut buffer = 0u32;
    let mut bits = 0u8;
    for byte in text.bytes().filter(|&b| !b.is_ascii_whitespace() && b != b'=') {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a',
            b'2'..=b'7' => byte - b'2' + 26,
            _ => return None,
        };
        buffer = (buffer << 5) | value as u32;
        bits += 5;
        if bits >= 8 {
            out.push(((buffer >> (bits - 8)) & 0xff) as u8);
            bits -= 8;
        }
    }
    Some(out)
}

fn strings(alphabet: &[u8], max_len: usize, mut check: impl FnMut(&str)) {
    fn visit(alphabet: &[u8], remaining: usize, input: &mut Vec<u8>, check: &mut impl FnMut(&str)) {
        check(std::str::from_utf8(input).unwrap());
        if remaining == 0 {
            return;
        }
        for &byte in alphabet {
            input.push(byte);
            visit(alphabet, remaining - 1, input, check);
            input.pop();
        }
    }
    visit(alphabet, max_len, &mut Vec::with_capacity(max_len), &mut check);
}

fn check_base64_case(text: &str) {
    let aot = frozen_aot_base64(text);
    let comptime = frozen_comptime_base64(text);
    if let (Some(aot), Some(comptime)) = (&aot, &comptime) {
        assert_eq!(aot, comptime, "historical base64 disagreement for {text:?}");
    }
    assert_eq!(
        base_encoding_2026::decode_base64(text).ok().as_ref(),
        aot.as_ref().or(comptime.as_ref()),
        "base64 union mismatch for {text:?}"
    );

    let prepared = url_input(text);
    let aot = frozen_aot_base64(&prepared);
    let comptime = frozen_comptime_base64(&prepared);
    if let (Some(aot), Some(comptime)) = (&aot, &comptime) {
        assert_eq!(aot, comptime, "historical base64url disagreement for {text:?}");
    }
    assert_eq!(
        base_encoding_2026::decode_base64url(text).ok().as_ref(),
        aot.as_ref().or(comptime.as_ref()),
        "base64url union mismatch for {text:?}"
    );
}

#[test]
fn exhaustive_short_inputs_match_frozen_2026_engines_without_disagreement() {
    let mut full = BASE64.to_vec();
    full.extend_from_slice(b"= \n-_*.");
    strings(&full, 3, check_base64_case);
    strings(b"ABPQfgvw09+/= \n-_*.", 4, check_base64_case);

    let mut base32 = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz234567".to_vec();
    base32.extend_from_slice(b"= \n01*.");
    strings(&base32, 3, |text| {
        assert_eq!(
            base_encoding_2026::decode_base32(text).ok(),
            frozen_base32(text),
            "base32 historical mismatch for {text:?}"
        );
    });
    strings(b"ABPZabpz237= \n01*.", 4, |text| {
        assert_eq!(
            base_encoding_2026::decode_base32(text).ok(),
            frozen_base32(text),
            "base32 structural mismatch for {text:?}"
        );
    });
}

#[test]
fn rfc4648_base64_base64url_and_base32_vectors_decode() {
    for (plain, base64, base32) in [
        (b"".as_slice(), "", ""),
        (b"f".as_slice(), "Zg==", "MY======"),
        (b"fo".as_slice(), "Zm8=", "MZXQ===="),
        (b"foo".as_slice(), "Zm9v", "MZXW6==="),
        (b"foob".as_slice(), "Zm9vYg==", "MZXW6YQ="),
        (b"fooba".as_slice(), "Zm9vYmE=", "MZXW6YTB"),
        (b"foobar".as_slice(), "Zm9vYmFy", "MZXW6YTBOI======"),
    ] {
        assert_eq!(base_encoding_2026::decode_base64(base64).unwrap(), plain);
        assert_eq!(
            base_encoding_2026::decode_base64url(base64.trim_end_matches('=')).unwrap(),
            plain
        );
        assert_eq!(base_encoding_2026::decode_base32(base32).unwrap(), plain);
    }
    assert_eq!(base_encoding_2026::decode_base64("+/8=").unwrap(), [251, 255]);
    assert_eq!(base_encoding_2026::decode_base64url("-_8").unwrap(), [251, 255]);
}
