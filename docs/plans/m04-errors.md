# M4 — Errors as values

**Decisions:** S34 (`Result<T, E>`), S35 (`or` fallback), S36 (`panic`/`require`)
ratified. Depends on M3 (enums, `==` patterns).
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

fn parse_age(raw: String) -> Result<Int, ParseError> {
    if raw.len() == 0 { return err(ParseError.Empty); };
    // … on success:
    return ok(value);
}

fn main() {
    val a = parse_age("42") or 0;            // fallback value
    val b = parse_age(raw_text) or return;   // bail out of main
    switch parse_age("x") {                  // full handling
        it == ok(n) -> { print("age {n}"); };
        it == err(e) -> { print("bad input: {e}"); };
    }
}

fn load() -> Result<Int, ParseError> {
    val n = parse_age("7")?;                 // propagate (S7)
    return ok(n * 2);
}
```

- **`Result<T, E>`** is the fallible return type (S34, S33 angle brackets):
  `Result` is a prelude builtin; `E` is any enum, struct, or `String`.
- **`ok(v)` / `err(e)`** construct the two cases; **`== ok(v)` /
  `== err(e)`** destructure them (same machinery as M3 `==` patterns).
- **`?`** (S7) propagates: unwraps `ok`, early-returns `err` — only
  inside a function whose return type carries a compatible error.
- **`or <expr>`** (S35) is the fallback operator on a result/Option
  value: yields the `ok`/`some` payload or evaluates the right side.
  The right side is either a value of the payload type, `return [expr]`,
  or a `panic(…)` call. (Also works on `T?` — retrofit note in sema.)
- **`panic("msg")`** stops with a friendly runtime report; **`require(cond)`**
  and **`require(cond, "msg")`** panic when the condition is false (S36).
- In a `switch` over a fallible call, `it` names the subject when the
  subject expression is not a plain name (small ergonomic rule — see
  sema rule 6).

### Grammar additions

```
type    += "Result" "<" type "," type ">" ;   // S34, same brackets as S33
expr    += "ok" "(" expr ")" | "err" "(" expr ")"
         | expr "?"                    // postfix, binds like a call
         | expr "or" orfallback ;
orfallback = expr | "return" [ expr ] | panic-call ;
pattern += "ok" "(" ident ")" | "err" "(" ident ")" ;
```

`or` the fallback operator is expression-only (S35); the lexer token is
distinct from logical `||` (S13). Precedence: `e or f` binds looser than
`&&`/`||` so `a? or b` and `x == 1 || y or 0` parse predictably;
document in docs/01.

## Sema rules

1. A `Result<T, E>` value cannot be used as a `T`: every use must go through
   `?`, `or`, or `== ok`/`== err` (E0401, fix lists all three). An *unused*
   fallible call as a statement → E0402 ("this can fail and nothing checks
   it"; fix: `… or panic(…)` if failure is impossible).
2. `?` requires the enclosing function to return `Result<U, E2>` where the
   propagated error type `E` equals `E2` (no conversions in v1 — E0403
   names both error types; fix: handle here with `== err`, or make the
   types match). `?` on `T?` propagates `null` iff the function returns
   an Option (same rule, same code).
3. `ok`/`err` only typecheck where a result type is expected (E0404,
   mirror of M3's E0308 for `null`); `err(e)` requires `e`'s type to be
   the declared error type.
4. `or` fallback: payload type and fallback expression type must match
   (E0405). `or return` requires the function's return type to permit a
   bare return; `or return expr` typechecks `expr` against it.
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
| `Result<T, E>`         | `Result<T, E>`                                    |
| `ok(v)` / `err(e)`     | `Ok(v)` / `Err(e)`                                |
| `e?`                   | `e?` (types align by construction)                |
| `v or fallback`        | `match v { Ok(x) => x, Err(_) => fallback }` (and Option equivalent) |
| `v or return [e]`      | `match v { Ok(x) => x, Err(_) => return [e] }`    |
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
need a result context · E0405 `or` fallback type mismatch.
Teaching: E0023 `throw`/`raise` → return `err(…)` · E0024 `catch`/
`except` → `or` / `== err` · E0025 `unwrap`/`expect` → `or panic(…)`.
E0014 (`try` → `?`) already exists; update its message to point at the
now-real feature.

## Examples & tests

- `examples/13_errors.jet` — parse a config-like string; happy path uses
  `?` and stays clean; one `or` default; one full `switch`.
- `examples/14_panic.jet` — require + panic output (golden test pins the
  runtime report and exit code 70).
- ui fixtures: every E04xx + the three teaching errors + `.fixed.jet`
  companions. A fixture proving `?` in `main` errors cleanly.
- Golden: rustc accepts all of it; an example where the error enum has
  payloads and prints via M3's derived Display.

## Out of scope

Error conversion/`From` chains, multi-error unions on the `E` side of
`Result`, `defer`/cleanup syntax, backtraces, catching panics, async
anything. Stdout/stderr distinction beyond the panic report.

## Suggested implementation order

1. syntax.rs: `Result` builtin, `or` (fallback), `ok`, `err`, `panic`,
   `require`; `?` un-stages.
2. Parser: `Result<T, E>` types, postfix `?`, `or` precedence (fixtures first).
3. Sema rules 1–8 (the must-check analysis is the heart — write
   exhaustive negative fixtures before implementing).
4. Codegen + runtime helper + exit-code golden test.
5. Teaching errors, E0014 message refresh, docs updates, snapshots.
