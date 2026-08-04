//! Optional administrator-installed shared-store broker.
//!
//! The broker accepts one provenance-bound unsigned Hangar archive per
//! activated process. It verifies and signs the archive after admission. It
//! never receives source, a build command, or an evaluator input.
//! A missing broker is transparent: callers keep using their per-user Hangar.

use super::{Archive, CacheExpectation, Roots, StoreEntry};
use crate::SHA256;
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const CONFIG_MAGIC: &str = "jet-shared-store-v1";
const REQUEST_MAGIC: &str = "jet-shared-store-request-v1";
const RESPONSE_MAGIC: &str = "jet-shared-store-response-v1";
const MAX_CONFIG_BYTES: usize = 16 * 1024;
const MAX_REQUEST_BYTES: usize = 1024 * 1024 * 1024;
const MAX_REFERENCE_BYTES: usize = 4096;
const MAX_PROVENANCE_FIELD_BYTES: usize = 4096;
const GRANT_MAGIC: &str = "jet-shared-store-grant-v2";
const GRANT_TTL_SECS: u64 = 15 * 60;
const INCOMING_STALE_AFTER_SECS: u64 = 15 * 60;
const MAX_INCOMING_ENTRIES: usize = 1024;
const STAGE_MARKER: &str = ".jet-shared-store-stage";
const BROKER_SANDBOX_POLICY: &str =
    "jet-shared-store-sandbox-v1\nno-source-evaluation\nsocket-activated\n";
const SHARED_DIR: &str = "shared-store";
const ADMIN_CONFIG: &str = "/etc/jet/shared-store/config";
const ADMIN_BASE: &str = "/var/lib/jet/shared-store";
const ADMIN_SOCKET: &str = "/run/jet/shared-store.sock";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedStoreConfig {
    pub socket: PathBuf,
    pub shared_root: PathBuf,
    pub trust_key: PathBuf,
    pub grants: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedStoreInstallReport {
    pub config: PathBuf,
    pub socket_unit: Option<PathBuf>,
    pub service_unit: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BrokerLayout {
    admin: bool,
    config: PathBuf,
    base: PathBuf,
    socket: PathBuf,
    shared_root: PathBuf,
    trust_key: PathBuf,
    grants: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProvenanceBinding {
    reference: String,
    source: String,
    builder: String,
    action: String,
    output: String,
    platform: String,
    sandbox: String,
    policy: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WriterGrant {
    read: bool,
    write: bool,
    expires: Option<u64>,
    credential: Option<String>,
    sources: BTreeSet<String>,
    builders: BTreeSet<String>,
}

fn user_broker_layout(roots: &Roots) -> BrokerLayout {
    let base = absolute_path(&roots.root).join(SHARED_DIR);
    BrokerLayout {
        admin: false,
        config: base.join("config"),
        socket: base.join("broker.sock"),
        shared_root: base.join("root"),
        trust_key: base.join("trust/hangar.key"),
        grants: base.join("users"),
        base,
    }
}

fn admin_broker_layout() -> BrokerLayout {
    let base = PathBuf::from(ADMIN_BASE);
    BrokerLayout {
        admin: true,
        config: PathBuf::from(ADMIN_CONFIG),
        socket: PathBuf::from(ADMIN_SOCKET),
        shared_root: base.join("root"),
        trust_key: base.join("trust/hangar.key"),
        grants: base.join("users"),
        base,
    }
}

/// Normal callers resolve their private root unless an administrator
/// descriptor is present. The install command and socket-activated service
/// resolve the administrator layout explicitly; they must not inherit a
/// caller-controlled `JETPACK_ROOT`.
fn broker_layout(roots: &Roots) -> BrokerLayout {
    user_broker_layout(roots)
}

#[cfg(unix)]
fn is_effective_root() -> bool {
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status.lines().find_map(|line| {
                let values = line.strip_prefix("Uid:")?.split_whitespace();
                values.skip(1).next()?.parse::<u32>().ok()
            })
        })
        == Some(0)
}

#[cfg(not(unix))]
fn is_effective_root() -> bool {
    false
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

pub fn shared_store_config(roots: &Roots) -> io::Result<Option<SharedStoreConfig>> {
    let layout = broker_layout(roots);
    if !layout.admin {
        // A user may consume an administrator-installed broker, but the
        // absence of its descriptor must fall back to the caller's private
        // broker. A malformed administrator descriptor is not ignored.
        if let Some(config) = read_shared_store_config(&admin_broker_layout())? {
            return Ok(Some(config));
        }
    }
    read_shared_store_config(&layout)
}

fn read_shared_store_config(layout: &BrokerLayout) -> io::Result<Option<SharedStoreConfig>> {
    let path = layout.config.clone();
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(invalid("shared-store config is not a regular file"));
    }
    if metadata.len() as usize > MAX_CONFIG_BYTES {
        return Err(invalid("shared-store config is too large"));
    }
    if layout.admin {
        require_admin_descriptor(&path)?;
    } else {
        require_private_mode(&path, "shared-store config")?;
    }
    let text = bounded_text_file(&path, MAX_CONFIG_BYTES, "shared-store config")?;
    let mut fields = std::collections::BTreeMap::new();
    let mut lines = text.lines();
    if lines.next() != Some(CONFIG_MAGIC) {
        return Err(invalid("shared-store config has an unknown format"));
    }
    for line in lines {
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| invalid("shared-store config has a malformed field"))?;
        if !matches!(key, "socket" | "shared_root" | "trust_key" | "grants")
            || fields.insert(key, value).is_some()
        {
            return Err(invalid("shared-store config has an unknown or duplicate field"));
        }
    }
    let decode = |key: &str| -> io::Result<PathBuf> {
        let value = fields
            .get(key)
            .ok_or_else(|| invalid(&format!("shared-store config is missing `{key}`")))?;
        let bytes = decode_hex(value)?;
        let text = String::from_utf8(bytes)
            .map_err(|_| invalid("shared-store config contains invalid UTF-8"))?;
        let path = PathBuf::from(text);
        if !path.is_absolute() {
            return Err(invalid("shared-store config paths must be absolute"));
        }
        Ok(path)
    };
    let config = SharedStoreConfig {
        socket: decode("socket")?,
        shared_root: decode("shared_root")?,
        trust_key: decode("trust_key")?,
        grants: decode("grants")?,
    };
    validate_config_paths(layout, &config)?;
    Ok(Some(config))
}

/// Create the administrator-selected broker configuration and
/// socket-activation units. The command does not create a resident daemon;
/// systemd starts the one-request service only on demand. Ordinary callers
/// continue to use their per-user Hangar when this configuration is absent.
pub fn install_shared_store(roots: &Roots) -> io::Result<SharedStoreInstallReport> {
    if !is_effective_root() {
        return Err(invalid(
            "shared-store install requires administrator authority; run `sudo jetpack shared-store install`",
        ));
    }
    let layout = admin_broker_layout();
    ensure_system_dir(layout.base.parent().unwrap_or(Path::new("/")))?;
    ensure_admin_public_dir(&layout.base)?;
    ensure_system_dir(layout.config.parent().unwrap_or(Path::new("/")))?;
    ensure_system_dir(layout.socket.parent().unwrap_or(Path::new("/")))?;
    let trust_dir = layout
        .trust_key
        .parent()
        .ok_or_else(|| invalid("shared-store trust key has no parent"))?;
    ensure_private_dir(trust_dir)?;
    ensure_admin_public_dir(&layout.grants)?;
    ensure_secret(&layout.trust_key, b"shared-store trust key", roots)?;

    let config = SharedStoreConfig {
        socket: layout.socket.clone(),
        shared_root: layout.shared_root.clone(),
        trust_key: layout.trust_key.clone(),
        grants: layout.grants.clone(),
    };
    let config_path = layout.config.clone();
    let mut text = String::from(CONFIG_MAGIC);
    text.push('\n');
    for (key, value) in [
        ("socket", &config.socket),
        ("shared_root", &config.shared_root),
        ("trust_key", &config.trust_key),
        ("grants", &config.grants),
    ] {
        if value
            .to_string_lossy()
            .bytes()
            .any(|byte| matches!(byte, b'\n' | b'\r' | b'='))
        {
            return Err(invalid("shared-store path contains an unsafe character"));
        }
        text.push_str(key);
        text.push('=');
        text.push_str(&encode_hex(value.to_string_lossy().as_bytes()));
        text.push('\n');
    }
    atomic_write(&config_path, text.as_bytes())?;
    if layout.admin {
        set_mode(&config_path, 0o644)?;
    }

    #[cfg(unix)]
    {
        let executable = std::env::current_exe()?;
        let unit_dir = if layout.admin {
            PathBuf::from("/etc/systemd/system")
        } else {
            user_systemd_unit_dir()?
        };
        ensure_real_dir(&unit_dir)?;
        let socket_unit = unit_dir.join("jet-shared-store.socket");
        let service_unit = unit_dir.join("jet-shared-store.service");
        let socket_text = if layout.admin {
            format!(
                "[Unit]\nDescription=Jet shared-store broker socket\n\n[Socket]\nListenStream={}\nSocketMode=0666\nDirectoryMode=0755\nRemoveOnStop=yes\n\n[Install]\nWantedBy=sockets.target\n",
                systemd_escape_path(&config.socket)
            )
        } else {
            format!(
                "[Unit]\nDescription=Jet shared-store broker socket\n\n[Socket]\nListenStream={}\nSocketMode=0600\nDirectoryMode=0700\nRemoveOnStop=yes\n\n[Install]\nWantedBy=sockets.target\n",
                systemd_escape_path(&config.socket)
            )
        };
        let service_text = admin_service_unit_text(&executable, &config);
        atomic_write(&socket_unit, socket_text.as_bytes())?;
        atomic_write(&service_unit, service_text.as_bytes())?;
        return Ok(SharedStoreInstallReport {
            config: config_path,
            socket_unit: Some(socket_unit),
            service_unit: Some(service_unit),
        });
    }
    #[cfg(not(unix))]
    {
        Ok(SharedStoreInstallReport {
            config: config_path,
            socket_unit: None,
            service_unit: None,
        })
    }
}

#[cfg(unix)]
fn systemd_escape_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace(' ', "\\x20")
}

