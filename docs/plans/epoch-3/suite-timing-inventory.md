# Suite timing inventory

Measured on branch `m01-timing` after the test build. The inventory covered every root file matched by `tests/*.rs`: 266 binaries.

Commands:

- Build once: `timeout 1800 scripts/agent/jet-env cargo build --tests`
- Measure each binary: `timeout 900 scripts/agent/jet-env cargo test --test NAME --`
- The trailing `--` leaves libtest at its default thread setting.

The bounded pass completed all 266 binaries. No measured wall time exceeded 900 seconds, and no row returned the 900-second timeout status. The inventory command wrote `.inventory2.tsv`; its final summary print had a shell-quote typo after the loop, so the count and offender checks were repeated directly from that completed TSV.

## Pre-split offenders

These were the original over-budget binaries. The first two timings for `jet_perf_trace` are the full pass and its isolated confirmation before the split.

| Original binary | Pre-split wall time | Change |
| --- | ---: | --- |
| `jet_perf_trace` | 901.453s full; 901.441s isolated | Split browser, IO, views, and capture lanes |
| `corelib_platform` | 902.924s | Move the email bound test into its own binary and use the existing string repeat operation to build the large fixture |
| `sema_soundness` | 903.701s | Split metadata, invalid, provenance, and the differential corpus into eight partitions; supervise the known hanging default-dev fixture with a 120s per-case deadline |

## Post-change split evidence

All replacement binaries stayed below 900 seconds in the final inventory. The email binary also passed a clean-cache rerun at 24.96s; its inventory row was contaminated by a concurrent cache temp-directory failure, not a test assertion.

| Replacement binary | Wall time | Result |
| --- | ---: | --- |
| `jet_perf_trace` | 1.313s | 4 passed, 3 existing failures |
| `jet_perf_trace_capture_attach` | 33.242s | 3 passed |
| `jet_perf_trace_capture_run` | 2.446s | 2 passed, 1 existing failure |
| `jet_perf_trace_views` | 4.957s | 6 passed |
| `corelib_platform` | 8.768s | 12 passed, 1 existing failure |
| `corelib_platform_email` | 24.960s clean-cache rerun | 3 passed |
| `sema_soundness` | 0.914s | 4 passed |
| `sema_soundness_invalid` | 19.560s | 2 passed, 1 existing failure |
| `sema_soundness_provenance` | 228.911s | 2 passed, 2 existing failures |
| `sema_soundness_differential` | 138.278s | 2 passed, 1 existing failure |
| `sema_soundness_differential_part2` | 138.586s | 2 passed, 1 existing failure |
| `sema_soundness_differential_part3` | 16.628s | 2 passed, 1 existing failure |
| `sema_soundness_differential_part4` | 11.302s | 2 passed, 1 existing failure |
| `sema_soundness_differential_part5` | 10.179s | 2 passed, 1 existing failure |
| `sema_soundness_differential_part6` | 27.112s | 2 passed, 1 existing failure |
| `sema_soundness_differential_part7` | 21.760s | 2 passed, 1 existing failure |
| `sema_soundness_differential_part8` | 11.589s | 3 passed |

## Complete inventory

