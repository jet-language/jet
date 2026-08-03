// ──────────────────────────────────────────────
// Private / mirror registry configuration
// ──────────────────────────────────────────────

use crate::Diagnostics::Diagnostic;
use crate::Publish::Index::{self, IndexEntry};
use crate::SHA256;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Registry endpoint configuration. Mirrors proxy the public registry;
/// private registries host organisation-internal packages.
/// D-PKGS1: no hard-coded public infrastructure — registries are configured.
#[derive(Debug, Clone)]
pub struct RegistryConfig {
    /// Registry name (used in `deps: { pkg: { registry: "private" } }` etc.)
    pub name: String,
    /// Base URL for the git registry index.
    pub url: String,
    /// If true, this registry mirrors the public one (proxies unknown packages).
    pub mirror: bool,
    /// If true, require signed metadata from this registry.
    pub require_signed: bool,
}

impl RegistryConfig {
    /// Build the default (well-known) public registry config.  The URL is used
    /// by the immutable git-index publish and resolve paths below.
    pub fn public_default() -> Self {
        Self {
            name: "jet".to_string(),
            url: "https://github.com/jet-lang/registry".to_string(),
            mirror: false,
            require_signed: false,
        }
    }

    /// Build a private mirror config.
    pub fn private(name: &str, url: &str, mirror: bool) -> Self {
        Self {
            name: name.to_string(),
            url: url.to_string(),
            mirror,
            require_signed: false,
        }
    }
}

/// Parse registry configs from environment-backed project policy.  The
/// manifest projection can feed the same typed values without changing the
/// registry or resolver mechanism.
pub fn parse_registries_from_env(
    env: &std::collections::HashMap<String, String>,
) -> Vec<RegistryConfig> {
    // Look for JET_REGISTRY_<NAME>_URL env vars.
    let mut result = Vec::new();
    for (key, url) in env {
        if let Some(suffix) = key.strip_prefix("JET_REGISTRY_") {
            if let Some(name) = suffix.strip_suffix("_URL") {
                let name_lc = name.to_lowercase();
                let mirror_key = format!("JET_REGISTRY_{}_MIRROR", name);
                let mirror = env.get(&mirror_key).map(|v| v == "true").unwrap_or(false);
                result.push(RegistryConfig::private(&name_lc, url, mirror));
            }
        }
    }
    if result.is_empty() {
        result.push(RegistryConfig::public_default());
    }
    result
}

// ──────────────────────────────────────────────
// Git-registry index push/pull (card c56)
// ──────────────────────────────────────────────

/// The registry publish/yank target. Defaults to the public registry; a
/// `JET_REGISTRY_URL` override points `jet registry publish`/`jet registry yank` at another git
/// index (a private registry, a CI mirror, or — in tests — a scratch bare repo).
pub fn resolve_publish_registry() -> RegistryConfig {
    match std::env::var("JET_REGISTRY_URL") {
        Ok(url) if !url.is_empty() => {
            let mut r = RegistryConfig::public_default();
            r.url = url;
            r
        }
        _ => RegistryConfig::public_default(),
    }
}

/// Local clone-cache path for a registry's git index:
/// `<JET_REGISTRY_CACHE_DIR|~/.jet/registry-index>/<registry-name>`.
pub fn index_repo_path(registry: &RegistryConfig) -> PathBuf {
    let base = match std::env::var("JET_REGISTRY_CACHE_DIR") {
        Ok(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".jet").join("registry-index")
        }
    };
    base.join(&registry.name)
}

/// Clone the registry index if we have no local cache, else `git pull --ff-only`
/// to refresh it. Same `Command::new("git")` idiom the dirty-tree check uses —
/// no new dependency. A pull failure on an empty upstream is tolerated (there is
/// nothing to fast-forward yet); a real network/auth failure surfaces as E1235.
pub fn ensure_index_clone(registry: &RegistryConfig) -> Result<PathBuf, Diagnostic> {
    let dir = index_repo_path(registry);
    if let Ok(metadata) = std::fs::symlink_metadata(&dir) {
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(e1235(
                &registry.url,
                "the local registry cache path is not a real directory",
            ));
        }
    }
    if dir.join(".git").is_dir() {
        let pull = Command::new("git")
            .args(["pull", "--ff-only"])
            .current_dir(&dir)
            .output()
            .map_err(|error| e1235(&registry.url, &error.to_string()))?;
        if !pull.status.success() {
            return Err(e1235(
                &registry.url,
                String::from_utf8_lossy(&pull.stderr).trim(),
            ));
        }
        return Ok(dir);
    }
    if let Some(parent) = dir.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let out = Command::new("git")
        .args(["clone", &registry.url, &dir.to_string_lossy()])
        .output();
    match out {
        Ok(o) if o.status.success() => Ok(dir),
        Ok(o) => Err(e1235(
            &registry.url,
            String::from_utf8_lossy(&o.stderr).trim(),
        )),
        Err(e) => Err(e1235(&registry.url, &e.to_string())),
    }
}