#[cfg(unix)]
fn admin_service_unit_text(executable: &Path, config: &SharedStoreConfig) -> String {
    format!(
        "[Unit]\nDescription=Jet shared-store broker request\nRequires=jet-shared-store.socket\n\n[Service]\nType=oneshot\nExecStart={} shared-store broker --fd 3\nDynamicUser=yes\nStateDirectory=jet/shared-store/root\nStateDirectoryMode=0700\nLoadCredential=hangar.key:{}\nEnvironment=JET_SHARED_STORE_TRUST_KEY=%d/hangar.key\nNoNewPrivileges=yes\nPrivateTmp=yes\nPrivateDevices=yes\nProtectSystem=strict\nProtectHome=read-only\nProtectKernelTunables=yes\nProtectKernelModules=yes\nProtectControlGroups=yes\nProtectClock=yes\nProtectProc=invisible\nProcSubset=pid\nLockPersonality=yes\nRestrictNamespaces=yes\nRestrictSUIDSGID=yes\nRestrictRealtime=yes\nMemoryDenyWriteExecute=yes\nCapabilityBoundingSet=\nAmbientCapabilities=\nRestrictAddressFamilies=AF_UNIX\nIPAddressDeny=any\nReadOnlyPaths={} {}\nUMask=0077\nTimeoutStartSec=120\nReadWritePaths={}\n",
        systemd_escape_path(executable),
        systemd_escape_path(&config.trust_key),
        systemd_escape_path(&config.grants),
        systemd_escape_path(&config.trust_key.parent().unwrap_or(Path::new("/"))),
        systemd_escape_path(&config.shared_root)
    )
}

/// Import a shared entry into the user's Hangar when the optional broker
/// installation has already published one. Returns `None` when no broker is
/// configured or the socket is not currently available.
pub fn reuse_shared_entry(
    roots: &Roots,
    reference: &str,
    expectation: &CacheExpectation,
) -> io::Result<Option<StoreEntry>> {
    let Some(config) = shared_store_config(roots)? else {
        return Ok(None);
    };
    if !config.socket.exists() {
        return Ok(None);
    }
    #[cfg(unix)]
    {
        use std::os::unix::net::UnixStream;
        let mut stream = match UnixStream::connect(&config.socket) {
            Ok(stream) => stream,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                ) => return Ok(None),
            Err(error) => return Err(error),
        };
        write_read_request(&mut stream, reference)?;
        let Some(bytes) = read_archive_response(&mut stream)? else {
            return Ok(None);
        };
        Archive::import_broker_archive(roots, &bytes)?;
        let Some(candidate) = super::find_by_reference(roots, reference) else {
            return Err(invalid("shared-store broker returned no matching entry"));
        };
        let proof = super::verify_cache_entry(roots, &candidate, reference, expectation);
        if !proof.trusted() {
            return Err(invalid("shared-store entry failed cache identity verification"));
        }
        Ok(Some(candidate))
    }
    #[cfg(not(unix))]
    {
        let _ = (config, reference, expectation);
        Err(invalid("shared-store sockets are unsupported on this host"))
    }
}

/// Promote a verified local entry when a broker is configured. Missing broker
/// state is the ordinary per-user path and is therefore not an error.
pub fn promote_shared_entry(roots: &Roots, entry: &StoreEntry) -> io::Result<bool> {
    let Some(config) = shared_store_config(roots)? else {
        return Ok(false);
    };
    if !config.socket.exists() {
        return Ok(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::net::UnixStream;
        let archive = Archive::export_unsigned_archive(roots, &entry.id, true)?;
        let binding = provenance_binding_for_entry(entry)?;
        let credential = writer_credential(&config)?;
        let mut stream = match UnixStream::connect(&config.socket) {
            Ok(stream) => stream,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                ) => return Ok(false),
            Err(error) => return Err(error),
        };
        write_request(&mut stream, &binding, &credential, &archive)?;
        read_response(&mut stream)
    }
    #[cfg(not(unix))]
    {
        let _ = (config, entry);
        Err(invalid("shared-store sockets are unsupported on this host"))
    }
}

/// Serve one systemd socket-activated request and exit. The raw descriptor
/// conversion is the single audited OS boundary; all protocol and archive
/// handling below remains safe Rust and bounded by MAX_REQUEST_BYTES.
#[cfg(unix)]
pub fn serve_shared_store_fd(_roots: &Roots, fd: i32) -> io::Result<()> {
    use std::os::fd::FromRawFd;
    use std::os::unix::net::UnixListener;
    if fd < 0 {
        return Err(invalid("shared-store broker fd is negative"));
    }
    // SAFETY: systemd owns fd 3 for the lifetime of this one-shot process and
    // passes a connected AF_UNIX listener according to the generated unit.
    let listener = unsafe { UnixListener::from_raw_fd(fd) };
    let (mut stream, _) = listener.accept()?;
    let config = read_shared_store_config(&admin_broker_layout())?
        .ok_or_else(|| invalid("shared-store broker is not installed"))?;
    let shared = Roots {
        root: config.shared_root.clone(),
        dev_mode: false,
    };
    let key = broker_trust_key(&config)?;
    let uid = peer_uid(&stream)?;
    match read_request_header(&mut stream)? {
        BrokerHeader::Write {
            length,
            binding,
            credential,
        } => {
            authorize_peer(&config, uid, true, Some(&binding), Some(&credential))?;
            let mut archive = vec![0u8; length];
            stream.read_exact(&mut archive)?;
            promote_staged_archive(&shared, &config, &key, &archive, &binding)?;
            write_response(&mut stream, "ok")
        }
        BrokerHeader::Read(reference) => {
            authorize_peer(&config, uid, false, None, None)?;
            let Some(entry) = super::find_by_reference(&shared, &reference) else {
                return write_response(&mut stream, "missing");
            };
            super::verify_hangar_object(&shared, &entry)
                .map_err(|error| io::Error::other(error.what()))?;
            let (archive, _) = Archive::export_archive(&shared, &entry.id, true, Some(&key))?;
            write_archive_response(&mut stream, &archive)
        }
    }
}

