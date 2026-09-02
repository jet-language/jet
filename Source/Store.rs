//! Nix-style package store (M12.1, D-PM1/5).
//!
//! Store layout: `~/.jet/store/<name>-<version>-<fingerprint>/`
//! Full plan fingerprint is the path suffix. Lookups use the lockfile,
//! not dirname parsing. Hardlinks into project `.jet-build/deps/` on
//! same device; falls back to copy cross-device. Append-only; `jet clean`
//! removes unreferenced entries (stub in M12.1).

use crate::Diagnostics::Diagnostic;
use crate::Syntax;
use crate::SHA256::StreamingSha256;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};


#[cfg(test)]
type StoreWalkHook = std::sync::Arc<dyn Fn(&Path, &OsStr) + Send + Sync + 'static>;

#[cfg(test)]
static STORE_WALK_HOOK: std::sync::Mutex<Option<StoreWalkHook>> = std::sync::Mutex::new(None);

#[cfg(test)]
fn before_store_child_open(path: &Path, name: &OsStr) {
    let hook = STORE_WALK_HOOK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    if let Some(hook) = hook {
        hook(path, name);
    }
}

#[cfg(test)]
fn install_store_walk_hook(hook: StoreWalkHook) -> StoreWalkHookGuard {
    *STORE_WALK_HOOK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(hook);
    StoreWalkHookGuard
}

#[cfg(test)]
struct StoreWalkHookGuard;

#[cfg(test)]
impl Drop for StoreWalkHookGuard {
    fn drop(&mut self) {
        *STORE_WALK_HOOK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }
}

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios"
))]
use std::os::fd::{AsRawFd, FromRawFd};

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios"
)))]
const DESCRIPTOR_PLATFORMS: &str = "linux, Android, macOS, or iOS";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthorityReplaceStatus {
    NotPublished,
    Published,
}

#[derive(Debug)]
pub struct AuthorityReplaceError {
    status: AuthorityReplaceStatus,
    error: io::Error,
}

impl AuthorityReplaceError {
    pub fn status(&self) -> AuthorityReplaceStatus {
        self.status
    }

    pub fn is_published(&self) -> bool {
        self.status == AuthorityReplaceStatus::Published
    }

    fn not_published(error: io::Error) -> Self {
        Self {
            status: AuthorityReplaceStatus::NotPublished,
            error,
        }
    }

    fn published(error: io::Error) -> Self {
        Self {
            status: AuthorityReplaceStatus::Published,
            error,
        }
    }
}

impl std::fmt::Display for AuthorityReplaceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(formatter)
    }
}

impl std::error::Error for AuthorityReplaceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

#[derive(Debug)]
pub struct DirectoryAuthority {
    path: PathBuf,
    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    handle: fs::File,
}

impl DirectoryAuthority {
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        #[cfg(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios"
        ))]
        let handle = directory_platform::open_path(path)?;
        #[cfg(not(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios"
        )))]
        directory_platform::open_path(path)?;
        Ok(Self {
            path: effective_directory_path(path),
            #[cfg(any(
                target_os = "linux",
                target_os = "android",
                target_os = "macos",
                target_os = "ios"
            ))]
            handle,
        })
    }

    pub(crate) fn create(path: &Path) -> io::Result<Self> {
        #[cfg(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios"
        ))]
        let handle = directory_platform::create_path(path)?;
        #[cfg(not(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios"
        )))]
        directory_platform::create_path(path)?;
        Ok(Self {
            path: effective_directory_path(path),
            #[cfg(any(
                target_os = "linux",
                target_os = "android",
                target_os = "macos",
                target_os = "ios"
            ))]
            handle,
        })
    }

    fn from_opened(path: PathBuf, file: fs::File) -> Self {
        #[cfg(not(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios"
        )))]
        let _ = file;
        Self {
            path,
            #[cfg(any(
                target_os = "linux",
                target_os = "android",
                target_os = "macos",
                target_os = "ios"
            ))]
            handle: file,
        }
    }


    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    fn try_clone(&self) -> io::Result<Self> {
        #[cfg(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios"
        ))]
        {
            return Ok(Self {
                path: self.path.clone(),
                handle: self.handle.try_clone()?,
            });
        }
        #[cfg(not(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios"
        )))]
        {
            Err(unsupported_descriptor_error())
        }
    }

    fn open_child(&self, name: &OsStr) -> io::Result<fs::File> {
        directory_platform::open_child(&self.handle_ref(), name, false)
    }

    pub(crate) fn open_child_directory(&self, name: &OsStr) -> io::Result<Self> {
        let handle = directory_platform::open_child(&self.handle_ref(), name, true)?;
        Ok(Self {
            path: self.path.join(name),
            #[cfg(any(
                target_os = "linux",
                target_os = "android",
                target_os = "macos",
                target_os = "ios"
            ))]
            handle,
        })
    }

    fn create_child_directory(&self, name: &OsStr) -> io::Result<Self> {
        directory_platform::mkdir_child(&self.handle_ref(), name)?;
        self.open_child_directory(name)
    }

    fn open_relative_parent(&self, relative: &Path) -> io::Result<(Self, OsString)> {
        let mut components = authority_path_components(relative)?;
        let name = components.pop().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "authority path has no file name",
            )
        })?;
        let mut parent = self.try_clone()?;
        for component in components {
            parent = parent.open_child_directory(&component)?;
        }
        Ok((parent, name))
    }

    fn ensure_relative_parent(&self, relative: &Path) -> io::Result<(Self, OsString)> {
        let mut components = authority_path_components(relative)?;
        let name = components.pop().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "authority path has no file name",
            )
        })?;
        let mut parent = self.try_clone()?;
        for component in components {
            parent = match parent.open_child_directory(&component) {
                Ok(directory) => directory,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    parent.create_child_directory(&component)?
                }
                Err(error) => return Err(error),
            };
        }
        Ok((parent, name))
    }

    fn snapshot_child_file(
        &self,
        name: OsString,
        path: PathBuf,
        relative: PathBuf,
        root_metadata: fs::Metadata,
    ) -> io::Result<AuthorityFileSnapshot> {
        self.revalidate()?;
        let file = self.open_child(&name)?;
        let metadata = file.metadata()?;
        validate_authority_file(&metadata)?;
        let bytes = read_authority_handle(&file, &metadata)?;
        self.revalidate()?;
        Ok(AuthorityFileSnapshot {
            authority: self.try_clone()?,
            name,
            path,
            relative,
            root_metadata,
            file,
            metadata,
            bytes,
        })
    }
    pub(crate) fn snapshot_file(&self, relative: &Path) -> io::Result<AuthorityFileSnapshot> {
        let (parent, name) = self.open_relative_parent(relative)?;
        #[cfg(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios"
        ))]
        let root_metadata = self.handle.metadata()?;
        #[cfg(not(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios"
        )))]
        let root_metadata = return Err(unsupported_descriptor_error());
        parent.snapshot_child_file(
            name,
            self.path.join(relative),
            relative.to_path_buf(),
            root_metadata,
        )
    }



    pub(crate) fn create_private_child(&self, prefix: &str) -> io::Result<(OsString, Self)> {
        let path = directory_platform::create_private_child(&self.handle_ref(), prefix)?;
        let name = path
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "temporary directory has no name"))?
            .to_os_string();
        let child = self.open_child_directory(&name)?;
        Ok((name, child))
    }

    pub(crate) fn list_names(&self) -> io::Result<Vec<OsString>> {
        let mut names = directory_platform::read_names(&self.handle_ref())?;
        names.sort_unstable();
        Ok(names)
    }

    fn create_child_file(&self, name: &OsStr) -> io::Result<fs::File> {
        directory_platform::create_child_file(&self.handle_ref(), name)
    }

    pub(crate) fn write_child_file(&self, name: &OsStr, bytes: &[u8]) -> io::Result<()> {
        let mut file = match directory_platform::open_child_write(&self.handle_ref(), name) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                directory_platform::create_child_file(&self.handle_ref(), name)?
            }
            Err(error) => return Err(error),
        };
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "destination is not a regular file",
            ));
        }
        if file_nlink(&metadata) > 1 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "refusing to overwrite a multiply linked destination file",
            ));
        }
        file.set_len(0)?;
        file.write_all(bytes)?;
        file.sync_all()
    }
    fn append_child_file_unlocked(&self, name: &OsStr, bytes: &[u8]) -> io::Result<()> {
        if bytes.len() as u64 > crate::SHA256::MAX_TREE_FILE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "authority file exceeds the tree-file size bound",
            ));
        }
        self.revalidate()?;
        let mut file = match directory_platform::open_child_append(&self.handle_ref(), name) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match directory_platform::create_child_append(&self.handle_ref(), name) {
                    Ok(file) => file,
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                        directory_platform::open_child_append(&self.handle_ref(), name)?
                    }
                    Err(error) => return Err(error),
                }
            }
            Err(error) => return Err(error),
        };
        let before = file.metadata()?;
        validate_authority_file(&before)?;
        if before
            .len()
            .checked_add(bytes.len() as u64)
            .is_none_or(|length| length > crate::SHA256::MAX_TREE_FILE_BYTES)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "authority file exceeds the tree-file size bound",
            ));
        }
        file.write_all(bytes)?;
        file.sync_all()?;
        let after = file.metadata()?;
        validate_authority_file(&after)?;
        if !same_store_object_identity(&before, &after)
            || after.len() != before.len().saturating_add(bytes.len() as u64)
        {
            return Err(io::Error::other(
                "authority file changed while appending the receipt",
            ));
        }
        let current = self.open_child(name)?;
        let current_metadata = current.metadata()?;
        validate_authority_file(&current_metadata)?;
        if !same_store_object_identity(&after, &current_metadata) {
            return Err(io::Error::other(
                "authority receipt target changed during append",
            ));
        }
        self.sync_all()?;
        Ok(())
    }

    pub fn append_child_file(&self, name: &OsStr, bytes: &[u8]) -> io::Result<()> {
        self.revalidate()?;
        self.lock_exclusive()?;
        let result = self.append_child_file_unlocked(name, bytes);
        let unlock = self.unlock_exclusive();
        match (result, unlock) {
            (Err(error), _) => Err(error),
            (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    pub(crate) fn remove_child_tree(&self, name: &OsStr) -> io::Result<()> {
        match self.open_child_directory(name) {
            Ok(child) => {
                child.remove_all_children()?;
                directory_platform::unlink_directory(&self.handle_ref(), name)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotADirectory => {
                directory_platform::unlink_any(&self.handle_ref(), name)
            }
            Err(error) => {
                // A no-follow open rejects links. Removing the entry itself is
                // safe, while following it would not be.
                if is_no_follow_error(&error) {
                    directory_platform::unlink_any(&self.handle_ref(), name)
                } else {
                    Err(error)
                }
            }
        }
    }

    fn remove_all_children(&self) -> io::Result<()> {
        let names = self.list_names()?;
        for name in names {
            self.remove_child_tree(&name)?;
        }
        Ok(())
    }

    pub fn revalidate(&self) -> io::Result<()> {
        #[cfg(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios"
        ))]
        {
            let current = directory_platform::open_path(&self.path)?;
            let held = self.handle.metadata()?;
            let observed = current.metadata()?;
            if !same_store_object_identity(&held, &observed) {
                return Err(io::Error::other(
                    "authority directory changed while it was in use",
                ));
            }
            return Ok(());
        }
        #[cfg(not(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios"
        )))]
        {
            Err(unsupported_descriptor_error())
        }
    }

    pub fn sync_all(&self) -> io::Result<()> {
        #[cfg(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios"
        ))]
        {
            return self.handle.sync_all();
        }
        #[cfg(not(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios"
        )))]
        {
            Err(unsupported_descriptor_error())
        }
    }
    fn handle_metadata(&self) -> io::Result<fs::Metadata> {
        #[cfg(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios"
        ))]
        {
            return self.handle.metadata();
        }
        #[cfg(not(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios"
        )))]
        {
            Err(unsupported_descriptor_error())
        }
    }


    fn lock_exclusive(&self) -> io::Result<()> {
        #[cfg(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios"
        ))]
        {
            return directory_platform::lock_exclusive(&self.handle);
        }
        #[cfg(not(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios"
        )))]
        {
            Err(unsupported_descriptor_error())
        }
    }

    fn unlock_exclusive(&self) -> io::Result<()> {
        #[cfg(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios"
        ))]
        {
            return directory_platform::unlock(&self.handle);
        }
        #[cfg(not(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios"
        )))]
        {
            Err(unsupported_descriptor_error())
        }
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    pub(crate) fn inheritable_clone(&self) -> io::Result<fs::File> {
        directory_platform::duplicate_inheritable(&self.handle)
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    fn handle_ref(&self) -> fs::File {
        self.handle.try_clone().expect("authority descriptor clone")
    }

    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    )))]
    fn handle_ref(&self) -> fs::File {
        unreachable!("unsupported descriptor authority has no handle")
    }
}
pub struct AuthorityTransaction {
    root: DirectoryAuthority,
    locked: bool,
}

