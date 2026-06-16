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

**S16 — `use` (M6+)** *(ratified 2026-06-11; amended 2026-06-12, **2026-06-16 D-S16-USE**)*:
**quotes mean a file path; no quotes mean a module.** Two forms; **`as alias`
is optional** in both. When omitted, the default namespace is the module
name (see below). Keyword is **`use`**; **`import`** is a teaching error (E0015).

```
use "./lib";                          // file path → namespace lib
use "grades/scoring" as g;            // file path, namespace g
use scoring;                          // module by name → namespace scoring
use scoring as gradebook;             // same module, namespace gradebook
```

1. **File use** — `use "<path>" [as alias];`
  The quotes are required — they mark a **path to a `.jet` file** or (C FFI,
  S59) a **header path** for auto-binding, not a logical module name. `<path>`
  is relative to the **using file's directory**, using `/` (no `.jet` suffix
  for Jet files; the compiler appends it). Same-directory files use an explicit
  `./` prefix (`"./lib"`). Subdirs use relative paths (`"util/text"`). Default
  namespace: the **last path segment** (`"grades/scoring"` → `scoring.letter(…)`).
2. **Module use** — `use <module-path> [as alias];`
  No quotes — the compiler resolves a **logical module**, not a filesystem
  path. `<module-path>` is a dot-separated name (`scoring`, `core.fs`; see
  S51). The compiler searches **recursively from the project root** for a module
  named after the **first** segment: either `name.jet` anywhere under the root,
  or a directory `name/` containing `name.jet` or `main.jet`. Skips `build/`,
  `target/`, and dot-directories. **Project root** = the directory containing
  `pack.jet` when a manifest exists (M12, U1); otherwise the directory of the
  **entry** `.jet` file. Ambiguous duplicate matches → **E0606** (lists every
  path found).

Cross-file access uses `namespace.item` for every `pub` item (S18).
Rejected: Rust `use a::b` re-export chains, unquoted file paths (`use lib`
when you mean `"./lib.jet"`), quoted module names (`use "core/fs"`), bare
`use;` with no path or name (teaching error only per S14), required `as`,
selective uses (`use module { item }`, `from module use item`). Former
**`import`** spelling → teaching error E0015 (D-S16-USE).

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
**Nested patterns (amended 2026-06-16, D-PAT1):** a pattern may appear inside a
payload slot — `r == ok(Rect(w, h))`, `x == ok(value(n))` — binding the inner
names; nesting composes to any depth, so `when`/`if` arms can destructure a
result and its contents in one test.
**Guards (amended 2026-06-16, D-PAT2):** a name bound by a pattern test is in
scope for the rest of the **same** condition, so a guard is just `&&` —
`when r { r == ok(Code(n)) && n >= 500 -> { … }; }` matches a 5xx error. No
dedicated guard keyword is added: S24 arms are already full `Bool` conditions, so
"…but only if" composes from `&&` and the freshly-bound name. Rejected: a
separate guard keyword (would need a new word now that `when` is taken).
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
**Amended 2026-06-16 (S80):** the default `Error` is now a rich carrier
(message + optional code + optional source) and `fn main()` may be fallible —
see S80.

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
Go implicit interfaces, `::` in Jet paths. Jet defaults to PascalCase for
types/traits/enums (S54); v1: signatures only —
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

**S55 — Built-in derive policy (M9)** *(ratified 2026-06-12; amended
2026-06-16, S82)*: **hybrid** derive policy for the four built-in traits.
**Auto-derive (silent):** `Printable`, `Equatable` — whenever every field
qualifies, `print("{p}")` and `==` work on day one; a hand-written `impl`
overrides the freebie. **Explicit opt-in:** `Comparable`, `Serialize` — require
a prefix attribute on the line before the type (S82):

```jet
@Comparable
@Serialize
struct Point {
    x: Float;
    y: Float;
}
```

**Configurable overrides** (D-JSON1): prefix `@Serialize` = automatic default
wire format; partial overrides go **inside the type body** as the first
statement(s):

```jet
@Serialize
struct Profile {
    @Serialize {
        rename score -> "user_score";
        skip internal_id;
    }
    name: String;
    score: Int;
    internal_id: String;
}
```

Comparable commits field order to sort/`largest`/`Map` ordering;
Serialize commits a public wire format — both are semantic commitments no
silent derive should make. Missing-trait errors teach `@Comparable` /
`@Serialize` or `sort_by` (M8) as alternatives. Rejected: auto-derive all
four (owner lean B), Rust `#[derive(…)]` attributes, prefix-line config
blocks (`@Serialize { … } struct …`), in-body `derive Trait;` (former S55
spelling), user-defined derive macros in v1 (S56 post-1.0).

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

