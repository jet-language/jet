use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

#[cfg(any(test, windows))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowsPinnedDirectoryContract {
    directory_share_mode: u32,
    directory_flags: u32,
    member_share_mode: u32,
    member_flags: u32,
}

#[cfg(any(test, windows))]
fn windows_pinned_directory_contract() -> WindowsPinnedDirectoryContract {
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    WindowsPinnedDirectoryContract {
        // Deliberately omit FILE_SHARE_DELETE: every opened path component is
        // pinned against rename/replacement for the directory object's life.
        directory_share_mode: FILE_SHARE_READ | FILE_SHARE_WRITE,
        directory_flags: FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
        member_share_mode: FILE_SHARE_READ | FILE_SHARE_WRITE,
        member_flags: FILE_FLAG_OPEN_REPARSE_POINT,
    }
}

pub(super) struct PinnedDirectory(platform::PinnedDirectory);

impl PinnedDirectory {
    pub(super) fn open_or_create(path: &Path) -> io::Result<Self> {
        platform::PinnedDirectory::open_or_create(path).map(Self)
    }

    pub(super) fn path(&self) -> &Path {
        self.0.path()
    }

    pub(super) fn names(&self, maximum: usize) -> io::Result<Vec<OsString>> {
        self.0.names(maximum)
    }

    pub(super) fn open_read(&self, name: &str) -> io::Result<File> {
        valid_name(name)?;
        self.0.open_read(name)
    }

    pub(super) fn create_new(&self, name: &str) -> io::Result<File> {
        valid_name(name)?;
        self.0.create_new(name)
    }

    pub(super) fn rename_open(
        &self,
        source: &File,
        old_name: &str,
        new_name: &str,
        replace: bool,
    ) -> io::Result<()> {
        valid_name(old_name)?;
        valid_name(new_name)?;
        self.0.rename_open(source, old_name, new_name, replace)
    }

    pub(super) fn remove_file(&self, name: &str) -> io::Result<()> {
        valid_name(name)?;
        self.0.remove_file(name)
    }

    pub(super) fn sync(&self) -> io::Result<()> {
        super::super::sync_store_directory_handle(self.0.handle()?)
    }
}

