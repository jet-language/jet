# Decision ballots — open owner queue

Every open decision, and **nothing else**. The instant a decision is submitted it
leaves this file: it is recorded in the decision log in
[`syntax-decisions.md`](syntax-decisions.md) and removed here. No "recently
ratified" section, no decided history — decided decisions never reappear.

**House rule for whoever edits this file (enforced — a card missing any of these is
not ballot-ready; Tower v2 Focus Mode renders these as labeled facets, so use the
exact bold labels):** every full decision card carries `**Gist:**` (one VERY short
plain sentence — the headline), `**Story.**` (a real person with an
American-traditional name and what they're doing), `**In the wild:**` (a fenced
```jet block of realistic project code where this bites), `**Other languages:**`
(short fenced blocks for Rust/TS/Swift/etc. when a cross-language compare helps),
`**Tradeoffs:**` (a compact table, one row per option, columns that actually differ —
subagent-reviewed), and a **worked example of every option** (each
`- **Option X — <name>.**` bullet with its own fenced ```jet/```shell block; mark the
recommended one `(recommended)`). Close with `**Recommendation:**` + a one-line why.
Put Owner Q&A in `**Owner Q …**` blocks — Tower routes those to a separate Q&A facet,
so keep them out of the recommendation. Decisions not yet drafted to that bar are
listed below as one-liners with a recommendation; expand one into a full card when
it's time to decide it.

---

## Open decisions

_Three card groups open for the owner: **c96** registry / jet publish (D-PUBLISH1A, D-VERSION1, D-RESOLVE1, D-LOCK1), **c136** generic-type serde (D-SERDE9–D-SERDE12), and **c50** build-from-source + wave-2 deps (D-DEP-ARCHIVE1, D-DEP-DB1, D-BFS1). All developed to the house format and agent-reviewed._


## M12.2 registry + jet publish UX — board card c96

**D-PUBLISH1 promoted to full cards (2026-06-24).** Was a deferred stub riding c50 (build-from-source) and c56 (registry-upload) infra. Split into four one-pick cards: **D-PUBLISH1A** (command shape + pre-flight refusals), **D-VERSION1** (version-immutability / re-publish policy), **D-RESOLVE1** (resolver default), **D-LOCK1** (lockfile commit default). All fit the ratified `pkg.jet` manifest + `.jet/lock` model (D-JPK-FILES, VERSION-#, S52/U2) and the `jet add/remove/fetch/update` verb family. New publish-side errors take the next free codes from **E1219** (E1201–E1218 are taken/reserved through D-SUPPLY1).

> Cross-checked against `syntax-decisions.md`: manifest filename is **`pkg.jet`** with identity block `payload: { name, version }` (D-JPK-FILES, 2026-06-18 — the latest rename; `PAYLOAD_FILE = "pkg.jet"` in `Source/Syntax.rs`; the older `pack.jet` was the U10 interim name, retired clean-break). Lockfile is **`.jet/lock`** (U2). Version pins use **`pkg#version`** (VERSION-#). `jet publish` already has two ratified side-contracts: it auto-generates+stores a signing keypair on first publish (D-PKGSIGN1) and always emits an SBOM to the registry index (D-SUPPLY1) — these cards must not contradict those.

---

### D-PUBLISH1A — `jet publish` command shape + pre-flight refusals (rec A)

**Gist:** One `jet publish` verb that reads the version from `pkg.jet` and refuses to publish from a dirty or untested tree.

**Story.** Hank maintains `textkit`, a small Jet string library. He bumps `payload: { version: "1.3.0" }`, runs `jet publish`, and expects the tool to catch it if he forgot to commit or if a test is red — before anything hits the registry where he can't take it back.

**In the wild:**
```jet
// pkg.jet
payload: {
  name:    "textkit"
  version: "1.3.0"
  license: "MIT"
  repository: "github@hank/textkit"
}
packages: { textkit: library }
```
```shell
$ jet publish
error[E1219]: working tree has uncommitted changes
  the version you publish must match committed source, or consumers
  can't reproduce it from `repository`.
  fix: commit or stash your changes, then `jet publish` again
       (expert override: `jet publish --allow-dirty`)
```

**Other languages:** `cargo publish` reads `version` from `Cargo.toml`, runs a verify-build, and warns (does not block) on a dirty tree unless `--allow-dirty`. `npm publish` takes the version from `package.json` (no dirty-tree check). `go` has no publish command — you `git tag vX.Y.Z` and push; the proxy fetches the tag. Jet follows cargo's "manifest is the source of truth" but makes the dirty-tree check a refusal, not a warning, to fit the beginner-safety bar.

**Tradeoffs:** (subagent-reviewed)

| Option | Command surface | Version source | Pre-flight | One-path fit |
|---|---|---|---|---|
| A `jet publish` (recommended) | one verb, sibling of `add/update` | from `pkg.jet` | refuse dirty tree + failing tests; `--allow-dirty` escape | strong — extends the ratified verb family |
| B `jet publish <version>` | version typed on CLI | from argument | same refusals | weak — two sources of truth for version (CLI vs manifest) |
| C `jet release` | new top-level verb | from `pkg.jet` | same refusals | weak — diverges from `add/remove/fetch/update` naming |

- **Option A — `jet publish`, version inferred from `pkg.jet`, refuse dirty/untested. (recommended)**
```shell
$ jet publish
checking textkit#1.3.0 …
  ✓ tree clean       ✓ tests pass (12)       ✓ version 1.3.0 unused
publishing textkit#1.3.0 to registry …
  ✓ uploaded   ✓ SBOM emitted   ✓ signed (key: hank@textkit)
done. consumers can now `jet add textkit#1.3.0`.
```

- **Option B — `jet publish <version>`, version typed on the command line.**
```shell
$ jet publish 1.3.0
error[E1220]: version 1.3.0 on the command line does not match
  `pkg.jet` (payload.version = "1.2.0")
  the manifest and the published version must agree.
  fix: bump `version:` in pkg.jet, or drop the argument
```

- **Option C — `jet release` as a distinct verb.**
```shell
$ jet release          # same behavior as A, different name
publishing textkit#1.3.0 …
```

**Recommendation:** A — one verb, version owned by the manifest (single source of truth), and footgun refusals turned on by default keep the magic path safe while `--allow-dirty` preserves the expert escape.

**Owner Q (D-PUBLISH1A):** Should "tests pass" be a hard pre-flight refusal (rec) or only a warning? cargo verify-builds but does not run tests; Jet's beginner-safety bar argues for the harder default with `--no-verify` as the expert escape.

---

### D-VERSION1 — version immutability / re-publish policy (rec A)

**Gist:** A published version is permanent; you can never overwrite `textkit#1.3.0`, only publish a new number.

**Story.** Mabel pinned `textkit#1.3.0` in her app six months ago. Hank discovers a bug in 1.3.0 and is tempted to silently re-upload a fixed 1.3.0. If the registry let him, Mabel's "reproducible" build would change underfoot with no version change — the exact supply-chain hazard the checksum floor (D-PKGSIGN1) exists to prevent.

**In the wild:**
```shell
$ jet publish          # 1.3.0 already exists in the registry
error[E1221]: textkit#1.3.0 is already published
  published versions are immutable — re-uploading would change
  what `textkit#1.3.0` means for everyone who pinned it.
  fix: bump `version:` in pkg.jet (1.3.1 for a fix, 2.0.0 for a
       breaking change), then `jet publish`
```

**Other languages:** crates.io versions are permanent — re-publish is rejected; you may only `cargo yank` (hides from new resolution, keeps existing pins working). npm allows `unpublish` within 72h then locks it (a famous footgun — left-pad). Go's module proxy is immutable + checksummed by design. A and crates.io agree: immutable + yank, never overwrite.

**Tradeoffs:** (subagent-reviewed)

| Option | Re-publish same version | Withdraw a bad version | Reproducibility | Hazard |
|---|---|---|---|---|
| A immutable + `jet yank` (recommended) | refused (E1221) | `jet yank` hides from new resolution, keeps pins | guaranteed | none |
| B immutable, no yank | refused | none — bad version stays selectable | guaranteed | bad versions linger |
| C overwrite allowed | allowed | overwrite | broken — same pin, different bytes | severe (silent drift) |

- **Option A — immutable, with `jet yank` to retract from new resolution. (recommended)**
```shell
$ jet yank textkit#1.3.0
yanked textkit#1.3.0 — new resolves skip it; existing `.jet/lock`
  pins still install it. publish 1.3.1 as the fix.
$ jet yank --undo textkit#1.3.0     # reversible
```

- **Option B — immutable, no retraction mechanism.**
```shell
$ jet yank textkit#1.3.0
error: no such command — versions cannot be retracted; publish a higher version
```

- **Option C — allow overwriting an existing version.**
```shell
$ jet publish          # silently replaces 1.3.0
warning: overwrote textkit#1.3.0   # Mabel's locked build now differs
```

**Recommendation:** A — immutability is the only policy compatible with the always-on checksum floor (D-PKGSIGN1) and reproducible `.jet/lock` installs; `jet yank` gives a safe, reversible way to steer people off a bad release without breaking anyone already pinned to it.

---

### D-RESOLVE1 — dependency resolver default (rec A)

**Gist:** A range like `textkit#^1.2` resolves to the highest compatible published version, recorded once in `.jet/lock`.

**Story.** Earl writes `textkit#^1.2` in his `pkg.jet`. When he first builds, the resolver should pick the newest 1.x (say 1.4.2) and write it to `.jet/lock` so his build is frozen — and then *stay* on 1.4.2 until he explicitly runs `jet update`, even if 1.5.0 ships next week.

**In the wild:**
```jet
// pkg.jet
deps: {
  textkit: textkit#^1.2          // range: ">=1.2.0, <2.0.0"
  parsekit: parsekit#1.0.3       // exact pin (VERSION-#)
  glue: { git: "github@earl/glue", tag: "@latest" }   // moving selector
}
```
```shell
$ jet build            # first build, no lock yet
resolving … textkit#^1.2 → 1.4.2   parsekit → 1.0.3   glue → @latest (abc123)
wrote .jet/lock
$ jet build            # later — 1.5.0 now exists, but lock is authoritative
using .jet/lock — textkit 1.4.2 (run `jet update` to move to 1.5.0)
```

**Other languages:** cargo = highest-compatible-in-range, frozen in `Cargo.lock`, moved only by `cargo update`. npm = highest-compatible, but historically re-resolved on install until lockfiles became authoritative (`npm ci`). Go = Minimal Version Selection: the *lowest* version satisfying all constraints (deliberately conservative, no lockfile-as-floor). A matches cargo, which is the closest fit to Jet's already-ratified `@latest` moving-selector + `--locked` freeze model (S52).

**Tradeoffs:** (subagent-reviewed)

| Option | First resolve picks | Repeat builds | Moving to newer | Surprise risk |
|---|---|---|---|---|
| A highest-compatible + lock-authoritative (recommended) | newest in range | frozen by `.jet/lock` | explicit `jet update` | low |
| B exact pins only, no ranges | only `pkg#x.y.z` exact | identical | hand-edit version + re-add | none, but no range ergonomics |
| C highest-compatible, re-resolve every build | newest in range | drifts upward silently | automatic | high (build changes with no source change) |

- **Option A — highest-compatible-in-range, then `.jet/lock` is authoritative. (recommended)**
```shell
$ jet update textkit    # explicit move within the ^1.2 range
textkit 1.4.2 → 1.5.0   updated .jet/lock
$ jet build --locked    # CI: fail if lock would change at all
```

- **Option B — exact pins only; reject range selectors.**
```jet
deps: { textkit: textkit#1.4.2 }   // must name an exact version
```
```shell
$ jet build   # always 1.4.2; to upgrade, hand-edit pkg.jet
```

- **Option C — highest-compatible, re-resolved on every build (no lock floor).**
```shell
$ jet build   # silently jumps 1.4.2 → 1.5.0 the day it publishes
using textkit 1.5.0   # Earl changed nothing; his build did
```

**Recommendation:** A — newest-in-range on first resolve gives good defaults, and a lock-authoritative repeat build gives reproducibility; the only way to move is the already-ratified `jet update`, so there's one path and no silent drift. This is the rec direction captured in the original stub.

---

### D-LOCK1 — is `.jet/lock` committed to version control by default? (rec A)

**Gist:** `jet new` checks `.jet/lock` into git (drops it from `.gitignore`) so teammates and CI rebuild byte-identically.

**Story.** Doris clones Earl's app and runs `jet build`. If `.jet/lock` is in the repo, she gets exactly Earl's resolved versions on the first try. If it's git-ignored, she silently re-resolves and may get different bytes than Earl is running — the lock exists precisely to stop that.

**In the wild:**
```shell
$ jet new myapp
created myapp/
  pkg.jet
  .gitignore        # ignores .jet/cache/ but NOT .jet/lock
  src/main.jet
$ cd myapp && git status
  .jet/lock         # tracked — committed with your first commit
```

**Other languages:** cargo commits `Cargo.lock` for applications (binaries), git-ignores it for libraries — the published-package case. npm commits `package-lock.json` always. Go commits `go.sum` always. Note: D-JPK-FILES' file table currently lists `.jet/lock` as **"Checked in? no"** — adopting A would amend that line to "yes (apps)", which this card exists to settle.

**Tradeoffs:** (subagent-reviewed)

| Option | App `.jet/lock` | Library `.jet/lock` | First-clone reproducibility | Matches D-JPK-FILES table |
|---|---|---|---|---|
| A commit for apps, ignore for libraries (recommended) | committed | git-ignored | guaranteed for apps | amends "no" → "yes (apps)" |
| B commit always | committed | committed | guaranteed | amends to "yes" |
| C never commit (keep current) | ignored | ignored | not guaranteed | matches current "no" |

- **Option A — commit `.jet/lock` for executables, git-ignore it for libraries. (recommended)**
```shell
# jet new myapp   (packages: { myapp: executable })  → .jet/lock committed
# jet new mylib   (packages: { mylib: library })     → .jet/lock git-ignored
```

- **Option B — commit `.jet/lock` everywhere.**
```shell
$ jet new mylib && git status
  .jet/lock   # tracked even for a library (consumers re-resolve anyway)
```

- **Option C — never commit `.jet/lock` (status quo per D-JPK-FILES table).**
```shell
$ jet new myapp && cat .gitignore
  .jet/        # whole managed folder ignored — Doris re-resolves on clone
```

**Recommendation:** A — committing the lock for apps is the only way to make a fresh clone build the same bytes Earl ships (the whole point of a lockfile + the immutable registry), while libraries ignore it because their consumers resolve against their own `pkg.jet`. This mirrors cargo exactly and amends the D-JPK-FILES table line.

**Owner Q (D-LOCK1):** Adopting A requires editing the D-JPK-FILES file-structure table (`.jet/lock` "Checked in? no" → "yes for executables"). Confirm that table amendment is in scope for this decision.

---

## Serde expert tier: generic-type (de)serialization — board card c136

The typed serde derive (`#[Codable]`/`#[Encode]`/`#[Decode]`) ships for concrete types. Generic types are gated by **E2413** (`Source/Sema/CheckerCoreLib.rs:2167`, raised at `:2247`; codegen bails early at `Source/Codegen/Items.rs:299`/`:411`). This card decides how `#[Codable] struct Box<T> { value: T }` lowers — specifically the bound propagation onto the generated `impl`s, mirroring how `derive Comparable` adds `T: PartialOrd` and the existing `rust_extra_clone_bounds`/`rust_extra_jetshow_bounds` (`Source/Generics.rs:447`/`:455`) add per-param Rust bounds. Three sub-decisions: how bounds get onto the impl, whether unbound/skipped params are handled, and what stays gated.

These extend D-SERDE1–8 and reuse the D-CASING1 marker conventions; none contradict the ratified model.

---

### D-SERDE9 — generic serde bound propagation (rec A)

**Gist:** When a `#[Codable]` type has a type parameter `T` used in an encoded field, the generated `impl` automatically requires `T: Encode`/`T: Decode` — the user writes nothing extra. `#[Codable] struct Box<T>` just works for any `Box<T>` whose `T` is itself codable; a `Box<Socket>` fails *at the use site* with the normal "not Encode" error, not at the definition.

**Story.** Margaret maintains an internal API toolkit. She has a generic envelope `Response<T> { status: Int, body: T }` she wants every endpoint to return as JSON. Today she hits E2413 and has to hand-write `jet_encode` for `Response<User>`, `Response<Order>`, `Response<[Item]>` — one impl per payload, all identical. She wants to put `#[Codable]` on `Response<T>` once and have every concrete `Response<Whatever>` serialize, exactly like Rust's `#[derive(Serialize)] struct Response<T>`.

**In the wild:**
```jet
#[Codable]
struct Page<T> {
    items: [T]
    next: String?
    total: Int
}

fn main() {
    page: Page<Order> @= json.decode<Page<Order>>(body) ?? panic("bad page")
    print(json.to_string(page))            // works: Order is Codable
}
```
The generated impl: `impl<T: user_Encode + Clone> user_Encode for user_Page<T> { … }` and the `Decode` mirror with `T: user_Decode`. A `Page<Socket>` (Socket not codable) is rejected at the call to `json.to_string`/`decode<T>` with the standard E2411/E0905 "Socket isn't Encode" message — the definition of `Page<T>` itself is always fine.

**Other languages:**
- **Rust serde** — `#[derive(Serialize)]` on a generic struct auto-adds `T: Serialize` to every type param via the derive macro's bound inference; `#[derive(Deserialize)]` adds `T: Deserialize<'de>`. This is the default and overwhelmingly the right behavior; the `#[serde(bound=...)]` escape hatch exists only for the rare cases the inference gets wrong (D-SERDE11).
- **Swift `Codable`** — synthesized `Codable` conformance for `struct Box<T>` requires you to declare `extension Box: Codable where T: Codable` (conditional conformance) — Swift makes you *write* the where-clause; it isn't inferred. Considered the main papercut of Swift generic Codable.
- **Kotlin** (`kotlinx.serialization`) — `@Serializable class Box<T>(val value: T)` generates a serializer that takes the `T` serializer as a constructor argument; the `T: Any?` constraint is structural, resolved per use. No hand-written bound.

**Tradeoffs:** *(subagent-reviewed)*

| | Auto-bound-all-params (A) | User writes the bound (B) | No propagation / per-instantiation check (C) |
|---|---|---|---|
| Beginner experience | best — zero ceremony | worst — Swift's papercut | ok until a confusing error |
| One-path | yes — matches Clone/JetShow precedent | extra surface (where-clause grammar) | yes but ad hoc |
| Correctness | sound (rustc verifies, I2) | sound | sound but error lands far from cause |
| Error locality | at use site, clear | at use site | at use site |
| Consistency w/ Jet | matches `rust_extra_clone_bounds` | foreign | foreign |

- **Option A — auto-bound-all-params.** *(recommended)* Every type param of a `#[Encode]`/`#[Decode]` type gets `T: Encode`/`T: Decode` added to the generated impl automatically (plus the existing `Clone` extra). No new syntax. Reuses the `rust_extra_*_bounds` machinery verbatim — add `rust_extra_encode_bounds`/`rust_extra_decode_bounds` and feed them to `rust_type_param_list` in `emit_struct_serde`/`emit_enum_serde`.
```jet
#[Codable]
struct Pair<A, B> { left: A, right: B }
// generates: impl<A: user_Encode + Clone, B: user_Encode + Clone> user_Encode for user_Pair<A,B>
//            impl<A: user_Decode, B: user_Decode> user_Decode for user_Pair<A,B>
val p = Pair { left: 3, right: "hi" }
print(json.to_string(p))   // {"left":3,"right":"hi"} — A=Int, B=String both codable
```

- **Option B — user writes the bound.** The author must annotate `#[Codable] struct Box<T: Codable>` or it stays gated. Explicit, but it's redundant ceremony (the bound is always exactly `Codable`) and it's the documented Swift papercut.
```jet
#[Codable]
struct Box<T: Codable> { value: T }   // must spell `: Codable` or E2413 persists
```

- **Option C — no propagation, check at instantiation.** Emit the impl with no bound; let monomorphization fail when a concrete `Box<Socket>` is built. Pushes a type error from the definition to a deep rustc-shaped failure — violates I2 (rustc would be the one rejecting it) unless sema re-derives the check anyway, in which case it's strictly worse than A.
```jet
#[Codable]
struct Box<T> { value: T }
// impl<T> user_Encode for user_Box<T>  ← rustc rejects when T isn't Encode (I2 violation)
```

**Recommendation:** **A** — it's the only option that keeps "safe by default + zero ceremony," matches the existing `Clone`/`JetShow` extra-bounds precedent exactly, and keeps rustc silent (I2): sema's `is_encodable_ty` already proves the use site, the impl bound just lets rustc verify monomorphization.

**Owner Q D-SERDE9:** Confirm auto-bound is *always implicit* (never spelled by the user) for the common case — matching the Clone/JetShow precedent rather than Swift's explicit where-clause?

---

### D-SERDE10 — phantom / non-serialized type params (rec A)

**Gist:** A type param can appear *only* in a `#[Skip]` field, or in no encoded field at all (a phantom marker param). Such a param needs **no** `Encode`/`Decode` bound — bounding it would wrongly reject perfectly serializable instances. Decide whether Jet bounds only params that actually reach the wire (A) or bounds all params bluntly (B).

**Story.** Walter writes a typed-ID wrapper `Id<Kind> { raw: Int }` where `Kind` is a phantom tag (`Id<User>`, `Id<Order>`) that never appears in a field — it exists only to stop you passing a user-id where an order-id goes. He puts `#[Codable]` on it so ids serialize as plain ints. If the derive blindly added `Kind: Encode`, then `Id<User>` would demand `User: Encode` for no reason — `User` may not even be codable. He wants `Id<Kind>` to serialize regardless of `Kind`.

**In the wild:**
```jet
#[Codable]
struct Audited<T> {
    value: T
    #[Skip] trace: Span      // Span isn't codable, and is never written
}
// value uses T (wire) → bound T; trace is skipped → no bound from it.
// generates: impl<T: user_Encode + Clone> user_Encode for user_Audited<T>
```
```jet
#[Codable]
struct Id<Kind> { raw: Int }     // Kind appears in NO encoded field → phantom
// generates: impl<Kind: Clone> user_Encode for user_Id<Kind>   (no Kind: Encode)
val uid: Id<User> = Id { raw: 7 }
print(json.to_string(uid))       // 7  — works even though User isn't Codable
```

**Other languages:**
- **Rust serde** — bounds *only* the params that appear in serialized fields; a phantom `PhantomData<K>` field or a `#[serde(skip)]` field does not pull a bound on `K`. This is serde's bound *inference* (it walks the fields), and the `#[serde(bound)]` override exists precisely because the inference is occasionally too liberal/conservative.
- **Swift** — the explicit `where T: Codable` you write applies to whatever you spell; you'd simply omit the constraint for a phantom param. Manual but precise.
- **Kotlin** — phantom params are rare; the generated serializer takes a serializer per declared param regardless, so phantom params are awkward (a known rough edge).

**Tradeoffs:** *(subagent-reviewed)*

| | Bound only wire-reaching params (A) | Bound all params (B) |
|---|---|---|
| Correctness | precise — phantom/skip work | rejects valid phantom/skip types |
| Beginner experience | "it just serializes" | mystifying "Kind isn't Encode" on a phantom |
| One-path | one rule: bound iff on the wire | one rule, but wrong |
| Matches serde | yes (field-walk inference) | no |
| I2 (rustc clean) | yes | yes (but over-constrains) |

- **Option A — bound only params that reach the wire.** *(recommended)* The derive walks the encoded fields (skipping `#[Skip]`, and for Decode skipping fields satisfied by `#[Default]`/`T?` as already handled), collects which type params actually appear, and bounds exactly those. A param used only in a skipped field or nowhere gets no `Encode`/`Decode` bound (it still gets the structural `Clone`). This is serde's inference, reusing the existing `is_encodable_ty` field walk to drive bound collection.
```jet
#[Codable]
struct Cache<K, V> {
    hits: [V]
    #[Skip] index: Map<K, Int>    // K only in a skipped field
}
// generates: impl<K: Clone, V: user_Encode + Clone> user_Encode for user_Cache<K,V>
//            (V bounded — on the wire; K unbounded — skipped)
```

- **Option B — bound every declared param.** Always add `T: Encode`/`T: Decode` for every param, ignore whether it reaches the wire. Simpler rule to state, but it breaks the phantom-tag and skip-only-param patterns above — a `Id<User>` would demand `User: Encode`. Over-constrains and contradicts the serde prior art.
```jet
#[Codable]
struct Id<Kind> { raw: Int }
// generates: impl<Kind: user_Encode + Clone> user_Encode for user_Id<Kind>
val uid: Id<Socket> = Id { raw: 7 }
print(json.to_string(uid))   // ERROR: Socket isn't Encode — even though it's never written
```

**Recommendation:** **A** — bound exactly the params that hit the wire. It's the correct rule (B silently rejects valid programs), it's what serde does, and it falls straight out of the field walk sema already performs in `is_encodable_ty`/`is_decodable_ty`. For Decode, a field covered by `#[Default]` or typed `T?`-with-omit still needs its param decodable when present, so it counts as wire-reaching (no special case).

**Owner Q D-SERDE10:** Agreed that a phantom or skip-only type param carries no `Codable` bound (so `Id<Kind>` serializes regardless of `Kind`), matching serde's field-walk inference?

---

### D-SERDE11 — manual bound override (rec A: none for now, reserve the attribute)

**Gist:** Rust serde's `#[serde(bound = "…")]` lets you override the inferred bounds for the rare case the field-walk inference is wrong (e.g. a field of type `Map<K, V>` where you want `K: Decode` from a custom key path). Decide whether Jet ships a manual-override attribute now, never, or reserves the spelling.

**Story.** Dorothy has a generic wrapper where a type param is used behind an associated-type-like indirection that the field walk can't see through. In Rust she'd reach for `#[serde(bound(deserialize = "T::Output: Deserialize"))]`. In Jet today there are no associated types and no such indirection — the field walk sees every param's real position — so she has no case that the auto-inference (D-SERDE9/10) gets wrong. She wants the door left open, not a feature she'll never use.

**In the wild:** *(reserved — no current Jet program needs it; the inference is exact for Jet's type system)*
```jet
// FUTURE / reserved spelling — not shipped:
#[Codable]
#[Bound(decode: "T: Decode + Clone")]   // override auto-inference (reserved, not built)
struct Weird<T> { … }
```

**Other languages:**
- **Rust serde** — `#[serde(bound = "...")]`, with directional `#[serde(bound(serialize=..., deserialize=...))]`. A stringly-typed escape hatch (the bound is a string parsed by the macro) needed because Rust has associated types, lifetimes, and `where` clauses the field walk can't always reproduce. Widely regarded as advanced/sharp-edged.
- **Swift** — the `where` clause on the conformance *is* the manual bound; there's no separate override because there's no inference to override.
- **Kotlin** — `@Serializable(with = …)` overrides the whole serializer, not the bounds; no bound-level override.

**Tradeoffs:** *(subagent-reviewed)*

| | None now, reserve spelling (A) | Ship `#[Bound(…)]` now (B) | Forbid forever (C) |
|---|---|---|---|
| Beginner experience | clean — no escape hatch to misread | extra advanced surface | clean |
| Correctness need today | none (inference exact) | none | none |
| One-path | yes (the inference is the one path) | adds a second path | yes |
| I8 ratchet | respected (reject+reserve) | adds a feature w/o a use | respected |
| Future-proof | yes (spelling held) | premature | risks repainting later |

- **Option A — no override now; reserve `#[Bound(…)]`.** *(recommended)* Auto-inference (D-SERDE9/10) is the single path. Don't ship an override attribute — Jet has no associated types/lifetimes, so the field walk is exact and nothing today needs an escape hatch (I8). Reserve the `#[Bound(serialize:…, decode:…)]` spelling in `Source/Syntax.rs` so a future associated-type story has a name ready, but it parses to an "not yet supported" diagnostic.
```jet
#[Codable]
struct Page<T> { items: [T] }   // inference is exact; no override exists or is needed
```

- **Option B — ship `#[Bound(…)]` now.** Add the override attribute immediately for symmetry with serde. But there is no Jet program it would fix today (no associated types to confuse the walk), so it's a feature with zero use cases — fails the I8 simplicity ratchet, and a stringly bound argument fights Jet's typed-over-stringly stance (D-SERDE5).
```jet
#[Codable]
#[Bound(decode: "T: Decode")]    // shipped but redundant with auto-inference
struct Page<T> { items: [T] }
```

- **Option C — forbid the concept forever.** Never add an override and don't reserve the spelling. Risks needing to repaint syntax if associated types/HKT ever land and the walk becomes incomplete. Reserving costs nothing and keeps the door open.
```jet
// no override mechanism, no reserved name — would need a fresh syntax decision later
```

**Recommendation:** **A** — ship only the auto-inference, reserve `#[Bound(…)]` against a future where Jet's type system grows indirection the field walk can't see. No current program needs an override, so adding one now violates the ratchet; refusing to reserve the name risks a later forced repaint.

**Owner Q D-SERDE11:** OK to ship auto-inference as the only path and *reserve* (not build) a `#[Bound(…)]` override for a future associated-type story — or do you want the override built now?

---

### D-SERDE12 — lift the E2413 gate (rec A)

**Gist:** Once generic derive lands (D-SERDE9/10), `#[Codable]` on a generic type is no longer an error. Decide whether E2413 is retired entirely, or kept for a residual unsupported corner.

**Story.** Same Margaret from D-SERDE9: after the feature ships, her `Response<T>` compiles. She should never see E2413 again for an ordinary generic struct/enum. The only question is whether any corner remains unsupported and still deserves a tailored message.

**In the wild:**
```jet
#[Codable] struct Box<T> { value: T }       // was E2413, now compiles
#[Codable] enum Tree<T> { Leaf(T), Node([Tree<T>]) }   // recursive generic enum — also compiles
```

**Other languages:** Rust serde has *no* "generic types unsupported" error — generic derive is fully general. Swift/Kotlin likewise have no such gate. There is no prior art for keeping a generic-serde block once the feature exists.

**Tradeoffs:** *(subagent-reviewed)*

| | Retire E2413 fully (A) | Keep for residual corner (B) |
|---|---|---|
| Beginner experience | best — generics just work | a lingering "yet" message |
| One-path | yes | implies an unsupported nook |
| Correctness | sound (D-SERDE9/10 cover all params) | only if a real gap exists |
| Honest diagnostics (I4) | yes | only if the corner is real + tested |

- **Option A — retire E2413 entirely.** *(recommended)* D-SERDE9/10 cover every type param uniformly (wire-reaching → bounded, phantom/skip → unbounded), so there is no generic shape left to reject. Delete `e2413`, the `type_params > 0` early-out at `CheckerCoreLib.rs:2247`, and the codegen bails at `Items.rs:299`/`:411`; the per-field checks (E2407–E2412) then run on generic types unchanged. Update the diagnostics doc + drop the ui snapshot.
```jet
#[Codable] struct Pair<A, B> { left: A, right: B }   // compiles; no E2413 anywhere
```

- **Option B — keep E2413 for a residual corner.** Retain the gate for some unsupported sub-case. But there is no such case under D-SERDE9/10 — keeping a "…yet" error for a corner that doesn't exist is a phantom diagnostic (I4: a diagnostic must describe a real rejection with a test). If a genuine gap surfaces later it earns its *own* coded diagnostic, not a vague "generic … yet."
```jet
// no demonstrable program that should still hit E2413 once D-SERDE9/10 ship
```

**Recommendation:** **A** — retire E2413 wholesale. D-SERDE9/10 make the derive total over generic shapes, so the gate has nothing left to guard; any future gap gets its own specific code, not a lingering catch-all.

**Owner Q D-SERDE12:** Confirm E2413 is deleted (not downgraded/kept) once generic derive ships, so generic `#[Codable]` is fully first-class with no "yet" wall?

---

**Cross-cutting note (no decision):** Implementation reuses existing precedent — `rust_extra_clone_bounds`/`rust_extra_jetshow_bounds` (`Source/Generics.rs:447`/`:455`) already prove the per-param extra-bound pattern; the generic `JetShow` impl at `Source/Codegen/Items.rs:114-124` is the exact template for a derived-trait impl over a generic type. New `rust_extra_encode_bounds`/`rust_extra_decode_bounds` feed `rust_type_param_list` in `emit_struct_serde`/`emit_enum_serde`. rustc verifies every monomorphization, so I2 holds.

---

## Package build-from-source + M9 wave-2 — board card c50

_Three cards open under I6 + D-DEP1: each new Rust crate behind a wrapping Jet
package needs its own owner sign-off (like `regex`/D-REGEX1). **D-DEP-ARCHIVE1**
(zip/tar crate for `jet.archive`), **D-DEP-DB1** (sqlite crate for `jet.db`), and
**D-BFS1** (where a wrapped crate's source lives for an offline build). All to the
house format and subagent-reviewed. Build-from-source mechanics themselves are
already ratified (D-BUILD1 C-FFI bridge + `Provider.rs` realize step); these cards
only decide the dependency surface c50 ships on top._

> Cross-checked against `syntax-decisions.md`: deps ship as **FFI-wrapping Jet
> packages** (D-DEP1), manifest is **`pkg.jet`** (`PAYLOAD_FILE` in
> `Source/Syntax.rs`), the version pin is **inline in `extern rust "crate@version"`**
> (S50, authoritative), and consumers depend on the *package* not the crate. Each
> approval is an I6 **bootstrap** sanction carrying D-REGEX1's standing native-ize
> obligation (replace the crate before the dependency-free end state). Precedents:
> `jet.tls`/`rustls` (D-NET1), `jet.regex`/`regex` (D-REGEX1), Cranelift runtime
> dep (D-JITDEP1).

---

### D-DEP-ARCHIVE1 — which crate(s) `jet.archive` wraps

**Gist:** Pick the zip + tar crate the `jet.archive` package wraps to read/write `.zip` and `.tar.gz`.

**Story.** Walter ships a CLI that bundles a project's logs into a single
`backup.zip` and also unpacks vendor `.tar.gz` releases. He wants `use jet.archive`
to just work — pure-Jet API, no crate names in his code — and he trusts that
whatever the package wraps was vetted once by the owner, not chosen per-install.

**In the wild:**
```jet
// his code — no crate ever appears
use jet.archive as ar

fn backup(logs: [Path]) -> Result<Unit> {
  val z = ar.zip_writer("backup.zip")?
  for f in logs { z.add(f.name, fs.read(f)?)? }
  z.finish()
}
```
```jet
// jet.archive/pkg.jet  — the wrapping package the owner is approving
payload: { name: "jet.archive", version: "0.1.0", license: "MIT OR Apache-2.0" }
packages: { jet.archive: library }
// no deps: block — the crate pin lives inline in the extern rust block (S50)
```
```jet
// jet.archive body — the extern rust block carrying the approved crate pins.
// Pins are EXACT (S50) so they match the vendored source tree (see D-BFS1).
extern rust "zip@2.1.3" {
  fn zip_open(bytes: Bytes) -> ZipHandle           = "zip::ZipArchive::new";
  fn zip_entry(h: ZipHandle, i: Int) -> Bytes      = "::jet_archive::zip_entry";
}
extern rust "tar@0.4.40" {
  fn tar_entries(gz: Bytes) -> [TarEntry]          = "::jet_archive::tar_list";
}
```

**Other languages:** Rust uses the `zip` crate (de-facto) + `tar` + `flate2` for
gzip. Go has `archive/zip` and `archive/tar` **in its standard library** (no third
party). Node leans on `adm-zip` (pure-JS) / `tar` npm modules. Python ships
`zipfile` + `tarfile` in stdlib. Jet is in Go/Python's eventual position (stdlib
archive) but bootstraps via Rust crates first, then native-izes (I6).

**Tradeoffs:** (subagent-reviewed)

| Option | Crates | Pure-Rust (no C) | Coverage | Supply-chain surface | Native-ize path |
|---|---|---|---|---|---|
| A `zip` + `tar` + `flate2` (recommended) | 3 well-known, pure-Rust | yes | zip, tar, tar.gz | 3 crates, all widely-audited, no C | clean — port deflate + zip + tar reader to Jet |
| B `zip` only (gzip/tar later) | 1 | yes | zip only at first | smallest now, but reopens the dep question for tar | partial — still owes tar |
| C bundled C (`libzip` via `jet bind`) | 0 Rust, 1 C lib | n/a (C) | zip, tar w/ libarchive | a C build dep + cflags-hash cache (D-BUILD1 Phase-3) | different axis — C lib not Rust crate |

- **Option A — `zip` + `tar` + `flate2`, all pure-Rust. (recommended)**
```jet
// covers the whole "Walter" story in one approval; flate2 is the gzip layer
extern rust "zip@2.1.3"   { fn zip_open(b: Bytes) -> ZipHandle = "zip::ZipArchive::new"; }
extern rust "tar@0.4.40"  { fn tar_list(b: Bytes) -> [TarEntry] = "::jet_archive::tar_list"; }
extern rust "flate2@1.0"  { fn gunzip(b: Bytes) -> Bytes        = "::jet_archive::gunzip"; }
```
All three are pure-Rust (no C toolchain on the user's machine), MIT/Apache,
heavily used. One owner approval covers the common archive needs end to end.

- **Option B — `zip` only now, defer tar/gzip.**
```jet
extern rust "zip@2.1.3" { fn zip_open(b: Bytes) -> ZipHandle = "zip::ZipArchive::new"; }
// .tar.gz unsupported → Walter's vendor-unpack path errors until a second ballot
```
Smaller surface today, but `.tar.gz` is half the real-world need; it reopens the
dependency question almost immediately and ships an incomplete `jet.archive`.

- **Option C — bundle C `libarchive`/`libzip` via `jet bind`.**
```jet
// jet.archive/pkg.jet
deps: { archive: c@"libarchive" }   // C build dep, not a Rust crate
```
Zero Rust crates, but forces a C toolchain + `pkg-config` on every consumer and
rides the not-yet-landed C-FFI Phase-3 cflags-hash cache — heavier and breaks the
"just works, no system deps" beginner path.

**Recommendation:** A — one approval covers zip + tar + tar.gz with three
pure-Rust, widely-audited crates and no C toolchain requirement; the native-ize
path (port deflate + zip + tar to Jet) is the cleanest of the three.

**Owner Q (D-DEP-ARCHIVE1):** Approve all three pure-Rust crates (`zip@2`,
`tar@0.4`, `flate2@1`) in one go, or only `zip` now and ballot tar/gzip
separately? Approving all three lets `jet.archive` ship complete; the cost is
three crates entering the bootstrap-dep set at once (all carry the I6 native-ize
obligation).

---

### D-DEP-DB1 — which sqlite crate `jet.db` wraps

**Gist:** Pick the SQLite crate behind `jet.db` — the ergonomic `rusqlite` or the thin `sqlite` binding — and whether SQLite's C is bundled or system-linked.

**Story.** Ruth builds a small expense tracker. She wants `use jet.db`, open a
file, run parameterized queries, and have it work on a fresh laptop with **no
system SQLite install**. She never wants to see a crate name, a `Connection`
type from Rust, or a "could not find libsqlite3" linker error.

**In the wild:**
```jet
use jet.db as db

fn record(amount: Money, memo: Text) -> Result<Unit> {
  val conn = db.open("expenses.db")?
  conn.exec("create table if not exists tx(amount int, memo text)")?
  conn.run("insert into tx(amount, memo) values (?, ?)", [amount, memo])?  // bound params
}
```
```jet
// jet.db/pkg.jet — the wrapping package the owner is approving
payload: { name: "jet.db", version: "0.1.0", license: "MIT OR Apache-2.0" }
packages: { jet.db: library }
```
```jet
// jet.db body — extern rust over the approved crate; bundled C so users need no system lib
extern rust "rusqlite@0.31" {        // feature "bundled" compiles SQLite's own C in
  fn db_open(path: Text) -> Conn                       = "::jet_db::open";
  fn db_run(c: Conn, sql: Text, args: [Value]) -> Int  = "::jet_db::run";
}
```

**Other languages:** Rust's two options are exactly this card — `rusqlite` (high
level, owns the `Connection`/`Statement` ergonomics, optional `bundled` feature
that compiles SQLite's amalgamation) vs `sqlite` (thinner). Go uses
`mattn/go-sqlite3` (cgo, bundles the C) or the pure-Go `modernc.org/sqlite`.
Node: `better-sqlite3` (native, bundles C). Python ships `sqlite3` **in stdlib**
(bundled C). The cross-language norm is to **bundle SQLite's C amalgamation** so
the user needs no system library — Jet should match that.

**Tradeoffs:** (subagent-reviewed)

| Option | Crate | SQLite C source | API ergonomics | User system deps | FFI boundary fit (S50 by-value) |
|---|---|---|---|---|---|
| A `rusqlite` + bundled (recommended) | `rusqlite@0.31` | bundled (compiled from amalgamation) | high — params, rows, txns | none | needs a thin `jet_db` shim to flatten Connection/rows to by-value |
| B `rusqlite` + system libsqlite3 | `rusqlite@0.31` | system `-l sqlite3` | high | requires libsqlite3 installed | same shim; plus a linker-error footgun |
| C `sqlite` thin binding | `sqlite@0.36` | bundled | low — closer to C API | none | thinner but more shim work to reach Ruth's clean API |

- **Option A — `rusqlite` with the `bundled` feature. (recommended)**
```jet
extern rust "rusqlite@0.31" {   // Cargo feature "bundled" → SQLite amalgamation compiled in
  fn db_open(path: Text) -> Conn = "::jet_db::open";
}
// Ruth's fresh laptop: no system SQLite, it just builds and runs.
```
Most-used Rust SQLite crate, mature, and `bundled` removes the system-library
footgun entirely (matches Python/Node/Go-cgo norms). The clean Jet API is short
to build over its ergonomic surface.

- **Option B — `rusqlite` linking the system `libsqlite3`.**
```jet
extern rust "rusqlite@0.31" { fn db_open(path: Text) -> Conn = "::jet_db::open"; }
// on a machine without libsqlite3-dev:
//   error: linking with `cc` failed: cannot find -lsqlite3
```
Same ergonomic crate but reintroduces exactly the "could not find libsqlite3"
error Ruth must never see — fails the beginner-safety bar.

- **Option C — the thin `sqlite` crate (bundled).**
```jet
extern rust "sqlite@0.36" { fn db_open(path: Text) -> Conn = "::jet_db::open"; }
// closer to the raw C API → more shim code to reach the bound-params/rows API above
```
Smaller crate, but its low-level surface pushes more of the ergonomic API into the
wrapping shim with no safety or supply-chain win over A.

**Recommendation:** A — `rusqlite@0.31` with the `bundled` feature: the standard
Rust choice, no system dependency (the cross-language norm), and its high-level
surface makes the clean Jet API short. Note for native-ize: SQLite's
public-domain C amalgamation is arguably already the "native" artifact, so the I6
obligation here may resolve to "keep bundled SQLite" rather than rewrite it —
flag for a later frozen card.

**Owner Q (D-DEP-DB1):** Approve `rusqlite@0.31` **with bundled SQLite C** (no
system dep, recommended)? And for the I6 native-ize obligation, is keeping
bundled public-domain SQLite C an acceptable end state, or must `jet.db`
eventually move to a native-Jet embedded store?

---

### D-BFS1 — where a wrapped crate's source lives for an offline build

**Gist:** When build-from-source compiles a wrapping package's `extern rust` crate, do the crate sources ship vendored in the package, or get fetched once and locked?

**Story.** Dale runs a locked CI build of an app that depends on `jet.archive`.
The build must be **offline and byte-reproducible** — no surprise network call to
crates.io mid-build, and the same crate source every time. He needs to know where
the `zip@2` source actually comes from when `jetpack build` compiles the wrapping
package.

**In the wild:**
```shell
# Dale's CI, network disabled
$ jetpack build --locked
  compiling jet.archive (zip@2, tar@0.4, flate2@1) from source …
  # where does zip@2's source come from with no network?
```
```jet
// jet.archive/pkg.jet under option A (vendored): crate source committed in-package
// jet.archive/
//   pkg.jet
//   vendor/zip-2.1.3/…   ← crate source travels WITH the package
extern rust "zip@2.1.3" { … }   // exact pin matches the vendored tree
```

**Other languages:** Cargo fetches from crates.io into `~/.cargo`, locks exact
versions in `Cargo.lock`, and `cargo vendor` copies sources into a `vendor/` dir
for offline/audited builds. Go's module proxy + `GOFLAGS=-mod=vendor` is the same
pattern. npm has `npm ci` against a committed `package-lock.json`. The split is
always "fetch-then-lock (default)" vs "vendor-in-tree (offline/audit)".

**Tradeoffs:** (subagent-reviewed)

| Option | Crate source location | Offline by default | Reproducible | Package size | Audit transparency | One-path fit |
|---|---|---|---|---|---|---|
| A vendored-in-package (recommended) | committed in the wrapping package's `vendor/` | yes — no network ever | yes — tree is the source | larger packages | high — source travels with the dep, hash-pinned in `.jet/lock` | strong — matches Jet's offline/deterministic stance (D-BUILD1) |
| B fetch-then-lock | crates.io → hangar store, pinned in `.jet/lock` | no — first build needs network | yes after lock | small | medium — must trust crates.io + checksum | familiar (cargo), but a network step in the magic path |
| C hybrid: fetch on publish, vendor in published artifact | author fetches; registry artifact carries vendored source | yes for consumers | yes | larger published artifacts | high for consumers | strong but adds a publish-time step |

- **Option A — vendored in the wrapping package. (recommended)**
```shell
$ jetpack build --locked      # network OFF
  compiling jet.archive from vendored zip@2.1.3 …   ✓ no network
```
The crate source is committed inside `jet.archive` and hash-pinned in `.jet/lock`.
Builds are offline from the first run, byte-reproducible, and the exact wrapped
source is auditable in the dependency tree (supply-chain transparency). Matches
the existing offline/deterministic build stance (D-BUILD1 runs offline by
default; `jetpack` never realizes on demand).

- **Option B — fetch from crates.io, lock the pin.**
```shell
$ jetpack build               # first build, no lock yet
  fetching zip@2.1.3 from crates.io …   # ← network in the magic path
  locked → .jet/lock
$ jetpack build --locked      # network OFF after lock: ok
```
Smaller packages and the familiar cargo flow, but the **first** build of any new
dependency needs network, and supply-chain trust rests on crates.io + the
checksum rather than source that travels with the package.

- **Option C — author fetches, published artifact carries vendored source.**
```shell
# author side
$ jet publish jet.archive     # fetches zip@2.1.3, vendors it into the artifact
# consumer side (Dale)
$ jetpack build --locked      # vendored source already inside the fetched package → offline
```
Consumers get A's offline/auditable property without bloating the source repo, at
the cost of a publish-time vendoring step and larger published artifacts.

**Recommendation:** A — vendored-in-package: offline and reproducible from the
first build with no network in the magic path, the wrapped crate source is
auditable in the dep tree (the strongest supply-chain posture), and it matches
Jet's already-offline build model. C is the fallback if committing crate sources
into source repos proves unwieldy at scale.

**Owner Q (D-BFS1):** Vendor crate source inside the wrapping package (A,
offline-first + max audit transparency), or fetch-then-lock from crates.io (B,
smaller repos, cargo-familiar, but network on first build)? This sets the default
supply-chain posture for every D-DEP1 package, not just c50's.

---

## Single-use deliberate-drop hatch — board card c69 (D-LIN1 follow-on)

### D-LIN1-DROP — how to deliberately discard a `#SingleUse` value (rec A)

**Gist:** A `#SingleUse` value must be consumed exactly once; decide how a user *intentionally* throws one away (the rare, audited escape) now that `#Audit` is retired.

**Story.** Walter holds a `#SingleUse` `Lock` but hits an error path where he must abandon it without the normal `unlock()` consume — a deliberate leak he wants on the record. The ratified D-LIN1 text said this needs an `#Audit("…")` note, but D-UNSAFE2 retired `#Audit` (its reason folded into `#Unsafe("reason")`), so the blessed spelling is now unspecified. Today his only options are the two real consumes (move to a `^` param or `return`); there is no sanctioned discard.

**In the wild:**
```jet
#SingleUse struct Lock { id: Int }

fn risky(l: ^Lock) -> () ? Error {
    if cannot_proceed() {
        // Walter must abandon `l` here on purpose. What does he write?
        ???
        return err(Error("aborted"))
    }
    unlock(l)        // normal consume
}
```

**Other languages:**
```text
Rust    — std::mem::forget(x) / ManuallyDrop — explicit, unsafe-adjacent leak
Swift   — no linear types; n/a
Vale/Austral (linear langs) — an explicit `destroy`/`drop` is the only way to end a linear value
```

**Tradeoffs:** (subagent-reviewed)

| Option | Spelling | Audit trail | Reuses existing surface |
|--------|----------|-------------|--------------------------|
| A — `drop(x)` inside `#Unsafe("reason")` (rec) | `#Unsafe("why") { drop(l) }` | the `#Unsafe` reason IS the audit | yes — D-UNSAFE2 already made `#Unsafe("…")` the audited-gate |
| B — dedicated `discard(x, "reason")` verb | `discard(l, "aborted")` | reason is an inline arg | no — new builtin + its own audit channel |
| C — method `l.abandon("reason")` | `l.abandon("aborted")` | inline arg | no — new method on every `#SingleUse` type |

- **Option A — `drop(x)` gated by `#Unsafe("reason")` (recommended).** The deliberate discard is `drop(value)`, legal only inside an `#Unsafe("reason")` region/fn — the `#Unsafe` reason is exactly the audit note the original D-LIN1 wanted, with zero new audit surface (D-UNSAFE2 already unified gate+reason).
  ```jet
  fn risky(l: ^Lock) -> () ? Error {
      if cannot_proceed() {
          #Unsafe("lock deliberately abandoned on the abort path") {
              drop(l)
          }
          return err(Error("aborted"))
      }
      unlock(l)
  }
  ```
- **Option B — dedicated `discard(x, "reason")` builtin.** A first-class verb carrying its own reason string. Reads clearly, but mints a new builtin + a parallel audit channel beside `#Unsafe`, fragmenting "the audited-escape story."
  ```jet
  discard(l, "lock deliberately abandoned on the abort path")
  ```
- **Option C — `x.abandon("reason")` method.** A method auto-provided on every `#SingleUse` type. Object-syntax is familiar, but it's a magic method on a marker tag and again a second audit channel.
  ```jet
  l.abandon("lock deliberately abandoned on the abort path")
  ```

**Recommendation:** A — the deliberate drop is genuinely an unsafe, audited act, and D-UNSAFE2 already made `#Unsafe("reason")` the one audited gate; `drop(x)` inside it reuses that surface and keeps a single escape-and-audit story (no new builtin, no second audit channel). It also matches Rust's "leaking a linear-ish value is an explicit, eyebrow-raising act."

**Owner Q:** Confirm the discard verb name `drop` (vs `forget`/`leak`/`discard`) — `drop` reads as "let it go," but if you'd rather reserve `drop` for a future general destructor, `forget` (Rust precedent) or `leak` are alternatives.

---

### D-TXN-ROLLBACK — how a value opts into `#Transact` rollback (rec A)

**Gist:** When a `#Transact` block fails on `?`, how does a mutated value get its `rollback` run — does the block track every mutation automatically, or does the author register each undo explicitly?

**Story.** Walter is moving money between two in-memory accounts inside a `#Transact(tx) { … }`. He debits `from`, then the credit to `to` hits a `?`-failure (the account is frozen). D-TXN1 promises the debit is undone "in reverse order on the values mutated so far." Walter needs to know *what he has to write* for that promise to hold: nothing (the block watches `from`)? a trait on `Account`? an explicit `tx.on_rollback(() => { from.credit(amt) })` next to the debit?

**In the wild:**
```jet
struct Account { balance: Int }

fn transfer(from: ~Account, to: ~Account, amt: Int) -> Bool ? Fail {
    #Transact(tx) {
        from.balance = from.balance - amt          // mutation #1

        // ── the open question: what makes mutation #1 reversible? ──
        // Option A (explicit hook):   tx.on_rollback(() => { from.balance = from.balance + amt })
        // Option B (Rollback trait):  (nothing here; `Account` derives `Rollback`, the block snapshots it)
        // Option C (auto-snapshot):   (nothing here; the block deep-copies every value it sees mutated)

        ok_or_freeze(to, amt)?                       // `?`-failure here → rollback mutation #1
        to.balance = to.balance + amt               // never reached
    }
    return ok(true)
}
```
On the `?`-failure, the debit to `from` must be undone. The three options differ entirely in what Walter writes and what the compiler must prove.

**Other languages:** Software-transactional-memory (Haskell STM, Clojure refs) auto-snapshots every ref touched in a transaction and retries — zero author annotation, but every participant must be a special `TVar`/`ref` cell, not a plain value. Rust has no language transactions; libraries (e.g. `scopeguard`'s `guard(val, |v| …)`) make you write the undo closure explicitly. Database `BEGIN/ROLLBACK` snapshots at the storage engine, invisible to the app. D-TXN3's already-ratified `on_commit` is the *forward* (commit) half and is explicit per-hook — option A is its mirror image (an explicit `on_rollback`), which is the most consistent with what already shipped.

**Tradeoffs:** (subagent-reviewed)

| Option | What the author writes | What the compiler must prove | Works on plain values? | Consistency with shipped `on_commit` |
|---|---|---|---|---|
| **A — explicit `tx.on_rollback(() => {…})` (recommended)** | one undo lambda per reversible mutation, beside the mutation | nothing new — it's the `on_commit` machinery run on the *failure* path instead of commit | yes (any value, captured by the closure) | exact mirror of `on_commit`; same Drop-backed model, same LIFO order |
| **B — `Rollback` trait the type derives** | `#[Rollback]` on the struct; nothing at the mutation site | the block must snapshot each `Rollback` value on entry and prove a non-`Rollback` value isn't mutated (a new E-code + a "mutated value isn't `Rollback`" diagnostic) | only on types that derive it | new concept (a trait + snapshot pass); `on_commit` stays a closure, so two different mental models |
| **C — auto-snapshot every mutated value** | nothing | the block must detect *every* mutation in its dynamic extent and deep-copy the pre-state — expensive and hard to bound for heap/collection types; aliasing makes "restore" ambiguous | yes, but with hidden cost | invisible magic vs. explicit `on_commit` — least consistent |

**Recommendation:** **A** — an explicit `tx.on_rollback(() => { … })` is the precise mirror of the already-shipped, already-ratified `tx.on_commit(() => { … })`: same handle, same Drop-backed mechanism, same LIFO order, just fired on the failure path instead of the commit path. It needs no new trait, no snapshot pass, and no "is this value reversible" proof; it works on plain values; and it keeps one mental model for the whole transaction surface ("register what to do on commit; register what to undo on rollback"). The D-TXN1 ratified phrase "calls `rollback(mut self)` (the `Rollback` trait)" reads toward B, so this fork genuinely needs the owner: keep the `Rollback`-trait spelling (B), or supersede it with the closure-symmetric `on_rollback` (A)?

**Owner Q (D-TXN-ROLLBACK):** D-TXN1 as ratified names a `Rollback` trait and "values mutated so far." After building `on_commit` (D-TXN3/4) as an explicit Drop-backed closure, the symmetric move is an explicit `tx.on_rollback(() => {…})` (A) rather than a trait + auto-snapshot (B). Do you want to (A) supersede the `Rollback`-trait wording with the closure-symmetric `on_rollback`, or (B) keep the trait and have the block auto-snapshot `Rollback`-deriving values? (The `#Transact` block, `on_commit`, and the D-TXN2 irreversible-effect rejection are already built and shipped; only this registration mechanism is held.)

---

### D-TAINT-SAN — sanitizer-function spelling: bare `sanitizer fn` vs `#Sanitizer fn` (rec B)

**Gist:** Pick the spelling of the taint-strip function modifier — a bare keyword `sanitizer fn` (as the D-TAINT1 card literally wrote it) or the PascalCase marker `#Sanitizer fn` (as D-CASING1 made `pure fn` → `#Pure fn`).

**Story.** Dolores is writing a web handler in Jet. She has a `#Sanitizer fn` that scrubs a raw query param before it reaches a SQL query (a `Db` sink). She just learned `#Pure`, `#Unsafe`, and `#Test` are all `#`-markers, and she's about to type the taint-strip one. Whatever she types, she wants it to look like the family she already knows — and the compiler's teaching error to point her at the one true spelling, not leave her guessing.

**Background.** The D-TAINT1 card (ratified 2026-06-21, same day as D-CASING1) writes the modifier bare: "A function declared **`sanitizer fn`** is the one blessed way to strip it." D-CASING1 rule 1 says *all tags are PascalCase `#`-markers*, and the D-CASING1 **follow-on** explicitly retired the bare `pure`/`test`/`todo` keywords to `#Pure`/`#Test`/`#Todo`. `sanitizer` is a fn-contract modifier of exactly the same shape as `pure` — so the two ratified-same-day cards point in opposite directions on this one word. This is a pure **spelling** decision; semantics (taint cleared by contract, return is untrusted-input-made-trusted) are unchanged either way. The implementation shipped the marker form `#Sanitizer fn` as the default so the feature is whole; this card confirms or flips that one token.

**In the wild:**
```jet
use jet.db as db

// The audited cleaning step: untrusted text in, trusted value out.
#Sanitizer fn safe_name(raw: String) -> String {
    return raw.replace("'", "")   // (illustrative; a real one rejects, not strips)
}

fn lookup(input: String) {        // `input` arrives #Tainted from the request
    name := safe_name(input)      // taint cleared — the result is trusted
    db.query("select * from users where name = '{name}'") ?? return  // Db sink: OK
}
```

**Other languages:** no mainstream language has a first-class sanitizer modifier (taint is usually a linter/annotation, e.g. CodeQL `sanitizer`, or PHP/Perl runtime taint with no keyword). So there's no cross-language convention to honor — the only consistency pull is *Jet's own* `#Pure`/`#Unsafe`/`#Test` marker family.

**Tradeoffs:**

| Option | Consistency with marker family | Matches ratified D-TAINT1 text | New keyword vs marker | Teaching-error story |
|--------|-------------------------------|-------------------------------|-----------------------|----------------------|
| A — bare `sanitizer fn` | Breaks it (lone bare modifier after D-CASING1 retired the others) | Yes (literal) | A bare contextual keyword | Must add a *new* "bare → `#`" error to mirror E0053-style teaching, OR accept an outlier |
| B — `#Sanitizer fn` (recommended) | Matches `#Pure`/`#Unsafe`/`#Test` exactly | No (normalizes per D-CASING1) | Reuses the marker grammar | Bare `sanitizer fn` → teaching error pointing at `#Sanitizer` (the S14/E0053 pattern) |

- **Option A — bare `sanitizer fn`.** Honor the D-TAINT1 card's literal spelling.
  ```jet
  sanitizer fn safe_name(raw: String) -> String { return raw.trim() }
  ```
  Cost: it's the only bare fn-contract keyword left after D-CASING1 retired `pure`/`test`/`todo`, so Dolores's mental model ("contracts are `#`-markers") breaks on this one word.

- **Option B — `#Sanitizer fn` (recommended).** Spell it as a PascalCase marker, like every other fn-contract modifier.
  ```jet
  #Sanitizer fn safe_name(raw: String) -> String { return raw.trim() }
  ```
  The bare `sanitizer fn` becomes a teaching error pointing at `#Sanitizer` (mirroring `pure`→`#Pure`/E0053). This is what shipped as the default.

**Recommendation:** **B.** D-CASING1's follow-on already settled this category — a fn-contract modifier is a `#`-marker — and shipping `sanitizer` as the lone bare exception would re-introduce exactly the "is this a keyword or a marker?" inconsistency D-CASING1 erased. The D-TAINT1 card predates seeing that follow-on applied; B is the faithful reconciliation.

**Owner Q — teaching error for the retired spelling.** If B: should bare `sanitizer fn` get a dedicated teaching error (a new E-code in the E005x teaching family, like `pure`→E0053), or is that over-investment for a word no shipped code uses yet? (Default if unanswered: add it for symmetry with `#Pure`/`#Test`/`#Todo`.)

---

### D-DET-CAPAPI — the method API for the deterministic `Clock` / `Rng` capabilities (rec A)

**Gist:** D-DET1 ratified "supply deterministic `Clock`/`Rng` as injected capabilities" but did not pin the methods those handles expose. A minimal sensible API shipped so the feature is whole; this fork ratifies (or revises) the exact surface.

**Story.** Priya writes a `#Pure` dice-roller and a `#Pure` retry-with-backoff. Both need time and randomness but must stay reproducible, so she takes a `Clock` and an `Rng` parameter (seeded by the caller). She needs to know the verbs: does she read the clock with `clock.now()`? advance it with `clock.tick(ms)` or `clock.advance(ms)`? draw an int with `rng.int(lo, hi)` (inclusive? half-open?) and a float with `rng.float()` (`[0,1)`?)? The current behavior is reproducible regardless, but the *spelling and ranges* are owner-facing and should be deliberate.

**What shipped (the default to confirm or revise):**
```jet
use core.time as time;
use core.random as random;

#Pure fn roll(clock: Clock, rng: ~Rng) -> String {
    ts  @= clock.now()        // current value in ms; pure read, no `~`
    die @= rng.int(1, 6)      // inclusive [1,6]; advances the stream, needs `~Rng`
    return "t={ts} rolled {die}"
}

fn main() {
    c @= time.clock(1000)     // Clock seeded at 1000 ms
    r := random.rng(42)       // Rng seeded at 42 (SplitMix64, std-only)
    print(roll(c, ~r))
    // clock.tick(ms) -> Int  advances the clock and returns the new value
    // rng.float() -> Float   draws in [0.0, 1.0)
}
```
- Constructors: `time.clock(seed: Int) -> Clock`, `random.rng(seed: Int) -> Rng` (both pure-callable; carry no ambient effect).
- `Clock`: `now() -> Int` (read, no `~`), `tick(ms: Int) -> Int` (advance + read, needs `~`).
- `Rng`: `int(lo: Int, hi: Int) -> Int` (inclusive), `float() -> Float` (`[0,1)`) — both advance the stream, need `~Rng`.

**Open sub-questions:**
1. **Clock advance verb** — `tick(ms)` (shipped) vs `advance(ms)` vs `set(ms)`-absolute vs offer both relative + absolute.
2. **Clock read unit** — ms (shipped) vs ns vs a `Duration`/`Instant` value type (would need that type minted first).
3. **`rng.int` range convention** — inclusive `[lo, hi]` (shipped, matches ambient `random.int`) vs half-open `[lo, hi)`.
4. **Extra `Rng` draws** — ship `bool()`, `pick(list)`, `shuffle(~list)` now (mirroring ambient `random.*`), or keep the minimal `int`/`float` and add on evidence.
5. **Are `Clock`/`Rng` the final type names** — vs `DetClock`/`SeededRng`, etc. (D-CASING1 PascalCase either way).

**Other languages:** Most ecosystems inject determinism as a *value*, not ambient: Go threads `*rand.Rand` (seeded) and `clock.Clock` interfaces (e.g. `benbjohnson/clock`) through call sites; Rust's `rand` passes an `Rng` impl explicitly and test code uses `StdRng::seed_from_u64`; Java's `Random(seed)` / `Clock.fixed(...)` are constructed and passed. All converge on "construct seeded, pass by parameter, read through methods" — exactly D-DET1's model; they differ only on method names and range conventions (the sub-questions above).

**Tradeoffs:**

| Option | Surface | Pro | Con |
|---|---|---|---|
| **A — keep the shipped minimal set (recommended)** | `now`/`tick`, `int`/`float`; inclusive int; ms | smallest learnable surface; matches ambient `random.int` inclusivity; nothing speculative | experts may want `bool`/`pick`/`shuffle` and a `Duration` clock later |
| **B — widen now** | + `rng.bool`/`pick`/`shuffle`, `clock.advance` absolute form, `Duration` reads | parity with ambient `random.*` + richer clock from day one | mints `Duration`/more API ahead of demand (I8 ratchet); larger snapshot surface |
| **C — rename + re-spec ranges** | half-open `rng.int`, ns clock, `SeededRng`/`DetClock` names | aligns with Rust-style half-open ranges | diverges from ambient `random.int` (inclusive) → two conventions for one verb |

**Recommendation:** **A** — keep the minimal `now`/`tick` + `int`/`float`, inclusive int range (consistent with the already-shipped ambient `random.int`), ms clock. It is whole, reproducible, and learnable; widen (`bool`/`pick`/`shuffle`, absolute clock, `Duration`) on real evidence per the simplicity ratchet (I8). The feature is fully built and shipped on this surface; only the names/ranges are held for the owner.

**Owner Q (D-DET-CAPAPI):** Confirm the shipped minimal `Clock`/`Rng` API (A: `clock.now()`/`clock.tick(ms)`, `rng.int(lo,hi)` inclusive / `rng.float()` `[0,1)`, ms, names `Clock`/`Rng`), or revise via the five sub-questions above (verb, unit, range convention, extra draws, type names)? The injection path and `assume_deterministic` escape are built, tested, and shipped — only this method surface is open.

---

---

# Ballot scratch — D-STATE1 typestate surface spelling forks

D-STATE1 (=A) is ratified: *typestate via transitioning tags — a fn takes the old
state tag and returns the next; wrong-state call = compile error E0150; tags erase,
zero runtime cost.* The **mechanism** is pinned. What the one-line ratification does
NOT pin is the exact owner-facing **spelling** of three surface elements. Per the
syntax-decision protocol I built the clearly-implied core (E0150, erasing tags,
forward state dataflow) using the established marker idioms, and queue the spellings
below for owner confirmation. The implemented spellings are the defaults; nothing
about the mechanism changes if the owner picks an alternative — only the lexer/parser
spelling and a re-bless.

The state *value-fact prefix* (`#Pending res`) is NOT in question — it is the
ratified D-QUAL1/D-TAINT1 value-fact-rides-the-value idiom (`#Tainted expr`),
already shipped. Only the two fn-modifier markers and the arrow glyph below are forks.

---

### D-STATE-REQ — the "this method requires state S" marker spelling

*User story:* A dev writes `check_in` on a `Reservation` and wants the compiler to
reject calling it unless the reservation is `#Confirmed`.

The ratified text says a transition fn "takes the old state tag". A *guarded* method
(non-transitioning, e.g. `check_in` which is valid only in `Confirmed`) needs a
require-state marker. Spelling options:

- **A (implemented default): `#State(Confirmed) fn check_in(self, …)`** — a paren-arg
  fn-modifier marker, exactly parallel to `#layout(c)` / `#UnitFamily(currency)` and
  the `#Sanitizer fn` modifier family. Reads "in state Confirmed".
- B: `#Requires(Confirmed) fn check_in(…)` — more explicit verb, longer.
- C: `#In(Confirmed) fn` — terse, but `In` collides conceptually with `for x in`.

Rec: **A**. One marker idiom, shortest that still reads as a noun-state.

### D-STATE-TRANS — the transition-fn marker + arrow glyph

*User story:* `confirm_payment` moves a reservation from `#Pending` to `#Confirmed`.

- **A (implemented default): `#Transition(Pending -> Confirmed) fn confirm(self) -> Reservation`**
  — paren-arg marker; the `->` inside mirrors the fn return arrow the dev already
  knows. Declares "consumes a value at Pending, yields one at Confirmed".
- B: `#Transition(Pending => Confirmed)` — `=>` to distinguish from the return arrow.
- C: two markers `#From(Pending) #To(Confirmed) fn` — no new glyph, but two markers.

Rec: **A**. `->` is already the "produces" arrow; reusing it reads naturally.

### D-STATE-DECL — do states need an explicit grouping declaration?

*User story:* the dev wants `Pending`, `Confirmed`, `CheckedIn` recognised as the
state set of `Reservation`, ideally exhaustively (so a typo `#Confrimed` is caught).

- **A (implemented default): no grouping needed.** Each state is an ordinary `tag`
  (D-QUAL2). The transition/require markers name them; the checker derives the state
  set from the markers used. Zero new declaration construct.
- B: an explicit `states Reservation { Pending, Confirmed, CheckedIn }` block — enables
  exhaustiveness + a "no transition out of CheckedIn" diagnostic, but is a brand-new
  owner-facing construct (and a new keyword), heavier than the ratified one-liner.

Rec: **A** for v1 (matches "tags erase, no new runtime/declaration weight"); B is a
natural follow-on if exhaustive state-machine checking is wanted later (would pair
with the deferred D-ROLE1 time-varying-roles card).

---

**Also still open upstream (named, not blocking the value-prefix core):**
D-QUAL4 — plain value-tag *type-position* spelling (`#Tag Type` vs `Type #Tag`). The
typestate core above never writes a state in a type position (states ride the value and
the markers), so it does not depend on D-QUAL4. If the owner later wants a state written
in a signature type (`fn f(r: Reservation #Pending)`), that rides D-QUAL4.

---

**Still deferred (not blocking; expand to a card when needed):**
- **D-SERDE-ACCESS — dynamic-tree accessor API.** How a user reads an untyped
  `Json`/agnostic `DataTree` by hand: pattern-match (shipped today) vs a fluent accessor
  (`tree.field("x").int()?`, `.text()`, `.bool()`, indexing). Only matters for the
  hand-impl / dynamic path (D-SERDE2), not the typed derive. Recommend: keep
  pattern-match as the floor; add minimal fluent accessors if hand-impl ergonomics demand it.

---

> **Drained 2026-06-24 (batch 5).** Owner decided the last open cards: **D-EFF4 = B**
> (ship the closed ten effects now — Net/Fs/Io/Db/Time/Rand/Env/Exec/Log/Gpu — and reserve a
> future `effect <Name>` user-declaration form), **D-EFF5 = A** (flat effect lattice; `#(Io)`
> = console only, no umbrella; `Io`→`Console` rename left as optional polish), and
> **D-JITDEP1 = approve Cranelift** for JIT tier-1 (runtime-side only, I6 holds; the own
> bytecode-VM and own native-JIT progression are frozen board cards so they're not lost).
> All recorded in `syntax-decisions.md`; the effect-system cluster (c62) is now unblocked.

> **Drained 2026-06-24 (batch 4).** The owner ratified all 11 remaining open full cards:
> **D-SIMD2 = A** (method-reduce SIMD surface; operator overloading on built-in lane types
> only), **D-SERDE2 = A** (Swift-plain hand-impl: `encode`/`decode`, `DataTree`, `DecodeError`),
> **D-SERDE3 = C** (typed `RenameAll` menu camel/snake/pascal/kebab/screaming),
> **D-SERDE4 = B, owner-modified** (umbrella `#[Codable]`; one-way `#[Encode]`/`#[Decode]`),
> **D-SERDE5 = A** (per-field bracket markers `#[Rename]`/`#[Skip]`/`#[Default(expr)?]`/`#[Flatten]`,
> absent-optional omitted, struct-flatten now), **D-SERDE6 = C** (typed `decode<T>` turbofish +
> expected-type; turbofish blessed as general grammar), **D-SERDE7 = A + ship chooser now**
> (externally tagged default; `#[Tag("type")]`/`#[Untagged]` container chooser — distinct from
> D-SERDE5 field attrs), **D-SERDE8 = A** (lenient default + `#[DenyUnknownFields]`),
> **D-NOSTD1 = A** (platform-implied std opt-out), **D-IF3 = A** (`if x == { … }` required
> dispatch marker; E0992/E0993), **D-FMT1 = A** (author-intent single-line bodies). The two
> **clarification corrections** were confirmed: **C-CASING** (plan tags → D-CASING1 PascalCase)
> and **C-MANIFEST** (`pkg.jet` → `pack.jet`). All recorded in `syntax-decisions.md`, cards
> stripped. Serde increment-2 implementation unblocked end-to-end (sidequests/serde-model.md).


> **Drained 2026-06-24 (batch 3).** Two follow-on cards ratified: **D-JSONVERB1 = A**
> (`json.to_string(v)` + `json.to_string_pretty(v)`, 2-space indent — renames/retires
> `json.render`; keeps Jet's one `to_`-prefixed conversion idiom, matching ratified `to_float`
> S42; bare `json.string`/`json.stringify` rejected) and **D-TXN4 = A** (`#Transact(order) { …
> order.on_commit(…) }` — the scope's name *is* the handle, mirroring ratified `region r { …
> r.alloc(…) }`; refines D-TXN3's `scope.on_commit` → `<name>.on_commit`, semantics unchanged;
> the D-TXN2 fix-it string is updated to match). The `.Type()`-conversion idea (`x.Float()`)
> was discussed and **declined** — `x.to_float()` (S42) stays as ratified and shipping; no
> reopen. Recorded in `syntax-decisions.md`, cards stripped.

---

> **Drained 2026-06-24 (batch 2).** The owner ratified six cards from the missing-decision
> audit: **D-DBG3 = A** (`jet debug` interactive surface — `step`/`next`/`continue`/`finish`
> + `s`/`n`/`c`/`f` aliases, `(jet)` prompt, `<- here`/`locals:` layout); **D-LINALG1 = A**
> (`jet.linalg` names `Vec2/3/4`/`Mat3/4`, `.dot`/`.cross`/`.matmul` — A names as aliases over
> a `Vec<N>`/`Matrix<M,N>` generic substrate, per owner); **D-SUPPLY1 = A** (dedicated
> `jet vendor` / `jet audit` verbs + `--vendor-dir`, SBOM as a `--sbom` flag); **D-TXN3 = A**
> (`scope.on_commit(() => {…})` library form, no new keyword — the D-TXN2 fix-it string is
> updated to match; the "name the transact scope" follow-on is now open as **D-TXN4**);
> **D-NUMOPS2 = A** (sized/unsigned integers inherit the D-NUMOPS1 trap-on-overflow default;
> `wrapping(…)` is the opt-in); **D-QUAL3 = C** (a `#UnitFamily` mints one distinct type per
> member — `usd`→`Usd` — so signatures read `price: Usd`; the family tag is PascalCase
> `#UnitFamily`). All recorded in `syntax-decisions.md`, cards stripped, plans unblocked
> (dap-debugger, math-linalg, package-ecosystem-trust, transact-rollback, dsg9, units; c68
> unblocked by D-QUAL3).

---

> **Drained 2026-06-24.** The owner ratified the last two open cards: **D-BENCH1 = A**
> (`#Bench "name" { … }` region-benchmark block, sibling of `#Test`, run by the existing
> `jet bench` verb) and **D-PKGSIGN1 = B + A opt-in** (SHA-256 checksum is the always-on
> integrity floor; Ed25519 author signing is an opt-in, non-blocking layer — `require_signed`
> off by default). Both recorded in `syntax-decisions.md`, cards stripped, plans unblocked
> (epoch-3/testing-docs-ergonomics.md §4; sidequests/package-ecosystem-trust.md §4).

---

> **Memory-model gate CLOSED — ratified 2026-06-23.** The owner decided all three gate
> cards: **D-CAP8 = C** (infer in bodies, freeze at `api: explicit`), **D-CAP9 = D** (`*x`
> = raw-of, dereference becomes postfix `p.*`, `*T` replaces `Ptr<T>`), **D-CAP10 = A**
> (overloads out of scope; call-site-sigil disambiguation on a single definition). Recorded
> in `syntax-decisions.md`; cards stripped. The whole access-capability model
> (`docs/prompt-memory-model-final.md`) is now unblocked — see
> `docs/research/memory-model-implementation-plan.md` for the build order.

---

> **Drained 2026-06-22.** The owner's 2026-06-22 batch ratified every open full card —
> D-UNSAFE2, D-FIXARR1, D-CAP2/3, D-EFF2/3, D-MIGRATE2A/B/C/D/E/F, D-JSONOUT1, D-ARGS1,
> D-MATHLIB1, D-SIMD1, D-REACT1, D-FANOUT2, D-STRPARSE1, D-CTCORE1, D-JIT1, D-HOTSWAP1,
> D-DEVMODE1, D-SOA2A/B/C/D, D-TEST1, D-TEST4, D-BIND2, D-NUMOPS1, D-SERDE1, D-ITER1 (plus
> the earlier batch D-EFF1/D-QUAL1/D-TXN1/D-MIGRATE1/D-SOA1 and D-DBG2). All are recorded
> in `syntax-decisions.md` and their cards stripped from this file. The effect-system
> surface is now fully decided (D-EFF1+D-QUAL1+D-EFF2+D-EFF3). **D-MUTSELF1** (self-mutation
> in `mut self` methods) was opened and ratified 2026-06-23 (option A) — recorded in
> `syntax-decisions.md`, card stripped. The memory-model gate (D-CAP8/9/10) was opened and
> ratified 2026-06-23 — see the note above. **No full decision cards remain open.** What's left
> below is informational only: the **deferred-ballots list**
> (stubs to promote when their prerequisites land), the **B6 `defer`** note, and the
> **Coverage / D-COV1** tooling note. Cards **c25** (range sugar) and **c55** (REPL v2) are
> implement-only. Submitting a decision records it in `syntax-decisions.md` and removes it
> from this file.

---

## Deferred ballots — promote when reached

The items below are not ready for owner decision. Each has a real user story
and a clear reason to wait. Promote a stub to a full card when its
prerequisite is ratified or its milestone is reached.

---

**D-PUBLISH1 — `jet publish` command shape + semver/resolver policy (board card c96).**
*User story:* Saoirse cuts a release of her Jet library and Amara pins a semver range to it.
*Decision (when promoted):* the `jet publish` command surface, version-immutability /
re-publish-refusal policy, and the resolver default (highest-compatible vs exact pins +
explicit update; lockfile default). *Why deferred:* rides **c50** (build-from-source) and
**c56** (registry upload) infra, both unverified/soft-blocked on dep approvals. Promote to a
full card with worked `jet publish` shell examples once M12.2 infra is verified.
Rec direction: `jet publish` infers version from `pkg.jet`, refuses re-publish + a dirty
tree, resolver defaults to highest-compatible with a committed lockfile. From the 2026-06-20
persona run (Saoirse, Amara).

---

**D-JITDEP1 — DECIDED 2026-06-24: approve Cranelift** (runtime-side JIT tier-1, I6 holds).
Recorded in `syntax-decisions.md`. Active work = board card for the Cranelift backend over
the `JitBackend` seam; the own-bytecode-VM and own-native-JIT progression are frozen cards.

---

**D-QUAL4 — Plain marker-tag type-position spelling (prefix vs postfix).**
*User story:* A web dev marks a value `#Tainted` at its source and needs to write
the *type* of a tainted string in a function signature — `flagged: #Tainted String`
vs `String #Tainted`. Same question for `#SingleUse`, `#NoCopy`, and the typestate
markers — the plain (non-parameterized) value-tags that attach to an existing type
rather than minting a new one (so D-QUAL3's "mint a type" Option C doesn't apply).
*Decision (when promoted):* prefix `#Tag Type` (matches every other Jet `#Marker`:
`#Test fn`, `#Numeric distinct`) vs postfix `Type #Tag`. Rec direction: **prefix**, for
one consistent marker idiom. *Why deferred:* no ready consumer — units (c68) ride D-QUAL3
and mint types; the first plain value-tag consumer is taint (D-TAINT1, gated on D-EFF1)
or single-use (D-LIN1, c71). Promote to a full card when c71 or the taint work starts.
Split from D-QUAL3 on 2026-06-24 (a single card can't pick both axes).

---

**D-PROP1 — Effect prohibitions: implicit propagation of `#(no_…)`.**
*User story:* A security engineer wants to know, by reading the root call
site, that a call graph never touches the network — without auditing every
callee. He writes `#(no_net)` on a function and the compiler traces every
reachable call for a net effect, naming the violating path.
*Why deferred:* Rides **D-EFF1** (the effect-propagation engine itself) plus
D-QUAL1's surface (`#(…)`); prohibition is the inverse-lattice follow-on once
positive effects propagate. Sequencing: D-EFF1 → D-PROP1. Board items #24/#4.

---

**D-ROLE1 — Time-varying roles: typestate + time.**
*User story:* A hotel booking system dev wants to express that a `Reservation`
is `#pending` before payment and `#confirmed` after — and that calling
`check_in` on a `#pending` reservation is a compile error.
*Why deferred:* Requires the typestate machinery from **D-STATE1** (gated on
D-QUAL2) to be ratified first; "time-varying" adds a temporal ordering
constraint on top of static typestate, a separate design question. Board item #13.

---

**D-REFINE1 — Refinement types.**
*User story:* A numeric processing library author wants `PositiveInt` to be a
type the compiler can prove is always > 0, so she doesn't pepper every
function with `require(n > 0)`.
*Why deferred:* Refinement types require a proof/SMT layer that is not in the
roadmap for v1; the simplicity ratchet (I8) requires a concrete milestone slot
and owner sign-off before any work begins. Board item #19.

---

**D-BUDGET1 — Budgets as types.**
*User story:* A systems developer writing a real-time renderer wants to express
that `render_frame` has a 16ms CPU budget and have the compiler warn if a
called function is known to exceed it.
*Why deferred:* Requires comptime cost-bound inference, which is not in the
v1 roadmap; no prior-art consensus on how to make it ergonomic without macros
(I8 / no macros). Board item #22.

---

**D-IFC1 — Information-flow and compliance tracking.**
*User story:* A fintech dev wants to annotate a value as `#pii` (personally
identifiable information) and have the compiler refuse to let it flow into a
logging call or a non-encrypted storage write without an explicit sanitize
step — enforced at compile time, not by code review.
*Why deferred:* This is **D-TAINT1 Option B** (full information-flow control —
security-label lattice, principals, `declassify`), which the **owner explicitly
deferred to post-Epoch-3 on 2026-06-21** when ratifying D-TAINT1 Option A
(`#tainted` + sanitizers). Captured here so it is not lost. Generalizes D-TAINT1
and requires the full effect/tag propagation model from D-EFF1 and D-QUAL1 to be
ratified first; the compliance dimension (what counts as a legal sink) is a policy
question that also interacts with the manifest capability model (D-QUAL1 Option A,
manifest surface). Board items #30/#33.

---

**D-REPLAY1 — Opt-in record and replay.**
*User story:* A game developer wants to record a session's inputs, replay
them deterministically to reproduce a bug, and have the compiler ensure no
hidden state (system clock, random, I/O) is read during replay without being
mocked.
*Why deferred:* Requires the effect system (D-EFF1) to tag non-deterministic
effects and a runtime record/replay harness; neither is in the v1 roadmap.
Board item #7.

---

**D-REVERSE1 — Opt-in reversible computation and solver integration.**
*User story:* A constraint-based UI layout author wants to write the forward
constraint (`width = parent.width - padding * 2`) and have Jet automatically
solve for `padding` given a target `width` — without writing the inverse by
hand.
*Why deferred:* Requires a reversibility annotation on functions and a
solver/SMT backend; no prior-art consensus on making this ergonomic without
macros or dependent types. Board item #36.

---

**D-PROTO1 — Protocol and session type generation.**
*User story:* A network protocol implementer wants to declare a
request/response handshake sequence as a type and have the compiler generate
both the client and server stubs, rejecting code that sends messages out of
order.
*Why deferred:* Session types require linear types (used exactly once, in
order) and typestate; **D-LIN1** (linear tag) and **D-STATE1** (typestate),
both gated on D-QUAL2, are prerequisites, and the code-generation surface for
protocol stubs is a separate design. Board item #9.

---

**D-VERIFY1 — Formal verification and proof integration.**
*User story:* A cryptography library author wants to attach a machine-checked
proof that her `constant_time_eq` function runs in time independent of its
inputs, and have the Jet toolchain refuse to ship the library if the proof
doesn't hold.
*Why deferred:* Requires a proof-carrying-code or SMT integration layer that
is explicitly post-v1; the simplicity ratchet (I8) bars this without a
concrete roadmap slot and owner sign-off. Board items #15/#17.

---

## B6 `defer` — already decided, no ballot

`defer` is solved; nothing to vote on. **D-DEFER1 (ratified + implemented 2026-06-20)** shipped `core.scope.guard(() => {…})` — a stdlib value whose `Drop` runs the stored lambda LIFO on every exit path including `?`. `defer`-as-primary stays rejected (S63); the `defer` keyword stays declined (D-SUGAR5).

```jet
use core.scope

fn copy_file(src: String, dst: String) -> () ? Error {
    f :: core.fs.open(src)?
    g1 :: scope.guard(() => { core.fs.close(f) })   // replaces `defer close(f)`
    g :: core.fs.create(dst)?
    g2 :: scope.guard(() => { core.fs.close(g) })   // fires before g1, even on early return
    core.fs.copy(f, g)?
}
```

**Reopen (owner-only):** you could later add `defer expr` as sugar over `scope.guard` (same Drop-backed lowering, zero runtime cost). For: it's the spelling Jai/Go/Swift/Odin/Zig converge on. Against: D-SUGAR5 declined it; it adds a second cleanup spelling and reintroduces Go's leak-by-omission class. No agent reopens this without your instruction.

---

## Coverage — D-COV1 (deferred, no ballot needed)

The epoch-3 plan scopes coverage as "tooling only — no new syntax; couples to the
test runner in `Source/main.rs` (`run_test`)." There is no user-facing surface
decision: `jet test --coverage` is the spelled-out verb and the output format (LCOV
/ HTML / stdout summary) is an implementation choice, not a syntax choice.

**Prior art:**
- **Rust tarpaulin** — `cargo tarpaulin --out Html`; produces HTML + lcov. No new
  Rust syntax. Jet takeaway: a `--coverage` flag on `jet test` is the right shape.
- **llvm-cov / cargo llvm-cov** — output: `--json`, `--lcov`, `--html`, `--text`.
  Jet takeaway: multiple formats are useful but can be deferred to a `--format`
  flag.
- **Python coverage.py** — `coverage run`; then `coverage report` / `coverage html`.
  Two-step. Jet takeaway: a single `jet test --coverage` that prints a summary to
  stdout (and optionally writes a report) is simpler than a two-step model.

**Deferred note:** if coverage ever needs a source annotation (e.g. `// @no_cover`
to exclude a line from the report), that is a syntax decision requiring a ballot.
Until then, coverage is tooling-only and can land without owner ratification. The
implementation milestone (exit criterion: `jet test --coverage` reports per-line /
per-function coverage) can proceed independently of D-TEST1 and D-TEST4.

---

