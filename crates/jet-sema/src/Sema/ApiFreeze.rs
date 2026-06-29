//! c129 (D-CAP4/D-CAP6/D-CAP8 — ratified): freeze the resolved public-capability
//! signature of a `library { api: stable | explicit }` target into durable
//! interface metadata.
//!
//! D-CAP8 already resolves every unmarked `Infer` parameter to a concrete
//! capability (`Read`/`Write`/`Move`/`Share`) from body usage, in memory, during
//! sema. c129 *persists* that resolved signature so it survives across builds and
//! a later read → `~`/`^`/`&` drift can be caught as a breaking change (E0912),
//! rather than silently flipping the public contract.
//!
//! Format (std-only, lockfile-style — no serde, I6):
//!
//! ```text
//! api_version = 1
//! package = mathkit
//! published_version = 1.2.0
//! fn scale(v: ~Vec3, factor: Float)
//! fn length(v: Vec3) -> Float
//! ```
//!
//! Each `fn` line is the canonical capability signature: every public function's
//! parameter list carries the frozen D-CAP7 sigil (`~`/`^`/`&`/`*`; plain read
//! emits none) plus the return type. The struct/enum/trait surface is diffed
//! separately by the SemVer API check (`Publish::diff_public_api`); this snapshot
//! is the *capability* contract specifically.
//!
//! Lives at `.jet/cache/api/<package>.api` (committed, durable contract — the same
//! discipline as the D-MIGRATE1 `#PublishedSchema` snapshot).

use crate::Syntax;
use crate::AST::{Func, Item};
use std::path::{Path, PathBuf};

pub const API_SNAPSHOT_VERSION: u32 = 1;

/// One frozen public function in a package's capability API.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FrozenFn {
    pub name: String,
    /// The canonical capability signature, e.g. `fn scale(v: ~Vec3, factor: Float)`.
    /// Carries the resolved D-CAP7 sigils that the caller must honour.
    pub signature: String,
}

/// The frozen public-capability surface of one package's library target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiSnapshot {
    pub api_version: u32,
    pub package: String,
    pub published_version: String,
    /// Public functions, sorted by name for a stable diff.
    pub funcs: Vec<FrozenFn>,
}

impl ApiSnapshot {
    /// Serialise to the lockfile-style text format.
    pub fn write(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("api_version = {}\n", self.api_version));
        out.push_str(&format!("package = {}\n", self.package));
        out.push_str(&format!("published_version = {}\n", self.published_version));
        for f in &self.funcs {
            out.push_str(&f.signature);
            out.push('\n');
        }
        out
    }

    /// Parse from the lockfile-style text format.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let mut api_version: Option<u32> = None;
        let mut package: Option<String> = None;
        let mut published_version: Option<String> = None;
        let mut funcs = Vec::new();

        for line in raw.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(rest) = line.strip_prefix("api_version = ") {
                api_version = Some(
                    rest.parse()
                        .map_err(|_| format!("invalid api_version: {}", rest))?,
                );
            } else if let Some(rest) = line.strip_prefix("package = ") {
                package = Some(rest.to_string());
            } else if let Some(rest) = line.strip_prefix("published_version = ") {
                published_version = Some(rest.to_string());
            } else if line.starts_with("fn ") {
                let name =
                    fn_name_of(line).ok_or_else(|| format!("malformed fn line: {}", line))?;
                funcs.push(FrozenFn {
                    name,
                    signature: line.to_string(),
                });
            } else {
                return Err(format!("unknown line in api snapshot: {}", line));
            }
        }

        Ok(ApiSnapshot {
            api_version: api_version.ok_or("missing api_version")?,
            package: package.ok_or("missing package")?,
            published_version: published_version.ok_or("missing published_version")?,
            funcs,
        })
    }

    /// The concatenated, canonical text of every frozen `fn` signature, sorted by
    /// name. Folded into the package pin/hash (c129) so a capability change shifts
    /// the lock fingerprint. Excludes the version/package header so the hash tracks
    /// the *shape* of the contract, not the release number.
    pub fn capability_digest(&self) -> String {
        let mut sigs: Vec<&str> = self.funcs.iter().map(|f| f.signature.as_str()).collect();
        sigs.sort_unstable();
        sigs.join("\n")
    }
}

