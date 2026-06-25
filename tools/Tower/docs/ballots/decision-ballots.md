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

_Two card groups open for the owner: **c96** registry / jet publish (D-PUBLISH1A, D-VERSION1, D-RESOLVE1, D-LOCK1) and **c136** generic-type serde (D-SERDE9–D-SERDE12). All developed to the house format and agent-reviewed._


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

