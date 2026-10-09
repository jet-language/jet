#!/usr/bin/env node
// Offline, once-only RustCrypto seed capture; no Cargo or new dependencies.
import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { resolve, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { inflateRawSync } from 'node:zlib';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const [scratch, archive, aesPdf, deps, aes, gcm, oracleLock] = process.argv.slice(2);
if (!oracleLock) throw Error('usage: freeze-aes-fixtures.mjs SCRATCH GCM_ZIP AES_PDF DEPS AES_RLIB AES_GCM_RLIB ORACLE_CARGO_LOCK');
const out = join(root, 'tests/conformance/corpus/core/crypto/aes');
if (existsSync(join(out, 'seed.stdout'))) throw Error('refusing to recapture frozen AES oracle');
const hash = data => createHash('sha256').update(data).digest('hex');
const zip = readFileSync(archive);
// ZIP central directory sizes, including entries with a data descriptor.
function member(name) {
  let eocd = zip.length - 22;
  while (eocd >= Math.max(0, zip.length - 65557) && zip.readUInt32LE(eocd) !== 0x06054b50) eocd--;
  if (eocd < 0) throw Error('missing ZIP central directory');
  let p = zip.readUInt32LE(eocd + 16);
  const count = zip.readUInt16LE(eocd + 10);
  for (let i = 0; i < count; i++) {
    if (zip.readUInt32LE(p) !== 0x02014b50) throw Error('bad ZIP central directory');
    const method = zip.readUInt16LE(p + 10), size = zip.readUInt32LE(p + 20);
    const n = zip.readUInt16LE(p + 28), extra = zip.readUInt16LE(p + 30), comment = zip.readUInt16LE(p + 32);
    const found = zip.subarray(p + 46, p + 46 + n).toString('utf8');
    if (found === name) {
      const local = zip.readUInt32LE(p + 42);
      if (zip.readUInt32LE(local) !== 0x04034b50) throw Error('bad ZIP local header');
      const start = local + 30 + zip.readUInt16LE(local + 26) + zip.readUInt16LE(local + 28);
      const bytes = zip.subarray(start, start + size);
      const data = method === 8 ? inflateRawSync(bytes) : method === 0 ? bytes : null;
      if (!data) throw Error(`unsupported ZIP compression ${method}`);
      return data;
    }
    p += 46 + n + extra + comment;
  }
  throw Error(`missing ZIP member ${name}`);
}
function parseRsp(name) {
  const data = member(name), parameters = {}, cases = [];
  let row;
  function flush() { if (row) cases.push({ ...parameters, ...row, member: name }); row = undefined; }
  for (const line of data.toString('ascii').split(/\r?\n/)) {
    const parameter = line.match(/^\[(\w+) = (\d+)\]$/);
    if (parameter) { flush(); parameters[parameter[1]] = Number(parameter[2]); continue; }
    const field = line.match(/^(\w+) = (.*)$/);
    if (field) {
      if (field[1] === 'Count') { flush(); row = { count: Number(field[2]) }; }
      else if (row) row[field[1]] = field[2].trim().toLowerCase();
    } else if (line === 'FAIL' && row) row.fail = true;
    else if (line === '') flush();
  }
  flush();
  return { cases, sha256: hash(data) };
}
const encrypt = parseRsp('gcmEncryptExtIV256.rsp'), decrypt = parseRsp('gcmDecrypt256.rsp');
const cases = [], seen = new Set();
for (const [kind, rows] of [['encrypt', encrypt.cases], ['decrypt', decrypt.cases]]) {
  for (const row of rows) {
    if (row.Keylen !== 256 || row.IVlen !== 96 || row.Taglen !== 128) continue;
    const selection = `${kind}/${row.PTlen}/${row.AADlen}/${Boolean(row.fail)}`;
    if (seen.has(selection)) continue;
    seen.add(selection);
    cases.push({ label: `nist_${kind}_${row.PTlen}_${row.AADlen}_${row.fail ? 'fail' : 'pass'}`, kind, key: row.Key, nonce: row.IV, aad: row.AAD, plain: row.PT ?? '', cipher: row.CT + row.Tag, fail: Boolean(row.fail), published: { member: row.member, count: row.count, PTlen: row.PTlen, AADlen: row.AADlen } });
  }
}
// Deterministic pseudorandom differential inputs, not an entropy provider.
let rng = 0x6d2b79f5;
function bytes(n) {
  const b = Buffer.alloc(n);
  for (let i = 0; i < n; i++) { rng ^= rng << 13; rng ^= rng >>> 17; rng ^= rng << 5; b[i] = rng & 255; }
  return b.toString('hex');
}
const sizes = [0, 1, 2, 7, 15, 16, 17, 31, 32, 33, 63, 64, 65, 127, 128, 129, 255, 256, 257, 1023, 1024, 4095, 4096];
for (let i = 0; i < 46; i++) {
  cases.push({ label: `random_${i}`, kind: 'random', key: bytes(32), nonce: bytes(12), aad: bytes(sizes[(i * 7) % 19]), plain: bytes(sizes[i % sizes.length]), fail: false });
}
const aesKat = {
  url: 'https://csrc.nist.gov/CSRC/media/Projects/Cryptographic-Standards-and-Guidelines/documents/examples/AES_Core256.pdf',
  sha256: hash(readFileSync(aesPdf)),
  key: '603deb1015ca71be2b73aef0857d77811f352c073b6108d72d9810a30914dff4',
  plain: ['6bc1bee22e409f96e93d7e117393172a', 'ae2d8a571e03ac9c9eb76fac45af8e51', '30c81c46a35ce411e5fbc1191a0a52ef', 'f69f2445df4f9b17ad2b417be66c3710'],
  cipher: ['f3eed1bdb5d2a03c064b5a7e3db181f8', '591ccb10d410ed26dc5ba74a31362870', 'b6ed21b99ca6f4f9f153e7b1beafed1d', '23304b7a39f9f3ff067d8d8f9e24ecc7'],
};
function locked(path, name) {
  const text = readFileSync(path, 'utf8');
  const row = text.split('[[package]]').find(r => r.includes(`name = "${name}"\n`));
  if (!row) throw Error(`missing ${name} in ${path}`);
  return { version: row.match(/version = "([^"]+)"/)[1], checksum: row.match(/checksum = "([^"]+)"/)[1] };
}
const dependencies = [['aes', aes], ['aes-gcm', gcm]].map(([name, path]) => {
  const seed = locked(join(root, 'Cargo.lock'), name), oracle = locked(oracleLock, name);
  if (JSON.stringify(seed) !== JSON.stringify(oracle)) throw Error(`oracle/seed mismatch for ${name}`);
  return { name, ...seed, rlibSha256: hash(readFileSync(path)), provenance: 'seed Cargo.lock and CoreCrypto1-3/oracle/Cargo.lock', path };
});
function vec(hex) { return `vec![${Buffer.from(hex, 'hex').join(',')}]`; }
mkdirSync(scratch, { recursive: true });
const source = join(scratch, 'seed.rs'), binary = join(scratch, 'seed');
writeFileSync(source, `use aes::{Aes256, cipher::{BlockEncrypt, KeyInit as BlockKeyInit, generic_array::GenericArray}};
use aes_gcm::{Aes256Gcm, Nonce, aead::{Aead, Payload}};
fn hex(bytes:&[u8])->String { bytes.iter().map(|b|format!("{b:02x}")).collect() }
fn row(label:&str,key:Vec<u8>,nonce:Vec<u8>,aad:Vec<u8>,plain:Vec<u8>,cipher:Vec<u8>,kind:&str) {
 let aead = Aes256Gcm::new_from_slice(&key).unwrap();
 if kind == "decrypt" {
  match aead.decrypt(Nonce::from_slice(&nonce),Payload{msg:&cipher,aad:&aad}) { Ok(p)=>println!("{label} open {}",hex(&p)),Err(_)=>println!("{label} fail") }
 } else {
  let c=aead.encrypt(Nonce::from_slice(&nonce),Payload{msg:&plain,aad:&aad}).unwrap();
  let p=aead.decrypt(Nonce::from_slice(&nonce),Payload{msg:&c,aad:&aad}).unwrap(); assert_eq!(p,plain);
  let mut bad=c.clone(); let end=bad.len()-1; bad[end]^=1; assert!(aead.decrypt(Nonce::from_slice(&nonce),Payload{msg:&bad,aad:&aad}).is_err());
  println!("{label} seal {}",hex(&c));
 }
}
fn main() {
 ${cases.map(c => `row(${JSON.stringify(c.label)},${vec(c.key)},${vec(c.nonce)},${vec(c.aad)},${vec(c.plain)},${vec(c.cipher ?? '')},${JSON.stringify(c.kind)});`).join('\n ')}
 let aes=Aes256::new_from_slice(&${vec(aesKat.key)}).unwrap();
 ${aesKat.plain.map((p, i) => `{let mut block=GenericArray::clone_from_slice(&${vec(p)});aes.encrypt_block(&mut block);println!("aes_block_${i} {}",hex(&block));}`).join('\n ')}
}
`);
function run(name, args) {
  const r = spawnSync(name, args, { encoding: 'utf8', timeout: 60000, maxBuffer: 16 * 1024 * 1024 });
  if (r.error || r.status !== 0) throw Error(`${name} exit=${r.status} ${r.error ?? ''}\n${r.stderr}`);
  return r.stdout;
}
const rustc = run('rustc', ['--version']).trim();
run('rustc', ['--edition=2024', source, '-L', `dependency=${deps}`, '--extern', `aes=${aes}`, '--extern', `aes_gcm=${gcm}`, '-o', binary]);
const stdout = run(binary, []);
const observed = new Map(stdout.trimEnd().split('\n').map(line => { const [label, action, value] = line.split(' '); return [label, { action, value: value ?? '' }]; }));
for (const c of cases) {
  const r = observed.get(c.label);
  if (!r) throw Error(`missing ${c.label}`);
  if (c.kind === 'decrypt') {
    if (c.fail ? r.action !== 'fail' : r.action !== 'open' || r.value !== c.plain) throw Error(`NIST decrypt mismatch ${c.label}`);
  } else {
    if (r.action !== 'seal' || (c.kind === 'encrypt' && r.value !== c.cipher)) throw Error(`NIST encrypt mismatch ${c.label}`);
    c.cipher = r.value;
  }
}
for (let i = 0; i < 4; i++) {
  // AES rows have two columns rather than three.
  if (observed.get(`aes_block_${i}`)?.action !== aesKat.cipher[i]) throw Error(`published AES block mismatch ${i}`);
}
mkdirSync(out, { recursive: true });
writeFileSync(join(out, 'seed.stdout'), stdout);
const archiveHash = hash(zip);
writeFileSync(join(out, 'fixtures.json'), JSON.stringify({
  seed: { source: 'crates/jet-pkg-model/src/Prelude/Crypto.rs:681-687 (RustCrypto AES-256-GCM)', sha256: hash(readFileSync(join(root, 'crates/jet-pkg-model/src/Prelude/Crypto.rs'))) },
  oracle: { rustc, sourceSha256: hash(readFileSync(source)), stdoutSha256: hash(stdout), dependencies, randomPattern: 'xorshift32 seed 0x6d2b79f5; key/nonce/AAD/plain sequential, 46 cases; every seal also roundtrip and last-tag-byte tamper rejection' },
  nist: { url: 'https://csrc.nist.gov/CSRC/media/Projects/Cryptographic-Algorithm-Validation-Program/documents/mac/gcmtestvectors.zip', archiveSha256: archiveHash, members: [{ name: 'gcmEncryptExtIV256.rsp', sha256: encrypt.sha256 }, { name: 'gcmDecrypt256.rsp', sha256: decrypt.sha256 }] },
  aesKat, cases,
}, null, 2) + '\n');
function jetBytes(hex) { return `[U8]{${Buffer.from(hex, 'hex').join(', ')}}`; }
const header = `// NIST: https://csrc.nist.gov/CSRC/media/Projects/Cryptographic-Algorithm-Validation-Program/documents/mac/gcmtestvectors.zip\n// Archive SHA-256: ${archiveHash}\n// Encrypt member SHA-256: ${encrypt.sha256}; decrypt: ${decrypt.sha256}\n// Frozen RustCrypto seed stdout SHA-256: ${hash(stdout)} (fixtures.json).\n// Jet execution UNEXERCISED at capture; run this corpus after composed Core rebuild.\n`;
for (const [name, rows] of [['nist_gcm', cases.filter(c => c.kind !== 'random')], ['seed_differential', cases.filter(c => c.kind === 'random')]]) {
  const checks = rows.map(c => `    // ${c.label}${c.published ? `, ${c.published.member} Count=${c.published.count}` : ''}\n    check(${jetBytes(c.key)}, ${jetBytes(c.nonce)}, ${jetBytes(c.plain)}, ${jetBytes(c.aad)}, ${jetBytes(c.cipher)}, ${c.fail})`).join('\n');
  writeFileSync(join(out, `${name}.jet`), `// core-conformance: core.crypto.aes.${name}\n${header}use core.crypto.expert as expert\nuse core.crypto.[CryptoError]\n\nfn check(key: [U8], nonce: [U8], plain: [U8], aad: [U8], cipher: [U8], fail: Bool) {\n    #Unsafe("published KAT or frozen seed AES-GCM fixture") {\n        if fail {\n            if expert.aes256gcm_open(key, nonce, cipher, aad) == {\n                .Err(error) -> assert_eq(error, CryptoError.OpenFailed)\n                .Ok(_) -> panic("invalid authentication accepted")\n            }\n        } else {\n            assert_eq(expert.aes256gcm_seal(key, nonce, plain, aad) ?? panic("seal"), cipher)\n            assert_eq(expert.aes256gcm_open(key, nonce, cipher, aad) ?? panic("open"), plain)\n            bad := cipher.copy()\n            bad[bad.len() - 1] = bad[bad.len() - 1].bit_xor(U8{1})\n            if expert.aes256gcm_open(key, nonce, bad, aad) == {\n                .Err(error) -> assert_eq(error, CryptoError.OpenFailed)\n                .Ok(_) -> panic("tampered tag accepted")\n            }\n        }\n    }\n}\n\nfn run() {\n${checks}\n    print(true)\n}\n`);
}
console.log(`AES_SEED_FROZEN nist=${cases.filter(c => c.kind !== 'random').length} fail=${cases.filter(c => c.fail).length} random=46 aes_blocks=4 stdout_sha256=${hash(stdout)} aes_pdf_sha256=${aesKat.sha256}`);
