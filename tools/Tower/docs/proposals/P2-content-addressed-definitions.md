# P2 — Content-addressed definitions

**Status:** idea / proposal (not a plan).

> *Scratchpad:* "A function's true identity is the hash of its content, not its
> name or location. Names become local aliases you can change for free. Prior
> art: Unison. Renaming is instant and global with zero risk; no merge
> conflicts on unrelated code; dependency hell disappears because old code
> still resolves to the exact bytes it always did."

---

## 0. Glossary

- **Content address** — the identity of a definition is a hash of its
  (normalized) body, not its name or file path.
- **Name** — a local, mutable label that points at a content address. Renaming
  changes the label, not the thing.
- **Codebase store** — a content-addressed database of definitions that the
  compiler reads instead of (or alongside) parsing a tree of text files.

---

## 1. The idea in one breath

Today a definition's identity is "the function called `parse` in `parser.jet`."
Move the file or rename the function and every reference must be chased.
Content addressing flips it: identity is `#a3f9…` (the hash of the body);
`parse` is just a name you've pinned to that hash locally. Rename for free —
the hash never moved. Two branches that both edited *different* functions can
never conflict, because they touched different hashes.

```jet
// You write ordinary code:
fn greet(name: String) -> String { "hello, " + name }

// The store records it as:
//   #a3f9c1…  =  fn (String) -> String { "hello, " + $0 }
//   alias "greet" -> #a3f9c1…   (local, editable)
```

---

## 2. What falls out (the actual value)

**Read this with §4's levels in mind.** Most of these payoffs are *Level-3*
(full database) properties — they require names and bodies to live outside
canonical text. The one that survives with text canonical is caching. §4 maps
each payoff to the level that actually delivers it.

- **Renaming is instant, global, risk-free** *(Level 3)*. Identity is the
  alias row, so no downstream consumer can break. With text canonical (L1/L2),
  call sites still contain the callee's *name*, so a rename is an ordinary
  whole-tree LSP rewrite — not Unison's free rename.
- **No merge conflicts on unrelated code** *(Level 3)*. True when the codebase
  is the store. If bodies live in git as text, you merge text and get ordinary
  git conflicts.
- **Dependency hell shrinks** *(Level 2–3)*. Old code resolves to the exact
  bytes it always did; a new version is a new hash that coexists with the old.
  Two versions of a library in one build is not a conflict — it is two hashes.
  (Coexistence ≠ interop: the two hashes are *different types* and values can't
  flow across the boundary — same as Unison.)
- **Perfect incremental builds + caching** *(Level 1 — available now)*. A hash
  that hasn't changed never recompiles and never re-tests. This is the real
  near-term carrot, and the only payoff that holds with text canonical.
- **Psychology** *(Level 3)*, owner's framing: kills naming anxiety and loss
  aversion — but only once identity actually *is* the hash, which L1/L2 don't
  reach.

---

## 3. What it costs / fights

This is the most architecturally invasive idea in the scratchpad. It touches
the **"a file is a complete program"** tenet head-on.

| Tension | Detail |
|---|---|
| **Distribution tenet** (philosophy: "a file is a complete program"). | Unison's model replaces the text-file tree with a database. Jet has explicitly promised `jet run foo.jet` with no project, no store. These must be reconciled: text stays the source of truth; the store is a *derived cache*, not the canonical form. |
| **Tooling expectations.** | Diff, grep, code review, and git all assume text files. A pure Unison-style store breaks every one of them unless text projection is first-class. |
| **`jet fmt` / one mechanical path.** | Hashing must normalize formatting, comments, and local names, or trivial edits churn hashes. That normalization *is* a spec surface. |
| **Diagnostics (I2/I4).** | Errors must point at `file:line` in the text the user wrote, not at a hash. The store can't leak into error messages. |
| **Scope.** | Full Unison semantics (typed runtime, `ucm` codebase manager, no text files) is a different language. Jet would adopt the *idea*, not the whole model. |

---

## 4. Three honest framings (pick the altitude)

The owner's call is really *how much* of this to adopt. Three coherent levels:

**Level 1 — Content-addressed build cache (invisible).**
Hashing is an internal compiler optimization. Users see ordinary files; the
compiler keys incremental compilation and test-skipping on normalized-body
hashes. Zero language-surface change. **Jet already has the seed of this:**
`Source/BuildCache.rs` content-keys builds on `sha256_hex(generated source +
profile)` using the std-only `Source/SHA256.rs`, stored out-of-tree under
`~/.cache/jet/` (the same pattern `FFI.rs` uses). The one caveat that keeps L1
from being a *free* win: normalization correctness is load-bearing. If two
semantically-different bodies normalize to the same hash, a failing test gets
silently skipped — a soundness bug against priority #1. So even L1 needs the
normalization rule spec-pinned and tested like a diagnostic (I4); it is "no
*new syntax*," not "no decision."

