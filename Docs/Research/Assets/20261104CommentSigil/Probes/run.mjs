// Usage: node run.mjs <check|run|raw> file.jet [file2.jet ...]
// Needs JET_SCRATCH_ROOT (holding discovery/base/latest) and JET_TOOLCHAIN_ROOT
// (a dev checkout). Runs the admitted compiler one file at a time (nice -n 19,
// 120 s timeout), prints a header naming the compiler commit, and appends
// everything to out.txt beside this script.
import { spawnSync } from 'node:child_process';
import { readFileSync, appendFileSync } from 'node:fs';
import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
const need = (name) => process.env[name] || (console.error(`set ${name}`), process.exit(2));
const B = `${need('JET_SCRATCH_ROOT')}/discovery/base/latest`;
const ROOT = need('JET_TOOLCHAIN_ROOT');
const HERE = dirname(fileURLToPath(import.meta.url));
const [mode, ...files] = process.argv.slice(2);
const identity = readFileSync(`${B}/IDENTITY`, 'utf8').trim();
const commit = readFileSync(`${B}/COMMIT`, 'utf8').trim();
const jobs = mode === 'raw' ? [files] : files.map((f) => [mode, f]);
for (const args of jobs) {
  const r = spawnSync('timeout', ['120', 'nice', '-n', '19', `${ROOT}/Tools/agent/jet-env`, `${B}/jet`, ...args], {
    env: { ...process.env, JET_COMPILER_IDENTITY: identity, JET_TOOLCHAIN_ROOT: ROOT },
    cwd: HERE,
    encoding: 'utf8',
    maxBuffer: 1 << 26,
  });
  const text = [`===== jet ${args.join(' ')} (compiler ${commit}, base/latest) -> exit ${r.status}`,
    (r.stdout || '').trimEnd(), (r.stderr || '').trimEnd()].filter(Boolean).join('\n') + '\n';
  appendFileSync(`${HERE}/out.txt`, text);
  process.stdout.write(text);
}
