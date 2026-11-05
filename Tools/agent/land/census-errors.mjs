// census-errors.mjs <keep-going report> <tree>: every row of a package-form keep-going census
// (the JET_KEEP_GOING_REPORT of `jet check Compiler/JetCli` or `jet build Compiler`), one line
// each, its span mapped to the repository file:line in <tree>, the source tree that was compiled
// (its compiler packages come from <tree>/Tools/agent/compiler-packages.mjs). The report, not the
// stderr, is the census: a dependency package's errors reach the stderr only as E0626.
// Output: "<n> rows {code: count}", then "code<TAB>file:line<TAB>message" per row (the report's
// span or item where the span names no file of <tree>).
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

// A keep-going span `path:start..end` -> {root (repository path), line} in the
// export `tree`. A package build names a file by its absolute path under the
// tree it compiled, which need not be `tree` (a chain stage builds its own
// export): the longest suffix of that path that is a file of `tree` names it.
// A relative path is from the repository root, from the workspace (Compiler/),
// or from its package (one of `packageDirs`, repo-relative); one that several
// of those hold is ambiguous and stays unmapped.
function sourceMapper(tree, packageDirs = []) {
  const lines = new Map();
  const isFile = file => fs.statSync(path.join(tree, file), { throwIfNoEntry: false })?.isFile() ?? false;
  const resolveFile = file => {
    if (path.isAbsolute(file)) {
      const parts = path.normalize(file).split(path.sep).filter(Boolean);
      for (let i = 0; i < parts.length; i++) if (isFile(parts.slice(i).join('/'))) return parts.slice(i).join('/');
      return null;
    }
    const found = new Set(['', 'Compiler', ...packageDirs].map(base => path.normalize(path.join(base, file))).filter(candidate => !candidate.startsWith('..') && isFile(candidate)));
    return found.size === 1 ? [...found][0] : null;
  };
  return span => {
    const at = /^(.*):(\d+)\.\.(\d+)$/.exec(span ?? '');
    if (!at) return null;
    const file = resolveFile(at[1]);
    if (!file) return null;
    if (!lines.has(file)) {
      const bytes = fs.readFileSync(path.join(tree, file));
      const starts = [0];
      for (let i = 0; i < bytes.length; i++) if (bytes[i] === 10) starts.push(i + 1);
      lines.set(file, starts);
    }
    const starts = lines.get(file), byte = Number(at[2]);
    let l = 0, h = starts.length - 1;
    while (l < h) { const mid = (l + h + 1) >> 1; if (starts[mid] <= byte) l = mid; else h = mid - 1; }
    return { root: file.split(path.sep).join('/'), line: l + 1 };
  };
}

const [reportFile, treeArg] = process.argv.slice(2);
if (!reportFile || !treeArg) throw Error('usage: census-errors.mjs <keep-going report> <tree>');
const tree = path.resolve(treeArg);
const packages = await import(pathToFileURL(path.join(tree, 'Tools/agent/compiler-packages.mjs')).href);
const packageDirs = packages.compilerSources(tree).map(entry => entry.directory);
const mapper = sourceMapper(tree, packageDirs);
if (!fs.existsSync(reportFile)) throw Error(`no keep-going report at ${reportFile}: the compile wrote no census`);
const lines = fs.readFileSync(reportFile, 'utf8').split('\n');
const header = (lines[0] ?? '').split('\t');
const col = name => header.indexOf(name);
const rows = [];
for (const line of lines.slice(1)) {
  if (!line.trim()) continue;
  const f = line.split('\t');
  const span = f[col('span')] ?? '-';
  const origin = mapper(span);
  const where = origin ? `${origin.root}:${origin.line}` : span !== '-' ? span : f[col('item')] ?? '-';
  rows.push({ code: f[col('code')] ?? '-', where, message: `${f[col('phase')] ?? '-'}: ${f[col('message')] ?? ''}`.slice(0, 200) });
}
const codes = {};
for (const r of rows) codes[r.code] = (codes[r.code] ?? 0) + 1;
console.log(`${rows.length} rows`, JSON.stringify(codes));
for (const r of rows) console.log(`${r.code}\t${r.where}\t${r.message}`);
