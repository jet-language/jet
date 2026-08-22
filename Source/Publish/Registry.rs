// ──────────────────────────────────────────────
// Private / mirror registry configuration
// ──────────────────────────────────────────────

use crate::Diagnostics::Diagnostic;
use crate::Publish::Index::{self, IndexEntry};
use crate::Publish::Sign;
use crate::SHA256;
use super::Tuf::verify_registry_package;
use super::Tier::{RegistryTier, community_gate_error};
use std::cell::Cell;
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
    /// The curated core or machine-gated community channel.
    pub tier: RegistryTier,
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
            tier: RegistryTier::Core,
        }
    }

    /// Build a private mirror config.
    pub fn private(name: &str, url: &str, mirror: bool) -> Self {
        Self {
            name: name.to_string(),
            url: url.to_string(),
            mirror,
            require_signed: false,
            tier: RegistryTier::Core,
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
            r.tier = RegistryTier::from_environment();
            r.require_signed = r.tier == RegistryTier::Community;
            r
        }
        _ => {
            let mut registry = RegistryConfig::public_default();
            registry.tier = RegistryTier::from_environment();
            registry.require_signed = registry.tier == RegistryTier::Community;
            registry
        }
    }
}

/// Remove embedded user information before a registry endpoint reaches any
/// user-visible diagnostic. Credentials belong in a provider, never in a git
/// URL or Jet output.
pub fn redact_registry_url(value: &str) -> String {
    let Some(separator) = value.find("://") else {
        return value.to_string();
    };
    let authority_start = separator + 3;
    let authority_end = value[authority_start..]
        .find(['/', '?', '#'])
        .map(|offset| authority_start + offset)
        .unwrap_or(value.len());
    let authority = &value[authority_start..authority_end];
    let Some(at) = authority.rfind('@') else {
        return value.to_string();
    };
    format!(
        "{}{}{}",
        &value[..authority_start],
        &authority[at + 1..],
        &value[authority_end..]
    )
}

fn registry_url_has_credentials(value: &str) -> bool {
    let Some(separator) = value.find("://") else {
        return false;
    };
    let authority_start = separator + 3;
    let authority_end = value[authority_start..]
        .find(['/', '?', '#'])
        .map(|offset| authority_start + offset)
        .unwrap_or(value.len());
    value[authority_start..authority_end].contains('@')
}

fn validate_registry_transport(registry: &RegistryConfig) -> Result<(), Diagnostic> {
    if registry_url_has_credentials(&registry.url) {
        return Err(e1235(
            &registry.url,
            "registry URLs must not contain embedded credentials",
        ));
    }
    Ok(())
}

fn git_command() -> Command {
    let mut command = Command::new("git");
    command.env("GIT_TERMINAL_PROMPT", "0");
    command.env_remove("JET_REGISTRY_URL");
    for (key, _) in std::env::vars_os() {
        let key = key.to_string_lossy();
        if key.starts_with("JET_REGISTRY_") && key.ends_with("_URL") {
            command.env_remove(&*key);
        }
    }
    command
}

/// Host-pinned root key location for a registry. The registry name is hashed
/// before it becomes a path component, so repository or environment input can
/// never escape the host trust directory.
pub fn registry_root_key_path(registry_name: &str) -> PathBuf {
    let base = Sign::keys_dir()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("registry-roots")
        .join(format!("{}.pub", SHA256::sha256_hex(registry_name.as_bytes())))
}

fn configured_registry_root_key_path(registry_name: &str) -> Option<PathBuf> {
    std::env::var_os("JET_REGISTRY_ROOT_KEY")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| Some(registry_root_key_path(registry_name)))
}

pub fn registry_checkpoint_path(registry_name: &str) -> PathBuf {
    let base = Sign::keys_dir()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("registry-checkpoints")
        .join(format!("{}.state", SHA256::sha256_hex(registry_name.as_bytes())))
}

