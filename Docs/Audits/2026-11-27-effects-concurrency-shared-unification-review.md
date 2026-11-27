# Pre-build adversarial review — #7064: effect spellings, extensible FFI root, `Cell` = confined `Shared`, `#Deadline`, `xs.par()`, `core.events`

Date: 2026-11-27. Scope: criterion 1 of #7064. This is a read-only review. Every
claim below cites a file, symbol, or line that was read in this session.
`task.race` / `task.any` keep their names by owner decision. This review does
not reopen them.

Sources read: `node Tools/Tower/tower.mjs card show 7064` (and 7057, 7059,
7060); `Docs/Audits/2026-11-27-cognitive-budget-orthogonality-audit.md` O14,
O21–O29, Tier 2 rows 7–9; `Docs/Audits/2026-11-27-zero-magic-memory-trio-review.md`
§4; `Compiler/Prelude/Effects.jet`; `Compiler/JetSema/Source/Sema/Effects/`
(Checks, Library, PureRoot); `Compiler/JetSema/Source/Sema/Ownership/Crossings.jet`;
`Compiler/JetFoundation/Source/Policy/Authority.jet`; `Compiler/JetIR/Source/BuildLazy.jet`,
`BuildTypes.jet`, `BuildParallel.jet`; `Compiler/JetRuntime/Source/Shared.jet`;
`Core/Builtins/Builtins.jet`; `Core/Tasks/Tasks.jet`; `Core/Event/Event.jet`;
`Docs/Spec/spec.md` §§ Shared state (2673–2807), One rule on every rung
(3061–3093), package effects / values / deny-only (3205–3293), Parallel
collection adapters (5109–5134), Deadlines (4853–4888), Select (5252–5270).

Note: `Core/Concurrency/` does not exist. Concurrency Core lives in
`Core/Tasks/`, `Core/Sync/`, `Core/Actor/` and `Core/Event/`.

---

## 0. Verdict up front

The card bundles five changes. Two can be built as written. Three have defects
that make the card text unbuildable or that lose power, so they need a ballot
or a reworded criterion before build:

| Part | Verdict | Blocking finding |
|---|---|---|
| 3 effect spellings | **Build** after one small ballot | Bare `#FX` has no stated replacement on a *block*. `#FX()` must be legal (F1.2). |
| `Ambient = {Mem.Alloc, Panic}` | **Defective as written** | The live ambient (deny-only) set is `Mem.*`, `Panic`, `Time.Wait` and `Unsafe` (`Authority.jet:40-43`). The card's set would force `Mem.Rc`/`Mem.Stack`/`Time.Wait`/`Unsafe` into every written ceiling. The name `Ambient` also collides with `AUTHORITY_AMBIENT_BASIC_EFFECTS = {IO, Mem.Alloc, Exec.Args}` (`Authority.jet:24`). **Ballot.** |
| `State.Read` / `Context.Read` | **Ballot** | `State.Read` alone leaves writes on the E3401 special case: two mechanisms for one meaning (I8). `Context.Read` must exempt the runtime's own deadline/cancel reads. |
| Unify scopes and values | **Ballot** (spelling + coverage law) | Scopes use prefix coverage and values use exact coverage. One form needs a per-field-type `covers` law. |
| FFI root + `c.FFI`/`python.FFI` | **Defective as written** | `c.FFI` declared in package `c` with `parent: FFI` resolves `FFI` to itself (cycle E0758). The swap reverses 216 files of `FFI.X` spellings. `Asm` and `C` have no package. **Ballot** on the spelling. |
| `Cell` = confined `Shared` | **Build, re-scoped** | `Cell` is *already* a native Shared cell that takes the full Shared lock plus a 16-byte heap token on every `get`/`set` (`BuildLazy.jet:2521-2537`, `Shared.jet:825-843`). The audit premise "Cell avoids lock cost by construction" (O14) is false. The zero-lock work is the same work as #7059 c3. Build it once, not twice. **Ballot** on whether the name `Cell` survives. |
| `#Deadline` replaces `*_within` | **Loses power as written** | An expired `#Deadline` wait that is not typed emits **E3003** and exits (`spec.md:4863-4864`). `guard_read_within` returns `None`. Replacing it turns a recoverable branch into process death. `task.timeout` today is `fn(delay) -[Time.Wait]> Never!` (a sleep-then-fail, `Tasks.jet:298`), not `timeout(d, f)`. **Ballot.** |
| `xs.par()` | **Build** with stated laws | `fold` needs a `merge` that sequential `Iter.fold` lacks. `par()` must refuse non-splittable sources. The lowest-index failure rule and the fixed 64-item chunks must carry over. |
| Events to `core.events` | **Name defect** | The module is `core.event` (singular) today (`Core/Event/Event.jet:2`, `Core/package.jet:9`). `Subscription` is shared with Reactive. Moving names out of `TYPE_NAMES` leaves about 10 compiler files that are keyed by name. |

---

## 1. Effects

### F1.1 — "7 → 3 spellings" is coherent; the count is honest only with `via`/`[]` as forms of `-[…]>`

Live list (`spec.md:3074-3082`): `-[…]>`, `-[]>`, `-[via f]>`, `#FX(...)` above a
declaration, `#FX: (...)` line in a body, `#FX(...) { }`, bare `#FX`, plus
package `authority:` and run flags `--allow/--deny/-A`.