fn promote_staged_archive(
    shared: &Roots,
    config: &SharedStoreConfig,
    key: &str,
    archive: &[u8],
    binding: &ProvenanceBinding,
) -> io::Result<()> {
    let incoming = config.shared_root.join(".incoming");
    ensure_real_dir(&incoming)?;
    sweep_stale_incoming(&incoming)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    let stage = incoming.join(format!("{}-{}", std::process::id(), nonce));
    fs::create_dir(&stage)?;
    atomic_write(
        &stage.join(STAGE_MARKER),
        now_secs().to_string().as_bytes(),
    )?;
    let staged = Roots {
        root: stage.clone(),
        dev_mode: false,
    };
    let result = (|| {
        // Admission happens in an ephemeral root first. The archive is
        // bounded and digest-checked, then signed by the broker before the
        // shared root is reachable by the promotion step.
        let attested = Archive::attest_archive(&staged, archive, key)?;
        Archive::import_archive(&staged, &attested, Some(key), false)?;
        let entries = super::list_checked(&staged)?;
        if entries.is_empty() {
            return Err(invalid("shared-store archive contains no verified entry"));
        }
        for entry in &entries {
            super::verify_hangar_object(&staged, entry)
                .map_err(|error| io::Error::other(error.what()))?;
        }
        let root = entries
            .iter()
            .find(|entry| entry.reference == binding.reference)
            .ok_or_else(|| invalid("shared-store archive has no provenance-bound root"))?;
        let actual = provenance_binding_for_entry(root)?;
        if actual != *binding {
            return Err(invalid("shared-store provenance binding does not match archive"));
        }
        crate::RuntimePolicy::with_lock(&shared.root, "shared-store-promote", || {
            Archive::import_archive(shared, &attested, Some(key), false)?;
            Ok(())
        })
    })();
    let cleanup = remove_incoming_entry(&stage);
    match (result, cleanup) {
        (Err(error), Err(cleanup_error)) => Err(invalid(&format!(
            "{error}; shared-store staging cleanup failed: {cleanup_error}"
        ))),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(invalid(&format!(
            "shared-store staging cleanup failed: {error}"
        ))),
        (Ok(()), Ok(())) => Ok(()),
    }
}

#[cfg(not(unix))]
pub fn serve_shared_store_fd(_roots: &Roots, _fd: i32) -> io::Result<()> {
    Err(invalid("shared-store socket activation is unsupported on this host"))
}

#[cfg(test)]
#[derive(Debug)]
enum BrokerRequest {
    Write {
        archive: Vec<u8>,
        binding: ProvenanceBinding,
        credential: String,
    },
    Read(String),
}

fn write_request(
    stream: &mut impl Write,
    binding: &ProvenanceBinding,
    credential: &str,
    archive: &[u8],
) -> io::Result<()> {
    if archive.len() > MAX_REQUEST_BYTES {
        return Err(invalid("shared-store archive exceeds the request limit"));
    }
    validate_provenance_binding(binding)?;
    validate_text_field(credential, "shared-store writer credential")?;
    writeln!(stream, "{REQUEST_MAGIC}")?;
    writeln!(stream, "op=write")?;
    for (key, value) in [
        ("reference", &binding.reference),
        ("source", &binding.source),
        ("builder", &binding.builder),
        ("action", &binding.action),
        ("output", &binding.output),
        ("platform", &binding.platform),
        ("sandbox", &binding.sandbox),
        ("policy", &binding.policy),
    ] {
        writeln!(stream, "{key}={}", encode_hex(value.as_bytes()))?;
    }
    writeln!(stream, "credential={}", encode_hex(credential.as_bytes()))?;
    writeln!(stream, "bytes={}", archive.len())?;
    writeln!(stream)?;
    stream.write_all(archive)?;
    stream.flush()
}

fn write_read_request(stream: &mut impl Write, reference: &str) -> io::Result<()> {
    if reference.is_empty()
        || reference.len() > MAX_REFERENCE_BYTES
        || reference.chars().any(char::is_control)
    {
        return Err(invalid("shared-store reference is invalid"));
    }
    writeln!(stream, "{REQUEST_MAGIC}")?;
    writeln!(stream, "op=read")?;
    writeln!(stream, "reference={}", encode_hex(reference.as_bytes()))?;
    writeln!(stream)?;
    stream.flush()
}

enum BrokerHeader {
    Write {
        length: usize,
        binding: ProvenanceBinding,
        credential: String,
    },
    Read(String),
}

fn read_request_header(stream: &mut impl Read) -> io::Result<BrokerHeader> {
    let mut header = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        stream.read_exact(&mut byte)?;
        header.push(byte[0]);
        if header.ends_with(b"\n\n") {
            break;
        }
        if header.len() > 4096 {
            return Err(invalid("shared-store request header is too large"));
        }
    }
    let text = std::str::from_utf8(&header)
        .map_err(|_| invalid("shared-store request header is not UTF-8"))?;
    let mut fields = std::collections::BTreeMap::new();
    let mut lines = text.lines();
    if lines.next() != Some(REQUEST_MAGIC) {
        return Err(invalid("shared-store request has an unknown format"));
    }
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| invalid("shared-store request has a malformed field"))?;
        if !matches!(
            key,
            "op"
                | "bytes"
                | "reference"
                | "source"
                | "builder"
                | "action"
                | "output"
                | "platform"
                | "sandbox"
                | "policy"
                | "credential"
        )
            || fields.insert(key, value).is_some()
        {
            return Err(invalid("shared-store request has an unknown or duplicate field"));
        }
    }
    match fields
        .get("op")
        .copied()
        .ok_or_else(|| invalid("shared-store request has no operation"))?
    {
        "write" => {
            let length = fields
                .get("bytes")
                .copied()
                .ok_or_else(|| invalid("shared-store request has no archive length"))?
                .parse::<usize>()
                .map_err(|_| invalid("shared-store archive length is invalid"))?;
            if length > MAX_REQUEST_BYTES {
                return Err(invalid("shared-store archive exceeds the request limit"));
            }
            let field = |key: &str| -> io::Result<String> {
                let value = fields
                    .get(key)
                    .copied()
                    .ok_or_else(|| invalid(&format!("shared-store write request has no `{key}`")))?;
                decode_text_field(value, key)
            };
            let binding = ProvenanceBinding {
                reference: field("reference")?,
                source: field("source")?,
                builder: field("builder")?,
                action: field("action")?,
                output: field("output")?,
                platform: field("platform")?,
                sandbox: field("sandbox")?,
                policy: field("policy")?,
            };
            validate_provenance_binding(&binding)?;
            let credential = field("credential")?;
            let _ = fields
                .get("bytes")
                .ok_or_else(|| invalid("shared-store request has no archive length"))?;
            Ok(BrokerHeader::Write {
                length,
                binding,
                credential,
            })
        }
        "read" => {
            if fields
                .keys()
                .any(|key| *key != "op" && *key != "reference")
            {
                return Err(invalid("shared-store read request cannot include archive bytes"));
            }
            let encoded = fields
                .get("reference")
                .copied()
                .ok_or_else(|| invalid("shared-store read request has no reference"))?;
            let reference = String::from_utf8(decode_hex(encoded)?)
                .map_err(|_| invalid("shared-store reference is not UTF-8"))?;
            if reference.is_empty()
                || reference.len() > MAX_REFERENCE_BYTES
                || reference.chars().any(char::is_control)
            {
                return Err(invalid("shared-store reference is invalid"));
            }
            Ok(BrokerHeader::Read(reference))
        }
        _ => Err(invalid("shared-store request has an unknown operation")),
    }
}

