# Syntax Decisions (the owner's control surface)

**The owner has final say on all user-facing syntax.** Agents implement
only what is Ratified, may rely on Provisional choices (clearly marked,
reversible), and must never invent surface syntax. To propose something
new: add a row to Open Decisions with options and tradeoffs, and stop.

How to ratify: move the row to Ratified with your chosen option. Agents
then update `src/syntax.rs` (and parser if structural), re-bless ui
snapshots (`UPDATE_EXPECT=1 cargo test`), and update docs/spec/spec.md.

## Ratified

**N1 — Language name** *(ratified 2026-06-11)*: **Jet**. Binary: `**jet`**.
Rejected: Jet, Cove, Olex-as-public-name.

**N2 — File extension** *(ratified 2026-06-11)*: `**.jet`**. Source files
are `name.jet`; the extension matches the language name (three letters).

**S1 — Function keyword** *(ratified 2026-06-11)*: `**fn`**. Rejected:
`func`, `def` — recognized only as foreign syntax to emit a teaching
error pointing at `fn` (see S14).

**S3 — Blocks** *(ratified 2026-06-11)*: **curly braces `{ }`**. Rejected:
`end` keywords, significant indentation.

**S8 — String interpolation** *(ratified 2026-06-11)*: `**"hi {name}"`**
— expressions inside `{ }` within quoted text (modern standard). Rejected:
`"hi " + name` concatenation (no `+` for strings; one obvious way).

**S9 — Print builtin** *(ratified 2026-06-11)*: `**print`** (adds a
newline). Rejected: `println` — recognized only as foreign syntax (S14).

**S11 — Built-in type names (M1)** *(ratified 2026-06-11)*: capitalized
`**Int`**, `**Float**`, `**Bool**`, `**String**`. Rejected: `Text`
(industry uses `String`; `Text` recognized only as foreign syntax per
S14), lowercase `int`/`text`.

**S2 — Variable bindings (M1)** *(ratified 2026-06-11)*: `**val`** for
immutable bindings, `**var**` for mutable bindings. Rejected: `set`
(sounds like mutation), `let` / `let mut` (Rust; teaching errors only per
S14).

**S18 — Visibility** *(ratified 2026-06-11)*: **private by default**;
prefix `**pub`** to export an item. Applies to top-level functions (M0+),
types and their fields (M3), and any future module-level bindings.
Within a file, private and `pub` items are equally visible to each other;
`pub` only controls what other files may access via `import` (S16, M6+).
Rejected: public-by-default (Go), explicit `private` keyword (noisy).
Considered and declined (owner, 2026-06-12): grouped visibility —
Jai-style `pub { }` blocks, `pub:`/`private:` section markers, and
top-of-file export lists. Reasons: a file mostly nested in a `pub`
block hollows out private-by-default, and positional grouping dictates
how users must structure their files. Per-item `pub` stands; revisit
only with post-v1 evidence of real boilerplate pain.

**S10 — Ownership keywords (M2)** *(ratified 2026-06-11)*: `**mut`**
(mutable borrow), `**take**` (move), `**view**` (borrow return type),
`**ref**` (stored field, tier 2). Default parameter access has no keyword
(shared read). Rejected: `read` / `write` / `owned` as canonical forms.

**S6 — Statement separators** *(ratified 2026-06-11)*: **semicolons,
required after every statement** — including the last statement before a
closing `}`. One rule, no exceptions. Rejected: newline separators,
optional-before-`}`.

**S12 — Entry point** *(ratified 2026-06-11)*: `**fn main()`** — a special
case; no `pub` required (the runtime always finds `main`). Canonical form
omits `pub`. Rejected: required `pub fn main` (ceremony), top-level
statements without a main.

**S19 — Loops (M1)** *(ratified 2026-06-11)*: `**while cond { }`** and
`**for i in <range> { }**`. Rejected: recursion-only M1, `loop` + `break`
as the primary construct.

**S22 — Range bounds (M1)** *(ratified 2026-06-11; amended 2026-06-15)*:
`**1..10` is inclusive** — it counts 1 through 10. Reads like English, kills the
classic beginner off-by-one. M5 slicing may bring its own evidence; revisit
there if needed. **Step (amended, D-SG8, implemented):** an optional
`**step n**` modifier — `0..10 step 2` yields 0, 2, 4, 6, 8, 10 (Kotlin
spelling). `step` is contextual (only meaningful in a range; an ordinary name
elsewhere). Rejected: half-open `..` (Rust/Python), dual `..`/`..=`, word
form `1 to 10`, and the `:` range spelling `0:2:10` (D-SG8 — collides with `:`
in type annotations, map literals, and trait bounds).

**S23 — Loop control (M1)** *(ratified 2026-06-11)*: `**break`** (leave
the loop now) and `**continue**` (skip to the next turn). Rejected:
plain-word `stop`/`skip`, omitting loop control from M1.

**S24 — Many-way choice: `when` (M1)** *(ratified 2026-06-11; keyword amended
to `when` 2026-06-15, D-SG1)*:

```
when x {
    x == 1 -> { ... };
    x == 2 || x == 3 -> { ... };
    else -> { ... };
}
```

Keyword `**when**` (reads "when x is …"); the head expression names the subject
being examined; each arm is a full `Bool` condition, then `->`, then a `{ }`
block, ended with `;` (S6). The first true arm runs; **an `else` arm is
required**. Arms are ordinary conditions, so ranges and compound tests
need no special pattern syntax (`x >= 400 && x <= 499 -> { … };`).
The backend lowers subject-equals-literal chains to a native Rust `match`
(jump tables where profitable) and everything else to an if/else chain —
optimization is the compiler's job, never the user's. Rejected: `switch`
(former keyword — now a teaching error pointing at `when`), C
`switch`/`case`/`default` (fallthrough baggage), bare-value `match`
(`match` is recognized only for an S14 teaching error pointing at `when`).
M3's enum exhaustiveness story extends `when`.

**S20 — Escapes & literal braces (M1)** *(ratified 2026-06-11)*: minimal
escape set `**\n` `\t` `\"` `\\`**; literal braces are written by doubling:
`**{{**` for `{` and `**}}**` for `}` (Rust/Python style). A lone `}` in
quoted text is an error teaching `}}`. More escapes (`\r`, `\u{…}`) wait
for demand. Rejected: `\{`, full C escape set.

**S21 — Float display (M1)** *(ratified 2026-06-11)*: a `Float` always
prints with a decimal part — `-5.0` prints `-5.0`, never `-5`. The value
visibly stays a Float. Rejected: Rust's `Display` default (drops `.0`).

**S27 — Methods (M3)** *(ratified 2026-06-11)*: instance methods use
`**self`** as the receiver name, with the same access prefixes as
parameters (`mut self`, `take self`; default is shared read). Call with
`**value.method(args)**` — e.g. `c.area()`. Methods may be written **inside
the `struct` / `enum` body** (C++-style) **or** in a separate top-level
`**impl Type { ... }`** block (Rust-style layout, Jet-owned semantics).
Both forms are equivalent; pick whichever keeps the file readable. A
method without `self` in either place is a **static** method on the type
(e.g. `Circle.unit()`). Rejected for M3: separate `interface` /
`trait` types (see S28); inheritance; method invocation as
`area(c)` when `c.area()` is available.

**S25 — Comparison distribution (M1)** *(ratified 2026-06-11)*: in a
`&&`/`||` chain, when the right side is a plain value instead of a yes/no,
the nearest comparison to its left is re-applied to it:
`day == "mon" || "tue"` means `day == "mon" || day == "tue"`. Works for
chains (`x == 1 || 2 || 3`) and every comparison operator
(`x != 1 && 2`). The value's type must match what was compared. When the
values really are different things, write the full comparisons as usual.
Rejected: always requiring full repetition (noisy), a set-membership
construct like `x in (1, 2)` (a whole new form for the same idea).

**S14 — Alias policy** *(ratified 2026-06-10)*: One canonical spelling per
construct; **no aliases, ever**. v1: the compiler recognizes common foreign
syntax (`and`, `try`, `let`, `set`, `func`, `def`, `println`, `Text`, …) and the error
teaches the canonical form.
Later (M6): the LSP offers an autocorrect quick-fix for foreign syntax and
`fmt` canonicalizes, so non-canonical input never survives to disk. True
dual forms are rejected permanently.

**S4 — Type annotations (M1)** *(ratified 2026-06-11)*: `**name: Type`**
after the binding or parameter name (e.g. `val x: Int = 1`). Rejected:
`Type name` before (C/Java).

**S5 — Comments** *(ratified 2026-06-11; amended 2026-06-15)*: `**//`** to end
of line, plus `**/* … */`** block comments (Rust/Go/C++ spelling). Block
comments **nest** — a `/*` inside a block comment opens an inner one that must
also close — so any region of code (including code that already contains
comments) can be commented out without surprise; an unbalanced `/*` is E0002.
Rejected: `#`. Doc comments: `///` (S49); doc-comment block form `/** … */`
stays rejected (S49).