After the change: (1) the arrow row `-[…]>`, with `-[]>` and `-[via f]>` as
list contents rather than separate spellings; (2) `#FX(...)`, which is one
marker on the block, type, module and file sites; (3) package `authority:`.
Run flags are command surface. The card does not count them, but the audit
must say they stay (they are the run rung).

The deletion is cheap. A repo sweep for an in-body `#FX: (` finds **4 lines in
3 files**:
`Docs/Proposals/DebugEvalNative20261118/Probes/fx_forms.jet:17`,
`.../fx_line.jet:6,11`, and
`Tests/Cases/Effects/RepeatedFxCapIsRefusedOnEveryTier/Input/inbody.jet:3`.
The last one is a test of the refusal that this card deletes. Its snapshot and
diagnostic must go too (see the cutover list).

### F1.2 — Bare `#FX` on a block has no replacement unless `#FX()` is legal

The card deletes bare `#FX` and says "use `-[]>`". That covers only the
function rung. A **pure block** (`#FX { … }`) and a **pure type/module** have
no arrow. The replacement must be `#FX()` (empty list). Before build, verify
that the marker grammar accepts an empty argument list and that E-codes do not
treat it as an arity error. If it does not, the card *adds* a spelling instead
of removing one. Proof owed: a `#FX() { io.print(…) }` refusal snapshot on
O0/O1/O2.

### F1.3 — `Ambient = {Mem.Alloc, Panic}` contradicts the live ambient set (blocking)

Live law: `authority_is_deny_only` (`Compiler/JetFoundation/Source/Policy/Authority.jet:40-43`)
is true for root `Panic`, root **`Mem`** (Alloc, Rc, Stack, Gc), **`Time.Wait`**
and **`Unsafe`**. `sema_effect_without_deny_only` (`Sema/Effects/Checks.jet:1206`)
strips exactly that set before E3401 and ceiling checks
(`PureRoot.jet:75-103`). So the *de facto* ambient set has 4 roots/leaves, not 2.

The card's `{Mem.Alloc, Panic}` would make every `-[FS]>` function whose body
copies an RC value (`Mem.Rc`), waits on its own task (`Time.Wait`) or reaches
non-Core `#Unsafe` (`Unsafe`) fail its own written ceiling. The compiler
itself is an example: `Compiler/JetCLI/Source/CLI/Debug.jet:536` writes
`-[Env, FFI.Asm, FS, IO, Time.Wait]>`, which shows that `Time.Wait` is
sometimes written but is never required.

Second defect: **`Ambient` collides with an existing internal meaning.**
`AUTHORITY_AMBIENT_BASIC_EFFECTS = {"IO", "Mem.Alloc", Exec.Args}`
(`Authority.jet:24`) is a *grantable* default bundle and contains `IO`. Two
"ambient" sets with different members break the one-name-one-meaning rule.

Required ballot (owner): pick one of
- **A (recommended):** `Ambient` = exactly the deny-only set
  (`Mem`, `Panic`, `Time.Wait`, `Unsafe`). This is a pure renaming of today's
  law, so it needs zero corpus churn. Rename the internal
  `AUTHORITY_AMBIENT_BASIC_EFFECTS` to something non-colliding (for example
  `…_DEFAULT_GRANT`).
- **B:** Narrow to `{Mem.Alloc, Panic}` as the card says. Before ratification
  this needs a sweep count of every written ceiling that would newly fail, and
  an answer for how `Mem.Rc` (inserted by the compiler, invisible to the
  author) can be written by a user at all.

Whichever wins, `Ambient` is a new public Prelude name. It is a #7057 G39
bundle, so it must be declared *as a bundle* with #7057's mechanism, not as a
compiler-known string.

### F1.4 — `State.Read` alone splits one meaning across two mechanisms (I8)

Today module-state reads **and writes** are "not an effect", but `-[]>` refuses
both through E3401 (`spec.md:3274-3284`). If only `State.Read` becomes an
effect, writes stay on the E3401 special case. "Touches module state" would
then be half effect and half special rule. That is an I8 violation the card
would create.

Required: either `State: enum { Read, Write }` with E3401's module-state branch
deleted (it becomes an ordinary ceiling violation, E0740/E0749), or a ballot
that explicitly keeps writes on E3401 with a stated reason. Also state:
- `access`: `State.Read` → `.Reads` and `State.Write` → `.Replaces`.
  Irreversibility: a module write inside `#Transact` is already snapshotted, so
  `false`.
- Locals, `^` lent parameters and cells built in the body stay free
  (`spec.md:3281-3283`). Only *module* places count.
- Debugger `--pure` probes keep their writes-only exemption (`spec.md:3283-3284`),
  and #7060 c4's dev probe must also carry zero `State.*`.
- The GlobalState task fixtures (`Compiler/JetSema/Tests/Fixtures/GlobalStateTask*.fixture`)
  must be re-expressed as rows.

### F1.5 — `Context.Read` must not tax every wait

`context.deadline` is read by **every** wait point to deliver deadlines
(`spec.md:4861-4871`), and `context.thread` is read by the runtime. If those
internal reads count, every `Time.Wait`/`Net`/`IO` function also gains
`Context.Read`, and the row becomes noise. Law needed: only a *source-level*
`context.<field>` read records `Context.Read`. Runtime delivery is part of the
waiting effect.

