# G15 audit results: dev 0d1ea12f79 (2026-11-04)

Prep-2 ran these for Main on the G15 "finish Rust removal" pass over #4717, #3590, #6096 and #4599.
The compiler is the dev build of 0d1ea12f79 from the discovery base. The host has no rustc, cargo, cc, clang or ld on PATH.
Testkit tiers: o0 = `jet run`, o1 = `jet build`, o2 = `jet build --release`. The interpreter tier is retired (`--interpret` is jet.UnknownFlag).

## How to rerun

```sh
B=<dev jet build dir>; export JET=$B/jet JET_COMPILER_IDENTITY="$(cat $B/IDENTITY)" JET_TOOLCHAIN_ROOT=$PWD
Tools/agent/jet-env jet run Tools/testkit -- golden --jet $JET --tier o0 --jobs 5 --results golden-o0.json   # also o1, o2
Tools/agent/jet-env jet run Tools/testkit -- ui --jet $JET --tier o0,o2 --jobs 5 --results ui.json
Tools/agent/jet-env jet run Tools/testkit -- cases --jet $JET --tier o0,o1 --filter <case path part> --results cases.json
```

## Goldens

| tier | counts |
|---|---|
| o0 | FAIL 17, KNOWN-FAILING 6, PASS 1546, REFUSED 2, SKIP-HOST 10 |
| o1 | FAIL 17, KNOWN-FAILING 5, PASS 1545, REFUSED 2, SKIP-HOST 10, UNSUPPORTED 2 |
| o2 | FAIL 20, KNOWN-FAILING 5, PASS 1542, REFUSED 2, SKIP-HOST 10, UNSUPPORTED 2 |

### Grouping table: every golden that does not pass on all tiers (#4599 c2)

| test | o0 | o1 | o2 | first error | cause card |
|---|---|---|---|---|---|
| collections/data_battery/run.jet | FAIL | FAIL | FAIL | missing expected stream: Examples/features/expected/collections/data_battery | none: no expected output is checked in |
| concurrency/job_queue.jet | FAIL | FAIL | FAIL | missing expected stream: Examples/features/expected/concurrency/job_queue | none: no expected output is checked in |
| errors/stack_overflow.jet | PASS | PASS | FAIL | expected `Stop [E3012]: Stack overflow in `recurse``, got `Stop [E3012]: Stack overflow in `??` at 0x4010a4` | none (new) |
| foundations/arrays/run.jet | FAIL | FAIL | FAIL | missing expected stream: Examples/features/expected/foundations/arrays | none: no expected output is checked in |
| foundations/bridges/run.jet | FAIL | FAIL | FAIL | missing expected stream: Examples/features/expected/foundations/bridges | none: no expected output is checked in |
| foundations/capabilities/run.jet | FAIL | FAIL | FAIL | missing expected stream: Examples/features/expected/foundations/capabilities | none: no expected output is checked in |
| foundations/distributed/run.jet | FAIL | FAIL | FAIL | missing expected stream: Examples/features/expected/foundations/distributed | none: no expected output is checked in |
| foundations/dsl/run.jet | FAIL | FAIL | FAIL | missing expected stream: Examples/features/expected/foundations/dsl | none: no expected output is checked in |
| foundations/exact/run.jet | FAIL | FAIL | FAIL | missing expected stream: Examples/features/expected/foundations/exact | none: no expected output is checked in |
| foundations/realtime/run.jet | FAIL | FAIL | FAIL | missing expected stream: Examples/features/expected/foundations/realtime | none: no expected output is checked in |
| foundations/storage/run.jet | FAIL | FAIL | FAIL | missing expected stream: Examples/features/expected/foundations/storage | none: no expected output is checked in |
| foundations/text/run.jet | FAIL | FAIL | FAIL | missing expected stream: Examples/features/expected/foundations/text | none: no expected output is checked in |
| foundations/units/run.jet | FAIL | FAIL | FAIL | missing expected stream: Examples/features/expected/foundations/units | none: no expected output is checked in |
| io/path.jet | FAIL | FAIL | FAIL | expected `walk non-empty: true`, got `walk non-empty: false` | none: testkit runs a staged copy, so the relative walk of `Examples/features/io` finds nothing |
| lowlevel/ffi.jet | KNOWN-FAILING | KNOWN-FAILING | KNOWN-FAILING | #6366 ir.build unlowered: foreign import requires a checked direct C scalar signature (BuildForeign.jet:35); extern rust slot trampoline jet_ffi_<name>_cabi ... | #6366 |
| lowlevel/opaque_handles/run.jet | FAIL | FAIL | FAIL | missing expected stream: Examples/features/expected/lowlevel/opaque_handles | none: no expected output is checked in |
| lowlevel/polyglot_go/run.jet | KNOWN-FAILING | KNOWN-FAILING | KNOWN-FAILING | #6418 `jet inspect bind` reports E2104 not built yet; `use go.handles` reports E1002 (foreign root `go` reserved): exit 2; expected 0 | #6418 |
| memory/cache_line_padding.jet | PASS | PASS | FAIL | Error [jet.ReservedPackedLayout]: `align` requires one integer byte value (or `target, N`) | none (new) |
| memory/stream_store.jet | PASS | PASS | FAIL | Error [jet.UnknownCoreItem]: `core.mem` has no item `stream_store` | none (new) |
| net/backend_battery/run.jet | KNOWN-FAILING | KNOWN-FAILING | KNOWN-FAILING | #3173 ir.build unlowered: `jet run` is an internal compiler error (exit 101): the `HTTPRouterRegister` handle method and the `jet_http_router_dispatch`/`jet_... | #3173 |
| packages/mod_load_errors.jet | KNOWN-FAILING | UNSUPPORTED | UNSUPPORTED | #4599 `mod.load` is not built yet (E2104): exit 1; expected 0 | #4599 |
| serde/field_metadata_edges.jet | KNOWN-FAILING | KNOWN-FAILING | KNOWN-FAILING | expected `missing optional+default: {"name":"Ada","city":"Reno","zip":"89501","nickname":null,"visits":0}`, got `missing optional+default: {"name":"Ada","city":"Reno","zip":"89501","visits":0}` | #3236 |
| tooling/compute_autodiff.jet | KNOWN-FAILING | KNOWN-FAILING | KNOWN-FAILING | #4599 ir.build unlowered: `jet check` is clean, but `jet run` is an internal compiler error (exit 101) with 7 faults "generic Core call core.compute.gradient... | #4599 |
| ui/battery/run.jet | FAIL | FAIL | FAIL | missing expected stream: Examples/features/expected/ui/battery | none: no expected output is checked in |
| ui/surface.jet | FAIL | FAIL | FAIL | missing expected stream: Examples/features/expected/ui/surface | none: no expected output is checked in |
| web/browser_surface.jet | PASS | UNSUPPORTED | UNSUPPORTED | Error [jet.InvalidCommandInput]: `browser.report_json` is not built yet | #4954 |
| web/state_surface.jet | FAIL | FAIL | FAIL | missing expected stream: Examples/features/expected/web/state_surface | #4599 c9 |
| tests/run/ownership/runtime_disjoint_proof_reports_bounds_and_duplicates_before_mutation.jet | REFUSED | REFUSED | REFUSED | line 4: invalid stream assertion or missing partial reason | none: fixture header refused by testkit |
| tests/run/ownership/runtime_disjoint_proof_reports_bounds_and_duplicates_before_mutation_2.jet | REFUSED | REFUSED | REFUSED | line 4: invalid stream assertion or missing partial reason | none: fixture header refused by testkit |

## UI fixtures (tests/ui, tests/ui_lint)

Counts over all rows (fixture x tier): ERROR 1, FAIL 318, KNOWN-FAILING 114, PASS 1888, REFUSED 33.

Rows that neither pass nor carry a known-failing row:

