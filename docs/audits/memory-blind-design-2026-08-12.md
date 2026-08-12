# Jet Memory — The Provenance Design (blind dossier)

Status: designed blind, from first principles, per brief. All syntax marked **proposed**. No knowledge of Jet's actual memory system was used or sought.

---

## 1. The One Idea, the Axes, the Law

### 1.1 The one idea, one sentence

> **All of memory management is one question — "can this reference outlive the memory it points into?" — and every strategy anyone has ever shipped (manual, arenas, ownership/borrowing, reference counting, tracing GC, generational handles) is just a different way of *witnessing* the answer; so Jet makes the witness a first-class, per-region choice, inferred for beginners, named by experts, and conserved by one law.**

The owner's hypothesis says manual control, GC, and Rust-style ownership are related and under-unified. This design's claim: they are the *same mechanism* — a region plus a witness — differing only in **when** the proof "no live reference remains" is produced (compile time, run time, or human audit) and **at what granularity** memory is reclaimed (value, region, or slot). The hypothesis is **proven for mechanism** and **conceded in one place for ergonomics** (§6, worst thing #2).

### 1.2 The four axes (orthogonal; every strategy is a point)

| Axis | Question it answers | Values (proposed) | Beginner default |
|---|---|---|---|
| **P — Placement** | *Where* does this value live? | function `local`, named region `r`, `static`, foreign | inferred (escape inference) |
| **R — Reclamation policy** | *How* does its region free memory? | `stack`, `arena`, `pool[T]`, `rc`, `gc`, `manual`, `fixed(buf)` | inferred (`stack`/`arena`) |
| **A — Access permission** | *What* may this reference do? | `&T` read, `&mut T` exclusive write, `shared T` cross-thread | inferred from use |
| **W — Witness tier** | *When/how* is "no live reference remains" proved? | `scope` (static), `gen`, `count`, `trace`, `trust` (audited) | `scope`, auto-promoted with audit |

Axes are independent: an `arena` can be freed by `scope` witness (classic arena) or held by `count` witness (shared arena); a `pool` naturally pairs with `gen`; `manual` policy requires `trust` obligations or static proof.

### 1.3 The strategy space — every prior art is a point, not a mode

```
                     WITNESS
             scope      gen        count       trace       trust
           ┌──────────┬──────────┬───────────┬───────────┬─────────┐
 P  stack  │ C locals │    —     │     —     │     —     │    —    │
 O  value  │ Rust     │    —     │ Swift ARC │     —     │ C++ new │
 L  arena  │ Cyclone  │    —     │ shared    │     —     │ classic │
 I         │ /MLKit   │          │ arenas    │           │ arenas  │
 C  pool   │    —     │ games,   │     —     │     —     │ games   │
 Y         │          │ Vale     │           │           │ (raw)   │
    heap   │ ASAP     │ Vale     │ Nim/Koka  │ Go, JVM   │ C       │
    fixed  │ MCU bufs │    —     │     —     │     —     │ MMIO    │
           └──────────┴──────────┴───────────┴───────────┴─────────┘
```

One model. Choosing a cell is choosing a *configuration*, not a dialect. No cell changes what any other cell means.

### 1.4 The law (conservation of provenance and exclusivity)

> **The Provenance Law.**
> 1. Every value has exactly **one owning region** from birth to reclamation; ownership may *move*, never fork.
> 2. Every reference carries its target's **region** and a **permission**; neither can be forged, widened, or erased — only narrowed or dropped.
> 3. A region is reclaimed **only after a witness** — scope, generation, count, trace, or audited trust — establishes that no live reference into it remains.
> 4. At any instant, each region has **at most one writer** (many readers XOR one writer), and write access moves between threads only by moving the region.

Every rule in the design is an instance of one clause:

| Language rule | Law clause |
|---|---|
| use-after-free impossible | 3 |
| double-free impossible | 1 |
| no dangling returns | 2 + 3 |
| iterator invalidation caught | 4 (one writer) |
| data races impossible | 4 |
| leaks bounded to region lifetime | 1 + 3 |
| `unsafe` is a named witness, not a hole | 3 (`trust` is a witness with a human signer) |

### 1.5 The magic-default contract (applies to every inference in this document)

Every magic default has all three, always:

| Magic | Audit trail (see what it did) | Explicit spelling (write it by hand) | Refusal switch (project-wide off) |
|---|---|---|---|
| placement inference | `jet audit memory` region ledger | `x := expr in r` | `policy memory { placement: explicit }` |
| copy→move elision | `jet audit copies` | `move x` / `copy x` | `policy memory { moves: explicit }` |
| witness promotion (scope→count/gen/trace) | promotion ledger, one line per promotion | `region c: rc { … }` | `policy memory { witness: scope }` |
| drop insertion (destruction at last use) | `jet audit drops` | `drop x` | `policy memory { drops: explicit }` |
| ambient region threading | shown in `jet explain fn` | explicit region parameter | `policy memory { ambient: none }` |

---

## 2. The Full Design, Element by Element

Each element carries a **two-sentence beginner card** (the learnability test from the brief), then climbs rungs.

### 2.1 Values: create, copy, move, die

**Beginner card:** *A variable holds its value; giving it to something else copies it, and Jet quietly skips the copy when you never touch the original again. Things clean up by themselves the moment you're done with them.*

- **Rung 0 (beginner):** Mutable value semantics (stolen from Hylo). `x := expr` mutable, `x :: expr` immutable. Assignment and argument passing are *logical copies*; the compiler converts every last-use copy into a move (ASAP/Mojo-style destruction at last use, not scope end). No hidden reference aliasing between two named variables, ever — the beginner's mental model is spreadsheet cells.
- **Rung 1:** `move x` and `copy x` written explicitly where the reader should see them; `jet audit copies` lists every copy the compiler could not elide, with size and location (the "why is this slow" tool).
- **Rung 2 (expert):** `struct Buf uncopyable { … }` — non-copyable (linear) types (stolen from Austral/Swift `~Copyable`). Moves become mandatory and checked: a linear value must be consumed exactly once; forgetting it is a compile error naming the drop point to add. This is how file handles, locks, unique device tokens, and FFI gifts are typed.
- **Death:** deterministic, at last provable use. Destructors (`fn drop(self: own Self)`) run in deterministic documented order (reverse creation within a region; region close runs all drops then frees in bulk — or skips drops for trivially-droppable contents, making arena close O(1)).

### 2.2 References and permissions (axis A)

**Beginner card:** *`&x` lets a function look at your value without taking it; `&mut x` lets exactly one place change it at a time. You never write anything else — the compiler tracks how long the look is allowed to last.*

- `&T` — shared read. `&mut T` — exclusive write. `shared T` — cross-thread share of a frozen or `sync` region (§2.6). Three permissions, total (Pony's six reference capabilities collapsed to three by moving isolation to the *region*, where it belongs).
- **Exclusivity is flow-checked, not scope-checked:** within a function, Polonius-grade dataflow decides where a borrow ends — the *last use*, not the closing brace. All intra-function borrowing needs zero annotations, full stop.
- Reads are plentiful, writes are singular: while any `&mut` into a region is live, no other reference may touch what it can reach (law clause 4). The classic get-or-insert cache pattern is proven directly by flow analysis (§3, example C) — the NLL "problem case #3" family is a solved requirement, not a known hole.

### 2.3 Regions (axis P) — the unit of everything

**Beginner card:** *Everything you make lives in a room, and when the room's time is over, everything in it is cleaned up together. You never say which room — Jet picks the smallest one that works — but you can name a room when you want a say.*

- **Implicit regions:** every function body is a region (`local`) — stack slots plus an inline function arena for things too big or too dynamic for the stack. Every call passes an implicit **destination region** for its result (region-polymorphic return, monomorphized or a single hidden pointer argument — this is Tofte–Talpin region polymorphism with the lesson learned: *per-call* result regions, never one shared result region per function, which is exactly the mistake that made MLKit's first results "terrible").
- **Escape inference:** a value that outlives its function is *inferred* to be built in the caller's destination region — statically, as part of the calling convention. This is Go's escape-analysis ergonomics with a guarantee instead of a heuristic: escape inference here changes *placement*, never silently changes *witness* (promotion is a separate, audited step, §2.5).
- **Named regions (proposed syntax):**

```jet
region ast: arena {                    // scope-tied region, arena policy
  tree :: parse(&toks) in ast          // explicit placement
}                                      // one bulk free, drops elided if trivial

region world: pool[Entity](cap: 4096)  // pool region, gen witness, stable handles
region cache: rc                       // count-witnessed region
region live: gc                        // trace-witnessed region (traced ALONE — pause ∝ this region, not the heap)
```

- **Regions are values at the expert rung:** a region handle is `uncopyable`; owning it in a struct nests the region (Verona-style tree); moving it transfers the whole subgraph across threads in O(1) (§2.6). At any instant the live regions form a forest — that shape is what makes clause 3 checkable locally.
- **First-class rule of reach:** references may point *up* the region tree (into longer-lived regions) freely; pointing *down or sideways* requires the container to sit in the shorter-lived region (inferred), a `gen` handle, or a dynamic witness. This single rule is the whole aliasing model.

### 2.4 Reclamation policies (axis R) and allocators

**Beginner card:** *A room can be cleaned different ways — all at once, slot by slot, or by counting who still needs it — and each room picks its own way. Your code doesn't change when the cleaning method does.*

- Policy is a property of the **region**, never of the type (rejecting Nim's global mode switch: a program-wide `--mm:` flag gives one program two meanings; Jet policies are local and composable, so one meaning on every tier).
- **Allocators are region backings** (stolen from Zig, made structural): `arena(backing: os)`, `arena(backing: fixed(scratch))`, `pool[T](cap: 1024, backing: static)`, `arena(backing: counting(os))` for instrumentation. Wrapping an allocator is wrapping a backing; swapping program-wide is rebinding the root region's backing in `main`; swapping scope-wide is `with region`:

```jet
with region tmp: arena(backing: fixed(buf64k)) {
  …   // every unplaced allocation in this dynamic extent lands in tmp
}
```

- **Ambient region:** every function has an implicit current-region parameter (Odin's `context` allocator, but typed, visible in `jet explain`, and part of the checked ABI — not a hidden global). Libraries allocate "wherever the caller said" by default; that is why allocator swapping needs no library cooperation.
- `manual` policy exists (expert): `free` is an operation whose safety obligation is discharged either statically (linear handle consumed exactly once) or by a `trust` block. C-style control, with the bug classes fenced.

### 2.5 Witness tiers (axis W) — and the promotion rule, stated honestly

**Beginner card:** *Jet proves at compile time that nothing points at freed memory; when your program's shape makes that impossible to prove, Jet adds the smallest runtime check that keeps it true and writes down that it did. You can always see the list, and a project can forbid it.*

| Tier | Mechanism | Cost | When inferred |
|---|---|---|---|
| `scope` | static proof (region containment + flow borrows) | zero | default, always tried first |
| `gen` | generation-tagged slot handles (`Handle[T]`), stale ⇒ deterministic `none`/error | 1 compare per deref-through-handle | pools; cyclic structures with kill/respawn |
| `count` | Perceus-precise reference counting, static elision, in-place reuse at count==1 | inc/dec at true share points only | shared escape with no cycles proven |
| `trace` | per-region tracing collector; traces only that region | pause ∝ region size; barriers only on that region's refs | shared escape with possible cycles |
| `trust` | human-signed obligation, audited | zero | never inferred — always written |

**The promotion rule.** When a beginner's program stores references in a shape no static proof covers (a global cache, an observer list, a mutable cross-linked graph), the compiler promotes the *smallest enclosing region* to the *cheapest sufficient* dynamic witness, records one ledger line (what, where, why, the explicit spelling, the refusal switch), and moves on. This is a deliberate reading of the brief's own priority order: beginner experience (#2) outranks zero-cost (#3), and the zero-cost principle is preserved in its honest form — **the runtime cost buys expressiveness the program actually demanded, never compiler convenience**, and it is never paid by code that didn't demand it. Experts and agents set `policy memory { witness: scope }` and get a compile error naming the region and the choice instead. The residual risk this creates is worst-thing #1 (§6).

MLKit's ghost, exorcised three ways: inference is **function-local** (no whole-program fixpoint to destabilize), promotions are **printed** (never silent), and policy can **refuse** (never trapped).

### 2.6 Concurrency — the same story, not a second one

**Beginner card:** *To give data to another thread you hand over its whole room; to share data, you freeze the room so nobody can change it. Jet won't compile a program where two threads can write the same thing at once.*

- **Transfer = move a region.** Region handles are `uncopyable`; `send(worker, give job)` moves region `job` — zero copy, no locks, no marshalling. Messages between tasks are regions.
- **Share = freeze a region.** `frozen :: freeze assets` makes the region deeply immutable; `shared` references to it flow anywhere. Frozen regions reclaim by a count **at region granularity** — O(regions) counting, not O(objects) (this is where Swift's per-object ARC traffic goes to die).
- **Shared mutable = `sync` policy (expert):** `region q: sync(mutex)` or `sync(atomic)` — interior mutability exists *only* as a region policy with a named synchronizer; there is no free-floating `UnsafeCell`. Lock acquisition is the temporary transfer of the region's single writer token — clause 4 again.
- **Theorem (the point of one story):** clauses 2+4 ⇒ data-race freedom for all safe code, on every tier — native, JIT, interpreter, wasm — because it is proved in the front end, not provided by a runtime.
- Hard-realtime note: `give`/`freeze` are O(1) and allocation-free; nothing in the concurrency model can introduce a pause.

### 2.7 The escape hatch — `trust`, raw memory, layout, MMIO

**Beginner card:** *Some code must tell the compiler "trust me" — it's marked, it must say exactly what it promises, and the project decides who is allowed to say it. Everything outside those marks stays fully checked.*

```jet
trust(obligation: "dma buffer is device-owned until IRQ; no Jet reference escapes") {
  p := raw &ring[head]                 // raw pointer *T exists only in trust blocks
  mmio.DMA_ADDR.write(p.addr)
}

struct Frame @layout(c) @align(64) { … }          // layout control: c, packed, align, bit-fields
reg UART_DR: volatile u32 @at(0x4000_C000)         // MMIO: volatile typed register at fixed address; never reordered, never elided
```

- Every `trust` block **must** carry a named obligation string; blocks without one do not parse. `jet audit trust` lists every block: file, obligation, author, last-touched commit — the org-level review surface.
- **Policy control (proposed):** `policy trust { allow: [hal.*, ffi.*]; deny: * }` in the project manifest — org-controllable, CI-checkable. A dependency cannot smuggle `trust` past a deny.
- What `trust` may do: raw pointers, address arithmetic, calling foreign code, `manual` frees, transmutes with layout proof. What it may **not** do, even inside: forge a region tag or a permission on a *safe* reference handed back out — the containment boundary is that safe types re-enter only via checked constructors (`Handle.adopt`, `slice.from_raw(p, len) in r`), so a `trust` bug is confined to what the obligation names.
- **CHERI:** on capability hardware, `*T` compiles to hardware capabilities — bounds and provenance enforced even inside `trust` (deploy-time hardening, free of source changes). **Fil-C-style containment** is available as a *foreign-region policy* for vendored legacy C (§2.8), not as Jet's own model — paying 1.5–4× inside a fence is a fine price for someone else's code, and no price at all for yours.

### 2.8 FFI — foreign memory is a foreign region

**Beginner card:** *Memory that comes from C lives in a marked foreign room, and the function signature says who must clean it up. Jet won't let a foreign pointer hide inside normal Jet data.*

Three transfer verbs at the boundary (proposed) — the who-frees-what table *is* the signature:

| Verb | Meaning | Who frees | Example |
|---|---|---|---|
| `loan` | borrow for this call only | caller (foreign or Jet) | `fn c_write(buf: loan *c u8, n: usize)` |
| `gift` | ownership crosses the boundary | receiver | `fn c_parse(doc: gift *c Doc)` — C must free; Jet side wraps `gift` returns as owned values |
| `bind` | foreign owns; Jet holds a contained handle | foreign, at its pace | `db :: bind c_open(path) @free(c_close)` — drop calls `c_close`, exactly once, linearly checked |

- Foreign pointers are `*c T` values confined to `extern(c)` regions (witness: `trust`, tier declared at the `extern` block). They cannot be stored in safe Jet structs; you `copy out`, or `bind` with a deleter, or keep the work inside a `trust` fence. Containment is clause 2: foreign provenance can't launder into safe provenance.
- C++/Rust mapped onto the same verbs: C++ move = `gift`; Rust `Box<T>` = `gift @free(rust_drop)`; `&T` across either boundary = `loan` with the call as its extent. One vocabulary, three languages.
- **Wasm/JS foreign GC heap:** `extern(js)` regions use the *engine's* witness — `externref`s are handles into a foreign traced region. They cannot dangle (engine guarantees it), they are opaque to Jet layout, and they never enter Jet regions except as handles. Jet's own heap on wasm is ordinary linear memory with ordinary Jet regions — one meaning on every tier.

### 2.9 The extremes — each is a configuration, not a dialect

| Domain | Configuration (all proposed, all checkable) | What the check proves |
|---|---|---|
| Bare-metal MCU (≤2 KB, no allocator) | `policy memory { heap: none; witness: scope; budget: 2048 }` — regions over `fixed`/`static` buffers only | whole-program peak memory ≤ 2048 bytes, at compile time; any heap op is a compile error naming the op |
| Kernel/driver | `heap: none` per module + `manual`/`fixed` regions + `trust` allowlist per subsystem | no hidden allocation; every `trust` block enumerated per subsystem |
| AAA frame loop | init-time pools + per-frame `arena(backing: fixed)`; `policy memory { steady_state: no os-alloc }` | after `@steady`, zero syscalls/allocs — violation is a compile error at the allocation site |
| Hard realtime | `witness: scope, gen` only (`count`, `trace` refused) | no pause mechanism exists in the binary; WCET of `gen` check is one compare |
| Long-lived server | region-per-request arenas (fragmentation dies by construction); shared state in one `rc` or `gc` region | request memory is O(1) ops to reclaim; traced region pause ∝ shared state only |
| Data pipeline | large `arena` batches, `freeze` for parallel readers, `pool` for records | zero-copy fan-out; bulk free between batches |
| One-liner script | nothing written; implicit `main` arena; promotions allowed and ledgered | safe, fast, zero ceremony — the whole point of inference |
| Wasm | Jet regions in linear memory + `extern(js)` foreign region | no dangling externrefs; Jet semantics unchanged |
| FFI-heavy embed | `extern` regions + three verbs + `bind` deleters | who-frees-what is in the signatures; leaks/double-frees localized to declared boundary |
| Agent-authored code | `policy memory { witness: scope }` pinned; ledger in CI | every verdict at compile time; no silent promotion drift under mass edits |

### 2.10 Diagnostics as product

**Anatomy of every memory error — what / why / fix, one fix, named:**

```
error[E-MEM-041]: `cfg` would outlive the text it references
  --> main.jet:9:3
  what: `LAST` lives in region 'static; `cfg.name` points into `text`, which dies at the end of `main`
  why:  a reference must never outlive its region (Provenance Law, clause 3)
  fix:  LAST = some(cfg.to_owned())        // copy out of `text`
  alt:  move `LAST` into a region that dies with `text` — `jet explain E-MEM-041` shows both, first is canonical
```

Design rules for the error surface (the agent-optimality contract):
- **One canonical fix first**, machine-applicable (`jet fix E-MEM-041 main.jet:9`), alternatives explicitly ranked below it — repair determinism is a *format requirement*, not a hope.
- Errors name **regions and witnesses**, never analysis internals; no error ever says "lifetime" or shows an inference variable.
- **Inspection surfaces:** `jet mem facts` (static: region forest, policies, promotion ledger, drop points, per-region peak bounds where provable), `jet mem profile` (runtime: per-region live/peak/frees, promotion hit counts), `jet audit memory|copies|drops|trust` (the four ledgers).
- **Policy statements are code**, checked like types: `policy memory { heap: none }` in a module makes *any* transitively-reached allocation a compile error at the offending call site with the call chain printed.

---

## 3. Centerpiece: References That Outlive an Expression

### 3.1 The reach rule, stated exactly

> A plain `&T`/`&mut T` may be **stored in structs, returned from functions, and held across calls — with zero annotations — whenever the compiler can place the container in a region that does not outlive the referent's region.** Placement inference makes this the overwhelmingly common case, because every struct with reference fields is *implicitly region-polymorphic* and every function result has a *destination region* chosen per call site.

Mechanics that make it reach far:
1. **Implicit region polymorphism.** `struct Parser { cur: &Buf }` *is* `struct Parser[in r] { cur: &r Buf }`; `r` is inferred at every use and never written. Lifetime variables exist; lifetime *syntax* does not, until rung 2.
2. **Result-region default = meet.** A function returning a reference derived from its parameters returns provenance equal to the **meet** (shortest-lived) of the contributing inputs. This is sound with *no annotation ever required* — strictly beyond Rust's elision, which errors on two-input cases. Annotation exists only to *loosen* (tie the result to one input), never to permit.
3. **Flow-based ends.** Borrows end at last use (Polonius-grade), so "holding across calls" costs nothing unless a genuine conflict exists — and then the error names the two uses.

**Where the model asks for more — the exact boundary, and the ask:**

| Situation | Why static proof ends | What Jet asks for (the whole ask) |
|---|---|---|
| Public API where ≥2 input regions must be *distinguished* in the result | meet-default is sound but tighter than intended | name a region in the signature: `fn pick(a: &r str, b: &str) => &r str` — one token, no variance, no bounds |
| Dynamic shape: cross-linked graphs with individual kill/respawn | no scope contains each element's lifetime | a `pool` region + `Handle[T]` (gen witness) — the canonical spelling; inferred for beginner code |
| Escape past every scope: globals, cross-request caches | nothing on the region stack survives long enough | a dynamic-witness region (`rc`/`gc`) — inferred + ledgered for beginners, written by experts, refusable by policy |

**Why this boundary is right:** it is exactly the line between *lifetimes that are facts about scopes* (compiler's job, free) and *lifetimes that are facts about runtime data* (undecidable statically without whole-program shape analysis — ASAP's own limit — so someone must pay at runtime, and the payer should be the region that needs it, not the whole heap). Rust puts the annotation tax on everyone to keep everything static; GC puts the runtime tax on everyone to keep everything dynamic; Jet's law makes the split per-region and the tax local. Designs that ban stored references (second-class references) fail the rubric's server, game, and FFI domains outright — the ban is rejected, not defended.

### 3.2 Worked example A — a struct holding a reference

```jet
struct Config { name: &str, verbose: bool }          // implicitly Config[in r]

fn load(src: &str) => Config {                       // inferred: fn load[r](src: &r str) => Config[r]
  Config{ name: src.slice(0, src.find(' ')), verbose: true }
}

fn main() {
  text :: read_file("app.conf")                      // owned by main's local region
  cfg  :: load(&text)                                // Config[main.local] — zero annotations
  print(cfg.name)                                    // ok: cfg provably dies with text
}
```

`jet explain load` shows the elaborated signature — the audit trail for the inference. Misuse (storing `cfg` into `static LAST`) produces E-MEM-041 exactly as shown in §2.10, with the owned-copy fix first.

### 3.3 Worked example B — returning a reference

```jet
fn best(xs: &List[Score]) => &Score {                // inferred: result lives in xs's region
  m := &xs[0]
  loop x, xs { if x.value > m.value { m = &x } }
  m
}

fn pick(a: &str, b: &str) => &str {                  // TWO inputs: result = meet(a, b) — still zero annotations
  if a.len > b.len { a } else { b }
}
```

`pick`'s result is valid while **both** inputs live — sound, inferred, and usually exactly what was meant. When the caller genuinely needs the result tied only to `a`:

```
error[E-MEM-052]: result of `pick` is tied to `b`, which dies here
  what: you kept the result past `b`'s region, but `pick` says the result may point into either input
  fix:  if the result only ever points into `a`, say so: fn pick(a: &r str, b: &str) => &r str
```

One error, one repair, and the repair is *loosening a signature you own* — never appeasing a checker with ritual.

### 3.4 Worked example C — a cache (the promotion story, end to end)

Intra-function get-or-insert — the classic borrow-checker fight — is just flow analysis here, plus one canonical entry op (one mechanical path):

```jet
fn render(m: &mut Memo, id: u64) => &Image {
  m.table.entry(id).or_put(|| rasterize(id))         // returns &Image tied to m's region; proven directly
}
```

The *shared, program-lifetime* cache — where static proof genuinely ends:

```jet
static MEMO: Memo := Memo.new()                      // beginner writes this, types nothing else

fn handle(req: &Request) => Response {
  img :: render(&mut MEMO, req.id)                   // values escape every scope → promotion
  …
}
```

```
$ jet audit memory
  cache.jet:1  region of `MEMO` promoted scope→count
               why:  values inserted in `handle` outlive every scope; no cycles possible in Memo (proven)
               hand: region memo: rc { static MEMO := Memo.new() in memo }
               off:  policy memory { witness: scope }   // makes this line a compile error instead
```

Expert rewrite for a trading system (no counts allowed anywhere): epoch arenas — `region epoch: arena` swapped atomically each rebuild, readers pinned per-request; the policy line makes the promotion impossible and the diagnostic will propose exactly this shape. Same program, three rungs, one model.

### 3.5 Worked example D — parser borrowing an input buffer

```jet
struct Tok  { text: &str, kind: TokKind }            // zero-copy token: a slice of the source
struct Node { kind: NodeKind, kids: List[Node], tok: Tok }

fn lex(src: &str) => List[Tok] { … }                 // inferred: List[Tok[src's region]]
fn parse(toks: &List[Tok]) => Node { … }             // Node built in the CALLER'S destination region

fn main() {
  src :: read_file("prog.jet")
  region ast: arena {
    tree :: parse(&lex(&src)) in ast                 // tokens borrow src; whole AST lands in the arena
    check(&tree)
  }                                                  // AST: one bulk free; src: dies with main
}
```

Everything a production parser wants — zero-copy tokens, arena-built AST, no per-node frees — with **one** written region name (`ast`), and even that only because the programmer *wanted* bulk-free semantics; deleting `region ast:`/`in ast` still compiles, placed in `main.local`. The dangling case (returning `tree` while `src` dies, when tokens are embedded in nodes) is E-MEM-041 with fix `tree.detach()` (deep-copy the borrowed slices into the AST's own region — spelled, costed, and shown in `jet audit copies`).

### 3.6 Worked example E — game entity graph

```jet
region world: pool[Entity](cap: 4096, backing: static)   // slots + generations; init-time, fixed footprint

struct Entity { pos: Vec3, hp: i32, target: Handle[Entity]? }   // cross-links are handles — the canonical spelling

fn spawn(w: &mut World, p: Vec3) => Handle[Entity] { w.world.put(Entity{ pos: p, hp: 100, target: none }) }

fn ai(w: &mut World, h: Handle[Entity]) {
  match w.world[h] {                                       // gen check: one compare
    some(e) => match e.target?.get(&w.world) {
      some(t) => e.pos = chase(e.pos, t.pos),
      none    => e.target = none                           // stale target: deterministic, named, handled
    },
    none => {}                                             // e itself was killed: same story
  }
}

fn frame(w: &mut World, scratch: &mut [u8]) {
  region tmp: arena(backing: fixed(scratch)) {             // per-frame allocs: zero heap, bulk free
    q :: broadphase(&w.world) in tmp
    loop pair, q { resolve(w, pair) }
  }
}

policy memory { steady_state: no os-alloc }                // after init: any allocation site that could reach the OS is a compile error
```

This *is* the pattern AAA teams hand-build in C++ (slot maps, generational indices, frame arenas) — here it is the one canonical spelling, the dangling-entity bug class is deterministically impossible instead of a heisenbug, and the zero-alloc steady state is a checked policy instead of a code-review prayer.

---

## 4. Prior-Art Table

| System | Stole | Paid / why rejected | Subsumed? |
|---|---|---|---|
| **Rust** (ownership, lifetimes, NLL/Polonius) | flow-based borrow ends; exclusivity law; move discipline | lifetime *syntax* as the default surface: annotation tax, variance puzzles, beginner cliff | **Yes** — Rust ≈ all-`scope`-witness Jet with per-value regions; Jet adds inferred region polymorphism + meet-default returns |
| **Zig** (explicit allocators, no hidden control flow) | allocator-as-value → region backings; audit-everything culture | safety is optional (release UB); allocator param threading is manual ceremony | **Yes** — `manual`/`fixed` regions + explicit rungs give Zig's control with the law kept |
| **Swift** (ARC + exclusivity + `~Copyable`) | exclusivity enforcement; non-copyable types | pervasive per-object ARC traffic; cycles leak silently; cost invisible in source | **Yes** — `count` is a per-region policy; region-granularity counting kills most traffic |
| **Go** (GC + escape analysis) | escape inference ergonomics (nobody annotates placement) | one global traced heap: pauses/barriers priced into every program; heuristic escape = perf folklore | **Yes** — `trace` region confines Go to the data that wants it |
| **C/C++** (RAII, smart pointers, the bug record) | RAII = deterministic region close; the bug record as the spec of what to make impossible | manual witness with no auditor: the CVE corpus | **Yes** — `manual` + `trust` obligations = C with a signed ledger |
| **Odin** (context allocator) | ambient region parameter | implicit context is dynamically scoped and unchecked; dangling unpoliced | **Yes** — ambient region is typed, visible, refusable |
| **Mojo** (origins, ASAP-style destruction) | destruction at last use; inferred origins direction | model still hardening around stored refs at scale; origins remain annotation-shaped at API edges | **Yes** — origins ≈ inferred region variables |
| **Vale** (generational references, regions) | `gen` witness wholesale (measured single-digit-% overhead, ~2–10.8% in Vale's own benchmark); region-immutability check elision | gen-everywhere as the *default* taxes the 95% that static proof covers free | **Yes** — `gen` is one tier, applied only where shape demands it |
| **Hylo** (mutable value semantics, subscripts) | MVS as rung 0; projection-style access for expression borrows | second-class references ration the centerpiece: no stored refs = fails server/game/FFI domains | **Partly** — rung 0 is Hylo; storage is regions, which Hylo lacks |
| **Austral** (linear types, capabilities) | linear discipline for `uncopyable`; capability-style `trust` policy | full linearity as default surface = ceremony as a lifestyle | **Yes** — linearity is the expert rung of axis A |
| **Koka/Lean** (Perceus) | precise counting, static inc/dec elision, in-place reuse at count==1 | counting as the *only* story: cross-thread counts, no graphs-with-identity story | **Yes** — `count` regions compile with Perceus elision |
| **Cyclone** (regions) | region containment as the safety argument; region polymorphism | annotation burden killed it: user-facing region variables everywhere | **Yes** — same bones, inference-first skin |
| **MLKit** (region inference) | region inference ambition; the retrospective's lessons (per-call result regions; stack small regions) | whole-program inference was brittle/opaque: one edit flipped allocations into long-lived regions with no visible cause — the canonical warning about silent inference | **Yes** — inference made local, printed, refusable |
| **ASAP** (static deallocation) | "static-automatic gap" framing; frees scheduled at compile time as the `scope`-tier ideal | whole-program shape analysis; imprecision over-retains; no expert dial when it fails | **Partly** — Jet stops at region granularity and falls to *named* dynamic tiers instead of quietly over-retaining |
| **Verona** (region ownership, per-region strategy) | region = unit of ownership AND concurrency; policy per region; region transfer — the closest ancestor | single-entry (iso/sentinel) regions + no cross-region refs is too rigid for rung 0; research-stage, no beginner story | **Yes** — Jet ≈ Verona with inferred membership, cross-region refs up the tree, and a beginner facet |
| **Nim** (ORC/ARC) | cycle-collected RC as a practical `count`+`trace` hybrid | program-wide `--mm` switch = one program, several meanings; hooks change semantics globally | **Yes** — policy is per-region, meaning never forks |
| **Lobster** (compile-time RC elision) | flow analysis eliding ~95% of counts as evidence `count` can be near-free | counting remains the floor for everything; single-threaded assumptions | **Yes** — same elision inside `count` regions |
| **Fil-C** (capability pointers + GC for C) | safety-by-containment for *foreign/legacy* code as a region policy | 1.5–4× slowdown and a GC — right price for vendored C, wrong price for a language's own model | **Partly** — adopted as an optional foreign-region containment mode, not as Jet semantics |
| **CHERI** (hardware capabilities) | hardware bounds/provenance as deploy-time hardening for `trust` and FFI | hardware-dependent; 2× pointer size; doesn't order frees | **No — complementary**: Jet emits capabilities for `*T` on CHERI targets |
| **Pony** (reference capabilities) | aliasing×mutation as THE concurrency question; data-race freedom by type | six capabilities + viewpoint adaptation = expert-only surface | **Yes** — collapsed to 3 permissions + region transfer, same theorem at region granularity |

---

## 5. Self-Scorecard (fixed rubric: 10 domains × 13 metrics)

Legend: ● win (best-in-class case) ◐ contested (named cost or credible rival) ○ lose (named loss). Domains: MCU bare-metal, KRN kernel/driver, GAME AAA frame loop, RT trading/audio, SRV long-lived server, PIPE data/compute, SH one-liner, WASM browser, FFI embedding, AGT agent-authored.

| Metric \ Domain | MCU | KRN | GAME | RT | SRV | PIPE | SH | WASM | FFI | AGT |
|---|---|---|---|---|---|---|---|---|---|---|
| Safety by default | ● | ● | ● | ● | ● | ● | ● | ● | ◐ | ● |
| R/W/reason ergonomics | ◐ | ◐ | ● | ◐ | ● | ● | ● | ● | ◐ | ● |
| Runtime perf + predictability | ● | ● | ● | ● | ◐ | ● | ● | ◐ | ● | ● |
| Compile-time cost of checking | ◐ | ◐ | ◐ | ◐ | ◐ | ○ | ● | ◐ | ◐ | ◐ |
| Learnability (two-sentence test) | ◐ | ○ | ◐ | ○ | ● | ● | ● | ● | ◐ | ● |
| Expert control ceiling | ● | ● | ● | ● | ● | ● | ● | ◐ | ● | ● |
| Diagnostics + repair determinism | ● | ● | ● | ● | ● | ● | ● | ● | ◐ | ● |
| FFI/embedded fit | ● | ● | ● | ● | ● | ● | ● | ◐ | ◐ | ● |
| Verdict fidelity | ● | ● | ● | ● | ◐ | ● | ● | ● | ○ | ● |
| Verdict latency | ● | ● | ◐ | ● | ◐ | ○ | ● | ● | ● | ◐ |
| Verdict actionability | ● | ● | ● | ● | ● | ● | ● | ● | ◐ | ● |
| Context economy | ● | ◐ | ● | ◐ | ● | ● | ● | ● | ◐ | ● |
| Repair determinism | ● | ● | ● | ● | ◐ | ● | ● | ● | ◐ | ◐ |

**The losing and contested cells, named honestly:**

| Cell | Why it loses / is contested |
|---|---|
| PIPE × compile cost — **○** | flow-sensitive borrow checking + region inference + policy monomorphization across a million-line pipeline codebase is real work; function-local inference and incrementality bound it, but Go-class compile speed is not credible here. [INFERENCE] |
| PIPE × verdict latency — **○** | same root cause: in huge modules the edit→verdict loop is where the checking bill arrives; mitigation (pinned public signatures make re-verdicts local) reduces but does not erase it. [INFERENCE] |
| KRN, RT × learnability — **○** | the expert floor here is genuinely higher than C or Zig: policy lattice + witness tiers + region naming must be learned *before* the first driver or trading loop; the two-sentence card covers rung 0, and these domains do not live on rung 0 |
| FFI × verdict fidelity — **○** | at the boundary, safety is **declared** (verbs + obligations), not proved; a lying `gift` annotation is a runtime bug the compiler cannot catch — containment localizes it, nothing eliminates it |
| FFI × safety/ergonomics/diagnostics/context/repair — **◐** | the three-verb ceremony and `trust` fences are real friction versus "just call it" C interop; errors at the fence can only name the obligation, not the foreign root cause |
| SRV × runtime perf — **◐** | `rc` regions can leak cycles (a leak, never unsafety) until the region is switched to `trace`; picking wrong shows up in production memory graphs, not at compile time — the facts/profile tooling is the mitigation, not a proof |
| SRV × verdict fidelity / repair determinism — **◐** | witness *choice* (rc vs trace vs epoch arenas) is a performance judgment with no compile-time verdict; two valid repairs exist until a policy pins one |
| GAME × verdict latency, compile cost — **◐** | heavy monomorphization (pools, policies) in a large game module taxes the loop; the frame-loop *runtime* wins, the *edit* loop pays |
| MCU × ergonomics/learnability, compile cost — **◐** | `heap: none` + static budget proofs mean fighting the budget checker on day one (that is the job, but it is not gentle); whole-program budget proof is the slowest analysis in the design |
| WASM × perf/ceiling/FFI-fit — **◐** | the foreign-GC fence costs handle indirection and forbids Jet layout control over JS-heap objects — inherent to the platform, still a paid cost |
| RT, KRN × context economy — **◐** | refusal policies + named regions + obligations add tokens precisely where the domain demands explicitness; economical *for the domain*, not in absolute terms |
| AGT × verdict latency / repair determinism — **◐** | inference ripple: editing a function body can change its inferred region signature and re-verdict callers; and at the witness seam two repairs are legitimate (name a region vs accept promotion) until `policy … { witness: scope }` is pinned — pinning is the documented agent posture, but it must be *adopted*, not automatic |

A note on what the ●s claim: they claim the *design* wins the cell given the stated mechanism; runtime numbers herein (e.g. gen-check cost, region-count traffic) are argued from mechanism and Vale's published measurements, and are **[INFERENCE]** until Jet-measured.

---

## 6. The Three Worst Things About This Design

**1. Witness promotion is a performance cliff hidden behind a ledger.** Unannotated beginner code has *no syntactic performance model*: the same ten lines are zero-cost or reference-counted depending on whether something escapes, and the only way to know is to read the audit ledger, which beginners — the very people the default serves — will not do. A refactor that makes one value escape can flip a region's witness, and while the ledger prints it and policy can refuse it, this is MLKit's failure mode *managed*, not *eliminated*: I moved the surprise from "where did my memory go" to "where did my nanoseconds go," and I chose that trade on purpose (priority 2 over 3). The cliff is real, and it is the first thing a hostile reviewer should attack.

**2. Dense mutable cross-linked graphs are rationed, and that concedes part of the hypothesis.** Anything shaped like a DOM — many small objects, mutable links in every direction, individually unpredictable lifetimes — gets three honest options: one big region (over-retention), pools + handles (restructuring ceremony), or an `rc`/`gc` region (runtime cost). A first-class global-GC language makes this shape *effortless*, and Jet makes it a *choice*. The mechanisms unify beautifully — GC really is just a region policy here — but the *ergonomics* do not fully unify: the owner's hypothesis is proven for mechanism and only three-quarters proven for experience, and this graph-shaped quarter is exactly where the concession lives.

**3. The annotation debt is not paid off — it is refinanced onto library authors and the witness seam.** "Beginners never annotate" survives because beginners rarely publish region-polymorphic zero-copy APIs; the people who do will write region names, pinned signatures, and policy blocks that are Rust-lifetime-shaped work under a friendlier name, concentrated at exactly the API boundaries where the ecosystem's leverage lives. And the scope/dynamic **seam** — *when* does inference stop proving and start promoting — is a genuinely novel concept with zero existing pedagogy; every prior language teaches one witness, and Jet must teach the *boundary between witnesses*, a thing nobody has yet taught well, including this document.
