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
//!     textkit: textkit#1.2.0,
//!     helpers:  ../helpers,
//!     parsekit: { git: "https://github.com/acme/parsekit", tag: "v0.4.1" },
//! }
//! ```
//!
//! This module is the structural parser for that shape (U1). It is std-only
//! (I6). Dependency values are a `name#version` pin, a bare local path, a
//! `target@provider` source ref (`owner/repo/rev@github`, classified through
//! `RefSpec::classify_provider_ref`, D-JPK-REF1), or an inline
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
pub use ParseBlocks::parse_build;

use super::RefSpec::{RefError, Source};
use crate::Syntax;
use Helpers::block_body;
use ParseBlocks::{
    parse_auto_derive_policy, parse_build_allow, parse_deps, parse_effects, parse_grants,
    parse_lints_policy, parse_package, parse_memory_policy, parse_packages, parse_provider_policy,
    parse_trust_policy,
};

/// D-BUILDPROFILE1: optimization level for a named build profile. Stored in
/// `Build.{ optimize: … }` inside `pkg.jet`'s `build { }` block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildOptimize {
    /// `optimize: none` — no optimization flags; fastest compile, slowest binary.
    /// Maps to rustc opt-level=0 (debug builds).
    None,
    /// `optimize: basic` — `-C opt-level=2`; the driver default when no profile is set.
    Basic,
    /// `optimize: full` — `-C opt-level=3`; maximum throughput (release builds).
    Full,
}

impl BuildOptimize {
    /// The `optimize:` value string this level maps to (from `Syntax::BUILD_OPTIMIZE_*`).
    pub fn as_str(self) -> &'static str {
        match self {
            BuildOptimize::None => Syntax::BUILD_OPTIMIZE_NONE,
            BuildOptimize::Basic => Syntax::BUILD_OPTIMIZE_BASIC,
            BuildOptimize::Full => Syntax::BUILD_OPTIMIZE_FULL,
        }
    }

    /// Cache-key tag string — unique per level so cache entries never collide.
    pub fn cache_tag(self) -> &'static str {
        match self {
            BuildOptimize::None => "opt:none",
            BuildOptimize::Basic => "opt:basic",
            BuildOptimize::Full => "opt:full",
        }
    }
}

/// D-BUILDPROFILE1: `panic:` mode for a build profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BuildPanic {
    #[default]
    Unwind,
    Abort,
}

/// D-BUILDPROFILE1: one named build profile declared in `pkg.jet`'s `build { }` block.
/// Written as `name: Build.{ optimize: <level>, … }`. Blessed names `release`/`debug`/`ci`
/// have built-in defaults; entries here override or extend them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildProfileDef {
    pub name: String,
    pub optimize: BuildOptimize,
    pub debug_info: bool,
    pub small: bool,
    pub panic: Option<BuildPanic>,
    pub features: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// D-JPK-GRANTSCHEMA1=A: source-reviewed trust policy from
/// `policy: { trust: { … } }`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrustPolicy {
    pub default: Option<TrustDecision>,
    pub ci_prompt: Option<TrustDecision>,
    pub services: Vec<(String, TrustDecision)>,
}

/// One reviewed `policy.providers.<root>` source authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderAuthority {
    pub provider: String,
    pub registry: String,
    pub allow: Vec<String>,
    pub deny: Vec<String>,
}

/// One trust decision value in `policy.trust`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustDecision {
    Allow,
    Prompt,
    Deny,
}

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
    /// D-RINGLAYER1=A: optional runtime ceiling (`core` / `alloc` / `hosted`).
    pub layer: Option<crate::Syntax::RuntimeLayer>,
    /// D-WEBDEFAULT1 (ratified 2026-07-01, c134): `target: "web"` — this package's default
    /// CLI backend, the manifest-level counterpart to a loose file's
    /// `#Target(Web)` marker. `--target=<x>` on the command line still wins.
    pub target: Option<String>,
}

