#!/usr/bin/env node
// TDigest witness lane: links TDigestWitness.c against a freshly composed Jet
// runtime object, runs it over TDigestVectors.txt and compares the frozen
// TDigestWitness.stdout.
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import { faultReportObject } from '../../../Tools/packaging/fault-report-object.mjs';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const here = dirname(fileURLToPath(import.meta.url));
const [outArg, object, ...extra] = process.argv.slice(2);
if (!outArg || !object || extra.length) throw Error('usage: run-tdigest-witness.mjs OUT fresh-runtime.o');
const out = resolve(outArg);
mkdirSync(out);
const binary = join(out, 'tdigest-witness');
const link = spawnSync(nativeCCompiler, ['-std=c11', '-O1', join(here, 'TDigestWitness.c'), resolve(object), faultReportObject(out), '-lm', '-pthread', '-o', binary], { encoding: 'utf8', maxBuffer: 1 << 24 });
writeFileSync(join(out, 'link.log'), `${link.status}\n${link.stdout ?? ''}${link.stderr ?? ''}`);
if (link.error || link.status !== 0) throw Error(`link: ${link.error?.message ?? link.status}\n${link.stderr ?? ''}`);
const run = spawnSync(binary, [join(here, 'TDigestVectors.txt')], { encoding: 'utf8', timeout: 60000 });
writeFileSync(join(out, 'run.receipt.json'), JSON.stringify({ status: run.status, signal: run.signal, error: run.error?.message ?? null, stdout: run.stdout, stderr: run.stderr }, null, 2) + '\n');
const expected = readFileSync(join(here, 'TDigestWitness.stdout'), 'utf8');
if (run.error || run.status !== 0 || run.signal || run.stderr !== '' || run.stdout !== expected)
  throw Error(`tdigest witness: ${run.error?.message ?? run.status ?? run.signal}\n${run.stdout}${run.stderr}`);
console.log(run.stdout.trim());
