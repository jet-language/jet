# Pre-build adversarial review: #7062 — uniform `^` marks and capture law, one conversion lattice, `TypeId` → `Type`

Date: 2026-11-27. Scope: criterion 1 of #7062. This review is read-only. It
changes no code and records no work state. Counts come from `awk` sweeps over
every tracked `*.jet` file (`fd -e jet`, 23,022 files, `Tools/Tower/.tower`
excluded), plus `Docs/**/*.md` where noted. Mark counts come from regexes and
are approximate; each method is stated next to its number. No build or test
was run. Every runtime claim below is read from source.

Sources read in this session:

- `node Tools/Tower/tower.mjs card show 7062` (also 7053 and 7047, for the
  refinement links).
- `Docs/Audits/2026-11-27-cognitive-budget-orthogonality-audit.md`: O2, O13,
  O19, O35, T1–T5, T13, T14, and summary rows 4, 5, 19 and 24.
- `Docs/Spec/spec.md`: 1884–1929, 1951–1980, 2082–2138, 2328–2338,
  2398–2406, 2948–3000, 4350–4369, 5184–5204.
- `Compiler/JetFoundation/Data/Syntax/Spellings.jet`: 74–76.
- `Compiler/JetSema/Source/Sema/Ownership/CallMarks.jet` (whole policy path).
- `Compiler/Prelude/Diagnostics.jet`: `CallMarkRequired` (E0228, 4329–4352)
  and `NestedFnFailureMismatch` (E0638, 9791–9816).
- `Compiler/JetSema/Source/Sema/Expressions/Lambdas.jet`: capture facts,
  2573–2946.
- `Compiler/JetIR/Source/BuildCoreClosures.jet`: 126–138.
- `Compiler/JetIR/Source/BuildExpr.jet`: 8644–8750.
- `Compiler/JetIR/Source/BuildTransact.jet`: 27–37.
- `Compiler/JetParser/Source/Parser/Items.jet`: 3303–3320.
- `Compiler/Prelude/Errors.jet`: 95–110.
- `Compiler/JetSema/Source/Sema/Registration/PreludeDerives.jet`: 173, 267+.
- `Compiler/JetFoundation/Source/Facts/Comptime.jet`: 93–170.
- `Core/Prelude/Prelude.jet`: 180–196 and 377–386.
- `Compiler/JetIR/Source/CompilerOwned.jet`: 182.
- `Compiler/JetEval/Source/IR/Host.jet`: 340–363.
- `Compiler/JetSema/Source/Sema/Expressions.jet`: 8193–8233.
- `Compiler/JetCLI/Source/CLI/FormatMarks.jet`: 155–248.

---

## Part 1 — Call-site access marks and the closure capture law

### 1.1 The contradiction has four parts, not three

The orthogonality audit (O19) lists three conflicting sources. The live code
adds a fourth:

| # | Source | What it says |
|---|---|---|
| A | `spec.md:2333`, `:2402`, `:2953`; example at `:2975` (`damage(player, 3)`) | "Calls carry no marks; the signature decides." |
| B | `spec.md:2995-2996` | Marks appear on receivers *and* arguments: `^buf.push(x)`, `fill(^xs)`. |
| C | `Spellings.jet:75`; E0228 `why` (`Diagnostics.jet:4345`) | `^` is **required** on every changed named argument. Receivers stay unmarked. `^^` is optional. |
| D | `CallMarks.jet:1-8`, `:131-134`, `:225-233`, `:317-327` | A missing `^` is reported **only** under `#Policy(writes: .Explicit)`, described as a "staged cutover". Outside that policy, a receiver mark is E0225 ("method receivers are never marked in Jet", `:266`). **Inside** the policy, receivers are checked by `sema_call_marks_mismatch` (`:261`), which can require a receiver mark. |

What this means:

- E0228's registry text (C) states a rule the compiler does not enforce by
  default (D). That violates I4: registry text must match the rule actually
  shipped.
- `#Policy(writes: .Explicit)` is a second mechanism for one meaning, which
  I8 forbids. It appears on 32 lines in 18 `.jet` files (`awk
  '/writes: *\.Explicit/'`).
- Receiver marks are illegal in the default mode but checked under the
  policy. The same source text means different things depending on a policy
  switch. That is an I8 pairwise-orthogonality defect.

Size of the migration (regex approximations):

