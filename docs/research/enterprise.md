# Enterprise adoption — assessment (2026-06-12, unratified)

Status: exploratory analysis, **not ratified**, decides no syntax or
semantics. Written to answer: "what would we want to implement for
industry/enterprises to start using Jet?" Grounded in docs/spec/philosophy.md and
docs/spec/roadmap.md. Most of what enterprises need is already on the roadmap — the
real gaps are policy and operations, not language features.

## Already on the roadmap (enterprise checklist items)

M6 (fmt/test/imports), M10 (stdlib), M11 (concurrency), M12 (packages
with exact pins, content-hashed lockfile, no install-time code
execution), M13 (LSP), M7/S59 (Rust and C FFI). That is the bulk of any
"can we adopt this" checklist, and the M12 design is already unusually
good for enterprise — "no install-time code execution" is something npm
and PyPI still cannot say.

The strongest wedge is one Jet already owns: **memory safety with no
`unsafe`, ever**. CISA/NSA memory-safe-language guidance is actively
pushing organizations off C/C++, and Jet's pitch — Rust's safety
guarantees without Rust's learning curve — is aimed precisely at the
teams that bounced off Rust. Single static binary with no config is
also a genuine ops win (scratch containers, no runtime to patch).

## The gaps — roughly in adoption-blocking order

### 1. Stability and release policy (cheapest, highest leverage, pure docs)

Enterprises adopt promises, not features. Needed before anything else:

- A written backward-compatibility guarantee post-1.0.
- A deprecation policy and release cadence; eventually an LTS
  designation.
- Rust's **edition system** is the model worth stealing — it preserves
  the simplicity ratchet *and* allows fixing mistakes.
- Explicit licenses for the compiler and, critically, a statement that
  **generated code carries no license obligations**.

### 2. A debugger

DAP source maps are currently "deferred past v1.0" — for industry this
is the wrong shelf. No enterprise team ships a language its developers
cannot step through. Since Jet transpiles to Rust, the pragmatic v1 is
line-directive-style source mapping so gdb/lldb/VS Code show Jet source
lines, not generated Rust. Recommendation: promote to the
committed-additions list in docs/spec/roadmap.md.

### 3. Supply-chain features in M12 Phase 2

When the registry lands, enterprises will require:

- Private/internal registries and mirror support (Artifactory/Nexus
  proxying).
- Vendoring for air-gapped builds.
- SBOM emission (CycloneDX/SPDX — nearly free given the lockfile).
- Namespace ownership rules.
- An advisory database and a `jet audit` command.

None of this conflicts with the existing M12 design; it is mostly
Phase 2 scope.

### 4. Observability stdlib

M10 has fs/io/json but nothing for production operations. Minimum bar:
structured logging in `std/log`. Eventually metrics and trace-context
propagation — but logging alone covers most CLI/tool use cases, and it
should exist before anyone runs Jet in production.

### 5. A server-side story

Committed-addition item 5 (blocking sockets + HTTP *client*) covers
tools, but enterprise bread-and-butter is services:

- An HTTP **server**.
- TLS — bridge to rustls via the FFI tier; never hand-rolled.
- Database connectivity (Postgres first; FFI to a vetted Rust driver).

This is also where "no async, tasks + channels only" gets
stress-tested. Thread-per-connection is fine for internal services at
hundreds of connections; hold that line for v1.x rather than reopen
async, but write the positioning down explicitly ("Jet services scale
like Go circa 2012; if you need 100k connections, that's not us yet").

### 6. Cross-compilation surfaced as a feature

rustc provides the target matrix nearly free — `jet build --target
linux-arm64` would be a one-flag enterprise feature (build on CI for
the deploy target) that is mostly inherited. Cheap to add to M6 or M14
scope.

### 7. CI/CD table stakes, post-M6

- Coverage output from `jet test`.
- Machine-readable diagnostics (`--json` — M13 needs this anyway).
- Prebuilt toolchains in containers; a GitHub Action.

Mostly packaging work.

## What NOT to do for enterprises

Async/await, exceptions, configurability of fmt/lints, or compliance
certifications (ISO spec, safety-critical) — all premature or contrary
to docs/spec/philosophy.md. Enterprises ask for configurability reflexively; Go proved
you can refuse and win.

## Sequencing reality

No enterprise touches a pre-1.0 language, so none of this jumps the
M6–M14 queue. The pattern to follow is Go's: be excellent for internal
CLI tools and small services first (exactly M14's showcase), get
adopted bottom-up by individual engineers, and have the policy story
(item 1) ready the day a platform team asks "can we standardize on
this?" The only items actionable now are doc-only ones: write the
stability/edition intent into docs/spec/philosophy.md or a new doc, and promote the
debugger from "deferred" to committed additions — both owner decisions.
