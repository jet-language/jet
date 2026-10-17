#!/usr/bin/env node
// S5 proof phase "boundary": the production compiler sources + each gate
// fixture (ExportBoundary.jet first: the sandbox/C export law merged from
// IOLead2 7dcd87e62), run natively by this commit's shared hosted `jet`.
// The four fixtures run concurrently; no Rust MIR interpreter is an oracle.
// Recipe: IOLead2 sandbox-proof.mjs step 6.
import fs from 'node:fs';
import path from 'node:path';
import {phase} from './s5-lib.mjs';

const P = phase('boundary');
if (!P.ready) { P.finish(); process.exit(process.exitCode); }
const {D, run, start, blocked} = P;
const jet = path.join(process.env.S5_BUILD, 'jet');
const fixtures = path.join(D, 'fixtures');
const expected = {
  'export-boundary': 'export boundary: passed',
};
if (!fs.existsSync(jet)) blocked('gate-fixtures', `no release jet at ${jet}`);
else if (run('fixtures-assemble', [process.execPath, 'Compiler/JetSema/Tests/run-gate-fixtures.mjs', fixtures])) {
  await Promise.all(['export-boundary', 'unsafe-obligations', 'gate-ledger', 'gate-projection'].map(name =>
    start(name, ['prlimit', '--stack=1073741824:1073741824', 'timeout', String(Number(process.env.S5_FIXTURE_TIMEOUT_S ?? 3000)), jet, 'run', 'src/compiler.jet'],
      {RUST_MIN_STACK: '268435456'},
      (code, row) => code === 0 && (!expected[name] || fs.readFileSync(row.stdout, 'utf8').includes(expected[name])),
      path.join(fixtures, name))));
} else blocked('gate-fixtures', 'fixture assembly failed');
P.finish();
