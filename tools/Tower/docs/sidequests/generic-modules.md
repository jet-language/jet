# Generic modules

**Card:** c91 / c1jixkit. **Decisions:** D-GENMOD1=A, D-GENMOD2=A. **Status:**
done (2026-06-30).

## Goal

Add ML-functor-style modules parameterized by types and values. Instantiating a
generic module produces a normal module with specialized exported types,
functions, and nested modules.

## Ratified Floor

D-GENMOD1=A approves the feature and examples the shape:

```jet
module Sorted(T: Ord) {
    pub fn sort(xs: [T]) -> [T] { ... }
}

module SortedInt = Sorted(Int)
```

D-GENMOD2=A ratified unified `<…>` parameters: type bounds (`K: Hash`) vs value
types (`capacity: Int`) in one list; instantiation mirrors declaration
(`Lru<String, 32>`).

## Build Plan (shipped 2026-06-30)

1. Record D-GENMOD1/D-GENMOD2 in `docs/spec/syntax-decisions.md`. ✓
2. Parser:
   - accept module parameter lists on `module Name(...) { ... }`;
   - accept module instantiation aliases;
   - carry spans on every parameter, argument, and alias target.
4. AST/foundation:
   - add `ModuleParam`, `ModuleArg`, and `ModuleAlias` nodes;
   - distinguish type params and value params without stringly parsing.
5. Sema:
   - register generic module definitions without instantiating them eagerly;
   - instantiate on alias/use with a deterministic substitution environment;
   - require type arguments to satisfy bounds;
   - require value arguments to be compile-time constants;
   - reject recursive module instantiation cycles;
   - keep generated module items private/public exactly as declared inside the
     generic module.
6. Codegen:
   - emit only instantiated modules reachable from the entry graph;
   - mangle instantiated module symbols with stable argument fingerprints;
   - keep TIR lowering unchanged: instantiated bodies enter sema as ordinary
     checked bodies before codegen.
7. Diagnostics:
   - E08xx for unknown module parameter, wrong argument count, bad value
     argument, unsatisfied type bound, and instantiation cycle;
   - each gets what/why/fix text in `docs/spec/diagnostics.md` and a UI
     snapshot.
8. Tests/examples:
   - generic sorting module over a type parameter;
   - value-parameterized fixed-size ring buffer or retry policy;
   - cross-file `use` of an instantiated module;
   - no-duplicate-codegen regression for two imports of the same instantiation;
   - golden example output.

## Invariants

- I3: rustc never validates generic-module semantics; sema owns every check.
- I7: every new sigil/keyword/spelling lands in `crates/jet-foundation/src/Syntax.rs`
  with D-GENMOD decision IDs.
- I8: generic modules must not become a second way to write a generic type. Use
  them only when a module bundles multiple related items behind one parameter set.

