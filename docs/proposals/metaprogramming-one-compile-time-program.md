# Metaprogramming — one compile-time program

2026-08-06. A first-principles rebuild of everything Jet does before your
program runs. Status: awaiting owner review. Ballot rows at the end. Nothing
here is implemented yet. Every line marked "proposed" is not ratified.

## Executive summary

**The finding.** Jet's compile-time design is right on paper. Nine peer
language families each fragmented here, and Jet refused the exact mechanisms
that broke them. The problem is not the design. The problem is that **half of
Jet's compile-time program is written in Rust, and that half cannot be read,
extended, or reflected.**

A user writes `#Known`, a derive body, and `fn build` in Jet. The compiler
writes the marker table, the effect roots, the dimension table, the DSL block
list, the derive engines, and the diagnostic registry in Rust. Both halves do
the same job. Only one of them is a program you can see.

**The one idea.** Compile time is one Jet program — including the parts now
written in Rust.

**Why now.** You ratified the same law twice, in two areas, one day apart, and
neither ruling names the other:

- **Marker law zero** (`D-VERDICT-1455-1`, 2026-08-05): a marker exists if and
  only if it is a registry row. Every row must be parsed, validated,
  formatted, highlighted, and reflected from that one row.
- **The plane law** (`D-TYPE2-PLANE1=A`, 2026-08-06): every fact plane is
  registered, nameable, reflectable, and readable.

Both say: a thing the compiler knows must be declared once, and that
declaration must be readable. Neither says where the declaration lives. Today
it lives in a hand-written Rust table. In the last two days, four proposals
each minted a fourth one.

**What this buys.** One declaration form replaces six closed Rust tables. One
sigil replaces four spellings of "compile time". Derives stop being escaped
strings and become the implementation they describe. Users gain rules, effect
roots, dimensions, and text DSL blocks — surfaces that today need a compiler
patch — and the standard sets ship filled, starting with the `Mass` dimension
Jet does not have. Beginners see no change at all: `#Codable` still reads
`#Codable` when you write it, and most of the time you no longer need to.

**The most visible change.** `$` becomes the one mark for compile time, in every
position:

```jet
// today                          // proposed
#Known limit :: 1000              $limit :: 1000
#Known if debug { … }             $if debug { … }
#Known { … }                      $ { … }
T.reflect().name                  T.$name
T.$layout                         T.$layout        (already ratified)
```

One question comes with it, and `D-META-STAGE1` asks it: does the mark belong to
the **binding** or to the **name**? If it belongs to the binding, uses stay plain
and today's rule for carrying a value out of a compile-time block survives. If it
belongs to the name, it is written at every mention and that rule has nothing
left to say, because the name is the same name everywhere. Only the second
reading actually deletes a concept.

**What the ballots ask.** Thirteen direction ballots on card #1508. One adopts
the model. Eleven decide a surface change each. One is a naming menu. Any subset
works alone. Six ratified rulings are amended by name: `D-DSLBLOCK1=A`,
`D-CTCORE1`, `D-VERDICT-1308-1`, `D-VERDICT-1308-2`, `D-CTMARKER1`, and
`D-AUTODERIVE1=E`. No other ratified decision is touched.

**What does not change.** The walls hold. No macros. No AST mutation. No
message loop. No comptime types. `S26` and `D-METAMUTATE1` survive — and this
model gives them a reason instead of a list.

---

## Glossary

- **Compile-time program** — every piece of Jet that runs before your program
  runs: constant folding, marker rules, derives, generic modules, and the
  build entry.
- **Declaration** — a named, typed thing written in Jet source. A `struct` is
  a declaration. This proposal makes a marker row one too.
- **Registration** — the act that makes a name legal. Today it is a row in a
  Rust table. Proposed: a declaration in Jet.
- **Contribution** — what compile-time code hands back: a value, a fact, some
  source code, or a diagnostic.
- **Stage** — when code runs. Jet has two: compile time and run time. `$` marks
  a name as belonging to the first.
- **Fact** — one named piece of knowledge the compiler holds about a target.
  `#Inline` records a fact. So does a unit dimension.
- **Target** — the code a rule applies to: a type, a field, a function, a
  block, a file.
- **The three verbs** — Read, Compute, Add. See "The model".

---

## The one idea

> **Compile time is one Jet program — including the parts now written in
> Rust.**

Today Jet runs two compile-time programs. One is written by you, in Jet. The
other is written by the compiler, in Rust. They do the same work and they
cannot see each other.

**The beginner story.** Nothing changes. You write `#Codable` above a struct
and encoding works. You write `x :: 2 * 3` and it folds. You never learn that
a marker is a declaration, because you never need to.