/// The public function name in a `fn name(...) ...` line, or `None`.
fn fn_name_of(line: &str) -> Option<String> {
    let rest = line.strip_prefix("fn ")?;
    let end = rest.find('(').unwrap_or(rest.len());
    let name = rest[..end].trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// The canonical capability signature of a public function — its parameter list
/// with frozen D-CAP7 sigils and the return type. Shared with the SemVer
/// `Publish::API` extractor (same surface; this one is exposed for the freeze).
pub fn fn_signature(f: &Func) -> String {
    let params: Vec<String> = f
        .params
        .iter()
        .map(|p| format!("{}: {}{}", p.name, p.convention.sigil(), p.ty.name()))
        .collect();
    let ret = match &f.return_type {
        Some(t) => format!(" -> {}", t.name()),
        None => String::new(),
    };
    format!("fn {}({}){}", f.name, params.join(", "), ret)
}

/// Build a frozen-API snapshot from a list of items (a module's items, post
/// capability resolution). Records every `pub fn` reachable on the public
/// surface — top-level, and inside a `pub module { … }` block (the common
/// library layout `module foo { pub fn … }`), recursively.
pub fn snapshot_from_items(items: &[Item], package: &str, version: &str) -> ApiSnapshot {
    let mut funcs = Vec::new();
    collect_pub_fns(items, &mut funcs);
    funcs.sort();
    ApiSnapshot {
        api_version: API_SNAPSHOT_VERSION,
        package: package.to_string(),
        published_version: version.to_string(),
        funcs,
    }
}

/// Recursively collect `pub fn`s from `items`, descending into inline
/// `module { … }` bodies. The package's own module block carries the library's
/// surface (`module foo { pub fn … }`) and need not itself be marked `pub`; it is
/// the `pub` on the *function* that puts it on the contract.
fn collect_pub_fns(items: &[Item], out: &mut Vec<FrozenFn>) {
    for item in items {
        match item {
            Item::Func(f) if f.is_pub && !f.is_package_pub => out.push(FrozenFn {
                name: f.name.clone(),
                signature: fn_signature(f),
            }),
            Item::CodeModule(m) => {
                if let Some(body) = &m.body {
                    collect_pub_fns(body, out);
                }
            }
            _ => {}
        }
    }
}

/// The API cache directory for a project (`<root>/.jet/cache/api/`), honouring the
/// `JET_API_CACHE_DIR` test override (mirrors the schema cache override).
pub fn api_cache_dir(project_root: &Path) -> PathBuf {
    if let Ok(override_dir) = std::env::var("JET_API_CACHE_DIR") {
        PathBuf::from(override_dir)
    } else {
        project_root
            .join(Syntax::SOURCE_ROOT_DIR)
            .join(Syntax::API_CACHE_SUBDIR)
    }
}

/// Load a package's frozen-API snapshot from disk, or `None` if no prior freeze.
pub fn load_snapshot(project_root: &Path, package: &str) -> Option<ApiSnapshot> {
    let path = api_cache_dir(project_root).join(format!("{}.api", package));
    let raw = std::fs::read_to_string(&path).ok()?;
    ApiSnapshot::parse(&raw).ok()
}

/// Write a snapshot to disk under `<project_root>/.jet/cache/api/`.
pub fn save_snapshot(project_root: &Path, snap: &ApiSnapshot) -> Result<(), String> {
    let dir = api_cache_dir(project_root);
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not create api cache dir: {}", e))?;
    let path = dir.join(format!("{}.api", snap.package));
    std::fs::write(&path, snap.write()).map_err(|e| format!("could not write api snapshot: {}", e))
}

/// Load every frozen-API snapshot in the project's api cache. Used to fold the
/// capability contract into the package pin/hash (c129).
pub fn load_all_snapshots(project_root: &Path) -> Vec<ApiSnapshot> {
    let dir = api_cache_dir(project_root);
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("api") {
                continue;
            }
            if let Ok(raw) = std::fs::read_to_string(&path) {
                if let Ok(snap) = ApiSnapshot::parse(&raw) {
                    out.push(snap);
                }
            }
        }
    }
    out.sort_by(|a, b| a.package.cmp(&b.package));
    out
}

