# c122 — Strengthen package and ecosystem trust
**Decision:** none required for the hardening plan. Registry signing protocol choice
(see D-PKGSIGN1 below) is one unresolved owner decision.
**Gate:** none — most work is unblocked now.

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

### Step 4 — Package signing (`Source/Publish/`, `Source/Lock.rs`)

**NEEDS BALLOT: D-PKGSIGN1** — signing scheme choice. Options:

A. **Ed25519 key pair per publisher, signatures in the registry index.** Registry stores
   `<package>-<version>.sig`; `jet fetch` verifies before installing. Requires a key
   management CLI (`jet keys generate`, `jet keys trust <pub_key>`). **Capability note:**
   `jet.crypto` today provides only SHA-256 (`jet_ring_crypto_sha256*` in `CoreLib.rs`) — there
   is **no** Ed25519 or SHA-512. Under I6 (zero external crates in `Source/`), Option A means
   implementing Ed25519 + SHA-512 natively in the ring layer, or delegating to a `signify`/
   `ssh-keygen` subprocess. The ballot must not claim signing "reuses existing `jet.crypto`"
   beyond the SHA-256 hash.

B. **Checksum-only (no asymmetric signing).** Registry publishes SHA-256 of each tarball;
   `jet fetch` verifies the hash matches the lock. No key management. Weaker than A (a
   compromised registry can publish a matching hash for a malicious payload), but zero key
   ceremony for publishers.

C. **Sigstore-style keyless signing** (OIDC + Rekor transparency log). Publisher identity
   is a CI token (GitHub Actions, etc.); no key management. Strongest provenance model.
   Requires an HTTP client (subprocess to `cosign` CLI — keeps Source/ clean, I6).

The owner must pick. Until D-PKGSIGN1 is ratified, `require_signed` in `RegistryConfig`
remains advisory (no enforcement). After ratification:

- For option A: add `LockedPackage::signature: Option<String>` to `Lock.rs`; implement
  `verify_signature(pubkey_path, sig, content)` in `Source/Publish/Sign.rs` (new file) using
  either a native Ed25519 impl or a `signify` subprocess.
- For option B: `verify_entry` already covers this — document it as the security model.
- For option C: add a `cosign` subprocess call in `Fetch.rs` after download.

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
| `Source/Lock.rs` | Bidirectional manifest/lock check; E1217; `signature` field (D-PKGSIGN1) |
| `Source/Fetch.rs` | Vendor fallback; signature verify call (D-PKGSIGN1) |
| `Source/CmdSupply.rs` | Semver gate E1218; `jet audit` call; SBOM emit |
| `Source/CmdCompile.rs` | `--sbom` flag; SBOM write |
| `Source/Publish/SemVer.rs` | `classify_diff` wired to `jet publish` |
| `Source/Publish/Diff.rs` | API diff wired to `jet publish` |
| `Source/Publish/Vendor.rs` | Full implementation |
| `Source/Publish/Advisory.rs` | `jet audit` implementation |
| `Source/Publish/SBOM.rs` | Real checksum in PackageChecksum |
| `Source/Publish/Sign.rs` (new, D-PKGSIGN1-gated) | Signing/verification |
| `Source/main.rs` | `jet vendor`, `jet audit`, `--sbom` verbs/flags |
| `docs/spec/diagnostics.md` | E1217, E1218 entries (E1204 already present) |
| `tests/ui/` | e1204_tampered_store, e1217, e1218 snapshots |

---

## Decision verdict

**NEEDS BALLOT: D-PKGSIGN1** — package signing scheme (Ed25519 key pairs vs checksum-only
vs Sigstore keyless). All other steps in this plan are unblocked.