/// Establish or verify the host-pinned TUF root key used by sparse metadata.
/// Publication may establish the pin from the local registry signing key;
/// resolution never accepts a key from the repository as a first trust input.
pub fn ensure_registry_root_key(registry_name: &str, public_key: &str) -> io::Result<PathBuf> {
    if public_key.len() != 64 || !public_key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "registry root key is not a 32-byte hexadecimal public key",
        ));
    }
    let configured = std::env::var_os("JET_REGISTRY_ROOT_KEY")
        .filter(|value| !value.is_empty())
        .is_some();
    let path = configured_registry_root_key_path(registry_name)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "registry root key path is empty"))?;
    if let Ok(metadata) = std::fs::symlink_metadata(&path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "registry root key is not a regular file",
            ));
        }
        let existing = std::fs::read_to_string(&path)?;
        if existing.trim() != public_key {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "registry root key pin changed",
            ));
        }
        return Ok(path);
    }
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "registry root key has no parent")
    })?;
    if configured {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("configured registry root key is unavailable at {}", path.display()),
        ));
    }
    std::fs::create_dir_all(parent)?;
    let mut file = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let existing = std::fs::read_to_string(&path)?;
            if existing.trim() != public_key {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "registry root key pin changed",
                ));
            }
            return Ok(path);
        }
        Err(error) => return Err(error),
    };
    use std::io::Write;
    file.write_all(public_key.as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(path)
}

pub fn read_registry_root_key(registry_name: &str) -> io::Result<String> {
    let path = configured_registry_root_key_path(registry_name)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "registry root key path is empty"))?;
    let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("registry root key is not installed at {}: {error}", path.display()),
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "registry root key is not a regular file",
        ));
    }
    let key = std::fs::read_to_string(&path)?.trim().to_string();
    if key.len() != 64 || !key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "registry root key is malformed",
        ));
    }
    Ok(key)
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
    // Registry names can come from environment-backed project policy. Keep
    // the human-readable component for ordinary names, but never let an
    // invalid name become a path component: a value such as `../shared` must
    // not escape the configured cache root.
    base.join(registry_cache_component(&registry.name))
}

fn registry_cache_component(name: &str) -> String {
    let safe = !name.is_empty()
        && name != "."
        && name != ".."
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
        });
    if safe {
        name.to_string()
    } else {
        format!("registry-{}", SHA256::sha256_hex(name.as_bytes()))
    }
}

struct RegistryCacheLock(PathBuf);

