#!/usr/bin/env node
// Every non-parked Runtime/Tests runner is registered, collect-all, with one
// runner receipt. Rust modes are UNCOVERED, never a scheduler rc=0 PASS.
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {phase, W} from './s5-lib.mjs';
import {readSharedPack} from './shared-publications.mjs';
import {consumePrepared} from './prepared.mjs';

const P = phase('fixtures'), {D, start, blocked} = P;
const node = process.execPath, T = 'Compiler/JetRuntime/Tests';
const build = process.env.S5_BUILD, RT = process.env.S5_RUNTIME;
const jetc0 = path.join(build, 'jetc0'), jet = path.join(build, 'jet');
const object = path.join(RT, 'runtime', 'jet_runtime_jet.o');
let pack;
if (P.ready) {
  try { pack = readSharedPack({directory: process.env.S5_SHARED_PACK_DIRECTORY, head: P.head, source: W}).pack; }
  catch (error) { blocked('shared-runtime-publication', error.message); P.ready = false; }
}
const provenance = path.join(RT, 'provenance.json');
const goldens = process.env.S5_GOLDENS, results = path.join(goldens, 'results.tsv');
const inputs = process.env.S5_INPUTS;
const baseline = '/mnt/jetscratch/candidates/NativeLead3/nat-n7full.results.tsv';
const hash = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const sys = ['-lpthread', '-ldl', '-lm'];
const env = {JET: jet, JET_RUNTIME_JET_OBJECT: object, JET_RUNTIME_PACK: pack,
  JET_CHECKER: jet, CC: 'cc', NM: 'nm', LD: 'ld', COMPOSED_SHA: P.head};
const out = name => path.join(D, name), r = (file, ...args) => [node, `${T}/${file}.mjs`, ...args];
const registry = new Map(), executed = new Map();
const parked = new Map();
const driverDirectory = path.dirname(fileURLToPath(import.meta.url));
for (const line of fs.readFileSync(path.join(driverDirectory, 'parked-modes.tsv'), 'utf8').trim().split('\n').slice(1)) {
  const [runner, mode, lastReceipt, receiptScope, because, tower] = line.split('\t');
  if (!parked.has(runner)) parked.set(runner, []);
  parked.get(runner).push({mode, verdict: 'UNCOVERED', because, tower, lastReceipt, receiptScope});
}
if (!P.ready) {
  blocked('fixtures-prerequisites', 'successful same-pin build/runtime/golden receipts are required');
  const helpers = new Set(['report-literals.mjs', 'golden-results.mjs']);
  const retiredOnly = new Set(['freeze-ed25519-fixtures.mjs', 'freeze-entropy-fixtures.mjs', 'freeze-hash-fixtures.mjs', 'freeze-os-entropy-fixtures.mjs', 'freeze-pbkdf2-fixtures.mjs', 'freeze-sha3-fixtures.mjs', 'run-utf8-corpus.mjs']);
  for (const file of fs.readdirSync(path.join(W, T)).filter(file => file.endsWith('.mjs') && !helpers.has(file))) P.runner(file, retiredOnly.has(file) ? [] : ['fixtures-prerequisites'], parked.get(file) ?? []);
  P.runner('Compiler/JetBackend/Tests/test-native-test-host.mjs', ['fixtures-prerequisites'], parked.get('Compiler/JetBackend/Tests/test-native-test-host.mjs') ?? []);
  P.finish(); process.exit(process.exitCode);
}
let sealedInputs = false;
if (!inputs || !fs.existsSync(path.join(inputs, 'manifest.json'))) blocked('frozen-inputs', 'immutable preserved input manifest absent');
else try {
  const manifest = JSON.parse(fs.readFileSync(path.join(inputs, 'manifest.json'), 'utf8'));
  if (manifest.head !== P.head || !Array.isArray(manifest.files) || manifest.files.length !== 14) throw Error('immutable preserved input census/pin mismatch');
  const changed = manifest.files.filter(file => !file.path || path.isAbsolute(file.path) || file.path.split(path.sep).includes('..') || !fs.existsSync(path.join(inputs, file.path)) || hash(path.join(inputs, file.path)) !== file.sha256);
  if (changed.length || new Set(manifest.files.map(file => file.path)).size !== 14) throw Error('immutable preserved input digest/inventory mismatch');
  sealedInputs = true; P.receipt('frozen-inputs', 'PASS', {manifest: path.join(inputs, 'manifest.json'), files: manifest.files.map(({path, sha256}) => ({path, sha256}))});
} catch (error) { blocked('frozen-inputs', error.message); }
let sharedResults = false, goldenPublication = null;
const goldenReceipt = path.join(goldens, 'native-goldens-provenance.json');
if (!fs.existsSync(goldenReceipt) || !fs.existsSync(results)) blocked('shared-native-results', 'single native golden union receipt absent');
else try {
  const receipt = JSON.parse(fs.readFileSync(goldenReceipt, 'utf8'));
  if (receipt.head !== P.head || receipt.backend !== 'jet-native' || receipt.resultsSha256 !== hash(results) || !fs.existsSync(pack) || receipt.packSha256 !== hash(pack) || !fs.existsSync(jetc0) || receipt.compilerSha256 !== hash(jetc0)) throw Error('native golden source/compiler/pack/result provenance mismatch');
  goldenPublication = receipt; sharedResults = true;
  P.receipt('shared-native-results', 'PASS', {receipt: goldenReceipt, resultsSha256: receipt.resultsSha256});
} catch (error) { blocked('shared-native-results', error.message); }
const charOracle = inputs ? path.join(inputs, 'char-debug-goldens.bin') : out('absent-char-oracle');
const fmtExpected = inputs ? path.join(inputs, 'fmt-expected.out') : out('absent-fmt-expected');
const witnessDir = out('witnesses'); fs.mkdirSync(witnessDir);
const binary = name => path.join(witnessDir, name);
const add = (runner, commands, needs = [], extra = {}) => registry.set(`${runner}.mjs`, {commands, needs, extra});
const modes = (runner, names, needs = [], directory = runner) => add(runner, names.map(mode => r(runner, out(directory), mode)), needs);
const front = [jetc0, jet], runtime = [object, provenance], nativeGoldens = [...front, ...runtime, pack, results];

