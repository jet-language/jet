import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { analyze, maskLine, parseType, showType } from './clone-gate.mjs';

const P = '__jet_src_c_csrc_scompiler_djet_c_c';
const STAGE_ZERO = `
#[derive(Clone)]
pub struct ${P}SemaRegistrationGraph {
    pub __jet_modules: Vec<${P}SemaRegistrationModule>,
    pub __jet_rows: JetMap<String, ${P}SemaRegistrationModule>,
}

#[derive(Clone)]
pub struct ${P}SemaRegistrationModule {
    pub __jet_name: String,
}

#[derive(Clone)]
pub struct ${P}SemaScopeEnvironment {
    pub __jet_registration_graph: JetOutcome<${P}SemaRegistrationGraph, JetAbsent>,
    pub __jet_module_identity: String,
}

fn ${P}probe(__jet_environment: &${P}SemaScopeEnvironment) -> bool {
 let mut _v3: Option<JetOutcome<${P}SemaRegistrationGraph, JetAbsent>> = None;
 let mut _v4: Option<String> = None;
 #[cfg(not(jet_release))]
 let _jet_stack_frame = jet_stack_enter("src/compiler.jet", 3u32, "probe", "fn probe(environment: SemaScopeEnvironment) -> Bool");
 _v3 = Some((((*__jet_environment).__jet_registration_graph).clone()));
 let _v5: bool = matches!(_v3.as_ref().expect("MIR value"), Ok(_));
 _v4 = Some((((*__jet_environment).__jet_module_identity).clone()));
 _v5
}

fn ${P}driver(__jet_environment: &${P}SemaScopeEnvironment) {
 #[cfg(not(jet_release))]
 let _jet_stack_frame = jet_stack_enter("src/compiler.jet", 9u32, "driver", "fn driver(environment: SemaScopeEnvironment)");
 'c0: loop {
  let _v1: bool = ${P}probe(__jet_environment);
  let _v2: ${P}SemaRegistrationModule = (*jet_index_vec_ref(& (*(match & ((*__jet_environment).__jet_registration_graph) { Ok(payload) => payload, _ => unreachable!("MIR payload variant mismatch") })).__jet_modules, 0, "src/compiler.jet", 11u32)).clone();
  break 'c0;
 }
}
`;
const UNIT = [
  '// [jet-bootstrap source: Compiler/A.jet]',
  'fn probe(environment: SemaScopeEnvironment) -> Bool {',
  '    environment.registration_graph == .Val(_)',
  '}',
  '// [jet-bootstrap source: Compiler/B.jet]',
  'fn driver(environment: SemaScopeEnvironment) {',
  '    loop {',
  '        probe(environment)',
  '        row :: environment.registration_graph?.modules[0]',
  '    }',
  '}',
].join('\n') + '\n';

function fixture() {
  const root = fs.mkdtempSync(path.join(os.homedir(), '.cache', 'jet-dev', 'scratch', 'clone-gate-test-'));
  fs.mkdirSync(path.join(root, 'compiler-project', 'src'), { recursive: true });
  fs.writeFileSync(path.join(root, 'stage-zero.rs'), STAGE_ZERO);
  fs.writeFileSync(path.join(root, 'compiler-project', 'src', 'compiler.jet'), UNIT);
  return root;
}

test('types parse and render with short names', () => {
  assert.equal(showType(parseType(`JetOutcome<${P}SemaRegistrationGraph, JetAbsent>`)), 'SemaRegistrationGraph?');
  assert.equal(showType(parseType(`&mut Vec<Box<${P}Expr>>`)), 'Vec<Box<Expr>>');
  assert.match(showType(parseType('Box<dyn Fn(&T) -> U>')), /^Box<Fn/, 'closure types terminate');
});

test('masking blanks literal contents and keeps columns', () => {
  const st = { str: false, raw: -1, comment: false };
  const line = 'f("a{b", \'{\') /* } */ { x }';
  const masked = maskLine(line, st);
  assert.equal(masked.length, line.length);
  assert.equal(masked.split('{').length - 1, 1);
});

test('large copies are typed, mapped to source, classed, and ranked', () => {
  const root = fixture();
  try {
    const result = analyze(path.join(root, 'stage-zero.rs'), { allow: path.join(root, 'none.tsv') });
    assert.equal(result.totalSites, 3);
    assert.equal(result.sites.length, 2, 'the String copy is not large');
    const [second, first] = result.sites;
    assert.equal(first.type, 'SemaRegistrationModule');
    assert.equal(first.tier, 2);
    assert.equal(first.path, 'per-loop');
    assert.equal(first.shape, 'element');
    assert.equal(first.source, 'Compiler/B.jet:6');
    assert.equal(second.type, 'SemaRegistrationGraph?');
    assert.equal(second.tier, 3);
    assert.equal(second.shape, 'pattern-test');
    assert.equal(second.path, 'per-item');
    assert.equal(second.jet, 'probe');
    assert.equal(second.source, 'Compiler/A.jet:2');
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});
