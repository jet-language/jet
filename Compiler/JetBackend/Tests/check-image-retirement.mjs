#!/usr/bin/env node
// One composed, held proof: same FINAL backend, pre-migration caller overlay
// versus migrated callers. Seed MIR/Jet-driver verification is not native
// compiler admission. Only an explicit lead-admitted pack supplies providers.
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync, readdirSync, rmSync } from 'node:fs';
import { join, resolve } from 'node:path';

const required = (name) => {
  const value = process.env[name];
  if (!value || value === 'TBD') throw new Error(`${name} is not pinned; keep this job held`);
  return value;
};
const repo = required('IMAGE_REPO');
const composed = required('IMAGE_COMPOSED_SHA');
const final = required('IMAGE_E2_FINAL');
const pack = required('JET_RUNTIME_PACK');
const receipt = required('IMAGE_RUNTIME_RECEIPT');
const seed = required('IMAGE_SEED_JET');
const out = resolve(required('IMAGE_PROOF_OUT'));
const base = '92549c6d87d05c4cfca844c8c3aa69e336a91e87';
const migration = required('IMAGE_MIGRATION_SHA');
const hash = (file) => createHash('sha256').update(readFileSync(file)).digest('hex');
for (const [file, pin] of [[pack, 'IMAGE_PACK_SHA256'], [receipt, 'IMAGE_RECEIPT_SHA256'], [seed, 'IMAGE_SEED_SHA256']]) {
  if (hash(file) !== required(pin)) throw new Error(`${file}: pinned hash mismatch`);
}
for (const sha of [final, composed, migration]) if (!/^[a-f0-9]{40}$/.test(sha)) throw new Error(`not an immutable SHA: ${sha}`);
const git = (...args) => {
  const r = spawnSync('git', ['-C', repo, ...args], { encoding: 'utf8', maxBuffer: 1 << 28 });
  if (r.status !== 0) throw new Error(`git ${args.join(' ')}: ${r.stderr}`);
  return r.stdout;
};
git('merge-base', '--is-ancestor', final, composed);
git('merge-base', '--is-ancestor', migration, composed);
mkdirSync(out, { recursive: true });
const rows = [], failures = [];
const env = { ...process.env, JET: seed, JET_LOWER_RUN: '1', JET_LOWER_SHARDS: '1', JET_LOWER_MEM: '8G', JET_RUNTIME_PACK: pack };
delete env.JET_RUNTIME_C_LIB;
delete env.FX_ONLY;
if (git('rev-parse', 'HEAD').trim() !== composed) throw new Error('IMAGE_REPO is not checked out at the composed pin');
delete env.FX_EXCLUDE;
delete env.FX_HEAD;
function step(name, command, args, cwd, mandatory = true) {
  const r = spawnSync(command, args, { cwd, env, encoding: 'utf8', maxBuffer: 1 << 28, timeout: 12 * 60 * 60 * 1000 });
  writeFileSync(join(out, `${name}.log`), `${r.stdout ?? ''}${r.stderr ?? ''}`);
  rows.push([name, r.status ?? r.signal ?? r.error?.code]);
  if (mandatory && r.status !== 0) failures.push(`${name}: ${r.status ?? r.signal ?? r.error?.message}`);
  return r;
}
const tests = 'Compiler/JetBackend/Tests';
const oldCallers = ['GoldenLower.jet', 'LowerFixtures.jet', 'AbiFixtures.jet', 'Fixtures/Unsigned.jet', 'run-goldens.mjs', 'run-lower-fixtures.mjs', 'check-native-fixtures.mjs'];
function snapshot(arm) {
  const dir = join(out, arm);
  mkdirSync(dir, { recursive: true });
  const archive = spawnSync('git', ['-C', repo, 'archive', composed], { maxBuffer: 1 << 30 });
  if (archive.status !== 0) throw new Error('composed source archive failed');
  const extracted = spawnSync('tar', ['-x', '-C', dir], { input: archive.stdout, maxBuffer: 1 << 20 });
  if (extracted.status !== 0) throw new Error('source snapshot extraction failed');
  if (arm === 'before') {
    for (const file of oldCallers) writeFileSync(join(dir, tests, file), git('show', `${base}:${tests}/${file}`));
    for (const file of ['TestPack.jet', 'Fixtures/ImageOverflow.jet']) rmSync(join(dir, tests, file));
  }
  return dir;
}
const dirs = Object.fromEntries(['before', 'after'].map((arm) => [arm, snapshot(arm)]));
// Stage-zero/composed sema includes TestPack's mutable module accesses in the
// assembled ABI unit; the parser alone is never credited for this condition.
step('composed-stage0', 'bash', [join(process.env.HOME, '.cache/jet-dev/stage0-check.sh'), repo, 'RtL7Host5-image'], repo);
for (const arm of ['before', 'after']) {
  const dir = dirs[arm], fx = join(out, `${arm}-fixtures`), golden = join(out, `${arm}-goldens`);
  step(`${arm}-assemble`, 'node', [join(dir, tests, 'run-lower-fixtures.mjs'), fx], dir);
  step(`${arm}-sema`, seed, ['check', join(fx, 'unit.jet')], fx);
  step(`${arm}-fixture-lower`, seed, ['run', 'unit.jet'], fx, false);
  step(`${arm}-fixture-check`, 'node', [join(dir, tests, 'check-native-fixtures.mjs'), fx], dir, false);
  // This mode actually exercises GoldenLower. Factory --front jet bypasses it.
  step(`${arm}-goldens`, 'node', [join(dir, tests, 'run-goldens.mjs'), golden, '--front', 'rust'], dir);
}
function goldenResults(arm) {
  return new Map(readFileSync(join(out, `${arm}-goldens/results.tsv`), 'utf8').trim().split('\n').map((line) => line.split('\t').slice(0, 2)));
}
function fixtureResults(arm) {
  const log = readFileSync(join(out, `${arm}-fixture-check.log`), 'utf8');
  const map = new Map();
  for (const match of log.matchAll(/^([\w-]+)(?: \([^\n]*?\))?: (ok|FAIL|MISSING|REFUSED)/gm)) {
    const [, name, verdict] = match;
    // The baseline can also print supplementary legs: any actual successful
    // old executable is an obligation for the corresponding new pack leg.
    if (map.get(name) !== 'ok') map.set(name, verdict);
  }
  return map;
}
const counts = [];
function compare(family, before, after) {
  const passed = (map) => [...map.values()].filter((v) => v === 'pass' || v === 'ok').length;
  counts.push([family, before.size, passed(before), after.size, passed(after)]);
  for (const [name, verdict] of before) {
    if (!after.has(name)) failures.push(`${family}: removed caller ${name}`);
    if (['pass', 'ok'].includes(verdict) && !['pass', 'ok'].includes(after.get(name))) failures.push(`${family}: pass regression ${name}`);
  }
  for (const [name, verdict] of after) if (['wrong', 'FAIL'].includes(verdict)) failures.push(`${family}: wrong result ${name}`);
  if (!before.size || passed(after) < passed(before)) failures.push(`${family}: counts did not meet equal-or-better gate`);
}
try {
  compare('GoldenLower', goldenResults('before'), goldenResults('after'));
  const before = fixtureResults('before'), after = fixtureResults('after');
  const unsigned = (name) => name.startsWith('unsigned') || name === 'bool-abi';
  compare('LowerFixtures/AbiFixtures', new Map([...before].filter(([name]) => !unsigned(name))), new Map([...after].filter(([name]) => !unsigned(name))));
  compare('Unsigned', new Map([...before].filter(([name]) => unsigned(name))), new Map([...after].filter(([name]) => unsigned(name))));
} catch (e) { failures.push(`counts: ${e.message}`); }
const fx = join(out, 'after-fixtures');
for (const [name, status, stdout] of [
  ['image-overflow-decimal', 0, '4611686018427387904\n9223372036854775808\n18446744073709551616\n-9223372036854775809\n'],
  ['word-entry-37', 37, ''], ['i32-entry-minus1', 255, ''], ['u32-entry-max', 255, ''], ['bool-entry-true', 1, ''],
]) {
  const r = step(`exact-${name}`, join(fx, `${name}.exe`), [], fx, false);
  if (r.status !== status || r.stdout !== stdout || r.stderr !== '') failures.push(`${name}: exact bytes/exit failed`);
}
// Resolve each live image import slot to the *actual* admitted pack definition.
// Addresses are emitted as a receipt, not inferred from old inline kernels.
const nm = step('pack-definitions', 'nm', ['-g', '--defined-only', pack], repo);
const definitions = new Map();
for (const line of (nm.stdout ?? '').split('\n')) {
  const match = /^\s*([a-fA-F0-9]+)\s+([A-Za-z])\s+(\S+)\s*$/.exec(line);
  if (!match || /[wWvV]/.test(match[2])) continue;
  const [, address, kind, symbol] = match;
  if (!definitions.has(symbol)) definitions.set(symbol, []);
  definitions.get(symbol).push({ address, kind });
}
const addresses = [];
for (const symbol of ['jet_rt_main', 'jet_rt_process_exit']) if (definitions.get(symbol)?.length !== 1) failures.push(`entry provider ${symbol}: not exactly one strong definition`);
for (const file of readdirSync(fx).filter((name) => name.endsWith('.mem'))) {
  const manifest = readFileSync(join(fx, file), 'utf8');
  const bytes = readFileSync(join(fx, file.replace(/\.mem$/, '.image')));
  const slots = new Set();
  for (const line of manifest.split('\n').filter((row) => row.startsWith('import '))) {
    const [, offsetText, symbol] = line.split(' '), offset = Number(offsetText);
    const defs = definitions.get(symbol) ?? [];
    if (!Number.isSafeInteger(offset) || offset % 8 || offset < 0 || offset + 8 > bytes.length || slots.has(offset) || defs.length !== 1 || !/[Tt]/.test(defs[0]?.kind ?? '')) failures.push(`${file}: invalid runtime address map ${line} (${defs.length} definitions)`);
    slots.add(offset);
    addresses.push([file, offset, symbol, defs[0]?.address ?? 'MISSING']);
  }
}
if (!addresses.length) failures.push('no live image runtime address-map witnesses');
writeFileSync(join(out, 'caller-counts.tsv'), 'family\tbefore-callers\tbefore-pass\tafter-callers\tafter-pass\n' + counts.map((r) => r.join('\t')).join('\n') + '\n');
writeFileSync(join(out, 'runtime-address-map.tsv'), 'image\tslot\tsymbol\tpack-object-address\n' + addresses.map((r) => r.join('\t')).join('\n') + '\n');
writeFileSync(join(out, 'steps.tsv'), rows.map((r) => r.join('\t')).join('\n') + '\n');
writeFileSync(join(out, 'receipt.json'), JSON.stringify({ base, final, composed, migration, pack, packSha256: hash(pack), runtimeReceipt: receipt, runtimeReceiptSha256: hash(receipt), seed, seedSha256: hash(seed), mode: 'seed-verification MIR + Jet driver; not native compiler admission', scalarContract: 'Convert zero-extends low source-width bits; old ELF.jet 82-95 exit_group keeps low eight bits. I32=-1 and U32=MAX ->255, Bool=true ->1; common transport serves every caller family.', counts, addressRows: addresses.length, failures }, null, 2) + '\n');
console.log(`image retirement gate: families=${counts.length}; address rows=${addresses.length}; failures=${failures.length}`);
for (const failure of failures) console.error(failure);
process.exit(failures.length ? 1 : 0);
