//! Optional administrator-installed shared-store broker.
//!
//! The broker accepts one authenticated, signed Hangar archive per activated
//! process. It never receives source, a build command, or an evaluator input.
//! A missing broker is transparent: callers keep using their per-user Hangar.

use super::{Archive, CacheExpectation, Roots, StoreEntry};
use crate::SHA256;
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

fn broker_layout(roots: &Roots) -> BrokerLayout {
    if roots.dev_mode {
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
    } else {
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
    validate_config_paths(roots, &config)?;
    Ok(Some(config))
}

/// Create the administrator-selected broker configuration and
/// socket-activation units. The command does not create a resident daemon;
/// systemd starts the one-request service only on demand. Ordinary callers
/// continue to use their per-user Hangar when this configuration is absent.
pub fn install_shared_store(roots: &Roots) -> io::Result<SharedStoreInstallReport> {
    let mut layout = broker_layout(roots);
    if layout.admin {
        ensure_system_dir(layout.base.parent().unwrap_or(Path::new("/")))?;
        ensure_system_dir(layout.config.parent().unwrap_or(Path::new("/")))?;
        ensure_system_dir(layout.socket.parent().unwrap_or(Path::new("/")))?;
    } else {
        ensure_private_dir(&layout.base)?;
    }
    ensure_private_dir(&layout.shared_root)?;
    let trust_dir = layout
        .trust_key
        .parent()
        .ok_or_else(|| invalid("shared-store trust key has no parent"))?;
    ensure_private_dir(trust_dir)?;
    ensure_private_dir(&layout.grants)?;
    ensure_secret(&layout.trust_key, b"shared-store trust key", roots)?;

    let base = layout.base.canonicalize()?;
    if !layout.admin {
        layout.base = base.clone();
        layout.config = base.join("config");
        layout.socket = base.join("broker.sock");
        layout.shared_root = base.join("root");
        layout.trust_key = base.join("trust/hangar.key");
        layout.grants = base.join("users");
    }
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
        let service_identity = if layout.admin { "User=root\nPrivateUsers=no\n" } else { "" };
        let service_text = format!(
            "[Unit]\nDescription=Jet shared-store broker request\nRequires=jet-shared-store.socket\n\n[Service]\nType=oneshot\nExecStart={} shared-store broker --fd 3\n{}NoNewPrivileges=yes\nPrivateTmp=yes\nProtectSystem=strict\nProtectHome=read-only\nRestrictAddressFamilies=AF_UNIX\nIPAddressDeny=any\nUMask=0077\nTimeoutStartSec=120\nReadWritePaths={}\n",
            systemd_escape_path(&executable),
            service_identity,
            systemd_escape_path(&config.shared_root)
        );
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
        let key = config.trust_key.to_string_lossy().to_string();
        Archive::import_archive(roots, &bytes, Some(&key), false)?;
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
        let key = config.trust_key.to_string_lossy().to_string();
        let (archive, _) = Archive::export_archive(roots, &entry.id, true, Some(&key))?;
        let mut stream = match UnixStream::connect(&config.socket) {
            Ok(stream) => stream,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                ) => return Ok(false),
            Err(error) => return Err(error),
        };
        write_request(&mut stream, &archive)?;
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
pub fn serve_shared_store_fd(roots: &Roots, fd: i32) -> io::Result<()> {
    use std::os::fd::FromRawFd;
    use std::os::unix::net::UnixListener;
    if fd < 0 {
        return Err(invalid("shared-store broker fd is negative"));
    }
    // SAFETY: systemd owns fd 3 for the lifetime of this one-shot process and
    // passes a connected AF_UNIX listener according to the generated unit.
    let listener = unsafe { UnixListener::from_raw_fd(fd) };
    let (mut stream, _) = listener.accept()?;
    let config = shared_store_config(roots)?
        .ok_or_else(|| invalid("shared-store broker is not installed"))?;
    let shared = Roots {
        root: config.shared_root,
        dev_mode: false,
    };
    let key = config.trust_key.to_string_lossy().to_string();
    let uid = peer_uid(&stream)?;
    match read_request_header(&mut stream)? {
        BrokerHeader::Write(length) => {
            authorize_peer(&config, uid, true)?;
            let mut archive = vec![0u8; length];
            stream.read_exact(&mut archive)?;
            Archive::import_archive(&shared, &archive, Some(&key), false)?;
            write_response(&mut stream, "ok")
        }
        BrokerHeader::Read(reference) => {
            authorize_peer(&config, uid, false)?;
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

#[cfg(not(unix))]
pub fn serve_shared_store_fd(_roots: &Roots, _fd: i32) -> io::Result<()> {
    Err(invalid("shared-store socket activation is unsupported on this host"))
}

#[derive(Debug)]
enum BrokerRequest {
    Write(Vec<u8>),
    Read(String),
}

fn write_request(stream: &mut impl Write, archive: &[u8]) -> io::Result<()> {
    if archive.len() > MAX_REQUEST_BYTES {
        return Err(invalid("shared-store archive exceeds the request limit"));
    }
    writeln!(stream, "{REQUEST_MAGIC}")?;
    writeln!(stream, "op=write")?;
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
    Write(usize),
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
        if !matches!(key, "op" | "bytes" | "reference")
            || fields.insert(key, value).is_some()
        {
            return Err(invalid("shared-store request has an unknown or duplicate field"));
        }
    }
    match fields
        .get("op")
        .ok_or_else(|| invalid("shared-store request has no operation"))?
    {
        "write" => {
            if fields.contains_key("reference") {
                return Err(invalid(
                    "shared-store write request cannot include a reference",
                ));
            }
            let length = fields
                .get("bytes")
                .ok_or_else(|| invalid("shared-store request has no archive length"))?
                .parse::<usize>()
                .map_err(|_| invalid("shared-store archive length is invalid"))?;
            if length > MAX_REQUEST_BYTES {
                return Err(invalid("shared-store archive exceeds the request limit"));
            }
            Ok(BrokerHeader::Write(length))
        }
        "read" => {
            if fields.contains_key("bytes") {
                return Err(invalid("shared-store read request cannot include archive bytes"));
            }
            let encoded = fields
                .get("reference")
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

fn read_request(stream: &mut impl Read) -> io::Result<BrokerRequest> {
    match read_request_header(stream)? {
        BrokerHeader::Write(length) => {
            let mut archive = vec![0u8; length];
            stream.read_exact(&mut archive)?;
            Ok(BrokerRequest::Write(archive))
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

fn authorize_peer(config: &SharedStoreConfig, uid: u32, write: bool) -> io::Result<()> {
    if uid == 0 {
        return Ok(());
    }
    // A rootless fixture can use the owner of its private shared root without
    // an enrollment file. Administrator-owned layouts require an explicit
    // per-uid grant, so a socket permission alone never grants store access.
    #[cfg(unix)]
    if !is_admin_config(config) && owner_uid(&config.shared_root)? == uid {
        return Ok(());
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
    let text = bounded_text_file(&grant, MAX_CONFIG_BYTES, "shared-store peer enrollment")?;
    let mut read = false;
    let mut write_grant = false;
    let mut lines = text.lines();
    if lines.next() != Some("jet-shared-store-grant-v1") {
        return Err(invalid("shared-store peer enrollment has an unknown format"));
    }
    for line in lines {
        match line {
            "read" => read = true,
            "write" => write_grant = true,
            "" => {}
            _ => return Err(invalid("shared-store peer enrollment has an unknown capability")),
        }
    }
    if !read || (write && !write_grant) {
        return Err(invalid("shared-store peer enrollment is incomplete"));
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
    let mut text = String::from("jet-shared-store-grant-v1\nread\n");
    if writable {
        text.push_str("write\n");
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

fn validate_config_paths(roots: &Roots, config: &SharedStoreConfig) -> io::Result<()> {
    let layout = broker_layout(roots);
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
    validate_private_directory(&base, "shared-store directory")?;
    validate_private_directory(&config.shared_root, "shared-store root")?;
    let trust_dir = base.join("trust");
    validate_private_directory(&trust_dir, "shared-store trust directory")?;
    validate_private_directory(&config.grants, "shared-store grants directory")?;
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

fn bounded_file(path: &Path, limit: usize, label: &str) -> io::Result<Vec<u8>> {
    let mut file = fs::File::open(path)?;
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

    #[test]
    fn write_request_round_trips_through_bounded_parser() {
        let mut encoded = Vec::new();
        write_request(&mut encoded, b"archive").unwrap();

        let request = read_request(&mut Cursor::new(encoded)).unwrap();
        match request {
            BrokerRequest::Write(archive) => assert_eq!(archive, b"archive"),
            BrokerRequest::Read(_) => panic!("write request parsed as read"),
        }
    }

    #[test]
    fn request_operations_reject_fields_from_the_other_operation() {
        let write = format!(
            "{REQUEST_MAGIC}\nop=write\nbytes=0\nreference=00\n\n"
        );
        let error = read_request(&mut Cursor::new(write.into_bytes())).unwrap_err();
        assert!(error.to_string().contains("cannot include a reference"));

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

        let error = validate_config_paths(&roots, &config).unwrap_err();
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

        assert!(authorize_peer(&config, uid, false).is_err());
        fs::write(
            grants.join(uid.to_string()),
            "jet-shared-store-grant-v1\nread\n",
        )
        .unwrap();
        assert!(authorize_peer(&config, uid, false).is_ok());
        assert!(authorize_peer(&config, uid, true).is_err());
        fs::write(
            grants.join(uid.to_string()),
            "jet-shared-store-grant-v1\nread\nwrite\n",
        )
        .unwrap();
        assert!(authorize_peer(&config, uid, true).is_ok());

        fs::remove_dir_all(root).unwrap();
    }
}
