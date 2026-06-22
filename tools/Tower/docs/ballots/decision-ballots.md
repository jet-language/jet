# Decision ballots — open owner queue

Every open decision, and **nothing else**. The instant a decision is submitted it
leaves this file: it is recorded in the decision log in
[`syntax-decisions.md`](syntax-decisions.md) and removed here. No "recently
ratified" section, no decided history — decided decisions never reappear.

**House rule for whoever edits this file (enforced — a card missing any of these is
not ballot-ready; Tower v2 Focus Mode renders these as labeled facets, so use the
exact bold labels):** every full decision card carries `**Gist:**` (one VERY short
plain sentence — the headline), `**Story.**` (a real person with an
American-traditional name and what they're doing), `**In the wild:**` (a fenced
```jet block of realistic project code where this bites), `**Other languages:**`
(short fenced blocks for Rust/TS/Swift/etc. when a cross-language compare helps),
`**Tradeoffs:**` (a compact table, one row per option, columns that actually differ —
subagent-reviewed), and a **worked example of every option** (each
`- **Option X — <name>.**` bullet with its own fenced ```jet/```shell block; mark the
recommended one `(recommended)`). Close with `**Recommendation:**` + a one-line why.
Put Owner Q&A in `**Owner Q …**` blocks — Tower routes those to a separate Q&A facet,
so keep them out of the recommendation. Decisions not yet drafted to that bar are
listed below as one-liners with a recommendation; expand one into a full card when
it's time to decide it.

---

## Open decisions

> Open decisions span the cards below: testing ergonomics, the remaining persona-gap
> decisions from the 2026-06-20 run, the JIT/hot-reload cluster (c77), the
> owner-requested constant-binding spelling D-BIND2 (c102), the numeric/serde/iterator
> trio D-NUMOPS1/D-SERDE1/D-ITER1 (c103–c105), plus a deferred-ballots list and
> informational notes. **Note (2026-06-22, batch 2):** the owner batch
> **D-EFF1=B, D-QUAL1=1, D-TXN1=A, D-MIGRATE1=A, D-SOA1=A** has been ratified into
> `syntax-decisions.md` and those cards stripped. **D-DBG2** resolved to **A default +
> expert `jet debug --raw-frames`** (owner: ratify A now, raw frames behind an expert
> flag; once Jet self-hosts there is no underlying Rust) — ratified, card stripped.
> Three follow-ons the ratifications spawned are now **open** below: **D-EFF2** + **D-EFF3**
> (the two effect-system sub-questions the owner asked be crosschecked + carded — surface
> spelling and S60 reopen are already settled by D-QUAL1=1, so these are non-duplicate),
> under **c66**; **D-MIGRATE2A/B/C** (the migration-op vocabulary, converter source,
> and `jet schema` verbs beyond the ratified `rename`) under **c73**; and **D-SOA2** (a better name than "SOA" + the
> three deferred layout questions, owner-requested) under **c78**. Cards **c25** (range
> sugar) and **c55** (REPL v2) are implement-only. Submitting a decision records it in
> `syntax-decisions.md` and removes it from this file.

---

## Safety and syntax cleanup — board cards c09, c82

### D-UNSAFE2 — Keep `#Unsafe` / `#Audit` separate or merge the audit text into unsafe? (rec A)

**Gist:** Keep the existing two-marker model: `#Audit("reason")` documents, `#Unsafe { }`
authorizes.

**Story.** Mara reviews a low-level driver patch. She wants to search for every unsafe
region and separately inspect the human justification for each one. If the audit reason
is folded into `#Unsafe("reason")`, the two jobs become one overloaded marker.

**In the wild:**

```jet
use core.mem

#Audit("MMIO status register; volatile read is required by the device manual")
#Unsafe {
    ready :: mem.read_volatile(status_ptr)
}
```

**Other languages:**

```rust
// Rust splits the operation (`unsafe`) from review policy (`SAFETY:` comments).
// Jet makes both machine-checkable instead of relying on comment convention.
unsafe {
    // SAFETY: pointer is valid for this register.
}
```

**Tradeoffs:**

| Option | Surface | Searchability | Review quality | Churn |
|--------|---------|---------------|----------------|-------|
| **A — keep `#Audit` + `#Unsafe` separate (rec)** | two explicit markers | best | best: reason and region are distinct | none |
| B — `#Unsafe("reason") { }` | one marker | good | weaker for multi-line / multi-reason audits | medium |
| C — comments only | no marker for the reason | poor | unenforced convention | low |

- **Option A — separate markers (recommended).** `#Audit` remains the review artifact;
  `#Unsafe` remains the capability boundary.

```jet
#Audit("calls C API that requires null-terminated buffer")
#Unsafe { c.send(ptr) }
```

- **Option B — reason inside `#Unsafe`.** Shorter, but the safety reason becomes an
  argument to the effect gate rather than its own auditable artifact.

```jet
#Unsafe("calls C API that requires null-terminated buffer") { c.send(ptr) }
```

- **Option C — comments.** Cheapest syntax, but loses the enforceable "unsafe needs an
  audit" rule.

```jet
// audit: calls C API that requires null-terminated buffer
#Unsafe { c.send(ptr) }
```

**Recommendation:** **A**. It matches the current shipped surface, keeps audit text
machine-checkable, and avoids churn in one of Jet's most credibility-sensitive areas.

---

### D-FIXARR1 — Should fixed-size lists `[T#N]` lower to real stack arrays? (rec B)

**Gist:** Make the existing `[T#N]` type a real fixed stack array in codegen.

**Story.** Walter writes firmware for a soil-moisture sensor. He needs a 4096-byte
scratch buffer that lives on the stack and is filled by DMA, so he writes
`#Uninit scratch: [U8#4096]`. Today that fixed-size list still lowers through `Vec`,
which means heap allocation or zero-fill. He wants the fixed-size promise to be real.

**In the wild:**

```jet
use core.mem

struct Frame {
    header: [U8#8]
    crc: [U8#4]
}

fn read_frame(dev: edit Device) -> Frame {
    #Uninit raw: [U8#12]
    dev.fill(edit raw)
    return Frame { header: raw[0..8], crc: raw[8..12] }
}
```

**Other languages:**

```rust
let a: [u8; 12] = [0; 12];                 // fixed, stack
let m = MaybeUninit::<[u8; 12]>::uninit();  // sound because layout is fixed
```

```zig
var raw: [12]u8 = undefined;               // fixed, stack, uninitialized
```

**Tradeoffs:**

| Option | Stack / no heap | `#Uninit` sound | Beginner model | Surface churn |
|--------|-----------------|-----------------|----------------|---------------|
| A — keep `Vec` lowering | no | no | same as list | none |
| **B — real stack array for `[T#N]` (rec)** | yes | yes | "a list whose size is locked" | none |
| C — add separate `[T; N]` spelling | yes | yes | three collection-ish types | high |

- **Option A — keep the current `Vec<T>` lowering.** This preserves the current backend
  but makes `#Uninit` either unsafe or forced to zero-fill, defeating the feature.

```jet
#Uninit raw: [U8#4096]   // secretly cannot be an uninitialized Vec safely
```

- **Option B — lower `[T#N]` to a real fixed stack array (recommended).** Keep S76's
  ratified spelling. Assignment copies when `T` is copyable and moves otherwise, following
  the element type's existing value semantics. A `[T#N]` widens to `[T]` by copying into a
  growable list when passed to a `[T]` slot. `var x := [1, 2, 3]` keeps S76's beginner rule
  and widens to `[Int]`; experts who need a mutable fixed array write `x: [Int#3]`.

```jet
pts: [Int#3] :: [10, 20, 30]
first :: pts[0..2]       // first: [Int], copied out as a growable list
```

- **Option C — introduce a separate fixed-array spelling.** This reopens the spelling
  S76 already settled and adds another collection kind.

```jet
refined: [Int#3] :: [1, 2, 3]
stacked: [Int; 3] :: [1, 2, 3]   // rejected direction: extra spelling
```

**Recommendation:** **B**. S76 already chose `[T#N]`; the only remaining question is
whether the backend honors it. Making `[T#N]` stack-backed unlocks D-UNINIT1 without new
syntax and makes the existing type mean what it says.

---

## Memory & capability model — board card c06


# Ballot c06 — Memory capability model

Source plan: `tools/Tower/docs/sidequests/memory-capability-model.md`. Replaces the
internal three-mode `AccessConvention` (`Read`/`Mutate`/`Move`) with a user-visible
four-capability vocabulary. `take` and `view` are already ratified ownership keywords
(S10, M2); the open work is parameter-position annotations, the copy/share verbs, the
manifest flag, and the inference defaults. Owner has final say on all syntax (I7, I8).

D-CAP1 (keyword spellings), D-CAP4/5/6, and all of c07 (D-TGT1..D-TGT5) were
ratified 2026-06-21 — see `syntax-decisions.md`. **D-CAP2** (copy/share form) and
**D-CAP3** (annotation order) remain open below.

---

### D-CAP2 — `copy` / `share` as keywords vs. method calls (rec A)

**User story.** Theo calls `party.add(player)` and then prints `player.name`. The
compiler tells him `player was taken`. He needs a one-word fix he can paste at the call
site — and it has to be obvious in review that he chose to duplicate or to share, not
that the compiler did it silently behind his back (the plan kills the implicit-clone
path, L0201).

| Option | Form | Visible at glance | Ceremony | Discoverable in diagnostic |
|--------|------|-------------------|----------|----------------------------|
| A | prefix keyword `copy x` / `share x` | yes — leads the line | low | trivial to quote in fix-it |
| B | method `x.copy()` / `x.share()` | trailing, easy to miss | low | reads like any other method |
| C | function `copy(x)` / `share(x)` | yes | low | but looks like stdlib, not a capability verb |
| D | sigil `~x` (copy) / `^x` (share) | terse | lowest | opaque to a beginner |

- **Option A — prefix keywords.** Matches the four-capability vocabulary; the verb leads
  the expression so the intent is the first thing read.

```jet
party.add(copy player)   // duplicate, keep my own
party.add(share player)  // both of us own it
print(player.name)       // ok — I kept a copy / a share
```

- **Option B — method calls.** No new keywords; rides existing method syntax.

```jet
party.add(player.copy())
party.add(player.share())
// duplication hides at the tail of the expression; in `f(g(x).copy())`
// it is easy to miss in review.
```

- **Option C — free functions.** `copy`/`share` as ordinary stdlib calls.

```jet
party.add(copy(player))
// indistinguishable from a user-defined helper; the capability story is invisible.
```

- **Option D — sigils.** Single-character prefixes.

```jet
party.add(~player)   // copy
party.add(^player)   // share
// terse but unteachable; violates the plain-vocabulary goal.
```

**Recommendation:** A — the prefix verb is the only form that is both leading-visible and
quotable verbatim in the post-take fix-it (`use copy player` / `use share player`).

---

**Owner Q (2026-06-21) — refresh me on the capability model; isn't `share` unsupportable
under the borrow checker (mutable = one, immutable = many)? What's inferred vs explicit?**

Your borrow-checker intuition is exactly right, and `share` does **not** break it —
because `share` is **shared ownership, not a shared *mutable* borrow.** The four
capabilities map cleanly onto what Rust already enforces:

| Jet capability | What it means | Rust it lowers to | Borrow-checker rule |
|---|---|---|---|
| `view x` | "I'll only read it" | `&T` | **many** allowed at once |
| `edit x` | "I'll mutate it in place" | `&mut T` | **exactly one**, no other live access |
| `take x` | "I own it now" | `T` (moved) | the old name is dead after |
| `share x` | "we co-own it" | `Rc<T>` / `Arc<T>` | many owners, **read-only** value |

The key: a **shared value is immutable.** `share` hands out multiple co-owners of the
*same* heap value (reference-counted), and you can only `view` through a share — there is
never a `&mut` to a shared value. So "mutable XOR shared" holds: `edit` is the exclusive
one; `view` and `share` are the many-readers cases. (`view` borrows for a scope; `share`
co-owns past the scope. To mutate a shared value you must opt into interior mutability —
an explicit expert tier, never implicit.) This is precisely Rust's `&T` / `&mut T` / `T` /
`Rc<T>` quartet, just renamed into plain verbs.

`copy` (this card) is the **fourth escape hatch**: instead of sharing one value, *duplicate*
it so each party owns an independent copy (Rust `.clone()` into an owned `T`). `copy` and
`share` are the two answers to "I used a value after it was `take`n" — duplicate it, or
co-own it.

**Beginner (inferred) vs expert (explicit):** for a `fn` parameter, the compiler **infers**
the capability from the body — read-only use → `view`; mutation → `edit`; the value escapes
(stored/returned/moved) → `take`. The beginner writes `fn heal(player: Player)` and never
types a capability; the contract is inferred and (for libraries) can be published (D-CAP4/5/6).
The **expert** writes the capability explicitly (`fn heal(player: edit Player)`) to *lock*
the contract so a later refactor that changes it is a visible API break, not a silent one.
At a **call site**, the one thing that is never inferred is duplicate-vs-share after a
`take` (the plan kills implicit clone, L0201) — that's exactly what `copy`/`share` (this
card) make the user say out loud. So: capabilities on *signatures* are inferred-for-beginners
/ explicit-for-experts; `copy`/`share` at *call sites* are always explicit (a one-word,
reviewable choice). Full model: `tools/Tower/docs/sidequests/memory-capability-model.md`.

---

**Owner Q (2026-06-21) — how does this interplay with experts who want low-level memory
controls like references and pointers?**

Cleanly, because they live on **different tiers** and compose. The capabilities are the
*safe* surface; raw pointers are the *expert* tier underneath them (S58 `core.mem` +
`#Unsafe`/`#Audit`):

- **Capabilities ARE Jet's references — the safe ones.** `view T` lowers to `&T`, `edit T`
  to `&mut T`. A beginner gets exactly the reference semantics an expert wants, without the
  word "reference" or any `&`/`*` sigils. So "expert who wants references" is already served
  by `view`/`edit` — they just get the borrow-checker guarantees for free.
- **Raw pointers (`Ptr<T>`) are a deliberate drop below the capability layer**, reached only
  through the expert gate: `use core.mem` (discovery) + an `#Unsafe`/`#Audit` region
  (operation). Inside that audited region the capability/borrow guarantees are *suspended* —
  that's the whole point of `#Unsafe`: the expert takes over the safety proof.
- **They bridge at the function boundary.** The clean idiom: a function keeps **safe
  capability params on its public signature** and drops to a raw `Ptr<T>` only *inside* a
  localized `#Unsafe` block for a hot loop / MMIO / FFI, then hands back safe values. The
  unsafe is contained and audited; callers never see a pointer.

```jet
use core.mem

// public signature stays in the SAFE capability vocabulary:
fn checksum(buf: view [U8]) -> U32 {     // `view` = &[u8], beginner-safe
    sum: U32 := 0
    #Audit("bounds proven by buf.len(); no aliasing — buf is view-only here")
    #Unsafe {
        p :: mem.address_of(buf)         // drop to a raw Ptr<U8> for the hot loop
        for i in 0..buf.len() {
            sum += mem.read(p, offset: i) // expert owns correctness in here
        }
    }
    sum                                   // safe value back out; callers see no pointer
}
```

- **`share` vs manual aliasing:** `share` (Rc/Arc) is the *safe* shared-ownership answer; an
  expert who wants manual aliasing instead uses raw pointers under `#Unsafe` and accepts the
  proof burden. Different tiers, no conflict.
- **Nothing leaks downward:** `Ptr<T>`, `&`, `*` are never in the beginner surface;
  capabilities are. This is the dual-facet exactly — `view`/`edit`/`take`/`share` is the safe
  99% (inferred for beginners, explicit for experts), and `core.mem`'s `Ptr<T>` is the
  audited 1% where an expert takes full manual control. (Ratified low-level tier: S58 / D-LL1.)

---

### D-CAP3 — Annotation order: `player: edit Player` vs. `edit player: Player` (rec A)

**User story.** Priya reads a signature `fn write(file: edit File, data: view Bytes)`.
She parses it left-to-right as "the param `file`, which is an editable File." The
capability is a property of *what the value is here*, not of its name — so it should sit
where the rest of the type information already lives.

| Option | Form | Groups with | Reads as |
|--------|------|-------------|----------|
| A | `player: edit Player` | the type | "player is an edit-Player" |
| B | `edit player: Player` | the binding | "edit the player (a Player)" |
| C | `edit Player player` | C-style | type-first, unlike all other Jet params |

- **Option A — type-side.** Capability attaches to the type, mirroring every other type
  annotation in Jet.

```jet
fn write(file: edit File, data: view Bytes) {
    file.append(data)
}
```

- **Option B — binding-side.** Capability prefixes the parameter name (Rust's `mut`
  pattern position).

```jet
fn write(edit file: File, view data: Bytes) {
    file.append(data)
}
// the keyword now sits where `pub`/`mut`-on-bindings live; a beginner
// can misread `edit file` as an imperative "edit the file" statement.
```

