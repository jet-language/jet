#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { readTrace } from '../throughput/measure.mjs';

const sha256 = value => crypto.createHash('sha256').update(value).digest('hex');
const median = values => [...values].sort((a, b) => a < b ? -1 : a > b ? 1 : 0)[Math.floor(values.length / 2)];

// The runner's canonical receipt is line-oriented key=value, not JSON.
export function startupReceipt(text) {
  const fields = Object.fromEntries(text.split('\n').filter(Boolean).map(line => {
    const at = line.indexOf('=');
    if (at < 1) throw new Error('invalid canonical startup receipt');
    return [line.slice(0, at), line.slice(at + 1)];
  }));
  return { ...fields, complete: fields.complete === 'true', report_count: Number(fields.report_count), source_bytes: Number(fields.source_bytes) };
}

export function startupInputHash(root) {
  const entries = [];
  const walk = relative => {
    const dir = path.join(root, relative);
    for (const entry of fs.readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name < b.name ? -1 : a.name > b.name ? 1 : 0)) {
      const name = path.join(relative, entry.name);
      if (entry.isDirectory()) walk(name);
      else if (entry.isFile()) entries.push([name, sha256(fs.readFileSync(path.join(root, name)))]);
      else throw new Error('startup inputs must be an immutable regular-file snapshot');
    }
  };
  walk('');
  if (!entries.length) throw new Error('startup input snapshot is empty');
  return sha256(JSON.stringify(entries));
}

export function startupTiming(events, timeText, selectedEntry) {
  const stacks = new Map();
  const spans = [];
  const lastTime = new Map();
  for (const event of events) {
    if (event.ph !== 'B' && event.ph !== 'E') continue;
    if (!Number.isSafeInteger(event.pid) || !Number.isSafeInteger(event.tid) ||
        !Number.isSafeInteger(event.ts) || typeof event.name !== 'string' ||
        typeof event.cat !== 'string') throw new Error('invalid trace span identity');
    const thread = `${event.pid}:${event.tid}`;
    if (event.ts < (lastTime.get(thread) ?? 0)) throw new Error('invalid trace span time order');
    lastTime.set(thread, event.ts);
    const stack = stacks.get(thread) ?? [];
    stacks.set(thread, stack);
    if (event.ph === 'B') {
      stack.push({ event, compile: stack.find(frame => frame.event.cat === 'compile' && frame.event.name === 'compile')?.event });
    } else {
      const frame = stack.pop();
      if (!frame || frame.event.name !== event.name || frame.event.cat !== event.cat) throw new Error('unbalanced trace spans');
      spans.push({ begin: frame.event, end: event, compile: frame.compile });
    }
  }
  if ([...stacks.values()].some(stack => stack.length)) throw new Error('unbalanced trace spans');
  const compiles = spans.filter(span => span.begin.cat === 'compile' && span.begin.name === 'compile');
  const parses = spans.filter(span => span.begin.cat === 'parse' && span.begin.name === selectedEntry);
  if (compiles.length !== 1 || parses.length !== 1) throw new Error('missing or ambiguous compile/selected-entry parse boundaries');
  const { begin, end } = compiles[0];
  const firstParse = parses[0].begin;
  if (parses[0].compile !== begin || !(begin.ts < firstParse.ts && parses[0].end.ts <= end.ts)) throw new Error('invalid compile/selected-entry parse boundaries');
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
    if (row.receipt.complete !== true || row.receipt.report_count !== 0 || !Number.isSafeInteger(row.receipt.source_bytes) || row.receipt.source_bytes <= 0 || row.receipt.selected_factory_tier !== 'Aot' || row.receipt.actual_factory_tier !== 'Aot') throw new Error('startup compile did not complete through the canonical AOT factory');
    if (!/^[0-9a-f]{64}$/.test(row.artifact_sha256 ?? '') || row.artifact_sha256 !== oracle.artifact_sha256 || row.receipt.source_bytes !== oracle.receipt.source_bytes) throw new Error('paired emitted artifact differs');
    if (!/^[0-9a-f]{64}$/.test(row.source_sha256 ?? '') || row.source_sha256 !== oracle.source_sha256) throw new Error('paired actual project inputs differ');
    for (const name of ['wall_ns', 'compile_ns', 'pre_parse_ns']) {
      if (typeof row.timing[name] !== 'bigint' || row.timing[name] <= 0n) throw new Error('missing or invalid startup timing');
    }
  }
  for (const rows of [plain, optimized]) {
    if (!rows[0].receipt.compiler_identity || rows[0].receipt.compiler_identity === 'unavailable') throw new Error('compiler identity unavailable');
    if (rows.some(row => row.receipt.compiler_identity !== rows[0].receipt.compiler_identity)) throw new Error('compiler identity changed within an arm');
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
  const inputRoot = fs.realpathSync(path.join(root, 'inputs'));
  if (startupInputHash(inputRoot) !== pair.source_sha256) throw new Error('startup input snapshot differs from the paired source hash');
  if (typeof pair.entry !== 'string' || path.isAbsolute(pair.entry) ||
      path.relative(inputRoot, fs.realpathSync(path.join(inputRoot, pair.entry))) !== pair.entry ||
      !fs.statSync(path.join(inputRoot, pair.entry)).isFile()) throw new Error('startup pair must pin its relative source entry');
  const names = arm => fs.readdirSync(path.join(root, arm)).filter(name => /^trial-[0-9]+$/.test(name)).sort();
  const trials = names('plain');
  if (JSON.stringify(trials) !== JSON.stringify(names('optimized'))) throw new Error('unmatched startup trial directories');
  const readArm = arm => trials.map(name => {
    const dir = path.join(root, arm, name);
    const exitCode = fs.readFileSync(path.join(dir, 'rc'), 'utf8').trim();
    if (exitCode !== '0') throw new Error('startup process failed');
    const invocation = JSON.parse(fs.readFileSync(path.join(dir, 'invocation.json'), 'utf8'));
    if (fs.realpathSync(invocation.source_root) !== inputRoot || invocation.source_sha256 !== pair.source_sha256 || invocation.entry !== pair.entry) throw new Error('startup trial did not use the actual matched input snapshot and entry');
    const receipt = startupReceipt(fs.readFileSync(path.join(dir, 'receipt'), 'utf8'));
    if (!receipt.complete || receipt.report_count !== 0) throw new Error('startup compile did not complete');
    const traceFile = path.join(dir, 'trace.json');
    if (invocation.trace_sha256 !== sha256(fs.readFileSync(traceFile))) throw new Error('startup trace differs from the recorded completed process');
    const trace = readTrace(traceFile, { completedProcess: {
      exitCode: Number(exitCode), pid: invocation.process_pid,
      startedUnixMs: invocation.process_started_unix_ms, finishedUnixMs: invocation.process_finished_unix_ms,
    } });
    return {
      receipt,
      source_sha256: invocation.source_sha256,
      artifact_sha256: sha256(fs.readFileSync(path.join(dir, 'out.rs'))),
      timing: startupTiming(trace, fs.readFileSync(path.join(dir, 'time.txt'), 'utf8'), pair.entry),
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
