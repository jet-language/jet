// ── D-UUIDENC1=A: core.encoding.hex / core.encoding.base64 / core.uuid ───────
// Pure std implementations; zero external crates (I6); memory-safe (I1).

fn jet_std_hex_encode(bytes: &Vec<u8>) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn jet_std_hex_decode(text: &String) -> Result<Vec<u8>, String> {
    let s = text.trim();
    if s.len() % 2 != 0 {
        return Err(format!("hex string has odd length ({})", s.len()));
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for i in (0..s.len()).step_by(2) {
        match u8::from_str_radix(&s[i..i + 2], 16) {
            Ok(b) => out.push(b),
            Err(_) => return Err(format!("invalid hex at offset {}: {:?}", i, &s[i..i + 2])),
        }
    }
    Ok(out)
}

const JET_B64_CHARS: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn jet_std_b64_encode(bytes: &Vec<u8>) -> String {
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

fn jet_b64_val(b: u8) -> Option<u8> {
    match b {
        b'A'..=b'Z' => Some(b - b'A'),
        b'a'..=b'z' => Some(b - b'a' + 26),
        b'0'..=b'9' => Some(b - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

// D-ENCBASE-STRICT1=A: edition 2026 accepts the union of the two historical
// decoders. Edition 2027 strict defaults remain gated on toolchain support.
fn jet_b64_decode_2026_aot(text: &str) -> Option<Vec<u8>> {
    let input: Vec<u8> = text.bytes().filter(|&b| !b.is_ascii_whitespace()).collect();
    if input.len() % 4 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    for chunk in input.chunks(4) {
        let a = jet_b64_val(chunk[0])?;
        let b = jet_b64_val(chunk[1])?;
        out.push(((a << 2) | (b >> 4)) as u8);
        if chunk[2] != b'=' {
            let c = jet_b64_val(chunk[2])?;
            out.push(((b << 4) | (c >> 2)) as u8);
            if chunk[3] != b'=' {
                let d = jet_b64_val(chunk[3])?;
                out.push(((c << 6) | d) as u8);
            }
        }
    }
    Some(out)
}

fn jet_b64_decode_2026_comptime(text: &str) -> Option<Vec<u8>> {
    let input = text.trim_end_matches('=').as_bytes();
    if input.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    for chunk in input.chunks(4) {
        let digits: Vec<u8> = chunk
            .iter()
            .map(|&byte| jet_b64_val(byte))
            .collect::<Option<_>>()?;
        out.push((digits[0] << 2) | (digits.get(1).copied().unwrap_or(0) >> 4));
        if digits.len() > 2 {
            out.push((digits[1] << 4) | (digits[2] >> 2));
        }
        if digits.len() > 3 {
            out.push((digits[2] << 6) | digits[3]);
        }
    }
    Some(out)
}

fn jet_b64_2026_error(text: &str, url: bool) -> String {
    let label = if url { "base64url" } else { "base64" };
    let alphabet = if url {
        "URL-safe base64"
    } else {
        "standard base64"
    };
    for (offset, &byte) in text.as_bytes().iter().enumerate() {
        let accepted = byte.is_ascii_alphanumeric()
            || matches!(byte, b'=' | b'+' | b'/')
            || (url && matches!(byte, b'-' | b'_'))
            || byte.is_ascii_whitespace();
        if !accepted {
            return format!(
                "invalid {label} at byte {offset}: byte 0x{byte:02X} is not in the {alphabet} alphabet"
            );
        }
    }
    if let Some(offset) = text.as_bytes().iter().position(|&byte| byte == b'=') {
        if text.as_bytes()[offset + 1..]
            .iter()
            .any(|&byte| byte != b'=' && !byte.is_ascii_whitespace())
        {
            return format!(
                "invalid {label} at byte {offset}: padding may appear only at the end"
            );
        }
    }
    format!(
        "invalid {label} at byte {}: encoded length cannot represent whole bytes",
        text.len()
    )
}

fn jet_b64_decode_2026(text: &str, url: bool) -> Result<Vec<u8>, String> {
    let prepared = if url {
        let mut value = text.trim().replace('-', "+").replace('_', "/");
        while value.len() % 4 != 0 {
            value.push('=');
        }
        value
    } else {
        text.to_string()
    };
    match (
        jet_b64_decode_2026_aot(&prepared),
        jet_b64_decode_2026_comptime(&prepared),
    ) {
        (Some(aot), Some(comptime)) if aot != comptime => Err(format!(
            "invalid {} at byte {}: historical decoders disagree",
            if url { "base64url" } else { "base64" },
            text.len()
        )),
        (Some(bytes), _) | (_, Some(bytes)) => Ok(bytes),
        (None, None) => Err(jet_b64_2026_error(text, url)),
    }
}

fn jet_std_b64_decode(text: &String) -> Result<Vec<u8>, String> {
    jet_b64_decode_2026(text, false)
}

fn jet_std_b64url_encode(bytes: &Vec<u8>) -> String {
    jet_std_b64_encode(bytes)
        .trim_end_matches('=')
        .replace('+', "-")
        .replace('/', "_")
}
fn jet_std_b64url_decode(text: &String) -> Result<Vec<u8>, String> {
    jet_b64_decode_2026(text, true)
}

const JET_BASE32_CHARS: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
fn jet_std_base32_encode(bytes: &Vec<u8>) -> String {
    let mut out = String::new();
    let mut buffer: u32 = 0;
    let mut bits = 0u8;
    for &b in bytes {
        buffer = (buffer << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            let idx = ((buffer >> (bits - 5)) & 31) as usize;
            out.push(JET_BASE32_CHARS[idx] as char);
            bits -= 5;
        }
    }
    if bits > 0 {
        let idx = ((buffer << (5 - bits)) & 31) as usize;
        out.push(JET_BASE32_CHARS[idx] as char);
    }
    while out.len() % 8 != 0 {
        out.push('=');
    }
    out
}
fn jet_base32_val(b: u8, offset: usize) -> Result<u8, String> {
    match b {
        b'A'..=b'Z' => Ok(b - b'A'),
        b'a'..=b'z' => Ok(b - b'a'),
        b'2'..=b'7' => Ok(b - b'2' + 26),
        _ => Err(format!(
            "invalid base32 at byte {offset}: byte 0x{b:02X} is not in the base32 alphabet"
        )),
    }
}
fn jet_std_base32_decode(text: &String) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut buffer: u32 = 0;
    let mut bits = 0u8;
    for (offset, b) in text.bytes().enumerate() {
        if b.is_ascii_whitespace() || b == b'=' {
            continue;
        }
        buffer = (buffer << 5) | jet_base32_val(b, offset)? as u32;
        bits += 5;
        if bits >= 8 {
            out.push(((buffer >> (bits - 8)) & 0xff) as u8);
            bits -= 8;
        }
    }
    Ok(out)
}

fn jet_xml_to_data_tree(value: crate::jet_xml_pull::Value) -> jet_std::DataTree {
    match value {
        crate::jet_xml_pull::Value::Null => jet_std::DataTree::Null,
        crate::jet_xml_pull::Value::Bool(value) => jet_std::DataTree::Bool(value),
        crate::jet_xml_pull::Value::Int(value) => jet_std::DataTree::Int(value),
        crate::jet_xml_pull::Value::Text(value) => jet_std::DataTree::Text(value),
        crate::jet_xml_pull::Value::Array(values) => jet_std::DataTree::Array(
            values.into_iter().map(jet_xml_to_data_tree).collect(),
        ),
        crate::jet_xml_pull::Value::Object(entries) => jet_std::DataTree::Object(
            entries
                .into_iter()
                .map(|(key, value)| (key, jet_xml_to_data_tree(value)))
                .collect(),
        ),
    }
}

fn jet_xml_from_data_tree(value: &jet_std::DataTree) -> Result<crate::jet_xml_pull::Value, String> {
    match value {
        jet_std::DataTree::Null => Ok(crate::jet_xml_pull::Value::Null),
        jet_std::DataTree::Bool(value) => Ok(crate::jet_xml_pull::Value::Bool(*value)),
        jet_std::DataTree::Int(value) => Ok(crate::jet_xml_pull::Value::Int(*value)),
        jet_std::DataTree::Text(value) => Ok(crate::jet_xml_pull::Value::Text(value.clone())),
        jet_std::DataTree::Array(values) => Ok(crate::jet_xml_pull::Value::Array(
            values
                .iter()
                .map(jet_xml_from_data_tree)
                .collect::<Result<Vec<_>, _>>()?,
        )),
        jet_std::DataTree::Object(entries) => Ok(crate::jet_xml_pull::Value::Object(
            entries
                .iter()
                .map(|(key, value)| Ok((key.clone(), jet_xml_from_data_tree(value)?)))
                .collect::<Result<Vec<_>, String>>()?,
        )),
        jet_std::DataTree::Float(_) | jet_std::DataTree::Bytes(_) => {
            Err("XML tree cannot contain Float or Bytes values".to_string())
        }
    }
}

fn jet_xml_reason(reason: crate::jet_xml_pull::Reason) -> jet_std::XMLReason {
    use crate::jet_xml_pull::Reason as Source;
    match reason {
        Source::InvalidEncoding => jet_std::XMLReason::InvalidEncoding,
        Source::Malformed => jet_std::XMLReason::Malformed,
        Source::MismatchedTag => jet_std::XMLReason::MismatchedTag,
        Source::InvalidName => jet_std::XMLReason::InvalidName,
        Source::Namespace => jet_std::XMLReason::Namespace,
        Source::DuplicateAttribute => jet_std::XMLReason::DuplicateAttribute,
        Source::Entity => jet_std::XMLReason::Entity,
        Source::EntityCycle => jet_std::XMLReason::EntityCycle,
        Source::Limit => jet_std::XMLReason::Limit,
        Source::Canonicalization => jet_std::XMLReason::Canonicalization,
        Source::Shape => jet_std::XMLReason::Shape,
        Source::Unsupported => jet_std::XMLReason::Unsupported,
    }
}

fn jet_xml_error(error: crate::jet_xml_pull::Error) -> jet_std::XMLError {
    jet_std::XMLError {
        kind: jet_xml_reason(error.kind),
        byte_offset: error.line.map(|_| error.offset as i64),
        line: error.line.map(|value| value as i64),
        column: error.column.map(|value| value as i64),
        path: error.path,
        reason: error.reason,
    }
}

fn jet_xml_options(options: &jet_std::XMLParseOptions) -> crate::jet_xml_pull::ParseOptions {
    let number = |value: i64| usize::try_from(value).unwrap_or(usize::MAX);
    let entities = match &options.entities {
        jet_std::XMLEntityPolicy::Preserve => crate::jet_xml_pull::EntityPolicy::Preserve,
        jet_std::XMLEntityPolicy::Reject => crate::jet_xml_pull::EntityPolicy::Reject,
        jet_std::XMLEntityPolicy::Resolve(values) => crate::jet_xml_pull::EntityPolicy::Resolve(values.clone()),
    };
    crate::jet_xml_pull::ParseOptions {
        entities,
        limits: crate::jet_xml_pull::Limits {
            max_depth: number(options.limits.max_depth),
            max_nodes: number(options.limits.max_nodes),
            max_attributes_per_element: number(options.limits.max_attributes_per_element),
            max_name_bytes: number(options.limits.max_name_bytes),
            max_text_bytes: number(options.limits.max_text_bytes),
            max_entity_declarations: number(options.limits.max_entity_declarations),
            max_entity_depth: number(options.limits.max_entity_depth),
            max_entity_replacement_bytes: number(options.limits.max_entity_replacement_bytes),
        },
    }
}

fn jet_std_xml_parse(text: &String) -> Result<jet_std::DataTree, jet_std::XMLError> {
    crate::jet_xml_pull::parse_document(text).map(jet_xml_to_data_tree).map_err(jet_xml_error)
}

fn jet_std_xml_parse_with(text: &String, options: &jet_std::XMLParseOptions) -> Result<jet_std::DataTree, jet_std::XMLError> {
    crate::jet_xml_pull::parse_document_with(text, &jet_xml_options(options))
        .map(jet_xml_to_data_tree)
        .map_err(jet_xml_error)
}

fn jet_std_xml_render(d: &jet_std::DataTree) -> String {
    jet_xml_from_data_tree(d)
        .and_then(|value| crate::jet_xml_pull::render_document(&value))
        .unwrap_or_default()
}

fn jet_std_xml_canonical(d: &jet_std::DataTree, options: &jet_std::XMLCanonical) -> Result<String, jet_std::XMLError> {
    let value = jet_xml_from_data_tree(d).map_err(|reason| jet_std::XMLError {
        kind: jet_std::XMLReason::Shape,
        byte_offset: None,
        line: None,
        column: None,
        path: String::new(),
        reason,
    })?;
    let mode = match options.mode {
        jet_std::XMLCanonicalMode::Inclusive11 => crate::jet_xml_pull::CanonicalMode::Inclusive11,
        jet_std::XMLCanonicalMode::Exclusive10 => crate::jet_xml_pull::CanonicalMode::Exclusive10,
    };
    crate::jet_xml_pull::canonical_document(&value, &crate::jet_xml_pull::CanonicalOptions {
        mode,
        comments: options.comments,
        inclusive_prefixes: options.inclusive_prefixes.clone(),
    }).map_err(jet_xml_error)
}

fn jet_cbor_push_len(out: &mut Vec<u8>, major: u8, n: u64) {
    if n < 24 { out.push((major << 5) | n as u8); }
    else if n <= u8::MAX as u64 { out.extend_from_slice(&[(major << 5) | 24, n as u8]); }
    else if n <= u16::MAX as u64 { out.push((major << 5) | 25); out.extend_from_slice(&(n as u16).to_be_bytes()); }
    else if n <= u32::MAX as u64 { out.push((major << 5) | 26); out.extend_from_slice(&(n as u32).to_be_bytes()); }
    else { out.push((major << 5) | 27); out.extend_from_slice(&n.to_be_bytes()); }
}
fn jet_cbor_error(kind: jet_std::CBORErrorKind, offset: usize, path: &str, reason: impl Into<String>) -> jet_std::CBORError {
    jet_std::CBORError { kind, byte_offset: offset as i64, path: path.to_string(), reason: reason.into() }
}
fn jet_cbor_encode_val(v: &jet_std::DataTree, out: &mut Vec<u8>, canonical: bool) -> Result<(), jet_std::CBORError> {
    match v {
        jet_std::DataTree::Null => out.push(0xf6),
        jet_std::DataTree::Bool(false) => out.push(0xf4),
        jet_std::DataTree::Bool(true) => out.push(0xf5),
        jet_std::DataTree::Int(n) if *n >= 0 => jet_cbor_push_len(out, 0, *n as u64),
        jet_std::DataTree::Int(n) => jet_cbor_push_len(out, 1, (-1 - *n) as u64),
        jet_std::DataTree::Float(f) => jet_cbor_push_preferred_float(out, *f),
        jet_std::DataTree::Text(s) => { jet_cbor_push_len(out, 3, s.len() as u64); out.extend_from_slice(s.as_bytes()); }
        jet_std::DataTree::Bytes(bs) => { jet_cbor_push_len(out, 2, bs.len() as u64); out.extend_from_slice(bs); }
        jet_std::DataTree::Array(xs) => { jet_cbor_push_len(out, 4, xs.len() as u64); for x in xs { jet_cbor_encode_val(x, out, canonical)?; } }
        jet_std::DataTree::Object(es) => {
            let mut encoded = Vec::with_capacity(es.len());
            for (k, v) in es {
                let mut key = Vec::new();
                jet_cbor_encode_val(&jet_std::DataTree::Text(k.clone()), &mut key, canonical)?;
                let mut value = Vec::new();
                jet_cbor_encode_val(v, &mut value, canonical)?;
                encoded.push((key, value));
            }
            if canonical { encoded.sort_by(|a, b| a.0.cmp(&b.0)); }
            if encoded.windows(2).any(|pair| pair[0].0 == pair[1].0) {
                return Err(jet_cbor_error(jet_std::CBORErrorKind::Unsupported, 0, "$", "duplicate encoded CBOR map key"));
            }
            jet_cbor_push_len(out, 5, encoded.len() as u64);
            for (key, value) in encoded { out.extend_from_slice(&key); out.extend_from_slice(&value); }
        }
    }
    Ok(())
}
fn jet_enc_cbor_to_bytes<T: user_Encode>(value: &T) -> Result<Vec<u8>, jet_std::CBORError> {
    let mut out = Vec::new();
    jet_cbor_encode_val(&value.jet_encode(), &mut out, false)?;
    Ok(out)
}
fn jet_enc_cbor_to_bytes_canonical<T: user_Encode>(value: &T) -> Result<Vec<u8>, jet_std::CBORError> {
    let mut out = Vec::new();
    jet_cbor_encode_val(&value.jet_encode(), &mut out, true)?;
    Ok(out)
}
fn jet_cbor_read_len(input: &[u8], i: &mut usize, add: u8, start: usize, canonical: bool, path: &str) -> Result<u64, jet_std::CBORError> {
    let need = match add { n @ 0..=23 => return Ok(n as u64), 24 => 1, 25 => 2, 26 => 4, 27 => 8, _ => return Err(jet_cbor_error(jet_std::CBORErrorKind::Unsupported, start, path, "indefinite/reserved CBOR length is unsupported by whole-value decoding")) };
    if *i + need > input.len() { return Err(jet_cbor_error(jet_std::CBORErrorKind::Truncated, input.len(), path, "CBOR length argument is truncated")); }
    let mut n = 0u64;
    for _ in 0..need { n = (n << 8) | input[*i] as u64; *i += 1; }
    if canonical && ((add == 24 && n < 24) || (add == 25 && n <= u8::MAX as u64) || (add == 26 && n <= u16::MAX as u64) || (add == 27 && n <= u32::MAX as u64)) {
        return Err(jet_cbor_error(jet_std::CBORErrorKind::NonCanonical, start, path, "CBOR argument does not use its shortest form"));
    }
    Ok(n)
}
struct JetCborAllocBudget { limit: usize, live: usize, peak: usize }
impl JetCborAllocBudget {
    fn new(limit: i64) -> Self { Self { limit: limit as usize, live: 0, peak: 0 } }
    fn reserve(&mut self, count: usize, unit: usize, offset: usize, path: &str, what: &str) -> Result<usize, jet_std::CBORError> {
        let available = self.limit - self.live;
        if unit != 0 && count > available / unit {
            return Err(jet_cbor_error(jet_std::CBORErrorKind::Limit, offset, path, format!("{what} allocation exceeds max_bytes {}", self.limit)));
        }
        let requested = count * unit;
        self.live += requested;
        self.peak = self.peak.max(self.live);
        Ok(requested)
    }
    fn release(&mut self, requested: usize) { self.live -= requested; }
}
fn jet_cbor_index_path(path: &str, index: usize, budget: &mut JetCborAllocBudget, offset: usize) -> Result<(String, usize), jet_std::CBORError> {
    let mut digits = [0u8; 20];
    let mut cursor = digits.len();
    let mut n = index;
    loop { cursor -= 1; digits[cursor] = b'0' + (n % 10) as u8; n /= 10; if n == 0 { break; } }
    let capacity = path.len() + 2 + digits.len() - cursor;
    let charged = budget.reserve(capacity, 1, offset, path, "CBOR path")?;
    let mut out = String::with_capacity(capacity);
    out.push_str(path); out.push('['); out.push_str(std::str::from_utf8(&digits[cursor..]).unwrap()); out.push(']');
    Ok((out, charged))
}
fn jet_cbor_key_path(path: &str, key: &str, budget: &mut JetCborAllocBudget, offset: usize) -> Result<(String, usize), jet_std::CBORError> {
    let escaped = key.chars().map(|c| c.escape_debug().map(|x| x.len_utf8()).sum::<usize>()).sum::<usize>();
    let capacity = path.len().checked_add(escaped).and_then(|n| n.checked_add(4)).ok_or_else(|| jet_cbor_error(jet_std::CBORErrorKind::Limit, offset, path, "CBOR path allocation exceeds target capacity"))?;
    let charged = budget.reserve(capacity, 1, offset, path, "CBOR path")?;
    let mut out = String::with_capacity(capacity);
    out.push_str(path); out.push('['); out.push('"'); for c in key.chars() { out.extend(c.escape_debug()); } out.push('"'); out.push(']');
    Ok((out, charged))
}
fn jet_cbor_count_item(items: &mut i64, options: &jet_std::CBOROptions, offset: usize, path: &str) -> Result<(), jet_std::CBORError> {
    *items = items.checked_add(1).ok_or_else(|| jet_cbor_error(jet_std::CBORErrorKind::Limit, offset, path, "max_items counter overflow"))?;
    if *items > options.max_items { return Err(jet_cbor_error(jet_std::CBORErrorKind::Limit, offset, path, format!("max_items {} exceeded", options.max_items))); }
    Ok(())
}
fn jet_cbor_indefinite_error(options: &jet_std::CBOROptions, offset: usize, path: &str) -> Result<(), jet_std::CBORError> {
    if options.require_canonical {
        Err(jet_cbor_error(jet_std::CBORErrorKind::NonCanonical, offset, path, "indefinite-length CBOR is not Core deterministic"))
    } else {
        Ok(())
    }
}
fn jet_cbor_decode_indefinite_string(input: &[u8], i: &mut usize, options: &jet_std::CBOROptions, budget: &mut JetCborAllocBudget, depth: i64, items: &mut i64, path: &str, major: u8, start: usize, allow_bytes: bool) -> Result<jet_std::DataTree, jet_std::CBORError> {
    jet_cbor_indefinite_error(options, start, path)?;
    if depth + 1 > options.max_depth { return Err(jet_cbor_error(jet_std::CBORErrorKind::Limit, start, path, format!("max_depth {} exceeded", options.max_depth))); }
    if major == 2 && !allow_bytes { return Err(jet_cbor_error(jet_std::CBORErrorKind::Unsupported, start, path, "CBOR byte strings are outside core.encoding.Data; use decode<[U8]>")); }
    let mut bytes = Vec::new();
    loop {
        if *i >= input.len() { return Err(jet_cbor_error(jet_std::CBORErrorKind::Truncated, input.len(), path, "indefinite CBOR string ended before its break")); }
        if input[*i] == 0xff { *i += 1; break; }
        let chunk_start = *i;
        let head = input[*i]; *i += 1;
        let chunk_major = head >> 5; let chunk_add = head & 31;
        jet_cbor_count_item(items, options, chunk_start, path)?;
        if chunk_major != major || chunk_add == 31 {
            return Err(jet_cbor_error(jet_std::CBORErrorKind::Syntax, chunk_start, path, "indefinite CBOR string contains a wrong or indefinite chunk"));
        }
        let n = usize::try_from(jet_cbor_read_len(input, i, chunk_add, chunk_start, false, path)?).map_err(|_| jet_cbor_error(jet_std::CBORErrorKind::Limit, chunk_start, path, "CBOR string chunk length exceeds target capacity"))?;
        if n > input.len() - *i { return Err(jet_cbor_error(jet_std::CBORErrorKind::Truncated, input.len(), path, "CBOR byte/text string chunk is truncated")); }
        if major == 3 && std::str::from_utf8(&input[*i..*i + n]).is_err() { return Err(jet_cbor_error(jet_std::CBORErrorKind::Syntax, chunk_start, path, "CBOR text chunk is not UTF-8")); }
        budget.reserve(n, 1, chunk_start, path, if major == 2 { "CBOR byte string" } else { "CBOR text string" })?;
        bytes.try_reserve_exact(n).map_err(|_| jet_cbor_error(jet_std::CBORErrorKind::Limit, chunk_start, path, "CBOR string allocation failed"))?;
        bytes.extend_from_slice(&input[*i..*i + n]); *i += n;
    }
    if major == 2 { Ok(jet_std::DataTree::Bytes(bytes)) }
    else { String::from_utf8(bytes).map(jet_std::DataTree::Text).map_err(|_| jet_cbor_error(jet_std::CBORErrorKind::Syntax, start, path, "CBOR text is not UTF-8")) }
}
fn jet_cbor_decode_val(input: &[u8], i: &mut usize, options: &jet_std::CBOROptions, budget: &mut JetCborAllocBudget, depth: i64, items: &mut i64, path: &str, allow_bytes: bool) -> Result<jet_std::DataTree, jet_std::CBORError> {
    if *i >= input.len() { return Err(jet_cbor_error(jet_std::CBORErrorKind::Truncated, input.len(), path, "CBOR value is missing")); }
    let start = *i; let b = input[*i]; *i += 1;
    jet_cbor_count_item(items, options, start, path)?;
    let major = b >> 5; let add = b & 31;
    match major {
        0 => i64::try_from(jet_cbor_read_len(input, i, add, start, options.require_canonical, path)?).map(jet_std::DataTree::Int).map_err(|_| jet_cbor_error(jet_std::CBORErrorKind::Unsupported, start, path, "CBOR integer is outside Jet Int")),
        1 => i64::try_from(jet_cbor_read_len(input, i, add, start, options.require_canonical, path)?).ok().and_then(|n| n.checked_neg()?.checked_sub(1)).map(jet_std::DataTree::Int).ok_or_else(|| jet_cbor_error(jet_std::CBORErrorKind::Unsupported, start, path, "CBOR integer is outside Jet Int")),
        2 | 3 => {
            if add == 31 { return jet_cbor_decode_indefinite_string(input, i, options, budget, depth, items, path, major, start, allow_bytes); }
            let n = usize::try_from(jet_cbor_read_len(input, i, add, start, options.require_canonical, path)?).map_err(|_| jet_cbor_error(jet_std::CBORErrorKind::Limit, start, path, "CBOR string length exceeds target capacity"))?;
            if n > input.len() - *i { return Err(jet_cbor_error(jet_std::CBORErrorKind::Truncated, input.len(), path, "CBOR byte/text string is truncated")); }
            if major == 2 && !allow_bytes { return Err(jet_cbor_error(jet_std::CBORErrorKind::Unsupported, start, path, "CBOR byte strings are outside core.encoding.Data; use decode<[U8]>")); }
            budget.reserve(n, 1, start, path, if major == 2 { "CBOR byte string" } else { "CBOR text string" })?;
            let mut bytes = Vec::with_capacity(n); bytes.extend_from_slice(&input[*i..*i + n]); *i += n;
            if major == 2 { if allow_bytes { Ok(jet_std::DataTree::Bytes(bytes)) } else { Err(jet_cbor_error(jet_std::CBORErrorKind::Unsupported, start, path, "CBOR byte strings are outside core.encoding.Data; use decode<[U8]>") ) } } else { String::from_utf8(bytes).map(jet_std::DataTree::Text).map_err(|_| jet_cbor_error(jet_std::CBORErrorKind::Syntax, start, path, "CBOR text is not UTF-8")) }
        }
        4 => {
            if add == 31 {
                jet_cbor_indefinite_error(options, start, path)?;
                if depth + 1 > options.max_depth { return Err(jet_cbor_error(jet_std::CBORErrorKind::Limit, start, path, format!("max_depth {} exceeded", options.max_depth))); }
                let mut xs = Vec::new();
                let mut index = 0usize;
                loop {
                    if *i >= input.len() { return Err(jet_cbor_error(jet_std::CBORErrorKind::Truncated, input.len(), path, "indefinite CBOR array ended before its break")); }
                    if input[*i] == 0xff { *i += 1; break; }
                    let (child_path, charged) = jet_cbor_index_path(path, index, budget, *i)?;
                    if *items >= options.max_items {
                        let error = jet_cbor_error(jet_std::CBORErrorKind::Limit, *i, &child_path, format!("max_items {} exceeded", options.max_items));
                        budget.release(charged);
                        return Err(error);
                    }
                    budget.reserve(1, std::mem::size_of::<jet_std::DataTree>(), *i, &child_path, "CBOR array")?;
                    xs.try_reserve_exact(1).map_err(|_| jet_cbor_error(jet_std::CBORErrorKind::Limit, *i, &child_path, "CBOR array allocation failed"))?;
                    let child = jet_cbor_decode_val(input, i, options, budget, depth + 1, items, &child_path, allow_bytes);
                    budget.release(charged);
                    xs.push(child?); index += 1;
                }
                return Ok(jet_std::DataTree::Array(xs));
            }
            if depth + 1 > options.max_depth { return Err(jet_cbor_error(jet_std::CBORErrorKind::Limit, start, path, format!("max_depth {} exceeded", options.max_depth))); }
            let n = usize::try_from(jet_cbor_read_len(input, i, add, start, options.require_canonical, path)?).map_err(|_| jet_cbor_error(jet_std::CBORErrorKind::Limit, start, path, "CBOR array length exceeds target capacity"))?;
            budget.reserve(n, std::mem::size_of::<jet_std::DataTree>(), start, path, "CBOR array")?;
            let mut xs = Vec::with_capacity(n);
            for index in 0..n {
                let (child_path, charged) = jet_cbor_index_path(path, index, budget, start)?;
                let child = jet_cbor_decode_val(input, i, options, budget, depth + 1, items, &child_path, allow_bytes);
                budget.release(charged);
                xs.push(child?);
            }
            Ok(jet_std::DataTree::Array(xs))
        }
        5 => {
            if add == 31 {
                jet_cbor_indefinite_error(options, start, path)?;
                if depth + 1 > options.max_depth { return Err(jet_cbor_error(jet_std::CBORErrorKind::Limit, start, path, format!("max_depth {} exceeded", options.max_depth))); }
                let mut es = Vec::new();
                loop {
                    if *i >= input.len() { return Err(jet_cbor_error(jet_std::CBORErrorKind::Truncated, input.len(), path, "indefinite CBOR map ended before its break")); }
                    if input[*i] == 0xff { *i += 1; break; }
                    let key_start = *i;
                    if *items >= options.max_items { return Err(jet_cbor_error(jet_std::CBORErrorKind::Limit, key_start, path, format!("max_items {} exceeded", options.max_items))); }
                    budget.reserve(1, std::mem::size_of::<(String, jet_std::DataTree)>(), key_start, path, "CBOR map")?;
                    es.try_reserve_exact(1).map_err(|_| jet_cbor_error(jet_std::CBORErrorKind::Limit, key_start, path, "CBOR map allocation failed"))?;
                    let k = match jet_cbor_decode_val(input, i, options, budget, depth + 1, items, path, false)? { jet_std::DataTree::Text(s) => s, _ => return Err(jet_cbor_error(jet_std::CBORErrorKind::Unsupported, key_start, path, "CBOR map key must be text")) };
                    if es.iter().any(|(old, _)| old == &k) { return Err(jet_cbor_error(jet_std::CBORErrorKind::Unsupported, key_start, path, "duplicate CBOR text map key")); }
                    if *i >= input.len() { return Err(jet_cbor_error(jet_std::CBORErrorKind::Truncated, input.len(), path, "indefinite CBOR map ended before its value")); }
                    if input[*i] == 0xff { return Err(jet_cbor_error(jet_std::CBORErrorKind::Syntax, *i, path, "indefinite CBOR map break appears where a value is required")); }
                    let (key_path, charged) = jet_cbor_key_path(path, &k, budget, key_start)?;
                    let value = jet_cbor_decode_val(input, i, options, budget, depth + 1, items, &key_path, allow_bytes);
                    budget.release(charged);
                    es.push((k, value?));
                }
                return Ok(jet_std::DataTree::Object(es));
            }
            if depth + 1 > options.max_depth { return Err(jet_cbor_error(jet_std::CBORErrorKind::Limit, start, path, format!("max_depth {} exceeded", options.max_depth))); }
            let n = usize::try_from(jet_cbor_read_len(input, i, add, start, options.require_canonical, path)?).map_err(|_| jet_cbor_error(jet_std::CBORErrorKind::Limit, start, path, "CBOR map length exceeds target capacity"))?;
            budget.reserve(n, std::mem::size_of::<(String, jet_std::DataTree)>(), start, path, "CBOR map")?;
            let mut es = Vec::with_capacity(n);
            let mut prior_key: Option<(usize, usize)> = None;
            for _ in 0..n {
                let key_start = *i;
                let k = match jet_cbor_decode_val(input, i, options, budget, depth + 1, items, path, false)? { jet_std::DataTree::Text(s) => s, _ => return Err(jet_cbor_error(jet_std::CBORErrorKind::Unsupported, key_start, path, "CBOR map key must be text")) };
                let key_end = *i;
                if options.require_canonical && prior_key.is_some_and(|(old_start, old_end)| input[old_start..old_end] >= input[key_start..key_end]) { return Err(jet_cbor_error(jet_std::CBORErrorKind::NonCanonical, key_start, path, "CBOR map keys are not in Core deterministic bytewise order")); }
                if es.iter().any(|(old, _)| old == &k) { return Err(jet_cbor_error(jet_std::CBORErrorKind::Unsupported, key_start, path, "duplicate CBOR text map key")); }
                prior_key = Some((key_start, key_end));
                let (key_path, charged) = jet_cbor_key_path(path, &k, budget, key_start)?;
                let value = jet_cbor_decode_val(input, i, options, budget, depth + 1, items, &key_path, allow_bytes);
                budget.release(charged);
                es.push((k, value?));
            }
            Ok(jet_std::DataTree::Object(es))
        }
        7 => match add {
            20 => Ok(jet_std::DataTree::Bool(false)),
            21 => Ok(jet_std::DataTree::Bool(true)),
            22 => Ok(jet_std::DataTree::Null),
            25 => { if *i + 2 > input.len() { return Err(jet_cbor_error(jet_std::CBORErrorKind::Truncated, input.len(), path, "CBOR Float16 is truncated")); } let bits=u16::from_be_bytes([input[*i],input[*i+1]]);*i+=2;if options.require_canonical && jet_cbor_half_to_f64(bits).is_nan() && bits != 0x7e00 { return Err(jet_cbor_error(jet_std::CBORErrorKind::NonCanonical,start,path,"CBOR NaN is not the canonical 0xf97e00 encoding")); }Ok(jet_std::DataTree::Float(jet_cbor_half_to_f64(bits))) }
            26 => { if *i + 4 > input.len() { return Err(jet_cbor_error(jet_std::CBORErrorKind::Truncated, input.len(), path, "CBOR Float32 is truncated")); } let mut buf=[0u8;4];buf.copy_from_slice(&input[*i..*i+4]);*i+=4;let value=f32::from_be_bytes(buf) as f64;if options.require_canonical && (value.is_nan() || jet_cbor_half_exact(value).is_some()) { return Err(jet_cbor_error(jet_std::CBORErrorKind::NonCanonical,start,path,"CBOR Float does not use its preferred shortest encoding")); }Ok(jet_std::DataTree::Float(value)) }
            27 => { if *i + 8 > input.len() { return Err(jet_cbor_error(jet_std::CBORErrorKind::Truncated, input.len(), path, "CBOR Float64 is truncated")); } let mut buf = [0u8; 8]; buf.copy_from_slice(&input[*i..*i+8]); *i += 8; let value=f64::from_be_bytes(buf);if options.require_canonical && (value.is_nan() || jet_cbor_half_exact(value).is_some() || ((value as f32) as f64).to_bits()==value.to_bits()) { return Err(jet_cbor_error(jet_std::CBORErrorKind::NonCanonical,start,path,"CBOR Float does not use its preferred shortest encoding")); }Ok(jet_std::DataTree::Float(value)) }
            31 => Err(jet_cbor_error(jet_std::CBORErrorKind::Syntax, start, path, "CBOR break outside an indefinite container")),
            _ => Err(jet_cbor_error(jet_std::CBORErrorKind::Unsupported, start, path, format!("unsupported CBOR simple value {add}"))),
        },
        6 => Err(jet_cbor_error(jet_std::CBORErrorKind::Unsupported, start, path, "CBOR tags are unsupported")),
        _ => Err(jet_cbor_error(jet_std::CBORErrorKind::Unsupported, start, path, format!("unsupported CBOR major type {major}"))),
    }
}
fn jet_cbor_validate_options(options: &jet_std::CBOROptions) -> Result<(), jet_std::CBORError> {
    if !(1..=4096).contains(&options.max_depth) { return Err(jet_cbor_error(jet_std::CBORErrorKind::Limit, 0, "$", "max_depth must be in 1..4096")); }
    if !(1..=1_000_000_000).contains(&options.max_items) { return Err(jet_cbor_error(jet_std::CBORErrorKind::Limit, 0, "$", "max_items must be in 1..1000000000")); }
    if !(0..=1_073_741_824).contains(&options.max_bytes) { return Err(jet_cbor_error(jet_std::CBORErrorKind::Limit, 0, "$", "max_bytes must be in 0..1073741824")); }
    Ok(())
}
fn jet_enc_cbor_parse(bytes: &Vec<u8>, options: jet_std::CBOROptions) -> Result<jet_std::DataTree, jet_std::CBORError> {
    jet_cbor_validate_options(&options)?;
    if bytes.len() as i64 > options.max_bytes { return Err(jet_cbor_error(jet_std::CBORErrorKind::Limit, 0, "$", format!("input exceeds max_bytes {}", options.max_bytes))); }
    let mut i = 0usize;
    let mut items = 0i64;
    let mut budget = JetCborAllocBudget::new(options.max_bytes);
    let v = jet_cbor_decode_val(bytes, &mut i, &options, &mut budget, 0, &mut items, "$", false)?;
    if i != bytes.len() { return Err(jet_cbor_error(jet_std::CBORErrorKind::TrailingData, i, "$", "trailing CBOR data after root value")); }
    Ok(v)
}
fn jet_enc_cbor_decode<T: user_Decode>(bytes: &Vec<u8>, options: jet_std::CBOROptions) -> Result<T, jet_std::CBORError> {
    jet_cbor_validate_options(&options)?;
    if bytes.len() as i64 > options.max_bytes { return Err(jet_cbor_error(jet_std::CBORErrorKind::Limit, 0, "$", format!("input exceeds max_bytes {}", options.max_bytes))); }
    let mut i=0usize; let mut items=0i64; let mut budget=JetCborAllocBudget::new(options.max_bytes);
    let tree=jet_cbor_decode_val(bytes,&mut i,&options,&mut budget,0,&mut items,"$",true)?;
    if i!=bytes.len(){return Err(jet_cbor_error(jet_std::CBORErrorKind::TrailingData,i,"$","trailing CBOR data after root value"));}
    T::jet_decode_traced(&tree).map(|(value,_)|value).map_err(|mut error| {
        if error.path.is_empty() { error.path.push('$'); } else { error.path.insert(0, '$'); }
        jet_std::CBORError {
            kind: jet_std::CBORErrorKind::TypeMismatch,
            byte_offset: 0,
            path: error.path,
            reason: error.reason,
        }
    })
}

// UUID helpers — pure std, zero deps. CSPRNG via /dev/urandom (POSIX); the
// fallback SplitMix64 engages only when /dev/urandom is unavailable.
fn jet_uuid_fill_random(out: &mut [u8]) {
    use std::io::Read;
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        if f.read_exact(out).is_ok() {
            return;
        }
    }
    // Fallback: SplitMix64 seeded from wall-clock nanoseconds.
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    let mut state = seed.wrapping_add(0x9E3779B97F4A7C15);
    for b in out.iter_mut() {
        state = state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = (state ^ (state >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        *b = (z ^ (z >> 31)) as u8;
    }
}

fn jet_uuid_format(b: &[u8; 16]) -> String {
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-\
         {:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0],
        b[1],
        b[2],
        b[3],
        b[4],
        b[5],
        b[6],
        b[7],
        b[8],
        b[9],
        b[10],
        b[11],
        b[12],
        b[13],
        b[14],
        b[15]
    )
}

fn jet_std_uuid_v4() -> String {
    let mut bytes = [0u8; 16];
    jet_uuid_fill_random(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant 10xx
    jet_uuid_format(&bytes)
}

fn jet_std_uuid_v7(clock: &jet_std::Clock) -> String {
    let ts_ms = clock.now as u64;
    let mut bytes = [0u8; 16];
    // 48-bit timestamp in the high bytes
    bytes[0] = (ts_ms >> 40) as u8;
    bytes[1] = (ts_ms >> 32) as u8;
    bytes[2] = (ts_ms >> 24) as u8;
    bytes[3] = (ts_ms >> 16) as u8;
    bytes[4] = (ts_ms >> 8) as u8;
    bytes[5] = ts_ms as u8;
    jet_uuid_fill_random(&mut bytes[6..]);
    bytes[6] = (bytes[6] & 0x0f) | 0x70; // version 7
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant 10xx
    jet_uuid_format(&bytes)
}
