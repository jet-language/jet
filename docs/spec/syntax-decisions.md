# Syntax Decisions (the owner's control surface)

**The owner has final say on all user-facing syntax.** Agents implement
only what is Ratified, may rely on Provisional choices (clearly marked,
reversible), and must never invent surface syntax. To propose something
new: add a row to Open Decisions with options and tradeoffs, and stop.

How to ratify: move the row to Ratified with your chosen option. Agents
then update `Source/Syntax.rs` (and parser if structural), re-bless ui
snapshots (`UPDATE_EXPECT=1 cargo test`), and update docs/spec/spec.md.

**Ratify = then build it, end to end.** An owner ballot answer on a decision with no
open upstream gate **is the "go"**: implement it fully — parser → sema → codegen, a
`tests/ui` snapshot for every diagnostic (I4), a golden-tested `examples/` entry where
user-visible (I5), all `cargo test` green — not a doc edit alone. A ratified entry may
sit **"milestone pending" / `src/` untouched ONLY when it is gated on another decision
that is still unratified** (e.g. a feature waiting on `D-EFF1`); name the gate in the
entry. "Ratified but unbuilt with no open gate" is not an allowed state.

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

**S2 — Variable bindings (M1)** *(ratified 2026-06-11; **superseded 2026-06-18
by D-BIND1**)*: bindings use **Odin-style sigils** — `**name :: expr`** for an
immutable binding, `**name := expr**` for a mutable binding, with an optional
type annotation before the sigil (`ratio: Float :: 3.14`, `count: Int := 0`).
`=` stays reassignment of an existing `:=` binding (S17). The former keywords
`**val**` / `**var**` are **retired to teaching errors** (E_KEYWORD_RETIRED →
"use `name :: value` / `name := value`"). Rejected: `set` (sounds like
mutation), `let` / `let mut` (Rust; teaching errors only per S14), and the
partial `:=`-only adoption that kept `val` (D-BIND1 option B). The owner
accepted **spending the `::` token** on immutable bindings — see D-BIND1; S83
(external definitions) must now pick a different separator.

**S18 — Visibility** *(ratified 2026-06-11)*: **private by default**;
prefix `**pub`** to export an item. Applies to top-level functions (M0+),
types and their fields (M3), and any future module-level bindings.
Within a file, private and `pub` items are equally visible to each other;
`pub` only controls what other files may access via `use` (S16, M6+).
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

**S6 — Statement separators** *(ratified 2026-06-11; **superseded 2026-06-18 by
S6-R = B**)*: **no visible semicolons.** A statement ends at the end of its
line; the **lexer inserts** a synthetic terminator (Go-style) after any line
whose last token can end a statement — identifier, literal, `break`,
`continue`, `return`, `)`, `]`, `}`. The grammar and diagnostics stay
terminator-based — the `;` token still exists internally; users just never type
it. **One layout rule** replaces the per-line `;`: a `-> Type` return
annotation and the opening `{` must stay on the same line as the parameter
list's closing `)` (a newline before `->` would insert a terminator after `)`).
**Continuation suppression (ratified 2026-06-18):** a terminator is **not**
inserted when the **next non-blank line begins with `.`** (continues an S69
method/field chain) or with a **binary/logical operator** (`&&`, `||`, `+`, `-`,
`*`, `/`, `%`, `==`, `!=`, `<`, `>`, `<=`, `>=`, …), so multi-line dot-chains
(S69) and line-broken expressions keep parsing. With the `->`/`{` rule, these
are the only suppression cases. Rejected: keeping required semicolons (S6-R
option A — the original rule), significant indentation, optional-`;`-before-`}`.
See S6-R.

**S12 — Entry point** *(ratified 2026-06-11)*: `**fn main()`** — a special
case; no `pub` required (the runtime always finds `main`). Canonical form
omits `pub`. Rejected: required `pub fn main` (ceremony), top-level
statements without a main.

**S19 — Loops (M1)** *(ratified 2026-06-11; **amended 2026-06-17, S19-amend
option A**)*: **one keyword, `loop`; the header picks the mode.**
Empty header = infinite; a boolean expression = conditional; `in` = iteration:

```jet
// infinite
loop {
    val line = read_line();
    if line == "quit" { break }
    print(line);
}

// conditional (replaces `while`)
var n = 10;
loop n > 0 {
    print(n);
    n = n - 1;
}

// iteration (replaces `for … in`)
loop i in 1..5 {
    print(i);
}
```

`while` and `for` are retired — recognized only for S14 teaching errors pointing
at `loop`. `break` and `continue` (S23) are unchanged.
Rejected: keeping three keywords (`loop`/`while`/`for`) as status quo (B),
folding only infinite + conditional while keeping `for` separate (C).
*Original (2026-06-11):* `while cond { }` and `for i in <range> { }`. Rejected:
recursion-only M1, `loop` + `break` as the primary construct.

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

**S24 — Many-way choice (M1)** *(ratified 2026-06-11; keyword amended to `when`
2026-06-15, D-SG1; **superseded 2026-06-18 by D-IF1**)*: the `when` **keyword is
retired** — multi-arm dispatch is now spelled `if subject { arm -> body … }`
(S68), and `when` → teaching error E_KEYWORD_RETIRED pointing at `if`. The arm
grammar described below is **unchanged** and now lives under `if` (S68); D-IF1
additionally adds the **inferred comparator** (a bare value arm `200 -> …` means
`subject == 200`), which reverses the bare-value-match rejection recorded here.
Original entry retained for the arm semantics:

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
position.

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
*Future relook (owner, 2026-06-15; updated 2026-06-18):* the owner may revisit
trait-attach sugar post-v1 (e.g. a C++-ish `Type::Trait` feel). Constraint:
`::` is **no longer available** — D-BIND1 (2026-06-18) made `name :: expr` the
immutable-binding sigil, so any trait-attach sugar must pick a different
separator — S83 picked `~~` (`impl Point~~Trait`). `::` inside `extern rust "rust::path"` strings
(S50) is unaffected — it lives inside a string literal, not a bare token.

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
max between items, no space before `,`/`(` of a call; no visible `;`
(S6/S6-R — the lexer inserts terminators). `jet fmt` is the only formatter; no style knobs. Rejected:
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
[`tools/Tower/docs/plans/epoch-3/user-derives-reflection.md`](../../tools/Tower/docs/plans/epoch-3/user-derives-reflection.md).
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
S82, D-LL2**)*: **two gates, one keyword.**
`**use core.mem**` is the discovery gate — unlocks the low-level
vocabulary: explicit **Zig-style allocators** (allocating APIs take an
allocator parameter; a fixed arena works on embedded), `**Ptr<T>**`,
layout/repr control, volatile wrappers. The audit gate for operations that can
violate memory safety — pointer **deref**, pointer math, transmute-class casts,
FFI pointer crossings — uses **`#Audit("…")`** then **`#Unsafe { … }`** (D-LL2:
audit required; lint **L3101** if missing). **`#Unsafe`** on the line before `fn`
marks a whole-function contract; calling one requires an enclosing `#Unsafe`
block. Taking a pointer (`&x`) is legal outside a block (a pointer is inert
data); *using* one (`*p`, `.offset`) requires the block. `&`/`*` are **core
grammar, sema-gated**: outside the gates they keep producing E0208-family
teaching errors. Codegen lowers blocks to Rust `unsafe`; **I1 is amended** —
generated `unsafe` appears only inside user-gated regions plus vetted std/mem
internals. Onboarding materials never mention any of it.
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

**S84 — Hyphens in package / module / system / image / env *names***
*(ratified 2026-06-16, finalist 2)*: **names in these positions may be
kebab-case** — `image.halcyon-iso`, `system.my-host`, `module web-app` —
matching the nixpkgs/npm package-name convention users already know. The grammar
is a **dashed name** `ident (-ident)*`: a `-` joins two segments only when it is
**span-adjacent** to both — `prev.end == minus.start` and
`minus.end == next.start`, i.e. no surrounding whitespace. That span-adjacency
rule is the whole safety mechanism: a spaced `a - b` stays **subtraction**, so
there is **no lexer change and no expression-grammar change** — the hyphen
handling lives entirely in the parser's `expect_dashed_name`, used only in name
positions (contribution names, `from: system.<name>` references, the `module`
declaration name; package names in `pkg.jet` are already hyphen-transparent
in the manifest parser). No leading, trailing, or doubled hyphen (`image.-iso`,
`image.a--b` produce the ordinary teaching diagnostic, never an ICE). **Code
identifiers** — variables, fields, types, functions — stay plain `ident`. No new
sigil (reuses the `-`/Minus token; recorded as `NAME_SEGMENT_SEP` in
`Source/Syntax.rs` per I7). Rejected: finalist 1 (underscores only, status quo) —
the ratified worked `config.jet` and the nixpkgs/npm convention both write
hyphens; a lexer-level identifier change (would break `a - b`).

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
compiler (I6). Full layout and architecture in tools/Tower/docs/plans/jetpack-jetos/unified-ecosystem.md (§10–11).

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
`Source/Syntax.rs` — a clean break, no alias; `PACK_FILE` (`pack.jet`) and
`UNIFIED_LOCK_FILE` (`.jet/lock`) are the only manifest/lock paths the compiler
knows. See `tools/Tower/docs/plans/jetpack-jetos/unified-ecosystem.md`.

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

