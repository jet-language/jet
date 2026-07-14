# Jet language-shape constitution

Status: owner ballot set. Nothing in this document is ratified syntax.

## Goal

A developer should be able to identify a construct's job and predict its shape.
The rule must work across ordinary code, manifests, Core APIs, tooling, and expert
features. It must preserve Jet's beginner default, expert reach, implicit typing,
and one-mechanical-path law.

Uniformity does not mean one spelling for unrelated jobs. It means one visible
axis for each real distinction. A record literal and an invariant-bearing
container should not look identical. A description attached to a declaration
and an authority-changing scope should not share a marker family.

## Why the previous ballot set failed

The previous ballots named isolated inconsistencies, then offered shallow
choices such as “fix all,” “leave as-is,” or “pick a subset.” They did not give
the punctuation a complete meaning, did not account for grammatical position,
and did not show how the answers compose into one language. Several ballots also
mixed independent choices or asked the owner to approve future cleanup rather
than decide an actual user experience.

The replacement starts with a slot grammar. Prefix markers, infix reference
separators, delimiters, declarations, constructors, and operations are different
slots. Each slot gets one predictable law. Reusing a glyph in two visibly
different slots is acceptable only when both readings are fixed and cannot be
confused by a reader or parser.

## Proposed default: semantic slots

The recommended direction is not “everything is a type.” It harvests the useful
part of that idea: typed data should look typed, and expected-type inference
should remove repeated names. It rejects the part that would make hidden work or
invariants look like a passive literal.

| Job | Canonical shape | Reader question |
| --- | --- | --- |
| Declare a named kind or namespace | `kind Name { ... }` or `kind Name = Type` | What does this name introduce? |
| Write a body or scoped enclosure | `{ ... }` | What is inside this body? |
| Write a plural group or collection | `[ ... ]` | Which items belong together? |
| Supply inputs or payload components | `( ... )` | What goes into this call/signature/variant? |
| Supply type parameters | `< ... >` | Which types specialize this declaration? |
| Construct transparent record data | `Type.{ field: value }` or inferred `.{ ... }` | Which visible fields make this value? |
| Select a choice | `Type.Variant(...)` or inferred `.Variant(...)` | Which case is this? |
| Create hidden state or uphold invariants | `Type.new(...)` or a precise named constructor | What work creates a valid value? |
| Convert an existing value | `Type.from(...)`, `Type.parse(...)`, or a precise conversion name | What source becomes this type? |
| Operate on a value | `value.verb(...)` | What happens to this receiver? |
| Attach a descriptive fact or contract | `@Name` or `@[A, B]` | What is true about this declaration/field? |
| Change source interpretation, authority, phase, or inclusion | `#Name ...` / `#Name { ... }` | Under what explicit context does this source run or compile? |
| Name an external reference | `source@package#version` | Where, what, and which exact version? |
| State alternatives | `a | b` | Which peer alternatives are accepted? |
| Carry a value through stages, if adopted | `value |> stage` | Where does this value go next? |

This table is a candidate law, not a bundle of automatic outcomes. Each row
that changes current syntax remains its own owner decision.

## Prefix `@` and `#`: a user-visible boundary

“Attribute versus compiler instruction” is not enough. It asks the user to know
compiler internals. The replacement boundary is visible in source:

- `@` attaches a description to the next declaration or field. It never opens a
  scope, appears as an expression, or changes which source exists. Removing it
  removes a promise, derived interface, wire fact, or documentation fact about
  that item.
- `#` establishes an active interpretation context. It opens a scope or applies
  to a whole declaration/file. It changes authority, phase, target inclusion,
  execution mode, or implementation language.

Under that rule:

```jet
@[Pure, MustUse, Codable]
fn decode(...) { ... }

@[Rename("user_id")]
id: UserId

#Unsafe("MMIO") { ... }
#Test("round trip") { ... }
#Target(Web) fn render(...) { ... }
#FFI(c) fn crc32(...) { """...""" }
```

