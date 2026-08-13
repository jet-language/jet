# Reflection, layout, and generic-call surface research

Date: 2026-08-03
Revision: single-sigil comparison and final S33 reconciliation ballot

## Outcome

The owner ratified both final surfaces:

- D-LAYOUT-FACTS1=B: focused facts use `Packet.@layout`; full reflection stays
  `Packet.reflect()`.
- D-GENERIC-CALL1=A: calls infer by default and permit explicit
  `call<T>(...)` everywhere.

The layout research now recommends the single-sigil option:

```jet
Packet.@layout
Packet.reflect()
```

`Packet.@layout` is the focused compiler-known view. `Packet.reflect()` is
the explicit full metaprogramming root. Both return the same typed metadata.
The focused form must not create a second fact engine.

Known fields use typed contextual selectors, not strings:

```jet
Packet.@layout.size
Packet.@layout.alignment
Packet.@layout[.count].offset

meta :: Packet.reflect()
loop field; meta.layout.fields { print(field.name) }
```

`[.count]` is checked against `Packet`, completed by the editor, rename-safe,
and navigates to the field declaration. Dynamic tools can search `.fields`.
Ordinary source does not turn a known field into text.

The compiler-dunder alternative was rejected. D-LAYOUT-FACTS1=B reserves the
focused member for the single `@` compiler-fact plane and leaves ordinary
double-underscore names governed by the existing identifier rules.

## Why `@` is the only credible single sigil

Jet already assigns these meanings:

- `@name`: compile-time splice;
- `_name`: soft or internal discovery;
- `__name`: compiler-owned identifier namespace;
- `#Rule`: applied rule;
- `@`: location, address, or source;
- `?`: optional and fallible structure;
- `!`: logical negation;
- `&`, `*`: memory access;
- `^`, `~`: ownership and value movement.

`%` has no useful reflection mnemonic. A new Unicode sigil would harm typing,
search, terminals, and fonts. `@`, `#`, `?`, `!`, `^`, `~`, `&`, and `*`
would create a false rhyme with an existing stronger meaning.

`@` now marks the compile-time transformations. D-LAYOUT-FACTS1 option B
would add a third contextual form:

```jet
@name         // materialize a user-known compile-time binding
@[ a, b ]@    // expand one statement per entry
Packet.@layout // select a compiler-known fact attached to Packet
```

Grammar position distinguishes all three forms. Users cannot declare
`@` members. The compiler-fact catalog stays small, closed, documented, and
visible in completion. This amends D-CTMARKER1 and reconciles
D-VERDICT-1320-1; it does not add a second metaprogramming engine.

Why this beats dunder on the surface:

- one character instead of two;
- explicit specialness without Python's learned “hidden member” association;
- one compile-time signal for splicing, expansion, and compiler-known facts;
- normal `Packet.` completion can show `@layout` under **Compiler facts**;
- no collision with ordinary user fields.

Its cost is real: `@` is shared by prefix compile-time uses and infix package
references. If the catalog grows into `@size`, `@alignment`, `@offset`, and
many peers, the option fails. The one focused member is `@layout`; its related
facts remain grouped inside it.

## Recovered S33 history

The compact S33 summary contradicted the recorded decision chain.

On 2026-06-12, S45 chose inference-only generic calls. It rejected explicit
call-site type arguments. Later D-BIND-BARE1 also removed local binding type
annotations, so underconstrained APIs need a typed literal or type-owned shape:

```jet
stack := [Int].{}
```

On 2026-06-24, D-SERDE6 authorized call-site `<T>` as general Jet grammar.
On 2026-07-14, D-SHAPE-CONVERT1 explicitly preserved S33's general ban.
On 2026-08-03, final verdict D-GENERIC-CALL1=A superseded all three clauses:
calls infer by default and every generic call may use explicit `<T>`.

The final implementation must reconcile every S45, D-SERDE6,
D-SHAPE-CONVERT1, parser-comment, example, and diagnostic reference.

The recommended option keeps:

- inference as the beginner default;
- signature, return, field, and argument context as inference evidence;
- optional `call<T>(...)` for missing evidence or local expert control;
- no Rust `::<T>` separator;
- one general grammar with no Core-only exceptions.

To avoid comparison ambiguity, explicit call arguments use an adjacency
contract: the callee, `<...>`, and `(...)` touch. The formatter writes
`call<T>(...)`. Spaced `a < B > (c)` remains a comparison.

## Peer API findings

### Python

Python reflection is split across `inspect.getmembers`, `dataclasses.fields`,
annotations, descriptors, and `ctypes`. C layout uses
`ctypes.sizeof(Packet)`, `ctypes.alignment(Packet)`, and
`Packet.count.offset`.

Python's underscore conventions remain useful. One leading underscore means
weak internal use. Documented dunders are system-defined names; PEP 8 warns
users not to invent them. Jet can enforce that boundary rather than rely on a
convention.

Sources:

- https://docs.python.org/3.14/library/inspect.html
- https://docs.python.org/3.14/library/dataclasses.html
- https://docs.python.org/3.14/library/ctypes.html
- https://docs.python.org/3/reference/lexical_analysis.html
- https://peps.python.org/pep-0008/

Lesson: Python wins on reach and concise attribute access. It loses on one
coherent model, static field identity, rename safety, target provenance, and
an enforceable magic-name boundary. Jet should keep the useful ownership
signal and remove the convention-only ambiguity.

### Swift

