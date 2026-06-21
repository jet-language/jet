# Decision ballots — open owner queue

Every open decision, and **nothing else**. The instant a decision is submitted it
leaves this file: it is recorded in the decision log in
[`syntax-decisions.md`](syntax-decisions.md) and removed here. No "recently
ratified" section, no decided history — decided decisions never reappear.

**House rule for whoever edits this file (enforced — a card missing any of these is
not ballot-ready):** every full decision card carries, in this order, (1) a **user
story** — a real person and what they're doing, so the owner sees why the decision
exists; (2) a **short tradeoff comparison** — a compact table, one row per option,
columns that actually differ (ceremony / failure mode / ratification cost /
familiarity); and (3) a **worked example of every option** in a fenced ```jet (or
```shell) block — what that person types, sees, and hits as an error. No abstract
option tables standing in for examples. Close with `**Recommendation:**` + a one-line
why. Decisions not yet drafted to that bar are listed below as one-liners with a
recommendation; expand one into a full card when it's time to decide it.

---

## Open decisions

> **17 open decisions across 13 cards** (incl. testing ergonomics), plus a deferred-ballots list and informational notes.
> story (why it exists), a tradeoff table, and a worked example per option. Cards
> **c25** (range sugar) and **c55** (REPL v2) turned out implement-only — every
> choice they raised is already covered by ratified decisions — so nothing is
> queued for them here. Submitting a decision records it in `syntax-decisions.md`
> and removes it from this file.

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

## Step-through debugger — board card c52


# Draft ballot cards — c52 (DAP debugger) + c25 (range arms)

> Status: draft — not yet queued in `decision-ballots.md`
> Date: 2026-06-20
> Prerequisite ratified decisions: D-DBG1 (`jet debug <file>` verb), D-OBS1 (source maps + Jet-line panics), D-RANGE1 (range arms reuse `..`), D-RANGE2 (ownership split), D-PATR (range patterns + exhaustiveness)

---


One choice remains for the owner. D-DBG1 (the `jet debug` command name) and D-OBS2 (the line-table is a sidecar `.jetmap`, ratified 2026-06-21) are settled; D-OBS1 scheduled the DAP debugger as a GA gate. The open decision below covers the policy for generated/library frames that have no Jet source line.

---

### D-DBG2 — Policy for frames with no Jet source line (rec A)

**User story.** A developer is stepping through a Jet program that calls `core.fs.read_file(path)`. Execution steps into generated glue code or a Rust `std` function that has no Jet source line. What does the editor show?

| Option | Editor display | Beginner experience | Expert escape hatch | I2 compliance |
|--------|---------------|---------------------|---------------------|---------------|
| A — step over silently; surface only frames with a Jet line | Next Jet frame shown | Clean; no Rust noise | None | Yes — no Rust paths/lines ever surface |
| B — show a synthetic frame `[jet runtime]` with no file/line | Placeholder frame visible | Slightly noisy but honest | No detail | Yes — still no Rust identity |
| C — show the raw Rust frame (file + line) | Rust file/line in editor | Confusing; breaks I2 | Yes | No — I2 violation |

- **Option A — step over any frame that has no Jet source line; resume at the next Jet frame.**
  The adapter walks the lldb frame list, skips every frame whose Rust line does not appear in the `.jetmap` table, and surfaces only the first (innermost) Jet frame.

    ```
    # user is in main(), steps into core.fs.read_file — no Jet line
    # adapter silently steps over the generated glue
    # next stop: back in main() after the call returns
    breakpoint hit  loops.jet:7  in main()
    (jet-dbg) step
       9 |     total += i       ← next Jet line; glue was invisible
    locals:  n = 5   total = 5   i = 2
    ```

    Fully I2-compliant. The cost: a user cannot inspect Jet stdlib internals at the source level (they see the call complete atomically). Acceptable for v1; expert source-level stdlib debug is a post-GA concern.

- **Option B — surface a synthetic frame `[jet runtime]` at any depth with no Jet source.**
  The adapter inserts a placeholder frame when a non-Jet frame is innermost, showing a label but no file or line.

    ```
    breakpoint hit  loops.jet:7  in main()
    (jet-dbg) step
    [jet runtime] — inside core.fs.read_file (no Jet source available)
    (jet-dbg) step
       9 |     total += i
    ```

    I2-compliant (no Rust paths). More visible about what is happening, but adds an extra step/frame the user must work through. Useful if users need to know "I am inside a runtime call."

- **Option C — show the raw Rust frame.**
  The adapter passes the lldb frame through as-is when no Jet line is found. The editor shows `src/std/fs.rs:418`.

    ```
    breakpoint hit  loops.jet:7  in main()
    (jet-dbg) step
    /rustc/.../src/std/fs.rs:418   ← Rust path surfaced to user
    ```

    Direct I2 violation. Listed to be explicitly closed.

**Recommendation:** A. Silent step-over is the cleanest beginner experience and is the hardest I2 guarantee to weaken later. The adapter can log a debug-level trace (visible only in `jet debug --verbose`) so developers of the adapter itself can see skipped frames, while users never do. Option B is a reasonable upgrade once users report that the opaqueness is confusing.

---

---

## Allocators — board card c05

> D-ALLOC2 ratified **2026-06-21 (option A — scope-bound `view`)**. A needs a **region**
> (the lifetime of the `arena ::` scope) that `view` (S10/c06) does not yet have, so it
> cannot be built until the region rule below is decided. D-REGION1 is the follow-on.

### D-REGION1 — How is an allocation region denoted? (rec A)

**User story.** Priya allocates per-frame scratch into an arena and wipes it with one
`reset`. She has never written a lifetime and does not want to. The compiler still has to
stop her from stashing a frame-N node where it outlives the arena. The question: does she
write *anything* to mark the region, or does the compiler infer it from the `arena ::`
binding scope she already wrote?

| Option | Region is | User writes | Escape check | Familiarity |
|--------|-----------|-------------|--------------|-------------|
| A — implicit, scope-inferred (rec) | the lexical scope of the `arena ::` binding | nothing | view tied to that scope; escaping it is E0631 | no lifetimes — matches Jet's "never write a borrow" promise |
| B — explicit `region r { … }` block | a named region introduced by a block | `region r { … }` wrapper | views carry `r`; leaving the block ends `r` | one new block keyword; visible but more ceremony |
| C — named lifetime on the binding | a user-named region label | `arena ::'a mem.Arena.new()` | views carry `'a` | Rust's `'a` — the exact thing Jet promised never to surface |

- **Option A — implicit, scope-inferred (rec).** The region *is* the scope of the
  `arena ::` binding; no syntax. The checker attaches that scope to each `alloc` view and
  rejects escape/use-after-reset by name.

```jet
fn frame(world: view World, out: mut [view CollisionPair]) {
    arena :: mem.Arena.new(capacity: 1 << 20)
    node  :: arena.alloc(CollisionPair{ a: 1, b: 2 })
    node.a += world.gravity        // ok — inside the arena's scope
    out.push(node)                 // error[E0631]: `node` escapes the arena that made it
}
```

- **Option B — explicit `region r { … }` block.** A region is a named block; views inside
  it carry `r` and may not leave it.

```jet
fn frame(out: mut [view CollisionPair]) {
    region r {
        arena :: mem.Arena.new()
        node  :: arena.alloc(CollisionPair{ a: 1, b: 2 })
        out.push(node)             // error[E0631]: `node` may not leave region `r`
    }
}
```

- **Option C — named lifetime on the binding.** A Rust-style label names the region.

```jet
fn frame() {
    arena ::'a mem.Arena.new()     // surfaces a lifetime name — the thing Jet promised to hide
    node  :: arena.alloc(Node{ id: 7 })
}
```

**Recommendation:** A as the inferred **beginner** default, **plus B as the explicit expert
tier** (see the owner Q below) — not A *xor* B. C reintroduces exactly the borrow-lifetime
surface Jet exists to avoid. Unblocks D-ALLOC2-A on ratification.

---

**Owner Q (2026-06-21) — how is this supportive of experts? Jet is beginner-automagic AND
expert-full-control.**

You're right to push on this — A alone is only the beginner half. The dual-facet answer is
**A is the inferred default; B (`region r { … }`) is the expert opt-in**, and both ship.
That's the same shape as the rest of Jet (capabilities are inferred but can be written;
`@unsafe`/`core.mem` is the expert gate over safe defaults):

- **Beginner / automagic (A, inferred):** write `arena :: mem.Arena.new()`, allocate, and
  the region *is* that binding's scope. No lifetime, no `region` block, no annotation. The
  checker silently proves no allocation escapes. 99% of code never names a region.

```jet
fn frame() {
    arena :: mem.Arena.new()
    node  :: arena.alloc(Node{ id: 7 })   // region inferred from `arena ::`
    use(node)                              // freed at scope end, automatically
}
```

- **Expert / full control (B, explicit `region`):** the moment an expert needs a region the
  inference *can't* give them — one spanning **two** allocators, or **narrower** than the
  enclosing function, or one they pass to a helper so allocations can flow back out under a
  named bound — they write it explicitly. The named region is the expert's lever; the
  checker enforces the same escape rule against `r` instead of an inferred scope.

```jet
fn build(out: edit Scene) {
    region r {                              // expert names the region explicitly
        scratch :: mem.Arena.new()
        staging :: mem.Arena.new()          // two arenas, ONE region `r`
        for part in source {
            node :: scratch.alloc(part.mesh)
            out.commit(node, in: r)         // legal: `node` lives as long as `r`
        }
    }                                       // both arenas freed here, together
    // a beginner never writes `region`; an expert reaches for it when scope-inference
    // is too coarse or too fine for what they're building.
}
```

So experts are not boxed in by the automagic: A removes lifetimes from the 99% path, and B
hands the expert an explicit, named region with full control over the 1% — exactly the
beginner-magic / expert-control split. (C is still rejected: a Rust-style `'a` *forces* the
lifetime surface on everyone, which is neither the beginner default nor a clean expert tier.)
**Revised recommendation: ratify A + B together.**

