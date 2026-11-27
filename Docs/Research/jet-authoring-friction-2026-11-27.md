# Writing Good Code in Jet: Open Authoring Friction TODO List & Jet vs. Go Errors

*Date: 2026-11-27 (Living TODO — items are removed from this list once approved, carded as P0 on Tower, and ready to implement)*  
*Sources: Unfiltered, independent feedback from three engineering agents who wrote thousands of lines of real Jet across `Core/`, `Compiler/`, and DX/Tooling, plus live verification against the compiler and Tower.*

> **Already Approved & Carded as P0 on Tower (removed from this TODO list):**
> - **#7042 [P0]:** `A1` (`#CatchAll`), `B12` (interpolation ICE), `B14` (contextual `U8`/`I64`/`U64` literal coercion), `B15` (`I64.MIN` literal + constants), `C20` (`.Variant` shorthand propagation), `C21` (struct update syntax)
> - **#7043 [P0]:** `E32` / `Go-2` (`#Error` + `Describe` auto-lift to `Err`), `E32` / `Go-3` (`Result` vs. `Option` & `error {}` diagnostics), `F35` (`Map.contains_key` + `Set<T>`), `F37` (`loop ^item in list`), `G41` (copy-pasteable `authority:` fix-it)
> - **#7044 [P0]:** `C23` (`Neg` operator trait), `D26` (`_: expr` read discard), `H44` (`#Layout(c)` typed struct pointers in `#Unsafe`)
> - **#7045 [P0]:** `B10` Part 1 (Option 1A: bare `Float` default, exact contextual `Decimal`/`Float`, `Type{value}` scalars, `#UnitFamily(Time)` `hour`/`day` replacing `h`/`d`, delete magic `f` suffix)
> - **#7046 [P0]:** `B10` Part 2 (Unified unquoted `Type{...}` / `Type{{{...}}}` `DSL` trait: type-owned `parse`, hole spans, LSP Semantic Tokens `highlight`, `jet fmt`, cutover of `SQL`/`HTML`/`Sh`/`Regex`/`Path`/`URL`/`DateTime`/`Grammar`/`Complex`/`Quaternion` + FFI foundation, delete magic `i`/`j`/`k` suffixes)
> - **#7047 [P0]:** Option 1 (`!` / `!(context)` / `!!` exclusively for Fallible `T E!`, reserving `?` / `?.` / `??` strictly for Optional `T?`, superseding `#7034`'s `?` failure-mark spelling) + `E53` (`!! { ... err ... }` everywhere)
> - **#7048 [P0]:** `A3` (`task {}` return inference), `A4` (chained pattern guards + guard extraction), `A5` (uniform `{}` arms + `L0514` at 2+ distinct variants), `A6` (`E0078` span on `loop`), `A7` (non-moving `_: expr` discard + `.drop` for linear), `A8` (rename loop `next` -> `skip`)
> - **#7049 [P0]:** `A2` + **Jet Composition Laws** (drop `->` on `if`/`loop`, `[loop ...]` comprehensions, zero postfix markers + stacked `#[A, B]` slots + in-body markers, flow-binding pattern algebra, `T?` finite `break val` loops, 3-class jump boundaries, full surface coverage), `B9` (`Type{...}` in `if`/`loop` headers), `B17` (binary `^` and `Int / Int` hints)
> - **#7050 [P0]:** `B11` (`\u{H}` lexer fix + multi/triple-backtick raw strings), `B13` (`U8{'\n'}` ASCII byte folding), `B16` (`..=` hint + range `.rev()` / descending stride)
> - **#7051 [P0]:** `C18-A` (`Type{expr}` ascription), `C19` (named enum patterns `.Click{x, y}`), `C22` (canonical `Float`/`Int` `From` ranking), `C24` (generic receiver closure inference like `sort_by`), `C25` (`Name<T>: struct` & `*` call-site label hints)
> - **#7052 [P0]:** `D28` (two-phase borrows for sequential nested calls — adversarial safety audit first), `D30` (`.in_place()` & `^guard: cell.guard_edit()` diagnostic hints)
> - **#7053 [P0]:** `D27 Part A` (structural `Copy` for small plain all-scalar structs) + `D27` last-use auto-move into `^^`, `D29` (zero-refcount `Shared<T>` borrow in scoped tasks — adversarial safety audit first)
> - **#7054 [P0]:** `E31`/`E51` (ordinary `fn` defaults infallible `Never!`, `run`/`#Test` keep `Err!`), `E33`/`E52` (nested `Task<T E!>.join()` ergonomics)
> - **#7055 [P0]:** `F34` (empty `[]`/`{}`/`None` inference), `F36` (`String.find` -> `Int?`, `StringBuilder`, `from_byte`, `byte_slice`, iterator `.to_list()` hint), `F38` (`UseAfterMove` `.copy()` hint), `G40` (anonymous `task.group {}`)
> - **#7056 [P0]:** `I46` (Single Source of Truth for Core library exports, canonical nominal type identity, single-file `jet check Core/<file>.jet`, monolith splits) + `I47` (poison broken declarations to stop error cascades)
> - **#7057 [P0]:** `G39` (named effect bundles & callback polymorphism), `G42` (import hints, `E0003` paths, direct qualified access keeping strict no-shadowing), `G43` (unified dev warnings vs `--deny-warnings`/`--release`), `H45` (exempt scripts/tests from `L2401`/`L2421`)
> - **#7058 [P0]:** `J48` (`jet check --errors` / `--warnings` / `--file` + errors last), `J49` (single `--flag value` CLI syntax + `jet test` fixes), `J50` (ZERO-cache fast cold single-file test + `jet fmt --stdout`/`--check-lines` + `jet inspect exports` + REPL `^name:`)
> - **#7059 [P0]:** `D27 Part B` & `#7031` replacement — **Zero-Magic Memory Trio** (COW/SSO zero-copy `String` with zero `from var` boilerplate, direct-field `Shared<T>`/`Weak<T>` with single-thread fast path retiring `@T`, and plain honest `Pool<T>`/`ID<T>` — mandatory adversarial audit first!)
>
> **Remaining in this TODO list for owner discussion & decision:** **0 items — ALL 52 ITEMS DECIDED & CARDED AS P0 ON TOWER (`#7042–#7059`, with `#7042`, `#7043`, `#7044` ALREADY CLOSED `done`)!**

