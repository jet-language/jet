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
Rejected: `#`. Doc comments are S49 (M6/M13).

**S7 — Error propagation (M4)** *(ratified 2026-06-11)*: postfix `**?`**
on a fallible call (e.g. `parse(raw)?`). Prefix `try` recognized only for
a teaching error (S14). Rejected: propagation-only-via-explicit-handling.

**S13 — Logical and comparison operators (M1)** *(ratified 2026-06-11)*:
`**&&` `||` `!`** for logic; `**==` `!=` `<` `>` `<=` `>=**` for
comparisons. Word forms (`and`, `or`, `not`) recognized only for teaching
errors (S14). Note: `or` as a *type* and *fallback* operator (S34/S35) is
a separate token in expression/type context — not logical OR.

**S17 — Compound assignment (M1)** *(ratified 2026-06-11)*: the full
C-family set `**+=` `-=` `*=` `/=` `%=` `&=` `|=` `^=` `<<=` `>>=*`*.
`+=` `-=` `*=` `/=` on `Int` and `Float`; the rest on `Int` only.
Left-hand side must be `var` or a `mut` parameter. Rejected: `=` only.

**S15 — Binary profile / panic strategy** *(ratified 2026-06-11)*:
**default build keeps unwinding** (`panic` can be caught inside generated
test harnesses and task `join`). `**jet build --small`** (M6) uses
`opt-level="z"`, full LTO, and `**panic=abort**`. Rejected: abort as the
only mode.

**S16 — Imports (M6+)** *(ratified 2026-06-11)*: two forms; `**as alias`
is optional** in both. When omitted, the default namespace is the module
name (see below).

```
import "grades/scoring";              // file path → namespace scoring
import "grades/scoring" as g;         // same file, namespace g
import scoring;                       // module by name → namespace scoring
import scoring as gradebook;          // same module, namespace gradebook
```

1. **File import** — `import "<path>" [as alias];`
  `<path>` is a quoted string, relative to the **importing file's
   directory**, using `/` (no `.jet` suffix; the compiler appends it).
   Subdirectories allowed (`"util/text"`). Default namespace: the **last
   path segment** (`"grades/scoring"` → `scoring.letter(…)`).
2. **Module import** — `import <name> [as alias];`
  `<name>` is a bare identifier. The compiler searches **recursively from
   the project root** for a module named `<name>`: either a file `name.jet`
   anywhere under the root, or a directory `name/` containing `name.jet`
   or `main.jet`. Skips `build/`, `target/`, and dot-directories.
   **Project root** = the directory containing `jet.toml` when a manifest
   exists (M12); otherwise the directory of the **entry** `.jet` file.
   Ambiguous duplicate matches → **E0606** (lists every path found).

Cross-file access uses `namespace.item` for every `pub` item (S18).
Rejected: Rust `use a::b`, bare `import;` with no path or name (teaching
error only per S14), required `as`.

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
Rejected: `is` keyword, Rust `match`, accessor-only extraction.

**S32 — Absence / Option (M3)** *(ratified 2026-06-11)*: `**T?`** marks
an optional value; `**value(expr)**` when present, bare `**null**` when
absent (lowercase, like `true`/`false`). No nullable references — `null`
is only legal where a `T?` is expected, never as a value of plain `T`.
In **type** position, `?` suffix means Option; in **expression** position,
postfix `?` is error propagation (S7) — parser disambiguates by context.
Rejected: `Option[T]`, `Some`/`None`, `some`/`none`, `T??`, pointer-style
null on non-option types.

**S33 — Generic type argument brackets (M3+)** *(ratified 2026-06-11)*:
`**Type[Args]`** — square brackets for type arguments, e.g. `List[Int]`,
`Map[String, Int]`, and (when S34 is ratified) `T or E` result sides.
Rejected: angle brackets `List<Int>` (comparison/`>` ambiguity; no
turbofish).

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
| S7  | M4        | `?` parses until errors-as-values land                           | E0006 |
| S16 | M6        | `import` statement parses until multi-file driver lands          | E0019 |
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

**S26. Compile-time execution (comptime) — DEFERRED, not open for work.**
Zig-style `comptime`: run a subset of Jet at compile time for constants,
specialized functions, and unrolled loops — evaluated in sema, lowered to
plain Rust by codegen (see architecture R1/R2). Postponed until M5+
data exists to motivate generics; Tier 2 per philosophy C1. No syntax
ratified; do not implement until owner promotes from deferred.

**S28. Traits / interfaces — DEFERRED, owner intends to add.** Polymorphism
via named capability types (`trait` / `interface` — spelling TBD) is
**not in M3**. The owner has confirmed traits will be needed; syntax,
milestone slot, and whether Jet exposes Rust-style trait objects or a
simpler model are **open**. Do not implement until ratified. Rejected for
now: importing Rust's trait system verbatim into user-facing syntax.
When designing S28, prefer diagnostics beginners can read over maximal
flexibility.

### Registered for M3–M14 (see docs/06-decision-ballots.md for options)


| ID  | Question                                        | Needed by |
| --- | ----------------------------------------------- | --------- |
| S34 | fallible return type spelling (`T or E`)        | M4        |
| S35 | error handling ergonomics (`or` fallback)       | M4        |
| S36 | `panic` / `assert` builtins                     | M4        |
| S37 | list literal                                    | M5        |
| S38 | map literal                                     | M5        |
| S39 | indexing & out-of-bounds behavior               | M5        |
| S40 | slicing semantics                               | M5        |
| S41 | string model: `Char`, `len`, iteration          | M5        |
| S42 | numeric types & conversions (no `as`)           | M5/M10    |
| S43 | test syntax (`test "name" { }`)                 | M6        |
| S44 | fmt style constants                             | M6        |
| S49 | doc comments (`///`)                            | M6/M13    |
| S50 | Rust FFI `extern` syntax                        | M7        |
| S46 | lambda syntax (`(x) => …`)                      | M8        |
| S47 | function types & closure capture rules          | M8        |
| S45 | generic function/type syntax (`fn f[T: Bound]`) | M9        |
| S48 | trait-as-type = auto dynamic dispatch           | M9        |
| S51 | std library import spelling (`import "std/fs"`) | M10       |
| S54 | naming convention lint (snake_case)             | M10       |
| S53 | concurrency surface (tasks + channels)          | M11       |
| S52 | package manifest format & commands (`jet.toml`) | M12       |


S26 (comptime) and S28 (traits) keep their entries above; their ballots
live in docs/06 Group 6 (S28 becomes the concrete trait-syntax ballot;
S26's recommendation is close-as-rejected once S28/S45 are ratified).

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
| 2026-06-11 | S29 | struct literals `Type { f: v }`             | owner |
| 2026-06-11 | S30 | enums; 1-field positional, 2+ named         | owner |
| 2026-06-11 | S31 | `==` pattern tests on enums and `T?`        | owner |
| 2026-06-11 | S32 | `T?`, `value` / `null`                      | owner |
| 2026-06-11 | S33 | generic args `Type[T]` square brackets      | owner |


