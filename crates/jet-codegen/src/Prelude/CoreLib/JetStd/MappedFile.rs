// Shared native read-only file mapping and byte-window storage.
//
// The mapping itself is deliberately private: callers can obtain only read-only
// byte views, never a writable pointer or a shared mutable mapping. Native
// adapters marshal into this carrier; public `files.map` projection and view
// provenance remain compiler-owned.
//

static JET_MAPPED_PATHS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::BTreeMap<String, usize>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::BTreeMap::new()));

fn jet_mapped_paths() -> &'static std::sync::Mutex<std::collections::BTreeMap<String, usize>> {
    &JET_MAPPED_PATHS
}

fn jet_mapped_path_key(path: &str) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| std::path::PathBuf::from(path))
        .to_string_lossy()
        .into_owned()
}

fn jet_mapped_source_identity(key: &str) -> String {
    let digest = super::jet_sha256_raw(key.as_bytes());
    let mut identity = String::with_capacity(71);
    identity.push_str("sha256-");
    for byte in digest {
        identity.push_str(&format!("{byte:02x}"));
    }
    identity
}

fn jet_mapped_register(key: &str) {
    let mut paths = jet_mapped_paths()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *paths.entry(key.to_owned()).or_insert(0) += 1;
}

fn jet_mapped_unregister(key: &str) {
    let mut paths = jet_mapped_paths()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match paths.get_mut(key) {
        Some(count) if *count > 1 => *count -= 1,
        Some(_) => {
            paths.remove(key);
        }
        None => {}
    }
}

fn jet_mapped_path_is_live(path: &str) -> bool {
    let key = jet_mapped_path_key(path);
    let paths = jet_mapped_paths()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    paths.get(&key).copied().unwrap_or(0) != 0
}


// JET_VETTED_UNSAFE_BEGIN: jet_mapped_file
// AUDIT: this narrow native mapping seam calls flock/mmap (or the Windows
// mapping API) and constructs read-only slices from the returned address.
// Safe Rust cannot express those OS handles and foreign pointers. Callers must
// provide a readable regular file, keep the map owner alive for every view,
// and use only checked windows; violation can unlock/unmap the wrong resource,
// read outside the mapping, or cause a use-after-unmap.

#[cfg(unix)]
mod jet_mapped_os {
    use std::os::fd::{AsRawFd, RawFd};

    const LOCK_NB: i32 = 4;
    const LOCK_SH: i32 = 1;
    const LOCK_UN: i32 = 8;
    const MAP_FAILED: *mut std::ffi::c_void = !0usize as *mut std::ffi::c_void;
    const MAP_PRIVATE: i32 = 2;
    const PROT_READ: i32 = 1;

    unsafe extern "C" {
        fn flock(fd: RawFd, operation: i32) -> i32;
        fn mmap(
            address: *mut std::ffi::c_void,
            length: usize,
            protection: i32,
            flags: i32,
            fd: RawFd,
            offset: i64,
        ) -> *mut std::ffi::c_void;
        fn munmap(address: *mut std::ffi::c_void, length: usize) -> i32;
    }

    pub(super) fn shared_lock(file: &std::fs::File) -> std::io::Result<()> {
        let result = unsafe { flock(file.as_raw_fd(), LOCK_SH | LOCK_NB) };
        if result == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }

    pub(super) fn shared_unlock(file: &std::fs::File) {
        // The mapping owns the descriptor and the one shared lock. Drop invokes
        // this exactly once after the view is unmapped.
        let _ = unsafe { flock(file.as_raw_fd(), LOCK_UN) };
    }

    pub(super) fn map(file: &std::fs::File, length: usize) -> std::io::Result<*const u8> {
        let pointer = unsafe {
            mmap(
                std::ptr::null_mut(),
                length,
                PROT_READ,
                MAP_PRIVATE,
                file.as_raw_fd(),
                0,
            )
        };
        if pointer == MAP_FAILED || pointer.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        Ok(pointer.cast::<u8>())
    }

    pub(super) fn unmap(pointer: *const u8, length: usize) {
        // `map` checked the pointer and the storage checks the original length;
        // this call is therefore one complete unmap of the owned region.
        let _ = unsafe { munmap(pointer.cast_mut().cast(), length) };
    }
}