/// The combined capability digest of every frozen API in a project, sorted by
/// package then signature. Folded into the package fingerprint (`Lock`) so a
/// public capability change (read → `~`/`^`/`&`) shifts the lock hash even when
/// the source tree hash otherwise matches. Empty when nothing is frozen.
pub fn project_capability_digest(project_root: &Path) -> String {
    let snaps = load_all_snapshots(project_root);
    let mut parts = Vec::new();
    for s in &snaps {
        parts.push(format!("{}\n{}", s.package, s.capability_digest()));
    }
    parts.join("\n--\n")
}

// ──────────────────────────────────────────────
// Unit tests
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Diagnostics::Span;
    use crate::AST::{AccessConvention, Func, Param, Type};

    fn zero() -> Span {
        Span::new(0, 0)
    }

    fn param(name: &str, conv: AccessConvention, ty: Type) -> Param {
        Param {
            convention: conv,
            name: name.to_string(),
            name_span: zero(),
            ty,
            ty_span: zero(),
            default: None,
        }
    }

    fn func(name: &str, is_pub: bool, params: Vec<Param>, ret: Option<Type>) -> Func {
        Func {
            is_pub,
            is_package_pub: false,
            name: name.to_string(),
            name_span: zero(),
            type_params: vec![],
            params,
            return_type: ret,
            is_view_return: false,
            is_unsafe: false,
            is_pure: false,
            is_sanitizer: false,
            declared_effects: None,
            effect_via: None,
            state_requires: None,
            state_transition: None,
            body: vec![],
        }
    }

    #[test]
    fn sig_carries_resolved_sigil() {
        let f = func(
            "scale",
            true,
            vec![
                param("v", AccessConvention::Write, Type::Named("Vec3".into())),
                param("factor", AccessConvention::Read, Type::Float),
            ],
            None,
        );
        assert_eq!(fn_signature(&f), "fn scale(v: ~Vec3, factor: Float)");
    }

    #[test]
    fn round_trip() {
        let items = vec![
            Item::Func(func(
                "length",
                true,
                vec![param(
                    "v",
                    AccessConvention::Read,
                    Type::Named("Vec3".into()),
                )],
                Some(Type::Float),
            )),
            Item::Func(func(
                "scale",
                true,
                vec![param(
                    "v",
                    AccessConvention::Write,
                    Type::Named("Vec3".into()),
                )],
                None,
            )),
            // private fn — excluded
            Item::Func(func("helper", false, vec![], None)),
        ];
        let snap = snapshot_from_items(&items, "mathkit", "1.0.0");
        assert_eq!(snap.funcs.len(), 2);
        let text = snap.write();
        let parsed = ApiSnapshot::parse(&text).expect("round trips");
        assert_eq!(parsed, snap);
        // Sorted by name: length before scale.
        assert_eq!(parsed.funcs[0].name, "length");
        assert_eq!(parsed.funcs[1].name, "scale");
    }

    #[test]
    fn digest_ignores_version() {
        let mk = |ver: &str| {
            let items = vec![Item::Func(func(
                "scale",
                true,
                vec![param(
                    "v",
                    AccessConvention::Write,
                    Type::Named("Vec3".into()),
                )],
                None,
            ))];
            snapshot_from_items(&items, "mathkit", ver).capability_digest()
        };
        assert_eq!(mk("1.0.0"), mk("2.5.9"), "digest tracks shape, not version");
    }

    #[test]
    fn digest_shifts_on_capability_change() {
        let read = snapshot_from_items(
            &[Item::Func(func(
                "scale",
                true,
                vec![param(
                    "v",
                    AccessConvention::Read,
                    Type::Named("Vec3".into()),
                )],
                None,
            ))],
            "mathkit",
            "1.0.0",
        );
        let write = snapshot_from_items(
            &[Item::Func(func(
                "scale",
                true,
                vec![param(
                    "v",
                    AccessConvention::Write,
                    Type::Named("Vec3".into()),
                )],
                None,
            ))],
            "mathkit",
            "1.0.0",
        );
        assert_ne!(read.capability_digest(), write.capability_digest());
    }

    #[test]
    fn parse_known_text() {
        let text = "api_version = 1\npackage = mk\npublished_version = 0.1.0\nfn f(x: ~Int)\n";
        let snap = ApiSnapshot::parse(text).unwrap();
        assert_eq!(snap.package, "mk");
        assert_eq!(snap.funcs.len(), 1);
        assert_eq!(snap.funcs[0].name, "f");
        assert_eq!(snap.funcs[0].signature, "fn f(x: ~Int)");
    }
}