impl AuthorityTransaction {
    pub fn begin(root_path: &Path) -> io::Result<Self> {
        let root = DirectoryAuthority::open(root_path)?;
        root.revalidate()?;
        root.lock_exclusive()?;
        Ok(Self {
            root,
            locked: true,
        })
    }

    pub fn snapshot_file(&self, relative: &Path) -> io::Result<AuthorityFileSnapshot> {
        self.root.snapshot_file(relative)
    }

    pub fn append_file(&self, relative: &Path, bytes: &[u8]) -> io::Result<()> {
        let (parent, name) = self.root.ensure_relative_parent(relative)?;
        parent.append_child_file_unlocked(&name, bytes)
    }

    pub fn replace_file(
        &self,
        snapshot: &AuthorityFileSnapshot,
        replacement: &[u8],
    ) -> Result<(), AuthorityReplaceError> {
        let root_metadata = self.root.handle_metadata().map_err(|error| {
            AuthorityReplaceError::not_published(error)
        })?;
        if !same_store_object_identity(&root_metadata, &snapshot.root_metadata) {
            return Err(AuthorityReplaceError::not_published(io::Error::other(
                "authority project root changed while approval was pending",
            )));
        }
        let (parent, name) = self
            .root
            .open_relative_parent(&snapshot.relative)
            .map_err(AuthorityReplaceError::not_published)?;
        let result = replace_authority_file_unlocked(&parent, &name, snapshot, replacement);
        if result.is_ok() {
            if let Err(error) = self.root.sync_all() {
                return Err(AuthorityReplaceError::published(error));
            }
        }
        result
    }

    pub fn finish(mut self) -> io::Result<()> {
        if !self.locked {
            return Ok(());
        }
        self.locked = false;
        self.root.unlock_exclusive()
    }
}

impl Drop for AuthorityTransaction {
    fn drop(&mut self) {
        if self.locked {
            let _ = self.root.unlock_exclusive();
            self.locked = false;
        }
    }
}


pub struct AuthorityFileSnapshot {
    authority: DirectoryAuthority,
    name: OsString,
    path: PathBuf,
    relative: PathBuf,
    root_metadata: fs::Metadata,
    file: fs::File,
    metadata: fs::Metadata,
    bytes: Vec<u8>,
}

impl AuthorityFileSnapshot {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn text(&self) -> io::Result<&str> {
        std::str::from_utf8(&self.bytes).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("authority file is not UTF-8: {error}"),
            )
        })
    }
}

fn validate_authority_file(metadata: &fs::Metadata) -> io::Result<()> {
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "authority target is not a regular file",
        ));
    }
    if file_nlink(metadata) > 1 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "refusing a multiply linked authority file",
        ));
    }
    Ok(())
}

fn read_authority_handle(
    file: &fs::File,
    initial: &fs::Metadata,
) -> io::Result<Vec<u8>> {
    if initial.len() > crate::SHA256::MAX_TREE_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "authority file exceeds the tree-file size bound",
        ));
    }
    let mut bytes = Vec::new();
    let mut limited = file.take(crate::SHA256::MAX_TREE_FILE_BYTES.saturating_add(1));
    limited.read_to_end(&mut bytes)?;
    if bytes.len() as u64 > crate::SHA256::MAX_TREE_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "authority file grew beyond the tree-file size bound",
        ));
    }
    let observed = file.metadata()?;
    validate_authority_file(&observed)?;
    if !same_store_file_identity(initial, &observed) || observed.len() != bytes.len() as u64 {
        return Err(io::Error::other(
            "authority file changed while it was being read",
        ));
    }
    Ok(bytes)
}

fn authority_path_components(path: &Path) -> io::Result<Vec<OsString>> {
    if path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "authority path must be relative to its retained root",
        ));
    }
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => components.push(name.to_os_string()),
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "authority path contains unsupported components",
                ))
            }
        }
    }
    Ok(components)
}

pub fn read_authority_file(path: &Path) -> io::Result<AuthorityFileSnapshot> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "authority path has no file name"))?
        .to_os_string();
    let authority = DirectoryAuthority::open(parent)?;
    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    let root_metadata = authority.handle.metadata()?;
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    )))]
    let root_metadata = return Err(unsupported_descriptor_error());
    authority.snapshot_child_file(
        name,
        path.to_path_buf(),
        PathBuf::from(path.file_name().unwrap()),
        root_metadata,
    )
}


pub fn append_authority_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "authority path has no file name"))?;
    DirectoryAuthority::create(parent)?.append_child_file(name, bytes)
}