fn valid_name(name: &str) -> io::Result<()> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.bytes().any(|byte| matches!(byte, b'/' | b'\\' | 0))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid pinned-directory member name",
        ));
    }
    Ok(())
}

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios"
))]
mod platform {
    use super::*;
    use std::ffi::{c_char, CStr, CString};
    #[cfg(target_os = "macos")]
    use std::ffi::c_void;
    use std::fs::{self, OpenOptions};
    use std::os::fd::{AsRawFd as _, FromRawFd as _};
    use std::os::unix::ffi::OsStrExt as _;
    use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};

    const O_RDONLY: i32 = 0;
    const O_WRONLY: i32 = 1;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const O_CREAT: i32 = 0o100;
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    const O_CREAT: i32 = 0x0200;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const O_EXCL: i32 = 0o200;
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
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
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    const O_NOFOLLOW: i32 = 0x0100;
    const MODE_PRIVATE: u32 = 0o700;
    const MODE_FILE: u32 = 0o600;
    const ENOENT: i32 = 2;

    unsafe extern "C" {
        fn openat(directory: i32, path: *const c_char, flags: i32, ...) -> i32;
        fn mkdirat(directory: i32, path: *const c_char, mode: u32) -> i32;
        fn renameat(
            old_directory: i32,
            old_path: *const c_char,
            new_directory: i32,
            new_path: *const c_char,
        ) -> i32;
        fn linkat(
            old_directory: i32,
            old_path: *const c_char,
            new_directory: i32,
            new_path: *const c_char,
            flags: i32,
        ) -> i32;
        fn unlinkat(directory: i32, path: *const c_char, flags: i32) -> i32;
        #[cfg(target_os = "macos")]
        fn dup(fd: i32) -> i32;
        #[cfg(target_os = "macos")]
        fn fdopendir(fd: i32) -> *mut c_void;
        #[cfg(target_os = "macos")]
        fn readdir(directory: *mut c_void) -> *mut MacDirent;
        #[cfg(target_os = "macos")]
        fn closedir(directory: *mut c_void) -> i32;
        #[cfg(target_os = "macos")]
        fn __error() -> *mut i32;
    }

    #[cfg(target_os = "macos")]
    #[repr(C)]
    struct MacDirent {
        inode: u64,
        seek_offset: u64,
        record_length: u16,
        name_length: u16,
        file_type: u8,
        name: [c_char; 1024],
    }

    #[cfg(target_os = "macos")]
    struct DirectoryStream(*mut c_void);

    #[cfg(target_os = "macos")]
    impl Drop for DirectoryStream {
        fn drop(&mut self) {
            // SAFETY: fdopendir returned this uniquely owned stream.
            let _ = unsafe { closedir(self.0) };
        }
    }

    pub(super) struct PinnedDirectory {
        file: File,
        path: PathBuf,
    }

    impl PinnedDirectory {
        pub(super) fn open_or_create(path: &Path) -> io::Result<Self> {
            let absolute = absolute(path)?;
            let mut directory = OpenOptions::new()
                .read(true)
                .custom_flags(O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
                .open(Path::new("/"))?;
            for component in absolute.components() {
                use std::path::Component;
                let Component::Normal(name) = component else {
                    if matches!(component, Component::RootDir) {
                        continue;
                    }
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "pinned directory path contains an unsupported component",
                    ));
                };
                let name = CString::new(name.as_bytes()).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "directory name contains NUL")
                })?;
                match open_directory_at(&directory, &name) {
                    Ok(child) => directory = child,
                    Err(error) if error.raw_os_error() == Some(ENOENT) => {
                        // SAFETY: parent fd and NUL-terminated component remain live;
                        // mode contains only ordinary permission bits.
                        if unsafe { mkdirat(directory.as_raw_fd(), name.as_ptr(), MODE_PRIVATE) }
                            != 0
                        {
                            let create_error = io::Error::last_os_error();
                            if create_error.kind() != io::ErrorKind::AlreadyExists {
                                return Err(create_error);
                            }
                        } else {
                            super::super::super::sync_store_directory_handle(&directory)?;
                        }
                        directory = open_directory_at(&directory, &name)?;
                    }
                    Err(error) => return Err(error),
                }
            }
            Ok(Self {
                file: directory,
                path: absolute,
            })
        }

        pub(super) fn path(&self) -> &Path {
            &self.path
        }

        pub(super) fn handle(&self) -> io::Result<&File> {
            Ok(&self.file)
        }

        pub(super) fn names(&self, maximum: usize) -> io::Result<Vec<OsString>> {
            #[cfg(any(target_os = "linux", target_os = "android"))]
            {
                let handle_path = PathBuf::from(format!("/proc/self/fd/{}", self.file.as_raw_fd()));
                return fs::read_dir(handle_path)?
                    .take(maximum)
                    .map(|entry| entry.map(|entry| entry.file_name()))
                    .collect();
            }
            #[cfg(target_os = "macos")]
            {
                return macos_names(self.file.as_raw_fd(), maximum);
            }
            #[cfg(target_os = "ios")]
            {
                let _ = maximum;
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "pinned lifecycle directory enumeration is unsupported on iOS",
                ))
            }
        }

        pub(super) fn open_read(&self, name: &str) -> io::Result<File> {
            let name = member_name(name)?;
            let file = open_file_at(&self.file, &name, O_RDONLY | O_CLOEXEC | O_NOFOLLOW, 0)?;
            require_regular(&file)?;
            Ok(file)
        }

        pub(super) fn create_new(&self, name: &str) -> io::Result<File> {
            let name = member_name(name)?;
            let file = open_file_at(
                &self.file,
                &name,
                O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC | O_NOFOLLOW,
                MODE_FILE,
            )?;
            require_regular(&file)?;
            Ok(file)
        }

        pub(super) fn rename_open(
            &self,
            source: &File,
            old_name: &str,
            new_name: &str,
            replace: bool,
        ) -> io::Result<()> {
            let old_name = member_name(old_name)?;
            let new_name = member_name(new_name)?;
            validate_open_name(&self.file, source, &old_name)?;
            if !replace {
                // linkat publishes the opened inode and atomically rejects an
                // existing destination. Removing the partial afterwards is
                // crash-safe: recovery recognizes and removes the second link.
                if unsafe {
                    linkat(
                        self.file.as_raw_fd(),
                        old_name.as_ptr(),
                        self.file.as_raw_fd(),
                        new_name.as_ptr(),
                        0,
                    )
                } != 0
                {
                    return Err(io::Error::last_os_error());
                }
                if let Err(error) = validate_open_name(&self.file, source, &new_name) {
                    let _ = unsafe { unlinkat(self.file.as_raw_fd(), new_name.as_ptr(), 0) };
                    return Err(error);
                }
                if unsafe { unlinkat(self.file.as_raw_fd(), old_name.as_ptr(), 0) } != 0 {
                    return Err(io::Error::last_os_error());
                }
                return Ok(());
            }
            // SAFETY: both directory fds and both NUL-terminated names remain
            // live through renameat; names contain no separators.
            if unsafe {
                renameat(
                    self.file.as_raw_fd(),
                    old_name.as_ptr(),
                    self.file.as_raw_fd(),
                    new_name.as_ptr(),
                )
            } != 0
            {
                return Err(io::Error::last_os_error());
            }
            validate_open_name(&self.file, source, &new_name)
        }

        pub(super) fn remove_file(&self, name: &str) -> io::Result<()> {
            let member = member_name(name)?;
            let file = self.open_read(name)?;
            validate_open_name(&self.file, &file, &member)?;
            // Lifecycle mutations are cooperative under the Hangar advisory
            // lock. Identity is checked immediately before unlink; external
            // writers that ignore that ownership law are corruption, not a
            // supported concurrent mutator.
            if unsafe { unlinkat(self.file.as_raw_fd(), member.as_ptr(), 0) } != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }

    fn absolute(path: &Path) -> io::Result<PathBuf> {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()?.join(path)
        };
        if path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        }) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "pinned directory path contains traversal",
            ));
        }
        Ok(path)
    }

    fn member_name(name: &str) -> io::Result<CString> {
        CString::new(name).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "pinned-directory member name contains NUL",
            )
        })
    }

    fn open_directory_at(parent: &File, name: &CStr) -> io::Result<File> {
        let file = open_file_at(
            parent,
            name,
            O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC,
            0,
        )?;
        if !file.metadata()?.is_dir() {
            return Err(io::Error::other("pinned path component is not a directory"));
        }
        Ok(file)
    }

    fn open_file_at(parent: &File, name: &CStr, flags: i32, mode: u32) -> io::Result<File> {
        // SAFETY: parent owns a live fd and name is NUL-terminated. Supplying a
        // mode is valid for O_CREAT and harmlessly ignored otherwise.
        let fd = unsafe { openat(parent.as_raw_fd(), name.as_ptr(), flags, mode) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful openat returned one newly owned descriptor.
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    fn require_regular(file: &File) -> io::Result<()> {
        if file.metadata()?.file_type().is_file() {
            Ok(())
        } else {
            Err(io::Error::other("pinned directory member is not a regular file"))
        }
    }

    fn validate_open_name(directory: &File, opened: &File, name: &CStr) -> io::Result<()> {
        let current = open_file_at(directory, name, O_RDONLY | O_NOFOLLOW | O_CLOEXEC, 0)?;
        let opened_meta = opened.metadata()?;
        let current_meta = current.metadata()?;
        if !current_meta.file_type().is_file()
            || opened_meta.dev() != current_meta.dev()
            || opened_meta.ino() != current_meta.ino()
        {
            return Err(io::Error::other("pinned directory member was replaced"));
        }
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn macos_names(fd: i32, maximum: usize) -> io::Result<Vec<OsString>> {
        // fdopendir owns its fd, so duplicate the pinned handle first.
        let duplicate = unsafe { dup(fd) };
        if duplicate < 0 {
            return Err(io::Error::last_os_error());
        }
        let raw = unsafe { fdopendir(duplicate) };
        if raw.is_null() {
            // fdopendir leaves the duplicate owned by the caller on failure.
            drop(unsafe { File::from_raw_fd(duplicate) });
            return Err(io::Error::last_os_error());
        }
        let stream = DirectoryStream(raw);
        let mut names = Vec::new();
        while names.len() < maximum {
            unsafe { *__error() = 0 };
            let entry = unsafe { readdir(stream.0) };
            if entry.is_null() {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(0) {
                    break;
                }
                return Err(error);
            }
            let entry = unsafe { &*entry };
            let length = usize::from(entry.name_length).min(entry.name.len());
            let bytes = unsafe {
                std::slice::from_raw_parts(entry.name.as_ptr().cast::<u8>(), length)
            };
            if matches!(bytes, b"." | b"..") {
                continue;
            }
            names.push(std::ffi::OsStr::from_bytes(bytes).to_os_string());
        }
        Ok(names)
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::ffi::c_void;
    use std::fs::{self, OpenOptions};
    use std::os::windows::ffi::OsStrExt as _;
    use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};
    use std::os::windows::io::AsRawHandle as _;

    type Handle = *mut c_void;
    const DELETE: u32 = 0x0001_0000;
    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    const FILE_RENAME_INFO_CLASS: i32 = 3;
    const FILE_DISPOSITION_INFO_CLASS: i32 = 4;

    #[repr(C)]
    struct FileRenameInfo {
        replace_if_exists: u8,
        root_directory: Handle,
        file_name_length: u32,
        file_name: [u16; 1],
    }

    #[repr(C)]
    struct FileDispositionInfo {
        delete_file: u8,
    }

    unsafe extern "system" {
        fn SetFileInformationByHandle(
            file: Handle,
            information_class: i32,
            information: *mut c_void,
            size: u32,
        ) -> i32;
    }

    pub(super) struct PinnedDirectory {
        handles: Vec<File>,
        path: PathBuf,
    }

    impl PinnedDirectory {
        pub(super) fn open_or_create(path: &Path) -> io::Result<Self> {
            let absolute = if path.is_absolute() {
                path.to_path_buf()
            } else {
                std::env::current_dir()?.join(path)
            };
            if absolute.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            }) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "pinned directory path contains traversal",
                ));
            }
            let mut current = PathBuf::new();
            let mut handles = Vec::new();
            for component in absolute.components() {
                current.push(component.as_os_str());
                if !matches!(component, std::path::Component::Normal(_)) {
                    continue;
                }
                match fs::create_dir(&current) {
                    Ok(()) => {
                        if let Some(parent) = handles.last() {
                            super::super::super::sync_store_directory_handle(parent)?;
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error),
                }
                handles.push(open_directory(&current)?);
            }
            if handles.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "pinned directory has no normal component",
                ));
            }
            Ok(Self {
                handles,
                path: absolute,
            })
        }

        pub(super) fn path(&self) -> &Path {
            &self.path
        }

        pub(super) fn handle(&self) -> io::Result<&File> {
            self.handles
                .last()
                .ok_or_else(|| io::Error::other("pinned directory has no open handle"))
        }

        pub(super) fn names(&self, maximum: usize) -> io::Result<Vec<OsString>> {
            fs::read_dir(&self.path)?
                .take(maximum)
                .map(|entry| entry.map(|entry| entry.file_name()))
                .collect()
        }

        pub(super) fn open_read(&self, name: &str) -> io::Result<File> {
            let file = open_member(
                &self.path.join(name),
                GENERIC_READ,
                false,
            )?;
            require_regular(&file)?;
            Ok(file)
        }

        pub(super) fn create_new(&self, name: &str) -> io::Result<File> {
            let file = open_member(
                &self.path.join(name),
                GENERIC_READ | GENERIC_WRITE | DELETE,
                true,
            )?;
            require_regular(&file)?;
            Ok(file)
        }

        pub(super) fn rename_open(
            &self,
            source: &File,
            _old_name: &str,
            new_name: &str,
            replace: bool,
        ) -> io::Result<()> {
            let name = std::ffi::OsStr::new(new_name)
                .encode_wide()
                .collect::<Vec<_>>();
            let offset = std::mem::offset_of!(FileRenameInfo, file_name);
            let size = offset + name.len() * std::mem::size_of::<u16>();
            let words = size.div_ceil(std::mem::size_of::<usize>());
            let mut storage = vec![0usize; words];
            let info = storage.as_mut_ptr().cast::<FileRenameInfo>();
            let directory_handle = self.handle()?.as_raw_handle().cast();
            // SAFETY: storage is pointer-aligned and sized for header plus the
            // exact UTF-16 name. All writes stay within that allocation.
            unsafe {
                (*info).replace_if_exists = u8::from(replace);
                (*info).root_directory = directory_handle;
                (*info).file_name_length = u32::try_from(name.len() * 2)
                    .map_err(|_| io::Error::other("journal filename is too long"))?;
                std::ptr::copy_nonoverlapping(
                    name.as_ptr(),
                    storage.as_mut_ptr().cast::<u8>().add(offset).cast::<u16>(),
                    name.len(),
                );
                if SetFileInformationByHandle(
                    source.as_raw_handle().cast(),
                    FILE_RENAME_INFO_CLASS,
                    info.cast(),
                    u32::try_from(size)
                        .map_err(|_| io::Error::other("rename record is too large"))?,
                ) == 0
                {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(())
        }

        pub(super) fn remove_file(&self, name: &str) -> io::Result<()> {
            let file = open_member(&self.path.join(name), GENERIC_READ | DELETE, false)?;
            require_regular(&file)?;
            let mut info = FileDispositionInfo { delete_file: 1 };
            let info_size = u32::try_from(std::mem::size_of::<FileDispositionInfo>())
                .map_err(|_| io::Error::other("disposition record is too large"))?;
            // SAFETY: file handle is live with DELETE access; info is initialized
            // and remains writable through this synchronous call.
            if unsafe {
                SetFileInformationByHandle(
                    file.as_raw_handle().cast(),
                    FILE_DISPOSITION_INFO_CLASS,
                    (&mut info as *mut FileDispositionInfo).cast(),
                    info_size,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }

    fn open_directory(path: &Path) -> io::Result<File> {
        let contract = super::windows_pinned_directory_contract();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(contract.directory_share_mode)
            .custom_flags(contract.directory_flags)
            .open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_dir()
            || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        {
            return Err(io::Error::other(
                "pinned path component is not a direct directory",
            ));
        }
        Ok(file)
    }

    fn open_member(path: &Path, access: u32, create: bool) -> io::Result<File> {
        let contract = super::windows_pinned_directory_contract();
        let mut options = OpenOptions::new();
        options
            .access_mode(access)
            .share_mode(contract.member_share_mode)
            .custom_flags(contract.member_flags);
        if create {
            options.create_new(true);
        }
        options.open(path)
    }

    fn require_regular(file: &File) -> io::Result<()> {
        let metadata = file.metadata()?;
        if metadata.file_type().is_file()
            && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0
        {
            Ok(())
        } else {
            Err(io::Error::other(
                "pinned directory member is not a direct regular file",
            ))
        }
    }
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    windows
)))]
mod platform {
    use super::*;

