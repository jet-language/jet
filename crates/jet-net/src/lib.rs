//! D-NETDEP1=A: blocking HTTP/file fetch for comptime `core.net.fetch`.
//!
//! Handles `file://` via std::fs and `http(s)://` via ureq.
//! D-TLS1=A: HTTPS is available in the default build through rustls plus
//! system trust roots. Use `--no-default-features` only for size/freestanding
//! builds that knowingly drop HTTPS.

use std::fs::File;
#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    windows
))]
use std::fs;
use std::io::Read;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::path::Path;
use std::sync::LazyLock;
use std::time::Duration;

static HTTP_AGENT: LazyLock<ureq::Agent> =
    LazyLock::new(|| ureq::AgentBuilder::new().redirects(0).build());
static COMPTIME_HTTP_AGENT: LazyLock<ureq::Agent> = LazyLock::new(|| {
    ureq::AgentBuilder::new()
        .redirects(0)
        .resolver(restricted_resolver)
        .build()
});

const MAX_FETCH_BYTES: usize = 64 * 1024 * 1024;

pub struct StreamResponse {
    status: u16,
    content_length: Option<u64>,
    location: Option<String>,
    reader: Box<dyn Read + Send>,
}

impl StreamResponse {
    pub fn status(&self) -> u16 {
        self.status
    }

    pub fn content_length(&self) -> Option<u64> {
        self.content_length
    }

    /// The redirect target, when the response carries a `Location` header.
    pub fn location(&self) -> Option<&str> {
        self.location.as_deref()
    }
}

impl Read for StreamResponse {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.reader.read(buffer)
    }
}

#[derive(Debug)]
pub enum FetchError {
    IO(String),
    HTTP {
        kind: FetchErrorKind,
        detail: String,
    },
    Scheme(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchErrorKind {
    General,
    TLSHandshake,
    TLSCertificate,
    TLSTrustRoots,
}

impl FetchError {
    pub fn diagnostic_code(&self) -> &'static str {
        match self {
            FetchError::HTTP {
                kind: FetchErrorKind::TLSHandshake,
                ..
            } => "E4201",
            FetchError::HTTP {
                kind: FetchErrorKind::TLSCertificate,
                ..
            } => "E4202",
            FetchError::HTTP {
                kind: FetchErrorKind::TLSTrustRoots,
                ..
            } => "E4203",
            FetchError::IO(_) | FetchError::HTTP { .. } | FetchError::Scheme(_) => "E3414",
        }
    }

    pub fn diagnostic_what(&self, url: &str) -> String {
        let host = host_from_url(url);
        match self {
            FetchError::HTTP {
                kind: FetchErrorKind::TLSHandshake,
                ..
            } => format!("TLS handshake with `{host}` failed"),
            FetchError::HTTP {
                kind: FetchErrorKind::TLSCertificate,
                ..
            } => format!("TLS certificate for `{host}` could not be trusted"),
            FetchError::HTTP {
                kind: FetchErrorKind::TLSTrustRoots,
                ..
            } => "HTTPS could not find system certificate roots".to_string(),
            _ => format!("fetch failed: {self}"),
        }
    }

    pub fn diagnostic_why(&self, url: &str) -> String {
        match self {
            FetchError::HTTP {
                kind: FetchErrorKind::TLSHandshake,
                ..
            } => format!(
                "`{url}` reached the server, but the connection did not complete a secure HTTPS handshake"
            ),
            FetchError::HTTP {
                kind: FetchErrorKind::TLSCertificate,
                ..
            } => format!(
                "`{url}` presented a certificate Jet could not verify for that host"
            ),
            FetchError::HTTP {
                kind: FetchErrorKind::TLSTrustRoots,
                ..
            } => "Jet uses rustls with the system trust store for default HTTPS, but no usable roots were available".to_string(),
            _ => format!("could not retrieve `{url}`"),
        }
    }

    pub fn diagnostic_fix(&self) -> String {
        match self {
            FetchError::HTTP {
                kind: FetchErrorKind::TLSHandshake,
                ..
            } => "verify the URL points at an HTTPS server, not plain HTTP; for local tests, start the TLS fixture server".to_string(),
            FetchError::HTTP {
                kind: FetchErrorKind::TLSCertificate,
                ..
            } => "use a certificate whose subject matches the host and chains to a trusted root; for tests, trust the local fixture CA explicitly".to_string(),
            FetchError::HTTP {
                kind: FetchErrorKind::TLSTrustRoots,
                ..
            } => "install the system certificate bundle (for example `ca-certificates`) or run in an image that includes it".to_string(),
            _ => "check the URL is reachable and the network is available; use `file://` for a local path inside the compile-time source directory".to_string(),
        }
    }

    fn http(url: &str, detail: String) -> Self {
        FetchError::HTTP {
            kind: classify_http_error(url, &detail),
            detail,
        }
    }
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::IO(s) | FetchError::Scheme(s) => f.write_str(s),
            FetchError::HTTP {
                kind: FetchErrorKind::General,
                detail,
            } => f.write_str(detail),
            FetchError::HTTP {
                kind: FetchErrorKind::TLSHandshake,
                ..
            } => f.write_str("TLS handshake failed"),
            FetchError::HTTP {
                kind: FetchErrorKind::TLSCertificate,
                ..
            } => f.write_str("TLS certificate could not be trusted"),
            FetchError::HTTP {
                kind: FetchErrorKind::TLSTrustRoots,
                ..
            } => f.write_str("HTTPS could not find system certificate roots"),
        }
    }
}

