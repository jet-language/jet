//! Std-only AES-256-GCM matching RustCrypto / NIST CAVS.

const SBOX: [u8; 256] = [
    0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
    0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
    0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
    0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
    0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
    0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
    0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
    0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
    0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
    0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
    0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
    0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
    0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
    0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
    0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
    0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
];

const RCON: [u8; 10] = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36];

fn xtime(value: u8) -> u8 {
    let high = value >> 7;
    (value << 1) ^ (high.wrapping_mul(0x1b))
}

fn mul(mut left: u8, mut right: u8) -> u8 {
    let mut product = 0u8;
    for _ in 0..8 {
        if right & 1 != 0 {
            product ^= left;
        }
        let high = left >> 7;
        left = (left << 1) ^ (high.wrapping_mul(0x1b));
        right >>= 1;
    }
    product
}

fn expand_key(key: &[u8; 32]) -> [[u8; 16]; 15] {
    let mut w = [0u8; 240];
    w[..32].copy_from_slice(key);
    let mut i = 8;
    while i < 60 {
        let mut temp = [
            w[(i - 1) * 4],
            w[(i - 1) * 4 + 1],
            w[(i - 1) * 4 + 2],
            w[(i - 1) * 4 + 3],
        ];
        if i % 8 == 0 {
            temp = [temp[1], temp[2], temp[3], temp[0]];
            for byte in &mut temp {
                *byte = SBOX[*byte as usize];
            }
            temp[0] ^= RCON[i / 8 - 1];
        } else if i % 8 == 4 {
            for byte in &mut temp {
                *byte = SBOX[*byte as usize];
            }
        }
        for j in 0..4 {
            w[i * 4 + j] = w[(i - 8) * 4 + j] ^ temp[j];
        }
        i += 1;
    }
    let mut rounds = [[0u8; 16]; 15];
    for (round, slot) in rounds.iter_mut().enumerate() {
        slot.copy_from_slice(&w[round * 16..(round + 1) * 16]);
    }
    rounds
}

fn add_round_key(state: &mut [u8; 16], key: &[u8; 16]) {
    for index in 0..16 {
        state[index] ^= key[index];
    }
}

fn sub_bytes(state: &mut [u8; 16]) {
    for byte in state.iter_mut() {
        *byte = SBOX[*byte as usize];
    }
}

fn shift_rows(state: &mut [u8; 16]) {
    let tmp = *state;
    state[1] = tmp[5];
    state[5] = tmp[9];
    state[9] = tmp[13];
    state[13] = tmp[1];
    state[2] = tmp[10];
    state[6] = tmp[14];
    state[10] = tmp[2];
    state[14] = tmp[6];
    state[3] = tmp[15];
    state[7] = tmp[3];
    state[11] = tmp[7];
    state[15] = tmp[11];
}

fn mix_columns(state: &mut [u8; 16]) {
    for col in 0..4 {
        let i = col * 4;
        let a = state[i];
        let b = state[i + 1];
        let c = state[i + 2];
        let d = state[i + 3];
        state[i] = mul(a, 2) ^ mul(b, 3) ^ c ^ d;
        state[i + 1] = a ^ mul(b, 2) ^ mul(c, 3) ^ d;
        state[i + 2] = a ^ b ^ mul(c, 2) ^ mul(d, 3);
        state[i + 3] = mul(a, 3) ^ b ^ c ^ mul(d, 2);
    }
}

fn aes_encrypt_block(round_keys: &[[u8; 16]; 15], input: &[u8; 16]) -> [u8; 16] {
    let mut state = *input;
    add_round_key(&mut state, &round_keys[0]);
    for round in 1..14 {
        sub_bytes(&mut state);
        shift_rows(&mut state);
        mix_columns(&mut state);
        add_round_key(&mut state, &round_keys[round]);
    }
    sub_bytes(&mut state);
    shift_rows(&mut state);
    add_round_key(&mut state, &round_keys[14]);
    state
}

fn ghash_mul(x: &mut [u8; 16], y: &[u8; 16]) {
    let mut z = [0u8; 16];
    let mut v = *y;
    for byte in x.iter() {
        for bit in (0..8).rev() {
            if (byte >> bit) & 1 != 0 {
                for i in 0..16 {
                    z[i] ^= v[i];
                }
            }
            let lsb = v[15] & 1;
            for i in (1..16).rev() {
                v[i] = (v[i] >> 1) | (v[i - 1] << 7);
            }
            v[0] >>= 1;
            if lsb != 0 {
                v[0] ^= 0xe1;
            }
        }
    }
    *x = z;
}

