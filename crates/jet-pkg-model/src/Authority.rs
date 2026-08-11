//! Checked filesystem authority for package and workspace metadata.
//!
//! Every authority-sensitive reader goes through this module.  The resolver
//! opens the root once, walks descendants without following links, keeps the
//! opened object and its identity in the returned snapshot, and checks the
//! pathname again before a caller uses the snapshot.

use crate::Package::PackageFacts;
use crate::Diagnostics::Diagnostic;
use crate::Syntax;
use std::fs::{self, File, Metadata};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

const KIND_FILE: &str = "regular file";
const KIND_DIRECTORY: &str = "directory";

/// The kind proven by a checked open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorityKind {
    File,
    Directory,
}

/// Stable identity of the object that was opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIdentity {
    pub kind: AuthorityKind,
    pub length: u64,
    pub modified_ns: Option<u128>,
    pub device: Option<u64>,
    pub inode: Option<u64>,
}

impl FileIdentity {
    fn from_metadata(metadata: &Metadata, kind: AuthorityKind) -> Self {
        let modified_ns = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos());
        #[cfg(unix)]
        let (device, inode) = {
            use std::os::unix::fs::MetadataExt;
            (Some(metadata.dev()), Some(metadata.ino()))
        };
        #[cfg(not(unix))]
        let (device, inode) = (None, None);
        Self {
            kind,
            length: metadata.len(),
            modified_ns,
            device,
            inode,
        }
    }
}

/// One no-follow opened file and the bytes read from that same handle.
#[derive(Debug, Clone)]
pub struct CheckedFile {
    pub path: PathBuf,
    pub relative: PathBuf,
    pub identity: FileIdentity,
    pub handle: Arc<File>,
    pub bytes: Vec<u8>,
}

impl CheckedFile {
    /// Decode the bytes that were read from the opened object.
    pub fn text(&self) -> Result<String, AuthorityError> {
        String::from_utf8(self.bytes.clone()).map_err(|error| AuthorityError::Invalid {
            path: self.path.clone(),
            detail: format!("authority file is not valid UTF-8: {error}"),
        })
    }
}

/// One no-follow opened directory.
#[derive(Debug, Clone)]
pub struct CheckedDirectory {
    pub path: PathBuf,
    pub relative: PathBuf,
    pub identity: FileIdentity,
    pub handle: Arc<File>,
}

/// A parsed canonical `package.jet` plus the opened manifest snapshot.
#[derive(Debug, Clone)]
pub struct CheckedManifest {
    pub file: CheckedFile,
    pub facts: PackageFacts,
}

/// A checked member directory and its canonical manifest.
#[derive(Debug, Clone)]
pub struct CheckedMember {
    pub directory: CheckedDirectory,
    pub manifest: CheckedManifest,
}

/// A member snapshot after checked Config composition and member validation.
#[derive(Debug, Clone)]
pub struct CheckedPackage {
    pub member: CheckedMember,
    pub facts: PackageFacts,
}

/// Failure from the shared authority resolver.
#[derive(Debug, Clone)]
pub enum AuthorityError {
    Missing(PathBuf),
    Io {
        path: PathBuf,
        operation: &'static str,
        detail: String,
    },
    Symlink(PathBuf),
    WrongKind {
        path: PathBuf,
        expected: &'static str,
        actual: &'static str,
    },
    Escapes(PathBuf),
    AmbiguousManifest(PathBuf),
    RetiredManifest(PathBuf),
    WorkspaceAmbiguous(Vec<PathBuf>),
    WorkspaceNoModule,
    Invalid { path: PathBuf, detail: String },
    Changed(PathBuf),
    Unsupported(String),
}

impl AuthorityError {
    pub fn is_missing(&self) -> bool {
        matches!(self, Self::Missing(_))
    }

