# M12.2 registry + `jet publish` UX (c96)

**Status: ready — active. All four gating decisions ratified 2026-06-25
(D-PUBLISH1A, D-VERSION1, D-RESOLVE1, D-LOCK1). No open owner decision. Implement
on the owner's "go".**

Unblocks: **Saoirse** (publish a library), **Amara** (reproducible scripts
pinned to a published version). Also unblocks D-PKGSIGN1 Tier A (Ed25519 author
signing), which the ratified log explicitly parks on c96.

## Goal

Ship the publish + version-resolution workflow end to end: a library author cuts
an immutable release with `jet publish`, can retract a bad one with `jet yank`,
and a consumer's `textkit#^1.2` resolves to the highest compatible published
version, frozen in `.jet/lock`. Today `jet publish` only validates locally and
prints "registry upload not yet implemented"; registry dependencies hard-error as
"planned for M12.2". This card closes that gap.

## Current state (verified, file:line)

- **`jet publish` command** — `Source/CmdSupply.rs:18` `run_publish(force, mode)`.
  Reads the version from `pkg.jet` (`mf.package.version`, line 35 — already the
  single source of truth). Pre-flight runs: build gate via sema (lines 44-66),
  a **tests stub** (`tests_ok = true`, line 72 — does not spawn `jet test`),
  SemVer API diff (E1218/E2601, lines 91-149), schema snapshots, capability
  freeze. Gate flag today is `--force` (not D-PUBLISH1A's `--allow-dirty`).
  **No git working-tree check exists.** Upload is deferred — prints the
  "registry upload not yet implemented (D-PKGS1 deferred)" note, lines 188-196.
- **Dispatch / CLI** — `Source/main.rs:443` (`"publish"` → `run_publish`),
  spec at `Source/CLI.rs:57`. No `--allow-dirty` flag registered.
- **`jet yank`** — does not exist anywhere (no command, no dispatch, no flag).
- **Resolver** — `Source/Publish/Resolve.rs` has `check_conflicts` → E2602
  (conflict *detection* only). The full SemVer range engine
  (`Source/Publish/SemVer.rs`: `VersionReq` caret/tilde/x/hyphen/OR, prerelease
  rules) is complete and unit-tested. There is **no highest-compatible
  *selection* from a registry index**: registry deps error out at
  `Source/Fetch.rs:405-410` and `Source/Fetch.rs:512-516`
  ("registry dependencies are not yet supported … planned for M12.2").
- **`.jet/lock`** — full read/write/verify in `Source/Lock.rs` (schema v1,
  std-only, I6). `LockedPackage` has `name/version/source/locked/fingerprint/
  dependencies`; `LockSource` is `Root | Path | Git` — **no `Registry` variant**.
  `dep_source` maps a registry dep to a `Path("registry:<name>")` placeholder
  (`Source/Lock.rs:495`). `verify_all_manifest_deps_locked` (E1217) already
  enforces every manifest dep is pinned, used in `--locked` + publish.
- **Lock commit policy** — `jet new` writes `.gitignore` containing `.jet/lock`
  unconditionally (`Source/CmdCompile.rs:267`:
  `"build/\n.jet-build/\n.jet/lock\n.jet/cache/\n"`). D-LOCK1 needs this made
  conditional on package kind.
- **Diagnostic codes** — diagnostics.md tops out at **E1218**. The E1219 block is
  free; D-PUBLISH1A reserves it for the new publish errors.
- **c56** ("Signed binary/source package cache") is **frozen** — it is *not* the
  live registry upload the stale plan claimed. The "validates locally + explains
  the push path" behavior *is* `run_publish` itself. c96 owns the real upload.

## Decision (ratified 2026-06-25)

- **D-PUBLISH1A (A)** — one verb `jet publish`, sibling of `add/update`. Version
  read from `pkg.jet` (single source of truth). Pre-flight **refuses** a dirty
  working tree and failing tests; `--allow-dirty` is the escape. CLI-version arg
  (B) and `jet release` (C) rejected. New publish errors take codes from E1219.
- **D-VERSION1 (A)** — a published version is permanent; re-publish is refused
  (E1221). `jet yank` (with `--undo`) hides a bad version from new resolution,
  while existing `.jet/lock` pins still install it.
- **D-RESOLVE1 (A)** — a range (`textkit#^1.2`) resolves to the **highest
  compatible** published version, frozen in `.jet/lock`; repeat builds stay on the
  locked version until an explicit `jet update`.
- **D-LOCK1 (A)** — `jet new` **commits** `.jet/lock` for executables (drop it
  from `.gitignore`) and **git-ignores** it for libraries. Amends the
  D-JPK-FILES table line.

## Implementation (staged, end-to-end per the skill standard)

Each stage: parser/sema/codegen-or-CLI wired → diagnostic in `diagnostics.md` +
`tests/ui` snapshot → runnable example + golden where user-visible → tests →
docs. The "verifier" here is the CLI/jetpack path, not the language front end.

### Stage 1 — `jet publish` (D-PUBLISH1A)

1. Register `--allow-dirty` in `Source/CLI.rs` + thread through
   `Source/main.rs:443` → `run_publish`. Keep `--force` as the build/semver
   override; `--allow-dirty` is specifically the working-tree escape (they are
   distinct gates).
2. Add a git working-tree check (std-only: shell `git status --porcelain`, same
   pattern as `Source/Jetpack/Provider.rs` git peeks; absence of `git` →
   treat as clean with a note, never a hard fail). Dirty → **E1219** refusal
   unless `--allow-dirty`.
3. Promote the tests stub (`Source/CmdSupply.rs:72`) to actually run the package
   test suite (spawn `jet test` as a subprocess, or call the shared test entry).
   Failing tests → **E1220** refusal unless `--force`.
4. Version stays sourced from `pkg.jet`; no version argument is accepted (reject
   a positional with a teaching error pointing at the manifest field).
5. Wire the actual registry upload (the path c56 froze): stage the package tree,
   write/append the registry index entry (git-registry model, S52). Keep it
   std-only and offline-validatable; the index write is the publishing handshake.

Diagnostics: **E1219** (dirty working tree), **E1220** (tests failing),
each with what/why/fix + a `tests/ui` snapshot (I4).

### Stage 2 — version immutability + `jet yank` (D-VERSION1)

1. On publish, check the registry index for an existing entry at this
   name+version → **E1221** (re-publish refused; immutable). Only `--force` must
   *not* bypass this — immutability is absolute (it underpins the D-PKGSIGN1
   checksum floor).
2. New command `jet yank` (+ `jet yank --undo`): mark a published version yanked
   in the index. Register in `Source/CLI.rs`, dispatch in `Source/main.rs`,
   handler alongside `run_publish` (new `run_yank`). A yank flips an index flag;
   it never deletes content (existing `.jet/lock` pins must still install).
3. Resolver (Stage 3) skips yanked versions for *new* selection but honors a
   pin that names a yanked version.

Diagnostics: **E1221** (immutable re-publish). Snapshot + a runnable shell
example showing the refusal and the `jet yank` flow.

### Stage 3 — highest-compatible resolver (D-RESOLVE1)

1. Add `LockSource::Registry { version }` to `Source/Lock.rs` (+ serialise/parse)
   and replace the `Path("registry:<name>")` placeholder
   (`Source/Lock.rs:495`).
2. Replace the `Fetch.rs:405`/`:512` "not supported" errors with real
   resolution: read the registry index for the package, filter to versions
   matching the `VersionReq` (engine already in `SemVer.rs`), drop yanked ones,
   pick the **highest**, fetch its tree into the store, pin it in `.jet/lock`.
3. Repeat builds load the pin from `.jet/lock` and skip re-resolution;
   `jet update` (existing) re-resolves to a newer highest-compatible version.
4. Feed selected versions into the existing `check_conflicts` so a true
   multi-dependent conflict still reports E2602.

Example + golden: a project depending on `textkit#^1.2` resolves to the highest
1.x in a test registry; output asserted.

### Stage 4 — `.jet/lock` commit policy (D-LOCK1)

1. In `Source/CmdCompile.rs` `jet new` scaffolding (line 267), make the
   `.gitignore` template conditional on the package kind: **executable** →
   omit `.jet/lock` (commit it); **library** → keep `.jet/lock` ignored.
2. Update the D-JPK-FILES file table in `docs/spec/syntax-decisions.md`:
   `.jet/lock` "Checked in? no" → "yes for executables".

### Stage 5 — docs

`docs/spec/diagnostics.md` (E1219/E1220/E1221), `docs/spec/spec.md` (publish/yank/
resolver workflow), `docs/spec/roadmap.md` (M12.2 registry done),
`syntax-decisions.md` status flips to **Implemented** for the four decisions.

## Sequencing / gates

- **No open owner decision** — all four ratified. The ballot answer is the "go"
  for the unblocked work; only the owner's explicit start gates it (carded
  item).