Behavior-free `#Meta` and field-level wire facts fail this rule and should be
reconsidered for `@`. Expression queries such as `#Caller()` also fail it and
should become ordinary comptime APIs. Effect notation gets its own ballot rather
than being forced onto either marker family.

The infix reference grammar is a separate, unmistakable slot:

```text
source@package#version
```

Here `@` separates source from package and `#` pins an exact selector. Prefix
markers cannot be confused with these infix separators. This preserves the
owner's reference direction without weakening the prefix-marker rule.

## Delimiter constitution

### Curly braces: bodies and scopes

Curly braces contain a body: declarations, executable code, a context applied to
nested code, or the visible field body of one transparent record value.

```jet
struct User { ... }
fn save(...) { ... }
#Transact { ... }
User.{ id: id, name: name }
```

An untyped loose manifest object such as `payload: { ... }` is therefore a bad
fit. The body should either belong to a named role module or be an explicit
typed value such as `PackageId.{ ... }`.

### Square brackets: plural groups and collections

Square brackets always answer “which items?” They cover lists, maps, fixed-size
collections, grouped modifiers, trait bounds, effect lists, and fan-out lists.
The surrounding slot supplies the element relationship.

```jet
[a, b, c]
["name": value]
@[Pure, MustUse]
effects [Net, !Fs]
T: [Renderable, Serializable]
```

`[T#N]` can remain coherent if infix `#` is defined as an exact pin: package
version in a reference, cardinality in a collection type. The delimiter still
means collection; the infix separator states which exact instance.

### Parentheses: inputs, signatures, and payloads

Parentheses group inputs supplied to a call, parameters accepted by a function,
or components carried by one variant. They should not be a generic “group
anything” delimiter.

```jet
fn resize(width: Int, height: Int) -> Image
resize(800, 600)
.Rect(width: 800, height: 600)
```

### Angle brackets: type specialization

Angle brackets remain type-parameter territory. Values do not enter this slot
except narrowly ratified generic-module specialization, where parameter kind is
already statically known.

## Construction without false uniformity

“Everything as a type” correctly attacks arbitrary factories. It fails when a
literal shape hides validation, allocation, entropy, I/O, identity, or
deduplication. A `Set` and a `Deque` are not passive field bags.

The recommended construction ladder makes those semantic differences visible:

```jet
Point.{ x: 3, y: 4 }          // transparent record data
.Connected(socket)            // inferred enum case
Deque.new()                   // fresh stateful value
Set.from(items)               // conversion from existing data
Reader.over(bytes)            // non-owning view
Int.parse(text)?              // fallible text conversion
Key.generate(rng)?            // entropy-producing operation
```

This is more than one spelling because the operations are genuinely different.
The predictable part is that the operation's semantics choose the spelling.
Named constructors remain available where “new,” “from,” or “over” would hide
meaning. Bare `Type(...)` remains an explicit ballot question for component
values and checked scalar wrappers.

Expected-type inference stays first-class:

```jet
color: Color :: .Red
result: Result<Data, Error> :: .Ok(data)
point: Point :: .{ x: 3, y: 4 }
```

The full type name is always legal when no expected type exists. Inference never
guesses between multiple possible types.

## Manifest and module shape

File placement and semantic shape must be independent. A role declaration
should mean the same thing whether it shares a file or lives alone.

This is not true of the current implementation. The parser recognizes a closed
set of role names through special branches rather than the ordinary module
mechanism. Package parsing also accepts some fields that never enter the typed
model. That is a truthfulness defect, not an implementation detail the new
surface may preserve. The replacement must use one closed typed graph, reject
unknown fields, and retain source provenance for every contribution.

Recommended role-module direction:

```jet
module package.identity {
    name: "jet"
    version: "1.0.0"
}

module package.sources {
    items: [.Package.{ reference: github@nixos/nixpkgs#unstable }]
}

module package.outputs {
    items: [
        .Executable.{ name: "jet", build: .Cargo.{ lock: "Cargo.lock" } },
        .Alias.{ name: "jetpack", of: output.jet },
    ]
}

module env.dev { ... }
module workspace.root { ... }
module system.build-box { ... }
```

