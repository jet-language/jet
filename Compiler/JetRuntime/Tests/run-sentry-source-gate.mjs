#!/usr/bin/env node
// tests/unsafe_sentries.rs:3-28 over the real composed native Sentry fixture.
// NODE-DEBT removal condition: at native cutover, the Jet test kit runner owns
// this real-fixture process launch and all six ledger predicates; remove this
// Node runner only after the Jet runner exercises the identical contract.
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
const [output, fixture] = process.argv.slice(2);
if (!output || !fixture) throw Error('usage: run-sentry-source-gate.mjs OUT REAL_WORLD_SENTRY_FIXTURE');
const out = resolve(output);
mkdirSync(out, { recursive: true });
const run = mkdtempSync(join(out, 'source-gate-'));
const ledger = join(run, 'ledger/ledger.jsonl');
const result = spawnSync(resolve(fixture), ['source-gate'], { encoding: 'utf8', timeout: 30000,
  env: { ...process.env, JET_MEMORY_LEDGER: ledger } });
writeFileSync(join(run, 'stdout'), result.stdout ?? '');
writeFileSync(join(run, 'stderr'), result.stderr ?? '');
writeFileSync(join(run, 'receipt.json'), JSON.stringify({ status: result.status, signal: result.signal,
  error: result.error?.message ?? null, ledger }, null, 2) + '\n');
if (result.error || result.signal || result.status !== 0 || result.stdout !== '' || result.stderr !== '') {
  throw Error(`source-gate fixture failed: ${result.error ?? result.signal ?? result.status}; see ${run}`);
}
const witness = readFileSync(ledger, 'utf8');
for (const predicate of [
  '"schema":"jet.memory.ledger"',
  '"kind":"sentry"',
  '"code":"R0801"',
  '"source":"unsafe.jet"',
  '"provenance":"source #Unsafe gate"',
  'derive the pointer from live storage inside this gate',
]) {
  if (!witness.includes(predicate)) throw Error(`source-gate ledger lacks ${JSON.stringify(predicate)}`);
}
if (!witness.length || witness.trimEnd().split('\n').length !== 1) throw Error('source-gate fault must write exactly one ledger row');
console.log('source-gate R0801/source metadata and six ledger predicates passed');
