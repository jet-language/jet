# P1 — The qualifier system: traits, attributes, tags

**Status:** idea / proposal (not a plan). **Routes:** every "consider as
trait/attribute/tag" note from the scratchpad lands here.

This report fixes one boundary and then classifies four feature ideas against
it. The boundary is the deliverable the owner asked for; the four ideas are
worked examples that prove the boundary holds.

---

## 0. Glossary

- **Qualifier** — umbrella word for anything you attach to code or a value to
  say something extra about it. Traits, attributes, and tags are the three
  kinds. (Working term; naming menu in §6.)
- **Propagate** — a property that flows along call edges or data edges: if `f`
  calls `g` and `g` has property P, `f` has P unless it discharges it.
- **Discharge** — satisfy or remove a propagated obligation: grant a
  capability, prove a bound, handle an uncertainty.
- **Lattice** — an ordering where every two items have a defined "higher of
  the two" (e.g. `experimental < tested < hardened`).

---

## 1. The boundary (the one primary note)

Jet already has two of the three. The proposal is to name the third precisely
so it stops leaking into the other two.

| Kind | One-line definition | Carries behavior? | Local or propagating? | Today |
|---|---|---|---|---|
| **Trait** | A named capability *with method bodies a type provides*. | Yes — method signatures. | Local to the `impl`. | Ratified (S28/S48). |
| **Attribute** | A *directive to the compiler/toolchain*, consumed at one declaration or region. | No — triggers machinery. | Local, one site. | Ratified (S82 as amended by D-ATTR1, `#`). |
| **Tag** | A *property that propagates and is checked relationally* — "may/​must-not depend," "must discharge before use." | No. | Propagates along edges. | First instance ratified (**S60 `pure fn`**); this proposal **names and generalizes** the kind. |

**The decision rule** (this is the part to ratify):

```
Does it define behavior a TYPE provides (methods)?          → Trait
Is it a one-shot directive consumed at its own site?        → Attribute
Does it propagate along call/data edges and get checked
    by a relational rule (X may not depend on Y, or
    "discharge before use")?                                → Tag
```

Three tells that something is a **tag**, not an attribute:

1. **It flows.** Putting it on `g` affects every `f` that calls `g`.
2. **It is checked between two things,** not at one thing: "hardened may not
   call experimental," "this value is uncertain *until* you handle it."
3. **It has no payload the compiler executes** — it is a predicate, not a
   request to run a derive/codegen pass.

### Jet already has a tag — `pure fn`

The third kind is not invented here; its first instance is ratified. **`pure
fn` (S60) is a tag spelled as a keyword.** Purity *propagates* (a `pure fn`
may call only pure fns), is *checked relationally* (the error names the impure
call path), and carries *no codegen payload* — the three tells from §1. It
reads like a modifier; it behaves like a tag. This proposal's job is to
recognize that kind by name so future qualifiers (maturity, capabilities,
uncertainty) reuse its rule and its machinery (§5) instead of each inventing
their own.

### The trap to call out

Syntax ≠ semantic kind. `pure fn` is a keyword; `#net` would be a sigil; both
are tags. A tag may also *look* like an attribute — you might write `#net` the
same way you write `#test`. The difference is what the compiler does next:
`#test` is consumed where it sits; `#net` propagates to every caller and is
checked against a grant. **The taxonomy classifies the semantic role, not the
sigil.** Whether tags get their own sigil (and their *position* — prefix line
vs. trailing in a signature) is an open decision (§7, Q1) — but the spelling
must not blur the three roles in the diagnostics or the mental model.

### The hardest case — `#unsafe` is dual-faced (and that's the point)

`#unsafe` is the case a careful reader will test the boundary against, because
it wears both faces, and seeing why settles the rule rather than breaking it:

- **`#unsafe { … }` region** — an **attribute**: a directive consumed where it
  sits, marking a block whose body may use gated operations. One site, no
  propagation.
- **`#unsafe fn` contract** — a **tag**: per S58/D-LL2, calling an `#unsafe fn`
  *requires an enclosing `#unsafe` context*. That obligation propagates up the
  call graph and is checked relationally — the three tells.

So `#unsafe` is not a counterexample to the rule; it is the sharpest proof of
"syntax ≠ semantic kind." One sigil, two roles, disambiguated exactly as §1
disambiguates: by whether the thing propagates.

It also shows tags propagate in **both directions**, and the rule covers both:

| Tag | Direction | Constraint |
|---|---|---|
| `pure fn` (S60) | downward (callees) | a pure fn **may not call** an impure one |
| `#unsafe fn` (S58) | upward (callers) | an unsafe fn **may only be called from** an unsafe context |

"Propagates along call edges and is checked relationally" holds for both; the
*direction* is part of each policy, not of the taxonomy.

---

## 2. Idea A — Maturity tags (`experimental` / `tested` / `hardened`)

> *Scratchpad:* "Code tags: experimental, tested, or hardened — restricts
> hardened code from silently depending on experimental code."

