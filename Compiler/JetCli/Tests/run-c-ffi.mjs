// Native O0/O2 C FFI parity. Invoked by the integrator under the cheap proof lane.
import { cpSync, mkdirSync, readFileSync, realpathSync } from 'node:fs';
import { basename, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

if (process.argv.length !== 4) {
  throw new Error('usage: node Compiler/JetCli/Tests/run-c-ffi.mjs <jet> <scratch>');
}
const jet = realpathSync(process.argv[2]);
const scratch = resolve(process.argv[3]);
const repo = fileURLToPath(new URL('../../../', import.meta.url));
mkdirSync(scratch, { recursive: true });
const filter = path => basename(path) !== '.jet';
cpSync(join(repo, 'Examples/features/lowlevel/native_c'), join(scratch, 'native_c'), { recursive: true, filter });
cpSync(join(repo, 'Core/vendor/sqlite3'), join(scratch, 'sqlite3'), { recursive: true, filter });
const fixtures = [
  ['native_c', readFileSync(join(repo, 'Examples/features/expected/lowlevel/native_c/run.out'), 'utf8')],
  ['sqlite3/fixture', 'sqlite-memory=42\n'],
];
for (const [directory, expected] of fixtures) {
  let native;
  for (const release of [false, true]) {
    const args = ['run', ...(release ? ['--release'] : []), join(scratch, directory, 'run.jet')];
    const result = spawnSync(jet, args, { cwd: scratch, encoding: 'utf8', env: { ...process.env, NO_COLOR: '1' } });
    const tier = release ? 'native O2' : 'native O0';
    if (result.error || result.status !== 0 || result.signal) {
      throw new Error(`${directory} ${tier} failed: ${result.error ?? result.signal ?? result.status}\n${result.stdout ?? ''}${result.stderr ?? ''}`);
    }
    if (result.stdout !== expected) {
      throw new Error(`${directory} ${tier} output differs: ${JSON.stringify(result.stdout)}; expected ${JSON.stringify(expected)}`);
    }
    if (release && result.stdout !== native) throw new Error(`${directory} tier meaning differs`);
    native = result.stdout;
  }
  console.log(`${directory}: native O0/O2 identical`);
}
