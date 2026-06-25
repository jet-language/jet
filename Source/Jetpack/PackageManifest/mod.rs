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
//! classified through `RefSpec::classify_provider_ref`, U6), or an inline
//! git struct (`{ git: "<url>", tag/branch/rev: "<value>" }`, D-JPK23 —
//! generalizes to any git remote, not just GitHub). `to_manifest` converts a
//! parsed `PackManifest` into the compiler's `Manifest::Manifest`, the type
//! `loader.rs`/`fetch.rs`/`lock.rs` operate on.

mod Convert;
mod Discovery;
mod Edit;
mod Helpers;
mod ParseBlocks;

pub use Convert::{new_template, to_manifest};
pub use Discovery::{discover_module_in, DiscoveryError};
pub use Edit::{add_dep, remove_dep};

use super::RefSpec::{RefError, Source};
use crate::Syntax;
use Helpers::block_body;
use ParseBlocks::{parse_deps, parse_package, parse_packages};

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

/// The realize axis for a package (U10): `library` is imported for code;
/// `executable` installs a binary on PATH (the devshell case). Derived from a
/// package's `targets:` list (D-TGT1) — an executable target wins, else library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageKind {
    Library,
    Executable,
}

/// One build target of a package (D-TGT1/D-TGT2, ratified 2026-06-21). The five
/// shipped targets; `plugin` is still reserved and rejected at parse time (c81).
/// `benchmark` (c80) routes `jet bench` at the package entry via the existing
/// `#Bench`/`jet bench` engine — it is not a new mechanism (I8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Library,
    Executable,
    Test,
    Example,
    /// c80 / D-TGT2: this package's entry is a benchmark; `jet bench` runs its
    /// `#Bench` regions via the shipped `compile_benches_with_path` path.
    Benchmark,
}

/// The capability-API mode of a library target (D-CAP4/D-CAP6). Default is
/// `Inferred` (no `api:` field) — capabilities are inferred and never frozen.
/// `Stable` and `Explicit` both freeze the resolved public capability signature
/// into durable interface metadata (c129); the difference is documentation
/// strictness, not freeze behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ApiMode {
    /// No `api:` field — inference only, signatures never frozen (D-CAP6 default).
    #[default]
    Inferred,
    /// `api: stable` — record resolved capability signatures and flag breaks.
    Stable,
    /// `api: explicit` — same freeze, plus hand-written annotations expected.
    Explicit,
}

impl ApiMode {
    /// `true` when this mode freezes the public capability signature into durable
    /// interface metadata (c129) — i.e. anything but the inferred default.
    pub fn freezes(self) -> bool {
        matches!(self, ApiMode::Stable | ApiMode::Explicit)
    }
}

/// One entry in the `packages: { … }` block (U10 + D-TGT1). `targets` is empty when
/// the manifest declares none (D-ILE1) — the kind is then inferred from the module's
/// `fn main` at realize time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageEntry {
    pub name: String,
    pub targets: Vec<Target>,
    /// The `api:` mode of this package's `library` target (D-CAP4). `Inferred`
    /// when no library target sets `api:`. Drives the c129 capability freeze.
    pub api: ApiMode,
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
        selector: crate::Manifest::GitSelector,
    },
    /// A native C-library link dependency (S59/D-CFFI2): `lib: c@system`
    /// (pkg-config, bare `-l <lib>` fallback) or `lib: c@"vendor/path"` (local
    /// dir). `target` is `"system"` or the unquoted local path. A CLib dep is a
    /// link dep, not a Jet package — it is skipped in package realization and
    /// never written to the package lock; `Source/CFFI.rs` reads it for `-L`/
    /// `-I`/`-l` link flags.
    CLib { target: String },
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
    /// A `packages:` entry names a target that is not a known shipped target (E1210).
    BadTarget { name: String, value: String },
    /// A `packages:` entry names a reserved target (`benchmark`/`plugin`) whose
    /// backend has not shipped yet (E1210, D-TGT2).
    ReservedTarget { name: String, value: String },
    /// A `packages:` block-form entry uses the removed `kind:` field; write
    /// `targets: [ … ]` instead (E1211, D-TGT1).
    KindFieldRemoved { name: String },
    /// A target block carries an unknown field, or `api:` has a value other than
    /// `stable`/`explicit` (E1215, D-TGT3/D-CAP4).
    BadTargetField { name: String, detail: String },
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
        dir.join(Syntax::PAYLOAD_FILE)
    }

    /// Load and parse the package manifest in `dir`, if present.
    pub fn load(dir: &std::path::Path) -> Option<Result<PackManifest, ManifestError>> {
        let text = std::fs::read_to_string(Self::path_in(dir)).ok()?;
        Some(parse(&text))
    }

    /// The declared kind of package `name`, derived from its `targets:` list
    /// (D-TGT1): an `executable` target wins, else a `library` target. Returns
    /// `None` when the package is not listed *or* declares no library/executable
    /// target (D-ILE1) — both leave the kind to be inferred from the source at
    /// realize time.
    pub fn package_kind(&self, name: &str) -> Option<PackageKind> {
        let entry = self.packages.iter().find(|p| p.name == name)?;
        if entry.targets.iter().any(|t| *t == Target::Executable) {
            Some(PackageKind::Executable)
        } else if entry.targets.iter().any(|t| *t == Target::Library) {
            Some(PackageKind::Library)
        } else {
            None
        }
    }
}

