//! Sealed Hangar verification manifests.
//!
//! A seal authenticates the content digest recorded at admission together
//! with the cheap filesystem identity used to decide whether that digest can
//! be reused. The tuple table is represented by its digest so the seal stays
//! small; rebuilding the table never opens a content file.

use super::OBJECTS_DIR;
use std::fs;
use std::io::{self, Write as _};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) const SEALS_DIR: &str = "seals";

const MAGIC: &str = "jet-hangar-seal-v1";
const SCHEMA: &str = "1";
const MAX_SEAL_BYTES: u64 = 512;
const PARTIAL_SUFFIX: &str = ".partial";

static SEAL_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
struct SealRecord {
    digest: String,
    root_kind: RootKind,
    tuple_count: usize,
    tuple_digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RootKind {
    Directory,
    File,
    Symlink,
}

impl RootKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Directory => "directory",
            Self::File => "file",
            Self::Symlink => "symlink",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "directory" => Some(Self::Directory),
            "file" => Some(Self::File),
            "symlink" => Some(Self::Symlink),
            _ => None,
        }
    }
}


/// Return the sealed digest when `path` is the direct canonical object named
/// by its digest and every recorded filesystem tuple still matches.
pub(crate) fn check(path: &Path, hangar: &Path) -> io::Result<Option<String>> {
    let Some(digest) = object_digest_for_path(path, hangar) else {
        return Ok(None);
    };
    let seal_dir = hangar.join(SEALS_DIR);
    let seal_path = seal_dir.join(&digest);
    let Some(record) = read_record(&seal_path)? else {
        return Ok(None);
    };
    if record.digest != digest {
        return Ok(None);
    }
    let Some(root_kind) = root_kind(path) else {
        return Ok(None);
    };
    if record.root_kind != root_kind {
        return Ok(None);
    }
    let (tuple_count, tuple_digest) = match tuple_table(path) {
        Ok(table) => table,
        Err(_) => return Ok(None),
    };
    if tuple_count == record.tuple_count && tuple_digest == record.tuple_digest {
        Ok(Some(record.digest))
    } else {
        Ok(None)
    }
}

/// Write or replace the seal for one canonical Hangar object. Callers hold
/// the Hangar lock; the temporary file and rename make the metadata atomic.
pub(crate) fn write(path: &Path, hangar: &Path, digest: &str) -> io::Result<()> {
    if object_digest_for_path(path, hangar).as_deref() != Some(digest) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot seal a non-canonical Hangar object",
        ));
    }
    let root_kind = root_kind(path).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "cannot seal an unsupported Hangar object root",
        )
    })?;
    let (tuple_count, tuple_digest) = tuple_table(path)?;
    let record = format!(
        "{MAGIC}\nschema={SCHEMA}\ndigest={digest}\nroot-kind={}\ntuples={tuple_count}:{tuple_digest}\n",
        root_kind.as_str()
    );
    let seals = hangar.join(SEALS_DIR);
    ensure_real_directory(&seals, "Hangar seal directory")?;
    let target = seals.join(digest);
    let temporary = seals.join(format!(
        ".{digest}-{}-{}{PARTIAL_SUFFIX}",
        std::process::id(),
        SEAL_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(record.as_bytes())?;
        file.sync_all()?;
        match fs::rename(&temporary, &target) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                #[cfg(windows)]
                {
                    fs::remove_file(&target)?;
                    fs::rename(&temporary, &target)
                }
                #[cfg(not(windows))]
                {
                    Err(error)
                }
            }
            Err(error) => Err(error),
        }
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    super::sync_store_directory(&seals)
}

