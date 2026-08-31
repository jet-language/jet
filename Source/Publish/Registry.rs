// ──────────────────────────────────────────────
// Private / mirror registry configuration
// ──────────────────────────────────────────────

use super::Tier::{community_gate_error, RegistryTier};
use super::Tuf::verify_registry_package;
use crate::Diagnostics::Diagnostic;
use crate::Publish::Index::{self, IndexEntry};
use crate::Publish::Sign;
use crate::SHA256;
use jet_foundation::JSON::{json_escape, parse_json, JSONValue};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Read};
use std::net::{IpAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const OCI_SBOM_TYPE: &str = "application/vnd.jet.sbom.v1";
const OCI_SIGNATURE_TYPE: &str = "application/vnd.jet.signature.v1";
const OCI_PROVENANCE_TYPE: &str = "application/vnd.jet.provenance.v1";
const OCI_REPRODUCIBILITY_TYPE: &str = "application/vnd.jet.reproducibility.v1";
const OCI_REFERRER_INDEX: &str = "index.json";
const OCI_PENDING_SBOM: &str = ".sbom.pending";
const MAX_OCI_REFERRER_BYTES: usize = 4 * 1024 * 1024;

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

/// Remove embedded user information and URL parameters before a registry
/// endpoint reaches any user-visible diagnostic. Credentials belong in the
/// host Git credential provider, never in a git URL or Jet output.
pub fn redact_registry_url(value: &str) -> String {
    let value = value.split(['?', '#']).next().unwrap_or(value);
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
    // Query and fragment text is never part of a registry identity. Reject it
    // even for transports without a `://` authority (for example file URLs),
    // so it cannot reach Git, locks, or provenance as a hidden credential.
    if value.contains(['?', '#']) {
        return true;
    }
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

#[derive(Debug, Clone)]
enum RegistryTransport {
    Local,
    Network {
        scheme: String,
        host: String,
        port: u16,
        explicit_port: bool,
        address: IpAddr,
    },
}

impl RegistryTransport {
    fn remote(&self, url: &str) -> String {
        let Self::Network {
            scheme,
            port,
            explicit_port,
            address,
            ..
        } = self
        else {
            return url.to_string();
        };
        if scheme != "git" {
            return url.to_string();
        }
        let Some((url_scheme, rest)) = url.split_once("://") else {
            return url.to_string();
        };
        let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let suffix = &rest[authority_end..];
        let port = if *explicit_port {
            format!(":{port}")
        } else {
            String::new()
        };
        format!("{url_scheme}://{}{}{}", registry_ip(*address), port, suffix)
    }

    fn command(&self) -> Command {
        let mut command = jetpack::Provider::hardened_git_command();
        let Self::Network {
            scheme,
            host,
            port,
            address,
            ..
        } = self
        else {
            return command;
        };
        if matches!(scheme.as_str(), "http" | "https") {
            for value in [
                "http.followRedirects=false".to_string(),
                "http.proxy=".to_string(),
                "http.sslVerify=true".to_string(),
                format!(
                    "http.curloptResolve={}:{}:{}",
                    registry_host(host),
                    port,
                    registry_curl_address(*address)
                ),
            ] {
                command.args(["-c", &value]);
            }
        } else if scheme == "ssh" {
            command.env(
                "GIT_SSH_COMMAND",
                format!(
                    "{} -oHostName={address}",
                    jetpack::Provider::hardened_ssh_command()
                ),
            );
        }
        command
    }
}

fn validate_registry_transport(registry: &RegistryConfig) -> Result<RegistryTransport, Diagnostic> {
    if registry_url_has_credentials(&registry.url) {
        return Err(e1235(
            &registry.url,
            "registry URLs must not contain embedded credentials or query/fragment parameters",
        ));
    }
    if let Some(path) = registry.url.strip_prefix("file://") {
        if path.is_empty() || !path.starts_with('/') || path.contains(['?', '#', '\\']) {
            return Err(e1235(
                &registry.url,
                "file registry URLs must name an absolute local repository",
            ));
        }
        return Ok(RegistryTransport::Local);
    }
    let (scheme, rest) = registry.url.split_once("://").ok_or_else(|| {
        e1235(
            &registry.url,
            "registry URL must use file://, https://, http://, git://, or ssh://",
        )
    })?;
    let scheme = scheme.to_ascii_lowercase();
    if !matches!(scheme.as_str(), "git" | "http" | "https" | "ssh") {
        return Err(e1235(
            &registry.url,
            "the registry transport scheme is not allowed",
        ));
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() || authority.contains('@') {
        return Err(e1235(
            &registry.url,
            "registry URL must not contain embedded credentials",
        ));
    }
    let (host, explicit_port) = registry_host_and_port(authority).map_err(|detail| {
        e1235(
            &registry.url,
            &format!("invalid registry transport: {detail}"),
        )
    })?;
    let port = explicit_port.unwrap_or(match scheme.as_str() {
        "https" => 443,
        "http" => 80,
        "git" => 9418,
        _ => 22,
    });
    let endpoint = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    let addresses = endpoint
        .to_socket_addrs()
        .map_err(|error| {
            e1235(
                &registry.url,
                &format!("could not resolve registry host: {error}"),
            )
        })?
        .collect::<Vec<_>>();
    if addresses.is_empty()
        || addresses
            .iter()
            .any(|address| !registry_public_ip(address.ip()))
    {
        return Err(e1235(
            &registry.url,
            "registry host resolves to a non-public address",
        ));
    }
    Ok(RegistryTransport::Network {
        scheme,
        host,
        port,
        explicit_port: explicit_port.is_some(),
        address: addresses[0].ip(),
    })
}

fn git_command(transport: &RegistryTransport) -> Command {
    let mut command = transport.command();
    // Git's configured credential helper is the host-owned provider. Keep its
    // request scoped to this repository path; the secret crosses only Git's
    // helper pipe and never becomes a Jet argument or environment value.
    command.args(["-c", "credential.useHttpPath=true"]);
    command.env("GIT_TERMINAL_PROMPT", "0");
    // Registry configuration is an input to Jet, not to Git. In particular,
    // do not let a secret-shaped registry variable cross into a credential
    // helper's process environment; the host helper receives only Git's
    // path-scoped stdin request.
    for (key, _) in std::env::vars_os() {
        let key = key.to_string_lossy();
        if key.starts_with("JET_REGISTRY_") {
            command.env_remove(&*key);
        }
    }
    command
}

fn registry_ip(address: IpAddr) -> String {
    match address {
        IpAddr::V4(address) => address.to_string(),
        IpAddr::V6(address) => format!("[{address}]"),
    }
}

fn registry_curl_address(address: IpAddr) -> String {
    match address {
        IpAddr::V4(address) => address.to_string(),
        IpAddr::V6(address) => format!("[{address}]"),
    }
}

fn registry_host(host: &str) -> String {
    if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

fn registry_host_and_port(authority: &str) -> Result<(String, Option<u16>), String> {
    if let Some(host) = authority.strip_prefix('[') {
        let (host, suffix) = host
            .split_once(']')
            .ok_or_else(|| "IPv6 host is not closed".to_string())?;
        if host.is_empty() || (!suffix.is_empty() && !suffix.starts_with(':')) {
            return Err("IPv6 host is malformed".to_string());
        }
        return Ok((
            host.to_string(),
            parse_registry_port(suffix.strip_prefix(':'))?,
        ));
    }
    let (host, port) = authority
        .rsplit_once(':')
        .map(|(host, port)| (host, Some(port)))
        .unwrap_or((authority, None));
    if host.is_empty() || host.contains(':') || host.chars().any(char::is_whitespace) {
        return Err("host is malformed".to_string());
    }
    Ok((host.to_string(), parse_registry_port(port)?))
}

fn parse_registry_port(port: Option<&str>) -> Result<Option<u16>, String> {
    port.map(|port| {
        let port = port
            .parse::<u16>()
            .map_err(|_| "port is malformed".to_string())?;
        (port != 0)
            .then_some(port)
            .ok_or_else(|| "port must not be zero".to_string())
    })
    .transpose()
}

fn registry_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(a == 0
                || a == 10
                || a == 100 && (b & 0xc0) == 0x40
                || a == 127
                || a == 169 && b == 254
                || a == 172 && (16..=31).contains(&b)
                || a == 192 && b == 0 && c == 0
                || a == 192 && b == 0 && c == 2
                || a == 192 && b == 168
                || a == 198 && (18..=19).contains(&b)
                || a == 198 && b == 51 && c == 100
                || a == 203 && b == 0 && c == 113
                || a >= 224)
        }
        IpAddr::V6(ip) => {
            if let Some(ipv4) = ip.to_ipv4_mapped() {
                return registry_public_ip(IpAddr::V4(ipv4));
            }
            let [first, second, ..] = ip.segments();
            (first & 0xe000) == 0x2000
                && !ip.is_unspecified()
                && !ip.is_loopback()
                && !ip.is_multicast()
                && (first & 0xfe00) != 0xfc00
                && (first & 0xffc0) != 0xfe80
                && !(first == 0x2001 && second == 0x0db8)
        }
    }
}

/// Host-pinned root key location for a registry. The registry name is hashed
/// before it becomes a path component, so repository or environment input can
/// never escape the host trust directory.
pub fn registry_root_key_path(registry_name: &str) -> PathBuf {
    let base = Sign::keys_dir()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("registry-roots").join(format!(
        "{}.pub",
        SHA256::sha256_hex(registry_name.as_bytes())
    ))
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
    base.join("registry-checkpoints").join(format!(
        "{}.state",
        SHA256::sha256_hex(registry_name.as_bytes())
    ))
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
    let path = configured_registry_root_key_path(registry_name).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "registry root key path is empty",
        )
    })?;
    if let Ok(metadata) = std::fs::symlink_metadata(&path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "registry root key is not a regular file",
            ));
        }
        let existing =
            String::from_utf8(read_registry_file_nofollow(&path, 4096)?).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "registry root key is not UTF-8")
            })?;
        if existing.trim() != public_key {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "registry root key pin changed",
            ));
        }
        return Ok(path);
    }
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "registry root key has no parent",
        )
    })?;
    if configured {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "configured registry root key is unavailable at {}",
                path.display()
            ),
        ));
    }
    std::fs::create_dir_all(parent)?;
    ensure_real_directory(parent, "registry root-key directory")?;
    let _lock = acquire_registry_root_key_lock(&path)?;

    // Re-check after taking the lock. A concurrent publisher may have
    // installed the pin while this process waited. The old create_new/write
    // sequence exposed an empty final file to that publisher, which looked
    // like a changed root key instead of the one shared immutable pin.
    if let Ok(metadata) = std::fs::symlink_metadata(&path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "registry root key is not a regular file",
            ));
        }
        let existing =
            String::from_utf8(read_registry_file_nofollow(&path, 4096)?).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "registry root key is not UTF-8")
            })?;
        if existing.trim() != public_key {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "registry root key pin changed",
            ));
        }
        return Ok(path);
    }

    let partial = parent.join(format!(
        ".{}.partial-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("root"),
        unique_suffix(),
    ));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        if !add_registry_nofollow_flags(&mut options) {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "no-follow registry key publication is unavailable on this platform",
            ));
        }
        let mut file = options.open(&partial)?;
        use std::io::Write;
        file.write_all(public_key.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&partial, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(&partial, &path)?;
        sync_registry_directory(parent)?;
        Ok::<_, io::Error>(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    result?;
    Ok(path)
}

struct RegistryRootKeyLock(PathBuf);

impl Drop for RegistryRootKeyLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn acquire_registry_root_key_lock(path: &Path) -> io::Result<RegistryRootKeyLock> {
    let lock_path = path.with_extension("lock");
    for _ in 0..100 {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(file) => {
                if let Err(error) = file.sync_all() {
                    drop(file);
                    let _ = std::fs::remove_file(&lock_path);
                    return Err(error);
                }
                return Ok(RegistryRootKeyLock(lock_path));
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let stale = std::fs::metadata(&lock_path)
                    .ok()
                    .and_then(|metadata| metadata.modified().ok())
                    .and_then(|modified| modified.elapsed().ok())
                    .is_some_and(|age| age > std::time::Duration::from_secs(300));
                if stale {
                    let _ = std::fs::remove_file(&lock_path);
                    continue;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "registry root-key pin is busy",
    ))
}

pub fn read_registry_root_key(registry_name: &str) -> io::Result<String> {
    let path = configured_registry_root_key_path(registry_name).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "registry root key path is empty",
        )
    })?;
    let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "registry root key is not installed at {}: {error}",
                path.display()
            ),
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "registry root key is not a regular file",
        ));
    }
    let key = String::from_utf8(read_registry_file_nofollow(&path, 4096)?)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "registry root key is not UTF-8"))?
        .trim()
        .to_string();
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
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
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

