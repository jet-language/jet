# Generic modules

**Card:** c91 / c1jixkit. **Decisions:** D-GENMOD1=A, D-GENMOD2=A,
D-CONF-GENSPELL1=A,
D-GENMOD-VALUE1=A, D-GENMOD-BODY1=A, D-GENMOD-IDENTITY1=A. **Status:** open.

## Goal

Add ML-functor-style modules parameterized by types and values. Instantiating a
generic module produces a normal module with specialized exported types,
functions, and nested modules.

## Ratified law

D-GENMOD1=A approves the feature. D-CONF-GENSPELL1=A fixes the surface:
type parameters and bounds use `<…>`, typed value parameters use `(…)`, and
instantiation uses `::`.

```jet
module ring<T>(capacity: Int) {
    pub struct Buffer { slots: [T#capacity] }
}

module packet_ring :: ring<Packet>(64)
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

Parser, sema, and codegen specialize full bodies, including nominal types,
traits and impls, constants, tests/benches, and nested modules/templates. Values
are evaluated and normalized, bounds and cycles are checked, definition-site
scope is retained, imported templates work, and equivalent applications share
one applicative instance. Instance identity reaches TIR/codegen, build-cache
inputs, semindex, and LSP symbols; digest/full-key collisions stop as E0859/ICE
before codegen. Exported nominal names encode each alias segment with its Unicode
character count, so distinct aliases stay distinct while obeying type casing
(`three_ints.Buffer` becomes `M5Three4IntsBuffer`).

The card remains open for its final executable acceptance matrix and remaining
documentation/example closure. Later cache/toolchain criteria must be verified
before claiming the entire card complete.

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
   - ship E0852, E0853, E0855, E0856, E0857, and E0859 with What/Why/Fix,
     `jet explain`, exact UI snapshots, and JSON/LSP parity;
   - make E0855 print the full application chain and E0859 exit 101 on a
     fingerprint/full-key collision without fallback;
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
