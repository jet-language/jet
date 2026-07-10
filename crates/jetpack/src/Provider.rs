//! Provider translation layer (D-JPK5).
//!
//! Jetpack owns the package lifecycle. Nix is a *compatibility provider*: we
//! translate a Jetpack ref into a flake ref, ask Nix to realize it, parse the
//! store path it prints, and turn that into a `bin` directory for PATH. The
//! native Jetpack builder can later sit beside this same `Realized` boundary.
//!
//! Determinism for tests: when a fixtures dir is supplied (the `--offline`
//! path, or `JETPACK_FIXTURES`), we read a canned `nix build --json` file
//! instead of shelling out — exactly the Forge fixture pattern.

use super::ModuleEval::{AdapterPlan, AdapterRecipe};
use super::PackageManifest;
use super::Recipe::{self, BuildContext, BuildRecipe, BuildStep};
use super::RefSpec::{ProviderKind, RefSpec, Source, SourceTable};
use super::JSON;
use crate::SHA256;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A realized package: where its bytes are and what to put on PATH. `bin` is
/// the directory to prepend to PATH, or **empty** for a `library` package (U10),
/// which is staged for import and contributes nothing to PATH.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Realized {
    pub name: String,
    /// Package version for the hangar id (`<name>-<version>-<fp>`, D-PM1), or
    /// empty when the provider can't determine one (Phase-1 nix refs often).
    pub version: String,
    pub reference: String,
    pub out: String,
    pub bin: String,
    /// Path to the built Rust rlib artifact (D-BFS1). Set when the core provider
    /// compiles a library package that carries a `Cargo.toml`. Empty otherwise.
    pub rlib: String,
    /// D-JPK-CACHE1=A: the A4 envelope for this realized output — output hash,
    /// platform, signature slot, provenance. Makes the object cache-substitutable.
    pub envelope: super::Envelope::Envelope,
    pub cache_identity: super::Store::CacheIdentity,
    /// D-JPK-CACHE1 reporting (T4): how this realization was satisfied.
    pub source_state: SourceState,
}

fn cache_identity(source: &str, recipe: &str, ctx: &Ctx) -> super::Store::CacheIdentity {
    super::Store::CacheIdentity {
        source_fingerprint: source.to_string(),
        recipe_fingerprint: SHA256::sha256_hex(recipe.as_bytes()),
        policy_fingerprint: super::RuntimePolicy::cache_policy_fingerprint(ctx.offline),
        platform: super::Envelope::host_platform(),
    }
}

fn core_recipe_identity(src_dir: &Path, kind: PackageManifest::PackageKind) -> String {
    if kind == PackageManifest::PackageKind::Library && src_dir.join("Cargo.toml").is_file() {
        let toolchain = super::Toolchain::Toolchain::resolve();
        format!(
            "core-cargo-rlib-v1:{}:{}:{}",
            toolchain.as_ref().map_or("missing", |tc| tc.id.as_str()),
            toolchain
                .as_ref()
                .map_or("", |tc| tc.version.as_str()),
            toolchain
                .as_ref()
                .map_or_else(String::new, |tc| tc.cargo.to_string_lossy().into_owned())
        )
    } else {
        "core-source-v1".to_string()
    }
}

/// Independently derive every fact required to trust an existing cache record.
/// A provider that cannot derive exact current source/recipe identity gets no
/// early cache path.
pub fn cache_expectation(
    spec: &RefSpec,
    table: &SourceTable,
    ctx: &Ctx,
) -> Option<super::Store::CacheExpectation> {
    match resolve_kind(spec, table, ctx.offline, ctx.store_dir) {
        ProviderKind::Core => {
            let upstream = table.upstream(spec.source.label())?;
            let repo = source_repo(upstream, &spec.package, ctx).ok()?;
            let src_dir = PackageManifest::discover_module_in(&repo, &spec.package).ok()?;
            let source_fingerprint =
                super::Envelope::try_output_hash_of(&src_dir.to_string_lossy()).ok()?;
            let manifest = PackageManifest::PackManifest::load(&repo).and_then(Result::ok);
            let kind = manifest
                .as_ref()
                .and_then(|manifest| manifest.package_kind(&spec.package))
                .unwrap_or_else(|| infer_package_kind(&src_dir));
            let recipe = core_recipe_identity(&src_dir, kind);
            let fp = tree_fingerprint(&src_dir);
            Some(super::Store::CacheExpectation {
                identity: cache_identity(&source_fingerprint, &recipe, ctx),
                owned_output: Some(
                    ctx.store_dir
                        .join(format!("{}-{}", spec.package, &fp[..12])),
                ),
                allow_unsigned_local: true,
            })
        }
        ProviderKind::Nix | ProviderKind::Infer => Some(super::Store::CacheExpectation {
            identity: cache_identity(
                &SHA256::sha256_hex(spec.raw.as_bytes()),
                "nix-compat-v1",
                ctx,
            ),
            owned_output: None,
            allow_unsigned_local: false,
        }),
    }
}

/// Derive the adapter cache identity without trusting an existing output.
/// Staging reads the declared source; the output path follows only from those
/// bytes plus the normalized recipe.
pub fn adapter_cache_expectation(
    plan: &AdapterPlan,
    ctx: &Ctx,
) -> Result<super::Store::CacheExpectation, ProviderError> {
    let source_ref = super::RefSpec::classify_provider_ref(&plan.source).map_err(|_| {
        ProviderError::Adapter(format!(
            "adapter source `{}` is not a provider ref",
            plan.source
        ))
    })?;
    let staged = stage_adapter_source(&source_ref, ctx)?;
    let recipe_hash = adapter_recipe_to_build(&plan.recipe).recipe_hash();
    let source_hash = tree_fingerprint(&staged);
    let id_input = format!(
        "u20-adapter-v1\nname={}\nsource={}\nsource_hash={}\nrecipe={}\n",
        plan.name, plan.source, source_hash, recipe_hash
    );
    let fp = SHA256::sha256_hex(id_input.as_bytes());
    Ok(super::Store::CacheExpectation {
        identity: cache_identity(
            &super::Envelope::try_output_hash_of(&staged.to_string_lossy())
                .map_err(ProviderError::Adapter)?,
            &format!("adapter-v1:{recipe_hash}"),
            ctx,
        ),
        owned_output: Some(
            ctx.store_dir
                .join(format!("{}-adapter-{}", plan.name, &fp[..12])),
        ),
        allow_unsigned_local: true,
    })
}

/// How a dependency was realized, for the `jet build` per-package report
/// (`built | substituted | cached`, mirroring the D-JPK-CACHE1 example output).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceState {
    /// Compiled from source by the first-party core provider this run.
    Built,
    /// Reused an already-realized, content-addressed object (no rebuild).
    Cached,
    /// Realized through the Nix compatibility provider (substituted, not built
    /// from source by Jetpack).
    Substituted,
}

impl SourceState {
    pub fn label(self) -> &'static str {
        match self {
            SourceState::Built => "built",
            SourceState::Cached => "cached",
            SourceState::Substituted => "substituted",
        }
    }
}

/// What can go wrong realizing a ref through a provider. Each maps to a
/// friendly diagnostic (see `report`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    /// The `nix` binary isn't installed / on PATH and this source needs it.
    NixMissing,
    /// `nix build` ran but failed; carries a trimmed reason.
    BuildFailed(String),
    /// The provider's JSON didn't have the shape we expected.
    BadOutput(String),
    /// Offline/fixture mode but no fixture file for this ref.
    FixtureMissing(PathBuf),
    /// The selected provider can't realize this ref yet.
    Unsupported(String),
    /// The first-party `core` builder could not realize the package.
    CoreBuild(String),
    /// E1232 (D-MONOREF1): a monorepo source could not be fetched — the sparse
    /// subtree checkout and the full-clone fallback both failed.
    MonorepoFetch(String),
    /// E1233 (D-MONOREF1): an in-repo transitive dependency names a package that
    /// is not a member of the source repo's workspace index.
    MemberOutsideWorkspace(String),
    /// E1270: an adapter source/recipe cannot be realized by the native adapter
    /// path.
    Adapter(String),
    /// E1273: a recipe-backed package failed while running a logged build step.
    BuildDebug(String),
    /// E1271: a source channel cannot be resolved or is unlocked in a context
    /// that may not resolve it.
    Channel(String),
    /// E1276: `--offline` forbids a network fetch or metadata refresh.
    Offline(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NixBridgeNeed {
    pub reference: String,
    pub package: String,
}

impl ProviderError {
    /// The registered diagnostic code, for the errors that carry one.
    pub fn code(&self) -> Option<&'static str> {
        match self {
            ProviderError::MonorepoFetch(_) => Some("E1232"),
            ProviderError::MemberOutsideWorkspace(_) => Some("E1233"),
            ProviderError::Adapter(_) => Some("E1270"),
            ProviderError::Channel(_) => Some("E1271"),
            ProviderError::BuildDebug(_) => Some("E1273"),
            ProviderError::Offline(_) => Some("E1276"),
            _ => None,
        }
    }
}

/// What a provider needs to realize a ref, beyond the ref and source table:
/// the offline fixtures dir (nix) and the Jetpack store dir to materialize into
/// (core). Bundled so the `Provider` trait stays stable as providers grow.
pub struct Ctx<'a> {
    pub fixtures: Option<&'a Path>,
    pub store_dir: &'a Path,
    pub offline: bool,
}

/// Translate a Jetpack ref into the provider's flake ref. Users never type
/// `#`; this is the single place `:` becomes the Nix selector. A named source
/// (D-JPK17) resolves through `table` to its upstream/pin, then selects the
/// package as a flake attr: `<upstream>#<package>`.
pub fn flake_ref(spec: &RefSpec, table: &SourceTable) -> String {
    match &spec.source {
        Source::Nixpkgs => format!("nixpkgs#{}", spec.package),
        Source::Github => format!("github:{}", spec.package),
        Source::Path => format!("path:{}", spec.package),
        Source::Named(name) => {
            let upstream = table.upstream(name).unwrap_or(name);
            format!("{upstream}#{}", spec.package)
        }
    }
}

/// The fixture filename for a ref, e.g. `nixpkgs-fastfetch.json`.
pub fn fixture_name(spec: &RefSpec) -> String {
    let pkg = spec.package.replace('/', "_");
    format!("{}-{}.json", spec.source.label(), pkg)
}

/// Resolve the fixtures dir from an explicit flag or `JETPACK_FIXTURES`.
pub fn fixtures_from_env(explicit: Option<PathBuf>) -> Option<PathBuf> {
    explicit.or_else(|| std::env::var_os("JETPACK_FIXTURES").map(PathBuf::from))
}

