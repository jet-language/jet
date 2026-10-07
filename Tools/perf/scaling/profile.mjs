// Responsible code for a superlinear phase (Tower #4601): one compile under
// `perf record` (realtime clock, frame-pointer call graphs), samples assigned
// to the JET_TRACE_FILE phase spans they fall in, and per phase the top
// self symbols and the top inclusive compiler functions, each resolved to
// its source file: a Jet compiler function (generated compiler, `jetc0`) to
// `Compiler/**.jet`, a Rust symbol (`jet`) to its crate module file.
//
// node profile.mjs BUILD_DIR TRACE_FILE PHASE...   (BUILD_DIR holds perf.data)

import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import readline from 'node:readline';
import { fileURLToPath } from 'node:url';
import { readTrace } from '../throughput/measure.mjs';

export const PERF = process.env.PERF ?? '/nix/store/v5fs9a5z17j8pfd2kfxs44hv5p3yq6ap-perf-linux-7.0.11/bin/perf';

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

const demangle = symbol => symbol.replace(/\+0x[0-9a-f]+$/, '').replace(/ \(.*\)$/, '').replace(/::h[0-9a-f]{16}$/, '');
// A compiler-owned frame: Jet compiler functions in a generated compiler and
// the `jet_*` crates of the Rust compiler.
const owned = symbol => /^<?(jetc_\w+|jet_\w+)::/.test(symbol) || /^(sema|jet|mir|tir|parser|lexer|lower|codegen|emit|optimize|ast|type|symbol|registration)_\w+$/.test(symbol);

let jetIndex = null;
// Jet compiler function name -> Compiler/<file>.jet:<line>.
function jetFunctions(repo) {
  if (jetIndex) return jetIndex;
  jetIndex = new Map();
  const walk = dir => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) walk(full);
      else if (entry.name.endsWith('.jet')) {
        fs.readFileSync(full, 'utf8').split('\n').forEach((line, index) => {
          const match = /^\s*(?:pub\s+)?fn\s+(\w+)/.exec(line);
          if (match && !jetIndex.has(match[1])) jetIndex.set(match[1], `${path.relative(repo, full)}:${index + 1}`);
        });
      }
    }
  };
  walk(path.join(repo, 'Compiler'));
  return jetIndex;
}

// Source location of one symbol, or null.
export function resolve(symbol, repo) {
  const plain = symbol.replace(/^<?jetc_\w+::/, '').replace(/^__jet_src_\w*?_c_c/, '');
  const name = plain.split('::').pop().replace(/[<>].*$/, '');
  const jet = jetFunctions(repo).get(name);
  if (jet && !/^jet_\w+::/.test(symbol)) return `${jet} ${name}`;
  const rust = /^<?(jet_[a-z_]+)::(.*)$/.exec(symbol);
  if (!rust) return null;
  const crate = path.join(repo, 'crates', rust[1].replace(/_/g, '-'), 'src');
  if (!fs.existsSync(crate)) return null;
  const segments = rust[2].split('::').filter(segment => /^[A-Za-z_]\w*$/.test(segment));
  for (let take = segments.length; take > 0; take--) {
    const base = path.join(crate, ...segments.slice(0, take));
    for (const candidate of [`${base}.rs`, path.join(base, 'mod.rs')]) {
      if (fs.existsSync(candidate)) return `${path.relative(repo, candidate)} ${segments[segments.length - 1] ?? name}`;
    }
  }
  return `${path.relative(repo, path.join(crate, 'lib.rs'))} ${name}`;
}

export const profileRun = {
  // argv prefix: record the compile with realtime timestamps and call graphs.
  wrap: dir => [PERF, 'record', '-q', '-F', '199', '-k', 'CLOCK_REALTIME', '--call-graph', 'fp', '-m', '16', '-o', path.join(dir, 'perf.data'), '--'],

  // Per phase: samples, top self symbols, top inclusive compiler functions.
  async attribute(dir, phases, { repo, traceFile, top = 12, ok = true } = {}) {
    const intervals = phaseIntervals(readTrace(traceFile));
    const wanted = new Set(phases);
    const stats = new Map(phases.map(phase => [phase, { samples: 0, self: new Map(), inclusive: new Map() }]));
    const phaseAt = t => {
      // The innermost wanted phase containing t.
      let best = null;
      for (const interval of intervals) {
        if (!wanted.has(interval.phase) || t < interval.start || t >= interval.end) continue;
        if (!best || interval.start >= best.start) best = interval;
      }
      return best?.phase ?? null;
    };
    let current = null;
    const flush = () => {
      if (!current) return;
      const phase = phaseAt(current.t);
      if (phase) {
        const s = stats.get(phase);
        s.samples += 1;
        const leaf = current.frames[0] ?? '?';
        s.self.set(leaf, (s.self.get(leaf) ?? 0) + 1);
        for (const frame of new Set(current.frames.filter(owned))) s.inclusive.set(frame, (s.inclusive.get(frame) ?? 0) + 1);
      }
      current = null;
    };
    const child = spawn(PERF, ['script', '-i', path.join(dir, 'perf.data'), '-F', 'time,ip,sym', '--ns'], { stdio: ['ignore', 'pipe', 'ignore'] });
    const lines = readline.createInterface({ input: child.stdout });
    for await (const line of lines) {
      const sample = /^\s*(\d+\.\d+):\s*$/.exec(line) ?? /^\s*(\d+\.\d+):/.exec(line);
      if (sample && !/^\s+[0-9a-f]+\s/.test(line)) { flush(); current = { t: Number(sample[1]), frames: [] }; continue; }
      const frame = /^\s+[0-9a-f]+\s+(.+)$/.exec(line);
      if (frame && current) current.frames.push(demangle(frame[1]));
    }
    flush();
    const ranked = (map, total) => [...map].sort((a, b) => b[1] - a[1]).slice(0, top)
      .map(([symbol, count]) => ({ share: count / Math.max(1, total), symbol, source: resolve(symbol, repo) }));
    return {
      perf_data: path.join(dir, 'perf.data'), compile_ok: ok,
      phases: Object.fromEntries([...stats].map(([phase, s]) => [phase, {
        samples: s.samples,
        self: ranked(s.self, s.samples),
        // Frames on (nearly) every sample of the phase are the phase driver itself.
        inclusive: ranked(new Map([...s.inclusive].filter(([, count]) => count < 0.95 * s.samples)), s.samples),
      }])),
    };
  },
};

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [dir, traceFile, ...phases] = process.argv.slice(2);
  if (!dir || !traceFile || !phases.length) {
    console.error('Usage: node profile.mjs BUILD_DIR TRACE_FILE PHASE...   (BUILD_DIR holds perf.data)');
    process.exit(2);
  }
  const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
  const result = await profileRun.attribute(path.resolve(dir), phases, { repo, traceFile: path.resolve(traceFile) });
  for (const [phase, data] of Object.entries(result.phases)) {
    console.log(`== ${phase}: ${data.samples} samples`);
    console.log('-- self');
    for (const row of data.self) console.log(`${(row.share * 100).toFixed(1).padStart(5)}%  ${row.symbol.slice(0, 120)}  ${row.source ?? ''}`);
    console.log('-- inclusive compiler functions');
    for (const row of data.inclusive) console.log(`${(row.share * 100).toFixed(1).padStart(5)}%  ${row.symbol.slice(0, 120)}  ${row.source ?? ''}`);
  }
}
