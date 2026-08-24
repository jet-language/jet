---
title: Beginner onboarding capstone and first-success evidence — 2026-08-24
---

# Beginner onboarding capstone and first-success evidence

## Result

Card #1037 has one executable beginner path: install on a supported host,
create a project, resolve its `run.jet`, run it, edit it, check it, test it,
and recover from the named entry and source failures. The path ends with a
golden-tested capstone rather than the first `hello, world` alone.

This report records production CLI evidence. It does not claim install or
network latency: those depend on the host, Nix cache, and connection.

## Capstone

The first-hour capstone is
`examples/features/basics/first_hour.jet`. Its expected output is checked by
the golden harness and its expert counterpart is
`examples/features/basics/first_hour_expert.jet`.

The capstone command is:

```text
jet run examples/features/basics/first_hour.jet
```

Observed output:

```text
Shipping first-hour
[ok] check source
[ok] build binary
[ok] run smoke test
```

Both the beginner and expert capstone sources produce this output through the
real `jet run` path.

## Measured first success

The measurement used the local `Jet 1.0.0` binary and three clean temporary
projects under the repository test scratch directory. Each sample ran
`jet new <name>`, then bare `jet run` from the created project. The output was
exactly `hello, world` followed by a newline in every sample.

| sample | `jet new` | bare `jet run` |
|---:|---:|---:|
| 1 | 21 ms | 132 ms |
| 2 | 21 ms | 122 ms |
| 3 | 20 ms | 123 ms |
| median | **21 ms** | **123 ms** |

The paired end-to-end samples were 153 ms, 143 ms, and 143 ms; the median
sample completed in **143 ms**. These numbers measure the local compiled CLI
and compiler/runtime cache state, not the network install.

## Production-path proof map

| behavior | implementation and proof |
|---|---|
| scaffold creates the canonical files and entry | `Source/CmdCompile.rs::run_new`; `tests/pkg.rs::cli_jet_new_creates_project_structure` |
| scaffold test is runnable before editing | `Source/CmdCompile.rs::run_new` writes the `#Test` smoke block; fresh `jet test run.jet` reports `the greeting stays stable: pass` |
| bare and explicit entry resolution | `Source/main.rs` resolver; fresh `jet run` and `jet run run.jet` both print `hello, world` |
| edit, invalid source, diagnostic, explain, and recovery | fresh CLI smoke: edited output, E0102 with `Fix:`, `jet explain E0102`, and recovered output |
| missing, ambiguous, legacy, and stale layouts | `tests/pkg.rs::cli_run_migrates_all_retired_entry_layouts`, `cli_run_reports_ambiguous_retired_entry_layout`; fresh missing-entry, E1226, and migration probes |
| capstone output and golden | `examples/features/basics/first_hour.jet`, `first_hour_expert.jet`, and `tests/golden.rs` |

The fresh binary also passed the complete scaffold edit/check/test/diagnose/
explain/recovery sequence and the onboarding example's `check`, `test`, and
`run` commands. The targeted prebuilt package tests passed for scaffold
creation, legacy migration, and ambiguous-entry recovery. A fresh cargo
compile for `tests/onboarding_recovery.rs` was not available because a
sibling-owned change currently fails in
`crates/jet-foundation/src/Syntax/package_files.rs:54-57`.

The beginner source keeps the ratified `run.jet`, `->`, and `#Test` forms. No
new syntax, public dependency, or execution-tier exception is introduced.

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
|---|---|---|
| ONB-001 | no-action | This report records shipped evidence and has no unresolved audit finding. |
<!-- /audit-dispositions -->
