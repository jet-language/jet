# M13 — LSP v2: a real language server

**Blocked on decisions:** S49 (doc comments — needed for hover docs).
Depends on M6 phase 4 (LSP v0 skeleton), M12 (multi-package projects).
**Error codes:** none new (tooling); internal robustness instead.

## Goal

Make Jet feel first-class in an editor. The compiler front end already
owns every fact the server needs; this milestone is about exposing it
with good latency and never crashing. Architecture rule: the LSP reuses
jeter/parser/sema as libraries — zero duplicated language knowledge.

## Capabilities (exact scope, in priority order — ship incrementally)

1. **Diagnostics** (from v0) — upgrade to per-keystroke with
   debouncing; whole-project (import graph) re-check; lints included
   with severity Hint→Warning mapping.
2. **Completion** — names in scope (locals, params, functions, types,
   modules), member completion after `.` (fields, methods, std module
   items), keyword completion in statement position, `import` path
   completion from the filesystem/package list. Snippet bodies for
   `fn`/`if`/`for`/`switch` (switch snippet pre-fills variant arms for
   enum subjects — the killer demo).
3. **Hover** — type + ownership info in Jet terms ("`words`:
   `List[String]` — `var`, may be changed here") and the item's doc
   comment (S49: `///` lines above an item, plain text v1, shown
   verbatim).
4. **Go to definition / find references** — needs a name-resolution
   table keyed by span; build it in sema once, reuse for both. Works
   across files and into dependencies (read-only).
5. **Rename** — span table again; refuses keywords/builtins; updates
   all files in the package atomically.
6. **Quick fixes** — every diagnostic with a mechanical fix carries a
   structured suggestion from sema (extend `Diagnostic` with an optional
   `fix: Vec<(Span, String)>`): S14 autocorrects, `did you mean`,
   "add `take`", "add missing variants to switch" (inserts arm stubs),
   "make this `pub`". The diagnostic *renderer* already prints these;
   this structures them. (This refactor lands first — it's the
   foundation, and CLI output gains `--fix` for free: `jet fmt --fix`
   applies safe fixes.)
7. **Formatting** — already wired to fmt; add range formatting.
8. **Semantic tokens** — token classification for editors (keyword,
   type, function, parameter-with-`mut`, …); makes ownership visible by
   color (mut params get their own token modifier).
9. **Inlay hints** — inferred types on bindings (`val x⟨: Int⟩ = …`)
   and ownership hints at call sites (⟨clone⟩ where L0201 fired). Off
   by default except the clone hint.

## Engineering requirements

- **Incrementality v1 = file-granular:** re-jet/parse only changed
  files; sema re-runs whole-program (it's fast; measure before getting
  clever). Budget: <100ms diagnostics for a 5k-line project on a
  laptop; add a `jet lsp --bench` harness that replays a recorded
  session and asserts the budget.
- **Crash policy:** any panic in a request handler is caught, logged to
  a file, the request answered with an error response — the server
  never dies mid-session. ICE banner equivalent: tell the user once via
  `window/showMessage`.
- Unsaved-buffer compilation: all file access in the front end goes
  through a `SourceProvider` trait (overlay of open buffers over disk).
  This refactor is prerequisite work — do it first, keep `jet run`
  byte-identical.
- JSON-RPC layer: revisit the M6 hand-rolled JSON under load; if it's
  the bottleneck or bug source, request owner approval for serde_json
  in the tooling binary (I6 protocol) rather than gold-plating.
- VS Code extension (editors/vscode) grows: configuration for binary
  path, semantic token theme defaults. Also ship editors/jet.tmGrammar
  and a tree-sitter grammar (tree-sitter-jet/) for everyone else —
  generated from src/syntax.rs where possible so keywords never drift.

## Exit criteria

- Scripted LSP integration tests (JSON transcripts in tests/lsp/)
  covering each capability: completion lists contain expected items,
  hover text pinned, rename produces the expected workspace edit,
  switch-arm quick fix inserts compilable code.
- The bench harness passes its latency budget in CI.
- Dogfood proof: a recorded demo task — write examples/16_wordcount.jet
  from scratch in VS Code using only completions/quick-fixes — has no
  server crash and no stale diagnostics (manual checklist in the PR).

## Out of scope

Debugger/DAP (post-v1 with source maps), workspace symbols fuzzy search,
call hierarchy, code lens, signature help (cheap later; not v1), other
editors' plugins beyond grammars, watch-mode builds.
