//! Remote package source discovery, fetching, caching, and staging.

use super::{ensure_network_allowed, Ctx, ProviderError};
use crate::Package;
use crate::RefSpec::Source;
use crate::SHA256;
use jet_pkg_model::Package::PackageFacts;
use std::path::{Path, PathBuf};

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
pub(super) fn source_repo(
    upstream: &str,
    want_package: &str,
    ctx: &Ctx,
) -> Result<PathBuf, ProviderError> {
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
    if cache_is_real_directory(&cache)? {
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
    if super::hardened_git_command()
        .arg("--version")
        .output()
        .is_err()
    {
        // No git at all: let the full-clone path produce the "need git" error.
        return SparseOutcome::NotMonorepo;
    }
    let cache_parent = match held_fs::HeldCacheParent::open_or_create(cache) {
        Ok(parent) => parent,
        Err(_) => return SparseOutcome::SparseFailed,
    };
    match cache_parent.is_real_directory() {
        Ok(true) => return SparseOutcome::Materialized(cache.to_path_buf()),
        Ok(false) => {}
        Err(_) => return SparseOutcome::SparseFailed,
    }
    let staging = match cache_parent
        .directory()
        .create_temp_directory("jetpack-sparse")
    {
        Ok(staging) => staging,
        Err(_) => return SparseOutcome::SparseFailed,
    };
    let tmp = match staging.process_path() {
        Ok(path) => path,
        Err(_) => return SparseOutcome::SparseFailed,
    };
    let _guard = staging;

    let git_ok = |args: &[&str]| -> bool {
        super::hardened_git_command()
            .arg("-C")
            .arg(&tmp)
            .args(args)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    };
    let git_out = |args: &[&str]| -> Option<String> {
        let o = super::hardened_git_command()
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
    if !remote_revision_is_safe(remote.rev.as_deref()) {
        return SparseOutcome::SparseFailed;
    }
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

    // Keep the opened checkout authority through validation and publication.
    if publish_remote_checkout_held(_guard.entry(), &cache_parent).is_err() {
        return SparseOutcome::SparseFailed;
    }
    SparseOutcome::Materialized(cache.to_path_buf())
}

#[cfg(test)]
fn publish_remote_checkout(tmp: &Path, cache: &Path) -> Result<(), String> {
    let source = held_fs::HeldDirectoryEntry::open(tmp).map_err(|error| error.to_string())?;
    let cache_parent =
        held_fs::HeldCacheParent::open_or_create(cache).map_err(|error| error.to_string())?;
    publish_remote_checkout_held(&source, &cache_parent)
}

fn publish_remote_checkout_held(
    source: &held_fs::HeldDirectoryEntry,
    cache: &held_fs::HeldCacheParent,
) -> Result<(), String> {
    cache
        .validate_existing()
        .map_err(|error| error.to_string())?;
    tree_fingerprint_held(source.directory()).map_err(|error| error.to_string())?;
    match source.rename_into(cache) {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            let destination = cache
                .open_or_create_directory()
                .map_err(|error| error.to_string())?;
            copy_tree_held(source.directory(), &destination).map_err(|copy_error| {
                format!("rename failed: {rename_error}; checked copy failed: {copy_error}")
            })
        }
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
    if let Package::DepSource::Provider {
        provider: Source::Path,
        target,
    } = source
    {
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
    if !remote_text_is_safe(upstream) {
        return Err(ProviderError::CoreBuild(
            "remote source is empty or contains unsafe control characters".to_string(),
        ));
    }
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
        let remote = RemoteSource {
            url: format!("https://github.com/{owner}/{repo}.git"),
            rev,
            label: format!("github:{owner}/{repo}"),
        };
        validate_remote_source(&remote)?;
        return Ok(remote);
    }

    if base.starts_with("git://")
        || base.starts_with("https://")
        || base.starts_with("ssh://")
        || base.starts_with("file://")
        || base.starts_with("git@")
    {
        let remote = RemoteSource {
            url: base.to_string(),
            rev,
            label: base.to_string(),
        };
        validate_remote_source(&remote)?;
        return Ok(remote);
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

pub(super) fn remote_revision_is_safe(revision: Option<&str>) -> bool {
    match revision {
        None => true,
        Some(revision) => {
            remote_text_is_safe(revision) && !revision.starts_with('-')
        }
    }
}

fn validate_remote_source(remote: &RemoteSource) -> Result<(), ProviderError> {
    if !remote_text_is_safe(&remote.url) || remote.url.starts_with('-') {
        return Err(ProviderError::CoreBuild(
            "remote URL is empty or contains unsafe control characters".to_string(),
        ));
    }
    if !remote_text_is_safe(&remote.label) || !remote_revision_is_safe(remote.rev.as_deref()) {
        return Err(ProviderError::CoreBuild(
            "remote revision is not allowed; use a branch, tag, or commit name without leading `-`"
                .to_string(),
        ));
    }
    Ok(())
}

fn remote_text_is_safe(value: &str) -> bool {
    !value.is_empty()
        && !value.chars().any(char::is_control)
        && !value.contains(['\u{2028}', '\u{2029}'])
}

pub(super) fn fetch_remote_repo(
    remote: &RemoteSource,
    ctx: &Ctx,
) -> Result<PathBuf, ProviderError> {
    validate_remote_source(remote)?;
    let cache = source_cache_dir(ctx.store_dir, remote);
    if cache_is_real_directory(&cache)? {
        return Ok(cache);
    }
    if ctx.offline {
        return Err(ProviderError::Offline(format!(
            "`{}` has no cached checkout and --offline forbids fetching source",
            remote.label
        )));
    }
    ensure_network_allowed("fetch source repo")?;
    if super::hardened_git_command()
        .arg("--version")
        .output()
        .is_err()
    {
        return Err(ProviderError::CoreBuild(
            "remote `core` sources need the `git` command to fetch source repos".to_string(),
        ));
    }

    let cache_parent = held_fs::HeldCacheParent::open_or_create(&cache).map_err(|error| {
        ProviderError::CoreBuild(format!("could not create source cache: {error}"))
    })?;
    if cache_parent.is_real_directory().map_err(|error| {
        ProviderError::CoreBuild(format!("could not inspect source cache: {error}"))
    })? {
        return Ok(cache);
    }
    let staging = cache_parent
        .directory()
        .create_temp_directory("jetpack-source")
        .map_err(|e| {
            ProviderError::CoreBuild(format!("could not create temporary source checkout: {e}"))
        })?;
    let tmp_root = staging.process_path().map_err(|e| {
        ProviderError::CoreBuild(format!("could not address temporary source checkout: {e}"))
    })?;
    let _guard = staging;
    let tmp = tmp_root.join("checkout");

    let output = super::hardened_git_command()
        .args(["clone", "--quiet", "--", &remote.url])
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
        return Err(ProviderError::CoreBuild(format!(
            "failed to fetch `{}`: {reason}",
            remote.label
        )));
    }

    if let Some(rev) = &remote.rev {
        if !remote_revision_is_safe(Some(rev)) {
            return Err(ProviderError::CoreBuild(
                "remote revision is not allowed".to_string(),
            ));
        }
        let output = super::hardened_git_command()
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
            return Err(ProviderError::CoreBuild(format!(
                "failed to check out `{rev}` from `{}`: {reason}",
                remote.label
            )));
        }
    }

    let checkout = held_fs::HeldDirectoryEntry::from_child(
        _guard.directory(),
        std::ffi::OsStr::new("checkout"),
    )
    .map_err(|error| {
        ProviderError::CoreBuild(format!("could not open fetched source checkout: {error}"))
    })?;
    publish_remote_checkout_held(&checkout, &cache_parent).map_err(|error| {
        ProviderError::CoreBuild(format!("could not place fetched source in cache: {error}"))
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

fn cache_is_real_directory(cache: &Path) -> Result<bool, ProviderError> {
    match held_fs::HeldCacheParent::open(cache) {
        Ok(parent) => parent.is_real_directory().map_err(|error| {
            ProviderError::CoreBuild(format!(
                "source cache must contain only real directories: {error}"
            ))
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(ProviderError::CoreBuild(format!(
            "source cache parent must contain only real directories: {error}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_revision_allowlist_rejects_option_injection() {
        assert!(remote_revision_is_safe(None));
        assert!(remote_revision_is_safe(Some("main")));
        assert!(!remote_revision_is_safe(Some("--upload-pack=touch pwned")));
        assert!(!remote_revision_is_safe(Some("main\u{2028}forged")));
        assert!(parse_remote_source("file:///workspace/repo#--upload-pack=touch").is_err());
        assert!(parse_remote_source("https://example.invalid/repo.git\nforged").is_err());
    }

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
        let path_dep = |target: &str| Package::DepSource::Provider {
            provider: Source::Path,
            target: target.to_string(),
        };
        assert!(matches!(
            classify_canonical_dep("log", &path_dep("../logging"), "packages/app", &members, &dirs),
            InRepoDep::Member(path) if path == "packages/logging"
        ));
        assert!(matches!(
            classify_canonical_dep("ghost", &path_dep("../tools"), "packages/app", &members, &dirs),
            InRepoDep::OutsideWorkspace(path) if path == "packages/tools"
        ));
        assert!(matches!(
            classify_canonical_dep(
                "http",
                &Package::DepSource::Version("4.2".to_string()),
                "packages/app",
                &members,
                &dirs
            ),
            InRepoDep::External
        ));
    }

    #[cfg(unix)]
    #[test]
    fn package_tree_operations_reject_symlink_escape() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "jetpack-package-tree-symlink-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let outside = root.with_file_name(format!(
            "jetpack-package-tree-outside-{}",
            std::process::id()
        ));
        std::fs::write(&outside, "must not be read or copied").unwrap();
        symlink(&outside, root.join("escape.txt")).unwrap();
        let copy = root.with_file_name(format!("jetpack-package-tree-copy-{}", std::process::id()));

        assert!(tree_fingerprint(&root).is_err());
        assert!(copy_tree(&root, &copy).is_err());
        assert_eq!(
            std::fs::read_to_string(&outside).unwrap(),
            "must not be read or copied"
        );

        let safe_source = root.join("safe-source");
        std::fs::create_dir_all(&safe_source).unwrap();
        std::fs::write(safe_source.join("safe.txt"), "safe").unwrap();
        let destination_outside = root.with_file_name(format!(
            "jetpack-package-tree-destination-outside-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&destination_outside).unwrap();
        std::fs::write(destination_outside.join("sentinel"), "must stay").unwrap();
        let destination_link = root.with_file_name(format!(
            "jetpack-package-tree-destination-link-{}",
            std::process::id()
        ));
        symlink(&destination_outside, &destination_link).unwrap();
        assert!(copy_tree(&safe_source, &destination_link).is_err());
        assert!(!destination_outside.join("safe.txt").exists());
        assert_eq!(
            std::fs::read_to_string(destination_outside.join("sentinel")).unwrap(),
            "must stay"
        );

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&copy);
        let _ = std::fs::remove_file(&outside);
        let _ = std::fs::remove_dir_all(&destination_outside);
        let _ = std::fs::remove_file(&destination_link);
    }

    #[cfg(unix)]
    #[test]
    fn source_cache_rejects_symlinked_root_and_parent() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "jetpack-source-cache-symlink-{}",
            std::process::id()
        ));
        let outside = root.with_file_name(format!(
            "jetpack-source-cache-outside-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();

        let cache_link = root.join("cache");
        symlink(&outside, &cache_link).unwrap();
        assert!(cache_is_real_directory(&cache_link).is_err());
        let parent_link = root.join("parent");
        symlink(&outside, &parent_link).unwrap();
        assert!(cache_is_real_directory(&parent_link.join("cache")).is_err());

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[cfg(unix)]
    #[test]
    fn remote_publish_rejects_symlink_before_fast_rename() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "jetpack-sparse-publish-symlink-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("checkout")).unwrap();
        let outside = root.with_file_name(format!(
            "jetpack-sparse-publish-outside-{}",
            std::process::id()
        ));
        std::fs::write(&outside, "must not be published").unwrap();
        symlink(&outside, root.join("checkout/escape.txt")).unwrap();
        let cache = root.join("cache");

        assert!(publish_remote_checkout(&root.join("checkout"), &cache).is_err());
        assert!(!cache.exists());
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "must not be published");

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(&outside);
    }
    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    fn unique_remote_test_root(label: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "jetpack-remote-{label}-{}-{sequence}",
            std::process::id()
        ))
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    #[test]
    fn remote_tree_fingerprint_and_copy_reject_multiply_linked_source_files() {
        let root = unique_remote_test_root("hardlink");
        let outside = unique_remote_test_root("hardlink-outside");
        let source = root.join("checkout");
        let destination = root.join("cache");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(&outside);
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(&outside, "outside bytes").unwrap();
        std::fs::hard_link(&outside, source.join("linked.txt")).unwrap();

        assert!(tree_fingerprint(&source).is_err());
        assert!(copy_tree(&source, &destination).is_err());
        assert!(!destination.join("linked.txt").exists());
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "outside bytes");

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(&outside);
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    #[test]
    fn remote_checkout_ancestor_swap_stays_on_held_checkout_and_cache() {
        use std::os::unix::fs::symlink;

        let root = unique_remote_test_root("ancestor-swap");
        let outside = unique_remote_test_root("ancestor-swap-outside");
        let holder = root.join("holder");
        let checkout = holder.join("checkout");
        let destination = root.join("cache");
        let held_holder = root.with_file_name(format!(
            "{}-held",
            root.file_name().unwrap().to_string_lossy()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
        let _ = std::fs::remove_dir_all(&held_holder);
        std::fs::create_dir_all(&checkout).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::create_dir_all(&destination).unwrap();
        std::fs::write(checkout.join("value.txt"), "inside bytes").unwrap();
        std::fs::write(outside.join("value.txt"), "outside bytes").unwrap();

        let source = held_fs::HeldDirectoryEntry::open(&checkout).unwrap();
        let destination_parent = held_fs::HeldCacheParent::open_or_create(&destination).unwrap();
        let destination_dir = destination_parent.open_or_create_directory().unwrap();
        let expected = tree_fingerprint_held(source.directory()).unwrap();

        std::fs::rename(&holder, &held_holder).unwrap();
        symlink(&outside, &holder).unwrap();

        assert_eq!(tree_fingerprint_held(source.directory()).unwrap(), expected);
        copy_tree_held(source.directory(), &destination_dir).unwrap();
        assert_eq!(
            std::fs::read_to_string(destination.join("value.txt")).unwrap(),
            "inside bytes"
        );
        assert_eq!(
            std::fs::read_to_string(outside.join("value.txt")).unwrap(),
            "outside bytes"
        );

        let _ = std::fs::remove_file(&holder);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
        let _ = std::fs::remove_dir_all(&held_holder);
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    #[test]
    fn remote_cache_publication_stays_on_held_parent_after_ancestor_swap() {
        use std::os::unix::fs::symlink;

        let root = unique_remote_test_root("publish-swap");
        let outside = unique_remote_test_root("publish-swap-outside");
        let parent = root.join("cache-parent");
        let checkout = parent.join("checkout");
        let cache = parent.join("cache");
        let held_parent = root.with_file_name(format!(
            "{}-held",
            root.file_name().unwrap().to_string_lossy()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
        let _ = std::fs::remove_dir_all(&held_parent);
        std::fs::create_dir_all(&checkout).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(checkout.join("value.txt"), "inside bytes").unwrap();
        std::fs::write(outside.join("sentinel"), "outside sentinel").unwrap();

        let source = held_fs::HeldDirectoryEntry::open(&checkout).unwrap();
        let cache_parent = held_fs::HeldCacheParent::open_or_create(&cache).unwrap();
        std::fs::rename(&parent, &held_parent).unwrap();
        symlink(&outside, &parent).unwrap();

        publish_remote_checkout_held(&source, &cache_parent).unwrap();
        assert_eq!(
            std::fs::read_to_string(held_parent.join("cache/value.txt")).unwrap(),
            "inside bytes"
        );
        assert!(!outside.join("cache").exists());
        assert_eq!(
            std::fs::read_to_string(outside.join("sentinel")).unwrap(),
            "outside sentinel"
        );

        let _ = std::fs::remove_file(&parent);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
        let _ = std::fs::remove_dir_all(&held_parent);
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    #[test]
    fn remote_staging_cleanup_stays_on_held_parent_after_ancestor_swap() {
        use std::os::unix::fs::symlink;

        let root = unique_remote_test_root("cleanup-swap");
        let outside = unique_remote_test_root("cleanup-swap-outside");
        let parent = root.join("cache-parent");
        let cache = parent.join("cache");
        let held_parent = root.with_file_name(format!(
            "{}-held",
            root.file_name().unwrap().to_string_lossy()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
        let _ = std::fs::remove_dir_all(&held_parent);
        std::fs::create_dir_all(&parent).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("sentinel"), "outside sentinel").unwrap();

        let cache_parent = held_fs::HeldCacheParent::open_or_create(&cache).unwrap();
        let staging = cache_parent
            .directory()
            .create_temp_directory("remote-cleanup")
            .unwrap();
        let staging_name = staging.entry().name().to_os_string();

        std::fs::rename(&parent, &held_parent).unwrap();
        symlink(&outside, &parent).unwrap();
        drop(staging);

        assert!(!held_parent.join(&staging_name).exists());
        assert!(!outside.join(&staging_name).exists());
        assert_eq!(
            std::fs::read_to_string(outside.join("sentinel")).unwrap(),
            "outside sentinel"
        );

        let _ = std::fs::remove_file(&parent);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
        let _ = std::fs::remove_dir_all(&held_parent);
    }
}

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios"
))]
mod held_fs {
    use std::ffi::{c_char, CString, OsStr, OsString};
    use std::fs::{self, File, FileType, Metadata, OpenOptions};
    use std::io;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
    use std::path::{Component, Path, PathBuf};

    const O_RDONLY: i32 = 0;
    const O_WRONLY: i32 = 1;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const O_CREAT: i32 = 0o100;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const O_EXCL: i32 = 0o200;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    const O_CREAT: i32 = 0x0200;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    const O_EXCL: i32 = 0x0800;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const O_CLOEXEC: i32 = 0o2000000;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    const O_CLOEXEC: i32 = 0x01000000;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const O_DIRECTORY: i32 = 0o200000;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    const O_DIRECTORY: i32 = 0x00100000;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const O_NOFOLLOW: i32 = 0o400000;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    const O_NOFOLLOW: i32 = 0x0100;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const O_NONBLOCK: i32 = 0o4000;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    const O_NONBLOCK: i32 = 0x0004;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const AT_REMOVEDIR: i32 = 0x200;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    const AT_REMOVEDIR: i32 = 0x80;

    unsafe extern "C" {
        fn openat(directory: i32, path: *const c_char, flags: i32, ...) -> i32;
        fn mkdirat(directory: i32, path: *const c_char, mode: u32) -> i32;
        fn renameat(
            old_directory: i32,
            old_path: *const c_char,
            new_directory: i32,
            new_path: *const c_char,
        ) -> i32;
        fn unlinkat(directory: i32, path: *const c_char, flags: i32) -> i32;
    }

    pub(super) struct DirectoryEntry {
        pub(super) name: OsString,
        pub(super) file_type: FileType,
    }

    pub(super) struct HeldDirectory {
        path: PathBuf,
        file: File,
    }

    pub(super) struct HeldDirectoryEntry {
        parent: HeldDirectory,
        name: OsString,
        directory: HeldDirectory,
    }

    pub(super) struct HeldTempDirectory {
        entry: HeldDirectoryEntry,
    }

    pub(super) struct HeldCacheParent {
        directory: HeldDirectory,
        name: OsString,
    }

    fn name(value: &OsStr) -> io::Result<CString> {
        CString::new(value.as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in path component"))
    }

    fn open_base(absolute: bool) -> io::Result<File> {
        let path = if absolute {
            Path::new("/")
        } else {
            Path::new(".")
        };
        OpenOptions::new()
            .read(true)
            .custom_flags(O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
            .open(path)
    }

    fn open_at(parent: &File, child: &OsStr, flags: i32, mode: u32) -> io::Result<File> {
        let child = name(child)?;
        let fd = unsafe { openat(parent.as_raw_fd(), child.as_ptr(), flags, mode) };
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(unsafe { File::from_raw_fd(fd) })
        }
    }

    fn remove_at(parent: &File, child: &OsStr, flags: i32) -> io::Result<()> {
        let child = name(child)?;
        if unsafe { unlinkat(parent.as_raw_fd(), child.as_ptr(), flags) } != 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    impl HeldDirectory {
        pub(super) fn open(path: &Path) -> io::Result<Self> {
            Self::walk(path, false)
        }

        pub(super) fn open_or_create(path: &Path) -> io::Result<Self> {
            Self::walk(path, true)
        }

        fn walk(path: &Path, create: bool) -> io::Result<Self> {
            let mut current = Self {
                path: if path.is_absolute() {
                    PathBuf::from("/")
                } else {
                    PathBuf::from(".")
                },
                file: open_base(path.is_absolute())?,
            };
            for component in path.components() {
                let part = match component {
                    Component::Normal(part) => part,
                    Component::RootDir | Component::CurDir => continue,
                    Component::ParentDir => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "directory path contains a parent component",
                        ))
                    }
                    Component::Prefix(_) => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "directory path has an unsupported prefix",
                        ))
                    }
                };
                match current.open_directory(part) {
                    Ok(next) => current = next,
                    Err(error) if create && error.kind() == io::ErrorKind::NotFound => {
                        current.create_directory(part)?;
                        current = current.open_directory(part)?;
                    }
                    Err(error) => return Err(error),
                }
            }
            Ok(current)
        }

        pub(super) fn duplicate(&self) -> io::Result<Self> {
            Ok(Self {
                path: self.path.clone(),
                file: self.file.try_clone()?,
            })
        }

        pub(super) fn process_path(&self) -> io::Result<PathBuf> {
            #[cfg(any(target_os = "linux", target_os = "android"))]
            {
                Ok(PathBuf::from(format!("/proc/self/fd/{}", self.file.as_raw_fd())))
            }
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            {
                Ok(PathBuf::from(format!("/dev/fd/{}", self.file.as_raw_fd())))
            }
        }

        pub(super) fn path(&self) -> &Path {
            &self.path
        }

        pub(super) fn open_directory(&self, child: &OsStr) -> io::Result<Self> {
            let file = open_at(
                &self.file,
                child,
                O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC,
                0,
            )?;
            if !file.metadata()?.is_dir() {
                return Err(io::Error::other("opened path is not a directory"));
            }
            Ok(Self {
                path: self.path.join(child),
                file,
            })
        }

        pub(super) fn open_file(&self, child: &OsStr) -> io::Result<File> {
            let file = open_at(
                &self.file,
                child,
                O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK,
                0,
            )?;
            if !file.metadata()?.is_file() {
                return Err(io::Error::other("opened path is not a regular file"));
            }
            Ok(file)
        }

        pub(super) fn entries(&self) -> io::Result<Vec<DirectoryEntry>> {
            let entries_path = self.process_path()?;
            let mut entries = fs::read_dir(entries_path)?
                .map(|entry| {
                    let entry = entry?;
                    Ok(DirectoryEntry {
                        name: entry.file_name(),
                        file_type: entry.file_type()?,
                    })
                })
                .collect::<io::Result<Vec<_>>>()?;
            entries.sort_by(|left, right| left.name.cmp(&right.name));
            Ok(entries)
        }

        pub(super) fn create_directory(&self, child: &OsStr) -> io::Result<()> {
            let child = name(child)?;
            if unsafe { mkdirat(self.file.as_raw_fd(), child.as_ptr(), 0o700) } != 0 {
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::AlreadyExists {
                    return Err(error);
                }
            }
            Ok(())
        }

        pub(super) fn open_or_create_directory(&self, child: &OsStr) -> io::Result<Self> {
            match self.open_directory(child) {
                Ok(directory) => Ok(directory),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    self.create_directory(child)?;
                    self.open_directory(child)
                }
                Err(error) => Err(error),
            }
        }

        pub(super) fn create_temp_directory(
            &self,
            prefix: &str,
        ) -> io::Result<HeldTempDirectory> {
            if prefix.is_empty()
                || prefix == "."
                || prefix == ".."
                || prefix.chars().any(char::is_control)
                || prefix.contains(['/', '\\'])
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "temporary-directory prefix must be one safe path component",
                ));
            }
            for _ in 0..16 {
                let bytes = crate::TrustRoot::os_random_bytes::<16>()?;
                let suffix = bytes
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                let child = OsString::from(format!("{prefix}-{suffix}"));
                let child_name = name(&child)?;
                if unsafe { mkdirat(self.file.as_raw_fd(), child_name.as_ptr(), 0o700) != 0 } {
                    let error = io::Error::last_os_error();
                    if error.kind() == io::ErrorKind::AlreadyExists {
                        continue;
                    }
                    return Err(error);
                }
                let directory = self.open_directory(&child)?;
                return Ok(HeldTempDirectory {
                    entry: HeldDirectoryEntry {
                        parent: self.duplicate()?,
                        name: child.clone(),
                        directory,
                    },
                });
            }
            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "could not allocate an exclusive temporary directory",
            ))
        }

        fn create_temp_file(&self, prefix: &str, mode: u32) -> io::Result<(OsString, File)> {
            for _ in 0..16 {
                let bytes = crate::TrustRoot::os_random_bytes::<16>()?;
                let suffix = bytes
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                let child = OsString::from(format!("{prefix}-{suffix}"));
                match open_at(
                    &self.file,
                    &child,
                    O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC,
                    mode,
                ) {
                    Ok(file) => return Ok((child, file)),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => return Err(error),
                }
            }
            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "could not allocate an exclusive temporary file",
            ))
        }

        pub(super) fn rename_child(
            &self,
            old: &OsStr,
            destination: &HeldCacheParent,
        ) -> io::Result<()> {
            let old = name(old)?;
            let new = name(destination.name())?;
            if unsafe {
                renameat(
                    self.file.as_raw_fd(),
                    old.as_ptr(),
                    destination.directory().file.as_raw_fd(),
                    new.as_ptr(),
                )
            } != 0
            {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        }

        pub(super) fn replace_child(&self, old: &OsStr, new: &OsStr) -> io::Result<()> {
            let old = name(old)?;
            let new = name(new)?;
            if unsafe {
                renameat(
                    self.file.as_raw_fd(),
                    old.as_ptr(),
                    self.file.as_raw_fd(),
                    new.as_ptr(),
                )
            } != 0
            {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        }

        pub(super) fn remove_tree(&self, child: &OsStr) -> io::Result<()> {
            let child_directory = match self.open_directory(child) {
                Ok(directory) => Some(directory),
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
                Err(_) => None,
            };
            if let Some(directory) = child_directory {
                for entry in directory.entries()? {
                    directory.remove_tree(&entry.name)?;
                }
                remove_at(&self.file, child, AT_REMOVEDIR)?;
                return Ok(());
            }
            match remove_at(&self.file, child, 0) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error),
            }
        }
    }

    impl HeldDirectoryEntry {
        pub(super) fn open(path: &Path) -> io::Result<Self> {
            let name = path.file_name().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "directory path has no final name")
            })?;
            let parent_path = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            let parent = HeldDirectory::open(parent_path)?;
            let directory = parent.open_directory(name)?;
            Ok(Self {
                parent,
                name: name.to_os_string(),
                directory,
            })
        }

        pub(super) fn from_child(parent: &HeldDirectory, name: &OsStr) -> io::Result<Self> {
            let directory = parent.open_directory(name)?;
            Ok(Self {
                parent: parent.duplicate()?,
                name: name.to_os_string(),
                directory,
            })
        }

        pub(super) fn directory(&self) -> &HeldDirectory {
            &self.directory
        }

        #[cfg(test)]
        pub(super) fn name(&self) -> &OsStr {
            &self.name
        }

        pub(super) fn rename_into(&self, destination: &HeldCacheParent) -> io::Result<()> {
            self.parent.rename_child(&self.name, destination)
        }
    }

    impl HeldTempDirectory {
        pub(super) fn entry(&self) -> &HeldDirectoryEntry {
            &self.entry
        }

        pub(super) fn directory(&self) -> &HeldDirectory {
            self.entry.directory()
        }

        pub(super) fn process_path(&self) -> io::Result<PathBuf> {
            self.entry.directory.process_path()
        }
    }

    impl Drop for HeldTempDirectory {
        fn drop(&mut self) {
            let _ = self.entry.parent.remove_tree(&self.entry.name);
        }
    }

    impl HeldCacheParent {
        pub(super) fn open(cache: &Path) -> io::Result<Self> {
            Self::open_with(cache, false)
        }

        pub(super) fn open_or_create(cache: &Path) -> io::Result<Self> {
            Self::open_with(cache, true)
        }

        fn open_with(cache: &Path, create: bool) -> io::Result<Self> {
            let name = cache.file_name().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "cache path has no final name")
            })?;
            let parent_path = cache
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            let directory = if create {
                HeldDirectory::open_or_create(parent_path)?
            } else {
                HeldDirectory::open(parent_path)?
            };
            Ok(Self {
                directory,
                name: name.to_os_string(),
            })
        }

        pub(super) fn directory(&self) -> &HeldDirectory {
            &self.directory
        }

        pub(super) fn name(&self) -> &OsStr {
            &self.name
        }


        pub(super) fn is_real_directory(&self) -> io::Result<bool> {
            match self.directory.open_directory(&self.name) {
                Ok(_) => Ok(true),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
                Err(error) if error.kind() == io::ErrorKind::NotADirectory => {
                    let path = self.directory.path.join(&self.name);
                    match fs::symlink_metadata(path) {
                        Ok(metadata) if metadata.file_type().is_symlink() => Err(
                            io::Error::new(
                                io::ErrorKind::PermissionDenied,
                                "source cache path must not be a symlink",
                            ),
                        ),
                        _ => Ok(false),
                    }
                }
                Err(error) => Err(error),
            }
        }

        pub(super) fn validate_existing(&self) -> io::Result<()> {
            match self.directory.open_directory(&self.name) {
                Ok(_) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::NotADirectory => {
                    let file = self.directory.open_file(&self.name)?;
                    let metadata = file.metadata()?;
                    reject_multiply_linked(&metadata)
                }
                Err(error) => Err(error),
            }
        }

        pub(super) fn open_or_create_directory(&self) -> io::Result<HeldDirectory> {
            self.directory.open_or_create_directory(&self.name)
        }
    }

    pub(super) fn reject_multiply_linked(metadata: &Metadata) -> io::Result<()> {
        if metadata.nlink() > 1 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "multiply linked files are not accepted in a remote checkout",
            ));
        }
        Ok(())
    }

    pub(super) fn copy_file(
        source: &HeldDirectory,
        source_name: &OsStr,
        destination: &HeldDirectory,
        destination_name: &OsStr,
    ) -> io::Result<()> {
        let mut source_file = source.open_file(source_name)?;
        let source_metadata = source_file.metadata()?;
        reject_multiply_linked(&source_metadata)?;

        match destination.open_file(destination_name) {
            Ok(existing) => {
                let metadata = existing.metadata()?;
                if !metadata.is_file() {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "copy destination must be a regular file",
                    ));
                }
                reject_multiply_linked(&metadata)?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }

        let mode = source_metadata.permissions().mode();
        let (temporary_name, mut destination_file) =
            destination.create_temp_file(".jetpack-copy", mode & 0o7777)?;
        let result = (|| {
            io::copy(&mut source_file, &mut destination_file)?;
            destination_file.set_permissions(fs::Permissions::from_mode(mode & 0o7777))?;
            destination_file.sync_all()?;
            let after = source_file.metadata()?;
            if !after.is_file()
                || after.dev() != source_metadata.dev()
                || after.ino() != source_metadata.ino()
                || after.len() != source_metadata.len()
                || after.modified().ok() != source_metadata.modified().ok()
            {
                return Err(io::Error::other("source file changed while copying"));
            }
            destination.replace_child(&temporary_name, destination_name)
        })();
        if result.is_err() {
            let _ = remove_at(&destination.file, &temporary_name, 0);
        }
        result
    }
}
#[cfg(any(target_os = "linux", target_os = "android"))]
const _: fn(&held_fs::HeldDirectory) -> std::io::Result<std::path::PathBuf> =
    held_fs::HeldDirectory::process_path;

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios"
)))]
mod held_fs {
    use super::Path;
    use std::ffi::{OsStr, OsString};
    use std::fs::{File, FileType};
    use std::io;
    use std::path::PathBuf;