fn clone_registry_to(
    registry: &RegistryConfig,
    path: &Path,
    transport: &RegistryTransport,
) -> Result<(), Diagnostic> {
    let path_display = path.to_string_lossy().into_owned();
    let remote = transport.remote(&registry.url);
    let output = git_command(transport)
        .args(["clone", "--", &remote, &path_display])
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
                        e1235(
                            &registry.url,
                            "couldn't remove the losing partial registry clone",
                        ),
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

fn validate_existing_index_clone(
    registry: &RegistryConfig,
    dir: &Path,
    transport: &RegistryTransport,
) -> Result<bool, Diagnostic> {
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
        let origin = git_command(transport)
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

    Ok(existing)
}

fn ensure_index_clone_locked(
    registry: &RegistryConfig,
    dir: &Path,
    parent: &Path,
    transport: &RegistryTransport,
) -> Result<PathBuf, Diagnostic> {
    let existing = validate_existing_index_clone(registry, dir, transport)?;

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
    clone_registry_to(registry, &partial, transport)?;
    install_registry_clone(registry, dir, parent, &partial, existing)
}

/// Clone the registry index into a private sibling, then install it by rename.
/// Refreshing an existing cache uses the same staged replacement, so an
/// interrupted or failed fetch leaves the previous verified clone untouched.
pub fn ensure_index_clone(registry: &RegistryConfig) -> Result<PathBuf, Diagnostic> {
    let transport = validate_registry_transport(registry)?;
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
    ensure_index_clone_locked(registry, &dir, parent, &transport)
}

/// Return the already-installed registry clone without contacting its remote.
/// Locked and offline consumers must validate the cache's real git origin
/// before trusting its index, metadata, or artifacts.
pub fn ensure_local_index_clone(registry: &RegistryConfig) -> Result<PathBuf, Diagnostic> {
    let transport = validate_registry_transport(registry)?;
    let dir = index_repo_path(registry);
    if !validate_existing_index_clone(registry, &dir, &transport)? {
        return Err(e1235(
            &registry.url,
            "the local registry cache is unavailable; locked mode never downloads a new index",
        ));
    }
    Ok(dir)
}

/// A short-lived clean checkout used for one registry mutation. The ordinary
/// resolver cache is never used as a publication worktree: credentials,
/// editor files, and another publisher's partial state cannot be swept into a
/// commit by accident.
pub struct PublishCheckout {
    path: PathBuf,
    cleanup_root: PathBuf,
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
        cleanup_publish_checkout(&self.cleanup_root, self.owns_path)
    }
}