Second issue: `Context.Read` in a `#run` reads the build's root context
(`spec.md:4886-4888`). Is it ambient at comptime or does it need a grant? Pick
one meaning, which I9 requires. Third: E0643 (a module value reads context)
already exists. Decide whether it is subsumed (a module value's row is `-[]>`,
so `Context.Read` is refused by the ordinary ceiling) or kept. Keeping both is
two mechanisms. `#Frame` → `#Context` (O28) is **not** in #7064's text. Leave
it out and do not silently widen the scope.

### F1.6 — "Unify scopes & values" needs a coverage law, not just a spelling

Values: `-[Bucket{name: "photos"}]>`, which covers itself only. A bare
`Bucket` covers every value, and denying any value denies the bare effect
(`spec.md:3239-3262`). Scopes: `FS.Read("templates/")`, where a bare
`FS.Read` covers every scope and a scope covers its *prefix descendants*
(`spec.md:3377-3438`, `--allow='FS.Read("data/")'` at 3499). These are two
coverage laws, not just two spellings.

One mechanism: an effect (struct or enum case) may carry fields. The spelling
is chosen by ballot: record `E{f: v}` (matches type literals) or call
`E(v)` (matches today's scopes and the authority strings in manifests and on
`--allow`). Coverage comes from the field type through one trait method on
`impl Effect` (`covers: fn(self, other: Self) -> Bool`). `Path` uses prefix
containment (`Path.is_within`, `Core/Files/Path.jet:34`), a host uses
equality, and `String`/`Int`/`Bool` use equality. This deletes the compiler's
`effect_scope_argument_valid` special path (`Checks.jet:1180-1182`) in favor
of the trait.

Asymmetry to fix or ballot: "denying a value denies the bare effect"
(`spec.md:3261`) is stricter than scoped denial ("a need that only overlaps
it…", `spec.md:3437-3438`). One rule must win.

**#7057 coupling (binding):** G39 c1 builds bundles "including scoped effects
like `FS.Read("path/")`". If #7057 lands its bundles first, it hard-codes the
old scope form. Order: #7064's parameter-form ballot → #7057 c1 build. Or #7057
c1 must be built on the unified form directly.

### F1.7 — FFI root + `c.FFI` / `python.FFI`: the spelling as carded does not resolve

Live: `FFI` is a 24-case enum (`Compiler/Prelude/Effects.jet:86-116`). Its
cases feed the `#FFI(<lang>)` menu that `Tools/Agent/CoreSourceRows.jet`
writes into `Core/Compiler/Lang.jet`, and Jetpack's `BinderDescriptor.jet` is
checked against them (`Effects.jet:82-85`). E0746 special-cases the
`"FFI"` root (`Checks.jet:1347`).

Attacks:
1. **Self-reference.** The package-effect law (`spec.md:3212-3237`) lets a
   package declare a kind with `parent:`. A kind named `FFI` inside package `c`
   writing `parent: FFI` resolves `FFI` to its own declaration (nearest scope),
   which is a cycle and E0758. Either the parent needs a qualified Prelude path
   (a new resolution rule) or the leaf needs a different name. As carded,
   `c.FFI` cannot be written with today's resolution.
2. **The parent must be a struct.** E0758 refuses an enum family as parent
   (`spec.md:3225-3227`, `Library.jet:307`). So `FFI` must become
   `FFI: struct { impl Effect { irreversible: true, access: .Replaces } }`.
   The card implies this but does not say it. Say it.
3. **Who declares `c.FFI` / `asm.FFI`?** C is the compiler-native ABI edge
   (`#Import(c)`, `#Export(c)`). There is no `c` *package* to declare a kind.
   `FFI.Asm` appears in the compiler's own rows (for example
   `Compiler/JetCLI/Source/CLI/Debug.jet:78`, `Front.jet:3024`,
   `Host.jet:926`). Inline asm is not a package at all. Tier 3 #23 ("FFI beyond
   C/Rust to packages") is a *separate, unballoted* item. #7064 cannot quietly
   decide which languages stay built-in.
4. **Blast radius.** `FFI\.(Go|…|Swift)\b` matches **216 files** outside
   `Docs/Site`. That includes manifests (`authority: { allow: [FFI.C, …] }`,
   `Compiler/JetBackend/Tests/NativeInlineC.fixture/NativeInlineC.jet:34`),
   `jet audit foreign` output text (`AuditForeign.jet:362-365`, which prints
   `FFI effects reached: FFI.C`), lock-recorded dependency rows
   (`spec.md:3086`), `--allow/--deny` strings and LSP hover. Reversing to
   `pkg.FFI` changes every one, which needs a `LANGUAGE_REWRITES` row
   (`Compiler/JetFoundation/Data/Syntax/Spellings.jet:114-126`) and a
   migration.
5. **Names.** `Py` vs `python`, `JS` vs `js`, `DotNet`. The carded example
   already renames a leaf (`Py` → `python`) without a ballot.
