# 04 — Diagnostics

Error messages are the language's user interface. They are designed, not
written; every change is reviewed against this file and pinned by a
snapshot in tests/ui/.

## The contract

Every diagnostic has four parts:

- **code** — stable ID (`E0102`). Never reuse or renumber.
- **what** — one line, plain language, names the thing in backticks.
- **why** — the rule behind the error, so the user learns the model.
- **fix** — a concrete next step, copy-pasteable when possible.

## Exact render format (pinned by snapshots)

Sentence capitalization throughout — `Error` / `Why:` / `Fix:` (owner,
2026-06-11). M0 snapshots using the old lowercase form are re-blessed as
part of M1.

```
Error [E0102]: nothing named `pirnt` exists here
  --> tests/ui/unknown_function.jet:2:5
    |
  2 |     pirnt("hi")
    |     ^^^^^
 Why: only functions that have been defined (or built in, like `print`) can be called
 Fix: did you mean `print`?
```

Diagnostics without a span (e.g. E0101) omit the location/source block.
Multiple diagnostics are separated by one blank line. Every stage reports
all the problems it can in one run (M1 error recovery): the lexer skips
past bad characters, the parser re-syncs at statement boundaries, and
sema checks every function. Caret columns are display-width aware, so
underlines line up under wide characters and emoji.

Lint warnings use the same shape with `Warning [L02xx]:` instead of
`Error [E02xx]:`. Lints do not block compilation; the driver prints them
before continuing.

## Voice rules

- Plain words. Banned: *token, expression, statement, identifier, parse,
  syntax error, illegal, invalid, lifetime, borrow checker*.
  Say: "the name `x`", "a piece of quoted text", "a number".
- Describe what the user wrote, not compiler internals.
- Ownership errors (M2) use the human framing: *while something is being
  changed, nobody else may be looking at it.*
- Staged features name their milestone and give today's workaround
  (see E0117). A future feature must never die as a generic error.
- Teaching errors (S14, E0008–E0016) recognize a familiar foreign
  spelling, name the one Jet form, and then keep going as if the canonical
  form had been written — one foreign word never hides the rest of the
  file's problems.
- Typos get suggestions (edit distance ≤ 2): "did you mean `print`?"
- Fixes are imperative and specific: "add a closing `\"`", never
  "consider revising".

## Error code registry

