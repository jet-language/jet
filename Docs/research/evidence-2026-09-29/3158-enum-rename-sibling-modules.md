# #3158 — enum variant rename across sibling modules (criteria 1, 4, 5)

Closer07, 2026-09-29. Binary: `jet-debug-snapshot14` (sha256 prefix `00b1e35e25ed941c`), run through `safe-jet.sh self lsp`.

## Question
Through the real LSP, does renaming an enum variant change every intended reference, while leaving these unchanged?
- a same-spelled variant of another enum in a sibling module;
- a `#Rename("Fast")` wire name;
- a `"Fast"` string literal.

## Fixture (`~/.cache/jet-test-scratch/Closer07/lsp/probe/`)
- `modes.jet`:
  - `pub enum Mode { Fast Slow }`
  - `pub fn fast() -> Mode { Mode.Fast }`
  - `is_fast`, which matches `.Fast` and `Mode.Slow`.
- `speeds.jet` (sibling module):
  - `pub enum Velocity { #Rename("Fast") Fast Slow }`
  - `fast() -> Velocity { Velocity.Fast }`
  - `speed_label`, which matches `.Fast` (returns `"Fast"`) and `Velocity.Slow`.
- `run.jet`: `use "modes"`, `use "speeds"`, and calls into both modules.

`safe-jet.sh run run.jet` prints `true` / `Fast` (exit 0).

## Method
`python3 ~/.cache/jet-test-scratch/Closer07/lsp/drive.py` does the following:
1. `initialize`, then `didOpen` for all three files.
2. At the `Fast` declaration in `modes.jet` (1:4): `prepareRename`, `references`, and `rename` → `Quick`.
3. `definition` from the `.Fast` pattern in `modes.jet`.
4. `references` on `Velocity.Fast` in `speeds.jet`.

Raw responses: `~/.cache/jet-test-scratch/Closer07/lsp/result.json`.

## Evidence
- `prepareRename` returns range 1:4-1:8 with placeholder `Fast`.
- `definition` from `.Fast` (modes.jet 9:9) resolves to `modes.jet` 1:4-1:8.
- `references` returns exactly three locations, all in modes.jet: 1:4 (declaration), 5:29 (`Mode.Fast` constructor) and 9:9 (`.Fast` pattern).
- `rename` → `documentChanges` holds only `modes.jet` v1, with the same three edits to `Quick`.
  - speeds.jet is untouched: `Velocity.Fast`, `#Rename("Fast")`, the `.Fast` pattern and the `"Fast"` literal are all unchanged.
  - The response's `semantic_ops` is `[{"kind":"rename","rule_id":"lsp.rename","from":"Fast","to":"Quick","targets":[],"files":[]}]`. Its `targets` and `files` are empty even though the edit touches one file.
- `references` on `Velocity.Fast` returns speeds.jet 1:20, 5:37 and 9:9 only. It never crosses into modes.jet.
- Diagnostics: only the lints L0513 and L0530 in modes.jet and speeds.jet; none in run.jet.

## What could not be exercised: cross-module variant references
The card fixture puts `.Fast` / `Mode.Slow` in a module other than the declaring one. The current binary cannot express that:
- `use "modes"` plus `fn describe(mode: modes.Mode)` with a `.Fast` arm → **E0305** "Pattern `Fast` doesn't match this value's type — Why: `.::modes.jet::Mode` is a struct, not an enum".
  - The same error appears for a value returned by `modes.fast()` compared with `if m == .Fast` (`~/.cache/jet-test-scratch/Closer07/lsp/probe` variant, recorded in this session).
- `modes.Mode.Fast` / `modes.Mode.Slow` in an expression or pattern → **E0107** "Nothing named `modes` exists here".
- `use modes.Mode` → **E0357** (alias must be snake_case) plus **E0611** "`Mode` is not defined in module `modes`".

So no sibling-module variant reference can exist in a program that checks, and the cross-module half of criteria 1/4/5 is blocked by these checker defects. It is not blocked by the LSP.

A separate ICE was found while building the fixture:
- A sibling file module `speeds.jet` declaring `pub enum Speed { … }`, imported with `use "speeds"`, fails on both `run` and `run --interpret` with `internal compiler error: checked TIR cannot lower to MIR at 13196..14575: duplicate checked type definition `.::speeds.jet::Speed``.
- Renaming the enum to `Velocity` (no other change) compiles and runs.
- `Speed` does not appear in `Core/` (grep).
- Repro, as left on disk: `~/.cache/jet-test-scratch/Closer07/lsp/probe2/`. There `speeds.jet` has `pub enum Speed { Fast Slow }`, `fast()` and `speed_label()`, and `run.jet` is `use "speeds"` with `fn run() { print(speeds.speed_label(speeds.fast())) }`. Run `safe-jet.sh run run.jet` → ICE (exit 101).

## Verdict
PARTIAL:
- Same-module rename through the real LSP in a multi-file workspace is precise, and it leaves the unrelated same-spelled variant, the `#Rename` wire name and the string literal unchanged.
- Cross-module rename (criteria 1/4) cannot be exercised, because the checker rejects every sibling-module variant reference form.
- Criterion 5's `lsp_enum_variant_rename_crosses_sibling_modules` does not exist in `tests/lsp.rs` (grep: only `lsp_enum_variant_navigation_completion_and_rename`).

## Follow-ups (defects for Pip)
1. E0305: a leading-dot pattern on a value of an enum type declared in a sibling file module is rejected with "is a struct, not an enum".
2. E0107: `module_alias.Enum.Variant` is not resolvable in expression or pattern position.
3. ICE: duplicate checked type definition for `pub enum Speed` in `speeds.jet`.
4. Minor: the rename response's `semantic_ops[0].targets`/`files` are empty although edits are produced.
