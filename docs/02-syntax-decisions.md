# 02 — Syntax Decisions (the owner's control surface)

**The owner has final say on all user-facing syntax.** Agents implement
only what is Ratified, may rely on Provisional choices (clearly marked,
reversible), and must never invent surface syntax. To propose something
new: add a row to Open Decisions with options and tradeoffs, and stop.

How to ratify: move the row to Ratified with your chosen option. Agents
then update `src/syntax.rs` (and parser if structural), re-bless ui
snapshots (`UPDATE_EXPECT=1 cargo test`), and update docs/01-spec.md.

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

**S22 — Range bounds (M1)** *(ratified 2026-06-11)*: `**1..10` is
inclusive** — it counts 1 through 10. Reads like English, kills the classic
beginner off-by-one. M5 slicing may bring its own evidence; revisit there
if needed. Rejected: half-open `..` (Rust/Python), dual `..`/`..=`, word
form `1 to 10`.

**S23 — Loop control (M1)** *(ratified 2026-06-11)*: `**break`** (leave
the loop now) and `**continue**` (skip to the next turn). Rejected:
plain-word `stop`/`skip`, omitting loop control from M1.

**S24 — Many-way choice: `switch` (M1)** *(ratified 2026-06-11)*:

```
switch x {
    x == 1 -> { ... };
    x == 2 || x == 3 -> { ... };
    else -> { ... };
}
```

Keyword `**switch**`; the head expression names the subject being
examined; each arm is a full `Bool` condition, then `->`, then a `{ }`
block, ended with `;` (S6). The first true arm runs; **an `else` arm is
required**. Arms are ordinary conditions, so ranges and compound tests
need no special pattern syntax (`x >= 400 && x <= 499 -> { … };`).
The backend lowers subject-equals-literal chains to a native Rust `match`
(jump tables where profitable) and everything else to an if/else chain —
optimization is the compiler's job, never the user's. Rejected: C
`switch`/`case`/`default` (fallthrough baggage), bare-value `match`
(`match` is recognized only for an S14 teaching error). M3's enum
exhaustiveness story extends `switch`.

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

**S5 — Comments** *(ratified 2026-06-11)*: `**//`** to end of line.
Rejected: `#`. Doc comments: `///` (S49).

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
position (see docs/plans/m03-data.md).

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
In **type** position, `?` suffix means Option; in **expression** position,
postfix `?` is error propagation (S7) — parser disambiguates by context.
Rejected: `Option[T]`, `Some`/`None`, `some`/`none`, `T??`, pointer-style
null on non-option types.

**S33 — Generic type argument brackets (M3+)** *(ratified 2026-06-11;
amended 2026-06-12)*: `**Type<Args>**` — angle brackets for type
arguments, e.g. `List<Int>`, `Map<String, Int>`, and `Result<T, E>`
fallible returns (S34). Square brackets `**[]**` are reserved for **value**
list/map literals (S37/S38) and indexing (S39) — never for generic types,
so `List<Int>` is a typed container and `[1, 2, 3]` is a list value.
Parser disambiguates `<` in type position from comparison; nested closings
split `>>` like Rust. Rejected: square-bracket type args `Type[Args]`
(E0034 teaches `Type<Args>`).

**S34 — Fallible return type (M4)** *(ratified 2026-06-11)*:
`**Result<T, E>**` — e.g. `fn parse(s: String) -> Result<Int, ParseError>`.
`Result` is a prelude builtin (S33 angle brackets); `E` is any enum,
struct, or `String`. Codegen lowers to Rust `Result<T, E>`. Rejected:
`T or E` in type position (A), Zig `!T` with inferred error sets (C).

**S45 — Generic function/type syntax (M9)** *(ratified 2026-06-12)*:
angle brackets for type parameters — `fn largest<T: Comparable>(…)`,
`struct Pair<T> { … }`, bounds `<T: A + B>`. Same brackets as S33
(`List<T>`, `Result<T, E>`). Inline bounds, no `where`, no call-site type
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

**S35 — Error handling ergonomics (M4)** *(ratified 2026-06-11)*:
`**or` fallback** on a fallible or optional value — e.g. `parse(x) or 0`,
`parse(x) or return`, `parse(x) or panic("…")`, `m.get(k) or 0` on `T?`.
Plus **`== ok(v)` / `== err(e)`** pattern tests (S31 machinery) and
postfix **`?`** propagation (S7). Rejected: Rust `.unwrap_or` / `.expect`
methods only (B), patterns + `?` with no `or` sugar (C).

**S36 — Bug stops (M4)** *(ratified 2026-06-11)*: `**panic("msg")**`
stops the program with a friendly runtime report (file, line, exit 70);
`**require(cond)**` and `**require(cond, "msg")**` panic when the
condition is false — for programmer invariants and preconditions, not
recoverable user errors (`Result<T, E>`). Both are prelude builtins like
`print`. Prefix `assert` is recognized only for a teaching error (S14)
pointing at `require`. Rejected: `assert` as the canonical builtin name,
user-facing `abort`/`fatal` (S15 already uses *abort* as a build-mode
name), panic-only without `require` sugar.

**S37 — List literal (M5)** *(ratified 2026-06-12)*: `**[a, b, c]**`;
empty `**[]**` needs a context type (same pattern as `null` / `none`).
Rejected: `List(1, 2, 3)`, brace literals `{1, 2, 3}`.

