// `jet run` keeps its program (Compiler/JetCli/Source/Cli/ScriptCache.jet).
// A rerun of unchanged inputs reuses it with identical output and marks it
// used. A source edit, a Core change and a compiler change each compile again
// and replace it. The cache holds 64 programs and drops the least recently
// run one. Runs against copies: the compiler (beside its jet-runtime), a
// toolchain root holding only Core, and its own XDG caches.
import { appendFileSync, copyFileSync, cpSync, existsSync, mkdirSync, readdirSync, readFileSync, realpathSync, rmSync, statSync, symlinkSync, writeFileSync } from 'node:fs';
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

const runRaw = (target = file, xdg = cache) => {
  const result = spawnSync(jet, ['run', target], { cwd: work, encoding: 'utf8', env: { ...process.env, JET_TOOLCHAIN_ROOT: root, XDG_CACHE_HOME: xdg, NO_COLOR: '1' } });
  if (result.error) throw new Error(`jet run failed: ${result.error}`);
  return result;
};
const run = (target = file, xdg = cache) => {
  const result = runRaw(target, xdg);
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
// Reused: the same record naming the same program, marked used since `before`.
const reused = (step, before) => {
  const after = slot();
  if (after.text !== before.text) throw new Error(`${step}: the kept program was replaced, not reused`);
  if (after.modified <= before.modified) throw new Error(`${step}: the reused record was not marked used`);
  console.log(`${step}: reused ${after.program}`);
  return after;
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
// The kept form holds the program part and the image's patches, not a copy
// of the runtime image.
const packs = readdirSync(join(bin, 'jet-runtime')).map(key => join(bin, 'jet-runtime', key, 'jet_runtime.image')).filter(path => existsSync(path));
const keptSize = statSync(join(kept, state.program)).size;
if (packs.length === 0 || keptSize >= statSync(packs[0]).size) throw new Error(`first run: the kept program has ${keptSize} bytes, no fewer than the runtime image ${packs[0]}`);
check('rerun', run(), first);
state = reused('rerun', state);

writeFileSync(file, source('two'));
check('source edit', run(), 'hello two\n');
state = rebuilt('source edit', state);
check('rerun after the edit', run(), 'hello two\n');
state = reused('rerun after the edit', state);
// A file the program writes beside its source is no compile input.
writeFileSync(join(work, 'greet.out'), 'written beside the source\n');
check('non-source file beside the entry', run(), 'hello two\n');
state = reused('non-source file beside the entry', state);

appendFileSync(join(root, 'Core/Time/Time.jet'), '\n// run-keep: a Core change\n');
check('Core change', run(), 'hello two\n');
state = rebuilt('Core change', state);

// A new compiler binary in the same place: a new file identity.
rmSync(jet);
copyFileSync(given, jet);
check('compiler change', run(), 'hello two\n');
state = rebuilt('compiler change', state);
check('rerun after the compiler change', run(), 'hello two\n');
state = reused('rerun after the compiler change', state);

// A program that stops: the kept run reports it as the compiling run did.
writeFileSync(file, 'fn run() {\n    print("before")\n    panic("run-keep stop")\n}\n');
const compiled = runRaw();
state = rebuilt('stopping program', state);
const rerun = runRaw();
state = reused('stopping program rerun', state);
if (compiled.status === 0) throw new Error(`stopping program: exit 0\n${compiled.stderr}`);
for (const field of ['status', 'signal', 'stdout', 'stderr']) {
  if (rerun[field] !== compiled[field]) throw new Error(`stopping program: kept run ${field} ${JSON.stringify(rerun[field])}, compiling run ${JSON.stringify(compiled[field])}`);
}
console.log(`stopping program: kept run matches (exit ${compiled.status})`);

// The cap: 64 entry files fill the cache; a rerun of the first marks it used,
// so keeping a 65th drops the second, which compiles again when run.
const capCache = join(scratch, 'cap-cache');
const capKept = join(capCache, 'jet/scripts');
const entry = index => join(work, `cap${index}.jet`);
const capRun = index => check(`cap entry ${index}`, run(entry(index), capCache), `entry ${index}\n`);
const capNames = () => {
  const names = readdirSync(capKept);
  const records = names.filter(name => name.endsWith('.run'));
  const programs = names.filter(name => !name.endsWith('.run'));
  if (records.length !== programs.length) throw new Error(`cap: ${records.length} records beside ${programs.length} programs: ${JSON.stringify(names)}`);
  return programs;
};
for (let index = 0; index <= 64; index++) writeFileSync(entry(index), `fn run() {\n    print("entry ${index}")\n}\n`);
for (let index = 0; index < 64; index++) capRun(index);
if (capNames().length !== 64) throw new Error(`cap: ${capNames().length} programs kept after 64 entries`);
let capBefore = capNames();
capRun(0);
if (capNames().join() !== capBefore.join()) throw new Error('cap: the rerun of entry 0 compiled again');
capRun(64);
if (capNames().length !== 64) throw new Error(`cap: ${capNames().length} programs kept after a 65th entry`);
capBefore = capNames();
capRun(0);
if (capNames().join() !== capBefore.join()) throw new Error('cap: entry 0, the most recently run, was dropped');
capRun(1);
const capAfter = capNames();
if (capAfter.length !== 64 || capAfter.every(name => capBefore.includes(name))) throw new Error('cap: entry 1, the least recently run, was not compiled again');
console.log('cap: 64 programs kept; the least recently run entry compiled again');
console.log('run-keep: ok');
