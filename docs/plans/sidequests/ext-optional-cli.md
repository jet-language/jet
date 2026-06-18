# Extension-optional CLI

**Status: done 2026-06-18.** `resolve_source_path` in `src/main.rs` applied to
`run`/`build`/`check`/`eval`; tests in `tests/cli.rs`
(`ext_optional_check_resolves_dot_jet`, `ext_optional_run_resolves_dot_jet`,
`ext_optional_missing_path_keeps_original_name`).

No syntax decision needed. Pure CLI behavior.

## Goal

`jet run examples/test` resolves to `examples/test.jet` if the literal path
doesn't exist as a file. Applies to all path-accepting subcommands.

## Tasks

1. `src/main.rs`: extract `fn resolve_source_path(raw: &str) -> PathBuf`. If
   `raw` exists as-is, return it. Otherwise try `raw + ".jet"`. If that exists,
   return it. If neither, return `raw` unchanged so the normal "file not found"
   error fires with the original name.
2. Apply to: `run`, `build`, `check`, `eval`. Not to `new` or `repl` (no path
   argument).
3. No new diagnostics needed — if neither path exists the existing
   file-not-found error is correct.
4. Tests: add a test case that calls `jet run` with a no-extension path pointing
   at an existing `.jet` file.
