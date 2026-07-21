//! D-BIGINT1-adjacent card #392 gap fix: `core.text` module functions
//! (`use core.text as text; text.trim(s)`, etc). Ported verbatim from AOT's
//! generated-prelude implementation
//! (`crates/jet-codegen/src/Prelude/CoreLib/Top/Text.rs::jet_text_*`) so
//! comptime/REPL tier-0 matches AOT byte-for-byte (R12 parity). Both consume
//! checksum-pinned Unicode 16.0.0 tables; no host Unicode version leaks into
//! normalization, casing, segmentation, width, or public classification.

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
    // Stable counting sort each non-starter run by its u8 combining class.
    // Avoids insertion sort's quadratic path on hostile descending marks.
    let mut start = 0usize;
    while start < seq.len() {
        if ccc(seq[start]) == 0 {
            start += 1;
        }
        let mut end = start;
        let mut ordered = true;
        let mut previous = 0u8;
        while end < seq.len() {
            let class = ccc(seq[end]);
            if class == 0 {
                break;
            }
            ordered &= class >= previous;
            previous = class;
            end += 1;
        }
        if !ordered {
            let mut counts = [0usize; 256];
            for &cp in &seq[start..end] {
                counts[ccc(cp) as usize] += 1;
            }
            let mut offsets = [0usize; 256];
            let mut next = start;
            for class in 1..256 {
                offsets[class] = next;
                next += counts[class];
            }
            let mut sorted = vec![0u32; end - start];
            for &cp in &seq[start..end] {
                let class = ccc(cp) as usize;
                let at = offsets[class] - start;
                sorted[at] = cp;
                offsets[class] += 1;
            }
            seq[start..end].copy_from_slice(&sorted);
        }
        start = end;
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

fn mapping_lookup(
    cp: u32,
    index: &'static [(u32, u32, u32)],
    pool: &'static [u32],
) -> Option<&'static [u32]> {
    let at = index.binary_search_by_key(&cp, |&(c, _, _)| c).ok()?;
    let (_, start, len) = index[at];
    Some(&pool[start as usize..(start + len) as usize])
}

fn append_mapping(out: &mut String, cp: u32, index: &'static [(u32, u32, u32)], pool: &'static [u32]) {
    if let Some(mapped) = mapping_lookup(cp, index, pool) {
        out.extend(mapped.iter().filter_map(|&mapped_cp| char::from_u32(mapped_cp)));
    } else if let Some(ch) = char::from_u32(cp) {
        out.push(ch);
    }
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

fn property(table: &[(u32, u32)], cp: u32) -> bool {
    bsearch_pair(table, cp)
}

pub(super) fn alphabetic(cp: u32) -> bool {
    property(UNICODE_ALPHABETIC, cp)
}

pub(super) fn numeric(cp: u32) -> bool {
    property(UNICODE_NUMERIC, cp)
}

pub(super) fn whitespace(cp: u32) -> bool {
    property(UNICODE_WHITE_SPACE, cp)
}

fn is_cased(cp: u32) -> bool {
    property(UNICODE_CASED, cp)
}

fn is_case_ignorable(cp: u32) -> bool {
    property(UNICODE_CASE_IGNORABLE, cp)
}

fn final_sigma(chars: &[char], at: usize) -> bool {
    let before = chars[..at]
        .iter()
        .rev()
        .find(|ch| !is_case_ignorable(**ch as u32))
        .is_some_and(|ch| is_cased(*ch as u32));
    let after = chars[at + 1..]
        .iter()
        .find(|ch| !is_case_ignorable(**ch as u32))
        .is_some_and(|ch| is_cased(*ch as u32));
    before && !after
}

pub(super) fn lower(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    for (at, &ch) in chars.iter().enumerate() {
        if ch == '\u{03A3}' && final_sigma(&chars, at) {
            out.push('\u{03C2}');
        } else {
            append_mapping(&mut out, ch as u32, UNICODE_LOWER_INDEX, UNICODE_LOWER_POOL);
        }
    }
    out
}

pub(super) fn upper(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        append_mapping(&mut out, ch as u32, UNICODE_UPPER_INDEX, UNICODE_UPPER_POOL);
    }
    out
}

// Unicode Default Caseless Matching: NFD(toCasefold(NFD(text))).
pub(super) fn caseless_eq(a: &str, b: &str) -> bool {
    nfd(&casefold(&nfd(a))) == nfd(&casefold(&nfd(b)))
}

// Card #298: real UAX#29 grapheme/word/sentence segmentation over the
// generated break-property tables (`UNICODE_GRAPHEME_BREAK`/`_WORD_BREAK`/
// `_SENTENCE_BREAK`/`_EXTENDED_PICTOGRAPHIC`), mirroring the AOT twin
// byte-for-byte (R12 parity) — see
// crates/jet-codegen/src/Prelude/CoreLib/Top/Text.rs for the identical
// algorithm. Tag encodings match `scripts/agent/gen-unicode-tables.mjs`'s
// `GRAPHEME_TAGS`/`WORD_TAGS`/`SENTENCE_TAGS` arrays exactly.