---

## Qualifier system: traits, effects & tags — board card c62

### D-QUAL1 — Organizing traits, effects & tags across three reader-split surfaces (rec A — Core D, with Roles)

This is the c62 linchpin: a single rule for where every "label" concept lives — traits, effects, value-facts, capabilities, markers, prohibitions — so each surface stays sparse and every declaration stays legible. The proposal (Variant D) routes by **who reads it**, and the hybrids are optional surfaces layered on top.

**User story.** Two people read the same `checkout` service. Priya, a feature dev, opens a function and needs to know *at a glance* what it touches (does it hit the network? the DB?) and what its types *are* (is `Receipt` serializable? can it be silently dropped?). Sam, the security owner, never reads function bodies — he needs *one* auditable place that says what the coupon plugin is allowed to do. Today these concerns would pile onto the same declaration and drown each other. D-QUAL1 picks a routing rule so each reader sees only their surface.

**The routing rule (the whole mnemonic — shape mirrors meaning).**

- **`#(…)` round parens → what it *touches*** (effects). On the signature line — a per-caller contract everyone must see.
- **`#[…]` square brackets → what it *is*** (a static *list* of tags: derives, traits, value-facts, markers). Above the declaration, for library users.
- **`module { … }` manifest → what it's *allowed* to do** (capability policy). One auditable place, for security/ops.

Round = runtime reach. Square = a static attribute list. Manifest = permissions. A beginner needs four facts: *types hold data, `#(…)` says what a function touches, `#[…]` is the tag list, the manifest walls things off.*

| Option | Effect surface | Grouping power | Beginner read | Signature-glance contract | Best when |
|---|---|---|---|---|---|
| **A Core D** | inline `#(…)` | good | clear | **yes** | sensible default |
| **B × Roles** | `#(Role)` | **highest (DRY)** | high at use-site | yes (via role) | large codebases, shared policy |
| **C × Unified block** | `#[ effects: … ]` | **highest (visual)** | high | no (look above) | declaration-heavy review |
| **D × Grammar** | `does (…)` | good | **highest** | yes | onboarding-first teams |
| **E × Type-row** | `! {net, db, log}` | good | lower | yes | effects must flow through generics |

These compose: A is the base; B/D are surface skins; C regroups everything into the bracket block. The strongest practical combo is **A + B**, with **D** as an optional grammar skin.

- **Option A — Core D (recommended).** Three surfaces, sigil-spelled. Effects inline on the signature, tag lists in `#[…]` above the declaration, capability policy in the `module { }` manifest. Inline `#fact` attaches a value-fact locally to one value.

```jet
// ── manifest: the security owner reads only this ──
module shop.checkout {
    plugins.coupon: deny(fs, db)        // policy collected here, never inline
}

// ── data: a list of tags reads like a list ──
#[
    derive(Comparable, Serialize),
]
struct Order { id: OrderId, total: Usd }

#[
    derive(Serialize),
    linear,                             // value-fact: can't be silently dropped
]
struct Receipt { order: OrderId, paid: Usd }

// ── logic ──
fn cart_total(items: [Item]) -> Usd {   // bare: pure, inferred
    items.sum(Item.price)
}

fn charge(o: take Order #unpaid) #(db) -> Receipt #paid ?   // typestate + effect

pub fn checkout(req: Request) #(net, db, log) -> Response ? {  // contract on line 1
    raw  :: req.body() #tainted         // value-fact rides the value inline
    rcpt :: charge(parse(sanitize(raw))?.order)?   // sanitize strips #tainted
    record(rcpt)?                       // MUST consume the linear receipt
    Response.ok()
}
```

- **Option B — Core D × Roles (named bundles).** Define a contract once in the manifest; wear it by name. A *role* is a named effect-set or tag-set, referenced wherever its members would go. The DRY answer for a many-route service. Cost: indirection — you open `Handler` to see what it touches (mitigate with `jet explain #(Handler)`).

```jet
module shop.checkout {
    role Handler = #(net, db, log)                            // an effect role
    role Money   = #[derive(Comparable, Serialize), linear]   // a tag role
    plugins.coupon: deny(fs, db)
}

#[ Money ]                              // expands to the tag list above
struct Receipt { order: OrderId, paid: Usd }

pub fn checkout(req: Request) #(Handler) -> Response ? { ... }   // one word = full contract
pub fn refund(req: Request)   #(Handler) -> Response ? { ... }   // change Handler once, both update
```

- **Option C — Core D × Unified labeled block.** Group *everything* — effects included — in the bracket list using labeled sections that self-route. Cost: effects leave the signature line, so a caller glances *above* the function instead of *at* it.

```jet
#[
    effects: net, db, log,
    panics:  never,
    marker:  route("/checkout"),
]
pub fn checkout(req: Request) -> Response ? { ... }

#[
    derive: Comparable, Serialize,
    facts:  linear,
]
struct Receipt { order: OrderId, paid: Usd }
```

- **Option D — Core D × Grammar keywords.** Keep the three surfaces but spell the two inline ones in English. Most readable for newcomers; effects (`does`) vs traits (`is`) are visually unmistakable. Cost: `is / does / forbids / as` are four keywords doing what one sigil family did, and reads less "systematic" to experts.

```jet
module shop.checkout {
    plugins.coupon forbids (fs, db)
}

struct Receipt is (Serialize), linear { order: OrderId, paid: Usd }

pub fn checkout(req: Request) does (net, db, log) -> Response ? {
    raw :: req.body() as tainted
    ...
}
```

- **Option E — Core D × Type-row effects.** Make the effect surface a type row on the return. Strictly more composable (effects flow through generics uniformly), but the heaviest-looking. Worth it only if effects must propagate through generic code.

```jet
pub fn checkout(req: Request) -> Response ! {net, db, log} ? {
    raw :: req.body() #tainted
    ...
}
```

**Recommendation:** **A (Core D)** as the ratified base — it keeps each surface sparse, puts the effect contract on the signature line where every caller sees it, and needs only four facts to teach. Adopt **B (Roles)** alongside it for any codebase with shared policy across many routes (the strongest practical combo is A + B). **D (Grammar)** is an optional skin to ratify later if onboarding wins over expert density; **C** and **E** are situational and can stay declined unless a review style or generic-effect-propagation need forces them.

