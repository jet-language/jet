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

#[derive(Debug, Clone, PartialEq, Eq)]
struct Tuple {
    relative: Vec<u8>,
    kind: u8,
    inode: u64,
    size: u64,
    mtime_ns: i128,
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
    let mut tuples = Vec::new();
    collect_tuples(path, &mut Vec::new(), &mut tuples)?;
    tuples.sort_by(|left, right| {
        left.relative
            .cmp(&right.relative)
            .then_with(|| left.kind.cmp(&right.kind))
    });
    let mut table = Vec::new();
    for tuple in &tuples {
        table.push(tuple.kind);
        table.push(b'\t');
        append_hex(&mut table, &tuple.relative);
        table.extend_from_slice(
            format!("\t{}\t{}\t{}\n", tuple.inode, tuple.size, tuple.mtime_ns).as_bytes(),
        );
    }
    Ok((tuples.len(), format!("sha256-{}", super::super::SHA256::sha256_hex(&table))))
}

fn collect_tuples(path: &Path, relative: &mut Vec<u8>, tuples: &mut Vec<Tuple>) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    let file_type = metadata.file_type();
    if file_type.is_symlink() || metadata.is_file() {
        tuples.push(Tuple {
            relative: relative.clone(),
            kind: if file_type.is_symlink() { b'l' } else { b'f' },
            inode: inode(&metadata),
            size: metadata.len(),
            mtime_ns: mtime_ns(&metadata),
        });
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Hangar object contains an unsupported node: {}", path.display()),
        ));
    }
    let mut entries = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by(|left, right| os_bytes(&left.file_name()).cmp(&os_bytes(&right.file_name())));
    for entry in entries {
        let old_len = relative.len();
        if old_len != 0 {
            relative.push(b'/');
        }
        relative.extend_from_slice(&os_bytes(&entry.file_name()));
        collect_tuples(&entry.path(), relative, tuples)?;
        relative.truncate(old_len);
    }
    Ok(())
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

fn append_hex(output: &mut Vec<u8>, bytes: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize]);
        output.push(HEX[(byte & 0x0f) as usize]);
    }
}

fn inode(metadata: &fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        metadata.ino()
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        metadata.file_index().unwrap_or_default()
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = metadata;
        0
    }
}

fn mtime_ns(metadata: &fs::Metadata) -> i128 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        return i128::from(metadata.mtime()) * 1_000_000_000 + i128::from(metadata.mtime_nsec());
    }
    #[cfg(not(unix))]
    metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| i128::from(duration.as_secs()) * 1_000_000_000 + i128::from(duration.subsec_nanos()))
        .unwrap_or_default()
}

fn os_bytes(value: &std::ffi::OsStr) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;
        return value.as_bytes().to_vec();
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt as _;
        return value
            .encode_wide()
            .flat_map(u16::to_be_bytes)
            .collect();
    }
    #[cfg(not(any(unix, windows)))]
    value.to_string_lossy().as_bytes().to_vec()
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
