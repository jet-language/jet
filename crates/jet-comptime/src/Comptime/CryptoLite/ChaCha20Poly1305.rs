//! Std-only XChaCha20-Poly1305 (draft-irtf-cfrg-xchacha) matching RustCrypto.

fn quarter_round(state: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    state[a] = state[a].wrapping_add(state[b]);
    state[d] ^= state[a];
    state[d] = state[d].rotate_left(16);
    state[c] = state[c].wrapping_add(state[d]);
    state[b] ^= state[c];
    state[b] = state[b].rotate_left(12);
    state[a] = state[a].wrapping_add(state[b]);
    state[d] ^= state[a];
    state[d] = state[d].rotate_left(8);
    state[c] = state[c].wrapping_add(state[d]);
    state[b] ^= state[c];
    state[b] = state[b].rotate_left(7);
}

fn chacha20_block(key: &[u8; 32], counter: u32, nonce: &[u8; 12]) -> [u8; 64] {
    let mut state = [
        0x6170_7865,
        0x3320_646e,
        0x7962_2d32,
        0x6b20_6574,
        u32::from_le_bytes(key[0..4].try_into().unwrap()),
        u32::from_le_bytes(key[4..8].try_into().unwrap()),
        u32::from_le_bytes(key[8..12].try_into().unwrap()),
        u32::from_le_bytes(key[12..16].try_into().unwrap()),
        u32::from_le_bytes(key[16..20].try_into().unwrap()),
        u32::from_le_bytes(key[20..24].try_into().unwrap()),
        u32::from_le_bytes(key[24..28].try_into().unwrap()),
        u32::from_le_bytes(key[28..32].try_into().unwrap()),
        counter,
        u32::from_le_bytes(nonce[0..4].try_into().unwrap()),
        u32::from_le_bytes(nonce[4..8].try_into().unwrap()),
        u32::from_le_bytes(nonce[8..12].try_into().unwrap()),
    ];
    let initial = state;
    for _ in 0..10 {
        quarter_round(&mut state, 0, 4, 8, 12);
        quarter_round(&mut state, 1, 5, 9, 13);
        quarter_round(&mut state, 2, 6, 10, 14);
        quarter_round(&mut state, 3, 7, 11, 15);
        quarter_round(&mut state, 0, 5, 10, 15);
        quarter_round(&mut state, 1, 6, 11, 12);
        quarter_round(&mut state, 2, 7, 8, 13);
        quarter_round(&mut state, 3, 4, 9, 14);
    }
    let mut out = [0u8; 64];
    for (index, word) in state.iter_mut().enumerate() {
        *word = word.wrapping_add(initial[index]);
        out[index * 4..(index + 1) * 4].copy_from_slice(&word.to_le_bytes());
    }
    out
}

fn hchacha20(key: &[u8; 32], nonce: &[u8; 16]) -> [u8; 32] {
    let mut state = [
        0x6170_7865,
        0x3320_646e,
        0x7962_2d32,
        0x6b20_6574,
        u32::from_le_bytes(key[0..4].try_into().unwrap()),
        u32::from_le_bytes(key[4..8].try_into().unwrap()),
        u32::from_le_bytes(key[8..12].try_into().unwrap()),
        u32::from_le_bytes(key[12..16].try_into().unwrap()),
        u32::from_le_bytes(key[16..20].try_into().unwrap()),
        u32::from_le_bytes(key[20..24].try_into().unwrap()),
        u32::from_le_bytes(key[24..28].try_into().unwrap()),
        u32::from_le_bytes(key[28..32].try_into().unwrap()),
        u32::from_le_bytes(nonce[0..4].try_into().unwrap()),
        u32::from_le_bytes(nonce[4..8].try_into().unwrap()),
        u32::from_le_bytes(nonce[8..12].try_into().unwrap()),
        u32::from_le_bytes(nonce[12..16].try_into().unwrap()),
    ];
    for _ in 0..10 {
        quarter_round(&mut state, 0, 4, 8, 12);
        quarter_round(&mut state, 1, 5, 9, 13);
        quarter_round(&mut state, 2, 6, 10, 14);
        quarter_round(&mut state, 3, 7, 11, 15);
        quarter_round(&mut state, 0, 5, 10, 15);
        quarter_round(&mut state, 1, 6, 11, 12);
        quarter_round(&mut state, 2, 7, 8, 13);
        quarter_round(&mut state, 3, 4, 9, 14);
    }
    let mut out = [0u8; 32];
    for (index, word) in [
        state[0], state[1], state[2], state[3], state[12], state[13], state[14], state[15],
    ]
    .into_iter()
    .enumerate()
    {
        out[index * 4..(index + 1) * 4].copy_from_slice(&word.to_le_bytes());
    }
    out
}

