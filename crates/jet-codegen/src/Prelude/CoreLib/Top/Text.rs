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
    let mut parts: Vec<&str> = Vec::new();
    let s = path.as_str();
    let absolute = s.starts_with('/');
    for seg in s.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    let joined = parts.join("/");
    if absolute {
        format!("/{}", joined)
    } else {
        joined
    }
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
fn jet_text_decompose_char(c: char, compat: bool, out: &mut String) {
    match c {
        'é' => out.push_str("e\u{0301}"), 'É' => out.push_str("E\u{0301}"),
        'è' => out.push_str("e\u{0300}"), 'á' => out.push_str("a\u{0301}"),
        'ó' => out.push_str("o\u{0301}"), 'í' => out.push_str("i\u{0301}"),
        'ú' => out.push_str("u\u{0301}"), 'ñ' => out.push_str("n\u{0303}"),
        'ö' => out.push_str("o\u{0308}"), 'Ö' => out.push_str("O\u{0308}"),
        'ü' => out.push_str("u\u{0308}"), 'Ü' => out.push_str("U\u{0308}"),
        'ä' => out.push_str("a\u{0308}"), 'Ä' => out.push_str("A\u{0308}"),
        'ç' => out.push_str("c\u{0327}"), 'Ç' => out.push_str("C\u{0327}"),
        'Å' => out.push_str("A\u{030A}"), 'å' => out.push_str("a\u{030A}"),
        'ﬃ' if compat => out.push_str("ffi"), 'ﬁ' if compat => out.push_str("fi"),
        '①' if compat => out.push('1'), '②' if compat => out.push('2'),
        _ => out.push(c),
    }
}
fn jet_text_compose_pair(a: char, b: char) -> Option<char> {
    match (a, b) {
        ('e', '\u{0301}') => Some('é'), ('E', '\u{0301}') => Some('É'),
        ('e', '\u{0300}') => Some('è'), ('a', '\u{0301}') => Some('á'),
        ('o', '\u{0301}') => Some('ó'), ('i', '\u{0301}') => Some('í'),
        ('u', '\u{0301}') => Some('ú'), ('n', '\u{0303}') => Some('ñ'),
        ('o', '\u{0308}') => Some('ö'), ('O', '\u{0308}') => Some('Ö'),
        ('u', '\u{0308}') => Some('ü'), ('U', '\u{0308}') => Some('Ü'),
        ('a', '\u{0308}') => Some('ä'), ('A', '\u{0308}') => Some('Ä'),
        ('c', '\u{0327}') => Some('ç'), ('C', '\u{0327}') => Some('Ç'),
        ('A', '\u{030A}') => Some('Å'), ('a', '\u{030A}') => Some('å'),
        _ => None,
    }
}
fn jet_text_nfd_inner(s: &String, compat: bool) -> String {
    let mut out = String::new();
    for c in s.chars() { jet_text_decompose_char(c, compat, &mut out); }
    out
}
fn jet_text_nfd(s: &String) -> String { jet_text_nfd_inner(s, false) }
fn jet_text_nfkd(s: &String) -> String { jet_text_nfd_inner(s, true) }
fn jet_text_compose(s: String) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if let Some(&next) = it.peek() {
            if let Some(composed) = jet_text_compose_pair(c, next) {
                out.push(composed);
                it.next();
                continue;
            }
        }
        out.push(c);
    }
    out
}
fn jet_text_nfc(s: &String) -> String { jet_text_compose(jet_text_nfd(s)) }
fn jet_text_nfkc(s: &String) -> String { jet_text_compose(jet_text_nfkd(s)) }
fn jet_text_casefold(s: &String) -> String {
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
fn jet_text_caseless_eq(a: &String, b: &String) -> bool {
    jet_text_casefold(&jet_text_nfkc(a)) == jet_text_casefold(&jet_text_nfkc(b))
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
