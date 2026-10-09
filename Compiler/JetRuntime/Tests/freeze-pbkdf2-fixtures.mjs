#!/usr/bin/env node
// Historical oracle recipe retained; frozen fixtures remain native inputs.
console.log("parked: Rust reference retired (owner #38/#59); Tower #4923");
process.exit(0);
// Capture existing SHAFamily PBKDF2 + SHA256Raw HMAC once, std only.
import {readFileSync,writeFileSync,mkdirSync,existsSync} from 'node:fs';
import {resolve,dirname,join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'../../..'),scratch=process.argv[2];
if(!scratch)throw Error('usage: freeze-pbkdf2-fixtures.mjs SCRATCH');
const out=join(root,'Compiler/JetRuntime/Tests');if(existsSync(join(out,'CryptoPbkdf2Witness.stdout')))throw Error('refusing to recapture frozen PBKDF2 fixtures');
mkdirSync(scratch,{recursive:true});
const seeds=['SHA256Raw','SHAFamily'].map(n=>join(root,`crates/jet-codegen/src/Prelude/CoreLib/Top/${n}.rs`)),source=join(scratch,'seed.rs'),binary=join(scratch,'seed');
writeFileSync(source,`#![allow(dead_code)]
${seeds.map(p=>`include!(${JSON.stringify(p)});`).join('\n')}
fn main(){
 let cases=[(0,0,1,0),(0,0,1,32),(1,1,2,1),(7,3,17,31),(63,55,2,32),(64,56,2,33),(65,63,3,63),(129,64,17,64),(257,65,2,65),(65,129,1,97),(32,128,3,33),(31,127,17,64)];
 for (p,s,c,n) in cases{let password=(0..p).map(|i|((i*197+23)%256)as u8).collect::<Vec<_>>();let salt=(0..s).map(|i|((i*131+7)%256)as u8).collect::<Vec<_>>();let bytes=jet_crypto_pbkdf2_hmac(&password,&salt,c,n);let hex=if bytes.is_empty(){"-".into()}else{bytes.iter().map(|b|format!("{b:02x}")).collect::<String>()};println!("{p} {s} {c} {n} {hex}");}
 for (p,s,c) in [("passwd","salt",1),("Password","NaCl",80000)]{let bytes=jet_crypto_pbkdf2_hmac(&p.as_bytes().to_vec(),&s.as_bytes().to_vec(),c,64);println!("official {c} {}",bytes.iter().map(|b|format!("{b:02x}")).collect::<String>());}
}`);
function run(name,args){const r=spawnSync(name,args,{encoding:'utf8',timeout:60000});if(r.error||r.status!==0)throw Error(`${name} exit=${r.status} ${r.error??''}\n${r.stderr}`);return r.stdout;}
const rustc=run('rustc',['--version']).trim();run('rustc',['--edition=2024',source,'-o',binary]);const stdout=run(binary,[]),lines=stdout.trimEnd().split('\n');
writeFileSync(join(out,'CryptoPbkdf2Goldens.h'),'// Once-frozen existing Rust seed.\nstatic const struct PbkdfGolden { int64_t password,salt,iterations,length; const char *hex; } pbkdf_goldens[]={\n'+lines.filter(s=>!s.startsWith('official ')).map(s=>{const [p,t,c,n,h]=s.split(' ');return ` {${p},${t},${c},${n},"${h}"},`;}).join('\n')+'\n};\n');
writeFileSync(join(out,'CryptoPbkdf2Witness.stdout'),stdout);const hash=b=>createHash('sha256').update(b).digest('hex');
writeFileSync(join(out,'CryptoPbkdf2Fixtures.json'),JSON.stringify({seeds:seeds.map(p=>({path:p.slice(root.length+1),sha256:hash(readFileSync(p))})),referenceSha256:hash(readFileSync(source)),stdoutSha256:hash(stdout),rustc,cases:lines.length,officialKAT:'RFC7914 section11: passwd/salt,c1,dkLen64; Password/NaCl,c80000,dkLen64'},null,2)+'\n');
console.log(`PBKDF2_SEED_FROZEN cases=${lines.length} sha256=${hash(stdout)}`);
