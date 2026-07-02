# Gameplay DNA: What UE's Tag/Effect/Ability Model Gives Core Jet

> Proposal for owner review (card #181, 2026-07-02, rev 2 — devil's-advocate
> pass applied). Four transplants from Unreal's Gameplay Ability System into
> core Jet. Rule for this rev: **every transplant lands as machinery users
> already know** — enums, derives, structs, patterns — never a parallel GAS
> vocabulary. If a beginner must learn a foreign system to benefit, the design
> is wrong. Each section shows *today's Jet* vs *proposed* side by side, then
> the pushback that shaped it. New spellings are unratified, flagged with
> ballot IDs.

Why GAS is worth mining: it lets non-experts assemble deeply interacting
systems (hundreds of abilities/effects/tags in a shipped game) without the
combinatorial bug explosion that normally kills that. Four moves —
hierarchical names, changes as data, requirements as declarations, fields
with policy — all bolted onto C++ as conventions. Jet can make them language
truths *without inventing new surface for any of them*.

Glossary: **GameplayTag** — hierarchical name (`Damage.Fire.Burn`) matched by
ancestry, not identity. **GameplayEffect** — a state change described as
data (what, how much), never a direct mutation. **GameplayAbility** — an
action bundling its activation requirements with its body.
**AttributeSet** — stats whose clamping/derivation rules live on the field.

---

## 1. Hierarchical names → nested enums

**UE move.** Every cross-system concept is a dot-path in one tree; systems
match on ancestors (`has .Damage.Fire` catches `.Damage.Fire.Burn`), so they
compose without importing each other. UE's failure modes: tags are stringly,
renames break silently, add/remove counting is a chronic bug (two stuns
applied, one removed → not stunned).

**Today.** Hierarchy by naming convention; ancestor matching by hand; no
counting:

```jet
enum Damage {
    PhysicalBlunt
    PhysicalPierce
    FireBurn
    Cold
}

fn is_fire(d: Damage) -> Bool {
    if d == {
        .FireBurn -> true
        else -> false        // add FireScald later: silently stays wrong
    }
}

active: Set<Damage> := Set.from([])   // no counts: two burns, one cure → cured
```

**Proposed.** The existing `enum`, one new axis: variants may group.
[D-TAG1 — unratified]

```jet
enum Damage {
    Physical { Blunt, Pierce }
    Fire { Burn, Scald }
    Cold
}

d :: Damage.Fire.Burn
d == .Fire                     // true — ancestor test via ratified pattern law (S31)

if d == {
    .Physical -> armor()       // arm matches the subtree; exhaustive without listing leaves
    .Fire -> douse()
    .Cold -> warm()
}

active: Bag<Damage> := Bag.new()        // counted multiset — core.collections sibling of Set
active.add(.Fire.Burn)
active.add(.Fire.Burn)
active.remove(.Fire.Burn)
active.has(.Fire.Burn)                  // true — one burn left (UE's stuck-stun bug, gone)
burning :: active.any((t: Damage) => t == .Fire)   // ancestor query, plain closure
```

Nothing foreign: a value is always a leaf; `==`-pattern tests, dispatch arms,
and leading-dot values work exactly as ratified (S31, S68, D-ENUMDOT1) with
one new fact — a group name matches its subtree. Renames are refactors,
exhaustiveness is enforced, the LSP completes every `.` level.

Beyond games: plugin systems, feature flags, permission models, diagnostics
categories — today done with flat enums plus name prefixes, or strings.

**Pushback (what the pass killed).**
- `tag Damage { }` keyword — collides with ratified D-QUAL2 `tag` (qualifier
  kind) and mints a second closed-name-set mechanism beside `enum` (I8).
  Dead. Nested enums instead.
- `TagSet`/`TagQuery` types — GAS vocabulary. `Bag<T>` (counted multiset) is
  the standard container name and works over any element type; a stored query
  is just `[Damage]` plus `any`. Serializable query values, if ever needed,
  are a later library decision.
