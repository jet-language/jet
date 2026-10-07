#!/usr/bin/env node
// D-CALLSITE-MARK1=A cutover codemod: `f(^name)` -> `f(name)` where the
// argument is the name's last use and option A no longer writes `^` there.
//
// usage: node Tools/agent/codemod-implicit-take.mjs --jet <jet binary> [--write] (--all | <file|dir>...)
//   --all   tracked .jet files under Examples/, Jetpack/, Tools/ and
//           tests/fuzz/. tests/ui and tests/conformance pin explicit-mark
//           diagnostics and stay as written. Compiler/ is checked as one
//           assembled unit, so it is not a `jet check` unit here.
//   --write-marks-only preserves every `^` while removing read-parameter `&`
//           marks, for Jetpack's pre-candidate bootstrap cutover.
//
// The compiler decides, not the script. For each check unit (a package
// directory with package.jet, otherwise one file) the script removes every
// argument-position `^name`, checks the unit, and puts `^` back at each site
// where a new diagnostic lands on that argument: a value with cleanup, a type
// parameter, a lambda capture, a name another argument also names, and so on.
// It repeats until the unit's diagnostics equal the baseline; a unit that
// cannot reach its baseline is restored untouched and reported. Re-runnable:
// a migrated tree reports removed=0.
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync, spawnSync } from 'node:child_process';

const argv = process.argv.slice(2);
const opt = (name) => {
  const i = argv.indexOf(name);
  return i >= 0 ? argv[i + 1] : undefined;
};
const jet = opt('--jet');
const write = argv.includes('--write');
const all = argv.includes('--all');
const marksOnly = argv.includes('--write-marks-only');
if (!jet) {
  console.error('usage: codemod-implicit-take.mjs --jet <jet> [--write] (--all | <file|dir>...)');
  process.exit(64);
}
const valueOpts = new Set(['--jet']);
const positional = argv.filter((a, i) => !a.startsWith('--') && !valueOpts.has(argv[i - 1]));

const ROOTS = ['Examples/', 'Jetpack/', 'Tools/', 'tests/fuzz/'];
const files = all
  ? execFileSync('git', ['ls-files', '*.jet'], { encoding: 'utf8' })
      .split('\n')
      .filter((f) => f && ROOTS.some((root) => f.startsWith(root)))
  : positional.flatMap((p) =>
      fs.statSync(p).isDirectory()
        ? execFileSync('git', ['ls-files', '*.jet'], { cwd: p, encoding: 'utf8' })
            .split('\n')
            .filter(Boolean)
            .map((f) => path.join(p, f))
        : [p],
    );

// An argument-position move mark on a bare name: after `(`, `,` or a
// `label:`, followed by `,` or `)`. `^self` is skipped (receiver declarations).
const SITE = /([(,]\s*(?:[a-z_][A-Za-z0-9_]*\s*:\s*)?)\^([a-z_][A-Za-z0-9_]*)(?=\s*[,)])/g;

function sites(text) {
  const out = [];
  let lineStart = 0;
  for (const line of text.split('\n')) {
    const code = line.replace(/\/\/.*$/, '');
    if (!/^\s*fn\s/.test(code) && !/"/.test(code)) {
      for (const m of code.matchAll(SITE)) {
        if (m[2] === 'self') continue;
        const caret = lineStart + m.index + m[1].length;
        out.push({ caret, start: caret + 1, end: caret + 1 + m[2].length, name: m[2] });
      }
    }
    lineStart += line.length + 1;
  }
  return out;
}

function unitOf(file) {
  let dir = path.dirname(path.resolve(file));
  const top = path.resolve('.');
  while (dir.startsWith(top) && dir !== top) {
    if (fs.existsSync(path.join(dir, 'package.jet'))) return dir;
    dir = path.dirname(dir);
  }
  return path.resolve(file);
}

function check(unit, withEdits = false) {
  const run = spawnSync(
    'systemd-run',
    ['--user', '--slice=jetwork.slice', '--scope', '-q', '-p', 'MemoryMax=4G', '-p', 'MemorySwapMax=0',
      'timeout', '600', jet, 'check', '--json', '--color=never', unit],
    { encoding: 'utf8', maxBuffer: 1 << 28 },
  );
  const reports = [];
  for (const line of (run.stdout || '').split('\n')) {
    if (!line.startsWith('{')) continue;
    try {
      const doc = JSON.parse(line);
      for (const r of doc.reports || []) {
        if (r.severity !== 'error' || !r.file || !r.span) continue;
        const report = { code: r.code, what: r.what || '', file: path.resolve(r.file), start: r.span.start, end: r.span.end };
        if (withEdits) report.edits = r.fix_edits || [];
        reports.push(report);
      }
    } catch {}
  }
  return reports;
}