/// A short-lived clean checkout used for one registry mutation. The ordinary
/// resolver cache is never used as a publication worktree: credentials,
/// editor files, and another publisher's partial state cannot be swept into a
/// commit by accident.
pub struct PublishCheckout {
    path: PathBuf,
}

impl PublishCheckout {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for PublishCheckout {
    fn drop(&mut self) {
        let temp = std::env::temp_dir();
        if self.path.parent() == Some(temp.as_path())
            && self
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("jet-registry-publish-"))
        {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

/// Clone a clean publication checkout from the registry's current remote.
pub fn prepare_publish_checkout(registry: &RegistryConfig) -> Result<PublishCheckout, Diagnostic> {
    let suffix = unique_suffix();
    let path = std::env::temp_dir().join(format!("jet-registry-publish-{suffix}"));
    let output = Command::new("git")
        .args(["clone", &registry.url, &path.to_string_lossy()])
        .output()
        .map_err(|error| e1235(&registry.url, &error.to_string()))?;
    if !output.status.success() {
        // `git clone` may create the destination before it reports a
        // transport/authentication failure.  Remove only the uniquely named
        // checkout we allocated above; leaving a partial tree lets a later
        // publish mistake it for a clean transaction.
        let _ = std::fs::remove_dir_all(&path);
        return Err(e1235(
            &registry.url,
            String::from_utf8_lossy(&output.stderr).trim(),
        ));
    }
    Ok(PublishCheckout { path })
}

/// Commit the working index tree and push it to the registry. Sets a fallback
/// commit identity so a fresh scratch clone (which has none) can commit. A
/// "nothing to commit" state is idempotent success (re-running publish/yank with
/// bytes already recorded). Any other git failure is E1235.
pub fn push_index(
    registry: &RegistryConfig,
    repo: &Path,
    message: &str,
    paths: &[PathBuf],
    expected: Option<&IndexEntry>,
) -> Result<(), Diagnostic> {
    let run = |args: &[&str]| Command::new("git").args(args).current_dir(repo).output();
    // A scratch clone may carry no user identity; set one so `commit` works.
    let _ = run(&["config", "user.email", "jet-publish@localhost"]);
    let _ = run(&["config", "user.name", "jet registry publish"]);

    let cached_clean = run(&["diff", "--cached", "--quiet"])
        .map_err(|e| e1235(&registry.url, &e.to_string()))?;
    if !cached_clean.status.success() {
        return Err(e1235(
            &registry.url,
            "publication checkout contains pre-staged changes",
        ));
    }
    if paths.is_empty() {
        return Err(e1235(&registry.url, "publication has no explicit paths"));
    }
    let mut add = Command::new("git");
    add.args(["add", "--"]);
    for path in paths {
        let relative = path
            .strip_prefix(repo)
            .map_err(|_| e1235(&registry.url, "publication path escapes its checkout"))?;
        add.arg(relative);
    }
    let add = add.current_dir(repo).output().map_err(|e| e1235(&registry.url, &e.to_string()))?;
    if !add.status.success() {
        return Err(e1235(
            &registry.url,
            String::from_utf8_lossy(&add.stderr).trim(),
        ));
    }
    let staged = run(&["diff", "--cached", "--name-only"])
        .map_err(|e| e1235(&registry.url, &e.to_string()))?;
    if !staged.status.success() {
        return Err(e1235(
            &registry.url,
            String::from_utf8_lossy(&staged.stderr).trim(),
        ));
    }
    let allowed = paths
        .iter()
        .filter_map(|path| path.strip_prefix(repo).ok())
        .map(|path| path.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/"))
        .collect::<Vec<_>>();
    for staged_path in String::from_utf8_lossy(&staged.stdout).lines() {
        let permitted = allowed.iter().any(|path| {
            staged_path == path
                || (staged_path.starts_with(path)
                    && staged_path.as_bytes().get(path.len()) == Some(&b'/'))
        });
        if !permitted {
            return Err(e1235(
                &registry.url,
                "publication attempted to stage a path outside its transaction",
            ));
        }
    }
    let commit =
        run(&["commit", "-m", message]).map_err(|e| e1235(&registry.url, &e.to_string()))?;
    if !commit.status.success() {
        let so = String::from_utf8_lossy(&commit.stdout);
        if !so.contains("nothing to commit") {
            return Err(e1235(
                &registry.url,
                String::from_utf8_lossy(&commit.stderr).trim(),
            ));
        }
    }
    let branch = run(&["rev-parse", "--abbrev-ref", "HEAD"])
        .map_err(|e| e1235(&registry.url, &e.to_string()))?;
    let branch = String::from_utf8_lossy(&branch.stdout).trim().to_string();
    if branch.is_empty() || branch == "HEAD" || branch == "." || branch == ".." {
        return Err(e1235(&registry.url, "publication checkout has no named branch"));
    }
    let push = run(&["push", "origin", &format!("HEAD:refs/heads/{branch}")])
        .map_err(|e| e1235(&registry.url, &e.to_string()))?;
    if !push.status.success() {
        // A concurrent publisher may have won the immutable version. Accept
        // only a byte-identical winner. Otherwise rebase this exact commit
        // onto the new branch head and retry once; a same-version conflict
        // fails closed instead of overwriting or silently yanking bytes.
        let fetch = run(&["fetch", "origin"])
            .map_err(|e| e1235(&registry.url, &e.to_string()))?;
        if !fetch.status.success() {
            return Err(e1235(
                &registry.url,
                String::from_utf8_lossy(&fetch.stderr).trim(),
            ));
        }
        let remote = format!("origin/{branch}");
        if expected.is_some_and(|entry| remote_contains_entry(repo, &remote, entry)) {
            return Ok(());
        }
        let rebase = run(&["rebase", &remote])
            .map_err(|e| e1235(&registry.url, &e.to_string()))?;
        if !rebase.status.success() {
            let _ = run(&["rebase", "--abort"]);
            return Err(e1235(
                &registry.url,
                "concurrent registry publication changed an immutable version",
            ));
        }
        let retry = run(&["push", "origin", &format!("HEAD:refs/heads/{branch}")])
            .map_err(|e| e1235(&registry.url, &e.to_string()))?;
        if !retry.status.success() {
            return Err(e1235(
                &registry.url,
                String::from_utf8_lossy(&retry.stderr).trim(),
            ));
        }
    }
    Ok(())
}

fn remote_contains_entry(repo: &Path, remote: &str, expected: &IndexEntry) -> bool {
    let path = match Index::index_entry_path(repo, &expected.name) {
        Ok(path) => path,
        Err(_) => return false,
    };
    let relative = match path.strip_prefix(repo) {
        Ok(path) => path.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/"),
        Err(_) => return false,
    };
    let spec = format!("{remote}:{relative}");
    let output = match Command::new("git")
        .args(["show", &spec])
        .current_dir(repo)
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => return false,
    };
    let line_matches = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(IndexEntry::parse_line)
        .any(|entry| entry == *expected);
    if !line_matches {
        return false;
    }
    let artifact = match artifact_path(repo, &expected.name, &expected.version) {
        Ok(path) => match path.strip_prefix(repo) {
            Ok(path) => path.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/"),
            Err(_) => return false,
        },
        Err(_) => return false,
    };
    Command::new("git")
        .args(["cat-file", "-e", &format!("{remote}:{artifact}")])
        .current_dir(repo)
        .output()
        .is_ok_and(|output| output.status.success())
}

/// D-VERSION1=A: the version already sits in the index and is not yanked —
/// republishing it is refused. Reads the local clone; the caller must
/// `ensure_index_clone` first.
pub fn find_published(
    repo: &Path,
    name: &str,
    version: &str,
) -> Result<Option<IndexEntry>, Diagnostic> {
    Index::find_entry(repo, name, version)
        .map_err(|error| super::Advisory::e2607("registry index", &error.to_string()))
}

/// Fetch-side resolution: clone/pull the registry index and return the versions
/// of `name` a resolver may still pick (yanked versions filtered out).
pub fn resolve_from_index(
    registry: &RegistryConfig,
    name: &str,
) -> Result<Vec<IndexEntry>, Diagnostic> {
    let repo = ensure_index_clone(registry)?;
    verify_registry_package(&repo, &registry.name, name)
        .map(|entries| entries.into_iter().filter(|entry| !entry.yanked).collect())
}

/// Registry source artifact convention. The git index and the source tree are
/// one publish transaction: `artifacts/<name>/<version>` is committed beside
/// the immutable index line. The artifact is deliberately a plain source tree
/// so a fresh machine can verify its `SHA256::tree_hash` before resolution.
pub fn artifact_path(repo: &Path, name: &str, version: &str) -> io::Result<PathBuf> {
    validate_artifact_component(name, "package name")?;
    validate_artifact_component(version, "package version")?;
    Ok(repo.join("artifacts").join(name).join(version))
}

/// Stage one package source tree into the registry clone and verify its source
/// hash before the index is changed. Existing identical artifacts are reused;
/// conflicting bytes fail closed.
pub fn publish_artifact(
    repo: &Path,
    source: &Path,
    name: &str,
    version: &str,
    expected_hash: &str,
) -> io::Result<PathBuf> {
    let source_metadata = std::fs::symlink_metadata(source)?;
    if source_metadata.file_type().is_symlink() || !source_metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "registry source artifact is not a directory",
        ));
    }
    let actual = SHA256::tree_hash(source);
    if !expected_hash.is_empty() && actual != expected_hash {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("registry source hash changed during publish: expected {expected_hash}, got {actual}"),
        ));
    }
    let destination = artifact_path(repo, name, version)?;
    if let Ok(metadata) = std::fs::symlink_metadata(&destination) {
        if metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "registry artifact path must not be a symlink",
            ));
        }
    }
    if destination.is_dir() {
        if SHA256::tree_hash(&destination) != actual {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "registry artifact already exists with conflicting source bytes",
            ));
        }
        return Ok(destination);
    }
    if destination.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "registry artifact path is not a directory",
        ));
    }
    let parent = destination
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "registry artifact has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let staging = parent.join(format!(
        ".{}.jet-artifact-{}",
        version,
        unique_suffix()
    ));
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    copy_artifact_tree(source, &staging)?;
    if SHA256::tree_hash(&staging) != actual {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "staged registry artifact does not match its source hash",
        ));
    }
    std::fs::rename(&staging, &destination)?;
    Ok(destination)
}