**The expert story.** Every rule the compiler applies to your code is a Jet
declaration you can open, read, copy, and write your own version of. The
marker table is Prelude source. The effect roots are declarations. Your team's
`#Retry` marker is the same kind of thing as the compiler's `#Inline`, checked
by the same pipeline, shown by the same tools.

---

## Evidence — the shadow systems

Every row is one job done more than once. File and line proof.

| # | Job | Mechanism A | Mechanism B | Defect |
|---|---|---|---|---|
| 1 | Register a compile-time rule | `Policy::APPLIED_RULES`, 99 rows in Rust (`crates/jet-foundation/src/Policy.rs:833`) | `known_derive_names` — user `derive T.X {}` names join the marker vocabulary (`crates/jet-sema/src/Sema/Bundle.rs:1234-1239`) | Two registries for one vocabulary. One is closed, one is open. |
| 2 | Generate code from a type | User derive: emits a **string**, re-lexed and re-parsed (`Bundle.rs:1654-1697`, E2710 on failure) | Built-in derive: hand-written Rust codegen (`crates/jet-codegen/src/Codegen/Items.rs:1158-1199`) | Two derive engines. Only one is writable by users. |
| 3 | Generate code from a type (again) | Serde via Jet-source template (`crates/jet-sema/src/Sema/Registration/Serde.rs:3-49`) | Serde via raw Rust strings for enums and unions (`Items.rs:1030-1090`) | The code admits a half-finished migration at `Items.rs:1811-1815`. |
| 4 | Splice a compile-time value | `Expr::ComptimeSplice`, a real AST node (`crates/jet-foundation/src/AST/expressions.rs:624-633`) | `apply_dollar_splices` — `$name` inside an `emit()` string, handled as string interpolation | One sigil, two implementations. |
| 5 | Instantiate code with parameters | Generic modules: real Jet, substituted (`crates/jet-sema/src/Sema/Bundle/GenericModules.rs:1-51`) | Derives: escaped string templates | Same job. Only the first is checkable before use. |
| 6 | Instantiate code with parameters (again) | Generic functions, specialized in codegen (`crates/jet-codegen/src/Codegen/TIR/mod.rs:816-902`) | Variadic-bound synthesis, a third AST-synthesis pass (`crates/jet-codegen/src/Codegen/VariadicBound.rs:1-30`) | Three instantiation engines in two crates. |
| 7 | Prove a whole-program property | Effect inference, a worklist fixpoint (`crates/jet-sema/src/Sema/Effects.rs:750-787`); run-time purity, a hand-rolled recursive walk raising E3401 (`crates/jet-sema/src/Sema/Purity.rs:765-1058`) | Comptime purity, a third checker raising E0951 (`crates/jet-comptime/src/Comptime/Purity.rs`, `Comptime/Diagnostics.rs:104-121`) | Three transitive-closure engines, two diagnostic families for one rule. Also flagged by `authority-one-model.md:118`. |
| 8 | Evaluate a compile-time field | Shared comptime evaluator (`crates/jet-env-model/src/ModuleEval/Eval.rs:897-907`) | `packages:` alone, by raw text slicing (`Eval.rs:1239-1300`) | One field carved out, with the carve-out documented inline. |
| 9 | Hold a diagnostic's text | Inline Rust strings (`Purity.rs:9-42`) | A markdown table parsed at build time (`crates/jet-cli/src/Explain.rs:1-33`) | Two sources, kept in sync by a manual checklist. |
| 10 | Name a compile-time constant | Comptime interpreter, full expressions | Array size `[T#N]`: literal or bare name only, E0963 otherwise (`crates/jet-parser/src/Parser/Types.rs:447-478`) | `[T#(N*2)]` is illegal. The evaluator exists and is not called. |
| 11 | Name a compile-time constant (again) | Comptime interpreter | Enum discriminant: literal integer only, E0035 (`crates/jet-parser/src/Parser/Items/enums_traits.rs:124-148`) | Same gap, second site. |
| 12 | Register a fact kind | Marker registry | Type plane registry (`D-TYPE2-PLANE1`), rights tree (`authority-one-model.md:475`), build fact plane (`build-config-one-plane.md:553`) | Four "one table, one law" registries minted within 48 hours, each citing the others. |

Four more defects with no second mechanism, only a closed door:

- **Effect roots are closed.** 28 entries (`crates/jet-foundation/src/Facts.rs:24`).
  `D-FFI-PY1=A` ratified a `Py` root. It was never added. A user cannot add one.
- **Dimensions are closed.** Length, Time, Speed, Area, Temperature. A user
  cannot mint `Mass`.
