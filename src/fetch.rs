//! Package fetch operations (M12.1, D-PM4).
//!
//! Network access is entirely via git subprocess — no HTTP in the compiler.
//! Path dependencies are resolved in-place (no fetch needed).
//! Results are stored in the Nix-style store (`~/.jet/store/`).

use crate::diag::Diagnostic;
use crate::lock::{self, LockFile, LockSource, LockedPackage, LockedRevision};
use crate::manifest::{check_toolchain, DepSpec, GitSelector, Manifest};
use crate::sha256::tree_hash;
use crate::store;
use crate::syntax;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;

// ──────────────────────────────────────────────
// Main entry points
// ──────────────────────────────────────────────

/// Options for a fetch operation.
pub struct FetchOptions {
    /// If true, refuse network; verify lock only (--locked CI mode).
    pub locked: bool,
    /// If true, re-resolve moving selectors (@latest, branches).
    pub update: bool,
    /// If Some, only update this specific dep name.
    pub update_dep: Option<String>,
}

/// Resolve, fetch, and install all dependencies from a manifest.
/// Returns the written lock file and a map of dep name → source directory.
pub fn fetch(
    project_root: &Path,
    manifest: &Manifest,
    existing_lock: Option<&LockFile>,
    opts: &FetchOptions,
) -> Result<(LockFile, HashMap<String, PathBuf>), Vec<Diagnostic>> {
    // Validate toolchain constraint.
    let manifest_path = project_root.join(syntax::PAYLOAD_FILE).display().to_string();
    if let Err(d) = check_toolchain(manifest, &manifest_path) {
        return Err(vec![d]);
    }

    // --locked: verify lock matches manifest, then stop.
    if opts.locked {
        let lock = existing_lock.ok_or_else(|| {
            vec![lock::e1202(
                &project_root.join(syntax::UNIFIED_LOCK_FILE).display().to_string(),
            )]
        })?;
        if let Err(d) = lock::verify_lock_matches_manifest(
            lock,
            manifest,
            &project_root.join(syntax::UNIFIED_LOCK_FILE).display().to_string(),
        ) {
            return Err(vec![d]);
        }
        let dep_dirs = build_dep_dirs_from_lock(lock, project_root, manifest)?;
        return Ok((lock.clone(), dep_dirs));
    }

    // Resolve the full dependency graph.
    let mut resolver = Resolver::new(project_root, existing_lock, opts);
    let (new_lock, dep_dirs) = resolver.resolve_manifest(manifest)?;

    // Write the lock file, inside the project's `.jet/` managed folder (U2).
    let lock_path = project_root.join(syntax::UNIFIED_LOCK_FILE);
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            vec![Diagnostic::error(
                "E1206",
                format!("couldn't create {}", parent.display()),
                "the lock file lives inside the project's `.jet/` managed folder".to_string(),
                format!("check write permissions: {}", e),
                None,
            )]
        })?;
    }
    let lock_str = lock::write(&new_lock);
    std::fs::write(&lock_path, &lock_str).map_err(|e| {
        vec![Diagnostic::error(
            "E1206",
            format!("couldn't write {}", syntax::UNIFIED_LOCK_FILE),
            "the lock file records exact package versions".to_string(),
            format!("check write permissions: {}", e),
            None,
        )]
    })?;

    Ok((new_lock, dep_dirs))
}

// ──────────────────────────────────────────────
// Resolver
// ──────────────────────────────────────────────

struct Resolver<'a> {
    project_root: &'a Path,
    existing_lock: Option<&'a LockFile>,
    opts: &'a FetchOptions,
    /// name → (version, source_dir, fingerprint, deps)
    resolved: BTreeMap<String, ResolvedPkg>,
    /// name → Vec<chain> — for E1201 blame chains.
    version_seen: HashMap<String, (String, Vec<String>)>,
}

struct ResolvedPkg {
    version: String,
    source: lock::LockSource,
    locked: Option<LockedRevision>,
    fingerprint: String,
    deps: Vec<String>,
    source_dir: PathBuf,
    tree_hash: String,
}

