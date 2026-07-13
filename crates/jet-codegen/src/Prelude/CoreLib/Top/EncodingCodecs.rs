            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }
    let mut out = [0u8; 32];
    for (i, &s) in state.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&s.to_be_bytes());
    }
    out
}

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

fn jet_b64_val(b: u8) -> Result<u8, String> {
    match b {
        b'A'..=b'Z' => Ok(b - b'A'),
        b'a'..=b'z' => Ok(b - b'a' + 26),
        b'0'..=b'9' => Ok(b - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(format!("invalid base64 character: {:?}", b as char)),
    }
}

fn jet_std_b64_decode(text: &String) -> Result<Vec<u8>, String> {
    let input: Vec<u8> = text.bytes().filter(|&b| !b.is_ascii_whitespace()).collect();
    if input.len() % 4 != 0 {
        return Err(format!(
            "base64 length must be a multiple of 4 (got {})",
            input.len()
        ));
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
    Ok(out)
}

fn jet_std_b64url_encode(bytes: &Vec<u8>) -> String {
    jet_std_b64_encode(bytes)
        .trim_end_matches('=')
        .replace('+', "-")
        .replace('/', "_")
}
fn jet_std_b64url_decode(text: &String) -> Result<Vec<u8>, String> {
    let mut s = text.trim().replace('-', "+").replace('_', "/");
    while s.len() % 4 != 0 {
        s.push('=');
    }
    jet_std_b64_decode(&s)
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
fn jet_base32_val(b: u8) -> Result<u8, String> {
    match b {
        b'A'..=b'Z' => Ok(b - b'A'),
        b'a'..=b'z' => Ok(b - b'a'),
        b'2'..=b'7' => Ok(b - b'2' + 26),
        _ => Err(format!("invalid base32 character: {:?}", b as char)),
    }
}
fn jet_std_base32_decode(text: &String) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut buffer: u32 = 0;
    let mut bits = 0u8;
    for b in text.bytes().filter(|b| !b.is_ascii_whitespace() && *b != b'=') {
        buffer = (buffer << 5) | jet_base32_val(b)? as u32;
        bits += 5;
        if bits >= 8 {
            out.push(((buffer >> (bits - 8)) & 0xff) as u8);
            bits -= 8;
        }
    }
    Ok(out)
}

fn jet_xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
fn jet_xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}
fn jet_xml_obj(name: String, attrs: Vec<(String, String)>, children: Vec<jet_std::DataTree>, text: String) -> jet_std::DataTree {
    jet_std::DataTree::Object(vec![
        ("name".to_string(), jet_std::DataTree::Text(name)),
        (
            "attrs".to_string(),
            jet_std::DataTree::Object(
                attrs
                    .into_iter()
                    .map(|(k, v)| (k, jet_std::DataTree::Text(v)))
                    .collect(),
            ),
        ),
        ("children".to_string(), jet_std::DataTree::Array(children)),
        ("text".to_string(), jet_std::DataTree::Text(text)),
    ])
}
fn jet_std_xml_parse(text: &String) -> Result<jet_std::DataTree, String> {
    #[derive(Clone)]
    struct Node { name: String, attrs: Vec<(String, String)>, children: Vec<jet_std::DataTree>, text: String }
    fn finish(n: Node) -> jet_std::DataTree { jet_xml_obj(n.name, n.attrs, n.children, n.text.trim().to_string()) }
    fn parse_tag(src: &str) -> Result<(String, Vec<(String, String)>, bool), String> {
        let mut s = src.trim().to_string();
        let self_close = s.ends_with('/');
        if self_close { s.pop(); }
        let mut parts = s.split_whitespace();
        let name = parts.next().ok_or_else(|| "empty XML tag".to_string())?.to_string();
        let mut attrs = Vec::new();
        let rest = &s[name.len()..];
        let mut i = 0usize;
        let bytes = rest.as_bytes();
        while i < bytes.len() {
            while i < bytes.len() && bytes[i].is_ascii_whitespace() { i += 1; }
            if i >= bytes.len() { break; }
            let start = i;
            while i < bytes.len() && bytes[i] != b'=' && !bytes[i].is_ascii_whitespace() { i += 1; }
            let key = rest[start..i].trim().to_string();
            while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b'=') { i += 1; }
            if i >= bytes.len() || bytes[i] != b'"' { return Err(format!("XML attribute `{key}` needs quoted value")); }
            i += 1;
            let val_start = i;
            while i < bytes.len() && bytes[i] != b'"' { i += 1; }
            if i >= bytes.len() { return Err(format!("XML attribute `{key}` is unterminated")); }
            attrs.push((key, jet_xml_unescape(&rest[val_start..i])));
            i += 1;
        }
        Ok((name, attrs, self_close))
    }
    let mut stack: Vec<Node> = Vec::new();
    let mut root: Option<jet_std::DataTree> = None;
    let mut i = 0usize;
    while let Some(rel) = text[i..].find('<') {
        let start = i + rel;
        if start > i {
            if let Some(top) = stack.last_mut() { top.text.push_str(&jet_xml_unescape(&text[i..start])); }
        }
        let end = text[start..].find('>').ok_or_else(|| "unterminated XML tag".to_string())? + start;
        let tag = text[start + 1..end].trim();
        if tag.starts_with("!--") || tag.starts_with('?') {
            i = end + 1;
            continue;
        }
        if let Some(close) = tag.strip_prefix('/') {
            let node = stack.pop().ok_or_else(|| format!("closing tag </{}> without opener", close.trim()))?;
            if node.name != close.trim() { return Err(format!("closing tag </{}> does not match <{}>", close.trim(), node.name)); }
            let tree = finish(node);
            if let Some(parent) = stack.last_mut() { parent.children.push(tree); } else { root = Some(tree); }
        } else {
            let (name, attrs, self_close) = parse_tag(tag)?;
            let node = Node { name, attrs, children: Vec::new(), text: String::new() };
            if self_close {
                let tree = finish(node);
                if let Some(parent) = stack.last_mut() { parent.children.push(tree); } else { root = Some(tree); }
            } else {
                stack.push(node);
            }
        }
        i = end + 1;
    }
    if i < text.len() {
        if let Some(top) = stack.last_mut() { top.text.push_str(&jet_xml_unescape(&text[i..])); }
    }
    if !stack.is_empty() { return Err(format!("unclosed XML tag <{}>", stack.last().unwrap().name)); }
    root.ok_or_else(|| "empty XML document".to_string())
}
fn jet_std_xml_render(d: &jet_std::DataTree) -> String {
    fn field<'a>(d: &'a jet_std::DataTree, name: &str) -> Option<&'a jet_std::DataTree> {
        if let jet_std::DataTree::Object(entries) = d {
            entries.iter().find(|(k, _)| k == name).map(|(_, v)| v)
        } else { None }
    }
    fn render_node(d: &jet_std::DataTree) -> String {
        let name = match field(d, "name") { Some(jet_std::DataTree::Text(s)) => s.clone(), _ => "node".to_string() };
        let attrs = match field(d, "attrs") {
            Some(jet_std::DataTree::Object(es)) => es.iter().filter_map(|(k, v)| match v {
                jet_std::DataTree::Text(s) => Some(format!(" {}=\"{}\"", k, jet_xml_escape(s))),
                _ => None,
            }).collect::<String>(),
            _ => String::new(),
        };
        let text = match field(d, "text") { Some(jet_std::DataTree::Text(s)) => jet_xml_escape(s), _ => String::new() };
        let children = match field(d, "children") {
            Some(jet_std::DataTree::Array(xs)) => xs.iter().map(render_node).collect::<String>(),
            _ => String::new(),
        };
        if text.is_empty() && children.is_empty() {
            format!("<{name}{attrs}/>")
        } else {
            format!("<{name}{attrs}>{text}{children}</{name}>")
        }
    }
    render_node(d)
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
    T::jet_decode_traced(&tree).map(|(value,_)|value).map_err(|error|jet_cbor_error(jet_std::CBORErrorKind::TypeMismatch,0,&error.path,error.reason))
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