/// Fetch `url` and return the raw bytes.
///
/// Supports:
/// - `file:///path` → `std::fs::read`, scoped to the current directory
/// - `http://…` / `https://…` → ureq blocking GET
pub fn fetch(url: &str) -> Result<Vec<u8>, FetchError> {
    fetch_in_root(url, Path::new("."))
}

/// Fetch a comptime resource while keeping local files below `base_dir` and
/// network connections on publicly routable destinations.
pub fn fetch_in_root(url: &str, base_dir: &Path) -> Result<Vec<u8>, FetchError> {
    fetch_with_root_timeout(url, Duration::from_secs(30), base_dir)
}

fn fetch_with_root_timeout(
    url: &str,
    timeout: Duration,
    base_dir: &Path,
) -> Result<Vec<u8>, FetchError> {
    if let Some(path) = url.strip_prefix("file://") {
        let file = open_scoped_file(path, base_dir)?;
        read_limited(file, MAX_FETCH_BYTES).map_err(|e| FetchError::IO(e.to_string()))
    } else if url.starts_with("http://") || url.starts_with("https://") {
        read_limited(comptime_http_stream(url, timeout)?, MAX_FETCH_BYTES)
            .map_err(|e| FetchError::http(url, e.to_string()))
    } else {
        let scheme = url.find("://").map(|i| &url[..i]).unwrap_or(url);
        Err(FetchError::Scheme(format!(
            "unsupported URL scheme `{scheme}`; expected `file://`, `http://`, or `https://`"
        )))
    }
}

