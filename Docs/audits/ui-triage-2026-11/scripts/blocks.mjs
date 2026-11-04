// node blocks.mjs UI.json TREE TRI.json CAPDIR : uses CAPDIR/actual/<fixture>.actual when present, else rebuilds the actual stream by applying the testkit diff to the expected
// snapshot, parses both into diagnostic blocks, and records a block-level comparison on each TRI row:
//   blk_kind: one of text-only / code-swap / missing-blocks / extra-blocks / mixed / order-only / no-diagnostic / n/a
//   blk_changes: list like `E0217@12:13 missing`, `E0803@4:15>E0311@4:28`, `E0151 why`, `E0150 extra`
//   reg_text: for text-only changes, whether the actual Why/Fix equal the registry row (DiagnosticRows.jet)
import { readFileSync, writeFileSync, existsSync } from 'node:fs';
const [uiPath, tree, triPath, capDir] = process.argv.slice(2);
const ui = JSON.parse(readFileSync(uiPath, 'utf8'));
const tri = JSON.parse(readFileSync(triPath, 'utf8'));
const rel = (p) => p.replace(/^.*?((?:tests|Examples)\/)/, '$1');

// Registry rows: code -> {name, what, why, fix, status}
const reg = new Map();
{
  const src = readFileSync(`${tree}/Compiler/JetFoundation/Source/Registry/DiagnosticRows.jet`, 'utf8');
  for (const m of src.matchAll(/\{\s*code: "([^"]+)",\s*name: "([^"]*)",[\s\S]*?status: DiagnosticRegistryStatus\.(\w+),[\s\S]*?what: "((?:[^"\\]|\\.)*)",\s*why: "((?:[^"\\]|\\.)*)",\s*fix: "((?:[^"\\]|\\.)*)",/g)) {
    reg.set(m[1], { name: m[2], status: m[3], what: m[4], why: m[5], fix: m[6] });
  }
}
const byName = new Map([...reg].map(([c, r]) => [r.name, c]));

function applyDiff(expected, diff) {
  const exp = expected.split('\n');
  const lines = String(diff).split('\n');
  const out = [];
  let ei = 0;
  let i = 0;
  while (i < lines.length && !lines[i].startsWith('@@')) i++;
  for (; i < lines.length; i++) {
    const l = lines[i];
    const h = l.match(/^@@ -(\d+)(?:,(\d+))? \+\d+(?:,\d+)? @@/);
    if (h) { const start = Number(h[1]) - (h[2] === '0' ? 0 : 1); while (ei < start) out.push(exp[ei++]); continue; }
    if (l.startsWith('+')) out.push(l.slice(1));
    else if (l.startsWith('-')) ei++;
    else if (l.startsWith(' ')) { out.push(exp[ei++]); }
    else if (l === '\\ No newline at end of file') continue;
  }
  while (ei < exp.length) out.push(exp[ei++]);
  return out.join('\n');
}

function parse(text) {
  const blocks = [];
  let cur = null;
  for (const line of text.split('\n')) {
    const h = line.match(/^(Error|Warning|Stop|Lint|Note|Panic)\s*\[([^\]]+)\]:\s?(.*)$/);
    if (h) { cur = { sev: h[1], name: h[2], head: h[3], span: '', why: '', fix: '', code: '', snippet: [] }; blocks.push(cur); continue; }
    if (!cur) continue;
    let m;
    if ((m = line.match(/^\s*--> (.*?):(\d+):(\d+)/))) { if (!cur.span) cur.span = `${m[2]}:${m[3]}`; continue; }
    if ((m = line.match(/^ Why: (.*)$/))) { cur.why = m[1]; continue; }
    if ((m = line.match(/^ Fix: (.*)$/))) { cur.fix = m[1]; continue; }
    if ((m = line.match(/^More: .*\/e\/(\S+)/))) { cur.code = m[1]; continue; }
    if (/^\s*(\d+ )?\|/.test(line)) cur.snippet.push(line.trimEnd());
  }
  for (const b of blocks) if (!b.code) b.code = /^[EL]\d{4}$/.test(b.name) ? b.name : (byName.get(b.name) ?? b.name);
  return blocks;
}

