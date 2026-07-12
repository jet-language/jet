# Package signing tier A (Ed25519)

**Card:** Tower #13 (`c146`) · **Epoch 4** · **Status recommend:** ready
(currently `planning` — advance it; see §0/§1, nothing left to design).
**Blocked by:** `c96` — **resolved**, see `signed-package-cache.md` §0 (same
gate, same resolution — c96's local publish surface is done; the network
push this card's key-distribution rides on is `c56`'s job, see §2 below for
the real sequencing note tower.json's flat `blockedBy` doesn't capture).
**Ratified law:** `D-PKGSIGN1` (tier A opt-in Ed25519 author signing, tier B
SHA-256 checksum always-on floor already shipped in `c122`).
**No open decision.** See §1 for why the crypto-dependency question in this
card's brief is already answered by existing, owner-approved infrastructure.

## 0. Real sequencing note (fix the flat blockedBy)

`c146.blockedBy` lists only `c96`, but the card body's own "TOFU pinning via
the index" mechanism needs an actual index file to pin keys *in* — that's
`c56`'s (Tower #3) git-registry push, not `c96`. Practically: land `c56`'s
index-write plumbing (`write_index_entry`/`Source/Publish/Registry.rs`) first
or in the same slice, then extend the same JSONL line with the signature
fields below. Don't block on a full `c56` merge — the index format is
additive (new keys on the same JSON line), so this can be built in parallel
against a stub/local index and wired together at integration time.

## 1. The crypto dependency question is already closed

The planning brief asked whether Ed25519 needs a new external crate (which
would be an owner gate). It does not:

- `ed25519-dalek` **2** is already an owner-approved dependency
  (`D-DEP-CRYPTO1=A`, ratified) — see
  `crates/jet-driver/src/FFI.rs:189-190` (`ED25519_CRATE_SPEC`) and
  `crates/jet-driver/src/Prelude/Crypto.rs` (`jet_crypto_sign_impl` /
  `jet_crypto_verify_impl`, both fully implemented: sign with a 32-byte
  seed, verify a 64-byte signature against a 32-byte public key).
- This is card body's stale assumption ("native Ed25519/SHA-512 ring
  primitives... only SHA-256 today") — written before `D-DEP-CRYPTO1`
  shipped. The primitive exists. This card is CLI plumbing on top of it, not
  a crypto card.

**The one real design question is architectural, not owner-facing:** how
does `jet`'s own CLI (`Source/`, zero-dependency per I6) invoke Ed25519
signing, given `Crypto.rs` is `include_str!`-embedded as a *template* into a
hidden bridge crate built for **compiled Jet programs** (`crates/jet-driver/src/FFI.rs:63-130`),
not linked into `jet`/`jetpack` itself?

