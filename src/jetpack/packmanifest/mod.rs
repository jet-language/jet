//! The Jet-syntax `pkg.jet` package manifest (U1/U10 — Cargo.toml analog).
//!
//! Unified-ecosystem §2.1: the package tier. One language everywhere — the
//! manifest is written in Jet syntax, not TOML. It holds payload identity, Jet
//! library dependencies, and the optional list of public modules the payload
//! exports:
//!
//! ```jet
//! payload: {
//!     name:    "wordstats",
//!     version: "0.1.0",
//!     edition: "2026",
//!     license: "MIT OR Apache-2.0",
//! }
//! packages: {
//!     web: library,
//!     cli: executable,
//! }
//! deps: {
//!     textkit:  "1.2.0",
//!     helpers:  path@../helpers,
//!     parsekit: { git: "https://github.com/acme/parsekit", tag: "v0.4.1" },
//! }
//! ```
//!
//! This module is the structural parser for that shape (U1). It is std-only
//! (I6). Dependency values are a registry version string (`"1.2.0"`), a
//! `provider@target` source ref (`path@../local`, `github@owner/repo/rev`,
//! classified through `refspec::classify_provider_ref`, U6), or an inline
//! git struct (`{ git: "<url>", tag/branch/rev: "<value>" }`, D-JPK23 —
//! generalizes to any git remote, not just GitHub). `to_manifest` converts a
//! parsed `PackManifest` into the compiler's `manifest::Manifest`, the type
//! `loader.rs`/`fetch.rs`/`lock.rs` operate on.

mod convert;
mod discovery;
mod edit;
mod helpers;
mod parse_blocks;

pub use convert::{new_template, to_manifest};
pub use discovery::{discover_module_in, DiscoveryError};
pub use edit::{add_dep, remove_dep};

use super::refspec::{RefError, Source};
use crate::syntax;
use helpers::block_body;
use parse_blocks::{parse_deps, parse_package, parse_packages};

/// Payload identity (the `payload: { … }` block, U10 — was `package:`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PackageMeta {
    pub name: String,
    pub version: String,
    pub edition: Option<String>,
    pub license: Option<String>,
    pub description: Option<String>,
    pub repository: Option<String>,
    /// Toolchain constraint, e.g. `jet: ">=1.0.0"` (E1208).
    pub jet_constraint: Option<String>,
}

/// A package's kind (U10): `library` is imported for code; `executable` installs
/// a binary on PATH (the devshell case).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageKind {
    Library,
    Executable,
}

/// One entry in the `packages: { … }` block (U10). `kind` is `None` when the
/// manifest omits it (D-ILE1) — the kind is then inferred from the module's
/// `fn main` at realize time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageEntry {
    pub name: String,
    pub kind: Option<PackageKind>,
}

/// Where a dependency resolves from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepSource {
    /// A registry version string, e.g. `"1.2.0"`.
    Version(String),
    /// A `provider@target` source ref, e.g. `path@../helpers`.
    Provider { provider: Source, target: String },
    /// An inline git dependency (D-JPK23): any remote, with an explicit
    /// selector — `{ git: "<url>", tag/branch/rev: "<value>" }`.
    Git {
        url: String,
        selector: crate::manifest::GitSelector,
    },
}

/// One `name: value` entry in the `deps: { … }` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dep {
    pub name: String,
    pub source: DepSource,
}

/// A parsed `pkg.jet` package manifest.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PackManifest {
    pub package: PackageMeta,
    /// Dependencies, in declaration order.
    pub deps: Vec<Dep>,
    /// Packages this payload declares (the `packages: { name: kind }` block, U10).
    pub packages: Vec<PackageEntry>,
}

