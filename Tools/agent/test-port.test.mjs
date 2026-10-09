import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, existsSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { functionsIn, collectFunctions, censusTsv, classify, checkOriginal, emitFunction, draftFunction, makeReference, machineDiagnostics, analyzeFunction, main } from './test-port.mjs';

function fixture(t) {
  const root = mkdtempSync(join(process.env.JET_SCRATCH_ROOT ?? tmpdir(), 'test-port-unit-'));
  mkdirSync(join(root, 'tests'), { recursive: true });
  t.after(() => rmSync(root, { recursive: true, force: true })); return root;
}
const fn = (body) => classify(functionsIn(`#[test]\nfn example() { ${body} }`, 'tests/sample.rs')[0]);
const diagnostic = { code: 'E0209', what: 'must copy value', why: 'one owner', fix: '~value', severity: 'error' };
const reference = async (source, mode) => ({ source, mode, code: 1, diagnostics: [diagnostic], rendered: 'Error [E0209]: must copy value\n' });

test('lexical census ignores planted tests in raw strings and nested comments', () => {
  const rows = functionsIn('/* outer /* #[test] fn fake() {} */ */\n#[test]\nfn real() { let src = r###"#[test] fn fake() {}"###; }\n#[test] fn second() {}');
  assert.deepEqual(rows.filter((row) => row.test).map((row) => row.name), ['real', 'second']);
  assert.equal(rows[0].line, 2);
});

test('Rust identifiers cannot collide with JavaScript delimiter prototypes', () => {
  assert.equal(functionsIn('#[test] fn real() { for (constructor, toString) in cases {} }').filter((row) => row.test).length, 1);
});

test('mod part, path include and physical deduplication', (t) => {
  const root = fixture(t); mkdirSync(join(root, 'tests/parts'));
  writeFileSync(join(root, 'tests/root.rs'), 'mod parts; #[path = "parts/one.rs"] mod alias;');
  writeFileSync(join(root, 'tests/parts/mod.rs'), 'mod one;');
  writeFileSync(join(root, 'tests/parts/one.rs'), '#[test] fn in_part() {}');
  const rows = collectFunctions(root);
  assert.equal(rows.files.length, 3); assert.equal(rows.functions.length, 1);
  assert.equal(rows.functions[0].name, 'in_part'); assert.equal(rows.modules.length, 3);
  assert.match(censusTsv(rows), /tests\/parts\/one.rs\tin_part\t1-1/u);
  writeFileSync(join(root, 'tests/strings.rs'), 'const X: &str = r#"#[test] fn fake() {}"#;');
  assert.deepEqual(collectFunctions(root).excluded, [{ file: 'tests/strings.rs', line: 1, reason: 'inside string or comment' }]);
});

test('missing inventory entry and unresolved module refuse census', (t) => {
  const root = fixture(t), inventory = join(root, 'inventory.tsv');
  writeFileSync(inventory, 'file\nmissing.rs\n'); assert.throws(() => collectFunctions(root, inventory), /inventory file missing/u);
  writeFileSync(join(root, 'tests/test.rs'), 'mod missing;'); assert.throws(() => collectFunctions(root), /unresolved mod/u);
});

test('per-function dispositions separate mixed surfaces and local helpers', () => {
  assert.equal(classify(fn('let x = jet::compile("fn run() {}").expect("ok");')).disposition, 'UI');
  assert.equal(classify(fn('jet::compile("fn run() {}").expect_err("must not defer to rustc");')).disposition, 'UI');
  assert.equal(classify(fn('let x = jet::compile("fn run() {}").unwrap(); assert!(x.rust.contains("x"));')).disposition, 'retire-with-Rust');
  assert.equal(classify(fn('let x = common::build_and_run("a", "b", "fn run() {}");')).disposition, 'golden');
  assert.equal(classify(fn('Command::new(env!("CARGO_BIN_EXE_jet"));')).disposition, 'CLI case');
  assert.equal(classify(fn('read_dir("Compiler");')).disposition, 'guard');
  const rows = functionsIn('fn helper() { common::build_and_run("a", "b", "fn run() {}"); } #[test] fn calls_helper() { helper(); }', 'tests/sample.rs');
  assert.equal(classify(rows[1], [rows[0]]).disposition, 'golden');
});

test('checks every code, positive and negative substring assertion', async () => {
  const row = fn('let src = r#"fn run() {}"#; let diags = jet::compile(src).expect_err("error"); assert!(diags.iter().any(|d| d.code == "E0209")); assert!(diags.iter().all(|d| !d.fix.contains("bad"))); let d = diags.iter().find(|d| d.code == "E0209").expect("code"); assert!(d.what.contains("copy")); assert_eq!(d.why, "one owner");');
  const result = await checkOriginal(row, reference); assert.equal(result.assertions, 5);
});

test('planted weakening is refused before any case file is written', async (t) => {
  const root = fixture(t);
  const row = fn('let src = "fn run() {}"; let diags = jet::compile(src).expect_err("bad"); assert!(diags.iter().any(|d| d.code == "E0209")); assert!(!diags.iter().any(|d| d.fix.contains("~value")));');
  await assert.rejects(emitFunction(row, { root, reference }), /not satisfied/u);
  assert.equal(existsSync(join(root, 'tests/ui')), false);
  const missingCode = async (source, mode) => ({ source, mode, code: 1, diagnostics: [{ ...diagnostic, code: 'E0003' }], rendered: 'Error [E0003]\n' });
  await assert.rejects(checkOriginal(row, missingCode), /not satisfied/u);
});

test('unsupported assertion or hidden control flow cannot be discarded', async () => {
  for (const body of [
    'jet::compile("fn run() {}").unwrap(); assert!(out.rust.contains("foo"));',
    'jet::compile("fn run() {}").unwrap(); if false { assert!(false); }',
    'jet::compile("fn run() {}").unwrap(); assert!(false || mystery());',
    'jet::compile("fn run() {}").unwrap(); assert!(diags[0].span.is_some());',
  ]) await assert.rejects(checkOriginal(fn(body), reference));
});

test('compile-only and successful-lint expectations check exit status', async () => {
  const row = fn('let out = jet::compile("fn run() {}").expect("ok"); assert!(out.lints.iter().any(|d| d.code == "L0202"));');
  await assert.rejects(checkOriginal(row, reference), /expect not satisfied/u);
  const ok = async (source, mode) => ({ source, mode, code: 0, diagnostics: [{ ...diagnostic, code: 'L0202' }], rendered: 'Warning [L0202]\n' });
  assert.equal((await checkOriginal(row, ok)).observations.length, 1);
});

test('matches alternatives and compound diagnostic predicates are evaluated on one diagnostic', async () => {
  const row = fn('let diags = jet::compile("fn run() {}").expect_err("bad"); assert!(diags.iter().any(|d| { matches!(d.code.as_str(), "E0205" | "E0209") && d.what.contains("copy") }));');
  await checkOriginal(row, reference);
  const split = async (source, mode) => ({ source, mode, code: 1, diagnostics: [{ ...diagnostic, what: 'different' }, { ...diagnostic, code: 'E0003' }] });
  await assert.rejects(checkOriginal(row, split), /not satisfied/u);
});

