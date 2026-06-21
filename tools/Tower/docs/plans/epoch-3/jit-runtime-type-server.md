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

## Three execution modes (explicit framing)

A Jai-borrowed "three-mode execution" proposal lines up almost exactly with Jet's
existing surface. Naming it explicitly so the modes, and their owners, are legible:

| Mode | Verb | Backend | Status |
|---|---|---|---|
| **Dev runtime / hot-reload** | `jet dev` (watch loop) → extends toward hot-swap; or `jet serve` (long-lived JIT) | interpreter today; JIT hot-swap in Epoch 3 | watch+re-run **SHIPPED** (E2-M4, D-DEV4); hot-swap/JIT **Epoch 3** |
| **Quick run** | `jet run` | interpreter, run-once | **SHIPPED** |
| **Release build** | `jet build` / `jet build --emit-rust` | rustc/LLVM native binary | **SHIPPED** |

The three CLI verbs are the **already-ratified surface** (D-DEV4 settled
`dev`/`run`/`build` naming — not reopened here). What Epoch 3 adds is *backend
power* under the dev-runtime mode: turning today's interpreter watch loop into a
JIT-backed hot-swap process, and pinning a dev↔release consistency guarantee. The
open questions are the JIT backend, the hot-swap/state semantics, and where the
hot-reload mode lives (see ballots `_jai-jit.md`: D-JIT1, D-HOTSWAP1, D-DEVMODE1).

## Relationship to Epoch 2

| Topic | Epoch 2 | Epoch 3 (this doc) |
|---|---|---|
| Edit feedback | Comptime interpreter (<200 ms, D-DEV3) | JIT compile hot paths |
| Shipping | `jet build` → rustc/LLVM binary | Same + optional long-lived mode |
| Cranelift | design note only | evaluate for JIT backend |
| TS/JS replacement framing | out of scope | in scope |

## Open design questions

- JIT backend: Cranelift vs incremental rustc vs hybrid vs stay-interpreter-for-v1
  (ballot **D-JIT1**).
- Hot-swap boundary: function, module, or whole program (ballot **D-HOTSWAP1**).
- Runtime type server API: how editors/agents query types at runtime.
- Interaction with `#async` scoped blocks (see [`async-networking.md`](async-networking.md)).

## Sub-items (from the three-mode proposal)

Numbered as **4x** so they slot under the dev-runtime mode work. Each notes
**shipped** vs **Epoch-3**.

- **4a — File-watcher / debounce.** *(extends shipped.)* `jet dev` already watches
  the project and re-checks+re-runs on save with sub-200ms feedback (E2-M4, D-DEV4).
  Epoch 3 extends that same watcher toward **hot-swap**: instead of tearing down and
  re-running the interpreter, the long-lived process swaps the changed unit in place.
  The debounce/coalesce-rapid-saves logic is reused as-is; what changes is the action
  taken when the watcher fires.
- **4b — JIT compile of hot paths.** *(Epoch 3.)* Replace the comptime interpreter on
  hot handlers with JIT-compiled native code. Backend is **D-JIT1**.
- **4c — Hot-reload state policy.** *(Epoch 3, ballot D-HOTSWAP1.)* On reload, what
  happens to live module/server state? Direction: **preserve module state when the
  type surface is unchanged** (swap code, keep data — the Erlang/Elixir model); when a
  reload changes types/layout in a way that would make existing state ill-typed, fall
  back to a **clean, announced restart** rather than reinterpret stale bytes. Never
  silently keep state that no longer matches its type — that would be a memory-safety
  (I1) hole. The boundary at which a swap is attempted (function / module /
  whole-program) is the other half of D-HOTSWAP1.
- **4d — Runtime type server API.** *(Epoch 3.)* How editors/agents query types from
  the live process (carried over from the open questions above).
- **4e — Dev↔release consistency guarantee.** *(Epoch 3, ballot D-DEVMODE1.)* A
  program must behave **identically** whether run through the dev runtime
  (interpreter / JIT hot-swap) or the release build (rustc native binary). Enforced by
  a `tests/` mode that runs every golden example through **both** paths and **diffs
  output**; any mismatch is a **release blocker**, not a warning. This protects I5
  (examples are the executable spec) across two execution backends, and keeps "works in
  dev, breaks in release" — the classic JIT/AOT divergence trap — out of Jet by
  construction. Ratifying this as a hard rule is part of D-DEVMODE1.

## Non-goals until Epoch 3

- No JIT in `jet dev` save loop for Epoch 2.
- No "run TS in Jet" bridge — typed Jet source only.