fn verify_authority_snapshot_target(
    authority: &DirectoryAuthority,
    name: &OsStr,
    expected_metadata: &fs::Metadata,
    expected_bytes: &[u8],
) -> io::Result<()> {
    let file = authority.open_child(name)?;
    let metadata = file.metadata()?;
    validate_authority_file(&metadata)?;
    if !same_store_object_identity(expected_metadata, &metadata) {
        return Err(io::Error::other(
            "authority target changed while approval was pending",
        ));
    }
    let bytes = read_authority_handle(&file, &metadata)?;
    if bytes != expected_bytes {
        return Err(io::Error::other(
            "authority target contents changed while approval was pending",
        ));
    }
    Ok(())
}

fn replace_authority_file_unlocked(
    authority: &DirectoryAuthority,
    name: &OsStr,
    snapshot: &AuthorityFileSnapshot,
    replacement: &[u8],
) -> Result<(), AuthorityReplaceError> {
    if replacement.len() as u64 > crate::SHA256::MAX_TREE_FILE_BYTES {
        return Err(AuthorityReplaceError::not_published(io::Error::new(
            io::ErrorKind::InvalidData,
            "authority replacement exceeds the tree-file size bound",
        )));
    }
    let held_metadata = snapshot
        .file
        .metadata()
        .map_err(AuthorityReplaceError::not_published)?;
    if !same_store_object_identity(&snapshot.metadata, &held_metadata)
        || held_metadata.len() != snapshot.metadata.len()
    {
        return Err(AuthorityReplaceError::not_published(io::Error::other(
            "authority snapshot changed before approval could be published",
        )));
    }
    authority
        .revalidate()
        .map_err(AuthorityReplaceError::not_published)?;
    let mut renamed = false;
    let mut temp_name = None;
    let result = (|| {
        verify_authority_snapshot_target(authority, name, &snapshot.metadata, &snapshot.bytes)?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let pid = std::process::id();
        let mut stage = None;
        for attempt in 0..32u32 {
            let candidate = OsString::from(format!(
                ".jet-authority-replace-{pid}-{stamp}-{attempt}"
            ));
            match authority.create_child_file(&candidate) {
                Ok(file) => {
                    temp_name = Some(candidate);
                    stage = Some(file);
                    break;
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        let temp_name = temp_name.as_ref().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                "could not reserve an authority replacement",
            )
        })?;
        let mut stage = stage.expect("authority replacement stage");
        stage.write_all(replacement)?;
        stage.sync_all()?;
        let staged_metadata = stage.metadata()?;
        validate_authority_file(&staged_metadata)?;
        drop(stage);
        verify_authority_snapshot_target(authority, name, &snapshot.metadata, &snapshot.bytes)?;
        directory_platform::rename_replace(
            &authority.handle_ref(),
            temp_name,
            &authority.handle_ref(),
            name,
        )?;
        renamed = true;
        authority.sync_all()?;
        let final_file = authority.open_child(name)?;
        let final_metadata = final_file.metadata()?;
        validate_authority_file(&final_metadata)?;
        if !same_store_object_identity(&staged_metadata, &final_metadata) {
            return Err(io::Error::other(
                "authority replacement target changed after publication",
            ));
        }
        let final_bytes = read_authority_handle(&final_file, &final_metadata)?;
        if final_bytes != replacement {
            return Err(io::Error::other(
                "authority replacement contents changed after publication",
            ));
        }
        Ok(())
    })();
    if !renamed {
        if let Some(temp_name) = temp_name.as_ref() {
            let _ = authority.remove_child_tree(temp_name);
        }
    }
    match result {
        Ok(()) => Ok(()),
        Err(error) if renamed => Err(AuthorityReplaceError::published(error)),
        Err(error) => Err(AuthorityReplaceError::not_published(error)),
    }
}

pub fn replace_authority_file(
    snapshot: &AuthorityFileSnapshot,
    replacement: &[u8],
) -> Result<(), AuthorityReplaceError> {
    let authority = &snapshot.authority;
    authority
        .revalidate()
        .map_err(AuthorityReplaceError::not_published)?;
    authority
        .lock_exclusive()
        .map_err(AuthorityReplaceError::not_published)?;
    let result = replace_authority_file_unlocked(authority, &snapshot.name, snapshot, replacement);
    let unlock = authority.unlock_exclusive();
    match (result, unlock) {
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(AuthorityReplaceError::published(error)),
        (Ok(()), Ok(())) => Ok(()),
    }
}

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios"
))]
pub(crate) fn descriptor_path_for(file: &fs::File) -> PathBuf {
    directory_platform::descriptor_path(file)
}

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios"
))]
pub(crate) fn directory_authority_from_file(
    path: &Path,
    file: &fs::File,
) -> io::Result<DirectoryAuthority> {
    Ok(DirectoryAuthority {
        path: effective_directory_path(path),
        handle: file.try_clone()?,
    })
}

pub(crate) fn open_directory_authority(path: &Path) -> io::Result<DirectoryAuthority> {
    DirectoryAuthority::open(path)
}

pub(crate) fn ensure_directory_authority(path: &Path) -> io::Result<DirectoryAuthority> {
    DirectoryAuthority::create(path)
}

pub(crate) fn publish_directory(
    source_parent: &DirectoryAuthority,
    source_name: &OsStr,
    destination_parent: &DirectoryAuthority,
    destination_name: &OsStr,
) -> io::Result<()> {
    directory_platform::rename_no_replace(
        &source_parent.handle_ref(),
        source_name,
        &destination_parent.handle_ref(),
        destination_name,
    )
}

fn effective_directory_path(path: &Path) -> PathBuf {
    if path.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        path.to_path_buf()
    }
}

pub fn begin_authority_transaction(path: &Path) -> io::Result<AuthorityTransaction> {
    AuthorityTransaction::begin(path)
}

mod directory_platform {
    use super::*;
    use std::ffi::{c_char, CString};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;

    const O_RDONLY: i32 = 0;
    const O_WRONLY: i32 = 1;
    const O_APPEND: i32 = if cfg!(any(target_os = "linux", target_os = "android")) {
        0o2000
    } else {
        0x0008
    };
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
    const O_NONBLOCK: i32 = 0o4000;
    const O_CREAT: i32 = 0o100;
    const O_EXCL: i32 = 0o200;
    const AT_REMOVEDIR: i32 = 0x200;
    const LOCK_EX: i32 = 2;
    const LOCK_UN: i32 = 8;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const AT_EMPTY_PATH: i32 = 0x1000;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const RENAME_NOREPLACE: u32 = 1;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    const RENAME_EXCL: u32 = 0x0004;

    unsafe extern "C" {
        fn openat(directory: i32, path: *const c_char, flags: i32, mode: u32) -> i32;
        fn mkdirat(directory: i32, path: *const c_char, mode: u32) -> i32;
        fn unlinkat(directory: i32, path: *const c_char, flags: i32) -> i32;
        #[cfg(any(target_os = "linux", target_os = "android"))]
        fn renameat2(
            old_directory: i32,
            old_path: *const c_char,
            new_directory: i32,
            new_path: *const c_char,
            flags: u32,
        ) -> i32;
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        fn renameatx_np(
            old_directory: i32,
            old_path: *const c_char,
            new_directory: i32,
            new_path: *const c_char,
            flags: u32,
        ) -> i32;
        #[cfg(any(target_os = "linux", target_os = "android"))]
        fn linkat(
            old_directory: i32,
            old_path: *const c_char,
            new_directory: i32,
            new_path: *const c_char,
            flags: i32,
        ) -> i32;
        fn fcntl(file: i32, command: i32, ...) -> i32;
        fn renameat(
            old_directory: i32,
            old_path: *const c_char,
            new_directory: i32,
            new_path: *const c_char,
        ) -> i32;
        fn flock(file: i32, operation: i32) -> i32;
    }

