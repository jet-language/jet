# Marker plane — first-principles rebuild

2026-08-05. Full audit of marker shape, syntax, and implementation, rebuilt from
the ground up. Status: awaiting owner review. Ballot rows at the end; nothing
here is implemented yet.

## Glossary

- **Marker** — a named, typed rule written `#Name` or `#Name(args)` that changes
  or records one fact about one target.
- **Target** — the code a marker applies to: a file, module, type, field,
  variant, function, method, parameter, binding, statement, block, or expression.
- **Registry** — the one compiler table (`Policy::APPLIED_RULES`) that owns every
  marker's name, signature, legal targets, and active/retired status.
- **Group** — one bracket list `#[A, B]` that applies two or more markers to one
  target.
- **Scoped rule** — a marker whose target is a brace block: `#Unsafe("…") { }`.
- **Authority rule** — a marker that unlocks checked power. It always carries a
  written reason.

## The one concept

Strip everything away and ask what this plane must do. The answer is one
sentence:

> **A marker applies one named rule to the next thing you write.**

Everything else follows from that sentence plus three requirements:

1. **Beginner reading.** `#Codable` before a struct reads as English. No
   wrapper words, no attribute classes, no macro imports.
2. **Expert control.** Rules can change semantics, gate authority, partition
   builds, and configure codegen — and every one of them is auditable from one
   table.
3. **One source of truth.** Parser, sema, formatter, LSP, explain, reflection,
   and editors all read the same registry row. A marker that bypasses the
   registry is a bug, not a shortcut.

The 2026-07 overhaul (D-VERDICT-732-1, D-MARKSIG1=A, D-MARK-STACK1=A) already
ratified most of this. The first-principles pass below confirms the core,
simplifies one law, and shows that the implementation has drifted a long way
from the ratified design.

## What other languages teach

| Language | Shape | Lesson for Jet |
|---|---|---|
| Python decorators | `@name` above `def`; runtime function wrapping; stacking = repeated lines | The single reading "modifies the next thing" is why beginners get it instantly. Jet keeps that reading and removes the traps: typos are compile errors (E0927), and nothing runs at runtime. |
| Rust attributes | `#[attr]`, `#[derive(A, B)]`, proc macros, tool attributes | One outer syntax hiding four mechanisms confuses. The `derive(…)` wrapper is pure ceremony — Jet's bare `#Codable` already beats it. Macro-powered markers wait for the metaprogramming epoch; they must land inside this plane, not beside it. |
| Java annotations | Annotations are types with typed elements; `@Target`/`@Retention` declare placement | The most-audited design in industry and it matches D-MARKSIG1 exactly: typed signature + declared legal sites. Java's failure is that annotations can *do* nothing without external processors. Jet's rules are honest: they change meaning, and the registry says which ones. |
| C# attributes | `[Attr(pos, Name = v)]`, positional then named args | Confirms the ordinary-call-grammar choice. C#'s `[method: Attr]` use-site targets show the cost of ambiguous placement — Jet avoids it because every rule declares its targets. |
| Swift | `@attributes` + property wrappers + result builders + macros | Four separate planes is the failure mode. Every future Jet feature that smells like an annotation must be a registry row (I8), never a new plane. |
| Nim pragmas | `{.inline, deprecated.}` postfix, always bracketed; `{.push.}`/`{.pop.}` ranges | Always-bracket taxes every single-marker line — Jet's bare-single wins. Range push/pop is solved better by `#Policy` lexical inheritance. |
| C++ | `[[nodiscard]]`, spec says ignorable | Pretending markers are inert metadata fails; vendors bolt semantics on anyway. Jet is honest that rules have meaning. |
| Zig | No attribute plane; keywords (`export`, `inline`) and builtins | Avoiding the plane spends keywords. Jet's I8 keeps the keyword set small and puts rules in one place instead. |
| D | UDAs + pragmas + built-in attributes | Three planes for one concept — the cautionary tale. |

Net: no surveyed language has all four of (a) bare English reading, (b) typed
checked signatures, (c) declared legal targets, (d) one grouping law. Jet's
ratified design has all four on paper. The gap is not the design — it is the
five-way form split and the implementation.

