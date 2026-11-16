# Design It Twice

When the user wants to explore alternative interfaces for a chosen deepening candidate, use this OMP-backed delegation pattern. Based on "Design It Twice" (Ousterhout) — your first idea is unlikely to be the best.

Uses the vocabulary in [SKILL.md](SKILL.md) — **module**, **interface**, **seam**, **adapter**, **leverage**.

## Handoff contract

- **Requested outcome:** Competing interface designs for one chosen deepening candidate.
- **Supplied inputs:** The candidate's problem-space frame, module/interface/seam vocabulary, domain context, dependency category, and existing ADRs.
- **Allowed child result:** Each authorized OMP design task returns one alternative interface, usage example, hidden implementation, adapter strategy, and trade-offs. It does not resolve the owner's choice, implement code, or dispatch another worker.
- **Completion owner:** `codebase-design` owns comparison and recommendation; the user owns the final choice.
- **Return point:** Every design task returns to the comparison step before the next design or recommendation.
- **Stopping condition:** Stop after the requested alternatives are compared and a recommendation is presented. Do not auto-chain implementation or an undeclared workflow.

## Process

### 1. Frame the problem space

Before spawning sub-agents, write a user-facing explanation of the problem space for the chosen candidate:

- The constraints any new interface would need to satisfy
- The dependencies it would rely on, and which category they fall into (see [DEEPENING.md](DEEPENING.md))
- A rough illustrative code sketch to ground the constraints — not a proposal, just a way to make the constraints concrete

Show this to the user, then proceed to Step 2. The user reads and thinks while
the delegated design tasks run. The owner guide controls concurrency and the
adapter; this document does not choose a model or a fixed worker count.

### 2. Delegate design tasks

Submit the genuinely independent design tasks through OMP `task` or `hub`,
using the most specific available role from `AGENTS.md`.
If the host cannot provide that route, report the routing failure rather than
pretending a child handoff occurred. Do not use a generic Agent tool, choose a
model in this skill, or exceed the owner's adaptive concurrency limits. Each
task must produce a **radically different** interface for the deepened module
and return only that design to `codebase-design`.

Prompt each sub-agent with a separate technical brief (file paths, coupling details, dependency category from [DEEPENING.md](DEEPENING.md), what sits behind the seam). The brief is independent of the user-facing problem-space explanation in Step 1. Give each agent a different design constraint:

- Agent 1: "Minimize the interface — aim for 1–3 entry points max. Maximise leverage per entry point."
- Agent 2: "Maximise flexibility — support many use cases and extension."
- Agent 3: "Optimise for the most common caller — make the default case trivial."
- Agent 4 (if applicable): "Design around ports & adapters for cross-seam dependencies."

Include both [SKILL.md](SKILL.md) vocabulary and CONTEXT.md vocabulary in the brief so each sub-agent names things consistently with the architecture language and the project's domain language.

Each sub-agent outputs:

1. Interface (types, methods, params — plus invariants, ordering, error modes)
2. Usage example showing how callers use it
3. What the implementation hides behind the seam
4. Dependency strategy and adapters (see [DEEPENING.md](DEEPENING.md))
5. Trade-offs — where leverage is high, where it's thin

### 3. Present and compare

Present designs sequentially so the user can absorb each one, then compare them in prose. Contrast by **depth** (leverage at the interface), **locality** (where change concentrates), and **seam placement**.

After comparing, give your own recommendation: which design you think is strongest and why. If elements from different designs would combine well, propose a hybrid. Be opinionated — the user wants a strong read, not a menu.