    fn name(value: &OsStr) -> io::Result<CString> {
        let bytes = value.as_bytes();
        if bytes.is_empty() || bytes == b"." || bytes == b".." || bytes.contains(&b'/') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "authority child name is not one safe path component",
            ));
        }
        CString::new(bytes).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "authority child name contains NUL",
            )
        })
    }

    fn open_anchor(path: &Path) -> io::Result<fs::File> {
        let anchor = if path.is_absolute() {
            Path::new("/")
        } else {
            Path::new(".")
        };
        let mut options = fs::OpenOptions::new();
        options.read(true).custom_flags(O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
        let file = options.open(anchor)?;
        if !file.metadata()?.is_dir() {
            return Err(io::Error::other("authority anchor is not a directory"));
        }
        Ok(file)
    }

    fn components(path: &Path) -> io::Result<Vec<OsString>> {
        let mut out = Vec::new();
        for component in path.components() {
            match component {
                std::path::Component::RootDir | std::path::Component::CurDir => {}
                std::path::Component::Normal(name) => out.push(name.to_os_string()),
                std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "authority path contains unsupported components",
                    ))
                }
            }
        }
        if out.is_empty() && path.as_os_str().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "authority path is empty",
            ));
        }
        Ok(out)
    }

    fn open_at(directory: &fs::File, value: &OsStr, want_directory: bool) -> io::Result<fs::File> {
        let value = name(value)?;
        let flags = if want_directory {
            O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC
        } else {
            O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK
        };
        let fd = unsafe { openat(directory.as_raw_fd(), value.as_ptr(), flags, 0) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { fs::File::from_raw_fd(fd) })
    }

    pub(super) fn open_path(path: &Path) -> io::Result<fs::File> {
        let mut current = open_anchor(path)?;
        for component in components(path)? {
            current = open_at(&current, &component, true)?;
        }
        Ok(current)
    }

    pub(super) fn create_path(path: &Path) -> io::Result<fs::File> {
        let mut current = open_anchor(path)?;
        for component in components(path)? {
            match open_at(&current, &component, true) {
                Ok(next) => current = next,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    let component_name = name(&component)?;
                    if unsafe { mkdirat(current.as_raw_fd(), component_name.as_ptr(), 0o700) } != 0 {
                        let mkdir_error = io::Error::last_os_error();
                        if mkdir_error.kind() != io::ErrorKind::AlreadyExists {
                            return Err(mkdir_error);
                        }
                    } else {
                        current.sync_all()?;
                    }
                    current = open_at(&current, &component, true)?;
                }
                Err(error) => return Err(error),
            }
        }
        Ok(current)
    }

    pub(super) fn open_child(
        directory: &fs::File,
        child: &OsStr,
        want_directory: bool,
    ) -> io::Result<fs::File> {
        open_at(directory, child, want_directory)
    }

    pub(super) fn mkdir_child(directory: &fs::File, child: &OsStr) -> io::Result<()> {
        let child = name(child)?;
        if unsafe { mkdirat(directory.as_raw_fd(), child.as_ptr(), 0o700) } != 0 {
            return Err(io::Error::last_os_error());
        }
        directory.sync_all()
    }

    pub(super) fn create_private_child(directory: &fs::File, prefix: &str) -> io::Result<PathBuf> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let pid = std::process::id();
        for attempt in 0..128u32 {
            let candidate = OsString::from(format!(".jet-{prefix}-{pid}-{stamp}-{attempt}"));
            let candidate_name = name(&candidate)?;
            if unsafe { mkdirat(directory.as_raw_fd(), candidate_name.as_ptr(), 0o700) } == 0 {
                directory.sync_all()?;
                return Ok(PathBuf::from(candidate));
            }
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::AlreadyExists {
                continue;
            }
            return Err(error);
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a private authority directory",
        ))
    }

    pub(super) fn read_names(directory: &fs::File) -> io::Result<Vec<OsString>> {
        fs::read_dir(descriptor_path(directory))?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect()
    }

    pub(super) fn create_child_file(directory: &fs::File, child: &OsStr) -> io::Result<fs::File> {
        let child = name(child)?;
        let fd = unsafe {
            openat(
                directory.as_raw_fd(),
                child.as_ptr(),
                O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { fs::File::from_raw_fd(fd) })
    }

    pub(super) fn open_child_write(directory: &fs::File, child: &OsStr) -> io::Result<fs::File> {
        let child = name(child)?;
        let fd = unsafe {
            openat(
                directory.as_raw_fd(),
                child.as_ptr(),
                O_WRONLY | O_NOFOLLOW | O_CLOEXEC,
                0,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { fs::File::from_raw_fd(fd) })
    }
    pub(super) fn open_child_append(
        directory: &fs::File,
        child: &OsStr,
    ) -> io::Result<fs::File> {
        let child = name(child)?;
        let fd = unsafe {
            openat(
                directory.as_raw_fd(),
                child.as_ptr(),
                O_WRONLY | O_APPEND | O_NOFOLLOW | O_CLOEXEC,
                0,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { fs::File::from_raw_fd(fd) })
    }

    pub(super) fn create_child_append(
        directory: &fs::File,
        child: &OsStr,
    ) -> io::Result<fs::File> {
        let child = name(child)?;
        let fd = unsafe {
            openat(
                directory.as_raw_fd(),
                child.as_ptr(),
                O_WRONLY | O_APPEND | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { fs::File::from_raw_fd(fd) })
    }

    pub(super) fn lock_exclusive(directory: &fs::File) -> io::Result<()> {
        if unsafe { flock(directory.as_raw_fd(), LOCK_EX) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(super) fn unlock(directory: &fs::File) -> io::Result<()> {
        if unsafe { flock(directory.as_raw_fd(), LOCK_UN) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(super) fn unlink_directory(directory: &fs::File, child: &OsStr) -> io::Result<()> {
        unlink(directory, child, AT_REMOVEDIR)
    }

    pub(super) fn unlink_any(directory: &fs::File, child: &OsStr) -> io::Result<()> {
        unlink(directory, child, 0)
    }

    fn unlink(directory: &fs::File, child: &OsStr, flags: i32) -> io::Result<()> {
        let child = name(child)?;
        if unsafe { unlinkat(directory.as_raw_fd(), child.as_ptr(), flags) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(super) fn rename_no_replace(
        source_parent: &fs::File,
        source_name: &OsStr,
        destination_parent: &fs::File,
        destination_name: &OsStr,
    ) -> io::Result<()> {
        let source_name = name(source_name)?;
        let destination_name = name(destination_name)?;
        let result = unsafe {
            #[cfg(any(target_os = "linux", target_os = "android"))]
            {
                renameat2(
                    source_parent.as_raw_fd(),
                    source_name.as_ptr(),
                    destination_parent.as_raw_fd(),
                    destination_name.as_ptr(),
                    RENAME_NOREPLACE,
                )
            }
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            {
                renameatx_np(
                    source_parent.as_raw_fd(),
                    source_name.as_ptr(),
                    destination_parent.as_raw_fd(),
                    destination_name.as_ptr(),
                    RENAME_EXCL,
                )
            }
        };
        if result != 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
    pub(super) fn rename_replace(
        source_parent: &fs::File,
        source_name: &OsStr,
        destination_parent: &fs::File,
        destination_name: &OsStr,
    ) -> io::Result<()> {
        let source_name = name(source_name)?;
        let destination_name = name(destination_name)?;
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


    pub(super) fn duplicate_inheritable(file: &fs::File) -> io::Result<fs::File> {
        const F_DUPFD: i32 = 0;
        let fd = unsafe { fcntl(file.as_raw_fd(), F_DUPFD, 3) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { fs::File::from_raw_fd(fd) })
    }

    pub(super) fn descriptor_path(file: &fs::File) -> PathBuf {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        let prefix = "/proc/self/fd/";
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        let prefix = "/dev/fd/";
        PathBuf::from(format!("{prefix}{}", file.as_raw_fd()))
    }


    pub(super) fn link_file(
        source: &fs::File,
        destination: &fs::File,
        child: &OsStr,
    ) -> io::Result<bool> {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            let destination_name = name(child)?;
            let empty = CString::new("").expect("empty C string");
            if unsafe {
                linkat(
                    source.as_raw_fd(),
                    empty.as_ptr(),
                    destination.as_raw_fd(),
                    destination_name.as_ptr(),
                    AT_EMPTY_PATH,
                )
            } == 0
            {
                return Ok(true);
            }
            let error = io::Error::last_os_error();
            if matches!(
                error.raw_os_error(),
                Some(value) if value == libc_errno::EXDEV
                    || value == libc_errno::EPERM
                    || value == libc_errno::EOPNOTSUPP
            ) {
                return Ok(false);
            }
            return Err(error);
        }
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            let _ = (source, destination, child);
            Ok(false)
        }
    }

    mod libc_errno {
        pub(super) const EXDEV: i32 = 18;
        pub(super) const EPERM: i32 = 1;
        pub(super) const EOPNOTSUPP: i32 = 95;
    }
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios"
)))]
mod directory_platform {
    use super::*;

    fn unsupported() -> io::Error {
        unsupported_descriptor_error()
    }

    pub(super) fn open_path(_path: &Path) -> io::Result<fs::File> {
        Err(unsupported())
    }
    pub(super) fn create_path(_path: &Path) -> io::Result<fs::File> {
        Err(unsupported())
    }
    pub(super) fn open_child(
        _directory: &fs::File,
        _child: &OsStr,
        _want_directory: bool,
    ) -> io::Result<fs::File> {
        Err(unsupported())
    }
    pub(super) fn mkdir_child(_directory: &fs::File, _child: &OsStr) -> io::Result<()> {
        Err(unsupported())
    }
    pub(super) fn create_private_child(
        _directory: &fs::File,
        _prefix: &str,
    ) -> io::Result<PathBuf> {
        Err(unsupported())
    }
    pub(super) fn read_names(_directory: &fs::File) -> io::Result<Vec<OsString>> {
        Err(unsupported())
    }
    pub(super) fn create_child_file(
        _directory: &fs::File,
        _child: &OsStr,
    ) -> io::Result<fs::File> {
        Err(unsupported())
    }
    pub(super) fn open_child_write(
        _directory: &fs::File,
        _child: &OsStr,
    ) -> io::Result<fs::File> {
        Err(unsupported())
    }
    pub(super) fn open_child_append(
        _directory: &fs::File,
        _child: &OsStr,
    ) -> io::Result<fs::File> {
        Err(unsupported())
    }
    pub(super) fn create_child_append(
        _directory: &fs::File,
        _child: &OsStr,
    ) -> io::Result<fs::File> {
        Err(unsupported())
    }
    pub(super) fn lock_exclusive(_directory: &fs::File) -> io::Result<()> {
        Err(unsupported())
    }
    pub(super) fn unlock(_directory: &fs::File) -> io::Result<()> {
        Err(unsupported())
    }
    pub(super) fn unlink_directory(_directory: &fs::File, _child: &OsStr) -> io::Result<()> {
        Err(unsupported())
    }
    pub(super) fn unlink_any(_directory: &fs::File, _child: &OsStr) -> io::Result<()> {
        Err(unsupported())
    }
    pub(super) fn rename_no_replace(
        _source_parent: &fs::File,
        _source_name: &OsStr,
        _destination_parent: &fs::File,
        _destination_name: &OsStr,
    ) -> io::Result<()> {
        Err(unsupported())
    }
    pub(super) fn link_file(
        _source: &fs::File,
        _destination: &fs::File,
        _name: &OsStr,
    ) -> io::Result<bool> {
        Err(unsupported())
    }
    pub(super) fn rename_replace(
        _source_parent: &fs::File,
        _source_name: &OsStr,
        _destination_parent: &fs::File,
        _destination_name: &OsStr,
    ) -> io::Result<()> {
        Err(unsupported())
    }
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios"
)))]
fn unsupported_descriptor_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        format!("descriptor-relative authority is unavailable outside {DESCRIPTOR_PLATFORMS}"),
    )
}

fn is_no_follow_error(error: &io::Error) -> bool {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        return error.raw_os_error() == Some(40);
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        return error.raw_os_error() == Some(62);
    }
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    )))]
    {
        let _ = error;
        false
    }
}

