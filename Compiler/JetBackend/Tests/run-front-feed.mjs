#!/usr/bin/env node
// #4548: every pass in an explicit native baseline, never a hand-picked subset.
// Prepare/build FrontFeed.jet with the same compiler sources, then compare both
// backend feeds against each other AND the golden. --bootstrap diagnoses the
// Jet front end before a standalone probe/Jet CLI artifact is available.
//
// run-front-feed.mjs OUT --baseline results.tsv --rust-mir DIR --assemble
// run-front-feed.mjs OUT --baseline results.tsv --rust-mir DIR --probe BINARY
// run-front-feed.mjs OUT --baseline results.tsv --bootstrap JETC0
// --assemble writes probe/unit.jet; build it with the reference compiler at
// --release, then run --probe with JET_RUNTIME_PACK pointing to the SAME pack
// used for the baseline. Preparation/census never claims differential success.
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createReadStream, existsSync, mkdirSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { basename, dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { collectGoldenEntries, loadExampleStdin } from '../../../Tools/agent/compiler-diff.mjs';
import { copyFeatureProject, featureProjectRoot } from '../../../Tools/agent/run-feature-examples.mjs';
import { Converter, encode, jetDecoder, loadMirSchema, parseRustDebug } from './mir-debug.mjs';
import { decodeStream, jetWriter } from './front-feed-codec.mjs';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const argv = process.argv.slice(2);
const output = argv.shift();
if (!output) throw new Error('expected OUT --baseline results.tsv [--rust-mir DIR|--rust-jet BINARY] --assemble|--probe BINARY|--bootstrap JETC0');
const options = {};
while (argv.length) { const flag = argv.shift(); if (flag === '--assemble') options.assemble = true; else if (['--baseline', '--rust-mir', '--rust-jet', '--probe', '--bootstrap'].includes(flag)) options[flag.slice(2)] = argv.shift(); else throw new Error(`unknown option ${flag}`); }
if (!options.baseline || [options.assemble, options.probe, options.bootstrap].filter(Boolean).length !== 1) throw new Error('require baseline and exactly one of assemble, probe, bootstrap');
const out = resolve(output);
mkdirSync(out, { recursive: true });
const entries = new Map(collectGoldenEntries(join(repo, 'Examples/features')).map(e => [e.stem, e]));
const baselineText = readFileSync(options.baseline, 'utf8');
const passing = baselineText.split('\n').filter(Boolean).map(row => row.split('\t')).filter(row => row[1] === 'pass').map(row => row[0]);
if (!passing.length || new Set(passing).size !== passing.length) throw new Error('baseline must name nonempty, unique native passes');
const schema = loadMirSchema(repo);
const stdinTable = loadExampleStdin();
const gaps = new Map();
const gap = (category, key, stem, detail) => { const identity = `${category}\t${key}`; if (!gaps.has(identity)) gaps.set(identity, { category, key, cases: new Set(), samples: [] }); const row = gaps.get(identity); row.cases.add(stem); if (row.samples.length < 3) row.samples.push({ stem, detail }); };
const cases = passing.map(stem => {
  const entry = entries.get(stem); if (!entry) throw new Error(`baseline case missing from current examples: ${stem}`);
  const slug = stem.replace(/[^A-Za-z0-9_-]+/gu, '_');
  const work = join(out, 'work', slug);
  rmSync(work, { recursive: true, force: true });
  const project = featureProjectRoot(entry);
  const sourceRoot = project ?? dirname(entry.path);
  const root = join(work, relative(repo, sourceRoot));
  const file = join(work, relative(repo, entry.path));
  if (project || basename(entry.path) === 'run.jet') copyFeatureProject(sourceRoot, root);
  else { mkdirSync(root, { recursive: true }); writeFileSync(file, readFileSync(entry.path)); }
  // The bootstrap host requires an explicit package authority for loose
  // fixtures. Do not accidentally include every sibling golden in that root.
  if (!existsSync(join(root, 'package.jet'))) writeFileSync(join(root, 'package.jet'), 'name: "front_feed_case"\nversion: "0.0.1"\nedition: "2028"\n');
  const prefix = join(out, 'feeds', slug); mkdirSync(dirname(prefix), { recursive: true });
  return { stem, slug, entry, work, file, root, prefix, expected: readFileSync(join(repo, 'Examples/features/expected', `${stem}.out`)), stdin: stdinTable.get(stem) ?? '' };
});
writeFileSync(join(out, 'manifest.json'), JSON.stringify({ baseline: resolve(options.baseline), baseline_sha256: createHash('sha256').update(baselineText).digest('hex'), cases: cases.map(c => ({ stem: c.stem, file: c.file, prefix: c.prefix })) }, null, 2) + '\n');
const results = [];
function save(mode) {
  const ranked = [...gaps.values()].sort((a, b) => b.cases.size - a.cases.size || a.category.localeCompare(b.category) || a.key.localeCompare(b.key)).map((row, i) => ({ rank: i + 1, category: row.category, key: row.key, affected: row.cases.size, cases: [...row.cases].sort(), samples: row.samples }));
  writeFileSync(join(out, 'gaps.json'), JSON.stringify(ranked, null, 2) + '\n');
  writeFileSync(join(out, 'gaps.tsv'), 'rank\tcategory\taffected\tkey\n' + ranked.map(r => `${r.rank}\t${r.category}\t${r.affected}\t${r.key.replace(/[\t\n]/g, ' ')}`).join('\n') + '\n');
  writeFileSync(join(out, 'results.tsv'), results.map(r => `${r.stem}\t${r.verdict}\t${r.detail.replace(/[\t\n]/g, ' ')}`).join('\n') + '\n');
  const summary = { mode, selected: passing.length, observed: results.length, pass: results.filter(r => r.verdict === 'pass').length, gaps: ranked.length, differential_verified: mode === 'differential' && results.length === passing.length && results.every(r => r.verdict === 'pass') };
  writeFileSync(join(out, 'summary.json'), JSON.stringify(summary, null, 2) + '\n'); console.log(JSON.stringify(summary));
  return summary;
}

if (options.bootstrap) {
  // A receipt census still diagnoses every selected case when MIR observation
  // is unavailable in an older candidate. Do not turn missing MIR into parity.
  for (const c of cases) {
    const receipt = `${c.prefix}.receipt`; const rust = `${c.prefix}.rs`; const dump = `${c.prefix}.jet.mir`;
    rmSync(receipt, { force: true }); rmSync(dump, { force: true });
    const compiled = spawnSync(resolve(options.bootstrap), [], { cwd: c.work, env: { ...process.env, CARGO_TARGET_DIR: '/mnt/jetscratch/targets/NativeFrontFeed', CARGO_INCREMENTAL: '0', JET_BOOTSTRAP_MODE: 'runner', JET_BOOTSTRAP_FACTORY_TIER: 'aot', JET_BOOTSTRAP_SOURCE_ROOT: c.root, JET_BOOTSTRAP_ENTRY: relative(c.root, c.file), JET_BOOTSTRAP_OUTPUT: rust, JET_BOOTSTRAP_RECEIPT: receipt, JET_FRONT_MIR_DUMP: dump, JET_STORE_DIR: join(out, 'store', c.slug), TMPDIR: out, RUST_MIN_STACK: '8388608' }, timeout: Number(process.env.JET_FRONT_TIMEOUT_MS ?? 120000), encoding: 'utf8', maxBuffer: 1 << 27 });
    writeFileSync(`${c.prefix}.compile.log`, `${compiled.stdout ?? ''}${compiled.stderr ?? ''}`);
    const text = existsSync(receipt) ? readFileSync(receipt, 'utf8') : '';
    const reports = text.split('\n').filter(line => line.startsWith('report_json=')).map(line => JSON.parse(line.slice('report_json='.length)));
    let verdict = 'front-ok';
    for (const report of reports) {
      const severity = String(report.severity ?? report.level ?? 'error').toLowerCase();
      if (severity.includes('warning') || severity.includes('lint')) continue;
      const code = report.code ?? 'UNKNOWN'; const message = report.what ?? report.message ?? JSON.stringify(report);
      gap('front-end', `${code}: ${message}`, c.stem, report); verdict = 'front-error';
    }
    if (!text.includes('complete=true\n') || compiled.status !== 0) {
      const panic = compiled.stderr?.match(/panicked at[^\n]*\n([^\n]+)/)?.[1];
      gap('front-end', compiled.signal ? `signal ${compiled.signal}` : compiled.error?.code ?? panic ?? 'incomplete', c.stem, { status: compiled.status, error: compiled.error?.message, receipt: text }); verdict = 'front-error';
    }
    if (!existsSync(dump)) gap('observation', 'Jet-produced MIR unavailable in candidate', c.stem, 'candidate has no JET_FRONT_MIR_DUMP observer');
    results.push({ stem: c.stem, verdict, detail: `${reports.length} diagnostic reports; MIR ${existsSync(dump) ? 'observed' : 'unobserved'}` });
    rmSync(rust, { force: true });
    save('front-end-census');
  }
  process.exitCode = results.some(r => r.verdict === 'front-error') ? 1 : 0;
} else {
  if (!options['rust-mir'] && !options['rust-jet']) throw new Error('differential requires --rust-mir or --rust-jet');
  for (const c of cases) {
    const dump = options['rust-jet'] ? `${c.prefix}.rust.mir` : join(resolve(options['rust-mir']), `${c.slug}.mir`);
    if (options['rust-jet']) {
      rmSync(dump, { force: true });
      const emitted = spawnSync(resolve(options['rust-jet']), ['emit', '--rust', c.file], { cwd: c.work, env: { ...process.env, JET_DUMP_MIR: dump }, timeout: Number(process.env.JET_FRONT_TIMEOUT_MS ?? 120000), stdio: ['ignore', 'ignore', 'pipe'], maxBuffer: 1 << 27 });
      writeFileSync(`${c.prefix}.rust-front.log`, emitted.stderr ?? Buffer.alloc(0));
    }
    rmSync(`${c.prefix}.rust.mird`, { force: true });
    rmSync(`${c.prefix}.rust.convert-error`, { force: true });
    try {
      const converter = new Converter(schema); const node = parseRustDebug(readFileSync(dump, 'utf8'));
      const value = converter.convert(node, { k: 'named', name: 'MIRProgram' });
      const observed = node.fields.find(([name]) => name === 'artifacts')?.[1]?.items ?? [];
      const entry = observed.map(artifact => artifact.fields.find(([name]) => name === 'entry')?.[1]).filter(Boolean).map(row => row.items?.[0]?.fields.find(([name]) => name === 'function')?.[1]?.items?.[0]?.items?.[0]?.v).find(Boolean);
      if (!entry) throw new Error(`Rust baseline dump lost entry: ${c.stem}`);
      const stream = [entry]; encode(value, { k: 'named', name: 'MIRProgram' }, schema, stream);
      writeFileSync(`${c.prefix}.rust.mird`, stream.join('\n'));
    } catch (error) {
      gap('schema-conversion', error.message, c.stem, error.message);
      writeFileSync(`${c.prefix}.rust.convert-error`, error.message + '\n');
    }
  }
  const probeDir = join(out, 'probe'); mkdirSync(probeDir, { recursive: true });
  const imports = /^use (?:jet_foundation|jet_lexer|jet_parser|jet_optimizer|jet_sema|jet_codegen|jet_eval|jet_driver|jet_backend|jet_cli|compiler_bootstrap)\.\[[^\]]*\]/gm;
  const seenImports = new Set();
  const clean = text => text.replace(imports, block => block.replace(/[^\n]/g, ' ')).replace(/^use core\.[^\[\n]*$/gm, line => { if (seenImports.has(line)) return line.replace(/[^\n]/g, ' '); seenImports.add(line); return line; });
  const sourcePaths = readFileSync(join(repo, 'Compiler/Bootstrap/sources.list'), 'utf8').split('\n').map(s => s.trim()).filter(s => s && !s.startsWith('#'));
  const sources = sourcePaths.map(path => clean(readFileSync(join(repo, path), 'utf8')));
  const golden = readFileSync(join(repo, 'Compiler/JetBackend/Tests/GoldenLower.jet'), 'utf8').replace(/\nfn run\(\) \{[\s\S]*$/, '\n');
  sources.push(clean(golden), jetDecoder(schema), jetWriter(schema), clean(readFileSync(join(repo, 'Compiler/JetBackend/Tests/FrontFeed.jet'), 'utf8')));
  writeFileSync(join(probeDir, 'unit.jet'), sources.join('\n\n'));
  writeFileSync(join(probeDir, 'package.jet'), 'name: "native_front_feed"\nversion: "0.0.1"\nedition: "2028"\n');
  writeFileSync(join(probeDir, 'front-cases.txt'), cases.map(c => `${c.file}\t${c.prefix}`).join('\n') + '\n');
  if (options.assemble) { save('prepared'); } else {
    const pack = process.env.JET_RUNTIME_PACK; if (!pack || !existsSync(pack)) throw new Error('probe requires JET_RUNTIME_PACK');
    writeFileSync(join(probeDir, 'runtime-pack.txt'), resolve(pack) + '\n');
    const hash = createHash('sha256');
    for await (const chunk of createReadStream(resolve(options.probe))) hash.update(chunk);
    const version = readFileSync(join(repo, 'Cargo.toml'), 'utf8').match(/^version = "([^"]+)"$/m)?.[1];
    if (!version) throw new Error('compiler package has no exact version');
    const compilerIdentity = `jet@${version}#${hash.digest('hex')}`;
    writeFileSync(join(probeDir, 'compiler-identity.txt'), compilerIdentity + '\n');
    // Run one case per process: a crashing compiler/backend must not hide the
    // rest of the selected census. Both feeds always use the identical pack.
    for (const c of cases) {
      for (const suffix of ['.rust.exe', '.jet.exe', '.jet.mird', '.rust.issues', '.jet.issues', '.jet.front-issues']) rmSync(c.prefix + suffix, { force: true });
      writeFileSync(`${c.prefix}.rust.want-lir`, '');
      writeFileSync(join(probeDir, 'front-cases.txt'), `${c.file}\t${c.prefix}\n`);
      const probe = spawnSync(resolve(options.probe), [], { cwd: probeDir, env: { ...process.env, JET_TOOLCHAIN_ROOT: repo, JET_COMPILER_IDENTITY: compilerIdentity }, timeout: Number(process.env.JET_FRONT_TIMEOUT_MS ?? 120000), encoding: 'utf8', maxBuffer: 1 << 27 });
      writeFileSync(`${c.prefix}.probe.log`, `${probe.stdout ?? ''}${probe.stderr ?? ''}`);
      let verdict = 'pass'; let detail = '';
      if (probe.status !== 0) { verdict = 'probe-error'; gap('probe', `exit ${probe.status}, signal ${probe.signal}`, c.stem, probe.error?.message ?? String(probe.stderr)); }
      const runs = [];
      for (const feed of ['rust', 'jet']) {
        const binary = `${c.prefix}.${feed}.exe`;
        if (!existsSync(binary)) {
          verdict = 'feed-error';
          const issues = [`${c.prefix}.${feed}.front-issues`, `${c.prefix}.${feed}.issues`].filter(existsSync).flatMap(path => readFileSync(path, 'utf8').split('\n').filter(Boolean));
          if (!issues.length) issues.push(`probe exited ${probe.status} signal ${probe.signal} without ${feed} executable`);
          for (const issue of issues) gap(feed === 'jet' ? 'jet-feed' : 'rust-feed', issue, c.stem, issue);
          continue;
        }
        const runDir = join(out, 'exec', c.slug, feed);
        rmSync(runDir, { recursive: true, force: true });
        copyFeatureProject(c.work, runDir);
        const run = spawnSync(binary, [], { cwd: runDir, input: c.stdin, timeout: Number(process.env.JET_FRONT_TIMEOUT_MS ?? 120000), maxBuffer: 1 << 27 });
        writeFileSync(`${c.prefix}.${feed}.stdout`, run.stdout ?? Buffer.alloc(0)); writeFileSync(`${c.prefix}.${feed}.stderr`, run.stderr ?? Buffer.alloc(0));
        runs.push({ feed, run });
        if (run.status !== 0 || !Buffer.from(run.stdout ?? '').equals(c.expected)) { verdict = 'wrong'; gap(`${feed}-output`, run.status !== 0 ? `exit ${run.status}, signal ${run.signal}` : 'stdout differs from golden', c.stem, String(run.stderr)); }
      }
      if (runs.length === 2 && (runs[0].run.status !== runs[1].run.status || !Buffer.from(runs[0].run.stdout ?? '').equals(Buffer.from(runs[1].run.stdout ?? '')) || !Buffer.from(runs[0].run.stderr ?? '').equals(Buffer.from(runs[1].run.stderr ?? '')))) { verdict = 'different'; gap('feed-output', 'Rust and Jet feed outputs/status differ', c.stem, 'see retained stdout/stderr'); }
      if (existsSync(`${c.prefix}.jet.mird`) && existsSync(`${c.prefix}.rust.mird`)) {
        try {
          const rust = decodeStream(readFileSync(`${c.prefix}.rust.mird`, 'utf8'), schema); const jet = decodeStream(readFileSync(`${c.prefix}.jet.mird`, 'utf8'), schema);
          writeFileSync(`${c.prefix}.jet.mir.json`, JSON.stringify(jet, null, 2)); writeFileSync(`${c.prefix}.rust.mir.json`, JSON.stringify(rust, null, 2));
          compareFacts(rust.program, jet.program, c.stem);
        } catch (error) {
          verdict = 'observation-error'; gap('observation', error.message, c.stem, error.message);
        }
      } else gap('observation', 'both checked MIR inventories unavailable', c.stem, 'see per-feed frontend/conversion errors');
      results.push({ stem: c.stem, verdict, detail }); save('differential');
    }
    process.exitCode = save('differential').differential_verified ? 0 : 1;
  }
}

function compareFacts(rust, jet, stem) {
  // Function/type IDs are source-authority-relative, not portable names. Pair
  // unique declaration names; retain raw MIR next to the semantic gap table.
  const tables = ['types', 'fields', 'functions', 'core_calls', 'prelude_calls', 'type_instances'];
  rust = symbolic(rust); jet = symbolic(jet);
  for (const table of tables) {
    const key = row => table === 'fields' ? row.field?.name ?? row.name : table === 'core_calls' || table === 'prelude_calls' ? `${row.module_name}.${row.member}` : row.name ?? row.kind?.variant;
    const group = rows => { const map = new Map(); for (const row of rows ?? []) { const k = key(row); if (!map.has(k)) map.set(k, []); map.get(k).push(row); } return map; };
    const a = group(rust[table]); const b = group(jet[table]);
    for (const name of new Set([...a.keys(), ...b.keys()])) {
      const left = a.get(name) ?? []; const right = b.get(name) ?? [];
      if (left.length !== right.length) gap(table, `${name}: inventory count differs`, stem, { rust: left.length, jet: right.length });
      if (left.length === 1 && right.length === 1) diff(left[0], right[0], `${table}.${name}`);
      else if (left.length && right.length) gap('observation', `${table}.${name}: ambiguous semantic pairing`, stem, 'raw field-complete MIR retained; do not pair by unstable IDs');
    }
  }
  function symbolic(program) {
    const refs = new Map();
    for (const table of tables) {
      const counts = new Map();
      const rows = program[table] ?? [];
      const label = row => table === 'core_calls' || table === 'prelude_calls' ? `${row.module_name}.${row.member}` : row.name ?? row.field?.name;
      for (const row of rows) { const name = label(row); counts.set(name, (counts.get(name) ?? 0) + 1); }
      for (const row of rows) if (counts.get(label(row)) === 1 && row.id?.value) refs.set(row.id.value, `${table}.${label(row)}`);
    }
    for (const fn of program.functions ?? []) {
      for (const table of ['values', 'places', 'locals', 'blocks', 'scopes']) (fn[table] ?? []).forEach((row, index) => {
        if (row.id?.value) refs.set(row.id.value, `functions.${fn.name}.${table}.${index}`);
        if (table === 'blocks') (row.instructions ?? []).forEach((instruction, i) => { if (instruction.id?.value) refs.set(instruction.id.value, `functions.${fn.name}.blocks.${index}.instructions.${i}`); });
      });
    }
    const visit = value => {
      if (Array.isArray(value)) return value.map(visit);
      if (!value || typeof value !== 'object') return value;
      if (Object.keys(value).length === 1 && refs.has(value.value)) return { ref: refs.get(value.value) };
      return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, visit(item)]));
    };
    return visit(program);
  }
  function diff(a, b, path) {
    if (/\.(id|identity|module_id|source_file|span|key|digest|value_id|place_id|block_id|scope_id)$/.test(path)) return;
    if (JSON.stringify(a) === JSON.stringify(b)) return;
    if (Array.isArray(a) && Array.isArray(b)) {
      if (a.length !== b.length) gap('MIR', `${path}.length`, stem, { rust: a.length, jet: b.length });
      for (let i = 0; i < Math.min(a.length, b.length); i++) diff(a[i], b[i], `${path}[${i}]`);
      return;
    }
    if (a && b && typeof a === 'object' && typeof b === 'object' && !Array.isArray(a) && !Array.isArray(b)) { for (const key of new Set([...Object.keys(a), ...Object.keys(b)])) diff(a[key], b[key], `${path}.${key}`); }
    else gap(/ownership|drops|\.drop/.test(path) ? 'drops' : /layout|abi|signature|access|params|return_type|failure/.test(path) && !/generic_params/.test(path) ? 'ABI-layout' : /generic_params|type_params|type_args|type_instances/.test(path) ? 'generics-mono' : /core_calls|prelude_calls/.test(path) ? 'runtime-routes' : 'MIR', path, stem, { rust: a, jet: b });
  }
}
