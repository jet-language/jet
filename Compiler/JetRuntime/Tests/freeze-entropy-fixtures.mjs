#!/usr/bin/env node
// Historical oracle recipe retained; frozen fixtures remain native inputs.
console.log("parked: Rust reference retired (owner #38/#59); Tower #4923");
process.exit(0);
// Capture the existing std-only seed ONCE. No Rust is part of the native pack.
import {readFileSync, writeFileSync, mkdirSync, existsSync} from 'node:fs';
import {resolve, dirname, join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
const root=resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const scratch=process.argv[2];
if(!scratch) throw Error('usage: freeze-entropy-fixtures.mjs SCRATCH');
const target=join(root,'Compiler/JetRuntime/Tests/CryptoEntropyWitness.stdout');
if(existsSync(target)) throw Error('refusing to recapture frozen entropy fixtures');
mkdirSync(scratch,{recursive:true});
const seed=join(root,'crates/jet-codegen/src/Prelude/CoreLib/Top/CryptoEntropy.rs');
const source=join(scratch,'seed.rs'), binary=join(scratch,'seed');
writeFileSync(source,`#![allow(dead_code,unused_imports)]
include!(${JSON.stringify(seed)});
use jet_crypto_entropy::{JetCryptoEntropyError,jet_crypto_entropy_fill};
fn main(){
 let mut out=[0u8;7];let mut calls=0;
 jet_crypto_entropy_fill_with(&mut out,|s|{calls+=1;match calls{1=>{s[..2].copy_from_slice(&[1,2]);JetCryptoEntropyStep::Filled(2)},2=>{s.fill(0xa5);JetCryptoEntropyStep::Interrupted},3=>{assert_eq!(s,[0;5]);s.copy_from_slice(&[3,4,5,6,7]);JetCryptoEntropyStep::Filled(5)},_=>unreachable!()}}).unwrap();
 assert_eq!(out,[1,2,3,4,5,6,7]);assert_eq!(calls,3);println!("suffix ok=0 calls=3 bytes=01020304050607");
 for step in [JetCryptoEntropyStep::Filled(0),JetCryptoEntropyStep::Failed,JetCryptoEntropyStep::Filled(33)]{let mut out=[0xa5;32];assert_eq!(jet_crypto_entropy_fill_with(&mut out,|s|{s[..8].fill(0x5a);step}),Err(JetCryptoEntropyError::EntropyUnavailable));assert_eq!(out,[0;32]);}println!("failure zero oversized ok=1 wipe=32");
 let mut first=true;jet_crypto_entropy_set_test_provider(move|s|{if first{first=false;s[..4].copy_from_slice(&[1,2,3,4]);JetCryptoEntropyStep::Filled(4)}else{JetCryptoEntropyStep::Failed}});assert_eq!(jet_crypto_entropy_bytes(16),Err(JetCryptoEntropyError::EntropyUnavailable));jet_crypto_entropy_clear_test_provider();println!("partial ok=1 wipe=16");
 let mut out=[0xa5;16];assert_eq!(jet_crypto_entropy_unsupported_for_test(&mut out),Err(JetCryptoEntropyError::EntropyUnavailable));assert_eq!(out,[0;16]);println!("unsupported ok=1 wipe=16");
 let cases=[
 JetCryptoError::InvalidLength{operation:"seal",parameter:"key",expected:"exactly 32",actual:31},
 JetCryptoError::InvalidEncoding{operation:"PasswordHash.parse",value_kind:"PHC string"},
 JetCryptoError::UnsupportedVersion{operation:"PasswordHash.parse",version:16},
 JetCryptoError::UnsupportedAlgorithm{operation:"PasswordHash.parse",algorithm:"argon2i".into()},
 JetCryptoError::OpenFailed,JetCryptoError::NonContributoryKey,
 JetCryptoError::OutputLength{operation:"hkdf_sha256",minimum:0,maximum:8160,actual:8161},
 JetCryptoError::PasswordPolicy{reason:"public-policy-id"},JetCryptoError::EntropyUnavailable,
 JetCryptoError::ResourceUnavailable{resource:"Argon2 worker pool"},JetCryptoError::Internal{incident_id:"crypto-17"}];
 for e in cases {let s=e.to_string();for p in ["hunter2","plaintext sentinel","ciphertext sentinel","/home/nate"]{assert!(!format!("{e:?} {s}").contains(p));}println!("{s}");}
}`);
function run(name,args){const r=spawnSync(name,args,{encoding:'utf8',timeout:60000});if(r.error||r.status!==0)throw Error(`${name} exit=${r.status} ${r.error??''}\n${r.stderr}`);return r.stdout;}
const version=run('rustc',['--version']);
run('rustc',['--edition=2024','--cfg','test',source,'-o',binary]);
const stdout=run(binary,[]);
writeFileSync(target,stdout);
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
writeFileSync(join(root,'Compiler/JetRuntime/Tests/CryptoEntropyFixtures.json'),JSON.stringify({seed:'crates/jet-codegen/src/Prelude/CoreLib/Top/CryptoEntropy.rs',seedSha256:hash(readFileSync(seed)),referenceSha256:hash(readFileSync(source)),stdoutSha256:hash(stdout),rustc:version.trim(),wasi:{status:'target-gap',tests:['wasi_interrupt_retries_zeroize_each_exact_count_buffer','wasi_stops_after_seventeen_interrupts'],reason:'Jet native runtime pack currently has only Linux x86-64 syscalls; no WASI target provider'},debugRedaction:'seed Debug+Display forbidden sentinel assertions exercised in capture; private runtime renderer exposes only Display, no new public error type'},null,2)+'\n');
console.log(`ENTROPY_SEED_FROZEN lines=${stdout.trimEnd().split('\n').length} sha256=${hash(stdout)}`);
