# Service: JSON HTTP

Build a real loopback HTTP service with `/health`, `/ready`, and one JSON POST
endpoint at `/sum`. The service must use a listening socket, accept concurrent
requests, delay readiness, enforce a bounded request body, reject malformed JSON,
handle a slow client, and shut down gracefully.

The input is a line-oriented interaction script. Blank lines and lines beginning
with `#` are ignored. The adapters must execute the same commands against the
running service. They must not print a precomputed answer.

```text
CONFIG|mode|bind|workers|timeout-ms|shutdown-ms|body-limit|ready-after-ms
GET|path|
POST|/sum|json-body
CONCURRENT|/sum|json-body|count
WAIT|milliseconds|
MALFORMED|/sum|partial-json
OVERSIZE|/sum|
SLOW|/sum|json-body
PUT|path|body
SHUTDOWN||
```

`CONFIG` is required for the frozen fixtures. Beginner mode uses a loopback
address, bounded workers, timeouts, shutdown grace, body size, and readiness
delay. Expert mode supplies those controls explicitly. Every bind remains
loopback-only. `CONCURRENT` must issue separate requests concurrently; the
worker value bounds service request work. `SLOW` must send headers with an
incomplete body and then close after the configured timeout. `SHUTDOWN` must use
the configured grace period and report a clean lifecycle.

The JSON contract is deterministic:

- `GET /health` returns `200` and `{"status":"ok"}`.
- `GET /ready` returns `503` and `{"error":"not-ready"}` before the delay.
- `GET /ready` returns `200` and `{"status":"ready"}` after the delay.
- A valid `POST /sum` body has `{"numbers":[1,2,3]}` shape and returns
  `200` with `{"sum":6}` (the fixtures also use other integer arrays).
- Malformed JSON returns `400` with `{"error":"malformed-json"}`.
- A body over the configured limit returns `413` with
  `{"error":"body-too-large"}`.
- An unknown route returns `404` with `{"error":"not-found"}`.

The stdout fixture records status and lifecycle facts in command order. The
Jet adapter uses Jet Core HTTP, networking, time, task, and JSON facilities.
The peer adapter is a pinned Go standard-library implementation of this same
service; it is not a claim that the adapter builds an upstream TechEmpower
application.
