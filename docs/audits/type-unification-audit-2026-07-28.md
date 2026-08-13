# Type-unification audit — 2026-07-28

Vocabulary: [Jet vocabulary](../spec/vocabulary.md).

**Status:** first run of a new audit kind (owner-directed). The question:
which of Jet's traits, tags, markers, and keyword constructs are secretly
types? Where would honest types buy clarity, functionality, magic, or
explicit control? This report is a fix plan, not a list of regrets. Every
finding names its fix, its vehicle (card or ballot), and its evidence.

**Method.** Full inventory of the marker registry
(`crates/jet-foundation/src/Policy.rs`), the syntax registry
(`crates/jet-foundation/src/Syntax*.rs`), the type surface (`AST/types.rs`,
`AST/items.rs`), and ratified law in `docs/spec/syntax-decisions.md`. Live
compiler probes back every headline claim; a fresh-context peer reviewer
re-ran all of them. Three review passes shaped this text: peer (factual),
adversarial (invariants and taste), and the "pay time up front" pass. Their
records are at the end.

## Thesis

Jet already runs on types-as-facts. It just refuses to say so. The compiler
keeps a shadow type system in four fragments that never meet:

1. The marker registry gives every `#Rule` a typed signature over 14
   parameter types (`State`, `Capability`, `TaintKind`, `PolicySetting`, …).
   No user can name, inspect, or extend them.
2. Sema mints erased handle types (`TaskGroup`, `Capability`, `Transaction`,
   `SelectBuilder`). They appear in error copy but reject user signatures.
3. Four mechanisms spell one behavior — erased named facts with set or tree
   matching: `tag`, `state`, taint kinds, effect leaves.
4. Physical dimensions live in a closed 5-row compiler constant. A user's
   `Mass` gets none of the algebra the compiler's `Length` gets.

The parts of Jet that should be type-shaped already are, and they are the
best parts: `#UnitFamily` mints real types, `protocol` compiles to typed
handles, typed text (`SQL`/`Sh`) is a type not a marker, and value-`if` /
yielding `loop` / `Task<T>` / `Stream<T>` reify control flow exactly where
it produces values. The fix direction is therefore not "add a meta-type
system". It is: finish the pattern Jet already chose. Open the one closed
representation (dimensions). Publish the phantom types the compiler already
reasons with. Unify the four fact spellings behind one model before they
calcify apart.

## The target shape (the cohesive end state)

Every fix below serves one picture. It has four planes, and Jet already has
three of them half-built:

- **Runtime types** — structs, enums, distincts, unit-family members,
  containers, trait objects. Complete today.
- **Fact types** — every erased compile-time classification: tags, states,
  taint kinds, effect leaves, dimensions. One declaration story, one
  subsumption algorithm, one namespace rule. Today: four stories, zero
  namespaces.
- **Handle types** — every runtime artifact a construct mints: `Task<T>`,
  `Stream<T>`, `Iter<T>`, `TaskGroup`, `Range`. All nameable. Today: three
  of five.
- **One meta surface** — `TypeInfo` from `T.reflect()`. It reports markers,
  states, dimensions, and rule arguments as typed values, not strings.
  Today: strings, and only markers.

One out-of-the-box move ties the planes together: **the compiler's
vocabulary ships as readable prelude source.** Rule argument enums,
dimension declarations, stdlib effect leaves, and fact kinds become ordinary
Jet declarations in the prelude. `jet explain` links to them. Users learn
the rule system by reading Jet, not by reading the compiler. The registry
generates the prelude text, so there is one source of truth and a
drift-guard test. No other language ships its marker plane as source the
user can read in their own syntax; this is cheap for us because the
registry already holds every signature.

## Fix plan at a glance

| # | Fix | Vehicle |
| --- | --- | --- |
| F1 | Open dimension algebra to user-declared dimensions | ballot `D-DIMENSION-OPEN1` |
| F2 | Marker argument types become real prelude enums | ballot `D-RULEARG-TYPES1` |
| F3 | One fact model behind tags, states, taint kinds, effect leaves | ballot `D-FACTMODEL1` |
| F4a | Undeclared tag in type position must error | bug card |
| F4b | Opt-in strict tags with an audited blessing gate | ballot `D-TAG-STRICT1` |
| F5 | States join the type's namespace and `TypeInfo` | ballot `D-STATE-NS1` |
| F6 | `TaskGroup` becomes a nameable parameter type | ballot `D-TASKGROUP-PARAM1` |
| F7 | Ranges become values (`Range` type) | ballot `D-RANGE-VALUE1` |
| F8 | Optional effect-leaf declarations kill typo leaves | ballot `D-EFFECT-DECL1` |
| F9 | User typed-text heads: recorded direction, deferred with IFC | note only |
| F10 | Marker/trait name-collision law | ballot `D-MARKER-NAME-HYGIENE1` |
| F11 | Spec law: constructs are never types; artifacts always are | doc card |
| — | `Machine.Off` diagnostic says the type does not exist | bug card |

