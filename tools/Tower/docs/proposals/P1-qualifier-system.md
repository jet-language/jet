# P1 — The qualifier system: leftover policies (maturity, uncertainty, cost)

**Status:** idea / proposal (not a plan).

> The taxonomy core of this proposal has been extracted and ratified. What
> remains below are the three *policies* that ride the tag engine but have no
> card or decision of their own yet: **maturity tags** (§2), **value
> uncertainty** (§4a), and **cost/budget types** (§4b). Everything else moved
> out — pointers mark where.

> [Extracted — §0 glossary, §1 trait/attribute/tag boundary + decision rule,
> and the `pure fn`/`#Unsafe` dual-face analysis → card **c62**, decisions
> **D-QUAL2** (two kinds: trait vs tag, ratified 2026-06-21) and **D-QUAL1**
> (surface = sigil-pure `#(…)`, ratified 2026-06-22). Casing settled by
> **D-CASING1** (all tags PascalCase). Plan: `qualifier-system-implementation`.]

> [Extracted — §3 capabilities and prohibitions (the effects system) → card
> **c66**, decision **D-EFF1** (inferred, erased effect set; `pure` = ∅;
> `#(net, db)` boundaries + `#caps(…){ }` regions; reopens S60), with follow-on
> **D-EFF2** (effect polymorphism) and **D-EFF3** (trait-method effects) still
> gating implementation. Scoped capabilities → card **c67** / **D-SCAP1**.]

> [Extracted — §5 "one shared tag engine" → the D-EFF1 propagation pass + the
> `qualifier-system-implementation` plan. §6 naming menu and §7 Qs 1/3/5
> (sigil/position, capability default, build order) → resolved by D-QUAL1,
> D-EFF1, and the ratified "taxonomy first" sequencing.]

---

## 2. Maturity tags (`experimental` / `tested` / `hardened`) — NOT YET CARDED

> *Scratchpad:* "Code tags: experimental, tested, or hardened — restricts
> hardened code from silently depending on experimental code."

A three-level lattice. The rule: **trust never flows downhill silently.**
Hardened code may not depend on tested or experimental code without an
explicit, visible acknowledgement.

```jet
#Hardened
fn settle_payment(order: Order) -> Receipt { ... }

#Experimental
fn estimate_fraud(order: Order) -> Float { ... }
```

If `settle_payment` calls `estimate_fraud`, the compiler stops:

```
Error [E07xx]: hardened code depends on experimental code
  ┌─ billing.jet:3:5
  │
3 │     let risk = estimate_fraud(order);
  │                ^^^^^^^^^^^^^^^ `estimate_fraud` is #Experimental
  │
Why: `settle_payment` is #Hardened — it must not silently rely on code that
     may still change or be wrong.
Fix: harden `estimate_fraud`, or opt in explicitly at the call site with
     `#Trusting(Experimental) { let risk = estimate_fraud(order); }`
```
*(Diagnostic codes here are illustrative; real codes get assigned in
diagnostics.md, which pins the `Error [E####]` / `Why:` / `Fix:` format.)*

This is a **tag** by the D-QUAL2 rule: it propagates (a function is as
trustworthy as its weakest call) and is checked relationally (hardened vs
experimental). It would ride the same engine as effects/taint — but it is its
own policy and has **no card or decision yet**.

Tradeoffs:

| For | Against |
|---|---|
| Trust boundaries become a compile error, not a code-review hope. | A thing to keep updated as code matures. |
| Refactors can't quietly pull unstable code into a stable path. | The lattice needs a default; unmarked code has to mean *something*. |
| Pairs with the package story — a registry can show a crate's maturity. | Over-tagging is noise; needs a light default and inference. |

**Open decision (un-carded):** what does unmarked code mean — `tested`,
`experimental`, or "untagged / unchecked"? Determines whether the rule bites
by default or only on opt-in.

---

## 4. Tracked value dimensions (uncertainty, cost) — NOT YET CARDED

> *Scratchpad:* uncertainty as a pervasive type dimension; cost/resource
> types.

These ride on **values** rather than code, but are the same tag kind by the
D-QUAL2 rule: a propagating, must-discharge property. Neither has a card or
decision. (`#Tainted` (D-TAINT1) is one *instance* of a value-tag, but the
general **uncertainty** axis — maybe-stale, untrusted, estimate ±5% — and
**cost** types are not carded.)

### 4a. Uncertainty

An axis on a value: "might be null," "came from untrusted input," "possibly
stale," "estimate ±5%." It propagates through expressions and must be
discharged before the value is used where it matters.

```jet
let age: Int?Untrusted = form.field("age").to_int();   // untrusted + maybe-absent
// using it raw is an error:
charge_adult_rate(age);   // error: `age` is #Untrusted and possibly absent

when age {
    some(a) if a >= 18 -> charge_adult_rate(a);   // discharged: present + checked
    _ -> reject();
}
```

The whole class of "I assumed this was fresh/clean/present" bugs becomes a
type error. Jet already has `Option` for the present/absent axis (M3) — this
generalizes the *idea* to other axes (trust, freshness, precision) rather than
minting a new `Option`-like type per axis. `#Tainted` (D-TAINT1) handles the
untrusted-input sub-axis specifically; the broader freshness/precision axes
remain unexplored.

### 4b. Cost / resource

The type tracks a budget: time complexity, allocation, latency. Exceeding it
is a compile error.

```jet
#Budget(latency: 10ms)
fn on_keystroke(e: Key) -> Edit { ... }   // compile error if a callee blows 10ms
```

Tradeoffs:

| For | Against |
|---|---|
| Uncertainty kills a whole bug category at compile time. | Cost types are research-fringe; sound static cost is genuinely hard. |
| Reuses the propagation engine the effect tags already need. | Too many value axes = annotations balloon; needs heavy inference. |
| `±5%` / freshness has no good prior art in a mainstream language — real differentiation. | Risk of a "type tetris" feel that violates priority #2 if not defaulted well. |

Honest call: **uncertainty is promising and mostly mechanism-shared with the
ratified effect/taint engine; cost types are the riskiest idea in the whole
scratchpad** and should probably be scoped to a narrow, opt-in expert form
(latency budgets on real-time handlers) rather than a pervasive complexity
axis.

**Open decision (un-carded):** ship uncertainty (recommended), defer/narrow
cost types to opt-in latency budgets (recommended), or pursue full pervasive
cost typing (high risk)?

---

## Why these survive

The taxonomy (§1), the effect/capability system (§3), units, linear, taint,
typestate, and transactions all became cards (c62, c66–c72) and ratified
decisions. **Maturity, general uncertainty, and cost types did not** — they
appear only as a phrase in c62's body ("…maturity tags … and uncertainty/cost
as policies on one shared tag engine") with no card, decision, or plan slice.
They are genuine, still-unexplored policies on the now-ratified tag engine.
