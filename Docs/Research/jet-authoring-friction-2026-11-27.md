# Writing Good Code in Jet: Open Authoring Friction TODO List & Jet vs. Go Errors

*Date: 2026-11-27 (Living TODO — items are removed from this list once approved, carded as P0 on Tower, and ready to implement)*  
*Sources: Unfiltered, independent feedback from three engineering agents who wrote thousands of lines of real Jet across `Core/`, `Compiler/`, and DX/Tooling, plus live verification against the compiler and Tower.*

> **Already Approved & Carded as P0 on Tower (removed from this TODO list):**
> - **#7042 [P0]:** `A1` (`#CatchAll`), `B12` (interpolation ICE), `B14` (contextual `U8`/`I64`/`U64` literal coercion), `B15` (`I64.MIN` literal + constants), `C20` (`.Variant` shorthand propagation), `C21` (struct update syntax)
> - **#7043 [P0]:** `E32` / `Go-2` (`#Error` + `Describe` auto-lift to `Err`), `E32` / `Go-3` (`Result` vs. `Option` & `error {}` diagnostics), `F35` (`Map.contains_key` + `Set<T>`), `F37` (`loop ^item in list`), `G41` (copy-pasteable `authority:` fix-it)
> - **#7044 [P0]:** `C23` (`Neg` operator trait), `D26` (`_: expr` read discard), `H44` (`#Layout(c)` typed struct pointers in `#Unsafe`)
> - **#7045 [P0]:** `B10` Part 1 (Option 1A: bare `Float` default, exact contextual `Decimal`/`Float`, `Type{value}` scalars, `#UnitFamily(Time)` `hour`/`day` replacing `h`/`d`, delete magic `f` suffix)
> - **#7046 [P0]:** `B10` Part 2 (Unified unquoted `Type{...}` / `Type{{{...}}}` `DSL` trait: type-owned `parse`, hole spans, LSP Semantic Tokens `highlight`, `jet fmt`, cutover of `SQL`/`HTML`/`Sh`/`Regex`/`Path`/`URL`/`DateTime`/`Grammar`/`Complex`/`Quaternion` + FFI foundation, delete magic `i`/`j`/`k` suffixes)
>
> **Remaining in this TODO list for owner discussion & decision:** **37 items** (`A2–A8`, `B9`, `B11`, `B13`, `B16–B17`, `C18–C19`, `C22`, `C24–C25`, `D27–D30`, `E31`, `E33`, `E51–E53`, `F34`, `F36`, `F38`, `G39–G40`, `G42–G43`, `H45`, `I46–I47`, `J48–J50`). Item numbers are kept stable as items are removed.

---

## Table of Contents