impl<'a> Resolver<'a> {
    fn new(
        project_root: &'a Path,
        existing_lock: Option<&'a LockFile>,
        opts: &'a FetchOptions,
    ) -> Self {
        Resolver {
            project_root,
            existing_lock,
            opts,
            resolved: BTreeMap::new(),
            version_seen: HashMap::new(),
        }
    }

    fn resolve_manifest(
        &mut self,
        manifest: &Manifest,
    ) -> Result<(LockFile, HashMap<String, PathBuf>), Vec<Diagnostic>> {
        let root_deps: Vec<String> = manifest.dependencies.keys().cloned().collect();

        // Resolve each direct dep recursively.
        for (dep_name, spec) in &manifest.dependencies {
            let chain = vec![
                format!("{} (root)", manifest.package.name),
                dep_name.clone(),
            ];
            self.resolve_dep(dep_name, spec, self.project_root, &chain)?;
        }

        // Build the lock file.
        let mut packages: Vec<LockedPackage> = Vec::new();

        // Root package.
        packages.push(LockedPackage {
            name: manifest.package.name.clone(),
            version: manifest.package.version.clone(),
            source: LockSource::Root,
            locked: None,
            fingerprint: String::new(),
            dependencies: root_deps.clone(),
        });

        // Dependency packages in stable order.
        for (name, pkg) in &self.resolved {
            packages.push(LockedPackage {
                name: name.clone(),
                version: pkg.version.clone(),
                source: pkg.source.clone(),
                locked: pkg.locked.clone(),
                fingerprint: pkg.fingerprint.clone(),
                dependencies: pkg.deps.clone(),
            });
        }

        let new_lock = LockFile {
            version: lock::LOCK_VERSION,
            packages,
            root_dependencies: root_deps,
        };

        // Build dep_dirs map.
        let mut dep_dirs = HashMap::new();
        for (name, pkg) in &self.resolved {
            dep_dirs.insert(name.clone(), pkg.source_dir.clone());
        }

        Ok((new_lock, dep_dirs))
    }

