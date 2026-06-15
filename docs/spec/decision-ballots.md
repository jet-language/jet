# Decision ballots (owner's queue)

Open syntax decisions awaiting the owner. **Ratified choices live only in
docs/spec/syntax-decisions.md** (and, for milestone-scoped IDs, in the relevant
plan under docs/plans/) — when the owner decides, agents add the row there and
remove it from this file. This file is the *pending queue only*; it never
duplicates ratified content.

Decide one group at a time. A group must be fully decided before its milestone
starts (plans in docs/plans/ are blocked on these IDs).

---

## Group 12 — E2-M18 REPL *(open — see docs/plans/epoch-2/m18-repl.md)*

Interactive `jet repl` is planned for Epoch 2 as **E2-M18**, after the E2-M4
interpreter ships. No code until every ID below is ratified in
docs/spec/syntax-decisions.md (or deferred with a recorded default in the
plan). Recommendations are in the plan file.

| ID | Question (one line) | Rec |
|---|---|---|
| D-REPL1 | Ship terminal REPL in Epoch 2? | **A** — E2-M18 after E2-M4 |
| D-REPL2 | Web playground in this milestone? | **A** — terminal only |
| D-REPL3 | Entry: `jet repl` only vs bare `jet` in TTY vs seed file | **A** — `jet repl` only |
| D-REPL4 | Backend: interpreter vs compile-each vs hybrid | **A** — interpreter |
| D-REPL5 | Input: stmts vs full decls vs expressions only | **A** — stmts + control flow |
| D-REPL6 | Reject FFI/tasks/low-level vs also package imports | **A** — reject native-only set |
| D-REPL7 | Session: accumulating module vs cells vs both | **C** — accumulating + optional `:cell` |
| D-REPL8 | Ownership across lines: real moves vs auto-clone vs borrow-only | **A** — real move semantics |
| D-REPL9 | Multi-line: brace-count prompt vs `;` submit vs single-line | **A** — brace-count + `...` |
| D-REPL10 | Project context: sandbox vs auto `jet.toml` vs always sandbox | **A** — sandbox + `--project` |
| D-REPL11 | Line editor: std-only vs crate vs crate+completion | **B** — line-editing crate |
| D-REPL12 | vs `jet eval --pure`: separate vs `--pure` mode vs no REPL | **A** — separate commands |
| D-REPL13 | vs `jet dev`: independent vs flag vs shared process | **A** — share library only |
| D-REPL14 | Native snippet: reject vs temp compile-run | **A** — reject with workaround |
| D-REPL15 | Meta-commands: minimal vs +load/type/help vs +doc/imports/emit | **B** — +`:load` `:type` `:help` |
| D-REPL16 | Results: implicit echo vs type+value vs print-only | **A** — implicit echo, `;` suppresses |
| D-REPL17 | Diagnostics: identical vs shorter vs session context | **A** — identical to batch |
| D-REPL18 | Crate if D-REPL11≠A: rustyline vs reedline vs other | **A** — `rustyline` (I6) |
| D-REPL19 | Playground arch (if D-REPL2≠A): external vs in-binary vs defer | **C** — defer |
| D-REPL20 | Tests: transcripts vs +PTY vs manual only | **A** — transcript fixtures |
| D-REPL21 | Timing: separate M18 vs thin REPL in M4 vs Epoch 3 | **A** — separate E2-M18 |

Open follow-ups (not ballot IDs yet): interpreter fuel/timeout per input,
startup banner, color policy, implicit `import std` — see m18-repl.md § Open
questions.

---

## Tally sheet (open only)

| Group              | IDs        | Needed by | Status |
| ------------------ | ---------- | --------- | ------ |
| 12 — E2-M18 REPL   | D-REPL1…21 | E2-M18    | ☐      |
| — (deferred)       | S56        | post-1.0  | ☐      |

---

## Already ratified (recorded elsewhere — do not re-list here)

Groups 1–11 are decided. Their content lives in the canonical sources, not in
this queue:

- **Groups 1–8** (S26–S64) — docs/spec/syntax-decisions.md.
- **Group 9** — D-PM1…8 — docs/plans/epoch-1/m12-packages.md.
- **Group 10** — D-LSP1…13 — docs/plans/epoch-1/m13-lsp.md.
- **Group 11** — D-JPK1…15 — docs/spec/syntax-decisions.md (plan:
  docs/plans/jetpack-jetos/README.md).
- **Group 13** — D-SG1…9 (syntax gallery) — docs/spec/syntax-decisions.md
  (S24/S22/S35/S42 amendments + S68–S74). Decided 2026-06-15; D-SG6 = option C
  (`??`, retire `or`), rest per recommendations. Implementation tracked in the
  syntax-gallery Resolved log.