#[cfg(windows)]
mod jet_mapped_os {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::{AsRawHandle, RawHandle};

    const FILE_MAP_READ: u32 = 0x0004;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const PAGE_READONLY: u32 = 0x0000_0002;

    unsafe extern "system" {
        #[link_name = "CreateFileMappingW"]
        fn create_file_mapping(
            file: RawHandle,
            attributes: *mut std::ffi::c_void,
            protection: u32,
            maximum_size_high: u32,
            maximum_size_low: u32,
            name: *const u16,
        ) -> RawHandle;
        #[link_name = "MapViewOfFile"]
        fn map_view_of_file(
            mapping: RawHandle,
            desired_access: u32,
            file_offset_high: u32,
            file_offset_low: u32,
            number_of_bytes_to_map: usize,
        ) -> *mut std::ffi::c_void;
        #[link_name = "UnmapViewOfFile"]
        fn unmap_view_of_file(address: *const std::ffi::c_void) -> i32;
        #[link_name = "CloseHandle"]
        fn close_handle(handle: RawHandle) -> i32;
    }

    pub(super) fn open_read(path: &str) -> std::io::Result<std::fs::File> {
        // FILE_SHARE_READ deliberately excludes write and delete sharing. A
        // foreign writer therefore cannot replace or mutate a mapped file while
        // this read handle is live.
        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(path)
    }

    pub(super) fn map(
        file: &std::fs::File,
        length: usize,
    ) -> std::io::Result<(*const u8, RawHandle)> {
        let length = u64::try_from(length).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "mapped file length does not fit the Windows mapping size",
            )
        })?;
        let mapping = unsafe {
            create_file_mapping(
                file.as_raw_handle(),
                std::ptr::null_mut(),
                PAGE_READONLY,
                (length >> 32) as u32,
                length as u32,
                std::ptr::null(),
            )
        };
        if mapping.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let pointer = unsafe { map_view_of_file(mapping, FILE_MAP_READ, 0, 0, 0) };
        if pointer.is_null() {
            let _ = unsafe { close_handle(mapping) };
            return Err(std::io::Error::last_os_error());
        }
        Ok((pointer.cast::<u8>(), mapping))
    }

    pub(super) fn unmap(pointer: *const u8, mapping: RawHandle) {
        // The view and mapping object are separate native resources. Each is
        // released exactly once by JetMappedStorage::drop.
        let _ = unsafe { unmap_view_of_file(pointer.cast()) };
        let _ = unsafe { close_handle(mapping) };
    }
}

struct JetMappedStorage {
    path: String,
    key: String,
    source_identity: String,
    length: usize,
    pointer: *const u8,
    #[cfg(unix)]
    file: std::fs::File,
    #[cfg(windows)]
    file: std::fs::File,
    #[cfg(windows)]
    mapping: std::os::windows::io::RawHandle,
}

