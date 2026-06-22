# Plan: Fan-out follow-ups

**Status:** planned. Depends on D-FANOUT2.

## Goal

Avoid accidental syntax growth around S75 fan-out while preserving a path for useful
namespace/member fan-out if the owner chooses it later.

## Implementation Steps

1. Keep S75 call fan-out `f.[a, b, c]` as the only shipped form unless D-FANOUT2 says
   otherwise.
2. Add parser tests that reject tempting unratified forms with clear messages.
3. If D-FANOUT2 chooses deferral, file examples in docs rather than implementing syntax.
4. If it chooses implementation, update parser, formatter, AST, and docs together.

## Verification

- UI snapshots for rejected unratified member/namespace fan-out.
- Golden test for existing S75 behavior stays green.
