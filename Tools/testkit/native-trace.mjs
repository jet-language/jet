import { createHash } from 'node:crypto';
import { existsSync, readFileSync, realpathSync } from 'node:fs';
import { basename, dirname, isAbsolute, relative, resolve, sep } from 'node:path';

const REASON = 'user Rust crate via FFI (E0703 class)';
const FORBIDDEN = /^(cargo|rustc|rustup|llc|opt|clang\+\+|clang|ld\.lld)(?:[-.]\d[\w.-]*)?$/;
const CALLS = new Set(['execve', 'execveat', 'clone', 'clone3', 'fork', 'vfork', 'chdir', 'fchdir']);
const inside = (parent, path) => { const part = relative(parent, path); return part === '' || (!isAbsolute(part) && part !== '..' && !part.startsWith(`..${sep}`)); };
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const sorted = object => Object.fromEntries(Object.entries(object).sort(([a], [b]) => a.localeCompare(b, 'en')));
const equalDependencies = (a, b) => JSON.stringify(sorted(a)) === JSON.stringify(sorted(b));

function dependencies(value) {
    if (!value || Array.isArray(value) || typeof value !== 'object') throw Error('dependencies must be a JSON object');
    for (const [name, version] of Object.entries(value)) {
        if (!/^[A-Za-z][A-Za-z0-9_-]*$/.test(name) || typeof version !== 'string' || !version.trim() || /[\r\n\x00]/.test(version)) throw Error('invalid crate name or version');
    }
    return sorted(value);
}

export function ffiCaseCachePath(cacheRoot, casePath, declaredDependencies) {
    const declared = dependencies(declaredDependencies);
    return resolve(cacheRoot, 'native-ffi', hash(`${casePath}\n${JSON.stringify(Object.entries(declared))}`));
}

// Strip Jet comments without treating quoted strings as comments.
function sourceWithoutComments(source) {
    return source.replace(/"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|\/\/[^\n]*|\/\*[\s\S]*?\*\//g, token => token.startsWith('//') || token.startsWith('/*') ? token.replace(/[^\n]/g, ' ') : token);
}

function caseName(name, root) {
    if (typeof name !== 'string' || !name || isAbsolute(name) || name.includes('\\') || name.includes('\0')) throw Error('case must be a source-root-relative path');
    const path = resolve(root, name);
    if (!inside(root, path) || relative(root, path).split(sep).join('/') !== name) throw Error('noncanonical case path');
    if (!inside(root, realpathSync(path))) throw Error('case source escapes source root');
    return name;
}

function caseWords(text) {
    const words = text.match(/"(?:\\.|[^"\\])*"|'[^']*'|[^\s]+/g) ?? [];
    return words.map(word => word.startsWith('"') ? JSON.parse(word) : word.startsWith("'") ? word.slice(1, -1) : word);
}

function caseSourcePaths(name, root) {
    if (name.endsWith('.jet')) return [name];
    if (!name.endsWith('.case')) throw Error(`unsupported user Rust source identity: ${name}`);
    const fixtures = [], commands = [];
    let current = null;
    for (let line of readFileSync(resolve(root, name), 'utf8').split(/\r?\n/)) {
        line = line.replace(/"(?:\\.|[^"\\])*"|'[^']*'|#.*$/g, token => token.startsWith('#') ? '' : token).trim();
        const fixture = /^fixture:\s*(.+)$/.exec(line);
        if (fixture) {
            const words = caseWords(fixture[1]);
            if (words.length !== 1) throw Error(`ambiguous case fixture: ${name}`);
            fixtures.push(words[0]);
        }
        const command = /^(?:step(?:\s+[^:]+)?|spawn\s+[^:]+):\s*(.*)$/.exec(line);
        if (command) {
            current = { argv: caseWords(command[1]), cwd: '' };
            commands.push(current);
        } else if (current && line.startsWith('cwd:')) {
            const words = caseWords(line.slice(4));
            if (words.length !== 1) throw Error(`ambiguous case cwd: ${name}`);
            current.cwd = words[0];
        }
    }
    if (fixtures.length !== 1 || isAbsolute(fixtures[0])) throw Error(`case needs one relative fixture: ${name}`);
    const fixtureRoot = resolve(root, dirname(name), fixtures[0]);
    if (!inside(root, realpathSync(fixtureRoot))) throw Error(`case fixture escapes source root: ${name}`);
    const sources = new Set();
    for (const command of commands) {
        if (command.argv[0] !== 'jet' || !['check', 'run', 'build', 'test'].includes(command.argv[1])) continue;
        const entries = command.argv.slice(2).filter(word => word.endsWith('.jet') && !word.startsWith('-'));
        if (entries.length !== 1 || isAbsolute(command.cwd) || isAbsolute(entries[0])) throw Error(`case needs one literal fixture entry per Jet command: ${name}`);
        const path = resolve(fixtureRoot, command.cwd, entries[0]);
        if (!inside(fixtureRoot, path) || !inside(realpathSync(fixtureRoot), realpathSync(path))) throw Error(`case entry escapes fixture: ${name}`);
        sources.add(caseName(relative(root, path).split(sep).join('/'), root));
    }
    if (!sources.size) throw Error(`case has no compiled fixture entries: ${name}`);
    return [...sources].sort();
}