test('multi-source function does not write first case when second assertion fails', async (t) => {
  const root = fixture(t);
  const row = fn('jet::compile("fn run() {}").expect_err("bad"); let d = jet::compile("fn run() {print(0)}").expect_err("bad"); assert!(d.iter().any(|x| x.code == "E0003"));');
  await assert.rejects(emitFunction(row, { root, reference }), /not satisfied/u);
  assert.equal(existsSync(join(root, 'tests/ui')), false);
});

test('include_str source, dry-run, exact streams and overwrite refusal', async (t) => {
  const root = fixture(t); writeFileSync(join(root, 'tests/source.jet'), 'fn run() {}');
  const row = fn('let src = include_str!("source.jet"); let (code, stdout, stderr) = common::build_and_run("a", "b", src); assert_eq!(code, 0); assert_eq!(stdout, "ok\\n"); assert!(stderr.is_empty());');
  const run = async (source, mode) => ({ source, mode, code: 0, stdout: 'ok\n', stderr: '' });
  const dry = await emitFunction(row, { root, reference: run, dryRun: true });
  assert.equal(dry.status, 'CONVERTED'); assert.equal(existsSync(join(root, dry.paths[0])), false);
  const emitted = await emitFunction(row, { root, reference: run });
  assert.equal(readFileSync(join(root, emitted.paths[1]), 'utf8'), 'ok\n');
  assert.ok(emitted.paths.includes('tests/run/sample/example.stderr.out'));
  assert.equal(existsSync(join(root, 'tests/run/sample/example.err.out')), false);
  await assert.rejects(emitFunction(row, { root, reference: run }), /already exists/u);
});

test('Rust UTF-8 lengths and whitespace are not weakened to JavaScript string semantics', async () => {
  const setup = 'let (code, stdout, stderr) = common::build_and_run("a", "b", "fn run() {}"); ';
  const run = async (source, mode) => ({ source, mode, code: 0, stdout: 'é', stderr: '' });
  await checkOriginal(fn(setup + 'assert_eq!(stdout.len(), 2);'), run);
  await assert.rejects(checkOriginal(fn(setup + 'assert_eq!(stdout.len(), 1);'), run), /not satisfied/u);
  const bom = async (source, mode) => ({ source, mode, code: 0, stdout: '\\uFEFFx', stderr: '' });
  await assert.rejects(checkOriginal(fn(setup + 'assert_eq!(stdout.trim(), "x");'), bom), /not satisfied/u);
  await assert.rejects(checkOriginal(fn(setup + 'assert_eq!(9007199254740992, 9007199254740993);'), run), /exact integer/u);
});

test('missing diagnostic selection, exact counts and negated codes are checked', async () => {
  await assert.rejects(checkOriginal(fn('let d = jet::compile("fn run() {}").expect_err("bad"); d.iter().find(|d| d.code == "E9999").expect("missing");'), reference), /no matching diagnostic/u);
  await checkOriginal(fn('let d = jet::compile("fn run() {}").expect_err("bad"); assert_eq!(d.len(), 1); assert!(d.iter().all(|d| d.code != "E0003"));'), reference);
});

test('finite source matrices check every iteration, typed counts and template expansion', async (t) => {
  const root = fixture(t), seen = [];
  const row = fn('for (name, code) in [("first", "E0209"), ("second", "E0209")] { let src = format!("fn run() {{ print(\\\"{name}\\\") }}"); let d = jet::compile(&src).expect_err("bad"); let selected: Vec<_> = d.iter().filter(|x| x.code == code).collect(); assert_eq!(selected.len(), 1); }');
  const observe = async (source, mode, metadata) => { seen.push([source, metadata.path]); return reference(source, mode); };
  const result = await emitFunction(row, { root, reference: observe });
  assert.deepEqual(seen, [['fn run() { print("first") }', 'tests/ui/sample/example.jet'], ['fn run() { print("second") }', 'tests/ui/sample/example_2.jet']]);
  assert.equal(result.sources, 2);
});

test('a weakened later matrix row refuses the whole write set', async (t) => {
  const root = fixture(t);
  const row = fn('for code in ["E0209", "E0003"] { let d = jet::compile("fn run() {}").expect_err("bad"); assert!(d.iter().any(|x| x.code == code)); }');
  await assert.rejects(emitFunction(row, { root, reference }), /not satisfied/u);
  assert.equal(existsSync(join(root, 'tests/ui')), false);
});

test('plain UI staging keeps the final path and does not insert a directive', async (t) => {
  const root = fixture(t), seen = [];
  const row = fn('let d = jet::compile("fn run() {}").expect_err("bad"); assert!(d.iter().any(|d| d.code == "E0209"));');
  const observe = async (source, mode, metadata) => { seen.push(metadata); return reference(source, mode); };
  const emitted = await emitFunction(row, { root, reference: observe });
  assert.deepEqual(seen, [{ path: 'tests/ui/sample/example.jet', lint: false, agreement: false, interpreterAgreement: false }]);
  assert.equal(readFileSync(join(root, emitted.paths[0]), 'utf8'), 'fn run() {}');
});

test('verbose lint expectations use grouped UI lint and a warning snapshot', async (t) => {
  const root = fixture(t);
  const row = fn('let out = jet::compile("fn run() {}").expect("ok"); assert!(out.lints.iter().any(|d| d.code == "L0202"));');
  const observe = async (source, mode, metadata) => ({ source, mode, code: 0, lint: metadata.lint,
    diagnostics: [{ ...diagnostic, code: 'L0202', severity: 'warning' }], rendered: 'Warning [L0202]\n' });
  const emitted = await emitFunction(row, { root, reference: observe });
  assert.ok(emitted.paths.includes('tests/ui_lint/sample/example.warn'));
  assert.equal(readFileSync(join(root, 'tests/ui_lint/sample/example.warn'), 'utf8'), 'Warning [L0202]\n');
  assert.equal(existsSync(join(root, 'tests/cases')), false);
});

test('multi-file input assembly writes real scratch files and restages the complete project', async (t) => {
  const root = fixture(t), scratch = join(root, 'scratch'); mkdirSync(scratch);
  const binary = join(root, 'reference');
  writeFileSync(binary, '#!/usr/bin/env node\nconst fs=require("node:fs"), path=require("node:path"); const entry=process.argv.at(-1); if(path.basename(entry)!=="run.jet" || fs.readFileSync(path.join(path.dirname(entry),"helper.jet"),"utf8")!=="fn helper() {}") process.exit(101); if(process.argv.includes("--json")) console.log(JSON.stringify({schema:"jet.report/v3",moment:"compile",code:"E0209",what:"bad",why:"owner",fix:"copy",severity:"error"})); else console.error("Error [E0209]: bad"); process.exitCode=1;\n', { mode: 0o755 });
  const row = fn('let root = common::unique_tmp("input"); fs::create_dir_all(&root).unwrap(); fs::write(root.join("helper.jet"), "fn helper() {}").unwrap(); let entry = root.join("main.jet"); let src = "fn run() {}"; fs::write(&entry, src).unwrap(); let d = jet::compile_with_path(src, entry.to_str().unwrap()).expect_err("bad"); assert!(d.iter().any(|d| d.code == "E0209"));');
  const result = await emitFunction(row, { root, reference: makeReference(binary, root, scratch) });
  assert.ok(result.paths.includes('tests/ui/sample/example/run.jet'));
  assert.ok(result.paths.includes('tests/ui/sample/example/helper.jet'));
  assert.equal(readFileSync(join(root, 'tests/ui/sample/example/helper.jet'), 'utf8'), 'fn helper() {}');
});