impl Drop for PublishCheckout {
    fn drop(&mut self) {
        if self.cleanup_attempted.replace(true) {
            return;
        }
        if let Err(error) = cleanup_publish_checkout(&self.cleanup_root, self.owns_path) {
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
    let detail = format!("cleanup failed: {}", checkout_cleanup_problem(path, error));
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
    // Git and its host credential helper may write arbitrary sensitive text.
    // Exit status is enough for the typed E1235 diagnostic; raw tool output
    // belongs outside Jet's user-visible error surface.
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
    let transport = validate_registry_transport(registry)?;
    let parent = std::env::temp_dir();
    let checkout_root = jetpack::Provider::exclusive_temp_dir(&parent, "jet-registry-publish")
        .map_err(|error| {
            e1235(
                &registry.url,
                &format!("could not create publication checkout: {error}"),
            )
        })?;
    let path = checkout_root.join("checkout");
    let path_display = path.to_string_lossy().into_owned();
    let remote = transport.remote(&registry.url);
    let output = git_command(&transport)
        .args(["clone", "--", &remote, &path_display])
        .output();
    let output = match output {
        Ok(output) => output,
        Err(error) => {
            return Err(clone_failure(
                registry,
                &checkout_root,
                true,
                error.to_string(),
            ));
        }
    };
    if !output.status.success() {
        return Err(clone_failure(
            registry,
            &checkout_root,
            true,
            clone_output_detail(&output),
        ));
    }
    Ok(PublishCheckout {
        path,
        cleanup_root: checkout_root,
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
    let transport = validate_registry_transport(registry)?;
    let run = |args: &[&str]| {
        git_command(&transport)
            .args(args)
            .current_dir(repo)
            .output()
    };
    // A scratch clone may carry no user identity; set one so `commit` works.
    let _ = run(&["config", "user.email", "jet-publish@localhost"]);
    let _ = run(&["config", "user.name", "jet registry publish"]);

    let cached_clean =
        run(&["diff", "--cached", "--quiet"]).map_err(|e| e1235(&registry.url, &e.to_string()))?;
    if !cached_clean.status.success() {
        return Err(e1235(
            &registry.url,
            "publication checkout contains pre-staged changes",
        ));
    }
    if paths.is_empty() {
        return Err(e1235(&registry.url, "publication has no explicit paths"));
    }
    let mut stage_paths = paths.to_vec();
    if let Some(entry) = expected.filter(|entry| !entry.yanked) {
        let referrers = existing_oci_referrer_root(repo, &entry.content_hash)
            .map_err(|error| e1235(&registry.url, &error.to_string()))?;
        if !referrers.join(OCI_REFERRER_INDEX).is_file() {
            return Err(e1235(
                &registry.url,
                "publication has no complete OCI referrer set; restore or rebuild the package evidence",
            ));
        }
        stage_paths.push(referrers);
    }
    let mut add = git_command(&transport);
    add.args(["add", "--"]);
    for path in &stage_paths {
        let relative = path
            .strip_prefix(repo)
            .map_err(|_| e1235(&registry.url, "publication path escapes its checkout"))?;
        add.arg(relative);
    }
    let add = add
        .current_dir(repo)
        .output()
        .map_err(|e| e1235(&registry.url, &e.to_string()))?;
    if !add.status.success() {
        return Err(e1235(&registry.url, "git add failed"));
    }
    let staged = run(&["diff", "--cached", "--name-only"])
        .map_err(|e| e1235(&registry.url, &e.to_string()))?;
    if !staged.status.success() {
        return Err(e1235(&registry.url, "git staged-path inspection failed"));
    }
    let allowed = stage_paths
        .iter()
        .filter_map(|path| path.strip_prefix(repo).ok())
        .map(|path| {
            path.to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/")
        })
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
            return Err(e1235(&registry.url, "git commit failed"));
        }
    }
    let branch = run(&["rev-parse", "--abbrev-ref", "HEAD"])
        .map_err(|e| e1235(&registry.url, &e.to_string()))?;
    let branch = String::from_utf8_lossy(&branch.stdout).trim().to_string();
    if branch.is_empty() || branch == "HEAD" || branch == "." || branch == ".." {
        return Err(e1235(
            &registry.url,
            "publication checkout has no named branch",
        ));
    }
    let push = run(&["push", "origin", &format!("HEAD:refs/heads/{branch}")])
        .map_err(|e| e1235(&registry.url, &e.to_string()))?;
    if !push.status.success() {
        // A concurrent publisher may have won the immutable version. Accept
        // only a byte-identical winner. Otherwise rebuild from a fresh remote
        // checkout; rebasing append-only index files can merge two conflicting
        // same-version lines and would violate immutable identity.
        let fetch = run(&["fetch", "origin"]).map_err(|e| e1235(&registry.url, &e.to_string()))?;
        if !fetch.status.success() {
            return Err(e1235(
                &registry.url,
                "git fetch failed after a concurrent push",
            ));
        }
        let remote = format!("origin/{branch}");
        if let Some(entry) = expected {
            if remote_contains_entry(repo, &remote, entry, &transport)
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
        crate::Publish::snapshot_verified_artifact(repo, &actual)
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

fn remote_contains_entry(
    repo: &Path,
    remote: &str,
    expected: &IndexEntry,
    transport: &RegistryTransport,
) -> bool {
    let path = match Index::index_entry_path(repo, &expected.name) {
        Ok(path) => path,
        Err(_) => return false,
    };
    let relative = match path.strip_prefix(repo) {
        Ok(path) => path
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/"),
        Err(_) => return false,
    };
    let spec = format!("{remote}:{relative}");
    let output = match git_command(transport)
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
            Ok(path) => path
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/"),
            Err(_) => return false,
        },
        Err(_) => return false,
    };
    if !git_command(transport)
        .args(["cat-file", "-e", &format!("{remote}:{artifact}")])
        .current_dir(repo)
        .output()
        .is_ok_and(|output| output.status.success())
    {
        return false;
    }
    let package_metadata =
        match crate::Publish::registry_package_metadata_path(repo, &expected.name) {
            Ok(path) => path,
            Err(_) => return false,
        };
    let log = repo.join("transparency").join("log");
    let checkpoint = repo.join("transparency").join("checkpoint");
    [package_metadata, log, checkpoint].iter().all(|path| {
        path.strip_prefix(repo)
            .ok()
            .map(|relative| {
                relative
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/")
            })
            .is_some_and(|relative| {
                git_command(transport)
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
    // Keep this older convenience API on the same trust path as the live
    // resolver. It must not expose a TUF/OCI-verified entry while bypassing
    // the publisher signature and community gate checks.
    let (entries, _warnings) = resolve_and_verify(registry, name)?;
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

/// A verified registry source snapshot. The registry checkout may be
/// refreshed or replaced after this value is created; callers consume the
/// held snapshot path instead of reopening the checkout artifact.
pub struct ArtifactSnapshot {
    path: PathBuf,
    cleanup_root: PathBuf,
    content_hash: String,
}

impl ArtifactSnapshot {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn content_hash(&self) -> &str {
        &self.content_hash
    }
}

impl Drop for ArtifactSnapshot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.cleanup_root);
    }
}

fn snapshot_registry_tree(scratch_parent: &Path, source: &Path) -> io::Result<ArtifactSnapshot> {
    // Keep package snapshots on the registry checkout's disk. `/tmp` is a
    // RAM-backed tmpfs on supported Jet hosts and is not an OOM-safe scratch
    // location for a hostile artifact.
    let cleanup_root =
        jetpack::Provider::exclusive_temp_dir(scratch_parent, "jet-registry-artifact")?;
    let path = cleanup_root.join("tree");
    let result = (|| {
        copy_artifact_tree(source, &path)?;
        copy_publish_lock(source, &path)?;
        let hash = registry_artifact_hash(&path)?;
        Ok((path.clone(), hash))
    })();
    match result {
        Ok((path, hash)) => Ok(ArtifactSnapshot {
            path,
            cleanup_root,
            content_hash: hash,
        }),
        Err(error) => {
            let _ = std::fs::remove_dir_all(&cleanup_root);
            Err(error)
        }
    }
}

/// Verify an indexed artifact, then copy and re-hash it into an exclusive
/// snapshot before any caller reads package or registry metadata.
pub fn snapshot_verified_artifact(repo: &Path, entry: &IndexEntry) -> io::Result<ArtifactSnapshot> {
    let source = verify_artifact(repo, entry)?;
    let snapshot = snapshot_registry_tree(repo, &source)?;
    if !entry.content_hash.is_empty() && snapshot.content_hash() != entry.content_hash {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "registry source artifact for {} {} changed while being snapshotted",
                entry.name, entry.version
            ),
        ));
    }
    Ok(snapshot)
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
    let source_snapshot = snapshot_registry_tree(repo, source)?;
    let source = source_snapshot.path();
    validate_registry_metadata_file(source, name, version)?;
    let actual = registry_artifact_hash(source)?;
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
        if registry_artifact_hash(&destination)? != actual {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "registry artifact already exists with conflicting source bytes",
            ));
        }
        stage_oci_sbom(repo, source, name, version, &actual)?;
        finalize_oci_referrers_for_package(repo, name, version)?;
        return Ok(destination);
    }
    if destination.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "registry artifact path is not a directory",
        ));
    }
    let parent = destination.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "registry artifact has no parent",
        )
    })?;
    ensure_real_directory(repo, "registry checkout")?;
    ensure_real_directory_if_present(&repo.join("artifacts"), "registry artifact root")?;
    ensure_real_directory_if_present(
        &repo.join("artifacts").join(name),
        "registry artifact package directory",
    )?;
    std::fs::create_dir_all(parent)?;
    ensure_real_directory(parent, "registry artifact parent")?;
    let staging = parent.join(format!(".{}.jet-artifact-{}", version, unique_suffix()));
    if std::fs::symlink_metadata(&staging).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "registry artifact staging path already exists",
        ));
    }
    let subject_root = repo.join("referrers").join(&actual);
    let subject_root_existed = std::fs::symlink_metadata(&subject_root).is_ok();
    let pending = subject_root.join(OCI_PENDING_SBOM);
    let pending_existed = std::fs::symlink_metadata(&pending).is_ok();
    let mut artifact_published = false;
    let result = (|| {
        // Stage all content-derived evidence before the artifact becomes
        // visible. An invalid lock or an oversized evidence payload therefore
        // cannot leave an apparently published source tree behind.
        stage_oci_sbom(repo, source, name, version, &actual)?;
        copy_artifact_tree(source, &staging)?;
        if registry_artifact_hash(&staging)? != actual {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "staged registry artifact does not match its source hash",
            ));
        }
        std::fs::rename(&staging, &destination)?;
        artifact_published = true;
        finalize_oci_referrers_for_package(repo, name, version)
    })();
    match result {
        Ok(()) => Ok(destination),
        Err(error) => match remove_staging_path(&staging) {
            Ok(()) => {
                if !pending_existed {
                    let _ = std::fs::remove_file(&pending);
                }
                if artifact_published && !subject_root_existed {
                    let _ = remove_staging_path(&destination);
                }
                if !subject_root_existed {
                    let _ = std::fs::remove_dir_all(&subject_root);
                }
                Err(error)
            }
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

fn load_publish_lock(source: &Path) -> io::Result<Option<crate::Lock::LockFile>> {
    let managed = source.join(".jet");
    match std::fs::symlink_metadata(&managed) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "package .jet metadata directory must be a real directory",
            ))
        }
        Ok(_) => {
            let path = managed.join("lock");
            match std::fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                    Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "package lock must be a regular file",
                    ))
                }
                Ok(_) => {
                    let raw = std::fs::read_to_string(&path)?;
                    crate::Lock::parse(&raw).map(Some).map_err(|error| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("package lock is malformed: {error}"),
                        )
                    })
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(error),
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn oci_referrer_root(repo: &Path, subject: &str) -> io::Result<PathBuf> {
    validate_oci_digest(subject)?;
    let root = repo.join("referrers");
    match std::fs::symlink_metadata(&root) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "registry OCI referrer root is not a real directory",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            std::fs::create_dir_all(&root)?;
        }
        Err(error) => return Err(error),
    }
    let root_metadata = std::fs::symlink_metadata(&root)?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "registry OCI referrer root is not a real directory",
        ));
    }
    let subject_root = root.join(subject);
    match std::fs::symlink_metadata(&subject_root) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "registry OCI subject referrers are not a real directory",
            ))
        }
        Ok(_) => Ok(subject_root),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            std::fs::create_dir(&subject_root)?;
            Ok(subject_root)
        }
        Err(error) => Err(error),
    }
}

