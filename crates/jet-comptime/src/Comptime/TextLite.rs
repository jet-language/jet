//! D-BIGINT1-adjacent card #392 gap fix: `core.text` module functions
//! (`use core.text as text; text.trim(s)`, etc). Ported verbatim from AOT's
//! hand-rolled prelude implementation
//! (`crates/jet-codegen/src/Prelude/CoreLib/Top/Text.rs::jet_text_*`) so
//! comptime/REPL tier-0 matches AOT byte-for-byte (R12 parity) — these are
//! house-style approximations of Unicode NFC/NFD/casefold/segmentation
//! (no external crate, I6), not full Unicode Standard algorithms; both tiers
//! share the same approximation, so they still agree with each other.

// Card #298: table-driven NFC/NFD/NFKC/NFKD + full case folding, mirroring
// the AOT twin byte-for-byte (R12 parity) — see
// crates/jet-codegen/src/Prelude/CoreLib/Top/Text.rs for the identical
// algorithm. Both consume the same generated tables directly from
// jet-foundation (this crate already depends on it; no duplicate table
// copy needed here — only the AOT prelude, which cannot depend on the
// compiler's own crates, carries a textual duplicate).
use jet_foundation::generated::UnicodeTables::*;

const HANGUL_SBASE: u32 = 0xAC00;
const HANGUL_LBASE: u32 = 0x1100;
const HANGUL_VBASE: u32 = 0x1161;
const HANGUL_TBASE: u32 = 0x11A7;
const HANGUL_LCOUNT: u32 = 19;
const HANGUL_VCOUNT: u32 = 21;
const HANGUL_TCOUNT: u32 = 28;
const HANGUL_NCOUNT: u32 = HANGUL_VCOUNT * HANGUL_TCOUNT;
const HANGUL_SCOUNT: u32 = HANGUL_LCOUNT * HANGUL_NCOUNT;

fn ccc(cp: u32) -> u8 {
    UNICODE_CCC
        .binary_search_by(|&(a, b, _)| {
            if cp < a { std::cmp::Ordering::Greater }
            else if cp > b { std::cmp::Ordering::Less }
            else { std::cmp::Ordering::Equal }
        })
        .map(|i| UNICODE_CCC[i].2)
        .unwrap_or(0)
}

fn hangul_decompose(cp: u32) -> Option<[u32; 3]> {
    if cp < HANGUL_SBASE || cp >= HANGUL_SBASE + HANGUL_SCOUNT {
        return None;
    }
    let s_index = cp - HANGUL_SBASE;
    let l = HANGUL_LBASE + s_index / HANGUL_NCOUNT;
    let v = HANGUL_VBASE + (s_index % HANGUL_NCOUNT) / HANGUL_TCOUNT;
    let t_index = s_index % HANGUL_TCOUNT;
    if t_index == 0 {
        Some([l, v, 0])
    } else {
        Some([l, v, HANGUL_TBASE + t_index])
    }
}

fn decomp_lookup(cp: u32, compat: bool) -> Option<&'static [u32]> {
    let idx = UNICODE_DECOMP_INDEX
        .binary_search_by_key(&cp, |&(c, _, _, _)| c)
        .ok()?;
    let (_, start, len, is_canon) = UNICODE_DECOMP_INDEX[idx];
    if !compat && is_canon == 0 {
        return None;
    }
    Some(&UNICODE_DECOMP_POOL[start as usize..(start + len) as usize])
}

// Recursive: UnicodeData.txt decomposition mappings are only one step, not
// maximal (e.g. U+1E14 -> U+0112 U+0300, and U+0112 itself further
// decomposes to U+0045 U+0304) — each produced codepoint must be expanded
// again until stable (conformance-verified against NormalizationTest.txt).
fn expand_char(cp: u32, compat: bool, seq: &mut Vec<u32>) {
    if let Some([l, v, t]) = hangul_decompose(cp) {
        seq.push(l);
        seq.push(v);
        if t != 0 {
            seq.push(t);
        }
        return;
    }
    if let Some(expansion) = decomp_lookup(cp, compat) {
        for &sub in expansion {
            expand_char(sub, compat, seq);
        }
        return;
    }
    seq.push(cp);
}