fn file_nlink(metadata: &fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        return metadata.nlink();
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        1
    }
}

fn same_store_file_identity(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        return left.dev() == right.dev()
            && left.ino() == right.ino()
            && left.len() == right.len()
            && left.modified().ok() == right.modified().ok();
    }
    #[cfg(not(unix))]
    {
        left.file_type() == right.file_type()
            && left.len() == right.len()
            && left.modified().ok() == right.modified().ok()
    }
}
fn same_store_object_identity(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        return left.dev() == right.dev() && left.ino() == right.ino();
    }
    #[cfg(not(unix))]
    {
        left.file_type() == right.file_type()
    }
}


fn copy_open_file(source: &mut fs::File, destination: &mut fs::File) -> io::Result<()> {
    io::copy(source, destination)?;
    destination.sync_all()
}

fn ignored_tree_name(name: &str) -> bool {
    name.starts_with('.') || name == "build" || name == "target"
}

struct AuthorityTreeFile {
    relative: String,
    file: fs::File,
    metadata: fs::Metadata,
}

fn collect_authority_tree(
    directory: &DirectoryAuthority,
    relative: &Path,
    files: &mut Vec<AuthorityTreeFile>,
    depth: usize,
    reject_hardlinks: bool,
) -> io::Result<()> {
    if depth > crate::SHA256::MAX_TREE_DEPTH {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "package tree exceeds the maximum directory depth",
        ));
    }
    let names = directory.list_names()?;
    for name in &names {
        #[cfg(test)]
        before_store_child_open(directory.path(), name);
        let file = directory.open_child(name)?;
        let metadata = file.metadata()?;
        let name_str = name.to_str().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "package tree contains a non-UTF-8 name",
            )
        })?;
        let child_relative = relative.join(name);
        if metadata.is_dir() {
            if ignored_tree_name(name_str) {
                continue;
            }
            let child = DirectoryAuthority::from_opened(directory.path.join(name), file);
            collect_authority_tree(
                &child,
                &child_relative,
                files,
                depth + 1,
                reject_hardlinks,
            )?;
        } else if metadata.is_file() {
            if reject_hardlinks && file_nlink(&metadata) > 1 {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("refusing multiply linked source file `{}`", child_relative.display()),
                ));
            }
            if ignored_tree_name(name_str) {
                continue;
            }
            if files.len() >= crate::SHA256::MAX_TREE_FILES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "package tree contains too many files",
                ));
            }
            files.push(AuthorityTreeFile {
                relative: child_relative.to_string_lossy().replace('\\', "/"),
                file,
                metadata,
            });
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("package tree contains a special entry `{}`", child_relative.display()),
            ));
        }
    }
    let after = directory.list_names()?;
    if names != after {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("package source changed while inspecting `{}`", directory.path.display()),
        ));
    }
    Ok(())
}

pub(crate) fn hash_directory_authority(
    root: &DirectoryAuthority,
    reject_hardlinks: bool,
) -> io::Result<String> {
    let mut files = Vec::new();
    collect_authority_tree(root, Path::new(""), &mut files, 0, reject_hardlinks)?;
    files.sort_by(|left, right| left.relative.cmp(&right.relative));
    let mut hasher = StreamingSha256::new();
    let mut total_bytes = 0u64;
    for entry in files {
        hasher.update(entry.relative.as_bytes());
        hasher.update(&[0]);
        let length = entry.metadata.len();
        if length > crate::SHA256::MAX_TREE_FILE_BYTES
            || total_bytes.saturating_add(length) > crate::SHA256::MAX_TREE_TOTAL_BYTES
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "package tree exceeds the hashing size bound",
            ));
        }
        hasher.update(&length.to_be_bytes());
        let mut file = entry.file;
        let mut limited = (&mut file).take(length.saturating_add(1));
        let mut actual = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let count = limited.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            actual = actual.saturating_add(count as u64);
            if actual > length {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "package source file grew while hashing",
                ));
            }
            hasher.update(&buffer[..count]);
        }
        let after = file.metadata()?;
        if actual != length || !same_store_file_identity(&entry.metadata, &after) {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("package source file changed while hashing `{}`", entry.relative),
            ));
        }
        total_bytes = total_bytes.saturating_add(length);
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    Ok(format!("sha256-{hex}"))
}

pub(crate) fn copy_directory_authority(
    source: &DirectoryAuthority,
    destination: &DirectoryAuthority,
    reject_hardlinks: bool,
) -> io::Result<()> {
    let names = source.list_names()?;
    for name in &names {
        let mut source_file = source.open_child(name)?;
        let metadata = source_file.metadata()?;
        let name_str = name.to_str().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "package tree contains a non-UTF-8 name")
        })?;
        if metadata.is_dir() {
            if ignored_tree_name(name_str) {
                continue;
            }
            let child_source = DirectoryAuthority::from_opened(source.path.join(name), source_file);
            let child = destination.create_child_directory(name)?;
            copy_directory_authority(&child_source, &child, reject_hardlinks)?;
        } else if metadata.is_file() {
            if reject_hardlinks && file_nlink(&metadata) > 1 {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("refusing multiply linked source file `{}`", source.path.join(name).display()),
                ));
            }
            if ignored_tree_name(name_str) {
                continue;
            }
            let mut destination_file = destination.create_child_file(name)?;
            copy_open_file(&mut source_file, &mut destination_file)?;
            let after = source_file.metadata()?;
            if !same_store_file_identity(&metadata, &after) {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!("package source file changed while copying `{}`", source.path.join(name).display()),
                ));
            }
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("package tree contains a special entry `{}`", source.path.join(name).display()),
            ));
        }
    }
    if names != source.list_names()? {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("package source changed while copying `{}`", source.path.display()),
        ));
    }
    Ok(())
}

pub(crate) fn link_directory_authority(
    source: &DirectoryAuthority,
    destination: &DirectoryAuthority,
) -> io::Result<()> {
    let names = source.list_names()?;
    for name in &names {
        let mut source_file = source.open_child(name)?;
        let metadata = source_file.metadata()?;
        let name_str = name.to_str().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "store entry contains a non-UTF-8 name")
        })?;
        if metadata.is_dir() {
            if ignored_tree_name(name_str) {
                continue;
            }
            let child_source = DirectoryAuthority::from_opened(source.path.join(name), source_file);
            let child = destination.create_child_directory(name)?;
            link_directory_authority(&child_source, &child)?;
        } else if metadata.is_file() {
            if ignored_tree_name(name_str) {
                continue;
            }
            if !directory_platform::link_file(&source_file, &destination.handle_ref(), name)? {
                let mut destination_file = destination.create_child_file(name)?;
                copy_open_file(&mut source_file, &mut destination_file)?;
            }
            let after = source_file.metadata()?;
            if !same_store_file_identity(&metadata, &after) {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!("store source file changed while linking `{}`", source.path.join(name).display()),
                ));
            }
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("store entry contains a special entry `{}`", source.path.join(name).display()),
            ));
        }
    }
    if names != source.list_names()? {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("store entry changed while linking `{}`", source.path.display()),
        ));
    }
    Ok(())
}