fn open_scoped_file(raw_path: &str, base_dir: &Path) -> Result<File, FetchError> {
    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    {
        if raw_path == "/dev/null" {
            return local_authority::open_no_follow(Path::new(raw_path))
                .map_err(|error| FetchError::IO(error.to_string()));
        }

        let before = fs::symlink_metadata(base_dir)
            .map_err(|error| FetchError::IO(error.to_string()))?;
        if before.file_type().is_symlink() || !before.is_dir() {
            return Err(FetchError::IO(
                "compile-time source directory must be a real directory".to_string(),
            ));
        }
        let root = fs::canonicalize(base_dir)
            .map_err(|error| FetchError::IO(error.to_string()))?;
        let after = fs::symlink_metadata(base_dir)
            .map_err(|error| FetchError::IO(error.to_string()))?;
        if after.file_type().is_symlink()
            || !after.is_dir()
            || !local_authority::same_directory(&before, &after)
        {
            return Err(FetchError::IO(
                "compile-time source directory changed during resolution".to_string(),
            ));
        }

        let root_handle = local_authority::open_root(&root)
            .map_err(|error| FetchError::IO(error.to_string()))?;
        let opened_root = root_handle
            .metadata()
            .map_err(|error| FetchError::IO(error.to_string()))?;
        if !local_authority::same_directory(&after, &opened_root) {
            return Err(FetchError::IO(
                "compile-time source directory changed during resolution".to_string(),
            ));
        }

        let requested = Path::new(raw_path);
        let requested = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            root.join(requested)
        };
        let canonical = fs::canonicalize(&requested)
            .map_err(|error| FetchError::IO(error.to_string()))?;
        if !canonical.starts_with(&root) {
            return Err(FetchError::IO(
                "file URL resolves outside the compile-time source directory".to_string(),
            ));
        }
        let relative = canonical.strip_prefix(&root).map_err(|_| {
            FetchError::IO("file URL resolves outside the compile-time source directory".to_string())
        })?;
        let file = local_authority::open_relative(&root_handle, relative)
            .map_err(|error| FetchError::IO(error.to_string()))?;
        let metadata = file
            .metadata()
            .map_err(|error| FetchError::IO(error.to_string()))?;
        if !metadata.is_file()
            || !local_authority::is_single_link_file(&file)
                .map_err(|error| FetchError::IO(error.to_string()))?
        {
            return Err(FetchError::IO(
                "file URL must name a regular, unshared file".to_string(),
            ));
        }
        return Ok(file);
    }

    #[cfg(windows)]
    {
        let root = fs::canonicalize(base_dir)
            .map_err(|error| FetchError::IO(error.to_string()))?;
        let requested = Path::new(raw_path);
        let requested = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            root.join(requested)
        };
        return local_authority::open_file(&root, &requested)
            .map_err(|error| FetchError::IO(error.to_string()));
    }

    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        windows
    )))]
    {
        let _ = (raw_path, base_dir);
        Err(FetchError::IO(
            "descriptor-relative no-follow file access is unavailable on this platform".to_string(),
        ))
    }
}

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios"
))]
mod local_authority {
    use super::*;
    use std::ffi::{c_char, CString};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    use std::path::Component;

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

    unsafe extern "C" {
        fn openat(directory: i32, path: *const c_char, flags: i32, ...) -> i32;
    }

    pub(super) fn same_directory(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
        left.is_dir() && right.is_dir() && left.dev() == right.dev() && left.ino() == right.ino()
    }

    pub(super) fn open_no_follow(path: &Path) -> std::io::Result<File> {
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(O_NOFOLLOW | O_CLOEXEC)
            .open(path)
    }

    pub(super) fn open_root(path: &Path) -> std::io::Result<File> {
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
            .open(path)
    }

    pub(super) fn open_relative(root: &File, relative: &Path) -> std::io::Result<File> {
        let components = relative.components().collect::<Vec<_>>();
        if components.is_empty() {
            return root.try_clone();
        }
        let mut current = root.try_clone()?;
        for (index, component) in components.iter().enumerate() {
            let Component::Normal(name) = component else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "file authority path has unsupported components",
                ));
            };
            let name = CString::new(name.as_bytes()).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "file authority path contains NUL",
                )
            })?;
            let last = index + 1 == components.len();
            let flags = if last {
                O_NOFOLLOW | O_CLOEXEC
            } else {
                O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC
            };
            let fd = unsafe { openat(current.as_raw_fd(), name.as_ptr(), flags, 0) };
            if fd < 0 {
                return Err(std::io::Error::last_os_error());
            }
            let opened = unsafe { File::from_raw_fd(fd) };
            if last {
                return Ok(opened);
            }
            current = opened;
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "file authority path is empty",
        ))
    }

    /// A hardlink inside the root can name an inode whose original path is
    /// outside the root. Path containment cannot distinguish that alias.
    pub(super) fn is_single_link_file(file: &File) -> std::io::Result<bool> {
        Ok(file.metadata()?.nlink() == 1)
    }
}

#[cfg(windows)]
mod local_authority {
    use super::*;
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;

    type Handle = *mut c_void;

    const GENERIC_READ: u32 = 0x8000_0000;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;

    #[repr(C)]
    struct FileStandardInfo {
        allocation_size: i64,
        end_of_file: i64,
        number_of_links: u32,
        delete_pending: u8,
        directory: u8,
    }

