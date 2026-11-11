# Release and compatibility policy

This page is the contract for package authors, toolchain maintainers, and registry
operators. It covers the pre-1.0 boundary, the owner-declared post-1.0 rules,
and release-facing CLI behavior. Executable truth lives in
`Jetpack/PackageModel/Source/Manifest/Manifest.jet`, `Jetpack/Foundation/Source/ExitCodes.jet`,
and the release fixtures under `tests/release/`.

## Compatibility boundary

Until the owner declares the compatibility boundary for a qualified 1.0
release, Jet is a greenfield pre-1.0 toolchain. Breaking changes are allowed;
there is no compatibility obligation for an existing source form until the
owner makes that declaration. The rules below are the ratified policy that
applies after that declaration, not evidence that a release is ready.

The executable banner reports the release disposition from
`pkg_current_release_status` in
`Jetpack/PackageModel/Source/Manifest/Manifest.jet`. A version string, command existence, or
local telemetry observation cannot promote a capability or make the post-1.0
promise active.

## Versions and editions

- **Compiler** — the `jet` binary, versioned with normal SemVer (D-REL1).
- **Edition** — a project-selected era of Jet syntax (D-REL3), written as
  `edition: "2026"` in `package.jet`. A toolchain supports a fixed set and
  advertises it in `jet --version`.
- **Epoch** — a descriptive era label, never encoded in the compiler version
  (D-REL2). The owner bumps the version manually.
- **Registry protocol** — the versioned index format spoken by a compiler and a
  package registry, independent of the compiler version.

After the owner declaration, the compatibility levels are:

| Level | What may change | Migration |
|---|---|---|
| **Patch** (`x.y.Z`) | Bug fixes and diagnostic text fixes; no behavior on which a correct program relied. | None. |
| **Minor** (`x.Y.0`) | Additive changes only: new standard-library items, diagnostics, or editions. Existing code keeps compiling. | None. |
| **Major** (`X.0.0`) | Breaking changes, gated behind a new edition. Old editions keep working. | Explicit edition bump and `jet fix`. |
| **Epoch** | A descriptive story label, with no compiler-version meaning (D-REL2). | None. |
| **Edition** | The opt-in unit of syntax compatibility. A project pins one; an unsupported edition is E2001. | `jet fix` followed by an explicit edition bump. |

Compiler versions use normal SemVer after the boundary (D-REL1), and epoch
numbers remain outside the version (D-REL2).

## Post-1.0 guarantee

Once the owner declares the boundary, code that compiles in edition *N* keeps
compiling on every later toolchain that still supports edition *N*. New syntax
that would break old code lands only behind a newer edition; pinning an older
edition opts out of it. The supported-edition list in `jet --version` is the
machine-visible statement of which editions a toolchain accepts.

## Deprecation and migration

The public lifecycle is one ladder: `_name` is soft-public, a plain name is
stable, and `#Deprecated` is the retiring rung for a stable public item; a
`priv module` keeps internals out of the ladder (D-VIS-MODULE1). A named
removal edition is the final delta.

1. Mark a public item with
   `#Deprecated(since: "1.2", use: "parse", removed_in: "2028")`. `since:`
   names the deprecation version or edition, `use:` names the replacement, and
   `removed_in:` names the removal edition when supplied.
2. Before the named removal edition, the item still compiles and emits L2001.
   The warning carries the replacement, and `jet fix` performs the plain
   replacement rename. For a qualified replacement such as `cbor.to_bytes`,
   the edit replaces the used member with `to_bytes`.
3. `removed_in:` is dormant until editions own removal. Before that edition it
   affects only warning text; at or after it, use becomes E2002 and names the
   replacement. Without `removed_in:`, the item remains warning-only.
4. Core declarations without Jet source use the same marker metadata on their
   ordinary declaration rows. User items and Core migrations therefore share
   one L2001/E2002 renderer.

D-REL5 permits source rewriting only through **`jet fix`** or an explicit
edition upgrade, and only on explicit request. No tool silently migrates
source. D-REL4 supplies no LTS branch before the compatibility boundary; the
LTS window begins at GA.

## Single-file and generated-code rules

Single-file `jet run file.jet` has no edition marker and uses the toolchain's
newest stable edition (E2-V4). Editions are a project-manifest concept; the
single-file path needs no manifest.

Rust source emitted by the Jet compiler carries no additional license
obligation from the compiler. The generated code is the user's output, under
the project's chosen license and the terms of its own dependencies; using Jet
adds no copyleft or attribution term to that output.

