# M4 — Errors as values

**Decisions:** S34 (`T ? E`, with `T ?` defaulting to `Error`), S35/S71
(`??` fallback), S36 (`panic`/`require`) ratified. Depends on M3
(enums, `==` patterns).
**Error codes:** E0401+. Teaching codes continue E0023+.
**Retires:** E0006 (`?` staged).

## Goal

Errors are ordinary values. A function that can fail says so in its
return type; callers must do *something* — propagate, fall back, or
handle. No exceptions, no null, no silently ignored failures. `panic`
exists for bugs only.

## Surface (ratified — S34/S35/S36)

```jet
enum ParseError {
    Empty;
    BadDigit(text: String);
}

fn parse_age(raw: String) -> Int ? ParseError {
    if raw.len() == 0 { return err(ParseError.Empty); };
    // … on success:
    return ok(value);
}

fn main() {
    val a = parse_age("42") ?? 0;            // fallback value
    val b = parse_age(raw_text) ?? return;   // bail out of main
    switch parse_age("x") {                  // full handling
        it == ok(n) -> { print("age {n}"); };
        it == err(e) -> { print("bad input: {e}"); };
    }
}

fn load() -> Int ? ParseError {
    val n = parse_age("7")?;                 // propagate (S7)
    return ok(n * 2);
}
```

- **`T ? E`** is the fallible return type (S34): `T` is the success
  payload; `E` is any enum, struct, `String`, or `Error`.
- **`T ?`** in a function return means `T ? Error`. Users may write `T?`
  in that position and `jet fmt` canonicalizes it to `T ?`. A function
  returning an optional writes `-> (T?)`.
- **`ok(v)` / `err(e)`** construct the two cases; **`== ok(v)` /
  `== err(e)`** destructure them (same machinery as M3 `==` patterns).
- **`?`** (S7) propagates: unwraps `ok`, early-returns `err` — only
  inside a function whose return type carries a compatible error.
- **`?? <expr>`** (S35/S71) is the fallback operator on a fallible/Option
  value: yields the `ok`/`some` payload or evaluates the right side.
  The right side is either a value of the payload type, `return [expr]`,
  or a `panic(…)` call. (Also works on `T?` — retrofit note in sema.)
  The retired word `or` is a teaching error pointing at `??`.
- **`panic("msg")`** stops with a friendly runtime report; **`require(cond)`**
  and **`require(cond, "msg")`** panic when the condition is false (S36).
- In a `switch` over a fallible call, `it` names the subject when the
  subject expression is not a plain name (small ergonomic rule — see
  sema rule 6).

### Grammar additions

```
return-type += type "?" [ type ] ;       // S34: `T ? E` or default `T ?`
type        += type "?" type ;           // explicit fallible annotation
expr        += "ok" "(" expr ")" | "err" "(" expr ")"
         | expr "?"                    // postfix, binds like a call
         | expr "??" orfallback ;
orfallback = expr | "return" [ expr ] | panic-call ;
pattern += "ok" "(" ident ")" | "err" "(" ident ")" ;
```

`??` the fallback operator is expression-only (S35/S71); the lexer token is
distinct from logical `||` (S13). Precedence: `e ?? f` binds looser than
`&&`/`||` so `a? ?? b` and `x == 1 || y ?? 0` parse predictably;
document in docs/spec/spec.md.

## Sema rules