**S7 — Error propagation (M4)** *(ratified 2026-06-11)*: postfix `**?`**
on a fallible call (e.g. `parse(raw)?`). Prefix `try` recognized only for
a teaching error (S14). Rejected: propagation-only-via-explicit-handling.

**S13 — Logical and comparison operators (M1)** *(ratified 2026-06-11)*:
`**&&` `||` `!`** for logic; `**==` `!=` `<` `>` `<=` `>=**` for
comparisons. Word forms (`and`, `or`, `not`) recognized only for teaching
errors (S14). Note: `or` as a *fallback* operator (S35) is a separate
token in expression context — not logical OR.

**S17 — Compound assignment (M1)** *(ratified 2026-06-11)*: the full
C-family set `**+=` `-=` `*=` `/=` `%=` `&=` `|=` `^=` `<<=` `>>=*`*.
`+=` `-=` `*=` `/=` on `Int` and `Float`; the rest on `Int` only.
Left-hand side must be `var` or a `mut` parameter. Rejected: `=` only.

**S15 — Binary profile / panic strategy** *(ratified 2026-06-11)*:
**default build keeps unwinding** (`panic` can be caught inside generated
test harnesses and task `join`). `**jet build --small`** (M6) uses
`opt-level="z"`, full LTO, and `**panic=abort**`. Rejected: abort as the
only mode.

**S16 — Imports (M6+)** *(ratified 2026-06-11; amended 2026-06-12)*:
**quotes mean a file path; no quotes mean a module.** Two forms; `**as alias`
is optional** in both. When omitted, the default namespace is the module
name (see below).

```
import "./lib";                       // file path → namespace lib
import "grades/scoring" as g;         // file path, namespace g
import scoring;                       // module by name → namespace scoring
import scoring as gradebook;          // same module, namespace gradebook
```

1. **File import** — `import "<path>" [as alias];`
  The quotes are required — they mark a **path to a `.jet` file**, not a
  logical module name. `<path>` is relative to the **importing file's
   directory**, using `/` (no `.jet` suffix; the compiler appends it).
   Same-directory files use an explicit `./` prefix (`"./lib"`). Subdirs
   use relative paths (`"util/text"`). Default namespace: the **last
   path segment** (`"grades/scoring"` → `scoring.letter(…)`).
2. **Module import** — `import <module-path> [as alias];`
  No quotes — the compiler resolves a **logical module**, not a filesystem
  path. `<module-path>` is a dot-separated name (`scoring`, `std.fs`; see
  S51 for `std`). The compiler searches **recursively from the project root**
  for a module named after the **first** segment: either `name.jet` anywhere
  under the root, or a directory `name/` containing `name.jet` or `main.jet`.
  Skips `build/`, `target/`, and dot-directories. **Project root** = the
  directory containing `jet.toml` when a manifest exists (M12); otherwise
  the directory of the **entry** `.jet` file. Ambiguous duplicate matches →
  **E0606** (lists every path found).

Cross-file access uses `namespace.item` for every `pub` item (S18).
Rejected: Rust `use a::b`, unquoted file paths (`import lib` when you mean
`"./lib.jet"`), quoted module names (`import "std/fs"`), bare `import;`
with no path or name (teaching error only per S14), required `as`,
selective imports (`import module { item }`, `from module import item`).

**S29 — Struct construction (M3)** *(ratified 2026-06-11)*:
`**Type { field: expr, … }`** — Rust-style struct literals. Every field
name required exactly once; order may differ from the declaration.
Rejected: call-style `Point(x: 1.0, y: 2.0)` (B), required factory
`new` (C). Parser disambiguates `ident {` from blocks in condition
position (see docs/plans/epoch-1/m03-data.md).

**S30 — Enum declaration & variants (M3)** *(ratified 2026-06-11)*:

```jet
enum Shape {
    Circle(Float);              // one payload field: positional type only
    Rect(w: Float, h: Float);  // two or more: named fields required
    Empty;
}
```

Variants are `**Type.Variant**` — e.g. `Shape.Circle(2.0)`,
`Shape.Rect(w: 1.0, h: 2.0)`. Single-payload variants use a positional
type in the declaration and positional args at the call site;
multi-payload variants require named fields in both places. Rejected:
`Shape::Variant` (`::`), enums without payloads in v1, named fields on
single-payload variants.

**S31 — Pattern tests (M3)** *(ratified 2026-06-11)*: `**==`** with a
pattern right-hand side when the left operand is an enum or `T?` —
e.g. `if s == Circle(r) { … }`, switch arms `s == Rect(w, h) -> { … };`,
`if x == value(n) { … }`, `if x == null { … }`. The result is a `Bool`
(S24-compatible). When every arm of a `switch` is `subject == <pattern>`,
sema checks exhaustiveness and `else` may be omitted; mixed arms keep
S24's mandatory `else`. Otherwise `==` is ordinary value equality (S13).
A bare name on the right is a variable when one is in scope; to test a
unit variant with the same spelling, qualify it (e.g. `Light.Red`).
Rejected: `is` keyword, Rust `match`, accessor-only extraction.

**S32 — Absence / Option (M3)** *(ratified 2026-06-11)*: `**T?`** marks
an optional value; `**value(expr)**` when present, bare `**null**` when
absent (lowercase, like `true`/`false`). No nullable references — `null`
is only legal where a `T?` is expected, never as a value of plain `T`.
In most **type** positions, `?` suffix means Option; in a **function return**
position, `T?` means fallible `T ?` and `jet fmt` writes the space. A function
that returns an optional writes `-> (T?)`. In **expression** position,
postfix `?` is error propagation (S7) — parser disambiguates by context.
Rejected: `Option[T]`, `Some`/`None`, `some`/`none`, `T??`, pointer-style
null on non-option types.

**S33 — Generic type argument brackets (M3+)** *(ratified 2026-06-11;
amended 2026-06-12; amended 2026-06-15)*: `**Type<Args>**` — angle brackets
for generic type arguments, e.g. `Stack<Int>`. Fallible returns use `T ? E`
(S34), not a generic `Result` type. Square brackets `**[]**` are reserved for
collection values (S37/S38), indexing (S39), and collection type shorthands
`[T]` (S65) / `[K, V]` (S64), not arbitrary generic type arguments.
Parser disambiguates `<` in type position from comparison; nested closings
split `>>` like Rust. Rejected: square-bracket type args `Type[Args]`
(E0034 teaches `Type<Args>`).

**S34 — Fallible return type (M4)** *(ratified 2026-06-11;
amended 2026-06-14)*:
`**T ? E**` — e.g. `fn parse(s: String) -> Int ? ParseError`. `E` is any
enum, struct, `String`, or the default `Error` type. In a function return,
`**T ?**` means `T ? Error`; users may write `T?` and `jet fmt` canonicalizes
it to `T ?`. Codegen lowers to Rust `Result<T, E>`, but `Result<T, E>` is not
Jet surface syntax. Rejected: `Result<T, E>` as canonical surface syntax,
`T or E` in type position (A), Zig `!T` with inferred error sets (C).

**S45 — Generic function/type syntax (M9)** *(ratified 2026-06-12)*:
angle brackets for type parameters — `fn largest<T: Comparable>(…)`,
`struct Pair<T> { … }`, bounds `<T: A + B>`. Same brackets as S33
(`List<T>`, `Map<K, V>`). Inline bounds, no `where`, no call-site type
arguments (annotate the binding if inference fails:
`val s: Stack<Int> = empty_stack()`). Rejected: square-bracket generics,
turbofish, `where` clauses.

**S28 — Traits (M9)** *(ratified 2026-06-12; amended 2026-06-12)*:
explicit named capabilities, not Go-structural. Declare
`trait Shape { fn area(self) -> Float; }`. Implement in **two equivalent
spellings**:

```jet
struct Point {
    x: Float;
    y: Float;
    impl Serialize {
        fn to_json(self) -> String { … }
    }
}
```

```jet
impl Point: Serialize {
    fn to_json(self) -> String { … }
}
```

**Sigils:** `**.**` walks namespaces and calls methods — modules (S16),
enum variants (S30), `c.area()`. **`:`** attaches a trait to a type in
`impl` blocks — same punctuation as type annotations (`x: Int`) and generic
bounds (`<T: Comparable>`): `impl other.Point: Serialize { … }` extends a
dependency's type. `::` is reserved for foreign Rust paths in `extern rust`
(S50), not user-facing Jet paths. Orphan rule: at least one of trait/type
defined in this program. Rejected: `impl Trait for Type`, `impl Type.Trait`,
Go implicit interfaces, `::` in Jet paths. Naming style is not prescribed
(S54). v1: signatures only —
no default bodies, associated types, or trait inheritance.
*Future relook (owner, 2026-06-15):* the owner may revisit trait-attach sugar
post-v1 (e.g. a C++-ish `Type::Trait` feel). Constraint to carry into that
discussion: `::` is already reserved for foreign Rust paths in `extern rust`
(S50) and was rejected for Jet paths, so any new sugar must not collide with it.