---

## Table of Contents

1. [The Bottom Line: Would Fixing These Make Jet Near-Perfect?](#1-the-bottom-line-would-fixing-these-make-jet-near-perfect)
2. [Productive Friction Worth Keeping (What Forces *Good* Code)](#2-productive-friction-worth-keeping-what-forces-good-code)
3. [Remaining TODO Items to Discuss & Decide (0 Items — Complete!)](#3-remaining-todo-items-to-discuss--decide-0-items--complete)
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
1. **Cold compile & test speed (the #1 real-world make-or-break):** Fast cold single-file `jet check` and targeted `jet test` (no caching — true 30M LOC/s cold compilation) will improve real-world Jet code quality more than any single syntax tweak.
2. **Cognitive budget & orthogonality (avoiding "C++/Scala syndrome"):** Keep the language small and orthogonal so programmers can *predict* how features combine instead of memorizing compiler special cases.
3. **Zero-magic memory ergonomics (`String` COW/SSO + `Shared`/`Weak` + `Pool`/`ID`):** Eliminates disguised copies (`"{line}"`), `from var` lifetime noise, and ambient store magic.
4. **Effect-system ergonomics at large codebase scale:** Let higher-order functions and layered libraries abstract over or bundle effects cleanly so leaf-effect additions don't ripple across packages.

---

## 2. Productive Friction Worth Keeping (What Forces *Good* Code)

All three agents **explicitly asked Jet to KEEP** these guardrails — annoying for 10 seconds, then make the resulting code objectively safer and clearer:

| Guardrail | Why it forces *better* code (KEEP) |
|---|---|
| **Visible `^` and `^^` on bindings, parameters, and call sites** | Every mutation (`bump(^x)`) and ownership transfer (`send(^^msg)`) is readable right at the call site. Zero hidden side effects. |
| **Visible `!` / `!!` on fallible calls & `?` / `??` on optionals** | 1-to-1 symbol alignment with `T E!` vs `T?`; every early exit is visible at the call site. |
| **Exhaustive enum matching by default** *(paired with prefix `#CatchAll` in `#7042`)* | Catches forgotten enum variants when types grow, while `#CatchAll` handles deliberate opt-outs. |
| **Refusing never-written `^x` bindings (`E0514`)** | Keeps immutability honest: `^x` always means `x` actually changes. |
| **Strict no-shadowing** | Keeps every identifier unambiguous within a scope. |
| **Strict `Decimal` vs. `Float` separation on variables** | Prevents silent financial/binary-float rounding bugs. |
| **Checked arithmetic, `/%` vs. `/`, and arbitrary-precision `Int` default** | Prevents silent integer overflow and silent integer-division truncation bugs. |
| **Typed `#Error` enums + explicit `impl Target.From(Source)` across domain boundaries** | Prevents Go-style stringly-typed error soup across layers. |
| **Checked effects (`-[IO, Net, Time.Wait]>`) & capability `authority:`** | Forces clean architectural separation between pure logic and I/O/network code. |
| **Structured concurrency (`task.group`, `limit:`, panic isolation, lossless `SendRefused<T>`)** | Eliminates leaked tasks, silent drops, and sentinel `-1` return codes. |
| **Mandatory `.drop("reason")` on linear/resource/error values** | Forces a real justification when discarding an error or handle. |
| **Explicit `.copy()` visibility** | Prevents silent O(N) deep-copy regressions. |

---

## 3. Remaining TODO Items to Discuss & Decide (0 Items — Complete!)

*All 52 items from the authoring friction audit and Jet-vs-Go error comparison have been discussed with the owner, refined, and carded as P0 on Tower (`#7042–#7059`).*

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
