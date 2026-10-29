//! Descriptor-relative, no-follow file access below a pinned root, for the
//! `core.plugin` host's scoped FS.Read/FS.Write imports (std-only, invariant I6).

/// Hard ceiling for one rooted read.
pub const MAX_TREE_FILE_BYTES: u64 = 512 * 1024 * 1024;

/// Read one regular file below a pinned directory descriptor.
///
/// Every path component is opened relative to the held root (and held
/// ancestors) with no-follow flags. The returned bytes come from that same
/// final descriptor; no canonicalized pathname is reopened after checking.
/// Platforms without descriptor-relative no-follow access fail closed.
pub fn read_file_nofollow_at_root(
    root: &std::path::Path,
    relative: &std::path::Path,
    max_bytes: u64,
) -> std::io::Result<Vec<u8>> {
    rooted_authority::read(root, relative, max_bytes)
}
/// Atomically replace one regular file below a pinned directory descriptor.
///
/// The root and every relative parent are opened without following links.
/// Parent creation, temporary-file writes, rename, and directory syncs all
/// stay descriptor-relative, so a pathname swap cannot redirect publication.
pub fn write_file_nofollow_at_root(
    root: &std::path::Path,
    relative: &std::path::Path,
    contents: &[u8],
) -> std::io::Result<()> {
    rooted_authority::write(root, relative, contents)
}

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios"
))]
fn is_single_link(metadata: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        return metadata.nlink() == 1;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        return metadata.number_of_links() == 1;
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = metadata;
        false
    }
}

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios"
))]
mod rooted_authority {
    use super::{is_single_link, MAX_TREE_FILE_BYTES};
    use std::ffi::{c_char, CString, OsStr};
    use std::fs::{File, OpenOptions};
    use std::io::{self, Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    use std::path::{Component, Path, PathBuf};

    const O_RDONLY: i32 = 0;
    const O_WRONLY: i32 = 1;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const O_CREAT: i32 = 0o100;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    const O_CREAT: i32 = 0x0200;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const O_EXCL: i32 = 0o200;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    const O_EXCL: i32 = 0x0800;
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
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const O_NONBLOCK: i32 = 0o4000;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    const O_NONBLOCK: i32 = 0x0004;

    unsafe extern "C" {
        fn openat(directory: i32, path: *const c_char, flags: i32, ...) -> i32;
        fn mkdirat(directory: i32, path: *const c_char, mode: u32) -> i32;
        fn unlinkat(directory: i32, path: *const c_char, flags: i32) -> i32;
        fn renameat(
            old_directory: i32,
            old_path: *const c_char,
            new_directory: i32,
            new_path: *const c_char,
        ) -> i32;
    }

    #[derive(Clone)]
    struct Identity {
        device: u64,
        inode: u64,
        links: u64,
        length: u64,
        modified: Option<std::time::SystemTime>,
    }

    pub(super) struct Authority {
        directories: Vec<File>,
        directory_paths: Vec<PathBuf>,
        file: File,
        file_path: PathBuf,
        identity: Identity,
    }

    pub(super) fn read(
        root: &Path,
        relative: &Path,
        max_bytes: u64,
    ) -> io::Result<Vec<u8>> {
        if max_bytes > MAX_TREE_FILE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "file read bound exceeds the tree hashing bound",
            ));
        }
        open(root, relative)?.read_bounded(max_bytes)
    }

    pub(super) fn write(root: &Path, relative: &Path, contents: &[u8]) -> io::Result<()> {
        let components = normal_components(relative)?;
        if components.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "file authority publication path is empty",
            ));
        }
        let (mut directories, _) = open_root(root)?;
        let root_directory_count = directories.len();
        let final_component = components
            .last()
            .expect("non-empty file authority publication path");
        for component in components.iter().take(components.len() - 1) {
            let name = c_name(component)?;
            let parent = directories.last().expect("root authority exists");
            let child = match open_at(
                parent,
                &name,
                O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC,
            ) {
                Ok(child) => child,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    let created = unsafe { mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) };
                    if created != 0 {
                        let mkdir_error = io::Error::last_os_error();
                        if mkdir_error.kind() != io::ErrorKind::AlreadyExists {
                            return Err(mkdir_error);
                        }
                    } else {
                        parent.sync_all()?;
                    }
                    open_at(
                        parent,
                        &name,
                        O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC,
                    )?
                }
                Err(error) => return Err(error),
            };
            if !child.metadata()?.is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "file authority publication parent is not a directory",
                ));
            }
            directories.push(child);
        }
        let parent = directories.last().expect("root authority exists");
        let expected_destination = inspect_destination(parent, final_component)?;
        let pid = std::process::id();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let mut temporary_name = None;
        let mut temporary_file = None;
        let mut published = false;
        let result = (|| {
            for attempt in 0..128u32 {
                let candidate = format!(".jet-lock-{pid}-{stamp}-{attempt}");
                let name = c_name(OsStr::new(&candidate))?;
                match create_child_file(parent, &name) {
                    Ok(file) => {
                        temporary_name = Some(candidate);
                        temporary_file = Some(file);
                        break;
                    }
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error),
                }
            }
            let temporary_name = temporary_name.as_ref().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "could not reserve a temporary authority file",
                )
            })?;
            let mut temporary_file = temporary_file
                .take()
                .expect("temporary authority file exists with its name");
            temporary_file.write_all(contents)?;
            temporary_file.sync_all()?;
            let temporary_metadata = temporary_file.metadata()?;
            if !is_single_link(&temporary_metadata) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "file authority temporary has multiple hard links",
                ));
            }
            let temporary_identity = object_identity(&temporary_metadata);
            drop(temporary_file);

            ensure_directories_current(root, &components, root_directory_count, &directories)?;
            let current_destination = inspect_destination(parent, final_component)?;
            if current_destination != expected_destination {
                return Err(io::Error::other(
                    "file authority publication target changed while staging",
                ));
            }
            rename_replace(parent, OsStr::new(temporary_name), parent, final_component)?;
            published = true;
            for directory in &directories {
                directory.sync_all()?;
            }
            ensure_directories_current(root, &components, root_directory_count, &directories)?;
            let final_identity = inspect_destination(parent, final_component)?.ok_or_else(|| {
                io::Error::other("file authority publication target disappeared after rename")
            })?;
            if final_identity != temporary_identity {
                return Err(io::Error::other(
                    "file authority publication target changed after rename",
                ));
            }
            Ok(())
        })();
        if !published {
            if let Some(name) = temporary_name {
                let _ = unlink_child(parent, OsStr::new(&name));
            }
        }
        result
    }
    #[cfg(test)]
    pub(super) fn open_for_test(root: &Path, relative: &Path) -> io::Result<Authority> {
        open(root, relative)
    }

    fn open(root: &Path, relative: &Path) -> io::Result<Authority> {
        let components = normal_components(relative)?;
        let (mut directories, mut directory_paths) = open_root(root)?;
        if components.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "file authority path is empty",
            ));
        }
        let mut current = directory_paths
            .last()
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "file authority root is empty"))?;
        for component in components.iter().take(components.len() - 1) {
            let name = c_name(component)?;
            let child = open_at(
                directories.last().expect("root authority exists"),
                &name,
                O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC,
            )?;
            if !child.metadata()?.is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "file authority path component is not a directory",
                ));
            }
            current.push(component);
            directories.push(child);
            directory_paths.push(current.clone());
        }
        let final_component = components
            .last()
            .expect("non-empty file authority path");
        let name = c_name(final_component)?;
        let file = open_at(
            directories.last().expect("root authority exists"),
            &name,
            O_RDONLY | O_NONBLOCK | O_NOFOLLOW | O_CLOEXEC,
        )?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "file authority input is not a regular file",
            ));
        }
        if !is_single_link(&metadata) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "file authority input has multiple hard links",
            ));
        }
        let identity = identity(&metadata);
        current.push(final_component);
        Ok(Authority {
            directories,
            directory_paths,
            file,
            file_path: current,
            identity,
        })
    }

    fn open_root(root: &Path) -> io::Result<(Vec<File>, Vec<PathBuf>)> {
        let (initial, initial_path) = if root.is_absolute() {
            (open_directory_path(Path::new("/"))?, PathBuf::from("/"))
        } else {
            (open_directory_path(Path::new("."))?, PathBuf::from("."))
        };
        let mut directories = vec![initial];
        let mut directory_paths = vec![initial_path];
        let mut current = directory_paths[0].clone();
        for component in root.components() {
            match component {
                Component::RootDir | Component::CurDir => {}
                Component::Normal(name) => {
                    let value = c_name(name)?;
                    let child = open_at(
                        directories.last().expect("root authority exists"),
                        &value,
                        O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC,
                    )?;
                    if !child.metadata()?.is_dir() {
                        return Err(io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            "file authority root component is not a directory",
                        ));
                    }
                    current.push(name);
                    directories.push(child);
                    directory_paths.push(current.clone());
                }
                Component::ParentDir | Component::Prefix(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "file authority root must not contain parent components",
                    ));
                }
            }
        }
        Ok((directories, directory_paths))
    }

    impl Authority {
        #[cfg(test)]
        pub(super) fn read_for_test(self, max_bytes: u64) -> io::Result<Vec<u8>> {
            self.read_bounded(max_bytes)
        }
        fn read_bounded(mut self, max_bytes: u64) -> io::Result<Vec<u8>> {
            self.ensure_paths_current()?;
            if self.identity.length > max_bytes {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "file exceeds its read bound",
                ));
            }
            let capacity = usize::try_from(self.identity.length)
                .unwrap_or(usize::MAX)
                .min(64 * 1024);
            let mut bytes = Vec::with_capacity(capacity);
            let mut limited = (&mut self.file).take(max_bytes.saturating_add(1));
            limited.read_to_end(&mut bytes)?;
            if bytes.len() as u64 > max_bytes {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "file exceeds its read bound",
                ));
            }
            let final_metadata = self.file.metadata()?;
            let final_identity = identity(&final_metadata);
            if self.ensure_paths_current().is_err()
                || !final_metadata.is_file()
                || !is_single_link(&final_metadata)
                || !same_identity(&self.identity, &final_identity)
                || final_identity.length != bytes.len() as u64
            {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "file changed while it was being read",
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
                        "file authority directory moved while it was held",
                    ));
                }
            }
            let path_metadata = std::fs::symlink_metadata(&self.file_path)?;
            let path_identity = identity(&path_metadata);
            if !path_metadata.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "file authority input is not a regular file",
                ));
            }
            if !is_single_link(&path_metadata) {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "file authority input has multiple hard links",
                ));
            }
            if !same_identity(&self.identity, &path_identity) {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "file authority input moved while it was held",
                ));
            }
            Ok(())
        }
    }

    fn normal_components(relative: &Path) -> io::Result<Vec<&OsStr>> {
        relative
            .components()
            .filter_map(|component| match component {
                Component::Normal(name) => Some(Ok(name)),
                Component::CurDir => None,
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => Some(Err(
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "file authority path must be relative and normalized",
                    ),
                )),
            })
            .collect()
    }

    fn c_name(name: &OsStr) -> io::Result<CString> {
        CString::new(name.as_bytes()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "file authority path contains NUL")
        })
    }

    fn create_child_file(directory: &File, name: &CString) -> io::Result<File> {
        let fd = unsafe {
            openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    fn inspect_destination(
        directory: &File,
        name: &OsStr,
    ) -> io::Result<Option<(u64, u64)>> {
        let name = c_name(name)?;
        let file = match open_at(
            directory,
            &name,
            O_RDONLY | O_NONBLOCK | O_NOFOLLOW | O_CLOEXEC,
        ) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "file authority publication target is not a regular file",
            ));
        }
        if !is_single_link(&metadata) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "file authority publication target has multiple hard links",
            ));
        }
        Ok(Some(object_identity(&metadata)))
    }

    fn ensure_directories_current(
        root: &Path,
        components: &[&OsStr],
        root_directory_count: usize,
        directories: &[File],
    ) -> io::Result<()> {
        let (strict_directories, _) = open_root(root)?;
        let strict_root = strict_directories
            .last()
            .ok_or_else(|| io::Error::other("file authority root is empty"))?;
        let held_root = directories
            .get(root_directory_count.checked_sub(1).ok_or_else(|| {
                io::Error::other("file authority root has no held directory")
            })?)
            .ok_or_else(|| io::Error::other("file authority root depth changed"))?;
        if !same_directory(&strict_root.metadata()?, &held_root.metadata()?) {
            return Err(io::Error::other(
                "file authority root moved while it was held",
            ));
        }
        let mut current = strict_root.try_clone()?;
        for (index, component) in components
            .iter()
            .take(components.len() - 1)
            .enumerate()
        {
            let name = c_name(component)?;
            current = open_at(
                &current,
                &name,
                O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC,
            )?;
            let held = directories
                .get(root_directory_count + index)
                .ok_or_else(|| io::Error::other("file authority parent depth changed"))?;
            if !same_directory(&current.metadata()?, &held.metadata()?) {
                return Err(io::Error::other(
                    "file authority publication parent moved while it was held",
                ));
            }
        }
        Ok(())
    }

    fn rename_replace(
        source_parent: &File,
        source_name: &OsStr,
        destination_parent: &File,
        destination_name: &OsStr,
    ) -> io::Result<()> {
        let source_name = c_name(source_name)?;
        let destination_name = c_name(destination_name)?;
        if unsafe {
            renameat(
                source_parent.as_raw_fd(),
                source_name.as_ptr(),
                destination_parent.as_raw_fd(),
                destination_name.as_ptr(),
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn unlink_child(directory: &File, name: &OsStr) -> io::Result<()> {
        let name = c_name(name)?;
        if unsafe { unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn object_identity(metadata: &std::fs::Metadata) -> (u64, u64) {
        (metadata.dev(), metadata.ino())
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
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(if cfg!(any(target_os = "linux", target_os = "android")) {
                40
            } else {
                62
            }) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "file authority path contains a symlink",
                ));
            }
            return Err(error);
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    fn identity(metadata: &std::fs::Metadata) -> Identity {
        Identity {
            device: metadata.dev(),
            inode: metadata.ino(),
            links: metadata.nlink(),
            length: metadata.len(),
            modified: metadata.modified().ok(),
        }
    }

    fn same_directory(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
        left.is_dir()
            && right.is_dir()
            && left.dev() == right.dev()
            && left.ino() == right.ino()
    }

    fn same_identity(left: &Identity, right: &Identity) -> bool {
        left.device == right.device
            && left.inode == right.inode
            && left.length == right.length
            && left.modified == right.modified
            && left.links == right.links
    }

    #[cfg(test)]
    mod open_flag_tests {
        use super::*;

        /// Exclusive child creation uses each system's own `fcntl.h` bits:
        /// Darwin's O_CREAT/O_EXCL are 0x200/0x800, not Linux's 0o100/0o200
        /// (the Linux bits on Darwin are O_SHLOCK/O_EXLOCK, so the create was
        /// neither created nor exclusive).
        #[test]
        fn exclusive_create_flags_match_the_target_headers() {
            let flags = O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC;
            #[cfg(any(target_os = "linux", target_os = "android"))]
            assert_eq!(flags, 0o2400301);
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            assert_eq!(flags, 0x0100_0B01);
        }
    }
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios"
)))]
mod rooted_authority {
    use std::io;
    use std::path::Path;