## Enterprise LTS

D-ADOPT-LTS1 was ratified on 2026-08-01. Jet starts one LTS line each year.
Each line has twelve months of active support and twenty-four months of
maintenance support, thirty-six months total, with at most three lines
overlapping.

Active support covers security, critical compiler and runtime correctness,
supported-host and toolchain breakage, and severe performance regressions; it
does not add language behavior. Maintenance support covers security and
critical data-loss, memory-safety, type-safety, and miscompilation fixes.

The public calendar gives six months' notice for a calendar change and never
shortens a live line. An LTS line keeps its advertised editions and hosts.
Dropping either before EOL requires the existing security or safety exception
process and a migration notice. The latest stable release and every live LTS
line receive applicable security fixes. The adoption pack carries the policy
calendar at `Tools/adoption/release/calendar.json`; dates and edition/host matrices
are published only when fixed.

## Runtime compatibility boundaries

### Process environment (D-ENV-MUTATE1)

`core.sys` mutations change Jet's locked logical environment, not the host
process environment. A later `core.sys.get` observes the write, and every
`core.process` child inherits it. Foreign code that calls libc `getenv` or
reads the Windows environment block after a Jet mutation sees the original
host value. Pass changed values to foreign APIs explicitly: mutating a
process-global environment while foreign threads may read it cannot meet Jet's
memory-safety guarantee. Existing editions keep `core.sys.set ()`; changing its
fallible `!EnvError` signature requires a major release and edition opt-in.

### TLS security gate (D-TLS1)

For `core.net.fetch` and the client path in `core.http.client`, `https://`
works by default through the rustls bridge and system certificate roots.
Replacing that default requires an external security audit and an interop
battery against rustls and OpenSSL test vectors. Server TLS is an explicit
named option on `core.http.server`:

```text
use server: core.http.server

run: fn() {
    cert := "server.crt"
    key := "server.key"
    mux: server.mux()
    server.serve(
        "127.0.0.1:0",
        mux,
        tls: server.tls(cert, key)
    ) ?? return
}
```

The `tls:` label is part of the API; an unlabeled third argument is rejected.

## Version banner

`jet version` and `jet --version` render the same deterministic banner, and
`jet --version --json` carries the same facts as one JSON object. The version
is `JET_VERSION` (`a0.0.N`, alpha) in `Jetpack/PackageModel/Source/Manifest/Manifest.jet`;
a release prints `jet a0.0.N (<commit10>, <date>, alpha)` from the `VERSION`
stamp beside its executable, and any other build prints
`jet a0.0.N-dev+<commit10>`. The output is golden-tested in
`tests/release/version_banner.txt` and the `pkg_version_banner` assertions in
`Jetpack/PackageModel/Tests/Manifest.jet`. A dev build prints:

```text
jet a0.0.43-dev+29517f5447
Release status: Prerelease
Release disposition: Current
Release readiness: Not ready
Compatibility policy: Future 1.0 (planned; not active)
Supported editions: 2026 (newest: 2026)
Registry protocol: v1
```

The `Compatibility policy` line describes future law, not a shipped
promise. `jet inspect claims --json` is the machine-readable release-claim
projection; it consumes the bounded `manifest.capability_relation.rows`
source and remains fail-closed for missing, stale, unavailable, failed, or
unqualified rows. Telemetry reports observations only.

`jet run Tools/hardening/Main.jet -- --regressions DIR` runs the #2337
regression seeds: semantic equality, indexed places, packed Int, release
emission and stdin transport. Each seed runs under `jet run` and as a release
build, and must print its expected bytes and exit 0. Any other result is
written to DIR as a `jet.hardening.repro.v1` bundle, and the run fails.
`--invert` puts each seam's broken behavior back. That run fails unless every
seed gives a bundle, so the check is proven to see each bug.

`jet run Tools/hardening/Main.jet -- --generate DIR` writes programs from a
seed (`--seed`, default 2342; `--cases`, default 16, at most 128). Each
program joins one to three constructs from expressions, statements, control
flow, patterns, generics, traits, effects, views, comptime and nested places.
Every program parses, loops only over ranges or lists, and prints each
value it makes. Each one must pass `jet check`, then print the same bytes
under `jet run` and as a release build. A program that fails is shrunk to
its main construct while the failure stays the same. It is then written to
DIR as a bundle, and the run fails. The run first checks four planted faults:
a missing family, an admitted but unlowered construct, an optimizer-only
change and a program that prints nothing. Each must fail, or the run stops.
`--invert` plants faults in every case and fails unless each one is found.
One seed on one compiler gives the same bundles in the same order; the last
line prints their digest without the run times.

