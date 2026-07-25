//! Deterministic std-only crypto transforms used by comptime and the REPL.

mod Aes256Gcm;
mod Argon2id;
mod ChaCha20Poly1305;
mod Ed25519;
mod SHA512;

type Field = [i64; 16];

fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut key_block = [0u8; 64];
    if key.len() > key_block.len() {
        key_block[..32].copy_from_slice(&crate::SHA256::sha256(key));
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Vec::with_capacity(64 + data.len());
    inner.extend(key_block.iter().map(|byte| byte ^ 0x36));
    inner.extend_from_slice(data);
    let inner = crate::SHA256::sha256(&inner);
    let mut outer = Vec::with_capacity(96);
    outer.extend(key_block.iter().map(|byte| byte ^ 0x5c));
    outer.extend_from_slice(&inner);
    crate::SHA256::sha256(&outer)
}

pub(super) fn hkdf_sha256(ikm: &[u8], salt: &[u8], info: &[u8], length: usize) -> Vec<u8> {
    let zero_salt = [0u8; 32];
    let prk = hmac_sha256(if salt.is_empty() { &zero_salt } else { salt }, ikm);
    let mut output = Vec::with_capacity(length);
    let mut previous = Vec::new();
    for counter in 1..=((length + 31) / 32) {
        let mut input = Vec::with_capacity(previous.len() + info.len() + 1);
        input.extend_from_slice(&previous);
        input.extend_from_slice(info);
        input.push(counter as u8);
        previous = hmac_sha256(&prk, &input).to_vec();
        output.extend_from_slice(&previous);
    }
    output.truncate(length);
    output
}

fn carry(value: &mut Field) {
    for index in 0..16 {
        value[index] += 1 << 16;
        let overflow = value[index] >> 16;
        if index < 15 {
            value[index + 1] += overflow - 1;
        } else {
            value[0] += 38 * (overflow - 1);
        }
        value[index] -= overflow << 16;
    }
}

fn select(left: &mut Field, right: &mut Field, swap: i64) {
    let mask = !(swap - 1);
    for index in 0..16 {
        let change = mask & (left[index] ^ right[index]);
        left[index] ^= change;
        right[index] ^= change;
    }
}

fn add(out: &mut Field, left: &Field, right: &Field) {
    for index in 0..16 {
        out[index] = left[index] + right[index];
    }
}

fn sub(out: &mut Field, left: &Field, right: &Field) {
    for index in 0..16 {
        out[index] = left[index] - right[index];
    }
}

fn multiply(out: &mut Field, left: &Field, right: &Field) {
    let mut product = [0i64; 31];
    for i in 0..16 {
        for j in 0..16 {
            product[i + j] += left[i] * right[j];
        }
    }
    for index in 0..15 {
        product[index] += 38 * product[index + 16];
    }
    out.copy_from_slice(&product[..16]);
    carry(out);
    carry(out);
}

fn square(out: &mut Field, value: &Field) {
    multiply(out, value, value);
}

fn invert(out: &mut Field, value: &Field) {
    let mut result = *value;
    for exponent in (0..=253).rev() {
        let snapshot = result;
        square(&mut result, &snapshot);
        if exponent != 2 && exponent != 4 {
            let snapshot = result;
            multiply(&mut result, &snapshot, value);
        }
    }
    *out = result;
}

fn unpack(out: &mut Field, bytes: &[u8; 32]) {
    for index in 0..16 {
        out[index] = i64::from(bytes[index * 2]) + (i64::from(bytes[index * 2 + 1]) << 8);
    }
    out[15] &= 0x7fff;
}

fn pack(out: &mut [u8; 32], value: &Field) {
    let mut reduced = *value;
    carry(&mut reduced);
    carry(&mut reduced);
    carry(&mut reduced);
    for _ in 0..2 {
        let mut candidate = [0i64; 16];
        candidate[0] = reduced[0] - 0xffed;
        for index in 1..15 {
            candidate[index] = reduced[index] - 0xffff - ((candidate[index - 1] >> 16) & 1);
            candidate[index - 1] &= 0xffff;
        }
        candidate[15] = reduced[15] - 0x7fff - ((candidate[14] >> 16) & 1);
        let borrow = (candidate[15] >> 16) & 1;
        candidate[14] &= 0xffff;
        select(&mut reduced, &mut candidate, 1 - borrow);
    }
    for index in 0..16 {
        out[index * 2] = reduced[index] as u8;
        out[index * 2 + 1] = (reduced[index] >> 8) as u8;
    }
}