- **Option C — C-style type-first.** Capability and type both precede the name.

```jet
fn write(edit File file, view Bytes data) {
    file.append(data)
}
// inverts Jet's `name: Type` ordering everywhere; non-starter for consistency.
```

**Recommendation:** A — consistent with `name: Type` everywhere else and reads as a
property of the value, not a command.

---

**Owner Q (2026-06-21) — how is this affected when no type is written (inferred type)?**

It isn't, and that actually *reinforces* A. Two facts resolve it:

1. **Capabilities are a *parameter/signature* concept, and parameters carry types.** A
   capability says how a *passed* value may be used by a callee (`view`/`edit`/`take`/`share`);
   it lives on `fn` parameters, which in Jet are written with types (`fn f(x: T)`). Local
   bindings (`name :: expr`) just *own* their value — there's no capability to place there —
   so "inferred-type local" never raises a capability-placement question at all.
2. **When the capability is omitted, it's inferred (the beginner path) — there's no token to
   place.** `fn heal(player: Player)` has no capability written; the compiler infers it
   (D-CAP1/c06). So the *only* time placement matters is when an expert *writes* a capability
   — and under A the capability rides the type, so writing one means writing the type too.
   The rule is one line: **capability and type travel together on the type side; no type
   written ⇒ capability inferred too.** You can never end up with a dangling capability and no
   type.

```jet
fn draw(scene: Scene)            { … }   // no capability → inferred (here: view). beginner.
fn draw(scene: view Scene)       { … }   // expert pins it — type is right there with it (A).

// the only "inferred type" spot for a param is a lambda whose type comes from context:
shapes.each((s) => render(s))            // `s`'s type inferred; capability also inferred (view)
shapes.each((s: edit Shape) => bump(s))  // to PIN edit, you write the type too — A keeps them as one unit
```

This is a point *for* A over B: A keeps everything about "what this value is here" — its type
*and* its capability — in **one place** (the type side). Under B (`edit scene: Scene`) the
capability would sit on the binding while the type sits after the colon, splitting that
information; the inferred-type case is the seam where that split would show. So: inferred type
⇒ inferred capability (nothing to write); explicit capability ⇒ written alongside the type, A's
single-location rule intact.

---

## Effect system — board card c66

### D-EFF2 — Effect polymorphism for higher-order functions (rec D)

**Gist:** A function that takes another function should inherit that function's effects — pick how Jet infers, bounds, and (for experts) spells that.

> **Context.** D-EFF1 ratified Option B — an inferred, erased effect system; surface spelling `#(net, db)` on the signature (D-QUAL1=1); `#Pure fn` = the empty set. D-EFF1's implementation is gated on two sub-questions the owner asked be crosschecked + carded. This is one of them; D-EFF3 (trait-method effects) is the other. E-codes below are illustrative (real codes assigned from the free range at impl).

**Story.** Marcus writes a tidy `#Pure fn summarize(rows)` that calls a stdlib `each(rows, f)` helper, passing a closure that quietly does `db.insert(r)`. He expects either a clean compile or a clear "you said pure but you touch the DB" error — *at his line*, not deep inside `each`. Later, Diane (platform owner) ships a `retry(times, action)` combinator and wants to *publish* that `retry` is exactly as effectful as the `action` handed to it — no more, no less — so callers' inferred effect sets stay tight. And Priya, reviewing a plugin, wants to *demand* that a sort comparator stays pure: `sort_by(items, key)` where `key` is forbidden from touching the network. Three people, one question: **what is a higher-order function's effect set, and who, if anyone, writes it down?**

The hinge is *whether the passed function is statically known at the call.* When `each(rows, (r) => …)` takes a literal lambda or a named `fn`, the compiler sees the body and the effect flows through precisely. When a function is stored in a struct, returned, or dynamically dispatched (S48 boxing), its body is *not* visible at the use site — so to stay sound its effect must live in the type or be assumed maximal. Koka needs effect-row variables because it doesn't monomorphize; Rust never sweats this because closures do. Jet already ratified exactly this split (S48: `<T>` monomorphizes, trait-in-type-position boxes), so the answer must name both halves.

**In the wild:**
```jet
// stdlib combinator — its OWN body is pure; all effect comes from `f`
fn each(xs: [Row], f: fn(Row) -> ()) {
    for r in xs { f(r) }
}

// Marcus's code
#Pure fn summarize(rows: [Row]) -> Int {
    each(rows, (r) => db.insert(r))   // does this compile? what's `each`'s effect here?
    rows.len()
}

// Diane's combinator — wants to PUBLISH "retry is as effectful as action"
fn retry(times: Int, action: fn() -> ()) { … }

// Priya's review — wants to DEMAND the comparator stays pure
fn sort_by(items: [Task], key: fn(Task) -> Int) -> [Task] { … }
```

**Other languages:**
```swift
// Swift `rethrows` — transparent effect polymorphism for ONE effect (throws).
// `map` throws only if its closure does; the caller writes nothing.
func map<T>(_ f: (Element) rethrows -> T) rethrows -> [T]
```
```koka
// Koka — explicit effect-row variable `e`; precise everywhere, no monomorphization.
fun map( xs : list<a>, f : a -> e b ) : e list<b>
```
```rust
// Rust — closures monomorphize, so the effect ("can it panic / await") rides
// the concrete type; no annotation, but no `async`-polymorphism either ("coloring").
fn map<F: Fn(A) -> B>(xs: Vec<A>, f: F) -> Vec<B>
```

**Tradeoffs:**

| Option | Beginner ceremony | Precision (known fn) | Precision (escaping fn) | Can assert callback purity? | Soundness | Familiarity |
|--------|-------------------|----------------------|-------------------------|-----------------------------|-----------|-------------|
| A — transparent only | none | exact | **maximal (lossy)** | no | sound (maximal fallback) | Swift `rethrows`, Rust |
| B — effect-row variables | must read `#(via f)` in stdlib sigs | exact | exact | yes (`#Pure fn` param) | sound | Koka |
| C — boundary wall | annotate every callback | maximal | maximal | only by annotation | sound | none (most conservative) |
| D — hybrid (default A + opt-in B) | none | exact | maximal unless bounded | yes | sound | Swift + Koka |

- **Option A — transparent inference only.** No syntax. A higher-order fn's effect set is the union of its own body's effects plus, *at each call site*, the effects of the function arguments whose bodies are statically known. Effects "flow through" `fn` params automatically.

```jet
fn each(xs: [Row], f: fn(Row) -> ()) { for r in xs { f(r) } }   // own effects: ∅

#Pure fn summarize(rows: [Row]) -> Int {
    each(rows, (r) => db.insert(r))   // ← lambda body is known → #(db) flows in
    rows.len()
}
```
```text
error[E0712]: `summarize` is declared `#Pure` but this call performs a database effect
  --> report.jet:4:5
   |
 4 |     each(rows, (r) => db.insert(r))
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `#(db)` flows in through the function argument `f`
   |
   = why: `#Pure` means the empty effect set; `db.insert` adds `#(db)`.
   = fix: drop `#Pure`, or declare the boundary you accept: `fn summarize(...) -> Int #(db)`
```
The magic case works with zero ceremony. Its real cost is the **escaping** case: when the passed function is stored, returned, or boxed (S48), its body isn't visible, so A must assume the *maximal* effect set to stay sound — and A gives no way to **demand** a callback stays pure (Priya's `sort_by`). Precision-lossy at the boundary, and missing one expert lever.

- **Option B — effect-row variables (Koka-style, Jet-spelled).** An expert names the pass-through with a parameter-reference row, `#(via f)` ("this fn's effect set *is* `f`'s"), and demands purity with a `#Pure fn(...)` parameter type. Static + inferred + **erased** — the row compiles away, no runtime value (I3-safe).

```jet
// Diane PUBLISHES the pass-through; callers get the tight inferred set, even when escaping:
fn retry(times: Int, action: fn() -> ()) #(via action) { … }

// Priya DEMANDS purity — a non-pure comparator is rejected at the call:
fn sort_by(items: [Task], key: #Pure fn(Task) -> Int) -> [Task] { … }

sort_by(tasks, (t) => fetch(t.url))   // rejected
```
```text
error[E0713]: `sort_by` requires a pure comparator, but this one performs a network effect
  --> board.jet:9:24
   |
 9 |     sort_by(tasks, (t) => fetch(t.url))
   |                    ^^^^^^^^^^^^^^^^^^^ `#(net)` here; `key` is declared `#Pure fn(Task) -> Int`
   |
   = why: sort order must be reproducible; an effectful key can reorder results run-to-run.
   = fix: precompute the keys before sorting, or remove the network call from the comparator.
```
Precise everywhere including escaping. Cost: the variable surfaces in stdlib signatures beginners read (`#(via f)`), even though they never *write* it.

- **Option C — boundary wall (v1 limitation).** Effects do not cross the `fn(...) -> ...` type boundary. A function value carries an *unknown/maximal* effect unless its type is annotated, so higher-order code is conservatively impure.

```jet
fn each(xs: [Row], f: fn(Row) -> ()) { for r in xs { f(r) } }

#Pure fn summarize(rows: [Row]) -> Int {
    each(rows, (r) => r.touch())   // pure lambda — but rejected anyway
    rows.len()
}
```
```text
error[E0712]: `summarize` is declared `#Pure` but calls `each`, which may perform any effect
  --> report.jet:4:5
   |
 4 |     each(rows, (r) => r.touch())
   |     ^^^^^ `each` takes a function parameter; its effect is assumed maximal here
   |
   = fix: annotate the boundary — `fn each(xs: [Row], f: #Pure fn(Row) -> ())`
```
Honest and simple, but it taxes the *common, pure* case: even a pure lambda through `each` fails without an annotation. Loses the dual-facet magic.

- **Option D — hybrid: transparent default + opt-in expert row (recommended).** Default is A (flow-through for statically-known functions, zero syntax — Marcus's case just works and errors precisely). Escaping/boxed function values default to the *maximal* effect set (sound). Experts then reach for **two distinct, optional tools**: `#Pure fn(...)` (and `#(net) fn(...)`) **param types** to *demand/bound* what a callback may do (Priya), and `#(via f)` on the **signature** to *publish* a tight pass-through that holds even when the value escapes (Diane). Beginners write neither.

```jet
// Marcus — magic default, no annotation, precise error (Option A behavior):
#Pure fn summarize(rows: [Row]) -> Int {
    each(rows, (r) => db.insert(r))   // E0712: #(db) flows in via `f`
    rows.len()
}

// Diane — publishes the pass-through so escaping callers stay precise:
fn retry(times: Int, action: fn() -> ()) #(via action) { … }

// Priya — demands purity:
fn sort_by(items: [Task], key: #Pure fn(Task) -> Int) -> [Task] { … }
```
```text
error[E0714]: this function value escapes, so its effect cannot be inferred from a body
  --> queue.jet:12:18
   |
12 |     self.handler = (e) => http.post(e)
   |                  ^^^^^^^^^^^^^^^^^^^ stored callbacks carry the maximal effect set
   |
   = note: callers of `dispatch` will conservatively inherit every effect.
   = fix: bound the field's type — `handler: fn(Event) -> () #(net)` — to publish exactly `#(net)`.
```
Magic for beginners (A), full control for experts (B's two levers), sound in the escaping case by maximal fallback — and the expert tools are *additive*, never on the beginner's path.

**Recommendation:** **D** — it's the only option that keeps the no-syntax flow-through beginners need *and* gives experts both levers the story demands (assert-pure and publish-pass-through), while the known-vs-escaping split is decided crisply (precise when known, sound-maximal when escaping) instead of left implicit; effect rows are static + erased, so I3 holds.

**Owner Q (anticipated) — does `#(via f)` add a runtime value or a coloring tax?** No. `#(via f)` is a *static* claim resolved at compile time and **erased** — codegen emits the same plain Rust it would without effects (I3). It is not "async/await coloring": there is no separate calling convention, no wrapper type, no `.await`. A beginner calling a `#(via f)` stdlib function writes an ordinary call; the row only governs which compile-time effect-set checks fire. The only surface a beginner *sees* is the row printed in a signature they read — never one they must write.

---

### D-EFF3 — Effects on trait methods (rec C)

**User story.** Two people, one trait system.

*Mara* writes a hashing library. Her `Hash` trait backs a `Set` whose correctness depends on `hash` being deterministic — same value, same bucket, every run. She wants the compiler to *guarantee* that no downstream `impl Hash` quietly reads a clock, hits the network, or mutates global state. Today she can write "`hash` must be pure" in a doc comment and hope. She wants it to be a compile error when an impl breaks it.

*Devin* holds a `[Drawable]` — a list of trait objects, concrete types erased — and loops over it calling `.draw()` inside a `#Pure fn`. He needs to know, *without seeing any concrete impl*, whether iterating and drawing can touch the network. If the answer is "depends on whichever impl happens to be in the list," his `#Pure fn` guarantee is a lie. He needs the trait itself to tell him the ceiling.

D-EFF3 decides whether a `trait` may speak about effects at all, and if so what that declaration binds.

> **Scope.** This card is about **trait methods** only. Effect *polymorphism* for plain higher-order functions (a `map` that inherits its closure's effects) is the companion card **D-EFF2** — do not re-decide it here. Ratified syntax assumed: traits implemented via the `~~` connector (`impl Point~~Drawable`, S83); effects spelled `#(net, db)` after the param list (D-EFF1 / D-QUAL1=1, e.g. `fn render(self) #(gpu)`); `#Pure fn` = the empty effect set (⊥). Effects are **static, inferred, erased** — no runtime mechanism (I3). E-codes below are illustrative; E0710/E0711 are free today.

> **The spine: static vs. dynamic dispatch.** The three options *only differ at trait-object (dynamic-dispatch) call sites.* For generic, monomorphized static dispatch — `fn sort<T~~Ord>(xs: mut [T])`, where the concrete type is known — effect inference is **exact and per-impl under all three options**: the compiler sees the real `cmp` body and infers its precise effect with zero annotation. The trait-level bound only becomes load-bearing when the concrete type is *erased* — Devin's `[Drawable]`, calling `.draw()` through the object. So "does Option A make my generic `Ord` code impure?" has one answer under every option: **no.** What the options actually contest is what happens when the type is gone.

> **One mechanism, not two.** `#Pure fn hash(self)` in a trait is not a separate feature from `fn render(self) #(gpu)` — it is the same effect-bound machinery with the set fixed to ∅. A `#(gpu)` bound is the non-empty case. The check an impl must pass is always the same: *inferred effects of the impl method ⊆ the trait's declared bound.* `#Pure` is just the bound `{}`.

Each option answers the owner's three real sub-questions: **(1) may a trait declare/forbid effects? (2) must an impl honor it? (3) what is the effect when the trait says nothing?**

| Option | Beginner ceremony | Soundness through dynamic dispatch | Expressiveness | Familiarity |
|--------|-------------------|------------------------------------|----------------|-------------|
| A — traits never mention effects | none — nothing new to learn | **unsound or pessimistic**: a trait-object call assumes maximal (`⊤`) effect; Mara *cannot* require a pure `Hash` at all | low — no way to constrain an impl | type-classes (Haskell): effect lives in the type, not the class |
| B — traits MAY declare an upper bound; impls must satisfy it | opt-in: write `#Pure`/`#(…)` only when you mean it | partial: bound checks impls, but a trait-object call's effect is still unknown unless the bound is also read at the call site | medium — can require purity; can't yet *rely* on it through an object | Rust `const fn` in traits, `unsafe fn` signatures |
| C — declared bound is BOTH the impl obligation AND the dispatch contract | same opt-in as B; un-annotated stays zero-ceremony | **sound**: a trait-object call's effect = the declared bound; Devin's `#Pure fn` is honest | high — purity is both required and dependable | Swift protocol `throws`/`async`; Rust trait-fn qualifiers |

- **Option A — traits may not mention effects (per-impl inference only).** (1) No — a `trait` declares only signatures; `#(…)`/`#Pure` are rejected on a trait method. (2) Nothing to honor. (3) Each impl's method effect is inferred from its own body — exact under static dispatch. But a call through a **trait object** has no concrete body to inspect, so it must conservatively assume the maximal effect set `⊤`. Mara cannot express "`hash` must be pure"; Devin's loop over `[Drawable]` is always treated as touching everything.

```jet
trait Hash {
    fn hash(self) -> U64        // no effect annotation allowed
}

struct Point { x: Int, y: Int }

impl Point~~Hash {
    fn hash(self) -> U64 { self.x.bits ^ (self.y.bits << 1) }   // inferred pure
}

// A VIOLATING impl — except under A nothing is violated; it just compiles:
impl Session~~Hash {
    fn hash(self) -> U64 {
        log_to_server(self.id)      // network I/O inside hash — allowed!
        self.id.bits
    }
}

// Devin's cost: a trait-object call is assumed maximal-effect.
fn fingerprint(items: [Hash]) -> U64 #(net, fs, db) {   // forced to ⊤
    acc := 0
    for it in items { acc ^= it.hash() }   // cannot prove pure — concrete type erased
    acc
}
```
No diagnostic exists because there is no rule to break. Mara's invariant is unstatable; Devin's `#Pure fn` over a `[Hash]` is impossible.

- **Option B — traits MAY declare an effect upper bound; impls must satisfy it.** (1) Yes — a trait method may carry `#(…)` or be `#Pure fn`; this is an *upper bound*. (2) Yes — each `impl` method's inferred effects must be `⊆` the bound, else **E0710**. (3) An *un-annotated* trait method is inferred per-impl (exact under static dispatch). The bound is checked at the impl, but it is **not** consulted at trait-object call sites — so a `[Renderer]` call still falls back to maximal effect even when the trait *did* declare a bound. B fixes Mara halfway (she can require purity) but not Devin (he still can't rely on it).

