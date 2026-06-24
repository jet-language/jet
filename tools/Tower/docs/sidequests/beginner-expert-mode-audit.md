# c120 — Beginner/expert mode separation audit
**Decision:** none required for the audit itself. Any gaps that need new syntax or CLI flags
will each generate their own ballot card.
**Gate:** none.

---

## Philosophy anchor

From `docs/spec/philosophy.md`: "Beginners get magic. Batteries included… Expert controls are
structurally hidden behind explicit opt-in gates, not merely undocumented." "Footguns are
opt-in, not opt-out. Beginners never encounter them. Experts choose them."

The audit checks every place in the compiler, CLI, diagnostics, examples, and docs where this
contract could be violated — silently exposing expert complexity to beginners, or silently
hiding expert capability from experts who need it.

---

## Audit scope

### Axis 1 — Language surface (Source/Syntax.rs, parser, sema)

Walk every entry in `Source/Syntax.rs` and classify:

| Class | Criterion |
|-------|-----------|
| **B** — beginner-default | Always available; no opt-in required |
| **E** — expert opt-in | Only reachable via an explicit gate (`@unsafe`, `use core.mem`, `#layout(…)`, `region`, `@audit`) |
| **GAP-B** | Expert feature reachable without explicit opt-in (a leak) |
| **GAP-E** | Expert feature documented/planned but not yet behind a real gate (a stub gate) |

Known ratified expert gates: `@unsafe { }` / `@unsafe fn` (E2-M13/D-LL1), `use core.mem`
(D-UNINIT1), `#layout(c)` / `#layout(columnar)` / `#layout(packed)` / `#layout(align(N))`
(D-REPRC1, D-SOA1), `region r { }` (D-REGION1), `@audit("…")` (I1), `jet debug --raw-frames`
(D-DBG2).

**Deliverable:** a table in `docs/spec/beginner-expert-map.md` listing every syntactic
feature with its class. Flag every GAP-B and GAP-E.

**Known gaps to investigate:**

- `core.mem` import: does the parser enforce that `use core.mem` is required before any
  `#uninit` binding, or is `#uninit` reachable without it? (`Source/Sema/CheckerItems.rs`)
- `@unsafe` regions: does sema enforce that raw pointer dereference (`*ptr`) is only legal
  inside `@unsafe { }`? Check `AccessConvention::Raw` handling in
  `Source/Sema/CheckerOwnership.rs`.
- `#layout(…)` structs: is the attribute gated to expert use, or can a beginner write it
  accidentally and get confusing output?
- Arena allocator (`mem.Arena`): reachable from normal imports or requires expert import?

### Axis 2 — CLI and tooling

Walk `Source/main.rs` verb dispatch and `Source/CmdDevTools.rs`. Classify each flag:

| Verdict | Meaning |
|---------|---------|
| **B-default** | Runs without flags; output is beginner-friendly |
| **E-flag** | Expert capability gated behind an explicit flag |
| **GAP** | Expert flag exposed in the default help or default output |

**Specific checks:**
- `jet dev --raw-frames` must not appear in default `--help` output (D-DBG2: expert opt-in).
- `jet build --profile expert` or similar layout flags: if they exist, are they visible in
  default help?
- Error output from `jet build`: does it ever mention Rust file paths or Rust error codes? If
  so, that is an I2 violation and an E-flag gap.

### Axis 3 — Diagnostics voice (`docs/spec/diagnostics.md`, `Source/Sema/Diagnostics.rs`)

For every error code in `docs/spec/diagnostics.md`:
- Is the "fix" copy written for a beginner (no assumed Rust knowledge, no jargon)?
- For expert-tier errors (e.g. `@unsafe` violations, layout errors), does the error clearly
  name the expert opt-in that was used, so the user knows they're in expert territory?
- Does any error message contain Rust identifiers (`Vec`, `Box`, `&mut`, `#[repr(C)]`)?
  These are I2 violations.

**Deliverable:** a list of diagnostic codes whose voice needs updating, with specific
before/after text.

### Axis 4 — Examples (`examples/`)

Classify each example:

| Class | Criterion |
|-------|-----------|
| **B** | Uses only beginner-tier features; no explicit expert gate |
| **E** | Uses at least one explicit expert gate |
| **mixed** | Mixes both without labeling |

Check that `examples/features/` has a clear progression (01_ through 99_ are roughly
beginner; expert examples are in `examples/showcase/` or labeled). Check that no beginner
example accidentally demonstrates an expert feature without labeling it.

### Axis 5 — Docs (`docs/spec/`)

- `docs/spec/philosophy.md`: does the beginner/expert framing match the actual implementation?
- `docs/spec/architecture.md`: do the pipeline rules (R1–R7) enforce the beginner-default
  invariant at each phase?
- Any doc that describes an expert feature without the explicit opt-in gate is a gap.

---

## Output

The audit produces:

1. `docs/spec/beginner-expert-map.md` — the classification table (Axis 1).
2. A list of GAP items per axis, each with a concrete fix:
   - GAP-B (leak): add a sema gate or a parser rejection; file as an implementation task.
   - GAP-E (stub gate): either implement the gate or remove the claim from docs.
   - I2 violation in diagnostics: rewrite the message copy (file as a diagnostics task).
   - CLI gap: move the flag out of default `--help` or add a `--expert` flag group.

3. For each gap that requires new user-facing syntax or a new CLI flag, draft a ballot card
   (the syntax/flag choice is an owner decision per the syntax decision protocol).

---

## Execution

The audit is a read-only code + docs sweep. One implementer walks all five axes,
filling in the map table and gap list. Estimated scope: ~1 day for axes 1–5; then separate
tasks for each gap.

**Files read (no writes during audit):**
- `Source/Syntax.rs`, `Source/Sema/CheckerOwnership.rs`, `Source/Sema/CheckerItems.rs`
- `Source/main.rs`, `Source/CmdDevTools.rs`
- `docs/spec/diagnostics.md`, `docs/spec/philosophy.md`, `docs/spec/architecture.md`
- `examples/features/*.jet`, `examples/showcase/*.jet`

**Files written (audit output):**
- `docs/spec/beginner-expert-map.md` (new)
- Gap list appended to the relevant sidequest or filed as new sidequest cards

---

## Decision verdict

No decision needed for the audit itself. Each discovered gap that requires a new user-facing
syntax or CLI flag becomes its own ballot card. Flag those for the owner per the syntax
decision protocol.