| Code  | Stage | Meaning                                  |
|-------|-------|------------------------------------------|
| E0001 | jet   | character/escape/lone brace means nothing here |
| E0002 | jet   | unterminated text literal or interpolation |
| E0003 | parse | expected X, found Y                       |
| E0004 | parse | *retired in M1* (was: parameters staged)  |
| E0005 | parse | *retired in M1* (was: variables staged)   |
| E0006 | parse | *retired in M4* (was: `?` staged)         |
| E0007 | jet   | integer too large for 64 bits             |
| E0008 | parse | teaching: `def`/`func` → `fn` (S14)       |
| E0009 | parse | teaching: `let`/`let mut` → `val`/`var`   |
| E0010 | parse | teaching: `set` → `val`                   |
| E0011 | sema  | *retired in M10* (was: `println` → `print`) |
| E0012 | parse | teaching: `and`/`or`/`not` → `&&`/`\|\|`/`!` |
| E0013 | parse | teaching: `Text` → `String`               |
| E0014 | parse | teaching: `try` → `?` (M4 — real feature)   |
| E0015 | parse | teaching: `use` → `import` (M6)           |
| E0016 | parse | teaching: `match` → `switch` (S24)        |
| E0017 | parse | teaching: `read` → default parameter access (S10) |
| E0018 | parse | teaching: `write` → `mut` (S10)          |
| E0019 | parse | *retired in M6* (was: `import` staged; S16 shipped) |
| E0020 | parse | teaching: `None`/`Some`/… → `null`/`value` (S32) |
| E0021 | parse | teaching: `class` → `struct` (S29)              |
| E0022 | parse | teaching: `trait`/`interface` staged → M9 (S28) |
| E0023 | parse | teaching: `case`/`default` → switch arm syntax (S24) |
| E0024 | parse | teaching: `catch`/`except` → `or` / `== err` (M4) |
| E0025 | parse | teaching: `unwrap`/`expect` → `or panic(…)` (M4) |
| E0026 | parse | teaching: `throw`/`raise` → `err(…)` (M4) |
| E0027 | parse | teaching: `append` → `push`               |
| E0028 | parse | teaching: `Vec`/`dict` → `List`/`Map`     |
| E0030 | parse | teaching: `as` → `.to_float()` etc.       |
| E0031 | parse | teaching: `unsafe` / C-style FFI → `extern rust` (S50) |
| E0032 | parse | teaching: `lambda` / `fn(x){…}` → `(x) => …` (S46) |
| E0033 | parse | teaching: `\|x\| …` Rust pipes → `(x) => …` (S46) |
| E0034 | parse | teaching: `Type[Args]` → `Type<Args>` (S33) |
| E0035 | parse | teaching: `where` clauses → inline bounds |
| E0036 | parse | teaching: `dyn`/`Box` → trait name in type position |
| E0037 | sema  | teaching: `println!`/`eprintln!` → `print`/`io.eprint` |
| E0038 | sema  | teaching: `open(`/`File.open` → `fs.read` / `fs.write` |
| E0039 | sema  | teaching: `os.environ`/`getenv` → `env.get` |
| E0101 | sema  | no `main` function                        |
| E0102 | sema  | unknown function (with suggestion)        |
| E0103 | sema  | `print` arity                             |
| E0104 | sema  | wrong number of arguments                 |
| E0105 | sema  | duplicate definition                      |
| E0106 | sema  | redefining a built-in                     |
| E0107 | sema  | unknown name (with suggestion)            |
| E0108 | sema  | binding type doesn't match its value      |
| E0109 | sema  | operator type mismatch (incl. Int/Float mixing, `+` on text) |
| E0110 | sema  | condition isn't `Bool` (`if`/`while`/arm/logic operand) |
| E0111 | sema  | changing a `val`, const, or read-only parameter |
| E0112 | sema  | value doesn't fit where it's used (argument/print/interpolation) |
| E0113 | sema  | `return` value mismatch (wrong/missing/unexpected) |
| E0114 | sema  | a path reaches the end without `return`   |
| E0115 | sema  | `break`/`continue` outside a loop         |
| E0116 | sema  | valueless call used as a value            |
| E0118 | sema  | name already taken (no shadowing)         |
| E0119 | sema  | unknown type name                         |
| E0120 | sema  | moving/returning a borrowed parameter     |
| E0121 | sema  | value used after it was given away        |
| E0122 | sema  | `main` with parameters or a return type   |
| E0201 | sema  | `take` required; value can't be copied    |
| E0202 | sema  | `mut` required at call site               |
| E0203 | sema  | `take` on a non-consuming parameter       |
| E0204 | sema  | same value used while `mut` is active in one call |
| E0206 | sema  | `view` return can't point at this value   |
| E0207 | sema  | multiple unlabeled `ref` fields           |
| E0208 | sema  | `*` outside `unsafe`                      |
| L0201 | sema  | implicit `.clone()` at call site (lint)   |
| L0202 | sema  | auto-clone `Shared` inside loop (lint)    |
| E0301 | sema  | `impl` for unknown type                   |
| E0302 | sema  | unknown field (with suggestion)           |
| E0303 | sema  | struct/variant construction field errors  |
| E0304 | sema  | unknown enum variant (with suggestion)    |
| E0305 | sema  | pattern doesn't belong to value's type    |
| E0306 | sema  | pattern binding count mismatch            |
| E0307 | sema  | `switch` not exhaustive (lists missing)   |
| E0308 | sema  | bare `null` needs a known `T?` type       |
| E0309 | sema  | nested `T??` rejected                     |
| E0310 | sema  | `T?` used where plain `T` expected        |
| E0311 | sema  | static/instance method confusion          |
| E0312 | sema  | value `==` unsupported (field detail)     |
| L0301 | sema  | unreachable `switch` pattern arm (lint)   |
| E0401 | sema  | fallible value used where plain `T` expected |
| E0402 | sema  | fallible call ignored as a statement      |
| E0403 | sema  | `?` error type / return context mismatch  |
| E0404 | sema  | `ok`/`err` need a fallible context        |
| E0405 | sema  | `or` fallback type mismatch               |
| E0501 | sema  | empty `[]` / `[:]` needs a context type   |
| E0502 | sema  | type can't be a map key                   |
| E0503 | sema  | strings aren't indexable with `[ ]`       |
| E0504 | sema  | mixed-type list/map literal               |
| E0505 | sema  | wrong index/key type or bad slice target  |
| E0507 | sema  | collection changed while `for` reads it   |
| L0501 | sema  | slice copy inside a loop (lint)           |
| E0601 | sema  | `test` block in wrong position / none found |
| E0602 | jet   | import path escapes the project (`..` or outside entry tree) |
| E0603 | jet   | imported file / module not found |
| E0604 | jet   | import cycle (lists the loop) |
| E0605 | sema  | item exists in another file but is private |
| E0606 | jet   | ambiguous module name (lists every matching path) |
| E0701 | sema  | non-`std` `extern rust` crate missing `@version` pin |
| E0702 | sema  | type or access mode can't cross the FFI boundary |
| E0703 | jet   | `cargo` not installed (needed for `extern rust` crates) |
| E0704 | jet   | foreign crate fetch/build failed (cargo detail indented) |
| E0705 | jet   | `= "rust::path"` doesn't match the Jet signature |
| E0801 | sema  | lambda parameter type unknown |
| E0802 | sema  | escaping lambda captures non-clonable value without `take` |
| E0803 | sema  | calling a value that isn't a function |
| E0804 | sema  | self-recursive lambda binding |
| L0801 | sema  | escaping lambda silently cloned a capture (lint) |
| E0901 | sema  | method needs a generic bound |
| E0902 | sema  | orphan `impl` (neither type nor trait local) |
| E0903 | sema  | hand-written built-in trait impl staged |
| E0904 | sema  | can't infer a type argument |
| E0905 | sema  | type doesn't implement required trait |
| E0906 | sema  | trait impl missing methods |
| E0907 | sema  | trait impl signature mismatch |
| E0908 | sema  | duplicate trait impl |
| E0909 | sema  | generic instantiation too deep |
| E0951 | sema  | comptime code reaches an impure operation (shows call path) |
| E0952 | sema  | comptime budget exhausted (fuel) |
| E0953 | sema  | comptime panic = user-authored compile error (message verbatim) |
| E0954 | parse | teaching: `comptime val`/`comptime var`/`const` → `comptime x = …` |
| E0955 | sema  | `embed_file`: missing / unreadable / not UTF-8 |
| E0956 | sema  | construct not yet supported in comptime evaluation |
| E1001 | jet   | unknown std module |
| E1002 | jet   | local module shadows reserved first-party root/name |
| E1003 | sema  | U8 literal out of range |
| E1004 | sema  | unknown item in std module |
| E1201 | jet   | two versions of one package required (M12.1) |
| E1202 | jet   | lock file out of date (M12.1) |
| E1203 | jet   | `git` not installed (M12.1) |
| E1204 | jet   | store entry tree-hash mismatch / tamper (M12.1) |
| E1205 | sema  | FFI pin belongs in `[dependencies:rust]` (M12.1) |
| E1206 | jet   | manifest syntax/shape error (M12.1) |
| E1207 | jet   | registry dependency not yet supported (M12.2) |
| E1208 | jet   | toolchain `[package].jet` incompatible (M12.1) |
| E1209 | jet   | reserved section used non-empty (M12.1) |

## Process for a new diagnostic

1. Claim the next code here. 2. Write what/why/fix per the voice rules.
3. Add a tests/ui fixture + snapshot. 4. Ship. A diagnostic without a
snapshot test does not exist (invariant I4).
