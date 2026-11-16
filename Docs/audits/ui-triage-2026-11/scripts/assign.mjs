// node assign.mjs TREE TRI.json CAPDIR OUT.tsv : one row per non-PASS ui cell with area root, verdict and cause.
// Verdicts: fix-compiler (Jet output differs from the oracle snapshot and nothing ratified the new output),
// regenerate (the Jet output is the ratified one), fixture-stale (the fixture source uses a retired or
// now-reserved spelling), harness (testkit/runner/environment), decide (needs an owner ruling).
import { readFileSync, writeFileSync, existsSync } from 'node:fs';
const [treeArg, triPath, capDir, outPath] = process.argv.slice(2);
const tri = JSON.parse(readFileSync(triPath, 'utf8'));
const TREE = treeArg.replace(/\/?$/, '/');
for (const t of tri) {
  if (t.blk_kind !== 'non-block-text' || !t.snapshot || !existsSync(`${capDir}/actual/${t.fixture}.actual`)) continue;
  const e = readFileSync(TREE + t.snapshot, 'utf8');
  const a = readFileSync(`${capDir}/actual/${t.fixture}.actual`, 'utf8');
  t.trail_blank = e !== a && e.replace(/\n+$/, '\n') === a.replace(/\n+$/, '\n');
}

const AREAS = [
  ['H-REFUSED', (t) => t.status === 'REFUSED'],
  ['H-NOSTREAM', (t) => t.cls === 'missing-stream'],
  ['H-ENV', (t) => /\/(migration_change_impl_ok|ffi_fetch_failed|ffi_bad_path|ffi_bad_type|cffi_e3205\/run)\.jet$/.test(t.fixture)],
  ['S-TRAILBLANK', (t) => t.blk_kind === 'non-block-text' && t.trail_blank],
  ['W-31OCT', (t) => /\/(cell_guard_storage|typestate_unknown_state|typestate\/unknown_state_in_marker_is_e0151)\.jet$/.test(t.fixture)],
  ['D-TEACHPAUSE', (t) => /\/(channel_retired_forms|cli_e0043_install|foreign_declaration_binding|foreign_method_names_list|phantom_fact_menu_capability|suppress_retired|tracked_origin_runtime_projection|try_suffix_staged|http_message_literal_migration)\.jet$/.test(t.fixture)],
  ['A-LINT', (t) => t.fixture.startsWith('tests/ui_lint/') && !/typestate/.test(t.fixture) || /\/(lint_cognitive_complexity|lint_cognitive_complexity_refactored|crypto_secret_name_collisions)\.jet$/.test(t.fixture)],
  ['A-TYPESTATE', (t) => /typestate|protocol_out_of_order/.test(t.fixture)],
  ['A-RUNTIME', (t) => /\/(runtime_e3014|runtime_rng_backward|view_mut_bounds_interpreter)\.jet$/.test(t.fixture)],
  ['A-PURITY', (t) => /\/(comptime_|pure\/|determinism\/|clock_system_pure|effect_math_random|impure_)/.test(t.fixture) || /E34\d\d/.test(t.exp_codes)],
  ['A-PACKAGES', (t) => /\/(inline_package_|manifest_|project_module_|use_unrealized_library|import_cycle|import_missing|check_scope_owned|script_imported|cffi\/foreign_js_|core_submodule_item_path|bench_command_retired_e2101|e1803_|secret_ungranted|cli_short_name_collision|cli_authority_flags_reserved|repl_effect_denied_e1803)/.test(t.fixture)],
  ['A-FFI', (t) => /\/(cffi|ffi_|lowlevel_|unsafe_obligation_)/.test(t.fixture)],
  ['A-PARSER', (t) => /\/(meta_on_expression|off_debug_attr_on_expression|marker_wrong_at_plane|trailing_block_not_function|callable_policy_invalid|policy_site_bound_authority|test_block_nested|tuple_duplicate_field|source_nesting_limit|params_not_yet|defer_only_close|empty_map_colon_retired|take_pattern_computed_arg|task_spawn_parameters|sema_soundness\/compound_updates_require_their_operator|matrix_measure_inner_mismatch|identifier_casing|pattern_hole_reuse_constant|enum_pattern_needs_dot|enum_pattern_needs_dot_payload|enum_named_payload_needs_dot_brace|parse_pattern_no_else|nested_option|loop_bindingless_named_bindings|derive_tag|inline_module_return_type_mismatch|unknown_char|todo_call_e0803|retired_cli_marker|semaphore_teaching|serde_rename_not_string|result_handler_non_result|layout_columnar_partial)\.jet$/.test(t.fixture)],
  ['A-CONCURRENCY', (t) => /\/(task_|parallel_|closures\/parallel_adapters|fixed_backing_crossing|browser_bidi\/native_bidi_session|taskgroup_parameter_view|ownership\/write_window_cannot_cross_task_boundary)/.test(t.fixture)],
  ['A-OWNERSHIP', (t) => /\/(ownership\/|lambda_|cell_guard_|local_cell_tiers\/|copies_explicit_view_field|payload_write_window_owner_use|move_from_borrowed_argument|implicit_take_keeps_mark|sigil_move_capability_misuse|take_required|compute_set_immutable|job_queue_checked_identity|shared_guard_computed_map|view_|vault_key_unlock_outlives_owner|arrow_batch_use_after_move|layout_columnar_unsupported_method|receiver_mark_extra_fresh|function_contract_laundering)/.test(t.fixture)],
  ['A-FAILURE', (t) => /\/(ambient_err_mixed|contract_result_misuse|e2421_public_failure_carrier|fallible_arm_table_tail|result_handler_type_mismatch|result_loop_break_payloads|value_carrying_return_resultless|never_return_|discarded_lost_tail)/.test(t.fixture)],
  ['A-SECURITY', (t) => /\/(crypto_|taint|vault_|transact_irreversible_effect|effect_callback_bound|arena_bounded_)/.test(t.fixture)],
  ['A-DOMAIN', (t) => /\/(quantity_|coordinate_|distinct_|operator_mixed_|computed_enum_|core_math_|layout_axis_mismatch|lowlevel_volatile)/.test(t.fixture)],
  ['A-LIBTYPES', (t) => /\/(web_|serde_|db_typed_sql|reflect_|compiler_api\/|corelib_parts_runtime\/|path_builtin_not_core_record|core_failure_record_literal_user|dyn_from_missing_encode|browser_bidi\/|delegation_no_impl|comptime_net|reader_take_sized_int|validate_over_unknown_field|iter_missing_method_hint|memo_lazy_iter|print_lazy_iter_view)/.test(t.fixture)],
  ['A-TYPES', () => true],
];

