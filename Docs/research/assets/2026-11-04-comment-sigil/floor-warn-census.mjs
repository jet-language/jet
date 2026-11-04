// Would the proposed warning fire on today's corpus? Rule: `//` right after
// an operand with at most one space, where the whole comment body is one
// operand expression (names, numbers, calls, indexing and + - * / %). After a
// name or number any such body counts (`7 // 2`, `total // people`); after
// `)` or `]` the body must start with a digit (`len(xs) // 2`), because
// `f(x) // JSON` is a common label comment. `loose` counts the broad "body
// starts with a name or number" rule, `operand` the operand-body rule without
// the `)`/`]` restriction. Lines whose `//` sits after an odd quote count are
// skipped (rough string guard). Read-only.
// usage: node floor-warn-census.mjs <repo>
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
const repo = process.argv[2];
const env = { ...process.env };
delete env.GIT_INDEX_FILE;
const files = execFileSync('git', ['-C', repo, 'ls-files', '-z', '--', 'Compiler/*.jet', 'Core/*.jet', 'Examples/*.jet', 'tests/*.jet', 'Jetpack/*.jet'], { env, maxBuffer: 1 << 28 }).toString().split('\0').filter(Boolean);
const re = /[A-Za-z0-9_)\]] ?\/\/ ?[A-Za-z0-9_(]/;
const atom = String.raw`[A-Za-z0-9_.]+(?:\([^()]*\)|\[[^\[\]]*\])?`;
const operandBody = new RegExp(String.raw`^ ?${atom}(?: ?[-+*/%] ?${atom})*\s*$`);
let loose = 0;
let operand = 0;
let hits = 0;
const samples = [];
for (const f of files) {
  let src;
  try { src = readFileSync(`${repo}/${f}`, 'utf8'); } catch { continue; }
  src.split('\n').forEach((line, i) => {
    const at = line.search(re);
    if (at < 0) return;
    const before = line.slice(0, at + 1);
    if ((before.match(/"/g) || []).length % 2 === 1 || before.includes('`')) return;
    if (/^\s*\/\//.test(line)) return;
    loose++;
    const body = line.slice(line.indexOf('//', at) + 2);
    if (!operandBody.test(body)) return;
    operand++;
    if (/[)\]]/.test(line[at]) && !/^ ?[0-9]/.test(body)) return;
    hits++;
    if (samples.length < 100) samples.push(`${f}:${i + 1}: ${line.trim().slice(0, 140)}`);
  });
}
console.log(JSON.stringify({ files: files.length, loose, operand, hits, samples }, null, 1));
