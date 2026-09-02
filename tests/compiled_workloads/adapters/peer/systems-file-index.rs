// #1414 peer adapter. This rail uses only Rust's standard library filesystem
// APIs and a local SHA-256 implementation.
use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

struct Entry {
    path: PathBuf,
    relative: String,
}

fn validate_root(root: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(root)?;
    if !metadata.file_type().is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            "filesystem walk root is not a directory",
        ));
    }
    for ancestor in root.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        if fs::symlink_metadata(ancestor)?.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "filesystem walk path contains symlink",
            ));
        }
    }
    Ok(())
}

fn collect(root: &Path, directory: &Path, entries: &mut Vec<Entry>) -> io::Result<()> {
    let mut children = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    children.sort_by_key(|entry| entry.file_name());
    for child in children {
        let path = child.path();
        let metadata = fs::symlink_metadata(&path);
        let is_dir = metadata
            .as_ref()
            .is_ok_and(|value| value.file_type().is_dir());
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if is_dir {
            collect(root, &path, entries)?;
        } else {
            entries.push(Entry { path, relative });
        }
    }
    Ok(())
}

fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1,
        0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
        0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
        0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
        0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
        0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut state = [
        0x6a09e667_u32,
        0xbb67ae85,
        0x3c6ef372,
        0xa54ff53a,
        0x510e527f,
        0x9b05688c,
        0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut message = Vec::with_capacity((data.len() + 72) / 64 * 64);
    message.extend_from_slice(data);
    message.push(0x80);
    while (message.len() + 8) % 64 != 0 {
        message.push(0);
    }
    message.extend_from_slice(&(data.len() as u64).wrapping_mul(8).to_be_bytes());
    for chunk in message.chunks_exact(64) {
        let mut schedule = [0_u32; 64];
        for index in 0..16 {
            let offset = index * 4;
            schedule[index] = u32::from_be_bytes([
                chunk[offset],
                chunk[offset + 1],
                chunk[offset + 2],
                chunk[offset + 3],
            ]);
        }
        for index in 16..64 {
            let value = schedule[index - 15].rotate_right(7)
                ^ schedule[index - 15].rotate_right(18)
                ^ (schedule[index - 15] >> 3);
            let other = schedule[index - 2].rotate_right(17)
                ^ schedule[index - 2].rotate_right(19)
                ^ (schedule[index - 2] >> 10);
            schedule[index] = schedule[index - 16]
                .wrapping_add(value)
                .wrapping_add(schedule[index - 7])
                .wrapping_add(other);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for index in 0..64 {
            let sum_one = e.rotate_right(6)
                ^ e.rotate_right(11)
                ^ e.rotate_right(25);
            let choose = (e & f) ^ ((!e) & g);
            let first = h
                .wrapping_add(sum_one)
                .wrapping_add(choose)
                .wrapping_add(K[index])
                .wrapping_add(schedule[index]);
            let sum_two = a.rotate_right(2)
                ^ a.rotate_right(13)
                ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let second = sum_two.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(first);
            d = c;
            c = b;
            b = a;
            a = first.wrapping_add(second);
        }
        for (slot, value) in [a, b, c, d, e, f, g, h].into_iter().enumerate() {
            state[slot] = state[slot].wrapping_add(value);
        }
    }
    let mut output = [0_u8; 32];
    for (index, value) in state.into_iter().enumerate() {
        output[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
    }
    output
}

fn hex_digest(data: &[u8]) -> String {
    sha256(data)
        .into_iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn main() {
    let root = PathBuf::from(env::args().nth(1).expect("input root"));
    validate_root(&root).expect("cannot validate input root");
    let mut entries = Vec::new();
    collect(&root, &root, &mut entries).expect("cannot walk input root");
    entries.sort_by(|left, right| left.relative.cmp(&right.relative));

    let mut files = Vec::new();
    let mut bytes = 0_u64;
    let mut links = 0_u64;
    let mut rejects = 0_u64;
    for entry in entries {
        if entry.relative == ".fixture-modes.tsv" {
            continue;
        }
        let metadata = match fs::symlink_metadata(&entry.path) {
            Ok(value) => value,
            Err(_) => {
                rejects += 1;
                continue;
            }
        };
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            links += 1;
            continue;
        }
        if !file_type.is_file() {
            rejects += 1;
            continue;
        }
        let content = match fs::read(&entry.path) {
            Ok(value) => value,
            Err(_) => {
                rejects += 1;
                continue;
            }
        };
        bytes += content.len() as u64;
        files.push(format!(
            "file|{}|{}|{}",
            entry.relative,
            content.len(),
            hex_digest(&content)
        ));
    }
    files.sort();
    let canonical = files.iter().map(|row| format!("{row}\n")).collect::<String>();
    println!("files={}", files.len());
    println!("bytes={bytes}");
    println!("links={links}");
    println!("rejects={rejects}");
    println!("index_sha256={}", hex_digest(canonical.as_bytes()));
    for row in files {
        println!("{row}");
    }
}
