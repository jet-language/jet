# Epoch 3 pillar — async networking & Go-scale concurrency

**Status:** owner-ratified direction (2026-06-16, D-NET2). **Epoch 3 pillar.**

## Goal

Ship a typed **`@async` / `@await`** runtime (reserved in S82) so Jet services
can handle **100k+ idle connections** and CPU-heavy concurrent workloads without
giving up memory safety or Jet-owned diagnostics.

## Epoch 2 vs Epoch 3

| | Epoch 2 (ships) | Epoch 3 (this doc) |
|---|---|---|
| Model | S53 **tasks + channels** (E2-V5) | Integrated async runtime |
| Scale target | Internal services, thousands of connections | Public APIs, 100k+ websockets |
| Syntax | blocking I/O + worker tasks | `@async { await … }` blocks |
| Honest line | "Go circa 2012" | "Go-class, but typed + safe" |

Epoch 2 **does not** block on this pillar — E2-M10 ships blocking HTTP/TLS with
task-per-request where needed.

## Likely building blocks

- `@async` / `@await` (S82 statement form, Epoch 3 activation).
- Netpoll / epoll / io_uring-backed executor (implementation TBD).
- Integration with `Fallible` + `?` (no callback-colored types in user surface).
- TLS and DB drivers remain FFI-bridged; async wraps blocking bridges first,
  native async I/O later.

## Open design questions

- Green threads vs work-stealing pool vs hybrid.
- Whether `@async fn` whole-function form mirrors `@transact`.
- Migration path from Epoch 2 `server.on_request(task => …)` APIs.

## Non-goals

- No full async rewrite inside Epoch 2.
- No `unsafe` callback bridges in user code.