    unsafe extern "system" {
        fn GetFileAttributesW(name: *const u16) -> u32;
        fn GetFinalPathNameByHandleW(
            file: Handle,
            path: *mut u16,
            path_len: u32,
            flags: u32,
        ) -> u32;
        fn GetFileInformationByHandleEx(
            file: Handle,
            information_class: u32,
            information: *mut c_void,
            buffer_size: u32,
        ) -> i32;
    }

    fn wide_path(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    fn is_reparse(path: &Path) -> std::io::Result<bool> {
        let wide = wide_path(path);
        let attributes = unsafe { GetFileAttributesW(wide.as_ptr()) };
        if attributes == u32::MAX {
            return Err(std::io::Error::last_os_error());
        }
        Ok(attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0)
    }

    fn final_path(file: &File) -> std::io::Result<String> {
        let needed = unsafe {
            GetFinalPathNameByHandleW(file.as_raw_handle(), std::ptr::null_mut(), 0, 0)
        };
        if needed == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut buffer = vec![0u16; needed as usize + 1];
        let written = unsafe {
            GetFinalPathNameByHandleW(
                file.as_raw_handle(),
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                0,
            )
        };
        if written == 0 || written as usize >= buffer.len() {
            return Err(std::io::Error::last_os_error());
        }
        buffer.truncate(written as usize);
        Ok(String::from_utf16_lossy(&buffer))
    }

    fn is_single_link_file(file: &File) -> std::io::Result<bool> {
        let mut information = std::mem::MaybeUninit::<FileStandardInfo>::zeroed();
        let result = unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                1,
                information.as_mut_ptr().cast(),
                std::mem::size_of::<FileStandardInfo>() as u32,
            )
        };
        if result == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(unsafe { information.assume_init() }.number_of_links == 1)
    }

    fn normalized_final_path(path: String) -> String {
        path.replace('/', "\\")
            .trim_start_matches(r"\\?\")
            .trim_end_matches(['\\', '/'])
            .to_ascii_lowercase()
    }

    fn is_beneath(root: &str, candidate: &str) -> bool {
        candidate == root
            || candidate
                .strip_prefix(root)
                .is_some_and(|tail| tail.starts_with('\\'))
    }

    pub(super) fn open_file(root: &Path, requested: &Path) -> std::io::Result<File> {
        let root_handle = open_root(root)?;
        let root_final = normalized_final_path(final_path(&root_handle)?);
        let expected_root = normalized_final_path(root.to_string_lossy().into_owned());
        if root_final != expected_root {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "compile-time source directory changed during resolution",
            ));
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(requested)?;
        if !file.metadata()?.is_file()
            || is_reparse(requested)?
            || !is_single_link_file(&file)?
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "opened object is not a regular, unshared non-reparse file",
            ));
        }
        let file_final = normalized_final_path(final_path(&file)?);
        if !is_beneath(&root_final, &file_final) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "opened file escapes the compile-time source directory",
            ));
        }
        Ok(file)
    }

    fn open_root(path: &Path) -> std::io::Result<File> {
        if is_reparse(path)? {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "compile-time source directory is a reparse point",
            ));
        }
        let root = std::fs::OpenOptions::new()
            .read(true)
            .access_mode(GENERIC_READ)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        if is_reparse(path)? || !root.metadata()?.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "compile-time source directory is not a real directory",
            ));
        }
        Ok(root)
    }
}

fn comptime_http_stream(
    url: &str,
    timeout: Duration,
) -> Result<StreamResponse, FetchError> {
    let response = match COMPTIME_HTTP_AGENT
        .get(url)
        .timeout(timeout)
        .set("Accept-Encoding", "identity")
        .call()
    {
        Ok(response) => response,
        Err(ureq::Error::Status(_, response)) => response,
        Err(error) => return Err(FetchError::http(url, error.to_string())),
    };
    let status = response.status();
    let content_length = response
        .header("Content-Length")
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| FetchError::http(url, "invalid Content-Length".to_string()))
        })
        .transpose()?;
    Ok(StreamResponse {
        status,
        content_length,
        location: response.header("Location").map(str::to_string),
        reader: Box::new(response.into_reader()),
    })
}