- Argument `^name` marks (`[(,] *\^[a-z_][A-Za-z0-9_.]*[,)]`): 2,149 lines in
  390 files.
- Argument `^^name` marks: 496 lines in 317 files.
- Receiver marks at the start of an expression
  (`\^[a-z_]\w*\.[a-z_]\w*\(`): 12 lines in 7 files. Some of these are
  inside diagnostic strings and test fixtures.

### 1.2 Recommended law, and attacks on it

**Law L1 (recommended):** every *named place* that a call may change or take
carries its mark, in every position. This covers arguments (`grow(^xs)`),
receivers (`^xs.push(4)`), forwarded parameters, and moves (`send(^^msg)`).
A mark goes only on a named place. Computed values and temporaries never take
one, which matches the existing E0225 `NotNamedPlace` logic.

`jet fmt` inserts the marks. It already does this for arguments: see
`FormatMarks.jet:248` (`skip_receivers: true`) and `AuditWrites.jet`. The
policy flag is deleted.

The alternative, **L0** ("no marks; the signature decides", source A), is
also coherent. Its cost: 2,645 written marks get deleted, and writes become
invisible in diffs.

The choice between L1 and L0 is a syntax decision, so it is an **owner
ballot**. The current hybrid is not a valid option: as shown in 1.1, it is a
defect under both I4 and I8.

Attacks on L1:

1. **Builder chains.** In `^b.add(x).add(y).build()`, only `b` is a named
   place. The first link carries the mark. Later links act on temporaries and
   take no mark (`NotNamedPlace`). If the first method takes `^^self`, the
   chain head is `^^b`.
   - Edge case: in `^cfg.items.push(x)` the mark covers the root `cfg`. That
     matches today's rule that marks name the root (`sema_receiver_mark_place_root`).
   - Residual noise: one sigil per statement whose head is mutated. This is
     acceptable, because `jet fmt` writes it.
2. **Auto-move into `^^`.** O19 and T14 say this "refines #7053". **The cited
   link is wrong.** #7053 is "structural Copy for small plain-data structs" plus
   "zero-refcount `Shared<T>` borrow in scoped tasks". It does not cover
   auto-move. The real interaction is a hazard:
   - Under L1, `^^` is mandatory, and `^^` on a Copy type is an error
     (`CallMarks.jet:348-351`, `MoveCopyType`).
   - #7053 infers Copy structurally (size and field-dependent).
   - So adding a field to a struct can flip it between Copy and move-only.
     That turns every `^^s` call site into an error, or every plain `s` into a
     missing-mark error.
   - Required fix: a `^^` mark on a structurally Copy value must be **accepted
     as a no-op copy**, not rejected. Otherwise call-site marks depend on
     layout. Today E0225/E0228 reject it, so this is a binding cutover item.
   - Separately: `spec.md:2337` ("a cloneable read value entering an owning
     destination is materialized automatically") is a hidden clone at a `^^`
     slot. Under L1, an unmarked argument into a `^^` parameter must be either
     a Copy value or a diagnosed error with fixes `^^x` / `x.copy()`. It must
     not be a silent clone. This needs to be on the ballot.
3. **Forwarded `^` parameters and generic callees.** E0228 already covers
   forwarding. Generic callees that write through a `T` bound have marks that
   depend on the signature, which is fine under L1. A marked argument into a
   callee that is generic over access is not expressible today, so there is no
   new hole.
4. **`fn(^T)` function types (T4).** `spec.md:1888-1890` says a function with
   `^`/`^^` parameters is direct-call-only (E0227). `Spellings.jet:76` says
   `fn(^T)` types exist and fit when the marks match, and that calls through
   them mark the argument (`st.action(^m)`).
   - The registry is the source of truth for syntax, so the spec paragraph is
     stale. Cutover: rewrite `spec.md:1887-1890` to match the registry.
     Without this, criterion 2 cannot be met.

### 1.3 Removing the hidden invocation lock: soundness

**What exists today:**

- A lambda whose capture facts have `needs_fn_mut` (it writes a capture) gets
  a runtime word mutex in its box (`IR_CLOSURE_LOCK`,
  `BuildCoreClosures.jet:131-138`).
- **Every** call of such a lambda takes that mutex, except task bodies
  (`ir_lambda_locked`, `BuildExpr.jet:8748-8750`). That includes purely
  single-threaded calls.
- Early exits unwind the lock through a dedicated transaction frame
  (`IRTxFrame.closure_lock`, `BuildTransact.jet:35-37`).
- So the cost is runtime overhead on every call of every writing closure. That
  violates philosophy priority 3 (no hidden overhead) as well as T3.

**Attack: plain deletion is unsound.**

- `needs_fn_mut` is a fact about the *lambda*, not about its *type*. Once a
  writing lambda is passed as `f: fn()`, the callee only sees a read-access
  function value.
- Read access can be shared: two `task.group` children may capture `f` by
  read borrow (`spec.md:5186-5204`: group children borrow).
- Without the lock, two children calling `f` concurrently race on the
  captured state. The lock is the only thing that makes this safe today.
- The orthogonality audit's T3 law ("a closure that writes captured state is
  not sendable") cannot be checked at that point. The type `fn()` does not say
  whether the closure writes.

**Sound replacement: the access mark on the function value carries "may write
captures".** This is Rust's `&F` / `&mut F` (`Fn` / `FnMut`) split, expressed
with Jet's existing `^`. It adds no new syntax.

- **R1.** Calling a function value whose lambda writes captures requires `^`
  access to that value. A writing lambda fits only a `^fn(…)` parameter or
  binding (or `^^` ownership). A read slot `fn(…)` accepts only lambdas that
  do not write captures. The new diagnostic names the written capture and
  offers `^f`.
- **R2.** `^` is exclusive. At most one group child, or the owner, holds `^`
  on the value at a time. Any second use is E0204. Concurrent calls become
  unrepresentable, so they need no lock.
- **R3.** Reentrancy: a call holds `^` on the closure for its duration, so
  passing the closure to itself during the call is a borrow conflict. Direct
  self-recursion through the binding is already E0804.
- **R4.** Shared mutable state uses `Shared<T>`. The lambda then reads only
  the handle, so `needs_fn_mut` is false and no lock exists. The guard is
  visible at the use site. This connects to #7053 part 2 (Shared borrow in
  scoped tasks), which lowers the cost of this path.
