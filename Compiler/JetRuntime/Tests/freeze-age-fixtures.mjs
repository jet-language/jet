#!/usr/bin/env node
// TEST ONLY. Freeze age0.10.1 Rust outputs + bundled official age testkit.
// No Rust/reference implementation or entropy fallback enters the runtime.
// Recipe: node freeze-age-fixtures.mjs RUST_FIXTURE_DIR AGE_TESTKIT_DIR OUT.json
import { readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { createPrivateKey, createPublicKey, diffieHellman, hkdfSync, createHmac, createCipheriv, createHash } from 'node:crypto';
const [rust, testkit, output] = process.argv.slice(2);
if (!rust || !testkit || !output || process.argv.length !== 5) throw new Error('usage: freeze-age-fixtures.mjs RUST_FIXTURE_DIR AGE_TESTKIT_DIR OUT.json');
const identity = 'AGE-SECRET-KEY-1GQ9778VQXMMJVE8SK7J6VT8UJ4HDQAJUVSFCWCM02D8GEWQ72PVQ2Y5J33';
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const cases = [];
for (const length of [0, 1, 65535, 65536, 65537, 131072]) {
    const cipher = readFileSync(join(rust, `age-rust-${length}.age`));
    const plain = readFileSync(join(rust, `age-rust-${length}.plain`));
    cases.push({name: `rust-0.10.1-${length}`, identity, valid: true, cipher: cipher.toString('base64'), cipherSha256: digest(cipher), plainSha256: digest(plain)});
}
for (const name of readdirSync(testkit).sort()) {
    const providerPassphraseReject = name === 'scrypt' || name === 'scrypt_and_x25519';
    if (!providerPassphraseReject && !/^(header_|hmac_|stanza_|stream_|version_|x25519)/u.test(name)) continue;
    const bytes = readFileSync(join(testkit, name));
    const separator = bytes.indexOf('\n\n');
    if (separator < 0) throw new Error(`missing testkit metadata separator: ${name}`);
    const metadata = bytes.subarray(0, separator).toString('ascii');
    const key = /^identity: (AGE-SECRET-KEY-\S+)$/mu.exec(metadata)?.[1] ?? identity;
    const expect = /^expect: ([^\r\n]+)$/mu.exec(metadata)?.[1];
    if (!expect) throw new Error(`missing expect: ${name}`);
    // age0.10.1/tests/testkit.rs:617-620 deliberately accepts these beta-era
    // stanzas despite the spec testkit's header-failure metadata.
    const seedLegacyAcceptance = ['stanza_missing_body', 'stanza_missing_final_line'].includes(name);
    const valid = !providerPassphraseReject && (expect === 'success' || seedLegacyAcceptance);
    const plainSha256 = /^payload: ([0-9a-f]{64})$/mu.exec(metadata)?.[1] ?? null;
    if (valid && !seedLegacyAcceptance && !plainSha256) throw new Error(`missing payload hash: ${name}`);
    const cipher = bytes.subarray(separator + 2);
    cases.push({name: `age-testkit-${name}`, identity: key, valid, providerPassphraseReject, seedLegacyAcceptance, cipher: cipher.toString('base64'), cipherSha256: digest(cipher), plainSha256});
}
// Independent deterministic age-v1 seal reference; Node/OpenSSL built-ins only.
// Fixed material is TEST DATA, not runtime randomness. Two identical recipients
// with distinct ephemeral scalars exercise multiple recipient framing.
const alphabet = 'qpzry9x8gf2tvdw0s3jn54khce6mua7l';
const groups = identity.toLowerCase().slice(16, -6).split('').map(c => alphabet.indexOf(c));
let bits = 0, accumulator = 0;
const secretBytes = [];
for (const value of groups) {
    if (value < 0) throw new Error('bad fixed identity');
    accumulator = (accumulator << 5) | value; bits += 5;
    if (bits >= 8) { bits -= 8; secretBytes.push((accumulator >>> bits) & 255); }
}
if (secretBytes.length !== 32) throw new Error('bad fixed identity length');
const privateKey = bytes => createPrivateKey({key: Buffer.concat([Buffer.from('302e020100300506032b656e04220420', 'hex'), bytes]), format: 'der', type: 'pkcs8'});
const publicBytes = key => createPublicKey(key).export({format: 'der', type: 'spki'}).subarray(-32);
const recipientKey = createPublicKey(privateKey(Buffer.from(secretBytes)));
const recipientBytes = publicBytes(privateKey(Buffer.from(secretBytes)));
const b64 = bytes => bytes.toString('base64').replace(/=+$/u, '');
const fileKey = Buffer.from('YELLOW SUBMARINE');
const fileNonce = Buffer.from(Array.from({length: 16}, (_, i) => (i * 13 + 11) & 255));
const grease = Buffer.from('-> jet-reference a\nAA\n');
const derive = (ikm, salt, info) => Buffer.from(hkdfSync('sha256', ikm, salt, Buffer.from(info), 32));
const seal = (key, nonce, plain) => {
    const cipher = createCipheriv('chacha20-poly1305', key, nonce, {authTagLength: 16});
    return Buffer.concat([cipher.update(plain), cipher.final(), cipher.getAuthTag()]);
};
const seals = [];
for (const [length, count] of [[0, 1], [1, 1], [65535, 1], [65536, 1], [65537, 1], [131072, 1], [0, 2]]) {
    const plain = Buffer.from(Array.from({length}, (_, i) => (i * 29 + 7) & 255));
    const stanzas = [];
    for (let recipient = 0; recipient < count; recipient++) {
        const scalar = Buffer.from(Array.from({length: 32}, (_, i) => ((recipient * 32 + i) * 7 + 3) & 255));
        const ephemeralKey = privateKey(scalar);
        const ephemeral = publicBytes(ephemeralKey);
        const shared = diffieHellman({privateKey: ephemeralKey, publicKey: recipientKey});
        const wrappingKey = derive(shared, Buffer.concat([ephemeral, recipientBytes]), 'age-encryption.org/v1/X25519');
        const wrapped = seal(wrappingKey, Buffer.alloc(12), fileKey);
        stanzas.push(Buffer.from(`-> X25519 ${b64(ephemeral)}\n${b64(wrapped)}\n`));
    }
    const authenticated = Buffer.concat([Buffer.from('age-encryption.org/v1\n'), ...stanzas, grease, Buffer.from('---')]);
    const mac = createHmac('sha256', derive(fileKey, Buffer.alloc(0), 'header')).update(authenticated).digest();
    const header = Buffer.concat([authenticated, Buffer.from(` ${b64(mac)}\n`)]);
    const payloadKey = derive(fileKey, fileNonce, 'payload');
    const chunks = [];
    for (let at = 0, counter = 0;; counter++) {
        const end = Math.min(at + 65536, length);
        const nonce = Buffer.alloc(12);
        nonce.writeBigUInt64BE(BigInt(counter), 3);
        nonce[11] = end === length ? 1 : 0;
        chunks.push(seal(payloadKey, nonce, plain.subarray(at, end)));
        at = end;
        if (at === length) break;
    }
    const cipher = Buffer.concat([header, fileNonce, ...chunks]);
    seals.push({name: `node-openssl-${length}-${count}`, identity, recipientCount: count, plain: plain.toString('base64'), cipher: cipher.toString('base64'), cipherSha256: digest(cipher)});
}
writeFileSync(output, JSON.stringify({provenance: {rustAge: '0.10.1', officialVectors: 'age0.10.1/tests/testdata/testkit', independentReference: 'Node built-in crypto/OpenSSL, age v1 fixed-material recipe in freeze-age-fixtures.mjs', node: process.version, openssl: process.versions.openssl}, cases, seals}, null, 2) + '\n');
console.log(`AGE_FIXTURE_FREEZE cases=${cases.length} seals=${seals.length} sha256=${digest(readFileSync(output))}`);
