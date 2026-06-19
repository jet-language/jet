# Sidequest: Memory Capability Model

**Status:** plan, awaiting owner sign-off — promoted from owner-todo.md 2026-06-19.

## Goal

Replace Jet's current three-mode access-convention system (`Read`/`Mutate`/`Move`) with a
four-capability vocabulary (`view`/`edit`/`take`/`share`) that users actually see and think
in. The compiler continues to lower to Rust's `&T`/`&mut T`/move/`Arc<T>` internally; the
change is in what surfaces to users — in parameter positions, diagnostics, and the stable-API
layer. The result: Rust-level safety with a dramatically simpler mental model, no borrow
checker or lifetime terminology visible at the Jet layer.

## Current state

The compiler tracks a three-variant `AccessConvention` enum (`src/ast.rs:8`):
`Read`, `Mutate`, `Move`. `LocalInfo.param_conv: Option<AccessConvention>`
(`src/sema/mod.rs:181`) marks parameters; a `moved` map tracks consumed values.
`rust_param_type` in `src/codegen/cx.rs:259` lowers conventions to Rust: `Read` →
`&T` (non-scalar by-value for scalars), `Mutate` → `&mut T`, `Move` → `T`.

Two surface keywords exist: `KW_MOVE = "take"` (S10, M2) at call sites, and `KW_VIEW =
"view"` (S10, M2) for return-type borrow annotations — both in `src/syntax.rs:80,83`.
These are caller-side only; there is no `edit` or `share` keyword, no parameter-position
capability annotation syntax, and no inference rule set.

The implicit-clone warning **L0201** (`src/sema/checker_ownership.rs:515`,
`checker_infer.rs`, `checker_stdlib.rs`) fires when a `Move`-convention parameter
receives a `Read`-convention argument that the compiler silently `.clone()`s. The
message already suggests `take name` — a sign the vocabulary is partway there.

No `share` concept exists at the sema layer today. Shared ownership must be handled
manually (e.g. a cloned `Arc` in generated Rust). There is no capability ordering,
no inference algorithm, and no API-mode manifest flag.

## Design

### Core Philosophy

Jet tracks *capabilities* rather than ownership. Every access falls into one of four:

```
view   read-only access ("look at it")
edit   exclusive temporary mutable access ("change it")
take   ownership is consumed and retained ("keep it")
share  multiple owners exist ("multiple owners")
```

The compiler guarantees:
1. Data cannot disappear while being used.
2. Two writers cannot modify the same value simultaneously.
3. Readers cannot observe partially-written state.

### Capability Ordering

Capabilities form a hierarchy:

```
view < edit < take < share
```

The compiler must always choose the *weakest* capability that safely supports the
function body. Never choose a stronger capability for optimization alone. Correctness
first; optimization second.

### Inference Rules

Users may omit capability annotations entirely. The compiler infers:

**Rule 1 — read-only:** a parameter that is only read infers `view`.

```jet
proc print_name(player: Player) {
    print(player.name)
}
// inferred: player: view Player
```

**Rule 2 — modified but does not escape:** a parameter (or any reachable field) that is
modified but stays within the function infers `edit`. Mutation does not imply ownership
transfer.

```jet
proc heal(player: Player) {
    player.hp += 10
}
// inferred: player: edit Player
```

**Rule 3 — value escapes:** a parameter that escapes the function via `return`, push into
a collection, or closure capture infers `take`.

```jet
proc add_member(party: Party, player: Player) {
    party.members.push(player)
}
// inferred: party: edit Party   (modified in place)
//           player: take Player  (escapes into party.members)
```

**Rule 4 — multiple owners required:** when a value must be accessible from multiple
independent owners simultaneously, infer or require `share`.

```jet
// texture used by many sprites
// inferred or required: texture: share Texture
```

### Explicit Capability Syntax

Experts may always override inference. Explicit annotations are promises the compiler
enforces:

```jet
proc draw(scene: view Scene)
proc heal(player: edit Player)
proc add(player: take Player)
proc cache(texture: share Texture)
```