**S43 — Test syntax (M6)** *(ratified 2026-06-12; amended 2026-06-16, S82)*:
top-level **`@test fn name { … }`** blocks (S82 attribute form), using
`**require**` and `**require_eq**` (M4/S36) for assertions. The test name is
the function identifier (replaces `test "name" { … }`). `jet run`/`build`
ignore test blocks; `jet test` runs them. Rejected: `#[test]` attributes,
`fn test_*` naming convention, quoted-name test blocks (former S43 spelling).

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
(M9):** built-in derives (S55 hybrid policy). **Layer 3 (Epoch 3):** typed reflection / user derives (S56) — see
[`docs/plans/epoch-3/user-derives-reflection.md`](../plans/epoch-3/user-derives-reflection.md).
**Rejected forever:** token/AST macros, custom syntax, attribute macros, comptime
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

**S58 — Expert low-level tier** *(ratified 2026-06-12; **amended 2026-06-16,
S82**; post-1.0 milestone pending)*: **two gates, one keyword.**
`**import core.mem**` is the discovery gate — unlocks the low-level
vocabulary: explicit **Zig-style allocators** (allocating APIs take an
allocator parameter; a fixed arena works on embedded), `**Ptr<T>**`,
layout/repr control, volatile wrappers. The audit gate for operations that can
violate memory safety — pointer **deref**, pointer math, transmute-class casts,
FFI pointer crossings — uses **`@unsafe { … }`** block form and **`@unsafe`**
on the line before `fn` (whole-function contract; calling one requires an
enclosing `@unsafe` block, Rust's rule). Optional **`@audit "…"`** on the line
before an `@unsafe` block carries a structured audit comment (D-LL2); a lint
flags missing/empty audits. Taking a pointer (`&x`) is legal outside a block (a
pointer is inert data); *using* one (`*p`, `.offset`) requires the block.
`&`/`*` are **core grammar, sema-gated**: outside the gates they keep producing
E0208-family teaching errors. Codegen lowers blocks to Rust `unsafe`; **I1 is
amended** — generated `unsafe` appears only inside user-gated regions plus
vetted std/mem internals. Onboarding materials never mention any of it.
Rejected: bare `unsafe { }` / `unsafe fn` (former S58 spelling), `trust`
spelling, library-only gating (Swift style), ungated sigils (C/Zig style).

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

**S51 — Core library (M10)** *(ratified 2026-06-12; amended
2026-06-13, **renamed `std` → `core` 2026-06-16**)*: the core library is
**exported as the `core` module** — a module `use` (S16 form 2, no quotes), not
a file path. `core` is the reserved short spelling for canonical package
`jet.core`; both spellings are valid. Dot paths select submodules:

```
use core;                         // whole core → namespace core
use jet.core as core;             // explicit canonical spelling
use core.fs as fs;                // submodule, optional alias
use core.io;                      // default namespace io
```

`core` and `jet.core` are compiler-reserved module roots; `core.<module>` and
`jet.core.<module>` select compiler-known submodules (`fs`, `io`, `json`,
…). Optional `as alias` works like S16. Core is never used via a quoted
path — `use "core/fs"` is wrong because `"core/fs"` is file-path syntax;
use `use core.fs`. The former spellings `import std` / `use std` /
`use std.fs` emit a teaching error pointing at `core` (S14). Rejected:
quoted core paths, separate `use core::` syntax, keeping the `std` module name.

**S54 — Naming convention** *(ratified 2026-06-12; **amended 2026-06-16**)*:
Jet and **`core`** default to **PascalCase** for types, traits, enums, and
constants (`Int`, `String`, `IOError`, `Fallible`, `Serialize`). Functions,
module path segments, and locals use **snake_case** (`read`, `core.fs`,
`my_var`) — the usual companion to PascalCase type names. Built-in type
capitalization is S11; standard acronyms are S66. v1 does **not** lint or
enforce naming on user code — `jet fmt` handles layout only (S44). Rejected:
mandatory snake_case lint; all-PascalCase for every identifier kind.

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
shared **hangar** store at **`/etc/jet/hangar/`**. The manifest reshape chunk
has since retired the old TOML constants (`jet.toml` / `jet.lock`) from
`src/syntax.rs` — a clean break, no alias; `PACK_FILE` (`pack.jet`) and
`UNIFIED_LOCK_FILE` (`.jet/lock`) are the only manifest/lock paths the compiler
knows. See `docs/plans/jetpack-jetos/unified-ecosystem.md`.

**S53 — Concurrency surface** *(ratified 2026-06-12; deferred past v1.0)*:
**deferred to v2** — no tasks, channels, or `std/tasks` in v1. When
implemented, the planned surface is ballot option A: `tasks.spawn(closure)
-> Task<T>`, `t.join() -> T`, `tasks.channel<T>()` with
`Sender`/`receive() -> T ? Closed`; no shared mutable state (ownership
rejects it). Rejected for v1: `go`-style `spawn { }` fire-and-forget,
shipping concurrency in v1.

**S59 — C FFI** *(ratified 2026-06-12; **amended 2026-06-16, D-CFFI2 + D-CFFI2-SYN**)*:
Epoch 2 (E2-M14) ships C import with **auto-generated bindings** (default) and
optional **user overlay** modules. By-value boundary first; pointers only inside
the S58 tier.

**Link resolution (D-CFFI2, ratified):**

1. **Jetpack project** — if `payload.jet` declares a matching dep (content-hash
   pinned in hangar), use hangar include/lib paths.
2. **Otherwise** — `pkg-config <link-name>`.
3. **Missing** — **E3201** naming both fixes.

**Surface (D-CFFI2-SYN, ratified 2026-06-16):**

| Layer | Shape |
|---|---|
| Autogen (compiler) | `@bindgen module c.<lib>.__bindgen__ { … }` in `.jet/bindings/c/<lib>.jet` |
| User overlay | `@extern module c.<lib> { … }` — empty `{ }` = no overrides yet |
| Call site | **`use … as alias`** (S16) — one form per lib per file |
| Script | `use "raylib.h" as rl` — compile-time bind on cache miss |
| Project | `use c.raylib as rl` — merged **bindgen ∪ overlay**; overlay wins on clash |

Link key = last segment **`<lib>`** (e.g. `raylib`). Reserved segment
**`__bindgen__`** — users cannot declare it; avoids namespace collision with
overlay modules.

```jet
// .jet/bindings/c/raylib.jet (generated — do not edit)
@bindgen module c.raylib.__bindgen__ {
    fn init_window(w: Int, h: Int, title: String) = "InitWindow";
}

// src/c/raylib.jet (optional overlay)
@extern module c.raylib {
    fn draw_text(text: String, x: Int, y: Int, size: Int, color: Color) = "DrawText";
}

// src/pong.jet
use c.raylib as rl;

fn main() {
    rl.init_window(800, 600, "pong");
}
```

**Bind timing (D-CFFI2-SYN-3):** compile-time check of `.jet/bindings/c/` + header
hash; invoke `jet bind` on cache miss/stale. Optional `jet bind` CLI for refresh.

Rejected: bare `extern c raylib { }` globals (S59 provisional A); shadow-only
override (overlay must merge with bindgen); two `use` forms for the same C lib
in one file. Rust FFI (S50) unchanged. See [`decision-ballots.md`](decision-ballots.md).

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

**D-JPK23 — git-dependency selector spelling in `pack.jet`'s `deps:` block**
*(ratified 2026-06-16)*: a git dependency is an inline struct value reusing
the struct-literal grammar `pack.jet` already has for `package:`/`deps:`:
`name: { git: "<url>", tag: "<tag>" }`, `{ git: "<url>", branch: "<branch>" }`,
or `{ git: "<url>", rev: "<rev>" }` — exactly one of `tag`/`branch`/`rev`.
This generalizes to **any** git remote (not just GitHub), closing the gap
where the `provider@target` grammar (`github@owner/repo/rev`) only covers
github.com and carries one ambiguous trailing segment. `provider@target`
stays the spelling for path/github/nixpkgs deps that don't need a selector;
the inline-struct form is only for git deps that do. Rejected: B (query-style
suffix on `provider@target`, e.g. `github@owner/repo?branch=main`) — a new
`?key=value` sigil with no precedent elsewhere in Jet syntax; C (drop
non-GitHub remotes and branch/tag tracking, rev-pin only) — a real capability
regression from the `jet.toml` baseline. Worked example:
```jet
deps: {
    textkit:  "1.2.0",
    helpers:  path@../helpers,
    parsekit: { git: "https://github.com/acme/parsekit", tag: "v0.4.1" },
    nightly:  { git: "https://github.com/acme/nightly", branch: "main" },
    selfhost: { git: "https://git.example.com/acme/thing", rev: "abc123" },
}
```

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
the `;`. See `examples/features/38_method_chain.jet`. The pipe operator `|>` is
**declined for now** (D-SUGAR2, 2026-06-16): S69 newline dot-chains cover the
readability need; revisit post-1.0 only with concrete evidence.

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
optionals; non-optional left side is E0047); **amended 2026-06-16 (D-SUGAR6):
`?.` extends to method calls** — `user?.display_name()` calls the method only
when the receiver is present and yields a `T?`, the natural completion of field
chaining; the former E0046 "no `?.` through methods" restriction is lifted (E0046
is retired); supersedes S35's `or`)*: `**?.`** optional
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
canonical order (S73). **Refutable binds (amended 2026-06-16, D-PAT3):** a
standalone bind whose pattern *can fail to match* — an enum variant, or
`value(n)` on a `T?` that might be `null` — must supply a `??` fallback
(`val value(n) = maybe_port() ?? return;`). Without the fallback the bind is a
compile error teaching `??` (or `when`/`if` to handle the empty case).