/// Why a `pkg.jet` package manifest could not be parsed. These are internal
/// (typed) errors for now; they become I4 diagnostics when the parser is wired
/// into the loader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    /// No `payload: { … }` block at all.
    MissingPayload,
    /// `payload` is missing a required `name` or `version`.
    MissingField(&'static str),
    /// A `deps` value is neither a quoted version nor a `provider@target` ref.
    BadDepValue { name: String, value: String },
    /// A `provider@target` dep ref failed to classify (U6).
    BadDepRef { name: String, err: RefError },
    /// An inline git dep (D-JPK23) is missing `git`, or doesn't have exactly
    /// one of `tag`/`branch`/`rev`.
    BadGitDep { name: String, reason: &'static str },
    /// A `packages:` entry's kind is not `library` or `executable` (E1210).
    BadPackageKind { name: String, value: String },
    /// A `packages:` block-form entry (`{ kind: … }`) is missing the `kind` field (E1211).
    MalformedPackageEntry { name: String },
    /// A reserved top-level key (`dev_deps`/`patch`/`workspace`) was used
    /// non-empty (carries forward jet.toml's reserved-section guard, S52/E1209).
    ReservedSection(&'static str),
}

/// Top-level keys reserved for a future Jet feature; using them non-empty
/// today is an error (carries forward jet.toml's `dev-dependencies`/`patch`/
/// `workspace` guard, S52/E1209).
const RESERVED_SECTIONS: &[&str] = &["dev_deps", "patch", "workspace"];

impl PackManifest {
    /// The path to the package manifest in a project dir.
    pub fn path_in(dir: &std::path::Path) -> std::path::PathBuf {
        dir.join(syntax::PAYLOAD_FILE)
    }

    /// Load and parse the package manifest in `dir`, if present.
    pub fn load(dir: &std::path::Path) -> Option<Result<PackManifest, ManifestError>> {
        let text = std::fs::read_to_string(Self::path_in(dir)).ok()?;
        Some(parse(&text))
    }

    /// The declared kind of package `name`. Returns `None` when the package is
    /// not listed *or* lists no `kind` (D-ILE1) — both leave the kind to be
    /// inferred from the source at realize time.
    pub fn package_kind(&self, name: &str) -> Option<PackageKind> {
        self.packages
            .iter()
            .find(|p| p.name == name)
            .and_then(|p| p.kind.clone())
    }
}

/// Parse a `pkg.jet` package manifest from its text (U1/U10).
pub fn parse(text: &str) -> Result<PackManifest, ManifestError> {
    let text = helpers::strip_line_comments(text);

    let package = match block_body(&text, syntax::MANIFEST_BLOCK_PAYLOAD, '{', '}') {
        Some(body) => parse_package(&body)?,
        None => return Err(ManifestError::MissingPayload),
    };

    let deps = match block_body(&text, "deps", '{', '}') {
        Some(body) => parse_deps(&body)?,
        None => Vec::new(),
    };

    let packages = match block_body(&text, syntax::MANIFEST_BLOCK_PACKAGES, '{', '}') {
        Some(body) => parse_packages(&body)?,
        None => Vec::new(),
    };

    for &section in RESERVED_SECTIONS {
        if let Some(body) = block_body(&text, section, '{', '}') {
            if !body.trim().is_empty() {
                return Err(ManifestError::ReservedSection(section));
            }
        }
    }

    Ok(PackManifest {
        package,
        deps,
        packages,
    })
}

#[cfg(test)]
mod tests {
    use super::discovery::file_declares_module;
    use super::*;
    use std::path::PathBuf;

    const FULL: &str = r#"
payload: {
    name:    "wordstats",
    version: "0.1.0",
    edition: "2026",
    license: "MIT OR Apache-2.0",
}
packages: {
    web: library,
    cli: executable,
}
deps: {
    textkit: "1.2.0",
    helpers: path@../helpers,
}
"#;

    #[test]
    fn parses_package_block() {
        let m = parse(FULL).unwrap();
        assert_eq!(m.package.name, "wordstats");
        assert_eq!(m.package.version, "0.1.0");
        assert_eq!(m.package.edition.as_deref(), Some("2026"));
        assert_eq!(m.package.license.as_deref(), Some("MIT OR Apache-2.0"));
    }

    #[test]
    fn parses_deps_version_and_provider_ref() {
        let m = parse(FULL).unwrap();
        assert_eq!(m.deps.len(), 2);
        assert_eq!(m.deps[0].name, "textkit");
        assert_eq!(m.deps[0].source, DepSource::Version("1.2.0".into()));
        assert_eq!(m.deps[1].name, "helpers");
        assert_eq!(
            m.deps[1].source,
            DepSource::Provider {
                provider: Source::Path,
                target: "../helpers".into(),
            }
        );
    }

    #[test]
    fn parses_packages_block() {
        let m = parse(FULL).unwrap();
        assert_eq!(m.packages.len(), 2);
        assert_eq!(m.packages[0].name, "web");
        assert_eq!(m.packages[0].kind, Some(PackageKind::Library));
        assert_eq!(m.packages[1].name, "cli");
        assert_eq!(m.packages[1].kind, Some(PackageKind::Executable));
    }

    #[test]
    fn package_kind_is_optional_and_inferred() {
        // D-ILE1: a bare `name` omits `kind` (inferred from the module's
        // `fn main` at realize time); an explicit kind still wins.
        let src =
            "payload: { name: \"x\", version: \"1\" }\npackages: { deploy, web: library }";
        let m = parse(src).unwrap();
        assert_eq!(m.packages.len(), 2);
        assert_eq!(m.packages[0].name, "deploy");
        assert_eq!(m.packages[0].kind, None);
        assert_eq!(m.packages[1].name, "web");
        assert_eq!(m.packages[1].kind, Some(PackageKind::Library));
        // package_kind collapses "not listed" and "listed without kind" to None
        // so the provider infers in both cases.
        assert_eq!(m.package_kind("deploy"), None);
        assert_eq!(m.package_kind("web"), Some(PackageKind::Library));
        assert_eq!(m.package_kind("absent"), None);
    }

    #[test]
    fn packages_block_form() {
        let src = r#"
payload: { name: "x", version: "1" }
packages: {
    server: { kind: executable },
    utils:  { kind: library },
}
"#;
        let m = parse(src).unwrap();
        assert_eq!(m.packages.len(), 2);
        assert_eq!(m.packages[0].kind, Some(PackageKind::Executable));
        assert_eq!(m.packages[1].kind, Some(PackageKind::Library));
    }

    #[test]
    fn deps_and_packages_are_optional() {
        let m = parse("payload: { name: \"x\", version: \"0.0.1\" }").unwrap();
        assert!(m.deps.is_empty());
        assert!(m.packages.is_empty());
        assert_eq!(m.package.name, "x");
    }

    #[test]
    fn bad_package_kind_bare_errors() {
        let src = "payload: { name: \"x\", version: \"1\" }\npackages: { web: plugin }";
        let err = parse(src).unwrap_err();
        assert!(
            matches!(err, ManifestError::BadPackageKind { ref name, ref value }
                if name == "web" && value == "plugin"),
            "{err:?}"
        );
    }

    #[test]
    fn bad_package_kind_in_block_errors() {
        let src =
            "payload: { name: \"x\", version: \"1\" }\npackages: { web: { kind: plugin } }";
        let err = parse(src).unwrap_err();
        assert!(
            matches!(err, ManifestError::BadPackageKind { ref name, ref value }
                if name == "web" && value == "plugin"),
            "{err:?}"
        );
    }

    #[test]
    fn malformed_package_entry_missing_kind_errors() {
        let src =
            "payload: { name: \"x\", version: \"1\" }\npackages: { web: { desc: \"no kind\" } }";
        let err = parse(src).unwrap_err();
        assert!(
            matches!(err, ManifestError::MalformedPackageEntry { ref name } if name == "web"),
            "{err:?}"
        );
    }

    #[test]
    fn github_provider_dep() {
        let src = r#"
payload: { name: "p", version: "0.1.0" }
deps: { up: github@NixOS/nixpkgs/nixos-24.05 }
"#;
        let m = parse(src).unwrap();
        assert_eq!(
            m.deps[0].source,
            DepSource::Provider {
                provider: Source::Github,
                target: "NixOS/nixpkgs/nixos-24.05".into(),
            }
        );
    }

    #[test]
    fn missing_package_block_errors() {
        assert_eq!(parse("deps: {}"), Err(ManifestError::MissingPayload));
    }

    #[test]
    fn missing_required_field_errors() {
        assert_eq!(
            parse("payload: { name: \"x\" }"),
            Err(ManifestError::MissingField("version"))
        );
        assert_eq!(
            parse("payload: { version: \"0.1.0\" }"),
            Err(ManifestError::MissingField("name"))
        );
    }

    #[test]
    fn bad_dep_value_errors() {
        // A bare token with no `@` and no quotes is not a valid dep value.
        let err = parse("payload: { name: \"x\", version: \"1\" }\ndeps: { y: notaref }")
            .unwrap_err();
        assert!(matches!(err, ManifestError::BadDepValue { .. }));
    }

    #[test]
    fn comments_are_ignored() {
        let src = r#"
// a leading comment
payload: {
    name: "x",      // inline comment
    version: "0.1.0",
}
"#;
        let m = parse(src).unwrap();
        assert_eq!(m.package.name, "x");
        assert_eq!(m.package.version, "0.1.0");
    }

    #[test]
    fn deps_prefix_does_not_match_inside_word() {
        // `dependencies:` must not be picked up as the `deps:` block.
        let src = r#"
payload: { name: "x", version: "1" }
dependencies: { should_be_ignored: "9.9.9" }
"#;
        let m = parse(src).unwrap();
        assert!(m.deps.is_empty(), "deps: {:?}", m.deps);
    }

    // ── inline git deps (D-JPK23) ──

    #[test]
    fn git_dep_tag() {
        let src = r#"
payload: { name: "p", version: "0.1.0" }
deps: { parsekit: { git: "https://github.com/acme/parsekit", tag: "v0.4.1" } }
"#;
        let m = parse(src).unwrap();
        assert_eq!(
            m.deps[0].source,
            DepSource::Git {
                url: "https://github.com/acme/parsekit".into(),
                selector: crate::manifest::GitSelector::Tag("v0.4.1".into()),
            }
        );
    }

    #[test]
    fn git_dep_branch() {
        let src = r#"
payload: { name: "p", version: "0.1.0" }
deps: { nightly: { git: "https://github.com/acme/nightly", branch: "main" } }
"#;
        let m = parse(src).unwrap();
        assert_eq!(
            m.deps[0].source,
            DepSource::Git {
                url: "https://github.com/acme/nightly".into(),
                selector: crate::manifest::GitSelector::Branch("main".into()),
            }
        );
    }

    #[test]
    fn git_dep_rev_and_non_github_remote() {
        let src = r#"
payload: { name: "p", version: "0.1.0" }
deps: { selfhost: { git: "https://git.example.com/acme/thing", rev: "abc123" } }
"#;
        let m = parse(src).unwrap();
        assert_eq!(
            m.deps[0].source,
            DepSource::Git {
                url: "https://git.example.com/acme/thing".into(),
                selector: crate::manifest::GitSelector::Rev("abc123".into()),
            }
        );
    }

    #[test]
    fn git_dep_missing_git_field_errors() {
        let src = r#"
payload: { name: "p", version: "0.1.0" }
deps: { bad: { tag: "v1.0.0" } }
"#;
        let err = parse(src).unwrap_err();
        assert!(matches!(err, ManifestError::BadGitDep { name, .. } if name == "bad"));
    }

    #[test]
    fn git_dep_missing_selector_errors() {
        let src = r#"
payload: { name: "p", version: "0.1.0" }
deps: { bad: { git: "https://example.com/x" } }
"#;
        let err = parse(src).unwrap_err();
        assert!(matches!(err, ManifestError::BadGitDep { name, .. } if name == "bad"));
    }

    #[test]
    fn git_dep_two_selectors_errors() {
        let src = r#"
payload: { name: "p", version: "0.1.0" }
deps: { bad: { git: "https://example.com/x", tag: "v1", branch: "main" } }
"#;
        let err = parse(src).unwrap_err();
        assert!(matches!(err, ManifestError::BadGitDep { name, .. } if name == "bad"));
    }

    #[test]
    fn mixed_dep_kinds_in_one_block() {
        let src = r#"
payload: { name: "p", version: "0.1.0" }
deps: {
    textkit:  "1.2.0",
    helpers:  path@../helpers,
    parsekit: { git: "https://github.com/acme/parsekit", tag: "v0.4.1" },
}
"#;
        let m = parse(src).unwrap();
        assert_eq!(m.deps.len(), 3);
        assert_eq!(m.deps[0].source, DepSource::Version("1.2.0".into()));
        assert!(matches!(m.deps[1].source, DepSource::Provider { .. }));
        assert!(matches!(m.deps[2].source, DepSource::Git { .. }));
    }

    #[test]
    fn reserved_section_nonempty_errors() {
        let src = r#"
payload: { name: "x", version: "1" }
workspace: { members: "foo" }
"#;
        let err = parse(src).unwrap_err();
        assert_eq!(err, ManifestError::ReservedSection("workspace"));
    }

    #[test]
    fn reserved_section_empty_is_fine() {
        let src = r#"
payload: { name: "x", version: "1" }
workspace: {}
"#;
        assert!(parse(src).is_ok());
    }

    // ── to_manifest conversion ──

    #[test]
    fn to_manifest_converts_version_and_path_deps() {
        let m = parse(FULL).unwrap();
        let mf = to_manifest(&m, "raw text").unwrap();
        assert_eq!(mf.package.name, "wordstats");
        assert_eq!(
            mf.dependencies.get("textkit"),
            Some(&crate::manifest::DepSpec::Registry("1.2.0".into()))
        );
        assert_eq!(
            mf.dependencies.get("helpers"),
            Some(&crate::manifest::DepSpec::Path {
                path: "../helpers".into()
            })
        );
    }

    #[test]
    fn to_manifest_converts_inline_git_dep() {
        let src = r#"
payload: { name: "p", version: "0.1.0" }
deps: { parsekit: { git: "https://github.com/acme/parsekit", tag: "v0.4.1" } }
"#;
        let m = parse(src).unwrap();
        let mf = to_manifest(&m, src).unwrap();
        assert_eq!(
            mf.dependencies.get("parsekit"),
            Some(&crate::manifest::DepSpec::Git {
                url: "https://github.com/acme/parsekit".into(),
                selector: crate::manifest::GitSelector::Tag("v0.4.1".into()),
            })
        );
    }

    #[test]
    fn to_manifest_converts_github_provider_ref_as_pinned_rev() {
        let src = r#"
payload: { name: "p", version: "0.1.0" }
deps: { up: github@NixOS/nixpkgs/nixos-24.05 }
"#;
        let m = parse(src).unwrap();
        let mf = to_manifest(&m, src).unwrap();
        assert_eq!(
            mf.dependencies.get("up"),
            Some(&crate::manifest::DepSpec::Git {
                url: "https://github.com/NixOS/nixpkgs".into(),
                selector: crate::manifest::GitSelector::Rev("nixos-24.05".into()),
            })
        );
    }

    #[test]
    fn to_manifest_rejects_nixpkgs_provider_dep() {
        let src = r#"
payload: { name: "p", version: "0.1.0" }
deps: { x: nixpkgs@fastfetch }
"#;
        let m = parse(src).unwrap();
        let err = to_manifest(&m, src).unwrap_err();
        assert_eq!(err.code, "E1206");
    }

    #[test]
    fn to_manifest_carries_jet_constraint() {
        let src = r#"
payload: { name: "p", version: "0.1.0", jet: ">=1.0.0" }
"#;
        let m = parse(src).unwrap();
        let mf = to_manifest(&m, src).unwrap();
        assert_eq!(mf.package.jet_constraint.as_deref(), Some(">=1.0.0"));
    }

    // ── template + comment-preserving edits ──

    #[test]
    fn template_plain_parses() {
        let raw = new_template("myapp", false);
        let m = parse(&raw).expect("plain template should parse");
        assert_eq!(m.package.name, "myapp");
        assert_eq!(m.package.version, "0.1.0");
        assert!(m.package.jet_constraint.is_some());
    }

    #[test]
    fn template_annotated_has_dep_comment_and_parses() {
        let raw = new_template("myapp", true);
        assert!(raw.contains("// Jet package dependencies:"), "{}", raw);
        let m = parse(&raw).expect("annotated template should parse");
        assert_eq!(m.package.name, "myapp");
    }

    #[test]
    fn add_dep_creates_block_when_absent() {
        let raw = new_template("myapp", true); // deps: block is commented out
        let updated = add_dep(
            &raw,
            "helpers",
            &crate::manifest::DepSpec::Path {
                path: "../helpers".into(),
            },
        );
        let m = parse(&updated).unwrap();
        assert_eq!(
            m.deps.iter().find(|d| d.name == "helpers").map(|d| &d.source),
            Some(&DepSource::Provider {
                provider: Source::Path,
                target: "../helpers".into(),
            })
        );
    }

    #[test]
    fn add_dep_inserts_into_existing_block_and_replaces() {
        let raw = "payload: { name: \"x\", version: \"1\" }\n\ndeps: {\n    a: \"1.0.0\",\n}\n";
        let updated = add_dep(
            raw,
            "b",
            &crate::manifest::DepSpec::Path { path: "../b".into() },
        );
        let m = parse(&updated).unwrap();
        assert_eq!(m.deps.len(), 2);

        // Replacing an existing dep updates in place rather than duplicating.
        let updated2 = add_dep(
            &updated,
            "a",
            &crate::manifest::DepSpec::Registry("2.0.0".into()),
        );
        let m2 = parse(&updated2).unwrap();
        assert_eq!(m2.deps.len(), 2);
        assert_eq!(
            m2.deps.iter().find(|d| d.name == "a").map(|d| &d.source),
            Some(&DepSource::Version("2.0.0".into()))
        );
    }

    #[test]
    fn remove_dep_drops_only_named_entry() {
        let raw = "payload: { name: \"x\", version: \"1\" }\n\ndeps: {\n    a: \"1.0.0\",\n    b: \"2.0.0\",\n}\n";
        let updated = remove_dep(raw, "a");
        let m = parse(&updated).unwrap();
        assert_eq!(m.deps.len(), 1);
        assert_eq!(m.deps[0].name, "b");
    }

    // ── package discovery (U10 Chunk 3) ──

    fn scratch_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!("{tag}-{nanos}"));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn discovers_module_in_subdirectory() {
        let root = scratch_dir("disc-found");
        let sub = root.join("pkgs/hello");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("hello.jet"), "module hello {\n}\n").unwrap();
        let result = discover_module_in(&root, "hello").unwrap();
        assert_eq!(result, sub);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn discovers_module_at_root() {
        let root = scratch_dir("disc-root");
        std::fs::write(root.join("world.jet"), "module world { }\n").unwrap();
        let result = discover_module_in(&root, "world").unwrap();
        assert_eq!(result, root);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn discovery_not_found_returns_error() {
        let root = scratch_dir("disc-none");
        std::fs::write(root.join("other.jet"), "module other { }\n").unwrap();
        let err = discover_module_in(&root, "hello").unwrap_err();
        assert!(matches!(err, DiscoveryError::NotFound { ref name } if name == "hello"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn discovery_ambiguous_returns_error() {
        let root = scratch_dir("disc-ambig");
        std::fs::write(root.join("a.jet"), "module hello { }\n").unwrap();
        std::fs::write(root.join("b.jet"), "module hello { }\n").unwrap();
        let err = discover_module_in(&root, "hello").unwrap_err();
        assert!(matches!(err, DiscoveryError::Ambiguous { ref name, .. } if name == "hello"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn discovery_skips_payload_jet() {
        // pkg.jet should never be scanned for module declarations.
        let root = scratch_dir("disc-skip-payload");
        std::fs::write(
            root.join("pkg.jet"),
            "payload: { name: \"x\", version: \"1\" }\n// module hello { }\n",
        )
        .unwrap();
        let err = discover_module_in(&root, "hello").unwrap_err();
        assert!(matches!(err, DiscoveryError::NotFound { .. }));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn discovery_skips_hidden_dirs() {
        let root = scratch_dir("disc-skip-hidden");
        let hidden = root.join(".cache");
        std::fs::create_dir_all(&hidden).unwrap();
        std::fs::write(hidden.join("hello.jet"), "module hello { }\n").unwrap();
        let err = discover_module_in(&root, "hello").unwrap_err();
        assert!(matches!(err, DiscoveryError::NotFound { .. }));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn discovery_ignores_module_in_comments() {
        let root = scratch_dir("disc-comment");
        std::fs::write(root.join("x.jet"), "// module hello { }\nmodule other { }\n").unwrap();
        let err = discover_module_in(&root, "hello").unwrap_err();
        assert!(matches!(err, DiscoveryError::NotFound { .. }));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn discovery_ignores_nested_module() {
        // `module hello` inside another module (depth > 0) must not count.
        let root = scratch_dir("disc-nested");
        std::fs::write(
            root.join("outer.jet"),
            "module outer {\n    module hello { }\n}\n",
        )
        .unwrap();
        let err = discover_module_in(&root, "hello").unwrap_err();
        assert!(matches!(err, DiscoveryError::NotFound { .. }));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn file_declares_module_word_boundary() {
        // `module hellostuff` must not match `hello`.
        assert!(!file_declares_module("module hellostuff { }", "hello"));
        assert!(file_declares_module("module hello { }", "hello"));
        assert!(file_declares_module("module hello\n{}", "hello"));
    }
}