    fn unsupported<T>() -> io::Result<T> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "remote checkout authority is unsupported on this platform",
        ))
    }

    pub(super) struct DirectoryEntry {
        pub(super) name: OsString,
        pub(super) file_type: FileType,
    }
    pub(super) struct HeldDirectory;
    pub(super) struct HeldDirectoryEntry;
    pub(super) struct HeldTempDirectory;
    pub(super) struct HeldCacheParent;

    impl HeldDirectory {
        pub(super) fn open(_: &Path) -> io::Result<Self> {
            unsupported()
        }
        pub(super) fn open_or_create(_: &Path) -> io::Result<Self> {
            unsupported()
        }
        pub(super) fn path(&self) -> &Path {
            unreachable!()
        }
        pub(super) fn process_path(&self) -> io::Result<PathBuf> {
            unsupported()
        }
        pub(super) fn open_directory(&self, _: &OsStr) -> io::Result<Self> {
            unsupported()
        }
        pub(super) fn open_file(&self, _: &OsStr) -> io::Result<File> {
            unsupported()
        }
        pub(super) fn entries(&self) -> io::Result<Vec<DirectoryEntry>> {
            unsupported()
        }
        pub(super) fn create_directory(&self, _: &OsStr) -> io::Result<()> {
            unsupported()
        }
        pub(super) fn open_or_create_directory(&self, _: &OsStr) -> io::Result<Self> {
            unsupported()
        }
        pub(super) fn create_temp_directory(&self, _: &str) -> io::Result<HeldTempDirectory> {
            unsupported()
        }
    }

    impl HeldDirectoryEntry {
        pub(super) fn open(_: &Path) -> io::Result<Self> {
            unsupported()
        }
        pub(super) fn from_child(_: &HeldDirectory, _: &OsStr) -> io::Result<Self> {
            unsupported()
        }
        pub(super) fn directory(&self) -> &HeldDirectory {
            unreachable!()
        }
        pub(super) fn path(&self) -> &Path {
            unreachable!()
        }
        pub(super) fn rename_into(&self, _: &HeldCacheParent) -> io::Result<()> {
            unsupported()
        }
    }

    impl HeldTempDirectory {
        pub(super) fn entry(&self) -> &HeldDirectoryEntry {
            unreachable!()
        }
        pub(super) fn directory(&self) -> &HeldDirectory {
            unreachable!()
        }
        pub(super) fn process_path(&self) -> io::Result<PathBuf> {
            unsupported()
        }
    }

    impl HeldCacheParent {
        pub(super) fn open(_: &Path) -> io::Result<Self> {
            unsupported()
        }
        pub(super) fn open_or_create(_: &Path) -> io::Result<Self> {
            unsupported()
        }
        pub(super) fn directory(&self) -> &HeldDirectory {
            unreachable!()
        }
        pub(super) fn name(&self) -> &OsStr {
            unreachable!()
        }
        pub(super) fn path(&self) -> &Path {
            unreachable!()
        }
        pub(super) fn is_real_directory(&self) -> io::Result<bool> {
            unsupported()
        }
        pub(super) fn validate_existing(&self) -> io::Result<()> {
            unsupported()
        }
        pub(super) fn open_or_create_directory(&self) -> io::Result<HeldDirectory> {
            unsupported()
        }
    }

    pub(super) fn reject_multiply_linked(_: &std::fs::Metadata) -> io::Result<()> {
        unsupported()
    }

    pub(super) fn copy_file(
        _: &HeldDirectory,
        _: &OsStr,
        _: &HeldDirectory,
        _: &OsStr,
    ) -> io::Result<()> {
        unsupported()
    }
}

