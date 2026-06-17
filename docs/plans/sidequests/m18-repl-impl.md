# Sidequest: E2-M18 — REPL implementation

**Plan:** `docs/plans/epoch-2/m18-repl.md`  
**Status:** all decisions ratified; ready to implement after E2-M4 ✅ interpreter is green  
**Depends on:** E2-M4 ✅ (interpreter backend), E2-M3 ✅ (CLI patterns)

## Critical amendments from 2026-06-17 re-ratification

| Decision | Revised ratification | Delta |
|---|---|---|
| D-REPL11 | **A — std-only line reader** | **CHANGED**: was C (rustyline + completions); revised to A |
| D-REPL18 | **Moot — no external crate** | **CHANGED**: was A (rustyline); revised to moot because D-REPL11 is now A |

**No external crate.** The REPL uses a std-only input loop. Richer editing (history, completion) is a future upgrade that needs a fresh I6 sign-off.

## Full ratified decisions (implement exactly as ratified)

See `m18-repl.md` owner-decisions tables. Key calls:
- D-REPL4=A: interpreter only; plain message when unsupported
- D-REPL5=A: expressions + statements + control flow
- D-REPL7=C: accumulating default + optional `:cell` mode
- D-REPL8=A: real move semantics across inputs
- D-REPL9=A: brace-count multi-line prompt
- D-REPL14: prompt user to compile & run when snippet needs native (don't silently reject)
- D-REPL15=B: `:quit :reset :load :type :help` meta-commands
- D-REPL16=B: echo type+value; trailing `;` silences
- D-REPL20=A: transcript fixtures in `tests/repl/`

## D-REPL-PRELOAD implementation

Auto-import `std.io` so `print` just works. On **first use** of an auto-imported symbol, print one teaching line:

```
note: `print` is from `use std.io` — imported automatically in the REPL
```

Only once per symbol per session, not every invocation.

## Implementation order

Follow the 8-step order in `m18-repl.md` §Implementation order. Skip step 6 (line-editor crate) — D-REPL11=A means std-only forever in this version.

## Exit criteria

See `m18-repl.md`. Key: `jet repl` starts; `1 + 2` → `3`; move error matches batch E02xx; transcript suite green. No external crates. `nix develop -c cargo test` green.