The exact names remain ballot material. The structural rule is the important
part: role modules own fields; fields do not float at file top level; output
kinds are typed choices, not bare keyword blocks; aliases are data in the output
family, not a one-off function.

Repeated role bodies need their own composition law. Every typed field is
`unique`, `append`, `keyed`, or `refinable`. File order never decides a conflict.
Unknown fields fail. Refinement requires a visible override and records both
sources. `jet inspect package --provenance` presents the merged graph and every
contributor.

One file may contain every role module. Conventional files may each contain one
or more. Moving a whole module between files changes no syntax and needs no
special split/fold language feature. Tooling may offer a source move, but file
layout is not semantics.

## Underscore: soft internal, not another access modifier

Jet is private by default, so Python's “underscore means private” would duplicate
`priv` and teach the wrong rule. The useful Python idea is discoverability.

The recommended ballot direction makes leading `_` mean soft internal:

- legal to access explicitly;
- hidden from default completion, generated docs, and beginner views;
- visible in expert/all-symbol views;
- excluded from stable public-API promises;
- never bypasses real access control or safety;
- user code may shadow compiler-provided `_` helpers only where Jet already
  permits user shadowing, such as the closed prelude rule.

Current Jet also uses a leading underscore to disable a role module. That
conflicts directly with the proposed internal meaning. Ratification therefore
requires a separate explicit disable mechanism; both meanings may not coexist.

This provides an expert hatch without turning hidden APIs into an untracked
second standard library. Hard privacy remains `priv`/default visibility.

## Pipe family

Bare `|` already means peer alternatives in dispatch patterns. That is a strong,
predictable use and should stay.

The useful extension is `|>` for left-to-right value flow:

```jet
request
    |> decode
    |> validate
    |> save
```

If adopted, it is a structural entrypoint to the same call semantics, not a new
dispatch, error, ownership, or effect mechanism. The input occupies one fixed
argument slot; labels and capabilities remain visible. Methods remain ordinary
methods. The ballot must decide the slot rule and whether this writing
flexibility is worth reopening D-SUGAR2.

The bold alternative is a typed flow block that combines alternatives and
stages, but it risks duplicating `if` dispatch. It belongs as the final creative
option, not the baseline recommendation.

## Decision architecture

The work should be decided in dependency order:

1. Shape constitution and delimiter roles.
2. Prefix marker boundary and effect spelling.
3. Declaration, construction, variants, lifecycle, and underscore rules.
4. Manifest role algebra and external references.
5. Core API vocabulary, resource lifetime, duration values, CLI grammar, and
   invokable entries.
6. Package-ecosystem ballots rewritten from the ratified laws.
7. One conformance matrix and automated lint prevent future drift.

The ecosystem wave includes an explicit D-ECO2–19 audit. Those decisions are
already ratified, but many examples mix passive values, constructors, keyword
blocks, aliases, build behavior, and tooling operations. Each receives a
recorded keep, reopen, or supersede result. A prior ratification is not evidence
that its spelling conforms to the later constitution.

Each ballot presents three developed conventional systems, then one additional
frontier option that challenges the assumptions behind them. The frontier
option must preserve safety and explain the new mental model; novelty alone is
not a merit.

## Acceptance test

Before ratification, build a sibling-prediction matrix from every public syntax
entry, Core constructor family, manifest construct, and CLI command. A rule
passes when a reader given the job and one sibling can predict the new shape.
Every miss must be one of:

1. a genuine semantic distinction made visible by the shape;
2. a closed, named DSL island with no effect outside its scope; or
3. a defect that gets its own ballot.

After ratification, the matrix becomes executable policy: `Syntax.rs`, Core API
registries, manifests, CLI tables, docs, examples, formatter output, and editor
grammars must agree. New surface cannot merge without a decision ID and a shape
classification.
