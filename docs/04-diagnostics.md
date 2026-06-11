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
  (see E0006/E0117). A future feature must never die as a generic error.
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
| E0006 | parse | staged: `?` (errors as values) arrives in M4 |
| E0007 | jet   | integer too large for 64 bits             |
| E0008 | parse | teaching: `def`/`func` → `fn` (S14)       |
| E0009 | parse | teaching: `let`/`let mut` → `val`/`var`   |
| E0010 | parse | teaching: `set` → `val`                   |
| E0011 | sema  | teaching: `println` → `print`             |
| E0012 | parse | teaching: `and`/`or`/`not` → `&&`/`\|\|`/`!` |
| E0013 | parse | teaching: `Text` → `String`               |
| E0014 | parse | teaching: `try` → `?` (M4)                |
| E0015 | parse | teaching: `use` → `import` (M6)           |
| E0016 | parse | teaching: `match` → `switch` (S24)        |
| E0017 | parse | teaching: `read` → default parameter access (S10) |
| E0018 | parse | teaching: `write` → `mut` (S10)          |
| E0019 | parse | staged: `import` (multi-file) arrives in M6 (S16) |
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
| E0020 | parse | teaching: `None`/`Some`/… → `null`/`value` (S32) |
| E0021 | parse | teaching: `class` → `struct` (S29)              |
| E0022 | parse | teaching: `trait`/`interface` staged → M9 (S28) |
| E0023 | parse | teaching: `case`/`default` → switch arm syntax (S24) |
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

## Process for a new diagnostic

1. Claim the next code here. 2. Write what/why/fix per the voice rules.
3. Add a tests/ui fixture + snapshot. 4. Ship. A diagnostic without a
snapshot test does not exist (invariant I4).
