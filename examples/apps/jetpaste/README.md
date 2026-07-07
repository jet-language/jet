# jetpaste

Paste-service implementation slice. Current deterministic mode proves SQLite
persistence through loopback HTTP create/read routes without a fixed port. It
also exposes health and stats probes plus deterministic IDs/TTL. Router-captured
state is blocked by today's `core.http` handler surface, so this slice uses
manual dispatch over `HttpRequest.method()` and `.path()`.