test('reference JSON/rendered code disagreement refuses conversion', async (t) => {
  const root = fixture(t), scratch = join(root, 'scratch'); mkdirSync(scratch);
  // Use the public runner against an actual process, not an injected diagnostic.
  const binary = join(root, 'reference');
  writeFileSync(binary, '#!/usr/bin/env node\nif(process.argv.includes("--json")) { console.log(JSON.stringify({schema:"jet.report/v3",moment:"compile",code:"E0209",what:"bad",why:"owner",fix:"copy",severity:"error"})); } else { console.error("Error [E0003]: different"); } process.exitCode=1;\n', { mode: 0o755 });
  await assert.rejects(makeReference(binary, root, scratch)('fn run() {}', 'compile'), /JSON\/rendered/u);
});

test('success without a stderr assertion omits the stderr golden', async (t) => {
  const root = fixture(t);
  const row = fn('let (code, out, ignored) = common::build_and_run("a", "b", "fn run() {}"); assert_eq!(code, 0); assert_eq!(out, "ok");');
  const result = await emitFunction(row, { root, reference: async (source, mode) => ({ source, mode, code: 0, stdout: 'ok', stderr: '' }) });
  assert.deepEqual(result.paths, ['tests/run/sample/example.jet', 'tests/run/sample/example.out']);
});

test('failure goldens retain exact exit and both streams in a sidecar', async (t) => {
  const root = fixture(t);
  const row = fn('let (code, stdout, stderr) = common::build_and_run("a", "b", "fn run() {}"); assert_eq!(code, 70); assert_eq!(stdout, "before"); assert_eq!(stderr, "failure");');
  const result = await emitFunction(row, { root, reference: async (source, mode) => ({ source, mode, code: 70, stdout: 'before', stderr: 'failure' }) });
  assert.deepEqual(result.paths, ['tests/run/sample/example.jet', 'tests/run/sample/example.err.out', 'tests/run/sample/example.stdout', 'tests/run/sample/example.case']);
  assert.equal(readFileSync(join(root, 'tests/run/sample/example.case'), 'utf8'), 'step: built\nexit: 70\nstdout: = example.stdout\nstderr: = example.err.out\n');
  assert.equal(readFileSync(join(root, 'tests/run/sample/example.err.out'), 'utf8'), 'failure');
  assert.equal(readFileSync(join(root, 'tests/run/sample/example.stdout'), 'utf8'), 'before');
  assert.equal(existsSync(join(root, 'tests/run/sample/example.err')), false);
  assert.equal(existsSync(join(root, 'tests/run/sample/example.out')), false);
});

test('ref tier runs the debug-built artifact with input, and refuses I9 disagreement', async (t) => {
  const root = fixture(t), scratch = join(root, 'scratch'); mkdirSync(scratch);
  const binary = join(root, 'reference');
  writeFileSync(binary, `#!/usr/bin/env node
const fs=require("node:fs");
const path=require("node:path");
const args=process.argv.slice(2);
if(args[0]==="build") {
  if(!args.includes("--profile=debug")) process.exit(101);
  const entry=args[1], directory=path.join(path.dirname(entry),".jet/build");
  fs.mkdirSync(directory,{recursive:true});
  fs.writeFileSync(path.join(directory,path.basename(entry,".jet").replaceAll(".","_")), '#!/usr/bin/env node\\nprocess.stdout.write("ref:"+require("node:fs").readFileSync(0,"utf8"));\\n', {mode:0o755});
} else if(args[0]==="run") {
  const source=fs.readFileSync(args.at(-1),"utf8");
  process.stdout.write((source==="DIVERGE" ? "wrong:" : "ref:")+fs.readFileSync(0,"utf8"));
} else process.exit(101);
`, { mode: 0o755 });
  const observe = makeReference(binary, root, scratch);
  const result = await observe('AGREE', 'run', { stdin: 'input', agreement: true, interpreterAgreement: true });
  assert.equal(result.stdout, 'ref:input');
  await assert.rejects(observe('DIVERGE', 'run', { agreement: true }), /I9 ref\/default/u);
});

test('reference check environment and plain-success snapshot match the UI contract', async (t) => {
  const root = fixture(t), scratch = join(root, 'scratch'); mkdirSync(scratch);
  const previous = { extension: process.env.JET_COMPILER_EXTENSION, schema: process.env.JET_SCHEMA_CACHE_DIR };
  process.env.JET_COMPILER_EXTENSION = 'must-clear'; process.env.JET_SCHEMA_CACHE_DIR = 'must-clear';
  t.after(() => {
    for (const [name, value] of [['JET_COMPILER_EXTENSION', previous.extension], ['JET_SCHEMA_CACHE_DIR', previous.schema]]) {
      if (value === undefined) delete process.env[name]; else process.env[name] = value;
    }
  });
  const binary = join(root, 'reference');
  writeFileSync(binary, `#!/usr/bin/env node
const e=process.env,a=process.argv.slice(2);
if(e.NO_COLOR!=="1" || e.CLICOLOR!=="0" || e.TERM!=="dumb" || e.JET_RECEIPT_BYPASS!=="1" || e.JET_REPL_HISTORY!=="off" || e.JET_COMPILER_EXTENSION || e.JET_SCHEMA_CACHE_DIR || a.includes("--no-color")) process.exit(101);
if(a.includes("--json")) console.log(JSON.stringify({schema:"jet.status/v1",action:"diagnostics",ok:true,reports:[{schema:"jet.report/v3",moment:"compile",severity:"warning",code:"L0202",what:"lint",why:"lint",fix:"lint"}]}));
else { console.log("not a snapshot"); console.error("Warning [L0202]: lint"); }
`, { mode: 0o755 });
  const observe = makeReference(binary, root, scratch);
  assert.equal((await observe('fn run() {}', 'compile')).rendered, '(no errors)\n');
  assert.equal((await observe('fn run() {}', 'compile', { lint: true })).rendered, 'Warning [L0202]: lint\n');
  writeFileSync(binary, '#!/usr/bin/env node\nif(process.argv.includes("--json")) console.log(JSON.stringify({schema:"jet.status/v1",action:"diagnostics",ok:true,reports:[]}));\n', { mode: 0o755 });
  await assert.rejects(observe('fn run() {}', 'compile', { lint: true }), /at least one lint/u);
});