1. **Jetpack project** — if `pkg.jet` declares a matching dep (content-hash
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

**Bind timing (D-CFFI2-SYN-3 / D-CBIND2):** compile-time check of `.jet/bindings/c/` +
header hash; invoke **`jet bind`** (same backend) on cache miss/stale; manual
**`jet bind`** subcommand for refresh.

**Bind engine (D-CBIND3):** bindgen-based helper (I6 waiver). Optional libclang fallback later.

**C strings (D-CBIND5):** `const char*` / `char*` at the edge → **`String`** (copy in/out);
lifetime-heavy cases → overlay or gated pointers.

**C macros (D-CBIND6):** bind emits **`#define` constants only**; skip function-like macros;
overlay for macro-wrapped symbols.

**Unsafe audit (D-LL2):** `#Audit("reason")` required on `#Unsafe { … }` blocks.

Rejected: bare `extern c raylib { }` globals (S59 provisional A); shadow-only
override (overlay must merge with bindgen); two `use` forms for the same C lib
in one file. Rust FFI (S50) unchanged. See [`decision-ballots.md`](decision-ballots.md).

**S60 — Pure-function marking** *(ratified 2026-06-12; post-1.0 milestone
pending)*: `**pure fn name(…)**` — a checked modifier; purity is part of
the signature; violations are compile errors naming the impure call path.
Enables `jet eval --pure` (layer 3, post-v1) and makes comptime
callability visible at API boundaries. Rejected: inference-only purity with
no marking. **Reopened by D-EFF1 (2026-06-22):** the "no full effects system"
stance is reversed — Jet now has an inferred, erased effect system; `pure fn`
survives unchanged as the empty effect set (⊥ of the lattice). **Naming note (D-CT-L2NAME):** "Layer 2"
here is the S60 *capability tier* (compile-time pure evaluation + data
embedding), **not** the S26 derive-ladder Layer 2 (built-in derives). The two
share the name by accident; compile-time embedding work files under S60.

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
The salvage record was folded into the jetpack plan and retired; its detail
lives in git history (`tools/Tower/docs/plans/jetpack-jetos/forge-salvage.md`).

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
names. Provider roadmap: `tools/Tower/docs/plans/jetpack-jetos/README.md` §3.3 (the
native-resolver design doc was folded in and retired; detail in git history).
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

**D-DEV4 — `jet dev` vs `jet env`** *(ratified 2026-06-17)*: the two jobs that
were colliding on the name `jet dev` get **two distinct verbs**, following the
prior art each one belongs to:

- **`jet dev`** = the **watch / interpret code loop** (E2-M4) — re-check and
  re-run the project entry on every save, sub-200ms feedback, interpreter-backed.
  This matches the JS/Bun/Deno convention where `dev` means "run my code with
  reload" (`bun --watch`, `deno run --watch`, `vite dev`).
- **`jet env`** = **drop into the project's development shell** built from
  `env.jet` (delegates to `jetpack enter`). This is the environment-provisioning
  job, the analog of `nix develop` — given its own word because it is a separate
  product, not a mode of running code.

This resolves the collision by **renaming the shipped shell-enter verb from
`jet dev` to `jet env`** (a clean break, no alias) and **reserving `jet dev`
for the E2-M4 loop**. Until E2-M4 lands, a bare `jet dev` reports that the watch
loop is not yet built and names `jet run`/`jet build` meanwhile; it is not yet
advertised in completions/man (honesty bar — advertised surface equals working
surface). Chosen over: A (disambiguate one verb by argument presence — one word
with two meanings is less discoverable); B (`jet watch` for the loop, leave
`jet dev` = shell — spends a second-class verb on what users reach for as
"dev"). The shipped `jet dev` → `jetpack enter` docs/behavior migrate to
`jet env`.

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
**Amended 2026-06-18 (D-IF1): `if` is the universal branching keyword.** Beyond
the boolean two-arm form and the expression form above, `**if subject { arm ->
body … }**` is **multi-arm dispatch** — the former `when` (S24), now folded into
`if`. `when` is retired to a teaching error. **Inferred comparator (owner
directive):** a bare value arm is implicitly compared against the subject, so
`if code { 200 -> …; 404 -> …; }` means `code == 200` / `code == 404`; arms may
be bare values *or* full `Bool` conditions (this reverses S24's bare-value-match
rejection). **Surface (D-IF2, ratified 2026-06-18):**
- **Catch-all arm: `else -> body`** (not `...`) — reuses the keyword the
  two-arm `if`/`else` already owns; no new sigil, and `...` stays free for a
  possible future spread/rest meaning.
- **Braceless arm bodies allowed:** `200 -> print("ok")` for a single
  expression; a `{ … }` block for a multi-statement body. Keep the simple case
  clean.
- **Bare-value vs condition — structural mix (Q3 = A):** an arm head with **no
  top-level comparison/logical operator** is a bare value (the compiler prepends
  `subject ==`); an arm containing one is a full `Bool` condition. The two mix
  freely in one block.

Arm termination follows S6-R = B (no semicolons). Exhaustiveness/type checks are
unchanged from S24.

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

**D-JPK-FILES — Jetpack file structure** *(ratified 2026-06-18; revises U1/U10)*:
the jetpack project layout is three named files plus one managed folder:

| File | Format | Location | Role | Checked in? |
|---|---|---|---|---|
| `jetpack.toml` | TOML | repo root | monorepo manifest: `[repo]`, `[sources]`, `[packages]` index | yes |
| `env.jet` | Jet | repo root | dev environment: sources + packages + shell prompt | yes |
| `pkg.jet` | Jet | each package dir (user-chosen) | package identity: `payload: { name, version }` + `packages: { name: library\|executable }` | yes |
| `.jet/lock` | TOML | `.jet/` | generated lockfile (resolved deps + fingerprints) | no |
| `.jet/cache/` | — | `.jet/` | generated build cache | no |

Rules:
- `jetpack.toml` + `env.jet` live at **repo root** (convention parity with
  `Cargo.toml`/`package.json`/`flake.nix`; discoverable, tool-findable).
- `pkg.jet` files live wherever the user organizes packages (flat, nested, deep
  monorepo); discovery is `find . -name pkg.jet`. One per publishable package.
- `.jet/` holds **only generated state** (lock, cache); never source manifests.
- Provider-kind inference (U9) probes for `pkg.jet` (was `payload.jet`).

**Renames from U10:** package-manifest filename `payload.jet` → **`pkg.jet`**;
new TOML monorepo manifest **`jetpack.toml`** added at root; `config.jet` (jetos
tier) deferred to Epoch 3. The `payload: { … }` identity **block name inside
`pkg.jet` is unchanged**. Implementation: `PAYLOAD_FILE` const in `Source/Syntax.rs`
and the loader/manifest/jetpack modules retarget `payload.jet` → `pkg.jet`; a
`jetpack.toml` TOML parser is new work (see the implementation note below).
Rejected: keeping `payload.jet` (poorer parity, conflated with `payload:` block
name), putting `jetpack.toml`/`env.jet` under `.jet/` (breaks root-manifest
convention, hurts discoverability).

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
`#Unsafe` on a function). **`pure fn`** and **`comptime`** bindings stay prefix
keywords (not migrated to `@`).

**Scoped effects** — `@Marker { … }` as a statement inside a function (`@transact
{ … }`, `#Unsafe { … }`, `@async { … }` reserved for Epoch 3). Same spelling as
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

#Audit("bounds checked against len")
#Unsafe { slice.get_unchecked(i); }
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
`tools/Tower/docs/plans/jetpack-jetos/unified-ecosystem.md` (status: owner-ratified,
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

**U11 — `System` field set** *(ratified 2026-06-16; D-SYS-FIELDS A)*: a `System`
(a jetos machine) has four fields — **`target`** (a typed platform value,
`linux.x64` / `linux.arm64`, *not* a string; see U13), **`packages`** (a `Pkg`
list; U6 sugar applies), **`services`** (a keyed `Service` map; U12), and
**`options`** (the escape hatch; U13). Constructed through the `system.<name>`
namespace (U3) with the type name inferred (U18). Rejected: a
`target`+`packages`-only v1 — a machine that can't enable a service or set an
option is not yet a machine.

**U12 — `Service` is an open record** *(ratified 2026-06-16; D-SVC A)*: a
`Service` is a record whose first field is **`enable: Bool`** and which carries
further typed per-service fields (`openssh: { enable: true, ports: [22] }`).
Modelling it as a record from the start means the first service that needs a knob
doesn't force a type change. Written as a bare `{ … }` under `services:` (type
inferred, U18). Rejected: an `enable`-only v1; folding services into raw
`options` strings (loses typing and discoverability).

**U13 — `options:` is an ordered list of `key: value` pairs** *(ratified
2026-06-16; D-OPTS, list-typed variant of B)*: the machine-option escape hatch is
a **list** of direct dotted-key/value entries — `options: [ net.hostName: laptop,
time.timeZone: "Europe/London" ]` — with **no `set(…)` wrapper**. A list (not a
map) keeps entries ordered and avoids map-merge ambiguity; a dotted key is a path
into the upstream option set. **Quoting:** values that are jet identifiers or
typed values are written **bare** (`net.hostName: laptop`, `users.nate.shell:
default.fish`); only free-form strings (timezones, locales, paths — anything with
`/`, spaces, or non-identifier characters) keep quotes. **`target`** is likewise a
**typed platform value** (`linux.x64`), not a string, so it type-checks and
LSP-completes. Typed value tables for timezones/locales may be added later; until
then those stay quoted strings. Rejected: the `set("k", v)` list-of-calls form
(A); a map literal (B as written — dotted string keys + ambiguous merge); a
typed-fields-only surface with no hatch (every gap becomes a ratification
request).

**U14 — `Image` derives from its source `System`** *(ratified 2026-06-16;
D-IMG-FIELDS B + anti-repetition)*: an `Image` (ISO / VM / disk) has **`from:
system.<name>`** and an optional **`format:`** — **`iso`** / **`qcow`** /
**`raw`** (default `iso`). `target`, `packages`, `services`, and `options` are
**inherited from the referenced `System`**: they are written once on the system
and never restated on the image. An explicit `target:` on the image is only for
cross-compiling to a different arch. Constructed through the `image.<name>`
namespace (type inferred, U18). Rejected: a `from`-only image with no `format` (B
chosen — installer/VM/raw is the immediate real need); restating
`target`/`packages` on the image (the duplication the owner rejected).

**U15 — the jetos tier lives under `jetpack os`** *(ratified 2026-06-16;
D-JETOS-BIN, `jetpack` subcommand)*: whole-machine management is a **`jetpack os
<verb>`** subcommand group (`jetpack os switch`, `jetpack os build`) — **not** a
separate `jetos` binary and **not** folded onto the sacred `jet` tool. The `os`
namespace preserves the conceptual separation NixOS gets from a distinct
`nixos-rebuild` binary, without shipping a third binary. Verbs mirror
`nixos-rebuild`: **`switch`** (build + activate + set boot default) and
**`build`** (build only); `boot` / `test` may be added under this protocol.
Rejected: a new `jetos` binary (A — extra binary to ship); `jet os …` (B —
muddies the language tool's boundary); a bare `jetpack switch` verb (overloads
package management).

**U16 — `config.jet` discovery + `@host` selector** *(ratified 2026-06-16;
D-CFG-LOAD)*: `jetpack os <verb>` takes a positional target **`[<config-path>]@<host>`**.
The **`@host`** segment selects which `System` in the config to apply (`jetpack os
switch @halcyon`); `<host>` is required. The **config path is optional** and
defaults to **`~/.jet/config.jet`**; an explicit path is the prefix before `@`
(`jetpack os switch ./jet-test@halcyon`). This reuses jet's existing `@`
source-selector convention — jet's `@host` is NixOS's `#hostname` flake-ref
selector. Rejected: an always-explicit path (B — verbose for the common case); a
`cwd → ~/.jet` search (C — dangerous ambiguity for a whole-system switch); a
separate `--config` flag (the path-prefix subsumes it).

**U17 — a `library` package is consumed with `use`** *(ratified 2026-06-16;
D-LIB-USE A)*: once a `library` package (U10) is realized (its source staged), it
is brought into code with the ordinary **`use <pkg>`** module form
(S16 / D-S16-USE) — `use jsonutil;` then `jsonutil.parse(…)`. One import concept
covers files, modules, and library packages; an `executable` package still goes on
PATH, not `use`. Rejected: a separate `libraries: […]` list distinct from
`packages:` (B — a second concept where `use` already fits); a bespoke
library-import keyword.

**U18 — inferred constructors via expected type** *(ratified 2026-06-16;
D-INFER-CTOR)*: when a value's **expected type is known** — a typed namespace
(`system.<name>:`, `image.<name>:`, `env.<name>:`) or a typed field (`services:`
holds `Service`s) — the constructor type name is **optional**: a bare `{ … }`
elaborates to it (`system.halcyon: { … }` ≡ `system.halcyon: System { … }`;
`pipewire: { enable: true }` ≡ `pipewire: Service { … }`). The explicit
`Type { … }` form (S29) stays legal as an escape hatch and wherever no expected
type is inferable (a bare binding, an ambiguous union); there, an un-annotated
`{ … }` is a diagnostic ("name it, e.g. `System { … }`"). Field typos still report
against the inferred type ("unknown field … in `Service`"). This is expected-type
elaboration — the Blueprint "typed pin" model — and applies wherever an expected
type reaches a record literal. Rejected: requiring the type name at every
constructor (the duplication the owner rejected); inferring a type for a value
binding that has no contextual type (kept explicit).