If an annotation is violated, the compiler errors in capability vocabulary:

```jet
proc inspect(player: view Player) {
    player.name = "Kai"   // error: cannot edit a value declared as view
}
```

### Package-type Defaults

**Executable packages** (`PackageKind::Executable`) prioritize ergonomics: infer
everything. Explicit annotations are available but optional.

```jet
proc update(player: Player)   // capabilities fully inferred
proc render(scene: Scene)
```

**Library packages** (`PackageKind::Library`) also infer by default but emit capability
metadata alongside the compiled artifact. Example:

Source:
```jet
pub proc heal(player: Player) {
    player.hp += 10
}
```

Published API metadata:
```text
heal(player: edit Player)
```

Consumers compile against the published metadata. Because Jet packages are hash-pinned,
a capability change in a library is a *versioning* issue, not a safety issue — existing
consumers remain pinned to the prior hash.

### Stable API Mode

Optional manifest flag:

```jet
package api = stable
```

The compiler records public capability signatures at publish time. Future changes that
alter a recorded signature trigger an API-break diagnostic rather than silently shifting.

### Explicit API Mode

Optional manifest flag:

```jet
package api = explicit
```

All `pub` functions must declare capabilities; unannotated public functions are a
compile error. Intended for library authors who want to make the contract visible in
source rather than relying on inferred metadata.

```jet
pub proc write(file: edit File)   // required under api = explicit
```

### Internal Compiler Representation

Capabilities lower to Rust's ownership mechanics. Users are not required to understand
this mapping; it is purely internal:

```
view   →  immutable reference / &T
edit   →  exclusive mutable reference / &mut T
take   →  ownership transfer / move
share  →  shared ownership / Arc<T> or equivalent
```

The source-level capability annotation (or inferred capability) is the authoritative
record. The lowering is an implementation detail.

### Copying and Sharing

Jet must never silently duplicate expensive values. To explicitly duplicate:

```jet
copy texture
```

To explicitly create shared ownership:

```jet
share texture
```

Both forms are intentional, visible, and user-authored. The implicit-clone path that
currently triggers L0201 should be eliminated: every duplication must be either
`copy`, `share`, or a scalar copy that costs nothing.

### Diagnostics Vocabulary

All capability-related diagnostics teach and use capability language. Preferred terms:

```
view  /  edit  /  take  /  share
value escapes
this function keeps the value
shared ownership
```

Never use in beginner-facing output:

```
borrow checker
lifetime
&T  /  &mut T
```

### Required Examples

The following examples must exist as golden tests (I5) and ui snapshots (I4).

**Read-only (view inference):**
```jet
proc print_name(player: Player) { print(player.name) }
// inferred: player: view Player
```

**Mutable (edit inference):**
```jet
proc heal(player: Player) { player.hp += 10 }
// inferred: player: edit Player
```

**Ownership (take inference):**
```jet
proc add_member(party: Party, player: Player) {
    party.members.push(player)
}
// inferred: party: edit Party, player: take Player
```

**Post-take error:**
```jet
player := Player{}
party.add(player)
print(player.name)   // error: player was taken by party.add
```

Diagnostic:
```
player was taken by party.add

Suggestions:
  use copy player
  use share player
  use player before the call
```

**Final goal:** Jet code reads as plain procedure calls with no capability annotations;
the compiler derives `view`/`edit`/`take`/`share` automatically and enforces memory
safety with minimal user-facing complexity.

```jet
heal(player)
draw(scene)
party.add(player)
```

## Decisions for the owner

Each decision needs a ruling before implementation. Capability ordering and inference
rules above are recommendations; the owner has final say on all syntax (I7, I8).

**D-CAP1 — Capability keyword spellings (I7)**

Are `view`, `edit`, `take`, `share` the ratified Jet keywords for parameter annotations?
`take` and `view` already exist in `src/syntax.rs` as S10/M2 caller-site keywords; this
would extend them to parameter-position annotations and introduce `edit` and `share` as
new ratified sigils.

