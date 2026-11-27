# Writing Good Code in Jet: Exhaustive Friction Audit, Guardrails vs. Traps, and Jet vs. Go Errors

*Date: 2026-11-27*  
*Sources: Unfiltered, independent feedback from three engineering agents who wrote thousands of lines of real Jet across the standard library (`Core/`), the compiler and runtime (`Compiler/`), and developer tools (`JetCLI/Repl`, `JetParser/Formatter`, `JetLSP`), plus direct verification against the live compiler and Tower board.*

---

## How to Read This Document

Not all friction is bad. A great programming language **should** push back when you are about to write sloppy, racy, or bug-prone code.

This report separates every finding into two fundamentally different buckets:

1. **Productive Guardrails (Keep!)** — rules that feel annoying for 10 seconds, then make your code objectively safer, clearer, and easier to review.
2. **Accidental or Counterproductive Friction (Fix!)** — syntax collisions, missing shortcuts, compiler gaps, and tooling drag that prevent **zero** bugs — or worse, actively push programmers toward **worse** code (like switching immutable variables to mutable ones, writing throwaway wrapper functions, or hiding copies inside string interpolation).

Every single item below is written in plain English with a concrete example and **proposed fixes / starting ideas** at the end.

---

## Table of Contents

1. [The Bottom Line: Would Fixing These Make Jet Near-Perfect?](#1-the-bottom-line-would-fixing-these-make-jet-near-perfect)
2. [Productive Friction Worth Keeping (What Forces *Good* Code)](#2-productive-friction-worth-keeping-what-forces-good-code)
3. [Exhaustive Catalog of Friction to Address (50 Items Across 10 Areas)](#3-exhaustive-catalog-of-friction-to-address-50-items-across-10-areas)
   - [A. Control Flow, Patterns & Matching (Items 1–8)](#a-control-flow-patterns--matching)
   - [B. Syntax, Braces, Literals & Strings (Items 9–17)](#b-syntax-braces-literals--strings)
   - [C. Types, Structs, Enums, Generics & Signatures (Items 18–25)](#c-types-structs-enums-generics--signatures)
   - [D. Ownership, Borrowing & Copying (Items 26–30)](#d-ownership-borrowing--copying)
   - [E. Error Handling Rough Edges (Items 31–33)](#e-error-handling-rough-edges)
   - [F. Collections, Strings, Bytes & Stdlib Ergonomics (Items 34–38)](#f-collections-strings-bytes--stdlib-ergonomics)
   - [G. Concurrency, Effects & Imports (Items 39–43)](#g-concurrency-effects--imports)
   - [H. Low-Level & Runtime Code (Items 44–45)](#h-low-level--runtime-code)
   - [I. Compiler Architecture & Core Library Coupling (Items 46–47)](#i-compiler-architecture--core-library-coupling)
   - [J. CLI, Testing, Formatting & Diagnostics Tooling (Items 48–50)](#j-cli-testing-formatting--diagnostics-tooling)
4. [Deep Dive: Jet Error Handling vs. Go Error Handling](#4-deep-dive-jet-error-handling-vs-go-error-handling)
5. [What Already Works Really Well (Do Not Regress)](#5-what-already-works-really-well-do-not-regress)

---

## 1. The Bottom Line: Would Fixing These Make Jet Near-Perfect?

**Short answer from all three agents: On core language design for writing *good, safe, maintainable* software — YES, Jet would genuinely beat Rust, Go, Swift, Zig, TypeScript, and Python.**

### Where Jet genuinely beats every mainstream peer
- **vs. Rust:** ~90% of Rust's memory safety, data-race freedom, and zero-GC performance with **no lifetime-annotation soup** (`'a`, `Pin<Box<dyn Future>>`, `Arc<Mutex<T>>`), **no `async` function coloring**, built-in structured concurrency (`task.group`), explicit call-site mutation (`^`), and first-class checked effects.
- **vs. Go:** Eliminates nil-pointer panics, data races, ignored errors, sentinel `-1` / zero-value traps, and leaked goroutines, while adding real sum types, ownership, and checked effects — keeping Go's "one canonical formatter / batteries-included toolchain" spirit.
- **vs. Swift:** Predictable native performance without ARC retain cycles or hidden copies, explicit effects/authority, exact `Decimal` vs. binary `Float`, and first-class Linux/systems support without Swift's attribute sprawl (`@escaping`, `@Sendable`, `nonisolated(unsafe)`).
- **vs. Zig:** Memory safety by default, real traits/generics/closures, real structured concurrency, and high-level productivity without manual allocator plumbing on every string or list — while still offering `#Layout(c)` and custom allocators when needed.
- **vs. TypeScript & Python:** Native compiled speed, sound static types, true multi-core parallelism (no GIL), and fearless large-scale refactoring.

### Brutal honesty: 4 deeper things that would *still* hold Jet back even after fixing every paper cut
1. **Incremental compile & test speed (the #1 real-world make-or-break):**  
   Writing *good* code is an iterative craft: refactor, simplify, re-run tests. When checking/testing one file takes 2–5 minutes instead of 1–2 seconds, programmers stop refactoring and settle for "good enough." Sub-second incremental `jet check` and targeted `jet test` will improve real-world Jet code quality more than any single syntax tweak.
2. **Cognitive budget & orthogonality (avoiding "C++/Scala syndrome"):**  
   Almost every individual Jet feature is well-motivated, but together there are many bespoke rules and compiler special cases (4 `if` forms, 4 `loop` forms, 3 discard forms, bespoke compiler paths for `Complex`/`Decimal`/`Quaternion`/`Duration` instead of general traits). The best languages stay **small and orthogonal** so you can *predict* how two features combine instead of memorizing exceptions.
3. **The borrowing middle ground (`View<T>` and borrowed `String`):**  
   Right now, returning or storing a read-only `String` slice without copying is awkward, which pushes authors into disguised copies (`return "{line}"`). Making `View<T>` / borrowed `String` returns and closure captures smooth is essential for idiomatic, zero-waste code.
4. **Effect-system ergonomics at large codebase scale:**  
   Explicit effects (`-[Net, FS.Read, Time.Wait]>`) are a superpower in small/medium modules, but in large layered libraries rigid leaf-effect lists can become the new "Java checked exceptions" unless higher-order functions (`map`, `filter`, callbacks) and library boundaries can abstract over or bundle effects cleanly.

---

## 2. Productive Friction Worth Keeping (What Forces *Good* Code)

Before touching anything, here is the friction all three agents **explicitly asked Jet to KEEP** — because even when it slowed them down for a moment, it directly prevented bugs or made the resulting code dramatically clearer:

| Guardrail | Why it feels annoying at first | Why it forces *better* code (KEEP) |
|---|---|---|
| **Visible `^` and `^^` on bindings, parameters, and call sites** | Extra characters when calling mutating helpers | You can read any function body and immediately see every variable that mutates (`^x`), every call that mutates an argument (`bump(^x)`), and every call that consumes ownership (`send(^^msg)`). Zero hidden side effects. |
| **Visible `?` on every propagating fallible call** *(just shipped in Batch 89!)* | Adding `?` after fallible calls | **Independently validated by 2 agents** who were still on the older seed compiler without `?`: invisible early returns leaked `ParseError!` 4 frames up the call stack into untouched files and made auditing locks/state hard. `?` completely eliminates that trap. |
| **Exhaustive enum matching by default** | Compiler rejects missing cases when an enum grows | Caught forgotten `.Interrupted` / `.DeadlineBlown` / `.Unstarted` states immediately. *(The fix is building the already-ratified `#CatchAll` opt-out — NOT dropping default exhaustiveness!)* |
| **Refusing never-written `^x` bindings (`E0514`)** | Extra error when refactoring removes a mutation | Keeps immutability honest: if a reader sees `^x`, they know `x` actually changes somewhere in scope. |
| **Strict `Decimal` vs. `Float` separation on variables** | Can't add a `Decimal` price and a `Float` ratio | Eliminates silent base-10 / binary-float financial rounding bugs. *(Only untyped literals like `1.0 - f` need symmetric contextual typing — keep variable separation strict!)* |
| **Checked arithmetic, `/%` vs. `/`, and arbitrary-precision `Int` default** | Muscle memory hits `/` on `Int` | Prevents silent integer overflow and silent integer-division truncation bugs. |
| **Typed `#Error` enums + explicit `impl Target.From(Source)` across domain boundaries** | Can't lazily throw arbitrary strings across layers | Forces intentional error boundaries and prevents Go-style stringly-typed error soup. |
| **Checked effects (`-[IO, Net, Time.Wait]>`) & capability `authority:`** | Must declare what I/O or waits a function uses | Forces clean architectural separation between pure logic and I/O/network code, and makes ambient authority auditable by construction. |
| **Structured concurrency (`task.group`, `limit:`, panic isolation, lossless `SendRefused<T>`)** | Can't fire-and-forget an untracked goroutine or ignore a full channel | Eliminates leaked tasks, silent message drops, and sentinel `-1` return codes. |
| **Mandatory `.drop("reason")` on linear/resource/error values** | Can't lazily ignore a resource or `IOError!` | Forces an explicit reason when discarding an error or handle instead of silent `let _ = ...`. *(Just don't force it on trivial non-linear `Bool`/`Option` returns.)* |
| **Explicit `.copy()` visibility** | Reminds you when a deep clone happens | Prevents silent O(N) performance regressions (like C++ copy constructors). *(As long as it doesn't force fake copies like `"{s}"`!)* |

---

## 3. Exhaustive Catalog of Friction to Address (50 Items Across 10 Areas)

Legend:
- **Status / Severity:**
  - `RATIFIED — NOT YET BUILT` = already approved on Tower, just missing from the compiler.
  - `ALREADY FIXED IN BATCH 89` = hit by agents on the older seed, already solved by recent work.
  - `COUNTERPRODUCTIVE` = actively pushes authors to write *worse* code.
  - `ACCIDENTAL FRICTION` = costs time/readability while preventing zero bugs.
  - `BUG / COMPILER GAP` = genuine defect or incomplete implementation.
  - `TOOLING QUICK WIN` = CLI/test/diagnostic improvement with zero language risk.

---

### A. Control Flow, Patterns & Matching

#### 1. Partial enum matching (`#CatchAll`) is ratified but not built yet — `RATIFIED — NOT YET BUILT` · `COUNTERPRODUCTIVE`
- **What happens:** You approved **`#CatchAll`** (`D-GUARD-LINT1`, outcome `A`, ratified 2026-11-18):
  ```jet
  if shape == {
      .Circle(r) -> 3 * r * r
      .Square(s) -> s * s
      else -> { 0 } #CatchAll
  }
  ```
  However, `CatchAll` was **never added** to `Compiler/Prelude/Markers.jet` or `Compiler/JetSema/Source/Sema/Coverage.jet` (only `#Open` from `D-GROWING-ENUM1` was built, and `#Open` only applies *across packages* to enums the author marked `#Open`). Today, `E0641` (`Jet.ClosedSetElse`) still rejects every `else ->` on a closed enum — forcing 1,500-character horizontal catch-all lines listing 40–100 ignored variants with exact arities (`.Fn(_, _, _, _, _, _) | .Map(_, _, _) | ...`) that break on every added variant.
- **Proposed fixes / ideas:**
  - Implement ratified `#CatchAll` (`D-GUARD-LINT1=A`) in `Markers.jet`, `Coverage.jet`, and `JetLSP` hover (hover lists the variants currently covered).
  - Also make single-case checks free (`if shape == { .Circle(r) -> ... else -> ... }` with 1 distinct case needs no marker, per `D-GUARD-LINT1=A`).

#### 2. Value-producing `if` requires `->`, while statement `if` uses `{}` — `COUNTERPRODUCTIVE`
- **What happens:** Writing:
  ```jet
  hint: if raw_mode {
      "Press Tab to complete, F1 for docs, Ctrl-C to stop."
  } else {
      "Type :help for commands or :quit to exit."
  }
  ```
  fails with `Expected -> after the condition of a value-producing if, found {`. Putting long strings after `->` breaks the 100-column formatter limit, so authors **give up on immutability** and rewrite it as `^hint: ""; if raw_mode { hint = ... } else { hint = ... }`.
- **Proposed fixes / ideas:**
  - Allow standard `{ ... } else { ... }` blocks on value-producing `if` expressions (keeping `->` as the concise one-liner shorthand).
  - Ensure `jet fmt` never forces rewriting a readable multi-line `{ ... }` branch onto an over-long `->` line.

#### 3. Multi-statement `task { ... }` blocks can't return a value naturally — `BUG / COMPILER GAP` · `COUNTERPRODUCTIVE`
- **What happens:**
  ```jet
  h: task {
      ^sum: 0
      loop n in items -> sum += n
      sum
  }
  ```
  fails with `This line computes a value but doesn't do anything with it` on `sum`; changing it to `return sum` then fails with `This needs Unit, but the value is Int`. Meanwhile `task fn() -> Int { ... }` fails to parse. Authors have to extract throwaway top-level helper functions just to return a value from a multi-statement task.
- **Proposed fixes / ideas:**
  - Make trailing value expressions work uniformly across `fn`, lambdas, `task { ... }`, `if { ... }`, and block expressions.
  - Fix `task { ... }` return-type inference when a multi-statement body ends in an expression or `return value`.

#### 4. Deep nested `if` ladders for multi-step pattern unwrapping — `ACCIDENTAL FRICTION`
- **What happens:** Checking an `Option` whose payload contains another `Option` or `Result` requires 3–4 levels of nested `if` blocks (`if a == .Val(x) { if f(x) == .Val(y) { ... } }`), because `if a == .Val(x) && f(x) == .Val(y)` and guard-clause unwrapping on arbitrary enum variants aren't generally supported.
- **Proposed fixes / ideas:**
  - Support chained pattern guards (`if a == .Val(x) && x > 0 && f(x) == .Val(y) { ... }`).
  - Generalize guard-clause extraction (`Val(x): opt ?? return`) or `else` guards (`x: opt ?? return`) cleanly across `Option`/`Result`/enum variants.

#### 5. Competing `if` / `else` forms & overly rigid `Jet.SameSubjectDispatch` lint — `ACCIDENTAL FRICTION`
- **What happens:** `if { c1 -> a; c2 -> b; else -> c }`, `if c1 -> a else if c2 { b }`, and `if x == { ... }` all coexist with subtle arrow/brace rules. Switching one branch from 1 statement to 2 forces rewriting both branches. Meanwhile, `Jet.SameSubjectDispatch` warns even on flat guarded early-return `if` statements (`if key.kind == F1 { ... } if key.kind == CHAR && key.text == "q" { ... }`), where a nested table is harder to read.
- **Proposed fixes / ideas:**
  - Allow `{ ... }` uniformly on single-statement branches without forcing `->` churn.
  - Exempt guarded or early-return `if` statements with heterogeneous conditions from `SameSubjectDispatch`.

#### 6. Finite value-loop `??` requirement (`E0078`) has an unclear span — `ACCIDENTAL FRICTION`
- **What happens:** `found: loop x in xs { if ok(x) -> break x }` requires a trailing `?? fallback` after the closing `}`, and the `E0078` error points at the closing `}` instead of the `loop` header.
- **Proposed fixes / ideas:**
  - Anchor `E0078` on the `loop` keyword with a clear fix-it showing `?? default_value` after the closing brace.

#### 7. Three competing discard idioms (`_:`, `^_:`, `.drop("reason")`, `?? ()`) — `ACCIDENTAL FRICTION`
- **What happens:** Ignoring a value sometimes requires `_: expr`, sometimes `^_: list.pop()`, sometimes `.drop("reason")`, and sometimes `?? ()`. It's hard to predict which one the compiler wants.
- **Proposed fixes / ideas:**
  - Unify non-linear discards under plain `_: expr` (never requiring `^_:`).
  - Reserve mandatory `.drop("reason")` strictly for linear/resource/error values (handles, tasks, `Result`).

#### 8. `next` keyword collides with common variable names — `ACCIDENTAL FRICTION`
- **What happens:** Jet uses `next` instead of `continue` in loops, yet `next` is one of the most common local variable names in iterators, parsers, and linked/state code (`.Ok(next) ->`, `next: ir_body_stmt(...)`).
- **Proposed fixes / ideas:**
  - Keep a crisp diagnostic when someone writes `continue` suggesting `next`, and warn cleanly if a local named `next` shadows loop `next` inside a loop body.

---

### B. Syntax, Braces, Literals & Strings

#### 9. Casts and struct literals (`Int{x}`, `Float{0.0}`, `Type.Named{...}`) collide with `if`/`loop` `{` blocks — `COUNTERPRODUCTIVE`
- **What happens:** Because `Int{x}`, `U8{x}`, `Float{0.0}`, and `Type.Named{name: "Unit"}` all use `{...}`, writing:
  ```jet
  if Int{bytes[i]} == 126 { ... }
  if ret_ty != Type.Named{name: "Unit"} { ... }
  if value - value == Float{0.0} { ... }
  ```
  parses `{` as starting the `if`/`loop` body! Jet actually has `E0637` (`TypedLiteralInHeader`) for `if a > I32{0}`, **but it misses the left-hand side** (`if Int{bytes[i]} == 126`) and emits a bizarre *"This line computes a value but doesn't do anything with it / found `==`"* error. Authors litter conditions with defensive parens (`if (Int{bytes[i]}) == 126`) or throwaway locals (`zero: Float{0.0}`).
- **Proposed fixes / ideas:**
  - **Best fix:** Disambiguate `TypeName{...}` inside `if`/`loop` headers in the parser when `{...}` is followed by an operator (`==`, `<`, `&&`, `||`, `.`), `->`, or `{`.
  - **Minimum fix:** Extend `E0637` to fire on *all* positions in `if`/`loop` headers (not only after comparison/`in`) with a 1-click parenthesizing fix-it.

#### 10. Asymmetric `Decimal` vs. `Float` literal typing (`x - 1.0` works, `1.0 - x` fails) — `COUNTERPRODUCTIVE`
- **What happens:** Bare decimals (`0.0`, `1.0`) default to `Decimal`, while math/SIMD/graphics use `Float`. Currently `t - 1.0` compiles, **but `1.0 - t` fails** (`Jet.FloatDecimalMix`), and `if cond -> f else -> 0.0` fails (`BranchTypeMismatch: Float and Decimal`). Formulas like `(1.0 - t) * a + t * b` turn into `(Float{1.0} - t) * a + t * b`.
- **Proposed fixes / ideas:**
  - Keep strict separation between typed `Decimal` and `Float` *variables* (productive guardrail!), but make untyped decimal *literals* adopt `Float` **symmetrically** on both LHS and RHS of binary operators, in comparisons (`number != 0.0`), and across `if`/match branches whenever the peer type is `Float` (or `F32`/`F16`/`Complex`/`Quaternion`).

#### 11. Every `"..."` string interpolates `{...}`, turning JSON, tests & `\u{...}` into `{{{{` soup — `ACCIDENTAL FRICTION`
- **What happens:** `"if true {"` fails to compile (*unterminated `{`*). `"\\u{7f}"` fails with *"f isn't a unit in scope"* (because `{7f}` parses as `7` with suffix `f`!). JSON becomes `"{{\"textDocument\":{{\"uri\":{uri}}}}}"`. Backtick raw strings `` `...` `` skip `{...}` interpolation, **but** they cannot contain `\n`, `\t`, or `\u{...}`, and you cannot nest backticks inside backtick strings.
- **Proposed fixes / ideas:**
  - Support a clean non-interpolating escaped string (e.g. `r"..."` or `#"..."#` / multi-backtick raw delimiters `` ``...`` ``), OR opt-in interpolation syntax (`$"..."` / `f"..."`), so JSON, regexes, and code-generating tests never need `{{{{` escaping.

#### 12. String interpolation can ICE on expressions that work normally — `BUG / COMPILER GAP`
- **What happens:** `print("{Float{2.0} * 3.0i}")` crashed the compiler with an ICE (`exit 101: ir.build unlowered: runtime route jet_complex_from_parts`), while binding `m: Float{2.0} * 3.0i; print("{m}")` worked fine.
- **Proposed fixes / ideas:**
  - Route every `{expr}` hole inside string interpolation through the exact same demand/sema/lowering pipeline as an ordinary expression (no separate mini-path).

#### 13. No ASCII byte literal syntax (`b'\n'`, `b'#'`, `b'['`) — `ACCIDENTAL FRICTION`
- **What happens:** Parsers, lexers, terminal decoders, and binary encoders are full of unreadable ASCII magic numbers: `U8{10}`, `U8{13}`, `U8{27}`, `U8{92}`, `b2 == 51`, `(Int{bytes[i]}) == 46`.
- **Proposed fixes / ideas:**
  - Add byte literals (`b'\n'`, `b'#'`, `b'\\'` of type `U8`) or `U8{'#'}` char-to-byte constant folding.

#### 14. No contextual integer literal coercion for `U8`, `I64`, `U64` — `ACCIDENTAL FRICTION`
- **What happens:** Every integer constant in byte/runtime/bit-level code needs explicit wrapping (`bytes[i] == (U8{10})`, `offset + I64{152}`, `mask & U64{15}`), which is verbose on its own and triggers item #9's brace collisions.
- **Proposed fixes / ideas:**
  - Allow untyped integer literals (`0`, `10`, `48`, `152`) to adopt `U8`/`I32`/`I64`/`U64` whenever the expected type is fixed by the other operand, parameter, or variable type (with compile-time range checking so `b == 300` on a `U8` is still a compile error!).

#### 15. `I64.MIN` cannot be written as `-9223372036854775808` — `BUG / COMPILER GAP`
- **What happens:** `9223372036854775808` overflows `I64.MAX` before unary `-` applies, forcing `-9223372036854775807 - 1`.
- **Proposed fixes / ideas:**
  - Fold unary `-` directly onto integer literals during lexing/parsing/sema, and provide `I64.MIN` / `I64.MAX` constants.

#### 16. No inclusive range `1..=N` or reverse range `(0..<n).rev()` — `ACCIDENTAL FRICTION`
- **What happens:** `loop i in 1..=20` fails with `Expected a value, found =` (no hint to use `..<`), and iterating backwards requires manual `^i: n - 1; loop i >= 0 { ...; i -= 1 }`.
- **Proposed fixes / ideas:**
  - Either support `..=` directly or emit a targeted hint (*"Jet ranges are exclusive (`..<`); write `1..<21`"*).
  - Add `.rev()` and `.step(k)` range helpers (`loop i in (0..<n).rev()`).

#### 17. Unhelpful error when writing `^` for bitwise XOR (actually `~`) or `/` on `Int` (actually `/%`) — `TOOLING QUICK WIN`
- **What happens:** Writing `bits ^ 0x8000` prints `Expected } to close a typed literal, found ^` with zero hint that bitwise XOR in Jet is `~`. Writing `10 / 3` silently turns into a `Fraction` until a downstream type mismatch appears.
- **Proposed fixes / ideas:**
  - Emit a crisp targeted hint whenever `^` appears in binary-operator position (*"Bitwise XOR in Jet is `~`; `^` is the write mark"*) and suggest `/%` when a `Fraction` from `Int / Int` meets an `Int` slot.

---

### C. Types, Structs, Enums, Generics & Signatures

#### 18. No general binding type-ascription syntax (`name: Type = expr`) — `COUNTERPRODUCTIVE`
- **What happens:** `^items: [Int]{}` works for literals/containers, but `job: Task<Int DomainError!> = task { parse(x) }` fails to parse. Whenever an expression needs an expected type (`task { ... }`, `Ok(...)`, `Err(...)`, closures, empty lists), authors must invent **dummy single-use helper functions** purely to supply a return type.
- **Proposed fixes / ideas:**
  - Support a clean local typed-binding or expression ascription form (e.g., `job: Task<Int DomainError!> = task { ... }` or `as`/`Type(expr)` — aligned cleanly with Jet's `:` declaration law).

#### 19. Named-field enum construction vs. positional pattern matching mismatch — `ACCIDENTAL FRICTION`
- **What happens:** Many enums construct with named fields (`TExprKind.Lambda{value: v}`, `Type.List{element: Type.Int}`) yet match positionally (`.Lambda(v)`), while other variants construct positionally (`Val(x)`). There's no visual rule telling you which one a variant uses until `UnknownArgumentLabel` fires.
- **Proposed fixes / ideas:**
  - Support named-field destructuring in patterns (`.Lambda{value: v}`, `.IntLit{..}`) AND document/enforce one clear convention for positional vs. named enum payloads.

#### 20. Inconsistent leading dot on `.Val(x)` vs. `Val(x)` and `.Variant` shorthand — `ACCIDENTAL FRICTION`
- **What happens:** Some code writes `== Val(true)` and other code writes `== .Val(index)`; `None` has no dot; `.Negative(n)` works in match arms, while `Err(DomainError.Negative(n))` needs the full enum name right next to it.
- **Proposed fixes / ideas:**
  - Propagate enum expected types uniformly through `Ok(...)`, `Err(...)`, `Val(...)`, and `==` comparisons so `.Variant` shorthand works everywhere the type is known.

#### 21. No struct-update syntax (`Struct{...base, field: new_val}`) — `COUNTERPRODUCTIVE`
- **What happens:** Updating 1 field of a 6-field immutable struct forces repeating all 5 unchanged fields verbatim in every state-transition function (e.g. 7 times in `TaskState`), inviting copy-paste bugs when a field is added later. *(Note: `D-STRUCT-UPDATE1` is ratified! Check implementation coverage across Core/Compiler.)*
- **Proposed fixes / ideas:**
  - Ensure ratified `Struct{field: val, ...base}` works everywhere in `Core/` and `Compiler/`, and auto-fill fields with declared defaults (or `None` for optional fields when appropriate).

#### 22. Single-argument `Type{x}` conversion hits `AmbiguousCall` with two numeric `From` impls — `BUG / COMPILER GAP`
- **What happens:** Giving `F16` both `impl From(Float)` and `impl From(F32)` made `F16{1.5}` fail with `Jet.AmbiguousCall` because the decimal literal matched both candidates instead of preferring `Float`.
- **Proposed fixes / ideas:**
  - Rank canonical default numeric types (`Float` over `F32`, `Int` over `I64`/`I32`) when resolving `From` overloads on untyped literals.

#### 23. No `impl Neg` operator trait for unary `-` — `BUG / COMPILER GAP`
- **What happens:** `Add`, `Sub`, `Mul`, `Div`, `Equatable`, `Comparable` all have standard operator traits, but unary `-` has no `Neg` trait — forcing `Quaternion` and `F16` to add `.negated()` plus special-case wiring inside `Expressions.jet`.
- **Proposed fixes / ideas:**
  - Add a standard `Neg` trait (`neg: fn(self) -> Self`) in Core Prelude so user structs and Core math types use one uniform mechanism.

#### 24. Closure parameter types not inferred in generic methods like `sort_by` — `ACCIDENTAL FRICTION`
- **What happens:** `edits.sort_by(fn(left, right) { ... })` forced repeating a 5-field anonymous record type three times (`LSPFootprint.jet`) because `sort_by` didn't propagate the list's element type into the closure's parameters.
- **Proposed fixes / ideas:**
  - Propagate generic method parameter types from the receiver (`List<T>.sort_by`) into closure arguments during type inference.

#### 25. Generic placement (`Endpoint: struct<Q, R>`) & `*` keyword-only call hints — `ACCIDENTAL FRICTION`
- **What happens:** Writing `Endpoint<Q, R>: struct` instead of `Endpoint: struct<Q, R>` trips muscle memory constantly; calling a function with parameters after `*` requires guessing which arguments must be labeled.
- **Proposed fixes / ideas:**
  - Add a tailored parser recovery hint for `Name<T>: struct` / `name<T>: fn(...)` showing the canonical `Name: struct<T>` form.
  - Include a copy-pasteable call-site fix-it in `E0104` / `E0769` naming the exact required labels.

---

### D. Ownership, Borrowing & Copying

#### 26. `_: param` is treated as a move (`E0120`) instead of a discard — `COUNTERPRODUCTIVE`
- **What happens:** Writing `_: axis` or `_: z` to silence an `UnusedParameter` warning on a read parameter fails with `E0120: z was not moved here, so it cannot be taken with .move()`. Authors resort to dummy `print("{z}")` or fake field reads in tests.
- **Proposed fixes / ideas:**
  - Treat `_: expr` on a read binding as an explicit no-op read/discard (not a consuming move), or standardize on `_param` parameter naming with a 1-click fix-it from `L0102`.

#### 27. Returning a borrowed `String` / small struct + `.copy()` ratchet pushes authors into disguised copies — `COUNTERPRODUCTIVE`
- **What happens:**
  - Returning a read `String` parameter (`if !color -> return line`) fails with `ReadOnlyValueNotOwned`. Under strict compiler `.copy()` ceilings, authors end up writing `return "{line}"` or `items.push("{value}")` — **still allocating a full copy, just disguised as string interpolation!**
  - Returning a tiny all-scalar struct like `Complex` (two `Float`s) fails without `.copy()` unless that type was hand-listed in the compiler's internal `Copy.jet` allowlist.
- **Proposed fixes / ideas:**
  - Infer `Copy` **structurally** for small plain all-scalar structs (any `struct` whose fields are all `Copy` and has no `#Close` / `#MoveOnly`), deleting the manual `Copy.jet` allowlist.
  - Make `View<String>` / borrowed returns effortless for pass-through helpers (or allow returning a read param directly when the return type is `View<String>`).
  - Auto-move last-use local variables into `^^` parameters without forcing `.move()` noise.

#### 28. Over-strict `UseWhileWriteActive` on sequential nested calls — `ACCIDENTAL FRICTION`
- **What happens:**
  ```jet
  // Fails with UseWhileWriteActive, even though the inner read completes first:
  jfmt_write(^f, jfmt_float_literal_spelling(f, span, value, raw))
  ```
  Even though `jfmt_float_literal_spelling(f, ...)` finishes completely before `jfmt_write(^f, ...)` starts mutating `f`, the checker refuses it and forces hoisting a temporary variable `spelling: ...` onto the line above.
- **Proposed fixes / ideas:**
  - Implement Rust-style **two-phase borrows** for method/function calls: reserve the `^arg` write place during argument evaluation, and activate exclusivity only when the callee actually executes.

#### 29. Capturing `Shared<T>` into multiple tasks/closures requires manual `.copy()` shadow variables — `ACCIDENTAL FRICTION`
- **What happens:** Even though `Shared<T>` exists specifically to be shared across tasks, spawning two tasks requires manual shadow bindings outside the closures (`h1: hits.copy(); h2: hits.copy(); task { ... h1 ... }; task { ... h2 ... }`).
- **Proposed fixes / ideas:**
  - Either allow an explicit capture list (`task[hits] { ... }`) that cheaply bumps `Shared<T>`'s handle, or auto-clone `Shared<T>` handles on task/closure capture (it is already an explicitly shared reference-counted handle).

#### 30. Non-obvious `.in_place()` and `^guard:` mutability diagnostics — `ACCIDENTAL FRICTION`
- **What happens:** Mutating a value inside an `Option`/enum requires knowing the non-obvious `.in_place()` method (`if x.in_place() == .Val(v)`), and mutating through a `Shared<T>` lock fails unless you write `^guard: cell.guard_edit()`.
- **Proposed fixes / ideas:**
  - Have the `ReadOnlyValueNotOwned` / immutability diagnostic on `Option`/`GuardMut` directly suggest the exact spelling (`add ^ before guard: ^guard: cell.guard_edit()` or `use .in_place()`).

---

### E. Error Handling Rough Edges

*(See [Section 4](#4-deep-dive-jet-error-handling-vs-go-error-handling) for the full Jet vs. Go comparison!)*

#### 31. Bare `fn() -> T` silently means `T Err!` instead of infallible (`Never!`) — `COUNTERPRODUCTIVE`
- **What happens:** Omitting the error rail on `fn(n: Int) -> Int` silently makes the function fallible (`Int Err!`), forcing almost every pure/internal helper in `Compiler/` and `Core/` to repeat `Never!` on every single signature (`-> Bool Never!`, `-> Int Never!`). On the older seed (before `#7034` `?` marks), omitting `Never!` also silently widened inferred error sets across 4 call frames.
- **Proposed fixes / ideas:**
  - Keep `run: fn()` and `#Test` defaulting to `Err!` (beginner/script convenience!), while letting library/internal `fn` either default to infallible or check clearly with `#7034` `?` marks so unannotated helpers never silently widen their error type.

#### 32. `#Error` boilerplate (`impl Err.From(MyErr)`) & misleading `Result` vs. `Option` diagnostics — `ACCIDENTAL FRICTION`
- **What happens:**
  - Declaring `#Error TaskStateError` with `Display` and `Describe` still fails in `run` / `#Test` with `MissingErrorConversion` unless you manually write `impl Err.From(TaskStateError)`. Meanwhile `SendError` converts automatically and `TaskFailure` does not.
  - Writing `MyErr: error { ... }` gives a confusing record-field error (`Expected ',' after this record entry`).
  - Matching `.Val`/`.None` on a `Result` (`rx.receive()` or `Int.parse()`) prints `MissingErrorConversion` or `Pattern Val doesn't match Int` instead of saying *"this returns a `Result`, not an `Option`"*.
- **Proposed fixes / ideas:**
  - Auto-derive `impl Err.From(E)` for any `#Error` type implementing `Describe` (zero boilerplate for `run`/`#Test`, while still requiring explicit conversions between *distinct* typed domain error enums!).
  - Add dedicated diagnostics when `.Val`/`.None` is matched against a `Result` (suggest `.Ok`/`.Err`) or when `Name: error { ... }` is written (suggest `#Error Name: enum { ... }`).

#### 33. Nested `Result` ergonomics (`(T E!) TaskFailure!`) & `if call() == { .Ok ... }` suppression subtlety — `ACCIDENTAL FRICTION`
- **What happens:** `Task<Int DomainError!>.join()` returns `(Int DomainError!) TaskFailure!`, which is awkward to bind or unwrap without passing the handle to a helper function. Note: **`#7034` (`?` marks) already fixed** the biggest half of this (`call()?` propagates, `call()` without `?` stays a `Result` value you can match directly!).
- **Proposed fixes / ideas:**
  - With `?` now explicit on propagation, make matching `if handle.join() == { .Ok(.Ok(v)) -> ... .Ok(.Err(e)) -> ... .Err(tf) -> ... }` work cleanly without helper wrappers, and ensure `task.all` / `task.race` / `task.any` support `Task<T E!>` uniformly.

---

### F. Collections, Strings, Bytes & Stdlib Ergonomics

#### 34. Empty collection / `None` literals require repeating known types (`[Int]{}`, `Int?{None}`) — `ACCIDENTAL FRICTION`
- **What happens:** Even inside a struct literal or typed parameter where the type is already known (`IROptOptions{disabled: [String]{}}`, `^idx: Int?{None}`, `Shared<[Int]>{[Int]{}}`), you must repeat the full type head on every empty list, map, or `None`.
- **Proposed fixes / ideas:**
  - Infer `[]`, `[:]`, and `None` whenever an expected `[T]`, `[K:V]`, or `T?` type is known from a struct field, parameter, binding, or return type.

#### 35. `Map` lacks `.contains_key(k)`, and no ergonomic `Set<T>` literal exists — `ACCIDENTAL FRICTION`
- **What happens:** Lists have `.contains(x)`, while maps force `seen.get(k) == None` everywhere. Because `Set<T>` has no lightweight literal/ergonomics, the entire compiler uses `[String:Bool]{}` and `[U32:Bool]{}` as makeshift sets (`seen[k] = true`).
- **Proposed fixes / ideas:**
  - Add `Map.contains_key(k)` (or `.has(k)`) and lightweight `Set<T>` literals / `.insert()` / `.contains()` in Prelude/Core.

#### 36. `String` stdlib rough edges (`find` returns `-1`, no `StringBuilder`, `split` returns iterator, no `byte_slice` / `from_byte`) — `ACCIDENTAL FRICTION`
- **What happens:**
  - `String.find` / `rfind` returns `-1` instead of `Int?` (contradicting Jet's own "no sentinel values" law!).
  - Building strings either does quadratic `out = "{out}{piece}"` or manually pushes `U8` into `[U8]{}` + `String.from_bytes(...) ?? ""`.
  - `s.split("\n")[0]` fails because `split` returns an iterator (needs `.to_list()`).
  - Emitting one byte (like ESC `27`) requires `String.from_bytes([U8]{27}) ?? ""`.
  - `len()` vs. `count_bytes()` vs. `slice(a..<b)` leads every package to roll its own clamped byte slicer.
  - `Float` `Display` prints `2.0` while `Complex` strips `.0` (`2 + 3i`).
- **Proposed fixes / ideas:**
  - Return `Int?` from `String.find` / `rfind`.
  - Add `StringBuilder` (or `^s.push_str(...)`), `String.from_byte(b: U8)`, `String.byte_slice(a..<b)`, and a targeted fix hint when indexing an iterator (*"call `.to_list()` first"*).
  - Document `Float` vs. `Complex`/`Quaternion` display formatting in one reference table.

#### 37. No in-place mutable element loop (`loop ^item in list`) — `ACCIDENTAL FRICTION`
- **What happens:** Mutating elements of a list in place forces an index loop (`loop i in 0..<arms.len() { arms[i].body = ... }`) instead of `loop ^arm in arms`.
- **Proposed fixes / ideas:**
  - Support `loop ^item in list` yielding a mutable borrow to each element in place.

#### 38. List literals `[a, b]` move elements without suggesting `.copy()` cleanly — `ACCIDENTAL FRICTION`
- **What happens:** Building a quick inspection list `[first, second]` of non-Copy values moves them and causes `UseAfterMove` later.
- **Proposed fixes / ideas:**
  - Keep strict move semantics, and enhance `UseAfterMove` hints to suggest `.copy()` when the element type implements `Clone`.

---

### G. Concurrency, Effects & Imports

#### 39. Rigid explicit effect lists cause cross-package churn (`#6861`) & unclear callback effects — `ACCIDENTAL FRICTION`
- **What happens:** Adding a legitimate low-level read effect (`Env` / `FS.Read`) to `Core/Net/TLS.jet` (e.g., reading system CA roots) immediately broke `Core/HTTP/Client.jet` and `Server.jet` because those callers spelled out explicit leaf ceilings (`-[Net, Time.Wait]>`). Separately, on callback types like `ask: fn(^Session, Int) -> Bool`, it's hard to predict without trial-and-error whether the callback is pure or effect-polymorphic.
- **Proposed fixes / ideas:**
  - Support effect bundles / aliases (e.g. `Net.TLS`) or effect-row polymorphism on higher-order helpers and traits so internal leaf additions don't ripple across every transitive caller.
  - Show inferred/allowed effect rows clearly on `fn(...)` types in LSP hover and diagnostics.

#### 40. Mandatory unused group name in `task.group g { ... }` & `task.all` shapes — `ACCIDENTAL FRICTION`
- **What happens:** `task.group workers { h: task foo() }` forces inventing an unused identifier `workers` even when bare `task` implicitly joins the group. Meanwhile `task.all` has 3 subtly different shapes (`task.all { a(), b() }`, `task.all { left: a(), right: b() }`, `task.all([t1, t2])`).
- **Proposed fixes / ideas:**
  - Allow anonymous `task.group { ... }` and `task.group(limit: 2) { ... }` when the group handle isn't referenced.

#### 41. Opaque transitive `authority: { allow: [...] }` requirements — `TOOLING QUICK WIN`
- **What happens:** Single-file programs using `core.service` or `core.data` have to guess/discover which transitive effects (`[Env, FS, IO, Rand, Time, Time.Wait]`) to add to `authority: { allow: [...] }`.
- **Proposed fixes / ideas:**
  - Emit a 1-line copy-pasteable `authority: { allow: [...] }` fix-it listing the exact missing capabilities directly inside `Jet.ProhibitedEffectReached`.

#### 42. Import alias shadowing, non-obvious `pkg.[a, b]` syntax, & opaque `E0003` — `ACCIDENTAL FRICTION`
- **What happens:**
  - `use files: core.files` collides with locals named `files`, forcing ugly prefixed aliases (`td_files`, `cb_files`, `service_time`) everywhere.
  - Writing `use core.files as f` or `jet_cli[a, b]` (missing the `.`) gives a generic syntax error.
  - `E0003` (*"An import declaration has no resolved graph edge"*) doesn't say *which* import failed or *where* Jet searched.
- **Proposed fixes / ideas:**
  - Add targeted parser hints for `use ... as ...` and `pkg[a, b]`.
  - Print the exact unresolved module path and searched directories/registries in `E0003`.
  - Consider letting module aliases live cleanly alongside local parameter names (or allowing direct `core.files.read(...)` without mandatory aliasing).

#### 43. `Jet.UnusedImport` is a hard compile error instead of a warning (and `priv` vs. `_` vs. `__` confusion) — `ACCIDENTAL FRICTION`
- **What happens:** During refactoring, leaving 1 unused import blocks `jet check` with exit code 1 (and untouched files with stale unused imports break whole-package `jet check`). Separately, `_extracted` (underscore prefix) vs. `priv foo` vs. `__reserved` (hard error `E0067`) creates three overlapping naming conventions.
- **Proposed fixes / ideas:**
  - Demote `UnusedImport` to a warning during dev `jet check` (keeping `--deny-warnings` for CI) and add auto-pruning in `jet fix` / `jet fmt`.
  - Standardize on `priv` for visibility and `_` strictly for intentionally unused locals/params.

---

### H. Low-Level & Runtime Code

#### 44. Raw `wrapping(ptr + I64{152})` magic byte offsets in `JetRuntime` — `ACCIDENTAL FRICTION`
- **What happens:** Runtime files (`Task.jet`, `Channel.jet`) are full of error-prone `sync_load(wrapping(child + I64{152}))` raw byte offsets because typed `#Layout(c)` struct pointers aren't ergonomic inside `JetRuntime`.
- **Proposed fixes / ideas:**
  - Support `#Layout(c)` typed struct pointer field reads/writes and pointer `.add(n)` helpers inside `#Unsafe` runtime blocks (combined with item #14 integer literal coercion).

#### 45. Public-API style lints (`L2401`, `L2421`) firing on small examples and test helpers — `COUNTERPRODUCTIVE`
- **What happens:** Top-level items default to public (`priv` opt-in). In a 10-line concurrency example, `boom: fn() -> Int { panic("..."); 0 }` drew `L2421` ("declare `Never`"); changing it to `boom: fn(should_panic: Bool)` then drew `L2401` ("positional `Bool` parameter"), forcing an ugly `priv boom: fn(mode: Int)` just to stay warning-free.
- **Proposed fixes / ideas:**
  - Exempt `#Test` files and `run`-entry examples/scripts from public-library API lints (`L2401`, `L2421`), or apply them only to exported library APIs.

---

### I. Compiler Architecture & Core Library Coupling

#### 46. Bare vs. `<corelib>/...` nominal type identity bug + 6-table Core coupling — `BUG / COMPILER GAP`
- **What happens:**
  - Adding one pure-Jet value struct to `Core/Math/Math.jet` required manually editing **6 separate compiler tables** (`Core.rows` 2,000-char single-line rows, `CoreModules.jet`, `Copy.jet`, `Loader.jet`, `CoreUsage.jet`, plus `Expressions.jet`).
  - `type_identity_equal` compares `.Named(name)` by raw string equality, so built-in `1i` (`"Complex"`) did not equal `"<corelib>/core.math.Complex"` resolved inside `Core/Math/Math.jet`, silently breaking `1k * 1i`!
  - `jet check Core/Tasks/Tasks.jet` or `Core/App/App.jet` directly fails with false `BuiltinsOutsideCore` / `ReservedDunderName` errors because single-file `jet check` doesn't recognize `Core/package.jet` as `<corelib>` the way `jet test` does.
  - Because `Core/` is baked into the compiler binary, `jet test Core/X.jet` tests your edited file while `jet run` silently loads the **stale embedded copy** in the seed binary!
- **Proposed fixes / ideas:**
  - Normalize nominal type identities in `type_identity_equal` (one canonical type ID, never raw string mismatch between bare and `<corelib>/...`).
  - Auto-generate `Core.rows` / `CoreModules.jet` from `Core/` declarations (or format `Core.rows` one export per line).
  - Make `jet check Core/<file>.jet` recognize `Core/package.jet` identically to `jet test`.
  - Split massive monoliths (`Core/Math/Math.jet` at 46,700 lines, `Expressions.jet` at 8,500 lines, `Loader.jet` at 8,000 lines) into focused submodules.

#### 47. Single declaration typos cascade into 20+ bogus downstream errors — `TOOLING QUICK WIN`
- **What happens:** One syntax typo (`DomainError: error { ... }`) triggered 20 follow-on errors (`E0119`, `E2417`, `E0107`, `E1102`, plus bogus `L0102: Parameter label is never used` on functions whose bodies never checked).
- **Proposed fixes / ideas:**
  - Poison/taint unresolved declaration symbols cleanly so downstream type/sendability/unused-param diagnostics stay silent on broken items.

---

### J. CLI, Testing, Formatting & Diagnostics Tooling

#### 48. `jet check` warning floods & missing `--errors-only` / `--file` filtering — `TOOLING QUICK WIN`
- **What happens:** `jet check Compiler/JetCLI` prints 1,160+ warnings (~10,300 lines) and `jet check Compiler/JetSema` prints 4,340+ warnings (~39,000 lines), completely burying 1–2 real errors.
- **Proposed fixes / ideas:**
  - Add `jet check --errors-only` (and `--file <path>` to filter warnings to touched files).
  - Always print `Error:` diagnostics **after** warnings so errors sit cleanly at the bottom of terminal output.

#### 49. `jet test` footguns (silent exit 0 on 0 `--filter` matches, file path runs whole package, `.case` files crash, truncated assert diffs, missing `#Test` caller frame) — `TOOLING QUICK WIN`
- **What happens (every agent hit at least 3 of these!):**
  - `jet test foo.jet --filter "a|b"` (substring-only) prints `0 passed, 0 failed` and **exits 0**, looking like a passing test!
  - `jet test Tests/Guards/panic_ratchet.jet` (passing a specific file inside a package) runs **every file in the entire package** unless `--filter` is also passed.
  - `jet test Tests/Cases/Foo/bar.case` tries to parse the `.case` file as Jet source (`UnterminatedText` on `#` comments) instead of running the case.
  - `--compiler /path` fails (`--compiler needs a value`, must write `--compiler=/path`), while `-O0` takes no `=`.
  - Failed `assert(s.contains(...))` truncates the actual string right before the mismatching line; panics inside test helpers omit which `#Test(...)` called them.
  - `.case` files reject `stdout: ~ "..."` without an inline `# reason` comment, without telling you to add `# reason`.
- **Proposed fixes / ideas:**
  - Exit non-zero whenever a user-provided `--filter` matches 0 tests (with an opt-in `--allow-empty`), and support consistent filter semantics across all test modes.
  - Default `jet test <single_file.jet>` to running only `#Test` blocks in that file, and let `jet test <file.case>` run `.case` files directly.
  - Accept both `--flag value` and `--flag=value` uniformly across every `jet` subcommand.
  - Show full diffs / mismatching context lines on failed `assert` / `assert_eq`, include the `#Test(...)` caller frame on panic, and add a `jet test --help-case` DSL cheat-sheet.

#### 50. Incremental edit-compile-test latency & `jet fmt` workflow gaps — `TOOLING QUICK WIN`
- **What happens:**
  - Editing 1 line in `Compiler/JetCLI/Tests/*.jet` or `JetSema/Tests/*.jet` takes **2–5 minutes** to recompile and run 1 filtered test.
  - `jet fmt` lacks `--stdout` (for checking formatting without mutating) and `--check-lines` (so pre-existing formatting drift elsewhere in a file doesn't block checking your touched lines).
  - Some REPL/Fmt goldens still display `:=` while source syntax moved to `^name:`.
  - No quick `jet inspect exports <pkg>` / `jet doc <symbol>` lookup command to find where a Core/compiler helper lives.
- **Proposed fixes / ideas:**
  - Cache compiled package library units cleanly so editing a single `Tests/*.jet` file only recompiles that test file (target: **1–3 seconds**).
  - Add `jet fmt --stdout`, `jet fmt --check-lines`, and `jet inspect exports <package>`.
  - Align REPL display output (`^alpha: Int = 1` vs. `alpha: Int := 1`) once across all goldens.

---

## 4. Deep Dive: Jet Error Handling vs. Go Error Handling

### Why Go's error handling is widely praised — and why programmers still complain about it

People praise Go's error handling for **4 genuinely great semantic ideas**:
1. **Errors are ordinary values, not invisible exceptions:** You can pass, wrap, inspect, log, and test them like any other data.
2. **Every failure exit is visible right in the code:** Reading a Go function top-to-bottom shows every place where it can return early — critical when auditing locks, files, or partial state mutations.
3. **Adding context as errors bubble up is natural:** `fmt.Errorf("load config %s: %w", path, err)` builds a clean story for the caller.
4. **Zero hidden runtime unwind machinery:** Predictable performance and simple control flow.

Yet Go programmers still complain constantly because **Go's *syntax* and *type system* carry 5 real flaws**:
1. **Relentless boilerplate:** Every single fallible call takes 3 lines (`v, err := f(); if err != nil { return zero, err }`), burying the happy path under 50% error plumbing.
2. **Untyped `error` interface loses exhaustiveness:** A function signature `(*Config, error)` doesn't tell you *which* errors can happen. Callers have to guess with `errors.Is` / `errors.As` and silently miss new error cases.
3. **Zero-value hazard:** Because Go returns `(T, error)` as a *pair* (product type) instead of a *choice* (sum type), nothing stops a caller from ignoring `err` and accidentally reading the uninitialized zero value of `T` (often a `nil` pointer -> runtime panic!).
4. **Easy to silently drop an error:** Calling `file.Close()` or `os.Remove(p)` without checking `err` compiles with zero warnings.
5. **No built-in audience separation:** Internal debug details (SQL queries, stack paths, file descriptors) get concatenated straight into `err.Error()`, which accidentally leaks into end-user UI or HTTP responses.

---

### Side-by-Side Comparison: Go vs. Jet

#### 1. Happy path + wrapping context + local fallback

**Go (17 lines, 3 noisy `if err != nil` blocks, zero-value returns):**
```go
func loadPort(path string) (int, error) {
    raw, err := os.ReadFile(path)
    if err != nil {
        return 0, fmt.Errorf("read config %s: %w", path, err)
    }
    cfg, err := parseConfig(string(raw))
    if err != nil {
        return 0, fmt.Errorf("parse config %s: %w", path, err)
    }
    port, err := strconv.Atoi(cfg.Port)
    if err != nil {
        port = 8080 // local fallback
    }
    return port, nil
}
```

**Jet (5 lines — every single failure exit is STILL 100% visible!):**
```jet
load_port: fn(path: String) -> Int ConfigError! {
    raw: files.read_text(path)?("read config {path}")
    cfg: parse_config(raw)?("parse config {path}")
    Int.parse(cfg.port) ?? 8080
}
```

Look at what Jet achieves over Go in those 5 lines:
- **Same local visibility as Go** (every early exit is marked right at the call site with `?` or `?(context)` — you can audit locks and partial mutations at a glance!), **with ~70% less noise**.
- **Impossible to read a zero-value `raw` or `cfg` on error** — Jet's `T E!` is a true sum type (`Result`), not a `(T, error)` tuple.
- **Impossible to silently ignore an error** — leaving off `?` or `??` is a compile error (`Jet.FailureMarkRequired` / must-use).
- **One clean local-fallback operator (`??`)** instead of re-assigning after `if err != nil`.

#### 2. Exhaustive typed errors & two-audience messages (where Jet leaves Go behind)

**Go:** Callers must know by tribal memory which sentinel errors exist, and internal details leak into `err.Error()`:
```go
if errors.Is(err, ErrNotFound) { ... } // compiler can't tell you if you missed ErrPermission!
```

**Jet:** Every error family is a typed `#Error` enum with exhaustive matching AND built-in two-audience `Describe` (`public` for end users vs. `detail` for operators/logs):
```jet
#Error
ConfigError: enum {
    Missing(String),
    BadPort(String),

    impl Describe {
        public: fn(self) -> String Never! { "Configuration could not be loaded." }
        detail: fn(self) -> String Never! {
            if self == {
                .Missing(p) -> "config file missing: {p}"
                .BadPort(v) -> "invalid port value: {v}"
            }
        }
    }
}
```
If you add `.Expired` to `ConfigError` tomorrow, Jet flags every caller matching `ConfigError` at compile time (unless they explicitly opted into `#CatchAll`). Go cannot do this at all.

---

### Feature-by-Feature Scorecard: Go vs. Jet Today vs. Jet After 5 Small Fixes

| Capability | Go | Jet Today (Batch 89) | Jet After the 5 Fixes Below |
|---|---|---|---|
| Errors are ordinary values (no hidden exceptions) | Yes | **Yes** | **Yes** |
| Every early exit visible at the call site | Yes (`if err != nil`) | **Yes (`?` / `?(ctx)`)** | **Yes** |
| Low happy-path boilerplate | **No** (3 lines per call) | **Yes** (1 char `?`) | **Yes** |
| Context wrapping on propagation | Manual (`fmt.Errorf`) | **Built-in (`?("msg")`)** | **Built-in** |
| Local default / fallback | 3-line `if err != nil` | **1 expression (`??`)** | **1 expression (`??`)** |
| Impossible to read invalid `T` when error occurred | **No** (zero-value/nil bug) | **Yes** (`Result` sum type) | **Yes** |
| Impossible to silently ignore an error | **No** | **Yes** | **Yes** |
| Exhaustive typed error cases | **No** (`error` interface) | **Yes** (`#Error` enum) | **Yes** (plus `#CatchAll`) |
| Safe public vs. internal error text | **No** | **Yes** (`Describe`) | **Yes** |
| Predictable infallible-by-default library functions | Yes (no `error` return = infallible) | **No** (`fn() -> Int` secretly means `Int Err!`) | **Yes** |
| Zero boilerplate using custom `#Error` in `run` / `#Test` | Yes (`error` satisfies `error`) | **No** (needs manual `impl Err.From(MyErr)`) | **Yes** (auto-lift from `#Error` + `Describe`) |
| Crystal-clear `Result` vs. `Option` mismatch errors | N/A | **No** (confusing `MissingErrorConversion`) | **Yes** |

### The 5 Small Fixes That Make Jet Strictly Easier AND Better Than Go

Jet's error semantics are **already** strictly stronger than Go's, and `#7034` (`?` marks) just gave Jet Go's single best property (visible early exits) without Go's boilerplate. Smoothing these **5 rough edges** finishes the job:

1. **Stop making unmarked library functions silently mean `Err!`:**  
   In Go, `func add(a, b int) int` is obviously infallible. In Jet, `add: fn(a: Int, b: Int) -> Int` silently means `Int Err!`, forcing `Never!` onto thousands of pure helpers. Keep `Err!` as the friendly default for `run: fn()` and `#Test` (so scripts/beginners write zero error annotations), and let ordinary library functions be infallible when no `!` rail is written!
2. **Auto-convert `#Error` types into `Err` at `Err!` boundaries:**  
   Anyone defining `#Error MyError` with `Describe` should be able to use `?` inside `run: fn()` or `#Test` immediately with **zero** manual `impl Err.From(MyError)` boilerplate — just like Go's `error` interface works effortlessly at the top level — while still requiring explicit `impl TargetErr.From(SourceErr)` between *two specific domain error enums*!
3. **Give unmistakable diagnostics for `Result` vs. `Option` mix-ups:**  
   When someone matches `.Val`/`.None` on a `T E!` (or `.Ok`/`.Err` on a `T?`), say plainly: *"`rx.receive()` returns a `Result` (`T Closed!`), not an `Option` — match `.Ok(v)` / `.Err(e)` or propagate with `?`"*.
4. **Make nested `Result` (`(T E!) TaskFailure!`) effortless to unwrap:**  
   Ensure `h.join()?` unwraps the outer `TaskFailure!` cleanly into `T E!` so you can write `result: h.join()?` or `val: h.join()??` (or clean nested `.Ok(.Ok(v))` matching) without inventing helper functions.
5. **Make `??` handle block/error binding cleanly everywhere:**  
   Keep `val: f() ?? { log(err); return fallback }` rock-solid across all expression positions (including inside `task { ... }` and lambdas) — that replaces Go's 3-line `if err != nil { log(err); return fallback }` with a single, clean, readable idiom that binds `err` only on the failure branch.

**Verdict vs. Go:** With `?` / `?(context)` + `??` + typed `#Error` + `Describe` + those 5 smoothing fixes, Jet keeps **100% of what people love about Go's errors** (explicit values, visible early returns, local control flow, easy context wrapping) while eliminating **100% of what people hate about Go's errors** (`if err != nil` spam, untyped `error`, nil/zero-value leaks, silently dropped errors, leaked internal messages).

---

## 5. What Already Works Really Well (Do Not Regress)

Every agent independently praised these parts of Jet — keep them protected as the language evolves:

- **Local `Why:` / `Fix:` diagnostics:** When errors point at the true local cause (`Jet.UnknownArgumentLabel`, `Jet.ClosedSetElse` listing missing variants, `Jet.UseWhileWriteActive` pinpointing the exact column), agents rated Jet's diagnostics **among the best of any compiler**.
- **Power-asserts:** Showing intermediate subexpression values and local variables inline on test failures makes debugging tests dramatically faster than Go, Rust, or Zig.
- **Field-precise access (`^self.[hits]`) and block footprints (`jet inspect footprint`):** Seeing exactly what a block reads, writes, and takes with zero lifetime annotations is a genuine breakthrough.
- **One meaning across O0, O1, and O2:** No separate interpreter with subtle semantic drift.
- **Structured `task.group` with `limit:`, cancellation, deadlines, and panic isolation:** Far cleaner and safer than Go goroutines or Rust's `tokio` + `Send + Sync + 'static`.
