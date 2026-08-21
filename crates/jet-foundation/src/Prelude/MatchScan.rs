// D-BINPAT1 / I9: one binary-pattern scan kernel. `Codegen/mod.rs` embeds this
// exact source in generated AOT programs; Foundation's AST adapter, Cranelift,
// and the TIR evaluator marshal into it. Keep this fragment std-only and free
// of `crate::` paths so it also compiles at the root of generated Rust.

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