    pub fn diagnostic(&self) -> Diagnostic {
        match self {
            Self::AmbiguousManifest(_path) => Diagnostic::error(
                "E1206",
                "the package has two manifest roots".to_string(),
                format!(
                    "`{}` and `{}` both exist; choosing one would make package identity ambiguous",
                    Syntax::PACKAGE_FILE,
                    Syntax::PAYLOAD_FILE,
                ),
                format!(
                    "remove `{}` or migrate it into `{}`",
                    Syntax::PAYLOAD_FILE,
                    Syntax::PACKAGE_FILE,
                ),
                None,
            ),
            Self::RetiredManifest(path) => Diagnostic::error(
                "E1226",
                format!("retired manifest `{}` is not a Package root", path.display()),
                format!(
                    "`{}` is retired; Package identity is read only from `{}`",
                    Syntax::PAYLOAD_FILE,
                    Syntax::PACKAGE_FILE,
                ),
                format!("rename `{}` to `{}`", Syntax::PAYLOAD_FILE, Syntax::PACKAGE_FILE),
                None,
            ),
            Self::WorkspaceAmbiguous(paths) => {
                let refs = paths.iter().map(PathBuf::as_path).collect::<Vec<_>>();
                crate::WorkspacePlan::e1239_ambiguous_workspace(&refs)
            }
            Self::WorkspaceNoModule => crate::WorkspacePlan::e0995_no_workspace_module(),
            Self::Missing(path) => Diagnostic::error(
                "E1334",
                format!("authority file `{}` is missing", path.display()),
                "authority metadata must exist before it can be used".to_string(),
                "restore the required metadata and try again".to_string(),
                None,
            ),
            Self::Symlink(path) => Diagnostic::error(
                "E1334",
                format!("authority path `{}` is a symlink", path.display()),
                "authority metadata is opened without following links".to_string(),
                "replace the symlink with the expected regular file or directory".to_string(),
                None,
            ),
            Self::WrongKind {
                path,
                expected,
                actual,
            } => Diagnostic::error(
                "E1334",
                format!("authority path `{}` is not a {expected}", path.display()),
                format!("the opened object is a {actual}, not the required {expected}"),
                format!("replace `{}` with a {expected}", path.display()),
                None,
            ),
            Self::Escapes(path) => Diagnostic::error(
                "E1322",
                format!("authority path `{}` escapes its root", path.display()),
                "authority identity is physical and cannot cross the checked root".to_string(),
                "use a relative path below the authority root".to_string(),
                None,
            ),
            Self::Invalid { path, detail } => Diagnostic::error(
                "E1334",
                format!("authority metadata `{}` is invalid", path.display()),
                detail.clone(),
                "fix the metadata fields and try again".to_string(),
                None,
            ),
            Self::Changed(path) => Diagnostic::error(
                "E1334",
                format!("authority object `{}` changed during resolution", path.display()),
                "the opened authority snapshot no longer matches the object at its path".to_string(),
                "restore the authority input and retry the operation".to_string(),
                None,
            ),
            Self::Io {
                path,
                operation,
                detail,
            } => Diagnostic::error(
                "E1334",
                format!("couldn't {operation} authority `{}`", path.display()),
                detail.clone(),
                "restore access to the authority metadata and try again".to_string(),
                None,
            ),
            Self::Unsupported(detail) => Diagnostic::error(
                "E1334",
                "authority resolution is unavailable on this platform".to_string(),
                detail.clone(),
                "use a platform with descriptor-relative no-follow filesystem access".to_string(),
                None,
            ),
        }
    }

    pub fn workspace_diagnostic(&self) -> Diagnostic {
        match self {
            Self::WorkspaceAmbiguous(paths) => {
                let refs = paths.iter().map(PathBuf::as_path).collect::<Vec<_>>();
                crate::WorkspacePlan::e1239_ambiguous_workspace(&refs)
            }
            Self::WorkspaceNoModule => crate::WorkspacePlan::e0995_no_workspace_module(),
            other => Diagnostic::error(
                "E1239",
                "couldn't inspect workspace sources".to_string(),
                other.to_string(),
                "restore the workspace metadata and try again".to_string(),
                None,
            ),
        }
    }
}