## Audit findings

### Surface (spec level)

The registry holds 79 active rows and 21 retired rows. The grammar law is
split across five `RuleForm`s (`Bare`, `Call`, `BareOrCall`, `Block`,
`Prefix`) — but the split does not survive first-principles inspection:

- `Bare` / `Call` / `BareOrCall` are not three forms. They are one call
  grammar plus defaulted parameters: `#Inline` and `#Inline(Always)` differ
  only in whether arguments are written.
- `Block` is not a form. It names a *target kind*: the marker's target is a
  brace block. `#Unsafe("r") { }` is the same shape as `#Inline fn hot…` —
  marker, then target.
- `Prefix` is not a form either. `#Known x :: 5` and `#Track speed :: f()` are
  the same shape — marker, then a same-line binding target — yet `Known` is
  `Prefix` and `Track` is `Bare` in the registry today.

One law covers all five. That is Ballot 1.

Two unratified spellings are typeable today (I7 violations):

- `#SQL<Row> { … }` — the only marker in the language with a generic type
  argument; the registry row declares zero parameters.
- `#HTML` names two unrelated mechanisms: a File-site pairing marker
  `#HTML("path")` and an inline DSL block `#HTML { … }`.

One authority inconsistency: `#Nondeterministic` requires a reason "like
`#Unsafe`/`#Impure`" (D-BLOCKPLANE1), but `#Impure`'s registered reason is
optional.

### Implementation (the larger problem)

D-MARKSIG1=A ruled: one registry, one call grammar, every tool reads the same
row. Card #759 closed. The code today does not honor the ruling:

- **~30 markers bypass the registry entirely.** They are hand-bumped as raw
  tokens with no `Marker` node, no signature check, no site check, no
  vocabulary check: `#MustUse`, `#Replayable`, `#WasmExport`, `#Persist`,
  `#Track`, `#Known`, `#Local`, `#Shared`, `#Static`, `#Off`, `#DebugOnly`,
  `#Shield`, `#Live`, `#Reactive`, `#PubFile`, `#NoPrelude`, `#Extern`,
  `#Bindgen`, and more. `#[PubFile, NoPrelude]` is validated; bare `#PubFile`
  is not — same marker, same target, two code paths.