test('project siblings cannot refer to the renamed entry', async (t) => {
  const root = fixture(t), scratch = join(root, 'scratch'); mkdirSync(scratch);
  const row = fn('let root = common::unique_tmp("input"); fs::write(root.join("helper.jet"), "import \\\"./main\\\";").unwrap(); let entry = root.join("main.jet"); let src = "fn run() {}"; fs::write(&entry, src).unwrap(); jet::compile_with_path(src, entry.to_str().unwrap()).expect_err("bad");');
  await assert.rejects(emitFunction(row, { root, reference: makeReference('/not-invoked', root, scratch) }), /references renamed entry/u);
  assert.equal(existsSync(join(root, 'tests/ui')), false);
});

test('mixed functions retire Rust aliases and closures, not Jet diagnostic facts', async (t) => {
  const root = fixture(t);
  const setup = 'let out = jet::compile("fn run() {}").expect("ok"); let code = out.rust; let includes = |needle: &str| code.contains(needle); assert!(includes("fn")); ';
  const row = fn(setup + 'assert!(out.lints.iter().any(|d| d.code == "L0202"));');
  assert.equal(classify(row).disposition, 'UI+retire-rust-asserts');
  const observe = async (source, mode, metadata) => ({ source, mode, code: 0, lint: metadata.lint, diagnostics: [{ ...diagnostic, code: 'L0202' }], rendered: 'Warning [L0202]\n' });
  const result = await emitFunction(row, { root, reference: observe, dryRun: true });
  assert.equal(result.retiredAssertions, 1);
  await assert.rejects(emitFunction(fn(setup + 'assert!(out.lints.iter().any(|d| d.code == "L9999"));'), { root, reference: observe }), /not satisfied/u);
  const pure = fn('let out = jet::compile("fn run() {}").unwrap(); assert!(out.rust.contains("fn"));');
  assert.equal(classify(pure).disposition, 'retire-with-Rust');
  await assert.rejects(emitFunction(pure, { root, reference: observe }), /must be retired/u);
});

test('compound Rust retirement keeps Jet predicates and closure shadowing', async () => {
  const row = fn('let out = jet::compile("fn run() {}").expect("ok"); let rust = out.rust; assert!(rust.contains("fn") && out.lints.iter().any(|rust| rust.code == "L0202"));');
  const good = async (source, mode) => ({ source, mode, code: 0, diagnostics: [{ ...diagnostic, code: 'L0202' }] });
  assert.equal((await checkOriginal(row, good)).retiredAssertions.length, 1);
  await assert.rejects(checkOriginal(row, async (source, mode) => ({ source, mode, code: 0, diagnostics: [] })), /not satisfied/u);
});

test('quoted Rust punctuation is a literal, never a parser delimiter or operator', async () => {
  const setup = 'let (code, stdout, stderr) = common::build_and_run("a", "b", "fn run() {}"); ';
  const observe = async (source, mode) => ({ source, mode, code: 0, stdout: '(', stderr: '' });
  await checkOriginal(fn(setup + 'assert_eq!(stdout, "("); assert!(stdout.contains("("));'), observe);
  for (const marker of ['!', ')', '&&']) await assert.rejects(checkOriginal(fn(setup + `assert!(stdout.contains("${marker}"));`), observe), /not satisfied/u);
});

test('canonical observed machine envelopes accept success and nested errors only', () => {
  const error = { schema: 'jet.report/v3', moment: 'compile', severity: 'error', code: 'E0104', what: '`show` expects 1 argument, got 2', why: 'Every argument must match a parameter', fix: 'Check the definition of `show`', no_fix_reason: { kind: 'ambiguous', next: 'Check the definition of `show`' } };
  const run = (row, code = 0) => ({ stdout: JSON.stringify(row), stderr: '', code });
  assert.deepEqual(machineDiagnostics(run({ schema: 'jet.status/v1', action: 'diagnostics', ok: false, reports: [error] }, 1)), [error]);
  assert.deepEqual(machineDiagnostics(run(error, 1)), [error]);
  const success = { status: 'passed', contract: 'jet.check/v1', schema_version: 1, provenance: { diagnostics: 0 }, rows: [{ output: 'not-applicable', name: 'entry resolution', status: 'not applicable', detail: 'explicit-file checks resolve only the named source file', diagnostic: 'E2389' }] };
  assert.deepEqual(machineDiagnostics(run(success)), []);
  assert.throws(() => machineDiagnostics(run({ ...success, status: 'failed' }, 1)), /success record/u);
  assert.throws(() => machineDiagnostics(run({ ...success, rows: [{ ...success.rows[0], status: 'failed' }] })), /proof row/u);
  assert.throws(() => machineDiagnostics(run({ ...success, contract: 'legacy.check/v1' })), /unknown machine/u);
  assert.throws(() => machineDiagnostics({ stdout: '', stderr: '', code: 0 }), /no machine record/u);
});

test('stderr assertions survive renamed stream bindings, aliases and direct tuples', async (t) => {
  const root = fixture(t);
  const observe = async (source, mode) => ({ source, mode, code: 0, stdout: 'ok', stderr: '' });
  for (const body of [
    'let (c, o, e) = common::build_and_run("a", "b", "fn run() {}"); let alias = e; assert!(alias.is_empty());',
    'assert_eq!(common::build_and_run("a", "b", "fn run() {}"), (0, "ok", ""));',
  ]) {
    const result = await emitFunction(fn(body), { root, reference: observe, dryRun: true });
    assert.ok(result.paths.includes('tests/run/sample/example.stderr.out'));
  }
});

test('reference timeout is ERROR, excluded from hand-port/eligibility, and exits nonzero', async (t) => {
  const root = fixture(t), binary = join(root, 'sleeping-reference'), reportPath = join(root, 'report.json');
  writeFileSync(join(root, 'tests/sample.rs'), '#[test] fn sleeps() { jet::compile("fn run() {}").expect("ok"); }');
  writeFileSync(binary, '#!/usr/bin/env node\nsetTimeout(() => {}, 1000);\n', { mode: 0o755 });
  const code = await main(['--root', root, '--scratch', root, '--emit', 'tests/sample.rs::*', '--bless-from', binary, '--timeout', '0.02', '--dry-run', '--report', reportPath]);
  const report = JSON.parse(readFileSync(reportPath, 'utf8'));
  assert.equal(code, 2);
  assert.equal(report.summary.errors, 1);
  assert.equal(report.summary.handPort, 0);
  assert.equal(report.summary.eligible, 0);
  assert.equal(report.summary.timeout_ms, 20);
  const row = report.results[0];
  assert.equal(row.status, 'ERROR');
  assert.equal(row.mechanicallyEligible, false);
  assert.match(row.reason, /ETIMEDOUT/u);
  assert.equal(row.referenceInvocations.length, 1);
  assert.ok(row.referenceInvocations[0].elapsed_ms >= 0);
  assert.equal(row.elapsed_ms, row.referenceInvocations[0].elapsed_ms);
  assert.equal(existsSync(join(root, 'tests/ui')), false);
});