    fn resolve_dep(
        &mut self,
        dep_name: &str,
        spec: &DepSpec,
        parent_dir: &Path,
        chain: &[String],
    ) -> Result<(), Vec<Diagnostic>> {
        if self.resolved.contains_key(dep_name) {
            // Already resolved — check for version conflict.
            let existing = &self.resolved[dep_name];
            let existing_version = existing.version.clone();
            if let Some((prev_version, prev_chain)) = self.version_seen.get(dep_name).cloned() {
                if prev_version != existing_version {
                    return Err(vec![lock::e1201(
                        dep_name,
                        &prev_version,
                        &prev_chain,
                        &existing_version,
                        chain,
                    )]);
                }
            }
            return Ok(());
        }

        match spec {
            DepSpec::Path { path } => {
                let abs_path = if Path::new(path).is_absolute() {
                    PathBuf::from(path)
                } else {
                    normalize_path(&parent_dir.join(path))
                };

                // Load the dep's manifest.
                let dep_manifest = self.load_dep_manifest(&abs_path, dep_name)?;

                let dep_version = dep_manifest.package.version.clone();
                let dep_name_in_manifest = dep_manifest.package.name.clone();

                // Check for version conflict with another dep that has the same package name.
                if let Some((prev_ver, prev_chain)) =
                    self.version_seen.get(&dep_name_in_manifest).cloned()
                {
                    if prev_ver != dep_version {
                        return Err(vec![lock::e1201(
                            &dep_name_in_manifest,
                            &prev_ver,
                            &prev_chain,
                            &dep_version,
                            chain,
                        )]);
                    }
                } else {
                    self.version_seen.insert(
                        dep_name_in_manifest.clone(),
                        (dep_version.clone(), chain.to_vec()),
                    );
                }

                // Compute tree hash (for fingerprint and store).
                let th = tree_hash(&abs_path);

                // Resolve transitive deps.
                let mut trans_deps = Vec::new();
                for (trans_name, trans_spec) in &dep_manifest.dependencies {
                    let mut child_chain = chain.to_vec();
                    child_chain.push(trans_name.clone());
                    self.resolve_dep(trans_name, trans_spec, &abs_path, &child_chain)?;
                    trans_deps.push(trans_name.clone());
                }

                // Compute fingerprint.
                let dep_fps: Vec<&str> = trans_deps
                    .iter()
                    .filter_map(|d| self.resolved.get(d).map(|r| r.fingerprint.as_str()))
                    .collect();
                let fp = lock::compute_fingerprint(&th, &dep_fps);

                // Store the path dep (copy to store for inode sharing).
                let store_path = store::ensure_path_dep(dep_name, &dep_version, &fp, &abs_path)
                    .map_err(|d| vec![d])?;

                // Link into project build dir.
                let link_dir = self
                    .project_root
                    .join(".jet-build")
                    .join("deps")
                    .join(dep_name);
                store::link_into_project(&store_path, &link_dir).map_err(|d| vec![d])?;

                self.resolved.insert(
                    dep_name.to_string(),
                    ResolvedPkg {
                        version: dep_version,
                        source: LockSource::Path(path.clone()),
                        locked: None,
                        fingerprint: fp,
                        deps: trans_deps,
                        source_dir: abs_path,
                        tree_hash: th,
                    },
                );
            }

            DepSpec::Git { url, selector } => {
                // Check if git is available.
                if !git_available() {
                    return Err(vec![lock::e1203()]);
                }

                // Determine what rev to fetch.
                let rev_to_fetch = self.resolve_git_rev(dep_name, url, selector)?;
                let clone_dir = git_cache_dir(url, &rev_to_fetch);

                // Clone/fetch if not already cached.
                if !clone_dir.is_dir() {
                    git_clone(url, &rev_to_fetch, &clone_dir)?;
                }

                // Load the dep's manifest from the cloned dir.
                let dep_manifest = self.load_dep_manifest(&clone_dir, dep_name)?;
                let dep_version = dep_manifest.package.version.clone();

                // Get the git tree hash.
                let git_tree_hash = git_tree_hash(&clone_dir, &rev_to_fetch).unwrap_or_else(|_| {
                    format!("sha256-{}", &rev_to_fetch[..8.min(rev_to_fetch.len())])
                });

                // Check for version conflicts.
                let dep_name_in_manifest = dep_manifest.package.name.clone();
                if let Some((prev_ver, prev_chain)) =
                    self.version_seen.get(&dep_name_in_manifest).cloned()
                {
                    if prev_ver != dep_version {
                        return Err(vec![lock::e1201(
                            &dep_name_in_manifest,
                            &prev_ver,
                            &prev_chain,
                            &dep_version,
                            chain,
                        )]);
                    }
                } else {
                    self.version_seen.insert(
                        dep_name_in_manifest.clone(),
                        (dep_version.clone(), chain.to_vec()),
                    );
                }

                // Resolve transitive deps.
                let mut trans_deps = Vec::new();
                for (trans_name, trans_spec) in &dep_manifest.dependencies {
                    let mut child_chain = chain.to_vec();
                    child_chain.push(trans_name.clone());
                    self.resolve_dep(trans_name, trans_spec, &clone_dir, &child_chain)?;
                    trans_deps.push(trans_name.clone());
                }

                // Compute fingerprint.
                let dep_fps: Vec<&str> = trans_deps
                    .iter()
                    .filter_map(|d| self.resolved.get(d).map(|r| r.fingerprint.as_str()))
                    .collect();
                let fp = lock::compute_fingerprint(&git_tree_hash, &dep_fps);

                // Store.
                let store_path = store::ensure_git_dep(dep_name, &dep_version, &fp, &clone_dir)
                    .map_err(|d| vec![d])?;

                // Link into project build dir.
                let link_dir = self
                    .project_root
                    .join(".jet-build")
                    .join("deps")
                    .join(dep_name);
                store::link_into_project(&store_path, &link_dir).map_err(|d| vec![d])?;

                let locked = LockedRevision {
                    rev: rev_to_fetch.clone(),
                    tree_hash: git_tree_hash.clone(),
                    last_modified: unix_now(),
                };

                self.resolved.insert(
                    dep_name.to_string(),
                    ResolvedPkg {
                        version: dep_version,
                        source: LockSource::Git {
                            url: url.clone(),
                            selector: lock::git_selector_str(selector),
                        },
                        locked: Some(locked),
                        fingerprint: fp,
                        deps: trans_deps,
                        source_dir: clone_dir,
                        tree_hash: git_tree_hash,
                    },
                );
            }

            DepSpec::Registry(_version) => {
                // Registry deps are M12.2. Give a clear error.
                return Err(vec![Diagnostic::error(
                    "E1207",
                    format!("registry dependencies are not yet supported in this version"),
                    "registry support (`textkit = \"1.2.0\"`) is planned for M12.2".to_string(),
                    format!(
                        "use `{name} = {{ path = \"...\" }}` or `{name} = {{ git = \"...\", tag = \"...\" }}` for now",
                        name = dep_name
                    ),
                    None,
                )]);
            }
        }

        Ok(())
    }

