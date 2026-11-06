#!/usr/bin/env node
// The witness counting route and its negative control. Links
// AllocObserverWitness.c against a freshly composed Jet runtime object: the
// balanced run must pass with the runtime's own frees counted, and the `leak`
// run, which keeps one string buffer, must fail its balance assert.
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import { faultReportObject } from '../../../Tools/packaging/fault-report-object.mjs';
import { mkdirSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const here = dirname(fileURLToPath(import.meta.url));
const [outArg, object] = process.argv.slice(2);
if (!outArg || !object) throw Error('usage: run-alloc-observer-witness.mjs OUT fresh-runtime.o');
const out = resolve(outArg);
mkdirSync(out);
const binary = join(out, 'alloc-observer');
const link = spawnSync(nativeCCompiler, ['-std=c11', '-Wall', '-Wextra', '-Werror', '-pedantic', join(here, 'AllocObserverWitness.c'), resolve(object), faultReportObject(out), '-lm', '-pthread', '-o', binary], { encoding: 'utf8', maxBuffer: 1 << 24 });
writeFileSync(join(out, 'link.log'), `${link.status}\n${link.stdout ?? ''}${link.stderr ?? ''}`);
if (link.error || link.status !== 0) throw Error(`link: ${link.error?.message ?? link.status}\n${link.stderr ?? ''}`);
function run(name, args) {
  const r = spawnSync(binary, args, { encoding: 'utf8', timeout: 30000 });
  writeFileSync(join(out, `${name}.json`), JSON.stringify({ status: r.status, signal: r.signal, error: r.error?.message ?? null, stdout: r.stdout, stderr: r.stderr }, null, 2) + '\n');
  return r;
}
const balanced = run('balanced', []);
if (balanced.error || balanced.status !== 0 || !/^alloc observer: \d+ runtime allocations and frees balanced\n$/.test(balanced.stdout) || balanced.stderr !== '')
  throw Error(`balanced run must pass: ${balanced.status}/${balanced.signal}\n${balanced.stdout}${balanced.stderr}`);
const leak = run('leak', ['leak']);
if (leak.error || leak.signal !== 'SIGABRT' || leak.stdout !== '' || !/alloc observer: 1 of \d+ blocks still live\n/.test(leak.stderr) || !leak.stderr.includes('live == 0 && allocations == frees'))
  throw Error(`leak run must fail its balance assert: ${leak.status}/${leak.signal}\n${leak.stdout}${leak.stderr}`);
console.log(`${balanced.stdout.trim()}; leak control fails (${leak.signal})`);