- **R5.** A detached `task f()` already takes the value (E0121,
  `spec.md:1977-1978`). Nothing changes there.

With R1–R5, deleting `IR_CLOSURE_LOCK`, `ir_lambda_locked`, and
`IRTxFrame.closure_lock` is sound. Every interleaving the lock serialized is
now rejected statically.

**Cost of R1 (must be measured before the build):**

- Every higher-order function in Core that calls a callback parameter must
  decide between `fn` and `^fn`.
- Every struct field holding a writing callback that is called from a
  `self`-read method then needs `^self`.
- The sweep census has not been done. It is a criterion (C1-4 below).
- `xs.map(f)` with a read `f` will reject a writing lambda. Today it accepts
  one and clones (`spec.md:1953-1964`), so this is a behavior change. It must
  get its own ballot item, with the alternative "`map` takes `^f`".

**Capture law (T2):**

- T2's "a capture is a parameter" removes the creation-time copy plus E0204
  trap for local non-escaping lambdas (`spec.md:1958-1961`). A read capture
  becomes a read borrow that lives as long as the lambda's last use.
- This depends on the borrow checker handling borrows held by lambdas. That is
  already partly true: the non-escaping write path borrows
  (`Lambdas.jet:2644-2646`, `:2721`).
- Escape converts a borrow to ownership. The conversion is visible (`^^` or
  `.copy()`) and is diagnosed at the escape point.
- The silent "writes its own copy" rule for escaping lambdas
  (`spec.md:1971-1973`) must be deleted. It is the same hidden-semantics class
  as the lock.

### 1.4 Binding cutover list (criterion 2)

1. Delete `#Policy(writes: .Explicit)` handling:
   - `CallMarks.jet:34-82` and the `writes_explicit` branches at `:132` and
     `:227`.
   - The `PolicyKey.Writes` `.Explicit` value, if nothing else uses it.
   - All 18 files that use the policy.
2. Rewrite E0228 `meaning`, `why`, `fix` and `safety_reason`
   (`Diagnostics.jet:4329-4352`) to match the chosen law. Under L1, also
   reword E0225's receiver text (`CallMarks.jet:264-272`).
3. Make `^^` on a structurally Copy value a no-op rather than an error
   (`CallMarks.jet:348-351`), or ballot the alternative.
