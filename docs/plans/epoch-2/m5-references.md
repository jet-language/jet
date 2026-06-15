# E2-M5 — Tier-2 references and zero-copy patterns

**Status:** draft — **blocked on D-REF1…D-REF3** (Group M5). Pattern-matching
ergonomics (Group 15: D-PAT1 nested patterns, D-PAT2 guards) may ride this
window if ratified; otherwise they stay deferred.
**Depends on:** E2-M1 (sendability rules interact with references crossing task
boundaries). Feeds E2-M13 low-level tier.
**Error codes:** E23xx block (claim in docs/spec/diagnostics.md).

## Goal

Unlock Rust-territory programs without surfacing Rust lifetime syntax. `view`
returns and stored `ref` fields already exist as tier-2 machinery (S10); this
milestone **specifies, hardens, and tests** them as a coherent post-v1 feature
rather than inventing a new model. No user-written lifetime names, ever.

## Owner decisions — ratify before any code

| ID | Question | Rec | Default if deferred |
|---|---|---|---|
| D-REF1 | Teaching order | **A** — after the beginner ownership chapter | A |
| D-REF2 | Ship arenas this milestone | **A** — only if the parser example needs them | A |
| D-REF3 | Inlay-hint defaults beyond clone | **A** — borrowed-return + cleanup scopes on | A |
| D-PAT1/2 (opt) | Nested patterns / `&&` guard scope | see Group 15 | deferred |

## Scope

- **Finalize reference rules + labels.** Stored/returned reference rules and
  their labels (`ref[src]` etc.); when a `view` may be returned and how long it
  may live.
- **Task/channel safety.** References must not cross task/channel boundaries
  unless explicitly proven safe — ties into E2-M1 sendability (a `ref`-holding
  struct is already E1102-unsendable). Keep that invariant; document it here.
- **Zero-copy APIs.** String/list/map view APIs where they are worth the
  complexity; reject the ones that aren't (I8 ratchet).
- **Arena/owner patterns (D-REF2).** Only if needed to make graphs and parsers
  ergonomic; do not ship speculative allocator surface.
- **LSP inlay hints (D-REF3).** Hints for borrowed returns and cleanup/borrow
  scopes, on by default beyond the existing clone hints.
- **Guide chapter.** Teaches this *after* the beginner ownership chapter (D-REF1)
  — beginners never need tier-2 to be productive.

## Sema focus — soundness matrix

The deliverable is a proven matrix, not new syntax. Every cell must be either
sound-and-allowed or rejected with a Jet-words diagnostic:

| Construct | returned `view` | stored `ref` field | nested struct | generic | closure capture | task boundary | package boundary |
|---|---|---|---|---|---|---|---|

Each "rejected" cell maps to an E23xx code whose text answers **"what owns
this?"** and **"how long can this view live?"** in Jet words — never "lifetime
`'a` does not outlive `'b`".

## Diagnostics to register

- **E2301** returned `view` outlives its owner ("what owns this?").
- **E2302** stored `ref` field would dangle ("how long can this view live?").
- **E2303** `ref`/`view` crosses a task/channel boundary (cross-ref E1102).
- **L2301** inlay/advisory: this return borrows; here is its source.

## Examples & tests

- `examples/features/35_zerocopy.jet` — a zero-copy parser that beats a
  clone-heavy version while staying readable (the milestone's headline).
- ui fixtures for each rejected matrix cell (E2301–E2303) with a fixed-with-clone
  companion.
- A soundness fuzz target (feeds E2-M17 audit) over returned views and ref fields.

## Out of scope

- User-visible lifetime syntax (forbidden).
- Self-referential structs / general graph cycles beyond the arena pattern.
- Shared mutable references across tasks (stays E1101/E1102 territory).
- Async borrows.

## Exit criteria

- The soundness matrix is complete; every rejected cell has a diagnostic + test.
- No user-written lifetime names anywhere.
- Diagnostics explain ownership and view lifetime in Jet words.
- The zero-copy parser example beats the clone-heavy version and stays readable.
- `nix develop -c cargo test` green.