    pub(super) fn read(
        _root: &Path,
        _relative: &Path,
        _max_bytes: u64,
    ) -> io::Result<Vec<u8>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "descriptor-relative no-follow file access is unavailable on this platform",
        ))
    }

    pub(super) fn write(
        _root: &Path,
        _relative: &Path,
        _contents: &[u8],
    ) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "descriptor-relative no-follow file access is unavailable on this platform",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    #[test]
    fn rooted_read_rejects_final_path_swap() {
        use std::os::unix::fs::symlink;
        use std::path::Path;

        let root = std::env::temp_dir().join(format!(
            "jet-rooted-read-final-swap-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let outside = root.with_file_name(format!("{}-outside", root.file_name().unwrap().to_string_lossy()));
        let payload = root.join("payload.txt");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(&outside);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&payload, b"inside\n").unwrap();
        std::fs::write(&outside, b"outside\n").unwrap();
        let authority = rooted_authority::open_for_test(&root, Path::new("payload.txt")).unwrap();
        std::fs::remove_file(&payload).unwrap();
        symlink(&outside, &payload).unwrap();
        assert!(authority.read_for_test(MAX_TREE_FILE_BYTES).is_err());
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
    fn rooted_read_rejects_ancestor_path_swap() {
        use std::os::unix::fs::symlink;
        use std::path::Path;

        let root = std::env::temp_dir().join(format!(
            "jet-rooted-read-ancestor-swap-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let outside = root.with_file_name(format!("{}-outside", root.file_name().unwrap().to_string_lossy()));
        let nested = root.join("nested");
        let payload = nested.join("payload.txt");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(&payload, b"inside\n").unwrap();
        std::fs::write(outside.join("payload.txt"), b"outside\n").unwrap();
        let authority = rooted_authority::open_for_test(&root, Path::new("nested/payload.txt")).unwrap();
        std::fs::rename(&nested, root.join("nested-old")).unwrap();
        symlink(&outside, &nested).unwrap();
        assert!(authority.read_for_test(MAX_TREE_FILE_BYTES).is_err());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    #[test]
    fn rooted_read_rejects_multiply_linked_input() {
        use std::path::Path;

        let root = std::env::temp_dir().join(format!(
            "jet-rooted-read-hardlink-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let outside = root.with_file_name(format!("{}-outside", root.file_name().unwrap().to_string_lossy()));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(&outside);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&outside, b"outside\n").unwrap();
        std::fs::hard_link(&outside, root.join("payload.txt")).unwrap();
        assert!(rooted_authority::open_for_test(&root, Path::new("payload.txt")).is_err());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(&outside);
    }

}
