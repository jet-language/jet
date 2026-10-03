import test from 'node:test';
import assert from 'node:assert/strict';
import { decodeJet, jetName, classify, renderFolded } from './profile.mjs';

test('canonical paths decode Jet byte escapes and compiler modules', () => {
  assert.equal(jetName('jetc_jetsema::__jet_src_c_csrc_scompiler_djet_c_cSemaRegistrationVariant'), 'JetSema::SemaRegistrationVariant');
  assert.equal(decodeJet('__jet_A_ub_dC_cD_sE_bF_hG_x24_xc3_xa9'), 'A_b.C:D/E\\F-G$é');
  assert.equal(decodeJet('__jet_ct_value'), '$value');
  assert.equal(decodeJet('__jet___temporary'), '[generated] temporary');
  assert.equal(jetName('__jet_field_check', new Map([['field_check', []]])), 'field_check');
});

test('six phases use functions, never generic registration type arguments', () => {
  const cases = [
    ['JetSema::sema_registration_graph', 'registration'],
    ['JetSema::sema_function_check', 'sema check'],
    ['JetEval::jet_eval_machine', 'comptime'],
    ['JetCodegen::jet_codegen_lower_type_definition', 'lowering'],
    ['JetCodegen::jet_codegen_emit_value', 'lowering'],
    ['JetOptimizer::mir_pass_fold_exact_constants', 'optimization'],
    ['JetCodegen::jet_rust_emit_program', 'emission'],
  ];
  for (const [frame, phase] of cases) assert.equal(classify(['malloc', frame]), phase);
  assert.equal(classify(['clone<JetSema::SemaRegistrationGraph>', 'JetSema::sema_function_check']), 'sema check');
  assert.equal(classify(['clone<JetSema::SemaRegistrationGraph>']), 'unclassified');
  assert.equal(classify(['JetEval::jet_eval_machine', 'JetSema::sema_function_check']), 'comptime');
});

test('renderer preserves counts and XML-escapes titles', () => {
  const svg = renderFolded('main;clone<A&B> 3\nmain;other 2\n');
  assert.match(svg, /5 samples/);
  assert.match(svg, /clone&lt;A&amp;B&gt; — 3 samples \(60\.00%\)/);
  assert.match(svg, /<\/svg>\n$/);
  assert.throws(() => renderFolded(''), /No folded samples/);
});
