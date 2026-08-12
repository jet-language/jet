// ── D-UUIDENC1=A: core.encoding.hex / core.encoding.base64 / core.uuid ───────
// Pure std implementations; zero external crates (I6); memory-safe (I1).

fn jet_std_b64_decode(text: &String) -> Result<Vec<u8>, String> {
    jet_std_b64_decode_opts(text, false, false)
}

fn jet_std_b64_decode_opts(
    text: &String,
    allow_whitespace: bool,
    allow_missing_padding: bool,
) -> Result<Vec<u8>, String> {
    if __JET_PACKAGE_EDITION >= 2027 {
        jet_base_encoding_strict::decode_base64(text, allow_whitespace, allow_missing_padding)
    } else {
        jet_xml_pull::base_encoding_2026::decode_base64(text)
    }
}

fn jet_std_b64url_decode(text: &String) -> Result<Vec<u8>, String> {
    jet_std_b64url_decode_opts(text, false, false)
}

fn jet_std_b64url_decode_opts(
    text: &String,
    allow_whitespace: bool,
    allow_padding: bool,
) -> Result<Vec<u8>, String> {
    if __JET_PACKAGE_EDITION >= 2027 {
        jet_base_encoding_strict::decode_base64url(text, allow_whitespace, allow_padding)
    } else {
        jet_xml_pull::base_encoding_2026::decode_base64url(text)
    }
}

fn jet_std_base32_decode(text: &String) -> Result<Vec<u8>, String> {
    jet_std_base32_decode_opts(text, false, false, false)
}

fn jet_std_base32_decode_opts(
    text: &String,
    allow_whitespace: bool,
    allow_missing_padding: bool,
    allow_lowercase: bool,
) -> Result<Vec<u8>, String> {
    if __JET_PACKAGE_EDITION >= 2027 {
        jet_base_encoding_strict::decode_base32(
            text,
            allow_whitespace,
            allow_missing_padding,
            allow_lowercase,
        )
    } else {
        jet_xml_pull::base_encoding_2026::decode_base32(text)
    }
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
        byte_offset: jet_outcome_of(error.line.map(|_| error.offset as i64)),
        line: jet_outcome_of(error.line.map(|value| value as i64)),
        column: jet_outcome_of(error.column.map(|value| value as i64)),
        path: error.path,
        reason: error.reason,
    }
}

fn jet_xml_source_error(error: crate::jet_xml_pull::Error) -> jet_std::XMLError {
    let offset = error.offset as i64;
    let mut converted = jet_xml_error(error);
    converted.byte_offset = Ok(offset);
    converted
}

