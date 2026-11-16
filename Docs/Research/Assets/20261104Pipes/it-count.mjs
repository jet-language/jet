// Count reads of the keyword `it` (the current subject) in tracked .jet files.
// Comments are removed. Uses inside string interpolation braces count separately.
// Usage: node it-count.mjs <repo-root>
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';

const root = process.argv[2];
const env = { ...process.env }; delete env.GIT_INDEX_FILE;
const files = execFileSync('git', ['ls-files', '--', '*.jet'], { cwd: root, encoding: 'utf8', env, maxBuffer: 1 << 28 })
  .split('\n').filter(Boolean);
const word = /(^|[^A-Za-z0-9_.$#@])it(?![A-Za-z0-9_])/;
let code = 0, interp = 0;
const byFolder = {};
for (const f of files) {
  const text = readFileSync(`${root}/${f}`, 'utf8').replace(/\/\*[\s\S]*?\*\//g, '');
  text.split('\n').forEach((raw, i) => {
    // Drop a line comment that is not inside a string.
    let line = '', inStr = false;
    for (let k = 0; k < raw.length; k++) {
      const c = raw[k];
      if (!inStr && c === '/' && raw[k + 1] === '/') break;
      if (c === '"' && raw[k - 1] !== '\\') inStr = !inStr;
      line += c;
    }
    const holes = [...line.matchAll(/"(?:\\.|[^"\\])*"/g)].flatMap(m => [...m[0].matchAll(/\{([^{}]*)\}/g)].map(h => h[1]));
    const bare = line.replace(/"(?:\\.|[^"\\])*"/g, '""');
    const kind = word.test(bare) ? 'code' : holes.some(h => word.test(h)) ? 'interp' : null;
    if (!kind) return;
    if (kind === 'code') code++; else interp++;
    const top = f.split('/')[0];
    byFolder[top] = (byFolder[top] ?? 0) + 1;
    console.log(`${kind} ${f}:${i + 1}: ${raw.trim()}`);
  });
}
console.log(JSON.stringify({ files: files.length, code, interp, byFolder }));