fn canonical_order(seq: &mut [u32]) {
    for i in 1..seq.len() {
        let cls = ccc(seq[i]);
        if cls == 0 {
            continue;
        }
        let mut j = i;
        while j > 0 && ccc(seq[j - 1]) > cls {
            seq.swap(j - 1, j);
            j -= 1;
        }
    }
}

fn nfd_inner(s: &str, compat: bool) -> String {
    let mut seq: Vec<u32> = Vec::with_capacity(s.len());
    for c in s.chars() {
        expand_char(c as u32, compat, &mut seq);
    }
    canonical_order(&mut seq);
    seq.into_iter().filter_map(char::from_u32).collect()
}

pub(super) fn nfd(s: &str) -> String {
    nfd_inner(s, false)
}
pub(super) fn nfkd(s: &str) -> String {
    nfd_inner(s, true)
}

fn compose_pair_cp(a: u32, b: u32) -> Option<u32> {
    UNICODE_COMPOSE_PAIRS
        .binary_search_by(|&(x, y, _)| (x, y).cmp(&(a, b)))
        .ok()
        .map(|i| UNICODE_COMPOSE_PAIRS[i].2)
}

fn hangul_compose_pair(a: u32, b: u32) -> Option<u32> {
    if a >= HANGUL_LBASE && a < HANGUL_LBASE + HANGUL_LCOUNT
        && b >= HANGUL_VBASE && b < HANGUL_VBASE + HANGUL_VCOUNT
    {
        let l_index = a - HANGUL_LBASE;
        let v_index = b - HANGUL_VBASE;
        return Some(HANGUL_SBASE + (l_index * HANGUL_VCOUNT + v_index) * HANGUL_TCOUNT);
    }
    if a >= HANGUL_SBASE && a < HANGUL_SBASE + HANGUL_SCOUNT
        && (a - HANGUL_SBASE) % HANGUL_TCOUNT == 0
        && b > HANGUL_TBASE && b < HANGUL_TBASE + HANGUL_TCOUNT
    {
        return Some(a + (b - HANGUL_TBASE));
    }
    None
}

fn compose(s: String) -> String {
    // UAX#15 canonical composition ("blocked" rule): a character C composes
    // with the tracked starter only if no character since that starter has
    // combining class >= ccc(C). `last_class` uses -1 (not 0) as the
    // "nothing has intervened yet" sentinel — a ccc=0 character (many Indic
    // vowel signs compose with ccc=0) is only unblocked immediately after
    // the starter itself, not after any intervening combining mark.
    let seq: Vec<u32> = s.chars().map(|c| c as u32).collect();
    let mut out: Vec<u32> = Vec::with_capacity(seq.len());
    let mut starter_idx: Option<usize> = None;
    let mut last_class: i32 = -1;
    for cp in seq {
        let cls = ccc(cp) as i32;
        if let Some(si) = starter_idx {
            let starter = out[si];
            let composed =
                hangul_compose_pair(starter, cp).or_else(|| compose_pair_cp(starter, cp));
            if let (Some(composed_cp), true) = (composed, last_class < cls) {
                out[si] = composed_cp;
                continue;
            }
        }
        if cls == 0 {
            out.push(cp);
            starter_idx = Some(out.len() - 1);
            last_class = -1;
        } else {
            out.push(cp);
            last_class = cls;
        }
    }
    out.into_iter().filter_map(char::from_u32).collect()
}

pub(super) fn nfc(s: &str) -> String {
    compose(nfd(s))
}
pub(super) fn nfkc(s: &str) -> String {
    compose(nfkd(s))
}

