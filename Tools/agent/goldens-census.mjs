#!/usr/bin/env node
// Collect-all closeout against one selected compiler. No Cargo, blessing or build of Jet.
import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { appendFileSync, closeSync, cpSync, createWriteStream, existsSync, mkdirSync, mkdtempSync, openSync, readFileSync, readSync, readdirSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { partitionRegistries } from './registry-shard.mjs';

const here = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const args = process.argv.slice(2), options = { jobs: 2, timeout: 90, registryTimeout: 600, tiers: 'o0,o1,o2' };
const flags = new Map([['--jobs', 'jobs'], ['--timeout', 'timeout'], ['--registry-timeout', 'registryTimeout'], ['--tier', 'tiers'], ['--scratch', 'scratch'], ['--runtime', 'runtime'], ['--roots', 'roots']]);
for (let i = 0; i < args.length; i += 2) {
    if (!flags.has(args[i]) || !args[i + 1]) throw Error('usage: goldens-census.mjs [--jobs N] [--timeout SEC] [--registry-timeout SEC] [--tier o0,o1,o2] [--scratch DIR] [--runtime DIR] [--roots JSON]');
    options[flags.get(args[i])] = args[i + 1];
}
for (const name of ['jobs', 'timeout', 'registryTimeout']) {
    options[name] = Number(options[name]);
    if (!Number.isSafeInteger(options[name]) || options[name] <= 0) throw Error(`${name} must be a positive integer`);
}
if (options.jobs > 8) throw Error('at most eight census workers');
if (!options.tiers.split(',').every(tier => ['o0', 'o1', 'o2'].includes(tier))) throw Error('invalid testkit tier');
if (!process.env.JET) throw Error('JET must select an already-built compiler');
const jet = realpathSync(process.env.JET), source = realpathSync(process.env.JET_TOOLCHAIN_ROOT ?? here);
// The enclosing cgroup, not a separate limit per shard, owns the TOTAL budget.
if (process.env.JET_CENSUS_SCOPED !== '1') {
    const available = Number(/^MemAvailable:\s+(\d+)\s+kB$/m.exec(readFileSync('/proc/meminfo', 'utf8'))?.[1] ?? 0) * 1024;
    if (available < 12 * 1024 ** 3) throw Error('census launch refused: MemAvailable is below 12 GiB');
    const budget = process.env.JET_CENSUS_MEMORY ?? '4G';
    if (!/^[1-8]G$/.test(budget)) throw Error('JET_CENSUS_MEMORY must be 1G through 8G (default 4G)');
    const scoped = spawnSync('systemd-run', ['--user', '--scope', '-p', `MemoryMax=${budget}`, 'env', 'JET_CENSUS_SCOPED=1', process.execPath, fileURLToPath(import.meta.url), ...args], { stdio: 'inherit' });
    if (scoped.error) throw scoped.error;
    process.exit(scoped.status ?? 2);
}
const base = resolve(options.scratch ?? join(homedir(), '.cache/jet-dev/scratch/GoldensCensus'));
mkdirSync(base, { recursive: true });
const inside = (parent, path) => { const part = relative(parent, path); return part === '' || (!isAbsolute(part) && part !== '..' && !part.startsWith(`..${sep}`)); };
if (inside(source, realpathSync(base)) || inside('/tmp', realpathSync(base))) throw Error('census scratch must be external and disk-backed');
const scratch = mkdtempSync(join(base, 'census-')), proof = join(scratch, 'snapshot'), root = join(proof, 'source'), work = join(scratch, 'work');
mkdirSync(root, { recursive: true });
mkdirSync(join(scratch, 'logs'));
console.log(`census evidence: ${scratch}`);
const json = (file, value) => writeFileSync(file, JSON.stringify(value, null, 2) + '\n');
const readJSON = file => existsSync(file) ? JSON.parse(readFileSync(file, 'utf8')) : null;
const hash = file => createHash('sha256').update(readFileSync(file)).digest('hex');
const rows = [], tasks = [], planned = [], children = new Set();
const clean = text => String(text ?? '').replaceAll('\t', ' ').replaceAll('\r', '').replaceAll('\n', ' \\n ').slice(0, 3000);
const classify = status => status === 'PASS' ? 'PASS' : ['SKIP', 'SKIP-HOST', 'UNSUPPORTED', 'OPT-IN', 'BLOCKED', 'KNOWN-FAILING', 'RETIRED'].includes(status) ? 'SKIP' : 'FAIL';
const first = text => String(text ?? '').split('\n').slice(0, 12).join('\n');
function firstLog(file) {
    const fd = openSync(file, 'r'), buffer = Buffer.alloc(64 * 1024);
    try { return first(buffer.toString('utf8', 0, readSync(fd, buffer, 0, buffer.length, 0))); }
    finally { closeSync(fd); }
}
writeFileSync(join(scratch, 'census.tsv'), 'test\tkind\tstatus\tdetail\n');
function record(row) {
    row.status = classify(row.rawStatus ?? row.status);
    row.detail = row.detail ?? '';
    if (row.run) {
        const { stdout, stderr, ...evidence } = row.run;
        row.run = evidence; // Full streams stay in logs, not repeated for every tier/Test.
    }
    rows.push(row);
    appendFileSync(join(scratch, 'census.tsv'), [row.test, row.kind, row.status, clean(row.detail)].join('\t') + '\n');
    appendFileSync(join(scratch, 'census.jsonl'), JSON.stringify(row) + '\n');
}
const env = { ...process.env, JET: jet, JET_TOOLCHAIN_ROOT: root, JET_KEEP_GOING: '1', NO_COLOR: '1', CLICOLOR: '0', TERM: 'dumb', JET_TEST_SCRATCH_DIR: scratch, JET_STORE_DIR: join(scratch, 'store'), JET_FFI_CACHE_DIR: join(scratch, 'ffi') };
for (const key of Object.keys(env)) {
    if (/^JET_(?:FRONT_|BOOTSTRAP_|TEST_FILTER|NATIVE_TEST_ID|UPDATE_SNAPSHOTS|DUMP_MIR)/.test(key) || key === 'JET_TEST_JSON') delete env[key];
}
let sequence = 0;
async function invoke(argv, cwd, seconds, overlay = {}) {
    const stem = join(scratch, 'logs', String(sequence++));
    const stdoutLog = `${stem}.stdout`, stderrLog = `${stem}.stderr`;
    const logs = [createWriteStream(stdoutLog), createWriteStream(stderrLog)];
    const logClosed = logs.map(log => new Promise((done, reject) => { log.on('finish', done); log.on('error', reject); }));
    let stdout = '', stderr = '', error = '', timedOut = false;
    const started = Date.now();
    const result = await new Promise(done => {
        const child = spawn(argv[0], argv.slice(1), { cwd, env: { ...env, ...overlay }, detached: true, stdio: ['ignore', 'pipe', 'pipe'] });
        children.add(child);
        child.stdout.pipe(logs[0]); child.stderr.pipe(logs[1]);
        child.stdout.on('data', bytes => { if (stdout.length < 1024 * 1024) stdout += bytes.toString(); });
        child.stderr.on('data', bytes => { if (stderr.length < 1024 * 1024) stderr += bytes.toString(); });
        child.once('error', failure => { error = failure.message; });
        const timer = seconds ? setTimeout(() => { timedOut = true; error = `TIMEOUT ${seconds}s`; try { process.kill(-child.pid, 'SIGKILL'); } catch {} }, seconds * 1000) : null;
        child.once('close', (code, signal) => {
            if (timer) clearTimeout(timer);
            children.delete(child);
            done({ argv, cwd, code, signal, error, timedOut, milliseconds: Date.now() - started, stdout, stderr, stdoutLog, stderrLog });
        });
    });
    await Promise.all(logClosed);
    return result;
}
function problem(result) { return result.error || (result.signal ? `signal ${result.signal}` : result.code ? `exit ${result.code}` : ''); }
function walk(directory, extension) {
    if (!existsSync(directory)) return [];
    return readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name)).flatMap(entry => {
        if (entry.name.startsWith('.')) return [];
        const path = join(directory, entry.name);
        return entry.isDirectory() ? walk(path, extension) : entry.isFile() && entry.name.endsWith(extension) ? [path] : [];
    });
}
function discover(directory, suite) {
    if (!existsSync(join(work, directory))) return [];
    return readdirSync(join(work, directory), { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name)).flatMap(entry => {
        if (entry.name.startsWith('.') || entry.name === 'expected') return [];
        const path = join(directory, entry.name);
        if (entry.isDirectory()) {
            if (['run.jet', 'package.jet', 'workspace.jet'].some(name => existsSync(join(work, path, name)))) {
                return existsSync(join(work, path, 'run.jet')) ? [{ suite, path: join(path, 'run.jet') }] : [];
            }
            return discover(path, suite);
        }
        return entry.name.endsWith('.jet') && !['package.jet', 'workspace.jet'].includes(entry.name) && !entry.name.endsWith('.fixed.jet') ? [{ suite, path }] : [];
    });
}
function summarize(complete, failure = '') {
    const totals = {};
    for (const row of rows) { totals[row.kind] ??= { PASS: 0, FAIL: 0, SKIP: 0 }; totals[row.kind][row.status]++; }
    const known = new Map();
    for (const bucket of readJSON(options.roots ? resolve(options.roots) : join(base, 'remaining-root-table.json'))?.roots ?? []) {
        for (const test of bucket.tests) known.set(test, bucket);
    }
    const buckets = new Map();
    for (const row of rows.filter(row => row.status !== 'PASS')) {
        const prior = known.get(row.test), code = /(?:Error|Stop|Warning) \[([^\]]+)\]/.exec(row.detail)?.[1];
        const rootName = prior?.root ?? `${row.kind}: ${code ?? (row.detail.split('\n')[0] || row.rawStatus || row.status)}`;
        if (!buckets.has(rootName)) buckets.set(rootName, { root: rootName, count: 0, kind: row.status === 'SKIP' ? 'host-only transport limit' : prior?.kind ?? 'unknown', ownerProposal: prior?.ownerProposal ?? 'Main: inspect the exercised native verdict and its repro before classifying or blessing', tests: [], samples: [] });
        const bucket = buckets.get(rootName);
        bucket.count++;
        if (!bucket.tests.includes(row.test)) bucket.tests.push(row.test);
        if (bucket.samples.length < 3) bucket.samples.push({ test: row.test, suite: row.kind, tier: row.tier, status: row.status, rawStatus: row.rawStatus, error: first(row.detail), repro: row.repro ?? row.run?.argv, cwd: row.run?.cwd, stdoutLog: row.run?.stdoutLog, stderrLog: row.run?.stderrLog });
    }
    const report = { complete, failure, source: root, work, jet, jetSha256: hash(jet), sourceHead: readJSON(join(proof, 'source.manifest.json'))?.commit, totals, recorded: rows.length, scratch, rows };
    json(join(scratch, 'census.json'), report);
    const roots = [...buckets.values()];
    json(join(scratch, 'remaining-root-table.json'), { complete, counts: totals, roots });
    writeFileSync(join(scratch, 'remaining-root-table.tsv'), 'root\tcount\tkind\townerProposal\ttests\n' + roots.map(bucket => [clean(bucket.root), bucket.count, bucket.kind, clean(bucket.ownerProposal), bucket.tests.join(',')].join('\t')).join('\n') + '\n');
    json(join(base, 'latest.json'), { scratch, complete, totals });
    cpSync(join(scratch, 'census.tsv'), join(base, 'census.tsv'));
    json(join(base, 'remaining-root-table.json'), { complete, counts: totals, roots });
    cpSync(join(scratch, 'remaining-root-table.tsv'), join(base, 'remaining-root-table.tsv'));
    return report;
}
for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => {
    for (const child of children) { try { process.kill(-child.pid, 'SIGKILL'); } catch {} }
    summarize(false, `interrupted by ${signal}`);
    process.exit(2);
});

