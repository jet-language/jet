//! AST adapters for D-PARSESTR1 / D-BINPAT1 match scans.
//!
//! Binary matching semantics live in the Foundation-owned Prelude fragment
//! `Prelude/MatchScan.rs`. AOT embeds that fragment; Foundation's AST adapter,
//! the Cranelift JIT host, and the canonical TIR evaluator marshal into it.
//! String matching remains separate: AST tiers use this text adapter, while AOT
//! keeps its existing generated text path.

use crate::AST::{BinEndian, BinMatchPart, BinSpec, StrMatchPart, Type};

mod bin_kernel {
    include!("Prelude/MatchScan.rs");
}

mod inline_range_semantics {
    include!("../../jet-codegen/src/Prelude/Core/InlineRange.rs");
}

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
        Some(Type::Int) => crate::Numeric::CtBigInt::from_str(raw).is_ok(),
        Some(Type::IntN { .. }) => raw.parse::<i64>().is_ok(),
        Some(Type::InlineRange { lo, hi, .. }) => raw
            .parse::<i64>()
            .ok()
            .and_then(|value| inline_range_semantics::jet_inline_range_from_int(value, *lo, *hi).ok())
            .is_some(),
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

/// Bytes a successful `bin_match_scan` consumed. A pattern that ends mid-byte
/// leaves no position a reader can hold, so it counts as a miss.
pub fn bin_match_consumed(bit_pos: usize) -> Option<usize> {
    (bit_pos % 8 == 0).then_some(bit_pos / 8)
}

/// Fixed-width holes use the smallest standard integer carrier that holds the
/// declared width. This matches sema and codegen (`U24` is carried as `U32`).
fn bin_bits_type(width: u8) -> Type {
    let bits = if width <= 8 {
        8
    } else if width <= 16 {
        16
    } else if width <= 32 {
        32
    } else {
        64
    };
    Type::IntN { signed: false, bits }
}

/// Byte-mode sibling of `str_match_scan`. Returns the bit position reached
/// plus the bound holes.
pub fn bin_match_scan(
    subject: &[u8],
    parts: &[BinMatchPart],
    consume_prefix: bool,
) -> Option<(usize, Vec<(String, Type, BinBind)>)> {
    let kernel_parts = parts
        .iter()
        .map(|part| match part {
            BinMatchPart::Lit(bytes) => bin_kernel::JetBinMatchPart::Lit(bytes.as_slice()),
            BinMatchPart::Hole { spec, .. } => match spec {
                BinSpec::Bits { width, endian } => bin_kernel::JetBinMatchPart::Bits {
                    width: *width as usize,
                    little: matches!(endian, BinEndian::Little),
                },
                BinSpec::Rest => bin_kernel::JetBinMatchPart::Rest,
            },
        })
        .collect::<Vec<_>>();
    let (bit_pos, values) =
        bin_kernel::jet_bin_match_scan(subject, &kernel_parts, consume_prefix)?;
    let mut values = values.into_iter();
    let mut binds = Vec::new();
    for part in parts {
        let BinMatchPart::Hole { name, spec, .. } = part else {
            continue;
        };
        let value = values.next()?;
        let (ty, bind) = match (spec, value) {
            (BinSpec::Bits { width, .. }, bin_kernel::JetBinMatchValue::Int(value)) => (
                bin_bits_type(*width),
                BinBind::Int(value as i64),
            ),
            (BinSpec::Rest, bin_kernel::JetBinMatchValue::Rest(bytes)) => (
                Type::List(Box::new(Type::IntN {
                    signed: false,
                    bits: 8,
                })),
                BinBind::Rest(bytes),
            ),
            _ => return None,
        };
        binds.push((name.clone(), ty, bind));
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

    #[test]
    fn bin_scan_reads_new_byte_widths_both_endians_and_rejects_short_input() {
        // Slice-typed rows on purpose: the widths differ, so a fixed-size array
        // element type would pin every row to the first row's length.
        let cases: [(u8, &[u8], i64, i64); 4] = [
            (24, &[1, 2, 3], 66051, 197121),
            (40, &[1, 2, 3, 4, 5], 4328719365, 21542142465),
            (48, &[1, 2, 3, 4, 5, 6], 1108152157446, 6618611909121),
            (56, &[1, 2, 3, 4, 5, 6, 7], 283686952306183, 1976943448883713),
        ];

        for (width, bytes, big_endian, little_endian) in cases {
            for (endian, expected) in [
                (BinEndian::Big, big_endian),
                (BinEndian::Little, little_endian),
            ] {
                let parts = vec![BinMatchPart::Hole {
                    name: "value".to_string(),
                    spec: BinSpec::Bits { width, endian },
                    span: Span::new(0, 0),
                }];
                let (bits, binds) =
                    bin_match_scan(bytes, &parts, false).expect("full-width match");
                assert_eq!(bits, width as usize);
                assert_eq!(binds[0].1, bin_bits_type(width));
                assert_eq!(binds[0].2, BinBind::Int(expected));
                assert!(bin_match_scan(&[], &parts, false).is_none());
                assert!(bin_match_scan(&bytes[..bytes.len() - 1], &parts, false).is_none());
            }
        }
    }
}