## The kind zoo (what declares a type-like thing today)

"Nameable" = usable in type position. "Reflectable" = visible through
`T.reflect()`. "Open" = users can mint new instances.

| Kind | Decision | Mints | Nameable | Reflectable | Open | Path |
| --- | --- | --- | --- | --- | --- | --- |
| `struct` / `enum` / `distinct` / `alias` | M3 / S30 / D-DIST1 / D-TYPEALIAS1 | nominal runtime types | yes | yes | yes | items pipeline |
| `enum` nested groups | D-TAG1 | runtime variant trees with subtree matching | yes | yes | yes | items pipeline |
| `trait` | S28/S48 | contract + dyn type | yes (auto-boxed) | partial (`implements`) | yes | `Traits.rs` registry |
| `tag` | D-QUAL2/D-QUAL4 | erased qualifier | as `#Tag T` prefix only | no | yes (see F4) | `Traits.rs` `local_tags` |
| `state T { … }` | D-STATE1/D-STATE-DECL | compile-time state set | **no** (probe: E0107) | no | yes | `Sema/State.rs` |
| `protocol` | D-PROTO1/2 | generated `.Client`/`.Server` handles + typestate | yes (generated) | partial | yes | `Sema/Protocol.rs` |
| `#UnitFamily(F) { a, b }` | D-QUAL3 | one `#Numeric` distinct per member | yes (`Usd`) | yes (as distincts) | yes | `Sema/Bundle.rs` |
| dimensions (`Length`, `Time`, …) | D-SHAPE-QUANTITY1 | compile-time exponent vectors | only via `Quantity<Dim,·>` bound | no | **no** (probe: `Mass` fails) | `Syntax.rs` `PHYSICAL_DIMENSIONS` |
| typed text heads (`SQL`/`HTML`/`Sh`) | D-TYPEDTEXT1 / D-UNIFYLIT1=A | nominal checked-text types | yes | no | **no** (heads closed) | `CheckerInfer/expr.rs` |
| `derive T.Name { … }` | D-METADERIVE1 | marker + generated items | applied as `#Name` | n/a | yes | comptime emit |
| `effect <Name>` | D-EFF4/5 | *reserved, unminted* | no | no | leaves open, roots closed | `Sema/Effects.rs` |
| `#Rule` markers (73 active, 19 retired) | D-MARKSIG1 | checks/facts/derives | no | `.markers` strings only | no (registry rows) | `Policy::APPLIED_RULES` |
| `TypeInfo` (`T.reflect()`) | D-METAREFLECT1 | the one user-touchable meta-type | in derive/comptime contexts | is the reflection surface | no (fixed fields) | `Comptime/Reflect.rs` |

Seven of these kinds get separate Pascal-case categories in the one casing
table (`Syntax.rs:278-314`: marker, protocol, state, state type, tag, trait,
unit family). The surface itself testifies that it has seven kinds of
type-like thing. The ontology has roughly two: runtime types and fact types.

## The phantom-type census (types that exist but cannot be written)