/// The realize axis for a package (U10): `library` is imported for code;
/// `executable` installs a binary on PATH (the devshell case). Derived from a
/// package's `targets:` list (D-TGT1) — an executable target wins, else library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageKind {
    Library,
    Executable,
}

/// One build target of a package (D-TGT1/D-TGT2, ratified 2026-06-21). The six
/// shipped targets. `benchmark` (c80) routes `jet bench` at the package entry
/// via the existing `#Bench`/`jet bench` engine — it is not a new mechanism
/// (I8). `plugin` (c81, D-PLUGIN1=B/D-DEP-WASM1=A) builds a sandboxed `wasm32`
/// Component Model module instead of a native binary — a package is *loaded*,
/// not imported or PATH-installed, so it maps to no `PackageKind`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Library,
    Executable,
    Test,
    Example,
    /// c80 / D-TGT2: this package's entry is a benchmark; `jet bench` runs its
    /// `#Bench` regions via the shipped `compile_benches_with_path` path.
    Benchmark,
    /// c81 / D-PLUGIN1=B: this package builds to a sandboxed WASM Component
    /// Model module. `export` is the `.wit` world name (D-PLUGIN-EXPORT1=A,
    /// `export:` target field) — `None` when omitted, defaulting to the
    /// package name at build time.
    Plugin {
        export: Option<String>,
    },
}

/// One entry in the `packages: { … }` block (U10 + D-TGT1). `targets` is empty when
/// the manifest declares none (D-ILE1) — the kind is then inferred from the module's
/// `fn run` at realize time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageEntry {
    pub name: String,
    pub targets: Vec<Target>,
}

