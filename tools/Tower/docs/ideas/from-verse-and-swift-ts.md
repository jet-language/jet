# Idea cards — mined from the Verse and Swift/TS research notes

Source files mined:
- `docs/research/jet-borrowing-from-verse.md`
- `docs/research/jet-vs-swift-typescript.md`

Each card is one distinct idea (near-duplicates merged). **Status** says whether
we've already decided or built it, so you only spend time on the genuinely new
ones. Read the **CEO note** for the one-line keep/skip call.

Headline: almost everything in the Verse brief is **already ratified or in the
ballot** — that doc is more "outside validation of bets we made" than new ideas.
The Swift/TS brief has the real new material: a typed full-stack protocol with a
migration gate, and the whole reactive-UI/web-frontend story.

---

## From `jet-borrowing-from-verse.md`

### 4. Structured concurrency without the "async tax" (no function coloring)
**What it is.** A small set of concurrency words (run-all, race-and-take-winner,
background) where tasks can't outlive their scope, and concurrency lives in the
one effect system instead of a separate "async" world that splits functions into
two incompatible colors.
**Source.** Section 2.4 "Structured concurrency".

### 5. A tiny formal core that everything desugars to
**What it is.** Define a small Jet "kernel" and make every surface feature
shorthand for kernel operations, so the language stays small and honest as it
grows instead of sprawling like C++.
**Source.** Section 2.5 "A tiny formal core — a process borrow, not a feature".
**Status.** `NEW` (as an explicit process) — but it restates principles already
baked into CLAUDE.md (the "simplicity ratchet" I8, "examples are the executable
spec" I5). No formal kernel document exists.
**CEO note.** Mostly a re-statement of our existing discipline; the only new ask
is "write down an actual minimal kernel + desugaring map now." Low urgency, but
cheap insurance against feature sprawl — a possible process card, not a feature.

### 6. Logic-programming subset: list-builders that auto-skip failures
**What it is.** Verse's full logic programming (functions that run "backwards,"
try many answers) is too exotic — Epic itself stripped it from Fortnite. Keep
only the gentle slice: list/collection builders that automatically skip the
elements where a step fails (filter-as-you-go).
**CEO note.** Possibly worth a small card: a failure-aware comprehension is a
natural payoff of idea #1 (failure-as-control-flow) and feels beginner-friendly.

---

## From `jet-vs-swift-typescript.md`

### 9. Reactivity built into the language (signals/derived/effects in the runtime)
**What it is.** Change one input and only what depends on it recomputes — and
because every value tracks where it came from, time-travel debugging is free.
Frameworks (React/Solid/Svelte/Leptos) bolt this on; Jet would put it in the
runtime, with `freeze(x)` to opt a hot path out.
**Source.** "The five things only Jet does" #1 (#1–4); UI table L0 "Reactivity".
**Status.** `ALREADY A CARD (c64 "Reactive / dataflow")` — but note the card's
current stance: adopt the dataflow graph as *tooling* + an opt-in `std.reactive`
library, and **reject reactivity as the core evaluation model** because it
collides with priorities #3/#4 and move semantics (proposal P3-reactive-dataflow).
**CEO note.** Important tension: this research wants reactivity *in the core
runtime*; our existing c64 card explicitly rejects that and keeps it a library.
Worth a deliberate decision — the Swift/TS doc's whole "replace everything" thesis
rests on core reactivity, so this is the one place to consciously pick a side.

