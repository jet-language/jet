#!/usr/bin/env node
// Emit the all-parts runtime text: `jet emit --rust` of AllpartsSeed.jet with
// the given jet. The output is the PROGRAM that prepare-call-runtime.mjs
// --allparts prepares for the native pack and the input
// Tools/agent/gen-c-abi-routes.mjs reads, so a gate regenerates it at its own
// head instead of reusing an older emission.
// usage: emit-allparts-runtime.mjs <jet> <output.rs>
import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const [jet, output] = process.argv.slice(2);
if (!jet || !output) throw new Error('usage: emit-allparts-runtime.mjs <jet> <output.rs>');
const out = resolve(output);
// The seed runs as a standalone script next to its output, outside the
// repository project, the way the emission was first produced.
const work = join(dirname(out), 'AllpartsSeed');
mkdirSync(work, { recursive: true });
copyFileSync(join(dirname(fileURLToPath(import.meta.url)), 'AllpartsSeed.jet'), join(work, 'seed.jet'));
const run = spawnSync(resolve(jet), ['emit', '--rust', 'seed.jet'], { cwd: work, encoding: 'utf8', maxBuffer: 1 << 30 });
if (run.status !== 0 || !run.stdout.includes('mod jet_c_abi {')) {
  writeFileSync(2, run.stderr ?? '');
  throw new Error(`jet emit --rust AllpartsSeed.jet failed (status ${run.status}${run.signal ? `, ${run.signal}` : ''})`);
}
writeFileSync(out, run.stdout);
console.log(`${out} ${run.stdout.length} bytes`);
