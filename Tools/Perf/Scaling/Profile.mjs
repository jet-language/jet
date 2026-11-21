// Responsible code for a superlinear phase (Tower #4601): one compile under
// `jet perf attach -- <compile>`, which samples its CPU from outside it, every
// thread and process it starts included, and keeps each sample's time and
// call stack in its trace's `timeline` section. The samples are assigned to
// the JET_TRACE_FILE phase spans they fall in, and per phase come the top
// self functions and the top inclusive compiler functions, each with the
// file and line that declares it.
//
// node profile.mjs BUILD_DIR TRACE_FILE PHASE...   (BUILD_DIR holds profile.jettrace)

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { readTrace } from '../Throughput/measure.mjs';

// Phase intervals in unix seconds: top-level spans (label == kind).
export function phaseIntervals(events) {
  const start = events.find(event => event.name === 'trace.start');
  const origin = start ? start.args.unix_ms / 1000 : 0;
  const open = new Map();
  const intervals = [];
  for (const event of events) {
    if ((event.ph !== 'B' && event.ph !== 'E') || event.name !== event.cat) continue;
    const key = `${event.tid}\0${event.cat}`;
    const t = origin + event.ts / 1e6;
    if (event.ph === 'B') { if (!open.has(key)) open.set(key, []); open.get(key).push(t); continue; }
    const begun = open.get(key)?.pop();
    if (begun !== undefined) intervals.push({ phase: event.cat, start: begun, end: t });
  }
  for (const [key, stack] of open) for (const begun of stack) intervals.push({ phase: key.split('\0')[1], start: begun, end: Infinity });
  return intervals;
}

// A compiler-owned function: one declared under `Compiler/`.
const owned = fn => /(^|\/)Compiler\//.test(fn.file ?? '');

// A function's source, `<file>:<line> <name>` with its bare name, or null
// when its image names no declaration.
const source = fn => fn.file ? `${fn.file}:${fn.line} ${fn.name.split('.').pop()}` : null;

// The `timeline` section of the `jet perf attach` trace `file`: its
// functions, and its samples in unix seconds, each with its frames' functions
// from the innermost; nothing when the trace is missing or has no timeline.
export function timeline(file) {
  const sampled = { functions: [], samples: [] };
  let trace;
  try { trace = JSON.parse(fs.readFileSync(file, 'utf8')); } catch { return sampled; }
  const found = trace?.content?.receipt_sections?.find(section => section.name === 'timeline')?.value;
  if (!found) return sampled;
  const origin = Number(found.origin_unix_ns) / 1e9;
  sampled.functions = found.functions;
  for (let at = 0; at + 1 < found.samples.length; at += 2) {
    const frames = [];
    for (let node = found.samples[at + 1]; node >= 0; node = found.nodes[2 * node + 1]) frames.push(found.functions[found.nodes[2 * node]]);
    sampled.samples.push({ t: origin + found.samples[at] / 1e6, frames });
  }
  return sampled;
}

export const profileRun = {
  // argv prefix: sample the compile from outside it with jet's own sampler.
  wrap: dir => [process.env.JET ?? 'jet', 'perf', 'attach', '--quiet', '--out', path.join(dir, 'profile.jettrace'), '--'],

  // Per phase: samples, top self functions, top inclusive compiler functions.
  async attribute(dir, phases, { traceFile, top = 12, ok = true } = {}) {
    const intervals = phaseIntervals(readTrace(traceFile));
    const wanted = new Set(phases);
    const stats = new Map(phases.map(phase => [phase, { samples: 0, self: new Map(), inclusive: new Map() }]));
    const sampled = timeline(path.join(dir, 'profile.jettrace'));
    // A function's source is that of the first function of its name in the
    // trace that has one.
    const sources = new Map();
    for (const fn of sampled.functions) if (source(fn) && !sources.has(fn.name)) sources.set(fn.name, source(fn));
    const phaseAt = t => {
      // The innermost wanted phase containing t.
      let best = null;
      for (const interval of intervals) {
        if (!wanted.has(interval.phase) || t < interval.start || t >= interval.end) continue;
        if (!best || interval.start >= best.start) best = interval;
      }
      return best?.phase ?? null;
    };
    for (const sample of sampled.samples) {
      const phase = phaseAt(sample.t);
      if (!phase) continue;
      const s = stats.get(phase);
      s.samples += 1;
      const leaf = sample.frames[0]?.name ?? '?';
      s.self.set(leaf, (s.self.get(leaf) ?? 0) + 1);
      for (const name of new Set(sample.frames.filter(owned).map(fn => fn.name))) s.inclusive.set(name, (s.inclusive.get(name) ?? 0) + 1);
    }
    const ranked = (map, total) => [...map].sort((a, b) => b[1] - a[1]).slice(0, top)
      .map(([symbol, count]) => ({ share: count / Math.max(1, total), symbol, source: sources.get(symbol) ?? null }));
    return {
      trace: path.join(dir, 'profile.jettrace'), compile_ok: ok,
      phases: Object.fromEntries([...stats].map(([phase, s]) => [phase, {
        samples: s.samples,
        self: ranked(s.self, s.samples),
        // Functions on (nearly) every sample of the phase are the phase driver itself.
        inclusive: ranked(new Map([...s.inclusive].filter(([, count]) => count < 0.95 * s.samples)), s.samples),
      }])),
    };
  },
};

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [dir, traceFile, ...phases] = process.argv.slice(2);
  if (!dir || !traceFile || !phases.length) {
    console.error('Usage: node profile.mjs BUILD_DIR TRACE_FILE PHASE...   (BUILD_DIR holds profile.jettrace)');
    process.exit(2);
  }
  const result = await profileRun.attribute(path.resolve(dir), phases, { traceFile: path.resolve(traceFile) });
  for (const [phase, data] of Object.entries(result.phases)) {
    console.log(`== ${phase}: ${data.samples} samples`);
    console.log('-- self');
    for (const row of data.self) console.log(`${(row.share * 100).toFixed(1).padStart(5)}%  ${row.symbol.slice(0, 120)}  ${row.source ?? ''}`);
    console.log('-- inclusive compiler functions');
    for (const row of data.inclusive) console.log(`${(row.share * 100).toFixed(1).padStart(5)}%  ${row.symbol.slice(0, 120)}  ${row.source ?? ''}`);
  }
}
