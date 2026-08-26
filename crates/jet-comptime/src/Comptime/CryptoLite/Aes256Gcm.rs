//! Std-only AES-256-GCM matching RustCrypto / NIST CAVS.

const RCON: [u8; 10] = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36];

// AES's lookup table is indexed by secret state/key bytes. Use fixed-round
// GF(2^8) arithmetic instead so the comptime/interpreter path has no
// secret-dependent memory access.
fn gf_mul(mut left: u8, mut right: u8) -> u8 {
    let mut product = 0u8;
    for _ in 0..8 {
        let right_mask = 0u8.wrapping_sub(right & 1);
        product ^= left & right_mask;
        let high_mask = 0u8.wrapping_sub(left >> 7);
        left = (left << 1) ^ (0x1b & high_mask);
        right >>= 1;
    }
    product
}

fn aes_sbox(value: u8) -> u8 {
    // The AES S-box is the multiplicative inverse followed by a fixed affine
    // transform. Every operation below has a public, fixed iteration count.
    let x2 = gf_mul(value, value);
    let x4 = gf_mul(x2, x2);
    let x8 = gf_mul(x4, x4);
    let x16 = gf_mul(x8, x8);
    let x32 = gf_mul(x16, x16);
    let x64 = gf_mul(x32, x32);
    let x128 = gf_mul(x64, x64);
    let inverse = gf_mul(
        gf_mul(gf_mul(x2, x4), gf_mul(x8, x16)),
        gf_mul(x32, gf_mul(x64, x128)),
    );
    inverse
        ^ inverse.rotate_left(1)
        ^ inverse.rotate_left(2)
        ^ inverse.rotate_left(3)
        ^ inverse.rotate_left(4)
        ^ 0x63
}

fn mul(mut left: u8, mut right: u8) -> u8 {
    let mut product = 0u8;
    for _ in 0..8 {
        let right_mask = 0u8.wrapping_sub(right & 1);
        product ^= left & right_mask;
        let high_mask = 0u8.wrapping_sub(left >> 7);
        left = (left << 1) ^ (0x1b & high_mask);
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
                *byte = aes_sbox(*byte);
            }
            temp[0] ^= RCON[i / 8 - 1];
        } else if i % 8 == 4 {
            for byte in &mut temp {
                *byte = aes_sbox(*byte);
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
        *byte = aes_sbox(*byte);
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
            let bit_mask = 0u8.wrapping_sub((byte >> bit) & 1);
            for i in 0..16 {
                z[i] ^= v[i] & bit_mask;
            }
            let lsb = v[15] & 1;
            for i in (1..16).rev() {
                v[i] = (v[i] >> 1) | (v[i - 1] << 7);
            }
            v[0] >>= 1;
            v[0] ^= 0xe1 & 0u8.wrapping_sub(lsb);
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