- **Five different "unknown marker" diagnostics** depending only on where the
  typo sits: E0927 on types/fields, E0355 in function lists, E0990 (a code the
  spec retired) at file scope, E0003 at statement position, E0733 ("there's no
  tag called X") at expression position. Expression position first swallows
  *any* `#Name` as a taint tag.
- **Two full function-marker applicators** parse the same markers with
  duplicated argument decoding (`#Every`, `#Pre`/`#Post`, `#State`,
  `#Transition`, `#Inline`, `#Target` each decoded twice, byte-for-byte).
- **Three shadow site tables** duplicate the registry's `sites` column
  (`is_reserved_item_rule_prefix` 18 names, `function_marker_has_applicator`
  23 names, the statement dispatch's hardcoded 14-name list) and can drift.
- **`#Unsafe` needs six keyword special cases** because its name lexes as
  `KwUnsafe`, not an identifier.
- **The formatter reconstructs markers from booleans** (a hand-summed count
  over 16 `Func` flags plus a 5-name layout hack) because parsing throws the
  `Marker` nodes away. Every new marker must be re-taught to `jet fmt`.
- **Retired markers still apply their effects**: `#Pure` sets `is_pure` and
  `#InlineAlways` sets `is_inline_always` *after* diagnosing.
- **Dead rows**: `#Authority` is never parsed anywhere; `#Summarize` falls
  through to a derive for a trait that does not exist. Both were supposed to
  be resolved by the ghost-marker triage (card #763).
- **Static resolution skips most sites**: block, statement, declaration,
  constant, and function markers never bind a site, so comptime argument
  evaluation silently never runs for them.
- **Distinct types skip the vocabulary check**: an unregistered marker on a
  `distinct` type is silently ignored.
- **Duplicate markers are silently legal**: `#[Inline, Inline]` parses clean.
- **Tree-sitter has drifted**: it still spells lowercase markers with the
  retired `@` sigil, cannot parse `#!Printable` or lowercase `allow`/`wire`,
  and accepts unbounded bare stacking the compiler rejects.
- **Reflection lies**: `MethodInfo.markers` is hardcoded to an empty list.
- **The lexer knows marker names** (`#FFI`, `#UnitFamily`, `#Unsafe` ASI
  lookback) — a layer that should never see the vocabulary.
- **Test coverage is type-site-only**: all four E0927 fixtures put the unknown
  marker before a struct; the positions that misbehave have zero fixtures.

The 2026-07 unification changed the registry and the spec. It did not migrate
the compiler onto them. That migration is the bulk of the work this proposal
proposes.

## The rebuilt model

### Grammar: one law

> A marker — or one bracket group — is written immediately before its target.
> The registry says which targets each rule accepts. Parentheses appear exactly
> when arguments are written.

That single sentence replaces the five forms. The registry row schema shrinks
to: **name, signature, targets, repeatable, status, inheritance**. Nothing in
the user surface changes spelling; the change is that the spec and registry
stop pretending there are five grammars.

Layout is the formatter's job, one rule: markers on a type, function, or
method go on their own line above; markers on a field, parameter, binding,
statement, or block stay inline. (This matches every current example.)

Grouping stays exactly as ratified (D-MARK-STACK1=A): bare when single, one
`#[A, B]` list for two or more, E0999 with autofix for the edges. Scoped rules
still never share one group — when two scoped rules interact
(`#Caps` then `#Grant`), nesting order is meaning, and the syntax must show it.

### The full surface in one program

```jet
#[Target(Web), HTML("dashboard.html")]          // file rules: one group

#[Codable, RenameAll(camel)]                    // type rules: one group
User :: struct {
    #Doc("display name") name: String           // field rule: bare, inline
    #[Skip, Default(0)] visits: Int             // field rules: group, inline
    #Redact secret: String
}

#Known limit :: 32                              // binding rule, same line

#[Job, Every(5min), Doc("refresh the cache")]   // function rules
fn refresh() { … }

#Inline
fn hot(a: Int) => Int = a * limit               // bare rule, line above

fn read_device(p: *Int) => Int {
    #Unsafe("p is a live MMIO register; the read has no side effects") {
        = mem.volatile_read(p)                  // scoped rule: block target
    }
}

#Test("refresh fills the cache") {
    .timeout(2s)                                // scope member (D-DOTSCOPE1)
    refresh()
    assert cache.len() > 0
}
```

One reading covers every line: the rule applies to the next thing.

### The sigil map (confirmed, no change)

`#` has exactly two readings, and both are old English uses of the character:

| Reading | Uses |
|---|---|
| "the following is ruled/tagged" | markers `#Codable`, value tags `#PII String`, interpolation selectors `{v#Debug}` (render *via* the rule), effect sets `#(FS)` |
| "number/pin" | fixed lists `[T#N]`, package pins `pkg#1.2.3` |

The interpolation selector deliberately rhymes with the derive plane:
`{value#Debug}` renders through the same `Debug` rule that `#Debug` derives.
This is a feature, not a collision.

### Validation: one pipeline

Every marker, at every site, flows through the same four checks, in order,
driven only by registry data:

1. **Vocabulary** — unknown or retired name → one E0927 family with
   did-you-mean and the retirement fix. Everywhere: types, fields, functions,
   statements, blocks, expressions, files, distinct types. E0990, the E0733
   marker path, and the E0003 fallbacks retire.
2. **Site** — wrong target → E0355 naming the rule's legal targets.
3. **Signature** — the one call-argument binder → E0930 printing the declared
   signature. Closed menus, first/last path-segment rules, and "owns its menu"
   exceptions become registry columns, not name-matched special cases.
4. **Duplicates** — the same rule twice on one target → error with a drop-it
   fix, unless the row is marked repeatable (`#Pre`, `#Post` legitimately
   repeat; see Ballot 2).

### Implementation architecture

- **Parse blind, keep the nodes.** One `parse_marker_sequence` runs at every
  node start. It accepts any name (identifier *or* keyword — killing all six
  `#Unsafe` special cases), never interprets, and attaches `Vec<Marker>` to
  the node. All hand-bumps, both function applicators, the
  `marker_sequence_leads_to_function` heuristic, and the three shadow site
  tables are deleted.
- **Interpret once, in sema.** One registry-driven pass runs the four checks,
  comptime-evaluates arguments, and lowers each marker to its typed fact
  (`is_unsafe`, `every`, …). Codegen keeps seeing typed facts; nothing about
  I3/I9 moves.
- **Consumers read markers, not flags.** The formatter prints the retained
  nodes (flag-reconstruction deleted). Tree-sitter's name list is already
  generated from the registry; the grammar joins it and gains `!`, lowercase
  names, and loses `@`. `MethodInfo.markers` becomes real.
- **The lexer forgets the vocabulary.** ASI lookback generalizes over `#` +
  any name; the `#UnitFamily` literal-range escape moves to the parser.
- **Rows stop lying.** `#Authority` and `#Summarize` get implemented or
  retired under card #763's already-ratified triage terms. Retired rows stop
  applying effects. `#Doc`-on-function-requires-`#Job` becomes registry data.

Forward-compatibility note: each registry row is already shaped like a
declaration (name + typed signature + argument enums that are real prelude
declarations per D-RULEARG-TYPES1). When the metaprogramming epoch opens
user-defined markers (D-CTMARKER1), rows can become `core.lang` declarations
without changing any spelling. Nothing in this proposal blocks that door.

## Verdicts

**Confirmed by first principles (ratified, no ballot needed):** the `#` sigil
plane (D-VERDICT-732-1); ordinary call grammar with typed signatures
(D-MARKSIG1=A); bare single / bracket group for 2+ (D-MARK-STACK1=A); no
derive wrapper; reason-gated authority blocks (D-UNSAFE-REASON1=A,
D-BLOCKPLANE1); scope members as the only scope vocabulary (D-DOTSCOPE1); the
`#Meta` funnel for inert metadata (D-MARK-META1=B); acronym casing
(D-ACRO-CASE1/LEX1); prelude enums for argument menus (D-RULEARG-TYPES1=A);
name-collision law (D-MARKER-NAME-HYGIENE1=A).

**Changed (needs a ballot):** the five-form split (Ballot 1), the duplicate
law (Ballot 2), `#Impure`'s optional reason (Ballot 3), the two unratified DSL
spellings (Ballot 4).

**Owed (implementation debt, cards, no ballot):** everything under
"Implementation architecture" — it implements rulings that already exist.

## Ballot rows

### Ballot 1 — One placement law; the five forms retire

**A (recommended).** One grammar law: a marker or one bracket group sits
immediately before its target; parentheses appear exactly when arguments are
written; empty parentheses are a formatter-fixed error. `RuleForm` leaves the
registry. No user-visible spelling changes; S82's prose is rewritten to the
one law.

```jet
#Known limit :: 32          // same shape…
#Track speed :: f()         // …same shape (today one is "Prefix", one "Bare")
#Unsafe("reason") { … }     // marker, then block target — same shape
#Inline fn hot() => Int     // marker, then declaration target — same shape
```

**B.** Keep the five registered forms as documented law.

```jet
// Same code compiles either way; under B the spec keeps five grammars
// and every new marker must pick one.
```

Why A: five categories dissolve into "signature + target" with zero spelling
cost; every future marker is one row, not one grammar decision.

### Ballot 2 — Duplicate markers on one target

**A (recommended).** The same rule twice on one target is an error with a
drop-the-repeat fix, in the E0999 family. Rows that legitimately repeat are
marked `repeatable` in the registry — `#Pre`, `#Post` (several contracts),
`#allow` (several lints).

```jet
#[Pre(n > 0, "n must be positive"), Pre(n < 100, "n must be small")]  // legal: repeatable
fn f(n: Int) { … }

#[Inline, Inline] fn g() { … }   // error: `#Inline` is already applied — remove the repeat
```

**B.** Keep today's behavior: silent acceptance, last one wins.

Why A: a silent duplicate is always a typo or a merge artifact; repeatable
rows make the legitimate cases explicit instead of accidental.

### Ballot 3 — `#Impure` joins the reason law

**A (recommended).** `#Impure(reason: String)` — reason required, exactly like
`#Unsafe` and `#Nondeterministic`. All three authority gates read identically.

```jet
#Impure("reads the wall clock during folding") {
    now := time.now()
}
```

**B.** Reason stays optional; `#Impure { … }` remains legal, and the spec
records that `#Impure` is a lighter gate than the other two.

Why A: D-BLOCKPLANE1 already names `#Impure` alongside `#Unsafe` as
reason-gated; the registry default of `none` contradicts the ruling. One law
for authority is easier to teach and audit.

### Ballot 4 — Register the two stray DSL spellings

Today `#SQL<Row> { … }` parses with a generic argument no registry row
declares, and `#HTML` names both a File pairing marker and an inline DSL
block. Both are typeable but unratified (I7).

**A (recommended).** `#SQL(Row) { … }` — the row type becomes an ordinary
marker argument through the one call grammar; the `<T>` spelling retires. The
inline DSL block keeps `#HTML { … }`; the File-site pairing marker takes its
own name. Name menu (pick one): `#Page("dashboard.html")`,
`#HTMLFile("dashboard.html")`, `#Hosts("dashboard.html")`,
`#Canvas("dashboard.html")`.

```jet
#Page("dashboard.html")          // file: this Jet file pairs with that page

rows := #SQL(Row) {              // block: typed inline SQL, ordinary call arg
    SELECT id, name FROM users
}
```

**B.** Keep both current spellings and ratify them as-is: `<T>` becomes a
registered signature feature available to markers, and `#HTML` is registered
twice with site-dependent signatures.

**C.** Drop the File pairing marker entirely; pairing moves to the package
manifest.

Why A: it removes the only generic-argument grammar in the plane and the only
name that means two things, at the cost of one rename.

## Implementation cards (post-review)

1. **Uniform attachment** — one blind parse path, markers retained on every
   AST node, hand-bumps/dual applicators/shadow tables/lexer vocabulary
   deleted, keyword marker names legal.
2. **One validation pipeline** — vocabulary/site/signature/duplicate checks at
   every site from registry data; E0990 and the E0733/E0003 marker fallbacks
   retire into the E0927/E0355/E0930 families; UI fixtures for every site
   class (function, statement, block, expression, distinct, file, parameter).
3. **Consumers** — formatter prints retained markers; tree-sitter grammar
   regenerated (lowercase, `!`, no `@`, no unbounded stacking);
   `MethodInfo.markers` real; `jet explain` reads the same rows.
4. **Row debt** — `#Authority`/`#Summarize` implemented or retired per card
   #763's terms; retired rows stop applying effects; menu special cases
   (`#FFI`, `#RenameAll`, first-vs-last segment) become registry columns;
   `#Doc`-with-`#Job` coupling becomes row data.
5. **Ballot outcomes** — form collapse (B1), duplicate law (B2), `#Impure`
   (B3), DSL rows (B4), each landing as one registry + spec + snapshot
   migration.

## Clerical notes for the owner

- The seven 2026-07-23 marker ballots (D-MARKSIG1, D-MARK-STACK1, D-ACRO-*,
  D-INLINE-PARAM1, D-CONSTMARK1, D-UNSAFE-REASON1) exist only in
  `plugins/tower/.tower/tower.json.pre-restore`, not the live board. The spec
  records their outcomes, so authority is intact, but the live decision list
  has lost their text. Board writes are owner-gated, so this is reported, not
  fixed.
- D-MARK-STACK1's ballot text proposed codes E0931/E0932 for the stacking
  edges; the shipped spec folds both into E0999 and reuses E0931 for the `!`
  signed-derive error. The spec is the live authority; noted here so the
  ballot text is not read as current law.