fn existing_oci_referrer_root(repo: &Path, subject: &str) -> io::Result<PathBuf> {
    validate_oci_digest(subject)?;
    let root = repo.join("referrers");
    let root_metadata = std::fs::symlink_metadata(&root)?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "registry OCI referrer root is not a real directory",
        ));
    }
    let subject_root = root.join(subject);
    let subject_metadata = std::fs::symlink_metadata(&subject_root)?;
    if subject_metadata.file_type().is_symlink() || !subject_metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "registry OCI subject referrers are not a real directory",
        ));
    }
    Ok(subject_root)
}

fn validate_oci_digest(value: &str) -> io::Result<()> {
    let Some(hex) = value.strip_prefix("sha256-") else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "OCI referrer subject is not a sha256- digest",
        ));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "OCI referrer subject is not a complete sha256- digest",
        ));
    }
    Ok(())
}

fn stage_oci_sbom(
    repo: &Path,
    source: &Path,
    name: &str,
    version: &str,
    subject: &str,
) -> io::Result<()> {
    let root = oci_referrer_root(repo, subject)?;
    let lock = load_publish_lock(source)?;
    let sbom = super::SBOM::registry_spdx(lock.as_ref(), name, version, subject);
    write_oci_file(
        &root.join(OCI_PENDING_SBOM),
        sbom.as_bytes(),
        "pending OCI SBOM",
    )
}

fn finalize_oci_referrers_for_package(repo: &Path, name: &str, version: &str) -> io::Result<()> {
    let Some(entry) = Index::find_entry(repo, name, version)? else {
        return Ok(());
    };
    finalize_oci_referrers(repo, &entry)
}

/// Finish the referrer set after both the immutable artifact and index line
/// exist. Index::write_index_entry also calls this for the normal publish
/// order; publish_artifact calls it for the race-rebuild order.
pub(super) fn finalize_oci_referrers(repo: &Path, entry: &IndexEntry) -> io::Result<()> {
    let artifact = artifact_path(repo, &entry.name, &entry.version)?;
    let artifact_metadata = match std::fs::symlink_metadata(&artifact) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if artifact_metadata.file_type().is_symlink() || !artifact_metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "registry OCI referrers require a real source artifact",
        ));
    }
    validate_oci_digest(&entry.content_hash)?;
    let root = oci_referrer_root(repo, &entry.content_hash)?;
    let pending = root.join(OCI_PENDING_SBOM);
    let sbom = match read_oci_file(&pending, MAX_OCI_REFERRER_BYTES) {
        Ok(bytes) => String::from_utf8(bytes).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "registry OCI SBOM is not UTF-8")
        })?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            // A missing staged SBOM is an incomplete publication, not an
            // invitation to synthesize a weaker fallback. The lock-backed
            // payload must remain the exact evidence chosen by the publisher.
            if root.join(OCI_REFERRER_INDEX).is_file() {
                verify_oci_referrers(repo, entry)?;
                return Ok(());
            }
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "registry OCI SBOM evidence was not staged; restore or rebuild the immutable evidence set",
            ));
        }
        Err(error) => return Err(error),
    };
    if !sbom.contains(&format!("# JetSubject: {}\n", entry.content_hash))
        || !sbom.contains(&format!("PackageName: {}\n", entry.name))
        || !sbom.contains(&format!("PackageVersion: {}\n", entry.version))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "registry OCI SBOM is not bound to its package subject",
        ));
    }
    let signature = oci_signature_payload(entry)?;
    let provenance = oci_provenance_payload(entry)?;
    let reproducibility = oci_reproducibility_payload(entry)?;
    let payloads = [
        (OCI_SBOM_TYPE, sbom.into_bytes()),
        (OCI_SIGNATURE_TYPE, signature.into_bytes()),
        (OCI_PROVENANCE_TYPE, provenance.into_bytes()),
        (OCI_REPRODUCIBILITY_TYPE, reproducibility.into_bytes()),
    ];
    let mut descriptors = Vec::with_capacity(payloads.len());
    for (artifact_type, bytes) in &payloads {
        let digest = format!("sha256-{}", SHA256::sha256_hex(bytes));
        write_oci_file(
            &root.join("blobs").join(&digest),
            bytes,
            "OCI referrer blob",
        )?;
        descriptors.push((*artifact_type, digest.clone(), bytes.len()));
    }
    let index = render_oci_index(&entry.content_hash, &descriptors);
    write_oci_file(
        &root.join(OCI_REFERRER_INDEX),
        index.as_bytes(),
        "OCI referrer index",
    )?;
    if std::fs::symlink_metadata(&pending).is_ok() {
        std::fs::remove_file(&pending)?;
    }
    Ok(())
}

fn oci_signature_payload(entry: &IndexEntry) -> io::Result<String> {
    validate_evidence_value(&entry.name, "package name")?;
    validate_evidence_value(&entry.version, "package version")?;
    validate_evidence_value(&entry.public_key, "publisher key")?;
    validate_evidence_value(&entry.signature, "publisher signature")?;
    Ok(format!(
        "jet-oci-signature-v1\nsubject={}\nname={}\nversion={}\npublic-key={}\nsignature={}\nstatus={}\n",
        entry.content_hash,
        entry.name,
        entry.version,
        entry.public_key,
        entry.signature,
        if entry.signature.is_empty() { "unsigned" } else { "signed" },
    ))
}

fn oci_provenance_payload(entry: &IndexEntry) -> io::Result<String> {
    let tier = entry.tier.label();
    let gate_status = entry.gate_status.summary();
    validate_evidence_value(tier, "registry tier")?;
    validate_evidence_value(&gate_status, "registry gate status")?;
    Ok(format!(
        "jet-oci-provenance-v1\nsubject={}\npackage={}#{}\ncontent-hash={}\nfingerprint={}\ntier={}\ngate-status={}\n",
        entry.content_hash,
        entry.name,
        entry.version,
        entry.content_hash,
        entry.fingerprint,
        tier,
        gate_status,
    ))
}

