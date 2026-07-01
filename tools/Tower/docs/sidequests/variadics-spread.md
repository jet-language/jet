# Variadics + Spread

**Card:** c93 / c1lixnlk. **Decision:** D-VARIADIC1=A. **Status:** ready to build.

## Goal

Ship one spread/rest surface across functions, calls, and list literals:

- variadic parameter: `name: ...T`, last parameter only;
- call spread: `f(...xs)`;
- list spread: `[...a, x, ...b]`.

This is not fan-out. Fan-out calls a function once per element; spread expands a
list into one call or one new list.

## Build Plan

1. Register `...` in `Syntax.rs` with D-VARIADIC1.
2. Lexer/parser:
   - parse variadic parameters only in final parameter position;
   - parse call spread arguments;
   - parse list spread elements.
3. Sema:
   - lower variadic parameters to a list-like value visible in the function body;
   - reject non-list spread at call/list sites;
   - reject fixed arguments after a spread unless the callee's remaining shape is known;
   - preserve labels/defaults interaction from S61.
4. Codegen/TIR:
   - pack trailing variadic arguments into a list;
   - expand call spread into the same argument pack path;
   - list spread lowers through one list builder path, not repeated concatenation code.
5. Diagnostics:
   - variadic param not last;
   - spread non-list;
   - spread used where callee shape cannot accept it.
6. Examples and tests:
   - logging variadic;
   - spread existing list into call;
   - list spread round-trip;
   - golden output and UI snapshots.

## Verification

- `nix develop -c cargo test --test diagnostic_snapshots`
- `nix develop -c cargo test --test golden`
- `nix develop -c cargo test`

