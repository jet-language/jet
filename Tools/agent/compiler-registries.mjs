#!/usr/bin/env node
// Canonical compiler-owned registry census and runner. No Cargo, oracle,
// selection filters, skips, or release/Rust emission.
// manifest SHA [--out EXTERNAL_DIR] prints the immutable-source-bound JSON;
// run --jet JET --manifest JSON [--receipt JSON] executes every listed entry.
// BLOCKED entries remain in the manifest and make the receipt fail. Supporting
// files and non-registry test inputs remain visible in inventory, not counted
// as executed registries. A registry runs in place, in package form, inside its Tests package.
import { createHash } from 'node:crypto';
import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, realpathSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { homedir } from 'node:os';
import { prepareTransport, runTransport } from './compiler-registry-transports.mjs';
import { assertPinnedSource } from './proof-source.mjs';
import { partitionRegistries } from './registry-shard.mjs';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const json = (path, value) => writeFileSync(path, JSON.stringify(value, null, 2) + '\n');
const proofPath = join(dirname(repo), 'source.manifest.json');
const currentHead = () => {
    const proof = JSON.parse(readFileSync(proofPath, 'utf8'));
    return assertPinnedSource(repo, proof.commit).commit;
};
function usePinnedSource(head) {
    if (!/^[a-f0-9]{40}$/.test(head ?? '')) throw Error('registry source requires a full committed SHA');
    if (existsSync(proofPath)) {
        assertPinnedSource(repo, head);
        return;
    }
    const source = execFileSync(process.execPath, [join(repo, 'Tools/agent/proof-source.mjs'), head, repo],
        { encoding: 'utf8', maxBuffer: 8 << 20 }).trim();
    assertPinnedSource(source, head);
    const child = spawnSync(process.execPath, [join(source, 'Tools/agent/compiler-registries.mjs'), ...process.argv.slice(2)],
        { cwd: source, env: process.env, stdio: 'inherit' });
    if (child.error) throw child.error;
    process.exit(child.status ?? 1);
}
const usage = 'compiler-registries.mjs manifest SHA [--out DIR]; run --jet JET --manifest JSON [--runtime DIR] [--receipt JSON] [--shard I/N]';
const [command, ...args] = process.argv.slice(2);
const options = {};
if (command === 'manifest') options.head = args.shift();
for (let i = 0; i < args.length; i += 2) {
    const name = args[i];
    if (!['--jet', '--manifest', '--receipt', '--out', '--runtime', '--shard'].includes(name) || !args[i + 1]) throw Error(usage);
    if (name === '--shard' && command !== 'run') throw Error(usage);
    options[name.slice(2)] = name === '--shard' ? args[i + 1] : resolve(args[i + 1]);
}
if (command === 'run' && !options.jet && process.env.JET) options.jet = resolve(process.env.JET);
function filesUnder(directory, extension = '.jet') {
    return readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name)).flatMap(entry => {
        const path = join(directory, entry.name);
        if (entry.isSymbolicLink()) throw Error(`registry inventory must not follow symlinks: ${path}`);
        return entry.isDirectory() ? filesUnder(path, extension) : entry.name.endsWith(extension) && entry.name !== 'package.jet' ? [path] : [];
    });
}
// Mask lexical literals/comments without moving offsets. A Test inside a parser
// input string is not a registry member. Jet supports nested comments, backtick
// literals and triple-quoted foreign/string bodies.
function codeMask(text) {
    let output = '', i = 0;
    const blank = start => { output += text.slice(start, i).replace(/[^\n]/g, ' '); };
    while (i < text.length) {
        const start = i;
        if (text.startsWith('//', i)) { while (i < text.length && text[i] !== '\n') i++; blank(start); }
        else if (text.startsWith('/*', i)) {
            i += 2; let depth = 1;
            while (i < text.length && depth) {
                if (text.startsWith('/*', i)) { depth++; i += 2; }
                else if (text.startsWith('*/', i)) { depth--; i += 2; }
                else i++;
            }
            blank(start);
        } else if (text.startsWith('"""', i)) {
            i += 3; const end = text.indexOf('"""', i); i = end < 0 ? text.length : end + 3; blank(start);
        } else if (text[i] === '"' || text[i] === '`' || text[i] === "'") {
            const quote = text[i++];
            while (i < text.length) {
                if (text[i] === '\\' && quote !== '`') { i += 2; continue; }
                if (text[i++] === quote) break;
            }
            blank(start);
        } else output += text[i++];
    }
    return output;
}
function testsIn(text) {
    const masked = codeMask(text);
    return [...masked.matchAll(/#Test\s*\(/g)].map(match => {
        const rest = text.slice(match.index + match[0].length);
        const name = /^\s*("(?:\\.|[^"\\])*")/.exec(rest);
        if (!name) throw Error('registry Test must have an exact literal name');
        return JSON.parse(name[1]);
    });
}
const stripImports = (text, seen) => text.replace(/^use (?:jet_\w+|compiler_bootstrap)\.(?:\[[^\]]*\]|[A-Za-z_][A-Za-z0-9_]*[ \t]*$)/gm,
    block => block.replace(/[^\n]/g, ' ')).replace(/^use core\.[^\[\n]*$/gm, line => {
        if (seen.has(line.trimEnd())) return line.replace(/[^\n]/g, ' ');
        seen.add(line.trimEnd()); return line;
    });
function inventory() {
    return readdirSync(join(repo, 'Compiler'), { withFileTypes: true }).filter(item => item.isDirectory()).sort((a, b) => a.name.localeCompare(b.name)).flatMap(item => {
        const directory = join(repo, 'Compiler', item.name, 'Tests');
        return existsSync(directory) ? filesUnder(directory).map(path => {
            const source = relative(repo, path), text = readFileSync(path, 'utf8'), masked = codeMask(text);
            const names = testsIn(text), entry = /^fn run\s*\(/m.test(masked);
            const abiRegistry = /Tests\.jet$/.test(source) && /\bfn\s+[A-Za-z_][A-Za-z0-9_]*\s*\(/.test(masked);
            const nestedInput = /\/Tests\/(?:[A-Za-z0-9]+\.fixture\/)?(?:fixtures|Fixtures)\//.test(source);
            return { package: item.name, source, sha256: hash(Buffer.from(text)), names, entry,
                role: !nestedInput && (names.length || entry || abiRegistry) ? 'registry' : 'support-or-fixture' };
        }) : [];
    });
}
function retirements() {
    return readFileSync(join(repo, 'Tools/testkit/retired-with-rust.tsv'), 'utf8').split('\n').flatMap(line => {
        if (!line.trim() || line.startsWith('#')) return [];
        const [path, reason, extra] = line.split('\t');
        if (!path || !reason || extra !== undefined) throw Error('retired rows need an exact source ID and reason');
        return [{ path, reason, ...(existsSync(join(repo, path)) ? { sha256: hash(readFileSync(join(repo, path))) } : {}) }];
    });
}
function registryRows(census) {
    const retired = new Set(retirements().map(row => row.path));
    return census.filter(row => row.role === 'registry' && !retired.has(row.source)).flatMap(row => {
        const id = row.source.replace(/^Compiler\//, '').replace(/\.jet$/, '');
        const mode = row.names.length ? 'test' : 'run';
        return [{ ...row, id, mode }, ...(row.names.length && row.entry ? [{ ...row, id: `${id}.run`, mode: 'run' }] : [])];
    });
}
function testkitInventory() {
    const sources = readdirSync(join(repo, 'Tools/testkit')).filter(name => name.endsWith('.jet')).sort()
        .map(name => `Tools/testkit/${name}`);
    const retirementPath = 'Tools/testkit/retired-with-rust.tsv';
    sources.push('Tools/testkit/package.jet', retirementPath);
    const table = retirements();
    const retiredCases = table.filter(row => row.path.startsWith('tests/cases/') && row.reason.startsWith('Retired with Rust:'));
    const pendingProductInterop = table.filter(row => row.path.startsWith('tests/cases/') && row.reason.startsWith('PRODUCT INTEROP RED:')).map(row => ({ ...row, status: 'RED' }));
    const retiredIds = new Set(retiredCases.map(row => row.path));
    const cases = filesUnder(join(repo, 'tests/cases'), '.case').map(path => relative(repo, path)).filter(path => !retiredIds.has(path));
    const rustToolRequirements = cases.flatMap(path => readFileSync(join(repo, path), 'utf8').split('\n')
        .flatMap((line, index) => /^(?:requires: tool:|when tool |step(?: [^:]+)?: )/.test(line) && /\b(?:cargo|rustc|rustup)\b/.test(line)
            ? [{ path, line: index + 1, source: line }] : []));
    return { sources: sources.map(path => ({ path, sha256: hash(readFileSync(join(repo, path))) })),
        cases: cases.map(path => ({ path, sha256: hash(readFileSync(join(repo, path))) })), retiredCases, pendingProductInterop, rustToolRequirements };
}
function environment() {
    const env = { ...process.env, JET_TOOLCHAIN_ROOT: repo, JET_TEST_JSON: '1', NO_COLOR: '1', TERM: 'dumb' };
    delete env.JET_RUNTIME_PACK;
    for (const name of Object.keys(env)) {
        if (name.startsWith('JET_FRONT_') || name.startsWith('JET_BOOTSTRAP_') || /^(JET_TEST_FILTER|JET_TEST_SHUFFLE_SEED|JET_TEST_SERIAL|JET_TEST_TRACE_TIERS|JET_NATIVE_TEST_ID|JET_NATIVE_TEST_REPORT_PATH|JET_NATIVE_TEST_RECORDS_PATH|JET_UPDATE_SNAPSHOTS|JET_SCHEMA_CACHE_DIR|FX_.*)$/.test(name)) delete env[name];
    }
    return env;
}
function registryEnvironment(entry) {
    const scratch = join(entry.cwd, 'scratch');
    return { TMPDIR: scratch, ...(entry.sources.some(path => readFileSync(join(repo, path), 'utf8').includes('"JET_CLI_FMT_TEST_ROOT"')) ? { JET_CLI_FMT_TEST_ROOT: scratch } : {}) };
}
function nativeRuntime(jet) {
    const runtimeDir = realpathSync(options.runtime ?? join(dirname(jet), 'jet-runtime'));
    const key = readFileSync(join(runtimeDir, 'native.key'), 'utf8').trim();
    if (!key || key === '.' || key === '..' || /[\\/\s]/.test(key)) throw Error('native.key must name one runtime directory');
    const runtimePack = join(runtimeDir, key, 'jet_runtime.pack.o');
    const runtimeObject = join(runtimeDir, key, 'jet_runtime_jet.o');
    const provenance = JSON.parse(readFileSync(join(runtimeDir, 'provenance.json'), 'utf8'));
    if (provenance.backend !== 'jet-native' || provenance.rustObjects !== 0 || provenance.llvmObjects !== 0
        || provenance.packSha256 !== hash(readFileSync(runtimePack)) || provenance.objectSha256 !== hash(readFileSync(runtimeObject))) {
        throw Error('registry transports require matching Jet-native, zero-Rust/LLVM runtime pack/object provenance');
    }
    return { runtimeDir, runtimePack, runtimeObject, packSha256: provenance.packSha256, objectSha256: provenance.objectSha256 };
}
function generate() {
    if (options.head !== currentHead()) throw Error('manifest requires its pinned committed source SHA');
    const out = options.out ?? join(homedir(), '.cache/jet-dev/scratch/RegistryRunnerLead', options.head);
    if (out === repo || out.startsWith(repo + '/')) throw Error('registry outputs belong outside the repository');
    if (existsSync(join(out, 'manifest.json'))) throw Error(`manifest output already sealed: ${out}; choose a fresh --out`);
    mkdirSync(out, { recursive: true });
    const census = inventory(), entries = [];
    const manifests = ['Compiler/JetRuntime/sources.list'].map(path => ({ path, sha256: hash(readFileSync(join(repo, path))) }));
    for (const row of registryRows(census)) {
        const standalone = row.package === 'JetRuntime' && row.mode === 'test';
        if (!standalone) {
            const transport = prepareTransport(row, { repo, out });
            if (transport) { entries.push(transport); continue; }
        }
        const mode = row.mode;
        let reason = null;
        if (!row.entry && !row.names.length) reason = 'hand-ABI fixture has no run or Test entry; its exact native/C witness must be registered';
        if (row.package === 'JetRuntime' && !standalone) reason = 'runtime hand-ABI registries require their native C fixture driver/runtime source assembly; not a compiler Test harness';
        if (row.package === 'JetBackend' && row.entry) reason = 'backend artifact/FFI registry needs its existing run-*.mjs source assembly plus native artifact execution/checker; a returned fn run is not its success verdict';
        const id = row.id;
        const cwd = join(out, id);
        if (!standalone) {
            // Package form: the registry is its own file inside its Tests package, which
            // declares and imports the compiler packages it uses; it runs in place.
            const path = join(repo, row.source);
            const entry = { id, package: row.package, mode, kind: 'package', path, cwd, sha256: hash(readFileSync(path)),
                stdout: mode === 'test' ? 'jet.test.v1' : null, testCount: mode === 'test' ? row.names.length : 1,
                testNames: mode === 'test' ? row.names : [], sources: [row.source], sourceManifests: manifests.map(item => item.path) };
            entry.environment = registryEnvironment(entry);
            entries.push(reason ? { ...entry, status: 'BLOCKED', reason } : entry);
            continue;
        }
        const stdout = 'jet.test.v1';
        const entry = { id, package: row.package, mode, path: join(cwd, 'registry.jet'), cwd,
            sha256: row.sha256, stdout, testCount: row.names.length, sources: [row.source], sourceManifests: manifests.map(item => item.path) };
        entry.environment = registryEnvironment(entry);
        if (reason) { entries.push({ ...entry, status: 'BLOCKED', reason }); continue; }
        const body = `\n// [compiler registry source: ${row.source}]\n` + stripImports(readFileSync(join(repo, row.source), 'utf8'), new Set()) + '\n';
        entry.sha256 = hash(Buffer.from(body));
        entry.testNames = testsIn(body);
        entry.testCount = entry.testNames.length;
        mkdirSync(entry.cwd, { recursive: true });
        writeFileSync(entry.path, body);
        writeFileSync(join(entry.cwd, 'package.jet'), 'name: "compiler_registry"\nversion: "0.0.1"\nedition: "2028"\n');
        entries.push(entry);
    }
    const kit = testkitInventory(), kitDir = join(out, 'testkit');
    mkdirSync(kitDir, { recursive: true });
    for (const source of kit.sources) writeFileSync(join(kitDir, source.path.split('/').at(-1)), readFileSync(join(repo, source.path)));
    entries.push({ id: 'testkit/cases', package: 'testkit', mode: 'run', kind: 'testkit',
        path: join(kitDir, 'run.jet'), cwd: repo, sha256: hash(readFileSync(join(kitDir, 'run.jet'))),
        stdout: JSON.stringify({ PASS: kit.cases.length, FAIL: 0, 'KNOWN-FAILING': 0, 'UNEXPECTED-PASS': 0,
            UNSUPPORTED: 0, 'SKIP-HOST': 0, 'OPT-IN': 0, REFUSED: 0, ERROR: 0, TIMEOUT: 0, suites: ['cases'] }) + '\n',
        testCount: kit.cases.length, sources: kit.sources.map(source => source.path), caseNames: kit.cases.map(source => source.path),
        ...(kit.pendingProductInterop.length || kit.rustToolRequirements.length ? { status: 'BLOCKED',
            reason: kit.pendingProductInterop.length
                ? `${kit.pendingProductInterop.length} mandatory user Rust product cases remain RED; see testkit.pendingProductInterop (not retired or excluded)`
                : `full testkit cases have ${kit.rustToolRequirements.length} active Rust tool requirements/guards; removing Rust would skip required case branches. See testkit.rustToolRequirements for exact paths/lines` } : {}) });
    if (options.head !== currentHead()) throw Error('pinned source changed during assembly');
    const sourceProof = { path: proofPath, sha256: hash(readFileSync(proofPath)), tree: JSON.parse(readFileSync(proofPath, 'utf8')).tree };
    const manifest = { head: options.head, completeCompilerRegistries: true, repo, sourceProof, sourceManifests: manifests,
        inventory: census, testkit: kit, entries,
        retiredWithRust: { testkitCases: kit.retiredCases,
            compilerRegistries: retirements().filter(row => row.path.startsWith('Compiler/')) } };
    json(join(out, 'manifest.json'), manifest);
    console.log(JSON.stringify(manifest, null, 2));
}
async function run() {
    if (!options.jet || !options.manifest) throw Error(usage);
    options.jet = realpathSync(options.jet);
    const bytes = readFileSync(options.manifest), manifest = JSON.parse(bytes);
    if (manifest.head !== currentHead() || manifest.repo !== repo || manifest.sourceProof?.sha256 !== hash(readFileSync(proofPath))
        || manifest.completeCompilerRegistries !== true || !manifest.entries?.length) throw Error('complete pinned-source manifest required');
    // Re-census: a forged/truncated entries array cannot turn omissions into PASS.
    const current = inventory(), required = [...registryRows(current).map(row => row.id), 'testkit/cases'];
    const ids = manifest.entries.map(entry => entry.id);
    if (new Set(ids).size !== ids.length || JSON.stringify(ids.slice().sort()) !== JSON.stringify(required.slice().sort())) throw Error('manifest does not account for every current registry');
    if (JSON.stringify(current) !== JSON.stringify(manifest.inventory)) throw Error('registry source inventory changed since manifest');
    for (const source of manifest.sourceManifests) if (hash(readFileSync(join(repo, source.path))) !== source.sha256) throw Error(`source manifest changed: ${source.path}`);
    if (JSON.stringify(testkitInventory()) !== JSON.stringify(manifest.testkit)) throw Error('testkit sources/cases changed since manifest');
    // Partition only after all full-manifest integrity/completeness checks.
    const selected = partitionRegistries(manifest.entries, options.shard);
    const receiptPath = options.receipt ?? `${options.manifest}.receipt.json`;
    mkdirSync(dirname(receiptPath), { recursive: true });
    const logDir = `${receiptPath}.logs`; mkdirSync(logDir, { recursive: true });
    const receipt = { status: 'FAIL', head: manifest.head, jet: options.jet, jetSha256: hash(readFileSync(options.jet)),
        manifestSha256: hash(bytes), sourceProof: manifest.sourceProof, total: selected.length, passed: 0, blocked: 0, failed: 0, entries: [],
        ...(options.shard ? { shard: options.shard, fullTotal: ids.length } : {}) };
    let runtime, runtimeError;
    if (selected.some(entry => entry.kind === 'transport' && entry.status !== 'BLOCKED')) {
        try { runtime = nativeRuntime(options.jet); receipt.runtime = runtime; }
        catch (error) { runtimeError = error; }
    }
    for (const entry of selected) {
        const result = { id: entry.id, mode: entry.mode, testCount: entry.testCount };
        if (entry.status === 'BLOCKED') { result.status = 'BLOCKED'; result.reason = entry.reason; receipt.blocked++; }
        else if (entry.kind === 'transport' && runtimeError) {
            result.status = 'BLOCKED'; result.reason = `runtime prerequisite: ${runtimeError.message}`; receipt.blocked++;
        }
        else {
            try {
                if (!['run', 'test'].includes(entry.mode) || !Number.isInteger(entry.testCount) || entry.testCount < 1 || hash(readFileSync(entry.path)) !== entry.sha256) throw Error('invalid or changed assembled registry');
                const stem = join(logDir, entry.id.replaceAll('/', '_'));
                let command, child;
                const started = Date.now();
                if (entry.kind === 'testkit') {
                    for (const source of manifest.testkit.sources) {
                        const assembled = join(dirname(entry.path), source.path.split('/').at(-1));
                        if (hash(readFileSync(assembled)) !== source.sha256) throw Error(`assembled testkit source changed: ${source.path}`);
                    }
                    const build = spawnSync(options.jet, ['build', entry.path], { cwd: dirname(entry.path), env: environment(),
                        encoding: 'utf8', maxBuffer: 128 << 20, timeout: Number(process.env.JET_REGISTRY_TIMEOUT_MS ?? 3500000) });
                    writeFileSync(`${stem}.build.stdout`, build.stdout ?? ''); writeFileSync(`${stem}.build.stderr`, build.stderr ?? '');
                    if (build.error || build.status !== 0 || build.signal) throw build.error ?? Error(`testkit native build exit ${build.status}, signal ${build.signal}`);
                    const binary = join(dirname(entry.path), '.jet/build/run'), kitReceipt = `${stem}.results.json`;
                    const scratch = join(dirname(entry.path), 'case-scratch'); mkdirSync(scratch, { recursive: true });
                    command = [binary, 'cases', '--jet', options.jet, '--opt-in', '--scratch', scratch, '--results', kitReceipt];
                    child = spawnSync(command[0], command.slice(1), { cwd: repo, env: environment(), encoding: 'utf8',
                        maxBuffer: 128 << 20, timeout: Number(process.env.JET_REGISTRY_TIMEOUT_MS ?? 3500000) });
                    if (!child.error && child.status === 0) {
                        const report = JSON.parse(readFileSync(kitReceipt, 'utf8'));
                        if (report.jet !== options.jet || report.results?.length !== entry.testCount || report.results.some(row => row.status !== 'PASS')
                            || JSON.stringify(report.results.map(row => row.case).sort()) !== JSON.stringify(entry.caseNames.slice().sort())) {
                            throw Error('testkit cases contain missing, non-PASS, skipped, or unexpected outcomes');
                        }
                        result.report = report;
                    }
                } else if (entry.kind === 'transport') {
                    const ran = await runTransport(entry, { jet: options.jet, repo, logDir, ...runtime });
                    if (ran.testCount !== entry.testCount) throw Error('transport verdict count differs from complete registry');
                    command = ['native-registry-transport', entry.path, entry.id];
                    child = { ...ran, status: ran.code };
                    result.transportEvidence = ran.evidence;
                } else {
                    command = [options.jet, entry.mode, ...(entry.mode === 'test' ? ['--capture=all', '--color=never'] : []), entry.path];
                    if (JSON.stringify(entry.environment) !== JSON.stringify(registryEnvironment(entry))) throw Error('registry environment changed since manifest');
                    mkdirSync(entry.environment.TMPDIR, { recursive: true });
                    result.environment = entry.environment;
                    child = spawnSync(command[0], command.slice(1), { cwd: entry.cwd, env: { ...environment(), ...entry.environment }, encoding: 'utf8',
                        maxBuffer: 128 << 20, timeout: Number(process.env.JET_REGISTRY_TIMEOUT_MS ?? 3500000) });
                }
                writeFileSync(`${stem}.stdout`, child.stdout ?? ''); writeFileSync(`${stem}.stderr`, child.stderr ?? '');
                Object.assign(result, { command, cwd: entry.cwd, code: child.status, signal: child.signal, milliseconds: Date.now() - started, stdoutLog: `${stem}.stdout`, stderrLog: `${stem}.stderr` });
                if (child.error || child.status !== 0 || child.signal) throw child.error ?? Error(`registry exit ${child.status}, signal ${child.signal}`);
                if (entry.mode === 'test') {
                    const reports = (child.stdout ?? '').split('\n').flatMap(line => { try { const value = JSON.parse(line); return value.schema === entry.stdout ? [value] : []; } catch { return []; } });
                    if (reports.length !== 1) throw Error('exact canonical jet.test.v1 success report required');
                    const report = reports[0];
                    if (report.ok !== true || report.failed !== 0 || report.skipped !== 0 || report.unexpectedPasses !== 0 || !Array.isArray(report.tests)) throw Error('test report contains failure, skip, or invalid success fields');
                    if (report.tests.length !== entry.testCount || report.passed + report.expectedFailures !== entry.testCount) throw Error('executed test count does not match registry');
                    if (JSON.stringify(report.tests.map(test => test.name).sort()) !== JSON.stringify(entry.testNames.slice().sort())) throw Error('executed test names do not match complete registry');
                    result.report = report;
                }
                result.status = 'PASS'; receipt.passed++;
            } catch (error) { result.status = 'FAIL'; result.reason = error.message; receipt.failed++; }
        }
        receipt.entries.push(result); json(receiptPath, receipt);
    }
    try { if (manifest.head !== currentHead()) throw Error('source SHA changed'); }
    catch (error) { receipt.failed++; receipt.reason = `pinned source changed during registry run: ${error.message}`; }
    receipt.status = receipt.passed === receipt.total && receipt.blocked === 0 && receipt.failed === 0 ? 'PASS' : 'FAIL';
    json(receiptPath, receipt);
    console.log(`${receipt.status} compiler registries ${receipt.passed}/${receipt.total}; blocked=${receipt.blocked} failed=${receipt.failed}; ${receiptPath}`);
    process.exitCode = receipt.status === 'PASS' ? 0 : 1;
}
usePinnedSource(command === 'manifest' ? options.head : options.manifest ? JSON.parse(readFileSync(options.manifest, 'utf8')).head : null);
if (command === 'manifest') generate();
else if (command === 'run') await run();
else throw Error(usage);
