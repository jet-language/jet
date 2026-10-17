#!/usr/bin/env node
// O-level benchmark sweep: every batch-mode gauntlet program (Tools/gauntlet/entries, jet and jet-expert
// variants) compiled by one native Jet compiler at each requested optimization level, every binary run
// N times, output checked against expected.out and across levels (AGENTS.md I9), runtime compared with
// a same-run Rust/C build and with the newest gauntlet receipt of every matched peer (AGENTS.md
// Performance gate: Rust parity band 1.05, every other peer < 1.00).
//
//   node sweep.mjs --compiler r29g=/path/to/jet-wrapper --level O0=build --level O2=build,--release \
//     --unavailable 'O1=r29g CLI has no O1 route' --runs 5 --out DIR [--entries sieve,nbody]
//   (round 2) --level O0=run --level O1=build --level O2=build,--release
//
// A level is `VERB[,FLAG...]`: `build` compiles with `jet build FLAGS run.jet`; `run` compiles with
// `jet run FLAGS run.jet -- ARGS` (the program runs once inside that measurement; the compile cost is
// then also reported net of the binary's median runtime). The binary is `.jet/build/run` either way.
// `--compiler` must name an executable that already carries the compiler's environment
// (JET_TOOLCHAIN_ROOT, JET_COMPILER_IDENTITY, stack limit).
//
// Programs are processed one at a time (compile every level, then the same-run peers, then every
// binary's runs); results.json and the tables are rewritten after each program, so a running sweep
// always has a current partial table. Per compile: wall (monotonic), user+sys CPU and peak RSS (GNU
// time), the compiler's phase trace (JET_TRACE_FILE), each compile alone in its own
// `systemd-run --user --scope` (MemoryMax=--scope-mem, no swap, RuntimeMaxSec). A compile killed before
// lowering began skips the program's remaining levels (the front end is level-independent). Per run:
// same metrics, run directly (no scope: its startup cost would swamp short programs), fixture restored
// before every run. `--cpus` pins compiles, runs and peers to one CPU set.
// `--cli-adapter argv` (default) rewrites a `#CLI struct Args` + `fn run(args: Args)` entry into a
// plain `fn run()` that reads the same positional arguments from `process.argv()`, because the native
// compilers reject the typed CLI entry (E1321); `literal` writes the spec arguments in as literals
// instead (no core.process import). The body is untouched and both digests are recorded.
//
// Writes DIR/results.json (every sample), DIR/table.md and DIR/table.tsv. Exit 1 when any
// level's output differs from expected.out or from another level, or any compile or run fails.
// `sweep.mjs --tables-only DIR` re-renders the tables of an existing results.json.
import { createHash } from 'node:crypto';
import { spawn } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync, copyFileSync, mkdtempSync } from 'node:fs';
import { dirname, join, resolve, basename } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir, cpus } from 'node:os';

const GNU_TIME = existsSync('/run/current-system/sw/bin/time') ? '/run/current-system/sw/bin/time' : '/usr/bin/time';
const RUST_BAND = 1.05;

function usage(message) {
  if (message) process.stderr.write(`sweep: ${message}\n`);
  process.stderr.write('usage: sweep.mjs --compiler LABEL=PATH --level NAME=VERB[,FLAG...]... [--unavailable NAME=REASON]... --out DIR\n' +
    '  [--runs 5] [--warmup 1] [--entries a,b (also the order)] [--scope-mem 6G] [--cpus 24-31] [--peers rust,c] [--repo DIR]\n' +
    '  [--compile-timeout 1800] [--run-timeout 300] [--cli-adapter argv|literal|off] [--identity TEXT]\n');
  process.exit(2);
}

function parseArgs(argv) {
  const o = { levels: [], unavailable: [], runs: 5, warmup: 1, entries: null, scopeMem: '6G', cpus: null, peers: ['rust', 'c'], repo: null, compileTimeout: 1800, runTimeout: 300, cliAdapter: 'argv', identity: null, profileLevels: [], perf: 'perf', toolchainNote: null };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const next = () => { if (i + 1 >= argv.length) usage(`${a} needs a value`); return argv[++i]; };
    const kv = text => { const at = text.indexOf('='); if (at <= 0) usage(`${a} wants NAME=VALUE`); return [text.slice(0, at), text.slice(at + 1)]; };
    if (a === '--compiler') { const [label, path] = kv(next()); o.compiler = { label, path: resolve(path) }; }
    else if (a === '--level') { const [name, spec] = kv(next()); const [verb, ...flags] = spec.split(','); if (!['build', 'run'].includes(verb)) usage(`level verb must be build or run: ${spec}`); o.levels.push({ name, verb, flags }); }
    else if (a === '--unavailable') { const [name, reason] = kv(next()); o.unavailable.push({ name, reason }); }
    else if (a === '--out') o.out = resolve(next());
    else if (a === '--runs') o.runs = Number(next());
    else if (a === '--warmup') o.warmup = Number(next());
    else if (a === '--entries') o.entries = next().split(',').filter(Boolean);
    else if (a === '--scope-mem') o.scopeMem = next();
    else if (a === '--peers') o.peers = next().split(',').filter(Boolean);
    else if (a === '--repo') o.repo = resolve(next());
    else if (a === '--compile-timeout') o.compileTimeout = Number(next());
    else if (a === '--run-timeout') o.runTimeout = Number(next());
    else if (a === '--cli-adapter') { o.cliAdapter = next(); if (!['argv', 'literal', 'off'].includes(o.cliAdapter)) usage('--cli-adapter is argv, literal or off'); }
    else if (a === '--identity') o.identity = next();
    else if (a === '--cpus') o.cpus = next();
    else if (a === '--profile') o.profileLevels = next().split(',').filter(Boolean);
    else if (a === '--perf') o.perf = next();
    else if (a === '--toolchain-note') o.toolchainNote = next();
  }
  if (!o.compiler || !o.out || o.levels.length === 0) usage('--compiler, --level and --out are required');
  if (!(o.runs >= 1) || !(o.warmup >= 0)) usage('bad --runs/--warmup');
  for (const p of o.peers) if (!['rust', 'c'].includes(p)) usage(`same-run peers supported: rust, c (got ${p})`);
  o.repo ??= resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
  return o;
}

