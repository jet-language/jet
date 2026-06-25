# Implementation plan: c71 — Typestate via transitioning tags

**Status: ratified, but BLOCKED on unbuilt prerequisites.** See §7 — do not start
without sequencing the prerequisites first.

## 1. Ratified decision + spec ref

- **D-STATE1 = A** — `syntax-decisions.md:2081`: typestate via transitioning tags.
  A function `take`s the old-state tag and returns the next-state tag; calling a
  method in the wrong state is a **compile error E0150**; tags **erase** at codegen
  (zero runtime cost). D-QUAL2 (tag kind) is ratified → the *decision* is unblocked.
  The owner's note on the same line: **"Sequence `#SingleUse` (D-LIN1) machinery first."**
- **D-QUAL2 = B** — `syntax-decisions.md:1870`: exactly two qualifier kinds — `trait`
  (has methods, dispatches) and **`tag`** (no methods, erases). Sema gains a
  first-class **`tag`** keyword; declaring a method on a `tag` is **E0732**; using a
  tag where dispatch is expected is **E0731**. Codegen unchanged (tags erase).
- **D-LIN1 = A** — `syntax-decisions.md:1824`: `#SingleUse` values must be consumed
  exactly once on every reachable path (`^` consumer param / returned / `drop(x)` with an
  `#Unsafe("reason")`). `#SingleUse` implies `#NoCopy`. Checker tracks consumption through
  branches: **E0140** (unconsumed at scope end), **E0141** (unconsumed on one branch).

## 2. Prerequisite reality check (grep findings — IMPORTANT)

`rg "SingleUse|TagKind|is_tag|no_copy|E0140|E0141|E0150|E0731|E0732" Source/` returns
**nothing**. D-QUAL2's `tag` keyword, D-LIN1's `#SingleUse` consumption tracker, and
typestate's E0150 are **all unimplemented**. So this card is really three layers,
bottom-up:

1. **D-QUAL2 tag foundation** — the `tag` keyword, tag declarations, marker-vs-trait
   enforcement (E0731/E0732). Nothing tag-related exists in sema today.
2. **D-LIN1 `#SingleUse`** — linear consumption tracking (the move/consume dataflow),
   E0140/E0141. This is the "machinery" the owner says to sequence first: typestate's
   "old tag consumed, new tag produced" reuses exactly D-LIN1's must-consume dataflow.
3. **D-STATE1 typestate** — transition functions that consume the old-state tag and
   produce the next, with E0150 on wrong-state calls.

## 3. Failing-test-first targets (per layer)

**Layer 1 (tag foundation):**
- `tests/ui/tag_method_rejected.rs` — a `tag` with a method body → **E0732**.
- `tests/ui/tag_used_as_dispatch.rs` — a tag where a trait is expected → **E0731**
  with the fix-it to declare a `trait`.