fn fold_lookup(cp: u32) -> Option<&'static [u32]> {
    let idx = UNICODE_FOLD_INDEX.binary_search_by_key(&cp, |&(c, _, _)| c).ok()?;
    let (_, start, len) = UNICODE_FOLD_INDEX[idx];
    Some(&UNICODE_FOLD_POOL[start as usize..(start + len) as usize])
}

pub(super) fn casefold(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match fold_lookup(c as u32) {
            Some(seq) => {
                for &cp in seq {
                    if let Some(fc) = char::from_u32(cp) {
                        out.push(fc);
                    }
                }
            }
            None => out.push(c),
        }
    }
    out
}

// caseless_eq = full case fold + NFD compare (AOT doc contract): normalize
// each side to NFD, then apply full case folding, then compare.
pub(super) fn caseless_eq(a: &str, b: &str) -> bool {
    casefold(&nfd(a)) == casefold(&nfd(b))
}

fn is_mark(c: char) -> bool {
    matches!(c as u32, 0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F)
}

pub(super) fn graphemes(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in s.chars() {
        if is_mark(c) || c == '\u{200D}' {
            if let Some(last) = out.last_mut() {
                last.push(c);
            } else {
                out.push(c.to_string());
            }
        } else {
            out.push(c.to_string());
        }
    }
    out
}

pub(super) fn words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        if c.is_alphanumeric() || c == '\'' {
            cur.push(c);
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub(super) fn sentences(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        cur.push(c);
        if matches!(c, '.' | '!' | '?') {
            let t = cur.trim();
            if !t.is_empty() {
                out.push(t.to_string());
            }
            cur.clear();
        }
    }
    let t = cur.trim();
    if !t.is_empty() {
        out.push(t.to_string());
    }
    out
}

fn char_width(c: char) -> i64 {
    let u = c as u32;
    if c == '\0' || c.is_control() || is_mark(c) {
        0
    } else if matches!(u, 0x1100..=0x115F | 0x2329..=0x232A | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE10..=0xFE19 | 0xFE30..=0xFE6F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6 | 0x1F300..=0x1FAFF)
    {
        2
    } else {
        1
    }
}

pub(super) fn width(s: &str) -> i64 {
    s.chars().map(char_width).sum()
}

pub(super) fn is_alphabetic(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_alphabetic())
}
pub(super) fn is_numeric(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_numeric())
}
pub(super) fn is_whitespace(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_whitespace())
}

pub(super) fn splitn(s: &str, pat: &str, n: i64) -> Vec<String> {
    s.splitn(n.max(0) as usize, pat)
        .map(|x| x.to_string())
        .collect()
}
pub(super) fn rsplitn(s: &str, pat: &str, n: i64) -> Vec<String> {
    s.rsplitn(n.max(0) as usize, pat)
        .map(|x| x.to_string())
        .collect()
}

pub(super) fn pad_start(s: &str, w: i64, fill: &str) -> String {
    let mut out = String::new();
    let f = fill.chars().next().unwrap_or(' ');
    for _ in 0..(w - width(s)).max(0) {
        out.push(f);
    }
    out.push_str(s);
    out
}
pub(super) fn pad_end(s: &str, w: i64, fill: &str) -> String {
    let mut out = s.to_string();
    let f = fill.chars().next().unwrap_or(' ');
    for _ in 0..(w - width(s)).max(0) {
        out.push(f);
    }
    out
}
pub(super) fn center(s: &str, w: i64, fill: &str) -> String {
    let gap = (w - width(s)).max(0);
    let left = gap / 2;
    let right = gap - left;
    let f = fill.chars().next().unwrap_or(' ');
    format!(
        "{}{}{}",
        f.to_string().repeat(left as usize),
        s,
        f.to_string().repeat(right as usize)
    )
}

pub(super) fn starts_any(s: &str, prefixes: &[String]) -> bool {
    prefixes.iter().any(|p| s.starts_with(p.as_str()))
}
pub(super) fn ends_any(s: &str, suffixes: &[String]) -> bool {
    suffixes.iter().any(|p| s.ends_with(p.as_str()))
}

