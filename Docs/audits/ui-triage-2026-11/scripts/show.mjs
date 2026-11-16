// node show.mjs TREE TRI.json CAPDIR FIXTURE... : prints expected snapshot and captured actual side by side (sequentially), trimmed.
import { readFileSync, existsSync } from 'node:fs';
const [T, triPath, capDir, ...wanted] = process.argv.slice(2);
const tri = JSON.parse(readFileSync(triPath, 'utf8'));
const max = Number(process.env.MAX ?? 30);
for (const f of wanted) {
  const t = tri.find((x) => x.fixture.endsWith(f));
  if (!t) { console.log('?? ' + f); continue; }
  const exp = t.snapshot && existsSync(`${T}/${t.snapshot}`) ? readFileSync(`${T}/${t.snapshot}`, 'utf8') : '(none)';
  const capf = `${capDir}/actual/${t.fixture}.actual`;
  const act = existsSync(capf) ? readFileSync(capf, 'utf8') : '(not captured)';
  const cut = (s) => s.split('\n').filter((l) => !/^\s*\|\s*$/.test(l)).slice(0, max).join('\n');
  console.log(`##### ${t.fixture} [${t.blk_kind}] ${t.blk_changes.slice(0, 120)}`);
  if (process.env.SRC) console.log('--- source\n' + readFileSync(`${T}/${t.fixture}`, 'utf8').split('\n').slice(0, Number(process.env.SRC)).join('\n'));
  console.log('--- expected\n' + cut(exp));
  console.log('--- actual\n' + cut(act));
}
