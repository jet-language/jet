# Broad gated build-time I/O (D-CTIO1 option C) — far horizon

**Status:** far-horizon idea — no card, no ballot. Parked per owner comment on the
D-CTIO1 ballot (2026-06-22): "Put C as an option to consider on an idea card for
far horizon."

D-CTIO1 ratified **option B**: comptime build-time I/O is limited to read-only
`embed_file(path) -> String` and `embed_bytes(path) -> [U8]`, with the path a
string literal resolved relative to the source file and no `..`-escape past the
project root. That implements the one blessed exception S26/S60 already name.

**Option C — deferred to here.** Broad *gated* build-time I/O: allow comptime code
to read env vars, hit the network, run a subprocess, or codegen at build time
(Jai's `#run` / Zig `@embedFile`-plus territory), behind a sandbox + an auditable
`.jet/build-io.lock` of every accessed path + cache-invalidation on change. Powerful
(full build scripting without a separate build step), but it adds a supply-chain
attack surface that the S26 "no ambient I/O at comptime" law was written to refuse —
the Nim/Jai evidence shows un-auditable spread once it ships.

**Revisit when:** post-Epoch-3, *if* a concrete codegen/build-script need appears
that `embed_file`/`embed_bytes` plus the normal build pipeline genuinely cannot
serve. Any revival must ship the sandbox + lockfile + cache-invalidation story in
the same ballot, not after. Until then, B is the ceiling.
