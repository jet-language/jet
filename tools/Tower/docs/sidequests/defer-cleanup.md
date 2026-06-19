# defer for deterministic cleanup (re-examination)

**Status:** Draft plan — needs owner review (2026-06-19)
**Card:** c21

## Problem & why it's being reconsidered

`defer` schedules a statement to run when the enclosing lexical scope exits
— LIFO order, on every exit path (return, `?`, break, fall-through, panic),
infallible. Card c21 asks for the Zig/Swift *block-scoped* model.

It was declined once: **D-SUGAR5** (2026-06-16) — "`defer` keyword declined;
RAII (S63) is the cleanup story." S63 (2026-06-12) made automatic scope-end
cleanup the single user-facing story and explicitly rejected `defer`-as-primary
(leak-by-omission, Go's perennial bug class) and `with`-blocks (nesting
pyramids). S63 did note `defer` as a *potential later complement* for
non-resource actions, owner-gated, never required for correctness.

The honest reason to look again: RAII as shipped today covers **std resource
types only**. A user cannot write their own scope-exit cleanup. So the question
is not "RAII vs defer" in the abstract — it's "what do users do *right now*
when they want a cleanup action that isn't a std file/socket close?"

## What RAII (S63) covers today — and the concrete gaps

**Covered, well.** Std resource types carry Drop and clean up on every exit
path. Verified in `Source/Prelude/Std.rs:334-335`: `FileReader`/`FileWriter`
are RAII and "Drop closes (and flushes) them on every exit path — including
`?` early returns and panics." `TcpStream.close` lowers to `drop(...)`
(`Source/Codegen/Expression.rs:874`).
Example `examples/features/49_stream.jet` shows `files.open(src)?` with no
explicit close — the handle closes on every path. For files, sockets, tasks
this is genuinely better than defer: nothing to forget, no leak-by-omission.

**The gap — verified, load-bearing.** There is **no user-implementable Drop /
destructor / `deinit` trait** in Jet today. Grepping
`docs/spec/syntax-decisions.md`, `spec.md`, `Source/Syntax.rs`, `Source/M9.rs`,
and `Source/Sema/*` for `Drop`/`deinit`/`destructor`/`on_drop` finds only the
internal codegen `drop(...)` and Rust-side tooling Drops in `Source/Jetpack/`
(not user-facing Jet). User traits (S28) exist, but Drop is not among the
traits a user can `impl`. So when a user wants scope-exit
cleanup for something that is **not** a std resource type, RAII offers them
nothing — they must hand-place the cleanup call before every exit path.

Concrete cases where a user reaches past RAII today, each with its current
workaround:

1. **Restore a mutated flag / counter on the way out.** e.g. set
   `depth += 1` on entry, must `depth -= 1` on *every* return.
   *Workaround:* duplicate the decrement before each `return` / `?`. Fragile;
   `?` early-returns make it easy to miss one.

2. **Logging / timing a span.** "entered X" … "left X (took …ms)".
   *Workaround:* manual log before each exit. Same fragility.

3. **Tearing down a non-std handle** (a third-party C-FFI handle, a temp dir,
   a lock you took manually). *Workaround:* wrap it in a struct — but the
   struct *cannot* get a Drop today, so this doesn't actually work. The user
   is stuck calling teardown by hand on each path, or leaking on `?`.

4. **errdefer — cleanup only on the error path** (roll back a half-built
   resource if construction fails partway). *Workaround:* none clean. RAII
   runs on *all* paths and can't distinguish success from error; a plain
   defer also runs on all paths. This is the one capability with no RAII
   analog at all.

Note what is **not** a gap: "infallible" is parity, not a defer win. RAII Drop
is also infallible and swallows errors; fallible cleanup (`flush()?`) needs an
explicit call in *both* models. defer earns no credit there.

The honest read: gaps 1–3 are real but share one root cause — **users can't
write their own Drop.** Gap 4 (errdefer) is the only one defer-the-keyword
uniquely answers, and it's arguably an anti-pattern (the resource's own
constructor/Drop should cover partial-build rollback).

## Prior art (terse)

- **Go `defer`** — *function-scoped*, not block-scoped. Runs at function
  return, LIFO. Classic bug: `defer` inside a loop accumulates until the
  function ends (file handles pile up). A real cost, not a footnote.
- **Zig `defer` / `errdefer`** — *block-scoped*, LIFO. `errdefer` runs only
  when the block exits via error. The model card c21 describes. errdefer is
  Zig's headline cleanup feature and the capability RAII can't express.