#[cfg(test)]
fn read_request(stream: &mut impl Read) -> io::Result<BrokerRequest> {
    match read_request_header(stream)? {
        BrokerHeader::Write {
            length,
            binding,
            credential,
        } => {
            let mut archive = vec![0u8; length];
            stream.read_exact(&mut archive)?;
            Ok(BrokerRequest::Write {
                archive,
                binding,
                credential,
            })
        }
        BrokerHeader::Read(reference) => Ok(BrokerRequest::Read(reference)),
    }
}

fn write_response(stream: &mut impl Write, status: &str) -> io::Result<()> {
    writeln!(stream, "{RESPONSE_MAGIC}")?;
    writeln!(stream, "status={status}")?;
    writeln!(stream)?;
    stream.flush()
}

#[cfg(target_os = "linux")]
fn peer_uid(stream: &std::os::unix::net::UnixStream) -> io::Result<u32> {
    use std::ffi::c_void;
    use std::os::fd::AsRawFd;

    #[repr(C)]
    struct PeerCred {
        pid: i32,
        uid: u32,
        gid: u32,
    }

    unsafe extern "C" {
        fn getsockopt(
            socket: i32,
            level: i32,
            name: i32,
            value: *mut c_void,
            length: *mut u32,
        ) -> i32;
    }

    const SOL_SOCKET: i32 = 1;
    const SO_PEERCRED: i32 = 17;
    let mut credential = PeerCred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<PeerCred>() as u32;
    // SAFETY: the socket is a connected AF_UNIX stream owned by this process;
    // the kernel writes at most `length` bytes into the correctly-sized C
    // credential structure.
    let result = unsafe {
        getsockopt(
            stream.as_raw_fd(),
            SOL_SOCKET,
            SO_PEERCRED,
            (&mut credential as *mut PeerCred).cast::<c_void>(),
            &mut length,
        )
    };
    if result != 0 || length < std::mem::size_of::<PeerCred>() as u32 {
        return Err(io::Error::last_os_error());
    }
    Ok(credential.uid)
}

#[cfg(all(unix, not(target_os = "linux")))]
fn peer_uid(_stream: &std::os::unix::net::UnixStream) -> io::Result<u32> {
    Err(invalid("shared-store peer credentials are unsupported on this Unix host"))
}

#[cfg(target_os = "linux")]
fn current_uid() -> io::Result<u32> {
    unsafe extern "C" {
        fn getuid() -> u32;
    }
    // SAFETY: `getuid` has no preconditions and returns the caller's uid.
    Ok(unsafe { getuid() })
}

#[cfg(not(target_os = "linux"))]
fn current_uid() -> io::Result<u32> {
    Err(invalid("shared-store enrollment is unsupported on this host"))
}

#[cfg(unix)]
fn owner_uid(path: &Path) -> io::Result<u32> {
    use std::os::unix::fs::MetadataExt;
    Ok(fs::symlink_metadata(path)?.uid())
}

fn is_admin_config(config: &SharedStoreConfig) -> bool {
    config.socket == Path::new(ADMIN_SOCKET)
}

fn require_safe_descriptor(path: &Path, label: &str) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(invalid(&format!("{label} is not a regular file")));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if metadata.permissions().mode() & 0o022 != 0 {
            return Err(invalid(&format!("{label} is writable by another user")));
        }
    }
    Ok(())
}

fn require_admin_descriptor(path: &Path) -> io::Result<()> {
    require_safe_descriptor(path, "shared-store administrator descriptor")?;
    #[cfg(unix)]
    if owner_uid(path)? != 0 {
        return Err(invalid("shared-store administrator descriptor is not root-owned"));
    }
    Ok(())
}

fn authorize_peer(
    config: &SharedStoreConfig,
    uid: u32,
    write: bool,
    binding: Option<&ProvenanceBinding>,
    credential: Option<&str>,
) -> io::Result<()> {
    let grant = read_writer_grant(config, uid)?;
    if !write {
        if !grant.read {
            return Err(invalid("shared-store peer has no read authority"));
        }
        return Ok(());
    }
    let binding = binding.ok_or_else(|| invalid("shared-store write has no provenance binding"))?;
    let credential = credential.ok_or_else(|| invalid("shared-store write has no credential"))?;
    authorize_grant(&grant, true, binding, credential)
}