    pub(super) struct PinnedDirectory;

    impl PinnedDirectory {
        pub(super) fn open_or_create(_path: &Path) -> io::Result<Self> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "pinned lifecycle directories are unsupported on this platform",
            ))
        }

        pub(super) fn path(&self) -> &Path {
            Path::new("")
        }

        pub(super) fn handle(&self) -> io::Result<&File> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "unsupported pinned directory has no handle",
            ))
        }

        pub(super) fn names(&self, _maximum: usize) -> io::Result<Vec<OsString>> {
            unreachable!("unsupported pinned directory cannot list")
        }

        pub(super) fn open_read(&self, _name: &str) -> io::Result<File> {
            unreachable!("unsupported pinned directory cannot open")
        }

        pub(super) fn create_new(&self, _name: &str) -> io::Result<File> {
            unreachable!("unsupported pinned directory cannot create")
        }

        pub(super) fn rename_open(
            &self,
            _source: &File,
            _old_name: &str,
            _new_name: &str,
            _replace: bool,
        ) -> io::Result<()> {
            unreachable!("unsupported pinned directory cannot rename")
        }

        pub(super) fn remove_file(&self, _name: &str) -> io::Result<()> {
            unreachable!("unsupported pinned directory cannot remove")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read as _, Write as _};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn fixture() -> PathBuf {
        std::env::temp_dir().join(format!(
            "jet-pinned-directory-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn handle_relative_io_survives_component_replacement() {
        let root = fixture();
        let original = root.join("a/b");
        let directory = PinnedDirectory::open_or_create(&original).unwrap();
        let moved = root.join("held");
        std::fs::rename(root.join("a"), &moved).unwrap();
        std::fs::create_dir_all(&original).unwrap();
        let mut file = directory.create_new("proof.partial").unwrap();
        file.write_all(b"held").unwrap();
        file.sync_all().unwrap();
        directory
            .rename_open(&file, "proof.partial", "proof.txn", false)
            .unwrap();
        directory.sync().unwrap();
        drop(file);
        assert!(!original.join("proof.txn").exists());
        let mut proof = directory.open_read("proof.txn").unwrap();
        let mut text = String::new();
        proof.read_to_string(&mut text).unwrap();
        assert_eq!(text, "held");
        drop(proof);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn enumeration_stops_at_requested_limit() {
        let root = fixture();
        let directory = PinnedDirectory::open_or_create(&root).unwrap();
        for index in 0..8 {
            let _ = directory.create_new(&format!("{index}.txn")).unwrap();
        }
        assert_eq!(directory.names(3).unwrap().len(), 3);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn no_replace_publication_is_atomic() {
        let root = fixture();
        let directory = PinnedDirectory::open_or_create(&root).unwrap();
        let mut source = directory.create_new("source.partial").unwrap();
        source.write_all(b"source").unwrap();
        source.sync_all().unwrap();
        let mut destination = directory.create_new("destination.txn").unwrap();
        destination.write_all(b"destination").unwrap();
        destination.sync_all().unwrap();
        assert_eq!(
            directory
                .rename_open(&source, "source.partial", "destination.txn", false)
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        let mut text = String::new();
        directory.open_read("destination.txn").unwrap().read_to_string(&mut text).unwrap();
        assert_eq!(text, "destination");
        assert!(directory.open_read("source.partial").is_ok());
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn component_symlink_is_never_followed() {
        use std::os::unix::fs::symlink;

        let root = fixture();
        let outside = fixture();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        symlink(&outside, root.join("lifecycle-db")).unwrap();
        assert!(PinnedDirectory::open_or_create(&root.join("lifecycle-db/journal")).is_err());
        assert!(std::fs::read_dir(&outside).unwrap().next().is_none());
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[test]
    fn windows_pinned_contract_denies_delete_and_opens_reparse_points() {
        let contract = windows_pinned_directory_contract();
        assert_eq!(contract.directory_share_mode, 0x3);
        assert_eq!(contract.directory_flags, 0x0220_0000);
        assert_eq!(contract.member_share_mode, 0x3);
        assert_eq!(contract.member_flags, 0x0020_0000);
    }
}
