# Owner ballot results

_submitted 2026-06-26 09:05 · **ratified 2026-06-26**_

Ratified into [`syntax-decisions.md`](../../../../docs/spec/syntax-decisions.md)
(decision-log entry dated 2026-06-26), card stripped from
[`decision-ballots.md`](decision-ballots.md), board card c157 advanced. Nothing
pending — kept as the submission record only.

## Decisions (ratified)

**D-NETDEP1 = A** — approve a small pure-Rust HTTP crate (`ureq`/`minreq`,
runtime-side, owner-gated, I6 holds) to back D-CTEFFECT1's build-time
`fetch(url, sha256:)`; hash-pinned in `.jet/lock`, carries the native-ize
obligation. Card c157's `fetch` backend → unblocked.
Comment: I want a full, complete http library, both server and client, better than go as part of the core libraries in jet.

**Owner-expanded mandate (recorded):** the end goal is a **full, complete HTTP
library — client and server, better than Go's `net/http` — as a Jet core
library.** The approved crate is the bootstrap; the native-ize end-state is a
first-party Jet HTTP stdlib. New core-library track opened (c164); the
client+server API surface gets its own design + ballots before that code is
written.