async function main() {
    // Pin into PRIVATE census scratch; never publish into the RC's source/target pool.
    const existing = readJSON(join(dirname(source), 'source.manifest.json'));
    let manifest;
    if (existing?.schema === 'jet-proof-source/v1') {
        manifest = existing;
        for (const file of manifest.files) { mkdirSync(dirname(join(root, file.path)), { recursive: true }); cpSync(join(source, file.path), join(root, file.path), { dereference: false }); }
    } else {
        const git = argv => {
            const result = spawnSync('git', ['-C', source, ...argv], { encoding: 'utf8', maxBuffer: 64 << 20 });
            if (result.error || result.status !== 0) throw result.error ?? Error(result.stderr);
            return result.stdout;
        };
        const commit = git(['rev-parse', 'HEAD']).trim(), archive = join(scratch, 'source.tar');
        git(['archive', '--format=tar', `--output=${archive}`, commit]);
        const extracted = await invoke(['tar', '-xf', archive, '-C', root], scratch, options.registryTimeout);
        if (problem(extracted)) throw Error(`source extraction: ${problem(extracted)} ${first(extracted.stderr)}`);
        rmSync(archive);
        manifest = { schema: 'jet-proof-source/v1', commit, tree: git(['rev-parse', `${commit}^{tree}`]).trim(), files: git(['ls-tree', '-r', '-z', '--full-tree', commit]).split('\0').filter(Boolean).map(row => { const at = row.indexOf('\t'), [mode, type, blob] = row.slice(0, at).split(' '); if (type !== 'blob') throw Error('unsupported source tree member'); return { path: row.slice(at + 1), mode, blob }; }) };
    }
    json(join(proof, 'source.manifest.json'), manifest);
    cpSync(root, work, { recursive: true });
    // Only this mutable copy is the cwd for tests or examples.
    const registryDir = join(scratch, 'registries'), registryManifest = join(registryDir, 'manifest.json');
    const generated = await invoke([process.execPath, join(root, 'Tools/agent/compiler-registries.mjs'), 'manifest', manifest.commit, '--out', registryDir], root, options.registryTimeout);
    const registries = readJSON(registryManifest);
    if (!registries) record({ test: 'compiler/registry-manifest', kind: 'compiler-registry', rawStatus: 'FAIL', detail: problem(generated) + '\n' + first(generated.stderr), run: generated });
    else {
        planned.push(...registries.entries.map(entry => ({ test: entry.id, kind: 'compiler-registry' })));
        for (const entry of registries.entries) planned.push(...(entry.testNames ?? []).map(name => ({ test: `${entry.id}::${name}`, kind: 'compiler-test' })));
        for (let index = 1; index <= options.jobs; index++) {
            const shard = `${index}/${options.jobs}`, selected = partitionRegistries(registries.entries, shard);
            if (!selected.length) continue;
            tasks.push(async () => {
                const receipt = join(scratch, `registries-${index}.json`);
                const argv = [process.execPath, join(root, 'Tools/agent/compiler-registries.mjs'), 'run', '--jet', jet, '--manifest', registryManifest, '--receipt', receipt, '--shard', shard, ...(options.runtime ? ['--runtime', realpathSync(options.runtime)] : [])];
                const run = await invoke(argv, root, 0, { JET_REGISTRY_TIMEOUT_MS: String(options.registryTimeout * 1000) });
                const results = readJSON(receipt), byId = new Map((results?.entries ?? []).map(row => [row.id, row]));
                if (!results || results.jet !== jet || results.shard !== shard || results.fullTotal !== registries.entries.length || results.total !== selected.length || byId.size !== selected.length || results.entries.some(row => !selected.some(entry => entry.id === row.id))) {
                    record({ test: `compiler/receipt-integrity-${shard}`, kind: 'harness', rawStatus: 'FAIL', detail: 'registry receipt is absent, inconsistent, or incomplete', run });
                }
                for (const entry of selected) {
                    const result = byId.get(entry.id), stderr = result?.stderrLog && existsSync(result.stderrLog) ? firstLog(result.stderrLog) : first(run.stderr);
                    const detail = [result?.reason ?? (result?.status === 'PASS' ? '' : `registry shard did not publish this verdict: ${problem(run)}`), stderr].filter(Boolean).join('\n');
                    const evidence = result ? { argv: result.command, cwd: result.cwd, milliseconds: result.milliseconds, stdoutLog: result.stdoutLog, stderrLog: result.stderrLog } : run;
                    record({ test: entry.id, kind: 'compiler-registry', rawStatus: result?.status ?? 'BLOCKED', detail, claims: entry.testCount, run: evidence });
                    const tests = new Map((result?.report?.tests ?? []).map(test => [test.name, test]));
                    for (const name of entry.testNames ?? []) {
                        const test = tests.get(name);
                        record({ test: `${entry.id}::${name}`, kind: 'compiler-test', rawStatus: test ? (test.skipped ? 'SKIP' : test.ok || (test.expectedFailure && result?.report?.ok === true) ? 'PASS' : 'FAIL') : result?.status === 'PASS' ? 'FAIL' : 'BLOCKED', detail: test ? JSON.stringify(test) : detail || 'registry did not publish an individual Test verdict', run: evidence });
                    }
                }
            });
        }
    }
    const kitProject = join(work, 'Tools/testkit');
    const built = await invoke([jet, 'build', join(kitProject, 'run.jet'), '--color=never'], work, options.registryTimeout);
    const kit = [join(kitProject, '.jet/build/run'), join(work, '.jet/build/run')].find(file => existsSync(file));
    let entries = [], listed;
    if (!problem(built) && kit) {
        listed = await invoke([kit, 'all', '--list', '--opt-in', '--tier', options.tiers, '--timeout', String(options.timeout)], work, options.registryTimeout);
        if (!problem(listed)) entries = readFileSync(listed.stdoutLog, 'utf8').split('\n').filter(line => /^(golden|ui|cases)\t/.test(line)).map(line => { const [suite, path] = line.split('\t'); return { suite, path }; });
    }
    if (!entries.length) {
        entries = [...discover('Examples/features', 'golden'), ...discover('tests/run', 'golden'), ...discover('tests/ui', 'ui'), ...discover('tests/ui_lint', 'ui'), ...walk(join(work, 'tests/cases'), '.case').map(file => ({ suite: 'cases', path: relative(work, file) }))];
        for (const entry of entries) record({ test: entry.path, kind: entry.suite, rawStatus: 'BLOCKED', detail: 'native testkit prerequisite failed; not exercised\n' + problem(listed ?? built) + '\n' + first((listed ?? built).stderr), run: listed ?? built });
    } else {
        json(join(scratch, 'testkit-inventory.json'), entries);
        planned.push(...entries.map(entry => ({ test: entry.path, kind: entry.suite })));
        const count = Math.min(options.jobs, entries.length);
        for (let index = 1; index <= count; index++) tasks.push(async () => {
            const receipt = join(scratch, `testkit-${index}.json`), shard = `${index}/${count}`;
            const argv = [kit, 'all', '--jet', jet, '--opt-in', '--tier', options.tiers, '--timeout', String(options.timeout), '--shard', shard, '--scratch', scratch, '--results', receipt];
            if (process.env.JETPACK) argv.push('--jetpack', realpathSync(process.env.JETPACK));
            const run = await invoke(argv, work, 0), report = readJSON(receipt);
            const selected = [...entries].sort((a, b) => a.path < b.path ? -1 : a.path > b.path ? 1 : 0).filter((_, offset) => offset % count === index - 1);
            const actual = new Set();
            const seen = new Set();
            if (!report || report.jet !== jet || !Array.isArray(report.results) || report.results.some(result => {
                const key = `${result.case}\0${result.tier}`;
                if (!selected.some(entry => entry.path === result.case && entry.suite === result.suite) || seen.has(key)) return true;
                seen.add(key);
                return false;
            })) {
                record({ test: `testkit/receipt-integrity-${shard}`, kind: 'harness', rawStatus: 'FAIL', detail: 'testkit receipt is absent, inconsistent, or contains unexpected/duplicate verdicts', run });
            }
            for (const result of report?.results ?? []) {
                actual.add(result.case);
                record({ test: result.case, kind: result.suite, tier: result.tier, rawStatus: result.status, detail: result.detail + (result.diff ? '\n' + first(result.diff) : ''), repro: [kit, result.suite, '--jet', jet, '--filter', result.case, '--tier', result.tier === '-' ? options.tiers : result.tier, '--opt-in', '--timeout', String(options.timeout), '--scratch', scratch], run });
            }
            for (const entry of selected.filter(entry => !actual.has(entry.path))) record({ test: entry.path, kind: entry.suite, rawStatus: 'BLOCKED', detail: `testkit shard published no verdict: ${problem(run)}\n${first(run.stderr)}`, run });
        });
    }
    for (const file of walk(join(work, 'Examples/suites'), '.jet').filter(file => !file.endsWith('/package.jet'))) tasks.push(async () => {
        const id = relative(work, file), expected = join(dirname(file), 'expected', file.split('/').at(-1).replace(/\.jet$/, '.out'));
        planned.push({ test: id, kind: 'suite' });
        if (!existsSync(expected)) return record({ test: id, kind: 'suite', rawStatus: 'BLOCKED', detail: 'missing suite golden; no invented oracle' });
        const run = await invoke([jet, 'run', file], work, options.timeout), actual = readFileSync(run.stdoutLog, 'utf8'), wanted = readFileSync(expected, 'utf8');
        const failure = problem(run) || (actual !== wanted ? 'stdout mismatch\n- ' + first(wanted) + '\n+ ' + first(actual) : '');
        record({ test: id, kind: 'suite', rawStatus: failure ? 'FAIL' : 'PASS', detail: failure + (failure && run.stderr ? '\n' + first(run.stderr) : ''), run });
    });
    for (const file of walk(join(work, 'Core'), '.jet')) {
        const text = readFileSync(file, 'utf8'), names = [...text.matchAll(/^\s*#Test\s*\(\s*("(?:\\.|[^"\\])*")/gm)].map(match => JSON.parse(match[1]));
        if (!names.length) continue;
        planned.push(...names.map(name => ({ test: `${relative(work, file)}::${name}`, kind: 'core' })));
        tasks.push(async () => {
            const id = relative(work, file), run = await invoke([jet, 'test', '--capture=all', '--color=never', file], work, options.timeout, { JET_TEST_JSON: '1' });
            const reports = readFileSync(run.stdoutLog, 'utf8').split('\n').flatMap(line => { try { const value = JSON.parse(line); return value.schema === 'jet.test.v1' ? [value] : []; } catch { return []; } });
            const report = reports.length === 1 ? reports[0] : null, tests = new Map((report?.tests ?? []).map(test => [test.name, test]));
            for (const name of names) {
                const test = tests.get(name);
                const passed = test?.ok === true || test?.status === 'passed' || (test?.expectedFailure && report?.ok === true);
                record({ test: `${id}::${name}`, kind: 'core', rawStatus: test?.skipped || test?.status === 'skipped' ? 'SKIP' : passed ? 'PASS' : 'FAIL', detail: test ? JSON.stringify(test) : `no canonical verdict for this Test\n${problem(run)}\n${first(run.stderr)}`, run });
            }
            if (report && (tests.size !== names.length || report.ok !== true || report.failed !== 0 || report.unexpectedPasses !== 0)) record({ test: `${id}::report-integrity`, kind: 'core', rawStatus: 'FAIL', detail: 'Core Test report has missing/extra/failed/unexpected verdicts\n' + first(run.stderr), run });
        });
    }
    let next = 0;
    await Promise.all(Array.from({ length: options.jobs }, async () => {
        while (next < tasks.length) {
            const task = tasks[next++];
            try { await task(); }
            catch (error) { record({ test: `census/task-${next}`, kind: 'harness', rawStatus: 'FAIL', detail: error.stack }); }
        }
    }));
    const recorded = new Set(rows.map(row => `${row.kind}\0${row.test}`));
    for (const entry of planned) {
        if (!recorded.has(`${entry.kind}\0${entry.test}`)) record({ ...entry, rawStatus: 'BLOCKED', detail: 'census task failed before publishing this verdict; not exercised' });
    }
    const report = summarize(true);
    console.log(JSON.stringify({ complete: true, scratch, totals: report.totals, recorded: rows.length }, null, 2));
    process.exitCode = rows.some(row => row.status !== 'PASS') ? 1 : 0;
}
main().catch(error => { summarize(false, error.stack); console.error(error.stack); process.exitCode = 2; });
