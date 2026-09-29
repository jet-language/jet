# Memory model foundations: references, identity, races, and reactivity

Status: dated findings and proposals, 2026-09-27. Nothing here is ratified or
changes behavior. Work state lives on Tower card #3645, which holds the ballots
named below and the soundness defects. This note preserves the evidence and the
reasons. It is not a work queue.

Evidence came from `target/debug/jet` (built 2026-09-26 03:05) at HEAD
`75f6b3412`. That binary rejects Core imports (#3591), so every probe below avoids
them. Re-run each probe on a fresh binary before acting. The source excerpts were
read from the tree at the same HEAD.

## Result

Jet can store references. It deliberately removed only the *unchecked* stored
reference: `&T` fields and `-> &T` returns (D-MEM1/S3). The checked
replacement is `View<T>`/`ViewMut<T>` (D-MEM-VIEWRET1=B), and the unchecked one
is `*T` inside `#Unsafe`.

The weak point is object identity: many owners sharing one mutable value. A
recursive `Shared` type is rejected outright, GC scope copies instead of sharing,
and a stored `Shared` handle is consumed. Across threads, generic code bypasses
the sendability check. Only AOT stops it, and only as an ICE, because the JIT tier
has no second checker.

## Today's reference model, mapped to C++

| C++ | Jet today | Checked by |
|---|---|---|
| `const T&` parameter | `T` (unmarked, no copy, callee cannot mutate) | sema |
| `T&` parameter | `&T`, call site `&x`, exclusive | sema (E0204) |
| `std::move` / `T&&` | `^T`, call site `^x` | sema (E0121, E0209) |
| explicit copy | `~x` | sema (E0211) |
| `T&` member, returned reference | `View<T>` / `ViewMut<T>` fields and returns, provenance inferred or declared with `from` | sema (E2305, E2307, E0212) |
| address-stable intrusive link | `Pin<T>` fields | sema (E0219) |
| `shared_ptr` / `weak_ptr` | `Shared<T>` (`Arc<RwLock>`), `Shared.Weak<T>` | sema plus runtime locks |
| handle into a container | `Pool<T>` + `Id<T>` (generation-checked; stale ID panics) | runtime |
| `T*` | `*T` values and struct fields, `*x`, `p.*`, all in `#Unsafe` | audit gate plus dev sentries |
| `std::atomic` | `Atomic<T>` (D-ATOMIC-WIDTH1=A) | type |
| GC | `#Policy(gc)` scope (D-OPTGC1=A) | sema promotion plus a private collector |

Sources: `Docs/spec/spec.md` §Ownership and borrowing, §Expert memory tier;
`Docs/spec/syntax-decisions.md` D-MEM1, D-MEM-VIEWRET1, D-MEMPROVENANCE3,
D-OPTGC1; `Examples/features/memory/{returned_views,pin,rawptr,shared_weak_cycle}.jet`.

## Findings

### Confirmed correct

These probes were rejected with the expected code:

- A mutable task capture (`counter += 1` in a `task`) → E1101.
- `push_both(&xs, xs)` → E0204.
- A direct `Cell` into a task (`task ^c { … }`) → E1102.
- One `Cell` used by two `task.group` children → E1102.
- Two group children writing `xs[i]` / `xs[j]` → E1101 (dynamic indexes
  conservatively overlap).
- Safe code calling `mem.from_addr` and `mem.volatile_write` without `#Unsafe` →
  E1004 and E3101.

AOT emits real Rust borrows for views (`Option<&str>` in `jet emit --rust` for
`Examples/features/memory/string_view.jet`), so rustc re-checks them.

### F1 — Generic code bypasses the sendability check (I1, I2, I9)

```jet
fn spawn_with<T>(value: ^T) -> Int {
    t :: task ^value { 1 }
    t.join() ?? 0
}
fn run() {
    c :: Cell.new(5)
    print(spawn_with(^c))
}
```

- `jet check`: passes with only L0102.
- `jet run`: prints `1`.
- `jet build`: exits 101 with an ICE. rustc reports that
  ``Rc<JetCellInner<…>>` cannot be shared between threads safely``.

The same program without the generic is rejected with E1102. The cause is in
`crates/jet-sema/src/Sema/CheckerOwnership.rs`, `sendability_problem_inner`:
`Type::Named(name) if is_type_var_name(name) || core_type_known(name) => None`.
Type variables and every known Core type are treated as sendable, except for a
hand-maintained name list (allocator handles, `Browser*`, `Cell*`,
`SharedGuard`, views, pins).

A second hazard follows: a new Core type backed by `Rc` or `RefCell` is sendable
by default.

### F2 — The JIT tier has no second checker

On AOT, a sema miss becomes an rustc rejection and exit 101, which fails closed.
On `jet run` and the interpreter, sema is the only check. The JIT host is a large
hand-written Rust runtime with `unsafe` blocks.

`crates/jet-jit/src/Concurrency.rs` (`jet_jit_sender_send`) documents a known
hazard: "A consuming task capture can retain a stack slot. If the parent returns
before the child first reads it, the slot may yield a stale out-of-range word".
The code recovers by guessing when there is exactly one live sender.

Readiness criterion #217/2 ("zero reachable UB from safe Jet") is open.

### F3 — Recursive `Shared` types are rejected (E0221)

```jet
struct Node {
    val: Int
    next: Shared<Node>?
}
```

This is E0221: "Field `next` on `Node` can form a strong `Shared` cycle". The
rejection is at the type level, so every singly linked list, tree, DOM, or scene
graph built from `Shared` nodes is refused. `#Policy(gc)` does not lift it. Rust
accepts the equivalent `Option<Rc<RefCell<Node>>>`.

D-SHARED-CYCLE1=C allowed either "construction that would cycle needs an
explicit expert form" or "cycles free when external roots drop". The
implementation rejects whole types instead of cycle construction.

### F4 — GC scope copies, contradicting D-OPTGC1=A

```jet
#Policy(gc)

struct Node {
    val: Int
    next: Node?
}

fn run() {
    b := Node{val: 1, next: None}
    a := Node{val: 0, next: Val(b)}
    b.val = 5
    if a.next == .Val(n) -> print("through a: {n.val}")
    print("b: {b.val}")
}
```

Output: `through a: 1`, `b: 5`. The store made a copy. The ratified D-OPTGC1=A
example says `a.links.push(b)` "creates a traced edge to b". The implementation
instead makes "value snapshots"
(`Examples/features/memory/gc_cyclic.jet` comment). The code and the ratified
decision conflict.

### F5 — Storing a `Shared` handle consumes it

Under the same `Node` shape with `peer: ?Shared<Node>`, the line
`a.peer = Val(b)` followed by `b.peer = Val(a)` reports E0121 for `b`.
`spec.md` calls `Shared<T>` "a lock-guarded, copyable handle" whose clone "clones
the cheap handle, not the payload".

### F6 — `~` on `Cell` aliases instead of copying

```jet
fn run() {
    c :: Cell.new(0)
    c2 :: ~c
    &c2.set(41)
    print(c.get())
}
```

This prints `41` on `jet run` and on the AOT binary. The tiers agree, but
`spec.md` defines `~x` as creating "an independent owned value". Jet has no
stated rule for how `~` treats handle types (`Shared`, `Cell`, `Signal`, `Sender`).

### F7 — No cheap single-thread identity

`Shared<T>` is always `Arc<RwLock<T>>` (syntax-decisions S6), so every field
access in single-threaded code takes a lock. Reactive boxes already have
`#Local`/`#Shared` representation pins (D-DATARACE1=C); `Shared` does not. The
only unlocked identity type is `Cell<T>`, which has the F6 ambiguity.

### F8 — The raw tier is thinner than C or Zig

`*T` struct fields work:

```jet
use core.mem
struct Link {
    val: Int
    next: *Int
}
fn run() {
    target :: 42
    #Unsafe("target outlives link in this frame") {
        link :: Link{val: 1, next: *target}
        print(link.next.*)
    }
}
```

This prints `42`. Missing pieces (per `Core/mem/mem.jet` and the `core.mem`
reference):

- a null or optional pointer (`mem.Ptr<T>.null()` is E0003);
- pointer offset and cast, except by round-tripping through an `Int` address,
  which drops the provenance that D-MEM-SENTRY1 and D-HARDENED1 check;
- raw aligned allocate/free outside the allocator families;
- turning pointer plus length into a view;
- raw copy and fill.

### F9 — Cascading diagnostic

After E0003 in the first F8 draft, a later `#Unsafe` block inside `fn run`
reported E0355: "`#Unsafe` cannot attach at the File site". The site is wrong.

### F10 — Silent deep copy on store (ratified; noted for cost)

In F4, `Val(b)` deep-copied a named aggregate without `~`. This follows the
ratified copy law ("a cloneable read value entering an owning destination is
materialized automatically"). `#Policy(copies: .Explicit)` is the expert control.
Nothing reports large implicit copies.

## Proposal: four verbs, two ways to hold data

The owner prefers Jet's read, write, move, and copy vocabulary over a separate
"borrow, object, raw" taxonomy. The proposal therefore keeps the verbs and adds
one idea: whether a slot holds its data by value or by reference.

### The verbs stay

| Verb | Parameter type | Call site | Meaning |
|---|---|---|---|
| read | `T` | `f(x)` | Look at it. No copy and no change. |
| write | `&T` | `f(&x)` | Change it; nobody else touches it meanwhile. |
| move | `^T` | `f(^x)` | Hand it over; `x` is unusable after. |
| copy | — | `~x` | Make an independent duplicate. |

Parameters already pass by reference without copying, so a function never needs
a reference marker just to avoid a copy.

### The new idea: by value or by reference

A field, list element, local name, return, or keeping parameter holds either:

- **a value:** its own data. Storing it elsewhere makes a copy. This is the
  default and today's behavior.
- **a link:** a live reference to data stored elsewhere. Reading through it
  shows the latest change from any path.

Write access through a link follows the same verbs. A read parameter cannot write
through any link it reaches. A `:=` name, a writable field path, or `&` lending
can.

The owner rejected `ref T` / `ref &T`. After comparing `@`, `shared`, `link`, and
`*`, the owner chose `@` on 2026-09-28 (D-MEMREF1=A). `@` reads as "at": a link
at a place, which matches `@`'s existing infix reading of "at a source". Raw
pointers keep `*T`, `*x`, and `p.*` behind `#Unsafe`.

```jet
struct Team {
    captain: ?@Player         // a link, not a copy
}

p := Player{hp: 10}
t := Team{captain: Val(@p)}   // a link at p
p.hp = 5                      // t.captain.hp is 5
snapshot :: ~p                // an independent copy
kid :: @Node{val: 1}          // a new object held by link
```

This takes prefix `@` away from compile time. Before the choice, prefix `@`
marked compile-time values and blocks, `@if`/`@loop`, metadata roots, fact
reads, build facts, const-generic binders, fences, name splices, and `@fn`.
Card #3662 carries one ballot per surface plus a family ballot that weighs
plain words (`prep`, `compiler.`), two underscores (`__`), and `#`. The link
cutover lands together with that migration.
```

### The compiler picks the cost; the programmer never writes lifetimes

- If the owner provably outlives every link, the link is a plain pointer that
  costs nothing (today's `View`).
- Otherwise the target lives in counted storage from its creation, as Go moves
  escaping variables to the heap. Types that can form cycles also get the
  existing private collector. `jet gc report` lists each case with a fix. This is
  the collector that works to make itself unnecessary.
- Experts pin the free case with `from` clauses, and they forbid collection with
  a `!Mem.Gc` denial and use `Weak<T>` back links.
- Ballot: D-MEMREF-LIFE1.

### One safety sentence

"While something is being changed, nobody else may be looking at it" now holds
per access. The compiler proves most cases. The rest, such as looping over a
list while another link grows it, get a runtime check that stops with a located
error, as Swift does. Ballot: D-MEMREF-EXCL1.

### Tasks

A link type that reaches another task switches automatically to thread-safe
storage. Each write is one locked step, and `#Transact` groups writes. The switch
is reported, and `#Local` or `#Shared` pins it. Ballot: D-MEMREF-TASK1.

### What the model replaces

`View`, `ViewMut`, `Shared`, `Cell`, `Pin` fields, and GC-scope identity all
become links. This resolves F3–F7. `Pool`/`Id` stays as a data structure.
Unchecked raw pointers stay behind `#Unsafe`, completed per ballot D-RAWPTR2.

### Reactivity follows

Writes through links happen at sema-known sites, so codegen can notify readers
exactly there. Plain data then updates screens and live queries, and `#Transact`
is one glitch-free batch shared with live database queries. Ballot:
D-REACT-REF1.

### Fixes that need no new syntax

- **F1:** check the sendability of concrete types at each generic instantiation,
  and make thread confinement a property declared on the type (opt-out, like an
  auto trait) instead of a name list.
- **F2 / #217:** add a differential oracle. For generated programs, if sema accepts
  and rustc rejects, record a sema bug. AOT becomes a free checker for the JIT
  tier.
- **F4–F6 and F9:** each conflicts with ratified text or the spec. Card #3645
  homes them.

## Ballots on card #3645

| Ballot | Decides |
|---|---|
| D-MEMREF1 | Spelling for data held by reference (ratified: `@`) |
| D-MEMREF-LIFE1 | What happens when a link outlives its owner |
| D-MEMREF-EXCL1 | Stopping a change during a read when links alias |
| D-MEMREF-TASK1 | Links that cross into another task |
| D-RAWPTR2 | Complete raw pointer tools behind `#Unsafe` |
| D-AUTOPIN1 | Every automatic choice can be seen and locked |
| D-DENY-COST1 | Proving hot code has no hidden costs |
| D-REACT-REF1 | Plain data that updates the screen |
| D-INVALIDATE1 | Compiler-derived cache invalidation instead of keys |
| D-OPTIMISTIC1 | Automatic optimistic updates |
| D-RESUME1 | Resumable pages and per-route bundles |
| D-WIRESKEW1 | Old clients keep working after a deploy |
| D-HOTSTATE1 | Keeping live state when a type changes in `jet dev` |
| D-SYSSCHED1 | Parallel game systems from their data access |
| D-STACKBOUND1 | Worst-case stack and heap per entry point |
| D-SECRETEDGE1 | Secrets can never reach the browser |

## Ballots on card #3662 (compile time leaves prefix `@`)

| Ballot | Decides |
|---|---|
| D-COMPILER-NS1 | The family: plain words, two underscores, or `#` |
| D-PREP-BRANCH1 | `@if` and `@loop`, including declaration templates |
| D-NAME-SPLICE1 | Computed declaration names and marker `@sites` |
| D-META-ROOT3 | Metadata roots, fact members, and `T.reflect()` as one path |
| D-BUILD-FACT2 | Where `@build.*` facts live |
| D-CONSTGEN2 | The compile-time number parameter `<@N: Int>` |
| D-FENCE2 | The `@[ … ]@` fence |
| D-PREP-FN1 | The `@fn` build-time function mark |
| D-AT-INFIX1 | Non-code `@`: package references, hosts, command home files |

## Using the language level to replace stack layers

### The principle

Most layers in a modern stack rebuild, at runtime, a fact that was lost at a
boundary. React re-renders and diffs to discover which DOM depends on which
state. TanStack Query needs hand-written keys to learn which cached reads a
write made stale. TypeScript erases types, so zod validates them again at
runtime. ORMs and migration tools rediscover schema drift. Bundlers guess which
code runs where. A tracing GC discovers ownership that the program never stated.

Jet owns both sides of each of those boundaries. The rule that follows is: **a
fact the compiler proves once replaces a layer that rediscovers it at runtime.**

Each replacement must reuse the existing fact graph: effect rows, the `App`
graph, `PolicyFacts`, taint, and view provenance. Adding a parallel mechanism
would break I8.

### The pattern for magic with expert control: choose, explain, pin

Every automatic decision follows three rules:

1. **Choose.** The compiler picks a sound default: representation, placement,
   partition, collection, or invalidation.
2. **Explain.** The choice is visible through `jet inspect`, a report, or an LSP
   hover, with the fact that drove it.
3. **Pin.** An expert marker turns the choice into a checked contract. When the
   program no longer satisfies the pin, compilation fails. The compiler never
   silently falls back.

Precedents already in Jet: the `#Local`/`#Shared` reactive pins,
`#Target(JS|Wasm)` with `--explain-partition`, `jet bind --freeze`,
`#Policy(copies: .Explicit)`, and `jet gc report`. Making this pattern the
general law gives beginners magic and experts control over the same mechanism.
Experts read the decision and pin it; they do not fight it.

The second half is proven absence of cost. C and C++ give control by leaving an
abstraction out. Jet can prove that a cost is absent: `!Mem.Alloc`,
`!Mem.Rc`, and `!Panic` exist. The same denial form extends to collection, locks,
notification code, and dynamic dispatch, on a function, module, or package. An
expert can make "this frame loop never allocates, locks, or collects" a compile
error when violated. C, C++, Rust, and Zig cannot prove that.

### Layer by layer

| Layer today | What it rediscovers | Jet's compile-time fact | State in Jet |
|---|---|---|---|
| React VDOM, re-render, memo | Which UI depends on which state | Sema's read and write sets | `JetDom` updates without a VDOM (D-DOMGEN1). Observable objects are proposed above. |
| TanStack Query keys, SWR | Which reads a write made stale | Effect footprints | `app.live` uses `DB.Read` footprints with no keys (D-LIVEQUERY1). Server functions still carry "dependent-data revalidation keys" (D-DX-SERVERFN1). Two invalidation mechanisms is an I8 tension: derive server-function revalidation from write footprints as well. |
| TypeScript, zod, OpenAPI, tRPC | The wire contract | `#Codable` and server-function facts | Checked endpoints, wire types, and effects (D-DX-SERVERFN1). Extend `#PublishedSchema` migrations (D-MIGRATE1) to wire types so client/server version skew is a compile fact. |
| Redux, Zustand, MobX | Shared mutable state and its change notifications | `Shared<T>` plus `#Transact` | Proposed above as observable objects. |
| Optimistic-update code | The client-side effect of a server write | A server function's write set over declared state | New: when a mutation is pure over client-visible state, run the same checked function on the client for the optimistic value and reconcile on commit. Convex and Replicache ask the user to write this by hand. |
| Next.js SSR, hydration, code splitting | What runs where, and what state a handler needs | The `App` graph, JS/Wasm partition, and handler read sets | The typed `App` graph (D-WEBAPP1) and effect-based partition (D-WASM1) exist. New: per-route bundles from graph reachability, and resumable pages that serialize only the state each handler reads. |
| ORM, migration tools | Schema drift and query shape | `#PublishedSchema` diff, `DataPlan` | Both exist in sema (`Schema.rs`, `SchemaMigration.rs`, `DataPlan.rs`). |
| Auth middleware, row security | Who may read what | Authority, effects, taint, `RowPolicy` | Taint and IFC exist (`Taint.rs`, `PolicyFacts.rs`). New: prove a secret-tagged value can never reach the browser partition or a response body. |
| ECS scheduler (Bevy-style) | Which systems conflict | Parameter access modes (`T`, `&T`) | A frame scheduler derived from access facts exists for `GameScene.on_frame` (`ResourceSchedule.rs`). New: extend it to user systems so parallel scheduling needs no runtime conflict check. |
| SoA and layout libraries | Hot-field layout | Type and access facts | `layout_columnar.jet` exists. Fits choose, explain, pin. |
| Tracing GC | Ownership | Sema ownership proof | D-OPTGC1 already states that `jet gc report` lists each allocation ownership could not prove and offers a fix, and that removing the opt-in is the supported end state. The proposal above narrows collection to cyclic-capable object types. Add `jet fix` autofixes and a CI budget for the promotion count. |
| Hot reload | Whether edited code is still type-compatible | `HotSwap.rs` type-surface check | New: when the type surface changes, migrate live state with the same `migration` declarations used for published schemas, so state survives a field change during `jet dev`. |
| RTOS and HAL crates | Stack, heap, and interrupt safety | Effect denials, interrupt crossings | `!Mem.*`, `!Panic`, and `InterruptCallback` sendability exist. New: compile-time worst-case stack and heap bounds per entry point. |

### Constraints

- **Compile speed and separate compilation.** Whole-program facts need summaries
  at package boundaries. The memory facts already require a sealed target set or
  a signed dependency summary for open-world dispatch; reuse that everywhere.
- **One meaning.** Magic must never make the same source mean different things
  depending on a distant setting. F4 and D-OPTGC1 show the hazard. An automatic
  choice may change cost, never meaning, and it must hold on every tier (I9).
- **Performance proof.** Each automatic choice needs a paired cell against the
  plain form under the strict performance gate.
- **Soundness first.** Every layer above trusts sema facts. F1, F2, and #217's
  soundness campaign come before any new fact consumer.

### Dependencies between the proposals

1. Soundness (F1, F2 oracle, #217) underpins everything.
2. The by-reference model (D-MEMREF1 and its three follow-ups, resolving F3–F7)
   underpins observable data, parallel game systems, and narrowed GC.
3. Observable links plus `#Transact` underpin unified invalidation, optimistic
   updates, and resumable pages.
4. The raw-tier completion (F8, D-RAWPTR2) is independent and can run in
   parallel.

D-AUTOPIN1 and D-DENY-COST1 carry the choose, explain, pin law and the
proven-absence denials. The remaining stack ballots are listed in the ballot
table above.

## Related decisions and cards

D-MEM1, D-MEM-VIEWRET1, D-MEMPROVENANCE2/3, D-SHAPE-PLACE1, D-SHAPE-COPY1,
D-MEM-COPYSEM1, D-OPTGC1, D-DEP-GC1, D-SHARED-CYCLE1, D-SHARED-API1,
D-CONC-SHARE1, D-LOCALCELL1, D-DATARACE1, D-CONC-FREEZE1, D-TASKBORROW1,
D-MEM-SENTRY1, D-HARDENED1, D-PIN1–3, D-ATOMIC-WIDTH1, D-LIVEQUERY1,
D-EFFDBREAD1, D-WEBAPP1, D-DX-SERVERFN1, D-DX-ROUTER1, D-WASM1, D-DOMGEN1,
D-MIGRATE1, D-HOTSWAP1. Cards: #217 (readiness and soundness campaign), #3318,
#3393, #3560, #3591.