**Interactions with ratified decisions (read before ratifying — A would amend these).**
- **D-ATTR2** ratified the multi-marker list as **bare** `#[Serialize, Comparable]` and explicitly *rejected* the Rust-literal `#[derive(…)]` form. The examples above use `#[ derive(Comparable, Serialize) ]` — ratifying D-QUAL1 as written would **reverse D-ATTR2** on that point. Decide whether tag lists keep the bare form (`#[Comparable, Serialize, linear]`) or adopt the `derive(…)`-grouped form here.
- **S60** deliberately rejected a full effects system (`pure fn` is the one ratified effect-tag). The `#(net, db, log)` effect surface **reopens S60**. D-QUAL1 is the place to decide that reopening explicitly.
- **S56 / S83** ratified user-defined derives via the external connector `~~` (`derive Point~~Serialize`); `#[…]` is for built-in derive *markers*. Keep the two distinct: `#[…]` lists markers, `~~` attaches a derive impl.
- **Manifest surface**: the `module shop.checkout { … }` block overlaps `pkg.jet` (`payload:`/`packages:`, D-JPK-FILES) and module paths (D-MOD1 uses `.`). Decide whether capability policy lives in `pkg.jet`, in an in-source `module { }` block, or both.

---

## Effect system — board card c66

### D-EFF1 — An effect system, expressed as tags on functions (rec B)

**User story.** Lena maintains a 200-file Jet service. A junior just landed a PR
where a function deep inside the pricing logic — a function everyone assumed was a
pure calculation — quietly grew a `core.net.fetch(...)` call to hit a currency API.
Nothing flagged it. Now the pricing path makes a network round-trip per line item
and nobody noticed until production latency spiked. Lena wants the compiler to know
which functions touch the network, the disk, the clock, the RNG — and to *stop* a
function she has declared pure from silently gaining a side effect. She does not
want to hand-annotate 200 files to get it.

| Option | Who writes effects | Failure mode it catches | Ceremony | Reopens S60? |
|--------|--------------------|-------------------------|----------|--------------|
| A — none (status quo) | nobody (`pure fn` only) | only "this `pure fn` isn't pure" | zero | no |
| B — inferred, annotate at boundaries (rec) | compiler infers; you assert/restrict | hidden effect creep, capability leaks, taint sinks | low — boundaries only | **yes** |
| C — explicit always | every function, every effect | same as B | high — the coloring tax | yes |

#### How other languages do this

- **Koka** — row-polymorphic effect *inference*: every function's type carries an
  inferred effect row (`<console,exn>`); you rarely write them, the compiler
  propagates. Takeaway: inference + propagation is the proven way to avoid the
  coloring tax — this is the model B copies.
- **Frank / Eff / Effekt** — algebraic effects with **runtime handlers**: an effect
  is *performed* and a dynamically-installed handler resumes the computation.
  Takeaway: powerful but a runtime mechanism; Jet wants none of the handler runtime
  — effects are a static fact, then erased.
- **OCaml 5** — effect handlers in the runtime (used for the scheduler/concurrency),
  but effects are **not yet tracked in the type system**. Takeaway: even a flagship
  ML shipped handlers before static checking; Jet inverts that — static checking,
  no handlers.
- **Unison** — "abilities" are typed effects (`{IO, Exception}`) checked at compile
  time and discharged by handlers. Takeaway: closest to a clean typed-effect surface
  on a function signature; Jet borrows the surface, drops the handler discharge.
- **Haskell (mtl / monad transformers)** — effects encoded as type-class
  constraints (`MonadReader`, `MonadIO`) stacked in a transformer tower. Takeaway:
  expressive but the stack is the coloring tax made manifest; Jet refuses to make
  beginners thread a monad stack.
- **Rust** — *no* effect system: `unsafe` and the `Send`/`Sync` auto-traits are the
  only coarse "capability" propagation, and async is famously a function color.
  Takeaway: the gap D-EFF1 fills — Rust users feel the missing effect layer most.

**Jet's is unlike all of these at runtime: STATIC + INFERRED + ERASED.** There is no
handler, no monad, no runtime effect value. The effect set is computed in sema,
checked against any assertion the user wrote, and then thrown away — codegen emits
plain Rust with no trace of it (I3). An effect is just a compile-time tag on a
function; `pure fn` (S60) is the empty set.

- **Option A — no effect system; keep only `pure fn`.** S60 stands as-is. The only
  thing the compiler knows is "this one function claimed purity." It cannot tell you
  what an impure function touches, cannot wall the network out of a subtree, and
  cannot back D-SCAP1 or D-TAINT1 (both need propagation).

```jet
pure fn price(items: [Item]) -> Usd {
    items.sum(Item.price)          // ok — provably pure
}

fn quote(items: [Item]) -> Usd {   // just "not pure" — touches WHAT? unknowable
    log(items.len())               // network? disk? clock? the type can't say
    price(items)
}
// A junior adds core.net.fetch(...) inside quote(). No diagnostic. Nothing to
// assert against, because there is no effect to name.
```

- **Option B — inferred effect tags; annotate at boundaries + cap regions (rec).**
  The compiler infers each function's effect set from its body and propagates it
  along calls (an effect of a callee is an effect of the caller, exactly like Koka's
  rows). You only *write* an effect to **assert** ("this function touches at most
  `#net`") or to **restrict** a region. A `pure fn` whose body gained an effect is a
  compile error. A scoped cap region `#caps(net) { … }` (S82 + D-ATTR1 marker-region
  form) bounds what the enclosed code may touch — anything outside the allowed set is
  rejected at the call site. All compile-time; erased in codegen.

```jet
pure fn price(items: [Item]) -> Usd {
    items.sum(Item.price)
}

fn quote(items: [Item]) -> Usd {   // inferred effect set: {#net} (from fetch_rate)
    rate :: fetch_rate()?          // fetch_rate is inferred #net; quote inherits it
    price(items) * rate
}

// Boundary assertion: the public entry point declares its contract on line 1.
pub fn checkout(req: Request) #(net, db) -> Response ? {
    rate :: fetch_rate()?          // #net — allowed
    save(order)?                   // #db  — allowed
    Response.ok()
}

// Restrict a region: inside here, only #net is permitted.
fn render_card(c: view Card) {
    #caps(net) {
        thumb :: fetch_image(c.url)?    // ok — #net
        write_temp(thumb)?              // error[E0701]: effect `#fs` not permitted
                                        //   in this `#caps(net)` region
                                        //  --> card.jet:4:9
                                        //   |
                                        // 4 |         write_temp(thumb)?
                                        //   |         ^^^^^^^^^^ `write_temp` touches the
                                        //   |                    disk (#fs); this region allows
                                        //   |                    only #net
                                        //   help: widen the region — `#caps(net, fs) { … }` —
                                        //         or move the write outside it
    }
}

// The bug Lena hit, now caught:
pure fn price(items: [Item]) -> Usd {
    rate :: fetch_rate()?          // error[E0702]: `pure fn price` performs effect `#net`
                                   //  --> price.jet:2:13
                                   //   |
                                   // 2 |     rate :: fetch_rate()?
                                   //   |             ^^^^^^^^^^^^ `fetch_rate` touches the
                                   //   |                          network; `price` is declared pure
                                   //   help: drop `pure`, or pass the rate in as a parameter
    items.sum(Item.price) * rate
}
```

  **Flag — this REOPENS S60.** S60 ratified `pure fn` as the *one* effect tag and
  "deliberately rejected a full effects system." Option B is that full system. It does
  not contradict `pure fn`'s spelling or meaning (purity becomes the empty effect set,
  the natural bottom of the lattice) but it *does* reverse S60's "no further effects"
  stance. The owner must reopen S60 to ratify B. **B is recommended but gated on
  resolving five sub-questions** before implementation: (1) **effect polymorphism /
  coloring** — does a higher-order fn like `map(f)` propagate `f`'s effects, and how is
  that written (Koka does it with effect-row variables; Jet needs a beginner-legible
  answer or an explicit "effects don't cross the `fn(...)` type boundary in v1"
  limitation); (2) **trait-bound interaction** — can a trait method declare/forbid
  effects, and does an `impl` have to honor it; (3) **diagnostic quality** — the
  whole value is in errors like E0701/E0702 reading well at scale; (4) **surface
  spelling** — `#net` inline tags vs. a `! {net, fs}` return-row slot (D-QUAL1's
  Option E) — pick one and pin it; (5) **overlap with D-QUAL1's `#(…)`** — these are
  the same surface and must not ship two spellings.

- **Option C — explicit effects always.** Every function annotates every effect it
  performs; no inference. This is the coloring tax in full: a one-line refactor that
  adds a `log()` call forces an effect annotation onto that function *and every
  transitive caller*, all the way up.

