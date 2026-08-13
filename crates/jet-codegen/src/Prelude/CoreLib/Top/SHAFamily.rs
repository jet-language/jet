// Dependency-free SHA-1, SHA-2, SHA-3, PBKDF2, and incremental hashing.

const SHA256_IV: [u32; 8] = [
    0x6a09e667,
    0xbb67ae85,
    0x3c6ef372,
    0xa54ff53a,
    0x510e527f,
    0x9b05688c,
    0x1f83d9ab,
    0x5be0cd19,
];

const SHA224_IV: [u32; 8] = [
    0xc1059ed8,
    0x367cd507,
    0x3070dd17,
    0xf70e5939,
    0xffc00b31,
    0x68581511,
    0x64f98fa7,
    0xbefa4fa4,
];

const SHA512_IV: [u64; 8] = [
    0x6a09e667f3bcc908,
    0xbb67ae8584caa73b,
    0x3c6ef372fe94f82b,
    0xa54ff53a5f1d36f1,
    0x510e527fade682d1,
    0x9b05688c2b3e6c1f,
    0x1f83d9abfb41bd6b,
    0x5be0cd19137e2179,
];

const SHA384_IV: [u64; 8] = [
    0xcbbb9d5dc1059ed8,
    0x629a292a367cd507,
    0x9159015a3070dd17,
    0x152fecd8f70e5939,
    0x67332667ffc00b31,
    0x8eb44a8768581511,
    0xdb0c2e0d64f98fa7,
    0x47b5481dbefa4fa4,
];

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
    0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
    0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
    0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
    0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
    0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
    0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
    0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
    0xc67178f2,
];

const SHA512_K: [u64; 80] = [
    0x428a2f98d728ae22, 0x7137449123ef65cd, 0xb5c0fbcfec4d3b2f, 0xe9b5dba58189dbbc,
    0x3956c25bf348b538, 0x59f111f1b605d019, 0x923f82a4af194f9b, 0xab1c5ed5da6d8118,
    0xd807aa98a3030242, 0x12835b0145706fbe, 0x243185be4ee4b28c, 0x550c7dc3d5ffb4e2,
    0x72be5d74f27b896f, 0x80deb1fe3b1696b1, 0x9bdc06a725c71235, 0xc19bf174cf692694,
    0xe49b69c19ef14ad2, 0xefbe4786384f25e3, 0x0fc19dc68b8cd5b5, 0x240ca1cc77ac9c65,
    0x2de92c6f592b0275, 0x4a7484aa6ea6e483, 0x5cb0a9dcbd41fbd4, 0x76f988da831153b5,
    0x983e5152ee66dfab, 0xa831c66d2db43210, 0xb00327c898fb213f, 0xbf597fc7beef0ee4,
    0xc6e00bf33da88fc2, 0xd5a79147930aa725, 0x06ca6351e003826f, 0x142929670a0e6e70,
    0x27b70a8546d22ffc, 0x2e1b21385c26c926, 0x4d2c6dfc5ac42aed, 0x53380d139d95b3df,
    0x650a73548baf63de, 0x766a0abb3c77b2a8, 0x81c2c92e47edaee6, 0x92722c851482353b,
    0xa2bfe8a14cf10364, 0xa81a664bbc423001, 0xc24b8b70d0f89791, 0xc76c51a30654be30,
    0xd192e819d6ef5218, 0xd69906245565a910, 0xf40e35855771202a, 0x106aa07032bbd1b8,
    0x19a4c116b8d2d0c8, 0x1e376c085141ab53, 0x2748774cdf8eeb99, 0x34b0bcb5e19b48a8,
    0x391c0cb3c5c95a63, 0x4ed8aa4ae3418acb, 0x5b9cca4f7763e373, 0x682e6ff3d6b2b8a3,
    0x748f82ee5defb2fc, 0x78a5636f43172f60, 0x84c87814a1f0ab72, 0x8cc702081a6439ec,
    0x90befffa23631e28, 0xa4506cebde82bde9, 0xbef9a3f7b2c67915, 0xc67178f2e372532b,
    0xca273eceea26619c, 0xd186b8c721c0c207, 0xeada7dd6cde0eb1e, 0xf57d4f7fee6ed178,
    0x06f067aa72176fba, 0x0a637dc5a2c898a6, 0x113f9804bef90dae, 0x1b710b35131c471b,
    0x28db77f523047d84, 0x32caab7b40c72493, 0x3c9ebe0a15c9bebc, 0x431d67c49c100d4c,
    0x4cc5d4becb3e42b6, 0x597f299cfc657e2a, 0x5fcb6fab3ad6faec, 0x6c44198c4a475817,
];

