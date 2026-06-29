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
by D-BIND1**; **immutable sigil superseded again 2026-06-22 by D-BIND2 — `::` → `@=`**)*:
bindings use **Odin-style sigils** — ~~`name :: expr`~~ **`name @= expr`** (D-BIND2) for an
immutable binding, `**name := expr**` for a mutable binding, with an optional
type annotation before the sigil (`ratio@ Float=  3.14`, `count: Int=  0`).
`=` stays reassignment of an existing `:=` binding (S17). The former keywords
`**val**` / `**var**` are **retired to teaching errors** (E_KEYWORD_RETIRED →
"use `name :: value` / `name := value`"). Rejected: `set` (sounds like
mutation), `let` / `let mut` (Rust; teaching errors only per S14), and the
partial `:=`-only adoption that kept `val` (D-BIND1 option B). The owner
accepted **spending the `::` token** on immutable bindings — see D-BIND1; S83
(external definitions) must now pick a different separator.

**S18 — Visibility** *(ratified 2026-06-11; amended 2026-06-26)*: **private by default**;
prefix `**pub`** to export an item. Applies to top-level functions (M0+),
types and their fields (M3), and any future module-level bindings.
Within a file, private and `pub` items are equally visible to each other;
`pub` only controls what other files may access via `use` (S16, M6+).
Rejected: public-by-default (Go), explicit `private` keyword (noisy).
Considered and declined (owner, 2026-06-12): grouped visibility —
Jai-style `pub { }` blocks and top-of-file export lists.
*Amendment (D-VISDEFAULT1=C, 2026-06-26)*: A **file-scope visibility marker** is
approved — a single marker that flips the default for an entire file (or a section
below the marker) to public-by-default, letting files that are primarily public API
omit per-item `pub`. The exact syntax of the marker is pending D-VISDEFAULT2 (ballot
open). Once decided, the S18 baseline (private-by-default) remains; the marker is an
explicit opt-in.

**S10 — Ownership keywords (M2)** *(ratified 2026-06-11; **spelling superseded 2026-06-23
by D-CAP7** — `mut`→`~`, `take`→`^`, `view`→`&`, default-read→bare `T`)*: `**mut`**
(mutable borrow), `**take**` (move), `**view**` (borrow return type),
`**ref**` (stored field, tier 2). Default parameter access has no keyword
(shared read). Rejected: `read` / `write` / `owned` as canonical forms.
**Retired 2026-06-24:** `mut`/`take`/`view` are no longer keywords — they lex only
to fire the E0056/E0057/E0058 teaching errors pointing at `~`/`^`/`&` (see D-CAP7
migration note). `ref` stays a live keyword.

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

**S25 — Comparison distribution (M1)** *(ratified 2026-06-11; **retired
2026-06-29 by D-S25-RETIRE1**)*: in a
`&&`/`||` chain, when the right side is a plain value instead of a yes/no,
the nearest comparison to its left is re-applied to it:
`day == "mon" || "tue"` means `day == "mon" || day == "tue"`. Works for
chains (`x == 1 || 2 || 3`) and every comparison operator
(`x != 1 && 2`). The value's type must match what was compared. When the
values really are different things, write the full comparisons as usual.
Rejected: always requiring full repetition (noisy), a set-membership
construct like `x in (1, 2)` (a whole new form for the same idea).
**[reopened 2026-06-26 by D-MATCHARM1]** in match arms `||`/`&&` stop distributing the
comparison and become boolean combinators; a new single `|` takes over value-alternation
(`400 | 404`), parens group, and a left-value-less boolean tests the subject implicitly.
**[retired 2026-06-29 by D-S25-RETIRE1]** `||`/`&&` comparison distribution is gone
everywhere. Comparator value alternatives are single `|` (`x == 1 | 2` in an arm
head, or `1 | 2` under an inferred comparator). `||` and `&&` combine boolean
expressions/guards only.

**S14 — Alias policy** *(ratified 2026-06-10)*: One canonical spelling per
construct; **no aliases, ever**. v1: the compiler recognizes common foreign
syntax (`and`, `try`, `let`, `set`, `func`, `def`, `println`, `Text`, …) and the error
teaches the canonical form.
Later (M6): the LSP offers an autocorrect quick-fix for foreign syntax and
`fmt` canonicalizes, so non-canonical input never survives to disk. True
dual forms are rejected permanently.

**S4 — Type annotations (M1)** *(ratified 2026-06-11; **explicit-binding form amended
2026-06-26 by D-BINDEXPLICIT1**)*: `**name: Type`**
after the binding or parameter name (e.g. `val x: Int =  1`). Rejected:
`Type name` before (C/Java). **[D-BINDEXPLICIT1, 2026-06-26]** in an explicit-typed
*binding* the mutability marker now hugs the name and the type goes bare: `name@ Type =  val`
(immutable) / `name: Type =  val` (mutable), with `=` binding-or-reassigning per the name's
marker. The inferred `name @= val` / `name := val` forms are unchanged; this reopens D-BIND2's
`=`-is-reassignment-only invariant.

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

**S29 — Struct construction (M3)** *(ratified 2026-06-11; **the dotless form
retired 2026-06-25 by D-DOTCTOR2=A**)*: every field name required exactly once;
order may differ from the declaration. **Canonical spelling is now `Type.{ field:
expr, … }`** (leading dot), matching named-enum construction `T.Variant` and the
inferred `.{ … }` (D-DOTCTOR1=A). The original dotless `Type { … }` is **removed**;
typing it is teaching error **E0320** (fix: insert `.` before `{`). Rejected:
call-style `Point(x: 1.0, y: 2.0)` (B), required factory `new` (C). Parser
disambiguates `ident.{` from blocks in condition position.

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
**[amended 2026-06-26 by D-ENUMDOT1]** enum-variant patterns in match arms take a leading dot
(`.Circle(r)`, `.Empty`), reading as "a member of the inferred enum". Value-position dot
(`.Red` where the type is known) is the open follow-on D-ENUMDOT2.

**S31 — Pattern tests (M3)** *(ratified 2026-06-11)*: `**==`** with a
pattern right-hand side when the left operand is an enum or `T?` —
e.g. `if s == Circle(r) { … }`, switch arms `s == Rect(w, h) -> { … };`,
`if x == value(n) { … }`, `if x == null { … }`. The result is a `Bool`
(S24-compatible). When every arm of a `switch` is `subject == <pattern>`,
sema checks exhaustiveness and `else` may be omitted; mixed arms keep
S24's mandatory `else`. Otherwise `==` is ordinary value equality (S13).
A bare name on the right is a variable when one is in scope; to test a
unit variant with the same spelling, qualify it (e.g. `Light.Red`).
**[amended 2026-06-26 by D-ENUMDOT1]** a variant pattern in a match arm now takes a leading dot
(`.Circle(r)`), which resolves this bare-name-vs-variable ambiguity directly.
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
`val s: Stack<Int> =  empty_stack()`). Rejected: square-bracket generics,
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
impl Point.Serialize {
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
Go implicit interfaces, `::` in Jet paths. **[reopened 2026-06-26 by D-IMPLDOT1]** the trait
separator is now `.` (`impl Type.Trait`, reading "Type's Trait"), overriding this entry's
explicit rejection of that spelling and retiring the S83-reserved `~~` trait-attach direction.
Jet defaults to PascalCase for
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
val fruits: [String, Float] =  [
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

**S43 — Test syntax (M6)** *(ratified 2026-06-12; amended 2026-06-16, S82; amended 2026-06-21, D-CASING1)*:
top-level **`#Test("name") { … }`** blocks, using `**require**` and `**require_eq**`
(M4/S36) for assertions. `jet run`/`build` ignore test blocks; `jet test` runs them.
`test` (lowercase) is a teaching error pointing at `#Test` (E0052). Rejected: `#[test]`
attributes, `fn test_*` naming convention, `@test fn` (former S82 form), quoted-name
test blocks without the `#Test` marker (former S43 spelling).
**[amended 2026-06-26 by D-TESTPAREN1]** a named test wraps its name as a parenthesized marker
argument: `#Test("name") { … }`. The `#Test fn` property form (D-TEST1) is unchanged — it has
no name string to wrap.

**S44 — Formatter style (M6)** *(ratified 2026-06-12)*: one true style,
zero config — **4-space indent**, **same-line `{`**, **line width 100**,
spaces around binary operators; **one statement per line in multiline bodies,
but a brace body the author wrote on a single line is preserved as-is when it
holds one simple statement, contains no inner comment, and fits within the
100-column width** (author-intent preservation, matching S69 for dot-chains);
single blank line max between items, no space before `,`/`(` of a call; no
visible `;` (S6/S6-R — the lexer inserts terminators). `jet fmt` is the only
formatter; no style knobs. Rejected: configurable width/indent,
significant-indent formatting.

*Revised by D-FMT1 (owner, 2026-06-24):* `jet fmt` output is **idempotent**
(`fmt(fmt(x)) == fmt(x)`) but no longer a strict canonical function of the AST —
a single-statement body's line shape follows the author's source. This is an
intentional trade of layout canonicality (non-binding per philosophy.md #4,
which exempts structural arrangement from the "one mechanical path" priority)
for author-respecting output (philosophy.md #2, beginner experience). The rule
applies uniformly to all brace bodies — `if`/`else`, `while`/`for`/`loop`,
`fn`, dispatch arms, and if-expression branches; if any branch of an if/else
chain is multiline the whole chain expands.

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

**S58 — Expert low-level tier** *(ratified 2026-06-12; **amended 2026-06-16
(S82, D-LL2), 2026-06-22 (D-UNSAFE2), 2026-06-23 (D-CAP9)**)*: **two gates, one keyword.**
`**use core.mem**` is the discovery gate — unlocks the low-level
vocabulary: explicit **Zig-style allocators** (allocating APIs take an
allocator parameter; a fixed arena works on embedded), the raw-pointer type
`**\*T**` (D-CAP9; `Ptr<T>` is a deprecated alias that teaches `*T` — **E0210**),
layout/repr control, volatile wrappers. The audit gate for operations that can
violate memory safety — pointer **deref** (postfix `p.*`), **raw-pointer-of**
(prefix `*x`), pointer math, transmute-class casts, FFI pointer crossings — is
**`#Unsafe("reason") { … }`** (D-UNSAFE2 folded the audit reason into the gate
itself; the separate `#Audit` marker is retired → **E0055**; lint **L3101** if the
reason is missing). **`#Unsafe`** on the line before `fn` marks a whole-function
contract; calling one requires an enclosing `#Unsafe` block. Address-of is
`mem.address_of(x)` (a call, not a sigil — the earlier "`&x` = address-of" claim
never shipped). Raw pointer ops are **core grammar, sema-gated**: outside an
`#Unsafe` region both `p.*` and `*x` emit the **E0208** teaching error. **D-CAP9
(resolved 2026-06-23):** prefix `*x` means *only* raw-pointer-of; **dereference is
postfix `p.*`** (Jai precedent), composing with `.field` as `p.*.field`; `*T` is
the canonical raw-pointer type and `Ptr<T>` its deprecated alias. Codegen lowers
blocks to Rust `unsafe`; **I1 is amended** — generated `unsafe` appears only inside
user-gated regions plus vetted std/mem internals. Onboarding materials never
mention any of it.
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
parameter values ride along: `fn f(x: Int, urgent: Bool =  false)` —
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
(`impl Service.Logger using logger;`). The field's type must implement
the trait; forwarding is all-or-nothing in v1 (partial override
deferred). Rejected: Jai-style field hoisting / `using` member
injection (invisible names), Rust Deref-abuse delegation.
**[amended 2026-06-26 by D-IMPLDOT1]** the forwarding separator follows S28's switch to `.`:
`impl App.Logger using logger`.

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
2026-06-13, **renamed `std` → `core` 2026-06-16**; **`jet.core` long form retired
— `core` is the sole name, owner 2026-06-26, D-CORENS1**)*: the core library is
**exported as the `core` module** — a module `use` (S16 form 2, no quotes), not
a file path. `core` is the one canonical name; there is no `jet.core` or `std`
spelling. Dot paths select submodules:

```
use core;                         // whole core → namespace core
use core.fs as fs;                // submodule, optional alias
use core.io;                      // default namespace io
```

`core` and `core` are compiler-reserved module roots; `core.<module>` and
`core.<module>` select compiler-known submodules (`fs`, `io`, `json`,
…). Optional `as alias` works like S16. Core is never used via a quoted
path — `use "core/fs"` is wrong because `"core/fs"` is file-path syntax;
use `use core.fs`. The former spellings `import std` / `use std` /
`use core.fs` emit a teaching error pointing at `core` (S14). Rejected:
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
inline pins remain authoritative). (Native C deps later moved into the Jet
`deps: { lib: c@system }` ref, S59/D-CFFI2 — no separate `[dependencies:c]`
table.) Reserved, not generated in v1:
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

**Link resolution (D-CFFI2, ratified):** C deps live in the Jet `deps: { … }`
block as a `c@<target>` provider ref — `lib: c@system` (pkg-config, with a bare
`-l <lib>` fallback when there is no `.pc`, e.g. libc) or `lib: c@"vendor/path"`
(local dir: `-L`/`-I`/`-l`). Order:

1. **Declared `<lib>: c@…` dep** in `pkg.jet`'s `deps:` block → resolve as above.
2. **Otherwise** — `pkg-config <link-name>` (an undeclared `use c.<lib>`).
3. **Missing** — **E3201** naming both fixes.

A C dep is a link dep, not a Jet package: it is skipped in package realization
and never written to the package lock. The retired TOML `[dependencies:c]` table
is replaced by this `c@…` ref (no alias).

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
    fn init_window(w: Int, h: Int, title: String) =  "InitWindow";
}