| Phantom | Where it lives | Who sees it |
| --- | --- | --- |
| `State` | `Policy.rs:546-547` rule signature (`#State(state: State)`) | E0930 signature text, `jet explain` |
| `Capability` | `effects_surface.rs:123` (`#Grant` handle); rule sigs (`Policy.rs:535,585`) | E0711/E0712 copy |
| `TaintKind` (`.Input/.PII/.Secret/.Credential`) | `Policy.rs:536`; closed set in D-TAINT1/2 | E0721/E0722 |
| `ObligationMode` (`.Track/.Skip/.None`) | `Policy.rs:534`; D-UNSAFE-OBLIG1 | unsafe policy |
| `PolicySetting` / `PolicyValue` | sig string `Policy.rs:526`; `PolicyValue` enum `Policy.rs:21-43`. No Rust `PolicySetting` type exists — it is rendered signature text only | `jet explain marker` |
| `InlineMode`, `Layout`, `IntType`, `NamingCase`, `Maturity`, `Target`, `ABI`, `FfiLanguage`, `Duration \| String` | `Policy.rs:541-596` signatures | E0930, LSP |
| `TaskGroup` | `effects_surface.rs:125-127`; probe: `fn f(g: TaskGroup)` → E0119 "there's no type called `TaskGroup`" | E1110 copy |
| `Transaction` | `effects_surface.rs:232-234` | D-TXN hooks |
| `SelectBuilder` | `effects_surface.rs:146` | select chains |
| `\0Quantity` + dimension names | `Syntax.rs:239-267` | quantity diagnostics |
| Effect leaves (`FS.Read`, `Net.HTTP.Get`) | D-EFFTREE1 — root closed, leaf freeform, never declared | effect rows, pkg.jet budgets |

The marker registry (`Policy.rs:520-625`) is the closest thing Jet has to a
meta-type system: name, typed signature, legal sites, form, inheritance,
resolution. It is compiler-perfect and user-invisible. F2 asks it to publish
what it already knows.

## Scorecard (owner's four lenses + forward compatibility)

| Family | Clarity | Functionality | Magic | Explicit control | Forward-compat |
| --- | --- | --- | --- | --- | --- |
| Runtime types (struct/enum/distinct/unit family) | aligned | aligned | aligned | aligned | aligned |
| Traits + derives | aligned, name collisions aside (F10) | aligned | aligned | aligned | aligned |
| Value tags | **drift** — inert, undeclared accepted (F4) | **drift** — no checking | looks magic, does nothing | no strict mode | habit risk, not break risk |
| Typestate | aligned copy, **drift** namespace (F5) | aligned | aligned | states unaddressable | aligned |
| Marker plane | **drift** — phantom sig types (F2) | registry-only | aligned | not reflectable as types | additive fix |
| Effects/capabilities | aligned rows; leaves stringly (F8) | authority is lexical-only by design | aligned | can't factor helpers (F6) | additive |
| Dimensions/quantities | aligned syntax | **blocked** for user dims (F1) | magic for 5 dims only | no user dimension | **the one hard retrofit** |
| Typed text | aligned | aligned | aligned | heads closed — re-ratified today (F9) | additive, deliberately deferred |
| Control flow (if/loop/taskgroup) | aligned | value forms shipped | aligned | `TaskGroup` unnameable (F6), ranges unvalued (F7) | additive |

## Findings

Ranked by gain-if-typed times difficulty-of-doing-it-right-later. Each
finding leads with the fix.

### F1 — Open dimension algebra to user-declared dimensions

**Fix.** Let `#UnitFamily(Mass)` opt in to minting dimension `Mass` as a
fact type. Exponent vectors become open-length maps over declared base
dimensions. The 5 built-ins become ordinary prelude declarations of the same
kind. Derived dimensions stay structural — no name needed until a family
claims one. `Quantity<Dim, Kind>` bounds resolve against declared
dimensions. Nominal-by-default stays: a family gets algebra only when it
asks (currency stays out, as D-QUAL3 deliberately chose —
`Syntax.rs:258-260`).

