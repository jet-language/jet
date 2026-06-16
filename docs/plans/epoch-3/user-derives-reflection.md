# Epoch 3 pillar — user-defined derives & typed reflection

**Status:** owner-ratified (2026-06-16). **Epoch 3** — implements S26 **layer 3**
(S56, deferred post-1.0).

## Goal

Let library authors ship **custom derive logic in typed Jet** — not token macros,
not Rust proc-macros — so users can write:

```jet
@MyWireFormat
struct Event { … }

// author provided:
trait MyWireFormat { fn encode(self) -> [U8]; … }
// compiler invokes author's pure Jet fragment to implement it
```

Same family as today's built-in `@Serialize` / `@Comparable`, but **user-authored**
with typed reflection over struct fields.

## Layering (S26)

| Layer | What | When |
|---|---|---|
| 1 | `comptime` bindings | Epoch 2 (M9.5) |
| 2 | Built-in derives (`Printable`, `@Serialize`, …) | Epoch 2 (M9) |
| **3** | **User derives + typed reflection** | **Epoch 3 (this doc)** |

## Constraints (carry forward)

- **No token/AST macros** (S26 rejected forever).
- **No attribute macros** that rewrite syntax — `@` markers only (S82).
- Reflection is **typed** — field names and types are known to sema; errors are
  Jet diagnostics (I4).
- User derive bodies run in the **pure** subset where possible (ties to D-PURE).

## Open design questions

- Syntax: `@derive(MyTrait)` vs `@MyTrait` as derive marker.
- Where config blocks live (S82 rule: overrides inside type body).
- Comptime vs `jet eval --pure` for derive execution.
- Orphan/coherence rules for derive impls across packages.

## Depends on

- S82 attribute surface (ratified).
- E2-M16 pure eval sandbox (D-PURE1/2 ratified).
- Evidence from built-in `@Serialize` overrides in the wild.

## Non-goals

- Not Epoch 2 GA.
- Not a general metaprogramming escape hatch.
