//! Dependency-free dev-server transport and watch policy.
#![allow(non_snake_case)]
#![deny(warnings)]

use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::{self, BufRead, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::thread;
use std::time::Duration;

pub mod BrowserTrace;
pub mod Canvas;
pub mod LiveInspect;
pub mod Session;
pub mod WatchService;
pub mod WebHost;

pub use Session::ResidentDevSession;

pub use WatchService::{
    any_stamp_changed, within_budget, ChangeKind, HotReplaceTxn, InvalidationReceipt, PersistEntry,
    PersistOutcome, PersistStore, RootKind, SessionSnapshot, WatchGraph, WatchSession,
    EDIT_TO_VISIBLE_BUDGET_MS,
};

pub const MAX_REQUEST_BODY_BYTES: usize = 1024 * 1024;
pub const MAX_REQUEST_LINE_BYTES: usize = 8 * 1024;
pub const MAX_REQUEST_HEADER_BYTES: usize = 32 * 1024;
pub const MAX_REQUEST_HEADER_COUNT: usize = 100;
pub(crate) const MAX_RESPONSE_BODY_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MAX_CHILD_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

static STATIC_PUBLICATION_LOCK: OnceLock<RwLock<()>> = OnceLock::new();

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WatchPolicy {
    Auto,
    Restart,
    Swap,
    Once,
}

pub fn watch_policy_from(raw: &[String], default: WatchPolicy) -> WatchPolicy {
    raw.iter().fold(default, |policy, arg| match arg.as_str() {
        "--restart" => WatchPolicy::Restart,
        "--swap" => WatchPolicy::Swap,
        "--watch=off" => WatchPolicy::Once,
        _ => policy,
    })
}

pub fn file_mtime(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

#[derive(Debug)]
pub struct Request {
    pub method: String,
    pub target: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn read(reader: &mut impl BufRead) -> std::io::Result<Option<Self>> {
        let mut line = String::new();
        if read_bounded_line(reader, &mut line, MAX_REQUEST_LINE_BYTES)? == 0 {
            return Ok(None);
        }
        let mut parts = line.split_whitespace();
        let method = parts.next().unwrap_or("");
        let target = parts.next().unwrap_or("");
        let version = parts.next().unwrap_or("");
        if method.is_empty()
            || target.is_empty()
            || version != "HTTP/1.1"
            || parts.next().is_some()
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "malformed devserver request line",
            ));
        }
        let mut content_length = None;
        let mut headers = HashMap::new();
        let mut header_bytes = 0usize;
        let mut header_count = 0usize;
        loop {
            let mut header = String::new();
            let read = read_bounded_line(reader, &mut header, MAX_REQUEST_LINE_BYTES)?;
            if read == 0 || header == "\r\n" || header == "\n" {
                break;
            }
            header_count = header_count.checked_add(1).ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "too many devserver headers")
            })?;
            header_bytes = header_bytes.checked_add(read).ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "devserver headers too large")
            })?;
            if header_count > MAX_REQUEST_HEADER_COUNT || header_bytes > MAX_REQUEST_HEADER_BYTES {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "devserver headers exceed the request budget",
                ));
            }
            let Some((name, value)) = header.trim_end_matches(['\r', '\n']).split_once(':') else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "malformed devserver header",
                ));
            };
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if name.is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "malformed devserver header name",
                ));
            }
            if matches!(
                name.as_str(),
                "authorization" | "content-length" | "host" | "origin" | "transfer-encoding"
            ) && headers.contains_key(&name)
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "duplicate devserver security header",
                ));
            }
            if name == "transfer-encoding" {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "devserver does not accept transfer-encoded requests",
                ));
            }
            if name == "content-length" {
                let length = value.parse::<usize>().map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "invalid devserver content length",
                    )
                })?;
                content_length = Some(length);
            }
            headers.insert(name, value);
        }
        let content_length = content_length.unwrap_or(0);
        if content_length > MAX_REQUEST_BODY_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "devserver request body exceeds 1 MiB",
            ));
        }
        let mut body = vec![0; content_length];
        reader.read_exact(&mut body)?;
        Ok(Some(Self {
            method: method.to_string(),
            target: target.to_string(),
            headers,
            body,
        }))
    }
}

fn read_bounded_line(
    reader: &mut impl BufRead,
    line: &mut String,
    limit: usize,
) -> std::io::Result<usize> {
    let read = reader
        .take(limit.saturating_add(1) as u64)
        .read_line(line)?;
    if read > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "devserver request line exceeds 8 KiB",
        ));
    }
    Ok(read)
}

