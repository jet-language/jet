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
    s.to_lowercase()
}
fn jet_text_unicode_upper(s: &String) -> String {
    s.to_uppercase()
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
    // Stable insertion-sort of adjacent nonzero-ccc runs (UAX#15 canonical ordering).
    for i in 1..seq.len() {
        let cls = jet_text_ccc(seq[i]);
        if cls == 0 {
            continue;
        }
        let mut j = i;
        while j > 0 && jet_text_ccc(seq[j - 1]) > cls {
            seq.swap(j - 1, j);
            j -= 1;
        }
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
// caseless_eq = full case fold + NFD compare (AOT doc contract): normalize
// each side to NFD, then apply full case folding, then compare byte-for-byte.
fn jet_text_caseless_eq(a: &String, b: &String) -> bool {
    jet_text_casefold(&jet_text_nfd(a)) == jet_text_casefold(&jet_text_nfd(b))
}
fn jet_text_is_mark(c: char) -> bool {
    matches!(c as u32, 0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F)
}
fn jet_text_graphemes(s: &String) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in s.chars() {
        if jet_text_is_mark(c) || c == '\u{200D}' {
            if let Some(last) = out.last_mut() { last.push(c); } else { out.push(c.to_string()); }
        } else {
            out.push(c.to_string());
        }
    }
    out
}
fn jet_text_words(s: &String) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        if c.is_alphanumeric() || c == '\'' { cur.push(c); }
        else if !cur.is_empty() { out.push(std::mem::take(&mut cur)); }
    }
    if !cur.is_empty() { out.push(cur); }
    out
}
fn jet_text_sentences(s: &String) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        cur.push(c);
        if matches!(c, '.' | '!' | '?') {
            let t = cur.trim();
            if !t.is_empty() { out.push(t.to_string()); }
            cur.clear();
        }
    }
    let t = cur.trim();
    if !t.is_empty() { out.push(t.to_string()); }
    out
}
fn jet_text_char_width(c: char) -> i64 {
    let u = c as u32;
    if c == '\0' || c.is_control() || jet_text_is_mark(c) { 0 }
    else if matches!(u, 0x1100..=0x115F | 0x2329..=0x232A | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE10..=0xFE19 | 0xFE30..=0xFE6F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6 | 0x1F300..=0x1FAFF) { 2 }
    else { 1 }
}
fn jet_text_width(s: &String) -> i64 { s.chars().map(jet_text_char_width).sum() }
fn jet_text_is_alphabetic(s: &String) -> bool { !s.is_empty() && s.chars().all(|c| c.is_alphabetic()) }
fn jet_text_is_numeric(s: &String) -> bool { !s.is_empty() && s.chars().all(|c| c.is_numeric()) }
fn jet_text_is_whitespace(s: &String) -> bool { !s.is_empty() && s.chars().all(|c| c.is_whitespace()) }
fn jet_text_splitn(s: &String, pat: &String, n: i64) -> Vec<String> {
    s.splitn(n.max(0) as usize, pat).map(|x| x.to_string()).collect()
}
fn jet_text_rsplitn(s: &String, pat: &String, n: i64) -> Vec<String> {
    s.rsplitn(n.max(0) as usize, pat).map(|x| x.to_string()).collect()
}
fn jet_text_pad_start(s: &String, width: i64, fill: &String) -> String {
    let mut out = String::new();
    let f = fill.chars().next().unwrap_or(' ');
    for _ in 0..(width - jet_text_width(s)).max(0) { out.push(f); }
    out.push_str(s);
    out
}
fn jet_text_pad_end(s: &String, width: i64, fill: &String) -> String {
    let mut out = s.clone();
    let f = fill.chars().next().unwrap_or(' ');
    for _ in 0..(width - jet_text_width(s)).max(0) { out.push(f); }
    out
}
fn jet_text_center(s: &String, width: i64, fill: &String) -> String {
    let gap = (width - jet_text_width(s)).max(0);
    let left = gap / 2;
    let right = gap - left;
    let f = fill.chars().next().unwrap_or(' ');
    format!("{}{}{}", f.to_string().repeat(left as usize), s, f.to_string().repeat(right as usize))
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
    std::fs::read_to_string(path).map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_read_bytes(path: &String) -> Result<Vec<u8>, jet_std::IoError> {
    std::fs::read(path).map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_write(path: &String, text: &String) -> Result<(), jet_std::IoError> {
    std::fs::write(path, text).map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_append(path: &String, text: &String) -> Result<(), jet_std::IoError> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| jet_std::io_error(path, e))?;
    f.write_all(text.as_bytes())
        .map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_exists(path: &String) -> bool {
    std::path::Path::new(path).exists()
}
fn jet_std_fs_remove(path: &String) -> Result<(), jet_std::IoError> {
    std::fs::remove_file(path).map_err(|e| jet_std::io_error(path, e))
}
// D-LSDIR1=A: returns DirEntry values with name, full path, and is_dir flag.
fn jet_std_fs_list_dir(path: &String) -> Result<Vec<jet_std::DirEntry>, jet_std::IoError> {
    let mut out = Vec::new();
    let rd = std::fs::read_dir(path).map_err(|e| jet_std::io_error(path, e))?;
    for entry in rd {
        let entry = entry.map_err(|e| jet_std::io_error(path, e))?;
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
    std::fs::create_dir_all(path).map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_create_dir_all(path: &String) -> Result<(), jet_std::IoError> {
    std::fs::create_dir_all(path).map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_is_dir(path: &String) -> bool {
    std::path::Path::new(path).is_dir()
}
fn jet_std_fs_remove_dir(path: &String) -> Result<(), jet_std::IoError> {
    std::fs::remove_dir(path).map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_remove_all(path: &String) -> Result<(), jet_std::IoError> {
    let p = std::path::Path::new(path);
    if p.is_dir() {
        std::fs::remove_dir_all(path).map_err(|e| jet_std::io_error(path, e))
    } else {
        std::fs::remove_file(path).map_err(|e| jet_std::io_error(path, e))
    }
}
fn jet_std_fs_copy(from: &String, to: &String) -> Result<(), jet_std::IoError> {
    std::fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| jet_std::io_error(from, e))
}
