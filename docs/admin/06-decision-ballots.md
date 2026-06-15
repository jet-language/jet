# 06 — Decision ballots (owner's queue)

Open syntax decisions for M3–M14. **Ratified choices live only in
docs/02-syntax-decisions.md** — when you decide, agents add the row there
and remove it from this file.

Decide one group at a time. A group must be fully decided before its
milestone starts (plans in docs/plans/ are blocked on these IDs).

---

## Group 7 — Platform *(ratified 2026-06-12 — see docs/02)*

*(S51 ratified 2026-06-12, amended 2026-06-13 — see docs/02: std is
exported as the `std` module (no quotes), short for canonical `jet.std`;
`import std`, `import jet.std as std`, `import std.fs as fs`, `import
std.io`. S16: quotes = file path (`import "./lib"`), no quotes = module.)*

*(S54 ratified 2026-06-12 — see docs/02: no prescribed naming convention in
v1; `jet fmt` layout only.)*

*(S53 ratified 2026-06-12 — see docs/02: concurrency deferred to v2; when
built, option A — `tasks.spawn`, channels, no shared mutable state.)*

*(S52 ratified 2026-06-12, amended 2026-06-13 — see docs/02 and
docs/plans/epoch-1/m12-packages.md: `jet.toml`, graph `jet.lock`,
`[dependencies:*]` colon tables, `@latest`, `.jet/` source root,
`jet add`/`fetch`/`update`; single files manifest-free forever.)*

---

## Group 8 — Post-1.0 horizon *(owner direction 2026-06-12)*

*(S58 ratified 2026-06-12 — see docs/02: `std/mem` discovery gate +
`unsafe` audit gate, Zig-style allocators, sema-gated `&`/`*`.)*

*(S59 ratified 2026-06-12 — see docs/02: C FFI deferred to v2; when built,
option A — `extern c` blocks mirroring S50.)*

*(S60 ratified 2026-06-12 — see docs/02: `pure fn name(…)` checked modifier;
purity in the signature; enables `jet eval --pure`.)*

*(S61 ratified 2026-06-12 — see docs/02: optional argument labels,
positional order fixed, trailing default values.)*

*(S62 ratified 2026-06-12 — see docs/02: Kotlin-style trait delegation
`impl Trait using field;`.)*

*(S63 ratified 2026-06-12 — see docs/02: RAII scope-end cleanup as the
one story; `defer` noted as a possible later complement.)*

---

## Group 9 — M12 package architecture *(ratified 2026-06-13 — see docs/plans/epoch-1/m12-packages.md)*

*(D-PM1…8 ratified 2026-06-13 — see m12 plan: Nix-style store from M12.1;
store path `~/.jet/store/<name>-<version>-<full-fingerprint>/`; `jet.toml`
only for manifest; exact pins M12.1, ranges+resolver M12.2; all M12.1 in
`jet`. Amended 2026-06-15: the jetpack binary/environment engine, reached
directly through `jetpack run/build/list/clean/add/remove` during Phase 1, is a
separate owner-gated track and not part of M12's Jet source-library workflow.)*

---

## Group 10 — M13 LSP *(ratified 2026-06-14 — see docs/plans/epoch-1/m13-lsp.md)*

*(D-LSP1…13 ratified 2026-06-14 — all recommendations accepted as written:
D-LSP1 `jet lsp` subcommand; D-LSP2 full error recovery; D-LSP3 debounced
~200ms via dirty-flag flush; D-LSP4 file-granular re-parse; D-LSP5 type-aware
completion + switch snippet + auto-import; D-LSP6 type + ownership + doc-comment
hover; D-LSP7 structured edits shared with CLI `jet fix`; D-LSP8 inlay hints
off by default except clone hint; D-LSP9 near-zero settings; D-LSP10 strict
standard LSP; D-LSP11 crash-proof handlers + `jet lsp doctor`; D-LSP12 fixture
+ transcript + latency bench in CI; D-LSP13 code lens deferred to `jet dev`.)*

---

## Group 11 — Jetpack & JetOS product split *(ratified 2026-06-15 — see docs/02)*

Source plan: `docs/plans/jetpack-jetos/README.md`. These decisions gate any
implementation of the new user-facing `jetpack` CLI or any cleanup that would
rename public commands. They amend the earlier M12 assumption by treating
existing `jet add/remove` as transitional commands that may later plumb to
Jetpack; until then, M12's source-library behavior remains documented.

Ratified in docs/admin/02-syntax-decisions.md:

- D-JPK1: build `jetpack` as an independent package-manager binary/engine first.
  Later `jet run github:...` can delegate to jetpack, but that plumbing is not
  part of Phase 1.
- D-JPK2: `jetpack run/build/list/clean/add/remove` are the Phase 1 command
  surface.
- D-JPK3: Phase 1 uses directive-style pack-file syntax.
- D-JPK4: `jetpack add/remove` own Phase 1 package/environment edits; existing
  `jet add/remove` can later plumb to Jetpack.
- D-JPK5: Jetpack owns packages; Nix is a compatibility provider translated by
  Jetpack, not the package manager.
- D-JPK6: Forge was salvaged into `docs/plans/jetpack-jetos/forge-salvage.md`
  and removed.
- D-JPK7: Jetpack is next; refs use `<source>:<package/path-to-package>`, e.g.
  `nixpkgs:fastfetch`.
- D-JPK8: Jet has a root pack file with the role of `flake.nix`; filenames are
  `pack.jet` and `pack.lock`.
- D-JPK9: direct `jetpack ...` commands are the Phase 1 surface.
- D-JPK11: `pack.jet` first; `flake.nix` fallback is translated by Jetpack.
- D-JPK12: system roots are `/etc/jet/` and `/etc/jet/store/`.
- D-JPK13: root pack file is `pack.jet`; lockfile is `pack.lock`.
- D-JPK14: prompt support covers bash/fish/zsh, default label `jetpack`.
- D-JPK15: Nix compatibility uses `<source>:<package/path>`, not `#`.

Amended implementation stance: while building Jetpack, do not implement the
`jet` delegation layer yet. Use `jetpack run/build/list/clean/add/remove`.

---

## Group 12 — E2-M18 REPL *(open — see docs/plans/epoch-2/m18-repl.md)*

Interactive `jet repl` is planned for Epoch 2 as **E2-M18**, after the E2-M4
interpreter ships. No code until every ID below is ratified in
docs/admin/02-syntax-decisions.md (or deferred with a recorded default in the
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


| Group              | IDs  | Needed by | Status |
| ------------------ | ---- | --------- | ------ |
| 12 — E2-M18 REPL   | D-REPL1…21 | E2-M18 | ☐ |
| — (deferred)       | S56  | post-1.0  | ☐      |


Ratified (see docs/02): Group 1 confirmations; Group 2 — S29–S33; Group 3 —
S34–S36; Group 4 — S37–S42; Group 5 — S43 S44 S49 S50; Group 6 — S26 S28
S45 S48 S46 S47 S55 S57; Group 7 — S51 S52 S53 S54; Group 8 — S58 S59 S60
S61 S62 S63 S64; Group 9 — D-PM1…8 (see docs/plans/epoch-1/m12-packages.md);
Group 10 — D-LSP1…13 (see docs/plans/epoch-1/m13-lsp.md); Group 11 —
D-JPK1…15 (see docs/plans/jetpack-jetos/README.md).
