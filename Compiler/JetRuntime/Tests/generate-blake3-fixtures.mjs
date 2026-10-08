#!/usr/bin/env node
// The seed uses the same official blake3 crate; no redundant Rust capture.
import {readFileSync,writeFileSync,existsSync} from 'node:fs';
import {resolve,dirname,join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {createHash} from 'node:crypto';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'../../..'),out=join(root,'Compiler/JetRuntime/Tests'),file=join(out,'Blake3OfficialVectors.json');
if(existsSync(join(out,'CryptoBlake3Goldens.h')))throw Error('refusing to overwrite frozen official BLAKE3 fixture');
const bytes=readFileSync(file),vectors=JSON.parse(bytes),hash=b=>createHash('sha256').update(b).digest('hex');
const rows=vectors.cases.map(row=>({length:row.input_len,hex:row.hash.slice(0,64)}));
writeFileSync(join(out,'CryptoBlake3Goldens.h'),'// Official BLAKE3-team default-length prefix of XOF vectors, input i%251.\nstatic const struct Blake3Golden {int64_t length;const char *hex;} blake3_goldens[]={\n'+rows.map(r=>` {${r.length},"${r.hex}"},`).join('\n')+'\n};\n');
const stdout=rows.map(r=>`${r.length} ${r.hex}`).join('\n')+'\n';writeFileSync(join(out,'CryptoBlake3Witness.stdout'),stdout);
writeFileSync(join(out,'CryptoBlake3Fixtures.json'),JSON.stringify({officialKAT:{url:'https://raw.githubusercontent.com/BLAKE3-team/BLAKE3/master/test_vectors/test_vectors.json',path:'Compiler/JetRuntime/Tests/Blake3OfficialVectors.json',sha256:hash(bytes),cases:rows.length,inputPattern:'i%251',output:'first 32 bytes of official hash/XOF vector as specified by the vector comment'},stdoutSha256:hash(stdout),seed:{path:'crates/jet-pkg-model/src/Prelude/Crypto.rs',line:351,body:'JetDigest256(blake3::hash(data).as_bytes().to_vec())',sha256:hash(readFileSync(join(root,'crates/jet-pkg-model/src/Prelude/Crypto.rs'))),capture:'SKIPPED per RuntimeJet18: seed directly calls the official blake3 crate; authoritative team vectors replace redundant Rust capture'}},null,2)+'\n');
console.log(`BLAKE3_OFFICIAL_FROZEN cases=${rows.length} max_length=${Math.max(...rows.map(r=>r.length))} sha256=${hash(stdout)}`);