fn oci_reproducibility_payload(entry: &IndexEntry) -> io::Result<String> {
    validate_evidence_value(&entry.fingerprint, "package fingerprint")?;
    Ok(format!(
        "jet-oci-reproducibility-v1\nsubject={}\nsource-hash={}\nfingerprint={}\nstatus=verified\n",
        entry.content_hash, entry.content_hash, entry.fingerprint,
    ))
}

fn validate_evidence_value(value: &str, label: &str) -> io::Result<()> {
    if value.bytes().any(|byte| byte == b'\n' || byte == b'\r') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("registry {label} contains a line break"),
        ));
    }
    Ok(())
}

fn write_oci_file(path: &Path, bytes: &[u8], label: &str) -> io::Result<()> {
    if bytes.len() > MAX_OCI_REFERRER_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{label} exceeds the 4 MiB limit"),
        ));
    }
    if let Ok(metadata) = std::fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{label} path is not a regular file"),
            ));
        }
        let existing = std::fs::read(path)?;
        if existing == bytes {
            return Ok(());
        }
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("immutable {label} changed"),
        ));
    }
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{label} has no parent"),
        )
    })?;
    std::fs::create_dir_all(parent)?;
    ensure_real_directory(parent, "OCI referrer parent")?;
    let partial = parent.join(format!(
        ".partial-{}-{}",
        std::process::id(),
        unique_suffix()
    ));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial)?;
        use std::io::Write;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&partial, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    match result {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            write_oci_file(path, bytes, label)
        }
        Err(error) => Err(error),
    }
}

fn read_oci_file(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "OCI referrer payload is not a regular file",
        ));
    }
    if metadata.len() > limit as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "OCI referrer payload exceeds its size limit",
        ));
    }
    std::fs::read(path)
}

pub(super) fn referrer_index_digest(repo: &Path, entry: &IndexEntry) -> io::Result<String> {
    let root = existing_oci_referrer_root(repo, &entry.content_hash).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("OCI referrer index is unavailable: {error}"),
        )
    })?;
    let bytes =
        read_oci_file(&root.join(OCI_REFERRER_INDEX), MAX_OCI_REFERRER_BYTES).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("OCI referrer index is unavailable: {error}"),
            )
        })?;
    Ok(format!("sha256-{}", SHA256::sha256_hex(&bytes)))
}

fn render_oci_index(subject: &str, descriptors: &[(&str, String, usize)]) -> String {
    let manifests = descriptors
        .iter()
        .map(|(artifact_type, digest, size)| {
            format!(
                "{{\"artifactType\":\"{}\",\"digest\":\"{}\",\"mediaType\":\"{}\",\"size\":{}}}",
                json_escape(*artifact_type),
                json_escape(digest),
                json_escape(*artifact_type),
                size,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schemaVersion\":2,\"subject\":{{\"digest\":\"{}\"}},\"manifests\":[{}]}}\n",
        json_escape(subject),
        manifests
    )
}

/// Verify the complete immutable OCI referrer set before a registry result is
/// allowed into resolution. Every descriptor is subject-bound, content
/// addressed, and cross-checked against the index entry's signed facts.
pub(super) fn verify_oci_referrers(repo: &Path, entry: &IndexEntry) -> io::Result<()> {
    validate_oci_digest(&entry.content_hash)?;
    let root = existing_oci_referrer_root(repo, &entry.content_hash).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "OCI referrer set is unavailable: {error}; restore it from the registry or republish the immutable package"
            ),
        )
    })?;
    let index = read_oci_file(&root.join(OCI_REFERRER_INDEX), MAX_OCI_REFERRER_BYTES)?;
    let descriptors = parse_oci_index(&index, &entry.content_hash)?;
    let blobs = root.join("blobs");
    ensure_real_directory(&blobs, "OCI referrer blob store")?;
    let mut referenced = BTreeSet::new();
    for (artifact_type, digest, size) in &descriptors {
        if !referenced.insert(digest.clone()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "OCI referrer index repeats a blob digest",
            ));
        }
        let bytes = read_oci_file(&blobs.join(digest), MAX_OCI_REFERRER_BYTES)?;
        if bytes.len() as u64 != *size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("OCI referrer {artifact_type} has the wrong blob size"),
            ));
        }
        let actual = format!("sha256-{}", SHA256::sha256_hex(&bytes));
        if actual != *digest {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("OCI referrer {artifact_type} failed its blob digest"),
            ));
        }
        let expected = match artifact_type.as_str() {
            OCI_SBOM_TYPE => None,
            OCI_SIGNATURE_TYPE => Some(oci_signature_payload(entry)?.into_bytes()),
            OCI_PROVENANCE_TYPE => Some(oci_provenance_payload(entry)?.into_bytes()),
            OCI_REPRODUCIBILITY_TYPE => Some(oci_reproducibility_payload(entry)?.into_bytes()),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "OCI referrer index contains an unknown artifact type",
                ));
            }
        };
        if let Some(expected) = expected {
            if bytes != expected {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("OCI referrer {artifact_type} is not bound to the index entry"),
                ));
            }
        } else {
            let sbom = String::from_utf8(bytes).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "OCI SBOM referrer is not UTF-8")
            })?;
            if !sbom.contains(&format!("# JetSubject: {}\n", entry.content_hash))
                || !sbom.contains(&format!("PackageName: {}\n", entry.name))
                || !sbom.contains(&format!("PackageVersion: {}\n", entry.version))
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "OCI SBOM referrer is not bound to the index entry",
                ));
            }
        }
    }
    let expected_blobs = referenced;
    for child in std::fs::read_dir(&blobs)? {
        let child = child?;
        let name = child.file_name().to_string_lossy().into_owned();
        let metadata = std::fs::symlink_metadata(child.path())?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || !expected_blobs.contains(&name)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "OCI referrer blob store contains an unreferenced or unsafe blob",
            ));
        }
    }
    for child in std::fs::read_dir(&root)? {
        let child = child?;
        let name = child.file_name().to_string_lossy().into_owned();
        if name != OCI_REFERRER_INDEX && name != "blobs" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "OCI subject referrer directory contains unexpected metadata",
            ));
        }
    }
    Ok(())
}

fn parse_oci_index(bytes: &[u8], subject: &str) -> io::Result<Vec<(String, String, u64)>> {
    let text = std::str::from_utf8(bytes).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "OCI referrer index is not UTF-8",
        )
    })?;
    let value = parse_json(text).map_err(|_| invalid_oci("OCI referrer index is malformed"))?;
    let object = value
        .as_object()
        .map_err(|_| invalid_oci("OCI referrer index is not an object"))?;
    require_oci_keys(object, &["schemaVersion", "subject", "manifests"], "index")?;
    if !matches!(object.get("schemaVersion"), Some(JSONValue::Number(2))) {
        return Err(invalid_oci(
            "OCI referrer index has an unsupported schema version",
        ));
    }
    let subject_object = object
        .get("subject")
        .ok_or_else(|| invalid_oci("OCI referrer index has no subject"))?
        .as_object()
        .map_err(|_| invalid_oci("OCI referrer subject is not an object"))?;
    require_oci_keys(subject_object, &["digest"], "subject")?;
    let recorded_subject = subject_object
        .get("digest")
        .ok_or_else(|| invalid_oci("OCI referrer subject has no digest"))?
        .as_str()
        .map_err(|_| invalid_oci("OCI referrer subject digest is not a string"))?;
    if recorded_subject != subject {
        return Err(invalid_oci(
            "OCI referrer subject does not match the index entry",
        ));
    }
    let manifests = object
        .get("manifests")
        .ok_or_else(|| invalid_oci("OCI referrer index has no manifests"))?
        .as_array()
        .map_err(|_| invalid_oci("OCI referrer manifests is not an array"))?;
    if manifests.len() != 4 {
        return Err(invalid_oci(
            "OCI referrer index must contain SBOM, signature, provenance, and reproducibility",
        ));
    }
    let mut seen = BTreeSet::new();
    let mut descriptors = Vec::with_capacity(manifests.len());
    for manifest in manifests {
        let manifest = manifest
            .as_object()
            .map_err(|_| invalid_oci("OCI referrer descriptor is not an object"))?;
        require_oci_keys(
            manifest,
            &["artifactType", "digest", "mediaType", "size"],
            "descriptor",
        )?;
        let artifact_type = manifest
            .get("artifactType")
            .ok_or_else(|| invalid_oci("OCI referrer descriptor has no artifact type"))?
            .as_str()
            .map_err(|_| invalid_oci("OCI referrer artifact type is not a string"))?
            .to_string();
        let media_type = manifest
            .get("mediaType")
            .ok_or_else(|| invalid_oci("OCI referrer descriptor has no media type"))?
            .as_str()
            .map_err(|_| invalid_oci("OCI referrer media type is not a string"))?;
        if media_type != artifact_type
            || !matches!(
                artifact_type.as_str(),
                OCI_SBOM_TYPE | OCI_SIGNATURE_TYPE | OCI_PROVENANCE_TYPE | OCI_REPRODUCIBILITY_TYPE
            )
            || !seen.insert(artifact_type.clone())
        {
            return Err(invalid_oci(
                "OCI referrer descriptor has an unknown or repeated artifact type",
            ));
        }
        let digest = manifest
            .get("digest")
            .ok_or_else(|| invalid_oci("OCI referrer descriptor has no digest"))?
            .as_str()
            .map_err(|_| invalid_oci("OCI referrer digest is not a string"))?
            .to_string();
        validate_oci_digest(&digest)?;
        let size = match manifest.get("size") {
            Some(JSONValue::Number(value)) if *value >= 0 => *value as u64,
            _ => {
                return Err(invalid_oci(
                    "OCI referrer descriptor size is not a non-negative integer",
                ))
            }
        };
        descriptors.push((artifact_type, digest, size));
    }
    Ok(descriptors)
}