/// Whether the `nix` binary is reachable on PATH (U16). Used by the two call
/// sites that shell out to `nix` for something other than a package ref —
/// `jet env`'s foreign-flake/devenv fallback and `jet bridge flake` — so both
/// fail with a clean E1256 up front instead of a raw spawn error partway
/// through.
pub fn nix_on_path() -> bool {
    Command::new("nix")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ──────────────────────────────────────────────
// Provider boundary (R0; see docs/plans/epoch-5/unified-ecosystem.md).
//
// The first-party core resolver owns realization; providers are extensions
// behind one trait. `core` realizes first-party Jet packages (no Nix); `nix`
// leverages nixpkgs. Today every built-in source routes to `nix`; source-aware
// dispatch (named sources picking `core` vs `nix`) is R1, gated on D-JPK16/17.
// ──────────────────────────────────────────────

/// A backend that realizes a ref into bytes + a `bin` dir. Both the first-party
/// `core` provider and the `nix` compatibility provider implement this.
pub trait Provider {
    /// Short stable name, used in diagnostics/listings (`core`, `nix`).
    fn name(&self) -> &'static str;
    /// Realize `spec`. `table` resolves named sources; `ctx` carries the
    /// offline fixtures dir and the store dir to materialize into.
    fn realize(
        &self,
        spec: &RefSpec,
        table: &SourceTable,
        ctx: &Ctx,
    ) -> Result<Realized, ProviderError>;
}

/// The Nix compatibility provider: translates a ref to a flake ref and shells
/// out to `nix build --no-link --json` (R3 will remove the installed-`nix`
/// requirement; the boundary here does not change).
pub struct NixProvider;

impl Provider for NixProvider {
    fn name(&self) -> &'static str {
        "nix"
    }
    fn realize(
        &self,
        spec: &RefSpec,
        table: &SourceTable,
        ctx: &Ctx,
    ) -> Result<Realized, ProviderError> {
        let stdout = match ctx.fixtures {
            Some(dir) => {
                let path = dir.join(fixture_name(spec));
                std::fs::read_to_string(&path).map_err(|_| ProviderError::FixtureMissing(path))?
            }
            None if ctx.offline => {
                return Err(ProviderError::Offline(format!(
                    "`{}` is not in the hangar and --offline forbids fetching provider output",
                    spec.raw
                )))
            }
            None => run_nix(spec, table)?,
        };
        let mut realized = parse_realization(spec, &stdout)?;
        realized.cache_identity = cache_identity(
            &SHA256::sha256_hex(spec.raw.as_bytes()),
            "nix-compat-v1",
            ctx,
        );
        Ok(realized)
    }
}

/// The first-party Jet package provider (R2/U10). Realizes a Jet package with
/// no Nix at all: it discovers the package's `module <name>` in the source repo
/// (Chunk 3), reads the repo's `pkg.jet` `packages:` index for the package's
/// kind (Chunk 4), and materializes that source tree into the Jetpack store —
/// staging a `bin/` for an `executable`, source-only for a `library`. R2
/// supports local and git-backed remote source repos.
pub struct CoreProvider;

impl Provider for CoreProvider {
    fn name(&self) -> &'static str {
        "core"
    }
    fn realize(
        &self,
        spec: &RefSpec,
        table: &SourceTable,
        ctx: &Ctx,
    ) -> Result<Realized, ProviderError> {
        let source_name = spec.source.label();
        let upstream = table.upstream(source_name).ok_or_else(|| {
            ProviderError::CoreBuild(format!("source `{source_name}` has no upstream"))
        })?;
        let repo = source_repo(upstream, &spec.package, ctx)?;
        let src_dir =
            PackageManifest::discover_module_in(&repo, &spec.package).map_err(|e| match e {
                PackageManifest::DiscoveryError::NotFound { name } => {
                    ProviderError::CoreBuild(format!(
                        "source repo at {} has no `module {name}` — add a .{} file declaring it",
                        repo.display(),
                        crate::Syntax::FILE_EXT,
                    ))
                }
                PackageManifest::DiscoveryError::Ambiguous { name, paths } => {
                    let list = paths
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    ProviderError::CoreBuild(format!(
                        "source repo at {} has `module {name}` in multiple files: {list}",
                        repo.display(),
                    ))
                }
            })?;
        if !src_dir.is_dir() {
            return Err(ProviderError::CoreBuild(format!(
                "package source {} does not exist",
                src_dir.display()
            )));
        }
        // Content-address the materialized package so identical sources share a
        // store entry and changes get a fresh one.
        let fp = tree_fingerprint(&src_dir);
        let out_dir = ctx
            .store_dir
            .join(format!("{}-{}", spec.package, &fp[..12]));
        // Reuse is owned by Store verification. Reaching the provider with an
        // existing object means no verified record leased it.
        if out_dir.exists() {
            return Err(ProviderError::CoreBuild(format!(
                "unverified existing output {}; run `jet clean` before rebuilding",
                out_dir.display()
            )));
        }
        copy_tree(&src_dir, &out_dir)
            .map_err(|e| ProviderError::CoreBuild(format!("could not place package: {e}")))?;
        // U10 Chunk 4: the repo's `pkg.jet` `packages:` index decides what
        // goes on PATH. `executable` stages the prebuilt `bin/` (the devshell
        // case); `library` stages module source for import and contributes no
        // PATH entry (an empty `bin`). With no manifest entry — a bare `core`
        // source declared by marker, no `pkg.jet` — we default to
        // `executable`, today's behavior.
        let manifest = PackageManifest::PackManifest::load(&repo).and_then(|r| r.ok());
        // D-ILE1: `kind` is inferred when `pkg.jet` omits it (or there is no
        // `pkg.jet`): a top-level `fn run` in the package source means
        // executable, otherwise library. An explicit `library`/`executable`
        // always wins.
        let kind = manifest
            .as_ref()
            .and_then(|pm| pm.package_kind(&spec.package))
            .unwrap_or_else(|| infer_package_kind(&out_dir));
        // `pkg.jet` carries the real version for core packages (U10).
        let version = manifest
            .as_ref()
            .map(|pm| pm.package.version.clone())
            .unwrap_or_default();
        let (bin, rlib, recipe_id) = match kind {
            PackageManifest::PackageKind::Executable => (
                out_dir.join("bin").to_string_lossy().into_owned(),
                String::new(),
                "core-source",
            ),
            PackageManifest::PackageKind::Library => {
                // D-BFS1: if the package ships a Cargo.toml, compile it to an
                // rlib now. The rlib lands *inside* the hangar object (`out_dir`)
                // so the object is self-contained and content-addressed; the
                // cargo target dir is a hangar-scoped scratch swept after the
                // build (D-JPK-GC1: build scratch is hangar-scoped, swept on
                // crash), never a sibling of the store root.
                let cargo_toml = out_dir.join("Cargo.toml");
                if cargo_toml.is_file() {
                    // D-JPK-BUILDTOOL1=A: compile through the pinned/realized Rust
                    // toolchain (a fixture stands in for #179's hangar object),
                    // never a bare host-`cargo` lookup. `resolve` falls back to the
                    // host dev toolchain when no pin is configured; only a machine
                    // with neither a toolchain object nor `cargo` yields `None`,
                    // the E1240 case (surfaced by the build reporting layer).
                    let rlib = super::Toolchain::Toolchain::resolve()
                        .and_then(|tc| build_rlib_from_cargo(&out_dir, ctx.store_dir, &tc))
                        .unwrap_or_default();
                    (String::new(), rlib, "core-cargo-rlib")
                } else {
                    (String::new(), String::new(), "core-source")
                }
            }
        };
        let out = out_dir.to_string_lossy().into_owned();
        let envelope = super::Envelope::Envelope::for_output(&out, &spec.raw, recipe_id);
        let source_fingerprint = super::Envelope::try_output_hash_of(&src_dir.to_string_lossy())
            .map_err(ProviderError::CoreBuild)?;
        let recipe_identity = core_recipe_identity(&src_dir, kind);
        Ok(Realized {
            name: spec.package.clone(),
            version,
            reference: spec.raw.clone(),
            out,
            bin,
            rlib,
            envelope,
            cache_identity: cache_identity(&source_fingerprint, &recipe_identity, ctx),
            source_state: SourceState::Built,
        })
    }
}

/// The hangar-scoped subdir that holds transient build scratch (cargo target
/// dirs). D-JPK-GC1: build scratch is hangar-scoped and swept on crash, never a
/// sibling of the store root.
pub const BUILD_SCRATCH_DIR: &str = "build-scratch";
pub const ACTIVE_TMP_MARKER: &str = ".active";

/// Remove every transient build-scratch dir under the hangar. Idempotent; used
/// to sweep scratch left behind by a crashed build (D-JPK-GC1). Returns the
/// number of scratch entries removed.
pub fn sweep_build_scratch(hangar_dir: &Path) -> usize {
    let root = hangar_dir.join(BUILD_SCRATCH_DIR);
    let mut removed = 0;
    if let Ok(rd) = std::fs::read_dir(&root) {
        for ent in rd.flatten() {
            if ent.path().join(ACTIVE_TMP_MARKER).exists() {
                continue;
            }
            if std::fs::remove_dir_all(ent.path()).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}

/// A hangar-scoped scratch dir that removes itself on drop — so a panic or an
/// early return between build start and finish never leaks a cargo target dir
/// into the hangar (D-JPK-GC1 crash-clean).
struct BuildScratch {
    path: PathBuf,
}

impl BuildScratch {
    fn new(hangar_dir: &Path, key: &str) -> BuildScratch {
        let path = hangar_dir.join(BUILD_SCRATCH_DIR).join(key);
        let _ = std::fs::remove_dir_all(&path);
        let _ = std::fs::create_dir_all(&path);
        let _ = std::fs::write(path.join(ACTIVE_TMP_MARKER), b"");
        BuildScratch { path }
    }
}

impl Drop for BuildScratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// D-BFS1: compile a library package's `Cargo.toml` to an rlib artifact.
///
/// The rlib is placed *inside* the hangar object (`pkg_dir`, the object root) so
/// the object is self-contained and content-addressed. The cargo target dir is
/// a hangar-scoped scratch (`<hangar>/build-scratch/<key>`) swept immediately
/// after the build and on crash (D-JPK-GC1). A prior realize of the same
/// content-addressed object leaves the rlib in place, so the rebuild is skipped
/// (cache hit). Returns the absolute path to the rlib inside the object, or an
/// empty string if the build is unavailable or fails.
///
/// `toolchain` is the resolved pinned/realized build toolchain
/// (D-JPK-BUILDTOOL1=A): the build execs *its* `cargo`, so a bridge's output
/// hash does not depend on whatever host `cargo` happens to be on PATH when the
/// toolchain is a pinned object.
pub(crate) fn build_rlib_from_cargo(
    pkg_dir: &Path,
    hangar_dir: &Path,
    toolchain: &super::Toolchain::Toolchain,
) -> Option<String> {
    // Cache hit: a previously realized object already carries its rlib.
    if let Some(existing) = find_rlib_in(pkg_dir) {
        return Some(existing);
    }
    let cache_key = pkg_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "pkg".to_string());
    let scratch = BuildScratch::new(hangar_dir, &cache_key);
    let out = Command::new(&toolchain.cargo)
        .arg("build")
        .arg("--lib")
        .arg("--release")
        .arg("--manifest-path")
        .arg(pkg_dir.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", &scratch.path)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    // Find the rlib in the scratch `release/` dir and copy it into the object.
    let release = scratch.path.join("release");
    let built = std::fs::read_dir(&release)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.extension().and_then(|e| e.to_str()) == Some("rlib")
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("lib"))
                    .unwrap_or(false)
        })?;
    let dest = pkg_dir.join(built.file_name()?);
    std::fs::copy(&built, &dest).ok()?;
    Some(dest.to_string_lossy().into_owned())
    // `scratch` drops here → the cargo target dir is swept.
}

