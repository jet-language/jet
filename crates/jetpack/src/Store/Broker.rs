//! Optional rootless shared-store broker.
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
const SHARED_DIR: &str = "shared-store";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedStoreConfig {
    pub socket: PathBuf,
    pub shared_root: PathBuf,
    pub trust_key: PathBuf,
    pub writer_token: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedStoreInstallReport {
    pub config: PathBuf,
    pub socket_unit: Option<PathBuf>,
    pub service_unit: Option<PathBuf>,
}

pub fn shared_store_config(roots: &Roots) -> io::Result<Option<SharedStoreConfig>> {
    let path = config_path(roots);
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
    let text = fs::read_to_string(&path)?;
    let mut fields = std::collections::BTreeMap::new();
    let mut lines = text.lines();
    if lines.next() != Some(CONFIG_MAGIC) {
        return Err(invalid("shared-store config has an unknown format"));
    }
    for line in lines {
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| invalid("shared-store config has a malformed field"))?;
        if !matches!(key, "socket" | "shared_root" | "trust_key" | "writer_token")
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
    Ok(Some(SharedStoreConfig {
        socket: decode("socket")?,
        shared_root: decode("shared_root")?,
        trust_key: decode("trust_key")?,
        writer_token: decode("writer_token")?,
    }))
}

/// Create the user-owned broker configuration and socket-activation units.
/// The command does not create a root-owned store and does not enable a
/// resident daemon. Systemd starts the one-request service only on demand.
pub fn install_shared_store(roots: &Roots) -> io::Result<SharedStoreInstallReport> {
    let base = roots.root.join(SHARED_DIR);
    ensure_private_dir(&base)?;
    let shared_root = base.join("root");
    ensure_private_dir(&shared_root)?;
    let trust_dir = base.join("trust");
    ensure_private_dir(&trust_dir)?;
    let trust_key = trust_dir.join("hangar.key");
    let writer_token = trust_dir.join("writer.token");
    ensure_secret(&trust_key, b"shared-store trust key", roots)?;
    ensure_secret(&writer_token, b"shared-store writer token", roots)?;

    let socket = base.join("broker.sock");
    let config = SharedStoreConfig {
        socket,
        shared_root,
        trust_key,
        writer_token,
    };
    let config_path = config_path(roots);
    let mut text = String::from(CONFIG_MAGIC);
    text.push('\n');
    for (key, value) in [
        ("socket", &config.socket),
        ("shared_root", &config.shared_root),
        ("trust_key", &config.trust_key),
        ("writer_token", &config.writer_token),
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

    #[cfg(unix)]
    {
        let executable = std::env::current_exe()?;
        let unit_dir = user_systemd_unit_dir()?;
        ensure_real_dir(&unit_dir)?;
        let socket_unit = unit_dir.join("jet-shared-store.socket");
        let service_unit = unit_dir.join("jet-shared-store.service");
        let socket_text = format!(
            "[Unit]\nDescription=Jet shared-store broker socket\n\n[Socket]\nListenStream={}\nSocketMode=0600\n\n[Install]\nWantedBy=sockets.target\n",
            config.socket.display()
        );
        let service_text = format!(
            "[Unit]\nDescription=Jet shared-store broker request\nRequires=jet-shared-store.socket\n\n[Service]\nType=oneshot\nExecStart={} shared-store broker --fd 3\n",
            executable.display()
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
    let shared = Roots {
        root: config.shared_root.clone(),
        dev_mode: false,
    };
    let candidate = super::find_by_reference(&shared, reference);
    let Some(candidate) = candidate else {
        return Ok(None);
    };
    let proof = super::verify_cache_entry(&shared, &candidate, reference, expectation);
    if !proof.trusted() {
        return Err(invalid("shared-store entry failed cache identity verification"));
    }
    let key = config.trust_key.to_string_lossy().to_string();
    let (bytes, _) = Archive::export_archive(&shared, &candidate.id, true, Some(&key))?;
    Archive::import_archive(roots, &bytes, Some(&key), false)?;
    Ok(super::find_by_reference(roots, reference))
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
    let token = read_secret(&config.writer_token, "shared-store writer token")?;
    let key = config.trust_key.to_string_lossy().to_string();
    let (archive, _) = Archive::export_archive(roots, &entry.id, true, Some(&key))?;
    #[cfg(unix)]
    {
        use std::os::unix::net::UnixStream;
        let mut stream = match UnixStream::connect(&config.socket) {
            Ok(stream) => stream,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                ) => return Ok(false),
            Err(error) => return Err(error),
        };
        write_request(&mut stream, &token, &archive)?;
        read_response(&mut stream)
    }
    #[cfg(not(unix))]
    {
        let _ = (config, token, archive);
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
    let token = read_secret(&config.writer_token, "shared-store writer token")?;
    let archive = read_request(&mut stream, &token)?;
    let shared = Roots {
        root: config.shared_root,
        dev_mode: false,
    };
    let key = config.trust_key.to_string_lossy().to_string();
    Archive::import_archive(&shared, &archive, Some(&key), false)?;
    write_response(&mut stream, "ok")
}

#[cfg(not(unix))]
pub fn serve_shared_store_fd(_roots: &Roots, _fd: i32) -> io::Result<()> {
    Err(invalid("shared-store socket activation is unsupported on this host"))
}

fn write_request(stream: &mut impl Write, token: &[u8], archive: &[u8]) -> io::Result<()> {
    if archive.len() > MAX_REQUEST_BYTES {
        return Err(invalid("shared-store archive exceeds the request limit"));
    }
    writeln!(stream, "{REQUEST_MAGIC}")?;
    writeln!(stream, "token={}", encode_hex(token))?;
    writeln!(stream, "bytes={}", archive.len())?;
    writeln!(stream)?;
    stream.write_all(archive)?;
    stream.flush()
}

fn read_request(stream: &mut impl Read, expected_token: &[u8]) -> io::Result<Vec<u8>> {
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
        if !matches!(key, "token" | "bytes") || fields.insert(key, value).is_some() {
            return Err(invalid("shared-store request has an unknown or duplicate field"));
        }
    }
    if decode_hex(
        fields
            .get("token")
            .ok_or_else(|| invalid("shared-store request has no writer token"))?,
    )? != expected_token
    {
        return Err(invalid("shared-store writer token is invalid"));
    }
    let length = fields
        .get("bytes")
        .ok_or_else(|| invalid("shared-store request has no archive length"))?
        .parse::<usize>()
        .map_err(|_| invalid("shared-store archive length is invalid"))?;
    if length > MAX_REQUEST_BYTES {
        return Err(invalid("shared-store archive exceeds the request limit"));
    }
    let mut archive = vec![0u8; length];
    stream.read_exact(&mut archive)?;
    Ok(archive)
}

fn write_response(stream: &mut impl Write, status: &str) -> io::Result<()> {
    writeln!(stream, "{RESPONSE_MAGIC}")?;
    writeln!(stream, "status={status}")?;
    writeln!(stream)?;
    stream.flush()
}

fn read_response(stream: &mut impl Read) -> io::Result<bool> {
    let mut text = String::new();
    stream.read_to_string(&mut text)?;
    let mut lines = text.lines();
    if lines.next() != Some(RESPONSE_MAGIC) {
        return Err(invalid("shared-store response has an unknown format"));
    }
    Ok(lines.any(|line| line == "status=ok"))
}

fn config_path(roots: &Roots) -> PathBuf {
    roots.root.join(SHARED_DIR).join("config")
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
            let secret = fs::read(path)?;
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

fn read_secret(path: &Path, label: &str) -> io::Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(invalid(&format!("{label} is not a regular file")));
    }
    require_private_mode(path, label)?;
    let secret = fs::read(path)?;
    if secret.len() < 32 {
        return Err(invalid(&format!("{label} is too short")));
    }
    Ok(secret)
}

fn entropy(roots: &Roots, label: &[u8]) -> Vec<u8> {
    #[cfg(unix)]
    if let Ok(bytes) = fs::read("/dev/urandom") {
        if bytes.len() >= 32 {
            return bytes[..32].to_vec();
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

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}
