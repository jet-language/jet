# Beginner/Expert Mode Separation Audit

Card: #8 / c120. Track: Epoch 3 planning. Gate: none for the audit.

## Goal

Prove Jet keeps one language with two faces:

- Beginner default: no target jargon, unsafe vocabulary, generated-code detail,
  package policy, or backend choice before the user asks for it.
- Expert opt-in: low-level, layout, allocation, ABI, target, scheduler, cache,
  generated source, and audit controls are available through explicit gates.
- Hybrid rule: prefer one semantic mechanism with beginner defaults and expert
  views over separate beginner and expert mechanisms.

## Current law

- `docs/spec/philosophy.md`: footguns are opt-in, never opt-out.
- I1/I2/I3/I8: safe by default, rustc hidden, sema owns checks, one canonical
  mechanism.
- S58/D-LL1: `use core.mem` plus `#Unsafe("reason")` gate raw memory.
- D-MATURITY1: `@Experimental`, `@Tested`, `@Hardened` are doc-only API tags.
- D-TARGET-* and D-WD11: embedded/freestanding facts stay hidden until a typed
  target profile is selected.
- D-WD2/D-EXPANDCLI1/D-SEMINDEX1: expert transparency is through dossier/facts,
  not alternate semantics.

## Audit Axes

### 1. Syntax Surface

Classify every user-typeable syntax entry in `crates/jet-foundation/src/Syntax.rs`
and `docs/spec/syntax-decisions.md`.

| Class | Meaning |
|---|---|
| B | Beginner-default, always available. |
| E | Expert-only, reachable through an explicit gate. |
| V | View-only expert transparency, no semantic power. |
| GAP-B | Expert power leaks without a gate. |
| GAP-E | Documented expert power has no real gate or view. |

Known expert gates to verify:

- `use core.mem` for low-level vocabulary.
- `#Unsafe("reason")` / `#Unsafe fn` for operations that can violate I1.
- `#Layout(c)` / `#Layout(columnar)` and typed target profiles for layout/ABI.
- `policy no_alloc` and target allocator facts for allocation ceilings.
- `jet expand --facts`, `jet dossier`, `--json`, and generated-source views for
  inspection.

Deliverable: `docs/spec/beginner-expert-map.md` with one row per surface feature,
its class, decision ID, gate, beginner copy impact, and expert control path.

### 2. CLI and Tooling

Classify `jet` and `jetpack` verbs and flags by default exposure:

- Beginner default: `jet run`, `jet check`, `jet test`, `jet dev`, `jet new`,
  ordinary diagnostics.
- Expert flags/views: generated code, raw frames, fact lenses, target profiles,
  trust/audit/provenance, cache/build graph, perf baselines.
- GAP: raw Rust, raw solver, raw linker, raw package-manager, or backend jargon
  visible in default help or normal errors.

Deliverable: CLI table in `docs/spec/beginner-expert-map.md` plus gap list.

### 3. Diagnostics Voice

Check every non-retired diagnostic in `docs/spec/diagnostics.md` and diagnostic
constructors:

- Beginner diagnostics must not require Rust, cargo, Nix, linker, solver, or
  backend knowledge unless the user opted into that expert tier.
- Expert diagnostics must name the gate the user crossed and the audit path.
- I2 violations are P0: rustc output or generated Rust outside the ICE banner.

Deliverable: before/after wording proposals for each GAP row. New codes still
need diagnostics.md registry rows and UI snapshots before implementation.

### 4. Examples and Docs

Classify `examples/features/**`, `examples/showcase/**`, `docs/reference/**`,
and onboarding docs:

- Beginner examples should run without manifests, targets, grants, or low-level
  imports unless the file is explicitly a low-level/domain example.
- Expert examples should lead with the gate and the reason.
- Mixed examples need section labels and an explanation of why both surfaces are
  present.

Deliverable: example/doc gap table. Do not rewrite examples in this audit unless
the gap is a one-line doc label.

### 5. Product Transparency

Check that every magic default has an expert view:

- syntax and sema facts: `jet expand --facts`, semantic index, dossier
- build/package/env: graph, lock explain, audit, trust grants, provenance
- target/embedded: `jet dossier target` plus stable audit JSON
- proof/perf: `jet prove`, replay artifacts, budget reports

Deliverable: missing-view gaps become plan cards or ballot text.

## Implementation Slices

1. Build the classification schema and `docs/spec/beginner-expert-map.md`.
2. Fill syntax rows from `Syntax.rs` and `syntax-decisions.md`.
3. Fill CLI/tooling rows from command registries and help tests.
4. Fill diagnostics rows from `diagnostics.md` and constructors.
5. Fill examples/docs rows with `rg`-driven evidence.
6. Produce a gap list with one fix shape per row: add gate, add view, rewrite
   diagnostic copy, move doc, or raise ballot.
7. Queue follow-up ballots only where a gap needs new syntax, CLI, manifest, API,
   external dependency, or invariant carve-out.

## Test Strategy

- Markdown/link sanity by `rg`/`ls` for this planning slice.
- Future implementation from findings must add targeted tests for each gate:
  parser/sema rejection, CLI help transcript, diagnostic snapshot, example
  classification, or dossier/fact output.
- Final acceptance for the audit is a complete map plus a gap list whose rows
  either point at ratified implementation work or ballot-ready owner choices.

## Ballots Needed

No ballot for the audit. Each discovered GAP row that needs new user-facing
surface must carry its own ballot before code changes.
