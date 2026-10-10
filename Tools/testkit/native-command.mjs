#!/usr/bin/env node
// An exec-only case marker for the single outer native-suite strace. Replacing
// this process preserves the command PID, process group, stdio and exit status.
import { accessSync, constants, mkdirSync, readFileSync, realpathSync } from 'node:fs';
import { delimiter, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { ffiCaseCachePath, readUserRustAllowlist } from './native-trace.mjs';

const args = process.argv.slice(2);
const boundary = args.indexOf('--');
if (boundary < 0 || boundary === args.length - 1 || boundary % 2) throw Error('native command requires case/root/cwd/jet/cache-root and -- COMMAND');
const options = new Map();
for (let i = 0; i < boundary; i += 2) {
    if (!['--case', '--root', '--cwd', '--jet', '--cache-root'].includes(args[i]) || options.has(args[i])) throw Error(`invalid native marker option ${args[i]}`);
    options.set(args[i], args[i + 1]);
}
for (const name of ['--case', '--root', '--cwd', '--jet', '--cache-root']) if (!options.get(name)) throw Error(`missing native marker ${name}`);
const root = realpathSync(options.get('--root'));
const cwd = realpathSync(options.get('--cwd'));
const cacheRoot = resolve(options.get('--cache-root'));
const inside = (parent, path) => { const part = relative(parent, path); return part === '' || (!isAbsolute(part) && part !== '..' && !part.startsWith(`..${sep}`)); };
mkdirSync(cacheRoot, { recursive: true });
const canonicalCache = realpathSync(cacheRoot);
if (inside(root, canonicalCache) || inside(realpathSync('/tmp'), canonicalCache)) throw Error('native foreign cache must be external and disk-backed');
const declarations = readUserRustAllowlist(readFileSync(join(root, 'Tools/testkit/user-rust-ffi.allow'), 'utf8'), root);
const dependencies = declarations.get(options.get('--case'));
const env = { ...process.env };
if (dependencies) {
    env.JET_FFI_CACHE_DIR = ffiCaseCachePath(canonicalCache, options.get('--case'), dependencies);
    mkdirSync(env.JET_FFI_CACHE_DIR, { recursive: true });
}
process.chdir(cwd);
const argv = args.slice(boundary + 1);
let executable = argv[0];
if (!executable.includes('/')) {
    executable = (env.PATH ?? '/bin:/usr/bin').split(delimiter).map(path => resolve(cwd, path, argv[0])).find(path => {
        try { accessSync(path, constants.X_OK); return true; } catch { return false; }
    });
    if (!executable) throw Error(`native command executable not found: ${argv[0]}`);
} else executable = resolve(cwd, executable);
executable = realpathSync(executable);
if (typeof process.execve !== 'function') throw Error('native case tracing requires Node with process.execve (22.15 or newer)');
process.execve(executable, argv, env);
