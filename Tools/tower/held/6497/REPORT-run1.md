# wave 22 on 69a987b5a66f7d9c8f939bf6ed1f36b37f8a0aa9: chain FIXED_POINT
## stale patches
## goldens
| status | base | wave |
|---|---:|---:|
| passed | 1203 | 1198 |
| failed | 0 | 4 |
| no-match | 0 | 1 |
| known-failing | 5 | 5 |
| unexpected-pass | 0 | 0 |
| timeout | 0 | 0 |
| skipped | 62 | 62 |
| total | 1270 | 1270 |

## Regressions: pass on base, not on wave (5)

| example | wave |
|---|---|
| basics/first_hour | failed E1803 Application authority is undecided for `Exec` |
| basics/onboarding | failed E1803 Application authority is undecided for `Exec` |
| cli/typed_entry_args | failed E1803 Application authority is undecided for `Env, Exec` |
| errors/env_divisor_trap | no-match E1803 Application authority is undecided for `Exec` |
| web/app_typed_args | failed E1803 Application authority is undecided for `Exec` |

## Gains: pass on wave, not on base (0)

REGRESSIONS 5
## ui
base PASS 1797  new PASS 1798  gained 1  lost 0
## cases
base PASS 2335  new PASS 2446  gained 124  lost 13
LOST	FAIL	tests/cases/cli_parts_budget/local_cli_type_wins_over_same_named_import.case	-
LOST	FAIL	tests/cases/cli_parts_budget/typed_cli_entry_accepts_an_imported_argument_type.case	-
LOST	FAIL	tests/cases/cli_parts_budget/typed_cli_field_markers_add_short_and_env_inputs_with_pinned_precedence.case	-
LOST	FAIL	tests/cases/cli_parts_surface/derived_help_uses_program_basename_for_compiled_and_jet_run_paths.case	-
LOST	FAIL	tests/cases/cli_positionals/surplus_bare_arguments_are_parse_errors_with_usage.case	-
LOST	FAIL	tests/cases/comptime_cli_parity/canonical_cli_recipe_rejects_bad_argv_and_matches_native_tiers.case	-
LOST	FAIL	tests/cases/comptime_cli_parity/typed_entry_optional_value_flag_matches_aot_and_default_run.case	-
LOST	FAIL	tests/cases/ice_regressions/b6_generated_trait_protocol_returns_are_raw.case	-
LOST	FAIL	tests/cases/memory_semantics/blocks_shared_by_two_tasks_are_freed_once.case	-
LOST	FAIL	tests/cases/output_callable/compiled_imported_typed_fallible_entry_uses_its_defining_module.case	-
LOST	FAIL	tests/cases/pkg/cli_beginner_onboarding_ordered_workflow_uses_real_binary.case	-
LOST	FAIL	tests/cases/web_app/typed_app_args_report_invalid_port_with_shared_fix.case	-
LOST	FAIL	tests/cases/web_app/typed_app_args_serve_in_default_and_aot_modes.case	-
## patches and their paths
CardClose-52.patch	tests/cases/ui_fixes/deprecated_rename_applies_only_where_the_call_still_checks.fixture/run.jet tests/cases/ui_fixes/deprecated_rename_applies_only_where_the_call_still_checks.fixed tests/cases/ui_fixes/deprecated_rename_applies_only_where_the_call_still_checks.check.stderr tests/cases/ui_fixes/deprecated_rename_applies_only_where_the_call_still_checks.case Tools/testkit/known-failing.tsv Docs/spec/reference/cli.md Compiler/JetSema/Tests/StructureLifecycle.jet Compiler/JetSema/Source/Sema/Edition.jet Compiler/JetCli/Tests/SourceTransaction.jet Compiler/JetCli/Source/Cli/Try.jet Compiler/JetCli/Source/Cli/SourceTransaction.jet Compiler/JetCli/Source/Cli/LspFixes.jet Compiler/JetCli/Source/Cli/FixCommands.jet Compiler/JetCli/Source/Cli/Codemod.jet 
FFIDesign-1.patch	Core/Args/Args.jet Compiler/Prelude/Core.jet Compiler/JetSema/Source/Sema/Registration/CliEntry.jet Compiler/JetFoundation/Source/Registry/CoreSourceOwned.jet Compiler/JetFoundation/Source/Registry/CoreEnumRows.jet Compiler/JetFoundation/Source/Registry/CoreCallRows.jet Compiler/JetCli/Source/Cli/StructureRegistry.jet Compiler/JetCli/Source/Cli/CoreSources.jet 
GoldC-62.patch	tests/ui/fixed_list_index_guard_off_by_one.stderr tests/ui/fixed_list_index_guard_off_by_one.jet tests/cases/web_build/web_scene_counter_mounts_into_jet_app_and_clicks_repaint.stdout.expected tests/cases/web_build/web_scene_counter_mounts_into_jet_app_and_clicks_repaint.fixture/main.jet tests/cases/web_build/web_scene_counter_mounts_into_jet_app_and_clicks_repaint.fixture/harness.mjs tests/cases/web_build/web_scene_counter_mounts_into_jet_app_and_clicks_repaint.case tests/cases/fixed_list_values/an_exit_guard_narrows_a_byte_index_into_a_fixed_list.out tests/cases/fixed_list_values/an_exit_guard_narrows_a_byte_index_into_a_fixed_list.fixture/main.jet tests/cases/fixed_list_values/an_exit_guard_narrows_a_byte_index_into_a_fixed_list.case Docs/spec/spec.md Docs/spec/reference/web-backend-wasm.md Compiler/JetWeb/Source/Web/Host.jet Compiler/JetSema/Source/Sema/Statements.jet Compiler/JetRuntime/Source/UiBackends.jet 
GoldC-63.patch	tests/cases/runtime_stops/fixtures/a_stop_report_reads_its_source_from_the_run_directory_on_every_tier/app/overflow.jet tests/cases/runtime_stops/a_stop_report_reads_its_source_from_the_run_directory_on_every_tier.case tests/cases/control_and_data/source_loop_strides_walk_every_source_on_every_tier.stop.out tests/cases/control_and_data/source_loop_strides_walk_every_source_on_every_tier.out tests/cases/control_and_data/source_loop_strides_walk_every_source_on_every_tier.fixture/stop.jet tests/cases/control_and_data/source_loop_strides_walk_every_source_on_every_tier.fixture/main.jet tests/cases/control_and_data/source_loop_strides_walk_every_source_on_every_tier.case tests/cases/atomic_values/atomic_try_new_builds_the_same_word_as_new_on_every_tier.out tests/cases/atomic_values/atomic_try_new_builds_the_same_word_as_new_on_every_tier.fixture/main.jet tests/cases/atomic_values/atomic_try_new_builds_the_same_word_as_new_on_every_tier.case Tools/testkit/known-failing.tsv Compiler/JetRuntime/Tests/AuthorityRoutes.tsv Compiler/JetRuntime/Source/Fault.jet Compiler/JetRuntime/Assets/FaultReport/report.S Compiler/JetIR/Source/RuntimeRoutes.jet Compiler/JetIR/Source/Lint.jet Compiler/JetIR/Source/BuildLazy.jet Compiler/JetIR/Source/BuildExpr.jet Compiler/JetIR/Source/BuildChannels.jet Compiler/JetCli/Source/Cli/ScriptCache.jet Compiler/JetCli/Source/Cli/Host.jet Compiler/JetBackend/Source/X64/FaultReport.jet 
UiGreen-37.patch	tests/cases/allocator_families/checked_alloc_request_is_decided_at_runtime_init.fixture/main.jet tests/cases/allocator_families/checked_alloc_request_is_decided_at_runtime_init.case Docs/spec/architecture.md Compiler/JetRuntime/Tests/run-alloc-parity.mjs Compiler/JetRuntime/Tests/EmbedHost.c Compiler/JetRuntime/lifecycle.list Compiler/JetWeb/Source/Web/Module.jet Compiler/JetIR/Source/BuildLibrary.jet Compiler/JetIR/Source/EntryOwned.jet Compiler/JetRuntime/Source/HostStartup.jet Compiler/JetRuntime/Source/Alloc.jet 