/// Find a `lib*.rlib` already sitting in an object root (a cache hit).
fn find_rlib_in(pkg_dir: &Path) -> Option<String> {
    std::fs::read_dir(pkg_dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.extension().and_then(|e| e.to_str()) == Some("rlib")
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("lib"))
                    .unwrap_or(false)
        })
        .map(|p| p.to_string_lossy().into_owned())
}

/// D-ILE1: infer a package's kind from its source. A top-level `fn run` in any
/// of the package's `.jet` files means `executable`; otherwise `library`. The
/// source is lexed (not string-matched) so `fn run` inside a comment or string
/// literal never produces a false positive.
fn infer_package_kind(dir: &Path) -> PackageManifest::PackageKind {
    // A staged, non-empty `bin/` is the realized-package convention for "installs
    // on PATH" — executable, regardless of source shape.
    let has_bin = std::fs::read_dir(dir.join("bin"))
        .map(|mut d| d.next().is_some())
        .unwrap_or(false);
    if has_bin || dir_has_top_level_run(dir) {
        PackageManifest::PackageKind::Executable
    } else {
        PackageManifest::PackageKind::Library
    }
}

fn dir_has_top_level_run(dir: &Path) -> bool {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || matches!(name.as_ref(), "build" | "target" | "bin") {
            continue;
        }
        if path.is_dir() {
            if dir_has_top_level_run(&path) {
                return true;
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some(crate::Syntax::FILE_EXT) {
            if let Ok(src) = std::fs::read_to_string(&path) {
                if file_has_top_level_run(&src) {
                    return true;
                }
            }
        }
    }
    false
}

/// True when `src` declares a top-level `fn run` (brace depth 0).
fn file_has_top_level_run(src: &str) -> bool {
    use crate::Lexer::TokKind;
    let (toks, _diags) = crate::Lexer::lex(src);
    let mut depth: i32 = 0;
    for i in 0..toks.len() {
        match &toks[i].kind {
            TokKind::LBrace => depth += 1,
            TokKind::RBrace => depth -= 1,
            TokKind::KwFn if depth == 0 => {
                if matches!(toks.get(i + 1).map(|t| &t.kind), Some(TokKind::Ident(n)) if n == "run")
                {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// Resolve a `core` upstream to a local checkout. `path:` sources are used in
/// place; `github:` and git URLs are fetched into a Jetpack source cache.
///
/// `want_package` is the package being realized. When the remote is a monorepo
/// (the package lives in a subdirectory with its own `pkg.jet`), resolution is
/// index-first: only that member's subtree — plus its in-repo dependencies — is
/// materialized via a sparse checkout, never the whole repo (Slice C, D-MONOREF1).
fn source_repo(upstream: &str, want_package: &str, ctx: &Ctx) -> Result<PathBuf, ProviderError> {
    if let Some(p) = upstream.strip_prefix("path:") {
        let path = PathBuf::from(p);
        let path = if path.is_absolute() {
            path
        } else {
            std::env::current_dir().unwrap_or_default().join(path)
        };
        return Ok(path);
    }

    let remote = parse_remote_source(upstream)?;
    fetch_remote_repo_indexed(&remote, want_package, ctx)
}

/// Index-first remote resolution (Slice C). A cached checkout wins; otherwise try
/// a sparse member-subtree fetch, and fall back to a full clone when the source
/// is not a monorepo or the provider can't do a partial/sparse checkout. When a
/// monorepo's sparse fetch fails *and* the full-clone fallback also fails, that
/// is E1232; a transitive in-repo dependency outside the workspace is E1233.
fn fetch_remote_repo_indexed(
    remote: &RemoteSource,
    want_package: &str,
    ctx: &Ctx,
) -> Result<PathBuf, ProviderError> {
    let cache = source_cache_dir(ctx.store_dir, remote);
    if cache.is_dir() {
        return Ok(cache);
    }
    if ctx.offline {
        return Err(ProviderError::Offline(format!(
            "`{}` has no cached checkout and --offline forbids fetching source",
            remote.label
        )));
    }
    ensure_network_allowed("fetch source repo")?;

    match try_sparse_member_fetch(remote, want_package, &cache) {
        // Only the addressed member's subtree was materialized.
        SparseOutcome::Materialized(repo) => Ok(repo),
        // Not a monorepo (or the package isn't a subtree member): full clone.
        SparseOutcome::NotMonorepo => fetch_remote_repo(remote, ctx),
        // A transitive in-repo dep points outside the workspace index: hard error.
        SparseOutcome::DepOutside(err) => Err(err),
        // Monorepo detected but the sparse mechanics failed: fall back to a full
        // clone; if that also fails the source is unreachable → E1232.
        SparseOutcome::SparseFailed => fetch_remote_repo(remote, ctx).map_err(|_| {
            ProviderError::MonorepoFetch(format!(
                "sparse subtree checkout and full-clone fallback both failed for `{}`",
                remote.label
            ))
        }),
    }
}

/// The result of attempting an index-first sparse member fetch.
enum SparseOutcome {
    /// The member subtree (+ in-repo deps) was checked out at this path.
    Materialized(PathBuf),
    /// The source is not a monorepo member layout — caller should full-clone.
    NotMonorepo,
    /// A transitive in-repo dep resolves inside the repo but is not a workspace
    /// member (E1233).
    DepOutside(ProviderError),
    /// A monorepo was detected but the sparse git mechanics failed.
    SparseFailed,
}

/// Fetch only the `want_package` member's subtree from a remote monorepo using a
/// partial clone (`--filter=blob:none`) + cone `git sparse-checkout`. Reads the
/// repo's object tree with `git ls-tree`/`git show` (no full checkout) to build
/// the member index, walks the member's `pkg.jet` for in-repo deps, then checks
/// out just those subtrees. This is the generalization of the peek-only
/// `remote_has_pack_jet` probe into a real materializing fetch.
fn try_sparse_member_fetch(
    remote: &RemoteSource,
    want_package: &str,
    cache: &Path,
) -> SparseOutcome {
    if Command::new("git").arg("--version").output().is_err() {
        // No git at all: let the full-clone path produce the "need git" error.
        return SparseOutcome::NotMonorepo;
    }
    let tmp = std::env::temp_dir().join(format!(
        "jetpack-sparse-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    if std::fs::create_dir_all(&tmp).is_err() {
        return SparseOutcome::SparseFailed;
    }
    let _guard = TmpDirGuard(tmp.clone());

    let git_ok = |args: &[&str]| -> bool {
        Command::new("git")
            .arg("-C")
            .arg(&tmp)
            .args(args)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    };
    let git_out = |args: &[&str]| -> Option<String> {
        let o = Command::new("git")
            .arg("-C")
            .arg(&tmp)
            .args(args)
            .output()
            .ok()?;
        if o.status.success() {
            Some(String::from_utf8_lossy(&o.stdout).into_owned())
        } else {
            None
        }
    };

    let rev = remote.rev.as_deref().unwrap_or("HEAD");
    if !(git_ok(&["init", "--quiet"]) && git_ok(&["remote", "add", "origin", &remote.url])) {
        return SparseOutcome::SparseFailed;
    }
    if !git_ok(&[
        "fetch",
        "--quiet",
        "--depth",
        "1",
        "--filter=blob:none",
        "origin",
        rev,
    ]) {
        return SparseOutcome::SparseFailed;
    }

    // List every tracked path (trees are present under blob:none; blobs are
    // lazily fetched only when `git show`/checkout touches them).
    let Some(listing) = git_out(&["ls-tree", "-r", "--name-only", "FETCH_HEAD"]) else {
        return SparseOutcome::SparseFailed;
    };
    let member_dirs = member_dirs_from_listing(&listing);
    // Map the requested package to a subtree member by directory basename.
    let Some(target) = member_dirs
        .iter()
        .find(|d| dir_basename(d) == want_package)
        .cloned()
    else {
        // The package is not a subtree member (single-package repo, or a
        // differently-shaped layout): not our monorepo fast path.
        return SparseOutcome::NotMonorepo;
    };

    // Walk the member's `pkg.jet` for in-repo dependencies, resolving each
    // against the member index. An in-repo path dep that names a directory in
    // the repo which is not a member is E1233.
    let mut wanted: Vec<String> = vec![target.clone()];
    let all_dirs = all_tree_dirs(&listing);
    if let Some(pkg_src) = git_out(&["show", &format!("FETCH_HEAD:{target}/pkg.jet")]) {
        if let Ok(manifest) = PackageManifest::parse(&pkg_src) {
            for dep in &manifest.deps {
                match classify_in_repo_dep(dep, &target, &member_dirs, &all_dirs) {
                    InRepoDep::Member(path) => {
                        if !wanted.contains(&path) {
                            wanted.push(path);
                        }
                    }
                    InRepoDep::OutsideWorkspace(path) => {
                        return SparseOutcome::DepOutside(ProviderError::MemberOutsideWorkspace(
                            format!(
                                "package `{want_package}` depends on in-repo `{path}`, which is \
                                 not a workspace member of `{}`",
                                remote.label
                            ),
                        ));
                    }
                    InRepoDep::External => {}
                }
            }
        }
    }

    // Materialize exactly the wanted subtrees (cone mode also keeps root files,
    // so the repo-root `pkg.jet`/`workspace.jet` are available for discovery).
    if !git_ok(&["sparse-checkout", "init", "--cone"]) {
        return SparseOutcome::SparseFailed;
    }
    let mut set_args: Vec<&str> = vec!["sparse-checkout", "set"];
    set_args.extend(wanted.iter().map(|s| s.as_str()));
    if !git_ok(&set_args) {
        return SparseOutcome::SparseFailed;
    }
    if !git_ok(&["checkout", "--quiet", "FETCH_HEAD"]) {
        return SparseOutcome::SparseFailed;
    }

    // Publish into the source cache. Rename can cross the temp/cache boundary; a
    // copy fallback covers a cross-filesystem rename failure.
    if let Some(parent) = cache.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::rename(&tmp, cache).is_err() {
        if copy_tree(&tmp, cache).is_err() {
            return SparseOutcome::SparseFailed;
        }
    }
    SparseOutcome::Materialized(cache.to_path_buf())
}

/// A temp dir removed on drop, so a sparse fetch that returns early never leaks.
struct TmpDirGuard(PathBuf);
impl Drop for TmpDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The directories that contain a `pkg.jet` (workspace members, `find()`
/// semantics), from a `git ls-tree -r --name-only` listing. Root-level `pkg.jet`
/// (the repo manifest) is not a member subtree.
fn member_dirs_from_listing(listing: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in listing.lines() {
        let line = line.trim();
        if let Some(dir) = line.strip_suffix("/pkg.jet") {
            if !dir.is_empty() && !out.contains(&dir.to_string()) {
                out.push(dir.to_string());
            }
        }
    }
    out.sort();
    out
}

/// Every directory that appears in the tree listing (for in-repo dep checks).
fn all_tree_dirs(listing: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in listing.lines() {
        let mut cur = std::path::Path::new(line.trim());
        while let Some(parent) = cur.parent() {
            let p = parent.to_string_lossy().to_string();
            if !p.is_empty() && !out.contains(&p) {
                out.push(p);
            }
            cur = parent;
        }
    }
    out
}

/// The last path segment of a `/`-separated member directory.
fn dir_basename(dir: &str) -> &str {
    dir.rsplit('/').next().unwrap_or(dir)
}

/// How an in-repo-shaped dependency resolves against the workspace member index.
enum InRepoDep {
    /// Resolves to a workspace member subtree at this path.
    Member(String),
    /// Resolves to a directory inside the repo that is not a member (E1233).
    OutsideWorkspace(String),
    /// Not an in-repo dependency (registry/git/nixpkgs/clib/external path).
    External,
}

/// Classify a member's dependency for sparse-subtree scoping. Only `path@…`
/// deps that resolve *inside the repo* are relevant; everything else is fetched
/// by its own provider and does not widen the sparse checkout.
fn classify_in_repo_dep(
    dep: &PackageManifest::Dep,
    member_dir: &str,
    member_dirs: &[String],
    all_dirs: &[String],
) -> InRepoDep {
    // A bare dep alias that matches a member name is an in-repo dep.
    if let Some(m) = member_dirs.iter().find(|d| dir_basename(d) == dep.name) {
        return InRepoDep::Member(m.clone());
    }
    if let PackageManifest::DepSource::Provider {
        provider: Source::Path,
        target,
    } = &dep.source
    {
        let resolved = join_repo_relative(member_dir, target);
        if let Some(resolved) = resolved {
            if member_dirs.contains(&resolved) {
                return InRepoDep::Member(resolved);
            }
            if all_dirs.contains(&resolved) {
                // A real directory inside the repo, but not a package member.
                return InRepoDep::OutsideWorkspace(resolved);
            }
        }
    }
    InRepoDep::External
}

/// Resolve a `path@<target>` relative to a member directory, staying inside the
/// repo. Returns `None` when the path escapes the repo root (an external local
/// dep, not our concern for sparse scoping).
fn join_repo_relative(member_dir: &str, target: &str) -> Option<String> {
    let mut parts: Vec<&str> = member_dir.split('/').filter(|s| !s.is_empty()).collect();
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return None; // escaped above the repo root
                }
            }
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RemoteSource {
    url: String,
    rev: Option<String>,
    label: String,
}

fn parse_remote_source(upstream: &str) -> Result<RemoteSource, ProviderError> {
    let (base, rev) = split_ref(upstream);
    if let Some(rest) = base.strip_prefix("github:") {
        let mut parts = rest.split('/');
        let owner = parts.next().unwrap_or_default();
        let repo = parts.next().unwrap_or_default();
        if owner.is_empty() || repo.is_empty() {
            return Err(ProviderError::CoreBuild(format!(
                "`github:` sources need `github:owner/repo`, got `{upstream}`"
            )));
        }
        let path_rev = parts.collect::<Vec<_>>().join("/");
        let rev = rev.or_else(|| (!path_rev.is_empty()).then_some(path_rev));
        return Ok(RemoteSource {
            url: format!("https://github.com/{owner}/{repo}.git"),
            rev,
            label: format!("github:{owner}/{repo}"),
        });
    }

    if base.starts_with("git://")
        || base.starts_with("https://")
        || base.starts_with("ssh://")
        || base.starts_with("file://")
        || base.starts_with("git@")
    {
        return Ok(RemoteSource {
            url: base.to_string(),
            rev,
            label: base.to_string(),
        });
    }

    Err(ProviderError::CoreBuild(format!(
        "the `core` provider supports `path:`, `github:`, and git URL sources (got `{upstream}`)"
    )))
}

fn split_ref(upstream: &str) -> (&str, Option<String>) {
    match upstream.split_once('#') {
        Some((base, rev)) if !rev.is_empty() => (base, Some(rev.to_string())),
        Some((base, _)) => (base, None),
        None => (upstream, None),
    }
}

fn fetch_remote_repo(remote: &RemoteSource, ctx: &Ctx) -> Result<PathBuf, ProviderError> {
    let cache = source_cache_dir(ctx.store_dir, remote);
    if cache.is_dir() {
        return Ok(cache);
    }
    if ctx.offline {
        return Err(ProviderError::Offline(format!(
            "`{}` has no cached checkout and --offline forbids fetching source",
            remote.label
        )));
    }
    ensure_network_allowed("fetch source repo")?;
    if Command::new("git").arg("--version").output().is_err() {
        return Err(ProviderError::CoreBuild(
            "remote `core` sources need the `git` command to fetch source repos".to_string(),
        ));
    }

    let parent = cache.parent().unwrap_or(ctx.store_dir);
    std::fs::create_dir_all(parent)
        .map_err(|e| ProviderError::CoreBuild(format!("could not create source cache: {e}")))?;
    let tmp = parent.join(format!(
        ".tmp-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    if tmp.exists() {
        let _ = std::fs::remove_dir_all(&tmp);
    }

    let output = Command::new("git")
        .args(["clone", "--quiet", &remote.url])
        .arg(&tmp)
        .output()
        .map_err(|e| ProviderError::CoreBuild(format!("could not run `git clone`: {e}")))?;
    if !output.status.success() {
        let reason = String::from_utf8_lossy(&output.stderr)
            .trim()
            .lines()
            .last()
            .unwrap_or("git clone failed")
            .to_string();
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(ProviderError::CoreBuild(format!(
            "failed to fetch `{}`: {reason}",
            remote.label
        )));
    }

    if let Some(rev) = &remote.rev {
        let output = Command::new("git")
            .args(["-C"])
            .arg(&tmp)
            .args(["checkout", "--quiet", rev])
            .output()
            .map_err(|e| ProviderError::CoreBuild(format!("could not run `git checkout`: {e}")))?;
        if !output.status.success() {
            let reason = String::from_utf8_lossy(&output.stderr)
                .trim()
                .lines()
                .last()
                .unwrap_or("git checkout failed")
                .to_string();
            let _ = std::fs::remove_dir_all(&tmp);
            return Err(ProviderError::CoreBuild(format!(
                "failed to check out `{rev}` from `{}`: {reason}",
                remote.label
            )));
        }
    }

    std::fs::rename(&tmp, &cache).map_err(|e| {
        let _ = std::fs::remove_dir_all(&tmp);
        ProviderError::CoreBuild(format!("could not place fetched source in cache: {e}"))
    })?;
    Ok(cache)
}

fn source_cache_dir(store_dir: &Path, remote: &RemoteSource) -> PathBuf {
    let root = store_dir.parent().unwrap_or(store_dir).join("sources");
    let key = SHA256::sha256_hex(
        format!(
            "{}\n{}",
            remote.url,
            remote.rev.as_deref().unwrap_or("HEAD")
        )
        .as_bytes(),
    );
    root.join(&key[..16])
}

/// A content fingerprint over a whole directory tree: every file's relative
/// path, length, bytes, and (on Unix) mode, in sorted order. Unlike the
/// compiler's `.jet`-only `tree_hash`, this addresses *any* package tree, so
/// distinct packages never collide in the store.
fn tree_fingerprint(root: &Path) -> String {
    let mut files: Vec<PathBuf> = Vec::new();
    collect_files(root, &mut files);
    files.sort();
    let mut input: Vec<u8> = Vec::new();
    for path in &files {
        let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy();
        input.extend_from_slice(rel.as_bytes());
        input.push(0);
        if let Ok(bytes) = std::fs::read(path) {
            input.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
            input.extend_from_slice(&bytes);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = std::fs::metadata(path) {
                input.extend_from_slice(&meta.permissions().mode().to_be_bytes());
            }
        }
    }
    SHA256::sha256_hex(&input)
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_files(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// Recursively copy a directory tree, preserving Unix file modes (so `bin/`
/// executables stay executable). std-only (I6).
fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(&from)?.permissions().mode();
                std::fs::set_permissions(&to, std::fs::Permissions::from_mode(mode))?;
            }
        }
    }
    Ok(())
}

/// Pick the provider for an already-resolved kind. `Core` → the first-party
/// builder; everything else → the Nix compatibility provider.
pub fn provider_for(kind: ProviderKind) -> Box<dyn Provider> {
    match kind {
        ProviderKind::Core => Box::new(CoreProvider),
        _ => Box::new(NixProvider),
    }
}

/// Resolve a ref's concrete provider kind (`Nix`/`Core`), running the U9
/// realize-time probe when the source table left the kind to **inference**
/// (a typed `github@…` source). `offline`/`cache_dir` come from the realize
/// context: offline never hits the network — it reuses a cached checkout if
/// present, else falls back to `nix`.
///
/// Built-in sources and `path@…`/`nixpkgs@…` named sources are already concrete
/// in the table, so no probe runs for them.
pub fn resolve_kind(
    spec: &RefSpec,
    table: &SourceTable,
    offline: bool,
    cache_dir: &Path,
) -> ProviderKind {
    let Source::Named(name) = &spec.source else {
        return ProviderKind::Nix;
    };
    match table.provider(name) {
        ProviderKind::Core => ProviderKind::Core,
        ProviderKind::Nix => ProviderKind::Nix,
        // U9: peek the remote's `pkg.jet` to choose core vs nix.
        ProviderKind::Infer => match table.upstream(name) {
            Some(upstream) => infer_remote_kind(upstream, offline, cache_dir),
            None => ProviderKind::Nix,
        },
    }
}

/// True when realizing this ref goes through the Nix compatibility provider.
/// Resolves the kind first (so an inferred `github@…` source is probed).
pub fn uses_nix_provider(
    spec: &RefSpec,
    table: &SourceTable,
    offline: bool,
    cache_dir: &Path,
) -> bool {
    resolve_kind(spec, table, offline, cache_dir) != ProviderKind::Core
}

/// U23 / D-JPK-NONIX1=A: package refs that resolve through the Nix
/// compatibility provider need the Nix bridge unless a fixture is standing in
/// for that provider. This fact is computed before spawning `nix`, so no-Nix
/// machines get one package-focused diagnostic instead of a raw spawn error.
pub fn needs_nix_bridge(
    spec: &RefSpec,
    table: &SourceTable,
    offline: bool,
    cache_dir: &Path,
) -> Option<NixBridgeNeed> {
    if uses_nix_provider(spec, table, offline, cache_dir) {
        Some(NixBridgeNeed {
            reference: spec.raw.clone(),
            package: spec.short_name().to_string(),
        })
    } else {
        None
    }
}

/// Realize a ref through its provider. The resolver entry point: it never knows
/// or cares which backend runs — that is the whole point of the boundary.
pub fn realize(spec: &RefSpec, table: &SourceTable, ctx: &Ctx) -> Result<Realized, ProviderError> {
    let kind = resolve_kind(spec, table, ctx.offline, ctx.store_dir);
    provider_for(kind).realize(spec, table, ctx)
}

/// U20: realize an inline `Pkg.adapt(...)` plan into the same `Realized`
/// boundary as provider-backed packages.
pub fn realize_adapter(plan: &AdapterPlan, ctx: &Ctx) -> Result<Realized, ProviderError> {
    let source_ref = super::RefSpec::classify_provider_ref(&plan.source).map_err(|_| {
        ProviderError::Adapter(format!(
            "adapter source `{}` is not a provider ref",
            plan.source
        ))
    })?;
    let staged = stage_adapter_source(&source_ref, ctx)?;
    let recipe = adapter_recipe_to_build(&plan.recipe);
    let recipe_hash = recipe.recipe_hash();
    let source_hash = tree_fingerprint(&staged);
    let id_input = format!(
        "u20-adapter-v1\nname={}\nsource={}\nsource_hash={}\nrecipe={}\n",
        plan.name, plan.source, source_hash, recipe_hash
    );
    let fp = SHA256::sha256_hex(id_input.as_bytes());
    let out_dir = ctx
        .store_dir
        .join(format!("{}-adapter-{}", plan.name, &fp[..12]));
    if out_dir.exists() {
        return Err(ProviderError::Adapter(format!(
            "unverified existing output {}; run `jet clean` before rebuilding",
            out_dir.display()
        )));
    }
    let fetch_cache = ctx.store_dir.join("fetch-cache");
    let build_ctx = BuildContext {
        source_dir: &staged,
        output_root: &out_dir,
        tools: std::collections::HashMap::new(),
        fetch_cache: &fetch_cache,
        offline: ctx.offline,
    };
    let mut attempt = super::BuildDebug::Attempt::new(
        &plan.name,
        &format!("adapt:{}:{}", plan.name, plan.source),
        "adapter",
        &recipe_hash,
        &source_hash,
    );
    if let Err(d) = Recipe::run_logged(&recipe, &build_ctx, None, &mut attempt) {
        attempt.preserve_scratch(ctx.store_dir, &staged, &out_dir);
        let _ = attempt.persist(ctx.store_dir);
        return Err(ProviderError::BuildDebug(format!(
                "adapter `{}` failed at step {} of {}: {} — full log: `jet logs {}`; rerun with `--shell-on-fail` to debug inside {}",
                plan.name,
                attempt.failed_step,
                attempt.steps.len(),
                d.what,
                plan.name,
                attempt.scratch_dir
            )));
    }
    let _ = attempt.persist(ctx.store_dir);
    let out = out_dir.to_string_lossy().into_owned();
    let bin_dir = out_dir.join("bin");
    let bin = if bin_dir.is_dir() {
        bin_dir.to_string_lossy().into_owned()
    } else {
        String::new()
    };
    let envelope = super::Envelope::Envelope::for_output(
        &out,
        &format!("adapt:{}:{}", plan.name, plan.source),
        &format!("adapter:{recipe_hash}"),
    );
    let source_fingerprint = super::Envelope::try_output_hash_of(&staged.to_string_lossy())
        .map_err(ProviderError::Adapter)?;
    Ok(Realized {
        name: plan.name.clone(),
        version: String::new(),
        reference: format!("adapt:{}:{}", plan.name, plan.source),
        out,
        bin,
        rlib: String::new(),
        envelope,
        cache_identity: cache_identity(
            &source_fingerprint,
            &format!("adapter-v1:{recipe_hash}"),
            ctx,
        ),
        source_state: SourceState::Built,
    })
}

fn adapter_recipe_to_build(recipe: &AdapterRecipe) -> BuildRecipe {
    match recipe {
        AdapterRecipe::Copy => BuildRecipe {
            steps: vec![BuildStep::InstallTree {
                src: ".".to_string(),
                dest: ".".to_string(),
            }],
        },
        AdapterRecipe::Prebuilt { bin, as_name } => BuildRecipe {
            steps: vec![BuildStep::Install {
                src: bin.clone(),
                dest: format!("bin/{as_name}"),
            }],
        },
        AdapterRecipe::Build(recipe) => recipe.clone(),
    }
}

fn stage_adapter_source(
    source: &super::RefSpec::ProviderRef,
    ctx: &Ctx,
) -> Result<PathBuf, ProviderError> {
    match source.provider {
        Source::Path => {
            let (target, _) = super::RefSpec::split_channel_ref(&source.target);
            let path = PathBuf::from(target);
            let path = if path.is_absolute() {
                path
            } else {
                std::env::current_dir().unwrap_or_default().join(path)
            };
            if path.is_dir() {
                Ok(path)
            } else {
                Err(ProviderError::Adapter(format!(
                    "adapter source `{}` is not a directory",
                    path.display()
                )))
            }
        }
        Source::Github => {
            let remote = parse_remote_source(&format!("github:{}", source.target))?;
            fetch_remote_repo(&remote, ctx)
        }
        Source::Nixpkgs => Err(ProviderError::Adapter(
            "`nixpkgs@...` is an index source, not source bytes; use `jetpack add <ref> --adapt` to draft a concrete adapter.".to_string(),
        )),
        Source::Named(_) => Err(ProviderError::Adapter(
            "adapter source must be a built-in provider ref like `path@vendor/tool`.".to_string(),
        )),
    }
}

/// U9 remote probe: classify a `github@…`/git upstream as `Core` (it carries a
/// `pkg.jet`) or `Nix` (it does not), peeking **only** `pkg.jet` — never
/// cloning a nixpkgs-sized repo just to classify it.
///
/// Resolution order:
/// 1. If a source-cache checkout already exists (a prior realize fetched it),
///    classify from the local tree — offline-safe, no network.
/// 2. Offline with no cache: we can't probe, so default to `nix`.
/// 3. Online: a lightweight `git` peek — a partial, no-checkout, depth-1 clone
///    (`--filter=tree:0`, so blobs/subtrees are never downloaded) into a temp
///    dir, then `git ls-tree <rev> pkg.jet`. Present → `Core`; absent or any
///    peek failure → `Nix` (the safe default; a github flake still realizes
///    through nix).
fn infer_remote_kind(upstream: &str, offline: bool, cache_dir: &Path) -> ProviderKind {
    let Ok(remote) = parse_remote_source(upstream) else {
        return ProviderKind::Nix;
    };
    // (1) Reuse a prior fetch.
    let cache = source_cache_dir(cache_dir, &remote);
    if cache.is_dir() {
        return pack_kind(cache.join(crate::Syntax::PAYLOAD_FILE).is_file());
    }
    // (2) Offline can't reach the network; a remote we haven't cached stays nix.
    if offline {
        return ProviderKind::Nix;
    }
    // (3) Lightweight online peek.
    pack_kind(remote_has_pack_jet(&remote))
}

fn pack_kind(has_pack: bool) -> ProviderKind {
    if has_pack {
        ProviderKind::Core
    } else {
        ProviderKind::Nix
    }
}

/// Peek whether `remote` has a `pkg.jet` at its root, without a full clone.
///
/// Fetches **only the named rev** into a throwaway repo, shallow (`--depth 1`)
/// and partial (`--filter=tree:0`, so trees/blobs are deferred), then reads the
/// root tree with `git ls-tree FETCH_HEAD`. Even a nixpkgs-sized repo transfers
/// just the one commit object plus the lazily-fetched root tree. `git fetch`
/// resolves a branch, tag, **or** commit SHA uniformly, so the rev's exact
/// `pkg.jet` is peeked regardless of how it was pinned. Any failure (no `git`,
/// network error, unfetchable rev) is treated as "no pkg.jet" by the caller
/// (→ nix), the safe default.
fn remote_has_pack_jet(remote: &RemoteSource) -> bool {
    if network_denied() {
        return false;
    }
    if Command::new("git").arg("--version").output().is_err() {
        return false;
    }
    let tmp = std::env::temp_dir().join(format!(
        "jetpack-peek-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    if std::fs::create_dir_all(&tmp).is_err() {
        return false;
    }

    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(&tmp)
            .args(args)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    };
    // A configured `origin` makes the partial fetch register a promisor remote,
    // so the deferred root tree can be lazily fetched on `ls-tree`.
    let rev = remote.rev.as_deref().unwrap_or("HEAD");
    let set_up = git(&["init", "--quiet"]) && git(&["remote", "add", "origin", &remote.url]);
    let fetched = set_up
        && git(&[
            "fetch",
            "--quiet",
            "--depth",
            "1",
            "--filter=tree:0",
            "origin",
            rev,
        ]);
    let has_pack = fetched
        && Command::new("git")
            .arg("-C")
            .arg(&tmp)
            .args(["ls-tree", "FETCH_HEAD", crate::Syntax::PAYLOAD_FILE])
            .output()
            .map(|o| o.status.success() && !o.stdout.is_empty())
            .unwrap_or(false);

    let _ = std::fs::remove_dir_all(&tmp);
    has_pack
}

fn run_nix(spec: &RefSpec, table: &SourceTable) -> Result<String, ProviderError> {
    ensure_network_allowed("run nix provider")?;
    let output = Command::new("nix")
        .args(["build", "--no-link", "--json"])
        .arg(flake_ref(spec, table))
        .output();
    let output = match output {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(ProviderError::NixMissing)
        }
        Err(e) => return Err(ProviderError::BuildFailed(e.to_string())),
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let reason = stderr
            .trim()
            .lines()
            .last()
            .unwrap_or("nix build failed")
            .to_string();
        return Err(ProviderError::BuildFailed(reason));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn network_denied() -> bool {
    std::env::var_os("JETPACK_DENY_NETWORK").is_some_and(|v| !v.is_empty())
}

fn ensure_network_allowed(need: &str) -> Result<(), ProviderError> {
    if network_denied() {
        Err(ProviderError::Offline(format!(
            "network disabled by JETPACK_DENY_NETWORK while trying to {need}"
        )))
    } else {
        Ok(())
    }
}

/// Parse `nix build --json` output: an array of build results, each with an
/// `outputs` object. Prefer a `bin` output, else `out`.
fn parse_realization(spec: &RefSpec, stdout: &str) -> Result<Realized, ProviderError> {
    let json = JSON::parse(stdout.trim()).map_err(|e| ProviderError::BadOutput(e))?;
    let arr = json.as_array().map_err(ProviderError::BadOutput)?;
    let first = arr
        .first()
        .ok_or_else(|| ProviderError::BadOutput("provider produced no build results".into()))?;
    let outputs = first.get("outputs").map_err(ProviderError::BadOutput)?;
    let outputs = outputs.as_object().map_err(ProviderError::BadOutput)?;

    let out = outputs
        .get("bin")
        .or_else(|| outputs.get("out"))
        .and_then(|j| j.as_str().ok())
        .ok_or_else(|| {
            ProviderError::BadOutput("provider output had no `out`/`bin` store path".into())
        })?;

    let bin = format!("{}/bin", out.trim_end_matches('/'));
    let name = spec.short_name().to_string();
    let envelope = super::Envelope::Envelope::for_output(out, &spec.raw, "nix");
    Ok(Realized {
        version: nix_store_version(out, &name),
        name,
        reference: spec.raw.clone(),
        out: out.to_string(),
        bin,
        rlib: String::new(),
        envelope,
        cache_identity: super::Store::CacheIdentity::default(),
        source_state: SourceState::Substituted,
    })
}

/// Recover a package version from a Nix store path basename, which by
/// convention is `<32-char-hash>-<pname>-<version>[-<output>]`. We strip the
/// fixed-width hash, the known `<name>-` prefix, and any trailing output
/// segment, then accept the remainder only if it looks like a version (leads
/// with a digit). Anything we can't confidently parse yields an empty version,
/// so the hangar id falls back to `<name>-<fp>` rather than guessing wrong.
fn nix_store_version(out: &str, name: &str) -> String {
    const HASH_LEN: usize = 32;
    const OUTPUT_SUFFIXES: &[&str] = &["-bin", "-dev", "-lib", "-doc", "-man", "-info", "-out"];

    let base = out.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let Some(rest) = base.get(HASH_LEN..) else {
        return String::new();
    };
    let rest = rest.strip_prefix('-').unwrap_or(rest);
    let Some(mut version) = rest.strip_prefix(name).and_then(|s| s.strip_prefix('-')) else {
        return String::new();
    };
    for suffix in OUTPUT_SUFFIXES {
        if let Some(stripped) = version.strip_suffix(suffix) {
            version = stripped;
            break;
        }
    }
    if version.starts_with(|c: char| c.is_ascii_digit()) {
        version.to_string()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::super::RefSpec::{classify, classify_in};
    use super::*;

    fn empty() -> SourceTable {
        SourceTable::empty()
    }

    #[test]
    fn top_level_run_drives_kind_inference() {
        // D-ILE1: a top-level `fn run` means executable.
        assert!(file_has_top_level_run("fn run() {}\n"));
        assert!(file_has_top_level_run(
            "fn helper() -> Int { return 1; }\nfn run() { print(\"hi\"); }\n"
        ));
        // A `run` nested in a module/impl block is not the entry point.
        assert!(!file_has_top_level_run("module m { fn run() {} }\n"));
        // A library: no top-level `fn run`.
        assert!(!file_has_top_level_run(
            "fn add(a: Int, b: Int) -> Int { return a + b; }\n"
        ));
        // `fn run` inside a comment or string never counts.
        assert!(!file_has_top_level_run("// fn run()\nfn lib() {}\n"));
        assert!(!file_has_top_level_run(
            "fn lib() { let s = \"fn run()\"; }\n"
        ));
    }

    #[test]
    fn nix_store_version_parses_path_suffix() {
        let h = "0000000000000000000000000000000a"; // 32-char stand-in hash
                                                    // Plain `out` path: version is the trailing segment.
        assert_eq!(
            nix_store_version(&format!("/nix/store/{h}-fastfetch-2.1.0"), "fastfetch"),
            "2.1.0"
        );
        // Split `bin` output: the `-bin` suffix is stripped.
        assert_eq!(
            nix_store_version(&format!("/nix/store/{h}-ripgrep-14.1.0-bin"), "ripgrep"),
            "14.1.0"
        );
        // Hyphenated package names are honored by matching the known name.
        assert_eq!(
            nix_store_version(&format!("/nix/store/{h}-jq-lib-1.7.1"), "jq-lib"),
            "1.7.1"
        );
        // No recognizable version → empty, so the id falls back to `<name>-<fp>`.
        assert_eq!(
            nix_store_version(&format!("/nix/store/{h}-hello-unstable"), "hello"),
            ""
        );
        assert_eq!(nix_store_version("/some/local/path", "hello"), "");
    }

    #[test]
    fn translates_ref_to_flake() {
        assert_eq!(
            flake_ref(&classify("nixpkgs:fastfetch").unwrap(), &empty()),
            "nixpkgs#fastfetch"
        );
        assert_eq!(
            flake_ref(&classify("github:o/r").unwrap(), &empty()),
            "github:o/r"
        );
    }

    #[test]
    fn named_source_flake_ref_uses_pin() {
        let table = SourceTable::from_decls([(
            "stable".to_string(),
            "github:NixOS/nixpkgs/nixos-24.05".to_string(),
            super::super::RefSpec::ProviderKind::Nix,
        )]);
        let spec = classify_in("stable:ripgrep", &table).unwrap();
        assert_eq!(
            flake_ref(&spec, &table),
            "github:NixOS/nixpkgs/nixos-24.05#ripgrep"
        );
        // The fixture name keys off the source name, so `stable-ripgrep.json`.
        assert_eq!(fixture_name(&spec), "stable-ripgrep.json");
    }

    #[test]
    fn fixture_name_sanitizes_slashes() {
        let s = classify("github:halcyonomega/cfg").unwrap();
        assert_eq!(fixture_name(&s), "github-halcyonomega_cfg.json");
    }

    #[test]
    fn parses_good_output() {
        let spec = classify("nixpkgs:fastfetch").unwrap();
        let stdout = r#"[{"outputs":{"out":"/nix/store/abc-fastfetch-2.0"}}]"#;
        let r = parse_realization(&spec, stdout).unwrap();
        assert_eq!(r.out, "/nix/store/abc-fastfetch-2.0");
        assert_eq!(r.bin, "/nix/store/abc-fastfetch-2.0/bin");
        assert_eq!(r.name, "fastfetch");
    }

    #[test]
    fn prefers_bin_output() {
        let spec = classify("nixpkgs:git").unwrap();
        let stdout = r#"[{"outputs":{"out":"/nix/store/x","bin":"/nix/store/x-bin"}}]"#;
        let r = parse_realization(&spec, stdout).unwrap();
        assert_eq!(r.bin, "/nix/store/x-bin/bin");
    }

    #[test]
    fn empty_output_is_bad() {
        let spec = classify("nixpkgs:x").unwrap();
        assert!(matches!(
            parse_realization(&spec, "[]"),
            Err(ProviderError::BadOutput(_))
        ));
    }

    #[test]
    fn garbage_output_is_bad() {
        let spec = classify("nixpkgs:x").unwrap();
        assert!(matches!(
            parse_realization(&spec, "not json"),
            Err(ProviderError::BadOutput(_))
        ));
    }

    #[test]
    fn missing_outputs_key_is_bad() {
        let spec = classify("nixpkgs:x").unwrap();
        assert!(matches!(
            parse_realization(&spec, r#"[{"drvPath":"/x.drv"}]"#),
            Err(ProviderError::BadOutput(_))
        ));
    }

    #[test]
    fn fixture_missing_errors() {
        let spec = classify("nixpkgs:nope").unwrap();
        let dir = std::env::temp_dir();
        let ctx = Ctx {
            fixtures: Some(&dir.join("definitely-not-here-xyz")),
            store_dir: &dir,
            offline: false,
        };
        match realize(&spec, &empty(), &ctx) {
            Err(ProviderError::FixtureMissing(_)) => {}
            other => panic!("expected FixtureMissing, got {other:?}"),
        }
    }

    #[test]
    fn core_provider_builds_local_package() {
        use super::super::RefSpec::{classify_in, ProviderKind, SourceTable};
        // Repo with pkg.jet + a `module hello` declaration + bin/. No env.jet
        // (U10 Chunk 3: CoreProvider discovers the package by module name).
        let base = unique_dir("jpk-core");
        let repo = base.join("jet-pkgs");
        let store = base.join("store");
        let hello_pkg = repo.join("pkgs/hello");
        let hello_bin = hello_pkg.join("bin");
        std::fs::create_dir_all(&hello_bin).unwrap();
        std::fs::write(hello_pkg.join("hello.jet"), "module hello { }\n").unwrap();
        std::fs::write(hello_bin.join("hello"), "#!/bin/sh\necho hi\n").unwrap();
        std::fs::create_dir_all(&store).unwrap();

        let upstream = format!("path:{}", repo.to_string_lossy());
        let table = SourceTable::from_decls([("mine".to_string(), upstream, ProviderKind::Core)]);
        let spec = classify_in("mine:hello", &table).unwrap();
        let ctx = Ctx {
            fixtures: None,
            store_dir: &store,
            offline: false,
        };
        // Dispatch must select the core provider, and it must materialize the
        // tree into the store with a real bin dir — no nix involved.
        assert_eq!(
            resolve_kind(&spec, &table, false, &store),
            ProviderKind::Core
        );
        assert_eq!(provider_for(ProviderKind::Core).name(), "core");
        let r = realize(&spec, &table, &ctx).unwrap();
        assert_eq!(r.name, "hello");
        assert!(std::path::Path::new(&r.bin).join("hello").is_file());
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn core_provider_kind_decides_path_entry() {
        // U10 Chunk 4: the repo's `pkg.jet` `packages:` index decides what a
        // realized `core` package puts on PATH. `executable` → a `bin/` dir;
        // `library` → no bin (staged source only). Both stage the tree.
        use super::super::RefSpec::{classify_in, ProviderKind, SourceTable};
        let base = unique_dir("jpk-core-kind");
        let repo = base.join("jet-pkgs");
        let store = base.join("store");
        std::fs::create_dir_all(&store).unwrap();
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(
            repo.join("pkg.jet"),
            "payload: { name: \"p\", version: \"0.1.0\" }\npackages: { hello: executable, mathlib: library }\n",
        )
        .unwrap();
        // executable: has a prebuilt bin/.
        let hello_bin = repo.join("pkgs/hello/bin");
        std::fs::create_dir_all(&hello_bin).unwrap();
        std::fs::write(repo.join("pkgs/hello/hello.jet"), "module hello { }\n").unwrap();
        std::fs::write(hello_bin.join("hello"), "#!/bin/sh\necho hi\n").unwrap();
        // library: module source, no bin/.
        let lib = repo.join("lib/mathlib");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("mathlib.jet"), "module mathlib { }\n").unwrap();

        let upstream = format!("path:{}", repo.to_string_lossy());
        let table = SourceTable::from_decls([("mine".to_string(), upstream, ProviderKind::Core)]);
        let ctx = Ctx {
            fixtures: None,
            store_dir: &store,
            offline: false,
        };

        let exe = realize(&classify_in("mine:hello", &table).unwrap(), &table, &ctx).unwrap();
        assert!(
            !exe.bin.is_empty() && std::path::Path::new(&exe.bin).join("hello").is_file(),
            "executable must stage a bin/ on PATH: {exe:?}"
        );

        let lib = realize(&classify_in("mine:mathlib", &table).unwrap(), &table, &ctx).unwrap();
        assert!(
            lib.bin.is_empty(),
            "library must contribute no PATH entry: {lib:?}"
        );
        assert!(
            std::path::Path::new(&lib.out).join("mathlib.jet").is_file(),
            "library must stage its module source: {lib:?}"
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn core_provider_fetches_remote_git_package() {
        use super::super::RefSpec::{classify_in, ProviderKind, SourceTable};
        if std::process::Command::new("git")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("note: skipping remote core provider test (git not found)");
            return;
        }

        let base = unique_dir("jpk-core-remote");
        let repo = base.join("remote");
        let store = base.join("store");
        let hello_pkg = repo.join("pkgs/hello");
        let hello_bin = hello_pkg.join("bin");
        std::fs::create_dir_all(&hello_bin).unwrap();
        std::fs::create_dir_all(&store).unwrap();
        std::fs::write(hello_pkg.join("hello.jet"), "module hello { }\n").unwrap();
        std::fs::write(hello_bin.join("hello"), "#!/bin/sh\necho remote\n").unwrap();

        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&repo)
            .output()
            .unwrap();
        for (k, v) in [
            ("user.email", "jetpack@example.invalid"),
            ("user.name", "Jetpack Test"),
        ] {
            std::process::Command::new("git")
                .args(["config", k, v])
                .current_dir(&repo)
                .output()
                .unwrap();
        }
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(&repo)
            .output()
            .unwrap();
        let commit = std::process::Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(
            commit.status.success(),
            "git commit failed: {}",
            String::from_utf8_lossy(&commit.stderr)
        );

        let upstream = format!("file://{}#HEAD", repo.to_string_lossy());
        let table = SourceTable::from_decls([("mine".to_string(), upstream, ProviderKind::Core)]);
        let spec = classify_in("mine:hello", &table).unwrap();
        let ctx = Ctx {
            fixtures: None,
            store_dir: &store,
            offline: false,
        };

        let r = realize(&spec, &table, &ctx).unwrap();
        assert_eq!(r.name, "hello");
        assert!(std::path::Path::new(&r.bin).join("hello").is_file());
        std::fs::remove_dir_all(&base).ok();
    }

    /// Init a git repo at `dir` with the given files and one commit. Returns
    /// false (skip) if `git` isn't available.
    fn init_git_repo(dir: &Path, files: &[(&str, &str)]) -> bool {
        if Command::new("git").arg("--version").output().is_err() {
            return false;
        }
        for (rel, body) in files {
            let path = dir.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        let run = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .unwrap()
        };
        run(&["init", "--quiet"]);
        run(&["config", "user.email", "jetpack@example.invalid"]);
        run(&["config", "user.name", "Jetpack Test"]);
        run(&["add", "."]);
        let commit = run(&["commit", "--quiet", "-m", "init"]);
        assert!(
            commit.status.success(),
            "git commit failed: {}",
            String::from_utf8_lossy(&commit.stderr)
        );
        true
    }

    #[test]
    fn resolve_kind_probes_remote_pack_jet() {
        use super::super::RefSpec::{classify_in, ProviderKind, SourceTable};
        let base = unique_dir("jpk-probe");
        let store = base.join("store");
        std::fs::create_dir_all(&store).unwrap();

        // A repo carrying `pkg.jet` is a Jet package source → core.
        let with = base.join("with-pack");
        if !init_git_repo(
            &with,
            &[("pkg.jet", "payload: { name: \"p\", version: \"0.1.0\" }\n")],
        ) {
            eprintln!("note: skipping remote probe test (git not found)");
            return;
        }
        let with_table = SourceTable::from_decls([(
            "mine".to_string(),
            format!("file://{}", with.to_string_lossy()),
            ProviderKind::Infer,
        )]);
        let with_spec = classify_in("mine:hello", &with_table).unwrap();
        assert_eq!(
            resolve_kind(&with_spec, &with_table, false, &store),
            ProviderKind::Core,
            "a remote carrying pkg.jet must infer core"
        );

        // A repo with no `pkg.jet` is a plain (nix) flake/source → nix.
        let without = base.join("no-pack");
        init_git_repo(&without, &[("flake.nix", "{}\n")]);
        let without_table = SourceTable::from_decls([(
            "plain".to_string(),
            format!("file://{}", without.to_string_lossy()),
            ProviderKind::Infer,
        )]);
        let without_spec = classify_in("plain:fd", &without_table).unwrap();
        assert_eq!(
            resolve_kind(&without_spec, &without_table, false, &store),
            ProviderKind::Nix,
            "a remote with no pkg.jet must infer nix"
        );

        // Offline with no cached checkout can't probe → defaults to nix even for
        // the pkg.jet-bearing repo.
        let cold = base.join("cold-store");
        std::fs::create_dir_all(&cold).unwrap();
        assert_eq!(
            resolve_kind(&with_spec, &with_table, true, &cold),
            ProviderKind::Nix,
            "offline with no cache must not hit the network"
        );

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn remote_probe_resolves_a_commit_sha_rev() {
        // The uniform `git fetch <rev>` peek must resolve a source pinned to an
        // exact commit SHA the same as a branch/tag name (the case the earlier
        // `--branch`-only peek could not handle).
        use super::super::RefSpec::{classify_in, ProviderKind, SourceTable};
        let base = unique_dir("jpk-probe-sha");
        let store = base.join("store");
        std::fs::create_dir_all(&store).unwrap();
        let repo = base.join("repo");
        if !init_git_repo(
            &repo,
            &[("pkg.jet", "payload: { name: \"p\", version: \"0.1.0\" }\n")],
        ) {
            eprintln!("note: skipping commit-sha probe test (git not found)");
            return;
        }
        let sha = String::from_utf8(
            Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_string();
        let upstream = format!("file://{}#{}", repo.to_string_lossy(), sha);
        let table = SourceTable::from_decls([("mine".to_string(), upstream, ProviderKind::Infer)]);
        let spec = classify_in("mine:hello", &table).unwrap();
        assert_eq!(
            resolve_kind(&spec, &table, false, &store),
            ProviderKind::Core,
            "a commit-SHA-pinned remote with pkg.jet must infer core"
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn realize_resolves_inferred_remote_to_core() {
        // U9 end-to-end at the realize boundary: an `Infer` source — the kind a
        // typed `github@…` source carries — whose remote has a `pkg.jet`
        // resolves to the `core` provider and builds the first-party package,
        // with no nix and no declared marker.
        use super::super::RefSpec::{classify_in, ProviderKind, SourceTable};
        let base = unique_dir("jpk-infer-build");
        let repo = base.join("remote");
        let store = base.join("store");
        std::fs::create_dir_all(&store).unwrap();
        if !init_git_repo(
            &repo,
            &[
                ("pkg.jet", "payload: { name: \"p\", version: \"0.1.0\" }\n"),
                ("pkgs/hello/hello.jet", "module hello { }\n"),
                ("pkgs/hello/bin/hello", "#!/bin/sh\necho hi-infer\n"),
            ],
        ) {
            eprintln!("note: skipping inferred remote build test (git not found)");
            return;
        }
        let upstream = format!("file://{}", repo.to_string_lossy());
        let table = SourceTable::from_decls([("mine".to_string(), upstream, ProviderKind::Infer)]);
        let spec = classify_in("mine:hello", &table).unwrap();
        let ctx = Ctx {
            fixtures: None,
            store_dir: &store,
            offline: false,
        };
        let r = realize(&spec, &table, &ctx).unwrap();
        assert_eq!(r.name, "hello");
        assert!(std::path::Path::new(&r.bin).join("hello").is_file());
        std::fs::remove_dir_all(&base).ok();
    }

    // ── Slice C: index-first sparse monorepo fetch (D-MONOREF1) ──

    #[test]
    fn sparse_fetch_materializes_only_addressed_member() {
        use super::super::RefSpec::{classify_in, ProviderKind, SourceTable};
        let base = unique_dir("jpk-sparse-one");
        let repo = base.join("mono");
        let store = base.join("store");
        std::fs::create_dir_all(&store).unwrap();
        if !init_git_repo(
            &repo,
            &[
                (
                    "workspace.jet",
                    "module workspace { members: find(\"./packages\") }\n",
                ),
                (
                    "packages/hello/pkg.jet",
                    "payload: { name: \"hello\", version: \"0.1.0\" }\n",
                ),
                ("packages/hello/hello.jet", "module hello { }\n"),
                ("packages/hello/bin/hello", "#!/bin/sh\necho hi\n"),
                (
                    "packages/world/pkg.jet",
                    "payload: { name: \"world\", version: \"0.1.0\" }\n",
                ),
                ("packages/world/world.jet", "module world { }\n"),
            ],
        ) {
            eprintln!("note: skipping sparse fetch test (git not found)");
            return;
        }
        let upstream = format!("file://{}", repo.to_string_lossy());
        let table =
            SourceTable::from_decls([("mine".to_string(), upstream.clone(), ProviderKind::Core)]);
        let spec = classify_in("mine:hello", &table).unwrap();
        let ctx = Ctx {
            fixtures: None,
            store_dir: &store,
            offline: false,
        };
        let r = realize(&spec, &table, &ctx).unwrap();
        assert_eq!(r.name, "hello");

        // The source-cache checkout has ONLY the addressed member's subtree.
        let remote = parse_remote_source(&upstream).unwrap();
        let cache = source_cache_dir(&store, &remote);
        assert!(
            cache.join("packages/hello/pkg.jet").is_file(),
            "addressed member must be checked out: {}",
            cache.display()
        );
        assert!(
            !cache.join("packages/world").exists(),
            "unaddressed member `world` must NOT be materialized (sparse): {}",
            cache.display()
        );
        // Root files are always present in cone mode.
        assert!(cache.join("workspace.jet").is_file());
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn sparse_fetch_includes_in_repo_dependency() {
        use super::super::RefSpec::{classify_in, ProviderKind, SourceTable};
        let base = unique_dir("jpk-sparse-dep");
        let repo = base.join("mono");
        let store = base.join("store");
        std::fs::create_dir_all(&store).unwrap();
        if !init_git_repo(
            &repo,
            &[
                (
                    "workspace.jet",
                    "module workspace { members: find(\"./packages\") }\n",
                ),
                // `app` depends on the in-repo `logging` member via a path ref
                // whose alias (`log`) differs from the member name — exercises
                // path-target resolution, not just name matching.
                (
                    "packages/app/pkg.jet",
                    "payload: { name: \"app\", version: \"0.1.0\" }\ndeps: { log: path@../logging }\n",
                ),
                ("packages/app/app.jet", "module app { }\n"),
                (
                    "packages/logging/pkg.jet",
                    "payload: { name: \"logging\", version: \"0.1.0\" }\n",
                ),
                ("packages/logging/logging.jet", "module logging { }\n"),
                (
                    "packages/unrelated/pkg.jet",
                    "payload: { name: \"unrelated\", version: \"0.1.0\" }\n",
                ),
                ("packages/unrelated/unrelated.jet", "module unrelated { }\n"),
            ],
        ) {
            eprintln!("note: skipping sparse dep test (git not found)");
            return;
        }
        let upstream = format!("file://{}", repo.to_string_lossy());
        let table =
            SourceTable::from_decls([("mine".to_string(), upstream.clone(), ProviderKind::Core)]);
        let spec = classify_in("mine:app", &table).unwrap();
        let ctx = Ctx {
            fixtures: None,
            store_dir: &store,
            offline: false,
        };
        realize(&spec, &table, &ctx).unwrap();

        let remote = parse_remote_source(&upstream).unwrap();
        let cache = source_cache_dir(&store, &remote);
        assert!(cache.join("packages/app/pkg.jet").is_file(), "app subtree");
        assert!(
            cache.join("packages/logging/pkg.jet").is_file(),
            "in-repo dependency `logging` must be pulled into the sparse checkout: {}",
            cache.display()
        );
        assert!(
            !cache.join("packages/unrelated").exists(),
            "an unrelated member must stay out of the sparse checkout: {}",
            cache.display()
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn in_repo_dep_outside_workspace_is_e1233() {
        use super::super::RefSpec::{classify_in, ProviderKind, SourceTable};
        let base = unique_dir("jpk-e1233");
        let repo = base.join("mono");
        let store = base.join("store");
        std::fs::create_dir_all(&store).unwrap();
        if !init_git_repo(
            &repo,
            &[
                (
                    "workspace.jet",
                    "module workspace { members: find(\"./packages\") }\n",
                ),
                // `app` depends on `packages/ghost`, a real repo directory that
                // is NOT a workspace member (no pkg.jet of its own).
                (
                    "packages/app/pkg.jet",
                    "payload: { name: \"app\", version: \"0.1.0\" }\ndeps: { ghost: path@../ghost }\n",
                ),
                ("packages/app/app.jet", "module app { }\n"),
                ("packages/ghost/notes.txt", "not a package\n"),
            ],
        ) {
            eprintln!("note: skipping E1233 test (git not found)");
            return;
        }
        let upstream = format!("file://{}", repo.to_string_lossy());
        let table = SourceTable::from_decls([("mine".to_string(), upstream, ProviderKind::Core)]);
        let spec = classify_in("mine:app", &table).unwrap();
        let ctx = Ctx {
            fixtures: None,
            store_dir: &store,
            offline: false,
        };
        match realize(&spec, &table, &ctx) {
            Err(e) => assert_eq!(e.code(), Some("E1233"), "expected E1233, got {e:?}"),
            Ok(r) => panic!("expected E1233, but realize succeeded: {r:?}"),
        }
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn core_build_writes_envelope() {
        // T0 (D-JPK-CACHE1=A): realizing a first-party library package produces
        // a hangar object whose record carries the full A4 envelope
        // (output_hash, platform, signature slot, provenance) — not just a
        // fingerprint. The envelope round-trips through the store record.
        use super::super::RefSpec::{classify_in, ProviderKind, SourceTable};
        use super::super::Store;
        let base = unique_dir("jpk-envelope");
        let repo = base.join("jet-pkgs");
        let store = base.join("hangar");
        std::fs::create_dir_all(&store).unwrap();
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(
            repo.join("pkg.jet"),
            "payload: { name: \"p\", version: \"0.1.0\" }\npackages: { mathlib: library }\n",
        )
        .unwrap();
        let lib = repo.join("lib/mathlib");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("mathlib.jet"), "module mathlib { }\n").unwrap();

        let upstream = format!("path:{}", repo.to_string_lossy());
        let table = SourceTable::from_decls([("mine".to_string(), upstream, ProviderKind::Core)]);
        let spec = classify_in("mine:mathlib", &table).unwrap();
        let ctx = Ctx {
            fixtures: None,
            store_dir: &store,
            offline: false,
        };
        let r = realize(&spec, &table, &ctx).unwrap();
        // The realized output carries a complete envelope.
        assert!(!r.envelope.is_empty(), "envelope must be populated: {r:?}");
        assert!(
            r.envelope.output_hash.starts_with("sha256-"),
            "output_hash must be a content hash: {:?}",
            r.envelope
        );
        assert!(!r.envelope.platform.is_empty(), "platform must be set");
        assert!(
            r.envelope.provenance.contains("mine:mathlib"),
            "provenance names the source ref: {:?}",
            r.envelope
        );
        assert!(
            r.envelope.signature.is_empty(),
            "signature slot stays empty until package signing (#13)"
        );

        // Persisting and re-reading the record keeps the envelope intact.
        let roots = Store::Roots {
            root: base.clone(),
            dev_mode: true,
        };
        let entry = Store::record_verified(
            &roots,
            &r.name,
            &r.version,
            &r.reference,
            &r.out,
            &r.bin,
            &r.rlib,
            &r.envelope,
            &r.cache_identity,
        )
        .unwrap();
        assert_eq!(entry.envelope, r.envelope);
        let listed = Store::list(&roots);
        let found = listed.iter().find(|e| e.id == entry.id).unwrap();
        assert_eq!(found.envelope.output_hash, r.envelope.output_hash);
        assert_eq!(found.envelope.platform, r.envelope.platform);
        assert_eq!(found.envelope.provenance, r.envelope.provenance);
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    #[cfg(unix)]
    fn bridge_build_uses_pinned_toolchain() {
        // T2 (D-JPK-BUILDTOOL1=A): a bridge build execs the *pinned* toolchain's
        // cargo, not host cargo. A fixture toolchain stands in for #179's hangar
        // object; its cargo shim emits a deterministic rlib, so the output hash
        // is stable across builds regardless of host cargo. Two fresh builds
        // (no cache hit) produce byte-identical output — proof the pinned tool
        // ran, not whatever host cargo is on PATH.
        use super::super::Toolchain::Toolchain;
        use std::collections::HashMap;
        use std::os::unix::fs::PermissionsExt;
        let base = unique_dir("jpk-bridge");
        let tc_dir = base.join("toolchain");
        std::fs::create_dir_all(&tc_dir).unwrap();
        let cargo = tc_dir.join("cargo");
        std::fs::write(
            &cargo,
            "#!/bin/sh\nmkdir -p \"$CARGO_TARGET_DIR/release\"\n\
             printf 'PINNED-RLIB-BYTES' > \"$CARGO_TARGET_DIR/release/libmath.rlib\"\nexit 0\n",
        )
        .unwrap();
        std::fs::set_permissions(&cargo, std::fs::Permissions::from_mode(0o755)).unwrap();
        let tc = Toolchain {
            cargo: cargo.clone(),
            id: "toolchain-test".to_string(),
            version: "9.9.9".to_string(),
            pinned: true,
            ring_artifacts: HashMap::new(),
        };

        let hangar = base.join("hangar");
        std::fs::create_dir_all(&hangar).unwrap();

        let build_once = |tag: &str| -> Vec<u8> {
            let pkg = base.join(tag);
            std::fs::create_dir_all(&pkg).unwrap();
            std::fs::write(pkg.join("Cargo.toml"), "[package]\nname=\"math\"\n").unwrap();
            let rlib =
                build_rlib_from_cargo(&pkg, &hangar, &tc).expect("pinned build produced rlib");
            let bytes = std::fs::read(&rlib).unwrap();
            // The rlib lands inside the object, and the scratch is swept.
            assert!(rlib.starts_with(pkg.to_string_lossy().as_ref()));
            assert!(
                !hangar.join(BUILD_SCRATCH_DIR).join(tag).exists(),
                "build scratch must be swept after the build"
            );
            bytes
        };

        let a = build_once("pkg-a");
        let b = build_once("pkg-b");
        assert_eq!(a, b"PINNED-RLIB-BYTES", "the pinned toolchain's cargo ran");
        assert_eq!(
            a, b,
            "output is stable across builds with the pinned toolchain"
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn tree_fingerprint_reflects_contents() {
        // Distinct package trees must hash differently (no store collisions);
        // identical trees must hash the same.
        let base = unique_dir("jpk-fp");
        let a = base.join("a");
        let b = base.join("b");
        let c = base.join("c");
        for (d, body) in [(&a, "one"), (&b, "two"), (&c, "one")] {
            std::fs::create_dir_all(d.join("bin")).unwrap();
            std::fs::write(d.join("bin/x"), body).unwrap();
        }
        assert_ne!(tree_fingerprint(&a), tree_fingerprint(&b));
        assert_eq!(tree_fingerprint(&a), tree_fingerprint(&c));
        std::fs::remove_dir_all(&base).ok();
    }

    fn unique_dir(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p =
            std::env::temp_dir().join(format!("{tag}-{nanos}-{:?}", std::thread::current().id()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}
