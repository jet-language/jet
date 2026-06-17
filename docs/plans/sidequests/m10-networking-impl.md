# Sidequest: E2-M10 — Networking and services implementation

**Plan:** `docs/plans/epoch-2/m10-network-services.md`  
**Status:** all decisions ratified; ready to implement after M7 + M9  
**Depends on:** E2-M1 ✅ (tasks/channels), E2-M7 (streaming I/O), E2-M9 (`jet.log`), E2-M14 ✅ (FFI tier for TLS)

## Critical amendment: D-NET1 / D-DEP1

**TLS is NOT a compiler crate.** The original plan assumed a rustls-class Rust crate would be linked at build time. The ratified approach (D-NET1 + D-DEP1) is:

> TLS via `rustls` delivered as the **`jet.tls` package** — an FFI-wrapping Jet package using `extern rust "rustls@<ver>"`. The compiler stays zero external crates (I6). `jet.http` depends on `jet.tls`.

This changes how M10 is implemented:
1. `jet.tls` is a first-party package in the jet.* ring (ships with M9 or M10)
2. It wraps `rustls` via the E2-M14 C/Rust FFI tier
3. Users add `jet.tls#<ver>` to their `payload.jet` deps; it is not ambient
4. `jet.http` takes `jet.tls` as an optional dep for HTTPS

## Other decisions (no amendments needed)

| Decision | What to implement |
|---|---|
| D-NET2 | S53 tasks/channels only; Go-scale async → Epoch 3 |
| D-NET3 | sqlite-first service showcase |

## Honesty note (required in docs)

M10 must include the documented honest positioning:

> "jet serve uses one task per connection — excellent for internal services and tools at hundreds of concurrent connections. For very high connection counts, Jet is not the right tool yet."

This text must appear in the service docs and optionally as a build note.

## Diagnostics to register (E28xx)

E2801 (socket/bind/connect failure), E2802 (TLS handshake error in Jet words), E2803 (body exceeds limit), L2801 (blocking accept loop advisory).

## Exit criteria

See `m10-network-services.md`. Key: HTTP client calls real API; HTTP server handles concurrent requests via tasks; TLS via `jet.tls` package (not compiler dep); docs state scalability model honestly. `nix develop -c cargo test` green.