modes('run-channel-fixtures', ['assemble','assemble-stm','assemble-sync','assemble-observe','assemble-publication','lower','link','link-stm','run-stm','link-sync','run-sync','link-observe','run-observe','link-publication','run-publication','link-weak','run-weak','run','fatal','perf'], front);
for (const runner of ['run-atomic-fixtures','run-crypto-fixtures','run-parallel-fixtures','run-scheduler-fixtures','run-testsuite-fixtures']) modes(runner, ['assemble','lower','audit','link','run'], front);
for (const runner of ['run-residue-fixtures','run-shared-source-fixtures']) modes(runner, ['assemble','lower','link','run'], front);
modes('run-host-fixtures', ['assemble','lower','audit','compile','link','run'], front);
modes('run-thread-fixtures', ['assemble','lower','audit','link','run','shared-link','shared-run'], front);
modes('run-world-sentry-fixtures', ['assemble','lower','audit','link','run','render'], front);
modes('run-object-fixtures', ['--assemble','--lower','--global','--initializer','--allocator'], front);
modes('run-collections-stop', ['--assemble','--lower','--run'], front);
add('run-closure-refs-shared', ['assemble','lower','link','run'].map(mode => r('run-closure-refs-shared', out('closure-refs-shared'), mode, object)), [...front,...runtime]);
for (const runner of ['run-cabi-driver','run-alloc-parity','run-compiler-needed-math','run-int-cabi','run-int-widen','run-duration','run-http-body']) add(runner, [r(runner, out(runner), object)], runtime);
add('run-main-fixtures', ['link','run'].map(mode => r('run-main-fixtures', out('main-fixtures'), mode, object)), runtime);
add('check-object', [r('check-object', object, out('object-audit'))], runtime);
add('check-runtime', [r('check-runtime', out('check-runtime'), '--assemble-only'), r('check-runtime', out('check-runtime-full'))], front);
for (const runner of ['check-lists-native','check-collections-census','check-int-hazard-cutover','test-route-abi','test-native-route-handles','test-native-route-records']) add(runner, [r(runner)]);
for (const runner of ['run-text-goldens','run-collections-goldens','run-trap-goldens']) add(runner, [r(runner, out(runner), baseline, '--results', results)], nativeGoldens);
for (const runner of ['run-int-goldens','run-unicode-goldens']) add(runner, [r(runner, out(runner), '--results', results)], nativeGoldens);
add('run-host-goldens', [{prepared: 'host', directory: out('host-goldens')}, r('run-host-goldens', out('host-goldens'), 'native', '--results', results)], nativeGoldens);
add('run-scan-goldens', [r('run-scan-goldens', out('scan-goldens'), results)], nativeGoldens);
add('run-display-goldens', [{prepared: 'display', directory: out('display-goldens')}, r('run-display-goldens', out('display-goldens'), '--object', object, '--pack', pack, '--provenance', provenance, '--results', results)], nativeGoldens);
registry.get('run-display-goldens.mjs').missingOracleSteps = [1];
registry.get('run-display-goldens.mjs').needsByStep = [[], nativeGoldens];
add('run-fmt-parity', [r('run-fmt-parity', 'prepare', out('fmt-parity'), fmtExpected), r('run-fmt-parity', 'compare', out('fmt-parity'), jet, fmtExpected)], [...front, fmtExpected]);
add('run-test-runtime', [r('run-test-runtime', out('test-runtime'), object, jet)], [...runtime, jet]);
add('run-native-layout', [r('run-native-layout', '--native', out('native-layout'), object)], runtime);
const nfc = out('nfc-expected.scalars');
const normalization = path.join(W, 'tests/data/unicode/NormalizationTest.txt');
if (fs.existsSync(normalization)) {
  const rows = fs.readFileSync(normalization, 'utf8').split('\n').filter(line => /^[0-9A-F]+;/.test(line));
  if (hash(normalization) !== '5019ffd530751a741900c849c0e010332f142a3612234639bd200b82138a87db' || rows.length !== 20034) throw Error('official Unicode17 normalization input drift');
  const expected = rows.flatMap(line => {const columns = line.split(';').slice(0,5).map(col => col.trim().split(/\s+/).map(cp => Number.parseInt(cp,16).toString(16).toUpperCase()).join(' ')); return columns.map((_,index) => columns[index < 3 ? 1 : 3]);});
  fs.writeFileSync(nfc, expected.join('\n') + '\n');
}
add('run-native-text-normalize', [r('run-native-text-normalize', '--native', out('native-text-normalize'), object)], [...runtime,nfc], {JET_NFC_FROZEN_SCALARS:nfc});
const ui = {
  'accessibility-projection':'UiAccessibilityProjectionWitness','browser':'UiBrowserWitness','choices':'UiCarriersChoicesWitness',
  'clipboard':'UiClipboardWitness','dialog-selection':'UiDialogSelectionWitness','drag':'UiDragWitness','events':'UiEventsWitness',
  'file-dialog':'UiFileDialogWitness','form-action-error':'UiFormActionErrorWitness','host-error':'UiHostErrorWitness','ime':'UiImeWitness',
  'key':'UiKeyWitness','raylib-metadata':'UiRaylibMetadataWitness','shortcut-binding':'UiShortcutBindingWitness',
  'slice2':'UiCarriersSlice2Witness','slice3':'UiCarriersSlice3Witness','view-snapshots':'UiViewSnapshotsWitness',
};
// One Jet program runs every UI witness; each witness keeps its own runner row.
for (const [name, witness] of Object.entries(ui)) {
  const runner = `ui-witness-${name}`, args = name === 'key' ? [charOracle] : [];
  registry.set(runner, {commands: [[jet, 'run', `${T}/UiWitness`, '--', name, out(runner), binary(witness), ...args]], needs: [jet, binary(witness), ...args], extra: {}});
}
for (const [runner,witness] of [['run-root-cleanup','RootCleanupWitness'],['run-data-sequence-stops','DataTreeWitness'],['run-scalar-reflect-stops','ScalarReflectWitness']]) add(runner,[r(runner,out(runner),binary(witness))],[binary(witness)]);
add('run-record-variant-panic',[r('run-record-variant-panic',out('record-variant-panic'),...['DataTreeWitness','DataEventWitness','TaskVariantPanicWitness'].map(binary))],['DataTreeWitness','DataEventWitness','TaskVariantPanicWitness'].map(binary));
add('run-io-witness',[r('run-io-witness',binary('IOWitness'))],[binary('IOWitness')]);
add('run-execution-fixtures',[r('run-execution-fixtures',binary('execution-fixture'))],[binary('execution-fixture')]);

