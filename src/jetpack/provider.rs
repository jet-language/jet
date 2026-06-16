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

use super::json;
use super::envfile;
use super::refspec::{ProviderKind, RefSpec, Source, SourceTable};
use crate::sha256;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A realized package: where its bytes are and what to put on PATH.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Realized {
    pub name: String,
    pub reference: String,
    pub out: String,
    pub bin: String,
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

// ──────────────────────────────────────────────
// Provider boundary (R0; see docs/plans/jetpack-jetos/native-resolver.md).
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
            None => run_nix(spec, table)?,
        };
        parse_realization(spec, &stdout)
    }
}

/// The first-party Jet package provider (R2). Realizes a Jet package with no
/// Nix at all: it reads the source repo's `env.jet`, finds the package's
/// `pkg.package(...)` declaration, and materializes that source tree into the
/// Jetpack store. R2 supports local and git-backed remote source repos.
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
        let repo = source_repo(upstream, ctx)?;
        let ef = envfile::load(&repo).ok_or_else(|| {
            ProviderError::CoreBuild(format!(
                "the source repo at {} has no {}",
                repo.display(),
                crate::syntax::ENV_FILE
            ))
        })?;
        let subpath = ef.provided(&spec.package).ok_or_else(|| {
            ProviderError::CoreBuild(format!(
                "repo `{source_name}` does not provide a package named `{}`",
                spec.package
            ))
        })?;
        let src_dir = repo.join(subpath.trim_start_matches("./"));
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
        if !out_dir.exists() {
            copy_tree(&src_dir, &out_dir)
                .map_err(|e| ProviderError::CoreBuild(format!("could not place package: {e}")))?;
        }
        let bin = out_dir.join("bin");
        Ok(Realized {
            name: spec.package.clone(),
            reference: spec.raw.clone(),
            out: out_dir.to_string_lossy().into_owned(),
            bin: bin.to_string_lossy().into_owned(),
        })
    }
}

