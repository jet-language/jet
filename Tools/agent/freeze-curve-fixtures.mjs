#!/usr/bin/env node
// Once-only existing-Rust-seed oracle for #4855. No Cargo/new dependencies.
// Regenerate Jet corpus from immutable JSON with --render; capture is refused
// after seed.stdout exists. Private Poly tests are emitted as a review fragment.
import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { resolve, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const out = join(root, 'tests/conformance/corpus/core/crypto/ct');
const hash = x => createHash('sha256').update(x).digest('hex');
const vec = hex => `vec![${Buffer.from(hex, 'hex').join(',')}]`;
const jet = hex => `[U8]{${Buffer.from(hex, 'hex').join(',')}}`;
function run(cmd, args) {
  const r = spawnSync(cmd, args, { encoding: 'utf8', timeout: 120000, maxBuffer: 32 * 1024 * 1024 });
  if (r.error || r.status !== 0) throw Error(`${cmd} tool_exit=${r.status}: ${r.error ?? r.stderr}`);
  return r.stdout;
}
function render(f) {
  let x = '// core-conformance supplementary: RFC 7748 and frozen seed differential\nuse core.crypto.expert as expert\n\nfn run() {\n    #Unsafe("published/frozen X25519 test inputs") {\n';
  for (const c of f.x) {
    x += `        // ${c.label}\n`;
    if (c.result === '00'.repeat(32)) x += `        rejected :: if expert.x25519_raw(${jet(c.key)}, ${jet(c.point)}) == {\n            .Err(.KeyRejected) -> true\n            .Err(_) -> false\n            .Ok(_) -> false\n        }\n        assert(rejected)\n`;
    else x += `        assert(expert.secret_bytes(expert.x25519_raw(${jet(c.key)}, ${jet(c.point)}) ?? panic("X25519")) == ${jet(c.result)})\n`;
  }
  x += `        k := ${jet('09' + '00'.repeat(31))}\n        u := ${jet('09' + '00'.repeat(31))}\n        loop i in 0..<1000 {\n            next :: expert.secret_bytes(expert.x25519_raw(k, u) ?? panic("iteration"))\n            u = k\n            k = next\n            if i == 0 -> assert(k == ${jet(f.iterations[0])})\n        }\n        assert(k == ${jet(f.iterations[1])})\n    }\n    print(true)\n}\n`;
  writeFileSync(join(out, '..', 'ct_x25519.jet'), x);
  let e = '// core-conformance supplementary: RFC 8032 and frozen Core/seed semantics\nuse core.crypto as crypto\nuse core.crypto.expert as expert\n\n';
  for (const [index, c] of f.ed.entries()) {
    e += `// ${c.label}\nfn case_${index}() {\n        public :: ${jet(c.publicKey)}\n        message :: ${jet(c.message)}\n        signature :: ${jet(c.signature)}\n`;
    if (c.seed) e += `        key :: crypto.SigningKey.from_bytes(${jet(c.seed)}) ?? panic("seed")\n        assert(key.public_key().bytes == public)\n        assert((crypto.sign(key, message) ?? panic("sign")).bytes == signature)\n        #Unsafe("frozen Ed25519 sign witness") {\n            assert((expert.ed25519_sign(${jet(c.seed)}, message) ?? panic("expert sign")).bytes == signature)\n        }\n`;
    e += `        assert((crypto.verify(crypto.VerifyKey{bytes: public}, message, crypto.Signature{bytes: signature}) ?? panic("verify")) == ${Boolean(c.coreOrdinary)})\n        #Unsafe("public Ed25519 acceptance witness") {\n`;
    if (c.coreStrict < 0) e += '            rejected :: if expert.ed25519_verify_strict(public, message, signature) == {\n                .Err(.KeyRejected) -> true\n                .Err(_) -> false\n                .Ok(_) -> false\n            }\n            assert(rejected)\n';
    else e += `            assert((expert.ed25519_verify_strict(public, message, signature) ?? panic("strict")) == ${Boolean(c.coreStrict)})\n`;
    e += '        }\n}\n\n';
  }
  e += `fn run() {\n${f.ed.map((_,i)=>`    case_${i}()`).join('\n')}\n    print(true)\n}\n`;
  writeFileSync(join(out, '..', 'ct_ed25519.jet'), e);
}
function polyFragment(f) {
  const p = f.poly[0], boundary = f.poly.filter(c => c.label.startsWith('poly_boundary_')), randomCases = f.poly.filter(c => c.label.startsWith('random_poly_'));
  const sizes = boundary.map(c => c.message.length / 2);
  let state = 0x6d2b79f5;
  const next = () => { state ^= state << 13; state ^= state >>> 17; state ^= state << 5; return state & 255; };
  for (let i=0;i<44;i++) for(let j=0;j<32+sizes[i%sizes.length];j++) next();
  for (let i=0;i<64*64+4*32;i++) next();
  const start = state >>> 0;
  for(const c of randomCases) {
    const key = Buffer.from(Array.from({length:32},next)).toString('hex');
    const msg = Buffer.from(Array.from({length:c.message.length/2},next)).toString('hex');
    if(key!==c.key||msg!==c.message) throw Error('random Poly fixture replay mismatch');
  }
  const tags = cases => `[[U8]]{${cases.map(c=>jet(c.result)).join(',\n        ')}}`;
  return `\n#Test("RFC8439 Poly1305 and frozen boundary/random differential") {\n    assert(poly1305(${jet(p.key)}, ${jet(p.message)}) == ${jet(p.result)})\n    sizes :: [Int]{${sizes.join(',')}}\n    tags :: ${tags(boundary)}\n    key := [U8]{}\n    loop i in 0..<32 -> &key.push(U8{(i * 197 + 23).bit_and(255)})\n    loop i in 0..<sizes.len() {\n        data := [U8]{}\n        loop j in 0..<sizes[i] -> &data.push(U8{(j * 29 + 7).bit_and(255)})\n        assert(poly1305(key, data) == tags[i])\n    }\n    random_tags :: ${tags(randomCases)}\n    state := U32{${start}}\n    loop i in 0..<32 {\n        random_key := [U8]{}\n        data := [U8]{}\n        loop j in 0..<(32 + sizes[i %% sizes.len()]) {\n            state = state.bit_xor(state << 13)\n            state = state.bit_xor(state >> 17)\n            state = state.bit_xor(state << 5)\n            if j < 32 -> &random_key.push(U8{state.bit_and(U32{255})})\n            else -> &data.push(U8{state.bit_and(U32{255})})\n        }\n        assert(poly1305(random_key, data) == random_tags[i])\n    }\n}\n`;
}
if (process.argv[2] === '--render') {
  const f = JSON.parse(readFileSync(join(out, 'fixtures.json')));
  render(f);
  if(process.argv[3]) writeFileSync(process.argv[3], polyFragment(f));
  console.log('CURVE_RENDER tool_exit=0');
  process.exit(0);
}
const [scratch, deps, official, edges] = process.argv.slice(2);
if (!edges) throw Error('usage: freeze-curve-fixtures.mjs SCRATCH EXISTING_DEPS RFC8032_JSON EXISTING_ED_EDGE_JSON');
if (existsSync(join(out, 'seed.stdout'))) throw Error('refusing to recapture curve seed');
mkdirSync(scratch, { recursive: true });
const officialData = JSON.parse(readFileSync(official));
const edgeData = JSON.parse(readFileSync(edges));
const ed = officialData.cases.map(c => ({ ...c, published: officialData.url }));
ed.push(...edgeData.cases.filter(c => !c.seed));
let rng = 0x6d2b79f5;
function random(n) { const b = Buffer.alloc(n); for (let i = 0; i < n; i++) { rng ^= rng << 13; rng ^= rng >>> 17; rng ^= rng << 5; b[i] = rng & 255; } return b.toString('hex'); }
const sizes = [0,1,2,7,15,16,17,31,32,33,63,64,65,111,112,113,127,128,129,255,256,257];
for (let i = 0; i < 44; i++) ed.push({ label: `random_sign_${i}`, seed: random(32), message: random(sizes[i % sizes.length]) });
const identity = '01' + '00'.repeat(31), negativeZero = '01' + '00'.repeat(30) + '80';
for (const [label, a, r] of [['negative_zero_A',negativeZero,identity],['negative_zero_R',identity,negativeZero]]) ed.push({label,publicKey:a,message:'',signature:r+'00'.repeat(32)});
const x = [
 ['rfc_7748_1','a546e36bf0527c9d3b16154b82465edd62144c0ac1fc5a18506a2244ba449ac4','e6db6867583030db3594c1a424b15f7c726624ec26b3353b10a903a6d0ab1c4c','c3da55379de9c6908e94ea4df28d084f32eccf03491c71f754b4075577a28552'],
 ['rfc_7748_2','4b66e9d4d1b4673c5ad22691957d6af5c11b6421e0ea01d42ca4169e7918ba0d','e5210f12786811d3f4b7959d0538ae2c31dbe7106fc03c3efc4cd549c715a493','95cbde9476e8907d7aade45cb4b873f88b595a68799fa152e6f8f7647aac7957'],
].map(([label,key,point,result]) => ({label,key,point,result,published:'https://www.rfc-editor.org/rfc/rfc7748#section-5.2'}));
for (const c of [...x]) { const p=Buffer.from(c.point,'hex');p[31]^=128;x.push({...c,label:c.label+'_topbit',point:p.toString('hex')}); }
for (let i = 0; i < 64; i++) x.push({label:`random_x_${i}`,key:random(32),point:random(32)});
for (const p of ['00'.repeat(32), identity, 'ed'+'ff'.repeat(30)+'7f', 'ee'+'ff'.repeat(30)+'7f']) x.push({label:`low_order_${x.length}`,key:random(32),point:p});
const poly = [{label:'rfc8439_2_5_2', key:'85d6be7857556d337f4452fe42d506a80103808afb0db2fd4abff6af4149f51b', message:Buffer.from('Cryptographic Forum Research Group').toString('hex'), result:'a8061dc1305136c6c22b8baf0c0127a9', published:'https://www.rfc-editor.org/rfc/rfc8439#section-2.5.2'}];
// These deterministic boundary inputs keep in-file #Test witnesses compact.
for (const n of sizes) poly.push({label:`poly_boundary_${n}`,key:Array.from({length:32},(_,i)=>(i*197+23)&255).map(x=>x.toString(16).padStart(2,'0')).join(''),message:Array.from({length:n},(_,i)=>(i*29+7)&255).map(x=>x.toString(16).padStart(2,'0')).join('')});
for (let i=0;i<32;i++) poly.push({label:`random_poly_${i}`,key:random(32),message:random(sizes[i % sizes.length])});
const source = `use ed25519_dalek::{SigningKey,VerifyingKey,Signature,Signer,Verifier};
use curve25519_dalek::{constants::ED25519_BASEPOINT_POINT,edwards::CompressedEdwardsY,scalar::Scalar};
use sha2::{Digest,Sha512};
use poly1305::{Poly1305,universal_hash::KeyInit};
fn hex(b:&[u8])->String{b.iter().map(|v|format!("{v:02x}")).collect()}
fn canonical(b:[u8;32])->bool{let mut y=b;y[31]&=127;let mut p=[255;32];p[0]=237;p[31]=127;(0..32).rev().find_map(|i|if y[i]!=p[i]{Some(y[i]<p[i])}else{None}).unwrap_or(false)}
fn ed(label:&str,seed:Vec<u8>,m:Vec<u8>,mut a:Vec<u8>,mut sig:Vec<u8>){
 if !seed.is_empty(){let k=SigningKey::from_bytes(&seed.try_into().unwrap());a=k.verifying_key().to_bytes().to_vec();sig=k.sign(&m).to_bytes().to_vec();}
 let aa:[u8;32]=a.clone().try_into().unwrap();let rr:[u8;32]=sig[..32].try_into().unwrap();let ss:[u8;32]=sig[32..].try_into().unwrap();
 let ap=if canonical(aa){CompressedEdwardsY(aa).decompress()}else{None};let rp=if canonical(rr){CompressedEdwardsY(rr).decompress()}else{None};let scalar:Option<Scalar>=Scalar::from_canonical_bytes(ss).into();
 let wide:[u8;64]=Sha512::new().chain_update(rr).chain_update(aa).chain_update(&m).finalize().into();let k=Scalar::from_bytes_mod_order_wide(&wide);
 let ordinary=match(ap,rp,scalar){(Some(a),Some(r),Some(s))=>ED25519_BASEPOINT_POINT*s==r+a*k,_=>false};
 let strict=match(ap,rp,scalar){(Some(a),Some(r),Some(_))=>if a.compress().to_bytes()!=aa{-1}else{(r.compress().to_bytes()==rr&&!a.is_small_order()&&!r.is_small_order()&&ordinary)as i32},(Some(a),_,_)=>if a.compress().to_bytes()!=aa{-1}else{0},_=>-1};
 let vk=VerifyingKey::from_bytes(&aa);let ds=Signature::from_bytes(&sig.clone().try_into().unwrap());let dalek=vk.as_ref().map(|v|v.verify(&m,&ds).is_ok()).unwrap_or(false);let dalek_strict=vk.as_ref().map(|v|v.verify_strict(&m,&ds).is_ok()).unwrap_or(false);
 println!("ed {label} {} {} {} {} {} {}",hex(&a),hex(&sig),ordinary as i32,strict,dalek as i32,dalek_strict as i32);
}
fn main(){
${x.map(c=>`println!("x ${c.label} {}",hex(&x25519_dalek::x25519(${vec(c.key)}.try_into().unwrap(),${vec(c.point)}.try_into().unwrap())));`).join('\n')}
let mut k=[0u8;32];k[0]=9;let mut u=k;for i in 0..1000{let next=x25519_dalek::x25519(k,u);u=k;k=next;if i==0||i==999{println!("iteration {} {}",i+1,hex(&k));}}
${ed.map(c=>`ed(${JSON.stringify(c.label)},${vec(c.seed??'')},${vec(c.message)},${vec(c.publicKey??'')},${vec(c.signature??'')});`).join('\n')}
${poly.map(c=>`println!("poly ${c.label} {}",hex(&Poly1305::new_from_slice(&${vec(c.key)}).unwrap().compute_unpadded(&${vec(c.message)})));`).join('\n')}
}`;
const src=join(scratch,'seed.rs'),bin=join(scratch,'seed');writeFileSync(src,source);
const rlibs=[['ed25519_dalek','libed25519_dalek-8963a5f6e6c4570c.rlib'],['curve25519_dalek','libcurve25519_dalek-2d7b74116500e8c9.rlib'],['sha2','libsha2-ea5a89a90f592458.rlib'],['x25519_dalek','libx25519_dalek-5f7e060eb15cffd0.rlib'],['poly1305','libpoly1305-c4a5158101c378ce.rlib']];
run('rustc',['--edition=2021',src,'-o',bin,'-L',`dependency=${deps}`,...rlibs.flatMap(([name,file])=>['--extern',`${name}=${join(deps,file)}`])]);
const stdout=run(bin,[]);const rows=new Map(stdout.trim().split('\n').map(l=>{const [kind,label,...v]=l.split(' ');return [`${kind}/${label}`,v];}));
for(const c of x){const result=rows.get(`x/${c.label}`)[0];if(c.result&&c.result!==result)throw Error(`${c.label} seed != published`);c.result=result;}
for(const c of ed){const [a,s,ordinary,strict,dalek,dalekStrict]=rows.get(`ed/${c.label}`);if(c.seed&&c.signature&&(s!==c.signature||a!==c.publicKey))throw Error(`${c.label} seed != RFC8032`);Object.assign(c,{publicKey:a,signature:s,coreOrdinary:Number(ordinary),coreStrict:Number(strict),dalekOrdinary:Number(dalek),dalekStrict:Number(dalekStrict)});}
for(const c of poly){const result=rows.get(`poly/${c.label}`)[0];if(c.result&&c.result!==result)throw Error(`${c.label} seed != RFC8439`);c.result=result;}
const iterations=[rows.get('iteration/1')[0],rows.get('iteration/1000')[0]];
if(iterations[0]!=='422c8e7a6227d7bca1350b3e2bb7279f7897b87bb6854b783c60e80311ae3079'||iterations[1]!=='684cf59ba83309552800ef566f2f4d3c1c3887c49360e3875f2eb94d99532c51')throw Error('seed != RFC7748 iterated');
const fixture={seed:{rustc:run('rustc',['--version']).trim(),sourceSha256:hash(source),stdoutSha256:hash(stdout),dependencies:rlibs.map(([name,file])=>({name,path:join(deps,file),sha256:hash(readFileSync(join(deps,file)))})),officialEdSha256:hash(readFileSync(official)),heldEdgesSha256:hash(readFileSync(edges)),nonClaim:'Frozen reference outputs only. Jet differential/execution and machine constant-time proof are not exercised by capture.'},x,ed,poly,iterations};
mkdirSync(out,{recursive:true});writeFileSync(join(out,'seed.stdout'),stdout);writeFileSync(join(out,'fixtures.json'),JSON.stringify(fixture,null,2)+'\n');render(fixture);
writeFileSync(join(scratch,'poly-tests.jet'),polyFragment(fixture));
console.log(`CURVE_SEED tool_exit=0 x=${x.length} ed=${ed.length} poly=${poly.length} iterations=1/1000 stdout_sha256=${hash(stdout)}`);
