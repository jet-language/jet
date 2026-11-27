# Unified `#Defer(when:, scope:)` cleanup marker — pre-build adversarial review

Date: 2026-11-27. Card: #7063 (criterion 1). Status: review only. No code was changed.

## Verdict

**Build it, but not as the card states it.** Card #7063 bundles one sound
collapse with three moves that would regress safety or lose capability:

- **Sound collapse:** `defer`, `scope.guard`, `on_commit`, and `on_rollback`
  become one `#Defer`.
- **Regressing moves:** deleting the `Rollback` trait, moving `#Close` "into an
  external C FFI library", and giving `.Fn`/label scopes a second capture
  meaning.

Building it as written would break I8 (one capture law), I1 (silent loss of
rollback for non-copy values), and I3/I4 (foreign close checking leaves sema).

The design below fixes each problem. Six findings need an owner ballot before
the build starts (§9).

## 0. Sources read this session

- Tower: `node Tools/Tower/tower.mjs card show 7063` shows phase `planning`,
  no decisions attached, and 3 open criteria. A `decision list` search for
  "defer", "cleanup", "rollback", and "transact" found no ratified decision for
  the `#Defer` spelling.
- Syntax registry:
  - `CoreSurface.jet` `KW_DEFER` (contextual keyword; "`defer close(resource)`
    stays the one closing form") and `RESOURCE_CLOSE`.
  - `EffectsSurface.jet` `KW_TRANSACT`, `TXN_ON_COMMIT`, `TXN_ON_ROLLBACK`.
  - `MathLayout.jet` `KW_NEXT: "next"`. There is no `skip` loop keyword; this
    review uses `next`.
- Prelude:
  - `Markers.jet` `Close` (`close: Ident`, site `.Function`), `Undo`, and
    `Transact` (`name: Ident`, site `.Block`).
  - `Traits.jet` `Rollback` (`Snapshot`, `snapshot`, `restore`).
- Parser: `Statements.jet` `parser_parse_defer_statement` and
  `parser_defer_close_call`.
- Sema:
  - `Statements.jet` `.Defer` arm (`SemaDeferFrame`, `sema_ownership_defer_moves`).
  - `Statements/Assignments.jet`: `sema_stmt_check_transaction`,
    `sema_stmt_transaction_collect_roots`, `sema_stmt_transaction_snapshots`,
    `sema_stmt_check_defer_close`, and the E0509/E0512 emitters.
  - `Calls/Direct.jet` `scope.guard`.
  - `Calls/Methods.jet` `on_commit`/`on_rollback`.
  - `Effects/Checks.jet` hook exemption from E0746.
  - `Registration/Checked.jet` `sema_checked_foreign_close` (E0702).
  - `MarkerModules.jet` marker-equals-trait rule.
  - `Ownership/Checks.jet` `sema_ownership_defer_moves`.
- IR:
  - `BuildTransact.jet`: `ir_defer`, `ir_run_defer`, `ir_transact`, the
    `IRTxFrame` notes, `ir_transaction_drop_body`/`_walk`, and
    `ir_transaction_hook`.
  - `BuildCoreClosures.jet` `ir_scope_guard`.
- Spec: `spec.md` "Cleanup at a process stop" (~L2245) and the `#Transact`
  section (~L2754–2783).
- Prior audit: `Docs/Audits/2026-11-27-cognitive-budget-orthogonality-audit.md`
  O16.
- Counts come from `git grep -c` and `git grep -l` over tracked files, excluding
  `Tools/Tower/.tower/`.

## 1. What ships today (facts the design must preserve or consciously drop)

| Mechanism | Lowering today | Heap? | Capture law |
|---|---|---|---|
| `defer stmt` / `defer { … }` | `ir_defer` pushes a marker into the binding list. `ir_run_defer` builds the body inline at every exit, with `stopping = true` so no stop point interrupts it. | **None.** The code is duplicated per exit. | **By place, read at exit.** The body is checked as lexical code. Moves it makes are recorded as happening at scope exit (`deferred_depth`). |
| `defer close(x)` | `Stmt.DeferClose`. The `Close` bound is checked (E0905), and the move of `x` is deferred to scope exit. | None | Moves `x` at scope exit; `x` stays usable until then. |
| `scope.guard(fn() {…})` | Closure box re-owned as a must-use `ScopeGuard` (E0419 if discarded) whose drop runs it. | **Yes:** a closure box per call. | Closure capture at the call. The guard is a first-class value, used inside `Bundle<ScopeGuard>` by a drop-order test. |
| `t.on_commit(f)` / `t.on_rollback(f)` | A 24-byte node plus a closure box pushed on the handle's list. The handle drop walks the chosen list LIFO. Rollback hooks run between `jet_rt_transaction_hook_enter`/`_leave` (masked stop, own 5 s deadline, failure becomes a secondary report). | **Yes:** 2 allocations per registration, including inside loops. | Closure capture at the call. |
| `impl Rollback` | **Not a hook.** It customises the per-value snapshot/restore that `#Transact` takes on entry for each assigned root (`sema_stmt_transaction_snapshots`). | Depends on the type | Not applicable |
| `#Close(f)` on a foreign fn | Sema-checked (E0702) contract: the returned handle gets `Close` through `f(^^handle)`. Per `MarkerModules.jet`, "a built-in trait and the Prelude marker of its name are one thing". | None | Not applicable |
| `#Undo(inv)` on a foreign fn | The inverse is registered on the enclosing transaction's rollback path, so the foreign call is legal inside `#Transact`. | Hook list | Captured call arguments |

Two divergences already exist; both are owed before or with this card:

- **D1 — spec/code divergence on rollback opt-out.** `spec.md` says "Registering
  an `on_rollback` hook takes control of undo for the handled value, so that
  value is not automatically snapshotted." Live code collects snapshot roots
  from every `.Assign` in the body, including inside `defer` bodies
  (`sema_stmt_transaction_collect_roots`). It never excludes values an
  `on_rollback` hook touches. Either the spec or the code is wrong today.
- **D2 — inconsistent failure-in-cleanup law.** A panic inside a rollback hook
  is masked, deadlined, and reported as a secondary failure, and the remaining
  hooks still run. A second panic inside a plain `defer` or `scope.guard`
  during stopped cleanup is "process-fatal" (`spec.md` "Cleanup at a process
  stop"). One marker cannot keep two laws.

## 2. Strongest peers

- **Zig `defer` / `errdefer`.**
  - Block-scoped only and lexical. Reads live locals at exit. Zero allocation.
  - `errdefer` fires only when the *function* returns an error through that
    scope, and it can capture the error (`errdefer |e|`).
  - Panics run neither.
  - No function-scoped defer: Zig rejects the loop-to-function case outright and
    makes you build a list.
- **Go `defer`.**
  - Function-scoped LIFO stack. Arguments are evaluated at the `defer`
    statement; the closure body reads live variables.
  - Go 1.14 "open-coded defers" give zero heap and a bitmask for up to 8
    defers per function, none in loops.
  - Defers in loops fall back to heap records, a well-known footgun:
    `for … { f, _ := os.Open(p); defer f.Close() }` leaks fds until return.
  - `recover` runs in defers, and panics run defers.
- **Swift `defer`.** Block-scoped, lexical, zero-cost. It cannot leave its
  scope (`return`/`break`/`throw` are banned), like Jet's E0509/E0512. No
  error-only variant.
- **Odin `defer`.** Block-scoped, lexical, statement or block, LIFO, zero-cost.
  No error variant and no function scope.
- **Rust RAII / `scopeguard`.**
  - Cleanup is a value with `Drop`: movable, storable, `ScopeGuard::into_inner`
    to disarm.
  - `scopeguard::defer!` is block-scoped.
  - `guard_on_unwind`/`guard_on_success` provide strategies, but panic-only;
    a returned `Err` is still "success".

**Lesson.** Every peer with zero-cost defers is block-scoped and lexical. The
only function-scoped design (Go) pays heap cost in loops and has a famous leak
footgun. No peer lets one defer spelling mean "read at exit" in one scope and
"snapshot at registration" in another. Jet must not be first to do so.

## 3. Attack 1 — loop-nested `.Fn` / label scope: O(N) allocation

**Attack.**
- `loop … { #Defer(scope: .Fn) { … } }` needs one record per iteration that
  outlives the iteration.
- An unbounded loop means unbounded memory and an unbounded cleanup run at
  function exit.
- It is also the Go fd-leak footgun: resources stay open until return, even
  though the author probably meant per-iteration cleanup.

**Resolution (required).**

1. **Static classification in sema, not codegen (I3).** Every `#Defer` gets a
   *registration multiplicity* from the target scope `S` and the declaring
   site `D`:
   - `Once`: no loop lies between `D` and `S`. This covers every `.Block`
     defer, since `S` is `D`'s own block.
   - `Bounded(n)`: the intervening loops have sema-known trip bounds.
   - `Unbounded`: anything else.
2. **`Once` lowers with zero heap and O(1) stack.**
   - It keeps today's `ir_defer` inline-at-exit lowering when `S` is the
     declaring block.
   - When `S` is an outer scope and registration is conditional (inside an
     `if`/`switch` arm), add one stack `Bool` "armed" slot per defer. This is
     Go's open-coded bitmask, but without Go's cap of 8, since Jet knows every
     exit statically.
   - Proof obligation: a golden whose `jet perf`/footprint shows 0 allocations
     for both block and non-loop `.Fn` defers, on O0, O1, O2, and web.
3. **`Bounded(n)`** lowers to a fixed `[Record#n]` stack array plus a count.
   Still zero heap.
4. **`Unbounded` is the only heap path, and it must be honest.**
   - The site carries the `Mem.Alloc` effect (the registry already has
     `!Mem.Alloc` / `!Mem.Alloc(above: N)` denials in `DiagnosticRows.jet`).
     A `-[!Mem.Alloc]>` function therefore rejects it with the existing
     denial diagnostic and full call path.
   - Allocation goes through the active `#Context(allocator:)`, not a hidden
     runtime malloc.
   - A tidy lint (new L-code with what/why/fix and a UI snapshot) fires on
     every `Unbounded` site. Its fix text names the two better spellings:
     a per-iteration `#Defer` (`.Block`), or collect resources into an owned
     list and register one `.Fn` defer over the list.
5. **Ballot B1.**
   - (A) Allow `Unbounded` with `Mem.Alloc` plus lint, as the card currently
     implies.
   - (B) Reject `Unbounded` with an error whose fix is the explicit-list
     rewrite. This is Zig's position: no hidden allocation, no Go footgun.
     Library code can already express it, so I8 1-in/1-out argues for B.
   - Recommendation: **B**, with A as a later opt-in if a real program needs
     it.
   - Edge cases either way:
     - Recursion does not create multiplicity, since each frame owns its own
       list.
     - `task` bodies are their own `.Fn`.
     - Comptime `#run` evaluates the same lowering (I9), so the comptime
       evaluator needs the same record list.

## 4. Attack 2 — capture safety across iterations

**Attack.**
- Today `defer` reads live places at exit. That is only sound because the
  places outlive the exit.
- A `.Fn` defer declared inside a loop body names loop locals that are dead at
  function exit. Snapshotting them at registration (Go semantics) gives the same
  marker **two capture laws**, chosen by an argument. That is a pairwise
  orthogonality violation under I8.
- `^` (write) captures held in a record keep a live mutable borrow across the
  rest of the function. Every later use of that variable then conflicts under
  two-phase borrows.

**Resolution: one law for every scope.**

- **Law C (lifetime-partitioned capture).** In a `#Defer` targeting scope `S`:
  - Every name declared in `S` or outside it is **read by place at `S`'s exit**,
    exactly as today. Writes through `^` are allowed because the defer body is
    checked as code at the exit point, not as a closure. The record holds no
    pointer; the cleanup reaches those slots through the frame.
  - Every name declared in a block strictly inside `S` (it dies before `S`
    exits) is **moved into the record when its own block ends** — a hand-off
    at the end of its own scope, not at the `#Defer` statement.
- **Why this works.**
  - It reuses today's deferred-move machinery (`deferred_depth` in
    `sema_ownership_defer_moves`).
  - The name stays usable for the rest of its block. `f: open(p);
    #Defer(scope: .Fn) { close(f) }; f.read()` stays legal, just as
    `defer close(f)` lets `f` be used until scope exit today.
  - For `.Block` scope the second clause is empty, so the law reduces exactly
    to today's `defer`. There is no behaviour change for the 159 existing
    statement-form `defer` lines.
- **Rejections (new codes, UI snapshots).**
  - An inner-lifetime name that is not movable: a borrowed view, a `Cell`
    guard, a `SharedGuard`, a region or arena value tied to the inner scope.
    Fix: move an owned value, or use `.Block`.
  - A `^` write to an inner-lifetime name. A write into a moved-out record is
    invisible to the program; reject rather than silently write to a copy.
  - An inner-lifetime name moved *after* the `#Defer` in its block. This is
    E0121-style "used after hand-off", reported at the move site, not
    silently skipped.
- **Auto-move interaction.** The hand-off counts as a last use for the auto-move
  pass. A value auto-moved elsewhere later in the block must be rejected the
  same way an explicit `^^` would be.
- **`task ^^name` / `freeze`.** A record never crosses a task. A `.Fn` defer in
  a task body targets the task's function, so there is no cross-task borrow.
- **Criterion to add.** Fixtures for loop-local `Close` resources under `.Fn`
  (closed at fn exit, LIFO), loop-local borrow (rejected), outer `^` counter
  incremented in a `.Fn` defer from a loop (sees the final value), and
  use-after-hand-off (rejected). Each must pass on O0/O1/O2/web.

## 5. Attack 3 — exact execution ordering

The card does not define what `.Ok` and `.Err` mean. The peers disagree:

- Zig `errdefer`: an error *return*, not panics.
- Rust `guard_on_unwind`: panics, not `Err`.
- Jet's own `#Transact`: *every* non-clean exit, including plain `return` and
  `break` (`spec.md` ~L2765).

So a direct `.Err == on_rollback` mapping is **false**: an early `return` from
a non-fallible function inside `#Transact` rolls back, yet it is not an error.

**Resolution: one outcome law per scope kind.** Each scope has exactly one
binary outcome decided before its defers run:

| Scope kind | `.Ok` outcome | `.Err` outcome |
|---|---|---|
| plain block, loop body, labeled loop | left by fallthrough, `return`, `break`, or `next` | left by a propagated failure or a stop (panic, trap, cancellation, deadline expiry, `process.exit`, `os.stop`) |
| function (`.Fn`) | returned normally, with an `Ok`/non-failure value | returned a failure or stopped |
| `#Transact` body (named scope, see §6) | **committed** | **rolled back** (any non-clean exit, as today) |

The transaction row differs from the block row on purpose. It is still one law:
`.Ok` means the scope fulfilled its contract. Name this law in the spec. It is
the only way `.Err` can replace `on_rollback` without a special case.

**Ordering at the exit of scope `S` (normative text to land in `spec.md`):**

1. The exit route is chosen: fallthrough, `return v` (value evaluated first),
   `break`/`next` (label resolved), a propagated failure (failure value built),
   or a stop. A stop includes task cancellation and `#Deadline` expiry; both
   arrive as stops (`IRTxFrame` notes; `ir_stop_return`).
2. Inner scopes being left unwind first, innermost out, each running steps 3–5.
   For a labeled `break outer` from three loops deep, the two inner loop-body
   scopes finish fully before the `outer` loop's scope runs.
3. If `S` is a transaction: commit (clean end) or restore snapshots latest first
   and drop buffered `Shared` writes (any other exit). The outcome is now fixed.
4. `S`'s cleanup runs strictly LIFO by registration position. Inline `#Defer`s
   of every kind, value drops/`Close`s, and records handed off from inner
   blocks (anchored at the statement of `S` that contains them) interleave in
   that order. Records anchored at one statement run in reverse hand-off order.
   For each `#Defer`, `.Always` runs, `.Ok` runs iff the outcome is Ok, and
   `.Err` runs iff it is Err.
5. Control transfers to the chosen target, or the stop keeps propagating.
   Rollback never turns a panic into a recoverable error; the existing rule
   stays.

`os.atexit` handlers run after all scope cleanup, in registration order, as
today. They are unaffected.

**Stops and masking: one law, resolving D2.** Every `#Defer` body runs as a
drop does today (`ir_run_defer` sets `stopping = true`): no stop point inside
interrupts it. Ballot B2 picks the failure law for all kinds:

- (A) Adopt today's rollback-hook law everywhere: masked, own deadline, panic
  becomes a secondary report, remaining cleanup continues. This needs
  `hook_enter`/`_leave` only on the stop path, so the fast path stays zero-cost.
- (B) Adopt the process-fatal second-panic law everywhere. This would break
  `#Transact` rollback guarantees.
- Recommendation: **A**. B silently weakens the existing transaction promise.

## 6. Attack 4 — `#Transact` interaction

- **Naming the transaction scope.** The card's scope menu is `.Block|.Fn|label`.
  Jet labels exist only on loops (`parser_parse_labeled_loop`). A `#Defer`
  nested in an `if` inside `#Transact` therefore cannot name the transaction,
  and `.Block` would silently pick the `if` body with plain-block outcome
  semantics. Ballot B3:
  - (A) `scope:` also accepts a transaction's name (`#Transact(order)` …
    `#Defer(.Ok, scope: order)`). This keeps handle names, which the card wants
    to drop.
  - (B) Add `.Transact`, meaning the innermost enclosing transaction.
  - (C) Make `.Block` resolve to the nearest transaction body when one encloses
    it. Rejected: it changes `.Block`'s meaning by context.
  - Recommendation: **B**. One value, no handle, works with anonymous
    `#Transact { … }`.
