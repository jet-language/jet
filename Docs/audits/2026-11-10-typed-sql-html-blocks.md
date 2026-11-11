# Typed SQL and HTML blocks: behavior → source → proof (2026-11-10)

Card #6622 holds three delivery slices from the old #698 umbrella: the fixed-whitelist block path
(#698), the typed SQL vertical (#1050) and the typed HTML vertical (#1051). The reopen finding
said SQL and HTML blocks were ordinary statement wrappers, and that an undeclared SQL row type
still compiled. This audit maps each behavior named by those slices and by their ratified
decisions to the production code that implements it and to the test that proves it. Every row
was run with the self-hosted compiler; the run is listed at the end.

Decisions: D-TYPEDTEXT1, D-TYPEDTEXT2, D-UNIFYLIT1=A, D-BOUND-RAW1=A (Docs/spec/syntax-decisions.md
"Typed text"), D-DSLBLOCK1=A, and D-SQL-ARG1=B / D-HTML-NAME1=B (M03 marker ballot migration).

## Typed text values (`SQL{"…"}`, `HTML{"…"}`)

| # | Behavior | Source | Proof |
|---|---|---|---|
| T1 | `SQL{"…"}` elaborates to a checked `SQL` value; each hole is an ordered bound value, and `.template()`/`.params()` read it back | Core/Db/Db.jet `SQL`; Compiler/JetSema/Source/Sema/Expressions/Constructors.jet typed heads | cases checked_text/typed_sql_and_html_blocks_run_on_every_tier (`?` placeholders, 3 params); cases dsl_blocks/typed_text_forms_cover_sql_html_sh_and_audited_raw |
| T2 | An ordinary HTML hole is escaped; a hole already typed `HTML` composes without a second escape; `.text()` reads it back | Core/Text/Html.jet `HTML` | cases checked_text/typed_sql_and_html_blocks_run_on_every_tier; cases checked_text/html_composition_matches_aot_jit_interpreter_and_web |
| T3 | A runtime `String` reaching a database sink is E0149, and the fix's `SQL{"…"}` spelling parses | Compiler/JetSema/Source/Sema/Diagnostics.jet E0149 | ui db_typed_sql_plain_string, db_typed_sql_all_sinks_plain_string, db_typed_sql_migrations_plain_string, typed_text_runtime_string |
| T4 | `SQL.raw`/`HTML.raw` are the only unchecked escapes, and they need `#Unsafe` | Core/Db/Db.jet, Core/Text/Html.jet `raw` | ui checked_text_raw_requires_unsafe; cases checked_text/checked_text_raw_requires_unsafe; ui taint/origin_marked_text_rejects_raw_sinks |
| T5 | Hole-free bodies elaborate under a typed head | Constructors.jet | cases checked_text/typed_sql_and_html_blocks_run_on_every_tier (`SQL{"select 1"}`) |
| T6 | Domain text is `SQL`/`HTML`/`Sh{"…"}` only; the body owns its backslashes | Constructors.jet; Compiler/JetLexer typed-head bodies | cases dsl_blocks/typed_text_forms_cover_sql_html_sh_and_audited_raw; cases dsl_blocks/regex_typed_head_owns_backslashes_but_rejects_holes |
| T7 | Malformed holes and block text fail at their own span, without a recovery cascade | Compiler/JetParser/Source/Parser/State.jet hole recovery; Constructors.jet E0112 | ui typed_text_hole_malformed, typed_text_block_errors |
| T8 | The same typed-head form covers boundary values: `URL{"…"}`, `Path{"…"}` and `DateTime{"…"}` holes work where the type allows them, and an invalid or interpolated literal is E0155 at compile time | Constructors.jet typed heads (E0155) | cases dsl_blocks/boundary_typed_heads_validate_and_compile |

## Checked text blocks (`#SQL(Row) { … }`, `#HTML { … }`)

| # | Behavior | Source | Proof |
|---|---|---|---|
| B1 | Blocks come from a fixed whitelist declared in `Prelude/Markers.jet` and read through the shared registry; an undeclared block is one E0927 | Compiler/JetFoundation/Data/Syntax/CoreSurface.jet `DSL_BLOCK_SQL`; Compiler/JetSema/Source/Sema/MarkerRules.jet E0927 | cases dsl_blocks/undeclared_checked_text_block_reports_one_registered_error; ui typed_text_block_errors (`#Sql`, `#CSS`) |
| B2 | The SQL row type is an ordinary argument, `#SQL(Row) { … }`, and it is a real type position: an unknown row is E0119 | Compiler/JetSema/Source/Sema/Statements.jet DSL header check | ui dsl_blocks/sql_row_header_is_a_real_declared_type_position; cases dsl_blocks/sql_and_html_blocks_compile_and_formatter_round_trip |
| B3 | A `#SQL` header that does not match its registered signature `SQL(row: Type{none})` is one E0930; `#HTML` takes no argument at a block site (E0617, whose fix names `#HTML { … }`) | Compiler/JetSema/Source/Sema/MarkerRules.jet signature check; Sema/Statements.jet E0617 | ui dsl_blocks/dsl_block_header_is_reported_once; ui scope_member_html_block_takes_no_argument |
| B4 | `#HTML(path)` at file sites takes a typed `Path`; bare `#HTML { … }` is the block form | Prelude/Markers.jet `HTML` rows | cases marker_rule_signatures/html_marker_uses_typed_path_signature; cases dsl_blocks/sql_and_html_blocks_compile_and_formatter_round_trip |
| B5 | A block body keeps normal sema and registry rules, and the block receives its whole nested region | Parser/Statements.jet `Stmt.ScopeMember{dsl: true}` | ui dsl_blocks/dsl_block_body_keeps_normal_sema_and_registry_rules, …_2; cases dsl_blocks/declared_block_receives_the_whole_nested_region |
| B6 | The formatter keeps both block heads and is stable on its own output | Compiler/JetFormat | cases dsl_blocks/sql_and_html_blocks_compile_and_formatter_round_trip; cases checked_text/typed_sql_and_html_blocks_run_on_every_tier (`jet fmt --check`) |
| B7 | Blocks and typed values run the same at default, O0, O1, O2 and release | the production backend | cases checked_text/typed_sql_and_html_blocks_run_on_every_tier; cases checked_text/checked_text_acceptance_matches_comptime_aot_jit_interpreter_and_web |
| B8 | Typed SQL keeps database policy and provenance on every tier | Core/Db | cases db_policy/typed_sql_canonical_scope_example_has_aot_default_and_interpreter_parity, typed_sql_hostile_operations_keep_policy_and_provenance_across_tiers |

## Run

All rows were run on 2026-11-10 with the self-hosted compiler, built one-stage from dev `76955986d0`
plus this change with `--threads 8`:

- Every ui and cases row named in a Proof column above: PASS (13 ui rows, 13 cases rows).
- `jet test Compiler --profile ui --filter dsl_blocks`: 5 PASS. `--profile cases` with `--filter`
  `dsl_blocks`: 6 PASS; `typed_sql`: 4 PASS; `html`: 6 PASS; `checked_sql`: 2 PASS.
- `jet test Compiler --profile ui` (full): 2395 PASS and 102 known-failing. The 1 FAIL
  (env_facet_missing) and 2 ERROR rows (locked_effect_row_widened, hangar_verify_digest_mismatch)
  need an installed jetpack, which the run host does not have.
- `jet fmt --check` on the changed files, DiagnosticRows, CoreSourceRows, RuntimeGenerators,
  UnsafeRatchet, LexicalLedger and DecisionIndex `--check`, and `jet self devtools grammars --check`:
  exit 0.
- typed_text_forms_cover_sql_html_sh_and_audited_raw and boundary_typed_heads_validate_and_compile
  were known-failing because their fixtures used the retired `Head.{"…"}` head. Their fixtures now
  use `Head{"…"}`, both pass, and their known-failing rows are removed.
