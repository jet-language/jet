# Plan: Detached-task idiom — fix L1101 on the server pattern (D-DETACH1)

**Status: plan — awaiting owner decision D-DETACH1.**

Unblocks: **Tariq** (HTTP server), any long-running server/daemon author.

---

## Goal

Every server pattern today trips **L1101** ("Task value dropped without
`.join()`") — verified in both `57_http_server.jet` and the `http_service`
showcase. The warning is *correct* for an accidental drop but *wrong* for a
deliberately-detached daemon task the program intends to outlive the current
scope (or run for the process lifetime). Give the author a one-word way to say
"I meant to detach this" so the lint goes quiet without suppressing real bugs.

Verified: L1101 is emitted from `Source/Sema/CheckerOwnership.rs:221` and
`CheckerCore.rs:739`; documented in `diagnostics.md:279,418`. There is no
`detach` verb today (`grep detach Source/` → nothing).

## Pipeline touch points

- **sema** (`CheckerOwnership.rs`): a detached task is exempt from L1101. Needs a
  way to mark a `Task` value as intentionally not-joined.
- **stdlib / parser**: depends on the chosen surface — a `task.detach()` method
  (stdlib + sema recognizes it), a `#detach` marker (parser + sema), or a
  distinct spawn verb (`detach { … }` vs `spawn { … }`).
- **codegen**: a detached task lowers to a fire-and-forget thread/handle that is
  not joined; for a process-lifetime server task, the runtime must keep it alive.
- **diagnostics**: L1101 text gains a "if intentional, detach it with …" fix-it.

## Invariants in play

- **I1** safety: detaching must not let a task read freed state. A detached task
  may only capture owned/`share` values, not `view` borrows of the caller's
  scope — otherwise it is a use-after-scope. Sema must enforce this.
- **I4** the L1101 fix-it must name the ratified detach spelling; snapshot it.
- **I8** prefer one verb over a family of spawn modes.

## Open questions (need owner decision — D-DETACH1)

1. **Surface** — `task.detach()` method on the handle, a `#detach` marker on the
   spawn, a dedicated `detach { … }` block parallel to `spawn { … }`, or a
   `spawn(detached: true)` named arg.
2. **Capture rule** — what may a detached task capture? Proposed: owned + `share`
   only; capturing a `view` is a compile error (names the borrow).
3. **Lifetime** — does a detached task block process exit, or may `main` return
   while it runs? (daemon vs background-worker semantics; the HTTP server wants
   "runs until the process is killed").
4. **Is L1101 still a warning or an error for non-detached drops** once an opt-out
   exists? (Leaning: keep it a warning; detach silences it.)

## Test plan

1. `examples/features/detached_task.jet` — spawn a detached worker, main returns
   cleanly, no L1101; golden output.
2. Negative: a detached task capturing a `view` → compile error snapshot.
3. Regression: an *accidental* dropped (non-detached) task still fires L1101.
4. Re-bless `57_http_server.jet` once the server idiom adopts detach (no warning).