    fn load_dep_manifest(&self, dir: &Path, dep_name: &str) -> Result<Manifest, Vec<Diagnostic>> {
        let result = crate::manifest::load(dir);
        match result {
            None => Err(vec![Diagnostic::error(
                "E1206",
                format!("dependency `{}` has no `{}`", dep_name, crate::syntax::PAYLOAD_FILE),
                format!("every Jet package must have a `{}` manifest", crate::syntax::PAYLOAD_FILE),
                format!("add a `{}` to `{}`", crate::syntax::PAYLOAD_FILE, dir.display()),
                None,
            )]),
            Some(Err(d)) => Err(vec![d]),
            Some(Ok(m)) => Ok(m),
        }
    }

    fn resolve_git_rev(
        &self,
        dep_name: &str,
        url: &str,
        selector: &GitSelector,
    ) -> Result<String, Vec<Diagnostic>> {
        match selector {
            GitSelector::Rev(r) => Ok(r.clone()),
            GitSelector::Tag(t) if t != "@latest" => {
                // If we have an existing lock and not updating, use locked rev.
                if !self.opts.update && !self.should_update(dep_name) {
                    if let Some(locked_rev) = self.find_locked_rev(dep_name) {
                        return Ok(locked_rev);
                    }
                }
                // Resolve the tag to a specific commit via ls-remote.
                git_resolve_ref(url, t).map_err(|d| vec![d])
            }
            GitSelector::Branch(b) | GitSelector::Tag(b) => {
                // Moving selector: always re-resolve if updating.
                if !self.opts.update && !self.should_update(dep_name) {
                    if let Some(locked_rev) = self.find_locked_rev(dep_name) {
                        return Ok(locked_rev);
                    }
                }
                git_resolve_ref(url, b).map_err(|d| vec![d])
            }
        }
    }

    fn should_update(&self, dep_name: &str) -> bool {
        self.opts.update_dep.as_deref() == Some(dep_name)
    }

    fn find_locked_rev(&self, dep_name: &str) -> Option<String> {
        let lock = self.existing_lock?;
        lock.packages
            .iter()
            .find(|p| p.name == dep_name)
            .and_then(|p| p.locked.as_ref())
            .map(|l| l.rev.clone())
    }
}

// ──────────────────────────────────────────────
// Build dep_dirs from existing lock (--locked mode)
// ──────────────────────────────────────────────

fn build_dep_dirs_from_lock(
    lock: &LockFile,
    project_root: &Path,
    manifest: &Manifest,
) -> Result<HashMap<String, PathBuf>, Vec<Diagnostic>> {
    let mut dep_dirs = HashMap::new();
    for (dep_name, spec) in &manifest.dependencies {
        let source_dir = match spec {
            DepSpec::Path { path } => normalize_path(&project_root.join(path)),
            DepSpec::Git { .. } => {
                // Find the locked rev and use the git cache dir.
                let locked_pkg = lock.packages.iter().find(|p| p.name == *dep_name);
                if let Some(pkg) = locked_pkg {
                    if let Some(rev) = pkg.locked.as_ref().map(|l| &l.rev) {
                        if let LockSource::Git { url, .. } = &pkg.source {
                            git_cache_dir(url, rev)
                        } else {
                            project_root.to_path_buf()
                        }
                    } else {
                        project_root.to_path_buf()
                    }
                } else {
                    project_root.to_path_buf()
                }
            }
            DepSpec::Registry(_) => {
                return Err(vec![Diagnostic::error(
                    "E1207",
                    "registry dependencies require `jet fetch` (not --locked)".to_string(),
                    "registry support is planned for M12.2".to_string(),
                    "use path or git dependencies".to_string(),
                    None,
                )]);
            }
        };
        dep_dirs.insert(dep_name.clone(), source_dir);
    }
    Ok(dep_dirs)
}