impl Drop for RegistryCacheLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn acquire_registry_cache_lock(parent: &Path, name: &str) -> io::Result<RegistryCacheLock> {
    let path = parent.join(format!(".{}.cache-lock", registry_cache_component(name)));
    for _ in 0..100 {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => {
                file.sync_all()?;
                return Ok(RegistryCacheLock(path));
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let stale = std::fs::metadata(&path)
                    .ok()
                    .and_then(|metadata| metadata.modified().ok())
                    .and_then(|modified| modified.elapsed().ok())
                    .is_some_and(|age| age > std::time::Duration::from_secs(300));
                if stale {
                    let _ = std::fs::remove_file(&path);
                    continue;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "registry cache refresh is busy",
    ))
}

fn clone_registry_to(registry: &RegistryConfig, path: &Path) -> Result<(), Diagnostic> {
    let path_display = path.to_string_lossy().into_owned();
    let output = git_command()
        .args(["clone", &registry.url, &path_display])
        .output();
    match output {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => Err(clone_failure(
            registry,
            path,
            true,
            clone_output_detail(&output),
        )),
        Err(error) => Err(clone_failure(registry, path, true, error.to_string())),
    }
}

fn install_registry_clone(
    registry: &RegistryConfig,
    dir: &Path,
    parent: &Path,
    partial: &Path,
    replace_existing: bool,
) -> Result<PathBuf, Diagnostic> {
    if !replace_existing {
        return match std::fs::rename(partial, dir) {
            Ok(()) => Ok(dir.to_path_buf()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let cleanup = cleanup_owned_clone(partial, true);
                if !dir.join(".git").is_dir() {
                    return Err(e1235(
                        &registry.url,
                        "a concurrent registry cache winner is incomplete",
                    ));
                }
                match cleanup {
                    Ok(()) => Ok(dir.to_path_buf()),
                    Err(cleanup_error) => Err(attach_checkout_cleanup_failure(
                        e1235(&registry.url, "couldn't remove the losing partial registry clone"),
                        partial,
                        &cleanup_error,
                    )),
                }
            }
            Err(error) => Err(clone_failure(registry, partial, true, error.to_string())),
        };
    }

    let backup = parent.join(format!(
        ".{}.backup-{}",
        registry_cache_component(&registry.name),
        unique_suffix()
    ));
    std::fs::rename(dir, &backup).map_err(|error| {
        clone_failure(
            registry,
            partial,
            true,
            format!("couldn't stage the existing registry cache: {error}"),
        )
    })?;
    if let Err(error) = std::fs::rename(partial, dir) {
        let restore = std::fs::rename(&backup, dir);
        let cleanup = cleanup_owned_clone(partial, true);
        let detail = match restore {
            Ok(()) => format!("couldn't install the refreshed registry cache: {error}"),
            Err(restore_error) => format!(
                "couldn't install the refreshed registry cache: {error}; restoring the previous cache failed: {restore_error}"
            ),
        };
        return match cleanup {
            Ok(()) => Err(e1235(&registry.url, &detail)),
            Err(cleanup_error) => Err(attach_checkout_cleanup_failure(
                e1235(&registry.url, &detail),
                partial,
                &cleanup_error,
            )),
        };
    }
    cleanup_owned_clone(&backup, true).map_err(|error| {
        e1235(
            &registry.url,
            &format!("refreshed registry cache installed but old-cache cleanup failed: {error}"),
        )
    })?;
    Ok(dir.to_path_buf())
}

fn ensure_index_clone_locked(
    registry: &RegistryConfig,
    dir: &Path,
    parent: &Path,
) -> Result<PathBuf, Diagnostic> {
    let existing = match std::fs::symlink_metadata(dir) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(e1235(
                &registry.url,
                "the local registry cache path is not a real directory",
            ));
        }
        Ok(_) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(e1235(&registry.url, &error.to_string())),
    };
    let git_dir = dir.join(".git");
    if existing {
        match std::fs::symlink_metadata(&git_dir) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(e1235(
                    &registry.url,
                    "the local registry cache has an invalid git directory",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(e1235(
                    &registry.url,
                    "the local registry cache is not a git repository",
                ));
            }
            Err(error) => return Err(e1235(&registry.url, &error.to_string())),
        }
        let origin = git_command()
            .args(["remote", "get-url", "origin"])
            .current_dir(dir)
            .output()
            .map_err(|error| e1235(&registry.url, &error.to_string()))?;
        if !origin.status.success()
            || String::from_utf8_lossy(&origin.stdout).trim() != registry.url.as_str()
        {
            return Err(e1235(
                &registry.url,
                "the local registry cache belongs to a different remote",
            ));
        }
    }

    let partial = parent.join(format!(
        ".{}.partial-{}",
        registry_cache_component(&registry.name),
        unique_suffix()
    ));
    if std::fs::symlink_metadata(&partial).is_ok() {
        return Err(e1235(
            &registry.url,
            "registry cache has a colliding partial clone",
        ));
    }
    clone_registry_to(registry, &partial)?;
    install_registry_clone(registry, dir, parent, &partial, existing)
}

