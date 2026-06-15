# E2-M10 — Networking and services

**Status:** draft — **blocked on D-NET1…D-NET3** (Group M10). Governed by the
ratified concurrency lock (E2-V5 / S53: tasks + channels, no async).
**Depends on:** E2-M1 (tasks/channels), E2-M7 (streaming I/O), E2-M9 (`jet.log`).
TLS depends on the E2-M14 FFI tier. Unblocks E2-M12 (service observability) and
the E2-M17 service showcase.
**Error codes:** E28xx block (claim in docs/spec/diagnostics.md).

## Goal

Enter Go's territory with **blocking tasks/channels, not async syntax**. The
honest positioning, written into the docs: blocking thread-per-task services are
right for the broad enterprise/internal-service case (hundreds of connections),
not 100k-connection async workloads (E2-V5/V7).

## Owner decisions — ratify before any code

| ID | Question | Rec | Default if deferred |
|---|---|---|---|
| D-NET1 | TLS/HTTP dependency | **A** — rustls-class via the FFI tier, never hand-rolled | A |
| D-NET2 | Server concurrency story | **A** — blocking thread-per-task + channels | A |
| D-NET3 | Service showcase backing store | **A** — sqlite-first | A |

## Scope

- **Blocking sockets.** TCP/UDP with timeouts and clean shutdown.
- **HTTP client** over streaming I/O (E2-M7) — calls a real API.
- **HTTP server** for small services; request limits, graceful shutdown.
- **TLS (D-NET1).** Via a vetted Rust library (rustls-class) through the E2-M14
  FFI tier. Never hand-roll crypto (cross-ref `jet.crypto` D-LR3).
- **Worker patterns.** Channel-based workers; timeouts compose with `jet.time`.
- **Structured logging.** Integrate `jet.log` (E2-M9).
- **Config/env conventions** without a framework (read env, typed config struct).
- **Service example** backed by sqlite (D-NET3) or a durable file store,
  depending on E2-M14/E2-M9 timing.

## Honesty diagnostic / docs note (example)

```
note: `jet serve` uses one task per connection. This is excellent for internal
      services and tools at hundreds of concurrent connections. For very high
      connection counts, Jet is not the right tool yet — see docs/services.md.
```

## Diagnostics to register

- **E2801** socket/bind/connect failure names the address and operation.
- **E2802** TLS handshake/cert error in Jet words (not an openssl dump).
- **E2803** request body exceeds the configured limit (names the limit).
- **L2801** advisory: blocking call on the accept loop without a worker; suggest
  spawning a task.

## Examples & tests

- `examples/features/44_http_client.jet` — fetch and parse a real API response.
- `examples/features/45_http_server.jet` — concurrent server with tasks/channels.
- A loopback integration test (client ↔ server) under `cargo test`.
- A TLS smoke test through the FFI tier (gated on E2-M14).

## Out of scope

- Async/await, event loops, `select` over sockets (E2-V5).
- A full web framework, routing DSL, or template engine.
- HTTP/2 / HTTP/3, websockets (revisit post-epoch on evidence).
- Hand-rolled TLS or crypto (always via vetted dependency).

## Exit criteria

- HTTP client example calls a real API.
- HTTP server handles concurrent requests with tasks/channels.
- TLS works through vetted dependencies.
- Docs state the scalability model honestly (blocking, not 100k-async).
- `nix develop -c cargo test` green.
