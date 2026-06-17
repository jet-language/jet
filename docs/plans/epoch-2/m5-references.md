# E2-M5 — Tier-2 references and zero-copy patterns

**Status:** ✅ IMPLEMENTED on branch `epoch-2-impl` (commits 3e4db31 + fd02646) —
soundness matrix complete, E2301/E2302/E2303/E2304/L2301 registered+tested,
`35_zerocopy.jet` golden-pinned, L2301 inlay hints wired, soundness fuzz target
green. D-REF1/D-REF3 ✅ ratified; **D-REF2 (arenas) OPEN → not implemented** (I7);
D-PAT1/2 deferred. See EPOCH2-IMPL-PROGRESS.md.
Was: draft — **blocked on D-REF1…D-REF3** (Group M5). Pattern-matching
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

| ID | Question | Rec | Default if deferred | Ratified |
|---|---|---|---|---|
| D-REF1 | Teaching order | **A** — after the beginner ownership chapter | A | ✅ ratified 2026-06-16 — A: teach references after the beginner ownership chapter |
| D-REF2 | Ship arenas this milestone | **A** — only if the parser example needs them | A | ✅ ratified 2026-06-17 — A: ship arenas; live directly in `core.mem` (not a submodule); surface the API as `core.mem.Arena` or equivalent flat path |
| D-REF3 | Inlay-hint defaults beyond clone | **A** — borrowed-return + cleanup scopes on | A | ✅ ratified 2026-06-16 — A: borrowed-return + cleanup-scope hints on by default |
| D-PAT1/2 (opt) | Nested patterns / `&&` guard scope | see Group 15 | deferred | — |

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

| Source / construct | returned `view` | stored `ref` field |
|---|---|---|
| parameter (whole) | **allowed** — caller owns it past the call (`view_pkg_boundary_ok`, also the package-boundary cell) | **rejected E2302** — the struct can outlive the call and the generated Rust struct has no lifetime to name the borrow |
| field of a parameter | **allowed** — borrow of a stored field the caller still owns (`view_field_of_param_ok`) | **rejected E2302** (`ref_field/field_of_param` in the fuzz target) |
| index / slice of a parameter | **rejected E2304** — the slice/index helper builds a fresh owned piece; a view would borrow a temporary (`view_index_into_param`) | **rejected E2302** |
| local (whole) | **rejected E0206** — fresh value freed at the closing brace | **rejected E2302** (`ref_field_dangles_local`) |
| field of a local | **rejected E2301** — names the owner that dies (`view_return_field_of_local`) | **rejected E2302** |
| fresh literal | **rejected E0206** | **rejected E2302** |
| const | **allowed** — `'static` | **allowed** — `'static` (the only sound `ref` source in v1) |
| through a generic type param | same as its source row — `view T` into a field of a `Wrap<T>` parameter is **allowed** (`view_generic_field_ok`); into a local is **rejected E2301** | same as its source row |
| through a closure capture | **rejected E0206 / E0113** — a closure can't smuggle a borrow of a local past a `view` return (`view_closure_returns_local`) | n/a — a closure can't construct an escaping `ref` field soundly |
| across a task / channel boundary | **rejected E1102** (E2303 cross-refs it) — a borrow can't be sent (`ref_struct_crosses_task`) | **rejected E1102** — a `ref`-holding struct is unsendable |

Verdict summary: a `view` may borrow into a **field of something the caller owns** (a parameter or const), including through a generic wrapper — this is the zero-copy primitive (`35_zerocopy.jet`). Everything that would outlive its owner is rejected with a Jet-words diagnostic. A stored `ref` field has **no sound construction in v1** except from a `'static` const (no string consts yet, no user lifetimes, arenas D-REF2 OPEN), so every value/borrowed source is rejected (E2302) rather than handed to rustc as an ICE. The `sema-accepted ⇒ rustc-accepted` invariant is pinned by `tests/ref_soundness_fuzz.rs`.

(Original wide matrix, kept for reference; each cell folds into the source-rows above.)

| Construct | returned `view` | stored `ref` field | nested struct | generic | closure capture | task boundary | package boundary |
|---|---|---|---|---|---|---|---|
| verdict | field-of-owner allowed; else E0206/E2301/E2304 | const-only; else E2302 | same as field-of rows (E2301/E2302) | folds to source row (allowed via param field; E2301 via local) | E0206/E0113 (no escape) | E1102 / E2303 | allowed for `view` of a param; `ref` struct unsendable (E1102) |

Each "rejected" cell maps to an E23xx code whose text answers **"what owns
this?"** and **"how long can this view live?"** in Jet words — never "lifetime
`'a` does not outlive `'b`".

## Diagnostics to register

- **E2301** returned `view` outlives its owner ("what owns this?").
- **E2302** stored `ref` field would dangle ("how long can this view live?").
- **E2303** `ref`/`view` crosses a task/channel boundary (cross-ref E1102).
- **E2304** an indexed or sliced piece can't be handed back as a `view` (the
  slice/index helper builds a fresh owned piece; a view would borrow a
  temporary). Added in chunk B for the zero-copy / generic cell.
- **L2301** inlay/advisory: this return borrows; here is its source. Wired into
  the LSP in chunk B (on by default, D-REF3); test `tests/lsp/08_view_return_hints.json`.

### v1 limitation — stored `ref` construction

A stored `ref` field is **declarable surface but only constructible from a
`'static` const** in v1: the generated Rust struct has no lifetime to name a
borrow of a parameter or local, and there are no string consts yet. Sema rejects
every non-const source with E2302 (closing the chunk-A ICE where a
parameter/field source slipped through to rustc). Full `ref`-field
construction waits on arenas (D-REF2, OPEN) — out of scope here.

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
