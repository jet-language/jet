//! Package discovery (U10 Chunk 3): resolve a package name to its source
//! directory by finding the unique `.jet` file that declares `module <name>`.

use super::Helpers::strip_line_comments;
use crate::Syntax;
use std::path::{Path, PathBuf};

// ── package discovery (U10 Chunk 3) ─────────────────────────────────────────

/// Why discovering a package's module failed (U10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryError {
    /// No `.jet` file in the source tree declares `module <name>`.
    NotFound { name: String },
    /// Multiple `.jet` files declare `module <name>`.
    Ambiguous { name: String, paths: Vec<PathBuf> },
}

/// Resolve package `name` to its source directory by finding the unique `.jet`
/// file under `root` that declares `module <name> { … }` at the top level.
///
/// Returns the **parent directory** of that file (the package's source tree).
/// Skips `.jet/`, hidden paths (starting with `.`), and `target/`. Scans
/// files in sorted order for determinism. Returns `DiscoveryError::NotFound`
/// if no file declares the module, `DiscoveryError::Ambiguous` if more than
/// one does.
pub fn discover_module_in(root: &Path, name: &str) -> Result<PathBuf, DiscoveryError> {
    let mut files = Vec::new();
    walk_jet_files(root, &mut files);
    let mut matches: Vec<PathBuf> = Vec::new();
    for file in &files {
        if let Ok(text) = std::fs::read_to_string(file) {
            if file_declares_module(&text, name) {
                matches.push(file.clone());
            }
        }
    }
    match matches.len() {
        0 => Err(DiscoveryError::NotFound {
            name: name.to_string(),
        }),
        1 => Ok(matches[0].parent().unwrap_or(root).to_path_buf()),
        _ => Err(DiscoveryError::Ambiguous {
            name: name.to_string(),
            paths: matches
                .into_iter()
                .map(|p| p.strip_prefix(root).unwrap_or(&p).to_path_buf())
                .collect(),
        }),
    }
}

/// Walk `dir` recursively, collecting sorted `.jet` file paths into `out`.
/// Skips: paths whose name starts with `.` (hidden/`.jet/` managed folder)
/// and `target/` directories.
fn walk_jet_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let fname = entry.file_name();
        let fname_str = fname.to_string_lossy();
        if fname_str.starts_with('.') || fname_str == "target" {
            continue;
        }
        if path.is_dir() {
            walk_jet_files(&path, out);
        } else if path.extension().and_then(|x| x.to_str()) == Some(Syntax::FILE_EXT)
            && fname_str != Syntax::PAYLOAD_FILE
        {
            out.push(path);
        }
    }
}

/// Return `true` if `text` (a `.jet` source file) declares `module <name> { … }`
/// at brace depth 0. Line comments are stripped before scanning.
pub(super) fn file_declares_module(text: &str, name: &str) -> bool {
    let text = strip_line_comments(text);
    let bytes = text.as_bytes();
    let kw = b"module";
    let name_bytes = name.as_bytes();
    let mut depth: i32 = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            b'm' if depth == 0 => {
                if bytes[i..].starts_with(kw) {
                    let after_kw = i + kw.len();
                    // `module` must be followed by whitespace.
                    if bytes.get(after_kw).map_or(false, |&c| {
                        c == b' ' || c == b'\t' || c == b'\n' || c == b'\r'
                    }) {
                        // Skip whitespace between keyword and name.
                        let mut j = after_kw;
                        while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                            j += 1;
                        }
                        if bytes[j..].starts_with(name_bytes) {
                            let after_name = j + name_bytes.len();
                            let next = bytes.get(after_name).copied();
                            // Word boundary: end of input or a non-ident char.
                            if next.map_or(true, |c| !c.is_ascii_alphanumeric() && c != b'_') {
                                return true;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}