4. Change `FormatMarks.jet:155-158` and `:248`: `skip_receivers` goes away
   under L1. Update `AuditWrites.jet:62-70` the same way.
5. Write a `jet fix` migration (successor to
   `Compiler/Migrations/MarkCaret1.jet`) that inserts receiver and `^^` marks,
   or deletes all marks under L0. Run it on all 23,022 `.jet` files and the
   `Docs/**/*.md` code blocks.
6. Spec: reconcile `:2333`, `:2402`, `:2953`, `:2975`, `:2996` and
   `:1887-1890` to one rule.
7. Delete the invocation lock:
   - `IR_CLOSURE_LOCK` (`BuildCoreClosures.jet:131-138`).
   - `ir_lambda_locked` and the `locked` offset (`BuildExpr.jet:8647-8656`,
     `:8744-8750`).
   - `IRTxFrame.closure_lock` (`BuildTransact.jet:35-37`).
   - The `spec.md:1974-1977` sentence.
8. Add the R1 sema rule with a new registered diagnostic and a UI snapshot.
   Sweep Core higher-order functions and callback fields.
9. Golden example, per I5: two group children with a writing closure must be
   rejected statically. The `Shared<T>` version must pass on O0, O1 and O2.
   The output must show the diagnostic and the shared-state result.
10. UI snapshots for E0228, E0225 and the R1 code.

---

## Part 2 — One conversion lattice

### 2.1 Census

| Item | Count (method) |
|---|---|
| `impl A -> B {` declarations | **106 lines** (`/^\s*impl\s+[A-Z]…\s+->\s+[A-Z]…\s*\{/`): Compiler 71, Jetpack 32, Tests 2, Core 1 |
| — of which twin copies | `Compiler/Prelude/Errors.jet` has 34 and `Compiler/JetSema/Source/Sema/Registration/PreludeDerives.jet` has 34 **identical** `impl <CoreError> -> Err` blocks (`PreludeDerives.jet:173`, `:267+`). One source of truth is already violated, independent of this card. |
| `impl X.From(` declarations | 33 lines in 32 files |
| `ErrorConv` references (`.jet`) | **450 lines in 151 files**: JetSema 282, JetCLI 30, JetParser 27, JetFoundation 24, JetDriver 23, JetSemIndex 12, Jetpack 24, Prelude 8, others 20. Most are exhaustive `Item` match arms that disappear when the variant is deleted. |
| `impl A -> B` in `Docs/**/*.md` | 16 lines |

The orthogonality audit's evidence list (O2: "`Core/Crypto/*.jet`") does not
match this sweep: Core has exactly 1 declaration, in `Core/DB/DB.jet`. The
Crypto claim is stale. Use these counts.

### 2.2 How `jet fix` rewrites `impl A -> B` to `impl B.From(A)`

Shape today (`Errors.jet:101-103`, `Items.jet:3303-3319`):

```
impl AppError -> Err { return Err("{self}") }
```

The body sees the source value as `self`. Target shape:

```
impl Err.From(AppError) { from: fn(error: AppError) -> Err { Err("{error}") } }
```

Attacks on the rewrite:

1. **`self` → parameter rename and capture.** The rewrite has to rename
   `self` to a fresh parameter name. If the body already binds that name, a
   naive rename captures it. Rule: pick a name that is not free in the body
   (`error`, then `error1`, …). The fix is Safe only when the rename is
   hygienic.
2. **Orphan rule.** E2406 requires the declaration to be owned by the source
   or target side. Today `impl A -> B` may live wherever the parser accepts
   it. The fix must check every one of the 106 sites against E2406 before it
   is graded Safe. Jetpack's 32 sites cross package boundaries most often.
3. **Markers.** `ErrorConvDef.markers` must carry over onto the `impl` item.
4. **Generic sources.** `spec.md:2120-2123` (bare generic name, applies to
   every instantiation) must hold for `From` exactly as it does for `->`.
   This needs a guard test.
5. **The PreludeDerives twin** must collapse to one source: either the
   prelude file or the derive generator, not both.

### 2.3 `!` propagation: `From` plus subtyping only

After the cutover, `!` converts along exactly two edges:

- **(a) Subtyping:** union membership widening (`spec.md:2133-2138`) and
  `Never`-failure ≤ `E`.
- **(b) A declared `From` whose target is a failure domain**
  (`spec.md:2082-2102`).

