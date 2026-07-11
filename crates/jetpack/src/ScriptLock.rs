//! U11 (D-JPK-SCRIPTDEP1=A): the `<script>.lock` sidecar `jet store lock <script>`
//! writes for a manifest-less script's inline `use pkg#version;` deps.
//!
//! Hand-written TOML-ish shape (no external crate — I6), the same style as
//! `Lock.rs`/`WorkspaceLock.rs`:
//!
//! ```toml
//! version = 1
//! script_hash = "sha256-…"
//!
//! [[dep]]
//! name = "textkit"
//! selector = "1.4"
//! resolved = "1.4.2"
//! content_hash = "sha256-…"
//! ```
//!
//! `script_hash` is the file-content hash U11 locks by (edit the script,
//! the lock goes stale); `content_hash` is each dep's realized-source hash
//! (the same `tree_hash` shape the project-level lock and hangar use).

use std::path::{Path, PathBuf};

pub const SCRIPT_LOCK_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedInlineDep {
    pub name: String,
    pub selector: String,
    pub resolved: String,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptLockFile {
    pub version: u32,
    pub script_hash: String,
    pub deps: Vec<LockedInlineDep>,
}

/// `<script>.lock` sits right next to the script (`stats.jet` →
/// `stats.jet.lock`) — never inside a `.jet/` folder, since a bare script may
/// not have one yet.
pub fn sidecar_path(script_path: &Path) -> PathBuf {
    let mut s = script_path.as_os_str().to_os_string();
    s.push(".lock");
    PathBuf::from(s)
}

pub fn write(script_path: &Path, lock: &ScriptLockFile) -> std::io::Result<()> {
    std::fs::write(sidecar_path(script_path), render(lock))
}

pub fn render(lock: &ScriptLockFile) -> String {
    let mut out = String::new();
    out.push_str(&format!("version = {}\n", lock.version));
    out.push_str(&format!("script_hash = \"{}\"\n", lock.script_hash));
    for dep in &lock.deps {
        out.push('\n');
        out.push_str("[[dep]]\n");
        out.push_str(&format!("name = \"{}\"\n", dep.name));
        out.push_str(&format!("selector = \"{}\"\n", dep.selector));
        out.push_str(&format!("resolved = \"{}\"\n", dep.resolved));
        out.push_str(&format!("content_hash = \"{}\"\n", dep.content_hash));
    }
    out
}

/// Load the sidecar next to `script_path`, if any.
pub fn load(script_path: &Path) -> Option<ScriptLockFile> {
    let raw = std::fs::read_to_string(sidecar_path(script_path)).ok()?;
    parse(&raw).ok()
}

pub fn parse(raw: &str) -> Result<ScriptLockFile, String> {
    let mut version = SCRIPT_LOCK_VERSION;
    let mut script_hash = String::new();
    let mut deps: Vec<LockedInlineDep> = Vec::new();
    let mut in_dep = false;
    let mut cur: Option<(String, String, String, String)> = None; // name, selector, resolved, content_hash

    fn flush(cur: &mut Option<(String, String, String, String)>, deps: &mut Vec<LockedInlineDep>) {
        if let Some((name, selector, resolved, content_hash)) = cur.take() {
            deps.push(LockedInlineDep {
                name,
                selector,
                resolved,
                content_hash,
            });
        }
    }

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[dep]]" {
            flush(&mut cur, &mut deps);
            cur = Some((String::new(), String::new(), String::new(), String::new()));
            in_dep = true;
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("malformed line in script lock: `{line}`"));
        };
        let key = key.trim();
        let value = value.trim().trim_matches('"').to_string();
        if in_dep {
            let Some(d) = cur.as_mut() else {
                return Err("dep field outside [[dep]]".to_string());
            };
            match key {
                "name" => d.0 = value,
                "selector" => d.1 = value,
                "resolved" => d.2 = value,
                "content_hash" => d.3 = value,
                other => return Err(format!("unknown dep field `{other}`")),
            }
        } else {
            match key {
                "version" => {
                    version = value.parse().map_err(|_| "bad version field".to_string())?
                }
                "script_hash" => script_hash = value,
                other => return Err(format!("unknown field `{other}`")),
            }
        }
    }
    flush(&mut cur, &mut deps);

    Ok(ScriptLockFile {
        version,
        script_hash,
        deps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips() {
        let lock = ScriptLockFile {
            version: 1,
            script_hash: "sha256-abc".to_string(),
            deps: vec![LockedInlineDep {
                name: "textkit".to_string(),
                selector: "1.4".to_string(),
                resolved: "1.4.2".to_string(),
                content_hash: "sha256-def".to_string(),
            }],
        };
        let rendered = render(&lock);
        let parsed = parse(&rendered).unwrap();
        assert_eq!(parsed, lock);
    }

    #[test]
    fn sidecar_path_appends_lock() {
        let p = sidecar_path(Path::new("stats.jet"));
        assert_eq!(p, PathBuf::from("stats.jet.lock"));
    }
}
