# M11 — Concurrency: tasks & channels

**Blocked on decisions:** S53 (spawn/channel surface). Depends on M8
(closure capture machinery does the heavy lifting) and M9 (generic
`Task[T]`/`Channel[T]` signatures).
**Error codes:** E1101+.

## Goal

Threads without data races, using the asset we already own: the
ownership checker. Model = structured tasks + message passing. No shared
mutable state in v1 — the compiler *proves* it, which is the whole pitch
("fearless concurrency without lifetime syntax").

## Surface (uses ballot recommendations — substitute ratified choices)

```jet
import "std/tasks" as tasks;

fn main() {
    // 1. Tasks: spawn a closure, join for the value.
    val t = tasks.spawn(() => slow_sum(1, 1000000));   // Task[Int]
    val other = quick_work();
    print(t.join() + other);

    // 2. Channels: typed pipes between tasks.
    val ch = tasks.channel[Int]();        // Channel[Int]
    for i in 1..4 {
        val sender = ch.sender();          // clonable send half
        tasks.spawn(take(sender) () => { sender.send(i * 10); });
    };
    for _ in 1..4 {
        print(ch.receive() or break);      // T or Closed
    };
}
```

- `tasks.spawn(f) -> Task[T]` takes a zero-parameter closure. The
  closure **must own everything it captures** (M8 escape rules apply
  with no clone-fallback for `var`s: shared reads of clonable values
  clone with L0801; anything mutable or non-clonable needs `take`,
  else E1101 with the M2 human framing: "the new task might outlive
  `data`; give it its own copy (`.clone()`) or hand it over (`take`)").
- `t.join() -> T` waits; calling `join` twice is prevented by ownership
  (`join` is `take self` — second use is E0121, for free).
  A task whose closure panics: `join` re-reports the panic (runtime
  report, exit 70) — fail loud, not half-dead programs.
- Dropping a `Task` without `join` → L1101 lint ("the program may end
  before this task finishes; call `.join()`").
- `tasks.channel[T]()` → `Channel[T]` (the receive half) with
  `.sender() -> Sender[T]` (clonable). `sender.send(v)` moves `v` in
  (take parameter). `ch.receive() -> T or Closed` blocks; returns
  `err(Closed)` when all senders are gone (`enum Closed { Closed; }` —
  fits the M4 story; `or break` in a loop reads beautifully).
- Boundary rule: values crossing `spawn`/`send` must be **sendable**:
  every built-in and user type is, EXCEPT `view`-returned borrows,
  tier-2 `ref`-holding structs, and non-`take`n closures (E1102 names
  the offending field/capture).

## Sema rules

1. Spawn capture analysis = M8 escape analysis with the stricter
   no-mut-borrow rule (rule above, E1101).
2. Sendability is a recursive structural check, cached per type; the
   error walks the path ("`Config` contains `logger`, which holds a
   `ref` field — give the task its own copy").
3. `join`/`send` are `take self`/`take v` — existing M2 checking covers
   reuse errors with zero new diagnostics.
4. No `Mutex`, no shared memory in v1: a `var` captured by two spawns is
   simply E1101 twice. The error's fix text teaches channels ("let the
   tasks send results back instead").
5. Determinism in tests: examples must not depend on interleaving
   (sum/collect patterns; sort received values before printing).

## Codegen lowering

| Jet                       | Rust                                        |
|---------------------------|----------------------------------------------|
| `tasks.spawn(f)`          | `std::thread::spawn(move || …)` wrapped in a `JetTask<T>` prelude struct holding the `JoinHandle` |
| `t.join()`                | `handle.join()` — `Err` (panic) re-raises the runtime report |
| `Channel[T]`/`Sender[T]`  | `std::sync::mpsc::{Receiver, Sender}` in prelude wrappers |
| `receive()`               | `recv()` mapping `RecvError` → `err(Closed)` |

Sendability maps to Rust `Send` but sema proves it independently (R2);
a rustc `Send` error on generated code is an ICE and a sema bug.

## Diagnostics to register

E1101 task capture needs ownership (clone/take) · E1102 value isn't
sendable (with the path) · L1101 task never joined.
Teaching: E0040 `async`/`await` → not in Jet; tasks block · E0041
`mutex`/`lock` → channels ("share by communicating").

## Examples & tests

- `examples/26_tasks.jet` — parallel sum split across 4 tasks.
- `examples/27_pipeline.jet` — producer/consumer over a channel.
- ui fixtures: shared-var capture (E1101 + fixed-with-channel
  companion), unsendable struct (E1102), unjoined task lint.
- Golden tests with deterministic outputs (sorted collection of
  results); a stress test (1000 messages) under `cargo test -- --ignored`.

## Out of scope

async/await (post-v1 decision, likely never — blocking tasks + channels
cover the audience), Mutex/RwLock/atomics, scoped borrows into threads,
thread pools / work stealing, select over channels, timeouts (compose
with `std/time` later), parallelism in stdlib collection methods.
