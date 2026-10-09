#!/usr/bin/env node
// Historical oracle recipe retained; frozen fixtures remain native inputs.
console.log("parked: Rust reference retired (owner #38/#59); Tower #4923");
process.exit(0);
// One std-only Rust seed capture; artifacts are native differential expectations,
// never runtime implementations. Existing SHAFamily/SHA256Raw bodies unchanged.
import {readFileSync,writeFileSync,mkdirSync,existsSync} from 'node:fs';
import {resolve,dirname,join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'../../..'),scratch=process.argv[2];
if(!scratch)throw Error('usage: freeze-hash-fixtures.mjs SCRATCH');
const out=join(root,'Compiler/JetRuntime/Tests');
if(existsSync(join(out,'CryptoHashWitness.stdout')))throw Error('refusing to recapture frozen hash fixtures');
mkdirSync(scratch,{recursive:true});
const raw=join(root,'crates/jet-codegen/src/Prelude/CoreLib/Top/SHA256Raw.rs'),family=join(root,'crates/jet-codegen/src/Prelude/CoreLib/Top/SHAFamily.rs');
const source=join(scratch,'seed.rs'),binary=join(scratch,'seed');
writeFileSync(source,`#![allow(dead_code)]
include!(${JSON.stringify(raw)});include!(${JSON.stringify(family)});
fn main(){
 for n in [0,1,2,3,7,55,56,63,64,65,111,112,127,128,129,255,256,1023,1024,1025,4097]{let bytes=(0..n).map(|i|((i*197+23)%256)as u8).collect::<Vec<_>>();println!("{} {} {} {}",n,jet_crypto_sha1_hex(&bytes),jet_crypto_sha224_hex(&bytes),jet_crypto_sha384_hex(&bytes));}
 let a=(0..32).map(|i|i as u8).collect::<Vec<_>>();for mode in 0..8{let mut b=a.clone();let mut left=&a[..];match mode{0=>{},1=>b[0]^=1,2=>b[16]^=1,3=>b[31]^=1,4=>{b.pop();},5=>b.push(0),6=>{left=&a[..0];b.clear();},_=>{left=&a[..0];b=vec![0];}}println!("eq {} {}",mode,if jet_ct_eq(left,&b){1}else{0});}
}`);
function run(name,args){const r=spawnSync(name,args,{encoding:'utf8',timeout:60000});if(r.error||r.status!==0)throw Error(`${name} exit=${r.status} ${r.error??''}\n${r.stderr}`);return r.stdout;}
const rustc=run('rustc',['--version']).trim();run('rustc',['--edition=2024',source,'-o',binary]);const stdout=run(binary,[]);
const lines=stdout.trimEnd().split('\n'),hashes=lines.filter(s=>!s.startsWith('eq ')).map(s=>s.split(' ')),equal=lines.filter(s=>s.startsWith('eq ')).map(s=>Number(s.split(' ')[2]));
writeFileSync(join(out,'CryptoHashGoldens.h'),'// Frozen existing Rust seed; official KATs are separate in the witness.\nstatic const struct HashGolden { int64_t length; const char *sha1,*sha224,*sha384; } hash_goldens[] = {\n'+hashes.map(([n,a,b,c])=>` {${n},"${a}","${b}","${c}"},`).join('\n')+'\n};\nstatic const int equal_goldens[]={'+equal.join(',')+'};\n');
writeFileSync(join(out,'CryptoHashWitness.stdout'),stdout);
const hash=b=>createHash('sha256').update(b).digest('hex');
writeFileSync(join(out,'CryptoHashFixtures.json'),JSON.stringify({seeds:[raw,family].map(p=>({path:p.slice(root.length+1),sha256:hash(readFileSync(p))})),referenceSha256:hash(readFileSync(source)),stdoutSha256:hash(stdout),rustc,hashCases:hashes.length,equalityCases:equal.length,officialKAT:'RFC6234 section8.5 SHA1/SHA224/SHA384 abc and standard long-message vectors; empty input is additionally covered by the frozen Rust differential'},null,2)+'\n');
console.log(`HASH_SEED_FROZEN hash_cases=${hashes.length} equality_cases=${equal.length} sha256=${hash(stdout)}`);