- **DSL blocks are closed.** Two entries, `SQL` and `HTML`
  (`crates/jet-foundation/src/Syntax/core_surface.rs:403-410`).
- **Reflection returns strings, not values.** `marker_names` maps every marker
  to a bare string (`crates/jet-comptime/src/Comptime/Reflect.rs:151-152`), so a
  derive reads `"Redact"` and not the typed row. Marker *arguments* already
  resolve to typed values (`:163-168`), so the two halves of one row disagree.

And one promise with no code: `jet inspect expand` has four lenses —
`inline`, `memory`, `web`, `layout` (`Source/CmdExpand.rs:32-57`). There is no
derive lens. The plan claims "hover a derive and see its emitted fragment" as
Jet's moat (`docs/plans/epoch-5/metaprogramming.md:490`). You cannot.

---

## The model

### Three verbs

Every compile-time mechanism in Jet is a bundle of three verbs.

| Verb | Means | Today's mechanisms |
|---|---|---|
| **Read** | look at something the compiler knows | `T.reflect()`, `ProgramInfo`, `embed`, `find`, `fetch` |
| **Compute** | run Jet code on what you read | `#Known` bindings, `#Known if`, `#Known { }`, implicit folding |
| **Add** | hand back a contribution | derives, generated modules, generic-module instances, facts set by `#Inline`, `b.error` |

Every current mechanism is one bundle:

- `#Codable` is Read(a type) + Add(code).
- `module cache<K, capacity: Int>` is Read(parameters) + Add(code).
- A build policy rule is Read(the program) + Add(a diagnostic).
- `#Inline` is Add(a fact).
- `#Known limit :: 1000` is Compute.
- `fn build` is all three.

The fragmentation has one shape: **each bundle got its own mechanism instead
of the verbs composing.**

### Two laws

**Law 1 — Add only.** Compile-time code never changes what you wrote. There is
no fourth verb.

**Law 2 — Knowledge erases.** A fact the compiler proves costs nothing at run
time.

These are not new rules. They are the rules you already ratified, stated once
instead of nine times. Each ratified decision below is a theorem of one of them:

| Ratified rule | Falls out of |
|---|---|
| `D-METAMUTATE1=A` — no AST mutation, no message loop, no user macros | Law 1. Each banned feature is a Change. |
| `D-BUILDGEN1=A` — generation is additive only | Law 1 |
| `D-METADEPTH1/2` — enforcement is read-only | Law 1 |
| `D-FRONTENDAPI1=A` — no AST mutation enters compilation | Law 1 |
| `S26` — comptime never creates or selects a type, never affects dispatch | Law 1. A comptime *value* is not a contribution that can become a type. Source text is, and it goes through sema like any other source. |
| `D-TYPE2-EXACT1=A` — the conservation law for precision | Law 2 |
| `D-TYPE2-PLANE1=A` — facts are registered, nameable, reflectable | Law 2 |
| `D-VERDICT-1455-1` — a marker exists only as a registry row | Law 2 |

`D-METAMUTATE1` is the important one. Today it reads as a list of banned
features. Under Law 1 it is one sentence: **Change is not a verb.** The list
stops being arbitrary. Anyone can check a new proposal against it without
reading the history.

### The connections

Spelled out, because each one deletes a mechanism:

1. **A marker and a derive are the same thing.** `#Codable` is a marker whose
   rule adds code. `#Inline` is a marker whose rule adds a fact. The marker
   proposal already saw this: "Markers already *are* Jet's derive mechanism"
   (`marker-plane-first-principles.md:86`).
2. **A registry row and a declaration are the same thing.** The marker
   proposal already saw this too: "each registry row is already shaped like a
   declaration… rows can become `core.lang` declarations without changing any
   spelling" (`:318-322`). This proposal walks through that door.
3. **A generic module and a derive are the same thing.** Both are Jet code with
   holes, instantiated from known values. One is written as code. The other is
   written as an escaped string.
4. **`b.generate(name, source)` is a third spelling of the same thing.**
5. **Marker law zero and the plane law are one law.** Both say: registered,
   nameable, reflectable. One covers markers, one covers type facts. Nothing
   makes them different.
6. **The comptime effect tiers and the runtime effect model are one model.**
   Tier 0, 1, 2 (`D-CTEFFECT1`) is what `=[FS]=>` already says at run time.
7. **Reading a constant is one job.** The comptime evaluator exists. Array
   sizes and enum discriminants do not call it.
8. **A splice and a compiler fact are the same reading.** `$name` weaves a
   compile-time value into code. `T.$layout` reads a compile-time fact. Both
   say "this is known before the program runs". One sigil already covers both,
   and `#Known` is the odd one out.

---

## The surface