- **Rides c50, not c56.** c56 is frozen; the registry-upload work lives here.
  The shared infra is **`.jet/lock`** (`Source/Lock.rs`) and the **vendor/hash-pin
  supply chain** that c50 establishes (D-BFS1). Coordinate the `.jet/lock` schema
  edit (Stage 3 adds `LockSource::Registry`) with c50's lock additions so the two
  streams don't fight over the same file.
- **Practical order vs c50:** a *published library* dependency is only fully
  usable downstream once c50's build-from-source compile step lands (a library
  must build, not just stage). So c96 publish + resolver can be built in
  parallel with c50, but end-to-end "publish a library, depend on it, build"
  needs c50's `realize()` compile step too. Neither hard-blocks the other in
  code (c96 touches `CmdSupply.rs`/`Fetch.rs`/`Lock.rs`; c50 touches
  `Provider.rs`/`FFI.rs`).
- **Downstream unblock:** completing the registry upload here unblocks D-PKGSIGN1
  Tier A (Ed25519 signing in `Source/Publish/Sign.rs` + `jet keygen`), which the
  ratified log parks specifically on c96.

## Files (when this burns down)

| File | Change |
|---|---|
| `Source/CLI.rs` | register `--allow-dirty`; add `yank` command spec |
| `Source/main.rs` | dispatch `--allow-dirty`, `jet yank` |
| `Source/CmdSupply.rs` | dirty-tree gate (E1219), real test gate (E1220), reject version arg, registry upload, immutability (E1221), `run_yank` |
| `Source/Fetch.rs` | replace registry "not supported" (lines 405, 512) with highest-compatible resolution |
| `Source/Lock.rs` | `LockSource::Registry { version }` + ser/parse; drop the `registry:` Path placeholder |
| `Source/Publish/Resolve.rs` | highest-compatible selection over a registry index; yank filtering |
| `Source/CmdCompile.rs` | conditional `.gitignore` (commit lock for executables) |
| `docs/spec/diagnostics.md` | E1219 / E1220 / E1221 |
| `docs/spec/syntax-decisions.md` | flip the four decisions to Implemented; amend D-JPK-FILES lock line |
| `docs/spec/spec.md`, `roadmap.md` | publish/yank/resolver workflow; M12.2 done |
| `examples/`, `tests/ui` | publish refusals + yank + resolver golden/snapshots (I4/I5) |