**Evidence.** `PHYSICAL_DIMENSIONS: &[(&str, [i32; 3])]`
(`Syntax.rs:261-267`) hardcodes Length, Time, Speed, Area, Temperature over
a 3-axis exponent vector. Live probe: `#UnitFamily(Mass) { kg }` gets no
dimensional identity — `kg * meter` is plain E0109 ("make both sides the
same type") while `meter / second` derives `Speed`. SI needs 7 base
dimensions. Real programs need domain axes: pixels per point, tokens per
second, dollars per hour.

**Gains.** Functionality: the science and engineering domain stops being a
demo. Magic: `4kg * 3meter` composes the way `12meter / 3second` already
does. Control: experts declare base dimensions and derived identities.
Clarity: a dimension becomes a thing with a name a user wrote.

**Scope, honestly.** This amends D-SHAPE-QUANTITY1 and the D-QUAL3
nominal/dimensional split. The hard design question is cross-package
identity: two packages declare `Mass` — same dimension or error? Recommend:
dimensions are nominal to their declaring package; sharing goes through a
common dependency, the same rule types follow.

**Why first.** This is the one genuinely representational retrofit in the
audit. The `[i32; 3]` vector is about to be serialized into unit facts, API
snapshots, and diagnostics. Change the representation now, even if the
user-facing declaration ships later. Everything else in this report is a
surface respell; this one is a data migration if we wait.

**Vehicle:** ballot `D-DIMENSION-OPEN1`.

### F2 — Marker argument types become real prelude enums

**Fix.** Declare the marker argument sets as ordinary enums in a reserved
prelude namespace (working name `core.lang`): `TaintKind`,
`ObligationMode`, `InlineMode`, `Maturity`, `Layout`, `Target`, `ABI`,
`FfiLanguage`, `NamingCase`, `PolicySetting`. The registry emits these
declarations from one source of truth, with a drift-guard test. Rendering,
diagnostics, and reflection read them. `TypeInfo.markers` upgrades from
strings to typed argument records.

No new type category is needed. Marker arguments are already
comptime-evaluated values. An earlier draft said "comptime-only enum types";
that phrase collides with S26's rejected-forever list ("comptime types") and
is dropped. These are ordinary enums that happen to be consumed at compile
time — the same way any enum literal in a marker position already is.

**Evidence.** `Policy::APPLIED_RULES` (`Policy.rs:520-625`; 73 active rows,
19 retired) declares typed signatures over 14 parameter types no user can
name. E0930 renders them in type syntax (`marker_argument_shape_error`,
`Policy.rs:380-395`). `TypeInfo.markers` exposes markers as bare strings
(`Comptime/Reflect.rs:42-43`).

**Gains.** Clarity: `jet explain`, LSP hovers, E0930, and docs all point at
one real declaration per argument type. Functionality: derives and `fn
build` reflection consume marker arguments as typed values. Magic:
dot-literals (`.Track`, `.PII`) resolve by the same expected-type rule as
everywhere else (D-ENUMDOT1). Control: `jet explain rule Unsafe` shows a
real signature over real types. This finding also delivers the "prelude as
compiler vocabulary" move from the target shape.

**Vehicle:** ballot `D-RULEARG-TYPES1`.

### F3 — One fact model behind tags, states, taint kinds, effect leaves

**Fix.** Two layers, decided in one ballot.

*Layer 1 — internal, ships regardless of the surface pick.* One fact
registry and one subsumption algorithm behind tags, states, taint kinds, and
declared effect leaves. One reflection surface on `TypeInfo`. This is
refactoring plus reflection. It makes every later surface choice cheap.

*Layer 2 — surface, owner picks one of three honest options:*

- **(a) Facts are enums.** States, taint kinds, and effect leaves become
  comptime-consumed enum declarations. This reuses D-TAG1 enum-group
  machinery wholesale: trees, subtree subsumption, exhaustiveness, payload
  rules — all already ratified and shipped. One classification mechanism for
  runtime and compile time. The bold reading: `enum` is Jet's only
  classification kind, and "erased" is just what happens when only markers
  consume it.
- **(b) Tree-bodied tags.** `tag Sensitive { Pii, Secret { Credential } }`.
  Keeps the qualifier plane separate from runtime enums. Requires amending
  D-QUAL2's ratified "exactly two qualifier kinds" text.
- **(c) Status quo per mechanism**, internal unification only.

The audit recommends (a). It spends nothing on new mechanisms, retires
spellings instead of adding one, and gives fact trees every capability enum
groups already have. Option (b) is the fallback if the owner wants the
qualifier plane visually distinct. Option (c) is the floor.

**Evidence.** The four mechanisms, from behavior: `tag X` — erased
qualifier, flat set (`local_tags`). `state T { A, B }` — erased label set,
membership checked at calls (E0150). Taint kinds — closed 4-set, erased,
dataflow-propagated (D-TAINT1/2). Effect leaves — dotted names under closed
roots with ancestor subsumption; D-EFFTREE1 says verbatim this is "the same
rule as D-TAG1's tag-tree subtree matching learned once and reused"
(`syntax-decisions.md:1642-1643`). Note the honesty correction from review:
an earlier draft quoted "the state is an ordinary tag" as D-STATE1 law; that
sentence exists only as a non-normative comment in
`docs/reference/syntax-surface.jet:576`. The argument stands on the
mechanisms, not the quote.

**Gated gain, stated plainly.** User-declared taint kinds (`#Tainted(Gdpr)`)
only pay after user-defined sinks exist. A kind with no sink checks nothing —
the same inert-magic disease F4 attacks. Full IFC is ratified as deferred
post-Epoch 3; user taint kinds inherit that gate.

**Vehicle:** ballot `D-FACTMODEL1` (names the D-QUAL2 amendment if option
(b) wins; amends — not "clarifies" — any S26-adjacent language).

### F4 — Value tags: fix the bug, then decide if tags check anything

**Fix (a) — bug, card not ballot.** An undeclared tag in type position must
error, with did-you-mean. Live probe: `#TotallyUndeclaredTag Int` compiles
today with no `tag` declaration anywhere. That is a resolution hole against
D-QUAL2's model, not a design.

**Fix (b) — strict tags, ballot.** Live probe: a plain `String` flows into
`fn announce(title: #Reviewed String)` with no diagnostic. The law is
honest — spec.md says tags erase — but the surface reads like a check and
is not one. And it rhymes falsely: `#Tainted String` (checked dataflow,
E0721) and `#Reviewed String` (inert) share one spelling plane. Proposal:
opt-in strict tags. A strict tag rejects untagged values at tagged
positions. Entry goes through an audited blessing gate — a
`#Sanitizer`-shaped marker (working spelling `#Blesses(Reviewed)`) —
mirroring how taint exits the lattice. Not "any function returning the
tagged type blesses"; that is one-line laundering.

**The rule that keeps I8.** `distinct` when you want a new nominal identity
with its own methods. A strict tag when the underlying type must keep
flowing through every existing API and only entry is controlled. If the
owner finds that boundary too thin, drop (b) and the spec says plainly:
tags are declared documentation.

**Vehicle:** bug card (a); ballot `D-TAG-STRICT1` (b).

### F5 — States join the type's namespace and `TypeInfo`

**Fix.** Register the state set as comptime-only members of the type's
namespace, spelled `T.State.Off` (the qualified plane cannot collide with
synthesized members like D-PATCH1's `T.Patch`). Route
`#State`/`#Transition` argument resolution through ordinary name lookup.
Add `.states` and `.transitions` to `TypeInfo`. Erasure is unchanged;
states stay facts, never runtime values.

**Evidence.** Live probe: `Machine.Off` → E0107 "nothing named `Machine`
exists here" — even though `struct Machine` is in scope and `Machine.make()`
resolves fine. (That diagnostic wording is its own bug; carded separately.)
Meanwhile `#State(On) fn speed` fires E0150 with excellent teaching copy.
The checking is great. The names live nowhere.

**Gains.** Clarity: `#Transition(Machine.State.Off, Machine.State.On)` is an
ordinary lookup with did-you-mean. Functionality: docs and diagram tooling
(D-METADEPTH2 `fn build`) render the state machine from reflection. Magic:
LSP completes states inside `#State(…)`. Control: experts assert state facts
in tests.

**Vehicle:** ballot `D-STATE-NS1`; plus the E0107-copy bug card.

### F6 — `TaskGroup` becomes a nameable parameter type

**Fix.** Publish `TaskGroup` as a nameable parameter type so structured
concurrency can be factored into helpers: `fn fan_out(g: TaskGroup, xs:
[Int]) => [Int]`. Soundness rule, priced honestly: **spawns through a group
received as a parameter require owned captures** (`~x` moves/copies only, no
view captures). This extends D-DETACH1's owned-captures discipline from
detached tasks to non-local spawns. Jet has no lifetimes; the lexical rule
is what makes view captures safe today, so the parameter path must close
that hole itself. This is a real mechanism with real ballot weight, not a
convenience tweak.

**Evidence.** Live probe: `fn f(g: TaskGroup)` → E0119 "there's no type
called `TaskGroup`"; `g.task` outside the block → E1110. Helpers are
impossible today.

**Demoted half.** Typed capability handles (`Capability<FS.Read>`) were in
an earlier draft and are withdrawn for v1. Jet's ratified authority story is
ambient: effect rows (D-EFF1) plus lexical `#Grant` scopes (D-SCAP1) plus
package budgets (D-EFFBUDGET1). A helper that needs `FS.Read` already
declares `=[FS.Read]=>`. Capability-passing style only pays when authority
flows through data, which v1's no-stored-references design forecloses.
Recorded as a post-v1 design note. The v1 slice that survives: diagnostics
stop naming a `Capability` type that cannot exist.

**Vehicle:** ballot `D-TASKGROUP-PARAM1`.

### F7 — Ranges become values

**Fix.** Mint one nominal `Range` type over `Int` in core. Both spellings
construct it (`a..b`, `a..<b`); the value carries its end-exclusivity.
Fields and methods: `.start`, `.end`, `.contains(x)`. No second
`RangeExclusive` type. Existing syntactic positions keep their compile-time
treatment — loops still specialize to jumps; zero-cost lowering is a codegen
fact, not a surface fact. One accepted asymmetry, stated in the spec:
`distinct Int(0..10)` stays a literal-only position, because a runtime
`Range` flowing into a type declaration is value-indexed types — a different
language.

**Evidence.** `a..b` is legal in exactly five slots (loop source, arm head,
slice, view arg, distinct constraint) and nowhere else. No `Range` type
exists in the registry (S22/S72, D-RANGE-EXCL1; `AST/lvalues.rs:134-145`).
`bands :: [0..<10, 10..<100]` has no type to be.

**Gains.** Functionality: ranges as data — histogram bands, pagination,
validation bounds. Clarity: `1..10` is a value like any literal, one lesson
instead of five special positions. Control: expert APIs take `Range`
parameters instead of `lo`/`hi` pairs.

**Vehicle:** ballot `D-RANGE-VALUE1`.

### F8 — Optional effect-leaf declarations kill typo leaves

**Fix.** Allow leaf declarations under the reserved `effect` keyword:
`effect FS.Read`. Once any leaf is declared under a root, undeclared leaves
under that root become resolution errors with did-you-mean. Never-declared
roots keep today's freedom. The prelude declares the stdlib's own leaves, so
every user gets typo protection on day one without writing a declaration.

**Evidence.** Roots are a closed 10-set (D-EFF4/5 — right call; the beginner
story needs a small vocabulary). Leaves are freeform strings: `=[FS.Raed]=>`
silently mints a new leaf. pkg.jet budgets (D-EFFBUDGET1) compare the same
undeclared strings. `effect <Name>` is reserved and unminted — the slot
already exists.

**Vehicle:** ballot `D-EFFECT-DECL1`. Sequence before any typed
effect-argument surface (F6's post-v1 note depends on it).

### F9 — User typed-text heads: direction recorded, decision deferred

**No action now, by law and on the merits.** D-UNIFYLIT1=A — ratified by the
owner today (2026-07-28, card #1265) — says domain text is `SQL`/`HTML`/`Sh`
only. This audit does not relitigate it. The deferral is also right
technically: the hard half of a user head is not the validator (a pure
comptime value function fits S26) but *sink registration*. A `String`
reaching `SQL` is E0721 because SQL is a registered taint sink;
user-registered sinks are user-extensible security policy — the same
territory as the deferred post-Epoch 3 IFC decision.

**Recorded direction for that future ballot.** A user head should be a
library-declared distinct String type, a comptime validator, and declared
sink semantics. Types, not markers, not grammar. Until then, typed text
stands as the template every future "checked literal" request gets pointed
at — including the open regex ballot (#1283), which should land as a typed
head, not a new mechanism.

### F10 — Marker/trait name-collision law

**Fix.** Spec/teaching unification plus one rule. The unification: a
type-site PascalCase marker *is* a trait/derive reference — one namespace.
User derives already work this way (`derive T.Wire` → apply `#Wire`); say it
for the built-ins too (`#Comparable` derives `Comparable`). The rule for
future markers: a rule that derives trait X is spelled `#X`; a rule that is
not a derive may not take a trait's name. Optional renames if the owner
wants the two worst collisions gone: serde's `#Tag("type")` (three meanings
of "tag" today) and scheduled `#Task fn` vs `Task<T>`.

**Evidence.** `Comparable` is a trait (`effects_surface.rs:220`), a marker
(`Policy.rs:556`), and a capability bundle (D-CAPBUNDLE1). `#Codable` ≡
`Encode`+`Decode`. "Tag" = `tag` keyword, serde `#Tag`, D-TAG1 enum groups.

**Vehicle:** ballot `D-MARKER-NAME-HYGIENE1`.

### F11 — Spec law: constructs are never types; their artifacts always are

**Fix.** One spec paragraph, ungated (doc card): **a control construct is an
expression wherever it produces a value, and its runtime artifacts are
types; the construct itself never is.** This is the audit's answer to the
owner's hunch that loops, ifs, and taskgroups feel type-shaped. The hunch is
right; the type lives one step away from where it feels.

**Why the constructs themselves stay keywords — the full argument:**

1. *A construct-as-value already has a name in Jet.* A "loop value" would be
   a computation you can run later. Jet has that value: the lambda
   `() => { … }`. A `Loop` or `If` type would be a second spelling of a
   lambda — a direct I8 violation, and the reason `dyn`, `async`, and
   trailing-block sugar were all rejected or retired.
2. *The value-producing cases are already expressions.* Value-`if` unifies
   arm types. Yielding `loop` produces `List<T>`. `Stream<T>` and `yield`
   cover laziness. Nothing expressible is missing on the construct side —
   every gap the hunch points at is an *artifact* gap, and those are F6
   (`TaskGroup`) and F7 (`Range`).
3. *First-class control breaks the beginner promise.* If `loop` is a value,
   code can no longer be read top to bottom — control can leave, be stored,
   and come back (continuations). Every effect row, escape check, and taint
   rule would need to model suspended control. That is a research-language
   tax on priority 2 (beginner experience) for zero new programs.
4. *Performance is the quiet casualty.* Loops compile to jumps because they
   are not values. Reify the construct and either every loop pays an object
   representation, or the compiler maintains two lowering paths forever.
5. *The artifacts capture all the value.* What you actually want to store,
   pass, and inspect is the handle: the task, the group, the stream, the
   range, the yielded list. Typing the artifact gives every gain — data,
   composition, reflection — while the construct stays a zero-cost keyword.

Loop labels stay non-values too (D-LOOPLABEL3): a label names a jump target,
and jump targets as data is continuations again.

**Vehicle:** doc card (spec paragraph in `docs/spec/philosophy.md` or
`spec.md` control-flow section).

### F12 — What must stay non-types

Typing these adds ceremony with no classification semantics. They are
declaration modifiers (ontology P09), tooling metadata, or control transfer
(C18) — not facts about values:

- `#Unsafe` (audit gate — its power is *not* being composable or passable),
  `#Test`/`#Bench`, `#Meta`/`#Doc`, `#Off`/`#DebugOnly`, `#Inline`,
  `#Persist`/`#Track`, `#PubFile`/`#NoPrelude`, `#Target`.
- `defer close(^r)`, `break`/`next`/`return`, loop labels.
- The `#Policy` scope ladder (config, not classification) — though its
  values (`.Forbid`, limits) ride F2's typed-argument story.
- `#Pre`/`#Post` conditions stay expressions, not proposition types —
  refinement typing beyond D-REFINE1 is a different language.

## Forward-compatibility ledger

Graded on Jet's actual compatibility policy: greenfield pre-v1, "no compat",
retired forms get ordinary errors (D-TRAILBLOCK2 respelled shipped surface
yesterday). Surface respells are near-free by policy. The real retrofit axes
are *representation* (serialized data, snapshots) and *habit* (code and
teaching that accrete around a behavior).

| Closed today | Open-later path | Retrofit class |
| --- | --- | --- |
| Dimension exponent vector `[i32;3]` | F1: open-length over declared bases | **Representational — the one hard one**; vectors are about to be serialized into unit facts/snapshots |
| Inert tags accepted undeclared | F4: declaration + optional strict tags | Habit — respell is free, but idioms accrete; decide direction early |
| Marker arg types as registry strings | F2: real prelude enums | Additive |
| Taint kinds closed at 4 | F3: user fact kinds | Gated by law — deferred IFC owns user sinks; nothing to pre-build |
| Effect leaves undeclared | F8: optional declarations | Additive |
| `TaskGroup` lexical-only | F6: parameter + owned-captures rule | Additive (new rule, no respell) |
| Ranges syntactic | F7: `Range` value | Additive |
| Typed-text heads closed at 3 | F9: post-Epoch 3 with IFC | Additive; owner re-ratified the closure today |
| Untyped `Capability` handle | post-v1 design note | Moot in v1 (no stored references); do not pre-build |

## Celebrated (already type-shaped — preserve and copy)

- `#UnitFamily` minting real distinct types with literal suffixes — the
  template for every "marker with inputs" (D-QUAL3, D-UNITLIT1).
- `protocol` → generated typed handles over erased typestate — the model
  citizen for F3/F5/F6: declaration in, nominal types + erased facts out.
- Typed text as types, not markers (F9).
- D-TAG1 enum groups — a ratified tree-classification algebra the law
  already reuses for effects; F3's option (a) reuses it again.
- Trait-in-type-position with invisible boxing (S48) — dyn without `dyn`.
- `Task<T>`, `Stream<T>`, `Iter<T>`, value-`if`, yielding `loop` — control
  flow reified exactly at its value boundary (F11).
- The marker registry itself (D-MARKSIG1) — one schema, one call grammar,
  one diagnostic family; F2 asks it to publish what it already knows.
- Typestate error copy (E0150 live probe reads like a tutor).

## Review passes

**Peer review (factual, fresh context).** Re-read every cited span and
independently re-ran all five probes; all reproduced exactly. One major
finding: a draft attributed a fabricated quote ("the state is an ordinary
`tag`") to D-STATE1 — the sentence exists only as a non-normative comment
in `docs/reference/syntax-surface.jet:576`. Fixed in F3. Minor corrections
applied: marker count 73 active + 19 retired (was "~60"); struct/enum
milestone is M3 not M2; `PolicySetting` is rendered signature text with no
Rust type behind it; the typed-text deferral is unnamed law inside
D-TYPEDTEXT2. All proposed ballot IDs verified collision-free against
`docs/` and the Tower board.

**Adversarial review (invariants and taste, fresh context).** Fourteen
attack findings; the material resolutions: F3's evidence rebuilt after the
misquote and the D-TAG1 conflation; the enum-group alternative promoted to
a first-class option. "Fact types" as a new user kind risked being the
fifth spelling — F3 now leads with internal unification and names the
D-QUAL2 amendment the tree-tag option needs. User taint kinds gated on
deferred IFC. F2's "comptime-only enum types" respelled to ordinary prelude
enums — no S26 contact. Typed `Capability<E>` demoted to a post-v1 note
(duplicates the ambient authority mechanism; v1 forecloses the style that
pays for it). F6 now carries an owned-captures rule extending D-DETACH1 and
is priced as a real mechanism. F9 rewritten to honor D-UNIFYLIT1=A,
ratified the same day. The compat ledger re-graded on representation and
habit instead of break-cost arithmetic the greenfield policy rejects. F5
collision law, F1 cross-package identity, and F7's representation fork all
resolved in the findings. F1 re-ranked above F3; `TypeInfo` added to the
kind zoo.

**Pay-up-front review (owner's standard).** Three places to spend now, on
"do it right the first time" grounds: (1) F1's dimension representation —
the only finding where waiting means migrating serialized data instead of
respelling surface; (2) F3's surface direction, decided once even if only
the internal unification ships first, so F4/F5/F8 are not designed against
divergent futures; (3) F4's strict-tag question, decided before inert-tag
idioms accrete. One place deliberately *not* to spend: typed capability
handles — v1's memory model forecloses the style that justifies them, and
building the generality now would speculate on a post-v1 design. That is
its own way of painting the corner.

## Next actions

Cards and ballots are live on the board (created 2026-07-28, this audit):

| Card | What | Ballot |
| --- | --- | --- |
| #1289 | Bug: undeclared value tag accepted in type position (F4a) | — |
| #1290 | Bug: E0107 says a type does not exist when it does (F5) | — |
| #1291 | Doc: F11 boundary-rule spec paragraph | — |
| #1292 | Open dimension algebra (F1) | `D-DIMENSION-OPEN1` |
| #1293 | Marker argument types as prelude enums (F2) | `D-RULEARG-TYPES1` |
| #1294 | One fact model (F3) | `D-FACTMODEL1` |
| #1295 | Strict tags with audited blessing (F4b) | `D-TAG-STRICT1` |
| #1296 | States join the namespace + `TypeInfo` (F5) | `D-STATE-NS1` |
| #1297 | Nameable `TaskGroup` parameters (F6) | `D-TASKGROUP-PARAM1` |
| #1298 | Ranges as values (F7) | `D-RANGE-VALUE1` |
| #1299 | Optional effect-leaf declarations (F8) | `D-EFFECT-DECL1` |
| #1300 | Marker/trait name-collision law (F10) | `D-MARKER-NAME-HYGIENE1` |

Notes, no action: user typed-text heads ride the IFC decision (F9); typed
capability handles are a post-v1 design note (F6).
