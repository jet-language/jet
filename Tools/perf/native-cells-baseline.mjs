#!/usr/bin/env node
// W7: actual executable wall times only; preserve every cell, including refusals.
import { createHash } from 'node:crypto';
import { appendFileSync, copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { basename, join } from 'node:path';
import { spawnSync } from 'node:child_process';

const [mode, root, base, o1, output] = process.argv.slice(2);
if (!['release', 'measure', 'check-release'].includes(mode) || !output) throw new Error('usage: baseline.mjs release|measure|check-release ROOT BASE O1 OUTPUT');
const features = join(base, 'Tools/perf/native-cells/Examples/features');
const cells = [];
for (const group of readdirSync(features, { withFileTypes: true }).filter(d => d.isDirectory() && d.name !== 'expected')) {
  for (const file of readdirSync(join(features, group.name)).filter(f => f.endsWith('.jet'))) {
    const stem = `${group.name}/${basename(file, '.jet')}`;
    const source = join(features, `${stem}.jet`);
    const other = join(o1, 'Tools/perf/native-cells/Examples/features', `${stem}.jet`);
    if (!readFileSync(source).equals(readFileSync(other))) throw new Error(`cell source differs between arms: ${stem}`);
    const expected = readFileSync(join(features, 'expected', `${stem}.out`));
    if (!expected.equals(readFileSync(join(o1, 'Tools/perf/native-cells/Examples/features/expected', `${stem}.out`)))) throw new Error(`expected output differs: ${stem}`);
    cells.push({ stem, slug: stem.replace(/[^A-Za-z0-9_-]+/gu, '_'), source, expected });
  }
}
cells.sort((a, b) => a.stem.localeCompare(b.stem));
if (cells.length !== 15) throw new Error(`expected 15 cells, found ${cells.length}`);
mkdirSync(root, { recursive: true });
const sha = file => createHash('sha256').update(readFileSync(file)).digest('hex');
const flatten = s => String(s).replace(/[\t\r\n]+/g, ' ').trim();
if (mode === 'release') {
  const rows = [];
  let bad = 0;
  for (const cell of cells) {
    const dir = join(root, 'release', cell.slug);
    mkdirSync(dir, { recursive: true });
    const source = join(dir, `${cell.slug}.jet`);
    copyFileSync(cell.source, source);
    const build = spawnSync(join(base, 'Tools/agent/jet-env'), [process.env.JET, 'build', '--release', source], {
      cwd: dir, env: process.env, encoding: 'utf8', timeout: 600000, maxBuffer: 64 << 20,
    });
    writeFileSync(join(dir, 'build.log'), `exit=${build.status} signal=${build.signal}\n${build.error ?? ''}\n${build.stdout ?? ''}\n${build.stderr ?? ''}`);
    const exe = join(dir, '.jet/build', cell.slug);
    const verdict = build.status === 0 && existsSync(exe) ? 'built' : 'build-failed';
    if (verdict !== 'built') bad = 1;
    rows.push(`${cell.stem}\t${verdict}\t${flatten(build.error ?? (build.stderr ?? '').split('\n').find(s => /error/i.test(s)) ?? `exit ${build.status}`)}`);
    console.log(`${cell.stem}: ${verdict}`);
  }
  writeFileSync(join(root, 'release-results.tsv'), `${rows.join('\n')}\n`);
  process.exitCode = bad;
} else {
  const nativeDirs = { O0: process.env.W7_O0_DIR ?? join(root, 'O0'), O1: process.env.W7_O1_DIR ?? join(root, 'O1') };
  const native = arm => {
    const file = join(nativeDirs[arm], 'results.tsv');
    if (!existsSync(file)) return new Map();
    return new Map(readFileSync(file, 'utf8').trim().split('\n').map(line => {
      const [stem, verdict, ...detail] = line.split('\t');
      return [stem, { verdict, detail: detail.join(' ') }];
    }));
  };
  const statuses = { O0: native('O0'), O1: native('O1') };
  const releases = new Map(readFileSync(join(root, 'release-results.tsv'), 'utf8').trim().split('\n').map(line => {
    const [stem, verdict, ...detail] = line.split('\t');
    return [stem, { verdict, detail: detail.join(' ') }];
  }));
  const samplesFile = `${output}.samples.jsonl`;
  if (existsSync(samplesFile) || existsSync(output)) throw new Error('refusing to overwrite measurement evidence');
  const rows = ['cell\tO0_ms\tO1_ms\trelease_ms\tO0/release\tO1/release\tO0_verdict\tO1_verdict\trelease_verdict\tO0_min_ms\tO0_max_ms\tO1_min_ms\tO1_max_ms\trelease_min_ms\trelease_max_ms\tissues'];
  let bad = 0;
  for (const cell of cells) {
    const arms = {};
    const issues = [];
    for (const arm of ['O0', 'O1', 'release']) {
      if (mode === 'check-release' && arm !== 'release') {
        arms[arm] = { verdict: 'not-tested', times: [] };
        continue;
      }
      const status = (arm === 'release' ? releases : statuses[arm]).get(cell.stem);
      const binary = arm === 'release' ? join(root, 'release', cell.slug, '.jet/build', cell.slug) : join(nativeDirs[arm], 'lower', `${cell.slug}.exe`);
      arms[arm] = { verdict: status?.verdict ?? 'missing-result', times: [] };
      const expectedVerdict = arm === 'release' ? 'built' : 'pass';
      if (status?.verdict !== expectedVerdict || !existsSync(binary)) {
        if (status?.verdict === expectedVerdict) arms[arm].verdict = 'missing-executable';
        let detail = status?.detail ?? 'no harness result';
        for (const suffix of ['.issues', '.pack-issues']) {
          const file = join(nativeDirs[arm] ?? join(root, arm), 'lower', `${cell.slug}${suffix}`);
          if (existsSync(file)) { detail = readFileSync(file, 'utf8').split('\n').find(Boolean) ?? detail; break; }
        }
        issues.push(`${arm}: ${detail}`);
        // Refusals are covered rows, not incorrect output. An O1 coverage loss
        // relative to O0, missing artifacts, driver crashes, or wrong runs fail.
        const coveredRefusal = arm !== 'release' && status?.verdict === 'unsupported';
        if (!coveredRefusal || (arm === 'O1' && statuses.O0.get(cell.stem)?.verdict === 'pass')) bad = 1;
        continue;
      }
      arms[arm].verdict = 'pass';
      arms[arm].binary = binary;
      arms[arm].sha256 = sha(binary);
    }
    // Alternate arms within each round to share the same measurement interval.
    for (let round = 1; round <= 7; round++) {
      for (const arm of ['O0', 'O1', 'release']) {
        const entry = arms[arm];
        if (!entry.binary) continue;
        const start = process.hrtime.bigint();
        const run = spawnSync('taskset', ['-c', '31', entry.binary], { encoding: null, timeout: 300000, maxBuffer: 16 << 20 });
        const ns = process.hrtime.bigint() - start;
        const ok = run.status === 0 && run.stdout?.equals(cell.expected) && run.stderr?.length === 0;
        appendFileSync(samplesFile, JSON.stringify({ cell: cell.stem, arm, round, cpu: 31, binary: entry.binary, sha256: entry.sha256, wall_ns: ns.toString(), exit: run.status, signal: run.signal, error: run.error?.message, verdict: ok ? 'pass' : 'wrong', stdout: run.stdout?.toString(), stderr: run.stderr?.toString() }) + '\n');
        if (ok) entry.times.push(Number(ns) / 1e6);
        else { entry.verdict = 'wrong'; bad = 1; issues.push(`${arm} round ${round}: ${run.error?.message ?? `exit ${run.status}; stdout/stderr check ${ok}`}`); }
      }
    }
    for (const entry of Object.values(arms)) {
      entry.times.sort((a, b) => a - b);
      // A partial set or any wrong result is not a measured arm.
      if (entry.verdict === 'pass' && entry.times.length === 7) {
        entry.median = entry.times[3]; entry.min = entry.times[0]; entry.max = entry.times[6];
      }
    }
    const number = n => n === undefined ? 'NA' : n.toFixed(6);
    const ratio = arm => arms[arm].median === undefined || arms.release.median === undefined ? 'NA' : number(arms[arm].median / arms.release.median);
    rows.push([cell.stem, ...['O0', 'O1', 'release'].map(a => number(arms[a].median)), ratio('O0'), ratio('O1'), ...['O0', 'O1', 'release'].map(a => arms[a].verdict), ...['O0', 'O1', 'release'].flatMap(a => [number(arms[a].min), number(arms[a].max)]), flatten(issues.join('; '))].join('\t'));
    console.log(rows.at(-1));
  }
  writeFileSync(output, `${rows.join('\n')}\n`);
  writeFileSync(join(root, 'inputs.sha256'), cells.flatMap(c => [`${sha(c.source)}  ${c.source}`, `${createHash('sha256').update(c.expected).digest('hex')}  ${c.stem}.out`]).join('\n') + '\n');
  process.exitCode = bad;
}