/// A content fingerprint over a whole directory tree: every file's relative
/// path, length, bytes, and (on Unix) mode, in sorted order. Unlike the
/// compiler's `.jet`-only `tree_hash`, this addresses *any* package tree, so
/// distinct packages never collide in the store.
pub(super) fn tree_fingerprint(root: &Path) -> Result<String, String> {
    let source = held_fs::HeldDirectoryEntry::open(root).map_err(|error| error.to_string())?;
    tree_fingerprint_held(source.directory())
}

fn tree_fingerprint_held(root: &held_fs::HeldDirectory) -> Result<String, String> {
    let mut files = Vec::new();
    collect_fingerprint_files(root, Path::new(""), &mut files)
        .map_err(|error| error.to_string())?;
    files.sort_by(|left, right| left.0.cmp(&right.0));

    let mut input = Vec::new();
    for (relative, bytes, mode) in files {
        input.extend_from_slice(relative.to_string_lossy().as_bytes());
        input.push(0);
        input.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
        input.extend_from_slice(&bytes);
        input.extend_from_slice(&mode.to_be_bytes());
    }
    Ok(SHA256::sha256_hex(&input))
}

fn collect_fingerprint_files(
    directory: &held_fs::HeldDirectory,
    relative: &Path,
    files: &mut Vec<(PathBuf, Vec<u8>, u32)>,
) -> std::io::Result<()> {
    use std::io::Read;

    for entry in directory.entries()? {
        let child_relative = relative.join(&entry.name);
        if entry.file_type.is_symlink() {
            return Err(std::io::Error::other(format!(
                "refusing symlink in fingerprinted package tree: {}",
                directory.path().join(&entry.name).display()
            )));
        }
        if entry.file_type.is_dir() {
            let child = directory.open_directory(&entry.name)?;
            collect_fingerprint_files(&child, &child_relative, files)?;
        } else if entry.file_type.is_file() {
            let mut file = directory.open_file(&entry.name)?;
            let metadata = file.metadata()?;
            held_fs::reject_multiply_linked(&metadata)?;
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            let after = file.metadata()?;
            held_fs::reject_multiply_linked(&after)?;
            if !same_file_identity(&metadata, &after) {
                return Err(std::io::Error::other(format!(
                    "fingerprinted source file changed while reading: {}",
                    directory.path().join(&entry.name).display()
                )));
            }
            files.push((child_relative, bytes, file_mode(&metadata)));
        } else {
            return Err(std::io::Error::other(format!(
                "unsupported fingerprinted package entry: {}",
                directory.path().join(&entry.name).display()
            )));
        }
    }
    Ok(())
}