pub fn write_response(
    mut out: impl Write,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    if body.len() > MAX_RESPONSE_BODY_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "devserver response exceeds the response budget",
        ));
    }
    write!(out, "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", body.len())?;
    out.write_all(body)?;
    out.flush()
}

pub fn query_param(target: &str, key: &str) -> Option<String> {
    let (_, query) = target.split_once('?')?;
    query.split('&').find_map(|part| {
        let (name, value) = part.split_once('=').unwrap_or((part, ""));
        (name == key).then(|| percent_decode(value))
    })
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(a), Some(b)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(a * 16 + b);
                i += 3;
                continue;
            }
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn static_relative_path(path: &str) -> Result<PathBuf, ()> {
    let relative = if path == "/" {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };
    if path.contains("..")
        || relative.starts_with('/')
        || relative.starts_with('\\')
        || relative.contains('\\')
        || has_windows_drive_prefix(relative)
        || Path::new(relative)
            .components()
            .any(|component| matches!(component, Component::Prefix(_) | Component::RootDir))
    {
        return Err(());
    }
    let relative = Path::new(relative);
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || !windows_components_are_safe(relative)
    {
        return Err(());
    }
    Ok(relative.to_path_buf())
}

fn has_windows_drive_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn windows_component_is_safe(name: &OsStr) -> bool {
    let text = name.to_string_lossy();
    if text.is_empty()
        || text
            .chars()
            .last()
            .is_some_and(|character| character == ' ' || character == '.')
        || text
            .chars()
            .any(|character| {
                character.is_control()
                    || matches!(character, ':' | '\\' | '<' | '>' | '"' | '|' | '?' | '*')
            })
    {
        return false;
    }
    let stem = text.split('.').next().unwrap_or_default();
    let upper = stem.to_ascii_uppercase();
    !matches!(
        upper.as_str(),
            "CON"
            | "CONIN$"
            | "CONOUT$"
            | "PRN"
            | "AUX"
            | "NUL"
            | "CLOCK$"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
            | "COM¹"
            | "COM²"
            | "COM³"
            | "LPT¹"
            | "LPT²"
            | "LPT³"
    )
}

fn windows_components_are_safe(relative: &Path) -> bool {
    relative.components().all(|component| {
        matches!(component, Component::Normal(name) if windows_component_is_safe(name))
    })
}

pub(crate) fn lock_static_publication() -> io::Result<std::sync::RwLockWriteGuard<'static, ()>> {
    STATIC_PUBLICATION_LOCK
        .get_or_init(|| RwLock::new(()))
        .write()
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "devserver publication lock poisoned"))
}

pub(crate) fn lock_static_read() -> io::Result<std::sync::RwLockReadGuard<'static, ()>> {
    STATIC_PUBLICATION_LOCK
        .get_or_init(|| RwLock::new(()))
        .read()
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "devserver publication lock poisoned"))
}

/// Run one child with one shared stdout/stderr budget. The child is stopped as
/// soon as either stream crosses the budget, before a receipt can materialize
/// the output. Readers are separate so a noisy child cannot deadlock on one
/// full pipe while the other is being drained.
pub(crate) fn command_output_bounded(
    command: &mut Command,
    limit: usize,
) -> io::Result<Output> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().ok_or_else(|| {
        io::Error::new(io::ErrorKind::Other, "child stdout pipe was not available")
    });
    let stderr = child.stderr.take().ok_or_else(|| {
        io::Error::new(io::ErrorKind::Other, "child stderr pipe was not available")
    });
    let (stdout, stderr) = match (stdout, stderr) {
        (Ok(stdout), Ok(stderr)) => (stdout, stderr),
        (stdout, stderr) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(stdout
                .err()
                .or_else(|| stderr.err())
                .expect("missing child pipe error"));
        }
    };
    let budget = Arc::new(AtomicUsize::new(0));
    let exceeded = Arc::new(AtomicBool::new(false));
    let read_failed = Arc::new(AtomicBool::new(false));
    let stdout_join = spawn_bounded_child_reader(
        stdout,
        limit,
        Arc::clone(&budget),
        Arc::clone(&exceeded),
        Arc::clone(&read_failed),
    );
    let stderr_join = spawn_bounded_child_reader(
        stderr,
        limit,
        budget,
        Arc::clone(&exceeded),
        Arc::clone(&read_failed),
    );

    let status = loop {
        if exceeded.load(Ordering::Acquire) || read_failed.load(Ordering::Acquire) {
            let _ = child.kill();
            break child.wait()?;
        }
        match child.try_wait()? {
            Some(status) => break status,
            None => thread::sleep(Duration::from_millis(1)),
        }
    };
    let stdout = join_bounded_child_reader(stdout_join)?;
    let stderr = join_bounded_child_reader(stderr_join)?;
    if exceeded.load(Ordering::Acquire) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "child output exceeds the aggregate output budget",
        ));
    }
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn spawn_bounded_child_reader<R>(
    mut reader: R,
    limit: usize,
    budget: Arc<AtomicUsize>,
    exceeded: Arc<AtomicBool>,
    read_failed: Arc<AtomicBool>,
) -> thread::JoinHandle<io::Result<Vec<u8>>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let count = match reader.read(&mut chunk) {
                Ok(count) => count,
                Err(error) => {
                    read_failed.store(true, Ordering::Release);
                    return Err(error);
                }
            };
            if count == 0 {
                return Ok(bytes);
            }
            let mut used = budget.load(Ordering::Acquire);
            let kept = loop {
                let available = limit.saturating_sub(used);
                let kept = available.min(count);
                let next = used.saturating_add(kept);
                match budget.compare_exchange(
                    used,
                    next,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => break kept,
                    Err(next) => used = next,
                }
            };
            if kept < count {
                exceeded.store(true, Ordering::Release);
                return Ok(bytes);
            }
            bytes.extend_from_slice(&chunk[..count]);
        }
    })
}