fn chacha20_xor(key: &[u8; 32], nonce: &[u8; 12], counter: u32, input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut block_counter = counter;
    for chunk in input.chunks(64) {
        let keystream = chacha20_block(key, block_counter, nonce);
        for (byte, mask) in chunk.iter().zip(keystream.iter()) {
            out.push(byte ^ mask);
        }
        block_counter = block_counter.wrapping_add(1);
    }
    out
}

/// Poly1305 (26-bit limbs) matching RustCrypto `poly1305` soft backend.
fn poly1305_mac(key: &[u8; 32], message: &[u8]) -> [u8; 16] {
    let mut r = [0u32; 5];
    let mut h = [0u32; 5];
    let mut pad = [0u32; 4];
    r[0] = u32::from_le_bytes(key[0..4].try_into().unwrap()) & 0x3ff_ffff;
    r[1] = (u32::from_le_bytes(key[3..7].try_into().unwrap()) >> 2) & 0x3ff_ff03;
    r[2] = (u32::from_le_bytes(key[6..10].try_into().unwrap()) >> 4) & 0x3ff_c0ff;
    r[3] = (u32::from_le_bytes(key[9..13].try_into().unwrap()) >> 6) & 0x3f0_3fff;
    r[4] = (u32::from_le_bytes(key[12..16].try_into().unwrap()) >> 8) & 0x00f_ffff;
    pad[0] = u32::from_le_bytes(key[16..20].try_into().unwrap());
    pad[1] = u32::from_le_bytes(key[20..24].try_into().unwrap());
    pad[2] = u32::from_le_bytes(key[24..28].try_into().unwrap());
    pad[3] = u32::from_le_bytes(key[28..32].try_into().unwrap());

    let compute_block = |h: &mut [u32; 5], block: &[u8; 16], partial: bool| {
        let hibit = if partial { 0 } else { 1 << 24 };
        let r0 = r[0];
        let r1 = r[1];
        let r2 = r[2];
        let r3 = r[3];
        let r4 = r[4];
        let s1 = r1 * 5;
        let s2 = r2 * 5;
        let s3 = r3 * 5;
        let s4 = r4 * 5;

        let mut h0 = h[0];
        let mut h1 = h[1];
        let mut h2 = h[2];
        let mut h3 = h[3];
        let mut h4 = h[4];

        h0 += u32::from_le_bytes(block[0..4].try_into().unwrap()) & 0x3ff_ffff;
        h1 += (u32::from_le_bytes(block[3..7].try_into().unwrap()) >> 2) & 0x3ff_ffff;
        h2 += (u32::from_le_bytes(block[6..10].try_into().unwrap()) >> 4) & 0x3ff_ffff;
        h3 += (u32::from_le_bytes(block[9..13].try_into().unwrap()) >> 6) & 0x3ff_ffff;
        h4 += (u32::from_le_bytes(block[12..16].try_into().unwrap()) >> 8) | hibit;

        let d0 = u64::from(h0) * u64::from(r0)
            + u64::from(h1) * u64::from(s4)
            + u64::from(h2) * u64::from(s3)
            + u64::from(h3) * u64::from(s2)
            + u64::from(h4) * u64::from(s1);
        let mut d1 = u64::from(h0) * u64::from(r1)
            + u64::from(h1) * u64::from(r0)
            + u64::from(h2) * u64::from(s4)
            + u64::from(h3) * u64::from(s3)
            + u64::from(h4) * u64::from(s2);
        let mut d2 = u64::from(h0) * u64::from(r2)
            + u64::from(h1) * u64::from(r1)
            + u64::from(h2) * u64::from(r0)
            + u64::from(h3) * u64::from(s4)
            + u64::from(h4) * u64::from(s3);
        let mut d3 = u64::from(h0) * u64::from(r3)
            + u64::from(h1) * u64::from(r2)
            + u64::from(h2) * u64::from(r1)
            + u64::from(h3) * u64::from(r0)
            + u64::from(h4) * u64::from(s4);
        let mut d4 = u64::from(h0) * u64::from(r4)
            + u64::from(h1) * u64::from(r3)
            + u64::from(h2) * u64::from(r2)
            + u64::from(h3) * u64::from(r1)
            + u64::from(h4) * u64::from(r0);

        let mut c = (d0 >> 26) as u32;
        h0 = d0 as u32 & 0x3ff_ffff;
        d1 += u64::from(c);
        c = (d1 >> 26) as u32;
        h1 = d1 as u32 & 0x3ff_ffff;
        d2 += u64::from(c);
        c = (d2 >> 26) as u32;
        h2 = d2 as u32 & 0x3ff_ffff;
        d3 += u64::from(c);
        c = (d3 >> 26) as u32;
        h3 = d3 as u32 & 0x3ff_ffff;
        d4 += u64::from(c);
        c = (d4 >> 26) as u32;
        h4 = d4 as u32 & 0x3ff_ffff;
        h0 += c * 5;
        c = h0 >> 26;
        h0 &= 0x3ff_ffff;
        h1 += c;

        h[0] = h0;
        h[1] = h1;
        h[2] = h2;
        h[3] = h3;
        h[4] = h4;
    };

    let mut offset = 0usize;
    while offset + 16 <= message.len() {
        let block: [u8; 16] = message[offset..offset + 16].try_into().unwrap();
        compute_block(&mut h, &block, false);
        offset += 16;
    }
    if offset < message.len() {
        let mut block = [0u8; 16];
        let rem = &message[offset..];
        block[..rem.len()].copy_from_slice(rem);
        block[rem.len()] = 1;
        compute_block(&mut h, &block, true);
    }

    let mut h0 = h[0];
    let mut h1 = h[1];
    let mut h2 = h[2];
    let mut h3 = h[3];
    let mut h4 = h[4];

    let mut c = h1 >> 26;
    h1 &= 0x3ff_ffff;
    h2 += c;
    c = h2 >> 26;
    h2 &= 0x3ff_ffff;
    h3 += c;
    c = h3 >> 26;
    h3 &= 0x3ff_ffff;
    h4 += c;
    c = h4 >> 26;
    h4 &= 0x3ff_ffff;
    h0 += c * 5;
    c = h0 >> 26;
    h0 &= 0x3ff_ffff;
    h1 += c;

    let mut g0 = h0.wrapping_add(5);
    c = g0 >> 26;
    g0 &= 0x3ff_ffff;
    let mut g1 = h1.wrapping_add(c);
    c = g1 >> 26;
    g1 &= 0x3ff_ffff;
    let mut g2 = h2.wrapping_add(c);
    c = g2 >> 26;
    g2 &= 0x3ff_ffff;
    let mut g3 = h3.wrapping_add(c);
    c = g3 >> 26;
    g3 &= 0x3ff_ffff;
    let mut g4 = h4.wrapping_add(c).wrapping_sub(1 << 26);

    let mut mask = (g4 >> 31).wrapping_sub(1);
    g0 &= mask;
    g1 &= mask;
    g2 &= mask;
    g3 &= mask;
    g4 &= mask;
    mask = !mask;
    h0 = (h0 & mask) | g0;
    h1 = (h1 & mask) | g1;
    h2 = (h2 & mask) | g2;
    h3 = (h3 & mask) | g3;
    h4 = (h4 & mask) | g4;

    h0 |= h1 << 26;
    h1 = (h1 >> 6) | (h2 << 20);
    h2 = (h2 >> 12) | (h3 << 14);
    h3 = (h3 >> 18) | (h4 << 8);

    let mut f = u64::from(h0) + u64::from(pad[0]);
    h0 = f as u32;
    f = u64::from(h1) + u64::from(pad[1]) + (f >> 32);
    h1 = f as u32;
    f = u64::from(h2) + u64::from(pad[2]) + (f >> 32);
    h2 = f as u32;
    f = u64::from(h3) + u64::from(pad[3]) + (f >> 32);
    h3 = f as u32;

    let mut tag = [0u8; 16];
    tag[0..4].copy_from_slice(&h0.to_le_bytes());
    tag[4..8].copy_from_slice(&h1.to_le_bytes());
    tag[8..12].copy_from_slice(&h2.to_le_bytes());
    tag[12..16].copy_from_slice(&h3.to_le_bytes());
    tag
}