**S75 — Fan-out operator `f.[ … ]`** *(ratified 2026-06-16; implemented)*:
`f.[a, b, c]` desugars to `[f(a), f(b), f(c)]` — a postfix fan-out applying
one callable to several typed inputs written inline. Grammar:
`fanout = primary ".[" [ expr { "," expr } [","] ] "]"`. `.[` is a new
parser-level adjacency of `.` and `[`; `#` does not collide with any current
syntax. `f` must be callable with exactly one argument (user functions,
sources, type/enum constructors). Items are typed by `f`'s parameter type
(expected-type elaboration). The result is a `[T#N]` (S76). Splicing: a
fan-out inside an enclosing list literal flattens. `f.[*xs]` spread is
rejected in v1 — literal items only. Motivated by Blueprint north-star
(type-directed authoring). Supersedes "Stage 1b = Pkg sugar":
`default.[ripgrep, fd]` (U6) is one instance of the general fan-out.
Diagnostics E0961 (callee not one-arg callable) and E0962 (item type mismatch).
Rejected: a dedicated keyword, requiring parentheses around the bracket list.

**S76 — Fixed-size list type `[T#N]`** *(ratified 2026-06-16; implemented)*:
`[T#N]` is a compile-time refinement of `[T]` where `N` is a known constant
length, e.g. `[Point#2]`, `[Int#3]`. Rules: (a) `val` + literal/fan-out ⇒
`[T#N]` (length tracked); (b) `var` initialized from a literal/fan-out widens
to `[T]` (growable intent); (c) `[T#N]` widens to `[T]` implicitly when passed
to a `[T]` slot — one-way, safe; (d) `.map` preserves N: `[T#N].map → [U#N]`;
(e) `.len` on `[T#N]` is a compile-time constant; (f) length-changing ops
(`push`/`pop`/`insert`) are rejected on `[T#N]` with a teaching error pointing
at `[T]`; (g) positional destructuring of a `[T#N]` is compile-time
length-checked. `[T#N]` erases to `Vec<T>` at codegen (I3). `#` is the
fixed-size separator. Diagnostics
E0963 (destructure length mismatch), E0964 (length-changing op on fixed list),
E0965 (compile-time index out of range). Rejected: Rust `[T; N]` spelling
(`;` clashes with S6 statement terminators), angle-bracket `[T<N>]` (already
used for generics S33), keeping `[T]` for both sizes (loses static safety).
**Amended 2026-06-16 (VERSION-#, owner option 1):** `#` no longer "appears
nowhere else" — its unifying role is now **"`#` introduces a pinned number."**
That covers a list length (`[T#N]`) and a **package version** (`pkg#version`);
see the version-pin entry below.

**VERSION-# — `#` introduces a pinned number (version pins)** *(ratified
2026-06-16, owner option 1)*: a package version is pinned with `#` —
`textkit#1.2.0`, `github@acme/parsekit#v0.4.1`. The **source** selector stays
`:` / `@` (`nixpkgs:fastfetch`, `github@owner/repo`; D-JPK7/15 unchanged — we
still don't write Nix's `nixpkgs#fastfetch` for *sources*); `#` attaches the
*version number* on the end. This is thematically consistent with S76, where `#`
already reads as "a specific count/number": `[Point#2]` = "2 points",
`parsekit#1.2.0` = "version 1.2.0". The richer inline-struct dep form
(D-JPK23, `{ git: "…", tag: "…" }`) remains for git deps needing branch/rev
selectors; `pkg#version` is the terse pin for simple semver. Rejected: option 2
(version only inside the dep struct, no `#`), option 3 (push `#` onto the source
selector too — discards the deliberate "don't look like Nix" rule and overloads
`#` ambiguously).

