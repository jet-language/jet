# Snapshots and goldens

Load this branch only when the acceptance criterion changes a diagnostic,
rendered UI snapshot, or executable example output. Preserve the canonical
source and the complete diff; blessing is not a way to hide a red behavior.

## Diagnostic source and snapshot

- Keep the row in
  `Compiler/Prelude/Diagnostics.jet`; the registry and consumers
  project that row. Use `Docs/spec/diagnostics.md` for the What/Why/Fix,
  structured-fix, coverage, and cross-tier contract.
- Keep the matching fixture and snapshot under `tests/ui/` or `tests/ui_lint/`.
  The report must retain the actionable span, registered code, structured
  fields, and no raw backend error. Check `jet explain <CODE>` on the same
  canonical row.
- Run the focused test without an update variable first and read its complete
  diff. If the reviewed change requires an update, preview and update only the
  named target, then inspect `git diff` immediately for unrelated churn. Run
  the focused test again with no update variable.

For example, a single UI fixture uses its repository-relative filter, and
`-u` blesses exactly the rows that filter selects:

```sh
jet test Compiler --profile ui tests/ui/arg_type_mismatch.jet
jet test Compiler --profile ui tests/ui/arg_type_mismatch.jet -u
```

Use the admitted native compiler named by `JET` before any `jet explain`,
runtime, or generated-page claim; then bless only the named target after its
review. Do not create a Markdown diagnostic catalog or
status mirror.

## Executable goldens

Use `Docs/spec/contributing/examples.md` and the relevant example under
`Examples/`. Run the exact example criterion through a fresh binary; when the
criterion applies to both execution forms, prove `jet run` and
`jet run --release`, then update only its matching file under
`Examples/`. Preserve the program's meaning and path
consumers rather than changing a golden to make a regression disappear.

A snapshot or golden is green only after its named focused command actually
runs and its complete output is reviewed. Other tiers remain unknown until
exercised.
