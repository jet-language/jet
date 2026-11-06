// node uitri.mjs UI.json TREE OUT.json : one row per non-passing, non-known-failing ui fixture with
// expected/actual codes, the change class and the snapshot's last commit at REV (env REPO, REV).
import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
const [uiPath, tree, out] = process.argv.slice(2);
const GIT = process.env.REPO ?? process.cwd(); // a git checkout that has REV
const REV = process.env.REV ?? '125e2e8f89';
const env = { ...process.env }; delete env.GIT_INDEX_FILE;
const ui = JSON.parse(readFileSync(uiPath, 'utf8'));
const home = process.env.HOME;
const rel = (p) => String(p ?? '').split(tree + '/').join('').split(home).join('~');

// path -> last commit touching it (one git pass)
const log = spawnSync('git', ['log', '--format=C %h %cs %s', '--name-only', REV, '--', 'tests/ui', 'tests/ui_lint'], { cwd: GIT, env, encoding: 'utf8', maxBuffer: 1 << 30 }).stdout;
const last = new Map();
let cur = null;
for (const line of log.split('\n')) {
  if (line.startsWith('C ')) { const [, h, d, ...s] = line.split(' '); cur = { h, d, s: s.join(' ').slice(0, 90) }; continue; }
  if (line && cur && !last.has(line)) last.set(line, cur);
}

const codesOf = (lines) => {
  const codes = [], names = [];
  for (const l of lines) {
    const m = l.match(/jet-lang\.dev\/e\/([EL]\d{4})/); if (m) codes.push(m[1]);
    const n = l.match(/^(?:Error|Warning|Stop|Lint|Note)\s*\[(jet\.[A-Za-z0-9]+|[EL]\d{4})\]/); if (n) names.push(n[1]);
  }
  return { codes, names };
};
const firstMsg = (lines) => (lines.find((l) => /^(Error|Warning|Stop|Lint)/.test(l)) ?? lines.find((l) => l.trim()) ?? '').trim().slice(0, 160);

const rows = [];
for (const r of ui.results) {
  if (r.status === 'PASS' || r.status === 'KNOWN-FAILING') continue;
  const fixture = rel(r.case);
  const detail = rel(r.detail);
  const row = { fixture, tier: r.tier, status: r.status, detail: detail.slice(0, 200), cls: '', exp_codes: '', act_codes: '', exp_name: '', act_name: '', exp_first: '', act_first: '' };
  const snapMatch = detail.match(/snapshot mismatch: (\S+)/);
  if (snapMatch) {
    const diff = String(r.diff ?? '').split('\n').filter((l) => !l.startsWith('---') && !l.startsWith('+++') && !l.startsWith('@@'));
    const exp = diff.filter((l) => l.startsWith('-') || l.startsWith(' ')).map((l) => l.slice(1));
    const act = diff.filter((l) => l.startsWith('+') || l.startsWith(' ')).map((l) => l.slice(1));
    const snap = `${tree}/${snapMatch[1]}`;
    const expAll = existsSync(snap) ? readFileSync(snap, 'utf8').split('\n') : exp;
    const e = codesOf(expAll), a = codesOf(act);
    row.snapshot = snapMatch[1];
    row.exp_codes = [...new Set(e.codes)].join(',');
    row.act_codes = [...new Set(a.codes)].join(',');
    row.exp_name = [...new Set(e.names)].join(',');
    row.act_name = [...new Set(a.names)].join(',');
    row.exp_first = firstMsg(expAll);
    row.act_first = firstMsg(act);
    const actText = act.join('\n');
    if (a.codes.length === 0 && /no errors|no problems/i.test(actText + ' ' + detail)) row.cls = 'no-diagnostic';
    else if (a.codes.length === 0 && act.join('').trim() === '') row.cls = 'empty-output';
    else if (row.exp_codes !== row.act_codes) row.cls = (e.codes[0] !== a.codes[0]) ? 'code-changed' : 'extra-or-missing-diagnostic';
    else row.cls = 'same-code-text';
  } else if (/missing expected stream/.test(detail)) row.cls = 'missing-stream';
  else if (/lint fixture must compile and emit a lint/.test(detail)) row.cls = 'lint-silent';
  else if (/missing CLI fixture API/.test(detail)) row.cls = 'refused-cli-api';
  else if (/tier-split/.test(detail)) row.cls = 'tier-split';
  else row.cls = 'other';
  const src = fixture.replace(/^.*?(tests\/ui(?:_lint)?\/)/, '$1');
  const lc = last.get(row.snapshot ?? src.replace(/\.jet$/, '.stderr')) ?? last.get(src);
  row.snap_commit = lc ? `${lc.h} ${lc.d}` : '';
  row.snap_subject = lc ? lc.s : '';
  rows.push(row);
}
writeFileSync(out, JSON.stringify(rows, null, 1) + '\n');
const by = {};
for (const r of rows) by[r.cls] = (by[r.cls] ?? 0) + 1;
console.log(rows.length, JSON.stringify(by), JSON.stringify(ui.counts));
