// node uihist.mjs TRI.json DIR... : reads every full ui run text file (ui*.txt, STATUS\tpath\ttier lines) in DIR...,
// and for each failing fixture in TRI records the newest run where it passed and the oldest later run where it failed.
import { readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
const [triPath, ...dirs] = process.argv.slice(2);
const runs = [];
for (const d of dirs) {
  let names = [];
  try { names = readdirSync(d); } catch { continue; }
  for (const n of names) {
    if (!/^ui.*\.txt$/.test(n)) continue;
    const p = `${d}/${n}`;
    const st = statSync(p);
    if (!st.isFile() || st.size < 300000) continue;
    const text = readFileSync(p, 'utf8');
    const status = new Map();
    let count = 0;
    for (const line of text.split('\n')) {
      const m = line.match(/^(PASS|FAIL|KNOWN-FAILING|REFUSED|ERROR|UNEXPECTED-PASS)\t(\S+)\t([^\t]*)/);
      if (!m) continue;
      const path = m[2].replace(/^.*?(tests\/ui(?:_lint)?\/)/, '$1');
      status.set(`${path}|${m[3] || '-'}`, m[1]);
      count++;
    }
    if (count < 1500) continue;
    const pass = [...status.values()].filter((s) => s === 'PASS').length;
    runs.push({ name: n, mtime: st.mtime.toISOString().slice(0, 16), status, count, pass });
  }
}
runs.sort((a, b) => a.mtime.localeCompare(b.mtime));
console.log(runs.map((r) => `${r.mtime} ${r.name} rows=${r.count} pass=${r.pass}`).join('\n'));
const tri = JSON.parse(readFileSync(triPath, 'utf8'));
for (const r of tri) {
  const key = `${r.fixture}|${r.tier || '-'}`;
  let lastPass = null, firstFailAfter = null, seen = 0;
  for (const run of runs) {
    const s = run.status.get(key);
    if (!s) continue;
    seen++;
    if (s === 'PASS') { lastPass = run; firstFailAfter = null; }
    else if (lastPass && !firstFailAfter) firstFailAfter = { run, s };
  }
  r.hist_seen = seen;
  r.hist_last_pass = lastPass ? `${lastPass.mtime} ${lastPass.name}` : '';
  r.hist_first_fail = firstFailAfter ? `${firstFailAfter.run.mtime} ${firstFailAfter.run.name}` : '';
  const first = runs.find((run) => run.status.has(key));
  r.hist_first_status = first ? `${first.status.get(key)} ${first.mtime}` : '';
}
writeFileSync(triPath, JSON.stringify(tri, null, 1) + '\n');
const sum = { never_seen: 0, never_passed: 0, passed_then_failed: 0 };
for (const r of tri) { if (!r.hist_seen) sum.never_seen++; else if (!r.hist_last_pass) sum.never_passed++; else sum.passed_then_failed++; }
console.log(JSON.stringify(sum));