fn require_oci_keys(
    object: &std::collections::BTreeMap<String, JSONValue>,
    keys: &[&str],
    label: &str,
) -> io::Result<()> {
    if object.len() != keys.len() || object.keys().any(|key| !keys.contains(&key.as_str())) {
        return Err(invalid_oci(&format!(
            "OCI {label} has unknown or missing fields"
        )));
    }
    Ok(())
}

fn invalid_oci(detail: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, detail)
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
    let package_dir = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "registry artifact has no parent",
        )
    })?;
    ensure_real_directory_if_present(repo, "registry checkout")?;
    ensure_real_directory_if_present(&repo.join("artifacts"), "registry artifact root")?;
    ensure_real_directory_if_present(package_dir, "registry artifact package directory")?;
    let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "registry has no source artifact for {} {}",
                    entry.name, entry.version
                ),
            )
        } else {
            error
        }
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "registry has no source artifact for {} {}",
                entry.name, entry.version
            ),
        ));
    }
    if !entry.content_hash.is_empty() && registry_artifact_hash(&path)? != entry.content_hash {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "registry source artifact for {} {} failed its content hash",
                entry.name, entry.version
            ),
        ));
    }
    verify_oci_referrers(repo, entry)?;
    Ok(path)
}

// ──────────────────────────────────────────────
// Registry package dependency metadata
// ──────────────────────────────────────────────

const REGISTRY_PACKAGE_METADATA_FILE: &str = "registry.json";
const MAX_REGISTRY_PACKAGE_METADATA_BYTES: u64 = 1024 * 1024;

/// Compute the registry content identity. Ordinary source identity remains
/// the foundation `tree_hash`; registry metadata is the one additional
/// package input because it changes dependency meaning and must be immutable
/// with the published artifact. The wire shape reuses the provider registry
/// JSON vocabulary and does not add a second Jet manifest syntax.
pub fn registry_artifact_hash(root: &Path) -> io::Result<String> {
    let mut entries = Vec::new();
    collect_registry_identity_files(root, root, &mut entries)?;
    entries.sort_by(|left, right| left.0.cmp(&right.0));

    let mut input = Vec::new();
    for (relative, content) in entries {
        input.extend_from_slice(relative.as_bytes());
        input.push(0);
        input.extend_from_slice(&(content.len() as u64).to_be_bytes());
        input.extend_from_slice(&content);
    }
    Ok(format!("sha256-{}", SHA256::sha256_hex(&input)))
}

fn collect_registry_identity_files(
    dir: &Path,
    root: &Path,
    out: &mut Vec<(String, Vec<u8>)>,
) -> io::Result<()> {
    let mut entries = std::fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_str().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "registry source contains a non-UTF-8 name `{}`",
                    path.display()
                ),
            )
        })?;
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() && !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "registry source contains an unsupported node `{}`",
                    path.display()
                ),
            ));
        }
        if name.starts_with('.') || name == "build" || name == "target" {
            continue;
        }
        if metadata.is_dir() {
            collect_registry_identity_files(&path, root, out)?;
            continue;
        }
        if metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "registry source contains an unsupported node `{}`",
                    path.display()
                ),
            ));
        }
        if !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "registry source contains an unsupported node `{}`",
                    path.display()
                ),
            ));
        }
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_str()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "registry source contains a non-UTF-8 path `{}`",
                        path.display()
                    ),
                )
            })?
            .replace('\\', "/");
        let is_registry_metadata = name == REGISTRY_PACKAGE_METADATA_FILE;
        // The artifact copy path preserves every visible regular file. Hash
        // that same set, so an auxiliary payload (for example an embedded
        // asset) cannot be changed after publication without changing the
        // signed content identity.
        let content = if is_registry_metadata {
            read_registry_file_nofollow(&path, MAX_REGISTRY_PACKAGE_METADATA_BYTES)?
        } else {
            read_registry_file_nofollow(&path, u64::MAX)?
        };
        out.push((relative, content));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegistryDependency {
    pub(crate) name: String,
    pub(crate) requirements: Vec<String>,
    pub(crate) roles: BTreeSet<String>,
    pub(crate) prefer: Vec<String>,
    pub(crate) reject: BTreeSet<String>,
    pub(crate) strict: bool,
    pub(crate) enabled_by_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegistryPackageMetadata {
    pub(crate) name: String,
    pub(crate) version: String,
    dependencies: Vec<RegistryDependency>,
    active_features: BTreeSet<String>,
    canonical: String,
}

impl RegistryPackageMetadata {
    pub(crate) fn active_dependencies(&self) -> Vec<RegistryDependency> {
        self.dependencies
            .iter()
            .filter(|dependency| {
                let dev_or_test_only = dependency
                    .roles
                    .iter()
                    .all(|role| matches!(role.as_str(), "dev" | "test"));
                if dev_or_test_only {
                    return false;
                }
                let has_non_optional_role = dependency
                    .roles
                    .iter()
                    .any(|role| role != "optional" && role != "dev" && role != "test");
                !dependency.roles.contains("optional")
                    || dependency.enabled_by_default
                    || self.active_features.contains(&dependency.name)
                    || has_non_optional_role
            })
            .cloned()
            .collect()
    }

    pub(crate) fn contains_dependency(&self, name: &str) -> bool {
        self.dependencies
            .iter()
            .any(|dependency| dependency.name == name)
    }

    pub(crate) fn canonical(&self) -> &str {
        &self.canonical
    }
}

/// Read and validate the artifact-bound registry metadata. Missing metadata
/// is intentional: ordinary `package.jet` registry dependencies retain the
/// normal role and the resolver supplies that compatibility default.
pub(crate) fn read_registry_package_metadata(
    artifact: &Path,
    expected_name: &str,
    expected_version: &str,
) -> io::Result<Option<RegistryPackageMetadata>> {
    let path = artifact.join(REGISTRY_PACKAGE_METADATA_FILE);
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(invalid_registry_metadata(
            "registry.json is not a regular file",
        ));
    }
    if metadata.len() > MAX_REGISTRY_PACKAGE_METADATA_BYTES {
        return Err(invalid_registry_metadata(
            "registry.json exceeds its size limit",
        ));
    }
    let text = std::fs::read_to_string(&path)?;
    parse_registry_package_metadata(&text, expected_name, expected_version).map(Some)
}

/// Publish-side validation runs before the immutable artifact staging path is
/// created. A malformed role/feature/constraint record cannot leave a partial
/// artifact or a publishable index line behind.
pub(crate) fn validate_registry_metadata_file(
    source: &Path,
    expected_name: &str,
    expected_version: &str,
) -> io::Result<()> {
    read_registry_package_metadata(source, expected_name, expected_version).map(|_| ())
}