**Answer (reuse the existing idiom, don't invent a new one):** `jet` already
shells out to external processes for privileged operations it can't do
in-process without breaking I6 — it calls `cargo` to build the FFI bridge
crate (`FFI.rs`) and `rustc` to produce the final binary
(`Source/CmdCompile.rs:1102`). Do the same for signing: extend
`build_bridge` (`FFI.rs`) to also emit a `[[bin]]` target
(`jet-crypto-helper`, reusing the *exact same* `CRYPTO_RUNTIME` template
already `include_str!`'d, unchanged) that reads a small stdin protocol
(`keygen` / `sign <secret_key_b64> <message_b64>` / `verify <pub_b64>
<msg_b64> <sig_b64>`) and writes the result to stdout. Cache it in the same
`cache_root`/`BuildLock` keyed cache the rlib already uses (`FFI.rs:285-325`)
— it's a cold-build-once, then-instant-reuse cost, identical to every other
FFI bridge dependency. `Source/Publish/Sign.rs` (new file, §3) shells out to
it via `std::process::Command`, exactly like `git_dirty_files` shells out to
`git`. Zero new Cargo.toml entries anywhere in `Source/` or `crates/jet-driver`;
`ed25519-dalek` stays exactly as confined as `D-DEP-CRYPTO1` already
authorized. This is plumbing, not a new mechanism — no ballot.

## 2. Key storage & `jet registry keygen`

Convention (follows the existing unilateral pattern for tool-owned
directories — `~/.jet/store`, `~/.cache/jet/build` — not a new decision
axis, just consistency):

```
~/.jet/keys/<registry-name>.ed25519      # 32-byte seed, mode 0600
~/.jet/keys/<registry-name>.ed25519.pub  # 32-byte public key, hex-encoded, mode 0644
```

- `jet registry keygen [--registry <name>]` (default registry `"jet"`): generates a
  32-byte seed through the shared D-CRYPTO-RNG1 direct OS provider, with no
  predictable fallback, through the bridge helper's `keygen`
  verb, writes both files, refuses to overwrite an existing key without
  `--force` (E-code below), prints the public key + a one-line nudge:
  `` `jet registry key backup` writes this to <path> — losing it means losing your
  ability to publish signed updates. `` (matches the owner Q&A wording
  exactly — this line is product copy, don't rephrase it).
- D-CRYPTO-KEYGEN-DIAG1=A: OS-entropy failure is a tool error with empty
  stdout and one exact four-line Jet diagnostic. It leaks no raw provider,
  helper, dependency, path, generated-code, or key text; exits 1; leaves no
  key/package/index/temporary artifact; and volatile-zeroizes secret
  temporaries. Auto-keygen aborts before upload or index mutation. An existing
  valid key bypasses entropy and is unchanged. The ballot selected E1275, but
  that code is already assigned to D-JPK-NODAEMON1's sandbox diagnostic;
  D-CRYPTO-KEYGEN-CODE2 must reconcile the code before this projection ships.
- `jet registry key backup [<dest>]`: copies the seed file to `<dest>` (default
  `./jet-signing-key.backup`, printed with a warning to store it somewhere
  safe, e.g. a password manager). No encryption of the backup file itself —
  it's the user's copy to protect; don't invent a passphrase flow, that's
  scope creep beyond what D-PKGSIGN1's Q&A asked for.
- **`jet registry publish` auto-keygen (D-PKGSIGN1 Q&A commitment):** if
  `~/.jet/keys/<registry>.ed25519` doesn't exist when `jet registry publish` runs and
  the registry doesn't have `require_signed: false` forcing skip... actually
  simpler and matches the Q&A literally: **always** auto-keygen silently on
  first publish (no flag needed) — "the beginner's required action is the
  command they were already running." Print the same one-line `jet registry key
  backup` nudge, once, then proceed with signing.

## 3. Signing flow — wire into `c56`'s index line

`c56` (`signed-package-cache.md` §2) writes one JSONL line per published
version. Extend it here with two fields:

```json
{"name":"textkit","version":"1.2.0","content_hash":"sha256-...","fingerprint":"sha256-...","yanked":false,"public_key":"<hex, first publish only, TOFU-pinned>","signature":"<base64 ed25519 sig over content_hash, present only if this publish was signed>"}
```

- `public_key` is written **once**, on the first published version of a
  package — that's the TOFU pin. Every later publish is checked against it
  (§4), not rewritten.
- `signature` = Ed25519 signature (via the bridge helper, §1) over the raw
  bytes of `content_hash` (the same field `c56` already writes — sign what's
  already the integrity anchor, don't invent a second hash to sign).
- Signing is opt-in per `D-PKGSIGN1`: if the user has no key
  (`jet registry publish --no-sign` explicitly, or a registry that sets
  `require_signed: false` and the user passes no sign flag — actually per
  §2 auto-keygen always happens, so in practice every publish IS signed
  unless `--no-sign` is passed). Add `--no-sign` to `jet registry publish` for the
  rare case (e.g. CI without secure key custody) — this is the honest
  escape hatch, not a footgun since checksum (tier B) still applies
  unconditionally.

### Build order

1. **`Source/Publish/Sign.rs` (new)** — `fn ensure_bridge_helper() ->
   Result<PathBuf, Diagnostic>` (calls into `jet_driver::FFI::build_bridge`
   with `needs_crypto: true`, no extern entries, returns the helper binary
   path — extend `FfiLink`/`build_bridge` to expose a `bin_path` alongside
   `rlib_path`); `fn keygen(registry: &str) -> Result<(PathBuf, PathBuf),
   Diagnostic>`; `fn sign(secret_key_path: &Path, content_hash: &str) ->
   Result<String, Diagnostic>` (base64 sig); `fn verify(public_key_hex: &str,
   content_hash: &str, signature_b64: &str) -> Result<(), Diagnostic>`.
2. **`crates/jet-driver/src/FFI.rs`** — extend `build_bridge`'s generated
   `Cargo.toml`/`src/` to also emit `src/bin/jet_crypto_helper.rs` (thin
   stdin-protocol wrapper around the existing `jet_crypto_*_impl` functions)
   when `needs_crypto`; extend `FfiLink` with `helper_bin_path:
   Option<PathBuf>`.
3. **`Source/CLI.rs`** — register `jet registry keygen` and `jet registry key
   backup` verbs (I7: these are new user-typeable subcommands, register in
   `crates/jet-foundation/src/Syntax.rs` alongside existing verb constants).
4. **`Source/CmdSupply.rs::run_publish`** — after building the index entry
   (from `c56`'s §2 work), call `Sign::keygen` if no key exists (auto-keygen,
   printing the nudge once), then `Sign::sign`, attach `public_key`/
   `signature` to the `IndexEntry` before `write_index_entry`. Honor
   `--no-sign`.
5. **`Source/Publish/Registry.rs`** — wire `require_signed` (field already
   exists, currently dead — `RegistryConfig.require_signed`, always `false`
   from both constructors) through to actual enforcement in §4.
6. **`Source/Fetch.rs`** (or wherever registry resolution lands, per
   `c56`'s §2 step 6) — on fetch, if the index entry carries a
   `public_key`+`signature`, verify (§4); if the registry's
   `require_signed` is `true` and the entry carries neither, refuse (E-code
   below).

## 4. Fetch-time verification + `require_signed`

- **Always** (any package with a signature present, regardless of
  `require_signed`): verify `signature` against `public_key` over
  `content_hash`. Mismatch → hard error (I1: never silently accept tampered
  bytes), new E-code below — this is a `core.crypto`-shaped mismatch, not a
  generic "corrupted download," say so.
- **TOFU pin check:** if a locally-cached copy of this package (in
  `.jet/lock` or the local index clone) previously saw a different
  `public_key` for the same `name` at an earlier version, warn (not a hard
  error — key rotation is legitimate; the git-push auth to the registry
  index IS the real trust root, per `D-PKGSIGN1`'s owner Q&A framing of
  publishing risk landing on publishers, not consumers). Print which
  version introduced the new key.
- **`require_signed: true`** (per-registry, off by default): a fetched
  entry with no `signature` is a hard error, new E-code below. This is the
  only place `require_signed` does anything — never a global default.
- **Consumer path stays silent on success** (Q&A commitment: "`jet add`
  verifies the signature silently and only ever speaks on a MISMATCH").

## 5. New diagnostics (I4 — ui snapshot each)

Next free jet/CLI codes after `signed-package-cache.md`'s E1226/E1227 are
**E1228+**:

| Code | What/why/fix |
|---|---|
| E1228 | `jet registry keygen` refused: a signing key already exists at `{path}`. Overwriting it would orphan every package you've published under the old key — consumers who pinned it (TOFU) would see a key-rotation warning on your next publish. Fix: use `jet registry keygen --force` if you're sure (e.g. the old key was compromised), or back it up first with `jet registry key backup`. |
| E1229 | Signature verification failed for `{name}` `{version}`: the signature doesn't match the recorded public key. This means the package was tampered with after signing, or the index entry is corrupt. Fix: do not use this version. Re-run `jet store fetch` after clearing the store entry; if the problem persists, report it — this should never happen for an untampered registry. |
| E1230 | Registry `{registry}` requires signed packages (`require_signed: true`) but `{name}` `{version}` has no signature. Fix: use a different registry, or ask the package author to publish a signed release (`jet registry publish` auto-signs by default — they likely used `--no-sign`). |

Add all three to both diagnostics.md tables.

## 6. Examples / golden tests (I5)

- Extend `signed-package-cache.md`'s scratch-registry integration test:
  after `jet registry publish` against the scratch git repo, assert the index line
  carries `public_key` + a valid `signature`, and that a fresh `jet registry keygen`
  in a clean `$HOME` happened automatically (assert the key files exist).
- Tamper test: hand-edit the pushed index entry's `content_hash` (simulate
  tampering) and assert the next fetch produces E1229.
- `require_signed` test: registry config with `require_signed: true`,
  publish with `--no-sign`, assert fetch produces E1230.
- Key-rotation warning test: publish v1 with key A, hand-rewrite the index
  entry's `public_key` for a v2 line to key B, assert fetch warns (not
  errors).

## 7. Exit criteria

- `jet registry keygen` / `jet registry key backup` work end-to-end, files at the documented
  paths, correct permissions.
- `jet registry publish` auto-generates a key on first use, signs by default, honors
  `--no-sign`.
- Fetch-time verification runs unconditionally when a signature is present;
  `require_signed` enforced per-registry, off by default.
- TOFU pin recorded and checked; rotation warns, never silently accepts a
  different key without saying so.
- Zero new entries in any `Cargo.toml` under `Source/` or `crates/jet-driver`
  — `ed25519-dalek` usage stays 100% inside the existing hidden bridge crate.
- Three new E-codes in both diagnostics.md tables + ui snapshots.
- `nix develop -c cargo test` green.
