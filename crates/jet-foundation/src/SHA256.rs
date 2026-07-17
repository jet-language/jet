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
        Self { state: H0, block: [0; 64], block_len: 0, byte_len: 0 }
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
            }
        }
        let mut chunks = data.chunks_exact(64);
        for chunk in &mut chunks { compress(&mut self.state, chunk); }
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
        for block in tail[..tail_len].chunks_exact(64) { compress(&mut self.state, block); }
        let mut out = [0u8; 32];
        for (index, word) in self.state.iter().enumerate() {
            out[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }
}

impl Default for StreamingSha256 {
    fn default() -> Self { Self::new() }
}

pub fn sha256_file_hex(path: &std::path::Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = StreamingSha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 { break; }
        hasher.update(&buffer[..count]);
    }
    Ok(hex(hasher.finalize()))
}

fn hex(bytes: [u8; 32]) -> String {
    bytes.iter().fold(String::with_capacity(64), |mut text, byte| {
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
/// All .jet files are hashed in sorted order (relative paths + contents).
pub fn tree_hash(root: &std::path::Path) -> String {
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    collect_jet_files(root, root, &mut entries);
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher_input: Vec<u8> = Vec::new();
    for (rel, content) in &entries {
        hasher_input.extend_from_slice(rel.as_bytes());
        hasher_input.push(0); // null separator
        hasher_input.extend_from_slice(&(content.len() as u64).to_be_bytes());
        hasher_input.extend_from_slice(content);
    }
    format!("sha256-{}", sha256_hex(&hasher_input))
}

fn collect_jet_files(
    dir: &std::path::Path,
    root: &std::path::Path,
    out: &mut Vec<(String, Vec<u8>)>,
) {
    // Internal modules remain hash inputs: D-SHAPE-MODULEINTERNAL1=A changes
    // automatic membership, not explicit imports or source-tree identity.
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with('.') || name == "build" || name == "target" {
            continue;
        }
        if p.is_dir() {
            collect_jet_files(&p, root, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some(crate::Syntax::FILE_EXT) {
            if let Ok(content) = std::fs::read(&p) {
                let rel = p
                    .strip_prefix(root)
                    .unwrap_or(&p)
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((rel, content));
            }
        }
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
        let data = (0..10_000).map(|value| (value % 251) as u8).collect::<Vec<_>>();
        let mut streaming = StreamingSha256::new();
        for chunk in data.chunks(73) { streaming.update(chunk); }
        assert_eq!(hex(streaming.finalize()), sha256_hex(&data));
    }
}