```jet
trait Hash {
    #Pure fn hash(self) -> U64        // bound = ∅ : every impl's hash must be pure
}

struct Point { x: Int, y: Int }

impl Point~~Hash {
    #Pure fn hash(self) -> U64 { self.x.bits ^ (self.y.bits << 1) }   // ∅ ⊆ ∅ — ok
}

// VIOLATING impl:
impl Session~~Hash {
    fn hash(self) -> U64 {
        log_to_server(self.id)
        //  ^^^^^^^^^^^^^^^^^^^
        // error[E0710]: `impl Session~~Hash`'s `hash` has effect `#(net)`, but
        //               trait `Hash` declares `hash` as `#Pure` (effect bound ∅).
        //   why: `Hash` promises a pure `hash`; callers (e.g. `Set`) rely on it.
        //   fix: remove the network call, or move it behind a separate method.
        self.id.bits
    }
}
```
Mara is satisfied. But Devin's `fingerprint(items: [Hash])` *could* now be proven pure — and under B it still isn't, because the call site doesn't read the bound. That gap is exactly what C closes.

- **Option C — the declared bound is BOTH the impl obligation AND the dispatch contract. (recommended)** (1) Yes — same spelling as B. (2) Yes — same `⊆` check, same **E0710** on violation. (3) An *un-annotated* trait method is still inferred per-impl (exact, zero-ceremony under static dispatch); when called through a **trait object**, an un-annotated method is treated as maximal `⊤` — but now with a fix-it pointing at the cure. The new power: at a **trait-object call site**, the call's effect *is* the trait's declared bound. So a `[Renderer]` declared `#(gpu)` contributes exactly `#(gpu)` to its caller — no more, regardless of impl. Mara *and* Devin are both satisfied, with one mechanism.

```jet
trait Renderer {
    fn render(self) #(gpu)        // upper bound: any impl may use at most #(gpu)
}

struct Sprite { tex: Texture }

impl Sprite~~Renderer {
    fn render(self) #(gpu) { gpu_blit(self.tex) }   // #(gpu) ⊆ #(gpu) — ok
}

// Devin's loop is now provably bounded — no concrete type needed:
fn draw_all(items: [Renderer]) #(gpu) {     // exactly #(gpu), not ⊤
    for it in items { it.render() }         // each call's effect = the bound #(gpu)
}

// VIOLATING impl — exceeds the dispatch contract:
impl NetSprite~~Renderer {
    fn render(self) {
        fetch_texture_over_http(self.url)   // adds #(net)
        //  ^^^^^^^^^^^^^^^^^^^^
        // error[E0710]: `impl NetSprite~~Renderer`'s `render` has effect
        //               `#(net, gpu)`, exceeding trait `Renderer`'s bound `#(gpu)`.
        //   why: anyone holding a `[Renderer]` was promised at most `#(gpu)`;
        //        a hidden `#(net)` would silently break their effect guarantee.
        //   fix: prefetch the texture before constructing `NetSprite`, or widen
        //        the trait: `fn render(self) #(gpu, net)`.
        gpu_blit(self.tex)
    }
}
```
And the un-annotated-through-object case carries its own guidance:

```jet
trait Drawable {
    fn draw(self)                 // no bound declared
}

fn paint(items: [Drawable]) #(gpu) {
    for it in items { it.draw() }
    //                ^^^^^^^^^
    // error[E0711]: `Drawable::draw` declares no effect bound, so a call through
    //               a `[Drawable]` trait object must assume the maximal effect set.
    //               `paint` cannot be held to `#(gpu)` here.
    //   why: with the concrete type erased, the compiler has no body to inspect.
    //   fix: declare the bound on the trait — `fn draw(self) #(gpu)` — or call
    //        each item via a generic bound `fn paint<T~~Drawable>(…)` so the
    //        concrete effect is inferred per type (static dispatch).
}
```
This error fires *only* when the surrounding context demands an effect ceiling (inside a `#Pure fn` or a `#(…)`-bounded fn). Plain unconstrained code calling `.draw()` through a `[Drawable]` is fine — it just inherits `⊤`, the honest answer.

**How other languages do this.**

| Language | Mechanism | Jet takeaway |
|----------|-----------|--------------|
| Rust — `const fn` in trait | a trait may require methods be `const`; impls must honor it; non-`const` impl is a compile error | Closest precedent: a *qualifier on a trait method that impls must satisfy*. Jet's effect bound is the same shape, generalized from "const-evaluable" to "effect set". |
| Rust — `unsafe fn` in trait signature | the trait fixes the safety qualifier; impls match it | Same impl-obligation idea; a binary qualifier where Jet's is a lattice of effects. |
| Haskell — type classes | the class carries **no** effect info; effects live in the method's monadic return type (`IO a`), not the class | Option A in spirit: the constraint is unstatable at the class. Jet rejects this — effects belong on the method so dispatch can read them (C). |
| Koka — effect rows + interfaces | effects are first-class row variables; an interface method can carry a concrete or polymorphic effect | C is the monomorphic specialization: a fixed declared bound per trait method, read at both impl-check and dispatch. Row-polymorphic trait methods are D-EFF2's territory. |
| Swift — protocol `throws` / `async` | the protocol fixes whether a method can throw / suspend; impls conform; callers through an existential see the protocol's declaration | Direct analogue of C: a protocol-declared *effect* is both the conformance rule *and* a fact the caller through an existential relies on. Effects generalize that one bit to a set. |

**Recommendation:** C — the declared bound is both the impl obligation (**E0710** on `⊄`) and the dispatch contract (a trait-object call's effect = the declared bound; un-annotated methods inferred per-impl statically, **E0711** with a fix-it when an un-annotated method is called through an object under an effect ceiling). It is the only option that satisfies *both* user stories with one mechanism: A cannot even state Mara's pure-`Hash` requirement and forces Devin to `⊤`; B states it but still strands Devin at the dispatch boundary; C subsumes B's check and closes the boundary, so safe-by-default holds *through* dynamic dispatch — and the common generic case stays zero-ceremony because static dispatch still infers exactly.

## Safe schema changes — board card c73

### D-MIGRATE2A — Migration operation vocabulary (rec A/A/A/B)

**User story.** Sam maintains `jet-records`, a published library. In v1 the core type was:

```jet
#PublishedSchema
struct UserRecord {
    name: String,
    price: Int,        // cents
    legacy_id: String,
}
```

In v2 Sam wants to (1) rename `name` → `display_name` (already ratified, D-MIGRATE1), (2) drop `legacy_id` (an internal artifact some consumers serialized anyway), and (3) change `price` from raw `Int` cents to a `Usd` value type. Without migrations the compiler fires E0910 on all three. The ratified vocabulary has exactly one verb — `rename old -> new`. This card decides the rest.

> **Context.** D-MIGRATE1 ratified Option A — compile-time `#PublishedSchema` shape enforcement; a breaking change with no migration is E0910; `migration Type { rename a -> b }` unblocks it. Scope was deliberately locked to `rename`. Owner requested this be split into multiple cards so each choice is independent. This card decides only the migration block's operation vocabulary.

**Gist:** Pick the extra verbs allowed inside `migration Type { }`.

---

#### Add a field with a default

A new field has no value in already-serialized data; the migration supplies a default.

- **Option A — `add f: T = val` (recommended).** Reads like a struct field with an initializer; reuses the `=` already used for struct-field defaults.

```jet
migration UserRecord {
    rename name -> display_name
    add verified: Bool = false
}
```

- **Option B — `add f: T default val`.** A `default` keyword used nowhere else in field syntax.

```jet
migration UserRecord {
    add verified: Bool default false
}
```

| | `add f: T = val` | `add f: T default val` |
|---|---|---|
| Reads like | a field decl with init | a config/SQL clause |
| Consistency with Jet | mirrors struct-field defaults | no parallel in ratified syntax |
| Ambiguity | low | low |

**Recommendation:** `add f: T = val` — one rule shared with struct-field defaults, not two.

---

#### Sub-question 1b — Remove / drop a field

- **Option A — `remove f` (recommended).**

```jet
migration UserRecord {
    remove legacy_id
}
```

- **Option B — `drop f`.**

```jet
migration UserRecord {
    drop legacy_id
}
```

| | `remove` | `drop` |
|---|---|---|
| Reads like | "this field no longer exists" | SQL `DROP` (db-level destruction) |
| Consistency with Jet | plain English, neutral | borrows SQL connotation |
| Ambiguity | low | low |

**Recommendation:** `remove` — `drop` implies database-level destruction; the migration step is a schema transformation, not a destructive command.

---

#### Sub-question 1c — Change a field's type

- **Option A — `change f: Old -> New via <expr>` (recommended for the inline case).** `change` names the op; `Old -> New` reuses the `->` arrow; `via` introduces the conversion.

```jet
migration UserRecord {
    change price: Int -> Usd via (cents) => Usd(cents)
}
```

- **Option B — `transform f: Old -> New { <expr> }`.** A single verb with a block body for multi-line converters.

```jet
migration UserRecord {
    transform price: Int -> Usd {
        (cents) => Usd(cents)
    }
}
```

- **Option C — `change f: Old -> New` + a named `impl Old -> New { }`.** The migration line declares only; the converter is the ratified D-ERR-CONV construct (see sub-Q2).

```jet
migration UserRecord {
    change price: Int -> Usd
}

impl Int -> Usd {              // reuses D-ERR-CONV's declaration surface
    fn convert(cents: Int) -> Usd { Usd(cents) }
}
```

| | `change … via` | `transform … { }` | `change` + `impl` |
|---|---|---|---|
| Reads like | field mutation + routing | block transformation | declaration + named converter |
| Consistency with Jet | `->` ratified; `via` localized-new | new keyword + new block form | fully reuses D-ERR-CONV |
| Ambiguity | low | low | low (if D-ERR-CONV known) |

**Recommendation:** `change f: Old -> New via <expr>` for inline one-liners, with the `impl Old -> New { }` form (sub-Q2) when the conversion is non-trivial. Absence of `via` means "find the `impl Old -> New` in scope."

---

#### Sub-question 1d — Reorder fields

- **Option A — `reorder [f1, f2, f3]`.** Explicit full ordering.

```jet
migration UserRecord {
    reorder [display_name, price, verified]
}
```

- **Option B — no verb; reordering is not a tracked breaking change (recommended).** Field order is not part of the `#PublishedSchema` baseline; order-sensitive wire formats are a serialization-library concern.

```jet
// no migration needed — reordering fields compiles cleanly
#PublishedSchema
struct UserRecord { price: Usd, display_name: String, verified: Bool }
```

**Recommendation:** no `reorder` verb — keep the vocabulary minimal; reordering is serialization-format-specific and belongs to a serializer's own versioning. Add later only on concrete need.

---

**Recommendation:** Ratify the vocabulary as **A/A/A/B**: `add f: T = val`, `remove f`,
`change f: Old -> New via <expr>`, and no `reorder` verb. This keeps the block small,
plain, and focused on schema shape changes that actually affect compatibility.

---

### D-MIGRATE2B — Migration type-change converter source (rec C)

**Gist:** Decide whether `change f: Old -> New` gets its converter inline, from a named
conversion impl, or both.

**Story.** Sam changes `price: Int` into `price: Usd`. For a trivial cents wrapper, an
inline lambda is clearest. For a real legacy cleanup that parses text, clamps old bad
values, and logs a note, a named converter is cleaner and testable.

**In the wild:**

```jet
migration UserRecord {
    change price: Int -> Usd via (cents) => Usd(cents)
}
```

**Other languages:**

```rust
impl From<i64> for Usd { fn from(cents: i64) -> Usd { Usd(cents) } }
```

```sql
ALTER TABLE users ALTER COLUMN price TYPE usd USING cents_to_usd(price);
```

**Tradeoffs:**

| Option | Best for | Local readability | Reuse/testability | Extra mechanism |
|---|---|---|---|---|
| A — always inline `via` | short transforms | best | weak | lambda in migration |
| B — named converter only | non-trivial transforms | split from field | best | reuses conversion impl |
| **C — inline wins, else named impl (rec)** | both | best for short case | best for long case | one resolution rule |

- **Option A — always inline `via` lambda.** Conversion sits next to the field.

```jet
migration UserRecord { change price: Int -> Usd via (cents) => Usd(cents) }
```

- **Option B — named converter only, via `impl Old -> New { }` (D-ERR-CONV).** The migration line carries no expression; the compiler resolves the `impl` in scope.

```jet
impl Int -> Usd { fn convert(cents: Int) -> Usd { Usd(cents) } }

migration UserRecord { change price: Int -> Usd }   // resolves impl Int -> Usd
```

- **Option C — both; inline `via` wins, else fall back to `impl` (recommended).** Resolution order: (1) inline `via`, (2) `impl Old -> New` in scope, (3) E0910 asking for one.

```jet
// short form — inline:
migration UserRecord { change price: Int -> Usd via (cents) => Usd(cents) }

// long form — reuse a named converter:
impl Int -> Usd { fn convert(cents: Int) -> Usd { Usd(cents) } }
migration UserRecord { change price: Int -> Usd }
```

> **Open point flagged for the owner / impl.** D-ERR-CONV's `impl Source -> Target { }` is ratified-scoped to *error* conversions that `?` applies automatically. Reusing the same surface here means a migration type-change conversion shares the declaration but is invoked **only by the migration machinery at data-load time, not by `?` at runtime**. Confirm that reuse (recommended — one conversion construct), or carve a distinct `migration-only` converter form if conflating the two surfaces is unwelcome.

**Recommendation:** Option C — inline `via` for the common one-liner; the ratified `impl Old -> New` for non-trivial conversions. No parallel mechanism; one predictable resolution rule.

---

### D-MIGRATE2C — `jet schema` command surface (rec A/B)

**Gist:** Decide the user-facing schema maintenance commands after migration blocks exist.

**Story.** Sam has ten released versions. He wants a human-readable status command before a
release, and later he wants to collapse old migrations into a new baseline so the repository
does not accumulate years of compatibility code.

**In the wild:**

```shell
$ jet schema status
UserRecord (jet-records v1.0.0 → current)
  baseline  v1.0.0   .jet/cache/schema/UserRecord.snapshot
  ├─ [1] rename  name -> display_name
  ├─ [2] remove  legacy_id
  ├─ [3] change  price: Int -> Usd  (via impl Int -> Usd)
  └─ [4] add     verified: Bool = false
  No uncommitted breaking changes.
```
`jet schema status` — confirm this spelling/output (no decision needed).

**Other languages:**

```shell
$ diesel migration run
$ prisma migrate status
$ alembic current
```

**Tradeoffs:**

| Choice | Option A | Option B | Recommendation |
|---|---|---|---|
| Squash cutoff spelling | `jet schema squash --before <ver>` | `jet schema squash <ver>` | **A** |
| Separate CI gate | `jet schema check` | rely on `jet build` / E0910 | **B** |

**Squash flag:**

- **Option A — `jet schema squash --before <ver>` (recommended).** Flag names the cutoff; everything strictly before is collapsed into a baseline.

```shell
$ jet schema squash --before v2.0.0
Collapsed 4 migration steps into baseline UserRecord@v2.0.0
```

- **Option B — `jet schema squash <ver>` (positional).** Shorter, but ambiguous about "through" vs "before" vs "at".

```shell
$ jet schema squash v2.0.0
```

**Recommendation:** `--before <ver>` — the flag name removes the through/at ambiguity that is a real source of mistakes.

**CI gate verb:**

- **Option A — add `jet schema check`.** Exits non-zero on an undeclared breaking change.

```shell
$ jet schema check
Error [E0910]: UserRecord has a breaking change with no migration declared.
$ echo $?
1
```

- **Option B — no extra verb; `jet build` is already the gate (recommended).**

```shell
$ jet build      # E0910 already fails the build in CI
```

**Recommendation:** no `jet schema check` — E0910 is already a compile error; a separate verb would either re-implement detection (violating I3) or shell out redundantly. Add a fast pre-compile lint later only if needed.

---

#### Full worked example — Sam's v2 migration

```jet
#PublishedSchema
struct UserRecord {
    display_name: String,
    price: Usd,
    verified: Bool,
}

impl Int -> Usd {                                    // named converter (D-ERR-CONV surface)
    fn convert(cents: Int) -> Usd { Usd(cents) }
}

migration UserRecord {
    rename name -> display_name        // D-MIGRATE1 (ratified)
    remove legacy_id                   // 1b
    change price: Int -> Usd           // 1c + 2: resolved via impl Int -> Usd
    add verified: Bool = false         // 1a
}
```

| Card | Recommendation |
|---|---|
| D-MIGRATE2A | `add f: T = val`; `remove f`; `change f: Old -> New via <expr>`; no `reorder` |
| D-MIGRATE2B | both converter forms; inline `via` wins, fall back to `impl Old -> New` |
| D-MIGRATE2C | `jet schema squash --before <ver>`; use `jet build` as the CI gate |

## Persona-gap decisions — board cards c83–c96 (2026-06-20 persona run)

13 owner decisions shaken out of the 2026-06-20 persona run, one per gap that needs a
user-facing call. Each board card (c83–c96) links its plan in `sidequests/`. House format,
no effort column. `c95` is implement-only (no decision); `D-PUBLISH1` is a stub in the
deferred list until M12.2 infra is verified.

### D-JSONOUT1 — Serialize a typed struct to JSON (rec A)

**User story.** Elena's ETL ends by emitting JSON. `json.render` takes the dynamic
`JSON` enum, so she hand-builds `Object([("id", Number(o.id)), …])` for every
struct — verbose and drift-prone. She wants `json.render(order)` to just work when
`order: Order`.

| Option | Mechanism | Needs S56? | One annotation for in+out | Field rename |
|---|---|---|---|---|
| A — built-in `#[Serialize]` marker | compiler honors via comptime field walk | no | yes (drives decode too) | via `#json("name")` |
| B — explicit `to_json(self)` method | user writes per type | no | no | manual |
| C — S56 user-derive | user-written derive | **yes (blocked)** | yes | yes |

- **Option A — built-in `#[Serialize]` marker.** A *built-in* marker (distinct from
  S56 user-derives; D-ATTR2 ratified the bare-marker form) tells the compiler to
  generate render (and decode) by walking fields.

```jet
#[Serialize]
struct Order { id: Int, customer: String, total: Float }

fn main() {
    order :: Order{ id: 7, customer: "Mara", total: 19.5 }
    print(json.render(order))      // {"id":7,"customer":"Mara","total":19.5}
}

#[Serialize]
struct Bad { cb: fn(Int) -> Int }
// error: `Bad` is not serializable — field `cb` is a function
```

- **Option B — explicit `to_json` method.**

```jet
impl Order {
    fn to_json(view self) -> JSON {
        JSON.object([("id", JSON.num(self.id)), ("customer", JSON.str(self.customer)),
                     ("total", JSON.num(self.total))])
    }
}
// works with no compiler help, but every struct re-writes the obvious thing
// and it drifts when a field is added.
```

- **Option C — S56 user-derive.**

```jet
derive Order ~~ Serialize     // S83 connector
// error: user-defined derives (S56) are deferred to Epoch 3
```

**Recommendation:** A — a built-in `#[Serialize]` marker (riding the already-shipped
comptime field walk and the ratified bare-marker form) closes the gap now without
S56, and the *same* marker should drive both `json.render` and typed decode so one
annotation covers in and out. **Owner must confirm** whether a built-in Serialize
marker is intended distinct from S56 user-derives before this ratifies — that
boundary is the real question.

---

### D-ARGS1 — Structured command-line argument parsing (rec A)

**User story.** Amara replaces a bash script with Jet. Her tool takes
`--input file`, `--verbose`, and a positional command. `io.args()` gives her a raw
`[String]`; she writes the flag loop by hand and her `--help` is a `print`. She
wants to declare the flags her tool accepts and get typed values plus a generated
`--help` and good errors for free.

| Option | Spec form | Needs S56/comptime? | Auto `--help` | Typed values |
|---|---|---|---|---|
| A — builder spec | `args.flag(...).option(...)` | no | yes | yes |
| B — `#[Args]` struct | fields are flags | yes (comptime/S56) | yes | yes |
| C — declarative table | a `[ArgSpec]` value | no | yes | yes |

- **Option A — builder spec value.** Build a spec, parse `io.args()` against it.

```jet
fn main() ? {
    spec :: args.spec()
        .flag("verbose", short: 'v', help: "noisy output")
        .option("input", String, required: true, help: "input file")
        .positional("command", String)
    cli :: spec.parse(io.args())?       // prints generated --help on `--help`
    if cli.flag("verbose") { log.verbose() }
    run(cli.option("input"), cli.positional("command"))
}
// unknown flag:
//   error: unknown flag `--inpt` (did you mean `--input`?)
//   usage: tool [--verbose] --input <file> <command>
```

- **Option B — `#[Args]` struct.** Declare a struct; fields become flags.

```jet
#[Args]
struct Cli {
    #flag(short: 'v')  verbose: Bool,
    #option(required)  input:   String,
    #positional        command: String,
}
// cli :: args.parse<Cli>(io.args())?
// error: deriving the parser needs user-derives (S56), deferred to Epoch 3
```

- **Option C — declarative table.** A value listing the args.

```jet
cli :: args.parse(io.args(), [
    args.flag("verbose", short: 'v'),
    args.option("input", String, required: true),
    args.positional("command", String),
])?
// equivalent to A's data without the builder chain.
```

**Recommendation:** A — a builder spec gives typed values, auto-generated `--help`,
and teaching errors *today* with no dependency on S56, and the spec value can later
back a `#[Args]` struct form (B) once derives land — same parser underneath. The
generated `--help` and error text are product copy and must be snapshot-tested.

---

### D-MATHLIB1 — Linear-algebra library home & scope (rec A)

**User story.** Marcus needs vectors and matrices — dot, cross, matmul, and ideally
decompositions/FFT — for a physics simulation. Today `core.math` is scalar `Float`
ops only, so he writes matrices from scratch or drops to Rust FFI. He wants a
numerics library that ships with the language.

| Option | Home | Dimensions | v1 scope |
|---|---|---|---|
| A — `jet.linalg` ring package | a ring package (like csv/toml/regex) | comptime-sized + runtime | small vectors + matrices core, FFT later |
| B — `core.math` extension | built into core | runtime-sized | grows core surface |
| C — expert-only `#Unsafe` BLAS binding | FFI overlay | runtime | thin wrapper, expert-tier |

- **Option A — `jet.linalg` ring package (rec).** Numerics ships as a first-party
  ring package, consistent with regex/csv/toml.

```jet
use jet.linalg

fn main() {
    a :: Matrix<2,2>{ {1.0, 2.0}, {3.0, 4.0} }     // comptime-sized (rides S76/c82)
    b :: Matrix<2,2>{ {5.0, 6.0}, {7.0, 8.0} }
    print((a * b).trace())                          // 67.0
    v :: Vec3{ 1.0, 0.0, 0.0 }
    print(v.cross(Vec3{ 0.0, 1.0, 0.0 }))           // Vec3{0,0,1}
}
```

- **Option B — extend `core.math`.** Matrices live in core.

```jet
use core.math
m :: math.Matrix.new(2, 2)        // runtime-sized only
// pulls a large numerics surface into core, which every program carries.
```

- **Option C — expert `#Unsafe` BLAS binding.** A thin FFI wrapper, expert-tier.

```jet
use jet.blas
#Unsafe { jet.blas.dgemm(a, b, out) }   // raw, fast, expert-only — no beginner path
```

**Recommendation:** A — a `jet.linalg` ring package keeps core small (I8), matches
how regex/csv/toml already ship, and can offer comptime-sized matrices (riding the
fixed-array work, c82/S76) for the cache-friendly, bounds-checked layout numerical
code wants. The I6 question (native vs bootstrap a numerics crate) is an
implementation gate flagged in the plan, decided like regex (c79).

---

### D-SIMD1 — SIMD primitive surface & safety tier (rec A)

**User story.** Marcus has a hot kernel adding two large `F32` arrays. He wants it
vectorized. Today there are no SIMD types — the expert tier gives him raw pointers
(`#Unsafe`/`Ptr<T>`) but no lanes, so he can't write portable vector math.

| Option | Surface | Safety | Portability |
|---|---|---|---|
| A — safe portable lane types `F32x4` | explicit lane values + ops | safe by default (`std::simd`) | portable; falls back when ISA absent |
| B — auto-vectorize hint on a loop | `#vectorize loop …` | safe | compiler-best-effort |
| C — target intrinsics behind `#Unsafe` | raw arch intrinsics | expert-only `#Unsafe` | per-target |

- **Option A — portable lane types (rec).** First-class `F32x4`/`F64x2` with safe
  ops that lower to portable SIMD.

```jet
fn add(xs: [F32], ys: [F32], out: mut [F32]) {
    loop i in (0..xs.len()).step(4) {
        a :: F32x4.load(xs, i)
        b :: F32x4.load(ys, i)
        (a + b).store(out, i)         // safe; lowers to std::simd, scalar fallback
    }
}
```

- **Option B — a vectorize hint.** Annotate a scalar loop; the compiler tries to
  vectorize.

```jet
#vectorize
loop i in 0..xs.len() { out[i] = xs[i] + ys[i] }
// no new types, but "best-effort" — Marcus can't tell if it actually vectorized.
```

- **Option C — target intrinsics behind `#Unsafe`.** Raw architecture intrinsics.

```jet
#Audit("AVX2 packed add; falls back required on non-AVX targets")
#Unsafe { simd.x86.mm256_add_ps(a, b) }
// maximum control, expert-only, non-portable, and unsafe — no beginner/portable path.
```

**Recommendation:** A — portable safe lane types (`std::simd` model) keep SIMD
*memory-safe by default* (I1) and portable across targets, which fits Jet's
safe-by-default-with-expert-opt-in spine; raw target intrinsics (C) remain
available behind `#Unsafe`/`#Audit` for the last-mile expert case. B is a nice
additive sugar but can't be the primitive (it's unpredictable).