### Sidequest language features (ratified 2026-06-18)

**D-ILE1 — Implicit lib/exec inference** *(ratified 2026-06-18, option A; amends
U10 / D-JPK-FILES)*: a package's **kind is inferred from `fn main()`**, not
required. Two levels:
- **No `pkg.jet`** — a single file or directory with a top-level `fn main()`
  compiles as an **executable**; without one, as a **library**. Two `fn main()`
  in one inferred package is **E_DUPMAIN** (add a `pkg.jet` `packages:` block to
  split them). Explicit `pkg.jet` always wins; `jet run file.jet` stays
  zero-ceremony (U7).
- **With `pkg.jet`** — in the `packages: { … }` block (U10) the **`kind` is
  optional**: a module with `fn main()` is `executable`, otherwise `library`.
  The user may still write the kind explicitly (`deploy: executable`) to
  override or document intent. This is the package-definition surface the owner
  asked for — `pkg.jet` *describes* the package(s); kind is inferred unless
  stated.

Rejected: requiring an explicit kind always (option B, U10 status quo) — walls
off the one-file "just run it" path.

**D-BIND1 — Binding sigils** *(ratified 2026-06-18, option A; amends S2)*: see
**S2** — full Odin sigils `name :: expr` (immutable) / `name := expr` (mutable);
`val` / `var` retired to teaching errors. The owner accepted **spending `::`**:
it now means "immutable binding" everywhere, so S83's external-definition form
chose a different separator (`~~`, ratified 2026-06-19); any later `Type::Trait`
sugar (S28) must likewise avoid `::`. `::` inside
`extern rust "rust::path"` strings (S50) is unaffected. Rejected: `:=`-only with
`val` kept (option B), status quo (option C).

**D-LABEL1 — Named loops & labeled break/continue** *(ratified 2026-06-18,
option B; amends S19 / S23)*: a loop may carry an **`@name` label** —
`@outer loop row in grid { … }` — and `**break @name**` / `**continue @name**`
target it, escaping or continuing the named (possibly outer) loop. Reuses the
S82 `@` marker sigil in a **new position** (inline, immediately before `loop`),
so it can never be confused with a labeled argument (S61). Diagnostics
`E_UNDEFINED_LABEL` (unknown label; lists labels in scope) and `E_LABEL_NOT_LOOP`
(`@name` not before a `loop`). Codegen maps `@name` → Rust `'name:` labels.
Rejected: bare `name: loop` (option A — visually collides with S61 labeled
args); Rust-style `'outer` (S41 already makes `'x'` a char literal — lexer clash).