**S77 — Struct field punning** *(ratified 2026-06-16; milestone pending)*: in a
struct literal `Type { … }` (S29), a bare field name is shorthand for
`name: name` when a binding of that name is in scope —
`Source { name, upstream, via: "nix" }` ≡
`Source { name: name, upstream: upstream, via: "nix" }`. Matches Rust
field-init shorthand and Nix `inherit`. Field checking is unchanged (S29):
every field still required exactly once; punned and explicit fields mix freely.
An unknown punned name is the ordinary "no such binding" error. Rejected:
JS-style `{ name }` object shorthand outside struct literals, punning with no
matching binding.

**S78 — Contextual empty-list inference** *(ratified 2026-06-16; milestone
pending)*: a bare `[]` (S37) infers its element type from the expected type at
its use site — a generic call argument, an accumulator/return type, or an
annotated binding — so `fold(xs, [])` and a `-> [Int]` body that returns `[]`
need no annotation. When no expected type is available, the explicit form
`[]: List<T>` / `[]: [T]` (S65) is still required and **always accepted**
(owner: keep explicit typing available). Mirrors `null` and empty-map `[:]`
inference (S32/S38). Rejected: defaulting `[]` to a placeholder element type,
requiring an annotation in every position.

**S79 — Expressions in `for … in <expr>` heads** *(ratified 2026-06-16;
milestone pending)*: the iterable in `for x in <expr> { … }` (S19) may be any
expression yielding an iterable — field access, method/function calls,
indexing, ranges — e.g. `for p in shape.points()`, `for c in row[i].chars()`,
`for n in lo..hi`. The head expression is evaluated once before the loop.
Rejected: restricting the head to a bare name or literal range.