fn bsearch_triple(table: &[(u32, u32, u8)], cp: u32) -> u8 {
    table
        .binary_search_by(|&(a, b, _)| {
            if cp < a {
                std::cmp::Ordering::Greater
            } else if cp > b {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .map(|i| table[i].2)
        .unwrap_or(0)
}
fn bsearch_pair(table: &[(u32, u32)], cp: u32) -> bool {
    table
        .binary_search_by(|&(a, b)| {
            if cp < a {
                std::cmp::Ordering::Greater
            } else if cp > b {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

fn grapheme_class(cp: u32) -> u8 {
    bsearch_triple(UNICODE_GRAPHEME_BREAK, cp)
}
fn word_class(cp: u32) -> u8 {
    bsearch_triple(UNICODE_WORD_BREAK, cp)
}
fn sentence_class(cp: u32) -> u8 {
    bsearch_triple(UNICODE_SENTENCE_BREAK, cp)
}
fn is_ext_pictographic(cp: u32) -> bool {
    bsearch_pair(UNICODE_EXTENDED_PICTOGRAPHIC, cp)
}
fn is_emoji_presentation(cp: u32) -> bool {
    bsearch_pair(UNICODE_EMOJI_PRESENTATION, cp)
}
fn is_emoji(cp: u32) -> bool {
    bsearch_pair(UNICODE_EMOJI, cp)
}
fn is_default_ignorable(cp: u32) -> bool {
    bsearch_pair(UNICODE_DEFAULT_IGNORABLE, cp)
}
fn general_category(cp: u32) -> u8 {
    UNICODE_GENERAL_CATEGORY
        .binary_search_by(|&(start, end, _)| {
            if cp < start {
                std::cmp::Ordering::Greater
            } else if cp > end {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .map(|at| UNICODE_GENERAL_CATEGORY[at].2)
        .unwrap_or(8)
}
fn is_control(cp: u32) -> bool {
    general_category(cp) == 0
}
fn is_zero_width_scalar(cp: u32) -> bool {
    matches!(general_category(cp), 2 | 3) || is_default_ignorable(cp)
}
/// East Asian Width: 0=narrow(N/Na/H) 1=Ambiguous 2=wide(W/F).
fn eaw_class(cp: u32) -> u8 {
    bsearch_triple(UNICODE_EAST_ASIAN_WIDTH, cp)
}
/// Indic_Conjunct_Break (GB9c): 0=None 1=Linker 2=Consonant 3=Extend.
fn incb_class(cp: u32) -> u8 {
    bsearch_triple(UNICODE_INCB, cp)
}
const INCB_LINKER: u8 = 1;
const INCB_CONSONANT: u8 = 2;
const INCB_EXTEND: u8 = 3;

// ---- GB1-GB13 grapheme cluster boundaries -----------------------------------
const GB_OTHER: u8 = 0;
const GB_CR: u8 = 1;
const GB_LF: u8 = 2;
const GB_CONTROL: u8 = 3;
const GB_EXTEND: u8 = 4;
const GB_ZWJ: u8 = 5;
const GB_RI: u8 = 6;
const GB_PREPEND: u8 = 7;
const GB_SPACINGMARK: u8 = 8;
const GB_L: u8 = 9;
const GB_V: u8 = 10;
const GB_T: u8 = 11;
const GB_LV: u8 = 12;
const GB_LVT: u8 = 13;

/// GB3-GB9b/GB999 (class-pair rules only — GB11's Extended_Pictographic
/// lookup and GB12/13's regional-indicator parity are stateful, handled by
/// the caller).
fn grapheme_break_classes(pc: u8, cc: u8) -> bool {
    if pc == GB_CR && cc == GB_LF {
        return false; // GB3
    }
    if matches!(pc, GB_CR | GB_LF | GB_CONTROL) {
        return true; // GB4
    }
    if matches!(cc, GB_CR | GB_LF | GB_CONTROL) {
        return true; // GB5
    }
    if pc == GB_L && matches!(cc, GB_L | GB_V | GB_LV | GB_LVT) {
        return false; // GB6
    }
    if matches!(pc, GB_LV | GB_V) && matches!(cc, GB_V | GB_T) {
        return false; // GB7
    }
    if matches!(pc, GB_LVT | GB_T) && cc == GB_T {
        return false; // GB8
    }
    if matches!(cc, GB_EXTEND | GB_ZWJ) {
        return false; // GB9
    }
    if cc == GB_SPACINGMARK {
        return false; // GB9a
    }
    if pc == GB_PREPEND {
        return false; // GB9b
    }
    let _ = GB_OTHER;
    true // GB999
}

pub(super) fn graphemes(s: &str) -> Vec<String> {
    let cps: Vec<char> = s.chars().collect();
    if cps.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    cur.push(cps[0]);
    let mut ri_run: usize = if grapheme_class(cps[0] as u32) == GB_RI { 1 } else { 0 };
    let mut saw_pic = is_ext_pictographic(cps[0] as u32);
    let mut incb_pending = incb_class(cps[0] as u32) == INCB_CONSONANT;
    let mut incb_linker = false;
    for i in 1..cps.len() {
        let prev = cps[i - 1];
        let curc = cps[i];
        let pc = grapheme_class(prev as u32);
        let cc = grapheme_class(curc as u32);
        let brk = if pc == GB_ZWJ && saw_pic && is_ext_pictographic(curc as u32) {
            false // GB11: \p{Extended_Pictographic} Extend* ZWJ x \p{Extended_Pictographic}
        } else if pc == GB_RI && cc == GB_RI {
            ri_run % 2 == 0 // GB12/GB13: pair up regional indicators
        } else if incb_pending && incb_linker && incb_class(curc as u32) == INCB_CONSONANT {
            false // GB9c: Indic conjunct (Consonant [Extend|Linker]* Linker [Extend|Linker]* x Consonant)
        } else {
            grapheme_break_classes(pc, cc)
        };
        if brk {
            out.push(std::mem::take(&mut cur));
        }
        cur.push(curc);
        ri_run = if cc == GB_RI { ri_run + 1 } else { 0 };
        saw_pic = if matches!(cc, GB_EXTEND | GB_ZWJ) {
            saw_pic
        } else {
            is_ext_pictographic(curc as u32)
        };
        match incb_class(curc as u32) {
            INCB_CONSONANT => {
                incb_pending = true;
                incb_linker = false;
            }
            INCB_LINKER => {
                if incb_pending {
                    incb_linker = true;
                }
            }
            INCB_EXTEND => {}
            _ => {
                incb_pending = false;
                incb_linker = false;
            }
        }
    }
    out.push(cur);
    out
}

// ---- WB1-WB16 word boundaries ------------------------------------------------
const WB_OTHER: u8 = 0;
const WB_CR: u8 = 1;
const WB_LF: u8 = 2;
const WB_NEWLINE: u8 = 3;
const WB_EXTEND: u8 = 4;
const WB_ZWJ: u8 = 5;
const WB_RI: u8 = 6;
const WB_FORMAT: u8 = 7;
const WB_KATAKANA: u8 = 8;
const WB_HEBREW: u8 = 9;
const WB_ALETTER: u8 = 10;
const WB_SINGLEQUOTE: u8 = 11;
const WB_DOUBLEQUOTE: u8 = 12;
const WB_MIDNUMLET: u8 = 13;
const WB_MIDLETTER: u8 = 14;
const WB_MIDNUM: u8 = 15;
const WB_NUMERIC: u8 = 16;
const WB_EXTENDNUMLET: u8 = 17;
const WB_WSEGSPACE: u8 = 18;

/// WB4: `Extend`/`Format`/`ZWJ` are transparent — they attach to the
/// preceding "word unit" and never start a new one. Reduces the raw
/// codepoint sequence to word units, each remembering the ORIGINAL index of
/// its last raw char (`ends[i]`) so boundary decisions map back onto the
/// real string.
fn word_reduce(cps: &[char]) -> (Vec<(u8, char)>, Vec<usize>) {
    let mut units: Vec<(u8, char)> = Vec::new();
    let mut ends: Vec<usize> = Vec::new();
    for (idx, &c) in cps.iter().enumerate() {
        let cl = word_class(c as u32);
        if matches!(cl, WB_EXTEND | WB_FORMAT | WB_ZWJ) {
            if let Some(&(last_cl, _)) = units.last() {
                // WB4's attach rule has an explicit carve-out: "except after
                // sot, or after CR, LF, or Newline" — a CR/LF/Newline unit
                // always starts its own boundary, so a following Extend/
                // Format/ZWJ must NOT merge into it (conformance-verified:
                // WordBreakTest.txt has `CR × Extend` cases wanting `÷`).
                if !matches!(last_cl, WB_CR | WB_LF | WB_NEWLINE) {
                    *ends.last_mut().unwrap() = idx;
                    continue;
                }
            }
        }
        units.push((cl, c));
        ends.push(idx);
    }
    (units, ends)
}

/// WB3-WB13b (pairwise, with the single lookback/lookahead unit each rule
/// needs). WB15/16's regional-indicator parity is stateful (running count),
/// handled by the caller.
fn word_break_at(units: &[(u8, char)], ends: &[usize], cps: &[char], i: usize) -> bool {
    let (pc, _) = units[i];
    let (cc, _) = units[i + 1];
    if pc == WB_CR && cc == WB_LF {
        return false; // WB3
    }
    if matches!(pc, WB_NEWLINE | WB_CR | WB_LF) {
        return true; // WB3a
    }
    if matches!(cc, WB_NEWLINE | WB_CR | WB_LF) {
        return true; // WB3b
    }
    // WB3c: ZWJ x Extended_Pictographic (the last RAW char merged into unit i
    // may be the ZWJ itself even though `pc` is the unit's anchor class).
    if word_class(cps[ends[i]] as u32) == WB_ZWJ && is_ext_pictographic(units[i + 1].1 as u32) {
        return false;
    }
    // WB3d: requires the two WSegSpace runes to be RAW-adjacent — a merged
    // Extend/Format/ZWJ between them (attached into unit `i` per WB4) means
    // the immediately preceding raw char is no longer WSegSpace, so this
    // does NOT fire (conformance-verified: `SPACE Extend SPACE` breaks at
    // the second boundary, `[999.0]` in WordBreakTest.txt, not WB3d).
    if pc == WB_WSEGSPACE && cc == WB_WSEGSPACE && word_class(cps[ends[i]] as u32) == WB_WSEGSPACE {
        return false;
    }
    if matches!(pc, WB_ALETTER | WB_HEBREW) && matches!(cc, WB_ALETTER | WB_HEBREW) {
        return false; // WB5
    }
    if matches!(pc, WB_ALETTER | WB_HEBREW) && matches!(cc, WB_MIDLETTER | WB_MIDNUMLET | WB_SINGLEQUOTE) {
        if let Some(&(nc, _)) = units.get(i + 2) {
            if matches!(nc, WB_ALETTER | WB_HEBREW) {
                return false; // WB6
            }
        }
    }
    if matches!(cc, WB_ALETTER | WB_HEBREW) && matches!(pc, WB_MIDLETTER | WB_MIDNUMLET | WB_SINGLEQUOTE) {
        if i >= 1 {
            let (pp, _) = units[i - 1];
            if matches!(pp, WB_ALETTER | WB_HEBREW) {
                return false; // WB7
            }
        }
    }
    if pc == WB_HEBREW && cc == WB_SINGLEQUOTE {
        return false; // WB7a
    }
    if pc == WB_HEBREW && cc == WB_DOUBLEQUOTE {
        if let Some(&(nc, _)) = units.get(i + 2) {
            if nc == WB_HEBREW {
                return false; // WB7b
            }
        }
    }
    if pc == WB_DOUBLEQUOTE && cc == WB_HEBREW {
        if i >= 1 {
            let (pp, _) = units[i - 1];
            if pp == WB_HEBREW {
                return false; // WB7c
            }
        }
    }
    if pc == WB_NUMERIC && cc == WB_NUMERIC {
        return false; // WB8
    }
    if matches!(pc, WB_ALETTER | WB_HEBREW) && cc == WB_NUMERIC {
        return false; // WB9
    }
    if pc == WB_NUMERIC && matches!(cc, WB_ALETTER | WB_HEBREW) {
        return false; // WB10
    }
    if cc == WB_NUMERIC && matches!(pc, WB_MIDNUM | WB_MIDNUMLET | WB_SINGLEQUOTE) {
        if i >= 1 {
            let (pp, _) = units[i - 1];
            if pp == WB_NUMERIC {
                return false; // WB11
            }
        }
    }
    if pc == WB_NUMERIC && matches!(cc, WB_MIDNUM | WB_MIDNUMLET | WB_SINGLEQUOTE) {
        if let Some(&(nc, _)) = units.get(i + 2) {
            if nc == WB_NUMERIC {
                return false; // WB12
            }
        }
    }
    if pc == WB_KATAKANA && cc == WB_KATAKANA {
        return false; // WB13
    }
    if matches!(pc, WB_ALETTER | WB_HEBREW | WB_NUMERIC | WB_KATAKANA | WB_EXTENDNUMLET) && cc == WB_EXTENDNUMLET {
        return false; // WB13a
    }
    if pc == WB_EXTENDNUMLET && matches!(cc, WB_ALETTER | WB_HEBREW | WB_NUMERIC | WB_KATAKANA) {
        return false; // WB13b
    }
    let _ = WB_OTHER;
    true // WB999
}

/// The full UAX#29 word segmentation — every boundary, including
/// whitespace-only and punctuation-only spans (what `WordBreakTest.txt`
/// checks). `words()` derives its public "give me the tokens" surface from
/// this by keeping only segments that contain a letter or digit.
pub(super) fn word_segments(s: &str) -> Vec<String> {
    let cps: Vec<char> = s.chars().collect();
    if cps.is_empty() {
        return Vec::new();
    }
    let (units, ends) = word_reduce(&cps);
    let n = units.len();
    let mut brk = vec![true; n.saturating_sub(1)];
    let mut ri_run: usize = 0;
    for i in 0..n {
        let (cl, _) = units[i];
        ri_run = if cl == WB_RI { ri_run + 1 } else { 0 };
        if i + 1 < n {
            let (nc, _) = units[i + 1];
            brk[i] = if cl == WB_RI && nc == WB_RI {
                ri_run % 2 == 0 // WB15/WB16
            } else {
                word_break_at(&units, &ends, &cps, i)
            };
        }
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut ui = 0usize;
    for (idx, &c) in cps.iter().enumerate() {
        cur.push(c);
        if idx == ends[ui] {
            if ui + 1 < n {
                if brk[ui] {
                    out.push(std::mem::take(&mut cur));
                }
                ui += 1;
            }
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub(super) fn words(s: &str) -> Vec<String> {
    word_segments(s)
        .into_iter()
        .filter(|w| w.chars().any(|c| alphabetic(c as u32) || numeric(c as u32)))
        .collect()
}

// ---- SB1-SB11 sentence boundaries -------------------------------------------
const SB_CR: u8 = 1;
const SB_LF: u8 = 2;
const SB_EXTEND: u8 = 3;
const SB_SEP: u8 = 4;
const SB_FORMAT: u8 = 5;
const SB_SP: u8 = 6;
const SB_LOWER: u8 = 7;
const SB_UPPER: u8 = 8;
const SB_OLETTER: u8 = 9;
const SB_NUMERIC: u8 = 10;
const SB_ATERM: u8 = 11;
const SB_SCONTINUE: u8 = 12;
const SB_STERM: u8 = 13;
const SB_CLOSE: u8 = 14;

/// The full UAX#29 sentence segmentation (SB1-SB11) over the reduced class
/// sequence. Returns `true` at position `i` when a break falls after unit
/// `i` (length `units.len() - 1`).
///
/// SB999's default is "do NOT break" (`Any × Any`) — unlike GB999/WB999,
/// which default to breaking. A sentence keeps accumulating text until an
/// explicit separator (SB4) or a terminator run (SB11) closes it; that is
/// why plain adjacent `Other` characters stay joined in the official
/// SentenceBreakTest.txt corpus.
fn sentence_breaks(units: &[u8]) -> Vec<bool> {
    let n = units.len();
    let mut brk = vec![false; n.saturating_sub(1)];
    let mut i = 0usize;
    while i + 1 < n {
        let cc0 = units[i];
        if cc0 == SB_CR && units[i + 1] == SB_LF {
            i += 1; // SB3 (redundant with the default, kept for clarity)
            continue;
        }
        if matches!(cc0, SB_SEP | SB_CR | SB_LF) {
            brk[i] = true; // SB4
            i += 1;
            continue;
        }
        if matches!(cc0, SB_ATERM | SB_STERM) {
            // SB6: ATerm x Numeric (a decimal point like "3.0" never ends a sentence).
            if cc0 == SB_ATERM && units[i + 1] == SB_NUMERIC {
                i += 1;
                continue;
            }
            // SB7: (Upper|Lower) ATerm x Upper (abbreviation-style initials).
            if cc0 == SB_ATERM
                && i > 0
                && matches!(units[i - 1], SB_UPPER | SB_LOWER)
                && i + 1 < n
                && units[i + 1] == SB_UPPER
            {
                i += 1;
                continue;
            }
            // Close* Sp* — interior boundaries stay unbroken by default
            // (SB9/SB9-interior), so no explicit action is needed for them.
            let mut j = i + 1;
            while j < n && units[j] == SB_CLOSE {
                j += 1;
            }
            let close_end = j;
            while j < n && units[j] == SB_SP {
                j += 1;
            }
            let sp_end = j;
            // SB9: (STerm|ATerm) Close* x (Close|Sp|Sep|CR|LF) — Close* is
            // maximal munch above, so only Sep/CR/LF (or a still-pending Sp,
            // already folded into `sp_end`) can land at `close_end`.
            let sb9 = close_end < n && matches!(units[close_end], SB_SEP | SB_CR | SB_LF);
            // SB10: (STerm|ATerm) Close* Sp* x (Sp|Sep|CR|LF).
            let sb10 = sp_end < n && matches!(units[sp_end], SB_SP | SB_SEP | SB_CR | SB_LF);
            // SB8a: (STerm|ATerm) Close* Sp* x (SContinue|STerm|ATerm).
            let sb8a = sp_end < n && matches!(units[sp_end], SB_SCONTINUE | SB_STERM | SB_ATERM);
            // SB8: ATerm Close* Sp* x (not OLetter/Upper/Lower/Sep/CR/LF/STerm/ATerm)* Lower.
            let sb8 = cc0 == SB_ATERM && {
                let mut k = sp_end;
                let mut found_lower = false;
                while k < n {
                    let kc = units[k];
                    if kc == SB_LOWER {
                        found_lower = true;
                        break;
                    }
                    if matches!(kc, SB_OLETTER | SB_UPPER | SB_SEP | SB_CR | SB_LF | SB_STERM | SB_ATERM) {
                        break;
                    }
                    k += 1;
                }
                found_lower
            };
            // SB11: otherwise, break right after Close* Sp*.
            if !(sb9 || sb10 || sb8a || sb8) && sp_end > 0 && sp_end - 1 < brk.len() {
                brk[sp_end - 1] = true;
            }
            i = if sp_end > i { sp_end } else { i + 1 };
            continue;
        }
        i += 1;
    }
    brk
}

/// The full UAX#29 sentence segmentation, untrimmed (what
/// `SentenceBreakTest.txt` checks — trailing space/close-punctuation stays
/// attached to its sentence per SB8a-SB10). `sentences()` derives the public
/// surface by trimming and dropping empties.
pub(super) fn sentence_segments(s: &str) -> Vec<String> {
    let cps: Vec<char> = s.chars().collect();
    if cps.is_empty() {
        return Vec::new();
    }
    // Reduction drops Extend/Format chars from the CLASS sequence but every
    // raw char still needs to land in the output text, so track original
    // indices for reduced units the same way `word_reduce` does.
    let mut units: Vec<u8> = Vec::new();
    let mut ends: Vec<usize> = Vec::new();
    for (idx, &c) in cps.iter().enumerate() {
        let cl = sentence_class(c as u32);
        if matches!(cl, SB_EXTEND | SB_FORMAT) {
            if let Some(&last_cl) = units.last() {
                // SB5's attach rule has the same "except after sot, CR, LF,
                // or Sep" carve-out as WB4 — see `word_reduce`.
                if !matches!(last_cl, SB_CR | SB_LF | SB_SEP) {
                    *ends.last_mut().unwrap() = idx;
                    continue;
                }
            }
        }
        units.push(cl);
        ends.push(idx);
    }
    let brk = sentence_breaks(&units);
    let n = units.len();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut ui = 0usize;
    for (idx, &c) in cps.iter().enumerate() {
        cur.push(c);
        if idx == ends[ui] {
            if ui + 1 < n {
                if brk[ui] {
                    out.push(std::mem::take(&mut cur));
                }
                ui += 1;
            }
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub(super) fn sentences(s: &str) -> Vec<String> {
    sentence_segments(s)
        .into_iter()
        .filter_map(|seg| {
            let t = trim(&seg);
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        })
        .collect()
}

// ---- D-TEXTWIDTH1=B: portable-default + explicit-policy display width ------
// Extended grapheme clusters via the real `graphemes()` above. East Asian
// Wide/Fullwidth and Emoji_Presentation clusters cost 2 columns; Ambiguous
// costs 1 (narrow) unless the policy asks for `.Wide`; combining/ZWJ
// components inside a cluster cost 0 (only the cluster total is charged);
// controls cost 0 unless the policy asks to `.Reject` them (a TextError).
fn cluster_is_wide(cluster: &str) -> bool {
    let codepoints: Vec<char> = cluster.chars().collect();
    let keycap = matches!(codepoints.as_slice(),
        ['0'..='9' | '#' | '*', '\u{20E3}']
        | ['0'..='9' | '#' | '*', '\u{FE0F}', '\u{20E3}']);
    let mut chars = cluster.chars();
    let ri_pair = if let Some(a) = chars.next() {
        if grapheme_class(a as u32) == GB_RI {
            matches!(chars.next(), Some(b) if grapheme_class(b as u32) == GB_RI)
        } else {
            false
        }
    } else {
        false
    };
    let emoji_style = cluster.contains('\u{FE0F}') && cluster.chars().any(|c| is_emoji(c as u32));
    keycap
        || ri_pair
        || emoji_style
        || cluster
            .chars()
            .any(|c| eaw_class(c as u32) == 2 || is_emoji_presentation(c as u32))
}

fn cluster_width(cluster: &str, ambiguous_wide: bool, controls_reject: bool) -> Result<i64, String> {
    let base = match cluster.chars().next() {
        Some(c) => c,
        None => return Ok(0),
    };
    if is_control(base as u32) {
        return if controls_reject {
            Err(format!("control character U+{:04X} rejected by TextWidth policy", base as u32))
        } else {
            Ok(0)
        };
    }
    if cluster_is_wide(cluster) {
        return Ok(2);
    }
    if cluster.chars().all(|c| is_zero_width_scalar(c as u32)) {
        return Ok(0);
    }
    if eaw_class(base as u32) == 1 {
        return Ok(if ambiguous_wide { 2 } else { 1 });
    }
    Ok(1)
}

pub(super) fn display_width_default(s: &str) -> i64 {
    graphemes(s)
        .iter()
        .map(|g| cluster_width(g, false, false).unwrap_or(0))
        .sum()
}

pub(super) fn display_width_policy(
    s: &str,
    ambiguous_wide: bool,
    controls_reject: bool,
) -> Result<i64, String> {
    let mut total = 0i64;
    for g in graphemes(s) {
        total += cluster_width(&g, ambiguous_wide, controls_reject)?;
    }
    Ok(total)
}

pub(super) fn is_alphabetic(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| alphabetic(c as u32))
}
pub(super) fn is_numeric(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| numeric(c as u32))
}
pub(super) fn is_whitespace(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| whitespace(c as u32))
}

pub(super) fn trim_start(s: &str) -> String {
    s.trim_start_matches(|c| whitespace(c as u32)).to_string()
}

pub(super) fn trim_end(s: &str) -> String {
    s.trim_end_matches(|c| whitespace(c as u32)).to_string()
}

pub(super) fn trim(s: &str) -> String {
    s.trim_matches(|c| whitespace(c as u32)).to_string()
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

fn fill_columns(fill: &str, columns: i64) -> String {
    let Some(unit) = graphemes(fill).into_iter().next() else {
        return " ".repeat(columns.max(0) as usize);
    };
    let width = display_width_default(&unit);
    if width <= 0 || width > columns {
        return " ".repeat(columns.max(0) as usize);
    }
    let repeats = columns / width;
    let remainder = columns % width;
    format!("{}{}", unit.repeat(repeats as usize), " ".repeat(remainder as usize))
}

pub(super) fn pad_start(s: &str, w: i64, fill: &str) -> String {
    let mut out = fill_columns(fill, (w - display_width_default(s)).max(0));
    out.push_str(s);
    out
}
pub(super) fn pad_end(s: &str, w: i64, fill: &str) -> String {
    let mut out = s.to_string();
    out.push_str(&fill_columns(fill, (w - display_width_default(s)).max(0)));
    out
}
pub(super) fn center(s: &str, w: i64, fill: &str) -> String {
    let gap = (w - display_width_default(s)).max(0);
    let left = gap / 2;
    let right = gap - left;
    format!("{}{}{}", fill_columns(fill, left), s, fill_columns(fill, right))
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

// Card #298: UAX#29 grapheme/word/sentence break conformance against the
// pinned Unicode 16.0.0 official {Grapheme,Word,Sentence}BreakTest.txt
// corpora (committed at tests/data/unicode/, no network/runtime file
// dependency — embedded at compile time). Each `÷`/`×`-annotated line names
// every legal break; a mismatch means the state machine above disagrees with
// the official test data.
#[cfg(test)]
mod break_conformance {
    /// Parses one `÷ 0061 × 0308 ÷ 0020 ÷` line (comment already stripped)
    /// into the full test string plus the expected list of segments (each
    /// `÷`-delimited run of codepoints).
    fn parse_break_line(line: &str) -> Option<(String, Vec<String>)> {
        let mut full = String::new();
        let mut segments: Vec<String> = Vec::new();
        let mut cur = String::new();
        let mut any_cp = false;
        for tok in line.split_whitespace() {
            match tok {
                "\u{00F7}" => {
                    if !cur.is_empty() {
                        segments.push(std::mem::take(&mut cur));
                    }
                }
                "\u{00D7}" => {}
                hex => {
                    let cp = u32::from_str_radix(hex, 16).ok()?;
                    let ch = char::from_u32(cp)?;
                    full.push(ch);
                    cur.push(ch);
                    any_cp = true;
                }
            }
        }
        if !cur.is_empty() {
            segments.push(cur);
        }
        if !any_cp {
            return None;
        }
        Some((full, segments))
    }

    fn run_corpus(corpus: &str, seg_fn: impl Fn(&str) -> Vec<String>, label: &str) -> (usize, usize) {
        let mut lines_checked = 0usize;
        let mut assertions = 0usize;
        for raw in corpus.lines() {
            let line = match raw.find('#') {
                Some(i) => &raw[..i],
                None => raw,
            };
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Some((full, expected)) = parse_break_line(line) else {
                continue;
            };
            lines_checked += 1;
            let got = seg_fn(&full);
            assert_eq!(got, expected, "{label} mismatch on line: {raw}");
            assertions += 1;
        }
        (lines_checked, assertions)
    }

    #[test]
    fn grapheme_break_test_txt_16_0_0() {
        let corpus = include_str!("../../../../tests/data/unicode/GraphemeBreakTest.txt");
        let (lines_checked, assertions) = run_corpus(corpus, |s| super::graphemes(s), "GraphemeBreakTest");
        assert!(lines_checked > 500, "expected the full corpus, only saw {lines_checked} lines");
        println!("GraphemeBreakTest.txt 16.0.0: {lines_checked} lines, {assertions} conformance assertions, all passed");
    }

    #[test]
    fn word_break_test_txt_16_0_0() {
        let corpus = include_str!("../../../../tests/data/unicode/WordBreakTest.txt");
        let (lines_checked, assertions) = run_corpus(corpus, |s| super::word_segments(s), "WordBreakTest");
        assert!(lines_checked > 500, "expected the full corpus, only saw {lines_checked} lines");
        println!("WordBreakTest.txt 16.0.0: {lines_checked} lines, {assertions} conformance assertions, all passed");
    }

    #[test]
    fn sentence_break_test_txt_16_0_0() {
        let corpus = include_str!("../../../../tests/data/unicode/SentenceBreakTest.txt");
        let (lines_checked, assertions) = run_corpus(corpus, |s| super::sentence_segments(s), "SentenceBreakTest");
        assert!(lines_checked > 100, "expected the full corpus, only saw {lines_checked} lines");
        println!("SentenceBreakTest.txt 16.0.0: {lines_checked} lines, {assertions} conformance assertions, all passed");
    }
}

#[cfg(test)]
mod unicode_property_conformance {
    use std::collections::BTreeMap;
    use std::time::{Duration, Instant};

    fn cps(field: &str) -> String {
        field
            .split_whitespace()
            .filter_map(|hex| u32::from_str_radix(hex, 16).ok())
            .filter_map(char::from_u32)
            .collect()
    }

    fn range(field: &str) -> Option<(u32, u32)> {
        if let Some((start, end)) = field.split_once("..") {
            Some((u32::from_str_radix(start.trim(), 16).ok()?, u32::from_str_radix(end.trim(), 16).ok()?))
        } else {
            let cp = u32::from_str_radix(field.trim(), 16).ok()?;
            Some((cp, cp))
        }
    }

    fn has(ranges: &[(u32, u32)], cp: u32) -> bool {
        ranges
            .binary_search_by(|&(start, end)| {
                if cp < start {
                    std::cmp::Ordering::Greater
                } else if cp > end {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .is_ok()
    }

    #[test]
    fn case_folding_txt_16_0_0_full_default() {
        let corpus = include_str!("../../../../tests/data/unicode/ucd/CaseFolding.txt");
        let mut checked = 0usize;
        for raw in corpus.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let fields: Vec<_> = line.split(';').map(str::trim).collect();
            if fields.len() < 3 || !matches!(fields[1], "C" | "F") {
                continue;
            }
            let input = cps(fields[0]);
            assert_eq!(super::casefold(&input), cps(fields[2]), "CaseFolding mismatch: {raw}");
            checked += 1;
        }
        assert!(checked > 1500, "expected full/default CaseFolding corpus, saw {checked}");
    }

    #[test]
    fn unicode_data_and_special_casing_16_0_0() {
        let mut lower = BTreeMap::<u32, String>::new();
        let mut upper = BTreeMap::<u32, String>::new();
        let unicode_data = include_str!("../../../../tests/data/unicode/ucd/UnicodeData.txt");
        for raw in unicode_data.lines() {
            let fields: Vec<_> = raw.split(';').collect();
            if fields.len() < 14 {
                continue;
            }
            let cp = u32::from_str_radix(fields[0], 16).unwrap();
            if !fields[12].is_empty() {
                upper.insert(cp, cps(fields[12]));
            }
            if !fields[13].is_empty() {
                lower.insert(cp, cps(fields[13]));
            }
        }
        let special = include_str!("../../../../tests/data/unicode/ucd/SpecialCasing.txt");
        for raw in special.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let fields: Vec<_> = line.split(';').map(str::trim).collect();
            if fields.len() < 5 || !fields[4].is_empty() {
                continue;
            }
            let cp = u32::from_str_radix(fields[0], 16).unwrap();
            lower.insert(cp, cps(fields[1]));
            upper.insert(cp, cps(fields[3]));
        }
        for cp in 0..=0x10ffff {
            let Some(ch) = char::from_u32(cp) else { continue };
            let input = ch.to_string();
            let expected_lower = lower.get(&cp).cloned().unwrap_or_else(|| input.clone());
            let expected_upper = upper.get(&cp).cloned().unwrap_or_else(|| input.clone());
            assert_eq!(super::lower(&input), expected_lower, "lowercase mismatch for U+{cp:04X}");
            assert_eq!(super::upper(&input), expected_upper, "uppercase mismatch for U+{cp:04X}");
        }
        assert_eq!(super::lower("ΟΣ"), "ος", "Final_Sigma context");
        assert_eq!(super::lower("ΟΣΑ"), "οσα", "non-final sigma context");
        assert_eq!(super::lower("ÁΣ"), "áς", "case-ignorable context");
    }

    #[test]
    fn unicode_16_classification_tables_match_official_inputs() {
        let derived = include_str!("../../../../tests/data/unicode/ucd/DerivedCoreProperties.txt");
        let alphabetic: Vec<_> = derived
            .lines()
            .filter_map(|raw| {
                let line = raw.split('#').next()?.trim();
                let (field, property) = line.split_once(';')?;
                (property.trim() == "Alphabetic").then(|| range(field).unwrap())
            })
            .collect();
        let props = include_str!("../../../../tests/data/unicode/ucd/PropList.txt");
        let whitespace: Vec<_> = props
            .lines()
            .filter_map(|raw| {
                let line = raw.split('#').next()?.trim();
                let (field, property) = line.split_once(';')?;
                (property.trim() == "White_Space").then(|| range(field).unwrap())
            })
            .collect();
        let unicode_data = include_str!("../../../../tests/data/unicode/ucd/UnicodeData.txt");
        let numeric: Vec<_> = unicode_data
            .lines()
            .filter_map(|raw| {
                let fields: Vec<_> = raw.split(';').collect();
                (fields.len() > 2 && matches!(fields[2], "Nd" | "Nl" | "No"))
                    .then(|| {
                        let cp = u32::from_str_radix(fields[0], 16).unwrap();
                        (cp, cp)
                    })
            })
            .collect();
        for cp in 0..=0x10ffff {
            if char::from_u32(cp).is_none() {
                continue;
            }
            assert_eq!(super::alphabetic(cp), has(&alphabetic, cp), "Alphabetic U+{cp:04X}");
            assert_eq!(super::numeric(cp), has(&numeric, cp), "Numeric U+{cp:04X}");
            assert_eq!(super::whitespace(cp), has(&whitespace, cp), "White_Space U+{cp:04X}");
        }
    }

    #[test]
    fn display_width_terminal_corpus() {
        assert_eq!(super::display_width_default("A·界"), 4);
        assert_eq!(super::display_width_policy("·", true, false), Ok(2));
        assert_eq!(super::display_width_default("🇺🇸"), 2);
        assert_eq!(super::display_width_default("👨‍👩‍👧‍👦"), 2);
        assert_eq!(super::display_width_default("1️⃣"), 2);
        assert_eq!(super::display_width_default("1⃣"), 2);
        assert_eq!(super::display_width_default("©️"), 2);
        assert_eq!(super::display_width_default("́‍"), 0);
        assert_eq!(super::display_width_default("\u{0}"), 0);
        assert!(super::display_width_policy("x\n", false, true).is_err());
        assert_eq!(super::display_width_default(&super::pad_start("x", 4, "界")), 4);
        assert_eq!(super::display_width_default(&super::center("x", 4, "界")), 4);
    }

    #[test]
    fn adversarial_unicode_algorithms_remain_linear_in_practice() {
        let mut combining = String::from("a");
        for _ in 0..4096 {
            combining.push('\u{0315}');
            combining.push('\u{0300}');
        }
        let started = Instant::now();
        assert_eq!(super::nfd(&combining).chars().count(), 8193);
        assert_eq!(super::graphemes(&combining).len(), 1);
        assert_eq!(super::display_width_default(&combining), 1);
        assert!(started.elapsed() < Duration::from_secs(5), "adversarial Unicode path exceeded linearity budget");

        let text = "Word. ".repeat(32768);
        let started = Instant::now();
        assert_eq!(super::words(&text).len(), 32768);
        assert_eq!(super::sentences(&text).len(), 32768);
        assert!(started.elapsed() < Duration::from_secs(5), "segmentation exceeded linearity budget");
    }
}