fn parse_registry_package_metadata(
    text: &str,
    expected_name: &str,
    expected_version: &str,
) -> io::Result<RegistryPackageMetadata> {
    // Keep the lock's one-line future field stable even when a publisher
    // formats registry.json across lines. JSON strings cannot contain literal
    // newlines, so trimming and joining lines preserves the parsed meaning.
    let canonical = text.lines().map(str::trim).collect::<String>();
    let JSONValue::Object(object) = parse_json(&canonical)
        .map_err(|_| invalid_registry_metadata("registry.json is not valid JSON"))?
    else {
        return Err(invalid_registry_metadata(
            "registry.json must contain one JSON object",
        ));
    };
    let name = required_metadata_string(&object, "name")?;
    let version = required_metadata_string(&object, "version")?;
    if name != expected_name || version != expected_version {
        return Err(invalid_registry_metadata(
            "registry.json identity disagrees with its artifact path",
        ));
    }

    let mut dependencies = BTreeMap::<String, RegistryDependency>::new();
    for (field, role) in [
        ("dependencies", "normal"),
        ("build_dependencies", "build"),
        ("tool_dependencies", "tool"),
        ("dev_dependencies", "dev"),
        ("test_dependencies", "test"),
        ("optional_dependencies", "optional"),
        ("peer_dependencies", "peer"),
        ("plugin_dependencies", "plugin"),
        ("target_dependencies", "target"),
    ] {
        let Some(value) = object.get(field) else {
            continue;
        };
        let JSONValue::Object(values) = value else {
            return Err(invalid_registry_metadata(&format!(
                "registry `{field}` dependencies must be an object"
            )));
        };
        for (dependency_name, descriptor) in values {
            merge_registry_dependency(&mut dependencies, dependency_name, descriptor, role)?;
        }
    }

    let mut feature_map = BTreeMap::<String, Vec<String>>::new();
    if let Some(value) = object.get("features") {
        let JSONValue::Object(features) = value else {
            return Err(invalid_registry_metadata(
                "registry `features` must be an object of string arrays",
            ));
        };
        for (feature, values) in features {
            let JSONValue::Array(values) = values else {
                return Err(invalid_registry_metadata(
                    "registry feature values must be arrays",
                ));
            };
            let mut names = Vec::new();
            for value in values {
                let JSONValue::String(name) = value else {
                    return Err(invalid_registry_metadata(
                        "registry feature members must be strings",
                    ));
                };
                validate_registry_dependency_name(name)?;
                names.push(name.clone());
            }
            names.sort();
            names.dedup();
            feature_map.insert(feature.clone(), names);
        }
    }

    if let Some(value) = object.get("constraints") {
        let JSONValue::Object(constraints) = value else {
            return Err(invalid_registry_metadata(
                "registry `constraints` must be an object",
            ));
        };
        for (dependency_name, descriptor) in constraints {
            merge_registry_dependency(&mut dependencies, dependency_name, descriptor, "normal")?;
        }
    }

    let dependency_names = dependencies.keys().cloned().collect::<BTreeSet<_>>();
    for values in feature_map.values() {
        for value in values {
            if !feature_map.contains_key(value) && !dependency_names.contains(value) {
                return Err(invalid_registry_metadata(
                    "registry feature names an undeclared dependency",
                ));
            }
        }
    }
    let mut active_features = BTreeSet::new();
    if feature_map.contains_key("default") {
        activate_registry_feature(
            "default",
            &feature_map,
            &mut active_features,
            &mut BTreeSet::new(),
        )?;
    }
    for dependency in dependencies.values_mut() {
        if dependency.roles.contains("optional") && active_features.contains(&dependency.name) {
            dependency.enabled_by_default = true;
        }
    }

    Ok(RegistryPackageMetadata {
        name,
        version,
        dependencies: dependencies.into_values().collect(),
        active_features,
        canonical,
    })
}

fn merge_registry_dependency(
    dependencies: &mut BTreeMap<String, RegistryDependency>,
    name: &str,
    descriptor: &JSONValue,
    role: &str,
) -> io::Result<()> {
    validate_registry_dependency_name(name)?;
    let (requirements, prefer, reject, strict, enabled_by_default) =
        parse_registry_dependency_descriptor(descriptor)?;
    let dependency = dependencies
        .entry(name.to_string())
        .or_insert_with(|| RegistryDependency {
            name: name.to_string(),
            requirements: Vec::new(),
            roles: BTreeSet::new(),
            prefer: Vec::new(),
            reject: BTreeSet::new(),
            strict: false,
            enabled_by_default: false,
        });
    dependency.roles.insert(role.to_string());
    for requirement in requirements {
        if !dependency.requirements.contains(&requirement) {
            dependency.requirements.push(requirement);
        }
    }
    for requirement in prefer {
        if !dependency.prefer.contains(&requirement) {
            dependency.prefer.push(requirement);
        }
    }
    dependency.reject.extend(reject);
    dependency.strict |= strict;
    dependency.enabled_by_default |= enabled_by_default;
    Ok(())
}

fn parse_registry_dependency_descriptor(
    descriptor: &JSONValue,
) -> io::Result<(Vec<String>, Vec<String>, BTreeSet<String>, bool, bool)> {
    let mut requirements = Vec::new();
    let mut prefer = Vec::new();
    let mut reject = BTreeSet::new();
    let mut strict = false;
    let mut enabled_by_default = false;
    match descriptor {
        JSONValue::String(requirement) => requirements.push(requirement.clone()),
        JSONValue::Object(fields) => {
            if let Some(value) = fields.get("require").or_else(|| fields.get("version")) {
                let JSONValue::String(requirement) = value else {
                    return Err(invalid_registry_metadata(
                        "registry dependency `require` must be a string",
                    ));
                };
                requirements.push(requirement.clone());
            } else {
                requirements.push("*".to_string());
            }
            if let Some(value) = fields.get("prefer") {
                let JSONValue::String(requirement) = value else {
                    return Err(invalid_registry_metadata(
                        "registry dependency `prefer` must be a string",
                    ));
                };
                prefer.push(requirement.clone());
            }
            if let Some(value) = fields.get("reject") {
                let JSONValue::Array(values) = value else {
                    return Err(invalid_registry_metadata(
                        "registry dependency `reject` must be an array",
                    ));
                };
                for value in values {
                    let JSONValue::String(version) = value else {
                        return Err(invalid_registry_metadata(
                            "registry dependency reject values must be strings",
                        ));
                    };
                    if super::SemVer::SemVer::parse(version).is_none() {
                        return Err(invalid_registry_metadata(
                            "registry dependency reject values must be SemVer",
                        ));
                    }
                    reject.insert(version.clone());
                }
            }
            if let Some(value) = fields.get("strict") {
                let JSONValue::Bool(value) = value else {
                    return Err(invalid_registry_metadata(
                        "registry dependency `strict` must be boolean",
                    ));
                };
                strict = *value;
            }
            if let Some(value) = fields.get("default") {
                let JSONValue::Bool(value) = value else {
                    return Err(invalid_registry_metadata(
                        "registry dependency `default` must be boolean",
                    ));
                };
                enabled_by_default = *value;
            }
            if let Some(value) = fields.get("features") {
                let JSONValue::Array(values) = value else {
                    return Err(invalid_registry_metadata(
                        "registry dependency `features` must be an array",
                    ));
                };
                if values
                    .iter()
                    .any(|value| !matches!(value, JSONValue::String(_)))
                {
                    return Err(invalid_registry_metadata(
                        "registry dependency feature values must be strings",
                    ));
                }
            }
        }
        _ => {
            return Err(invalid_registry_metadata(
                "registry dependency values must be strings or objects",
            ));
        }
    }
    for requirement in requirements.iter().chain(prefer.iter()) {
        if super::SemVer::VersionReq::parse(requirement).is_none() {
            return Err(invalid_registry_metadata(
                "registry dependency constraints must be valid SemVer requirements",
            ));
        }
    }
    Ok((requirements, prefer, reject, strict, enabled_by_default))
}

fn activate_registry_feature(
    feature: &str,
    features: &BTreeMap<String, Vec<String>>,
    active: &mut BTreeSet<String>,
    visiting: &mut BTreeSet<String>,
) -> io::Result<()> {
    if !visiting.insert(feature.to_string()) {
        return Err(invalid_registry_metadata(
            "registry feature closure contains a cycle",
        ));
    }
    if let Some(values) = features.get(feature) {
        for value in values {
            if features.contains_key(value) {
                activate_registry_feature(value, features, active, visiting)?;
            } else {
                active.insert(value.clone());
            }
        }
    }
    visiting.remove(feature);
    Ok(())
}