export function readUserRustAllowlist(text, root) {
    root = realpathSync(root);
    const result = new Map();
    for (const [index, line] of String(text ?? '').split(/\r?\n/).entries()) {
        if (!line.trim() || line.trimStart().startsWith('#')) continue;
        const fields = line.split('\t');
        if (fields.length !== 4 || fields[1] !== REASON) throw Error(`invalid user Rust allowlist row ${index + 1}`);
        const name = caseName(fields[0], root);
        if (result.has(name)) throw Error(`duplicate user Rust case: ${name}`);
        const declared = dependencies(JSON.parse(fields[3]));
        const sources = JSON.parse(fields[2]);
        if (!Array.isArray(sources) || !sources.length || new Set(sources).size !== sources.length) throw Error(`invalid source provenance: ${name}`);
        const expected = caseSourcePaths(name, root);
        const names = sources.map(source => caseName(source, root)).sort();
        if (JSON.stringify(names) !== JSON.stringify(expected)) throw Error(`source provenance differs from case entries: ${name}`);
        const provenance = names.map(path => ({ path, sha256: hash(readFileSync(resolve(root, path))) }));
        let hasExternRust = false;
        const observed = {};
        for (const file of names) {
            const source = sourceWithoutComments(readFileSync(resolve(root, file), 'utf8'));
            for (const match of source.matchAll(/\bextern\s+rust\s+"([^"\n]*)"/g)) {
                hasExternRust = true;
                if (match[1] === 'std') continue;
                const declaration = /^([A-Za-z][A-Za-z0-9_-]*)@([^@\s]+)$/.exec(match[1]);
                if (!declaration) throw Error(`noncanonical extern rust declaration: ${file}`);
                if (Object.hasOwn(observed, declaration[1]) && observed[declaration[1]] !== declaration[2]) throw Error(`conflicting extern rust declaration: ${name}`);
                observed[declaration[1]] = declaration[2];
            }
        }
        if (!hasExternRust) throw Error(`no extern rust declaration: ${name}`);
        if (!equalDependencies(observed, declared)) throw Error(`extern rust declarations differ from allowlist: ${name}`);
        result.set(name, { dependencies: declared, provenance });
    }
    return result;
}

// strace strings use C escapes, not JavaScript escapes. Decode bytes, never eval.
function cString(token) {
    token = token.trim();
    if (!/^"(?:\\.|[^"\\])*"$/.test(token)) throw Error('missing, truncated, or malformed C string');
    const bytes = [];
    for (let i = 1; i < token.length - 1; i++) {
        if (token[i] !== '\\') {
            const point = token.codePointAt(i);
            bytes.push(...Buffer.from(String.fromCodePoint(point)));
            if (point > 0xffff) i++;
            continue;
        }
        const char = token[++i];
        const escapes = { a: 7, b: 8, t: 9, n: 10, v: 11, f: 12, r: 13, '\\': 92, '"': 34 };
        if (Object.hasOwn(escapes, char)) bytes.push(escapes[char]);
        else if (/[0-7]/.test(char)) {
            let octal = char;
            while (octal.length < 3 && /[0-7]/.test(token[i + 1] ?? 'x')) octal += token[++i];
            const byte = parseInt(octal, 8);
            if (byte > 255) throw Error('invalid octal byte');
            bytes.push(byte);
        } else if (char === 'x' && /^[\da-fA-F]{2}$/.test(token.slice(i + 1, i + 3))) {
            bytes.push(parseInt(token.slice(i + 1, i + 3), 16)); i += 2;
        } else throw Error('unsupported C escape');
    }
    const buffer = Buffer.from(bytes);
    const value = buffer.toString('utf8');
    if (!Buffer.from(value).equals(buffer) || value.includes('\0')) throw Error('non-UTF8 or NUL C string');
    return value;
}

function splitArguments(text) {
    const output = [];
    let start = 0, quote = false, escape = false;
    const stack = [];
    for (let i = 0; i < text.length; i++) {
        const char = text[i];
        if (quote) { if (escape) escape = false; else if (char === '\\') escape = true; else if (char === '"') quote = false; continue; }
        if (char === '"') quote = true;
        else if ('([{'.includes(char)) stack.push(char);
        else if (')]}'.includes(char)) {
            if (stack.pop() !== { ')': '(', ']': '[', '}': '{' }[char]) throw Error('unbalanced syscall arguments');
        } else if (char === ',' && !stack.length) { output.push(text.slice(start, i).trim()); start = i + 1; }
    }
    if (quote || stack.length) throw Error('unterminated syscall arguments');
    output.push(text.slice(start).trim());
    return output;
}

