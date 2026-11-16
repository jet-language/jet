# ui triage on a pinned tree (#6035 c15, 2026-11-04)

Rows: `triage.tsv` (352 rows, one per non-PASS ui cell). Prep-2 ran this for Main.

## Run

- Tree: git-archive export of 125e2e8f89. Compiler: the discovery base build of 125e2e8f89.
- Result: PASS 1888, FAIL 318, KF 114, REFUSED 33, ERROR 1. Wall time 314 s.
- The 352 non-PASS rows are the same 352 as an earlier run whose source tree did not match its compiler, so that mix did not cause them.
- testkit cuts its diff at 37 lines. scripts/capture.mjs re-ran 279 FAIL rows the testkit way (`jet check [--verbose] FILE`, rendered stderr only) to get the full actual output. scripts/blocks.mjs then compared diagnostic blocks (code, span, head, Why, Fix).

## Main finding: almost nothing here is a recent regression

- **349 of 352 rows never passed** in any of the 268 recorded full ui runs (the Main lane run logs, outside the repo, 2026-10-27 to 11-04). The first run, on 10-27, already showed FAIL 264, REFUSED 33 and ERROR 52.
- Snapshot content era, by the last commit that changed more than the header respelling:
  - 227 rows are Rust-era (before the 2026-10-23 cutover). These are the Rust checker's oracle output.
  - 63 rows are Jet-era.
  - 62 rows have no snapshot (33 REFUSED, 11 with no stream, 15 lint-silent, 3 others).
- 37eae4f5d6 (2026-11-04) respelled `Error [E0803]` as `Error [jet.Name]` in 1875 ui snapshots. It only changed headers and caused no failure.
- The ui ratchet only counts newly lost passes ("0 lost"), so it hid 349 rows that were never green.
- Verdict rule: the UiGreen campaign practice is "restore oracle text in the sources, never re-bless" (1c41206b13, UiGreen-2). So a mismatch is `fix-compiler` unless a decision or a clear registry row makes the Jet output right.

## Verdict totals

| verdict | rows | meaning |
|---|---|---|
| fix-compiler | 279 | Jet differs from the oracle and nothing ratified the new output |
| harness | 41 | testkit, runner or environment, not the compiler |
| decide | 16 | needs an owner ruling before anyone fixes or reblesses |
| regenerate | 14 | the Jet output is the ratified one; rebless from real output |
| fixture-stale | 2 | the fixture source uses a retired or now-reserved spelling |

The `how` column says how each verdict was reached. `checked` (33 rows) means both outputs were read and the registry or spec. `history` (3 rows) comes from the run history. `rule` comes from the kind of mismatch: nothing reported, missing, extra, a different code first, or text only. Before a `rule` row is fixed, its owner should spot-check it.

## Batches (one per root cause; the `batch` column is `root/verdict card`)

Cards are suggestions [INFERENCE] unless the area clearly maps. `new` means no open card fits.

