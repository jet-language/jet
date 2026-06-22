# Plan: Units as parameterized tags

**Status:** planned. D-UNIT1 is ratified; implementation rides D-QUAL1/D-QUAL2 tag
machinery.

## Goal

Implement unit tags as erased, parameterized value/type qualifiers, not as distinct-type
newtypes.

## Implementation Steps

1. Build the generic parameterized-tag representation.
2. Parse and preserve unit tags such as `#unit(usd)` according to the ratified qualifier
   syntax.
3. Type-check arithmetic so matching units compose safely and mismatched units produce
   diagnostics.
4. Erase units in codegen after sema proves compatibility.
5. Add docs and examples for currency and physical units.

## Verification

- UI snapshots for unit mismatch and valid conversion points.
- Golden example for arithmetic with matching units.
- Codegen check that tags erase.