function stringArray(token) {
    if (!token?.startsWith('[') || !token.endsWith(']')) throw Error('missing or abbreviated argv');
    const body = token.slice(1, -1).trim();
    return body ? splitArguments(body).map(cString) : [];
}

function parseTrace(text, violations) {
    const events = [], pending = new Map();
    for (const [lineIndex, original] of String(text ?? '').split(/\r?\n/).entries()) {
        if (!original.trim()) continue;
        let line = original.trim(), pid = 0;
        const prefix = /^(?:\[pid\s+(\d+)\]\s*|(\d+)\s+)(?=\d{2}:|\d+\.\d+\s|[a-zA-Z_<+*-])/.exec(line);
        if (prefix) { pid = Number(prefix[1] ?? prefix[2]); line = line.slice(prefix[0].length); }
        line = line.replace(/^(?:\d{2}:\d{2}:\d{2}(?:\.\d+)?|\d+\.\d+)\s+/, '');
        const index = lineIndex + 1;
        if (/^(?:\+\+\+|---|strace: Process \d+ attached)/.test(line)) continue;
        if (line.startsWith('strace:')) { violations.push({ code: 'trace-diagnostic', line: index, detail: line }); continue; }
        const resumed = /^<\.\.\. (\w+) resumed>(.*)$/.exec(line);
        let order = index;
        if (resumed) {
            const unfinished = pending.get(pid);
            if (!unfinished || unfinished.call !== resumed[1]) { violations.push({ code: 'unmatched-resumed', pid, line: index, detail: line }); continue; }
            pending.delete(pid); line = unfinished.text + resumed[2]; order = unfinished.order;
        }
        const unfinishedAt = line.indexOf('<unfinished ...>');
        if (unfinishedAt >= 0) {
            const call = /^(\w+)\(/.exec(line)?.[1];
            if (!CALLS.has(call) || pending.has(pid)) violations.push({ code: 'ambiguous-unfinished', pid, line: index, detail: line });
            else pending.set(pid, { call, text: line.slice(0, unfinishedAt), order });
            continue;
        }
        const complete = /^(\w+)\(([\s\S]*)\)\s+=\s+(.+)$/.exec(line);
        if (!complete || !CALLS.has(complete[1])) { violations.push({ code: 'unparsed-trace', pid, line: index, detail: line }); continue; }
        try {
            events.push({ pid, call: complete[1], args: splitArguments(complete[2]), result: complete[3], line: index, order });
        } catch (error) { violations.push({ code: 'ambiguous-syscall', pid, line: index, detail: error.message }); }
    }
    for (const [pid, event] of pending) violations.push({ code: 'incomplete-syscall', pid, line: event.order, detail: event.text });
    return events;
}

function canonical(path, cwd) {
    if (!isAbsolute(path) && !cwd) throw Error('relative path with unknown cwd');
    const absolute = resolve(cwd ?? '/', path);
    return existsSync(absolute) ? realpathSync(absolute) : absolute;
}

function executable(event, cwd) {
    const offset = event.call === 'execveat' ? 1 : 0;
    const raw = cString(event.args[offset]);
    if (!raw && (event.call !== 'execveat' || event.args[0].startsWith('AT_FDCWD'))) throw Error('empty executable path without a decoded executable fd');
    let path;
    if (event.call === 'execveat' && !isAbsolute(raw) && !event.args[0].startsWith('AT_FDCWD')) {
        const fd = /^\d+<([^>]+)>$/.exec(event.args[0]);
        if (!fd || !isAbsolute(fd[1]) || fd[1].endsWith(' (deleted)')) throw Error('unknown execveat directory fd');
        path = canonical(raw || fd[1], raw ? fd[1] : '/');
        if (!raw && !event.args[4]?.includes('AT_EMPTY_PATH')) throw Error('empty execveat without AT_EMPTY_PATH');
    } else path = canonical(raw, cwd);
    const names = [basename(raw), basename(path)];
    const kinds = [...new Set(names.map(name => FORBIDDEN.exec(name)?.[1]).filter(Boolean))];
    if (kinds.length > 1) throw Error('conflicting forbidden basename and canonical executable identity');
    return { path, raw, kind: kinds[0], argv: stringArray(event.args[offset + 1]), env: event.args[offset + 2] };
}

function nodeScript(exec, cwd) {
    if (!/^node(?:js)?(?:[-.]\d[\w.-]*)?$/.test(basename(exec.path))) return null;
    // Only an actual Node script operand is a launcher, never text inside -e/-p.
    let index = 1;
    while (exec.argv[index]?.startsWith('--') && exec.argv[index] !== '--') {
        if (!/^(?:--no-warnings|--enable-source-maps|--experimental-[\w-]+)$/.test(exec.argv[index])) return null;
        index++;
    }
    if (exec.argv[index] === '--') index++;
    if (!exec.argv[index] || exec.argv[index].startsWith('-')) return null;
    return { path: canonical(exec.argv[index], cwd), args: exec.argv.slice(index + 1) };
}

export function ffiRunCachePath(context, casePath, declared) {
    const expected = context.ffiCache ? resolve(context.cwd, context.ffiCache) : ffiCaseCachePath(context.cacheRoot, casePath, declared);
    const cache = canonical(expected);
    if (cache !== expected || inside(context.root, cache) || inside(cache, context.root)) throw Error('foreign cache aliases or overlaps source root');
    if (!context.ffiCache && !inside(context.cacheRoot, cache)) throw Error('case cache escapes the exact external digest namespace');
    return cache;
}

function sandboxMounts(exec, cwd, context, declared) {
    if (exec.argv.filter(arg => arg === '--clearenv').length !== 1) throw Error('foreign sandbox must clear its environment');
    const mounts = [], directories = [];
    for (let i = 1; i < exec.argv.length; i++) {
        const arg = exec.argv[i];
        if (arg === '--') break;
        if (['--ro-bind', '--ro-bind-try', '--bind', '--bind-try'].includes(arg)) {
            if (!exec.argv[i + 1] || !exec.argv[i + 2]) throw Error('incomplete sandbox bind');
            const host = canonical(exec.argv[++i], cwd), path = exec.argv[++i];
            mounts.push({ host, path, readonly: arg.startsWith('--ro-') });
        } else if (arg === '--chdir') {
            directories.push(exec.argv[++i]);
        } else if (['--tmpfs', '--proc', '--dev'].includes(arg)) {
            const path = exec.argv[++i];
            if (inside('/work/source', path) || inside(path, '/work/source') || inside('/work/output', path) || inside(path, '/work/output')) throw Error('sandbox overlay obscures helper mounts');
        }
    }
    const source = mounts.filter(mount => mount.path === '/work/source');
    const output = mounts.filter(mount => mount.path === '/work/output');
    if (source.length !== 1 || !source[0].readonly || output.length !== 1 || output[0].readonly || directories.length !== 1 || directories[0] !== '/work/source') throw Error('foreign sandbox source/output/chdir differs from generated helper contract');
    const cache = ffiRunCachePath(context, context.case, declared);
    const workspace = relative(cache, dirname(source[0].host)).split(sep).join('/');
    if (!/^[0-9a-f]{64}\/\.trace-workspaces\/\d+-\d+$/.test(workspace) || basename(source[0].host) !== 'source' || output[0].host !== resolve(dirname(source[0].host), 'output')) throw Error('sandbox helper is outside the retained full-key workspace contract');
    for (const mount of [source[0], output[0]]) if (!inside(cache, mount.host)) throw Error('sandbox helper mount is outside the actual foreign cache');
    for (const mount of mounts) {
        if (mount === source[0] || mount === output[0]) continue;
        if (inside('/work/source', mount.path) || inside(mount.path, '/work/source') || inside('/work/output/metadata-source', mount.path) || inside(mount.path, '/work/output/metadata-source')) throw Error('sandbox bind obscures generated helper source');
    }
    return { source: source[0].host, output: output[0].host };
}

function sandboxPath(path, cwd, sandbox) {
    const absolute = resolve(cwd ?? '/', path);
    if (!sandbox) return canonical(absolute);
    for (const [virtual, host] of [['/work/source', sandbox.source], ['/work/output', sandbox.output]]) {
        if (inside(virtual, absolute)) return canonical(resolve(host, relative(virtual, absolute)));
    }
    throw Error('foreign sandbox manifest is outside its source/output mounts');
}

function marker(script, root, jet) {
    const args = script.args, divider = args.indexOf('--');
    if (divider < 0 || !args.slice(divider + 1).length) throw Error('marker has no command');
    const values = new Map();
    for (let i = 0; i < divider; i += 2) {
        if (!['--case', '--root', '--cwd', '--jet', '--cache-root', '--ffi-cache'].includes(args[i]) || args[i + 1] === undefined || args[i] !== '--ffi-cache' && !args[i + 1] || values.has(args[i])) throw Error('invalid marker arguments');
        values.set(args[i], args[i + 1]);
    }
    if (values.size !== 6) throw Error('incomplete marker');
    for (const key of ['--root', '--cwd', '--jet', '--cache-root']) if (!isAbsolute(values.get(key))) throw Error(`marker ${key} must be absolute`);
    if (realpathSync(values.get('--root')) !== root || realpathSync(values.get('--jet')) !== jet) throw Error('marker root or jet differs from admitted identity');
    // The immutable wrapper validates cwd before exec; completed cases can
    // have removed their scratch directory by the final outer-trace audit.
    const cwd = canonical(values.get('--cwd'));
    const cacheRoot = realpathSync(values.get('--cache-root'));
    if (inside(root, cacheRoot) || inside(cacheRoot, root)) throw Error('marker cache must be an external namespace');
    return { case: caseName(values.get('--case'), root), root, cwd, cacheRoot, ffiCache: values.get('--ffi-cache'), command: args.slice(divider + 1) };
}

// A deliberately small, fail-closed TOML subset matching generated FFI helpers.
function manifestDependencies(contents, path, cache) {
    let section = '', sawPackage = false, sawDependencies = false;
    const declared = {}, seen = new Set();
    // The native planner's helper is its own empty workspace root and records
    // its artifact mode (Compiler/JetDriver RustForeign.jet manifest); a
    // workspace key such as `members` remains an unsupported manifest key.
    const ordinary = {
        package: new Set(['name', 'version', 'edition', 'publish', 'build', 'autobins', 'autoexamples', 'autotests', 'autobenches']),
        'package.metadata.jet': new Set(['native-helper-artifact-mode']),
        workspace: new Set(),
        lib: new Set(['name', 'path', 'crate-type']),
        'profile.release': new Set(['opt-level', 'lto', 'codegen-units', 'panic', 'strip', 'debug', 'overflow-checks']),
        'profile.dev': new Set(['opt-level', 'debug', 'overflow-checks']),
    };
    for (let line of contents.split(/\r?\n/)) {
        line = line.replace(/"(?:\\.|[^"\\])*"|'[^']*'|#.*$/g, token => token.startsWith('#') ? '' : token).trim();
        if (!line) continue;
        const header = /^\[([\w.-]+)\]$/.exec(line);
        if (header) {
            section = header[1];
            if (seen.has(`[${section}]`) || section !== 'dependencies' && !Object.hasOwn(ordinary, section)) throw Error('unsupported or duplicate manifest section');
            seen.add(`[${section}]`); sawPackage ||= section === 'package'; sawDependencies ||= section === 'dependencies'; continue;
        }
        const assignment = /^(?:([A-Za-z][\w-]*)|"([A-Za-z][\w-]*)")\s*=\s*(.+)$/.exec(line);
        if (!assignment) throw Error('unsupported manifest syntax');
        const key = assignment[1] ?? assignment[2], value = assignment[3];
        if (seen.has(`${section}.${key}`)) throw Error('duplicate manifest key');
        seen.add(`${section}.${key}`);
        if (section === 'dependencies') {
            const inline = /^\{\s*version\s*=\s*("(?:\\.|[^"\\])*"|'[^']*')\s*,?\s*\}$/.exec(value);
            const version = inline ? inline[1] : value;
            if (!/^(?:"[^"\\]*"|'[^']*')$/.test(version)) throw Error('dependency must be a version string or version-only table');
            declared[key] = version.slice(1, -1);
        } else {
            if (!ordinary[section]?.has(key)) throw Error('unsupported manifest key');
            if (key === 'build' && value !== 'false') throw Error('manifest build script is forbidden');
            if (key === 'path') {
                if (!/^"[^"\\]*"$/.test(value)) throw Error('unsupported library path');
                const library = realpathSync(resolve(dirname(path), value.slice(1, -1)));
                if (!inside(cache, library)) throw Error('library source escapes case cache');
            } else if (!/^(?:"[^"\\]*"|'[^']*'|true|false|\d+|\[\s*(?:"[\w-]+"\s*,?\s*)*\])$/.test(value)) throw Error('unsupported manifest value');
        }
    }
    if (!sawPackage || !sawDependencies || !seen.has('package.name') || !seen.has('package.version')) throw Error('missing package or dependencies');
    if (existsSync(resolve(dirname(path), 'build.rs'))) throw Error('implicit build script is forbidden');
    return dependencies(declared);
}

function cargoManifest(exec, cwd, context, declared, sandbox) {
    const manifestArgs = [];
    for (let i = 1; i < exec.argv.length; i++) {
        if (exec.argv[i] === '--manifest-path') {
            if (!exec.argv[i + 1]) throw Error('missing manifest path');
            manifestArgs.push(exec.argv[++i]);
        } else if (exec.argv[i].startsWith('--manifest-path=')) manifestArgs.push(exec.argv[i].slice(16));
        else if (/^(?:--config|--manifest-path)/.test(exec.argv[i])) throw Error('unsupported Cargo manifest/config override');
    }
    if (manifestArgs.length > 1) throw Error('ambiguous Cargo manifest path');
    if (!['build', 'check', 'rustc', 'metadata'].includes(exec.argv[1])) throw Error('Cargo command is not a foreign build');
    if (exec.argv[1] === 'metadata' && (manifestArgs.length !== 1 || exec.argv[2] !== '--format-version' || exec.argv[3] !== '1' || !(exec.argv.length === 6 && exec.argv[4] === '--manifest-path' || exec.argv.length === 5 && exec.argv[4].startsWith('--manifest-path=')))) throw Error('only exact generated-helper metadata format-version 1 is admitted');
    if (exec.argv.includes('--') || exec.argv.some(arg => /^--(?:workspace|all|package|exclude|example|test|bench|bin)(?:=|$)/.test(arg)) || exec.argv.some(arg => /^-(?:p|C)/.test(arg))) throw Error('Cargo target/workspace/directory override');
    const cache = ffiRunCachePath(context, context.case, declared);
    const path = realpathSync(sandboxPath(manifestArgs[0] ?? 'Cargo.toml', cwd, sandbox));
    if (!inside(cache, path)) throw Error('manifest is outside the actual foreign cache');
    const bytes = readFileSync(path), contents = bytes.toString('utf8');
    if (!Buffer.from(contents).equals(bytes)) throw Error('manifest is not UTF8');
    if (!equalDependencies(manifestDependencies(contents, path, cache), declared)) throw Error('manifest dependencies differ from declared user crates');
    if (!sandbox && exec.env?.startsWith('[')) {
        const environment = stringArray(exec.env).filter(value => value.startsWith('JET_FFI_CACHE_DIR='));
        if (environment.length !== 1 || canonical(environment[0].slice(18), context.cwd) !== cache) throw Error('JET_FFI_CACHE_DIR differs from case cache');
    }
    return { manifestPath: path, manifestSha256: hash(bytes), manifestContents: contents, cachePath: cache, cacheSource: context.ffiCache ? 'explicit' : 'case-digest', dependencies: declared, command: exec.argv[1], sandbox: sandbox ?? null };
}

export function auditNativeTrace({ text, root, jet, allowlistText = '', tracePath = null }) {
    const receipt = { schema: 'jet-native-case-trace/v1', status: 'FAIL', cases: [], zeroRustCases: [], allowedFfiBuilds: [], allowedFfiQueries: [], excludedPerformancePeers: [], violations: [], events: 0, failedExecAttempts: [], tracePath };
    let allowlist;
    try { root = realpathSync(root); jet = realpathSync(jet); allowlist = readUserRustAllowlist(allowlistText, root); }
    catch (error) { receipt.violations.push({ code: 'invalid-audit-input', detail: error.message }); return receipt; }
    const events = parseTrace(text, receipt.violations); receipt.events = events.length;
    if (!events.length) receipt.violations.push({ code: 'empty-trace', detail: 'no reconstructable syscalls' });
    const processes = new Map(), cases = new Map(), rustCases = new Set(), births = new Map();
    const pendingExecCwds = [];
    const forks = events.filter(event => ['clone', 'clone3', 'fork', 'vfork'].includes(event.call) && /^\d+\b/.test(event.result) && Number.parseInt(event.result) > 0);
    // strace can print a child's syscall before the parent's completed fork line.
    // Move only the birth record back; unfinished calls already carry entry order.
    for (const event of forks) {
        const child = Number.parseInt(event.result);
        if (births.has(child)) receipt.violations.push({ code: 'ambiguous-pid-reuse', pid: child, line: event.line });
        births.set(child, event);
    }
    const first = new Map();
    for (const event of events) first.set(event.pid, Math.min(first.get(event.pid) ?? Infinity, event.order));
    for (let pass = 0; pass <= forks.length; pass++) {
        let changed = false;
        for (const event of forks) {
            const child = Number.parseInt(event.result), target = (first.get(child) ?? event.order + 1) - 0.001;
            if (target < event.order) { event.order = target; first.set(event.pid, Math.min(first.get(event.pid) ?? Infinity, target)); changed = true; }
        }
        if (!changed) break;
        if (pass === forks.length) receipt.violations.push({ code: 'cyclic-process-ancestry' });
    }
    events.sort((a, b) => a.order - b.order || a.line - b.line);
    const process = pid => {
        if (!processes.has(pid)) processes.set(pid, { fs: { cwd: null }, history: [], context: null, approvedCargo: null, approvedRustc: null, sandbox: null });
        return processes.get(pid);
    };
    const violate = (event, state, code, detail, exec = null) => receipt.violations.push({ code, pid: event.pid, line: event.line, case: state.context?.case ?? null, executable: exec?.path ?? null, detail, ancestry: state.history });
    for (const event of events) {
        const state = process(event.pid);
        const result = /^(-?\d+)\b/.exec(event.result);
        if (['clone', 'clone3', 'fork', 'vfork'].includes(event.call)) {
            if (result && Number(result[1]) > 0) {
                const child = Number(result[1]);
                if (processes.has(child)) violate(event, state, 'ambiguous-birth', 'child state existed before reconstructed birth');
                processes.set(child, { fs: /\bCLONE_FS\b/.test(event.args.join(',')) ? state.fs : { ...state.fs }, history: [...state.history], context: state.context, approvedCargo: state.approvedCargo, approvedRustc: state.approvedRustc, sandbox: state.sandbox });
            } else if (!result) violate(event, state, 'ambiguous-fork', event.result);
            continue;
        }
        if (event.call === 'chdir' || event.call === 'fchdir') {
            if (result?.[1] === '0') {
                try {
                    for (const pending of pendingExecCwds) {
                        if (pending.fs === state.fs && event.order < pending.until) violate(event, state, 'ambiguous-shared-cwd', `cwd mutation overlaps unfinished exec in pid ${pending.pid}`);
                    }
                    state.fs.uncertainUntil = Math.max(state.fs.uncertainUntil ?? 0, event.line);
                    if (event.call === 'chdir') state.fs.cwd = canonical(cString(event.args[0]), state.fs.cwd);
                    else {
                        const fd = /^\d+<([^>]+)>$/.exec(event.args[0]);
                        if (!fd || !isAbsolute(fd[1]) || fd[1].endsWith(' (deleted)')) throw Error('fchdir requires -yy absolute fd path');
                        state.fs.cwd = canonical(fd[1]);
                    }
                } catch (error) { state.fs.cwd = null; violate(event, state, 'ambiguous-cwd', error.message); }
            } else if (!result) violate(event, state, 'ambiguous-cwd-result', event.result);
            continue;
        }
        let exec;
        try { exec = executable(event, state.fs.cwd); }
        catch (error) {
            if (result && Number(result[1]) < 0) { receipt.failedExecAttempts.push({ pid: event.pid, line: event.line, detail: error.message, result: event.result }); continue; }
            violate(event, state, 'ambiguous-execution', error.message); continue;
        }
        if (result && Number(result[1]) < 0) {
            if (exec.kind) receipt.failedExecAttempts.push({ pid: event.pid, line: event.line, executable: exec.path, result: event.result });
            continue;
        }
        if (!result || result[1] !== '0') { violate(event, state, 'ambiguous-exec-result', event.result, exec); continue; }
        if (event.order < (state.fs.uncertainUntil ?? 0)) violate(event, state, 'ambiguous-shared-cwd', 'execution overlaps unfinished shared cwd mutation', exec);
        if (event.order < event.line) pendingExecCwds.push({ fs: state.fs, until: event.line, pid: event.pid });
        // exec unshares a CLONE_FS cwd; fork/thread history remains immutable.
        state.fs = { ...state.fs };
        let script = null;
        try {
            script = nodeScript(exec, state.fs.cwd);
            if (script && basename(script.path) === 'native-command.mjs') {
                state.context = null; state.approvedCargo = null; state.approvedRustc = null; state.sandbox = null;
                if (script.path !== realpathSync(resolve(root, 'Tools/testkit/native-command.mjs'))) throw Error('marker script is not the immutable source-root wrapper');
                state.context = { ...marker(script, root, jet), markerLine: event.line, markerPid: event.pid };
                // The immutable wrapper promises this cwd before its command exec.
                state.fs.cwd = state.context.cwd;
                if (!cases.has(state.context.case)) cases.set(state.context.case, { case: state.context.case, markerPids: [], executions: 0 });
                cases.get(state.context.case).markerPids.push(event.pid);
            }
        } catch (error) { violate(event, state, 'invalid-marker-or-launcher', error.message, exec); }
        if (state.context) cases.get(state.context.case).executions++;
        const entry = { pid: event.pid, executable: exec.path, argv: exec.argv, case: state.context?.case ?? null, markerLine: state.context?.markerLine ?? null };
        if (exec.path === jet) {
            state.approvedCargo = null; state.approvedRustc = null; state.sandbox = null;
            entry.admittedJet = true;
            entry.foreignBuild = !!state.context && ['build', 'run', 'check', 'test'].includes(exec.argv[1]) && !exec.argv.some((arg, index) => /^--(?:emit|rust)(?:=|$)/.test(arg) || /^(?:--backend|--codegen)=rust$/.test(arg) || ['--backend', '--codegen'].includes(arg) && exec.argv[index + 1] === 'rust');
        }
        if (basename(exec.path) === 'bwrap' && exec.argv.includes('/work/source')) {
            try {
                const declared = state.context && allowlist.get(state.context.case)?.dependencies;
                const latest = state.history.at(-1);
                if (!declared || !latest?.admittedJet || !latest.foreignBuild || latest.case !== state.context.case || latest.markerLine !== state.context.markerLine) throw Error('sandbox helper requires the same admitted Jet foreign branch');
                state.sandbox = { ...sandboxMounts(exec, state.fs.cwd, state.context, declared), pid: event.pid };
            } catch (error) {
                state.sandbox = null;
                violate(event, state, 'invalid-foreign-sandbox', error.message, exec);
            }
        }
        if (exec.kind) {
            if (state.context) rustCases.add(state.context.case);
            const declaration = state.context && allowlist.get(state.context.case);
            const declared = declaration?.dependencies;
            try {
                if (exec.kind === 'cargo') {
                    const admitted = [...state.history].reverse().find(item => item.admittedJet);
                    if (!state.context || !declared || !admitted?.foreignBuild || admitted.case !== state.context.case || admitted.markerLine !== state.context.markerLine) throw Error('Cargo requires allowlisted case and admitted Jet foreign-build ancestry in the same marker branch');
                    const manifest = cargoManifest(exec, state.fs.cwd, state.context, declared, state.sandbox);
                    const build = { case: state.context.case, reason: REASON, provenance: declaration.provenance, ...manifest, cargoPid: event.pid, jetPid: admitted.pid, pids: [event.pid], ancestry: [...state.history, entry], executions: [{ ...entry, ancestry: [...state.history] }] };
                    receipt.allowedFfiBuilds.push(build); state.approvedCargo = build; state.approvedRustc = null;
                } else if (exec.kind === 'rustc') {
                    const latest = state.history.at(-1);
                    if (state.approvedCargo && latest?.executable !== jet) {
                        const cargoAt = state.history.findLastIndex(item => item.executable && FORBIDDEN.exec(basename(item.executable))?.[1] === 'cargo');
                        if (cargoAt < 0 || state.history.slice(cargoAt + 1).some(item => item.admittedJet)) throw Error('Rustc is not below approved Cargo foreign build');
                        state.approvedRustc = { pid: event.pid, build: state.approvedCargo };
                        if (!state.approvedCargo.pids.includes(event.pid)) state.approvedCargo.pids.push(event.pid);
                        state.approvedCargo.executions.push({ ...entry, ancestry: [...state.history] });
                    } else if (declared && latest?.admittedJet && latest.foreignBuild && latest.case === state.context.case && latest.markerLine === state.context.markerLine && exec.argv.length === 2 && exec.argv[1] === '-vV') {
                        const cachePath = ffiRunCachePath(state.context, state.context.case, declared);
                        receipt.allowedFfiQueries.push({ case: state.context.case, kind: 'rustc-toolchain-identity', dependencies: declared, provenance: declaration.provenance, cachePath, cacheSource: state.context.ffiCache ? 'explicit' : 'case-digest', pid: event.pid, jetPid: latest.pid, argv: exec.argv, ancestry: [...state.history, entry] });
                    } else if (latest?.performancePeer) {
                        receipt.excludedPerformancePeers.push({ pid: event.pid, case: state.context?.case ?? null, executable: exec.path, launcher: latest.executable, script: latest.performancePeer, ancestry: [...state.history, entry], reason: 'direct Rust performance peer' });
                    } else throw Error('Rustc is not a declared Cargo foreign build or direct performance peer');
                } else if (exec.kind === 'ld.lld' && state.approvedRustc) {
                    if (!state.approvedRustc.build.pids.includes(event.pid)) state.approvedRustc.build.pids.push(event.pid);
                    state.approvedRustc.build.executions.push({ ...entry, ancestry: [...state.history] });
                } else throw Error(`${exec.kind} is forbidden outside the constrained Rust FFI branch`);
            } catch (error) { state.approvedCargo = null; state.approvedRustc = null; violate(event, state, 'forbidden-execution', error.message, exec); }
        }
        if (script && existsSync(script.path) && (script.path === resolve(root, 'Tools/ci/compiled-workload-runner.mjs') || inside(resolve(root, 'Tools/perf'), script.path) && script.path.endsWith('.mjs'))) entry.performancePeer = script.path;
        state.history = [...state.history, entry];
    }
    receipt.cases = [...cases.values()].sort((a, b) => a.case.localeCompare(b.case));
    receipt.zeroRustCases = receipt.cases.map(entry => entry.case).filter(name => !rustCases.has(name));
    receipt.status = receipt.violations.length ? 'FAIL' : 'PASS';
    return receipt;
}
