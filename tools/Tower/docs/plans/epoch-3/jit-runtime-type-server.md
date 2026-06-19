# Epoch 3 pillar — JIT runtime type server

**Status:** owner-ratified (2026-06-16, D-DEV2). **Epoch 3 only.**

## Goal

A long-lived Jet process that JIT-compiles typed handlers and hot-swaps them on
save — a safe, high-performance alternative to TypeScript/Node for app servers
and interactive tools, without giving up Jet's memory safety or front-end-owned
diagnostics.

```
$ jet serve app.jet           # long-lived process, JIT-backed
# edit a handler, save → running server swaps in new typed code,
# no restart, no dropped connections — like nodemon, but the code is
# type-checked and memory-safe before it goes live.
```

## Relationship to Epoch 2

| Topic | Epoch 2 | Epoch 3 (this doc) |
|---|---|---|
| Edit feedback | Comptime interpreter (<200 ms, D-DEV3) | JIT compile hot paths |
| Shipping | `jet build` → rustc/LLVM binary | Same + optional long-lived mode |
| Cranelift | design note only | evaluate for JIT backend |
| TS/JS replacement framing | out of scope | in scope |

## Open design questions

- JIT backend: Cranelift vs incremental rustc vs hybrid.
- Hot-swap boundary: function, module, or whole program.
- Runtime type server API: how editors/agents query types at runtime.
- Interaction with `@async` scoped blocks (see [`async-networking.md`](async-networking.md)).

## Non-goals until Epoch 3

- No JIT in `jet dev` save loop for Epoch 2.
- No "run TS in Jet" bridge — typed Jet source only.
