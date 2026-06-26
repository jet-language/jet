# c122 — Strengthen package and ecosystem trust
**Decision:** D-PKGSIGN1 ratified 2026-06-24 = **B + A opt-in** (checksum floor +
opt-in Ed25519 signing). See Step 4.
**Gate:** none — every step (1–7) is now unblocked.

---

## Current state (from Source/)

- **Lockfile:** `Source/Lock.rs` (448 lines) — schema v1; SHA-256 tree hashes; `--locked` CI
  mode; `verify_lock_matches_manifest`; `verify_store_fingerprint`.
- **Store:** `Source/Store.rs` (339 lines) — Nix-style `~/.jet/store/`; hardlink/copy into
  project; `verify_entry` (re-hashes tree); `gc`; generations log.
- **Fetch:** `Source/Fetch.rs` (658 lines) — git-subprocess only; no HTTP in compiler; path
  + git deps; `--locked` / `--update` modes.
- **Publish:** `Source/Publish/` — SBOM (SPDX 2.3 tag-value), Registry config,
  SemVer, API surface, Schema snapshot, Vendor, Diff, Advisory. `require_signed` field on
  `RegistryConfig` exists but is not enforced.
- **Missing:** package signing/verification, offline-first guarantees, vendoring (stub in
  `Publish/Vendor.rs`), semver compatibility tests, SBOM integration into the build output.

---

## Plan

### Step 1 — Content-hash verification on every install (`Source/Store.rs`)

`verify_entry` exists but is not called on every `link_into_project`. Make verification
mandatory:

In `link_into_project` (`Store.rs:72`), before `link_or_copy_tree`, call `verify_entry`
(`Store.rs:81`, signature `(pkg_name, store_entry, expected_tree_hash)`) with `expected_tree_hash`
from the lockfile's `tree_hash`. If it mismatches, **`verify_entry` already returns E1204**
("store entry tree-hash mismatch / tamper", `diagnostics.md:323`) — propagate that, do not
mint a new code. This makes verification mandatory on every link instead of an opt-in helper.

**Diagnostic (I4):** none new — reuse the existing **E1204**. Add a `tests/ui/e1204_tampered_store.txt`
snapshot if one is missing. *(The writer's proposed E1210 would have collided: E1210 is already
"unknown/reserved target in `packages:` block", D-TGT1/D-TGT2.)*

### Step 2 — Lockfile stability: manifest-to-lock completeness check (`Source/Lock.rs`)