// src/c/raylib.jet (optional overlay)
@extern module c.raylib {
    fn draw_text(text: String, x: Int, y: Int, size: Int, color: Color) =  "DrawText";
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
| `jetpack.toml` | TOML | repo root | repo metadata and source defaults: `[repo]`, `[sources]`; `[packages]` moved to `workspace.jet` by D-WORKSPACE1 | yes |
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
`impl MyFail.Fallible { fn to_error(self) -> Error { … } }`. The default
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
`enum`, or `fn`. Covers derive-like markers (`@Serialize`, `@Comparable`), and
whole-item effects (`@transact`, `#Unsafe` on a function). **`comptime`** bindings
stay prefix keywords. Note: harness markers (`#Test`, `#Todo`, `#Pure`) use `#`
per D-ATTR3=B and D-CASING1 — they do not use the `@` prefix.

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

#Test("reversing twice") {
    require_eq(reverse(reverse([1, 2, 3])), [1, 2, 3])
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
named form `T.{ … }` (S29, dot-prefixed by D-DOTCTOR2=A) stays legal as an escape
hatch and wherever no expected type is inferable (a bare binding, an ambiguous
union); there, an un-annotated `{ … }` is a diagnostic ("name it, e.g.
`System.{ … }`"). Field typos still report
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
**[amended 2026-06-26 by D-LOOPLABEL2]** the `@` moves from a prefix to a **suffix** on the
label name, at declaration and at break/continue: `outer@ loop { break outer@ }`. Codegen still
maps to Rust `'name:` labels.

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
**[amended 2026-06-26 by D-MATCHARM1]** the arm-head operator model is reopened: single `|`
alternates values, `||`/`&&` combine a value-pattern with a boolean expr (no longer
distribute the comparison, per S25), and parens group. Precedence is the open follow-on
D-MATCHARM2.

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

**D-ATTR3 — Loop labels stay `@`** *(ratified 2026-06-19, option B; **reversed
2026-06-26 by D-LOOPLABEL2** — label `@` is now a suffix `outer@`, not a prefix)*: attributes
move to `#` but labels (D-LABEL1) keep `@` —
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
S29; **dot-prefixed 2026-06-25 by D-DOTCTOR2=A**)*: the canonical construction
style is flush — the type name hugs its field block the way a call's `(` hugs its
callee, now with the leading dot: **`Point.{x: 3.0, y: 4.0}`**; colon spacing
(`x: 1`) keeps the language-wide `: ` rule. The flush rule extends to destructuring
patterns (**`Point.{x, y} :: make()`**) for build-vs-match symmetry. This is a
formatter-canonical-style change layered on D-DOTCTOR2's removal of the dotless
form.

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

> **Spelling superseded 2026-06-23 — see D-CAP7.** D-CAP2/D-CAP3 later ratified
> (2026-06-22), then the owner replaced the whole word vocabulary with prefix sigils
> (`T`/`~T`/`^T`/`&T`/`*T`). The four capabilities are unchanged; only the spelling moved
> from words to sigils. D-CAP4/5/6 (metadata) are unaffected.

**D-TGT1 — `targets:` replaces `kind:`** *(ratified 2026-06-21, option B; owner:
"fully remove kind, we are still greenfield"; supersedes U10 / D-ILE1 on the kind
field)*: a package declares a **`targets:` list**, not a `kind:`. `kind:` is **removed
entirely** — no deprecation alias; a `kind:` field in `packages:` is now an unknown
field (teaching error → "write `targets: [ … ]`"). When `targets:` is omitted the
D-ILE1 inference carries forward onto the new vocabulary: a module with `fn main()`
infers `[executable]`, otherwise `[library]`. Rejected: augmenting `kind:` with a
parallel `targets:` (option A — two ways to say one thing).

**D-TGT2 — first-increment targets** *(ratified 2026-06-21, option A; `benchmark`
backend shipped 2026-06-25, c80)*: the shipped targets are **`library`**,
**`executable`**, **`test`**, **`example`**, **`benchmark`** — the five with working
build paths. **`plugin`** remains a **reserved** target keyword (c81/D-DEP-WASM1):
writing it is a teaching error ("target `plugin` has no backend yet"), not an
unknown-keyword error. `benchmark` (c80) routes `jet bench` at the package entry
via the existing `#Bench`/`jet bench` engine — no new mechanism (I8). Rejected:
shipping all six now at ratification (option B — keywords with stub backends).

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

**D-CAP1 — capability keyword spellings** *(ratified 2026-06-21, option A; **spelling
superseded 2026-06-23 by D-CAP7** — the four capabilities survive, now sigils `T`/`~`/`^`/`&`)*: the
four-capability vocabulary is **`view` / `edit` / `take` / `share`**. `view` and `take`
are already ratified ownership keywords (S10); **`edit`** and **`share`** are new
reserved capability words. Parameter-position placement is still **open (D-CAP3)** and
the copy/share call form is **open (D-CAP2)** — only the spellings are fixed here.
Rejected: reusing `mut` for the edit slot (option B — reads as Rust `&mut`); `read` /
`write` / `own` (option C — S10 already rejected these); `look` / `change` / `keep`
(option D — orphans the live `take` / `view` keywords).

**D-CAP4 — `api:` is a per-target field** *(ratified 2026-06-21, option D; rides
D-TGT3 blocks; **c129 implemented 2026-06-25**)*: a library target records its public
capability signatures by setting **`api:`** inside its target block —
`library { api: stable }` (record + flag API breaks) or `library { api: explicit }`.
Default is inference (D-CAP6). Rejected: a top-level `api:` field (option A), a
`payload api = …` statement (option B), an attribute (option C).

**c129 — capability freeze (implemented 2026-06-25, under D-CAP4/D-CAP6/D-CAP8).** The
manifest `api:` mode is now surfaced (`PackageManifest::ApiMode`, default `Inferred`).
For a `stable`/`explicit` library, `jet publish` freezes each public function's resolved
capability signature into durable interface metadata at
`.jet/cache/api/<package>.api` (`Publish::ApiFreeze`). A sema pass
(`Sema::CapabilityFreeze`, runs after `resolve_capabilities`) diffs the resolved
signature against that frozen contract on every build: a read → `~`/`^`/`&` drift is
**E0912**, a breaking change, never a silent flip (D-CAP8). The frozen capability digest
is folded into the package pin (`Lock::compute_fingerprint`), so a public capability
change shifts the lock hash. Gated on unbuilt publish/registry: only the *registry
upload* of the frozen `.api` (D-PKGS1 deferred, c96 ballot) is outstanding — the in-
compiler freeze, drift detection, and pin are complete.

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

**D-CAP7 — capability is a prefix sigil, not a keyword** *(ratified 2026-06-23, owner
mandate)*: the four value-access capabilities are spelled as **prefix sigils on the
type**, replacing the word vocabulary of D-CAP1/2/3. The surface is owner-frozen
(`docs/prompt-memory-model-final.md`) — do not re-spell it:

```jet
T     // infer: starts at read/view, elevates only as the body requires
~T    // edit:  exclusive write/mutate access
^T    // take:  ownership moved/consumed
&T    // share: may escape the scope, be retained, cached, spawned, stored
*T    // raw:   unsafe pointer/address (gated; see S58 collision below)
```

The same signature in the retired words vs the ratified sigils:

```jet
// D-CAP1/3 (retired spelling)
fn write(file: edit File, data: view Bytes)
fn equip(player: edit Player, item: take Item)

// D-CAP7 (ratified)
fn write(file: ~File, data: Bytes)        // view is the inferred default → no sigil
fn equip(player: ~Player, item: ^Item)
```

The call site mirrors the type — `damage(~player, 10)`, `close(^file)`, `cache(&texture)`
— and method receivers carry the sigil on `self`: `fn damage(~self)`, `fn destroy(^self)`,
`fn share(&self)`; plain `self` is infer/read.

**Supersedes / amends:**
- **S10** — `mut` → `~`, `take` → `^`, `view` (borrow-return) → `&`, default-read → bare
  `T`. The retired keywords (`mut`, `view`) become S14 teaching errors pointing at the
  sigils.
- **D-CAP1** — the words `view`/`edit`/`take`/`share` are retired *as the spelling*; the
  four capabilities they named are preserved unchanged, now written `T`/`~`/`^`/`&`.
- **D-CAP2** — the call-site `share x` verb becomes `&x`. `copy x` has **no sigil** (the
  five-sigil set is closed); duplication is a value op, not an access capability, so it
  stays a verb/method — a residual to settle, not a sixth sigil.
- **D-CAP3** — type-side placement is **kept** (`name: ~Type`, exactly like `name: Type`);
  only the marker changes from word to sigil.
- **D-CAP4/5/6** — unchanged: the `api: stable | explicit` mechanism still records resolved
  public capabilities; it now records sigils.
- **D-MUTSELF1** — unchanged in substance: `~self` may mutate its receiver in place; the
  `mut self` semantics carry over to the new spelling.

**Downstream gate — RESOLVED 2026-06-23:**
- **`*` / deref (D-CAP9 = D).** Prefix `*` means only raw-pointer-of (`#Unsafe`-only);
  **dereference is now postfix `p.*`** (Jai precedent), retiring prefix `*p`; `*T` replaces
  `Ptr<T>` (deprecated alias). `~x`/`^x`/`&x` are free position-disambiguated prefixes — the
  earlier "S58 `&x` = address-of" claim never shipped (address-of is `mem.address_of(x)`), so
  that S58 prose is amended, not collided with.
- **Unmarked-`T` default (D-CAP8 = C).** `Infer` in bodies (elevates by usage), frozen into
  the public signature at an `api: explicit` boundary. Repoints E0202/E0205 off the
  fixed-read default.
- **Capability overloads (D-CAP10 = A).** Out of scope under S14 — call-site-sigil
  disambiguation on a single definition, not overload resolution.

Rejected: keeping the word vocabulary (D-CAP1/2/3 as-was); a words-in-libraries /
sigils-in-apps split; a sixth sigil for `copy`.

**Migration note — `mut`/`take`/`view` → sigils (RETIRED 2026-06-24).** The old S10
keywords are no longer valid syntax: they are removed from `JET_KEYWORD_LIST` and lex
only to fire S14 teaching errors — **E0056** (`mut` → `~`), **E0057** (`take` → `^`),
**E0058** (`view` return → `&`) — which recover by parsing as if the sigil were written,
and `jet fmt` rewrites them to the sigil. The migration table:

| Old | New | Position |
|-----|-----|----------|
| `fn f(mut x: T)` | `fn f(x: ~T)` | parameter |
| `fn f(take x: T)` | `fn f(x: ^T)` | parameter |
| `fn f() -> view T` | `fn f() -> &T` | return borrow |
| `mut self` | `~self` | receiver |
| `take self` | `^self` | receiver |
| `f(take x)` (call site) | `f(^x)` | call site |
| `fn f(x: T)` (default read) | `fn f(x: T)` | unchanged |

### Safety tiers — scoped capabilities, units, single-use (ratified 2026-06-21)

Three value/effect-safety features, each **ratified as the target** but **gated** on an
upstream decision still in the ballot — implementation is sequenced after the gate, no
`src/` change until then.

**D-SCAP1 — Scoped capabilities** *(ratified 2026-06-21, option A; gated on D-EFF1;
**implemented 2026-06-24**)*: a **capability is a first-class value** granted into a
lexical scope — `#Grant(Fs) { caps -> … }` — and **revoked at scope end** by the RAII
rule (S63). The capability authorizes its effect (`Fs`/`Net`/…) inside the scope; letting
it escape (stored, returned, shared, captured) is a compile error (**E0711**), and using
an effect with no capability in scope is **E0712**. This is **authority to perform an
effect**, distinct from the c06 value-ownership capabilities
(`view`/`edit`/`take`/`share`); it generalizes the S58 `#Audit`/`#Unsafe` gate from
"unsafe ops" to "any guarded power." Rejected: effect-tag-only capabilities with no value
(option B — can't lend a power per-call).

**Implementation (2026-06-24).** `#Grant(<effects>) { <handle> -> … }` is the dual of
`#Caps`: where `#Caps` *restricts* a region to a set, `#Grant` *authorizes* one and binds
a first-class capability handle for the block (`KW_GRANT`, lowercase per the ratified
spelling; `Stmt::Grant` in the AST). Built on the audited D-EFF1 region machinery
(`RegionAccum`/`RegionSummary` carry a `grant` flag): the block is bounded to the granted
set transitively — an effect reached inside that the grant omits has no backing
capability and is **E0712** (the dual of E0741). The handle is a sema-only, unnameable
type (`Capability`); it is erased in codegen (I3 — the grant lowers to a plain block, no
runtime grant/revoke value, no `unsafe`). Escape of the handle past the block (return,
store, alias, closure capture) is **E0711**. Snapshots `tests/ui/grant_out_of_set`
(E0712) + `tests/ui/grant_handle_escapes` (E0711); example `effect_grant.jet`.

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
**Status: implemented (2026-06-24).** `#SingleUse` is a `#`-marker before a
`struct`/`enum` (same idiom as `#PublishedSchema`), registered in `Source/Syntax.rs`
(`ATTR_SINGLE_USE`). Sema sets a per-binding consume duty on owned `#SingleUse`
locals and proves it discharged exactly once: dropped-at-scope-end is **E0140**,
consumed-on-one-`if`-branch-only is **E0141**, lending it (`&`/read instead of `^`)
is **E0142**; use-after-move is the existing **E0121**. Moving to a `^` parameter or
returning the value consumes it (the terminal `^` recipient satisfies linearity — a
move-param does not re-inherit the duty). The tag erases in codegen (I3 — no runtime
value, no `unsafe`). Snapshots: `tests/ui/single_use_{dropped,branch,aliased}`;
example `examples/features/109_single_use.jet`. **Open follow-on (deferred, needs a
ballot):** the spec lists an explicit `drop(x)` escape hatch "requiring an `#Audit`",
but `#Audit` was retired by D-UNSAFE2 (the reason is now an argument of `#Unsafe`).
The blessed `drop`-with-audit spelling is therefore unspecified and **not built** —
the three legal consumes (move / return) ship; the deliberate-drop hatch awaits a
decision (proposed name **D-LIN1-DROP**).

### Uninitialized memory & `core.regex` (ratified 2026-06-21)

**D-UNINIT1 — Visible uninitialization** *(ratified 2026-06-21, option C; owner chose
the attribute form over the rec)*: skipping the default zero-fill of a binding is opted
into with the **`#Uninit` attribute** on the binding — `#Uninit buffer: [U8#4096]` —
reusing the `#` marker sigil (D-ATTR1) like `#Unsafe`/`#Audit`. Gated behind
**`use core.mem`** (S58 low-level tier); outside that gate it is a teaching error
pointing at the gate. Safety is a **compile-time** write-before-read proof: sema tracks
each `#Uninit` binding's initialized state by dataflow across all paths, and a read on
any path that may precede a full write is **E0420** (snapshot required when implemented,
I4). Codegen lowers to `MaybeUninit::<T>::uninit()` after the proof passes — never a
runtime trap (the rail Zig's `= undefined` and C's silence lack). **Status:** the sema
write-before-read proof (E0420, with the gate E0424 and POD-only E0423) is implemented
and green; **codegen is gated on a discovered prerequisite** — `[T#N]` fixed-size lists
currently lower to `Vec<T>`, on which `MaybeUninit` is unsafe and the safe lowering
zero-fills (defeating the feature), so fixed-size lists must first lower as real stack
arrays (proposed owner decision **D-FIXARR1**, board card c82). The parser stays
unwired until then. Rejected: `:= ---` Jai sigil (option A — opaque, greps
badly); `:= uninit` value-keyword
(option B, the rec — owner preferred the `#`-marker idiom).

**D-REGEX1 — `core.regex` ships on the `regex` crate** *(ratified 2026-06-21, option B;
owner-approved I6 bootstrap dep)*: `core.regex` ships now backed by Rust's **`regex`**
crate (DFA/NFA hybrid, **linear-time, no ReDoS**), surface `use core.regex as re` /
`re.match(pattern, text)?`. This is an explicit, **owner-approved I6 exception** — the
one external Core-library dep sanctioned for the regex bootstrap — carrying a standing
obligation to **native-ize (replace with an in-house RE2-style engine) before the end of
Epoch 3**, so the end state stays dependency-free (I6). The compiler (`Source/`) takes no
crate; the dep lives only in the `core.regex` Core sub-library. Rejected: native engine
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

**D-STATE1 — Typestate via transitioning tags** *(ratified 2026-06-22, option A)*: a value
moves through named **states**, each an ordinary `tag` (D-QUAL2). The ratified mechanism: *a
fn takes the old state tag and returns the next; a wrong-state call is the compile error
**E0150**; tags erase, zero runtime cost.*
**(impl 2026-06-24)** Built end-to-end. Two fn-modifier markers (parallel to `#Sanitizer fn`
/ `#Layout(c)`): **`#State(S) fn m(self, …)`** is a require-state guard — `m` is valid only
when its receiver is in state `S`; **`#Transition(From -> To) fn m(self) -> T`** is a
transition — it consumes a value in `From` and yields one in `To`. The from-state may be `_`
(an **entry** transition: a constructor producing the initial state from nothing). The
current state of a value is an **intraprocedural forward-dataflow fact** (`Source/Sema/State.rs`,
same shape as `Taint.rs`): seeded by an entry-transition constructor call, advanced by each
transition call, threaded through `:=` rebindings; a require/transition call on a value in the
wrong state is **E0150** (naming both states + the transition that reaches the required one).
Markers are **erased in codegen** (I3 — generated Rust is identical to the untagged version;
golden-verified). `tests/ui/typestate_wrong_state`, `examples/features/113_typestate.jet`,
`tests/typestate.rs`. **Temporal ordering:** legal state order is expressed by the transition
graph itself — a value can only reach a late state by passing through every `#Transition` edge
on the path. No separate ordering surface exists or is needed; the graph enforces it (Card #19,
resolved 2026-06-27). **Deferred forks (queued for owner confirmation, implemented as the
defaults):** the exact marker spellings — **D-STATE-REQ** (`#State(S)` vs `#Requires(S)`),
**D-STATE-TRANS** (`#Transition(A -> B)` arrow glyph), **D-STATE-DECL** (whether an explicit
`states { … }` grouping is wanted for exhaustiveness) — and the upstream **D-QUAL4** (plain
value-tag *type-position* spelling `#Tag Type` vs `Type #Tag`), which the typestate core does
not depend on (states ride the value + the markers, never a signature type position). Rejected
mechanisms were not on the ballot — A was the sole ratified option.

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
**(impl 2026-06-24)** Built end-to-end: the value-fact tag is spelled **`#Tainted`**
(PascalCase per D-CASING1 — the card's lowercase `#tainted` is normalized to the tag
convention) as a prefix on a value expression (`#Tainted expr`, `Expr::Tainted` in the
AST, `KW_TAINTED` in Syntax.rs). Taint is an **intraprocedural forward dataflow**
(`Source/Sema/Taint.rs`): it spreads through bindings, reassignment, arithmetic, field/
index reads, string interpolation, and collection/struct construction. The
taint-strip contract is **`#Sanitizer fn`** (a fn/method modifier in the `#Pure`/
`#Unsafe` family; `KW_SANITIZER`, `Func.is_sanitizer`) — its result is untainted by
contract. **Sinks are effect-based** (the card's `#db`/`#exec`/`#net`): a Core call
carrying the **`Db`/`Exec`/`Net`** effect with a tainted argument is **E0721**, with a
"pass it through a `#Sanitizer fn`" fix-it. The tag is static, **erased in codegen**
(I3 — `#Tainted x` lowers to `x` unchanged). `tests/ui/taint_sink_unsanitized`,
`examples/features/90_taint.jet`, `tests/taint.rs`. **One spelling fork deferred:** the
ratified card writes the modifier bare `sanitizer fn`, but the D-CASING1 follow-on (which
moved `pure fn` → `#Pure fn`) makes `#Sanitizer fn` the consistent marker spelling —
implemented as the default, queued as **D-TAINT-SAN** for owner confirmation.

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
   `core_imports`, …), error copy, and ui-snapshot names. **[superseded 2026-06-26 by
   D-CORENS1]** the first-party ring packages no longer keep a separate `jet.*` namespace —
   they move under `core.*` too, so every first-party library (built-in module or ring
   package) is spelled `core.<name>`; there is no `jet.*` or `std.*` library namespace.

This amends the marker casing in S82/D-ATTR1 (markers were mixed-case) and the value-fact/
effect spellings throughout c62/c66–c73. The high-impact gate renames (`#unsafe` →
`#Unsafe`, `#audit` → `#Audit`) are **confirmed** (owner, 2026-06-21: PascalCase reinforces
that these are weighty, unique declarations). Rejected: keeping lowercase tags / the name "std".

**D-CASING1 follow-on (owner-directed 2026-06-21): `test` / `todo` / `pure` become PascalCase
`#`-markers.** These three "unique declarations" join the tag family rather than staying bare
keywords, so they draw the same attention as every other tag:
- **`#Test`** replaces the `test "name" { … }` block keyword (S43/S82): `#Test("name") { … }`.
  The `jet test` harness recognizes `#Test`.
- **`#Todo`** replaces the bare `todo` typed-hole expression (D-TOOL2).
- **`#Pure`** replaces the `pure fn` modifier (S60): `#Pure fn name() { … }`.
The lowercase spellings (`test`/`todo`/`pure`) are retired to teaching errors pointing at the
`#`-marker forms (S14 pattern). Amends S43, S60, D-TOOL2, S82 on spelling only — semantics
unchanged.

**[confirmed 2026-06-26 by D-MARKERCASE1]** rule 1 (every `#`-marker is PascalCase) is
reaffirmed for `#Grant` and `#Layout`; the lowercase `#Grant` (D-SCAP1 impl note) and
`#Layout` (D-SOA1/2) spellings are pre-existing drift to fix, not an exception.

### Owner batch (ratified 2026-06-22)

This batch drains the open ballot (cards c06, c09, c51, c64–c66, c73, c77–c78, c82,
c90–c91, c94, c97–c98, c102–c105). Each entry is also a row in the decision log.

**D-UNSAFE2 — `#Audit` text merges into `#Unsafe`** *(ratified 2026-06-22, option B)*:
the safety justification becomes the argument to the gate: `#Unsafe("reason") { … }` (and
`#Unsafe("reason") fn`). The separate `#Audit("…")` marker line is **retired** — the
unsafe description IS the review artifact, not a second LOC. Owner: "the recommendation
is naive." Amends D-LL1/E2-M13's two-marker form; `#Audit` spellings move to a teaching
error pointing at `#Unsafe("…")`. UNBLOCKED. (c09)

**D-FIXARR1 — `[T#N]` lowers to a real fixed stack array** *(ratified 2026-06-22, option
B)*: the ratified S76 `[T#N]` type becomes a real fixed-size stack array in codegen (no
`Vec` lowering). Copies when `T` is copyable, moves otherwise; a `[T#N]` widens to `[T]`
by copying into a growable list when passed to a `[T]` slot; `var x := [1,2,3]` keeps
S76's beginner rule (widens to `[Int]`). Unlocks `#Uninit` (D-UNINIT1) soundness with no
new syntax. UNBLOCKED. (c82)

**D-CAP2 — `copy` / `share` are prefix keywords** *(ratified 2026-06-22, option A;
**amended 2026-06-23 by D-CAP7** — `share x`→`&x`; `copy` stays a verb, has no sigil)*:
duplicate-vs-share after a `take` is spelled with a leading verb at the call site —
`party.add(copy player)` / `party.add(share player)` — never inferred (the plan kills
implicit clone, L0201). UNBLOCKED. (c06)

**D-CAP3 — capability sits on the type side** *(ratified 2026-06-22, option A; **kept by
D-CAP7** — placement unchanged, only word→sigil)*: parameter
capability attaches to the type, not the binding: `fn write(file: edit File, data: view
Bytes)`. Consistent with `name: Type` everywhere; no type written ⇒ capability inferred
too (one location for both). UNBLOCKED. (c06)

**D-EFF2 — effect polymorphism (hybrid)** *(ratified 2026-06-22, option D)*: default is
transparent flow-through — a higher-order fn's effect set is its own body plus, at each
call, the effects of statically-known function arguments (zero syntax; Marcus's
`#Pure`-violation errors at his line). Escaping/boxed function values default to the
maximal effect set (sound). Two optional expert levers: `#Pure fn(…)` / `#(net) fn(…)`
**param types** to demand/bound a callback, and `#(via f)` on the **signature** to publish
a tight pass-through that holds when the value escapes. Effect rows are static + erased
(I3). (c66) **IMPLEMENTED** (2026-06-24): the flow-through default already shipped; both
expert levers now build end-to-end. Lever 1 rides the front of a function *type* —
`#Pure fn(T) -> U` / `#(E) fn(T) -> U` parsed onto `Type::Fn.effect_bound` (ignored in
structural type equality — it's a call-site obligation, not a subtype), checked at each
call site against the actual callback's inferred effects (E0747). Lever 2 `#(via f)` parses
into `Func.effect_via` and seeds the function's published effect set with callback param
`f`'s declared bound (maximal if `f` is unbounded) before the fixpoint; a `via` naming a
non-parameter or non-callback is E0748. Example: `examples/features/effect_levers.jet`.

**D-EFF3 — effects on trait methods (dispatch contract)** *(ratified 2026-06-22, option
C)*: a trait method may declare an effect upper bound (`#Pure fn hash(self)`, `fn
render(self) #(gpu)`). The bound is BOTH the impl obligation (an impl's inferred effects
must be ⊆ the bound, else E0710) AND the dispatch contract (a trait-object call's effect IS
the declared bound; un-annotated methods inferred per-impl under static dispatch, E0711
with a fix-it when called through an object under an effect ceiling). Safe-by-default holds
through dynamic dispatch. (c66)

> **D-EFF2 + D-EFF3 complete the effect-system surface.** With D-EFF1=B, D-QUAL1=1, D-EFF2=D,
> D-EFF3=C ratified, the effect-system surface is now **fully decided**. Implementation of
> the whole cluster (D-EFF1 and everything gated on it — D-SCAP1/D-TAINT1/D-DET1/D-TXN1/
> D-TXN2/D-PROP1) was waiting only on these two sub-questions; the gate is cleared. Build now.

**D-MUTSELF1 — self-mutation in `mut self` methods** *(ratified 2026-06-23, option A)*: a
`mut self` method may mutate its receiver in place — `self.field = v` (and the compound
`self.field += v`, S17) lowers to `(*self).field = v` on the `&mut Self` receiver. Whole-`self`
reassignment `self = New{…}` is likewise sanctioned (lowers to `*self = …`, fixing a prior
AST-path I2 hole where the `mut self` slot wasn't dereferenced on the LHS). **No new syntax** —
S17 already admits a `mut` parameter as an assignment LHS, and `mut self` is one. `self.field =
v` in a non-`mut` method (a shared-read `self`) or a call on a non-`mut` binding is **E0205**,
pointed at the assignment with a "write the receiver as `mut self`" fix (owner Q1: at the
assignment, not the signature). The copy-into-a-local write-back form (option B) is **not** a
parallel sanctioned spelling (owner Q2). Memory-safe by construction — the `mut` borrow
discipline + rustc verify are unchanged (I1). Rejected: C (functional update via struct-spread,
needs unratified spread syntax), D (a new `with` keyword for what A does with none), and
keep-banned. Unblocks deleting the legacy AST codegen path (c109 Phase N). Implementation
touches: parser lvalue grammar (a new `LValue::Field`), the E0003 lvalue site, the sema
mut-receiver check (new E0205 + a tests/ui snapshot, I4), codegen `Stmt::Assign` + the self-slot
`deref` flag, and TIR coverage (drop the `stmt_assigns_self` exclusion).

**D-MIGRATE2A — `add f: T =  val`** *(ratified 2026-06-22, option A)*: a migration adds a
field with a default using the `=` already used for struct-field defaults: `add verified:
Bool = false`. UNBLOCKED. (c73)

**D-MIGRATE2D — `remove f`** *(ratified 2026-06-22, option A)*: a migration deletes a field
with the plain verb `remove legacy_id` (not `drop` — `drop` connotes destructive
db-level deletion). UNBLOCKED. (c73)

**D-MIGRATE2E — `change f: Old -> New via { expr }`, verb spelled `change`** *(ratified
2026-06-22, option B structure, owner-renamed)*: a field type-change is spelled `change`
(not `transform`); it carries an inline converter in curly braces and supports both
multi-line and single-line forms: `change price: Int -> Usd via { (cents) => Usd(cents) }`.
Owner: "instead of the word transform use **change**. Keep the structure/syntax the same as
b." Reuses the ratified `->` arrow; omitting the converter falls back to an `impl Old ->
New` in scope (D-MIGRATE2B). UNBLOCKED. (c73)

**D-MIGRATE2F — no `reorder` verb** *(ratified 2026-06-22, option B)*: reordering a
struct's fields is not a tracked breaking change and needs no migration verb; field order
belongs to a serializer's own versioning, not the `#PublishedSchema` baseline. UNBLOCKED.
(c73)

**D-MIGRATE2B — converter source: inline wins, else named `impl`** *(ratified 2026-06-22,
option C)*: a `change` converter resolves in order — (1) the inline `via { … }`, (2) an
`impl Old -> New` in scope, (3) E0910 asking for one. One resolution rule; reuses the
ratified D-ERR-CONV `impl Source -> Target` surface (invoked by the migration machinery at
data-load time, not by `?`). UNBLOCKED. (c73)

**D-MIGRATE2C — `jet schema` surface** *(ratified 2026-06-22, option A)*: squash with
`jet schema squash --before <ver>` (the flag names the cutoff, removing through/at/before
ambiguity); `jet schema status` confirmed; **no** separate `jet schema check` verb — `jet
build`'s E0910 is already the CI gate (a second verb would re-implement detection, I3).
UNBLOCKED. (c73)

**D-JSONOUT1 — built-in `#[Serialize]` marker drives JSON** *(ratified 2026-06-22, option
A)*: a built-in `#[Serialize]` marker (distinct from S56 user-derives) makes the compiler
generate `json.render`/typed decode by walking fields; one marker covers in and out; field
rename via `#json("name")`. **Must coordinate with D-SERDE1** — the same serialize model
backs JSON, so JSON is one format of the unified serde data model, not a parallel path.
Owner: "joined at the hip with the serde planning." Gated-on D-SERDE1 (shared model). (c90)

**D-ARGS1 — builder-spec CLI parsing** *(ratified 2026-06-22, option A)*: a builder spec
(`args.spec().flag(…).option(…).positional(…)`) parsed against `io.args()` gives typed
values, an auto-generated `--help`, and teaching errors today with no S56 dependency; the
spec value can later back a `#[Args]` struct form once derives land. Generated `--help` and
error text are product copy → snapshot-tested. UNBLOCKED. (c91)

**D-MATHLIB1 — `core.linalg` ring package** *(ratified 2026-06-22, option A)*: numerics
(vectors, matrices, dot/cross/matmul, decompositions/FFT later) ship as a first-party ring
package like regex/csv/toml — keeps Core small (I8); can offer comptime-sized matrices
(rides D-FIXARR1/S76). Native-vs-bootstrap-crate (I6) is an impl gate decided like regex.
UNBLOCKED. (c94)

**D-SIMD1 — safe portable lane types** *(ratified 2026-06-22, option A)*: first-class
portable lane types (`F32x4`/`F64x2`) with safe ops lowering to portable SIMD with scalar
fallback — memory-safe by default (I1). Raw target intrinsics stay available behind
`#Unsafe`. UNBLOCKED. (c94)

**D-REACT1 — reactivity is tooling + a library, not core semantics** *(ratified 2026-06-22,
option B; implemented 2026-06-25)*: ordinary binding semantics are unchanged; the compiler
may expose the derived dataflow graph to tooling/IDEs, and runtime reactivity ships as an
opt-in `core.reactive` library. Shipped surface (`use core.reactive as reactive`):
`reactive.signal(initial) -> Signal<T>` (a mutable reactive source),
`reactive.derived(() => expr) -> Derived<T>` (a value recomputed from the signals it reads),
and `reactive.effect(() => { … })` (a side effect re-run when a signal it read changes).
Methods: `Signal.get()`/`Signal.set(v)`, `Derived.get()`. No new keyword or sigil — reactive
values are ordinary values produced by library calls, exactly as option B requires; dependency
tracking is explicit-by-read (a `.get()` inside a derived/effect body subscribes it). The
runtime is pure std (Rc/RefCell + a thread-local observer stack — no external crate, no
raw-memory tier). Diagnostics E2910–E2913 guard misuse. The *tooling-side* dataflow-graph
exposure (LSP/docs/build-invalidation) remains a future tooling task — the library is the
ratified runtime deliverable and is now in. DONE. (c64)

**D-FANOUT2 — defer namespace/member fan-out** *(ratified 2026-06-22, option B)*: only the
ratified S75 call fan-out `f.[a,b,c]` ships; a second fan-out axis (`service.{start,stop}`
/ `obj.[x,y]`) waits for real-use evidence before another dot/bracket meaning is added.
UNBLOCKED. (c65)

**D-STRPARSE1 — runtime parse APIs + comptime `Result`/`Option`** *(ratified 2026-06-22,
option A)*: add runtime string-parse APIs (`parse_int`, `.lines()`, …) AND allow comptime
evaluation through `Result`/`Option` for pure parse paths, so comptime schema/config
ingestion works. UNBLOCKED. (c97)

**D-CTCORE1 — curated pure comptime-Core whitelist** *(ratified 2026-06-22, option B)*:
comptime executes only a curated whitelist of deterministic, pure Core functions (math,
string helpers); other Core calls (`fs.read`, `env.get`, …) produce a teaching diagnostic.
No inline execution of arbitrary Core at comptime — keeps builds reproducible and comptime
pure; build-time I/O stays the explicit, audited tier (rides D-CTIO1's `embed_*`). The
whitelist grows with tests. UNBLOCKED. (c98)

**D-JIT1 — stay-interpreter-for-v1, JIT behind a seam** *(ratified 2026-06-22, option D)*:
`jet serve` ships hot-reload on the proven comptime interpreter behind a stable
`JitBackend` seam; a Cranelift JIT lands later as tier-1 (interpreter stays permanent
tier-0). rustc-in-the-interactive-loop (B) rejected as an I2 hazard. A runtime-side
Cranelift dep (D+) needs separate owner dep-approval (I6 runtime exception); without it,
plain D holds. UNBLOCKED. (c77)

**D-HOTSWAP1 — module-boundary swap + type-stable state preservation** *(ratified
2026-06-22, option B)*: the hot-reload unit is a module; a type-stable edit swaps code and
keeps the module's live state; a type/layout-changing edit does NOT reinterpret old data —
it does a clean, announced, connection-drained restart. The type-surface check is a sema
job (I3). UNBLOCKED. (c77)

**D-DEVMODE1 — one `jet dev` verb (auto-detect) + dev↔release identity is a HARD RULE**
*(ratified 2026-06-22, option A for home; Q2 ratified as a hard rule)*: `jet dev <entry>`
is the single dev command — it detects run-to-completion programs (rerun on save) vs
resident programs (hot-swap on save); experts override with `--restart`/`--swap`/`--watch=off`
flags, not a second verb. **Q2 hard rule (owner: "Programs must absolutely behave identically
as release build & interpreter/JIT"):** a program's output under the dev runtime
(interpreter/JIT) MUST be byte-identical to the release (rustc) build; a `tests/` mode runs
every golden example through both paths and diffs — **any divergence is a release blocker**,
not a warning. UNBLOCKED. (c77)

**D-SOA2A — the `soa` layout keyword is renamed `columnar`** *(ratified 2026-06-22, option
C)*: the layout keyword inside `#layout(…)` is **`columnar`** (Arrow/Parquet column-store
term, self-defining, no concurrency-vocabulary collision). This **renames the ratified
D-SOA1 `#Layout(soa)` to `#Layout(columnar)`** (see the amended D-SOA1 entry).
**IMPLEMENTED (c78).** (c78)

**D-SOA2B — whole-struct columnar only in v1** *(ratified 2026-06-22, option A)*:
`#Layout(columnar)` converts every field; partial annotation (`#Layout(columnar: x, y)`)
is deferred — two memory regions need new ownership/aliasing surface. **IMPLEMENTED (c78):
the partial form is rejected at parse time (E1109).** (c78)

**D-SOA2C — reserve the per-container prefix-keyword spelling** *(ratified 2026-06-22,
option A)*: reserve `columnar [Particle]` (prefix keyword on a list type) for a future
per-use layout override; a layout is a storage modifier, not a type parameter, so the
generic-style `Columnar<T>` form is not reserved. **IMPLEMENTED (c78): `columnar [T]` in
type position is parse-and-reserved (E1107); nothing ships behind it.** (c78)

**D-SOA2D — `#Layout(columnar)` is serialization-transparent** *(ratified 2026-06-22, option
A)*: serialization sees the logical struct; output is identical with or without the layout
attribute. `#Layout` is a memory concern only; columnar serialization (e.g. Arrow IPC) is a
purpose-built serializer, not the default `#[Serialize]`. **IMPLEMENTED (c78): the columnar
storage type's `Encode`/`Decode`/`JetShow` delegate to the gathered AoS form, so
`json.to_string` output is byte-identical with or without the attribute.** (c78)

**D-SOA1 implementation note (c78).** Whole-struct columnar storage ships end-to-end:
parser→sema→codegen. A `[S]` of a `#Layout(columnar)` struct lowers to a generated
`user_<S>_columns` struct-of-arrays (one `Vec` per field) with a logical-`Vec` inherent API.
**v1 list surface on a columnar `[S]`:** construct (list literal incl. empty), index-read
(`xs[i]` gathers an `S`), field-read (`xs[i].f` reads the field's column directly — the
cache-friendly path), `len`, `is_empty`, `push`, and iteration (`loop p in xs`). **Deferred
(E1108, rejected — never miscompiled):** the functional/mutation surface (`map`, `filter`,
`sort`, `pop`, `remove`, `insert`, `get`, `first`, `last`, …), slicing `xs[a..b]`,
index-write `xs[i] = …`, and field-write `xs[i].f = …`. Codegen emits zero `unsafe` (I1).

**D-TEST1 — a parameterized `#Test fn` is a property test** *(ratified 2026-06-22, option
B; implemented c51)*: an `#Test fn` with parameters is a property test (inputs generated from
the parameter types, automatic invisible shrinking); one with no parameters is a unit test.
Zero new syntax — matches the S82 worked example. **Implementation note (c51):** the spelling
is `#Test fn name(p: T, …) { … }`; `jet test` generates ~200 cases per run from a deterministic
seed (`JET_PROP_SEED` overrides), and on the first failing case greedily shrinks each argument
to a minimal counterexample reported as `property failed for p = <value>, …`. Generatable
parameter types: `Int`, `Float`, `Bool`, `String`, `Char`, sized integers, `F32`, and `[T]`/`T?`
of those; any other type is **E0613** at compile time (no silent miscompile, I3). The generator
is std-only (I6); the body type-checks with the params in scope exactly like a function body.

**D-TEST4 — doctest: fenced ```jet block + `// =>` trailing comment** *(ratified 2026-06-22,
option A; implemented c51)*: code examples inside `///` doc comments (S49) run as tests;
expected output is a `// =>` comment on the producing line; a mismatch fires E2901. Reuses the
`//` comment marker (S5); no new tokens. **Implementation note (c51):** `jet test` discovers
every ```` ```jet ```` fenced block inside a `///` doc-comment run, compiles each as a
self-contained program (setup lines verbatim; each `EXPR // => VALUE` line is run and its
`JetShow` rendering compared to `VALUE`), and reports `doctest at <file>:<line>: pass/FAIL`. A
mismatch fires **E2901** pointing at the producing line. A file with only doctests (no `#Test`
blocks) is still testable.

**D-COV1 — `jet test --coverage` (tooling, no syntax)** *(deferred-no-ballot; implemented
c51)*: coverage is tooling only — no user-facing syntax. **Implementation note (c51):**
`jet test --coverage` builds an instrumented test harness (probes are emitted only in this
mode, so normal codegen stays byte-identical — golden tests untouched) that records which user
functions ran, then prints a per-function `HIT`/`MISS` table with `file:line` plus an overall
`covered/total functions (pct)` summary to stdout. Granularity is per-function (each function's
source line); finer per-line/branch instrumentation (which needs statement-level spans carried
through the typed IR) is a future refinement, not a syntax decision. Std-only (I6).

**D-BIND2 — immutable binding spelled `@=`** *(ratified 2026-06-22, option A)*: the immutable
binding is `name @= expr`. `:=` stays the mutable binding, `=` stays reassignment of a `:=`
binding (S17). This is a fundamental token change: **`@=` supersedes the `::` immutable
binding** spent by D-BIND1/S2 (2026-06-18). Owner picked A (`@=`), not the card's rec C
(`$=`). Requires a repo-wide migration of `::`-bindings to `@=` (implementation). UNBLOCKED.
(c102) **[reopened 2026-06-26 by D-BINDEXPLICIT1]** the `=`-is-reassignment-only invariant is
reopened: in the explicit-typed binding form (`name@ Type =  val` / `name: Type =  val`) `=` also
binds when a mutability marker precedes the name. The inferred `@=`/`:=` forms here are unchanged.

**D-NUMOPS1 — checked-by-default integer overflow + expert numeric surface** *(ratified
2026-06-22, option A)*: plain integer arithmetic **traps on overflow** by default
(safe-by-default; a silent corruption becomes a caught bug); experts opt a specific op into
`wrapping(…)` / `saturating(…)` / `checked(…) -> T?`, visible at the use site. Ship the
standard numeric value/op surface: per-type `MIN`/`MAX`, float `INFINITY`/`NAN`/`EPSILON` +
predicates (`is_nan`), bit ops (`<<`, `&`, `count_ones`), and explicit width conversions
(`.to_u8()?` narrowing, `.to_i64()` widening — no implicit narrowing). **Gated on D-SG9's
sized integers being implemented first** (the `Type` enum is still Int/Float); implementing
D-SG9 (esp. `U8`) also unblocks `embed_bytes` (c75). (c103)

**D-SERDE1 — one format-agnostic Serialize/Deserialize data model** *(ratified 2026-06-22,
option A)*: a type derives `Serialize`/`Deserialize` once against an abstract data model;
each format (JSON/CSV/TOML/binary) is an adapter implementing a `Serializer`/`Deserializer`
protocol, so one derive drives every present and future format. Adds `Deserialize` as the
symmetric counterpart to S55's `Serialize`; field attributes `#[rename/default/skip/flatten/
rename_all]`. CSV (D-CSVROW1) and JSON (D-JSONOUT1) are arms of THIS model, sharing one
decoder path. Data model lives in Core; each format adapter is a ring library. The
`Serialize`/`Deserialize` *derive* is a **built-in, compiler-owned codegen field-walk**
(like `derive Comparable` → `PartialOrd`) — NOT the S56 user-defined-derive system and NOT
comptime reflection, so it is **buildable now, not gated on S56**. S56 later lets users
author *their own* derives against this same model. (c104)

**D-ENC1 — `core.encoding`: one library, every format an arm** *(ratified 2026-06-24 by
owner)*: the D-SERDE1 model ships as a single core library `core.encoding` with per-format
submodules (`core.encoding.{json,csv,toml,yaml}`, extensible). Two import surfaces, both
supported: the whole library — `use core.encoding` then `encoding.json.to_string(x)` /
`encoding.csv.decode<Row>(rec)` (nested-namespace access) — and the terse leaf —
`use core.encoding.json as json` then `json.to_string(x)` (existing flat machinery). **Clean
break, migrate all**: `core.json` and the `core.csv`/`jet.toml`/`core.yaml` ring modules are
retired and everything moves under `core.encoding.*` with no deprecated alias (examples
`30_json`/`51_csv`/`52_toml`/`53_yaml` migrate). The encode verb is the D-JSONVERB1 pair
`to_string`/`to_string_pretty`, applied uniformly across every format; typed decode is the
generic `decode<T>`; full field attributes `#[rename/default/skip/flatten/rename_all]` ship.
Merges c89 (typed CSV) + c90 (typed JSON out) into c104. (c104)

**D-ITER1 — full lazy iterator-adapter set** *(ratified 2026-06-22, option A)*: ship the
everyday lazy adapter family (enumerate, zip, chunks, windows, take/skip(_while), flat_map,
scan, group_by, dedup, step_by, peekable, partition, find/position, fold/reduce, min/max_by,
…) as methods on the ratified iterator protocol (D-EXT1 Tier 1) — lazy, allocation-free
until a terminal op, no new grammar. Conservative familiar spellings. UNBLOCKED. (c105)

**D-DBG2 amendment — owner affirmed C (expert raw-frame access)** *(ratified 2026-06-22)*:
the owner's final ballot picked **C (surface raw Rust frames)**. This does NOT introduce an
unconditional I2 violation: C is satisfied by the existing expert opt-in `jet debug
--raw-frames` (the default view stays clean — I2 intact). See the amended D-DBG2 decision-log
row.

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

**D-NET1 (M10) — TLS via `rustls`, delivered as the `core.tls` package** (an
instance of D-DEP1). `core.http` depends on `core.tls`; `core.tls` wraps `rustls`
via `extern rust`. No crate enters the compiler. HTTPS works with zero config in
user code (`use core.http`).

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
| 2026-06-24 | D-EFF4 | effect vocabulary (B — closed now, reserve extensibility): ship the closed set of ten built-in effects — `Net`, `Fs`, `Io`, `Db`, `Time`, `Rand`, `Env`, `Exec`, `Log`, `Gpu` (as already carried in `Source/Sema/Effects.rs`) — and **reserve** a future `effect <Name>` user-declaration form (no syntax minted today). An unknown effect name in `#(…)` is an error (E0119) for now. Reviewer can read the whole vocabulary; safe-by-default airtight; the extensibility door is reserved for when a domain effect is needed. Unblocks the effect-system cluster (c62 + D-TAINT1/D-DET1/D-TXN1-2/D-SCAP1/D-LIN1). c62 | owner |
| 2026-06-24 | D-EFF5 | effect lattice shape (A — flat, all independent): the ten effects are a **flat** set with no subsumption — `#(Io)` means the console effect only; a `Net` call under `#(Io)` is an error (E0740). A signature's effect list means exactly what it says (matches Koka / mainstream effect libs and the current `Effects.rs`). Owner-Q (rename the console effect `Io`→`Console`) left as an optional future polish — `Io` kept for now. Unblocks c62 effect cluster. c62 | owner |
| 2026-06-24 | D-JITDEP1 | JIT tier-1 dependency (approve Cranelift, with a planned native progression): approve **Cranelift** as a **runtime-side** dependency (never in compiler `Source/` — I6 holds; scoped + owner-signed like D-REGEX1/regex) to implement a `CraneliftBackend` over the shipped `JitBackend` seam (c77) as JIT tier-1, for fast `jet serve`/hot-swap. Production stays AOT (compile-to-Rust); the JIT is the resident dev-loop tier only. **Progression captured as frozen cards:** own bytecode VM (zero-dep, replaces Cranelift) → own native JIT (master-of-all). c77 | owner |
| 2026-06-24 | D-SERDE2 | serde hand-impl surface (A — Swift-plain): the expert hand-impl path uses method verbs `encode`/`decode`, the format-agnostic value tree is **`DataTree`** (variants `.Null/.Bool/.Int/.Float/.Text/.Bytes/.Array/.Object`), and the decode error is **`DecodeError`** (`{ path, reason }`); encode is infallible so no `EncodeError` is minted (I8). No "Serde" package name leaks (owner constraint), mirroring Swift `Codable`. NOT an increment-2 blocker on its own — the built-in derive (D-ENC1) generates these impls and the model layer ships now; the user only writes `impl T.Encode { fn encode(self) -> DataTree { … } }` when hand-rolling. The `DataTree` tree is distinct from the already-shipped dynamic `Json` enum (`json.parse`/lenient `json.decode` still return `Json`). Plan: sidequests/serde-model.md. c104 | owner |
| 2026-06-24 | D-SERDE3 | `RenameAll` casing menu (C — typed, full real-wire): `#[RenameAll(camel)]` accepts a closed set of typed keywords `camel`/`snake`/`pascal`/`kebab`/`screaming` (short spellings — `rename_all` already supplies "case" context). Typed-over-stringly (LSP completes the five; bad style → E2409 prints the closed list). Covers every wire the research found (camel+snake dominate; pascal=.NET, kebab=CLI, screaming=env) without serde's dead styles. The marker name is PascalCase `RenameAll` (D-CASING1); the keyword argument keeps its own case. This is the WIRE casing, not Jet's own identifier casing (that stays `#PascalCase`). Plan: sidequests/serde-model.md. c104 | owner |
| 2026-06-24 | D-SERDE4 | derive-marker shape (B, owner-modified): the collapsed both-directions umbrella marker is **`#[Codable]`**; the one-way markers are **`#[Encode]`** (write-only) and **`#[Decode]`** (read-only) — owner: "the collapsed version to be Codable, with Encode & Decode as the one way types". `Codable` is pure sugar expanding to `#[Encode, Decode]`. This resolves the D-SERDE1 "Serialize/Deserialize" model *naming* to the **Encode/Decode/Codable** surface (coherent with D-SERDE2=A's `encode`/`decode` verbs); the Rust trait codegen names are `user_Encode`/`user_Decode`. The legacy stub `derive Serialize`/`to_json` (S55/D-JSONOUT1, no shipped code, no codegen) is superseded by this real model — no parallel path. Markers are bracket form (D-ATTR2), PascalCase (D-CASING1). Plan: sidequests/serde-model.md. c104 | owner |
| 2026-06-24 | D-SERDE5 | per-field attribute surface (A — bracket markers): field-level wire control uses the D-ATTR2 `#[…]` grammar on the field — `#[Rename("customer")] who: String`, `#[Skip] cache: Blob`, `#[Default] retries: Int` (with `#[Default(8080)]` for an explicit literal expression — never serde's stringly `default= "fn"` path), `#[Flatten] meta: Meta` (struct-flatten; the `Map<String,V>` catch-all is reserved, pairs with D-SERDE8). Marker names PascalCase, positional literal args (D-CASING1). Owner-Q defaults adopted as recommended: support `#[Default(expr)]`; an absent `T?` field is **omitted** from output (no per-field incantation — kills serde's `skip_serializing_if` boilerplate); struct-flatten ships now, map catch-all reserved; asymmetric directional `#[Rename(in:…, out:…)]` deferred (reserved). Plan: sidequests/serde-model.md. c104 | owner |
| 2026-06-24 | D-SERDE6 | typed decode + verb coherence (C — both turbofish + expected-type): `decode<T>(text)` (explicit call-site type argument) AND expected-type elaboration (`cfg@ Config=  json.decode(text)` infers `T = Config`). Bare `decode(s)` with **no** target stays the D-JSON3 lenient-dynamic `Json` — the presence of `<T>`/an expected type switches typed vs dynamic on one verb (no fourth verb). This introduces **call-site type arguments (`<T>` turbofish) as general Jet grammar** (owner-Q recommended: bless generally, not encoding-only) — Kotlin/C#/Rust all have it; available for any generic call going forward. Plan: sidequests/serde-model.md. c104 | owner |
| 2026-06-24 | D-SERDE7 | enum wire representation (A + ship chooser now): externally tagged is the default — `{"Click":{"x":5}}`, lossless, works for every payload shape; a single-value variant emits the bare value `{"Pageview":"/"}` (no `"_0"` wrapper). Owner: ship the chooser now (not post-v1) — `#[Tag("type")]` selects internally-tagged (`{"type":"Click",…}`) and `#[Untagged]` selects untagged; these are **container** attrs and do NOT collide with D-SERDE5's **field** attrs (different concern). PascalCase `Tag`/`Untagged` (D-CASING1; `Tag` also dodges the reserved `tag` keyword). Plan: sidequests/serde-model.md. c104 | owner |
| 2026-06-24 | D-SERDE8 | unknown-field policy (A — lenient default + opt-in strict): decoding into a typed struct ignores unknown wire keys by default (forward-compatible — a producer adding a field doesn't break consumers); `#[DenyUnknownFields]` on the struct makes an unknown key an error (E2412). Matches every mainstream system (serde/Go/C#/pydantic). The one place "safe by default" cuts toward lenient (strict-by-default breaks innocent producers, not attackers). Plan: sidequests/serde-model.md. c104 | owner |
| 2026-06-24 | D-SIMD2 | SIMD lane construction & access surface (A — method reduce): adopt the community-converged surface — constructor `F32x4(1.0,…)`, splat `F32x4.splat(x)`, lane index `v[i]`, element-wise operators `+`/`-`/`*`/`/`, reduce `v.sum()` with general `v.reduce(#Add)`/`v.reduce(#Max)`; `[F32#4]` round-trips via `from_array`/`to_array` (D-FIXARR1 bridge). Operator overloading is blessed on the built-in lane types ONLY (closed compiler-provided family — no user-defined `+`), which also unblocks D-LINALG1 Option D. Defer named lanes (`v.x/.y/.z/.w`, Option C) to a graphics follow-on. Plan: sidequests/math-linalg-simd.md. c94 | owner | **(impl 2026-06-25)** `F32x4`/`F64x2` are built-in Core value types (`Source/Syntax.rs`, `core_type_known`). Constructor/`splat`/`from_array`/`to_array`/lane index/`sum`/`product`/`min`/`max`/`reduce(#Op)` + element-wise `+`/`-`/`*`/`/` wired sema (`Source/Sema/CheckerCoreLib.rs` math tables, `CheckerInfer.rs` ctor/static/method/index/operator gates) → codegen (`Source/Codegen/TIR.rs` `MathBuiltin`/`MathMethod`/`MathLaneIndex`; prelude structs+free fns in `Source/Prelude/CoreLib.rs`). I6: pinned rustc is stable 1.95 (no `std::simd`), so lanes lower to a **scalar-array fallback** (`[f32;4]`/`[f64;2]` newtypes with element-wise ops) — correct + memory-safe, no intrinsics, no `un`+`safe`; a `std::simd` backend can swap in behind the same surface. `reduce` op marker = new `Expr::ReduceMarker`; diagnostics E2510 (bad/misplaced marker) / E2511 (operator mismatch). Example `examples/features/116_linalg_simd.jet`; ui `simd_reduce_bad_marker`/`math_operator_mismatch`. |
| 2026-06-24 | D-NOSTD1 | embedded/freestanding std opt-out (A — platform-implied): no `no_std`/`std:` boolean — the std baseline follows the typed platform `target:` in `pack.jet` (a bare-metal platform value ⇒ no-std; a hosted `linux.x64` ⇒ std), reusing the D-TGT1/3 typed-platform model. This sets the *direction* now so plans stop writing the borrowed `no_std: true`. The user-facing surface lands with the embedded-platform milestone (D-OS1, post-E3); the freestanding demo ships meanwhile as an internal build mode. Embedded stays a committed goal. Plan: jetpack-jetos / embedded milestone. | owner |
| 2026-06-24 | D-IF3 | explicit `if subject == { … }` value/pattern dispatch (A — `==` marker, required): `==` between the subject and `{` enters multi-way dispatch; arms are bare values/patterns with no repeated `subject ==` (`"index" -> …`, `Active(id) | Reconnecting(id) -> …`, `else -> …`). `==` names the operation each arm performs, killing the yes/no-vs-dispatch body-scan ambiguity at the block level. The marker is **required**: a bare `if subject { head -> … }` becomes teaching error E0992 (auto-fixed by inserting `==`, and by `jet fmt`). Arms keep `->` (D-IF2), braceless single-line surviving `jet fmt` only with D-FMT1. Predicate/guard arm heads are disallowed inside `== {` (E0993) — use range arms (`400..499 ->`) or conventional `if/else if`. Owner-initiated revision of D-IF1/D-IF2. c134 | owner |
| 2026-06-24 | D-FMT1 | `jet fmt` preserves single-line bodies (A — author-intent): a brace body the author wrote on one line stays one line (if it fits 100 cols, holds exactly one simple non-block statement, and has no inner comment); a body broken across lines stays multiline — the same author-intent model already blessed for S69 dot-chains. Idempotent, not canonical; philosophy.md #4 exempts structural arrangement, so S44's canonical-layout property was never load-bearing. Applies uniformly to all brace bodies (`if`/`else`/`while`/`for`/`loop`/`fn`/dispatch arms); an `if/else` chain expands wholly if any branch is multiline; braces stay required (S3). This is the gate that lets D-IF3's single-line arms survive `jet fmt`. Owner-initiated revision of S44. c135 | owner |
| 2026-06-24 | C-CASING | clarification (no vote): tag spellings in plans reconcile to D-CASING1 PascalCase — `units-tag.md` `#unit(usd)` → `#Unit(usd)`, `transact-rollback-semantics.md` `#transact` → `#Transact`, `c71-typestate-impl.md` `#no_copy` → `#NoCopy`. Nothing user-facing changes. | owner |
| 2026-06-24 | C-MANIFEST | clarification (no vote) — **CORRECTED 2026-06-24**: the ratified manifest filename is **`pkg.jet`** (D-JPK-FILES, 2026-06-18; `Source/Syntax.rs` `PAYLOAD_FILE = "pkg.jet"`), renamed from `payload.jet`; `pack.jet` was the retired U10 *interim* name. So stale **`pack.jet`** references in plans reconcile to **`pkg.jet`** (the original row had this inverted — `package-ecosystem-trust.md`, `flagship-vertical-slices.md`, and any remaining `pack.jet` in plan docs → `pkg.jet`). | owner |
| 2026-06-24 | D-BENCH1 | benchmark surface = `#Bench "name" { … }` block (A): a first-class region-benchmark block, the exact sibling of `#Test("name") { … }`, discovered + run by the **existing** `jet bench` verb (D-TOOL5) — which today times a whole program; D-BENCH1 adds per-region timing (ops/sec + ns/iter). Owner Q (runner verb) resolved to the existing `jet bench` — NO new `jet test --bench` form. Keyword `#Bench` (KW_BENCH) registered in Syntax.rs; the PascalCase marker joins the `#Test`/`#Pure`/`#Todo`/`#Caps` family (D-CASING1). `benchmark` manifest target (TARGET_BENCHMARK, c80) points `jet bench` at a package entry — same engine, manifest-level pointer only. No new diagnostic. c121 (blocks + runner), c80 (manifest target) | owner |
| 2026-06-24 | D-PKGSIGN1 | package authenticity = checksum floor + opt-in signing (B + A opt-in): SHA-256 content-hash verification (B) is the ALWAYS-ON integrity floor — mandatory `verify_entry` on every install (E1204); Ed25519 author signing (A) is an OPT-IN, NON-BLOCKING authenticity layer on top. `require_signed` on `RegistryConfig` stays a per-registry/per-dependency policy, **OFF by default** — never a hard gate refusing unsigned packages. Consumers auto-verify silently and speak only on a MISMATCH; signing touches only the registry-publish path (path/git/unpublished deps are never signed). `jet publish` auto-generates+stores a keypair on first publish (no separate keygen on the magic path) and nudges `jet key backup`; experts opt into explicit keygen + out-of-band fingerprint pinning. Ed25519/SHA-512 primitives are added natively to the ring layer (I6) when the opt-in path is built (only SHA-256 exists today). Sigstore/keyless (C) rejected (needs network + a transparency-log service, at odds with offline-first/std-only). Plan: sidequests/package-ecosystem-trust.md Step 4. c122 | owner | **(impl 2026-06-25)** Tier B (always-on SHA-256 floor) ships: `Store::verify_entry` re-hashes the store tree on every install, called before `link_into_project` at both fetch sites (`Source/Fetch.rs:273`/`365`), propagating **E1204** on mismatch — no opt-in, no key ceremony. **Gated on c96 (registry):** Tier A (Ed25519 author signing — `Source/Publish/Sign.rs`, native Ed25519/SHA-512 ring primitives, `LockedPackage::signature`, `jet keygen`/`jet key backup`, publish auto-keygen, `require_signed` enforcement) is NOT built — every Tier-A surface needs the registry-publish handshake that c96's open ballot owns. The `require_signed` field already exists on `RegistryConfig` (OFF by default) and stays inert until c96. |
| 2026-06-24 | D-DBG3 | debugger interactive command surface (A): the `jet debug` in-session prompt is `(jet)`; step words are lldb-familiar `step`/`next`/`continue`/`finish` with single-letter aliases `s`/`n`/`c`/`f` (shipped v1); breakpoint/locals layout uses a `<- here` caret on the current line + a one-line `locals:` dump; `help` lists the verbs. I2-safe — only Jet frames/lines/safe-locals shown by default (D-DBG2); no-Jet-line frames stepped over. Implements c52's in-session surface (D-DBG1 launch verb + D-OBS1/2 already ratified). Plan: sidequests/dap-debugger.md. c52 | owner | **(impl 2026-06-25)** `jet debug <file>` (D-DBG1=A) ships as a **source-level step debugger over the existing tree-walking interpreter** (`Source/Comptime/Interpreter.rs`) — the same engine behind `jet dev`/`jet repl`, NOT lldb. The interpreter calls a `DebugHook` (`Source/Comptime/Interpreter.rs`) before every statement, threading call `depth`/`cur_func`; the driver (`Source/Debug.rs`) runs the `(jet)` prompt with `step`/`next`/`continue`/`finish` (+ `s`/`n`/`c`/`f`), `break N`/`print X`/`locals`/`backtrace`/`list`/`help`/`quit`. Each stop prints `breakpoint hit file:line in fn()`, a source window with the `<- here` caret, and the one-line `locals:` dump; every value is rendered through `CtValue::jet_show()` (I2 — never generated Rust). Because it drives the dev interpreter, it declines the same FFI/tasks/`#Unsafe`/native-std features with **E2203** (debug-specific boundary, `Source/Interpreter.rs::debug_boundary_scan`), pointing at the real build; `quit` mid-run surfaces **E2204**. CLI verb in `Source/CLISpec.rs`/`Source/CLI.rs`; keyword + verb constants `CMD_DEBUG`/`DBG_*` in `Source/Syntax.rs` (D-DBG1/D-DBG3, I7); std-only, no DAP crate (I6). Example `examples/features/118_debug.jet`; tests `tests/debug.rs` (step/next/continue+breakpoint/finish/backtrace/print/locals/help/E2203/E2204). **Deferred fork (NOT a re-litigation):** the **DAP/lldb native backend** in the plan (step-through of the *full* native feature set, `--raw-frames` D-DBG2, editor DAP wiring) is D-DBG3 *step 2* — its surface (this command vocabulary) is already ratified and unchanged; only the native backend remains, gated on no new owner decision. |
| 2026-06-24 | D-LINALG1 | `core.linalg` type & method names (A, over a generic substrate): user-facing names `Vec2`/`Vec3`/`Vec4`, `Mat3`/`Mat4`, methods `.dot()`/`.cross()`/`.matmul()`. Owner: these are ALIASES over a generic `Vec<N>`/`Matrix<M,N>` substrate — the const-generic `<N>` spelling (value args in `<…>`) is blessed here and coexists with `[T#N]` (S76). D-MATHLIB1 package home unchanged. Operator overloading on linalg types rides the D-SIMD2 crux Owner Q. Plan: sidequests/math-linalg-simd.md. c94 | owner | **(impl 2026-06-25)** the user-facing names `Vec2`/`Vec3`/`Vec4`/`Mat3`/`Mat4` ship as built-in Core value types directly (the const-generic `Vec<N>`/`Matrix<M,N>` *substrate spelling* is NOT yet parsed — these concrete aliases ARE the v1 surface; const-generic value-in-`<…>` is deferred, not load-bearing for the ratified names). Methods `.dot()`/`.cross()` (Vec3)/`.matmul()`/`.transpose()`/`.transform()`/`.length()`/`.normalize()`, constructors, `from_array`/`to_array`; operators element-wise `+`/`-`, `*` (Hadamard, or matrix×matrix), and `Mat*Vec`. Same sema/codegen path as D-SIMD2 (shared math tables in `Source/Sema/CheckerCoreLib.rs`; column-major `[f64;N*N]` matrix structs in `Source/Prelude/CoreLib.rs`). Plain std math, no `un`+`safe`. Example `examples/features/116_linalg_simd.jet`. |
| 2026-06-24 | D-SUPPLY1 | supply-chain command surface (A): dedicated top-level verbs `jet vendor` (with `--vendor-dir <path>`) and `jet audit` (advisory scan; nonzero exit on CRITICAL), mirroring `jet test`/`jet debug`; SBOM emitted via a `--sbom` flag on `jet build` (`jet publish` always emits to the registry index). Supersedes the never-defined "D-PKGS1" milestone label. Backed by E1204 (store tamper) today; E1217/E1218 minted by the plan. Plan: sidequests/package-ecosystem-trust.md Steps 5-7. c122 | owner | **(impl 2026-06-25)** all three command surfaces ship, std-only (I6): **`jet vendor [--vendor-dir <path>]`** copies resolved deps into a vendor tree (default `vendor/`, relocatable) + writes `vendor/manifest.json` recording each dep's name/version/fingerprint (`Source/Publish/Vendor.rs`, `Source/CmdSupply.rs::run_vendor`); **`jet audit`** scans the lockfile against an advisory DB with a per-advisory `Severity` (low|medium|high|critical) and exits nonzero **only on a CRITICAL match** (advisory otherwise), reusing **E2603** now severity-prefixed (`Source/Publish/Advisory.rs`); **`jet build --sbom`** writes an SPDX 2.3 SBOM next to the binary (`Source/CmdCompile.rs::write_sbom_for_build`). New diagnostics: **E1217** (manifest dep with no locked revision — bidirectional completeness check `Lock::verify_all_manifest_deps_locked`, fires in `--locked`/publish) and **E1218** (breaking public-API change under a non-major publish bump — local gate diffing against the frozen API snapshot, `Source/Publish/Diff.rs::e1218` wired in `run_publish`). Flags `--vendor-dir`/`--sbom` registered in `Source/CLI.rs` (I7). The registry-publish path (`jet publish` upload + always-emit-SBOM-to-index) stays gated on c96. |
| 2026-06-24 | D-TXN3 | deferred post-commit effects (A): an irreversible effect that must run only after a `#Transact` commits is registered via the library form `scope.on_commit(() => {…})` — same Drop-backed model as the ratified `scope.guard` (D-DEFER1), NO new keyword (I7 untouched); the lambda runs LIFO on a clean commit, dropped on rollback. The D-TXN2 fix-it string is updated to name `scope.on_commit`. Naming/parameterizing the transaction scope is the open follow-on D-TXN4. Plan: sidequests/transact-rollback-semantics.md. c72. **(impl 2026-06-24)** built on the `scope.guard` machinery: `name.on_commit(() => {…})` lowers to a `JetTransaction` whose Drop runs the boxed hooks LIFO **only if `commit()` ran**; a `?`-failure skips commit so the hooks drop un-run. Erased effect/txn state (I3); no `unsafe` in generated code. | owner |
| 2026-06-24 | D-NUMOPS2 | sized/unsigned integer overflow default (A): every integer width (`U8`/`I16`/`U32`/…) inherits the D-NUMOPS1 trap-on-overflow default — no width-dependent silent wrap; `wrapping(…)`/`saturating(…)`/`checked(…)` are the explicit opt-ins. One overflow rule for all widths (I8; no-silent-bugs). The dsg9 plan documents "`U8` traps; `wrapping(…)` gives the C behavior". Plan: sidequests/dsg9-sized-integers-impl.md. c132 | owner | **(impl 2026-06-24)** every width's `+`/`-`/`*`/`/` lowers through the `JetArith` trap helpers (`jet_add`/…, `checked_*` + Jet panic, I2); `wrapping`/`saturating`/`checked` builtins opt out at the use site; bit operators `&`/`\|`/`^`/`<<`/`>>` (shift count any integer, traps past width via `jet_shl`/`jet_shr`), per-type `MIN`/`MAX`, float `INFINITY`/`NAN`/`EPSILON` + `is_nan`/`is_infinite`/`is_finite`, bit queries `count_ones`/…, and named width conversions (`.to_u8()?` narrowing / `.to_i64()` widening — no implicit mixing). Example `82_sized_integers.jet`; no `unsafe` in generated code. |
| 2026-06-24 | D-QUAL3 | unit-tagged number type annotation (C): a `#UnitFamily(currency) { usd, eur, gbp }` declaration mints one DISTINCT type per member (`usd`→`Usd`, erasing to `Float`), so signatures read plain English (`fn subtotal(price: Usd, qty: Int) -> Usd`) and the `#Unit` sigil stays out of everyday code — the "upgrade to D-DIST2" D-UNIT1 framed. The family tag is PascalCase **`#UnitFamily`** (owner; D-CASING1 — supersedes any lowercase `#unit_family` spelling). Coercion already pinned by D-UNIT1 (E0129; `.raw()` strips). Unblocks c68. The plain non-parameterized value-tag type-position spelling is the deferred D-QUAL4. Plan: sidequests/units-tag.md. c68 | owner | **(impl 2026-06-25)** pure sugar over the D-DIST1/D-DIST3 machinery: `#UnitFamily(name) { m1, m2 }` parses to an `UnitFamilyDef` (`Source/Syntax.rs` `ATTR_UNIT_FAMILY`; `Source/Parser/Items.rs` `unit_family_def`), which lowers in sema-registration and codegen to one `#Numeric` distinct `DistinctDef` per member, PascalCased (`usd`→`Usd`) and erasing to `Float` — so each member rides the existing distinct construct/`.raw()`/arithmetic/`#[repr(transparent)]`-newtype path with no sema/codegen changes. Construction `Usd(9.99)`, same-unit arithmetic, and `.raw()` work via D-DIST3. Cross-unit mixing (`Usd + Eur`) reuses the distinct same-type rule **E0127** (the spec's "E0129" predates the diagnostics.md split where E0129 =  distinct-over-distinct; reusing the distinct machinery keeps one rule). Example `examples/features/112_unit_family.jet`; snapshot `tests/ui/unit_family_mix` (E0127). Formatter emits the family verbatim (no expansion); no `unsafe` in generated code. |
| 2026-06-24 | D-ENC1 | `core.encoding` unified library (owner): the D-SERDE1 model ships as ONE core library `core.encoding` with per-format submodules (`core.encoding.{json,csv,toml,yaml}`, extensible). Two import surfaces: whole-library `use core.encoding` → `encoding.json.to_string(x)`/`encoding.csv.decode<Row>(rec)` (new nested-namespace access) AND terse leaf `use core.encoding.json as json` → `json.to_string(x)` (existing flat path). Clean break — `core.json` + `jet.{csv,toml,yaml}` retired, all moved under `core.encoding.*`, no alias (examples 30_json/51_csv/52_toml/53_yaml migrate). Encode verb = D-JSONVERB1 `to_string`/`to_string_pretty` uniformly across formats; typed decode = generic `decode<T>`; full field attrs `#[rename/default/skip/flatten/rename_all]`. The `Serialize`/`Deserialize` derive is built-in compiler codegen (like `derive Comparable`), NOT S56/comptime — buildable now. Merges c89 + c90 into c104. Plan: sidequests/serde-model.md. c104 | owner |
| 2026-06-24 | D-JSONVERB1 | value→JSON-string verb (A): `json.to_string(v)` (compact) + `json.to_string_pretty(v)` (2-space indent) — serde_json's lauded pair; names the return type on its face and matches Jet's ratified `to_float`/`to_int` named-conversion idiom (S42), keeping one consistent `to_`-prefixed conversion+serialize story. Supersedes/renames the prior `json.render`/`json.render_pretty` (D-JSONOUT1) — `render` retired (no shipped user code). Bare `json.string` (drop `to_`) and `json.stringify` rejected — the bare form collides with S42's `to_…` idiom and reads ambiguously vs a string-node accessor. Folded into D-ENC1 / sidequests/serde-model.md. c90 | owner |
| 2026-06-24 | D-TXN4 | named transaction handle (A): `#Transact(order) { … }` binds a transaction handle named `order`; the post-commit hook (and future `order.rollback()`/savepoints) is called on that name — `order.on_commit(() => {…})` — mirroring the ratified `region r { … r.alloc(…) }` pattern (D-REGION1). Refines D-TXN3 = A's spelling: `scope.on_commit` → `<name>.on_commit`; the Drop-backed post-commit semantics are unchanged and the general `scope.guard` cleanup (D-DEFER1) is untouched. The handle name is a user-chosen binding (any identifier; lowercase recommended so it doesn't read like a type). A bare `#Transact { }` with no hooks stays legal; the generic implicit `scope.on_commit` (B) is rejected in favor of the named handle. Plan: sidequests/transact-rollback-semantics.md. c72 | owner |
| 2026-06-23 | D-CAP7 | capability is a prefix sigil, not a keyword (owner mandate): `T` infer / `~T` edit / `^T` take / `&T` share / `*T` raw, on the type (`name: ~Type`) and mirrored at the call site (`~player`, `^file`, `&texture`) and on receivers (`~self`). Supersedes the word spelling of S10 (`mut`/`take`/`view`) and D-CAP1/2/3; the four capabilities are unchanged. `copy` stays a verb (no sigil). DOWNSTREAM RESOLVED 2026-06-23: D-CAP8=C, D-CAP9=D, D-CAP10=A (rows below). Surface frozen in docs/prompt-memory-model-final.md. c124 | owner |
| 2026-06-23 | D-CAP8 | unmarked `T` = infer-in-bodies, freeze-at-API (C): inside executables and private/package code an unmarked param starts as `Infer` and elevates to Read/Write/Move/Share from body usage via a deterministic fixed point (raw never inferred); at a `library { api: explicit }` boundary the resolved capability is frozen into interface metadata (D-CAP4/5/6) and a later read→`~`/`^`/`&` drift is a breaking-change error, not a silent flip. Repoints today's fixed-read default (`parse_access_prefix` Source/Parser/Expressions.rs:1631) and the E0202/E0205 triggers. Owner Qs settled with recs: call-site sigil still required for inferred params (moves/edits stay visible); overgrant warns (never auto-downgrade); inferred `&`-share freezes like `~`/`^`. c125 | owner |
| 2026-06-23 | D-CAP9 | `*x`=raw-of, dereference becomes postfix `p.*`, `*T` replaces `Ptr<T>` (D, "use full recommendation"): prefix `*` has exactly one meaning — raw-pointer-of, legal only in `#Unsafe`; dereference moves from prefix `*p` to **postfix `p.*`** (Jai precedent; composes with `.field`, honors the clean-`.` field-access rule); E0208 reworded to teach `p.*`. `*T` is the canonical raw pointer type and `Ptr<T>` becomes a deprecated alias that teaches `*T`. `~x`/`^x`/`&x` are free position-disambiguated prefixes (`~` was unlexed; `&x`/`^x` were parse errors). **Amends S58 prose** (was `&x`=address-of, never shipped — address-of is `mem.address_of(x)`) and scrubs the retired `#Audit` refs in S58/c131. `.read()`/`.write()` remain for explicit/volatile ops. c127 | owner |
| 2026-06-23 | D-CAP10 | capability overloads out of scope (A): Jet keeps one definition per name (S14, E0105). A single `fn` declares/infers one capability; a call-site sigil (`process(~data)`) requests stronger access against that one signature and type-checks or errors — no overload resolution, no perf-driven selection (the doc's "perf flag" is dropped). Prior art backs this: Odin forbids implicit overloading (explicit `proc{…}` groups only); neither Odin nor Jai treats capability/mutability as an overload axis. Future escape hatch if multiple capability bodies are ever wanted: Odin-style explicit groups, owner-only. c128 | owner |
| 2026-06-22 | D-UNSAFE2 | merge audit text into unsafe (B): the safety reason becomes the argument to the gate — `#Unsafe("reason") { … }` / `#Unsafe("reason") fn` — and the separate `#Audit("…")` marker is retired (the unsafe description IS the review artifact). Amends D-LL1/E2-M13; `#Audit` → teaching error pointing at `#Unsafe("…")`. UNBLOCKED. c09 | owner |
| 2026-06-22 | D-FIXARR1 | `[T#N]` lowers to a real fixed stack array (B): the ratified S76 type becomes a real fixed-size stack array in codegen (no `Vec`); copies when `T` copyable, moves otherwise; widens to `[T]` by copy into a growable list when passed to a `[T]` slot; `var x := [1,2,3]` keeps S76 (widens to `[Int]`). Unlocks `#Uninit` (D-UNINIT1) soundness, no new syntax. UNBLOCKED. c82 | owner |
| 2026-06-22 | D-CAP2 | `copy`/`share` are prefix keywords (A): duplicate-vs-share after a `take` is a leading call-site verb (`add(copy player)` / `add(share player)`), never inferred (kills implicit clone L0201). UNBLOCKED. c06 | owner |
| 2026-06-22 | D-CAP3 | capability on the type side (A): `fn write(file: edit File, data: view Bytes)` — capability rides the type, consistent with `name: Type`; no type written ⇒ capability inferred too. UNBLOCKED. c06 | owner |
| 2026-06-22 | D-EFF2 | effect polymorphism, hybrid (D): default transparent flow-through (own body + statically-known fn-arg effects, zero syntax); escaping/boxed fn values default maximal (sound); expert levers `#Pure fn(…)`/`#(net) fn(…)` param types (demand/bound a callback) and `#(via f)` on the signature (publish a tight pass-through that holds when escaping). Static + erased (I3). With D-EFF3 this completes the effect-system surface and clears the D-EFF1 impl gate. c66 | owner |
| 2026-06-22 | D-EFF3 | effects on trait methods, dispatch contract (C): a trait method may declare an effect upper bound (`#Pure fn hash(self)`, `fn render(self) #(gpu)`) that is BOTH the impl obligation (inferred ⊆ bound, else E0710) AND the dispatch contract (a trait-object call's effect = the declared bound; un-annotated inferred per-impl statically, E0711 + fix-it when called through an object under an effect ceiling). Safe-by-default holds through dynamic dispatch. Completes the effect-system surface; clears the D-EFF1 impl gate. c66 | owner |
| 2026-06-22 | D-MIGRATE2A | migration add-field (A): `add f: T =  val` — reuses `=` from struct-field defaults. UNBLOCKED. c73 | owner |
| 2026-06-22 | D-MIGRATE2D | migration remove-field (A): plain verb `remove f` (not `drop`). UNBLOCKED. c73 | owner |
| 2026-06-22 | D-MIGRATE2E | migration change-type (B structure, verb renamed `change`): `change f: Old -> New via { expr }` — owner renamed `transform`→`change`; multi-line or single-line `via { … }`; reuses the `->` arrow; omitting the converter falls back to an `impl Old -> New` in scope (D-MIGRATE2B). UNBLOCKED. c73 | owner |
| 2026-06-22 | D-MIGRATE2F | migration reorder (B): no `reorder` verb — field order is not a tracked breaking change; belongs to a serializer's own versioning, not the `#PublishedSchema` baseline. UNBLOCKED. c73 | owner |
| 2026-06-22 | D-MIGRATE2B | converter source (C): resolve a `change` converter as (1) inline `via { … }`, (2) `impl Old -> New` in scope, (3) E0910 asking for one. Reuses D-ERR-CONV's `impl Source -> Target` (invoked by migration machinery at data-load time, not by `?`). UNBLOCKED. c73 | owner |
| 2026-06-22 | D-MIGRATE2C | `jet schema` surface (A): squash via `jet schema squash --before <ver>` (flag names the cutoff); `jet schema status` confirmed; NO separate `jet schema check` — `jet build`'s E0910 is already the CI gate (a 2nd verb would re-implement detection, I3). UNBLOCKED. c73 | owner |
| 2026-06-22 | D-JSONOUT1 | built-in `#[Serialize]` marker drives JSON (A): built-in marker (distinct from S56 user-derives) generates `json.render`/typed decode by field-walk; one marker covers in+out; rename via `#json("name")`. **Coordinate with D-SERDE1** — JSON is one format of the unified serde model, not a parallel path (owner: "joined at the hip with the serde planning"). Gated-on D-SERDE1 (shared model). c90 | owner |
| 2026-06-22 | D-ARGS1 | builder-spec CLI parsing (A): `args.spec().flag(…).option(…).positional(…)` parsed against `io.args()` → typed values + auto `--help` + teaching errors, no S56 dep; later backs a `#[Args]` struct form when derives land. `--help`/error text = product copy → snapshot-tested. UNBLOCKED. c91 | owner |
| 2026-06-22 | D-MATHLIB1 | `core.linalg` ring package (A): numerics ship as a first-party ring package (like regex/csv/toml), keeping Core small (I8); comptime-sized matrices ride D-FIXARR1/S76. Native-vs-bootstrap-crate is an I6 impl gate decided like regex. UNBLOCKED. c94 | owner |
| 2026-06-22 | D-SIMD1 | safe portable lane types (A): first-class `F32x4`/`F64x2` with safe ops lowering to portable SIMD + scalar fallback (memory-safe by default, I1); raw target intrinsics stay behind `#Unsafe`. UNBLOCKED. c94 | owner | **(impl 2026-06-25, with D-SIMD2)** lane types ship via the scalar-fallback half of this decision — the pinned stable rustc lacks `std::simd`, so the safe scalar-array path is the shipped backend (portable-SIMD backend is a drop-in behind the same surface). Raw `#Unsafe` target intrinsics remain a future expert-tier add-on. |
| 2026-06-22 | D-REACT1 | reactivity = tooling + library, not core semantics (B): ordinary binding semantics unchanged; compiler may expose the dataflow graph to tooling; runtime reactivity ships as opt-in `core.reactive`. UNBLOCKED. c64 | owner |
| 2026-06-22 | D-FANOUT2 | defer namespace/member fan-out (B): only the ratified S75 call fan-out `f.[a,b,c]` ships; a second axis (`s.{…}` / `obj.[x,y]`) waits for real-use evidence. UNBLOCKED. c65 | owner |
| 2026-06-22 | D-STRPARSE1 | runtime parse APIs + comptime Result/Option (A): add runtime string-parse APIs (`parse_int`, `.lines()`) AND comptime evaluation through `Result`/`Option` for pure parse paths (comptime schema/config ingestion). UNBLOCKED. c97 | owner |
| 2026-06-22 | D-CTCORE1 | curated pure comptime-Core whitelist (B): comptime executes only a curated whitelist of deterministic pure Core fns (math/string); other Core calls (`fs.read`, `env.get`) → teaching diagnostic. No inline arbitrary-Core execution at comptime — builds stay reproducible, comptime stays pure; build-time I/O is the explicit audited tier (D-CTIO1 `embed_*`). Whitelist grows with tests. UNBLOCKED. c98 | owner |
| 2026-06-22 | D-JIT1 | stay-interpreter-for-v1, JIT behind a seam (D): `jet serve` ships hot-reload on the comptime interpreter behind a stable `JitBackend` seam; Cranelift JIT lands later as tier-1 (interpreter = permanent tier-0); rustc-in-the-interactive-loop rejected (I2 hazard). A runtime-side Cranelift dep (D+) needs separate owner dep-approval (I6 runtime exception); else plain D. UNBLOCKED. c77 | owner |
| 2026-06-22 | D-HOTSWAP1 | module-boundary swap + type-stable state preservation (B): reload unit = module; type-stable edit swaps code, keeps live state; a type/layout change does NOT reinterpret old data — clean announced connection-drained restart. Type-surface check is sema (I3). UNBLOCKED. c77 | owner |
| 2026-06-22 | D-DEVMODE1 | one `jet dev` verb auto-detect (A) + dev↔release identity is a HARD RULE: `jet dev <entry>` detects run-to-completion (rerun) vs resident (hot-swap); experts override with `--restart`/`--swap`/`--watch=off` flags, not a 2nd verb. **Q2 hard rule (owner):** dev (interpreter/JIT) output MUST be byte-identical to the release (rustc) build — a `tests/` mode diffs every golden example through both paths; **any divergence is a release blocker**, not a warning. UNBLOCKED. c77 | owner |
| 2026-06-22 | D-SOA2A | `soa` layout keyword renamed `columnar` (C): the `#Layout(…)` keyword is `columnar` (Arrow/Parquet term); **renames ratified D-SOA1 `#Layout(soa)` → `#Layout(columnar)`** (D-SOA1 row amended). Impl deferred post-v1. c78 | owner |
| 2026-06-22 | D-SOA2B | whole-struct columnar only in v1 (A): `#Layout(columnar)` converts every field; partial annotation deferred (two memory regions need new ownership/aliasing surface). Deferred post-v1. c78 | owner |
| 2026-06-22 | D-SOA2C | reserve per-container prefix spelling (A): reserve `columnar [Particle]` (prefix keyword on a list type) for a future per-use override; generic-style `Columnar<T>` not reserved (layout is a storage modifier, not a type param). Grammar reservation only. Deferred post-v1. c78 | owner |
| 2026-06-22 | D-SOA2D | `#Layout(columnar)` serialization-transparent (A): serialization sees the logical struct; output identical with/without the layout attribute; columnar serialization is a purpose-built serializer, not default `#[Serialize]`. Deferred post-v1. c78 | owner |
| 2026-06-22 | D-TEST1 | parameterized `#Test fn` is a property test (B): an `#Test fn` with params = property test (inputs generated from param types, automatic invisible shrinking); no params = unit test. Zero new syntax (matches S82). UNBLOCKED. c51 | owner |
| 2026-06-22 | D-TEST4 | doctest = fenced ```jet block + `// =>` trailing comment (A): code in `///` doc comments (S49) runs as tests; expected output is `// =>` on the producing line; mismatch fires E2901; reuses `//` (S5), no new tokens. UNBLOCKED. c51 | owner |
| 2026-06-22 | D-BIND2 | immutable binding spelled `@=` (A, NOT the card's rec `$=`): `name @= expr` immutable; `:=` stays mutable, `=` stays reassignment (S17). **`@=` supersedes the `::` immutable binding** spent by D-BIND1/S2 (fundamental token change); requires a repo-wide migration of `::`-bindings to `@=`. UNBLOCKED. c102 | owner |
| 2026-06-22 | D-NUMOPS1 | checked-by-default integer overflow + expert numeric surface (A): plain integer arithmetic traps on overflow; opt-ins `wrapping`/`saturating`/`checked(…)->T?` visible at the use site; ship per-type `MIN`/`MAX`, float `INFINITY`/`NAN`/`EPSILON` + predicates, bit ops, explicit width conversions (`.to_u8()?` / `.to_i64()`, no implicit narrowing). **Gated on D-SG9's sized ints being implemented first** (Type enum still Int/Float); implementing D-SG9 (U8) also unblocks `embed_bytes` (c75). c103 | owner |
| 2026-06-22 | D-SERDE1 | one format-agnostic Serialize/Deserialize data model (A): derive `Serialize`/`Deserialize` once against an abstract data model; each format (JSON/CSV/TOML/binary) is a `Serializer`/`Deserializer`-protocol adapter (one derive, every present+future format). Adds `Deserialize` counterpart to S55; field attrs `#[rename/default/skip/flatten/rename_all]`. CSV (D-CSVROW1) + JSON (D-JSONOUT1) are arms of this model, one decoder path. Model in Core; adapters are ring libs. **Gated on user-defined derives (S56, Epoch 3)** — ratify shape now, build when S56 lands. c104 | owner |
| 2026-06-22 | D-ITER1 | full lazy iterator-adapter set (A): ship the everyday lazy family (enumerate/zip/chunks/windows/take_while/skip_while/flat_map/scan/group_by/dedup/step_by/peekable/partition/find/fold/min_by/…) as methods on the ratified iterator protocol (D-EXT1 Tier 1) — lazy, allocation-free until a terminal op, no new grammar; conservative familiar spellings. UNBLOCKED. c105 | owner |
| 2026-06-22 | D-EFF1 | effect system (B): inferred per-fn effect set propagated along calls (Koka-style rows), erased in codegen (I3 — no handler/monad/runtime value); `pure fn` becomes the empty set; assert/restrict at boundaries (`#(net, db)` on the signature) and in `#caps(net) { … }` regions (out-of-set + impure-`pure` diagnostics; the card's illustrative E0701/E0702 collide with existing FFI codes — real codes assigned from the free range at impl). **Reopens S60's "no further effects" stance** (S60's `pure` spelling+meaning preserved as ⊥). Surface spelling pinned to `#(…)` by D-QUAL1=1 (sub-Qs 4+5 resolved). **Implementation gated on new D-EFF2 (effect polymorphism / higher-order propagation) + D-EFF3 (trait-method effects)**; diagnostic quality is an impl concern. Carries D-SCAP1/D-TAINT1/D-DET1/D-TXN1/D-TXN2 | owner |
| 2026-06-22 | D-QUAL1 | qualifier-surface dialect = **Option 1 (Sigil-pure)**: effects ride the signature as `#(net, db)`; the tag/trait list stays the bare `#[Serialize, Comparable]` form (**D-ATTR2 kept, untouched**); roles `role X = #(…)` / `#[…]`; manifest policy `plugins.coupon: deny(fs, db)` in the in-source `module { }` block; declaration-heavy grouping uses the same `#[ effects:…, facts:… ]` labeled bracket (purely additive); value-facts ride the value (`#tainted`, `#paid`). Delivers Core D + Roles + Unified block. Same `#(…)` surface as D-EFF1 — one spelling, no duplicate. **Reopens S60's effect surface**; follow-on must place capability policy across pkg.jet (D-JPK-FILES) vs the in-source block | owner |
| 2026-06-22 | D-TXN1 | `#transact { }` rollback (A): semantic contract — every `?`-failure inside the block calls `rollback(mut self)` (the `Rollback` trait) in reverse order on the values mutated so far; clean exit commits, zero runtime cost beyond the rollback calls; a non-`Rollback` mutation inside the block is a compile error naming the type + fix-it (the card's illustrative E0801 is already assigned — real code from the free range at impl). Honest by construction (only declared-reversible types are covered). Semantic contract ratified now; the effect-region wiring follows **D-EFF1**. Ships with D-TXN2. **(impl 2026-06-24)** the block, commit/clean-exit semantics, and the post-commit hook (D-TXN3/4) are built; on a `?`-failure the registered `on_commit` hooks are correctly dropped un-run (the "nothing committed" half of rollback). The **rollback-registration mechanism** — how a participant opts a mutated value into `rollback(mut self)` (auto-tracked mutations vs. explicit `name.rollback(…)` registration vs. a `Rollback` trait the type derives) — is an owner-facing API the ratified text underspecifies, so it is **deferred to a new ballot D-TXN-ROLLBACK** (card drafted, not guessed in code). | owner |
| 2026-06-22 | D-MIGRATE1 | compile-time enforcement of breaking data-shape changes (A): `#PublishedSchema` types have their field layout snapshotted at release (`.jet/cache/`); a breaking change without a declared migration is **E0910** (compile error, not a lint; the card said E0901 but that code is already assigned — use E0910, first free slot); `migration UserRecord { rename old -> new }` unblocks it. The CHECK is core sema (I3); the up/down conversion fns (`from_vXXX`) are generated by the Build-tier versioning library (#11). Bloat bounded by published-API × support-window (squash-to-baseline + support-floor). **Scope locked to the card's grammar** (`#PublishedSchema` + `migration { rename a -> b }`); other ops (add-with-default, type-change, delete) + `jet schema status`/`squash` verbs → follow-on **D-MIGRATE2**. Unblocked → build now | owner |
| 2026-06-22 | D-SOA1 | cache-friendly data layout (A): **`#Layout(columnar) struct …`** (keyword renamed from `soa` by D-SOA2A=C) — whole-struct structure-of-arrays, field access (`p.x`) unchanged, layout is part of the type (consistent with D-ATTR1). **Syntax ratified; implementation deferred post-v1 (Later tier).** The four D-SOA2 follow-on Qs are now resolved: D-SOA2A= C (keyword = `columnar`), D-SOA2B=A (whole-struct only), D-SOA2C=A (reserve per-container prefix form `columnar [T]`), D-SOA2D=A (serialization-transparent) | owner |
| 2026-06-22 | D-DBG2 | no-Jet-source-line frame policy — **owner ballot affirmed C (expert access to raw Rust frames)**, satisfied by the expert opt-in `jet debug --raw-frames` so the default view stays clean (NO unconditional I2 violation): **default** — the DAP adapter steps over any frame absent from the `.jetmap` and surfaces only Jet frames (I2 intact — no Rust paths in the default view); **expert opt-in `jet debug --raw-frames`** surfaces the raw Rust frame (file+line) for adapter/expert debugging, an explicit, flagged I2 carve-out scoped to the debugger surface. Owner note: once Jet self-hosts there is no underlying Rust and the distinction dissolves. Implements c52's open policy (D-DBG1 verb + D-OBS1/2 already ratified) | owner |
| 2026-06-22 | D-DETACH1 | intentional task detach (A): `task.detach()` — a method on the spawn handle that consumes it and exempts it from L1101 ("task value dropped without `.join()`"); reads as a deliberate choice, quotable in the L1101 fix-it. A detached task that captures a borrowed `view` of the caller's scope is a compile error (it can outlive the borrow) with a "pass an owned `copy`/`share`" fix-it. Keeps one spawn verb | owner |
| 2026-06-22 | D-REPRC1 | C-compatible struct layout (**B**, not rec A): `#Layout(c)` — C repr joins the **one `#Layout(…)` family** alongside `#Layout(soa)` (D-SOA1) and `#Layout(packed)` / `#Layout(align(N))`; codegen stamps `#[repr(C)]` on the generated struct. A growable field (`[U32]`) under `#Layout(c)` is a compile error (use fixed `[U32#N]` or drop the annotation). Owner chose the unified-family fork the rec flagged → reconciles with D-SOA1/D-SOA2 (the SOA rename applies only to the `soa` slot; `c`/`packed`/`align` are sibling layout kinds) | owner |
| 2026-06-22 | D-STDIN1 | streaming stdin (A): `io.stdin()` handle with `.lines()` / `.read_line()`, mirroring the file reader (reuses the `FileLines` streaming type) so one idiom spans files + stdin and a fn written for one accepts the other; constant-memory. `read_all_input` stays as a small-input convenience; a `#Pure fn` reading stdin stays rejected (impure) | owner |
| 2026-06-22 | D-TERM1 | terminal direct-input primitive (surface **A** + name **`live`**): scoped `live { … }` block enters un-buffered/no-echo input for its body and **guarantees** restore on every exit incl. panic (built on D-DEFER1 scope-guard); keys via a `Key` enum. "raw mode" jargon dropped (`live` = rec; owner picked surface A, name taken as the recommendation). The full TUI (old Option D) is confirmed a **separate batteries-included `core.tui` library**, not core — experts get the primitive, beginners get widgets on top. (termios-vs-bootstrap-crate is an I6 impl choice, not user-facing) | owner |
| 2026-06-22 | D-LSDIR1 | directory listing (A **+ C helper**): `fs.list_dir` returns `[DirEntry]` (`{name, path, is_dir}`) — the full path + type in one step, killing a class of separator bugs (return-type change to a shipped fn, called out). Per owner, **also ship `path.join(dir, name)`** (option C, portable join) alongside for experts needing finer control | owner |
| 2026-06-22 | D-CSVROW1 | typed CSV row decoding (A, **folded into D-ENC1 / the serde plan**): `csv.decode<Row>(record)` maps columns to `Row`'s fields by header name with coercion; a bad cell is a typed per-row error composing with the ratified `??` skip. Owner: CSV is **part of the unified serde model (D-SERDE1)** for toml/yaml/json/csv — not standalone; the typed decode rides the built-in `Serialize`/`Deserialize` derive (compiler codegen field-walk, like `derive Comparable`), NOT comptime reflection (no type-level field reflection exists) and NOT the S56 user-derive system — so buildable now. A future `#[CsvRow]` convenience derive (gated on S56) must share this one decoder path. Plan: sidequests/serde-model.md | owner |
| 2026-06-22 | D-LOGFMT1 | `core.log` output format (A): auto-detect by TTY — human-readable text line when stderr is a terminal, JSON lines when piped; `log.setup(format: text|json)` overrides when detection guesses wrong. Same `log.info(…)` calls; format chosen at runtime. The text line layout is product copy → snapshot-tested. Implements c91 | owner |
| 2026-06-22 | D-FLOATW1 | sized-float math/precision policy (A): `core.math` functions are width-generic — `sqrt(F32) -> F32`, `sqrt(F64) -> F64` (full per-width path, F32 is a real precision choice not just storage); precision-losing moves are explicit (`.to_f32()`), mixing `F32`+`Float` is a compile error with a convert fix-it — consistent with D-SG9 (no implicit widening, named conversions). Policy only; **gated on D-SG9's sized floats being implemented first** (F32/F64 spellings ratified but the `Type` enum is still Int/Float) | owner |
| 2026-06-22 | D-STATE1 | typestate via transitioning tags (A): a fn `take`s the old state tag and returns the next; wrong-state call = compile error (E0150); tags erase, zero runtime cost. D-QUAL2 (tag kind) ratified → **unblocked**. Sequence `#SingleUse` (D-LIN1) machinery first | owner |
| 2026-06-22 | D-DET1 | `pure` ⇒ reproducible (A): inside `pure fn` reject wall-clock/OS-rng/fs/net + calls to non-`pure`; supply deterministic `Clock`/`Rng` as injected capabilities; `assume_deterministic { }` expert escape (semantic footgun, v1-legal). Subsumes Clock/Rng fork 2.5. **Gated on D-EFF1** (effect-tracking pass is the enforcement engine). **(impl 2026-06-24)** base purity (E3401 impure-call / E3403 wall-clock+OS-rng) already shipped; the two remaining pieces are now built end-to-end. **Injected caps**: `Clock`/`Rng` are Core handle value types (prelude), constructed deterministically from a caller seed — `time.clock(seed) -> Clock`, `random.rng(seed) -> Rng` (these constructors carry NO ambient effect, so they are pure-callable). A `#Pure fn` reads time/randomness THROUGH a `Clock`/`Rng` param — `clock.now()`/`clock.tick(ms)`, `rng.int(lo, hi)`/`rng.float()` (mutating draws need a `~Rng` receiver) — while ambient `time.now()`/`random.int(…)` stay E3403. RNG is a std-only SplitMix64 (no crate, I6); erased-but-real values in codegen. **`assume_deterministic { }`**: a contextual-keyword block (`KW_ASSUME_DET`, modeled on `live`/`#Caps`); inside it the determinism rejections (E3403, and the E3401 impure-Core/builtin checks) are suspended via a Checker depth flag; erased to a plain Rust block (I3). `examples/features/111_determinism.jet`; `tests/determinism.rs`. **Deferred fork**: the exact `Clock`/`Rng` method API (names/arities — a minimal sensible set shipped) → ballot **D-DET-CAPAPI** (drafted, not guessed). | owner |
| 2026-06-22 | D-TXN2 | reject irreversible effects inside `#Transact { }` (A): a net/fs/subprocess effect that can't be rolled back is a compile error pointing at the call; fix = move after the block, or name the transaction and register the effect on its handle to run post-commit — `#Transact(tx) { … tx.on_commit(() => {…}) }` (D-TXN3 = A semantics, D-TXN4 = A named-handle spelling; **not** an `on_commit { }` keyword). **Gated on D-EFF1** (effect classification); ships with D-TXN1. **(impl 2026-06-24)** built as **E0746** (free code; the card's illustrative E0801 collided): the irreversible set is `Net`/`Fs`/`Exec`; an irreversible Core call directly in the block is rejected at the call site, while the same call inside an `on_commit(…)` lambda (a deferred context) is accepted. `tests/ui/transact_irreversible_effect`. | owner |
| 2026-06-22 | D-EXT1 | library extensibility ceiling (A): Tier 0 vocabulary + Tier 1 blessed protocols **open to all**; Tier 2 marked DSL blocks **stdlib-only** (widen later on evidence); Tier 3 proc macros rejected (conflicts S26 no-macros law); Tier 4 sigils/keywords/grammar **rejected, even for experts**. Standing policy: local footguns allowed, global footguns rejected; mark library syntax; diagnostics are the ceiling | owner |
| 2026-06-22 | D-CTIO1 | comptime build-time I/O (B): ratify `embed_file(path)->String` + `embed_bytes(path)->[U8]`; path must be a string literal, resolved relative to source, no `..`-escape past project root; **no** broad build-time I/O (option C → far-horizon idea card). Implements the S26/S60 blessed exception | owner |
| 2026-06-22 | D-CTX1 | Smart Context grammar (G2): `#context(field: value) { … }` reusing Jet's single `name: value` spelling (S61/S29); `=` stays reassign-only (S17). Q1=A2 (explicit allocator-passing wins when present), Q2=Cβ (per-block) already owner-set; no single-field shorthand, bundle-spread deferred | owner |
| 2026-06-22 | D-ROUTE1 | HTTP route registration & dispatch surface (A) for `core.http`: register routes with path patterns + `:param` extraction parsed for the handler, replacing the manual `request.path` if/match ladder. Implements c83 | owner |
| 2026-06-21 | D-CASING1 | tags PascalCase; traits PascalCase; "Core" not "std" (owner-directed casing/naming) | owner |
| 2026-06-21 | D-OBS2 | debug line-table is a sidecar `<file>.jetmap` JSON (versioned, std-only); part of the DAP debugger | owner |
| 2026-06-21 | D-ALLOC2 | arena `alloc` returns scope-bound `view`; use-after-reset/escape = compile error (E0631/E0632); region rule ratified (D-REGION1) → unblocked | owner |
| 2026-06-21 | D-REGION1 | allocation regions: implicit scope-inferred (A, beginner) + explicit `region r { … }` (B, expert) — both; unblocks D-ALLOC2 | owner |
| 2026-06-21 | D-TAINT1 | `#tainted` tag + `sanitizer fn`; tainted→sink is E0721 (gated on D-EFF1); full IFC (opt B) deferred post-Epoch-3 → D-IFC1. **(impl 2026-06-24)** value-fact tag `#Tainted expr` (PascalCase, D-CASING1) + `#Sanitizer fn` modifier; intraprocedural forward dataflow in `Source/Sema/Taint.rs`; sinks are `Db`/`Exec`/`Net`-effect Core calls; E0721 with sanitizer fix-it; erased in codegen (I3). Spelling fork `sanitizer fn` vs `#Sanitizer fn` deferred → D-TAINT-SAN (default `#Sanitizer fn` shipped). | owner |
| 2026-06-21 | D-QUAL2 | two qualifier kinds — `trait` (methods, dispatches) vs `tag` (no methods, erases); unblocks value-tags cluster | owner |
| 2026-06-21 | D-UNINIT1 | `#uninit` binding marker, gated by `use core.mem`; write-before-read proof (E0420) | owner |
| 2026-06-21 | D-REGEX1 | `core.regex` on the Rust `regex` crate (owner-approved I6 bootstrap; native-ize before Epoch 3 ends) | owner |
| 2026-06-21 | D-SCAP1 | scoped capabilities: `#Grant(Fs) { caps -> … }`, RAII-revoked (gated on D-EFF1). **(impl 2026-06-24)** the dual of `#Caps` — a `#Grant(<effects>) { <handle> -> … }` statement region (`KW_GRANT`) that authorizes the listed effects inside the block and binds a first-class capability handle, RAII-revoked at scope end (erased in codegen, I3). Like `#Caps`, the block is bounded to the granted set (transitively): an effect inside that the grant omits has no backing capability — **E0712** (the dual of E0741). The handle is sema-only and unnameable as a type; letting it escape (returned, stored, aliased, captured) is **E0711**. `tests/ui/grant_out_of_set` + `tests/ui/grant_handle_escapes`; `examples/features/effect_grant.jet`. | owner |
| 2026-06-21 | D-UNIT1 | units as `#unit(usd)` tag + `9.99.usd` literal (gated on D-QUAL2) | owner |
| 2026-06-21 | D-LIN1 | single-use values `#SingleUse` (renamed from `linear`; gated on D-QUAL2) | owner |
| 2026-06-21 | D-TGT1 | `targets:` list replaces `kind:` (kind removed; greenfield) | owner |
| 2026-06-21 | D-TGT2 | first targets: library, executable, test, example; benchmark (c80, shipped 2026-06-25), plugin reserved | owner |
| 2026-06-21 | D-TGT3 | bare keyword (no fields) or block (with fields) | owner |
| 2026-06-21 | D-TGT4 | bare `executable` searches `src/main.jet` then `<package>.jet` | owner |
| 2026-06-21 | D-TGT5 | `#test` fns auto-collected; optional `test { entry: … }` | owner |
| 2026-06-21 | D-CAP1 | capability words `view` / `edit` / `take` / `share` (edit, share new) | owner |
| 2026-06-21 | D-CAP4 | `api:` per-target field — `library { api: stable }` | owner |
| 2026-06-21 | D-CAP5 | library-producing targets emit capability metadata; binaries infer | owner |
| 2026-06-21 | D-CAP6 | inference is the library default forever; `api: explicit` opt-in | owner |
| 2026-06-17 | D-DEP1 | third-party deps ship as FFI-wrapping Jet packages (`extern rust`, S50); compiler stays zero-crate; native port later keeps API | owner |
| 2026-06-17 | D-NET1 | TLS via `rustls` delivered as the `core.tls` package (D-DEP1); `core.http`→`core.tls`; no compiler crate | owner |
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
| 2026-06-12 | S28 | amended: `impl Type.Trait` (D-IMPLDOT1); `.` for paths  | owner |
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
| 2026-06-16 | S51 | amended: std library module renamed **`core`** (`core`); `import std` → teaching error | owner |
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
| 2026-06-19 | D-NARG-D2 | **defaults may reference earlier params** (A): `fn box(w: Int, h: Int =  w)` allowed. Owner: hard work on the backend so the frontend feels magic, while exposing expert tools. **Ratified, implemented 2026-06-20** (current default-fill treats defaults as self-contained; extend to allow earlier-param refs) | owner |
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
| 2026-06-19 | D-DIST1 | **`UserId :: distinct Int`** (C, binding form): reuses the ratified `::` immutable sigil (D-BIND1) + the `distinct` keyword; no new separator token; `distinct`-over-`distinct` chaining rejected in v1. **Ratified, implemented 2026-06-20.** **2026-06-22: spelling follows D-BIND2 — the `::` becomes `@=` (`UserId @= distinct Int`); covered by the `@=` migration** | owner |
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
| 2026-06-25 | D-PUBLISH1A | **`jet publish` command shape** (A): one verb, sibling of `add/update`; version read from `pkg.jet` (single source of truth); pre-flight **refuses** a dirty tree + failing tests, `--allow-dirty` escape; CLI-version (B) and `jet release` (C) rejected. New publish errors take codes from E1219. **Ratified** | owner |
| 2026-06-25 | D-VERSION1 | **version immutability** (A): a published version is permanent — re-publish refused (E1221); `jet yank` (with `--undo`) hides a bad version from new resolution while existing `.jet/lock` pins still install it. Overwrite (C) and no-retraction (B) rejected; only policy compatible with the D-PKGSIGN1 checksum floor. **Ratified** | owner |
| 2026-06-25 | D-RESOLVE1 | **resolver default** (A): a range like `textkit#^1.2` resolves to the highest compatible published version, frozen in `.jet/lock`; repeat builds stay on the locked version until explicit `jet update`. Exact-pins-only (B) and re-resolve-every-build (C) rejected; matches cargo + the `@latest`/`--locked` model (S52). **Ratified** | owner |
| 2026-06-25 | D-LOCK1 | **`.jet/lock` commit policy** (A): `jet new` commits `.jet/lock` for executables (dropped from `.gitignore`) and git-ignores it for libraries, so app clones rebuild byte-identically. Amends the D-JPK-FILES file table line `.jet/lock` "Checked in? no" → "yes for executables". Commit-always (B) / never (C) rejected. **Ratified** | owner |
| 2026-06-25 | D-SERDE9 | **generic serde bound propagation** (A): a `#[Codable]` generic type auto-adds `T: Encode`/`T: Decode` (plus the existing `Clone`) to the generated impls — never spelled by the user, matching the `rust_extra_clone_bounds`/`jetshow` precedent; a non-codable arg fails at the use site, not the definition. User-written bound (B) and per-instantiation/no-propagation (C) rejected. **Ratified + implemented (c136)** | owner |
| 2026-06-25 | D-SERDE10 | **phantom / non-serialized type params** (A): bound only the type params that actually reach the wire (driven by the `is_encodable_ty` field walk, skipping `#[Skip]` fields); a phantom or skip-only param carries **no** `Encode`/`Decode` bound (still gets structural `Clone`), so `Id<Kind>` serializes regardless of `Kind`. Bound-all-params (B) rejected. **Ratified + implemented (c136)** | owner |
| 2026-06-25 | D-SERDE11 | **manual bound override** (A): ship auto-inference (D-SERDE9/10) as the only path; no override attribute now, since Jet's field walk is exact (no associated types). The manual-bound-override / "shipping bound" `#[Bound(…)]` idea is **reserved** and tracked as a follow-on board card so it isn't lost (per owner). Ship-now (B) and forbid-forever (C) rejected. **Ratified + implemented (c136)** | owner |
| 2026-06-25 | D-SERDE12 | **lift the E2413 gate** (A): retire E2413 entirely once generic derive lands — delete the diagnostic, the `type_params > 0` early-out, and the codegen bails; generic `#[Codable]` becomes fully first-class with no "yet" wall; per-field checks (E2407–E2412) run on generic types unchanged. Keep-for-residual-corner (B) rejected; any future gap earns its own coded diagnostic. **Ratified + implemented (c136)** | owner |
| 2026-06-25 | D-DEP-ARCHIVE1 | **`core.archive` crates** (A): wrap `zip@2.1.3` + `tar@0.4.40` + `flate2@1.0` (all pure-Rust, no C toolchain) covering zip/tar/tar.gz in one approval. `zip`-only (B) and bundled-C libarchive (C) rejected. I6 bootstrap sanction — all three carry the D-REGEX1 native-ize obligation. **Ratified** | owner |
| 2026-06-25 | D-DEP-DB1 | **`core.db` sqlite crate** (A): wrap `rusqlite@0.31` with the `bundled` feature (SQLite C amalgamation compiled in → no system libsqlite3). System-link (B) and thin `sqlite` crate (C) rejected. I6 bootstrap sanction; native-ize end state may be "keep bundled public-domain SQLite C" (flagged for a later frozen card). **Ratified** | owner |
| 2026-06-25 | D-BFS1 | **wrapped-crate source for offline build** (A): vendor the crate source inside the wrapping package (committed `vendor/`), hash-pinned in `.jet/lock` — offline + byte-reproducible from the first build, fully auditable in the dep tree. Fetch-then-lock (B) and publish-time vendor (C) rejected; sets the supply-chain default for every D-DEP1 package. **Ratified** | owner |
| 2026-06-25 | D-LIN1-DROP | **deliberate discard of a `#SingleUse` value** (A): `drop(x)` legal only inside an `#Unsafe("reason")` region/fn — the `#Unsafe` reason IS the audit note, reusing D-UNSAFE2's audited gate; no new builtin, no second audit channel. Dedicated `discard(x,"reason")` (B) and `x.abandon("reason")` method (C) rejected. **Ratified** **(impl 2026-06-25)** `drop` builtin (`Syntax::BUILTIN_DROP`, shadowable); sema in `CheckerInfer/calls.rs` counts `drop(x)` as the terminal consumption of a `#SingleUse` value and gates it on `self.in_unsafe` (now also set for `#Unsafe fn` bodies in `Registration.rs`), else E0143; erased to a plain Rust `drop(x)` via `TExprKind::Drop` (no `unsafe`, I3). Example `examples/features/121_single_use_discard.jet`. | owner |
| 2026-06-25 | D-TXN-ROLLBACK | **`#Transact` rollback model** (C, owner-modified — LAYERED): three layers. (default) auto-snapshot every mutated value on `#Transact` entry, restore on `?`-failure — magic out of the box. (expert opt-out 1) a `Rollback` trait a type derives to customize how it snapshots/restores. (expert opt-out 2) an explicit `tx.on_rollback(() => {…})` hook (mirror of the shipped `on_commit`) to define undo by hand and skip the auto-snapshot cost. Plain-A (explicit-only) and plain-B (trait-only) superseded by the layered design. **Ratified.** **(impl 2026-06-25 — layers 1+3 fully built; layer 2 trait shape deferred to a ballot)** Layer 3 `tx.on_rollback(() => {…})` is the exact mirror of `on_commit`: a Drop-backed LIFO hook on the `JetTransaction` guard that runs only when `commit()` did NOT run (a `?`-failure/early return) and is dropped un-run on a clean commit; sema dispatch + subset + lower + emit all mirror `on_commit` (`Source/Sema/CheckerInfer/calls.rs`, `Source/Codegen/TIR/{subset,lower,emit}.rs`). Layer 1 auto-snapshot: on `#Transact` entry the compiler snapshots every root local/param the block **assigns** (`x = …`/`+=`/`x.f = …`/`x[i] = …`, recursing through nested control flow but not into nested `#Transact` blocks or lambda bodies), restored LIFO on a `?`-failure via the vetted `mod jet_txn` prelude (a clone + Drop-backed writeback; the one raw-pointer restore is confined to that module, never user code — golden parity). **Deferred corners (reported, not silently miscompiled):** (a) a value mutated *only* through a `~self` method call with no assignment, and mutation through a deep alias, are NOT auto-snapshotted in v1 — the analyzable assignment-target case is covered fully and correctly; (b) the "skip the snapshot for a value an explicit `on_rollback` handles" optimization is not expressible because the surface has no value→hook binding, so layer 3 hooks *compose on top of* (run alongside) layer-1 snapshots rather than suppressing them per-value; (c) **layer 2 — the `Rollback` trait's method shape** (paired `snapshot`/`restore` w/ an associated snapshot type vs `rollback(mut self, snapshot)` vs an undo-log) is a genuine owner-facing API fork → ballot **D-ROLLBACK-TRAIT**; the trait NAME is reserved (`Source/Syntax.rs::TRAIT_ROLLBACK`) but dispatch into the snapshot path awaits that decision. `examples/features/110_transact.jet`; `tests/effects.rs`. | owner |
| 2026-06-25 | D-TAINT-SAN | **sanitizer-fn spelling** (B): the taint-strip modifier is the PascalCase marker `#Sanitizer fn`, matching the `#Pure`/`#Unsafe`/`#Test` family (D-CASING1); bare `sanitizer fn` (A) rejected and becomes a dedicated teaching error pointing at `#Sanitizer` (E005x family, mirroring `pure`→E0053 — the teaching error IS to be added). **Ratified** **(impl 2026-06-25)** bare `sanitizer fn`/`sanitizer pub fn` → E0059 teaching error pointing at `#Sanitizer`, in `Source/Parser/Items.rs` (mirrors the `pure`→`#Pure` E0053 path); recovers by parsing as a `#Sanitizer fn`. `sanitizer` as an ordinary identifier elsewhere is unaffected. | owner |
| 2026-06-25 | D-DET-CAPAPI | **deterministic Clock/Rng API** (B — widen now): keep the shipped minimal `clock.now()`/`clock.tick(ms)`, `rng.int(lo,hi)` inclusive / `rng.float()` `[0,1)`, and **add** `rng.bool()`/`rng.pick(list)`/`rng.shuffle(~list)`, an absolute `clock.advance(...)` form, and `Duration`-based reads (mint `Duration` if needed) — parity with ambient `random.*` plus a richer clock. Names stay `Clock`/`Rng`; rename+half-open (C) rejected. **Ratified**; **(impl 2026-06-25)** built end-to-end: `rng.bool()`/`rng.pick(list)→T?`/`rng.shuffle(~list)` (every draw needs `~Rng`; `shuffle`'s list needs `~`), absolute `clock.advance(to_ms)` + Duration `clock.wait(d)` alongside relative `clock.tick(ms)`, and a minimal std-only `Duration` value (`time.ms(n)`/`time.secs(n)` pure constructors, `duration.millis()` read). `Duration`/method names registered in `Source/Syntax.rs`. `examples/features/111_determinism.jet`; `tests/determinism.rs`. | owner |
| 2026-06-25 | D-PARSE-1 | **correctness-sensitive parsers** (C): build FULL native std-only parsers — complete JSON, TOML, and SemVer (incl. `+build` metadata) — replacing today's silently-lossy subset readers; no lossy subsets, no external dep, I6 stays hard. Document-the-subset (A) and vetted-dependency (B) rejected. **Ratified** | owner |
| 2026-06-25 | D-STATE-REQ | **require-state marker** (A): `#State(S) fn` says a method is callable only when the receiver is in state `S` (wrong-state call = E0150); matches the paren-arg marker family and the shipped spelling. `#Requires(S)` (B) and `#In(S)` (C) rejected. **Ratified** | owner |
| 2026-06-25 | D-STATE-TRANS | **transition marker** (A): `#Transition(From -> To) fn` declares a state-advancing method using the `->` return-arrow glyph; `_` from-state (`#Transition(_ -> Pending)`) marks an entry constructor; matches the shipped spelling. `=>` glyph (B) and two-marker `#From/#To` (C) rejected. **Ratified** | owner |
| 2026-06-25 | D-JIT2 | **Cranelift JIT dependency home** (A, owner-modified): the Cranelift dep lives in a new workspace-member crate `jet-jit/`; the `jet` compiler crate (`Source/`) stays std-only so I6 is machine-checkable (a lockfile grep of crate `jet` shows zero external crates). OWNER MOD: the JIT ships **on by default** in the `jet` binary (not behind `--features jit`), with an opt-**out** flag to build/run without it — exact flag name (`--interpret` / `--aot-only` / `--no-jit`, "named better than `--no-jit`") chosen during the c139 build, not a separate ballot. cfg-gated single-crate carve-out (B) and out-of-tree component (C) rejected. **Ratified** | owner |
| 2026-06-25 | D-STATE-DECL | **typestate state-set declaration** (B): a typestate's states are declared in a dedicated `state TypeName { Pending, Confirmed, CheckedIn }` block — a bare-keyword declaration in the `tag`/`struct`/`enum` family, with `#State(X)` requiring a state and `#Transition(A -> B)` moving between them (one cohesive `state`/`#State`/`#Transition` family; `state` declares vs `#State` marks, like `tag` vs `#Tainted`). The set is bounded/typo-checked and tied to the type by name; erases (pure compile-time, no runtime discriminant). A dead-end state (no outgoing transition) is a **warning** (default, so a half-built machine still compiles). Loose-`tag`s (A, shipped) and overloading `enum` rejected. **Ratified** | owner |
| 2026-06-25 | D-ROLLBACK-TRAIT | **`Rollback` trait method shape** (A): the layer-2 trait a type impls/derives to customize `#Transact` snapshotting is `trait Rollback { type Snapshot; fn snapshot(self) -> Snapshot; fn restore(self: ~Self, snap: Snapshot) }` — an associated `Snapshot` type with paired capture/put-back methods, the only shape that delivers a snapshot *cheaper* than layer-1's deep clone while staying a plain trait the block dispatches on. `restore` must be total (no `?`; a fallible restore is rejected — Owner Q2). `derive Rollback` emits the field-wise full-clone impl (`Snapshot =  Self`), i.e. layer 1 but overridable (Owner Q1: derive kept for symmetry with the other auto-derives). Single combined `Self`-snapshot (B, can't beat layer 1) and undo-log `record(tx)` (C, layer 3 wearing a trait) rejected. Closes D-TXN-ROLLBACK layer 2. **Ratified** | owner |
| 2026-06-25 | D-ASSOC-NOW | **sequence associated-types completion vs D-PARSE-1** (C — both streams in parallel): D-LIB2 associated types were ratified (2026-06-17) but built parse-only (`assoc_types`/`assoc_type_impls` read only by the Formatter; an assoc-typed trait method fails E0907), gating layer 2's `type Snapshot`. Owner funds both: complete associated-type resolution (c149 → unblocks c72 layer 2) **and** build the full native parsers (c111, D-PARSE-1) concurrently. Not a syntax change — a build-priority call; associated types need no new decision (D-LIB2 already ratified). Defer-one options (A assoc-first, B parsers-first) rejected in favor of both. **Decided** | owner |
| 2026-06-25 | D-ENC-DYN1 | **dynamic value of `core.encoding` formats** (A+ — one underlying structure, per-format aliases): every format's untyped `parse` returns ONE shared rich dynamic value — the user-facing face of the internal `DataTree` (`.Object/.Array/.Int/.Float/.Text/.Bool/.Null`) — replacing the lossy flat `Map<String,String>` that TOML/YAML return today. Per owner: the single underlying type is `Data`, and `Json`/`Toml`/`Yaml`/`Csv` are **type aliases** over it (`Json = Data`, …) so internals stay unified (minimal maintenance, one walker) while beginners still see a format-named type at each `parse` call (discoverability). The shipped `JSON` enum collapses into `Data`/`Json` as a clean break (no parallel path), migrating examples 30/73/108 + the jsonfmt showcase (`Number`→`.Int`/`.Float`). Per-format dynamic enums (B), and keep-flat-`Map` (C, leaves a lossy public surface) rejected. Build = c152 (reuse `Source/Jetpack/TOML.rs`); `csv.parse` yields a shallow `Data.Array` of records. **Ratified** **(impl 2026-06-25 — c152)** `Data` is the user-facing face of `jet_std::DataTree` (variants `.Null/.Bool/.Int/.Float/.Text/.Array/.Object`); `Json`/`Toml`/`Yaml`/`Csv` registered in `Source/Syntax.rs` (`TYPE_DATA` + `is_data_type_name`/`is_data_variant`) and canonicalized to `Data` in `Sema::resolve_type` (they unify). Codegen maps all five to `jet_std::DataTree`; the dynamic `Object` payload is a `Map<String,Data>` (BTreeMap↔ordered-pairs conversion at construct/pattern boundaries). The shipped `JSON` enum collapsed into `Data` (clean break); examples 30/73/54 + jsonfmt + capstone server/config migrated, goldens re-blessed. `Data`/`Json`/`Toml`/`Yaml`/`Csv` are now reserved core type names. | owner |
| 2026-06-25 | D-ENC-YAML1 | **YAML support scope** (A): the new std-only YAML parser (c152) covers the YAML 1.2 core that serialized config needs — block + flow mappings/sequences, core-schema typed scalars, single/double-quoted + plain + block scalars (`|`/`>`), comments, `---` document markers, and `&anchor`/`*alias` reuse — but **defers explicit/custom tags** (`!!str`, `!MyType`) to a separate frozen card (c153, full YAML 1.2). On encode, anchors are always expanded (lossless, simpler). Minimal/no-anchors (B, fails on real K8s/CI YAML) and full-1.2-now incl. tags (C, tag machinery rarely used in serialized config) rejected; C re-filed as frozen c153 per owner. **Ratified** **(impl 2026-06-25 — c152)** full std-only YAML parser+renderer in the emitted prelude (`Source/Prelude/CoreLib.rs`, `jet_std::yaml`): block+flow maps/sequences, core-schema typed scalars, `\|`/`>` block scalars with chomping, comments, `---`/`...` documents, `&anchor`/`*alias`; `yaml.parse`→`Data`, `yaml.decode<T>` walks the rich tree, `yaml.to_string` renders block YAML. TOML reuses a prelude port of `Source/Jetpack/TOML.rs` (`jet_std::toml`). Examples 52/53 typed-decode nested docs; `tests/corelib.rs` round-trip tests. c153 (full YAML 1.2 — explicit/custom tags) filed as a frozen card. | owner |
| 2026-06-25 | D-CTEFFECT1 | **compile-time effect boundary** (A — three tiers, hermetic CI): build-time/`comptime` code has Tier 0 (pure, always on), Tier 1 (effects whose input is hashed into `.jet/lock` → reproducible: `@embed`, `find`, `fetch(url, sha256:)`), and Tier 2 (ambient/non-deterministic effects) which require BOTH the `#Impure("reason")` audited gate (parallels `#Unsafe`, reusing the `#(fs,net,exec)` effect-tag machinery) AND `--allow-impure` at build, so CI is hermetic unless an expert opens it. Warn-only (B), pure-only (C, denies the expert), Jai-ungated (D, footgun default) rejected. **Owner**: a project build file may alias/relax the impure gate (a config knob to disable the `--allow-impure` requirement per-project), and the `$`-as-comptime-marker idea (like c3) is split to a new ballot **D-CTMARKER1**. Gate name `#Impure` (alternatives `#Ambient`/`#NonHermetic`/`#Effectful`/`#BuildEffect`/`#Untracked` offered, confirmable). **Ratified — not yet implemented** (gates D-BUILDPROFILE1's hermeticity; built when the comptime-effects surface lands). | owner |
| 2026-06-25 | D-DOTCTOR1 | **dot-inferred construction** (A — replace U18 with `.{ }` / `T.{ }`): one leading-dot rule for every inferred construction, structs and enums alike — `.{ name: …, grade: .A }` when the type is known from context (binding annotation), `T.{ … }` when it must be named, matching the enum-variant dot already shipped. Coexist-with-bare-`{}` (B, violates I8 — two spellings one job) and keep-U18 (C, structs un-dotted while enums dotted) rejected. Owner-Q defaults (confirmable): `.{}` is the empty/unit construct; `.{ }` works in return position when `-> T` supplies the type; positional `T.(a,b)` deferred (named-fields only for v1). **Ratified — not yet implemented** (parser/sema/codegen + formatter + example + re-bless examples/snapshots when built). | owner |
| 2026-06-25 | D-MONOREF1 | **monorepo package addressing** (A — dot form `source.package`): a named-source member is `mono.ranker` (reads like field access, matches the `default.ripgrep` sugar); an in-repo sibling is path-style `infra/logging` with a bare name (`logging`) as sugar when unambiguous. Resolution is index-first — fetch the source's `jetpack.toml`/manifest only, then a sparse fetch of just that package's subtree + its transitive in-repo deps (so "pull one out of a 40-package repo" never clones/builds the rest); full-clone fallback when a provider lacks sparse checkout. Colon form `"mono:ranker"` (B) and no-in-repo-addressing (C, kills the monorepo story) rejected. **Ratified — not yet implemented** (jetpack resolver work). | owner |
| 2026-06-25 | D-BUILDPROFILE1 | **build profiles** (A — named, flag-selected): a package's `build { }` surface defines named profiles (`release`/`debug`/`ci` as `Build.{ optimize: …, targets: […] }`); the active profile is chosen by an explicit flag (`--release` sugar for `--profile= release`, general `--profile=<name>`), never by ambient environment — same commit + same flag ⇒ byte-identical binary on every machine. Blessed names `release`/`debug` carry built-in defaults; others are user-defined. Ambient-env selection (B, the `CMAKE_BUILD_TYPE` footgun, an ungated ambient read on the default build) rejected as it contradicts D-CTEFFECT1. **Ratified — not yet implemented** (build-driver + `build {}` surface). | owner |
| 2026-06-25 | D-CTCODEGEN1 | **build-time code generation** (A — re-enter the checked pipeline): every derive/comptime step that synthesizes code emits a typed *source fragment* that re-enters lexer→parser→sema exactly like hand-written code — never injects pre-parsed AST past the sema gatekeeper. Standing architecture rule: no generation path may inject nodes downstream of sema, so generated code is trustworthy-by-construction (I3 codegen-dumb, R2 sema-gatekeeper, I2 rustc-never-speaks all hold) and any error in generated output surfaces as a real sema diagnostic pinned to the user's trigger site (the struct/field/derive), with the generated fragment shown only as optional context. AST-injection (B, Jai-style — breaks I3/R2, risks I2, opens the user-macro door v1 closed) rejected. **Ratified** — a standing rule the existing `#[Codable]` derive already follows; enforced for all future derives/build steps (to be encoded in `architecture.md`). | owner |
| 2026-06-25 | D-COMPILERLIB1 | **factor the compiler into internal seams** (A — during Epoch 3): split `Source/` into internal Rust library seams — `lexer` / `parser` / `sema` / `tir` / `codegen` / `comptime` + a `driver` that composes them — each owning its types behind a small documented API, with today's coarse `lib.rs` (`check_*`/`compile_*`/`render_*`) kept as a thin façade built on the seams. Lets the build driver and LSP drive each stage as a library call (and inspect the typed IR between sema and codegen) instead of re-deriving the pipeline — collapsing the LSP's forked pipeline onto one — and gives the future self-host port crisp crate-by-crate boundaries (rustc/Roslyn precedent). I6-safe: these are the compiler's OWN internal crates, not external deps, so no invariant carve-out is needed. Defer-to-`jet-bootstrap` (B, carries a forked-pipeline tax until a big-bang port) rejected. Owner-Q3 (seam boundaries) left confirmable. **Ratified — not yet implemented** (Epoch 3 refactor). | owner |
| 2026-06-25 | D-WORKSPACE1 | **monorepo index → a Jet `Workspace` surface** (B — fully computable, owner-modified from rec A): retire the root `jetpack.toml` monorepo index for a `module workspace` (in `workspace.jet`, parallel to `env.jet`/`system.jet`), so the whole project is ONE grammar (Jet) instead of two and the `sources:` double-home across `jetpack.toml`+`env.jet` ends. Owner chose **B (full power)** over the rec A (restricted `find()`-only + materialized `.jet/workspace.lock`): the `members:` field may run arbitrary `comptime` (members derived from anything — `find("./packages") + comptime gen()`), consistent with the owner's computed-modules / pure-eval direction. Accepted trade: external tools must evaluate Jet to know the layout (no static-readable index) — mitigated by the resolver still emitting a generated lock for the common case. Keep-`jetpack.toml` (C, two languages) rejected. **Ratified; partial impl 2026-06-28**: `workspace.jet` with `module workspace { members: [...] }` and `members: find("./packages")` is implemented, tested in `tests/workspace.rs`, and demonstrated in `examples/workspace/`; arbitrary comptime member expressions, unified-lock verification, sparse subtree fetch, and old-index migration remain on c90. | owner |
| 2026-06-25 | D-METADEPTH1 | **v1 user-metaprogramming ceiling** (A — reflection/derives only): user code may READ type info (S56 reflection) and author derives that generate behavior (`derive Encode for T`) on top of the compiler-provided built-ins (S55, e.g. `#[Codable]`), but may NOT rewrite arbitrary code, inject AST, or define macros. The no-macros non-goal is **load-bearing**, not provisional (Owner Q4). A⊂B⊂C is a ladder revisitable post-self-host: B (a read-only `lint`-style rejection pass — Marcus's "no panic in net") and C (full Jai message loop / user macros) stay OFF the v1 table and rise only by a future vote. Honors I3 (codegen dumb), I8 (small surface), and pairs with D-CTCODEGEN1= A (generated code re-enters sema). Per owner, **full Jai (option C) is tracked as a frozen card** for later consideration. Devil's-advocate analysis (full-Jai vs A across beginner/student/power-user/enterprise) recorded on the ballot card (Owner Q5): "inside the language" removes only a toolchain seam, not Jai's costs (local reasoning dies; still build-time RCE/non-reproducible); A is the safe-by-default, forward-compatible floor. **Ratified** (S56 user-authored-derive surface tracked as a board card; built-in derives already shipped). | owner |
| 2026-06-25 | D-CTMARKER1 | **`$` sigil for compile-time constructs + a comptime execution block** (C, plus Owner-Q4 confirmed): `$` is reserved for the metaprogramming **splice site only** — `$name` marks a compile-time value being woven into runtime/generated code (the reflection/derive surface, D-METADEPTH1=A) — and the `comptime` keyword keeps declaring bindings/`comptime if`, so there is NO `$if`/`comptime if` duplicate (I8 holds). `$`-everywhere C3-style (A, a second spelling for `comptime`) and keyword-only/no-sigil (B, leaves the splice unmarked) rejected. **Owner: "Ensure we add a comptime execution block"** — confirms Owner Q4: add a keyword-spelled `comptime { … }` statement block (Jet's gated equivalent of Jai's free-standing `#run { … }`, consistent with `comptime if`), filling the one gap where Jet today has comptime bindings + `comptime if` but no comptime block. Both still run inside the D-CTCORE1 pure-Core whitelist + D-CTEFFECT1 reproducible/`#Impure` tiers (Jai's power, Jet's reproducible-by-default wall). `$` is currently unused in the lexer; verify before code. **Ratified — not yet implemented** (board card; downstream of the S56 reflection/derive surface c155). | owner |
| 2026-06-25 | D-DOTCTOR2 | **retire the dotless `T { }` struct literal** (A — one spelling): now that D-DOTCTOR1=A makes `T.{ … }` the named-construction form, the bare dotless `Type { … }` (S29 / S29-FLUSH) is **removed** — `T.{ … }` is the sole named-construction spelling, so named struct construction reads exactly like named enum construction (`T.{ … }` beside `T.Variant`, `.{ … }` beside `.Variant`) and a beginner learns one rule: a leading dot means "construct". Typing the old `Type { … }` is a teaching error **E0320** ("named construction uses a dot: `Server.{ … }`", auto-fixed by inserting `.` before `{` and by `jet fmt`). Amends **S29** (the `Type { f: v }` construction form) and **S29-FLUSH** (the flush block, which now hugs the dot — `Point.{x: 3.0, y: 4.0}`); the flush destructuring pattern `Point{x, y} :: make()` moves to `Point.{x, y}` for the same symmetry, and U18's escape-hatch is now `T.{ … }`. Coexist (B, two spellings for one job = I8 violation) and inference-only-dot (C, named struct dotless while named enum dotted = asymmetric) rejected. **Ratified — not yet implemented** (parser/sema/formatter + E0320 + a fmt round-trip stability test + migrate every struct literal & destructuring pattern across examples/tests + re-bless snapshots; rides c158 with D-DOTCTOR1). | owner |
| 2026-06-25 | D-METAREFLECT1 | **reflection read-API surface** (B — one reflected `Type` handle): build-time code reads a type's shape through a single reflected value — `T.reflect()` returns a `Type` whose `.name`/`.fields` hang off it, each field carrying `.name`/`.ty`/`.markers` and `.has_marker("…")` — rather than a bag of free comptime builtins (A, scatters into globals) or a dedicated `comptime for field in T` control form (C, a second comptime control form that still needs B underneath). One discoverable, LSP-completable entry point (Blueprint north-star, I8), mirroring Swift `Mirror` / Zig `@typeInfo`-as-value; C may later be added as pure sugar over B if field-walking dominates real derive bodies. Costs one first-class comptime `Type` value. This is the **read** half of the S56 user-derives/reflection surface. Marker reads are PascalCase (`has_marker("Skip")`, D-CASING1). **Implemented in the user-derive slice** (2026-06-28 verification: `T.reflect()`, `.name`, field marker reads used by derive expansion; richer reflection breadth/privacy hardening remains on c129). | owner |
| 2026-06-25 | D-PLUGIN1 | **plugin target model & ABI substrate** (B — WASM sandbox): a package with `target: plugin` (the reserved keyword, E1210 today) compiles to a sandboxed WASM module the host loads and calls through a typed interface; the plugin runs isolated and can only touch what the host explicitly grants, so loading an untrusted third-party plugin is **safe by default with no `#Unsafe` gate** — exactly what I1 and the beginner experience demand. Native cdylib + `#Unsafe` (A — every load is expert-gated unsafe, the I1 footgun the default must never be) and out-of-process RPC (C — highest latency, no shared typed state) rejected; A is the natural **future expert opt-in** layered on top of B, deferred to its own card so the safe default ships first. **I6 cost named honestly:** this introduces a WASM runtime as a new stdlib external dependency requiring **owner approval** and, per the Epoch-3 dependency rule, eventual native Jet/Rust replacement — a gate, not a ranking factor. That dep approval is its own open ballot **D-DEP-WASM1** (which WASM runtime; rec A wasmtime), and no plugin code is written until it lands. Deferred sub-decisions (Owner Q1): the versioning/ABI handshake (reuse D-CAP4 `api: stable`?) and the export-surface spelling (`#Plugin` marker vs manifest `entry:` + `pub` contract) become full cards once the engine is chosen. **Ratified — not yet implemented** (plugin-target backend + loader; blocked on D-DEP-WASM1). c81 | owner |
| 2026-06-25 | D-WORKSPACE2 | **workspace surface keyword + filename** (A — `workspace` / `workspace.jet`): the Jet-grammar file that indexes a multi-package repo (D-WORKSPACE1=B) is `module workspace` written in `workspace.jet`, confirming D-WORKSPACE1's Owner Q4. Owner kept the **industry-standard term** over the aviation-themed recommendation B (`fleet`/`fleet.jet`) and the rest of the menu (`roster`/`wing`/`squadron`; `hangar` rejected — collides with the Jetpack store `Source/Jetpack/Store.rs`; `manifest` rejected — overloaded vs the per-package `pkg.jet`). `workspace` reads as the universal "set of packages in this repo" with zero new vocabulary and is collision-free against `env.jet`/`config.jet`/`pkg.jet`. Registered in `Source/Syntax.rs` (`WORKSPACE_FILE = "workspace.jet"` + reserved namespace `workspace`, I7). **Ratified** **(impl 2026-06-28 — c90)** `workspace.jet`, namespace `workspace`, and field `members` are registered and parsed. c156 | owner |
| 2026-06-25 | D-METADERIVE1 | **user-derive authoring + output mechanism** (A — `derive Trait for T` + source-fragment re-entry): a library author writes a derive as an impl-like block `derive Wire for T { … }`; its body uses reflection (D-METAREFLECT1=B `T.reflect()`) and `$name` splices (D-CTMARKER1=C) to build Jet **text** that re-enters lexer→parser→sema exactly like hand-written code — the literal D-CTCODEGEN1=A path, so errors pin at the user's `#[…]` derive trigger (E-DERIVE-FRAGMENT family) and rustc never speaks (I2). Triggered by the existing `#[…]` marker router (`split_type_markers` routes unknown marker names to user derives — zero new trigger syntax; a user `#[Wire]` reads exactly like the built-in `#[Codable]`), Rust-style **local-only** orphan rule (derive only where the trait or the type is defined). Typed-AST return (B — receives pre-built AST, can't pin spans, reopens D-CTCODEGEN1=A) and attribute+skeleton `#[Derive(T)] impl …` (C — splits the declaration, header duplicates the attribute) rejected. This is the **authoring** half of the S56 surface (c155); the read half is D-METAREFLECT1=B. Matches the two lauded author-side systems (Rust proc-macro `quote!` TokenStream + Swift `@attached` macros — both emit re-checked source, never a pre-validated tree). **Implemented 2026-06-28** (`derive Trait for T`, `emit("...")`/triple-string source fragments, `$name` splices, local orphan rule, generated-fragment diagnostics, example `128_user_derive`). Emit-template quoting refinements remain a follow-up, not a blocker. c155/c131 | owner |
| 2026-06-26 | D-COMPILERSEAMS1 | **compiler seam workspace crate graph** (B — foundation crate + merged `jet-codegen` seam): `jet-foundation` crate holds `Syntax`, `Diagnostics`, `AST`, `Span`, `Generics` (shared leaf types); six seam crates (`jet-lexer`, `jet-parser`, `jet-sema`, `jet-codegen`, `jet-comptime`, `jet-driver`) each depend on it; TIR stays as an internal submodule inside `jet-codegen` (resolving the bidirectional `Cx`/`TFunc` cycle without moving code). Option A (full split — move `struct Cx` into `jet-tir`) rejected: mechanical but unnecessary when the self-host port can import `jet_codegen::TIR::TFunc` cleanly. Option C (no foundation, shared types stay in root `jet` lib) rejected: `cargo tree` shows no seam boundaries, undermining the I6 machine-check and self-host crate-by-crate goal. Crate naming sub-decision split to **D-COMPILERSEAMS2**. **Ratified** **(impl 2026-06-28 — c160/c89)** workspace split exists with `jet-foundation`, `jet-lexer`, `jet-parser`, `jet-comptime`, `jet-sema`, `jet-codegen`, and `jet-driver`; `tests/truthfulness.rs` enforces path-only dependencies for the compiler seam crates (I6). | owner |
| 2026-06-26 | D-CTFIND1 | **`find` in comptime Tier 1** (B — new `find(glob) -> [String]` comptime builtin): `find` is a first-class comptime callable that accepts a glob pattern and returns a sorted list of matching file paths, hash-recorded into `.jet/lock` under `[[comptime_inputs]]` so a change in the file set causes a rebuild. U4 `imports: find("./path")` import-discovery also gains Tier-1 hash-recording as an orthogonal benefit. Option A (U4 import-discovery directive only — hash the discovered module set but `find` is not callable inside `comptime { }`) rejected: makes the "Tier-1 effect" framing incoherent — `find` would be a manifest directive that cannot be used in `comptime` blocks. Glob implementation sub-decision split to **D-CTFIND2**. **Ratified — not yet implemented** (c157 Stage 2; gates after D-CTFIND2 resolves glob implementation). | owner |
| 2026-06-26 | D-NETDEP1 | **build-time `fetch` network backend + full HTTP library mandate** (A — pure-Rust HTTP client, owner-expanded): approve a small pure-Rust, blocking HTTP crate (`ureq`/`minreq` — no async runtime, no C) as a **runtime-side** dependency to make D-CTEFFECT1's Tier-1 `fetch(url, sha256:)` actually download, verify against the `sha256:` pin, bake the bytes in, and record `{url, sha256}` in `.jet/lock` so every machine's build is byte-identical (the Nix/Cargo/Zig hash-pinned-fetch model). Owner-gated exactly like Cranelift (D-JITDEP1), regex (D-REGEX1), sqlite (D-DEP-DB1), wasmtime (D-DEP-WASM1): scoped, hash-pinned, carrying the native-ize obligation; **I6 holds** — never enters compiler `Source/`. Shell-out to curl/wget (B — unvetted ambient tool, Windows gaps), git-only fetch (C — can't fetch a plain file URL), and defer (D — leaves the ratified `fetch` a stub) rejected. **Owner expanded the mandate:** the goal is not just the `fetch` backend but **a full, complete HTTP library — both client and server, better than Go's `net/http` — as a Jet core library.** The approved crate is the *bootstrap*; the native-ize end-state (per the Epoch-3 dep rule) is a first-party Jet/Rust HTTP client+server stdlib. This is now a major core-library track: the client+server **API surface** (request/response/handler/router/middleware naming and shapes) will need its own design + ballots before that code is written; `fetch(url, sha256:)`'s build-time backend ships first against the bootstrap crate and unblocks c157. **Ratified — not yet implemented** (c157 fetch backend immediate; full HTTP-library plan + API ballots to follow). c157 | owner |
| 2026-06-26 | D-COMPILERSEAMS2 | **compiler seam crate naming convention** (A — `jet-<seam>` technical): the seven workspace-member crates created by D-COMPILERSEAMS1=B are named `jet-foundation`, `jet-lexer`, `jet-parser`, `jet-sema`, `jet-codegen`, `jet-comptime`, `jet-driver`. Matches the existing `jet-jit` / `jet-net` convention; zero mapping to learn; tells contributors immediately what each crate does. Aviation theme (B — `runway`/`flightplan`/`airspace`/`payload`/`groundcrew`) and `jetc-<seam>` compiler prefix (C) rejected. **Ratified** **(impl 2026-06-28 — c160/c89)** the workspace members use the approved `jet-<seam>` names. | owner |
| 2026-06-26 | D-CTFIND2 | **glob implementation for the `find` comptime builtin** (A — hand-rolled, owner-expanded to full spec): the `find(glob)` builtin uses a hand-rolled, I6-clean std-only glob engine supporting `*` (single-level), `**` (recursive), `?` (single char), `{a,b}` (brace expansion), and `[abc]` / `[a-z]` (character classes). **Owner overrode the recommendation's "95% coverage" scope:** brace expansion and character classes are required — "not something half assed." Zero external dep; native-ize obligation does not apply. Option B (`glob` crate) rejected. **Ratified — not yet implemented** (c157 Stage 2, after D-CTFIND1 builtin lands). | owner |
| 2026-06-26 | D-HTTPLIB1 | **`core.http` server handler model** (A — function-first mux): server handlers are plain `fn(req: Request) -> Response` functions registered on a `mux` value (`mux.get("/path", handler)`); route params via `req.params["id"]`; `http.serve(addr, mux)?` starts the listener. Typed extractors (B — Axum-style `fn(id: Path<String>)`; medium learning curve) and unified fn / Rack-style (C — user-level routing; most flexible but least discoverable) rejected. **Ratified — not yet implemented** (c164; API surface design + bootstrap crate selection precedes code). c164 | owner |
| 2026-06-26 | D-HTTPLIB2 | **`core.http` module structure** (B — split `core.http.client` / `core.http.server`): the HTTP library exposes two named submodules; a CLI tool imports only `core.http.client` without pulling in the server router; a microservice imports only `core.http.server`. Unified `core.http` (A — Go net/http model; everything bleeds into one namespace) rejected: a known pain point. Both submodules live in one `core.http` package but are cleanly separated. **Ratified — not yet implemented** (c164). c164 | owner |
| 2026-06-26 | D-HTTPLIB3 | **`core.http` v1 protocol scope** (C — HTTP/1.1 + HTTP/2 + WebSocket): the first-party `core.http` covers all three in v1. HTTP/1.1 only (A — too limited for real deployment) and HTTP/1.1 + HTTP/2 without WebSocket (B) rejected: WebSocket is table-stakes for any TypeScript replacement (every modern web app uses it); HTTP/2 is required for gRPC. Bootstrap crates (`ureq`, hyper, tungstenite) are all owner-approved on the same posture as rusqlite — runtime-side, I6 holds, native-ize obligation. **Ratified — not yet implemented** (c164). c164 | owner |
| 2026-06-26 | D-HTTPLIB4 | **`core.http` TLS** (B — rustls, pure Rust): HTTPS works out of the box via `rustls` + `webpki-roots` (cert bundle embedded, ~3MB overhead, zero system dep). System TLS / `native-tls` (A — links OpenSSL/SecureTransport/SChannel; non-Nix Linux users need `libssl-dev`) rejected in favor of zero-friction installs; HTTP-only v1 (C — makes the library unusable for any real-world API) rejected. Consistent with the native-ize obligation (pure Rust). **Ratified — not yet implemented** (c164). c164 | owner |
| 2026-06-25 | D-DEP-WASM1 | **plugin-sandbox WASM runtime** (A — wasmtime + Component Model): approve **wasmtime** as the runtime-side dependency that backs D-PLUGIN1=B's `plugin` sandbox — the audited, widely-embedded (Zed/Shopify/Fastly/Envoy) secure WASM engine. It is Cranelift-based, so it **reuses the dep already approved in D-JITDEP1** (no new codegen backend), and its **Component Model** delivers the typed host↔plugin interface D-PLUGIN1 promised (a `.wit` contract, deny-by-default capabilities) without hand-marshaling. wasmi (B — tiny pure-Rust core-wasm interpreter, closest to the I6 end-state but hand-marshaled interface + interpreted speed), wasmer (C — no Cranelift reuse, less-mature Component Model), and write-our-own-now (D — standing up a security sandbox from scratch spends the one resource, safety, we don't) rejected. **Runtime-side only — I6 holds** (never in `Source/`; like Cranelift/regex/sqlite); hash-pinned in `.jet/lock`; carries the D-REGEX1 native-ize obligation whose frozen end-state IS option D (our own WASM runtime). Unblocks D-PLUGIN1 — and the deferred versioning/ABI + export-surface sub-decisions now promote to full cards on wasmtime's interface model. **Ratified — not yet implemented** (plugin-target backend + loader, Epoch 3; the wasmtime crate is wrapped when that backend is built). c81 | owner |
| 2026-06-27 | D-RAYLIB1 | **official graphics package** (A — first-party `core.raylib` FFI-bridge package): Jet ships an official opt-in raylib binding for games, tools, visual demos, and flagship examples. It is a package / stdlib bridge, **not** a compiler dependency: it follows the existing FFI-bridge pattern, sources raylib from nixpkgs on Nix and bundled raylib source where needed, and never adds an external crate to `Source/` (I6 holds). Community-only (B) and own-renderer-only-later (C) rejected; an own renderer may still layer on top later. **Ratified — not yet implemented** (c60/coivmgi; plan `raylib-graphics`). | owner |
| 2026-06-27 | D-GENMOD1 | **generic modules** (A — ML-functor-style module parameterization): a module may be parameterized by a type or value, and instantiating it produces a new normal module with specialized exported items. Generic types/functions remain the default; generic modules are for cases where a parameter set governs a family of related types/functions/modules. Status-quo generic-types-only (B) and defer (C) rejected. **Ratified — not yet implemented** (c91/c1jixkit; blocked on D-GENMOD2 for exact type/value parameter and instantiation spelling). | owner |
| 2026-06-27 | D-GRAPHEME1 | **Unicode graphemes and normalization** (B — first-party `core.text.unicode` package, not Core): grapheme-cluster iteration and Unicode normalization (NFC/NFD) ship as an opt-in first-party package so correct user-facing text is available without forcing UCD table size into every program. Core-by-default (A) rejected for mandatory size; grapheme-only (C) rejected because normalization is part of the same correctness job. **Ratified — not yet implemented** (c66/cuiw349; plan `unicode-text`). | owner |
| 2026-06-27 | D-CODECS1 | **compression codecs** (A — gzip + zstd now, brotli later): standalone codec APIs ship as `core.compress.gzip` and `core.compress.zstd`, separate from archive containers; brotli is deferred to a follow-on. Bootstrap implementations use the owner-approved stdlib bridge posture (runtime/package side, hash-pinned, native-ize obligation; never in compiler `Source/`). Package-only (B) and archive-only (C) rejected. **Ratified — not yet implemented** (c67/cviw4t7; plan `compression-codecs`). | owner |
| 2026-06-27 | D-PUBPKG1 | **package-scoped visibility** (A — `pub(package)`): add a visibility tier between private and public. `pub(package)` exposes an item to packages in the same payload/workspace package boundary while hiding it from downstream consumers. `internal` (C) rejected to avoid a new keyword; private+`pub` only (B) rejected because it over-exposes shared internals. **Ratified — not yet implemented** (c74/c12iwkw3; parser/sema/API-surface docs + ui snapshots). | owner |
| 2026-06-27 | D-SWIZZLE1 | **vector swizzles** (A — member swizzles read + write): vector lane names and swizzles (`v.xyz`, `v.wzyx`, `v.xy = .{...}`) are blessed member access on vector types, including lvalue swizzles. Read-only (B), method-only (C), and defer (D) rejected. Overlapping write swizzles must be diagnosed rather than miscompiled. **Ratified — not yet implemented** (c82/c1aix1yl; parser/sema/codegen/diagnostic + examples). | owner |
| 2026-06-27 | D-PROP2 | **effect prohibition spelling** (A — `#(!Net)` inside the existing effect set): a leading `!` before an effect name means this function and every reachable callee must not use that effect. Dedicated `#Forbid(Net)` (B) and `#(-Net)` (C) rejected. **Ratified — not yet implemented** (extends D-PROP1; parser effect-list negation + sema inverse propagation + E-code + ui snapshot). c18 | owner |
| 2026-06-27 | D-PROTO2 | **protocol declaration spelling** (A — `protocol Name { client -> server: Msg(...) }`): a top-level `protocol` block declares the ordered conversation and generates `.Client`/`.Server` handles over the existing linear + typestate machinery. `session` (B) and hand-written `state`/`#Transition` (C) rejected. **Ratified — not yet implemented** (parser/Syntax.rs/sema generated handles + example + ui snapshot). c20 | owner |
| 2026-06-27 | D-VARIADIC1 | **variadics and spread** (A — `...` for all three jobs): variadic parameters use final-position `name: ...T`, call spread uses `f(...xs)`, and list spread uses `[...a, x, ...b]`. List-only (B), variadic-only (C), and continued deferral (D) rejected. This is distinct from S75 fan-out, which calls once per element. **Ratified — not yet implemented** (parser/sema/codegen + diagnostics + examples). c93 | owner |
| 2026-06-27 | D-SEMINDEX1 | **stable semantic-index API** (A — versioned public API over compiler seam crates): expose symbols, references, types, call graph, and effects through a stable public query surface instead of private LSP internals. Internal-only (B) and defer (C) rejected. **Ratified — not yet implemented** (public API crate/module + schema/version tests). c96 | owner |
| 2026-06-27 | D-DBDRIVER1 | **generic database driver interface** (A — Driver trait + parameterized-only API, SQLite first): `core.db` grows a backend-neutral driver layer that accepts SQL plus separate parameters and exposes no raw string-execute escape. Per-database-only APIs (B) and async-gated deferral (C) rejected. **Ratified — not yet implemented** (core.db trait/types + SQLite implementation + examples/tests). c117 | owner |
| 2026-06-28 | D-DOTFIELD1 | **dot-field struct literal syntax stays rejected** (A — keep D-DOTCTOR1 status quo): `.{ field: value }` and `T.{ field: value }` keep Jet's existing field syntax; Zig-style `.{ .field = value }` is not added. **Ratified (declined/no build)**. c39 | owner |
| 2026-06-27 | D-PENDING1 | **blessed async/loading state type** (B — `Loadable<T, E>` stdlib enum): add a standard enum for idle/loading/loaded/failed state instead of forcing every app to invent it. **Implemented/closed in Tower; canonical behavior recorded here.** c40 | owner |
| 2026-06-28 | D-FMTPARENS1 | **formatter preserves author grouping parentheses** (A — preserve all author parens): `jet fmt` must not strip grouping parens the author wrote, even when precedence makes them redundant. **Ratified — not yet implemented** (formatter tests/docs). c45 | owner |
| 2026-06-27 | D-DUPLINT1 | **copy-paste drift lint declined** (C — do not pursue): no structural-duplication lint is added for now. **Ratified (declined/no build).** c50 | owner |
| 2026-06-28 | D-RANDSPLIT1 | **PRNG and CSPRNG APIs are structurally separated** (A — distinct namespaces + typed outputs + misuse lint): seedable deterministic randomness and cryptographic randomness must be separate enough that crypto misuse is caught. **Ratified — not yet implemented** (API/lint/tests). c52 | owner |
| 2026-06-28 | D-PRELUDEX1 | **prelude opt-out only** (A — no library injection): add a way to opt out of the prelude, but do not let arbitrary libraries inject names into the no-prefix surface. **Ratified — not yet implemented**. c63 | owner |
| 2026-06-28 | D-BIGINT1 | **Core BigInt** (A — explicit construction, no auto-promotion): add arbitrary-precision integers as an explicit Core type; fixed-width `Int` arithmetic does not silently promote. **Ratified — not yet implemented**. c65 | owner |
| 2026-06-28 | D-PQCRYPTO1 | **crypto envelope gets algorithm-agility seam now** (A — PQ algorithms later): design crypto envelope APIs so hybrid/PQ primitives can slot in without breaking users, but do not ship PQ algorithms yet. **Ratified — plan/API work pending.** c71 | owner |
| 2026-06-28 | D-GLOBIMPORT1 | **wildcard imports stay rejected** (A — keep D-MOD2 status quo): `use module.*` remains unsupported; explicit imports stay canonical. **Ratified (declined/no build).** c73 | owner |
| 2026-06-28 | D-TYPEALIAS1 | **transparent type aliases return narrowly** (B — `alias X = Y` for generic shortcuts only): aliases are for shortening long generic type spellings, not for primitive/unit newtypes. **Ratified — not yet implemented**. c76 | owner |
| 2026-06-28 | D-MATURITY1 | **maturity tags are doc-only now** (B — propagation later): `#Experimental`/`#Tested`/`#Hardened` are not semantic markers yet; capture as documentation convention until effect-like propagation is justified. **Ratified (docs-only/no compiler build now).** c79 | owner |
| 2026-06-28 | D-BUILDNORM1 | **content-addressed build cache normalization contract** (A — AST-level, rename-sensitive): hash normalized source/semantic structure, but identifier renames remain content changes. **Ratified — plan/build-cache work pending.** c85 | owner |
| 2026-06-28 | D-DOSSIER1 | **dossier view deferred behind semantic index** (B): do not build scattered-member dossier views until D-SEMINDEX1 is stable. **Ratified — blocked on c96.** c87 | owner |
| 2026-06-28 | D-BREADCRUMB1 | **phantom breadcrumb hints deferred behind semantic index** (B): do not build editor phantom stubs until semantic-index/dossier foundations exist. **Ratified — blocked on c96.** c88 | owner |
| 2026-06-28 | D-IMPACT1 | **impact analyzer rides semantic-index API** (A): build blast-radius queries on top of D-SEMINDEX1, not by duplicating compiler internals. **Ratified — blocked on c96.** c97 | owner |
| 2026-06-28 | D-CODEMOD1 | **codemods are named, reversible objects** (A): refactors become replayable/reversible codemod objects over the semantic index and replay log. **Ratified — blocked on semantic-index/replay foundations.** c98 | owner |
| 2026-06-27 | D-RACEWIN1 | **success race folds into structured-concurrency combinators** (A): try-both/keep-winner is a nursery-level `race`/`any` operation, not a standalone primitive. **Ratified — blocked on D-NURSERY1/c36.** c103 | owner |
| 2026-06-28 | D-UNDOKW1 | **bare `undo` keyword rejected** (A — keep `#Transact` only): reversal stays under the existing transaction mechanism; no new keyword. **Ratified (declined/no build).** c104 | owner |
| 2026-06-28 | D-OOBPROOF1 | **bounds-check proof escape rides refinements** (A): bounds-check elision must be proof-carrying and depends on the refinement-types direction, not an unchecked escape. **Ratified — blocked on D-REFINE1/c25.** c106 | owner |
| 2026-06-28 | D-UNCERTAIN1 | **tracked uncertainty/freshness/precision deferred** (A): do not add one general type dimension now; let honest-number and TTL-secret designs carry their own axes. **Ratified (deferred/no build now).** c107 | owner |
| 2026-06-27 | D-FAILCOMP1 | **failure-aware comprehensions become adapters** (A): use explicit `filter_map`/`try_collect` adapters; no new comprehension syntax. **Implemented/closed in Tower; canonical behavior recorded here.** c108 | owner |
| 2026-06-27 | D-APPROX1 | **approximate algorithms are library-only** (A): approximate/sketch structures live in library APIs, not the language. **Implemented/closed in Tower; canonical behavior recorded here.** c113 | owner |
| 2026-06-27 | D-AUTOPAR1 | **auto-parallelism stays explicit** (A): provide explicit `par_*` adapters; do not secretly parallelize ordinary maps/folds. **Implemented/closed in Tower; canonical behavior recorded here.** c114 | owner |
| 2026-06-27 | D-CADEFS1 | **content-addressed definitions are far-horizon research** (C): no current language/tooling work; capture only. **Ratified (deferred/frozen).** c116 | owner |
| 2026-06-27 | D-A11Y1 | **accessibility by default in UI kit** (A): components ship ARIA/focus/keyboard behavior by default, with release-gated a11y diagnostics. **Ratified — blocked on UI stack/backend/signal gates.** c121 | owner |
| 2026-06-27 | D-NATIVEUI1 | **native UI starts with platform widget FFI, own renderer later** (A): phase 1 wraps AppKit/Win32-or-WinUI/GTK; phase 2 may move to Jet's own renderer. **Ratified — blocked on D-RENDERTGT2.** c122 | owner |
| 2026-06-27 | D-NATIVEUI2 | **native UI targets all desktop platforms together** (B): build macOS, Windows, and Linux backends against the same trait seam instead of one-platform-first. **Ratified — blocked on D-RENDERTGT2.** c122 | owner |
| 2026-06-27 | D-WEBBACKEND1 | **web backend is JS DOM + WASM logic hybrid** (A): view/DOM work emits JS; compute/pure logic can compile to WASM. **Ratified — blocked on D-JSBIND1, D-WEBKIND1, and D-DOMGEN1 web partition details.** c123 | owner |
| 2026-06-27 | D-JSWIFTFFI1 | **JS/npm first, Swift later through C ABI** (A): JS interop attaches to the web backend; Swift interop waits for native UI/C-ABI work. **Ratified — blocked on web/native backend gates.** c124 | owner |
| 2026-06-27 | D-ASYNCRT1 | **Go-scale concurrency uses M:N green threads** (A): keep Jet's task/channel model; blocking-looking calls yield on an M:N scheduler; no `async`/`await` function coloring. **Ratified — implementation gated on scheduler/OS readiness details.** c126 | owner |
| 2026-06-27 | D-USERDERIVE1 | **user-authored derives + typed reflection ship** (A): build the ratified derive/reflection surface within the existing `derive Trait for T` + `T.reflect()` ceiling. **Ratified; derive output done, reflection hardening remains on c129.** c129 | owner |
| 2026-06-27 | D-REACTCORE1 | **reactivity is an explicit `#Reactive` scope marker** (D): no global spreadsheet semantics; the compiler recognizes `#Reactive fn`/`#Reactive {}` and lowers to `core.reactive` tracking. **Ratified — blocked on D-SIGNAL1 API.** c132 | owner |
| 2026-06-27 | D-RENDERTGT1 | **render-target backend trait seam exists before concrete backends** (A): define the backend abstraction before web/native/embedded/TUI implementations. **Ratified — blocked on D-RENDERTGT2 exact trait API.** c133 | owner |
| 2026-06-27 | D-ADAPTRT1 | **adaptive runtime signals deferred** (C): no battery/network/load/carbon adaptive runtime API for now. **Ratified (deferred/frozen).** c136 | owner |
| 2026-06-27 | D-ADAPTFID1 | **adaptive fidelity is a library signal** (A): expose a readable/manual fidelity knob in the library, not a global runtime policy engine. **Implemented/closed in Tower; canonical behavior recorded here.** c137 | owner |
| 2026-06-27 | D-CASTORE1 | **jetpack store identity is content-addressed** (A): package/cache identity is based on content hash, extending the existing cache model. **Implemented/closed in Tower; canonical behavior recorded here.** c139 | owner |
| 2026-06-27 | D-PROCMACRO1 | **procedural macros remain rejected for now** (C): arbitrary AST injection is post-self-host research only. **Ratified (deferred/frozen).** c140 | owner |
| 2026-06-27 | D-READERMACRO1 | **reader macros remain rejected** (A): libraries cannot mutate Jet grammar, sigils, or keywords. **Ratified (declined/frozen).** c141 | owner |
| 2026-06-27 | D-LOGICPROG1 | **full logic programming deferred** (C): no backtracking/multi-answer relation subset now; far-horizon solver research only. **Ratified (deferred/frozen).** c142 | owner |
| 2026-06-27 | D-STRUCTMERGE1 | **structural merge captured as far-horizon tooling** (A): keep the semantic merge ambition, but do not build it now. **Ratified — frozen/research.** c143 | owner |
| 2026-06-27 | D-STRUCTMERGE2 | **structural merge by meaning folds into D-STRUCTMERGE1** (A): no separate duplicate card. **Ratified — consolidated.** c144 | owner |
| 2026-06-27 | D-REVERSE1 | **reversible/constraint solving is UI-layout scoped** (B): pursue a bounded linear constraint solver under `core.layout`, not general reversible computation. **Ratified — blocked on D-LAYOUT1 API surface.** c28 | owner |
| 2026-06-26 | D-PROP1 | **whole-graph effect prohibition** (A — a prohibition marker naming one forbidden effect): a function may forbid a single effect across its entire reachable call graph via a marker that is the exact inverse of D-EFF1's positive effect set, run on the ratified propagation engine — no new machinery. Whitelist-the-allowed-set (B — must restate every permitted effect, reads oddly to ban one thing) and lean-on-`#()` (C — all-or-nothing, can't express "no Net while Io stays fine") rejected. **Ratified — not yet implemented** (sema inverse-propagation pass + E-code + ui snapshot + example). c18 | owner |
| 2026-06-26 | D-ROLE1 | **temporal state ordering falls out of typestate** (A — no new surface): the legal order of a value's states is already pinned by D-STATE1's `#Transition(A -> B)` edges — the only path to a late state runs through the earlier ones — so no dedicated ordering surface is added. Declared edge-list in the `state` block (B — states each edge twice) and a `#timeline` happy-path annotation (C — second place to keep in sync, drifts) rejected. **Ratified (confirmed — no new surface)** (ordering is emergent from existing typestate edges). c19 | owner |
| 2026-06-26 | D-PROTO1 | **session/protocol declaration + stub generation** (A — declare the exchange once, generate both handles): a user writes the ordered request/response exchange once and the compiler generates `Handshake.Client`/`Handshake.Server` handle types, each step consuming and returning the handle on the ratified linear (D-LIN1) + typestate (D-STATE1) machinery, so out-of-order sends are caught at compile time. Hand-written `#SingleUse` + `#Transition` duals (B — the boilerplate a protocol decl exists to erase) and comptime-read-a-value (C — buries the sequence in a struct literal) rejected. **Ratified — not yet implemented** (protocol-decl parser + handle codegen over D-LIN1/D-STATE1 + example). c20 | owner |
| 2026-06-26 | D-QUAL4 | **type-position marker is prefix** (A — `#Tainted String`): a value-tag decorating a type in a signature sits before the type, matching every existing Jet marker (`#Pure fn`, `#Unsafe`, `#SingleUse struct`) — one mental rule for markers everywhere. Postfix `String #Tainted` (B — points the opposite way from every other `#`-marker, two rules for where a marker lives) rejected. **Implemented 2026-06-26** (`Type::Tagged { marker, inner }` in AST; parser `#PascalIdent T` → Tagged; transparent to PartialEq/is_numeric/is_scalar/codegen; fmt emits `#Marker T`; `Syntax.rs` note; fmt stability test; ui snapshot `tests/ui/value_tag_type`; example `131_value_tag_type.jet`). c21 | owner |
| 2026-06-26 | D-SERDE-ACCESS | **fluent accessors over dynamic Data trees** (B — small `?`-chaining accessor set): reading an untyped `Data`/`Json` tree gains `.field(name)`, `.at(i)`, and leaf coercions `.int()`/`.text()`/`.bool()`/`.float()` — each returning a `Result` so `?` chains cleanly — atop the shipped pattern-match floor. Match-only status quo (A — a tower of nested matches to reach one field) and a path mini-language (C — a query DSL in string literals, a second grammar, I8 collision) rejected. **Implemented 2026-06-26** (accessor methods on `Json`/`Data` and `DataTree` in CoreLib.rs; sema: `datatree_method_return` + routing in `infer_method_call`; TIR: `JsonField`/`DataTreeField` etc. in `THandleOp`; golden example 133). c22 | owner |
| 2026-06-26 | D-REPLAY1 | **deterministic-replay opt-in marker** (A — a `#Replayable` marker proving no hidden non-determinism): a function marker rejects any reachable `#(Time/Rand/Net/Io)` not routed through a mockable capability, via the same inverse-propagation walk as D-PROP1, so replay-soundness is proven statically and cannot silently rot. Library-by-convention (B — one forgotten `clock.now()` silently breaks replay, no compile error) rejected; the record/replay runtime harness (C's second half) is separable post-v1 build work, not decided here. **Ratified — not yet implemented** (sema replay-soundness pass + marker in Syntax.rs + E-code + ui snapshot). c23 | owner |
| 2026-06-26 | D-BINDEXPLICIT1 | **explicit-type binding: mutability marker hugs the name** (A — `name@ Type =  val` / `name: Type =  val`): the explicit-typed binding collapses to one marker fused to the name (`@`=immutable, `:`=mutable), a bare type, and a plain `=` that binds-or-reassigns depending on the name's marker (the Go/Odin/Pascal `:=` family). The inferred `name @= val` / `name := val` forms are unchanged. Amends **S4** (annotation position) and reopens **D-BIND2**'s `=`-is-reassignment-only invariant. Status-quo two-marker form (B — type with `:`, mutability with `@=`/`:=`) rejected. **Implemented 2026-06-26** (parser: `sigil_binding` handles `name@ Type = val` and `name: Type = val`; formatter: `fmt_binding` emits new form; E0988 teaching error for old prefix label form; Syntax.rs; migrated all explicit-typed bindings in examples; re-blessed snapshots). c29 | owner |
| 2026-06-26 | D-LOOPLABEL2 | **loop labels become a suffix `outer@`** (A — suffix at declaration and at break/continue): the loop-label `@` moves from prefix to suffix everywhere it appears — `outer@ loop { break outer@ }` — matching the binding-card marker-suffixes-the-name direction. Reverses **D-ATTR3** (which deliberately kept the `@` prefix with the mixed-sigil trap flagged) and amends **D-LABEL1**. Codegen still maps to Rust `'name:` labels. Status-quo prefix `@outer` (B) rejected. **Implemented 2026-06-26** (parser: `name@ loop` suffix declaration, `break name@`/`continue name@`; E0988 teaching error for old `@name loop`/`break @name` prefix form; formatter; Syntax.rs; migrated all labeled loops in examples; re-blessed snapshots). c30 | owner |
| 2026-06-26 | D-MATCHARM1 + D-MATCHARM2 | **richer match-arm patterns** (D-MATCHARM1=A — `\|` alternates values, `\|\|`/`&&` combine with booleans, parens group; D-MATCHARM2=B — `\|` binds tighter, parens required on mixed heads, E0328 teaching error): a single `\|` takes over value-alternation (`400 \| 404`), `\|\|`/`&&` become boolean combinators, predicate arms (`code >= 500`) now valid, and mixing `\|` with `&&`/`\|\|` without parens is E0328. Reopens **S25** (retired — the old `\|\|`-distributes arm syntax is migrated). **Implemented 2026-06-28** (arm-head grammar: `parse_arm_value_cond` / `parse_arm_and_cond` / `parse_arm_alternates_cond`; `arm_head_term` flag in parser; E0328 teaching error + `tests/ui/matcharm_mixing_needs_parens` snapshot; E0993 retired; formatter: `is_all_subject_alts` + `fmt_arm_alternates`; `examples/features/07_switch.jet` migrated to `\|` syntax). c31 | owner |
| 2026-06-26 | D-ENUMDOT1 | **leading dot on enum-variant patterns** (A — `.Circle(r)` in match arms): variant patterns in arms take a leading `.`, matching dot-construction and signalling inferred-enum-member resolution, also resolving S31's bare-name-vs-variable ambiguity. Amends **S30/S31**. Status-quo bare variant (B — a name shadowing an in-scope variable must be qualified `Light.Red`) rejected. Whether the leading dot also applies to value-position enum access is the open follow-on **D-ENUMDOT2**. **Implemented 2026-06-26** (parser: `try_pattern_rhs` + switch arm head; formatter: `fmt_pattern` emits leading `.`; Syntax.rs note; bare form still accepted). c32 | owner |
| 2026-06-26 | D-IMPLDOT1 | **impl/forwarding trait separator becomes `.`** (A — `impl Type.Trait`): the trait attaches to the type with `.` for both plain impls (`impl FileErr.Fallible`) and `using` forwarding (`impl App.Logger using logger`), reading "Type's Trait". Reopens **S28** (which explicitly rejected `impl Type.Trait` and chose `:`) and amends **S62**, retiring the **S83**-reserved `~~` trait-attach direction. Status-quo `:` separator (B) rejected. **Implemented 2026-06-26. c33 | owner |
| 2026-06-26 | D-MARKERCASE1 | **`#Grant`/`#Layout` are PascalCase** (A — confirm, lowercase is drift): rubber-stamps **D-CASING1**'s blanket "every `#`-marker is PascalCase" rule for two markers whose lowercase spellings (`#Grant` in D-SCAP1's impl note, `#Layout` throughout D-SOA1/2) are pre-existing drift, not a new rule. Intentional-lowercase-exception (B — a carve-out from D-CASING1) rejected. **Implemented 2026-06-26.** c34 | owner |
| 2026-06-26 | D-TESTPAREN1 | **test name as a parenthesized argument** (A — `#Test("name") { }`): a named test writes its name as a normal marker argument in parens, consistent with the argument-carrying marker family. Amends **S43**. The `#Test fn` property form (D-TEST1) is unchanged — it draws its name from the function, so there is no string to wrap. Status-quo bare adjacent string `#Test("name") { }` (B) rejected. **Implemented 2026-06-26.** c35 | owner |
| 2026-06-26 | D-CONCCOMB1 | **Verse-style structured task combinators** (A — `race`/`all`/`any` as first-class core combinators): adopt race (cancels losers), all (waits all, fails fast), any (first Ok, waits rest) over the S53 task primitives. Library-only `core.tasks` (B — worse beginner experience, combinators end up there anyway unblessed) and defer entirely (C — roadmap gap) rejected. The task/channel primitives themselves (D-NURSERY1) are still open and the memory-capability model review the owner required is a prerequisite. **Ratified — gated on D-NURSERY1** (the concurrency primitives); combinator semantics land once the primitives + memory-capability model settle. c36 | owner |
| 2026-06-26 | D-IGNORERET1 | **explicit discard of a fallible result** (B — a visible mandatory discard sigil): a caller may silence a `#MustUse`/`T ? E` result only through a visible, deliberate discard sigil at the call site, with sema emitting a lint (not an error) pointing at what was dropped — the footgun stays opt-in and visible (I1 holds). Status-quo mandatory-handling (A — `?? ()` only) rejected as too rigid; Jai-style implicit ignore (C — silent error loss, the I1 footgun beginners must never hit) rejected. **Ratified — not yet implemented** (parser discard sigil + sema `#MustUse` lint + W-code + ui snapshot). c37 | owner |
| 2026-06-26 | D-VECARITH1 | **lane-wise arithmetic stays closed to built-ins** (A — confirm, no user opt-in): lane-wise `+`/`-`/`*`/`/` stays on the compiler-provided types (`F32x4`/`F64x2`, `Vec2`/`Vec3`/`Vec4`, `Mat*`); user structs use method calls. Confirms D-SIMD2's closure. A `#ComponentWise` derive for user structs (B — a new derivable surface, surprising results) and free `impl Add for MyType` (C — operator soup, the exact problem I8 prevents) rejected. **Ratified (confirmed — no new surface)** (the closed lane-wise set is already shipped; no expert escape added). c42 | owner |
| 2026-06-26 | D-TSSWIFT1 | **TS/Swift competitive lens stays in the roadmap** (B — no standalone doc): the replace-TypeScript/Swift gap analysis stays folded into roadmap milestone descriptions rather than a separate maintained doc. A standalone one-page gap table (A — another doc to keep current) rejected. **Ratified (declined — no new surface)** (the competitive lens lives in the roadmap, not a new file). c43 | owner |
| 2026-06-26 | D-ASSIGNCOND1 | **assignment-in-condition teaching diagnostic** (A — grammar-reject `=` in condition + dedicated what/why/fix): confirm (or add) grammar rejection of `if x = 5` and wire a dedicated diagnostic ("looks like assignment in a condition; did you mean `==`?") with a code and `tests/ui` snapshot, the I4-compliant path. Do-nothing/raw-parse-error (B — against I4 and beginner-first) rejected. **Implemented 2026-06-28** (E0322 parser condition-position guard + `tests/ui/assign_in_condition` snapshot). c47 | owner |
| 2026-06-26 | D-SMELLLINT1 | **semantic-smell lints** (A — float `==` + duplicate branch default-on, constant condition opt-in): ship high-signal lints — comparing floats with `==` and duplicate match/if branches default-on, always-true/false conditions opt-in (legitimate in comptime-adjacent code) — each with a W-code, message, fix, and ui snapshot (I4); split-shipping allowed. All-opt-in under a `#lint(smell)` profile (B — lower beginner value) and build-infra-first then defer (C — same bugs keep shipping) rejected. **Ratified — not yet implemented** (sema AST lint passes + W-codes + ui snapshots). c48 | owner |
| 2026-06-26 | D-CONFUSE1 | **confusable-name lint** (A — homoglyphs default-on, plural/singular opt-in): a lint fires at declaration when a new identifier is confusable with one already in scope — homoglyphs (`l`/`1`/`I`, `O`/`0`) default-on (almost never intentional), plural/singular near-names (`user`/`users`) opt-in (frequently legitimate); two codes, two snapshots. No-default-warnings (B — misses the beginner safety net) and defer-until-lint-profiles-land (C — a real cost to beginners) rejected. **Implemented 2026-06-26** (L0503 homoglyph lint in `declare()` in CheckerCore.rs; wave-1 is homoglyphs-only — `l`/`I`/`1` and `O`/`0`; snapshot `tests/ui_lint/confusable_name`; `diagnostics.md` entry; L0504 plural/singular is opt-in and planned for a subsequent wave). c49 | owner |
| 2026-06-26 | D-DISPLAYDBG1 | **Display vs Debug rendering split** (A — two blessed protocol hooks): add `Display` (user-facing, explicit impl, no default) and `Debug` (developer-facing, auto-derived like Rust) as two distinct hooks; interpolation uses `{}` for Display and the D-DISPLAYDBG2 selector for Debug. Two distinct jobs, so no I8 conflict. One-hook (B — forces one representation serving both) and auto-Debug-only-no-redaction (C — Debug can't redact sensitive fields) rejected. **Ratified — not yet implemented** (Display/Debug hooks + Debug auto-derive + interpolation surface + golden example). c51 | owner |
| 2026-06-26 | D-TIMEDEPTH1 | **full civil-time `core.time` in Core** (A — Date/DateTime/Duration/Zone, IANA-backed): `core.time` grows to cover civil dates, durations, calendar math, and time zones via the IANA tz database (tz data from the host OS on Nix, bundled fallback with a clear error otherwise), layered on the existing injectable Clock. Instants-only-in-Core + civil-in-a-package (B — splits a naturally unified API, two imports) and status-quo Clock/Instant only (C — significant omission for a general-purpose language) rejected. **Ratified — not yet implemented** (`core.time` Core module + tz-data sourcing + golden examples). c53 | owner |
| 2026-06-26 | D-COLLBREADTH1 | **Set and Deque in Core collections** (A — `Set<T: Hash + Eq>` + ring-buffer `Deque<T>`): add hash-backed `Set<T>` and a ring-buffer-backed `Deque<T>` to `core.collections`; `OrderedSet<T: Ord>` is a follow-on, the ring buffer is the Deque backing not a separate type (I8). Set-now-Deque-later (B — BFS/sliding-window patterns still need workarounds) and ship-as-packages (C — fragments a unified collections story, can't integrate with type inference) rejected. **Ratified — implemented 2026-06-26** (`Set<T>`/`Deque<T>` types, methods, codegen, golden examples 136_set + 137_deque, E0506 UI snapshot). c55 | owner |
| 2026-06-26 | D-UUIDENC1 | **UUID + hex/base64 codecs in Core** (A — `core.uuid` + `core.encoding`): ship `core.encoding` (hex, base64) and `core.uuid` (v4 via system CSPRNG, v7 via the injectable Clock) in Core; the CSPRNG is system-sourced with an injectable interface for deterministic tests. Hex/base64-Core-but-UUID-package (B — a two-import pattern for the common case) and FFI/community-only (C — generating an ID should not need unsafe glue) rejected. **Implemented 2026-06-26** (`core.encoding.hex`/`core.encoding.base64` encode/decode, `core.uuid.v4`/`v7`; pure std, zero deps; golden examples 134\_hex\_base64 + 135\_uuid). c56 | owner |
| 2026-06-26 | D-DECIMAL1 | **exact Decimal type + float-money lint** (A — `Decimal` in Core + default-on `E-FLOATMONEY`): ship an arbitrary-precision base-10 `Decimal` in `core.numeric` plus a default-on lint that fires when a money-named field (price/cost/amount/total/fee/balance/tax) holds a float, suppressible with `#[allow(float_money)]`. Decimal-as-a-package no-lint (B — the footgun stays the silent default) and lint-only-no-Decimal (C — nowhere first-class to point the user) rejected. The lint is buildable now; the `Decimal` type needs a BigInt backing and is **gated on card #65** (the BigInt decision, still open). **Ratified — gated on c65** (Decimal's bigint backing); the float-money lint (W-code + ui snapshot) ships independently. c57 | owner |
| 2026-06-26 | D-PATHFS1 | **typed Path API in Core** (A — `core.fs.Path` + atomic write + dir-walk): ship a Core `Path` type (`from`, `join`, `parent`, `extension`, `stem` as pure methods), `write_atomic()` (temp-file-then-rename), and `walk()` (lazy iterator with symlink-cycle detection); the thin raw-string `path`/`list_dir` helpers become teaching errors pointing to `Path` (I8: one way to work with paths). Functions-on-str (B — platform separator bugs persist, no type-level path/string distinction) and raw-strings status quo (C — leaves separator + torn-write footguns) rejected. **Ratified — not yet implemented** (Core `core.fs.Path` + atomic write + walk + teaching errors + golden example). c58 | owner |
| 2026-06-26 | D-OWNCOMP1 | **copy-in-and-own UI component distribution** (B — `jetpack add` copies source the user owns): UI components are copied into the user's tree and owned outright (shadcn-style), no version lock; patches don't auto-propagate (user re-runs add or diffs). Fits the ownership ethos and I8 (one distribution path). Block-until-the-stack-is-ratified (A) and locked versioned dependency (C — conflicts with the ownership ethos) rejected. The reactive-UI stack itself (**D-REACTUI1**) is still open and gates what these components target. **Ratified — gated on D-REACTUI1** (the reactive-UI stack); the `jetpack add` copy mechanism lands once the stack is chosen. c61 | owner |
| 2026-06-26 | D-DEFERKW1 | **no `defer` keyword** (B — `core.scope.guard` stays canonical): decline the `defer` keyword; the existing `core.scope.guard` remains the one scope-exit cleanup path, so no new parse surface and no I8 violation (the ergonomic gap is one extra let-binding). Adding `defer` as canonical with `core.scope.guard` deprecated (A) rejected. **Ratified (declined — no new surface)** (`core.scope.guard` is the sole cleanup mechanism). c62 | owner |
| 2026-06-26 | D-CRYPTOENV1 | **misuse-resistant crypto envelope** (A — `seal`/`open` + `sign`/`verify` as the blessed default): ship a high-level envelope (libsodium model) that hides nonce/IV/mode selection; raw AES/ChaCha/RSA primitives require an explicit expert import, making misuse a deliberate opt-in (I1). Raw-primitives-direct (B — maximum footgun surface) and hold-for-post-quantum-agility (C — delays a useful safe-by-default API) rejected. **Ratified — not yet implemented** (Core `crypto.seal`/`open`/`sign`/`verify` over the ratified CSPRNG + golden example). c64 | owner |
| 2026-06-26 | D-HONESTNUM1 | **measurement-uncertainty type as a package** (A — `core.science.measurement`, no language change): a `Measurement<T>` library type carrying `±` error-bound propagation ships as a first-party package, not Core — a scientific niche that doesn't warrant a Core footprint (I8: one numeric type per job). Add to core std (B — a type most programs never need, I8 risk vs a future units story) and align-with-a-tracked-uncertainty-card (C) rejected. **Ratified — not yet implemented** (first-party `core.science.measurement` package; pure library, no compiler change). c68 | owner |
| 2026-06-26 | D-OPTGC1 | **opt-in GC library** (A — `Gc<T>` traced handle): ship a `gc` stdlib module with a `Gc<T>` smart pointer whose backing allocation is traced and collected on a side thread, for cyclic heap data (graphs, back-edges); ownership stays the default, `Gc<T>` is a deliberate expert import and the library itself is safe Jet — no sema/codegen/default-allocation changes. Status-quo arenas-only (B) and decide-later (C) rejected. The backing Rust GC crate is a new I6 exemption requiring **owner approval** (a separate ballot). **Ratified — not yet implemented** (gated on the backing-crate I6 ballot; `gc` module + `Gc<T>` once approved). c69 | owner |
| 2026-06-26 | D-SELIMPORT1 | **grouped + aliased selective imports** (A — `use mod.{a, b as c}`): add `use math.{sin, cos}` (bring items into scope) and `use math.{sin as s}` (alias); single-item `use math.sin` is the degenerate case. Additive to the module-qualified path style (no glob reopen — D-MOD2's E0612 stands); one mechanism (selective bring-into-scope + optional rename), no I8 conflict. Single-item-only no-grouping-no-alias (B — multiple `use` lines, no aliases) and qualify-everything (C — verbose at call sites) rejected. **Implemented 2026-06-26** (`ImportKind::Unqualified.items` extended to `Vec<(String, Option<String>)>`; parser parses `as alias` after each item in `{…}` and after single-item `use mod.item as alias`; sema binds the local alias; codegen Imports.rs uses alias as the lookup key; formatter emits `name as alias`; fmt stability test; ui snapshot `selective_import_alias`; example `132_selective_imports.jet`). c72 | owner |
| 2026-06-26 | D-NAMESPACE1 | **no `namespace { }` spelling** (A — keep `module name { }` only): decline a distinct `namespace { }` keyword; inline `module name { }` already groups items into a named in-file scope, so a second spelling is an I8 violation with no expressiveness gain (a confusing word is a docs problem, not a language one). Both-keywords (B — I8 violation) and reserve-`module`-for-files-only-use-`namespace`-inline (C — breaks existing code) rejected. **Ratified (declined — no new surface)** (`module name { }` is the sole in-file grouping spelling). c75 | owner |
| 2026-06-26 | D-CORENS1 | **single `core.*` first-party namespace** (owner-directed): every first-party library — whether a built-in compiler module (`core.fs`/`core.mem`/…) or a ring package (`core.http`/`core.regex`/`core.linalg`/…) — is spelled `core.<name>`. The `jet.*` ring namespace and the `jet.core` long-form canonical spelling are **retired**; there is no `jet.*` or `std.*` library namespace. Amends S51 and the D-CASING1 ring-package note (which had kept `jet.*` for the ring). Docs + Tower migrated 2026-06-26. **Implemented 2026-06-27** (KNOWN_CORE_MODULES updated to `core.*`; normalize_core_module maps `core.<ring>` → `jet.<ring>` internal key; E0341 teaching error for old `jet.<ring>` spelling with tests/ui snapshot; all ~14 ring examples + tests migrated to `core.*`; goldens re-blessed). | owner |
| 2026-06-28 | D-STDRUBRIC1 | **stdlib ergonomic-law checklist proceeds** (A): create a Core/std API review rubric for naming, fallibility, ownership, allocation, docs, and examples before broad API expansion. **Ratified — planning/doc work pending.** c44 | owner |
| 2026-06-28 | D-FORMALCORE1 | **formal core deferred to Epoch 6** (C): keep a tiny desugaring/formal-core map as far-horizon verification infrastructure, not current compiler scope. **Ratified — deferred/no build now.** c54 | owner |
| 2026-06-28 | D-TTLVAL1 | **TTL/rotting values proceed as typed library capability** (A): expiring secrets/values get a first-class typed API instead of ad hoc timestamp fields. **Ratified — not yet implemented.** c59 | owner |
| 2026-06-28 | D-JETDOC1 | **jetdoc rides semantic graph** (B): generated docs are built from the semantic index/graph rather than parser text alone. **Ratified — blocked on semantic-index work.** c86 | owner |
| 2026-06-28 | D-REFRELOOK1 | **stored-ref relook closes with follow-up ballot** (E): original concern was recovered and narrowed; the real remaining gate is labeled ref-field syntax/safety (D-REFSTRUCT1). **Ratified — follow-up opened; no build on this recovery card.** c100 | owner |
| 2026-06-28 | D-COROUTINE1 | **coroutines as primitives proceed** (A): suspend/resume primitives stay uncolored by async function syntax and compose with the task/runtime model. **Ratified — not yet implemented.** c101 | owner |
| 2026-06-28 | D-SELFVER1 | **self-versioning values proceed** (A): values may carry version/conversion history through a typed library/tooling surface. **Ratified — not yet implemented.** c105 | owner |
| 2026-06-28 | D-DEADLINE1 | **deadline propagation rides context/taskgroups** (A): deadlines/cancellation flow through `#Context` and structured taskgroups instead of globals. **Ratified — blocked on structured taskgroup implementation.** c112 | owner |
| 2026-06-28 | D-ASKCODE1 | **ask-your-codebase query engine rides semantic index** (B): codebase questions are tooling queries over semantic facts, not language syntax. **Ratified — blocked on semantic-index work.** c115 | owner |
| 2026-06-28 | D-TYPEDSTYLE1 | **typed style values proceed** (A): UI style properties/units become typed library values once the reactive UI stack exists. **Ratified — blocked on UI stack.** c120 | owner |
| 2026-06-28 | D-MOTION1 | **motion is reactive state** (A): animation derives from reactive state and timing signals rather than imperative frame mutation. **Ratified — blocked on reactive UI/runtime foundations.** c135 | owner |
| 2026-06-28 | D-CARBON1 | **carbon/battery runtime policy deferred** (C): no runtime-level adaptive carbon/battery policy now; keep as far-horizon research. **Ratified — deferred/no build now.** c138 | owner |
| 2026-06-28 | D-REPLAY2 | **record/replay runtime harness proceeds** (A): deterministic replay gets an opt-in runtime harness layered on D-REPLAY1 soundness. **Ratified — not yet implemented.** c23 | owner |
| 2026-06-28 | D-DEP-GC1 | **GC backend uses a pure Rust mark-sweep implementation** (A): approve the safe in-tree Rust collector path for the opt-in `Gc<T>` library rather than an external GC crate. **Ratified — unblocks c69 implementation.** c69 | owner |
| 2026-06-28 | D-DEP-CRYPTO1 | **crypto backend dependency approved for envelope work** (A): use the approved backend path for `core.crypto` envelope primitives. **Ratified — unblocks c64 implementation.** c64 | owner |
| 2026-06-28 | D-BUDGET1 | **budgets are explicit effect/loop profiles** (B): `#Budget` / budget profiles constrain transitive effects and bounded loops for latency-sensitive code. **Ratified — not yet implemented.** c26 | owner |
| 2026-06-28 | D-REFLECT1 | **reflection read API proceeds** (A): expose type/member/marker facts through the ratified typed reflection surface. **Ratified — partly overlaps D-METAREFLECT1; hardening remains.** c130 | owner |
| 2026-06-28 | D-MNIO1 | **Go-scale networking uses task runtime** (A): keep blocking-looking APIs over M:N tasks instead of async/await coloring. **Ratified — runtime implementation pending.** c126 | owner |
| 2026-06-28 | D-CONCSELECT1 | **scoped select is fluent on taskgroups** (A): `g.select().recv(...).read(...).after(...).wait()?` is the chosen select surface. **Ratified — blocked on taskgroups/channels.** c36 | owner |
| 2026-06-28 | D-TASKSCOPE1 | **structured taskgroup scope** (A): task spawning is scoped by a nursery/taskgroup that owns child handles and cancellation. **Ratified — not yet implemented.** c102 | owner |
| 2026-06-28 | D-WORKSPACELOCK1 | **workspace lock stays unified under `.jet/lock`** (A): monorepo workspace resolution shares the existing lock path instead of adding `.jet/workspace.lock`. **Ratified — c90 implementation pending.** c90 | owner |
| 2026-06-28 | D-TOWERRETIRE1 | **Tower v1/v2 retirement audit proceeds as archive-then-delete** (C): identify remaining v1 files, migrate anything still needed into v2, then retire v1. **Ratified — planning/audit work pending.** c148 | owner |
| 2026-06-28 | D-FALLTHROUGH1 | **no switch/case fallthrough** (A): shared cases use grouped arms (`400 | 404 -> ...`); hidden fallthrough is not added. **Ratified (declined/no build).** c151 | owner |
| 2026-06-28 | D-RINGLAYER1 | **runtime layers infer by default with expert ceilings** (A): the compiler infers a package's minimum `core`/`alloc`/`std` layer from imports, while experts may set an optional `layer: core|alloc|std` ceiling in package metadata; imports above the ceiling are errors. The capability axis is called `layer`, not `ring`, avoiding the first-party-package terminology collision. **Ratified — not yet implemented.** c118 | owner |
| 2026-06-28 | D-DISPLAYDBG2 | **Debug interpolation selector is `@Debug` suffix** (A, owner amendment): bare `{value}` uses Display; `{value@Debug}` uses Debug. The owner rejected `:Debug` and selected the suffix form to keep selectors visibly Jet-owned without creating a Rust-style format mini-language. Unknown selectors are compile errors listing the closed set. **Ratified — not yet implemented; amends D-DISPLAYDBG1's interpolation spelling.** c51 | owner |
| 2026-06-28 | D-CFFI-SYNTAX-REOPEN | **keep C FFI model but spell binding blocks `#Extern module`** (A, owner amendment): do not add `#Lang_C`; keep the S59 C FFI model (`use c.<lib> as alias`, C binding module, same safety gates), but update the visible marker from `@extern module` to Jet-canonical `#Extern module`. `@extern` becomes retired teaching syntax. **Ratified — not yet implemented.** c149 | owner |
| 2026-06-28 | D-LOOP-SURFACE-REOPEN | **add semicolon counted-loop headers under `loop`** (B): keep `loop` as the single loop keyword, but allow a compact counted-loop header `loop init; condition; afterthought { ... }` for cases where `loop i in range step n` is not expressive enough. Semicolons are legal only inside this header, not general statement separators. **Ratified — not yet implemented.** c150 | owner |
| 2026-06-28 | D-DYNAMIC-TYPE1 | **closed union type syntax proceeds** (B): add a compact closed union type form such as `Int | Float | String` for short-lived fixed alternatives. General `Any` stays rejected; dynamic trees/interop remain separate mechanisms. Requires parser/sema/pattern-matching design to avoid drifting into a second enum mechanism. **Ratified — not yet implemented.** c152 | owner |
| 2026-06-28 | D-ORRETURN-ERG1 | **unify local fallible exits under `?? <control>`** (B): keep `??` as Jet's fallback operator and complete the local-control family so loop skip is `expr ?? continue`, matching existing `expr ?? return value` and `expr ?? break`. This supersedes the special `?continue` spelling. Odin-style `or_*` keywords stay rejected except as possible teaching diagnostics. **Ratified — not yet implemented; amends S81/S71.** c153 | owner |
| 2026-06-28 | D-HOLE1 | **no general hole type; use Option combinators** (A): typed holes do not enter the language surface. Optional composition gets library helpers (`map`, `and_then`, `zip`, `lift2`, `lift3`) instead of a new bottom-like placeholder type. **Ratified — library work pending.** c109 | owner |
| 2026-06-28 | D-PROVENANCE1 | **provenance/debug history deferred to Epoch 6** (C): value provenance is a far-horizon tooling/debug capability, not current language or runtime surface. **Ratified — deferred/no build now.** c110 | owner |
| 2026-06-28 | D-TIMETRAVEL1 | **time-travel debugging deferred to Epoch 6** (C): no reversible execution/runtime timeline API now; preserve the idea as later tooling after replay/runtime foundations mature. **Ratified — deferred/no build now.** c111 | owner |
| 2026-06-28 | D-IGNORERET2 | **explicit discard is `.drop("reason")` plus scoped suppressions** (A): a `#MustUse` value is intentionally ignored with a method-style `.drop("reason")`; broader generated-code or wrapper cases use a visible `#Suppress(MustUse)` marker. Silent ignore remains rejected. **Ratified — not yet implemented; supersedes D-IGNORERET1's unspecified discard sigil.** c37 | owner |
| 2026-06-28 | D-VERIFY1 | **contracts plus finite-domain `#Verify`** (A): add `require`/`ensure` contract checks and a `#Verify` marker for bounded, finite-domain proof attempts. Verification remains sema-owned; rustc is not used as a checker. **Ratified — not yet implemented.** c27 | owner |
| 2026-06-28 | D-WASM1 | **browser target is an inferred `Browser` effect plus module-level `#Target`** (A): web-capable code is classified by effects, with DOM/view code on JS and pure/compute code eligible for WASM under explicit module target markers. **Ratified — downstream web ABI/runtime decisions remain in D-JSBIND1, D-WEBKIND1, and D-DOMGEN1.** c123 | owner |
| 2026-06-28 | D-REFSTRUCT1 | **stored reference fields use `#Ref(Label)`** (B, owner amendment): a stored reference field is marked `#Ref(arena) field: T`; sema proves the named owner relationship and no `use core.mem` gate is required. The older bracket spelling is rejected. **Ratified — not yet implemented.** c147 | owner |
| 2026-06-28 | D-GENMOD2 | **generic modules use unified `<...>` parameters** (A): type and value module parameters share angle brackets; annotation kind distinguishes type bounds (`K: Hash`) from value parameters (`capacity: Int`), and instantiation mirrors declaration (`Lru<String, 32>`). Supersedes D-GENMOD1's placeholder paren syntax. **Ratified — not yet implemented.** c91 | owner |
| 2026-06-28 | D-NURSERY1 | **scoped task groups are canonical spawn** (A): task creation lives inside a lexical taskgroup/nursery over the M:N scheduler; every child must finish, cancel, or report before scope exit. Detached spawn is an expert escape hatch. **Ratified — exact API spelling handled by D-TASKSCOPE1.** c102 | owner |
| 2026-06-28 | D-VISDEFAULT1 | **file-scope public-default marker is approved** (C): keep private-by-default globally, but allow an explicit file-level marker that flips following items to public-by-default for API-heavy files. **Ratified — exact spelling handled by D-VISDEFAULT2.** c127 | owner |
| 2026-06-28 | D-VISDEFAULT2 | **file-scope marker is `#PubFile` with `priv` exceptions** (A): `#PubFile` flips a file to public-by-default; `priv` marks exceptions inside that file. **Ratified — not yet implemented; registers `priv` as user-typeable syntax.** c127 | owner |
| 2026-06-28 | D-REACTUI1 | **reactive UI stack is strategic and layered** (A): build reactivity, view model, typed styles, headless/styled components, motion, and app kit in order after the reactivity and render-target gates. **Ratified — gated on D-REACTCORE1/D-RENDERTGT1 follow-through.** c134 | owner |
| 2026-06-28 | D-MATCHARM2 | **mixed match-arm heads require parens** (B): `|` value alternation binds tighter than `&&`/`||`, and any mixed head must parenthesize the alternation or boolean grouping. **Implemented 2026-06-28 with D-MATCHARM1.** c31 | owner |
| 2026-06-28 | D-ENUMDOT2 | **leading enum dot extends to value position** (A): `.Red` is allowed anywhere an expected enum type is known; `Color.Red` remains valid everywhere. **Ratified — not yet implemented.** c145 | owner |
| 2026-06-28 | D-REFINE1 | **refinements extend `distinct` with `#Invariant` and pure-Rust LIA** (A): no new `refine` keyword; invariants attach to `distinct` wrappers, smart constructors validate, and a pure-Rust linear integer arithmetic prover handles preservation and bounds proofs where possible. **Ratified — not yet implemented.** c25 | owner |
| 2026-06-28 | D-IFC1 | **taint labels use a closed `#Tainted(.Kind)` taxonomy** (A): one taint engine tracks `.Input`, `.PII`, `.Secret`, and `.Credential`; bare `#Tainted` becomes sugar for `.Input`. Declassify syntax, sanitizer specificity, sink declarations, and interprocedural scope remain follow-on ballots before implementation. **Ratified — blocked on those follow-up ballots.** c24 | owner |
| 2026-06-28 | D-LAYOUT1 | **constraint layout uses a `layout {}` block with typed-axis variables** (A): `layout {}` desugars to first-class `Constraint` handles over axis-typed variables; closed-type comparison operators may produce constraints for these layout types. If either language gate is later declined, method-only builders are the fallback. **Ratified — not yet implemented.** c28 | owner |
| 2026-06-28 | D-RENDERTGT2 | **render backend trait is measure-layout-paint** (A): platform backends implement `measure`, `layout`, `paint`, and a separate event entry point, giving UI code one portable backend seam. **Ratified — not yet implemented.** c133 | owner |
| 2026-06-28 | D-SIGNAL1 | **reactive primitives are `Signal<T>`, `Computed<T>`, and `Effect`** (A): `#Reactive` lowers onto a small explicit library vocabulary rather than creating a second language. **Ratified — not yet implemented.** c132 | owner |
| 2026-06-29 | D-WEBKIND1 | **first browser WASM target is `wasm32-unknown-unknown` plus generated JS loader** (A): the web backend emits browser-focused WASM and a JS loader/runtime seam first; WASI/component targets are not the first shipping browser contract. **Ratified — unblocks c123 web-backend planning.** c123 | owner |
| 2026-06-29 | D-DOMGEN1 | **generated JS uses a tiny first-party DOM runtime shim** (A): web codegen calls a stable `JetDom`-style shim for create/update/event wiring instead of raw repeated DOM boilerplate or a full virtual DOM runtime. **Ratified — unblocks c123 web-backend planning.** c123 | owner |
| 2026-06-29 | D-NPMTYPE1 | **npm interop uses first-party typed Jet stub packages** (A): typed npm bindings are authored as reviewed Jet packages; direct `.d.ts` parsing and untyped dynamic npm imports are rejected as the default path. **Ratified — unblocks c124 planning.** c124 | owner |
| 2026-06-29 | D-S14-PAUSE | **old/foreign syntax teaching is paused until post-Epoch 6** (A): delete old Jet and other-language teaching fixtures/snapshots/docs now; retired spellings get ordinary syntax errors until a post-Epoch 6 migration-teaching pass explicitly reintroduces targeted diagnostics. **Ratified — cleanup pending.** c154 | owner |
| 2026-06-29 | D-RECONCILE-SCOPE1 | **syntax reconciliation is a strict repo-wide purge** (A): remove stale/foreign syntax from docs, examples, tests, snapshots, diagnostics, parser recovery, syntax ledgers, comments, and generated teaching fixtures unless explicitly allowlisted. **Ratified — cleanup pending.** c154 | owner |
| 2026-06-29 | D-CANON-SOURCE1 | **canonical syntax truth is `Syntax.rs` plus the ratified decision log, CI-checked** (A): user-typeable forms live in `Syntax.rs` with decision IDs, mirrored by this log; reconciliation should add forbidden-spelling checks so stale syntax cannot drift back in. **Ratified — cleanup/tooling pending.** c154 | owner |
| 2026-06-29 | D-S25-RETIRE1 | **retire S25 `||`/`&&` comparison distribution** (A): `||` and `&&` no longer reuse the nearest comparator for bare values. Comparator alternatives are single `|` (`x == 1 | 2` in an arm head, or `1 | 2` under an inferred comparator); `||`/`&&`/`==`/`!=` remain boolean arm condition syntax. **Ratified — syntax reconciliation cleanup pending.** c154 | owner |
| 2026-06-29 | D-BIND-CANON1 | **binding syntax stays current law** (A): canonical bindings are `name = value`, `name@ Type = value`, and `name: Type = value`; older alternatives are cleanup targets. **Ratified — confirms D-BINDEXPLICIT1 for reconciliation.** c154 | owner |
| 2026-06-29 | D-MARKER-CANON1 | **all user-typeable `#` markers are PascalCase** (A): argument markers use parens and generated/cache marker spellings are not exempt (`#Test`, `#Unsafe`, `#Extern`, `#Bindgen`, `#Layout`, `#Grant`, `#Context`). **Ratified — cleanup pending.** c154 | owner |
| 2026-06-29 | D-CFFI-CANON1 | **C FFI marker family is `#Extern` plus `#Bindgen` only** (A): delete `@extern`, `#extern`, `@bindgen`, and `#bindgen` fixtures/usages; generated binding modules use PascalCase too. **Ratified — amends D-CFFI-SYNTAX-REOPEN cleanup details.** c154 | owner |
| 2026-06-29 | D-RESULT-OPTION-CANON1 | **`T?` always means Optional** (B): fallible types must use spaced `T ? E` / `T ?`; optional return values do not need grouping to avoid fallible parsing. **Ratified — syntax/spec cleanup pending.** c154 | owner |
| 2026-06-29 | D-ORRETURN-CANON1 | **early-exit fallbacks use the `?? <control>` family only** (A): canonical forms are `expr ?? return`, `expr ?? continue`, and `expr ?? break`; delete `?return`, `?continue`, and `?break` fixtures/usages. **Ratified — confirms D-ORRETURN-ERG1 cleanup.** c154 | owner |
| 2026-06-29 | D-LOOP-SEMICOLON1 | **counted-loop semicolon header is reopened for separator redesign** (C): keep the need for a compact counted-loop form, but the `loop init; condition; afterthought` semicolon header is not final; choose a non-general-statement-separator design in follow-up before implementation. **Ratified — amends D-LOOP-SURFACE-REOPEN.** c154 | owner |
| 2026-06-29 | D-TYPE-ALIAS-CANON1 | **container/pointer types are canonical-only before Epoch 6** (A): use `[T]`, `[K,V]`, and `*T`; delete teaching/fixtures for `List<T>`, `Map<K,V>`, and `Ptr<T>`. **Ratified — cleanup pending.** c154 | owner |
| 2026-06-29 | D-CORENS-CANON1 | **`core.*` is the only standard namespace spelling** (A): delete `std.*`, `jet.*`, `jet.core`, `core.json`, and old namespace fixtures; no aliases or pre-Epoch-6 teaching diagnostics. **Ratified — strengthens D-CORENS1 cleanup.** c154 | owner |
| 2026-06-29 | D-ACRONYM-CANON1 | **standard acronym type names stay full-caps** (A): use `JSON`, `TOML`, `YAML`, `CSV`, `IOError`, and `UTF8Error` style consistently; PascalCase data aliases are rejected for this cleanup. **Ratified — cleanup pending.** c154 | owner |
| 2026-06-29 | D-SERDE-CANON1 | **serialization vocabulary is `Codable` / `Encode` / `Decode` only** (A): delete `Serialize` and `Deserialize` syntax docs/tests until any post-Epoch-6 compatibility pass. **Ratified — cleanup pending.** c154 | owner |