1. A `T ? E` value cannot be used as a `T`: every use must go through
   `?`, `??`, or `== ok`/`== err` (E0401, fix lists all three). An *unused*
   fallible call as a statement → E0402 ("this can fail and nothing checks
   it"; fix: `… ?? panic(…)` if failure is impossible).
2. `?` requires the enclosing function to return `U ? E2` where the
   propagated error type `E` equals `E2` (no conversions in v1 — E0403
   names both error types; fix: handle here with `== err`, or make the
   types match). `?` on `T?` propagates `null` iff the function returns
   an Option (same rule, same code).
3. `ok`/`err` only typecheck where a fallible type is expected (E0404,
   mirror of M3's E0308 for `null`); `err(e)` requires `e`'s type to be
   the declared error type.
4. `??` fallback: payload type and fallback expression type must match
   (E0405). `?? return` requires the function's return type to permit a
   bare return; `?? return expr` typechecks `expr` against it.
5. `main` may not declare an error return in v1 (keeps E0122's story);
   errors reaching `main` are handled explicitly. (Revisit post-v1.)
6. The `it` subject name in `switch <fallible-expr> { it == ok(n) … }`
   is bound only when the subject is not already a name; shadowing rules
   E0118 apply.
7. Exhaustiveness: a pattern-switch over a result must cover `ok` and
   `err` (extends M3 E0307 — message says "you forgot the `err` case").
8. `panic`/`require` are builtins like `print` (arity checked, E0103
   pattern; `require` cond must be Bool, E0110). Redefining them → E0106.

## Codegen lowering

| Jet                    | Rust                                              |
|------------------------|---------------------------------------------------|
| `T ? E`                | `Result<T, E>`                                    |
| `T ?`                  | `Result<T, String>` initially (`Error` surface)   |
| `ok(v)` / `err(e)`     | `Ok(v)` / `Err(e)`                                |
| `e?`                   | `e?` (types align by construction)                |
| `v ?? fallback`        | `match v { Ok(x) => x, Err(_) => fallback }` (and Option equivalent) |
| `v ?? return [e]`      | `match v { Ok(x) => x, Err(_) => return [e] }`    |
| `panic("m {x}")`       | `jet_panic(file, line, format!(…))` runtime helper |
| `require(c, "m")`      | `if !(c) { jet_panic(…) }`                        |

Runtime report format (pinned by a golden test capturing stderr):

```
The program stopped: <message>
  --> file.jet:12
```

Codegen embeds the Jet file/line of each `panic`/`require` as string/int
constants (no source maps needed). The helper prints to stderr and exits
with code 70. Rust panics from generated code remain ICEs (R5) — the
helper never uses `panic!`.

## Diagnostics to register

E0401 fallible value used unchecked · E0402 fallible result ignored ·
E0403 `?` error type doesn't match the function's · E0404 `ok`/`err`
need a result context · E0405 `??` fallback type mismatch.
Teaching: E0023 `throw`/`raise` → return `err(…)` · E0024 `catch`/
`except` → `??` / `== err` · E0025 `unwrap`/`expect` → `?? panic(…)`.
E0014 (`try` → `?`) already exists; update its message to point at the
now-real feature.

## Examples & tests

- `examples/features/13_errors.jet` — parse a config-like string; happy path uses
  `?` and stays clean; one `??` default; one full `switch`.
- `examples/features/14_panic.jet` — require + panic output (golden test pins the
  runtime report and exit code 70).
- ui fixtures: every E04xx + the three teaching errors + `.fixed.jet`
  companions. A fixture proving `?` in `main` errors cleanly.
- Golden: rustc accepts all of it; an example where the error enum has
  payloads and prints via M3's derived Display.

## Out of scope

General error conversion/`IntoError` chains, multi-error unions on the `E`
side of `T ? E`, `defer`/cleanup syntax, backtraces, catching panics, async
anything. Stdout/stderr distinction beyond the panic report.

## Suggested implementation order

1. syntax.rs: `T ? E`, `or` (fallback), `ok`, `err`, `panic`,
   `require`; `?` un-stages.
2. Parser: `T ? E` return types, postfix `?`, `or` precedence (fixtures first).
3. Sema rules 1–8 (the must-check analysis is the heart — write
   exhaustive negative fixtures before implementing).
4. Codegen + runtime helper + exit-code golden test.
5. Teaching errors, E0014 message refresh, docs updates, snapshots.