/// Resolve a `core` upstream to a local checkout. `path:` sources are used in
/// place; `github:` and git URLs are fetched into a Jetpack source cache.
fn source_repo(upstream: &str, ctx: &Ctx) -> Result<PathBuf, ProviderError> {
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
    fetch_remote_repo(&remote, ctx)
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
        return Err(ProviderError::CoreBuild(format!(
            "offline mode has no cached checkout for `{}`",
            remote.label
        )));
    }
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
    let key = sha256::sha256_hex(
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
    sha256::sha256_hex(&input)
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
        // U9: peek the remote's `pack.jet` to choose core vs nix.
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

/// Realize a ref through its provider. The resolver entry point: it never knows
/// or cares which backend runs — that is the whole point of the boundary.
pub fn realize(spec: &RefSpec, table: &SourceTable, ctx: &Ctx) -> Result<Realized, ProviderError> {
    let kind = resolve_kind(spec, table, ctx.offline, ctx.store_dir);
    provider_for(kind).realize(spec, table, ctx)
}

/// U9 remote probe: classify a `github@…`/git upstream as `Core` (it carries a
/// `pack.jet`) or `Nix` (it does not), peeking **only** `pack.jet` — never
/// cloning a nixpkgs-sized repo just to classify it.
///
/// Resolution order:
/// 1. If a source-cache checkout already exists (a prior realize fetched it),
///    classify from the local tree — offline-safe, no network.
/// 2. Offline with no cache: we can't probe, so default to `nix`.
/// 3. Online: a lightweight `git` peek — a partial, no-checkout, depth-1 clone
///    (`--filter=tree:0`, so blobs/subtrees are never downloaded) into a temp
///    dir, then `git ls-tree <rev> pack.jet`. Present → `Core`; absent or any
///    peek failure → `Nix` (the safe default; a github flake still realizes
///    through nix).
fn infer_remote_kind(upstream: &str, offline: bool, cache_dir: &Path) -> ProviderKind {
    let Ok(remote) = parse_remote_source(upstream) else {
        return ProviderKind::Nix;
    };
    // (1) Reuse a prior fetch.
    let cache = source_cache_dir(cache_dir, &remote);
    if cache.is_dir() {
        return pack_kind(cache.join(crate::syntax::PACK_FILE).is_file());
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

/// Peek whether `remote` has a `pack.jet` at its root, without a full clone.
///
/// Fetches **only the named rev** into a throwaway repo, shallow (`--depth 1`)
/// and partial (`--filter=tree:0`, so trees/blobs are deferred), then reads the
/// root tree with `git ls-tree FETCH_HEAD`. Even a nixpkgs-sized repo transfers
/// just the one commit object plus the lazily-fetched root tree. `git fetch`
/// resolves a branch, tag, **or** commit SHA uniformly, so the rev's exact
/// `pack.jet` is peeked regardless of how it was pinned. Any failure (no `git`,
/// network error, unfetchable rev) is treated as "no pack.jet" by the caller
/// (→ nix), the safe default.
fn remote_has_pack_jet(remote: &RemoteSource) -> bool {
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
            .args(["ls-tree", "FETCH_HEAD", crate::syntax::PACK_FILE])
            .output()
            .map(|o| o.status.success() && !o.stdout.is_empty())
            .unwrap_or(false);

    let _ = std::fs::remove_dir_all(&tmp);
    has_pack
}

fn run_nix(spec: &RefSpec, table: &SourceTable) -> Result<String, ProviderError> {
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

/// Parse `nix build --json` output: an array of build results, each with an
/// `outputs` object. Prefer a `bin` output, else `out`.
fn parse_realization(spec: &RefSpec, stdout: &str) -> Result<Realized, ProviderError> {
    let json = json::parse(stdout.trim()).map_err(|e| ProviderError::BadOutput(e))?;
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
    Ok(Realized {
        name: spec.short_name().to_string(),
        reference: spec.raw.clone(),
        out: out.to_string(),
        bin,
    })
}

#[cfg(test)]
mod tests {
    use super::super::refspec::{classify, classify_in};
    use super::*;

    fn empty() -> SourceTable {
        SourceTable::empty()
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
            super::super::refspec::ProviderKind::Nix,
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
        use super::super::refspec::{classify_in, ProviderKind, SourceTable};
        // Build a throwaway repo: env.jet declaring a `hello` package whose
        // source tree has a runnable bin/hello.
        let base = unique_dir("jpk-core");
        let repo = base.join("jet-pkgs");
        let store = base.join("store");
        let hello_bin = repo.join("pkgs/hello/bin");
        std::fs::create_dir_all(&hello_bin).unwrap();
        std::fs::write(
            repo.join("env.jet"),
            "pkg.package(\"hello\", \"./pkgs/hello\");\n",
        )
        .unwrap();
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
        assert_eq!(resolve_kind(&spec, &table, false, &store), ProviderKind::Core);
        assert_eq!(provider_for(ProviderKind::Core).name(), "core");
        let r = realize(&spec, &table, &ctx).unwrap();
        assert_eq!(r.name, "hello");
        assert!(std::path::Path::new(&r.bin).join("hello").is_file());
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn core_provider_fetches_remote_git_package() {
        use super::super::refspec::{classify_in, ProviderKind, SourceTable};
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
        let hello_bin = repo.join("pkgs/hello/bin");
        std::fs::create_dir_all(&hello_bin).unwrap();
        std::fs::create_dir_all(&store).unwrap();
        std::fs::write(
            repo.join("env.jet"),
            "pkg.package(\"hello\", \"./pkgs/hello\");\n",
        )
        .unwrap();
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
        use super::super::refspec::{classify_in, ProviderKind, SourceTable};
        let base = unique_dir("jpk-probe");
        let store = base.join("store");
        std::fs::create_dir_all(&store).unwrap();

        // A repo carrying `pack.jet` is a Jet package source → core.
        let with = base.join("with-pack");
        if !init_git_repo(
            &with,
            &[("pack.jet", "package: { name: \"p\", version: \"0.1.0\" }\n")],
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
            "a remote carrying pack.jet must infer core"
        );

        // A repo with no `pack.jet` is a plain (nix) flake/source → nix.
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
            "a remote with no pack.jet must infer nix"
        );

        // Offline with no cached checkout can't probe → defaults to nix even for
        // the pack.jet-bearing repo.
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
        use super::super::refspec::{classify_in, ProviderKind, SourceTable};
        let base = unique_dir("jpk-probe-sha");
        let store = base.join("store");
        std::fs::create_dir_all(&store).unwrap();
        let repo = base.join("repo");
        if !init_git_repo(
            &repo,
            &[("pack.jet", "package: { name: \"p\", version: \"0.1.0\" }\n")],
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
        let table =
            SourceTable::from_decls([("mine".to_string(), upstream, ProviderKind::Infer)]);
        let spec = classify_in("mine:hello", &table).unwrap();
        assert_eq!(
            resolve_kind(&spec, &table, false, &store),
            ProviderKind::Core,
            "a commit-SHA-pinned remote with pack.jet must infer core"
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn realize_resolves_inferred_remote_to_core() {
        // U9 end-to-end at the realize boundary: an `Infer` source — the kind a
        // typed `github@…` source carries — whose remote has a `pack.jet`
        // resolves to the `core` provider and builds the first-party package,
        // with no nix and no declared marker.
        use super::super::refspec::{classify_in, ProviderKind, SourceTable};
        let base = unique_dir("jpk-infer-build");
        let repo = base.join("remote");
        let store = base.join("store");
        std::fs::create_dir_all(&store).unwrap();
        if !init_git_repo(
            &repo,
            &[
                ("pack.jet", "package: { name: \"p\", version: \"0.1.0\" }\n"),
                ("env.jet", "pkg.package(\"hello\", \"./pkgs/hello\");\n"),
                ("pkgs/hello/bin/hello", "#!/bin/sh\necho hi-infer\n"),
            ],
        ) {
            eprintln!("note: skipping inferred remote build test (git not found)");
            return;
        }
        let upstream = format!("file://{}", repo.to_string_lossy());
        let table =
            SourceTable::from_decls([("mine".to_string(), upstream, ProviderKind::Infer)]);
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