- **Nested transactions.** Sema flattens an inner `#Transact` into a `Region`
  when `transaction_depth > 0` (`sema_stmt_check_transaction`,
  `inside_transaction`). A `.Ok` defer targeting the inner transaction must
  therefore run on the **outermost** commit. Otherwise an irreversible effect
  fires and the outer transaction can still roll back. Same for `.Err`.
  Criterion: a fixture with an inner `.Ok` defer and an outer rollback, where
  the effect must not run.
- **E0746 attribution law.** Today hook bodies are exempt from the
  irreversible-effect check (`Effects/Checks.jet` `.OnCommit | .OnRollback`
  under `.TransactCollect`). Generalise: a `#Defer` body's effects are checked
  at its target scope's exit point. A body targeting a transaction scope
  (`.Ok`, `.Err`, or `.Always`) or any scope outside the transaction runs after
  the outcome is fixed, so it is exempt. A `#Defer` targeting a block *inside*
  the transaction body is still checked as transactional code, which closes a
  hole in naive designs. One rule, no special case per kind.
- **`.Fn` from inside `#Transact`.** It runs at function exit, after the
  transaction settled, and the function's outcome (not the transaction's)
  decides it. Trap: the transaction commits, a later statement fails the
  function, and a `.Fn`/`.Ok` defer meant to publish the commit never runs; the
  reverse (`.Fn`/`.Err` after a commit) compensates work that actually
  committed. Lint (new L-code) every `.Fn` defer with `.Ok`/`.Err` declared
  inside `#Transact`; its fix text teaches `scope: .Transact` for
  commit-coupled effects.
