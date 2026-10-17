#!/usr/bin/env node
// One Jet-front native golden union per pin; no Rust-checked MIR/reference leg.
// Runtime exit runners consume these actual results and retain their raw tests.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {phase, W} from './s5-lib.mjs';
import {readSharedPack} from './shared-publications.mjs';
import {preparedFiles} from './prepared.mjs';

const P = phase('goldens');
if (!P.ready) { P.finish(); process.exit(process.exitCode); }
const {D, run, start, blocked} = P;
const node = process.execPath, T = 'Compiler/JetRuntime/Tests';
const build = process.env.S5_BUILD, runtime = process.env.S5_RUNTIME;
const jetc0 = path.join(build, 'jetc0'), jet = path.join(build, 'jet');
const object = path.join(runtime, 'runtime', 'jet_runtime_jet.o');
let pack;
try { pack = readSharedPack({directory: process.env.S5_SHARED_PACK_DIRECTORY, head: P.head, source: W}).pack; }
catch (error) { blocked('shared-runtime-publication', error.message); P.finish(); process.exit(process.exitCode); }
const baseline = '/mnt/jetscratch/candidates/NativeLead3/nat-n7full.results.tsv';
const hash = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const env = {JET_RUNTIME_JET_OBJECT: object, JET_RUNTIME_PACK: pack, JET: jet, JET_MIR_EMIT: '0'};
const plans = [
  ['int', 'run-int-goldens', ['--plan'], 'int.cases'],
  ['unicode', 'run-unicode-goldens', ['--plan'], 'unicode.cases'],
  ['text', 'run-text-goldens', [baseline, '--plan'], 'text.cases'],
  ['collections', 'run-collections-goldens', [baseline, '--plan'], 'collections.cases'],
  ['trap', 'run-trap-goldens', [baseline, '--plan'], 'trap.cases'],
  ['host', 'run-host-goldens', ['prepare'], 'cases.list'],
  ['display', 'run-display-goldens', ['--prepare'], 'golden.cases'],
];
const union = new Set(['errors/default_err_value', 'errors/ambient_err', 'types/optional_result_variants']);
let passing;
try {
  const baselineRows = fs.readFileSync(baseline, 'utf8').trim().split('\n').map(line => line.split('\t'));
  if (hash(baseline) !== '89f7900fd37d7e0cf98aa6231a0e39d7f2f155643f40b7bcbbaada936ab4f0a1') throw Error('native baseline changed');
  passing = baselineRows.filter(([, verdict]) => verdict === 'pass').map(([stem]) => stem);
  if (passing.length !== 140 || new Set(passing).size !== 140) throw Error('baseline must hold 140 unique passes');
} catch (error) { blocked('native-baseline', error.message); P.finish(); process.exit(process.exitCode); }
for (const stem of passing) union.add(stem);
for (const stem of fs.readFileSync(path.join(W, T, 'scan.cases'), 'utf8').trim().split('\n')) union.add(stem);
const prepared = {};
for (const [name, runner, args, cases] of plans) {
  const directory = path.join(D, 'plans', name);
  if (run(`plan-${name}`, [node, `${T}/${runner}.mjs`, directory, ...args], env)) {
    const file = path.join(directory, cases);
    if (!fs.existsSync(file)) { blocked(`plan-cases-${name}`, `runner did not publish ${file}`); continue; }
    if (name === 'host' || name === 'display') prepared[name] = preparedFiles(directory);
    for (const stem of fs.readFileSync(file, 'utf8').trim().split('\n').filter(Boolean)) union.add(stem);
  }
}
const stems = [...union].sort();
fs.writeFileSync(path.join(D, 'native.cases'), stems.join('\n') + '\n');
const missing = [jetc0, jet, object, pack].filter(file => !fs.existsSync(file));
const shardCount = 8, width = Number(process.env.S5_GOLDEN_WIDTH ?? 2);
if (width !== 2) throw Error('S5_GOLDEN_WIDTH must be 2 for the declared 20G');
const directories = [];
if (missing.length) blocked('native-golden-union', `missing shared build/runtime inputs: ${missing.join(', ')}`);
else {
  const lists = Array.from({length: shardCount}, () => []);
  stems.forEach((stem, index) => lists[index % shardCount].push(stem));
  const tasks = lists.filter(list => list.length).map((list, index) => {
    const cases = path.join(D, `slice-${index}.cases`), directory = path.join(D, `slice-${index}`);
    fs.writeFileSync(cases, list.join('\n') + '\n'); directories.push(directory);
    return () => start(`native-slice-${index}`, ['timeout', '--kill-after=30s', '3000s', node, 'Compiler/JetBackend/Tests/run-goldens.mjs', directory, '--front', 'jet', '--case-list', cases], env);
  });
  await Promise.all(Array.from({length: width}, async () => { while (tasks.length) await tasks.shift()(); }));
}
// Merge real rows and process-edge artifacts, without creating synthetic PASSes.
const rows = new Map(), duplicates = [];
const runDirectory = path.join(D, 'run'); fs.mkdirSync(runDirectory);
for (const directory of directories) {
  const file = path.join(directory, 'results.tsv');
  if (!fs.existsSync(file)) continue;
  for (const line of fs.readFileSync(file, 'utf8').trim().split('\n').filter(Boolean)) {
    const [stem, verdict] = line.split('\t');
    if (rows.has(stem)) duplicates.push(stem);
    rows.set(stem, verdict);
  }
  const artifacts = path.join(directory, 'run');
  if (fs.existsSync(artifacts)) for (const file of fs.readdirSync(artifacts)) {
    const source = path.join(artifacts, file);
    if (fs.statSync(source).isFile()) fs.copyFileSync(source, path.join(runDirectory, file));
  }
}
const results = path.join(D, 'results.tsv');
fs.writeFileSync(results, stems.map(stem => `${stem}\t${rows.get(stem) ?? 'missing'}`).join('\n') + '\n');
const failures = stems.filter(stem => rows.get(stem) !== 'pass');
P.receipt('native-golden-union-results', failures.length || duplicates.length ? 'FAIL' : 'PASS', {required: stems.length, passing: stems.length - failures.length, failures, duplicates, results});
const baselineFailures = passing.filter(stem => rows.get(stem) !== 'pass');
P.receipt('jet-native-140-parity', baselineFailures.length ? 'FAIL' : 'PASS', {required: 140, passing: 140 - baselineFailures.length, failures: baselineFailures});
fs.writeFileSync(path.join(D, 'native-goldens-provenance.json'), JSON.stringify({head: P.head, backend: 'jet-native', results, resultsSha256: hash(results), packSha256: fs.existsSync(pack) ? hash(pack) : null, compilerSha256: fs.existsSync(jetc0) ? hash(jetc0) : null, shards: directories, stems, prepared}, null, 2) + '\n');
P.receipt('rust-checked-mir-native-140', 'UNCOVERED', {because: 'Rust oracle retired, owner #38/#59; Tower #4923; historical receipts remain Gate 2'});
P.finish();
