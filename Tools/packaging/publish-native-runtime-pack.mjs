#!/usr/bin/env node
// Internal proof evidence and post-done0 driver. This file never writes AC/CAS
// entries or implements JCS: the compiled Jet Store publisher owns both.
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { closeSync, constants, copyFileSync, existsSync, fstatSync, mkdirSync, openSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ARTIFACTS = ['pack_object', 'jet_object', 'remaining_native_archive', 'remaining_raw_leaf_archive', 'runtime_sources', 'runtime_exports', 'runtime_imports'];
const GROUPS = ['generators', 'abi', 'routes', 'providers'];
const HANDLE = /^[0-9a-f]{64}:(0|[1-9][0-9]*)$/;
function insist(value, message) { if (!value) throw Error(message); }
function exact(value, keys, label) {
  insist(value && typeof value === 'object' && !Array.isArray(value), `${label}: expected object`);
  insist(Object.keys(value).length === keys.length && keys.every(key => Object.hasOwn(value, key)), `${label}: missing or unknown fields`);
  return value;
}
function text(value, label) { insist(typeof value === 'string' && value.length > 0, `${label}: expected nonempty string`); return value; }
function rc(value) { insist(typeof value === 'string' && /^(0|[1-9][0-9]*)$/.test(value) && BigInt(value) <= 255n, 'invalid process rc'); return value; }
function object(value) { insist(typeof value === 'string' && HANDLE.test(value) && BigInt(value.slice(65)) <= 18446744073709551615n, 'invalid sha256:length ObjectHandle'); return value; }
function json(path) { return JSON.parse(readFileSync(path, 'utf8')); }
function freshJson(path, value) { mkdirSync(dirname(path), { recursive: true }); writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`, { flag: 'wx' }); }

// Hold the opened inode while hashing, and reject an in-place or pathname swap.
// BigInt lengths never pass through a JavaScript Number wire value.
export function fileObject(path) {
  const fd = openSync(path, 'r');
  try {
    const before = fstatSync(fd, { bigint: true });
    insist(before.isFile(), `not a regular publication input: ${path}`);
    const bytes = readFileSync(fd);
    const after = fstatSync(fd, { bigint: true });
    const named = statSync(path, { bigint: true });
    insist(before.dev === after.dev && before.ino === after.ino && before.size === after.size && before.mtimeNs === after.mtimeNs && before.ctimeNs === after.ctimeNs && named.dev === before.dev && named.ino === before.ino && named.size === before.size && BigInt(bytes.length) === before.size, `publication input changed while held: ${path}`);
    return `${createHash('sha256').update(bytes).digest('hex')}:${before.size.toString()}`;
  } finally { closeSync(fd); }
}
function roles(rows, visit) {
  insist(Array.isArray(rows), 'role list must be an array');
  const seen = new Set();
  return rows.map(row => {
    exact(row, ['role', 'path'], 'role input');
    text(row.role, 'role'); text(row.path, 'role path');
    insist(!seen.has(row.role), `duplicate role: ${row.role}`); seen.add(row.role);
    return { role: row.role, object: visit(resolve(row.path)) };
  });
}


export function parseProofqMetadata(bytes, name) {
  const values = new Map();
  for (const line of new TextDecoder('utf-8', { fatal: true }).decode(bytes).split('\n')) {
    if (line === '') continue;
    const at = line.indexOf('=');
    insist(at > 0, 'malformed proofq metadata');
    const key = line.slice(0, at);
    insist(!values.has(key), 'duplicate proofq metadata key');
    values.set(key, line.slice(at + 1));
  }
  insist(values.get('name') === name && values.get('rc') === '0', 'proofq metadata does not name the actual successful job');
  for (const key of ['start', 'end']) insist(/^(0|[1-9][0-9]*)$/.test(values.get(key) ?? '') && BigInt(values.get(key)) <= 18446744073709551615n, `proofq metadata lacks actual ${key}`);
  insist(BigInt(values.get('end')) >= BigInt(values.get('start')), 'proofq terminal time precedes actual start');
  insist(['build', 'test', 'measure'].includes(values.get('kind')), 'proofq metadata lacks actual job kind');
  return values;
}

// Reuse completed proofq's original sibling script, status and metadata bytes.
// Names locate receipts; admission remains their content identity, not a path.
export function captureTerminal(manifest, contract, subject, blob, directory) {
  const name = text(contract.proofq_name, 'admitted proofq name');
  insist(/^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(name), 'invalid admitted proofq name');
  const done = resolve(manifest.done), base = dirname(done);
  insist(basename(done) === `${name}.rc`, 'terminal status does not locate the admitted proofq job');
  insist(readFileSync(done, 'utf8') === '0\n', 'terminal status is not actual proofq done0');
  const completedJob = join(base, `${name}.sh`), metadata = join(base, `${name}.meta`);
  parseProofqMetadata(readFileSync(metadata), name);
  const actualJob = blob(completedJob);
  insist(actualJob === object(contract.job) && actualJob === blob(manifest.job), 'completed proofq script differs from admitted job bytes');
  const path = join(directory, 'proofq-terminal.json');
  freshJson(path, { schema: 'jet.native-runtime-proofq-terminal/v1', job: actualJob, rail_contract: blob(manifest.rail_contract), done: blob(done), proofq_metadata: blob(metadata), proofq_name: name, runtime_source_revision: subject.runtime_source_revision, runtime_source_tree: subject.runtime_source_tree });
  return blob(path);
}
// These are named raw proof snapshots, not a second content-addressed cache:
// no digest directory, lookup/reuse policy or machine-wide publication exists.
// They preserve overwritten producer binaries and consumed input bytes.
export function captureStep(descriptorPath, directory) {
  const input = exact(json(descriptorPath), ['schema', 'id', 'invocation', 'inputs'], 'step capture');
  insist(input.schema === 'jet.native-runtime-step-capture/v1', 'unknown capture schema');
  const dir = resolve(directory);
  insist(!existsSync(dir), 'refusing to overwrite captured step evidence');
  mkdirSync(dir, { recursive: true });
  const files = [];
  function snapshot(path) {
    const target = join(dir, `input-${files.length}.bytes`);
    copyFileSync(path, target, constants.COPYFILE_EXCL);
    const handle = fileObject(target);
    insist(handle === fileObject(path), `input changed during snapshot: ${path}`);
    files.push({ object: handle, path: target });
    return handle;
  }
  const invocation = snapshot(resolve(text(input.invocation, 'actual invocation path')));
  const inputs = roles(input.inputs, snapshot);
  freshJson(join(dir, 'capture.json'), { schema: 'jet.native-runtime-captured-step/v1', id: text(input.id, 'step id'), invocation, inputs, files });
  return join(dir, 'capture.json');
}

export function observeStep(capturePath, status, outputsPath, logPath, resultPath) {
  const captured = exact(json(capturePath), ['schema', 'id', 'invocation', 'inputs', 'files'], 'captured step');
  insist(captured.schema === 'jet.native-runtime-captured-step/v1', 'unknown captured-step schema');
  rc(status);
  const files = [...captured.files];
  function snapshot(path) {
    const target = join(dirname(resolve(resultPath)), `output-${files.length}.bytes`);
    mkdirSync(dirname(target), { recursive: true });
    copyFileSync(path, target, 1);
    const handle = fileObject(target);
    insist(handle === fileObject(path), `output changed during snapshot: ${path}`);
    files.push({ object: handle, path: target });
    return handle;
  }
  const outputs = roles(json(outputsPath), snapshot);
  const log = snapshot(resolve(logPath));
  const receipt = { schema: 'jet.native-runtime-step-receipt/v1', id: captured.id, rc: status, invocation: object(captured.invocation), inputs: captured.inputs, outputs, log };
  freshJson(resultPath, { schema: 'jet.native-runtime-step-observation/v1', receipt, files });
  return receipt;
}

function git(wt, ...args) {
  const result = spawnSync('git', args, { cwd: wt, encoding: 'utf8' });
  insist(!result.error && result.status === 0, `git ${args.join(' ')} failed: ${result.error?.message ?? result.stderr}`);
  return result.stdout.trimEnd();
}
function runReadelf(path, args) {
  const result = spawnSync('readelf', [...args, path], { encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 });
  insist(!result.error && result.status === 0, `readelf failed: ${result.error?.message ?? result.stderr}`);
  return result.stdout;
}
export function lifecycleSection(phase, priority) {
  insist(['init', 'fini'].includes(phase) && typeof priority === 'string' && /^(0|[1-9][0-9]*)$/.test(priority) && BigInt(priority) <= 65535n, 'invalid lifecycle phase or canonical priority');
  return `.${phase}_array.${priority.padStart(5, '0')}`;
}

// readelf -SW's bracketed index precedes the complete section-name field.
// Evidence must name that field exactly, never a substring in another name.
export function readelfSectionNames(text) {
  return new Set([...text.matchAll(/^[ \t]*\[[ \t]*[0-9]+\][ \t]+([^ \t\r\n]+)[ \t]/gm)].map(match => match[1]));
}

function lifecycle(wt, manifest, subject, blob, directory) {
  const sectionText = runReadelf(manifest.artifacts.pack_object, ['-SW']);
  const sectionNames = readelfSectionNames(sectionText);
  const relocText = runReadelf(manifest.artifacts.pack_object, ['-rW']);
  const sectionsPath = join(directory, 'lifecycle.sections');
  const relocsPath = join(directory, 'lifecycle.relocations');
  writeFileSync(sectionsPath, sectionText, { flag: 'wx' });
  writeFileSync(relocsPath, relocText, { flag: 'wx' });
  const providers = new Map(manifest.roles.providers.map(row => [resolve(row.path), subject.providers.find(value => value.role === row.role).object]));
  const declarations = new Map();
  for (const file of readFileSync(manifest.artifacts.runtime_sources, 'utf8').split(/\r?\n/).map(line => line.trim()).filter(line => line && !line.startsWith('#'))) {
    const path = resolve(wt, file);
    for (const match of readFileSync(path, 'utf8').matchAll(/^\/\/ runtime-(init|fini): (\d+)\n#Export\(c\)\s+(?:pub\s+)?fn\s+([A-Za-z_]\w*)\s*\(/gm)) {
      insist(!declarations.has(match[3]), `duplicate lifecycle declaration ${match[3]}`);
      insist(providers.has(path), `lifecycle provider missing from admitted subject: ${file}`);
      declarations.set(match[3], { phase: match[1], priority: match[2], provider: providers.get(path) });
    }
  }
  const entries = [];
  for (const line of readFileSync(manifest.artifacts.runtime_exports, 'utf8').split(/\r?\n/).filter(Boolean)) {
    const fields = line.trim().split(/\s+/);
    if (fields.length === 1 || (fields.length === 2 && fields[1] === 'int-index-seam')) continue;
    insist(fields.length === 3 && ['init', 'fini'].includes(fields[1]) && /^(0|[1-9][0-9]*)$/.test(fields[2]) && BigInt(fields[2]) <= 65535n, `invalid lifecycle export ${line}`);
    const [symbol, phase, priority] = fields;
    const actual = declarations.get(symbol);
    insist(actual && actual.phase === phase && actual.priority === priority, `lifecycle annotation differs for ${symbol}`);
    const section = lifecycleSection(phase, priority);
    insist(sectionNames.has(section), `pack lost lifecycle section ${section}`);
    const relocationSection = `.rela${section}`;
    const block = relocText.split(/(?=Relocation section ')/).find(part => part.startsWith(`Relocation section '${relocationSection}'`));
    insist(block && block.split('\n').some(row => row.includes('R_X86_64_64') && row.split(/\s+/).includes(symbol)), `pack lost lifecycle relocation ${symbol}`);
    entries.push({ symbol, phase, priority, provider: actual.provider, section });
    declarations.delete(symbol);
  }
  insist(declarations.size === 0, 'source lifecycle declaration omitted from exports');
  insist(entries.some(row => row.symbol === 'jet_rt_alloc_init' && row.phase === 'init' && row.priority === '0'), 'missing allocator initialization priority 0');
  insist(entries.some(row => row.symbol === 'jet_rt_host_init' && row.phase === 'init' && row.priority === '1'), 'missing host initialization priority 1');
  insist(entries.some(row => row.symbol === 'jet_rt_stdout_flush' && row.phase === 'fini' && row.priority === '1'), 'missing canonical stdout finalizer');
  const path = join(directory, 'lifecycle.json');
  freshJson(path, { schema: 'jet.native-runtime-lifecycle-evidence/v1', pack: subject.pack_object, jet_object: subject.jet_object, entries, sections: blob(sectionsPath), relocations: blob(relocsPath) });
  return blob(path);
}

// Positional internal invocation: prepare/publish manifest publisher output-dir
// [explicit-authorization]. The compiler consumer's held-executable seam is
// separately gated on #4896; this publisher hashes the actual produced file.
export function publication(mode, manifestPath, publisher, directory, authorizationPath) {
  insist(mode === 'prepare' || mode === 'publish', 'unknown publication phase');
  insist((mode === 'publish') === (authorizationPath !== undefined), 'publish requires explicit subject-bound authorization, prepare forbids it');
  const manifest = exact(json(manifestPath), ['schema', 'worktree', 'done', 'job', 'admission', 'rail_contract', 'compiler', 'generation', 'raw_frontend', 'final_source_tree', 'steps_tsv', 'observations', 'artifacts', 'roles'], 'publication manifest');
  insist(manifest.schema === 'jet.native-runtime-publication-input/v1', 'unknown publication-input schema');
  insist(readFileSync(manifest.done, 'utf8') === '0\n', 'publication requires actual terminal proofq done rc 0');
  const wt = resolve(text(manifest.worktree, 'runtime worktree'));
  insist(git(wt, 'status', '--porcelain') === '', 'publication requires clean FINAL runtime source');
  const revision = git(wt, 'rev-parse', 'HEAD');
  const tree = git(wt, 'rev-parse', 'HEAD^{tree}');
  const generation = json(manifest.generation);
  insist(generation.deterministic === true && generation.final === revision, 'generation receipt does not bind clean runtime FINAL');
  const contract = json(manifest.rail_contract);
  insist(contract.schema === 'jet.native-runtime-rail-contract/v1' && Array.isArray(contract.steps) && contract.steps.length > 0, 'missing admitted ONE rail contract');
  const stepRows = readFileSync(manifest.steps_tsv, 'utf8').trimEnd().split('\n').map(line => {
    const match = /^([^\t]+)\trc=(0|[1-9][0-9]*)$/.exec(line);
    insist(match, 'malformed actual step status row'); return { id: match[1], rc: rc(match[2]) };
  });
  insist(Array.isArray(manifest.observations) && manifest.observations.length === contract.steps.length && stepRows.length === contract.steps.length, 'missing/extra completed rail steps');
  const dir = resolve(directory);
  insist(!existsSync(dir), 'refusing to overwrite publication evidence');
  mkdirSync(dir, { recursive: true });
  const blobs = new Map();
  function blob(path, expected) {
    path = resolve(path); const actual = fileObject(path);
    if (expected !== undefined) insist(actual === object(expected), `captured object bytes changed: ${path}`);
    if (!blobs.has(actual)) blobs.set(actual, { object: actual, path });
    return actual;
  }
  const steps = manifest.observations.map((path, index) => {
    const observed = exact(json(path), ['schema', 'receipt', 'files'], 'step observation');
    insist(observed.schema === 'jet.native-runtime-step-observation/v1', 'unknown observation schema');
    const receipt = exact(observed.receipt, ['schema', 'id', 'rc', 'invocation', 'inputs', 'outputs', 'log'], 'actual step receipt');
    insist(receipt.schema === 'jet.native-runtime-step-receipt/v1' && receipt.id === contract.steps[index].id && receipt.id === stepRows[index].id && receipt.rc === stepRows[index].rc && receipt.rc === contract.steps[index].expected_rc && receipt.rc === '0', `actual rail step ${index} is red/reordered/different`);
    insist(Array.isArray(observed.files), 'observation has no original input/output bytes');
    for (const file of observed.files) { exact(file, ['object', 'path'], 'observed blob'); blob(file.path, file.object); }
    return receipt;
  });
  exact(manifest.artifacts, ARTIFACTS, 'runtime artifacts'); exact(manifest.roles, GROUPS, 'runtime role groups');
  const subject = { schema: 'jet.native-runtime-pack-subject/v1', runtime_source_revision: revision, runtime_source_tree: tree };
  for (const name of ARTIFACTS) subject[name] = blob(manifest.artifacts[name]);
  for (const group of GROUPS) subject[group] = roles(manifest.roles[group], blob);
  subject.lifecycle = lifecycle(wt, manifest, subject, blob, dir);
  const request = { schema: 'jet.native-runtime-publication-request/v1', subject, blobs: [], admission: blob(manifest.admission), rail_contract: blob(manifest.rail_contract), job: blob(manifest.job), proof_done: blob(manifest.done), proof_terminal: captureTerminal(manifest, contract, subject, blob, dir), final_source_tree: blob(manifest.final_source_tree), generation: blob(manifest.generation), raw_frontend: blob(manifest.raw_frontend), steps, compiler: blob(manifest.compiler) };
  insist(request.job === object(contract.job), 'executed proofq job differs from admitted contract');
  if (mode === 'publish') request.publication_authorization = blob(authorizationPath);
  request.blobs = [...blobs.values()];
  const requestPath = join(dir, 'request.json'), resultPath = join(dir, 'result.json');
  freshJson(requestPath, request);
  const result = spawnSync(resolve(publisher), [mode, requestPath, resultPath], { cwd: wt, env: process.env, encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 });
  writeFileSync(join(dir, 'publisher.stdout'), result.stdout ?? '', { flag: 'wx' });
  writeFileSync(join(dir, 'publisher.stderr'), result.stderr ?? '', { flag: 'wx' });
  freshJson(join(dir, 'publisher.receipt.json'), { status: result.status === null ? null : String(result.status), signal: result.signal, error: result.error?.message ?? null, request: fileObject(requestPath) });
  insist(!result.error && result.status === 0 && existsSync(resultPath), `Jet Store publisher failed: ${result.error?.message ?? result.status ?? result.signal}`);
  return json(resultPath);
}

export function main(args) {
  const [mode, ...rest] = args;
  if (mode === 'capture' && rest.length === 2) return captureStep(...rest);
  if (mode === 'observe' && rest.length === 5) return observeStep(...rest);
  if ((mode === 'prepare' && rest.length === 3) || (mode === 'publish' && rest.length === 4)) return publication(mode, ...rest);
  throw Error('internal publication tool requires capture/observe/prepare/publish positional inputs');
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { main(process.argv.slice(2)); }
  catch (error) { process.stderr.write(`runtime publication refused: ${error.message}\n`); process.exitCode = 1; }
}
