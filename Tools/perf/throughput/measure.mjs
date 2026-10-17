// Process-tree measurement and trace phase extraction for the
// compile-throughput harness (Tower #4529). Dependency-free, Linux /proc.
//
// One measured command runs under GNU time (exact whole-tree user/system CPU
// from rusage and the largest single-process high-water RSS) while a sampler
// walks the live process tree every SAMPLE_MS: summed RSS (the tree peak),
// summed threads, and cumulative tree CPU (live utime+stime plus the
// cutime+cstime of reaped children), stamped on the same clock as the
// compiler's JET_TRACE_FILE spans. Per-phase CPU is the sampled CPU curve
// read across each phase's wall interval, so its resolution is the sampling
// interval and the kernel clock tick.

import fs from 'node:fs';
import { spawn, spawnSync } from 'node:child_process';

export const GNU_TIME = '/nix/store/n0wrh3vjfwcqfyswwai0zcxvkpibq34v-time-1.10/bin/time';
export const SAMPLE_MS = 20;
const TICKS_PER_SECOND = Number(spawnSync('getconf', ['CLK_TCK'], { encoding: 'utf8' }).stdout.trim()) || 100;
const PAGE_BYTES = Number(spawnSync('getconf', ['PAGESIZE'], { encoding: 'utf8' }).stdout.trim()) || 4096;
const NS_PER_TICK = 1e9 / TICKS_PER_SECOND;

// /proc/<pid>/stat fields after the parenthesised command name.
function readStat(pid) {
  let text;
  try { text = fs.readFileSync(`/proc/${pid}/stat`, 'utf8'); } catch { return null; }
  const fields = text.slice(text.lastIndexOf(')') + 2).split(' ');
  // fields[0] is field 3 (state): utime=14, stime=15, cutime=16, cstime=17,
  // num_threads=20, rss=24 in proc(5) numbering.
  const at = number => Number(fields[number - 3]);
  return { cpuTicks: at(14) + at(15) + at(16) + at(17), threads: at(20), rssBytes: at(24) * PAGE_BYTES };
}

function children(pid) {
  const result = [];
  let tasks;
  try { tasks = fs.readdirSync(`/proc/${pid}/task`); } catch { return result; }
  for (const task of tasks) {
    let text = '';
    try { text = fs.readFileSync(`/proc/${pid}/task/${task}/children`, 'utf8'); } catch { continue; }
    for (const child of text.trim().split(/\s+/)) if (child) result.push(Number(child));
  }
  return result;
}

// One sample of the tree rooted at `root`.
export function sampleTree(root) {
  const seen = new Set();
  const stack = [root];
  let cpuTicks = 0;
  let threads = 0;
  let rssBytes = 0;
  let processes = 0;
  while (stack.length) {
    const pid = stack.pop();
    if (seen.has(pid)) continue;
    seen.add(pid);
    const stat = readStat(pid);
    if (!stat) continue;
    processes += 1;
    cpuTicks += stat.cpuTicks;
    threads += stat.threads;
    rssBytes += stat.rssBytes;
    stack.push(...children(pid));
  }
  return { cpuNs: cpuTicks * NS_PER_TICK, threads, rssBytes, processes };
}