impl JetMappedStorage {
    fn open(path: &str) -> std::io::Result<Self> {
        #[cfg(not(any(unix, windows)))]
        {
            let _ = path;
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "read-only file mappings are unavailable on this target",
            ));
        }

        #[cfg(any(unix, windows))]
        {
            #[cfg(unix)]
            let file = std::fs::File::open(path)?;
            #[cfg(windows)]
            let file = jet_mapped_os::open_read(path)?;

            let length = file.metadata()?.len();
            let length = usize::try_from(length).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "mapped file is too large for this target",
                )
            })?;
            // Rust slice construction and pointer arithmetic require the region to
            // fit in isize even though the OS APIs take usize/SIZE_T.
            if length > isize::MAX as usize {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "mapped file is too large for a byte view",
                ));
            }

            #[cfg(unix)]
            jet_mapped_os::shared_lock(&file)?;

            let key = jet_mapped_path_key(path);
            let source_identity = jet_mapped_source_identity(&key);
            if length == 0 {
                jet_mapped_register(&key);
                return Ok(Self {
                    path: path.to_string(),
                    key,
                    source_identity,
                    length,
                    pointer: std::ptr::NonNull::<u8>::dangling().as_ptr(),
                    file,
                    #[cfg(windows)]
                    mapping: std::ptr::null_mut(),
                });
            }

            #[cfg(unix)]
            let pointer = match jet_mapped_os::map(&file, length) {
                Ok(pointer) => pointer,
                Err(error) => {
                    jet_mapped_os::shared_unlock(&file);
                    return Err(error);
                }
            };
            #[cfg(windows)]
            let (pointer, mapping) = jet_mapped_os::map(&file, length)?;

            jet_mapped_register(&key);
            Ok(Self {
                path: path.to_string(),
                key,
                source_identity,
                length,
                pointer,
                file,
                #[cfg(windows)]
                mapping,
            })
        }
    }
    fn invalid_window(&self, start: i64, end: i64) -> IOError {
        let length = i64::try_from(self.length).unwrap_or(i64::MAX);
        IOError::other(
            IOOperation::Read,
            Some(self.path.clone()),
            format!("mapped window {start}..{end} is outside file length {length}"),
        )
    }

    fn checked_window(&self, start: i64, end: i64) -> Result<(usize, usize), IOError> {
        let Some(start) = usize::try_from(start).ok() else {
            return Err(self.invalid_window(start, end));
        };
        let Some(end) = usize::try_from(end).ok() else {
            return Err(self.invalid_window(start as i64, end));
        };
        if start > end || end > self.length {
            return Err(self.invalid_window(start as i64, end as i64));
        }
        Ok((start, end))
    }

    fn bytes(&self, start: usize, end: usize) -> &[u8] {
        debug_assert!(start <= end && end <= self.length);
        // SAFETY: `open` maps exactly `length` readable bytes (or stores a
        // non-null dangling pointer for an empty file). `checked_window` and
        // all internal callers keep both bounds within that region.
        unsafe { std::slice::from_raw_parts(self.pointer.add(start), end - start) }
    }
}
// JET_VETTED_UNSAFE_END: jet_mapped_file


impl Drop for JetMappedStorage {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            if self.length != 0 {
                jet_mapped_os::unmap(self.pointer, self.length);
            }
            jet_mapped_os::shared_unlock(&self.file);
        }
        #[cfg(windows)]
        {
            if self.length != 0 {
                jet_mapped_os::unmap(self.pointer, self.mapping);
            }
        }
        // Keep the registry lease until after unmapping/unlocking, so a writer
        // can never observe the path as free while bytes are still borrowed.
        jet_mapped_unregister(&self.key);
    }
}

/// A private read-only mapping. The file descriptor/handle and native mapping
/// are owned by an `Arc` so every returned byte view pins the storage until its
/// own final drop. There is intentionally no `Clone` or mutable accessor.
pub struct JetMappedFile {
    storage: std::sync::Arc<JetMappedStorage>,
}

impl JetMappedFile {
    fn new(storage: JetMappedStorage) -> Self {
        Self {
            storage: std::sync::Arc::new(storage),
        }
    }

    /// Stable opaque identity for the normalized source. The path remains
    /// private while adapters can carry the mapping provenance.
    pub fn source_identity(&self) -> &str {
        &self.storage.source_identity
    }

    pub fn is_empty(&self) -> bool {
        self.storage.length == 0
    }

    /// Return a checked byte window described by offset and length.
    pub fn window_len(
        &self,
        offset: i64,
        length: i64,
    ) -> Result<JetMappedByteView, IOError> {
        let Some(end) = offset.checked_add(length) else {
            return Err(self.storage.invalid_window(offset, length));
        };
        self.window(offset, end)
    }

    pub fn len(&self) -> i64 {
        i64::try_from(self.storage.length).unwrap_or(i64::MAX)
    }

    /// Return a checked half-open byte window. The view owns a read-only Arc
    /// lease, so it cannot outlive the native mapping even when the map binding
    /// itself is dropped first.
    pub fn window(&self, start: i64, end: i64) -> Result<JetMappedByteView, IOError> {
        let (start, end) = self.storage.checked_window(start, end)?;
        Ok(JetMappedByteView {
            storage: self.storage.clone(),
            start,
            end,
        })
    }
    /// Return a borrowed byte window tied to this map binding.
    pub fn window_view(&self, start: i64, end: i64) -> Result<&[u8], IOError> {
        let (start, end) = self.storage.checked_window(start, end)?;
        Ok(self.storage.bytes(start, end))
    }