fn read_writer_grant(config: &SharedStoreConfig, uid: u32) -> io::Result<WriterGrant> {
    // A rootless fixture can use the owner of its private shared root without
    // an enrollment file. This is a private namespace, so shared-namespace
    // source and builder allowlists do not apply.
    #[cfg(unix)]
    if !is_admin_config(config) && owner_uid(&config.shared_root)? == uid {
        return Ok(WriterGrant {
            read: true,
            write: true,
            expires: None,
            credential: Some("private-owner".to_string()),
            sources: BTreeSet::new(),
            builders: BTreeSet::new(),
        });
    }
    let grant = config.grants.join(uid.to_string());
    let metadata = fs::symlink_metadata(&grant)
        .map_err(|_| invalid("shared-store peer has no enrollment grant"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(invalid("shared-store peer enrollment is not a regular file"));
    }
    if is_admin_config(config) {
        require_admin_descriptor(&grant)?;
    } else {
        require_safe_descriptor(&grant, "shared-store peer enrollment")?;
    }
    parse_writer_grant(&bounded_text_file(
        &grant,
        MAX_CONFIG_BYTES,
        "shared-store peer enrollment",
    )?)
}

fn parse_writer_grant(text: &str) -> io::Result<WriterGrant> {
    let mut lines = text.lines();
    if lines.next() != Some(GRANT_MAGIC) {
        return Err(invalid("shared-store peer enrollment has an unknown format"));
    }
    let mut seen = BTreeSet::new();
    let mut grant = WriterGrant {
        read: false,
        write: false,
        expires: None,
        credential: None,
        sources: BTreeSet::new(),
        builders: BTreeSet::new(),
    };
    for line in lines {
        if line.is_empty() {
            continue;
        }
        match line {
            "read" => {
                if !seen.insert("read") {
                    return Err(invalid("shared-store peer enrollment repeats `read`"));
                }
                grant.read = true;
            }
            "write" => {
                if !seen.insert("write") {
                    return Err(invalid("shared-store peer enrollment repeats `write`"));
                }
                grant.write = true;
            }
            _ => {
                let (key, value) = line
                    .split_once('=')
                    .ok_or_else(|| invalid("shared-store peer enrollment has a malformed field"))?;
                match key {
                    "expires" => {
                        if !seen.insert("expires") {
                            return Err(invalid("shared-store peer enrollment repeats `expires`"));
                        }
                        grant.expires = Some(
                            value
                                .parse::<u64>()
                                .map_err(|_| invalid("shared-store peer enrollment expiry is invalid"))?,
                        );
                    }
                    "credential" => {
                        if !seen.insert("credential") {
                            return Err(invalid(
                                "shared-store peer enrollment repeats `credential`",
                            ));
                        }
                        validate_text_field(value, "shared-store writer credential")?;
                        grant.credential = Some(value.to_string());
                    }
                    "source" => {
                        validate_text_field(value, "shared-store writer source")?;
                        if !grant.sources.insert(value.to_string()) {
                            return Err(invalid("shared-store peer enrollment repeats `source`"));
                        }
                    }
                    "builder" => {
                        validate_text_field(value, "shared-store writer builder")?;
                        if !grant.builders.insert(value.to_string()) {
                            return Err(invalid("shared-store peer enrollment repeats `builder`"));
                        }
                    }
                    _ => return Err(invalid("shared-store peer enrollment has an unknown field")),
                }
            }
        }
    }
    if !grant.read {
        return Err(invalid("shared-store peer enrollment is missing read authority"));
    }
    if grant.write
        && (grant.expires.is_none()
            || grant.credential.is_none()
            || grant.sources.is_empty()
            || grant.builders.is_empty())
    {
        return Err(invalid(
            "shared-store write enrollment requires expiry, credential, source, and builder authority",
        ));
    }
    Ok(grant)
}

fn authorize_grant(
    grant: &WriterGrant,
    write: bool,
    binding: &ProvenanceBinding,
    credential: &str,
) -> io::Result<()> {
    if !grant.read {
        return Err(invalid("shared-store peer has no read authority"));
    }
    if !write {
        return Ok(());
    }
    if !grant.write {
        return Err(invalid("shared-store peer has no write authority"));
    }
    if grant.credential.as_deref() == Some("private-owner") {
        if credential != "private-owner" {
            return Err(invalid("shared-store writer credential is invalid"));
        }
        return Ok(());
    }
    let expires = grant
        .expires
        .ok_or_else(|| invalid("shared-store writer grant has no expiry"))?;
    if now_secs() >= expires {
        return Err(invalid("shared-store writer grant is expired"));
    }
    if grant.credential.as_deref() != Some(credential) {
        return Err(invalid("shared-store writer credential is invalid"));
    }
    if !grant.sources.contains(&binding.source) {
        return Err(invalid("shared-store writer source is not allowlisted"));
    }
    if !grant.builders.contains(&binding.builder) {
        return Err(invalid("shared-store writer builder is not allowlisted"));
    }
    Ok(())
}

/// Add or replace one administrator-approved uid grant. The caller must own
/// the local fixture root or be uid 0 for the system layout.
pub fn enroll_shared_store(roots: &Roots, uid: &str, writable: bool) -> io::Result<PathBuf> {
    let Some(config) = shared_store_config(roots)? else {
        return Err(invalid("shared-store broker is not installed"));
    };
    let uid = uid
        .parse::<u32>()
        .map_err(|_| invalid("shared-store enrollment uid is invalid"))?;
    let caller = current_uid()?;
    #[cfg(unix)]
    if caller != 0
        && (is_admin_config(&config) || owner_uid(&config.shared_root)? != caller)
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "only the administrator may enroll a system shared-store user",
        ));
    }
    ensure_real_dir(&config.grants)?;
    let path = config.grants.join(uid.to_string());
    let mut text = String::from(GRANT_MAGIC);
    text.push_str("\nread\n");
    if writable {
        text.push_str("write\n");
        let expires = now_secs().saturating_add(GRANT_TTL_SECS);
        let credential = encode_hex(&entropy(roots, b"shared-store-writer-credential"));
        text.push_str(&format!("expires={expires}\ncredential={credential}\n"));
        // Source and builder facts are deliberately administrator-selected.
        // Until they are appended, a write grant is fail-closed.
    }
    atomic_write(&path, text.as_bytes())?;
    set_mode(&path, 0o644)?;
    Ok(path)
}

fn write_archive_response(stream: &mut impl Write, archive: &[u8]) -> io::Result<()> {
    if archive.len() > MAX_REQUEST_BYTES {
        return Err(invalid("shared-store archive exceeds the response limit"));
    }
    writeln!(stream, "{RESPONSE_MAGIC}")?;
    writeln!(stream, "status=ok")?;
    writeln!(stream, "bytes={}", archive.len())?;
    writeln!(stream)?;
    stream.write_all(archive)?;
    stream.flush()
}

fn read_response(stream: &mut impl Read) -> io::Result<bool> {
    let mut bytes = Vec::new();
    stream.take(4097).read_to_end(&mut bytes)?;
    if bytes.len() > 4096 {
        return Err(invalid("shared-store response is too large"));
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| invalid("shared-store response is not UTF-8"))?;
    let mut lines = text.lines();
    if lines.next() != Some(RESPONSE_MAGIC) {
        return Err(invalid("shared-store response has an unknown format"));
    }
    let mut status = None;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| invalid("shared-store response has a malformed field"))?;
        if key != "status" || status.replace(value).is_some() {
            return Err(invalid("shared-store response has an unknown or duplicate field"));
        }
    }
    match status.ok_or_else(|| invalid("shared-store response has no status"))? {
        "ok" => Ok(true),
        "missing" => Ok(false),
        _ => Err(invalid("shared-store response has an unknown status")),
    }
}

fn read_archive_response(stream: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut header = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        stream.read_exact(&mut byte)?;
        header.push(byte[0]);
        if header.ends_with(b"\n\n") {
            break;
        }
        if header.len() > 4096 {
            return Err(invalid("shared-store response header is too large"));
        }
    }
    let text = std::str::from_utf8(&header)
        .map_err(|_| invalid("shared-store response header is not UTF-8"))?;
    let mut fields = std::collections::BTreeMap::new();
    let mut lines = text.lines();
    if lines.next() != Some(RESPONSE_MAGIC) {
        return Err(invalid("shared-store response has an unknown format"));
    }
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| invalid("shared-store response has a malformed field"))?;
        if !matches!(key, "status" | "bytes") || fields.insert(key, value).is_some() {
            return Err(invalid("shared-store response has an unknown or duplicate field"));
        }
    }
    match fields
        .get("status")
        .copied()
        .ok_or_else(|| invalid("shared-store response has no status"))?
    {
        "missing" => {
            if fields.contains_key("bytes") {
                return Err(invalid(
                    "shared-store missing response cannot include archive bytes",
                ));
            }
            Ok(None)
        }
        "ok" => {
            let length = fields
                .get("bytes")
                .copied()
                .ok_or_else(|| invalid("shared-store response has no archive length"))?
                .parse::<usize>()
                .map_err(|_| invalid("shared-store response archive length is invalid"))?;
            if length > MAX_REQUEST_BYTES {
                return Err(invalid("shared-store archive exceeds the response limit"));
            }
            let mut archive = vec![0u8; length];
            stream.read_exact(&mut archive)?;
            Ok(Some(archive))
        }
        _ => Err(invalid("shared-store response has an unknown status")),
    }
}