impl std::fmt::Display for AuthorityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing(path) => write!(f, "authority path `{}` is missing", path.display()),
            Self::Io {
                path,
                operation,
                detail,
            } => write!(f, "couldn't {operation} `{}`: {detail}", path.display()),
            Self::Symlink(path) => write!(f, "authority path `{}` is a symlink", path.display()),
            Self::WrongKind { path, expected, actual } => write!(
                f,
                "authority path `{}` is a {actual}, expected a {expected}",
                path.display()
            ),
            Self::Escapes(path) => write!(f, "authority path `{}` escapes its root", path.display()),
            Self::AmbiguousManifest(path) => write!(
                f,
                "both `{}` and `{}` exist in `{}`",
                Syntax::PACKAGE_FILE,
                Syntax::PAYLOAD_FILE,
                path.display()
            ),
            Self::RetiredManifest(path) => {
                write!(f, "retired manifest `{}` is not accepted", path.display())
            }
            Self::WorkspaceAmbiguous(paths) => write!(
                f,
                "workspace authority is declared in {} files",
                paths.len()
            ),
            Self::WorkspaceNoModule => f.write_str("canonical workspace source has no workspace module"),
            Self::Invalid { path, detail } => write!(f, "invalid authority `{}`: {detail}", path.display()),
            Self::Changed(path) => write!(f, "authority object `{}` changed during resolution", path.display()),
            Self::Unsupported(detail) => f.write_str(detail),
        }
    }
}

impl std::error::Error for AuthorityError {}

/// One checked authority root.  Descendant paths are opened relative to the
/// held root descriptor on supported platforms.
#[derive(Debug, Clone)]
pub struct AuthorityResolver {
    root: PathBuf,
    root_identity: FileIdentity,
    root_handle: Arc<File>,
}

