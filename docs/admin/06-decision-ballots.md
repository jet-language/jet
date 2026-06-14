# 06 — Decision ballots (owner's queue)

Open syntax decisions for M3–M14. **Ratified choices live only in
docs/02-syntax-decisions.md** — when you decide, agents add the row there
and remove it from this file.

Decide one group at a time. A group must be fully decided before its
milestone starts (plans in docs/plans/ are blocked on these IDs).

---

## Group 7 — Platform *(ratified 2026-06-12 — see docs/02)*

*(S51 ratified 2026-06-12, amended 2026-06-13 — see docs/02: std is
exported as the `std` module (no quotes), short for canonical `jet.std`;
`import std`, `import jet.std as std`, `import std.fs as fs`, `import
std.io`. S16: quotes = file path (`import "./lib"`), no quotes = module.)*

*(S54 ratified 2026-06-12 — see docs/02: no prescribed naming convention in
v1; `jet fmt` layout only.)*

*(S53 ratified 2026-06-12 — see docs/02: concurrency deferred to v2; when
built, option A — `tasks.spawn`, channels, no shared mutable state.)*

*(S52 ratified 2026-06-12, amended 2026-06-13 — see docs/02 and
docs/plans/m12-packages.md: `jet.toml`, graph `jet.lock`,
`[dependencies:*]` colon tables, `@latest`, `.jet/` source root,
`jet add`/`fetch`/`update`; single files manifest-free forever.)*

---

## Group 8 — Post-1.0 horizon *(owner direction 2026-06-12)*

*(S58 ratified 2026-06-12 — see docs/02: `std/mem` discovery gate +
`unsafe` audit gate, Zig-style allocators, sema-gated `&`/`*`.)*

*(S59 ratified 2026-06-12 — see docs/02: C FFI deferred to v2; when built,
option A — `extern c` blocks mirroring S50.)*

*(S60 ratified 2026-06-12 — see docs/02: `pure fn name(…)` checked modifier;
purity in the signature; enables `jet eval --pure`.)*

*(S61 ratified 2026-06-12 — see docs/02: optional argument labels,
positional order fixed, trailing default values.)*

*(S62 ratified 2026-06-12 — see docs/02: Kotlin-style trait delegation
`impl Trait using field;`.)*

*(S63 ratified 2026-06-12 — see docs/02: RAII scope-end cleanup as the
one story; `defer` noted as a possible later complement.)*

---

## Group 9 — M12 package architecture *(ratified 2026-06-13 — see docs/plans/m12-packages.md)*

*(D-PM1…8 ratified 2026-06-13 — see m12 plan: Nix-style store from M12.1;
store path `~/.jet/store/<name>-<version>-<full-fingerprint>/`; `jet.toml`
only for manifest; exact pins M12.1, ranges+resolver M12.2; all M12.1 in
`jet`; layer 3 may add internal jetpack helper behind `jet` subcommands.)*

---

## Group 10 — M13 LSP *(ratified 2026-06-14 — see docs/plans/epoch-1/m13-lsp.md)*

*(D-LSP1…13 ratified 2026-06-14 — all recommendations accepted as written:
D-LSP1 `jet lsp` subcommand; D-LSP2 full error recovery; D-LSP3 debounced
~200ms via dirty-flag flush; D-LSP4 file-granular re-parse; D-LSP5 type-aware
completion + switch snippet + auto-import; D-LSP6 type + ownership + doc-comment
hover; D-LSP7 structured edits shared with CLI `jet fix`; D-LSP8 inlay hints
off by default except clone hint; D-LSP9 near-zero settings; D-LSP10 strict
standard LSP; D-LSP11 crash-proof handlers + `jet lsp doctor`; D-LSP12 fixture
+ transcript + latency bench in CI; D-LSP13 code lens deferred to `jet dev`.)*

---

## Tally sheet (open only)


| Group              | IDs  | Needed by | Status |
| ------------------ | ---- | --------- | ------ |
| — (deferred)       | S56  | post-1.0  | ☐      |


Ratified (see docs/02): Group 1 confirmations; Group 2 — S29–S33; Group 3 —
S34–S36; Group 4 — S37–S42; Group 5 — S43 S44 S49 S50; Group 6 — S26 S28
S45 S48 S46 S47 S55 S57; Group 7 — S51 S52 S53 S54; Group 8 — S58 S59 S60
S61 S62 S63; Group 9 — D-PM1…8 (see docs/plans/m12-packages.md); Group 10 —
D-LSP1…13 (see docs/plans/epoch-1/m13-lsp.md).