/// Where a dependency resolves from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepSource {
    /// The selector from a `name#version` registry ref.
    Version(String),
    /// A bare path or `target@provider` source ref.
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
    /// D-BUILDPROFILE1: named build profiles from the `build { }` block, in
    /// declaration order. Empty when no `build: { … }` block is present.
    pub build_profiles: Vec<BuildProfileDef>,
    /// D-CTEFFECT1: standing capabilities granted to this package's `fn build`.
    /// Parsed from the exact `build: { allow: #(…) }` field.
    pub build_allow: Vec<String>,
    /// D-EFFBUDGET1: whether an `effects: { … }` block is present at all. Its
    /// presence (even empty) turns on whole-graph enforcement; absence means
    /// report-only (the always-on summary still prints).
    pub effects_enabled: bool,
    /// D-EFFBUDGET1: `effects: { allow: […], deny: […] }` halves. `None` for a
    /// half means that half imposes no restriction; `Some(vec![])` (an
    /// explicit empty list) restricts to nothing.
    pub effects_allow: Option<Vec<String>>,
    pub effects_deny: Option<Vec<String>>,
    /// D-EFFBUDGET1: `grants: { "dep": [Effect], … }` — the audited
    /// per-dependency escape from the `effects:` budget, in declaration order.
    pub grants: Vec<(String, Vec<String>)>,
    /// D-JPK-GRANTSCHEMA1=A: reviewed trust policy facts for the unified grant
    /// graph.
    pub trust_policy: Option<TrustPolicy>,
    /// D-JPK-PROVIDERAUTH1=A: explicit provider registry/fetch authorities.
    pub provider_policy: Vec<ProviderAuthority>,
    /// D-LINTPOLICY1=A (the override law): lint codes from
    /// `policy: { lints: { deny: […] } }` that fail the build when they fire.
    /// `None` when no `policy.lints` block is present at all (warn-never-block
    /// stays the default, I1/D-LINTPOLICY1); `Some(vec![])` is an explicit
    /// empty deny list (a no-op wall).
    pub lints_deny: Option<Vec<String>>,
    /// D-PACKAGE-POLICY-SCOPE1: typed package policy declarations; tightening only.
    pub memory_policy: Vec<crate::Policy::PolicyDeclaration>,
    /// D-AUTODERIVE1=E: absent means the safe beginner default, enabled.
    pub auto_derive: Option<bool>,
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
    /// A `deps` value is not a `name#version`, bare path, or source ref.
    BadDepValue { name: String, value: String },
    /// A dependency source ref failed to classify (D-JPK-REF1).
    BadDepRef { name: String, err: RefError },
    /// An inline git dep (D-JPK23) is missing `git`, or doesn't have exactly
    /// one of `tag`/`branch`/`rev`.
    BadGitDep { name: String, reason: &'static str },
    /// A `packages:` entry names a target that is not a known shipped target (E1210).
    BadTarget { name: String, value: String },
    /// A `packages:` entry names a target reserved for a future increment whose
    /// backend has not shipped yet (E1210, D-TGT2). `benchmark` (c80) and
    /// `plugin` (c81) have both shipped; `TARGET_RESERVED` is empty until the
    /// next reserved target is proposed — this variant stays as the generic
    /// scaffold for it.
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
    /// D-BUILDPROFILE1: a `build { }` profile entry is malformed — missing the
    /// `Build.{ optimize: … }` value shape or has an unknown optimize level.
    BadBuildProfile { name: String, reason: &'static str },
    /// D-RINGLAYER1=A: `runtime:` in `payload` is not `core`, `alloc`, or `hosted`.
    BadLayer { value: String },
    /// D-EFFBUDGET1 (E1221): a malformed `effects:`/`grants:` block — an
    /// unknown field, a non-list value, or an effect name outside D-EFF4.
    BadEffectsBlock { detail: String },
    /// D-JPK-GRANTSCHEMA1=A: malformed `policy.trust`.
    BadTrustPolicy { detail: String },
    /// D-JPK-PROVIDERAUTH1=A: malformed `policy.providers` authority.
    BadProviderPolicy { detail: String },
    /// D-LINTPOLICY1=A: malformed `policy.lints` — an unknown field, a
    /// non-list `deny:` value, or an entry not shaped like a lint code.
    BadLintsPolicy { detail: String },
    /// D-PACKAGE-POLICY-SCOPE1: malformed or widening package policy.
    BadMemoryPolicy { detail: String },
    /// D-AUTODERIVE-SYNTAX1=D: malformed `policy.auto_derive`.
    BadAutoDerivePolicy { detail: String },
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
        let path = Self::path_in(dir);
        let text = std::fs::read_to_string(&path).ok()?;
        Some(parse(&text).map(|mut manifest| {
            let source = path.display().to_string();
            for declaration in &mut manifest.memory_policy { declaration.source = source.clone(); }
            manifest
        }))
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

/// Parse a `pkg.jet` package manifest from its text (U1/U10/D-BUILDPROFILE1).
pub fn parse(text: &str) -> Result<PackManifest, ManifestError> {
    let text = Helpers::strip_line_comments(text);

    let package = match block_body(&text, Syntax::MANIFEST_BLOCK_PAYLOAD, '{', '}').or_else(|| block_body(&text, "identity", '{', '}')) {
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

    // D-BUILDPROFILE1: parse named build profiles from `build: { … }`.
    let build_profiles = match block_body(&text, Syntax::MANIFEST_BLOCK_BUILD, '{', '}') {
        Some(body) => parse_build(&body)?,
        None => Vec::new(),
    };
    let build_allow = match block_body(&text, Syntax::MANIFEST_BLOCK_BUILD, '{', '}') {
        Some(body) => parse_build_allow(&body)?,
        None => Vec::new(),
    };

    // D-EFFBUDGET1: `effects: { allow: […], deny: […] }` turns on whole-graph
    // enforcement; absent entirely means report-only (no enforcement).
    let effects_block = block_body(&text, Syntax::MANIFEST_BLOCK_EFFECTS, '{', '}');
    let effects_enabled = effects_block.is_some();
    let (effects_allow, effects_deny) = match effects_block {
        Some(body) => parse_effects(&body)?,
        None => (None, None),
    };

    // D-EFFBUDGET1: `grants: { "dep": [Effect], … }` — the audited per-dep escape.
    let grants = match block_body(&text, Syntax::MANIFEST_BLOCK_GRANTS, '{', '}') {
        Some(body) => parse_grants(&body)?,
        None => Vec::new(),
    };

    let trust_policy = match block_body(&text, Syntax::MANIFEST_BLOCK_POLICY, '{', '}') {
        Some(body) => parse_trust_policy(&body)?,
        None => None,
    };
    let provider_policy = match block_body(&text, Syntax::MANIFEST_BLOCK_POLICY, '{', '}') {
        Some(body) => parse_provider_policy(&body)?,
        None => Vec::new(),
    };

    // D-LINTPOLICY1=A: `policy: { lints: { deny: […] } }` — absent entirely
    // means warn-never-block stays the default.
    let lints_deny = match block_body(&text, Syntax::MANIFEST_BLOCK_POLICY, '{', '}') {
        Some(body) => parse_lints_policy(&body)?,
        None => None,
    };
    let memory_policy = match block_body(&text, Syntax::MANIFEST_BLOCK_POLICY, '{', '}') {
        Some(body) => parse_memory_policy(&body)?,
        None => Vec::new(),
    };
    let auto_derive = match block_body(&text, Syntax::MANIFEST_BLOCK_POLICY, '{', '}') {
        Some(body) => parse_auto_derive_policy(&body)?,
        None => None,
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
        build_profiles,
        build_allow,
        effects_enabled,
        effects_allow,
        effects_deny,
        grants,
        trust_policy,
        provider_policy,
        lints_deny,
        memory_policy,
        auto_derive,
    })
}

/// Return the Jet declarations that live beside the manifest blocks in
/// `pkg.jet`, preserving their original byte offsets.  D-BUILDSCOPE1 makes
/// `pkg.jet` a valid home for the package's single `fn build`; the manifest
/// reader must therefore expose that source without teaching the manifest
/// grammar how to parse ordinary Jet items.
///
/// Known manifest blocks are blanked with spaces (newlines are retained), so
/// diagnostics from the later compiler parse still point at `pkg.jet`.  The
/// result is `None` when the remaining top-level source has no `fn build`.
pub fn build_entry_source(text: &str) -> Option<String> {
    let mut masked = text.as_bytes().to_vec();
    mask_manifest_blocks(text, &mut masked);
    let source = String::from_utf8(masked).ok()?;
    has_top_level_build_function(&source).then_some(source)
}

const MANIFEST_BLOCKS: &[&str] = &[
    Syntax::MANIFEST_BLOCK_PAYLOAD,
    "identity",
    "deps",
    Syntax::MANIFEST_BLOCK_PACKAGES,
    Syntax::MANIFEST_BLOCK_BUILD,
    Syntax::MANIFEST_BLOCK_EFFECTS,
    Syntax::MANIFEST_BLOCK_GRANTS,
    Syntax::MANIFEST_BLOCK_POLICY,
    "dev_deps",
    "patch",
    "workspace",
];

fn mask_manifest_blocks(text: &str, masked: &mut [u8]) {
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut paren_depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    while i < bytes.len() {
        if in_string {
            if escaped {
                escaped = false;
            } else if bytes[i] == b'\\' {
                escaped = true;
            } else if bytes[i] == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if bytes[i] == b'"' {
            in_string = true;
            i += 1;
            continue;
        }
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/') {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if brace_depth == 0 && bracket_depth == 0 && paren_depth == 0 {
            if let Some((start, open)) = manifest_block_start(bytes, i) {
                if let Some(end) = balanced_block_end(text, open) {
                    blank_range(masked, start, end);
                    i = end;
                    continue;
                }
            }
        }
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i = skip_block_comment(bytes, i);
            continue;
        }
        match bytes[i] {
            b'{' => brace_depth += 1,
            b'}' => brace_depth = brace_depth.saturating_sub(1),
            b'[' => bracket_depth += 1,
            b']' => bracket_depth = bracket_depth.saturating_sub(1),
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            _ => {}
        }
        i += 1;
    }
}

fn manifest_block_start(bytes: &[u8], at: usize) -> Option<(usize, usize)> {
    for key in MANIFEST_BLOCKS {
        let end = at.checked_add(key.len())?;
        if bytes.get(at..end) != Some(key.as_bytes()) {
            continue;
        }
        let before_ok = at == 0 || !is_ident_byte(bytes[at - 1]);
        let after_ok = end == bytes.len() || !is_ident_byte(bytes[end]);
        if !before_ok || !after_ok {
            continue;
        }
        let mut cursor = end;
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if bytes.get(cursor) != Some(&b':') {
            continue;
        }
        cursor += 1;
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if bytes.get(cursor) == Some(&b'.') {
            cursor += 1;
            while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
                cursor += 1;
            }
        }
        if bytes.get(cursor) == Some(&b'{') {
            return Some((at, cursor));
        }
    }
    None
}