/// Remove a seal when its canonical object is rolled back or quarantined.
pub(crate) fn remove(path: &Path, hangar: &Path) -> io::Result<()> {
    let Some(digest) = object_digest_for_path(path, hangar) else {
        return Ok(());
    };
    // A removed seal means the object is quarantined or replaced; drop its
    // process-local closure-member proof with it.
    super::Closure::invalidate_proven_member(hangar, &digest);
    let seal = hangar.join(SEALS_DIR).join(digest);
    match fs::symlink_metadata(&seal) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Hangar seal is not a regular file: {}", seal.display()),
            ))
        }
        Ok(_) => fs::remove_file(seal),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Sweep interrupted atomic seal writes without creating a missing seal dir.
pub(crate) fn recover_unlocked(hangar: &Path) -> io::Result<usize> {
    let seals = hangar.join(SEALS_DIR);
    let metadata = match fs::symlink_metadata(&seals) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Hangar seal directory is not a real directory; repair the path before recovery",
        ));
    }
    let mut swept = 0;
    for entry in fs::read_dir(&seals)? {
        let path = entry?.path();
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
        if !name.starts_with('.') || !name.ends_with(PARTIAL_SUFFIX) {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Hangar seal partial is not a regular file: {}", path.display()),
            ));
        }
        fs::remove_file(path)?;
        swept += 1;
    }
    Ok(swept)
}

/// Identify only `hangar/objects/<digest>`, including callers that use a
/// symlinked path to the Hangar root. The object itself is never canonicalized
/// because a symlink root is a valid admitted object shape.
pub(crate) fn object_digest_for_path(path: &Path, hangar: &Path) -> Option<String> {
    let objects = fs::canonicalize(hangar.join(OBJECTS_DIR)).ok()?;
    let parent = fs::canonicalize(path.parent()?).ok()?;
    if parent != objects {
        return None;
    }
    let name = path.file_name()?.to_str()?;
    valid_digest(name).then(|| name.to_string())
}

fn read_record(path: &Path) -> io::Result<Option<SealRecord>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > MAX_SEAL_BYTES {
        return Ok(None);
    }
    let bytes = fs::read(path)?;
    if bytes.len() as u64 > MAX_SEAL_BYTES {
        return Ok(None);
    }
    Ok(parse_record(std::str::from_utf8(&bytes).ok().unwrap_or_default()))
}

fn parse_record(text: &str) -> Option<SealRecord> {
    let mut lines = text.lines();
    if lines.next() != Some(MAGIC) || lines.next() != Some("schema=1") {
        return None;
    }
    let digest = lines.next()?.strip_prefix("digest=")?.to_string();
    if !valid_digest(&digest) {
        return None;
    }
    let root_kind = RootKind::parse(lines.next()?.strip_prefix("root-kind=")?)?;
    let tuples = lines.next()?.strip_prefix("tuples=")?;
    if lines.next().is_some() {
        return None;
    }
    let (count, tuple_digest) = tuples.split_once(':')?;
    let tuple_count = count.parse().ok()?;
    if !valid_digest(tuple_digest) {
        return None;
    }
    Some(SealRecord {
        digest,
        root_kind,
        tuple_count,
        tuple_digest: tuple_digest.to_string(),
    })
}

fn tuple_table(path: &Path) -> io::Result<(usize, String)> {
    // The stat walk lives in opt-level=3 `jet-pkg-model::SealWalk`: an
    // unoptimized walk of a toolchain-sized object burns tens of seconds of
    // pure CPU and defeats the sealed warm path.
    jet_pkg_model::SealWalk::tuple_table(path)
}

fn root_kind(path: &Path) -> Option<RootKind> {
    let metadata = fs::symlink_metadata(path).ok()?;
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        Some(RootKind::Symlink)
    } else if metadata.is_dir() {
        Some(RootKind::Directory)
    } else if metadata.is_file() {
        Some(RootKind::File)
    } else {
        None
    }
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256-") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}


fn ensure_real_directory(path: &Path, label: &str) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{label} is not a real directory; repair the path before writing"),
            ))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(path)?;
            let metadata = fs::symlink_metadata(path)?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{label} is not a real directory; repair the path before writing"),
                ));
            }
            Ok(())
        }
        Err(error) => Err(error),
    }
}
