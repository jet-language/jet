// D-BINPAT1 / I9: one binary-pattern scan kernel. `Codegen/mod.rs` embeds this
// exact source in generated AOT programs; Foundation's AST adapter, Cranelift,
// and the TIR evaluator marshal into it. Keep this fragment std-only and free
// of `crate::` paths so it also compiles at the root of generated Rust.

/// Typed capture produced by the shared text and binary pattern routes.
///
/// Names are deliberately absent: MIR records holes in source order and the
/// capture index is the only adapter-visible binding identity.
#[derive(Clone, Debug, PartialEq)]
pub enum JetPatternCapture {
    Text(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Bytes(Vec<u8>),
}

/// Sema-admitted text-hole conversions.  The route receives this exact
/// descriptor; it must not infer a type from the matched spelling.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JetTextHoleKind {
    Text,
    Int,
    Float,
    Bool,
    InlineRange { lo: i64, hi: i64 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JetTextMatchPart<'a> {
    Literal(&'a str),
    Hole { kind: JetTextHoleKind },
}

fn jet_text_capture(kind: JetTextHoleKind, raw: &str) -> Option<JetPatternCapture> {
    match kind {
        JetTextHoleKind::Text => Some(JetPatternCapture::Text(raw.to_string())),
        JetTextHoleKind::Int => raw.parse::<i64>().ok().map(JetPatternCapture::Int),
        JetTextHoleKind::Float => raw.parse::<f64>().ok().map(JetPatternCapture::Float),
        JetTextHoleKind::Bool => match raw {
            "true" | "True" | "1" => Some(JetPatternCapture::Bool(true)),
            "false" | "False" | "0" => Some(JetPatternCapture::Bool(false)),
            _ => None,
        },
        JetTextHoleKind::InlineRange { lo, hi } => raw
            .parse::<i64>()
            .ok()
            .filter(|value| (*value >= lo) && (*value <= hi))
            .map(JetPatternCapture::Int),
    }
}

/// Match a complete UTF-8 subject against the canonical typed descriptor.
pub fn jet_text_pattern_match(
    subject: &str,
    parts: &[JetTextMatchPart<'_>],
) -> Option<Vec<JetPatternCapture>> {
    let mut cursor = 0usize;
    let mut captures = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        match part {
            JetTextMatchPart::Literal(literal) => {
                let tail = subject.get(cursor..)?;
                if !tail.starts_with(literal) {
                    return None;
                }
                cursor = cursor.checked_add(literal.len())?;
            }
            JetTextMatchPart::Hole { kind } => {
                let end = match parts.get(index + 1) {
                    Some(JetTextMatchPart::Literal(next)) => {
                        subject.get(cursor..)?.find(next).map(|offset| cursor + offset)?
                    }
                    Some(JetTextMatchPart::Hole { .. }) | None => subject.len(),
                };
                let raw = subject.get(cursor..end)?;
                captures.push(jet_text_capture(*kind, raw)?);
                cursor = end;
            }
        }
    }
    (cursor == subject.len()).then_some(captures)
}

/// Match a complete byte subject using the Foundation bit scanner and return
/// its typed captures in source order.
pub fn jet_binary_pattern_match(
    subject: &[u8],
    parts: &[JetBinMatchPart<'_>],
) -> Option<Vec<JetPatternCapture>> {
    let (_, values) = jet_bin_match_scan(subject, parts, false)?;
    values
        .into_iter()
        .map(|value| match value {
            JetBinMatchValue::Int(value) => Some(JetPatternCapture::Int(value as i64)),
            JetBinMatchValue::Rest(value) => Some(JetPatternCapture::Bytes(value)),
        })
        .collect()
}

#[derive(Clone, Copy, Debug)]
pub enum JetBinMatchPart<'a> {
    Lit(&'a [u8]),
    Bits { width: usize, little: bool },
    Rest,
}

#[derive(Clone, Debug, PartialEq)]
pub enum JetBinMatchValue {
    Int(u64),
    Rest(Vec<u8>),
}

/// Scan one byte-pattern description from the current bit position.
///
/// `consume_prefix` permits trailing subject bytes, but still requires the
/// match to end on a byte boundary because `Reader` advances by bytes.
pub fn jet_bin_match_scan(
    subject: &[u8],
    parts: &[JetBinMatchPart<'_>],
    consume_prefix: bool,
) -> Option<(usize, Vec<JetBinMatchValue>)> {
    let total = subject.len().checked_mul(8)?;
    let mut bit_pos = 0usize;
    let mut values = Vec::new();

    for part in parts {
        match *part {
            JetBinMatchPart::Lit(bytes) => {
                if bit_pos % 8 != 0 {
                    return None;
                }
                let byte_pos = bit_pos / 8;
                let end = byte_pos.checked_add(bytes.len())?;
                if end > subject.len() || &subject[byte_pos..end] != bytes {
                    return None;
                }
                bit_pos = bit_pos.checked_add(bytes.len().checked_mul(8)?)?;
            }
            JetBinMatchPart::Bits { width, little } => {
                let end = bit_pos.checked_add(width)?;
                if end > total {
                    return None;
                }
                let mut value = 0u64;
                for offset in 0..width {
                    let position = bit_pos + offset;
                    let byte = subject[position / 8];
                    let bit = 7 - (position % 8);
                    value = (value << 1) | u64::from((byte >> bit) & 1);
                }
                if little && width % 8 == 0 {
                    let bytes = width / 8;
                    let mut swapped = 0u64;
                    for index in 0..bytes {
                        swapped |= ((value >> (8 * index)) & 0xff) << (8 * (bytes - 1 - index));
                    }
                    value = swapped;
                }
                bit_pos = end;
                values.push(JetBinMatchValue::Int(value));
            }
            JetBinMatchPart::Rest => {
                if bit_pos % 8 != 0 {
                    return None;
                }
                values.push(JetBinMatchValue::Rest(subject[bit_pos / 8..].to_vec()));
                bit_pos = total;
            }
        }
    }

    if !consume_prefix && bit_pos != total {
        let has_rest = parts
            .iter()
            .any(|part| matches!(*part, JetBinMatchPart::Rest));
        if !has_rest {
            return None;
        }
    }
    if consume_prefix && bit_pos % 8 != 0 {
        return None;
    }
    Some((bit_pos, values))
}