### 10. One type system across the wire — typed client/server protocol
**What it is.** Describe the client/server protocol once (`protocol Orders { … }`)
and the compiler generates both sides, so a mismatched API can't ship. Replaces
the tRPC/Zod/hand-synced-two-stacks dance.
**Source.** "The five things only Jet does" #2 (#9, #10); `protocol Orders` example.
**Status.** `NEW` — no protocol/over-the-wire card, ballot item, or ratified
decision. Roadmap only has generic networking (E2-M10) far out.
**CEO note.** Genuinely new and strategically central to "replace TypeScript." Big
scope (it's a whole full-stack story), but this is a headline differentiator worth
a real card/decision.

### 14. Refinement types (values constrained beyond their base type)
**What it is.** Types that carry an extra constraint (e.g. a non-empty list, a
positive int), so more mistakes are caught at compile time.
**Source.** "Five things" #4 (#19), listed alongside units/taint/capabilities.
**Status.** Partial — `ALREADY RATIFIED (limited)`: `[T#N]` exists as a
compile-time length refinement of a slice. General user-defined refinement types
are not in any decision/ballot/card.
**CEO note.** The narrow array-length form exists; general refinement types are
effectively `NEW` but only mentioned in passing here. Low priority unless someone
asks for it explicitly — flag, don't chase.

### 17. Reactive UI stack with an ownable, editable component kit
**What it is.** A layered UI stack (reactivity → view model → typed styling →
headless behavior → styled kit → motion → app kit) where the bottom layers are
the compiler's job and the kit is copy-in-and-own (shadcn-style), not a locked
theme. The pitch for "replace SwiftUI and the React ecosystem."
**Source.** "The UI stack" section, layer table L0–L6; "Four moves that are
Jet-only."
**Status.** `NEW` — no UI/view/component cards, ballots, or ratified decisions
(roadmap has nothing here; it's all post-v1 expansion in the doc's own agent
handoff). Depends on idea #9 (reactivity) landing in the core.

### 18. Typed styles — CSS that won't compile with a typo or wrong-unit value
**What it is.** Styles written in Jet with typed tokens: a misspelled property
(`paddng`) or a wrong-category unit (`delay: 200.px` — delay wants a time) is a
compile error instead of silently doing nothing.
**Source.** UI section "Typed styles that won't compile…"; diagnostics JET-E0460,
E0461.
**Status.** `NEW` — no styling/CSS decisions exist. (It reuses the ratified units
system from #12, but the styling layer itself is undecided.)
**CEO note.** New, and a clean demoable win, but it sits on top of the whole UI
stack (#17). Bundle with that track rather than as a standalone card.

### 19. Accessibility on by default in UI components
**What it is.** Headless components ship focus-trap, ARIA, and keyboard handling
for free; a11y attributes are applied for you (override per element), and a missing
accessible label warns then hard-errors on release builds.
**Source.** UI section "Accessible behavior you get for free"; diagnostic
JET-W0110; a11y-enforcement ballot row.
**Status.** `NEW` — no a11y decision/card exists.
**CEO note.** New, but part of the UI-stack track (#17) and post-v1. Nice safety
story; not a standalone near-term item.

### 20. Motion as derived reactive state (no separate animation runtime)
**What it is.** Animation is just reactive values changing over time (`opacity:
shown ? 1 : 0`, with a spring), instead of shipping a whole runtime like Framer
Motion.
**Source.** UI section "Motion is just reactive state."
**Status.** `NEW` — no decision/card. Depends entirely on core reactivity (#9).
**CEO note.** New, but a downstream payoff of the reactivity decision; part of the
UI track (#17), not a standalone item.

### 21. Render-target abstraction (web / native / embedded / TUI as backends)
**What it is.** UI components target an abstract renderer interface, not a specific
backend, so web (DOM+WASM), native (FFI or own-renderer), embedded, and TUI are all
just pluggable backends. The doc stresses deciding this *early* or you marry one
backend forever.
**Source.** "Plan early" ("Render-target abstraction"); ballot row "Render-target
abstraction" (rec: design the trait now).
**Status.** `NEW` — no render-target decision/card.
**CEO note.** New and time-sensitive per the research ("decide before the kit
exists"), even though delivery is post-v1. The cheap, high-leverage move is to
sketch the trait now; worth a small early card even if the UI itself waits.

### 22. Web backend: compile views to JS DOM ops, send logic/compute to WASM
**What it is.** WASM can't touch the DOM cheaply, so emit the view as plain JS DOM
operations (Svelte's move) and send only logic to WASM — dodging the boundary tax.
Cost: maintaining two backends (one emitting JS, one WASM).
**Source.** "Three frontiers — Web, vs JavaScript"; ballot "Web view architecture"
(rec b).
**Status.** `NEW` — nothing in roadmap/cards on a web/WASM/JS backend.
**CEO note.** New; the concrete delivery path for "replace TypeScript." Post-v1
and heavy (two backends), but it's the architectural choice the web story hinges on.

### 23. App backend: FFI to native widgets first, own-renderer canvas later
**What it is.** For apps-vs-Swift: start by FFI-ing to native widgets
(SwiftUI/UIKit, Jetpack Compose) for true native look, then add a Flutter/Skia-style
own-renderer for one-codebase-everywhere consistency.
**Source.** "Three frontiers — Apps, vs Swift" (two-route table); ballot "App UI
strategy" (rec c, native first).
**Status.** `NEW` — no card; general C FFI exists on the roadmap (E2-M14) but not a
native-UI strategy.
**CEO note.** New, post-v1, and the most expensive frontier (per-platform UI work).
Strategic direction only at this stage.

### 24. Interop as a first-class, day-one target (call JS/npm and Swift/native)
**What it is.** The cold-start problem (npm has millions of packages, Jet has zero)
kills new languages; the only fix is calling into JS/npm and Swift/native from day
one and migrating logic in gradually. The doc calls this the single most
adoption-critical decision.
**Source.** "Three frontiers — The cold start"; "Plan early" (interop first-class);
ballot "Interop priority" (rec a, JS/npm first).
**Status.** `NEW` (for JS/Swift) — only **C** FFI is on the roadmap (M7 `extern
rust`, E2-M14 C FFI). No JS/npm or Swift interop decision exists.
**CEO note.** New and flagged by the research as adoption-critical.

### 25. Auto-parallelism — sequential-looking code runs in parallel when proven safe
**What it is.** `users.map(fetchProfile)` runs in parallel automatically when the
compiler can prove there's no data race — Rust-level safety without fighting the
borrow checker. The doc calls it the hardest item and says ship no-GC speed first,
let auto-parallel land later.
**Source.** "Five things" #3 (#26); `let profiles = users.map(...)`; sequencing
section.
**Status.** `NEW` as *auto*-parallelism. Manual tasks/channels are ratified (S53,
deferred) and on the roadmap (E2-M1, data-race-free tasks via ownership). But
implicit auto-parallelization of `.map` is not in any decision/card.