struct SnapshotCleanup {
    parent: DirectoryAuthority,
    name: OsString,
}

pub(crate) fn snapshot_directory_authority(
    source: &DirectoryAuthority,
) -> Result<SourceSnapshot, Diagnostic> {
    let store = ensure_directory_authority(&store_dir())
        .map_err(|error| io_error("creating source snapshot store", &store_dir(), error))?;
    let snapshots = match store.open_child_directory(OsStr::new(".snapshots")) {
        Ok(directory) => directory,
        Err(error) if error.kind() == io::ErrorKind::NotFound => store
            .create_child_directory(OsStr::new(".snapshots"))
            .map_err(|error| io_error("creating source snapshot root", &store.path.join(".snapshots"), error))?,
        Err(error) => {
            return Err(io_error(
                "opening source snapshot root",
                &store.path.join(".snapshots"),
                error,
            ))
        }
    };
    let (name, stage) = snapshots
        .create_private_child("source")
        .map_err(|error| io_error("creating source snapshot", snapshots.path(), error))?;
    let cleanup = || {
        let _ = snapshots.remove_child_tree(&name);
    };
    if let Err(error) = copy_directory_authority(source, &stage, true) {
        cleanup();
        return Err(io_error("snapshotting package source", source.path(), error));
    }
    let content_hash = match hash_directory_authority(&stage, false) {
        Ok(hash) => hash,
        Err(error) => {
            cleanup();
            return Err(io_error("hashing source snapshot", stage.path(), error));
        }
    };
    let cleanup_parent = snapshots
        .try_clone()
        .map_err(|error| io_error("retaining source snapshot authority", snapshots.path(), error))?;
    Ok(SourceSnapshot {
        path: stage.path.clone(),
        cleanup: Some(SnapshotCleanup {
            parent: cleanup_parent,
            name,
        }),
        content_hash,
    })
}

// ──────────────────────────────────────────────
// Store location
// ──────────────────────────────────────────────

/// Returns `~/.jet/store`.
/// If `JET_PACKAGE_STORE_DIR` is set, that directory is used instead (for testing).
pub fn store_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("JET_PACKAGE_STORE_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".jet").join("store")
}

/// Store path for a package: `~/.jet/store/<name>-<version>-<fingerprint>/`.
pub fn store_path(name: &str, version: &str, fingerprint: &str) -> PathBuf {
    let fp = fingerprint.strip_prefix("sha256-").unwrap_or(fingerprint);
    store_dir().join(format!("{}-{}-{}", name, version, fp))
}

/// A private, immutable-by-construction source snapshot. Callers must keep
/// this value alive while reading the package; the original path is never
/// consulted after the snapshot has been made.
pub struct SourceSnapshot {
    path: PathBuf,
    cleanup: Option<SnapshotCleanup>,
    content_hash: String,
}

impl SourceSnapshot {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn content_hash(&self) -> &str {
        &self.content_hash
    }
}

impl Drop for SourceSnapshot {
    fn drop(&mut self) {
        if let Some(cleanup) = self.cleanup.take() {
            let _ = cleanup.parent.remove_child_tree(&cleanup.name);
        }
    }
}

/// Copy a source tree into an exclusive temporary directory and hash the
/// copied bytes. The returned hash describes the bytes callers must consume,
/// not a later re-read of the mutable source path.
pub fn snapshot_tree(source_dir: &Path) -> Result<SourceSnapshot, Diagnostic> {
    let source = open_directory_authority(source_dir)
        .map_err(|error| io_error("checking package source", source_dir, error))?;
    snapshot_directory_authority(&source)
}


// ──────────────────────────────────────────────
// Install / link into project
// ──────────────────────────────────────────────

/// Ensure a path dep's source is available in the store.
/// For path deps the store entry is the canonical source copy.
/// Returns `(store_path, content_hash)` — hash of the installed tree (D-CASTORE1=A).
pub fn ensure_path_dep(
    name: &str,
    version: &str,
    fingerprint: &str,
    source_dir: &Path,
) -> Result<(PathBuf, String), Diagnostic> {
    validate_store_component(name).map_err(|reason| store_path_diagnostic(name, &reason))?;
    validate_store_component(version).map_err(|reason| store_path_diagnostic(version, &reason))?;
    let fingerprint = fingerprint.strip_prefix("sha256-").unwrap_or(fingerprint);
    validate_store_component(fingerprint)
        .map_err(|reason| store_path_diagnostic(fingerprint, &reason))?;

    let source = open_directory_authority(source_dir)
        .map_err(|error| io_error("checking package source", source_dir, error))?;
    let dest = store_path(name, version, fingerprint);
    let parent = dest
        .parent()
        .ok_or_else(|| io_error("locating store entry parent", &dest, invalid_path_error()))?;
    let parent_authority = ensure_directory_authority(parent)
        .map_err(|error| io_error("creating store entry parent", parent, error))?;
    let dest_name = dest
        .file_name()
        .ok_or_else(|| io_error("locating store entry", &dest, invalid_path_error()))?;

    match parent_authority.open_child_directory(dest_name) {
        Ok(existing) => {
            let hash = hash_directory_authority(&existing, false)
                .map_err(|error| io_error("hashing store entry", &dest, error))?;
            return Ok((dest, hash));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error("checking store entry", &dest, error)),
    }

    let (staging_name, staging) = parent_authority
        .create_private_child("entry")
        .map_err(|error| io_error("creating store staging entry", parent, error))?;
    let result = (|| {
        copy_directory_authority(&source, &staging, true)
            .map_err(|error| io_error("copying to store", &dest, error))?;
        let hash = hash_directory_authority(&staging, false)
            .map_err(|error| io_error("hashing store entry", &dest, error))?;
        match publish_directory(&parent_authority, &staging_name, &parent_authority, dest_name) {
            Ok(()) => Ok(hash),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let winner = parent_authority
                    .open_child_directory(dest_name)
                    .map_err(|error| io_error("opening concurrent store entry", &dest, error))?;
                let winner_hash = hash_directory_authority(&winner, false)
                    .map_err(|error| io_error("hashing concurrent store entry", &dest, error))?;
                if winner_hash == hash {
                    Ok(winner_hash)
                } else {
                    Err(io_error(
                        "publishing store entry",
                        &dest,
                        io::Error::new(
                            io::ErrorKind::AlreadyExists,
                            "concurrent store entry has different contents",
                        ),
                    ))
                }
            }
            Err(error) => Err(io_error("publishing store entry", &dest, error)),
        }
    })();
    let _ = parent_authority.remove_child_tree(&staging_name);
    Ok((dest, result?))
}

/// D-CASTORE1=A: verify a store entry's content hash against the recorded lock value.
/// Returns E1204 on mismatch (tampered store) or if entry is missing.
pub fn verify_content_hash(
    pkg_name: &str,
    store_entry: &Path,
    expected_content_hash: &str,
) -> Result<(), Diagnostic> {
    let entry = match open_directory_authority(store_entry) {
        Ok(entry) => entry,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(Diagnostic::error(
                "E1204",
                format!("the store entry for `{}` is missing", pkg_name),
                "a package source tree must be present in the store".to_string(),
                "run `jet fetch` to re-download the package".to_string(),
                None,
            ))
        }
        Err(error) => return Err(io_error("checking store entry", store_entry, error)),
    };
    if expected_content_hash.is_empty() {
        return Err(missing_content_hash(pkg_name));
    }
    let actual = hash_directory_authority(&entry, false)
        .map_err(|error| io_error("hashing store entry", store_entry, error))?;
    if actual != expected_content_hash {
        return Err(Diagnostic::error(
            "E1204",
            format!("the store entry for `{}` has been modified", pkg_name),
            format!(
                "expected content hash `{}` but got `{}`",
                expected_content_hash, actual
            ),
            "delete the store entry and run `jet fetch` to re-install".to_string(),
            None,
        ));
    }
    Ok(())
}

/// Ensure a git dep is stored.
/// `git_dir` is the directory where the revision has been checked out.
pub fn ensure_git_dep(
    name: &str,
    version: &str,
    fingerprint: &str,
    git_dir: &Path,
) -> Result<(PathBuf, String), Diagnostic> {
    ensure_path_dep(name, version, fingerprint, git_dir)
}

