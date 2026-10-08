#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';

const sha256 = value => crypto.createHash('sha256').update(value).digest('hex');
const median = values => [...values].sort((a, b) => a < b ? -1 : a > b ? 1 : 0)[Math.floor(values.length / 2)];

export function startupTiming(events, timeText) {
  const begin = events.find(event => event.name === 'compile' && event.ph === 'B');
  const end = events.find(event => event.name === 'compile' && event.ph === 'E');
  const firstParse = events.find(event => event.name === 'main' && event.cat === 'parse' && event.ph === 'B');
  if (![begin?.ts, end?.ts, firstParse?.ts].every(Number.isSafeInteger) || !(begin.ts < firstParse.ts && firstParse.ts < end.ts)) throw new Error('missing or invalid compile/first-parse trace boundaries');
  const elapsed = /Elapsed \(wall clock\) time \(h:mm:ss or m:ss\):\s*(?:(\d+):)?(\d+):(\d+(?:\.\d+)?)/.exec(timeText);
  if (!elapsed) throw new Error('missing process wall time');
  const wallSeconds = Number(elapsed[1] ?? 0) * 3600 + Number(elapsed[2]) * 60 + Number(elapsed[3]);
  if (!Number.isFinite(wallSeconds) || wallSeconds <= 0) throw new Error('invalid process wall time');
  return { wall_ns: BigInt(Math.round(wallSeconds * 1e9)), compile_ns: BigInt(end.ts - begin.ts) * 1000n, pre_parse_ns: BigInt(firstParse.ts - begin.ts) * 1000n };
}

export function compareStartup(plain, optimized) {
  if (plain.length < 3 || plain.length !== optimized.length) throw new Error('require at least three matched startup trials');
  const oracle = plain[0];
  for (const row of [...plain, ...optimized]) {
    if (row.receipt.complete !== true || row.receipt.report_count !== 0 || !Number.isSafeInteger(row.receipt.source_bytes) || row.receipt.source_bytes <= 0 || row.receipt.selected_factory_tier !== 'aot' || row.receipt.actual_factory_tier !== 'aot') throw new Error('startup compile did not complete through the canonical AOT factory');
    if (!/^[0-9a-f]{64}$/.test(row.artifact_sha256 ?? '') || row.artifact_sha256 !== oracle.artifact_sha256 || row.receipt.source_bytes !== oracle.receipt.source_bytes) throw new Error('paired compiler output or source closure differs');
    for (const name of ['wall_ns', 'compile_ns', 'pre_parse_ns']) {
      if (typeof row.timing[name] !== 'bigint' || row.timing[name] <= 0n) throw new Error('missing or invalid startup timing');
    }
  }
  for (const rows of [plain, optimized]) {
    if (!rows[0].receipt.compiler_identity || rows.some(row => row.receipt.compiler_identity !== rows[0].receipt.compiler_identity)) throw new Error('compiler identity changed within an arm');
  }
  const metrics = {};
  for (const name of ['wall_ns', 'compile_ns', 'pre_parse_ns']) {
    const base = median(plain.map(row => row.timing[name]));
    const candidate = median(optimized.map(row => row.timing[name]));
    metrics[name] = { plain_ns: String(base), optimized_ns: String(candidate), verdict: candidate < base ? 'win' : 'loss' };
  }
  return { schema: 'jet.native-host-shape-pair.v1', trials: plain.length, artifact_sha256: oracle.artifact_sha256, metrics, verdict: Object.values(metrics).every(row => row.verdict === 'win') ? 'win' : 'loss' };
}

export function compareStartupDirectory(root) {
  const pair = JSON.parse(fs.readFileSync(path.join(root, 'pair.json'), 'utf8'));
  if (pair.schema !== 'jet.native-host-shape-pair.v1' || !/^[0-9a-f]{64}$/.test(pair.source_sha256 ?? '') || !pair.machine || !pair.build_profile) throw new Error('startup pair must record shared source hash, machine, and build profile');
  const names = arm => fs.readdirSync(path.join(root, arm)).filter(name => /^trial-[0-9]+$/.test(name)).sort();
  const trials = names('plain');
  if (JSON.stringify(trials) !== JSON.stringify(names('optimized'))) throw new Error('unmatched startup trial directories');
  const readArm = arm => trials.map(name => {
    const dir = path.join(root, arm, name);
    if (fs.readFileSync(path.join(dir, 'rc'), 'utf8').trim() !== '0') throw new Error('startup process failed');
    return {
      receipt: JSON.parse(fs.readFileSync(path.join(dir, 'receipt'), 'utf8')),
      artifact_sha256: sha256(fs.readFileSync(path.join(dir, 'out.rs'))),
      timing: startupTiming(JSON.parse(fs.readFileSync(path.join(dir, 'trace.json'), 'utf8')), fs.readFileSync(path.join(dir, 'time.txt'), 'utf8')),
    };
  });
  return { ...compareStartup(readArm('plain'), readArm('optimized')), pair, trial_directories: trials };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [root, out] = process.argv.slice(2);
  if (!root || !out) throw new Error('usage: startup.mjs PAIR_DIRECTORY NEW_RECEIPT_FILE');
  const receipt = compareStartupDirectory(path.resolve(root));
  fs.writeFileSync(out, JSON.stringify(receipt, null, 2) + '\n', { flag: 'wx' });
  console.log(JSON.stringify(receipt, null, 2));
  if (receipt.verdict !== 'win') process.exitCode = 1;
}
