//! Interpreted D-PARSESTR1 / D-BINPAT1 match scans.
//!
//! AOT specializes each pattern into inline Rust (`str_match_scan_closure_ex` /
//! `bin_match_scan_closure_ex`). Every tier that still holds the pattern as AST
//! at run time — the Cranelift JIT host and the canonical TIR evaluator — walks
//! it here instead, so quick-run and full build agree on what matches, what
//! binds, and how far the subject is consumed (I9, I8: one matcher).

use crate::AST::{BinEndian, BinMatchPart, BinSpec, StrMatchPart, Type};

/// One value bound by a binary pattern hole: a fixed-width integer, or the
/// `..rest` tail as raw bytes.
#[derive(Clone, Debug, PartialEq)]
pub enum BinBind {
    Int(i64),
    Rest(Vec<u8>),
}

/// Shared string-match scan used by pattern arms and `Cursor.take_pattern`.
/// `consume_prefix` allows unmatched trailing text (cursor mode); otherwise the
/// pattern must cover the whole subject.
pub fn str_match_scan(
    subject: &str,
    parts: &[StrMatchPart],
    consume_prefix: bool,
) -> Option<Vec<(String, Type, String)>> {
    let mut i = 0usize;
    let mut binds = Vec::new();
    for (pi, part) in parts.iter().enumerate() {
        match part {
            StrMatchPart::Lit(lit) => {
                if !subject[i..].starts_with(lit.as_str()) {
                    return None;
                }
                i += lit.len();
            }
            StrMatchPart::Hole { name, ty, .. } => {
                let end = match parts.get(pi + 1) {
                    Some(StrMatchPart::Lit(next)) => {
                        subject[i..].find(next.as_str()).map(|o| i + o)?
                    }
                    // A trailing hole takes the rest of the subject in both
                    // modes; in consume mode the caller stops at `i`.
                    _ => subject.len(),
                };
                let raw = &subject[i..end];
                if !str_hole_admits(ty, raw) {
                    return None;
                }
                binds.push((name.clone(), ty.clone().unwrap_or(Type::String), raw.to_string()));
                i = end;
            }
        }
    }
    if !consume_prefix && i != subject.len() {
        return None;
    }
    Some(binds)
}

fn str_hole_admits(ty: &Option<Type>, raw: &str) -> bool {
    match ty {
        Some(Type::Int) | Some(Type::IntN { .. }) => raw.parse::<i64>().is_ok(),
        Some(Type::Float) | Some(Type::Float32) => raw.parse::<f64>().is_ok(),
        Some(Type::Bool) => matches!(raw, "true" | "false" | "True" | "False" | "0" | "1"),
        _ => true,
    }
}

/// Byte length of `subject` consumed by a successful `str_match_scan` in
/// consume mode — how far `Cursor.take_pattern` advances.
pub fn str_match_consumed(subject: &str, parts: &[StrMatchPart]) -> Option<usize> {
    let mut i = 0usize;
    for (pi, part) in parts.iter().enumerate() {
        match part {
            StrMatchPart::Lit(lit) => {
                if !subject[i..].starts_with(lit.as_str()) {
                    return None;
                }
                i += lit.len();
            }
            StrMatchPart::Hole { ty, .. } => {
                let end = match parts.get(pi + 1) {
                    Some(StrMatchPart::Lit(next)) => {
                        subject[i..].find(next.as_str()).map(|o| i + o)?
                    }
                    _ => subject.len(),
                };
                if !str_hole_admits(ty, &subject[i..end]) {
                    return None;
                }
                i = end;
            }
        }
    }
    Some(i)
}

