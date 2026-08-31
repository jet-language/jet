//! Minimal SHA-256 implementation (std-only, invariant I6).
//! Used for package fingerprints in M12 store.

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

const H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

// Tree identity is a hostile-input boundary. Keep the per-file read and the
// total accepted payload bounded while hashing from a fixed-size buffer.
pub const MAX_TREE_FILE_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_TREE_TOTAL_BYTES: u64 = 4 * 1024 * 1024 * 1024;
pub const MAX_TREE_FILES: usize = 1_000_000;
pub const MAX_TREE_DEPTH: usize = 256;

/// Incremental SHA-256. Keeps at most one partial 64-byte block, allowing
/// compiler/tool identities to hash large artifacts without whole-file reads.
pub struct StreamingSha256 {
    state: [u32; 8],
    block: [u8; 64],
    block_len: usize,
    byte_len: u64,
}

impl StreamingSha256 {
    pub fn new() -> Self {
        Self {
            state: H0,
            block: [0; 64],
            block_len: 0,
            byte_len: 0,
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.byte_len = self.byte_len.wrapping_add(data.len() as u64);
        if self.block_len != 0 {
            let take = (64 - self.block_len).min(data.len());
            self.block[self.block_len..self.block_len + take].copy_from_slice(&data[..take]);
            self.block_len += take;
            data = &data[take..];
            if self.block_len == 64 {
                compress(&mut self.state, &self.block);
                self.block_len = 0;
            } else {
                return;
            }
        }
        let mut chunks = data.chunks_exact(64);
        for chunk in &mut chunks {
            compress(&mut self.state, chunk);
        }
        let remainder = chunks.remainder();
        self.block[..remainder.len()].copy_from_slice(remainder);
        self.block_len = remainder.len();
    }

    pub fn finalize(mut self) -> [u8; 32] {
        let mut tail = [0u8; 128];
        tail[..self.block_len].copy_from_slice(&self.block[..self.block_len]);
        tail[self.block_len] = 0x80;
        let tail_len = if self.block_len < 56 { 64 } else { 128 };
        tail[tail_len - 8..tail_len].copy_from_slice(&self.byte_len.wrapping_mul(8).to_be_bytes());
        for block in tail[..tail_len].chunks_exact(64) {
            compress(&mut self.state, block);
        }
        let mut out = [0u8; 32];
        for (index, word) in self.state.iter().enumerate() {
            out[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }
}

impl Default for StreamingSha256 {
    fn default() -> Self {
        Self::new()
    }
}

pub fn sha256_file_hex(path: &std::path::Path) -> std::io::Result<String> {
    use std::io::Read;
    let (mut file, expected) = open_regular_nofollow(path)?;
    if expected.len() > MAX_TREE_FILE_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file exceeds the tree hashing bound",
        ));
    }
    let mut hasher = StreamingSha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut actual_len = 0u64;
    let mut limited = (&mut file).take(MAX_TREE_FILE_BYTES.saturating_add(1));
    loop {
        let count = limited.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        actual_len = actual_len.saturating_add(count as u64);
        if actual_len > MAX_TREE_FILE_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "file grew beyond the tree hashing bound",
            ));
        }
        hasher.update(&buffer[..count]);
    }
    let after = file.metadata()?;
    let path_after = std::fs::symlink_metadata(path)?;
    if actual_len != expected.len()
        || !same_file_identity(&expected, &after)
        || !same_file_identity(&expected, &path_after)
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "file changed while hashing",
        ));
    }
    Ok(hex(hasher.finalize()))
}