fn restricted_resolver(netloc: &str) -> std::io::Result<Vec<SocketAddr>> {
    let addresses = netloc.to_socket_addrs()?.collect::<Vec<_>>();
    if !addresses_are_public(&addresses) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "compile-time network destination is not public",
        ));
    }
    Ok(addresses)
}

/// Resolve a host and reject any DNS answer that is not globally routable.
/// Callers that invoke an external client should pin the returned addresses
/// so the client cannot perform a second, different DNS lookup.
pub fn resolve_public_addresses(host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
    let host = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host);
    let endpoint = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    let addresses = endpoint
        .to_socket_addrs()
        .map_err(|error| format!("could not resolve provider destination: {error}"))?
        .collect::<Vec<_>>();
    if !addresses_are_public(&addresses) {
        return Err("provider destination resolves to a non-public address".to_string());
    }
    Ok(addresses)
}

fn addresses_are_public(addresses: &[SocketAddr]) -> bool {
    !addresses.is_empty() && addresses.iter().all(|address| is_public_ip(address.ip()))
}

/// Return whether an address is safe for a comptime outbound request.
/// Only globally routable unicast space is allowed. This check is applied to
/// every address returned by DNS, so a mixed public/private answer is denied.
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(a == 0
                || a == 10
                || a == 100 && (b & 0b1100_0000) == 0b0100_0000
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
                return is_public_ip(IpAddr::V4(ipv4));
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

fn read_limited(mut reader: impl Read, limit: usize) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take(limit.saturating_add(1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("fetch response exceeds {limit} bytes"),
        ));
    }
    Ok(bytes)
}

pub fn get_stream(url: &str, timeout: Duration) -> Result<StreamResponse, FetchError> {
    get_stream_with_timeout(url, timeout)
}

/// Perform one redirect-free request using an address set resolved by the
/// caller. The URL remains host-based so HTTPS keeps its normal SNI and
/// certificate-host verification; the resolver prevents ureq from doing a
/// second DNS lookup for the socket destination.
pub fn get_stream_pinned(
    url: &str,
    addresses: &[SocketAddr],
    timeout: Duration,
) -> Result<StreamResponse, FetchError> {
    if addresses.is_empty() {
        return Err(FetchError::IO(
            "pinned network transport has no resolved addresses".to_string(),
        ));
    }
    get_stream_with_agent(url, timeout, {
        let addresses = addresses.to_vec();
        ureq::AgentBuilder::new()
            .redirects(0)
            .resolver(move |_: &str| Ok(addresses.clone()))
            .build()
    })
}

fn get_stream_with_timeout(url: &str, timeout: Duration) -> Result<StreamResponse, FetchError> {
    get_stream_with_agent(url, timeout, (*HTTP_AGENT).clone())
}

fn get_stream_with_agent(
    url: &str,
    timeout: Duration,
    agent: ureq::Agent,
) -> Result<StreamResponse, FetchError> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        let scheme = url.find("://").map(|i| &url[..i]).unwrap_or(url);
        return Err(FetchError::Scheme(format!(
            "unsupported URL scheme `{scheme}`; expected `http://` or `https://`"
        )));
    }
    let response = match agent
        .get(url)
        .timeout(timeout)
        .set("Accept-Encoding", "identity")
        .call()
    {
        Ok(response) => response,
        Err(ureq::Error::Status(_, response)) => response,
        Err(error) => return Err(FetchError::http(url, error.to_string())),
    };
    let status = response.status();
    let content_length = response
        .header("Content-Length")
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| FetchError::http(url, "invalid Content-Length".to_string()))
        })
        .transpose()?;
    Ok(StreamResponse {
        status,
        content_length,
        location: response.header("Location").map(str::to_string),
        reader: Box::new(response.into_reader()),
    })
}

/// Fetch a bounded stream while following a small, explicit redirect chain.
/// The plain `get_stream` API remains redirect-free for callers that need to
/// inspect the original HTTP response.
pub fn get_stream_follow_redirects(
    url: &str,
    timeout: Duration,
    max_redirects: usize,
) -> Result<StreamResponse, FetchError> {
    get_stream_follow_redirects_with(url, timeout, max_redirects, |current, location| {
        resolve_redirect(current, location).map_err(|error| error.to_string())
    })
}