This is the point of the proposal. Every change below makes code shorter or
clearer to read. Each is marked **ratified**, **amended**, or **new**.

### S1 — `$` is the one compile-time sigil (amended)

Today, compile time is spelled four ways. A binding is `#Known x :: 5`. A branch
is `#Known if`. A splice is `$name`. A compiler fact is `T.$layout`. Two of
those already use `$`.

Proposed: **`$` means compile time, in every position.**

```jet
// today                                  // proposed
#Known limit :: 1000                      $limit :: 1000
#Known if debug { … } else { … }          $if debug { … } else { … }
#Known { … }                              $ { … }
T.$layout                                 T.$layout
info :: T.reflect(); info.name            T.$name
```

**What the mark means at a binder.** `$x :: expr` does not claim that `x` is
already known. It puts the requirement on `expr`: compute this before the
program runs, or stop the build. The name is the result. A binder introduces and
a use consumes, so there is no circle — the obligation is discharged in one place
and relied on in the others.

**What it means at a use is the open question.** `D-META-STAGE1` asks it
directly, because the answer decides whether this change deletes a concept or
only renames one:

- **The mark belongs to the binding.** Uses stay plain, as they are today:
  `$limit :: 1000` then `print("{limit}")`. Today's rule for carrying a value
  out of a compile-time block survives unchanged, so `$ratio` still means
  "carry this out". Smallest migration, two rules remain.
- **The mark belongs to the name.** It is written at every mention:
  `$limit :: 1000` then `print("{$limit}")`. Now there is nothing to carry,
  because the name is the same name inside and outside the block, and
  `D-CTMARKER1` retires rather than being generalized. One rule, slightly
  noisier code.

Today's ratified behaviour is the first: `examples/features/comptime/comptime_block.jet`
uses plain `limit` for a module-level binding and `$ratio` for one bound inside
a `#Known { }` block. The second reading is the one that removes a rule.

Two things hold either way:

1. **`#Known` leaves the marker plane.** A marker applies a rule to a target.
   A stage is not a rule about a target, which is why `#Known` needed four legal
   sites and a `Prefix` form nothing else used. `#` keeps exactly two readings
   and gets cleaner, not busier.
2. **Compiler facts already read this way.** `T.$layout` is ratified
   (`D-LAYOUT-FACTS1=B`). `T.$name` and `T.$fields` join it instead of needing a
   separate reflection call.

Amends `D-VERDICT-1308-1` and `D-VERDICT-1308-2` (which made `#Known` and
`#Known if` the spellings). Whether `D-CTMARKER1` is generalized or retired
depends on the answer above. Implicit folding is untouched: an ordinary binding
still folds when it can, and still says nothing when it cannot.

### S2 — Derive bodies are the implementation (new)

Today, from `examples/features/serde/user_derive.jet`:

```jet
derive T.DebugText {
    info :: T.reflect()
    tname :: info.name
    emit("""
impl $tname {{
    fn debug_string(self) => String {{
        return "$tname"
    }}
}}
""")
}
```

Proposed:

```jet
derive T.DebugText {
    fn debug_string(self) => String = T.$name
}
```

The header already says which trait, for which type. The body is simply the
members. There is no nested `impl`, because there is nothing left to name. Seven
lines become three, and every one of them is real Jet that the parser checks
when you write it.

The `derive T.Trait` spelling is unchanged, as `D-METADERIVE1` requires. Only
the body changes: members instead of a string.

When a derive must generate one member per field, compile-time control flow does
it, with the same `$` from S1:

```jet
derive T.Describe {
    fn describe(self) => String {
        parts := [String].{}
        $loop f, T.$fields {
            parts.add("{f.$name}={self.$f}")
        }
        return parts.join(", ")
    }
}
```

Three readings of one sigil, and they agree. `T.$fields` is a compiler fact.
`f.$name` is a compiler fact on the field handle the loop bound. `self.$f`
reads the field that handle names. In every case the name after `$` belongs to
compile time — which is the same rule as `$limit` and the same rule as the
ratified `T.$layout`.

`$loop` is the ratified `loop` verb at compile time, not a second iteration
mechanism. It is the same word, marked by the same sigil, and it is the reason
this proposal needs no `for` keyword.

### S3 — Rule rows are declarations (new, fulfills `D-VERDICT-1455-1`)

Today a marker row is a Rust macro call at `crates/jet-foundation/src/Policy.rs:854`:

```rust
rule!("Inline", sig!(mode: InlineMode = .Hint),
      &[RuleSite::Function, RuleSite::Method, RuleSite::Constant]);
```