test('success-with-warnings non-lint schema mismatch has its own blocker class', async (t) => {
  const root = fixture(t), scratch = join(root, 'scratch'); mkdirSync(scratch);
  const binary = join(root, 'reference');
  writeFileSync(binary, '#!/usr/bin/env node\nif(process.argv.includes("--json")) console.log(JSON.stringify({status:"passed",contract:"jet.check/v1",schema_version:1,provenance:{diagnostics:1},rows:[]})); else console.error("Warning [L0202]: lint");\n', { mode: 0o755 });
  await assert.rejects(makeReference(binary, root, scratch)('fn run() {}', 'compile'), /success-with-warnings, non-lint/u);
});

test('reference artifacts use nearest package root and dotted entry stem without progress parsing', async (t) => {
  const root = fixture(t), scratch = join(root, 'scratch'); mkdirSync(scratch);
  const binary = join(root, 'reference');
  writeFileSync(binary, `#!/usr/bin/env node
const fs=require("node:fs"), path=require("node:path"), entry=process.argv[3];
const project=path.dirname(path.dirname(entry));
if(!fs.existsSync(path.join(project,"package.jet"))) process.exit(101);
const directory=path.join(project,".jet/build");
fs.mkdirSync(directory,{recursive:true});
fs.writeFileSync(path.join(directory,path.basename(entry,".jet").replaceAll(".","_")), '#!/usr/bin/env node\\nprocess.stdout.write("package artifact");\\n', {mode:0o755});
console.log("progress display, not an artifact receipt");
`, { mode: 0o755 });
  const observe = makeReference(binary, root, scratch);
  const result = await observe('fn run() {}', 'run', {
    path: 'project/nested/main.extra.jet',
    project: { entry: 'project/nested/main.extra.jet', files: [{ path: 'project/package.jet', content: 'name: "sample"\nversion: "0.1.0"\n' }] },
  });
  assert.equal(result.stdout, 'package artifact');
  writeFileSync(binary, '#!/usr/bin/env node\n', { mode: 0o755 });
  await assert.rejects(observe('fn run() {}', 'run'), (error) => {
    assert.equal(error.status, 'ERROR');
    assert.match(error.message, /missing artifact: .*test_port\/\.jet\/build\/run$/u);
    return true;
  });
});

test('exit-101 reference retains panic stderr in the reason and JSON receipt', async (t) => {
  const root = fixture(t), binary = join(root, 'panicking-reference'), reportPath = join(root, 'report.json');
  const panic = "thread 'main' panicked at compiler.rs:17: ICE witness";
  writeFileSync(join(root, 'tests/sample.rs'), '#[test] fn panics() { jet::compile("fn run() {}").expect("ok"); }');
  writeFileSync(binary, `#!/usr/bin/env node\nprocess.stderr.write(${JSON.stringify(`${panic}\nextra context\n`)}); process.exit(101);\n`, { mode: 0o755 });
  const code = await main(['--root', root, '--scratch', root, '--emit', 'tests/sample.rs::*', '--bless-from', binary, '--dry-run', '--report', reportPath]);
  const report = JSON.parse(readFileSync(reportPath, 'utf8')), row = report.results[0];
  assert.equal(code, 2);
  assert.equal(row.status, 'ERROR');
  assert.ok(row.reason.includes(panic));
  assert.equal(row.referenceInvocations[0].code, 101);
  assert.equal(row.referenceInvocations[0].stderr, `${panic}\nextra context\n`);
  assert.equal(existsSync(join(root, 'tests/ui')), false);
});

test('human PORT attempt bypasses only census retirement, never a failed assertion', async (t) => {
  const root = fixture(t), binary = join(root, 'reference'), reportPath = join(root, 'report.json'), attempt = join(root, 'attempt.tsv');
  const source = `#[test] fn accepts() {
    let out = jet::compile("fn run() {}").expect("ok");
    assert!(out.rust.contains("fn main"));
  }
  #[test] fn fails() {
    jet::compile("fn run() {}").expect("ok");
    assert!(false);
  }
  #[test] fn untouched() { assert!(true); }`;
  writeFileSync(join(root, 'tests/selfhost_parser.rs'), source);
  writeFileSync(attempt, '# Human PORT rulings\nfunction\ntests/selfhost_parser.rs::accepts\ntests/selfhost_parser.rs::fails\n');
  writeFileSync(binary, '#!/usr/bin/env node\nif(process.argv.includes("--json")) console.log(JSON.stringify({status:"passed",contract:"jet.check/v1",schema_version:1,provenance:{diagnostics:0},rows:[]}));\n', { mode: 0o755 });
  const original = functionsIn(source, 'tests/selfhost_parser.rs');
  assert.equal(classify(original[0]).disposition, 'retire-with-Rust');
  assert.equal(classify(original[1]).disposition, 'retire-with-Rust');
  const code = await main(['--root', root, '--scratch', root, '--attempt', attempt, '--bless-from', binary, '--report', reportPath]);
  const report = JSON.parse(readFileSync(reportPath, 'utf8'));
  assert.equal(code, 2);
  assert.equal(report.summary.selected, 2);
  assert.equal(report.summary.converted, 1);
  assert.equal(report.summary.handPort, 1);
  assert.equal(report.summary.retired, 0);
  const [accepted, failed] = report.results;
  assert.equal(accepted.status, 'CONVERTED');
  assert.equal(accepted.attempted, true);
  assert.equal(accepted.retiredAssertions, 1);
  assert.ok(existsSync(join(root, 'tests/ui/selfhost_parser/accepts.jet')));
  assert.equal(failed.status, 'HAND-PORT');
  assert.equal(failed.attempted, true);
  assert.match(failed.reason, /original assert! not satisfied/u);
  assert.equal(existsSync(join(root, 'tests/ui/selfhost_parser/fails.jet')), false);
});

test('attempt and emit selections intersect and unknown human rows are refused', async (t) => {
  const root = fixture(t), attempt = join(root, 'attempt.tsv'), reportPath = join(root, 'report.json');
  writeFileSync(join(root, 'tests/sample.rs'), '#[test] fn first() { assert!(true); }\n#[test] fn second() { assert!(false); }');
  writeFileSync(attempt, 'tests/sample.rs::first\ntests/sample.rs::second\n');
  const code = await main(['--root', root, '--scratch', root, '--attempt', attempt, '--emit', 'tests/sample.rs::second', '--bless-from', '/unused', '--dry-run', '--report', reportPath]);
  const report = JSON.parse(readFileSync(reportPath, 'utf8'));
  assert.equal(code, 2);
  assert.equal(report.summary.selected, 1);
  assert.equal(report.results[0].function, 'tests/sample.rs::second');
  assert.equal(report.results[0].status, 'HAND-PORT');
  writeFileSync(attempt, 'tests/sample.rs::missing\n');
  await assert.rejects(main(['--root', root, '--attempt', attempt, '--bless-from', '/unused']), /attempt function not found/u);
});