| Binary | State | Wall seconds | Harness result |
| --- | --- | ---: | --- |
| `agent_workloads` | FAIL | 99.144 | test result: FAILED. 6 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 98.16s  |
| `allocator_families` | FAIL | 3.082 | test result: FAILED. 9 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.08s  |
| `archive` | FAIL | 14.747 | test result: FAILED. 5 passed; 4 failed; 0 ignored; 0 measured; 0 filtered out; finished in 13.71s  |
| `arena` | FAIL | 5.368 | test result: FAILED. 10 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.76s  |
| `auto_derive_policy` | FAIL | 2.345 | test result: FAILED. 3 passed; 4 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.29s  |
| `ban_bare_panic` | FAIL | 2.437 | test result: FAILED. 3 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.55s  |
| `ban_jetpack_unwrap_expect` | FAIL | 1.491 | test result: FAILED. 7 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.22s  |
| `base_encoding_2026` | PASS | 4.943 | test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.23s  |
| `bind` | PASS | 9.496 | test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 7.99s  |
| `bitwise_not` | PASS | 3.105 | test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.90s  |
| `browser_bidi` | FAIL | 24.546 | test result: FAILED. 26 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out; finished in 23.23s  |
| `browser_lock` | PASS | 2.606 | test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.29s  |
| `build_cache_normalization` | PASS | 1.656 | test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s  |
| `build_cache_race` | PASS | 1.377 | test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.18s  |
| `build_entry` | FAIL | 12.348 | no test-result summary |
| `build_entry_epoch4` | FAIL | 1.759 | test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.26s  |
| `build_graph` | PASS | 1.279 | test result: ok. 48 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.09s  |
| `build_sandbox` | PASS | 0.948 | test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s  |
| `canvas` | FAIL | 8.603 | test result: FAILED. 63 passed; 15 failed; 0 ignored; 0 measured; 0 filtered out; finished in 7.65s  |
| `canvas_scenarios` | FAIL | 1.632 | test result: FAILED. 44 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.59s  |
| `capabilities` | PASS | 1.197 | test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.23s  |
| `feature_acceptance` | PASS | 147.988 | test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 147.01s  |
| `cffi` | FAIL | 5.661 | test result: FAILED. 54 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.75s  |
| `cffi_native_matrix` | FAIL | 0.912 | test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s  |
| `cli` | FAIL | 307.782 | test result: FAILED. 13 passed; 9 failed; 0 ignored; 0 measured; 0 filtered out; finished in 306.81s  |
| `cli_bindings` | FAIL | 33.905 | test result: FAILED. 17 passed; 14 failed; 0 ignored; 0 measured; 0 filtered out; finished in 32.86s  |
| `cli_budget` | FAIL | 277.159 | test result: FAILED. 17 passed; 15 failed; 0 ignored; 0 measured; 0 filtered out; finished in 276.09s  |
| `cli_commands` | FAIL | 33.005 | test result: FAILED. 27 passed; 5 failed; 0 ignored; 0 measured; 0 filtered out; finished in 32.03s  |
| `cli_expand` | PASS | 82.430 | test result: ok. 31 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 81.53s  |
| `cli_positionals` | PASS | 61.224 | test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 60.33s  |
| `cli_runtime` | FAIL | 54.096 | test result: FAILED. 21 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 53.20s  |
| `cli_scene_assets` | PASS | 145.710 | test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 144.82s  |
| `cli_scene_draws` | PASS | 452.455 | test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 451.46s  |
| `cli_scene_forged` | PASS | 275.094 | test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 274.20s  |
| `cli_scene_memory` | FAIL | 7.337 | test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6.44s  |
| `cli_scene_probe` | PASS | 624.556 | test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 623.63s  |
| `cli_surface` | FAIL | 36.452 | test result: FAILED. 26 passed; 6 failed; 0 ignored; 0 measured; 0 filtered out; finished in 35.51s  |
| `closures` | FAIL | 1.413 | test result: FAILED. 13 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.49s  |
| `codemod` | FAIL | 1.259 | test result: FAILED. 17 passed; 8 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.35s  |
| `compiler_api` | PASS | 1.111 | test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.17s  |
| `compiler_stack` | PASS | 1.245 | test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.32s  |
| `comptime_cli_parity` | PASS | 1.254 | test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.34s  |
| `comptime_core_one_home` | FAIL | 0.927 | test result: FAILED. 9 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s  |
| `comptime_core_pure_parity` | FAIL | 7.721 | no test-result summary |
| `comptime_diff` | FAIL | 8.089 | no test-result summary |
| `comptime_sequence_parity` | FAIL | 2.656 | test result: FAILED. 2 passed; 5 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.73s  |
| `compute_extended` | FAIL | 10.029 | test result: FAILED. 10 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 9.11s  |
| `compute_parity` | PASS | 5.541 | test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.62s  |
| `compute_views` | FAIL | 5.664 | test result: FAILED. 6 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.73s  |
| `concurrency_boundaries` | FAIL | 1.555 | test result: FAILED. 28 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.62s  |
| `core_call_table` | FAIL | 0.919 | test result: FAILED. 7 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s  |
| `core_surface_ledger` | FAIL | 1.107 | test result: FAILED. 2 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.20s  |
| `corelib` | PASS | 18.014 | test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 17.10s  |
| `corelib_compile` | FAIL | 8.530 | test result: FAILED. 7 passed; 8 failed; 0 ignored; 0 measured; 0 filtered out; finished in 7.60s  |
| `corelib_derives` | FAIL | 10.102 | test result: FAILED. 28 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 9.15s  |
| `corelib_encoding_streams` | FAIL | 12.683 | no test-result summary |
| `corelib_encoding_surface` | PASS | 9.119 | test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 8.17s  |
| `corelib_http_data` | FAIL | 6.730 | test result: FAILED. 10 passed; 9 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.79s  |
| `corelib_net` | FAIL | 11.849 | test result: FAILED. 11 passed; 24 failed; 0 ignored; 0 measured; 0 filtered out; finished in 10.91s  |
| `corelib_platform` | FAIL | 8.768 | test result: FAILED. 12 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 7.86s  |
| `corelib_platform_email` | FAIL | 6.074 | test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.14s  |
| `corelib_runtime` | FAIL | 14.897 | test result: FAILED. 16 passed; 11 failed; 0 ignored; 0 measured; 0 filtered out; finished in 14.00s  |
| `corelib_system` | FAIL | 66.803 | test result: FAILED. 18 passed; 3 failed; 2 ignored; 0 measured; 0 filtered out; finished in 65.87s  |
| `cross` | FAIL | 1.426 | test result: FAILED. 11 passed; 4 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.51s  |
| `crypto_c12` | FAIL | 1.957 | no test-result summary |
| `crypto_diagnostics` | FAIL | 0.972 | test result: FAILED. 6 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s  |
| `crypto_entropy` | FAIL | 12.747 | test result: FAILED. 17 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 11.81s  |
| `data_bridges` | FAIL | 1.048 | test result: FAILED. 4 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.13s  |
| `data_hostile` | FAIL | 5.682 | test result: FAILED. 6 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.77s  |
| `data_one_kernel` | PASS | 1.539 | test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.63s  |
| `dataflow_stream` | FAIL | 5.220 | test result: FAILED. 2 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.31s  |
| `db_policy` | FAIL | 23.828 | test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 22.91s  |
| `deadline_and_crypto_text_single_home` | PASS | 0.938 | test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s  |
| `deadline_clock_single_home` | PASS | 1.002 | test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.09s  |
| `debug` | FAIL | 1.237 | test result: FAILED. 13 passed; 5 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.32s  |
| `decisions` | PASS | 1.855 | test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.94s  |
| `determinism` | FAIL | 4.835 | test result: FAILED. 22 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.93s  |
| `dev` | FAIL | 7.913 | no test-result summary |
| `devtools` | FAIL | 2.273 | test result: FAILED. 9 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.36s  |
| `diagnostic_gate` | PASS | 28.823 | test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 27.89s  |
| `diagnostic_snapshots` | FAIL | 287.344 | test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 286.42s  |
| `diagnostics_coverage` | FAIL | 2.058 | test result: FAILED. 13 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.14s  |
| `diagnostics_format` | FAIL | 0.945 | test result: FAILED. 5 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s  |
| `dom_runtime_parity` | PASS | 0.908 | test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s  |
| `dsl_blocks` | PASS | 1.219 | test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.31s  |
| `duration_runtime` | FAIL | 1.222 | test result: FAILED. 5 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.29s  |
| `effects` | FAIL | 1.702 | test result: FAILED. 48 passed; 10 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.76s  |
| `encoding_corpus` | FAIL | 5.570 | test result: FAILED. 8 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.64s  |
| `encoding_edition` | PASS | 1.091 | test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.14s  |
| `encoding_hostile_io` | PASS | 73.305 | test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 72.35s  |
| `encoding_parity` | FAIL | 19.928 | test result: FAILED. 15 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out; finished in 18.92s  |
| `encoding_variance` | FAIL | 57.906 | test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 56.96s  |
| `engine_dispatch` | PASS | 1.254 | test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.32s  |
| `env_dev_trust` | FAIL | 0.971 | test result: FAILED. 9 passed; 6 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s  |
| `env_hook` | FAIL | 0.961 | test result: FAILED. 15 passed; 4 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s  |
| `env_overlay` | PASS | 5.913 | test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.99s  |
| `event_hooks` | PASS | 5.159 | test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.24s  |
| `event_observations` | FAIL | 37.875 | test result: FAILED. 5 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 36.95s  |
| `example_artifacts` | PASS | 0.915 | test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s  |
| `fact_declaration_tiers` | FAIL | 2.302 | test result: FAILED. 11 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.37s  |
| `fenced_names` | PASS | 2.364 | test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.43s  |
| `fleet` | PASS | 0.945 | test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s  |
| `floor_division` | FAIL | 3.731 | test result: FAILED. 17 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.80s  |
| `flow_facts_tiers` | PASS | 2.306 | test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.38s  |
| `fmt` | FAIL | 1.362 | test result: FAILED. 165 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.44s  |
| `fmt_project` | PASS | 0.949 | test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s  |
| `fuzz_sema` | FAIL | 45.884 | test result: FAILED. 2 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 44.97s  |
| `gen_errors` | PASS | 0.928 | test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s  |
| `generics` | PASS | 31.627 | test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 30.70s  |
| `golden` | FAIL | 132.968 | test result: FAILED. 5 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 132.03s  |
| `grammar` | PASS | 1.036 | test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s  |
| `guard_selftest` | PASS | 0.897 | test result: ok. 3 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.00s  |
| `hardening` | FAIL | 1.226 | test result: FAILED. 18 passed; 5 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.29s  |
| `harness_tools` | PASS | 0.904 | test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s  |
| `help_pty` | PASS | 1.950 | test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.05s  |
| `http_client_law` | FAIL | 29.832 | test result: FAILED. 33 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 28.89s  |
| `http_dependency_audit` | FAIL | 0.932 | test result: FAILED. 5 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s  |
| `http_i9` | FAIL | 1.104 | test result: FAILED. 3 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.18s  |
| `http_server_lifecycle` | PASS | 1.934 | test result: ok. 77 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.03s  |
| `http_server_tls` | FAIL | 11.294 | test result: FAILED. 3 passed; 5 failed; 0 ignored; 0 measured; 0 filtered out; finished in 10.36s  |
| `ice_regressions` | FAIL | 5.780 | test result: FAILED. 8 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.86s  |
| `ice_report_single_home` | PASS | 0.939 | test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s  |
| `image` | FAIL | 0.974 | test result: FAILED. 13 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s  |
| `impact` | PASS | 0.960 | test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s  |
| `inline_deps` | FAIL | 0.970 | test result: FAILED. 7 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s  |
| `int_division` | PASS | 6.277 | test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.35s  |
| `interpreter_extension_lint_policy` | FAIL | 0.967 | test result: FAILED. 2 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s  |
| `jet_perf_trace` | FAIL | 1.313 | test result: FAILED. 4 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.38s  |
| `jet_perf_trace_capture_attach` | PASS | 33.242 | test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 32.33s  |
| `jet_perf_trace_capture_run` | FAIL | 2.446 | test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.53s  |
| `jet_perf_trace_views` | PASS | 4.957 | test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.06s  |
| `jet_test` | PASS | 76.302 | test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 75.36s  |
| `jetpack_build_debug` | FAIL | 0.976 | test result: FAILED. 6 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s  |
| `jetpack_discovery` | PASS | 0.936 | test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s  |
| `jetpack_dispatch` | PASS | 1.004 | test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.09s  |
| `jetpack_engine` | FAIL | 1.435 | test result: FAILED. 85 passed; 11 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.52s  |
| `jetpack_hangar_store_v2` | FAIL | 1.012 | test result: FAILED. 4 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.10s  |
| `jetpack_jetos` | PASS | 0.907 | test result: ok. 2 passed; 0 failed; 49 ignored; 0 measured; 0 filtered out; finished in 0.00s  |
| `jetpack_nix_eval_boundary` | PASS | 0.916 | test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s  |
| `jetpack_no_daemon` | FAIL | 0.957 | test result: FAILED. 5 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s  |
| `jetpack_offline` | PASS | 1.174 | test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.26s  |
| `jetpack_output` | PASS | 0.966 | test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s  |
| `jetpack_platform` | PASS | 0.902 | test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s  |
| `jetpack_semantic_lock` | PASS | 0.916 | test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s  |
| `jetpack_services` | FAIL | 9.456 | test result: FAILED. 14 passed; 3 failed; 1 ignored; 0 measured; 0 filtered out; finished in 8.54s  |
| `jetpack_studio` | FAIL | 134.393 | test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 133.48s  |
| `jetpack_tasks` | FAIL | 5.617 | test result: FAILED. 4 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.70s  |
| `jetpack_tool` | FAIL | 342.550 | test result: FAILED. 12 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out; finished in 341.63s  |
| `jetpack_trust_root` | PASS | 0.939 | test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s  |
| `jetpack_truth` | FAIL | 1.676 | test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.75s  |
| `jetpack_variants` | PASS | 0.919 | test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s  |
| `jit_run` | FAIL | 2.709 | no test-result summary |
| `layout` | PASS | 2.572 | test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.66s  |
| `lib_use` | FAIL | 0.977 | test result: FAILED. 5 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s  |
| `library_outputs` | FAIL | 31.824 | test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 30.93s  |
| `live_inspect` | PASS | 32.893 | test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 31.97s  |
| `local_cell_tiers` | FAIL | 5.070 | test result: FAILED. 7 passed; 4 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.13s  |
| `lsp` | FAIL | 64.489 | test result: FAILED. 31 passed; 19 failed; 0 ignored; 0 measured; 0 filtered out; finished in 63.55s  |
| `marker_declarations` | FAIL | 1.359 | test result: FAILED. 7 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.43s  |
| `marker_registry_coverage` | PASS | 0.931 | test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s  |
| `marker_rule_signatures` | PASS | 1.437 | test result: ok. 26 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.51s  |
| `math_display_parity` | FAIL | 31.864 | test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 30.94s  |
| `modules` | PASS | 2.428 | test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.50s  |
| `net_tls` | PASS | 0.921 | test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s  |
| `notebook` | PASS | 1.072 | test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.14s  |
| `numeric_widening` | PASS | 5.286 | test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.36s  |
| `numops` | FAIL | 1.897 | test result: FAILED. 9 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.97s  |
| `observe` | FAIL | 12.083 | test result: FAILED. 13 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 11.15s  |
| `os_interrupt_windows` | PASS | 0.873 | test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s  |
| `os_native` | FAIL | 5.386 | test result: FAILED. 3 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.48s  |
| `outcome_carrier` | FAIL | 1.157 | test result: FAILED. 7 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.22s  |
| `output_callable` | FAIL | 28.601 | test result: FAILED. 11 passed; 4 failed; 0 ignored; 0 measured; 0 filtered out; finished in 27.67s  |
| `ownership` | FAIL | 115.912 | test result: FAILED. 182 passed; 16 failed; 0 ignored; 0 measured; 0 filtered out; finished in 114.93s  |
| `package_outputs` | PASS | 28.891 | test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 27.97s  |
| `package_root_single_home` | PASS | 1.264 | test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.32s  |
| `parity` | FAIL | 1.691 | test result: FAILED. 3 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.77s  |
| `parity_lint` | FAIL | 1.160 | test result: FAILED. 5 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.24s  |
| `path_durability` | PASS | 5.048 | test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.12s  |
| `performance_budget_providers` | PASS | 0.924 | test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s  |
| `pin` | PASS | 6.476 | test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.56s  |
| `pkg` | PASS | 59.763 | test result: ok. 151 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 58.84s  |
| `policy_scope` | PASS | 1.046 | test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.12s  |
| `polyglot_systems` | FAIL | 1.463 | test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.55s  |
| `pool_id_equality` | FAIL | 1.010 | test result: FAILED. 3 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.09s  |
| `power_and_exclusive_or` | PASS | 2.395 | test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.47s  |
| `prove` | FAIL | 156.770 | test result: FAILED. 30 passed; 6 failed; 0 ignored; 0 measured; 0 filtered out; finished in 155.85s  |
| `provider_visibility` | PASS | 1.198 | test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.24s  |
| `pure` | PASS | 1.184 | test result: ok. 25 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.20s  |
| `reflect` | PASS | 0.952 | test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s  |
| `regex` | PASS | 5.890 | test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.99s  |
| `release_gates` | FAIL | 189.749 | test result: FAILED. 15 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 188.80s  |
| `repl` | FAIL | 11.565 | test result: FAILED. 90 passed; 59 failed; 1 ignored; 0 measured; 0 filtered out; finished in 10.63s  |
| `report` | PASS | 29.897 | test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 29.01s  |
| `resource_close` | FAIL | 2.511 | test result: FAILED. 12 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.58s  |
| `retirement_ratchet` | FAIL | 1.227 | test result: FAILED. 5 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.29s  |
| `ring_layer` | PASS | 1.063 | test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.13s  |
| `ring_shipping` | PASS | 0.921 | test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s  |
| `rollback` | PASS | 1.885 | test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.94s  |
| `run_cache` | PASS | 2.628 | test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.68s  |
| `run_entry` | FAIL | 30.257 | test result: FAILED. 18 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out; finished in 29.31s  |
| `run_interpret` | FAIL | 1.023 | test result: FAILED. 3 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.08s  |
| `run_tier_diagnostics` | FAIL | 1.361 | test result: FAILED. 3 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.42s  |
| `scheduler_native` | FAIL | 2.014 | test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.08s  |
| `secrets` | PASS | 2.546 | test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.62s  |
| `secrets_expiry` | PASS | 5.796 | test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.80s  |
| `sema_soundness` | PASS | 0.914 | test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s  |
| `sema_soundness_differential` | FAIL | 138.278 | test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 137.37s  |
| `sema_soundness_differential_part2` | FAIL | 138.586 | test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 137.67s  |
| `sema_soundness_differential_part3` | FAIL | 16.628 | test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 15.69s  |
| `sema_soundness_differential_part4` | FAIL | 11.302 | test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 10.36s  |
| `sema_soundness_differential_part5` | FAIL | 10.179 | test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 9.24s  |
| `sema_soundness_differential_part6` | FAIL | 27.112 | test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 26.18s  |
| `sema_soundness_differential_part7` | FAIL | 21.760 | test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 20.82s  |
| `sema_soundness_differential_part8` | PASS | 11.589 | test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 10.66s  |
| `sema_soundness_invalid` | FAIL | 19.560 | test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 18.64s  |
| `sema_soundness_provenance` | FAIL | 228.911 | test result: FAILED. 2 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 227.97s  |
| `semindex` | FAIL | 2.080 | no test-result summary |
| `services_runtime` | PASS | 6.677 | test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.76s  |
| `shared_guards` | PASS | 5.821 | test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.87s  |
| `shared_weak` | PASS | 5.069 | test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.14s  |
| `single_use` | FAIL | 1.189 | test result: FAILED. 9 passed; 9 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.26s  |
| `source_import` | FAIL | 0.906 | test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s  |
| `structural_merge` | FAIL | 1.324 | test result: FAILED. 13 passed; 4 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.42s  |
| `sync_crdt` | PASS | 5.553 | test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.64s  |
| `syntax_reconciliation` | FAIL | 2.468 | test result: FAILED. 20 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.56s  |
| `tag_group_pattern_coverage` | PASS | 1.005 | test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.08s  |
| `taint` | PASS | 1.360 | test result: ok. 26 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.43s  |
| `target_machines` | FAIL | 1.699 | no test-result summary |
| `task_control_tiers` | FAIL | 14.062 | test result: FAILED. 10 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 13.15s  |
| `taskgroup_params` | FAIL | 21.818 | test result: FAILED. 9 passed; 6 failed; 0 ignored; 0 measured; 0 filtered out; finished in 20.88s  |
| `terminal_default_single_home` | FAIL | 1.388 | test result: FAILED. 3 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.48s  |
| `text_unicode` | FAIL | 2.427 | test result: FAILED. 3 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.49s  |
| `tir_collections_and_methods` | PASS | 6.465 | test result: ok. 35 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.54s  |
| `tir_control_and_data` | FAIL | 6.199 | test result: FAILED. 25 passed; 9 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.27s  |
| `tir_core_and_closures` | FAIL | 6.660 | test result: FAILED. 25 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.73s  |
| `tir_data_math_reactive` | FAIL | 5.509 | test result: FAILED. 8 passed; 6 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.58s  |
| `tir_exhaustive_match` | FAIL | 0.921 | test result: FAILED. 13 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s  |
| `tir_io_and_ownership` | FAIL | 9.375 | test result: FAILED. 12 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out; finished in 8.45s  |
| `tir_language_features` | FAIL | 6.779 | test result: FAILED. 24 passed; 9 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.84s  |
| `tir_modules_and_enums` | FAIL | 5.925 | test result: FAILED. 25 passed; 4 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.00s  |
| `tir_operators_and_runtime` | FAIL | 2.316 | test result: FAILED. 7 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.38s  |
| `tir_patterns_and_fields` | FAIL | 5.627 | test result: FAILED. 18 passed; 9 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.69s  |
| `tir_target_structure` | FAIL | 0.916 | test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s  |
| `tir_unsafe_and_runtime` | FAIL | 10.275 | test result: FAILED. 25 passed; 5 failed; 0 ignored; 0 measured; 0 filtered out; finished in 9.35s  |
| `toolchain_pin` | PASS | 28.855 | test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 27.94s  |
| `truthfulness` | FAIL | 2.179 | test result: FAILED. 22 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.26s  |
| `type_sentinel_guard` | FAIL | 0.939 | test result: FAILED. 3 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s  |
| `typestate` | PASS | 1.269 | test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.35s  |
| `ui_backend` | FAIL | 5.074 | test result: FAILED. 3 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.16s  |
| `ui_fixes` | FAIL | 10.290 | test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 9.36s  |
| `uninit_fixed` | PASS | 2.606 | test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.66s  |
| `unit_family` | FAIL | 5.619 | test result: FAILED. 41 passed; 13 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.69s  |
| `vault_key_wrap` | FAIL | 43.660 | test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 42.73s  |
| `vault_keys` | PASS | 10.226 | test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 9.31s  |
| `web_app` | FAIL | 3.980 | test result: FAILED. 7 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.07s  |
| `web_app_graph` | FAIL | 1.592 | no test-result summary |
| `web_browser` | PASS | 0.926 | test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s  |
| `web_build` | FAIL | 1.781 | test result: FAILED. 31 passed; 31 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.84s  |
| `web_dev` | FAIL | 30.971 | test result: FAILED. 7 passed; 4 failed; 0 ignored; 0 measured; 0 filtered out; finished in 30.04s  |
| `web_examples_doc` | FAIL | 0.943 | test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s  |
| `web_partition` | FAIL | 1.280 | test result: FAILED. 14 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.38s  |
| `web_tir_contract` | PASS | 0.927 | test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s  |
| `workspace` | PASS | 0.935 | test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s  |
| `workspace_crates` | FAIL | 1.714 | test result: FAILED. 2 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.77s  |
| `ws_law` | FAIL | 1.437 | test result: FAILED. 20 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.51s  |
| `zip_family` | PASS | 2.812 | test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.86s |