pub(super) fn char_indices(s: &str) -> Vec<String> {
    s.char_indices()
        .map(|(i, c)| format!("{}:{}", i, c))
        .collect()
}

// Card #298: conformance against the pinned Unicode 16.0.0 official
// NormalizationTest.txt corpus (committed at tests/data/unicode/, no
// network/runtime file dependency — embedded at compile time). Tests the
// exact algorithm shared (hand-mirrored) with the AOT twin in
// crates/jet-codegen/src/Prelude/CoreLib/Top/Text.rs.
#[cfg(test)]
mod normalization_conformance {
    use super::{nfc, nfd, nfkc, nfkd};

    fn cps_to_string(field: &str) -> String {
        field
            .split_whitespace()
            .filter_map(|tok| u32::from_str_radix(tok, 16).ok())
            .filter_map(char::from_u32)
            .collect()
    }

    #[test]
    fn normalization_test_txt_16_0_0() {
        let corpus = include_str!("../../../../tests/data/unicode/NormalizationTest.txt");
        let mut lines_checked = 0usize;
        let mut assertions = 0usize;
        for raw in corpus.lines() {
            let line = match raw.find('#') {
                Some(i) => &raw[..i],
                None => raw,
            };
            let line = line.trim();
            if line.is_empty() || line.starts_with('@') {
                continue;
            }
            let fields: Vec<&str> = line.split(';').collect();
            if fields.len() < 5 {
                continue;
            }
            let c1 = cps_to_string(fields[0]);
            let c2 = cps_to_string(fields[1]);
            let c3 = cps_to_string(fields[2]);
            let c4 = cps_to_string(fields[3]);
            let c5 = cps_to_string(fields[4]);
            lines_checked += 1;

            // NFC: c2 == nfc(c1) == nfc(c2) == nfc(c3); c4 == nfc(c4) == nfc(c5)
            for (label, input) in [("c1", &c1), ("c2", &c2), ("c3", &c3)] {
                assert_eq!(nfc(input), c2, "NFC({label}) mismatch on line: {raw}");
                assertions += 1;
            }
            for (label, input) in [("c4", &c4), ("c5", &c5)] {
                assert_eq!(nfc(input), c4, "NFC({label}) mismatch on line: {raw}");
                assertions += 1;
            }
            // NFD: c3 == nfd(c1) == nfd(c2) == nfd(c3); c5 == nfd(c4) == nfd(c5)
            for (label, input) in [("c1", &c1), ("c2", &c2), ("c3", &c3)] {
                assert_eq!(nfd(input), c3, "NFD({label}) mismatch on line: {raw}");
                assertions += 1;
            }
            for (label, input) in [("c4", &c4), ("c5", &c5)] {
                assert_eq!(nfd(input), c5, "NFD({label}) mismatch on line: {raw}");
                assertions += 1;
            }
            // NFKC: c4 == nfkc(c1..=c5)
            for (label, input) in [("c1", &c1), ("c2", &c2), ("c3", &c3), ("c4", &c4), ("c5", &c5)] {
                assert_eq!(nfkc(input), c4, "NFKC({label}) mismatch on line: {raw}");
                assertions += 1;
            }
            // NFKD: c5 == nfkd(c1..=c5)
            for (label, input) in [("c1", &c1), ("c2", &c2), ("c3", &c3), ("c4", &c4), ("c5", &c5)] {
                assert_eq!(nfkd(input), c5, "NFKD({label}) mismatch on line: {raw}");
                assertions += 1;
            }
        }
        assert!(lines_checked > 15000, "expected the full corpus, only saw {lines_checked} lines");
        println!(
            "NormalizationTest.txt 16.0.0: {lines_checked} lines, {assertions} conformance assertions, all passed"
        );
    }
}