---

---

## Proposal follow-ups and comptime gaps — board cards c64, c65, c97, c98

### D-REACT1 — Should reactive/dataflow be core semantics, tooling, or a library? (rec B)

**Gist:** Keep normal execution semantics; use the derived dataflow graph for tooling and
ship reactivity as an opt-in library.

**Story.** Priya builds a dashboard. She wants cells to recompute when their inputs
change, but she does not want every Jet program to become a spreadsheet runtime where
assignment secretly installs observers.

**In the wild:**

```jet
use jet.reactive as rx

price := rx.signal(10)
qty := rx.signal(3)
total := rx.derived(() => price.get() * qty.get())
```

**Other languages:**

```swift
// SwiftUI makes reactivity a framework layer, not the whole language.
@State var count = 0
```

```ts
// Solid/Svelte: explicit reactive layer on top of normal JavaScript semantics.
const total = createMemo(() => price() * qty())
```

**Tradeoffs:**

| Option | Core semantics risk | Tooling value | Beginner surprise | Scope |
|--------|---------------------|---------------|-------------------|-------|
| A — make Jet reactive by default | high | high | high | language-wide |
| **B — dataflow graph for tooling + `jet.reactive` lib (rec)** | low | high | low | opt-in |
| C — no reactive story | low | low | low | none |

- **Option A — reactive by default.** Every binding participates in a dependency graph.
  Powerful, but collides with ownership, mutation, and predictable systems semantics.

```jet
total :: price * qty   // later price changes would imply total changes too
```

- **Option B — tooling graph plus opt-in library (recommended).** The compiler can expose
  dependency information to IDEs/build tools, while runtime reactivity is explicit.

```jet
total := rx.derived(() => price.get() * qty.get())
```

- **Option C — no reactive surface.** Keeps the language small but forfeits an obvious
  application-framework layer.

```jet
// users hand-roll observers, invalidation, and graph debugging
```

**Recommendation:** **B**. It captures the Blueprint/tooling value without changing the
meaning of ordinary bindings, and it leaves app-level reactivity to a library where it
belongs.

---

### D-FANOUT2 — Add namespace/member fan-out sugar beyond S75 call fan-out? (rec B)

**Gist:** Defer namespace/member fan-out; keep only the already-ratified call fan-out
until real use proves the second axis.

**Story.** Alana sees `f.[a, b, c]` and asks whether `service.{start, stop, status}` or
`obj.[x, y]` should also exist. It is tempting sugar, but it risks making `.` carry too
many meanings right after S75 landed.

**In the wild:**

```jet
scores :: normalize.[raw_a, raw_b, raw_c]   // S75: already ratified and implemented
```

**Other languages:**

```js
// JavaScript has destructuring, not member fan-out syntax.
const { start, stop } = service
```

```swift
// Swift keeps key paths explicit.
users.map(\.name)
```

**Tradeoffs:**

| Option | Expressiveness | Parser/formatter risk | Reader cost | Timing |
|--------|----------------|-----------------------|-------------|--------|
| A — add namespace/member fan-out now | medium | medium | medium | early |
| **B — defer; keep S75 only (rec)** | enough for now | low | low | evidence-driven |
| C — reject permanently | low | low | low | closes useful door |

- **Option A — add the sugar now.** Introduce a second fan-out axis for member or
  namespace selection.

```jet
handlers :: routes.{get, post, delete}
```

- **Option B — defer (recommended).** Keep S75's function-call fan-out as the only
  shipped form and collect examples before adding another dot/bracket meaning.

```jet
handlers :: [routes.get, routes.post, routes.delete]
```

- **Option C — reject permanently.** Forces explicit lists forever.

```jet
handlers :: [routes.get, routes.post, routes.delete]
```

**Recommendation:** **B**. S75 is new; another compact fan-out form should wait for
evidence so Jet does not accrete clever punctuation faster than users can read it.

---

### D-STRPARSE1 — String parse APIs and comptime `Result`/`Option` evaluation (rec A)

**Gist:** Add normal runtime string parsing APIs and allow comptime evaluation through
`Result`/`Option` for pure parse paths.

**Story.** Nora writes a comptime schema loader. She can read embedded text, but she
cannot cleanly split it into lines and parse numbers at comptime because the string
methods and `Result` flow are incomplete there.

**In the wild:**

```jet
const ports = embed_str("ports.txt")
    .lines()
    .map((line) => line.parse_int()?)
```

**Other languages:**

```rust
let n: i64 = "42".parse()?;
```

```zig
const n = try std.fmt.parseInt(i64, "42", 10);
```

**Tradeoffs:**

| Option | Runtime usefulness | Comptime usefulness | Implementation scope |
|--------|--------------------|---------------------|----------------------|
| **A — add parse APIs + comptime `Result`/`Option` (rec)** | high | high | medium |
| B — runtime APIs only | high | low | low |
| C — no new APIs | low | low | none |

- **Option A — both runtime parse APIs and comptime `Result`/`Option` (recommended).**

```jet
n :: "42".parse_int()?
lines :: text.lines()
```

- **Option B — runtime only.** Good for apps, still blocks comptime data ingestion.

```jet
n :: input.parse_int()?   // ok at runtime, not in const/comptime paths
```

- **Option C — no new surface.** Users keep writing ad hoc parsers.

```jet
// manual digit loops everywhere
```

**Recommendation:** **A**. Parsing text into typed values is a core library expectation,
and the comptime path is exactly where Jet wants schema/config ingestion to feel strong.

---

### D-CTCORE1 — Should comptime execute Core-module calls inline? (rec B)

**Gist:** Add a curated comptime Core whitelist first, not the whole runtime Core.

**Story.** Imani writes `const root = math.sqrt(81)` and expects it to fold. The native
program can call `core.math`, but the comptime interpreter cannot execute arbitrary Core
module calls yet.

**In the wild:**

```jet
use core.math as math

const tile = math.sqrt(256)
```

**Other languages:**

```zig
const tile = std.math.sqrt(256.0); // comptime when inputs are comptime-known
```

```rust
const N: usize = 4 + 4; // only const-approved functions are callable in const contexts
```

**Tradeoffs:**

| Option | Power | Determinism risk | Maintenance |
|--------|-------|------------------|-------------|
| A — execute all Core calls | highest | high | high |
| **B — curated pure whitelist (rec)** | useful | low | medium |
| C — no Core calls at comptime | low | low | low |

- **Option A — all Core calls.** Maximum power, but drags IO, platform, allocation, and
  runtime behavior into the interpreter boundary.

```jet
const data = fs.read("x")   // bad direction: compile-time effects by accident
```

- **Option B — curated pure whitelist (recommended).** Only deterministic, pure Core
  functions are callable at comptime; other calls produce a teaching diagnostic.

```jet
const tile = math.sqrt(256)
const lines = "a\nb".lines()
```

- **Option C — no Core calls.** Keeps comptime tiny, but forces duplicate evaluator code.

```jet
const tile = 16   // user must precompute or write local helpers
```

