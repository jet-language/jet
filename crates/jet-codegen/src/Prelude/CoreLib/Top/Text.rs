// ── std.path helpers (D-IO1) ──────────────────────────────────────────────────
fn jet_std_path_join(base: &String, part: &String) -> String {
    let b = std::path::Path::new(base.as_str());
    b.join(part.as_str()).to_string_lossy().to_string()
}
fn jet_std_path_parent(path: &String) -> String {
    std::path::Path::new(path.as_str())
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}
fn jet_std_path_extension(path: &String) -> String {
    std::path::Path::new(path.as_str())
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default()
}
fn jet_std_path_normalize(path: &String) -> String {
    // Resolve `.` and `..` components without hitting the filesystem.
    let source = std::path::Path::new(path.as_str());
    let rooted = source.has_root();
    let mut normalized = std::path::PathBuf::new();
    let mut normal_depth = 0usize;
    for component in source.components() {
        match component {
            std::path::Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            std::path::Component::RootDir => normalized.push(component.as_os_str()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir if normal_depth > 0 => {
                normalized.pop();
                normal_depth -= 1;
            }
            std::path::Component::ParentDir if !rooted => normalized.push(".."),
            std::path::Component::ParentDir => {}
            std::path::Component::Normal(part) => {
                normalized.push(part);
                normal_depth += 1;
            }
        }
    }
    normalized.to_string_lossy().into_owned()
}

// ── core.text.unicode helpers (D-TEXTUNICODE1) ───────────────────────────────
fn jet_text_unicode_scalar_count(s: &String) -> i64 {
    s.chars().count() as i64
}
fn jet_text_unicode_byte_count(s: &String) -> i64 {
    s.len() as i64
}
fn jet_text_unicode_is_ascii(s: &String) -> bool {
    s.is_ascii()
}
fn jet_text_unicode_lower(s: &String) -> String {
    jet_text_lower(s)
}
fn jet_text_unicode_upper(s: &String) -> String {
    jet_text_upper(s)
}
fn jet_text_unicode_scalars(s: &String) -> Vec<String> {
    s.chars().map(|c| c.to_string()).collect()
}

// ── card #298: table-driven NFC/NFD/NFKC/NFKD + full case folding ──────────
// Real Unicode 16.0.0 algorithms over the generated tables in
// `UnicodeTables.rs` (pinned UCD, see scripts/agent/gen-unicode-tables.mjs):
// canonical/compatibility decomposition, canonical-combining-class ordering,
// Hangul algorithmic (de)composition, composition exclusions, and full
// (C+F) case folding. Replaces the old ~18-entry hand-written match table.
const HANGUL_SBASE: u32 = 0xAC00;
const HANGUL_LBASE: u32 = 0x1100;
const HANGUL_VBASE: u32 = 0x1161;
const HANGUL_TBASE: u32 = 0x11A7;
const HANGUL_LCOUNT: u32 = 19;
const HANGUL_VCOUNT: u32 = 21;
const HANGUL_TCOUNT: u32 = 28;
const HANGUL_NCOUNT: u32 = HANGUL_VCOUNT * HANGUL_TCOUNT; // 588
const HANGUL_SCOUNT: u32 = HANGUL_LCOUNT * HANGUL_NCOUNT; // 11172

fn jet_text_ccc(cp: u32) -> u8 {
    UNICODE_CCC
        .binary_search_by(|&(a, b, _)| {
            if cp < a { std::cmp::Ordering::Greater }
            else if cp > b { std::cmp::Ordering::Less }
            else { std::cmp::Ordering::Equal }
        })
        .map(|i| UNICODE_CCC[i].2)
        .unwrap_or(0)
}

fn jet_text_hangul_decompose(cp: u32) -> Option<[u32; 3]> {
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

fn jet_text_decomp_lookup(cp: u32, compat: bool) -> Option<&'static [u32]> {
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
fn jet_text_expand_char(cp: u32, compat: bool, seq: &mut Vec<u32>) {
    if let Some([l, v, t]) = jet_text_hangul_decompose(cp) {
        seq.push(l);
        seq.push(v);
        if t != 0 {
            seq.push(t);
        }
        return;
    }
    if let Some(expansion) = jet_text_decomp_lookup(cp, compat) {
        for &sub in expansion {
            jet_text_expand_char(sub, compat, seq);
        }
        return;
    }
    seq.push(cp);
}

fn jet_text_canonical_order(seq: &mut [u32]) {
    // Stable linear counting sort of each non-starter run by u8 CCC.
    let mut start = 0usize;
    while start < seq.len() {
        if jet_text_ccc(seq[start]) == 0 { start += 1; }
        let mut end = start;
        let mut ordered = true;
        let mut previous = 0u8;
        while end < seq.len() {
            let class = jet_text_ccc(seq[end]);
            if class == 0 { break; }
            ordered &= class >= previous;
            previous = class;
            end += 1;
        }
        if !ordered {
            let mut counts = [0usize; 256];
            for &cp in &seq[start..end] { counts[jet_text_ccc(cp) as usize] += 1; }
            let mut offsets = [0usize; 256];
            let mut next = start;
            for class in 1..256 {
                offsets[class] = next;
                next += counts[class];
            }
            let mut sorted = vec![0u32; end - start];
            for &cp in &seq[start..end] {
                let class = jet_text_ccc(cp) as usize;
                let at = offsets[class] - start;
                sorted[at] = cp;
                offsets[class] += 1;
            }
            seq[start..end].copy_from_slice(&sorted);
        }
        start = end;
    }
}

fn jet_text_nfd_inner(s: &String, compat: bool) -> String {
    let mut seq: Vec<u32> = Vec::with_capacity(s.len());
    for c in s.chars() {
        jet_text_expand_char(c as u32, compat, &mut seq);
    }
    jet_text_canonical_order(&mut seq);
    seq.into_iter()
        .filter_map(char::from_u32)
        .collect()
}
fn jet_text_nfd(s: &String) -> String { jet_text_nfd_inner(s, false) }
fn jet_text_nfkd(s: &String) -> String { jet_text_nfd_inner(s, true) }

fn jet_text_compose_pair_cp(a: u32, b: u32) -> Option<u32> {
    UNICODE_COMPOSE_PAIRS
        .binary_search_by(|&(x, y, _)| (x, y).cmp(&(a, b)))
        .ok()
        .map(|i| UNICODE_COMPOSE_PAIRS[i].2)
}

fn jet_text_hangul_compose_pair(a: u32, b: u32) -> Option<u32> {
    // L + V -> LV
    if a >= HANGUL_LBASE && a < HANGUL_LBASE + HANGUL_LCOUNT
        && b >= HANGUL_VBASE && b < HANGUL_VBASE + HANGUL_VCOUNT
    {
        let l_index = a - HANGUL_LBASE;
        let v_index = b - HANGUL_VBASE;
        return Some(HANGUL_SBASE + (l_index * HANGUL_VCOUNT + v_index) * HANGUL_TCOUNT);
    }
    // LV + T -> LVT
    if a >= HANGUL_SBASE && a < HANGUL_SBASE + HANGUL_SCOUNT
        && (a - HANGUL_SBASE) % HANGUL_TCOUNT == 0
        && b > HANGUL_TBASE && b < HANGUL_TBASE + HANGUL_TCOUNT
    {
        return Some(a + (b - HANGUL_TBASE));
    }
    None
}

fn jet_text_compose(s: String) -> String {
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
        let cls = jet_text_ccc(cp) as i32;
        if let Some(si) = starter_idx {
            let starter = out[si];
            let composed = jet_text_hangul_compose_pair(starter, cp)
                .or_else(|| jet_text_compose_pair_cp(starter, cp));
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
fn jet_text_nfc(s: &String) -> String { jet_text_compose(jet_text_nfd(s)) }
fn jet_text_nfkc(s: &String) -> String { jet_text_compose(jet_text_nfkd(s)) }

fn jet_text_fold_lookup(cp: u32) -> Option<&'static [u32]> {
    let idx = UNICODE_FOLD_INDEX
        .binary_search_by_key(&cp, |&(c, _, _)| c)
        .ok()?;
    let (_, start, len) = UNICODE_FOLD_INDEX[idx];
    Some(&UNICODE_FOLD_POOL[start as usize..(start + len) as usize])
}
fn jet_text_mapping_lookup(
    cp: u32,
    index: &'static [(u32, u32, u32)],
    pool: &'static [u32],
) -> Option<&'static [u32]> {
    let at = index.binary_search_by_key(&cp, |&(c, _, _)| c).ok()?;
    let (_, start, len) = index[at];
    Some(&pool[start as usize..(start + len) as usize])
}
fn jet_text_append_mapping(
    out: &mut String,
    cp: u32,
    index: &'static [(u32, u32, u32)],
    pool: &'static [u32],
) {
    if let Some(mapped) = jet_text_mapping_lookup(cp, index, pool) {
        out.extend(mapped.iter().filter_map(|&mapped_cp| char::from_u32(mapped_cp)));
    } else if let Some(ch) = char::from_u32(cp) {
        out.push(ch);
    }
}
fn jet_text_casefold(s: &String) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match jet_text_fold_lookup(c as u32) {
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
fn jet_text_simple_fold(cp: u32) -> u32 {
    UNICODE_SIMPLE_FOLD
        .binary_search_by_key(&cp, |&(source, _)| source)
        .map(|at| UNICODE_SIMPLE_FOLD[at].1)
        .unwrap_or(cp)
}
fn jet_text_property(table: &[(u32, u32)], cp: u32) -> bool {
    jet_text_bsearch_pair(table, cp)
}
fn jet_text_alphabetic(cp: u32) -> bool { jet_text_property(UNICODE_ALPHABETIC, cp) }
fn jet_text_numeric(cp: u32) -> bool { jet_text_property(UNICODE_NUMERIC, cp) }
fn jet_text_whitespace(cp: u32) -> bool { jet_text_property(UNICODE_WHITE_SPACE, cp) }
fn jet_text_letter(cp: u32) -> bool { jet_text_property(UNICODE_LETTER, cp) }
fn jet_text_is_cased(cp: u32) -> bool { jet_text_property(UNICODE_CASED, cp) }
fn jet_text_is_case_ignorable(cp: u32) -> bool { jet_text_property(UNICODE_CASE_IGNORABLE, cp) }
fn jet_text_final_sigma(chars: &[char], at: usize) -> bool {
    let before = chars[..at]
        .iter()
        .rev()
        .find(|ch| !jet_text_is_case_ignorable(**ch as u32))
        .is_some_and(|ch| jet_text_is_cased(*ch as u32));
    let after = chars[at + 1..]
        .iter()
        .find(|ch| !jet_text_is_case_ignorable(**ch as u32))
        .is_some_and(|ch| jet_text_is_cased(*ch as u32));
    before && !after
}
fn jet_text_lower(s: &String) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    for (at, &ch) in chars.iter().enumerate() {
        if ch == '\u{03A3}' && jet_text_final_sigma(&chars, at) {
            out.push('\u{03C2}');
        } else {
            jet_text_append_mapping(&mut out, ch as u32, UNICODE_LOWER_INDEX, UNICODE_LOWER_POOL);
        }
    }
    out
}
fn jet_text_upper(s: &String) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        jet_text_append_mapping(&mut out, ch as u32, UNICODE_UPPER_INDEX, UNICODE_UPPER_POOL);
    }
    out
}
// Unicode Default Caseless Matching: NFD(toCasefold(NFD(text))).
fn jet_text_caseless_eq(a: &String, b: &String) -> bool {
    jet_text_nfd(&jet_text_casefold(&jet_text_nfd(a)))
        == jet_text_nfd(&jet_text_casefold(&jet_text_nfd(b)))
}
// Card #298: real UAX#29 grapheme/word/sentence segmentation over the
// generated break-property tables, mirroring the comptime twin byte-for-byte
// (R12 parity) — see crates/jet-comptime/src/Comptime/TextLite.rs for the
// identical algorithm and the conformance tests (this AOT copy is validated
// by that mirror, not a separate compiled test — it is spliced flat into the
// user's generated program, not compiled as part of this crate).