/// Read one regular file through a held no-follow descriptor. The caller must
/// provide a finite bound; the shared tree-file bound is the hard ceiling for
/// every package identity read.
pub fn read_file_nofollow(
    path: &std::path::Path,
    max_bytes: u64,
) -> std::io::Result<Vec<u8>> {
    use std::io::Read;

    if max_bytes > MAX_TREE_FILE_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "file read bound exceeds the tree hashing bound",
        ));
    }
    let (mut file, expected) = open_regular_nofollow(path)?;
    if expected.len() > max_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file exceeds its read bound",
        ));
    }
    let capacity = usize::try_from(expected.len())
        .unwrap_or(usize::MAX)
        .min(64 * 1024);
    let mut content = Vec::with_capacity(capacity);
    let mut limited = (&mut file).take(max_bytes.saturating_add(1));
    limited.read_to_end(&mut content)?;
    if content.len() as u64 > max_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file exceeds its read bound",
        ));
    }
    let after = file.metadata()?;
    let path_after = std::fs::symlink_metadata(path)?;
    if content.len() as u64 != expected.len()
        || !same_file_identity(&expected, &after)
        || !same_file_identity(&expected, &path_after)
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "file changed while reading",
        ));
    }
    Ok(content)
}

fn open_regular_nofollow(
    path: &std::path::Path,
) -> std::io::Result<(std::fs::File, std::fs::Metadata)> {
    let expected = std::fs::symlink_metadata(path)?;
    if expected.file_type().is_symlink() || !expected.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file is not a regular file",
        ));
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    if !add_nofollow_flags(&mut options) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "no-follow regular-file reads are unavailable on this platform",
        ));
    }
    let file = options.open(path)?;
    let opened = file.metadata()?;
    if !opened.is_file() || !same_file_identity(&expected, &opened) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "file changed before reading",
        ));
    }
    Ok((file, expected))
}

fn hex(bytes: [u8; 32]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(64), |mut text, byte| {
            use std::fmt::Write;
            let _ = write!(text, "{byte:02x}");
            text
        })
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut state = H0;
    let bit_len = (data.len() as u64).wrapping_mul(8);

    // Pad: append 0x80, then zeros, then 8-byte big-endian bit length,
    // so total length ≡ 56 (mod 64).
    let pad_len = if data.len() % 64 < 56 {
        56 - data.len() % 64
    } else {
        120 - data.len() % 64
    };
    let mut msg: Vec<u8> = Vec::with_capacity(data.len() + pad_len + 8);
    msg.extend_from_slice(data);
    msg.push(0x80);
    msg.extend(std::iter::repeat(0u8).take(pad_len - 1));
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        compress(&mut state, chunk);
    }

    let mut out = [0u8; 32];
    for (i, word) in state.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

fn compress(state: &mut [u32; 8], block: &[u8]) {
    let mut w = [0u32; 64];
    for i in 0..16 {
        w[i] = u32::from_be_bytes(block[i * 4..i * 4 + 4].try_into().unwrap());
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }

    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ (!e & g);
        let temp1 = h
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let temp2 = s0.wrapping_add(maj);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(temp1);
        d = c;
        c = b;
        b = a;
        a = temp1.wrapping_add(temp2);
    }
    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex(sha256(data))
}

/// Compute a canonical tree hash of a source directory.
/// All copied source files are hashed in sorted order (relative paths + contents).
pub fn tree_hash(root: &std::path::Path) -> String {
    try_tree_hash(root).unwrap_or_else(|error| {
        panic!("cannot hash source tree `{}`: {error}", root.display())
    })
}

/// Failure while proving a source tree's contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeHashError {
    Io { path: std::path::PathBuf, detail: String },
    Symlink(std::path::PathBuf),
    Special(std::path::PathBuf),
    InvalidName(std::path::PathBuf),
    TooLarge {
        path: std::path::PathBuf,
        limit: u64,
    },
}

impl std::fmt::Display for TreeHashError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, detail } => write!(formatter, "could not read `{}`: {detail}", path.display()),
            Self::Symlink(path) => write!(formatter, "`{}` is a symlink", path.display()),
            Self::Special(path) => write!(formatter, "`{}` is not a regular file or directory", path.display()),
            Self::InvalidName(path) => write!(formatter, "`{}` has a non-UTF-8 name", path.display()),
            Self::TooLarge { path, limit } => write!(
                formatter,
                "`{}` exceeds the {}-byte tree hashing bound",
                path.display(),
                limit
            ),
        }
    }
}

impl std::error::Error for TreeHashError {}