fn join_bounded_child_reader(
    reader: thread::JoinHandle<io::Result<Vec<u8>>>,
) -> io::Result<Vec<u8>> {
    reader
        .join()
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "child output reader panicked"))?
}

/// Open one static response below a held root directory. The authority owns
/// every directory needed for the open and reads only the already-opened
/// regular file; callers never re-resolve a validated path for I/O.
pub(crate) fn read_static_file_bounded(
    root: &Path,
    relative: &Path,
    max_bytes: u64,
) -> io::Result<Vec<u8>> {
    let _publication = lock_static_read()?;
    static_authority::read(root, relative, max_bytes)
}

#[cfg(windows)]
pub(crate) fn read_file_without_symlinks_bounded(
    path: &Path,
    max_bytes: u64,
) -> io::Result<Vec<u8>> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "file path has no regular file name",
        )
    })?;
    read_static_file_bounded(parent, Path::new(name), max_bytes)
}

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios"
))]
mod static_authority {
    use super::{io, Component, Path, PathBuf};
    use std::ffi::{c_char, CString, OsStr};
    use std::fs::{File, OpenOptions};
    use std::io::Read;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    const O_RDONLY: i32 = 0;
    const O_CLOEXEC: i32 = if cfg!(any(target_os = "linux", target_os = "android")) {
        0o2000000
    } else {
        0x01000000
    };
    const O_DIRECTORY: i32 = if cfg!(any(target_os = "linux", target_os = "android")) {
        0o200000
    } else {
        0x00100000
    };
    const O_NOFOLLOW: i32 = if cfg!(any(target_os = "linux", target_os = "android")) {
        0o400000
    } else {
        0x0100
    };
    const O_NONBLOCK: i32 = if cfg!(any(target_os = "linux", target_os = "android")) {
        0o4000
    } else {
        0x0004
    };

    unsafe extern "C" {
        fn openat(directory: i32, path: *const c_char, flags: i32, ...) -> i32;
    }

    #[derive(Clone, Copy)]
    struct Identity {
        device: u64,
        inode: u64,
        links: u64,
        length: u64,
    }

    pub(super) struct Authority {
        directories: Vec<File>,
        directory_paths: Vec<PathBuf>,
        file: File,
        file_path: PathBuf,
        identity: Identity,
    }

    pub(super) fn read(root: &Path, relative: &Path, max_bytes: u64) -> io::Result<Vec<u8>> {
        open(root, relative)?.read_bounded(max_bytes)
    }

    #[cfg(test)]
    pub(super) fn open_for_test(root: &Path, relative: &Path) -> io::Result<Authority> {
        open(root, relative)
    }

    #[cfg(test)]
    impl Authority {
        pub(super) fn read_for_test(self, max_bytes: u64) -> io::Result<Vec<u8>> {
            self.read_bounded(max_bytes)
        }
    }