/// Clone the registry index into a private sibling, then install it by rename.
/// Refreshing an existing cache uses the same staged replacement, so an
/// interrupted or failed fetch leaves the previous verified clone untouched.
pub fn ensure_index_clone(registry: &RegistryConfig) -> Result<PathBuf, Diagnostic> {
    validate_registry_transport(registry)?;
    let dir = index_repo_path(registry);
    if let Some(parent) = dir.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            return Err(e1235(&registry.url, &error.to_string()));
        }
    }
    let parent = dir
        .parent()
        .ok_or_else(|| e1235(&registry.url, "registry cache path has no parent"))?;
    let _lock = acquire_registry_cache_lock(parent, &registry.name)
        .map_err(|error| e1235(&registry.url, &error.to_string()))?;
    ensure_index_clone_locked(registry, &dir, parent)
}

/// A short-lived clean checkout used for one registry mutation. The ordinary
/// resolver cache is never used as a publication worktree: credentials,
/// editor files, and another publisher's partial state cannot be swept into a
/// commit by accident.
pub struct PublishCheckout {
    path: PathBuf,
    cleanup_attempted: Cell<bool>,
    owns_path: bool,
}

impl PublishCheckout {
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Remove this checkout before an orderly return.
    pub fn cleanup(&self) -> io::Result<()> {
        self.cleanup_attempted.set(true);
        cleanup_publish_checkout(&self.path, self.owns_path)
    }
}

impl Drop for PublishCheckout {
    fn drop(&mut self) {
        if self.cleanup_attempted.replace(true) {
            return;
        }
        if let Err(error) = cleanup_publish_checkout(&self.path, self.owns_path) {
            let diagnostic = checkout_cleanup_diagnostic(&self.path, &error);
            eprint!("{}", crate::Diagnostics::render_all("", "", &[diagnostic]));
        }
    }
}

fn checkout_cleanup_problem(path: &Path, error: &io::Error) -> String {
    format!(
        "couldn't remove registry checkout `{}`: {error}",
        path.display()
    )
}

fn checkout_cleanup_diagnostic(path: &Path, error: &io::Error) -> Diagnostic {
    let problem = checkout_cleanup_problem(path, error);
    jet_foundation::Diagnostics::Diagnostic::from_row(
        "E2105",
        &[("problem", problem.as_str())],
        None,
    )
}

fn attach_checkout_cleanup_failure(
    mut primary: Diagnostic,
    path: &Path,
    error: &io::Error,
) -> Diagnostic {
    let detail = format!(
        "cleanup failed: {}",
        checkout_cleanup_problem(path, error)
    );
    if let Some(existing) = primary.detail.as_mut() {
        if !existing.is_empty() && !existing.ends_with('\n') {
            existing.push('\n');
        }
        existing.push_str(&detail);
    } else {
        primary.detail = Some(detail);
    }
    primary
}

fn finish_publish_checkout<T>(
    checkout: PublishCheckout,
    operation: impl FnOnce(&Path) -> Result<T, Diagnostic>,
) -> Result<T, Diagnostic> {
    let path = checkout.path().to_path_buf();
    let result = operation(&path);
    let cleanup = checkout.cleanup();
    match cleanup {
        Ok(()) => result,
        Err(error) => match result {
            Ok(_) => Err(checkout_cleanup_diagnostic(&path, &error)),
            Err(primary) => Err(attach_checkout_cleanup_failure(primary, &path, &error)),
        },
    }
}

fn cleanup_owned_clone(path: &Path, owned: bool) -> io::Result<()> {
    if !owned {
        return Ok(());
    }
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn cleanup_publish_checkout(path: &Path, owned: bool) -> io::Result<()> {
    let temp = std::env::temp_dir();
    let safe_name = path.parent() == Some(temp.as_path())
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("jet-registry-publish-"));
    cleanup_owned_clone(path, owned && safe_name)
}

fn clone_output_detail(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stdout.is_empty() {
        return stdout;
    }
    format!("git clone exited with {}", output.status)
}

fn clone_failure(
    registry: &RegistryConfig,
    path: &Path,
    owns_destination: bool,
    detail: String,
) -> Diagnostic {
    let primary = e1235(&registry.url, &detail);
    match cleanup_owned_clone(path, owns_destination) {
        Ok(()) => primary,
        Err(cleanup_error) => attach_checkout_cleanup_failure(primary, path, &cleanup_error),
    }
}