test('emission keeps the helper-aware disposition computed by its caller', async (t) => {
  const root = fixture(t);
  const rows = functionsIn(`fn rust_oracle() -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_jet")).args(["emit", "--rust"]).output().unwrap();
    String::from_utf8(output.stdout).unwrap()
  }
  #[test] fn acceptance() {
    let out = jet::compile("fn run() {}").expect("ok");
    assert_eq!(out.rust, rust_oracle());
  }`, 'tests/sample.rs');
  const original = rows.find(row => row.test), helpers = rows.filter(row => !row.test);
  assert.equal(classify(original).disposition, 'retire-with-Rust');
  const classified = classify(original, helpers);
  assert.equal(classified.disposition, 'CLI case+retire-rust-asserts');
  const observe = async (source, mode) => ({ source, mode, code: 0, diagnostics: [], rendered: '(no errors)\n' });
  const result = await emitFunction(classified, { root, reference: observe });
  assert.equal(result.status, 'CONVERTED');
  assert.equal(result.assertions, 1);
  assert.equal(result.retiredAssertions, 1);
  assert.equal(readFileSync(join(root, 'tests/ui/sample/acceptance.jet'), 'utf8'), 'fn run() {}');
  await assert.rejects(emitFunction(original, { root, reference: observe }), /requires a classified function/u);
});

test('analysis collects all three independent blockers and a clean function has none', () => {
  const row = fn('let a = first_helper(); let b = second_helper(); third_helper();');
  assert.deepEqual(analyzeFunction(row).classes, ['unmodeled call first_helper', 'unmodeled call second_helper', 'unmodeled call third_helper']);
  assert.deepEqual(analyzeFunction(fn('let d = jet::compile("fn run() {}").expect_err("bad"); assert_eq!(d.len(), 1);')).classes, []);
  assert.equal(analyzeFunction(row).reference, 'runtime-unknown');
});

test('analysis recovers nested syntax blockers without discarding later calls or assertions', () => {
  const result = analyzeFunction(fn('if true { hidden_helper(); } let a = first_helper(); let b = a.unknown_method();'));
  assert.ok(result.classes.includes('unmodeled call hidden_helper'));
  assert.ok(result.classes.includes('unmodeled call first_helper'));
  assert.ok(result.classes.includes('unmodeled method unknown_method'));
  assert.ok(result.classes.some(name => name.startsWith('expected ;, got')));
  assert.deepEqual(analyzeFunction(fn('assert!(true, message_helper());')).classes, []);
  assert.deepEqual(analyzeFunction(fn('let d = jet::compile(SOURCE).unwrap();')).classes, ['unbound value SOURCE']);
});

test('analyze CLI writes only the requested static TSV and never needs a reference', async (t) => {
  const root = fixture(t), output = join(root, 'analysis.tsv');
  writeFileSync(join(root, 'tests/sample.rs'), '#[test] fn clean() { jet::compile("fn run() {}").unwrap(); }\\n#[test] fn blocked() { one(); two(); three(); }'.replaceAll('\\n', '\n'));
  assert.equal(await main(['--root', root, '--analyze', output]), 0);
  const rows = readFileSync(output, 'utf8').trimEnd().split('\n');
  assert.equal(rows.length, 3);
  assert.match(rows[1], /clean\t-\t\truntime-unknown$/u);
  assert.match(rows[2], /unmodeled call one;unmodeled call three;unmodeled call two/u);
  assert.equal(existsSync(join(root, 'tests/ui')), false);
  assert.equal(existsSync(join(root, 'test-port-')), false);
});

test('E2 imported aliases preserve two/three streams and all-tier agreement flags', async () => {
  for (const [module, args, outputs] of [
    ['tir_support', '"name", "fn run() {}"', '(0, "ok")'],
    ['common', '"prefix", "name", "fn run() {}"', '(0, "ok", "")'],
  ]) {
    const row = functionsIn(`use ${module}::build_and_run as execute;
      #[test] fn example() { assert_eq!(execute(${args}), ${outputs}); }`, 'tests/sample.rs')[0];
    const checked = await checkOriginal(row, async (source, mode) => ({ source, mode, code: 0, stdout: 'ok', stderr: '' }));
    assert.equal(checked.assertions, 1);
    await assert.rejects(checkOriginal(row, async (source, mode) => ({ source, mode, code: 0, stdout: 'wrong', stderr: '' })), /not satisfied/u);
  }
  const row = functionsIn('use tir_support::assert_tiers_agree as agree; #[test] fn example() { agree("name", "fn run() {}", "ok"); }', 'tests/sample.rs')[0];
  let observed;
  await checkOriginal(row, async (source, mode, metadata) => { observed = metadata; return { source, mode, code: 0, stdout: 'ok', stderr: '' }; });
  assert.equal(observed.agreement, true);
  assert.equal(observed.interpreterAgreement, true);
  await assert.rejects(checkOriginal(row, async (source, mode) => ({ source, mode, code: 1, stdout: 'ok', stderr: '' })), /all-tier.*not satisfied/u);
});

test('E2 exact local run wrappers qualify, but same names with altered bodies stay hand-port', async () => {
  for (const [helper, outputs] of [
    ['fn build_and_run(name: &str, src: &str) -> (i32, String, String) { common::build_and_run("prefix", name, src) }', '(0, "ok", "")'],
    ['fn build_and_run(name: &str, src: &str) -> (i32, String) { let (code, stdout, _stderr) = common::build_and_run("prefix", name, src); (code, stdout) }', '(0, "ok")'],
  ]) {
    const rows = functionsIn(`${helper} #[test] fn example() { assert_eq!(build_and_run("name", "fn run() {}"), ${outputs}); }`, 'tests/sample.rs');
    const checked = await checkOriginal(classify(rows[1], [rows[0]]), async (source, mode) => ({ source, mode, code: 0, stdout: 'ok', stderr: '' }));
    assert.equal(checked.assertions, 1);
  }
  for (const helper of [
    'fn build_and_run(name: &str, src: &str) -> (i32, String) { (0, String::new()) }',
    'fn assert_tiers_agree(name: &str, src: &str, out: &str) { assert_eq!(out, "ok"); }',
    'fn build_and_run(name: &str, src: &str) -> (i32, String, String) { assert!(false); common::build_and_run("prefix", name, src) }',
  ]) {
    const leaf = helper.startsWith('fn assert') ? 'assert_tiers_agree' : 'build_and_run';
    const rows = functionsIn(`${helper} #[test] fn example() { ${leaf}("name", "fn run() {}"); }`, 'tests/sample.rs');
    await assert.rejects(checkOriginal(classify(rows[1], [rows[0]]), reference), /unmodeled call/u);
  }
  const row = functionsIn('use other::build_and_run; #[test] fn example() { build_and_run("name", "fn run() {}"); }', 'tests/sample.rs')[0];
  await assert.rejects(checkOriginal(row, reference), /unmodeled call/u);
});

test('analyzer recovery never treats byte literal punctuation as a call delimiter', () => {
  for (const text of ['(', '::', '!']) {
    const row = fn(`unknown(&mut data, b"${text}"); later();`);
    assert.doesNotThrow(() => analyzeFunction(row));
    assert.ok(analyzeFunction(row).classes.includes('unmodeled call later'));
  }
});

