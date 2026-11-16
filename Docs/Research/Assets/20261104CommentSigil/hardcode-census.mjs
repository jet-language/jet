// Finds code that spells Jet comment syntax by hand instead of reading the
// registry: "//" or "///" string literals, '/' == '/' char tests, and
// byte-47 pairs (read-only). Prints counts and every site.
// usage: [OUT=file.txt] node hardcode-census.mjs <repo>
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';

const repo = process.argv[2];
const env = { ...process.env };
delete env.GIT_INDEX_FILE;
const files = execFileSync('git', ['-C', repo, 'ls-files', '-z', '--', 'Compiler', 'Tools', 'Jetpack', 'Core'], { env, maxBuffer: 1 << 28 })
  .toString().split('\0').filter((f) => /\.(jet|mjs|js|json|toml|scm|tmGrammar)$/.test(f) && !f.includes('/src/parser.c') && !f.endsWith('grammar.json') && !f.endsWith('node-types.json'));
const rules = [
  ['string "//" or "///" or "/*"', /"\/\/\/?"|"\/\*"|"\*\/"|starts_with\("\/\/|strip_prefix\("\/\/|"\/\/\/ |"\/\/ "/],
  ['char test / then /', /'\/' && \w+ == '\/'|ch === "\/" && next === "\/"/],
  ['byte 47 pair', /\b47\b[^\n]{0,60}\b4[27]\b/],
  ['regex //', /\/\\\/\\\/|"\/\/\.\*\$"|"\/\/\/\.\*\$"|seq\("\/\/\/?"/],
];
const hits = [];
const counts = Object.fromEntries(rules.map(([k]) => [k, 0]));
const fileSet = new Set();
for (const f of files) {
  let src;
  try { src = readFileSync(`${repo}/${f}`, 'utf8'); } catch { continue; }
  src.split('\n').forEach((line, i) => {
    const t = line.trim();
    if (t.startsWith('//') || t.startsWith('///')) return;
    for (const [k, re] of rules) {
      if (re.test(line)) { counts[k]++; hits.push(`${f}:${i + 1}: [${k}] ${t.slice(0, 120)}`); fileSet.add(f); break; }
    }
  });
}
const text = `counts ${JSON.stringify(counts)}\nfiles ${fileSet.size}\n` + hits.join('\n') + '\n';
if (process.env.OUT) writeFileSync(process.env.OUT, text);
console.log(`counts ${JSON.stringify(counts)} files ${fileSet.size}`);