const sha256 = data => createHash('sha256').update(data).digest('hex');
const median = xs => { const s = [...xs].sort((a, b) => a - b); const m = s.length >> 1; return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2; };
const loadavg = () => readFileSync('/proc/loadavg', 'utf8').trim().split(' ').slice(0, 3).join(' ');
const memAvailableGb = () => { const m = /MemAvailable:\s+(\d+)/.exec(readFileSync('/proc/meminfo', 'utf8')); return m ? Number(m[1]) / 1048576 : null; };

// One measured process: wall from the monotonic clock around the spawn, CPU and peak RSS from GNU time.
// `cpus` pins the process: compiles through the scope's AllowedCPUs, runs through taskset.
function measure(command, { cwd, env = process.env, timeoutS, scopeMem = null, cpus = null }) {
  const timeFile = join(mkdtempSync(join(tmpdir(), 'olevel-time-')), 'time');
  const bounded = ['timeout', '--kill-after=10', String(timeoutS), ...command];
  const timed = [GNU_TIME, '-f', '%U %S %M %x', '-o', timeFile, ...(cpus ? ['taskset', '-c', cpus] : []), ...bounded];
  // The scope's RuntimeMaxSec backs up `timeout`: the Jet compiler ignores SIGTERM, and `--kill-after` only reaches it while it stays in timeout's process group.
  const full = scopeMem ? ['systemd-run', '--user', '--scope', '--quiet', '--collect', '-p', `MemoryMax=${scopeMem}`, '-p', 'MemorySwapMax=0', '-p', `RuntimeMaxSec=${timeoutS + 20}`, ...(cpus ? ['-p', `AllowedCPUs=${cpus}`] : []), ...timed] : timed;
  return new Promise(done => {
    const started = process.hrtime.bigint();
    const child = spawn(full[0], full.slice(1), { cwd, env, stdio: ['ignore', 'pipe', 'pipe'] });
    const out = [], err = [];
    let errBytes = 0;
    child.stdout.on('data', d => out.push(d));
    child.stderr.on('data', d => { if (errBytes < 1 << 20) { err.push(d); errBytes += d.length; } });
    let timedOut = false;
    const timer = setTimeout(() => { timedOut = true; child.kill('SIGKILL'); }, (timeoutS + 30) * 1000);
    child.on('close', (code, signal) => {
      clearTimeout(timer);
      const wall = Number(process.hrtime.bigint() - started) / 1e9;
      let user = null, sys = null, rssKb = null;
      try {
        const lines = readFileSync(timeFile, 'utf8').trim().split('\n');
        const fields = lines[lines.length - 1].trim().split(/\s+/).map(Number);
        [user, sys, rssKb] = fields;
        const signalLine = lines.find(l => l.startsWith('Command terminated by signal'));
        if (signalLine && !signal) signal = signalLine.replace('Command terminated by signal ', 'SIG');
      } catch { /* time output missing: the process was killed */ }
      rmSync(dirname(timeFile), { recursive: true, force: true });
      done({ code: timedOut || code === 124 ? 'timeout' : code, signal, wall, cpu: user === null ? null : user + sys, user, sys, rss_kb: rssKb, stdout: Buffer.concat(out), stderr: Buffer.concat(err).toString('utf8') });
    });
  });
}