// Run argv under GNU time and the sampler. Resolves with exact totals and
// the sample series ({tNs, unixMs, cpuNs, rssBytes, threads}); times are
// nanoseconds from the spawn.
export function runMeasured(argv, { cwd, env, stdoutPath, stderrPath, timeoutMs }) {
  return new Promise((resolve, reject) => {
    const timeFile = `${stderrPath}.time`;
    fs.rmSync(timeFile, { force: true });
    const out = fs.openSync(stdoutPath, 'w');
    const err = fs.openSync(stderrPath, 'w');
    const spawnUnixMs = Date.now();
    const start = process.hrtime.bigint();
    const child = spawn(GNU_TIME, ['-f', '%e %U %S %M', '-o', timeFile, ...argv], { cwd, env, stdio: ['ignore', out, err] });
    fs.closeSync(out);
    fs.closeSync(err);
    const samples = [];
    const take = () => {
      const tNs = Number(process.hrtime.bigint() - start);
      samples.push({ tNs, ...sampleTree(child.pid) });
    };
    const timer = setInterval(take, SAMPLE_MS);
    let killed = false;
    const deadline = timeoutMs ? setTimeout(() => { killed = true; killTree(child.pid); }, timeoutMs) : null;
    child.on('error', error => { clearInterval(timer); if (deadline) clearTimeout(deadline); reject(error); });
    child.on('exit', (code, signal) => {
      const wallNs = Number(process.hrtime.bigint() - start);
      clearInterval(timer);
      if (deadline) clearTimeout(deadline);
      let rusage = null;
      try {
        const text = fs.readFileSync(timeFile, 'utf8').trim().split('\n').pop();
        const match = /^(\d+(?:\.\d+)?) (\d+(?:\.\d+)?) (\d+(?:\.\d+)?) (\d+)$/.exec(text);
        if (match) rusage = { userNs: Math.round(Number(match[2]) * 1e9), sysNs: Math.round(Number(match[3]) * 1e9), maxRssBytes: Number(match[4]) * 1024 };
      } catch { /* a missing rusage line fails the sample below */ }
      const peakSampled = samples.reduce((peak, sample) => Math.max(peak, sample.rssBytes), 0);
      resolve({
        code, signal, killed, wallNs, spawnUnixMs, samples, rusage,
        peakRssBytes: Math.max(peakSampled, rusage?.maxRssBytes ?? 0),
        threadsUsed: samples.reduce((peak, sample) => Math.max(peak, sample.threads), 0),
      });
    });
  });
}

function killTree(pid) {
  for (const child of children(pid)) killTree(child);
  try { process.kill(pid, 'SIGKILL'); } catch { /* already gone */ }
}

// Cumulative tree CPU at `tNs`, linear between samples (0 before the first).
export function cpuAt(samples, tNs) {
  if (!samples.length || tNs <= samples[0].tNs) return samples.length ? samples[0].cpuNs * Math.max(0, tNs) / Math.max(1, samples[0].tNs) : 0;
  for (let i = 1; i < samples.length; i++) {
    if (tNs <= samples[i].tNs) {
      const a = samples[i - 1];
      const b = samples[i];
      return a.cpuNs + (b.cpuNs - a.cpuNs) * (tNs - a.tNs) / Math.max(1, b.tNs - a.tNs);
    }
  }
  return samples[samples.length - 1].cpuNs;
}

// JET_TRACE_FILE is an open array of newline-terminated object/comma rows.
// Live readers may see a partial last row; completed proofs must bind the
// producer's process identity to a successful, independently recorded run.
export function readTrace(file, { completedProcess } = {}) {
  const text = fs.readFileSync(file, 'utf8');
  const strict = completedProcess !== undefined;
  if (strict && (completedProcess.exitCode !== 0 ||
      !Number.isSafeInteger(completedProcess.pid) || completedProcess.pid <= 0 ||
      !Number.isSafeInteger(completedProcess.startedUnixMs) ||
      !Number.isSafeInteger(completedProcess.finishedUnixMs) ||
      completedProcess.startedUnixMs > completedProcess.finishedUnixMs)) {
    throw new Error('completed trace requires successful recorded process identity');
  }
  if (strict && (!text.startsWith('[\n') || !text.endsWith('\n'))) {
    throw new Error('invalid or truncated completed trace stream');
  }
  const events = [];
  const lines = text.split('\n');
  for (const [index, raw] of lines.entries()) {
    if (strict && (index === 0 || index === lines.length - 1)) continue;
    if (strict && (!raw.startsWith('{') || !raw.endsWith('},'))) {
      throw new Error('invalid or truncated completed trace event');
    }
    const line = raw.trim().replace(/,$/, '');
    if (!line.startsWith('{')) continue;
    try {
      events.push(JSON.parse(line));
    } catch (error) {
      if (strict) throw new Error('malformed completed trace event', { cause: error });
      // A live producer may still be writing the last event.
    }
  }
  if (strict) {
    const starts = events.filter(event => event.name === 'trace.start' && event.ph === 'i');
    if (starts.length !== 1 || !Number.isSafeInteger(starts[0].args?.unix_ms) ||
        starts[0].args.unix_ms < completedProcess.startedUnixMs ||
        starts[0].args.unix_ms > completedProcess.finishedUnixMs ||
        events.some(event => event.pid !== completedProcess.pid)) {
      throw new Error('completed trace does not match the recorded process identity');
    }
    const stacks = new Map();
    const lastTime = new Map();
    for (const event of events) {
      if (!Number.isSafeInteger(event.tid) || event.tid < 0 ||
          typeof event.name !== 'string' || !event.name ||
          !['M', 'i', 'C', 'B', 'E'].includes(event.ph)) throw new Error('invalid completed trace event schema');
      // Producer metadata has no timestamp. Its unix_ms anchor and the
      // independently recorded process window have millisecond precision.
      if (event.ph === 'M' && event.ts === undefined) continue;
      if (!Number.isSafeInteger(event.ts) || event.ts < 0 ||
          Math.floor(starts[0].args.unix_ms + event.ts / 1000) > completedProcess.finishedUnixMs) {
        throw new Error('completed trace event is outside the recorded process window');
      }
      const thread = `${event.pid}:${event.tid}`;
      if (event.ts < (lastTime.get(thread) ?? 0)) throw new Error('invalid completed trace time order');
      lastTime.set(thread, event.ts);
      if (event.ph !== 'B' && event.ph !== 'E') continue;
      if (typeof event.cat !== 'string' || !event.cat) throw new Error('invalid completed trace span category');
      const stack = stacks.get(thread) ?? [];
      stacks.set(thread, stack);
      if (event.ph === 'B') {
        stack.push(event);
      } else {
        const begin = stack.pop();
        if (!begin || begin.name !== event.name || begin.cat !== event.cat) throw new Error('unbalanced completed trace spans');
      }
    }
    if ([...stacks.values()].some(stack => stack.length)) throw new Error('unbalanced completed trace spans');
  }
  return events;
}