Proposed, in Prelude Jet source. A rule declaration is an ordinary Jet
declaration with named parameters. The rule's own arguments and the facts about
the rule share one list, and `$` says which is which:

```jet
marker Inline(mode: InlineMode = .Hint,
              $sites: [.Function, .Method, .Constant])

marker Unsafe(reason: String, obligations: ObligationMode = .None,
              $sites: [.Function, .Method, .Block, .Operation])

marker Pre(condition: Any, message: String,
           $sites: [.Function, .Method], $repeatable: true)
```

`mode`, `reason`, and `condition` are what a user writes at the use site.
`$sites` and `$repeatable` are facts about the rule, so they carry the
compile-time mark from S1. This is the same rule as `$limit` and `T.$layout`,
applied to a parameter list.

Three things fall out:

- **No new keywords.** `marker` is the only word introduced, and `D-META-NAME1`
  picks it. There is no `on`, no `repeatable`, no trailing grammar.
- **The site set is data.** `$sites` is a list of enum members, so a program can
  read it. `Site` is already a real compiler enum (`RuleSite`, 18 members),
  published as a Prelude enum by `D-RULEARG-TYPES1=A`.
- **The row is open-ended.** A new fact about rules is a new named optional
  parameter, not a new clause in a declaration grammar.

The user surface does not change one character. `#Inline` is still `#Inline`.
What changes is that the row is readable, reflectable, and — with S4 — writable.

This is what law zero asked for. "A marker exists if and only if it is a registry
row" becomes structurally true: there is one parse path because a marker name
resolves to a declaration, the same way a function name does. The ~30
hand-parsed markers cannot return, because there is no second way to write one.

`D-RULEARG-TYPES1=A` generates fourteen marker-argument enums from this table.
It reads declarations instead, and generates exactly the same fourteen.

### S4 — Users may declare rules (new)

A rule with no body records a fact. A rule with a body contributes items or
rejects the build. That is the whole difference between `#Inline` and
`#Codable`.

```jet
// records a fact; no body needed
marker Audited(owner: String, $sites: [.Function])

// rejects the build; the body is ordinary Jet
marker NeedsTimeout($sites: [.Function]) {
    if not target.$params.has("timeout") {
        reject(code: "ORG_NET01",
            what: "network function has no timeout",
            why: "company services must fail predictably",
            fix: "add a `timeout` parameter")
    }
}
```

`if` and `not` are ratified. `reject` is a Prelude function whose signature
requires code, what, why, and fix, so I4 quality holds by construction — the
same shape `b.error` already has. A rule body that contributes items reads
exactly like a derive body (S2). No verb is introduced for either.

The four checks from `D-MARK-FORM1=A` — vocabulary, site, signature, duplicates
— run unchanged. They read a declaration instead of a Rust row.

### S5 — Generic modules stop leaking mangled names (new)

Today, from `examples/features/modules/generic_modules.jet`:

```jet
module three_ints = fixed_buffer<Int, 3>
buffer :: M5Three4IntsBuffer.{items: fixed}     // a real line in a shipped example
```

Proposed:

```jet
module three_ints = fixed_buffer<Int, 3>
buffer :: three_ints.Buffer.{items: fixed}
```

The instance is a module. Its types are its members. A user should never type a
name the compiler minted.

### S6 — `b.generate` uses the derive body form (amended)

Today, from `examples/features/tooling/programmable_build/main.jet`:

```jet
b.generate("build_message", "fn generated_build_message() => String {{ ... }}")?
```

Proposed:

```jet
b.generate("build_message") {
    fn generated_build_message() => String = $stamp
}?
```

Same block of items as a derive body, in the same `$` vocabulary. One way to
write generated code at every rung, and no new kind of value.

### S7 — Compile-time expressions where constants are legal (new)

```jet
// today: E0963 and E0035
$LANES :: 8
$BASE  :: 100

buffer: [Int#($LANES * 2)]              // proposed
enum Code { First = $BASE + 1 }         // proposed
```

The evaluator already exists. Two parsers refuse to call it. This is a
capability gained by deleting code.

### S8 — One effect model for compile-time code (amended)

Compile-time purity has its own allowed-call list (`D-CTCORE1`), its own tier
names, and its own diagnostics (E0951), beside the run-time effect system with
its own (E3401). Proposed: compile-time code declares effects the same way
run-time code does.

```jet
fn load_schema() =[FS]=> String { … }     // proposed: same syntax at both stages
```

Tier 0, 1, and 2 stay exactly as ratified in `D-CTEFFECT1`. They stop being a
separate vocabulary and become what the effect set already says.

### S9 — Open the closed tables, and fill them (new)

Declarations, not Rust rows:

