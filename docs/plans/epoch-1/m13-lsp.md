# M13 — LSP v2: a real language server

**Status:** implemented and verified 2026-06-14. D-LSP1…13 ratified
2026-06-14 (all recommendations accepted — see docs/admin/06-decision-ballots.md Group 10).
Depends on M6 phase 4 (LSP v0 skeleton), M12 (multi-package projects).
**Error codes:** none new (tooling); internal robustness instead.

## Goal

Make Jet feel first-class in an editor. The compiler front end already
owns every fact the server needs; this milestone exposes it with good
latency and never crashes.

**Vision:** The compiler is the server. Zero duplicated language knowledge
(zls is the cautionary tale). Broken code is the normal case — every
capability works mid-keystroke on incomplete programs.

---

## Decisions — ratified 2026-06-14

All recommendations accepted as written.

| ID | Question | Rec |
|---|---|---|
| D-LSP1 | Server location | **A** — `jet lsp` subcommand of the one binary |
| D-LSP2 | Half-typed code | **A** — full error recovery; every feature works mid-keystroke |
| D-LSP3 | Diagnostic cadence | **A** — live, debounced ~200ms, stale work cancelled |
| D-LSP4 | Large projects | **A** — re-parse changed files only; measure before getting clever |
| D-LSP5 | Completion | **A** — type-aware ranking + switch-arm snippet + auto-import |
| D-LSP6 | Hover | **A** — type + ownership in Jet words + doc comment |
| D-LSP7 | Quick fixes | **A** — structured edits from sema, shared with CLI `jet fix` |
| D-LSP8 | Inlay hints | **A** — off by default, except hidden-clone hint |
| D-LSP9 | Configuration | **A** — near-zero settings |
| D-LSP10 | Protocol | **A** — strict standard LSP for v1 |
| D-LSP11 | Server crashes | **A** — crash-proof handlers + `jet lsp doctor` |
| D-LSP12 | Testing | **A** — fixture tests + transcript tests + latency bench in CI |
| D-LSP13 | Code lens / eval | **A** — defer to `jet dev` (post-v1); design foundation now |

Postfix completion (D-LSP5 alt C) and custom protocol extensions (D-LSP10
alt B) would need separate owner ballots if requested post-v1.

---

## Invariants (LSP-I1…LSP-I6)

- **LSP-I1** Server reuses lexer/parser/sema/fmt as libraries; new facts
  get added to sema, never recomputed locally.
- **LSP-I2** Panics in handlers are caught, logged, answered — process
  death is P0 (ICE sibling).
- **LSP-I3** Every request cancellable; no work for stale questions.
- **LSP-I4** Results reflect overlay buffers; diagnostic text byte-identical
  to terminal (same renderer, I4 snapshots bind both).
- **LSP-I5** Every capability has fixture tests on *incomplete* programs.
- **LSP-I6** Latency budgets enforced in CI; regression fails the build.

---

## Implementation order

Each step independently shippable:

1. **`SourceProvider` overlay** (prerequisite; `jet run` byte-identical) +
   structured-fix refactor on `Diagnostic` (D-LSP7 — CLI `jet fix` falls out).
2. **Error-recovering parser** (D-LSP2) — load-bearing; terminal cascades
   improve as side effect.
3. **Debounced live diagnostics + cancellation** (D-LSP3) + file-granular
   incrementality (D-LSP4) + `jet lsp --bench` + fixture/transcript harness
   (D-LSP12).
4. **Completion** (D-LSP5), **hover** (D-LSP6), **go-to-definition /
   references / rename** (span table in sema, built once, reused).
5. **Semantic tokens**; inlay clone hint (D-LSP8); `jet lsp doctor`
   (D-LSP11); tree-sitter + TextMate grammars from `src/syntax.rs`.

**Already shipped (M6 v0):** diagnostics on open/change, formatting, S14
autocorrect code actions, VS Code extension skeleton.

---

## Capabilities (exact scope)

1. **Diagnostics** — per-keystroke debounced; whole-project import graph;
   lints with Hint→Warning mapping.
2. **Completion** — scope names, member after `.`, keywords, import paths;
   switch snippet pre-fills variant arms; type-aware ranking; auto-import.
3. **Hover** — type + ownership ("`words`: `List<String>` — `var`, may be
   changed here") + `///` doc comment (S49, plain text v1).
4. **Go to definition / find references** — span table in sema; cross-file
   and into dependencies (read-only).
5. **Rename** — span table; refuses keywords/builtins; atomic multi-file edit.
6. **Quick fixes** — optional `fix: Vec<(Span, String)>` on `Diagnostic`:
   S14 autocorrects, did-you-mean, "add `take`", missing switch arms,
   "make this `pub`". CLI: `jet fmt --fix` / `jet fix`.
7. **Formatting** — wired to fmt; add range formatting.
8. **Semantic tokens** — keyword, type, function, parameter-with-`mut`, …
9. **Inlay hints** — inferred types on bindings; clone hint at L0201 sites.

---

## Engineering requirements

- **Incrementality v1 = file-granular:** re-parse changed files only; sema
  whole-program. Budget: <100ms diagnostics for 5k lines on a laptop;
  `jet lsp --bench` replays a recorded session in CI.
- **Crash policy:** catch panics, log to file, error response — never die
  mid-session; one `window/showMessage` for ICE-class failures.
- **Unsaved buffers:** all file access via `SourceProvider` trait.
- **Shared foundation:** server process, overlay, incremental front end, and
  crash policy also host future `jet dev` watch mode (roadmap #10). Design as
  reusable library pieces — no dev-mode features in M13 scope.
- **JSON-RPC:** revisit M6 hand-rolled JSON under load; if bottleneck,
  request owner approval for serde_json in tooling binary (I6 protocol).
- **VS Code extension:** binary path config, semantic token theme defaults.
  Ship `editors/jet.tmGrammar` + tree-sitter-jet/ generated from syntax.rs.

---

## Exit criteria

- Scripted LSP integration tests (JSON transcripts in `tests/lsp/`) per
  capability: completion lists, hover text pinned, rename workspace edit,
  switch-arm quick fix inserts compilable code.
- Bench harness passes latency budget in CI.
- Dogfood: write `examples/features/16_wordcount.jet` from scratch in VS Code using
  only completions/quick-fixes — no crash, no stale diagnostics (PR checklist).

---

## Out of scope

Debugger/DAP, workspace symbols fuzzy search, call hierarchy, code lens,
signature help, other editor plugins beyond grammars, watch-mode builds.
Code lens / inline eval → post-v1 `jet dev` (D-LSP13).