- **A failure thrown inside a defer body during unwind.** The static rule stays:
  E0512 (no propagation out) and E0509 (no exit out) apply to every
  `#Defer`. Snapshots exist: `Tests/UI/defer_{break,next,return,try}`. The
  registry text "a `defer` body" must be reworded to "a `#Defer` body". Panics
  follow B2.
- **The `Rollback` trait is not a hook. Do not delete it as card criterion 3
  says.** `impl Rollback { snapshot; restore }` customises the *automatic*
  per-value snapshot `#Transact` takes for every assigned root
  (`sema_stmt_transaction_snapshots` → `IRTxSnapshot.rollback/restore`).
  - Deleting it leaves values that cannot be copied, or are expensive to copy,
    with no rollback strategy.
  - Hand-written `#Defer(.Err) { x.restore(saved) }` cannot replace it: the
    auto-snapshot would *also* run and restore over the user's restore, unless
    sema infers "handled" values from defer bodies.
  - That inference is a silent hazard: a `.Err` body that merely *logs* `x`
    would disable `x`'s rollback (D1 shows the inference is not even
    implemented for `on_rollback` today).
  - Ballot B4:
    - (A) Keep `Rollback` as the per-type snapshot strategy. It is a trait, not
      a cleanup mechanism, so I8 is unaffected.
    - (B) Delete it and require an explicit per-value opt-out spelling. That is
      new surface, so it fails 1-in/1-out.
  - Recommendation: **A**. Resolve D1 by deleting the spec's opt-out sentence:
    with no `on_rollback`, nothing can opt out.
