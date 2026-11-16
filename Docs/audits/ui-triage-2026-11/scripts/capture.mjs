// node capture.mjs TREE TRI.json OUTDIR : re-runs every plain/lint/project FAIL row of TRI the way testkit's
// ui runner does (staged tests/ui + tests/ui_lint, `jet check [--verbose] PATH`, rendered-only stderr,
// path normalisation) and writes the full actual stream to OUTDIR/<fixture>.actual, because the
// testkit diff keeps only 37 lines. Rows with `// @directive` lines are skipped (listed as skipped).
// Output that depends on the host (network, cargo, C toolchain) goes to <fixture>.actual.envdiff, so
// blocks.mjs falls back to the suite diff for those rows.
import { readFileSync, writeFileSync, mkdirSync, existsSync, cpSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, basename } from 'node:path';
const [tree, triPath, outDir] = process.argv.slice(2);
const jet = process.env.JET;
const src = `${outDir}/ui-source`;
const scratch = `${outDir}/scratch`;
mkdirSync(`${scratch}/tmp`, { recursive: true });
if (!existsSync(`${src}/tests/ui`)) {
  for (const d of ['tests/ui', 'tests/ui_lint']) cpSync(`${tree}/${d}`, `${src}/${d}`, { recursive: true });
}
const env = {
  ...process.env, NO_COLOR: '1', JET_RECEIPT_BYPASS: '1', JET_OPT_VERIFY: '1', CLICOLOR: '0', TERM: 'dumb', JET_REPL_HISTORY: 'off',
  JET_STORE_DIR: `${scratch}/store`, JET_FFI_CACHE_DIR: `${scratch}/ffi`, TMPDIR: `${scratch}/tmp`, TMP: `${scratch}/tmp`, TEMP: `${scratch}/tmp`,
  JETPACK_ROOT: `${scratch}/jetpack`, JET_SCHEMA_CACHE_DIR: `${scratch}/schema`,
};
delete env.JET_COMPILER_EXTENSION;
function renderedOnly(text) {
  const lines = text.split('\n');
  const n = lines.length;
  if (n < 5 || lines[n - 1] !== '' || !lines[n - 2].startsWith('run `jet explain ') || !lines[n - 2].endsWith('` to learn more') || lines[n - 4] !== '') return text;
  const s = lines[n - 3];
  const p = s.split(' ');
  const counted = p.length === 3 && Number.isInteger(Number(p[0])) && (p[1] === 'problem' || p[1] === 'problems') && p[2] === 'found';
  const warnings = s.startsWith('ok: `') && s.includes('` has no errors (') && (s.endsWith(' warning)') || s.endsWith(' warnings)'));
  if (!counted && !warnings) return text;
  return lines.slice(0, n - 4).join('\n') + '\n';
}
function normalize(shown, actual) {
  let text = actual.split(`${src}/`).join('');
  const lines = text.replace(/\r\n/g, '\n').split('\n');
  let out = '';
  lines.forEach((line, i) => { if (i === lines.length - 1 && line === '') return; if (line.includes('Blocking waiting for file lock on package cache')) return; out += line + '\n'; });
  const prefix = `  --> ${basename(shown)}:`;
  return out.split('\n').map((l) => (l.startsWith(prefix) ? `  --> ${shown}:${l.slice(prefix.length)}` : l)).join('\n');
}
const ENV_DEPENDENT = new Set(['tests/ui/cffi_e3205/run.jet', 'tests/ui/ffi_bad_path.jet', 'tests/ui/ffi_bad_type.jet', 'tests/ui/ffi_fetch_failed.jet']);
const tri = JSON.parse(readFileSync(triPath, 'utf8'));
const skipped = [];
let ran = 0, differs = 0, same = 0;
for (const t of tri) {
  if (t.status !== 'FAIL' || (t.tier && t.tier !== '-') || !t.snapshot) continue;
  const text = readFileSync(`${tree}/${t.fixture}`, 'utf8');
  if (text.split('\n').some((l) => /^\s*\/\/\s*@/.test(l))) { skipped.push(t.fixture); t.capture = 'skipped-directive'; continue; }
  const lint = t.fixture.startsWith('tests/ui_lint/');
  const argv = lint ? ['check', '--verbose', t.fixture] : ['check', t.fixture];
  const r = spawnSync(jet, argv, { cwd: src, env, encoding: 'utf8', timeout: 120000, maxBuffer: 1 << 28 });
  ran++;
  const stderr = r.stderr ?? '';
  let actual = lint ? renderedOnly(stderr) : r.status === 0 ? '(no errors)\n' : renderedOnly(stderr);
  actual = normalize(t.fixture, actual);
  const file = `${outDir}/actual/${t.fixture}.actual${ENV_DEPENDENT.has(t.fixture) ? '.envdiff' : ''}`;
  mkdirSync(dirname(file), { recursive: true });
  writeFileSync(file, actual);
  const expected = normalize(t.fixture, readFileSync(`${tree}/${t.snapshot}`, 'utf8'));
  const eq = expected === actual;
  if (eq) same++; else differs++;
  t.capture = eq ? 'matches-snapshot' : 'captured';
  t.capture_exit = r.status;
}
writeFileSync(triPath, JSON.stringify(tri, null, 1) + '\n');
console.log(JSON.stringify({ ran, differs, same, skipped: skipped.length }));
console.log(skipped.join('\n'));