fn jet_text_bsearch_triple(table: &[(u32, u32, u8)], cp: u32) -> u8 {
    table
        .binary_search_by(|&(a, b, _)| {
            if cp < a { std::cmp::Ordering::Greater }
            else if cp > b { std::cmp::Ordering::Less }
            else { std::cmp::Ordering::Equal }
        })
        .map(|i| table[i].2)
        .unwrap_or(0)
}
fn jet_text_bsearch_pair(table: &[(u32, u32)], cp: u32) -> bool {
    table
        .binary_search_by(|&(a, b)| {
            if cp < a { std::cmp::Ordering::Greater }
            else if cp > b { std::cmp::Ordering::Less }
            else { std::cmp::Ordering::Equal }
        })
        .is_ok()
}
fn jet_text_grapheme_class(cp: u32) -> u8 { jet_text_bsearch_triple(UNICODE_GRAPHEME_BREAK, cp) }
fn jet_text_word_class(cp: u32) -> u8 { jet_text_bsearch_triple(UNICODE_WORD_BREAK, cp) }
fn jet_text_sentence_class(cp: u32) -> u8 { jet_text_bsearch_triple(UNICODE_SENTENCE_BREAK, cp) }
fn jet_text_is_ext_pictographic(cp: u32) -> bool { jet_text_bsearch_pair(UNICODE_EXTENDED_PICTOGRAPHIC, cp) }
fn jet_text_is_emoji_presentation(cp: u32) -> bool { jet_text_bsearch_pair(UNICODE_EMOJI_PRESENTATION, cp) }
fn jet_text_is_emoji(cp: u32) -> bool { jet_text_bsearch_pair(UNICODE_EMOJI, cp) }
fn jet_text_is_default_ignorable(cp: u32) -> bool { jet_text_bsearch_pair(UNICODE_DEFAULT_IGNORABLE, cp) }
fn jet_text_general_category(cp: u32) -> u8 {
    UNICODE_GENERAL_CATEGORY
        .binary_search_by(|&(start, end, _)| {
            if cp < start { std::cmp::Ordering::Greater }
            else if cp > end { std::cmp::Ordering::Less }
            else { std::cmp::Ordering::Equal }
        })
        .map(|at| UNICODE_GENERAL_CATEGORY[at].2)
        .unwrap_or(8)
}
fn jet_text_is_control(cp: u32) -> bool { jet_text_general_category(cp) == 0 }
fn jet_text_is_zero_width_scalar(cp: u32) -> bool {
    matches!(jet_text_general_category(cp), 2 | 3) || jet_text_is_default_ignorable(cp)
}
/// East Asian Width: 0=narrow(N/Na/H) 1=Ambiguous 2=wide(W/F).
fn jet_text_eaw_class(cp: u32) -> u8 { jet_text_bsearch_triple(UNICODE_EAST_ASIAN_WIDTH, cp) }
/// Indic_Conjunct_Break (GB9c): 0=None 1=Linker 2=Consonant 3=Extend.
fn jet_text_incb_class(cp: u32) -> u8 { jet_text_bsearch_triple(UNICODE_INCB, cp) }
const JET_INCB_LINKER: u8 = 1;
const JET_INCB_CONSONANT: u8 = 2;
const JET_INCB_EXTEND: u8 = 3;