// ──────────────────────────────────────────────
// Git helpers
// ──────────────────────────────────────────────

pub fn git_available() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}

fn git_cache_dir(url: &str, rev: &str) -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    let url_hash = crate::sha256::sha256_hex(url.as_bytes());
    PathBuf::from(home)
        .join(".jet")
        .join("git-cache")
        .join(&url_hash[..16])
        .join(&rev[..16.min(rev.len())])
}

fn git_resolve_ref(url: &str, refname: &str) -> Result<String, Diagnostic> {
    let out = Command::new("git")
        .args(["ls-remote", "--exit-code", url, refname])
        .output()
        .map_err(|_| lock::e1203())?;
    if !out.status.success() {
        return Err(Diagnostic::error(
            "E1203",
            format!("couldn't resolve git ref `{}` at `{}`", refname, url),
            "the git ref may not exist or the URL may be unreachable".to_string(),
            format!("check the URL and ref name in {}", crate::syntax::PAYLOAD_FILE),
            None,
        ));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let rev = stdout.split_whitespace().next().unwrap_or("").to_string();
    if rev.is_empty() {
        return Err(Diagnostic::error(
            "E1203",
            format!("git ref `{}` not found at `{}`", refname, url),
            "the tag or branch name must exist in the remote repository".to_string(),
            format!("check the ref spelling in {}", crate::syntax::PAYLOAD_FILE),
            None,
        ));
    }
    Ok(rev)
}

fn git_clone(url: &str, rev: &str, dest: &Path) -> Result<(), Vec<Diagnostic>> {
    std::fs::create_dir_all(dest).map_err(|e| {
        vec![Diagnostic::error(
            "E1206",
            format!("couldn't create git cache directory: {}", e),
            "the git cache lives in ~/.jet/git-cache/".to_string(),
            "check disk space and permissions".to_string(),
            None,
        )]
    })?;

    // Clone the repository and check out the specific revision.
    let tmp = dest.with_extension("_tmp");
    let _ = std::fs::remove_dir_all(&tmp);

    let clone_ok = Command::new("git")
        .args(["clone", "--quiet", url, tmp.to_str().unwrap_or(".")])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !clone_ok {
        return Err(vec![Diagnostic::error(
            "E1203",
            format!("failed to clone `{}`", url),
            "git clone returned an error".to_string(),
            "check the git URL and your network connection".to_string(),
            None,
        )]);
    }

    let checkout_ok = Command::new("git")
        .args([
            "-C",
            tmp.to_str().unwrap_or("."),
            "checkout",
            "--quiet",
            rev,
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !checkout_ok {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(vec![Diagnostic::error(
            "E1203",
            format!("couldn't check out revision `{}` from `{}`", rev, url),
            "git checkout returned an error".to_string(),
            "check that the revision exists in the repository".to_string(),
            None,
        )]);
    }

    std::fs::rename(&tmp, dest).map_err(|e| {
        vec![Diagnostic::error(
            "E1206",
            format!("couldn't move cloned repo into place: {}", e),
            "filesystem rename failed".to_string(),
            "check disk space and permissions".to_string(),
            None,
        )]
    })?;

    Ok(())
}

fn git_tree_hash(repo_dir: &Path, _rev: &str) -> Result<String, String> {
    let out = Command::new("git")
        .args([
            "-C",
            repo_dir.to_str().unwrap_or("."),
            "rev-parse",
            "HEAD^{tree}",
        ])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err("git rev-parse failed".to_string());
    }
    let hash = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok(format!("sha256-{}", hash))
}

fn normalize_path(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