/// Link a store entry into a project's local deps dir via hardlinks (or copy).
/// `link_root` is typically `<project>/.jet-build/deps/<name>/`.
pub fn link_into_project(store_entry: &Path, link_root: &Path) -> Result<(), Diagnostic> {
    let source = open_directory_authority(store_entry)
        .map_err(|error| io_error("checking store entry", store_entry, error))?;
    let expected = hash_directory_authority(&source, false)
        .map_err(|error| io_error("hashing store entry", store_entry, error))?;
    let parent = link_root
        .parent()
        .ok_or_else(|| io_error("locating dep link parent", link_root, invalid_path_error()))?;
    let parent_authority = ensure_directory_authority(parent)
        .map_err(|error| io_error("creating dep link parent", parent, error))?;
    let link_name = link_root
        .file_name()
        .ok_or_else(|| io_error("locating dep link", link_root, invalid_path_error()))?;

    match parent_authority.open_child_directory(link_name) {
        Ok(existing) => {
            let actual = hash_directory_authority(&existing, false)
                .map_err(|error| io_error("hashing dependency link", link_root, error))?;
            if actual != expected {
                return Err(io_error(
                    "checking dependency link",
                    link_root,
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "existing dependency link does not match its immutable store entry",
                    ),
                ));
            }
            return Ok(());
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error("checking dependency link", link_root, error)),
    }

    let (staging_name, staging) = parent_authority
        .create_private_child("dep-link")
        .map_err(|error| io_error("creating dep link staging dir", parent, error))?;
    let result = (|| {
        link_directory_authority(&source, &staging)
            .map_err(|error| io_error("linking dependency tree", link_root, error))?;
        let actual = hash_directory_authority(&staging, false)
            .map_err(|error| io_error("hashing staged dependency link", link_root, error))?;
        if actual != expected {
            return Err(io_error(
                "checking staged dependency link",
                link_root,
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "staged dependency link does not match its immutable store entry",
                ),
            ));
        }
        match publish_directory(&parent_authority, &staging_name, &parent_authority, link_name) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let winner = parent_authority
                    .open_child_directory(link_name)
                    .map_err(|error| io_error("opening concurrent dependency link", link_root, error))?;
                let winner_hash = hash_directory_authority(&winner, false)
                    .map_err(|error| io_error("hashing concurrent dependency link", link_root, error))?;
                if winner_hash == expected {
                    Ok(())
                } else {
                    Err(io_error(
                        "publishing dependency link",
                        link_root,
                        io::Error::new(
                            io::ErrorKind::AlreadyExists,
                            "concurrent dependency link has different contents",
                        ),
                    ))
                }
            }
            Err(error) => Err(io_error("publishing dependency link", link_root, error)),
        }
    })();
    let _ = parent_authority.remove_child_tree(&staging_name);
    result
}

/// Copy an immutable Hangar object into a project without creating hardlinks
/// outside the Hangar CAS. Hangar verification treats external hardlinks as a
/// mutation risk; registry dependencies therefore use this boundary while
/// legacy path/git stores retain `link_into_project`'s inode sharing.
pub fn copy_into_project(store_entry: &Path, project_root: &Path) -> Result<(), Diagnostic> {
    let source = open_directory_authority(store_entry)
        .map_err(|error| io_error("checking store entry", store_entry, error))?;
    let expected = hash_directory_authority(&source, false)
        .map_err(|error| io_error("hashing store entry", store_entry, error))?;
    let parent = project_root.parent().ok_or_else(|| {
        io_error(
            "locating dep copy parent",
            project_root,
            invalid_path_error(),
        )
    })?;
    let parent_authority = ensure_directory_authority(parent)
        .map_err(|error| io_error("creating dep copy parent", parent, error))?;
    let project_name = project_root
        .file_name()
        .ok_or_else(|| io_error("locating dep copy", project_root, invalid_path_error()))?;

    match parent_authority.open_child_directory(project_name) {
        Ok(existing) => {
            let actual = hash_directory_authority(&existing, false)
                .map_err(|error| io_error("hashing dep copy", project_root, error))?;
            if actual != expected {
                return Err(io_error(
                    "checking dep copy",
                    project_root,
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "existing dependency copy does not match its immutable store entry",
                    ),
                ));
            }
            return Ok(());
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error("checking dep copy", project_root, error)),
    }

    let (staging_name, staging) = parent_authority
        .create_private_child("dep-copy")
        .map_err(|error| io_error("creating dep copy staging dir", parent, error))?;
    let result = (|| {
        copy_directory_authority(&source, &staging, false)
            .map_err(|error| io_error("copying dep tree", project_root, error))?;
        let actual = hash_directory_authority(&staging, false)
            .map_err(|error| io_error("hashing staged dep copy", project_root, error))?;
        if actual != expected {
            return Err(io_error(
                "checking staged dep copy",
                project_root,
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "staged dependency copy does not match its immutable store entry",
                ),
            ));
        }
        match publish_directory(&parent_authority, &staging_name, &parent_authority, project_name) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let winner = parent_authority
                    .open_child_directory(project_name)
                    .map_err(|error| io_error("opening concurrent dep copy", project_root, error))?;
                let winner_hash = hash_directory_authority(&winner, false)
                    .map_err(|error| io_error("hashing concurrent dep copy", project_root, error))?;
                if winner_hash == expected {
                    Ok(())
                } else {
                    Err(io_error(
                        "publishing dep copy",
                        project_root,
                        io::Error::new(
                            io::ErrorKind::AlreadyExists,
                            "concurrent dependency copy has different contents",
                        ),
                    ))
                }
            }
            Err(error) => Err(io_error("publishing dep copy", project_root, error)),
        }
    })();
    let _ = parent_authority.remove_child_tree(&staging_name);
    result
}

/// Verify the content hash of a store entry matches expected. Returns E1204 on mismatch.
pub fn verify_entry(
    pkg_name: &str,
    store_entry: &Path,
    expected_tree_hash: &str,
) -> Result<(), Diagnostic> {
    let entry = match open_directory_authority(store_entry) {
        Ok(entry) => entry,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(Diagnostic::error(
                "E1204",
                format!("the store entry for `{}` is missing", pkg_name),
                "a package source tree must be present in the store before it can be used".to_string(),
                "run `jet fetch` to re-download the package".to_string(),
                None,
            ))
        }
        Err(error) => return Err(io_error("checking store entry", store_entry, error)),
    };
    if expected_tree_hash.is_empty() {
        return Err(missing_content_hash(pkg_name));
    }
    let actual = hash_directory_authority(&entry, false)
        .map_err(|error| io_error("hashing store entry", store_entry, error))?;
    if actual != expected_tree_hash {
        return Err(Diagnostic::error(
            "E1204",
            format!("the store entry for `{}` has been modified", pkg_name),
            format!(
                "the content hash of the stored source tree doesn't match the fingerprint in {}",
                Syntax::UNIFIED_LOCK_FILE
            ),
            "run `jet fetch` to re-download the package, or run `jetpack hangar verify` to check all entries"
                .to_string(),
            None,
        ));
    }
    Ok(())
}


// ──────────────────────────────────────────────
// `jetpack hangar verify`
// ──────────────────────────────────────────────

/// Re-verify all store entries against their expected tree hashes.
/// The `entries` map is `(name, store_path, expected_tree_hash)`.
pub fn verify_all(entries: &[(&str, &Path, &str)]) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for (name, path, expected) in entries {
        if let Err(d) = verify_entry(name, path, expected) {
            diags.push(d);
        }
    }
    diags
}

// ──────────────────────────────────────────────
// `jet clean` — remove unreferenced store entries (stub)
// ──────────────────────────────────────────────

/// List all store entries.
pub fn list_entries() -> Vec<PathBuf> {
    let dir = store_dir();
    let Ok(authority) = open_directory_authority(&dir) else {
        return Vec::new();
    };
    let Ok(names) = authority.list_names() else {
        return Vec::new();
    };
    names
        .into_iter()
        .filter(|name| name != OsStr::new(".snapshots"))
        .filter_map(|name| {
            authority
                .open_child_directory(&name)
                .ok()
                .map(|_| dir.join(name))
        })
        .collect()
}

/// Remove store entries whose fingerprints are not in the provided set.
/// The set contains the fingerprint suffix (without `sha256-` prefix) of in-use entries.
pub fn gc(in_use_fingerprints: &std::collections::HashSet<String>) -> Vec<PathBuf> {
    let dir = store_dir();
    let Ok(authority) = open_directory_authority(&dir) else {
        return Vec::new();
    };
    let Ok(names) = authority.list_names() else {
        return Vec::new();
    };
    let mut removed = Vec::new();
    for name in names {
        if name == OsStr::new(".snapshots") {
            continue;
        }
        let Ok(entry) = authority.open_child_directory(&name) else {
            continue;
        };
        let dir_name = name.to_string_lossy();
        let fp = dir_name.rsplitn(2, '-').next().unwrap_or("");
        if !in_use_fingerprints.contains(fp)
            && authority.remove_child_tree(&name).is_ok()
        {
            let _ = entry;
            removed.push(dir.join(name));
        }
    }
    removed
}