**Recommendation:** **B**. It gives users the expected pure math/string helpers while
preserving the compile-time effect boundary. The whitelist can grow with tests.

**Owner Q (2026-06-22) — I don't understand the downside. Why not just allow all Core
calls?**

The downside is that "all Core at comptime" changes comptime from **pure evaluation** into
**running a second copy of the runtime during compilation**. That creates five concrete
problems:

1. **Builds stop being reproducible by default.** `fs.read`, `env.get`, `time.now`,
   `random`, `process.run`, networking, and current-directory access can produce different
   outputs on two machines or two minutes apart. If those are allowed in arbitrary comptime
   code, the same source can compile into different binaries without the source changing.
2. **Compilation gains ambient side effects.** A package could run a process, touch the
   network, read secrets from the environment, or depend on local files just because an
   imported module has a `const` initializer. That fights Jet's supply-chain and package
   trust story unless every compile is sandboxed and audited.
3. **The interpreter must faithfully duplicate the whole runtime.** Core functions today are
   Rust-backed runtime helpers. Letting all of them run at comptime means reimplementing or
   safely bridging file handles, sockets, TLS, subprocesses, clocks, randomness, readers,
   writers, paths, JSON, channels, allocation behavior, and platform errors inside the
   comptime interpreter. Any mismatch becomes "works at runtime, fails at comptime" or worse,
   silently different behavior.
4. **Security policy becomes backwards.** The safe default should be "comptime is pure unless
   you explicitly opt into build-time IO." Option A makes IO/network/process execution the
   default capability of the compiler itself. That should be an expert, audited build recipe
   tier, not something reachable through a normal `const`.
5. **Error messages get worse.** With a whitelist, an unsupported call gets a clean teaching
   error: "`fs.read` is build-time IO; use `@embed`/package recipe/etc." With all-Core, every
   platform/runtime failure becomes a compile failure that depends on the user's machine.

So B is not "less power forever." It is sequencing:

```jet
const n = math.sqrt(81)          // allowed: deterministic pure whitelist
const parts = "a,b".split(",")   // allowed once String helpers are whitelisted
const home = env.get("HOME")     // rejected: ambient machine state
const data = fs.read("x.json")   // rejected here; use explicit build-time IO / embed path
```

The expert path is still available: add explicit build-time IO APIs later (`@embed`,
package recipes, sandboxed build steps, or a named `#BuildIO`/recipe capability). The line is
that **plain comptime stays deterministic and pure**, while build-time effects are explicit,
auditable, and package-policy-visible.

---

## Deferred ballots — promote when reached

The items below are not ready for owner decision. Each has a real user story
and a clear reason to wait. Promote a stub to a full card when its
prerequisite is ratified or its milestone is reached.

---

**D-PUBLISH1 — `jet publish` command shape + semver/resolver policy (board card c96).**
*User story:* Saoirse cuts a release of her Jet library and Amara pins a semver range to it.
*Decision (when promoted):* the `jet publish` command surface, version-immutability /
re-publish-refusal policy, and the resolver default (highest-compatible vs exact pins +
explicit update; lockfile default). *Why deferred:* rides **c50** (build-from-source) and
**c56** (registry upload) infra, both unverified/soft-blocked on dep approvals. Promote to a
full card with worked `jet publish` shell examples once M12.2 infra is verified.
Rec direction: `jet publish` infers version from `pkg.jet`, refuses re-publish + a dirty
tree, resolver defaults to highest-compatible with a committed lockfile. From the 2026-06-20
persona run (Saoirse, Amara).

---

**D-PROP1 — Effect prohibitions: implicit propagation of `#(no_…)`.**
*User story:* A security engineer wants to know, by reading the root call
site, that a call graph never touches the network — without auditing every
callee. He writes `#(no_net)` on a function and the compiler traces every
reachable call for a net effect, naming the violating path.
*Why deferred:* Rides **D-EFF1** (the effect-propagation engine itself) plus
D-QUAL1's surface (`#(…)`); prohibition is the inverse-lattice follow-on once
positive effects propagate. Sequencing: D-EFF1 → D-PROP1. Board items #24/#4.

---

**D-ROLE1 — Time-varying roles: typestate + time.**
*User story:* A hotel booking system dev wants to express that a `Reservation`
is `#pending` before payment and `#confirmed` after — and that calling
`check_in` on a `#pending` reservation is a compile error.
*Why deferred:* Requires the typestate machinery from **D-STATE1** (gated on
D-QUAL2) to be ratified first; "time-varying" adds a temporal ordering
constraint on top of static typestate, a separate design question. Board item #13.

---

**D-REFINE1 — Refinement types.**
*User story:* A numeric processing library author wants `PositiveInt` to be a
type the compiler can prove is always > 0, so she doesn't pepper every
function with `require(n > 0)`.
*Why deferred:* Refinement types require a proof/SMT layer that is not in the
roadmap for v1; the simplicity ratchet (I8) requires a concrete milestone slot
and owner sign-off before any work begins. Board item #19.

---

**D-BUDGET1 — Budgets as types.**
*User story:* A systems developer writing a real-time renderer wants to express
that `render_frame` has a 16ms CPU budget and have the compiler warn if a
called function is known to exceed it.
*Why deferred:* Requires comptime cost-bound inference, which is not in the
v1 roadmap; no prior-art consensus on how to make it ergonomic without macros
(I8 / no macros). Board item #22.

---

**D-IFC1 — Information-flow and compliance tracking.**
*User story:* A fintech dev wants to annotate a value as `#pii` (personally
identifiable information) and have the compiler refuse to let it flow into a
logging call or a non-encrypted storage write without an explicit sanitize
step — enforced at compile time, not by code review.
*Why deferred:* This is **D-TAINT1 Option B** (full information-flow control —
security-label lattice, principals, `declassify`), which the **owner explicitly
deferred to post-Epoch-3 on 2026-06-21** when ratifying D-TAINT1 Option A
(`#tainted` + sanitizers). Captured here so it is not lost. Generalizes D-TAINT1
and requires the full effect/tag propagation model from D-EFF1 and D-QUAL1 to be
ratified first; the compliance dimension (what counts as a legal sink) is a policy
question that also interacts with the manifest capability model (D-QUAL1 Option A,
manifest surface). Board items #30/#33.

---

**D-REPLAY1 — Opt-in record and replay.**
*User story:* A game developer wants to record a session's inputs, replay
them deterministically to reproduce a bug, and have the compiler ensure no
hidden state (system clock, random, I/O) is read during replay without being
mocked.
*Why deferred:* Requires the effect system (D-EFF1) to tag non-deterministic
effects and a runtime record/replay harness; neither is in the v1 roadmap.
Board item #7.

---

**D-REVERSE1 — Opt-in reversible computation and solver integration.**
*User story:* A constraint-based UI layout author wants to write the forward
constraint (`width = parent.width - padding * 2`) and have Jet automatically
solve for `padding` given a target `width` — without writing the inverse by
hand.
*Why deferred:* Requires a reversibility annotation on functions and a
solver/SMT backend; no prior-art consensus on making this ergonomic without
macros or dependent types. Board item #36.

---

**D-PROTO1 — Protocol and session type generation.**
*User story:* A network protocol implementer wants to declare a
request/response handshake sequence as a type and have the compiler generate
both the client and server stubs, rejecting code that sends messages out of
order.
*Why deferred:* Session types require linear types (used exactly once, in
order) and typestate; **D-LIN1** (linear tag) and **D-STATE1** (typestate),
both gated on D-QUAL2, are prerequisites, and the code-generation surface for
protocol stubs is a separate design. Board item #9.

---

**D-VERIFY1 — Formal verification and proof integration.**
*User story:* A cryptography library author wants to attach a machine-checked
proof that her `constant_time_eq` function runs in time independent of its
inputs, and have the Jet toolchain refuse to ship the library if the proof
doesn't hold.
*Why deferred:* Requires a proof-carrying-code or SMT integration layer that
is explicitly post-v1; the simplicity ratchet (I8) bars this without a
concrete roadmap slot and owner sign-off. Board items #15/#17.

---

## B6 `defer` — already decided, no ballot

`defer` is solved; nothing to vote on. **D-DEFER1 (ratified + implemented 2026-06-20)** shipped `core.scope.guard(() => {…})` — a stdlib value whose `Drop` runs the stored lambda LIFO on every exit path including `?`. `defer`-as-primary stays rejected (S63); the `defer` keyword stays declined (D-SUGAR5).

```jet
use core.scope

fn copy_file(src: String, dst: String) -> () ? Error {
    f :: core.fs.open(src)?
    g1 :: scope.guard(() => { core.fs.close(f) })   // replaces `defer close(f)`
    g :: core.fs.create(dst)?
    g2 :: scope.guard(() => { core.fs.close(g) })   // fires before g1, even on early return
    core.fs.copy(f, g)?
}
```

**Reopen (owner-only):** you could later add `defer expr` as sugar over `scope.guard` (same Drop-backed lowering, zero runtime cost). For: it's the spelling Jai/Go/Swift/Odin/Zig converge on. Against: D-SUGAR5 declined it; it adds a second cleanup spelling and reintroduces Go's leak-by-omission class. No agent reopens this without your instruction.

---

## Three-mode execution & JIT dev runtime — board card c77

### D-JIT1 — JIT backend (rec D)

**User story.** Sam runs `jet serve api.jet` for a 40-endpoint service and edits a
handler. He wants the change live in well under a second, with the new code running at
something close to native speed — and he never wants to see a Rust or LLVM error,
because Jet promised him the front end owns every message. The question is what
machine actually turns his saved handler into running code inside the live process.

| Option | Latency to live | Peak throughput | New compiler dep (I6) | I2/I3 risk | Ratification cost |
|--------|-----------------|-----------------|------------------------|-----------|-------------------|
| A Cranelift JIT | very low (ms) | good, below LLVM | yes — Cranelift in the runtime crate | low (sema gates before emit) | high (new backend) |
| B incremental rustc | high (rustc per swap) | best (LLVM) | no new dep, but rustc in the hot loop | **high** — rustc errors in the live path | medium |
| C hybrid (Cranelift dev, rustc release) | very low dev / best release | best release | yes — both | medium — two backends to keep consistent | highest |
| D stay-interpreter-for-v1 | low (no compile) | interpreter-speed | **none** | lowest | lowest |

