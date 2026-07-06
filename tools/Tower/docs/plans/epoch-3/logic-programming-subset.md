# Logic Programming Subset

## Goal

Card #142 captures the logic-programming question. Current owner law does not approve a full Prolog-style language subset. It parks the idea as post-v1 constraint/solver research and points practical work toward failure-aware iterators and explicit solver libraries.

The planning goal is to keep Jet useful for constraint enumeration without adding a second execution model.

## Current law

- D-LOGICPROG1=C: full logic programming is far-horizon research; a future library may implement backtracking through explicit solver machinery.
- The rejected full version would add unification, choice points, and non-deterministic execution, conflicting with deterministic ownership and I8.
- Failure-aware iterators/comprehensions are the preferred practical slice when that roadmap item is active.
- I1 and I3 require ownership and checking to stay sema-owned; codegen must not discover logic failures by running another engine.

No new logic syntax, relation blocks, or variable-binding rules are ratified.

## Vertical slices

1. Constraint library research: define an explicit `Solver`-style library model in planning notes, with variables and constraints as ordinary values.
2. Failure-aware iterator alignment: map common solver/enumeration use cases onto existing iterator/filter/collect patterns and the planned failure-aware iterator work.
3. Deterministic search API: if implemented later, search state is an explicit value with owned resources, not hidden backtracking in the language.
4. Diagnostics for rejected surfaces: if users write relation/block syntax after a ballot, errors should point to the library path or failure-aware iterator path.
5. Performance profile: solvers can later specialize search without changing Jet evaluation rules.

## Acceptance tests

- Planning-only now: no compiler implementation until exact library/API or syntax is separately ratified.
- If a library slice is later approved: examples solve finite puzzles with explicit solver state and deterministic output.
- UI snapshots: any new rejected relation syntax or unification operator must have a Jet diagnostic before shipping.
- Ownership test: solver variables cannot bypass move/write rules.
- Dev/AOT parity: solver examples produce identical result order under `jet dev` and compiled execution.

## Dependency order

1. Finish or promote failure-aware iterator/comprehension work if it is still separate.
2. Ratify whether Epoch 3 wants an explicit solver library slice.
3. Implement finite deterministic solver examples as library code.
4. Add diagnostics only for surfaces the owner chooses to recognize.
5. Revisit full logic programming only after deterministic library demand is proven.

## Owner ballots needed

- D-SOLVER-LIB1: whether Core gets an explicit constraint/solver library in Epoch 3, and its public API shape.
- D-FAILITER1: exact failure-aware iterator/comprehension surface if not already ratified elsewhere.
- D-LOGIC-SYNTAX1: only needed if anyone proposes relation blocks, unification syntax, or multi-answer language forms; current law does not allow them.

## Adversarial tradeoffs

- Safety first: hidden backtracking must not disturb ownership, drops, or mutation order.
- Beginner experience: ordinary filtering and solving should read like Jet, not a second language inside Jet.
- Runtime performance: explicit search state can optimize later while keeping costs visible.
- One mechanical path: relation blocks, comprehensions, and solver APIs must not become three ways to express the same search. Prefer explicit library search plus normal iteration.
- Ecosystem breadth: constraint solving is useful for configuration, games, tests, and planning, but it must remain a library capability unless owner law changes.