    fn open(root: &Path, relative: &Path) -> io::Result<Authority> {
        let components = normal_components(relative)?;
        let expected_root = std::fs::metadata(root)?;
        if !expected_root.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "static root is not a directory",
            ));
        }
        let root_file = open_directory_path(root)?;
        let opened_root = root_file.metadata()?;
        if !same_directory(&expected_root, &opened_root) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "static root changed during secure open",
            ));
        }
        let mut directories = vec![root_file];
        let mut directory_paths = vec![root.to_path_buf()];
        let mut current = root.to_path_buf();
        for component in components[..components.len() - 1].iter().copied() {
            let name = c_name(component)?;
            let child = open_at(
                directories.last().expect("root authority exists"),
                &name,
                O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC,
            )?;
            if !child.metadata()?.is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "static path component is not a directory",
                ));
            }
            directories.push(child);
            current.push(component);
            directory_paths.push(current.clone());
        }
        let final_component = components.last().copied().expect("non-empty path");
        let name = c_name(final_component)?;
        let file = open_at(
            directories.last().expect("root authority exists"),
            &name,
            O_RDONLY | O_NONBLOCK | O_NOFOLLOW | O_CLOEXEC,
        )?;
        let metadata = file.metadata()?;
        let identity = identity(&metadata);
        require_regular(identity, &metadata)?;
        current.push(final_component);
        Ok(Authority {
            directories,
            directory_paths,
            file,
            file_path: current,
            identity,
        })
    }

    impl Authority {
        fn read_bounded(mut self, max_bytes: u64) -> io::Result<Vec<u8>> {
            self.ensure_paths_current()?;
            let max_bytes = usize::try_from(max_bytes).unwrap_or(usize::MAX);
            if self.identity.length > max_bytes as u64 {
                return Err(response_limit());
            }
            let capacity = usize::try_from(self.identity.length).map_err(|_| response_limit())?;
            let mut bytes = Vec::with_capacity(capacity);
            let mut chunk = [0u8; 8192];
            loop {
                let read = self.file.read(&mut chunk)?;
                if read == 0 {
                    break;
                }
                let next = bytes.len().checked_add(read).ok_or_else(response_limit)?;
                if next > max_bytes || next > capacity {
                    return Err(response_limit());
                }
                bytes.extend_from_slice(&chunk[..read]);
            }
            let final_metadata = self.file.metadata()?;
            let final_identity = identity(&final_metadata);
            if self.ensure_paths_current().is_err()
                || !same_identity(self.identity, final_identity)
                || final_identity.length != self.identity.length
                || final_identity.length != bytes.len() as u64
                || final_identity.links != 1
            {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "static file changed while it was being read",
                ));
            }
            Ok(bytes)
        }

        fn ensure_paths_current(&self) -> io::Result<()> {
            for (directory, path) in self.directories.iter().zip(&self.directory_paths) {
                let path_metadata = std::fs::symlink_metadata(path)?;
                if !same_directory(&path_metadata, &directory.metadata()?) {
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        "static directory moved while it was held",
                    ));
                }
            }
            let path_metadata = std::fs::symlink_metadata(&self.file_path)?;
            let path_identity = identity(&path_metadata);
            if !path_metadata.is_file()
                || path_identity.links != 1
                || !same_identity(self.identity, path_identity)
            {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "static file moved while it was held",
                ));
            }
            Ok(())
        }
    }

    fn normal_components(relative: &Path) -> io::Result<Vec<&OsStr>> {
        let components = relative
            .components()
            .map(|component| match component {
                Component::Normal(name) if super::windows_component_is_safe(name) => Ok(name),
                _ => Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "static path must be relative and normalized",
                )),
            })
            .collect::<io::Result<Vec<_>>>()?;
        if components.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "static path is empty",
            ));
        }
        Ok(components)
    }

    fn c_name(name: &OsStr) -> io::Result<CString> {
        CString::new(name.as_bytes()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "static path contains NUL")
        })
    }

    fn open_directory_path(path: &Path) -> io::Result<File> {
        OpenOptions::new()
            .read(true)
            .custom_flags(O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
            .open(path)
    }

    fn open_at(directory: &File, name: &CString, flags: i32) -> io::Result<File> {
        let fd = unsafe { openat(directory.as_raw_fd(), name.as_ptr(), flags, 0) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    fn identity(metadata: &std::fs::Metadata) -> Identity {
        Identity {
            device: metadata.dev(),
            inode: metadata.ino(),
            links: metadata.nlink(),
            length: metadata.len(),
        }
    }

    fn same_directory(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
        left.is_dir()
            && right.is_dir()
            && left.dev() == right.dev()
            && left.ino() == right.ino()
    }

    fn same_identity(left: Identity, right: Identity) -> bool {
        left.device == right.device && left.inode == right.inode
    }

    fn require_regular(identity: Identity, metadata: &std::fs::Metadata) -> io::Result<()> {
        if metadata.is_file() && identity.links == 1 {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "static response is not a singly-linked regular file",
            ))
        }
    }

    fn response_limit() -> io::Error {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "static response exceeds the response limit",
        )
    }
}

#[cfg(windows)]
mod static_authority {
    use super::{io, Component, Path};
    use std::ffi::c_void;
    use std::fs::{File, OpenOptions};
    use std::io::Read;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;