// `#CLI struct Args {...}` + `fn run(args: Args) RET {` → plain `fn run() RET` that builds the same
// Args. Mode `argv`: from the positional arguments (field order, as the gauntlet passes them) through
// process.argv(). Mode `literal`: the entry's spec args written in as literals (no core.process
// import; the values become compile-time constants, which the table states).
function adaptCli(source, mode, args) {
  const runHead = /^fn run\(args: Args\)([^{\n]*)\{/m.exec(source);
  if (!runHead) return { source, adapted: null };
  const struct = /#CLI\s+struct Args\s*\{([^}]*)\}/.exec(source);
  if (!struct) throw new Error('fn run(args: Args) without a `#CLI struct Args`');
  const fields = struct[1].split(/[\n,]/).map(s => s.trim()).filter(Boolean).map(f => {
    const m = /^(\w+)\s*:\s*(\w+)$/.exec(f);
    if (!m || !['Int', 'String'].includes(m[2])) throw new Error(`unsupported Args field \`${f}\``);
    return { name: m[1], type: m[2] };
  });
  const argv = mode === 'argv' && fields.length > 0;
  const values = fields.map((f, i) => {
    if (mode === 'literal') {
      const value = args[i];
      if (value === undefined) throw new Error(`no spec argument ${i + 1} for field ${f.name}`);
      if (f.type === 'Int') { if (!/^-?\d+$/.test(value)) throw new Error(`spec argument ${value} is not an Int`); return `${f.name}: ${value}`; }
      return `${f.name}: "${value.replace(/\\/g, '\\\\').replace(/"/g, '\\"').replace(/\{/g, '{{').replace(/\}/g, '}}')}"`;
    }
    const word = `(olevel_argv.get(${i + 1}) ?? panic("olevel: missing argument ${i + 1}"))`;
    return `${f.name}: ${f.type === 'Int' ? `Int.parse(${word}) ?? panic("olevel: argument ${i + 1} is not an Int")` : word}`;
  });
  const ret = runHead[1];
  let out = source.replace(struct[0], struct[0].replace(/^#CLI\s+/, ''));
  out = out.replace(runHead[0], `fn olevel_body(args: Args)${ret}{`);
  out = `${argv ? 'use core.process as olevel_process\n' : ''}${out.endsWith('\n') ? out : `${out}\n`}\n` +
    `fn run()${ret}{\n${argv ? '    olevel_argv :: olevel_process.argv()\n' : ''}    olevel_body(Args{${values.join(', ')}})\n}\n`;
  return { source: out, adapted: mode };
}

function loadEntries(o) {
  const root = join(o.repo, 'Tools/gauntlet/entries');
  const programs = [], skipped = [];
  for (const name of o.entries ?? readdirSync(root).sort()) {
    if (!existsSync(join(root, name, 'entry.json'))) { skipped.push({ entry: name, reason: 'no such gauntlet entry' }); continue; }
    const dir = join(root, name);
    const entry = JSON.parse(readFileSync(join(dir, 'entry.json'), 'utf8'));
    const expectedPath = join(dir, entry.spec?.expected ?? 'expected.out');
    if (entry.mode !== 'batch') { skipped.push({ entry: name, reason: `mode ${entry.mode} (not a batch program)` }); continue; }
    if (!existsSync(expectedPath)) { skipped.push({ entry: name, reason: 'no expected output' }); continue; }
    if (name === 'http-client') { skipped.push({ entry: name, reason: 'needs the gauntlet fixture HTTP server' }); continue; }
    for (const variant of (entry.languages ?? []).filter(l => l === 'jet' || l === 'jet-expert')) {
      if (!existsSync(join(dir, variant, 'run.jet'))) { skipped.push({ entry: name, variant, reason: `missing ${variant}/run.jet` }); continue; }
      programs.push({ entry: name, variant, dir, spec: entry.spec ?? {}, cells: entry.cells ?? [], expected: readFileSync(expectedPath), peers: (entry.languages ?? []).filter(l => !l.startsWith('jet')) });
    }
  }
  return { programs, skipped };
}

// Newest gauntlet receipt row of every matched peer (status ok, measured runtime).
function peerReceipts(o) {
  const dir = join(o.repo, 'Tools/gauntlet/results');
  const byEntry = new Map();
  for (const file of readdirSync(dir).filter(f => f.endsWith('.json')).sort()) {
    let report;
    try { report = JSON.parse(readFileSync(join(dir, file), 'utf8')); } catch { continue; }
    for (const e of report.entries ?? []) {
      const name = typeof e.entry === 'object' ? e.entry?.name : e.entry;
      if (!name) continue;
      // Reports keep rows as a list (newer) or keyed by language (older).
      const rows = Array.isArray(e.rows) ? e.rows : Object.entries(e.rows ?? {}).map(([language, row]) => ({ language, ...row }));
      for (const row of rows) {
        if (row.status !== 'ok' || row.metrics?.runtime_wall_seconds == null || String(row.language).startsWith('jet')) continue;
        const peers = byEntry.get(name) ?? new Map();
        peers.set(row.language, { language: row.language, runtime_wall_seconds: row.metrics.runtime_wall_seconds, runtime_peak_rss_kb: row.metrics.runtime_peak_rss_kb ?? null, receipt: `Tools/gauntlet/results/${file}`, generated: report.generated ?? null });
        byEntry.set(name, peers);
      }
    }
  }
  return byEntry;
}

function stageProgram(prog, into, entryStage, o) {
  rmSync(into, { recursive: true, force: true });
  cpSync(join(prog.dir, prog.variant), into, { recursive: true });
  for (const junk of ['.jet', 'build']) rmSync(join(into, junk), { recursive: true, force: true });
  if (existsSync(join(into, 'fixture-state'))) { cpSync(join(into, 'fixture-state'), join(into, '.jet'), { recursive: true }); rmSync(join(into, 'fixture-state'), { recursive: true, force: true }); }
  for (const item of readdirSync(prog.dir, { withFileTypes: true })) {
    if (!item.isFile() || item.name === 'entry.json' || item.name === basename(prog.spec.expected ?? 'expected.out')) continue;
    copyFileSync(join(prog.dir, item.name), join(into, item.name));
  }
  if (prog.spec.fixtureGen) cpSync(join(entryStage, 'fixtures'), join(into, 'fixtures'), { recursive: true });
  const original = readFileSync(join(into, 'run.jet'), 'utf8');
  const adapted = o.cliAdapter === 'off' ? { source: original, adapted: null } : adaptCli(original, o.cliAdapter, prog.spec.args ?? []);
  if (adapted.adapted) writeFileSync(join(into, 'run.jet'), adapted.source);
  return { source_sha256: sha256(original), measured_source_sha256: sha256(adapted.source), cli_adapted: adapted.adapted };
}

function fixtureReset(prog, progDir, entryStage) {
  if (!prog.spec.fixtureGen) return () => {};
  const out = prog.spec.fixtureGen.out.replace(/\/$/, '');
  return () => {
    rmSync(join(progDir, out), { recursive: true, force: true });
    cpSync(join(entryStage, out), join(progDir, out), { recursive: true });
  };
}

// Compiler phase trace (JET_TRACE_FILE): seconds per top-level phase span (names without a `/`),
// the timestamp of the last event (the compile's own end; `jet run` execs the binary after it), and the
// last event's name (where a failed or killed compile stopped).
function traceSummary(path) {
  if (!existsSync(path)) return { trace_phases: null };
  const phases = {}, open = new Map();
  let last = null;
  for (const line of readFileSync(path, 'utf8').split('\n')) {
    const text = line.trim().replace(/,$/, '');
    if (!text.startsWith('{')) continue;
    let e;
    try { e = JSON.parse(text); } catch { continue; }
    last = e;
    if ((e.cat && e.cat !== e.name) || String(e.name).includes('/')) continue;
    if (e.ph === 'B') open.set(e.name, e.ts);
    else if (e.ph === 'E' && open.has(e.name)) { phases[e.name] = (phases[e.name] ?? 0) + (e.ts - open.get(e.name)) / 1e6; open.delete(e.name); }
  }
  return { trace_phases: phases, trace_end_s: last ? last.ts / 1e6 : null, trace_last_event: last ? `${last.name}${last.cat ? ` (${last.cat})` : ''}` : null, trace_open_phases: [...open.keys()] };
}

// User-space CPU profile of one run (perf record, cpu-clock:u at 999 Hz, frame-pointer call graphs)
// and its five hottest symbols by self time.
async function profileBinary(command, cwd, reset, dataPath, o) {
  reset();
  rmSync(dataPath, { force: true });
  const recorded = await measure([o.perf, 'record', '-q', '-e', 'cpu-clock:u', '-F', '999', '-g', '-o', dataPath, '--', ...command], { cwd, timeoutS: o.runTimeout, cpus: o.cpus });
  if (recorded.code !== 0) return { status: 'record-failed', code: recorded.code, stderr: recorded.stderr.slice(-1000) };
  const report = await measure([o.perf, 'report', '-i', dataPath, '--stdio', '--no-children', '--sort', 'symbol', '-g', 'none', '-q'], { cwd, timeoutS: 300 });
  if (report.code !== 0) return { status: 'report-failed', code: report.code, stderr: report.stderr.slice(-1000), data: dataPath };
  const top = [];
  for (const line of report.stdout.toString('utf8').split('\n')) {
    const m = /^\s*([\d.]+)%\s+\[(.)\]\s+(.+?)\s*$/.exec(line);
    if (m && top.length < 5) top.push({ pct: Number(m[1]), kind: m[2], symbol: m[3] });
  }
  return { status: 'ok', data: dataPath, wall: recorded.wall, top };
}


function summarize(samples) {
  const ok = samples.filter(s => s.code === 0);
  if (ok.length !== samples.length || ok.length === 0) return null;
  const walls = ok.map(s => s.wall);
  return { median_wall: median(walls), min_wall: Math.min(...walls), max_wall: Math.max(...walls), spread_pct: 100 * (Math.max(...walls) - Math.min(...walls)) / median(walls), median_cpu: median(ok.map(s => s.cpu)), median_rss_kb: median(ok.map(s => s.rss_kb)) };
}

async function runSeries(command, cwd, reset, o, expected) {
  const samples = [];
  for (let i = 0; i < o.warmup + o.runs; i++) {
    reset();
    const r = await measure(command, { cwd, timeoutS: o.runTimeout, cpus: o.cpus });
    const sample = { warmup: i < o.warmup, code: r.code, signal: r.signal, wall: r.wall, cpu: r.cpu, rss_kb: r.rss_kb, stdout_sha256: sha256(r.stdout), expected_equal: r.stdout.equals(expected), stderr: r.code === 0 ? undefined : r.stderr.slice(-2000) };
    samples.push(sample);
    if (r.code !== 0) break;
  }
  return samples;
}

async function main() {
  const o = parseArgs(process.argv.slice(2));
  if (existsSync(join(o.out, 'results.json'))) usage(`refusing to overwrite ${o.out}/results.json`);
  mkdirSync(o.out, { recursive: true });
  const work = join(o.out, 'work'), bins = join(o.out, 'bin'), traces = join(o.out, 'traces');
  mkdirSync(work, { recursive: true }); mkdirSync(bins, { recursive: true }); mkdirSync(traces, { recursive: true });
  const { programs, skipped } = loadEntries(o);
  const receipts = peerReceipts(o);
  const results = {
    schema: 'jet.olevel-sweep.v1', started: new Date().toISOString(), compiler: { ...o.compiler, wrapper_sha256: sha256(readFileSync(o.compiler.path)), identity: o.identity },
    levels: o.levels, unavailable: o.unavailable, runs: o.runs, warmup: o.warmup, scope_mem: o.scopeMem, pinned_cpus: o.cpus, cli_adapter: o.cliAdapter, profile_levels: o.profileLevels, toolchain_note: o.toolchainNote,
    machine: { cpus: cpus().length, model: cpus()[0]?.model, load_start: loadavg(), mem_available_gb_start: memAvailableGb() },
    repo: o.repo, skipped, programs: [],
  };
  const save = () => writeFileSync(join(o.out, 'results.json'), `${JSON.stringify(results, null, 1)}\n`);
  const log = text => process.stdout.write(`[${new Date().toISOString().slice(11, 19)}] ${text}\n`);

  const flags = results.flags = [];
  const finish = () => {
    results.finished = new Date().toISOString();
    results.machine.load_end = loadavg();
    save();
    writeTables(results, o);
  };

  // One program at a time: stage, compile every level, same-run peers (once per entry), run every
  // binary, verdicts; results.json and the tables are rewritten after each program.
  const entryStages = new Map();
  for (const prog of programs) {
    const entryStage = join(work, prog.entry);
    if (!entryStages.has(prog.entry)) {
      mkdirSync(entryStage, { recursive: true });
      const fx = prog.spec.fixtureGen;
      let fixtureError = null;
      if (fx) {
        cpSync(join(prog.dir, dirname(fx.script)), join(entryStage, 'fixtures'), { recursive: true });
        const g = await measure(['python3', fx.script, fx.out], { cwd: entryStage, timeoutS: 600 });
        if (g.code !== 0) fixtureError = `fixture generator exit ${g.code}: ${g.stderr.slice(-500)}`;
      }
      entryStages.set(prog.entry, { entryStage, fixtureError, peers: {}, peersDone: false });
    }
    const st = entryStages.get(prog.entry);
    const record = { entry: prog.entry, variant: prog.variant, cells: prog.cells, args: prog.spec.args ?? [], expected_sha256: sha256(prog.expected), fixture_error: st.fixtureError, levels: {}, peers_same_run: st.peers, peer_receipts: Object.fromEntries(receipts.get(prog.entry) ?? []) };
    results.programs.push(record);
    if (st.fixtureError) { flags.push({ program: `${prog.entry}/${prog.variant}`, level: 'all', kind: 'fixture-failed', detail: st.fixtureError }); finish(); continue; }

    // Compiles, each level in its own copy of the program, one at a time.
    let frontendStall = null;
    for (const level of o.levels) {
      if (frontendStall) { record.levels[level.name] = { status: 'not-attempted', error: frontendStall }; continue; }
      const progDir = join(work, prog.entry, `${prog.variant}-${level.name}`);
      let staged;
      try { staged = stageProgram(prog, progDir, entryStage, o); }
      catch (error) { record.levels[level.name] = { status: 'stage-failed', error: String(error.message ?? error) }; continue; }
      fixtureReset(prog, progDir, entryStage)();
      const command = level.verb === 'build' ? [o.compiler.path, 'build', ...level.flags, 'run.jet'] : [o.compiler.path, 'run', ...level.flags, 'run.jet', '--', ...(prog.spec.args ?? [])];
      log(`compile ${prog.entry}/${prog.variant} ${level.name}: ${command.slice(1).join(' ')}`);
      const tracePath = join(traces, `${prog.entry}-${prog.variant}-${level.name}.trace.jsonl`);
      rmSync(tracePath, { force: true });
      const c = await measure(command, { cwd: progDir, env: { ...process.env, JET_TRACE_FILE: tracePath }, timeoutS: o.compileTimeout, scopeMem: o.scopeMem, cpus: o.cpus });
      const binary = join(progDir, '.jet/build/run');
      const built = c.code === 0 && existsSync(binary);
      // systemd-run receives SIGTERM when its scope fails (the kernel OOM-killed a process at MemoryMax).
      const scopeKilled = !built && c.signal === 'SIGTERM' && c.code !== 'timeout';
      const rec = { ...staged, status: built ? 'built' : scopeKilled ? `compile-killed (scope MemoryMax=${o.scopeMem} OOM)` : 'compile-failed', command: command.slice(1), dir: progDir,
        compile: { code: c.code, signal: c.signal, wall: c.wall, cpu: c.cpu, user: c.user, sys: c.sys, rss_kb: c.rss_kb, includes_one_run: level.verb === 'run', load: loadavg(), trace: tracePath, ...traceSummary(tracePath) } };
      if (level.verb === 'run') { rec.compile.run_stdout_sha256 = sha256(c.stdout); rec.compile.run_expected_equal = c.stdout.equals(prog.expected); }
      if (!built) rec.compile.stderr = c.stderr.slice(-4000);
      else {
        const kept = join(bins, `${prog.entry}-${prog.variant}-${level.name}`);
        copyFileSync(binary, kept);
        rec.binary = kept; rec.binary_bytes = statSync(kept).size; rec.binary_sha256 = sha256(readFileSync(kept));
      }
      log(`  ${rec.status} wall ${c.wall.toFixed(1)}s cpu ${c.cpu?.toFixed(1)}s rss ${((c.rss_kb ?? 0) / 1024).toFixed(0)}MB`);
      record.levels[level.name] = rec;
      // The front end (load … sema) is the same at every level: a compile killed before lowering
      // started would stall the same way at the next level, so that level is not attempted.
      const reached = [...Object.keys(rec.compile.trace_phases ?? {}), ...(rec.compile.trace_open_phases ?? [])];
      if (c.code === 'timeout' && !reached.some(p => p === 'lower' || p.startsWith('native'))) frontendStall = `the level-independent front end did not finish at ${level.name} within ${o.compileTimeout}s (last trace event ${rec.compile.trace_last_event ?? '–'})`;
      save();
    }

    // Same-run peers (gauntlet build commands), once per entry.
    if (!st.peersDone) {
      st.peersDone = true;
      for (const lang of o.peers.filter(p => prog.peers.includes(p))) {
        const dir = join(entryStage, `peer-${lang}`);
        rmSync(dir, { recursive: true, force: true });
        cpSync(join(prog.dir, lang), dir, { recursive: true });
        for (const item of readdirSync(prog.dir, { withFileTypes: true })) if (item.isFile() && item.name !== 'entry.json' && item.name !== basename(prog.spec.expected ?? 'expected.out')) copyFileSync(join(prog.dir, item.name), join(dir, item.name));
        if (prog.spec.fixtureGen) cpSync(join(entryStage, 'fixtures'), join(dir, 'fixtures'), { recursive: true });
        const build = lang === 'rust' ? ['rustc', '--edition=2021', '-O', 'main.rs', '-o', 'main-rust'] : ['gcc', '-O2', 'main.c', '-o', 'main-c', '-lm'];
        const b = await measure(build, { cwd: dir, timeoutS: 900, cpus: o.cpus });
        const peer = { build: build.join(' '), build_code: b.code, build_wall: b.wall };
        if (b.code !== 0) peer.build_stderr = b.stderr.slice(-2000);
        else {
          log(`peer ${prog.entry} ${lang}: run x${o.runs}`);
          peer.samples = await runSeries([join(dir, lang === 'rust' ? 'main-rust' : 'main-c'), ...(prog.spec.args ?? [])], dir, fixtureReset(prog, dir, entryStage), o, prog.expected);
          peer.summary = summarize(peer.samples.filter(s => !s.warmup));
          peer.expected_equal = peer.samples.every(s => s.expected_equal);
        }
        st.peers[lang] = peer;
        save();
      }
    }

    // Runtime series, one binary at a time.
    for (const level of o.levels) {
      const rec = record.levels[level.name];
      if (!rec || rec.status !== 'built') continue;
      log(`run ${prog.entry}/${prog.variant} ${level.name} x${o.runs}`);
      rec.samples = await runSeries([rec.binary, ...(prog.spec.args ?? [])], rec.dir, fixtureReset(prog, rec.dir, entryStage), o, prog.expected);
      rec.summary = summarize(rec.samples.filter(s => !s.warmup));
      rec.run_status = rec.summary ? 'ok' : 'run-failed';
      if (rec.compile.includes_one_run && rec.summary) rec.compile.wall_net_of_run = rec.compile.wall - rec.summary.median_wall;
      if (rec.summary && o.profileLevels.includes(level.name)) {
        log(`profile ${prog.entry}/${prog.variant} ${level.name}`);
        rec.profile = await profileBinary([rec.binary, ...(prog.spec.args ?? [])], rec.dir, fixtureReset(prog, rec.dir, entryStage), join(traces, `${prog.entry}-${prog.variant}-${level.name}.perf.data`), o);
      }
    }

    // Verdicts.
    const name = `${record.entry}/${record.variant}`;
    const levelRecs = o.levels.map(l => [l.name, record.levels[l.name]]).filter(([, r]) => r);
    const digests = new Set();
    for (const [level, r] of levelRecs) {
      if (r.status !== 'built') { flags.push({ program: name, level, kind: r.status, detail: r.compile ? `exit ${r.compile.code}${r.compile.signal ? ` ${r.compile.signal}` : ''} after ${r.compile.wall.toFixed(0)}s, peak ${((r.compile.rss_kb ?? 0) / 1048576).toFixed(1)} GB, last trace event ${r.compile.trace_last_event ?? '–'}: ${(r.compile.stderr ?? '').trim().split('\n').slice(-3).join(' | ')}` : r.error }); continue; }
      if (r.run_status !== 'ok') { const bad = r.samples?.find(s => s.code !== 0); flags.push({ program: name, level, kind: 'run-failed', detail: `exit ${bad?.code} ${bad?.signal ?? ''} ${(bad?.stderr ?? '').trim().split('\n').slice(-2).join(' | ')}` }); continue; }
      for (const s of r.samples) digests.add(s.stdout_sha256);
      r.expected_equal = r.samples.every(s => s.expected_equal);
      if (!r.expected_equal) flags.push({ program: name, level, kind: 'I9-output-differs-from-expected', detail: `stdout ${[...new Set(r.samples.map(s => s.stdout_sha256.slice(0, 12)))].join(',')} vs expected ${record.expected_sha256.slice(0, 12)}` });
      if (r.compile.includes_one_run && !r.compile.run_expected_equal) flags.push({ program: name, level, kind: 'I9-output-differs-from-expected', detail: 'the run inside `jet run`' });
      const rust = record.peers_same_run.rust?.summary;
      if (rust) { r.rust_same_run_ratio = r.summary.median_wall / rust.median_wall; r.rust_verdict = r.rust_same_run_ratio < 1 ? 'win' : r.rust_same_run_ratio <= RUST_BAND ? 'parity' : 'loss'; }
      r.receipt_ratios = Object.fromEntries(Object.values(record.peer_receipts).map(p => [p.language, r.summary.median_wall / p.runtime_wall_seconds]));
    }
    record.outputs_equal_across_levels = digests.size <= 1 && levelRecs.length > 0 && levelRecs.every(([, r]) => r.run_status === 'ok');
    if (digests.size > 1) flags.push({ program: name, level: 'all', kind: 'I9-output-differs-across-levels', detail: [...digests].map(d => d.slice(0, 12)).join(' ') });
    const lv = l => record.levels[l]?.summary?.median_wall;
    const spread = l => `${record.levels[l].summary.spread_pct.toFixed(1)}%`;
    if (lv('O1') != null && lv('O2') != null && lv('O2') > lv('O1')) flags.push({ program: name, level: 'O2', kind: 'O2-slower-than-O1', detail: `O2 ${lv('O2').toFixed(4)}s > O1 ${lv('O1').toFixed(4)}s (spreads ${spread('O2')} / ${spread('O1')})` });
    if (lv('O0') != null && lv('O1') != null && lv('O1') > lv('O0')) flags.push({ program: name, level: 'O1', kind: 'O1-slower-than-O0', detail: `O1 ${lv('O1').toFixed(4)}s > O0 ${lv('O0').toFixed(4)}s (spreads ${spread('O1')} / ${spread('O0')})` });
    if (lv('O0') != null && lv('O2') != null && !record.levels.O1 && lv('O2') > lv('O0')) flags.push({ program: name, level: 'O2', kind: 'O2-slower-than-O0', detail: `O2 ${lv('O2').toFixed(4)}s > O0 ${lv('O0').toFixed(4)}s (spreads ${spread('O2')} / ${spread('O0')})` });
    finish();
  }
  results.complete = true;
  finish();
  log(`done: ${results.programs.length} programs, ${flags.length} flags → ${o.out}/table.md`);
  process.exitCode = flags.some(f => f.kind.startsWith('I9') || f.kind.endsWith('failed')) ? 1 : 0;
}

function writeTables(results, o) {
  const f = (x, d = 3) => x == null ? '–' : x.toFixed(d);
  const mb = kb => kb == null ? '–' : (kb / 1024).toFixed(0);
  const header = ['program', 'level', 'compile wall s', 'compiler traced s', 'compile CPU s', 'compile RSS MB', 'run median s', 'run min–max s', 'spread %', 'run CPU s', 'run RSS MB', 'out=expected', 'out equal across levels', 'Jet/Rust same-run', 'Jet/peer receipts'];
  const md = [`# O-level sweep: ${results.compiler.label}`, '',
    `Compiler: \`${results.compiler.path}\`${results.compiler.identity ? ` (${results.compiler.identity})` : ''}. Levels: ${results.levels.map(l => `${l.name} = \`jet ${l.verb}${l.flags.length ? ` ${l.flags.join(' ')}` : ''}\``).join(', ')}.` +
    `${results.unavailable.length ? ` Unavailable: ${results.unavailable.map(u => `${u.name} (${u.reason})`).join('; ')}.` : ''}`,
    '`compile wall s` is the whole measured command (scope, wrapper and toolchain environment startup included); `compiler traced s` is the timestamp of the compiler\'s last trace event (the compiler process alone; for `jet run`, the compile before the program starts).',
    `Runs: ${results.runs} measured after ${results.warmup} warmup; one compile at a time, each in a scope with MemoryMax=${results.scope_mem} and no swap${results.pinned_cpus ? `; compiles, runs and peers pinned to CPUs ${results.pinned_cpus}` : ''}. Machine: ${results.machine.cpus} CPUs, load ${results.machine.load_start} → ${results.machine.load_end}. Started ${results.started}, ${results.complete ? `finished ${results.finished}` : `PARTIAL as of ${results.finished}`}.`,
    `Rust ratio verdict: < 1 win, ≤ ${RUST_BAND} parity, > ${RUST_BAND} loss (same-run \`rustc -O\`). Receipt ratios use the newest gauntlet receipt (older run, other load): context, not gate evidence.${results.toolchain_note ? ` Toolchain: ${results.toolchain_note}.` : ''}`,
    results.cli_adapter === 'off' ? 'CLI adapter: off.' : `CLI adapter (${results.cli_adapter}): the native compiler rejects the typed \`#CLI struct Args\` + \`fn run(args: Args)\` entry (E1321), so such entries were measured as \`fn run()\` that ${results.cli_adapter === 'argv' ? 'reads the same positional arguments from `process.argv()`' : 'builds `Args` from the entry\'s spec arguments written as literals (no `core.process` import; the arguments become compile-time constants, unlike the peers)'}; the program body is unchanged (results.json keeps both source digests).`, '',
    `| ${header.join(' | ')} |`, `|${header.map(() => '---').join('|')}|`];
  const tsv = [header.join('\t')];
  for (const p of results.programs) {
    const names = [...results.levels.map(l => l.name), ...results.unavailable.map(u => u.name)].sort();
    for (const name of names) {
      const r = p.levels[name];
      const unavailable = results.unavailable.find(u => u.name === name);
      const s = r?.summary;
      const rust = r?.rust_same_run_ratio != null ? `${f(r.rust_same_run_ratio, 2)} ${r.rust_verdict}` : '–';
      const receipts = r?.receipt_ratios ? Object.entries(r.receipt_ratios).map(([k, v]) => `${k} ${f(v, 2)}`).join(', ') : '–';
      const status = unavailable ? 'unavailable' : !r ? 'not run' : r.status !== 'built' ? r.status : r.run_status !== 'ok' ? 'run-failed' : null;
      const compileWall = r?.compile ? (r.compile.includes_one_run && r.compile.wall_net_of_run != null ? `${f(r.compile.wall, 1)} (net ${f(r.compile.wall_net_of_run, 1)})` : f(r.compile.wall, 1)) : '–';
      const cells = [`${p.entry}${p.variant === 'jet' ? '' : `/${p.variant}`}`, name,
        compileWall, f(r?.compile?.trace_end_s, 1), f(r?.compile?.cpu, 1), mb(r?.compile?.rss_kb),
        status ?? f(s.median_wall, 4), s ? `${f(s.min_wall, 4)}–${f(s.max_wall, 4)}` : '–', s ? f(s.spread_pct, 1) : '–', s ? f(s.median_cpu, 3) : '–', s ? mb(s.median_rss_kb) : '–',
        r?.expected_equal == null ? '–' : r.expected_equal ? 'yes' : '**NO**', p.outputs_equal_across_levels ? 'yes' : '**NO**', rust, receipts];
      md.push(`| ${cells.join(' | ')} |`);
      tsv.push(cells.join('\t'));
    }
    const peerRows = Object.entries(p.peers_same_run).filter(([, v]) => v.summary);
    for (const [lang, v] of peerRows) {
      if (p.variant !== 'jet') continue;
      const cells = [p.entry, `${lang} (same run)`, f(v.build_wall, 1), '–', '–', '–', f(v.summary.median_wall, 4), `${f(v.summary.min_wall, 4)}–${f(v.summary.max_wall, 4)}`, f(v.summary.spread_pct, 1), f(v.summary.median_cpu, 3), mb(v.summary.median_rss_kb), v.expected_equal ? 'yes' : '**NO**', '', '', ''];
      md.push(`| ${cells.join(' | ')} |`);
      tsv.push(cells.join('\t'));
    }
  }
  const profiled = results.programs.flatMap(p => Object.entries(p.levels).filter(([, r]) => r.profile).map(([level, r]) => [p, level, r.profile]));
  if (profiled.length) {
    md.push('', '## Hot symbols (perf record cpu-clock:u, self time, one run)', '');
    for (const [p, level, prof] of profiled) md.push(`- ${p.entry}${p.variant === 'jet' ? '' : `/${p.variant}`} ${level}: ${prof.status === 'ok' ? prof.top.map(t => `${t.pct.toFixed(1)}% \`${t.symbol}\``).join(', ') : `${prof.status} (${(prof.stderr ?? '').trim().split('\n').slice(-1)[0]})`}`);
  }
  md.push('', '## Flags', '');
  if (results.flags.length === 0) md.push('None.');
  for (const fl of results.flags) md.push(`- **${fl.kind}** ${fl.program} ${fl.level}: ${fl.detail}`);
  md.push('', '## Skipped entries', '');
  for (const s of results.skipped) md.push(`- ${s.entry}${s.variant ? `/${s.variant}` : ''}: ${s.reason}`);
  writeFileSync(join(o.out, 'table.md'), `${md.join('\n')}\n`);
  writeFileSync(join(o.out, 'table.tsv'), `${tsv.join('\n')}\n`);
}

if (process.argv.includes('--tables-only')) {
  // Re-render the tables of a finished results.json: sweep.mjs --tables-only DIR
  const dir = resolve(process.argv[process.argv.indexOf('--tables-only') + 1]);
  writeTables(JSON.parse(readFileSync(join(dir, 'results.json'), 'utf8')), { out: dir });
} else {
  await main();
}