/// Byte-mode sibling of `str_match_scan`. Returns the bit position reached
/// plus the bound holes.
pub fn bin_match_scan(
    subject: &[u8],
    parts: &[BinMatchPart],
    consume_prefix: bool,
) -> Option<(usize, Vec<(String, Type, BinBind)>)> {
    let mut bit_pos = 0usize;
    let mut binds = Vec::new();
    for part in parts {
        match part {
            BinMatchPart::Lit(bytes) => {
                let need = bytes.len() * 8;
                if bit_pos % 8 != 0 {
                    return None;
                }
                let byte_pos = bit_pos / 8;
                if byte_pos + bytes.len() > subject.len() {
                    return None;
                }
                if &subject[byte_pos..byte_pos + bytes.len()] != bytes.as_slice() {
                    return None;
                }
                bit_pos += need;
            }
            BinMatchPart::Hole { name, spec, .. } => {
                let (width, be) = match spec {
                    BinSpec::Bits { width, endian } => (
                        *width as usize,
                        matches!(endian, BinEndian::Big | BinEndian::None),
                    ),
                    BinSpec::Rest => {
                        if bit_pos % 8 != 0 {
                            return None;
                        }
                        let rest = subject[bit_pos / 8..].to_vec();
                        binds.push((
                            name.clone(),
                            Type::List(Box::new(Type::IntN {
                                signed: false,
                                bits: 8,
                            })),
                            BinBind::Rest(rest),
                        ));
                        bit_pos = subject.len() * 8;
                        continue;
                    }
                };
                if width % 8 == 0 && bit_pos % 8 == 0 {
                    let nbytes = width / 8;
                    let byte_pos = bit_pos / 8;
                    if byte_pos + nbytes > subject.len() {
                        return None;
                    }
                    let slice = &subject[byte_pos..byte_pos + nbytes];
                    let v = match (nbytes, be) {
                        (1, _) => slice[0] as i64,
                        (2, true) => u16::from_be_bytes([slice[0], slice[1]]) as i64,
                        (2, false) => u16::from_le_bytes([slice[0], slice[1]]) as i64,
                        (4, true) => {
                            u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]) as i64
                        }
                        (4, false) => {
                            u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]) as i64
                        }
                        (8, true) => u64::from_be_bytes([
                            slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6],
                            slice[7],
                        ]) as i64,
                        (8, false) => u64::from_le_bytes([
                            slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6],
                            slice[7],
                        ]) as i64,
                        _ => return None,
                    };
                    binds.push((
                        name.clone(),
                        Type::IntN {
                            signed: false,
                            bits: width as u8,
                        },
                        BinBind::Int(v),
                    ));
                    bit_pos += width;
                } else {
                    // Nibble-oriented (U4): read bit by bit MSB-first.
                    if bit_pos + width > subject.len() * 8 {
                        return None;
                    }
                    let mut v = 0u64;
                    for _ in 0..width {
                        let byte = subject[bit_pos / 8];
                        let bit = 7 - (bit_pos % 8);
                        v = (v << 1) | ((byte >> bit) as u64 & 1);
                        bit_pos += 1;
                    }
                    binds.push((
                        name.clone(),
                        Type::IntN {
                            signed: false,
                            bits: width as u8,
                        },
                        BinBind::Int(v as i64),
                    ));
                }
            }
        }
    }
    if !consume_prefix && bit_pos != subject.len() * 8 {
        let has_rest = parts
            .iter()
            .any(|p| matches!(p, BinMatchPart::Hole { spec: BinSpec::Rest, .. }));
        if !has_rest {
            return None;
        }
    }
    Some((bit_pos, binds))
}

#[cfg(test)]
mod match_scan_tests {
    use super::*;

    use crate::Diagnostics::Span;

    fn hole(name: &str, ty: Type) -> StrMatchPart {
        StrMatchPart::Hole {
            name: name.to_string(),
            ty: Some(ty),
            span: Span::new(0, 0),
        }
    }

    #[test]
    fn str_scan_binds_and_reports_consumption() {
        let parts = vec![
            StrMatchPart::Lit("inc-".to_string()),
            hole("id", Type::Int),
            StrMatchPart::Lit(" sev ".to_string()),
            hole("sev", Type::Int),
            StrMatchPart::Lit(": ".to_string()),
        ];
        let subject = "inc-42 sev 3: disk full";
        let binds = str_match_scan(subject, &parts, true).expect("consume-mode match");
        assert_eq!(binds[0].2, "42");
        assert_eq!(binds[1].2, "3");
        assert_eq!(str_match_consumed(subject, &parts), Some(14));
        // Full-match mode rejects the unconsumed tail.
        assert!(str_match_scan(subject, &parts, false).is_none());
        // A non-numeric hole fails the Int check.
        assert!(str_match_scan("inc-x sev 3: y", &parts, true).is_none());
    }

    #[test]
    fn bin_scan_reads_widths_and_rest() {
        let parts = vec![
            BinMatchPart::Hole {
                name: "tag".to_string(),
                spec: BinSpec::Bits {
                    width: 16,
                    endian: BinEndian::Little,
                },
                span: Span::new(0, 0),
            },
            BinMatchPart::Hole {
                name: "body".to_string(),
                spec: BinSpec::Rest,
                span: Span::new(0, 0),
            },
        ];
        let (bits, binds) = bin_match_scan(&[1, 0, 9, 9], &parts, false).expect("match");
        assert_eq!(bits, 32);
        assert_eq!(binds[0].2, BinBind::Int(1));
        assert_eq!(binds[1].2, BinBind::Rest(vec![9, 9]));
    }
}