fn ghash(h: &[u8; 16], aad: &[u8], ciphertext: &[u8]) -> [u8; 16] {
    let mut y = [0u8; 16];
    for chunk in aad.chunks(16) {
        let mut block = [0u8; 16];
        block[..chunk.len()].copy_from_slice(chunk);
        for i in 0..16 {
            y[i] ^= block[i];
        }
        ghash_mul(&mut y, h);
    }
    for chunk in ciphertext.chunks(16) {
        let mut block = [0u8; 16];
        block[..chunk.len()].copy_from_slice(chunk);
        for i in 0..16 {
            y[i] ^= block[i];
        }
        ghash_mul(&mut y, h);
    }
    let mut len_block = [0u8; 16];
    let aad_bits = (aad.len() as u64).wrapping_mul(8);
    let ct_bits = (ciphertext.len() as u64).wrapping_mul(8);
    len_block[0..8].copy_from_slice(&aad_bits.to_be_bytes());
    len_block[8..16].copy_from_slice(&ct_bits.to_be_bytes());
    for i in 0..16 {
        y[i] ^= len_block[i];
    }
    ghash_mul(&mut y, h);
    y
}

fn inc32(counter: &mut [u8; 16]) {
    for byte in counter[12..].iter_mut().rev() {
        *byte = byte.wrapping_add(1);
        if *byte != 0 {
            break;
        }
    }
}

fn gctr(round_keys: &[[u8; 16]; 15], icb: &[u8; 16], input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut counter = *icb;
    for chunk in input.chunks(16) {
        let keystream = aes_encrypt_block(round_keys, &counter);
        for (byte, mask) in chunk.iter().zip(keystream.iter()) {
            out.push(byte ^ mask);
        }
        inc32(&mut counter);
    }
    out
}

pub(super) fn seal(key: &[u8], nonce: &[u8], plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>, ()> {
    let key: [u8; 32] = key.try_into().map_err(|_| ())?;
    let nonce: [u8; 12] = nonce.try_into().map_err(|_| ())?;
    let round_keys = expand_key(&key);
    let h = aes_encrypt_block(&round_keys, &[0u8; 16]);
    let mut j0 = [0u8; 16];
    j0[..12].copy_from_slice(&nonce);
    j0[15] = 1;
    let mut counter = j0;
    inc32(&mut counter);
    let ciphertext = gctr(&round_keys, &counter, plaintext);
    let s = ghash(&h, aad, &ciphertext);
    let tag_mask = aes_encrypt_block(&round_keys, &j0);
    let mut tag = [0u8; 16];
    for i in 0..16 {
        tag[i] = s[i] ^ tag_mask[i];
    }
    let mut out = ciphertext;
    out.extend_from_slice(&tag);
    let _ = xtime; // silence if unused in some paths
    Ok(out)
}

pub(super) fn open(key: &[u8], nonce: &[u8], ciphertext: &[u8], aad: &[u8]) -> Result<Vec<u8>, ()> {
    if ciphertext.len() < 16 {
        return Err(());
    }
    let key: [u8; 32] = key.try_into().map_err(|_| ())?;
    let nonce: [u8; 12] = nonce.try_into().map_err(|_| ())?;
    let (body, tag) = ciphertext.split_at(ciphertext.len() - 16);
    let round_keys = expand_key(&key);
    let h = aes_encrypt_block(&round_keys, &[0u8; 16]);
    let mut j0 = [0u8; 16];
    j0[..12].copy_from_slice(&nonce);
    j0[15] = 1;
    let s = ghash(&h, aad, body);
    let tag_mask = aes_encrypt_block(&round_keys, &j0);
    let mut expected = [0u8; 16];
    for i in 0..16 {
        expected[i] = s[i] ^ tag_mask[i];
    }
    let mut diff = 0u8;
    for (left, right) in expected.iter().zip(tag.iter()) {
        diff |= left ^ right;
    }
    if diff != 0 {
        return Err(());
    }
    let mut counter = j0;
    inc32(&mut counter);
    Ok(gctr(&round_keys, &counter, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        text.as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let digit = |byte: u8| {
                    if byte <= b'9' {
                        byte - b'0'
                    } else {
                        byte - b'a' + 10
                    }
                };
                digit(pair[0]) * 16 + digit(pair[1])
            })
            .collect()
    }

    #[test]
    fn nist_cavs_empty_payload() {
        let key = hex("b52c505a37d78eda5dd34f20c22540ea1b58963cf8e5bf8ffa85f9f2492505b4");
        let nonce = hex("516c33929df5a3284ff463d7");
        let expected = hex("bdc1ac884d332457a1d2664f168c76f0");
        let sealed = seal(&key, &nonce, &[], &[]).unwrap();
        assert_eq!(sealed, expected);
        assert_eq!(open(&key, &nonce, &sealed, &[]).unwrap(), Vec::<u8>::new());
    }
}