The `#Error` auto-lift into `Err` (`spec.md:2104-2118`) must become the
**generated** `impl Err.From(T)` that the `#Error` derive emits. It must not
remain a separate sema path.

Attacks:

1. **Two candidate routes.** If both a union-membership edge and a declared
   `From` apply, the spec says "a declared conversion wins". That is an
   implicit priority rule, and it must be kept explicit. Simpler alternative:
   E2405 (duplicate) also fires when a declared `From` duplicates a subtyping
   edge. Ballot item.
2. **Value-type `From`.** `impl Float.From(Reading)` must never become a
   failure route (`spec.md:2100-2101`). The lattice must key on "the target
   is a failure domain", decided once in sema. It must never depend on which
   of the 6 implicit paths (O13) was spelled.
3. **The `#As` field forwarding and distinct forwarding paths (O13)** are out
   of #7062's criteria. Say so explicitly when closing. Do not claim O13 is
   closed.

### 2.4 E0638 asymmetry → subtyping

Today:

- A single function value widens from non-failing to failing at a pass, store
  or return (a coercion).
- Functions inside lists, maps, optionals, tuples and results do not widen
  (`spec.md:1916-1929`). E0638 is graded Soundness, because the two forms have
  different calling conventions (`Diagnostics.jet:9799`).

**Attack: subtyping alone does not fix this.**

- Covariant widening of `[fn(Int) -> Int]` to `[fn(Int) -> Int Err!]` needs
  the two forms to share one **runtime representation**. Otherwise a
  read-only list view would call a non-failing function with the failing ABI.
- Options:
  - **(i)** Make the non-failing ABI a strict prefix of the failing one, with
    a zero failure word. This is one calling convention and makes widening
    free.
  - **(ii)** Widen by mapping the container. That allocates, so it is
    rejected as hidden cost.
  - **(iii)** Keep E0638 and document it. That is not a fix.
- Only (i) makes `Never` ≤ `E` true subtyping with no runtime work. It is a
  backend change across O0, O1, O2 and web.
- Variance: widening is allowed through covariant positions (read containers,
  results) and rejected through `^` containers and parameters (invariant,
  T5). The rejection reuses E0638 with reworded text.
- Ask the owner whether to ballot (i), because it is an ABI change (I9).

### 2.5 Binding cutover list (criterion 3)

1. Delete the `Item.ErrorConv` variant, `ErrorConvDef`, and the parser branch
   (`Items.jet:3303-3320`, plus the `is_error_conversion` detection). Delete
   all 450 reference lines in 151 files.
2. `jet fix` migration (per 2.2) over all 106 sites plus 16 docs lines. Then
   collapse the `PreludeDerives.jet` twin.
3. Turn the `#Error` auto-lift into a generated `From` impl. Delete the
   separate sema route.
4. Change the E0638 rule to variance-based subtyping, with the ABI decision
   from 2.4. Reword E0638 and refresh its UI snapshot.
5. Spec: `:1916-1929`, `:2082-2138`. Remove the `impl A -> B` spelling from
   `Spellings.jet` if a row exists. Ratify the deletion on Tower (I7).
6. Golden example (I5): a `!` chain through a declared `From`, a union
   widening, and a generated `#Error` lift. Also `run_all([ok])` accepted.
   Outputs must show each conversion's report text on O0, O1 and O2.

---

## Part 3 — Unify `TypeId` into `Type`

### 3.1 What exists

There are two unrelated representations.

**Runtime `Type`** (`Core/Prelude/Prelude.jet:190-196`):

- It is `struct { priv path: String }`, with `Display` printing the path.
- Equality and map keys therefore compare **strings**. Each runtime `Type`
  value carries a heap-or-static path string.

**Compile-time `TypeId`:**

- It is a compiler-owned record with the members `identity`, `name`,
  `signature`, `generic_bindings`, `kind` (`CompilerOwned.jet:182`).
- At comptime it is represented as a `{handle, table}` pair pointing into the
  interner (`Comptime.jet:93-122`).
- Comptime IR writes the full record into native memory
  (`Host.jet:340-363`).
- It is referenced by 13+ `CoreRecordFieldRow` entries (`CompilerParam.type`,
  `TypeInfo.identity`, `LayoutField.type`, …).

`TypeId` census: **299 lines in 70 `.jet` files** (Compiler 248, Tests 23,
Core 16, Examples 12) and **100 lines** in `Docs/**/*.md`. For scale, `Type{`
appears on 441 lines in 97 files, `type_of(` on 36, and `.reflect()` on 705
lines in 196 files.