// D-CALLSITE-MARK1=A: E0225 `&` on an argument whose parameter only reads it.
// Applies the compiler's Safe removal until the unit has none left.
function removeUnbackedWriteMarks(unit) {
  let removed = 0;
  for (let round = 0; round < 8; round++) {
    const edits = check(unit, true)
      .filter((r) => r.code === 'E0225' && /only reads this argument/.test(r.what))
      .flatMap((r) => r.edits.map((e) => ({ file: path.resolve(e.file), start: e.span.start, end: e.span.end, text: e.new_text })));
    if (!edits.length) break;
    const byFile = new Map();
    for (const e of edits) {
      if (!byFile.has(e.file)) byFile.set(e.file, new Map());
      byFile.get(e.file).set(e.start, e);
    }
    for (const [file, spans] of byFile) {
      let text = fs.readFileSync(file, 'utf8');
      for (const e of [...spans.values()].sort((a, b) => b.start - a.start)) {
        const line = text.slice(0, e.start).split('\n').length;
        console.log(`${path.relative('.', file)}:${line}: remove unbacked \`${text.slice(e.start, e.end)}\``);
        text = text.slice(0, e.start) + e.text + text.slice(e.end);
        removed++;
      }
      fs.writeFileSync(file, text);
    }
  }
  return removed;
}

// Map an offset in the edited text back to the original text.
function toOriginal(removed, offset) {
  let shift = 0;
  for (const caret of removed) if (caret < offset + shift) shift++;
  return offset + shift;
}

function key(r) {
  return `${r.code}@${r.file}:${r.start}`;
}

const WRITE_ARG = /[(,]\s*(?:[a-z_][A-Za-z0-9_]*\s*:\s*)?&[a-z_]/;
const units = new Map();
for (const f of files) {
  const text = fs.readFileSync(f, 'utf8');
  if ((!marksOnly && sites(text).length) || WRITE_ARG.test(text)) {
    const u = unitOf(f);
    if (!units.has(u)) units.set(u, []);
    units.get(u).push({ file: path.resolve(f), original: text });
  }
}

let removedTotal = 0;
let keptTotal = 0;
let marksTotal = 0;
const failed = [];
for (const [unit, members] of units) {
  // The `&` pass edits any file of the unit, so it runs only with --write.
  if (write) marksTotal += removeUnbackedWriteMarks(unit);
  for (const m of members) {
    m.text = fs.readFileSync(m.file, 'utf8');
    m.sites = marksOnly ? [] : sites(m.text);
    m.active = m.sites.map(() => true);
  }
  if (!members.some((m) => m.sites.length)) {
    if (!write) for (const m of members) fs.writeFileSync(m.file, m.original);
    continue;
  }
  const baseline = new Set(check(unit).map(key));
  let ok = false;
  for (let round = 0; round < 8; round++) {
    const removedBy = new Map();
    for (const m of members) {
      const removed = m.sites.filter((_, i) => m.active[i]).map((s) => s.caret);
      removedBy.set(m.file, removed);
      let out = m.text;
      for (const caret of [...removed].sort((a, b) => b - a)) out = out.slice(0, caret) + out.slice(caret + 1);
      fs.writeFileSync(m.file, out);
    }
    const fresh = check(unit)
      .map((r) => {
        const removed = removedBy.get(r.file) || [];
        return { ...r, start: toOriginal(removed, r.start), end: toOriginal(removed, Math.max(r.start, r.end - 1)) + 1 };
      })
      .filter((r) => !baseline.has(key(r)));
    if (!fresh.length) {
      ok = true;
      break;
    }
    let restored = 0;
    for (const r of fresh) {
      const m = members.find((x) => x.file === r.file);
      if (!m) continue;
      m.sites.forEach((s, i) => {
        if (m.active[i] && r.start <= s.end && r.end >= s.caret) {
          m.active[i] = false;
          restored++;
        }
      });
    }
    if (!restored) break;
  }
  for (const m of members) {
    const removedHere = ok ? m.active.filter(Boolean).length : 0;
    if (!write) fs.writeFileSync(m.file, m.original);
    else if (!ok) fs.writeFileSync(m.file, m.text);
    removedTotal += removedHere;
    keptTotal += m.sites.length - removedHere;
    const rel = path.relative('.', m.file);
    m.sites.forEach((s, i) => {
      if (ok && m.active[i]) {
        const line = m.text.slice(0, s.caret).split('\n').length;
        console.log(`${rel}:${line}: ^${s.name} -> ${s.name}`);
      }
    });
  }
  if (!ok) failed.push(path.relative('.', unit));
}
console.log(`removed=${removedTotal} kept=${keptTotal} unbacked_marks=${marksTotal} units=${units.size} failed=${failed.length}`);
for (const f of failed) console.log(`FAILED ${f}`);
