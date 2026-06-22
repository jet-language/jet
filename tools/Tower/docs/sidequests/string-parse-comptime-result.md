# Plan: String parse APIs + comptime `Result` / `Option`

**Status:** planned. Depends on D-STRPARSE1.

## Goal

Add first-class text-to-value parsing APIs and make pure parse flows usable during
comptime evaluation.

## Implementation Steps

1. Add runtime `String` methods such as `lines`, `parse_int`, and generic `parse` only
   where the type system can express the result clearly.
2. Return typed `Result`/`Option` values with existing `?` behavior.
3. Extend the comptime evaluator to represent and branch through `Result`/`Option`.
4. Keep parse errors deterministic and snapshot-tested.
5. Add examples for embedded config/schema parsing.

## Verification

- Unit tests for string parsing.
- Comptime tests for successful and failed parse paths.
- Golden example using embedded text.