### 3.2 Unified design, with a zero-RTTI guarantee

There is one prelude `Type`. Its *value* is a compile-time type-row handle.
Its *lowering* depends on which members a program reaches:

| Use | O0 / comptime | O1 / O2 / web lowering |
|---|---|---|
| `.kind`, `.fields`, `.variants`, `.reflect()` | Read from the interner row through the handle (as `TypeId` does today) | **Comptime-only members.** Reaching them from runtime code is a sema error (new code, or reuse of the comptime-only-call code if one is registered). They never lower. |
| `==`, map key, `Hash` | Handle equality | A per-program **dense integer** (or the address of a static per-type descriptor), interned at link time |
| `Display` | Path from the row | A static string table that is emitted **only if** `Display`/`.path` on `Type` is reachable (`CoreReach`) |

Attacks:

1. **RTTI leakage.** If a runtime `Type` stores the comptime record (as
   `Host.jet` materializes `TypeId` today), every map keyed by `Type` drags in
   `signature`/`generic_bindings` tables.
   - Guard: the runtime lowering of `Type` has **exactly one word**. A binary
     that uses only `==` or map keys has **zero** path strings and zero
     descriptor tables.
   - Proof: a two-program size check. Program 1 uses `Type` keys only. Program
     2 adds `print(t)`. Program 1's binary must contain no type-path strings.
     Measure with `jet build` plus a symbol and section dump, not by
     inspection.
2. **Identity across separate compilation.** A dense integer is unstable
   across packages and incremental builds. Equality must be decided at final
   link, not per module.
   - The interner's canonical key (`Comptime.jet:93-96`) is the identity
     source.
   - Web has no linker step of the same shape. The web lowering needs an
     explicit interning pass, or a criterion that states why it does not.
3. **Generic instantiation.** `Type{Box<V>}` equals `Type{Box<Int>}` at
   `V = Int` (`spec.md:4359-4362`). Inside a shared-code generic, the handle
   must come from the instance's dictionary or monomorph, never from a
   runtime path-string build. Comptime already does this through
   `BodyRequests.jet:451-535`.
4. **Comptime → runtime boundary.** `#run` producing a `Type` that flows into
   runtime code must lower to the runtime word only. Today a `TypeId` crossing
   that boundary carries the whole record. Rule: crossing strips the value to
   its identity, and any comptime-only member read after crossing is a sema
   error.
5. **Equality semantics change.** Path-string equality can conflate two types
   whose displayed paths collide (for example, same module path from two
   package versions). Handle identity fixes this. That is a behavior change,
   and it needs a golden.
6. **Merging worker tables.** `Comptime.jet:124-170` renumbers `TypeId`
   handles when worker tables merge. The unified `Type` inherits this.
   Renumbering must also cover `Type` values inside comptime-built constant
   data that reaches runtime.

### 3.3 Binding cutover list (criterion 4)

1. Rename every `Type.Named{name: "TypeId"}` site to `Type`:
   - `Expressions.jet:8193-8233`.
   - `FactReads.jet` (8+ sites).
   - `Reflection.jet`, `BodyRequests.jet`, `Derives.jet:2259-2279`,
     `OwnedTraitContracts.jet:550`, `Calls/Build.jet:644`, `:673`,
     `CheckRules.jet:1478`, `Constructors.jet:2569`.
   - `CoreRecordRows.jet` (all `TypeId` fields).
   - `CompilerOwned.jet:182`, `PreludeRows.jet:184`, `TypeRelations.jet:1349`,
     `BuildAdapter.jet:868`.
   - `Comptime.jet` `type_name: "TypeId"` and `TYPE_ID_TRAIT`.
2. Change `Core/Prelude/Prelude.jet:190-196` so `Type` is no longer a `path`
   struct. It becomes a compiler-owned identity with comptime-only reflection
   members. `TypeKind.Single(TypeId)` (`:377`) becomes `Single(Type)`.
3. `JetEval/Source/IR/Host.jet:340-363`: the comptime record stays
   comptime-only. Add the runtime one-word lowering in JetIR, and in the web
   backend with its interning pass.
4. Migrate Core (16), Examples (12) and Tests (23), for example
   `Core/Data/Data.jet:2852`, `:2858` (`TypeId{T}`). Update 100 docs lines,
   and update `spec.md:4350-4369`, which already names only `Type`.
