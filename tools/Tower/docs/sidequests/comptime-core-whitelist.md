# Plan: Comptime Core-module whitelist

**Status:** planned. Depends on D-CTCORE1.

## Goal

Let comptime execute a curated set of deterministic Core calls without importing the
whole runtime into the interpreter.

## Implementation Steps

1. Define a whitelist table for pure Core calls (`math`, selected `String` helpers, small
   collection helpers).
2. Add a comptime dispatcher that maps resolved Core symbols to interpreter functions.
3. Reject non-whitelisted Core calls with a teaching diagnostic naming the boundary.
4. Keep effectful APIs (`fs`, net, clock, rng) out unless a later decision explicitly
   allows them.
5. Add differential tests against runtime behavior for each whitelisted call.

## Verification

- Comptime unit tests for whitelisted calls.
- UI snapshot for non-whitelisted call.
- Golden example using `core.math` in a const/comptime context.