6. **Granularity loss.** Today `--deny FFI.Py` is one row. After the change,
   denying "all Python" depends on every Python binder package declaring
   `parent: FFI` *and* sharing one kind. Two Python binders (`python.FFI`,
   `cpython.FFI`) are two kinds, and denying one does not deny the other. The
   package-effect law permits exactly this ("two packages may each declare an
   `Audit` kind of `Log` without clashing", `spec.md:3235-3237`). Language-level
   denial needs a ballot answer: a shared intermediate kind per language, or
   accept per-package denial.

Recommended ballot options:
- **A:** `FFI` root struct. C, Rust and Asm stay built-in leaves as the
  parameterized form of F1.6 (`FFI("c")`/`FFI{lang: "c"}`), which O23
  originally proposed. Other languages are package kinds (`python.Python` with
  `parent: FFI`). Leaf-first spelling is preserved, so `FFI("c")` needs only a
  mechanical rewrite.
- **B:** The card's `c.FFI` form, plus a ratified qualified-parent rule and a
  `c`/`asm` pseudo-package. This has a larger blast radius and adds a
  resolution rule.

#7060 compatibility: c3 requires explicit ceilings (`#FX` or per-callsite) on
dynamic trait values. That is unaffected, because `#FX(...)` on types/traits
survives. c4's dev probe "carries zero effect inside `-[]>`/`#FX(...)`" must
be re-read under F1.3/F1.4: zero non-ambient effects *and* zero `State.*` /
`Context.Read`. c2 (auto-`via`) is unaffected, because `-[via f]>` remains a
list form.

Existing `authority` rows: the fixture at `NativeInlineC.jet:34` grants
`Mem.Alloc` in `allow:`, while `Authority.jet:36-39` says deny-only rights are
never written into `allow:`. Decide whether that is accepted-and-ignored or an
error. Under a named `Ambient` it must be one or the other, the same on every
tier.

---

## 2. `Cell<T>` = thread-confined `Shared<T>`

### F2.1 — The premise is wrong. `Cell` is already a Shared cell, and it is slower than a field read.

- `Compiler/JetIR/Source/BuildLazy.jet:2521-2524`: "The local `Cell<T>` … a
  native Shared cell (ir_shared_new), aliased by every copy of its handle."
- `ir_cell_access` (`BuildLazy.jet:2529-2533`) calls `jet_rt_native_cell_guard`
  on **every** `get`/`set`/`replace`. That function
  (`Compiler/JetRuntime/Source/Shared.jet:836-843`) calls
  `jet_rt_shared_guard_acquire` (mutex plus permit words, `Shared.jet:780-808`)
  and then `jet_rt_alloc(16)` for a token (`Shared.jet:829-834`). Drop frees it
  and releases the permit (`Shared.jet:866-872`).

So a `cell.get()` costs the Shared lock **plus a heap alloc/free**. That is
strictly more than a Shared field read. O14's trade-off ("`Cell` today avoids
lock cost by construction") is false, and the unification carries **no
regression risk** against today's numbers. The *zero-lock* requirement is
net-new work, and it is the same work as #7059 c3 ("single-thread lock-elided
fast path"). The #7059 review (§4.2 items 1–4) already specifies it: one mode
bit, a plain exclusivity word (`jet_rt_link_read_enter_local`), and elided
replay/wait-cycle bookkeeping.

**Binding sequencing:** #7064's Cell part must build *on* #7059 c3's ST path.
It must not ship a second fast path. A second elision mechanism would violate
I8.

### F2.2 — How confinement is a type fact after the merge (ballot)

Today confinement is name-keyed: `sema_ownership_crossing_local_cell` matches
`"Cell" | "CellReadGuard" | "CellEditGuard"` (`Crossings.jet:520-525`, again
at `:353`). The guards are `#[MoveOnly, Local]` carriers
(`Core/Builtins/Builtins.jet:484-488`). `Shared<T>` is by design *copyable and
sendable* (`spec.md:2675-2682`). Three options:

- **A (static, recommended):** Keep the name `Cell<T>` as the confined
  instantiation of `Shared`, written `#Local` the same way other types become
  confined (`Crossings.jet:512-513` already has `.DeclaredLocal`). It has the
  same field-access and guard API as `Shared`. The confined type has *no mode
  bit at all*, so the IR is plain loads/stores, and crossing a task is a
  compile error (E1101 family) rather than a silent promotion. This deletes
  the separate `get/set/replace/get_or_set` API and the two Cell guard types.
  It keeps one name for the opt-in.
- **B (dynamic):** Delete `Cell` entirely and rely on #7059's ST/MT promotion.
  The cost: a confined cell still pays the mode-bit test, which does not meet
  the card's "zero lock overhead verified by IR/asm" text if a branch counts.
  It also loses the compile-time "cannot cross a task" guarantee
  (`spec.md:2805-2807`), which is a safety regression.
- **C:** Spell it as `Shared<T, .Local>` or similar. This adds a type
  parameter users must learn, so it is worse than A on I8.

A needs one more ballot answer: does a `Cell` handle **alias** on copy, like
`Shared` (live behavior, `BuildLazy.jet:2522`), or copy its payload? Live code
aliases. The spec text "`Cell.new(value)`" with `get` copying the *payload*
does not say. Keep aliasing, because it matches `Shared`, which is the point
of the merge.

### F2.3 — Exactly one guard family

After the change: `SharedGuard<T>` only. Delete `CellReadGuard`,
`CellEditGuard`, `jet_rt_native_cell_guard*` (`Shared.jet:825-872`),
`ir_cell_guard_editable` (`BuildTypes.jet:124-134`), `ir_local_cell_inner`'s
special case (`BuildTypes.jet:115-122`), `THostCall.CellGuardProject`
(`SharedOwner.jet:404`), and `sema_call_try_cell_method`
(`SharedOwner.jet:408-450`). Read vs edit is the guard's access mode, which
`^guard: SharedGuard<T>` already expresses (`spec.md:2740-2742`). `map` and
`split` already exist on `SharedGuard` (`spec.md:2737-2739`), so nothing is
lost.

A confined `SharedGuard` keeps `#[MoveOnly, Local]`. It must still be
refused in user structs, lists, escaping lambdas and so on
(`spec.md:2802-2805`). Today that rule is Cell-only text. It must become the
guard rule for *both*, or Shared guards lose it on the confined path.

### F2.4 — Re-entrant borrow safety: two failure modes become one

- Cell today: a conflicting edit guard **panics** with "`Cell borrow conflict`"
  (`spec.md:2798-2799`).
- Shared today: recursive acquisition **blocks** and the wait-cycle detector
  stops the task (#7059 review §4.2.2).

One type cannot have both. Pick one report for every instance, confined or
not, and snapshot it on O0/O1/O2. Recommended: the dynamic exclusivity-word
check on the confined path raises *the same stop and the same text* as the MT
cycle stop, as #7059 §4.2.2 already requires. The "`Cell borrow conflict`"
text is retired. Proof owed: `a.x = f(a)` where `f` edits `a`, `guard_edit()`
held while a helper reads, and `split` disjointness, each on a confined and an
unconfined instance, with identical output on every tier.

### F2.5 — Proof required for "zero lock overhead"

- **IR check:** a confined `c.x += 1` lowers to load/add/store on the payload
  address. It has no `jet_rt_shared_guard_acquire`, no `jet_rt_alloc`, no
  `sync_mutex_lock`, no `replay_take` and no `jet_rt_native_cell_guard*`.
- **asm check:** the same function at O2 contains no `call` into the runtime.
  Use `Tools/Perf/AsmParity` against a hand-written `Rc<RefCell<T>>`-free Rust
  baseline (`Cell<i64>`) for cycles and bytes.
- **Peer cell:** a hot confined RMW loop vs Rust `Cell<T>` and
  `Rc<RefCell<T>>`, with `Jet/peer < 1.00` on wall and peak memory. Then a
  second, different program, such as a tree walk with a confined memo.
- The dynamic check needed by F2.4 (one plain exclusivity word) is the only
  allowed residue. The criterion text "zero lock overhead" is met by that,
  but the evidence must say "one plain word test", not "zero instructions".

### F2.6 — Other name-keyed `Cell` sites in the cutover

`Sema/CoreUsage.jet:218,229,238,317,323,372,740` (core.mem import surface);
`Sema/Scopes.jet:3491,3518` (`Cell`/`Atom`/`Mutex` by name — `Atom` and
`Mutex` look dead and should be verified and deleted); `Sema/SharedAccess.jet:68,82,94,103`;
`Sema/Registration/Items.jet:2485`; `Sema/KeyBy.jet:554`;
`JetIR/Source/OptLicm.jet:389`. `Atomic<T>` as a Shared specialization (O14) is
**not** in #7064's text, so leave it alone.

---

## 3. Concurrency

### F3.1 — `#Deadline` replacing `*_within` loses a recoverable result (blocking)

Live `*_within` APIs that are time limits (other `_within` names, such as
`Path.is_within`, `body_bytes_within` and `ssl_move_within`, are not, and must
not be touched):
- `Shared.guard_read_within(limit)` / `guard_edit_within(limit)` return
  `SharedGuard<T>?` (`spec.md:2743-2745`; runtime `jet_rt_native_shared_guard_within`,
  `Shared.jet:402`; test
  `Tests/Cases/SharedGuards/TryAndTimedGuardsRefuseAHeldPermitAndTakeAFreeOne.fixture`).
- `Semaphore.acquire_within(limit) -[Time.Wait]> Bool`
  (`Core/Tasks/Tasks.jet:183`; `Examples/Concurrency/OnceAndSemaphore.jet:33`).
- `term.take_within(n, wait) -[IO, Time.Wait]> [U8]? IOError!`
  (`Core/Term/Term.jet:392`).

The `#Deadline` law (`spec.md:4861-4871`): an expired wait whose operation
returns a typed error returns it. **Any other wait emits E3003 and exits with
the runtime error code.** Once a deadline passes, every later wait in the
block fails as well.

So `#Deadline(20ms) { cell.guard_read() }` is **not** equivalent to
`cell.guard_read_within(20ms) ?? fallback`. The first kills the process. The
second returns `None` and lets the caller try something else in the same
scope. Lock acquisition with a timeout and a fallback is a standard pattern
(try-lock-with-backoff, lease probing). Deleting it loses power. The O27
claim of "no trade-off in power" is wrong.

Options (ballot):
- **A (recommended):** Keep a bounded wait as **one labeled parameter**, not
  a name suffix: `guard_read(within: d) -> SharedGuard<T>?`,
  `acquire(within: d) -> Bool`, `take(n, within: d)`. This deletes every
  `*_within` *name* (the card's I8 goal) and keeps the recoverable result.
  `#Deadline` stays the scope-wide limit. The law: a per-call `within:` can
  only shorten the deadline in force, the same as nested `#Deadline`.
- **B:** Give `guard_read`/`acquire` a typed timeout error under `#Deadline`
  (for example `SharedGuard<T> TimedOut!`). This changes the signature of
  *every* caller, including the common no-deadline case, which is a large
  ergonomic regression.
- **C:** The card as written, with E3003 on lock timeout. This is a power
  loss, so the owner must accept it explicitly.

**`try_*` (non-blocking).** `try_guard_read()`/`try_guard_edit()`
(`spec.md:2742`) and `Semaphore.try_acquire` (`Tasks.jet:173`) are *not*
`#Deadline(0)`: the deadline form E3003s, while `try_*` returns `None`/`false`.
Under option A, `try_x()` ≡ `x(within: 0s)`, and `try_*` can be deleted as a
second spelling. That deletion needs a ballot, because it is a public API.
Under B or C, `try_*` must stay.

**Select `after`** (`spec.md:5256-5264`) stays. It is an arm that fires
inside one wait and never a limit on code, and the select `else` arm is the
non-blocking form. Neither interacts with `#Deadline` except through the law
that an outer deadline still fires during the wait (`spec.md:5268-5270`).
State that law on the select page. No change is needed.

**`task.timeout` is not what the card thinks it is.** Live:
`timeout: fn(delay: time.Duration) -[Time.Wait]> Never! { _: recv(core.tasks.after(delay, 0)) }`
(`Core/Tasks/Tasks.jet:298`), which sleeps then fails. The card's
"`task.timeout(d, f)` as sugar for `#Deadline(d) { f() }`" is a *new
signature* and a new meaning under an existing name. Under greenfield cutover
that is fine, but it is a public API change and needs a ballot line, every
caller swept, and the doc reference at `spec.md:4875` reworded. It also needs
an answer for how `task.timeout`'s result type surfaces a deadline that fires
on a non-typed wait inside `f` (E3003 process exit vs a typed
`TimedOut!`). Sugar must not differ from the block form, so whatever
`#Deadline` does, `task.timeout` does.

`#Shield` (O27 list) is not in #7064's scope. Do not touch it.

### F3.2 — `xs.par()` replacing `para_*`: the laws that must carry over

Live contract (`spec.md:5109-5134`; `BuildParallel.jet:1-7`;
`JetRuntime/Source/Parallel.jet`):
1. Order: map/filter keep source order, and partition keeps order within each
   side.
2. Chunking: stable boundaries; `para_fold` uses *exactly* 64-item chunks
   merged by a deterministic adjacent-pair tree. That is why
   `jet_list_para_fold_add_fixed_f64` (`JetRuntime/Source/IterNumeric.jet:169`)
   gives the same float sum regardless of worker count.
3. Failures: each chunk stops at its first failure, all started chunks join,
   and the lowest source index wins, independent of completion order.
4. Captures: E1101/E1102 at the boundary, with no implicit serialization.
   Sendable globals are readable with zero copies, and writes are E1101.
5. `para_map(f, limit: n)` bounds concurrency.

Attacks on `xs.par()` and fusion:
- **`fold` signature mismatch.** Sequential `Iter.fold(seed, step)` has no
  `merge`. A parallel fold needs `seed_factory`, `step` and `merge`. So
  `xs.par().fold(...)` is a *different* method under the same name. Either it
  keeps three arguments (and the claim "same Iter vocabulary" is overstated),
  or `par().fold` requires a `Monoid`-like trait on the accumulator.
  **Ballot**, because it is a public API.
- **Non-splittable sources.** `par()` on a channel, generator or `Stream`
  cannot split without buffering (Rayon separates `IndexedParallelIterator`
  from `par_bridge` for this reason). Law: `par()` exists only on splittable
  sources (List, View, ranges, Pool ids). Elsewhere it is a compile error,
  never a silent sequential or buffering fallback.
- **Fusion and order.** `xs.par().map(f).filter(g)` must run `f` then `g`
  per chunk and concatenate chunk outputs in chunk order. That is the same
  engine with a fused helper body, and order stays deterministic. Work
  stealing may change *which worker* runs a chunk, never chunk boundaries.
  Boundaries must remain a function of length alone, as today, or law 2 and
  float reproducibility break.
- **Laziness and effects.** Fused stages run interleaved per element, not as
  stage-by-stage passes. If `f` has effects, the observable order of `f`/`g`
  side effects changes compared with today's `para_map` followed by
  `para_filter` (two passes). The spec must say that only *results* are
  ordered, and effect order across elements is unspecified. That is already
  true within one `para_map`, so it now applies across stages too.
- **Error propagation across stages.** "Lowest source index" must mean the
  index of the *source element* whose pipeline failed, whichever stage failed.
  Say it and test it with a `map` failure at index 9 and a `filter` failure at
  index 3: index 3 must win.
- **Where `limit` goes.** `xs.par(limit: n)` on the adapter. Delete the
  special-cased labeled argument in `Sema/Calls/Concurrency.jet:38-43` and
  `ValueMethods.jet:42`.
- **Crossing boundary.** `SemaSendCrossing` for parallel workers must apply to
  *every* closure in a `par()` chain. The spec's "parallel-adapter boundary"
  (`spec.md:2806`) already names it.
- **Web/O0.** I9: `par()` must have the same result on web, where the walk may
  run on one worker. Laws 1–3 make that hold by construction, but it needs a
  web golden.

The lowering already has one chunk engine (`jet_rt_native_para_walk`). The
change is in front of it (adapter recognition plus fused helper bodies), so it
is not a new scheduler.

### F3.3 — Events to `core.events`: name and depth defects

- **Module name.** The module is **`core.event`** (`Core/Event/Event.jet:2`,
  `Core/package.jet:9`, `Core/App/App.jet:50`, `Core/Web/Web.jet:872`,
  `RuntimeHandles.jet:136-141` `<corelib>/core.event.*`). The card's
  `core.events` is either a typo, which should be corrected on the card text
  by ballot because workers may not reword criteria, or an undeclared rename.
  Recommended: keep `core.event`. A rename adds churn and no meaning.
- **The eight names** (`Compiler/JetFoundation/Source/Syntax/Spellings.jet:125-132`;
  `Types/Types.jet:998` `SYMBOL_BUILTIN_TYPES`): `Event`, `Hook`,
  `DecisionHook`, `AsyncEvent`, `DispatchReport`, `EventScope`,
  `Subscription`, `EventTrace`. A ninth, `FfiCallbackEvent`, is name-keyed in
  `Sema/Calls/EventHandles.jet:25` but is not in `TYPE_NAMES`. Decide whether
  it moves too.
- **`Subscription` is shared.** Reactive owns `Subscription` methods
  (`Tools/Agent/CoreSurfaceLedger.jet:1806` → `Sema/Calls/ReactiveHandles.jet`).
  Moving it to `core.event` makes `core.reactive` depend on `core.event`. The
  alternatives are two `Subscription` types or a shared home. Pick one.
- **Depth.** Removing the names from `TYPE_NAMES` only unreserves them.
  Compiler knowledge stays name-keyed in Sema `Calls/EventHandles.jet`,
  `Comptime/Fold.jet:143`, IR `BuildEvents.jet:222-304`, runtime `Events.jet`,
  `AsyncEvents.jet` and `EventHooks.jet`, `JetDebug/Source/Debug/EventObservation.jet:107`,
  `JetCLI/Source/CLI/LiveEvents.jet:32`, `JetDevServer/Source/Canvas/GraphProjection.jet:510-614`,
  `JetFoundation/Data/Syntax/MathLayout.jet:158-183` (`TYPE_*` constants),
  `JetCLI/Source/CLI/SyntaxDictionaryData.jet:146-149`, and
  `Tools/Editors/TreeSitter/grammar.js:22`. O26's intent ("built on channels
  and closures") is a library reimplementation. The card's text ("move out of
  `TYPE_NAMES` into core.events") is satisfied by the namespace move alone.
  The audit records the gap: after #7064 the event family is *namespaced but
  still compiler-lowered*. Re-keying by module identity (`core.event.Event`,
  like `<corelib>/core.event.*` handles already) is the minimum that keeps a
  user-declared `Event` type from being hijacked. The full library
  reimplementation belongs to Tier 1 #2 (role-trait law). It must not be
  claimed here.
- **Shadowing proof owed.** A user program that declares its own
  `Event: struct {…}` and `Hook: enum {…}` compiles and runs on O0/O1/O2
  without touching the compiler's event lowering. That proves the names are
  really free.

---

## 4. Binding cutover list (the card cannot close while any item remains)

**Effects**
1. Delete the in-body `#FX: (...)` parse path and bare `#FX`. Rewrite the 4
   lines in 3 files listed in F1.1. Delete or re-purpose
   `Tests/Cases/Effects/RepeatedFxCapIsRefusedOnEveryTier` and its diagnostic
   if no other form can trigger it. Add a `LANGUAGE_REWRITES` row and a
   `Compiler/Migrations/` rewrite.
2. Make `#FX()` legal and give it a snapshot (F1.2).
3. Declare `Ambient` as a #7057 bundle with the ballot-chosen members (F1.3).
   Rename `AUTHORITY_AMBIENT_BASIC_EFFECTS`. Make `authority_is_deny_only` and
   `sema_effect_without_deny_only` read the bundle instead of hard-coded root
   strings (`Authority.jet:40-43`).
4. `State` (Read, plus Write if ballot A) and `Context.Read` in
   `Compiler/Prelude/Effects.jet`. Delete E3401's module-state branch if it is
   subsumed. Rewrite `spec.md:3264-3284`, `:4881-4888` and the `Effects.jet`
   header comment (lines 16-19, "Reading the task context … is no effect").
   Update the E0643 decision. Add UI snapshots for each new refusal.
5. One parameterized effect form plus the `covers` law. Delete
   `effect_scope_argument_valid`'s special path. Rewrite `spec.md:3239-3262`
   and `:3377-3438`. Migrate manifest/`--allow` strings if the spelling changes.
6. `FFI` becomes a struct root. Delete the 24 cases. Regenerate
   `Core/Compiler/Lang.jet`'s `#FFI(<lang>)` menu from the new source
   (`Tools/Agent/CoreSourceRows.jet`). Re-check Jetpack `BinderDescriptor.jet`.
   E0746's `"FFI"` root test (`Checks.jet:1347`). Rewrite all 216 files with
   `FFI.<Lang>`, plus `jet audit foreign` output text and goldens
   (`AuditForeign.jet:365`), lock-file rows, LSP hover and
   `Docs/Spec/Reference/` pages.
7. Diagnostics registry (`Compiler/Prelude/Diagnostics.jet`) and
   `Docs/Spec/diagnostics.md`: reword every what/why/fix that names a deleted
   spelling, the 24 leaves, or "not an effect" (I4).

**Cell / Shared**
8. Delete `CellReadGuard`/`CellEditGuard` (`Core/Builtins/Builtins.jet:484-488`),
   the `jet_rt_native_cell_guard*` exports (`Shared.jet:825-872`) and their
   rows in `RuntimeCExports`/`RuntimeRoutes`, IR `ir_cell_*`
   (`BuildLazy.jet:2520-2717`) and `BuildTypes.jet:115-134`, Sema
   `sema_call_try_cell_method` and the Cell guard split
   (`SharedOwner.jet:374-450`), and the Cell arms in
   `Crossings.jet:353,520-525`, `SharedAccess.jet`, `CoreUsage.jet`,
   `Scopes.jet:3491,3518`, `Registration/Items.jet:2485`, `KeyBy.jet:554` and
   `OptLicm.jet:389`.
9. One re-entrancy report text (F2.4). Retire "`Cell borrow conflict`" and
   re-snapshot.
10. Rewrite `spec.md:2794-2807` and `:2688`. Move the guard-storage rule onto
    `SharedGuard`. Rewrite every example and test that uses
    `get/set/replace/get_or_set`.
11. IR and asm proof plus two peer cells (F2.5), built on #7059 c3's ST path.

**Concurrency**
12. Time-limit cutover per the F3.1 ballot. Rewrite
    `Shared.guard_*_within`, `Semaphore.acquire_within`, `term.take_within` and
    their tests/examples (`TryAndTimedGuards…fixture`, `OnceAndSemaphore.jet`,
    `Tasks.jet:707-720`). If ballot A deletes `try_*`, rewrite those too. New
    `task.timeout` signature, all callers, and `spec.md:4772,4875`.
13. `par()` adapter. Delete `METHOD_PARA_*` (`Data/Syntax/Spellings.jet:225-231`),
    the Sema `para_*` arms (`Expressions.jet:4454,4496,4562`;
    `Calls/Collections.jet:264`; `Calls/Concurrency.jet:38-306`;
    `Calls/Methods.jet:1970-2015`; `Calls/ValueMethods.jet:34-42`), and the IR
    routes (`RouteClosure.jet:224-227`). Keep the runtime engine and rename
    its comments. Rewrite the `GlobalStateTask*` fixtures, `spec.md:5109-5134`,
    `:2806`, and every example. Add a web golden.
14. Events: drop the 8 names from `TYPE_NAMES` (`Source/Syntax/Spellings.jet:125-132`)
    and `SYMBOL_BUILTIN_TYPES` (`Types/Types.jet:998`). Re-key every site in
    F3.3 by module identity. `MathLayout.jet` `TYPE_*` constants,
    `SyntaxDictionaryData.jet`, the tree-sitter grammar and `CoreSurfaceLedger.jet`.
    Add a user-shadowing golden.

**Sweeps** (collect-all, before landing any stricter rule): every `.jet` in
the repo for `#FX:`, bare `#FX`, `FFI.<Lang>`, `Cell.new`/`Cell<`, `_within(`
on the four APIs, `para_`, and the eight event names used unqualified. Run a
full suite after landing, because this is a cross-cutting sema, codegen and
runtime change.

---

## 5. Owner ballots required before build

1. **Ambient set and name** (F1.3): A = the deny-only set, B = `{Mem.Alloc, Panic}`
   with sweep counts. Also the rename of `AUTHORITY_AMBIENT_BASIC_EFFECTS`.
2. **State effect shape** (F1.4): `State.{Read, Write}` replacing E3401's
   module-state branch, vs `State.Read` only with E3401 kept for writes.
3. **`Context.Read` law** (F1.5): source reads only, plus its comptime meaning
   and the fate of E0643.
4. **Parameterized effect spelling and coverage** (F1.6): `E{f: v}` vs `E(v)`;
   the per-type `covers` trait; one denial-overlap rule.
5. **FFI spelling** (F1.7): A = `FFI("c")`, with built-in C/Rust/Asm and package
   kinds for the rest, vs B = the card's `c.FFI` with a qualified-parent rule
   and pseudo-packages. Also: which languages stay built-in (overlaps
   Tier 3 #23), and leaf names (`Py`/`python`).
6. **Cell surface** (F2.2): A = `Cell<T>` kept as the static `#Local`
   instantiation of `Shared` (recommended), B = delete `Cell` and use dynamic
   promotion, C = a `Shared` type parameter.
7. **Bounded waits** (F3.1): A = a `within:` parameter replacing `*_within` and
   `try_*`, B = a typed timeout error on plain waits, C = the card as written,
   with E3003 on lock timeout. Also the new `task.timeout(d, f)` signature.
8. **`par().fold` signature** (F3.2): three-argument `fold` vs a merge trait.
9. **Event module name and `Subscription` home** (F3.3): `core.event` vs
   `core.events`.

Within approved scope, the rest are implementation choices: fused helper
bodies, re-keying by module identity, and IR/asm proof harnesses.

## 6. Sequencing against neighbors

- **#7059 c3 first**: the ST Shared path is the confined-Cell lowering (F2.1).
- **Ballot 4 before #7057 c1**: bundles must be built on the unified parameter
  form (F1.6). `Ambient` (ballot 1) is itself a #7057 bundle.
- **#7060**: no structural conflict. c4's probe must also carry zero `State.*`
  and `Context.Read` once ballots 2–3 land (F1.7).