fn required_metadata_string(object: &BTreeMap<String, JSONValue>, key: &str) -> io::Result<String> {
    match object.get(key) {
        Some(JSONValue::String(value)) if !value.trim().is_empty() => Ok(value.clone()),
        _ => Err(invalid_registry_metadata(&format!(
            "registry.json requires a non-empty `{key}`"
        ))),
    }
}

fn validate_registry_dependency_name(name: &str) -> io::Result<()> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(invalid_registry_metadata(
            "registry dependency name is not a safe package name",
        ));
    }
    Ok(())
}

fn invalid_registry_metadata(detail: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, detail)
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
    let source_metadata = std::fs::symlink_metadata(source)?;
    if source_metadata.file_type().is_symlink()
        || is_registry_reparse_point(&source_metadata)
        || !source_metadata.is_dir()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "registry source root is not a real directory `{}`",
                source.display()
            ),
        ));
    }
    ensure_registry_tree_directory(destination)?;
    let mut entries = std::fs::read_dir(source)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    let mut names = entries
        .iter()
        .map(|entry| entry.file_name())
        .collect::<Vec<_>>();
    names.sort_unstable();
    for entry in entries {
        let name = entry.file_name();
        let name = name.to_str().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "registry source contains a non-UTF-8 name `{}`",
                    entry.path().display()
                ),
            )
        })?;
        let from = entry.path();
        let metadata = std::fs::symlink_metadata(&from)?;
        if metadata.file_type().is_symlink()
            || is_registry_reparse_point(&metadata)
            || !metadata.is_dir() && !metadata.is_file()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "registry source contains an unsupported node `{}`",
                    from.display()
                ),
            ));
        }
        // These are local state, never package source. tree_hash applies the
        // same exclusion, so omitting them preserves the published identity.
        if name.starts_with('.') || name == "build" || name == "target" {
            continue;
        }
        let to = destination.join(entry.file_name());
        if metadata.is_dir() {
            ensure_registry_tree_directory(&to)?;
            copy_artifact_tree(&from, &to)?;
            let after = std::fs::symlink_metadata(&from)?;
            if !same_registry_identity(&metadata, &after) {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!(
                        "registry source directory changed while copying `{}`",
                        from.display()
                    ),
                ));
            }
        } else {
            copy_registry_file_nofollow(&from, &to, &metadata)?;
        }
    }
    let mut after_names = std::fs::read_dir(source)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<io::Result<Vec<_>>>()?;
    after_names.sort_unstable();
    let after_metadata = std::fs::symlink_metadata(source)?;
    if !same_registry_identity(&source_metadata, &after_metadata) || names != after_names {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "registry source changed while copying `{}`",
                source.display()
            ),
        ));
    }
    Ok(())
}

fn copy_publish_lock(source: &Path, destination: &Path) -> io::Result<()> {
    let source_metadata = source.join(".jet");
    let metadata = match std::fs::symlink_metadata(&source_metadata) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink()
        || is_registry_reparse_point(&metadata)
        || !metadata.is_dir()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "package .jet metadata directory must be a real directory",
        ));
    }
    let source_lock = source_metadata.join("lock");
    let lock_metadata = match std::fs::symlink_metadata(&source_lock) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if lock_metadata.file_type().is_symlink()
        || is_registry_reparse_point(&lock_metadata)
        || !lock_metadata.is_file()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "package lock must be a regular file",
        ));
    }
    let destination_metadata = destination.join(".jet");
    ensure_registry_tree_directory(&destination_metadata)?;
    copy_registry_file_nofollow(
        &source_lock,
        &destination_metadata.join("lock"),
        &lock_metadata,
    )
}

fn ensure_registry_tree_directory(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.file_type().is_symlink() || is_registry_reparse_point(&metadata) =>
        {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "registry tree directory must not be a symlink or reparse point",
            ))
        }
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "registry tree path is not a directory",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                ensure_registry_tree_directory(parent)?;
            }
            std::fs::create_dir(path)
        }
        Err(error) => Err(error),
    }
}

fn copy_registry_file_nofollow(
    source: &Path,
    destination: &Path,
    expected: &std::fs::Metadata,
) -> io::Result<()> {
    let mut source_options = std::fs::OpenOptions::new();
    source_options.read(true);
    add_registry_nofollow_flags(&mut source_options);
    let mut source_file = source_options.open(source)?;
    let opened = source_file.metadata()?;
    if !opened.is_file()
        || is_registry_reparse_point(&opened)
        || !same_registry_identity(expected, &opened)
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "registry source file changed before copy `{}`",
                source.display()
            ),
        ));
    }
    let mut destination_options = std::fs::OpenOptions::new();
    destination_options.write(true).create(true).truncate(true);
    add_registry_nofollow_flags(&mut destination_options);
    let mut destination_file = destination_options.open(destination)?;
    std::io::copy(&mut source_file, &mut destination_file)?;
    destination_file.sync_all()?;
    let after = std::fs::symlink_metadata(source)?;
    if !same_registry_identity(expected, &after) {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "registry source file changed while copying `{}`",
                source.display()
            ),
        ));
    }
    Ok(())
}

fn read_registry_file_nofollow(path: &Path, maximum: u64) -> io::Result<Vec<u8>> {
    let expected = std::fs::symlink_metadata(path)?;
    if expected.file_type().is_symlink()
        || is_registry_reparse_point(&expected)
        || !expected.is_file()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "registry identity entry is not a regular file `{}`",
                path.display()
            ),
        ));
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    add_registry_nofollow_flags(&mut options);
    let mut file = options.open(path)?;
    let opened = file.metadata()?;
    if !same_registry_identity(&expected, &opened) {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "registry identity entry changed before read `{}`",
                path.display()
            ),
        ));
    }
    let mut content = Vec::new();
    file.by_ref()
        .take(maximum.saturating_add(1))
        .read_to_end(&mut content)?;
    if content.len() as u64 > maximum {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "registry identity file exceeds its size limit",
        ));
    }
    let after = std::fs::symlink_metadata(path)?;
    if !same_registry_identity(&expected, &after) {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "registry identity entry changed while reading `{}`",
                path.display()
            ),
        ));
    }
    Ok(content)
}

fn add_registry_nofollow_flags(options: &mut std::fs::OpenOptions) -> bool {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        const O_CLOEXEC: i32 = 0o2000000;
        const O_NOFOLLOW: i32 = 0o400000;
        options.custom_flags(O_NOFOLLOW | O_CLOEXEC);
        return true;
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        const O_CLOEXEC: i32 = 0x01000000;
        const O_NOFOLLOW: i32 = 0x0100;
        options.custom_flags(O_NOFOLLOW | O_CLOEXEC);
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        return true;
    }
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        windows
    )))]
    {
        let _ = options;
        false
    }
}

fn sync_registry_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        return std::fs::File::open(path)?.sync_all();
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

fn is_registry_reparse_point(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        return metadata.file_attributes() & 0x0000_0400 != 0;
    }
    #[cfg(not(windows))]
    {
        let _ = metadata;
        false
    }
}

fn same_registry_identity(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        return left.dev() == right.dev() && left.ino() == right.ino();
    }
    #[cfg(not(unix))]
    {
        left.file_type() == right.file_type() && left.len() == right.len()
    }
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
///   - An entry that declares a key differing from the pin is a takeover. Its
///     signature must verify against the newly declared key; the registry
///     review receipt is enforced by `Index::validate_takeover`.
///
/// Returns any retained warnings. `all` is every recorded line for the package
/// (yanked included), so the pin can be found regardless of yanks.
pub fn verify_index_entry(
    all: &[IndexEntry],
    entry: &IndexEntry,
    require_signed: bool,
    registry_name: &str,
) -> Result<Vec<String>, Diagnostic> {
    let warnings = Vec::new();

    let takeover = Index::is_takeover(all, entry);
    if entry.signature.trim().is_empty() {
        if require_signed || takeover {
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
    let verification_key = if takeover {
        entry.public_key.trim()
    } else {
        pin.as_str()
    };
    if !crate::Publish::Sign::verify(verification_key, &entry.content_hash, &entry.signature)? {
        return Err(e1246(&entry.name, &entry.version));
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
pub fn e1235(url: &str, _detail: &str) -> Diagnostic {
    let safe_url = redact_registry_url(url);
    Diagnostic::error(
        "E1235",
        format!("couldn't reach the registry index at `{safe_url}`"),
        "the git operation against the registry failed (network, auth, or a stale local clone)"
            .to_string(),
        format!(
            "check network access and credentials for `{safe_url}`, or set `JET_REGISTRY_URL` to a \
             reachable mirror."
        ),
        None,
    )
}