fn balanced_block_end(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut i = open;
    let mut in_string = false;
    let mut escaped = false;
    while i < bytes.len() {
        if in_string {
            if escaped {
                escaped = false;
            } else if bytes[i] == b'\\' {
                escaped = true;
            } else if bytes[i] == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if bytes[i] == b'"' {
            in_string = true;
        } else if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/') {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        } else if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i = skip_block_comment(bytes, i);
            continue;
        } else if bytes[i] == b'{' {
            depth += 1;
        } else if bytes[i] == b'}' {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(i + 1);
            }
        }
        i += 1;
    }
    None
}

fn blank_range(masked: &mut [u8], start: usize, end: usize) {
    for byte in masked.get_mut(start..end).into_iter().flatten() {
        if *byte != b'\n' && *byte != b'\r' {
            *byte = b' ';
        }
    }
}

fn has_top_level_build_function(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    while i < bytes.len() {
        if in_string {
            if escaped {
                escaped = false;
            } else if bytes[i] == b'\\' {
                escaped = true;
            } else if bytes[i] == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if bytes[i] == b'"' {
            in_string = true;
            i += 1;
            continue;
        }
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/') {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i = skip_block_comment(bytes, i);
            continue;
        }
        if depth == 0 && word_at(bytes, i, b"fn") {
            let mut cursor = i + 2;
            while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
                cursor += 1;
            }
            if word_at(bytes, cursor, b"build") {
                return true;
            }
        }
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        i += 1;
    }
    false
}

