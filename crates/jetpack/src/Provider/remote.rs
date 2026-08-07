//! Remote package source discovery, fetching, caching, and staging.

use super::{ensure_network_allowed, Ctx, ProviderError};
use crate::Package;
use crate::RefSpec::Source;
use crate::SHA256;
use jet_pkg_model::Package::PackageFacts;
use std::path::{Path, PathBuf};
use std::process::Command;

/// D-ILE1: infer a package's kind from its source. A top-level `fn run` in any
/// of the package's `.jet` files means `executable`; otherwise `library`. The
/// source is lexed (not string-matched) so `fn run` inside a comment or string
/// literal never produces a false positive.
pub(super) fn infer_package_kind(dir: &Path) -> Package::PackageKind {
    // A staged, non-empty `bin/` is the realized-package convention for "installs
    // on PATH" — executable, regardless of source shape.
    let has_bin = std::fs::read_dir(dir.join("bin"))
        .map(|mut d| d.next().is_some())
        .unwrap_or(false);
    if has_bin || dir_has_top_level_run(dir) {
        Package::PackageKind::Executable
    } else {
        Package::PackageKind::Library
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
pub(super) fn file_has_top_level_run(src: &str) -> bool {
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
/// (the package lives in a subdirectory with its own `package.jet` or
/// migration-era `pkg.jet`), resolution is
/// index-first: only that member's subtree — plus its in-repo dependencies — is
/// materialized via a sparse checkout, never the whole repo (Slice C, D-MONOREF1).
pub(super) fn source_repo(upstream: &str, want_package: &str, ctx: &Ctx) -> Result<PathBuf, ProviderError> {
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
/// the member index, walks the member's package marker for in-repo deps, then checks
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

    // Walk the member's package marker for in-repo dependencies, resolving
    // each against the member index. An in-repo path dep that names a
    // directory in the repo which is not a member is E1233.
    let mut wanted: Vec<String> = vec![target.clone()];
    let all_dirs = all_tree_dirs(&listing);
    let marker = [crate::Syntax::PACKAGE_FILE, crate::Syntax::PAYLOAD_FILE]
        .iter()
        .map(|name| format!("FETCH_HEAD:{target}/{name}"))
        .find_map(|path| git_out(&["show", &path]));
    if let Some(marker) = marker {
        if let Ok(facts) = PackageFacts::parse(&marker, format!("{target}/package")) {
            for (name, source) in facts.deps {
                match classify_canonical_dep(&name, &source, &target, &member_dirs, &all_dirs) {
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
    // so the repo-root package marker/workspace file are available for discovery).
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

/// The directories that contain a package marker (workspace members,
/// `find()` semantics), from a `git ls-tree -r --name-only` listing. Root-level
/// markers are not member subtrees.
fn member_dirs_from_listing(listing: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in listing.lines() {
        let line = line.trim();
        let dir = line
            .strip_suffix(&format!("/{}", crate::Syntax::PACKAGE_FILE))
            .or_else(|| line.strip_suffix(&format!("/{}", crate::Syntax::PAYLOAD_FILE)));
        if let Some(dir) = dir {
            if !dir.is_empty() && !out.contains(&dir.to_string()) {
                out.push(dir.to_string());
            }
        }
    }
    out.sort();
    out
}

fn classify_canonical_dep(
    name: &str,
    source: &Package::DepSource,
    member_dir: &str,
    member_dirs: &[String],
    all_dirs: &[String],
) -> InRepoDep {
    if let Some(member) = member_dirs.iter().find(|dir| dir_basename(dir) == name) {
        return InRepoDep::Member(member.clone());
    }
    if let Package::DepSource::Provider { provider: Source::Path, target } = source {
        let resolved = join_repo_relative(member_dir, target);
        if let Some(resolved) = resolved {
            if member_dirs.contains(&resolved) {
                return InRepoDep::Member(resolved);
            }
            if all_dirs.contains(&resolved) {
                return InRepoDep::OutsideWorkspace(resolved);
            }
        }
    }
    InRepoDep::External
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

/// Resolve a bare path relative to a member directory, staying inside the
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
pub(super) struct RemoteSource {
    pub(super) url: String,
    pub(super) rev: Option<String>,
    pub(super) label: String,
}

pub(super) fn parse_remote_source(upstream: &str) -> Result<RemoteSource, ProviderError> {
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

pub(super) fn fetch_remote_repo(remote: &RemoteSource, ctx: &Ctx) -> Result<PathBuf, ProviderError> {
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

pub(super) fn source_cache_dir(store_dir: &Path, remote: &RemoteSource) -> PathBuf {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_member_index_ignores_root_and_deduplicates_migration_markers() {
        let listing = "package.jet\npackages/app/package.jet\npackages/app/pkg.jet\npackages/logging/package.jet\n";
        assert_eq!(
            member_dirs_from_listing(listing),
            vec!["packages/app".to_string(), "packages/logging".to_string()]
        );
    }

    #[test]
    fn canonical_path_dependency_resolves_by_target_not_alias() {
        let members = vec!["packages/app".to_string(), "packages/logging".to_string()];
        let dirs = vec![
            "packages".to_string(),
            "packages/app".to_string(),
            "packages/logging".to_string(),
            "packages/tools".to_string(),
        ];
        assert!(matches!(
            classify_canonical_dep("log", "../logging", "packages/app", &members, &dirs),
            InRepoDep::Member(path) if path == "packages/logging"
        ));
        assert!(matches!(
            classify_canonical_dep("ghost", "../tools", "packages/app", &members, &dirs),
            InRepoDep::OutsideWorkspace(path) if path == "packages/tools"
        ));
        assert!(matches!(
            classify_canonical_dep("http", "4.2", "packages/app", &members, &dirs),
            InRepoDep::External
        ));
    }
}

/// A content fingerprint over a whole directory tree: every file's relative
/// path, length, bytes, and (on Unix) mode, in sorted order. Unlike the
/// compiler's `.jet`-only `tree_hash`, this addresses *any* package tree, so
/// distinct packages never collide in the store.
pub(super) fn tree_fingerprint(root: &Path) -> String {
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
pub(super) fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let metadata = std::fs::symlink_metadata(&from)?;
        if metadata.file_type().is_symlink() {
            return Err(std::io::Error::other(format!(
                "refusing symlink in copied package tree: {}",
                from.display()
            )));
        }
        if metadata.is_dir() {
            copy_tree(&from, &to)?;
        } else if metadata.is_file() {
            std::fs::copy(&from, &to)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = metadata.permissions().mode();
                std::fs::set_permissions(&to, std::fs::Permissions::from_mode(mode))?;
            }
        } else {
            return Err(std::io::Error::other(format!(
                "refusing non-file in copied package tree: {}",
                from.display()
            )));
        }
    }
    Ok(())
}