- **`#Undo(inverse)`.** This is a foreign-binding contract that registers on the
  rollback path (`transact_ffi_with_undo_registers_rollback_hook.case`).
  Deleting `on_rollback` must re-lower it as a compiler-generated
  `#Defer(.Err, scope: .Transact)` record capturing the call's arguments. That
  is a record because the call can sit in a loop, so it takes the §3 path; under
  B1-B, `#Undo` inside an unbounded loop needs an explicit decision. Keep
  `#Undo` (it is a foreign contract, like `#Close`) and list it in the cutover.
- **Order migration hazard.** Today commit/rollback hooks run in the handle's
  drop. The handle is bound first in the scope, so hooks run *after* every
  `defer` in the transaction body. Under §5 step 4 they interleave by position.
  `t.on_commit(A)` followed by `defer B` keeps the order (B, then A). But
  `defer B` followed by `t.on_commit(A)` flips from B,A to A,B. `jet fix` must
  hoist migrated hooks to preserve observed order, or the card must ballot the
  order change. Fixture: `Tests/Run/FailureFoundation/TransactExitKinds.jet`
  plus a mixed-order case.

## 7. Attack 5 — `#Close` and the deletions

- **Moving `#Close` "into an external C FFI library" is not viable as stated.**
  - `#Close` *is* the `Close` trait applied to a foreign handle
    (`MarkerModules.jet`: marker and trait of one name are one thing).
  - Its validity check is a sema rule with a registered code (E0702,
    `sema_checked_foreign_close`, including the provider close-adapter path).
  - A library cannot own a sema check (I3), and markers come from the Prelude
    registry.
  - It is also **not a scope-cleanup mechanism**; it belongs to the
    `impl Close` axis, which the card keeps.
