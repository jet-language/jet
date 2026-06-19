# P3 — Reactive / dataflow-by-default

**Status:** idea / proposal (not a plan).

> *Scratchpad:* "The whole program is a spreadsheet. State synchronization is
> one of the largest sources of bugs… Make automatic recalculation the core
> model, not a framework you bolt on. Prior art: Eve, FRP, and the fact that
> spreadsheets are the most successful programming environment in history…
> **Consider this as tooling. A generated artifact in the .jet folder?**"

The owner's own annotation — "consider this as tooling, a generated artifact in
the .jet folder" — is the load-bearing instinct. This report takes that
seriously: reactivity as a *derived view and tool*, not a change to Jet's
evaluation model.

---

## 0. Glossary

- **Reactive value** — a value that automatically recomputes when its inputs
  change (a spreadsheet cell).
- **Dataflow graph** — the dependency graph of which values feed which.
- **Derived artifact** — something the compiler generates *from* your code into
  `.jet/`, that you read but don't hand-write.

---

## 1. Two very different things this could mean

The scratchpad blurs two ideas that must be split, because one fights Jet's
core priorities and the other doesn't.

**(a) Reactivity as the language's evaluation model.** Every binding is a cell;
assignment triggers cascading recomputation. This is Eve / FRP / a spreadsheet
runtime.

**(b) Reactivity as a derived view + library.** Jet stays an ordinary
eagerly-evaluated language. The compiler *derives* the dataflow graph from
your code and emits it as a tool artifact; an opt-in stdlib gives explicit
reactive values where you actually want them (UI, config).

**(a) is almost certainly a non-goal.** It collides with:

- **Priority #3 (zero-cost, no hidden machinery)** — a reactive runtime is
  exactly the hidden boxing/scheduling the philosophy forbids.
- **Priority #4 (one mechanical path) + C1** — "assignment moves" (C1) is a
  settled, central model; "assignment triggers recomputation" is a different
  language.
- **Non-goals for v1** already exclude global mutable state and async.

So this report develops **(b)**, which matches the owner's "tooling / `.jet`
artifact" note.

---

## 2. The valuable, in-scope core: derived dataflow

Jet's front end already builds a def-use/dependency graph for ownership and
sema. That is *not* yet the value-level dataflow graph the query below implies
(cross-field, control-flow-aware) — exposing it as a tool artifact needs an
extraction/serialization pass that doesn't exist today. So this is cheap-ish,
not free: the raw material is there; the serialized artifact is new work.

**Tooling artifact** — `jet` writes the dataflow graph to `.jet/` and the LSP
visualizes it: "what feeds this value, what does it feed, what recomputes if I
change it." This is the spreadsheet's superpower (see-the-dependencies) without
the spreadsheet's runtime. It also directly serves the **Blueprint north-star**
(see memory): a typed, visual view of data flow is the "see the wires" half of
Blueprint.

```
$ jet graph billing.jet --of total_due
total_due
├── subtotal      (sum of line_items[].price)
├── tax           (subtotal * rate)
└── discount      (from coupon, #untrusted until validated)
```

**Opt-in reactive values** — where recomputation genuinely helps (UI state,
live config), an explicit stdlib type makes the cell visible and costed, never
ambient:

```jet
use std.reactive as r

let price  = r.cell(10.0);
let qty    = r.cell(3);
let total  = r.derived(() => price.get() * qty.get());   // recomputes on change

price.set(12.0);
print(total.get());   // 36.0 — recomputed, explicitly, because you asked
```

The cost is visible (`r.cell`, `r.derived`), the rest of the program is
ordinary Jet, and priority #3 holds because nothing recomputes unless you built
a reactive graph on purpose.

---

## 3. Tradeoffs

| For | Against |
|---|---|
| The *tool* (derived graph + LSP view) is high-value and moderate-cost — the raw def-use graph exists; the serialized artifact + query is new work. | The *language model* (everything-is-a-cell) is off the table; must be clearly fenced or it scope-creeps. |
| Opt-in reactive stdlib serves real domains (UI, config) without taxing hello-world. | An stdlib reactive lib needs careful design to stay zero-cost when unused. |
| Directly advances the Blueprint north-star (visualize the wires). | "Spreadsheet program" framing will keep tempting toward model (a); needs a firm non-goal line. |
| `.jet/` artifact fits the content-addressed direction (P2) — graph keyed by hash. | Cross-cutting with P2 and the LSP; sequencing matters. |

---

## 4. Fit with Jet's existing decisions

- **Honors C1 / priority #3:** evaluation stays eager and move-based; reactivity
  is opt-in library + derived view, never the default model.
- **Pairs with P2:** the dataflow graph is naturally keyed by content address —
  unchanged nodes don't re-render.
- **Pairs with the LSP requirement** (S82 LSP note, Blueprint memory): the
  graph is exactly the kind of structural surface the LSP should expose.
- **Stdlib scope:** `std.reactive` is post-v1 ecosystem (priority #6), squarely
  in the same post-v1 bucket as networking — *not* v1 scope. Same posture as
  the non-goals list treats ecosystem growth.

---

## 5. Implementation sketch (not a plan)

- **Derived graph (tool):** add a pass that serializes the existing
  dependency/def-use graph to `.jet/graph.*`; add a `jet graph` query command
  and an LSP code-lens/visualization. No language change.
- **Reactive stdlib (opt-in):** `std.reactive` with `cell` / `derived` /
  observers, built on ordinary Jet closures (S46/S47) and Option/Result — no runtime
  privilege, no compiler magic.

## 6. Open decisions for the owner (future ballot rows)

1. **Scope line.** Confirm model (a) — reactivity as the evaluation model — is
   a **non-goal**, and only the derived-graph tool + opt-in stdlib are pursued.
   (Recommended: yes.)
2. **Tool first or library first?** The derived-graph artifact + LSP view is
   the cheaper, higher-confidence half; the reactive stdlib is a post-v1
   ecosystem item. Sequence accordingly.
3. **Artifact home.** Confirm `.jet/` as the generated-graph location (project
   mode only — single-file `jet run` must not require it, per file-is-a-program)
   and whether it is committed or gitignored.
4. **`jet graph` CLI surface.** The query spelling (`jet graph foo.jet --of
   total_due`) is a new user-facing command — its name and flags are open.
5. **Zero-cost guarantee.** How does `std.reactive` stay zero runtime cost when
   not imported (expected: monomorphization + no global scheduler)? Confirm
   before any implementation, since priority #3 forbids hidden machinery.

## 7. Recommendation

Split the idea: **adopt the derived dataflow graph as tooling** (cheap, serves
the Blueprint north-star, no language risk) and **a later opt-in
`std.reactive`** for genuine reactive domains. **Reject reactivity as the
evaluation model** — it contradicts priorities #1, #3, and #4 and the
move-based C1 resolution. The owner's own "consider as tooling" note is the
right read; this report just draws the fence explicitly. Each open decision is
a ballot row, not a build step.