pub(super) fn x25519(secret: &[u8], public: &[u8]) -> Option<[u8; 32]> {
    let mut scalar: [u8; 32] = secret.try_into().ok()?;
    let public: [u8; 32] = public.try_into().ok()?;
    scalar[0] &= 248;
    scalar[31] &= 127;
    scalar[31] |= 64;

    let mut x = [0i64; 16];
    unpack(&mut x, &public);
    let mut a = [0i64; 16];
    let mut b = x;
    let mut c = [0i64; 16];
    let mut d = [0i64; 16];
    a[0] = 1;
    d[0] = 1;
    let constant = [121665, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    for bit in (0..=254).rev() {
        let swap = i64::from((scalar[bit >> 3] >> (bit & 7)) & 1);
        select(&mut a, &mut b, swap);
        select(&mut c, &mut d, swap);
        let (a0, b0, c0, d0) = (a, b, c, d);
        let mut e = [0i64; 16];
        let mut f = [0i64; 16];
        add(&mut e, &a0, &c0);
        sub(&mut a, &a0, &c0);
        add(&mut c, &b0, &d0);
        sub(&mut b, &b0, &d0);
        square(&mut d, &e);
        square(&mut f, &a);
        let (a0, b0, c0, e0) = (a, b, c, e);
        multiply(&mut a, &c0, &a0);
        multiply(&mut c, &b0, &e0);
        let (a0, c0) = (a, c);
        add(&mut e, &a0, &c0);
        sub(&mut a, &a0, &c0);
        square(&mut b, &a);
        let d0 = d;
        sub(&mut c, &d0, &f);
        let c0 = c;
        multiply(&mut a, &c0, &constant);
        let a0 = a;
        add(&mut a, &a0, &d0);
        let (a0, c0) = (a, c);
        multiply(&mut c, &c0, &a0);
        multiply(&mut a, &d0, &f);
        multiply(&mut d, &b, &x);
        square(&mut b, &e);
        select(&mut a, &mut b, swap);
        select(&mut c, &mut d, swap);
    }
    let c0 = c;
    invert(&mut c, &c0);
    let a0 = a;
    multiply(&mut a, &a0, &c);
    let mut output = [0u8; 32];
    pack(&mut output, &a);
    Some(output)
}

pub(super) fn ed25519_verify_strict(
    public: &[u8; 32],
    message: &[u8],
    signature: &[u8; 64],
) -> Result<bool, ()> {
    Ed25519::verify_strict(public, message, signature)
}

pub(super) fn ed25519_sign(seed: &[u8; 32], message: &[u8]) -> [u8; 64] {
    Ed25519::sign(seed, message)
}

pub(super) fn xchacha20poly1305_seal(
    key: &[u8],
    nonce: &[u8],
    plaintext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, ()> {
    ChaCha20Poly1305::xchacha20poly1305_seal(key, nonce, plaintext, aad)
}

pub(super) fn xchacha20poly1305_open(
    key: &[u8],
    nonce: &[u8],
    ciphertext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, ()> {
    ChaCha20Poly1305::xchacha20poly1305_open(key, nonce, ciphertext, aad)
}

pub(super) fn aes256gcm_seal(
    key: &[u8],
    nonce: &[u8],
    plaintext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, ()> {
    Aes256Gcm::seal(key, nonce, plaintext, aad)
}

pub(super) fn aes256gcm_open(
    key: &[u8],
    nonce: &[u8],
    ciphertext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, ()> {
    Aes256Gcm::open(key, nonce, ciphertext, aad)
}

pub(super) fn argon2id(
    password: &[u8],
    salt: &[u8],
    memory_kib: u32,
    iterations: u32,
    lanes: u32,
    output_length: usize,
) -> Result<Vec<u8>, ()> {
    Argon2id::hash(password, salt, memory_kib, iterations, lanes, output_length)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(hex: &str) -> Vec<u8> {
        hex.as_bytes()
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
    fn rfc_5869_hkdf_sha256_case_1() {
        let actual = hkdf_sha256(
            &[0x0b; 22],
            &bytes("000102030405060708090a0b0c"),
            &bytes("f0f1f2f3f4f5f6f7f8f9"),
            42,
        );
        assert_eq!(actual, bytes("3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865"));
    }

    #[test]
    fn rfc_7748_x25519_shared_secret() {
        let secret = bytes("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a");
        let public = bytes("de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f");
        assert_eq!(
            x25519(&secret, &public).unwrap().to_vec(),
            bytes("4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742")
        );
    }

    #[test]
    fn expert_aead_roundtrips() {
        let key = vec![0x11u8; 32];
        let xnonce = vec![0x24u8; 24];
        let anonce = vec![0x12u8; 12];
        let message = b"expert crypto".to_vec();
        let aad = b"tenant".to_vec();
        let xsealed = xchacha20poly1305_seal(&key, &xnonce, &message, &aad).unwrap();
        assert_eq!(xchacha20poly1305_open(&key, &xnonce, &xsealed, &aad).unwrap(), message);
        let asealed = aes256gcm_seal(&key, &anonce, &message, &aad).unwrap();
        assert_eq!(aes256gcm_open(&key, &anonce, &asealed, &aad).unwrap(), message);
    }

    #[test]
    fn rfc_8032_sign_empty() {
        let seed = bytes("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60");
        let expected = bytes("e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b");
        let seed: [u8; 32] = seed.try_into().unwrap();
        assert_eq!(ed25519_sign(&seed, b"").to_vec(), expected);
    }
}
