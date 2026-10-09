#!/usr/bin/env node
// Historical oracle recipe retained; frozen fixtures remain native inputs.
console.log("parked: Rust reference retired (owner #38/#59); Tower #4923");
process.exit(0);
// ONCE exact dalek 2.2.0 edge oracle, linking existing seed dependencies only.
import {readFileSync,writeFileSync,mkdirSync,existsSync} from 'node:fs';
import {resolve,dirname,join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {generateEd25519Headers} from './generate-ed25519-headers.mjs';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'../../..'),[scratch,deps,dalek,curve,sha2]=process.argv.slice(2),out=join(root,'Compiler/JetRuntime/Tests');
if(!scratch||!deps||!dalek||!curve||!sha2)throw Error('usage: freeze-ed25519-fixtures.mjs SCRATCH DEPS DALEK_RLIB CURVE_RLIB SHA2_RLIB');
if(existsSync(join(out,'CryptoEd25519SignatureWitness.stdout')))throw Error('refusing to recapture frozen Ed25519 oracle');
mkdirSync(scratch,{recursive:true});const source=join(scratch,'seed.rs'),binary=join(scratch,'seed');
writeFileSync(source,`use ed25519_dalek::{SigningKey,VerifyingKey,Signature,Signer,Verifier};
use curve25519_dalek::{constants::{ED25519_BASEPOINT_POINT,EIGHT_TORSION},edwards::CompressedEdwardsY,scalar::Scalar};
use sha2::{Digest,Sha512};
fn hex(bytes:&[u8])->String{if bytes.is_empty(){return "-".into()}bytes.iter().map(|b|format!("{b:02x}")).collect()}
fn emit(label:&str,seed:&[u8],a:[u8;32],m:&[u8],s:[u8;64]){let key=VerifyingKey::from_bytes(&a);let sig=Signature::from_bytes(&s);let decoded=key.is_ok();let ordinary=key.as_ref().map(|k|k.verify(m,&sig).is_ok()).unwrap_or(false);let strict=key.as_ref().map(|k|k.verify_strict(m,&sig).is_ok()).unwrap_or(false);println!("{label} {} {} {} {} {} {} {}",hex(seed),hex(&a),hex(m),hex(&s),decoded as u8,ordinary as u8,strict as u8);}
fn main(){
 let seed:[u8;32]=[0x9d,0x61,0xb1,0x9d,0xef,0xfd,0x5a,0x60,0xba,0x84,0x4a,0xf4,0x92,0xec,0x2c,0xc4,0x44,0x49,0xc5,0x69,0x7b,0x32,0x69,0x19,0x70,0x3b,0xac,0x03,0x1c,0xae,0x7f,0x60];let key=SigningKey::from_bytes(&seed);let public=key.verifying_key().to_bytes();
 for n in [0usize,1,2,31,32,63,64,65,111,112,113,127,128,129,1023,1024,4096]{let m=(0..n).map(|i|((i*197+23)%256)as u8).collect::<Vec<_>>();let s=key.sign(&m).to_bytes();emit(&format!("sign_{n}"),&seed,public,&m,s);}
 let m=b"edge acceptance";let good=key.sign(m).to_bytes();let mut changed=good;changed[0]^=1;emit("tamper_R",&[],public,m,changed);let mut changed=good;changed[32]^=1;emit("tamper_S",&[],public,m,changed);emit("tamper_message",&[],public,b"edge acceptancf",good);
 let identity=EIGHT_TORSION[0].compress().to_bytes();let mut s=[0u8;64];s[..32].copy_from_slice(&identity);
 for i in 0..8{emit(&format!("small_order_A_{i}"),&[],EIGHT_TORSION[i].compress().to_bytes(),m,s);}
 for i in 0..8{let mut t=s;t[..32].copy_from_slice(&EIGHT_TORSION[i].compress().to_bytes());emit(&format!("small_order_R_{i}"),&[],identity,m,t);}
 // ZIP215 decoding allows y>=p; original compressed A remains in the challenge.
 for y in 0..19{for sign in 0..2{let mut a=[255u8;32];a[0]=237+y;a[31]=127|(sign<<7);emit(&format!("noncanonical_A_y{y}_sign{sign}"),&[],a,m,s);let mut t=s;t[..32].copy_from_slice(&a);emit(&format!("noncanonical_R_y{y}_sign{sign}"),&[],identity,m,t);}}
 let l:[u8;32]=[0xed,0xd3,0xf5,0x5c,0x1a,0x63,0x12,0x58,0xd6,0x9c,0xf7,0xa2,0xde,0xf9,0xde,0x14,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0x10];let mut t=s;t[32..].copy_from_slice(&l);emit("S_equal_L",&[],identity,m,t);t[32]+=1;emit("S_above_L",&[],identity,m,t);t[32..].fill(255);emit("S_all_ff",&[],identity,m,t);t[32..].copy_from_slice(&l);t[32]-=1;t[..32].copy_from_slice(&(-ED25519_BASEPOINT_POINT).compress().to_bytes());emit("S_L_minus_1",&[],identity,m,t);
 // Strong mixed-order A is allowed. Forge mathematically valid signatures with
 // known prime-subgroup scalar3 plus torsion, searching only public nonce/phase.
 let a=Scalar::from(3u64);for ti in [1usize,2,4]{let point=ED25519_BASEPOINT_POINT*a+EIGHT_TORSION[ti];let encoded=point.compress().to_bytes();let mut found=false;for nonce in 1u64..100{for phase in 0..8{let r=Scalar::from(nonce);let rp=ED25519_BASEPOINT_POINT*r+EIGHT_TORSION[phase];let rb=rp.compress().to_bytes();let hash: [u8;64]=Sha512::new().chain_update(rb).chain_update(encoded).chain_update(m).finalize().into();let k=Scalar::from_bytes_mod_order_wide(&hash);let mut sig=[0u8;64];sig[..32].copy_from_slice(&rb);sig[32..].copy_from_slice(&(r+k*a).to_bytes());let vk=VerifyingKey::from_bytes(&encoded).unwrap();if vk.verify_strict(m,&Signature::from_bytes(&sig)).is_ok(){emit(&format!("valid_mixed_order_{ti}"),&[],encoded,m,sig);found=true;break;}}if found{break;}}assert!(found);}
 // Canonical encodings that fail point decompression, selected independently.
 let mut invalid=0;for y in 2u8..64{let mut a=[0u8;32];a[0]=y;if CompressedEdwardsY(a).decompress().is_none(){emit(&format!("invalid_point_y{y}"),&[],a,m,s);invalid+=1;if invalid==4{break;}}}
}
`);
function run(name,args){const r=spawnSync(name,args,{encoding:'utf8',timeout:60000,maxBuffer:8*1024*1024});if(r.error||r.status!==0)throw Error(`${name} exit=${r.status} ${r.error??''}\n${r.stderr}`);return r.stdout;}
const rustc=run('rustc',['--version']).trim();run('rustc',['--edition=2024',source,'-L',`dependency=${deps}`,'--extern',`ed25519_dalek=${dalek}`,'--extern',`curve25519_dalek=${curve}`,'--extern',`sha2=${sha2}`,'-o',binary]);const captured=run(binary,[]),cases=captured.trimEnd().split('\n').map(line=>{const [label,seed,publicKey,message,signature,decoded,ordinary,strict]=line.split(' ');return {label,seed:seed==='-'?'':seed,publicKey,message:message==='-'?'':message,signature,decoded:Number(decoded),ordinary:Number(ordinary),strict:Number(strict)};});
const stdout=cases.map(c=>`${c.label} ${c.decoded} ${c.ordinary} ${c.strict}`).join('\n')+'\n',hash=b=>createHash('sha256').update(b).digest('hex');
writeFileSync(join(out,'CryptoEd25519SignatureWitness.stdout'),stdout);writeFileSync(join(out,'CryptoEd25519SignatureFixtures.json'),JSON.stringify({seedCitation:{path:'crates/jet-pkg-model/src/Prelude/Crypto.rs',strict:'216-219: VerifyingKey.verify_strict',ordinary:'802-832: Verifier.verify',sign:'209-214: SigningKey::from_bytes(seed).sign(message)',sha256:hash(readFileSync(join(root,'crates/jet-pkg-model/src/Prelude/Crypto.rs')))},oracle:{rustc,sourceSha256:hash(readFileSync(source)),dependencies:[dalek,curve,sha2].map(path=>({path,sha256:hash(readFileSync(path))})),dalekVersion:'2.2.0',features:['alloc','default','fast','std','zeroize'],notEnabled:'legacy_compatibility',provenance:'CoreCrypto1-3 Cargo.lock262-265 and ed25519-dalek-8963a5f6e6c4570c fingerprint; existing deps built jet-env, no cargo invoked here',capturedStdoutSha256:hash(captured)},stdoutSha256:hash(stdout),cases},null,2)+'\n');
generateEd25519Headers(out);
console.log(`ED25519_EDGE_FROZEN cases=${cases.length} sha256=${hash(stdout)}`);
