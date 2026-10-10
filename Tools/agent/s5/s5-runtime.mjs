#!/usr/bin/env node
// S5 runtime: one Jet-only pack production, independently audited published
// objects, and every native C/process-edge case. Rust providers are parked.
import fs from 'node:fs';
import path from 'node:path';
import {phase, W} from './s5-lib.mjs';
import {readSharedPack} from './shared-publications.mjs';
import {pathToFileURL} from 'node:url';
const {runtimeExportSignatures} = await import(pathToFileURL(path.join(W, 'Tools/agent/runtime-route-abi.mjs')));

const P = phase('runtime');
if (!P.ready) { P.finish(); process.exit(process.exitCode); }
const {D, run, start, blocked} = P;
const node = process.execPath;
const jet = path.join(process.env.S5_BUILD, 'jet');
const object = path.join(D, 'runtime', 'jet_runtime_jet.o');
const pack = path.join(D, 'jet_runtime.pack.o');
const packDir = path.join(process.env.S5_SHARED_PACK_DIRECTORY, 'jet-runtime');
const env = {JET: jet, JET_RUNTIME_JET_OBJECT: object, JET_RUNTIME_PACK: pack};
const haveJet = fs.existsSync(jet);
const receiptPath = path.join(process.env.S5_SHARED_PACK_DIRECTORY, 'receipt.json');
let produced = false;
try {
  if (!haveJet) throw Error('shared native CLI absent');
  const publication = readSharedPack({directory: process.env.S5_SHARED_PACK_DIRECTORY, head: P.head, source: W});
  produced = true;
  env.JET_RUNTIME_PACK = publication.pack;
  P.receipt('runtime-pack', 'PASS', {receipt: receiptPath, runtimeKey: publication.key, packSha256: publication.receipt.packSha256, producer: process.env.S5_SHARED_PACK_JOB ?? 'Main-RuntimePackLead-native-pack-r1'});
} catch (error) { blocked('runtime-pack', error.message); }

// The publisher is the sole compiler/lowerer. Verify its real object, pack,
// checked ABI and provenance before any consumer sees canonical phase paths.
let published = false;
if (!produced) blocked('pack-publication', 'Jet-only producer did not complete successfully');
else published = run('pack-publication', [node, '-e', `
const fs = require('fs'), path = require('path'), crypto = require('crypto');
const root = ${JSON.stringify(W)}, out = ${JSON.stringify(packDir)};
const key = fs.readFileSync(path.join(out, 'native.key'), 'utf8').trim();
const dir = path.join(out, key), receipt = JSON.parse(fs.readFileSync(path.join(dir, 'provenance.json'), 'utf8'));
const outer = JSON.parse(fs.readFileSync(path.join(out, 'provenance.json'), 'utf8'));
if (JSON.stringify(receipt) !== JSON.stringify(outer) || receipt.backend !== 'jet-native' ||
    receipt.runtimeKey !== key || receipt.rustObjects !== 0 || receipt.llvmObjects !== 0) throw Error('pack provenance is not one Jet-only production');
const hash = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
for (const [file, digest] of [['jet_runtime_jet.o', receipt.objectSha256], ['jet_runtime.pack.o', receipt.packSha256], ['runtime.abi.json', receipt.abiSha256]]) {
  if (!digest || hash(path.join(dir, file)) !== digest) throw Error('published artifact hash mismatch: ' + file);
}
if (hash(${JSON.stringify(jet)}) !== receipt.compilerSha256) throw Error('pack compiler does not match the shared build');
for (const group of ['source', 'backendInputs', 'lifecycle']) {
  if (!receipt[group]?.files?.length) throw Error('pack provenance lacks ' + group);
  for (const file of receipt[group].files) if (hash(path.resolve(root, file.path)) !== file.sha256) throw Error('pack input hash mismatch: ' + file.path);
}
for (const archive of receipt.cPlatformArchives ?? []) {
  if (archive.language !== 'C' || archive.role !== 'platform' || hash(archive.path) !== archive.sha256) throw Error('non-C or changed platform archive');
}
fs.mkdirSync(${JSON.stringify(path.dirname(object))}, {recursive:true});
fs.copyFileSync(path.join(dir, 'jet_runtime_jet.o'), ${JSON.stringify(object)});
fs.copyFileSync(path.join(dir, 'jet_runtime.pack.o'), ${JSON.stringify(pack)});
for (const file of ['provenance.json', 'runtime.abi.json', 'runtime.exports']) fs.copyFileSync(path.join(dir, file), path.join(${JSON.stringify(D)}, file));
console.log('one Jet-only object/pack; checked ABI, compiler and all input hashes verified');
`]);

const parallel = [];
if (!published) ['pack-exports', 'abi-witness'].forEach(name => blocked(name, 'no verified Jet-only publication'));
else {
    run('pack-exports', [node, '-e', `
const fs=require('fs'), {spawnSync}=require('child_process');
const result=spawnSync('nm',['-g','--defined-only','--format=posix',${JSON.stringify(pack)}],{encoding:'utf8'});
fs.writeFileSync(${JSON.stringify(path.join(D,'pack.exports.nm'))},result.stdout ?? '');
if(result.error || result.status !== 0) throw Error('pack nm failed');
const definitions=new Map(result.stdout.trim().split('\\n').map(row=>row.split(/\\s+/)));
const lost=${JSON.stringify([...runtimeExportSignatures(W).keys()])}.filter(name=>definitions.get(name)!=='T');
if(lost.length) throw Error('Jet exports lost from pack: '+lost.join(' '));
console.log('every declared Jet C export is defined in the pack');
`]);
    const witness = path.join(D, 'abi-witness');
    if (run('abi-witness-build', ['cc', '-O0', '-no-pie', '-o', witness, 'Compiler/JetRuntime/Tests/abi-witness.c', object, '-lpthread', '-ldl', '-lm'])) run('abi-witness', [witness]);
    else blocked('abi-witness', 'witness did not link');
}
if (!haveJet || !published) blocked('pack-roots', 'no verified same-pin hosted Jet and runtime publication');
else parallel.push(start('pack-roots', [node, 'Compiler/JetBackend/Tests/run-runtime-pack-roots.mjs', path.join(D, 'pack-roots')], env));
await Promise.all(parallel);
P.finish();