**S6-R — No visible semicolons** *(ratified 2026-06-18, option B; supersedes
S6's required-`;` rule)*: see **S6** — no `;` in source; the lexer inserts
terminators Go-style after a line whose last token can end a statement. Layout
rules: `-> Type` and `{` stay on the parameter list's closing-`)` line; and a
terminator is **suppressed** when the next non-blank line begins with `.` (S69
chain) or a binary/logical operator (line-broken expressions) — see S6 for the
full list. `E_MISSING_SEMI` is retired (the lexer handles insertion); the grammar
stays terminator-based internally. Rejected: keeping required semicolons (option
A); significant indentation; optional-`;`.

**D-IF1 — `if` as universal branching** *(ratified 2026-06-18, option A; amends
S24 / S68)*: see **S68** — `if` is the one branching keyword; `when` is retired
to a teaching error pointing at `if`. `if subject { arm -> body … }` is
multi-arm dispatch (the former `when`), and a bare value arm is implicitly
compared against the subject (**inferred comparator**, owner directive):
`if code { 200 -> …; 404 -> …; }` ≡ `code == 200` / `code == 404`. This reverses
S24's bare-value-match rejection. **Surface (D-IF2, ratified 2026-06-18):**
catch-all arm is `else -> body` (`...` rejected, stays free for future spread);
arm bodies may be **braceless** single expressions (`{ … }` for multi-statement
bodies); bare-value-vs-condition is a **structural mix** — an arm head with no
top-level comparison/logical operator is a bare value (compiler prepends
`subject ==`), an arm with one is a full `Bool` condition, mixed freely. Rejected:
keeping `if`/`when` separate (D-IF1 option B); `...` catch-all and mandatory arm
braces (D-IF2 alternatives).

### Next-Tasks wave (ratified 2026-06-19)

Six of the eight Next-Tasks ballots. The constructor-shapes (D-CTOR1) and
allocator-spelling (D-ALLOC1) cards stay open pending owner explanations.
**Ratified but not yet implemented** — the parser / sema / codegen / fmt changes
land on the owner's word; `src/` is untouched until then.

**D-ATTR1 — Attribute sigil `@` → `#`** *(ratified 2026-06-19, option B; reverses
the S55 / S82 marker spelling)*: attributes and markers are written `#Unsafe`,
`#Serialize`, `#Audit("…")`. Positional disambiguation keeps the existing `#`
uses — fixed-size `[T#N]` and version-pin `name#ver` — working, since those never
sit in the marker (item/statement-prefix) position. The teaching error flips to
reject `#Unsafe` and point at `#Unsafe`. Reverses the *spelling* the owner picked
in S55 (derive policy) and S82 (marker sigil), not their semantics.

**D-ATTR2 — Multi-marker list form** *(ratified 2026-06-19, option A; live with
D-ATTR1 = B)*: multiple markers list plainly inside brackets —
`#[Serialize, Comparable]`. The Rust-literal `#[derive(…)]` wrapper (option B)
stays rejected, as S55 already declined it; only the sigil changed, not the list
shape.

**D-ATTR3 — Loop labels stay `@`** *(ratified 2026-06-19, option B; live with
D-ATTR1 = B)*: attributes move to `#` but labels (D-LABEL1) keep `@` —
`@outer loop { break @outer }`. Source therefore carries two marker sigils: `#`
for attributes/markers, `@` for loop labels (and the U6/U16 ref/host `@`, which
lives in CLI/manifest strings, not source). The plan flagged the mixed-sigil
outcome as a trap; the owner chose it with that flag visible. fmt prints each in
its own position.

**D-NARG1 — Named args + defaults on methods** *(ratified 2026-06-19, option A;
extends S61)*: method and constructor calls behave like free-function calls —
call-site labels (`rect.draw(filled: true)`) are checked against parameter names,
and trailing defaults fill when omitted (`rect.draw()` → `filled = false`). Closes
the S61 gap where a method label was parsed then silently dropped and method
defaults never filled.

**D-NARG2 — fmt preserves call-site labels** *(ratified 2026-06-19, option A;
refines S61)*: label presence is the author's documentation choice; fmt never
adds a missing label nor strips a present one. Canonicalization is revisited only
with the LSP quick-fix (S14 / M6), not in v1.

**S29-FLUSH — Flush constructor block** *(ratified 2026-06-19, option A; amends
S29)*: the canonical construction style is flush — `Point{x: 3.0, y: 4.0}`, the
type name hugging its field block the way a call's `(` hugs its callee; colon
spacing (`x: 1`) keeps the language-wide `: ` rule. The flush rule extends to
destructuring patterns (`Point{x, y} :: make()`) for build-vs-match symmetry. The
parser already accepts both forms, so this is a formatter-canonical-style change.
S29's canonical example above still shows the spaced form and is corrected when
the fmt change lands.

**D-CTOR1 — Named constructors only** *(ratified 2026-06-19, option A; formalizes
existing behavior)*: many ways to build a type = many distinctly-named no-`self`
statics that return it (`Point.cartesian(…)`, `Point.polar(…)`); a duplicate name
stays a hard **E0105**, whose teaching text points at naming each constructor.
Overloading (options B/C) is rejected: it only disambiguates when signatures
differ, so same-typed shapes still need names — it adds name-mangling +
resolution machinery without removing the need for names. Zero codegen change;
this is the `Point.unit()` precedent made policy. Related: **D-CTOR2** (no marker
keyword — return-type-is-the-type identifies a constructor) follows from this.

**D-ALLOC1 — Allocator method style** *(ratified 2026-06-19, option A)*: construct
an allocator with a named constructor and allocate with a method —
`arena :: mem.Arena.new()` then `node :: arena.alloc(value)` (freed at scope end).
Capacity rides as an optional S61 default (`mem.Arena.new(capacity: 4096)`), so
A subsumes option C; the free-builtin `make(Node, in: arena)` form (B) is rejected.
An arena value is **not** `#Unsafe` — `use core.mem` is the opt-in gate (D-ALLOC-B).
Ships with the `core.mem` arena work (D-REF2).

### Targets model & capability vocabulary (ratified 2026-06-21)

The c07 targets reshape (all five D-TGT) and the decided half of the c06 capability
model (D-CAP1/4/5/6). D-CAP2 (copy/share form) and D-CAP3 (annotation order) stay
**open** — until they ratify, the capability words below are reserved spellings only;
their parameter-position syntax is not yet finalized.

**D-TGT1 — `targets:` replaces `kind:`** *(ratified 2026-06-21, option B; owner:
"fully remove kind, we are still greenfield"; supersedes U10 / D-ILE1 on the kind
field)*: a package declares a **`targets:` list**, not a `kind:`. `kind:` is **removed
entirely** — no deprecation alias; a `kind:` field in `packages:` is now an unknown
field (teaching error → "write `targets: [ … ]`"). When `targets:` is omitted the
D-ILE1 inference carries forward onto the new vocabulary: a module with `fn main()`
infers `[executable]`, otherwise `[library]`. Rejected: augmenting `kind:` with a
parallel `targets:` (option A — two ways to say one thing).

**D-TGT2 — first-increment targets** *(ratified 2026-06-21, option A)*: the shipped
targets are **`library`**, **`executable`**, **`test`**, **`example`** — the four with
working build paths. **`benchmark`** and **`plugin`** are **reserved** target keywords
(owner: denote them for future addressing): writing one is a teaching error ("target
`benchmark` has no backend yet"), not an unknown-keyword error. Rejected: shipping all
six now (option B — keywords with stub backends).

**D-TGT3 — bare keyword or block** *(ratified 2026-06-21, option A)*: a target with no
fields is a **bare keyword** (`library`); a target with fields is a **block**
(`executable { entry: "src/cli.jet" }`). Mirrors the ratified U10 `name: kind` vs
`name: { … }` shorthand. Rejected: mandatory empty `{}` (option B — pure noise).

**D-TGT4 — default executable entry** *(ratified 2026-06-21, option B; owner call, no
rec)*: a bare `executable` is allowed; the compiler searches fixed conventions —
**`src/main.jet`**, then **`<package>.jet`** — for the entry module. Zero matches or
two-or-more matches is an error asking for an explicit `entry:`. Rejected: requiring
`entry:` always (option A — no zero-config path); single-root-file rule (option C).

**D-TGT5 — `#test` fns + optional `test` target** *(ratified 2026-06-21, option C,
hybrid)*: `jet test` **auto-collects** every `#test` fn (S82 marker, `#` per D-ATTR1)
in the package; a **`test { entry: … }`** target is optional, for an out-of-tree
integration file. Both run. Rejected: an explicit `test` target carrying everything,
`#test` not auto-run (option A); implicit-only with no out-of-tree slot (option B).

**D-CAP1 — capability keyword spellings** *(ratified 2026-06-21, option A)*: the
four-capability vocabulary is **`view` / `edit` / `take` / `share`**. `view` and `take`
are already ratified ownership keywords (S10); **`edit`** and **`share`** are new
reserved capability words. Parameter-position placement is still **open (D-CAP3)** and
the copy/share call form is **open (D-CAP2)** — only the spellings are fixed here.
Rejected: reusing `mut` for the edit slot (option B — reads as Rust `&mut`); `read` /
`write` / `own` (option C — S10 already rejected these); `look` / `change` / `keep`
(option D — orphans the live `take` / `view` keywords).

**D-CAP4 — `api:` is a per-target field** *(ratified 2026-06-21, option D; rides
D-TGT3 blocks)*: a library target records its public capability signatures by setting
**`api:`** inside its target block — `library { api: stable }` (record + flag API
breaks) or `library { api: explicit }`. Default is inference (D-CAP6). Rejected: a
top-level `api:` field (option A), a `payload api = …` statement (option B), an
attribute (option C).

**D-CAP5 — which targets emit capability metadata** *(ratified 2026-06-21, option A)*:
**any target that produces a consumable library artifact** emits capability metadata;
**executable/binary targets infer and emit nothing**. Holds under D-TGT1=B. Rejected:
only a literally-named `library` target emits (option B); decouple from targets and let
`api:` alone decide (option C).

**D-CAP6 — library capability default** *(ratified 2026-06-21, option A)*: inference is
the library default **forever**; `api: explicit` is opt-in and never auto-flips.
Inference already guarantees capability *safety*, so explicitness buys documentation,
not correctness — keeping it opt-in honors the simplicity ratchet (I8). Rejected:
flipping to mandatory-explicit at 1.0 (option B — silent future break); explicit from
day one (option C — taxes the beginner for what inference provides).

### Safety tiers — scoped capabilities, units, single-use (ratified 2026-06-21)

Three value/effect-safety features, each **ratified as the target** but **gated** on an
upstream decision still in the ballot — implementation is sequenced after the gate, no
`src/` change until then.

**D-SCAP1 — Scoped capabilities** *(ratified 2026-06-21, option A; gated on D-EFF1)*: a
**capability is a first-class value** granted into a lexical scope —
`#grant(fs) { caps -> … }` — and **revoked at scope end** by the RAII rule (S63). The
capability authorizes its effect (`#fs`/`#net`) inside the scope; letting it escape
(stored, returned, shared) is a compile error (**E0711**), and using an effect with no
capability in scope is **E0712**. This is **authority to perform an effect**, distinct
from the c06 value-ownership capabilities (`view`/`edit`/`take`/`share`); it generalizes
the S58 `#Audit`/`#Unsafe` gate from "unsafe ops" to "any guarded power." **Gated on
D-EFF1** (the effect system, c66) — the capability is what authorizes an effect region,
so D-EFF1 must land first. Rejected: effect-tag-only capabilities with no value (option
B — can't lend a power per-call).

**D-UNIT1 — Units of measure as a tag** *(ratified 2026-06-21, option B; gated on
D-QUAL2)*: units are a **parameterised tag `#unit(usd)`** on a numeric type, declared in
families (`#unit_family(currency) { usd, eur, gbp }`), with method-literal syntax
**`9.99.usd`**. The compiler derives the wrapper (erases to the raw numeric, F#-style)
and enforces unit-matching arithmetic — unit-vs-unit mismatch is **E0128**, unit-vs-bare
is **E0129**; `.raw()` strips the unit. This is the **upgrade** to D-DIST2 (the
hand-written `distinct` newtype stays valid and is the fallback); it does not undo
D-DIST1/D-DIST3. **Gated on D-QUAL2** (parameterised tags). Rejected: library-newtype
only (option A — boilerplate per unit, no natural literal).

**D-LIN1 — Single-use (must-consume) values** *(ratified 2026-06-21, option A; gated on
D-QUAL2; owner renamed `linear` → `SingleUse`)*: a type marked **`#SingleUse`** must be
consumed **exactly once on every reachable path** — passed to a `take` parameter,
returned, or explicitly `drop(x)`'d (drop requires an `#Audit`). `#SingleUse` implies
`#no_copy`. The checker tracks consumption through branches and names the unconsumed
binding — **E0140** (unconsumed at scope end), **E0141** (unconsumed on one branch). This
adds the "at least once" half to `take`'s "at most once" (S10). **Naming (owner call):**
the type-theory term *linear* is spelled **`SingleUse`** — plain words over jargon, the
same precedent as the `view`/`edit`/`take`/`share` capability vocabulary. **Gated on
D-QUAL2**; `#must_use` (option B) is the strict-subset stepping stone and may ship first.
Rejected: `#must_use`-only (option B — misses the bound-then-silently-dropped case).

### Uninitialized memory & `jet.regex` (ratified 2026-06-21)

**D-UNINIT1 — Visible uninitialization** *(ratified 2026-06-21, option C; owner chose
the attribute form over the rec)*: skipping the default zero-fill of a binding is opted
into with the **`#Uninit` attribute** on the binding — `#Uninit buffer: [4096]U8` —
reusing the `#` marker sigil (D-ATTR1) like `#Unsafe`/`#Audit`. Gated behind
**`use core.mem`** (S58 low-level tier); outside that gate it is a teaching error
pointing at the gate. Safety is a **compile-time** write-before-read proof: sema tracks
each `#Uninit` binding's initialized state by dataflow across all paths, and a read on
any path that may precede a full write is **E0420** (snapshot required when implemented,
I4). Codegen lowers to `MaybeUninit::<T>::uninit()` after the proof passes — never a
runtime trap (the rail Zig's `= undefined` and C's silence lack). **Status:** the sema
write-before-read proof (E0420, with the gate E0424 and POD-only E0423) is implemented
and green; **codegen is gated on a discovered prerequisite** — `[N]T` fixed-lists
currently lower to `Vec<T>`, on which `MaybeUninit` is unsafe and the safe lowering
zero-fills (defeating the feature), so fixed arrays must first become real stack arrays
`[T; N]` (proposed owner decision **D-FIXARR1**, board card c82). The parser stays
unwired until then. Rejected: `:= ---` Jai sigil (option A — opaque, greps
badly); `:= uninit` value-keyword
(option B, the rec — owner preferred the `#`-marker idiom).

**D-REGEX1 — `jet.regex` ships on the `regex` crate** *(ratified 2026-06-21, option B;
owner-approved I6 bootstrap dep)*: `jet.regex` ships now backed by Rust's **`regex`**
crate (DFA/NFA hybrid, **linear-time, no ReDoS**), surface `use jet.regex as re` /
`re.match(pattern, text)?`. This is an explicit, **owner-approved I6 exception** — the
one external Core-library dep sanctioned for the regex bootstrap — carrying a standing
obligation to **native-ize (replace with an in-house RE2-style engine) before the end of
Epoch 3**, so the end state stays dependency-free (I6). The compiler (`Source/`) takes no
crate; the dep lives only in the `jet.regex` Core sub-library. Rejected: native engine
first (option A — weeks of work, blocks the #1 persona gap meanwhile); defer (option C —
leaves the largest adoption gap open).

### Qualifier taxonomy (ratified 2026-06-21)

**D-QUAL2 — Two kinds of qualifier** *(ratified 2026-06-21, option B)*: there are exactly
**two** kinds — **`trait`** (has at least one method; dispatches via vtable) and **`tag`**
(no methods; erases at runtime). The beginner rule is one sentence: *methods → trait, no
methods → tag.* This collapses the former four overlapping concepts — attributes, effects,
typestate markers, units, taint, must-use, tool-markers — all into **tag**. A `#Name` with
no method body is a tag; derives are traits (they attach method impls); effects like
`#(db)` are tags whose propagation sema tracks. Sema gains a first-class **`tag`** keyword
(declaring methods on a `tag` is **E0732**; using a tag where dispatch is expected is
**E0731**, with a fix-it to declare a `trait`); codegen is unchanged (tags already erase).
This is the **taxonomy foundation** that gates **D-QUAL1** (surface routing, still open) and
**unblocks the value-tags cluster** — D-UNIT1 (`#unit`) and D-LIN1 (`#SingleUse`) now have
their tag machinery; their exact inline spelling still rides D-QUAL1's surface decision.
No upstream gate on the tag foundation — slated for full end-to-end implementation (the
`tag` keyword + dispatch-vs-marker enforcement + E0731/E0732 snapshots); D-QUAL1 only
adds the effect-routing surface on top. Rejected: four kinds (option A —
the status quo and the source of "what's a tag vs an attribute?" confusion); one "label"
kind (option C — erases the dispatch-vs-marker distinction that actually matters).

**D-TAINT1 — Taint tracking** *(ratified 2026-06-21, option A; gated on D-EFF1; option B
deferred post-Epoch-3)*: an untrusted value carries a **`#tainted`** tag, attached inline
at its source (S82/D-ATTR1). The tag **spreads** — anything derived from a tainted value
(assignment, interpolation, field store, return) is tainted. A function declared
**`sanitizer fn`** is the one blessed way to strip it: its return is `#untainted` by
contract. A tainted value reaching a **sink effect** (`#db`/`#exec`/`#net`) is **E0721**,
naming the sink with a "pass it through a sanitizer" fix-it. This rides D-EFF1's effect
propagation (a sink is just an effect) and the tag is static, erased in codegen (I3).
**Gated on D-EFF1.** **Option B deferred (owner, 2026-06-21 — captured, not lost):** full
**information-flow control** (security-label lattice, principals, explicit `declassify`)
is real and handles confidentiality + multi-level integrity + implicit-flow leaks, but is
research-grade ceremony for the v1 injection-class win (I8). It is **deferred to
post-Epoch-3** as the dedicated IFC ballot — tracked as **D-IFC1** in the deferred-ballots
list (board items #30/#33). Rejected-for-now: shipping IFC as the v1 taint model (B —
wrong altitude for the 80% case one `#tainted` bit already covers).

**D-ALLOC2 — Arena `alloc` return + reset/free safety** *(ratified 2026-06-21, option A;
gated on a new region rule)*: `arena.alloc(value)` returns a **scope-bound `view`** into the
arena's storage — readable/writable inside the `arena ::` binding scope, but the checker
**forbids it escaping** (store/return/share → **E0631**) and **forbids any use after
`reset`/`free`** (**E0632**; `reset`/`free` take the arena by `mut`, legal only when no
escaping view is live). This is bumpalo/typed-arena's `&'bump T` reworded in Jet's
capability vocabulary — real shared bump-allocation, use-after-reset a *compile error*, no
runtime trap (P1 safe-by-default). **Gate:** A needs a **region** — the lifetime of the
`arena ::` scope — which `view` (S10/c06) does not yet have; **D-ALLOC2-A cannot be built
until that region rule is ratified.** A follow-on decision **D-REGION1** (where regions are
denoted/inferred, part of c06) is queued in the ballot; option B (opaque generational
`Handle<T>`) is the recorded fallback if regions slip. Replaces the c05 stub where
`alloc(v)` just returned `v`. Rejected: opaque handle as the primary (option B — per-access
indirection + a runtime check on statically-unknown handles); owned-clone stub (option C —
"barely an arena", no shared buffer).

**D-REGION1 — Allocation regions** *(ratified 2026-06-21, options A **and** B together;
owner: "A & B together — ratify it"; unblocks D-ALLOC2)*: regions are **implicit and
scope-inferred by default (A — the beginner/automagic tier)**: the region *is* the lexical
scope of the `arena ::` binding, no lifetime is ever typed, and the checker proves no
allocation escapes it (E0631) or outlives a `reset`/`free` (E0632, per D-ALLOC2). **Plus an
explicit `region r { … }` block (B — the expert tier)** for the cases inference cannot give:
a region spanning **two** allocators, **narrower** than the enclosing function, or **named**
so allocations flow back out under a stated bound. Both ship — the beginner never writes
`region`; the expert reaches for it when scope-inference is too coarse/fine. The escape rule
is enforced against the inferred scope or the named region identically. This is the **region
mechanism D-ALLOC2-A required**, so the scope-bound arena `view` is now buildable. Rejected:
Rust-style named lifetime `'a` (option C — forces the lifetime surface on everyone, neither
the beginner default nor a clean expert tier).

**D-OBS2 — Debug line-table format** *(ratified 2026-06-21, option B)*: the Jet→Rust line
table is a **sidecar `<file>.jetmap` JSON file** written beside the generated Rust, schema
`{ "version": 1, "source": "<path>", "lines": [[rust_line, jet_line], …] }`. A versioned,
std-only, third-party-readable contract: any DAP adapter reads one file to translate editor
breakpoints to Jet lines. Codegen records the `(jet_line, rust_line)` pairs it already
holds (I3 — codegen stays dumb); a hand-written serializer (`Source/Debug/linemap.rs`)
writes the JSON (zero crates, I6). rustc retains full DWARF for lldb. Part of the DAP
debugger (rides D-OBS1's source-map foundation → GA). Rejected: inline `// jet:line`
comments (option A — a reformatter or `rustfmt` strips comments, so it is not a stable
contract for third-party tools); custom binary section (option C — invisible to source
tools and platform-coupled per ELF/Mach-O/PE, the wrong fit for an editor-facing map).

**D-CASING1 — Tag & trait casing; "Core" not "std"** *(ratified 2026-06-21, owner-directed)*:
two naming conventions, applied everywhere (code, examples, snapshots, docs, ballot cards):
1. **All tags are PascalCase.** A *tag* is the D-QUAL2 marker category — any `#`-marker with
   no methods. So every `#`-marker is PascalCase: value-facts (`#Tainted`, `#Paid`,
   `#Unpaid`), gates (`#Unsafe`, `#Audit`), harness (`#Test`, `#Todo`), feature markers
   (`#Uninit`, `#Unit`, `#SingleUse`, `#NoCopy`, `#MustUse`, `#Repr`, `#Transact`, `#Grant`,
   `#Detach`, `#Route`, …), and effect-tag members (`#(Net, Db, Log)`, `#(NoNet)`). The tag
   *name* is PascalCase; value-style arguments keep their own case (a user unit `#Unit(usd)`),
   type/ABI-style arguments are PascalCase (`#Repr(C)`). Built-in derive markers
   (`#[Serialize, Comparable]`) were already PascalCase (D-ATTR2) — unchanged.
2. **Traits are PascalCase** (they are types; enforce it).
3. **The standard library is "Core", never "std".** The user-facing namespace is already
   `core.*` (`core.fs`/`core.mem`/…) — kept. The *terminology* and *identifiers* "std"/"stdlib"
   are renamed to "Core" everywhere: docs ("the Core library"), filenames (`Std.rs` →
   `Core.rs`), consts (`KNOWN_STD_MODULES` → `KNOWN_CORE_MODULES`, `std_imports` →
   `core_imports`, …), error copy, and ui-snapshot names. The `jet.*` ring packages keep
   their namespace but are collectively part of **Core**, not "std".

This amends the marker casing in S82/D-ATTR1 (markers were mixed-case) and the value-fact/
effect spellings throughout c62/c66–c73. The high-impact gate renames (`#unsafe` →
`#Unsafe`, `#audit` → `#Audit`) are **confirmed** (owner, 2026-06-21: PascalCase reinforces
that these are weighty, unique declarations). Rejected: keeping lowercase tags / the name "std".

**D-CASING1 follow-on (owner-directed 2026-06-21): `test` / `todo` / `pure` become PascalCase
`#`-markers.** These three "unique declarations" join the tag family rather than staying bare
keywords, so they draw the same attention as every other tag:
- **`#Test`** replaces the `test "name" { … }` block keyword (S43/S82): `#Test "name" { … }`.
  The `jet test` harness recognizes `#Test`.
- **`#Todo`** replaces the bare `todo` typed-hole expression (D-TOOL2).
- **`#Pure`** replaces the `pure fn` modifier (S60): `#Pure fn name() { … }`.
The lowercase spellings (`test`/`todo`/`pure`) are retired to teaching errors pointing at the
`#`-marker forms (S14 pattern). Amends S43, S60, D-TOOL2, S82 on spelling only — semantics
unchanged.

## Enforcement

Ratified decisions are **frozen**. `cargo test` runs `tests/decisions.rs`,
which fails if:

- any `Source/Syntax.rs` entry is `(provisional)` while ratified in this file;
- any open or deferred decision ID appears in `Source/Syntax.rs`;
- the Provisional table below lists a real decision ID;
- a staged decision loses its pinned error code in docs/spec/diagnostics.md.

Agents: after ratifying a row, update `syntax.rs` to `(ratified)`, clear
the Provisional table row, and add a ui snapshot if behavior changes.

## Staged implementation (ratified syntax, milestone pending)

Syntax and semantics below are **decided** — do not re-litigate. Only the
implementation milestone is pending.


| ID  | Milestone | Enforcement today                                                | Code  |
| --- | --------- | ---------------------------------------------------------------- | ----- |
| S15 | M6        | default unwind in `Source/main.rs`; `--small` + `panic=abort` in M6 | —     |


## Provisional — currently in the code


| ID  | Choice in code                         | Where |
| --- | -------------------------------------- | ----- |
| —   | *(none — Group 1 ratified 2026-06-11)* |       |


## Open decisions — owner input needed

> **Ballots:** every open decision below (and all new ones for M3–M14)
> has a full ballot — options, how Rust does it, expert lean, beginner
> lean, recommendation — in **tools/Tower/docs/ballots/decision-ballots.md**, grouped so
> the owner decides one milestone-sized batch at a time. The rows here
> are the registry; the ballots are the briefing.

### Registered for M3–M14 (see tools/Tower/docs/ballots/decision-ballots.md for options)


| ID   | Question                                   | Needed by |
| ---- | ------------------------------------------ | --------- |
| S56  | typed reflection / user derives | **Epoch 3** — [`tools/Tower/docs/plans/epoch-3/user-derives-reflection.md`](../../tools/Tower/docs/plans/epoch-3/user-derives-reflection.md) |

> Jetpack native-resolver decisions **D-JPK16** (tvix-shim posture) and
> **D-JPK17** (named sources) were ratified 2026-06-15 — see the Ratified
> section above and `tools/Tower/docs/plans/jetpack-jetos/README.md` §3.3 (provider roadmap).


Group 6 (S26–S28, S45–S48, S46–S47, S55, S57) and Group 7 (S51–S54, S52)
are fully ratified above. **S59 (C FFI)** ships in **Epoch 2** (E2-M14). **S53**
(concurrency) is ratified as deferred past v1.0. S60 is ratified post-1.0. S56
(user derives via typed reflection) is deferred past v1.0 by S26's ratified layering.

## Ratified 2026-06-17 — dependency strategy + M10 / M12 / M18 calls

**D-DEP1 — third-party dependencies ship as FFI-wrapping Jet packages.** The
compiler binary stays **zero external crates** (I6 holds). Any capability that
needs a Rust crate (TLS, regex, zip, sqlite, an OTel exporter, …) is delivered
as a normal **Jet package** whose source wraps the crate via the ratified Rust
FFI (`extern rust "crate@version" { … }`, S50) and exposes a clean Jet API.
Consumers depend on the *package*, never the crate. A later native port swaps
the package body and keeps the public API — callers don't change. The version
pin lives inline in the `extern rust "crate@version"` block (S50, authoritative);
the Jet package itself is pinned with `pkg#version` (VERSION-#). The package
manifest is **`pkg.jet`** (`payload:` / `deps:` / `packages:`), never the
retired `jet.toml`/`payload.jet`/`pack.jet`. **Exception:** the compiler's own internals
(e.g. the `jet repl` line reader) cannot consume a Jet package — bootstrapping —
so those stay std-only or take a directly-vetted crate only by separate owner
sign-off.

**D-NET1 (M10) — TLS via `rustls`, delivered as the `jet.tls` package** (an
instance of D-DEP1). `jet.http` depends on `jet.tls`; `jet.tls` wraps `rustls`
via `extern rust`. No crate enters the compiler. HTTPS works with zero config in
user code (`use jet.http`).

**D-OBS1 (M12) — observability foundation in M12, full debugger at GA.** M12
ships source maps + Jet-line panic/error reports (no generated Rust shown to
users). Full DAP step-through debugging is a **GA (E2-M17) gate**, not an M12
blocker.

**D-OBS3 (M12) — OpenTelemetry-aligned names, std-only now.** Structured JSON
logs/metrics with OTel-aligned keys, std-only, in M12. An actual OTel *exporter*,
when wanted, ships later as an FFI-wrapped Jet package (D-DEP1) — never a
compiler dependency, never a framework baked into std.

**D-REPL18 (revised 2026-06-17) — `jet repl` ships a std-only line reader; no
`rustyline`.** Supersedes the 2026-06-16 pick of `rustyline`: it would have been
the first-ever crate in the zero-dep compiler, and the REPL is compiler-internal
so D-DEP1's package-wrapping can't apply. The REPL ships its interpreter session
on a `std::io` reader now; richer line editing (history, arrow keys) is a later
upgrade that must re-earn an owner crate sign-off.

## Decision log


| Date       | ID  | Decision                                    | By    |
| ---------- | --- | ------------------------------------------- | ----- |
| 2026-06-22 | D-EFF1 | effect system (B): inferred per-fn effect set propagated along calls (Koka-style rows), erased in codegen (I3 — no handler/monad/runtime value); `pure fn` becomes the empty set; assert/restrict at boundaries (`#(net, db)` on the signature) and in `#caps(net) { … }` regions (out-of-set + impure-`pure` diagnostics; the card's illustrative E0701/E0702 collide with existing FFI codes — real codes assigned from the free range at impl). **Reopens S60's "no further effects" stance** (S60's `pure` spelling+meaning preserved as ⊥). Surface spelling pinned to `#(…)` by D-QUAL1=1 (sub-Qs 4+5 resolved). **Implementation gated on new D-EFF2 (effect polymorphism / higher-order propagation) + D-EFF3 (trait-method effects)**; diagnostic quality is an impl concern. Carries D-SCAP1/D-TAINT1/D-DET1/D-TXN1/D-TXN2 | owner |
| 2026-06-22 | D-QUAL1 | qualifier-surface dialect = **Option 1 (Sigil-pure)**: effects ride the signature as `#(net, db)`; the tag/trait list stays the bare `#[Serialize, Comparable]` form (**D-ATTR2 kept, untouched**); roles `role X = #(…)` / `#[…]`; manifest policy `plugins.coupon: deny(fs, db)` in the in-source `module { }` block; declaration-heavy grouping uses the same `#[ effects:…, facts:… ]` labeled bracket (purely additive); value-facts ride the value (`#tainted`, `#paid`). Delivers Core D + Roles + Unified block. Same `#(…)` surface as D-EFF1 — one spelling, no duplicate. **Reopens S60's effect surface**; follow-on must place capability policy across pkg.jet (D-JPK-FILES) vs the in-source block | owner |
| 2026-06-22 | D-TXN1 | `#transact { }` rollback (A): semantic contract — every `?`-failure inside the block calls `rollback(mut self)` (the `Rollback` trait) in reverse order on the values mutated so far; clean exit commits, zero runtime cost beyond the rollback calls; a non-`Rollback` mutation inside the block is a compile error naming the type + fix-it (the card's illustrative E0801 is already assigned — real code from the free range at impl). Honest by construction (only declared-reversible types are covered). Semantic contract ratified now; the effect-region wiring follows **D-EFF1**. Ships with D-TXN2 | owner |
| 2026-06-22 | D-MIGRATE1 | compile-time enforcement of breaking data-shape changes (A): `#PublishedSchema` types have their field layout snapshotted at release (`.jet/cache/`); a breaking change without a declared migration is **E0910** (compile error, not a lint; the card said E0901 but that code is already assigned — use E0910, first free slot); `migration UserRecord { rename old -> new }` unblocks it. The CHECK is core sema (I3); the up/down conversion fns (`from_vXXX`) are generated by the Build-tier versioning library (#11). Bloat bounded by published-API × support-window (squash-to-baseline + support-floor). **Scope locked to the card's grammar** (`#PublishedSchema` + `migration { rename a -> b }`); other ops (add-with-default, type-change, delete) + `jet schema status`/`squash` verbs → follow-on **D-MIGRATE2**. Unblocked → build now | owner |
| 2026-06-22 | D-SOA1 | cache-friendly data layout (A): `#layout(soa) struct …` — whole-struct structure-of-arrays, field access (`p.x`) unchanged, layout is part of the type (consistent with D-ATTR1). **Syntax ratified; implementation deferred post-v1 (Later tier).** Owner: wants a better name than "SOA" → new naming ballot; 3 open Qs (partial `#layout(soa: f, …)`; future-reserve the Option-B `soa [T]` per-container spelling; `#Serialize`/reflection interaction) → new ballot **D-SOA2** | owner |
| 2026-06-22 | D-DBG2 | no-Jet-source-line frame policy (A + expert opt-in): **default A** — the DAP adapter steps over any frame absent from the `.jetmap` and surfaces only Jet frames (I2 intact — no Rust paths in the default view); **expert opt-in `jet debug --raw-frames`** surfaces the raw Rust frame (file+line) for adapter/expert debugging, an explicit, flagged I2 carve-out scoped to the debugger surface. Owner note: once Jet self-hosts there is no underlying Rust and the distinction dissolves. Implements c52's open policy (D-DBG1 verb + D-OBS1/2 already ratified) | owner |
| 2026-06-22 | D-DETACH1 | intentional task detach (A): `task.detach()` — a method on the spawn handle that consumes it and exempts it from L1101 ("task value dropped without `.join()`"); reads as a deliberate choice, quotable in the L1101 fix-it. A detached task that captures a borrowed `view` of the caller's scope is a compile error (it can outlive the borrow) with a "pass an owned `copy`/`share`" fix-it. Keeps one spawn verb | owner |
| 2026-06-22 | D-REPRC1 | C-compatible struct layout (**B**, not rec A): `#layout(c)` — C repr joins the **one `#layout(…)` family** alongside `#layout(soa)` (D-SOA1) and `#layout(packed)` / `#layout(align(N))`; codegen stamps `#[repr(C)]` on the generated struct. A growable field (`[U32]`) under `#layout(c)` is a compile error (use fixed `[U32#N]` or drop the annotation). Owner chose the unified-family fork the rec flagged → reconciles with D-SOA1/D-SOA2 (the SOA rename applies only to the `soa` slot; `c`/`packed`/`align` are sibling layout kinds) | owner |
| 2026-06-22 | D-STDIN1 | streaming stdin (A): `io.stdin()` handle with `.lines()` / `.read_line()`, mirroring the file reader (reuses the `FileLines` streaming type) so one idiom spans files + stdin and a fn written for one accepts the other; constant-memory. `read_all_input` stays as a small-input convenience; a `#Pure fn` reading stdin stays rejected (impure) | owner |
| 2026-06-22 | D-TERM1 | terminal direct-input primitive (surface **A** + name **`live`**): scoped `live { … }` block enters un-buffered/no-echo input for its body and **guarantees** restore on every exit incl. panic (built on D-DEFER1 scope-guard); keys via a `Key` enum. "raw mode" jargon dropped (`live` = rec; owner picked surface A, name taken as the recommendation). The full TUI (old Option D) is confirmed a **separate batteries-included `jet.tui` library**, not core — experts get the primitive, beginners get widgets on top. (termios-vs-bootstrap-crate is an I6 impl choice, not user-facing) | owner |
| 2026-06-22 | D-LSDIR1 | directory listing (A **+ C helper**): `fs.list_dir` returns `[DirEntry]` (`{name, path, is_dir}`) — the full path + type in one step, killing a class of separator bugs (return-type change to a shipped fn, called out). Per owner, **also ship `path.join(dir, name)`** (option C, portable join) alongside for experts needing finer control | owner |
| 2026-06-22 | D-CSVROW1 | typed CSV row decoding (A, **folded into the serde plan**): comptime `csv.decode<Row>(record)` walks `Row`'s fields (S57/S60 comptime, shipped) and maps columns by header name with coercion; a bad cell is a typed per-row error composing with the ratified `??` skip. Owner: CSV is **part of the unified serde model (D-SERDE1)** for toml/yaml/json/csv — not standalone; a future `#[CsvRow]` derive (C, gated on S56) must share A's one decoder path | owner |
| 2026-06-22 | D-LOGFMT1 | `jet.log` output format (A): auto-detect by TTY — human-readable text line when stderr is a terminal, JSON lines when piped; `log.setup(format: text|json)` overrides when detection guesses wrong. Same `log.info(…)` calls; format chosen at runtime. The text line layout is product copy → snapshot-tested. Implements c91 | owner |
| 2026-06-22 | D-FLOATW1 | sized-float math/precision policy (A): `core.math` functions are width-generic — `sqrt(F32) -> F32`, `sqrt(F64) -> F64` (full per-width path, F32 is a real precision choice not just storage); precision-losing moves are explicit (`.to_f32()`), mixing `F32`+`Float` is a compile error with a convert fix-it — consistent with D-SG9 (no implicit widening, named conversions). Policy only; **gated on D-SG9's sized floats being implemented first** (F32/F64 spellings ratified but the `Type` enum is still Int/Float) | owner |
| 2026-06-22 | D-STATE1 | typestate via transitioning tags (A): a fn `take`s the old state tag and returns the next; wrong-state call = compile error (E0150); tags erase, zero runtime cost. D-QUAL2 (tag kind) ratified → **unblocked**. Sequence `#SingleUse` (D-LIN1) machinery first | owner |
| 2026-06-22 | D-DET1 | `pure` ⇒ reproducible (A): inside `pure fn` reject wall-clock/OS-rng/fs/net + calls to non-`pure`; supply deterministic `Clock`/`Rng` as injected capabilities; `assume_deterministic { }` expert escape (semantic footgun, v1-legal). Subsumes Clock/Rng fork 2.5. **Gated on D-EFF1** (effect-tracking pass is the enforcement engine) | owner |
| 2026-06-22 | D-TXN2 | reject irreversible effects inside `#transact { }` (A): a net/fs/subprocess effect that can't be rolled back is a compile error pointing at the call; fix = move after block or `on_commit { }`. **Gated on D-EFF1** (effect classification); ships with D-TXN1 | owner |
| 2026-06-22 | D-EXT1 | library extensibility ceiling (A): Tier 0 vocabulary + Tier 1 blessed protocols **open to all**; Tier 2 marked DSL blocks **stdlib-only** (widen later on evidence); Tier 3 proc macros rejected (conflicts S26 no-macros law); Tier 4 sigils/keywords/grammar **rejected, even for experts**. Standing policy: local footguns allowed, global footguns rejected; mark library syntax; diagnostics are the ceiling | owner |
| 2026-06-22 | D-CTIO1 | comptime build-time I/O (B): ratify `embed_file(path)->String` + `embed_bytes(path)->[U8]`; path must be a string literal, resolved relative to source, no `..`-escape past project root; **no** broad build-time I/O (option C → far-horizon idea card). Implements the S26/S60 blessed exception | owner |
| 2026-06-22 | D-CTX1 | Smart Context grammar (G2): `#context(field: value) { … }` reusing Jet's single `name: value` spelling (S61/S29); `=` stays reassign-only (S17). Q1=A2 (explicit allocator-passing wins when present), Q2=Cβ (per-block) already owner-set; no single-field shorthand, bundle-spread deferred | owner |
| 2026-06-22 | D-ROUTE1 | HTTP route registration & dispatch surface (A) for `jet.http`: register routes with path patterns + `:param` extraction parsed for the handler, replacing the manual `request.path` if/match ladder. Implements c83 | owner |
| 2026-06-21 | D-CASING1 | tags PascalCase; traits PascalCase; "Core" not "std" (owner-directed casing/naming) | owner |
| 2026-06-21 | D-OBS2 | debug line-table is a sidecar `<file>.jetmap` JSON (versioned, std-only); part of the DAP debugger | owner |
| 2026-06-21 | D-ALLOC2 | arena `alloc` returns scope-bound `view`; use-after-reset/escape = compile error (E0631/E0632); region rule ratified (D-REGION1) → unblocked | owner |
| 2026-06-21 | D-REGION1 | allocation regions: implicit scope-inferred (A, beginner) + explicit `region r { … }` (B, expert) — both; unblocks D-ALLOC2 | owner |
| 2026-06-21 | D-TAINT1 | `#tainted` tag + `sanitizer fn`; tainted→sink is E0721 (gated on D-EFF1); full IFC (opt B) deferred post-Epoch-3 → D-IFC1 | owner |
| 2026-06-21 | D-QUAL2 | two qualifier kinds — `trait` (methods, dispatches) vs `tag` (no methods, erases); unblocks value-tags cluster | owner |
| 2026-06-21 | D-UNINIT1 | `#uninit` binding marker, gated by `use core.mem`; write-before-read proof (E0420) | owner |
| 2026-06-21 | D-REGEX1 | `jet.regex` on the Rust `regex` crate (owner-approved I6 bootstrap; native-ize before Epoch 3 ends) | owner |
| 2026-06-21 | D-SCAP1 | scoped capabilities: `#grant(fs) { caps -> … }`, RAII-revoked (gated on D-EFF1) | owner |
| 2026-06-21 | D-UNIT1 | units as `#unit(usd)` tag + `9.99.usd` literal (gated on D-QUAL2) | owner |
| 2026-06-21 | D-LIN1 | single-use values `#SingleUse` (renamed from `linear`; gated on D-QUAL2) | owner |
| 2026-06-21 | D-TGT1 | `targets:` list replaces `kind:` (kind removed; greenfield) | owner |
| 2026-06-21 | D-TGT2 | first targets: library, executable, test, example; benchmark/plugin reserved | owner |
| 2026-06-21 | D-TGT3 | bare keyword (no fields) or block (with fields) | owner |
| 2026-06-21 | D-TGT4 | bare `executable` searches `src/main.jet` then `<package>.jet` | owner |
| 2026-06-21 | D-TGT5 | `#test` fns auto-collected; optional `test { entry: … }` | owner |
| 2026-06-21 | D-CAP1 | capability words `view` / `edit` / `take` / `share` (edit, share new) | owner |
| 2026-06-21 | D-CAP4 | `api:` per-target field — `library { api: stable }` | owner |
| 2026-06-21 | D-CAP5 | library-producing targets emit capability metadata; binaries infer | owner |
| 2026-06-21 | D-CAP6 | inference is the library default forever; `api: explicit` opt-in | owner |
| 2026-06-17 | D-DEP1 | third-party deps ship as FFI-wrapping Jet packages (`extern rust`, S50); compiler stays zero-crate; native port later keeps API | owner |
| 2026-06-17 | D-NET1 | TLS via `rustls` delivered as the `jet.tls` package (D-DEP1); `jet.http`→`jet.tls`; no compiler crate | owner |
| 2026-06-17 | D-OBS1 | observability foundation (source maps + Jet-line panic reports) in M12; full DAP debugger at GA (M17) | owner |
| 2026-06-17 | D-OBS3 | OTel-aligned, std-only structured logs/metrics in M12; OTel exporter later as an FFI-wrapped Jet package | owner |
| 2026-06-17 | D-REPL18 | revised: `jet repl` ships a std-only line reader; no `rustyline` (compiler stays zero-crate) | owner |
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
| 2026-06-16 | D-CBIND6 | `#define` constants in bind output; skip function-like macros | owner |
| 2026-06-16 | D-CBIND2 | auto bind on compile + `jet bind` subcommand (same backend) | owner |
| 2026-06-16 | D-CBIND3 | bindgen helper crate (I6 waiver) | owner |
| 2026-06-16 | D-CBIND5 | `String` at C string boundary | owner |
| 2026-06-16 | D-LL2 | `@audit("…")` on `@unsafe` blocks | owner |
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
| 2026-06-16 | U11 | `System` fields: `target`(typed)/`packages`/`services`/`options` (D-SYS-FIELDS A) | owner |
| 2026-06-16 | U12 | `Service` open record (`enable: Bool` + typed per-service fields) (D-SVC A) | owner |
| 2026-06-16 | U13 | `options:` ordered list of bare `key: value` pairs, no `set()`; quotes only for free-form strings (D-OPTS) | owner |
| 2026-06-16 | U14 | `Image { from: system.X, format: iso\|qcow\|raw }`; target/packages inherited from system (D-IMG-FIELDS B) | owner |
| 2026-06-16 | U15 | jetos tier under `jetpack os <verb>` (switch/build); no separate binary (D-JETOS-BIN) | owner |
| 2026-06-16 | U16 | `jetpack os` target `[<config-path>]@<host>`; path defaults `~/.jet/config.jet`; `@host` selects System (D-CFG-LOAD) | owner |
| 2026-06-16 | U17 | a realized `library` package is consumed with `use <pkg>` (D-LIB-USE A) | owner |
| 2026-06-16 | U18 | inferred constructors: bare `{…}` elaborates to the expected type; explicit `Type {…}` optional (D-INFER-CTOR) | owner |
| 2026-06-16 | D-PAT6 | parameter destructuring deferred; unpack on first body line | owner |
| 2026-06-16 | S84 | hyphens allowed in package/module/system/image/env *names* (finalist 2); dashed-name `ident (-ident)*`, span-adjacent only; no lexer change | owner |
| 2026-06-17 | D-OS4 | jetos priorities syntax: **C — priority map** `[default: x, force: y]`; bare assignment with no map implicitly uses `default` priority; `force` requires explicit map form | owner |
| 2026-06-17 | D-OS6 | jetos user scope: **A** — `user.<name>.*` with `user.me` alias; additive multi-user with no restructure | owner |
| 2026-06-17 | D-REF2 | arena allocators ship in M5; live directly in `core.mem` (flat, not a submodule) | owner |
| 2026-06-17 | D-LIB2 | generics v1: **A** — associated types + default method bodies; no higher-kinded types | owner |
| 2026-06-17 | S19 | **amended (S19-amend A):** unified `loop` keyword; header picks mode (empty=infinite, bool=conditional, `in`=iteration); `while`/`for` retired to S14 teaching errors | owner |
| 2026-06-17 | D-JSON1-decode | JSON decode strictness: **B — lenient coerce** where unambiguous (`"8080"` → `8080`); only error on truly impossible conversions; implementation must surface coercions (see owner-todo.md). Note: S82/S55 reference a prior D-JSON1 for `@Serialize` config overrides — those are separate; this entry covers the decode-strictness ballot ratification. | owner |
| 2026-06-18 | D-MOD1 | code module system: **Rust's model with two surface swaps** — keyword `module` (not `mod`) and `.` (not `::`) for scoping. `module math;` finds `math.jet` then `math/module.jet`; `module math { … }` is an inline module; a missing file or an ambiguous match is a compile error (E0607). `use "path" as alias` stays as the ceremony-free single-file entry point. | owner |
| 2026-06-18 | D-MOD2 | import access: **two-step, dot notation, no wildcard** — `module math;` (or inline) then optional `use math.clamp;` / `use math.{a, b};` to bring items unqualified; `math.clamp(…)` qualified form always available; `use math.*` rejected (E0612). | owner |
| 2026-06-18 | D-MOD3 | visibility: **private by default, `pub` to export** — items don't escape their file/inline module unless `pub`; `M.private()` from outside is E0609; cross-file private access is E0605/E0102. | owner |
| 2026-06-18 | D-MOD4 | re-export surface: **Rust-exact `pub use`** (supersedes the 2026-06-17 auto-surface call). A directory module's `module.jet` must `pub use sub.Item;` to expose a submodule item; nothing auto-surfaces. A `pub`-but-not-re-exported item stays internal to the directory. | owner |
| 2026-06-18 | D-MOD-DIR | directory-module summary file is **`module.jet`** (not Rust's `mod.jet`), matching the `module` keyword. `module foo;` resolves `foo.jet` then `foo/module.jet`. | owner |
| 2026-06-18 | D-CBIND3 | `jet bind` backend: **native std-only C-prototype parser** in `Source/CBind.rs` (supersedes the ratified bindgen-crate + I6-waiver route). No external crate, no libclang, no `cbind` feature. Binds the C subset Jet's FFI uses (scalars, `char*`→String, `void`); unbindable declarations are skipped and reported (never faked, I3). Anything beyond the subset stays a hand-written `@extern module c.<lib>` overlay. | owner |
| 2026-06-18 | D-JPK-FILES | jetpack file structure: `jetpack.toml` (TOML monorepo manifest) + `env.jet` (dev env) at **repo root**; `pkg.jet` (package definition, renamed from `payload.jet`) in user-chosen package dirs; `.jet/` holds only generated `lock`+`cache/`. Revises U1/U10; `config.jet`/jetos tier deferred to E3. | owner |
| 2026-06-18 | D-ILE1 | lib/exec **inferred from `fn main()`** (A): no `pkg.jet` → file with `main()` is executable else library (two `main()` = E_DUPMAIN); with `pkg.jet`, the `packages:` `kind` is optional/inferred, user-overridable. Amends U10/D-JPK-FILES | owner |
| 2026-06-18 | D-BIND1 | full Odin binding sigils `name :: expr` (immutable) / `name := expr` (mutable) (A); `val`/`var` retired to teaching errors; **`::` spent** (S83 needs a new separator); amends S2 | owner |
| 2026-06-18 | D-LABEL1 | labeled loops `@name loop { }` + `break @name`/`continue @name` (B); reuses S82 `@` in a new inline position; rejected bare `name:` (A) and `'outer` (S41 char-literal clash); amends S19/S23 | owner |
| 2026-06-18 | S6-R | **no visible semicolons** (B); Go-style lexer terminator insertion; `-> Type {` stays on the param-close line; **continuation suppressed** when the next line starts with `.` (S69 chain) or a binary/logical operator; `E_MISSING_SEMI` retired; supersedes S6's required-`;` rule | owner |
| 2026-06-18 | D-IF1 | `if` is the **universal branching keyword** (A); `when` retired to a teaching error; multi-arm `if subject { … }` with **inferred comparator** (bare `200 ->` ≡ `subject == 200`, reverses S24); amends S24/S68 | owner |
| 2026-06-18 | D-IF2 | multi-arm `if` surface: catch-all is **`else ->`** (Q1-B; `...` rejected); **braceless arm bodies** allowed, `{ }` for multi-statement (Q2-A); bare-value-vs-condition is a **structural mix** (Q3-A — head with no top-level comparison op = bare value, prepend `subject ==`); amends S68/D-IF1 | owner |
| 2026-06-19 | D-ATTR1 | attribute/marker sigil **`@` → `#`** (B): `#unsafe`, `#Serialize`, `#audit("…")`; `[T#N]` and `name#ver` keep `#` (non-marker position); teaching error rejects `@unsafe`→`#unsafe`; reverses S55/S82 spelling, not semantics. **Ratified, implemented 2026-06-20** | owner |
| 2026-06-19 | D-ATTR2 | multi-marker list **bare `#[Serialize, Comparable]`** (A); Rust-literal `#[derive(…)]` (B) stays rejected per S55; only the sigil changed. **Ratified, implemented 2026-06-20** | owner |
| 2026-06-19 | D-ATTR3 | loop labels **stay `@`** (B): attributes move to `#`, labels keep `@outer loop { break @outer }`; two marker sigils coexist in source (the flagged trap, chosen knowingly). **Ratified, implemented 2026-06-20** | owner |
| 2026-06-19 | D-NARG1 | named args + defaults **on methods/constructors** (A): call-site labels checked, trailing defaults fill; closes the S61 method gap (label parsed then dropped). **Implemented 2026-06-19** (Source/Sema/mod.rs: MethodSig param_info/defaults; Source/Sema/CheckerItems.rs: check_method_args label+fill; Source/Sema/Registration.rs: L2401 on pub methods; examples/features/63_named_args.jet; tests/ui/method_label_mismatch.*; tests/ui_lint/l2401_method_bool.*) | owner |
| 2026-06-19 | D-NARG2 | fmt **preserves** call-site labels (A): never adds or strips; canonicalization deferred to the LSP quick-fix (S14/M6). **Implemented 2026-06-19** (Source/Formatter/Expressions.rs fmt_call_args already preserves labels; verified no change needed) | owner |
| 2026-06-19 | S29-FLUSH | **flush constructor block** `Point{x: 3.0, y: 4.0}` (A; amends S29); flush also for destructuring `Point{x, y} :: make()`; `: ` colon spacing unchanged; formatter-canonical change. **Implemented 2026-06-19** (Source/Formatter: StructLit + fmt_bind_pattern; tests/fmt.rs) | owner |
| 2026-06-19 | D-CTOR1 | **named constructors only** (A): many shapes = many named no-`self` statics returning the type; duplicate name = E0105 (teach naming each); overloading rejected (only disambiguates when sigs differ). Zero codegen change; formalizes `Point.unit()`. D-CTOR2: no marker keyword. **Implemented 2026-06-19** (Source/Sema/Registration.rs method_defined_twice ctor-aware fix hint; tests/ui/named_constructors.*) | owner |
| 2026-06-19 | D-ALLOC1 | **allocator method style** (A): `mem.Arena.new()` + `arena.alloc(value)`; capacity as optional S61 default (subsumes C); free-builtin form (B) rejected; arena not `@unsafe` (`use core.mem` gate, D-ALLOC-B); ships with D-REF2. **Ratified, not yet implemented** | owner |
| 2026-06-19 | D-CTOR2 | **no constructor marker** (A): a no-`self` static returning the type *is* a constructor; no `new`/`init`/`@constructor` keyword. Confirms D-CTOR1. **Implemented** (shape is the signal) | owner |
| 2026-06-19 | D-ALLOC-C | **all four allocators now, grouped** (C): `Arena`/`Bump`/`Pool`/`Fixed` all ship, namespaced under **`core.mem.alloc`**. Refines D-ALLOC1. **Ratified, not yet implemented** | owner |
| 2026-06-19 | D-ALLOC-D | **both `reset` and `free`** (C): `reset` keeps the backing buffer for reuse, `free` returns it to the OS — two verbs, two lifetimes; use-after error names the site. **Ratified, not yet implemented** | owner |
| 2026-06-19 | D-NARG-D2 | **defaults may reference earlier params** (A): `fn box(w: Int, h: Int = w)` allowed. Owner: hard work on the backend so the frontend feels magic, while exposing expert tools. **Ratified, implemented 2026-06-20** (current default-fill treats defaults as self-contained; extend to allow earlier-param refs) | owner |
| 2026-06-19 | D-NARG-D4 | **dedicated label-mismatch diagnostic** (A): transposed/unknown call-site labels get their own teaching code, not folded into E0104. **Ratified, implemented 2026-06-20** (D-NARG1 currently reuses E0104; carve out a dedicated code + snapshot) | owner |
| 2026-06-19 | D-JSON3 | **surface JSON coercions via a log line** (B): lenient decode (D-JSON1) emits one log line per coercion; decoded value comes back plain. **Ratified, not yet implemented** | owner |
| 2026-06-19 | S53 | **concurrency: tasks & channels — direction ratified** (A). Owner caveat: relook the surface syntax AND the memory-capability model first (major potential impact) before diving deep. **Not ready to implement** | owner |
| 2026-06-19 | S56 | **user-defined derives + typed reflection — direction ratified** (A). Surface uses the external-definition connector `~~` (S83): `derive Point~~Serialize`, mirroring `impl Point~~Drawable`. **Ratified (direction); connector resolved to `~~`** | owner |
| 2026-06-19 | S60 | **compile-time pure evaluation + data embedding — ratified to pursue** (A); `comptime` Layer 2 promoted from post-1.0. **Ratified, not yet implemented** | owner |
| 2026-06-19 | D-OS1 | **jetos config/platform — research-now, implement-post-E3** (C). Hold implementation until post-Epoch-3 (stable core); meanwhile research + document the end-state syntax, back-propagating from a magical end-user experience with core support (esp. pure eval). Documented as a research avenue to explore at the owner's discretion. **Held; research path documented** | owner |
| 2026-06-19 | S83 | **external-definition connector = `~~`** (double tilde, D). Attaches an out-of-body definition to a type, Type-first: `fn Point~~dist(self)`, `impl Point~~Drawable`, `derive Point~~Serialize`. Free token (whole tilde family was unspent); never collides with `->` (return/arm) or `=>` (lambda). Resolves S56's derive spelling. **Ratified, not yet implemented** | owner |
| 2026-06-19 | D-TOOL-SPLIT | **one bundled `jet` binary** (A): fmt/lint/lsp are `jet` subcommands sharing the front end (I2/I3 — they need the real lexer/parser/sema). Reserve splitting only the LSP *artifact* (not codebase) if editor release cadence ever demands it. **Ratified, not yet implemented** | owner |
| 2026-06-19 | D-PATW | **`_` for ignored payload fields, `else` for catch-all** (D, split): `_` ignores one slot (`Active(_)`); `else ->` stays the only tail catch-all (no bare `_` arm). `_` special-cased in pattern position (still a legal ident char + S34 separator elsewhere). **Ratified, implemented 2026-06-20** | owner |
| 2026-06-19 | D-PATR | **range patterns at all positions, reuse S22 `..`** (A): an arm head or a payload slot may hold `lo..hi`; the checker gap-checks coverage; open `Int`/`Char` always still requires `else`/`_`. c20/D-PATR owns range-pattern meaning + exhaustiveness at all positions. **Ratified, implemented 2026-06-20** | owner |
| 2026-06-19 | D-PATO | **`|` (single pipe) for structural or-patterns** (B): `Active(id) | Reconnecting(id) -> …`; alternatives must bind the same names at the same types (E0317). `||` stays value-distribution/boolean; new pattern-only meaning for `|`. **Ratified, implemented 2026-06-20** | owner |
| 2026-06-19 | D-RANGE1 | **range arms reuse inclusive `..`, desugar to `>= && <=`** (A): `90..100 -> "A"`; one range token across loops (S22), slices (S40), and arms. Exhaustiveness governed by D-PATR. **Ratified, implemented 2026-06-20** | owner |
| 2026-06-19 | D-RANGE2 | **arm-head range ownership** (A): S22 owns the `..` token; c20/D-PATR owns arm-head range *semantics* (checking + exhaustiveness); c25 owns only the terse `lo..hi ->` sugar + `..=`/`step`/inverted-band porting-error teaching, shippable first under D-PATR's rules. One spelling, one checker. **Ratified, partially implemented 2026-06-20 (range arms shipped via c20/D-PATR; `..=`/`step`/inverted-band teaching pending c25)** | owner |
| 2026-06-19 | D-ERR-CONV | **`impl Source -> Target { … }`** (A): reuses `->` + `impl`; conversion declared once, total, rejected unless declared (orphan rule S28 applies); `?` applies it; `Fallible` unifies as prelude `impl T -> Error`. Owner asked whether S83's `~~` belongs here — **no**: `~~` attaches a member to a type, an error conversion is a distinct construct (a `Source→Target` declaration), so `->` is correct (verified vs `tools/Tower/docs/sidequests/typed-error-families.md`). **Ratified, implemented 2026-06-20** | owner |
| 2026-06-19 | D-DIST1 | **`UserId :: distinct Int`** (C, binding form): reuses the ratified `::` immutable sigil (D-BIND1) + the `distinct` keyword; no new separator token; `distinct`-over-`distinct` chaining rejected in v1. **Ratified, implemented 2026-06-20** | owner |
| 2026-06-19 | D-DIST2 | **units of measure: in scope, delivered as a stdlib extension** (B + owner comment): units are NOT deferred, but ride on top of distinct types (D-DIST1) via a **stdlib extension layer, not core-language syntax**. Distinct types ship in core; dimensional algebra (derived units) lives in stdlib. **Ratified, not yet implemented** | owner |
| 2026-06-19 | D-WHEN1 | **`comptime if`** (A): reuses two ratified words; condition is a comptime expression; only the selected arm is checked + lowered. Extends S57's bindings-only comptime scope; the S26 dispatch law (no comptime type/trait/generic selection) is unchanged. **Ratified, implemented 2026-06-20** | owner |
| 2026-06-19 | D-WHEN2 | **unselected `comptime if` arm: name-resolution only** (A): the dropped arm is scanned for unknown names (typos still teach) but not type-checked against its surroundings (off-target intrinsics allowed). **Ratified, implemented 2026-06-20** | owner |
| 2026-06-19 | D-NARG-DIAG | **E0125 (label mismatch) + E0126 (later-param ref in default)** (A): two purpose-built teaching codes (product copy for the ratified D-NARG-D4 / D-NARG-D2 follow-ups); E0125 covers transposed + unknown-label sub-cases, E0126 teaches "reorder so the param comes first." **Ratified, implemented 2026-06-20** | owner |
| 2026-06-19 | D-CLI1 | **error & teach on an unknown `--`-flag before `--`** (A): extend the existing E2102 path's Fix line to point at `jet run app.jet -- --flag`; no silent forwarding of typo'd jet flags. **Ratified, implemented 2026-06-20** | owner |
| 2026-06-19 | D-L0201 | **liveness-gate the implicit-clone lint** (A): fire L0201 only when the cloned value is dead after the call (a wasteful clone); stay silent when it's reused. Needs a last-use analysis threaded through the four firing sites. **Ratified, implemented 2026-06-20** | owner |
| 2026-06-19 | D-DBG1 | **`jet debug <file>`** (A): a dedicated verb parallel to `jet run` / `jet test`, discoverable in `jet --help`; the editor launches the same command. **Ratified, not yet implemented** | owner |
| 2026-06-19 | D-EVAL1 | **`jet eval --pure`: pretty by default, `--json` for machine output** (A): humans get indented, Jet-typed output; pipelines opt into the existing compact stable JSON via the global `--json` flag (no new `--pretty`). **Ratified, not yet implemented** | owner |
| 2026-06-20 | D-DIST3 | **distinct types: explicit both directions, opt-in same-type arithmetic** (A): construct `UserId(expr)`, unwrap with `.raw()` (S42 named-cast family); **no** implicit base↔distinct coercion either way; arithmetic only via a `#Numeric` marker and only between two values of the *same* distinct type, else E0127. Completes c23 (with D-DIST1/D-DIST2). **Ratified, implemented 2026-06-20** | owner |
| 2026-06-20 | D-PRELUDE1 | **`print` + `input` ambient** (B): the two primitives a first interactive program reaches for work with no `use`; `eprint`, `args`, `read_all_input` stay qualified behind `use core.io`. **Ratified, implemented 2026-06-20** | owner |
| 2026-06-20 | D-CT-L2NAME | **disambiguate the two "Layer 2" names** (A): keep both numbers; add a cross-reference note on S60 marking it the capability tier, not the S26 derive layer. Docs-only; no code, no rename. **Ratified** | owner |
| 2026-06-20 | D-DEFER1 | **user-writable scope-exit cleanup** (B): ship a stdlib `core.scope.guard(() => {…})` value whose Drop runs a stored lambda, firing LIFO on every exit path including `?`. No new syntax; `defer` (C) stays declined (D-SUGAR5); user-definable Drop remains the long-term roadmap item. **Ratified, implemented 2026-06-20** | owner |
