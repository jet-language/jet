# Compatibility & release policy (ratified)

This is the promise an enterprise adopts before it depends on Jet. It is the
ratified output of milestone E2-M2.
Every decision below was ratified 2026-06-16 (D-REL1…D-REL5).

## Glossary first

- **Compiler** — the `jet` binary. Versioned with normal SemVer (D-REL1).
- **Edition** — a per-project opt-in to a specific era of Jet *syntax*
  (D-REL3), written `edition: "2026"` in `package.jet`. A toolchain supports a
  fixed set of editions and prints them in `jet --version`.
- **Epoch** — era storytelling for marketing/roadmap (e.g. "epoch 2"). It is
  **never** encoded into the compiler version (D-REL2); the owner bumps the
  version manually.
- **Registry protocol** — the versioned index format the compiler speaks to a
  package registry, independent of the compiler's own version.

## Compatibility levels — what may change

| Level | What may change | Migration |
|-------|-----------------|-----------|
| **Patch** (`x.y.Z`) | Bug fixes; diagnostic *text* fixes. No behavior a correct program relied on. | none |
| **Minor** (`x.Y.0`) | Additive only: new std items, new diagnostics, new editions. Existing code keeps compiling. | none |
| **Major** (`X.0.0`) | Breaking changes, gated behind a new **edition**. Old editions keep working. | opt-in edition bump + `jet fix` |
| **Epoch** | Pure storytelling. No compiler-version meaning (D-REL2). | n/a |
| **Edition** | The unit of opt-in syntax compatibility. A project pins one; the toolchain refuses an edition it doesn't ship (E2001). | `jet fix` + edition bump |

External versioning is **normal SemVer, forever** (D-REL1). Epoch numbers are
not encoded into the version (D-REL2); version bumps are the owner's manual call.

## Backward-compatibility guarantee

Post-1.0, code that compiles in edition *N* keeps compiling on every later
toolchain that still supports edition *N*. New syntax that would break old code
lands only behind a *newer* edition; pinning an older edition opts out of it.
A toolchain advertises the editions it supports in `jet --version`.

### Environment safety correction (D-ENV-MUTATE1)

`core.sys` mutations now change Jet's locked logical environment rather than
the host process environment. Valid Jet behavior remains compatible: a later
`core.sys.get` observes the write, and every `core.process` child inherits it.
Foreign code that calls libc `getenv` or reads the Windows environment block
after a Jet mutation now sees the original host value. This is the ratified
narrow safety exception to the normal compatibility promise: mutating a
process-global host environment while foreign threads may read it cannot meet
Jet's memory-safety guarantee. Pass changed values to foreign APIs explicitly.
Existing editions keep `core.sys.set => ()`; its fallible
`() ! EnvError` signature requires a future major release and edition opt-in.

## Deprecation policy + migration window

The public lifecycle is one ladder: `_name` is internal, `pub _name` is
soft-public, `pub` is stable, and `#Deprecated` is the retiring rung for a
stable public item. A named removal edition is the final delta.

1. A public item is marked **deprecated** with
   `#Deprecated(since: "1.2", use: "parse", removed_in: "2028")`. `since:` names
   the deprecation version or edition, `use:` names the replacement, and the
   optional `removed_in:` names the removal edition.
2. While the project's edition is before the named removal edition, the item
   still compiles and emits **L2001** (a lint). The consumer warning carries the
   replacement and `jet fix` performs the plain replacement rename. For a
   qualified replacement such as `cbor.to_bytes`, the edit replaces the used
   member with `to_bytes`.
3. `removed_in:` is dormant until editions own removal. Before that edition it
   has no effect beyond the warning text; at or after it, use becomes **E2002**
   and names the replacement. Without `removed_in:`, the item remains
   warning-only.
4. Core declarations without Jet source use the same marker metadata on their
   ordinary declaration rows. User items and Core migrations therefore share
   the L2001/E2002 renderer. The former duplicate Core-only deprecation tables
   are retired; there is one lifecycle source.

## Migration authority (D-REL5)

Only **`jet fix`** and an explicit **edition upgrade** may rewrite a user's
code, and only on explicit request. No tool silently migrates source. There is
no LTS branch pre-GA (D-REL4); the LTS window is set at GA.

## The single-file exemption

Single-file `jet run file.jet` carries **no** edition marker and always uses the
toolchain's newest stable edition (E2-V4). Editions are a project-manifest
concept; the single-file path stays sacred — no manifest required.

## Generated-code license

Rust source emitted by the Jet compiler carries **no additional license
obligation from the compiler**. The compiler is a translator: the generated
code is yours, under whatever license you choose for your project. Using Jet to
build a program imposes no copyleft, attribution, or other term on that
program's output beyond what your own dependencies require.

## TLS security gate (D-TLS1)

D-TLS1 makes `https://` work by default for the client path
(`core.net.fetch` and `core.http.client`) through the rustls bridge and system
certificate roots. A native Jet TLS implementation may replace rustls as the
default only after an external security audit and an interop battery against
rustls and OpenSSL test vectors. Advanced client configuration lives under
`core.net.tls`; server TLS is the D-TLSSERVE1 named option
`Server.serve(addr, mux, tls: Server.tls(cert, key))`.

## `jet --version` contract (E2-D1)

`jet --version` prints, deterministically (golden-tested in `tests/release_gates.rs`):

```
Jet 1.0.0
supported editions: 2026 (newest: 2026)
registry protocol: v1
```

- compiler SemVer,
- the supported edition range and the newest stable edition,
- the registry-protocol compatibility version.

## Exit-code table (E2-M3, extends E2-M2)

`jet` returns a stable, documented exit code so shells and CI gates can branch
on the outcome without parsing text. The numbers never change meaning. The
single source of truth is `crates/jet-foundation/src/ExitCodes.rs`.

| Code | Name           | Meaning                                                |
|------|----------------|--------------------------------------------------------|
| 0    | `OK`           | success                                                |
| 1    | `USER_ERROR`   | an unhandled entry error report, or a driver-reported user problem |
| 2    | `USAGE`        | the command line itself was wrong (unknown command, missing/invalid argument or flag) |
| 70   | `RUNTIME_PANIC`| a built program breached or stopped at runtime (`panic`, `require`, an index fault, or another program-side fault); emitted by the Prelude boundary |
| 101  | `ICE`          | Jet's own compiler defect (invariant I2): rustc rejected generated code, or the compiler hit an impossible state — never a user-program exit |

`USER_ERROR` (1) and `USAGE` (2) are deliberately distinct: "my program has a
bug" versus "I called `jet` wrong". Golden-tested in `tests/cli.rs`.

## Where this is enforced

- `edition:` field — parsed in `crates/jet-pkg-model/src/Manifest.rs`, surfaced on
  `manifest::PackageMeta`, recorded in `crates/jet-foundation/src/Syntax.rs` (`MANIFEST_FIELD_EDITION`,
  D-REL3).
- Supported editions — `manifest::SUPPORTED_EDITIONS`; the check is
  `manifest::check_edition_support` (E2001), called from `crates/jet-driver/src/Loader.rs`.
- Banner — `manifest::version_banner`, printed by `jet --version`.
- Diagnostics — E2001/E2002/L2001 in docs/spec/diagnostics.md, snapshotted in
  `tests/release/`.
