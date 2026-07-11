# Generic modules

**Card:** c91 / c1jixkit. **Decisions:** D-GENMOD1=A, D-GENMOD2=A,
D-GENMOD-VALUE1=A, D-GENMOD-BODY1=A, D-GENMOD-IDENTITY1=A. **Status:** open.

## Goal

Add ML-functor-style modules parameterized by types and values. Instantiating a
generic module produces a normal module with specialized exported types,
functions, and nested modules.

## Ratified law

D-GENMOD1=A approves the feature. D-GENMOD2=A fixes one `<…>` parameter list:
type parameters use bounds (`K: Hash`), value parameters use value types
(`capacity: Int`), and instantiation mirrors declaration.

```jet
module Ring<T, capacity: Int> {
    pub struct Buffer { slots: [T#capacity] }
}

module PacketRing = Ring<Packet, 64>
```

D-GENMOD-VALUE1=A admits immutable Tier-0 comptime `Bool`, `Int`, `Char`,
`String`, and fieldless-enum values. Values normalize before specialization;
`Int` additionally fills the narrowly approved `[T#capacity]` layout slot.

D-GENMOD-BODY1=A gives templates full ordinary-module item and marker parity,
with definition-site lexical capture. D-GENMOD-IDENTITY1=A makes instances
applicative: the same resolved template DefinitionId and normalized arguments
mean one instance, including nominal types, sema, TIR/codegen, cache, and LSP
identity.

## Current implementation

The parser and AST carry templates, type/value parameters, applications, and
spans. The sema pre-pass expands same-file aliases whose bodies contain only
functions, substitutes type parameters in function signatures, and erases the
template before codegen. E0850, E0851, the generic-module example, and the
existing UI fixtures cover that floor.

This is not the ratified completion state. Value arguments are not evaluated or
substituted, type bounds and cycles are not checked, function bodies are not
specialized, non-function items still hit E0854, repeated applications clone
aliases instead of sharing one applicative instance, and cross-file templates
are not complete.

## Remaining build plan

1. Parser and AST:
   - parse every ratified closed value argument as an expression;
   - retain exact spans and the type/value distinction.
2. Sema:
   - register generic module definitions without instantiating them eagerly;
   - evaluate and normalize value arguments under Tier-0 law;
   - substitute type and value parameters through every admitted body item;
   - require type arguments to satisfy bounds;
   - reject recursive module instantiation cycles;
   - preserve definition-site capture and ordinary module visibility.
3. Identity and codegen:
   - intern one instance by DefinitionId plus normalized arguments;
   - share nominal types, sema/TIR, generated code, cache, and LSP references;
   - emit only instantiated modules reachable from the entry graph;
   - keep one stable InstanceFingerprint per instance.
4. Diagnostics and proof:
   - ship E0852, E0853, and E0855 with what/why/fix UI snapshots;
   - value-parameterized fixed-size ring buffer or retry policy;
   - full-body declarations and definition-site capture;
   - cross-file `use` of an instantiated module;
   - same-argument nominal identity and no-duplicate-codegen regressions;
   - a golden example and exact complete-instantiation acceptance test.

## Invariants

- I3: rustc never validates generic-module semantics; sema owns every check.
- I7: every new sigil/keyword/spelling lands in `crates/jet-foundation/src/Syntax.rs`
  with D-GENMOD decision IDs.
- I8: generic modules must not become a second way to write a generic type. Use
  them only when a module bundles multiple related items behind one parameter set.