```jet
fn deep(x: Int) #log -> Int {      // add one log()...
    log(x)
    x + 1
}
fn mid(x: Int) #log -> Int { deep(x) }      // ...now mid must say #log...
fn top(x: Int) #log -> Int { mid(x) }       // ...and top, and its callers, forever.
// error[E0703]: `mid` calls `deep` (#log) but does not declare effect `#log`
//   help: add `#log` to mid's signature  — repeated up the entire call chain
```

**Recommendation:** B — inference kills the coloring tax (the thing that makes C
unlivable and A's `pure fn` an island), keeps `pure fn` meaningful as the empty set,
and is the only option that can carry D-SCAP1 and D-TAINT1. Ratify only after pinning
the surface spelling against D-QUAL1 and answering effect-polymorphism, and reopen
S60 explicitly.

---

<!-- value-tags cluster: D-UNIT1, D-LIN1, D-STATE1 -->

# Value-tag cluster — draft ballot cards

Three cards. All assume tags are first-class (gated on D-QUAL2 from the c62
qualifier-taxonomy work). UNIT1 and LIN1 are lower complexity; STATE1 is
mid-pack. None of these cards should be ratified before D-QUAL2 settles the
tag-vs-effect-vs-trait routing rule.

**Dependency note (applies to all three cards):** D-QUAL1 (c62) proposed the
taxonomy: a *tag* is a label without methods, written `#[Tag]` on a declaration
or `#Tag` inline on a value. D-QUAL2 is the ballot that ratifies whether tags
are first-class in the language at all and what surface they live on. D-UNIT1,
D-LIN1, and D-STATE1 are built on top of that; treat them as "ratify D-QUAL2
first, then decide these in any order."

---

---

## Typestate — board card c71

### D-STATE1 — Order-of-events types: typestate (rec A)

**User story.** Fatima writes an e-commerce checkout. The invariant is: an `Order`
must be *charged* before it can be *shipped*. Today she enforces this with a
`require(order.is_charged)` at the top of `ship()` — a runtime check that fires in
production, not at compile time. She wants `ship(order)` to be a compile error
unless `order` has passed through `charge()`. She does not want to read a research
paper to achieve this.

Typestate = a tag that changes as a value moves through its lifecycle. A function
consumes one tag-state and returns the next. The tag lives only in sema; it erases
completely at runtime (no vtable, no enum discriminant, no overhead).

| Option | Compile-time guarantee | Runtime cost | Author ceremony | Failure error |
|--------|----------------------|-------------|-----------------|---------------|
| A — transitioning tags (RECOMMENDED) | yes — wrong-state call is a compile error | zero — tags erase | declare states + transitions; write `#[State]` on return type | clear: "expected `#charged`, found `#pending`" |
| B — runtime `require(…)` only | no — wrong-state call panics at runtime | `require` overhead | none | panic in production; message is a string |

**How other languages do this.**

- **Plaid** — the typestate research language; methods carry pre/post state annotations; the type checker verifies transitions; objects live in exactly one state at any time. The academic source for most of what Jet's D-STATE1 proposes; Jet simplifies by routing state through tags rather than separate type declarations.
- **Rust typestate pattern (phantom types)** — a common Rust idiom: `struct Connection<S>(PhantomData<S>)`; `fn open(c: Connection<Closed>) -> Connection<Open>`; state changes force a new type. Correct, but phantom types are invisible boilerplate and the pattern requires careful hand-threading. Jet's tag approach achieves the same guarantee without any phantom-type machinery.
- **Austral** — linear types enforce state protocols: a `Connection` value must be explicitly transitioned; the old state is consumed, the new one produced. Jet takeaway: consuming the old tag-state (`take`) and returning the new one is the right model; it maps directly onto Jet's `take` ownership keyword (S10).
- **Session types (process calculi / Haskell `session-types`)** — encode communication protocols in the type system; a channel has a type that steps with each send/receive. Jet takeaway: the session-types insight is that protocols are sequences of typed operations — typestate is exactly that idea applied to values, not channels.
- **TypeScript discriminated-union state machines** — `type Order = { status: "pending" } | { status: "charged" } | { status: "shipped" }`; a `ship` function takes only the `charged` variant. Works, but the state is a runtime enum discriminant (nonzero cost); the narrowing is done by the type checker reading the `status` field. Jet takeaway: the tag-based model achieves the same narrowing with zero runtime cost because the tag erases.
- **Ada/SPARK (pre/post conditions)** — `Pre => Order.Status = Charged`; verified statically by SPARK's prover. Jet takeaway: SPARK proves preconditions but the precondition is still a runtime value (`Status`); Jet's tag is stronger because the state is the type itself — there is no runtime field to check.

**Options.**

- **Option A — typestate via transitioning tags (RECOMMENDED).** States are tags. A function that *transitions* a value from one state to another takes the old state (consuming the value via `take`) and returns the new state. The tag on a binding tracks which state it is currently in; a call that requires a different state is a compile error naming the mismatch.

```jet
// state tags — plain tags, no methods
// (declared as tag constants; exact declaration syntax gated on D-QUAL2)
#tag Pending
#tag Charged
#tag Shipped

struct Order {
    id:    OrderId
    total: Float #unit(usd)
}

// transition: Pending → Charged
fn charge(take order: Order #[Pending]) -> Order #[Charged] ? {
    // ... call payment processor ...
    ok(order)          // order is returned with the Charged tag
}

// transition: Charged → Shipped
fn ship(take order: Order #[Charged]) -> Order #[Shipped] ? {
    // ... dispatch courier ...
    ok(order)
}

// correct lifecycle
fn checkout(take order: Order #[Pending]) -> String ? {
    charged  :: charge(order)?    // order: Order #[Charged]
    shipped  :: ship(charged)?    // charged: Order #[Shipped]
    ok("shipped: {shipped.id}")
}

// error — skipping charge
fn bad_checkout(take order: Order #[Pending]) -> String ? {
    ship(order)?
    // error[E0150]: state mismatch
    //  --> checkout.jet:23:5
    //   |
    // 23 |     ship(order)?
    //    |          ^^^^^ expected `Order #[Charged]`, found `Order #[Pending]`
    //    = note: `ship` requires the order to be in state `#[Charged]`
    //    = note: `order` is currently in state `#[Pending]`
    //    help: call `charge(order)` first to transition to `#[Charged]`
    ok("done")
}

// error — using the old binding after transition
fn stale(take order: Order #[Pending]) -> String ? {
    charged :: charge(order)?
    ship(order)?         // `order` was moved into `charge`; this is the old binding
    // error[E0031]: use of moved value `order`
    //  --> checkout.jet:35:10
    //   |
    // 33 |     charged :: charge(order)?
    //    |                       ----- `order` moved here
    // 35 |     ship(order)?
    //    |          ^^^^^ value used after move
    //    help: use `charged` (the transitioned value) instead
    ok("done")
}
```

- **Option B — runtime `require(…)` only.** No language change. The author adds a precondition check at the top of `ship`; the compiler does not enforce it.

```jet
struct Order {
    id:      OrderId
    total:   Float
    charged: Bool     // runtime flag — the thing typestate replaces
}

fn ship(view order: Order) -> String ? {
    require(order.charged, "order must be charged before shipping")
    // ... dispatch courier ...
    ok("shipped")
}

// compiles fine — crashes at runtime
fn bad_checkout(take order: Order) -> String ? {
    ship(view order)?    // order.charged is false — panic at runtime:
    // thread 'main' panicked at checkout.jet:10:
    // order must be charged before shipping
    ok("done")
}

// the bug class is alive; the compiler never sees it
```

**Recommendation:** A — the whole value of typestate is moving the bug class from
runtime to compile time; Option B is the status quo that typestate exists to
replace. Option B is listed only to make explicit what "no decision" means in
practice. Complexity sequencing: implement `#linear` (D-LIN1 Option A) first since
it exercises the same "track a tag on a value across branches" machinery; typestate
then adds the "tag changes on transition" layer on top. Both are gated on D-QUAL2.

<!-- foundation+misc: D-QUAL2, D-TXN1, D-MIGRATE1 + deferred -->

# Draft ballot cards — D-QUAL2, D-TXN1, D-MIGRATE1 + deferred stubs

> Status: draft — not yet promoted to `decision-ballots.md`.
> Date: 2026-06-20
>
> **Read order for owner:** D-QUAL2 first (foundational taxonomy); D-QUAL1
> (already in the open queue, board card c62) builds on whatever D-QUAL2
> ratifies and should be re-read in that light. D-TXN1 and D-MIGRATE1 are
> independent.

