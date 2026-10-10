#!/usr/bin/env node
// Exercise the production route-shape generator's flattened tuple/Option ABI.
import assert from 'node:assert/strict';
import { readFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const output = resolve(process.argv[2] ?? '/tmp/jet-route-composition');
mkdirSync(output, { recursive: true });
const source = readFileSync(`${root}/Tools/agent/gen-c-abi-routes.mjs`, 'utf8');
const slice = (begin, end) => {
  const first = source.indexOf(begin), last = source.indexOf(end, first);
  assert(first >= 0 && last > first, `${begin}: generator declaration must exist`);
  return source.slice(first, last);
};
const context = vm.createContext({
  resultAlias: () => null, rustOutcomeAlias: () => false,
  runtimeHandle: () => null,
  fixedInts: new Set(), opaqueName: /^NeverAFixtureType$/,
});
vm.runInContext([
  slice('function elementCarrier(', '// A List parameter element'),
  slice('function paramCarrier(', '// A value carrier'),
  slice('function valueCarrier(', '// Split '),
  slice('function splitTop(', '// A runtime signature'),
  slice('function resultShape(', '// ---------------------------------------------------------------------------\n// 4.'),
].join('\n'), context);
assert.equal(vm.runInContext('paramCarrier("i64", "", "").expr(["word"])', context), 'native(word)');
assert.equal(vm.runInContext('paramCarrier("&i64", "", "").expr(["word"])', context), '&native(word)');
assert.equal(vm.runInContext('elementCarrier("i64").read', context), 'native(w as i64)');
const shape = vm.runInContext('resultShape("(bool,Option<i64>,Option<bool>,Option<Option<i64>>)", "", false)', context);
assert(shape && shape.ret === null);
assert.deepEqual(Array.from(shape.outs, item => item.name), [
  'item0', 'item1_tag', 'item1_some', 'item2_tag', 'item2_some',
  'item3_tag', 'item3_some_tag', 'item3_some_some',
]);
assert.match(shape.jet, /Option\{some: LowerRouteCarrier\.Option/);
const params = shape.outs.map(item => `${item.name}: *mut ${item.cty}`).join(', ');
const rust = `fn projected(${params}, state: u8) ${shape.body('(true, (state != 0).then_some(41i64), (state != 0).then_some(false), (state != 0).then_some((state == 2).then_some(99i64)))')}\nfn main() {
  for state in 0..3u8 {
    let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h) = (false, -1i64, -1i64, -1i64, true, -1i64, -1i64, -1i64);
    projected(&mut a, &mut b, &mut c, &mut d, &mut e, &mut f, &mut g, &mut h, state);
    assert!(a);
    if state == 0 { assert_eq!((b, c, d, e, f, g, h), (0, -1, 0, true, 0, -1, -1)); }
    else { assert_eq!((b, c, d, e, f), (1, 41, 1, false, 1)); assert_eq!(g, if state == 2 { 1 } else { 0 }); assert_eq!(h, if state == 2 { 99 } else { -1 }); }
  }
  println!("PASS: tuple/Option out slots, None, Some(false), nested None and nested Some");
}\n`;
writeFileSync(`${output}/tuple-option.rs`, rust);
execFileSync('rustc', ['--edition=2021', `${output}/tuple-option.rs`, '-o', `${output}/tuple-option`], { stdio: 'inherit' });
execFileSync(`${output}/tuple-option`, [], { stdio: 'inherit' });
const routes = readFileSync(`${root}/Compiler/JetBackend/Source/Lower/RuntimeRoutes.jet`, 'utf8');
for (const name of ['jet_walk8_metadata', 'jet_walk8_rules', 'jet_walk8_read_dir', 'jet_walk8_next', 'jet_terminal8_read_all']) {
  assert(routes.split('\n').some(line => line.includes(`symbol: "${name}"`) && line.includes('LowerRouteCarrier.Option')), `${name}: composed carrier route missing`);
}
assert(routes.includes('LOWER_ENVIRONMENT_ROUTES'), 'environment route table must survive generation');
console.log('PASS: composed native route inventory and environment table retained');