/// Clone a clean publication checkout from the registry's current remote.
pub fn prepare_publish_checkout(registry: &RegistryConfig) -> Result<PublishCheckout, Diagnostic> {
    validate_registry_transport(registry)?;
    let suffix = unique_suffix();
    let path = std::env::temp_dir().join(format!("jet-registry-publish-{suffix}"));
    if std::fs::symlink_metadata(&path).is_ok() {
        return Err(e1235(
            &registry.url,
            "publication checkout path already exists",
        ));
    }
    let path_display = path.to_string_lossy().into_owned();
    let output = git_command()
        .args(["clone", &registry.url, &path_display])
        .output();
    let output = match output {
        Ok(output) => output,
        Err(error) => {
            return Err(clone_failure(
                registry,
                &path,
                true,
                error.to_string(),
            ));
        }
    };
    if !output.status.success() {
        return Err(clone_failure(
            registry,
            &path,
            true,
            clone_output_detail(&output),
        ));
    }
    Ok(PublishCheckout {
        path,
        cleanup_attempted: Cell::new(false),
        owns_path: true,
    })
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
    push_index_inner(registry, repo, message, paths, expected, true)
}

fn push_index_inner(
    registry: &RegistryConfig,
    repo: &Path,
    message: &str,
    paths: &[PathBuf],
    expected: Option<&IndexEntry>,
    recover_race: bool,
) -> Result<(), Diagnostic> {
    validate_registry_transport(registry)?;
    let run = |args: &[&str]| git_command().args(args).current_dir(repo).output();
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
    let mut add = git_command();
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
        // only a byte-identical winner. Otherwise rebuild from a fresh remote
        // checkout; rebasing append-only index files can merge two conflicting
        // same-version lines and would violate immutable identity.
        let fetch = run(&["fetch", "origin"])
            .map_err(|e| e1235(&registry.url, &e.to_string()))?;
        if !fetch.status.success() {
            return Err(e1235(
                &registry.url,
                String::from_utf8_lossy(&fetch.stderr).trim(),
            ));
        }
        let remote = format!("origin/{branch}");
        if let Some(entry) = expected {
            if remote_contains_entry(repo, &remote, entry)
                && verify_remote_winner(registry, entry)?
            {
                return Ok(());
            }
        }
        if recover_race {
            return rebuild_publication_after_race(registry, repo, message, paths, expected);
        }
        return Err(e1235(
            &registry.url,
            "concurrent registry publication changed an immutable version",
        ));
    }
    Ok(())
}

fn verify_remote_winner(
    registry: &RegistryConfig,
    expected: &IndexEntry,
) -> Result<bool, Diagnostic> {
    let checkout = prepare_publish_checkout(registry)?;
    finish_publish_checkout(checkout, |repo| {
        let entries =
            crate::Publish::verify_registry_package(repo, &registry.name, &expected.name)?;
        let Some(actual) = entries
            .into_iter()
            .find(|entry| entry.version == expected.version)
        else {
            return Ok(false);
        };
        if actual != *expected {
            return Ok(false);
        }
        crate::Publish::verify_artifact(repo, &actual)
            .map(|_| true)
            .map_err(|error| {
                super::Advisory::e2607("concurrent registry winner", &error.to_string())
            })
    })
}