fn validate_store_component(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.contains(['/', '\\', ':'])
        || value.chars().any(char::is_control)
        || !matches!(
            Path::new(value).components().next(),
            Some(std::path::Component::Normal(_))
        )
        || Path::new(value).components().nth(1).is_some()
    {
        return Err("the value must be one safe path component".to_string());
    }
    Ok(())
}

fn store_path_diagnostic(value: &str, reason: &str) -> Diagnostic {
    Diagnostic::error(
        "E1206",
        format!("package store path component `{value}` is not allowed"),
        reason.to_string(),
        "use a package name, version, and fingerprint without path separators".to_string(),
        None,
    )
}


fn io_error(action: &str, path: &Path, err: std::io::Error) -> Diagnostic {
    Diagnostic::error(
        "E1206",
        format!("I/O error while {} at `{}`", action, path.display()),
        "a filesystem operation failed during package installation".to_string(),
        format!("check permissions and disk space: {}", err),
        None,
    )
}


fn invalid_path_error() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "store entry has no parent",
    )
}

fn missing_content_hash(pkg_name: &str) -> Diagnostic {
    Diagnostic::error(
        "E1204",
        format!("the store entry for `{pkg_name}` has no content hash"),
        "locked package sources must carry a content hash before they are used".to_string(),
        "run `jet fetch` to recreate the lock with verified content hashes".to_string(),
        None,
    )
}

// ──────────────────────────────────────────────
// D-PURE3=B (E2-M16): signed cache + generation tracking
// ──────────────────────────────────────────────

/// Generation log path: `~/.jet/store/generations.log`.
/// Each line: `<generation_number> <timestamp_utc> <store_entry_list_hash>`.
pub fn generations_log_path() -> PathBuf {
    store_dir().join("generations.log")
}

/// One generation record.
#[derive(Debug, Clone)]
pub struct Generation {
    pub number: u64,
    pub timestamp: String,
    pub entry_hash: String,
}

/// Record the current store state as a new generation.
/// Returns the new generation number.
pub fn record_generation() -> u64 {
    let entries = list_entries();
    let mut entry_names: Vec<String> = entries
        .iter()
        .map(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string()
        })
        .collect();
    entry_names.sort();
    let entry_list = entry_names.join(",");
    let entry_hash = {
        // Simple hash: sha256 of the sorted entry list.
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        entry_list.hash(&mut h);
        format!("{:016x}", h.finish())
    };

    let log_path = generations_log_path();
    // Read existing generations.
    let existing_raw = fs::read_to_string(&log_path).unwrap_or_default();
    let next_gen = existing_raw
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .filter_map(|n| n.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
        + 1;

    // Append the new generation.
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string());
    let line = format!("{} {} {}\n", next_gen, ts, entry_hash);
    let _ = fs::create_dir_all(store_dir());
    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .and_then(|mut f| {
            use std::io::Write;
            f.write_all(line.as_bytes())
        });

    next_gen
}

/// Read all recorded generations from the log.
pub fn list_generations() -> Vec<Generation> {
    let log_path = generations_log_path();
    let raw = fs::read_to_string(&log_path).unwrap_or_default();
    raw.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let number = parts.next()?.parse::<u64>().ok()?;
            let timestamp = parts.next()?.to_string();
            let entry_hash = parts.next().unwrap_or("").to_string();
            Some(Generation {
                number,
                timestamp,
                entry_hash,
            })
        })
        .collect()
}

/// Roll back to a prior generation number.
/// In this implementation, rollback writes a new generation entry that
/// records the intent (store entries cannot be erased — the store is
/// append-only; a rollback marks which generation is "current").
/// Returns the rolled-back-to generation number, or an error string.
pub fn rollback_to(gen_number: u64) -> Result<Generation, String> {
    let gens = list_generations();
    let target = gens
        .iter()
        .find(|g| g.number == gen_number)
        .ok_or_else(|| format!("generation {} does not exist", gen_number))?
        .clone();
    // Write a "rollback" marker generation pointing at the target hash.
    let log_path = generations_log_path();
    let next_gen = gens.iter().map(|g| g.number).max().unwrap_or(0) + 1;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string());
    let line = format!("{} {} rollback-to-{}\n", next_gen, ts, gen_number);
    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .and_then(|mut f| {
            use std::io::Write;
            f.write_all(line.as_bytes())
        });
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn authority_test_root(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test clock must be after the Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "jet-store-{name}-{}-{stamp}",
            std::process::id()
        ))
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    #[test]
    fn authority_replace_rejects_a_changed_pending_target() {
        let root = authority_test_root("authority-change");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("package.jet");
        fs::write(&path, b"approved\n").unwrap();
        let snapshot = read_authority_file(&path).unwrap();

        fs::write(&path, b"attacker\n").unwrap();
        let error = replace_authority_file(&snapshot, b"published\n").unwrap_err();

        assert!(
            error
                .to_string()
                .contains("authority target contents changed while approval was pending"),
            "unexpected authority replacement error: {error}"
        );
        assert_eq!(fs::read(&path).unwrap(), b"attacker\n");
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    #[test]
    fn authority_replace_publishes_only_the_snapshotted_target() {
        let root = authority_test_root("authority-replace");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("package.jet");
        fs::write(&path, b"approved\n").unwrap();
        let snapshot = read_authority_file(&path).unwrap();

        replace_authority_file(&snapshot, b"published\n").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"published\n");
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    #[test]
    fn authority_file_operations_reject_symlink_targets() {
        use std::os::unix::fs::symlink;

        let root = authority_test_root("authority-symlink");
        fs::create_dir_all(&root).unwrap();
        let victim = root.join("victim");
        let path = root.join("authority.jsonl");
        fs::write(&victim, b"victim\n").unwrap();
        symlink(&victim, &path).unwrap();

        assert!(read_authority_file(&path).is_err());
        assert!(append_authority_file(&path, b"grant\n").is_err());
        assert_eq!(fs::read(&victim).unwrap(), b"victim\n");
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    #[test]
    fn held_store_authority_survives_active_ancestor_swap() {
        use std::io::Read as _;
        use std::os::unix::fs::symlink;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{Arc, Barrier};

        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("test clock must be after the Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "jet-store-held-root-{}-{stamp}",
            std::process::id()
        ));
        let held = root.with_file_name(format!(
            "jet-store-held-root-renamed-{}-{stamp}",
            std::process::id()
        ));
        let outside = root.with_file_name(format!(
            "jet-store-held-root-outside-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(root.join("trusted.jet"), b"trusted bytes\n").unwrap();
        fs::write(outside.join("evil.jet"), b"evil bytes\n").unwrap();

        let authority = DirectoryAuthority::open(&root).expect("source authority must open");
        let reached = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let fired = Arc::new(AtomicBool::new(false));
        let hook_root = root.clone();
        let hook_reached = Arc::clone(&reached);
        let hook_release = Arc::clone(&release);
        let hook_fired = Arc::clone(&fired);
        let _hook = install_store_walk_hook(Arc::new(move |directory, name| {
            if directory == hook_root.as_path()
                && name == OsStr::new("trusted.jet")
                && !hook_fired.swap(true, Ordering::AcqRel)
            {
                hook_reached.wait();
                hook_release.wait();
            }
        }));

        let attacker_root = root.clone();
        let attacker_held = held.clone();
        let attacker_outside = outside.clone();
        let attacker_reached = Arc::clone(&reached);
        let attacker_release = Arc::clone(&release);
        let attacker = std::thread::spawn(move || {
            attacker_reached.wait();
            fs::rename(&attacker_root, &attacker_held).expect("source root must be movable");
            symlink(&attacker_outside, &attacker_root).expect("hostile root link must be made");
            attacker_release.wait();
        });

        let mut files = Vec::new();
        let walk = collect_authority_tree(&authority, Path::new(""), &mut files, 0, true);
        attacker.join().expect("ancestor swap thread must finish");
        drop(_hook);

        fs::remove_file(&root).expect("hostile root link must be removable");
        fs::rename(&held, &root).expect("held source root must be restored");

        walk.expect("held authority must retain the verified root");
        assert_eq!(files.len(), 1, "walk must not enter the hostile root");
        assert_eq!(files[0].relative, "trusted.jet");
        let mut bytes = Vec::new();
        files[0]
            .file
            .read_to_end(&mut bytes)
            .expect("verified file must remain readable");
        assert_eq!(bytes, b"trusted bytes\n");
        assert!(root.is_dir(), "the original root must be restored");

        fs::remove_dir_all(&root).unwrap();
        fs::remove_dir_all(&outside).unwrap();
    }
}
