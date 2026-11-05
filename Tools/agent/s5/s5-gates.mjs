#!/usr/bin/env node
// S5 gates: Jet metadata and source-only generator checks.
// Rust compiler/provider suites and cargo compilation are parked under Gate 2.
import {phase} from './s5-lib.mjs';

const P = phase('gates');
const {run, start} = P;
const node = process.execPath;
const jet = process.env.JET || 'jet';

const cheap = [];
for (const script of [
  'Tools/agent/gen-core-tables.mjs', 'Tools/agent/check-core-surface-ledger.mjs', 'Tools/agent/gen-unicode-tables.mjs',
  'Tools/agent/gen-runtime-trap-messages.mjs', 'Tools/agent/gen-runtime-lifecycle.mjs', 'Tools/agent/gen-runtime-stop-exports.mjs',
  'Tools/agent/gen-runtime-host-carriers.mjs', 'Tools/agent/gen-runtime-host-errors.mjs', 'Tools/agent/gen-runtime-scheduler-messages.mjs',
  'Tools/agent/gen-runtime-services-messages.mjs', 'Tools/agent/gen-runtime-test-messages.mjs',
]) cheap.push(start('check-' + script.split('/').pop().replace(/\.mjs$/, ''), [node, script, '--check']));
// The fault-report check assembles with GNU as, a host tool only jet-env supplies until jet does.
cheap.push(start('check-gen-runtime-fault-report', ['Tools/agent/jet-env', 'node', 'Tools/agent/gen-runtime-fault-report.mjs', '--check']));
cheap.push(start('check-core-source-rows', [jet, 'Tools/agent/CoreSourceRows.jet', '--check']));
cheap.push(start('check-diagnostic-rows', [jet, 'Tools/agent/DiagnosticRows.jet', '--check']));
cheap.push(start('check-lexer-keyword-rows', [jet, 'Tools/agent/LexerKeywordRows.jet', '--check']));
cheap.push(start('stop-exports-test', [node, '--test', 'Tools/agent/gen-runtime-stop-exports.test.mjs']));
cheap.push(start('trap-messages-test', [node, '--test', 'Tools/agent/gen-runtime-trap-messages.test.mjs']));
for (const tool of ['LexerKeywordRows', 'UnicodeNameAsset', 'AgentDocFlags', 'DecisionIndex', 'UnsafeRatchet', 'DiagnosticRows']) {
  cheap.push(start(`${tool}-test`, [jet, 'test', `Tools/agent/${tool}.jet`]));
}
cheap.push(start('compiler-packages', [node, '--test', 'Tools/agent/compiler-packages.test.mjs']));
cheap.push(start('proof-source-guard', [node, '--test', 'Tools/agent/proof-source.test.mjs']));
cheap.push(start('core-type-references', [node, '--test', 'Tools/agent/core-type-references.test.mjs']));
cheap.push(start('core-runtime-wrapper', [node, '--test', 'Tools/agent/core-runtime-wrapper.test.mjs']));
cheap.push(start('native-cli-transport', [node, '--test', 'Tools/agent/compiler-native-transport.test.mjs']));
cheap.push(start('s5-completion-receipts', [node, '--test', 'Tools/agent/s5/receipts.test.mjs']));
await Promise.all(cheap);

P.finish();