// ---- GB1-GB13 grapheme cluster boundaries -----------------------------------
const JET_GB_CR: u8 = 1;
const JET_GB_LF: u8 = 2;
const JET_GB_CONTROL: u8 = 3;
const JET_GB_EXTEND: u8 = 4;
const JET_GB_ZWJ: u8 = 5;
const JET_GB_RI: u8 = 6;
const JET_GB_PREPEND: u8 = 7;
const JET_GB_SPACINGMARK: u8 = 8;
const JET_GB_L: u8 = 9;
const JET_GB_V: u8 = 10;
const JET_GB_T: u8 = 11;
const JET_GB_LV: u8 = 12;
const JET_GB_LVT: u8 = 13;

/// GB3-GB9b/GB999 (class-pair rules only — GB11's Extended_Pictographic
/// lookup, GB12/13's regional-indicator parity, and GB9c's Indic conjunct
/// scan are stateful, handled by the caller).
fn jet_text_grapheme_break_classes(pc: u8, cc: u8) -> bool {
    if pc == JET_GB_CR && cc == JET_GB_LF { return false; } // GB3
    if matches!(pc, JET_GB_CR | JET_GB_LF | JET_GB_CONTROL) { return true; } // GB4
    if matches!(cc, JET_GB_CR | JET_GB_LF | JET_GB_CONTROL) { return true; } // GB5
    if pc == JET_GB_L && matches!(cc, JET_GB_L | JET_GB_V | JET_GB_LV | JET_GB_LVT) { return false; } // GB6
    if matches!(pc, JET_GB_LV | JET_GB_V) && matches!(cc, JET_GB_V | JET_GB_T) { return false; } // GB7
    if matches!(pc, JET_GB_LVT | JET_GB_T) && cc == JET_GB_T { return false; } // GB8
    if matches!(cc, JET_GB_EXTEND | JET_GB_ZWJ) { return false; } // GB9
    if cc == JET_GB_SPACINGMARK { return false; } // GB9a
    if pc == JET_GB_PREPEND { return false; } // GB9b
    true // GB999
}