impl AuthorityResolver {
    /// Open and pin a regular authority root directory.
    pub fn open(root: &Path) -> Result<Self, AuthorityError> {
        let metadata = fs::symlink_metadata(root).map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                AuthorityError::Missing(root.to_path_buf())
            } else {
                AuthorityError::Io {
                    path: root.to_path_buf(),
                    operation: "inspect",
                    detail: error.to_string(),
                }
            }
        })?;
        if metadata.file_type().is_symlink() {
            return Err(AuthorityError::Symlink(root.to_path_buf()));
        }
        if !metadata.is_dir() {
            return Err(AuthorityError::WrongKind {
                path: root.to_path_buf(),
                expected: KIND_DIRECTORY,
                actual: kind_name(&metadata),
            });
        }
        let expected_root_identity = FileIdentity::from_metadata(&metadata, AuthorityKind::Directory);
        let canonical = fs::canonicalize(root).map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                AuthorityError::Missing(root.to_path_buf())
            } else {
                AuthorityError::Io {
                    path: root.to_path_buf(),
                    operation: "resolve",
                    detail: error.to_string(),
                }
            }
        })?;
        let final_metadata = fs::symlink_metadata(root).map_err(|error| AuthorityError::Io {
            path: root.to_path_buf(),
            operation: "revalidate",
            detail: error.to_string(),
        })?;
        if final_metadata.file_type().is_symlink() {
            return Err(AuthorityError::Symlink(root.to_path_buf()));
        }
        if !final_metadata.is_dir()
            || FileIdentity::from_metadata(&final_metadata, AuthorityKind::Directory)
                != expected_root_identity
        {
            return Err(AuthorityError::Changed(root.to_path_buf()));
        }
        let handle = platform::open_root(&canonical).map_err(|error| {
            Self::map_io(&canonical, error, true)
        })?;
        let metadata = handle.metadata().map_err(|error| AuthorityError::Io {
            path: canonical.clone(),
            operation: "inspect",
            detail: error.to_string(),
        })?;
        if !metadata.is_dir() {
            return Err(AuthorityError::WrongKind {
                path: canonical,
                expected: KIND_DIRECTORY,
                actual: KIND_FILE,
            });
        }
        let root_identity = FileIdentity::from_metadata(&metadata, AuthorityKind::Directory);
        if root_identity != expected_root_identity {
            return Err(AuthorityError::Changed(root.to_path_buf()));
        }
        Ok(Self {
            root: canonical,
            root_identity,
            root_handle: Arc::new(handle),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Open one regular file below the pinned root and read its bytes from the
    /// same opened object.
    pub fn checked_file(&self, path: &Path) -> Result<CheckedFile, AuthorityError> {
        let relative = self.relative_path(path)?;
        let full = self.root.join(&relative);
        let handle = platform::open_relative(&self.root_handle, &relative, false)
            .map_err(|error| Self::map_io(&full, error, false))?;
        let metadata = handle.metadata().map_err(|error| AuthorityError::Io {
            path: full.clone(),
            operation: "inspect",
            detail: error.to_string(),
        })?;
        if !metadata.is_file() {
            return Err(AuthorityError::WrongKind {
                path: full,
                expected: KIND_FILE,
                actual: kind_name(&metadata),
            });
        }
        let identity = FileIdentity::from_metadata(&metadata, AuthorityKind::File);
        let handle = Arc::new(handle);
        let mut reader = handle.try_clone().map_err(|error| AuthorityError::Io {
            path: full.clone(),
            operation: "clone",
            detail: error.to_string(),
        })?;
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .map_err(|error| AuthorityError::Io {
                path: full.clone(),
                operation: "read",
                detail: error.to_string(),
            })?;
        let checked = CheckedFile {
            path: full,
            relative,
            identity,
            handle,
            bytes,
        };
        self.revalidate_file(&checked)?;
        Ok(checked)
    }

    /// Open one directory below the pinned root without following links.
    pub fn checked_directory(&self, path: &Path) -> Result<CheckedDirectory, AuthorityError> {
        let relative = self.relative_path(path)?;
        let full = self.root.join(&relative);
        let handle = platform::open_relative(&self.root_handle, &relative, true)
            .map_err(|error| Self::map_io(&full, error, true))?;
        let metadata = handle.metadata().map_err(|error| AuthorityError::Io {
            path: full.clone(),
            operation: "inspect",
            detail: error.to_string(),
        })?;
        if !metadata.is_dir() {
            return Err(AuthorityError::WrongKind {
                path: full,
                expected: KIND_DIRECTORY,
                actual: kind_name(&metadata),
            });
        }
        let identity = FileIdentity::from_metadata(&metadata, AuthorityKind::Directory);
        let checked = CheckedDirectory {
            path: full,
            relative,
            identity,
            handle: Arc::new(handle),
        };
        self.revalidate_directory(&checked)?;
        Ok(checked)
    }

    /// Open and parse the only accepted manifest, `package.jet`.
    pub fn checked_manifest(&self, directory: &Path) -> Result<CheckedManifest, AuthorityError> {
        let directory = self.relative_path(directory)?;
        let canonical = self.probe_file(&directory.join(Syntax::PACKAGE_FILE))?;
        let retired = self.probe_file(&directory.join(Syntax::PAYLOAD_FILE))?;
        if canonical.is_some() && retired.is_some() {
            return Err(AuthorityError::AmbiguousManifest(self.root.join(&directory)));
        }
        let Some(file) = canonical else {
            if let Some(file) = retired {
                return Err(AuthorityError::RetiredManifest(file.path));
            }
            return Err(AuthorityError::Missing(
                self.root.join(directory).join(Syntax::PACKAGE_FILE),
            ));
        };
        let text = file.text()?;
        let facts = PackageFacts::parse_uncomposed(&text, file.path.display().to_string())
            .map_err(|error| AuthorityError::Invalid {
                path: file.path.clone(),
                detail: error.to_string(),
            })?;
        self.revalidate_file(&file)?;
        if self
            .probe_file(&directory.join(Syntax::PAYLOAD_FILE))?
            .is_some()
        {
            return Err(AuthorityError::AmbiguousManifest(self.root.join(&directory)));
        }
        self.revalidate_root()?;
        Ok(CheckedManifest { file, facts })
    }

    /// Open and parse one package member directory.
    pub fn checked_member(&self, path: &Path) -> Result<CheckedMember, AuthorityError> {
        let directory = self.checked_directory(path)?;
        let manifest = self.checked_manifest(&directory.relative)?;
        let member = CheckedMember {
            directory,
            manifest,
        };
        self.revalidate_member(&member)?;
        Ok(member)
    }

    /// Compose one checked Package without reopening its authority paths.
    pub fn checked_package(&self, path: &Path) -> Result<CheckedPackage, AuthorityError> {
        let member = self.checked_member(path)?;
        let mut facts = member.manifest.facts.clone();
        facts
            .compose_configs_checked(self)
            .map_err(|error| AuthorityError::Invalid {
                path: member.manifest.file.path.clone(),
                detail: error.to_string(),
            })?;
        facts
            .validate_defaults()
            .map_err(|error| AuthorityError::Invalid {
                path: member.manifest.file.path.clone(),
                detail: error.to_string(),
            })?;
        facts
            .validate_members_in_checked(self)
            .map_err(|error| AuthorityError::Invalid {
                path: member.manifest.file.path.clone(),
                detail: error.to_string(),
            })?;
        self.revalidate_member(&member)?;
        Ok(CheckedPackage { member, facts })
    }

    /// Discover immediate package member directories. Symlink entries are
    /// rejected, even when their target would otherwise be inside the root.
    pub fn discover_members(&self, path: &Path) -> Result<Vec<CheckedMember>, AuthorityError> {
        let scan = self.checked_directory(path)?;
        let mut entries = fs::read_dir(&scan.path).map_err(|error| AuthorityError::Io {
            path: scan.path.clone(),
            operation: "read",
            detail: error.to_string(),
        })?.collect::<Result<Vec<_>, _>>().map_err(|error| AuthorityError::Io {
            path: scan.path.clone(),
            operation: "inspect",
            detail: error.to_string(),
        })?;
        entries.sort_by_key(|entry| entry.file_name());
        let mut members = Vec::new();
        for entry in entries {
            let file_type = entry.file_type().map_err(|error| AuthorityError::Io {
                path: entry.path(),
                operation: "inspect",
                detail: error.to_string(),
            })?;
            if file_type.is_symlink() {
                return Err(AuthorityError::Symlink(entry.path()));
            }
            if !file_type.is_dir() {
                continue;
            }
            let relative = scan.relative.join(entry.file_name());
            match self.checked_member(&relative) {
                Ok(member) => {
                    self.revalidate_member(&member)?;
                    members.push(member);
                }
                Err(error) if error.is_missing() => {}
                Err(error) => return Err(error),
            }
        }
        self.revalidate_directory(&scan)?;
        Ok(members)
    }

    /// Discover regular files in one checked directory without following
    /// candidate links. Every returned file is opened and revalidated through
    /// this resolver before it leaves the authority seam.
    pub fn discover_files(
        &self,
        path: &Path,
        extension: Option<&str>,
    ) -> Result<Vec<CheckedFile>, AuthorityError> {
        let scan = self.checked_directory(path)?;
        let mut entries = fs::read_dir(&scan.path)
            .map_err(|error| AuthorityError::Io {
                path: scan.path.clone(),
                operation: "read",
                detail: error.to_string(),
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| AuthorityError::Io {
                path: scan.path.clone(),
                operation: "inspect",
                detail: error.to_string(),
            })?;
        entries.sort_by_key(|entry| entry.file_name());
        let mut files = Vec::new();
        for entry in entries {
            let name = entry.file_name();
            if extension.is_some_and(|extension| {
                Path::new(&name)
                    .extension()
                    .and_then(|extension| extension.to_str())
                    != Some(extension)
            }) {
                continue;
            }
            let file_type = entry.file_type().map_err(|error| AuthorityError::Io {
                path: entry.path(),
                operation: "inspect",
                detail: error.to_string(),
            })?;
            let relative = scan.relative.join(name);
            if file_type.is_symlink() {
                return Err(AuthorityError::Symlink(self.root.join(relative)));
            }
            if !file_type.is_file() {
                continue;
            }
            let file = self.checked_file(&relative)?;
            self.revalidate_file(&file)?;
            files.push(file);
        }
        self.revalidate_directory(&scan)?;
        Ok(files)
    }

    /// Discover source files below the pinned root. Directory traversal and
    /// every returned file stay inside the same descriptor-relative resolver.
    pub fn discover_source_files(&self) -> Result<Vec<CheckedFile>, AuthorityError> {
        let mut files = Vec::new();
        self.discover_source_files_from(Path::new("."), &mut files)?;
        files.sort_by(|left, right| left.relative.cmp(&right.relative));
        self.revalidate_root()?;
        Ok(files)
    }

    /// Hash every checked `.jet` file below one checked directory using the
    /// same path/content shape as the package tree identity.
    pub fn source_tree_hash(
        &self,
        directory: &CheckedDirectory,
    ) -> Result<String, AuthorityError> {
        self.revalidate_directory(directory)?;
        let mut files = Vec::new();
        self.discover_source_files_from(&directory.relative, &mut files)?;
        files.sort_by(|left, right| left.relative.cmp(&right.relative));
        let mut input = Vec::new();
        for file in files {
            let relative = if directory.relative.as_os_str().is_empty() {
                file.relative.clone()
            } else {
                file.relative
                    .strip_prefix(&directory.relative)
                    .map_err(|_| AuthorityError::Escapes(file.path.clone()))?
                    .to_path_buf()
            };
            let relative = relative.to_string_lossy().replace('\\', "/");
            input.extend_from_slice(relative.as_bytes());
            input.push(0);
            input.extend_from_slice(&(file.bytes.len() as u64).to_be_bytes());
            input.extend_from_slice(&file.bytes);
            self.revalidate_file(&file)?;
        }
        self.revalidate_directory(directory)?;
        Ok(format!(
            "sha256-{}",
            crate::SHA256::sha256_hex(&input)
        ))
    }

    /// Resolve workspace authority from checked top-level source snapshots.
    pub fn resolve_workspace_source(
        &self,
    ) -> Result<Option<crate::WorkspacePlan::WorkspaceSource>, AuthorityError> {
        self.revalidate_root()?;
        let mut entries = fs::read_dir(&self.root).map_err(|error| Self::map_io(&self.root, error, true))?
            .collect::<Result<Vec<_>, _>>().map_err(|error| AuthorityError::Io {
                path: self.root.clone(),
                operation: "inspect",
                detail: error.to_string(),
            })?;
        entries.sort_by_key(|entry| entry.file_name());

        let mut canonical = None;
        let mut authorities = Vec::new();
        let mut malformed_canonical = false;
        for entry in entries {
            let name = entry.file_name();
            if Path::new(&name).extension().and_then(|extension| extension.to_str())
                != Some(crate::Syntax::FILE_EXT)
            {
                continue;
            }
            let relative = PathBuf::from(&name);
            let file = self.checked_file(&relative)?;
            let source = file.text()?;
            if crate::WorkspacePlan::declares_workspace_module(&source) {
                let role = if name.to_str() == Some(crate::Syntax::WORKSPACE_FILE) {
                    crate::WorkspacePlan::WorkspaceSourceRole::Index
                } else {
                    crate::WorkspacePlan::WorkspaceSourceRole::Authority
                };
                let candidate = crate::WorkspacePlan::WorkspaceSource {
                    path: file.path.clone(),
                    source,
                    role,
                    checked: file,
                };
                self.revalidate_source(&candidate)?;
                if role == crate::WorkspacePlan::WorkspaceSourceRole::Index {
                    canonical = Some(candidate);
                } else {
                    authorities.push(candidate);
                }
            } else if name.to_str() == Some(crate::Syntax::WORKSPACE_FILE) {
                malformed_canonical = true;
            }
        }

        self.revalidate_root()?;

        if malformed_canonical && canonical.is_none() {
            return Err(AuthorityError::WorkspaceNoModule);
        }
        if let Some(canonical) = canonical {
            if authorities.is_empty() {
                return Ok(Some(canonical));
            }
            let mut paths = vec![canonical.path];
            paths.extend(authorities.iter().map(|source| source.path.clone()));
            return Err(AuthorityError::WorkspaceAmbiguous(paths));
        }
        match authorities.len() {
            0 => Ok(None),
            1 => Ok(authorities.pop()),
            _ => Err(AuthorityError::WorkspaceAmbiguous(
                authorities.into_iter().map(|source| source.path).collect(),
            )),
        }
    }

    /// Revalidate the root and one checked file before using its bytes.
    pub fn revalidate_file(&self, file: &CheckedFile) -> Result<(), AuthorityError> {
        self.revalidate_root()?;
        self.revalidate_path(&file.path, AuthorityKind::File, &file.identity)?;
        Ok(())
    }

    /// Revalidate the root and one checked directory before using it.
    pub fn revalidate_directory(
        &self,
        directory: &CheckedDirectory,
    ) -> Result<(), AuthorityError> {
        self.revalidate_root()?;
        self.revalidate_path(
            &directory.path,
            AuthorityKind::Directory,
            &directory.identity,
        )?;
        Ok(())
    }

    /// Revalidate a complete member snapshot before realizing it.
    pub fn revalidate_member(&self, member: &CheckedMember) -> Result<(), AuthorityError> {
        self.revalidate_directory(&member.directory)?;
        self.revalidate_file(&member.manifest.file)
    }

    /// Revalidate a workspace source snapshot before realizing it.
    pub fn revalidate_source(
        &self,
        source: &crate::WorkspacePlan::WorkspaceSource,
    ) -> Result<(), AuthorityError> {
        self.revalidate_file(&source.checked)
    }

    /// Return the checkout-relative physical identity for a checked path.
    pub fn relative_identity(
        &self,
        directory: &CheckedDirectory,
    ) -> Result<String, AuthorityError> {
        self.revalidate_directory(directory)?;
        let canonical = fs::canonicalize(&directory.path).map_err(|error| AuthorityError::Io {
            path: directory.path.clone(),
            operation: "resolve",
            detail: error.to_string(),
        })?;
        self.revalidate_directory(directory)?;
        if !canonical.starts_with(&self.root) {
            return Err(AuthorityError::Escapes(directory.path.clone()));
        }
        let relative = canonical
            .strip_prefix(&self.root)
            .map_err(|_| AuthorityError::Escapes(canonical))?
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        Ok(if relative.is_empty() {
            ".".to_string()
        } else {
            relative
        })
    }

    fn discover_source_files_from(
        &self,
        path: &Path,
        files: &mut Vec<CheckedFile>,
    ) -> Result<(), AuthorityError> {
        let scan = self.checked_directory(path)?;
        let mut entries = fs::read_dir(&scan.path)
            .map_err(|error| AuthorityError::Io {
                path: scan.path.clone(),
                operation: "read",
                detail: error.to_string(),
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| AuthorityError::Io {
                path: scan.path.clone(),
                operation: "inspect",
                detail: error.to_string(),
            })?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let name = entry.file_name();
            let name_text = name.to_string_lossy();
            if name_text.starts_with('.')
                || matches!(name_text.as_ref(), "target" | "build" | "node_modules")
            {
                continue;
            }
            let file_type = entry.file_type().map_err(|error| AuthorityError::Io {
                path: entry.path(),
                operation: "inspect",
                detail: error.to_string(),
            })?;
            let relative = scan.relative.join(&name);
            if file_type.is_symlink() {
                return Err(AuthorityError::Symlink(self.root.join(relative)));
            }
            if file_type.is_dir() {
                self.discover_source_files_from(&relative, files)?;
            } else if file_type.is_file()
                && Path::new(&name)
                    .extension()
                    .and_then(|extension| extension.to_str())
                    == Some(crate::Syntax::FILE_EXT)
            {
                let file = self.checked_file(&relative)?;
                self.revalidate_file(&file)?;
                files.push(file);
            }
        }
        self.revalidate_directory(&scan)
    }

    fn probe_file(&self, path: &Path) -> Result<Option<CheckedFile>, AuthorityError> {
        match self.checked_file(path) {
            Ok(file) => Ok(Some(file)),
            Err(error) if error.is_missing() => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn relative_path(&self, path: &Path) -> Result<PathBuf, AuthorityError> {
        let relative = if path.is_absolute() {
            path.strip_prefix(&self.root)
                .map_err(|_| AuthorityError::Escapes(path.to_path_buf()))?
                .to_path_buf()
        } else {
            path.to_path_buf()
        };
        let mut normalized = PathBuf::new();
        for component in relative.components() {
            match component {
                Component::Normal(name) => normalized.push(name),
                Component::CurDir => {}
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                    return Err(AuthorityError::Escapes(path.to_path_buf()))
                }
            }
        }
        Ok(normalized)
    }

    pub fn revalidate_root(&self) -> Result<(), AuthorityError> {
        self.revalidate_path(&self.root, AuthorityKind::Directory, &self.root_identity)
    }

    fn revalidate_path(
        &self,
        path: &Path,
        kind: AuthorityKind,
        expected: &FileIdentity,
    ) -> Result<(), AuthorityError> {
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                AuthorityError::Missing(path.to_path_buf())
            } else {
                AuthorityError::Io {
                    path: path.to_path_buf(),
                    operation: "revalidate",
                    detail: error.to_string(),
                }
            }
        })?;
        if metadata.file_type().is_symlink() {
            return Err(AuthorityError::Symlink(path.to_path_buf()));
        }
        let actual_kind = if metadata.is_dir() {
            AuthorityKind::Directory
        } else if metadata.is_file() {
            AuthorityKind::File
        } else {
            return Err(AuthorityError::WrongKind {
                path: path.to_path_buf(),
                expected: kind_name_for(kind),
                actual: kind_name(&metadata),
            });
        };
        if actual_kind != kind
            || FileIdentity::from_metadata(&metadata, actual_kind) != *expected
        {
            return Err(AuthorityError::Changed(path.to_path_buf()));
        }
        let canonical = fs::canonicalize(path).map_err(|error| AuthorityError::Io {
            path: path.to_path_buf(),
            operation: "revalidate",
            detail: error.to_string(),
        })?;
        if !canonical.starts_with(&self.root) {
            return Err(AuthorityError::Escapes(path.to_path_buf()));
        }
        let relative = path
            .strip_prefix(&self.root)
            .map_err(|_| AuthorityError::Escapes(path.to_path_buf()))?;
        let handle = platform::open_relative(
            &self.root_handle,
            relative,
            kind == AuthorityKind::Directory,
        )
        .map_err(|error| Self::map_io(path, error, kind == AuthorityKind::Directory))?;
        let opened = handle.metadata().map_err(|error| AuthorityError::Io {
            path: path.to_path_buf(),
            operation: "revalidate",
            detail: error.to_string(),
        })?;
        let opened_kind = if opened.is_dir() {
            AuthorityKind::Directory
        } else if opened.is_file() {
            AuthorityKind::File
        } else {
            return Err(AuthorityError::WrongKind {
                path: path.to_path_buf(),
                expected: kind_name_for(kind),
                actual: kind_name(&opened),
            });
        };
        if opened_kind != kind
            || FileIdentity::from_metadata(&opened, opened_kind) != *expected
        {
            return Err(AuthorityError::Changed(path.to_path_buf()));
        }
        Ok(())
    }

    fn map_io(path: &Path, error: io::Error, directory: bool) -> AuthorityError {
        if error.kind() == io::ErrorKind::NotFound {
            return AuthorityError::Missing(path.to_path_buf());
        }
        if is_symlink_error(&error) {
            return AuthorityError::Symlink(path.to_path_buf());
        }
        if directory && error.kind() == io::ErrorKind::NotADirectory {
            return AuthorityError::WrongKind {
                path: path.to_path_buf(),
                expected: KIND_DIRECTORY,
                actual: KIND_FILE,
            };
        }
        AuthorityError::Io {
            path: path.to_path_buf(),
            operation: if directory { "open directory" } else { "open file" },
            detail: error.to_string(),
        }
    }
}