**S48 — Dynamic dispatch (M9)** *(ratified 2026-06-12)*: writing a trait
name in type position (`List<Shape>`, `fn f(s: Shape)`) means automatic
boxing and dynamic dispatch; `<T: Shape>` means monomorphization. Same
invisible-boxing policy as M3 recursion and M8 stored closures. Rejected:
explicit `dyn`/`Box<dyn>` in v1. **Post-1.0:** reopen expert-facing
low-level control (explicit `dyn`, stack vs heap, `Send` bounds) — default
stays beginner-friendly; experts opt in.

**S46 — Lambda syntax (M8)** *(ratified 2026-06-12)*:
`**(params) => expr**` or `**(params) => { … }**` — e.g. `(x) => x * 2`,
`(x: Int) => x * 2`, `(a, b) => { return a + b; }`. Parameter types
optional when inferable from context. `**=>**` is the lambda arrow;
`**->**` stays for return types and `switch` arms (S24) — distinct on
purpose. Rejected: `|x| …` (Rust pipes), `fn(x) …` anonymous-fn keyword.

**S47 — Function types & closure captures (M8)** *(ratified 2026-06-12)*:
function type is `**fn(T1, T2) -> R**` (parameter names omitted; `-> ()`
may be omitted like ordinary functions). Named `fn`s coerce to function
values when referenced without a call. Captures follow M2 automatically —
shared read for names only read, mutable borrow for names written (binding
must be `var`). **Escaping** closures (returned, stored, or passed to a
`take` parameter) must own captures: clonable captures are cloned (lint
L0801); non-clonable captures require an explicit `**take(name)**` prefix
on the lambda: `take(sender) () => …`. Rejected: surfacing Rust's
Fn/FnMut/FnOnce, C++ capture lists on every lambda.