- Open trees (packages extending each other's enums) — killed for v1;
  closedness is what buys exhaustiveness. Plugin extension is a separate
  future decision.
- Ballot detail: payloads on grouped variants — recommend leaf-only, no
  payloads on group names, v1. Whether `Bag.has(.Fire)` accepts subtree
  patterns as sugar for the `any` closure is a ballot option, not assumed.

## 2. Changes as values → derived Patch type

**UE move.** Nothing mutates a stat directly. A change is a data value, so it
can be previewed, logged, reverted, sent over the wire, predicted
client-side and reconciled.

**Today.** `.{ }` copy-update (D-COPYUPDATE1, ratified) applies a change
*now*. A change you hold — for undo, audit, an API PATCH body — is
hand-rolled per type:

```jet
struct EmployeePatch {
    level: Int?
    title: String?
}
fn apply(e: Employee, p: EmployeePatch) -> Employee {
    e.{
        level: p.level ?? e.level,
        title: p.title ?? e.title,    // one line per field, per type, forever
    }
}
```

**Proposed.** A derive on the `@` contract plane, like `@Codable`.
[D-PATCH1 — unratified]

```jet
@[Patchable]
struct Employee {
    level: Int
    title: String
    team: String
}

promote :: Employee.Patch.{ level: 4, title: "Senior" }   // ordinary .{ } — set fields = the change
emp2 :: emp.apply(promote)
undo :: Employee.diff(emp2, emp)       // the patch that gets you back
both :: promote.merge(relocate)        // right side wins on overlap
audit.log(promote)                     // Patch is Codable by construction
```

`Employee.Patch` is just a generated struct whose fields are all optional —
constructing, reading, matching, and serializing it are skills the user
already has. Zero new syntax; the ballot decides the derive surface only.

What it replaces: hand-rolled undo/redo, audit-log diff code, optimistic-UI
apply-predict-reconcile, DB partial updates — one canonical change value
(I8). Natural record format for `#Transact` and D-REPLAY1.

The I8 seam, stated: `.{ }` is *change, applied now*; `Patch` is *change as
a value, applied later or elsewhere* — and `apply` is defined by `.{ }`
semantics, so there is one meaning of "edited copy".

**Pushback (what the pass killed).**
- Relative magnitudes (`level: +1`) — literal semantics that exist nowhere
  else in the language. Dead. A computed change is code, not data: write
  `emp.{ level: emp.level + 1 }`. Patches carry absolute values — which is
  what serialization and reconciliation need anyway.
- `invert(base)` — footgun: only valid against the exact base it was
  inverted on. Dead; `diff(new, old)` is the honest spelling.
- Duration/stacking policy (`Timed<Patch<T>>`, stack rules) — GAS-shaped
  library speculation. Out of the proposal; libraries can build these on the
  core value.
- "Isn't a patch just `fn(T) -> T`?" — a lambda applies, but can't be
  logged, diffed, merged field-wise, or sent. Data-not-code is the point.

## 3. Requirements at the signature — already law, zero ballots

**UE move.** An ability declares required/blocked tags and costs next to its
body; the engine checks them; UIs gray buttons that can't fire and say why.

**Today = proposed.** Jet already ratified the pieces; §1 supplies the
dynamic state to condition on:

```jet
@Pre(session.roles.any((r: Role) => r == .Auth.Admin), "requires admin")  // D-PREPOST1 + D-TAG1
fn purge(session: Session) #(Db.Write)                                    // effects = declared cost

#State(Open) fn close(self)                                               // D-STATE1 lifecycle gate
```

The transplant is tooling, not syntax: because requirements are
declarations, the LSP can answer "why can't I call this here?" with the
exact failed clause, and the Blueprint editor (blueprint-editor.md) grays
un-wireable nodes with the reason — UE's best UX affordance, derived from
law already on the books.

**Pushback.** Rev 1's derived `purge.available(session)` runtime query is
cut. Arity is unsolved (which arguments does the check take when clauses
mention several params?), and the LSP/editor story needs no runtime API. If
a UI framework later needs a runtime "can I?", that is a follow-on API
decision that must answer the arity question first.

## 4. Fields with policy → computed fields

**UE move.** A stat is not a float: clamping, base-vs-current, and reaction
rules live on the field declaration, so no caller anywhere can produce -3
health.

**Today.** Clamping is done — `type Health :: Int(0..1000)` (D-RANGETYPE1,
ratified); `#Invariant` (D-REFINE1) covers non-range rules. A field defined
by its siblings is a method:

```jet
struct Stats {
    strength: Int
    gear_mod: Int
}
fn attack(self) -> Int { self.strength * 2 + self.gear_mod }
// works — but invisible to @Codable output, and the moment someone
// stores attack for speed or serialization, it goes stale
```

**Proposed.** Computed field — `=>` because it computes.
[D-FIELDPOL1 — unratified]

```jet
struct Stats {
    strength: Int(1..99)
    gear_mod: Int(-20..20)
    attack: Int => strength * 2 + gear_mod
}

s :: Stats.{ strength: 10, gear_mod: 3 }
s.attack                    // 23 — never stored, never stale
s2 :: s.{ strength: 12 }    // 27 in the copy; setting attack is an error with a fix-it
```

Reads like Kotlin/Swift/C# computed properties — the standard spelling for
this idea. `=>` already means "computes to" (lambdas); `=` stays free for
the field-default slot every mainstream language spells that way.

Combined with §2: patch `strength`, `attack` is correct in the copy —
GAS's base/current split with zero bookkeeping. Kills the
stale-denormalized-field bug class (caches, totals, display strings).

**Pushback.**
- "It's just a zero-arg method" — semantically yes; the differences are the
  value: appears in `@Codable` encode, recomputes under `.{ }` and patches,
  unsettable, no parens. If the ballot judges those not worth a spelling,
  the honest option B is "keep methods, no feature".
- Rev 1's `attack: Int = expr` — dead: `=` in field position reads as
  *default value* in every mainstream language; taking that slot would
  foreclose the eventual defaults decision.
- Cycles (`a => b`, `b => a`) — compile error; derived-from-derived allowed,
  dependency-ordered.
- Change-reaction hooks — cut. Reactive `Computed<T>` (ratified reactive
  stack) is the answer when you need reactions; this is deliberately its
  non-reactive plain-struct twin. One authoring story per job.

## 5. Effect hierarchy — flagged: reopens ratified law

D-EFF4/5 deliberately ratified a **flat** closed vocabulary with **no
subsumption** (`Net` under `#(Io)` is E0740). This section proposes
reversing that: re-base the ten names as tree roots; ancestor matching *is*
subsumption — the same rule as §1, learned once.
[D-EFFTREE1 — unratified, a reopen]

```jet
fn load(path: String) -> Config #(Fs.Read)     // finer than #(Fs)
fn fetch(url: String) -> Body #(Net.Http.Get)

#Grant(Fs.Read) { … }      // D-SCAP1 grants gain subtree precision
```

`#(Fs)` in a signature accepts any `Fs.*` callee; a `#(Fs.Read)` caller
rejects an `Fs.Write` callee. Existing flat names remain valid as roots — no
migration break. Settles the acknowledged D-EFF2 subsumption documentation
gap structurally.

**Pushback.** Only transplant that reopens a decided question, so it carries
that burden explicitly: no-subsumption was chosen for teachability (ten
names, no taxonomy arguments). The counter: §1 makes ancestor matching a
rule users learn anyway, and read/write precision on `Fs`/`Db`/`Net` is the
most-requested capability refinement. Sequence last; ballot as a reopen of
D-EFF4/5, not a fresh decision.

## The common thread

All four are the bet Jet already made: **make the thing declarative, give it
a dot-path, let the LSP show the tree** — via standard machinery only: one
enum axis, one derive, existing contracts, one field form. Each kills a bug
class at compile time and gives the Blueprint editor one more thing to
render, gray out, or autocomplete. Nothing here asks a user to learn a
gameplay framework.

Adopt order by leverage: **D-TAG1** (unlocks §3's dynamic requirements and
§1's architecture) → **D-PATCH1** (state story) → **D-FIELDPOL1** (small,
high beginner value) → **D-EFFTREE1** (reopen; sequence behind the others).

## Open decisions

| ID | Question | Feeds |
|---|---|---|
| D-TAG1 | Nested variant groups in `enum`: grouping spelling, subtree matching + exhaustiveness, leaf-only payloads, `Bag<T>` counted multiset in core.collections, pattern-sugar on `has` | §1, §3 |
| D-PATCH1 | `@[Patchable]` derive: nested `T.Patch` type, `apply`/`diff`/`merge` surface, Codable interplay | §2 |
| D-FIELDPOL1 | Computed struct fields `name: T => expr-over-siblings`: recompute under `.{ }`/patches, Codable output, cycle rule | §4 |
| D-EFFTREE1 | **Reopen D-EFF4/5**: effect vocabulary as a tree with ancestor subsumption (`#(Fs.Read)`, subtree `#Grant`) | §5; settles the D-EFF2 gap |

§3 ships no ballot — it composes ratified law (D-PREPOST1, D-STATE1,
D-SCAP1) with D-TAG1.