/// Compute a canonical tree hash and fail closed on every unreadable,
/// linked, or special filesystem entry. Entries excluded from package
/// identity are still inspected first, so an attacker cannot hide a link or
/// device node behind an ignored name.
pub fn try_tree_hash(root: &std::path::Path) -> Result<String, TreeHashError> {
    let mut entries = Vec::new();
    let metadata = std::fs::symlink_metadata(root).map_err(|error| TreeHashError::Io {
        path: root.to_path_buf(),
        detail: error.to_string(),
    })?;
    if metadata.file_type().is_symlink() {
        return Err(TreeHashError::Symlink(root.to_path_buf()));
    }
    if !metadata.is_dir() {
        return Err(TreeHashError::Special(root.to_path_buf()));
    }
    collect_tree_files(root, root, &mut entries, 0)?;
    entries.sort_by(|left, right| left.relative.cmp(&right.relative));

    let mut hasher = StreamingSha256::new();
    let mut total_bytes = 0u64;
    for entry in entries {
        hasher.update(entry.relative.as_bytes());
        hasher.update(&[0]); // null separator
        hash_regular_nofollow(
            &entry.path,
            &entry.metadata,
            &mut hasher,
            &mut total_bytes,
        )?;
    }
    Ok(format!("sha256-{}", hex(hasher.finalize())))
}

struct TreeFile {
    relative: String,
    path: std::path::PathBuf,
    metadata: std::fs::Metadata,
}

fn collect_tree_files(
    dir: &std::path::Path,
    root: &std::path::Path,
    out: &mut Vec<TreeFile>,
    depth: usize,
) -> Result<(), TreeHashError> {
    // Internal modules remain hash inputs: D-SHAPE-MODULEINTERNAL1=A changes
    // automatic membership, not explicit imports or source-tree identity.
    let rd = std::fs::read_dir(dir).map_err(|error| TreeHashError::Io {
        path: dir.to_path_buf(),
        detail: error.to_string(),
    })?;
    for entry in rd {
        let entry = entry.map_err(|error| TreeHashError::Io {
            path: dir.to_path_buf(),
            detail: error.to_string(),
        })?;
        let p = entry.path();
        let name = p
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| TreeHashError::InvalidName(p.clone()))?;
        let metadata = std::fs::symlink_metadata(&p).map_err(|error| TreeHashError::Io {
            path: p.clone(),
            detail: error.to_string(),
        })?;
        if metadata.file_type().is_symlink() {
            return Err(TreeHashError::Symlink(p));
        }
        if !metadata.is_dir() && !metadata.is_file() {
            return Err(TreeHashError::Special(p));
        }
        if name.starts_with('.') || name == "build" || name == "target" {
            continue;
        }
        if metadata.is_dir() {
            if depth >= MAX_TREE_DEPTH {
                return Err(TreeHashError::TooLarge {
                    path: p,
                    limit: MAX_TREE_DEPTH as u64,
                });
            }
            collect_tree_files(&p, root, out, depth + 1)?;
        } else {
            if out.len() >= MAX_TREE_FILES {
                return Err(TreeHashError::TooLarge {
                    path: root.to_path_buf(),
                    limit: MAX_TREE_FILES as u64,
                });
            }
            let rel = p
                .strip_prefix(root)
                .map_err(|_| TreeHashError::InvalidName(p.clone()))?
                .to_str()
                .ok_or_else(|| TreeHashError::InvalidName(p.clone()))?
                .replace('\\', "/");
            out.push(TreeFile {
                relative: rel,
                path: p,
                metadata,
            });
        }
    }
    Ok(())
}