---

---

## Scoped transactions — board card c72

### D-TXN1 — Rollback semantics for `#transact { }` (rec A)

**User story.** Kai is writing a game action system. A single `use_ability`
call must spend stamina, apply a cooldown, and damage the target — or do none
of those things if any step fails. Today he writes a ladder of manual rollback
calls after each `?`. He misses one. The bug ships. He wants the compiler to
guarantee that a failed sequence is cleanly unwound without him hand-writing
the ladder.

> **Note on syntax.** The `#transact { }` scoped-region syntax is **already
> ratified** (S82 / D-ATTR1: `#Marker { }` is the scoped-effect form). This
> decision is about **rollback semantics** — what `#transact { }` actually
> does when a `?` propagates — not syntax. Do not re-open the surface.

| Option | What rolls back | Who writes rollback logic | Honest about limits? | After D-EFF1? |
|--------|----------------|--------------------------|----------------------|---------------|
| A — trait-declared rollback | types that impl `Rollback` | the type author, once | yes — only types that know how | natural sequencing (after D-EFF1) |
| B — library-only compensation | nothing (caller hand-writes) | every caller | technically honest; no language help | independent; always available |

- **Option A — `#transact { }` over types that declare `Rollback`. (RECOMMENDED)**
  A type opts into the transaction protocol by implementing the `Rollback`
  trait. Inside a `#transact { }` block, every `?`-failure triggers the
  reverse sequence: each step's `rollback` method is called in reverse order
  on the values that were mutated. On clean exit (no `?` propagation), the
  transaction commits — no rollback needed. The compiler tracks which values
  were mutated inside the block and synthesizes the reverse-call chain.

  This is honest: only operations on types that declare a rollback are
  covered. If you use a type that doesn't implement `Rollback` inside the
  block, sema tells you.

```jet
trait Rollback {
    fn rollback(mut self)
}

struct Stamina { current: Int, reserved: Int }

impl Stamina: Rollback {
    fn rollback(mut self) {
        self.current += self.reserved
        self.reserved = 0
    }
}

struct Cooldown { active: Bool }

impl Cooldown: Rollback {
    fn rollback(mut self) {
        self.active = false
    }
}

fn use_ability(player: mut Player, target: mut Enemy) -> Unit ? {
    #transact {
        player.stamina.spend(10)?   // if this fails: nothing to roll back yet
        player.cooldown.apply()?    // if this fails: rolls back stamina.spend
        target.hp.damage(25)?       // if this fails: rolls back cooldown + stamina
    }
    // all three succeeded — committed, no rollback
}
```

```jet
// Error: using a non-Rollback type inside #transact
struct Logger { entries: [String] }
// Logger does not impl Rollback

fn risky(logger: mut Logger) -> Unit ? {
    #transact {
        logger.entries.push("started")?
        //             ^^^^
        // error[E0801]: `Logger` does not implement `Rollback`; mutations
        //               inside `#transact` must be reversible.
        //   fix: impl Logger: Rollback { fn rollback(mut self) { … } }
        //        or move `logger.entries.push` outside the `#transact` block.
    }
}
```

  Natural sequencing note: `#transact` is an effect region (S82). After
  D-EFF1 ratifies the full effects model, rollback becomes a named effect
  that propagates through call sites like any other. Ratify A now as the
  semantic contract; the effect-system wiring follows D-EFF1.

- **Option B — Library-only manual compensation.** No language change. Every
  caller hand-writes the rollback ladder using `??` fallback arms. The `#transact`
  syntax is not used for rollback; it could still be used for other region
  semantics (locking, tracing), but rollback is purely caller responsibility.

```jet
fn use_ability(player: mut Player, target: mut Enemy) -> Unit ? {
    // hand-written compensation ladder — no language help
    player.stamina.spend(10) ?? {
        return err(Error.message("stamina failed"))
    }
    player.cooldown.apply() ?? {
        player.stamina.rollback()         // caller must remember this
        return err(Error.message("cooldown failed"))
    }
    target.hp.damage(25) ?? {
        player.cooldown.rollback()        // and this
        player.stamina.rollback()         // and this
        return err(Error.message("damage failed"))
    }
    return ok(())
}

// A new teammate adds a fourth step and forgets the rollback:
fn use_ability_v2(player: mut Player, target: mut Enemy) -> Unit ? {
    player.stamina.spend(10) ?? { return err(Error.message("stamina")) }
    player.cooldown.apply()  ?? { player.stamina.rollback(); return err(…) }
    target.hp.damage(25)     ?? { player.cooldown.rollback(); player.stamina.rollback(); return err(…) }
    emit_sound(player.sfx)?
    // no rollback for emit_sound — partial success shipped silently
}
```

  Zero language change. Leak-by-omission is the exact failure mode Option B
  accepts: every new step is a rollback the caller might forget.

**How other languages do this.**

| Language | Mechanism | Jet takeaway |
|----------|-----------|-------------|
| Haskell STM (`stm`) | `atomically` block over `TVar`s; the runtime retries on conflict; no partial state ever visible | Jet doesn't have shared mutable state across tasks (S53 deferred); STM's retry loop doesn't apply, but the "all-or-nothing block" idea does |
| Clojure `dosync` / refs | Software transactional memory; `alter`/`ref-set` inside a `dosync` block; retries on conflict | Same as STM — the retry model is for concurrent shared state; Jet's `#transact` is single-threaded sequential undo |
| Database ACID transactions | BEGIN / COMMIT / ROLLBACK; the DB engine tracks the undo log automatically | Jet's Option A is the same contract at the language level: each type declares its own undo; the block synthesizes the ROLLBACK call sequence |
| Saga pattern (microservices) | Each step publishes a compensating action; a saga orchestrator calls compensations in reverse on failure | Option A is a local, synchronous Saga: the `Rollback` trait *is* the compensating action; `#transact` *is* the orchestrator |
| Temporal Workflows | Compensations written as separate activities; the framework calls them on failure | More infrastructure, same idea; Jet's version is zero-framework, compiler-synthesized |

Jet's Option A is unusually explicit: types opt in, the rollback logic is
type-authored and auditable, and the compiler synthesizes the call sequence.
There is no hidden retry, no global undo log, and no runtime overhead outside
the `Rollback` calls themselves.

**Recommendation:** A — `#transact { }` over `Rollback`-implementing types.
The honesty is a feature: only operations whose authors have declared a
rollback are covered; everything else is a compile error telling you what to
fix. Option B is the status quo and the source of the bug Kai hit.

---

---

## Safe schema changes — board card c73

### D-MIGRATE1 — Compile-time enforcement of breaking data-shape changes (rec A)

**User story.** Dev team at a Jet shop ships a library with a public `UserRecord`
struct. Three months later, someone renames a field. Every consumer silently
recompiles, gets default-zero for the missing field, and ships corrupted data to
production before anyone notices. Sam, the library author, wants the compiler to
refuse the rename until he writes an explicit migration — the same guarantee a
database gives when you try to drop a column.

| Option | When is the break caught? | Who writes conversion? | Ignorable? | Needs recorded shape? |
|--------|--------------------------|----------------------|------------|----------------------|
| A — compile-time enforcement + conversion library | at compile time of the library change | the library author | no — it's a compile error | yes — a published shape must be snapshotted |
| B — lint/warn only | at compile time, advisory | nobody required to | yes — warnings are ignorable | no |

