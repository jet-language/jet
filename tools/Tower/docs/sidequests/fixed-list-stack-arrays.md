# Plan: Fixed-size lists `[T#N]` as stack arrays

**Status:** planned. Depends on D-FIXARR1.

## Goal

Make the ratified fixed-size list type `[T#N]` lower to a true fixed stack layout so it
can support `#Uninit` safely.

## Implementation Steps

1. Change type lowering for `Type::FixedList` from `Vec<T>` to Rust `[T; N]`.
2. Define widening from `[T#N]` to `[T]` as an explicit generated copy into a growable
   list.
3. Audit indexing, slicing, destructuring, and `.len` behavior.
4. Restrict `#Uninit` to fixed-layout element types.
5. Add codegen tests around copy/move behavior and stack layout.

## Verification

- Existing S76 tests stay green.
- UI snapshots for out-of-range fixed indexes.
- Golden example for `[U8#N]` buffer use.
- Then resume `visible-uninit`.