fn validate_config_paths(
    layout: &BrokerLayout,
    config: &SharedStoreConfig,
) -> io::Result<()> {
    let base = layout
        .base
        .canonicalize()
        .map_err(|error| invalid(&format!("could not resolve shared-store directory: {error}")))?;
    let expected = if layout.admin {
        vec![
            ("socket", PathBuf::from(ADMIN_SOCKET), config.socket.clone()),
            ("shared root", base.join("root"), config.shared_root.clone()),
            ("trust key", base.join("trust/hangar.key"), config.trust_key.clone()),
            ("grants", base.join("users"), config.grants.clone()),
        ]
    } else {
        vec![
            ("socket", base.join("broker.sock"), config.socket.clone()),
            ("shared root", base.join("root"), config.shared_root.clone()),
            ("trust key", base.join("trust/hangar.key"), config.trust_key.clone()),
            ("grants", base.join("users"), config.grants.clone()),
        ]
    };
    for (label, expected, actual) in expected {
        if actual != expected {
            return Err(invalid(&format!(
                "shared-store {label} is outside the installed private boundary"
            )));
        }
    }
    if layout.admin {
        require_admin_descriptor(&PathBuf::from(ADMIN_CONFIG))?;
    }
    if layout.admin {
        validate_admin_directory(&base, "shared-store directory")?;
        validate_managed_directory(&config.shared_root, "shared-store root")?;
    } else {
        validate_private_directory(&base, "shared-store directory")?;
        validate_private_directory(&config.shared_root, "shared-store root")?;
    }
    let trust_dir = base.join("trust");
    validate_private_directory(&trust_dir, "shared-store trust directory")?;
    if layout.admin {
        validate_admin_directory(&config.grants, "shared-store grants directory")?;
    } else {
        validate_private_directory(&config.grants, "shared-store grants directory")?;
    }
    for (path, label) in [(&config.trust_key, "shared-store trust key")] {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(invalid(&format!("{label} is not a regular file")));
        }
        require_private_mode(path, label)?;
    }
    if let Ok(metadata) = fs::symlink_metadata(&config.socket) {
        if metadata.file_type().is_symlink() {
            return Err(invalid("shared-store broker socket is a symlink"));
        }
    }
    Ok(())
}

fn validate_private_directory(path: &Path, label: &str) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(invalid(&format!("{label} is not a real directory")));
    }
    require_private_mode(path, label)
}

fn validate_managed_directory(path: &Path, label: &str) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(invalid(&format!("{label} is not a real directory")))
        }
        Ok(_) => require_private_mode(path, label),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn user_systemd_unit_dir() -> io::Result<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| invalid("shared-store install cannot locate the user config directory"))?;
    Ok(base.join("systemd").join("user"))
}

fn ensure_real_dir(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(invalid("shared-store path is not a real directory"))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(path)?;
            let metadata = fs::symlink_metadata(path)?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(invalid("shared-store path is not a real directory"));
            }
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn ensure_system_dir(path: &Path) -> io::Result<()> {
    ensure_real_dir(path)?;
    require_admin_dir(path)
}

fn ensure_admin_public_dir(path: &Path) -> io::Result<()> {
    let existed = fs::symlink_metadata(path).is_ok();
    ensure_real_dir(path)?;
    if existed {
        require_admin_dir(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            if fs::symlink_metadata(path)?.permissions().mode() & 0o005 != 0o005 {
                set_mode(path, 0o755)?;
            }
        }
    } else {
        set_mode(path, 0o755)?;
    }
    require_admin_public_dir(path)
}

fn require_admin_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt as _};
        let metadata = fs::symlink_metadata(path)?;
        if metadata.uid() != 0 {
            return Err(invalid("shared-store administrator directory is not root-owned"));
        }
        if metadata.permissions().mode() & 0o022 != 0 {
            return Err(invalid("shared-store administrator directory is writable by another user"));
        }
    }
    let _ = path;
    Ok(())
}

fn require_admin_public_dir(path: &Path) -> io::Result<()> {
    require_admin_dir(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if fs::symlink_metadata(path)?.permissions().mode() & 0o005 != 0o005 {
            return Err(invalid(
                "shared-store administrator directory is not traversable by enrolled users",
            ));
        }
    }
    Ok(())
}

fn validate_admin_directory(path: &Path, label: &str) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(invalid(&format!("{label} is not a real directory")));
    }
    require_admin_public_dir(path)
}

fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    let _ = (path, mode);
    Ok(())
}

fn ensure_private_dir(path: &Path) -> io::Result<()> {
    let existed = fs::symlink_metadata(path).is_ok();
    ensure_real_dir(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let metadata = fs::symlink_metadata(path)?;
        if metadata.permissions().mode() & 0o077 != 0 {
            if existed {
                return Err(invalid("shared-store directory has insecure permissions"));
            }
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        }
    }
    Ok(())
}

fn require_private_mode(path: &Path, label: &str) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = fs::symlink_metadata(path)?.permissions().mode();
        if mode & 0o077 != 0 {
            return Err(invalid(&format!("{label} has insecure permissions")));
        }
    }
    let _ = (path, label);
    Ok(())
}

fn set_private_mode(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    let _ = path;
    Ok(())
}

fn ensure_secret(path: &Path, label: &[u8], roots: &Roots) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(invalid("shared-store secret is not a regular file"))
        }
        Ok(_) => {
            require_private_mode(path, "shared-store secret")?;
            let secret = bounded_file(path, MAX_CONFIG_BYTES, "shared-store secret")?;
            if secret.len() < 32 {
                Err(invalid("shared-store secret is too short"))
            } else {
                Ok(())
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let mut secret = entropy(roots, label);
            let result = atomic_write(path, &secret);
            if result.is_ok() {
                set_private_mode(path)?;
            }
            for byte in &mut secret {
                *byte = 0;
            }
            result
        }
        Err(error) => Err(error),
    }
}

fn entropy(roots: &Roots, label: &[u8]) -> Vec<u8> {
    #[cfg(unix)]
    if let Ok(mut file) = fs::File::open("/dev/urandom") {
        let mut bytes = [0u8; 32];
        if file.read_exact(&mut bytes).is_ok() {
            return bytes.to_vec();
        }
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos().to_string())
        .unwrap_or_else(|_| "0".to_string());
    SHA256::sha256_hex(
        format!(
            "jet-shared-store-secret-v1\n{}\n{}\n{}\n{}",
            roots.root.display(),
            std::process::id(),
            now,
            String::from_utf8_lossy(label)
        )
        .as_bytes(),
    )
    .into_bytes()
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid("shared-store file has no parent"))?;
    ensure_real_dir(parent)?;
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(invalid("shared-store destination is not a regular file"));
        }
    }
    let partial = parent.join(format!(
        ".{}.partial-{}",
        path.file_name().and_then(|name| name.to_str()).unwrap_or("file"),
        std::process::id()
    ));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = options.open(&partial)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&partial, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode_hex(value: &str) -> io::Result<Vec<u8>> {
    if value.len() % 2 != 0 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid("shared-store field is not valid hexadecimal"));
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char)
                .to_digit(16)
                .ok_or_else(|| invalid("shared-store field is not valid hexadecimal"))?;
            let low = (pair[1] as char)
                .to_digit(16)
                .ok_or_else(|| invalid("shared-store field is not valid hexadecimal"))?;
            Ok(((high << 4) | low) as u8)
        })
        .collect()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn validate_text_field(value: &str, label: &str) -> io::Result<()> {
    if value.is_empty()
        || value.len() > MAX_PROVENANCE_FIELD_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(invalid(&format!("{label} is invalid")));
    }
    Ok(())
}

fn decode_text_field(value: &str, label: &str) -> io::Result<String> {
    let text = String::from_utf8(decode_hex(value)?)
        .map_err(|_| invalid(&format!("{label} is not UTF-8")))?;
    validate_text_field(&text, &format!("shared-store {label}"))?;
    Ok(text)
}

fn validate_provenance_binding(binding: &ProvenanceBinding) -> io::Result<()> {
    for (label, value) in [
        ("reference", &binding.reference),
        ("source", &binding.source),
        ("builder", &binding.builder),
        ("action", &binding.action),
        ("output", &binding.output),
        ("platform", &binding.platform),
        ("sandbox", &binding.sandbox),
        ("policy", &binding.policy),
    ] {
        validate_text_field(value, &format!("shared-store provenance {label}"))?;
    }
    Ok(())
}