fn rebuild_publication_after_race(
    registry: &RegistryConfig,
    stale_repo: &Path,
    message: &str,
    _paths: &[PathBuf],
    expected: Option<&IndexEntry>,
) -> Result<(), Diagnostic> {
    let expected = expected.ok_or_else(|| {
        e1235(
            &registry.url,
            "concurrent publication cannot be rebuilt without an immutable entry",
        )
    })?;
    let checkout = prepare_publish_checkout(registry)?;
    finish_publish_checkout(checkout, |repo| {
        let index = Index::index_entry_path(repo, &expected.name)
            .map_err(|error| e1235(&registry.url, &error.to_string()))?;
        if expected.yanked {
            match Index::mark_yanked(repo, &expected.name, &expected.version) {
                Ok(true) => {}
                Ok(false) => {
                    return Err(e1235(
                        &registry.url,
                        "concurrent yank lost its immutable registry entry",
                    ));
                }
                Err(error) => return Err(e1235(&registry.url, &error.to_string())),
            }
        } else {
            match Index::find_entry(repo, &expected.name, &expected.version)
                .map_err(|error| e1235(&registry.url, &error.to_string()))?
            {
                Some(existing) if existing != *expected => {
                    return Err(e1234(&expected.name, &expected.version));
                }
                Some(_) => {}
                None => Index::write_index_entry(repo, expected).map_err(|error| {
                    if error.kind() == io::ErrorKind::AlreadyExists {
                        e1234(&expected.name, &expected.version)
                    } else {
                        e1235(&registry.url, &error.to_string())
                    }
                })?,
            }
            let source = artifact_path(stale_repo, &expected.name, &expected.version)
                .map_err(|error| e1235(&registry.url, &error.to_string()))?;
            if !source.is_dir() {
                return Err(e1235(
                    &registry.url,
                    "concurrent publication lost its staged source artifact",
                ));
            }
            publish_artifact(
                repo,
                &source,
                &expected.name,
                &expected.version,
                &expected.content_hash,
            )
            .map_err(|error| e1235(&registry.url, &error.to_string()))?;
        }
        let metadata = crate::Publish::refresh_registry_metadata(repo, &registry.name)
            .map_err(|diagnostic| e1235(&registry.url, &diagnostic.what))?;
        let mut paths = vec![index];
        if !expected.yanked {
            paths.push(
                artifact_path(repo, &expected.name, &expected.version)
                    .map_err(|error| e1235(&registry.url, &error.to_string()))?,
            );
        }
        paths.extend(metadata.paths);
        push_index_inner(registry, repo, message, &paths, Some(expected), false)
    })
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
    let output = match git_command()
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
    if !git_command()
        .args(["cat-file", "-e", &format!("{remote}:{artifact}")])
        .current_dir(repo)
        .output()
        .is_ok_and(|output| output.status.success())
    {
        return false;
    }
    let package_metadata = match crate::Publish::registry_package_metadata_path(repo, &expected.name) {
        Ok(path) => path,
        Err(_) => return false,
    };
    let log = repo.join("transparency").join("log");
    let checkpoint = repo.join("transparency").join("checkpoint");
    [package_metadata, log, checkpoint].iter().all(|path| {
        path.strip_prefix(repo)
            .ok()
            .map(|relative| relative.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/"))
            .is_some_and(|relative| {
                git_command()
                    .args(["cat-file", "-e", &format!("{remote}:{relative}")])
                    .current_dir(repo)
                    .output()
                    .is_ok_and(|output| output.status.success())
            })
    })
}

/// D-VERSION1=A: a version already sits in the index — yanked or live — so
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
    let entries = verify_registry_package(&repo, &registry.name, name)?;
    for entry in &entries {
        verify_entry_tier(entry)?;
    }
    Ok(entries.into_iter().filter(|entry| !entry.yanked).collect())
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
    ensure_real_directory(repo, "registry checkout")?;
    ensure_real_directory_if_present(&repo.join("artifacts"), "registry artifact root")?;
    ensure_real_directory_if_present(
        &repo.join("artifacts").join(name),
        "registry artifact package directory",
    )?;
    std::fs::create_dir_all(parent)?;
    ensure_real_directory(parent, "registry artifact parent")?;
    let staging = parent.join(format!(
        ".{}.jet-artifact-{}",
        version,
        unique_suffix()
    ));
    if std::fs::symlink_metadata(&staging).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "registry artifact staging path already exists",
        ));
    }
    let result = (|| {
        copy_artifact_tree(source, &staging)?;
        if SHA256::tree_hash(&staging) != actual {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "staged registry artifact does not match its source hash",
            ));
        }
        std::fs::rename(&staging, &destination)
    })();
    match result {
        Ok(()) => Ok(destination),
        Err(error) => match remove_staging_path(&staging) {
            Ok(()) => Err(error),
            Err(cleanup_error) => Err(io::Error::new(
                error.kind(),
                format!(
                    "{error}; couldn't remove staged registry artifact `{}`: {cleanup_error}",
                    staging.display()
                ),
            )),
        },
    }
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