/// Follow redirects through a caller-supplied policy.
///
/// The policy receives the URL that produced the redirect and its raw
/// `Location` value. It must return the next URL or reject the hop. The
/// ordinary redirect helper above keeps its historical permissive behavior;
/// security-sensitive callers can enforce an origin or scheme invariant here.
pub fn get_stream_follow_redirects_with<F>(
    url: &str,
    timeout: Duration,
    max_redirects: usize,
    mut redirect: F,
) -> Result<StreamResponse, FetchError>
where
    F: FnMut(&str, &str) -> Result<String, String>,
{
    let mut current = url.to_string();
    for _ in 0..=max_redirects {
        let response = get_stream_with_timeout(&current, timeout)?;
        if !(300..400).contains(&response.status()) {
            return Ok(response);
        }
        let location = response.location().ok_or_else(|| {
            FetchError::http(
                &current,
                "redirect response has no Location header".to_string(),
            )
        })?;
        let next = redirect(&current, location)
            .map_err(|detail| FetchError::http(&current, detail))?;
        current = next;
    }
    Err(FetchError::http(
        url,
        format!("too many redirects (limit {max_redirects})"),
    ))
}

/// Fetch a bounded body while applying a caller-supplied redirect policy.
///
/// This keeps timeout, redirect-count, response-size, and framing checks in
/// the shared network seam while allowing a caller to bind redirects to its
/// own security origin.
pub fn fetch_bounded_with_redirect_policy<F>(
    url: &str,
    timeout: Duration,
    max_redirects: usize,
    limit: u64,
    redirect: F,
) -> Result<Vec<u8>, FetchError>
where
    F: FnMut(&str, &str) -> Result<String, String>,
{
    let response = get_stream_follow_redirects_with(url, timeout, max_redirects, redirect)?;
    if !(200..300).contains(&response.status()) {
        return Err(FetchError::http(
            url,
            format!("URL returned HTTP {}", response.status()),
        ));
    }
    let content_length = response.content_length();
    if content_length.is_some_and(|length| length > limit) {
        return Err(FetchError::http(url, "response exceeds its size bound".to_string()));
    }
    let mut body = Vec::new();
    response
        .take(limit.saturating_add(1))
        .read_to_end(&mut body)
        .map_err(|error| FetchError::http(url, format!("could not read response: {error}")))?;
    if body.len() as u64 > limit {
        return Err(FetchError::http(url, "response exceeds its size bound".to_string()));
    }
    if content_length.is_some_and(|length| length != body.len() as u64) {
        return Err(FetchError::http(
            url,
            "response Content-Length disagrees".to_string(),
        ));
    }
    Ok(body)
}

fn resolve_redirect(current: &str, location: &str) -> Result<String, FetchError> {
    if location.starts_with("https://") || location.starts_with("http://") {
        return Ok(location.to_string());
    }
    if location.starts_with('/') {
        let authority = current
            .split_once("://")
            .and_then(|(_, rest)| rest.split('/').next())
            .ok_or_else(|| FetchError::http(current, "redirect URL has no authority".into()))?;
        return Ok(format!(
            "{}://{}{}",
            current.split_once("://").unwrap().0,
            authority,
            location
        ));
    }
    Err(FetchError::http(
        current,
        "redirect Location must be an absolute or root-relative URL".into(),
    ))
}

fn classify_http_error(url: &str, detail: &str) -> FetchErrorKind {
    if !url.starts_with("https://") {
        return FetchErrorKind::General;
    }
    let lower = detail.to_ascii_lowercase();
    if lower.contains("no valid certificates loaded")
        || lower.contains("no root cert")
        || lower.contains("no roots")
        || lower.contains("empty root")
        || lower.contains("trust store")
    {
        FetchErrorKind::TLSTrustRoots
    } else if lower.contains("certificate")
        || lower.contains("unknownissuer")
        || lower.contains("notvalid")
        || lower.contains("expired")
        || lower.contains("hostname")
        || lower.contains("cert")
    {
        FetchErrorKind::TLSCertificate
    } else if lower.contains("tls")
        || lower.contains("rustls")
        || lower.contains("handshake")
        || lower.contains("alert")
    {
        FetchErrorKind::TLSHandshake
    } else {
        FetchErrorKind::General
    }
}

