# D-WS-ROUTE1 review: a WebSocket route kind on the HTTP mux

Date: 2026-11-10. Author: BallotLead-3 (Claude Opus). Compiler: a0.0.43 (dev c460ba91e5).
Board: ballot D-WS-ROUTE1. Earlier pass: BallotFix REPORT.md, section D-WS-ROUTE1.

## Question

Should the mux gain `mux.websocket(pattern, handler)`? The server would check the handshake, and the handler
`fn(HTTPRequest, WsConn) WsError!` would return nothing.

## Findings that changed the ballot

1. **The serial server is a defect against a ratified decision, not an A gain.** New probe w04 holds one
   WebSocket open for 400 ms and asks `GET /hi` at 50 ms. It prints `GET /hi answered at 400 ms`.
   `HTTPServer.serve` serves each accepted connection to the end (`Core/Http/Server.jet:262-289`), and its
   shutdown comment says "Requests are answered one at a time" (`:295`). But D-HTTP-SERVER2=A, ratified
   2026-07-11, says: "global connections 10k with per-IP 256, handler concurrency bounded by task scheduler.
   Over-limit returns 400/408/413/431/503". So the loop breaks the ratified server, and its fix serves every
   option.
   - The draft said "Under B, C and D … each handler must start its own task". That is false once the server
     meets SERVER2, because each connection already has its own task.
   - A's own cap ("default 256, then 503") would have been a second limit beside SERVER2's. It is dropped.
   - A's gain "an open socket no longer stops the server" becomes "tls: with a WebSocket route fails at bind".
   - No open card covers the serial loop. #4635, "http.serve must route through the Jet-owned server runtime",
     is the nearest, and its criteria do not cover concurrency.
2. **The marker form lacked WebSocket.** D-HTTP-ROUTE-EXPOSE1=A gives every verb a marker read by
   `mux.mount` (`Server.jet:25-75`: Get, Post, Put, Delete, Patch, Head, Options). A now adds
   `#Server.WebSocket(path)`, so the call form and the marker form agree.
3. **Wording and lines.** "We recommend" and "your handler" are removed. The upgraded reply is skipped at
   `Server.jet:1159-1196`. The claim cell is `Core/Http/Http.jet:271-277`. The TLS identity check at bind
   is `Server.jet:374-377`.

## Probes (a0.0.43; outputs in probes/out43)

| Probe | Result |
|---|---|
| w01 WSEcho | `echo:ping` |
| w02 `mux.websocket` | E0102 |
| w03 A's handler body | `handler checks` |
| w04 GET behind an open socket (new) | `echo:ping`, `GET /hi answered at 400 ms (asked at 50 ms; socket held until 400 ms)` |

Census (unchanged): `ws.upgrade(` appears in 12 lines in 11 Jet files. Eight are HTTP handlers that return a
fake response.

## Peers (sourced)

- Axum: `WebSocketUpgrade` extractor, `on_upgrade`. <https://docs.rs/axum/latest/axum/extract/ws/index.html>
- gorilla/websocket: `Upgrader.Upgrade` inside a plain handler. <https://pkg.go.dev/github.com/gorilla/websocket#Upgrader>
- FastAPI: a separate `@app.websocket` route kind. <https://fastapi.tiangolo.com/advanced/websockets/>

## Adversarial review (per claim)

| Claim | Attack | Verdict |
|---|---|---|
| One open socket stops the server | w04 | Holds: 400 ms wait |
| Only A can run sockets concurrently | D-HTTP-SERVER2=A | **Failed**: ratified concurrency covers every option |
| A needs its own 256 cap | SERVER2's limits | **Failed**: second limit. Dropped |
| One route form | ROUTE-EXPOSE1 markers | **Failed**: marker missing. Added |
| TLS refused at bind | `__bind_entry` | Holds: same path as the certificate check |

## Remaining losses

- The mux gains a second handler type and one more route marker.
- TLS connections cannot upgrade under any option.

## Different-family review (GPT, 2026-11-10)

Independent results: w01 prints `echo:ping`; w02 is E0102; w03 prints `handler checks`; w04 prints GET answered at 400 ms. These show today's behavior and a handler body, not an implemented A.

| Claim | Attack, evidence and repair | Verdict |
|---|---|---|
| A's handler needs no unused response | w01 and w03 independently run. The current server discards the reply after the upgrade claim; origin/dev Http.jet:271-277 and Server.jet:1159-1196 confirm the state. | Holds as a proposal; not implemented registration. |
| A fixes concurrency | Read ratified D-HTTP-SERVER2=A and reproduced w04. Concurrency is already required for all options. | Fails as an A-only benefit. |
| Refusing TLS at bind designs away the TLS problem | Http.jet's claim cell owns a plain descriptor, not TLS. A secure server is still impossible if refused earlier. | Fails. All options must repair the shared transport ownership; no insecure-only release is authorized. |
| One more route type is harmless | GET collisions, HEAD fallback, middleware and shutdown were unspecified. | Holds only with exact conflict rules, no HEAD upgrade, pre-handshake auth/Origin policy, and server-supervised upgraded lifetimes. |
| Every error closes with 1011 | A peer may already be closed, or a protocol error may prescribe another code. | Fails. Preserve protocol codes and closed state; 1011 applies to application failures while writable. |
| B/C/D exhaust the strong alternatives | Axum's on_upgrade returns a real response and an owned callback; FastAPI uses an explicit socket route. Neither requires a fake post-upgrade HTTP response. | Fails. Added E: one typed response-or-upgrade outcome, clean-cutover all handlers. |
| OpenAPI describes the socket | A GET 101 operation can describe the HTTP handshake, not messages sent later. | Holds within stated limits; wording narrowed. |

Read the cited ratifications directly: D-HTTP-SERVER2=A, D-HTTP-ROUTE-EXPOSE1=A, D-WS1=B and D-HTTPDEPTH1=A. Read origin/dev Server and Http plus Axum and FastAPI sources. The earlier census is historical, not independently remeasured or used in this recommendation.

Agree with repaired A for explicit socket routes. E is stronger for mixed HTTP/socket handlers and has a real whole-handler migration cost. No performance advantage was measured. Do not post TLS refusal as a fix or w03 as end-to-end proof.