fn jet_text_graphemes(s: &String) -> Vec<String> {
    let cps: Vec<char> = s.chars().collect();
    if cps.is_empty() { return Vec::new(); }
    let mut out = Vec::new();
    let mut cur = String::new();
    cur.push(cps[0]);
    let mut ri_run: usize = if jet_text_grapheme_class(cps[0] as u32) == JET_GB_RI { 1 } else { 0 };
    let mut saw_pic = jet_text_is_ext_pictographic(cps[0] as u32);
    let mut incb_pending = jet_text_incb_class(cps[0] as u32) == JET_INCB_CONSONANT;
    let mut incb_linker = false;
    for i in 1..cps.len() {
        let prev = cps[i - 1];
        let curc = cps[i];
        let pc = jet_text_grapheme_class(prev as u32);
        let cc = jet_text_grapheme_class(curc as u32);
        let brk = if pc == JET_GB_ZWJ && saw_pic && jet_text_is_ext_pictographic(curc as u32) {
            false // GB11
        } else if pc == JET_GB_RI && cc == JET_GB_RI {
            ri_run % 2 == 0 // GB12/GB13
        } else if incb_pending && incb_linker && jet_text_incb_class(curc as u32) == JET_INCB_CONSONANT {
            false // GB9c
        } else {
            jet_text_grapheme_break_classes(pc, cc)
        };
        if brk { out.push(std::mem::take(&mut cur)); }
        cur.push(curc);
        ri_run = if cc == JET_GB_RI { ri_run + 1 } else { 0 };
        saw_pic = if matches!(cc, JET_GB_EXTEND | JET_GB_ZWJ) { saw_pic } else { jet_text_is_ext_pictographic(curc as u32) };
        match jet_text_incb_class(curc as u32) {
            JET_INCB_CONSONANT => { incb_pending = true; incb_linker = false; }
            JET_INCB_LINKER => { if incb_pending { incb_linker = true; } }
            JET_INCB_EXTEND => {}
            _ => { incb_pending = false; incb_linker = false; }
        }
    }
    out.push(cur);
    out
}

// ---- WB1-WB16 word boundaries ------------------------------------------------
const JET_WB_CR: u8 = 1;
const JET_WB_LF: u8 = 2;
const JET_WB_NEWLINE: u8 = 3;
const JET_WB_EXTEND: u8 = 4;
const JET_WB_ZWJ: u8 = 5;
const JET_WB_RI: u8 = 6;
const JET_WB_FORMAT: u8 = 7;
const JET_WB_KATAKANA: u8 = 8;
const JET_WB_HEBREW: u8 = 9;
const JET_WB_ALETTER: u8 = 10;
const JET_WB_SINGLEQUOTE: u8 = 11;
const JET_WB_DOUBLEQUOTE: u8 = 12;
const JET_WB_MIDNUMLET: u8 = 13;
const JET_WB_MIDLETTER: u8 = 14;
const JET_WB_MIDNUM: u8 = 15;
const JET_WB_NUMERIC: u8 = 16;
const JET_WB_EXTENDNUMLET: u8 = 17;
const JET_WB_WSEGSPACE: u8 = 18;