**How other languages do this.**
- **Cranelift JIT** (wasmtime, rustc's `-Zcodegen-backend=cranelift`): fast machine-code
  emit, designed for low-latency compile, weaker optimizer than LLVM. *Jet takeaway:*
  the natural "fast swap, decent speed" backend, but it's an external crate in the
  runtime — needs an I6 stance even though it's runtime-side, not in the `Source/`
  compiler.
- **JVM HotSpot / tiered JIT**: interpret first, JIT the hot methods on a background
  thread. *Jet takeaway:* tiering (interpret cold, JIT hot — plan item 4b) is the right
  shape regardless of which backend wins; v1 can be tier-0 only.
- **incremental rustc**: real LLVM codegen but seconds-scale per change, and the tool
  doing the compile is the one Jet has sworn never lets speak to users (I2). *Jet
  takeaway:* fine as the *release* backend (already shipped as `jet build`), wrong as
  the *interactive* backend.
- **Cranelift-as-rustc-backend**: rustc itself can emit via Cranelift for faster debug
  builds. *Jet takeaway:* shows the hybrid is real prior art, not a fantasy — but it's
  two backends to keep output-identical (see D-DEVMODE1 / 4e).

- **Option A — Cranelift JIT in the live process.** sema fully checks the unit, then a
  Cranelift backend emits native code in-process. rustc stays the release backend only.

```shell
$ jet serve api.jet
serving on :8080 (JIT: cranelift, tier-0)
# edit handlers/checkout.jet, save
[checkout.jet] checked ok → JIT-compiled in 31ms → swapped live
```

- **Option B — incremental rustc per swap.** Reuse the shipped rustc path for every
  reload. One backend, but rustc runs in the interactive loop.

```shell
$ jet serve api.jet
# edit + save
[checkout.jet] checked ok → rustc rebuild… 4.2s → swapped live
# and if rustc ever rejected the generated crate, that is an I2 ICE, never Sam's error
```

- **Option C — hybrid: Cranelift for dev/serve, rustc for release.** Fast swap in the
  live process, LLVM-grade binary on `jet build`. Pays for two backends and must prove
  they agree (4e).

```shell
$ jet serve api.jet      # cranelift, ms-scale swaps
$ jet build api.jet      # rustc/LLVM, optimized binary
# CI runs every example through both and diffs (D-DEVMODE1 / 4e)
```

- **Option D — stay interpreter for v1, design the JIT seam now.** `jet serve` ships
  on the comptime interpreter (D-DEV3) with hot-swap; the JIT backend lands behind a
  stable seam in a later Epoch-3 milestone. Zero new deps, lowest risk, ships the
  *experience* (live hot-reload server) before the *speed*.

```shell
$ jet serve api.jet
serving on :8080 (interpreter; JIT backend: planned)
# edit + save → re-checked + hot-swapped at interpreter speed, sub-200ms
```

**Recommendation: D.** Ship the hot-reload *experience* on the already-proven
interpreter first — it's the part users feel — and keep the JIT behind a seam so the
backend choice (A vs C) can be made on real workloads without blocking the pillar.
Cranelift (A) is the likely successor; rustc-in-the-loop (B) is rejected outright as an
I2 hazard.

---

**Owner directive (2026-06-21): "Option D, but don't defer the full implementation if
avoidable — we're close to Epoch 3. Propose a new approach to get it working."**

Here it is — **Option D+ : seam now, Cranelift JIT *this* Epoch-3, tiered (not deferred).**
D and A were framed as "ship the experience now / pick the backend later." D+ collapses
that into one Epoch-3 deliverable so the speed lands with the experience:

- **Phase 1 (now, days):** `jet serve` ships on the interpreter with hot-swap — behind a
  stable `JitBackend` trait seam (one Rust trait: `compile_unit(checked_ir) -> CodePtr`).
  The interpreter is the first impl. Users get live reload immediately (D's win).
- **Phase 2 (same Epoch-3 milestone — named, not "a later milestone"):** implement a
  **Cranelift** `JitBackend` behind that seam and flip `jet serve` to it. The interpreter
  stays as **tier-0** (cold / not-yet-JITed / unsupported-op fallback); Cranelift is
  **tier-1** for hot code — the JVM-style tiering the plan already wanted (4b). So nothing
  built in Phase 1 is thrown away; the interpreter is permanent tier-0, not scaffolding.

```shell
$ jet serve api.jet
serving on :8080 (tier-0 interpreter; tier-1 cranelift: warming)
# edit handlers/checkout.jet, save
[checkout.jet] checked ok → interpreted live in 8ms        # instant (tier-0)
[checkout.jet] hot path → cranelift-compiled in 31ms        # upgraded to native (tier-1)
```

**Why this is safe and I-compliant (the whole reason D was cautious):**
- **I6 holds.** Cranelift is a **runtime-side** crate in the `jet serve` runtime, **not in
  `Source/`** (the compiler). I6 forbids crates in the *compiler*; the runtime already
  links rustc for `jet build`. This needs your **dep approval** — the same kind you gave the
  `regex` crate (D-REGEX1) — for a runtime-only Cranelift dependency. That approval is the
  one decision that unblocks Phase 2; everything else is engineering.
- **I2/I3 hold.** sema fully checks every unit *before* any emit; the JIT only ever lowers
  already-valid IR (codegen stays dumb, I3). If Cranelift ever rejects valid IR that's an
  internal ICE, never Sam's error (I2) — identical to the rustc contract.
- **B stays rejected** (rustc in the interactive loop = seconds + I2 hazard). C (two
  release backends kept output-identical) is **not** needed: release stays rustc/LLVM via
  `jet build`; `jet serve` is dev-only, so dev-vs-release output-identity is a test concern,
  not a correctness gate — Cranelift dev + rustc release is fine because the release artifact
  is always the rustc one.

**What I need from you:** approve the **runtime-side Cranelift dependency** (I6 runtime
exception, like regex/rustc). With that yes, D+ ships the hot-reload experience now AND the
real JIT inside Epoch 3 — no open-ended defer. If you'd rather not take the dep at all,
fall back to plain D (interpreter-only, JIT genuinely later). **Recommend D+.**

---

### D-HOTSWAP1 — hot-reload semantics (rec: module boundary + type-stable state preservation)

**User story.** Priya's `jet serve` process holds an in-memory session cache and 200
open websocket connections. She fixes a typo in one handler and saves. She expects the
fix live with the cache and the sockets intact. Next she changes the *shape* of the
session record. She does **not** expect the server to keep reinterpreting old bytes as
the new type — that's exactly the memory-unsafety Jet exists to prevent. She'd rather
be told "this change needs a restart" and have it happen cleanly.

Two coupled questions: **(Q1) swap boundary** — what unit gets replaced; **(Q2) state
policy** — what happens to live state across the swap.

| Option | Swap unit | State on type-compatible edit | State on type-changing edit | Safety story |
|--------|-----------|-------------------------------|------------------------------|--------------|
| A function | single fn | preserved | n/a (fn body only) | tight blast radius; can't swap a type at all |
| B module | module | **preserved** (code swapped, data kept) | **announced clean restart** | matches Erlang; clear safety line |
| C whole-program | process | always restart | always restart | trivially safe, loses the live-state win |

- **Option A — function-granularity swap, state untouched.** Only function bodies hot-
  swap; any signature/type/struct change forces a restart. Smallest blast radius,
  simplest invalidation — but most real edits touch more than one body.

```jet
# edit the body only → swapped in place, all state preserved
fn price(c: Cart) -> Money {
    c.lines.sum((l) => l.qty * l.unit) - c.discount?   # was: ... (no discount)
}
```

- **Option B — module-granularity swap with type-stable state preservation.** The
  reload unit is a module. If the module's **public type surface is unchanged**, swap
  the code and **keep the module's live state** (the session cache, the sockets). If a
  reload **changes a type/layout** that live state depends on, Jet does **not**
  reinterpret old data — it performs a **clean, announced restart** of that module (or
  the process, if the change crosses module walls), draining connections first.

```jet
module sessions {
    cache :: SessionCache.new()   # live state

    fn touch(id: SessionId) { cache.bump(id) }   # type-stable edit → hot-swap, cache kept
}
```

```shell
# type-stable edit:
[sessions] checked ok → hot-swapped; module state preserved
# type-changing edit (Session gains a field):
[sessions] type surface changed (Session: +field `region`)
  → live state of `sessions` is no longer well-typed; announced restart in 2s
  → draining 200 connections… restarted clean
```

- **Option C — whole-program swap, always restart.** Every save restarts the process.
  Trivially safe, zero stale-state risk — but throws away the live-cache/live-socket
  benefit that justifies a long-lived JIT process at all.

```shell
[any change] → full restart (state not preserved)
```

**Recommendation: B (module boundary + type-stable preservation).** This is the
Erlang/Elixir gold-standard model adapted to a no-GC, statically-typed setting: keep
state across code-only swaps, refuse to reinterpret state across type changes, and make
the restart *announced and clean* rather than silent. The type-surface check is a sema
job (I3) — the runtime never guesses. Function-only (A) is too coarse a win for too
many restarts; whole-program (C) defeats the pillar's purpose.

---

### D-DEVMODE1 — hot-reload home + dev↔release consistency guarantee (rec: B for home, ratify the guarantee)

**User story.** Theo just wants "edit, see it instantly." He already knows `jet dev`
(the shipped watch-and-rerun loop) and `jet run`/`jet build`. The open question isn't
the verbs — D-DEV4 settled those — it's whether *instant hot-reload* is `jet dev`
growing a hot-swap upgrade, or a separate `jet serve` for long-lived processes. And
separately: Theo must be able to trust that what he sees in dev is exactly what ships.

**Q1 — where does hot-reload live?** **Q2 — ratify the consistency guarantee (4e) as a
hard rule?** (Verb naming is NOT on this ballot.)

| Option (Q1) | Home of hot-swap | Mental model | Cost |
|-------------|------------------|--------------|------|
| A | extend `jet dev` | "dev got faster — same verb, now swaps instead of reruns" | one verb does both short scripts and long servers |
| B | new `jet serve` (D-DEV2) | "`dev` = rerun my script; `serve` = long-lived process I hot-swap into" | two verbs, but each matches its prior art |

- **Option A — hot-reload is an upgrade to the shipped `jet dev` loop.** The watch loop
  (4a, already shipped) keeps its verb; in Epoch 3 it gains hot-swap instead of full
  re-run. One verb for all reload.

```shell
$ jet dev script.jet     # short script: watch + rerun (today) → watch + hot-swap (E3)
$ jet dev api.jet        # also the long-lived server? one verb, two lifetimes
```

- **Option B — `jet dev` stays the rerun loop; `jet serve` is the long-lived hot-swap
  process.** `jet dev` keeps the shipped semantics (re-run my entry on save). The
  long-lived JIT/hot-swap process is `jet serve` (the D-DEV2 surface). The watcher and
  debounce (4a) are shared machinery; the difference is rerun-vs-swap-into-a-living-
  process.

```shell
$ jet dev script.jet     # unchanged shipped behavior: re-run entry on save
$ jet serve api.jet      # long-lived; modules hot-swapped in place (D-HOTSWAP1)
```

**How other languages do this.**
- **Bun `--watch` / Vite dev (HMR)**: `--watch` restarts the process; Vite HMR swaps
  modules into a *running* app — and they are deliberately *different* tools/flags.
  *Jet takeaway:* the ecosystem itself separates "rerun" from "swap into a live app" —
  supports B's two-verb split.
- **nodemon**: pure restart-on-change, no state preservation. *Jet takeaway:* that's
  today's `jet dev` exactly; the hot-swap upgrade is a genuinely different capability,
  worth its own home.
- **Erlang/Elixir hot code swapping**: the gold standard — versioned modules swapped
  into a running node with state carried via `code_change`. *Jet takeaway:* this is a
  `serve`-shaped, long-lived-process feature, not a "re-run my script" feature.
- **JVM HotSwap / JRebel**: HotSwap allows method-body changes only; JRebel extends to
  structural changes. *Jet takeaway:* mirrors D-HOTSWAP1's "type-stable swap vs
  restart" line; reinforces that hot-swap belongs to a persistent process.
- **Wasm component reload**: swap a component instance, explicitly hand off state across
  the boundary. *Jet takeaway:* state hand-off is an explicit, typed operation, never
  an implicit byte-reinterpret — exactly the I1 line D-HOTSWAP1 draws.

**Q2 — the consistency guarantee (4e).** Regardless of Q1: a program must behave
**identically** under the dev runtime (interpreter / JIT) and the release build (rustc
binary). Ratify as a **hard rule**: a `tests/` mode runs every golden example through
**both** paths and **diffs output**; any mismatch is a **release blocker**. This is the
I5 guard across two backends and the standard JIT/AOT-divergence defense.

```shell
$ jet test --consistency
running 142 examples through {interpreter, rustc-release}…
  141 identical
  ✗ 03_floats.jet: dev=0.30000000000000004  release=0.3
  → RELEASE BLOCKED: dev/release output diverged (4e)
```

**Recommendation: B for Q1, and ratify Q2 as a hard rule.** Keep the shipped `jet dev`
rerun loop exactly as users learned it; give long-lived hot-swap its own `jet serve`
verb (already the D-DEV2 surface) — matching how Bun/Vite/Erlang separate the two
lifetimes. And make the dev↔release diff a release blocker, not a warning, so Jet never
ships the "works in dev, breaks in prod" bug class.

---

## Cache-friendly layout (SOA, deferred) — board card c78

### D-SOA2 — Naming the cache-friendly layout + remaining layout questions (rec: `columnar`)

**User story.** Maya is writing a particle system for a real-time renderer. She has 50 000 `Particle` structs, and her hot loop reads only `x`, `y`, `z`, and `alive` each frame. She annotates the struct once with a layout attribute, and the compiler arranges memory so those fields land in contiguous arrays — SIMD and prefetch work without her restructuring anything. She doesn't care what "SOA" stands for; she wants a word that makes the annotation self-explanatory the first time a teammate reads it.

> **Context.** D-SOA1 ratified Option A — `#layout(soa) struct …`, whole-struct, field access `p.x` unchanged, layout part of the type. Syntax is locked; implementation is deferred post-v1. This card decides the remaining four questions the owner asked be carded: the **name**, partial vs whole, the reserved per-container spelling, and the serialization interaction.

---

#### Sub-question 1 — What should `soa` be called inside `#layout(…)`?

The word `soa` is an acronym for "structure-of-arrays," a data-oriented-design term opaque to most beginners. Nine candidates, each shown in context.

```jet
#layout(soa)        struct Particle { x: Float, y: Float, z: Float, alive: Bool }  // baseline (acronym)
#layout(lane)       struct Particle { x: Float, y: Float, z: Float, alive: Bool }  // each field is a parallel lane
#layout(columnar)   struct Particle { x: Float, y: Float, z: Float, alive: Bool }  // each field becomes a column
#layout(striped)    struct Particle { x: Float, y: Float, z: Float, alive: Bool }  // fields split into parallel stripes
#layout(slipstream) struct Particle { x: Float, y: Float, z: Float, alive: Bool }  // aviation; parallel streams
#layout(parallel)   struct Particle { x: Float, y: Float, z: Float, alive: Bool }  // arrays run parallel
#layout(split)      struct Particle { x: Float, y: Float, z: Float, alive: Bool }  // struct split into per-field arrays
#layout(fieldwise)  struct Particle { x: Float, y: Float, z: Float, alive: Bool }  // arranged field-by-field
#layout(wingspan)   struct Particle { x: Float, y: Float, z: Float, alive: Bool }  // aviation; wide flat spread
```

| Name | What it evokes | Beginner clarity | Collision / keyword risk | Prior-art familiarity |
|---|---|---|---|---|
| `soa` | DOD acronym | low — opaque initialism | none | high (C++/DOD) |
| `lane` | parallel channel, SIMD lane | medium | low | low-medium (SIMD docs) |
| `columnar` | database column store | high — maps to the memory picture | none | high (Arrow, Parquet) |
| `striped` | RAID striping | medium | none | medium |
| `slipstream` | aviation, parallel streams | medium-low — metaphorical | none | low |
| `parallel` | parallel arrays | high | medium — overloaded w/ concurrency | very high |
| `split` | the struct split apart | medium — implies destruction | none | low |
| `fieldwise` | field-by-field | high | none | low |
| `wingspan` | aviation spread | low — purely metaphorical | none | none |

**Recommendation:** `columnar` — self-defining from the table picture, strong prior art (Arrow/Parquet/column stores), zero collision with concurrency vocabulary.

---

#### Sub-question 2 — Whole-struct only in v1, or also partial annotation?

- **Option A — whole-struct only (recommended).** Every field is columnar; one unambiguous rule.

```jet
#layout(columnar)
struct Particle { x: Float, y: Float, z: Float, alive: Bool, tag: U32 }  // all fields columnar
```

- **Option B — partial annotation.** Only the listed fields go columnar; the rest stay interleaved. Field access is still `p.x`, but the compiler now manages two memory regions.

```jet
#layout(columnar: x, y, z)
struct Particle { x: Float, y: Float, z: Float, alive: Bool, tag: U32 }  // x,y,z columnar; alive,tag interleaved
```

| | Whole-struct | Partial |
|---|---|---|
| Safety | one memory region per struct | two regions; aliasing rules must extend |
| Beginner clarity | one path: annotate, done | must know which fields are "hot" at annotation time |
| Long-term correctness | layout fully captured by one attribute | partial layouts compose poorly with generics/trait bounds |

**Recommendation:** whole-struct only in v1 — partial layout is a real case but needs new ownership/aliasing surface; defer to a future ballot on evidence.

---

#### Sub-question 3 — Reserve the per-container spelling for the future?

D-SOA1 chose whole-struct (Option A) over per-container (`particles: columnar [Particle]`, old Option B). Should the per-container spelling be grammar-reserved so a future ballot can add per-container overrides without a syntax conflict?

- **Option A — reserve (recommended).** The grammar reserves `columnar [T]` in type position; no surface today.

```jet
#layout(columnar) struct Particle { x: Float, y: Float, z: Float, alive: Bool }
particles: [Particle] := []            // layout follows the struct annotation
// future (if later ratified): particles: columnar [Particle] := []   // per-use override
```

- **Option B — do not reserve.** Only the `#layout(…)` form exists; any future per-container form designs its own spelling.

```jet
#layout(columnar) struct Particle { x: Float, y: Float, z: Float, alive: Bool }
particles: [Particle] := []            // the only layout surface, permanently
```

| | Reserve | Do not reserve |
|---|---|---|
| Future flexibility | per-container override possible later, no conflict | future form needs a fresh design |
| Beginner clarity | no visible difference today | one annotation surface |
| Long-term correctness | prevents accidental reuse of `columnar` as an unrelated qualifier | nothing to track |

**Recommendation:** reserve — a hot struct used in both layouts is a plausible future need; reserving costs nothing visible today and is a pure grammar note until a future ballot ratifies the form.

---

#### Sub-question 4 — Does `#layout(columnar)` affect the serialized representation?

- **Option A — layout-transparent (recommended).** Serialization sees the logical struct; output is identical with or without the layout attribute.

```jet
#layout(columnar)
#[Serialize]
struct Particle { x: Float, y: Float, z: Float, alive: Bool }
// JSON: { "x": 1.0, "y": 2.0, "z": 3.0, "alive": true } — unchanged by #layout
```

- **Option B — layout-visible.** The serializer exposes the columnar arrays; the serialized shape changes when the layout attribute is added or removed.

```jet
#layout(columnar)
#[Serialize]
struct Particle { x: Float, y: Float, z: Float, alive: Bool }
// JSON becomes { "x": [1.0, 2.0, …], "y": [...], … } — a refactor of #layout breaks the wire format
```

| | Layout-transparent | Layout-visible |
|---|---|---|
| Safety | wire contract stable; adding `#layout` never breaks a format | layout change silently alters serialized shape |
| Beginner clarity | `#layout` = memory, `#[Serialize]` = format; one model each | two annotations interact; reason about both at once |
| Long-term correctness | layout and serialization stay independent | couples two concerns that should evolve separately |

**Recommendation:** layout-transparent — `#layout(columnar)` is a memory concern only; a user wanting columnar serialization (e.g. Arrow IPC) reaches for a purpose-built serializer, not the default `#[Serialize]`.

## Testing ergonomics — board card c51

Source plan: `tools/Tower/docs/plans/epoch-3/testing-docs-ergonomics.md`.

The Epoch 2 test core shipped: `#test fn name { … }` blocks (S43/S82), `require`
/ `require_eq` assertions (S36), snapshot `expect(…).snapshot()` with
`--update-snapshots`, `todo` typed holes, and `jet bench`. These three items are
**ergonomics layer**, not core language, and each is gated on a syntax decision Jet
has not yet made.

**What is add-only (not re-deciding):**
- `#test fn name { … }` — the unit-test surface (S43, S82). Ratified. Untouched.
- `require` / `require_eq` — assertion builtins (S36). Ratified. Untouched.
- `///` doc-comment marker — the *existence* of `///` is ratified (S49): "summary
  lines immediately above items; plain text in v1; shown by hover/docs tooling."
  D-TEST4 decides only how code examples *inside* those comments are delimited and
  executed as tests. S49's `///` marker stays.

D-TEST1 gates property testing + shrinking. D-TEST4 gates doc-example execution.
Coverage (D-COV1 below) needs no syntax decision; it is noted as deferred.

---

### D-TEST1 — property-test surface + shrinking (rec B)

**User story.**

Mia writes a `reverse` function for lists. She wants to say "for any list of
integers, reversing twice returns the original" — and when her implementation is
wrong she wants the test runner to hand her the *smallest* list that breaks it, not
just the first random one it found. She has never heard of QuickCheck. She types
something that looks like a normal test and discovers property testing by accident.

**How other languages do this.**

| Language | Spelling | Shrinking |
|----------|----------|-----------|
| **Haskell QuickCheck** | `prop_reverse xs = reverse (reverse xs) == xs`; `quickCheck prop_reverse` — an ordinary function whose args are `Arbitrary` | Automatic (typeclass-driven); built into the library |
| **Python Hypothesis** | `@given(st.lists(st.integers())) def test_rev(xs): assert rev(rev(xs)) == xs` — decorator + strategy objects | Automatic shrinking; strategies carry shrinkers |
| **Rust proptest** | `proptest!(|(xs: Vec<i32>)| { assert_eq!(rev(rev(&xs)), xs); });` macro; strategy is the type | Automatic; macro-driven |
| **JavaScript fast-check** | `fc.assert(fc.property(fc.array(fc.integer()), xs => reverseReverse(xs)))` — imperative | Automatic; arbitraries carry shrinkers |

Jet takeaway from all four: the surface that feels lowest-ceremony is one where the
test annotation and a parameter list together communicate "generate inputs for me."
The shrinking behavior is automatic and invisible in every respected property-test
library; the user never writes a shrinker.

**Current state (add-only).**

S82 shows `#test fn reversing_twice(xs: [Int]) { require_eq(reverse(reverse(xs)), xs) }` as a worked example in the attribute-syntax ratification. This is the *aspirational* property-test surface hinted there; it is **not yet executable** — the plan explicitly states property testing is blocked on D-TEST1. That example establishes the Jet aesthetic the owner already prefers (looks like a normal test, parameter list carries the generated type), so options below are ordered by how closely they follow it.

**Tradeoff comparison.**

| Option | Surface | New keyword/sigil? | Shrinking surface | Ceremony |
|--------|---------|-------------------|-------------------|----------|
| A — `#property fn` attribute | `#property fn name(x: T) { … }` | new attribute `#property` | implicit, always on | low — one new marker |
| B — `#test fn` with parameters | `#test fn name(x: T) { … }` | none (extends existing `#test`) | implicit | zero — same marker, params signal property |
| C — `forall` expression inside test | `#test fn name { forall n: Int { … } }` | new keyword `forall` | implicit | medium — nested blocks |
| D — generator call as a parameter default | `#test fn name(x: Int = Gen.int()) { … }` | none | explicit shrinker optional | medium — call-site annotation noise |


- **Option A — `#property fn` attribute.** A distinct attribute signals "this is a
  property test, not a unit test." The runner generates inputs from the parameter
  types.

```jet
#property
fn reversing_twice(xs: [Int]) {
    require_eq(reverse(reverse(xs)), xs)
}

#property
fn addition_commutes(a: Int, b: Int) {
    require_eq(a + b, b + a)
}
```

Shrinking failure output (automatic — user never writes a shrinker):

```
FAIL property reversing_twice
  failed after 47 examples
  counterexample: xs = [3, 1]
  shrunk to:      xs = [1, 0]
  error: require_eq failed
    left:  [0, 1]
    right: [1, 0]
```

**Tradeoff:** two test markers (`#test` and `#property`) is two concepts to teach
and two words to remember. The distinction is real but adds cognitive overhead on
first contact.

- **Option B — `#test fn` with parameters (recommended).** An `#test fn` with
  parameters is a property test; one with no parameters is a unit test. The runner
  generates inputs from the parameter types. Zero new syntax; the parameter list
  already tells the reader something interesting is happening.

```jet
// unit test — no params, exactly as before
#test
fn empty_list_reverses_to_empty() {
    require_eq(reverse([]: [Int]), []: [Int])
}

// property test — params present → generate and shrink
#test
fn reversing_twice(xs: [Int]) {
    require_eq(reverse(reverse(xs)), xs)
}

#test
fn addition_commutes(a: Int, b: Int) {
    require_eq(a + b, b + a)
}
```

Same failure output as Option A (automatic shrinking; zero user effort). The rule
is a single sentence: "a test function with parameters is a property test."

**Tradeoff:** slightly less obvious from the marker alone that a function is a
property test. The parameter list is the signal, not the attribute. For experts
wanting to pin generated ranges or enumerate cases, a future `#[test, cases(…)]`
multi-marker form (S82) is compatible without breaking this surface.

- **Option C — `forall` expression inside test.** A `forall` keyword inside a test
  block introduces generated variables. Closer to mathematical notation.

```jet
#test
fn prop_reverse_is_involution() {
    forall xs: [Int] {
        require_eq(reverse(reverse(xs)), xs)
    }
}

#test
fn prop_commutes() {
    forall a: Int, b: Int {
        require_eq(a + b, b + a)
    }
}
```

**Tradeoff:** `forall` is a new keyword (I7 demands a slot in `Source/Syntax.rs`
and a decision ID). The nested block adds indentation. The mathematical flavour
may feel out-of-place next to Jet's plain-English style. Benefit: visually
unambiguous — property tests look different from unit tests inside the body.

- **Option D — generator call as parameter default.** Users annotate parameters
  with explicit generator calls; the runner recognizes parameters with generator
  defaults.

```jet
#test
fn prop_reverse(xs: [Int] = Gen.list(Gen.int())) {
    require_eq(reverse(reverse(xs)), xs)
}

// with range constraint:
#test
fn prop_bounded(n: Int = Gen.int(0..100)) {
    require(n >= 0 && n <= 100)
}
```

**Tradeoff:** explicit generators are more powerful (users can constrain ranges
day one) but more verbose. `Gen.int()` is a stdlib API call, not syntax — the
line between library and language blurs. Property tests look like ordinary tests
with defaults, which may cause confusion about which functions actually run
generators.

**Recommendation:** B. It adds zero syntax: an `#test fn` with parameters is a
property test; without parameters it is a unit test. This matches the S82 worked
example the owner already ratified, removes a cognitive split between two
attributes, and follows the simplicity ratchet (I8). Shrinking is always automatic
and invisible. The rule teaches in one sentence. If the owner wants an explicit
generator-constraint story, that is a follow-on decision layered on top of B
without breaking it (e.g. `#test fn prop(n: Int) where n in 0..100 { … }` or a
future `#[test, config(runs: 500)]` multi-marker).

---

### D-TEST4 — doctest convention (rec A)

**User story.**

Lena writes a `parse_int` function and adds a `///` doc comment explaining what it
does (S49). She wants the code example in that comment to run as part of `jet test`
so it never goes stale. She does not know the word "doctest" — she just wants her
example to be checked.

**How other languages do this.**

| Language | Doc marker | Example delimiter | Expected-output convention | Jet takeaway |
|----------|-----------|-------------------|---------------------------|--------------|
| **Rust** | `///` or `//!` | fenced ` ```rust ``` ` or bare ` ``` ` inside doc comment | `// ` comment after expression is *not* checked; `assert_eq!` used instead | Jet can't require `assert_eq!` calls (that's two concepts); expected output must be a simpler convention |
| **Python doctest** | triple-quoted docstring | `>>>` prompt prefix; expected output on the next line(s) | Visually distinct from prose; REPL-style | The `>>>` prompt is universally understood but adds a new sigil |
| **Elixir ExUnit** | `#doc """…"""` | fenced ` ```elixir ``` ` block; `iex>` prompt | `iex>` lines run as doctests; return value on next line is checked | Prompt-style is readable inline; `iex>` is language-specific branding |
| **Julia** | `"""…"""` docstring | ` ```jldoctest ``` ` block with `julia>` prompt | Checked against output | Language-specific fenced language tag works but requires tooling recognition |

Jet takeaway: fenced code blocks inside `///` comments are the cross-language
convention; the question is whether the expected output is a trailing comment, a
following plain line, or embedded via a prompt style. Jet has no REPL yet (c55
deferred), so a prompt style implies a surface that doesn't exist. A trailing
comment convention (`// => value`) is lightweight and already reads naturally in
Jet comments.

**Current state (add-only).**

S49 ratifies `///` as the doc-comment marker and defers example running to M13. E2901
("doctest output mismatch") is reserved in `diagnostics.md`. This decision picks the
delimiter and expected-output convention so D-TEST4 can be implemented.

**Tradeoff comparison.**

| Option | Example delimiter | Expected output | New syntax? | Reads naturally? |
|--------|------------------|-----------------|-------------|-----------------|
| A — fenced ` ```jet ``` ` + `// =>` trailing comment | ` ```jet…``` ` block inside `///` | `// => value` comment on the last expression line | none — reuses `//` comment (S5) and fenced block convention | yes — comment is Jet syntax |
| B — fenced ` ```jet ``` ` + plain following line | ` ```jet…``` ` block; output as a bare second block or following plain text | prose line after the code block | none | ambiguous — prose vs expected output |
| C — `>>>` prompt prefix | `/// >>> parse_int("42")` with `/// 42` on the next line | plain line after `>>>` line | new inline prompt convention | familiar to Python users; unfamiliar to others |
| D — `#doctest` attribute on function | separate attribute triggers example extraction | no in-comment convention | new `#doctest` attribute | separates docs and tests; poor discoverability |


- **Option A — fenced ` ```jet ``` ` block + `// =>` trailing comment (recommended).**
  Examples are delimited by a standard fenced code block inside `///` lines. The
  expected output is a `// =>` comment on the line where a value is produced. The
  runner extracts the block, compiles it, and checks the printed/returned value
  against the `// =>` annotation.

```jet
/// Parse a decimal integer from a string.
///
/// Returns an error if the string contains non-digit characters.
///
/// ```jet
/// parse_int("42")  // => 42
/// parse_int("-7")  // => -7
/// parse_int("hi")  // => err(ParseError { … })
/// ```
pub fn parse_int(s: String) -> Int ? ParseError {
    …
}
```

A mismatch fires E2901:

```
error[E2901]: doctest output mismatch
  --> src/math.jet:6
   |
 6 |   parse_int("42")  // => 99
   |                         ^^
   |   expected: 99
   |   actual:   42
   |   note: update the `// =>` comment to match, or fix the implementation
```

Multiple statements, no expected output for intermediate lines:

```jet
/// ```jet
/// x :: parse_int("10")?
/// y :: parse_int("20")?
/// x + y  // => 30
/// ```
```

**Tradeoff:** the `// =>` convention adds no new tokens (S5 ratified `//` as the
line-comment marker); the runner just looks for that specific comment prefix on the
last expression of a block. The fenced block is the universal doc-example
convention. Downside: the `// =>` idiom is not self-describing on first encounter
(though it reads naturally: "this produces 42").

- **Option B — fenced ` ```jet ``` ` + separate plain-text output block.** A second
  fenced block (or a plain indented block) after the code block holds expected
  output. Rust's standard approach for prose output (not for expression values).

```jet
/// ```jet
/// print(parse_int("42"))
/// ```
///
/// Output:
///
/// ```
/// 42
/// ```
```

**Tradeoff:** two blocks per example doubles the visual weight. The separator label
("Output:") is prose that the runner must parse. Works well when expected output is
multi-line `print` output; awkward for simple expression values.

- **Option C — `>>>` prompt prefix.** REPL-style inline convention, each line
  prefixed by `>>>` inside `///` comments.

```jet
/// >>> parse_int("42")
/// 42
/// >>> parse_int("hi")
/// err(ParseError { … })
pub fn parse_int(s: String) -> Int ? ParseError { … }
```

**Tradeoff:** `>>>` is a new inline convention inside `///` comments — not a token
the lexer sees (it lives in comment text), but a convention the doctest runner must
parse. Familiar to Python users. Jet has no interactive REPL today (c55 deferred),
so `>>>` implies a mode that doesn't exist. The prompt may confuse beginners who
try to type `>>>` at a terminal.

- **Option D — `#doctest` attribute on the function, examples in a separate file.**
  A marker attribute on the function points the runner at examples stored elsewhere.

```jet
#doctest("examples/parse_int.jet")
pub fn parse_int(s: String) -> Int ? ParseError { … }
```

**Tradeoff:** discoverability is poor — the example lives in a different file from
the doc comment. Breaks the "docs and examples colocate" ergonomic goal. Not
recommended.

**Recommendation:** A. The `// =>` trailing-comment convention reuses existing
comment syntax (S5), requires zero new tokens, and reads naturally in Jet — the
`// =>` prefix is already idiomatically used in prose code snippets to show "this
evaluates to." The fenced ` ```jet ``` ` delimiter matches how examples already
appear in this codebase's docs. The diagnostic E2901 slots into the reserved
position cleanly.

---

## Coverage — D-COV1 (deferred, no ballot needed)

The epoch-3 plan scopes coverage as "tooling only — no new syntax; couples to the
test runner in `Source/main.rs` (`run_test`)." There is no user-facing surface
decision: `jet test --coverage` is the spelled-out verb and the output format (LCOV
/ HTML / stdout summary) is an implementation choice, not a syntax choice.

**Prior art:**
- **Rust tarpaulin** — `cargo tarpaulin --out Html`; produces HTML + lcov. No new
  Rust syntax. Jet takeaway: a `--coverage` flag on `jet test` is the right shape.
- **llvm-cov / cargo llvm-cov** — output: `--json`, `--lcov`, `--html`, `--text`.
  Jet takeaway: multiple formats are useful but can be deferred to a `--format`
  flag.
- **Python coverage.py** — `coverage run`; then `coverage report` / `coverage html`.
  Two-step. Jet takeaway: a single `jet test --coverage` that prints a summary to
  stdout (and optionally writes a report) is simpler than a two-step model.

**Deferred note:** if coverage ever needs a source annotation (e.g. `// @no_cover`
to exclude a line from the report), that is a syntax decision requiring a ballot.
Until then, coverage is tooling-only and can land without owner ratification. The
implementation milestone (exit criterion: `jet test --coverage` reports per-line /
per-function coverage) can proceed independently of D-TEST1 and D-TEST4.

---

## Constant-binding spelling: `::` → `$=` — board card c102

> Owner-requested 2026-06-22. **Reopens D-BIND1 / S2** (ratified 2026-06-18, which
> deliberately "spent the `::` token" on immutable bindings). The owner wants the
> binding family to be *consistent* — every binding/assignment operator should
> contain `=` — while keeping the immutable form **visually distinct from the
> mutable `:=`** so the two never blur at a glance or when zoomed out. This card
> only re-decides the *immutable* spelling; `:=` (mutable) and `=` (reassignment,
> S17) are unchanged.

### D-BIND2 — Spelling of the immutable binding (rec C — `$=`)

**User story.** Sam skims a 60-line function at editor-zoom-out (minimap) looking
for which values are fixed and which can change. Today immutable is `name :: expr`
and mutable is `name := expr` — two colon-led sigils that, shrunk down or read
fast, differ only by the second glyph (`:` vs `=`) and are easy to misread. Worse,
`::` is the one member of the binding family with no `=` in it, so it reads as a
different *kind* of operator than `:=` and `=`. Sam wants the immutable form to (a)
carry an `=` like the rest of the family and (b) have a distinct leading shape so a
fixed binding is unmistakable from a changeable one at a glance.

**The family today vs. the consistency goal.**

| Role | Today | Contains `=`? | Leading glyph |
|------|-------|---------------|---------------|
| immutable binding | `name :: expr` | ✗ | `:` (same as mutable) |
| mutable binding | `name := expr` | ✓ | `:` |
| reassignment (S17) | `name = expr` | ✓ | `=` |

The immutable form is the outlier on both axes the owner named: no `=`, and a
leading glyph identical to the mutable form. (`=` alone can't be the immutable
binding — it already means *reassign an existing `:=` binding*, S17.)

| Option | Immutable spelling | Logic signal | Psychology / first impression | Distinct from `:=` at a glance | Collision / baggage |
|--------|--------------------|--------------|-------------------------------|-------------------------------|---------------------|
| A `@=` | `name @= expr` | "anchored / located / attached" | feels deliberate and technical | high | `@` already carries label/address vibes |
| B `#=` | `name #= expr` | "marked / tagged / fixed by marker" | feels declarative and metadata-adjacent | high | `#` is already Jet's attribute sigil |
| **C `$=` (rec)** | `name $= expr` | "named value / fixed value" | feels lightweight, value-centric, and clearly non-mutable | high | `$` is unused in Jet today |

This is now the focused set: three two-character candidates, all prefix forms, all
containing `=`, all visually far from `:=`. The real question is not which one is
"available" — all three are available enough — but what *mental model* each one teaches.

- **Option A — `@=`.** Immutable becomes `name @= expr`; mutable stays `name := expr`.
  Its logic is "anchor this name to this value." That is coherent, but the psychology is
  more ambiguous: `@` often reads as location, address, label, mention, or indirection.
  In Jet specifically, `@` already has semantic gravity from loop labels, and in systems
  contexts it leans toward "address-like" rather than "fixed value." So `@=` looks crisp
  on the page, but it teaches the wrong thing: *where* or *which one*, not *unchangeable*.

```jet
ratio: Float @= 3.14
count: Int   := 0
count = count + 1             // reassign:  bare =, S17 (mutable only)
```

- **Option B — `#=`.** Logic: "this binding is marked/fixed." Psychologically this is the
  most *declarative* of the three. It feels like a definition stamp. The problem is local
  language coherence: Jet already spent `#` on attributes/tags (`#Pure`, `#Test`, `#Unsafe`,
  `#Audit`, `#layout`, etc.). Reusing `#` for a core binding operator creates a visual field
  where "things beginning with `#`" no longer belong to one conceptual family. That weakens
  one of Jet's current strengths: `#` means marker/qualifier syntax.

```jet
ratio: Float #= 3.14
count: Int   := 0
```

- **Option C — `$=` (recommended).** Logic: "this name is a value binding." The symbol is
  economically strong because it does **not** already mean "attribute", "address", "label",
  or "operator family" inside Jet. Psychologically it reads light, plain, and value-oriented.
  `$` already connotes "a named value" across shell, PHP, interpolation syntax, and general
  programmer culture, but it does **not** carry a strong assignment meaning. That makes `$=`
  easy to learn as "bind this fixed value here" without dragging in another Jet feature's
  semantics. It also stays maximally distinct from `:=` in minimap/zoomed-out reading.

```jet
ratio: Float $= 3.14
count: Int   := 0
count = count + 1

UserId $= distinct Int
PI     $= 3.14159
```

**Logic / psychology summary.**

| Prefix | What it logically suggests | What it psychologically feels like | Main risk |
|---|---|---|---|
| `@` | anchored / addressed / attached | technical, pointed, location-like | teaches address/label more than constness |
| `#` | marked / declared / qualified | declarative, stamped, metadata-like | collapses binding into the attribute/tag family |
| `$` | value / name / fixed slot | lightweight, value-centric, readable | some shell/PHP flavor, but no direct assignment baggage |

**Recommendation:** **C (`$=`)**. `@=` and `#=` are both internally coherent, but each
reuses a prefix Jet already wants to mean something else: `@` is directional/label-like and
`#` is Jet's marker/qualifier family. `$=` is the only one of the three that gives you the
owner's two hard requirements at once:

1. It contains `=` and keeps the binding family visually consistent.
2. It is obviously not `:=` at a glance.

And it does that **without** stealing meaning from another established Jet sigil family. That
is the decisive tradeoff.

**Owner Q (2026-06-22) — `$` is fine for me, but for people using Jet globally: is such a
commonly needed symbol an issue? What if people don't have it on their keyboard? How likely
is that — reasonable concern or not a big deal?**

Not a big deal — and it's *strictly consistent* with choices Jet already made. Three facts:

1. **`$` is plain ASCII (U+0024) and effectively universal.** Every physical keyboard layout
   in common use can type it; it's one of the oldest printable ASCII symbols and ships on the
   base plane of essentially every layout. The set of glyphs that is *truly* guaranteed on
   every national keyboard is the ISO-646 *invariant* subset — and that subset already
   excludes `$`, `#`, `@`, `` ` ``, `{`, `}`, `[`, `]`, `\`, `|`, `~`, `^`. Jet depends on
   `{ } [ ]` for blocks/collections, `#` for the entire attribute system, and `@` for loop
   labels. We left the invariant-subset constraint behind long ago; `$` adds nothing new to
   that ledger. If `#` and `@` are acceptable (they're load-bearing and ratified), `$` is no
   worse — by placement it's actually *more* uniformly available than `#`/`@`, which sit on
   AltGr/Option layers on several European and Mac layouts.

2. **Programmers type `$` constantly.** It's the variable sigil in shell, PHP, Perl; the
   template-literal lead in JS/TS (`${…}`); jQuery's `$`; Scala/Kotlin interpolation. There's
   no population of working programmers who lack muscle memory or a key for `$`. And because no
   mainstream language uses `$` as a *binding/assignment* operator, it carries no conflicting
   habit — a clean glyph for a clean meaning (the card's "no false analogy" point).

3. **The honest residual.** On a few layouts `$` is a shifted or AltGr keypress rather than a
   dedicated key — but so are `#`, `{`, `}` on those same layouts, and Jet already requires
   those every few lines. A constant binding is *rarer* in a file than a block brace, so `$`'s
   keystroke cost is bounded below the cost we've already accepted. The concern is reasonable
   to raise; it resolves to "no blocker."

Bottom line: ratifying `$=` introduces no new keyboard-accessibility burden beyond what Jet's
ratified `# @ { } [ ]` already imply. Recommendation stands at **A (`$=`)**. (Decision stays
open — owner's pick.)

---

## Expert numeric surface: overflow policy + values & ops — board card c103

> Owner-requested 2026-06-22 ("expand the integer and float to have values experts
> are most likely to use"). **The sized type *spellings* are already ratified**
> (D-SG9: `I8 I16 I32 I64 U8 U16 U32 U64 F32 F64`; `Int`=I64, `Float`=F64) but
> **unimplemented** (the `Type` enum is `Int`/`Float` only). This card does NOT
> re-decide spellings — it decides the **policy + value surface** experts need on
> top of them: arithmetic-overflow behavior, the per-type constants/operations a
> serious language is expected to ship, and conversions. Subsumes idea-cards fork
> **2.3** (checked overflow). Float *math* precision is the separate open D-FLOATW1;
> implementing D-SG9's sized ints (esp. `U8`) also **unblocks `embed_bytes` (c75)**.

### D-NUMOPS1 — Integer overflow behavior + the expert numeric value/op surface (rec A)

**User story.** Aisha is porting a checksum and a price calculator to Jet. She needs
(1) a `U8` that wraps deterministically in the hash, (2) an `I64` that **traps**
instead of silently wrapping when a balance overflows, and (3) the constants and
ops every systems language gives her: `U8.MAX`, `I64.MIN`, `F64.INFINITY`,
`F32.EPSILON`, `x.is_nan()`, bit ops (`<<`, `&`, `count_ones`), and explicit width
conversions. Today Jet has only `Int`/`Float` with unspecified overflow and no
named numeric constants — so she can't express either the wrap or the trap, and
reaches for another language. That's the legitimacy gap.

| Option | Default `+`/`*` overflow | Opt-outs | Per-type values/ops | Familiar to |
|--------|--------------------------|----------|---------------------|-------------|
| **A — checked by default; `wrapping`/`saturating`/`checked` opt-in (rec)** | trap (debug) / defined trap (release) | `wrapping(…)`, `saturating(…)`, `checked(…)→T?` | `T.MIN`/`T.MAX`, float `INFINITY`/`NAN`/`EPSILON`, `is_nan`, bit ops, `.to_<width>()` | Rust (debug), Swift (traps) |
| B — wrap by default (C/Go) | silent two's-complement wrap | `checked` for safety | same values | C, Go, Java |
| C — types only, overflow unspecified | unspecified | none | minimal | status quo |

- **Option A — checked by default, explicit escape hatches (recommended).** Plain
  arithmetic on any integer width **traps on overflow** (a safety bug becomes a
  loud failure, not silent corruption — Jet's safe-by-default identity). Experts
  opt a specific operation into a different discipline with a named wrapper, so the
  intent is visible at the call site. Every numeric type carries the constants and
  operations experts expect.

```jet
// safe default: overflow traps, doesn't silently corrupt
bal: I64 := I64.MAX
bal = bal + 1            // panics: integer overflow (I64) — not a wrapped negative

// expert opt-ins, visible at the use site:
h: U8 := wrapping(h * 31 + byte)     // hash WANTS modular wrap — say so
clamped: U8 := saturating(a + b)     // pin to U8.MAX instead of trapping
maybe: I32? := checked(x * y)        // ? on overflow, handle it as a value

// the values experts reach for, per type:
lo :: I64.MIN            hi :: U8.MAX            // 255
inf :: F64.INFINITY      nan :: F64.NAN          eps :: F32.EPSILON
if r.is_nan() { … }                              // float predicates
mask :: (flags << 2) & 0xFF                      // bit ops on integer widths
n :: bits.count_ones()

// explicit width conversion (mirrors D-SG9 / D-FLOATW1 — no implicit narrowing):
small: U8 := big.to_u8()?            // ? because it can't always fit
wide:  I64 := small.to_i64()         // widening is total, no ?
```

- **Option B — wrap by default (C/Go).** Plain `+` wraps two's-complement; you opt
  *into* safety with `checked`. Familiar to C/Go/Java refugees and matches hardware,
  but it makes the dangerous thing the default — the silent-overflow bug class Jet's
  safety stance exists to kill. The hash case is ergonomic; the balance case ships
  the bug.

```jet
bal: I64 := I64.MAX
bal = bal + 1            // silently becomes I64.MIN — no signal; the classic CVE
```

- **Option C — types only, overflow unspecified.** Ship the widths, leave `+`
  behavior undefined/implementation-detail, no constants. Smallest decision, but
  "unspecified overflow" is exactly what a serious language must not have, and the
  missing constants force every expert to hand-roll them.

**How others do it.** Rust traps in debug, wraps in release, with
`wrapping_*/saturating_*/checked_*` methods + `i32::MAX`/`f64::EPSILON` — the model
A adapts (Jet makes the trap consistent across profiles so behavior is predictable).
Swift traps by default with `&+` for wrap. Zig requires you to pick (`+%` wrap, `+`
trap). C/Go wrap silently (B) — the source of countless overflow bugs. Python has
arbitrary-precision ints (Jet declined these in D-SG9). The presence of
`checked/wrapping/saturating` + named constants is table-stakes for a language
claiming systems credibility.

**Recommendation:** **A** — checked-by-default keeps the safe-by-default identity
(the trap turns a silent corruption into a caught bug), while the three named
escape hatches give experts exact control with the choice visible in review. Ship
the per-type `MIN`/`MAX`, float `INFINITY`/`NAN`/`EPSILON` + predicates, bit ops,
and explicit `.to_<width>()` conversions as the standard surface. Prerequisite:
implement D-SG9's sized integers (this is the work that also unblocks `embed_bytes`,
c75). Sequence the overflow-trap codegen alongside; conversions mirror D-SG9's
no-implicit-narrowing rule and D-FLOATW1's float policy.

---

## Serde-grade serialization: unified Serialize + Deserialize — board card c104

> Owner-requested 2026-06-22 ("improvements based on serde"). Jet already ratified
> a built-in **`Serialize`** derive (S55/S56) and has typed JSON/CSV
> (D-JSONOUT1/D-CSVROW1) + `core` `parse_json`. What serde has that Jet doesn't:
> (1) a **format-agnostic data model** so ONE derive drives JSON *and* CSV *and*
> TOML *and* binary; (2) a **`Deserialize`** counterpart (Jet can emit typed JSON
> but has no general typed *decode*); (3) **field attributes** (rename/default/
> skip/flatten). The idea-cards file parks "serde-unified" as a 3-Horizon north-star
> blocked on user-defined derives (S56, Epoch 3) — this promotes it to a concrete
> ballot so the *shape* is decided now and built when derives land.

### D-SERDE1 — A unified, format-agnostic Serialize/Deserialize model (rec A)

> **Owner directive (2026-06-22, from D-CSVROW1=A):** **CSV must be one of the formats this
> unified model handles** (alongside toml/yaml/json). The ratified `csv.decode<Row>(record)`
> comptime path (D-CSVROW1) is the CSV arm of *this* model — it must share the same decoder
> mechanism, not a parallel one. Fold CSV into the format list when this is decided.

**User story.** Dmitri has a `Config` struct. He wants `Config.to_json()`,
`Config.to_toml()`, and `Config.from_json(text)?` to all just work from **one**
`#[Serialize, Deserialize]` on the type — and he wants `#[rename("user-id")]` on a
field so the wire name differs from the Jet name, and a default for a missing field
on decode. Today he can derive `Serialize` (S55) but there's no `Deserialize`, and
each format would be its own bespoke method — the tRPC/Zod/hand-rolled-codec mess
Jet wants to displace.

| Option | Derive count for N formats | Decode (`Deserialize`) | Field control | New machinery |
|--------|----------------------------|------------------------|---------------|---------------|
| **A — one data-model, format adapters (serde model) (rec)** | 1 derive, any format | yes, symmetric | `#[rename/default/skip/flatten]` | a `Serializer`/`Deserializer` protocol + per-format adapter |
| B — per-format derives (`ToJson`, `ToToml`, …) | N derives | per-format | per-format attrs | less core, more boilerplate + drift |
| C — Serialize only, no Deserialize | 1 (encode only) | **no** | encode attrs only | least; but can't *read* typed data |

- **Option A — one data model, format adapters (the serde architecture) (rec).**
  A type derives `Serialize`/`Deserialize` once against an abstract data model
  (records, sequences, scalars, enums-as-tagged-unions). Each format ships an
  adapter implementing the `Serializer`/`Deserializer` protocol; the derive is
  written once and works for every present and future format. Field attributes
  control the wire shape.

```jet
#[Serialize, Deserialize]
struct Config {
    #[rename("user-id")] user_id: Str
    #[default] retries: Int           // missing on decode → Int's default
    #[skip] cache: Cache              // never crosses the wire
}

cfg  :: Config.from_json(text)?       // typed decode, errors as values (S34)
js   :: cfg.to_json()                 // same derive
to   :: cfg.to_toml()                 // ...drives every format
// a new `msgpack` adapter later needs ZERO changes to Config.
```

- **Option B — per-format derives.** `#[ToJson]`, `#[ToToml]`, … each generate
  their own code. Less shared machinery, but N derives per type, N sets of
  attributes, and the formats drift apart — the opposite of "define once."

```jet
#[ToJson, FromJson, ToToml, FromToml]   // and one more pair per format, forever
struct Config { … }
```

- **Option C — encode-only (status quo + nothing).** Keep `Serialize`, never add
  `Deserialize`. But a language that can write typed data and not *read* it back
  isn't credible for config/APIs/persistence — the decode half is half the value.

**How others do it.** **serde** (Rust) is the gold standard: one
`#[derive(Serialize, Deserialize)]`, a data-model the format crates plug into,
`#[serde(rename/default/skip/flatten/rename_all)]`; ~every Rust data library speaks
it. Go's `encoding/json` uses struct tags but is JSON-specific and reflection-based
(slower, less general). Python's `pydantic`/`dataclasses` validate+parse but are
runtime. Swift's `Codable` is the closest mainstream analog to serde's compile-time
model — one protocol, many encoders. A is serde's proven architecture, which is
*why* serde is universally lauded.

**Recommendation:** **A** — the format-agnostic data model is the entire reason
serde won; one derive that drives every format (and future formats for free) is a
massive legitimacy and ergonomics win and directly advances the "replace the
TypeScript/codec dance" thesis. Add `Deserialize` as the symmetric counterpart to
S55's `Serialize`, and the `#[rename/default/skip/flatten/rename_all]` attribute
set. **Gated on user-defined derives (S56, Epoch 3)** — the derive engine is the
prerequisite; ratify the shape now, build when S56 lands. Keep the data model in
Core; each format adapter is a ring library (matches the two-tier lib design).

---

## Iterator adapters: the lazy-sequence surface — board card c105

> Owner-requested 2026-06-22 (compare to lauded libraries; reinforce legitimacy).
> Jet has composable iterators (E2-M7) and `map`/`filter`/`sum`. The gap vs.
> Rust's `Iterator` / Python `itertools` / Swift sequences is the **breadth of
> lazy adapters** experts reach for daily — the single most-cited "is this a
> serious language?" library surface after collections.

### D-ITER1 — Standard lazy iterator-adapter set (rec A)

**User story.** Lena processes a log stream. She wants `lines.enumerate()`,
`a.zip(b)`, `xs.chunks(100)`, `events.windows(2)`, `items.group_by(Item.kind)`,
`xs.take_while(|x| x.ok)`, `xs.flat_map(expand)`, `xs.scan(0, +)` — the everyday
toolkit. Today she writes manual `loop`s with index bookkeeping for each, which is
exactly the boilerplate a lauded standard library removes.

| Option | Adapter set | Laziness | Surface cost |
|--------|-------------|----------|--------------|
| **A — full lazy adapter set on the iterator protocol (rec)** | enumerate, zip, chunks, windows, take/skip(_while), flat_map, scan, group_by, dedup, step_by, peekable, partition, find/position, fold/reduce, min/max_by | lazy, allocation-free until collected | one trait-method family; no new grammar (rides the ratified iterator protocol) |
| B — minimal set, rest in a third-party lib | map/filter/sum + a few | lazy | smaller Core; ecosystem fragmentation, everyone re-adds the basics |
| C — eager collection methods only | operate on built lists | eager | simplest; defeats streaming (E2-M7) and allocates per step |

- **Option A — the full lazy set on the iterator protocol (rec).** Each adapter is
  a method on the ratified iterator protocol (Tier-1 blessed protocol, D-EXT1), lazy
  and composable; nothing materializes until a terminal op (`collect`, `sum`,
  `for`). No new grammar.

```jet
// everyday toolkit, lazy and chainable — no manual index loops:
for (i, line) in lines.enumerate() { … }
pairs   :: names.zip(scores)
batches :: rows.chunks(100)                 // [[row;100], …]
deltas  :: ticks.windows(2).map(|w| w[1] - w[0])
byKind  :: items.group_by(Item.kind)
head    :: xs.take_while(|x| x.is_ok())
flat    :: nested.flat_map(|g| g.items)
running :: nums.scan(0, |acc, x| acc + x)   // running totals, lazy
```

- **Option B — minimal Core + third-party.** Ship only the basics; leave the rest
  to a library. Smaller Core, but every serious program pulls the same missing
  adapters from somewhere, and the ecosystem splinters on naming — the fragmentation
  Python/Go avoid by putting this in the standard library.

- **Option C — eager only.** Methods operate on fully-built lists. Simple, but
  allocates an intermediate per step and can't run over a stream (defeats the E2-M7
  streaming I/O design).

**How others do it.** Rust's `Iterator` (~70 adapters, all lazy, zero-cost) is the
benchmark and a top reason Rust feels productive; `itertools` adds `group_by`,
`chunks`, `windows`, `dedup`. Python's `itertools` is a flagship stdlib module.
Swift's lazy sequences and C#'s LINQ are the same idea. A language without this set
reads as a toy for data work.

**Recommendation:** **A** — the lazy adapter set is high-leverage, needs no new
grammar (methods on the ratified iterator protocol, D-EXT1 Tier 1), and removes the
manual-loop boilerplate that makes a stdlib feel small. Put the common set in Core
on the iterator protocol; keep it lazy to honor streaming (E2-M7). Name conservative,
familiar spellings (`enumerate`/`zip`/`chunks`/`windows`/`group_by`) so refugees
from Rust/Python/Swift are immediately at home.

---
