// Shared filesystem operation kernels used by every native adapter.
//
// The surrounding Prelude parts own fault injection and `IOError` projection;
// this part owns the actual filesystem operation so AOT, JIT, and the
// interpreter cannot grow separate filesystem algorithms.
//
// Plain `//` comments: this file is `include!`d into jet-comptime, where
// inner `//!` docs are illegal.

pub fn jet_fs_rename(from: &str, to: &str) -> std::io::Result<()> {
    std::fs::rename(from, to)
}

pub fn jet_fs_open(path: &str) -> std::io::Result<std::fs::File> {
    std::fs::File::open(path)
}

pub fn jet_fs_canonicalize(path: &str) -> std::io::Result<String> {
    std::fs::canonicalize(path).map(|path| path.to_string_lossy().into_owned())
}

pub fn jet_fs_glob(pattern: &str) -> std::io::Result<Vec<String>> {
    let split = pattern.find(['*', '?']).unwrap_or(pattern.len());
    let base = pattern[..split]
        .rsplit_once(std::path::MAIN_SEPARATOR)
        .map(|(dir, _)| if dir.is_empty() { "." } else { dir })
        .unwrap_or(".");
    let mut matches = Vec::new();
    collect_glob_entries(std::path::Path::new(base), &mut matches)?;
    matches.retain(|path| jet_fs_glob_match(pattern, path));
    matches.sort();
    Ok(matches)
}

fn collect_glob_entries(root: &std::path::Path, output: &mut Vec<String>) -> std::io::Result<()> {
    let mut entries = std::fs::read_dir(root)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        output.push(path.to_string_lossy().into_owned());
        if entry.file_type()?.is_dir() {
            collect_glob_entries(&path, output)?;
        }
    }
    Ok(())
}

fn jet_fs_glob_match(pattern: &str, text: &str) -> bool {
    fn inner(pattern: &[u8], text: &[u8]) -> bool {
        if pattern.is_empty() {
            return text.is_empty();
        }
        match pattern[0] {
            b'*' => inner(&pattern[1..], text) || (!text.is_empty() && inner(pattern, &text[1..])),
            b'?' => !text.is_empty() && inner(&pattern[1..], &text[1..]),
            character => {
                !text.is_empty() && character == text[0] && inner(&pattern[1..], &text[1..])
            }
        }
    }
    inner(pattern.as_bytes(), text.as_bytes())
}