fn jet_xml_shape_error(reason: String) -> jet_std::XMLError {
    jet_std::XMLError {
        kind: jet_std::XMLReason::Shape,
        byte_offset: Err(JetAbsent),
        line: Err(JetAbsent),
        column: Err(JetAbsent),
        path: String::new(),
        reason,
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
    crate::jet_xml_kernel::parse_document(text).map(jet_xml_to_data_tree).map_err(jet_xml_error)
}

fn jet_std_xml_parse_with(text: &String, options: &jet_std::XMLParseOptions) -> Result<jet_std::DataTree, jet_std::XMLError> {
    crate::jet_xml_kernel::parse_document_with(text, &jet_xml_options(options))
        .map(jet_xml_to_data_tree)
        .map_err(jet_xml_error)
}

fn jet_std_xml_parse_bytes(bytes: &Vec<u8>, options: jet_std::XMLParseOptions) -> Result<jet_std::DataTree, jet_std::XMLError> {
    crate::jet_xml_kernel::parse_document_bytes_with(bytes, &jet_xml_options(&options))
        .map(jet_xml_to_data_tree)
        .map_err(jet_xml_source_error)
}

fn jet_std_xml_render(d: &jet_std::DataTree) -> String {
    jet_xml_from_data_tree(d)
        .ok()
        .and_then(|value| crate::jet_xml_kernel::render_document(&value).ok())
        .unwrap_or_default()
}

fn jet_std_xml_to_bytes(d: &jet_std::DataTree, options: jet_std::XMLRenderOptions) -> Result<Vec<u8>, jet_std::XMLError> {
    let value = jet_xml_from_data_tree(d).map_err(jet_xml_shape_error)?;
    crate::jet_xml_kernel::render_document_bytes(
        &value,
        jet_xml_render_encoding(&options.encoding),
        jet_xml_lexical_policy(&options.lexical),
    )
    .map_err(jet_xml_error)
}

fn jet_std_xml_canonical(d: &jet_std::DataTree, options: &jet_std::XMLCanonical) -> Result<String, jet_std::XMLError> {
    let value = jet_xml_from_data_tree(d).map_err(|reason| jet_std::XMLError {
        kind: jet_std::XMLReason::Shape,
        byte_offset: Err(JetAbsent),
        line: Err(JetAbsent),
        column: Err(JetAbsent),
        path: String::new(),
        reason,
    })?;
    let mode = match options.mode {
        jet_std::XMLCanonicalMode::Inclusive11 => crate::jet_xml_pull::CanonicalMode::Inclusive11,
        jet_std::XMLCanonicalMode::Exclusive10 => crate::jet_xml_pull::CanonicalMode::Exclusive10,
    };
    crate::jet_xml_kernel::canonical_document(&value, &crate::jet_xml_pull::CanonicalOptions {
        mode,
        comments: options.comments,
        inclusive_prefixes: options.inclusive_prefixes.clone(),
    }).map_err(jet_xml_error)
}

// D-ENCXML-PROJECTION1=A: focused helpers + typed decode over the closed tree.
fn jet_std_xml_root(document: &jet_std::DataTree) -> Result<jet_std::DataTree, jet_std::XMLError> {
    let value = jet_xml_from_data_tree(document).map_err(jet_xml_shape_error)?;
    crate::jet_xml_kernel::document_root(&value)
        .map(jet_xml_to_data_tree)
        .map_err(jet_xml_error)
}

fn jet_std_xml_expanded_name(
    node: &jet_std::DataTree,
) -> Result<(String, JetOutcome<String, JetAbsent>, String, JetOutcome<String, JetAbsent>), jet_std::XMLError> {
    let value = jet_xml_from_data_tree(node).map_err(jet_xml_shape_error)?;
    crate::jet_xml_kernel::expanded_name_parts(&value)
        .map(|(raw, prefix, local, uri)| (raw, jet_outcome_of(prefix), local, jet_outcome_of(uri)))
        .map_err(jet_xml_error)
}

fn jet_std_xml_attribute(
    element: &jet_std::DataTree,
    name: &String,
) -> Result<JetOutcome<String, JetAbsent>, jet_std::XMLError> {
    let value = jet_xml_from_data_tree(element).map_err(jet_xml_shape_error)?;
    crate::jet_xml_kernel::lookup_attribute(&value, name)
        .map(jet_outcome_of)
        .map_err(jet_xml_error)
}

fn jet_std_xml_content(element: &jet_std::DataTree) -> Result<Vec<jet_std::DataTree>, jet_std::XMLError> {
    let value = jet_xml_from_data_tree(element).map_err(jet_xml_shape_error)?;
    crate::jet_xml_kernel::element_content(&value)
        .map(|nodes| nodes.into_iter().map(jet_xml_to_data_tree).collect())
        .map_err(jet_xml_error)
}

fn jet_decode_path(path: &str) -> String {
    if path == "$" {
        String::new()
    } else if let Some(path) = path.strip_prefix("$.") {
        path.to_string()
    } else {
        path.strip_prefix('$').unwrap_or(path).to_string()
    }
}

fn jet_xml_decode_source_error(error: crate::jet_xml_pull::Error) -> Vec<jet_std::FieldError> {
    jet_std::FieldError::at(
        jet_decode_path(&error.path),
        format!("XML {:?}: {}", jet_xml_reason(error.kind), error.reason),
    )
}

fn jet_xml_decode_value_error(error: jet_std::XMLError) -> Vec<jet_std::FieldError> {
    jet_std::FieldError::at(
        jet_decode_path(&error.path),
        format!("XML {:?}: {}", error.kind, error.reason),
    )
}

fn jet_xml_decode_shape_error(reason: String) -> Vec<jet_std::FieldError> {
    jet_std::FieldError::one(reason)
}

fn jet_enc_xml_decode_projected<T: __jet_Decode>(projected: &jet_std::DataTree) -> Result<T, Vec<jet_std::FieldError>> {
    match T::jet_decode(projected) {
        Ok(value) => Ok(value),
        Err(primary) => {
            if let jet_std::DataTree::Object(entries) = projected {
                if let Some((_, jet_std::DataTree::Text(text))) =
                    entries.iter().find(|(key, _)| key == "$text")
                {
                    return T::jet_decode(&jet_std::DataTree::Text(text.clone())).map_err(|secondary| {
                        if secondary.first().is_some_and(|error| error.path.is_empty())
                            && primary.first().is_some_and(|error| !error.path.is_empty())
                        {
                            primary
                        } else {
                            secondary
                        }
                    });
                }
            }
            Err(primary)
        }
    }
}

fn jet_enc_xml_decode<T: __jet_Decode>(
    text: &String,
    options: jet_std::XMLParseOptions,
) -> Result<T, Vec<jet_std::FieldError>> {
    let document = jet_std_xml_parse_with(text, &options).map_err(jet_xml_decode_value_error)?;
    let value = jet_xml_from_data_tree(&document).map_err(jet_xml_decode_shape_error)?;
    let projected = crate::jet_xml_kernel::project_document_for_decode(&value)
        .map_err(jet_xml_decode_source_error)?;
    jet_enc_xml_decode_projected(&jet_xml_to_data_tree(projected))
}

fn jet_enc_xml_decode_bytes<T: __jet_Decode>(
    bytes: &Vec<u8>,
    options: jet_std::XMLParseOptions,
) -> Result<T, Vec<jet_std::FieldError>> {
    let document = jet_std_xml_parse_bytes(bytes, options).map_err(jet_xml_decode_value_error)?;
    let value = jet_xml_from_data_tree(&document).map_err(jet_xml_decode_shape_error)?;
    let projected = crate::jet_xml_kernel::project_document_for_decode(&value)
        .map_err(jet_xml_decode_source_error)?;
    jet_enc_xml_decode_projected(&jet_xml_to_data_tree(projected))
}

fn jet_cbor_value(value: &jet_std::DataTree) -> crate::jet_cbor_kernel::Value {
    match value {
        jet_std::DataTree::Null => crate::jet_cbor_kernel::Value::Null,
        jet_std::DataTree::Bool(value) => crate::jet_cbor_kernel::Value::Bool(*value),
        jet_std::DataTree::Int(value) => crate::jet_cbor_kernel::Value::Int(*value),
        jet_std::DataTree::Float(value) => crate::jet_cbor_kernel::Value::Float(*value),
        jet_std::DataTree::Text(value) => crate::jet_cbor_kernel::Value::Text(value.clone()),
        jet_std::DataTree::Bytes(value) => crate::jet_cbor_kernel::Value::Bytes(value.clone()),
        jet_std::DataTree::Array(values) => crate::jet_cbor_kernel::Value::Array(
            values.iter().map(jet_cbor_value).collect(),
        ),
        jet_std::DataTree::Object(entries) => crate::jet_cbor_kernel::Value::Object(
            entries
                .iter()
                .map(|(key, value)| (key.clone(), jet_cbor_value(value)))
                .collect(),
        ),
    }
}

fn jet_cbor_tree(value: crate::jet_cbor_kernel::Value) -> jet_std::DataTree {
    match value {
        crate::jet_cbor_kernel::Value::Null => jet_std::DataTree::Null,
        crate::jet_cbor_kernel::Value::Bool(value) => jet_std::DataTree::Bool(value),
        crate::jet_cbor_kernel::Value::Int(value) => jet_std::DataTree::Int(value),
        crate::jet_cbor_kernel::Value::Float(value) => jet_std::DataTree::Float(value),
        crate::jet_cbor_kernel::Value::Text(value) => jet_std::DataTree::Text(value),
        crate::jet_cbor_kernel::Value::Bytes(value) => jet_std::DataTree::Bytes(value),
        crate::jet_cbor_kernel::Value::Array(values) => {
            jet_std::DataTree::Array(values.into_iter().map(jet_cbor_tree).collect())
        }
        crate::jet_cbor_kernel::Value::Object(entries) => jet_std::DataTree::Object(
            entries
                .into_iter()
                .map(|(key, value)| (key, jet_cbor_tree(value)))
                .collect(),
        ),
    }
}

fn jet_cbor_options(options: &jet_std::CBOROptions) -> crate::jet_cbor_kernel::Options {
    crate::jet_cbor_kernel::Options {
        max_depth: options.max_depth,
        max_items: options.max_items,
        max_bytes: options.max_bytes,
        require_canonical: options.require_canonical,
    }
}

fn jet_cbor_error(error: crate::jet_cbor_kernel::Error) -> jet_std::CBORError {
    let crate::jet_cbor_kernel::Error {
        kind,
        byte_offset,
        path,
        reason,
    } = error;
    let kind = match kind {
        crate::jet_cbor_kernel::ErrorKind::Syntax => jet_std::CBORErrorKind::Syntax,
        crate::jet_cbor_kernel::ErrorKind::Truncated => jet_std::CBORErrorKind::Truncated,
        crate::jet_cbor_kernel::ErrorKind::Unsupported => jet_std::CBORErrorKind::Unsupported,
        crate::jet_cbor_kernel::ErrorKind::Limit => jet_std::CBORErrorKind::Limit,
        crate::jet_cbor_kernel::ErrorKind::TypeMismatch => jet_std::CBORErrorKind::TypeMismatch,
        crate::jet_cbor_kernel::ErrorKind::TrailingData => jet_std::CBORErrorKind::TrailingData,
        crate::jet_cbor_kernel::ErrorKind::NonCanonical => jet_std::CBORErrorKind::NonCanonical,
    };
    jet_std::CBORError {
        kind,
        byte_offset: byte_offset as i64,
        path,
        reason,
    }
}

fn jet_enc_cbor_to_bytes<T: __jet_Encode>(value: &T) -> Result<Vec<u8>, jet_std::CBORError> {
    let tree = value.jet_encode();
    crate::jet_cbor_kernel::encode(&jet_cbor_value(&tree), false).map_err(jet_cbor_error)
}

fn jet_enc_cbor_to_bytes_canonical<T: __jet_Encode>(
    value: &T,
) -> Result<Vec<u8>, jet_std::CBORError> {
    let tree = value.jet_encode();
    crate::jet_cbor_kernel::encode(&jet_cbor_value(&tree), true).map_err(jet_cbor_error)
}

fn jet_enc_cbor_encode(value: &jet_std::DataTree) -> Vec<u8> {
    jet_enc_cbor_to_bytes(value).unwrap_or_else(|error| {
        panic!("cbor.encode failed: {}", error.reason)
    })
}

fn jet_enc_cbor_decode_legacy(bytes: &Vec<u8>) -> Result<jet_std::DataTree, String> {
    jet_enc_cbor_parse(bytes, jet_std::CBOROptions::safe()).map_err(|error| error.reason)
}

fn jet_enc_cbor_parse_with(
    bytes: &Vec<u8>,
    options: jet_std::CBOROptions,
    allow_bytes: bool,
) -> Result<jet_std::DataTree, jet_std::CBORError> {
    crate::jet_cbor_kernel::decode(bytes, &jet_cbor_options(&options), allow_bytes)
        .map(jet_cbor_tree)
        .map_err(jet_cbor_error)
}

fn jet_enc_cbor_parse(
    bytes: &Vec<u8>,
    options: jet_std::CBOROptions,
) -> Result<jet_std::DataTree, jet_std::CBORError> {
    jet_enc_cbor_parse_with(bytes, options, false)
}

fn jet_cbor_decode_source_error(error: jet_std::CBORError) -> Vec<jet_std::FieldError> {
    jet_std::FieldError::at(
        jet_decode_path(&error.path),
        format!(
            "CBOR {:?} at byte {}: {}",
            error.kind, error.byte_offset, error.reason
        ),
    )
}

fn jet_enc_cbor_decode<T: __jet_Decode>(
    bytes: &Vec<u8>,
    options: jet_std::CBOROptions,
) -> Result<T, Vec<jet_std::FieldError>> {
    let tree = jet_enc_cbor_parse_with(bytes, options, true)
        .map_err(jet_cbor_decode_source_error)?;
    T::jet_decode_traced(&tree).map(|(value, _)| value)
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

// #1481 core.uuid: parse a hyphenated UUID string into its 16 raw bytes,
// rejecting anything that is not exactly 8-4-4-4-12 hex digits.
fn jet_uuid_bytes(s: &str) -> Result<[u8; 16], String> {
    let hex: String = s.chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("`{s}` is not a UUID (want 8-4-4-4-12 hex digits)"));
    }
    let groups: Vec<usize> = s.match_indices('-').map(|(i, _)| i).collect();
    if groups != [8, 13, 18, 23] {
        return Err(format!("`{s}` is not a UUID (want 8-4-4-4-12 hex digits)"));
    }
    let mut bytes = [0u8; 16];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).map_err(|_| {
            format!("`{s}` is not a UUID (want 8-4-4-4-12 hex digits)")
        })?;
    }
    Ok(bytes)
}

