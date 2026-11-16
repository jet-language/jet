// node codemap.mjs TREE OUT.json CODE... : per code, its registry name, whether Diagnostics.jet marks it retired,
// and its emit sites in Compiler/ and Jetpack/ source (registry tables and Tests excluded).
import { readFileSync, readdirSync, writeFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
const [tree, out, ...codes] = process.argv.slice(2);
const REG = /Registry\/DiagnosticRows\.jet$|Prelude\/Diagnostics\.jet$|TrapMessages\.jet$|LintRows|Registry\/Lint/;
const files = [];
const walk = (d) => { for (const e of readdirSync(join(tree, d), { withFileTypes: true })) { const r = `${d}/${e.name}`; if (e.isDirectory()) { if (!/^(\.jet|Tests?|node_modules)$/.test(e.name)) walk(r); } else if (/\.(jet|mjs|js|c)$/.test(e.name) && !REG.test(r)) files.push(r); } };
walk('Compiler'); walk('Jetpack');
const text = new Map(files.map((f) => [f, readFileSync(join(tree, f), 'utf8')]));
const rows = readFileSync(join(tree, 'Compiler/JetFoundation/Source/Registry/DiagnosticRows.jet'), 'utf8');
const diag = readFileSync(join(tree, 'Compiler/Prelude/Diagnostics.jet'), 'utf8');
const alias = new Map();
for (const m of rows.matchAll(/code: "([A-Z0-9-]+)",\s*name: "([^"]+)",\s*lint_name: (?:Val\("([^"]+)"\)|None)/g)) alias.set(m[1], [m[2], m[3]].filter(Boolean));
const esc = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
const result = {};
for (const code of [...new Set(codes)]) {
  const names = [code, ...(alias.get(code) ?? [])];
  const re = new RegExp(`["\`](${names.map(esc).join('|')})["\`]`);
  const emit = files.filter((f) => re.test(text.get(f)));
  const at = diag.indexOf(`code :: "${code}"`);
  const block = at >= 0 ? diag.slice(at, at + 900) : '';
  const retired = /retired|Retired|superseded/.test(block.split(/\n\s*\}\n/)[0] ?? '');
  result[code] = { name: (alias.get(code) ?? [])[0] ?? '', registered: alias.has(code), retired, emit: emit.length, sites: emit.slice(0, 4) };
}
writeFileSync(out, JSON.stringify(result, null, 1) + '\n');
console.log(Object.keys(result).length, 'codes');