**Level 2 — Content-addressed names (semi-visible).**
Add a notion of stable identity so renames and refactors are tracked as
alias moves, and two library versions can coexist by hash. Text files stay
canonical; the store is derived. Touches the package/version story
(`name#ver`, U6 source refs) and `jet fmt` normalization. Medium risk, large
payoff for the dependency story.

**Level 3 — Full Unison-style codebase (visible, invasive).**
Identity *is* the hash everywhere; names are pure projection; the codebase is
a database. Maximum payoff for the psychology and merge story, but collides
with the file-is-a-program tenet and every text-based tool. High risk; likely
post-v1 if ever.

---

## 5. Fit with Jet's existing decisions

- **Pairs with the package story — but isn't it yet.** `name#ver` pins are
  deliberately human-readable *semver strings* (VERSION-#), and U6
  `provider@target` is a *source location* — neither is a body hash, so don't
  call them content-addressed. Level 2's fit is narrower and friendlier: hashes
  could *back* a pin (verify the bytes behind `textkit#1.2.0`) without changing
  its spelling.
- **Pairs with P1 capability tags.** A hash is a perfect key for "this exact
  code was audited / is hardened" — maturity and capability facts attach to a
  content address and never go stale on rename.
- **Reinforces I3 (dumb codegen).** Hashing is a sema/front-end concern;
  codegen is unaffected.

---

## 6. Implementation sketch (not a plan)

- **Normalization pass** after parse: strip formatting/comments, alpha-rename
  locals to positional slots, canonicalize so semantically-identical bodies
  hash equal. (Hardest and most spec-worthy piece — and it overlaps `jet fmt`,
  which already canonicalizes formatting under one-mechanical-path; reuse that
  machinery rather than build a parallel normalizer.)
- **Hash + store**: content hash via the existing std-only `Source/SHA256.rs`
  (do **not** pull in `blake3`/`sha2` — I6); an out-of-tree store under
  `~/.cache/jet/` mapping hash → definition and name → hash. **Not `.jet/`** —
  U7 (file-is-a-program) forbids `jet run foo.jet` from needing any in-tree
  ecosystem dir; `.jet/` is project-mode only. `Source/BuildCache.rs` already
  works exactly this way and is the L1 seed.
- **Resolver**: name lookup goes name → hash → definition; unchanged hashes
  short-circuit compile and test.
- **Text projection** (Levels 2–3): render any hash back to canonical Jet
  text so diff/review/grep keep working.

External-crate rules (I6) are satisfied by reusing `SHA256.rs` — this stays
std-only compiler-internal Rust.

## 7. Open decisions for the owner (future ballot rows)

1. **Altitude.** Level 1 (build cache only), 2 (stable names + versions), or 3
   (full codebase)? Determines everything below.
2. **Canonical form.** Does text remain the source of truth (store is a
   derived cache) — strongly recommended to keep the file-is-a-program tenet —
   or does the store become canonical (Unison)?
3. **Normalization spec.** What does the hash ignore (formatting, comments,
   local names) and what does it preserve? This is a real, testable spec
   surface and must be pinned like a diagnostic.
4. **Version coexistence.** Should two hashes of "the same" library be allowed
   in one build (kills dependency hell) or rejected (one version per name)?
   Note coexistence ≠ interop: the two are different types and values can't
   cross between them.
5. **Hash-format stability.** If normalization or the AST changes between
   compiler versions, every hash moves. Harmless for an L1 cache (just
   invalidation) but fatal at L2/L3 where the hash *is* identity — it silently
   breaks "old code resolves to the exact bytes it always did." Unison versions
   its hash format explicitly; Jet must decide whether to pin/version it.

## 8. Recommendation

Adopt **Level 1 now** — an invisible win (caching, incremental builds,
test-skipping) that needs **no new syntax** and largely already exists in
`BuildCache.rs`. Its one gate is the normalization rule, which must be
spec-pinned and tested (decision #3) before it can be trusted not to skip a
failing test. Treat **Level 2** as a serious post-cache design tied to the
package story, gated on the canonical-form and hash-stability decisions (#2,
#5) so it never violates file-is-a-program. Hold **Level 3** as a north-star
reference, not a v1 target — its marquee wins (free rename, conflict-free
merges, the psychology payoff) are exactly the ones the file-is-a-program tenet
forecloses below L3. Each numbered decision above is a ballot row, not a build
step.