fn ensure_real_directory(path: &Path, label: &str) -> io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{label} is not a real directory"),
        ));
    }
    Ok(())
}

fn ensure_real_directory_if_present(path: &Path, label: &str) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => ensure_real_directory(path, label),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn remove_staging_path(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            std::fs::remove_file(path)
        }
        Ok(_) => std::fs::remove_dir_all(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
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
/// index, then verify every recorded entry. This all-entry view is required for
/// exact locked yanks: a yank hides a version from new selection but never
/// invalidates an existing lock.
pub fn resolve_and_verify_all(
    registry: &RegistryConfig,
    name: &str,
) -> Result<(Vec<IndexEntry>, Vec<String>), Diagnostic> {
    let repo = ensure_index_clone(registry)?;
    let all = verify_registry_package(&repo, &registry.name, name)?;
    let mut warnings = Vec::new();
    for e in &all {
        verify_entry_tier(e)?;
        warnings.extend(verify_index_entry(
            &all,
            e,
            registry.require_signed || e.tier == RegistryTier::Community,
            &registry.name,
        )?);
    }
    Ok((all, warnings))
}

/// Fetch-side resolution view: return only versions a new dependency may pick.
pub fn resolve_and_verify(
    registry: &RegistryConfig,
    name: &str,
) -> Result<(Vec<IndexEntry>, Vec<String>), Diagnostic> {
    let (all, warnings) = resolve_and_verify_all(registry, name)?;
    let live = all.into_iter().filter(|entry| !entry.yanked).collect();
    Ok((live, warnings))
}

pub fn verify_entry_tier(entry: &IndexEntry) -> Result<(), Diagnostic> {
    if entry.tier == RegistryTier::Community && !entry.gate_status.community_open() {
        return Err(community_gate_error(
            &entry.name,
            &entry.version,
            &entry.gate_status,
        ));
    }
    Ok(())
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

/// E1234 — version immutability (D-VERSION1): a published version can never be
/// overwritten or reused after a yank.
pub fn e1234(name: &str, version: &str) -> Diagnostic {
    Diagnostic::error(
        "E1234",
        format!("`{name}` {version} is already reserved in the registry index"),
        "published versions are immutable (D-VERSION1) — a version can never be overwritten or \
         reused after a yank, so anyone who already locked it keeps building the exact same bytes."
            .to_string(),
        format!(
            "bump the version in `{}` and publish again, or `jet registry yank {version}` the existing \
             one first if it was a mistake (yanking hides it from new resolution; it does not \
             free the version number for reuse).",
            crate::Syntax::PACKAGE_FILE
        ),
        None,
    )
}

/// E1235 — the registry index could not be reached (clone/pull/push failed).
pub fn e1235(url: &str, detail: &str) -> Diagnostic {
    let safe_url = redact_registry_url(url);
    let safe_detail = detail.replace(url, &safe_url);
    Diagnostic::error(
        "E1235",
        format!("couldn't reach the registry index at `{safe_url}`"),
        format!(
            "the git operation against the registry failed (network, auth, or a stale local \
             clone): {safe_detail}"
        ),
        format!(
            "check network access and credentials for `{safe_url}`, or set `JET_REGISTRY_URL` to a \
             reachable mirror."
        ),
        None,
    )
}