fn file_mode(metadata: &std::fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode()
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        0
    }
}

/// Recursively copy a directory tree, preserving Unix file modes (so `bin/`
/// executables stay executable). Every source and destination operation is
/// relative to a held directory descriptor.
pub(super) fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    let source = held_fs::HeldDirectoryEntry::open(src)?;
    let destination_parent = held_fs::HeldCacheParent::open_or_create(dst)?;
    let destination = destination_parent.open_or_create_directory()?;
    copy_tree_held(source.directory(), &destination)
}

fn copy_tree_held(
    source: &held_fs::HeldDirectory,
    destination: &held_fs::HeldDirectory,
) -> std::io::Result<()> {
    for entry in source.entries()? {
        let source_path = source.path().join(&entry.name);
        if entry.file_type.is_symlink() {
            return Err(std::io::Error::other(format!(
                "refusing symlink in copied package tree: {}",
                source_path.display()
            )));
        }
        if entry.file_type.is_dir() {
            let child_destination = destination.open_or_create_directory(&entry.name)?;
            let child_source = source.open_directory(&entry.name)?;
            copy_tree_held(&child_source, &child_destination)?;
        } else if entry.file_type.is_file() {
            held_fs::copy_file(source, &entry.name, destination, &entry.name)?;
        } else {
            return Err(std::io::Error::other(format!(
                "refusing non-file in copied package tree: {}",
                source_path.display()
            )));
        }
    }
    Ok(())
}

fn same_file_identity(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        return left.dev() == right.dev()
            && left.ino() == right.ino()
            && left.len() == right.len()
            && left.nlink() == right.nlink()
            && left.modified().ok() == right.modified().ok();
    }
    #[cfg(not(unix))]
    {
        left.file_type() == right.file_type()
            && left.len() == right.len()
            && left.modified().ok() == right.modified().ok()
    }
}
