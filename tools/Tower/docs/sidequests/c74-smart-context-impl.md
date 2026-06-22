# Implementation plan: c74 — Smart Context `#context(field: value){…}`

**Status: ratified, ready to build.** D-CTX1 = G2 (ratified 2026-06-22).
Supersedes the pre-ratification `smart-context.md` design stub for the
implementation phase; keep that file for the sema/risk discussion.

## 1. Ratified decision + spec ref

- **D-CTX1 = G2** — `syntax-decisions.md:2086`: Smart Context grammar G2 is
  `#context(field: value) { … }`, reusing Jet's single `name: value` spelling
  (S61/S29). `=` stays reassign-only (S17). **Q1 = A2** — an explicitly passed
  allocator wins over the ambient context when present. **Q2 = Cβ** — the swap is
  **per-block** (lexical/dynamic-extent block, restored on exit). No single-field
  shorthand; bundle-spread deferred.
- Teaching line (the one sentence sema must honour): **a passed allocator always
  beats the ambient context allocator.** Resolution order for an allocating call:
  explicit `in:`/allocator arg → else current ambient context allocator.
- v1 bundle is **allocator + logger only** (per the design stub's v1 scope).

## 2. Failing-test-first targets (write these first, all red)

1. **`tests/ui/context_beginner_silent.rs`** (R1 guard): a beginner program that
   allocates (`[]`, `.push`) and prints, asserting the strings `context`,
   `allocator`, `ambient` never appear in stdout/stderr. Red until codegen leaves
   beginner output untouched.
2. **`examples/features/72_smart_context.jet`** + `expected/72_smart_context.out`
   (golden, I5): expert swaps the allocator to an arena for one block; a downstream
   helper that allocates lands in the arena; arena freed at block exit; a value
   built *before* the block is unaffected. (Use `mem.Arena` from D-ALLOC1, already
   shipped.)
3. **`tests/ui/context_explicit_wins.rs`** (Q1 = A2): `arena.alloc(v)` *inside* a
   `#context(allocator: other)` block lands in `arena`, not `other`. Differential
   golden, not a diagnostic — assert the address/owner.
4. **`tests/ui/context_restore_paths.rs`**: force `?`-propagation, `return`,
   `break`, and a caught panic out of a `#context` block; each asserts the ambient
   allocator afterward is the *outer* one (R3). Restore must be RAII-guard based.
5. **`tests/ui/context_eq_rejected.rs`**: `#context(allocator = x){…}` (using `=`)
   → **E0760** "context fields are set with `:`, not `=`" (S17 stays reassign-only).

## 3. Pipeline work, in order

Grep anchors: `#`-marker parsing lives next to `#Unsafe`/`#Audit` in
`Source/Parser/Statements.rs` and `Source/Parser/Items.rs`; the type lowering and
emit live in `Source/Codegen/`.

### Lexer — `Source/Lexer/`
No new token. `#context` is `#` + ident `context`, same shape as `#Unsafe`. Confirm
the lexer already emits `#` as a marker sigil (it does for `#Unsafe`/`#Audit`); no
change expected. If `context` needs to be a recognised marker keyword, add the
spelling const to **`Source/Syntax.rs`** (I7): `pub const CTX_BLOCK: &str = "context";`
plus an entry in the all-keywords array (the `TYPE_INT, …` style list near line 896).

### Parser — `Source/Parser/Statements.rs`
Add `#context(field: value, …) { block }` as a statement form (it nests in any block
body). Parse:
- `#` `context` `(` then a comma list of `ident : expr` pairs (reuse the S61
  argument-label parse path — find it via the labelled-call parser in
  `Source/Parser/Expressions.rs`).
- Reject `ident = expr` here with **E0760** (point span at the `=`).
- Then a brace block (reuse the existing block parser).
- Allowed field names in v1: `allocator`, `logger` only; an unknown field is
  **E0761**.

### AST — `Source/AST.rs`
Add a statement variant:
```
ContextBlock { fields: Vec<(String, Expr, Span)>, body: Vec<Stmt>, span: Span }
```
Span on the node and on each field (R4).

### Sema — `Source/Sema/`
This is the bulk. Implement **codegen Option 2 (scoped thread-local)** per the design
stub's lean — smallest surface, no signature churn.
- **`Source/Sema/CheckerCore.rs`**: type-check each field value — `allocator` must be
  an allocator handle type (the `mem.Arena`/allocator opaque types already known to
  `alloc_handle_rust_type` in `Codegen/Context.rs`; mirror that allow-list in sema);
  `logger` must be a logger value. Field/value type mismatch → **E0762**.
- Record, on the `ContextBlock`, which fields are overlaid (for codegen).
- **Q1 = A2 precedence**: where an allocating call resolves its allocator (find the
  existing default-heap allocation path — `[]`, `.push`, list/map growth in
  `Source/Sema/CheckerCore.rs` / `Collections.rs`), the rule is: if the call site
  passes an explicit allocator arg, use it; otherwise the call uses the ambient
  context (resolved at runtime via the thread-local — sema records "uses ambient",
  codegen emits the lookup). Sema makes **no** static binding of which context; it
  only verifies types and records the swap. Dynamic-extent (Q-b): library code called
  inside the block reroutes because the thread-local is live for the whole dynamic
  extent — no per-call analysis needed.
- **R1 (P0)**: ensure no diagnostic emitted from beginner-tier code mentions
  "context"/"allocator". The only context-aware diagnostics (E0760–E0763) fire only
  on source that literally wrote `#context`, so this holds by construction; add a test
  asserting it.

### Codegen — `Source/Codegen/Statement.rs` + `Source/Codegen/Context.rs`
- Emit a `thread_local!` holding the current `JetContext { allocator, logger }`,
  initialised to the default heap allocator + default logger. Put the runtime type and
  the `with_context` guard in **`Source/Prelude/CoreLib.rs`** (a `JetContext` struct +
  a `JetContextGuard` whose `Drop` restores the saved value — RAII, so it restores on
  normal exit, `?`, `return`, `break`, and panic-unwind, satisfying R3).
- Lower `ContextBlock` to: snapshot current context → overlay only the named fields
  (copy-on-write: unmentioned fields keep the outer value, Q-c) → construct a guard →
  run the block → guard `Drop` restores. Roughly:
  ```rust
  { let _ctx_guard = jet_ctx_push(JetContextDelta { allocator: Some(arena), logger: None });
    /* block */ }
  ```
- Ambient-allocating calls that sema marked "uses ambient" read
  `JETCTX.with(|c| c.borrow().allocator.clone())` at the call site; explicit-allocator
  calls (Q1=A2) ignore the thread-local entirely (codegen already has the explicit
  arg). Codegen stays dumb (I3): sema told it which calls are ambient vs explicit.

## 4. Diagnostics (next free in the E07xx block; E0701–E0705 used)

| Code | What | Why | Fix |
|------|------|-----|-----|
| **E0760** | "context fields are set with `:`, not `=`" | `=` is reassignment only (S17); context fields use the `name: value` spelling. | "write `#context(allocator: my_arena) { … }`" |
| **E0761** | "`<name>` isn't a context field" | v1 context carries only `allocator` and `logger`. | "the context bundle holds `allocator` and `logger`" |
| **E0762** | "`<field>` needs a <kind>, got <type>" | allocator/logger fields are typed. | "pass an allocator, e.g. `mem.Arena.new()`" / "pass a logger" |
| **E0763** | (optional) "context swap with no fields does nothing" (lint L07xx instead?) | empty `#context(){…}` is a no-op. | "drop the `#context` wrapper, or name a field to swap" |

Each needs a `tests/ui/` snapshot (I4); pick the next free L-code if E0763 is demoted
to a lint. Confirm E076x is free before writing (E07xx currently tops out at E0705).

## 5. Examples

- `examples/features/72_smart_context.jet` (the golden in §2.2). Expected output: the
  checksum/print proving (a) the in-block helper allocated in the arena, (b) the
  pre-block value is intact, (c) after the block the default allocator is back.
- All `#context` material is **expert-tier docs only** (R1) — do not add it to any
  beginner tutorial example.

## 6. Exit criteria

- All five §2 tests green; `72_smart_context.jet` builds and matches golden output.
- E0760–E0762 (and E0763/L07xx) have ui snapshots; `jet explain` covers each.
- `nix develop -c cargo test` green, including `tests/golden.rs` (no `unsafe` leaks
  from the new prelude code unless gated — the guard uses safe `RefCell`/`thread_local`).
- Beginner-silence test passes: no allocation-heavy beginner program mentions context.

## 7. Effort / risk + one-pass judgment

Self-contained: one new statement form, one thread-local runtime carrier, one RAII
guard, four diagnostics. The hard parts are all *already de-risked* by the design stub:
Option 2 chosen, A2 precedence fixed, Cβ per-block fixed. The arena allocator
(D-ALLOC1) is shipped, so there's a real allocator to swap to. Main risk is the
ambient-vs-explicit resolution wiring in sema touching the existing allocation path —
contained, well-anchored. R3 (restore on every exit) is fully handled by RAII `Drop`,
which Rust gives for free on all exit paths including unwind.

**Completable in one focused agent pass? YES.** This is the most self-contained of the
four. The only thing that could spill is if the existing allocation path in sema is
more entangled than the grep suggests — but even then it is a localised change.
