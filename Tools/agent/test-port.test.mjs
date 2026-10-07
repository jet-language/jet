import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, existsSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { functionsIn, collectFunctions, censusTsv, classify, checkOriginal, emitFunction, makeReference } from './test-port.mjs';

function fixture(t) {
  const root = mkdtempSync(join(process.env.JET_SCRATCH_ROOT ?? tmpdir(), 'test-port-unit-'));
  mkdirSync(join(root, 'tests'), { recursive: true });
  t.after(() => rmSync(root, { recursive: true, force: true })); return root;
}
const fn = (body) => functionsIn(`#[test]\nfn example() { ${body} }`, 'tests/sample.rs')[0];
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
  assert.deepEqual(seen, [{ path: 'tests/ui/sample/example.jet', verbose: false }]);
  assert.equal(readFileSync(join(root, emitted.paths[0]), 'utf8'), 'fn run() {}');
});

test('verbose lint expectations use a CLI case and exact separate streams', async (t) => {
  const root = fixture(t);
  const row = fn('let out = jet::compile("fn run() {}").expect("ok"); assert!(out.lints.iter().any(|d| d.code == "L0202"));');
  const observe = async (source, mode, metadata) => ({ source, mode, code: 0, verbose: metadata.verbose,
    diagnostics: [{ ...diagnostic, code: 'L0202' }], stdout: '', stderr: 'Warning [L0202]\\n' });
  const emitted = await emitFunction(row, { root, reference: observe });
  assert.ok(emitted.paths.includes('tests/cases/sample/example.case'));
  assert.match(readFileSync(join(root, 'tests/cases/sample/example.case'), 'utf8'), /step: jet check --verbose example.jet/u);
  assert.equal(existsSync(join(root, 'tests/ui')), false);
});

test('multi-file input assembly writes real scratch files and restages the complete project', async (t) => {
  const root = fixture(t), scratch = join(root, 'scratch'); mkdirSync(scratch);
  const binary = join(root, 'reference');
  writeFileSync(binary, '#!/usr/bin/env node\nconst fs=require("node:fs"), path=require("node:path"); const entry=process.argv.at(-1); if(fs.readFileSync(path.join(path.dirname(entry),"helper.jet"),"utf8")!=="fn helper() {}") process.exit(101); if(process.argv.includes("--json")) console.log(JSON.stringify({schema:"jet.report/v3",code:"E0209",what:"bad",why:"owner",fix:"copy",severity:"error"})); else console.error("Error [E0209]: bad"); process.exitCode=1;\n', { mode: 0o755 });
  const row = fn('let root = common::unique_tmp("input"); fs::create_dir_all(&root).unwrap(); fs::write(root.join("helper.jet"), "fn helper() {}").unwrap(); let entry = root.join("main.jet"); let src = "fn run() {}"; fs::write(&entry, src).unwrap(); let d = jet::compile_with_path(src, entry.to_str().unwrap()).expect_err("bad"); assert!(d.iter().any(|d| d.code == "E0209"));');
  const result = await emitFunction(row, { root, reference: makeReference(binary, root, scratch) });
  assert.ok(result.paths.includes('tests/ui/sample/example/main.jet'));
  assert.ok(result.paths.includes('tests/ui/sample/example/helper.jet'));
  assert.equal(readFileSync(join(root, 'tests/ui/sample/example/helper.jet'), 'utf8'), 'fn helper() {}');
});

test('reference JSON/rendered code disagreement refuses conversion', async (t) => {
  const root = fixture(t), scratch = join(root, 'scratch'); mkdirSync(scratch);
  // Use the public runner against an actual process, not an injected diagnostic.
  const binary = join(root, 'reference');
  writeFileSync(binary, '#!/usr/bin/env node\nif(process.argv.includes("--json")) { console.log(JSON.stringify({schema:"jet.report/v3",code:"E0209",what:"bad",why:"owner",fix:"copy",severity:"error"})); } else { console.error("Error [E0003]: different"); } process.exitCode=1;\n', { mode: 0o755 });
  await assert.rejects(makeReference(binary, root, scratch)('fn run() {}', 'compile'), /JSON\/rendered/u);
});