const tmpl = (s) => new RegExp('^' + s.replace(/\\"/g, '"').replace(/[.*+?^${}()|[\]\\]/g, '\\$&').replace(/\\\{\\\{[^}]*\\\}\\\}/g, '.*') + '$', 'i');
const regMatch = (b, field) => { const r = reg.get(b.code); if (!r || !r[field]) return false; try { return tmpl(r[field]).test(b[field]); } catch { return false; } };

const rows = new Map(ui.results.map((r) => [`${rel(r.case)}|${r.tier || '-'}`, r]));
for (const t of tri) {
  const r = rows.get(`${t.fixture}|${t.tier || '-'}`);
  t.blk_kind = 'n/a'; t.blk_changes = ''; t.reg_text = '';
  if (!r || r.status !== 'FAIL' || !t.snapshot || !existsSync(`${tree}/${t.snapshot}`) || !String(r.diff).includes('@@')) continue;
  const expected = readFileSync(`${tree}/${t.snapshot}`, 'utf8');
  const capFile = `${capDir}/actual/${t.fixture}.actual`;
  let actual;
  if (existsSync(capFile)) { actual = readFileSync(capFile, 'utf8'); t.actual_source = 'captured'; }
  else {
    const lines = String(r.diff).split('\n');
    const h = lines.find((l) => l.startsWith('@@'))?.match(/\+1,(\d+)/);
    const plus = lines.filter((l) => l.startsWith('+') && !l.startsWith('+++')).map((l) => l.slice(1));
    actual = plus.join('\n');
    t.actual_source = h && plus.length >= Number(h[1]) - 1 ? 'diff-complete' : 'diff-truncated';
  }
  const E = parse(expected), A = parse(actual);
  if (A.length === 0 && E.length > 0) { t.blk_kind = 'no-diagnostic'; t.blk_changes = E.map((b) => `${b.code}@${b.span} missing`).join('; '); t.actual_text = actual.slice(0, 400); continue; }
  const used = new Set();
  const changes = [];
  const kinds = new Set();
  const regs = [];
  const matched = [];
  // pass 1: same code and same span
  for (const e of E) {
    const k = A.findIndex((a, j) => !used.has(j) && a.code === e.code && a.span === e.span);
    if (k >= 0) { used.add(k); matched.push([e, A[k]]); e.m = true; }
  }
  // pass 2: same code, other span
  for (const e of E) {
    if (e.m) continue;
    const k = A.findIndex((a, j) => !used.has(j) && a.code === e.code);
    if (k >= 0) { used.add(k); matched.push([e, A[k]]); e.m = true; }
  }
  for (const [e, a] of matched) {
    const f = [];
    if (e.span !== a.span) f.push(`span ${e.span}>${a.span}`);
    if (e.head !== a.head) f.push('head');
    if (e.why !== a.why) f.push('why');
    if (e.fix !== a.fix) f.push('fix');
    if (e.sev !== a.sev) f.push(`sev ${e.sev}>${a.sev}`);
    if (e.snippet.join('\n') !== a.snippet.join('\n') && e.span === a.span) f.push('snippet');
    if (f.length) {
      kinds.add('text'); changes.push(`${e.code} ${f.join(',')}`);
      if (f.includes('why') || f.includes('fix') || f.includes('head')) regs.push(`${e.code}:${['what', 'why', 'fix'].map((x) => (x === 'what' ? (regMatch({ ...a, what: a.head }, 'what') ? 'W' : 'w') : regMatch(a, x) ? x[0].toUpperCase() : x[0])).join('')}/${['what', 'why', 'fix'].map((x) => (x === 'what' ? (regMatch({ ...e, what: e.head }, 'what') ? 'W' : 'w') : regMatch(e, x) ? x[0].toUpperCase() : x[0])).join('')}`);
    }
  }
  const missE = E.filter((e) => !e.m);
  const extraA = A.filter((a, j) => !used.has(j));
  // pair a missing with an extra at the same line as a code swap
  for (const e of missE) {
    const line = e.span.split(':')[0];
    const k = extraA.findIndex((a) => !a.p && a.span.split(':')[0] === line);
    if (k >= 0) { extraA[k].p = true; e.p = true; kinds.add('swap'); changes.push(`${e.code}@${e.span}>${extraA[k].code}@${extraA[k].span}`); }
  }
  for (const e of missE) if (!e.p) {
    const k = extraA.findIndex((a) => !a.p);
    if (k >= 0 && missE.filter((x) => !x.p).length === 1 && extraA.filter((x) => !x.p).length === 1) { extraA[k].p = true; e.p = true; kinds.add('swap'); changes.push(`${e.code}@${e.span}>${extraA[k].code}@${extraA[k].span}`); }
  }
  for (const e of missE) if (!e.p) { kinds.add('missing'); changes.push(`${e.code}@${e.span} missing`); }
  for (const a of extraA) if (!a.p) { kinds.add('extra'); changes.push(`${a.code}@${a.span} extra`); }
  if (!changes.length) {
    const eo = E.map((b) => b.code + b.span).join(), ao = A.map((b) => b.code + b.span).join();
    t.blk_kind = eo !== ao ? 'order-only' : 'non-block-text';
  } else if (kinds.size === 1) t.blk_kind = { text: 'text-only', swap: 'code-swap', missing: 'missing-blocks', extra: 'extra-blocks' }[[...kinds][0]];
  else t.blk_kind = 'mixed:' + [...kinds].sort().join('+');
  t.blk_changes = changes.join('; ');
  t.reg_text = regs.join(' ');
  if (t.blk_kind === 'non-block-text') t.actual_text = actual.slice(0, 400);
}
writeFileSync(triPath, JSON.stringify(tri, null, 1) + '\n');
const sum = {};
for (const t of tri) sum[t.blk_kind] = (sum[t.blk_kind] ?? 0) + 1;
console.log(JSON.stringify(sum), 'registry rows', reg.size);
