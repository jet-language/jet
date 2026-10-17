#!/usr/bin/env node
// Package-free pure planner/emitter witnesses; canonical AST declarations and
// helper dependencies are copied verbatim from the compiler source closure.
// Usage: node Compiler/JetDriver/Tests/run-rust-foreign-fixtures.mjs <outdir> [--host | --host-full]
// --eval <jet> executes the actual pure fixtures without the hosted emitter.
// --native-abi --cargo-target <diskdir> also builds only that emitted helper,
// then links/runs the C descriptor witness with the opt-in musl toolchain.
// --host-env executes the actual isolated-helper environment argument producer.
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { spawnSync, execFileSync } from 'node:child_process';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const out = process.argv[2];
if (!out) throw new Error('usage: run-rust-foreign-fixtures.mjs <outdir>');
const read = path => readFileSync(resolve(repo, path), 'utf8');
const core = new Set();
function body(text) {
  const kept = []; let skipping = false;
  for (const line of text.split('\n')) {
    if (skipping) { if (line.startsWith(']') || line === '}') skipping = false; continue; }
    if (/^use jet_\w+\.\[\s*$/.test(line)) { skipping = true; continue; }
    if (/^use jet_/.test(line)) continue;
    if (/^use core\./.test(line)) { core.add(line); continue; }
    kept.push(line);
  }
  return kept.join('\n');
}
const declarations = new Map();
for (const path of read('Compiler/Bootstrap/sources.list').split('\n').filter(line => line.trim() && !line.startsWith('#'))) {
  if (!path.startsWith('Compiler/JetFoundation/') && path !== 'Compiler/JetCli/Source/Cli/RustForeign.jet') continue;
  const lines = read(path).split('\n');
  for (let i = 0; i < lines.length; i++) {
    const match = /^(?:struct|enum|fn)\s+([A-Za-z_]\w*)\b|^([A-Z][A-Z_\d]*)\s*::/.exec(lines[i]);
    if (!match) continue;
    const name = match[1] ?? match[2]; let start = i;
    while (start > 0 && lines[start - 1].startsWith('#')) start--;
    let end = i;
    if (lines[i].trimEnd().endsWith('{')) {
      end = lines.findIndex((line, index) => index > i && line.startsWith('}'));
      if (end < 0) throw new Error(`${path}: unclosed ${name}`);
    }
    if (declarations.has(name)) throw new Error(`duplicate compiler declaration ${name}`);
    declarations.set(name, { path, text: lines.slice(start, end + 1).join('\n') });
    i = end;
  }
}
const planner = read('Compiler/JetDriver/Source/Driver/RustForeign.jet');
const prefix = planner.indexOf('struct JetDriverRustForeignRecord ');
if (prefix < 0) throw new Error('missing planner/checked-graph boundary');
const fixture = read('Compiler/JetDriver/Tests/RustForeignFixtures.jet');
const environmentFixture = process.argv.includes('--host-env');
const host = process.argv.includes('--host') || process.argv.includes('--host-full') || environmentFixture;
const roots = [body(planner.slice(0, prefix)), body(read('Compiler/JetDriver/Source/Driver/RustForeignSource.jet')), body(host ? fixture.slice(0, fixture.indexOf('fn run()')) : fixture)];
if (host) {
  if (process.argv.includes('--host-full')) roots.push(body(read('Compiler/JetCli/Source/Cli/RustForeign.jet')));
  else roots.push(body(read('Compiler/JetCli/Source/Cli/RustForeign.jet').split('\n').filter(line => line.startsWith('use core.')).join('\n')));
  if (environmentFixture) {
    const tests = read('Compiler/JetCli/Tests/RustForeign.jet');
    const start = tests.indexOf('fn cli_rust_foreign_test_environment()');
    const end = tests.indexOf('\nfn run()', start);
    if (start < 0 || end < 0) throw new Error('missing canonical helper environment regression');
    roots.push(body('use core.sys as rust_foreign_test_sys\n' + tests.slice(start, end) + '\nfn run() { cli_rust_foreign_test_environment()\nprint("FFI environment fixtures passed") }\n'));
  } else roots.push(body(read('Compiler/JetCli/Tests/RustForeignFixtures.jet')));
}
const owned = new Set(roots.join('\n').match(/\b(?:JetDriverRustForeign\w*|jet_driver_rust_foreign_\w*|rust_foreign_fixture_\w*|JET_DRIVER_RUST_FOREIGN_\w*)\b/g) ?? []);
owned.add('run');
if (process.argv.includes('--host-full')) {
  for (const [name, declaration] of declarations) if (declaration.path === 'Compiler/JetCli/Source/Cli/RustForeign.jet') owned.add(name);
}
const selected = new Map();
const queue = [...roots];
while (queue.length) {
  const text = queue.pop();
  // Literal descriptor words (e.g. RustExternBlock) are not AST dependencies.
  // Retain function calls inside interpolations, including hashing/display.
  const code = text.replace(/\/\/[^\n]*/g, '').replace(/"(?:\\.|[^"\\])*"/g, '');
  const calls = [...text.matchAll(/\b([A-Za-z_]\w*)\s*\(/g)].map(match => match[1]).filter(name => /^fn /m.test(declarations.get(name)?.text ?? ''));
  // A variant named Field/Symbol is not the unrelated AST Field/Symbol type.
  const types = [...code.matchAll(/\b([A-Za-z_]\w*)\s*(?=[.{])|(?:[:[\]<]|->)\s*([A-Za-z_]\w*)/g)].map(match => match[1] ?? match[2]);
  const constants = text.match(/\b[A-Z][A-Z_\d]*\b/g) ?? [];
  for (const name of [...types, ...calls, ...constants]) {
    if (owned.has(name) || selected.has(name) || !declarations.has(name)) continue;
    const declaration = declarations.get(name);
    selected.set(name, declaration); queue.push(declaration.text);
  }
}
const parts = [...selected.values()].map(({ path, text }) => `// [canonical ${path}]\n${body(text)}`);
for (const path of new Set([...selected.values()].map(declaration => declaration.path))) {
  if (path.startsWith('Compiler/JetFoundation/')) body(read(path));
}
mkdirSync(out, { recursive: true });
writeFileSync(resolve(out, 'unit.jet'), [...core, ...parts, ...roots].join('\n\n'));
console.log(`${selected.size} canonical dependencies; wrote ${resolve(out, 'unit.jet')}`);
const option = name => {
  const index = process.argv.indexOf(name);
  if (index < 0) return undefined;
  const value = process.argv[index + 1];
  if (!value || value.startsWith('--')) throw new Error(`${name} needs a value`);
  return value;
};
const jet = option('--eval');
if (process.argv.includes('--native-abi') && (!jet || host)) throw new Error('--native-abi needs the helper --eval witness');
if (jet) {
  const evaluated = spawnSync(jet, ['eval', resolve(out, 'unit.jet'), '--json'], { encoding: 'utf8', maxBuffer: 1 << 24 });
  if (evaluated.status !== 0) throw new Error(`${evaluated.stdout}\n${evaluated.stderr}`);
  if (JSON.parse(evaluated.stdout).ok !== true) throw new Error('pure fixture evaluation did not pass');
  const marker = environmentFixture ? 'FFI environment fixtures passed' : host ? 'FFI host fixtures passed' : 'FFI helper fixtures passed';
  if (!evaluated.stderr.includes(marker)) throw new Error('missing executed fixture assertions');
  console.log(marker);
  if (!host) {
    const manifestMarker = '---JET-FFI-MANIFEST---\n', sourceMarker = '---JET-FFI-SOURCE---\n';
    const begin = evaluated.stderr.indexOf(manifestMarker);
    const middle = evaluated.stderr.indexOf(sourceMarker);
    if (begin < 0 || middle <= begin) throw new Error('missing actual helper output');
    const manifest = evaluated.stderr.slice(begin + manifestMarker.length, middle).replace(/\n$/, '');
    const source = evaluated.stderr.slice(middle + sourceMarker.length).replace(/\n$/, '');
    const helper = resolve(out, 'native-helper');
    mkdirSync(resolve(helper, 'src'), { recursive: true });
    writeFileSync(resolve(helper, 'Cargo.toml'), manifest);
    writeFileSync(resolve(helper, 'src/lib.rs'), source);
    if (process.argv.includes('--native-abi')) {
      const target = option('--cargo-target');
      if (!target || !target.startsWith('/') || target === '/tmp' || target.startsWith('/tmp/')) throw new Error('helper cargo target must be an explicit absolute disk-backed directory');
      const toolchain = process.env.JET_RUST_MUSL_TOOLCHAIN, linker = process.env.JET_RUST_MUSL_LINKER, archiver = process.env.JET_RUST_MUSL_AR;
      if (!toolchain || !linker || !archiver) throw new Error('native ABI witness needs the opt-in musl toolchain/linker/AR');
      const env = { ...process.env, RUSTC: `${toolchain}/bin/rustc`, CARGO_TARGET_DIR: target, CARGO_BUILD_JOBS: '1', CARGO_INCREMENTAL: '0', CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER: linker, CC_x86_64_unknown_linux_musl: linker, AR_x86_64_unknown_linux_musl: archiver };
      delete env.RUSTC_WRAPPER; delete env.RUSTC_WORKSPACE_WRAPPER;
      // Cargo comes only from PATH, exactly as the production realizer.
      execFileSync('cargo', ['build', '--release', '--target', 'x86_64-unknown-linux-musl', '--manifest-path', resolve(helper, 'Cargo.toml')], { env, stdio: 'inherit' });
      const crate = /^name = "(jet_ffi_[0-9a-f]{64})"$/m.exec(manifest)?.[1];
      if (!crate) throw new Error('unexpected helper artifact name');
      const archive = resolve(target, 'x86_64-unknown-linux-musl/release', `lib${crate}.a`);
      const binary = resolve(out, 'native-abi');
      execFileSync(linker, ['-static', resolve(repo, 'Compiler/JetDriver/Tests/RustForeignABI.c'), archive, '-lpthread', '-ldl', '-lm', '-o', binary], { env, stdio: 'inherit' });
      execFileSync(binary, [], { stdio: 'inherit' });
      console.log(`actual native helper archive: ${archive}`);
    }
  }
}
