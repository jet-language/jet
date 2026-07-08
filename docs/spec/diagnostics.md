# Diagnostics

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
- Live teaching errors recognize a familiar non-canonical form, name the one
  Jet form, and then keep going as if the canonical form had been written.
  D-S14-PAUSE keeps old syntax spellings out of this default path.
- Typos get suggestions (edit distance ≤ 2): "did you mean `print`?"
- Fixes are imperative and specific: "add a closing `\"`", never
  "consider revising".

## Error code registry

| Code  | Stage | Meaning                                  |
|-------|-------|------------------------------------------|
| E0001 | jet   | character/escape/lone brace means nothing here |
| E0002 | jet   | unterminated text literal, interpolation, or block comment |
| E0003 | parse | expected X, found Y                       |
| E0004 | parse | *retired in M1* (was: parameters staged)  |
| E0005 | parse | *retired in M1* (was: variables staged)   |
| E0006 | parse | *retired in M4* (was: `?` staged)         |
| E0007 | jet   | integer too large for 64 bits             |
| E0008 | parse | *retired by D-S14-PAUSE* (was: `def`/`func` teaching) |
| E0009 | parse | *retired by D-S14-PAUSE* (was: `let`/`let mut` teaching) |
| E0010 | parse | *retired by D-S14-PAUSE* (was: `set` teaching) |
| E0011 | sema  | *retired in M10* (was: `println` → `print`) |
| E0012 | parse | *retired by D-S14-PAUSE* (was: foreign boolean-word teaching) |
| E0013 | parse | *retired by D-S14-PAUSE* (was: `Text` teaching) |
| E0014 | parse | *retired by D-S14-PAUSE* (was: `try` teaching) |
| E0015 | parse | *retired by D-S14-PAUSE* (was: `import` teaching) |
| E0016 | parse | *retired by D-S14-PAUSE* (was: `match` teaching) |
| E0017 | parse | *retired by D-S14-PAUSE* (was: `read` teaching) |
| E0018 | parse | *retired by D-S14-PAUSE* (was: `write` teaching) |
| E0019 | parse | *retired in M6* (was: `import` staged; S16 shipped) |
| E0020 | parse | *retired by D-S14-PAUSE* (was: foreign optional teaching) |
| E0021 | parse | *retired by D-S14-PAUSE* (was: `class` teaching) |
| E0022 | parse | *retired by D-S14-PAUSE* (was: `trait`/`interface` teaching) |
| E0023 | parse | *retired by D-S14-PAUSE* (was: `case`/`default` teaching) |
| E0024 | parse | *retired by D-S14-PAUSE* (was: `catch`/`except` teaching) |
| E0025 | parse | *retired by D-S14-PAUSE* (was: `unwrap`/`expect` teaching) |
| E0026 | parse | *retired by D-S14-PAUSE* (was: `throw`/`raise` teaching) |
| E0027 | parse | *retired by D-S14-PAUSE* (was: `append` teaching) |
| E0028 | parse | *retired by D-S14-PAUSE* (was: `Vec`/`dict` teaching) |
| E0029 | parse | two capability markers on one parameter (D-CAP7/D-MEM1) |
| E0030 | parse | *retired by D-S14-PAUSE* (was: `as` teaching) |
| E0031 | parse | teaching: `unsafe` / C-style FFI → `extern rust` (S50) |
| E0032 | parse | *retired by D-S14-PAUSE* (was: lambda teaching) |
| E0033 | parse | *retired by D-S14-PAUSE* (was: Rust pipe-lambda teaching) |
| E0034 | parse | teaching: `Type[Args]` → `Type<Args>` (S33) |
| E0035 | parse | teaching: `where` clauses → inline bounds |
| E0036 | parse | *retired by D-S14-PAUSE* (was: `dyn`/`Box` teaching) |
| E0037 | sema  | teaching: `println!`/`eprintln!` → `print`/`io.eprint` |
| E0038 | sema  | *retired by D-S14-PAUSE* (was: file-open teaching) |
| E0039 | sema  | teaching: `os.environ`/`getenv` → `env.get` |
| E0040 | sema  | teaching: `async`/`await` → blocking tasks/channels |
| E0041 | sema  | teaching: `Mutex`/`lock` → channels |
| E0043 | jet   | teaching: `jet install` → `jet fetch` |
| E0044 | parse | *retired by D-S14-PAUSE* (was: `switch` teaching) |
| E0045 | parse | *retired by D-S14-PAUSE* (was: `or` fallback teaching) |
| E0046 | parse | `?.` optional chaining reaches fields, not methods (S71) |
| E0047 | type | `?.` left side must be optional `T?` (S71, D-SG6) |
| E0048 | parse | teaching: positional tuples → named members (S73, D-SG7) |
| E0049 | parse | teaching: `.0` field access → named members (S73, D-SG7) |
| E0050 | parse | *retired by D-S14-PAUSE* (was: `while` teaching) |
| E0051 | parse | *retired by D-S14-PAUSE* (was: `for x in` teaching) |
| E0052 | parse | *retired by D-S14-PAUSE* (was: bare `test` teaching) |
| E0053 | parse | *retired by D-S14-PAUSE* (was: bare `pure` teaching) |
| E0054 | parse | *retired by D-S14-PAUSE* (was: bare `todo` teaching) |
| E0055 | parse | teaching: `#Audit("…")` retired → reason is now the argument of `#Unsafe("…")` (D-UNSAFE2) |
| E0056 | parse | *retired by D-S14-PAUSE* (was: `mut` capability keyword teaching) |
| E0057 | parse | *retired by D-S14-PAUSE* (was: `take` keyword teaching) |
| E0058 | parse | *retired by D-MEM1/S3* (was: `view` return keyword teaching → `&` sigil; `-> &T` returns no longer exist to point at) |
| E0059 | parse | teaching: bare `sanitizer fn` → `#Sanitizer fn` (D-TAINT-SAN) |
| E0060 | parse | teaching: retired C FFI marker spelling → `#Extern` / `#Bindgen` (D-CFFI-SYNTAX-REOPEN, D-CFFI-CANON1) |
| E0062 | parse | teaching: a contract marker written with `#` → write it with `@` (D-MARKER-FAMILY1, D-MARKERMOVE1/2/3) |
| E0063 | parse | teaching: a directive marker written with `@` → write it with `#` (D-MARKER-FAMILY1) |
| E0984 | parse | *retired by D-S14-PAUSE* (was: `when` teaching) |
| E0985 | parse | *retired by D-S14-PAUSE* (was: `val`/`var` binding teaching) |
| E0986 | parse | teaching: `-> Type`/`{` split from the closing `)` (S6-R layout) |
| E0998 | parse | teaching: retired explicit binding forms → `: Type ::` / `: Type :=` (D-BIND4) |
| E0992 | parse | teaching: implicit dispatch — a multi-arm `if` needs `==` between the subject and `{` (D-IF3) |
| E0993 | parse | ~~retired by D-MATCHARM1=A~~ — predicate/Bool arm heads are now allowed |
| E0994 | parse | teaching: a redundant `subject ==` on an arm head — the `if`'s `==` already applies it (D-IF3) |
| E0999 | parse | teaching: stacked `#[…]` marker lines → one `#[A, B]` list or lone `#A` (D-ATTR2) |
| E0101 | sema  | no `run` function                         |
| E0102 | sema  | unknown function (with suggestion)        |
| E0103 | sema  | `print` arity                             |
| E0104 | sema  | wrong number of arguments                 |
| E0105 | sema  | duplicate definition                      |
| E0106 | sema  | redefining a built-in                     |
| E0107 | sema  | unknown name (with suggestion)            |
| E0108 | sema  | binding type doesn't match its value      |
| E0109 | sema  | operator type mismatch (incl. Int/Float mixing, `+` on text) |
| E0110 | sema  | condition isn't `Bool` (`if`/`while`/arm/logic operand) |
| E0111 | sema  | changing a `::`, const, or read-only parameter |
| E0112 | sema  | value doesn't fit where it's used (argument/print/interpolation) |
| E0113 | sema  | `return` value mismatch (wrong/missing/unexpected) |
| E0114 | sema  | a path reaches the end without `return`   |
| E0115 | sema  | `break`/`continue` outside a loop         |
| E0987 | sema  | `break name@`/`continue name@` names a loop label not in scope (D-LOOPLABEL2) |
| E0988 | parse | teaching: old prefix `@name loop` / `break @name` → suffix `name@ loop` / `break name@` (D-LOOPLABEL2) |
| E0989 | sema  | `comptime if` condition is not a comptime expression (D-WHEN1) |
| E0990 | parse | *retired by D-MARKER-CANON1* (was: `@` marker-prefix teaching) |
| E0116 | sema  | valueless call used as a value            |
| E0118 | sema  | name already taken (no shadowing)         |
| E0119 | sema  | unknown type name                         |
| E0120 | sema  | moving/returning a parameter without move (`^`) access |
| E0121 | sema  | value used after it was given away        |
| E0122 | sema  | `run` returns something other than nothing or `Void ?` in run mode |
| E0123 | sema  | `for` range `step` must be a positive Int (S22, D-SG8) |
| E0124 | sema  | `if`-expression branches produce different types (S68, D-SG2) |
| E0125 | sema  | call-site label mismatch: transposed or unknown label (D-NARG-D4) |
| E0126 | sema  | default expression references a later parameter (D-NARG-D2) |
| E0127 | sema  | arithmetic on a distinct type without `@Numeric`, or between two different distinct types — incl. cross-unit mixing (D-DIST3, D-QUAL3) |
| E0128 | sema  | implicit coercion between a distinct type and its base (D-DIST3) |
| E0129 | sema  | distinct-over-distinct: base type is itself a distinct type (D-DIST1) |
| E0130 | sema  | `Int` and `BigInt` mixed without explicit construction (D-BIGINT1) |
| E0131 | sema  | `Float` and `Decimal` mixed (D-DECIMAL1) |
| E0132 | sema  | `BigInt` and `Decimal` mixed (D-BIGINT1/D-DECIMAL1) |
| E0133 | sema  | unsupported operator on `BigInt`/`Decimal` (D-BIGINT1/D-DECIMAL1) |
| E0134 | sema  | a numeric literal's unit suffix isn't a `#UnitFamily` member in scope (D-UNITLIT1) |
| E0135 | sema  | a compile-time literal outside a range type's declared bounds (D-RANGETYPE1) |
| E0136 | sema  | a runtime value constructed into a range type without the fallible `?` form (D-RANGETYPE1) |
| E0137 | parse | a range type's declared bounds are empty/reversed (`lo > hi`) (D-RANGETYPE1) |
| E0138 | sema  | an operation used on a nominal `distinct` type whose capability bundles don't grant it (D-CAPBUNDLE1) |
| E0139 | sema  | a `@Pre`/`@Post` contract condition uses an effect (D-PREPOST1) |
| E0140 | sema  | `#SingleUse` value dropped without being consumed at scope end (D-LIN1) |
| E0141 | sema  | `#SingleUse` value consumed on only one `if` branch (D-LIN1) |
| E0142 | sema  | `#SingleUse` value lent/shared instead of moved (D-LIN1) |
| E0143 | sema  | `drop` of a `#SingleUse` value outside an `#Unsafe("reason")` region/fn — the audited deliberate-discard hatch (D-LIN1-DROP) |
| E0144 | sema  | `result` used inside a `@Pre` condition — it only exists once the function has returned (D-PREPOST1) |
| E0145 | parse | `@Persist` on a binding that isn't module-level (D-PERSIST1) |
| E0147 | parse | two `{}` holes in a str-match pattern with no literal text between them (D-PARSESTR1/D-PARSESTR2) |
| E0148 | sema  | a str-match pattern used in an `if == {}` table with no `else` arm (D-PARSESTR1) |
| E0149 | sema  | a runtime `String` used where `Sql`/`Html` is expected (D-TYPEDTEXT1) |
| E0150 | sema  | typestate: an operation is called on a value in the wrong state (D-STATE1) |
| E0151 | sema  | typestate: `#State(X)` or `#Transition(A -> B)` references a state not in the `state TypeName { … }` declaration (D-STATE-DECL) |
| E0153 | sema  | protocol expansion failed to parse a generated handle fragment (D-PROTO1) |
| E0160 | sema  | `++`/`--` operand is not an assignable lvalue (D-INCR1) |
| E0161 | sema  | `++`/`--` on an immutable binding or read-only parameter (D-INCR1) |
| E0162 | sema  | `++`/`--` on a non-integer type (D-INCR1) |
| E0163 | sema  | `++`/`--` can't target an indexed slot (D-INCR1) |
| E0154 | parse | protocol message endpoint pair is not `client -> server` or `server -> client` (D-PROTO2) |
| E0805 | sema  | `yield` used outside a function declared `-> Stream<T>` (D-STREAMYIELD1) |
| E0806 | sema  | a generator's `return` carries a value (D-STREAMYIELD1) |
| E0807 | sema  | a `yield`ed value's type doesn't match the stream's element type (D-STREAMYIELD1) |
| L0151 | sema  | typestate: a declared state has no outgoing `#Transition(S -> …)` — a dead-end state (D-STATE-DECL, warning) |
| E0201 | sema  | `take` (`^`) required; value can't be copied |
| E0202 | sema  | `mut` (`&`) required at call site — write access not granted |
| E0203 | sema  | `take` on a non-consuming parameter       |
| E0204 | sema  | same value used while `mut` is active in one call |
| E0205 | sema  | `self.field = v` without write access (`&`) on the receiver (D-MUTSELF1) |
| E0206 | sema  | *retired by D-MEM1/S3* (was: `view` return can't point at this value; `-> &T` returns no longer exist) |
| E0207 | sema  | *retired by D-MEM1/S3* (was: a stored-reference `&T` field's owner ambiguous, D-REF-SHORTHAND1; stored-ref fields no longer exist) |
| E0208 | sema  | raw pointer op outside `#Unsafe`: postfix `p.*` deref or prefix `*x` raw-of (D-CAP9) |
| E0209 | sema  | a named binding passed where it would be silently cloned — Move-param arg without `^`, or a std constructor consuming a borrowed value (D-MEM1/S2; hard error, was lint `L0201`) |
| E0210 | parse | *retired by D-TYPE-ALIAS-CANON1* (was: pointer alias teaching) |
| E0211 | sema  | `copy x` on a value that can't be copied — a function, a trait value, or a type Jet doesn't know how to duplicate (D-CAP2/D-MEM1/S4) |
| L0201 | sema  | *retired by D-MEM1/S2* (was: implicit `.clone()` at call site, liveness-gated lint; superseded by hard error E0209 — no silent clone ever) |
| L0202 | sema  | auto-clone `Shared` inside loop (lint)    |
| L0203 | jet   | an inline script dependency (`use pkg#version;`) uses a loose/unpinned version selector (U11, D-JPK-SCRIPTDEP1) |
| L0204 | jet   | a `flake.nix`/`devenv.nix` field `jet bridge flake` couldn't translate into `env.*` form (U16) |
| L0205 | jetpack | build sandboxing is unavailable and fallback is allowed by policy (U28, D-JPK-NODAEMON1) |
| E0301 | sema  | `impl` for unknown type                   |
| E0302 | sema  | unknown field (with suggestion)           |
| E0303 | sema  | struct/variant construction field errors  |
| E0304 | sema  | unknown enum variant (with suggestion)    |
| E0305 | sema  | pattern doesn't belong to value's type    |
| E0306 | sema  | pattern binding count mismatch            |
| E0307 | sema  | `if` dispatch not exhaustive (lists missing)   |
| E0308 | sema  | bare `None` needs a known `T?` type       |
| E0309 | sema  | nested `T??` rejected                     |
| E0310 | sema  | `T?` used where plain `T` expected        |
| E0311 | sema  | static/instance method confusion          |
| E0312 | sema  | value `==` unsupported (field detail)     |
| E0313 | sema  | destructuring target's shape doesn't match the value (S74) |
| E0315 | sema  | list-pattern arity ≠ a known-length list literal (S74) |
| E0316 | sema  | D-PATR: range pattern on non-integer field, or `lo > hi` (empty range) |
| E0317 | sema  | D-PATO: or-pattern alternatives bind different names or types |
| E0318 | parse | C25/D-RANGE2: `..=` in an arm head — Jet's `..` is already inclusive; write `lo..hi` |
| E0319 | parse | C25/D-RANGE2: `step` in an arm head — `step` is a loop modifier, not an arm construct |
| E0320 | parse | teaching: `Type { … }` dotless construction → `Type.{ … }` (D-DOTCTOR1) |
| E0321 | parse | teaching: `impl Type: Trait` old colon separator → `impl Type.Trait` (D-IMPLDOT1) |
| E0322 | parse | assignment `=` in `if` condition — did you mean `==`? (D-ASSIGNCOND1) |
| E0323 | parse | `namespace` keyword not in Jet — use `module name { }` for in-file grouping (D-NAMESPACE1) |
| E0324 | sema  | type alias without type parameters — use `struct` for a distinct primitive name (D-TYPEALIAS1) |
| E0325 | parse | teaching: external inherent method `~~` connector → `.` (D-EXTMETH1) |
| E0326 | sema  | a partial struct destructure (`.{ … }`) with no trailing `..` (D-DESTRUCT1) |
| E0327 | sema  | a redundant `..` on a destructure that already names every field (D-DESTRUCT1) |
| E0328 | parse | value alternates (`\|`) mixed with `&&`/`\|\|` without grouping parens in an arm head (D-MATCHARM2=B) |
| E0330 | sema  | leading-dot enum variant (`.Variant`) with no inferable type from context (D-ENUMDOT2=A) |
| E0331 | parse | a payload on a variant group name (D-TAG1) |
| E0332 | sema  | a group name used as a value (D-TAG1) |
| E0333 | parse | a chained comparison changes direction (`a < b > c`) (D-CHAINCMP1) |
| E0334 | sema  | a trailing `{ }` block argument's slot isn't a zero-parameter function (D-TRAILBLOCK1) |
| E0335 | parse | a second trailing block, or a trailing block on a non-call (D-TRAILBLOCK1) |
| E0336 | sema  | `@[Patchable]` on a generic struct (D-PATCH1) |
| E0337 | sema  | `@[Patchable]` struct has a function-typed field (D-PATCH1) |
| E0338 | sema  | a cycle among computed-field formulas, including self-reference (D-FIELDPOL1) |
| E0339 | sema  | a computed field given in a struct literal or assigned to directly (D-FIELDPOL1) |
| E0340 | sema  | teaching: `read_dir` is not a Jet API — use `Path.from(p).walk()` (D-PATHFS1) |
| E0341 | sema  | *retired by D-CORENS-CANON1* (was: old first-party namespace teaching) |
| E0350 | sema  | `Any` type requested, but Jet has no general top type (D-DYNAMIC-TYPE1) |
| L0301 | sema  | unreachable dispatch pattern arm (lint)   |
| E0401 | sema  | fallible value used where plain `T` expected |
| E0402 | sema  | fallible call ignored as a statement      |
| E0403 | sema  | `?` error type / return context mismatch  |
| E0404 | sema  | `ok`/`err` need a fallible context        |
| E0405 | sema  | `??` fallback type/`return` mismatch       |
| E0406 | parse | old `Result<T, E>` fallible type syntax   |
| E0407 | sema  | `.drop()` reason missing or invalid (D-IGNORERET2) |
| E0410 | parse | `#Suppress` unknown argument (D-IGNORERET2) |
| E0411 | parse | unknown `pub(…)` visibility qualifier — only `pub(package)` exists (D-PUBPKG1) |
| E0412 | parse | teaching: `private` → `priv` inside `#PubFile` files (D-VISDEFAULT2) |
| E0413 | parse | `priv` used outside a `#PubFile` file (D-VISDEFAULT2) |
| E0414 | parse | redundant `pub` inside a `#PubFile` file (D-VISDEFAULT2) |
| E0415 | parse | section visibility labels `pub:` / `priv:` rejected (D-VISDEFAULT2 option C) |
| E0416 | parse | duplicate `#PubFile` marker in one file (D-VISDEFAULT2) |
| E0417 | parse | conflicting `pub` and `priv` on one item (D-VISDEFAULT2) |
| E0418 | parse | teaching: `#PublicFile` → `#PubFile` (D-VISDEFAULT2) |
| E0419 | sema  | `@MustUse` result ignored as a bare statement (D-MUSTUSE1) |
| E0420 | sema  | `:= uninit` binding read before it is given a value (D-UNINIT1, reworded D-UNINIT-SENTINEL1) |
| E0421 | parse | `:= uninit` binding needs a type annotation (D-UNINIT1, reworded D-UNINIT-SENTINEL1) |
| E0422 | parse | *retired by D-UNINIT-SENTINEL1* (was: `#Uninit` binding cannot have an initializer — structurally inapplicable now that `uninit` is the initializer) |
| E0423 | sema  | `:= uninit` binding's type is not plain data (D-UNINIT1, reworded D-UNINIT-SENTINEL1) |
| E0424 | sema  | `:= uninit` used without `use core.mem` (D-UNINIT1, reworded D-UNINIT-SENTINEL1) |
| E0426 | parse | teaching: retired `#Uninit name: Type` marker → `name: Type := uninit` (D-UNINIT-SENTINEL1) |
| E0427 | parse | *retired by D-MEM1/S3* (was: teaching retired `#Ref(owner) name: T` field form → `name: &T`, D-REF-SHORTHAND1; stored-ref fields no longer exist) |
| E0501 | sema  | empty `[]` needs a context type           |
| E0502 | sema  | type can't be a map key                   |
| E0503 | sema  | strings aren't indexable with `[ ]`       |
| E0504 | sema  | mixed-type list/map literal               |
| E0505 | sema  | wrong index/key type or bad slice target  |
| E0506 | sema  | `Set<T>` element type is not hashable (D-COLLBREADTH1) |
| E0507 | sema  | collection changed while `for` reads it   |
| E0510 | sema  | raw crypto expert API used without `use core.crypto.expert` and/or outside `#Unsafe` — use `crypto.seal`/`open` instead (D-CRYPTOENV1) |
| E0511 | sema  | `Expiring.force` / `Rotting.force` bypasses fallible expiry access — use `get(clock)` (D-TTLVAL1) |
| L0501 | sema  | slice copy inside a loop (lint)           |
| L0502 | sema  | float `==`/`!=` comparison is unreliable (D-SMELLLINT1) |
| L0503 | sema  | visually confusable single-character names in one scope (`l`/`I`/`1`, `O`/`0`) (D-CONFUSE1) |
| L0504 | sema  | money-like name holds `Float` instead of `Decimal` (D-DECIMAL1) |
| L0505 | sema  | heap growth in a loop after `use core.mem` — consider an arena (c26) |
| L0506 | sema  | hidden allocation inside `#Context` without an allocator (c26) |
| L0520 | sema  | auto-printable struct used in bare `{value}` without `Display` (migration lint, D-DISPLAY-SHAPE) |
| E0601 | sema  | `#Test` block in wrong position / none found |
| E0602 | jet   | `use` path escapes the project (`..` or outside entry tree) |
| E0603 | jet   | `use` target file / module not found |
| E0604 | jet   | `use` cycle (lists the loop) |
| E0605 | sema  | item exists in another file but is private |
| E0606 | jet   | ambiguous module name (lists every matching path) |
| E0607 | jet   | `module name;` file declaration not found (D-MOD1) |
| E0608 | sema  | function not defined in inline code module (D-MOD2) |
| E0609 | sema  | `use alias.item` but item is private (D-MOD3) |
| E0610 | sema  | `use alias.item` but alias is not a module (D-MOD3) |
| E0611 | sema  | `use alias.item` but item is not defined (D-MOD3) |
| E0612 | jet   | wildcard imports (`use math.*`) are not supported |
| E0613 | sema  | property-test (`#Test fn`) parameter has a type the runner can't generate (D-TEST1) |
| E0614 | sema  | unknown scope member, or a member used in a marker that declares none (D-DOTSCOPE1) |
| E0615 | sema  | a `.name { … }` scope-member statement outside a member-declaring marker block (D-DOTSCOPE1) |
| E0616 | sema  | `.setup` is not the first statement in the test (D-DOTSCOPE1) |
| E0617 | sema  | a scope member has the wrong argument shape (D-DOTSCOPE1) |
| E0618 | sema  | a scope member is nested instead of a top-level statement of the marker block (D-DOTSCOPE1) |
| E0631 | sema  | an arena `view` escapes its region — returned, stored, given away, or captured (D-ALLOC2/D-REGION1) |
| E0632 | sema  | an arena `view` is read after its arena was `reset`/`free`d (D-ALLOC2) |
| E0701 | sema  | non-`std` `extern rust` crate missing `@version` pin |
| E0702 | sema  | type or access mode can't cross the FFI boundary |
| E0703 | jet   | `cargo` not installed (needed for `extern rust` crates) |
| E0704 | jet   | foreign crate fetch/build failed (cargo detail indented) |
| E0705 | jet   | `= "rust::path"` doesn't match the Jet signature |
| E0740 | sema  | a function's inferred effects exceed its declared `#(…)` bound (D-EFF1) |
| E0741 | sema  | an effect used inside a `#Caps(…)` region is not in its cap list (D-EFF1) |
| E0742 | sema  | a trait-method impl uses effects beyond the trait method's declared bound (D-EFF3) |
| E0711 | sema  | the capability handle bound by a `#Grant(…)` region escapes its scope — returned, stored, or captured (D-SCAP1) |
| E0712 | sema  | an effect used inside a `#Grant(…)` region has no capability — it isn't in the grant's list (D-SCAP1) |
| E0721 | sema  | an untrusted (`#Tainted`) value reaches a sink effect (`Db`/`Exec`/`Net`) without passing through a `#Sanitizer fn` (D-TAINT1) |
| E0725 | sema  | a `#Replayable` function reaches ambient `Time`/`Rand`/`Net`/`Io` (D-REPLAY1) |
| E0731 | sema  | a `tag` is used where dispatch/methods are expected — `derive`d, or implemented/used as a trait (D-QUAL2) |
| E0732 | sema  | a method is declared in a `tag` body, but tags have no methods (D-QUAL2) |
| E0745 | sema  | a `@Pure fn` also carries a non-empty `#(…)` effect list — a contradiction (D-EFF1) |
| E0746 | sema  | an irreversible effect (Net/Fs/Exec) used directly inside a `#Transact { … }` block — can't be rolled back (D-TXN2) |
| E0747 | sema  | a callback argument exceeds its parameter's effect bound (`@Pure fn(…)` / `#(E) fn(…)`) (D-EFF2) |
| E0748 | sema  | `#(via f)` names a non-existent parameter, or one that isn't a function type (D-EFF2) |
| E0749 | sema  | a function reaches an effect it prohibits with `#(!E)` in its own call graph (D-PROP1=A) |
| E-WEB-ABI-TYPE | sema | a JS/WASM boundary type is not ABI-safe (D-JSBIND1) |
| E-WEB-CROSS-PARTITION | sema | a function in one web bucket calls a function in another (D-WASM1) |
| E-WEB-TARGET-BROWSER | sema | a Wasm-pinned function also carries the `Browser` effect (D-WASM1) |
| E-WEB-TIR-UNSUPPORTED | driver | a web-targeted executable body is outside the checked TIR boundary (D-WEBTIR1) |
| E-OSTARGET-MIXED-AXIS | sema | a `#Target(Os.*)`-gated impl's file/module also carries a web-bucket ceiling (D-OSTARGET1) |
| E-OSTARGET-UNMATCHED-CALL | sema | a function/method not gated to match takes or returns a value of a `#Target(Os.*)`-gated type (D-OSTARGET1) |
| E-OSTARGET-BUILD-CONTEXT | sema | a `comptime if … == { }` OS dispatch's subject is not `build.os` (D-OSTARGET2) |
| E-OSTARGET-DISPATCH-ARM | sema | a `comptime if build.os == { }` arm head is not a bare `.Linux`/`.Macos`/`.Windows` variant, or repeats one (D-OSTARGET2) |
| E-OSTARGET-DISPATCH-EXHAUSTIVE | sema | a `comptime if build.os == { }` dispatch leaves some target OS uncovered with no `else` (D-OSTARGET2) |
| E0760 | parser | `#Context` field uses `=` instead of `:` (D-CTX1, S17) |
| E0761 | parser | unknown `#Context` field name (v1 allows only `allocator`, `logger`, `deadline`) |
| E0762 | sema   | `#Context` field type mismatch (`allocator` must be an allocator handle; `deadline` must be Int epoch-ms) |
| E3001 | runtime | panic report with Jet source location, function name, source-line context box, and (in debug builds) safe local values (E2-M12, D-OBS1/D-OBS2) |
| E3002 | runtime | error-return trace entry on a `?`-propagated failure, Zig-style (E2-M12, D-OBS1) |
| E3003 | runtime | deadline exceeded at a wait/IO point while a `#Context(deadline: …)` budget is active (D-DEADLINE1) |
| E3005 | runtime | a `@Pre`/`@Post` contract clause failed — checked in every build, not a debug/release split (D-PREPOST1) |
| E3101 | sema  | low-level memory operation used outside an `#Unsafe("…")` block (D-LL1/D-UNSAFE2) |
| E3102 | sema  | low-level memory vocabulary used without `use core.mem` (D-LL1/D-UNSAFE2) |
| E3103 | sema  | `#Unsafe fn` called without an enclosing `#Unsafe("…")` block (D-UNSAFE2) |
| E3104 | sema  | value allocated in an arena used after `arena.reset()` or `arena.free()` (D-ALLOC-D) |
| E3110 | sema  | invalid swizzle lane on vector/SIMD type (D-SWIZZLE1) |
| E3111 | sema  | overlapping write swizzle repeats a lane (D-SWIZZLE1) |
| L3101 | sema  | `#Unsafe` block/function missing its reason argument — add `#Unsafe("…")` (D-UNSAFE2, D-UNSAFE-REASON1) |
| L3102 | sema  | `#Impure` block missing its reason argument — write `#Impure("…") { … }` (D-CTEFFECT1) |
| E3201 | jet   | C library `<lib>` not found (hangar + pkg-config) |
| E3202 | sema  | pointer/gated type crosses C boundary outside `#Unsafe` / `core.mem` |
| E3203 | sema  | non-C-ABI type in `#Extern` / `#Bindgen` fn signature |
| E3204 | sema  | two C `use` forms for the same lib in one file |
| E3205 | sema  | overlay symbol clashes with bindgen (incompatible signature) |
| E3206 | parse | user declared reserved `__bindgen__` segment |
| E3207 | parse | `#Bindgen` outside generated `.jet/bindings/c/` file |
| E3208 | jet   | `jet bind` / header translation failed |
| E3209 | jet   | linker couldn't find a declared C library at link time |
| E3210 | jet   | C library auto-provision from nixpkgs failed |
| E3301 | sema  | OS-dependent std API called in a `--freestanding` build |
| E3302 | jet   | target triple unknown or toolchain component missing |
| E3303 | sema  | freestanding build allocates memory with no global allocator |
| E3410 | sema  | Tier-2 comptime effect (`core.files`/`env`/`io`/`exec`) called outside a `#Impure` gate (D-CTEFFECT1) |
| E3411 | sema  | Tier-2 comptime effect inside `#Impure` gate but `--allow-impure` not passed (D-CTEFFECT1) |
| E3412 | sema  | `core.net.{method}()` is not available at comptime (only `fetch` is Tier-1) |
| E3413 | sema  | comptime `fetch` sha256 mismatch — content hash doesn't match the `sha256:` pin (D-CTEFFECT1 / D-NETDEP1=A) |
| E3414 | sema  | comptime `fetch` failed — bad URL, unsupported scheme, network error, or non-UTF-8 content (D-CTEFFECT1 / D-NETDEP1=A) |
| E4201 | sema  | HTTPS client TLS handshake failed before any response was received (D-TLS1) |
| E4202 | sema  | HTTPS client certificate could not be trusted (D-TLS1) |
| E4203 | sema  | HTTPS client could not find usable system certificate roots (D-TLS1) |
| E3401 | sema  | impure call inside a `@Pure fn` / pure-eval context (call-trace path) |
| E3402 | sema  | package build attempted ambient I/O or network (names the call) |
| E3403 | sema  | non-deterministic construct in pure evaluation (e.g. time/random) |
| E1801 | repl  | per-input fuel cap hit — snippet ran more than ~10M interpreter steps |
| E1802 | repl  | hard-rejected feature in the REPL (FFI, tasks, `#Unsafe`, OS-level APIs) |
| E0801 | sema  | lambda parameter type unknown |
| E0802 | sema  | escaping lambda captures non-clonable value without `take` |
| E0803 | sema  | calling a value that isn't a function |
| E0804 | sema  | self-recursive lambda binding |
| L0801 | sema  | escaping lambda silently cloned a capture (lint) |
| E0850 | sema  | D-GENMOD2=A: module alias target not found in scope |
| E0851 | sema  | D-GENMOD2=A: wrong number of type/value arguments to module alias |
| E0852 | sema  | D-GENMOD2=A: type argument does not satisfy bound |
| E0853 | sema  | D-GENMOD2=A: value argument has wrong type |
| E0854 | sema  | D-GENMOD2=A: generic module body contains non-fn item (MVP restriction) |
| E0855 | sema  | D-GENMOD2=A: circular module alias instantiation |
| E0901 | sema  | method needs a generic bound |
| E0902 | sema  | orphan `impl` (neither type nor trait local) |
| E0903 | sema  | hand-written built-in trait impl staged |
| E0904 | sema  | can't infer a type argument |
| E0905 | sema  | type doesn't implement required trait |
| E0906 | sema  | trait impl missing methods |
| E0907 | sema  | trait impl signature mismatch |
| E0908 | sema  | duplicate trait impl |
| E0909 | sema  | generic instantiation too deep |
| E0910 | sema  | `@PublishedSchema` struct made a breaking shape change (drop / type-change / add-without-default) with no migration to bridge it, or a declared migration op is nonsensical |
| E0911 | parse | migration block uses an unknown verb (`drop`→`remove`, `reorder` not needed) |
| E0912 | sema  | *retired by D-MEM1/S2* (was: frozen public capability signature drift under `library { api: stable/explicit }`, D-CAP8/c129; the `api:` field and capability freeze are gone — `ApiFreeze`'s snapshot survives as unconditional pub-fn semver diffing, E1218/E2601) |
| E0913 | sema  | trait impl missing associated type (D-LIB2) |
| E0914 | sema  | unknown interpolation selector after `@` (D-DISPLAYDBG2) |
| E0915 | sema  | bare `{value}` on a type without `Display` (D-DISPLAY-SHAPE) |
| E0916 | sema  | auto-derived `Debug` blocked by a non-debuggable field (D-DEBUG-REDACT) — *defined, not yet emitted* |
| E0917 | sema  | `@InlineAlways fn` calls itself — inlining a recursive call has no fixed expansion (D-METHODMACRO1) |
| E0918 | sema  | `@InlineAlways fn` had its address taken (stored, returned, or passed as a callback) instead of being called directly (D-METHODMACRO1) |
| E0919 | sema  | `@InlineAlways fn` body exceeds the statement ceiling `@InlineAlways` enforces (D-METHODMACRO1) |
| E0920 | parse | a function/method written with both `@Inline` and `@InlineAlways` (D-METHODMACRO1) |
| E0921 | sema  | `policy no_alloc` floor violation — an allocation-shaped expression in the module's own function bodies (D-MEM1/S7, D-NOALLOC-SEM1) |
| E0951 | sema  | comptime code reaches an impure operation (shows call path) |
| E0952 | sema  | comptime budget exhausted (fuel) |
| E0953 | sema  | comptime panic = user-authored compile error (message verbatim) |
| E0954 | parse | *retired by D-S14-PAUSE* (was: two-keyword comptime binding teaching) |
| E0955 | sema  | comptime file input missing / unreadable (`embed_file` also: not UTF-8) |
| E0956 | sema  | construct not yet supported in comptime evaluation |
| E0957 | sema  | `embed_file`/`embed_bytes` path or `find` glob not a literal, absolute, or escaping via `..` |
| E0958 | sema  | **retired** (D-CTEFFECT1 2026-06-25): replaced by E3410 (Tier-2 effect without `#Impure` gate) |
| E0960 | parse | module contribution names a non-reserved namespace (U3: `env`/`system`/`image`) |
| E0961 | sema  | fan-out callee is not callable with exactly one argument (S75) |
| E0962 | sema  | fan-out item doesn't fit the parameter type (S75) |
| E0963 | sema  | positional destructure count ≠ fixed-size list length (S76) |
| E0964 | sema  | length-changing op (`push`/`pop`/`insert`) on a fixed-size `[T#N]` (S76) |
| E0965 | sema  | compile-time or refinement-proven index out of range on `[T#N]` (S76, D-REFINE1) |
| E1310 | parse/sema | variadic parameter not last, or variadic param has a default (D-VARIADIC1) |
| E1311 | sema  | spread operand is not a list (D-VARIADIC1) |
| E1312 | sema  | call spread at a callee without a variadic rest parameter (D-VARIADIC1) |
| E1313 | sema  | trait-bounded variadic call-site argument doesn't implement the bound trait (D-ANY-JAI1) |
| E1314 | sema  | trait-bounded variadic parameter used outside a `loop x in name { … }` loop (D-ANY-JAI1) |
| E0966 | jetpack | module contribution value isn't a struct literal of its namespace's type (`Env`/`System`/`Image`) |
| E0967 | jetpack | §6 merge conflict: a named source or scalar setting got irreconcilable values |
| E0968 | jetpack | a module `sources:` entry isn't a `provider@target` ref (U6/U8) |
| E0969 | jetpack | an `imports:` directive isn't `find("<dir>")` with a literal path (U4) |
| E0970 | jetpack | `imports: find("<dir>")` points at a directory that doesn't exist (U4) |
| E0971 | jetpack | a discovered module has its own `imports:` (liftability law, U4) |
| E0972 | jetpack | unknown field on a `System` / `Image` / `Service` record (U11/U14) |
| E0973 | jetpack | `target` (or cross-compile platform) isn't a known platform value (U13) |
| E0974 | jetpack | a `System` has no `target` (U11) |
| E0975 | jetpack | a `Service` has no `enable`, or `enable` isn't `true`/`false` (U12) |
| E0976 | jetpack | an `Image` uses a frozen disk-image format (D-JETOS-FREEZE1) |
| E0977 | jetpack | an `Image` has no active `from`, or restates a frozen system field (D-JETOS-FREEZE1) |
| E0978 | jetpack | an `Image` `from:` references a frozen/unknown `System` (D-JETOS-FREEZE1) |
| E0979 | jetpack | a `jet os` target has no host (D-JPK-OSHOST1) |
| E0980 | jetpack | a `jet os` host names a `System` the config doesn't define (D-JPK-OSHOST1) |
| E0981 | jetpack | a `jet os` config file doesn't exist (D-JPK-OSHOST1) |
| E0982 | jetpack | `use <pkg>` names an `executable` package — executables go on PATH, not `use` (U17) |
| E0983 | jetpack | `use <pkg>` names a declared `library` dependency that hasn't been realized yet (U17) |
| E1001 | jet   | unknown core module |
| E1002 | jet   | local module shadows reserved first-party or foreign root/name |
| E1003 | sema  | integer literal out of range for its width |
| E1004 | sema  | unknown item in core module |
| E1005 | sema  | overflow opt-in not wrapping a single integer op |
| E1006 | sema  | `use core.*` import or emitted helper exceeds package `runtime:` ceiling (D-RINGLAYER1) |
| E1301 | sema  | `ArgsSpec.flag` or `ParsedArgs.flag` called with wrong arity (D-ARGS1) |
| E1302 | sema  | `ArgsSpec.option` or `ParsedArgs.option` called with wrong arity (D-ARGS1) |
| E1303 | sema  | `ArgsSpec.positional` or `ParsedArgs.positional` called with wrong arity (D-ARGS1) |
| E1304 | sema  | `ArgsSpec.parse` called with wrong arity (D-ARGS1) |
| E1305 | sema  | `@[Cli]` struct field has a type with no CLI flag mapping (D-CLIFLAG1) |
| E1306 | sema  | two `@[Cli]` fields (or a field and the reserved `--help`) derive the same flag name (D-CLIFLAG1) |
| E1307 | sema  | subcommand `enum` variant's payload isn't a `@[Cli]`-derived struct (D-CLIFLAG1) |
| E1308 | sema  | `fn run`'s entry parameter isn't a `@[Cli]` struct or an enum of `@[Cli]` payloads (D-CLIFLAG1) |
| E1101 | sema  | task capture needs ownership              |
| E1102 | sema  | value crossing task/channel boundary is not sendable |
| E1103 | sema  | `.detach()` called on a task that had a sendability error at spawn (D-DETACH1) |
| E1104 | sema  | `#Layout(c)` struct contains a growable field (D-REPRC1) |
| E1105 | sema  | `#Layout(packed)` / `#Layout(align(N))` not yet supported (D-REPRC1 reserved) |
| E1106 | sema  | `.detach()` called on a task that captured a `view` borrow (D-DETACH1) — the view may outlive the caller |
| E1107 | sema  | `columnar [T]` per-container layout prefix is reserved (D-SOA2C) |
| E1108 | sema  | list method not yet supported on a `#Layout(columnar)` list (D-SOA1) |
| E1109 | sema  | partial `#Layout(columnar: …)` is deferred — whole-struct only in v1 (D-SOA2B) |
| E1110 | sema  | `.task { … }` outside a `taskgroup` scope, or on the wrong handle (D-TASKSCOPE1) |
| L1101 | sema  | Task value dropped without `.join()` or `.detach()`  |
| W0410 | sema  | `core.random.bytes` output used in a crypto context — `core.random` is PRNG only; use `core.crypto.random.bytes` (D-RANDSPLIT1) |
| E2301 | sema  | *retired by D-MEM1/S3* (was: returned `view` outlives the local that owns it; `-> &T` returns no longer exist) |
| E2302 | sema  | *retired by D-MEM1/S3* (was: stored `ref` field would point at something that dies first; `&T` fields no longer exist) |
| E2303 | sema  | a `View<T>` crosses a task/channel boundary (E2-M5; emitted as E1102) |
| E2304 | sema  | *retired by D-MEM1/S3* (was: an indexed/sliced piece can't be handed back as a `view`; `-> &T` returns no longer exist) |
| E2305 | sema  | a `View<T>` (`.view(...)`) escapes the scope of the list it borrows from (D-DYNARRAY1) |
| E2306 | sema  | *retired by D-MEM1/S3* (was: `#Ref(label)` on a `&T` field names no in-scope value of the referent type, D-REF-SHORTHAND2; stored-ref fields no longer exist) |
| L2301 | sema  | *retired by D-MEM1/S3* (was: advisory naming a borrowed return's source; `-> &T` returns no longer exist) |
| E2307 | sema  | a string view (`.trim()`/`.after()`/`.before()`) escapes the scope of the `String` it borrows from (D-MEM1 stage S5) |
| E1201 | jet   | two versions of one package required (M12.1) |
| E1202 | jet   | lock file out of date (M12.1) |
| E1203 | jet   | `git` not installed (M12.1) |
| E1204 | jet   | store entry tree-hash mismatch / tamper (M12.1) |
| E1206 | jet   | manifest syntax/shape error (M12.1) |
| E1207 | jet   | registry dependency not yet supported (M12.2) |
| E1208 | jet   | toolchain `jet:` field in `pkg.jet` incompatible (M12.1) |
| E1209 | jet   | reserved section used non-empty (M12.1) |
| E1210 | jet   | unknown or reserved target in `packages:` block (D-TGT1/D-TGT2) |
| E1211 | jet   | `packages:` block-form entry uses the removed `kind:` field — write `targets:` (D-TGT1) |
| E1212 | jet   | package declared in `packages:` but no `module <name>` found in source tree (U10) |
| E1213 | jet   | package declared in `packages:` but `module <name>` found in multiple files (U10) |
| E1214 | jet   | `jetpack.toml` has a malformed line — not a valid `key = "value"` assignment or `[table]` header (D-JPK-FILES) |
| E1215 | jet   | `jetpack.toml` contains an unknown table or key name, with a did-you-mean suggestion (D-JPK-FILES) |
| E1216 | jet   | a `targets:` block has an unknown field (D-TGT3) |
| E1217 | jet   | a dependency in `pkg.jet` has no locked revision — `--locked`/publish needs every dep pinned (D-SUPPLY1) |
| E1218 | jet   | a breaking public-API change is published under a non-major version bump (D-SUPPLY1) |
| E1219 | jet   | unknown build profile name passed to `--profile` (D-BUILDPROFILE1) |
| E1220 | jet   | a transitive dependency uses an effect outside the `pkg.jet` `effects:` budget (D-EFFBUDGET1) |
| E1221 | jet   | a malformed `effects:`/`grants:` block in `pkg.jet` (D-EFFBUDGET1) |
| E1225 | jet   | `jetpack.toml` uses the retired `[packages]` monorepo index (D-WORKSPACE1) |
| E1226 | jet   | a retired manifest filename (`pack.jet`/`payload.jet`/`jet.toml`) found where `pkg.jet` belongs (D-JPK-FILENAME2) |
| E1227 | jet   | `jet` and the `jetpack`/`jetos` engine binary disagree on protocol version (D-JPK-DISPATCH1) |
| E1228 | jet   | an engine verb needs an engine binary (`jetpack`/`jetos`) that isn't installed (D-JPK-DISPATCH1) |
| E1229 | jet   | a role-module contribution uses the retired `module name { ns.path: Type.{ } }` form (D-JPK-MODBODY1) |
| E1230 | jet   | a bare/path-form ref matched more than one workspace member (D-MONOREF1) |
| E1231 | jet   | a bare/path-form ref matched no workspace member (D-MONOREF1) |
| E1232 | jet   | a monorepo source could not be fetched — sparse subtree checkout and full-clone fallback both failed (D-MONOREF1) |
| E1233 | jet   | an in-repo dependency names a package outside the source's workspace index (D-MONOREF1) |
| E1234 | jet   | `jet publish` refused: the version already exists in the registry index and is not yanked — versions are immutable (D-VERSION1) |
| E1235 | jet   | `jet publish`/`jet yank` couldn't reach the registry index (git clone/pull/push failed) |
| E1236 | jet   | a build step reached the network without a locked `fetch(url, sha256:)` (D-JPK-ADAPTER1) |
| E1237 | jet   | a build step wrote outside the package output root (D-JPK-ADAPTER1) |
| E1238 | jet   | a build recipe named a tool that is not a realized `Pkg` dependency (D-JPK-ADAPTER1) |
| E1239 | jet   | `module workspace` declared in more than one file (discovery-by-declaration, D-JPK-FILENAME2) |
| E1240 | jet   | no realized Rust toolchain and no Nix to build an `extern rust` bridge dep (D-JPK-BUILDTOOL1) |
| E1241 | jet   | a staged `core.<ring>` artifact is missing for the active platform (D-JPK-RINGSHIP1) |
| E1242 | jet   | a captured `fleet.<name>` host references an unknown captured `system.<name>` (D-JETOS-FREEZE1) |
| E1243 | jet   | `jet push <fleet>` on a valid fleet — deployment gated on single-host jetos realization (U15, Phase D) |
| E1244 | jet   | an unknown field on a captured `Fleet` record (D-JETOS-FREEZE1) |
| E1245 | jet   | a captured `Fleet` with no `hosts:` map (D-JETOS-FREEZE1) |
| E1246 | jet   | a package signature doesn't verify against its pinned public key (D-PKGSIGN1) |
| E1247 | jet   | a registry with `require_signed: true` served an unsigned package (D-PKGSIGN1) |
| E1248 | jet   | `jet keygen` refused: a signing key already exists (use `--force`) (D-PKGSIGN1) |
| E1249 | jet   | a `jet:` toolchain pin isn't a valid version/channel ref (D-JPK-TOOLCHAIN1) |
| E1250 | jet   | a `jet:` channel pin is unlocked under `--offline`/CI — no `[[toolchain]]` lock entry (D-JPK-TOOLCHAIN1) |
| E1251 | jet   | the pinned Jet toolchain has no prebuilt object for this platform — never source-built (D-JPK-TOOLCHAIN1) |
| E1252 | jet   | `jet init` refused: a `pkg.jet` already exists here (D-JPK-TOOLCHAIN1) |
| E1253 | jet   | an inline script dependency (`use pkg#version;`) didn't resolve (U11, D-JPK-SCRIPTDEP1) |
| E1254 | jet   | project-level `jet dev` has neither `fn dev()` nor `fn run()` in its entry file (U19, D-JPK-DEVCOMPOSE1) |
| E1255 | jet   | an untrusted project env hit a non-interactive path with no `--trust`/prior grant (U19, D-JPK-DEVCOMPOSE1) |
| E1256 | jet   | `jet bridge flake` or foreign-flake detection needs `nix`, which isn't on PATH (U16) |
| E1257 | jet   | a `target: plugin` package's exported interface changed incompatibly since the last frozen build (D-PLUGIN-VERSION1=A) |
| E1258 | jet   | a `target: plugin` package's own code uses an effect — plugins are deny-by-default, zero host capabilities (D-PLUGIN1=B) |
| E1259 | jet   | couldn't build a plugin's WASM Component — missing/failed `rustc`/`wasm-tools` toolchain (D-DEP-WASM1=A) |
| E1260 | jet   | a plugin's exported `pub fn` isn't all-`Int` or all-`Float` (v1 plugin scope, D-PLUGIN-EXPORT1=A) |
| E1261 | jet   | a dev-supervised service never became healthy within the readiness timeout (U12) |
| E1262 | jet   | a dev-supervised `Service` field jetpack doesn't recognize at supervision time (U12) |
| E1263 | jetpack | `jetpack secrets get <name>` names an entry that isn't in the encrypted store (U13) |
| E1264 | sema  | a function reaches `core.vault.get` without declaring the `Secret` effect (U13, D-JPK-SECRETCRYPTO1) |
| E1265 | comptime | `core.vault.get` reached from a build-time (comptime) context — secrets are never readable at build time (U13) |
| E1266 | jet   | an `Image`'s `kind:` isn't active `.Oci`, or disagrees with what `from:` names (D-JPK-IMAGE1, D-JETOS-FREEZE1) |
| E1267 | jet   | an `.Oci` image's `from: packages.<name>` doesn't name an `executable`-kind package (U14, D-JPK-IMAGE1) |
| E1268 | jetpack | `jet image <name> --push` — gated on TLS support for registry pushes, which doesn't exist yet (U14, D-JPK-IMAGE1) |
| E1269 | jet   | an `.Oci` image field (`kind`/`expose`/`env_vars`/`files`/`base`) isn't shaped the way D-JPK-IMAGE1 spells it (U14) |
| E1270 | jetpack | an ad-hoc adapter declaration/source/recipe is not shaped or realizable (U20, D-JPK-ADAPTER1) |
| E1271 | jetpack | a channel source ref (`#latest`/`#main`/`#vN.x`) is not locked, or cannot be resolved during `jetpack update` (U21, D-JPK-CHANNEL1) |
| E1272 | jetpack | one or more package refs need the Nix bridge on a machine without `nix` (U23, D-JPK-NONIX1) |
| E1273 | jetpack | a recipe-backed package build failed at a logged build step (U27, D-JPK-BUILDDBG1) |
| E1274 | jetpack | no persisted build logs/explain data exist for the requested package/ref (U27, D-JPK-BUILDDBG1) |
| E1275 | jetpack | sandbox fallback is forbidden by policy but unprivileged sandboxing is unavailable (U28, D-JPK-NODAEMON1) |
| E1276 | jetpack | `--offline` would need network access or a missing local package object (U29, D-JPK-OFFLINE1) |
| E1277 | jetpack | a jetos option key uses a retired namespace (D-JPK-OSNS1) |
| E1278 | jetpack | a jetos activation proof is incomplete (D-WD8) |
| E1279 | jetpack | VM/media proof tools are missing (D-JOS-VMDEPS1) |
| E1280 | jetpack | the first-party CachyOS kernel package is missing (D-JOS-KERNELSRC1) |
| E1281 | jetpack | the first-party systemd init package is missing (D-JPK-OSINIT1) |
| E1282 | jetpack | the first-party CachyOS kernel package lacks bootable kernel/initrd artifacts (D-JOS-KERNELSRC1) |
| E1283 | jetpack | the first-party systemd package lacks init artifacts (D-JPK-OSINIT1) |
| E1284 | jetpack | the first-party CachyOS kernel package lacks source-built recipe/builder provenance (D-JOS-KERNELBOOTSTRAP1) |
| E1285 | jetpack | the VM install/reboot harness exists but no guest proof has run (D-JOS-VMCOMMAND1) |
| E1286 | jetpack | the first-party CachyOS kernel source recipe failed to build boot artifacts |
| E2001 | jet   | `pkg.jet` requests an edition this toolchain can't provide (E2-M2, D-REL3) |
| E2002 | jet   | a deprecated item is used past its migration window (E2-M2, D-REL5) |
| E2101 | jet   | unknown subcommand on the command line, with a "did you mean" (E2-M3, D-DX) |
| E2102 | jet   | unknown or ambiguous flag on the command line, with a suggestion (E2-M3, D-DX) |
| E2201 | interp | `jet dev` can't interpret a feature (task/FFI/`#Unsafe`/native std); names it and `jet build`/`jet run` (E2-M4, D-DEV1) |
| E2202 | interp | `jet dev` interpreter step budget exhausted — likely an unbounded loop (E2-M4) |
| E2203 | interp | `jet debug` can't step through a feature its interpreter doesn't cover (task/FFI/`#Unsafe`/native std); names it and `jet build`/`jet run` (D-DBG3) |
| E2204 | interp | `jet debug` session ended early — the user typed `quit` at the `(jet)` prompt before the program finished (D-DBG3) |
| E2210 | interp | a `jet dev`/`jet serve` edit changed a type surface, so the dev loop restarts instead of swapping (c77, D-HOTSWAP1) |
| L2001 | jet   | a deprecated item still compiles but should be migrated; suggests `jet fix` (E2-M2, D-REL5) |
| L2101 | jet   | `jet doctor` advisory: a rustc / cache / PATH problem with a fix (E2-M3, D-DX2) |
| E2701 | runtime | malformed input to a ring library parse function — row/line number and detail (E2-M9) |
| E2702 | sema  | crypto API misuse at the boundary — reserved for future statically-detectable crypto errors (E2-M9, D-LR3) |
| L2701 | sema  | advisory: regex pattern may catastrophically backtrack; suggest an anchor (E2-M9) |
| E2910 | sema  | `reactive.derived`/`effect` argument isn't a lambda (D-REACT1) |
| E2911 | sema  | `reactive.derived`/`effect` lambda takes parameters (D-REACT1) |
| E2912 | sema  | `reactive.derived` lambda returns nothing (D-REACT1) |
| E2913 | sema  | a reactive `Signal`/`Derived` can't hold a function value (D-REACT1) |
| E2914 | sema  | `#Reactive fn` must not return a value (D-REACTCORE1) |
| E2930 | sema  | an interactive `UiAriaRole` node has an empty accessible label, lint-only (D-A11YGATE1) |
| E2931 | sema  | two interactive nodes in an inline focus group share an accessible label, lint-only (D-A11YGATE1) |
| E2932 | sema  | a `layout { … }` constraint mixes a horizontal and vertical value (D-LAYOUT1 / D-LAYOUT-GATES1) |
| E2933 | sema  | a line inside `layout { … }` doesn't produce a `Constraint` (D-LAYOUT1) |
| E2934 | sema  | a `layout { … }` constraint exactly duplicates an earlier one in the same block, lint-only (D-LAYOUT1) |

## Editions and release policy (E2-M2)

These enforce the compatibility contract in docs/spec/release-policy.md. An
**edition** opts a project into a specific era of Jet syntax (D-REL3); the
toolchain advertises the editions it supports in `jet --version`. **E2001** is
fully reachable from a real `pkg.jet`. **E2002** and **L2001** read from the
deprecation registry in `Source/Manifest.rs` (`DEPRECATIONS`); that registry is
empty pre-1.0 by design — Jet has deprecated nothing post-1.0 yet — so these two
codes are registered and snapshotted but not yet user-triggerable. They become
reachable the moment the first real deprecation is added, with no change to the
diagnostic plumbing (the C-FFI E3202 precedent: registered + honest about reach).

| Code | What | Why | Fix |
|------|------|-----|-----|
| E2001 | This package needs a newer Jet. | Editions opt a project into a specific era of Jet syntax. A newer edition can use syntax this compiler does not understand. | Upgrade with `jet upgrade`, or set `edition: "2026"` in `pkg.jet`. |
| E2002 | A deprecated item was used past its migration window. | The item was deprecated in an earlier edition and no longer exists in this one; it has reached the end of its migration window. | Use the named replacement, or run `jet fix` to migrate automatically. |
| L2001 | An item is deprecated in this edition. | It still works during its migration window but will be removed in a later edition. | Use the named replacement, or run `jet fix` to migrate automatically. |

## CLI arg-parsing diagnostics (D-ARGS1, `core.args`)

`core.args` provides a declarative builder: `args.spec().flag(…).option(…).positional(…)`
parsed against `io.args()` into a typed `ParsedArgs`. These errors fire when builder
or query methods are called with the wrong number of arguments.

| Code | What | Why | Fix |
|------|------|-----|-----|
| E1301 | `` `flag` expects 2 arguments (name, help), got N `` | `ArgsSpec.flag(name, help)` registers a boolean flag like `--verbose`; both the flag name and a one-line help string are required. | Pass exactly two strings: the flag name and a help description, e.g. `.flag("verbose", "enable verbose output")`. |
| E1302 | `` `option` expects 3 arguments (name, help, metavar), got N `` | `ArgsSpec.option(name, help, metavar)` registers a value option like `--output FILE`; all three strings are required. | Pass three strings: the option name, a help description, and a placeholder like `FILE`, e.g. `.option("output", "write to FILE", "FILE")`. |
| E1303 | `` `positional` expects 2 arguments (name, help), got N `` | `ArgsSpec.positional(name, help)` registers a required positional argument; both name and help are required. | Pass exactly two strings: the positional name and a help description, e.g. `.positional("input", "file to process")`. |
| E1304 | `` `parse` expects 1 argument (argv), got N `` | `ArgsSpec.parse(argv)` parses a `[String]` against the spec; pass exactly the argv list. | Pass exactly one argument: the argv list, e.g. `spec.parse(io.args())`. |

### Typed entry-signature CLI parsing (D-CLIFLAG1)

`@[Cli]` is a derive (sibling of `@[Codable]`) that turns a struct's fields into
`core.args` flag registrations; `fn run(args: T)` / `fn run(cmd: Enum)` is the
typed form of Jet's only entry point. It parses `io.args()` against
the derived spec before calling the user's function. See docs/spec/spec.md
"Typed entry-signature CLI parsing" for the full field-mapping rule. These
errors are all compile-time shape checks; a bad flag value at runtime reuses
the `core.args` runtime-error voice above (no new code for that).

| Code | What | Why | Fix |
|------|------|-----|-----|
| E1305 | `` field `name` has no CLI flag mapping (Type) `` | Only `Int`, `Float`, `Bool`, `String`, `Path`, and `T?` of those map to a flag; a nested `@[Cli]` struct, a `Map`, a closure, or a plain `[T]` don't. | Change the field to a supported type, or drop it from the `@[Cli]` struct. |
| E1306 | two `@[Cli]` fields both derive the same flag | Every field needs a distinct `--flag`; `--help` is also reserved (every generated CLI gets one automatically). | Rename one of the fields. |
| E1307 | a subcommand variant's payload isn't a `@[Cli]` struct | Each `enum Cmd { Variant(Payload) }` variant used as a `fn run` parameter needs a single `@[Cli]`-derived struct payload — that's where the subcommand's own flags come from. | Give the variant a single `@[Cli]` struct payload. |
| E1308 | `` `run`'s parameter isn't a CLI-derived type `` | A typed `fn run(args: T)` entry only works when `T` is `@[Cli]`-derived, or an `enum` whose every variant carries a `@[Cli]` struct payload. | Mark the struct `@[Cli]`, or give the enum's variants `@[Cli]` struct payloads. |

## Dev-loop diagnostics (E2-M4, `jet dev`)

`jet dev` runs your program in a built-in tree-walking interpreter (the M9.5
comptime evaluator, extended to whole programs) so a save gives feedback in
well under 200ms (D-DEV3). The interpreter is a dev convenience only — `jet
build`/`jet run` never use it, and it never produces a release artifact
(I2/I3). When it can't run a program, it says so plainly and names the real
build path; it never silently falls back to a different answer.

| Code | What | Why | Fix |
|------|------|-----|-----|
| E2201 | `jet dev` can't interpret this program yet — it uses a feature the dev interpreter doesn't cover (a task/channel, `extern rust`/C FFI, an `#Unsafe`/`core.mem` region, or a native-only core module like files/clock/random/environment/process). | The dev interpreter runs a deterministic, pure-enough subset for instant feedback; features that touch threads, foreign code, raw memory, or the outside world need the real native build. | Run `jet build` then the binary, or `jet run <file>` to compile and run it; `jet dev` keeps showing checks live. Opt in with `jet dev <file> --try-anyway` to attempt execution past the boundary, with no guarantees (D-DEV1). |
| E2202 | A program ran too long for `jet dev` to keep interpreting (the step budget was exhausted). | `jet dev` interprets your program; a run that never finishes is almost always a loop whose condition never becomes false. | Check the loop near the pointed-at line for a condition that never ends; `jet run` executes the real build with no step limit. |
| E2203 | `jet debug` can't step through this program yet — it uses a feature the debugger's interpreter doesn't cover (a task/channel, `extern rust`/C FFI, an `#Unsafe`/`core.mem` region, or a native-only core module like files/clock/random/environment/process). | `jet debug` steps your program in the same interpreter `jet dev` uses; features that touch threads, foreign code, raw memory, or the outside world can't be stepped at the Jet source level yet. | Run `jet build` then the binary, or `jet run <file>` to compile and run it; for a step-through, remove the unsupported feature or wait for the native-debugger milestone (D-DBG3 step 2). |
| E2204 | The `jet debug` session ended before the program finished — you typed `quit` at the `(jet)` prompt. | Quitting the debugger stops the interpreted run; the program did not run to completion. | Run `jet debug <file>` again and use `continue` (or `c`) to run to the end, or `jet run <file>` to run it without the debugger. |
| E2210 | This edit changed a type, so `jet dev` is restarting instead of swapping (the message names what changed — a struct field, an enum variant, or a function signature). | A hot swap re-applies code while the program's types stay the same; changing the shape of your data means the running code is rebuilt cleanly from the new types. | Nothing to fix — `jet dev` restarted with the new types; this note just explains why the swap became a restart. Type-stable edits (function bodies, statements) swap without a restart. |

## Range arm porting diagnostics (C25/D-RANGE2)

Jet's `..` is inclusive (S22). Users porting from Rust or Odin may write `..=`
(Rust's explicit inclusive range) or `step` (a loop modifier). These two
codes teach the Jet spelling and stop before misleading the user with a generic
parse error.

| Code | What | Why | Fix |
|------|------|-----|-----|
| E0318 | `` `..=` `` is not a Jet operator — Jet's `..` is already inclusive. | In Rust, `..` is exclusive and `..=` is inclusive; in Jet, `..` always includes both ends, so there is no `..=`. | Write `lo..hi` — it already means "lo through hi inclusive." |
| E0319 | `` `step` `` is not allowed in a range arm — range arms test a band, not a sequence. | `step` modifies a loop range to skip values (`loop i in 0..10 step 2`); an arm head like `1..10` just checks whether the subject is between 1 and 10. A stepped range is not a contiguous band and can't be used for membership testing. | Remove `step …`; to match only multiples of N, use a full condition: `subject >= lo && subject <= hi && subject % n == 0 ->`. |

## Fan-out and fixed-size list diagnostics

| Code | What | Why | Fix |
|------|------|-----|-----|
| E0961 | The callee of a fan-out `.[` is not a one-argument function. | `f.[a, b, c]` expands to `[f(a), f(b), f(c)]` — `f` must accept exactly one argument so each item can be passed to it. | Use a one-argument function or lambda as the fan-out callee. |
| E0962 | A fan-out item has the wrong type for the callee's parameter. | Each item in `f.[a, b, c]` is passed to `f`; they must match `f`'s parameter type. | Change the item to match the parameter type, or adjust the function. |
| E0963 | A positional destructure pattern has a different count than the fixed-size list's known length. | `[T#N]` has exactly N elements at compile time; the pattern must name exactly N bindings or the binding would leave elements unnamed. | Match the number of names in the pattern to the size N shown in the error. |
| E0964 | A length-changing method (`push`, `pop`, `insert`, `remove`, `clear`) was called on a fixed-size `[T#N]`. | The length of `[T#N]` is fixed at compile time and cannot change at runtime. | If you need a growable list, bind it with `:=` (e.g. `r := [...]`) so its length can change. |
| E0965 | An index is out of range for a `[T#N]` at compile time. | Literal indexes and `#Invariant`-refined distinct indexes must fit 0 through N−1; anything outside that range would panic at runtime. | Use an index in the valid range, widen to `[T]` for runtime checking, or tighten the refinement invariant. |

## Variadic and spread diagnostics (D-VARIADIC1)

| Code | What | Why | Fix |
|------|------|-----|-----|
| E1310 | A `...` rest parameter is not last, or a variadic parameter has a default value. | A variadic parameter collects every trailing argument, so nothing may follow it; a default would contradict that job. | Move `name: ...T` to the end of the parameter list and remove any `= …` default. |
| E1311 | A spread operand is not a list. | List spread `[...xs]` and call spread `f(...xs)` expand a list's elements — the operand must be `[T]`. | Spread a list value, or build the list without spread. |
| E1312 | A call uses spread at a function with no variadic rest parameter. | `f(...xs)` only applies when the callee's final parameter is variadic (`name: ...T`). | Pass arguments individually, or call a function whose last parameter is variadic. |
| E1313 | `` `{arg}` doesn't implement `{Trait}` `` — a trait-bounded variadic call-site argument fails one of the bound trait(s) (D-ANY-JAI1). | `{param}: ...{Trait}` (or `...[A, B]`) checks every argument against the bound trait(s) — that's how a function like this accepts a mix of types safely, with each argument monomorphized to its own concrete type (zero boxing). | Implement `{Trait}` for the argument's type, or drop the value from this call. |
| E1314 | `` `{name}` can only be used in a `loop … in {name}` loop here `` — a trait-bounded variadic parameter referenced outside its one supported shape (D-ANY-JAI1). | A trait-bounded variadic's elements can have different concrete types, so there's no single Rust type to give the whole parameter (`.len()`, indexing, passing it on, a second loop, …) outside a loop that visits each argument once. | Iterate it with `loop x in {name} { … }` — that's the only supported use in v1. |

## Module evaluation diagnostics (jetpack)

These come from the jetpack module evaluator (`Source/Jetpack/ModuleEval.rs`,
computed-modules arc), which gives `module name { … }` contributions meaning
by reducing them via pure-eval (M9.5) and feeding them through the §6 merge
table. Not (yet) reachable through `jet build`/`jet run` — `Item::Module` is a
deliberate parse-time no-op there until env.jet/config.jet are wired into the
CLI.

| Code | What | Why | Fix |
|------|------|-----|-----|
| E0966 | A module contribution's value isn't a struct literal of its namespace's type. | `env.dev: Env { … }` ties a namespace to its matching type so the merge engine knows what it's combining. | Wrap the value in the matching type, e.g. `Env { … }`. |
| E0967 | Two modules contributed irreconcilable values to the same source name or scalar setting. | §6: sources merge by name (refs must agree) and scalar settings merge to one value; without a priority marker, differing contributions can't be reconciled automatically. | Make every contribution agree, or remove the conflicting one. |
| E0968 | A `sources:` entry's value isn't a `provider@target` ref. | A named source resolves to an upstream written as `provider@target` (U6), e.g. `github@owner/repo/rev`; the resolver needs the provider and target to realize it. | Write the ref as `provider@target`, e.g. `default: github@owner/repo/rev`. |
| E0969 | An `imports:` directive isn't `find("<dir>")` with a single literal path. | Imports auto-discover a directory of modules (U4); the only directive is `find` with one string-literal path, so a non-`find` call or an interpolated/missing argument can't be walked. | Write `imports: find("./modules")`. |
| E0970 | `imports: find("<dir>")` points at a directory that doesn't exist. | `find` walks that directory for `.jet` modules (U4); it must exist relative to the file that declares it, or there is nothing to discover. | Create the directory, or fix the path so it points at your modules folder. |
| E0971 | A module discovered by `find(…)` has its own `imports:`. | The liftability law (U4): modules contribute to the merged whole, they never import each other — nesting `find` would make composition explode and break "drop a file in." | Remove the `imports:` from the discovered module; declare all `find(…)` directives in the top-level env.jet. |
| E0972 | A `System` / `Image` / `Service` record has a field it doesn't define. | Each of these records has a fixed set of fields (U11/U14); an unknown field is usually a typo or a value that belongs elsewhere. | Remove the field, or use one of the known fields named in the error. |
| E0973 | A `target` (or cross-compile platform) names a platform Jet doesn't know. | U13: a `target` is a typed platform value, not quoted text — it must be `linux.x64` or `linux.arm64`, so it type-checks and LSP-completes. | Write `target: linux.x64` or `target: linux.arm64`. |
| E0974 | A `System` has no `target`. | U11: every machine names the platform it runs on with a typed `target`. | Add `target: linux.x64` (or `linux.arm64`). |
| E0975 | A `Service` has no `enable`, or its `enable` isn't a yes/no value. | U12: every `Service` is an open record whose required first field is `enable: Bool`. | Add `enable: true` (or `false`) to the service. |
| E0976 | An `Image` uses a frozen disk-image format. | D-JETOS-FREEZE1: `iso`, `qcow`, and `raw` disk images are jetos research capture; Jetpack only builds `.Oci` images today. | Use `kind: .Oci` for an active image, or keep disk-image notes in the jetos research appendix. |
| E0977 | An `Image` has no active `from`, or restates a frozen system-inherited field. | Active Jetpack images are OCI containers built `from: packages.<name>`; `system.*` disk images are frozen jetos research. | Add `from: packages.<name>` for an `.Oci` image, or remove fields inherited from frozen `system.*` research. |
| E0978 | An `Image` `from:` references a frozen/unknown system. | D-JETOS-FREEZE1: `from: system.<name>` is frozen jetos disk-image research; Jetpack's active image path uses `from: packages.<name>`. | Use `from: packages.<name>` for an `.Oci` image, or keep the system image as research capture. |
| E0979 | `jet os` was given no host to apply. | D-JPK-OSHOST1=C: a bare host selects `system.<host>` in `./config.jet`; `path@host` selects an exact external root. | Write `jet os switch laptop` or `jet os switch ../machines@laptop`. |
| E0980 | A `jet os` host names a system the config doesn't define. | D-JPK-OSHOST1=C: the host selects one `system.<host>` contribution in `config.jet`. | Define `system.<host>: { … }`, or select one of the systems the config already defines. |
| E0981 | The `jet os` config file doesn't exist. | D-JPK-OSHOST1=C: a bare host loads `./config.jet`; `path@host` loads `path/config.jet` when `path` is a directory, or the file named by `path`. | Create it with `jet os init <host>`, or pass an external root as `path@host`. |
| E0982 | `use <pkg>` named a package that is realized as an `executable`. | U17: one import concept (`use`) covers files, modules, and `library` packages; an `executable` package installs a binary on your PATH — you run it, you don't import its code. | Remove the `use`, and run the executable's binary instead; or, if you meant to import its code, change the package to `library` in `pkg.jet`. |
| E0983 | `use <pkg>` named a `library` dependency the project declares but that hasn't been realized (its source isn't staged in the shared hangar store, and isn't on disk as a path dep). | U17: a `library` is consumed with the ordinary `use` form only after it is realized — `jet build`/`run` never realize on demand, keeping them offline and deterministic (the same flow as pre-fetched deps). | Run `jetpack build` to realize the library into the hangar, then `use <pkg>;` resolves it. |

## Concurrency diagnostics

| Code | What | Why | Fix |
|------|------|-----|-----|
| E1101 | A spawned task captures a value it does not own. | Tasks run concurrently and may outlive the scope that created them; shared `var` state is not allowed. | Give the task its own copy or use `take(name)` so the task owns the value; use a channel to send results back. |
| E1102 | A value crossing `tasks.spawn` or `Sender.send` is not sendable. | Task and channel boundaries move owned data between threads; a `View<T>`/string view, a trait value, or a non-`take`n closure cannot cross. | Send plain owned data (`copy` it, or use a `Shared<T>` handle for genuinely shared state), or hand a closure over with `take`. |
| E1103 | `.detach()` called on a task that had a sendability error (E1102) at spawn. | A detached task runs unsupervised and may outlive the caller; a task that already has sendability problems is doubly unsafe to detach. | Fix the E1102 error at the spawn site first; once the task only holds owned data, `.detach()` is safe. |
| E1106 | `.detach()` called on a task that captured a `view` borrow. | A detached task runs unsupervised and may outlive the borrow's source; the captured `view` would dangle. | Pass an owned `copy`, or a `Shared<T>` handle, to the task instead of a `view`. |
| E1104 | `#Layout(c)` struct contains a field whose type is growable (`[T]`, `Map`, or `String`). | Growable Rust heap types don't have a stable C layout — the raw data pointer and length live at unpredictable offsets. | Use a fixed-size array `[T#N]` instead, or remove `#Layout(c)` if C interop is not required. |
| E1105 | `#Layout(packed)` or `#Layout(align(N))` written on a struct. | The supported variants are `c` (C-compatible) and `columnar` (struct-of-arrays); `packed`/`align` are reserved for future milestones. | Use `#Layout(c)` or `#Layout(columnar)`, or omit `#Layout` for the default. |
| E1107 | The per-container layout prefix `columnar [T]` was written in a type. | A per-use columnar override isn't built yet — only the whole-struct form `#Layout(columnar) struct …` ships in v1 (D-SOA2C reserves this spelling). | Put `#Layout(columnar)` on the `struct` declaration instead. |
| E1108 | A list method (e.g. `.map`, `.filter`, `.sort`, `.pop`, `.remove`, `.get`) was called on a `#Layout(columnar)` list. | v1 columnar lists support the core surface — indexing, field access, `len`, `is_empty`, `push`, and iteration; the rest is deferred rather than silently miscompiled. | Drop `#Layout(columnar)` from the struct to use the full list API, or rewrite the operation with indexing and a loop. |
| E1109 | A partial columnar annotation `#Layout(columnar: f, g)` was written. | v1 supports whole-struct columnar only — every field becomes a column; per-field columnar needs new ownership/aliasing surface (D-SOA2B, deferred). | Write `#Layout(columnar)` to convert the whole struct. |
| E1110 | `.task { … }` was called outside a `taskgroup` block, or on a handle other than the active taskgroup name. | Structured spawning is scoped: only the handle bound by `taskgroup g { … }` may spawn children, and the taskgroup joins them at scope exit. | Wrap the code in `taskgroup g { … }` and write `g.task { … }` using that same name. |
| L1101 | A `Task` is dropped without `.join()` or `.detach()`. | The program may end before that task finishes. | Call `.join()` to wait for the result, or `.detach()` if fire-and-forget is intentional. |
| E0040 | `async` or `await` was written. | Jet uses blocking tasks and channels rather than async syntax. | Use `core.tasks as tasks` and call `tasks.spawn(() => work())`. |
| E0041 | `Mutex`, `RwLock`, `mutex`, or `lock` was written. | Jet avoids shared mutable state; tasks communicate by sending messages. | Import `core.tasks as tasks`, create a channel, and use `sender.send`/`channel.receive`. |

## Tier-2 reference diagnostics (E2-M5, D-DYNARRAY1 `View<T>`, D-MEM1 S5 string views)

D-MEM1/S3 deleted `-> &T` borrow returns and stored-reference (`&T`) fields
outright — there is no first-class borrow to store or return in v1. What's
left of this tier is the still-live `View<T>` zero-copy window
(`list.view(a..b)`, D-DYNARRAY1) plus, since D-MEM1 stage S5, the same
zero-copy treatment for `String` slicing (`s.trim()`/`s.after(sep)`/
`s.before(sep)` bound to a local): both never mention lifetimes, speaking in
Jet words instead — *what owns this* and *how long can this view live*. A
string view carries no distinct Jet-level type the way `View<T>` does
(`String` stays one type end to end, D-MEM1 gallery) — the check instead
tracks which *bindings* are views, the same scope-liveness proof applied to
a different owner kind. E2303 is the reference-specific name for the
task/channel rule — that situation is **reported once, as E1102** (an
unsendable value), for both `View<T>` and a captured string view; E2303
exists so `jet explain E2303` points there and the soundness matrix has a
named cell.

| Code | What | Why | Fix |
|------|------|-----|-----|
| E2303 | A `View<T>` (or a string view) crosses a `tasks.spawn` or `Sender.send` boundary. | A view points into something another scope owns; a task or channel moves owned data between threads, so a view can't cross without ownership. Reported as **E1102** (the unsendable-value rule), not separately, so one situation gives one error. | Send plain owned data, or rebuild the value as an owned copy (`copy x`) before crossing. |
| E2305 | A `View<T>` (`list.view(a..b)`) escapes the scope of the list it borrows from — returned from a function that owns the list, rebound to another local, or stored in a struct field. | `.view(a..b)` is a zero-copy window into the list's own backing storage, not a copy; if the list is made and freed inside this function (or scope), a window into it would outlive what owns it — there'd be nothing left to look at. | Return/store an owned copy instead (`list[a..b]` for a copying slice, or `.map(...)` the window into an owned list), or accept the list as a parameter so the caller keeps owning it. |
| E2307 | A string view (`s.trim()`/`s.after(sep)`/`s.before(sep)` bound to a local) escapes the scope of the `String` it borrows from — returned, rebound to another local, or stored in a struct field. | These calls return a zero-copy `&str` window into `s`'s own buffer when sema can prove it stays inside `s`'s scope (D-MEM1 S5); if `s` is made and freed inside this function (or scope), the window would outlive what owns it. | Keep the view inside the owner's scope, or materialize an owned `String` with `copy` before it leaves. |

## Library authoring diagnostics (E2-M6)

S61 (argument labels/defaults), S62 (trait delegation), D-LIB3 (Fallible `?`
conversion), and S77 (field punning) introduce these codes. E24xx is the
block reserved for M6.

| Code | What | Why | Fix |
|------|------|-----|-----|
| E2401 | The delegation target `{field}` doesn't implement `{trait}`, or the type has no field named `{field}`. | `impl Type.Trait using field` forwards every `Trait` method to the `field` field; if that field's type hasn't implemented `Trait`, there's nothing to forward to. | Implement `impl FieldType.Trait` on the field's type, or choose a different field that does implement `Trait`. If the field doesn't exist, add `{field}: FieldType` to the struct. |
| E2402 | `?` can't convert `{err}` into `Error` — `{err}` has no `Fallible` implementation. | `?` inside a `T ? Error` function can propagate errors whose type implements `Fallible`; the `to_error` method converts them. Without an impl, there's no path from `{err}` to `Error`. | Add `impl {err}: Fallible { fn to_error(self) -> Error { Error(str(self)) } }` (or a more descriptive conversion), or change the return type to `T ? {err}`. |
| E2403 | Field-pun name `{name}` is not in scope (or is not a field of `{type}`). | `Type { name }` is shorthand for `Type { name: name }` — it reads the local variable `name` and assigns it to the field of the same name. If no such local exists, or if `Type` has no field by that name, the shorthand is ambiguous. | Introduce a local `name :: …;` before the struct literal, or write the long form `Type { field_name: value }`. |
| E2404 | `` `?` can't turn a `{Source}` into a `{Target}` here ``. | `?` changes an error's type only when you've declared how via `impl Source -> Target { … }` (D-ERR-CONV); no such declaration exists for this pair. | Add `impl {Source} -> {Target} { … }` before the function that uses `?`. |
| E2405 | `impl {Source} -> {Target}` is already declared. | There can be at most one declared way to convert a `Source` error into a `Target`; the second block is rejected. | Remove one of the two `impl … -> …` blocks. |
| E2406 | Can't declare `impl {Source} -> {Target}` — neither type is defined in this program. | Error conversions obey the same orphan rule as trait impls (S28): at least one of `Source` or `Target` must be a type you defined, so conversions between two foreign types can't be added silently. | Define one of these types locally, or use `Fallible` (D-ERR2) if you don't own either type. |
| E2407 | `#[Rename(...)]` needs a string literal. | The wire key a `@[Codable]` field maps to is a constant string (D-SERDE5); a number or expression has no place on the wire. | Pass one quoted string — `#[Rename("wire_name")]`. |
| E2408 | `#[Flatten]` on `{field}` needs a struct-typed field. | Flatten splices another struct's keys into this object (D-SERDE5), so the field must itself be a `@[Codable]` struct — not a primitive, list, or map. | Give `{field}` a `@[Codable]` struct type, or drop `#[Flatten]`. |
| E2409 | `#[RenameAll({style})]` isn't a known casing style. | The wire-casing menu is the closed typed set `camel`/`snake`/`pascal`/`kebab`/`screaming` (D-SERDE3); anything else is rejected so a typo fails at compile time, not on the wire. | Pick one of `camel` / `snake` / `pascal` / `kebab` / `screaming`. |
| E2410 | `E2410: missing required field `{field}`` (runtime `DecodeError`). | Decoding into a `@[Codable]`/`@[Decode]` type found no wire value for a required field and the field has no `#[Default]` and isn't optional (D-SERDE5). | Mark the field optional (`T?`), give it `#[Default]`/`#[Default(value)]`, or fix the input so the key is present. Compose with `??` to supply a fallback. |
| E2411 | `{Type}` can't be serialized / decoded. | Only types that opt in with `@[Codable]`/`@[Encode]`/`@[Decode]` (and whose fields all have a wire form) can cross the wire; a field holding a non-Codable type, closure, or handle has nothing to (de)serialize (D-SERDE1). | Add `@[Codable]`/`@[Encode]`/`@[Decode]` to the offending type, or remove it from the encoded value (e.g. `#[Skip]`). |
| E2412 | `E2412: unknown field `{field}`` (runtime `DecodeError`). | The struct is marked `#[DenyUnknownFields]` (D-SERDE8) and the input carried a key the struct doesn't declare, so decoding fails fast instead of silently dropping it. | Remove `#[DenyUnknownFields]` to ignore extra keys (the lenient default), add the field, or fix the producer. |
| E2413 | retired (D-SERDE12) — generic `@[Codable]` is first-class; the derive auto-injects `Encode`/`Decode` bounds on the wire-reaching type params (D-SERDE9/D-SERDE10). A non-codable type argument fails at the use site (E0905), not the definition. | — | — |
| L2401 | Public function `{fn}` has a positional `Bool` parameter `{param}`. | Positional booleans are easy to transpose: `connect(host, true, false)` is a guessing game. Labels (S61) make the intent clear at the call site. | Callers can use `{param}: true` to document intent; or give the parameter a default value so it can be omitted. No action required — this is advisory. |
| L0520 | `` `{type}` has no `Display` impl — bare `{}` will require one soon ``. | Bare `{value}` interpolation is moving to the explicit `Display` hook (D-DISPLAY-SHAPE); auto-printable structs still compile via a temporary `jet_show` fallback. | Add `impl {type}.Display { fn display(self) -> String { … } }`, or use `{value@Debug}` for debug output. |

## Streaming I/O diagnostics (E2-M7, D-IO1..3)

RAII file handles (`files.open`, `files.create`, `files.append`) close on every
exit path including `?` early returns. E25xx covers misuse of those handles.
L2501 is reserved for a "whole-file read advisory" but is not emitted yet (the
test harness can't normalise paths in exact-match comparisons; revisit when that
is fixed).

| Code | What | Why | Fix |
|------|------|-----|-----|
| E2501 | `{method}` is not available on a {direction} file handle. | `files.open` returns a read-only handle; `files.create`/`files.append` return a write-only handle. Calling a write method on a reader (or a read method on a writer) is a type error. | Use the correct handle type for the operation: `files.open` to read, `files.create`/`files.append` to write. |
| E2502 | A line stream can only be used directly in a loop. | `.lines()` hands back a lazy line reader meant to be iterated in place; storing it in a name would let it leave the loop, where it has no use. (The boundary is enforced in sema so codegen never has to lower a stray line stream — c109/I3.) | Iterate it directly: `loop line in handle.lines() { … }`. |
| L2501 | (reserved) `fs.read` loads the whole file into memory at once. | For large files this can exhaust memory; streaming reads use bounded space. | Use `files.open(path)?` and `loop line in handle.lines() { … }` to stream line-by-line. Not emitted yet. |
| E2510 | `#{op}` isn't a reduce operation (or a reduce marker used outside `.reduce`). | A SIMD lane reduction (D-SIMD2) folds the lanes with one of a fixed set of operations, named by a marker so the fold is explicit; only `#Add`/`#Mul`/`#Min`/`#Max` are reduce operations, and the marker is meaningful only as the sole argument to `.reduce(…)`. | Use `v.reduce(#Add)` / `#Mul` / `#Min` / `#Max`, or the named reductions `v.sum()` / `v.product()` / `v.min()` / `v.max()`. |
| E2511 | operator `{op}` isn't defined between `{lhs}` and `{rhs}`. | Operator overloading is blessed on the closed built-in math family ONLY (D-SIMD2/D-LINALG1): element-wise `+`/`-` (and `/` for lanes), `*` (element-wise, or matrix×vector), and `==`/`!=` — both sides must be the same lane/vector type (or a matrix and its matching vector). | Match the operand types, or use a named method like `.dot()`/`.cross()`/`.matmul()`. |
| E3110 | lane `{lane}` isn't valid on `{type}`. | Swizzle members name lanes with `x`/`y`/`z`/`w`; each type exposes only its lane count (`Vec2`: x/y, `Vec3`: x/y/z, …). | Use only the lanes defined for `{type}`. |
| E3111 | write swizzle `{pattern}` repeats a lane on `{type}`. | Each lane may be written at most once — overlapping patterns like `v.xx` have no single meaning (D-SWIZZLE1). | Assign each lane once, e.g. `v.xy = …` instead of `v.xx = …`. |

## Package supply-chain diagnostics (E2-M8, D-PKGS1–4)

Enforced SemVer, resolver conflicts, audit advisories, and integrity
verification live here. E26xx is the block for M8. These fire from the
`jet publish`, `jet fetch`, and `jet audit` commands, never from compiling
source files. Each diagnostic names the affected package and version so the
output is machine-parseable with `--json`.

| Code | What | Why | Fix |
|------|------|-----|-----|
| E2601 | This release is tagged `{version}` but removes (or changes incompatibly) the public API item `{item}`. | `{version}` is a {bump_kind} bump, which promises no breaking changes under SemVer. Callers pinned to `^{major}.0` would stop compiling. | Bump to `{next_major}.0.0`, or restore `{item}` (a deprecated forwarding shim counts). Use `--force` to publish anyway with an explicit warning banner. |
| E2602 | Dependency resolver conflict: `{package}` requires `{req_a}` from `{from_a}` but `{req_b}` from `{from_b}`, and no version satisfies both. | Jet uses a PubGrub-style resolver that requires a single version per package. Two incompatible constraints cannot both be met. | Upgrade or downgrade one of the conflicting dependents so their `{package}` constraints overlap, or ask the authors to release a version that satisfies both. |
| E2603 | `[{severity}]` advisory `{advisory_id}` matches `{package}` `{version}`: {title}. | The advisory database flags this version as having a known vulnerability, exposed interface, or supply-chain risk. `jet audit` exits nonzero only on a `critical` match; lower severities inform and exit 0. | Upgrade to `>= {fixed_version}` (or the version listed in the advisory). Run `jet audit --explain {advisory_id}` for details. |
| E2604 | Integrity check failed for `{package}` `{version}` — expected `{expected}`, got `{actual}`. | A fetched artifact's content hash differs from what the lockfile recorded. This means the artifact changed after it was locked — accidental or deliberate tampering. | Re-run `jet fetch` after cleaning stale Jetpack hangar entries (`jet clean`). If the problem persists, the upstream source may have been altered; audit the change before proceeding. |
| E2605 | `{name}` v{version} cannot be published from a dirty working tree. | The registry records the exact source revision that was published. A dirty tree means uncommitted changes would be silently excluded, making the published package unreproducible. | Commit or stash all uncommitted changes (`git status` to list them), then run `jet publish` again. Use `--force` to bypass with an explicit warning banner. |
| E2606 | `jet yank` requires a version argument. | A yank marks one specific published version as deprecated; without a version the command doesn't know which one to yank. | Run `jet yank <version>`, e.g. `jet yank 1.2.3`. |
| E1217 | `{dep}` is in `pkg.jet` but has no locked revision. | A `--locked` build (and `jet publish`) requires every dependency to be pinned in the lockfile to a resolved version, so the build is reproducible. The dep is declared but not pinned. | Run `jet fetch` to resolve and pin `{dep}`, then commit the lockfile. |
| E1218 | Publishing `{new}` after `{old}` is a {bump} bump but breaks the public API item `{item}`. | A {bump} bump promises callers no breaking changes under SemVer, but the public API changed since `{old}`. This is the local publish-time gate; the registry re-checks live with E2601 on receipt. | Bump to `{next_major}.0.0` (a major release), or restore `{item}` (a deprecated shim counts). Use `--force` to publish anyway with an explicit warning banner. |
| E1219 | `--profile={name}` is not a defined build profile. | Blessed profiles `release`, `debug`, and `ci` have built-in defaults. Any other name must be declared in your `pkg.jet` `build { }` block as `{name}: Build.{ optimize: … }`. | Use `--release` for the release profile, `--profile=debug` for debug, `--profile=ci` for CI, or add `{name}: Build.{ optimize: full }` (or `none`/`basic`) to the `build { }` block in `pkg.jet`. |
| E1220 | `{dep}` uses the `{effect}` effect, which this package's budget doesn't allow. | An `effects:` budget fails the build when any dependency reaches an effect you didn't list — supply-chain review as a compile error. | Add `{effect}` to `allow`, or grant it to `{dep}` in `grants:`, or drop the dependency. |
| E1221 | `pkg.jet` has a malformed `effects:`/`grants:` block. | `effects: { allow: […], deny: […] }` and `grants: { "dep": […] }` only take effect names from the ten-effect vocabulary (D-EFF4), as lists. | Fix the field name or effect name; see docs/spec/syntax-decisions.md. |

## First-party ring library diagnostics (E2-M9, D-LR1–4)

Wave-1 ring packages (`core.encoding.{csv,toml,yaml,json}`, `core.log`, `core.time`, `core.crypto`) are compiler-known modules — no external crates in `src/` (I6). E27xx is the block for M9.

| Code | What | Why | Fix |
|------|------|-----|-----|
| E2701 | `{parser}` found malformed input at row/line {n} — {detail}. | The ring library parse function encountered text it can't interpret: a missing delimiter, an unclosed quote, or an unexpected character. The row or line number points at the first offending record. | Fix the input at the location named, or validate it before parsing. |
| E2702 | Crypto API misuse: {detail}. | A `core.crypto` call would use a key, nonce, or algorithm in an unsafe way — for example, an IV too short or a key length the algorithm doesn't accept. Reserved for future static checks. | Follow the fix named in the error; the `core.crypto` API is intentionally restrictive to surface misuse. |
| E2710 | `` `derive T.{Trait}` body failed while expanding `#[{Trait}]` on `{Type}` ``. | The user-authored derive body ran at compile time (D-METADERIVE1=A, D-CTCODEGEN1=A) and threw a comptime error — typically an undefined name, a bad method call, or a type mismatch in the body. The span points at the `#[Trait]` marker on the struct that triggered expansion. | Fix the `derive T.{Trait}` body: check that every name it references is bound in scope, every method it calls is valid on the reflected type, and every `emit()` argument is a `String`. |
| E2711 | Derive orphan rule: `` `derive T.{Trait}` `` and `` `{Type}` `` are both in an imported module. | User-derive expansion can only run in the entry module (D-METADERIVE1=A). Both the `derive` block and the struct it targets must live in the entry file; expansion locked to an imported module produces an `impl` the entry module cannot override or suppress. | Move both `derive {Trait}` and `struct {Type}` to your entry module; derive expansion only runs there. |
| E2712 | *retired by D-CTBLOCKEXPOSE1* (was: `$` splice outside comptime context). | Runtime `$name` splices are allowed when a comptime value is in scope. | Define the value with `comptime name = ...`, or remove the `$` prefix. |
| E2713 | There is no comptime value named `{name}`. | `$name` splices a value that was computed by a `comptime` binding or `comptime {}` block. | Define `comptime {name} = ...` before using `$name`. |
| E2714 | A user derive is written `derive T.{Trait}`. | The type parameter comes first, joined to the trait name with a dot (D-METADERIVE1, amended 2026-07-01); the `derive {Trait} for T` spelling was retired. | Write `derive T.{Trait} { … }`. |
| L2701 | This regex pattern may catastrophically backtrack on certain inputs. | A regex with unbounded quantifiers nested inside another unbounded quantifier can run in exponential time on adversarial inputs, causing a denial-of-service. Reserved for future `core.regex` patterns. | Anchor the pattern at the start (`^`) or end (`$`), or restructure it to avoid nested quantifiers. |

## Networking and services diagnostics (E2-M10, D-NET1–3)

`core.net` provides blocking TCP/UDP sockets; `core.http` provides HTTP client and
server built on top. E28xx is the block for M10.

| Code | What | Why | Fix |
|------|------|-----|-----|
| E2801 | {operation} on `{address}` failed: {detail}. | A socket bind, listen, connect, or accept call returned an OS error. The address and operation are named so you can act on the specific failure. | Check that the address is reachable and the port is not already in use. For `bind`, try a different port. For `connect`, verify the server is running. |
| E2802 | TLS handshake with `{host}` failed: {detail}. | The TLS layer could not complete a secure handshake — the certificate may be invalid, expired, or untrusted by the system trust store. | Verify the server's certificate, ensure the system trust store is up-to-date, or use `core.tls.insecure_skip_verify()` in a test environment only. |
| E2803 | Request body exceeds the {limit}-byte limit. | The server's configured `max_body` cap was reached. Allowing unbounded bodies risks memory exhaustion. | Raise the `max_body` option passed to `http.serve`, or reject the request in your handler before reading the body. |
| E2804 | Two routes both match `{METHOD} {pattern}`. | A duplicate route makes dispatch ambiguous — one handler would never be reached. Registered at start-up so the bug surfaces immediately. | Remove one of the duplicate registrations, or make the patterns distinct (e.g. add a static prefix to one). |
| L2801 | Blocking call inside the accept loop without a worker task. | A slow handler inside `http.serve` or a raw `net.tcp_accept` loop blocks all new connections until it returns. | Wrap the handler body in `tasks.spawn(() => …)` so each connection runs in its own task. |

## Testing and tooling diagnostics (E2-M11)

Quality workflows: doctests, snapshot testing, `todo` typed holes, `jet bench`, and capability summaries. E29xx is the block for M11.

| Code | What | Why | Fix |
|------|------|-----|-----|
| E2901 | Doctest output mismatch. Expected: `{expected}` Got: `{actual}` | The example in the doc comment claims a different result from what the code produces. Docs cannot lie (D-TEST4/I5 generalized to user code). | Run `jet test --update-snapshots` to update the golden output, or fix the code to match the claimed output. |
| E2902 | `#Todo` at `{file}:{line}` — expected `{type}` | A `#Todo` typed hole was reached at runtime. The hole compiles anywhere and type-checks, but panics when executed (D-TOOL2). | Replace `#Todo` with a real implementation. |
| E2910 | `reactive.{kind}` needs a lambda, not {type}. | `reactive.derived`/`reactive.effect` build a reactive value from a `() => …` body so it can re-run when a signal changes (D-REACT1=B). A non-lambda argument has nothing to re-run. | Write `reactive.derived(() => … )` or `reactive.effect(() => { … })`. |
| E2911 | `reactive.{kind}` needs a zero-parameter lambda, got {n} parameter(s). | The body of a derived/effect takes no arguments — it reads the signals it depends on via `.get()` (D-REACT1=B). | Drop the parameters: `reactive.{kind}(() => { … })`. |
| E2912 | `reactive.derived` must compute and return a value. | A derived value is recomputed from its signals, so its lambda has to return the new value (D-REACT1=B). A body that returns nothing is a side effect, not a value. | Return a value from the body, or use `reactive.effect(() => { … })` for a side effect. |
| E2913 | a reactive {kind} can't hold a {type}. | Signals and derived values hold ordinary data so it can be copied to dependents (D-REACT1=B). A function value isn't reactive data — wrap behaviour in an effect instead. | Use a data value (number, text, list, struct, …); put behaviour in `reactive.effect`. |
| L2901 | This `#Test` block has no assertions. | A test with no `require`, `require_eq`, or `expect(…).snapshot()` call cannot find bugs — it always passes. | Add at least one assertion, or remove the test if it only exercises compilation. |

## Scope member diagnostics (D-DOTSCOPE1)

Inside a `#Marker { }` block a statement-position `.name { … }` / `.name(args) { … }` resolves against that marker's declared vocabulary. `#Test`'s members are `.setup`, `.expect_fail`, `.timeout`, `.skip`.

| Code | What | Why | Fix |
|------|------|-----|-----|
| E0614 | `.{name}` isn't a member of `#{marker}`. | Inside a marker block a `.name { … }` statement must name one of that marker's declared members. `#{marker}` understands: {list}. | Use one of the listed members, or remove the block. |
| E0615 | `.{name}` only works inside a marker block that declares it. | A leading-dot member statement resolves against the enclosing `#Marker`'s vocabulary; out here there is no such block. | Move it inside a `#Test("…") { … }` block, or write an ordinary statement. |
| E0616 | `.setup` must be the first statement in the test. | `.setup` marks the test's initialization; anything before it would run first. | Move `.setup { … }` to the top of the block. |
| E0617 | this scope member has the wrong arguments. | Each member has a fixed shape: `.timeout(500ms)` takes one duration, `.setup`/`.expect_fail` take none, `.skip` takes an optional reason string. | Match the member's shape, e.g. `.timeout(500ms) { … }` or `.skip("reason") { … }`. |
| E0618 | scope members can't be nested. | Each member is a top-level region of the marker block; nesting one inside another member or a control block has no meaning. | Move the member out to the top level of the block. |

## Accessibility diagnostics (D-A11YGATE1=B, c134 Phase 6)

`jet lint --a11y <file>` is the opt-in surface for accessibility issues.
E2930/E2931 are computed during ordinary sema (same as any other lint), but
they never appear in `jet build`/`jet run`/`jet check`/`jet emit` output —
only `jet lint --a11y` prints them, and it exits non-zero when it finds one so
a project can wire "zero a11y warnings" into CI without those warnings ever
blocking ordinary compilation (D-A11YGATE1 rejected making these compile
errors — over-strict for interactive iteration). Both are static and
literal-only: they check the `label`/`role` arguments at a `ui.node_role(…)`
call site directly, and (E2931) an inline `[…]` list literal passed to
`set_focus_group`. A label read from a variable or a computed expression is
not traced back to its source, by design — the lint never guesses about a
runtime value it can't see.

| Code | What | Why | Fix |
|------|------|-----|-----|
| E2930 | this {role} has no accessible label | Screen readers announce a control by its accessible label; an empty label is invisible to assistive tech. | Pass a real label, e.g. `ui.node_role("Submit", w, h, ui.aria_role_button())`. |
| E2931 | two interactive nodes both have the label "{label}" | Assistive tech announces controls by their label — identical labels make them indistinguishable (WCAG 2.5.3). | Give each interactive node a distinct, descriptive label. |

## Layout constraint diagnostics (D-LAYOUT1 / D-LAYOUT-GATES1)

`layout NAME { … }` (D-LAYOUT1=A) is a Cassowary-style linear constraint
block. GATE 1 lets `>=`/`<=`/`==` between the closed layout types
(`HVar`/`VVar`/`LengthVar`) produce a `Constraint` instead of `Bool`; GATE 2
puts those types (plus `Constraint`/`LayoutHandle`) in the compiler's closed
type family. E2932/E2933 are ordinary compile errors (shown in `jet build`/
`jet run`/`jet check` like any other); E2934 is an ordinary warning
(non-blocking). All three are static — computed from the constraint
expressions' shapes, not from solved values.

Infeasibility (two required constraints that can't both hold) is NOT one of
these — it generally depends on values the solver only has at runtime, so it
isn't a static diagnostic. `LayoutHandle.is_feasible()` / `.conflict()`
query it explicitly (`.conflict()` names the contradicting required
constraints, straight from the simplex tableau); `LayoutHandle.value(v)`
panics with the same conflict list if the layout is infeasible when a value
is read, rather than silently returning a wrong number (I1 — a loud failure
beats a quiet wrong one). Detecting infeasibility from a WHOLLY
comptime-known constraint set, at COMPILE time, is future work (the static-
layout-evaluation optimization D-LAYOUT1's plan describes) — not yet
implemented.

| Code | What | Why | Fix |
|------|------|-----|-----|
| E2932 | layout constraint mixes a horizontal and vertical value (`{lt}` and `{rt}`) | `left`/`right`/`width` are horizontal (`HVar`); `top`/`bottom`/`height` are vertical (`VVar`) — combining or comparing across axes is caught at compile time instead of producing a nonsensical layout. | Compare or combine values from the same axis (a `LengthVar`, or a plain number, fits either axis). |
| E2933 | this line inside `layout {name}` doesn't produce a constraint (found `{ty}`) | Every line directly inside a `layout { … }` block must be a `>=`/`<=`/`==` comparison of layout values (a `Constraint`). | Write a comparison, e.g. `label.width >= 80.0`, or capture it: `c :: label.width >= 80.0`. |
| E2934 | this constraint repeats one already written in this `layout` block | An exact duplicate constraint doesn't tighten the layout — it's almost always a copy-paste leftover. | Remove the duplicate line, or change it if a different constraint was meant. |

## Debugging and observability diagnostics (E2-M12, D-OBS1–3)

Runtime panic reports and structured log shape. E30xx is the M12 block.
E3001/E3002 are **runtime** reports, not compile-time diagnostics — the
span is embedded in the message (Jet file + line + function name).
`jet explain E3001` explains the report format and D-OBS2 safe-locals policy.

| Code | What | Why | Fix |
|------|------|-----|-----|
| E3001 | `panic: {msg}` — with Jet file, line, function name, source-line context box, and (debug builds only) safe local variable values. | The program hit a `panic`, `require`, or `require_eq` call that failed, or a bounds/key check triggered at runtime. Jet file and line are shown in Jet terms — never generated-Rust terms (I2). | Fix the logic that led to the failure; the source line and locals show what values were in play. |
| E3002 | `error propagated from: {fn} ({file}:{line}) via ?` — an error-return trace entry appended when a `?` re-raises an error. | Each `?` that propagates an error adds a frame, making the full error path visible. | Follow the trace from the innermost `Err` origin to the outermost `?` to find where the error was created and which callers forwarded it. |
| E3003 | `deadline exceeded while waiting in {wait_kind}`. | A wait/IO point observed an active `#Context(deadline: …)` budget and the remaining time reached zero before the operation completed. | Raise the deadline budget, shorten the work before the wait point, or remove/adjust the ambient deadline for this scope. |
| E3005 | `@{Pre\|Post} contract failed: {msg}` — with file:line. | A `@Pre` (argument claim, checked at entry) or `@Post` (`result` claim, checked before return) condition evaluated false at runtime. `{msg}` is the clause's own message string. Checked in every build (not a debug/release split). | Fix the caller (a failed `@Pre` means an argument violated the function's stated contract) or the function body (a failed `@Post` means it broke its own promise about the result). |

## Uninitialized binding diagnostics (D-UNINIT1, spelling moved by D-UNINIT-SENTINEL1)

`name: Type := uninit` opts out of automatic zero-fill for a single binding. It
is gated by `use core.mem` (E0424) and restricted to plain-data types (E0423).
The compiler proves, by forward dataflow, that every read follows a write on
all control-flow paths (E0420). Codegen lowers to
`MaybeUninit::uninit().assume_init()`.

This is the same engine D-UNINIT1 shipped (ratified 2026-06-21); D-UNINIT-SENTINEL1
(ratified 2026-07-02) only moved the surface syntax off the `#Uninit name: Type`
marker onto the `uninit` contextual keyword. The old marker is now a hard parse
error (E0426) pointing at the new spelling — see D-UNINIT-SENTINEL1's fixture,
`tests/ui/uninit_marker_retired.jet`.

| Code | What | Why | Fix |
|------|------|-----|-----|
| E0420 | `` `{name}` may be read before it is given a value ``. | `` `{name}` was declared `:= uninit`, so it holds no value until you write to it — this read could see garbage ``. | Write to `{name}` on every path before reading it (e.g. fill it via `mut {name}`). |
| E0421 | `` `uninit` needs a type annotation ``. | An uninitialized binding has no value to infer its type from, so the type must be written. | Write `` `{name}: <Type> := uninit` ``, e.g. `` `buffer: [4096]U8 := uninit` ``. |
| E0423 | `` `uninit` needs a plain-data type ``. | The named type may own heap memory or need cleanup, so leaving it uninitialized is unsafe. | Use plain data — a number, `Bool`, `Char`, `U8`, or a fixed array of those (e.g. `[4096]U8`). |
| E0424 | `` `uninit` needs the low-level memory tier ``. | `` `uninit` skips the automatic zero-fill — an expert-tier operation ``. | Add `use core.mem` at the top of this file to opt in. |
| E0426 | `` `#Uninit` is retired ``. | Uninitialized storage is a fact about the initializer, not the declaration — it now reads `` `name: Type := uninit` ``. | Write `` `{name}: <Type> := uninit` ``. |

## Low-level tier diagnostics (E2-M13, S58)

The expert tier is gated twice: `use core.mem` unlocks the vocabulary, and an
`#Unsafe("…") { … }` region (or an `#Unsafe fn` contract) opens the
operations that can violate memory safety. Ordinary Jet never reaches these.

| Code | What | Why | Fix |
|------|------|-----|-----|
| E3101 | `{op}` can only run inside an `#Unsafe` block. | This operation can violate memory safety, so it must sit in an audited region. | Wrap it: `#Unsafe("why this is safe") { … }`. |
| E3102 | `{item}` is part of the low-level tier. | Naming `Ptr`, `volatile_read`, or an allocator needs the discovery gate. | Add `use core.mem;` at the top of the file. |
| E3103 | `{fn}` is an `#Unsafe` function. | Its contract can't be checked by the compiler, so the caller must vouch for it. | Call it inside `#Unsafe("…") { … }`. |
| E3104 | `{arena}` was already {reset/freed}; this value lives in `{arena}` which is gone. | Calling `arena.reset()` or `arena.free()` invalidates all values allocated in it. | Move the `alloc` call before the `reset`/`free`, or create a new allocator. |
| L3101 | This `#Unsafe` block/function has no reason. | Every gated region/function records, in one line, why it can't break memory safety. | Add the reason: `#Unsafe("why this is safe") { … }` or `#Unsafe("why this is safe") fn ...`. |
| L3102 | This `#Impure` block has no reason. | Every comptime effect gate records, in one line, why ambient I/O is needed. | Add the reason: `#Impure("reading build config") { … }`. |

## Comptime effect tiers (D-CTEFFECT1)

Tier-0 (pure) calls are whitelisted Core builtins — always safe, no gate needed.
Tier-1 (`embed_file`/`embed_bytes`/`find`) hashes inputs into `.jet/lock`.
Tier-2 (ambient) requires both a `#Impure("reason") { … }` gate **and** `--allow-impure`.

| code | what | why | fix |
|------|------|-----|-----|
| E3410 | `{module}.{call}` is a Tier-2 ambient comptime effect and can't run at compile time without a `#Impure` gate. | `core.files`, `core.env`, `core.io`, and `core.exec` touch the host system at compile time. Without the gate any build tooling (caches, hermetic sandboxes) may get different results. | Wrap the call in `#Impure("reading config") { … }`. |
| E3411 | `#Impure` gate present but `--allow-impure` was not passed. | The `#Impure` block opts in to ambient I/O, but the build flag is also required so CI can audit builds that touch the host. | Add `--allow-impure` to your `jet build` / `jet run` invocation. |
| E3412 | `core.net.{method}()` is not available at comptime. | Only `core.net.fetch(url, sha256:)` is supported at compile time as a Tier-1 hermetic effect. Other `core.net` methods are not planned for comptime access. | Use `core.net.fetch(url, sha256: "…")` for content-hash-pinned downloads. |
| E3413 | fetch: sha256 mismatch for `{url}`. | The downloaded content does not hash to the expected `sha256:` value. The pin ensures every machine gets byte-identical content; a mismatch means the URL content changed or the pin is wrong. | Update the `sha256:` argument to match the actual content hash shown in the Why line, or verify the URL points to the correct file. |
| E3414 | fetch failed / bad argument / non-UTF-8 content (message varies). | Common causes: unsupported URL scheme (only `file://`, `http://`, `https://`), unreachable host, missing `sha256:` argument, or binary content (use `embed_bytes` for that). HTTPS TLS failures use E4201–E4203. | Check the URL and arguments; use `file://` for local test paths. |
| E4201 | TLS handshake with `{host}` failed. | The URL reached a server, but the connection did not complete a secure HTTPS handshake. The usual cause is pointing an `https://` URL at a plain HTTP server or a server with a broken TLS setup. | Verify the URL points at an HTTPS server, not plain HTTP. For local tests, start the TLS fixture server. |
| E4202 | TLS certificate for `{host}` could not be trusted. | The server presented a certificate Jet could not verify for that host. It may be expired, for another name, self-signed, or missing an intermediate. | Use a certificate whose subject matches the host and chains to a trusted root. For tests, trust the local fixture CA explicitly. |
| E4203 | HTTPS could not find system certificate roots. | D-TLS1 uses rustls with the system trust store for default HTTPS. Minimal images can omit that bundle, so there is no root set to verify public certificates against. | Install the system certificate bundle (for example `ca-certificates`) or run in an image that includes it. |

## Arena region diagnostics (D-ALLOC2 / D-REGION1)

`arena.alloc(value)` hands back a *view* into the arena's storage, not an owned
copy — real shared bump-allocation. A view is sound only inside its **region**
(the lexical scope of the `arena` binding, or an explicit `region r { … }`) and
only until the arena is `reset`/`free`d. Two compile-time checks keep it that way,
both at least as strict as Rust's borrow checker, so a use-after-free is a
*compile error*, never a runtime trap. Unlike E3104 (which catches `alloc` on an
already-freed arena), these track the views themselves.

| Code | What | Why | Fix |
|------|------|-----|-----|
| E0631 | `{view}` cannot be shared — it does not live long enough to {escape}. | `{view}` is a view into `{arena}`; sharing it outside the region would let it outlive `{arena}` and point into freed memory. | Keep the view inside the arena's region, or copy what you need out with `.clone()` before it leaves. |
| E0632 | `{arena}` was {reset/freed} here, so the value `{view}` points into is gone. | `reset`/`free` invalidate every value allocated in the arena; reading the view afterward would read freed memory. | Use the view before `reset`/`free`, or re-`alloc` after to get a fresh value. |

## Dynamic type diagnostics

| Code | What | Why | Fix |
|------|------|-----|-----|
| E0350 | Jet does not have an `Any` type. | A value should keep a precise shape: use an enum for known variants, generics or traits for abstraction, `T?` for absence, and `Data` for parsed dynamic data. | Replace `Any` with the specific mechanism for this value. |

## Effect system diagnostics (D-EFF1, D-QUAL1)

Every function carries an inferred effect set (the ambient powers its body
reaches — `Net`, `Fs`, `Io`, `Db`, `Time`, …). A `#(…)` list on the signature
declares an upper bound; `@Pure` declares the empty set. The inferred set must
be a subset of the declared one. Effects are erased in codegen (I3), so these
are compile-time-only diagnostics. An unknown effect name in a `#(…)` list is
reported as **E0119** (unknown name).

| Code | What | Why | Fix |
|------|------|-----|-----|
| E0740 | `{fn}` uses the effect `{effect}`, which its signature doesn't allow. | A `#(…)` list is an upper bound on what the body may do; the inferred effects must be a subset. An effect the body reaches that the bound omits breaks that contract. | Add the named effect to the `#(…)` list, or stop using it (drop the Core call that introduces it, or move it out of this function). |
| E0741 | This `#Caps` region uses the effect `{effect}`, which it doesn't allow. | `#Caps(…)` restricts a region to a fixed set of effects; anything reached inside — even transitively through a call — must be in that set, so the region is a hard local ceiling. | Add the named effect to the `#Caps(…)` list, or move that work outside the region. |
| E0742 | This `{method}` impl uses the effect `{effect}`, which the trait doesn't allow. | A trait method may declare an effect upper bound (`@Pure fn hash(self)`, `fn render(self) #(Gpu)`); every implementation's inferred effects must fit inside it, so the bound holds for all impls (D-EFF3). | Remove the offending work from the impl, or widen the bound on the trait method. |
| E0711 | The capability `{handle}` can't escape its `#Grant` block. | `#Grant(…)` grants a capability into a lexical scope and revokes it at scope end (RAII, S63); returning, storing, or capturing the handle would let a revoked authority outlive the block (D-SCAP1). | Use the handle only inside the `#Grant` block, or perform the work that needs it there. |
| E0712 | This `#Grant` region uses the effect `{effect}`, which it has no capability for. | `#Grant(…)` authorizes exactly the listed effects through its handle; the dual of `#Caps`, an effect reached inside — even transitively through a call — that the grant omits has no capability backing it (D-SCAP1). | Add the named effect to the `#Grant(…)` list, or move that work outside the grant. |
| E0721 | Untrusted (`#Tainted`) data reaches `{api}` without being sanitized. | A `#Tainted` value is untrusted input; it spreads to anything derived from it. `{api}` is a sink effect (`Db`/`Exec`/`Net` — a database query, subprocess command, or network request), and an untrusted value used there unchecked is the classic injection bug (D-TAINT1). | Pass the value through a `#Sanitizer fn` first — its return value is trusted by contract, so it may reach the sink. |
| E0725 | `{fn}` is `#Replayable` but reaches `{effect}`. | `#Replayable` code must replay from explicit inputs; ambient time, randomness, network, or console IO would make the same replay diverge. | Inject a deterministic clock/RNG or mockable capability, pass recorded data in, or move the ambient effect outside the replayable function. |
| E0745 | `{fn}` is `@Pure` but also declares effects. | `@Pure` already means the empty effect set; a non-empty `#(…)` list on the same function asks for both empty and non-empty at once. | Drop the `#(…)` list to keep the function pure, or remove `@Pure` to allow the listed effects. |
| E0746 | `{api}` has the `{effect}` effect, which can't be rolled back inside a `#Transact` block. | A `#Transact` block undoes its work on a `?`-failure; a network, file, or subprocess effect (`Net`/`Fs`/`Exec`) leaves committed external state a rollback can't take back, so performing it on the block's direct path would break the all-or-nothing contract (D-TXN2). | Move the call after the block, or register it with `<handle>.on_commit(() => { … })` so it runs only after a clean commit. |
| E0747 | This callback uses the effect `{effect}`, which the parameter doesn't allow. | A `@Pure fn(…)` parameter demands a pure callback, and a `#(E) fn(…)` parameter bounds the callback to the listed effects; the actual callback's inferred effects must be a subset (D-EFF2). The bound is checked at the call site, so an impure callback is rejected before it runs. | Pass a callback within the bound (a `@Pure fn` for a pure parameter), or widen the parameter's effect bound. |
| E0748 | `#(via {param})` on `{fn}` names no such parameter / a parameter that isn't a callback. | `#(via f)` publishes a function's effects as a tight pass-through of its callback parameter `f` (D-EFF2); `f` must be a parameter of the function whose type is a `fn(…)`. | Point `via` at a function-typed parameter, or drop the `#(via …)` annotation. |

## Web backend partition diagnostics (c123, D-WASM1 / D-JSBIND1)

| Code | What | Why | Fix |
|------|------|-----|-----|
| E-WEB-CROSS-PARTITION | `{caller}` is compiled to {caller_bucket} but calls `{callee}`, which lives in {callee_bucket}. | The web backend keeps DOM/view code in JS and compute in WASM; a direct call across that boundary is not allowed yet (D-WASM1). | Move the call behind a generated bridge, colocate both functions in the same bucket, or adjust `#Target` / `#Wasm` / `#Js` markers. Run `jet build --target web --explain-partition` to audit assignments. |
| E-WEB-ABI-TYPE | `{type}` cannot cross the JS/WASM boundary {context}. | Web exports and imports only admit ABI-safe types: scalars, `String`, `List`/`Map` of ABI-safe values, and `@[Codable]` structs/enums whose fields are ABI-safe (D-JSBIND1). | Use a scalar, `String`, a `List`/`Map` of ABI-safe values, or add `@[Codable]` to the struct/enum and keep every field ABI-safe. |
| E-WEB-TARGET-BROWSER | `{fn}` is pinned to Wasm but uses the `Browser` effect. | A Wasm-pinned function cannot call browser/DOM APIs directly; the partition keeps view code in JS (D-WASM1). | Remove the `#Wasm` / `#WasmExport` pin, move browser work into a `#Js` function, or drop the browser API calls. |
| E-WEB-TIR-UNSUPPORTED | Web output cannot compile `{fn}` yet. | Web builds use the same checked executable body path as native builds; this function uses a construct the web output cannot lower today (D-WEBTIR1). | Move the unsupported work behind a Wasm export that uses covered Jet constructs, or simplify this function for the web target. |

## Native OS platform gating diagnostics (c134, D-OSTARGET1)

| Code | What | Why | Fix |
|------|------|-----|-----|
| E-OSTARGET-MIXED-AXIS | `#Target(Os.{os})` can't combine with `#Target({web})` on `{item}`. | The OS axis (`Os.Linux`/`Os.Macos`/`Os.Windows`, native platform gating) and the web axis (`Wasm`/`Js`/`Web`, D-WASM1's browser partition) are mutually exclusive — one item can't compile for both a specific native OS and a web bucket. | Pick one axis: remove the `#Target(Os.{os})` marker or the web-axis marker. |
| E-OSTARGET-UNMATCHED-CALL | `{caller}` uses `{gated_type}`, whose `impl` is gated to `#Target(Os.{os})`, without itself being gated to match. | An OS-gated impl only exists in the build for that OS; code reachable on other platforms would hit a missing method, so this is caught at compile time, not left to fail as a link (or a raw rustc) error. | Only use `{gated_type}` from inside an `impl` already gated to `#Target(Os.{os})`, or move `{caller}`'s body into one. |
| E-OSTARGET-BUILD-CONTEXT | a `comptime if … == { … }` dispatch branches on `build.os`. | `build.os` is the one compiler-known comptime value this dispatch folds on — it selects the arm matching the build's target OS at compile time (D-OSTARGET2). | write `comptime if build.os == { .Linux -> … .Macos -> … .Windows -> … }`, or use a plain runtime `if` for a value that isn't known at compile time. |
| E-OSTARGET-DISPATCH-ARM | `{found}` is not an OS arm — a `build.os` dispatch matches `.Linux`, `.Macos`, or `.Windows`. | Each arm gates code for exactly one native OS, so its head is a bare, payload-free OS variant — the same set `#Target(Os.*)` uses — and each OS appears at most once. | write `.Linux -> …`, `.Macos -> …`, or `.Windows -> …` (add an `else -> …` for a shared fallback). |
| E-OSTARGET-DISPATCH-EXHAUSTIVE | this `build.os` dispatch doesn't cover every target OS — missing: {list}. | A build can target any native OS, so the dispatch must handle each one — otherwise a build for a missing OS would have no arm to run. | add an arm for each missing OS ({list}), or an `else -> …` catch-all. |

## Qualifier taxonomy diagnostics (D-QUAL2)

There are exactly two kinds of qualifier. A **`trait`** has at least one method
and dispatches via a vtable; a **`tag`** has no methods and erases at runtime.
The beginner rule is one sentence: *methods → trait, no methods → tag.* A tag is
a pure marker, so it may not carry methods (E0732) and may not stand where
dispatch or method attachment is expected — `derive`d, or implemented/used as a
trait (E0731). Both are compile-time-only; a tag generates no code.

| Code | What | Why | Fix |
|------|------|-----|-----|
| E0731 | `{tag}` is a tag, but {context} needs a trait. | A `tag` is a marker that erases at runtime and carries no methods; dispatch and method attachment need a `trait`. | Declare `{tag}` as a `trait` with the method(s) it should provide. |
| E0732 | The tag `{tag}` declares a method `{method}`, but tags have no methods. | A `tag` is a marker that erases at runtime; only a `trait` carries methods and dispatches. | Make `{tag}` a `trait` if `{method}` should dispatch, or remove the method to keep `{tag}` a marker tag. |

A method written in a tag body (E0732):

```
Error [E0732]: the tag `Reviewed` declares a method `review`, but tags have no methods
  --> tags.jet:2:8
    |
  2 |     fn review(self) -> Int;
    |        ^^^^^^
 Why: a `tag` is a marker that erases at runtime; only a `trait` carries methods and dispatches
 Fix: make `Reviewed` a `trait` if `review` should dispatch, or remove the method to keep `Reviewed` a marker tag
```

Deriving a tag (E0731) — `derive` attaches method impls, so its target must be a
trait:

```
Error [E0731]: `Reviewed` is a tag, but `derive` needs a trait
  --> derive.jet:6:12
    |
  6 |     derive Reviewed
    |            ^^^^^^^^
 Why: a `tag` is a marker that erases at runtime and carries no methods; dispatch and method attachment need a `trait`
 Fix: declare `Reviewed` as a `trait` with the method(s) it should provide
```

## Typestate diagnostics (D-STATE1 / D-STATE-DECL / D-STATE-REQ / D-STATE-TRANS)

A value moves through named **states**. Operations declare the state they need with
`#State(S)` and the state they advance the value to with `#Transition(From -> To)`.
Calling an operation on a value in the wrong state is **E0150**, caught at compile
time. States are compile-time facts threaded through each function; they **erase in
codegen** (zero runtime cost). When the checker cannot follow a value's state
precisely (it escapes into a field, a non-local receiver, a state-divergent branch
join) it stays silent rather than risk a false error on correct code.

States are declared in a dedicated block (D-STATE-DECL, option B):

```jet
state Reservation { Pending, Confirmed, CheckedIn }
```

When a `state TypeName { … }` block is present, every `#State(X)` / `#Transition(A
-> B)` marker on `TypeName` methods must reference a name from the declared set
(unknown name = **E0151**). A declared state with no outgoing `#Transition(S -> …)`
is a **dead-end** warning (**L0151**) — a half-built machine still compiles.

| Code | What | Why | Fix |
|------|------|-----|-----|
| E0150 | `{op}` needs `{type}` in state `{required}`, but `{value}` is in state `{current}`. | Typestate (D-STATE1): an operation is valid only in a given state; calling it out of order is the bug typestate prevents. | Transition the value into `{required}` first — call the transition that reaches it (e.g. `pay` to reach `Confirmed`). |
| E0151 | `{state}` is not a declared state of `{type}`. | Typestate (D-STATE-DECL): `state {type} { … }` defines the valid state labels; a name not in that set is likely a typo — a phantom state no transition can reach. | Correct the spelling, or add the name to the `state {type} { … }` declaration. |
| E0153 | protocol `{name}` failed to expand into handle types. | Protocol/session types (D-PROTO1): the compiler generates `#SingleUse` `.Client`/`.Server` stubs from the `protocol` block — a generated fragment did not parse. | Check the protocol declaration for typos; if this persists, file a bug. |
| E0160 | this value can't be incremented or decremented. | Only a mutable name or field like `count` or `self.hits` accepts `++`/`--` (D-INCR1). | Use a `:=` binding and write `name += 1` / `name -= 1`. |
| E0161 | `{what}` | Increment and decrement edit the binding or field in place (D-INCR1). | Declare with `:=` or mark the parameter `&` if the function should change it. |
| E0162 | `` `++`/`--` is not defined for {type} ``. | Increment and decrement work on integer types only (D-INCR1). | On `Float`, use `+= 1.0` / `-= 1.0`; otherwise use `+= 1` / `-= 1` on an integer binding. |
| E0163 | increment and decrement can't target an indexed slot. | Write the full update: `map[key] = map[key] + 1` (D-INCR1). | Use `+= 1` on a name, or assign through `=` with the whole right-hand side. |
| E0154 | after `{from} ->` expected `{expected}`, found `{to}`. | Protocol messages (D-PROTO2): each line must be `client -> server: Msg(…)` or `server -> client: Msg(…)` — other endpoint pairs are not part of the ratified spelling. | Write `client -> server: …` or `server -> client: …`. |
| L0151 | `{state}` (in `state {type}`) has no outgoing transition. | Typestate (D-STATE-DECL): a state with no `#Transition({state} -> …)` is a dead end — a value that reaches it can never advance further. | Add `#Transition({state} -> NextState) fn …`, or remove `{state}` from the declaration. |

`check_in` requires a `Confirmed` reservation, but the value is still `Pending`:

```
Error [E0150]: `check_in` needs `Reservation` in state `Confirmed`, but `r` is in state `Pending`
  --> reservation.jet:29:11
    |
 29 |     r = r.check_in()
    |           ^^^^^^^^
 Why: typestate (D-STATE1): `check_in` is only valid in state `Confirmed`; calling it in `Pending` is the out-of-order-events bug it prevents
 Fix: transition it first: call `pay` to reach `Confirmed`
```

## C FFI diagnostics (E2-M14, S59)

| Code | What | Why | Fix |
|------|------|-----|-----|
| E3201 | C library `{lib}` was not found. | Jet looked for a `{lib}: c@…` dep in `pkg.jet`, then tried `pkg-config {lib}` on the system; neither provided include/link paths. | Install the system package (e.g. `pacman -S {lib}`), or declare it as `{lib}: c@system` in `deps:`. |
| E3202 | Type `{ty}` cannot cross the C boundary here. | C FFI allows by-value scalars and `String` in ordinary code; pointers and other gated types need `use core.mem` and an `#Unsafe { … }` region (S58). | Move the call inside `#Unsafe`, or change the type to a C-safe value type. |
| E3203 | `{ty}` is not a C-compatible type for a foreign function parameter or return. | `#Extern` / `#Bindgen` functions must use types with a stable C ABI at the edge. | Use scalars, `String`, or a struct with C layout; pointers only through the gated tier. |
| E3204 | Two different `use` forms refer to the same C library `{lib}`. | S59 allows one bring-in per C lib per file — either `use "{header}" as alias` or `use c.{lib} as alias`, not both. | Remove one line; keep the form that matches your workflow. |
| E3205 | Overlay `{name}` disagrees with the generated binding. | User `#Extern module c.{lib}` may override bindgen symbols, but the Jet signature must stay compatible when replacing. | Match the generated signature, or rename your overlay function. |
| E3206 | Module path `{path}` uses the reserved segment `__bindgen__`. | Autogen lives in `c.{lib}.__bindgen__`; users declare overlays as `#Extern module c.{lib}` only. | Drop `__bindgen__` from your module path, or use `#Extern module c.{lib} { … }`. |
| E3207 | `#Bindgen` is only allowed in generated cache files. | `.jet/bindings/c/{lib}.jet` is written by `jet bind`; hand-written sources use `#Extern module`. | Edit your overlay file with `#Extern module`, or regenerate the cache with `jet bind`. |
| E3208 | Could not generate bindings from `{header}`. | Header parsing or translation failed in the bind backend. | Fix the header path, install dev headers, run `jet bind` manually for details, or hand-write `#Extern module c.{lib}`. |
| E3209 | The linker couldn't find C library `{lib}`. | Your program links against `{lib}`, but the linker reported `cannot find -l{lib}` — the library isn't on the link search path. | Declare it in `deps:` so Jet provisions it: `{lib}: c@system` (host pkg-config, else fetched from nixpkgs), or `{lib}: c@nixpkgs:<attr>` to pick the nixpkgs attribute, or install the system package. |
| E3210 | Couldn't fetch C library `{lib}` from nixpkgs. | `{lib}: c@system` asked Jet to provision `nixpkgs#{attr}`, but `nix build` failed: `{reason}`. | Check the attr exists (`nix build nixpkgs#{attr}`), or point at a local build with `{lib}: c@"<path>"`, or install it and use `system`. |

## Cross-compilation and freestanding diagnostics (E2-M15)

| Code | What | Why | Fix |
|------|------|-----|-----|
| E3301 | `{api}` is not available in a freestanding build. | `--freestanding` targets have no OS; only `core`-level APIs are available. | Embed data at compile time with `@embed("file")`, or build without `--freestanding`. |
| E3302 | Target `{triple}` is not available. | rustc doesn't have the standard library for this target compiled in, or the target triple is not recognised. | Run `jet doctor --target=<triple>` to see what's missing, or `rustup target add <triple>` to install it. |
| E3303 | This freestanding program allocates memory but has no global allocator configured. | `--freestanding` builds cannot use the OS heap; a custom allocator is required. | Add `use core.mem;` and configure an arena or fixed allocator with `mem.set_allocator(…)`. |

## Pure evaluation diagnostics (E2-M16)

| Code | What | Why | Fix |
|------|------|-----|-----|
| E3401 | `{pure_fn}` calls the impure function `{call}`. | A `@Pure fn` may only call other `@Pure fn`s and pure builtins. Impure calls make the result non-deterministic (D-PURE2). In `jet eval --pure` the whole call graph from `run` is checked transitively; the why-line shows the full chain (`run → a → b calls \`print\``) so the user can find the leak. | Mark `{call}` as `@Pure fn`, or remove the call from `{pure_fn}`. |
| E3402 | `{call}` is not allowed during a sandboxed package build. | Package builds run with ambient I/O and network access disabled (D-PURE2). | Compute this value at compile time or pass it in as a parameter. |
| E3403 | `{what}` is non-deterministic and cannot appear in a pure evaluation. | Pure evaluation must produce the same result on every machine (D-PURE2). | Remove this call, or do not mark the enclosing function `@Pure`. |

## REPL diagnostics (E2-M18 `jet repl`)

These are produced by `jet repl` — the interactive REPL session. They follow
the same what/why/fix voice as all other diagnostics (D-REPL17=A), with the
REPL step number in place of a file span (`<repl:N>`).

| Code | What | Why | Fix |
|------|------|-----|-----|
| E1801 | This snippet ran more than `{N}` interpreter steps without finishing. | The REPL interpreter caps each input to avoid hanging your session; this almost always means a loop that never ends. | Check any loops for a condition that never becomes false. Use `:run` to allow unbounded execution (compiles and runs instead of interpreting). |
| E1802 | The REPL interpreter can't run `{feature}`. | The REPL is an interpreter for learning Jet; some features — FFI, tasks/channels, `#Unsafe`, and OS-level APIs — require the real compiler. | Run `jet run <file.jet>` or `jet build <file.jet>` to use the full compiler. |

## CLI diagnostics (E2-M3 developer command UX)

These are produced by the `jet` driver itself, not by checking a `.jet`
file, so they have no source span. They use the same what/why/fix voice
and exit with code **2** (usage error — see "Exit codes" in
docs/spec/architecture.md). Both carry a "did you mean" when a known
command/flag is within edit distance 2. Their golden transcripts live in
`tests/cli/` (blessed with `UPDATE_EXPECT=1 cargo test --test cli`).

| Code | What | Why | Fix |
|------|------|-----|-----|
| E2101 | `{cmd}` isn't a jet command. | Every jet run starts with a command like `run`, `check`, or `new`. | Did you mean `jet {closest}`? Run `jet help` to see them all. |
| E2102 | `{flag}` isn't a flag jet understands. | jet ignores no flags silently, so a typo can't quietly change a build. | Did you mean `{closest}`? Run `jet help` to see the flags. |

### `jet doctor` advisories

`jet doctor` (decision **D-DX2**, ratified 2026-06-16 — health checks *and*
auto-fix) self-diagnoses the environment Jet hides: the rustc backend, the
build cache and package store, PATH, the language server, and the C-FFI/cargo
bridge (the FFI section is decision **D-BUILD1**). It runs **offline by
default** — only `--online`/`--network` lets it probe the registry. Each
problem prints a single advisory line tagged **L2101** with the concrete fix.
Safely auto-fixable problems (a missing cache or store directory) are applied
under `jet doctor --fix`; doctor never modifies user source or package
manifests. Exit code is 0 when every check is healthy or only advisories
remain, and 1 when a hard problem (no rustc, an unwritable store) blocks normal
use.

| Code | What | Why | Fix |
|------|------|-----|-----|
| L2101 | `jet doctor` found an environment problem with a known fix. | Jet hides a rustc backend, a build cache/store, and a C-FFI bridge; doctor surfaces a broken one before it derails a build. | Apply the fix printed on the advisory line; for a missing cache or store directory, run `jet doctor --fix`. |

## Workspace diagnostics (D-WORKSPACE1=B, D-WORKSPACE2=A)

These diagnostics are emitted by the `workspace.jet` evaluator when the file
exists but can't be evaluated to a valid `WorkspacePlan`.

| Code | What | Why | Fix |
|------|------|-----|-----|
| E0995 | `workspace.jet` has no `module workspace { … }` declaration. | `workspace.jet` is the monorepo index (D-WORKSPACE2=A); it must contain exactly one `module workspace { members: … }` body. | Write `module workspace { members: find("./packages") }` (or an explicit list) in `workspace.jet`. |
| E0996 | `members:` evaluated to something other than a list of strings. | The `members:` value must evaluate to `[String]` — a list of relative package directory paths. | Use `find("./packages")` or a list literal like `["./packages/hello", "./packages/ranker"]`. |
| E0997 | `find("…")` in `members:` names a directory that doesn't exist. | `find` scans that directory for subdirectories containing `pkg.jet`; the directory must exist relative to `workspace.jet`. | Create the directory or correct the path in `members: find("…")`. |

### E0995 — No workspace module

```
error[E0995]: `workspace.jet` must declare `module workspace { … }`
  --> workspace.jet
  |
  = `workspace.jet` is the monorepo workspace index (D-WORKSPACE2=A); it must
    contain exactly one `module workspace { members: … }` declaration
  = write `module workspace { members: find("./packages") }` in `workspace.jet`
```

### E0996 — members: not a list

```
error[E0996]: `members:` must evaluate to a list of package paths
  --> workspace.jet:2:14
  |
2 |     members: 42
  |              ^^ not a `[String]`
  |
  = `members:` describes the packages in this workspace; it must be a `[String]`
    list of relative paths or a `find("…")` call
  = example: `members: find("./packages")` or `members: ["./pkg/hello"]`
```

### E0997 — find dir missing

```
error[E0997]: `find` can't read the directory `./no-such-packages`
  --> workspace.jet:2:14
  |
2 |     members: find("./no-such-packages")
  |              ^^^^^^^^^^^^^^^^^^^^^^^^^^
  |
  = `members: find("<dir>")` scans that directory for package subdirectories;
    it must exist relative to this file
  = create the directory, or fix the path so it points at your packages folder
```

## Package management diagnostics (M12, D-JPK-FILES)

`jetpack.toml` is read by the `jetpack` CLI (never by `jet run`/`jet build` —
R9). Malformed input is surfaced as E1214/E1215 before any resolution runs.
These diagnostics have no source span (the file is not a Jet source file) and
follow the same spanless voice as CLI diagnostics. Pinned as rendered-output
snapshots in `tests/jetpack.rs` (the `tests/ui/` harness only renders front-end
`.jet` diagnostics).

| Code | What | Why | Fix |
|------|------|-----|-----|
| E1214 | `jetpack.toml` line {n} is not a valid assignment or table header. | Every line in `jetpack.toml` must be `key = "value"` (inside a table), a `[table]` header, or a blank/comment line. Anything else can't be interpreted. | Fix the line so it is either `[table]`, `key = "value"`, or a blank or `#`-comment line. |
| E1215 | `jetpack.toml` {kind} `{name}` is not recognized. | `jetpack.toml` only accepts the tables `[repo]` and `[sources]`, and the keys listed for each. An unknown name is usually a typo. | Did you mean `{suggestion}`? Check the allowed names for this table. |
| E1225 | `jetpack.toml` `[packages]` is retired. | Monorepo member indexes now live in `workspace.jet` so package sets use Jet's module grammar instead of a second manifest shape. | Move the member list to `workspace.jet`: `module workspace { members: find("./packages") }`. |
| E1226 | `{name}` is not the package manifest name — Jet reads `pkg.jet`. | The manifest filename is frozen to one spelling (D-JPK-FILES/D-JPK-FILENAME2) so tooling, docs, and every worked example never have to guess which file to read. `pack.jet`, `payload.jet`, and `jet.toml` are retired names from earlier manifest reshapes. | Rename `{name}` to `pkg.jet`. |
| E1227 | `jet` {jet_version} and `{engine}` {engine_version} disagree. | `jet` and its engine binaries (`jetpack`, `jetos`) ship as one toolchain and must match exactly — a version-skewed engine may not understand what `jet` sends it. `jet` checks this with an `--engine-protocol` handshake before running any engine verb. | Use matching `jet`/`{engine}` versions — reinstall the toolchain so both binaries come from the same release. |
| E1228 | `{verb}` needs the `{engine}` engine, which isn't installed. | `{verb}` is an engine verb — `jet` execs `{engine}` for it (D-JPK-DISPATCH1) rather than linking package-manager/OS logic into the compiler binary. | Install the matching Jet toolchain; the `{engine}` binary ships alongside `jet`. |
| E1229 | Role namespace `{ns}` belongs in the module declaration name. | `module {name} { {ns}.{role}: {Type}.{ … } }` splits the role across two places; the canonical form puts it once, in the declaration name, so discovery-by-declaration (`module env.dev`) reads the role straight off the name (D-JPK-MODBODY1). | Write `module {ns}.{role} { … }` and move the contribution's fields up to the module body. |
| E1239 | `module workspace` is declared in more than one file: `{list}`. | The workspace index is discovered by declaration (D-JPK-FILENAME2), so exactly one file may declare `module workspace { … }`. | Keep one declaration (conventionally in `workspace.jet`) and delete the others. |
| E1230 | `{query}` matches more than one workspace member. | A bare (`logging`) or path-form (`packages/logging`) ref with no `source:` prefix resolves against the workspace member index; this one is not unique (D-MONOREF1). | Address one member by its relative path (e.g. `infra/logging`), or use `source:package`. |
| E1231 | `{query}` is not a workspace member. | A bare/path-form ref must name a member listed in `workspace.jet` `members:`; nothing in the index matched (D-MONOREF1). | Use one of the listed members (a did-you-mean is offered), fix the name, or add the package to `members:`. |
| E1232 | A monorepo source could not be fetched. | Resolving a monorepo package fetches the source's workspace index and materializes only the addressed subtree; both the sparse subtree checkout and the full-clone fallback failed (D-MONOREF1). | Check the source URL/rev and network access; if the provider lacks partial-clone support the full clone should still work, so this usually means the rev or repo is unreachable. |
| E1233 | In-repo dependency `{name}` is outside the workspace. | A member's `pkg.jet` depends on another in-repo package, but that package is not in the source repo's `workspace.jet` member index, so the sparse checkout can't include it (D-MONOREF1). | Add the dependency to the source repo's `workspace.jet` `members:`, or depend on it as an external `source:package` ref. |
| E1234 | `{name}` {version} already exists in the registry index and is not yanked. | Published versions are immutable (D-VERSION1) — a version can never be overwritten, only yanked, so anyone who already locked it keeps building the exact same bytes. | Bump the version in `pkg.jet` and publish again, or `jet yank {version}` the existing one first if it was a mistake (yanking hides it from new resolution; it does not free the version number for reuse). |
| E1235 | Couldn't reach the registry index at `{url}`. | The git operation against the registry failed — network, auth, or a stale local clone. The registry is a git repo, so `jet publish`/`jet yank` clone/pull it, write the version line, then commit and push. | Check network access and credentials for `{url}`, or set `JET_REGISTRY_URL` to a reachable mirror. |
| E1236 | A build step tried to reach the network without a locked fetch. | During a build, network access is denied except a locked `fetch(url, sha256:)`; an unpinned fetch would make the build unreproducible (D-JPK-ADAPTER1). | Add the source hash: `fetch("…", sha256: "…")`, or vendor the source with `jet vendor`. |
| E1237 | A build step tried to write outside the output root. | A build may only install files under its own package output root; writing elsewhere would let a build mutate the machine or other packages (D-JPK-ADAPTER1). | Install into a path under the output root (no `..`, no absolute paths). |
| E1238 | Build tool `{tool}` is not a realized dependency. | Build tools must be realized `Pkg` dependencies so the build is reproducible; a build never falls through to host `/usr/bin` (D-JPK-ADAPTER1). | Add `{tool}` as a build dependency in `pkg.jet` so it is realized into the hangar. |
| E1240 | No Rust build toolchain is available to build this package. | Building an `extern rust` bridge dependency needs a pinned Rust toolchain realized into the hangar, or Nix; neither is present (D-JPK-BUILDTOOL1). | Run `jet update jet` to realize the pinned toolchain, or install Nix so the bridge builds through the compatibility provider. |
| E1241 | The staged `core.{ring}` artifact is missing for this platform. | The active toolchain object carries prebuilt ring artifacts, but none for `core.{ring}` on this platform (D-JPK-RINGSHIP1). | The build falls back to the compiler-embedded `core.{ring}`; to ship the staged artifact, realize a toolchain object built for this platform (`jet update jet`). |
| E1242 | The fleet `{fleet}` host `{host}` names an unknown captured system `{system}`. | D-JETOS-FREEZE1: fleets are frozen jetos research capture; captured hosts must still point at a captured system so the plan is coherent. | Define captured `system.{system}: { … }`, or point the host at an existing captured system. |
| E1243 | Fleet `{fleet}` is validated, but `jet push` is not available yet. | The fleet's hosts parse and cross-check clean, but rolling a fleet out over ssh needs single-host jetos realization, which is gated (U15; Phase D, owner greenlight required). `jet push` never fakes a deploy. | Until the jetos realization tier lands, `jet push` captures and validates fleets without deploying them. |
| E1244 | `{field}` isn't a captured `Fleet` field. | D-JETOS-FREEZE1: fleet deployment remains frozen jetos research; only `hosts` is captured for planning. | Remove `{field}`; captured fleets use `hosts: { … }`. |
| E1245 | This captured `Fleet` has no `hosts`. | D-JETOS-FREEZE1: fleet deployment is frozen jetos research, but captured fleets still name hosts for later planning. | Add `hosts: { web1: system.<name> }` if this is research capture. |
| E1246 | Signature verification failed for `{name}` {version}: the signature doesn't match the recorded public key. | This means the package was tampered with after signing, or the index entry is corrupt — the author's Ed25519 signature over the content hash no longer checks out (D-PKGSIGN1). | Do not use this version. Re-run `jet fetch` after clearing the store entry; if the problem persists, report it — this should never happen for an untampered registry. |
| E1247 | Registry `{registry}` requires signed packages (`require_signed: true`) but `{name}` {version} has no signature. | The registry is configured to accept only author-signed releases; an unsigned entry can't be trusted under that policy (D-PKGSIGN1). | Use a different registry, or ask the package author to publish a signed release (`jet publish` auto-signs by default — they likely used `--no-sign`). |
| E1248 | `jet keygen` refused: a signing key already exists at `{path}`. | Overwriting it would orphan every package you've published under the old key — consumers who pinned it (TOFU) would see a key-rotation warning on your next publish (D-PKGSIGN1). | Use `jet keygen --force` if you're sure (e.g. the old key was compromised), or back it up first with `jet key backup`. |
| E1249 | `{value}` is not a valid toolchain pin. | The `jet:` field pins which Jet toolchain builds this project (D-JPK-TOOLCHAIN1); its value is a channel ref, not a version range. `>=1.0.0`-style constraints don't name one reproducible toolchain. | Write a channel: `jet: 0.4` (track the 0.4 series), `jet: 0.4.2` (exact), or a named channel like `jet: main`. |
| E1250 | Toolchain channel `{channel}` is pinned but not locked. | An `--offline`/CI build won't resolve a channel — it needs the exact toolchain version recorded in `.jet/lock`, and none is present (D-JPK-TOOLCHAIN1). Resolving a channel reaches the network, which offline/CI forbids. | Run `jet update jet` to resolve `{channel}` to an exact version, then commit `.jet/lock`. |
| E1251 | Toolchain {channel} ({version}) isn't available for {platform}. | This project pins a Jet toolchain, but no prebuilt object for it was found for this platform. Jet realizes the pinned compiler as a prebuilt — it never builds the compiler from source and never silently falls back to a different `jet` (D-JPK-TOOLCHAIN1). | Move the pin with `jet update jet <channel>` to a toolchain your platform has, or install the pinned toolchain from the release page. |
| E1252 | `jet init` refused: a `pkg.jet` already exists here. | `jet init` writes a fresh package manifest pinning the running toolchain; overwriting one would discard its dependencies, pins, and identity (D-JPK-TOOLCHAIN1). | Edit the existing manifest, or run `jet init` in an empty directory. |
| E1253 | Inline dependency `{name}#{selector}` didn't resolve. | A manifest-less script's `use {name}#{selector};` (U11) has no source to resolve from — the Jet package registry has no fetch path yet, so an inline dependency only resolves from a committed local copy (D-JPK-SCRIPTDEP1). | Commit a copy at `.jet/inline-deps/{name}/<version>/`, or run `jet init` and depend on `{name}` through `pkg.jet` once you have a real source for it. |
| E1254 | This project has no `jet dev` entry. | Project-level `jet dev` (no file argument) runs the entry file's top-level `fn dev()` if it defines one, else `fn run()` (U19, D-JPK-DEVCOMPOSE1). The entry file defines neither. | Add `fn dev() { … }` (a custom dev command) or `fn run() { … }` (the default) to the entry file. |
| E1255 | This project's environment isn't trusted yet. | Entering a project's declared env (`jet env`/`jet dev`) is a supply-chain decision — first entry to a repo that declares packages needs a trust decision (U19, D-JPK-DEVCOMPOSE1). stdin isn't a terminal, so an interactive prompt would hang instead of asking. | Pass `--trust` for this one run, or pre-authorize with `jet config trust add <pattern>`. |
| E1256 | `{cmd}` needs `nix`, which isn't on PATH. | `jet bridge flake` translates a `flake.nix`'s devShell, and `jet env`'s foreign-flake/`devenv.nix` detection shells out to `nix` as the ratified stopgap (U16); neither works without the `nix` binary. | Install Nix from the official installer, or skip the foreign flake and declare packages in `env.*` instead. |
| E1257 | This plugin's exported interface changed incompatibly. | A `target: plugin` package's frozen exported interface is the load-time contract (D-PLUGIN-VERSION1=A) — a prior build's `.jet/cache/api/plugin__<name>.api` snapshot shows an export was removed or its signature changed. Adding a new export is always compatible. | Restore the removed/changed export, or accept this as an intentional breaking change (delete the stale snapshot to re-freeze). |
| E1258 | A plugin can't use any effect. | This package builds as `target: plugin` (D-PLUGIN1=B) — plugins run fully sandboxed with zero host capabilities (the wasmtime host registers no host imports), so any effect (`Fs`/`Net`/`Db`/…) would fail to instantiate at load time. There is no gate or grant to widen this (I1: the sandbox is the safety boundary, not an opt-in). | Remove the effectful call, or move it out of the plugin into the host program that loads it. |
| E1259 | Couldn't build the plugin's WASM Component. | Building a `target: plugin` package shells out to `rustc --target wasm32-unknown-unknown` and `wasm-tools component embed`/`new` (D-DEP-WASM1=A); one of them is missing or failed. | Make sure `rustc` supports `wasm32-unknown-unknown` and `wasm-tools` is on PATH (both ship in the project's `nix develop` shell). |
| E1260 | A plugin's exported function has an unsupported signature. | v1 plugin exports (D-PLUGIN-EXPORT1=A) support only functions whose parameters and return type are all `Int` or all `Float` — Bool/Text need more of the Component Model's ABI machinery, a real follow-on rather than this increment's scope. | Narrow the signature to all-`Int`/all-`Float`, or drop `pub` if this function isn't meant to be called across the plugin boundary. |
| E1261 | Service `{name}` never became healthy. | `jet dev`/`jetpack services up` supervises a `services:` process, then polls its readiness contract (`ready:`, else a TCP probe on its first `ports:` entry, else a bare process-alive check) until it passes or a timeout elapses (U12); it never passed in time. | Check `jetpack services logs {name}` for what the process printed, confirm its `init`/`ready` commands are correct, or raise the timeout isn't configurable yet — fix the service itself. |
| E1262 | Service `{name}` has a field jetpack doesn't recognize: `{field}`. | A dev-supervised `Service` stays the one ratified open record (U12) at parse time, but jetpack's dev-runtime tier is the only consumer of a dev service's fields — unlike the jetos `system.*.services` capture, nothing downstream forwards unread metadata, so an unrecognized key here is almost always a typo. | Rename `{field}` to one of the recognized keys (`enable`, `ports`, `init`, `shutdown`, `data_dir`, `ready`), or remove it. |
| E1263 | No secret named `{name}`. | `jetpack secrets get {name}` decrypted the store (`.jet/secrets.age`) fine, but it has no entry called `{name}` (U13). | Set it first with `jetpack secrets set {name} <value>`, or check the spelling. |
| E1264 | `{fn}` reads a secret but doesn't declare the `Secret` effect. | Reading a secret (`core.vault.get`) always requires an explicit grant (U13, D-JPK-SECRETCRYPTO1) — unlike every other effect, there is no silently-inferred default: a bare `fn` with no `#(…)` list, or one that omits `Secret`, is rejected even though the same function calling `core.files`/`core.net` with no declared bound at all would pass silently. | Add `#(Secret)` to `{fn}`'s signature (or widen an existing `#(…)` list to cover it). |
| E1265 | `core.vault.get` can't be reached from a build-time context. | Module-field/comptime evaluation (`pkg.jet`, `env.jet`, …) runs before secrets are ever decrypted (U13) — a repo's encrypted store is only ever opened at ordinary runtime (`core.vault.get` inside a `#(Secret)`-graned function), and unlike the Tier-2 comptime effect gate (E3410/E3411), there is no `#Impure`/`--allow-impure` escape hatch here: a build artifact must never bake in a decrypted secret. | Move the secret read out of comptime/module-field evaluation and into ordinary runtime code. |
| E1266 | `` `<word>` isn't an active image kind `` (or `` `kind: .<word>` doesn't match this image's `from:` ``). | D-JPK-IMAGE1 + D-JETOS-FREEZE1: active Jetpack images use `.Oci`; `.Iso` disk images are frozen jetos research capture. | Write `kind: .Oci` for active Jetpack images, or keep `.Iso` only as research capture. |
| E1267 | The image `{image}` is built from a non-executable package `{package}`. | D-JPK-IMAGE1: an `.Oci` image's `from: packages.<name>` must name a package this project's `pkg.jet` declares `executable` — a `library`-kind package has no binary to containerize, and an undeclared name can't be confirmed either way. | Declare `{package}: executable` in `pkg.jet`, or point `from:` at an existing executable package. |
| E1268 | `` `jet image <name>` can't push to `<ref>` yet. `` | D-JPK-IMAGE1: `--push` speaks the registry protocol, which needs TLS support jetpack doesn't have yet. `jet image` builds the OCI layout natively either way; it just never fakes the push. | Build without `--push`, then push the OCI layout with another tool for now; `--push` will work once TLS lands. |
| E1269 | `` `<field>` isn't shaped like <expected>. `` | D-JPK-IMAGE1: an `.Oci` image's `kind`/`expose`/`env_vars`/`files`/`base` fields each have one fixed shape (a bare leading-dot value, a list of ports, a string-keyed map, a list of paths, `oci("<ref>")`) — `Image` is a closed record, so a misshapen recognized field is rejected rather than silently ignored. | Rewrite the field to match its documented shape. |
| E1270 | Adapter package could not be realized. | `Pkg.adapt(...)` turns source bytes into a normal package, so its `source:` must be a provider ref such as `path@vendor/tool` and its recipe must be one of the supported U20 recipes (`Recipe.copy()` or `Recipe.prebuilt(bin:, as:)`). | Check the `Pkg.adapt(...)` source and recipe. |
| E1271 | Source channel `{name}` is not locked / could not be resolved. | D-JPK-CHANNEL1 keeps tracking intent (`#latest`, `#main`, `#vN.x`) beside an exact lock entry. Build/run/env never re-resolve channels, and CI/offline may not invent a fresh exact source. | Run `jetpack update {name}` with network or fixture metadata, then commit `.jet/lock`. |
| E1272 | `{count}` package refs need the Nix bridge, and Nix is not installed. | D-JPK-NONIX1 lets Nix-free packages realize first, then reports only the holes that still route through the Nix compatibility provider. This is distinct from E1256: foreign flake and `jet bridge flake` commands cannot run at all without `nix`, while package refs can coexist with native core/adapted packages. | Install Nix from the official installer, or replace the listed refs with native sources/adapters; `jetpack add <ref> --adapt` drafts an adapter snippet. |
| E1273 | Package build failed at a logged step. | D-JPK-BUILDDBG1 preserves the failed build scratch under the hangar and records each recipe step's command/output. The primary error names the failed step instead of dumping provider noise. | Run `jet logs <pkg>` for the full per-step log, or rerun the build with `--shell-on-fail` to debug inside the preserved scratch. |
| E1274 | No build log exists for `{pkg}`. | `jet logs` and package-form `jet explain <ref>` read persisted Jetpack build attempts. If a package has not failed or built through the logged runner on this machine, there is nothing local to explain. | Run `jet build <ref>` first; for diagnostic-code help, keep using `jet explain E1234`. |
| E1275 | Build sandboxing is required but unavailable. | `jetpack config sandbox require` turns sandbox fallback into a hard failure. This machine cannot provide Jetpack's unprivileged sandbox tier, so running adapter builds would violate local policy. | Run `jetpack config sandbox allow` to permit fallback, or enable unprivileged sandbox support on this machine. |
| E1276 | `--offline` forbids network access. | Realize-class verbs must run from the current lock and local hangar when offline. Network-class verbs (`add`, `update`, `outdated`, publish/cache sync) cannot refresh metadata under `--offline`, and a missing local object cannot be fetched. | Drop `--offline` for this command, or realize/fetch the needed object before going offline. |
| E1277 | A jetos option key uses a retired namespace. | D-JPK-OSNS1=B and D-JOS-SYSTEMTREE1=A: jetos option keys start with full-word namespaces: `filesystem`, `network`, `packages`, `services`, `users`, `groups`, `secrets`, `boot`, `kernel`, `init`, or `health`. | Rename the option namespace, for example `net.hostName` becomes `network.hostName`. |
| E1278 | A jetos activation proof is incomplete. | D-WD8 requires `jet os switch` to prove the plan, risk class, generated service artifacts, and rollback evidence before changing the active generation pointers. | Rebuild the generation so the proof artifacts are regenerated, or discard a hand-edited generation. |
| E1279 | jetos VM proof tools are missing. | D-JOS-VMDEPS1=A requires pinned QEMU, firmware, ISO, bootloader, filesystem, EFI image, and initrd compression tools before install/reboot proof can run. | Realize or expose the required tools, then rerun `jet os vm prove <host> --disk <disk>`. |
| E1280 | The jetos CachyOS kernel package is missing. | D-JOS-KERNELSRC1=A: `.CachyOS` resolves to a first-party `cachyos-kernel` package with boot artifacts and provenance. | Declare a first-party source that provides `cachyos-kernel`, or select a different ratified kernel. |
| E1281 | The jetos systemd init package is missing. | D-JPK-OSINIT1=A: the default jetos init path is systemd, so the generation needs a first-party `systemd` package with bootable init artifacts. | Declare a first-party source that provides `systemd`, or select a ratified init override. |
| E1282 | The jetos CachyOS boot artifacts are missing. | D-JOS-KERNELSRC1=A: the first-party `cachyos-kernel` package must provide a Linux kernel image and initrd with bootable file headers so the generation and installer can boot the same payload. | Add `boot/vmlinuz-cachyos` and `boot/initrd-cachyos` with real boot payloads, or select a different ratified kernel. |
| E1283 | The jetos systemd init artifact is missing. | D-JPK-OSINIT1=A: the first-party `systemd` package must provide a bootable init binary for `/sbin/init`. | Add `bin/systemd`, `lib/systemd/systemd`, or `sbin/init` to the package output, or select a ratified init override. |
| E1284 | The jetos CachyOS source recipe is missing. | D-JOS-KERNELBOOTSTRAP1=A: the first-party `cachyos-kernel` package must carry source-built recipe, builder, config, patch, and initrd-input provenance beside the boot artifacts. | Add `source/recipe.jet`, `source/build.sh`, `source/config`, `source/patches.manifest`, and `source/initrd-inputs.manifest` to the package output. |
| E1285 | The jetos VM guest proof has not run. | D-JOS-VMCOMMAND1=A and D-JOS-DESKTOP1=A require `jet os vm prove` to record an actual installer boot, disk install, reboot, serial guest verification, and graphical desktop verification before claiming proof. A written harness is not proof, and a guest proof must match the host, generation, disk, media proof, tool hashes, and terminal/desktop/graphical guest assertions. | Inspect the VM run logs, fix the boot/install path, then rerun `jet os vm prove` to capture a guest proof marker. |
| E1286 | The jetos CachyOS source build failed. | D-JOS-KERNELBOOTSTRAP1=A requires the first-party `cachyos-kernel` package to build boot artifacts from its recorded source recipe before VM/install proof can claim the selected `.CachyOS` kernel. | Check the first-party `cachyos-kernel` source recipe and rerun `jet os build`. |
| E1287 | jetos VM run needs a proved installed disk. | D-JOS-VMRUN1=A keeps `jet os vm run` on the same proof-before-use path as `jet os vm prove`: an interactive VM may open only the latest generation's disk after a matching guest-passed proof binds host, generation, disk, media proof, tool hashes, and assertions. | Run `jet os vm prove <host> --disk <disk>` first, then rerun `jet os vm run`. |
| E1288 | The jetos GNOME desktop package is missing. | D-JOS-DESKTOP1=A makes the default desktop profile GNOME-on-Wayland with terminal fallback, so the generation must include first-party `gdm`, `gnome-session`, and `gnome-shell` commands. | Declare first-party packages for `gdm`, `gnome-session`, and `gnome-shell`, or select a ratified non-GNOME desktop profile. |
| L0203 | `use {name}#{selector};` isn't pinned to an exact version. | An inline script dependency (U11) has no lockfile until `jet lock` runs; a loose selector (`1.4` rather than `1.4.2`) can resolve to a different version on a fresh clone (D-JPK-SCRIPTDEP1). | Write the exact version Jet resolved (`use {name}#<major.minor.patch>;`), or run `jet lock` to pin it in `<script>.lock`. |
| L0204 | `{field}` in `{file}` has no `env.*` equivalent yet. | `jet bridge flake` (U16) is a best-effort translator; some `flake.nix`/`devenv.nix` fields (`shellHook`, multiple named devShells, `buildInputs` vs `nativeBuildInputs`) have no ratified `env.*` spelling. | Review the generated shim and add `{field}`'s effect by hand if you need it — the shim is a starting point, not a full translation. |
| L0205 | Build sandboxing is unavailable; adapter builds will run unsandboxed. | D-JPK-NODAEMON1 forbids privileged helpers and daemons. When the platform cannot offer an unprivileged sandbox, Jetpack must say so instead of silently downgrading. | Run `jetpack config sandbox require` to refuse fallback. |

## Machine-readable diagnostics (`--json`)

Passing `--json` to `jet check`, `jet build`, or `jet test` makes the
driver emit diagnostics as **data** instead of prose, for scripts, CI,
and editors. This is decision **D-DX1** (ratified 2026-06-16): a single,
**stable, versioned** schema, shared by the `--json` CLI flag, the future
`jet fix` engine, and the LSP. The serializer lives in `Source/DiagnosticsJSON.rs`
(`to_json` / `render_all_json`); this section is its single source of
truth. Adding a field is allowed any time; **removing or repurposing one
requires bumping `schema_version`.**

**Shape — JSON Lines.** One self-contained JSON object per diagnostic,
each terminated by `\n`, matching `cargo --message-format=json`. A run
with N diagnostics prints N lines on **stdout**; a clean run prints
nothing on stdout. Human prose and the `jet explain` footer still go to
**stderr** in the non-`--json` path, and `--json` emits **no ANSI ever**
(scripts must never parse ANSI). Field order is fixed and numbers are
integers, so the bytes are deterministic and snapshot-pinnable.

**Fields (schema_version 1):**

| Field | Type | Meaning |
|-------|------|---------|
| `schema_version` | integer | Schema version; `1` today. Bumped only for breaking changes. |
| `code` | string | The diagnostic code, e.g. `"E0037"`. Pairs with `jet explain`. |
| `severity` | string | `"error"` or `"warning"`. |
| `message` | string | The one-line *what* (same text as the human `Error [...]:` line). |
| `why` | string | The *why* — the rule behind the diagnostic. |
| `fix` | string | The *fix* — the concrete next step (human text). |
| `file` | string | Path of the source file the diagnostic is about. |
| `span` | object \| null | Source location, or `null` for whole-file diagnostics. |
| `suggestions` | array | Machine-applicable fixes (possibly empty). |
| `detail` | string \| null | Extra indented detail (e.g. tool output), or `null`. |

A **`span`** object carries both human and machine coordinates:
`start_byte`, `end_byte` (byte offsets into the file, the range a fix
slices), and 1-based `start_line` / `start_col` / `end_line` / `end_col`.

A **`suggestions`** entry is `{ "message", "replacements": [...] }`, where
each replacement is `{ "file", "span", "new_text" }` — apply `new_text`
over the byte range `[start_byte, end_byte)` in `file`. This is the
contract the future `jet fix` engine and LSP code actions consume; today
it is populated from live teaching auto-corrects (e.g. E0037 "replace
`println` with `print`"). Diagnostics with no mechanical fix emit
`"suggestions": []` — the field is always present so consumers never
special-case its absence.

Example (`jet check`, one teaching error, wrapped for readability —
the real output is one line):

```json
{"schema_version":1,"code":"E0037","severity":"error",
 "message":"Jet calls it `print`, not `println`","why":"...","fix":"replace `println` with `print`",
 "file":"hello.jet","span":{"start_byte":16,"end_byte":23,"start_line":2,"start_col":5,"end_line":2,"end_col":12},
 "suggestions":[{"message":"replace `println` with `print`",
   "replacements":[{"file":"hello.jet","span":{"start_byte":16,"end_byte":23,"start_line":2,"start_col":5,"end_line":2,"end_col":12},"new_text":"print"}]}],
 "detail":null}
```

The golden transcripts pinning these bytes live in `tests/cli/json_*.txt`
(blessed with `UPDATE_EXPECT=1 cargo test --test cli`).
### E0910 — Published schema breaking change

`@PublishedSchema` pins a record's saved shape at release (D-MIGRATE1/2). A
breaking data-shape change is refused unless a `migration` op declares the intent.
E0910 is the umbrella for every such case; the what/why/fix is case-specific.

| Case | What | Why | Fix |
|------|------|-----|-----|
| dropped (D-MIGRATE1) | The published record `{Type}` dropped `{field}` since version `{version}`, with no migration to bridge it. | Old data already written with `{field}` could no longer be read. | Add `migration {Type} { remove {field} }` to delete it, `rename {field} -> {new}` if you renamed it, or bump the major version. |
| type-changed (D-MIGRATE2E) | The published record `{Type}` changed `{field}` from `{Old}` to `{New}`, with no migration to bridge it. | Old data stored at the previous type could no longer be read. | Add `migration {Type} { change {field}: {Old} -> {New} via { (old) => … } }`, or bump the major version. |
| change with no converter (D-MIGRATE2B) | The `change {field}: {Old} -> {New}` migration on `{Type}` has no converter. | A type change needs a way to turn an old value into a new one — old data on disk is read through it. | Add an inline `via { (old) => … }`, or declare `impl {Old} -> {New} { … }` in scope. |
| added (D-MIGRATE2A) | The published record `{Type}` added `{field}`, but old data has no value for it. | Records already written without this field can't be read unless there's a default to fill it. | Add `migration {Type} { add {field}: {Type} = {default} }`, or bump the major version. |
| invalid op | A `remove`/`add`/`change` op contradicts the real shape (removes a field that still exists, adds one that already existed, or names a field in neither shape). | A migration op must reference a real shape difference. | Fix or delete the offending op. |

D-MIGRATE2B converter resolution for `change`: (1) the inline `via { … }`, else
(2) an `impl Old -> New` in scope (the same surface as D-ERR-CONV), else (3) E0910.
D-MIGRATE2F: reordering fields is keyed by name, never a breaking change — no op,
no error. E0910 checks intent; the runtime data conversion (reading old records
through the declared converter/default at decode time) is the D-MIGRATE4 chain
(spec.md "Runtime migration chain") and reuses ordinary decode errors — no new
diagnostic.

### E0911 — Unknown migration verb

| Case | What | Why | Fix |
|------|------|-----|-----|
| `drop` (D-MIGRATE2D) | `drop` isn't a migration verb — use `remove`. | A migration deletes a field with `remove`; `drop` is not a Jet keyword here. | Write `remove <field>`. |
| `reorder` (D-MIGRATE2F) | `reorder` isn't a migration verb — field order isn't a breaking change. | A `@PublishedSchema` record is keyed by field name, so reordering is safe. | Delete the `reorder` line; write the fields in any order. |
| other | `{op}` isn't a known migration verb. | A migration block contains `rename`, `add`, `remove`, or `change` operations. | Use one of those four verbs. |

### E0914 — Unknown interpolation selector (D-DISPLAYDBG2)

| What | Why | Fix |
|------|-----|-----|
| Unknown interpolation selector `@…`. | String interpolation supports a closed selector set — only `@Debug` is defined today. | Write `{value@Debug}` for debug formatting, or `{value}` for display. |

### E0915 — No Display implementation (D-DISPLAY-SHAPE)

| What | Why | Fix |
|------|-----|-----|
| `` `{type}` has no `Display` implementation ``. | Bare `{value}` interpolation calls `Display` — there is no default for user types. | Add `impl {type}.Display { fn display(self) -> String { … } }`, or use `{value@Debug}` for debug output. |

### E0916 — Debug auto-derive blocked (D-DEBUG-REDACT)

*Defined in sema; not yet emitted — reserved for when auto-derived `Debug` must reject a non-debuggable field type.*

| What | Why | Fix |
|------|-----|-----|
| `` `{type}` can't auto-derive `Debug` because field `{field}` isn't debuggable ``. | Auto-derived `Debug` requires every non-`@[Redact]` field to be debuggable. | Mark `{field}` with `@[Redact]`, change its type, or implement `Debug` manually for `{type}`. |

### E0917 — `@InlineAlways` self-recursive (D-METHODMACRO1)

`@InlineAlways` is a checked promise, not a hint (`@Inline` is the hint): if the
compiler can't literally inline every call, that's a compile error naming why,
never a silent miss. A function that calls itself has no fixed expansion.

| What | Why | Fix |
|------|-----|-----|
| `` `{name}` calls itself, so `@InlineAlways` cannot expand it ``. | Inlining a recursive call would either loop forever at compile time or require an artificial depth cutoff — neither is a real inline. | Drop `@InlineAlways` (use `@Inline` as a hint), or restructure the function to be non-recursive. |

Coverage: direct self-recursion only (a function calling itself by name).
Mutual recursion between two `@InlineAlways` functions is not checked.

### E0918 — `@InlineAlways` address taken (D-METHODMACRO1)

| What | Why | Fix |
|------|-----|-----|
| `` `{name}` cannot be inlined: its address is taken ``. | `@InlineAlways` promises every call to `{name}` expands in place — but `{name}` is also used as a plain value somewhere (stored, returned, or passed as a callback), and a value needs a real function to point at. | Drop `@InlineAlways`, or call `{name}` directly instead of through a value. |

Methods can't trigger E0918 — Jet's grammar has no way to read a method's bare
name as a value, so this only ever fires for top-level functions.

### E0919 — `@InlineAlways` too large (D-METHODMACRO1)

| What | Why | Fix |
|------|-----|-----|
| `` `{name}` is too large for `@InlineAlways` ``. | Its body has `{n}` statements — over the 40-statement ceiling `@InlineAlways` enforces so a promised inline doesn't quietly bloat every call site. | Drop `@InlineAlways` (use `@Inline` as a hint the compiler is free to ignore), or split the function so the hot part is small enough to inline. |

The ceiling is a statement count (`INLINE_ALWAYS_MAX_STMTS = 40` in
`crates/jet-sema/src/Sema/CheckerInline.rs`), counted transitively through
nested blocks (`if`/`loop`/dispatch/etc.) but not through a nested lambda
literal's body (a separate closure, not inline text of the function).

### E0920 — conflicting `@Inline`/`@InlineAlways` markers (D-METHODMACRO1)

| What | Why | Fix |
|------|-----|-----|
| a function can't be both `@Inline` and `@InlineAlways`. | `@Inline` is a soft hint the compiler may ignore; `@InlineAlways` is a checked promise it must honor or reject — one declaration can't carry both meanings. | Keep one: `@Inline` to suggest inlining, `@InlineAlways` to require it. |

### E0921 — `policy no_alloc` floor violation (D-MEM1/S7, D-NOALLOC-SEM1)

`policy no_alloc` is a module-level allocation floor (a bare item at the top
of a file, like `use`). The check is **local only**: it flags an
allocation-shaped expression written directly in the policy'd module's own
function bodies, and never follows a call into another function — a helper
that itself allocates is that helper's own module's problem, not this one's.

| What | Why | Fix |
|------|-----|-----|
| string interpolation allocates a new `String`. | any `"{…}"` hole builds a fresh `String` at runtime — a plain literal with no hole is one constant piece of text, not concatenation/interpolation, so it isn't flagged. | Avoid the allocation here, or move this code out of the `policy no_alloc` module. |
| `` `.push`/`.insert` may allocate to grow this collection's heap allocation. `` | capacity headroom isn't provable at compile time in general, so every call of this shape is flagged, full stop. | Avoid the allocation here, or move this code out of the `policy no_alloc` module. |
| constructing `{type}` here allocates — it owns heap data. | a struct/enum literal for a type with a `String`/`[T]`/`[K,V]`/`Shared<T>`/`Pool<T>` field (directly or transitively) allocates when built. | Avoid the allocation here, or move this code out of the `policy no_alloc` module. |
| `` `copy` of `{type}` allocates — it owns heap data. `` | `copy` duplicates a value; duplicating a heap-owning type allocates a new one. | Avoid the allocation here, or move this code out of the `policy no_alloc` module. |

Coverage note: a bare list/map/string literal that isn't one of the four
shapes above (e.g. `xs := [1, 2, 3]`) is not checked — only the ratified
denylist is enforced; extending it is a future policy-list ballot, not a
silent expansion here.

## Process for a new diagnostic

1. Claim the next code here. 2. Write what/why/fix per the voice rules.
3. Add a tests/ui fixture + snapshot. 4. Ship. A diagnostic without a
snapshot test does not exist (invariant I4).