    type Handle = *mut c_void;
    const GENERIC_READ: u32 = 0x8000_0000;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const FILE_SHARE_DELETE: u32 = 0x0000_0004;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;

    #[allow(dead_code)]
    #[repr(C)]
    struct FileTime {
        low: u32,
        high: u32,
    }

    #[allow(dead_code)]
    #[repr(C)]
    struct ByHandleFileInformation {
        attributes: u32,
        creation: FileTime,
        access: FileTime,
        write: FileTime,
        volume_serial: u32,
        size_high: u32,
        size_low: u32,
        links: u32,
        index_high: u32,
        index_low: u32,
    }

    unsafe extern "system" {
        fn GetFileInformationByHandle(
            file: Handle,
            information: *mut ByHandleFileInformation,
        ) -> i32;
        fn GetFinalPathNameByHandleW(
            file: Handle,
            path: *mut u16,
            path_len: u32,
            flags: u32,
        ) -> u32;
    }

    #[derive(Clone)]
    struct Identity {
        attributes: u32,
        volume: u32,
        index_high: u32,
        index_low: u32,
        links: u32,
        length: u64,
    }

    pub(super) struct Authority {
        directories: Vec<File>,
        directory_paths: Vec<String>,
        file: File,
        file_path: String,
        identity: Identity,
    }

    pub(super) fn read(root: &Path, relative: &Path, max_bytes: u64) -> io::Result<Vec<u8>> {
        open(root, relative)?.read_bounded(max_bytes)
    }

    #[cfg(test)]
    pub(super) fn open_for_test(root: &Path, relative: &Path) -> io::Result<Authority> {
        open(root, relative)
    }

    #[cfg(test)]
    impl Authority {
        pub(super) fn read_for_test(self, max_bytes: u64) -> io::Result<Vec<u8>> {
            self.read_bounded(max_bytes)
        }
    }

    fn open(root: &Path, relative: &Path) -> io::Result<Authority> {
        let components = normal_components(relative)?;
        let root_file = open_directory(root)?;
        let root_final = normalize(final_path(&root_file)?)?;
        let expected_root = normalize(std::fs::canonicalize(root)?.to_string_lossy().into_owned())?;
        if root_final != expected_root {
            return Err(permission("static root changed during secure open"));
        }
        let mut directories = vec![root_file];
        let mut directory_paths = vec![expected_root.clone()];
        let mut expected_parent = expected_root;
        let mut current = root.to_path_buf();
        for component in components[..components.len() - 1].iter().copied() {
            current.push(component);
            let child = open_directory(&current)?;
            let actual = normalize(final_path(&child)?)?;
            let expected = normalize(
                Path::new(&expected_parent)
                    .join(Path::new(component))
                    .to_string_lossy()
                    .into_owned(),
            )?;
            if actual != expected {
                return Err(permission("static ancestor changed during secure open"));
            }
            expected_parent = actual;
            directories.push(child);
            directory_paths.push(expected_parent.clone());
        }
        let final_component = components.last().copied().expect("non-empty path");
        current.push(final_component);
        let file = open_file(&current)?;
        let actual = normalize(final_path(&file)?)?;
        let expected = normalize(
            Path::new(&expected_parent)
                .join(Path::new(final_component))
                .to_string_lossy()
                .into_owned(),
        )?;
        if actual != expected {
            return Err(permission("static file changed during secure open"));
        }
        let identity = file_identity(&file)?;
        if identity.links != 1 || !file.metadata()?.is_file() {
            return Err(permission("static response is not a singly-linked regular file"));
        }
        Ok(Authority {
            directories,
            directory_paths,
            file,
            file_path: actual,
            identity,
        })
    }