`jet run Tools/hardening/Main.jet -- --write` generates the rows source above as
`.jet/hardening-manifest.json`. Its rows come from the Core registries and the
route and dispatcher facts, and each covered row names the observable sink its
conformance witness uses. `--check` fails unless the stored file is
byte-identical to a fresh generation. It reports the file stale when the
source snapshot hash has moved, so evidence recorded under the old hash no
longer counts. `--repro-check` and `--repro-replay` validate and replay
`jet.hardening.repro.v1` bundles.

## Compiler release candidates

`jet run Tools/release/Main.jet -- candidate DIR` turns a compiler build into a
release candidate. It writes `DIR/CANDIDATE.json` (schema
`jet.release.candidate.v1`), one record that names the source commit and
tree, the toolchain and its chain fixed point, every shipped and uploaded
file's sha256, the signer, the test receipts, the support matrix
(`Tools/release/support-matrix.tsv`) and the provenance. The record carries
its own content digest, and `CANDIDATE.json.sig` is an Ed25519 signature of
that digest by the `jet-release` key.

`jet run Tools/release/Main.jet -- publish DIR` is the one route to a GitHub
release. It derives the record again from disk and refuses any difference, a
bad signature or an untrusted signer. These also block release: a target or
shard with no receipt, a check that passed in one run and failed in another, a
skipped or partly run check, a stale artifact, a signing failure, a chain
without a fixed point, an archive that packs differently twice, and a release
GitHub holds only in part. Every run appends a JSON entry under `DIR/audit/`,
and a rerun moves the earlier stage and record there instead of deleting them.
Evidence comes first: `publish` refuses a record with no `candidate` entry,
and it writes its own entry before `gh release create` runs.
The release also uploads each pinned asset jet embeds: the C toolchain pack
(`CC_PACK_ASSETS`) and the static bubblewrap the native sandbox falls back to
(`jet-bwrap-<arch>-linux`, `NATIVE_SANDBOX_BWRAP_ASSETS` in
`Jetpack/Foundation/Source/NativeSandbox.jet`, built by
`Tools/agent/bwrap.nix`). `admit` builds both from their pinned Nix sources,
refuses one whose SHA-256 or size differs from the row jet embeds, and puts
them beside jet; the seed and combined archives carry the bubblewrap, and
`SHA256SUMS` lists it. `candidate` and `publish` read both tables at the
release commit and block a staged asset whose bytes jet does not embed, and
a release that lacks the bubblewrap jet embeds. An asset is published with the
first release whose jet embeds it; an earlier release is never replaced.
`Tools/release/Tests.jet` covers each case with a fake `gh`.

## Exit codes

`jet` returns stable documented exit codes so shells and CI can branch without
parsing text. The single source of truth is
`Jetpack/Foundation/Source/ExitCodes.jet`.

| Code | Name | Meaning |
|---:|---|---|
| 0 | `OK` | Success. |
| 1 | `USER_ERROR` | An unhandled entry error report or a driver-reported user problem. |
| 2 | `USAGE` | The command line was wrong: an unknown command or a missing, invalid, or unknown argument/flag. |
| 70 | `RUNTIME_PANIC` | A built program stopped at runtime through `panic`, `require`, an index fault, or another program-side fault; emitted by the Prelude boundary. |
| 101 | `ICE` | Jet's own compiler defect: the compiler reached an impossible state (invariant I2), never a user-program exit. |

`USER_ERROR` (1) and `USAGE` (2) stay distinct: the former means the program
or its inputs failed, while the latter means the `jet` invocation was wrong.

## Enforcement map

- `edition:` is parsed in `Jetpack/PackageModel/Source/Manifest/Manifest.jet`
  and recorded in the syntax registry
  (`Compiler/JetFoundation/Data/Syntax/JetpackConfig.jet`) as
  `MANIFEST_FIELD_EDITION` (D-REL3).
- Supported editions are `SUPPORTED_EDITIONS` in `Manifest.jet`; E2001 comes
  from `pkg_check_edition_support` there and from the compiler loader in
  `Compiler/JetDriver/Source/Driver/Loader.jet`.
- The banner is `pkg_version_banner`; release status is
  `pkg_current_release_status` in `Manifest.jet`.
- E2001, E2002, and L2001 are registered in
  `Compiler/Prelude/Diagnostics.jet` and the Foundation
  diagnostic registry, with release snapshots under `tests/release/`.