- **Option A — Compile-time enforcement: the CHECK is core; conversion is the
  Build-tier versioning library (#11). (RECOMMENDED)** When a type is marked
  `#[PublishedSchema]` (or equivalent), the compiler snapshots its field layout
  at release time and stores it alongside the package (in `.jet/cache/` or
  embedded in the artifact). On the next build, if the shape has changed in a
  breaking way (field removed, type changed, field renamed without migration),
  sema emits **E0901** naming the field and the published version. The author
  must either write a migration (using the Build-tier versioning library) or
  explicitly bump the major version with a breaking-change marker.

```jet
// pkg.jet — published type, shape is snapshotted at release
#PublishedSchema
struct UserRecord {
    id: Int,
    email: String,
    name: String,
}

// Later: rename `name` → `display_name` without a migration:
#PublishedSchema
struct UserRecord {
    id: Int,
    email: String,
    display_name: String,   // renamed from `name`
}
// error[E0901]: breaking change to published schema `UserRecord` (v0.4.0)
//   field `name: String` removed; `display_name: String` added
//   Consumers reading v0.4.0 data will get a missing-field error at runtime.
//   Options:
//     1. Write a migration: `migration UserRecord { rename name -> display_name }`
//     2. Bump the major version and mark this as a breaking release.
//     3. Keep the old field and deprecate it.
```

```jet
// With a migration — the compiler accepts the change:
migration UserRecord {
    rename name -> display_name
}

#PublishedSchema
struct UserRecord {
    id: Int,
    email: String,
    display_name: String,
}
// compiles: the migration tells consumers how to upgrade v0.4.0 → v0.5.0 data.
```

```jet
// Consumer side — reading old data with the new shape:
record :: UserRecord.from_v040(raw_bytes)?
// the versioning library generates `from_v040` from the migration chain;
// up/down conversion is the Build-tier library's job, not the compiler's.
```

  The compiler's job is the **check** — refuse a breaking shape change without
  a declared migration. The conversion functions (`from_v040`, `to_v040`) are
  generated by the Build-tier versioning library (#11), not by `sema` or
  `codegen` directly. This keeps codegen dumb (I3) and gives the library room
  to handle complex cases (field reorder, type coercion, default injection)
  without adding new compiler machinery for each.

- **Option B — Lint/warn only.** The compiler notices the structural change
  and emits a warning, but the build does not fail. A `jet fix` or `--allow`
  suppresses it.

```jet
// Same rename as above, no migration:
#PublishedSchema
struct UserRecord {
    id: Int,
    email: String,
    display_name: String,
}
// warning[W0901]: breaking change to published schema `UserRecord` (v0.4.0)
//   field `name: String` removed; `display_name: String` added
//   (use --allow schema-break to suppress)
```

  Warnings are suppressible. The one time you most want an unbreakable
  guarantee — public wire formats — is the one time a warning is ignored under
  release deadline pressure. Option B is the database world's equivalent of a
  migration framework you can opt out of: it exists, and the bugs still ship.

**How other languages do this.**

| Language | Mechanism | Jet takeaway |
|----------|-----------|-------------|
| Protocol Buffers | Field numbers + `reserved` keyword; removing a field number is a protocol error at decode time | Runtime check, not compile-time; Jet catches it earlier. The "number-is-identity, name-is-docs" rule is worth considering for the migration syntax |
| Apache Avro | Reader/writer schema resolution at decode time; missing optional fields get defaults | Runtime resolution, not compile-time; Jet's check is stronger. Avro's reader/writer schema pair is the direct analog of Jet's published-vs-current shape diff |
| Rust + serde | No built-in schema versioning; authors use `#[serde(rename = "…")]` and hope; `serde_versioning` crates exist but are optional | Jet enforces what Rust leaves as convention; the `migration` block is the `#[serde(rename)]` that the compiler requires |
| Elm records | The compiler checks record type compatibility structurally; a renamed field is a type error at every call site, found immediately | Elm catches breaks locally (within a codebase); Jet's `#PublishedSchema` catches breaks at the library boundary, where Elm's type system stops |
| Flyway / Alembic (database migration frameworks) | Migration scripts versioned and applied in order; the framework refuses to run if migrations are missing | The exact model Jet's option A adopts at the language level: migration = required, ordered, tracked. Jet makes it a compile error; Flyway makes it a deploy error |
| Ecto migrations (Elixir) | Migrations are first-class modules; `mix ecto.migrate` fails if the schema is ahead of migrations | Same as Flyway; Jet's version is language-native, not a deploy-time CLI |

Jet's Option A is the strongest guarantee in this table: it is a **compile
error**, not a runtime decode error (Avro), a type error within a codebase
(Elm), or a deploy-time failure (Flyway/Ecto). That strength comes at a cost:
a published shape must be snapshotted and stored so the compiler can diff it.
The `.jet/cache/` store is the natural home.

**Recommendation:** A — compile-time enforcement is the only form of this
guarantee that cannot be silenced by a deadline. The conversion library (#11)
handles up/down migration logic without burdening the compiler; sema's job is
exactly the check (I3). Option B is a lint, and lints get suppressed.

---

---

## Deferred ballots — promote when reached

The items below are not ready for owner decision. Each has a real user story
and a clear reason to wait. Promote a stub to a full card when its
prerequisite is ratified or its milestone is reached.

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

## Smart Context — board card c74

### D-CTX1 — Smart Context: an implicit allocator+logger bundle threaded through every call (rec A2 + Cβ)

There are **two coupled questions** here. The owner must answer both:

- **Q1 (the S58 question, dominant):** does an implicit context *replace*, *complement*,
  or get *rejected* against the ratified explicit-allocator stance?
- **Q2 (syntax):** if it ships, how is a per-block swap spelled?

**User story.** Mia, four weeks into programming, writes a program that builds a big list
of records and prints a report. She never types the word "allocator" — she has never
heard it. Her code runs, is memory-safe, and frees everything at scope end. Later, Dev, an
embedded engineer on the same team, needs that exact report-building function to run
against a fixed 64 KB arena with no heap and a silent logger — *without editing the
function*. He wraps the call in one block that swaps the context, and every allocation and
log inside (including in library code he didn't write) reroutes. Mia's source is
untouched and still reads like nothing happened. That "swap once at the top, everything
downstream follows, restores on exit" is the whole feature — and it is exactly the power
S58 deliberately made *explicit and visible* instead.

#### The S58 tension (read this first)

S58 ratified, verbatim: *"explicit Zig-style allocators — allocating APIs take an
allocator parameter; a fixed arena works on embedded."* D-ALLOC1 then ratified the
spelling `arena :: mem.Arena.new()` / `node :: arena.alloc(value)`. The whole point of
that line was **the allocator is a visible parameter you pass**.

Smart Context is the opposite move: the allocator becomes an **implicit, invisible**
value threaded through the call graph, so `alloc` finds it without anyone passing it. That
is genuinely useful (it is *why* beginners never see memory), but it **partly reverses
S58's explicit stance**. The two designs answer the same question — "where does an
allocating function get its allocator?" — with opposite answers. They cannot both be the
default. So Q1 is not a nicety; it is the gate.

| | Where `alloc` gets its allocator | Beginner sees | Expert control | S58 status |
|---|---|---|---|---|
| **S58 today (explicit)** | a parameter the caller passes | passes/sees the allocator (or a defaulted one) | total, local, visible | as ratified |
| **Context replace** | the implicit context, always | nothing | swap a block | **reverses S58** |
| **Context complement** | context **unless** an explicit allocator is passed | nothing | pass param *or* swap block | **extends S58, keeps it valid** |
| **Reject** | a parameter the caller passes | the allocator | total, local, visible | unchanged |

#### How other languages do this

- **Jai — `context` + `push_context`.** A hidden `context` (allocator, logger, …) is
  passed into every call; `push_context new_ctx { … }` swaps it for a block and restores
  on exit; library code transparently picks up the new allocator. This is the direct
  ancestor of the proposal. *Jet takeaway:* this is exactly the ergonomic we want — adopt
  the block-scoped swap-and-restore shape.
- **Odin — implicit `context`.** Every scope has an implicit `context` passed by pointer
  on each Odin-convention call; `new(T)` uses `context.allocator` unless overridden;
  **copy-on-write** so a callee can't back-propagate a bad context to the caller. Built for
  *intercepting third-party code's* allocation/logging. *Jet takeaway:* steal the
  copy-on-write / per-scope-local guarantee — a swap inside a block must never leak
  outward, which also gives us the auto-restore for free.
- **Go — `context.Context` (explicit, the contrast case).** Go threads context as an
  *explicit first parameter* (`func F(ctx context.Context, …)`) and the community treats
  invisible/implicit context as an anti-pattern. *Jet takeaway:* this is the cautionary
  twin — Go chose visibility and ceremony on purpose; it shows the cost of *not* hiding it
  (every signature grows a param) and the benefit (no magic). It is essentially "the
  Reject option, productized."
- **Scheme / Racket — `parameterize`.** Dynamic parameters (`make-parameter`) hold values
  looked up dynamically; `(parameterize ([p v]) body)` rebinds for the dynamic extent of
  `body` and restores after. *Jet takeaway:* the precise semantics we want are
  *dynamic-extent* rebinding, not lexical — proves the swap-restore model is a 40-year-old,
  well-understood construct, not a novelty.
- **Thread-locals (C/C++/Rust `thread_local!`).** A per-thread global the callee reads
  without a parameter. *Jet takeaway:* the likely **codegen** substrate for the implicit
  value — but a leaky mental model for users, so it stays a backend detail and is *never*
  surfaced (mirrors S58's "onboarding never mentions any of it").

#### Q1 options — REPLACE vs COMPLEMENT vs REJECT (the S58 interaction)

- **Option A1 — Replace.** Context becomes *the* way allocators are found; S58's
  explicit-parameter line is superseded. Allocating APIs no longer take an allocator
  parameter — they read the context.

  ```jet
  // Library function — note: NO allocator parameter anymore.
  fn build_report(rows: [Row]) -> [Line] {
      out :: []                 // allocates from the implicit context
      loop r in rows { out.push(format(r)) }
      out
  }
  ```

  Pro: maximally beginner-clean, one mechanism. Con: **directly reverses S58 and
  D-ALLOC1's "alloc is a visible method on a named arena"** — the embedded story
  "a fixed arena works because you pass it" evaporates into invisible threading; experts
  lose the local, visible control S58 promised. Violates the simplicity ratchet by
  *removing* an already-shipped explicit path.

- **Option A2 — Complement (recommended).** Explicit S58/D-ALLOC1 allocator-passing stays
  exactly as ratified and **wins when present**; the implicit context is only the
  **default used when no allocator is passed explicitly**. Nothing about S58 is reversed —
  context fills the hole S58 already had (beginners weren't passing allocators anyway; the
  default heap allocator simply *becomes nameable and swappable*).

  ```jet
  arena :: mem.Arena.new(capacity: 65536)   // S58 / D-ALLOC1, unchanged

  // Explicit wins — exactly S58 today:
  node :: arena.alloc(value)

  // Implicit default — beginner path, fed by the context:
  list :: []                                // uses context.allocator

  // Expert swaps the *default* for a block; explicit calls still override locally:
  using context.allocator = arena {
      report :: build_report(rows)          // build_report's internal allocs -> arena
  }                                         // context restored here
  ```

  Pro: **add-only** — S58 and D-ALLOC1 keep their exact meaning; the explicit parameter is
  still the override and still the embedded story. Beginners get the magic; experts get
  *both* knobs (pass a param for one call, swap the block for a subtree). Con: two ways to
  pick an allocator coexist (mitigated: explicit always wins, one precedence rule, easy to
  teach — "passed beats ambient").

- **Option A3 — Reject.** No implicit context. Allocators stay strictly explicit (S58 as
  is). Loggers, if wanted, are an ordinary passed value or a plain module-level function.

  ```jet
  fn build_report(rows: [Row], in: mem.Allocator) -> [Line] { … }   // S58 forever
  ```

  Pro: zero new magic, S58 fully intact, simplicity ratchet satisfied by *not* adding a
  feature. Con: the beginner "never see an allocator" story still leans on a single hidden
  default that nobody can swap; no clean seam to later carry capabilities/effects (c06 /
  D-EFF1) — we'd reinvent this carrier when effects land.

#### Q2 options — per-block swap syntax (only if A1 or A2 wins)

- **Option Cα — `using context.allocator = arena { … }`.** Jai/`using`-flavored; reads as
  prose, names the exact field being swapped.

  ```jet
  using context.allocator = arena {
      report :: build_report(rows)          // arena is the ambient allocator in here
      log.info("built {report.len()} lines") // still the outer logger
  }                                          // allocator auto-restored on exit
  ```

  Con: `using` collides conceptually with S62's rejected "Jai-style `using` member
  injection" — reusing the word the owner already declined elsewhere is a trap.

- **Option Cβ — `#context(allocator = arena) { … }` (recommended).** A `#` marker block
  (consistent with D-ATTR1's `#unsafe`/`#audit`), naming swapped fields as `field = value`;
  multiple fields comma-separated (D-ATTR2 list feel). Auto-restores on block exit.

  ```jet
  silent :: log.Silent.new()
  #context(allocator = arena, logger = silent) {
      report :: build_report(rows)          // arena + silent logger flow downstream
  }                                          // BOTH fields restored here
  ```

  Pro: rides the **already-ratified `#` marker grammar** — no new top-level keyword, no
  collision with `using` (S62) or `use` (S16); the marker form signals "compiler-managed,
  scoped" exactly like `#unsafe`. Con: a `#(…)` block is a slightly heavier read than bare
  `using`.

- **Option Cγ — `push_context my_ctx { … }`.** Jai-literal: build a whole context value,
  push it.

  ```jet
  my_ctx :: context.with(allocator = arena, logger = silent)
  push_context my_ctx { report :: build_report(rows) }
  ```

  Pro: closest to the prior art, swaps the whole bundle at once. Con: a new top-level
  keyword (`push_context`) for a niche expert op — fails the keyword-budget bar; forces
  users to name a context value even for a one-field swap.

#### Recommendation

**A2 (Complement) + Cβ (`#context(…) { … }`).** A2 is the only option that **does not
reverse a ratified call**: S58 and D-ALLOC1 keep their exact meaning, the explicit
allocator parameter stays the override and the embedded story, and the implicit context
merely makes the *already-hidden default* nameable and swappable — pure add. Precedence is
one sentence: **a passed allocator always beats the ambient one.** Cβ reuses the ratified
`#` marker grammar, dodges the `using` (S62) and `push_context` (keyword-budget) traps, and
the scoped marker form reads as "compiler-managed, restores on exit." v1 holds the bundle
to **allocator + logger only**; the context is the natural future carrier for c06
capabilities and D-EFF1 effects, but that expansion is explicitly out of scope here and
must come back as its own card. **Reject A1** — replacing S58 trades a shipped, visible,
teachable expert path for invisible threading. If the owner wants zero new magic, **A3** is
the clean no; everything beginner-facing still works, we just never get the swap seam.

**Stop-work:** Smart Context implementation is blocked until D-CTX1 (Q1 at minimum) is
decided.

---

Sources (prior-art verification):
- Jai context / `push_context`: [The Way to Jai — Context](https://github.com/Ivo-Balbaert/The_Way_to_Jai/blob/main/book/25A_Context.md), [Jai Community wiki — Context](https://jai.community/t/context/163)
- Odin implicit `context`: [gingerBill — Odin's Most Misunderstood Feature: context](https://www.gingerbill.org/article/2025/12/15/odins-most-misunderstood-feature-context/), [Odin overview](https://odin-lang.org/docs/overview/)
- Go `context.Context`, Racket `parameterize`, thread-locals: standard language docs.

---

## Build-time I/O at comptime — board card c75

### D-CTIO1 — Gated build-time I/O at comptime (rec B)

> **Update (verified 2026-06-21): `embed_file` is already implemented and working** — `comptime x = embed_file("path")` reads the file at compile time and bakes it in (eval_embed_file, E0955, purity-allowed). So this decision is NARROWER than framed below: Option A’s “embed_file unimplemented” is moot. Live questions: (1) add `embed_bytes` for binary alongside the working `embed_file`? (2) harden path-escape rules (literal-path, no `..`)? (3) allow broader build-time I/O beyond embed_file (rec: **no**). Option B is largely shipped for text — ratify `embed_bytes` + the path rules.

Jet's comptime engine is already ratified and partly shipped: S26 (the comptime law — value-only, no macros, no comptime types), S57 (`comptime x = …` bindings), S60 (Layer 2 — compile-time pure evaluation + data embedding), D-PURE2 (no ambient I/O; `embed_file` the one named exception), D-WHEN1/2 (`comptime if`, shipped). So this ballot does **not** re-decide comptime. The one unresolved question: **should Jet permit build-time I/O beyond `embed_file`?** Jai's `#run` allows full filesystem access at compile time — a supply-chain risk Jet's S26 law was written to refuse. This card settles the policy boundary.

**User story.** Dana is shipping a graphics tool. She wants a WGSL shader (and a root cert, a JSON schema) baked into the binary as a constant at compile time — without a separate build script, and without opening `jet build` to arbitrary code execution from a dependency.

| | A — pure-only forever | B — ratify `embed_file`/`embed_bytes` | C — broad gated build I/O |
|---|---|---|---|
| Supply-chain risk | none | minimal (read-only, path-checked) | high un-audited; moderate gated |
| Power | lowest | covers ~90% of embed needs | full (env, network, codegen) |
| Consistency w/ S26 law | perfect | good (S26 already names `embed_file`) | strained |
| Ratchet (I8) cost | none | small (two builtins) | high (new gate, lockfile, sandbox) |
| Prior-art twin | — | Zig `@embedFile`, Rust `include_str!` | Jai `#run`, Nim `staticExec` |

**How other languages do this.**
- **Zig** — `@embedFile("path")` bakes a file's bytes in; no general comptime I/O (a compile error). The cleanest precedent for B — takeaway: a dedicated embed builtin, not an I/O grant.
- **Jai** — `#run fn()` runs *anything* at compile time, including filesystem/network/process spawn. A buggy dep can read `~/.ssh` during `jai build`. Takeaway: this is exactly the model S26 refuses.
- **Rust** — `include_str!`/`include_bytes!` are embed-only; arbitrary build execution is isolated to a separate `build.rs`. Takeaway: safe embed built-in, dangerous execution quarantined to a distinct, visible mechanism.
- **D** — CTFE over a pure subset; `import("file")` is the sole file-read intrinsic. Takeaway: even a powerful comptime keeps I/O to one named read.
- **Nim** — `staticRead` (safe embed) *and* `staticExec` (shell at compile time). Takeaway: `staticExec` is the footgun that spreads through packages once it exists — the cautionary tale against C.

- **Option A — keep pure-only forever.** No build-time I/O at all; `embed_file` stays unimplemented. Assets embed via a separate codegen step or are read at runtime.

  ```jet
  comptime shader :: read_file("shaders/main.wgsl")  // error: I/O not allowed in comptime; use a build step or core.fs at runtime
  ```
  Safest and simplest (I8 favors it), but forfeits an ergonomic win the spec already blessed and forces a separate build step for every embedded asset.

- **Option B — ratify `embed_file` / `embed_bytes` (recommended).** Ship the read-only builtins S26/D-PURE2 already name: `embed_file(path) -> String`, `embed_bytes(path) -> [U8]`. Path must be a string literal, resolved relative to the source file, no `..`-escape past the project root. Not new I/O capability — it implements the blessed exception.

  ```jet
  comptime shader_src :: embed_file("shaders/main.wgsl")   // String, baked into the binary
  comptime cert_der   :: embed_bytes("certs/root.der")     // [U8]

  comptime bad :: embed_file(build_path())          // error: path must be a string literal
  comptime esc :: embed_file("../../etc/passwd")    // error: path escapes the project root
  ```

- **Option C — broad gated build-time I/O.** Allow arbitrary comptime functions to do I/O when explicitly gated with a visible audit marker (mirroring the S58 `#audit`/`#unsafe` model).

  ```jet
  #audit("reads the local package list at build time — no network, no secrets")
  comptime pkgs :: #run(io) {
      core.fs.read("local-packages.txt").lines().filter((l) => l.len() > 0)
  }
  ```
  Sandboxed subprocess, an auditable `.jet/build-io.lock` of accessed paths, cache-invalidation on change. Powerful, but a new marker + lockfile + sandbox — heavy against the ratchet, and the Nim/Jai evidence shows un-auditable spread once shipped.

**Recommendation:** **B** — it's the answer Zig and Rust already prove safe at scale, it's what S26/D-PURE2/S60 already committed to (so this is an implementation/surface ratification, not a policy change), and it closes the door on C's supply-chain class. Owner sign-off questions: (1) `embed_bytes` in scope or embed-as-`String` only? (2) does the path restriction get its own diagnostic code? (3) does `embed_file` ride S60 Layer 2's milestone or get its own slot?

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

### D-SOA1 — Cache-friendly data layout (SOA) (rec A, deferred to Later)

**Tier: Later / deferred.** This decision is ballot-ready but implementation is
deferred until after v1. The owner's vote locks in the syntax now so the feature can
be planned against a fixed spelling.

**User story.** Dev is writing a particle system that updates 100 000 `Particle`
records per frame. Profiling shows cache misses dominate: the default array-of-structs
(AOS) layout loads the `x`, `y`, `z`, and `color` fields of one particle into a cache
line even when the update loop only touches `x`, `y`, `z`. He wants
structure-of-arrays (SOA) layout — one contiguous array per field — without rewriting
every access as `particles_x[i]`, `particles_y[i]`, `particles_z[i]`.

| Option | Spelling | Annotation site | Field-access change? | Ceremony | Composability |
|--------|----------|-----------------|----------------------|----------|----------------|
| A | `#layout(soa) struct Particle { … }` | type definition | none — `p.x` still works | low | layout is part of the type; composable with `#Serialize` etc. |
| B | `particles: soa [Particle]` | variable declaration | none — `p.x` still works | low | layout is per-container; same type can be AOS in one place, SOA in another |

**How other languages do this**

- **Jai (`#place` / `using`):** Jai lets you embed one struct inside another with
  `#place` to force field co-location; SOA is built into the language's array
  primitives. No single annotation; requires structural knowledge of the layout
  system. Jet takeaway: a single annotation is the right UX; the compiler does the
  structural transformation, not the user.
- **Zig (`MultiArrayList`):** `std.MultiArrayList(T)` is a stdlib type that stores
  fields in separate arrays; access is via `.items(.field_name)`, breaking normal
  field syntax. Jet takeaway: field syntax must stay identical; a compile-time
  transform that preserves `p.x` is the goal.
- **Rust (`soa-derive` / `slotmap`):** The `soa-derive` crate generates a parallel
  struct via a procedural macro; `slotmap` provides SOA slots. Both require
  importing a crate and annotating the struct. Jet takeaway: the annotation-on-type
  shape is familiar from Rust macros; a built-in transform avoids external crate
  dependency (I6).
- **ISPC / data-oriented design (manual):** ISPC's `soa<N> T` type declaration
  generates SOA layout for SIMD; elsewhere, data-oriented design achieves SOA by
  hand — splitting one `struct Particle` into multiple parallel arrays. Jet
  takeaway: a compiler-managed transform is superior to manual splitting; the ISPC
  `soa<N>` shape (annotation on the type) confirms the Option A position.
- **Unity DOTS (`[StructLayout]` / `IComponentData`):** Unity's ECS requires
  implementing `IComponentData` and relies on the runtime's archetype system;
  the developer does not choose AOS vs SOA directly. Jet takeaway: the decision
  should be explicit and developer-controlled, not hidden in a runtime framework.

- **Option A — `#layout(soa)` on the struct.**

```jet
#layout(soa)
struct Particle {
    x: Float
    y: Float
    z: Float
    color: U32
}

fn update(particles: mut [Particle]) {
    loop p in particles {
        p.x += p.velocity_x   // field access unchanged
        p.y += p.velocity_y
    }
}
```

The type carries its layout. Any `[Particle]` collection is automatically SOA; the
caller does not need to know. Mixing AOS and SOA `Particle` values in the same
collection is a type error (they are the same nominal type, so sema must track the
layout tag).

*Partial-SOA variant (open question — recommend deferring):*

```jet
#layout(soa: x, y, z)   // only hot fields go SOA; color stays interleaved
struct Particle { … }
// Complexity: the field-access lowering for the cold fields differs.
// Recommend: whole-struct only for v1.
```

- **Option B — `soa` keyword on the container.**

```jet
struct Particle {
    x: Float
    y: Float
    z: Float
    color: U32
}

fn update(particles: mut soa [Particle]) {
    loop p in particles {
        p.x += p.velocity_x   // field access unchanged
        p.y += p.velocity_y
    }
}
```

The layout is per-collection. A `[Particle]` passed to a non-`soa` function is a
type mismatch; the caller must decide the layout. This is more flexible (same type,
two layouts) but surfaces the layout decision to every call site.

**Recommendation:** A — `#layout(soa)` on the type is consistent with the `#`
attribute system (D-ATTR1, ratified) and keeps the layout decision at the definition,
not scattered across every call site. The tradeoff (one layout per type in v1) is
acceptable for the common case; partial SOA and per-container layout are open
questions for a later revision. Defer implementation until after v1; ratify the
syntax now so plans can be written against a fixed spelling.

**Open questions for the owner:**
1. Whole-struct SOA only in v1 (recommended), or support `#layout(soa: field, …)`
   partial annotation?
2. Should `soa` [Particle] (Option B) be a future-reserved spelling even if A is
   chosen, to enable per-container overrides later?
3. Interaction with `#Serialize` and reflection: does SOA layout affect the
   serialized representation?

---

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