**Layer 2 (#SingleUse):**
- `tests/ui/singleuse_unconsumed_scope.rs` — `#SingleUse` value bound, never consumed
  at scope end → **E0140**, naming the binding.
- `tests/ui/singleuse_branch.rs` — consumed on one `if` arm, not the other → **E0141**.
- `tests/ui/singleuse_no_copy.rs` — copying a `#SingleUse` value → the `#no_copy`
  error (reuse existing no-copy/move-after-move diagnostic if one exists; else new).
- `examples/features/73_single_use.jet` + expected out — happy path: a `#SingleUse`
  handle passed to a `take` consumer; erases to nothing at runtime.

**Layer 3 (typestate):**
- `tests/ui/typestate_wrong_state.rs` — call a method requiring state `Open` on a
  value currently in state `Closed` → **E0150**, naming both states.
- `examples/features/74_typestate.jet` + expected out — a connection modelled with
  states (e.g. `Disconnected → Connected → Closed`); transition fns thread the tag;
  a correct sequence compiles and runs; tags erase (golden proves runtime behaviour
  identical to the untagged version).

## 4. Pipeline work, in order

Anchors: tag/marker parsing belongs with `#Unsafe("reason")` handling in
`Source/Parser/Items.rs` + `Source/Parser/Statements.rs`; the consume/move dataflow
belongs in `Source/Sema/CheckerOwnership.rs` (already does move/borrow tracking — the
natural home for linear consumption); type registration in
`Source/Sema/CheckerItems.rs` and `Source/Sema/Registration.rs`.

### Syntax — `Source/Syntax.rs`
Add (I7) the `tag` keyword const and the `#SingleUse` / `#no_copy` marker spellings,
each with its decision ID in a comment; register in the all-keywords array.

### Lexer — `Source/Lexer/`
`tag` is a new keyword token (mirror how `trait` is lexed — find it). `#SingleUse` is
`#`+ident, same shape as `#Unsafe`; no lexer change beyond the keyword.

### Parser
- **`Source/Parser/Items.rs`**: parse `tag Name { … }` declarations (parallel to the
  existing `trait` item parser). A tag body has no method bodies; if one is present,
  surface E0732 in sema (parse it, flag it) rather than failing in the parser.
- Parse `#SingleUse` and a typestate tag as a marker on a type/struct decl.
- Typestate transition functions: D-STATE1's surface is a fn that takes the old-state
  tag and returns the next. Confirm the exact inline tag spelling — **see §7 gate**:
  D-QUAL2 line 1881 says the *exact inline spelling of value tags still rides D-QUAL1*
  (surface routing), which is **still open**. This is a real upstream gate.

### AST — `Source/AST.rs`
Add a `Tag` item variant; add a tag/marker list field to type and parameter nodes so a
state tag can be attached to a value's type. Spans on all (R4).

### Sema — the bulk
- **`Source/Sema/Registration.rs` / `CheckerItems.rs`**: register `tag` decls;
  enforce no-methods (**E0732**); enforce tag-not-used-for-dispatch (**E0731**).
- **`Source/Sema/CheckerOwnership.rs`**: linear-consumption dataflow for `#SingleUse`
  — track each such binding's consumed/live state across branches; **E0140** at scope
  end if live, **E0141** if live on some-but-not-all paths. `#SingleUse` implies
  `#no_copy` (a copy/clone is an error). This is the machinery D-STATE1 reuses.
- **typestate**: a value carries a state tag in its type. A transition fn's signature
  consumes the value at state S and returns it at state S'. Calling a method that
  requires state S on a value currently at S'' (S'' ≠ S) → **E0150**. Implement as:
  the state tag is part of the value's sema type; method-resolution checks the required
  state against the tracked current state (which advances only through transition fns).
- **Codegen — `Source/Codegen/`**: nothing. Tags erase (D-QUAL2/D-STATE1). Confirm the
  state tag never reaches `rust_type` in `Codegen/Context.rs` — it must be stripped in
  sema so codegen sees the plain underlying type (I3).

## 5. Diagnostics (E013x–E015x is EMPTY today; E0731/E0732 are pre-assigned by the decisions)

| Code | What | Why | Fix |
|------|------|-----|-----|
| **E0140** | "`<x>` is a single-use value but it's never used" | a `#SingleUse` value must be consumed on every path. | "pass it to a `^` consumer, return it, or `drop(x)` with an `#Unsafe("reason")`" |
| **E0141** | "`<x>` is used on one path but not the other" | single-use must be consumed on *every* reachable path. | "consume it in the other branch too, or hoist the consume after the `if`" |
| **E0150** | "this needs `<X>` in state `<S>`, but it's in state `<S''>`" | typestate: a method is only valid in a given state. | "transition it first: call `<the transition fn>` to reach `<S>`" |
| **E0731** | "`<Tag>` is a tag, not a trait — it can't be dispatched" | tags have no methods and erase. | "declare `trait <Name>` if you need methods/dispatch" |
| **E0732** | "tags can't have methods" | methods → trait, no methods → tag (D-QUAL2 one-liner). | "move the method to a `trait`, or drop the body" |

All need `tests/ui/` snapshots + `jet explain` (I4). Also a `#no_copy`/move-after-move
diagnostic — reuse the existing move diagnostic from `CheckerOwnership.rs` if present.

## 6. Exit criteria

- All §3 tests green across the three layers; `73_single_use.jet` and
  `74_typestate.jet` build and match golden output; both prove tags erase (no runtime
  cost — generated Rust identical to the untagged equivalent).
- E0140/E0141/E0150/E0731/E0732 each have a ui snapshot and `jet explain`.
- `tests/golden.rs` green; no `unsafe` introduced.

## 7. Effort / risk + one-pass judgment

This is **sema-heavy and three features deep**, and two hard gates apply:

1. **Upstream gate (real).** D-QUAL2:1881 says *the exact inline spelling of the value
   tags (which includes typestate state tags) still rides D-QUAL1's surface-routing
   decision, which is still OPEN.* You can build the D-QUAL2 `tag` foundation and the
   D-LIN1 consumption tracker without it, but the **typestate surface syntax itself is
   not fully pinned** — the transition-fn/state-tag spelling depends on D-QUAL1. Name
   this gate; do not invent the spelling (syntax-decision protocol).
2. **Sequencing.** The owner explicitly says build `#SingleUse` (D-LIN1) machinery
   first, and D-LIN1/D-QUAL2 are *both unimplemented* (grep confirms). So the true
   scope is: tag keyword + E0731/E0732, then linear-consumption dataflow +
   E0140/E0141, then typestate + E0150 — three sequenced sema features, the middle one
   (linear dataflow across branches) being genuinely subtle.

**Completable in one focused agent pass? NO.** Three stacked sema features, one of
them (linear/branch consumption tracking) intricate, plus an open upstream surface gate
(D-QUAL1) on the typestate spelling. Recommended split: (a) D-QUAL2 tag foundation as
its own pass, (b) D-LIN1 `#SingleUse` as its own pass, (c) D-STATE1 typestate last —
and (c) only after D-QUAL1 ratifies the value-tag inline spelling.