// Generator runners operate only in fresh writable data copies outside W.
for (const runner of ['generate-blake3-fixtures','generate-ed25519-headers','generate-rfc8032-fixtures']) {
  const directory = path.join(D,'data',runner,'Compiler/JetRuntime/Tests'); fs.mkdirSync(directory,{recursive:true});
  for (const file of fs.readdirSync(path.join(W,T)).filter(file => /\.(?:mjs|json|h)$/.test(file))) fs.copyFileSync(path.join(W,T,file),path.join(directory,file));
  if (runner==='generate-blake3-fixtures') fs.rmSync(path.join(directory,'CryptoBlake3Goldens.h'));
  if (runner==='generate-rfc8032-fixtures') fs.rmSync(path.join(directory,'Ed25519OfficialVectors.json'));
  const args = runner==='generate-rfc8032-fixtures' ? [out('rfc8032.txt')] : [];
  add(runner,[[node,path.join(directory,`${runner}.mjs`),...args]],args);
}
const official = JSON.parse(fs.readFileSync(path.join(W,T,'Ed25519OfficialVectors.json'),'utf8'));
await start('official-rfc8032-input',[node,'--input-type=module','-e',`import fs from 'node:fs';import crypto from 'node:crypto';const response=await fetch('https://www.rfc-editor.org/rfc/rfc8032.txt');if(!response.ok)throw Error('RFC8032 fetch '+response.status);const bytes=Buffer.from(await response.arrayBuffer());if(crypto.createHash('sha256').update(bytes).digest('hex')!==${JSON.stringify(official.sourceSha256)})throw Error('official RFC8032 bytes changed');fs.writeFileSync(${JSON.stringify(out('rfc8032.txt'))},bytes);`]);
add('freeze-age-fixtures',[r('freeze-age-fixtures',inputs?path.join(inputs,'age-rust'):out('absent-age-rust'),out('absent-age-testkit'),out('age-generated.json'))]);
registry.get('freeze-age-fixtures.mjs').missingOracle = true;
for (const runner of ['freeze-ed25519-fixtures','freeze-entropy-fixtures','freeze-hash-fixtures','freeze-os-entropy-fixtures','freeze-pbkdf2-fixtures','freeze-sha3-fixtures','run-utf8-corpus']) add(runner,[]);
for (const file of fs.readdirSync(path.join(W,T)).filter(file=>file.endsWith('.test.mjs'))) registry.set(file,{commands:[[node,'--test',`${T}/${file}`]],needs:[],extra:{}});
// Follow-ups depend on actual binaries/results of earlier runners.
add('run-http-body-perf',[r('run-http-body-perf',out('http-body-perf'),path.join(out('run-http-body'),'http-body.hosted'))],[path.join(out('run-http-body'),'http-body.hosted')]);
add('run-sentry-source-gate',[r('run-sentry-source-gate',out('sentry-source-gate'),path.join(out('run-world-sentry-fixtures'),'world-sentry-fixture'))],[path.join(out('run-world-sentry-fixtures'),'world-sentry-fixture')]);
add('test-unit-runtime',[]);