test('draft has one TODO per unsupported call and retains every assertion comment in order', async (t) => {
  const root = fixture(t), out = fixture(t);
  const row = fn('let source = "fn run() {}"; unknown_helper(); assert!(true); assert_eq!(source, "fn run() {}");');
  const result = await draftFunction(row, { root, draft: out, converterSha: 'planted-sha', slice: 'S1', reference: async () => { throw new Error('must not run'); } });
  const text = readFileSync(result.draft, 'utf8');
  assert.equal(result.status, 'DRAFTED');
  assert.equal(result.underlyingStatus, 'HAND-PORT');
  assert.equal(result.todoCount, 1);
  assert.equal(text.match(/# TODO\(port\):/gu).length, 1);
  assert.match(text, /unmodeled call unknown_helper at tests\/sample.rs:/u);
  assert.ok(text.indexOf('# assert!(true)') < text.indexOf('# assert_eq!(source'));
  assert.equal(result.assertionComments, 2);
  assert.match(text, /DRAFT — not a case; finish and move into tests\//u);
  assert.equal(readFileSync(join(out, 'S1/sample/example.files/bindings/source.jet'), 'utf8'), 'fn run() {}');
  assert.equal(existsSync(join(root, 'tests/ui')), false);
});

test('draft refuses checkout outputs and never invokes calls hidden by unknown control flow', async (t) => {
  const root = fixture(t), out = fixture(t);
  await assert.rejects(draftFunction(fn('assert!(true);'), { root, draft: join(root, 'drafts'), converterSha: 'planted-sha' }), /outside the checkout/u);
  const row = fn('if true { jet::compile("fn run() {}").unwrap(); } assert!(true);');
  let calls = 0;
  const result = await draftFunction(row, { root, draft: out, converterSha: 'planted-sha', reference: async () => { calls++; throw new Error('must not run'); } });
  assert.equal(calls, 0);
  assert.equal(result.capturedSteps, 0);
  assert.ok(result.assertionComments >= 2);
});

test('draft captures actual modeled CLI streams without conversion or blessing', async (t) => {
  const root = fixture(t), out = fixture(t), scratch = join(out, 'scratch');
  mkdirSync(scratch);
  const binary = join(root, 'reference');
  writeFileSync(binary, '#!/usr/bin/env node\nif(process.argv.includes("--json"))console.log(JSON.stringify({status:"passed",contract:"jet.check/v1",schema_version:1,provenance:{diagnostics:0},rows:[]}));', { mode: 0o755 });
  const row = fn('let out = jet::compile("fn run() {}").unwrap(); unknown_helper(); assert!(out.lints.is_empty());');
  const result = await draftFunction(row, { root, draft: out, converterSha: 'planted-sha', binary, reference: makeReference(binary, root, scratch, 600000, true) });
  assert.equal(result.capturedSteps, 2);
  assert.equal(result.todoCount, 1);
  const text = readFileSync(result.draft, 'utf8');
  assert.match(text, /step: jet check --json/u);
  assert.match(text, /# assertion mapped: captured step 2/u);
  const stdout = readFileSync(join(out, 'unassigned/sample/example.capture-1.stdout'), 'utf8');
  assert.equal(JSON.parse(stdout).contract, 'jet.check/v1');
  assert.equal(existsSync(join(root, 'tests/ui')), false);
});

test('draft CLI emits per-slice index and preserves inline sources in unsupported calls', async (t) => {
  const root = fixture(t), out = fixture(t), report = join(out, 'report.json'), slices = join(out, 'slices.tsv');
  writeFileSync(join(root, 'tests/sample.rs'), '#[test] fn partial() { unknown("fn run() {}"); debug_assert!(true); }');
  writeFileSync(slices, 'slice\tfile\nS1\ttests/sample.rs\n');
  assert.equal(await main(['--root', root, '--draft', out, '--bless-from', '/not-invoked', '--converter-sha', 'planted-sha', '--slice-map', slices, '--report', report]), 0);
  const result = JSON.parse(readFileSync(report, 'utf8'));
  assert.equal(result.summary.converted, 0);
  assert.equal(result.summary.drafted, 1);
  assert.match(readFileSync(join(out, 'S1/index.tsv'), 'utf8'), /tests\/sample.rs::partial\t/u);
  assert.equal(readFileSync(join(out, 'S1/sample/partial.files/literals/1.jet'), 'utf8'), 'fn run() {}');
  assert.match(readFileSync(join(out, 'S1/sample/partial.case'), 'utf8'), /# debug_assert!\(true\)/u);
});

test('draft resumes independent statements after unknown blocks but never trusts consumed inputs', async (t) => {
  const root = fixture(t), out = fixture(t), scratch = join(out, 'scratch');
  mkdirSync(scratch);
  const binary = join(root, 'reference');
  writeFileSync(binary, '#!/usr/bin/env node\nif(process.argv.includes("--json"))console.log(JSON.stringify({status:"passed",contract:"jet.check/v1",schema_version:1,provenance:{diagnostics:0},rows:[]}));', { mode: 0o755 });
  const reference = makeReference(binary, root, scratch, 600000, true);
  const row = fn('if other() { jet::compile("fn nested() {}").unwrap(); } let src = "fn run() {}"; let d = jet::compile(src).unwrap(); assert!(d.lints.is_empty());');
  const result = await draftFunction(row, { root, draft: out, converterSha: 'planted-sha', binary, reference });
  assert.equal(result.capturedSteps, 2);
  const receipts = JSON.parse(readFileSync(result.draft.replace(/\.case$/u, '.receipts.json'), 'utf8'));
  assert.equal(receipts.sources.length, 1);
  assert.equal(receipts.sources[0].source, 'fn run() {}');
  const consumed = fn('let src = "fn run() {}"; unknown(src); jet::compile(src).unwrap();');
  const second = await draftFunction({ ...consumed, name: 'consumed' }, { root, draft: out, converterSha: 'planted-sha', binary, reference });
  assert.equal(second.capturedSteps, 0);
});

const compileCodes = 'fn codes(src: &str) -> Vec<String> { match jet::compile(src) { Ok(_) => Vec::new(), Err(diags) => diags.iter().map(|d| d.code.clone()).collect(), } }';
const requiredCodes = 'fn codes(src: &str) -> Vec<String> { jet::compile(src).err().unwrap().into_iter().map(|d| d.code.to_string()).collect() }';
const stagedCodes = 'fn codes(source: &str, stem: &str) -> Vec<String> { let root = common::unique_tmp(stem); let path = root.join("main.jet"); fs::write(&path, source).unwrap(); let diagnostics = jet::check_with_path(path.to_str().unwrap()); let _ = fs::remove_dir_all(root); diagnostics.into_iter().map(|diagnostic| diagnostic.code).collect() }';
const pathCodes = 'fn codes(entry: &str) -> Vec<String> { jet::Driver::check_file_with_effect_facts(entry, None, false).0.iter().map(|d| d.code.clone()).collect() }';
const setCodes = 'fn codes(diags: &[jet::Diagnostics::Diagnostic]) -> BTreeSet<String> { diags.iter().map(|d| d.code.clone()).collect() }';
function codesRow(helper, body, imports = '') {
  const rows = functionsIn(`${imports} ${helper} #[test] fn example() { ${body} }`, 'tests/sample.rs');
  return classify(rows.find(row => row.test), rows.filter(row => !row.test));
}

test('E3 compile codes drops successful lints and keeps ordered duplicate errors only', async () => {
  for (const helper of [compileCodes, compileCodes.replace('diags.iter().map(|d| d.code.clone())', 'diags.into_iter().map(|d| d.code.to_string())')]) {
    const row = codesRow(helper, 'assert_eq!(codes("fn run() {}"), ["E0209", "E0209"]);');
    const observed = async (source, mode) => ({ source, mode, code: 1, diagnostics: [diagnostic, { ...diagnostic, severity: 'warning', code: 'L0100' }, diagnostic] });
    assert.equal((await checkOriginal(row, observed)).assertions, 1);
    assert.deepEqual(analyzeFunction(row).classes, []);
    await assert.rejects(checkOriginal(row, reference), /original assert_eq! not satisfied/u);
    const success = codesRow(helper, 'assert!(codes("fn run() {}").is_empty());');
    await checkOriginal(success, async (source, mode) => ({ source, mode, code: 0, diagnostics: [{ ...diagnostic, severity: 'warning', code: 'L0100' }] }));
  }
});

test('E3 required-failure codes retains the implicit unwrap assertion', async () => {
  const row = codesRow(requiredCodes, 'assert_eq!(codes("fn run() {}"), ["E0209"]);');
  assert.equal((await checkOriginal(row, reference)).assertions, 2);
  await assert.rejects(checkOriginal(row, async (source, mode) => ({ source, mode, code: 0, diagnostics: [] })), /compile\.err\(\)\.unwrap\(\) not satisfied/u);
});

test('E3 staged check codes preserves all diagnostics, order, duplicates and exact project input', async (t) => {
  const root = fixture(t), row = codesRow(stagedCodes, 'assert_eq!(codes("fn run() {}", "scope"), ["L0100", "E0209", "L0100"]);');
  const warning = { ...diagnostic, severity: 'warning', code: 'L0100' };
  let metadata;
  const observed = async (source, mode, options) => {
    metadata = options;
    return { source, mode, code: 1, diagnostics: [warning, diagnostic, warning], rendered: 'real reference warning and error\n' };
  };
  await checkOriginal(row, observed, root);
  assert.equal(metadata.allDiagnostics, true);
  assert.deepEqual(metadata.project, { entry: 'run.jet', files: [{ path: 'run.jet', content: 'fn run() {}' }] });
  await assert.rejects(checkOriginal(row, reference, root), /original assert_eq! not satisfied/u);
  await assert.rejects(emitFunction(row, { root, reference: observed }), /explicit verbose CLI case/u);
  assert.equal(existsSync(join(root, 'tests/ui')), false);
});

test('E3 Driver codes uses the supplied staged entry rather than treating its path as source', async (t) => {
  const root = fixture(t), row = codesRow(pathCodes, 'let root = common::unique_tmp("scope"); let path = root.join("entry.jet"); fs::write(&path, "fn run() {}").unwrap(); assert_eq!(codes(path.to_str().unwrap()), ["E0209"]);');
  let metadata, actualSource;
  const observed = async (source, mode, options) => { actualSource = source; metadata = options; return reference(source, mode); };
  observed.createProject = makeReference('/not-invoked', root, root).createProject;
  await checkOriginal(row, observed, root);
  assert.equal(actualSource, 'fn run() {}');
  assert.equal(metadata.allDiagnostics, true);
  assert.equal(metadata.project.files[0].content, actualSource);
  await assert.rejects(checkOriginal(codesRow(pathCodes, 'codes("outside.jet");'), reference, root), /staged input path/u);
});

test('E3 diagnostic slice codes sorts with Rust UTF-8 ordering and deduplicates, without another check', async () => {
  const row = codesRow(setCodes, 'let d = jet::compile("fn run() {}").unwrap_err(); assert_eq!(codes(&d), ["E0209", "", "𐀀"]);', 'use std::collections::BTreeSet;');
  let calls = 0;
  await checkOriginal(row, async (source, mode) => { calls++; return { source, mode, code: 1, diagnostics: [ { ...diagnostic, code: '𐀀' }, diagnostic, { ...diagnostic, code: '' }, diagnostic] }; });
  assert.equal(calls, 1);
  await assert.rejects(checkOriginal(row, reference), /original assert_eq! not satisfied/u);
});

test('E3 lexical aliases qualify only an exact codes body; different bodies and signatures remain hand-port', async () => {
  const alias = codesRow(compileCodes, 'assert_eq!(inspected("fn run() {}"), ["E0209"]);', 'use crate::{codes as inspected};');
  await checkOriginal(alias, reference);
  for (const helper of [
    compileCodes.replace('diags.iter()', 'diags.iter().filter(|d| d.code != "E0209")'),
    compileCodes.replace('match jet::compile(src)', 'assert!(false); match jet::compile(src)'),
    compileCodes.replace('src: &str', 'src: &String'),
    setCodes.replace('BTreeSet<String>', 'Vec<String>'),
    'fn codes(src: &str) -> Vec<String> { Vec::new() }',
  ]) {
    const row = codesRow(helper, 'codes("fn run() {}");');
    await assert.rejects(checkOriginal(row, reference), /unmodeled call codes/u);
    assert.ok(analyzeFunction(row).classes.some(name => name.startsWith('unmodeled call codes')));
  }
  const foreign = codesRow(compileCodes, 'inspected("fn run() {}");', 'use foreign::codes as inspected;');
  await assert.rejects(checkOriginal(foreign, reference), /unmodeled call inspected/u);
});

test('draft composes E3 codes and verbose captures while changed helper bodies stay unsupported', async (t) => {
  const root = fixture(t), out = fixture(t), scratch = join(out, 'scratch');
  mkdirSync(scratch);
  const binary = join(root, 'reference');
  writeFileSync(binary, '#!/usr/bin/env node\nif(process.argv.includes("--json"))console.log(JSON.stringify({status:"passed",contract:"jet.check/v1",schema_version:1,provenance:{diagnostics:0},rows:[]}));', { mode: 0o755 });
  const reference = makeReference(binary, root, scratch, 600000, true);
  const row = codesRow(stagedCodes, 'assert!(codes("fn run() {}", "scope").is_empty()); unknown_helper();');
  const result = await draftFunction(row, { root, draft: out, converterSha: 'planted-sha', binary, reference });
  assert.equal(result.capturedSteps, 2);
  assert.equal(result.todoCount, 1);
  assert.match(readFileSync(result.draft, 'utf8'), /step: jet check --json --verbose/u);
  const changed = codesRow(stagedCodes.replace('diagnostics.into_iter()', 'diagnostics.into_iter().filter(|d| false)'), 'codes("fn run() {}", "scope");');
  const near = await draftFunction({ ...changed, name: 'near' }, { root, draft: out, converterSha: 'planted-sha', binary, reference });
  assert.equal(near.capturedSteps, 0);
  assert.ok(analyzeFunction(changed).classes.some(value => value.startsWith('unmodeled call codes')));
});