// #1481 core.uuid: validate and normalize (lowercase) a UUID string — the
// String representation stays canonical (D-CORE-TREE1 area; no new UUID type).
fn jet_std_uuid_parse(s: &String) -> Result<String, String> {
    jet_uuid_bytes(s).map(|bytes| jet_uuid_format(&bytes))
}

// #1481 core.uuid: RFC 4122 SHA-1 (version 5) — deterministic, no CSPRNG.
// Pure std, zero deps (I6); this is the one extra hash function UUID v5
// needs and is not reused elsewhere, so it stays local instead of a shared
// crypto primitive.
fn jet_uuid_sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 80];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            *word = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for (i, word) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }
    let mut out = [0u8; 20];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

// #1481 core.uuid: v5 (namespace + name, SHA-1) — deterministic sibling of
// the already-shipped v4 (random) and v7 (time-ordered).
fn jet_std_uuid_v5(namespace: &String, name: &String) -> Result<String, String> {
    let ns = jet_uuid_bytes(namespace)?;
    let mut input = ns.to_vec();
    input.extend_from_slice(name.as_bytes());
    let digest = jet_uuid_sha1(&input);
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50; // version 5
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant 10xx
    Ok(jet_uuid_format(&bytes))
}

fn jet_std_uuid_v4() -> String {
    jet_crypto_uuid_v4()
}

fn jet_std_uuid_v7(clock: &jet_std::Clock) -> String {
    jet_crypto_uuid_v7(clock.now())
}