1. [The Bottom Line: Would Fixing These Make Jet Near-Perfect?](#1-the-bottom-line-would-fixing-these-make-jet-near-perfect)
2. [Productive Friction Worth Keeping (What Forces *Good* Code)](#2-productive-friction-worth-keeping-what-forces-good-code)
3. [Remaining TODO Items to Discuss & Decide (37 Items)](#3-remaining-todo-items-to-discuss--decide-37-items)
   - [A. Control Flow, Patterns & Matching (`A2–A8`)](#a-control-flow-patterns--matching)
   - [B. Syntax, Braces, Literals & Strings (`B9`, `B11`, `B13`, `B16–B17`)](#b-syntax-braces-literals--strings)
   - [C. Types, Structs, Enums, Generics & Signatures (`C18–C19`, `C22`, `C24–C25`)](#c-types-structs-enums-generics--signatures)
   - [D. Ownership, Borrowing & Copying (`D27–D30`)](#d-ownership-borrowing--copying)
   - [E. Error Handling & Jet-vs-Go Remaining Items (`E31`, `E33`, `E51–E53`)](#e-error-handling--jet-vs-go-remaining-items)
   - [F. Collections, Strings, Bytes & Stdlib Ergonomics (`F34`, `F36`, `F38`)](#f-collections-strings-bytes--stdlib-ergonomics)
   - [G. Concurrency, Effects & Imports (`G39–G40`, `G42–G43`)](#g-concurrency-effects--imports)
   - [H. Lints & Low-Level Code (`H45`)](#h-lints--low-level-code)
   - [I. Compiler Architecture & Core Library Coupling (`I46–I47`)](#i-compiler-architecture--core-library-coupling)
   - [J. CLI, Testing, Formatting & Diagnostics Tooling (`J48–J50`)](#j-cli-testing-formatting--diagnostics-tooling)
4. [Reference: Jet Error Handling vs. Go Error Handling](#4-reference-jet-error-handling-vs-go-error-handling)
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
1. **Incremental compile & test speed (the #1 real-world make-or-break):** Sub-second incremental `jet check` and targeted `jet test` will improve real-world Jet code quality more than any single syntax tweak.
2. **Cognitive budget & orthogonality (avoiding "C++/Scala syndrome"):** Keep the language small and orthogonal so programmers can *predict* how features combine instead of memorizing compiler special cases.
3. **The borrowing middle ground (`View<T>` and borrowed `String`):** Make returning/passing borrowed views effortless so authors never disguise copies as `"{line}"`.
4. **Effect-system ergonomics at large codebase scale:** Let higher-order functions and layered libraries abstract over or bundle effects cleanly so leaf-effect additions don't ripple across packages.

---

## 2. Productive Friction Worth Keeping (What Forces *Good* Code)

All three agents **explicitly asked Jet to KEEP** these guardrails — annoying for 10 seconds, then make the resulting code objectively safer and clearer:

| Guardrail | Why it forces *better* code (KEEP) |
|---|---|
| **Visible `^` and `^^` on bindings, parameters, and call sites** | Every mutation (`bump(^x)`) and ownership transfer (`send(^^msg)`) is readable right at the call site. Zero hidden side effects. |
| **Visible `?` on every propagating fallible call** *(shipped in Batch 89!)* | Every early exit is visible at the call site; prevents silent error leaks 4 frames away. |
| **Exhaustive enum matching by default** *(paired with P0 `#CatchAll` in `#7042`)* | Catches forgotten enum variants when types grow, while `#CatchAll` handles deliberate opt-outs. |
| **Refusing never-written `^x` bindings (`E0514`)** | Keeps immutability honest: `^x` always means `x` actually changes. |
| **Strict `Decimal` vs. `Float` separation on variables** | Prevents silent financial/binary-float rounding bugs. |
| **Checked arithmetic, `/%` vs. `/`, and arbitrary-precision `Int` default** | Prevents silent integer overflow and silent integer-division truncation bugs. |
| **Typed `#Error` enums + explicit `impl Target.From(Source)` across domain boundaries** | Prevents Go-style stringly-typed error soup across layers. |
| **Checked effects (`-[IO, Net, Time.Wait]>`) & capability `authority:`** | Forces clean architectural separation between pure logic and I/O/network code. |
| **Structured concurrency (`task.group`, `limit:`, panic isolation, lossless `SendRefused<T>`)** | Eliminates leaked tasks, silent drops, and sentinel `-1` return codes. |
| **Mandatory `.drop("reason")` on linear/resource/error values** | Forces a real justification when discarding an error or handle. |
| **Explicit `.copy()` visibility** | Prevents silent O(N) deep-copy regressions. |

---

## 3. Remaining TODO Items to Discuss & Decide (37 Items)

Legend:
- `COUNTERPRODUCTIVE` = actively pushes authors to write *worse* code.
- `ACCIDENTAL FRICTION` = costs time/readability while preventing zero bugs.
- `BUG / COMPILER GAP` = genuine defect or incomplete implementation.
- `TOOLING QUICK WIN` = CLI/test/diagnostic improvement with zero language risk.

---

### A. Control Flow, Patterns & Matching

#### A2. Value-producing `if` requires `->`, while statement `if` uses `{}` — `COUNTERPRODUCTIVE`
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

#### A3. Multi-statement `task { ... }` blocks can't return a value naturally — `BUG / COMPILER GAP` · `COUNTERPRODUCTIVE`
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

#### A4. Deep nested `if` ladders for multi-step pattern unwrapping — `ACCIDENTAL FRICTION`
- **What happens:** Checking an `Option` whose payload contains another `Option` or `Result` requires 3–4 levels of nested `if` blocks (`if a == .Val(x) { if f(x) == .Val(y) { ... } }`), because `if a == .Val(x) && f(x) == .Val(y)` and guard-clause unwrapping on arbitrary enum variants aren't generally supported.
- **Proposed fixes / ideas:**
  - Support chained pattern guards (`if a == .Val(x) && x > 0 && f(x) == .Val(y) { ... }`).
  - Generalize guard-clause extraction (`Val(x): opt ?? return`) or `else` guards cleanly across `Option`/`Result`/enum variants.

#### A5. Competing `if` / `else` forms & overly rigid `Jet.SameSubjectDispatch` lint — `ACCIDENTAL FRICTION`
- **What happens:** `if { c1 -> a; c2 -> b; else -> c }`, `if c1 -> a else if c2 { b }`, and `if x == { ... }` coexist with subtle arrow/brace rules. Switching one branch from 1 statement to 2 forces rewriting both branches. Meanwhile, `Jet.SameSubjectDispatch` warns even on flat guarded early-return `if` statements (`if key.kind == F1 { ... } if key.kind == CHAR && key.text == "q" { ... }`).
- **Proposed fixes / ideas:**
  - Allow `{ ... }` uniformly on single-statement branches without forcing `->` churn.
  - Exempt guarded or early-return `if` statements with heterogeneous conditions from `SameSubjectDispatch`.

#### A6. Finite value-loop `??` requirement (`E0078`) has an unclear span — `ACCIDENTAL FRICTION`
- **What happens:** `found: loop x in xs { if ok(x) -> break x }` requires a trailing `?? fallback` after the closing `}`, and the `E0078` error points at the closing `}` instead of the `loop` header.
- **Proposed fixes / ideas:**
  - Anchor `E0078` on the `loop` keyword with a clear fix-it showing `?? default_value` after the closing brace.

#### A7. Three competing discard idioms (`_:`, `^_:`, `.drop("reason")`, `?? ()`) — `ACCIDENTAL FRICTION`
- **What happens:** Ignoring a value sometimes requires `_: expr`, sometimes `^_: list.pop()`, sometimes `.drop("reason")`, and sometimes `?? ()`. It's hard to predict which one the compiler wants.
- **Proposed fixes / ideas:**
  - Unify non-linear discards under plain `_: expr` (never requiring `^_:`).
  - Reserve mandatory `.drop("reason")` strictly for linear/resource/error values (handles, tasks, `Result`).

#### A8. `next` keyword collides with common variable names — `ACCIDENTAL FRICTION`
- **What happens:** Jet uses `next` instead of `continue` in loops, yet `next` is one of the most common local variable names in iterators, parsers, and linked/state code (`.Ok(next) ->`, `next: ir_body_stmt(...)`).
- **Proposed fixes / ideas:**
  - Keep a crisp diagnostic when someone writes `continue` suggesting `next`, and warn cleanly if a local named `next` shadows loop `next` inside a loop body.

---

### B. Syntax, Braces, Literals & Strings

#### B9. Casts and struct literals (`Int{x}`, `Float{0.0}`, `Type.Named{...}`) collide with `if`/`loop` `{` blocks — `COUNTERPRODUCTIVE`
- **What happens:** Because `Int{x}`, `U8{x}`, `Float{0.0}`, and `Type.Named{name: "Unit"}` all use `{...}`, writing `if Int{bytes[i]} == 126 { ... }` or `if ret_ty != Type.Named{name: "Unit"} { ... }` parses `{` as starting the `if`/`loop` body! Jet has `E0637` (`TypedLiteralInHeader`) for `if a > I32{0}`, **but misses the left-hand side** and emits *"This line computes a value but doesn't do anything with it / found `==`"*.
- **Proposed fixes / ideas:**
  - **Best fix:** Disambiguate `TypeName{...}` inside `if`/`loop` headers in the parser when `{...}` is followed by an operator (`==`, `<`, `&&`, `||`, `.`), `->`, or `{`.
  - **Minimum fix:** Extend `E0637` to fire on *all* positions in `if`/`loop` headers with a 1-click parenthesizing fix-it.

#### B11. Every `"..."` string interpolates `{...}`, turning JSON, tests & `\u{...}` into `{{{{` soup — `ACCIDENTAL FRICTION`
- **What happens:** `"if true {"` fails to compile (*unterminated `{`*). `"\\u{7f}"` fails with *"f isn't a unit in scope"* (because `{7f}` parses as `7` with suffix `f`!). JSON becomes `"{{\"textDocument\":{{\"uri\":{uri}}}}}"`. Backtick raw strings `` `...` `` skip `{...}` interpolation, **but** cannot contain `\n`, `\t`, or `\u{...}`, and you cannot nest backticks inside backtick strings.
- **Proposed fixes / ideas:**
  - Support a clean non-interpolating escaped string (e.g. `r"..."` or `#"..."#` / multi-backtick raw delimiters `` ``...`` ``), OR opt-in interpolation syntax (`$"..."`), so JSON, regexes, and code-generating tests never need `{{{{` escaping.

#### B13. No ASCII byte literal syntax (`b'\n'`, `b'#'`, `b'['`) — `ACCIDENTAL FRICTION`
- **What happens:** Parsers, lexers, terminal decoders, and binary encoders are full of unreadable ASCII magic numbers: `U8{10}`, `U8{13}`, `U8{27}`, `U8{92}`, `b2 == 51`, `(Int{bytes[i]}) == 46`.
- **Proposed fixes / ideas:**
  - Add byte literals (`b'\n'`, `b'#'`, `b'\\'` of type `U8`) or `U8{'#'}` char-to-byte constant folding.

#### B16. No inclusive range `1..=N` or reverse range `(0..<n).rev()` — `ACCIDENTAL FRICTION`
- **What happens:** `loop i in 1..=20` fails with `Expected a value, found =` (no hint to use `..<`), and iterating backwards requires manual `^i: n - 1; loop i >= 0 { ...; i -= 1 }`.
- **Proposed fixes / ideas:**
  - Either support `..=` directly or emit a targeted hint (*"Jet ranges are exclusive (`..<`); write `1..<21`"*).
  - Add `.rev()` and `.step(k)` range helpers (`loop i in (0..<n).rev()`).

#### B17. Unhelpful error when writing `^` for bitwise XOR (actually `~`) or `/` on `Int` (actually `/%`) — `TOOLING QUICK WIN`
- **What happens:** Writing `bits ^ 0x8000` prints `Expected } to close a typed literal, found ^` with zero hint that bitwise XOR in Jet is `~`. Writing `10 / 3` silently turns into a `Fraction` until a downstream type mismatch appears.
- **Proposed fixes / ideas:**
  - Emit a crisp targeted hint whenever `^` appears in binary-operator position (*"Bitwise XOR in Jet is `~`; `^` is the write mark"*) and suggest `/%` when a `Fraction` from `Int / Int` meets an `Int` slot.

---

### C. Types, Structs, Enums, Generics & Signatures

#### C18. No general binding type-ascription syntax (`name: Type = expr`) — `COUNTERPRODUCTIVE`
- **What happens:** `^items: [Int]{}` works for literals/containers, but `job: Task<Int DomainError!> = task { parse(x) }` fails to parse. Whenever an expression needs an expected type (`task { ... }`, `Ok(...)`, `Err(...)`, closures, empty lists), authors must invent **dummy single-use helper functions** purely to supply a return type.
- **Proposed fixes / ideas:**
  - Support a clean local typed-binding or expression ascription form aligned with Jet's `:` declaration law.

#### C19. Named-field enum construction vs. positional pattern matching mismatch — `ACCIDENTAL FRICTION`
- **What happens:** Many enums construct with named fields (`TExprKind.Lambda{value: v}`, `Type.List{element: Type.Int}`) yet match positionally (`.Lambda(v)`), while other variants construct positionally (`Val(x)`). There's no visual rule telling you which one a variant uses until `UnknownArgumentLabel` fires.
- **Proposed fixes / ideas:**
  - Support named-field destructuring in patterns (`.Lambda{value: v}`, `.IntLit{..}`) AND document/enforce one clear convention for positional vs. named enum payloads.

#### C22. Single-argument `Type{x}` conversion hits `AmbiguousCall` with two numeric `From` impls — `BUG / COMPILER GAP`
- **What happens:** Giving `F16` both `impl From(Float)` and `impl From(F32)` made `F16{1.5}` fail with `Jet.AmbiguousCall` because the literal matched both candidates instead of preferring `Float`.
- **Proposed fixes / ideas:**
  - Rank canonical default numeric types (`Float` over `F32`, `Int` over `I64`/`I32`) when resolving `From` overloads on untyped literals.

#### C24. Closure parameter types not inferred in generic methods like `sort_by` — `ACCIDENTAL FRICTION`
- **What happens:** `edits.sort_by(fn(left, right) { ... })` forced repeating a 5-field anonymous record type three times (`LSPFootprint.jet`) because `sort_by` didn't propagate the list's element type into the closure's parameters.
- **Proposed fixes / ideas:**
  - Propagate generic method parameter types from the receiver (`List<T>.sort_by`) into closure arguments during type inference.

#### C25. Generic placement (`Endpoint: struct<Q, R>`) & `*` keyword-only call hints — `ACCIDENTAL FRICTION`
- **What happens:** Writing `Endpoint<Q, R>: struct` instead of `Endpoint: struct<Q, R>` trips muscle memory constantly; calling a function with parameters after `*` requires guessing which arguments must be labeled.
- **Proposed fixes / ideas:**
  - Add a tailored parser recovery hint for `Name<T>: struct` / `name<T>: fn(...)` showing the canonical `Name: struct<T>` form.
  - Include a copy-pasteable call-site fix-it in `E0104` / `E0769` naming the exact required labels.

---

### D. Ownership, Borrowing & Copying

#### D27. Returning a borrowed `String` / small struct + `.copy()` ratchet pushes authors into disguised copies — `COUNTERPRODUCTIVE`
- **What happens:**
  - Returning a read `String` parameter (`if !color -> return line`) fails with `ReadOnlyValueNotOwned`. Under strict compiler `.copy()` ceilings, authors end up writing `return "{line}"` or `items.push("{value}")` — **still allocating a full copy, just disguised as string interpolation!**
  - Returning a tiny all-scalar struct like `Complex` (two `Float`s) fails without `.copy()` unless that type was hand-listed in the compiler's internal `Copy.jet` allowlist.
- **Proposed fixes / ideas:**
  - Infer `Copy` **structurally** for small plain all-scalar structs (any `struct` whose fields are all `Copy` and has no `#Close` / `#MoveOnly`), deleting the manual `Copy.jet` allowlist.
  - Make `View<String>` / borrowed returns effortless for pass-through helpers.
  - Auto-move last-use local variables into `^^` parameters without forcing `.move()` noise.

#### D28. Over-strict `UseWhileWriteActive` on sequential nested calls — `ACCIDENTAL FRICTION`
- **What happens:** `jfmt_write(^f, jfmt_float_literal_spelling(f, span, value, raw))` fails with `UseWhileWriteActive`, even though the inner read completes completely before `jfmt_write` begins mutating `f`. Forces hoisting a temp variable onto the line above.
- **Proposed fixes / ideas:**
  - Implement Rust-style **two-phase borrows** for calls: reserve the `^arg` write place during argument evaluation, and activate exclusivity only when the callee actually executes.

#### D29. Capturing `Shared<T>` into multiple tasks/closures requires manual `.copy()` shadow variables — `ACCIDENTAL FRICTION`
- **What happens:** Even though `Shared<T>` exists specifically to be shared across tasks, spawning two tasks requires manual shadow bindings (`h1: hits.copy(); h2: hits.copy(); task { ... h1 ... }; task { ... h2 ... }`).
- **Proposed fixes / ideas:**
  - Either allow an explicit capture list (`task[hits] { ... }`) that cheaply bumps `Shared<T>`'s handle, or auto-clone `Shared<T>` handles on task/closure capture.

#### D30. Non-obvious `.in_place()` and `^guard:` mutability diagnostics — `ACCIDENTAL FRICTION`
- **What happens:** Mutating a value inside an `Option`/enum requires knowing `.in_place()` (`if x.in_place() == .Val(v)`), and mutating through a `Shared<T>` lock fails unless you write `^guard: cell.guard_edit()`.
- **Proposed fixes / ideas:**
  - Have the immutability diagnostic on `Option`/`GuardMut` directly suggest the exact spelling (`add ^ before guard: ^guard: cell.guard_edit()` or `use .in_place()`).

---

### E. Error Handling & Jet-vs-Go Remaining Items

*(Note: `E32`, `Go-2`, and `Go-3` are already approved & carded as P0 in `#7043`!)*

#### E31 / E51 (Go-1). Bare `fn() -> T` silently means `T Err!` instead of infallible (`Never!`) — `COUNTERPRODUCTIVE`
- **What happens:** In Go, `func add(a, b int) int` is obviously infallible. In Jet, omitting the error rail on `add: fn(a: Int, b: Int) -> Int` silently makes it fallible (`Int Err!`), forcing almost every pure/internal helper in `Compiler/` and `Core/` to repeat `Never!` on every single signature (`-> Bool Never!`, `-> Int Never!`).
- **Proposed fixes / ideas:**
  - Keep `Err!` as the friendly default for `run: fn()` and `#Test` (so beginners and scripts write zero error annotations), and let ordinary library/helper `fn` declarations default to infallible (`Never!`) when no `!` rail is written!

#### E33 / E52 (Go-4). Nested `Result` ergonomics (`(T E!) TaskFailure!`) — `ACCIDENTAL FRICTION`
- **What happens:** `Task<Int DomainError!>.join()` returns `(Int DomainError!) TaskFailure!`, which is awkward to bind or unwrap without passing the handle to a helper function.
- **Proposed fixes / ideas:**
  - Ensure `h.join()?` cleanly unwraps the outer `TaskFailure!` into `T E!`, make nested `.Ok(.Ok(v))` / `.Ok(.Err(e))` / `.Err(tf)` matching smooth without helper wrappers, and support `Task<T E!>` uniformly across `task.all` / `task.race` / `task.any`.

#### E53 (Go-5). Rock-solid `??` block/error binding everywhere (`val: f() ?? { log(err); return fallback }`) — `ACCIDENTAL FRICTION`
- **What happens:** Replacing Go's 3-line `if err != nil { log(err); return fallback }` relies on `val: f() ?? { log(err); return fallback }` working cleanly in *every* expression position (including inside `task { ... }`, lambdas, and multi-statement blocks) with `err` bound cleanly on the failure branch.
- **Proposed fixes / ideas:**
  - Audit and ensure `?? { ... }` with implicit `err` binding works uniformly across `fn`, lambdas, `task { ... }`, and `if`/loop arms, with zero surprising `Unit`/type-mismatch rejections.

---

### F. Collections, Strings, Bytes & Stdlib Ergonomics

#### F34. Empty collection / `None` literals require repeating known types (`[Int]{}`, `Int?{None}`) — `ACCIDENTAL FRICTION`
- **What happens:** Even inside a struct literal or typed parameter where the type is already known (`IROptOptions{disabled: [String]{}}`, `^idx: Int?{None}`, `Shared<[Int]>{[Int]{}}`), you must repeat the full type head on every empty list, map, or `None`.
- **Proposed fixes / ideas:**
  - Infer `[]`, `[:]`, and `None` whenever an expected `[T]`, `[K:V]`, or `T?` type is known from a struct field, parameter, binding, or return type.

#### F36. `String` stdlib rough edges (`find` returns `-1`, no `StringBuilder`, `split` returns iterator, no `byte_slice` / `from_byte`) — `ACCIDENTAL FRICTION`
- **What happens:**
  - `String.find` / `rfind` returns `-1` instead of `Int?` (contradicting Jet's "no sentinel values" law!).
  - Building strings either does quadratic `out = "{out}{piece}"` or manually pushes `U8` into `[U8]{}` + `String.from_bytes(...) ?? ""`.
  - `s.split("\n")[0]` fails because `split` returns an iterator (needs `.to_list()`).
  - Emitting one byte (like ESC `27`) requires `String.from_bytes([U8]{27}) ?? ""`.
  - `len()` vs. `count_bytes()` vs. `slice(a..<b)` leads every package to roll its own clamped byte slicer.
  - `Float` `Display` prints `2.0` while `Complex` strips `.0` (`2 + 3i`).
- **Proposed fixes / ideas:**
  - Return `Int?` from `String.find` / `rfind`.
  - Add `StringBuilder` (or `^s.push_str(...)`), `String.from_byte(b: U8)`, `String.byte_slice(a..<b)`, and a targeted fix hint when indexing an iterator (*"call `.to_list()` first"*).
  - Document `Float` vs. `Complex`/`Quaternion` display formatting in one reference table.

#### F38. List literals `[a, b]` move elements without suggesting `.copy()` cleanly — `ACCIDENTAL FRICTION`
- **What happens:** Building a quick inspection list `[first, second]` of non-Copy values moves them and causes `UseAfterMove` later.
- **Proposed fixes / ideas:**
  - Keep strict move semantics, and enhance `UseAfterMove` hints to suggest `.copy()` when the element type implements `Clone`.

---

### G. Concurrency, Effects & Imports

#### G39. Rigid explicit effect lists cause cross-package churn (`#6861`) & unclear callback effects — `ACCIDENTAL FRICTION`
- **What happens:** Adding a legitimate low-level read effect (`Env` / `FS.Read`) to `Core/Net/TLS.jet` (e.g., reading system CA roots) immediately broke `Core/HTTP/Client.jet` and `Server.jet` because those callers spelled out explicit leaf ceilings (`-[Net, Time.Wait]>`). Separately, on callback types like `ask: fn(^Session, Int) -> Bool`, it's hard to predict whether the callback is pure or effect-polymorphic.
- **Proposed fixes / ideas:**
  - Support effect bundles / aliases (e.g. `Net.TLS`) or effect-row polymorphism on higher-order helpers and traits so internal leaf additions don't ripple across every transitive caller.
  - Show inferred/allowed effect rows clearly on `fn(...)` types in LSP hover and diagnostics.

#### G40. Mandatory unused group name in `task.group g { ... }` & `task.all` shapes — `ACCIDENTAL FRICTION`
- **What happens:** `task.group workers { h: task foo() }` forces inventing an unused identifier `workers` even when bare `task` implicitly joins the group. Meanwhile `task.all` has 3 subtly different shapes (`task.all { a(), b() }`, `task.all { left: a(), right: b() }`, `task.all([t1, t2])`).
- **Proposed fixes / ideas:**
  - Allow anonymous `task.group { ... }` and `task.group(limit: 2) { ... }` when the group handle isn't referenced.

#### G42. Import alias shadowing, non-obvious `pkg.[a, b]` syntax, & opaque `E0003` — `ACCIDENTAL FRICTION`
- **What happens:**
  - `use files: core.files` collides with locals named `files`, forcing ugly prefixed aliases (`td_files`, `cb_files`, `service_time`).
  - Writing `use core.files as f` or `jet_cli[a, b]` (missing the `.`) gives a generic syntax error.
  - `E0003` (*"An import declaration has no resolved graph edge"*) doesn't say *which* import failed or *where* Jet searched.
- **Proposed fixes / ideas:**
  - Add targeted parser hints for `use ... as ...` and `pkg[a, b]`.
  - Print the exact unresolved module path and searched directories/registries in `E0003`.
  - Consider letting module aliases live cleanly alongside local parameter names (or allowing direct `core.files.read(...)` without mandatory aliasing).

#### G43. `Jet.UnusedImport` is a hard compile error instead of a warning (and `priv` vs. `_` vs. `__` confusion) — `ACCIDENTAL FRICTION`
- **What happens:** During refactoring, leaving 1 unused import blocks `jet check` with exit code 1 (and untouched files with stale unused imports break whole-package `jet check`). Separately, `_extracted` vs. `priv foo` vs. `__reserved` (`E0067`) creates three overlapping naming conventions.
- **Proposed fixes / ideas:**
  - Demote `UnusedImport` to a warning during dev `jet check` (keeping `--deny-warnings` for CI) and add auto-pruning in `jet fix` / `jet fmt`.
  - Standardize on `priv` for visibility and `_` strictly for intentionally unused locals/params.

---

### H. Lints & Low-Level Code

#### H45. Public-API style lints (`L2401`, `L2421`) firing on small examples and test helpers — `COUNTERPRODUCTIVE`
- **What happens:** Top-level items default to public (`priv` opt-in). In a 10-line concurrency example, `boom: fn() -> Int { panic("..."); 0 }` drew `L2421` ("declare `Never`"); changing it to `boom: fn(should_panic: Bool)` then drew `L2401` ("positional `Bool` parameter"), forcing an ugly `priv boom: fn(mode: Int)` just to stay warning-free.
- **Proposed fixes / ideas:**
  - Exempt `#Test` files and `run`-entry examples/scripts from public-library API lints (`L2401`, `L2421`), or apply them only to exported library APIs.

---

### I. Compiler Architecture & Core Library Coupling

#### I46. Bare vs. `<corelib>/...` nominal type identity bug + 6-table Core coupling — `BUG / COMPILER GAP`
- **What happens:**
  - Adding one pure-Jet value struct to `Core/Math/Math.jet` required manually editing **6 separate compiler tables** (`Core.rows` 2,000-char single-line rows, `CoreModules.jet`, `Copy.jet`, `Loader.jet`, `CoreUsage.jet`, plus `Expressions.jet`).
  - `type_identity_equal` compares `.Named(name)` by raw string equality, so built-in `1i` (`"Complex"`) did not equal `"<corelib>/core.math.Complex"` resolved inside `Core/Math/Math.jet`.
  - `jet check Core/Tasks/Tasks.jet` or `Core/App/App.jet` directly fails with false `BuiltinsOutsideCore` / `ReservedDunderName` errors because single-file `jet check` doesn't recognize `Core/package.jet` as `<corelib>` the way `jet test` does.
  - Because `Core/` is baked into the compiler binary, `jet test Core/X.jet` tests your edited file while `jet run` silently loads the **stale embedded copy** in the seed binary!
- **Proposed fixes / ideas:**
  - Normalize nominal type identities in `type_identity_equal` (one canonical type ID, never raw string mismatch between bare and `<corelib>/...`).
  - Auto-generate `Core.rows` / `CoreModules.jet` from `Core/` declarations (or format `Core.rows` one export per line).
  - Make `jet check Core/<file>.jet` recognize `Core/package.jet` identically to `jet test`.
  - Split massive monoliths (`Core/Math/Math.jet` at 46,700 lines, `Expressions.jet` at 8,500 lines, `Loader.jet` at 8,000 lines) into focused submodules.

#### I47. Single declaration typos cascade into 20+ bogus downstream errors — `TOOLING QUICK WIN`
- **What happens:** One syntax typo (`DomainError: error { ... }`) triggered 20 follow-on errors (`E0119`, `E2417`, `E0107`, `E1102`, plus bogus `L0102: Parameter label is never used` on functions whose bodies never checked).
- **Proposed fixes / ideas:**
  - Poison/taint unresolved declaration symbols cleanly so downstream type/sendability/unused-param diagnostics stay silent on broken items.

---

### J. CLI, Testing, Formatting & Diagnostics Tooling

#### J48. `jet check` warning floods & missing `--errors-only` / `--file` filtering — `TOOLING QUICK WIN`
- **What happens:** `jet check Compiler/JetCLI` prints 1,160+ warnings (~10,300 lines) and `jet check Compiler/JetSema` prints 4,340+ warnings (~39,000 lines), completely burying 1–2 real errors.
- **Proposed fixes / ideas:**
  - Add `jet check --errors-only` (and `--file <path>` to filter warnings to touched files).
  - Always print `Error:` diagnostics **after** warnings so errors sit cleanly at the bottom of terminal output.

#### J49. `jet test` footguns (silent exit 0 on 0 `--filter` matches, file path runs whole package, `.case` files crash, truncated assert diffs, missing `#Test` caller frame) — `TOOLING QUICK WIN`
- **What happens:**
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

#### J50. Incremental edit-compile-test latency & `jet fmt` workflow gaps — `TOOLING QUICK WIN`
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

## 4. Reference: Jet Error Handling vs. Go Error Handling

### Why Go's error handling is widely praised — and why programmers still complain about it

People praise Go's error handling for **4 great semantic ideas**:
1. **Errors are ordinary values, not invisible exceptions:** You can pass, wrap, inspect, log, and test them like any other data.
2. **Every failure exit is visible right in the code:** Reading a Go function top-to-bottom shows every early return — critical when auditing locks, files, or partial state mutations.
3. **Adding context as errors bubble up is natural:** `fmt.Errorf("load config %s: %w", path, err)`.
4. **Zero hidden runtime unwind machinery:** Predictable performance and simple control flow.

Yet Go's *syntax* and *type system* carry **5 real flaws**:
1. **Relentless boilerplate:** Every fallible call takes 3 lines (`v, err := f(); if err != nil { return zero, err }`), burying the happy path.
2. **Untyped `error` interface loses exhaustiveness:** A signature `(*Config, error)` doesn't say *which* errors can happen; callers silently miss new error cases.
3. **Zero-value hazard:** `(T, error)` is a pair, not a sum type — forgetting to check `err` lets you read an uninitialized zero value / `nil` pointer.
4. **Easy to silently drop an error:** Calling `file.Close()` or `os.Remove(p)` without checking `err` compiles cleanly.
5. **No built-in audience separation:** Internal debug details get concatenated into `err.Error()` and leak into user-facing UI/HTTP responses.

### Side-by-Side: Go vs. Jet

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
