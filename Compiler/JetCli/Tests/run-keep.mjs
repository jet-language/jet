// `jet run` keeps its program (Compiler/JetCli/Source/Cli/ScriptCache.jet).
// A rerun of unchanged inputs reuses it with identical output. A source edit,
// a Core change and a compiler change each compile again and replace it.
// Runs against copies: the compiler (beside its jet-runtime), a toolchain
// root holding only Core, and its own XDG cache.
import { appendFileSync, copyFileSync, cpSync, mkdirSync, readdirSync, readFileSync, realpathSync, rmSync, statSync, symlinkSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

if (process.argv.length !== 4) {
  throw new Error('usage: node Compiler/JetCli/Tests/run-keep.mjs <jet> <scratch>');
}
const given = realpathSync(process.argv[2]);
const scratch = resolve(process.argv[3]);
const repo = fileURLToPath(new URL('../../../', import.meta.url));
rmSync(scratch, { recursive: true, force: true });
const bin = join(scratch, 'bin');
const root = join(scratch, 'root');
const cache = join(scratch, 'cache');
const work = join(scratch, 'work');
for (const directory of [bin, root, cache, work]) mkdirSync(directory, { recursive: true });
const jet = join(bin, 'jet');
copyFileSync(given, jet);
symlinkSync(realpathSync(join(dirname(given), 'jet-runtime')), join(bin, 'jet-runtime'));
cpSync(join(repo, 'Core'), join(root, 'Core'), { recursive: true });
const kept = join(cache, 'jet/scripts');
const file = join(work, 'greet.jet');
const source = word => `fn run() {\n    print("hello ${word}")\n}\n`;

const runRaw = () => {
  const result = spawnSync(jet, ['run', file], { cwd: work, encoding: 'utf8', env: { ...process.env, JET_TOOLCHAIN_ROOT: root, XDG_CACHE_HOME: cache, NO_COLOR: '1' } });
  if (result.error) throw new Error(`jet run failed: ${result.error}`);
  return result;
};
const run = () => {
  const result = runRaw();
  if (result.status !== 0 || result.signal) {
    throw new Error(`jet run failed: ${result.signal ?? result.status}\n${result.stdout ?? ''}${result.stderr ?? ''}`);
  }
  return result.stdout;
};
// The slot's record: its text and modification time, and the programs kept beside it.
const slot = () => {
  const names = readdirSync(kept);
  const records = names.filter(name => name.endsWith('.run'));
  if (records.length !== 1) throw new Error(`expected one slot record, found ${JSON.stringify(names)}`);
  const record = join(kept, records[0]);
  const program = readFileSync(record, 'utf8').split('\n')[1];
  const programs = names.filter(name => !name.endsWith('.run'));
  if (programs.length !== 1 || programs[0] !== program) throw new Error(`expected only the program ${program}, found ${JSON.stringify(names)}`);
  return { text: readFileSync(record, 'utf8'), modified: statSync(record, { bigint: true }).mtimeNs, program };
};
const check = (step, output, expected) => {
  if (output !== expected) throw new Error(`${step}: output ${JSON.stringify(output)}, expected ${JSON.stringify(expected)}`);
};
const reused = (step, before) => {
  const after = slot();
  if (after.text !== before.text || after.modified !== before.modified) throw new Error(`${step}: the kept program was replaced, not reused`);
  console.log(`${step}: reused ${after.program}`);
};
const rebuilt = (step, before) => {
  const after = slot();
  if (after.program === before.program) throw new Error(`${step}: still the program ${before.program}; expected a new compile`);
  console.log(`${step}: compiled again (${before.program} -> ${after.program})`);
  return after;
};

writeFileSync(file, source('one'));
const first = run();
check('first run', first, 'hello one\n');
let state = slot();
console.log(`first run: kept ${state.program}`);
check('rerun', run(), first);
reused('rerun', state);

writeFileSync(file, source('two'));
check('source edit', run(), 'hello two\n');
state = rebuilt('source edit', state);
check('rerun after the edit', run(), 'hello two\n');
reused('rerun after the edit', state);

appendFileSync(join(root, 'Core/Time/Time.jet'), '\n// run-keep: a Core change\n');
check('Core change', run(), 'hello two\n');
state = rebuilt('Core change', state);

// A new compiler binary in the same place: a new file identity.
rmSync(jet);
copyFileSync(given, jet);
check('compiler change', run(), 'hello two\n');
state = rebuilt('compiler change', state);
check('rerun after the compiler change', run(), 'hello two\n');
reused('rerun after the compiler change', state);

// A program that stops: the kept run reports it as the compiling run did.
writeFileSync(file, 'fn run() {\n    print("before")\n    panic("run-keep stop")\n}\n');
const compiled = runRaw();
state = rebuilt('stopping program', state);
const rerun = runRaw();
reused('stopping program rerun', state);
if (compiled.status === 0) throw new Error(`stopping program: exit 0\n${compiled.stderr}`);
for (const field of ['status', 'signal', 'stdout', 'stderr']) {
  if (rerun[field] !== compiled[field]) throw new Error(`stopping program: kept run ${field} ${JSON.stringify(rerun[field])}, compiling run ${JSON.stringify(compiled[field])}`);
}
console.log(`stopping program: kept run matches (exit ${compiled.status})`);
console.log('run-keep: ok');