fn pad16(buf: &mut Vec<u8>) {
    while buf.len() % 16 != 0 {
        buf.push(0);
    }
}

fn chacha20poly1305_seal(key: &[u8; 32], nonce: &[u8; 12], plaintext: &[u8], aad: &[u8]) -> Vec<u8> {
    let mut poly_key = [0u8; 32];
    poly_key.copy_from_slice(&chacha20_block(key, 0, nonce)[..32]);
    let ciphertext = chacha20_xor(key, nonce, 1, plaintext);
    let mut mac_data = Vec::with_capacity(aad.len() + 16 + ciphertext.len() + 16 + 16);
    mac_data.extend_from_slice(aad);
    pad16(&mut mac_data);
    mac_data.extend_from_slice(&ciphertext);
    pad16(&mut mac_data);
    mac_data.extend_from_slice(&(aad.len() as u64).to_le_bytes());
    mac_data.extend_from_slice(&(ciphertext.len() as u64).to_le_bytes());
    let tag = poly1305_mac(&poly_key, &mac_data);
    let mut out = ciphertext;
    out.extend_from_slice(&tag);
    out
}

fn chacha20poly1305_open(
    key: &[u8; 32],
    nonce: &[u8; 12],
    ciphertext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, ()> {
    if ciphertext.len() < 16 {
        return Err(());
    }
    let (body, tag) = ciphertext.split_at(ciphertext.len() - 16);
    let mut poly_key = [0u8; 32];
    poly_key.copy_from_slice(&chacha20_block(key, 0, nonce)[..32]);
    let mut mac_data = Vec::with_capacity(aad.len() + 16 + body.len() + 16 + 16);
    mac_data.extend_from_slice(aad);
    pad16(&mut mac_data);
    mac_data.extend_from_slice(body);
    pad16(&mut mac_data);
    mac_data.extend_from_slice(&(aad.len() as u64).to_le_bytes());
    mac_data.extend_from_slice(&(body.len() as u64).to_le_bytes());
    let expected = poly1305_mac(&poly_key, &mac_data);
    let mut diff = 0u8;
    for (left, right) in expected.iter().zip(tag.iter()) {
        diff |= left ^ right;
    }
    if diff != 0 {
        return Err(());
    }
    Ok(chacha20_xor(key, nonce, 1, body))
}

pub(super) fn xchacha20poly1305_seal(
    key: &[u8],
    nonce: &[u8],
    plaintext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, ()> {
    let key: [u8; 32] = key.try_into().map_err(|_| ())?;
    let nonce: [u8; 24] = nonce.try_into().map_err(|_| ())?;
    let subkey = hchacha20(&key, nonce[..16].try_into().unwrap());
    let mut sub_nonce = [0u8; 12];
    sub_nonce[4..].copy_from_slice(&nonce[16..]);
    Ok(chacha20poly1305_seal(&subkey, &sub_nonce, plaintext, aad))
}

pub(super) fn xchacha20poly1305_open(
    key: &[u8],
    nonce: &[u8],
    ciphertext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, ()> {
    let key: [u8; 32] = key.try_into().map_err(|_| ())?;
    let nonce: [u8; 24] = nonce.try_into().map_err(|_| ())?;
    let subkey = hchacha20(&key, nonce[..16].try_into().unwrap());
    let mut sub_nonce = [0u8; 12];
    sub_nonce[4..].copy_from_slice(&nonce[16..]);
    chacha20poly1305_open(&subkey, &sub_nonce, ciphertext, aad)
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
    fn cfrg_xchacha_appendix_a31() {
        let key = hex("808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f");
        let nonce = hex("404142434445464748494a4b4c4d4e4f5051525354555657");
        let plaintext = hex(concat!(
            "4c616469657320616e642047656e746c656d656e206f662074686520636c6173",
            "73206f66202739393a204966204920636f756c64206f6666657220796f75206f",
            "6e6c79206f6e652074697020666f7220746865206675747572652c2073756e73",
            "637265656e20776f756c642062652069742e",
        ));
        let aad = hex("50515253c0c1c2c3c4c5c6c7");
        let expected = hex(concat!(
            "bd6d179d3e83d43b9576579493c0e939572a1700252bfaccbed2902c21396cbb",
            "731c7f1b0b4aa6440bf3a82f4eda7e39ae64c6708c54c216cb96b72e1213b452",
            "2f8c9ba40db5d945b11b69b982c1bb9e3f3fac2bc369488f76b2383565d3fff9",
            "21f9664c97637da9768812f615c68b13b52e",
            "c0875924c1c7987947deafd8780acf49",
        ));
        let sealed = xchacha20poly1305_seal(&key, &nonce, &plaintext, &aad).unwrap();
        assert_eq!(sealed, expected);
        assert_eq!(
            xchacha20poly1305_open(&key, &nonce, &sealed, &aad).unwrap(),
            plaintext
        );
    }
}
