//! D-BIGINT1-adjacent card #392 gap fix: `core.text` module functions
//! (`use core.text as text; text.trim(s)`, etc). Ported verbatim from AOT's
//! hand-rolled prelude implementation
//! (`crates/jet-codegen/src/Prelude/CoreLib/Top/Text.rs::jet_text_*`) so
//! comptime/REPL tier-0 matches AOT byte-for-byte (R12 parity) — these are
//! house-style approximations of Unicode NFC/NFD/casefold/segmentation
//! (no external crate, I6), not full Unicode Standard algorithms; both tiers
//! share the same approximation, so they still agree with each other.

pub(super) fn decompose_char(c: char, compat: bool, out: &mut String) {
    match c {
        'é' => out.push_str("e\u{0301}"),
        'É' => out.push_str("E\u{0301}"),
        'è' => out.push_str("e\u{0300}"),
        'á' => out.push_str("a\u{0301}"),
        'ó' => out.push_str("o\u{0301}"),
        'í' => out.push_str("i\u{0301}"),
        'ú' => out.push_str("u\u{0301}"),
        'ñ' => out.push_str("n\u{0303}"),
        'ö' => out.push_str("o\u{0308}"),
        'Ö' => out.push_str("O\u{0308}"),
        'ü' => out.push_str("u\u{0308}"),
        'Ü' => out.push_str("U\u{0308}"),
        'ä' => out.push_str("a\u{0308}"),
        'Ä' => out.push_str("A\u{0308}"),
        'ç' => out.push_str("c\u{0327}"),
        'Ç' => out.push_str("C\u{0327}"),
        'Å' => out.push_str("A\u{030A}"),
        'å' => out.push_str("a\u{030A}"),
        'ﬃ' if compat => out.push_str("ffi"),
        'ﬁ' if compat => out.push_str("fi"),
        '①' if compat => out.push('1'),
        '②' if compat => out.push('2'),
        _ => out.push(c),
    }
}

pub(super) fn compose_pair(a: char, b: char) -> Option<char> {
    match (a, b) {
        ('e', '\u{0301}') => Some('é'),
        ('E', '\u{0301}') => Some('É'),
        ('e', '\u{0300}') => Some('è'),
        ('a', '\u{0301}') => Some('á'),
        ('o', '\u{0301}') => Some('ó'),
        ('i', '\u{0301}') => Some('í'),
        ('u', '\u{0301}') => Some('ú'),
        ('n', '\u{0303}') => Some('ñ'),
        ('o', '\u{0308}') => Some('ö'),
        ('O', '\u{0308}') => Some('Ö'),
        ('u', '\u{0308}') => Some('ü'),
        ('U', '\u{0308}') => Some('Ü'),
        ('a', '\u{0308}') => Some('ä'),
        ('A', '\u{0308}') => Some('Ä'),
        ('c', '\u{0327}') => Some('ç'),
        ('C', '\u{0327}') => Some('Ç'),
        ('A', '\u{030A}') => Some('Å'),
        ('a', '\u{030A}') => Some('å'),
        _ => None,
    }
}

fn nfd_inner(s: &str, compat: bool) -> String {
    let mut out = String::new();
    for c in s.chars() {
        decompose_char(c, compat, &mut out);
    }
    out
}

pub(super) fn nfd(s: &str) -> String {
    nfd_inner(s, false)
}
pub(super) fn nfkd(s: &str) -> String {
    nfd_inner(s, true)
}

fn compose(s: String) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if let Some(&next) = it.peek() {
            if let Some(composed) = compose_pair(c, next) {
                out.push(composed);
                it.next();
                continue;
            }
        }
        out.push(c);
    }
    out
}

pub(super) fn nfc(s: &str) -> String {
    compose(nfd(s))
}
pub(super) fn nfkc(s: &str) -> String {
    compose(nfkd(s))
}

pub(super) fn casefold(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            'ß' | 'ẞ' => out.push_str("ss"),
            'ς' => out.push('σ'),
            _ => out.push_str(&c.to_lowercase().to_string()),
        }
    }
    out
}

pub(super) fn caseless_eq(a: &str, b: &str) -> bool {
    casefold(&nfkc(a)) == casefold(&nfkc(b))
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
