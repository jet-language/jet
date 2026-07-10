# Plan: Reactive/dataflow follow-up

**Status:** planned. Depends on D-REACT1.

## Goal

Keep ordinary Jet execution non-reactive while making dependency information useful for
tools and leaving room for an explicit `jet.reactive` library.

## Implementation Steps

1. Add a compiler-internal dependency graph extraction pass for modules/functions.
2. Expose the graph to tooling first: LSP, docs, build invalidation, or visualization.
3. Design `jet.reactive` as a library layer with explicit `signal` / `derived` values.
4. Keep runtime observers out of core language semantics.
5. Add examples showing normal assignment versus explicit reactive values.

## Verification

- Unit tests for dependency graph extraction.
- LSP/tooling snapshot if exposed.
- Library examples once `jet.reactive` exists.