```jet
// Before: AccessConvention (Read/Mutate/Move) — internal only, not user-visible
proc heal(player: Player) { … }

// D-CAP1 = ratify: four keywords usable in parameter position
proc heal(player: edit Player) { … }
```

Recommendation: ratify all four. `take`/`view` are already halfway there.

---

**D-CAP2 — `copy` and `share` as keywords vs. method calls**

Should `copy x` and `share x` be first-class keyword expressions, or should they be
explicit method/function calls (`x.copy()`, `Arc::new(x)` equivalent)?

```jet
// Keyword form (explicit + teachable):
copy texture
share texture

// Method form:
texture.copy()
texture.share()
```

Recommendation: keyword form — it matches the four-capability vocabulary and is visible
at a glance.

---

**D-CAP3 — Annotation order: `player: edit Player` vs. `edit player: Player`**

Where does the capability keyword sit — on the type side or the binding side?

```jet
// Type-side (proposed above):
proc heal(player: edit Player)

// Binding-side (alternative):
proc heal(edit player: Player)
```

Recommendation: type-side (`player: edit Player`) — the capability modifies what the
parameter *is*, not the name; mirrors how other type annotations work.

---

**D-CAP4 — `package api = stable | explicit` manifest spelling**

Is the package-type manifest syntax `package api = stable` / `package api = explicit`,
or a different form (e.g., an `#[api(stable)]` attribute, or an `api:` field in
pack.jet)?

```jet
// Proposed form in pack.jet or at package declaration:
package api = stable

// Alternative: attribute
#[api(stable)]
package MyLib
```

Recommendation: manifest field in pack.jet (`api: stable`), consistent with how
`PackageKind` is already declared.

---

**D-CAP5 — Interaction with lib/exe targets redesign (cross-ref D-TGT1)**

The capability defaults above distinguish `exe` (infer everything) from `lib` (infer +
emit metadata). If the lib/exe binary distinction is dissolved into fine-grained
targets (see `lib-exe-targets-model.md`), which targets carry the "emit capability
metadata" behavior, and which get the ergonomic defaults?

```jet
// Today: PackageKind::Library emits metadata, Executable does not
// After targets: does a `binary` target infer-only? Does a `library` target emit?
```

Recommendation: resolve D-TGT1 first; the capability model's package-type defaults
will follow that ruling.

---

**D-CAP6 — When does `api = explicit` become the library default?**

Should `api = explicit` ever become the default for published libraries, or remain
permanently opt-in?

Recommendation: opt-in forever. Inference is the default; explicit is for authors who
want to make contracts visible in source.

## Acceptance checklist

- [ ] Failing ui snapshots for view/edit/take inference (I4): one `.jet` + `.stderr`
      per inference rule before implementation.
- [ ] Failing golden example: a feature file exercises all four capabilities
      inferred, compiles, produces expected output.
- [ ] D-CAP1 through D-CAP6 resolved by owner.
- [ ] `src/syntax.rs` updated with ratified capability keywords + decision IDs (I7).
- [ ] `docs/spec/spec.md` section added / updated for capability model.
- [ ] `src/ast.rs` `AccessConvention` extended or replaced with four-variant enum.
- [ ] Inference rules implemented in `src/sema/checker_ownership.rs` (replace or
      extend current `Read`/`Mutate`/`Move` logic).
- [ ] `edit` and `share` lowering added to `rust_param_type`
      (`src/codegen/cx.rs:259`).
- [ ] L0201 implicit-clone warning eliminated or rephrased in capability vocabulary;
      `copy`/`share` are the new user-facing paths.
- [ ] Post-take diagnostic uses capability wording (no `borrow`/`lifetime` terms).
- [ ] `package api = stable` / `package api = explicit` parsed in
      `src/jetpack/packmanifest/parse_blocks.rs`; manifest-level behavior wired.
- [ ] `nix develop -c cargo test` green; no new `unsafe` in generated user-visible
      code (I1).
- [ ] All new diagnostics in `docs/spec/diagnostics.md` with codes; ui snapshots
      pinned (I4).
- [ ] Docs updated: `docs/spec/syntax-decisions.md` row for each new capability
      keyword.
