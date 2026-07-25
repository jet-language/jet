//! Dependency-free Ed25519 verification matching `ed25519_dalek::verify_strict`.

use super::{add, multiply, pack, select, square, sub, Field};

type Point = [Field; 4];

const ZERO: Field = [0; 16];
const ONE: Field = [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
const D: Field = [
    0x78a3, 0x1359, 0x4dca, 0x75eb, 0xd8ab, 0x4141, 0x0a4d, 0x0070, 0xe898, 0x7779, 0x4079, 0x8cc7,
    0xfe73, 0x2b6f, 0x6cee, 0x5203,
];
const D2: Field = [
    0xf159, 0x26b2, 0x9b94, 0xebd6, 0xb156, 0x8283, 0x149a, 0x00e0, 0xd130, 0xeef3, 0x80f2, 0x198e,
    0xfce7, 0x56df, 0xd9dc, 0x2406,
];
const BASE_X: Field = [
    0xd51a, 0x8f25, 0x2d60, 0xc956, 0xa7b2, 0x9525, 0xc760, 0x692c, 0xdc5c, 0xfdd6, 0xe231, 0xc0a4,
    0x53fe, 0xcd6e, 0x36d3, 0x2169,
];
const BASE_Y: Field = [
    0x6658, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666,
    0x6666, 0x6666, 0x6666, 0x6666,
];
const SQRT_M1: Field = [
    0xa0b0, 0x4a0e, 0x1b27, 0xc4ee, 0xe478, 0xad2f, 0x1806, 0x2f43, 0xd7a7, 0x3dfb, 0x0099, 0x2b4d,
    0xdf0b, 0x4fc1, 0x2480, 0x2b83,
];
const ORDER: [u8; 32] = [
    0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde, 0x14,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10,
];

pub(super) fn verify_strict(
    public: &[u8; 32],
    message: &[u8],
    signature: &[u8; 64],
) -> Result<bool, ()> {
    if !scalar_is_canonical(&signature[32..]) {
        return Ok(false);
    }
    let mut public_point = decode_neg(public).ok_or(())?;
    let signature_r: [u8; 32] = signature[..32].try_into().unwrap();
    let signature_point = match decode_neg(&signature_r) {
        Some(point) => point,
        None => return Ok(false),
    };
    if is_small_order(public_point) || is_small_order(signature_point) {
        return Ok(false);
    }

    let mut hash_input = Vec::with_capacity(64 + message.len());
    hash_input.extend_from_slice(&signature_r);
    hash_input.extend_from_slice(public);
    hash_input.extend_from_slice(message);
    let mut challenge = super::SHA512::digest(&hash_input);
    reduce(&mut challenge);

    scalar_multiply_in_place(&mut public_point, &challenge);
    let mut scalar_base = basepoint_multiply(&signature[32..]);
    point_add(&mut public_point, &mut scalar_base);
    Ok(encode(&public_point) == signature_r)
}

/// RFC 8032 §5.1.6 Ed25519 signing from a 32-byte seed.
pub(super) fn sign(seed: &[u8; 32], message: &[u8]) -> [u8; 64] {
    let digest = super::SHA512::digest(seed);
    let mut az = digest;
    az[0] &= 248;
    az[31] &= 63;
    az[31] |= 64;
    let public = encode(&basepoint_multiply(&az[..32]));

    let mut nonce_input = Vec::with_capacity(32 + message.len());
    nonce_input.extend_from_slice(&digest[32..]);
    nonce_input.extend_from_slice(message);
    let mut nonce = super::SHA512::digest(&nonce_input);
    reduce(&mut nonce);
    let r_point = encode(&basepoint_multiply(&nonce[..32]));

    let mut challenge_input = Vec::with_capacity(64 + message.len());
    challenge_input.extend_from_slice(&r_point);
    challenge_input.extend_from_slice(&public);
    challenge_input.extend_from_slice(message);
    let mut challenge = super::SHA512::digest(&challenge_input);
    reduce(&mut challenge);

    let mut signature = [0u8; 64];
    signature[..32].copy_from_slice(&r_point);
    // S = (r + k*a) mod L
    let mut product = [0i64; 64];
    for i in 0..32 {
        for j in 0..32 {
            product[i + j] += i64::from(challenge[i]) * i64::from(az[j]);
        }
    }
    for i in 0..32 {
        product[i] += i64::from(nonce[i]);
    }
    let mut carry = 0i64;
    for slot in &mut product {
        let value = *slot + carry;
        *slot = value & 255;
        carry = value >> 8;
    }
    let mut reduced = [0u8; 32];
    mod_order(&mut reduced, &mut product);
    signature[32..].copy_from_slice(&reduced);
    signature
}

fn scalar_is_canonical(scalar: &[u8]) -> bool {
    for index in (0..32).rev() {
        if scalar[index] != ORDER[index] {
            return scalar[index] < ORDER[index];
        }
    }
    false
}

fn decode_neg(bytes: &[u8; 32]) -> Option<Point> {
    let mut point = [ZERO, ZERO, ONE, ZERO];
    unpack(&mut point[1], bytes);
    let mut numerator = ZERO;
    square(&mut numerator, &point[1]);
    let mut denominator = ZERO;
    multiply(&mut denominator, &numerator, &D);
    let numerator0 = numerator;
    sub(&mut numerator, &numerator0, &ONE);
    let denominator0 = denominator;
    add(&mut denominator, &denominator0, &ONE);

    let mut denominator2 = ZERO;
    square(&mut denominator2, &denominator);
    let mut denominator4 = ZERO;
    square(&mut denominator4, &denominator2);
    let mut denominator6 = ZERO;
    multiply(&mut denominator6, &denominator4, &denominator2);
    let mut root = ZERO;
    multiply(&mut root, &denominator6, &numerator);
    let root0 = root;
    multiply(&mut root, &root0, &denominator);
    let root0 = root;
    pow2523(&mut root, &root0);
    let root0 = root;
    multiply(&mut root, &root0, &numerator);
    let root0 = root;
    multiply(&mut root, &root0, &denominator);
    let root0 = root;
    multiply(&mut root, &root0, &denominator);
    multiply(&mut point[0], &root, &denominator);

    let mut check = ZERO;
    square(&mut check, &point[0]);
    let check0 = check;
    multiply(&mut check, &check0, &denominator);
    if !field_equal(&check, &numerator) {
        let x = point[0];
        multiply(&mut point[0], &x, &SQRT_M1);
    }
    square(&mut check, &point[0]);
    let check0 = check;
    multiply(&mut check, &check0, &denominator);
    if !field_equal(&check, &numerator) {
        return None;
    }
    if field_parity(&point[0]) == (bytes[31] >> 7) {
        let x = point[0];
        sub(&mut point[0], &ZERO, &x);
    }
    let x = point[0];
    let y = point[1];
    multiply(&mut point[3], &x, &y);
    Some(point)
}

fn unpack(out: &mut Field, bytes: &[u8; 32]) {
    for index in 0..16 {
        out[index] = i64::from(bytes[index * 2]) + (i64::from(bytes[index * 2 + 1]) << 8);
    }
    out[15] &= 0x7fff;
}

fn pow2523(out: &mut Field, value: &Field) {
    let mut result = *value;
    for exponent in (0..=250).rev() {
        let snapshot = result;
        square(&mut result, &snapshot);
        if exponent != 1 {
            let snapshot = result;
            multiply(&mut result, &snapshot, value);
        }
    }
    *out = result;
}

fn field_equal(left: &Field, right: &Field) -> bool {
    let mut left_bytes = [0; 32];
    let mut right_bytes = [0; 32];
    pack(&mut left_bytes, left);
    pack(&mut right_bytes, right);
    left_bytes == right_bytes
}

fn field_parity(value: &Field) -> u8 {
    let mut bytes = [0; 32];
    pack(&mut bytes, value);
    bytes[0] & 1
}

fn point_add(left: &mut Point, right: &mut Point) {
    let mut a = ZERO;
    let mut b = ZERO;
    let mut c = ZERO;
    let mut d = ZERO;
    let mut e = ZERO;
    let mut f = ZERO;
    let mut g = ZERO;
    let mut h = ZERO;
    let mut temp = ZERO;
    sub(&mut a, &left[1], &left[0]);
    sub(&mut temp, &right[1], &right[0]);
    let a0 = a;
    multiply(&mut a, &a0, &temp);
    add(&mut b, &left[0], &left[1]);
    add(&mut temp, &right[0], &right[1]);
    let b0 = b;
    multiply(&mut b, &b0, &temp);
    multiply(&mut c, &left[3], &right[3]);
    let c0 = c;
    multiply(&mut c, &c0, &D2);
    multiply(&mut d, &left[2], &right[2]);
    let d0 = d;
    add(&mut d, &d0, &d0);
    sub(&mut e, &b, &a);
    sub(&mut f, &d, &c);
    add(&mut g, &d, &c);
    add(&mut h, &b, &a);
    multiply(&mut left[0], &e, &f);
    multiply(&mut left[1], &h, &g);
    multiply(&mut left[2], &g, &f);
    multiply(&mut left[3], &e, &h);
}

fn point_select(left: &mut Point, right: &mut Point, swap: i64) {
    for index in 0..4 {
        select(&mut left[index], &mut right[index], swap);
    }
}

fn scalar_multiply(point: &mut Point, scalar: &[u8]) {
    let mut result = [ZERO, ONE, ONE, ZERO];
    for bit in (0..=255).rev() {
        let swap = i64::from((scalar[bit >> 3] >> (bit & 7)) & 1);
        point_select(&mut result, point, swap);
        point_add(point, &mut result);
        let mut duplicate = result;
        point_add(&mut result, &mut duplicate);
        point_select(&mut result, point, swap);
    }
    *point = result;
}

fn scalar_multiply_in_place(point: &mut Point, scalar: &[u8; 64]) {
    scalar_multiply(point, scalar);
}

fn basepoint_multiply(scalar: &[u8]) -> Point {
    let mut point = [BASE_X, BASE_Y, ONE, ZERO];
    multiply(&mut point[3], &BASE_X, &BASE_Y);
    scalar_multiply(&mut point, scalar);
    point
}

fn encode(point: &Point) -> [u8; 32] {
    let mut inverse = ZERO;
    super::invert(&mut inverse, &point[2]);
    let mut x = ZERO;
    let mut y = ZERO;
    multiply(&mut x, &point[0], &inverse);
    multiply(&mut y, &point[1], &inverse);
    let mut output = [0; 32];
    pack(&mut output, &y);
    output[31] ^= field_parity(&x) << 7;
    output
}

fn is_small_order(mut point: Point) -> bool {
    for _ in 0..3 {
        let mut copy = point;
        point_add(&mut point, &mut copy);
    }
    encode(&point)
        == [
            1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ]
}

fn reduce(bytes: &mut [u8; 64]) {
    let mut wide = [0_i64; 64];
    for (out, byte) in wide.iter_mut().zip(bytes.iter()) {
        *out = i64::from(*byte);
    }
    let mut reduced = [0; 32];
    mod_order(&mut reduced, &mut wide);
    bytes[..32].copy_from_slice(&reduced);
}

fn mod_order(out: &mut [u8; 32], value: &mut [i64; 64]) {
    for index in (32..64).rev() {
        let mut carry = 0;
        let mut target = index - 32;
        while target < index - 12 {
            value[target] += carry - 16 * value[index] * i64::from(ORDER[target - (index - 32)]);
            carry = (value[target] + 128) >> 8;
            value[target] -= carry << 8;
            target += 1;
        }
        value[target] += carry;
        value[index] = 0;
    }
    let mut carry = 0;
    for index in 0..32 {
        value[index] += carry - (value[31] >> 4) * i64::from(ORDER[index]);
        carry = value[index] >> 8;
        value[index] &= 255;
    }
    for index in 0..32 {
        value[index] -= carry * i64::from(ORDER[index]);
    }
    for index in 0..32 {
        value[index + 1] += value[index] >> 8;
        out[index] = value[index] as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes<const N: usize>(hex: &str) -> [u8; N] {
        let decoded = hex
            .as_bytes()
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
            .collect::<Vec<_>>();
        decoded.try_into().ok().unwrap()
    }

    #[test]
    fn rfc_8032_empty_message() {
        let public = bytes("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a");
        let signature = bytes(
            "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e06522490155\
                               5fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
        );
        assert_eq!(verify_strict(&public, b"", &signature), Ok(true));
    }

    #[test]
    fn rfc_8032_sign_empty_message() {
        let seed = bytes::<32>("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60");
        let expected = bytes::<64>(
            "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e06522490155\
             5fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
        );
        let signature = sign(&seed, b"");
        assert_eq!(signature, expected);
        let public = bytes::<32>("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a");
        assert_eq!(verify_strict(&public, b"", &signature), Ok(true));
    }

    #[test]
    fn strict_rejects_modified_and_weak_signatures() {
        let public = bytes("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a");
        let mut signature = bytes("e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e06522490155\
                                   5fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b");
        signature[0] ^= 1;
        assert_eq!(verify_strict(&public, b"", &signature), Ok(false));
        let weak = [
            1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ];
        assert_eq!(verify_strict(&weak, b"", &signature), Ok(false));
    }
}