fn sha_hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[usize::from(*byte >> 4)] as char);
        out.push(HEX[usize::from(*byte & 0x0f)] as char);
    }
    out
}

fn sha1_raw(data: &[u8]) -> [u8; 20] {
    let mut state = [
        0x67452301u32,
        0xefcdab89,
        0x98badcfe,
        0x10325476,
        0xc3d2e1f0,
    ];
    let mut message = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());
    for block in message.chunks(64) {
        let mut w = [0u32; 80];
        for index in 0..16 {
            w[index] = u32::from_be_bytes(block[index * 4..index * 4 + 4].try_into().unwrap());
        }
        for index in 16..80 {
            w[index] = (w[index - 3] ^ w[index - 8] ^ w[index - 14] ^ w[index - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = state;
        for index in 0..80 {
            let (function, constant) = match index {
                0..=19 => ((b & c) | (!b & d), 0x5a827999),
                20..=39 => (b ^ c ^ d, 0x6ed9eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1bbcdc),
                _ => (b ^ c ^ d, 0xca62c1d6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(function)
                .wrapping_add(e)
                .wrapping_add(constant)
                .wrapping_add(w[index]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
    }
    let mut out = [0u8; 20];
    for (index, word) in state.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

fn sha256_compress(state: &mut [u32; 8], block: &[u8]) {
    let mut w = [0u32; 64];
    for index in 0..16 {
        w[index] = u32::from_be_bytes(block[index * 4..index * 4 + 4].try_into().unwrap());
    }
    for index in 16..64 {
        let a = w[index - 15];
        let b = w[index - 2];
        let s0 = a.rotate_right(7) ^ a.rotate_right(18) ^ (a >> 3);
        let s1 = b.rotate_right(17) ^ b.rotate_right(19) ^ (b >> 10);
        w[index] = w[index - 16]
            .wrapping_add(s0)
            .wrapping_add(w[index - 7])
            .wrapping_add(s1);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for index in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ (!e & g);
        let temp1 = h
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(SHA256_K[index])
            .wrapping_add(w[index]);
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

fn sha256_raw_with_iv(data: &[u8], initial: [u32; 8]) -> [u8; 32] {
    let mut state = initial;
    let mut message = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());
    for block in message.chunks(64) {
        sha256_compress(&mut state, block);
    }
    let mut out = [0u8; 32];
    for (index, word) in state.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

fn sha512_compress(state: &mut [u64; 8], block: &[u8]) {
    let mut w = [0u64; 80];
    for index in 0..16 {
        w[index] = u64::from_be_bytes(block[index * 8..index * 8 + 8].try_into().unwrap());
    }
    for index in 16..80 {
        let a = w[index - 15];
        let b = w[index - 2];
        let s0 = a.rotate_right(1) ^ a.rotate_right(8) ^ (a >> 7);
        let s1 = b.rotate_right(19) ^ b.rotate_right(61) ^ (b >> 6);
        w[index] = w[index - 16]
            .wrapping_add(s0)
            .wrapping_add(w[index - 7])
            .wrapping_add(s1);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for index in 0..80 {
        let s1 = e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41);
        let ch = (e & f) ^ (!e & g);
        let temp1 = h
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(SHA512_K[index])
            .wrapping_add(w[index]);
        let s0 = a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39);
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

fn sha512_raw_with_iv(data: &[u8], initial: [u64; 8]) -> [u8; 64] {
    let mut state = initial;
    let mut message = data.to_vec();
    let bit_len = (data.len() as u128).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 128 != 112 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());
    for block in message.chunks(128) {
        sha512_compress(&mut state, block);
    }
    let mut out = [0u8; 64];
    for (index, word) in state.iter().enumerate() {
        out[index * 8..index * 8 + 8].copy_from_slice(&word.to_be_bytes());
    }
    out
}

fn keccak_f(state: &mut [u64; 25]) {
    const ROTATION: [[u32; 5]; 5] = [
        [0, 36, 3, 41, 18],
        [1, 44, 10, 45, 2],
        [62, 6, 43, 15, 61],
        [28, 55, 25, 21, 56],
        [27, 20, 39, 8, 14],
    ];
    const ROUND: [u64; 24] = [
        0x0000000000000001,
        0x0000000000008082,
        0x800000000000808a,
        0x8000000080008000,
        0x000000000000808b,
        0x0000000080000001,
        0x8000000080008081,
        0x8000000000008009,
        0x000000000000008a,
        0x0000000000000088,
        0x0000000080008009,
        0x000000008000000a,
        0x000000008000808b,
        0x800000000000008b,
        0x8000000000008089,
        0x8000000000008003,
        0x8000000000008002,
        0x8000000000000080,
        0x000000000000800a,
        0x800000008000000a,
        0x8000000080008081,
        0x8000000000008080,
        0x0000000080000001,
        0x8000000080008008,
    ];
    for constant in ROUND {
        let mut column = [0u64; 5];
        for x in 0..5 {
            column[x] = state[x] ^ state[x + 5] ^ state[x + 10] ^ state[x + 15] ^ state[x + 20];
        }
        let mut delta = [0u64; 5];
        for x in 0..5 {
            delta[x] = column[(x + 4) % 5] ^ column[(x + 1) % 5].rotate_left(1);
        }
        for y in 0..5 {
            for x in 0..5 {
                state[x + 5 * y] ^= delta[x];
            }
        }
        let mut permuted = [0u64; 25];
        for y in 0..5 {
            for x in 0..5 {
                let new_x = y;
                let new_y = (2 * x + 3 * y) % 5;
                permuted[new_x + 5 * new_y] = state[x + 5 * y].rotate_left(ROTATION[x][y]);
            }
        }
        for y in 0..5 {
            for x in 0..5 {
                state[x + 5 * y] = permuted[x + 5 * y]
                    ^ ((!permuted[(x + 1) % 5 + 5 * y]) & permuted[(x + 2) % 5 + 5 * y]);
            }
        }
        state[0] ^= constant;
    }
}

fn sha3_raw(data: &[u8], rate: usize, output_len: usize) -> Vec<u8> {
    let mut state = [0u64; 25];
    let mut message = data.to_vec();
    message.push(0x06);
    while message.len() % rate != rate - 1 {
        message.push(0);
    }
    message.push(0x80);
    for block in message.chunks(rate) {
        for index in 0..rate / 8 {
            state[index] ^= u64::from_le_bytes(block[index * 8..index * 8 + 8].try_into().unwrap());
        }
        keccak_f(&mut state);
    }
    let mut out = Vec::with_capacity(output_len);
    for lane in state.iter().take(rate / 8) {
        out.extend_from_slice(&lane.to_le_bytes());
    }
    out.truncate(output_len);
    out
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut key_block = if key.len() > 64 {
        sha256_raw_with_iv(key, SHA256_IV).to_vec()
    } else {
        key.to_vec()
    };
    key_block.resize(64, 0);
    let mut inner = vec![0x36u8; 64];
    let mut outer = vec![0x5cu8; 64];
    for index in 0..64 {
        inner[index] ^= key_block[index];
        outer[index] ^= key_block[index];
    }
    inner.extend_from_slice(data);
    let inner_hash = sha256_raw_with_iv(&inner, SHA256_IV);
    outer.extend_from_slice(&inner_hash);
    sha256_raw_with_iv(&outer, SHA256_IV)
}

pub fn jet_crypto_sha1_hex(data: &Vec<u8>) -> String {
    sha_hex_encode(&sha1_raw(data))
}

pub fn jet_crypto_sha224_hex(data: &Vec<u8>) -> String {
    sha_hex_encode(&sha256_raw_with_iv(data, SHA224_IV)[..28])
}

pub fn jet_crypto_sha384_hex(data: &Vec<u8>) -> String {
    sha_hex_encode(&sha512_raw_with_iv(data, SHA384_IV)[..48])
}

pub fn jet_crypto_sha3_224_hex(data: &Vec<u8>) -> String {
    sha_hex_encode(&sha3_raw(data, 144, 28))
}

pub fn jet_crypto_sha3_256_hex(data: &Vec<u8>) -> String {
    sha_hex_encode(&sha3_raw(data, 136, 32))
}

pub fn jet_crypto_sha3_384_hex(data: &Vec<u8>) -> String {
    sha_hex_encode(&sha3_raw(data, 104, 48))
}

pub fn jet_crypto_sha3_512_hex(data: &Vec<u8>) -> String {
    sha_hex_encode(&sha3_raw(data, 72, 64))
}

pub fn jet_crypto_pbkdf2_hmac(
    password: &Vec<u8>,
    salt: &Vec<u8>,
    iterations: i64,
    key_len: i64,
) -> Vec<u8> {
    let Ok(key_len) = usize::try_from(key_len) else {
        return Vec::new();
    };
    if iterations <= 0 || key_len > 64 * 1024 * 1024 {
        return Vec::new();
    }
    let block_count = key_len.div_ceil(32);
    let Ok(block_count) = u32::try_from(block_count) else {
        return Vec::new();
    };
    let mut output = Vec::with_capacity(key_len);
    for block in 1..=block_count {
        let mut input = salt.clone();
        input.extend_from_slice(&block.to_be_bytes());
        let mut u = hmac_sha256(password, &input);
        let mut mixed = u;
        for _ in 1..iterations {
            u = hmac_sha256(password, &u);
            for (left, right) in mixed.iter_mut().zip(u) {
                *left ^= right;
            }
        }
        output.extend_from_slice(&mixed);
    }
    output.truncate(key_len);
    output
}

pub struct JetCryptoHasher {
    state: [u32; 8],
    buffer: Vec<u8>,
    length: u64,
}

pub fn jet_crypto_hasher_new() -> JetCryptoHasher {
    JetCryptoHasher {
        state: SHA256_IV,
        buffer: Vec::new(),
        length: 0,
    }
}

pub fn jet_crypto_hasher_update(hasher: &mut JetCryptoHasher, data: &Vec<u8>) {
    hasher.length = hasher.length.wrapping_add(data.len() as u64);
    hasher.buffer.extend_from_slice(data);
    while hasher.buffer.len() >= 64 {
        let block: Vec<u8> = hasher.buffer.drain(..64).collect();
        sha256_compress(&mut hasher.state, &block);
    }
}

pub fn jet_crypto_hasher_digest(hasher: &JetCryptoHasher) -> String {
    let mut state = hasher.state;
    let mut tail = hasher.buffer.clone();
    tail.push(0x80);
    while tail.len() % 64 != 56 {
        tail.push(0);
    }
    tail.extend_from_slice(&hasher.length.wrapping_mul(8).to_be_bytes());
    for block in tail.chunks(64) {
        sha256_compress(&mut state, block);
    }
    let mut output = [0u8; 32];
    for (index, word) in state.iter().enumerate() {
        output[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    sha_hex_encode(&output)
}
