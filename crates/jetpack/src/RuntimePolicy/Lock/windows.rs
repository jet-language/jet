use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::os::windows::io::AsRawHandle as _;
use std::path::{Path, PathBuf};

type Handle = *mut c_void;

const FILE_SHARE_READ: u32 = 0x0000_0001;
const FILE_SHARE_WRITE: u32 = 0x0000_0002;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const HANDLE_FLAG_INHERIT: u32 = 0x0000_0001;
const LOCKFILE_FAIL_IMMEDIATELY: u32 = 0x0000_0001;
const LOCKFILE_EXCLUSIVE_LOCK: u32 = 0x0000_0002;
const ERROR_LOCK_VIOLATION: i32 = 33;

#[repr(C)]
struct Overlapped {
    internal: usize,
    internal_high: usize,
    offset: [u32; 2],
    event: Handle,
}

#[repr(C)]
struct FileTime {
    low: u32,
    high: u32,
}

#[repr(C)]
struct ByHandleFileInformation {
    attributes: u32,
    creation_time: FileTime,
    last_access_time: FileTime,
    last_write_time: FileTime,
    volume_serial: u32,
    size_high: u32,
    size_low: u32,
    links: u32,
    file_index_high: u32,
    file_index_low: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    volume_serial: u32,
    file_index_high: u32,
    file_index_low: u32,
}

pub(super) struct LockFile {
    file: File,
    path: PathBuf,
    identity: FileIdentity,
    parents: Vec<PinnedComponent>,
}

struct PinnedComponent {
    file: File,
    path: PathBuf,
    identity: FileIdentity,
}

unsafe extern "system" {
    fn SetHandleInformation(handle: Handle, mask: u32, flags: u32) -> i32;
    fn GetFileInformationByHandle(
        handle: Handle,
        information: *mut ByHandleFileInformation,
    ) -> i32;
    fn LockFileEx(
        handle: Handle,
        flags: u32,
        reserved: u32,
        bytes_low: u32,
        bytes_high: u32,
        overlapped: *mut Overlapped,
    ) -> i32;
    fn UnlockFileEx(
        handle: Handle,
        reserved: u32,
        bytes_low: u32,
        bytes_high: u32,
        overlapped: *mut Overlapped,
    ) -> i32;
}

fn overlapped() -> Overlapped {
    Overlapped {
        internal: 0,
        internal_high: 0,
        offset: [0, 0],
        event: std::ptr::null_mut(),
    }
}

pub(super) fn open(path: &Path) -> io::Result<LockFile> {
    open_with_create(path, true)
}

pub(super) fn open_existing(path: &Path) -> io::Result<LockFile> {
    open_with_create(path, false)
}

fn open_with_create(path: &Path, create: bool) -> io::Result<LockFile> {
    // Pin each existing directory from the filesystem root through the lock
    // parent before opening the leaf. Denying delete sharing on every handle
    // prevents a junction or directory replacement from changing what later
    // path validation names.
    let parents = pin_parent_components(path)?;
    let file = open_raw(path, create)?;
    let identity = file_identity(&file)?;
    let lock = LockFile {
        file,
        path: path.to_path_buf(),
        identity,
        parents,
    };
    validate_path(&lock, path)?;
    Ok(lock)
}

fn pin_parent_components(path: &Path) -> io::Result<Vec<PinnedComponent>> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "jetpack lock path has no parent")
    })?;
    let mut paths = parent
        .ancestors()
        .filter(|component| !component.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .collect::<Vec<_>>();
    paths.reverse();
    let mut pinned = Vec::with_capacity(paths.len());
    for component in paths {
        let file = open_directory_raw(&component)?;
        let identity = file_identity(&file)?;
        pinned.push(PinnedComponent {
            file,
            path: component,
            identity,
        });
    }
    Ok(pinned)
}

fn open_raw(path: &Path, create: bool) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(create)
        // Open reparse object itself, never its target. Validation below
        // rejects every reparse-point attribute before lock use.
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        // Deny delete sharing: path cannot be replaced while any contender
        // holds its handle.
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(path)?;
    // SAFETY: raw handle belongs to live file; inheritance metadata only.
    if unsafe { SetHandleInformation(file.as_raw_handle().cast(), HANDLE_FLAG_INHERIT, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if file.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::other(format!(
            "jetpack lock path `{}` is a reparse point",
            path.display()
        )));
    }
    Ok(file)
}

