# Service: JSON HTTP

Build a loopback HTTP service with `/health`, `/ready`, and one JSON POST
endpoint. Exercise concurrent requests, readiness timing, bounded body size,
malformed JSON, slow clients, and graceful shutdown. The response and errors
must match the canonical JSON contract. Beginner mode uses safe bind and
limits. Expert mode exposes bind, worker, timeout, and shutdown controls.