// Hand-checked verdicts (fixture suffix -> [verdict, note]).
const O = {
  'meta_on_expression.jet': ['regenerate', 'Jet reports E0349 MetaNotAllowedHere, the registry row for exactly this; the oracle used the generic E0355'],
  'off_debug_attr_on_expression.jet': ['regenerate', 'Jet reports E0343 SwitchOffAsExpression, the registry row for exactly this; the oracle used the generic E0355'],
  'effect_math_random_in_pure_fn.jet': ['regenerate', 'Jet reports E3403 NondeterminismInPureEval for math.random, the registry row for exactly this'],
  'nested_callable_containers_keep_required_public_labels.jet': ['regenerate', 'Jet reports E0771 FunctionValueLabelMismatch, more exact than the oracle E0108'],
  'path_builtin_not_core_record.jet': ['regenerate', 'argument mismatch is E0112 by its registry meaning; the oracle used E0108'],
  'tuple_duplicate_field.jet': ['regenerate', 'Jet reports E0105 DuplicateDefinition naming the repeated tuple field; the oracle had a parse E0003'],
  'user_policy_bad_args.jet': ['regenerate', 'Jet reports E0766 MissingArgument naming `topic`; more exact than the oracle E0104'],
  'cffi_string_literal_interior_nul.jet': ['fixture-stale', 'the fixture declares `fn take`; `take` is now a keyword, so the file stops at parse (E0003 at 4:8)'],
  'sema_soundness/compound_updates_require_their_operator.jet': ['fixture-stale', 'line 8 uses `&=`, retired by D-SIGIL-ALGEBRA1=B; the parse stop at line 8 hides the E0109 rows; also Docs/spec/syntax-decisions.md S17 still lists &= |= ^='],
  'project_module_duplicate/run.jet': ['decide', 'fixture dir has no package.jet, so Jet reports E2393 ImportWithoutProject; give the fixture a project or rebless'],
  'project_module_invalid/run.jet': ['decide', 'fixture dir has no package.jet, so Jet reports E2393 ImportWithoutProject; give the fixture a project or rebless'],
  'project_module_missing/run.jet': ['decide', 'fixture dir has no package.jet, so Jet reports E2393 ImportWithoutProject; give the fixture a project or rebless'],
  'distinct_numeric_ok.jet': ['fix-compiler', 'false positive: "A `Meters` can\'t be used where a `Meters` is expected" under raw header [E0128]; E0128 is now ZipLengthMismatch in the registry, so the distinct check emits a reused code'],
  'distinct_coerce_base.jet': ['fix-compiler', 'extra E0128 from the distinct check; E0128 is now ZipLengthMismatch in the registry (reused code)'],
  'distinct_constructor_alias.jet': ['decide', 'oracle code E0128 is now ZipLengthMismatch; Jet says E0803 "`UserID` is UserID, not a function"; rule which code the distinct-alias constructor gets'],
  'manifest_bad_layer/run.jet': ['decide', 'oracle code E1206 is now LintSelectorIsCode in the registry; Jet says E0003 "Package runtime ceiling is unsupported"; pick the manifest code'],
  'manifest_unknown_field/run.jet': ['decide', 'oracle code E1206 is now LintSelectorIsCode in the registry; Jet says E0003 "Package manifest has no name"; pick the manifest code'],
  'db_typed_sql_all_sinks_plain_string.jet': ['fix-compiler', 'right text ("A runtime `String` can\'t be used as `SQL`") under the wrong code E0102; the oracle code is E0149'],
  'never_return_field.jet': ['fix-compiler', 'right text ("A stored field cannot contain Never") under E0109; the oracle code is E2422'],
  'source_nesting_limit.jet': ['fix-compiler', 'right text ("nested too deeply") under E0003; the oracle code is E1403 NestingTooDeep'],
  'bare_member_callable_arity.jet': ['fix-compiler', 'Jet prints "Nothing named `` exists here" (empty name, E0107)'],
  'inline_module_return_type_mismatch.jet': ['fix-compiler', 'Jet prints mangled type names (`M4Vis24BankAccount`) in E0113'],
  'call_non_function.jet': ['fix-compiler', 'Docs/spec/spec.md:925 and Values.jet:263 make `.call(…)` on a non-function E0803; Jet routes it to E0311 UnknownMethod'],
  'migration_change_impl_ok.jet': ['harness', 'passes when run alone (capture matches the snapshot); fails only inside the suite run with E0910, so state leaks between fixtures [INFERENCE: the shared JET_SCHEMA_CACHE_DIR]'],
  'ffi_fetch_failed.jet': ['harness', 'network/cargo dependent; the snapshot also holds literal \\n escapes the renderer now prints as lines'],
  'ffi_bad_path.jet': ['harness', 'output depends on the cargo registry in the environment (capture differs from the suite run)'],
  'ffi_bad_type.jet': ['harness', 'output depends on the cargo registry in the environment (capture differs from the suite run)'],
  'cffi_e3205/run.jet': ['harness', 'output depends on the C toolchain in the environment (capture differs from the suite run); suite run also moved the span to 1:1'],
  'runtime_e3014.jet': ['fix-compiler', 'internal compiler error, exit 101'],
  'runtime_rng_backward.jet': ['fix-compiler', 'the Stop [E3010] report lost its `--> file:line` location line'],
  'repl_effect_denied_e1803.jet': ['fix-compiler', 'the REPL run exits 0 where the fixture requires a nonzero exit for the denied effect'],
  'bad_map_key.jet': ['fix-compiler', 'the fixed companion bad_map_key.fixed.jet does not check clean'],
  'lint_cognitive_complexity_refactored.jet': ['decide', 'score for `tangled` is 3 (oracle 2); rule which cognitive-complexity count is right'],
};
const EXPECT_NOERR = (t) => (t.exp_first || '').startsWith('(no errors)') || t.exp_codes === '';