Swift generic function calls infer their type arguments. Callers do not write
a generic argument clause on a function call. Underconstrained APIs need an
annotation or another shape.

Swift groups physical facts under
`MemoryLayout<Packet>.size/.alignment/.stride` and uses a typed key path for
field offset. This is coherent and safer than strings, but ceremonious.

Sources:

- https://docs.swift.org/swift-book/documentation/the-swift-programming-language/genericparametersandarguments/
- https://developer.apple.com/documentation/swift/memorylayout

Lesson: keep inference as the default and group related facts. Improve on
Swift by putting the inspected type first and using a contextual field literal.

### Kotlin and C#

Kotlin and C# allow explicit call-site type arguments while normally inferring
them. C# documents both `Swap<int>(...)` and the inferred `Swap(...)`.
Kotlin likewise treats explicit arguments as an uncommon escape.

Sources:

- https://kotlinlang.org/docs/generics.html
- https://learn.microsoft.com/en-us/dotnet/csharp/programming-guide/generics/generic-methods

Lesson: optional explicitness gives a short default and precise local control.
It is better than mandatory arguments or API-specific exceptions.

### Rust

Rust exposes `size_of`, `align_of`, and `offset_of!` separately. Expression
paths use `::<...>` to distinguish type arguments from comparison operators.

Sources:

- https://doc.rust-lang.org/std/mem/
- https://doc.rust-lang.org/reference/paths.html
- https://doc.rust-lang.org/stable/reference/type-layout.html

Lesson: retain exactness and const availability. Reject the fragmented
vocabulary and extra turbofish punctuation.

### Zig

Zig uses compiler built-ins:
`@sizeOf(Packet)`, `@alignOf(Packet)`, `@offsetOf(Packet, "count")`, and
`@typeInfo(Packet)`. The prefix makes compiler ownership obvious. The costs
are a flat catalog and string field names.

Source:

- https://ziglang.org/documentation/master/

Lesson: a compiler sigil works when its meaning is closed and visible. Jet can
improve on Zig by putting the type first, grouping facts, and typing fields.

## Shared layout contract

Voting for any D-LAYOUT-FACTS1 option also selects this inner API:

```jet
layout.kind
layout.size       // Int? when physical layout is not guaranteed
layout.alignment  // Int?
layout.stride     // Int?
layout.target
layout.guarantee
layout.source
layout.fields
layout[.count].offset // Int?
layout[.count].size   // Int?
```

The active build target is implicit. The value names its target and guarantee.
Exact facts come from a canonical Jet target-layout engine, never rustc.
Field byte facts are optional under the same law as whole-type byte facts.
Semantic field identity remains known when offset and size are unknown.

The full path remains explicit:

```jet
#Known {
    meta :: Packet.reflect()
    print(meta.name)
    print(meta.layout.target)
    print(meta.layout.guarantee)

    loop field; meta.layout.fields {
        print("{field.name}: offset={field.offset}, size={field.size}")
    }
}
```

The full root owns fields, methods, rules, type parameters, semantic facts,
and layout. A focused member is exactly its `.layout` projection.

## Tooling contract

- completion after `Packet.` shows the chosen focused member in a visible
  **Compiler facts** group;
- completion inside `[.count]` shows Packet fields;
- hover shows value, unit, target, guarantee, and source rule;
- go-to-definition on `.count` opens the field declaration;
- rename updates the selector;
- a misspelling gives a Jet what/why/fix diagnostic with a suggested field;
- `jet inspect expand --facts layout` projects the same typed metadata;
- human and JSON views use the same schema;
- empty, unavailable, unsupported-target, ANSI, and `NO_COLOR` states are
  designed and snapshot-tested.

Beginner discoverability is part of the language API. Jet completion must not
hide compiler facts by default.

## Ballot options

### A — Compiler-owned hidden member

A compiler-owned hidden member would keep `$` splice-only, but would cost two
underscores and inherit hidden-member expectations. The owner rejected it.

### B — Single `$` meta member

```jet
Packet.@layout.size
Packet.@layout[.count].offset
Packet.reflect().layout
```

Recommended. Short, collision-free, and visually compile-time. Adds a third
contextual `$` form beside known-value splice and statement expansion.

### C — Explicit reflection only

```jet
Packet.reflect().layout.size
Packet.reflect().layout[.count].offset
```

No sigil amendment. Clean for experts, but makes a focused beginner query pay
the full metaprogramming path.

## Rejected shapes

- `T._layout`: falsely means internal or soft-public.
- Python-style trailing-underscore compiler members: trailing underscores add noise.
- scalar magic members such as `@size` and `@offset`: flat catalog.
- `T.@layout`, `T.#layout`, `T.?layout`, and similar forms: collide with a
  stronger existing plane.
- a new `%` or Unicode sigil: no mnemonic and worse typing or search.
- strings for statically named fields: lose completion, rename, and navigation.
- separate focused and reflection models: violates one mechanism.

## Decision bar

A winning verdict must prove all of these:

1. Common layout reads are shorter and clearer than Python, Swift, Rust, and Zig.
2. Full reflection remains explicit and extensible.
3. Focused and full paths share one typed metadata object.
4. Known fields never require strings.
5. Compiler facts cannot collide with user members.
6. Target and guarantee provenance are visible to experts.
7. S33 has one call rule with no `decode` exception.
8. Completion, hover, navigation, diagnostics, CLI, and JSON ship as one API.