| fixture | tier | status | first line |
|---|---|---|---|
| tests/ui/ambient_err_mixed.jet | - | FAIL | expected `Error [jet.ErrInOptionalFallback]: `err` is not available in an optional fallback`, got `Error [jet.FailureDomainNotErrorType]: Explicit failure domain `String (text)` is not an Error type` |
| tests/ui/ambiguous_call_e0772.jet | - | REFUSED | missing CLI fixture API: ambiguous_call |
| tests/ui/arena_bounded_open_multiplicity.jet | - | FAIL | expected `--> tests/ui/arena_bounded_open_multiplicity.jet:9:18`, got `--> tests/ui/arena_bounded_open_multiplicity.jet:4:24` |
| tests/ui/arena_bounded_transitive_overflow.jet | - | FAIL | expected `Error [jet.MemoryDenialViolated]: `Arena.new` reserves bounded arena storage at arena_bounded_transitive_ov...`, got `Error [jet.MemoryDenialViolated]: `!Mem.Alloc(above: 4)` cannot be proved through the open dispatch at aren...` |
| tests/ui/arg_label_forbidden.jet | - | FAIL | expected `Error [jet.LabelOnPositionalParameter]: `host` is a positional-only parameter of `connect``, got `Error [jet.UnknownArgumentLabel]: `connect` has no parameter labelled `host`` |
| tests/ui/arrow_batch_use_after_move.jet | - | FAIL | expected `Error [jet.UseAfterMove]: `batch` was consumed by `import`, so it can't be used here`, got `Error [jet.UninferableTypeArgument]: Can't figure out what `T` should be here` |
| tests/ui/auto_derive_opt_out_use.jet | - | FAIL | expected `--> tests/ui/auto_derive_opt_out_use.jet:12:13`, got `` |
| tests/ui/bad_map_key.jet | fixed-check | FAIL | tier-split: fixed companion must check clean |
| tests/ui/bare_member_callable_arity.jet | - | FAIL | expected `Error [jet.MemberShorthandArity]: A bare member chain cannot supply a 2-parameter callable here`, got `Error [jet.UnknownName]: Nothing named `` exists here` |
| tests/ui/bench_command_retired_e2101.jet | - | FAIL | expected `Error [jet.UnknownCommand]: `bench` isn't a jet command`, got `Error [jet.UnknownCommand]: `bench` isn't a jet command.` |
| tests/ui/browser_bidi/native_bidi_bad_method_arguments_stop_in_sema.jet | - | FAIL | expected `Error [jet.ValueTypeMismatch]: `subscribe` wants String (text) for argument 1, but this is Int (a whole num...`, got `` |
| tests/ui/browser_bidi/native_bidi_session_handles_cannot_cross_tasks_or_channels.jet | - | FAIL | expected `Error [jet.ValueNotSendable]: `session` can't cross into a task because `Browser` isn't sendable`, got `Error [jet.MoveWithoutMarker]: Implicit clone of `session`` |
| tests/ui/build_action_failed.jet | - | REFUSED | missing CLI fixture API: build_grants |
| tests/ui/build_action_ungranted.jet | - | REFUSED | missing CLI fixture API: programmable_build |
| tests/ui/build_enforce_reserved_code.jet | - | REFUSED | missing CLI fixture API: programmable_build |
| tests/ui/build_entry_bad_sig.jet | - | REFUSED | missing CLI fixture API: programmable_build |
| tests/ui/build_entry_conflict/run.jet | - | REFUSED | missing CLI fixture API: programmable_build |
| tests/ui/build_generated_collision/run.jet | - | REFUSED | missing CLI fixture API: programmable_build |
| tests/ui/build_generated_cycle/run.jet | - | REFUSED | missing CLI fixture API: programmable_build |
| tests/ui/build_locked_drift.jet | - | REFUSED | missing CLI fixture API: build_locked |
| tests/ui/build_plan_invalid.jet | - | REFUSED | missing CLI fixture API: programmable_build |
| tests/ui/call_non_function.jet | - | FAIL | expected `Error [jet.CalledNonFunction]: This is Int (a whole number), not a function`, got `Error [jet.UnknownMethod]: `call` isn't a method on this value` |
| tests/ui/call_struct_value.jet | - | FAIL | expected `Why: Parentheses call functions and lambdas only; `Scale` has the methods apply, as_fn`, got `Why: Parentheses call functions and lambdas only; `Scale` has the methods apply, as_fn, compare, encode, equal` |
| tests/ui/callable_policy_invalid.jet | - | FAIL | expected `--> tests/ui/callable_policy_invalid.jet:3:30`, got `--> tests/ui/callable_policy_invalid.jet:1:4` |
| tests/ui/cell_guard_capture.jet | - | FAIL | expected `\|            ^^^^^^^^^^^^^^^`, got `\|            ^^^^^^^^^^^^^^^^^` |
| tests/ui/cell_guard_storage.jet | - | FAIL | expected `--> tests/ui/cell_guard_storage.jet:6:11`, got `` |
| tests/ui/cffi/foreign_js_import_uses_generated_binding_cache_for_symbols/run.jet | - | FAIL | expected `(no errors)`, got `Error [jet.ReservedRootShadowed]: `js` is reserved for first-party or foreign packages` |
| tests/ui/cffi/foreign_js_inline_member_list_uses_the_same_binding_cache/run.jet | - | FAIL | expected `(no errors)`, got `Error [jet.ReservedRootShadowed]: `js` is reserved for first-party or foreign packages` |
| tests/ui/cffi/foreign_js_inline_pub_member_list_reexports_the_namespace/run.jet | - | FAIL | expected `(no errors)`, got `Error [jet.ReservedRootShadowed]: `js` is reserved for first-party or foreign packages` |
| tests/ui/cffi_e3204_two_use_forms.jet | - | FAIL | expected `--> tests/ui/cffi_e3204_two_use_forms.jet:3:5`, got `--> tests/ui/cffi_e3204_two_use_forms.jet:1:1` |
| tests/ui/cffi_e3205/run.jet | - | FAIL | expected `--> tests/ui/cffi_e3205/run.jet:4:8`, got `--> tests/ui/cffi_e3205/run.jet:1:1` |
| tests/ui/cffi_string_literal_interior_nul.jet | - | FAIL | expected `Error [E3211]: This string literal has an embedded NUL byte, so it can't cross into a C function.`, got `Error [jet.InvalidStatement]: Expected a name after `fn`, found the keyword `take`` |
| tests/ui/channel_retired_forms.jet | - | FAIL | expected `Error [jet.UnknownType]: There's no type called `Channel``, got `` |
| tests/ui/check_scope_owned/run.jet | - | FAIL | expected `(no errors)`, got `Error [jet.UseTargetNotFound]: The imported module is not present in the package source snapshot` |
| tests/ui/cli_authority_flags_reserved.jet | - | FAIL | expected `Error [jet.DuplicateCliFlag]: Two `#[CLI]` fields both derive the flag `--deny``, got `` |
| tests/ui/cli_e0043_install.jet | - | FAIL | expected `Error [jet.ForeignInstallCommand]: `jet install` isn't a Jet command`, got `Error [jet.InvalidCommandInput]: `jet install` is not built yet` |
| tests/ui/cli_short_name_collision.jet | - | FAIL | expected `Error [jet.InvalidShortName]: `#Short("p")` is used by both `port` and `profile``, got `` |
| tests/ui/clock_system_pure.jet | - | FAIL | expected `Error [jet.NondeterminismInPureEval]: `Clock.system` is non-deterministic and cannot appear in a pure evalu...`, got `Error [jet.ImpureCallInPureContext]: `now` calls the impure function `call`` |
| tests/ui/closures/parallel_adapters_reject_unsafe_boundaries_before_codegen_2.jet | - | FAIL | expected `\|                                      ^^^^^^^^^^^^^^^^^^^^^^^^^^^`, got `\|                                      ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^` |
| tests/ui/closures/parallel_adapters_reject_unsafe_boundaries_before_codegen_5.jet | - | FAIL | expected `Error [jet.ValueNotSendable]: `parallel result` can't cross a parallel worker boundary because `[fn(Int) -[...`, got `Error [jet.ValueNotSendable]: `parallel result` can't cross a parallel worker boundary because `[fn(Int) -[...` |
| tests/ui/closures/parallel_adapters_reject_unsafe_boundaries_before_codegen_6.jet | - | FAIL | expected `Error [jet.BindingTypeMismatch]: Argument 2 to `.para_fold()` should be fn(fn(Int) -[]> Int, Int) -> fn(Int...`, got `Error [jet.ReturnTypeMismatch]: This lambda parameter should be fn(Int) -[]> Int Never!, not fn(Int) -> Int` |
| tests/ui/closures/parallel_adapters_reject_unsafe_boundaries_before_codegen_8.jet | - | FAIL | expected `Error [jet.ValueNotSendable]: `stored` can't cross a parallel worker boundary because `fn(Int) -[]> Int` is...`, got `Error [jet.ValueNotSendable]: `stored` can't cross a parallel worker boundary because `fn(Int) -[]> Int Nev...` |
| tests/ui/command_override_conflict/run.jet | - | REFUSED | missing CLI fixture API: command_override_conflict |
| tests/ui/compiler_api/compiler_api_rejects_zero_and_extra_arguments_at_checking_6.jet | - | FAIL | expected `Error [jet.ValueTypeMismatch]: `check` wants `CompilerSyntaxTree` here, but this is String (text)`, got `Error [jet.BindingTypeMismatch]: This needs CompilerSyntaxTree, but the value is String` |
| tests/ui/comptime_fuel.jet | - | FAIL | expected `Error [jet.ComptimeFuelExhausted]: Compile-time evaluation ran out of steps`, got `Error [jet.ComptimeFuelExhausted]: Compile-time evaluation exhausted its fuel` |
| tests/ui/comptime_impure.jet | - | FAIL | expected `Error [jet.ImpureCallInPureContext]: `print` is not allowed in comptime code`, got `Error [jet.ComptimeEffectWithoutImpure]: `standard output` is a Tier-2 comptime effect — it requires a `#Im...` |
| tests/ui/comptime_net_fetch_bad_hash.jet | - | FAIL | expected `Error [jet.FetchHashMismatch]: Fetch: sha256 mismatch for `file:///dev/null``, got `Error [jet.UnknownCoreItem]: `core.net` has no item `fetch`` |
| tests/ui/comptime_net_fetch_bad_url.jet | - | FAIL | expected `Error [jet.FetchFailed]: Fetch failed: unsupported URL scheme `ftp`; expected `file://`, `http://`, or `htt...`, got `Error [jet.UnknownCoreItem]: `core.net` has no item `fetch`` |
| tests/ui/comptime_net_fetch_ok.jet | - | FAIL | expected `(no errors)`, got `Error [jet.UnknownCoreItem]: `core.net` has no item `fetch`` |
| tests/ui/comptime_nondeterministic.jet | - | FAIL | expected `--> tests/ui/comptime_nondeterministic.jet:6:21`, got `--> tests/ui/comptime_nondeterministic.jet:8:31` |
| tests/ui/compute_set_immutable.jet | - | FAIL | expected `Error [jet.ImmutableAssignment]: `tensor` was made with `::`, so it can't be changed`, got `Error [jet.FieldWriteWithoutMarker]: Cannot edit through `tensor` — it has read access only` |
| tests/ui/computed_enum_noninteger.jet | - | FAIL | expected `Error [jet.InvalidEnumDiscriminant]: An enum discriminant must be an integer, got Decimal (an exact base-10...`, got `Error [jet.BindingTypeMismatch]: This needs Int, but the value is Decimal` |
| tests/ui/computed_enum_unknown.jet | - | FAIL | expected `Error [jet.InvalidEnumDiscriminant]: An enum discriminant must be computable at compile time`, got `Error [jet.UnknownName]: Nothing named `missing` exists here` |
| tests/ui/contract_result_misuse.jet | - | FAIL | snapshot mismatch: tests/ui/contract_result_misuse.stderr |
| tests/ui/coordinate_point_mismatch.jet | - | FAIL | expected `Error [jet.CoordinateSpaceMismatch]: `+` needs `Screen` space, got `World`.`, got `Error [jet.UnknownName]: Nothing named `core` exists here` |
| tests/ui/coordinate_transform_mismatch.jet | - | FAIL | expected `Error [jet.TransformCompositionMismatch]: Cannot compose `Transform<Screen, World>` with `Transform<View, C...`, got `Error [jet.UninferableTypeArgument]: Can't figure out what `Next` should be here` |
| tests/ui/copies_explicit_view_field.jet | - | FAIL | expected `Error [jet.StringViewOutlivesOwner]: `user` cannot be used directly here without an owning copy`, got `` |
| tests/ui/core_failure_record_literal_user.jet | - | FAIL | expected `Error [jet.UnknownType]: There's no type called `TextError``, got `Error [jet.UnknownName]: Nothing named `TextError` exists here` |
| tests/ui/core_math_float_type_mismatch.jet | - | FAIL | expected `Error [jet.ValueTypeMismatch]: `sqrt` needs Float, F32, or Measurement<Float>, not Int (a whole number)`, got `Error [jet.TraitNotImplemented]: `Int` does not satisfy the canonical Core constraint for type parameter `T`` |
| tests/ui/core_math_float_type_mismatch_pow.jet | - | FAIL | expected `Error [jet.OperatorTypeMismatch]: `pow` needs both float operands to have the same width`, got `Error [jet.BindingTypeMismatch]: Argument 1 to `.pow()` should be Float (an approximate binary number), not...` |
| tests/ui/core_math_float_type_mismatch_sign.jet | - | FAIL | expected `Error [jet.ValueTypeMismatch]: `sign` wants Int (a whole number) here, but this is String (text)`, got `Error [jet.ValueTypeMismatch]: `sign` wants Int (a whole number) for argument 1, but this is String (text)` |
| tests/ui/core_submodule_item_path.jet | - | FAIL | expected `Fix: `core.crypto.uuid` is a submodule, so name an item on it: `core.crypto.uuid.v4``, got `Fix: `core.crypto.uuid` is a submodule, so name an item on it: `core.crypto.uuid.UUIDError`` |
| tests/ui/corelib_parts_runtime/solve_constructor_is_static_only.jet | - | FAIL | expected `Error [jet.UnknownMethod]: `new` isn't a method on this value`, got `Error [jet.ArgumentCountMismatch]: `new` expects 0 arguments, got 1` |
| tests/ui/crypto_expert_nonce_callback_bound_precedence.jet | - | FAIL | expected `Error [jet.ValueTypeMismatch]: `transform` wants fn(Int) Int -[]> for argument 2, but this is fn(Int) Int`, got `Error [jet.ValueTypeMismatch]: `transform` wants fn(Int) -[]> Int for argument 2, but this is fn(Int) -> Int` |
| tests/ui/crypto_release_entropy_seam.jet | - | FAIL | expected `Error [jet.CryptoMisuse]: Crypto API misuse`, got `Error [jet.ValueTypeMismatch]: `password_hash_with_salt` wants `Secret` for argument 1, but this is String ...` |
| tests/ui/crypto_safe_raw_algorithm.jet | - | FAIL | expected `Error [jet.MissingErrorConversion]: This implicit fallible call can't convert `CryptoError` into `Err` — no...`, got `Error [jet.UnknownArgumentLabel]: `core.crypto.seal` has no parameter labelled `algorithm`` |
| tests/ui/crypto_safe_raw_nonce.jet | - | FAIL | expected `Error [jet.MissingErrorConversion]: This implicit fallible call can't convert `CryptoError` into `Err` — no...`, got `Error [jet.UnknownArgumentLabel]: `core.crypto.seal` has no parameter labelled `nonce`` |
| tests/ui/crypto_secret_local_operations.jet | - | FAIL | expected `Error [jet.TraitNotImplemented]: `Secret` does not implement `Encode``, got `` |
| tests/ui/crypto_secret_name_collisions.jet | - | FAIL | expected `Warning [jet.DisplayMigration]: `Secret` has no `Display` impl — bare `{}` will require one soon`, got `` |
| tests/ui/crypto_secret_nested_provenance.jet | - | FAIL | expected `Error [jet.UnknownCoreItem]: `core.crypto` exposes `Secret`, but not in this position`, got `Error [jet.ModuleAsValue]: `crypto` is a module, not a value` |
| tests/ui/db_typed_sql_all_sinks_plain_string.jet | - | FAIL | expected `--> tests/ui/db_typed_sql_all_sinks_plain_string.jet:8:26`, got `` |
| tests/ui/default_forward_ref.jet | - | FAIL | expected `Error [jet.ArgumentCountMismatch]: `f` expects 2 arguments, got 1`, got `Error [jet.UnknownName]: Nothing named `b` exists here` |
| tests/ui/defer_only_close.jet | - | FAIL | expected `\|     ^^^^^^^^^^^`, got `\|     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^` |
| tests/ui/delegation_no_impl.jet | - | FAIL | expected `Fix: Implement `impl Inner: Greeter` on the field's type, or choose a different field`, got `Fix: Implement `impl Inner.Greeter` on the field's type, or choose a different field` |
| tests/ui/derive_tag.jet | - | FAIL | expected `Error [jet.TagUsedAsTrait]: `Reviewed` is a tag, but `derive` needs a trait`, got `Error [jet.UnknownAppliedRule]: `#Reviewed` isn't a known applied rule` |
| tests/ui/determinism/assume_deterministic_suppresses_e3401.jet | - | FAIL | expected `(no errors)`, got `Error [jet.ImpureCallInPureContext]: `risky` calls the impure function `print`` |
| tests/ui/determinism/pure_code_rejects_clock_provenance_laundered_through_a_struct_6.jet | - | FAIL | expected `Error [jet.NondeterminismInPureEval]: `Clock formatting` is non-deterministic and cannot appear in a pure e...`, got `Error [jet.ValueTypeMismatch]: `Clock` can't be shown with :Debug yet` |
| tests/ui/determinism/rng_pick_returns_element_option.jet | - | FAIL | expected `(no errors)`, got `Error [jet.MissingReceiverMark]: This call writes `rng`, but `rng` has no `&` mark` |
| tests/ui/discarded_lost_tail.jet | - | FAIL | expected `--> tests/ui/discarded_lost_tail.jet:14:14`, got `--> tests/ui/discarded_lost_tail.jet:14:9` |
| tests/ui/distinct_coerce_base.jet | - | FAIL | expected `(no errors)`, got `Error [E0128]: A `Int` can't be used where a `UserID` is expected` |
| tests/ui/distinct_constructor_alias.jet | - | FAIL | expected `Error [E0128]: `UserID(value)` is not a distinct conversion`, got `Error [jet.CalledNonFunction]: `UserID` is UserID, not a function` |
| tests/ui/distinct_numeric_ok.jet | - | FAIL | expected `(no errors)`, got `Error [E0128]: A `Meters` can't be used where a `Meters` is expected` |
| tests/ui/dyn_from_missing_encode.jet | - | FAIL | expected `--> tests/ui/dyn_from_missing_encode.jet:8:19`, got `--> tests/ui/dyn_from_missing_encode.jet:8:14` |
| tests/ui/e1803_script_inline_authority.jet | - | FAIL | expected `Error [jet.ApplicationAuthorityDenied]: Application authority is undecided for `FS``, got `Error [jet.ApplicationAuthorityDenied]: Application authority is undecided for `FS.Write`` |
| tests/ui/e1803_script_spawn_needs_exec.jet | - | FAIL | missing expected stream: tests/ui/e1803_script_spawn_needs_exec.stderr |
| tests/ui/e2421_public_failure_carrier.jet | - | FAIL | expected `Error [jet.PrivateFailureMemberExposed]: Public fallible function `load` exposes non-public failure carrier...`, got `Error [jet.UnknownName]: Nothing named `internal` exists here` |
| tests/ui/effect_callback_bound.jet | - | FAIL | expected `Error [jet.ValueTypeMismatch]: `transform` wants fn(Int) Int -[]> for argument 2, but this is fn(Int) Int`, got `Error [jet.ValueTypeMismatch]: `transform` wants fn(Int) -[]> Int for argument 2, but this is fn(Int) -> Int` |
| tests/ui/effect_math_random_in_pure_fn.jet | - | FAIL | expected `Error [jet.ImpureCallInPureContext]: `draw` calls the impure function `core_core_math_random::__word``, got `Error [jet.NondeterminismInPureEval]: `math.random` is non-deterministic and cannot appear in a pure evalua...` |
| tests/ui/embedded_generic_method_scope.jet | - | FAIL | expected ``, got `Error [jet.UninferableTypeArgument]: No trait named `Clone` is in scope for the bound on `T`` |
| tests/ui/empty_map_colon_retired.jet | - | FAIL | expected `Why: A value can be a name, a number, quoted text, `true`/`false`, or a call`, got `Why: A value can be a name, literal, collection, call, or parenthesized expression` |
| tests/ui/enum_named_payload_needs_dot_brace.jet | - | FAIL | expected `Error [jet.InvalidConstructionField]: Variant `Box` requires labeled fields`, got `Error [jet.UnknownArgumentLabel]: Variant `Box` needs named payload fields` |
| tests/ui/enum_pattern_needs_dot.jet | - | FAIL | expected `Error [jet.BareCasePattern]: Pattern `Red` needs a leading `.``, got `Error [jet.UnknownName]: Nothing named `Red` exists here` |
| tests/ui/enum_pattern_needs_dot_payload.jet | - | FAIL | expected `Error [jet.BareCasePattern]: Pattern `Fade` needs a leading `.``, got `` |
| tests/ui/env_facet_missing/run.jet | - | REFUSED | missing CLI fixture API: env_facet_missing |
| tests/ui/fact_contribution_conflict/run.jet | - | REFUSED | missing CLI fixture API: programmable_build |
| tests/ui/fact_contribution_undeclared/run.jet | - | REFUSED | missing CLI fixture API: programmable_build |
| tests/ui/fallible_arm_table_tail.jet | - | FAIL | expected `--> tests/ui/fallible_arm_table_tail.jet:1:12`, got `` |
| tests/ui/ffi_bad_path.jet | - | FAIL | expected `cargo said:\n       Compiling jet_ffi_ef188a0424bcbabad445d97ca4272af90b6bd81c976cdd875944f0094b2664e0 v0.1...`, got `cargo said:` |
| tests/ui/ffi_bad_type.jet | - | FAIL | expected `Error [jet.FfiBoundaryTypeNotAllowed]: An FFI boundary rejects call `bad` uses capability `&` outside `#Uns...`, got `Error [jet.ForeignSignatureMismatch]: The Rust item doesn't match the Jet signature at `bad` in `extern rus...` |
| tests/ui/ffi_fetch_failed.jet | - | FAIL | expected `cargo said:\n    error: no matching package named `not-a-real-crate-xyz` found\n    location searched: crat...`, got `cargo said:` |
| tests/ui/ffi_missing_version.jet | - | FAIL | expected `Fix: Write: extern rust "base64@0.1" { ... }`, got `Fix: Write: extern rust `base64@0.1` { ... }` |
| tests/ui/fixed_backing_crossing.jet | - | FAIL | expected `Error [jet.ValueNotSendable]: `task_fixed` cannot be shared into a task — it is a view that does not live l...`, got `Error [jet.UseAfterMove]: `task_fixed` was consumed by the binding `_`, so it can't be used here` |
| tests/ui/foreign_declaration_binding.jet | - | FAIL | expected `Error [jet.InvalidStatement]: `fun` is a function declaration keyword; Jet writes `fn``, got `` |
| tests/ui/foreign_method_names_list.jet | - | FAIL | expected `Error [jet.UnknownMethod]: `append` isn't a method on this value`, got `Error [jet.MissingWriteMarker]: Cannot write to `nums` — it does not have the write-access marker `&`; requ...` |
| tests/ui/freestanding_e3301.jet | - | REFUSED | missing CLI fixture API: no-os |
| tests/ui/freestanding_e3303.jet | - | REFUSED | missing CLI fixture API: no-os |
| tests/ui/function_contract_laundering.jet | - | FAIL | expected `--> tests/ui/function_contract_laundering.jet:3:56`, got `` |
| tests/ui/generated_cffi_e3206.jet | - | REFUSED | missing CLI fixture API: generated_source |
| tests/ui/generated_cffi_e3207.jet | - | REFUSED | missing CLI fixture API: generated_source |
| tests/ui/generic_constructor_ambiguous.jet | - | FAIL | expected `Error [jet.UninferableTypeArgument]: `Empty.new` needs its generic receiver type here`, got `Error [jet.UninferableTypeArgument]: Cannot infer the owner type arguments for `Empty.new`` |
| tests/ui/generic_constructor_bound.jet | - | FAIL | expected `Fix: Write `impl Token: Named { … }` with every required method`, got `Fix: Write `impl Token.Named { … }` with every required method` |
| tests/ui/generic_lambda_conflict.jet | - | FAIL | expected `Error [jet.UninferableTypeArgument]: Can't figure out what `T` should be here`, got `Error [jet.ValueTypeMismatch]: `apply` wants String (text) for argument 2, but this is Int (a whole number)` |
| tests/ui/genmod_value_not_comptime.jet | - | FAIL | expected `\|                          ^^^^^^^^^^^^^`, got `\|                          ^^^^^^^^^^^^^^^` |
| tests/ui/handle_unknown_field.jet | - | FAIL | expected `Error [jet.UnknownField]: `.unix_ms` only works on struct and tuple values`, got `Error [jet.UnknownField]: `Clock` has no field `unix_ms`` |
| tests/ui/hangar_verify_digest_mismatch.jet | - | REFUSED | missing CLI fixture API: jetpack_hangar_digest_mismatch |
| tests/ui/http_message_literal_migration.jet | - | FAIL | expected `Error [jet.DeprecatedPastWindow]: `HTTPResponse.{ ... }` was replaced by the shared HTTP response constructor`, got `Error [jet.BindingTypeMismatch]: This needs Int, but the value is String` |
| tests/ui/identifier_casing.jet | - | FAIL | expected `Error [jet.CasingMismatch]: Struct `bad_type` must be PascalCase`, got `` |
| tests/ui/implicit_take_keeps_mark.jet | - | FAIL | expected `Fix: Write the move marker `^` (`^x`) to move it — this is `x`'s last use — or `x.copy()` to keep a copy`, got `Fix: Write the move marker `^` (`^x`) if this is the last use, or copy first with `x.copy()`` |
| tests/ui/import_cycle/run.jet | - | FAIL | expected `Error [jet.UseTargetNotFound]: Can't find the file `main``, got `Error [jet.MissingFunctionBody]: `fn a` has no body` |
| tests/ui/import_missing/run.jet | - | FAIL | expected `Error [jet.UseTargetNotFound]: Can't find the file `no_such_module``, got `Error [jet.UseTargetNotFound]: The imported file is not in the authorized package source snapshot` |
| tests/ui/impure_net_stub.jet | - | REFUSED | missing CLI fixture API: gate |
| tests/ui/impure_no_flag.jet | - | FAIL | expected `Error [jet.ImpureGateNotAllowed]: `core.files.read()` inside `#Impure` gate, but `--gate impure=allow` was ...`, got `Error [jet.ComptimeEffectWithoutImpure]: `opening a file` is a Tier-2 comptime effect — it requires a `#Imp...` |
| tests/ui/inline_module_return_type_mismatch.jet | - | FAIL | expected `Error [jet.ArrowFunctionBody]: A braced callable with a non-unit success result needs `->` before its body.`, got `Error [jet.ReturnTypeMismatch]: This value is `M4Vis24BankAccount`, but `bad` returns M4BankAccount` |
| tests/ui/inline_package_cycle/run.jet | - | FAIL | expected `Error [jet.UseCycle]: These files import each other in a circle`, got `Error [jet.RelativeImportInPackage]: `use "a"` imports a file, but this file belongs to a package` |
| tests/ui/inline_package_stale_lock/run.jet | - | FAIL | expected `Error [jet.StaleLockFile]: The lock file is out of date`, got `Error [jet.UseTargetNotFound]: A path dependency has no matching authorized package root` |
| tests/ui/iter_missing_method_hint.jet | - | FAIL | expected `--> tests/ui/iter_missing_method_hint.jet:8:16`, got `` |
| tests/ui/jetlib_effect_refused.jet | - | REFUSED | missing CLI fixture API: jetlib_effect_refused |
| tests/ui/jetlib_version_mismatch.jet | - | REFUSED | missing CLI fixture API: jetlib_version_mismatch |
| tests/ui/job_queue_checked_identity.jet | - | FAIL | expected `Error [jet.ValueTypeMismatch]: `JobQueue.enqueue` expects a checked `#Job` function`, got `Error [jet.FieldWriteWithoutMarker]: Cannot edit through `queue` — it has read access only` |
| tests/ui/lambda_escape_no_take.jet | - | FAIL | expected `Error [jet.UseAfterMove]: `item` was consumed by an escaping lambda, so it can't be used here`, got `(no errors)` |
| tests/ui/lambda_map_mutate_list.jet | - | FAIL | expected `Error [jet.CollectionChangedWhileRead]: While the loop is reading `nums`, nothing may change it`, got `Error [jet.UseWhileWriteActive]: `nums` is already shared in this call, so it can't be changed here too` |
| tests/ui/lambda_mut_borrow_conflict.jet | - | FAIL | expected `Error [jet.UseWhileWriteActive]: `total` is being changed in this call, so it can't be used again here`, got `(no errors)` |
| tests/ui/lambda_self_rec.jet | - | FAIL | expected `\|                 ^^^^^^^^^^^^^`, got `\|                 ^^^^^^^^^^^^^^^^` |
| tests/ui/language_features/nested_callable_containers_keep_required_public_labels.jet | - | FAIL | expected `Error [jet.BindingTypeMismatch]: This needs [String:fn(*, force: Bool) -> Int], but the value is [String:fn...`, got `Error [jet.FunctionValueLabelMismatch]: This needs fn(*, force: Bool) -> Int, but the function value is fn(...` |
| tests/ui/language_features/range_type_literal_proofs_cover_every_numeric_source_family.jet | - | FAIL | expected `Error [jet.LiteralOutsideRange]: `11` is outside `Severity`'s range 0..10`, got `` |
| tests/ui/layout_axis_mismatch.jet | - | FAIL | expected `--> tests/ui/layout_axis_mismatch.jet:6:29`, got `--> tests/ui/layout_axis_mismatch.jet:6:23` |
| tests/ui/layout_columnar_partial.jet | - | FAIL | snapshot mismatch: tests/ui/layout_columnar_partial.stderr |
| tests/ui/layout_columnar_unsupported_method.jet | - | FAIL | expected `Error [jet.ColumnarMethodNotSupported]: Writing through `[ ]` isn't supported on a columnar list `[Particle...`, got `Error [jet.MissingWriteMarker]: Cannot write to `ps` — it does not have the write-access marker `&`` |
| tests/ui/library_invalid_binding/run.jet | - | REFUSED | missing CLI fixture API: library_invalid_binding |
| tests/ui/lint_cognitive_complexity.jet | - | FAIL | expected `tests/ui/lint_cognitive_complexity.jet:2 fn tangled — score 4 (budget 3)`, got `tests/ui/lint_cognitive_complexity.jet:2 fn tangled — score 7 (budget 3)` |
| tests/ui/lint_cognitive_complexity_refactored.jet | - | FAIL | expected `tests/ui/lint_cognitive_complexity_refactored.jet:10 fn tangled — score 2 (budget 3)`, got `tests/ui/lint_cognitive_complexity_refactored.jet:10 fn tangled — score 3 (budget 3)` |
| tests/ui/lint_policy_code_name.jet | - | REFUSED | missing CLI fixture API: lint_policy_config |
| tests/ui/live_freestanding_e3301.jet | - | REFUSED | missing CLI fixture API: no-os |
| tests/ui/local_cell_tiers/local_cell_guard_storage_boundary_reports_once_per_site_4.jet | - | FAIL | expected `\|            ^^^^^^^^^^^^^^^`, got `\|            ^^^^^^^^^^^^^^^^^` |
| tests/ui/loop_bindingless_named_bindings.jet | - | FAIL | expected `Error [jet.LoopMissingBinding]: This loop needs a named source binding.`, got `Error [jet.OperatorTypeMismatch]: Values of type Int can't be used as a loop source` |
| tests/ui/lowlevel_e3101.jet | - | FAIL | expected `4 \|     p :: *Int{*value}`, got `4 \|     p :: *value` |
| tests/ui/lowlevel_volatile_write_without_unsafe.jet | - | FAIL | expected `Error [jet.MemoryOperationOutsideUnsafe]: `volatile_write` can only run inside an `#Unsafe` block`, got `` |
| tests/ui/lowlevel_volatile_write_wrong_ptr.jet | - | FAIL | expected `Error [jet.ValueTypeMismatch]: `volatile_write` needs a `Ptr<T>`, not Int (a whole number)`, got `Error [jet.ValueTypeMismatch]: `volatile_write` wants `*Int` for argument 1, but this is Int (a whole number)` |
| tests/ui/lowlevel_volatile_write_wrong_value.jet | - | FAIL | expected `(no errors)`, got `Error [jet.ValueTypeMismatch]: `volatile_write` wants Int (a whole number) for argument 2, but this is Stri...` |
| tests/ui/manifest_bad_allocator/run.jet | - | FAIL | expected `Why: `allocator:` must be `mem.Heap` or `mem.Counting.over(mem.Heap, cap: <bytes>)``, got `--> package.jet:3:1` |
| tests/ui/manifest_bad_layer/run.jet | - | FAIL | expected `Error [jet.LintSelectorIsCode]: `package.jet` has a shape error`, got `Error [jet.InvalidStatement]: Package runtime ceiling is unsupported` |
| tests/ui/manifest_lock_stale/run.jet | - | FAIL | expected `Error [jet.StaleLockFile]: The lock file is out of date`, got `Error [jet.UseTargetNotFound]: A path dependency has no matching authorized package root` |
| tests/ui/manifest_unknown_field/run.jet | - | FAIL | expected `Error [jet.LintSelectorIsCode]: Unknown field `payload:` in `package.jet``, got `Error [jet.InvalidStatement]: Package manifest has no name` |
| tests/ui/marker_wrong_at_plane.jet | - | FAIL | expected `Error [jet.BlockAfterCall]: Trailing blocks are gone — pass code with `() ->``, got `Error [jet.UnknownFunction]: Nothing named `@Unsafe` exists here` |
| tests/ui/matrix_measure_inner_mismatch.jet | - | FAIL | expected `Error [jet.MatrixShapeMismatch]: Matrix multiplication needs equal inner sides, but these are 4 and 5`, got `Error [jet.InvalidStatement]: Expected a type, found `3`` |
| tests/ui/memo_lazy_iter_arg_e0940.jet | - | FAIL | expected `Error [jet.UnknownType]: There's no type called `Iter``, got `Error [jet.MemoLazyIter]: `#Memo` cannot cache lazy `Iter` values` |
| tests/ui/memo_lazy_iter_e0940.jet | - | FAIL | expected `Error [jet.UnknownType]: There's no type called `Iter``, got `Error [jet.MemoLazyIter]: `#Memo` cannot cache a lazy `Iter` result` |
| tests/ui/meta_on_expression.jet | - | FAIL | expected `Error [jet.InvalidScopedPolicy]: `#Meta` cannot attach at the Expression site`, got `Error [jet.MetaNotAllowedHere]: `#Meta` attaches to a binding or function` |
| tests/ui/migration_change_impl_ok.jet | - | FAIL | expected `(no errors)`, got `Error [jet.BreakingSchemaChange]: The `change note: String -> Label` migration on `Order` has no converter` |
| tests/ui/missing_assoc_type.jet | - | FAIL | expected `Error [jet.BuiltinRedefined]: The name `Tally` is built in and can't be redefined`, got `` |
| tests/ui/move_from_borrowed_argument.jet | - | FAIL | expected `--> tests/ui/move_from_borrowed_argument.jet:25:14`, got `` |
| tests/ui/nested_option.jet | - | FAIL | expected `Error [jet.ArrowFunctionBody]: This named function body follows an arrow instead of braces.`, got `Error [jet.NestedOptional]: `??` isn't allowed on a type` |
| tests/ui/never_return_fallthrough.jet | - | FAIL | expected `Error [jet.NeverFunctionReachesEnd]: Function `almost` promises `Never` but can reach the end.`, got `Error [jet.MissingBlockValue]: When `flag` is false, `almost` reaches the end without a value` |
| tests/ui/never_return_field.jet | - | FAIL | expected `Error [jet.NeverAsValue]: `Never` cannot be used in a value position.`, got `Error [jet.OperatorTypeMismatch]: A stored field cannot contain Never` |
| tests/ui/never_return_module_exit.jet | - | FAIL | expected `Error [jet.NeverFunctionReachesEnd]: Function `almost` promises `Never` but can reach the end.`, got `Error [jet.ReturnTypeMismatch]: This value is `Unit`, but `almost` returns Never` |
| tests/ui/never_return_stored.jet | - | FAIL | expected `Error [jet.NeverAsValue]: `Never` cannot be used in a value position.`, got `Error [jet.UnknownName]: Nothing named `Never` exists here` |
| tests/ui/none_sibling_arm_untyped.jet | - | FAIL | expected `--> tests/ui/none_sibling_arm_untyped.jet:4:24`, got `` |
| tests/ui/off_debug_attr_on_expression.jet | - | FAIL | expected `Error [jet.InvalidScopedPolicy]: `#Off` cannot attach at the Expression site`, got `Error [jet.SwitchOffAsExpression]: `#Off` does not produce a value` |
| tests/ui/operator_mixed_no_mirror.jet | - | FAIL | expected `Error [jet.OperatorTypeMismatch]: `-` can't compare or combine Int (a whole number) and `Debt``, got `Error [jet.MissingOperatorHook]: No `-` operator is defined for `Int`` |
| tests/ui/operator_mixed_nonnumeric_left.jet | - | FAIL | snapshot mismatch: tests/ui/operator_mixed_nonnumeric_left.stderr |
| tests/ui/operator_mixed_orphan_reverse/run.jet | - | FAIL | expected `Error [jet.OrphanImpl]: This `impl` can't live here`, got `Error [jet.ImplOnBuiltinType]: `impl Int.Sub` needs a user-defined type` |
| tests/ui/ownership/aggregate_view_slot_composes_through_parameter_projection.jet | - | FAIL | expected `Error [jet.OwnerMovedWhileViewed]: `left` cannot be changed by `.push()` while `selected` is still looking ...`, got `Error [jet.ViewOutlivesOwner]: Returned views need a stable owner relationship` |
| tests/ui/ownership/borrowed_mutable_view_enum_requires_take_before_destructuring.jet | - | FAIL | missing expected stream: tests/ui/ownership/borrowed_mutable_view_enum_requires_take_before_destructuring.stderr |
| tests/ui/ownership/borrowed_mutable_view_enum_requires_take_before_destructuring_2.jet | - | FAIL | missing expected stream: tests/ui/ownership/borrowed_mutable_view_enum_requires_take_before_destructuring_2.stderr |
| tests/ui/ownership/builtin_and_composite_receivers_use_the_call_access_frame.jet | - | FAIL | expected `Error [jet.UseWhileWriteActive]: `values` is being changed in this call, so it can't be used again here`, got `Error [jet.OptionalWhereValueExpected]: This needs Int (a whole number), but the value is Int?` |
| tests/ui/ownership/builtin_mut_receiver_uses_two_phase_reservation_2.jet | - | FAIL | expected `Error [jet.UseWhileWriteActive]: `values` is being changed in this call, so it can't be used again here`, got `Error [jet.OptionalWhereValueExpected]: This needs Int (a whole number), but the value is Int?` |
| tests/ui/ownership/composite_lambda_capture_walks_if_prefix_and_fallback_values.jet | - | FAIL | expected `Error [jet.UseWhileWriteActive]: `values` is being changed in this call, so it can't be used again here`, got `(no errors)` |
| tests/ui/ownership/edit_disjoint_callback_views_reject_every_retaining_boundary_3.jet | - | FAIL | missing expected stream: tests/ui/ownership/edit_disjoint_callback_views_reject_every_retaining_boundary_3.stderr |
| tests/ui/ownership/edit_disjoint_callback_views_reject_every_retaining_boundary_4.jet | - | FAIL | missing expected stream: tests/ui/ownership/edit_disjoint_callback_views_reject_every_retaining_boundary_4.stderr |
| tests/ui/ownership/enum_pattern_payload_keeps_its_original_owner_live.jet | - | FAIL | missing expected stream: tests/ui/ownership/enum_pattern_payload_keeps_its_original_owner_live.stderr |
| tests/ui/ownership/evaluated_statement_accesses_are_scoped_and_mode_aware_2.jet | - | FAIL | expected `(no errors)`, got `Error [jet.NameAlreadyTaken]: The name `value` is already taken here` |
| tests/ui/ownership/function_value_call_rejects_move_and_tracks_aliases_per_call_2.jet | - | FAIL | expected `Error [jet.UseWhileWriteActive]: `value` is being changed in this call, so it can't be used again here`, got `Error [jet.UnusedPlaceMark]: `callback` only reads this argument, so it takes no `&` mark` |
| tests/ui/ownership/indexed_simulation_rejects_dynamic_disjointness_claim.jet | - | FAIL | snapshot mismatch: tests/ui/ownership/indexed_simulation_rejects_dynamic_disjointness_claim.stderr |
| tests/ui/ownership/indexed_simulation_rejects_hostile_overlap.jet | - | FAIL | snapshot mismatch: tests/ui/ownership/indexed_simulation_rejects_hostile_overlap.stderr |
| tests/ui/ownership/lending_disjoint_views_reject_every_retaining_boundary_4.jet | - | FAIL | expected `Error [jet.ViewOutlivesOwner]: Returned views need a stable owner relationship`, got `Error [jet.ValueThrownAway]: This ViewMut<Int> value is thrown away` |
| tests/ui/ownership/local_cell_guard_mapping_and_splitting_keep_projected_types.jet | - | FAIL | expected `(no errors)`, got `Error [jet.UnusedPlaceMark]: This call only reads its receiver, so it takes no `&` mark` |
| tests/ui/ownership/local_cell_surface_uses_read_receivers_and_host_borrows.jet | - | FAIL | expected `(no errors)`, got `Error [jet.UnusedPlaceMark]: This call only reads its receiver, so it takes no `&` mark` |
| tests/ui/ownership/local_owned_view_return_reports_e2305_once.jet | - | FAIL | expected `Error [jet.ValueThrownAway]: The result of `make` is thrown away`, got `` |
| tests/ui/ownership/move_lambda_capture_identity_matches_rust_2021_places_5.jet | - | FAIL | expected `Error [jet.UseWhileWriteActive]: `values[…]` is being changed in this call, so it can't be used again here`, got `Error [jet.OwnerMovedWhileViewed]: `values` cannot be passed with write access while `first` is still looki...` |
| tests/ui/ownership/move_lambda_construction_consumes_owned_nonscalar_captures.jet | - | FAIL | expected `Error [jet.ValueTypeMismatch]: `both` wants fn() for argument 1, but this is fn() -[]> Int`, got `Error [jet.ValueTypeMismatch]: `both` wants fn() for argument 1, but this is fn() -[]> Int Never!` |
| tests/ui/ownership/move_lambda_construction_consumes_owned_nonscalar_captures_2.jet | - | FAIL | expected `Error [jet.UseWhileWriteActive]: `values` is already shared in this call, so it can't be changed here too`, got `Error [jet.ValueTypeMismatch]: `both` wants fn() for argument 2, but this is fn() -[]> Int Never!` |
| tests/ui/ownership/multi_source_returned_aggregate_still_blocks_owner_invalidation.jet | - | FAIL | expected `Error [jet.OwnerMovedWhileViewed]: `left` cannot be changed by `.push()` while `result` is still looking in...`, got `Error [jet.MissingReceiverMark]: This call writes `left`, but `left` has no `&` mark` |
| tests/ui/ownership/multi_source_returned_aggregate_still_blocks_owner_invalidation_2.jet | - | FAIL | expected `Error [jet.OwnerMovedWhileViewed]: `right` cannot be changed by `.push()` while `result` is still looking i...`, got `Error [jet.MissingReceiverMark]: This call writes `right`, but `right` has no `&` mark` |
| tests/ui/ownership/mutable_view_aggregates_are_not_cloneable.jet | - | FAIL | expected `12 \|     duplicate :: ~borrowed`, got `12 \|     duplicate :: borrowed.copy()` |
| tests/ui/ownership/mutable_view_extraction_retires_the_source_aggregate.jet | - | FAIL | expected `Error [jet.UseAfterMove]: `aggregate` was consumed by the binding `selected`, so it can't be used here`, got `Error [jet.ReadDuringWriteWindow]: `aggregate.values` cannot be read while `selected` has a live exclusive ...` |
| tests/ui/ownership/mutable_view_extraction_retires_the_source_aggregate_2.jet | - | FAIL | expected `Error [jet.UseAfterMove]: `optional` was consumed by the pattern match, so it can't be used here`, got `Error [jet.ValueTypeMismatch]: `print` doesn't know how to show ViewMut<Int>?` |
| tests/ui/ownership/named_write_window_calls_keep_alias_and_read_view_checks.jet | - | FAIL | expected `Error [jet.ImmutableAssignment]: `window` was made with `::`, so it can't be changed`, got `Error [jet.ReadDuringWriteWindow]: `value` cannot be read while `window` has a live exclusive write window ...` |
| tests/ui/ownership/named_write_window_calls_keep_alias_and_read_view_checks_2.jet | - | FAIL | expected `Error [jet.ImmutableAssignment]: `read` was made with `::`, so it can't be changed`, got `Error [jet.FieldWriteWithoutMarker]: Cannot edit through `read` — it has read access only` |
| tests/ui/ownership/nested_lambda_forms_use_the_enclosing_call_access_frame.jet | - | FAIL | expected `Error [jet.UseWhileWriteActive]: `values` is being changed in this call, so it can't be used again here`, got `(no errors)` |
| tests/ui/ownership/open_dynamic_trait_view_dispatch_is_rejected.jet | - | FAIL | expected `Error [jet.ViewOutlivesOwner]: Returned views need a stable owner relationship`, got `Error [jet.MissingDynamicEffectBound]: Dynamic call `Select::select` has no effect bound` |
| tests/ui/ownership/optional_constructor_payload_obeys_field_ownership.jet | - | FAIL | missing expected stream: tests/ui/ownership/optional_constructor_payload_obeys_field_ownership.stderr |
| tests/ui/ownership/parameter_rooted_lambda_and_generic_callback_preserve_view_provenance.jet | - | FAIL | expected `(no errors)`, got `Error [jet.ViewOutlivesOwner]: Returned views need a stable owner relationship` |
| tests/ui/ownership/pattern_value_tests_and_reactive_clones_use_runtime_capture_places.jet | - | FAIL | expected `Why: The pattern already names every field of `Incident``, got `Why: The pattern already names every field` |
| tests/ui/ownership/pattern_value_tests_and_reactive_clones_use_runtime_capture_places_2.jet | - | FAIL | expected `--> tests/ui/ownership/pattern_value_tests_and_reactive_clones_use_runtime_capture_places_2.jet:12:27`, got `--> tests/ui/ownership/pattern_value_tests_and_reactive_clones_use_runtime_capture_places_2.jet:12:21` |
| tests/ui/ownership/reactive_root_capture_replaces_existing_owner_projections.jet | - | FAIL | expected `Error [jet.UseWhileWriteActive]: `pair` is being changed in this call, so it can't be used again here`, got `(no errors)` |
| tests/ui/ownership/reactive_root_capture_replaces_existing_owner_projections_2.jet | - | FAIL | expected `--> tests/ui/ownership/reactive_root_capture_replaces_existing_owner_projections_2.jet:10:15`, got `--> tests/ui/ownership/reactive_root_capture_replaces_existing_owner_projections_2.jet:10:9` |
| tests/ui/ownership/return_fallback_view_does_not_reach_the_enclosing_call.jet | - | FAIL | expected `(no errors)`, got `Error [jet.ReturnTypeMismatch]: This value is [Int], but `first` returns View<Int>` |
| tests/ui/ownership/returned_string_view_field_cannot_outlive_local_owner.jet | - | FAIL | expected `Error [jet.ViewOutlivesOwner]: This view points into `email`, which this function owns`, got `` |
| tests/ui/ownership/returned_view_aggregate_blocks_owner_resize.jet | - | FAIL | expected `Error [jet.OwnerMovedWhileViewed]: `values` cannot be changed by `.push()` while `result` is still looking ...`, got `(no errors)` |
| tests/ui/ownership/returned_view_aggregate_cannot_cross_task_boundary.jet | - | FAIL | missing expected stream: tests/ui/ownership/returned_view_aggregate_cannot_cross_task_boundary.stderr |
| tests/ui/ownership/returned_view_composes_through_inherent_method.jet | - | FAIL | expected `(no errors)`, got `Error [jet.ViewOutlivesOwner]: Returned views need a stable owner relationship` |
| tests/ui/ownership/runtime_disjoint_views_reject_alias_storage_and_owner_invalidation_2.jet | - | FAIL | expected `Error [jet.OwnerMovedWhileViewed]: `values` cannot be changed by `.push()` while `selected` is still lookin...`, got `` |
| tests/ui/ownership/semantic_capture_events_drive_retention_and_deferred_clone_modes_3.jet | - | FAIL | expected `Error [jet.UseAfterMove]: `values` was consumed by a lambda that captures it, so it can't be used here`, got `(no errors)` |
| tests/ui/ownership/shared_edit_guard_spans_helper_calls.jet | - | FAIL | expected `(no errors)`, got `Error [jet.MissingReceiverMark]: This call writes `guard.value`, but `guard.value` has no `&` mark` |
| tests/ui/ownership/stored_lambda_returning_view_is_rejected_before_codegen.jet | - | FAIL | expected `Error [jet.ViewOutlivesOwner]: This view points into `values`, which this function owns`, got `(no errors)` |
| tests/ui/ownership/trait_object_mut_self_calls_use_receiver_change_rules_inside_callbacks.jet | - | FAIL | missing expected stream: tests/ui/ownership/trait_object_mut_self_calls_use_receiver_change_rules_inside_callbacks.stderr |
| tests/ui/ownership/undeclared_view_callback_freezes_every_non_scalar_argument.jet | - | FAIL | expected `Error [jet.OwnerMovedWhileViewed]: `b` cannot be assigned while `result` is still looking into it`, got `Error [jet.ViewOutlivesOwner]: Returned views need a stable owner relationship` |
| tests/ui/ownership/whole_place_alias_blocks_overlapping_write_argument.jet | - | FAIL | expected `Error [jet.UseWhileWriteActive]: `xs` is being changed in this call, so it can't be used again here`, got `(no errors)` |
| tests/ui/ownership/wrapper_returned_view_aggregates_render_lifetimes_on_named_leaves.jet | - | FAIL | expected `(no errors)`, got `Error [jet.FailureDomainNotErrorType]: Explicit failure domain `String (text)` is not an Error type` |
| tests/ui/ownership/write_window_cannot_be_stored.jet | - | FAIL | expected `Error [jet.OwnerMovedWhileViewed]: `xs[…]` already has a live view that conflicts with `holder``, got `` |
| tests/ui/ownership/write_window_cannot_cross_task_boundary.jet | - | FAIL | expected `Error [jet.ValueNotSendable]: `edit` cannot be shared into a task — it is a view that does not live long en...`, got `Error [jet.ArenaViewEscapes]: `edit` cannot be shared — it does not live long enough to be captured by a st...` |
| tests/ui/ownership/zero_copy_parser_blocks_source_replacement_while_token_is_live.jet | - | FAIL | expected `Error [jet.OwnerMovedWhileViewed]: `source` cannot be assigned while `selected` is still looking into it`, got `Error [jet.ViewOutlivesOwner]: Returned views need a stable owner relationship` |
| tests/ui/ownership/zero_copy_parser_blocks_source_replacement_while_token_is_live_2.jet | - | FAIL | expected `Error [jet.OwnerMovedWhileViewed]: `source` cannot be assigned while `selected` is still looking into it`, got `Error [jet.ViewOutlivesOwner]: Returned views need a stable owner relationship` |
| tests/ui/ownership/zero_copy_parser_rejects_locally_owned_source.jet | - | FAIL | expected `--> tests/ui/ownership/zero_copy_parser_rejects_locally_owned_source.jet:11:25`, got `` |
| tests/ui/ownership/zero_copy_parser_returns_token_and_remainder_views.jet | - | FAIL | expected `(no errors)`, got `Error [jet.MissingDisplay]: ``View`<str>` has no `Display` implementation` |
| tests/ui/parallel_function_worker_type.jet | - | FAIL | expected `Error [jet.ValueNotSendable]: `parallel item` can't cross a parallel worker boundary because `fn(Int) Int` ...`, got `` |
| tests/ui/params_not_yet.jet | - | FAIL | expected `Error [jet.PatternTypeMismatch]: Multi-head variant `name` is not an enum variant`, got `Error [jet.InvalidStatement]: Expected `:` after the parameter `name`` |
| tests/ui/parent_control_cancelled.jet | - | REFUSED | missing CLI fixture API: parent_control_cancel |
| tests/ui/parse_pattern_no_else.jet | - | FAIL | expected `\|     ^^^^^^^^^^^^^`, got `\|     ^^` |
| tests/ui/path_builtin_not_core_record.jet | - | FAIL | expected `Error [jet.BindingTypeMismatch]: This needs path.Path, but the value is Path`, got `Error [jet.ValueTypeMismatch]: `join` wants `path.Path` for argument 1, but this is `Path`` |
| tests/ui/pattern_binding_count.jet | - | FAIL | expected `--> tests/ui/pattern_binding_count.jet:7:8`, got `--> tests/ui/pattern_binding_count.jet:7:13` |
| tests/ui/pattern_hole_reuse_constant.jet | - | FAIL | expected `Why: Jet enforces PascalCase for type-like names, snake_case for value-like names, and ALL_CAPS (SCREAMING_...`, got `Why: Jet enforces PascalCase for type-like names, snake_case for value-like names, and ALL_CAPS (SCREAMING_...` |
| tests/ui/pattern_ratified_map_non_exhaustive.jet | - | FAIL | expected `Error [jet.NonExhaustiveDispatch]: This `if` doesn't cover every `[String:Int]` value — it needs an `else` arm`, got `Error [jet.NonExhaustiveDispatch]: This `if` doesn't cover every case — missing: a map with other keys` |
| tests/ui/pattern_wrong_type.jet | - | FAIL | expected `Error [jet.PatternTypeMismatch]: Pattern `Red` doesn't match Int?`, got `Error [jet.PatternTypeMismatch]: Pattern `Red` doesn't match `Int?`` |
| tests/ui/payload_write_window_owner_use.jet | - | FAIL | expected `Why: `session` is a live exclusive mutable view into `result.session.{session}`; changing or moving the own...`, got `Why: `session` is a live exclusive mutable view into `result.session`; changing or moving the owner could i...` |
| tests/ui/phantom_fact_menu_capability.jet | - | FAIL | expected `Error [jet.UnknownType]: `Ability` is retired`, got `Error [jet.UnknownType]: There's no type called `Ability`` |
| tests/ui/policy_site_bound_authority.jet | - | FAIL | expected `Why: Authority markers stay attached to the audited operation or declaration; policy scope cannot widen them`, got `Why: The compiler owns the closed policy registry and its scope rules` |
| tests/ui/primitive_static_to_string.jet | - | FAIL | expected `Error [jet.UnknownFunction]: `Bool` has no static method `to_string``, got `Error [jet.UnknownName]: Nothing named `Bool` exists here` |
| tests/ui/print_lazy_iter_view.jet | - | FAIL | expected `Error [jet.ValueTypeMismatch]: `print` cannot show the one-pass source `HTTPBodyChunks``, got `` |
| tests/ui/project_module_duplicate/run.jet | - | FAIL | expected `Error [jet.AmbiguousModule]: Project module `project._bench` is declared more than once`, got `Error [jet.ImportWithoutProject]: Use project.<module> needs an owning project context.` |
| tests/ui/project_module_invalid/run.jet | - | FAIL | expected `Error [jet.UseTargetNotFound]: Can't load project module `project._bench` from `broken.jet``, got `Error [jet.ImportWithoutProject]: Use project.<module> needs an owning project context.` |
| tests/ui/project_module_missing/run.jet | - | FAIL | expected `Error [jet.UseTargetNotFound]: Can't find a project module named `project._bench``, got `Error [jet.ImportWithoutProject]: Use project.<module> needs an owning project context.` |
| tests/ui/protocol_out_of_order.jet | - | FAIL | expected `Error [jet.ValueThrownAway]: This protocol_out_of_order.Payment.Client value is thrown away`, got `Error [jet.ValueThrownAway]: This Payment.Client value is thrown away` |
| tests/ui/pure/pure_fn_comptime_block_is_excluded_from_runtime_check.jet | - | FAIL | expected `Error [jet.ImpureCallInPureContext]: `print` is not allowed in comptime code`, got `Error [jet.ComptimeEffectWithoutImpure]: `standard output` is a Tier-2 comptime effect — it requires a `#Im...` |
| tests/ui/quantity_dimension_mismatch.jet | - | FAIL | expected `Error [jet.DimensionMismatch]: `+` can't combine Length with Time`, got `Error [jet.DistinctArithmeticNotAllowed]: `+` is not available between `Meter` and `Duration`` |
| tests/ui/quantity_measured_scale.jet | - | FAIL | expected `--> tests/ui/quantity_measured_scale.jet:4:13`, got `--> tests/ui/quantity_measured_scale.jet:4:12` |
| tests/ui/quantity_point_plus_point.jet | - | FAIL | expected `Error [jet.BindingTypeMismatch]: Argument to `CelsiusPoint.from_float` should be Float, not Decimal`, got `Error [jet.DistinctArithmeticNotAllowed]: `+` is not available between `CelsiusPoint` and `CelsiusPoint`` |
| tests/ui/reader_take_sized_int.jet | - | FAIL | expected `(no errors)`, got `Error [jet.BindingTypeMismatch]: This needs Int, but the value is U64` |
| tests/ui/receiver_mark_extra_fresh.jet | - | FAIL | expected `Error [jet.UnusedPlaceMark]: `&` marks a fresh value, not a named place`, got `Error [jet.MissingWriteMarker]: The write-access marker `&` needs a place after it` |
| tests/ui/reflect_name_ledger/run.jet | - | FAIL | expected `--> tests/ui/reflect_name_ledger/run.jet:10:17`, got `--> tests/ui/reflect_name_ledger/run.jet:12:17` |
| tests/ui/reflect_of_not_displayable.jet | - | FAIL | expected `Error [jet.ValueTypeMismatch]: fn(Int) Int -[]> can't be reflected yet`, got `Error [jet.ValueTypeMismatch]: fn(Int) -[]> Int Never! can't be reflected yet` |
| tests/ui/refutable_bind_same_name_shadow.jet | - | FAIL | snapshot mismatch: tests/ui/refutable_bind_same_name_shadow.stderr |
| tests/ui/repl_effect_denied_e1803.jet | - | FAIL | directive requires nonzero exit |
| tests/ui/result_handler_non_result.jet | - | FAIL | snapshot mismatch: tests/ui/result_handler_non_result.stderr |
| tests/ui/result_handler_type_mismatch.jet | - | FAIL | expected `Error [jet.PatternTypeMismatch]: This pattern belongs to a fallible result, not Int`, got `Error [jet.PatternTypeMismatch]: This pattern belongs to a fallible result, not `Int`` |
| tests/ui/result_loop_break_payloads.jet | - | FAIL | expected `Error [jet.LoopResultMismatch]: This result loop exits without a value`, got `` |
| tests/ui/retired_cli_marker.jet | - | FAIL | snapshot mismatch: tests/ui/retired_cli_marker.stderr |
| tests/ui/retired_environment_flag.jet | - | REFUSED | missing CLI fixture API: jetpack_retired_environment_flag |
| tests/ui/return_value_wrong_shape.jet | - | FAIL | expected `--> tests/ui/return_value_wrong_shape.jet:6:5`, got `` |
| tests/ui/runtime_e3014.jet | - | ERROR | internal compiler error (exit 101) |
| tests/ui/runtime_rng_backward.jet | - | FAIL | expected `--> tests/ui/runtime_rng_backward.jet:6`, got `Why: The operands or position do not produce a valid result, so the safe runtime stops instead of returning...` |
| tests/ui/script_imported/run.jet | - | FAIL | expected `Error [jet.ImportedScriptExecutes]: imported script `helper.jet` has executable top-level statements`, got `Error [jet.TopLevelStatementsWithoutRun]: Top-level executable statements require an explicit `fn run`` |
| tests/ui/secret_ungranted.jet | - | FAIL | expected `--> tests/ui/secret_ungranted.jet:3:4`, got `--> tests/ui/secret_ungranted.jet:3:1` |
| tests/ui/sema_soundness/compound_updates_require_their_operator.jet | - | FAIL | expected `Error [jet.OperatorTypeMismatch]: `+` can't compare or combine [Int] and [Int]`, got `` |
| tests/ui/semaphore_teaching.jet | - | FAIL | snapshot mismatch: tests/ui/semaphore_teaching.stderr |
| tests/ui/serde_field_not_serializable.jet | - | FAIL | expected `Error [jet.NotSerializable]: `plain` can't be serialized`, got `Error [jet.NotSerializable]: `Plain` can't be serialized` |
| tests/ui/serde_generated_cascade_keeps_user_impl_error.jet | - | FAIL | expected `Error [jet.DuplicateDefinition]: method `encode` is defined twice on `Bad``, got `Error [jet.DuplicateDefinition]: Method `encode` is defined twice on `Bad`` |
| tests/ui/serde_rename_not_string.jet | - | FAIL | snapshot mismatch: tests/ui/serde_rename_not_string.stderr |
| tests/ui/serde_u64_direct_codable.jet | - | FAIL | expected `(no errors)`, got `Error [jet.UnknownCoreItem]: `core.encoding.cbor` exposes `decode`, but not in this position` |
| tests/ui/shared_guard_computed_map.jet | - | FAIL | expected `\|                    ^^^^^^^^^^^^^^^^^^^^^`, got `\|                    ^^^^^` |
| tests/ui/sigil_move_capability_misuse.jet | - | FAIL | expected `Error [jet.MissingMoveMarker]: `consume` needs the move marker `^` here — this value can't be copied`, got `Error [jet.MoveWithoutMarker]: Implicit clone of `n`` |
| tests/ui/source_nesting_limit.jet | - | FAIL | expected `Error [jet.NestingTooDeep]: this code nests 257 levels deep; the limit is 256`, got `Error [jet.InvalidStatement]: This expression is nested too deeply` |
| tests/ui/spread_bad_call.jet | - | FAIL | expected `--> tests/ui/spread_bad_call.jet:7:5`, got `--> tests/ui/spread_bad_call.jet:7:11` |
| tests/ui/suppress_retired.jet | - | FAIL | expected `Error [jet.UnknownAppliedRule]: `#Suppress` is retired`, got `Error [jet.UnknownAppliedRule]: `#Suppress` isn't a known applied rule` |
| tests/ui/taint/external_boundaries_seed_origin_facts_3.jet | - | FAIL | expected `Error [jet.ValueThrownAway]: This SQL value is thrown away`, got `Error [jet.TaggedValueDenied]: A `$origin.process` value is denied at `SQL.raw`` |
| tests/ui/taint/external_boundaries_seed_origin_facts_4.jet | - | FAIL | expected `Error [jet.ValueThrownAway]: This SQL value is thrown away`, got `Error [jet.MissingWriteMarker]: Parameter `stream` requires the write-access marker `&` at the call site` |
| tests/ui/taint/origin_marked_text_rejects_raw_sinks.jet | - | FAIL | expected `--> tests/ui/taint/origin_marked_text_rejects_raw_sinks.jet:7:8`, got `` |
| tests/ui/taint/resolved_instance_method_paths_introduce_declared_tags.jet | - | FAIL | expected `Error [jet.TaggedValueDenied]: A `Stored` value is denied at `process.run``, got `` |
| tests/ui/taint/tagged_struct_fields_enter_dataflow_on_access.jet | - | FAIL | expected `Error [jet.TaggedValueDenied]: A `PII` value is denied at `process.run``, got `` |
| tests/ui/taint/tainted_credential_to_jsonl_is_e0722.jet | - | FAIL | expected `Error [jet.ValueThrownAway]: This String value is thrown away`, got `Error [jet.UnknownName]: Nothing named `JSON` exists here` |
| tests/ui/taint_sink_unsanitized.jet | - | FAIL | expected `Error [jet.TaggedValueDenied]: a `Input` value is denied at `process.run``, got `Error [jet.UnknownCoreItem]: `core.process` has no item `run`` |
| tests/ui/take_pattern_computed_arg.jet | - | FAIL | expected `Error [jet.InvalidStatement]: `take_pattern` takes a literal pattern string, not the name `pat``, got `Error [jet.InvalidStatement]: `take_pattern` needs a literal text or binary pattern` |
| tests/ui/take_required.jet | - | FAIL | expected `Error [jet.MissingMoveMarker]: `consume` needs the move marker `^` here — this value can't be copied`, got `Error [jet.MoveWithoutMarker]: Implicit clone of `n`` |
| tests/ui/task_detach_unsound.jet | - | FAIL | expected `Error [jet.ReadOnlyValueNotOwned]: `greet` was not moved here, so it cannot be captured by a spawned task`, got `Error [jet.ReadOnlyValueNotOwned]: `greet` was not moved here, so it cannot be captured by a stored lambda` |
| tests/ui/task_detach_view_capture.jet | - | FAIL | expected `Error [jet.ViewOutlivesOwner]: a returned view cannot borrow from a temporary argument`, got `Error [jet.ValueNotSendable]: This task returns a view that cannot be shared — it does not live long enough...` |
| tests/ui/task_spawn_parameters.jet | - | FAIL | expected `Error [jet.ValueNotSendable]: this task returns `fn(Int) Int -[]>`, which isn't sendable`, got `Error [jet.InvalidStatement]: Something unexpected appeared after this line, found `->`` |
| tests/ui/task_view_result.jet | - | FAIL | expected `Error [jet.ViewOutlivesOwner]: a returned view cannot borrow from a temporary argument`, got `Error [jet.ValueNotSendable]: This task returns a view that cannot be shared — it does not live long enough...` |
| tests/ui/taskgroup_parameter_view.jet | - | FAIL | expected `--> tests/ui/taskgroup_parameter_view.jet:2:16`, got `--> tests/ui/taskgroup_parameter_view.jet:2:10` |
| tests/ui/test_block_nested.jet | - | FAIL | expected `Why: the registry allows `#Test` only at: `Test``, got `Why: The registry allows `#Test` only at: `Test`` |
| tests/ui/todo_call_e0803.jet | - | FAIL | snapshot mismatch: tests/ui/todo_call_e0803.stderr |
| tests/ui/tracked_origin_runtime_projection.jet | - | FAIL | expected `Error [jet.UnknownMethod]: `.origin()` is retired for tracked values`, got `Error [jet.UnknownMethod]: `origin` isn't a method on this value` |
| tests/ui/trailing_block_not_function.jet | - | FAIL | expected `Error [jet.BlockAfterCall]: trailing blocks are gone — pass code with `() ->``, got `Error [jet.ValueTypeMismatch]: `add` wants Int (a whole number) for argument 2, but this is fn()` |
| tests/ui/transact_irreversible_effect.jet | - | FAIL | expected `Error [jet.IrreversibleEffectInTransaction]: `files.read` has the `FS` effect, which can't be rolled back i...`, got `(no errors)` |
| tests/ui/try_suffix_staged.jet | - | FAIL | expected `Error [E-ERR-SUFFIX]: `Error!` is a retired suffix failure contract.`, got `Error [jet.BareQuestionMark]: A bare `?` has no meaning.` |
| tests/ui/tuple_duplicate_field.jet | - | FAIL | expected `Error [jet.InvalidStatement]: tuple member `x` appears more than once`, got `Error [jet.DuplicateDefinition]: Tuple field `x` is supplied more than once` |
| tests/ui/typestate/state_section_shape_diagnostics_are_distinct_5.jet | - | FAIL | expected `--> tests/ui/typestate/state_section_shape_diagnostics_are_distinct_5.jet:2:27`, got `--> tests/ui/typestate/state_section_shape_diagnostics_are_distinct_5.jet:2:13` |
| tests/ui/typestate/unknown_state_in_marker_is_e0151.jet | - | FAIL | expected `Error [jet.TypestateWrongState]: `get` needs `Crate` in state `Stuffed`, but `b` is in state `Full``, got `` |
| tests/ui/typestate_state_diverges.jet | - | FAIL | expected `Warning [jet.DivergentStatePaths]: `order` ends in state `Confirmed` on one path and `Cancelled` on another.`, got `(no errors)` |
| tests/ui/typestate_unknown_state.jet | - | FAIL | expected `Error [jet.TypestateWrongState]: `get` needs `Crate` in state `Stuffed`, but `b` is in state `Full``, got `` |
| tests/ui/unknown_char.jet | - | FAIL | snapshot mismatch: tests/ui/unknown_char.stderr |
| tests/ui/unknown_type.jet | - | FAIL | expected `Error [jet.ValueTypeMismatch]: `print` doesn't know how to show `Strng``, got `Error [jet.BindingTypeMismatch]: This needs Strng, but the value is String` |
| tests/ui/unsafe_and_runtime/optional_flow_narrowing_rejects_unstable_subjects_2.jet | - | FAIL | expected `--> tests/ui/unsafe_and_runtime/optional_flow_narrowing_rejects_unstable_subjects_2.jet:7:17`, got `--> tests/ui/unsafe_and_runtime/optional_flow_narrowing_rejects_unstable_subjects_2.jet:7:15` |
| tests/ui/unsafe_obligation_missing.jet | - | FAIL | expected `Error [jet.MissingUnsafeObligation]: `raw_pointer` is missing unsafe obligations: no_alias`, got `(no errors)` |
| tests/ui/unsafe_obligation_no_bleed.jet | - | FAIL | expected `Error [jet.MissingUnsafeObligation]: `raw_pointer` is missing unsafe obligations: no_alias`, got `(no errors)` |
| tests/ui/use_unrealized_library/run.jet | - | FAIL | expected `Error [jet.UnrealizedLibrary]: The library package `stringsmith_xyz` hasn't been built yet`, got `Error [jet.UseTargetNotFound]: The version dependency `stringsmith_xyz` (`acme/stringsmith_xyz/abc123`) has...` |
| tests/ui/user_policy_bad_args.jet | - | FAIL | expected `Error [jet.ArgumentCountMismatch]: Policy `audit` expects 1 argument, got 0`, got `Error [jet.MissingArgument]: This call to `audit` is missing `topic`` |
| tests/ui/validate_over_unknown_field.jet | - | FAIL | snapshot mismatch: tests/ui/validate_over_unknown_field.stderr |
| tests/ui/value_carrying_return_resultless.jet | - | FAIL | expected `Error [jet.FallbackTypeMismatch]: `?? return` can't return a value here`, got `Error [jet.ValueThrownAway]: This Int value is thrown away` |
| tests/ui/value_loop_exhaustion_route.jet | - | FAIL | expected `--> tests/ui/value_loop_exhaustion_route.jet:6:5`, got `--> tests/ui/value_loop_exhaustion_route.jet:2:35` |
| tests/ui/variadic_bound_body_shape.jet | - | FAIL | expected `Error [jet.VariadicOutsideLoop]: `parts` can only be used in a direct `loop x in parts { … }` loop here`, got `Error [jet.UnknownType]: There's no type called `Renderable`` |
| tests/ui/variadic_trait_bound_unmet.jet | - | FAIL | expected `Error [jet.VariadicBoundNotImplemented]: `fn(Int) Int -[]>` doesn't implement `Renderable``, got `Error [jet.UninferableTypeArgument]: No trait named `Renderable` is in scope for the bound on `parts__Va1`` |
| tests/ui/vault_expert_import_bypass.jet | - | FAIL | snapshot mismatch: tests/ui/vault_expert_import_bypass.stderr |
| tests/ui/vault_key_unlock_outlives_owner.jet | - | FAIL | expected `Error [jet.ViewOutlivesOwner]: This view points into `identity`, which this function owns`, got `(no errors)` |
| tests/ui/view_from_slots_escape.jet | - | FAIL | expected `Error [jet.ViewOutlivesOwner]: Returned view escapes its declared `from` clause`, got `(no errors)` |
| tests/ui/view_mut_bounds_interpreter.jet | - | FAIL | expected `--> tests/ui/view_mut_bounds_interpreter.jet:4 in run`, got `--> tests/ui/view_mut_bounds_interpreter.jet:4` |
| tests/ui/view_return_unknown_owner_set.jet | - | FAIL | expected `Error [jet.ViewOutlivesOwner]: Returned views need a stable owner relationship`, got `Error [jet.ReturnTypeMismatch]: This value is Int (a whole number), but `bad_view` returns View<Int>` |
| tests/ui/wasip2_unsupported_socket_operation_e3305.jet | - | REFUSED | missing CLI fixture API: target |
| tests/ui/wasip2_unsupported_socket_surface_e3304.jet | - | REFUSED | missing CLI fixture API: target |
| tests/ui/wasip2_unsupported_surface_e3304.jet | - | REFUSED | missing CLI fixture API: target |
| tests/ui/wasip2_unsupported_tls_surface_e3305.jet | - | REFUSED | missing CLI fixture API: target |
| tests/ui/wasip2_unsupported_websocket_e3305.jet | - | REFUSED | missing CLI fixture API: target |
| tests/ui/web_environment_host_unsupported.jet | - | FAIL | missing expected stream: tests/ui/web_environment_host_unsupported.stderr |
| tests/ui/web_suite_forms_type_mismatch.jet | - | FAIL | expected `Error [jet.ValueTypeMismatch]: `field` wants WebForm for argument 1, but this is String (text)`, got `Error [jet.ValueTypeMismatch]: `field` wants `WebForm` for argument 1, but this is String (text)` |
| tests/ui/web_suite_query_type_mismatch.jet | - | FAIL | expected `Error [jet.ValueTypeMismatch]: `invalidate` wants WebQuery for argument 1, but this is String (text)`, got `Error [jet.ValueTypeMismatch]: `invalidate` wants `WebQuery` for argument 1, but this is String (text)` |
| tests/ui/web_suite_router_type_mismatch.jet | - | FAIL | expected `Error [jet.ValueTypeMismatch]: `navigate` wants WebRouter for argument 1, but this is String (text)`, got `Error [jet.ValueTypeMismatch]: `navigate` wants `WebRouter` for argument 1, but this is String (text)` |
| tests/ui/web_target_browser.jet | - | FAIL | snapshot mismatch: tests/ui/web_target_browser.stderr |
| tests/ui/workspace_lock_e1202/run.jet | - | REFUSED | missing CLI fixture API: workspace_lock_e1202 |
| tests/ui/yielding_loop_item_mismatch.jet | - | FAIL | expected `Error [jet.CollectingLoopItemMismatch]: This collecting loop produces incompatible item types`, got `Error [jet.BranchTypeMismatch]: This `if`'s branches produce different types: Int (a whole number) and Stri...` |
| tests/ui_lint/a11y_duplicate_label.jet | - | FAIL | lint fixture must compile and emit a lint |
| tests/ui_lint/a11y_unlabeled_control.jet | - | FAIL | lint fixture must compile and emit a lint |
| tests/ui_lint/adjacent_subject_dispatch.jet | - | FAIL | lint fixture must compile and emit a lint |
| tests/ui_lint/arena_heap_loop.jet | - | FAIL | expected `Warning [jet.HeapGrowthInLoop]: Heap growth inside a loop — consider an arena`, got `Warning [jet.UnusedImport]: Import `mem` is never used.` |
| tests/ui_lint/arrow_loop_value_drop.jet | - | FAIL | lint fixture must compile and emit a lint |
| tests/ui_lint/build_entry_liveness.jet | - | FAIL | expected `Warning [jet.RedundantFinalReturn]: An explicit `return` is used on the final value line`, got `Warning [jet.UnusedParameter]: Parameter `b` is never used.` |
| tests/ui_lint/busy_spin.jet | - | FAIL | expected `Warning [jet.MutableNeverReassigned]: This mutable binding receives no later write, so its mutability is un...`, got `` |
| tests/ui_lint/compiler_extension_finding.jet | - | FAIL | lint fixture must compile and emit a lint |
| tests/ui_lint/compute_transform_inline.jet | - | FAIL | lint fixture must compile and emit a lint |
| tests/ui_lint/cost_hidden_in_loop.jet | - | FAIL | expected `Warning [jet.HiddenCostInLoop]: This copy of `String (text)` view into an owned slot repeats inside a loop.`, got `` |
| tests/ui_lint/hidden_alloc_context.jet | - | FAIL | lint fixture must compile and emit a lint |
| tests/ui_lint/layout_redundant_constraint.jet | - | FAIL | expected `Warning [jet.DuplicateLayoutConstraint]: This constraint repeats one already written in this `Layout.{ … }`...`, got `Warning [jet.DuplicateLayoutConstraint]: This constraint repeats one already written in this `Layout{ … }` ...` |
| tests/ui_lint/ownership/shared_auto_clone_in_loop_is_lint.jet | - | FAIL | expected `--> tests/ui_lint/ownership/shared_auto_clone_in_loop_is_lint.jet:8:14`, got `` |
| tests/ui_lint/prelude_assert_eq_shadow.jet | - | FAIL | lint fixture must compile and emit a lint |
| tests/ui_lint/prelude_shadow_warn.jet | - | FAIL | lint fixture must compile and emit a lint |
| tests/ui_lint/raw_multiline_html.jet | - | FAIL | lint fixture must compile and emit a lint |
| tests/ui_lint/redundant_fixed_cleanup.jet | - | FAIL | lint fixture must compile and emit a lint |
| tests/ui_lint/redundant_tail_return.jet | - | FAIL | lint fixture must compile and emit a lint |
| tests/ui_lint/subjectless_guard_enum_dispatch.jet | - | FAIL | lint fixture must compile and emit a lint |
| tests/ui_lint/subjectless_guard_field_enum_dispatch.jet | - | FAIL | lint fixture must compile and emit a lint |
| tests/ui_lint/tail_call_blocked.jet | - | FAIL | expected `Warning [jet.TailCallBlocked]: This tail call can't reuse the frame because `d` closes after it returns`, got `Warning [jet.RedundantFinalReturn]: An explicit `return` is used on the final value line` |
| tests/ui_lint/typestate_unreachable.jet | - | FAIL | expected `Warning [jet.RedundantFinalReturn]: An explicit `return` is used on the final value line`, got `Warning [jet.UnreachableState]: `Flow.State.Orphan` is unreachable from every entry transition.` |
| tests/ui_lint/unit_scalar_rewrap.jet | - | FAIL | lint fixture must compile and emit a lint |
| tests/ui_lint/unreachable_else_enum.jet | - | FAIL | missing expected stream: tests/ui_lint/unreachable_else_enum.warn |

Known-failing rows by card: #6425 102, #6006 9, #4926 2, #4717 1.

## Cases (filtered)

Filters: `encoding_parity/serde_derive_examples_match_production_cli_tiers`, `production_receipt/production_failure_receipt`, `tir_core_and_closures/parallel_collection_adapters`, `corelib/core_data_group_fields`, `corelib/core_data_lazy_and_join`, `compute_parity/`, `compute_extended/`, `compute_cuda/cuda_public_precision_gate`, `fact_declaration_tiers/measure_rows`, `memory_semantics/memo_computed_and_view_copy`, `dev_parts_tier_parity/ui_and_web_match`, `services_runtime/source_worker_slot`, `ui_fixes/repeated_list_head`, `cli_parts_commands/check_fixed_dynamic_size`, `cli_parts_commands/check_receipt_invalidates`, `single_use/`, `inline_package/`, `output_callable/`, `testing_tiers/doctests`, `place_window_lifetimes/`, `policy_scope/unsafe_obligations`, `memory_semantics/manifest_memory_denial`, `pkg/cli_build_enforces_lint_policy_e1293`, `determinism/`, `pkg/compiler_rejects_transitive_path_dependency`, `core_and_closures/parallel_collection_adapters_use_stable`, `corelib/core_net_dns_transaction_ids`.

Counts: ERROR 1, FAIL 12, KNOWN-FAILING 27, PASS 79.

| case | tier | status | card or first line |
|---|---|---|---|
| cli_parts_commands/check_fixed_dynamic_size_reports_e0103_without_internal_failure.case | - | FAIL | exit 1; expected 0 |
| cli_parts_commands/check_receipt_invalidates_when_higher_priority_entry_appears.case | - | FAIL | stream predicate ~ failed |
| compute_cuda/cuda_public_precision_gate_is_identical_on_all_tiers.case | - | PASS |  |
| compute_extended/autodiff_sparse_simd_and_streams_use_real_paths.case | - | KNOWN-FAILING | #3955: ICE ir.build unlowered generic Core call core.compute.gradient/jvp/vjp/value_and_gradient: autodiff is not built in Core |
| compute_extended/broadcast_to_matches_the_index_formula_on_every_axis_pattern.case | - | PASS |  |
| compute_extended/curried_autodiff_shapes_share_the_prelude_handle.case | - | KNOWN-FAILING | #5454: verify-intent: ICE ir.build unlowered generic Core call core.compute.value_and_gradient/jvp/vjp (autodiff runtime tape w |
| compute_extended/data_series_feeds_the_same_compute_tensor_path.case | - | PASS |  |
| compute_extended/f32_simd_matmul_keeps_vjp_and_jvp_on_the_cpu_oracle.case | - | KNOWN-FAILING | #5454: verify-intent: ICE ir.build unlowered generic Core call core.compute.gradient/jvp (autodiff runtime tape waits on ballot |
| compute_extended/f32_tile_matches_cpu_oracle_on_tails_and_rejects_hostile_matrices.case | - | PASS |  |
| compute_extended/fft_preserves_prime_and_composite_length_inputs.case | - | PASS |  |
| compute_extended/linalg_and_fft_use_the_cpu_oracle.case | - | PASS |  |
| compute_extended/ml_serialization_and_placement_failures_stay_in_the_same_tier.case | - | PASS |  |
| compute_extended/portable_accelerator_example_uses_vulkan_and_fails_closed_for_native_webgpu.case | - | PASS |  |
| compute_extended/portable_accelerator_vulkan_operations_match_cpu_across_tiers.case | - | PASS |  |
| compute_extended/portable_accelerator_vulkan_rejects_nonfinite_kernel_output.case | - | PASS |  |
| compute_extended/safe_kernel_boundary_and_f32_profile_are_explicit.case | - | PASS |  |
| compute_extended/safe_kernel_rejects_effectful_bodies_before_codegen.case | - | PASS |  |
| compute_parity/compute_broadcast_ufunc_fuses_indexing_and_arithmetic.case | - | PASS |  |
| compute_parity/compute_cpu_oracle_aot_covers_storage_algebra_and_corruption.case | - | PASS |  |
| compute_parity/compute_cpu_oracle_default_run_matches_aot_meaning.case | - | PASS |  |
| compute_parity/compute_fixed_vec_and_matrix_aliases_keep_shape_facts_on_the_tensor_substrate.case | - | KNOWN-FAILING | #5454: E0003 Expected a type, found `3` |
| compute_parity/compute_result_tensor_payload_survives_resident_return_cleanup.case | - | PASS |  |
| compute_parity/compute_set_requires_mutable_binding_before_codegen.case | - | KNOWN-FAILING | #5454: stream predicate ~ failed; first diagnostic: E0205 Cannot edit through `tensor` — it has read access only |
| compute_parity/compute_tensor_in_record_has_parity_across_tiers.case | - | PASS |  |
| compute_parity/matrix_shape_measures_match_or_diagnose_inner_sides.case | - | KNOWN-FAILING | #5454: E0003 Expected a type, found `3` |
| compute_parity/named_gradient_composes_into_second_derivative.case | - | KNOWN-FAILING | #5454: verify-intent: ICE ir.build unlowered generic Core call core.compute.gradient (autodiff runtime tape waits on ballot) |
| core_and_closures/parallel_collection_adapters_use_stable_bounded_chunks.case | - | PASS |  |
| corelib/core_data_group_fields_survive_core_name_registration_across_tiers.case | - | PASS |  |
| corelib/core_data_lazy_and_join_routes_match_aot_in_forced_interpreter.case | - | PASS |  |
| corelib/core_net_dns_transaction_ids_are_not_a_fixed_sequence.case | - | PASS |  |
| determinism/ambient_draws_are_never_folded_into_the_build.case | - | PASS |  |
| determinism/assume_deterministic_is_scoped.case | - | PASS |  |
| determinism/assume_deterministic_suppresses_e3401.case | - | KNOWN-FAILING | #5454: ref build failed: ICE: semantic body rechecking did not return a complete resolved package graph |
| determinism/assume_deterministic_suppresses_e3403.case | - | KNOWN-FAILING | #5454: assert not satisfied: original assert! not satisfied: [false] |
| determinism/clock_advance_needs_mut_receiver.case | - | PASS |  |
| determinism/pure_code_rejects_clock_observation_through_an_imported_nominal_type.case | - | KNOWN-FAILING | #5454: assert not satisfied: original assert! not satisfied: [false] |
| determinism/pure_code_rejects_clock_provenance_laundered_through_a_struct.case | - | KNOWN-FAILING | #5454: ref build failed: ICE: semantic body rechecking did not return a complete resolved package graph |
| determinism/pure_fn_ambient_random_still_e3403.case | - | PASS |  |
| determinism/pure_fn_ambient_time_still_e3403.case | - | PASS |  |
| determinism/pure_fn_constructs_caps_ok.case | - | KNOWN-FAILING | #5454: assert not satisfied: original assert! not satisfied: [false] |
| determinism/pure_fn_injected_rng_ok.case | - | KNOWN-FAILING | #5454: assert not satisfied: original assert! not satisfied: [false] |
| determinism/pure_fn_widened_clock_ok.case | - | KNOWN-FAILING | #5454: assert not satisfied: original assert! not satisfied: [false] |
| determinism/pure_fn_widened_rng_ok.case | - | KNOWN-FAILING | #5454: assert not satisfied: original assert! not satisfied: [false] |
| determinism/rng_bool_needs_mut_receiver.case | - | PASS |  |
| determinism/rng_pick_returns_element_option.case | - | KNOWN-FAILING | #5454: assert not satisfied: original assert! not satisfied: [false] |
| determinism/rng_shuffle_needs_mut_list_arg.case | - | PASS |  |
| determinism/system_clock_is_monotonic_and_effectful.case | - | KNOWN-FAILING | #5454: internal compiler error: semantic body rechecking did not return a complete resolved package graph (#5507) |
| dev_parts_tier_parity/ui_and_web_match_interpreter_default_run_and_aot_web_ui_showcase.case | - | PASS |  |
| dev_parts_tier_parity/ui_and_web_match_interpreter_default_run_and_aot_web_ui_web_click.case | - | PASS |  |
| dev_parts_tier_parity/ui_and_web_match_interpreter_default_run_and_aot_web_web_app.case | - | KNOWN-FAILING | #5454: FAIL: `jet run web_app.jet` exits 1 (expected 0): Error [E1803]: Application authority is undecided for `Browser, Net` |
| dev_parts_tier_parity/ui_and_web_match_interpreter_default_run_and_aot_web_web_wasm_callback.case | - | PASS |  |
| encoding_parity/serde_derive_examples_match_production_cli_tiers.case | - | FAIL | exit 1; expected 0 |
| fact_declaration_tiers/measure_rows_share_one_registry_and_all_tiers_examples.case | - | PASS |  |
| inline_package/inline_and_extracted_package_keep_facts_graph_and_authority_identity.case | - | PASS |  |
| inline_package/inline_package_conflicts_with_manifest_for_every_entry.case | - | FAIL | stream predicate ~ failed |
| inline_package/inline_package_example_matches_release_default_and_interpreter.case | - | PASS |  |
| inline_package/inline_package_extraction_matches_extracted_entry_across_build_and_run_tiers.case | - | PASS |  |
| inline_package/inline_package_import_cycle_fails_with_cycle_diagnostic_and_file_provenance.case | - | FAIL | JSON value differs: expected "E0604", got "E0623" |
| inline_package/inline_package_keeps_package_only_visibility.case | - | PASS |  |
| inline_package/inline_package_mask_preserves_parser_offsets_and_code.case | - | PASS |  |
| inline_package/inline_package_path_escape_fails_before_state_mutation_with_provenance.case | - | FAIL | exit 0; expected nonzero |
| inline_package/inline_package_rejects_ambiguous_manifest_metadata_before_loading.case | - | FAIL | JSON value differs: expected "E1239", got "E1363" |
| inline_package/inline_package_rejects_duplicate_carriers_before_state_change.case | - | PASS |  |
| inline_package/inline_package_rejects_retired_manifest_metadata_without_mutation.case | - | FAIL | exit 0; expected nonzero |
| inline_package/inline_package_rejects_stale_lock_metadata_with_lock_provenance.case | - | FAIL | JSON value differs: expected "E1202", got "E0603" |
| inline_package/inline_package_symlinked_metadata_is_rejected_without_hidden_host_state.case | - | FAIL | JSON parse or path failed: $.reports[0].code |
| memory_semantics/manifest_memory_denial_reaches_the_same_sema_fact_pass.case | - | PASS |  |
| memory_semantics/memo_computed_and_view_copy_examples_agree_across_execution_tiers.case | - | PASS |  |
| output_callable/aot_dev_and_jit_consume_the_resolved_entry.case | - | PASS |  |
| output_callable/check_outputs_are_plural_real_test_harness_entries_without_test_blocks.case | - | KNOWN-FAILING | #6035: unbuilt: Check outputs are not real test harness entries |
| output_callable/checked_default_selects_one_of_multiple_executables.case | - | PASS |  |
| output_callable/compiled_imported_typed_fallible_entry_uses_its_defining_module.case | - | PASS |  |
| output_callable/explicit_service_address_reaches_lowering_and_real_cli_runtime.case | - | KNOWN-FAILING | #6418: `jet run --output` is not built yet (E2104) |
| output_callable/inspect_output_reports_selected_callable_facts.case | - | PASS |  |
| output_callable/interpreter_runs_hot_swaps_and_reports_fallible_selected_entry.case | - | PASS |  |
| output_callable/invalid_output_selection_stops_in_jet_before_codegen.case | - | KNOWN-FAILING | #6418: unbuilt: E2104 command form not built (E2104) |
| output_callable/nested_output_links_report_e1321_before_codegen.case | - | PASS |  |
| output_callable/parser_retains_typed_output_function_reference.case | - | PASS |  |
| output_callable/qualified_entry_follows_nested_quoted_import_graph.case | - | KNOWN-FAILING | #4260: owner question: the nested entry `app.cli.command.run` and the re-export `app.cli_run` reach through plain `use` imports |
| output_callable/qualified_entry_keeps_one_definition_and_effect_identity.case | - | KNOWN-FAILING | #4260: no doc decides the inspect output authority label (Rust `safe-jet`, Jet `application default`) |
| output_callable/renamed_entry_is_not_recovered_from_a_stale_lock.case | - | KNOWN-FAILING | #5454: verify-intent: the fixture's package file imports `use "helper"`, which E0623 rejects (package files share one namespace |
| output_callable/runnable_contracts_and_selection_fail_in_sema.case | - | KNOWN-FAILING | #6418: unbuilt: `jet run --output` is not built yet (E2104) for the probe7 step; every other probe reports its E1321/E0101 from |
| output_callable/sema_resolves_sole_executable_entry_before_codegen.case | - | PASS |  |
| output_callable/service_edge_emits_one_structured_report_record.case | - | KNOWN-FAILING | #6418: `jet run --output` is not built yet (E2104) |
| output_callable/typed_executable_output_reuses_the_checked_cli_schema.case | - | PASS |  |
| pkg/cli_build_enforces_lint_policy_e1293.case | - | FAIL | exit 0; expected nonzero |
| pkg/compiler_rejects_transitive_path_dependency_escape.case | - | PASS |  |
| place_window_lifetimes/borrowed_window_stays_caller_owned.case | - | PASS |  |
| place_window_lifetimes/card_2821_nested_copy_binding_tier_parity.case | - | PASS |  |
| place_window_lifetimes/card_2822_nested_write_view.case | - | PASS |  |
| place_window_lifetimes/card_2823_scalar_field_write_view.case | - | PASS |  |
| place_window_lifetimes/card_2826_nested_scalar_write_view.case | - | PASS |  |
| place_window_lifetimes/card_4392_returned_view_names_its_owner_lifetime.case | - | PASS |  |
| place_window_lifetimes/card_4392_view_carrying_struct_names_its_owner_lifetime.case | - | PASS |  |
| place_window_lifetimes/card_4392_view_into_consumed_parameter_is_rejected_in_sema.case | - | PASS |  |
| place_window_lifetimes/heap_element_window_stays_allocator_owned.case | - | PASS |  |
| place_window_lifetimes/stack_window_uses_expiring_sentry_address.case | - | PASS |  |
| policy_scope/unsafe_obligations_are_checked_before_codegen.case | - | FAIL | stream predicate ~ failed |
| production_receipt/production_failure_receipt_is_canonical_redacted_and_tier_stable.case | - | ERROR | first-entry: not found during read `<case work dir>/tier-0/.jet/reports`: No such file or directory (os error 2) |
| services_runtime/source_worker_slot_keeps_checked_function_item_and_moves_the_mailbox.case | - | PASS |  |
| single_use/aliased_is_e0142.case | - | PASS |  |
| single_use/both_branches_consume_is_clean.case | - | PASS |  |
| single_use/consumed_once_compiles.case | - | PASS |  |
| single_use/drop_erases_no_unsafe_in_codegen.case | - | PASS |  |
| single_use/drop_inside_unsafe_block_satisfies_single_use.case | - | PASS |  |
| single_use/drop_inside_unsafe_fn_satisfies_single_use.case | - | PASS |  |
| single_use/drop_outside_unsafe_is_e0143.case | - | PASS |  |
| single_use/dropped_is_e0140.case | - | PASS |  |
| single_use/one_branch_is_e0141.case | - | PASS |  |
| single_use/passthrough_moves_the_duty.case | - | PASS |  |
| single_use/returned_value_is_consumed.case | - | PASS |  |
| single_use/reuse_after_drop_is_e0121.case | - | PASS |  |
| single_use/single_use_enum_dropped_is_e0140.case | - | PASS |  |
| single_use/tag_erases_in_codegen.case | - | PASS |  |
| single_use/task_handles_owe_join_or_detach.case | - | PASS |  |
| single_use/used_twice_is_e0121.case | - | PASS |  |
| single_use/user_drop_fn_shadows_builtin.case | - | PASS |  |
| testing_tiers/doctests_compare_renderings_and_update_claims.case | - | PASS |  |
| ui_fixes/repeated_list_head_fix_preserves_behavior.case | - | KNOWN-FAILING | #5454: `jet fix` does not apply the L0523 repeated list head edit (`LineItem{` stays, `[LineItem]{` expected) (#5575) |

## Small probes

PROBES.md lists every probe under `probes/` with its command, exit code and output on `jet run` and, where it applies, `jet run --release`.

## Static checks on dev 0d1ea12f79

- Core dispatcher rows (Compiler/Prelude/Core.jet `owns {…}` against all exported rows): 1,150 of 1,868 rows are owned by a Jet source module, across 78 modules. Examples: core.text 120/120, core.db 10/10, core.data 57/58, core.encoding 69/86, core.net 81/136, core.http 21/25, core.crypto 29/43 (crypto 5, vault 24), core.ui 19/40.
- `CoreCallSymbol.Rust` rows: 241. The tree still holds 64 `.rs` files (fixtures and data, not built).
- Parity ledger Docs/audits/rust-exit-parity/VERIFIED.md: 42,909 items. PORTED 16,428, PARTIAL 1,215, PORTED-UNMERGED 1,192, DRAFT-ONLY 2,087, ABSENT 18,345, OBSOLETE 3,642. It does not cover Jetpack.
- Diagnostic ratchet Tools/agent/diagnostic-parity-ratchet.json lists 138 `rustOnly` codes. 57 of them are stale because Jet now emits them. 81 have no Jet emit site.
- Codes registered but never emitted by Jet source (searched in Compiler/, Core/, Jetpack/, Tools/; dictionary text not counted): L0505 L0506 L0508 L0510 L0515 L0518 L0519 L0520 L0524 L0525 L0526 L0527 L0528 L0302 L1141 L0601 L2421 E0068 E0160 E0161 E0162 E0163 E0982 E0983 E1241 E2937.
- Codes emitted but with no ui, case or golden fixture: L0501 L0502 L0509 L0511 L0512 L0531 L0207 L2401 L0303 E3105 E3106.
- Compiler/JetCli/Tests: `jet check` exits 0 with warnings only (32 EmptyTest, 16 PositionalBoolParameter, 7 BranchArmTable, 2 UnusedImport, 1 FloatComparison). `jet test` there exits 1: 49 passed, 17 failed. The failures include "the retired report root stays retired", "Rust canonical comparison identity", "set JET_CLI_FMT_TEST_ROOT to disk-backed scratch", "UTF-8 name", "curated related diagnostic" and a slice panic "can't slice 39 characters from 47 to 61".
- `bash Jetpack/Bootstrap/check.sh --area Publication --tests` exits 1 at once: "the Jetpack compiler slice drifted from the compiler's packages". The slice lacks Compiler/JetFoundation/Source/Registry/RegistrationFacts.jet (COMPILER_BUILD_CPU_CACHE_LINE_GENERIC, used by Parser/DeclarationMarkers.jet), Parser/Declarations/Effects.jet and Parser/Declarations/InBodyMarkers.jet.
- `jet add textkit`, `jet fetch` and `jet build --locked` each exit 2 with jet.InvalidCommandInput "`jet <cmd>` is not built yet" (E2104). probes/c3590_cli.sh reproduces this.
- A loose script with `use textkit#1.4` stops with jet.UnresolvedInlineDependency (E1253) (probes/c3590_9_loose_dep.jet).
- Tools/agent/check-core-surface-ledger.mjs reads decisions only from Tools/tower/.tower/tower.json. D-STR-DECLINE1 is ratified but archived in history.json, so the tool reports "declined row names an unknown decision".
- Tools/agent/s5/s5-fixtures.mjs:116 still marks freeze-age-fixtures as `missingOracle: true`.
- Compiler/JetCli/Source/Cli/SelfMaintenance.jet:82-100 (`jet self devtools check-fixture-paths`) still collects only `tests/**/*.rs`.

## New defects with no card (found by this audit)

1. Use after free: `fn first_two(xs: [Int]) -> [Int] { xs[0..2] }` called on a temporary (`first_two([7, 8, 9])`) is accepted and prints a freed value, such as `[7, 1400…]`, on `jet run` and `--release` (probes/c4717_81_temporary_refused.jet).
2. o2 only: errors/stack_overflow prints "Stop [E3012]: Stack overflow in `??`" and loses the `--> stack_overflow.jet:2 in recurse` frame.
3. o2 only: memory/cache_line_padding stops with jet.ReservedPackedLayout (E1105) on `align($build.cpu.cache_line)`.
4. o2 only: memory/stream_store stops with jet.UnknownCoreItem (E1004): "`core.mem` has no item `stream_store`".
5. Testkit: io/path fails on every tier because the testkit runs a staged copy, and `Path.from("Examples/features/io").walk()` is relative to the working directory. The discovery example run passes it.
6. Testkit refuses tests/run/ownership/runtime_disjoint_proof_reports_bounds_and_duplicates_before_mutation.jet and its `_2` twin: "line 4: invalid stream assertion or missing partial reason".
7. Writing a captured value after a copying lambda captured it gives no warning (probes/c4717_75_write_after_lambda_copy.jet): `jet check` is clean, and the run prints 1 then 2.
8. The ui suite is far from green on dev: 290 snapshot mismatches (225 with a different diagnostic code), 15 lint fixtures that emit no lint, 11 fixtures with no expected stream, and 33 fixtures that need a missing CLI fixture API. Only 114 rows carry a known-failing card.
9. check-core-surface-ledger.mjs does not read history.json (see above).
10. Compiler/JetCli/Tests: 17 of 66 `jet test` tests fail on dev (see the static checks above).
11. Comptime type-checks an FFI(asm) body that its closure reaches on an untaken branch and fails with E0114 (probes/c4599_28_comptime_asm_reach.jet; tracked by #4599 c28).

