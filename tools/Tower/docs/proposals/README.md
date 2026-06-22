# Proposals

Idea-stage feature proposals distilled from the owner's scratchpad. Each is a
**digestible report, not a plan** — it lays out the idea, worked examples,
tradeoffs, an implementation sketch, and the open decisions the owner would
need to make. Each report ends at the decision seam: its "open decisions" are
the future ballot rows. Approve a direction and a report converts cleanly into
a sidequest plan.

Only genuinely-unexplored ideas remain here. Anything already carded or
ratified has been extracted out (the proposal files note where).

| # | Proposal | What's left (un-carded) | One-line read |
|---|---|---|---|
| **P1** | [Qualifier system — leftover policies](P1-qualifier-system.md) | maturity tags (`experimental`/`tested`/`hardened`); general value **uncertainty**; **cost/budget** types | The taxonomy + effects are ratified (c62/c66, D-QUAL1/2, D-EFF1); these three policies on the tag engine have no card yet. |
| **P2** | [Content-addressed definitions](P2-content-addressed-definitions.md) | Unison-style identity = content hash; free renames; no merge conflicts; dependency-hell relief | Genuinely new — not carded. Adopt as an invisible build cache now; the visible name/version model is a gated, larger design. |

Fully extracted and removed (now tracked entirely as cards + decisions):

- **P3 — reactive / dataflow** → card **c64**, decision **D-REACT1** (reactivity
  = tooling + opt-in `jet.reactive` library, not core semantics).
- **P4 — sigils & fan-out** → card **c65**, decision **D-FANOUT2** (defer
  namespace fan-out; keep S75 call fan-out; reference sigil stays `&`).