// Phase spans are trace spans whose label is their kind (item spans carry a
// module or function label). Per kind: `wall_ns` is the union of its spans
// over all threads (critical-path wall), `span_ns` their summed duration
// (thread time), `cpu_ns` the tree CPU spent inside the union, `count` and
// `items`. `offsetNs` places trace time 0 on the sampler's clock.
export function phaseTable(events, samples, offsetNs) {
  const open = new Map();
  const spans = new Map();
  let firstBegin = null;
  for (const event of events) {
    if ((event.ph !== 'B' && event.ph !== 'E') || event.name !== event.cat) continue;
    const key = `${event.tid}\0${event.cat}`;
    const tNs = event.ts * 1000 + offsetNs;
    if (event.ph === 'B') {
      if (firstBegin === null || tNs < firstBegin) firstBegin = tNs;
      if (!open.has(key)) open.set(key, []);
      open.get(key).push(tNs);
    } else {
      const begun = open.get(key)?.pop();
      if (begun === undefined) continue;
      if (!spans.has(event.cat)) spans.set(event.cat, []);
      spans.get(event.cat).push({ start: begun, end: tNs, items: event.args?.items ?? null });
    }
  }
  const unclosed = [...open.entries()].filter(([, stack]) => stack.length).map(([key]) => key.split('\0')[1]);
  const table = {};
  for (const [kind, list] of [...spans.entries()].sort(([a], [b]) => a.localeCompare(b))) {
    list.sort((a, b) => a.start - b.start);
    const union = [];
    for (const span of list) {
      const last = union[union.length - 1];
      if (last && span.start <= last.end) last.end = Math.max(last.end, span.end);
      else union.push({ start: span.start, end: span.end });
    }
    const items = list.reduce((sum, span) => span.items === null ? sum : (sum ?? 0) + span.items, null);
    table[kind] = {
      wall_ns: Math.round(union.reduce((sum, span) => sum + span.end - span.start, 0)),
      span_ns: Math.round(list.reduce((sum, span) => sum + span.end - span.start, 0)),
      cpu_ns: Math.round(union.reduce((sum, span) => sum + cpuAt(samples, span.end) - cpuAt(samples, span.start), 0)),
      count: list.length,
      items,
    };
  }
  return { phases: table, firstBeginNs: firstBegin, unclosed };
}

// JET_PHASE_COUNTERS lines (the #4319 counter protocol, Tools/perf/scaling).
export { parseCounters } from '../scaling/harness.mjs';