fn provenance_binding_for_entry(entry: &StoreEntry) -> io::Result<ProvenanceBinding> {
    let producer = super::ProducerRecord::decode(&entry.producer_record)
        .map_err(|error| invalid(&format!("shared-store producer record is invalid: {error}")))?;
    let platform = entry.envelope.platform.clone();
    if platform != entry.cache_identity.platform {
        return Err(invalid(
            "shared-store cache and envelope platforms do not match",
        ));
    }
    let builder = SHA256::sha256_hex(
        format!(
            "jet-builder-v1\n{}\n{}\n{}",
            producer.provider, producer.immutable_source, producer.source_digest
        )
        .as_bytes(),
    );
    let binding = ProvenanceBinding {
        reference: entry.reference.clone(),
        source: entry.cache_identity.source_fingerprint.clone(),
        builder,
        action: entry.cache_identity.recipe_fingerprint.clone(),
        output: entry.envelope.output_hash.clone(),
        platform,
        sandbox: SHA256::sha256_hex(BROKER_SANDBOX_POLICY.as_bytes()),
        policy: entry.cache_identity.policy_fingerprint.clone(),
    };
    validate_provenance_binding(&binding)?;
    Ok(binding)
}

fn broker_trust_key(config: &SharedStoreConfig) -> io::Result<String> {
    if is_admin_config(config) {
        if let Some(path) = std::env::var_os("JET_SHARED_STORE_TRUST_KEY") {
            let path = PathBuf::from(path);
            if !path.is_absolute() {
                return Err(invalid("shared-store broker trust credential is not absolute"));
            }
            return Ok(path.to_string_lossy().into_owned());
        }
    }
    Ok(config.trust_key.to_string_lossy().into_owned())
}

fn writer_credential(config: &SharedStoreConfig) -> io::Result<String> {
    let grant = read_writer_grant(config, current_uid()?)?;
    if !grant.write {
        return Err(invalid("shared-store peer has no write authority"));
    }
    if let Some(expires) = grant.expires {
        if now_secs() >= expires {
            return Err(invalid("shared-store writer grant is expired"));
        }
    }
    grant
        .credential
        .ok_or_else(|| invalid("shared-store writer grant has no credential"))
}

fn sweep_stale_incoming(incoming: &Path) -> io::Result<usize> {
    sweep_stale_incoming_at(incoming, now_secs())
}

fn sweep_stale_incoming_at(incoming: &Path, now: u64) -> io::Result<usize> {
    ensure_real_dir(incoming)?;
    let entries = fs::read_dir(incoming)?.collect::<Result<Vec<_>, _>>()?;
    if entries.len() > MAX_INCOMING_ENTRIES {
        return Err(invalid("shared-store incoming staging directory has too many entries"));
    }
    let mut removed = 0;
    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(invalid("shared-store incoming staging contains a symlink"));
        }
        let created = stage_created_at(&path, &metadata)?;
        if now.saturating_sub(created) > INCOMING_STALE_AFTER_SECS {
            remove_incoming_entry(&path)?;
            removed += 1;
        }
    }
    Ok(removed)
}

fn stage_created_at(path: &Path, metadata: &fs::Metadata) -> io::Result<u64> {
    if metadata.is_dir() {
        let marker = path.join(STAGE_MARKER);
        if let Ok(marker_metadata) = fs::symlink_metadata(&marker) {
            if marker_metadata.file_type().is_symlink() || !marker_metadata.is_file() {
                return Err(invalid("shared-store incoming stage marker is not a regular file"));
            }
            let text = bounded_text_file(&marker, 64, "shared-store incoming stage marker")?;
            return text
                .trim()
                .parse::<u64>()
                .map_err(|_| invalid("shared-store incoming stage marker is invalid"));
        }
    }
    Ok(metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or_default())
}

fn remove_incoming_entry(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(invalid("shared-store incoming staging contains a symlink"));
    }
    if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else if metadata.is_file() {
        fs::remove_file(path)
    } else {
        Err(invalid("shared-store incoming staging has an unsupported entry"))
    }
}

fn bounded_file(path: &Path, limit: usize, label: &str) -> io::Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(invalid(&format!("{label} is too large")));
    }
    Ok(bytes)
}