**S80 — Error carrier & fallible `main`** *(ratified 2026-06-16; amends
S34/S12; milestone pending)*: the default **`Error`** type (S34) grows from a
`String` wrapper to a structured carrier — a **message**, an **optional code**,
and an **optional source** (the lower-level error it wrapped) — so context
survives as an error travels up through `?`. Beginners still write
`-> Config ?` and get the default **`Error`**; the richer fields are
constructors/accessors used only when wanted (`Error.message("…")`,
`Error.code(n)`, `Error.with_source(e)`). **Fallible `main`:** `fn main() -> Unit ?`
is allowed (amends S12); a returned **`Error`** is printed in the standard
diagnostic voice and the process exits non-zero. **Cross-type `?` conversion
(D-ERR2, ratified 2026-06-16):** opt-in via the **`Fallible`** trait —
`impl MyFail: Fallible { fn to_error(self) -> Error { … } }`. The default
**`Error`** type is both what bare `-> T ?` returns and the target of
`Fallible` conversion. Prelude types (`String`, std I/O errors, …) implement
**`Fallible`** by default; unrelated enums do not convert silently. Rejected:
message-only carrier, keeping `String`-only, a non-fallible `main` as the only
form, naming the conversion trait **`Error`** (collides with the type), separate
carrier type names (`Fault`, `Snag`, …).

**S81 — `?continue` loop skip** *(ratified 2026-06-16; milestone pending)*:
inside a `for`/`while` body, postfix **`?continue`** on a fallible or optional
value skips to the next iteration when the value is failed/empty and binds the
success value otherwise — `val line = next()?continue;` reads "take the next
line, or skip this turn". A loop-scoped sibling of `?` propagation (S7) and
`??` fallback (S71); legal only inside a loop (outside → teaching error).
`?break` is **not** added in v1 (write `?? break`). Rejected: deferring the
feature (owner chose to add it), a method `.or_continue()` form.

**S82 — Attribute syntax (`@` markers)** *(ratified 2026-06-16; ATTR-SHAPE,
D-LL2, D-JSON1)*: **`@` not `#`** — declaration markers and scoped effects
share one sigil; **position disambiguates**.

| Form | Meaning |
|---|---|
| `@Marker` | single attribute, line immediately before a declaration |
| `@[Marker, Marker, …]` | comma-separated list on that prefix line |
| `@Marker { … }` | scoped effect region (statement in a function body), **or** in-body config (first lines inside a type body) |

**Declaration markers** — `@Marker` or `@[…]` on the line before `struct`,
`enum`, or `fn`. Covers derive-like markers (`@Serialize`, `@Comparable`),
harness markers (`@test`, `@todo`), and whole-item effects (`@transact`,
`@unsafe` on a function). **`pure fn`** and **`comptime`** bindings stay prefix
keywords (not migrated to `@`).

**Scoped effects** — `@Marker { … }` as a statement inside a function (`@transact
{ … }`, `@unsafe { … }`, `@async { … }` reserved for Epoch 3). Same spelling as
in-body config; parser distinguishes by context.

**Configurable markers (D-JSON1):** prefix `@Serialize` (etc.) = automatic
default; partial overrides go **inside the type body** as `@Serialize { rename …;
skip …; }` — **not** on the prefix line. Rejected: `#[…]` Rust-style attributes,
prefix-line config blocks.

**LSP (owner requirement):** surface every attribute applicable to the item
under the cursor — prefix attrs, in-body config attrs, and scoped blocks —
via hover, inlays, and completion filtered by item kind.

