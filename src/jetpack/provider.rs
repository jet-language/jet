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

/// Pick the provider for a ref. Built-ins and nix-backed named sources use the
/// `nix` provider; a named source declared `core` (D-JPK17 `via`, R2) uses the
/// first-party builder.
pub fn provider_for(spec: &RefSpec, table: &SourceTable) -> Box<dyn Provider> {
    match &spec.source {
        Source::Named(name) if table.provider(name) == ProviderKind::Core => Box::new(CoreProvider),
        _ => Box::new(NixProvider),
    }
}

/// True when realizing this ref goes through the Nix compatibility provider.
pub fn uses_nix_provider(spec: &RefSpec, table: &SourceTable) -> bool {
    !matches!(&spec.source, Source::Named(name) if table.provider(name) == ProviderKind::Core)
}

/// Realize a ref through its provider. The resolver entry point: it never knows
/// or cares which backend runs — that is the whole point of the boundary.
pub fn realize(spec: &RefSpec, table: &SourceTable, ctx: &Ctx) -> Result<Realized, ProviderError> {
    provider_for(spec, table).realize(spec, table, ctx)
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
        assert_eq!(provider_for(&spec, &table).name(), "core");
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
