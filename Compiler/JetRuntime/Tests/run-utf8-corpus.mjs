#!/usr/bin/env node
// Native pack symbol proof for the UTF-8 validator, then the Rust utf8-corpus.rs
// acceptance witness. That witness is retired: this runner reports UNCOVERED
// with cardRequired:true and exits 2 without invoking rustc. The producer must
// supply this exact tree's freshly emitted Jet object and same-tree runtime pack.
import { existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { join, resolve } from 'node:path';

const [output] = process.argv.slice(2);
if (!output || process.argv.length !== 3) throw new Error('usage: run-utf8-corpus.mjs OUT');
const object = process.env.JET_RUNTIME_JET_OBJECT;
const pack = process.env.JET_RUNTIME_PACK;
if (!object || !pack || !existsSync(object) || !existsSync(pack)) {
  throw new Error('JET_RUNTIME_JET_OBJECT and JET_RUNTIME_PACK must name fresh same-tree Jet artifacts');
}
const out = resolve(output);
mkdirSync(out, { recursive: true });
const failures = [];
for (const [label, path] of [['object', object], ['pack', pack]]) {
  const symbols = spawnSync('nm', ['-g', '--defined-only', '--format=posix', path], { encoding: 'utf8', maxBuffer: 1 << 28 });
  writeFileSync(join(out, `${label}.symbols`), `${symbols.stdout ?? ''}${symbols.stderr ?? ''}`);
  if (symbols.error || symbols.status !== 0) failures.push(`${label} nm: ${symbols.error?.message ?? symbols.stderr}`);
  const definitions = (symbols.stdout ?? '').split('\n').map(line => line.trim().split(/\s+/u));
  const count = definitions.filter(([name, kind]) => name === 'jet_string_validate_utf8' && kind === 'T').length;
  if (count !== 1) failures.push(`${label}: validator has ${count} strong definitions`);
}
const reason = 'utf8-corpus.rs Rust acceptance witness retired (owner #38/#59; Tower #4923): exact UTF-8 acceptance and error run not executed; no rustc invoked';
const receipt = { status: 'UNCOVERED', cardRequired: true, reason, object, pack, symbolFailures: failures };
writeFileSync(join(out, 'receipt.json'), `${JSON.stringify(receipt, null, 2)}\n`);
console.log(JSON.stringify(receipt));
process.exit(2);
