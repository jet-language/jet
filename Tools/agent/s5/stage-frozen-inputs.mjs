#!/usr/bin/env node
// Stage the preserved age 0.10.1 captures from tracked data. Each cipher is
// the base64 bytes AgeV1Fixtures.json froze; each plain is that cipher opened
// with the fixed test identity (age v1 X25519, Node/OpenSSL built-ins). Both
// must match the table's sha256 pins, so no scratch origin is read.
// The char Debug and formatter goldens are tracked under
// Compiler/JetRuntime/Tests (CharDebugPlain.ranges, FmtParityExpected.out).
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {pinnedHead} from './s5-lib.mjs';
const [head, source, output] = process.argv.slice(2);
if (!/^[a-f0-9]{40}$/.test(head ?? '') || !source || !output || process.argv.length !== 5) throw Error('usage: stage-frozen-inputs.mjs FULL_SHA SOURCE_DIRECTORY IMMUTABLE_INPUT_DIRECTORY');
pinnedHead(source, head);
const tablePath = 'Compiler/JetRuntime/Tests/AgeV1Fixtures.json';
const table = JSON.parse(fs.readFileSync(path.join(source, tablePath), 'utf8'));
const digest = bytes => crypto.createHash('sha256').update(bytes).digest('hex');

// The identity is a bech32 X25519 scalar; the recipient is its public key.
function secretKey(identity) {
  const alphabet = 'qpzry9x8gf2tvdw0s3jn54khce6mua7l', bytes = [];
  let bits = 0, accumulator = 0;
  for (const value of identity.toLowerCase().slice(16, -6).split('').map(c => alphabet.indexOf(c))) {
    if (value < 0) throw Error('bad identity');
    accumulator = (accumulator << 5) | value; bits += 5;
    if (bits >= 8) { bits -= 8; bytes.push((accumulator >>> bits) & 255); }
  }
  if (bytes.length !== 32) throw Error('bad identity length');
  return Buffer.from(bytes);
}
const privateKey = bytes => crypto.createPrivateKey({key: Buffer.concat([Buffer.from('302e020100300506032b656e04220420', 'hex'), bytes]), format: 'der', type: 'pkcs8'});
const publicKey = bytes => crypto.createPublicKey({key: Buffer.concat([Buffer.from('302a300506032b656e032100', 'hex'), bytes]), format: 'der', type: 'spki'});
const publicBytes = key => crypto.createPublicKey(key).export({format: 'der', type: 'spki'}).subarray(-32);
const derive = (ikm, salt, info) => Buffer.from(crypto.hkdfSync('sha256', ikm, salt, Buffer.from(info), 32));
function open(key, nonce, sealed) {
  const decipher = crypto.createDecipheriv('chacha20-poly1305', key, nonce, {authTagLength: 16});
  decipher.setAuthTag(sealed.subarray(-16));
  return Buffer.concat([decipher.update(sealed.subarray(0, -16)), decipher.final()]);
}
const unpadded = text => Buffer.from(text, 'base64');

