# Signed binary/source package cache

**Card:** Tower #3 (`c56`) · **Epoch 4** · **Status:** ready · **workOrder:** 13
**Blocked by:** `c96` (registry + publish UX) — **resolved**, see §0.
**Ratified law this card implements inside:** `D-JPK-CACHE1=A`, `D-CASTORE1=A`.
**No open decision.** Every owner-facing choice below is already ratified; this
is a pure implementation plan.

## 0. Gate check — is c96 still blocking?

No. `c96` ("M12.2 registry + jet registry publish UX") is `phase: done`. Its own log
records the split precisely: c96 shipped the **local** publish surface —
dirty-tree gate (E2605), test gate, SemVer diff (E1218/E2601), `jet registry yank`
marker (E2606), resolver (`select_highest_compatible`, E2602) — and
deliberately left the **actual git-registry network push** stubbed
(`Source/CmdSupply.rs` prints "registry upload pending c56"), by design,
because that push was scoped to *this* card. `c56.blockedBy: ["c96"]` is
satisfied: everything c56 needs from c96 (manifest, lock schema, resolver,
`RegistryConfig`) already exists. **c56 is unblocked. Proceed.**

## 1. What ships this epoch vs. later

`D-JPK-CACHE1=A` (ratified 2026-07-02) splits this card's scope explicitly:

- **NOW (this card, E4):** (a) make `jet registry publish`/`jet registry yank` actually push to
  the git-registry index instead of validating-and-explaining; (b) freeze the
  binary-cache **envelope fields** (output hash, platform key, signature
  slot, provenance link) into the hangar/lock schema so nothing later has to
  migrate every lockfile in existence.
- **LATER (Epoch 6, separate card, behind the TLS gate):** the actual
  output-hash-addressed HTTP(S) substitution *protocol* (`jet cache push`,
  substituting a hangar object instead of building from source). Do not
  build the network protocol now — only the schema it will slot into.

Card body's "signed binary/source cache was design-only... out of scope"
note is stale as of D-JPK-CACHE1; the design is no longer open, it's ratified
and scoped as above.

## 2. Part A — git-registry push (make `jet registry publish`/`jet registry yank` real)

The registry is **not** a bespoke network service — `Source/Publish/Registry.rs`
already models it as a git repo (`RegistryConfig.url`, default
`https://github.com/jet-lang/registry`). "Push to the registry" = write an
index entry file + `git commit` + `git push`, using the same
`std::process::Command::new("git")` pattern `Source/CmdSupply.rs` already uses
for the dirty-tree check (`git_dirty_files`, line 18). No new external
dependency, no new I6 surface.