- **Steel-man of the intent:** "fewer cleanup spellings in the Prelude". The
  correct collapse is to **delete the `#Close` marker** and have the C header
  binder (`CHeaderBind.jet`, `Jetpack/PackageModel/.../HeaderBinder.jet`,
  `BindingRender.jet`) emit `impl Close` on the opaque handle type, calling the
  consuming foreign function.
  - Sema then checks it as any `impl Close`; E0702's foreign-close wording
    retires or rewords.
  - That is a true 1-out with no capability loss.
  - Ballot B5 (it changes the binding format): (A) delete the marker and have
    binders emit `impl Close`, or (B) keep `#Close` in the Prelude unchanged.
    Recommendation: A. Do not "move to a library".
- **Deleting the `ScopeGuard` value loses movability.** Today a guard can be
  stored (`Bundle<ScopeGuard>` in
  `Tests/Cases/PatternsAndFields/OwnedDestructurePreservesObservableDropOrderOnEveryTier`
  and its fuzz twin). `#Defer` is lexical-only. The replacement for a movable
  cleanup is a user type with `impl Close`, which already exists, so there is
  no capability loss. The drop-order tests must switch to such a type, not be
  deleted.
- **Single-statement verbosity.** `defer close(f)` (120 of the 159
  statement-form lines) becomes `#Defer { close(f) }`. The parser must keep the
  `DeferClose` recognition (Close bound E0905, resource id, deferred move) for a
  one-statement `#Defer` body whose statement is `close(name)`. Otherwise the
  resource tracker loses its main input.