    impl Authority {
        fn read_bounded(mut self, max_bytes: u64) -> io::Result<Vec<u8>> {
            self.ensure_paths_current()?;
            if self.identity.length > max_bytes {
                return Err(response_limit());
            }
            let capacity = usize::try_from(self.identity.length).map_err(|_| response_limit())?;
            let mut bytes = Vec::with_capacity(capacity);
            let mut chunk = [0u8; 8192];
            loop {
                let read = self.file.read(&mut chunk)?;
                if read == 0 {
                    break;
                }
                let next = bytes.len().checked_add(read).ok_or_else(response_limit)?;
                if next > usize::try_from(max_bytes).unwrap_or(usize::MAX) || next > capacity {
                    return Err(response_limit());
                }
                bytes.extend_from_slice(&chunk[..read]);
            }
            let final_identity = file_identity(&self.file)?;
            if self.ensure_paths_current().is_err()
                || !same_identity(&self.identity, &final_identity)
                || final_identity.length != self.identity.length
                || final_identity.length != bytes.len() as u64
                || final_identity.links != 1
            {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "static file changed while it was being read",
                ));
            }
            Ok(bytes)
        }

        fn ensure_paths_current(&self) -> io::Result<()> {
            for (directory, path) in self.directories.iter().zip(&self.directory_paths) {
                if normalize(final_path(directory)?)? != *path {
                    return Err(permission("static directory moved while it was held"));
                }
            }
            let identity = file_identity(&self.file)?;
            if normalize(final_path(&self.file)?)? != self.file_path
                || identity.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
                || identity.links != 1
            {
                return Err(permission("static file moved while it was held"));
            }
            Ok(())
        }
    }

    fn normal_components(relative: &Path) -> io::Result<Vec<&std::ffi::OsStr>> {
        let components = relative
            .components()
            .map(|component| match component {
                Component::Normal(name) if super::windows_component_is_safe(name) => Ok(name),
                _ => Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "static path must be relative and normalized",
                )),
            })
            .collect::<io::Result<Vec<_>>>()?;
        if components.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "static path is empty",
            ));
        }
        Ok(components)
    }

    fn open_directory(path: &Path) -> io::Result<File> {
        let file = OpenOptions::new()
            .read(true)
            .access_mode(GENERIC_READ)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        let identity = file_identity(&file)?;
        if identity.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 || !file.metadata()?.is_dir() {
            return Err(permission("static path contains a reparse point or is not a directory"));
        }
        Ok(file)
    }

    fn open_file(path: &Path) -> io::Result<File> {
        let file = OpenOptions::new()
            .read(true)
            .access_mode(GENERIC_READ)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        let identity = file_identity(&file)?;
        if identity.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 || !file.metadata()?.is_file() {
            return Err(permission("static response is not a regular file"));
        }
        Ok(file)
    }

    fn file_identity(file: &File) -> io::Result<Identity> {
        let mut information = unsafe { std::mem::zeroed::<ByHandleFileInformation>() };
        if unsafe {
            GetFileInformationByHandle(file.as_raw_handle(), &mut information)
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Identity {
            attributes: information.attributes,
            volume: information.volume_serial,
            index_high: information.index_high,
            index_low: information.index_low,
            links: information.links,
            length: (u64::from(information.size_high) << 32) | u64::from(information.size_low),
        })
    }

    fn final_path(file: &File) -> io::Result<String> {
        let needed = unsafe {
            GetFinalPathNameByHandleW(file.as_raw_handle(), std::ptr::null_mut(), 0, 0)
        };
        if needed == 0 {
            return Err(io::Error::last_os_error());
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
            return Err(io::Error::last_os_error());
        }
        buffer.truncate(written as usize);
        Ok(String::from_utf16_lossy(&buffer))
    }

    fn normalize(path: String) -> io::Result<String> {
        let path = path.replace('/', "\\");
        let path = if let Some(path) = path.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{path}")
        } else {
            path.strip_prefix(r"\\?\").unwrap_or(&path).to_string()
        };
        let path = path.trim_end_matches(['\\', '/']);
        if path.len() == 2 && path.as_bytes()[1] == b':' {
            Ok(format!(r"{path}\").to_ascii_lowercase())
        } else {
            Ok(path.to_ascii_lowercase())
        }
    }

    fn same_identity(left: &Identity, right: &Identity) -> bool {
        left.volume == right.volume
            && left.index_high == right.index_high
            && left.index_low == right.index_low
    }

    fn permission(message: &str) -> io::Error {
        io::Error::new(io::ErrorKind::PermissionDenied, message)
    }

    fn response_limit() -> io::Error {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "static response exceeds the response limit",
        )
    }
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    windows
)))]
mod static_authority {
    use super::{io, Path};

    pub(super) fn read(_: &Path, _: &Path, _: u64) -> io::Result<Vec<u8>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "secure static file access is unavailable on this platform",
        ))
    }
}