fn jet_text_word_reduce(cps: &[char]) -> (Vec<(u8, char)>, Vec<usize>) {
    let mut units: Vec<(u8, char)> = Vec::new();
    let mut ends: Vec<usize> = Vec::new();
    for (idx, &c) in cps.iter().enumerate() {
        let cl = jet_text_word_class(c as u32);
        if matches!(cl, JET_WB_EXTEND | JET_WB_FORMAT | JET_WB_ZWJ) {
            if let Some(&(last_cl, _)) = units.last() {
                if !matches!(last_cl, JET_WB_CR | JET_WB_LF | JET_WB_NEWLINE) {
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

fn jet_text_word_break_at(units: &[(u8, char)], ends: &[usize], cps: &[char], i: usize) -> bool {
    let (pc, _) = units[i];
    let (cc, _) = units[i + 1];
    if pc == JET_WB_CR && cc == JET_WB_LF { return false; } // WB3
    if matches!(pc, JET_WB_NEWLINE | JET_WB_CR | JET_WB_LF) { return true; } // WB3a
    if matches!(cc, JET_WB_NEWLINE | JET_WB_CR | JET_WB_LF) { return true; } // WB3b
    if jet_text_word_class(cps[ends[i]] as u32) == JET_WB_ZWJ && jet_text_is_ext_pictographic(units[i + 1].1 as u32) {
        return false; // WB3c
    }
    if pc == JET_WB_WSEGSPACE && cc == JET_WB_WSEGSPACE && jet_text_word_class(cps[ends[i]] as u32) == JET_WB_WSEGSPACE {
        return false; // WB3d
    }
    if matches!(pc, JET_WB_ALETTER | JET_WB_HEBREW) && matches!(cc, JET_WB_ALETTER | JET_WB_HEBREW) { return false; } // WB5
    if matches!(pc, JET_WB_ALETTER | JET_WB_HEBREW) && matches!(cc, JET_WB_MIDLETTER | JET_WB_MIDNUMLET | JET_WB_SINGLEQUOTE) {
        if let Some(&(nc, _)) = units.get(i + 2) { if matches!(nc, JET_WB_ALETTER | JET_WB_HEBREW) { return false; } } // WB6
    }
    if matches!(cc, JET_WB_ALETTER | JET_WB_HEBREW) && matches!(pc, JET_WB_MIDLETTER | JET_WB_MIDNUMLET | JET_WB_SINGLEQUOTE) {
        if i >= 1 { let (pp, _) = units[i - 1]; if matches!(pp, JET_WB_ALETTER | JET_WB_HEBREW) { return false; } } // WB7
    }
    if pc == JET_WB_HEBREW && cc == JET_WB_SINGLEQUOTE { return false; } // WB7a
    if pc == JET_WB_HEBREW && cc == JET_WB_DOUBLEQUOTE {
        if let Some(&(nc, _)) = units.get(i + 2) { if nc == JET_WB_HEBREW { return false; } } // WB7b
    }
    if pc == JET_WB_DOUBLEQUOTE && cc == JET_WB_HEBREW {
        if i >= 1 { let (pp, _) = units[i - 1]; if pp == JET_WB_HEBREW { return false; } } // WB7c
    }
    if pc == JET_WB_NUMERIC && cc == JET_WB_NUMERIC { return false; } // WB8
    if matches!(pc, JET_WB_ALETTER | JET_WB_HEBREW) && cc == JET_WB_NUMERIC { return false; } // WB9
    if pc == JET_WB_NUMERIC && matches!(cc, JET_WB_ALETTER | JET_WB_HEBREW) { return false; } // WB10
    if cc == JET_WB_NUMERIC && matches!(pc, JET_WB_MIDNUM | JET_WB_MIDNUMLET | JET_WB_SINGLEQUOTE) {
        if i >= 1 { let (pp, _) = units[i - 1]; if pp == JET_WB_NUMERIC { return false; } } // WB11
    }
    if pc == JET_WB_NUMERIC && matches!(cc, JET_WB_MIDNUM | JET_WB_MIDNUMLET | JET_WB_SINGLEQUOTE) {
        if let Some(&(nc, _)) = units.get(i + 2) { if nc == JET_WB_NUMERIC { return false; } } // WB12
    }
    if pc == JET_WB_KATAKANA && cc == JET_WB_KATAKANA { return false; } // WB13
    if matches!(pc, JET_WB_ALETTER | JET_WB_HEBREW | JET_WB_NUMERIC | JET_WB_KATAKANA | JET_WB_EXTENDNUMLET) && cc == JET_WB_EXTENDNUMLET { return false; } // WB13a
    if pc == JET_WB_EXTENDNUMLET && matches!(cc, JET_WB_ALETTER | JET_WB_HEBREW | JET_WB_NUMERIC | JET_WB_KATAKANA) { return false; } // WB13b
    true // WB999
}

fn jet_text_word_segments(s: &String) -> Vec<String> {
    let cps: Vec<char> = s.chars().collect();
    if cps.is_empty() { return Vec::new(); }
    let (units, ends) = jet_text_word_reduce(&cps);
    let n = units.len();
    let mut brk = vec![true; n.saturating_sub(1)];
    let mut ri_run: usize = 0;
    for i in 0..n {
        let (cl, _) = units[i];
        ri_run = if cl == JET_WB_RI { ri_run + 1 } else { 0 };
        if i + 1 < n {
            let (nc, _) = units[i + 1];
            brk[i] = if cl == JET_WB_RI && nc == JET_WB_RI {
                ri_run % 2 == 0 // WB15/WB16
            } else {
                jet_text_word_break_at(&units, &ends, &cps, i)
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
                if brk[ui] { out.push(std::mem::take(&mut cur)); }
                ui += 1;
            }
        }
    }
    if !cur.is_empty() { out.push(cur); }
    out
}

fn jet_text_words(s: &String) -> Vec<String> {
    jet_text_word_segments(s)
        .into_iter()
        .filter(|w| w.chars().any(|c| jet_text_alphabetic(c as u32) || jet_text_numeric(c as u32)))
        .collect()
}

// ---- SB1-SB11 sentence boundaries -------------------------------------------
const JET_SB_CR: u8 = 1;
const JET_SB_LF: u8 = 2;
const JET_SB_EXTEND: u8 = 3;
const JET_SB_SEP: u8 = 4;
const JET_SB_FORMAT: u8 = 5;
const JET_SB_SP: u8 = 6;
const JET_SB_LOWER: u8 = 7;
const JET_SB_UPPER: u8 = 8;
const JET_SB_OLETTER: u8 = 9;
const JET_SB_NUMERIC: u8 = 10;
const JET_SB_ATERM: u8 = 11;
const JET_SB_SCONTINUE: u8 = 12;
const JET_SB_STERM: u8 = 13;
const JET_SB_CLOSE: u8 = 14;

/// SB999's default is "do NOT break" (`Any x Any`) — unlike GB999/WB999,
/// which default to breaking; see the comptime twin for the full rationale.
fn jet_text_sentence_breaks(units: &[u8]) -> Vec<bool> {
    let n = units.len();
    let mut brk = vec![false; n.saturating_sub(1)];
    let mut i = 0usize;
    while i + 1 < n {
        let cc0 = units[i];
        if cc0 == JET_SB_CR && units[i + 1] == JET_SB_LF { i += 1; continue; } // SB3
        if matches!(cc0, JET_SB_SEP | JET_SB_CR | JET_SB_LF) { brk[i] = true; i += 1; continue; } // SB4
        if matches!(cc0, JET_SB_ATERM | JET_SB_STERM) {
            if cc0 == JET_SB_ATERM && units[i + 1] == JET_SB_NUMERIC { i += 1; continue; } // SB6
            if cc0 == JET_SB_ATERM
                && i > 0
                && matches!(units[i - 1], JET_SB_UPPER | JET_SB_LOWER)
                && i + 1 < n
                && units[i + 1] == JET_SB_UPPER
            {
                i += 1; continue; // SB7
            }
            let mut j = i + 1;
            while j < n && units[j] == JET_SB_CLOSE { j += 1; }
            let close_end = j;
            while j < n && units[j] == JET_SB_SP { j += 1; }
            let sp_end = j;
            let sb9 = close_end < n && matches!(units[close_end], JET_SB_SEP | JET_SB_CR | JET_SB_LF);
            let sb10 = sp_end < n && matches!(units[sp_end], JET_SB_SP | JET_SB_SEP | JET_SB_CR | JET_SB_LF);
            let sb8a = sp_end < n && matches!(units[sp_end], JET_SB_SCONTINUE | JET_SB_STERM | JET_SB_ATERM);
            let sb8 = cc0 == JET_SB_ATERM && {
                let mut k = sp_end;
                let mut found_lower = false;
                while k < n {
                    let kc = units[k];
                    if kc == JET_SB_LOWER { found_lower = true; break; }
                    if matches!(kc, JET_SB_OLETTER | JET_SB_UPPER | JET_SB_SEP | JET_SB_CR | JET_SB_LF | JET_SB_STERM | JET_SB_ATERM) { break; }
                    k += 1;
                }
                found_lower
            };
            if !(sb9 || sb10 || sb8a || sb8) && sp_end > 0 && sp_end - 1 < brk.len() {
                brk[sp_end - 1] = true; // SB11
            }
            i = if sp_end > i { sp_end } else { i + 1 };
            continue;
        }
        i += 1;
    }
    brk
}

fn jet_text_sentence_segments(s: &String) -> Vec<String> {
    let cps: Vec<char> = s.chars().collect();
    if cps.is_empty() { return Vec::new(); }
    let mut units: Vec<u8> = Vec::new();
    let mut ends: Vec<usize> = Vec::new();
    for (idx, &c) in cps.iter().enumerate() {
        let cl = jet_text_sentence_class(c as u32);
        if matches!(cl, JET_SB_EXTEND | JET_SB_FORMAT) {
            if let Some(&last_cl) = units.last() {
                if !matches!(last_cl, JET_SB_CR | JET_SB_LF | JET_SB_SEP) {
                    *ends.last_mut().unwrap() = idx;
                    continue;
                }
            }
        }
        units.push(cl);
        ends.push(idx);
    }
    let brk = jet_text_sentence_breaks(&units);
    let n = units.len();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut ui = 0usize;
    for (idx, &c) in cps.iter().enumerate() {
        cur.push(c);
        if idx == ends[ui] {
            if ui + 1 < n {
                if brk[ui] { out.push(std::mem::take(&mut cur)); }
                ui += 1;
            }
        }
    }
    if !cur.is_empty() { out.push(cur); }
    out
}

fn jet_text_sentences(s: &String) -> Vec<String> {
    jet_text_sentence_segments(s)
        .into_iter()
        .filter_map(|seg| { let t = jet_text_trim(&seg); if t.is_empty() { None } else { Some(t) } })
        .collect()
}

// ---- D-TEXTWIDTH1=B: portable-default + explicit-policy display width ------
fn jet_text_cluster_is_wide(cluster: &str) -> bool {
    let codepoints: Vec<char> = cluster.chars().collect();
    let keycap = matches!(codepoints.as_slice(),
        ['0'..='9' | '#' | '*', '\u{20E3}']
        | ['0'..='9' | '#' | '*', '\u{FE0F}', '\u{20E3}']);
    let mut chars = cluster.chars();
    let ri_pair = if let Some(a) = chars.next() {
        if jet_text_grapheme_class(a as u32) == JET_GB_RI {
            matches!(chars.next(), Some(b) if jet_text_grapheme_class(b as u32) == JET_GB_RI)
        } else { false }
    } else { false };
    let emoji_style = cluster.contains('\u{FE0F}') && cluster.chars().any(|c| jet_text_is_emoji(c as u32));
    keycap || ri_pair || emoji_style || cluster.chars().any(|c| jet_text_eaw_class(c as u32) == 2 || jet_text_is_emoji_presentation(c as u32))
}
fn jet_text_cluster_width(cluster: &str, ambiguous_wide: bool, controls_reject: bool) -> Result<i64, String> {
    let base = match cluster.chars().next() { Some(c) => c, None => return Ok(0) };
    if jet_text_is_control(base as u32) {
        return if controls_reject {
            Err(format!("control character U+{:04X} rejected by TextWidth policy", base as u32))
        } else {
            Ok(0)
        };
    }
    if jet_text_cluster_is_wide(cluster) { return Ok(2); }
    if cluster.chars().all(|c| jet_text_is_zero_width_scalar(c as u32)) { return Ok(0); }
    if jet_text_eaw_class(base as u32) == 1 { return Ok(if ambiguous_wide { 2 } else { 1 }); }
    Ok(1)
}
fn jet_text_display_width_default(s: &String) -> i64 {
    jet_text_graphemes(s).iter().map(|g| jet_text_cluster_width(g, false, false).unwrap_or(0)).sum()
}
fn jet_text_display_width_policy(s: &String, ambiguous_wide: bool, controls_reject: bool) -> Result<i64, String> {
    let mut total = 0i64;
    for g in jet_text_graphemes(s) {
        total += jet_text_cluster_width(&g, ambiguous_wide, controls_reject)?;
    }
    Ok(total)
}
fn jet_text_display_width(s: &String, policy: &jet_std::TextWidth) -> Result<i64, jet_std::TextError> {
    let ambiguous_wide = matches!(policy.ambiguous, jet_std::TextWidthAmbiguous::Wide);
    let controls_reject = matches!(policy.controls, jet_std::TextWidthControls::Reject);
    jet_text_display_width_policy(s, ambiguous_wide, controls_reject)
        .map_err(|message| jet_std::TextError { message })
}
fn jet_text_is_alphabetic(s: &String) -> bool { !s.is_empty() && s.chars().all(|c| jet_text_alphabetic(c as u32)) }
fn jet_text_is_numeric(s: &String) -> bool { !s.is_empty() && s.chars().all(|c| jet_text_numeric(c as u32)) }
fn jet_text_is_whitespace(s: &String) -> bool { !s.is_empty() && s.chars().all(|c| jet_text_whitespace(c as u32)) }
fn jet_text_trim_start(s: &String) -> String { s.trim_start_matches(|c| jet_text_whitespace(c as u32)).to_string() }
fn jet_text_trim_end(s: &String) -> String { s.trim_end_matches(|c| jet_text_whitespace(c as u32)).to_string() }
fn jet_text_trim(s: &String) -> String { s.trim_matches(|c| jet_text_whitespace(c as u32)).to_string() }
fn jet_text_splitn(s: &String, pat: &String, n: i64) -> Vec<String> {
    s.splitn(n.max(0) as usize, pat).map(|x| x.to_string()).collect()
}
fn jet_text_rsplitn(s: &String, pat: &String, n: i64) -> Vec<String> {
    s.rsplitn(n.max(0) as usize, pat).map(|x| x.to_string()).collect()
}
fn jet_text_fill_columns(fill: &String, columns: i64) -> String {
    let Some(unit) = jet_text_graphemes(fill).into_iter().next() else {
        return " ".repeat(columns.max(0) as usize);
    };
    let width = jet_text_display_width_default(&unit);
    if width <= 0 || width > columns {
        return " ".repeat(columns.max(0) as usize);
    }
    let repeats = columns / width;
    let remainder = columns % width;
    format!("{}{}", unit.repeat(repeats as usize), " ".repeat(remainder as usize))
}
fn jet_text_pad_start(s: &String, width: i64, fill: &String) -> String {
    let mut out = jet_text_fill_columns(fill, (width - jet_text_display_width_default(s)).max(0));
    out.push_str(s);
    out
}
fn jet_text_pad_end(s: &String, width: i64, fill: &String) -> String {
    let mut out = s.clone();
    out.push_str(&jet_text_fill_columns(fill, (width - jet_text_display_width_default(s)).max(0)));
    out
}
fn jet_text_center(s: &String, width: i64, fill: &String) -> String {
    let gap = (width - jet_text_display_width_default(s)).max(0);
    let left = gap / 2;
    let right = gap - left;
    format!("{}{}{}", jet_text_fill_columns(fill, left), s, jet_text_fill_columns(fill, right))
}
fn jet_text_starts_any(s: &String, prefixes: &Vec<String>) -> bool {
    prefixes.iter().any(|p| s.starts_with(p))
}
fn jet_text_ends_any(s: &String, suffixes: &Vec<String>) -> bool {
    suffixes.iter().any(|p| s.ends_with(p))
}
fn jet_text_char_indices(s: &String) -> Vec<String> {
    s.char_indices().map(|(i, c)| format!("{}:{}", i, c)).collect()
}

fn jet_std_fs_read(path: &String) -> Result<String, jet_std::IoError> {
    std::fs::read_to_string(path).map_err(|e| jet_std::io_error_at(jet_std::IoOperation::Read, path, e))
}
fn jet_std_fs_read_bytes(path: &String) -> Result<Vec<u8>, jet_std::IoError> {
    std::fs::read(path).map_err(|e| jet_std::io_error_at(jet_std::IoOperation::Read, path, e))
}
fn jet_std_fs_write(path: &String, text: &String) -> Result<(), jet_std::IoError> {
    std::fs::write(path, text).map_err(|e| jet_std::io_error_at(jet_std::IoOperation::Write, path, e))
}
fn jet_std_fs_append(path: &String, text: &String) -> Result<(), jet_std::IoError> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| jet_std::io_error_at(jet_std::IoOperation::Write, path, e))?;
    f.write_all(text.as_bytes())
        .map_err(|e| jet_std::io_error_at(jet_std::IoOperation::Write, path, e))
}
fn jet_std_fs_exists(path: &String) -> bool {
    std::path::Path::new(path).exists()
}
fn jet_std_fs_remove(path: &String) -> Result<(), jet_std::IoError> {
    std::fs::remove_file(path).map_err(|e| jet_std::io_error_at(jet_std::IoOperation::Write, path, e))
}
// D-LSDIR1=A: returns DirEntry values with name, full path, and is_dir flag.
fn jet_std_fs_list_dir(path: &String) -> Result<Vec<jet_std::DirEntry>, jet_std::IoError> {
    let mut out = Vec::new();
    let rd = std::fs::read_dir(path).map_err(|e| jet_std::io_error_at(jet_std::IoOperation::Read, path, e))?;
    for entry in rd {
        let entry = entry.map_err(|e| jet_std::io_error_at(jet_std::IoOperation::Read, path, e))?;
        let name = entry.file_name().to_string_lossy().to_string();
        let full_path = std::path::Path::new(path.as_str())
            .join(&name)
            .to_string_lossy()
            .to_string();
        let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
        out.push(jet_std::DirEntry {
            name,
            path: full_path,
            is_dir,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}
fn jet_std_fs_create_dir(path: &String) -> Result<(), jet_std::IoError> {
    std::fs::create_dir_all(path).map_err(|e| jet_std::io_error_at(jet_std::IoOperation::Write, path, e))
}
fn jet_std_fs_create_dir_all(path: &String) -> Result<(), jet_std::IoError> {
    std::fs::create_dir_all(path).map_err(|e| jet_std::io_error_at(jet_std::IoOperation::Write, path, e))
}
fn jet_std_fs_is_dir(path: &String) -> bool {
    std::path::Path::new(path).is_dir()
}
fn jet_std_fs_remove_dir(path: &String) -> Result<(), jet_std::IoError> {
    std::fs::remove_dir(path).map_err(|e| jet_std::io_error_at(jet_std::IoOperation::Write, path, e))
}
fn jet_std_fs_remove_all(path: &String) -> Result<(), jet_std::IoError> {
    let p = std::path::Path::new(path);
    if p.is_dir() {
        std::fs::remove_dir_all(path).map_err(|e| jet_std::io_error_at(jet_std::IoOperation::Write, path, e))
    } else {
        std::fs::remove_file(path).map_err(|e| jet_std::io_error_at(jet_std::IoOperation::Write, path, e))
    }
}
fn jet_std_fs_copy(from: &String, to: &String) -> Result<(), jet_std::IoError> {
    std::fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| jet_std::io_error_at(jet_std::IoOperation::Write, from, e))
}