async function pool(tasks,width) {const queue=[...tasks];await Promise.all(Array.from({length:width},async()=>{while(queue.length)await queue.shift()();}));}
const width=Number(process.env.S5_FIXTURE_WIDTH??6);
if(!Number.isInteger(width)||width<2||width>6)throw Error('fixture width must be2..6 within declared20G');
const cryptoWitnesses=['Sha256','CryptoX25519','CryptoEd25519','CryptoChaCha','CryptoAead','CryptoHkdf','CryptoArgon','AgeCodec','AgeV1','CryptoEntropy','CryptoHash','CryptoPbkdf2','CryptoSha3','CryptoBlake3','CryptoEd25519Signature','CryptoZeroize','CryptoOsEntropy'].map(name=>`${name}Witness`);
const ownedWitness=new Set(['CAbiDriver','DisplayWitness','RootStdioWitness','HostFilesTests','HostSpawnTests',...cryptoWitnesses]);
const dedicated=new Set([...Object.values(ui),'RootCleanupWitness','DataTreeWitness','ScalarReflectWitness','DataEventWitness','TaskVariantPanicWitness','IOWitness']);
const witnesses=fs.readdirSync(path.join(W,T)).filter(file=>/Witness\.c$/.test(file)).map(file=>file.slice(0,-2)).filter(name=>!ownedWitness.has(name));
witnesses.push('execution-fixture');
await pool(witnesses.map(name=>async()=>{
  if(!fs.existsSync(object)||!fs.existsSync(provenance))return blocked(`witness-build-${name}`,'no sealed Jet-only runtime object');
  const built=await start(`witness-build-${name}`,['cc','-std=c11','-O1','-no-pie','-pthread',`${T}/${name}.c`,object,...sys,'-o',binary(name)],env);
  const golden=path.join(W,T,`${name}.stdout`);
  if(built&&!dedicated.has(name))await start(`witness-run-${name}`,[binary(name)],env,(code,row)=>code===0&&!row.signal&&(!fs.existsSync(golden)||fs.readFileSync(row.stdout).equals(fs.readFileSync(golden)))&&fs.readFileSync(row.stderr).length===0);
}),Math.min(16,os.availableParallelism()));
async function execute(file) {
  const definition=registry.get(file), names=[];executed.set(file,names);
  for(const [index,argv]of definition.commands.entries()) {
    const name=`${file.replace(/\.mjs$/,'')}-${index}`;names.push(name);
    if(definition.missingOracle||definition.missingOracleSteps?.includes(index)) {
      names.pop();
      const evidence={mode:name,because:'Rust oracle retired; required historical capture not preserved (owner #38(b)/#59)',cardRequired:true};
      P.receipt(name,'UNCOVERED',evidence);
      console.log(JSON.stringify({name,verdict:'UNCOVERED',...evidence}));
      if(!parked.has(file))parked.set(file,[]);
      parked.get(file).push({verdict:'UNCOVERED',...evidence});
      continue;
    }
    if (argv.prepared) {
      try {
        if (!sharedResults) throw Error('successful same-pin golden receipt absent');
        const files = consumePrepared(path.join(goldens, 'plans', argv.prepared), argv.directory, goldenPublication.prepared?.[argv.prepared]);
        P.receipt(name, 'PASS', {receipt: goldenReceipt, producerStep: `plan-${argv.prepared}`, files});
      } catch (error) { blocked(name, error.message); }
      continue;
    }
    const needs=definition.needsByStep?.[index]??definition.needs;
    const absent=needs.filter(file=>!fs.existsSync(file));
    if(needs.includes(results)&&!sharedResults)blocked(name,'same-pin native golden union absent or unsealed');
    else if((needs.includes(charOracle)||needs.includes(fmtExpected))&&!sealedInputs)blocked(name,'preserved frozen inputs absent or unsealed');
    else if(absent.length)blocked(name,`required inputs absent: ${absent.join(', ')}`);
    else await start(name,['timeout','--kill-after=10s','3000s',...argv],{...env,...definition.extra});
  }
}
const followups=new Set(['run-http-body-perf.mjs','run-sentry-source-gate.mjs','test-unit-runtime.mjs']);
await pool([...registry.keys()].filter(file=>!followups.has(file)).map(file=>()=>execute(file)),width);
await pool([...followups].filter(file=>file!=='test-unit-runtime.mjs').map(file=>()=>execute(file)),width);
// test-unit-runtime already executes in run-test-runtime; consume its REAL row.
const nested=path.join(out('test-runtime'),'results.json');
let unit=null,host=null;
try { if(fs.existsSync(nested)){const rows=JSON.parse(fs.readFileSync(nested,'utf8')).results;if(!Array.isArray(rows))throw Error('nested execution results malformed');unit=rows.find(row=>row.name==='unit-outcomes');host=rows.find(row=>row.name==='native-three-examples');} }
catch(error){blocked('nested-execution-receipt',error.message);}
for(const [file,row]of [['test-unit-runtime.mjs',unit],['Compiler/JetBackend/Tests/test-native-test-host.mjs',host]]){
  const name=`nested-${path.basename(file,'.mjs')}`;
  if(!row)blocked(name,'parent runner did not publish execution receipt');
  else P.receipt(name,row.status===0&&!row.error&&!row.signal?'PASS':'FAIL',{receipt:nested,result:row});
  executed.set(file,[name]);
}
// Fail closed on any present runner missing from the registry; library helpers
// are not executable runners. Removed parked runners retain UNCOVERED rows.
const helpers=new Set(['report-literals.mjs','golden-results.mjs']);
for(const file of fs.readdirSync(path.join(W,T)).filter(file=>file.endsWith('.mjs')&&!helpers.has(file)))if(!registry.has(file)){
  const name=`unmapped-${file}`;blocked(name,'runtime runner has no canonical S5 argv mapping');executed.set(file,[name]);
}
for(const [file,definition]of registry) {
  const names=executed.get(file)??[];
  const retired=parked.get(file)??[];
  if(!names.length&&!retired.length){const name=`unclassified-${file}`;blocked(name,'runner has no exercised native step or ratified parked mode');names.push(name);}
  P.runner(file,names,retired);
}
for(const[file,names]of executed)if(!registry.has(file))P.runner(file,names,parked.get(file)??[],file.startsWith('Compiler/')?nested:null);
P.finish();