fn host_from_url(url: &str) -> String {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    rest.split('/').next().unwrap_or(rest).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn file_fetch_still_works_without_network() {
        let path = std::env::temp_dir().join(format!("jet-net-file-fetch-{}", std::process::id()));
        std::fs::write(&path, b"fixture").expect("write temp fixture");
        let url = format!("file://{}", path.display());

        let bytes = fetch_in_root(&url, path.parent().unwrap()).expect("file fetch");

        assert_eq!(bytes, b"fixture");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn comptime_file_fetch_rejects_paths_outside_source_root() {
        let root = std::env::temp_dir().join(format!("jet-net-fetch-root-{}", std::process::id()));
        let outside = std::env::temp_dir().join(format!("jet-net-fetch-outside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(&outside);
        std::fs::create_dir_all(&root).expect("create source root");
        std::fs::write(&outside, b"private fixture").expect("write outside fixture");

        let error = fetch_in_root(&format!("file://{}", outside.display()), &root)
            .expect_err("absolute path outside source root must fail");
        assert!(error.to_string().contains("outside the compile-time source directory"));

        #[cfg(unix)]
        {
            let link = root.join("outside-link");
            std::os::unix::fs::symlink(&outside, &link).expect("create escape symlink");
            let error = fetch_in_root("file://outside-link", &root)
                .expect_err("symlink outside source root must fail");
            assert!(error
                .to_string()
                .contains("outside the compile-time source directory"));
            let _ = std::fs::remove_file(link);
        }

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(outside);
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    #[test]
    fn comptime_file_fetch_is_race_safe_against_symlink_swap() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };

        let root = std::env::temp_dir().join(format!(
            "jet-net-fetch-race-root-{}",
            std::process::id()
        ));
        let outside = std::env::temp_dir().join(format!(
            "jet-net-fetch-race-outside-{}",
            std::process::id()
        ));
        let target = root.join("target");
        let safe_slot = root.join("safe-slot");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(&outside);
        std::fs::create_dir_all(&root).expect("create source root");
        std::fs::write(&target, b"inside").expect("write source fixture");
        std::fs::write(&outside, b"outside").expect("write outside fixture");

        let stop = Arc::new(AtomicBool::new(false));
        let mutator_stop = Arc::clone(&stop);
        let mutator_outside = outside.clone();
        let mutator = thread::spawn(move || {
            while !mutator_stop.load(Ordering::Relaxed) {
                std::fs::rename(&target, &safe_slot).expect("move source fixture aside");
                std::os::unix::fs::symlink(&mutator_outside, &target)
                    .expect("install escape symlink");
                std::fs::remove_file(&target).expect("remove escape symlink");
                std::fs::rename(&safe_slot, &target).expect("restore source fixture");
            }
        });

        for _ in 0..2048 {
            if let Ok(bytes) = fetch_in_root("file://target", &root) {
                assert_eq!(bytes, b"inside", "a file fetch followed the escape symlink");
            }
        }

        stop.store(true, Ordering::Relaxed);
        mutator.join().expect("source mutator joins");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(&outside);
    }

    #[test]
    fn comptime_http_fetch_rejects_loopback_before_connecting() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback fixture");
        let port = listener.local_addr().expect("loopback address").port();
        let error = fetch(&format!("http://127.0.0.1:{port}/secret"))
            .expect_err("loopback destination must fail");
        assert!(format!("{error:?}").contains("not public"), "unexpected error: {error:?}");

        listener
            .set_nonblocking(true)
            .expect("make listener nonblocking");
        assert!(matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock));
    }

    #[test]
    fn public_destination_policy_rejects_reserved_ipv4_space() {
        for address in ["192.0.0.1", "192.0.0.9", "192.0.0.255"] {
            let address = address.parse().expect("reserved IPv4 fixture");
            assert!(!is_public_ip(address), "reserved address was accepted: {address}");
        }
        assert!(is_public_ip("8.8.8.8".parse().expect("public IPv4 fixture")));
    }

    #[test]
    fn redirect_policy_hook_is_applied_to_each_hop() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind redirect fixture");
        let address = listener.local_addr().expect("redirect fixture address");
        let server = thread::spawn(move || {
            for (expected_path, location) in [
                ("/one", Some("/two")),
                ("/two", Some("/done")),
                ("/done", None),
            ] {
                let (mut stream, _) = listener.accept().expect("accept redirect request");
                let mut request = [0_u8; 1024];
                let length = stream.read(&mut request).expect("read redirect request");
                let request = String::from_utf8_lossy(&request[..length]);
                assert!(
                    request.starts_with(&format!("GET {expected_path} HTTP/1.1")),
                    "unexpected request: {request}"
                );
                let response = match location {
                    Some(location) => format!(
                        "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    ),
                    None => {
                        "HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ndone"
                            .to_string()
                    }
                };
                stream
                    .write_all(response.as_bytes())
                    .expect("write redirect response");
            }
        });

        let mut hops = Vec::new();
        let body = fetch_bounded_with_redirect_policy(
            &format!("http://{address}/one"),
            Duration::from_secs(1),
            5,
            16,
            |current, location| {
                hops.push((current.to_string(), location.to_string()));
                resolve_redirect(current, location).map_err(|error| error.to_string())
            },
        )
        .expect("redirect chain fetch");
        assert_eq!(body, b"done");
        assert_eq!(hops.len(), 2);
        assert_eq!(hops[0].1, "/two");
        assert_eq!(hops[1].1, "/done");
        server.join().expect("redirect fixture joins");
    }

    #[test]
    fn unsupported_scheme_names_allowed_schemes() {
        let err = fetch("ftp://example.invalid/data").expect_err("ftp is rejected");

        assert!(err
            .to_string()
            .contains("expected `file://`, `http://`, or `https://`"));
    }

    #[test]
    fn fetch_reader_rejects_an_endless_response_at_the_boundary() {
        let error = read_limited(std::io::repeat(0), 8).expect_err("reader must be bounded");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    #[cfg(feature = "tls")]
    fn https_default_build_has_tls_backend() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind local fixture");
        let addr = listener.local_addr().expect("local addr");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept one client");
            let mut buf = [0_u8; 8];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(b"not tls");
        });

        let result = get_stream_with_timeout(
            &format!("https://localhost:{}", addr.port()),
            Duration::from_millis(500),
        );
        assert!(result.is_err(), "fixture is not a TLS server");
        server.join().expect("fixture server joins");

        let err = result.err().unwrap();
        assert_eq!(err.diagnostic_code(), "E4201", "{err:?}");
        assert_eq!(err.to_string(), "TLS handshake failed");
    }

    #[test]
    fn tls_error_classifier_names_certificate_failures() {
        let err = FetchError::http(
            "https://api.example.test/data",
            "Connection Failed: invalid peer certificate: UnknownIssuer".to_string(),
        );

        assert_eq!(err.diagnostic_code(), "E4202");
        assert!(err
            .diagnostic_what("https://api.example.test/data")
            .contains("api.example.test"));
    }

    #[test]
    fn tls_error_classifier_names_missing_trust_roots() {
        let err = FetchError::http(
            "https://api.example.test/data",
            "no valid certificates loaded by rustls-native-certs".to_string(),
        );

        assert_eq!(err.diagnostic_code(), "E4203");
        assert!(err.diagnostic_fix().contains("ca-certificates"));
    }

    #[test]
    fn tls_fixture_pems_are_checked_in_for_future_handshake_tests() {
        let cert = include_str!("../../../tests/fixtures/tls/localhost.cert.pem");
        let key = include_str!("../../../tests/fixtures/tls/localhost.key.pem");

        assert!(cert.contains("BEGIN CERTIFICATE"));
        assert!(cert.contains("DNS:localhost") || cert.contains("MIID"));
        assert!(key.contains("BEGIN PRIVATE KEY"));
    }
}