- **Swift `defer`** — *block-scoped*, LIFO, runs on every exit including
  `throw`. No errdefer variant.
- **C++ / Rust RAII** — destructors run on scope exit, all paths, LIFO by
  reverse-declaration. No `defer` needed; the type carries cleanup. This is
  Jet's S63 model — but in C++/Rust the user can *write* destructors, which
  is exactly the capability Jet lacks today.

Takeaway: defer's ergonomic win over RAII evaporates once the user can write
their own Drop. The languages that have both (Rust) lean on RAII and barely
miss defer. Go/Zig reach for defer largely *because* per-block guard types are
heavier to write there.

## Proposed design IF added (worked Jet example) — kept conditional

This section is conditional; the recommendation below leans against adding a
keyword. If the owner did add an expert-tier `defer`, the least-bad shape:

- Block-scoped (Zig/Swift), LIFO, runs on all exit paths including `?` and
  panic. **Not** Go's function scope.
- Infallible body only — a `defer` body may not contain `?` (fallible
  cleanup stays an explicit call). Diagnostic if it does.
- Expert-tier framing (it's a footgun-adjacent control-flow feature):
  available but not in beginner onboarding, mirroring S63's "owner-gated
  complement" note.

```jet
fn parse(input: String) -> Tree ? ParseError {
    depth := 0
    depth = (depth + 1)
    defer depth = (depth - 1)   // runs on every return below, LIFO

    if input.is_empty() {
        return err(ParseError.Empty)   // defer fires here
    }
    node :: parse_node(input)?         // …and here, on the `?` early return
    return ok(node)                    // …and here
}
```

But see the third option — the same effect with **zero new syntax**.

## Implementation sketch — pipeline touchpoints (if a keyword were added)

- **Syntax.rs (I7):** new `defer` keyword + decision ID. Currently absent
  (verified — good).
- **Lexer:** keyword token.
- **Parser:** `defer <statement>` as a statement form; reject `defer` at item
  position.
- **Sema:** scope tracking already exists (`Source/Sema/Registration.rs`
  scopes stack). Record deferred statements per scope; check body is
  infallible (no `?`); ownership-check the captured values (a deferred body
  reads locals — must not move what's used later).
- **Codegen:** the hard part. Lower to a guard struct per `defer` whose `Drop`
  runs the body, *or* emit the body before each exit edge. Guard-struct
  lowering reuses existing Drop machinery (FileWriter pattern). LIFO falls out
  of reverse-declaration drop order. Panic paths covered by Drop automatically.
- **Diagnostics (I4):** at least E-codes for `defer` with `?` inside, and for
  `defer` capturing a moved value. Each needs a `tests/ui` snapshot.
- **Examples (I5):** a `defer` feature example + golden output.

## Test plan

- `tests/ui`: `defer` body containing `?` → teaching error; `defer` capturing
  a later-moved local → ownership error.
- Golden example exercising LIFO order + firing on `?` early return + normal
  return + (if testable) panic via process exit code.
- `tests/decisions.rs`: the keyword must be ratified before it appears in
  `Source/Syntax.rs` (ratification enforcement).
- For the **third option** (closure-guard), the test plan collapses to: a
  stdlib `Guard`/`Defer` type with a Drop that runs a stored lambda, one
  feature example, and an ownership snapshot for the captured closure — no
  parser/lexer/keyword tests at all.

## Risks & invariant check

- **I8 (simplicity ratchet) — the central risk.** defer was already declined
  once. Re-opening needs a gap with no good workaround. The gap is real
  (no user Drop) but the *cleanest* fix is user-definable Drop, not a new
  control-flow keyword. Adding `defer` would give Jet two cleanup mechanisms
  where philosophy priority #4 (one mechanical path) wants one. That's the
  strongest argument to stay declined.
- **Priority #2 (beginner experience).** S63's whole point: "when a value
  goes out of scope, Jet cleans it up" is one sentence. `defer` adds a second
  cleanup concept and reintroduces leak-by-omission (forget the `defer`,
  leak the cleanup) — the exact Go bug class S63 rejected.
- **I1.** No unsafe implications; defer is safe control flow.
- **I7.** Keyword would need a Syntax.rs row + decision ID (not present today).
- **I3/I2.** Codegen-only concern is keeping the guard-struct lowering dumb;
  no rustc-as-checker.

## Open decisions

1. Is the real ask **user-definable Drop** rather than `defer`? If users could
   `impl Drop` (or a `Cleanup` trait) on their own types, gaps 1–3 close via
   the existing single mechanic (S63), no new keyword. This may be the actual
   roadmap item hiding behind c21.
2. Does **errdefer** (gap 4) survive on its own merit, or is partial-build
   rollback better handled by the constructor returning the resource only on
   success (so there's nothing to roll back)?
3. If anything ships, is it a **keyword** or a **stdlib guard value**? The
   guard value needs no syntax and no ratification.

## Proposed decision card(s)

### D-DEFER1 — `defer` for deterministic cleanup (rec B)

You declined `defer` once (D-SUGAR5) in favor of RAII (S63). RAII works well
for std resource types, but a user **cannot write their own scope-exit
cleanup today** — there's no user-implementable Drop. So when someone wants
to restore a flag, log a span, or tear down a non-std handle on every exit
path (including `?`), they hand-place the cleanup before each return and can
miss one. This card asks how to close that gap.

- **Option A — keep declined; RAII (S63) stays the only cleanup story.**
  No new surface. The gap closes later via *user-definable Drop* (a separate
  roadmap item), not a control-flow keyword. Today's workaround for a flag is
  explicit:

    ```jet
    fn parse(input: String) -> Tree ? ParseError {
        depth := 0
        depth = (depth + 1)
        if input.is_empty() {
            depth = (depth - 1)        // restore before THIS error return
            return err(ParseError.Empty)
        }
        node :: parse_node(input)?     // `?` exits WITHOUT decrementing — the missed path
        depth = (depth - 1)            // success path only
        return ok(node)
    }
    ```

  Honest cost: the restore is duplicated per explicit exit, and the `?` on the
  `parse_node` line silently skips it — the exact leak-by-omission this card is
  about. Honest benefit: one cleanup concept, S63 intact.

- **Option B — add a stdlib `Guard` value (no new syntax).** Ship a
  `core` type whose Drop runs a stored lambda (S46/S47 closures already
  exist; FileWriter's Drop already proves runs-code-on-scope-exit works). The
  user binds a guard; it fires on every exit path, LIFO with other Drops:

    ```jet
    use core.scope as scope
    fn parse(input: String) -> Tree ? ParseError {
        depth := 0
        depth = (depth + 1)
        _g :: scope.guard(() => { depth = (depth - 1) })  // fires on every exit
        node :: parse_node(input)?                    // restore runs here too
        return ok(node)
    }
    ```

  This delivers defer's ergonomics for gaps 1–3 with **zero new syntax**, on
  existing Drop + lambda machinery, and stays inside S63 (it *is* RAII — a
  value cleaning up at scope end). Cost: a deliberately-bound `_g`, and it
  can't do errdefer (no success/error distinction).

- **Option C — add an expert-tier `defer` keyword (block-scoped, Zig/Swift).**
  Real LIFO scope-exit on all paths; could later add `errdefer`. Worked
  example in the conditional design section above.

    ```jet
    fn parse(input: String) -> Tree ? ParseError {
        depth := 0
        depth = (depth + 1)
        defer depth = (depth - 1)      // LIFO, fires on every exit path
        node :: parse_node(input)?
        return ok(node)
    }
    ```

  Cost: a second cleanup concept beside RAII (fights priority #4), reintroduces
  leak-by-omission (the Go bug class S63 named), needs a keyword + parser +
  codegen + diagnostics + ratification. Benefit over B: no bound name, and the
  door to `errdefer` (the one capability neither RAII nor a guard offers).

**Recommendation: B — ship the stdlib `Guard` now; the `defer` keyword (C)
stays declined; user-definable Drop is the real long-term roadmap item.** The
honest finding is that the gap behind c21 is *not* the absence of `defer` —
it's the absence of user-writable cleanup. Option A alone leaves the gap open
(its own example shows the `?` leak). Option B closes gaps 1–3 *today* with
zero new syntax and zero ratification, on existing Drop + lambda machinery, and
stays inside S63 (a guard value *is* RAII) — the I8-cleanest move. The keyword
(C) is the one to keep declined: it adds a second cleanup concept beside RAII
and reintroduces the Go leak-by-omission bug class S63 named. Longer term, when
a user can `impl Drop` on their own type, even the Guard's bound `_g` becomes
optional and `defer`'s ergonomic edge largely vanishes (Rust's experience:
with RAII, `defer` is barely missed). Option C should stay declined
unless **errdefer** (partial-build rollback) proves to be a recurring real
need that user-Drop genuinely can't serve — and even then it's an expert-tier
add, never beginner surface, exactly as S63 framed it.