fn bounded_text_file(path: &Path, limit: usize, label: &str) -> io::Result<String> {
    String::from_utf8(bounded_file(path, limit, label)?)
        .map_err(|_| invalid(&format!("{label} is not UTF-8")))
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn test_binding() -> ProvenanceBinding {
        ProvenanceBinding {
            reference: "demo@ci".into(),
            source: "source".into(),
            builder: "builder".into(),
            action: "action".into(),
            output: "output".into(),
            platform: "platform".into(),
            sandbox: "sandbox".into(),
            policy: "policy".into(),
        }
    }

    #[test]
    fn write_request_round_trips_through_bounded_parser() {
        let mut encoded = Vec::new();
        let binding = test_binding();
        write_request(&mut encoded, &binding, "credential", b"archive").unwrap();

        let request = read_request(&mut Cursor::new(encoded)).unwrap();
        match request {
            BrokerRequest::Write {
                archive,
                binding: actual,
                credential,
            } => {
                assert_eq!(archive, b"archive");
                assert_eq!(actual, binding);
                assert_eq!(credential, "credential");
            }
            BrokerRequest::Read(reference) => {
                panic!("write request parsed as read: {reference}")
            }
        }
    }

    #[test]
    fn request_operations_reject_fields_from_the_other_operation() {
        let write = format!("{REQUEST_MAGIC}\nop=write\nbytes=0\ncredential=00\n\n");
        let error = read_request(&mut Cursor::new(write.into_bytes())).unwrap_err();
        assert!(error.to_string().contains("no `reference`"));

        let read = format!(
            "{REQUEST_MAGIC}\nop=read\nreference={}\nbytes=0\n\n",
            encode_hex(b"app")
        );
        let error = read_request(&mut Cursor::new(read.into_bytes())).unwrap_err();
        assert!(error.to_string().contains("cannot include archive bytes"));
    }

    #[test]
    fn archive_response_round_trips_success_and_missing_status() {
        let mut encoded = Vec::new();
        write_archive_response(&mut encoded, b"archive").unwrap();
        assert_eq!(
            read_archive_response(&mut Cursor::new(encoded)).unwrap(),
            Some(b"archive".to_vec())
        );

        let missing = format!("{RESPONSE_MAGIC}\nstatus=missing\n\n");
        assert_eq!(
            read_archive_response(&mut Cursor::new(missing.into_bytes())).unwrap(),
            None
        );
    }

    #[test]
    fn configured_paths_must_stay_inside_the_installed_boundary() {
        let root = std::env::temp_dir().join(format!(
            "jet-shared-store-config-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(SHARED_DIR)).unwrap();
        let base = root.join(SHARED_DIR).canonicalize().unwrap();
        let config = SharedStoreConfig {
            socket: root.join("outside.sock"),
            shared_root: base.join("root"),
            trust_key: base.join("trust/hangar.key"),
            grants: base.join("users"),
        };
        let roots = Roots {
            root: root.clone(),
            dev_mode: false,
        };

        let layout = user_broker_layout(&roots);
        let error = validate_config_paths(&layout, &config).unwrap_err();
        assert!(error.to_string().contains("outside the installed private boundary"));
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn peer_authorization_requires_read_and_write_grants() {
        let root = std::env::temp_dir().join(format!(
            "jet-shared-store-grants-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let base = root.join(SHARED_DIR);
        let shared = base.join("root");
        let grants = base.join("users");
        fs::create_dir_all(&shared).unwrap();
        fs::create_dir_all(&grants).unwrap();
        let config = SharedStoreConfig {
            socket: base.join("broker.sock"),
            shared_root: shared,
            trust_key: base.join("trust.key"),
            grants: grants.clone(),
        };
        let uid = 4_242_424u32;

        let binding = test_binding();
        assert!(authorize_peer(&config, uid, false, None, None).is_err());
        fs::write(
            grants.join(uid.to_string()),
            "jet-shared-store-grant-v2\nread\n",
        )
        .unwrap();
        assert!(authorize_peer(&config, uid, false, None, None).is_ok());
        assert!(authorize_peer(&config, uid, true, Some(&binding), Some("abc")).is_err());
        fs::write(
            grants.join(uid.to_string()),
            format!(
                "jet-shared-store-grant-v2\nread\nwrite\nexpires={}\ncredential=abc\nsource=source\nbuilder=builder\n",
                now_secs() + GRANT_TTL_SECS
            ),
        )
        .unwrap();
        assert!(authorize_peer(&config, uid, true, Some(&binding), Some("abc")).is_ok());

        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn admin_service_unit_uses_ephemeral_non_root_state_boundary() {
        let config = SharedStoreConfig {
            socket: PathBuf::from(ADMIN_SOCKET),
            shared_root: PathBuf::from(ADMIN_BASE).join("root"),
            trust_key: PathBuf::from(ADMIN_BASE).join("trust/hangar.key"),
            grants: PathBuf::from(ADMIN_BASE).join("users"),
        };
        let service = admin_service_unit_text(Path::new("/usr/bin/jetpack"), &config);
        assert!(service.contains("DynamicUser=yes"), "{service}");
        assert!(!service.contains("User=root"), "{service}");
        assert!(service.contains("StateDirectory=jet/shared-store/root\n"));
        assert!(service.contains("ReadOnlyPaths=/var/lib/jet/shared-store/users"));
        assert!(service.contains("NoNewPrivileges=yes"));
        assert!(service.contains("RestrictAddressFamilies=AF_UNIX"));
    }

    #[cfg(unix)]
    #[test]
    fn admin_service_unit_passes_systemd_unit_validation() {
        let root = std::env::temp_dir().join(format!(
            "jet-shared-store-systemd-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let trust = root.join("trust");
        let grants = root.join("users");
        let shared = root.join("root");
        fs::create_dir_all(&trust).unwrap();
        fs::create_dir_all(&grants).unwrap();
        fs::create_dir_all(&shared).unwrap();
        fs::write(trust.join("hangar.key"), [b'k'; 32]).unwrap();
        let config = SharedStoreConfig {
            socket: root.join("broker.sock"),
            shared_root: shared,
            trust_key: trust.join("hangar.key"),
            grants,
        };
        let service_path = root.join("jet-shared-store.service");
        let socket_path = root.join("jet-shared-store.socket");
        fs::write(
            &service_path,
            admin_service_unit_text(&std::env::current_exe().unwrap(), &config),
        )
        .unwrap();
        fs::write(
            &socket_path,
            format!(
                "[Unit]\nDescription=Jet shared-store broker socket\n\n[Socket]\nListenStream={}\nSocketMode=0666\nDirectoryMode=0755\nRemoveOnStop=yes\n",
                config.socket.display()
            ),
        )
        .unwrap();
        let status = std::process::Command::new("systemd-analyze")
            .args(["verify"])
            .arg(&service_path)
            .arg(&socket_path)
            .status();
        if let Ok(status) = status {
            assert!(status.success(), "systemd-analyze verify failed: {status}");
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn stale_incoming_cleanup_removes_old_stages_and_rejects_symlinks() {
        let root = std::env::temp_dir().join(format!(
            "jet-shared-store-incoming-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("old-stage")).unwrap();
        fs::create_dir_all(root.join("fresh-stage")).unwrap();
        let now = now_secs();
        fs::write(
            root.join("old-stage").join(STAGE_MARKER),
            now.saturating_sub(INCOMING_STALE_AFTER_SECS + 1).to_string(),
        )
        .unwrap();
        fs::write(root.join("fresh-stage").join(STAGE_MARKER), now.to_string()).unwrap();

        let error = sweep_stale_incoming_at(&root, now).unwrap();
        assert_eq!(error, 1);
        assert!(!root.join("old-stage").exists());
        assert!(root.join("fresh-stage").exists());

        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("fresh-stage"), root.join("link")).unwrap();
        #[cfg(unix)]
        assert!(sweep_stale_incoming_at(&root, now_secs()).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn write_request_binds_all_provenance_facts() {
        let binding = ProvenanceBinding {
            reference: "demo@ci".into(),
            source: "source-digest".into(),
            builder: "builder-digest".into(),
            action: "action-digest".into(),
            output: "sha256-output".into(),
            platform: "x86_64-linux".into(),
            sandbox: "sandbox-digest".into(),
            policy: "policy-digest".into(),
        };
        let mut encoded = Vec::new();
        write_request(&mut encoded, &binding, "credential", b"archive").unwrap();
        let request = read_request(&mut Cursor::new(encoded)).unwrap();
        match request {
            BrokerRequest::Write {
                archive,
                binding: actual,
                credential,
            } => {
                assert_eq!(archive, b"archive");
                assert_eq!(actual, binding);
                assert_eq!(credential, "credential");
            }
            BrokerRequest::Read(reference) => panic!("write request parsed as read: {reference}"),
        }
    }

    #[test]
    fn writer_grants_require_short_lived_source_and_builder_authority() {
        let incomplete = "jet-shared-store-grant-v2\nread\nwrite\n";
        let error = parse_writer_grant(incomplete).unwrap_err();
        assert!(error.to_string().contains("expiry"));

        let expired = format!(
            "jet-shared-store-grant-v2\nread\nwrite\nexpires={}\ncredential=abc\nsource=source\nbuilder=builder\n",
            now_secs().saturating_sub(1)
        );
        let grant = parse_writer_grant(&expired).unwrap();
        let binding = test_binding();
        let error = authorize_grant(&grant, true, &binding, "abc").unwrap_err();
        assert!(error.to_string().contains("expired"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_peer_credentials_prove_namespace_uid_is_not_socket_mode() {
        use std::os::unix::net::{UnixListener, UnixStream};
        use std::time::{Duration, Instant};

        let socket = std::env::temp_dir().join(format!(
            "jet-shared-store-peer-{}",
            std::process::id()
        ));
        let _ = fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let child = std::process::Command::new("unshare")
            .args([
                "--user",
                "--map-users=100000,0,1",
                "--map-groups=100000,0,1",
                "--setgroups=deny",
                "--",
            ])
            .arg(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "Store::Broker::tests::broker_peer_probe_child",
                "--nocapture",
            ])
            .env("JET_BROKER_PROBE_SOCKET", &socket)
            .spawn();
        let Ok(mut child) = child else {
            let _ = fs::remove_file(socket);
            return;
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut accepted = None;
        while Instant::now() < deadline {
            match listener.accept() {
                Ok((stream, _)) => {
                    accepted = Some(stream);
                    break;
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("peer proof listener failed: {error}"),
            }
        }
        let status = child.wait().unwrap();
        let Some(stream) = accepted else {
            eprintln!("note: unprivileged user namespaces unavailable; peer proof not run");
            let _ = fs::remove_file(socket);
            return;
        };
        let uid = peer_uid(&stream).unwrap();
        assert_ne!(uid, current_uid().unwrap());
        drop(stream);
        assert!(status.success(), "peer probe failed: {status}");
        let _ = UnixStream::connect(&socket);
        let _ = fs::remove_file(socket);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn broker_peer_probe_child() {
        let Some(socket) = std::env::var_os("JET_BROKER_PROBE_SOCKET") else {
            return;
        };
        let _ = std::os::unix::net::UnixStream::connect(socket).unwrap();
    }

}
