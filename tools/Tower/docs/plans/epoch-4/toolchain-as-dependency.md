# Toolchain as a dependency — U30 implementation plan

**Card:** `catoolchain` (#179). **Gate:** `D-JPK-TOOLCHAIN1=A` (ratified). No
open owner decisions — the ballot settled the full surface. This file is the
HOW. Run repo commands through Nix (`nix develop -c ...`); no external crates in
the compiler (I6); every diagnostic gets `docs/spec/diagnostics.md` + a snapshot.

## What A ratified (do not re-decide)

- `pkg.jet` gains a `jet:` field. Value is a channel ref (D-JPK-CHANNEL1
  semantics); the lock records the exact resolved version.
- A `jet` whose version differs from the pin **realizes the pinned toolchain
  into the hangar as a prebuilt object via the D-JPK-CACHE1 substitution path**,
  then **execs it** (D-JPK-DISPATCH1 process seam). No source build of the
  compiler on the user's machine; a cache miss for the platform is an honest
  error, not a from-scratch compile.
- Offline once realized (D-JPK-OFFLINE1); GC-bounded (D-JPK-GC1); no Nix
  required (D-JPK-NONIX1); no daemon / no root (D-JPK-NODAEMON1).
- **Frozen-forward identity block**: the `payload:` block and the `jet:` line
  have grammar every past and future `jet` can parse. Version dispatch can never
  be wedged by later manifest evolution (the Go `go.mod` contract).
- Verbs: `jet toolchain` (show the pin + resolved version), `jet update jet`
  (move the pin deliberately).

## Dependency order

Ships after Phase A of the epoch-4 implementation plan (`implementation.md`):
dispatch seam (A1), `pkg.jet` canon (A2), and the hangar object envelope (A4 —
prebuilt toolchain objects are envelope-carrying hangar objects). It needs no
jetos work.

---

## Slice T1 — frozen-forward identity block (failing test first)

Goal: guarantee any `jet` parses the identity block of any `pkg.jet`, regardless
of what else the manifest grows.

1. **Failing test** `identity_block_parses_across_unknown_fields`: a `pkg.jet`
   whose body has unknown future top-level keys and unknown nested syntax still
   yields a parsed identity block `{ payload, jet }`. Put it under the manifest
   parser crate tests (`crates/jet-driver/src/Jetpack/PackageManifest/`).
2. Add a dedicated **identity pre-parse** in `PackageManifest`: a small,
   grammar-frozen reader that extracts `payload: { name, version }` and the
   `jet:` line **before** the full manifest parse, tolerating unknown
   surrounding syntax (skip-to-matching-brace / skip-unknown-line). This reader's
   grammar is contract-frozen: document it in `docs/spec/spec.md` under the
   manifest section and never narrow it.
3. `jet:` value parses as a channel ref via the existing `RefSpec`/channel
   reader (D-JPK-CHANNEL1). Absent `jet:` = unpinned (rung-0/1 stays
   frictionless): use the running `jet`, no fetch.

Diagnostic:

- `E1233` `bad-toolchain-pin`: `jet:` value is not a valid version/channel ref.
  what/why/fix names the accepted forms (`jet: 0.4`, `jet: 0.4.2`, `jet: main`).

Exit: identity block extracts from a manifest containing arbitrary unknown
fields; snapshot for `E1233`.

## Slice T2 — pin resolution + lock record

1. **Failing test** `toolchain_pin_locks_exact`: `jet:` channel `0.4` resolves
   to an exact `0.4.z` recorded in `.jet/lock`; re-runs read the lock, not the
   channel.
2. Resolve the channel to an exact toolchain version only in `jet update jet`
   and first realization; otherwise read the locked exact version (mirror the
   D-JPK-CHANNEL1 rule: channels resolve only on update/first-add, lock stays
   exact).
3. Extend the lock schema with a `[toolchain]` record (`channel`, `version`,
   `output_hash`, `platform`) — reuse the A4 envelope fields so a toolchain is
   an ordinary hangar object. Writer/reader in `WorkspaceLock.rs`.

Diagnostic:

- `E1234` `toolchain-channel-in-ci`: an unlocked channel pin under `--offline` /
  CI with no lock entry (parallels the D-JPK-CHANNEL1 `unlocked-channel-in-ci`
  rule). Fix: run `jet update jet` and commit the lock.

Exit: lock round-trips a `[toolchain]` record; channel never re-resolves with a
lock present.

## Slice T3 — realize + exec the pinned toolchain

1. **Failing test** `mismatched_jet_realizes_and_execs_pin` (offline provider
   fixture, `JETPACK_FIXTURES`): a fixture toolchain object stands in for a
   downloaded prebuilt; a `jet` with a different self-version realizes it into
   the hangar and re-execs. Assert the exec argv, env passthrough, and that the
   child's reported version is the pinned one.
2. Self-version check at startup of a manifest-driven verb (`jet run/build/test`
   in a repo with `pkg.jet`): if installed `jet` version ≠ locked toolchain
   version, realize the toolchain object (CACHE1 substitution path; core
   provider realization boundary in `Provider.rs`) and exec it through the
   D-JPK-DISPATCH1 process seam. Print the honest one-liner:
   `jet: project pins toolchain 0.4 (installed 0.6.1); realizing… exec`.
3. **Re-exec guard**: set a `JET_TOOLCHAIN_EXEC=<version>` env marker before
   exec; the child, seeing its own version match the marker, does not re-realize
   or re-exec (prevents loops and lets the pinned child run natively).
4. Cache miss for the platform → `E1235` `toolchain-unavailable`: names the
   version + platform, offers `jet update jet` (move pin) or an install link;
   never falls back to building the compiler from source or to the wrong `jet`.

Diagnostics:

- `E1235` `toolchain-unavailable`.
- `E1227` `engine-version-skew` (already planned in A1) covers protocol skew
  between a pinned `jet` and its sibling engine binaries.

Exit: version-skew repo re-execs into the pinned toolchain deterministically
under the fixture; no re-exec loop; honest error on platform miss.

## Slice T4 — verbs

1. `jet toolchain`: prints the pin (`jet:` channel), the locked exact version,
   the hangar object id (`jet-<version>-<fp>`, D-PM1), and realized/​missing
   state. Read-only.
2. `jet update jet [<channel>]`: re-resolves the channel, updates the lock's
   `[toolchain]` record, realizes the new object. Only place the pin moves.
3. `jet init` (U11 lift) writes a `jet:` line pinning the running toolchain's
   channel by default, so a lifted project is reproducible from birth.

Wire both into the CLI registry (`Jetpack/CLI.rs` verb match) and the jet-side
dispatch contract (`jet toolchain` / `jet update` route to the jetpack engine
per DISPATCH1).

Tests: `jet_toolchain_reports_pin`, `jet_update_jet_moves_lock`,
`jet_init_pins_running_channel`.

---

## Exit criteria checklist

- [ ] Identity block (`payload:` + `jet:`) parses under arbitrary unknown
      manifest fields; grammar documented as frozen in `docs/spec/spec.md`.
- [ ] `jet:` channel resolves to an exact version; `.jet/lock` `[toolchain]`
      record round-trips with envelope fields.
- [ ] Version-mismatched `jet` realizes the pinned prebuilt object and re-execs;
      re-exec guard prevents loops; the pinned child runs natively.
- [ ] Platform cache miss = `E1235`, never a source build of the compiler and
      never silent use of the wrong `jet`.
- [ ] `--offline` with satisfied lock never touches the network (D-JPK-OFFLINE1
      golden sweep includes the toolchain path).
- [ ] `jet toolchain`, `jet update jet` implemented + tested; `jet init` writes a
      pin.
- [ ] Diagnostics `E1233`–`E1235` in `docs/spec/diagnostics.md` with snapshots.
- [ ] Full `cargo test` green; example under `examples/features/` shows a pinned
      project running (fixture toolchain).

## Cross-references

- Distribution model (prebuilt via cache, not source-built): **settled** by
  `D-JPK-TOOLCHAIN1=A` detail — do not re-open.
- The Rust/native build toolchain that compiles a user's `extern rust` bridge
  dep is a **different** toolchain and a separate decision — see
  `package-build-from-source.md` (`D-JPK-BUILDTOOL1`).