```jet
effect Py                                // proposed — D-FFI-PY1=A ratified this, it never shipped
dimension Mass                           // proposed
marker GraphQL<Row>($sites: [.Block]) { … }   // proposed — see D-META-DSL1
```

Opening a table is not enough. **The standard sets ship filled.** Jet has five
dimensions today — Length, Time, Speed, Area, Temperature — and no `Mass`. A
language that cannot weigh anything is not finished. The Prelude ships the seven
SI base dimensions and the common derived ones, as declarations, so a user opens
the table only for a dimension nobody standardised. This lands on
`D-TYPE2-MEASURE1=A`, which already ratified one measure substrate.

The marker type parameter on `GraphQL<Row>` is ratified law: `D-SQL-ARG1=B`
ratified angle brackets and gave markers a type-parameter feature.

### S10 — `jet inspect expand` gains a derive lens (new, no ballot)

The lens table takes one more row. `Source/CmdExpand.rs:22-24` says adding a lens
is "one row here… never new commands". Then §13's promise becomes true: you can
see what a derive generated.

---

## Beginner magic, expert control

The model has to serve someone who has never heard the word "compile time" and
someone who audits a build for a bank. It does that with one ladder. The bottom
rung is what you get for free. Every rung above it is opt-in and invisible until
you need it.

Two rules govern the whole ladder. **Anything the compiler can decide correctly
on its own is a default you refuse, not a feature you request.** And **every
default owes you three things**: a way to refuse it on one type, a way to refuse
it project-wide, and a way to see what it did.

**Rung 0 — you type nothing.** Anything the compiler can derive from a type's
shape, it derives (`D-META-AUTO1`). Ordinary immutable bindings fold at compile
time with no marker, and nothing is reported when they cannot
(`D-VERDICT-1308-1`). A beginner gets printing, comparison, encoding, and
constant folding without knowing any of it happened.

```jet
User :: struct { name: String, visits: Int }

fn run() {
    u :: User.{name: "ada", visits: 3}
    print(u)                     // nobody asked for it
    print(json.encode(u))        // nobody asked for this either
}
```

**Rung 1 — you refuse one.** The exclamation mark rejects one
compiler-generated implementation on one type. Nothing is forced.

```jet
#!Codable
Internal :: struct { cursor: Int }
```

**Rung 2 — you refuse it everywhere.** A project switches any default off in
its settings (`D-AUTODERIVE-SYNTAX1=D`). One line, one place.

```jet
// package.jet
settings: { auto_derive: [!Codable] }
```

**Rung 3 — you ask for what cannot be guessed.** A trait the compiler cannot
derive from a shape stays opt-in, because there is nothing to derive. This is
the only place a marker is required rather than offered.

**Rung 4 — you require the answer now.** An ordinary binding folds at compile
time when it can, and stays a run-time value when it cannot. Neither outcome is
an error. Writing `$` says the value must be known before the program runs, so
failing to compute it stops the build instead of passing quietly.

```jet
$limit :: parse_budget(embed_file("budget.txt"))
```

**Rung 5 — you see what happened.** `jet inspect expand` prints the generated
code, the folded value, and the layout. Generated modules are real files under
`.jet/generated/` you can open, diff, and set a breakpoint in
(`D-BUILDGEN1=A`).

**Rung 6 — you write the rule.** A declaration, checked by the same four checks
as every compiler rule.

**Rung 7 — you control the authority.** Effects declared on the code, permitted
by the package, capped by the workspace (`D-BUILDSCOPE1=A`). Every compile-time
read recorded in `.jet/lock`. `jet inspect audit-effects` reads the whole
dependency graph statically, running nothing.

The rungs are strictly ordered, and no rung is a prerequisite for the one below
it. A beginner never leaves rung 0. Nothing at rung 6 or 7 changes what rung 0
does.

---

## What it looks like

### Beginner — unchanged

```jet
#Codable
User :: struct {
    name: String
    visits: Int
}

fn run() {
    u :: User.{name: "ada", visits: 3}
    print(json.encode(u))
}
```

No line of this changes. That is the test S3 and S4 must pass.

### The middle — a team rule that lives on the code it governs

Today this is impossible without patching the compiler, or it lives in a build
entry that only runs at the root, far from the function it judges.

```jet
// proposed
marker NeedsTimeout($sites: [.Function]) {
    if not target.$params.has("timeout") {
        reject(code: "ORG_NET01",
            what: "network function has no timeout",
            why: "company services must fail predictably",
            fix: "add a `timeout` parameter")
    }
}

#NeedsTimeout
fn charge(card: Card, amount: Money, timeout: Duration) => Receipt ? { … }
```

### A derive that reads a type's shape

