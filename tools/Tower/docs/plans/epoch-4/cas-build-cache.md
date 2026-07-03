# Content-addressed build cache normalization contract

**Card:** Tower #85 (`c1dix8nw`) · **Epoch 4** · **Status:** ready
**Ratified law:** `D-BUILDNORM1=A` (AST-level, rename-sensitive cache-key
contract) — this card's plan field in tower.json is the terse ratified
summary; this file is the full expansion per the E4-prep brief.
**Interlock:** `D-JPK-CACHE1=A` freezes the same four envelope field names
(output hash, platform key, signature slot, provenance link) that
`signed-package-cache.md` adds to `LockedPackage` — this card's normalized
key becomes that `output_hash`, so name it that way from the start.
**No open decision.**

## 0. What's actually broken today (found 2026-07-02, must fix in this card)

Real, reproduced bug in the current cache, not a hypothetical:
`Source/BuildCache.rs` + `Source/CmdCompile.rs::build()` (line 1000-1175).

- `cache_key()` (`BuildCache.rs:26`) correctly SHA-256-hashes
  `generated_rust_code + profile_tag`.
- But `build()` compiles **directly onto a shared, non-content-addressed
  path**: `bin_path(file)` = `build/<stem(file)>` (`CmdCompile.rs:811`,
  called at lines 248/260/272/282/354), with no per-process uniqueness.
  `rustc ... -o &bin` (`CmdCompile.rs:1112`) writes straight there.
- Two concurrent `jet` processes compiling *different* source with the same
  file stem (e.g. two `main.jet` in different dirs, or rapid `jet run`/`jet
  test` invocations) race: process A's `rustc` finishes, but before A's
  `store_cached(key_A, &bin)` (`CmdCompile.rs:1172`) runs, process B has
  already overwritten `build/<stem>` with its own binary. **A's hash
  permanently maps to B's binary** in the shared `~/.cache/jet/build/`
  cache — a real content-addressing violation (integrity bug, not a perf
  bug). This already broke `tests/cli.rs::simple_exec_runs_without_a_manifest`
  during concurrent Epoch 3 work.
- `store_cached()` (`BuildCache.rs:54`) itself is also non-atomic:
  `fs::copy(bin, &dest)` can be read mid-write by a concurrent
  `try_copy_cached()` on the same key (rarer — same key means same content
  in a correct world — but still a torn-file risk during the copy).

### Fix (do this regardless of the normalization-contract work in §1-3 — it's an independent correctness bug in the same file)

1. **Unique per-process compile target.** In `build()`, compile to a private
   temp path, not the shared `bin` argument directly:
   `let tmp_bin = bin.with_file_name(format!(".{}.tmp.{}", stem, std::process::id()));`
   — `rustc ... -o &tmp_bin`. `std::process::id()` is enough to disambiguate
   concurrent *processes*; no external tempfile crate needed (I6).
2. **Store from the private path.** `store_cached(&key, &tmp_bin)` — this
   is now racing only against processes computing the exact same key (same
   content by construction), which is safe to overwrite.
3. **Then copy/rename into the shared display path.** `fs::rename(&tmp_bin,
   &bin)` (or copy+remove if `bin`'s directory differs — same filesystem in
   practice since both are under `build/`, so `rename` is atomic and cheap).
   Whichever process finishes last "wins" the human-readable `build/<stem>`
   slot — that's fine, it was always just a convenience path for `jet run`/
   `jet build`'s own immediate use, never a content identity. The content
   *cache* (`~/.cache/jet/build/<hash>/bin`) is now always correct
   regardless of this race.
4. **Make `store_cached` itself atomic:** write to
   `dir.join(format!("bin.tmp.{}", std::process::id()))` then
   `fs::rename` into `dir.join("bin")`. `try_copy_cached`'s reader then never
   observes a partially-written file.
5. **Regression test:** spawn N (e.g. 8) concurrent `jet run` processes on
   distinct source files that happen to share a file stem (e.g.
   `a/main.jet`, `b/main.jet`, ... all named `main.jet` in different temp
   dirs) with distinct content (different literal output strings); assert
   every process's stdout matches its own source, and that the resulting
   cache entries are internally consistent (recompute each key, `try_copy_cached`
   it fresh, confirm the binary it returns actually produces that source's
   expected output when run). This is the test that would have caught the
   real bug.

## 1. The normalization contract (D-BUILDNORM1=A)

Ratified contract: cache key = `SHA256(canonical_ast_bytes)`, where
`canonical_ast_bytes` is the parsed AST (post-parse, **pre-sema**) with
whitespace and comments stripped, identifiers kept as-written (rename-
sensitive). Two definitions differing only by whitespace/comments hash
identically; a local rename produces a different key.

**Why this matters beyond "correctness":** today's `cache_key()` hashes the
*generated Rust source* (post-codegen) — meaning sema + codegen must run to
completion before the cache can even be consulted. That defeats the point of
an incremental cache: unchanged input should skip the expensive pipeline
stages, not just skip `rustc`. Moving the key to pre-sema AST bytes lets
`jet build`/`jet run` short-circuit the whole front end on a cache hit.

### What "canonical AST bytes" means, precisely

Serialize the parsed `AST` (`crates/jet-foundation/src/AST.rs`) to bytes
deterministically:

- Include: every node's discriminant/kind tag, every identifier exactly as
  written (source text, not span), every literal value, structural order
  (children in source order — this is what makes reordering produce a
  different key, matching the `D-BUILDNORM1` comparison table's "reorder →
  different key" example).
- Exclude: source spans/line-col positions, whitespace, comments (including
  doc comments — confirm at implementation time whether doc comments feed
  into generated Rust `///` output; if codegen currently threads doc
  comments through to the emitted Rust, hashing must still exclude them
  from the *cache key* per the ratified contract, even though codegen still
  emits them — the contract governs the key, not what codegen does with a
  cache miss).
- Deterministic: no `HashMap` iteration order in the serializer; use `Vec`/
  `BTreeMap` or explicit field order matching struct declaration order.

### Build order

1. **New module `crates/jet-foundation/src/CanonicalAST.rs`** (or
   `crates/jet-parser/src/CanonicalAST.rs` — put it beside `AST.rs`'s owner
   crate): `pub fn canonical_bytes(ast: &AST) -> Vec<u8>` — a deterministic,
   span-free, comment-free serialization. Write it as a straightforward
   recursive visitor over the existing `AST` node enum; no derive macro
   needed, no external crate.
2. **`crates/jet-foundation/src/SHA256.rs`** — already has `sha256_hex`
   (used by `BuildCache::cache_key` today); reuse it:
   `pub fn ast_cache_key(ast: &AST, profile_tag: &str, jet_version: &str) -> String`.
3. **Include a toolchain-version salt.** The ratified contract only
   specifies *AST* normalization; it does not by itself prevent a cache
   entry built by `jet` version N from being served (wrongly) to version
   N+1 after a codegen change with the identical AST. Every serious content
   cache salts by toolchain identity (Rust incremental hashes in the rustc
   version; Nix's derivation closure includes the full toolchain). Fold
   `env!("CARGO_PKG_VERSION")` (or a dedicated `jet::Syntax::COMPILER_VERSION`
   if one exists — check before adding a new constant) into the key
   alongside the profile tag. This is a correctness fix, not a new design
   axis — no ballot.
4. **`Source/CmdCompile.rs`** — move the cache-key computation and
   cache-hit check from *after* codegen (current `build()`, line 1069-1086,
   which receives already-generated `rust_code`) to *before* sema/codegen
   run, operating on the parsed AST. This likely means restructuring the
   call chain above `build()` (wherever `rust_code` is currently produced —
   locate the sema+codegen call sites feeding into `build()`, e.g. around
   `check_with_path`/`Driver::` calls) to check the cache first and skip
   straight to `try_copy_cached` + early return before invoking sema/codegen
   at all. Preserve current behavior exactly for the `use_cache = false`
   paths (FFI-linked, C-linked, cross-compiled builds — `CmdCompile.rs:1065`)
   since those still can't be pre-sema cached (FFI bridge presence, C
   links, and target triple all currently gate caching and stay unknown
   until later in the pipeline — leave that gating logic as-is, just move
   the *key computation* earlier for the cases that do use the cache).
5. **`BuildCache::cache_key`** — keep the existing `cache_key(source: &str,
   profile_tag: &str)` signature working (other call sites / tests may
   depend on it) but add the new `ast_cache_key` alongside it; have
   `CmdCompile.rs` call the new one. Don't delete the old one if anything
   else references it — check call sites first.

## 2. Interlock with `signed-package-cache.md` (`D-JPK-CACHE1`)

The frozen envelope's `output_hash` field (added to `LockedPackage` in
`signed-package-cache.md` §3) is conceptually the same kind of value this
card computes for the *local* build cache — a content-addressed identity for
a build's output. Keep the naming and hash algorithm consistent (SHA-256
throughout, matching `D-CASTORE1=A`'s "consistent with D-PKGSIGN1=B" note)
so that when the Epoch 6 substitution protocol lands, the local build cache
this card hardens and the hangar/lock `output_hash` field are the same
concept computed the same way, not two parallel schemes needing
reconciliation later (I8).

## 3. Tests (I4/I5 — normalization is exactly the kind of contract that
needs pinned tests, not just a description)

Add a test module (`Source/BuildCache.rs`'s existing `#[cfg(test)]` block,
or a new `tests/build_cache_normalization.rs` integration test):

- **Whitespace-insensitive:** `fn add(a: Int, b: Int) -> Int { a + b }` vs.
  the same with extra newlines/indentation → identical `ast_cache_key`.
- **Comment-insensitive:** same function with a leading doc comment and
  inline comments added/removed → identical key.
- **Rename-sensitive:** `fn add(a, b) { a + b }` vs. `fn add(x, y) { x + y }`
  → different keys (this is the D-BUILDNORM1 ratified example verbatim).
- **Reorder-sensitive:** `a + b` vs. `b + a` in the body → different keys.
- **Profile-sensitive:** unchanged from today's existing
  `cache_key_changes_with_profile` test (`BuildCache.rs:68`) — keep it
  green under the new key function too.
- **Version-sensitive:** same AST, different `jet_version` string → different
  keys (regression guard for the toolchain-salt fix in §1 step 3).
- **Race regression** (§0 step 5).

## 4. Exit criteria

- Cache-poisoning race (§0) fixed: `store_cached`/`build()` never let a
  concurrent process's output land under the wrong hash.
- Cache key computed from canonical pre-sema AST bytes, not post-codegen
  Rust source; sema/codegen skipped entirely on a cache hit.
- Toolchain-version salt folded into the key.
- All six normalization properties (whitespace/comment/rename/reorder/
  profile/version -insensitive-or-sensitive as specified) covered by tests
  that currently fail without this card's work and pass after.
- Naming/algorithm consistent with the `D-JPK-CACHE1` envelope fields this
  card's key will eventually feed (`signed-package-cache.md` §3).
- `nix develop -c cargo test` green, including a real concurrent-process
  regression test for §0.