```jet
@Serialize
struct Profile {
    @Serialize { rename score -> "user_score"; skip internal_id; }
    name: String;
    score: Int;
    internal_id: String;
}

@[Comparable, Serialize]
struct Score { value: Int; }

@test
fn reversing_twice(xs: [Int]) {
    require_eq(reverse(reverse(xs)), xs);
}

fn try_move(player: mut Player, target: Point) -> Bool ? {
    @transact {
        player.spend_stamina(10)?;
        player.step(target)?;
    }
    return ok(true);
}

@audit "bounds checked against len"
@unsafe { slice.get_unchecked(i); }
```

**S83 — Multi-head functions (D-PAT5)** *(ratified 2026-06-16)*: a function may
have **multiple heads** — same name, different parameter **patterns** — each
with its own body. Dispatched by matching the call argument shape (Haskell/
ML-style case analysis on definitions):

```jet
fn area(Circle(r: Float)) -> Float {
    return 3.14 * r * r;
}

fn area(Rect(w: Float, h: Float)) -> Float {
    return w * h;
}

fn eval(Lit(n: Int)) -> Int {
    return n;
}

fn eval(Add(a: Int, b: Int)) -> Int {
    return eval(a) + eval(b);
}
```

Heads must be **exhaustive** for the types they collectively cover (same rule
spirit as enum `when`). **`when` inside a single body remains valid** — two
branching forms coexist by owner choice (D-PAT5 = accept B). Rejected: deferring
multi-head forever (owner prefers the math/recursion ergonomics).

### Unified ecosystem — `jet` + `jetpack` + `jetos` (U-series)

The owner-ratified design-of-record is
`docs/plans/jetpack-jetos/unified-ecosystem.md` (status: owner-ratified,
2026-06-16). Its naming ledger (§10) and the U-series below are **ratified**.
These records establish the authoring-surface tokens; behavior lands in the
Jetpack/Jetos implementation chunks (no syntax is invented beyond this).

