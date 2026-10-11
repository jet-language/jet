#!/usr/bin/env node
// S5 gates: Jet metadata and source-only generator checks.
// Rust compiler/provider suites and cargo compilation are parked under Gate 2.
import {phase} from './s5-lib.mjs';

const P = phase('gates');
const {run, start} = P;
const node = process.execPath;

const cheap = [];
for (const script of [
  'Tools/agent/gen-core-tables.mjs', 'Tools/agent/check-core-surface-ledger.mjs', 'Tools/agent/gen-unicode-tables.mjs',
  'Tools/agent/gen-runtime-host-carriers.mjs',
  'Compiler/Bootstrap/generate-prelude-derives.mjs', 'Compiler/Bootstrap/generate-core-sources.mjs',
]) cheap.push(start('check-' + script.split('/').pop().replace(/\.mjs$/, ''), [node, script, '--check']));
// Jet-authored generators run with the compiler built from this tree (JET): their `--check`s are
// the fixture gate's `check <generator>` tests, and each generator's own #Test blocks run beside.
const jet = process.env.JET ?? 'jet';
cheap.push(start('check-jet-generators', [jet, 'test', 'Tools/testkit', 'check ', '--capture=all', '--color=never'], {JET: jet}));
for (const script of ['Tools/agent/RuntimeGenerators', 'Tools/agent/LexerKeywordRows.jet']) {
  cheap.push(start('test-' + script.split('/').pop().replace(/\.jet$/, ''), [jet, 'test', script]));
}
cheap.push(start('bootstrap-source-inventory', [node, '--test', 'Tools/agent/bootstrap-source-inventory.test.mjs']));
cheap.push(start('mir-schema-grammar', [node, '--test', 'Compiler/JetBackend/Tests/mir-schema.test.mjs']));
cheap.push(start('proof-source-guard', [node, '--test', 'Tools/agent/proof-source.test.mjs']));
cheap.push(start('core-type-references', [node, '--test', 'Tools/agent/core-type-references.test.mjs']));
cheap.push(start('core-data-event-projection', [node, '--test', 'Tools/agent/core-data-event-projection.test.mjs']));
cheap.push(start('core-runtime-wrapper', [node, '--test', 'Tools/agent/core-runtime-wrapper.test.mjs']));
cheap.push(start('native-cli-transport', [node, '--test', 'Tools/agent/compiler-native-transport.test.mjs']));
cheap.push(start('s5-completion-receipts', [node, '--test', 'Tools/agent/s5/receipts.test.mjs']));
await Promise.all(cheap);

P.finish();