fn hash_regular_nofollow(
    path: &std::path::Path,
    expected: &std::fs::Metadata,
    hasher: &mut StreamingSha256,
    total_bytes: &mut u64,
) -> Result<(), TreeHashError> {
    use std::io::Read;

    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    if !add_nofollow_flags(&mut options) {
        return Err(TreeHashError::Io {
            path: path.to_path_buf(),
            detail: "no-follow regular-file reads are unavailable on this platform".to_string(),
        });
    }
    let mut file = options.open(path).map_err(|error| TreeHashError::Io {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;
    let opened = file.metadata().map_err(|error| TreeHashError::Io {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;
    if !opened.is_file() || !same_file_identity(expected, &opened) {
        return Err(TreeHashError::Io {
            path: path.to_path_buf(),
            detail: "file changed before hashing".to_string(),
        });
    }
    if opened.len() > MAX_TREE_FILE_BYTES {
        return Err(TreeHashError::TooLarge {
            path: path.to_path_buf(),
            limit: MAX_TREE_FILE_BYTES,
        });
    }
    let next_total = total_bytes
        .checked_add(opened.len())
        .ok_or_else(|| TreeHashError::TooLarge {
            path: path.to_path_buf(),
            limit: MAX_TREE_TOTAL_BYTES,
        })?;
    if next_total > MAX_TREE_TOTAL_BYTES {
        return Err(TreeHashError::TooLarge {
            path: path.to_path_buf(),
            limit: MAX_TREE_TOTAL_BYTES,
        });
    }
    hasher.update(&opened.len().to_be_bytes());
    let mut actual_len = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    let mut limited = (&mut file).take(opened.len().saturating_add(1));
    loop {
        let count = limited.read(&mut buffer).map_err(|error| TreeHashError::Io {
            path: path.to_path_buf(),
            detail: error.to_string(),
        })?;
        if count == 0 {
            break;
        }
        actual_len = actual_len.saturating_add(count as u64);
        hasher.update(&buffer[..count]);
        if actual_len > opened.len() {
            return Err(TreeHashError::Io {
                path: path.to_path_buf(),
                detail: "file grew while hashing".to_string(),
            });
        }
    }
    let after = file.metadata().map_err(|error| TreeHashError::Io {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;
    if !same_file_identity(expected, &after) || after.len() != actual_len {
        return Err(TreeHashError::Io {
            path: path.to_path_buf(),
            detail: "file changed while hashing".to_string(),
        });
    }
    *total_bytes = next_total;
    Ok(())
}

fn add_nofollow_flags(options: &mut std::fs::OpenOptions) -> bool {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        const O_CLOEXEC: i32 = 0o2000000;
        const O_NOFOLLOW: i32 = 0o400000;
        options.custom_flags(O_NOFOLLOW | O_CLOEXEC);
        return true;
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        const O_CLOEXEC: i32 = 0x01000000;
        const O_NOFOLLOW: i32 = 0x0100;
        options.custom_flags(O_NOFOLLOW | O_CLOEXEC);
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        return true;
    }
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        windows
    )))]
    {
        let _ = options;
        false
    }
}

fn same_file_identity(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_empty() {
        // SHA-256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        let got = sha256_hex(b"");
        assert_eq!(
            got,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_abc() {
        // SHA-256("abc") — NIST FIPS 180-4 test vector.
        let got = sha256_hex(b"abc");
        assert_eq!(
            got,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn streaming_matches_one_shot_across_block_boundaries() {
        let data = (0..10_000)
            .map(|value| (value % 251) as u8)
            .collect::<Vec<_>>();
        let mut streaming = StreamingSha256::new();
        for chunk in data.chunks(73) {
            streaming.update(chunk);
        }
        assert_eq!(hex(streaming.finalize()), sha256_hex(&data));
    }

    #[test]
    fn streaming_preserves_partial_blocks_across_small_updates() {
        let mut streaming = StreamingSha256::new();
        for byte in b"abc" {
            streaming.update(std::slice::from_ref(byte));
        }
        assert_eq!(hex(streaming.finalize()), sha256_hex(b"abc"));
    }

    #[cfg(unix)]
    #[test]
    fn tree_hash_rejects_recursive_symlink_nodes() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "jet-tree-hash-symlink-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/value.txt"), b"stable\n").unwrap();
        symlink(".", root.join("src/loop")).unwrap();
        assert!(matches!(
            try_tree_hash(&root),
            Err(TreeHashError::Symlink(_))
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn tree_hash_rejects_special_nodes() {
        use std::os::unix::net::UnixListener;

        let root = std::env::temp_dir().join(format!(
            "jet-sha-tree-special-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let socket = root.join("socket");
        let listener = UnixListener::bind(&socket).unwrap();
        assert!(matches!(
            try_tree_hash(&root),
            Err(TreeHashError::Special(path)) if path == socket
        ));
        drop(listener);
        let _ = std::fs::remove_dir_all(root);
    }
}
