//! Package discovery (U10 Chunk 3): resolve a package name to its source
//! directory by finding the unique `.jet` file that declares `module <name>`.

use super::strip_comments as strip_line_comments;
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
    // This is an explicit named lookup, so D-SHAPE-MODULEINTERNAL1=A does not
    // filter `_name`; only callers enumerating modules automatically apply the
    // ModuleDecl discovery predicate.
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
            && fname_str != Syntax::PACKAGE_FILE
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

// ──────────────────────────────────────────────
// Tests — D-JPK-FILENAME2=B (A2) acceptance: `pkg.jet` is the only reserved
// filename; every role module (`workspace`, dev/system role names) is
// discovered by declaration, regardless of which `.jet` file it lives in.
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!(
            "discovery-a2-{tag}-{nanos}-{:?}",
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn module_found_regardless_of_filename() {
        let dir = tempdir("arbitrary-filename");
        // Nothing here is named after the module it declares.
        std::fs::write(
            dir.join("whatever.jet"),
            "module workspace { members: [] }\n",
        )
        .unwrap();
        let found = discover_module_in(&dir, "workspace").unwrap();
        assert_eq!(found, dir);
    }

    #[test]
    fn several_role_modules_discovered_across_arbitrary_files() {
        let dir = tempdir("several-roles");
        std::fs::write(dir.join("a.jet"), "module workspace { members: [] }\n").unwrap();
        std::fs::write(dir.join("b.jet"), "module dev { }\n").unwrap();
        std::fs::write(dir.join("c.jet"), "module laptop { }\n").unwrap();
        assert!(discover_module_in(&dir, "workspace").is_ok());
        assert!(discover_module_in(&dir, "dev").is_ok());
        assert!(discover_module_in(&dir, "laptop").is_ok());
    }

    #[test]
    fn explicit_internal_module_lookup_remains_allowed() {
        let dir = tempdir("explicit-internal");
        std::fs::write(dir.join("arbitrary.jet"), "module _bench { }\n").unwrap();
        assert_eq!(discover_module_in(&dir, "_bench").unwrap(), dir);
    }

    /// Only `pkg.jet` is reserved: discovery skips it even if it happens to
    /// contain module-shaped text, so a module can never be "found" via the
    /// manifest file itself — only via the tree it manages.
    #[test]
    fn pkg_jet_is_excluded_from_discovery() {
        let dir = tempdir("pkg-jet-excluded");
        std::fs::write(
            dir.join(Syntax::PAYLOAD_FILE),
            "module bogus { }\npayload: { name: \"x\", version: \"0.1.0\" }\n",
        )
        .unwrap();
        assert_eq!(
            discover_module_in(&dir, "bogus"),
            Err(DiscoveryError::NotFound {
                name: "bogus".to_string()
            })
        );
    }

    #[test]
    fn ambiguous_when_two_files_declare_the_same_module() {
        let dir = tempdir("ambiguous");
        std::fs::write(dir.join("one.jet"), "module dup { }\n").unwrap();
        std::fs::write(dir.join("two.jet"), "module dup { }\n").unwrap();
        assert!(matches!(
            discover_module_in(&dir, "dup"),
            Err(DiscoveryError::Ambiguous { .. })
        ));
    }
}
