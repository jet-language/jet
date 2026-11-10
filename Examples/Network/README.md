# Network

HTTP clients and servers, sockets, URLs and browser automation.

Run a lesson from the repository root, for example `jet run Examples/Network/<Lesson>.jet`. Each lesson's expected output sits beside it as `<Lesson>.out`.

## Lessons

- [DNSLookup](DNSLookup.jet) — DNS PTR + well-known service port table (core.net).
- [IPPublicDestination](IPPublicDestination.jet) — Reachable versus safe to fetch.
- [IPSpecialProfile](IPSpecialProfile.jet) — IANA IPv4 Special-Purpose Address Space, updated 2025-10-09.
- [NetworkProjection](NetworkProjection.jet) — Loopback TCP/UDP, readiness, typed addresses, and DNS records.
- [SocketAddrs](SocketAddrs.jet) — `socket_addrs` returns every address a host names, in the resolver's order; a numeric host is its one address.
- [SocketEcho](SocketEcho.jet) — Typed addresses, TCP echo, UDP datagrams, Unix sockets.
- [TLSPeerIdentity](TLSPeerIdentity.jet) — A real loopback TLS handshake uses only the checked-in test CA, then exposes the verified name, chain and version.
- [URLMIME](URLMIME.jet)
- [URLSourceLiteral](URLSourceLiteral.jet) — No import: the typed URL head loads its source-owned carrier and constructor.
- [WSEcho](WSEcho.jet) — core.net.ws client/server echo over cleartext WebSocket.

## Browser

- [BrowserAgent](Browser/BrowserAgent.jet) — Agent-facing browser automation cookbook.
- [BrowserArtifacts](Browser/BrowserArtifacts.jet) — Storage, files, screenshots, and PDF.
- [BrowserCDP](Browser/BrowserCDP.jet) — Checked expert CDP supplement.
- [BrowserLifecycle](Browser/BrowserLifecycle.jet) — Browser/context/page/tab/frame lifecycle.
- [BrowserLocators](Browser/BrowserLocators.jet) — Semantic locators, actions, and deterministic waits.
- [BrowserLockedProvision](Browser/BrowserLockedProvision.jet) — Project-locked browser provisioning.
- [BrowserNetwork](Browser/BrowserNetwork.jet) — Events, network inspection, and interception.
- [BrowserPrivacy](Browser/BrowserPrivacy.jet) — Cleanup, profile isolation, privacy, and redaction.

## HTTP

- [HTTPClient](HTTP/HTTPClient.jet) — core.http is the one-shot rung; core.http.client is the configurable rung.
- [HTTPMessageValues](HTTP/HTTPMessageValues.jet) — Checked HTTP message values: methods, statuses, versions, fields and bodies.
- [HTTPObservability](HTTP/HTTPObservability.jet) — Static responses and access records share the server surface.
- [HTTPRestService](HTTP/HTTPRestService.jet) — Realistic core.http client + server, no third-party package.
- [HTTPRouteMarkers](HTTP/HTTPRouteMarkers.jet) — Tags expose only this explicitly mounted value.
- [HTTPRoutes](HTTP/HTTPRoutes.jet) — HTTP router — typed path inputs, 404/405.
- [HTTPServeEntry](HTTP/HTTPServeEntry.jet) — `http.serve(mux, deadline:)` binds a loopback server whose route deadline is a per-request #Context; `shutdown` drains it and returns the shutdown report.
- [HTTPServer](HTTP/HTTPServer.jet) — core.http.server: function-first mux with path params, served on a loopback listener and answered for a client in the same program.
- [HTTPServerDate](HTTP/HTTPServerDate.jet) — Every server response carries one Date header in IMF-fixdate form, read from the program clock; a handler-set Date passes through unchanged.
- [HTTPServerLifecycle](HTTP/HTTPServerLifecycle.jet) — One bound Server owns bounded serve and graceful shutdown.
- [HTTPServerLimits](HTTP/HTTPServerLimits.jet) — Reject ambiguous framing and oversized bodies before handlers run.
- [HTTPServerMiddleware](HTTP/HTTPServerMiddleware.jet) — Middleware composes on the same Handler server engine.
- [HTTPServerTrailers](HTTP/HTTPServerTrailers.jet) — Trailers live on HTTP messages, not Body.
- [HTTPServerWASIP2](HTTP/HTTPServerWASIP2.jet) — The same HTTP server Prelude runs as a WASI Preview 2 Component.
- [HTTPWebDefaults](HTTP/HTTPWebDefaults.jet) — The three defaults a browser front end plus a Jet API needs.