fn open_directory_raw(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        // Metadata/identity access only. Do not request data read access from
        // directories whose ACL permits traversal but not listing.
        .access_mode(0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        // No FILE_SHARE_DELETE: every component name remains pinned while
        // the lock participates in acquisition or validation.
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(path)?;
    // SAFETY: raw handle belongs to live directory; inheritance metadata only.
    if unsafe { SetHandleInformation(file.as_raw_handle().cast(), HANDLE_FLAG_INHERIT, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if file.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::other(format!(
            "jetpack lock parent `{}` is a reparse point",
            path.display()
        )));
    }
    Ok(file)
}

fn file_identity(file: &File) -> io::Result<FileIdentity> {
    let mut information = std::mem::MaybeUninit::<ByHandleFileInformation>::uninit();
    // SAFETY: live handle; Windows initializes complete output on success.
    if unsafe {
        GetFileInformationByHandle(file.as_raw_handle().cast(), information.as_mut_ptr())
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: call above succeeded and initialized output.
    let information = unsafe { information.assume_init() };
    Ok(FileIdentity {
        volume_serial: information.volume_serial,
        file_index_high: information.file_index_high,
        file_index_low: information.file_index_low,
    })
}

pub(super) fn validate_path(lock: &LockFile, path: &Path) -> io::Result<()> {
    if path != lock.path {
        return Err(io::Error::other("jetpack lock validation path changed"));
    }
    if lock.file.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::other("jetpack lock handle became a reparse point"));
    }
    for component in &lock.parents {
        if component.file.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(io::Error::other(format!(
                "jetpack lock parent `{}` became a reparse point",
                component.path.display()
            )));
        }
        let reopened = open_directory_raw(&component.path)?;
        if file_identity(&reopened)? != component.identity {
            return Err(io::Error::other(format!(
                "jetpack lock parent `{}` changed file identity",
                component.path.display()
            )));
        }
    }
    let reopened = open_raw(path, false)?;
    if file_identity(&reopened)? != lock.identity {
        return Err(io::Error::other(format!(
            "jetpack lock path `{}` changed file identity",
            path.display()
        )));
    }
    Ok(())
}

pub(super) fn try_lock(lock: &LockFile) -> io::Result<bool> {
    let mut state = overlapped();
    // SAFETY: handle live; initialized state lives through synchronous call.
    if unsafe {
        LockFileEx(
            lock.file.as_raw_handle().cast(),
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            1,
            0,
            &mut state,
        )
    } != 0
    {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(ERROR_LOCK_VIOLATION) {
        Ok(false)
    } else {
        Err(error)
    }
}

pub(super) fn unlock(lock: &LockFile) -> io::Result<()> {
    let mut state = overlapped();
    // SAFETY: handle live; range matches LockFileEx above.
    if unsafe { UnlockFileEx(lock.file.as_raw_handle().cast(), 0, 1, 0, &mut state) } != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lockfileex_runtime_serializes_and_pins_file_identity() {
        let root = std::env::temp_dir().join(format!("jetpack-lockfileex-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("lock");
        let first = open(&path).unwrap();
        let second = open(&path).unwrap();
        assert!(try_lock(&first).unwrap());
        assert!(!try_lock(&second).unwrap());
        assert!(std::fs::rename(&path, root.join("replacement")).is_err());
        unlock(&first).unwrap();
        assert!(try_lock(&second).unwrap());
        unlock(&second).unwrap();
        drop(first);
        drop(second);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reparse_points_fail_closed_when_creation_is_permitted() {
        let root = std::env::temp_dir().join(format!(
            "jetpack-lockfileex-reparse-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("target");
        let link = root.join("lock");
        std::fs::write(&target, "target").unwrap();
        match std::os::windows::fs::symlink_file(&target, &link) {
            Ok(()) => assert!(open(&link).is_err()),
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {}
            Err(error) => panic!("couldn't create reparse fixture: {error}"),
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parent_junction_is_rejected_before_lock_leaf_open() {
        let root = std::env::temp_dir().join(format!(
            "jetpack-lockfileex-parent-junction-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("target");
        let junction = root.join("junction");
        std::fs::create_dir_all(&target).unwrap();
        let output = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&junction)
            .arg(&target)
            .output()
            .unwrap();
        if output.status.success() {
            assert!(
                open(&junction.join("lock")).is_err(),
                "a junction in any parent component must fail closed"
            );
            std::fs::remove_dir(&junction).unwrap();
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parent_replacement_is_blocked_while_component_identity_is_pinned() {
        let root = std::env::temp_dir().join(format!(
            "jetpack-lockfileex-parent-replace-{}",
            std::process::id()
        ));
        let parent = root.join("state");
        let displaced = root.join("state-displaced");
        std::fs::create_dir_all(&parent).unwrap();
        let path = parent.join("lock");
        let lock = open(&path).unwrap();
        assert!(std::fs::rename(&parent, &displaced).is_err());
        validate_path(&lock, &path).unwrap();
        drop(lock);
        std::fs::rename(&parent, &displaced).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