1. **H-REFUSED, 33, harness, #6035.** testkit does not port these CLI fixture directives yet: programmable_build ×9, target ×5 (wasip2), no-os ×3, generated_source ×2, and 14 single ones. The fix is in the testkit runner, not the compiler.
2. **H-NOSTREAM, 11.**
   - 8 tests/ui/ownership/* rows have no snapshot on purpose: 53257fc2aa "Preserve seven red ownership originals without blessing false snapshots". Bless them once the checker rejects them. They are `fix-compiler`.
   - 3 rows never had a stream written: e1803_script_spawn_needs_exec, web_environment_host_unsupported and ui_lint/unreachable_else_enum. Write a stream from reviewed output.
3. **H-ENV, 5, harness, #6035.**
   - migration_change_impl_ok matches its snapshot when run alone. Inside the suite it gets E0910, so state leaks between fixtures [INFERENCE: the shared JET_SCHEMA_CACHE_DIR].
   - ffi_fetch_failed, ffi_bad_path, ffi_bad_type and cffi_e3205/run depend on the network, cargo or the C toolchain.
4. **S-TRAILBLANK, 7, regenerate, #6035.** Each snapshot ends with an extra blank line; all 1800 passing snapshots end without one. The stray line came in 853489ab06 (CardClose-66, 11-02). Rebless all 7: layout_columnar_partial, result_handler_non_result, retired_cli_marker, semaphore_teaching, serde_rename_not_string, todo_call_e0803, unknown_char.
5. **W-31OCT, 3, fix-compiler (real regressions).** cell_guard_storage (E0217@12:13 missing), typestate_unknown_state and typestate/unknown_state_in_marker_is_e0151 (E0150 missing after E0151).
   - These are the only rows that passed and then failed. They passed in ui-UiGreen15.txt (2026-10-31 17:34Z) and first failed in ui-UiGreen16.txt (17:58Z).
   - The candidate cause is the UiGreen batch landed at 17:18 -0400: 575d2f44e1 (UiGreen-15), d7d1114945 (UiGreen-16, literal field reports), 2a64086b44, 109dfcf587, 199517decc and 17983e7451. [INFERENCE] d7d1114945 touches literal checking, and E0217 is lost at the `[guard]` list literal. Bisect it on a built compiler; no older base compiler exists.
   - Owner: #6076 for the typestate rows; the ownership owner for cell_guard_storage.
6. **D-TEACHPAUSE, 9, decide, #4260.** The oracle expects teaching reports for retired or foreign spellings: channel_retired_forms, cli_e0043_install, foreign_declaration_binding, foreign_method_names_list, phantom_fact_menu_capability, suppress_retired, tracked_origin_runtime_projection, try_suffix_staged and http_message_literal_migration.
   - D-FOREIGN-TEACH1=B (ratified 10-28) keeps the teaching pause. D-S14-PAUSE=A (owner comment "NO RETIRED SPELLINGS either") said to delete such fixtures.
   - So either the generic Jet error is ratified (rebless), or the fixtures retire to held/. Some registry rows (for example E0043 ForeignInstallCommand, which has 0 emit sites) then go inactive. semaphore_teaching shows Jet still teaches one foreign word, so the pause is not applied evenly.
7. **A-OWNERSHIP, 67, fix-compiler, #4620; the 23 silent or missing rows go to #6425.** Covers ownership/views/cells, lambda captures, write windows and take/move. 48 rows are Rust-era.
   - Recurring swaps: E0212→E2305 ("Returned views need a stable owner relationship") ×4, E0204→E0310 ×2, and E0111/E0112→E0205 ("read access only").
   - 11 rows report nothing, 12 miss blocks and 13 add blocks.
8. **A-TYPES, 31, #6325; 3 silent rows to #6425.** Call, label, generic, variadic and pattern routing.
   - Regenerate: nested_callable_containers_keep_required_public_labels (E0771) and user_policy_bad_args (E0766).
   - Clear defects:
     - call_non_function: spec.md:925 says E0803, Jet gives E0311.
     - bare_member_callable_arity: Jet says "Nothing named `` exists here".
     - primitive_static_to_string: "Nothing named Bool".
     - unknown_type, and variadic_* (E1313/E1314 lost).
     - bad_map_key: its fixed companion does not check clean.
9. **A-PARSER, 26, #6281.**
   - Regenerate: meta_on_expression (E0349), off_debug_attr_on_expression (E0343) and tuple_duplicate_field (E0105).
   - Fixture-stale: sema_soundness/compound_updates_require_their_operator. Its `&=` was retired by D-SIGIL-ALGEBRA1=B, but Docs/spec/syntax-decisions.md S17 still lists `&=`, `|=` and `^=`, so that doc is stale too.
   - Defects: source_nesting_limit (right text under E0003 instead of E1403), inline_module_return_type_mismatch (mangled `M4Vis24BankAccount`), enum dot forms, task_spawn_parameters, matrix_measure_inner_mismatch, loop_bindingless_named_bindings and derive_tag.
10. **A-LINT, 25, #6006 (Map: Lints); 20 silent rows also #6425.**
    - 15 lint fixtures emit nothing. These codes have no emit site at all: L0508, L1401, L1141, L0506, L0510, L0524, L0518, L0302 and L0519. L0514 and L0528 are emitted only from LSP or CLI data.
    - Decide: lint_cognitive_complexity_refactored (`tangled` scores 3; the oracle says 2).
11. **A-PACKAGES, 24, #4275.** Packages, manifest, modules, CLI and authority.
    - Decide (5):
      - project_module_duplicate/invalid/missing: the fixture dirs have no package.jet, so Jet's E2393 ImportWithoutProject is arguably right. Add a project or rebless.
      - manifest_bad_layer and manifest_unknown_field: the oracle code E1206 is now LintSelectorIsCode in the registry.
    - Defects: use_unrealized_library (E0983 has 0 emit sites), inline_package_cycle (E0623), stale lock (E0603) and secret_ungranted (span moved to 3:1).
12. **A-LIBTYPES, 23, new; 3 silent rows to #6425.** web, serde, db, reflect, compiler_api, corelib_parts and lazy iterators.
    - Regenerate: path_builtin_not_core_record (argument mismatch is E0112).
    - Defect: db_typed_sql_all_sinks_plain_string (right text under E0102; the oracle has E0149).
13. **A-SECURITY, 18, new; 5 silent rows to #6425.** crypto, taint, vault, transact and arena memory denial (E0921).
14. **A-DOMAIN, 17, #6004.** quantity, coordinate, distinct, computed enum and core.math.
    - distinct_numeric_ok and distinct_coerce_base give a false positive, "A `Meters` can't be used where a `Meters` is expected". It is printed under a raw `[E0128]` header, but E0128 is now ZipLengthMismatch: a reused code.
    - Decide: distinct_constructor_alias (the same E0128 collision).
15. **A-PURITY, 13, #4926.** pure, determinism, comptime and E34xx.
    - Regenerate: effect_math_random_in_pure_fn (E3403).
    - Defects: comptime_net_fetch_* (`core.net` has no item `fetch`), clock_system_pure and impure_no_flag.
16. **A-CONCURRENCY, 12, new.** Tasks, parallel adapters, fixed backing crossing and the bidi session.
17. **A-FAILURE, 12, new.** Result and failure contracts, plus never-return. never_return_field has the right text under E0109; the oracle code is E2422.
18. **A-FFI, 9, new; 3 silent rows to #6425.** cffi, ffi, lowlevel and unsafe obligations.
    - Fixture-stale: cffi_string_literal_interior_nul declares `fn take`, and `take` is now a keyword.
    - cffi_e3204_two_use_forms: the span moved to 1:1.
19. **A-TYPESTATE, 4, #6076.** protocol_out_of_order, typestate_state_diverges, ui_lint/typestate_unreachable (order) and one Rust-era row.
20. **A-RUNTIME, 3, #6035.**
    - runtime_e3014: ICE, exit 101.
    - runtime_rng_backward: the Stop report lost its `--> file:line` line. This is the same family as c17 [INFERENCE].
    - view_mut_bounds_interpreter.

## Side findings

- Reused codes break oracle snapshots: E0128 (now ZipLengthMismatch), E1206 (now LintSelectorIsCode). Codes with no emit site: E0043, E0983 and 9 lint codes. These belong with #4260 (I4 diagnostics gaps).
- Docs/spec/syntax-decisions.md S17 conflicts with D-SIGIL-ALGEBRA1=B (`&=`, `|=` and `^=` are retired).

## How to rerun

From a git checkout that has 125e2e8f89, with `B` the compiler build dir and `T` a git-archive export of the same commit:

```sh
export JET=$B/jet JET_COMPILER_IDENTITY="$(cat $B/IDENTITY)" JET_TOOLCHAIN_ROOT=$T
(cd $T && $JET run Tools/testkit/run.jet -- ui --jet $JET --results ~/uit/ui.json)
A=Docs/audits/ui-triage-2026-11/scripts
node $A/uitri.mjs ~/uit/ui.json $T ~/uit/tri.json        # one row per non-PASS cell (env REPO, REV)
node $A/lastcontent.mjs ~/uit/tri.json                    # last content commit beyond the header respelling
node $A/uihist.mjs ~/uit/tri.json <dir with ui*.txt run logs>...
node $A/capture.mjs $T ~/uit/tri.json ~/uit/cap           # full actual output; testkit cuts its diff at 37 lines
node $A/blocks.mjs ~/uit/ui.json $T ~/uit/tri.json ~/uit/cap
node $A/assign.mjs $T ~/uit/tri.json ~/uit/cap ~/uit/triage.tsv
node $A/show.mjs $T ~/uit/tri.json ~/uit/cap ui/unknown_type.jet   # expected then actual for named rows
node $A/codemap.mjs $T ~/uit/codes.json E0043 E0983       # emit sites per code
```

Rerunning on 125e2e8f89 gives this `triage.tsv` byte for byte, except that `ever_passed` counts the run logs present at the time.
