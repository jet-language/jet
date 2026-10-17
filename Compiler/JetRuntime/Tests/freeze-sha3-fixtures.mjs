#!/usr/bin/env node
// Historical oracle recipe retained; frozen fixtures remain native inputs.
console.log("parked: Rust reference retired (owner #38/#59); Tower #4923");
process.exit(0);
// ONE original-Rust capture; NIST CAVP KATs + explicit known seed divergences.
import {readFileSync,writeFileSync,mkdirSync,existsSync} from 'node:fs';
import {resolve,dirname,join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'../../..'),[scratch,archive]=process.argv.slice(2),out=join(root,'Compiler/JetRuntime/Tests');
if(!scratch||!archive)throw Error('usage: freeze-sha3-fixtures.mjs SCRATCH NIST-sha-3bytetestvectors.zip');
if(existsSync(join(out,'CryptoSha3Witness.stdout')))throw Error('refusing to recapture frozen SHA3 fixtures');
mkdirSync(scratch,{recursive:true});
function run(name,args){const r=spawnSync(name,args,{encoding:'utf8',timeout:60000,maxBuffer:8*1024*1024});if(r.error||r.status!==0)throw Error(`${name} exit=${r.status} ${r.error??''}\n${r.stderr}`);return r.stdout;}
const variants=[[224,144],[256,136],[384,104],[512,72]],official=[];
// Existing Tools/editors/vscode/install.sh uses stdlib zipfile; no new package.
for(const [bits,rate] of variants){const member=`SHA3_${bits}ShortMsg.rsp`,rsp=run('python3',['-c','import sys,zipfile; sys.stdout.write(zipfile.ZipFile(sys.argv[1]).read(sys.argv[2]).decode(\"ascii\"))',archive,member]);const rows=[...rsp.matchAll(/Len = (\d+)\s+Msg = ([0-9a-fA-F]+)\s+MD = ([0-9a-fA-F]+)/g)];for(const n of [0,1,rate-1,rate]){const row=rows.find(r=>Number(r[1])===n*8);if(!row)throw Error(`missing official ${member} Len=${n*8}`);official.push({bits,length:n,message:n?row[2].toLowerCase():'',hex:row[3].toLowerCase(),member});}}
const seeds=['SHA256Raw','SHAFamily'].map(n=>join(root,`crates/jet-codegen/src/Prelude/CoreLib/Top/${n}.rs`)),source=join(scratch,'seed.rs'),binary=join(scratch,'seed');
writeFileSync(source,`#![allow(dead_code)]\n${seeds.map(p=>`include!(${JSON.stringify(p)});`).join('\n')}
fn main(){for n in [0usize,1,7,8,63,64,70,71,72,73,102,103,104,105,134,135,136,137,142,143,144,145,207,215,271,287,4096]{let data=(0..n).map(|i|((i*197+23)%256)as u8).collect::<Vec<_>>();println!("224 {n} {}",jet_crypto_sha3_224_hex(&data));println!("256 {n} {}",jet_crypto_sha3_256_hex(&data));println!("384 {n} {}",jet_crypto_sha3_384_hex(&data));println!("512 {n} {}",jet_crypto_sha3_512_hex(&data));}}`);
const rustc=run('rustc',['--version']).trim();run('rustc',['--edition=2024',source,'-o',binary]);const captured=run(binary,[]),rows=captured.trimEnd().split('\n').map(line=>{const [bits,n,hex]=line.split(' ');return {bits:Number(bits),length:Number(n),hex,line};}),divergences=[],parity=[];
const goldens=rows.map(r=>{const rate=variants.find(v=>v[0]===r.bits)[1],divergence=r.length%rate===rate-1;let hex=r.hex;if(divergence){const input=Buffer.from(Array.from({length:r.length},(_,i)=>(i*197+23)%256));hex=createHash(`sha3-${r.bits}`).update(input).digest('hex');divergences.push({bits:r.bits,length:r.length,legacySeedHex:r.hex,canonicalHex:hex,reason:'KNOWN SEED DIVERGENCE: SHAFamily.rs347-352 adds a second padding block at length%rate==rate-1; current Core/FIPS202 combines 0x06|0x80 into 0x86',canonicalReference:`Node ${process.version} crypto/OpenSSL ${process.versions.openssl}; boundary independently bound by NIST CAVP ShortMsg KAT`});}else parity.push(r.line);return ` {${r.bits},${r.length},${divergence?1:0},"${hex}"},`;});
writeFileSync(join(out,'CryptoSha3Goldens.h'),'// Frozen Rust parity and explicitly canonical known-divergence expectations.\nstatic const struct Sha3Golden {int64_t bits,length;int divergence;const char *hex;} sha3_goldens[]={\n'+goldens.join('\n')+'\n};\n// Official NIST CAVP byte-oriented ShortMsg, original response file MD.\nstatic const struct Sha3Official {int64_t bits,length;const char *message,*hex;} sha3_official[]={\n'+official.map(r=>` {${r.bits},${r.length},"${r.message}","${r.hex}"},`).join('\n')+'\n};\n');
const stdout=parity.join('\n')+'\n',hash=b=>createHash('sha256').update(b).digest('hex');writeFileSync(join(out,'CryptoSha3Witness.stdout'),stdout);writeFileSync(join(out,'CryptoSha3Fixtures.json'),JSON.stringify({seeds:seeds.map(p=>({path:p.slice(root.length+1),sha256:hash(readFileSync(p))})),referenceSha256:hash(readFileSync(source)),capturedStdoutSha256:hash(captured),stdoutSha256:hash(stdout),rustc,parityCases:parity.length,knownSeedDivergences:divergences,officialKAT:{url:'https://csrc.nist.gov/CSRC/media/Projects/Cryptographic-Algorithm-Validation-Program/documents/sha3/sha-3bytetestvectors.zip',archiveSha256:hash(readFileSync(archive)),cases:official.map(r=>({member:r.member,lenBits:r.length*8,md:r.hex}))}},null,2)+'\n');
console.log(`SHA3_SEED_FROZEN parity=${parity.length} known_divergences=${divergences.length} official=${official.length} sha256=${hash(stdout)}`);