**S55 — Built-in derive policy (M9)** *(ratified 2026-06-12)*: **hybrid**
derive policy for the four built-in traits. **Auto-derive (silent):**
`Printable`, `Equatable` — whenever every field qualifies,
`print("{p}")` and `==` work on day one; a hand-written `impl` overrides
the freebie. **Explicit opt-in:** `Comparable`, `Serialize` — require a
`**derive Trait;**` line in the type body (S28's in-type scope):

```jet
struct Point {
    x: Float;
    y: Float;
    derive Comparable;
    derive Serialize;
}
```

Comparable commits field order to sort/`largest`/`Map` ordering;
Serialize commits a public wire format — both are semantic commitments no
silent derive should make. Missing-trait errors teach `derive Trait;` or
`sort_by` (M8) as alternatives. Rejected: auto-derive all four (owner lean
B), Rust `#[derive(…)]` attributes, user-defined derive macros in v1
(S56 post-1.0).

**S35 — Error handling ergonomics (M4)** *(ratified 2026-06-11; fallback
operator changed to `??` 2026-06-15, D-SG6/S71)*: a **fallback** on a fallible
or optional value, spelled **`??`** (the one fallback spelling, shared with
optionals per S71) — e.g. `parse(x) ?? 0`, `parse(x) ?? return`,
`parse(x) ?? panic("…")`, `m.get(k) ?? 0` on `T?`. Plus **`== ok(v)` /
`== err(e)`** pattern tests (S31 machinery) and postfix **`?`** propagation (S7).
The earlier `or` fallback is **retired** — `or` is now recognized only as a
teaching error pointing at `??` (S14). Rejected: keeping `or` as the fallback
(D-SG6 option B), a `??`/`or` split by type (D-SG6 option A), Rust
`.unwrap_or` / `.expect` methods only.

**S36 — Bug stops (M4)** *(ratified 2026-06-11)*: `**panic("msg")**`
stops the program with a friendly runtime report (file, line, exit 70);
`**require(cond)**` and `**require(cond, "msg")**` panic when the
condition is false — for programmer invariants and preconditions, not
recoverable user errors (`T ? E`). Both are prelude builtins like
`print`. Prefix `assert` is recognized only for a teaching error (S14)
pointing at `require`. Rejected: `assert` as the canonical builtin name,
user-facing `abort`/`fatal` (S15 already uses *abort* as a build-mode
name), panic-only without `require` sugar.

**S37 — List literal (M5)** *(ratified 2026-06-12)*: `**[a, b, c]**`;
empty `**[]**` needs a context type (same pattern as `null` / `none`).
Rejected: `List(1, 2, 3)`, brace literals `{1, 2, 3}`.

**S38 — Map literal (M5)** *(ratified 2026-06-12)*: `**["key": value, …]**`;
empty `**[:]**`. `Map<K, V>` remains the explicit type name, but S64 adds
`[K, V]` as the ergonomic map-type shorthand: `val fruits: [String, Float] =
["limes": 420.0]`. Rejected: brace literals `{"k": v}` (JSON confusion with
blocks), constructor-only maps with no literal.

**S39 — Indexing & out-of-bounds (M5)** *(ratified 2026-06-12)*:
`**xs[i]**` and map read `**m[k]**` stop the program with a friendly
runtime report on out-of-bounds / missing key; `**xs.get(i) -> (T?)**` (and
`m.get(k) -> (V?)`) for safe access. Write `m[k] = v` inserts. Rejected:
indexing always returns `T?` (unwrap ceremony), split policy (Option for
maps only).

**S64 — Map shorthand and entry iteration (M5/M8)** *(ratified 2026-06-15)*:
`**[K, V]**` is accepted in type position as sugar for `Map<K, V>`.
This is not a general tuple type and not a replacement for angle-bracket
generic arguments; it is only the visual shorthand for map key/value pairs.

**S65 — List type shorthand (M5)** *(ratified 2026-06-15)*:
`**[T]**` is accepted in type position as sugar for `List<T>` and is the
canonical formatter output for list types. This mirrors S64's map shorthand,
so a Jetpack pack file can write `pub fn shell() -> [JSON]`. `List<T>` remains
accepted for compatibility, but docs and examples prefer `[T]`.

**S66 — Fully capitalized standard acronyms** *(ratified 2026-06-15)*:
Standard Jet type names use fully capitalized acronyms: `JSON`, `JSONError`,
`IOError`, `UTF8Error`, and `U8`. Legacy spellings (`Json`, `JsonError`,
`IoError`, `Utf8Error`) remain accepted while examples and formatter output
move to the capitalized forms.

Iterating a map with one binding yields a built-in named entry value:

```jet
val fruits: [String, Float] = [
    "strawberries": 69.0,
    "limes": 420.0,
    "tangerines": 1337.0,
];

for fruit in fruits {
    print("{fruit.key}: {fruit.value}");
}
```

The entry fields are `key` and `value`, so the feature works for every
`Map<K, V>`, not only maps whose keys are names. The existing two-binding
form remains supported:

```jet
for name, amount in fruits {
    print("{name}: {amount}");
}
```

Rejected: naming the first field `name` (too domain-specific), making
`[K, V]` a general tuple syntax in v1, requiring users to call `.entries()`
for the common case.

**S40 — Slicing (M5)** *(ratified 2026-06-12)*: `**xs[a..b]**` is
**inclusive** (S22-consistent) and **copies** elements (tier 1: no exposed
references). Same rule for `**s.slice(a..b) -> String**` on character
positions. Lint L0501 on slice copies inside loops. Rejected: half-open
slices (Rust/Python), mathematical `(`/`)` endpoint markers (conflicts
with S22's single inclusive `..` meaning).

**S41 — Strings & `Char` (M5)** *(ratified 2026-06-12)*: `**Char**` is a
built-in type; single-quoted `**'a'**` literals; `**s.len()**` counts
Unicode scalar values (characters, not bytes); `**for c in s.chars()**`.
No `**s[i]**` string indexing — E0503 teaches `.chars()` / `.slice(…)`.
Rejected: byte-length strings, UTF-32 O(1) indexing.

**S42 — Numeric types & conversions (M5/M10)** *(ratified 2026-06-12;
named-method casts reconfirmed and C/Go cast syntax declined 2026-06-15,
D-SG9)*:
`**Int**` and `**Float**` are the **default** numeric types — untyped
literals, inference, tutorials, and std APIs use them unless a binding or
parameter is explicitly annotated otherwise (`Int` = i64, `Float` = f64).
A full **sized-type menu** is available for experts and FFI/binary work:
`**I8**` `**I16**` `**I32**` `**I64**` `**U8**` `**U16**` `**U32**`
`**U64**` `**F32**` `**F64**`. `Int`/`Float` are the beginner-facing
spellings for the 64-bit types; `I64`/`F64` exist for explicit-width and
FFI code. Conversions are **named methods only** — e.g. `n.to_float()`,
`f.to_int()`, `x.to_i32()`, `Int.parse(s) -> Int ? ParseError`;
no `**as**` keyword (E0030 teaches the named forms), and no C-style
`(Type)x` or Go-style `x.(Type)` cast syntax (D-SG9). Rejected:
arbitrary-precision integers (C), implicit widening, lowercase Rust
spellings (`i64`), C/Go cast punctuation.

**S43 — Test syntax (M6)** *(ratified 2026-06-12)*: first-class
`**test "name" { … }**` blocks at top level only, using `**require**` and
`**require_eq**` (M4/S36) for assertions. `jet run`/`build` ignore test
blocks; `jet test` runs them. Rejected: `#[test]` attributes, `fn test_*`
naming convention.

**S44 — Formatter style (M6)** *(ratified 2026-06-12)*: one true style,
zero config — **4-space indent**, **same-line `{`**, **line width 100**,
spaces around binary operators, one statement per line, single blank line
max between items, no space before `;`/`,`/`(` of a call; trailing `;` per
S6. `jet fmt` is the only formatter; no style knobs. Rejected:
configurable width/indent, significant-indent formatting.

**S49 — Doc comments (M6/M13)** *(ratified 2026-06-12)*: `**///**`
summary lines immediately above items; plain text in v1; shown by hover/docs
tooling (M13). Degrades gracefully to an ordinary comment. Rejected:
`##` headings, Python docstrings, block `/** … */`.

**S50 — Rust FFI syntax (M7)** *(ratified 2026-06-12)*:

```
extern rust "crate@version" {
    fn name(args) -> T = "rust::path";
}
```

Explicit `**extern rust**` blocks; each entry is a Jet signature plus
`**= "rust::path"**` naming the foreign item. Version pins are required for
non-`std` crates (reproducibility) and remain valid in source even when a
manifest exists; FFI is not package-manager-gated. Boundary types pass by value
only — no borrows, callbacks, or trait objects across the edge. Rejected:
per-function
`@rust(…)` annotations, manifest-only mapping with no inline declaration.

**S26 — Comptime, value-level (M9.5)** *(ratified 2026-06-12)*: layered,
**value-only** compile-time execution. **One law: comptime never creates,
parameterizes, or selects a type, and never affects dispatch** —
polymorphism is traits-only (S28/S45/S48). Any pure Jet function is
comptime-callable with no annotation. **Layer 1 (M9.5):** `comptime`
bindings (S57) evaluate a pure, deterministic Jet subset (no
FFI/IO/time/random; `embed_file("path")` builtin is the one exception) in
a sema tree-walking interpreter — type-checked as ordinary Jet *first*,
then evaluated; fuel-limited with a call-trace diagnostic; `panic` at
comptime is a user-authored compile error (a feature); results lower to
plain Rust constant data (codegen stays dumb, I3). Requires the permanent
differential CI battery: the comptime interpreter and the compiled
runtime must agree bit-for-bit on every evaluable expression. **Layer 2
(M9):** built-in derives (S55 hybrid policy). **Layer 3
(post-1.0):** typed reflection / user derives (S56, deferred). **Rejected
forever:** token/AST macros, custom syntax, attribute macros, comptime
types (types-as-values), const generics in v1. Rejected: closing comptime
entirely (prior recommendation), full Zig-style comptime (imports
instantiation-time diagnostics — the part of Zig we refuse).

**S57 — Comptime binding spelling (M9.5)** *(ratified 2026-06-12)*:
`**comptime x = f();**` — `comptime` is itself the binding keyword; the
binding is always immutable (it is a compile-time constant), so no
`val`/`var` follows it. `comptime val` / `comptime var` / `const` are
recognized only for teaching errors (S14). v1 scope: comptime bindings
only — no comptime blocks, parameters, or function annotations
(smallness; revisit post-1.0). Rejected: `comptime val x` (two keywords
where one suffices), `const` (a second binding keyword competing with
`val`), silent const-folding with no keyword (invisible, unpredictable,
and kills the comptime-panic feature).

**S58 — Expert low-level tier** *(ratified 2026-06-12; post-1.0
milestone pending)*: **two gates, one keyword.**
`**import std.mem**` is the discovery gate — unlocks the low-level
vocabulary: explicit **Zig-style allocators** (allocating APIs take an
allocator parameter; a fixed arena works on embedded), `**Ptr<T>**`,
layout/repr control, volatile wrappers. The keyword `**unsafe**` is the
audit gate for operations that can violate memory safety — pointer
**deref**, pointer math, transmute-class casts, FFI pointer crossings —
in block form (`unsafe { … }`) and contract form (`unsafe fn`; calling
one requires an enclosing block, Rust's rule). Taking a pointer (`&x`)
is legal outside a block (a pointer is inert data); *using* one (`*p`,
`.offset`) requires the block. `&`/`*` are **core grammar, sema-gated**:
outside the gates they keep producing E0208-family teaching errors.
Codegen lowers blocks to Rust `unsafe`; **I1 is amended** — generated
`unsafe` appears only inside user-gated regions plus vetted std/mem
internals. Onboarding materials never mention any of it. Rejected:
`trust` spelling, library-only gating (Swift style), ungated sigils
(C/Zig style).

**S61 — Argument labels & defaults** *(ratified 2026-06-12; post-1.0
milestone pending)*: **optional labels, positional order fixed.** A
caller may write `name: value` on any argument for readability —
`schedule("backup", delay: 30, repeat: 5, urgent: true);` — but
arguments always stay in **declaration order**; labels never reorder.
A label that doesn't match the parameter at that position is a compile
error showing the correct order — labels are compiler-checked
documentation and catch transposed arguments for free. Default
parameter values ride along: `fn f(x: Int, urgent: Bool = false)` —
trailing parameters with defaults may be omitted at call sites.
Rejected: Swift-style required labels (ceremony), Kotlin/Python
reordering by name (two call shapes for one function), no labels at all.

**S62 — Trait delegation** *(ratified 2026-06-12; post-1.0, lands only
after M9 traits show real usage)*: `**impl Trait using field;**` — the
compiler writes the forwarding methods for that one trait to that one
named field. Mirrors S28's two impl spellings: in-type
(`impl Logger using logger;`) or top-level
(`impl Service: Logger using logger;`). The field's type must implement
the trait; forwarding is all-or-nothing in v1 (partial override
deferred). Rejected: Jai-style field hoisting / `using` member
injection (invisible names), Rust Deref-abuse delegation.

**S63 — Resource cleanup** *(ratified 2026-06-12; binding from
streaming-I/O work onward)*: **automatic scope-end cleanup (RAII)** is
the single user-facing story. Std resource types (files, channels,
tasks, …) clean themselves up when the value goes out of scope, on
every exit path — taught as one sentence: *"when a value goes out of
scope, Jet cleans it up."* Backed by Rust `Drop` in codegen (already
true for memory today — this ratifies the contract, not new machinery).
An LSP inlay hint may later visualize cleanup points. A `**defer**`
statement is **noted as a potential later complement** for non-resource
actions (timers, logging) — owner-gated, and never required for
correctness. Rejected: `defer`-as-primary (leak-by-omission, Go's
perennial bug class), `with`-blocks (nesting pyramids).

**S51 — Std library import (M10)** *(ratified 2026-06-12; amended
2026-06-13)*: the std library is **exported as the `std` module** — a
module import (S16 form 2, no quotes), not a file path. `std` is the
reserved short spelling for canonical package `jet.std`; both spellings are
valid. Dot paths select submodules:

```
import std;                    // whole std → namespace std
import jet.std as std;         // explicit canonical spelling
import std.fs as fs;           // submodule, optional alias
import std.io;                  // default namespace io
```

`std` and `jet.std` are compiler-reserved module roots; `std.<module>` and
`jet.std.<module>` select compiler-known submodules (`fs`, `io`, `json`,
…). Optional `as alias` works like S16. Std is never imported via a quoted
path — `import "std/fs"` is wrong because `"std/fs"` is file-path syntax;
use `import std.fs`. Rejected: quoted std paths, separate `use std::`
syntax.

**S54 — Naming convention** *(ratified 2026-06-12)*: **no prescribed naming
convention** in v1 — Jet does not lint or enforce snake_case vs
camelCase/PascalCase. `jet fmt` handles layout only (S44). Rejected:
mandatory snake_case lint.

**S52 — Package manifest (M12)** *(ratified 2026-06-12; amended
2026-06-13)*: `**jet.toml`** — tiny TOML subset, hand-parsed in the
compiler (I6). Full layout ratified in docs/plans/epoch-1/m12-packages.md.

`[package]`: `name`, `version`, `jet` (toolchain constraint), `description`,
`license`, `repository`. `[dependencies]`: Jet deps, name-as-key, git/path/
registry pins; moving selectors `branch = "main"` and `tag = "@latest"`
allowed when `jet.lock` is authoritative (`jet update` refreshes;
`--locked` freezes). Dependency kinds use colon suffixes:
`[dependencies]`, `[dependencies:rust]` (optional FFI metadata; `extern rust`
inline pins remain authoritative),
`[dependencies:c]` (reserved for S59). Reserved, not generated in v1:
`[dev-dependencies]`, `[patch]`, `[workspace]`, `[tool.*]` (ignored except
warn on `[tool.jet]`). Lockfile `**jet.lock**` — graph-shaped, schema
versioned, original+locked per node, content-hash verified. Commands:
`jet add` / `jet remove` / `jet fetch` / `jet update`; registry in M12.2.
`jet new` writes a useful template; `jet new --annotated` adds commented
examples. Optional `.jet/` directory is the source root when present;
`jet.toml` stays at project root. Single-file `jet run file.jet` stays
manifest-free forever (R9). Rejected: JSON manifest; v1 manifest written
in Jet (build.zig style); `[rust-dependencies]` as a separate table name.
**Amended (owner, 2026-06-16, unified ecosystem U1/U2):** the package manifest
becomes **`pack.jet`** (Jet syntax, replacing `jet.toml`) and the lockfile
becomes the single **`.jet/lock`** (replacing `jet.lock` and `pack.lock`) inside
the already-ratified `.jet/` managed folder; realized packages live in the
shared **hangar** store at **`/etc/jet/hangar/`**. The TOML constants
(`jet.toml` / `jet.lock`) remain in `src/syntax.rs` only until the manifest
reshape chunk migrates the paths; the unified records above are the
authoritative target. See `docs/plans/jetpack-jetos/unified-ecosystem.md`.

**S53 — Concurrency surface** *(ratified 2026-06-12; deferred past v1.0)*:
**deferred to v2** — no tasks, channels, or `std/tasks` in v1. When
implemented, the planned surface is ballot option A: `tasks.spawn(closure)
-> Task<T>`, `t.join() -> T`, `tasks.channel<T>()` with
`Sender`/`receive() -> T ? Closed`; no shared mutable state (ownership
rejects it). Rejected for v1: `go`-style `spawn { }` fire-and-forget,
shipping concurrency in v1.

**S59 — C FFI** *(ratified 2026-06-12; deferred past v1.0)*: **deferred to
v2**. Rust FFI (S50/M7) is v1's interop story. When implemented, the
planned surface is ballot option A: `extern c "header-or-lib" { … }` blocks
mirroring S50's `extern rust` shape — one FFI idiom, two backends;
by-value boundary first, pointers only inside the S58 tier. Like Rust FFI,
C FFI declarations are source-level declarations; a package manager may help
install or locate native libraries, but it must not be required just to declare
an external function. Rejected for v1: bindgen-style auto-generation as the
primary surface, Rust-crate detour only. **Amended (owner, 2026-06-15):**
optional header-to-Jet tooling (`jet bind`, compile-time `import c`) is
deferred **past Epoch 2** — see docs/plans/post-epoch-2/c-header-bindings.md.
Epoch 2 (E2-M14) ships manual `extern c` only.

**S60 — Pure-function marking** *(ratified 2026-06-12; post-1.0 milestone
pending)*: `**pure fn name(…)**` — a checked modifier; purity is part of
the signature; violations are compile errors naming the impure call path.
Enables `jet eval --pure` (layer 3, post-v1) and makes comptime
callability visible at API boundaries. Rejected: inference-only purity with
no marking, full effects system.

**D-JPK1 — Jetpack invocation boundary** *(ratified 2026-06-15; amended
2026-06-15)*: `jetpack` is the real package-manager binary/engine for binary
packages, environments, Nix interop, and later JetOS system builds. During
Jetpack Phase 1, it is built and tested **independently** from the `jet` binary:
users and agents invoke `jetpack ...` directly. Later, `jet run github:owner/repo`
and related `jet` commands may delegate to `jetpack`, but that plumbing is not
part of the initial implementation. `jet run main.jet` keeps the normal
local-file Jet behavior. Rejected for Phase 1: hiding Jetpack behind `jet`
before Jetpack itself is functional.

**D-JPK2 — Jetpack command surface, Phase 1** *(ratified 2026-06-15; amended
2026-06-15)*: implement `jetpack run`, `jetpack build`, `jetpack list`,
`jetpack clean`, `jetpack add`, and `jetpack remove` as the Phase 1 command
surface. `jetpack run <ref>` is the zero-config temporary environment/run path.
`jetpack build <ref>` realizes the package/environment without entering it.
`jetpack list` inspects realized environments/store entries. `jetpack clean`
collects unused Jetpack-managed entries. `jetpack add/remove <ref>` edit the
project `pack.jet`. Later `jet` commands can become wrappers around these.

**D-JPK3 — Phase 1 config authoring surface** *(ratified 2026-06-15)*:
Phase 1 ships the directive config surface that today's language can support,
for example `pkg.packages(["ripgrep", "claude-code"])`. The intended evolution
is a first-party fluent Jetpack module after the compiler can support it.
Rejected for Phase 1: waiting for language-level `option`/`when` and pure eval.
The root pack file is `pack.jet` with lockfile `pack.lock` (D-JPK13).

**D-JPK4 — `jet add/remove` transition** *(ratified 2026-06-15)*:
For Jetpack Phase 1, `jetpack add/remove` own package/environment edits. Existing
`jet add/remove` from the pre-Jetpack package-manager work are treated as
transitional commands that may later be replaced by plumbing to
`jetpack add/remove` where the ref belongs to Jetpack's package/environment
domain. Until that plumbing is implemented, agents should not make `jet` and
`jetpack` share mutable state implicitly.

**D-JPK8 — Jet pack file role** *(ratified 2026-06-15; amended 2026-06-15)*:
Jet has a root pack file equivalent in role to Nix's `flake.nix`: the repo
configuration for inputs, package outputs, apps, dev shells, and later JetOS
system/ISO outputs. It is not an arbitrary second manifest for ordinary Jet
source-library dependencies. The ratified filenames are `pack.jet` and
`pack.lock` (D-JPK13). The earlier prototype name was `config.jet`.

**D-JPK9 — Direct Jetpack commands** *(ratified 2026-06-15)*:
Direct `jetpack ...` commands are the Phase 1 product surface, not merely expert
aliases. Build and test `jetpack run/build/list/clean/add/remove` before adding
any `jet` wrapper/delegation layer.

**D-JPK5 — Provider translation layer** *(ratified 2026-06-15)*:
Jetpack owns package management. Nix is not the manager of Jetpack packages; it
is a compatibility provider that Jetpack can translate and tap to access the
massive nixpkgs repository. Phase 1 may invoke Nix as a provider backend, but
the user model, refs, lock/state, shell composition, and lifecycle are Jetpack's.
Jetpack must be designed so a native Jetpack builder can replace or sit beside
the Nix provider.

**D-JPK6 — Forge removal** *(ratified 2026-06-15)*:
Salvage useful Forge ideas into Jetpack planning, then remove
`examples/capstone/forge/` so there is not a competing package-manager capstone.
The salvage record lives in `docs/plans/jetpack-jetos/forge-salvage.md`.

**D-JPK7 — Jetpack priority and ref syntax** *(ratified 2026-06-15)*:
Jetpack Phase 1 is the next implementation track. Public package refs use
`<source>:<package/path-to-package>`, for example `nixpkgs:fastfetch` and
`github:halcyonomega/my-fastfetch-jet-config`. Users should not type Nix's
`#` selector syntax in Jetpack commands.

**D-JPK11 — Remote ref contract** *(ratified 2026-06-15)*:
For `jetpack run <source>:<package/path-to-package>`, Jetpack first looks for
`pack.jet` in the target repo. If absent,
Jetpack may translate a `flake.nix` fallback into Jetpack's internal plan.
Fallback translation still leaves Jetpack in charge of realization, lock/state,
and shell composition.

**D-JPK12 — System state and store roots** *(ratified 2026-06-15)*:
Jetpack should function like Nix at the system level while using Jet-owned
paths: `/etc/jet/` for system configuration/state and `/etc/jet/store/` for the
system store. Project-local metadata may still exist for developer workflows,
but the end-state package store is system-scoped, not hidden inside each
project.

**D-JPK14 — Shell prompt support** *(ratified 2026-06-15)*:
Phase 1 supports bash, fish, and zsh prompt injection. The prompt must clearly
show that the user is inside a Jetpack/Jet shell, and the default visible label
should be `jetpack` rather than the app name. Prompt styling is configurable.

**D-JPK15 — Nix compatibility syntax** *(ratified 2026-06-15)*:
Support flake compatibility and nixpkgs attribute compatibility, but expose them
through Jetpack's uniform `<source>:<package/path-to-package>` shape. Example:
`jetpack run nixpkgs:fastfetch`, not `jetpack run nixpkgs#fastfetch`. Jet pack
files take priority over flake fallback when both exist.

**D-JPK13 — Jet pack file and lockfile naming** *(ratified 2026-06-15)*:
The root Jet pack file is `pack.jet`; its lockfile is `pack.lock`. "Jet packs"
are the product noun (analogous to Nix flakes). Rejected: B `config.jet` +
`jetpack.lock` (generic config name; lock repeats the tool name beside
`jet.lock`); C `config.jet` + `config.lock` (too generic; weak Jetpack
identity); D `jetpack.jet` + `jetpack.lock` (noisy; repeats `jet`).

**D-JPK17 — Named sources in the pack file** *(ratified 2026-06-15)*:
A pack file may declare named sources as values and use them inline through the
ratified `<source>:<package>` ref syntax (D-JPK7/15). Declaration:
`pkg.source("<name>", "<upstream/pin>")`; use: `<name>:<package>` in
`pkg.packages([...])`. The built-in source names `nixpkgs`, `github`, and `path`
need no declaration. A single-argument `pkg.source("nixpkgs")` sets the default
source for bare (unprefixed) package entries. The realizing provider is inferred
from the upstream (R1 routes all named sources through the `nix` provider; the
first-party `core` provider and explicit `via:` override arrive with R2). An
unknown source name is a friendly error listing the built-ins plus any declared
names. Design + worked examples: `docs/plans/jetpack-jetos/native-resolver.md`.
Rejected: a separate `packages_from("name", [...])` grouping (introduces a second
way to attach a source; the inline `<source>:<package>` form is preferred).

**D-JPK16 — Native-resolver posture & Nix-eval engine** *(ratified 2026-06-15)*:
Jetpack's first-party **core** resolver owns realization; backends are providers
behind one trait (`core`, `nix`, later others). For the no-installed-`nix` goal
(roadmap R3), the chosen interim engine is **tvix** (the Rust reimplementation of
Nix), used as a support shim behind the `nix` provider until a first-party Jet
translator replaces it — a natural fit since Jet itself transpiles to Rust. This
requires an **I6 waiver scoped to Jetpack's `nix` provider only**: tvix and its
dependency tree must be isolated (a separate crate or a non-default cargo
feature) so the `jet` compiler proper stays std-only. tvix integration is its own
milestone (R3): `tvix-eval` evaluates Nix but does not by itself realize/
substitute packages, so R3 also needs store/substituter glue. Ship core-first
(R0–R2) before R3. Rejected: B build a from-scratch Nix evaluator immediately
(largest scope, blocks everything); keeping `nix`-binary orchestration as the
permanent path (contradicts the no-installed-`nix` goal).

**S67 — Numeric literals** *(ratified 2026-06-15)*: Rust/Swift/Kotlin-style
numeric literals. **`_` digit separators** anywhere among the digits, stripped
before parsing (`1_000_000`, `0xDEAD_BEEF`). **Base prefixes** `**0x`** (hex),
`**0o`** (octal), `**0b`** (binary) producing an `Int`; a prefix with no digits
is E0001. **Float exponent** `e`/`E` with an optional sign (`6.022e23`,
`3.14e-2`), which makes the literal a `Float`. `1..10` still lexes as range, not
a float, because a `.` only begins a decimal part when a digit follows it.
Rejected: C-style leading-zero octal (`017` — a footgun), no separators.

**S68 — `if` as an expression + optional condition parens** *(ratified
2026-06-15, D-SG2; implemented)*: `if`/`else` may be used in
**expression position** — `val m = if a > b { a } else { b };` — where each
branch is a block whose final expression (no trailing `;`) is its value; an
`else` is **required** in expression position and both branches must share a
type. Mismatched branch types are E0124; a missing `else` in expression
position is E0003. The statement form is unchanged. **Optional parens:**
`if (cond) { … }` and `while (cond) { … }` are accepted as equivalent to the
paren-free form; `jet fmt` strips the redundant outer parens to the no-paren
house style. This subsumes a ternary, so `?:` stays rejected (see gallery §29).
Rejected: C `?:`, statement-only `if`.

**S69 — Newlines in dot-chains** *(ratified 2026-06-15, D-SG3; implemented)*: a
method/field chain may break before a `.` (with an optional trailing line
comment), so steps can be commented individually:
`data\n    .filter(p)   // keep\n    .map(f)\n    .sum();`. Unambiguous because
statements end at `;` (S6). Jet is not newline-sensitive, so breaks already
parse; `jet fmt` preserves author-placed chain breaks (each step on its own line,
one space then `// comment`) and the final step's trailing comment stays after
the `;`. See `examples/features/38_method_chain.jet`. The pipe operator `|>`
remains separately **undecided** (not disallowed).

**S70 — Multi-line strings** *(ratified 2026-06-15, D-SG5; implemented)*:
`**"""…"""`** triple-quoted strings span multiple lines; escapes (S20) and
`{interp}` (S8) stay active. **Swift-style whitespace:** the newline immediately
after the opening `"""` is dropped, the newline immediately before the closing
`"""` is dropped, and the indentation set by the closing `"""`'s column is
stripped from every line. An unterminated `"""` is E0002. `jet fmt` re-derives
the triple-quoted shape from the source span, indents the body to the statement's
column, and is idempotent. See `examples/features/39_multiline_string.jet`.
Rejected: verbatim/Python (leading newline + source indent kept), Go backticks,
Zig `\\` line prefix.

**S71 — Optional chaining and `??` default** *(ratified 2026-06-15, D-SG6
option C; `??` fallback + retired-`or` teaching error **implemented**; `?.`
**field** chaining **implemented** (`a?.b?.c` → `T?`, flattening nested
optionals; non-optional left side is E0047); `?.` through a **method** is E0046
for now; supersedes S35's `or`)*: `**?.`** optional
chaining — `user?.address?.city` yields a `T?` and short-circuits to `null` on
the first absent link. `**??`** is the **single fallback spelling for both**
optionals (`T?`) and fallible values (`T ? E`): `x ?? default`, `x ?? return`,
`x ?? panic("…")` (same right-hand grammar the retired `or` had). `or` is
retired to a teaching error pointing at `??`. Pattern tests (`== ok`/`== err`,
S31) and `?` propagation (S7) are unchanged. Rejected: keeping `or` (option B),
a type-split `??`/`or` (option A).

**S72 — Range step** *(amends S22; ratified 2026-06-15, D-SG8; implemented)*:
see S22 — `start..end step n`; `..` stays inclusive; the `:` range spelling is
rejected (collisions). A non-positive literal step is E0123.

**S73 — Tuples (named-only)** *(ratified 2026-06-15, D-SG7; implemented)*: lightweight aggregates with **named members only** —
`val p = (x: 1, y: 2);`, member access `p.x`, and usable in type position
`fn bounds() -> (min: Int, max: Int)`. Rejected: positional tuples and `.0`
member access (collides with float-literal lexing; a one-field-per-purpose
`struct` covers the rare positional case).

**S74 — Standalone destructuring** *(extends S31; ratified 2026-06-15, D-SG4;
implemented)*: a `val`/`var` binding may
destructure a struct (`val Point { x, y } = p;`), a tuple (`val (x, y) = p;`,
S73), or a list (`val [a, b] = xs;`), reusing the existing bracket
conventions — no new sigils. `var` binds each name mutably; move/borrow follow
the per-name M2 rules. Destructuring is no longer limited to `when` arms (S31).
Rejected: a separate `let`-pattern keyword, JS object-rename syntax in v1.
Struct destructuring is irrefutable — it binds any subset of the struct's fields
and an unknown field is E0302; a non-struct value is E0313. List destructuring
binds each element by position, guarded by a runtime length check; a list
literal of the wrong length is the compile error E0315 and a non-list value is
E0313. The tuple form `val (x, y) = p;` binds named tuple members in
canonical order (S73).

### Unified ecosystem — `jet` + `jetpack` + `jetos` (U-series)

The owner-ratified design-of-record is
`docs/plans/jetpack-jetos/unified-ecosystem.md` (status: owner-ratified,
2026-06-16). Its naming ledger (§10) and the U-series below are **ratified**.
These records establish the authoring-surface tokens; behavior lands in the
Jetpack/Jetos implementation chunks (no syntax is invented beyond this).

**U1 — Package manifest is `pack.jet`** *(ratified 2026-06-16)*: the package
manifest is `pack.jet`, written in **Jet syntax** (not TOML), holding package
identity + Jet library deps (+ optional exported modules). It **replaces**
`jet.toml`. **Amends S52.** Rejected: keeping a separate TOML manifest beside a
Jet pack file (two manifest languages).

**U2 — Single lockfile `.jet/lock` and the `.jet/` managed folder**
*(ratified 2026-06-16)*: one lockfile, `.jet/lock`, **replaces** both `jet.lock`
and `pack.lock`. The project-local `.jet/` folder is the managed area (lockfile,
caches, GC roots), never hand-edited; realized packages live in the shared store,
not here. **Amends S52.** Rejected: per-tool lockfiles (`jet.lock` +
`pack.lock`) that drift.

**U3 — Modules: `module name {}` + leading-`_` disable; `env`/`system`/`image`
namespaces with `Env`/`System`/`Image`** *(ratified 2026-06-16)*: a module is an
explicit named declaration `module name { … }`; multiple modules may share a
file. A **leading underscore** (`module _name { … }`) disables a module — it is
not discovered or merged (one character, reversible). Modules may not import each
other (liftability law); they only contribute to the merged whole. Reserved
namespaces any module may contribute to: **`env`** (type `Env`, a dev
environment / shell), **`system`** (type `System`, a whole machine, jetos), and
**`image`** (type `Image`, an ISO / VM / disk image, jetos). The project
environment file is `env.jet`; the master system config is `config.jet` (default
dir `~/.jet/`). **Supersedes jetos D-OS1** (file-is-module). Rejected:
file-is-module, `import = [ … ]` manual lists, cross-module imports.

**U4 — Import-tree discovery via `find("./path")`** *(ratified 2026-06-16)*:
`imports: find("./modules")` auto-discovers every `.jet` file in the tree and
merges each module's typed contributions — no manual import list
(flake-parts / import-tree by default). **Generalizes jetos D-OS7.** Rejected:
hand-maintained import lists as the default surface.

**U5 — One canonical merge table for all tiers** *(ratified 2026-06-16)*: the
merge rules in unified-ecosystem.md §6 are the single referee across `env` /
`system` / `image`: `sources` merge by key (duplicate names with different refs
conflict unless overridden); `packages` concatenate, de-duplicate, preserve
source identity; namespace entries merge by key with package lists combining;
scalar conflicts are diagnostics unless priority-marked (`default`/`force`).
**Replaces** jetos §5.4 + the former pack-abi merge table. Rejected: divergent
per-tier merge semantics.

**U6 — Source refs `provider@target`; package type `Pkg` + sugar**
*(ratified 2026-06-16)*: source refs are `provider@target` —
`github@owner/repo/rev`, `path@../local`, `nixpkgs@…`. Packages are values of
type **`Pkg`**; in `packages:` lists the type-directed sugar applies
(`default.ripgrep`, `default.[ripgrep, fd]`, `unstable.neovim`), with strings
(`"mine@hello"`) as the escape hatch. (Carries forward the former D-JPK18/19
intent into the unified surface; the `<source>:<package>` Phase-1 command-line
ref form, D-JPK7/15, is unchanged for `jetpack run` arguments.) Rejected:
untyped string-only refs as the primary surface.

**U7 — `jet run file.jet` stays zero-ceremony forever** *(ratified 2026-06-16;
reaffirms R9)*: a single `.jet` file is a complete program; `jet run app.jet`
never needs a manifest, `.jet/`, or any ecosystem file. This is the hard line
that keeps jet usable on its own (the one-way arrow `jetos → jetpack → jet`).
Rejected: requiring any manifest for single-file runs.

## Enforcement

Ratified decisions are **frozen**. `cargo test` runs `tests/decisions.rs`,
which fails if:

- any `src/syntax.rs` entry is `(provisional)` while ratified in this file;
- any open or deferred decision ID appears in `src/syntax.rs`;
- the Provisional table below lists a real decision ID;
- a staged decision loses its pinned error code in docs/spec/diagnostics.md.

Agents: after ratifying a row, update `syntax.rs` to `(ratified)`, clear
the Provisional table row, and add a ui snapshot if behavior changes.

## Staged implementation (ratified syntax, milestone pending)

Syntax and semantics below are **decided** — do not re-litigate. Only the
implementation milestone is pending.


| ID  | Milestone | Enforcement today                                                | Code  |
| --- | --------- | ---------------------------------------------------------------- | ----- |
| S15 | M6        | default unwind in `src/main.rs`; `--small` + `panic=abort` in M6 | —     |


## Provisional — currently in the code


| ID  | Choice in code                         | Where |
| --- | -------------------------------------- | ----- |
| —   | *(none — Group 1 ratified 2026-06-11)* |       |


## Open decisions — owner input needed

> **Ballots:** every open decision below (and all new ones for M3–M14)
> has a full ballot — options, how Rust does it, expert lean, beginner
> lean, recommendation — in **docs/spec/decision-ballots.md**, grouped so
> the owner decides one milestone-sized batch at a time. The rows here
> are the registry; the ballots are the briefing.

### Registered for M3–M14 (see docs/spec/decision-ballots.md for options)


| ID  | Question                                   | Needed by |
| --- | ------------------------------------------ | --------- |
| S56 | typed reflection / user derives (deferred) | post-1.0  |
| S6-R | revisit statement terminators (see note below) | owner-paced |

> **S6-R — Statement terminators, revisit (future).** S6 is ratified today
> (semicolons required after every statement) and stays binding until the
> owner decides otherwise. The owner has flagged this for a **future**
> reconsideration narrowed to exactly **two finalists**:
>
> 1. **Keep 100% semicolons** — the current S6 rule, one terminator, no
>    exceptions. Unambiguous parsing, hard error-recovery sync points, no
>    silent-newline surprises; cost is visual noise and a "missing `;`"
>    error class.
> 2. **Go-style lexer insertion** — no semicolons in source; the *lexer*
>    inserts terminators at line ends when the last token can end a
>    statement. Clean source for beginners while the grammar and diagnostics
>    stay terminator-based; cost is that line-break placement becomes
>    style-constrained (e.g. `{` must sit on the opening line).
>
> Significant-indent and optional-`;` schemes are **out of scope** for this
> revisit. **Decision gate:** the owner wants to compare **multiple bigger
> `.jet` files** (not toy snippets) rendered under each finalist — real
> programs showing multi-line expressions, nested blocks, `switch` arms,
> struct/enum literals, and what a *mistake* looks like under each — before
> choosing. Build the side-by-side example set first; do not re-litigate
> S6's text until then.

> Jetpack native-resolver decisions **D-JPK16** (tvix-shim posture) and
> **D-JPK17** (named sources) were ratified 2026-06-15 — see the Ratified
> section above and `docs/plans/jetpack-jetos/native-resolver.md`.


Group 6 (S26–S28, S45–S48, S46–S47, S55, S57) and Group 7 (S51–S54, S52)
are fully ratified above. S53 (concurrency) and S59 (C FFI) are ratified
as deferred past v1.0. S60 is ratified post-1.0. S56 (user derives via
typed reflection) is deferred past v1.0 by S26's ratified layering.

## Decision log


| Date       | ID  | Decision                                    | By    |
| ---------- | --- | ------------------------------------------- | ----- |
| 2026-06-11 | N1  | Jet; binary `jet`                           | owner |
| 2026-06-11 | N2  | extension `.jet`                            | owner |
| 2026-06-11 | S3  | `{ }` blocks                                | owner |
| 2026-06-11 | S8  | `"text {expr}"` interpolation               | owner |
| 2026-06-11 | S9  | `print` (not `println`)                     | owner |
| 2026-06-11 | S2  | `val` / `var` (not `set` or `let`)          | owner |
| 2026-06-11 | S18 | private by default; `pub` to export         | owner |
| 2026-06-11 | S11 | `String` (not `Text`); `Int` `Float` `Bool` | owner |
| 2026-06-11 | S1  | `fn` (not `func` or `def`)                  | owner |
| 2026-06-11 | S10 | `mut` / `take` / `view` / `ref`             | owner |
| 2026-06-10 | S14 | no true aliases; teach foreign forms        | owner |
| 2026-06-11 | S6  | semicolons required after every statement   | owner |
| 2026-06-11 | S12 | `fn main()`, no `pub` required              | owner |
| 2026-06-11 | S19 | `while` + `for i in <range>` loops          | owner |
| 2026-06-11 | S20 | minimal escapes; `{{` `}}` literal braces   | owner |
| 2026-06-11 | S21 | Float always prints a decimal part          | owner |
| 2026-06-11 | S22 | `1..10` is inclusive (1 through 10)         | owner |
| 2026-06-11 | S23 | `break` + `continue`                        | owner |
| 2026-06-11 | S24 | `switch` with condition arms (not `match`)  | owner |
| 2026-06-11 | S25 | comparison distribution: `x == 1            |       |
| 2026-06-11 | S27 | `self`; `c.area()`; inline + `impl` methods | owner |
| 2026-06-11 | S28 | traits deferred; owner plans to add later   | owner |
| 2026-06-11 | S4  | `name: Type` annotations                    | owner |
| 2026-06-11 | S5  | `//` comments                               | owner |
| 2026-06-15 | S5  | amended: add nesting `/* … */` block comments | owner |
| 2026-06-11 | S7  | `?` error propagation                       | owner |
| 2026-06-11 | S13 | symbol logic/comparison operators           | owner |
| 2026-06-11 | S17 | full compound-assignment set                | owner |
| 2026-06-11 | S15 | unwind default; abort in `--small`          | owner |
| 2026-06-11 | S16 | file + module imports; optional `as`        | owner |
| 2026-06-12 | S16 | amended: quotes = file path; no quotes = module | owner |
| 2026-06-11 | S29 | struct literals `Type { f: v }`             | owner |
| 2026-06-11 | S30 | enums; 1-field positional, 2+ named         | owner |
| 2026-06-11 | S31 | `==` pattern tests on enums and `T?`        | owner |
| 2026-06-11 | S32 | `T?`, `value` / `null`                      | owner |
| 2026-06-11 | S33 | generic args `Type[T]` square brackets      | owner |
| 2026-06-12 | S33 | amended: `Type<Args>`; `[]` for value lists | owner |
| 2026-06-11 | S34 | fallible returns `T ? E`                   | owner |
| 2026-06-12 | S45 | angle-bracket generics; inline bounds       | owner |
| 2026-06-12 | S28 | traits; in-type or `impl Type.Trait`        | owner |
| 2026-06-12 | S28 | amended: `impl Type: Trait`; `.` for paths  | owner |
| 2026-06-12 | S48 | trait-as-type auto-dyn; expert reopen later | owner |
| 2026-06-11 | S35 | `or` fallback + patterns + `?`              | owner |
| 2026-06-11 | S36 | `panic` + `require` for bug stops           | owner |
| 2026-06-12 | S37 | list literal `[a, b, c]`; empty `[]`          | owner |
| 2026-06-12 | S38 | map literal `["k": v]`; empty `[:]`          | owner |
| 2026-06-12 | S39 | `xs[i]` reports; `.get` -> `T?`             | owner |
| 2026-06-12 | S40 | inclusive copy slices `xs[a..b]`            | owner |
| 2026-06-12 | S41 | `Char`, char-length `String`, no `s[i]`     | owner |
| 2026-06-12 | S42 | `Int`/`Float` default; sized menu; no `as`  | owner |
| 2026-06-12 | S43 | `test "name" { }` with `require`/`require_eq` | owner |
| 2026-06-12 | S44 | fmt: 4-space, same-line `{`, width 100      | owner |
| 2026-06-12 | S49 | `///` doc comments, plain text v1           | owner |
| 2026-06-12 | S50 | `extern rust` blocks with `= "rust::path"`  | owner |
| 2026-06-12 | S26 | comptime rescoped: value-level, layered     | owner |
| 2026-06-12 | S57 | `comptime x = f();` binding spelling        | owner |
| 2026-06-12 | S46 | `(x) => …` lambda syntax                    | owner |
| 2026-06-12 | S47 | `fn(T)->R`; M2 captures; `take` on escape  | owner |
| 2026-06-12 | S55 | hybrid derive: auto Print/Eq; opt-in Cmp/Serialize | owner |
| 2026-06-12 | S58 | low-level tier: `std/mem` + `unsafe` gates  | owner |
| 2026-06-12 | S61 | optional arg labels; positional order fixed | owner |
| 2026-06-12 | S62 | trait delegation `impl Trait using field;`  | owner |
| 2026-06-12 | S63 | RAII scope-end cleanup; `defer` maybe later | owner |
| 2026-06-15 | S65 | list type shorthand `[T]`; `List<T>` compatibility | owner |
| 2026-06-15 | S66 | standard acronyms fully capitalized (`JSON`, `IOError`) | owner |
| 2026-06-15 | S67 | numeric literals: `_` separators, `0x`/`0o`/`0b`, float exponent | owner |
| 2026-06-15 | S24 | keyword `switch` → `when` (D-SG1)            | owner |
| 2026-06-15 | S68 | `if` as expression + optional condition parens (D-SG2) | owner |
| 2026-06-15 | S69 | newlines allowed in dot-chains (D-SG3)      | owner |
| 2026-06-15 | S70 | `"""…"""` multi-line strings, Swift trim (D-SG5) | owner |
| 2026-06-15 | S71 | `?.` chaining + `??` default; `or` retired (D-SG6 opt C) | owner |
| 2026-06-15 | S72 | range `step n`; `:` spelling rejected (D-SG8) | owner |
| 2026-06-15 | S73 | named-only tuples `(x: 1, y: 2)` (D-SG7)    | owner |
| 2026-06-15 | S74 | standalone destructuring (D-SG4)            | owner |
| 2026-06-15 | S42 | confirmed: named-method casts; C/Go casts declined (D-SG9) | owner |
| 2026-06-12 | S51 | std imports: `import std.fs as fs` module form | owner |
| 2026-06-12 | S54 | no prescribed naming convention in v1        | owner |
| 2026-06-12 | S52 | `jet.toml` manifest; `jet.lock`; jet add/fetch | owner |
| 2026-06-13 | S52 | amended: `[dependencies:*]` colon tables, lock graph, `@latest`, `.jet/` folder, useful `jet new` template | owner |
| 2026-06-12 | S53 | concurrency deferred to v2; option A when built | owner |
| 2026-06-12 | S59 | C FFI deferred to v2; `extern c` when built  | owner |
| 2026-06-12 | S60 | `pure fn` checked purity modifier            | owner |
| 2026-06-15 | D-JPK1 | build `jetpack` independently first; `jet` plumbing later | owner |
| 2026-06-15 | D-JPK2 | `jetpack run/build/list/clean/add/remove` Phase 1 | owner |
| 2026-06-15 | D-JPK3 | Phase 1 directive `pack.jet` surface | owner |
| 2026-06-15 | D-JPK4 | `jet add/remove` can later plumb to `jetpack add/remove` | owner |
| 2026-06-15 | D-JPK8 | Jet pack file has `flake.nix` role; `pack.jet`/`pack.lock` | owner |
| 2026-06-15 | D-JPK9 | direct `jetpack ...` commands are Phase 1 surface | owner |
| 2026-06-15 | D-JPK5 | Jetpack owns packages; Nix is a compatibility provider | owner |
| 2026-06-15 | D-JPK6 | salvage Forge notes, then remove Forge capstone | owner |
| 2026-06-15 | D-JPK7 | Jetpack next; refs use `<source>:<package/path>` | owner |
| 2026-06-15 | D-JPK11 | Jet pack file first; flake fallback translated | owner |
| 2026-06-15 | D-JPK12 | system roots `/etc/jet/` and `/etc/jet/store/` | owner |
| 2026-06-15 | D-JPK14 | bash/fish/zsh prompt support; default label `jetpack` | owner |
| 2026-06-15 | D-JPK15 | nixpkgs attrs use `source:attr`, not `source#attr` | owner |
| 2026-06-15 | D-JPK13 | `pack.jet` + `pack.lock` ("Jet packs") | owner |
| 2026-06-15 | D-JPK17 | named sources declared in pack.jet, used inline `name:pkg` | owner |
| 2026-06-15 | D-JPK16 | core resolver + providers; tvix shim for no-installed-nix (R3), I6 waiver scoped to jetpack | owner |
| 2026-06-16 | U1  | package manifest `pack.jet` (Jet syntax) replaces `jet.toml`; amends S52 | owner |
| 2026-06-16 | U2  | single `.jet/lock` replaces `jet.lock`/`pack.lock`; `.jet/` managed folder; amends S52 | owner |
| 2026-06-16 | U3  | `module name {}` + leading-`_` disable; `env`/`system`/`image` ns; `Env`/`System`/`Image`; supersedes D-OS1 | owner |
| 2026-06-16 | U4  | `find("./path")` import-tree discovery as default; generalizes D-OS7 | owner |
| 2026-06-16 | U5  | one canonical merge table (unified-ecosystem §6) across all tiers | owner |
| 2026-06-16 | U6  | source refs `provider@target`; package type `Pkg` + list sugar | owner |
| 2026-06-16 | U7  | `jet run file.jet` stays zero-ceremony forever (reaffirms R9) | owner |
| 2026-06-16 | S52 | amended: `pack.jet`/`.jet/lock` (U1/U2); hangar store `/etc/jet/hangar` | owner |