```jet
// today
derive T.Wire {
    info :: T.reflect()
    tname :: info.name
    emit("""
impl $tname {{
    fn tag(self) => String {{ return "$tname" }}
}}
""")
}

// proposed
derive T.Describe {
    fn describe(self) => String {
        parts := [String].{}
        $loop f, T.$fields {
            parts.add("{f.$name}={self.$f}")
        }
        return parts.join(", ")
    }
}
```

### Expert — the same verbs at build scope

```jet
// proposed spelling of a shipped mechanism
fn build(b: BuildContext) =[FS]=> BuildPlan ? {
    schema :: b.embed("schema/app.sql")?

    b.generate("db_client") {
        module db_client {
            $loop table, parse_tables(schema) {
                pub fn $table.name(id: Int) => $table.row_type { … }
            }
        }
    }?

    loop f, b.program.functions() {
        if f.effects.has("Net") and not f.params.has("timeout") {
            b.error(f.span, code: "ORG_NET01",
                what: "network function has no timeout",
                why: "company services must fail predictably",
                fix: "add a `timeout` parameter")
        }
    }

    app :: b.add_executable("ledger", sources: ["src/main.jet"],
                            generated: ["db_client"])
    return b.plan(default: app)
}
```

The generated block is now code the compiler checks and the editor understands.
Today the same work is a helper function that returns `String`, built by joining
text.

Note the two loops. `$loop` runs at compile time and emits one function per
table. `loop` runs when the build runs and reads the checked program. Same verb,
and the sigil is the only thing that says which stage you are in.

## What this unlocks

- **Teams get org rules that travel with the code.** Today a policy rule only
  runs at the root build entry. A marker attaches at the site it governs.
- **Domain libraries get first-class rules.** A test framework, an ORM, or a
  game engine can ship `#Entity` or `#Component` without a compiler patch and
  without a macro system.
- **Physics and engineering get `Mass` on day one.** The table opens *and* the
  seven SI base dimensions ship in the Prelude. A language with `Speed` and no
  `Mass` cannot state force, energy, or pressure.
- **FFI gets its promised roots.** `D-FFI-PY1=A` ships as a declaration.
- **One thing to learn about compile time.** `$` in every position, instead of
  a marker for bindings, a second marker for branches, a sigil for splices, and
  a method call for reflection.
- **Text DSLs open safely.** `#GraphQL`, `#Regex`, `#Shader` become
  declarations that read a text region and add code. Jet's grammar does not
  move (see `D-META-DSL1`).
- **Simulation and embedded get computed sizes.** `[T#(LANES * 2)]`.
- **Every tool improves at once.** The formatter stops rebuilding markers from
  16 booleans. Tree-sitter generates from declarations. Reflection stops
  returning an empty list for methods.

---

## What stays

Each item earns its place. None is kept because it shipped.

- **No macros, no AST mutation, no message loop.** `D-METAMUTATE1=A` holds,
  restated as Law 1. Nine peer languages prove the cost of the alternative.
- **`S26` — comptime never creates, parameterizes, or selects a type, and never
  affects dispatch.** Unchanged. Compile-time *values* stay values. Generated
  *source* becomes types the same way hand-written source does, through sema,
  exactly as `D-CTCODEGEN1` already rules.
- **`D-CTCODEGEN1` — generated source re-enters lexer, parser, and sema.** This
  is the one channel between stages. Racket, MetaOCaml, and Scala 3 each
  arrived at the same discipline.
- **`D-BUILDGEN1=A` — materialized, additive, addressed.** Generated code is a
  real file you can open. Swift shipped the same conclusion in 2023.
- **The marker sigil map.** `#` keeps exactly two readings, and gets cleaner:
  `#Known` leaves for `$`, so `#` means only "a rule applies to this target".
  No new sigil is minted — `$` already exists and already carries this meaning
  in `T.$layout`.
- **The one placement law** (`D-MARK-FORM1=A`) and grouping
  (`D-MARK-STACK1=A`). Zero spelling changes for any marker that stays.
- **Implicit folding** (`D-VERDICT-1308-1`). An ordinary binding still folds
.
- **`D-SQL-ARG1=B`** — markers carry type parameters. `#GraphQL<Row>` in S9 is
  that ratified feature, not a new one.
- **Tier 0, 1, 2** (`D-CTEFFECT1`). The tiers are right. Only their separate
  vocabulary goes.
- **No higher-kinded types, no top type.** Untouched.

---

## Decisions for the owner

Thirteen ballots, on card #1508. Each stands alone.