Index layout (one file per package, versions appended — mirrors crates.io's
sparse index and cargo's proven shape):

```
<registry>/index/<name>/<name>.jsonl     # one JSON line per published version
```

Each line (the "index entry" this card and `c146`/package-signing both write
to):

```json
{"name":"textkit","version":"1.2.0","content_hash":"sha256-...","fingerprint":"sha256-...","yanked":false}
```

`content_hash` / `fingerprint` reuse the exact fields already on
`LockedPackage` (`crates/jet-driver/src/Lock.rs:32-35`) — do not invent new
hash field names here; signature/key fields are added by `c146` on top of
this same line (see `package-signing.md` §3).

### Build order

1. **`Source/Publish/Registry.rs`** — add `index_repo_path(registry: &RegistryConfig) -> PathBuf`
   (a `~/.jet/registry-index/<registry-name>/` clone cache) and
   `fn ensure_index_clone(registry: &RegistryConfig) -> Result<PathBuf, Diagnostic>`
   (git clone if absent, `git pull --ff-only` if present — same
   `Command::new("git")` idiom as `git_dirty_files`).
2. **`Source/Publish/mod.rs` (or new `Source/Publish/Index.rs`)** —
   `fn write_index_entry(repo: &Path, entry: &IndexEntry) -> io::Result<()>`
   (append JSONL line, create `index/<name>/<name>.jsonl` if missing) and
   `fn mark_yanked(repo: &Path, name: &str, version: &str) -> io::Result<()>`
   (rewrite the line's `yanked` field — JSONL rewrite-in-place: read all
   lines, patch, write back).
3. **`Source/CmdSupply.rs::run_publish`** — after the existing 3-step gate
   (build/tests/API diff) passes, replace the "upload pending c56" stub with:
   clone/pull index → write entry → `git add` + `git commit -m "publish
   <name> <version>"` + `git push`. On push failure (network, auth, or a
   **version-immutability conflict** — the line already exists and isn't a
   yank) print a clean diagnostic (new code, §4) — never an ICE (I2 doesn't
   apply here since this is jet's own CLI, not generated-code rejection, but
   the same "never a raw stack trace" bar applies).
4. **`Source/CmdSupply.rs::run_yank`** — same clone/pull, then
   `mark_yanked` + commit + push, replacing its "upload pending c56" stub.
5. **Version immutability (D-VERSION1):** before writing, check the index
   for an existing non-yanked line with the same `name`+`version`; refuse
   with the new E-code below. This is the enforcement point D-VERSION1
   promised but couldn't land without a real push target.
6. **Fetch side:** `Source/Fetch.rs` (or wherever `registry` dep resolution
   currently lives — confirm at implementation time) gains
   `fn resolve_from_index(registry: &RegistryConfig, name: &str) -> Vec<IndexEntry>`,
   reading the same JSONL file (clone/pull-then-read), replacing E1207
   ("registry dependency not yet supported") for the default registry.

## 3. Part B — freeze the binary-cache envelope (schema only)

Extend `LockedPackage` (`crates/jet-driver/src/Lock.rs:25`) with the four
frozen fields, all `Option<...>` so old lockfiles keep parsing (same pattern
as `content_hash: Option<String>` already there):

```rust
/// D-JPK-CACHE1=A: content hash of the BUILT output (not the source tree —
/// see `content_hash` for that). `None` until a binary cache actually exists
/// (Epoch 6); present once a substitutable object is produced for this entry.
pub output_hash: Option<String>,
/// D-JPK-CACHE1=A: target triple this output_hash was built for. A package
/// with builds for multiple platforms gets one LockedPackage entry per
/// platform key once the cache exists; `None` for source-only entries.
pub platform_key: Option<String>,
/// D-JPK-CACHE1=A: Ed25519 signature over `output_hash`, base64. Reuses the
/// exact signing machinery `c146` (package-signing) builds — one signature
/// mechanism, not two (I8). `None` until a cache object is signed.
pub signature: Option<String>,
/// D-JPK-CACHE1=A: provenance link back to the exact build that produced
/// `output_hash` — the source `fingerprint` (already on this struct) IS the
/// provenance link; this field is reserved for a future builder identity
/// (e.g. "built by CI run <url>") once the protocol ships. `None` for now.
pub provenance: Option<String>,
```

Do not build anything that populates these fields yet — that's the E6
protocol card. This card only makes the shape exist so E6 is additive.
Round-trip test: a lockfile with all four fields `None` parses identically
before/after; a hand-written lockfile with all four fields populated
round-trips through `Lock::load`/`Lock::write` unchanged (byte-for-byte on
the new fields).

`signature` here is the SAME field shape `c146` adds — coordinate: whichever
of `c56`/`c146` lands first defines the field, the other reuses it. Do not
add two signature fields under different names.

## 4. New diagnostics (I4 — each needs a ui snapshot)

Next free jet/CLI code after E1225 is **E1226**:

| Code | What/why/fix |
|---|---|
| E1226 | `jet registry publish` refused: `{name}` `{version}` already exists in the registry index and is not yanked. Published versions are immutable (D-VERSION1) — a version can never be overwritten, only yanked. Fix: bump the version in `pkg.jet` and publish again, or `jet registry yank` the existing version first if it was a mistake (yanking hides it from new resolution, it does not free the version number). |
| E1227 | `jet registry publish`/`jet registry yank` couldn't reach the registry index at `{url}`: `{detail}`. The git push failed (network, auth, or a stale local clone). Fix: check network/credentials for `{url}`, or run with a `--registry` pointing at a reachable mirror. |

Add both to the two diagnostics.md tables (summary line ~485 range, full
message/why/fix table near the E12xx block) — do not add only one.

## 5. Examples / golden tests (I5)

- `examples/features/jetpack/publish-push.jet`-style test harness (not a
  runnable `.jet` example — this is CLI behavior): an integration test under
  `tests/` that spins up a scratch bare git repo as the "registry", runs
  `jet registry publish` against it, asserts the index file gained the JSONL line, then
  runs `jet registry publish` again with the same version and asserts E1226.
- A second test: `jet registry yank <version>` against the scratch registry, assert
  the line's `yanked` flips to `true` and a subsequent resolve skips it.
- Lockfile round-trip test (schema-only, §3) lives beside the existing
  `content_hash` round-trip test in `crates/jet-driver/src/Lock.rs`'s test
  module (or `Source/`'s lock tests — confirm exact location at
  implementation time; grep `content_hash` test coverage first).

## 6. Exit criteria

- `jet registry publish` against a real (scratch, in tests) git registry index
  actually writes and pushes an entry; the old "upload pending c56" message
  is gone from both `run_publish` and `run_yank`.
- Version immutability enforced at push time (E1226), not just documented.
- `jet registry yank` flips the index entry, doesn't delete it.
- `LockedPackage` carries the four frozen envelope fields, all optional,
  fully round-tripping; nothing populates them yet (that's E6).
- Two new E-codes in both diagnostics.md tables + ui snapshots.
- `nix develop -c cargo test` green; no `unsafe` substring introduced
  (golden.rs greps for it).