5. Sema error for a comptime-only `Type` member reached at runtime: a
   registered code with a UI snapshot.
6. The zero-RTTI size proof from 3.2 attack 1, on O1, O2 and web. A full
   suite, as criterion 4 requires.

---

## Prior art checked

- **Rust:** `Fn`/`FnMut`/`FnOnce`, where `&mut F` is exclusivity for writing
  closures with no runtime lock. This is the model for R1–R2.
  `std::any::TypeId` is a 128-bit hash with no reflection attached, and
  `type_name` is separate and opt-in. This is the model for 3.2's split
  between identity and display.
- **Swift:** `throws` subtyping, where a non-throwing function is a subtype of
  a throwing one and the representation is shared, so conversion is free.
  This is the model for 2.4 option (i). Also `inout` with `&x` marks at call
  sites, which are mandatory for arguments; Swift does not mark receivers of
  `mutating` methods. This is the same trade-off as the L1 receiver choice.
- **Kotlin:** nullability subtyping through covariant positions only.
- **C# `ref`/`out`:** mandatory call-site marks, receivers unmarked. Swift
  and C# both ship the "receivers unmarked" hybrid deliberately. L1's
  receiver marks go beyond both, so the owner should see that precedent on
  the ballot.

## Verdict

- **Part 1:** proceed only after a ballot.
  - The contradiction is real. It has four parts (1.1), including a policy
    switch that I8 forbids and E0228 text that misstates the shipped rule
    (I4).
  - The lock cannot simply be deleted. It is sound to remove only together
    with R1, the `^`-access requirement on writing function values. That is
    a migration with Fn/FnMut-like reach whose size has not been measured.
  - The "#7053 auto-move" link in the orthogonality audit is a wrong
    citation. The real interaction is with #7053's structural Copy (1.2
    attack 2).
- **Part 2:** sound and mostly mechanical. Its scope is 106 declarations,
  450 reference lines and 1 duplicated twin. The E0638 fix is **not** a
  sema-only change: it needs either an ABI decision (2.4 option (i)) or it
  stays a coercion.
- **Part 3:** sound if runtime `Type` becomes a one-word identity and
  reflection members are comptime-only and never lowered. The zero-RTTI claim
  needs a measured binary proof, not a review.

## Gaps to turn into criteria or ballots

Ballots (owner gates):

- B1. Choose between L1 (marks everywhere, receivers included) and L0 (no
  marks). The hybrid is not an option.
- B2. R1: a writing closure needs `^` function-value access, so `xs.map`
  with a writing lambda is rejected unless `map` takes `^f`.
- B3. Under L1, an unmarked argument into a `^^` parameter is an error
  (except Copy values), deleting the auto-clone at `spec.md:2337`.
- B4. Accept `^^` on a structurally Copy value as a no-op.
- B5. Make a declared `From` that duplicates a subtyping edge E2405, instead
  of letting it win silently.
- B6. 2.4 option (i), the shared failing/non-failing ABI, versus keeping
  E0638.

Criteria on #7062:

- C1-1. Delete `#Policy(writes: .Explicit)` and all 18 using files, and make
  E0228's text match the enforced rule.
- C1-2. Delete the lock: `IR_CLOSURE_LOCK`, `ir_lambda_locked`,
  `IRTxFrame.closure_lock`, `spec.md:1974-1977`. Add a static-rejection
  golden on O0, O1 and O2.
- C1-3. Reconcile `spec.md:1887-1890` with `Spellings.jet:76`.
- C1-4. Census of Core higher-order functions and callback fields affected
  by R1, recorded before the build starts.
- C2-1. Hygienic `self` rename and a per-site E2406 check in the `jet fix`
  rewrite.
- C2-2. Collapse the `Errors.jet` / `PreludeDerives.jet` twin.
- C2-3. Generate the `#Error` auto-lift as a `From` impl.
- C3-1. Zero-RTTI binary proof on O1, O2 and web.
- C3-2. Interning for `Type` identity across separate compilation, and on
  web.
- C3-3. Sema error, with a UI snapshot, for a comptime-only member reached at
  runtime.
- C3-4. Behavior-change golden for path-collision equality.
- Out of scope, stated explicitly when closing: the `#As` and distinct
  forwarding paths (O13), and the fact-root consolidation (O35).