- **Marker schema.** `scope:` takes either an enum case or a bare label/name.
  `Markers.jet` field kinds are typed (`Ident`, `Value`, an enum). A union field
  needs a new marker-field kind, which is a compiler special case under I8's
  pairwise gate. B3-B (`.Transact`) plus "label requires `scope: label(name)`"
  or an enum-only menu avoids it. Decide alongside B3.
- **Ratification gap (I7).** `#Defer`, the `when:`/`scope:` field names, and the
  case spellings `.Always/.Err/.Ok/.Block/.Fn` are new user-typable surface. The
  card says "owner-ratified", but no Tower decision is attached and none was
  found. A ballot (or the owner's recorded instruction) must back the exact
  spelling before registry rows land.

## 8. Binding cutover list

Counts are tracked files from `git grep`, excluding `Tools/Tower/.tower/`.

**Syntax registry and Prelude**
- `CoreSurface.jet`: delete `KW_DEFER` and reword `RESOURCE_CLOSE`'s doc.
- `EffectsSurface.jet`: delete `TXN_ON_COMMIT`/`TXN_ON_ROLLBACK`; reword the
  `KW_TRANSACT` doc (it names `order.on_commit`).
- Add `#Defer` rows.
- `Jetpack/Foundation/Source/SyntaxRegistry.jet` mirror.
- `Markers.jet`: add `Defer`; delete `Close` if B5-A.
- `Traits.jet` `Rollback`: keep if B4-A.
- `Core/Mem/Scope.jet`: delete.

**Parser and formatter**
- `parser_parse_defer_statement`/`parser_defer_close_call` move to marker-block
  parsing.
- Formatter `Items.jet` (`#Close`) and the formatter's defer printing.
- `Foreign.jet` `#Close` parsing.

**AST, Sema, IR exhaustive matches**
- The `.Defer(` variant appears on 216 lines in 87 files across
  Compiler/Jetpack/Tools.
- `DeferClose` appears in 88 files.
- Every arity change touches them.
- Closure kinds `Guard`/`OnCommit`/`OnRollback` appear in 14 files, including
  `Tree.jet`, `BodyFlatten.jet`, `Lambdas.jet`, `Methods.jet`, `Direct.jet`,
  `Effects/Checks.jet`, `Assignments.jet`, `Footprint.jet`, `Body.jet`, and
  `BodyCodec.jet`.
- IR: `ir_defer`/`ir_run_defer` (extend), `ir_scope_guard` and
  `BuildCoreClosures.jet` guard drop (delete), `ir_transaction_hook*` and the
  hook lists in the `Transaction` box (replace with scope records; the handle
  box shrinks).
- `JetRuntime/SharedTransactions.jet` `hook_enter/leave`: keep and reuse for B2.
- The comptime evaluator, `LocalValues.jet`, and `Reflection.jet` must execute
  the same law (I9).
- The web backend handles the `ScopeGuard` drop (`JetWeb/Select.jet`) and needs
  the record path.

**Diagnostics** (`Compiler/Prelude/Diagnostics.jet`, `DiagnosticRows.jet`,
`Tests/diagnostics_census.tsv`)
- Reword E0509/E0512, E0419 (drops its `ScopeGuard` example), E0702 (if B5-A),
  and E0746 (fix text names `on_commit`).
- Add codes for §4 rejections, the B1 lint or error, and the §6 `.Fn`-in-
  `#Transact` lint.
- Regenerated site pages: `Docs/Site/Dist/e/Jet.DeferLeavesItsBlock`,
  `Jet.DeferPropagatesFailure`, `Jet.IrreversibleEffectInTransaction` (generated;
  regenerate, do not hand-edit).

**Callers**
- Statement `defer`: 159 lines in 81 `.jet` files (Core 16, Compiler 39,
  Examples 12, Jetpack 22, Tests 69, Tools 1). 120 of those lines in 64 files
  are `defer close(`.
- `scope.guard(`: 51 lines in 26 `.jet` files; 39 files total including docs.
  Core uses: `Core/Files/Files.jet` (2), `Core/Term/Term.jet` (1).
- `on_commit`/`on_rollback`: 49 files.
- `impl Rollback`: 9 files. These stay if B4-A.
- `#Close(`: 21 files, including `.jet/bindings` goldens under
  `Examples/Interop/FFI/OpaqueHandles` and the Cffi test fixtures.

**Examples and goldens** (I5)
- `Examples/Memory/ScopeGuard.jet`/`.out` (rewrite as `#Defer`) and its README
  rows in `Examples/README.md` and `Examples/Memory/README.md`.
- `Examples/ErrorsAndEffects/Errors/Transact.jet`/`.out` and `MustUse.jet`.
- `Examples/Memory/Shared/SharedTransact.jet` and
  `Examples/Systems/Process/ProcessExitCleanup.jet`.
- New goldens showing the 0-allocation footprint (§3) and the ordering trace
  (§5) on every tier.

**Tests**
- `Tests/UI/defer_*`.
- `Tests/UI/scope_guard_discarded*`.
- `Tests/UI/Effects/transact_on_{commit,rollback}_*`,
  `transact_fs_in_on_commit_ok`, and the `transact_*_e0746.stderr` fix texts.
- `Tests/Run/FailureFoundation/Transact*`.
- `Tests/Cases/Effects/TransactOnCommit*` and
  `transact_ffi_with_undo_registers_rollback_hook`.
- `Compiler/JetSema/Tests/TransactOnCommitFixtures.jet` plus fixtures.
- `Compiler/JetIR/Tests/CoreOwnerGuardFixtures.jet`.
- The Comptime `NativeO0ClosureKinds` fixture.
- The `ClosureBoxesFreeAtTheirLastOwner` fixture.
- The fuzz corpora twins.

**Tools**
- Editor grammars (`Tools/Editors/TreeSitter/grammar.js` plus generated
  `grammar.json` and corpus, `Vscode/Syntaxes/jet.tmLanguage.json`,
  `Zed/.../highlights.scm`, `jet.tmGrammar`).
- `Tools/Perf/BuildTime/Units/Source/Midsize.jet` and `Midsize.mjs`.
- `Tools/Tower/App/UI/tower.js` (verify whether this is a Jet-keyword
  highlighter).
- Gauntlet Go/Zig entries are peer code: leave them.

**Docs**
- `spec.md` "Cleanup at a process stop" and the `#Transact` section (resolve D1
  and D2).
- `Docs/Spec/Reference/syntax-surface.jet`.
- `Docs/Spec/Contributing/examples.md`.
- `Docs/Proposals/ffi-perfect-boundary-2026-10-31.md` (`#Close`), if still a
  live proposal.
- Dated audits and research stay as written.

**`jet fix` migration**
- Hoist-order preservation (§6).
- `scope.guard` bindings become `#Defer` blocks only when the guard is never
  moved; otherwise migrate by hand to an `impl Close` type.
- Migration is rejected for hooks whose closure captures by value at
  registration in a loop: under Law C those values hand off at block end, which
  is observably different if mutated after registration.

## 9. Ballots and criteria owed before build

| Ballot | Question | Recommendation |
|---|---|---|
| B1 | `Unbounded` loop-nested outer-scope `#Defer`: allow with `Mem.Alloc` plus lint, or reject with an explicit-list fix | Reject (Zig position) |
| B2 | One failure-in-cleanup law: masked, secondary report (hook law) or process-fatal | Hook law for all |
| B3 | How `#Defer` names a transaction scope: handle name, `.Transact`, or contextual `.Block` | `.Transact` |
| B4 | `Rollback` trait: keep as snapshot strategy, or delete plus explicit opt-out | Keep; fix the D1 spec sentence |
| B5 | `#Close`: delete and have binders emit `impl Close`, or keep in the Prelude | Delete and emit `impl Close`. Never "move to a library" |
| B6 | Exact spelling of `#Defer`, its fields, and its cases (I7), plus the hook-order change if `jet fix` cannot hoist | Ratify before registry rows |

Criteria to attach to #7063, verbatim scope:

1. The outcome law table (§5) and exit-ordering text land in `spec.md`, with an
   ordering-trace golden on O0/O1/O2/web/comptime. The golden covers a labeled
   `break` across 3 loops, `return`, a propagated failure, panic, cancellation,
   and `#Deadline` expiry.
2. A 0-allocation footprint golden for `.Block` and non-loop `.Fn` `#Defer` on
   every tier, plus the `Bounded(n)` stack-array case.
3. Law C capture fixtures (§4, 4 cases) with UI snapshots for each new code.
4. Nested-transaction `.Ok`/`.Err` fixture: the inner defer fires only on the
   outermost outcome.
5. E0746 attribution fixture: a `#Defer` targeting an inner block inside
   `#Transact` doing `FS.Write` is rejected, while `scope: .Transact` with
   `.Ok` is accepted.
6. `#Undo` re-lowered onto the record path, with the existing
   `transact_ffi_with_undo` case still passing.
7. D1 and D2 resolved in spec and code, in the same batch.
8. Card criterion 3 reworded via ballot (B4/B5). As written it orders deletion
   of `Rollback` and relocation of `#Close`, which this review finds unsafe.

## 10. Claims attacked and their status

| Claim | Status |
|---|---|
| "One marker replaces seven mechanisms" | Partly. `Rollback` (snapshot strategy) and `#Close` (foreign `impl Close`) are not scope cleanup; folding them in loses capability. Real collapse: `defer`, `DeferClose` (as a user form), `scope.guard`, `on_commit`, `on_rollback` into `#Defer`. |
| "`#Defer(.Err)` ≡ `on_rollback`" | False without the per-scope outcome law and a transaction scope name (§5, §6). |
| "O(1) stack for block and non-loop `.Fn`" | Achievable: today's inline lowering plus an armed bit, statically classified in sema. |
| "Safe loop-nested cleanup stack" | Achievable only under Law C, plus `Mem.Alloc`/context allocator, plus lint or rejection (B1). |
| "Move `#Close` to a C FFI library" | Not viable under I3/I4; the replacement is binder-emitted `impl Close` (B5). |
