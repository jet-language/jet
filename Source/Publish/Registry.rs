// ──────────────────────────────────────────────
// Private / mirror registry configuration
// ──────────────────────────────────────────────

use crate::Diagnostics::Diagnostic;
use crate::Publish::Index::{self, IndexEntry};
use std::path::{Path, PathBuf};
use std::process::Command;

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
    /// Build the default (well-known) public registry config.
    /// In v1, the URL is advisory — the registry ops are deferred (M8 out of scope).
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

/// Parse registry configs from a simple block in the manifest extra fields.
/// Format: `registries: { name: { url: "...", mirror: true } }`
/// This is the placeholder for the future registry section in pkg.jet.
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
    if dir.join(".git").is_dir() {
        let _ = Command::new("git")
            .args(["pull", "--ff-only"])
            .current_dir(&dir)
            .output();
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

/// Commit the working index tree and push it to the registry. Sets a fallback
/// commit identity so a fresh scratch clone (which has none) can commit. A
/// "nothing to commit" state is idempotent success (re-running publish/yank with
/// bytes already recorded). Any other git failure is E1235.
pub fn push_index(registry: &RegistryConfig, repo: &Path, message: &str) -> Result<(), Diagnostic> {
    let run = |args: &[&str]| Command::new("git").args(args).current_dir(repo).output();
    // A scratch clone may carry no user identity; set one so `commit` works.
    let _ = run(&["config", "user.email", "jet-publish@localhost"]);
    let _ = run(&["config", "user.name", "jet registry publish"]);

    let add = run(&["add", "-A"]).map_err(|e| e1235(&registry.url, &e.to_string()))?;
    if !add.status.success() {
        return Err(e1235(
            &registry.url,
            String::from_utf8_lossy(&add.stderr).trim(),
        ));
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
    let push =
        run(&["push", "origin", "HEAD"]).map_err(|e| e1235(&registry.url, &e.to_string()))?;
    if !push.status.success() {
        return Err(e1235(
            &registry.url,
            String::from_utf8_lossy(&push.stderr).trim(),
        ));
    }
    Ok(())
}

/// D-VERSION1=A: the version already sits in the index and is not yanked —
/// republishing it is refused. Reads the local clone; the caller must
/// `ensure_index_clone` first.
pub fn find_published(repo: &Path, name: &str, version: &str) -> Option<IndexEntry> {
    Index::find_entry(repo, name, version)
}

/// Fetch-side resolution: clone/pull the registry index and return the versions
/// of `name` a resolver may still pick (yanked versions filtered out).
pub fn resolve_from_index(
    registry: &RegistryConfig,
    name: &str,
) -> Result<Vec<IndexEntry>, Diagnostic> {
    let repo = ensure_index_clone(registry)?;
    Ok(Index::non_yanked_entries(&repo, name))
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
    let all = Index::read_entries(&repo, name);
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