fn unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{}-{nanos}", std::process::id())
}

/// Resolve a previously published source artifact and re-check its immutable
/// source hash before a resolver can load its manifest.
pub fn verify_artifact(repo: &Path, entry: &IndexEntry) -> io::Result<PathBuf> {
    let path = artifact_path(repo, &entry.name, &entry.version)?;
    let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("registry has no source artifact for {} {}", entry.name, entry.version),
            )
        } else {
            error
        }
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("registry has no source artifact for {} {}", entry.name, entry.version),
        ));
    }
    if !entry.content_hash.is_empty() && SHA256::tree_hash(&path) != entry.content_hash {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("registry source artifact for {} {} failed its content hash", entry.name, entry.version),
        ));
    }
    Ok(path)
}

fn validate_artifact_component(value: &str, label: &str) -> io::Result<()> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\')
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("registry {label} is not one safe path component"),
        ));
    }
    Ok(())
}

fn copy_artifact_tree(source: &Path, destination: &Path) -> io::Result<()> {
    std::fs::create_dir_all(destination)?;
    let mut entries = std::fs::read_dir(source)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // These are local state, never package source. tree_hash applies the
        // same exclusion, so omitting them preserves the published identity.
        if name.starts_with('.') || name == "build" || name == "target" {
            continue;
        }
        let from = entry.path();
        let to = destination.join(entry.file_name());
        let metadata = std::fs::symlink_metadata(&from)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() && !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("registry source contains an unsupported node `{}`", from.display()),
            ));
        }
        if metadata.is_dir() {
            copy_artifact_tree(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

// ──────────────────────────────────────────────
// c146 (D-PKGSIGN1) — fetch-time signature verification
// ──────────────────────────────────────────────

/// Verify one resolved index entry against the package's TOFU pin.
///
/// Rules (D-PKGSIGN1 §4):
///   - A signed entry is verified against the **pinned** key (the first
///     recorded `public_key` for the package). A mismatch is a hard error
///     (E1246) — never silently accept tampered bytes (I1).
///   - `require_signed: true` + no signature → hard error (E1247).
///   - An entry that declares a key differing from the pin is a legitimate key
///     rotation: a warning (returned as text), never an error — the registry's
///     git-push auth is the trust root.
///
/// Returns any warnings (rotation notices). `all` is every recorded line for the
/// package (yanked included), so the pin can be found regardless of yanks.
pub fn verify_index_entry(
    all: &[IndexEntry],
    entry: &IndexEntry,
    require_signed: bool,
    registry_name: &str,
) -> Result<Vec<String>, Diagnostic> {
    let mut warnings = Vec::new();

    if entry.signature.trim().is_empty() {
        if require_signed {
            return Err(e1247(registry_name, &entry.name, &entry.version));
        }
        return Ok(warnings); // unsigned and not required — nothing to verify
    }

    // Signed: verify against the TOFU pin. A signature with no discoverable
    // pinned key can't be validated, so it is rejected rather than trusted.
    let pin = match Index::pinned_public_key(all) {
        Some(k) => k,
        None => return Err(e1246(&entry.name, &entry.version)),
    };
    if !crate::Publish::Sign::verify(&pin, &entry.content_hash, &entry.signature)? {
        return Err(e1246(&entry.name, &entry.version));
    }

    let declared = entry.public_key.trim();
    if !declared.is_empty() && declared != pin {
        warnings.push(format!(
            "warning: `{}` {} declares a different signing key than the one first pinned for this \
             package (key rotation). Key rotation is legitimate — the registry's git-push auth is \
             the trust root — but if you did not expect a new publisher key, verify this release \
             before using it.",
            entry.name, entry.version
        ));
    }
    Ok(warnings)
}

/// Fetch-side resolution **with** c146 signature verification: clone/pull the
/// index, then verify every non-yanked entry. Returns the live entries plus any
/// key-rotation warnings; a signature mismatch or a `require_signed` violation
/// aborts with the matching diagnostic.
pub fn resolve_and_verify(
    registry: &RegistryConfig,
    name: &str,
) -> Result<(Vec<IndexEntry>, Vec<String>), Diagnostic> {
    let repo = ensure_index_clone(registry)?;
    let all = verify_registry_package(&repo, &registry.name, name)?;
    let live: Vec<IndexEntry> = all.iter().filter(|e| !e.yanked).cloned().collect();
    let mut warnings = Vec::new();
    for e in &live {
        warnings.extend(verify_index_entry(
            &all,
            e,
            registry.require_signed,
            &registry.name,
        )?);
    }
    Ok((live, warnings))
}

/// E1246 — a package signature does not verify against its pinned public key.
pub fn e1246(name: &str, version: &str) -> Diagnostic {
    Diagnostic::error(
        "E1246",
        format!(
            "signature verification failed for `{name}` {version}: the signature doesn't match the \
             recorded public key"
        ),
        "this means the package was tampered with after signing, or the index entry is corrupt — \
         the author's Ed25519 signature over the content hash no longer checks out."
            .to_string(),
        "do not use this version. Re-run `jet store fetch` after clearing the store entry; if the \
         problem persists, report it — this should never happen for an untampered registry."
            .to_string(),
        None,
    )
}

/// E1247 — a registry that requires signed packages served an unsigned entry.
pub fn e1247(registry: &str, name: &str, version: &str) -> Diagnostic {
    Diagnostic::error(
        "E1247",
        format!(
            "registry `{registry}` requires signed packages (`require_signed: true`) but `{name}` \
             {version} has no signature"
        ),
        "this registry is configured to accept only author-signed releases; an unsigned entry \
         can't be trusted under that policy."
            .to_string(),
        "use a different registry, or ask the package author to publish a signed release \
         (`jet registry publish` auto-signs by default — they likely used `--no-sign`)."
            .to_string(),
        None,
    )
}

/// E1234 — version immutability (D-VERSION1): a published, non-yanked version
/// can never be overwritten.
pub fn e1234(name: &str, version: &str) -> Diagnostic {
    Diagnostic::error(
        "E1234",
        format!("`{name}` {version} already exists in the registry index and is not yanked"),
        "published versions are immutable (D-VERSION1) — a version can never be overwritten, \
         only yanked, so anyone who already locked it keeps building the exact same bytes."
            .to_string(),
        format!(
            "bump the version in `{}` and publish again, or `jet registry yank {version}` the existing \
             one first if it was a mistake (yanking hides it from new resolution; it does not \
             free the version number for reuse).",
            crate::Syntax::PAYLOAD_FILE
        ),
        None,
    )
}

/// E1235 — the registry index could not be reached (clone/pull/push failed).
pub fn e1235(url: &str, detail: &str) -> Diagnostic {
    Diagnostic::error(
        "E1235",
        format!("couldn't reach the registry index at `{url}`"),
        format!(
            "the git operation against the registry failed (network, auth, or a stale local \
             clone): {detail}"
        ),
        format!(
            "check network access and credentials for `{url}`, or set `JET_REGISTRY_URL` to a \
             reachable mirror."
        ),
        None,
    )
}