pub fn content_type_for(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match extension.as_deref() {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "application/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("json" | "map") => "application/json; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

pub struct CanvasAsset {
    pub status: &'static str,
    pub content_type: &'static str,
    pub body: String,
}

pub fn canvas_asset(method: &str, target: &str, path: &str) -> Option<CanvasAsset> {
    let body = if target == "/?jet_panel=1" {
        jet_canvas::canvas_html_query()
    } else if target == "/?jet_panel_app=1" {
        jet_canvas::canvas_js()
    } else if matches!(
        path,
        "/__jet_canvas" | "/__jet_canvas/" | "/canvas" | "/canvas/" | "/panel" | "/panel/"
    ) {
        jet_canvas::canvas_html_for(if path.starts_with("/panel") {
            "/panel"
        } else {
            "/canvas"
        })
    } else if matches!(
        path,
        "/__jet_canvas/app.js" | "/canvas/app.js" | "/panel/app.js"
    ) {
        jet_canvas::canvas_js()
    } else {
        return None;
    };
    if method != "GET" {
        return Some(CanvasAsset {
            status: "405 Method Not Allowed",
            content_type: "text/plain; charset=utf-8",
            body: "method not allowed".into(),
        });
    }
    let content_type = if path.ends_with("app.js") || target == "/?jet_panel_app=1" {
        "application/javascript; charset=utf-8"
    } else {
        "text/html; charset=utf-8"
    };
    Some(CanvasAsset {
        status: "200 OK",
        content_type,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn request_and_query_policy() {
        let mut raw = &b"POST /x?q=a%20b HTTP/1.1\r\nContent-Length: 2\r\n\r\nok"[..];
        let r = Request::read(&mut raw).unwrap().unwrap();
        assert_eq!(r.body, b"ok");
        assert_eq!(r.headers.get("content-length").map(String::as_str), Some("2"));
        assert_eq!(query_param(&r.target, "q").as_deref(), Some("a b"));
    }
    #[test]
    fn malformed_request_metadata_fails_closed() {
        let mut invalid_length = &b"POST /x HTTP/1.1\r\nContent-Length: nope\r\n\r\n"[..];
        assert!(Request::read(&mut invalid_length).is_err());

        let mut malformed_line = &b"GET /x\r\nHost: localhost\r\n\r\n"[..];
        assert!(Request::read(&mut malformed_line).is_err());

        let raw = format!(
            "POST /x HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_REQUEST_BODY_BYTES + 1
        );
        let mut oversized = raw.as_bytes();
        assert!(Request::read(&mut oversized).is_err());
    }
    #[test]
    fn request_lines_and_headers_are_bounded_before_allocation() {
        let raw = format!(
            "GET /{} HTTP/1.1\r\n\r\n",
            "x".repeat(MAX_REQUEST_LINE_BYTES)
        );
        assert!(Request::read(&mut raw.as_bytes()).is_err());

        let raw = format!(
            "GET / HTTP/1.1\r\nX-Hostile: {}\r\n\r\n",
            "x".repeat(MAX_REQUEST_LINE_BYTES)
        );
        assert!(Request::read(&mut raw.as_bytes()).is_err());
    }
    #[test]
    fn traversal_is_rejected() {
        assert!(static_relative_path("/../x").is_err());
    }
    #[test]
    fn windows_absolute_static_paths_are_rejected() {
        for path in [
            "/C:/Windows/win.ini",
            "/C:\\Windows\\win.ini",
            "/\\\\server\\share\\secret",
            "/\\Windows\\win.ini",
            "/CON.txt",
            "/CONIN$.txt",
            "/CONOUT$",
            "/nested/PRN.log",
            "/page:secret",
            "/nested/foo\\bar",
        ] {
            assert!(
                static_relative_path(path).is_err(),
                "absolute Windows path escaped static root: {path}"
            );
        }
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    #[test]
    fn static_authority_rejects_symlinks_hardlinks_and_specials() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "jet-devserver-static-hostile-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let outside = root.with_file_name(format!(
            "jet-devserver-static-outside-{}",
            std::process::id()
        ));
        std::fs::write(&outside, "must not be served").unwrap();
        symlink(&outside, root.join("escape.js")).unwrap();
        assert!(
            read_static_file_bounded(&root, Path::new("escape.js"), 1024).is_err(),
            "static serving must not follow a symlink"
        );

        let hardlink_target = root.join("hardlink.js");
        std::fs::hard_link(&outside, &hardlink_target).unwrap();
        assert!(
            read_static_file_bounded(&root, Path::new("hardlink.js"), 1024).is_err(),
            "static serving must not expose a hard-linked file"
        );

        std::fs::create_dir(root.join("directory.js")).unwrap();
        assert!(
            read_static_file_bounded(&root, Path::new("directory.js"), 1024).is_err(),
            "static serving must not expose a directory as a response"
        );
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "must not be served");

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
    fn static_authority_rejects_final_ancestor_and_root_relocation() {
        const LIMIT: u64 = 64 * 1024 * 1024;
        let base = std::env::temp_dir().join(format!(
            "jet-devserver-static-relocation-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let root = base.join("root");
        let nested = root.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("page.html"), "safe").unwrap();

        let authority = static_authority::open_for_test(&root, Path::new("nested/page.html"))
            .unwrap();
        std::fs::rename(nested.join("page.html"), nested.join("page-held.html")).unwrap();
        assert!(
            authority.read_for_test(LIMIT).is_err(),
            "final-file relocation must fail closed"
        );

        std::fs::write(nested.join("page.html"), "safe").unwrap();
        let authority = static_authority::open_for_test(&root, Path::new("nested/page.html"))
            .unwrap();
        let nested_held = root.join("nested-held");
        std::fs::rename(&nested, &nested_held).unwrap();
        std::fs::create_dir(&nested).unwrap();
        std::fs::write(nested.join("page.html"), "attacker").unwrap();
        assert!(
            authority.read_for_test(LIMIT).is_err(),
            "ancestor relocation must fail closed"
        );

        let replacement_root = base.join("root-replacement");
        std::fs::create_dir_all(replacement_root.join("nested")).unwrap();
        std::fs::write(replacement_root.join("nested/page.html"), "attacker").unwrap();
        let authority = static_authority::open_for_test(&nested_held, Path::new("page.html"))
            .unwrap();
        let root_held = base.join("root-held");
        std::fs::rename(&root, &root_held).unwrap();
        std::fs::rename(replacement_root, &root).unwrap();
        assert!(
            authority.read_for_test(LIMIT).is_err(),
            "root relocation must fail closed"
        );

        let oversized = root_held.join("oversized.js");
        let file = std::fs::File::create(&oversized).unwrap();
        file.set_len(LIMIT + 1).unwrap();
        drop(file);
        assert!(
            read_static_file_bounded(&root_held, Path::new("oversized.js"), LIMIT).is_err(),
            "oversized static responses must fail before allocation"
        );

        let _ = std::fs::remove_dir_all(base);
    }

    #[cfg(windows)]
    #[test]
    fn windows_static_authority_rejects_hardlinks_specials_and_relocation() {
        const LIMIT: u64 = 64 * 1024 * 1024;
        let base = std::env::temp_dir().join(format!(
            "jet-devserver-static-windows-hostile-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let root = base.join("root");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(base.join("outside.js"), "outside").unwrap();
        std::fs::hard_link(base.join("outside.js"), root.join("hardlink.js")).unwrap();
        assert!(
            read_static_file_bounded(&root, Path::new("hardlink.js"), LIMIT).is_err(),
            "static serving must reject hardlinks on Windows"
        );
        std::fs::create_dir(root.join("directory.js")).unwrap();
        assert!(
            read_static_file_bounded(&root, Path::new("directory.js"), LIMIT).is_err(),
            "static serving must reject directories on Windows"
        );

        std::fs::write(root.join("page.html"), "safe").unwrap();
        let authority = static_authority::open_for_test(&root, Path::new("page.html")).unwrap();
        std::fs::rename(root.join("page.html"), root.join("page-held.html")).unwrap();
        assert!(
            authority.read_for_test(LIMIT).is_err(),
            "final-file relocation must fail closed on Windows"
        );

        let nested = root.join("nested");
        std::fs::create_dir(&nested).unwrap();
        std::fs::write(nested.join("page.html"), "safe").unwrap();
        let authority =
            static_authority::open_for_test(&root, Path::new("nested/page.html")).unwrap();
        let nested_held = root.join("nested-held");
        std::fs::rename(&nested, &nested_held).unwrap();
        std::fs::create_dir(&nested).unwrap();
        std::fs::write(nested.join("page.html"), "attacker").unwrap();
        assert!(
            authority.read_for_test(LIMIT).is_err(),
            "ancestor relocation must fail closed on Windows"
        );

        std::fs::write(root.join("page.html"), "safe").unwrap();
        let authority = static_authority::open_for_test(&root, Path::new("page.html")).unwrap();
        let root_held = base.join("root-held");
        std::fs::rename(&root, &root_held).unwrap();
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("page.html"), "attacker").unwrap();
        assert!(
            authority.read_for_test(LIMIT).is_err(),
            "root relocation must fail closed on Windows"
        );

        let oversized = root_held.join("oversized.js");
        let file = std::fs::File::create(&oversized).unwrap();
        file.set_len(LIMIT + 1).unwrap();
        drop(file);
        assert!(
            read_static_file_bounded(&root_held, Path::new("oversized.js"), LIMIT).is_err(),
            "oversized static responses must fail before allocation on Windows"
        );

        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn canvas_assets_are_owned_routes() {
        let page = canvas_asset("GET", "/canvas", "/canvas").unwrap();
        assert_eq!(page.status, "200 OK");
        assert!(page.body.contains("<!doctype html>"));
        let js = canvas_asset("GET", "/canvas/app.js", "/canvas/app.js").unwrap();
        assert_eq!(js.content_type, "application/javascript; charset=utf-8");
        assert_eq!(
            canvas_asset("POST", "/canvas", "/canvas").unwrap().status,
            "405 Method Not Allowed"
        );
    }
}