fn skip_block_comment(bytes: &[u8], start: usize) -> usize {
    let mut depth = 1usize;
    let mut i = start.saturating_add(2);
    while i < bytes.len() {
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
            depth = depth.saturating_add(1);
            i += 2;
        } else if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
            depth = depth.saturating_sub(1);
            i += 2;
            if depth == 0 {
                return i;
            }
        } else {
            i += 1;
        }
    }
    bytes.len()
}

fn word_at(bytes: &[u8], at: usize, word: &[u8]) -> bool {
    let end = at.saturating_add(word.len());
    end <= bytes.len()
        && &bytes[at..end] == word
        && (at == 0 || !is_ident_byte(bytes[at - 1]))
        && (end == bytes.len() || !is_ident_byte(bytes[end]))
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// D-UNSAFE-OBLIG1=A: parse the manifest-shaped admin policy document without
/// requiring package identity. It accepts exactly the ordinary `policy` block.
pub fn parse_policy_document(text: &str) -> Result<Vec<crate::Policy::PolicyDeclaration>, ManifestError> {
    let text = Helpers::strip_line_comments(text);
    let mut rest = text.trim().strip_prefix(Syntax::MANIFEST_BLOCK_POLICY)
        .ok_or(ManifestError::BadMemoryPolicy { detail: "expected only `policy: .{ … }`".to_string() })?
        .trim_start();
    rest = rest.strip_prefix(':')
        .ok_or(ManifestError::BadMemoryPolicy { detail: "expected `:` after `policy`".to_string() })?
        .trim_start();
    rest = rest.strip_prefix('.').unwrap_or(rest).trim_start();
    rest = rest.strip_prefix('{')
        .ok_or(ManifestError::BadMemoryPolicy { detail: "expected `.{` after `policy:`".to_string() })?;
    let close = rest.find('}')
        .ok_or(ManifestError::BadMemoryPolicy { detail: "missing `}` after organization policy".to_string() })?;
    if !rest[close + 1..].trim().is_empty() {
        return Err(ManifestError::BadMemoryPolicy { detail: "organization policy file may contain only the `policy` block".to_string() });
    }
    parse_memory_policy(&rest[..close])
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
    textkit: textkit#1.2.0,
    helpers: ../helpers,
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
    fn parses_auto_derive_package_policy() {
        let disabled = parse(
            "payload: { name: \"app\", version: \"1\" }\npolicy: .{ auto_derive: false }",
        )
        .unwrap();
        assert_eq!(disabled.auto_derive, Some(false));

        let defaulted = parse("payload: { name: \"app\", version: \"1\" }").unwrap();
        assert_eq!(defaulted.auto_derive, None);

        for policy in [
            "policy: .{ auto_derive: sometimes }",
            "policy: .{ auto_derive: true, auto_derive: false }",
        ] {
            assert!(
                matches!(
                    parse(&format!(
                        "payload: {{ name: \"app\", version: \"1\" }}\n{policy}"
                    )),
                    Err(ManifestError::BadAutoDerivePolicy { .. })
                ),
                "{policy}"
            );
        }
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
        // module's `fn run` at realize time); an explicit target still wins.
        let src = "payload: { name: \"x\", version: \"1\" }\npackages: { deploy, web: library }";
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
    fn plugin_target_parses() {
        // c81 / D-PLUGIN1=B: `plugin` is no longer reserved — it now produces
        // `Target::Plugin` and routes to the wasm32 Component Model backend.
        let src = "payload: { name: \"x\", version: \"1\" }\npackages: { web: plugin }";
        let m = parse(src).unwrap();
        assert_eq!(m.packages[0].targets, vec![Target::Plugin { export: None }]);
        // A plugin-only package has no PackageKind — it is loaded, not
        // imported or PATH-installed.
        assert_eq!(m.package_kind("web"), None);
    }

    #[test]
    fn plugin_target_export_field() {
        // D-PLUGIN-EXPORT1=A: `export:` names the `.wit` world.
        let src = r#"
payload: { name: "x", version: "1" }
packages: {
    mathkit: { targets: [plugin { export: "mathkit" }] },
}
"#;
        let m = parse(src).unwrap();
        assert_eq!(
            m.packages[0].targets,
            vec![Target::Plugin {
                export: Some("mathkit".to_string())
            }]
        );
    }

    #[test]
    fn plugin_target_rejects_unknown_field() {
        let src = r#"
payload: { name: "x", version: "1" }
packages: {
    mathkit: { targets: [plugin { api: stable }] },
}
"#;
        let err = parse(src).unwrap_err();
        assert!(
            matches!(err, ManifestError::BadTargetField { ref name, .. } if name == "mathkit"),
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
        // D-TGT3/D-TGT4: entry/name are valid target-block fields.
        let src = r#"
payload: { name: "x", version: "1" }
packages: {
    app: { targets: [executable { name: "app", entry: "src/cli.jet" }, library] },
}
"#;
        let m = parse(src).unwrap();
        assert_eq!(
            m.packages[0].targets,
            vec![Target::Executable, Target::Library]
        );
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
    fn target_block_api_field_is_unknown_field_error() {
        // D-MEM1/S2 greenfield: `api:` (was D-CAP4) ceases to exist — an
        // ordinary unknown-field error, exactly like any other typo'd key.
        let src =
            "payload: { name: \"x\", version: \"1\" }\npackages: { app: { targets: [library { api: stable }] } }";
        let err = parse(src).unwrap_err();
        assert!(
            matches!(err, ManifestError::BadTargetField { ref name, ref detail }
                if name == "app" && detail.contains("api")),
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
deps: { up: NixOS/nixpkgs/nixos-24.05@github }
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
    fn provider_first_dep_is_a_teaching_error() {
        let err = parse(
            r#"payload: { name: "p", version: "0.1.0" }
deps: { up: github@NixOS/nixpkgs/nixos-24.05 }
"#,
        )
        .unwrap_err();
        assert!(matches!(
            err,
            ManifestError::BadDepRef {
                err: RefError::ProviderFirst {
                    ref replacement,
                    ..
                },
                ..
            } if replacement == "NixOS/nixpkgs/nixos-24.05@github"
        ));
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
        assert_eq!(
            m.deps[0].source,
            DepSource::CLib {
                target: "system".into()
            }
        );
        assert_eq!(m.deps[1].name, "mylib");
        assert_eq!(
            m.deps[1].source,
            DepSource::CLib {
                target: "vendor/mylib".into()
            }
        );
        assert_eq!(m.deps[2].name, "c");
        assert_eq!(
            m.deps[2].source,
            DepSource::CLib {
                target: "system".into()
            }
        );
    }

    #[test]
    fn c_lib_dep_is_skipped_in_to_manifest() {
        // A CLib dep is a link dep, not a Jet package: it must not appear in the
        // converted Manifest's dependency map (never realized / locked).
        let src = r#"
payload: { name: "p", version: "0.1.0" }
deps: { textkit: textkit#1.2.0, raylib: c@system }
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
        let err =
            parse("payload: { name: \"x\", version: \"1\" }\ndeps: { y: notaref }").unwrap_err();
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
    textkit: textkit#1.2.0,
    helpers:  ../helpers,
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
deps: { up: NixOS/nixpkgs/nixos-24.05@github }
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
deps: { x: fastfetch@nixpkgs }
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
            m.deps
                .iter()
                .find(|d| d.name == "helpers")
                .map(|d| &d.source),
            Some(&DepSource::Provider {
                provider: Source::Path,
                target: "../helpers".into(),
            })
        );
    }

    #[test]
    fn add_dep_inserts_into_existing_block_and_replaces() {
        let raw = "payload: { name: \"x\", version: \"1\" }\n\ndeps: {\n    a: a#1.0.0,\n}\n";
        let updated = add_dep(
            raw,
            "b",
            &crate::Manifest::DepSpec::Path {
                path: "../b".into(),
            },
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
        let raw = "payload: { name: \"x\", version: \"1\" }\n\ndeps: {\n    a: a#1.0.0,\n    b: b#2.0.0,\n}\n";
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
        std::fs::write(
            root.join("x.jet"),
            "// module hello { }\nmodule other { }\n",
        )
        .unwrap();
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

    // ── D-BUILDPROFILE1: build { } profiles ──

    #[test]
    fn parses_build_profiles_block() {
        let src = r#"
payload: { name: "p", version: "0.1.0" }
build: {
    release: Build.{ optimize: full },
    debug: { optimize: none, debug_info: true },
    ci: Build.{ optimize: basic, debug_info: true, panic: abort },
    fast: Build.{ optimize: full, features: [ "fast_path" ] },
}
"#;
        let m = parse(src).unwrap();
        assert_eq!(m.build_profiles.len(), 4);
        assert_eq!(m.build_profiles[0].name, "release");
        assert_eq!(m.build_profiles[0].optimize, BuildOptimize::Full);
        assert_eq!(m.build_profiles[1].name, "debug");
        assert!(m.build_profiles[1].debug_info);
        assert_eq!(m.build_profiles[2].panic, Some(BuildPanic::Abort));
        assert_eq!(m.build_profiles[3].features, vec!["fast_path".to_string()]);
    }

    #[test]
    fn build_profile_duplicate_name_errors() {
        let src = r#"
payload: { name: "p", version: "0.1.0" }
build: {
    fast: Build.{ optimize: full },
    fast: Build.{ optimize: basic },
}
"#;
        let err = parse(src).unwrap_err();
        assert!(
            matches!(err, ManifestError::BadBuildProfile { ref name, ref reason }
                if name == "fast" && reason.contains("duplicate")),
            "{err:?}"
        );
    }

    #[test]
    fn build_profile_unknown_field_errors() {
        let src = r#"
payload: { name: "p", version: "0.1.0" }
build: { fast: Build.{ optimize: full, bogus: true } }
"#;
        let err = parse(src).unwrap_err();
        assert!(
            matches!(err, ManifestError::BadBuildProfile { ref name, .. } if name == "fast"),
            "{err:?}"
        );
    }

    #[test]
    fn package_build_entry_source_masks_manifest_but_keeps_jet_items() {
        let src = r#"
payload: { name: "p", version: "0.1.0" }
build: { allow: #(FS) }
fn helper() => String { return "ok" }
fn build(b: BuildContext) => BuildPlan ? {
    return b.plan()
}
"#;
        let source = build_entry_source(src).expect("package build entry should be found");
        assert!(source.contains("fn helper"));
        assert!(source.contains("fn build"));
        assert!(!source.contains("payload:"));
        assert!(!source.contains("allow: #(FS)"));
        assert_eq!(source.len(), src.len());
    }

    #[test]
    fn package_build_entry_source_ignores_manifest_text_and_comments() {
        let src = r#"
payload: { name: "p", version: "0.1.0", description: "fn build()" }
// fn build() in a comment is not an entry.
"#;
        assert!(build_entry_source(src).is_none());
    }

    #[test]
    fn package_build_entry_source_handles_nested_comments_and_utf8() {
        let src = r#"/* π /* fn build() is still a comment */ */
payload: { name: "p", version: "0.1.0" }
fn build(b: BuildContext) => BuildPlan ? { return b.plan() }
"#;
        let source = build_entry_source(src).expect("real build entry should survive comments");
        assert!(source.contains("fn build"));
        assert!(!source.contains("payload:"));
    }

    #[test]
    fn provider_policy_parses_explicit_mirror_allow_and_deny() {
        let manifest = parse(r#"
payload: { name: "p", version: "0.1.0" }
policy: {
    providers: {
        ruby: {
            registry: "https://mirror.example.test",
            allow: ["mirror.example.test", "dist.example.test"],
            deny: ["blocked.example.test"],
        },
    },
}
"#).unwrap();
        assert_eq!(manifest.provider_policy, vec![ProviderAuthority {
            provider: "ruby".into(),
            registry: "https://mirror.example.test".into(),
            allow: vec!["mirror.example.test".into(), "dist.example.test".into()],
            deny: vec!["blocked.example.test".into()],
        }]);
        for malformed in [
            r#"payload: { name: "p", version: "1" } policy: { providers: { ruby: { allow: ["x"] } } }"#,
            r#"payload: { name: "p", version: "1" } policy: { providers: { ruby: { registry: mirror.example.test } } }"#,
            r#"payload: { name: "p", version: "1" } policy: { providers: { ruby: { registry: "https://a", registry: "https://b" } } }"#,
        ] {
            assert!(matches!(parse(malformed), Err(ManifestError::BadProviderPolicy { .. })));
        }
    }
}
