# Writing Good Code in Jet: Open Authoring Friction TODO List & Jet vs. Go Errors

*Date: 2026-11-27 (Living TODO — items are removed from this list once approved, carded as P0 on Tower, and ready to implement)*  
*Sources: Unfiltered, independent feedback from three engineering agents who wrote thousands of lines of real Jet across `Core/`, `Compiler/`, and DX/Tooling, plus live verification against the compiler and Tower.*

> **Already Approved & Carded as P0 on Tower (removed from this TODO list):**
> - **#7042 [P0]:** `A1` (`#CatchAll`), `B12` (interpolation ICE), `B14` (contextual `U8`/`I64`/`U64` literal coercion), `B15` (`I64.MIN` literal + constants), `C20` (`.Variant` shorthand propagation), `C21` (struct update syntax)
> - **#7043 [P0]:** `E32` / `Go-2` (`#Error` + `Describe` auto-lift to `Err`), `E32` / `Go-3` (`Result` vs. `Option` & `error {}` diagnostics), `F35` (`Map.contains_key` + `Set<T>`), `F37` (`loop ^item in list`), `G41` (copy-pasteable `authority:` fix-it)
> - **#7044 [P0]:** `C23` (`Neg` operator trait), `D26` (`_: expr` read discard), `H44` (`#Layout(c)` typed struct pointers in `#Unsafe`)
> - **#7045 [P0]:** `B10` Part 1 (Option 1A: bare `Float` default, exact contextual `Decimal`/`Float`, `Type{value}` scalars, `#UnitFamily(Time)` `hour`/`day` replacing `h`/`d`, delete magic `f` suffix)
> - **#7046 [P0]:** `B10` Part 2 (Unified unquoted `Type{...}` / `Type{{{...}}}` `DSL` trait: type-owned `parse`, hole spans, LSP Semantic Tokens `highlight`, `jet fmt`, cutover of `SQL`/`HTML`/`Sh`/`Regex`/`Path`/`URL`/`DateTime`/`Grammar`/`Complex`/`Quaternion` + FFI foundation, delete magic `i`/`j`/`k` suffixes)
> - **#7047 [P0]:** Option 1 (`!` / `!(context)` / `!!` exclusively for Fallible `T E!`, reserving `?` / `?.` / `??` strictly for Optional `T?`, superseding `#7034`'s `?` failure-mark spelling)
> - **#7048 [P0]:** `A3` (`task {}` return inference), `A4` (chained pattern guards + guard extraction), `A5` (uniform `{}` arms + `L0514` at 2+ distinct variants), `A6` (`E0078` span on `loop`), `A7` (non-moving `_: expr` discard + `.drop` for linear), `A8` (rename loop `next` -> `skip`)
> - **#7049 [P0]:** `A2` + **Jet Composition Laws** (drop `->` on `if`/`loop`, `[loop ...]` comprehensions, zero postfix markers + stacked `#[A, B]` slots + in-body markers, flow-binding pattern algebra, `T?` finite `break val` loops, 3-class jump boundaries, full surface coverage), `B9` (`Type{...}` in `if`/`loop` headers), `B17` (binary `^` and `Int / Int` hints)
> - **#7050 [P0]:** `B11` (`\u{H}` lexer fix + multi/triple-backtick raw strings), `B13` (`U8{'\n'}` ASCII byte folding), `B16` (`..=` hint + range `.rev()` / descending stride)
> - **#7051 [P0]:** `C18-A` (`Type{expr}` ascription), `C19` (named enum patterns `.Click{x, y}`), `C22` (canonical `Float`/`Int` `From` ranking), `C24` (generic receiver closure inference like `sort_by`), `C25` (`Name<T>: struct` & `*` call-site label hints)
> - **#7052 [P0]:** `D28` (two-phase borrows for sequential nested calls — adversarial safety audit first), `D30` (`.in_place()` & `^guard: cell.guard_edit()` diagnostic hints)
> - **#7053 [P0]:** `D27 Part A` (structural `Copy` for small plain all-scalar structs), `D29` (zero-refcount `Shared<T>` borrow in scoped tasks — adversarial safety audit first)
>
> **Remaining in this TODO list for owner discussion & decision:** **17 items** (`D27 Part B`, `E31`, `E33`, `E51–E53`, `F34`, `F36`, `F38`, `G39–G40`, `G42–G43`, `H45`, `I46–I47`, `J48–J50`). Item numbers are kept stable as items are removed.

---

## Table of Contents

1. [The Bottom Line: Would Fixing These Make Jet Near-Perfect?](#1-the-bottom-line-would-fixing-these-make-jet-near-perfect)
2. [Productive Friction Worth Keeping (What Forces *Good* Code)](#2-productive-friction-worth-keeping-what-forces-good-code)
3. [Remaining TODO Items to Discuss & Decide (17 Items)](#3-remaining-todo-items-to-discuss--decide-17-items)
   - [D. Ownership, Borrowing & Copying (`D27 Part B`)](#d-ownership-borrowing--copying)
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

## 3. Remaining TODO Items to Discuss & Decide (17 Items)

Legend:
- `COUNTERPRODUCTIVE` = actively pushes authors to write *worse* code.
- `ACCIDENTAL FRICTION` = costs time/readability while preventing zero bugs.
- `BUG / COMPILER GAP` = genuine defect or incomplete implementation.
- `TOOLING QUICK WIN` = CLI/test/diagnostic improvement with zero language risk.

---

### D. Ownership, Borrowing & Copying

#### D27 Part B. Functional `View<str>` / `View<T>` pass-through returns & last-use auto-move — `COUNTERPRODUCTIVE`
- **What happens:** Returning a plain borrowed `String` parameter as `View<str> from line` currently fails (`E2307` in `StringViews.jet:302` refuses plain `String` without `.trim()`/`.before()`/`.after()`), pushing authors into disguised allocations (`return "{line}"`). Also last-use local moves into `^^` params still sometimes require `.move()` noise.
- **Interaction with `#7031` (`D-HANDLE-NAV1`):** `#7031` replaces stored `@T` live links with `Pool<T>`/`ID<T>` handles while **explicitly keeping `View<T>`/`View<str>`** for temporary zero-copy borrows — zero conflict!
- **Proposed fixes / ideas:**
  - Allow `View<str>` / `View<T>` to accept plain borrowed `String` / `T` parameters directly (zero-copy pass-through).
  - Auto-move last-use locals into `^^` parameters without `.move()` noise.

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
  - Ensure `h.join()!` cleanly unwraps the outer `TaskFailure!` into `T E!`, make nested `.Ok(.Ok(v))` / `.Ok(.Err(e))` / `.Err(tf)` matching smooth without helper wrappers, and support `Task<T E!>` uniformly across `task.all` / `task.race` / `task.any`.

#### E53 (Go-5). Rock-solid `!!` block/error binding everywhere (`val: f() !! { log(err); return fallback }`) — `ACCIDENTAL FRICTION`
- **What happens:** Replacing Go's 3-line `if err != nil { log(err); return fallback }` relies on `val: f() !! { log(err); return fallback }` working cleanly in *every* expression position (including inside `task { ... }`, lambdas, and multi-statement blocks) with `err` bound cleanly on the failure branch.
- **Proposed fixes / ideas:**
  - Audit and ensure `!! { ... }` with implicit `err` binding works uniformly across `fn`, lambdas, `task { ... }`, and `if`/loop arms, with zero surprising `Unit`/type-mismatch rejections (can roll directly into `#7047` if approved).

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