Add `verify_all_manifest_deps_locked(manifest, lock)` which checks that every dep named in
the manifest appears in the lock with a `locked` revision. Currently `verify_lock_matches_manifest`
only checks the inverse (no extra entries in lock). Make it bidirectional. Emit **E1217**
(dep in manifest has no locked revision) — this fires in `--locked` CI mode and during
publish. *(E1211, the writer's original pick, is taken: "`packages:` block uses removed
`kind:` field". E1217 is the first free E12xx slot.)*

### Step 3 — Semver compatibility tests (`Source/Publish/SemVer.rs`, `Source/Publish/Diff.rs`)

`Publish/SemVer.rs` and `Publish/Diff.rs` exist. Wire them into `jet publish` (pre-publish
gate in `Source/CmdSupply.rs`):

Before a publish:
1. `Diff::diff_public_api(old_api, new_api) -> Vec<BreakingChange>` (`Diff.rs:23`) — compare
   public fn signatures and struct fields against the previous published version's API snapshot
   (`Publish/Schema.rs` / `API.rs`).
2. `SemVer::classify_bump(old, new) -> BumpKind` (`SemVer.rs:81`) → Patch | Minor | Major;
   combine with the breaking-change set from step 1.
3. If the diff is `Major` but the version bump is only `Minor` or `Patch`, emit **E1218**
   (breaking API change requires major version bump) and abort.

This enforces semver correctness at publish time, not just advisory.

**Diagnostic (I4):**
E1218 — breaking API change without major version bump. Add to `docs/spec/diagnostics.md`
and snapshot. *(E1212, the writer's pick, is taken: "package declared in `packages:` but no
`module` found". E1218 is free.)*

### Step 4 — Package signing (`Source/Publish/`, `Source/Lock.rs`) — D-PKGSIGN1 = B + A opt-in

**RATIFIED 2026-06-24 = B (always-on checksum floor) + A as an opt-in, non-blocking
authenticity layer.** Sigstore/keyless (C) rejected (needs network + a transparency-log
service, at odds with offline-first/std-only). Build both tiers:

**Tier B — checksum integrity (always on, mandatory).** This is Step 1: `verify_entry`
re-hashes the store tree (SHA-256) on every `link_into_project` and propagates **E1204** on
mismatch. This is the security model for the default path and needs no key ceremony. Document
it as the baseline; nothing else is required for a beginner or a simple/local/unpublished
project.

**Tier A — Ed25519 author signing (opt-in, never a hard gate).** Layers authorship proof on
top of the checksum:

- **Consumer is silent unless it fails.** When a dependency has a pinned author key and a
  signature, `jet fetch` verifies it offline and says nothing on success; on a mismatch it
  emits a teaching diagnostic (new code from the free E12xx range at impl) and refuses the
  install. No key command is ever required to *consume* a package.
- **`require_signed` stays OFF by default**, a per-registry / per-dependency policy an org can
  turn on. It is **not** a gate that refuses unsigned packages out of the box — unsigned
  packages still install with checksum integrity (Tier B).
- **Publishing auto-keys on the magic path.** `jet publish` generates and stores a keypair on
  first publish (`~/.jet/keys/ed25519`) — no separate `jet keygen` step — then prints one line
  nudging `jet key backup`. Experts opt into explicit keygen, hardware keys, and out-of-band
  fingerprint pinning.
- **Key distribution:** TOFU on first pin (author public key published in the registry index),
  with the pinned fingerprint recorded in the lockfile; experts may require an out-of-band
  fingerprint in `pkg.jet`.

**Capability note (I6):** `jet.crypto` today ships only SHA-256 (`jet_ring_crypto_sha256*` in
`CoreLib.rs`) — there is **no** Ed25519 or SHA-512. Tier A means implementing Ed25519 + SHA-512
**natively in the ring layer** (preferred), or delegating to a `signify`/`ssh-keygen`
subprocess. Tier B reuses the existing SHA-256.

**Build (Tier A):**
- add `LockedPackage::signature: Option<String>` + pinned-key fingerprint to `Lock.rs`;
- implement `verify_signature(pubkey, sig, content)` and keygen/sign in `Source/Publish/Sign.rs`
  (new file);
- `jet fetch` verifies opt-in signatures after the mandatory `verify_entry` (Tier B) in
  `Fetch.rs`;
- `jet keygen` / `jet key backup` verbs + `jet publish` auto-keygen-on-first-publish in
  `main.rs`;
- enforce `require_signed` only where a registry/dep opts in.

### Step 5 — Offline builds and vendoring (`Source/Publish/Vendor.rs`)

`Publish/Vendor.rs` is a stub. Implement:

`jet vendor` command: copies all locked deps into `vendor/` in the project root, writes
`vendor/manifest.json` with name + version + fingerprint for each. In `--locked` mode,
`Fetch.rs` checks for `vendor/<name>/` before attempting network access (git subprocess);
if found and fingerprint matches, use the local copy. This enables fully offline builds.

**CLI:** `jet vendor` verb in `Source/main.rs`. `--vendor-dir <path>` flag to relocate.

### Step 6 — SBOM integration into build output (`Source/Publish/SBOM.rs`, `Source/CmdCompile.rs`)

`SBOM.rs` already emits SPDX 2.3. Wire it into `jet build`:

- After a successful build, if `--sbom` flag is passed, write `<output>.spdx` alongside the
  binary.
- The SBOM includes: root package, all locked deps with their tree-hash checksums, and the
  Jet compiler version.
- `jet publish` always writes the SBOM to the registry index (prepublish gate in
  `CmdSupply.rs`).

**PackageChecksum field** in `SBOM.rs` currently emits `NOASSERTION`; replace with
`SHA256: <tree_hash>` from the lockfile entry.

### Step 7 — Advisory integration (`Source/Publish/Advisory.rs`)

`Advisory.rs` exists. Implement `jet audit`:

- Reads the lockfile; fetches the advisory database (a known git repo URL, or a local
  `advisory-db/` path for offline mode).
- Cross-references each `(name, version)` against advisories.
- Prints a table of vulnerabilities by severity; exits nonzero if any CRITICAL advisory
  matches.
- Gate: `jet publish` runs `jet audit` as a pre-publish check.

---

## Files touched

| File | Change |
|------|--------|
| `Source/Store.rs` | Mandatory `verify_entry` on link; reuse existing E1204 |
| `Source/Lock.rs` | Bidirectional manifest/lock check; E1217; `signature` + pinned-key fingerprint fields (D-PKGSIGN1 Tier A) |
| `Source/Fetch.rs` | Vendor fallback; opt-in signature verify after mandatory verify_entry (D-PKGSIGN1) |
| `Source/Publish/Registry.rs` | enforce `require_signed` only where opted in (off by default) |
| `Source/CmdSupply.rs` | Semver gate E1218; `jet audit` call; SBOM emit |
| `Source/CmdCompile.rs` | `--sbom` flag; SBOM write |
| `Source/Publish/SemVer.rs` | `classify_diff` wired to `jet publish` |
| `Source/Publish/Diff.rs` | API diff wired to `jet publish` |
| `Source/Publish/Vendor.rs` | Full implementation |
| `Source/Publish/Advisory.rs` | `jet audit` implementation |
| `Source/Publish/SBOM.rs` | Real checksum in PackageChecksum |
| `Source/Publish/Sign.rs` (new, D-PKGSIGN1 Tier A) | Ed25519 keygen + sign + verify; native ring-layer Ed25519/SHA-512 (I6) |
| `Source/main.rs` | `jet vendor`, `jet audit`, `jet keygen`, `jet key backup`, `--sbom`; `jet publish` auto-keygen on first publish |
| `docs/spec/diagnostics.md` | E1217, E1218 entries (E1204 already present) |
| `tests/ui/` | e1204_tampered_store, e1217, e1218 snapshots |

---

## Decision verdict

**D-PKGSIGN1 ratified 2026-06-24 = B + A opt-in** (checksum integrity floor always on;
Ed25519 author signing opt-in and non-blocking, `require_signed` off by default). **Every
step (1–7) is now unblocked — this plan is implement-ready for the burn-down.**

## Implementation status (2026-06-25, c122)

Done — the buildable, registry-independent surface:

- **Step 1 / Tier B (E1204)** — already wired: `Store::verify_entry` runs before
  `link_into_project` at both fetch sites (`Source/Fetch.rs:273`/`365`). Mandatory SHA-256
  integrity floor on every install; no key ceremony.
- **Step 2 (E1217)** — `Lock::verify_all_manifest_deps_locked` (bidirectional completeness),
  enforced in the `--locked` fetch path (`Source/Fetch.rs`). Every manifest dep must resolve
  to a pinned version.
- **Step 3 (E1218)** — local SemVer gate in `run_publish`: diffs the current public API against
  the frozen API snapshot (`ApiFreeze::load_snapshot`); a breaking change under a non-major bump
  is E1218 (`--force` overrides). Distinct from the registry-side E2601.
- **Step 5 — `jet vendor [--vendor-dir <path>]`** — copies deps into a vendor tree (default
  `vendor/`, relocatable) + writes `vendor/manifest.json` (name/version/fingerprint per dep).
- **Step 6 — `jet build --sbom`** — writes `<bin>.spdx` (SPDX 2.3) next to the binary.
  Dep `PackageChecksum` already carries the real `SHA256: <tree_hash>`.
- **Step 7 — `jet audit`** — advisory scan with a per-advisory `Severity` (low|medium|high|
  critical); exits nonzero **only on CRITICAL**, advisory otherwise. E2603 is now severity-
  prefixed. Advisory-DB line format gained an optional 6th `severity` field.

Diagnostics added (I4): E1217, E1218 in `docs/spec/diagnostics.md` + covered by
`tests/pkg.rs`. CLI flags `--vendor-dir`/`--sbom` registered in `Source/CLI.rs`.

**Gated on c96 (registry, open ballot) — NOT built:**

- **Step 4 / Tier A** — Ed25519 author signing in full: `Source/Publish/Sign.rs`, native
  Ed25519/SHA-512 ring primitives, `LockedPackage::signature` + pinned-key fingerprint,
  `jet keygen`/`jet key backup`, `jet publish` auto-keygen-on-first-publish, signature verify
  in `jet fetch`, and `require_signed` enforcement. Every one of these needs the
  registry-publish handshake (key distribution via the registry index, TOFU pinning) that
  c96's open publish/registry ballot owns. `require_signed` already exists on `RegistryConfig`
  (OFF by default) and stays inert until then.
- **`jet publish` registry upload** + the "publish always emits an SBOM to the registry index"
  half of D-SUPPLY1 — both need the live registry (c96).