A three-level lattice. The rule: **trust never flows downhill silently.**
Hardened code may not depend on tested or experimental code without an
explicit, visible acknowledgement.

```jet
#hardened
fn settle_payment(order: Order) -> Receipt { ... }

#experimental
fn estimate_fraud(order: Order) -> Float { ... }
```

If `settle_payment` calls `estimate_fraud`, the compiler stops:

```
Error [E07xx]: hardened code depends on experimental code
  ┌─ billing.jet:3:5
  │
3 │     let risk = estimate_fraud(order);
  │                ^^^^^^^^^^^^^^^ `estimate_fraud` is #experimental
  │
Why: `settle_payment` is #hardened — it must not silently rely on code that
     may still change or be wrong.
Fix: harden `estimate_fraud`, or opt in explicitly at the call site with
     `#trusting(experimental) { let risk = estimate_fraud(order); }`
```
*(Diagnostic codes here are illustrative; real codes get assigned in
diagnostics.md, which pins the `Error [E####]` / `Why:` / `Fix:` format.)*

This is a **tag** by the rule: it propagates (a function is as trustworthy as
its weakest call) and is checked relationally (hardened vs experimental).

Tradeoffs:

| For | Against |
|---|---|
| Trust boundaries become a compile error, not a code-review hope. | A fourth thing to keep updated as code matures. |
| Refactors can't quietly pull unstable code into a stable path. | The lattice needs a default; unmarked code has to mean *something*. |
| Pairs with the package story — a registry can show a crate's maturity. | Over-tagging is noise; needs a light default and inference. |

---

## 3. Idea B — Capabilities and prohibitions

> *Scratchpad:* effects/capabilities so a signature tells you everything a
> function can do; negative-space types — declare what code must never do.

> **Reopens a closed decision.** S60 (`pure fn`) explicitly *rejected* a "full
> effects system." This idea is that effects system. It must be brought as an
> **owner-gated reopening of S60's rejection**, not as a fresh feature — and
> the case for reopening is precisely that `pure fn` already proved the
> machinery works for one effect (purity); capabilities generalize it.

These are one mechanism with two signs. A **capability** says what code *may*
do (network, filesystem, clock, randomness). A **prohibition** says what code
*must not* do (allocate, block, log PII). Both propagate; both are tags.
Purity (S60) is the bottom of this lattice: a `pure fn` is one with the empty
capability set.

```jet
fn fetch(url: Url) #net -> Bytes          // grants the net capability
fn parse(b: Bytes) -> Config              // no capability — like pure fn
fn handle_request(r: Request) #net #fs -> Response   // touches network + disk
```

*(Tag **position** above — written before the `->` — is illustrative only.
S82/D-ATTR1 today sanction the prefix line and `#[…]` forms; where a
propagating tag sits on a signature is open syntax, see §7 Q1.)*

A caller inherits the union of its callees' capabilities unless it sits at a
boundary that grants them. The headline value: **you trust a dependency by
reading its signature, not by auditing its source.**

```
Error [E07xx]: this function may not touch the network
  ┌─ render.jet:8:14
  │
1 │ #no(net)
  │ ~~~~~~~~ `render_email` is forbidden from network access
  ⋮
8 │     let logo = fetch(brand.logo_url);
  │                ^^^^^ `fetch` requires #net
  │
Why: a #no(net) function must not call a #net function.
Fix: pass the bytes in as a parameter, or lift the fetch to the caller.
```

Prohibitions are the negative-space idea: `#no(alloc)`, `#no(block)`,
`#no(pii)`. The PII case needs a companion value-tag (see §4 — uncertainty's
"untrusted" axis is the same machinery pointed at data provenance).

Tradeoffs:

| For | Against |
|---|---|
| Supply-chain-attack-resistant: capabilities are visible and bounded. | Capability inference must be near-total or every signature gets noisy. |
| Beginners get safety for free if the *default* is "no ambient capability." | A default of "no capability" can make hello-world print need a grant. |
| Prohibitions match how engineers reason about risk (red lines). | Effect systems are a known usability cliff (Koka, Eff) — must stay invisible until needed (priority #2). |

This is the biggest of the four and the one most in tension with priority #2
(beginner experience). The likely resolution mirrors C1's progressive
disclosure: capabilities are **inferred and silent** in Tier 1, and only
become visible syntax when an expert writes a boundary (`#no(...)`) or a
package declares its surface.

---

## 4. Idea C — Tracked value dimensions (uncertainty, cost)

> *Scratchpad:* uncertainty as a pervasive type dimension; cost/resource
> types.

The maturity/capability tags ride on **code**. These two ride on **values**,
but they are the same kind by the rule: a propagating, must-discharge
property.

**Uncertainty** — an axis on a value: "might be null," "came from untrusted
input," "possibly stale," "estimate ±5%." It propagates through expressions
and must be discharged before the value is used where it matters.

```jet
let age: Int?untrusted = form.field("age").to_int();   // untrusted + maybe-absent
// using it raw is an error:
charge_adult_rate(age);   // error: `age` is #untrusted and possibly absent

when age {
    some(a) if a >= 18 -> charge_adult_rate(a);   // discharged: present + checked
    _ -> reject();
}
```

The whole class of "I assumed this was fresh/clean/present" bugs becomes a
type error. Jet already has `Option` for the present/absent axis (M3) — this
generalizes the *idea* to other axes (trust, freshness, precision) rather than
minting a new `Option`-like type per axis.

**Cost/resource** — the type tracks a budget: time complexity, allocation,
latency. Exceeding it is a compile error.

```jet
#budget(latency: 10ms)
fn on_keystroke(e: Key) -> Edit { ... }   // compile error if a callee blows 10ms
```

Tradeoffs:

| For | Against |
|---|---|
| Uncertainty kills a whole bug category at compile time. | Cost types are research-fringe; sound static cost is genuinely hard. |
| Reuses the propagation engine the capability tags need. | Too many value axes = type annotations balloon; needs heavy inference. |
| `±5%` / freshness has no good prior art in a mainstream language — real differentiation. | Risk of a "type tetris" feel that violates priority #2 if not defaulted well. |

Honest call: **uncertainty is promising and mostly mechanism-shared with
capabilities; cost types are the riskiest idea in the whole scratchpad** and
should probably be scoped to a narrow, opt-in expert form (latency budgets on
real-time handlers) rather than a pervasive complexity axis.

---

## 5. Why these four belong together

All four are *tags* by the §1 rule, so they share one engine:

- a **propagation pass** in sema that flows properties along the call graph
  (capabilities, maturity) and the dataflow graph (uncertainty, cost);
- a **relational check** ("may-not-depend," "discharge-before-use");
- a **diagnostics family** that explains the violation in the §1–§4 voice.

That engine already exists in embryo: the **S60 `pure fn` checker** is exactly
a call-graph propagation plus relational check (it names the impure call
path). Generalize *it* — purity is the empty-capability point of the
capability lattice — rather than standing up a second, parallel propagation
pass; a duplicate engine would fight I8. Build the one engine once and the four
ideas become policies on top of it. That is the argument for one qualifier
system rather than four bolt-ons — and for nailing the taxonomy (§1) before any
of them.

---

## 6. Naming menu (working terms are placeholders)

The umbrella term "qualifier" and the verb "tag" are placeholders. Aviation /
jet-themed candidates, owner to pick or reject:

- **Umbrella for the third kind:** *tag*, *mark*, *band*, *grade*, *stripe*,
  *placard*, *rating*, *cert* (as in airworthiness certificate), *manifest*.
- **Maturity levels:** `experimental`/`tested`/`hardened` (scratchpad) ·
  `prototype`/`proven`/`certified` · `draft`/`flown`/`airworthy` ·
  `hangar`/`testflight`/`production`.
- **Capability grant verb:** `#net` (bare) · `#grant(net)` · `#uses(net)` ·
  `#needs(net)`.
- **Prohibition:** `#no(pii)` · `#never(block)` · `#forbid(alloc)` ·
  `#groundnot(...)`.

(Per house style: these are starting points, not a final pick.)

---

## 7. Open decisions for the owner (future ballot rows)

1. **Tag sigil and position.** Do tags reuse the attribute `#` (one sigil,
   role inferred by propagation) or get their own marker so the three kinds are
   visually distinct? And where do they sit — the prefix line and `#[…]` forms
   that S82/D-ATTR1 already sanction, or a new in-signature position? Trade:
   fewer sigils vs. a clearer mental model.
2. **Default maturity.** What does unmarked code mean — `tested`,
   `experimental`, or "untagged / unchecked"? Determines whether the rule bites
   by default or only when you opt in.
3. **Capability default.** Is ambient authority off by default (max safety,
   but hello-world may need a grant) or on until a boundary forbids it (zero
   friction, weaker guarantee)? This is a priority #1-vs-#2 call.
4. **Scope of value dimensions.** Ship uncertainty (recommended), defer/narrow
   cost types to opt-in latency budgets (recommended), or pursue full
   pervasive cost typing (high risk)?
5. **Build order.** Ratify the taxonomy (§1) standalone first, then sequence
   the four policies — or treat the whole system as one milestone?

## 8. Recommendation

Ratify **§1 (the taxonomy + decision rule)** on its own — it is cheap, it is
the owner's stated primary need, and it unblocks classification of every
future "is this a trait or an attribute?" question. Then treat maturity tags
and uncertainty as the first two policies (lowest risk, highest payoff),
capabilities/prohibitions as a larger progressive-disclosure design that
**reopens S60's rejection of a full effects system** (so it needs explicit
owner sign-off, not just a ballot tick), and cost types as opt-in expert-only.
Nothing here is a plan yet; each numbered open decision is a ballot row waiting
for your word.
