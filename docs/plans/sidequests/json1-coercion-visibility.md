# Sidequest: D-JSON1 — Surface lenient JSON coercions

**Ratified:** 2026-06-17 (D-JSON1-decode option B)  
**Owner todo:** `docs/plans/owner-todo.md` — "brainstorm a way to surface coercions"  
**Milestone:** E2-M9 (`jet.json` package implementation)

## What and why

D-JSON1-decode: `jet.json` decodes leniently where unambiguous — e.g. `"8080"` → `8080` (Int), `"true"` → `true` (Bool). No errors, no breakage. But the owner wants the magic to be *legible*: silent coercion should not be invisible to the developer.

## Implementation requirement (ratified)

The implementation **must surface coercions**. Exact mechanism is up to the implementor to propose, but the constraint is: a developer must be able to see what was silently converted without changing the program's behavior.

## Approaches to evaluate (pick one; document the choice)

**A — Per-decode report struct:**  
`jet.json.decode_verbose(src)` returns `{ value: T, coercions: List<Coercion> }` where each `Coercion` names the field, original string, and coerced value. The normal `decode` path returns `T` unchanged. No output unless the developer requests the verbose form.

**B — Build-time report file:**  
`jet build` emits a `.jet/coercions.json` file listing all coercions encountered during the last test run (requires instrumented test mode). Developer can inspect it; CI can fail if non-empty (opt-in).

**C — Advisory lint:**  
Sema detects struct fields typed as non-string being decoded from JSON and emits L-code advisories listing the fields that will be leniently coerced. Fires at compile time, not runtime. Zero overhead at runtime.

The owner's stated concern is "make the magic legible without breaking." Prefer whichever approach surfaces the coercion closest to where the developer would notice it — probably A or C. Document the chosen approach in `m9-first-party-ring.md` under `jet.json`.

## Exit criteria

- `jet.json.decode` works leniently (`"8080"` → `8080` where the target type is `Int`)
- There is at least one mechanism by which the developer can discover what was coerced
- The mechanism is tested (golden test or unit test)
- Documented in `jet.json` package docs
