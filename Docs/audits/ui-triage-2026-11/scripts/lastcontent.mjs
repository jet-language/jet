// node lastcontent.mjs TRI.json : adds content_commit (last commit that changed the snapshot beyond the
// `Error [E0803]` -> `Error [jet.Name]` header respelling) and first_commit (the commit that added it).
import { readFileSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
const [triPath] = process.argv.slice(2);
const GIT = process.env.REPO ?? process.cwd(); // a git checkout that has REV
const REV = process.env.REV ?? '125e2e8f89';
const env = { ...process.env }; delete env.GIT_INDEX_FILE;
const tri = JSON.parse(readFileSync(triPath, 'utf8'));
const paths = [...new Set(tri.map((r) => r.snapshot).filter(Boolean))];
const norm = (l) => l.replace(/\[(?:E\d{4}|L\d{4}|jet\.[A-Za-z0-9]+|[A-Za-z]+\.[A-Za-z0-9.]+)\]/g, '[*]');
const info = new Map();
const chunk = 60;
for (let i = 0; i < paths.length; i += chunk) {
  const part = paths.slice(i, i + chunk);
  const r = spawnSync('git', ['log', '-p', '--no-renames', '--format=@@C %h %cs %s', REV, '--', ...part], { cwd: GIT, env, encoding: 'utf8', maxBuffer: 1 << 30 });
  let commit = null, file = null, minus = [], plus = [], added = false;
  const flush = () => {
    if (!file || !commit) return;
    const rec = info.get(file) ?? { content: null, first: null, header_only: [] };
    const headerOnly = !added && minus.length === plus.length && minus.every((m, k) => norm(m) === norm(plus[k]));
    if (headerOnly) rec.header_only.push(commit.h); else if (!rec.content) rec.content = commit;
    if (added) rec.first = commit;
    info.set(file, rec);
  };
  for (const line of r.stdout.split('\n')) {
    if (line.startsWith('@@C ')) { flush(); file = null; const [, h, d, ...s] = line.split(' '); commit = { h, d, s: s.join(' ').slice(0, 100) }; continue; }
    if (line.startsWith('diff --git ')) { flush(); file = line.split(' b/')[1]; minus = []; plus = []; added = false; continue; }
    if (line.startsWith('new file mode')) { added = true; continue; }
    if (line.startsWith('---') || line.startsWith('+++')) continue;
    if (line.startsWith('-')) minus.push(line.slice(1));
    else if (line.startsWith('+')) plus.push(line.slice(1));
  }
  flush();
}
for (const r of tri) {
  const rec = r.snapshot ? info.get(r.snapshot) : null;
  r.content_commit = rec?.content ? `${rec.content.h} ${rec.content.d}` : '';
  r.content_subject = rec?.content?.s ?? '';
  r.first_commit = rec?.first ? `${rec.first.h} ${rec.first.d}` : '';
}
writeFileSync(triPath, JSON.stringify(tri, null, 1) + '\n');
const era = {};
for (const r of tri) { const d = r.content_commit.split(' ')[1] ?? ''; const k = !d ? 'n/a' : d < '2026-10-23' ? 'rust-era' : 'jet-era'; era[k] = (era[k] ?? 0) + 1; }
console.log(JSON.stringify(era));