const rows = [];
for (const t of tri) {
  const area = AREAS.find(([, f]) => f(t))[0];
  const key = Object.keys(O).find((k) => t.fixture.endsWith('/' + k));
  let verdict, note, how = 'rule';
  if (key) { [verdict, note] = O[key]; how = 'checked'; }
  else if (area === 'H-REFUSED') { verdict = 'harness'; note = t.detail; }
  else if (area === 'H-NOSTREAM') {
    if (/tests\/ui\/ownership\//.test(t.fixture)) { verdict = 'fix-compiler'; note = 'no snapshot on purpose (53257fc2aa "Preserve seven red ownership originals without blessing false snapshots"); bless once the checker rejects it'; }
    else { verdict = 'harness'; note = 'no expected stream was ever written; write it from reviewed output'; }
  }
  else if (area === 'S-TRAILBLANK') { verdict = 'regenerate'; note = 'only difference: the snapshot ends with an extra blank line; every passing snapshot ends without one'; }
  else if (area === 'W-31OCT') { verdict = 'fix-compiler'; note = 'passed in ui-UiGreen15 (2026-10-31 17:34Z), failed from ui-UiGreen16 (17:58Z) onward; a diagnostic after the first one is dropped'; how = 'history'; }
  else if (area === 'D-TEACHPAUSE') { verdict = 'decide'; note = 'oracle expects a retired/foreign-spelling teaching report; D-FOREIGN-TEACH1=B keeps the teaching pause, so the generic Jet error may be the ratified output (regenerate) or the fixture retires'; }
  else if (t.blk_kind === 'no-diagnostic' || (t.capture === 'captured' && t.blk_kind === 'no-diagnostic')) { verdict = 'fix-compiler'; note = 'Jet reports nothing where the oracle rejects'; }
  else if (t.cls === 'lint-silent') { verdict = 'fix-compiler'; note = 'lint fixture emits no warning'; }
  else if (t.blk_kind === 'missing-blocks') { verdict = 'fix-compiler'; note = 'Jet misses oracle diagnostics'; }
  else if (t.blk_kind === 'extra-blocks' && EXPECT_NOERR(t)) { verdict = 'fix-compiler'; note = 'false positive on a program the oracle accepts'; }
  else if (t.blk_kind === 'extra-blocks') { verdict = 'fix-compiler'; note = 'Jet adds diagnostics the oracle does not report'; }
  else if (t.blk_kind === 'text-only' && /span [^,]*>1:1\b|span >/.test(t.blk_changes)) { verdict = 'fix-compiler'; note = 'span lost (points at 1:1 or nowhere)'; }
  else if (t.blk_kind === 'text-only' && /^(\S+ (snippet|span [^,]+)(,(snippet))?;? ?)+$/.test(t.blk_changes)) { verdict = 'fix-compiler'; note = 'same code and text; span or source snippet differs'; }
  else if (t.blk_kind === 'text-only') { verdict = 'fix-compiler'; note = 'same code; Jet text differs from the oracle text (UiGreen practice: restore oracle text in the source, never rebless)'; }
  else if (/swap/.test(t.blk_kind)) { verdict = 'fix-compiler'; note = 'a different rule fires first'; }
  else if (t.blk_kind === 'order-only') { verdict = 'fix-compiler'; note = 'diagnostic order or first rule differs'; }
  else { verdict = 'fix-compiler'; note = t.blk_kind; }
  rows.push({ t, area, verdict, note, how });
}
const CARD = {
  'H-REFUSED': '#6035', 'H-NOSTREAM': '#6035', 'H-ENV': '#6035', 'S-TRAILBLANK': '#6035', 'W-31OCT': '#6076',
  'D-TEACHPAUSE': '#4260', 'A-LINT': '#6006', 'A-TYPESTATE': '#6076', 'A-RUNTIME': '#6035', 'A-PURITY': '#4926',
  'A-PACKAGES': '#4275', 'A-FFI': 'new', 'A-PARSER': '#6281', 'A-CONCURRENCY': 'new', 'A-OWNERSHIP': '#4620',
  'A-FAILURE': 'new', 'A-SECURITY': 'new', 'A-DOMAIN': '#6004', 'A-LIBTYPES': 'new', 'A-TYPES': '#6325',
};
const head = ['fixture', 'tier', 'status', 'kind', 'changes', 'exp_codes', 'act_codes', 'root', 'verdict', 'how', 'note', 'snapshot_content_commit', 'ever_passed', 'batch'];
const esc = (s) => String(s ?? '').replace(/[\t\n]/g, ' ');
const lines = [head.join('\t')];
rows.sort((a, b) => a.area.localeCompare(b.area) || a.verdict.localeCompare(b.verdict) || a.t.fixture.localeCompare(b.t.fixture));
for (const { t, area, verdict, note, how } of rows) {
  const kind = t.status === 'REFUSED' ? 'refused' : t.cls === 'missing-stream' ? 'missing-stream' : t.cls === 'lint-silent' ? 'lint-silent' : t.cls === 'tier-split' ? 'tier-split' : t.blk_kind === 'n/a' ? t.cls : t.blk_kind;
  const ever = t.hist_last_pass ? `yes, last ${t.hist_last_pass}` : `no (${t.hist_seen} runs since 2026-10-27)`;
  lines.push([t.fixture, t.tier || '-', t.status, kind, esc(t.blk_changes).slice(0, 300), t.exp_codes, t.act_codes, area, verdict, how, esc(note), t.content_commit, ever, `${area}/${verdict} ${(verdict==="fix-compiler"&&/reports nothing|misses oracle|emits no warning/.test(note))?"#6425":CARD[area]}`].map(esc).join('\t'));
}
writeFileSync(outPath, lines.join('\n') + '\n');
const sum = {};
for (const { area, verdict } of rows) { sum[area] ??= {}; sum[area][verdict] = (sum[area][verdict] ?? 0) + 1; }
for (const [a, v] of Object.entries(sum).sort()) console.log(a, CARD[a], JSON.stringify(v));
console.log('rows', rows.length, 'checked', rows.filter((r) => r.how === 'checked').length);