| Ballot | Asks | Recommendation |
|---|---|---|
| `D-META-ONE1` | Adopt the model. Rule rows move from Rust to Jet declarations. Fulfills `D-VERDICT-1455-1`. | A |
| `D-META-STAGE1` | **Amends `D-VERDICT-1308-1/2`; may retire `D-CTMARKER1`.** One mark for compile time, and whether it belongs to the binding or the name. | B |
| `D-META-BODY1` | What a derive body contains: the implementation itself. | A |
| `D-META-FORM1` | How a rule declaration carries its facts: named parameters marked with the compile-time sigil, and no new keywords. | A |
| `D-META-CODE1` | Generated code is written as code, not as a string. Retire `emit()` and `{{` escaping. | A |
| `D-META-AUTO1` | **Amends `D-AUTODERIVE1=E`.** Everything derivable is derived unless refused; only the underivable stays opt-in. | A |
| `D-META-USER1` | What a user-written rule may do: record facts and add code. Walks through the door the marker proposal left open. | A |
| `D-META-DSL1` | **Amends `D-DSLBLOCK1=A`.** Open the text region to libraries; keep the grammar closed. | A |
| `D-META-EFFECT1` | **Amends `D-CTCORE1`.** One effect model at both stages; the allow-list retires. | A |
| `D-META-CONST1` | Compile-time expressions wherever a constant is legal. Retire E0963 and E0035 literal-only rules. | A |
| `D-META-MODNAME1` | Generic-module instances expose members by name. Retire mangled names from user code. | A |
| `D-META-REG1` | One registration table behind markers, planes, rights, and build facts. Touches three sibling proposals. | A |
| `D-META-NAME1` | The word that declares a rule. Naming menu. | A |

Five ratified rulings are amended by name:

| Amended | By | What changes |
|---|---|---|
| `D-DSLBLOCK1=A` | `D-META-DSL1` | The text region opens to libraries; the grammar stays closed. |
| `D-CTCORE1` | `D-META-EFFECT1` | The allowed-call list retires; the effect set carries the fact. |
| `D-VERDICT-1308-1` | `D-META-STAGE1` | The explicit form moves from `#Known` to the sigil. Implicit folding is untouched. |
| `D-VERDICT-1308-2` | `D-META-STAGE1` | `#Known if` becomes `$if`. |
| `D-CTMARKER1` | `D-META-STAGE1` | Generalized if the mark belongs to the binding; retired outright if it belongs to the name. The ballot decides which. |
| `D-AUTODERIVE1=E` | `D-META-AUTO1` | The default set widens from four traits to everything structurally derivable. Refusal spelling is unchanged. |

No other ratified decision is touched.

This proposal introduces exactly **one new word**: the keyword that declares a
rule, which `D-META-NAME1` picks. Everything else reuses ratified syntax. Facts
about a rule are named optional parameters marked with `$`. Rule bodies use
`if`, `not`, and `loop`. `reject` is a Prelude function, not a keyword, with the
same code/what/why/fix signature `b.error` already has. `$loop` is the ratified
`loop` verb at compile time. That word ships with a row in
`crates/jet-foundation/src/Syntax.rs` and a decision ID, as I7 requires.

---

## Implementation shape

**Phase A — internals, no surface change.** Move the 99 marker rows to Prelude
declarations behind the existing spellings. Delete the ~30 hand-parsed paths.
One vocabulary diagnostic replaces five. All tests stay green.

**Phase B — land the ratified backlog on the new substrate.** The five open
marker cards (#1456, #1457, #1458, #1460, #1461) and the plane law
(`D-TYPE2-PLANE1=A`) build once, on declarations, instead of twice on tables.

**Phase C — the balloted surface changes.** Each is a greenfield migration that
deletes the replaced form: code-shaped derives, user markers, open tables,
compile-time constants, one effect model.

---

## Open items for verification

Two ledgers disagree and one is wrong. `tests/jit_gaps.txt` marks
`comptime/embed` and `comptime/find` covered. The tier-parity audit
(`docs/audits/jit-run-tier-vs-aot-parity-audit-2026-07-28.md`) records both
failing at the default `run` tier with E0956. `jit_gaps.txt` tracks compile
coverage, not run coverage. This needs its own card; it is not caused by this
proposal and does not block it.

Separately: the comptime substrate decisions are missing from the Tower store.
`D-CTCORE1`, `D-CTEFFECT1`, `D-CTIO1`, `D-CTFIND1`, `D-CTCODEGEN1`, and
`D-METADEPTH1` exist only as prose in `docs/spec/syntax-decisions.md`. The
build-graph and mutation decisions are present and ratified, so this gap is
specific rather than general. It matters here because `D-META-EFFECT1` amends
`D-CTCORE1` by name, and `tower lint` cannot see an amendment to a record that
does not exist. This needs its own card.