    /// Return a borrowed byte window described by offset and length.
    pub fn window_len_view(&self, offset: i64, length: i64) -> Result<&[u8], IOError> {
        let Some(end) = offset.checked_add(length) else {
            return Err(self.storage.invalid_window(offset, length));
        };
        self.window_view(offset, end)
    }

    /// Iterate line windows without materializing line bytes. Newline bytes are
    /// excluded, and a preceding CR is excluded with LF for CRLF input.
    pub fn lines(&self) -> JetMappedLines {
        JetMappedLines {
            storage: self.storage.clone(),
            cursor: 0,
        }
    }
    /// Borrowed line iteration ties every yielded slice to the map binding.
    pub fn lines_view(&self) -> JetMappedLineViews<'_> {
        JetMappedLineViews {
            storage: &self.storage,
            cursor: 0,
        }
    }
}

/// A read-only byte window pinned to one `JetMappedStorage` lease.
#[derive(Clone)]
pub struct JetMappedByteView {
    storage: std::sync::Arc<JetMappedStorage>,
    start: usize,
    end: usize,
}

impl JetMappedByteView {
    /// Stable opaque identity of the normalized source path. The path itself
    /// remains private so adapters can carry provenance without leaking it.
    pub fn source_identity(&self) -> &str {
        &self.storage.source_identity
    }

    /// Absolute byte offset of this view within its mapped source.
    pub fn offset(&self) -> u64 {
        u64::try_from(self.start).unwrap_or(u64::MAX)
    }

    pub fn len(&self) -> usize {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.storage.bytes(self.start, self.end)
    }

    /// Explicitly materialize a view when an owned byte buffer is required.
    pub fn to_vec(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }

    pub fn get(&self, index: i64) -> Option<u8> {
        let index = usize::try_from(index).ok()?;
        self.as_bytes().get(index).copied()
    }

    pub fn starts_with(&self, prefix: &[u8]) -> bool {
        self.as_bytes().starts_with(prefix)
    }

    pub fn ends_with(&self, suffix: &[u8]) -> bool {
        self.as_bytes().ends_with(suffix)
    }

    pub fn iter(&self) -> std::slice::Iter<'_, u8> {
        self.as_bytes().iter()
    }

    pub fn window(&self, start: i64, end: i64) -> Result<Self, IOError> {
        let Some(start) = usize::try_from(start).ok() else {
            return Err(self.storage.invalid_window(start, end));
        };
        let Some(end) = usize::try_from(end).ok() else {
            return Err(self.storage.invalid_window(start as i64, end));
        };
        let relative_len = self.end - self.start;
        if start > end || end > relative_len {
            return Err(self.storage.invalid_window(start as i64, end as i64));
        }
        let Some(absolute_start) = self.start.checked_add(start) else {
            return Err(self.storage.invalid_window(start as i64, end as i64));
        };
        let Some(absolute_end) = self.start.checked_add(end) else {
            return Err(self.storage.invalid_window(start as i64, end as i64));
        };
        Ok(Self {
            storage: self.storage.clone(),
            start: absolute_start,
            end: absolute_end,
        })
    }

    /// Return a checked nested window described by relative offset and length.
    pub fn window_len(&self, offset: i64, length: i64) -> Result<Self, IOError> {
        let Some(end) = offset.checked_add(length) else {
            return Err(self.storage.invalid_window(offset, length));
        };
        self.window(offset, end)
    }
}

impl AsRef<[u8]> for JetMappedByteView {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl std::ops::Deref for JetMappedByteView {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_bytes()
    }
}

impl<'a> IntoIterator for &'a JetMappedByteView {
    type Item = &'a u8;
    type IntoIter = std::slice::Iter<'a, u8>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl std::fmt::Debug for JetMappedByteView {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JetMappedByteView")
            .field("start", &self.start)
            .field("end", &self.end)
            .finish()
    }
}

/// Zero-copy line iterator over a mapped file. The iterator owns only another
/// Arc lease and each item remains a read-only byte view into the same mapping.
pub struct JetMappedLines {
    storage: std::sync::Arc<JetMappedStorage>,
    cursor: usize,
}

