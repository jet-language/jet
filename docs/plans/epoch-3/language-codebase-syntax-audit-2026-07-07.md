# Language/codebase syntax audit — 2026-07-07

Scope: docs/spec, syntax registry, lexer/parser, formatter, sema, codegen/TIR,
diagnostics snapshots, core-library docs, then-current examples/apps (removed
2026-07-14), and live Tower cards.

This pass excludes the earlier jetos/canvas/core-lib batch except where those
surfaces expose general language law drift.

## Findings

### 1. Retired-syntax teaching layer conflicts with D-S14-PAUSE

`docs/spec/syntax-decisions.md` says retired spellings get ordinary syntax
errors until after Epoch 6. The implementation still keeps a broad teaching
layer: `FOREIGN_*` constants in `crates/jet-foundation/src/Syntax.rs`, lexer
tokens for retired words, parser recovery for `def`/`func`/`switch`/`match`/
`for`/`while`/`mut`/`take`/`todo`/`import`, and diagnostics entries marked
retired while still reachable in parser code.

Devil pass: the teaching layer is useful for migration and discoverability, but
it is currently neither clearly default-on law nor clearly off. This makes the
language larger than the greenfield spec says, and new contributors cannot tell
whether to delete, preserve, or add teaching paths.

Action: owner ballot. Pick strict pause, curated teaching, or opt-in teaching
mode. Then remove or harden code/tests accordingly.

### 2. Ratified-unbuilt status is stale and not machine-checked

`syntax-decisions.md` still carries multiple `ratified, unbuilt` or `unbuilt`
notes, including refinements, discard sigil, replayable, comptime find, Path
ops, time, math, HTTP, and measurement. Some are true gaps; some appear stale.
Example: D-PATHFS1 is documented as unbuilt while sema/TIR/prelude already carry
typed Path operations including `write_atomic` and `walk`.

Devil pass: unbuilt notes are useful when truthful, but stale notes are worse
than absent notes. They train agents to open duplicate cards and make the
language look less coherent than the repo actually is.

Action: build a status matrix from syntax decisions to Syntax.rs, parser, sema,
codegen/TIR, formatter, docs, examples, and tests. Every ratified entry should
be marked shipped, gated, declined, or split into a card.

### 3. Dispatch surface still leaks `switch`

The canonical user surface is `if subject == { ... }`, but docs, diagnostics,
AST/TIR names, tests, and comments still say `switch`. Some internal names are
fine, but user-facing diagnostics like E0307/L0301 and OS-target errors still
call the construct a switch.

Devil pass: internal `Switch` names are cheap and may be fine in code, but user
copy should teach the canonical shape. Keeping `switch` in diagnostics reopens a
retired concept.

Action: audit user-facing text and tests; keep internal names only where they do
not reach docs, diagnostics, examples, or LSP-visible explanations.

### 4. Pattern/dispatch implementation and docs disagree

`syntax-decisions.md` says struct-pattern dispatch arm heads are unbuilt. The
source now has `Pattern::Struct`, parser support, sema checks, formatter paths,
and TIR subset/lowering/emit support around struct-shaped arm heads.

Devil pass: this may be a shipped feature with stale docs, or a partially
shipped feature missing end-to-end examples. Either way, the spec cannot be the
only place that says "gap" after implementation lands.

Action: verify with a focused UI/example/TIR battery, then update the spec or
split remaining real gaps.

### 5. Marker-plane law remains hard to discover

Tower already has marker-family decisions, but the surface is still hard to read
from source alone: `#` directives, `@` contracts, derive-ish markers, unsafe
regions, comptime directives, test/bench markers, and capability markers are
spread through decisions and implementation. The language needs one compact
source-of-truth table that tells agents and users which plane a marker belongs
to and why.

Devil pass: do not reopen settled marker decisions unless implementation finds a
real contradiction. The needed artifact is a law/table plus reconciliation
tests, not another taste debate.

Action: produce a marker family matrix over Syntax.rs, parser, formatter, docs,
and syntax decisions; queue only contradictions as ballots.

### 6. Diagnostic voice has no broad linter

`docs/spec/diagnostics.md` bans rustc/user-hostile language, but the repo has no
single ratchet over snapshots and diagnostic constructors for terms like
`switch` where retired, raw parser jargon, or Rust implementation leakage.
There is one concrete runtime leak: the core HTTP router duplicate-route path
panics with `E2804` from generated Rust prelude code.

Devil pass: some words are legitimate in internal comments and non-Jet file
formats. The linter must target user-facing snapshots, diagnostic constructors,
and generated-runtime errors, not every source comment.

Action: add a diagnostics voice lint/test and convert raw runtime panics that
represent Jet user errors into Jet-owned diagnostics or fallible API errors.

### 7. examples/apps were slices, not capstones (removed 2026-07-14)

At audit time, `examples/apps/*/README.md` described `jetgrep`, `jetpaste`,
`jettasks`, `jetfighter`, and `metal` as implementation slices — useful proof
fixtures, not product capstones. That tree and `tests/slices.rs` were deleted
on 2026-07-14; do not treat those paths as living fixtures.

Devil pass: cheap deterministic slices were valuable. The failure mode was
labeling them as capstones. Capstone proof belongs on JetLab / JetPlay cards,
not under `examples/apps/`.

Action (then): capstone proof ratchet + JetLab / JetPlay. Action (now): keep
those cards; do not recreate `examples/apps/` unless the owner re-homes them.

## Capstone candidates

### JetLab — local-first AI/agent/data workbench

Purpose: replace the Python/Node glue stack agents use today for repo ops,
data transforms, evals, local files, HTTP tools, notebooks, and durable audit
logs.

Comparisons: Python notebooks + pandas + Typer + LangChain/LlamaIndex +
Open WebUI + Obsidian + shell scripts. Jet should beat them with one typed
project, compiled CLIs/services/web UI, deterministic replay, safe filesystem
and network effects, first-class diagnostics, and audit logs.

Proof bar: standalone app; no fake LLM-only facade; deterministic local model
adapter fixture; indexed workspace search; task runner; data import/export;
plugin/tool sandbox; saved runs; headless tests; web UI tests; perf and LOC
comparison against a reference Python/TS implementation.

Risk: it can become a wrapper around external services. Hardening rule:
core workflows must run offline against fixtures and local files.

### JetPlay — 2D game/editor/workbench

Purpose: prove Jet can own game runtime, editor tooling, assets, deterministic
replay, input/audio/rendering, and export.

Comparisons: Godot, Bevy, Love2D, Unity mini-projects, Raylib C/Zig/Odin demos.
Jet should beat them on one-language gameplay+tools, safety, deterministic
headless tests, and clean expert control over render/audio/input loops.

Proof bar: playable game, editor modifies live source-backed assets, deterministic
replay, asset pipeline, packaged native build, browser or headless proof where
applicable, perf budget, and LOC comparison against an equivalent Love2D/Godot
or Bevy project.

Risk: it can stop at an engine demo. Hardening rule: ship one playable game plus
one editor workflow that changes the shipped game and is tested.

## Cards queued from this audit

- Teaching-layer ballot after D-S14-PAUSE.
- Syntax-law/source status matrix.
- Ratified-unbuilt closure.
- Dispatch/pattern naming and doc reconciliation.
- Marker-plane discoverability matrix.
- Diagnostics voice/runtime-error audit.
- Capstone proof ratchet (examples/apps removed 2026-07-14; ratchet is historical).
- JetLab capstone.
- JetPlay capstone.
