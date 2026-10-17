#!/usr/bin/env node
// The public no-Cargo entry point. Build the real Jet testkit outside the checkout.
import { spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, mkdtempSync, realpathSync, writeFileSync } from 'node:fs';
import { dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { homedir } from 'node:os';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const args = process.argv.slice(2);
if (!args.length || args[0].startsWith('--') && args[0] !== '--help') args.unshift('all');
const value = name => { const index = args.indexOf(name); return index < 0 ? undefined : args[index + 1]; };
const binary = value('--jet') ?? process.env.JET;
if (!binary) throw Error('JET or --jet must name the admitted compiler binary');
const jet = realpathSync(binary);
const base = resolve(value('--scratch') ?? process.env.JET_TEST_SCRATCH_DIR ?? join(homedir(), '.cache/jet-dev/scratch'));
mkdirSync(base, { recursive: true });
const canonical = realpathSync(base);
const inside = (parent, path) => { const part = relative(parent, path); return part === '' || (!isAbsolute(part) && part !== '..' && !part.startsWith(`..${sep}`)); };
if (inside(realpathSync(repo), canonical) || inside(realpathSync('/tmp'), canonical)) throw Error('testkit scratch must be external and disk-backed');
const scratch = mkdtempSync(join(canonical, 'testkit-build-'));
const project = join(scratch, 'Tools/testkit');
cpSync(join(repo, 'Tools/testkit'), project, { recursive: true, filter: path => !['.jet', 'build', 'node_modules'].includes(path.split(sep).at(-1)) });
const env = { ...process.env, JET: jet, NO_COLOR: '1', TERM: 'dumb', TMPDIR: scratch, TMP: scratch, TEMP: scratch, JET_STORE_DIR: join(scratch, 'store') };
const timeout = Number(process.env.JET_TESTKIT_BUILD_TIMEOUT_MS ?? 600000);
const built = spawnSync(jet, ['build', join(project, 'run.jet'), '--color=never'], { cwd: scratch, env, encoding: 'utf8', timeout, maxBuffer: 128 << 20 });
writeFileSync(join(scratch, 'build.stdout'), built.stdout ?? '');
writeFileSync(join(scratch, 'build.stderr'), built.stderr ?? '');
if (built.error || built.signal || built.status !== 0) {
    process.stderr.write(built.stderr ?? '');
    console.error(`testkit build failed (${built.status ?? built.signal ?? built.error}); evidence: ${scratch}`);
    process.exit(built.status > 0 ? built.status : 2);
}
const reported = (built.stdout ?? '').match(/(?:built|artifact:)\s+([^\n]+)/)?.[1]?.trim();
const candidates = [join(project, '.jet/build/run'), join(scratch, '.jet/build/run'), ...(reported ? [resolve(scratch, reported)] : [])];
const executable = candidates.find(path => existsSync(path));
if (!executable) throw Error(`testkit build published no executable; evidence: ${scratch}`);
const invocation = [...args];
if (!value('--jet')) invocation.push('--jet', jet);
if (!value('--scratch')) invocation.push('--scratch', canonical);
if (!value('--tier')) invocation.push('--tier', 'o0,o1,o2');
const ran = spawnSync(executable, invocation, { cwd: repo, env, stdio: 'inherit' });
if (ran.error || ran.signal) { console.error(`testkit failed: ${ran.error ?? ran.signal}; evidence: ${scratch}`); process.exit(2); }
process.exitCode = ran.status ?? 2;
