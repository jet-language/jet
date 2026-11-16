# Branch ownership census: Compiler/JetIR

Date: 2026-11-13. Scope: every lowering in `Compiler/JetIR/Source` that builds
user code (an expression, statement or body from the program) inside a branch
it opens itself: an `If` region from `ir_open_if`, or a `Block`/`Loop` region
from `ir_open`.

A move on one path of such a branch leaves the variable owned on the other.
Lowering tracks ownership per binding (`IROwnState`), so a branch that builds
user code must start every path from the ownership it opened with and join
the paths where they meet (`ir_meet_paths`, `ir_close_arms`, `ir_join_owned`,
`ir_loop_join`). Without that, the state one path leaves is taken for all of
them: a value the skipped path still owns is never ended (a leak), or a path
ends a value another path already moved.

## Method

1. Roots: the functions that build user code, `ir_expr`, `ir_expr_slot`,
   `ir_stmts`, `ir_stmt`, `ir_stmt_body`, `ir_stmt_collecting` and
   `ir_value_as`.
2. Callers: every function that reaches a root through at most four calls.
   Deeper levels add only runtime and closure helpers (`ir_runtime`,
   `ir_call_closure`), which build no user code.
3. Regions: in each of those functions, every `ir_open_if` and `ir_open`
   region, and whether a root call (direct or through a caller from step 2)
   falls between the open and its `ir_end`.
4. Each region was classified by reading it.

## Already save and join

| Lowering | File | How |
|---|---|---|
| `ir_condition`, `ir_short_circuit`, `ir_branch_value` (`ir_expr_slot`), `ir_stmt_body`, `ir_range_switch`, `ir_mixed_switch`, `ir_match`, `ir_or_fallback`, `ir_optional_call`, `ir_require_stop`, `ir_text_pair` | BuildExpr.jet | `ir_meet_paths` / `ir_close_arms` |
| `ir_contract` | BuildContracts.jet | `ir_meet_paths` |
| `ir_stop_catches` | Build.jet | `ir_close_arms` |
| `ir_range_loop`, `ir_range_value_loop`, `ir_loop_value`, `ir_list_walk`, `ir_iterable_walk`, `ir_encoding_reader_walk`, `ir_text_walk`, `ir_line_next_walk`, `ir_stdin_lines_walk`, `ir_lazy_walk_source` | Build*.jet | `ir_loop_join` |

## Branch only on a leaving path

The other path never meets the branch's end, so no join is needed there.

| Lowering | Why |
|---|---|
| `ir_refutable_bind` | Sema requires the fallback to leave; `ir_stmts_leave` restores the states for a leaving block. |
| `ir_try_slot`, `ir_try_failure`, `ir_option_map` | The failure path returns or stops. |
| `ir_stop_point`, `ir_sort_key_stop` | Through `ir_stop_return`. |
| `ir_test_main`, `ir_transaction_hook_call` | Through `ir_stop_check`. |

## No user code in the branch

| Lowering | Why |
|---|---|
| `ir_dyn_option_encode`, `ir_dyn_union_encode`, `ir_dyn_encode_slot` | Build a generated encode over a generated local that owns nothing. |
| `ir_scope_member` (`Timeout`, `ExpectFail`) | The body is straight-line; its catches go through the catch frame. |

## Found and fixed

Each is pinned by a case that leaks on the unfixed compiler.

| Lowering | Branch | Fix | Case |
|---|---|---|---|
| `ir_try_from_or` (BuildExpr.jet) | the numeric fallback under the range test | `ir_meet_paths` | `core_files_opaque_handles/numeric_fallback_moves_keep_the_fitting_path_owner` |
| `ir_map_take` (BuildMaps.jet) | the group update fallback under the presence test | `ir_meet_paths` | `core_files_opaque_handles/group_update_fallback_moves_keep_the_present_key_owner` |
| `ir_option_lift2` (BuildRoutes.jet) | the callable, built under both presence tests | a ran flag, `ir_meet_paths` | `core_files_opaque_handles/lift2_callable_moves_keep_the_absent_operand_owner` |
| `ir_trait_call` (BuildTraits.jet) | the arguments, built once per implementation | each arm starts from the same states, `ir_close_arms` | `core_files_opaque_handles/trait_call_argument_moves_keep_one_owner_per_implementation` |
| `ir_contract_return` (BuildContracts.jet) | `#Post` clauses under the payload's presence test | `ir_meet_paths` | `memory_semantics/post_clause_moves_keep_the_failed_return_owner` |
| `ir_pattern_test` (variant payload slot, struct value and nested fields, later alternatives), `ir_list_pattern_test`, `ir_map_pattern_test`, `ir_refine_payload` (BuildExpr.jet) | a pattern value slot (`.Count(f(x.move()))`), built only once the earlier tests matched | `ir_end_tested` (Build.jet): each path sets a live flag for what the slot moved | `core_files_opaque_handles/pattern_value_moves_keep_the_unmatched_path_owner` |

A pattern test rewrites the Bool its `If` tested, so no test is left to join
on after the region: `ir_end_tested` sets the live flags on the region's own
two paths. Every fix builds nothing extra when the branch moves nothing.

## Outside JetIR

`ir_measure_scope` builds a `.measure` body inside a loop that runs it many
times. Sema accepts a move of a variable declared outside that body, so the
second iteration moves a value the first already ended. That is a missing
refusal in sema, not a missing join, and is reported for its own card.
