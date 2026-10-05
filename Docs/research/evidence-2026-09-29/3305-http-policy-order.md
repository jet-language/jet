# #3305 — HTTP policy ordering outside browser form actions (evidence, 2026-09-29)

## Question

On a raw HTTP client over loopback, does Jet's HTTP server behave as follows?

- CORS never acts as authorization.
- Headers survive both normal and error responses.
- Oversized or malformed bodies are rejected before the handler runs.
- Forwarded identity can't replace the socket peer.

Which policy capabilities are missing, and so become owner gates?

## Method

- Witness: `~/.cache/jet-dev/scratch/Closer05/t3305/http_policy_order.jet`. It writes raw
  TCP requests (OPTIONS preflight from an allowed and a denied origin, POST with and without
  auth, 404, handler `Err`, a `Content-Length: 1048577` body, `Content-Length` plus
  `Transfer-Encoding: chunked`, and `X-Forwarded-For: 203.0.113.9`). Handlers count their
  invocations through a channel. The package grants `[IO, Mem.Alloc, Net, Panic, Time, Time.Wait]`.
- Baseline: `Examples/features/net/http_server_limits.jet`.
- Source read: `Core/http/server.jet:11-91` (committed) plus the uncommitted working-tree diff
  of `Core/http/server.jet` by another worker, `Core/http/http.jet:456-458`, and
  `crates/jet-codegen/src/Prelude/CoreLib/Top/HTTPServer.rs:577-596,4100-4102,5189-5194,5645-5651`.

## Inventory (source)

| Policy | Existing mechanism | Status |
|---|---|---|
| CORS | `cors_policy(origins)` / `cors(mux, policy)` delegate to the host provider | Exists. Not exercised (see "Execution") |
| Request id | `request_id(mux)` | Exists |
| Body limit | `max_body_bytes = JET_HTTP_MAX_BODY_BYTES`. `Content-Length` over the limit returns 413 while the request is read, before dispatch (HTTPServer.rs:4100-4102, 5189-5194). The chunked decoder enforces the same limit (:499-507) | Exists (source) |
| Framing | Ambiguous framing returns 400 (the `http_server_limits` golden asserts 400 with no handler run) | Exists (source/golden) |
| Handler error mapping | `HTTPError.BodyTooLarge`→413, `InvalidFraming`→400, `UnsupportedEncoding`→415, anything else→500 (HTTPServer.rs:5645-5651) | Exists (source) |
| Connection caps | `max_connections` 10,000 and `max_connections_per_ip` 256, keyed on the socket peer (:577-596, :2179-2205) | Exists (source) |
| Trust-proxy / forwarded identity | None in Core or HTTPServer.rs (grep `trust_?proxy\|x-forwarded\|forwarded` finds nothing relevant) | **Missing → owner gate** |
| Security-header policy (HSTS, nosniff, CSP, frame options) | None. `strict-transport-security` appears only in a header-name table (HTTPServer.rs:2778) | **Missing → owner gate** |
| Rate identity / rate limit | None apart from the per-IP connection cap | **Missing → owner gate** |
| Spec documentation of server body/transport limits | `Docs/spec/reference/core-library.md:956-976` lists the API but not the 1 MiB body limit, the 413/400 mapping or the connection caps | Missing doc (criterion 5) |

## Execution

No `core.http.server` program compiles on the current binary. The baseline
`Examples/features/net/http_server_limits.jet` fails the same way as the witness:

- `E2404 ? can't turn the Err from mux() into HTTPError | NetError | TaskFailure`: the
  committed `mux()` has an inferred fallible contract.
- `E0104 serve expects 4 arguments, got 2 --> Core/http/http.jet:457:22`:
  `core.http.server.serve(addr, mux)` calls the committed 4-positional `serve`.

An uncommitted working-tree edit of `Core/http/server.jet` by another worker changes
`mux()` to `Never!` and gives `serve`/`bind` keyword defaults. That edit would clear both
errors once it lands in a snapshot. The witness's own errors (`access_log` needed `??`,
handlers needed explicit `HTTPResponse HTTPError!` closure types, and a collecting-loop
form) were fixed in the witness. The run results on the pinned binary are in the JSON report.

## Owner gates to file (one ballot each, no API added here)

1. **Trusted-proxy identity.** An explicit mux option naming trusted proxy CIDRs. When the peer
   isn't trusted, `X-Forwarded-For`/`Forwarded` are ignored and identity is the socket peer.
   Recommended default: trust nobody.
2. **Security-header policy.** A mux-level header set (HSTS for TLS listeners, nosniff,
   frame-options, CSP opt-in) applied to every response, including 4xx/5xx and
   framework-generated 413/400.
3. **Rate identity and limiting.** A token bucket keyed on the resolved identity from gate 1
   (socket peer by default), returning 429 before the handler runs.

## Verdict

BLOCKED on the current binary for criteria 1 to 4 and 6. These are behaviour proofs that
need a running server, and no `core.http.server` program compiles. Criterion 5 is partly met
by source only: the limits are enforced at read time before dispatch, and TLS is separate
(`tls(cert, key)` / `bind(…, tls)`). The spec doc is still missing. The inventory and the
three gates above are delivered here.