// age v1: the header lines, the header MAC, then a 16-byte nonce and
// 64 KiB ChaCha20-Poly1305 STREAM chunks whose last nonce byte marks the end.
function decrypt(cipher, identity) {
  const lines = [];
  let at = 0;
  for (;;) {
    const end = cipher.indexOf(0x0a, at);
    if (end < 0) throw Error('unterminated age header');
    lines.push(cipher.subarray(at, end).toString('latin1')); at = end + 1;
    if (lines.at(-1).startsWith('--- ')) break;
  }
  if (lines[0] !== 'age-encryption.org/v1') throw Error('not an age v1 file');
  const secret = secretKey(identity), ownKey = privateKey(secret), recipient = publicBytes(ownKey);
  let fileKey = null;
  for (let line = 1; line < lines.length - 1;) {
    const args = lines[line++].split(' ');
    if (args[0] !== '->') throw Error('malformed age stanza');
    let body = '';
    for (;;) { const chunk = lines[line++]; body += chunk; if (chunk.length < 64) break; }
    if (args[1] !== 'X25519' || fileKey) continue;
    const ephemeral = unpadded(args[2]);
    const shared = crypto.diffieHellman({privateKey: ownKey, publicKey: publicKey(ephemeral)});
    fileKey = open(derive(shared, Buffer.concat([ephemeral, recipient]), 'age-encryption.org/v1/X25519'), Buffer.alloc(12), unpadded(body));
  }
  if (!fileKey) throw Error('no X25519 stanza opens with the identity');
  const authenticated = cipher.subarray(0, at - lines.at(-1).length - 1 + 3);
  const mac = crypto.createHmac('sha256', derive(fileKey, Buffer.alloc(0), 'header')).update(authenticated).digest();
  if (!mac.equals(unpadded(lines.at(-1).slice(4)))) throw Error('age header MAC mismatch');
  const payloadKey = derive(fileKey, cipher.subarray(at, at + 16), 'payload'), plain = [];
  at += 16;
  for (let counter = 0n; ; counter++) {
    const sealed = cipher.subarray(at, Math.min(at + 65536 + 16, cipher.length)), nonce = Buffer.alloc(12);
    at += sealed.length;
    nonce.writeBigUInt64BE(counter, 3); nonce[11] = at === cipher.length ? 1 : 0;
    plain.push(open(payloadKey, nonce, sealed));
    if (at === cipher.length) return Buffer.concat(plain);
  }
}

const files = [];
for (const row of table.cases.filter(row => row.name.startsWith('rust-0.10.1-'))) {
  const length = row.name.slice('rust-0.10.1-'.length), cipher = Buffer.from(row.cipher, 'base64');
  const origin = `${tablePath}#${row.name}`;
  files.push({path: `age-rust/age-rust-${length}.age`, origin, sha256: row.cipherSha256, bytes: cipher});
  files.push({path: `age-rust/age-rust-${length}.plain`, origin, sha256: row.plainSha256, bytes: decrypt(cipher, row.identity)});
}
if (files.length !== 12 || files.some(file => !/^[a-f0-9]{64}$/.test(file.sha256))) throw Error('preserved input census or digest drift');
for (const file of files) if (digest(file.bytes) !== file.sha256) throw Error(`preserved capture changed: ${file.path}`);
const hash = file => digest(fs.readFileSync(file));
const destination = path.resolve(output);
const manifest = {head, files: files.map(({path, origin, sha256}) => ({path, origin, sha256})), missing: [{runner: 'freeze-age-fixtures.mjs', inputs: ['age testkit/'], because: 'Rust-captured oracle not preserved'}]};
if (fs.existsSync(destination)) {
  const actual = JSON.parse(fs.readFileSync(path.join(destination, 'manifest.json'), 'utf8'));
  if (JSON.stringify(actual) !== JSON.stringify(manifest)) throw Error('immutable input manifest changed');
  for (const file of files) if (hash(path.join(destination, file.path)) !== file.sha256) throw Error(`immutable input changed: ${file.path}`);
  console.log(destination); process.exit(0);
}
const staging = `${destination}.staging-${process.pid}`;
fs.mkdirSync(staging, {recursive: true});
for (const file of files) {
  const target = path.join(staging, file.path);
  fs.mkdirSync(path.dirname(target), {recursive: true}); fs.writeFileSync(target, file.bytes);
  if (hash(target) !== file.sha256) throw Error(`input copy differs: ${file.path}`);
  fs.chmodSync(target, 0o444);
}
fs.writeFileSync(path.join(staging, 'manifest.json'), JSON.stringify(manifest, null, 2) + '\n', {mode: 0o444});
fs.chmodSync(path.join(staging, 'age-rust'), 0o555); fs.chmodSync(staging, 0o555);
fs.renameSync(staging, destination);
console.log(destination);
