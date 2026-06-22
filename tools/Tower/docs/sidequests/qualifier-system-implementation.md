# Plan: Qualifier system implementation

**Status:** planned. D-QUAL1 and D-QUAL2 are ratified; D-EFF2/D-EFF3 still gate the
effect-specific parts.

## Goal

Turn the qualifier taxonomy into implementation slices without mixing tags, traits,
effects, and manifest policy into one patch.

## Slices

1. **Taxonomy cleanup:** ensure parser/sema/docs consistently distinguish traits from
   tags.
2. **Value tags:** implement erased tags that carry no methods and participate in type
   checking.
3. **Parameterized tags:** support cases like units (`#unit(usd)`) once the tag surface is
   wired.
4. **Effects:** implement `#(net, db)` only after D-EFF2 and D-EFF3 unblock D-EFF1.
5. **Manifest/policy:** keep package policy separate from expression/type semantics.

## Verification

- Tag/trait parser snapshots.
- Sema tests for tag erasure and non-dispatch behavior.
- Decision drift test for D-QUAL1/D-QUAL2 references.