fn kind_name(metadata: &Metadata) -> &'static str {
    if metadata.is_dir() {
        KIND_DIRECTORY
    } else if metadata.is_file() {
        KIND_FILE
    } else {
        "special file"
    }
}

fn kind_name_for(kind: AuthorityKind) -> &'static str {
    match kind {
        AuthorityKind::File => KIND_FILE,
        AuthorityKind::Directory => KIND_DIRECTORY,
    }
}

#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos", target_os = "ios"))]
mod platform {
    use super::*;
    use std::ffi::{c_char, CString};
    use std::fs::OpenOptions;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;

    const O_RDONLY: i32 = 0;
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

    pub(super) fn open_root(path: &Path) -> io::Result<File> {
        OpenOptions::new()
            .read(true)
            .custom_flags(O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
            .open(path)
    }

    pub(super) fn open_relative(root: &File, relative: &Path, directory: bool) -> io::Result<File> {
        let components = relative.components().collect::<Vec<_>>();
        if components.is_empty() {
            return root.try_clone();
        }
        let mut current = root.try_clone()?;
        for (index, component) in components.iter().enumerate() {
            let Component::Normal(name) = component else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "authority path has unsupported components",
                ));
            };
            let name = CString::new(name.as_bytes()).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "authority path contains NUL")
            })?;
            let last = index + 1 == components.len();
            let flags = if last && !directory {
                O_RDONLY | O_NOFOLLOW | O_CLOEXEC
            } else {
                O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC
            };
            let fd = unsafe { openat(current.as_raw_fd(), name.as_ptr(), flags, 0) };
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            let opened = unsafe { File::from_raw_fd(fd) };
            if last {
                return Ok(opened);
            }
            current = opened;
        }
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "authority path is empty",
        ))
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos", target_os = "ios")))]
mod platform {
    use super::*;

    pub(super) fn open_root(_path: &Path) -> io::Result<File> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "descriptor-relative no-follow authority is unavailable on this platform",
        ))
    }

    pub(super) fn open_relative(_root: &File, _path: &Path, _directory: bool) -> io::Result<File> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "descriptor-relative no-follow authority is unavailable on this platform",
        ))
    }
}

fn is_symlink_error(error: &io::Error) -> bool {
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