**S38 — Map literal (M5)** *(ratified 2026-06-12)*: `**["key": value, …]**`;
empty `**[:]**`. Rejected: brace literals `{"k": v}` (JSON confusion with
blocks), constructor-only maps with no literal.

**S39 — Indexing & out-of-bounds (M5)** *(ratified 2026-06-12)*:
`**xs[i]**` and map read `**m[k]**` stop the program with a friendly
runtime report on out-of-bounds / missing key; `**xs.get(i) -> T?**` (and
`m.get(k) -> V?`) for safe access. Write `m[k] = v` inserts. Rejected:
indexing always returns `T?` (unwrap ceremony), split policy (Option for
maps only).

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

**S42 — Numeric types & conversions (M5/M10)** *(ratified 2026-06-12)*:
`**Int**` and `**Float**` are the **default** numeric types — untyped
literals, inference, tutorials, and std APIs use them unless a binding or
parameter is explicitly annotated otherwise (`Int` = i64, `Float` = f64).
A full **sized-type menu** is available for experts and FFI/binary work:
`**I8**` `**I16**` `**I32**` `**I64**` `**U8**` `**U16**` `**U32**`
`**U64**` `**F32**` `**F64**`. `Int`/`Float` are the beginner-facing
spellings for the 64-bit types; `I64`/`F64` exist for explicit-width and
FFI code. Conversions are **named methods only** — e.g. `n.to_float()`,
`f.to_int()`, `x.to_i32()`, `Int.parse(s) -> Result<Int, ParseError>`;
no `**as**` keyword (E0026 teaches the named forms). Rejected:
arbitrary-precision integers (C), implicit widening, lowercase Rust
spellings (`i64`).

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
non-`std` crates (reproducibility); pins may migrate into `jet.toml` when
a manifest exists (M12). Boundary types pass by value only — no borrows,
callbacks, or trait objects across the edge. Rejected: per-function
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

**S52 — Package manifest (M12)** *(ratified 2026-06-12)*: `**jet.toml`** —
tiny TOML subset, hand-parsed in the compiler (I6). Sections:
`[package]` (`name`, `version`), `[dependencies]` (git/path, exact pins),
`[rust-dependencies]` (M7 FFI pins migrate here). Lockfile `**jet.lock**`;
commands `jet add` / `jet fetch`; registry later as a static git index.
Single-file `jet run file.jet` stays manifest-free forever (R9). Rejected:
JSON manifest, manifest written in Jet (build.zig style).

**S53 — Concurrency surface** *(ratified 2026-06-12; deferred past v1.0)*:
**deferred to v2** — no tasks, channels, or `std/tasks` in v1. When
implemented, the planned surface is ballot option A: `tasks.spawn(closure)
-> Task<T>`, `t.join() -> T`, `tasks.channel<T>()` with
`Sender`/`receive() -> T or Closed`; no shared mutable state (ownership
rejects it). Rejected for v1: `go`-style `spawn { }` fire-and-forget,
shipping concurrency in v1.

**S59 — C FFI** *(ratified 2026-06-12; deferred past v1.0)*: **deferred to
v2**. Rust FFI (S50/M7) is v1's interop story. When implemented, the
planned surface is ballot option A: `extern c "header-or-lib" { … }` blocks
mirroring S50's `extern rust` shape — one FFI idiom, two backends;
by-value boundary first, pointers only inside the S58 tier. Rejected for
v1: bindgen-style auto-generation as the primary surface, Rust-crate
detour only.

**S60 — Pure-function marking** *(ratified 2026-06-12; post-1.0 milestone
pending)*: `**pure fn name(…)**` — a checked modifier; purity is part of
the signature; violations are compile errors naming the impure call path.
Enables `jet eval --pure` (jetpack JP0 direction) and makes comptime
callability visible at API boundaries. Rejected: inference-only purity with
no marking, full effects system.

## Enforcement

Ratified decisions are **frozen**. `cargo test` runs `tests/decisions.rs`,
which fails if:

- any `src/syntax.rs` entry is `(provisional)` while ratified in this file;
- any open or deferred decision ID appears in `src/syntax.rs`;
- the Provisional table below lists a real decision ID;
- a staged decision loses its pinned error code in docs/04.

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
> lean, recommendation — in **docs/06-decision-ballots.md**, grouped so
> the owner decides one milestone-sized batch at a time. The rows here
> are the registry; the ballots are the briefing.

### Registered for M3–M14 (see docs/06-decision-ballots.md for options)


| ID  | Question                                   | Needed by |
| --- | ------------------------------------------ | --------- |
| S56 | typed reflection / user derives (deferred) | post-1.0  |


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
| 2026-06-11 | S34 | fallible returns `Result[T, E]`             | owner |
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
| 2026-06-12 | S51 | std imports: `import std.fs as fs` module form | owner |
| 2026-06-12 | S54 | no prescribed naming convention in v1        | owner |
| 2026-06-12 | S52 | `jet.toml` manifest; `jet.lock`; jet add/fetch | owner |
| 2026-06-12 | S53 | concurrency deferred to v2; option A when built | owner |
| 2026-06-12 | S59 | C FFI deferred to v2; `extern c` when built  | owner |
| 2026-06-12 | S60 | `pure fn` checked purity modifier            | owner |
