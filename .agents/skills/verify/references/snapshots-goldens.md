# Snapshots and goldens

Load this branch only when the acceptance criterion changes a diagnostic,
rendered UI snapshot, or executable example output. Preserve the canonical
source and the complete diff; blessing is not a way to hide a red behavior.

## Diagnostic source and snapshot

- Keep the row in
  `crates/jet-codegen/src/Prelude/Diagnostics.jet`; the registry and consumers
  project that row. Use `docs/spec/diagnostics.md` for the What/Why/Fix,
  structured-fix, coverage, and cross-tier contract.
- Keep the matching fixture and snapshot under `tests/ui/` or `tests/ui_lint/`.
  The report must retain the actionable span, registered code, structured
  fields, and no raw backend error. Check `jet explain <CODE>` on the same
  canonical row.
- Run the focused test without an update variable first and read its complete
  diff. If the reviewed change requires an update, preview and update only the
  named target, then inspect `git diff` immediately for unrelated churn. Run
  the focused test again with no update variable.

For example, a single UI fixture uses its repository-relative filter:

```sh
scripts/agent/jet-env env JET_UI_FILTER=tests/ui/arg_type_mismatch.jet \
  cargo test --test diagnostic_snapshots ui_snapshots -- --nocapture
scripts/agent/jet-env env JET_UI_FILTER=tests/ui/arg_type_mismatch.jet \
  UPDATE_EXPECT=tests/ui/arg_type_mismatch.jet \
  cargo test --test diagnostic_snapshots ui_snapshots -- --nocapture
scripts/agent/jet-env jet self devtools bless tests/ui/arg_type_mismatch.jet --dry-run
```

Build a fresh binary with `scripts/agent/jet-env cargo build` before any `jet
explain`, runtime, or generated-page claim; then bless only the named target
after its dry run and review. Do not create a Markdown diagnostic catalog or
status mirror.

## Executable goldens

Use `docs/spec/contributing/examples.md` and the relevant example under
`examples/`. Run the exact example criterion through a fresh binary; when the
criterion applies to both execution forms, prove `jet run` and
`jet run --release`, then update only its matching file under
`examples/features/expected/`. Preserve the program's meaning and path
consumers rather than changing a golden to make a regression disappear.

A snapshot or golden is green only after its named focused command actually
runs and its complete output is reviewed. Other tiers remain unknown until
exercised.
