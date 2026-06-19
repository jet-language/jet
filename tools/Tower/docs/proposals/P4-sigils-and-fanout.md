# P4 — Sigils & fan-out sugar

**Status:** idea / proposal (not a plan).

Two small, concrete syntax notes from the scratchpad. Both are sugar-level, but
both collide with already-ratified spellings, so each needs a clear-eyed
conflict check before it could become a ballot row.

---

## A. Reference sigil: `@` instead of `&`

> *Scratchpad:* "Use @ instead of & for reference sigil."

### Today

`&x` takes a pointer and `*p` derefs it; both are **core grammar, sema-gated**
in the expert low-level tier (S58 / `core.mem`). Outside the gate they raise
E0208-family teaching errors. So `&`/`*` are not beginner surface — they live
behind `use core.mem`.

### The conflict (the reason this isn't a free swap)

`@` is already one of the busiest sigils in Jet:

- **Loop labels** keep `@` — `@outer loop { break @outer }` (D-ATTR3, ratified
  2026-06-19).
- **Source refs** `provider@target` (U6) and **host selector** `@host` (U16) —
  though these live in CLI/manifest strings, not source bodies.
- **D-ATTR1** (same day) moved attributes *off* `@` to `#`; **D-ATTR3** then
  ruled loop labels nonetheless *stay* `@`, accepting the two-sigil
  coexistence knowingly. So the owner just deliberately reduced `@`'s load —
  adding the reference sigil to `@` would push straight back the other way.

Meanwhile `&` is comparatively free — its only real use is the pointer sigil
plus `&&` logical-and (S13), which never sits in prefix position, so no clash.

### Read

The scratchpad's instinct (a friendlier reference sigil) is reasonable, but the
evidence points the other way: `&` is *less* overloaded than `@`, and the
recent D-ATTR3 work spent effort moving load *off* `@`. Swapping `&`→`@` would
re-crowd the sigil the owner just relieved. If the goal is "references feel
nicer," the stronger move may be a **named** form in the low-level tier (e.g.
`ref x` / `ptr x`) rather than trading one punctuation for a busier one.

| Option | Trade |
|---|---|
| Keep `&`/`*` (status quo) | Familiar to C/Rust/Zig experts; `@` stays relieved. No work. |
| Swap to `@`/`*` | Matches scratchpad; re-crowds `@` right after D-ATTR3 unloaded it. |
| Named (`ptr`, not `ref`) | Most beginner-legible, but verbose in pointer-heavy expert code. **`ref` is taken** — it is a ratified S10 ownership keyword (stored field, tier 2) — so a named form must use `ptr` or similar, not `ref`. |

**Recommendation:** likely **keep `&`** (or explore a named `ptr` form), not
adopt `@`. Flag for the owner with the conflict visible — this is a
sigil-budget call, not a clear win.

---

## B. Namespace fan-out: `s.{ f1(…) f2(…) f3(…) }`

> *Scratchpad:*
> ```jet
> use std as s
> fn main() {
>   s.{ func1(...)
>       func2(...)
>       func3(...) }
> }
> ```
> "would expand to s.func1, s.func2, s.func3."

### Today

Jet already has a fan-out operator, **ratified and implemented** as **S75**
(2026-06-16): `f.[a, b, c]` desugars to `[f(a), f(b), f(c)]`, items typed by
`f`'s parameter (expected-type elaboration), result type `[T#N]` (S76),
diagnostics E0961/E0962. `U6`'s `default.[ripgrep, fd]` is "one instance of the
general fan-out." So axis one is *settled code*, not a concept — the namespace
form below is the **unratified second axis** of an already-shipped operator,
which is a much cleaner thing to propose than a greenfield construct.

### How the two relate

These are two different fan-out axes, and naming them together clarifies both:

| Shape | Means | Fans out over |
|---|---|---|
| `f.[a, b, c]` (existing) | `[f(a), f(b), f(c)]` | **arguments** to one function |
| `s.{ f1(…) f2(…) f3(…) }` (this note) | `[s.f1(…), s.f2(…), s.f3(…)]` | **members** of one namespace |

`.[…]` distributes one callee over many args; `.{…}` distributes one receiver
over many calls. They are duals. Adopting `.{…}` should be framed as *the
second axis of the existing fan-out*, not a separate feature — that keeps the
"one mechanical path" story intact (one idea, two axes) rather than minting an
unrelated construct.

### What it buys

```jet
// without:
let a = config.validate(input);
let b = config.normalize(input);
let c = config.persist(input);

// with namespace fan-out:
config.{ validate(input) normalize(input) persist(input) }
```

Cuts the repeated receiver — squarely in the owner's documented
anti-repetition lane (see `owner-anti-repetition-example-driven` memory).

### Open questions this raises

- **Result shape.** Does `.{…}` return a tuple/list of the results, or is it
  statement-style (run each for effect, discard)? The scratchpad example is in
  a `fn main()` body, suggesting statement-style; `.[…]` is expression-style.
  They may need different answers.
- **Separator.** The scratchpad example separates calls with whitespace, but
  S75's ratified grammar *mandates commas* (`f.[a, b, c]`). A whitespace-
  separated `.{…}` would diverge from its already-shipped sibling — a real
  consistency cost, not just a readability preference. Defaulting `.{…}` to
  commas too is the conservative choice.
- **Receiver kind.** Modules (`s.`), struct instances (`config.`), or both?

| Option | Trade |
|---|---|
| Adopt as second fan-out axis (extends S75) | One coherent fan-out story; cuts receiver repetition. |
| Keep only `.[…]` | Smaller surface; receiver repetition stays. |

**Recommendation:** extend the **ratified S75** spec to cover the second axis,
so the operator is specified once with a consistent story (same separator,
same `[T#N]` result shape where it applies). Axis one already ships, so this is
an additive amendment, not a new operator.

---

## Open decisions for the owner (future ballot rows)

1. **Reference sigil:** keep `&`/`*` (recommended), swap to `@`/`*`, or
   introduce a named `ptr` form (not `ref` — taken by S10)?
2. **Namespace fan-out:** extend the ratified S75 fan-out with `.{…}` as its
   second axis, or keep `.[…]` only?
3. **If adopted — fan-out result shape:** expression (returns a list) vs.
   statement (runs for effect); and the separator (commas vs. whitespace).

Both items are sugar; neither is urgent. The value of writing them down now is
the **conflict and duality analysis** — so that when they reach the ballot, the
sigil-budget cost (A) and the one-operator-two-axes framing (B) are already on
the table. Not a plan; these are ballot rows.