**U1 — Package manifest is `pack.jet`** *(ratified 2026-06-16; **amended by
U10** — renamed `payload.jet`, identity block `payload:`, `packages:` model)*:
the package manifest is written in **Jet syntax** (not TOML), holding package
identity + Jet library deps (+ exported packages). It **replaces** `jet.toml`.
**Amends S52.** Rejected: keeping a separate TOML manifest beside a Jet pack file
(two manifest languages). *(The filename `pack.jet` and `package:` identity block
named here are superseded by U10's `payload.jet` / `payload:`.)*

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

**U8 — `sources:`/`imports:` nest inside the module body** *(ratified
2026-06-16; amends U4)*: a module's `sources:` and `imports:` are fields **inside
`module name { … }`**, siblings of the typed contributions (`env.dev: Env { … }`)
— not file top-level fields. `module` stays the single outermost construct (like
a flake's one `{}` holding both inputs and outputs). `sources:` holds
`name: provider@target` entries (merged by key across modules per U5);
`imports:` holds `find(…)` directives (the U4 discovery walk). **Amends U4** (its
example previously showed `imports:` at file top level) and supersedes the
top-level `sources:`/`imports:` shape sketched in unified-ecosystem.md §2.2.
Rejected: file top-level manifest fields (would make a second top-level construct
beside `module`, and require a bespoke env-file parser à la `pack.jet`).

**U9 — A source's provider kind is *inferred*, never declared** *(ratified
2026-06-16)*: a named source is always just `name: provider@target` — there is
**no `via:`/kind marker** in the surface (this dissolves the former open
question). Whether a source realizes through the first-party **core** provider
or falls back to a **nix** flake is discovered from its target: a target that
has a **`pack.jet`** is a Jet package repo → core; otherwise → nix flake. The
probe is cheap and never clones a nixpkgs-sized repo: `path@…` stats locally,
`nixpkgs@…` is unconditionally nix (never probed), and `github@…`/git URLs peek
at **only** `pack.jet` (raw fetch / shallow `git archive`) before committing to a
full fetch. **core-by-default with a safe nix fallback** keeps the syntax clean
and gives every env the whole nixpkgs repo for free. See unified-ecosystem.md §6.
Rejected: a per-source `via: core` field, an inline `via` keyword, and a `core@…`
provider prefix (all add ceremony to express what the target already tells us).

**U10 — Manifest is `payload.jet`; payload → packages → modules** *(ratified
2026-06-16; amends U1)*: the package manifest is renamed **`payload.jet`** (was
`pack.jet`) and its identity block is **`payload: { name, version, … }`** (was
`package:`). A **payload** is a collection of **packages**; a **package is a
top-level `module`** — the unit a dev exports — which may contain public/private
submodules (scoping is the module system's job). A payload lists its packages in
a **`packages: { … }`** block whose entries are `name: kind`, where *name* is a
top-level module name and *kind* is **`library`** (consumers *import* it for its
code — the build-graph axis) or **`executable`** (consumers *install* it as a
binary on PATH — the nix-flake devshell case). The value is either a **bare
keyword** (`deploy: executable`, defaults assumed) or a **block** (`deploy: {
kind: executable, … }`) — the block is the extension point for advanced
per-package config (a `bin` name, an explicit entry module, a per-package
version), each new field gated by this protocol. A package's **module name is its
identity** (robust to file moves; renaming the module is the real breaking
change); its file is **discovered** by recursively walking the payload tree
(sorted + bounded — skip `.jet/`, hidden, and build dirs) for the `module <name>`
declaration. Each name must resolve to **exactly one** module — zero matches or
an ambiguous duplicate is a diagnostic. `jet new`/`jet init` always scaffolds
`packages:` (explicit-by-default; the user edits it only to add/remove). This
`packages:` index is what the **core** provider reads to resolve a requested
package → its source, **replacing** the misplaced `env.jet` `pkg.package(...)`
index (this completes the U9 marker convergence): `env.jet` stays purely the
dev-shell (devenv) descriptor and is **never** read by the provider. The old
`exports: [module …]` list (U1) folds into `packages:`. **Realize timing:** no
Jet→binary compiler exists yet — today `library` packages stage their module
source and `executable` packages stage a prebuilt `bin/`; the native compiler
slots into the same realize boundary later (manifest is designed for the
end-state now). **Amends U1** (manifest filename + identity block). Rejected: a
`package:`-singular manifest with hand-written package *paths* (fragile to file
moves); a separate `via:`/path index; and reusing `env.jet` as a package index
(category error — a dev shell is not an export list).

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
| S56 | typed reflection / user derives | **Epoch 3** — [`docs/plans/epoch-3/user-derives-reflection.md`](../plans/epoch-3/user-derives-reflection.md) |
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
are fully ratified above. **S59 (C FFI)** ships in **Epoch 2** (E2-M14). **S53**
(concurrency) is ratified as deferred past v1.0. S60 is ratified post-1.0. S56
(user derives via typed reflection) is deferred past v1.0 by S26's ratified layering.

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
| 2026-06-16 | S51 | amended: std library module renamed **`core`** (`jet.core`); `import std` → teaching error | owner |
| 2026-06-16 | S54 | amended: PascalCase default for types/traits/enums/constants; snake_case for fn/module/local; no user lint | owner |
| 2026-06-12 | S54 | no prescribed naming convention in v1        | owner |
| 2026-06-12 | S52 | `jet.toml` manifest; `jet.lock`; jet add/fetch | owner |
| 2026-06-13 | S52 | amended: `[dependencies:*]` colon tables, lock graph, `@latest`, `.jet/` folder, useful `jet new` template | owner |
| 2026-06-12 | S53 | concurrency deferred to v2; option A when built | owner |
| 2026-06-16 | S59 | amended: E2-M14 ships C FFI (`@bindgen`/`@extern module`, `use c.<lib>`); no longer deferred past v1 | owner |
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
| 2026-06-16 | U8  | `sources:`/`imports:` nest inside `module {}` (siblings of contributions), not file top-level; amends U4 | owner |
| 2026-06-16 | U9  | source provider kind inferred (`pack.jet` → core, else nix flake); no `via:` marker; manifest-only remote probe | owner |
| 2026-06-16 | U10 | manifest renamed `payload.jet`; `payload:` identity block; `packages: { name: library\|executable }` (bare or block); package = top-level module discovered by name; replaces `env.jet` pkg index (completes U9); amends U1 | owner |
| 2026-06-16 | S52 | amended: `pack.jet`/`.jet/lock` (U1/U2); hangar store `/etc/jet/hangar` | owner |
| 2026-06-16 | S75 | fan-out operator `f.[ … ]`; `.[` adjacency; E0961/E0962        | owner |
| 2026-06-16 | S76 | fixed-size list type `[T#N]`; `#` separator; erase to Vec; E0963–E0965 | owner |
| 2026-06-16 | D-JPK23 | git deps in `pack.jet` `deps:` are inline structs `{ git: "...", tag/branch/rev: "..." }`; any remote, not just GitHub | owner |
| 2026-06-16 | S77 | struct field punning `Type { name }` (D-FP1)         | owner |
| 2026-06-16 | S78 | contextual empty-list inference; explicit `[]: [T]` kept (D-FP4) | owner |
| 2026-06-16 | S79 | expressions allowed in `for … in <expr>` heads (D-FP5) | owner |
| 2026-06-16 | S80 | rich `Error` carrier (msg+code+source); fallible `main` (D-ERR1/D-ERR3) | owner |
| 2026-06-16 | D-ERR2 | cross-type `?` via **`Fallible`** trait; default type stays **`Error`** | owner |
| 2026-06-16 | D-DEV2 | JIT runtime type server → Epoch 3 pillar; Epoch 2 interpreter-only | owner |
| 2026-06-16 | D-FP2 | defer expression-body `fn … = expr;` (use `{ return …; }` or lambdas) | owner |
| 2026-06-16 | D-REF3 | LSP: borrowed-return + cleanup-scope inlay hints on by default | owner |
| 2026-06-16 | D-DX5 | PATH `jet-*` discovery now; formal plugin API → Epoch 3 | owner |
| 2026-06-16 | D-PAT5 | multi-head functions (S83); `when` + heads both allowed | owner |
| 2026-06-16 | D-PURE1 | pure eval + sandboxed package build blocks in Epoch 2 | owner |
| 2026-06-16 | D-PURE2 | no ambient I/O/network in pure eval; `embed_file` only | owner |
| 2026-06-16 | D-TOOL4 | snapshot testing; `-u` / `--update-snapshots` | owner |
| 2026-06-16 | D-S16-USE | S16 amended: **`import` → `use`**; E0015 teaches `import` | owner |
| 2026-06-16 | D-CFFI2-SYN | `@extern module c.<lib>` overlay + `@bindgen module c.<lib>.__bindgen__`; `use` at call site | owner |
| 2026-06-16 | D-CFFI2-SYN-1 | one C `use` form per lib per file | owner |
| 2026-06-16 | D-CFFI2-SYN-2 | empty overlay = no overrides; `__bindgen__` reserved for autogen | owner |
| 2026-06-16 | D-CFFI2-SYN-3 | compile-time bind on cache miss; `.jet/bindings/c/` | owner |
| 2026-06-16 | D-CBIND1 | tool-generated `.jet` in cache (S59 default) | spec |
| 2026-06-16 | D-CBIND4 | `Ptr<T>` for C pointers (S58) | spec |
| 2026-06-16 | D-CBIND7 | cache dir `.jet/bindings/c/` (D-CFFI2-SYN-3) | spec |
| 2026-06-16 | D-CBIND8 | encourage curated registry packages | spec |
| 2026-06-16 | D-CFFI2-SYN-4 | merge bindgen ∪ overlay; overlay wins on clash | owner |
| 2026-06-16 | D-CFFI2 | hangar-if-dep else pkg-config (S59 link resolution) | owner |
| 2026-06-16 | D-NET2 | Go-scale async/concurrency → Epoch 3 pillar | owner |
| 2026-06-16 | E2-V12 | retired — split across D-PURE + Epoch 3 pillars | owner |
| 2026-06-16 | S56 | user derives / typed reflection → Epoch 3 (layer 3) | owner |
| 2026-06-16 | S83 | multi-head function patterns (D-PAT5) | owner |
| 2026-06-16 | S82 | `@` attribute syntax; amends S43/S55/S58 (ATTR-SHAPE, D-LL2, D-JSON1) | owner |
| 2026-06-16 | S81 | `?continue` loop skip (D-ERR4)                       | owner |
| 2026-06-16 | S31 | amended: nested patterns in payload slots (D-PAT1)   | owner |
| 2026-06-16 | S74 | amended: refutable bind requires `??` fallback (D-PAT3) | owner |
| 2026-06-16 | D-SUGAR2 | pipe `\|>` declined for now; dot-chains (S69) cover it | owner |
| 2026-06-16 | D-SUGAR4 | newtype keyword declined; one-field struct covers it | owner |
| 2026-06-16 | S31 | amended: `&&`-guards reuse pattern-bound names; no guard keyword (D-PAT2) | owner |
| 2026-06-16 | S71 | amended: `?.` extends to method calls; E0046 retired (D-SUGAR6) | owner |
| 2026-06-16 | S76 | amended: `#` = "pinned number" role; adds `pkg#version` (VERSION-#) | owner |
| 2026-06-16 | VERSION-# | version pins use `#` (`pkg#version`); source selector stays `:`/`@` | owner |
| 2026-06-16 | D-SUGAR3 | transparent type alias declined for now (use newtype/struct) | owner |
| 2026-06-16 | D-SUGAR5 | `defer` keyword declined; RAII (S63) is the cleanup story | owner |
| 2026-06-16 | D-FP6 | list spread `[...xs, y]` deferred; use `.concat`/`.with` for now | owner |
| 2026-06-16 | D-PAT6 | parameter destructuring deferred; unpack on first body line | owner |