/// Parse a `pkg.jet` package manifest from its text (U1/U10).
pub fn parse(text: &str) -> Result<PackManifest, ManifestError> {
    let text = Helpers::strip_line_comments(text);

    let package = match block_body(&text, Syntax::MANIFEST_BLOCK_PAYLOAD, '{', '}') {
        Some(body) => parse_package(&body)?,
        None => return Err(ManifestError::MissingPayload),
    };

    let deps = match block_body(&text, "deps", '{', '}') {
        Some(body) => parse_deps(&body)?,
        None => Vec::new(),
    };

    let packages = match block_body(&text, Syntax::MANIFEST_BLOCK_PACKAGES, '{', '}') {
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
    use super::Discovery::file_declares_module;
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
        assert_eq!(m.packages[0].targets, vec![Target::Library]);
        assert_eq!(m.packages[1].name, "cli");
        assert_eq!(m.packages[1].targets, vec![Target::Executable]);
        assert_eq!(m.package_kind("web"), Some(PackageKind::Library));
        assert_eq!(m.package_kind("cli"), Some(PackageKind::Executable));
    }

    #[test]
    fn targets_are_optional_and_inferred() {
        // D-ILE1/D-TGT1: a bare `name` declares no targets (inferred from the
        // module's `fn main` at realize time); an explicit target still wins.
        let src =
            "payload: { name: \"x\", version: \"1\" }\npackages: { deploy, web: library }";
        let m = parse(src).unwrap();
        assert_eq!(m.packages.len(), 2);
        assert_eq!(m.packages[0].name, "deploy");
        assert!(m.packages[0].targets.is_empty());
        assert_eq!(m.packages[1].name, "web");
        assert_eq!(m.packages[1].targets, vec![Target::Library]);
        // package_kind collapses "not listed" and "no library/executable target"
        // to None so the provider infers in both cases.
        assert_eq!(m.package_kind("deploy"), None);
        assert_eq!(m.package_kind("web"), Some(PackageKind::Library));
        assert_eq!(m.package_kind("absent"), None);
    }

    #[test]
    fn packages_block_targets_list() {
        // D-TGT1/D-TGT3: a block-form package lists `targets:`; bare and
        // block-with-fields target entries coexist.
        let src = r#"
payload: { name: "x", version: "1" }
packages: {
    server: { targets: [executable { entry: "src/cli.jet" }] },
    utils:  { targets: [library, test] },
}
"#;
        let m = parse(src).unwrap();
        assert_eq!(m.packages.len(), 2);
        assert_eq!(m.packages[0].targets, vec![Target::Executable]);
        assert_eq!(m.packages[1].targets, vec![Target::Library, Target::Test]);
        assert_eq!(m.package_kind("server"), Some(PackageKind::Executable));
        assert_eq!(m.package_kind("utils"), Some(PackageKind::Library));
    }

    #[test]
    fn deps_and_packages_are_optional() {
        let m = parse("payload: { name: \"x\", version: \"0.0.1\" }").unwrap();
        assert!(m.deps.is_empty());
        assert!(m.packages.is_empty());
        assert_eq!(m.package.name, "x");
    }

    #[test]
    fn bad_target_bare_errors() {
        let src = "payload: { name: \"x\", version: \"1\" }\npackages: { web: zonk }";
        let err = parse(src).unwrap_err();
        assert!(
            matches!(err, ManifestError::BadTarget { ref name, ref value }
                if name == "web" && value == "zonk"),
            "{err:?}"
        );
    }

    #[test]
    fn reserved_target_errors() {
        // D-TGT2: `plugin` is reserved until c81 ships (D-DEP-WASM1).
        let src = "payload: { name: \"x\", version: \"1\" }\npackages: { web: plugin }";
        let err = parse(src).unwrap_err();
        assert!(
            matches!(err, ManifestError::ReservedTarget { ref name, ref value }
                if name == "web" && value == "plugin"),
            "{err:?}"
        );
    }

    #[test]
    fn benchmark_target_parses() {
        // c80 / D-TGT2: `benchmark` is no longer reserved — it now produces
        // `Target::Benchmark` and wires into the existing `jet bench` engine.
        let src = "payload: { name: \"x\", version: \"1\" }\npackages: { perf: benchmark }";
        let m = parse(src).unwrap();
        assert_eq!(m.packages[0].targets, vec![Target::Benchmark]);
        // A benchmark-only package has no PackageKind (not library/executable).
        assert_eq!(m.package_kind("perf"), None);
    }

    #[test]
    fn target_block_accepts_known_fields() {
        // D-TGT3/D-TGT4/D-CAP4: entry/name/api are valid target-block fields.
        let src = r#"
payload: { name: "x", version: "1" }
packages: {
    app: { targets: [executable { name: "app", entry: "src/cli.jet" }, library { api: stable }] },
}
"#;
        let m = parse(src).unwrap();
        assert_eq!(m.packages[0].targets, vec![Target::Executable, Target::Library]);
        assert_eq!(m.package_kind("app"), Some(PackageKind::Executable));
    }

    #[test]
    fn target_block_unknown_field_errors() {
        let src =
            "payload: { name: \"x\", version: \"1\" }\npackages: { app: { targets: [executable { bogus: 1 }] } }";
        let err = parse(src).unwrap_err();
        assert!(
            matches!(err, ManifestError::BadTargetField { ref name, ref detail }
                if name == "app" && detail.contains("bogus")),
            "{err:?}"
        );
    }

    #[test]
    fn target_block_bad_api_mode_errors() {
        // D-CAP4: api: only stable/explicit.
        let src =
            "payload: { name: \"x\", version: \"1\" }\npackages: { app: { targets: [library { api: zonk }] } }";
        let err = parse(src).unwrap_err();
        assert!(
            matches!(err, ManifestError::BadTargetField { ref name, ref detail }
                if name == "app" && detail.contains("zonk")),
            "{err:?}"
        );
    }

    #[test]
    fn kind_field_is_removed() {
        // D-TGT1: the old `kind:` field is a teaching error pointing at `targets:`.
        let src =
            "payload: { name: \"x\", version: \"1\" }\npackages: { web: { kind: executable } }";
        let err = parse(src).unwrap_err();
        assert!(
            matches!(err, ManifestError::KindFieldRemoved { ref name } if name == "web"),
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
    fn c_lib_dep_system_and_path() {
        // S59/D-CFFI2: `c@system` and `c@"path"` parse to DepSource::CLib.
        let src = r#"
payload: { name: "p", version: "0.1.0" }
deps: {
    raylib: c@system,
    mylib:  c@"vendor/mylib",
    c:      c@system,
}
"#;
        let m = parse(src).unwrap();
        assert_eq!(m.deps.len(), 3);
        assert_eq!(m.deps[0].name, "raylib");
        assert_eq!(m.deps[0].source, DepSource::CLib { target: "system".into() });
        assert_eq!(m.deps[1].name, "mylib");
        assert_eq!(
            m.deps[1].source,
            DepSource::CLib { target: "vendor/mylib".into() }
        );
        assert_eq!(m.deps[2].name, "c");
        assert_eq!(m.deps[2].source, DepSource::CLib { target: "system".into() });
    }

    #[test]
    fn c_lib_dep_is_skipped_in_to_manifest() {
        // A CLib dep is a link dep, not a Jet package: it must not appear in the
        // converted Manifest's dependency map (never realized / locked).
        let src = r#"
payload: { name: "p", version: "0.1.0" }
deps: { textkit: "1.2.0", raylib: c@system }
"#;
        let m = parse(src).unwrap();
        let mf = to_manifest(&m, src).unwrap();
        assert!(mf.dependencies.contains_key("textkit"));
        assert!(
            !mf.dependencies.contains_key("raylib"),
            "C link dep must not be a converted Jet dependency"
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
                selector: crate::Manifest::GitSelector::Tag("v0.4.1".into()),
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
                selector: crate::Manifest::GitSelector::Branch("main".into()),
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
                selector: crate::Manifest::GitSelector::Rev("abc123".into()),
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
            Some(&crate::Manifest::DepSpec::Registry("1.2.0".into()))
        );
        assert_eq!(
            mf.dependencies.get("helpers"),
            Some(&crate::Manifest::DepSpec::Path {
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
            Some(&crate::Manifest::DepSpec::Git {
                url: "https://github.com/acme/parsekit".into(),
                selector: crate::Manifest::GitSelector::Tag("v0.4.1".into()),
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
            Some(&crate::Manifest::DepSpec::Git {
                url: "https://github.com/NixOS/nixpkgs".into(),
                selector: crate::Manifest::GitSelector::Rev("nixos-24.05".into()),
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
            &crate::Manifest::DepSpec::Path {
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
            &crate::Manifest::DepSpec::Path { path: "../b".into() },
        );
        let m = parse(&updated).unwrap();
        assert_eq!(m.deps.len(), 2);

        // Replacing an existing dep updates in place rather than duplicating.
        let updated2 = add_dep(
            &updated,
            "a",
            &crate::Manifest::DepSpec::Registry("2.0.0".into()),
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
