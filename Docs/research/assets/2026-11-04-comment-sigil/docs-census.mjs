// Counts comment lines inside ```jet fences of tracked Markdown, plus other
// tracked non-.jet files that spell Jet comments (read-only).
// usage: [OUT=file.json] node docs-census.mjs <repo>
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';

const repo = process.argv[2];
const env = { ...process.env };
delete env.GIT_INDEX_FILE;
const all = execFileSync('git', ['-C', repo, 'ls-files', '-z'], { env, maxBuffer: 1 << 28 }).toString().split('\0').filter(Boolean);
const md = all.filter((f) => f.endsWith('.md'));
const byTop = {};
const total = { mdFiles: 0, jetFences: 0, fenceLines: 0, commentLines: 0, docLines: 0, trailing: 0, filesTouched: 0, mdHeadingInDoc: 0 };
for (const f of md) {
  let src;
  try { src = readFileSync(`${repo}/${f}`, 'utf8'); } catch { continue; }
  const top = f.startsWith('Docs/') ? f.split('/').slice(0, 2).join('/') : f.split('/')[0];
  byTop[top] ||= { mdFiles: 0, jetFences: 0, commentLines: 0, docLines: 0, trailing: 0 };
  const b = byTop[top];
  b.mdFiles++; total.mdFiles++;
  let inJet = false; let touched = false;
  for (const raw of src.split('\n')) {
    const line = raw.trim();
    if (/^(```|~~~)/.test(line)) {
      if (!inJet && /^(```|~~~)\s*jet\b/.test(line)) { inJet = true; b.jetFences++; total.jetFences++; continue; }
      if (inJet && /^(```|~~~)\s*$/.test(line)) { inJet = false; continue; }
    }
    if (!inJet) continue;
    total.fenceLines++;
    const noStr = line.replace(/"(?:[^"\\]|\\.)*"/g, '""');
    if (line.startsWith('///')) { b.docLines++; total.docLines++; touched = true; if (/^\/\/\/\s*#{1,6}\s/.test(line)) total.mdHeadingInDoc++; }
    else if (line.startsWith('//')) { b.commentLines++; total.commentLines++; touched = true; }
    else if (/\/\//.test(noStr) && !/:\/\//.test(noStr)) { b.trailing++; total.trailing++; touched = true; }
  }
  if (touched) total.filesTouched++;
}
const out = { repo, total, byTop };
if (process.env.OUT) writeFileSync(process.env.OUT, JSON.stringify(out, null, 1) + '\n');
console.log(JSON.stringify(out, null, 1));