impl Iterator for JetMappedLines {
    type Item = JetMappedByteView;

    fn next(&mut self) -> Option<Self::Item> {
        if self.cursor >= self.storage.length {
            return None;
        }
        let start = self.cursor;
        let bytes = self.storage.bytes(start, self.storage.length);
        let mut newline = 0usize;
        while newline < bytes.len() && bytes[newline] != b'\n' {
            newline += 1;
        }
        let newline_end = start + newline;
        let line_end = if newline_end > start
            && self.storage.bytes(newline_end - 1, newline_end)[0] == b'\r'
        {
            newline_end - 1
        } else {
            newline_end
        };
        self.cursor = if newline < bytes.len() {
            newline_end.saturating_add(1)
        } else {
            newline_end
        };
        Some(JetMappedByteView {
            storage: self.storage.clone(),
            start,
            end: line_end,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.storage.length.saturating_sub(self.cursor)))
    }
}

/// Borrowed line iterator over a mapped file. The map binding is the owner,
/// so each item remains valid for the iterator's lifetime without a copy.
pub struct JetMappedLineViews<'a> {
    storage: &'a JetMappedStorage,
    cursor: usize,
}

impl<'a> Iterator for JetMappedLineViews<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        if self.cursor >= self.storage.length {
            return None;
        }
        let start = self.cursor;
        let bytes = self.storage.bytes(start, self.storage.length);
        let mut newline = 0usize;
        while newline < bytes.len() && bytes[newline] != b'\n' {
            newline += 1;
        }
        let newline_end = start + newline;
        let line_end = if newline_end > start
            && self.storage.bytes(newline_end - 1, newline_end)[0] == b'\r'
        {
            newline_end - 1
        } else {
            newline_end
        };
        self.cursor = if newline < bytes.len() {
            newline_end.saturating_add(1)
        } else {
            newline_end
        };
        Some(self.storage.bytes(start, line_end))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.storage.length.saturating_sub(self.cursor)))
    }
}

/// Shared low-level map entry. FSIo owns fault injection and the public Core
/// projection; this function owns the deterministic IOError conversion for the
/// native mapping operation.
pub(crate) fn jet_std_files_map(path: &String) -> Result<JetMappedFile, IOError> {
    JetMappedStorage::open(path)
        .map(JetMappedFile::new)
        .map_err(|error| io_error_at(IOOperation::Read, path, error))
}

/// Shared view constructor for map offsets. The checked offset/length policy
/// stays on `JetMappedFile`; adapters only marshal the carrier.
pub(crate) fn jet_std_files_map_window(
    map: &JetMappedFile,
    offset: i64,
    length: i64,
) -> Result<JetMappedByteView, IOError> {
    map.window_len(offset, length)
}
pub(crate) fn jet_std_files_map_window_view(
    map: &JetMappedFile,
    start: i64,
    end: i64,
) -> Result<&[u8], IOError> {
    map.window_view(start, end)
}

pub(crate) fn jet_std_files_map_window_len_view(
    map: &JetMappedFile,
    offset: i64,
    length: i64,
) -> Result<&[u8], IOError> {
    map.window_len_view(offset, length)
}

/// Shared lazy line carrier. Each item pins the mapping through its own Arc.
pub(crate) fn jet_std_files_map_lines(map: &JetMappedFile) -> JetMappedLines {
    map.lines()
}
/// Borrowed line carrier; the caller's map binding owns the mapping lease.
pub(crate) fn jet_std_files_map_lines_view(map: &JetMappedFile) -> JetMappedLineViews<'_> {
    map.lines_view()
}

/// Writer paths call this before opening/truncating. The process registry closes
/// the same-process race; Unix also holds an advisory shared flock and Windows
/// denies write/delete sharing on the mapped handle. Foreign Unix writers must
/// cooperate with that advisory lock.
pub(crate) fn jet_std_files_writer_refusal(path: &String) -> Option<IOError> {
    jet_mapped_path_is_live(path).then(|| {
        IOError::other(
            IOOperation::Write,
            Some(path.clone()),
            "file is mapped read-only",
        )
    })
}

