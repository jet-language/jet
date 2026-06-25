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
| E0002 | jet   | unterminated text literal, interpolation, or block comment |
| E0003 | parse | expected X, found Y                       |
| E0004 | parse | *retired in M1* (was: parameters staged)  |
| E0005 | parse | *retired in M1* (was: variables staged)   |
| E0006 | parse | *retired in M4* (was: `?` staged)         |
| E0007 | jet   | integer too large for 64 bits             |
| E0008 | parse | teaching: `def`/`func` → `fn` (S14)       |
| E0009 | parse | teaching: `let`/`let mut` → `val`/`var`   |
| E0010 | parse | teaching: `set` → `val`                   |
| E0011 | sema  | *retired in M10* (was: `println` → `print`) |
| E0012 | parse | teaching: `and`/`not` → `&&`/`!` |
| E0013 | parse | teaching: `Text` → `String`               |
| E0014 | parse | teaching: `try` → `?` (M4 — real feature)   |
| E0015 | parse | teaching: `import` → `use` (S16, D-S16-USE) |
| E0016 | parse | teaching: `match` → `when` (S24)          |
| E0017 | parse | teaching: `read` → default parameter access (S10) |
| E0018 | parse | teaching: `write` → `~` sigil (S10, D-CAP7) |
| E0019 | parse | *retired in M6* (was: `import` staged; S16 shipped) |
| E0020 | parse | teaching: `None`/`Some`/… → `null`/`value` (S32) |
| E0021 | parse | teaching: `class` → `struct` (S29)              |
| E0022 | parse | teaching: `trait`/`interface` staged → M9 (S28) |
| E0023 | parse | teaching: `case`/`default` → `when` arm syntax (S24) |
| E0024 | parse | teaching: `catch`/`except` → `or` / `== err` (M4) |
| E0025 | parse | teaching: `unwrap`/`expect` → `or panic(…)` (M4) |
| E0026 | parse | teaching: `throw`/`raise` → `err(…)` (M4) |
| E0027 | parse | teaching: `append` → `push`               |
| E0028 | parse | teaching: `Vec`/`dict` → `List`/`Map`     |
| E0029 | parse | two capability markers on one parameter (D-CAP7) |
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
| E0040 | sema  | teaching: `async`/`await` → blocking tasks/channels |
| E0041 | sema  | teaching: `Mutex`/`lock` → channels |
| E0043 | jet   | teaching: `jet install` → `jet fetch` |
| E0044 | parse | teaching: `switch` → `when` (S24, D-SG1)  |
| E0045 | parse | teaching: `or` fallback → `??` (S71, D-SG6) |
| E0046 | parse | `?.` optional chaining reaches fields, not methods (S71) |
| E0047 | type | `?.` left side must be optional `T?` (S71, D-SG6) |
| E0048 | parse | teaching: positional tuples → named members (S73, D-SG7) |
| E0049 | parse | teaching: `.0` field access → named members (S73, D-SG7) |
| E0050 | parse | teaching: `while` → `loop cond { }` (S19-amend) |
| E0051 | parse | teaching: `for x in` → `loop x in` (S19-amend) |
| E0052 | parse | teaching: bare `test "name" { }` → `#Test "name" { }` (D-CASING1 follow-on) |
| E0053 | parse | teaching: bare `pure fn` → `#Pure fn` (D-CASING1 follow-on) |
| E0054 | parse | teaching: bare `todo` → `#Todo` (D-CASING1 follow-on) |
| E0055 | parse | teaching: `#Audit("…")` retired → reason is now the argument of `#Unsafe("…")` (D-UNSAFE2) |
| E0056 | parse | teaching: `mut` capability keyword → `~` sigil (D-CAP7) |
| E0057 | parse | teaching: `take` capability keyword → `^` sigil (D-CAP7) |
| E0058 | parse | teaching: `view` return keyword → `&` sigil (D-CAP7) |
| E0984 | parse | teaching: `when` → `if subject { arm -> body }` (D-IF1) |
| E0985 | parse | teaching: `val`/`var` keyword → `name @=`/`name :=` sigil (D-BIND2) |
| E0986 | parse | `-> Type`/`{` split from the closing `)` (S6-R layout) |
| E0991 | parse | teaching: `::` retired immutable-binding sigil → `@=` (D-BIND2) |
| E0992 | parse | teaching: implicit dispatch — a multi-arm `if` needs `==` between the subject and `{` (D-IF3) |
| E0993 | parse | a predicate/Bool arm head in `if subject == { … }` — use a range arm or a boolean `if`/`else if` (D-IF3 Q4) |
| E0994 | parse | teaching: a redundant `subject ==` on an arm head — the `if`'s `==` already applies it (D-IF3) |
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
| E0987 | sema  | `break @name`/`continue @name` names a loop label not in scope (D-LABEL1) |
| E0988 | parse | a `@name` loop label is not followed by `loop` (D-LABEL1) |
| E0989 | sema  | `comptime if` condition is not a comptime expression (D-WHEN1) |
| E0990 | parse | `@` used as attribute prefix; teaching error — use `#` (D-ATTR1) |
| E0116 | sema  | valueless call used as a value            |
| E0118 | sema  | name already taken (no shadowing)         |
| E0119 | sema  | unknown type name                         |
| E0120 | sema  | moving/returning a parameter without move (`^`) access |
| E0121 | sema  | value used after it was given away        |
| E0122 | sema  | `main` with parameters or a return type   |
| E0123 | sema  | `for` range `step` must be a positive Int (S22, D-SG8) |
| E0124 | sema  | `if`-expression branches produce different types (S68, D-SG2) |
| E0125 | sema  | call-site label mismatch: transposed or unknown label (D-NARG-D4) |
| E0126 | sema  | default expression references a later parameter (D-NARG-D2) |
| E0127 | sema  | arithmetic on a distinct type without `@Numeric` (D-DIST3) |
| E0128 | sema  | implicit coercion between a distinct type and its base (D-DIST3) |
| E0129 | sema  | distinct-over-distinct: base type is itself a distinct type (D-DIST1) |
| E0140 | sema  | `#SingleUse` value dropped without being consumed at scope end (D-LIN1) |
| E0141 | sema  | `#SingleUse` value consumed on only one `if` branch (D-LIN1) |
| E0142 | sema  | `#SingleUse` value lent/shared instead of moved (D-LIN1) |
| E0201 | sema  | `take` (`^`) required; value can't be copied |
| E0202 | sema  | `mut` (`~`) required at call site — write access not granted |
| E0203 | sema  | `take` on a non-consuming parameter       |
| E0204 | sema  | same value used while `mut` is active in one call |
| E0205 | sema  | `self.field = v` without write access (`~`) on the receiver (D-MUTSELF1) |
| E0206 | sema  | `view` return can't point at this value   |
| E0207 | sema  | multiple unlabeled `ref` fields           |
| E0208 | sema  | raw pointer op outside `#Unsafe`: postfix `p.*` deref or prefix `*x` raw-of (D-CAP9) |
| E0210 | parse | teaching: `Ptr<T>` is the deprecated alias → write `*T` (D-CAP9) |
| L0201 | sema  | implicit `.clone()` at call site — fired only when the value is dead after the call (D-L0201 liveness gate) |
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
| E0313 | sema  | destructuring target's shape doesn't match the value (S74) |
| E0315 | sema  | list-pattern arity ≠ a known-length list literal (S74) |
| E0316 | sema  | D-PATR: range pattern on non-integer field, or `lo > hi` (empty range) |
| E0317 | sema  | D-PATO: or-pattern alternatives bind different names or types |
| E0318 | parse | C25/D-RANGE2: `..=` in an arm head — Jet's `..` is already inclusive; write `lo..hi` |
| E0319 | parse | C25/D-RANGE2: `step` in an arm head — `step` is a loop modifier, not an arm construct |
| L0301 | sema  | unreachable `switch` pattern arm (lint)   |
| E0401 | sema  | fallible value used where plain `T` expected |
| E0402 | sema  | fallible call ignored as a statement      |
| E0403 | sema  | `?` error type / return context mismatch  |
| E0404 | sema  | `ok`/`err` need a fallible context        |
| E0405 | sema  | `??` fallback type/`return` mismatch       |
| E0406 | parse | old `Result<T, E>` fallible type syntax   |
| E0420 | sema  | `#Uninit` binding read before it is given a value (D-UNINIT1) |
| E0421 | parse | `#Uninit` binding needs a type annotation (D-UNINIT1) |
| E0422 | parse | `#Uninit` binding cannot have an initializer (D-UNINIT1) |
| E0423 | sema  | `#Uninit` binding's type is not plain data (D-UNINIT1) |
| E0424 | sema  | `#Uninit` used without `use core.mem` (D-UNINIT1) |
| E0501 | sema  | empty `[]` / `[:]` needs a context type   |
| E0502 | sema  | type can't be a map key                   |
| E0503 | sema  | strings aren't indexable with `[ ]`       |
| E0504 | sema  | mixed-type list/map literal               |
| E0505 | sema  | wrong index/key type or bad slice target  |
| E0507 | sema  | collection changed while `for` reads it   |
| L0501 | sema  | slice copy inside a loop (lint)           |
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
| E0711 | sema  | the capability handle bound by a `#grant(…)` region escapes its scope — returned, stored, or captured (D-SCAP1) |
| E0712 | sema  | an effect used inside a `#grant(…)` region has no capability — it isn't in the grant's list (D-SCAP1) |
| E0721 | sema  | an untrusted (`#Tainted`) value reaches a sink effect (`Db`/`Exec`/`Net`) without passing through a `#Sanitizer fn` (D-TAINT1) |
| E0731 | sema  | a `tag` is used where dispatch/methods are expected — `derive`d, or implemented/used as a trait (D-QUAL2) |
| E0732 | sema  | a method is declared in a `tag` body, but tags have no methods (D-QUAL2) |
| E0745 | sema  | a `#Pure fn` also carries a non-empty `#(…)` effect list — a contradiction (D-EFF1) |
| E0746 | sema  | an irreversible effect (Net/Fs/Exec) used directly inside a `#Transact { … }` block — can't be rolled back (D-TXN2) |
| E0747 | sema  | a callback argument exceeds its parameter's effect bound (`#Pure fn(…)` / `#(E) fn(…)`) (D-EFF2) |
| E0748 | sema  | `#(via f)` names a non-existent parameter, or one that isn't a function type (D-EFF2) |
| E0760 | parser | `#Context` field uses `=` instead of `:` (D-CTX1, S17) |
| E0761 | parser | unknown `#Context` field name (v1 allows only `allocator`, `logger`) |
| E0762 | sema   | `allocator` field given a non-allocator type (D-CTX1, Q1=A2) |
| E3001 | runtime | panic report with Jet source location, function name, source-line context box, and (in debug builds) safe local values (E2-M12, D-OBS1/D-OBS2) |
| E3002 | runtime | error-return trace entry on a `?`-propagated failure, Zig-style (E2-M12, D-OBS1) |
| E3101 | sema  | low-level op (`from_addr`/`volatile_read`/…) used outside an `#Unsafe("…")` block (D-UNSAFE2) |
| E3102 | sema  | `core.mem` item (`Ptr`/`volatile_read`/allocator) named without `use core.mem` (S58) |
| E3103 | sema  | `#Unsafe fn` called without an enclosing `#Unsafe("…")` block (D-UNSAFE2) |
| E3104 | sema  | value allocated in an arena used after `arena.reset()` or `arena.free()` (D-ALLOC-D) |
| L3101 | sema  | `#Unsafe` block missing its reason argument — write `#Unsafe("…") { … }` (D-UNSAFE2) |
| E3201 | jet   | C library `<lib>` not found (hangar + pkg-config) |
| E3202 | sema  | pointer/gated type crosses C boundary outside `#Unsafe` / `core.mem` |
| E3203 | sema  | non-C-ABI type in `@extern` / `@bindgen` fn signature |
| E3204 | sema  | two C `use` forms for the same lib in one file |
| E3205 | sema  | overlay symbol clashes with bindgen (incompatible signature) |
| E3206 | parse | user declared reserved `__bindgen__` segment |
| E3207 | parse | `@bindgen` outside generated `.jet/bindings/c/` file |
| E3208 | jet   | `jet bind` / header translation failed |
| E3209 | jet   | linker couldn't find a declared C library at link time |
| E3210 | jet   | C library auto-provision from nixpkgs failed |
| E3301 | sema  | OS-dependent std API called in a `--freestanding` build |
| E3302 | jet   | target triple unknown or toolchain component missing |
| E3303 | sema  | freestanding build allocates memory with no global allocator |
| E3401 | sema  | impure call inside a `#Pure fn` / pure-eval context (call-trace path) |
| E3402 | sema  | package build attempted ambient I/O or network (names the call) |
| E3403 | sema  | non-deterministic construct in pure evaluation (e.g. time/random) |
| E1801 | repl  | per-input fuel cap hit — snippet ran more than ~10M interpreter steps |
| E1802 | repl  | hard-rejected feature in the REPL (FFI, tasks, `#Unsafe`, OS-level APIs) |
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
| E0910 | sema  | `#PublishedSchema` struct made a breaking shape change (drop / type-change / add-without-default) with no migration to bridge it, or a declared migration op is nonsensical |
| E0911 | parse | migration block uses an unknown verb (`drop`→`remove`, `reorder` not needed) |
| E0951 | sema  | comptime code reaches an impure operation (shows call path) |
| E0952 | sema  | comptime budget exhausted (fuel) |
| E0953 | sema  | comptime panic = user-authored compile error (message verbatim) |
| E0954 | parse | teaching: `comptime val`/`comptime var`/`const` → `comptime x = …` |
| E0955 | sema  | `embed_file`/`embed_bytes`: missing / unreadable file (`embed_file` also: not UTF-8) |
| E0956 | sema  | construct not yet supported in comptime evaluation |
| E0957 | sema  | `embed_file`/`embed_bytes` path not a literal, absolute, or escaping via `..` |
| E0958 | sema  | an I/O Core call (`core.fs`/`io`/`env`/`net`) can't run at comptime (D-CTCORE1) |
| E0960 | parse | module contribution names a non-reserved namespace (U3: `env`/`system`/`image`) |
| E0961 | sema  | fan-out callee is not callable with exactly one argument (S75) |
| E0962 | sema  | fan-out item doesn't fit the parameter type (S75) |
| E0963 | sema  | positional destructure count ≠ fixed-size list length (S76) |
| E0964 | sema  | length-changing op (`push`/`pop`/`insert`) on a fixed-size `[T#N]` (S76) |
| E0965 | sema  | compile-time index out of range on `[T#N]` (S76) |
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
| E0976 | jetpack | an `Image` `format:` isn't `iso` / `qcow` / `raw` (U14) |
| E0977 | jetpack | an `Image` has no `from`, or restates an inherited field (U14) |
| E0978 | jetpack | an `Image` `from:` references an unknown `System` (U14) |
| E0979 | jetpack | a `jetpack os` target has no `@host` selector (U16) |
| E0980 | jetpack | a `jetpack os` `@host` names a `System` the config doesn't define (U16) |
| E0981 | jetpack | a `jetpack os` config file doesn't exist (U16) |
| E0982 | jetpack | `use <pkg>` names an `executable` package — executables go on PATH, not `use` (U17) |
| E0983 | jetpack | `use <pkg>` names a declared `library` dependency that hasn't been realized yet (U17) |
| E1001 | jet   | unknown core module |
| E1002 | jet   | local module shadows reserved first-party root/name |
| E1003 | sema  | integer literal out of range for its width |
| E1004 | sema  | unknown item in core module |
| E1005 | sema  | overflow opt-in not wrapping a single integer op |
| E1301 | sema  | `ArgsSpec.flag` or `ParsedArgs.flag` called with wrong arity (D-ARGS1) |
| E1302 | sema  | `ArgsSpec.option` or `ParsedArgs.option` called with wrong arity (D-ARGS1) |
| E1303 | sema  | `ArgsSpec.positional` or `ParsedArgs.positional` called with wrong arity (D-ARGS1) |
| E1304 | sema  | `ArgsSpec.parse` called with wrong arity (D-ARGS1) |
| E1101 | sema  | task capture needs ownership              |
| E1102 | sema  | value crossing task/channel boundary is not sendable |
| E1103 | sema  | `.detach()` called on a task that had a sendability error at spawn (D-DETACH1) |
| E1104 | sema  | `#layout(c)` struct contains a growable field (D-REPRC1) |
| E1105 | sema  | `#layout(…)` variant not yet supported (D-REPRC1 reserved) |
| E1106 | sema  | `.detach()` called on a task that captured a `view` borrow (D-DETACH1) — the view may outlive the caller |
| L1101 | sema  | Task value dropped without `.join()` or `.detach()`  |
| E2301 | sema  | returned `view` outlives the local that owns it (E2-M5) |
| E2302 | sema  | stored `ref` field would point at something that dies first (E2-M5) |
| E2303 | sema  | `ref`/`view` crosses a task/channel boundary (E2-M5; emitted as E1102) |
| E2304 | sema  | an indexed or sliced piece can't be handed back as a `view` (E2-M5) |
| L2301 | sema  | this return borrows; here is its source (advisory, E2-M5) |
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
| E1216 | jet   | a `targets:` block has an unknown field, or `api:` is not `stable`/`explicit` (D-TGT3/D-CAP4) |
| E2001 | jet   | `pkg.jet` requests an edition this toolchain can't provide (E2-M2, D-REL3) |
| E2002 | jet   | a deprecated item is used past its migration window (E2-M2, D-REL5) |
| E2101 | jet   | unknown subcommand on the command line, with a "did you mean" (E2-M3, D-DX) |
| E2102 | jet   | unknown or ambiguous flag on the command line, with a suggestion (E2-M3, D-DX) |
| E2201 | interp | `jet dev` can't interpret a feature (task/FFI/`#Unsafe`/native std); names it and `jet build`/`jet run` (E2-M4, D-DEV1) |
| E2202 | interp | `jet dev` interpreter step budget exhausted — likely an unbounded loop (E2-M4) |
| E2210 | interp | a `jet dev`/`jet serve` edit changed a type surface, so the dev loop restarts instead of swapping (c77, D-HOTSWAP1) |
| L2001 | jet   | a deprecated item still compiles but should be migrated; suggests `jet fix` (E2-M2, D-REL5) |
| L2101 | jet   | `jet doctor` advisory: a rustc / cache / PATH problem with a fix (E2-M3, D-DX2) |
| E2701 | runtime | malformed input to a ring library parse function — row/line number and detail (E2-M9) |
| E2702 | sema  | crypto API misuse at the boundary — reserved for future statically-detectable crypto errors (E2-M9, D-LR3) |
| L2701 | sema  | advisory: regex pattern may catastrophically backtrack; suggest an anchor (E2-M9) |

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

## Command-line diagnostics (E2-M3)

These come from the CLI driver (`Source/main.rs`, `Source/CLI.rs`), not the language
front end. They use the same `Error [E####]` / `Why:` / `Fix:` voice so the
command line teaches the same way the compiler does. The "did you mean"
suggestion reuses the edit-distance muscle behind the S14 teaching errors.

| Code | What | Why | Fix |
|------|------|-----|-----|
| E2101 | That isn't a Jet command. | The first word after `jet` must be a known command (like `run`, `check`, or `test`) or an installed `jet-<name>` plugin on your PATH. | Run `jet help` to see the commands, or use the closest match named in the error. |
| E2102 | That flag isn't one this command understands. | Each command accepts a fixed set of flags; an unknown or half-typed flag is usually a typo or a flag meant for a different command. | Drop the flag, or use the closest match named in the error; `jet help` lists the flags. |

| Code | What | Why | Fix |
|------|------|-----|-----|
| L2101 | `jet doctor` found something in your environment that will bite you later. | Jet leans on a hidden toolchain (rustc, a build cache, your PATH); when one is missing or stale, builds fail with confusing errors far from the cause. | Apply the fix named in the report — many are auto-fixable with `jet doctor --fix`. |

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
| E0964 | A length-changing method (`push`, `pop`, `insert`, `remove`, `clear`) was called on a fixed-size `[T#N]`. | The length of `[T#N]` is fixed at compile time and cannot change at runtime. | If you need a growable list, widen the binding: `var r: [T] = ...`. |
| E0965 | A literal index is out of range for a `[T#N]` at compile time. | The valid indexes for `[T#N]` are 0 through N−1; anything outside that range would panic at runtime. | Use an index in the valid range, or check at runtime with a condition. |

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
| E0968 | A `sources:` entry's value isn't a `provider@target` ref. | A named source resolves to an upstream written as `provider@target` (U6), e.g. `github@NixOS/nixpkgs/nixos-24.05`; the resolver needs the provider and target to realize it. | Write the ref as `provider@target`, e.g. `default: github@owner/repo/rev`. |
| E0969 | An `imports:` directive isn't `find("<dir>")` with a single literal path. | Imports auto-discover a directory of modules (U4); the only directive is `find` with one string-literal path, so a non-`find` call or an interpolated/missing argument can't be walked. | Write `imports: find("./modules")`. |
| E0970 | `imports: find("<dir>")` points at a directory that doesn't exist. | `find` walks that directory for `.jet` modules (U4); it must exist relative to the file that declares it, or there is nothing to discover. | Create the directory, or fix the path so it points at your modules folder. |
| E0971 | A module discovered by `find(…)` has its own `imports:`. | The liftability law (U4): modules contribute to the merged whole, they never import each other — nesting `find` would make composition explode and break "drop a file in." | Remove the `imports:` from the discovered module; declare all `find(…)` directives in the top-level env.jet. |
| E0972 | A `System` / `Image` / `Service` record has a field it doesn't define. | Each of these records has a fixed set of fields (U11/U14); an unknown field is usually a typo or a value that belongs elsewhere. | Remove the field, or use one of the known fields named in the error. |
| E0973 | A `target` (or cross-compile platform) names a platform Jet doesn't know. | U13: a `target` is a typed platform value, not quoted text — it must be `linux.x64` or `linux.arm64`, so it type-checks and LSP-completes. | Write `target: linux.x64` or `target: linux.arm64`. |
| E0974 | A `System` has no `target`. | U11: every machine names the platform it runs on with a typed `target`. | Add `target: linux.x64` (or `linux.arm64`). |
| E0975 | A `Service` has no `enable`, or its `enable` isn't a yes/no value. | U12: every `Service` is an open record whose required first field is `enable: Bool`. | Add `enable: true` (or `false`) to the service. |
| E0976 | An `Image` `format:` isn't one of the three disk-image formats. | U14: an image is built as `iso`, `qcow`, or `raw` (default `iso`). | Write `format: iso`, `format: qcow`, or `format: raw`. |
| E0977 | An `Image` has no `from`, or restates a field it inherits from its system. | U14: an image is built `from: system.<name>` and inherits that system's `packages`/`services`/`options` — they are written once on the system. | Add `from: system.<name>`, or remove the inherited field (only an explicit `target:` may be restated, for cross-compiling). |
| E0978 | An `Image` `from:` references a system no contribution defines. | U14: `from: system.<name>` must name a `System` defined by some module, because the image inherits its target, packages, services, and options. | Define `system.<name>: { … }`, or point `from:` at an existing system. |
| E0979 | A `jetpack os` target was given with no `@host` selector. | U16: `jetpack os <verb>` takes `[<config-path>]@<host>`; the `@host` segment selects which `System` in the config to apply, and it is required. | Write `jetpack os switch @<host>` (default config) or `jetpack os switch ./config.jet@<host>`. |
| E0980 | A `jetpack os` `@host` selector names a system the config doesn't define. | U16: the `@host` selector picks which `System` to apply; it must name a `system.<name>:` contribution the config defines. | Define `system.<host>: { … }`, or select one of the systems the config already defines. |
| E0981 | The `jetpack os` config file (named, or the default `~/.jet/config.jet`) doesn't exist. | U16: `jetpack os <verb>` loads `[<config-path>]@<host>`; with no path prefix it defaults to `~/.jet/config.jet`. | Create the config file, or pass an explicit path before the `@`, e.g. `jetpack os switch ./config.jet@<host>`. |
| E0982 | `use <pkg>` named a package that is realized as an `executable`. | U17: one import concept (`use`) covers files, modules, and `library` packages; an `executable` package installs a binary on your PATH — you run it, you don't import its code. | Remove the `use`, and run the executable's binary instead; or, if you meant to import its code, change the package to `library` in `pkg.jet`. |
| E0983 | `use <pkg>` named a `library` dependency the project declares but that hasn't been realized (its source isn't staged in the shared hangar store, and isn't on disk as a path dep). | U17: a `library` is consumed with the ordinary `use` form only after it is realized — `jet build`/`run` never realize on demand, keeping them offline and deterministic (the same flow as pre-fetched deps). | Run `jetpack build` to realize the library into the hangar, then `use <pkg>;` resolves it. |

## Concurrency diagnostics

| Code | What | Why | Fix |
|------|------|-----|-----|
| E1101 | A spawned task captures a value it does not own. | Tasks run concurrently and may outlive the scope that created them; shared `var` state is not allowed. | Give the task its own copy or use `take(name)` so the task owns the value; use a channel to send results back. |
| E1102 | A value crossing `tasks.spawn` or `Sender.send` is not sendable. | Task and channel boundaries move owned data between threads; shared views (`&`), `ref`-holding structs, trait values, and non-`take`n closures cannot cross. | Send plain owned data, remove the shared-view field, or hand a closure over with `take`. |
| E1103 | `.detach()` called on a task that had a sendability error (E1102) at spawn. | A detached task runs unsupervised and may outlive the caller; a task that already has sendability problems is doubly unsafe to detach. | Fix the E1102 error at the spawn site first; once the task only holds owned data, `.detach()` is safe. |
| E1106 | `.detach()` called on a task that captured a `view` borrow. | A detached task runs unsupervised and may outlive the borrow's source; the captured `view` would dangle. | Pass an owned `copy` or `share` to the task instead of a `view`. |
| E1104 | `#layout(c)` struct contains a field whose type is growable (`[T]`, `Map`, or `String`). | Growable Rust heap types don't have a stable C layout — the raw data pointer and length live at unpredictable offsets. | Use a fixed-size array `[T#N]` instead, or remove `#layout(c)` if C interop is not required. |
| E1105 | `#layout(packed)`, `#layout(align(N))`, or `#layout(columnar)` written on a struct. | Only `#layout(c)` is implemented in this release; the other variants are reserved for future milestones. | Use `#layout(c)` for C-compatible layout, or omit `#layout` for the default. |
| L1101 | A `Task` is dropped without `.join()` or `.detach()`. | The program may end before that task finishes. | Call `.join()` to wait for the result, or `.detach()` if fire-and-forget is intentional. |
| E0040 | `async` or `await` was written. | Jet uses blocking tasks and channels rather than async syntax. | Use `core.tasks as tasks` and call `tasks.spawn(() => work())`. |
| E0041 | `Mutex`, `RwLock`, `mutex`, or `lock` was written. | Jet avoids shared mutable state; tasks communicate by sending messages. | Use `tasks.channel()`, `sender.send`, and `channel.receive`. |

## Tier-2 reference diagnostics (E2-M5, S10 `view`/`ref`)

These harden the ownership checker around borrowed returns and stored
references. They never mention lifetimes; they speak in Jet words — *what
owns this* and *how long can this view live*. E2301/E2302 supplement the
tier-1 reference codes (E0206 bare-local `view` return, E0207 unlabeled
`ref` fields); they do not replace them. E2303 is the reference-specific
name for the task/channel rule — that situation is **reported once, as
E1102** (a `view`/`ref` value is unsendable); E2303 exists so `jet explain
E2303` points there and the soundness matrix has a named cell.

| Code | What | Why | Fix |
|------|------|-----|-----|
| E2301 | A `-> view` function returns a view into a field of a value this function owns. | The owning local is made inside the call and freed when it returns, so a view into its fields would outlive what owns it — there'd be nothing left to look at. | Return an owned copy (`.clone()` the field into an owned return type), or accept the source as a parameter so the caller keeps owning it. |
| E2302 | A `ref` field is filled from a value that won't outlive the struct. | A `ref` field stores a view, not its own copy, so its source must outlive the struct; a local or a fresh literal lives only as long as the call. | Store an owned value (drop `ref` so the struct keeps its own copy, or `.clone()` into it), or fill the `ref` from a parameter the caller keeps owning. |
| E2303 | A `view` or a `ref`-holding struct crosses a `tasks.spawn` or `Sender.send` boundary. | A shared view (`&`) points into something another scope owns; a task or channel moves owned data between threads, so a view can't cross without ownership. Reported as **E1102** (the unsendable-value rule), not separately, so one situation gives one error. | Send plain owned data, remove the shared-view field before crossing, or rebuild the value as an owned copy. |
| E2304 | A `-> view` function returns an indexed or sliced piece of a value (e.g. `text[0..2]` or `items[i]`). | Indexing or slicing builds a fresh, owned piece — there's no longer-lived value for a view to point at, so the piece would vanish the moment the function returns. A `view` into a whole *field* of a parameter is fine (the caller still owns the field); only the freshly-cut piece is the problem. | Return the piece owned (drop `view`; the caller keeps its own copy), or hand back a whole field with `view` and let the caller index it. |
| L2301 | This return hands back a borrowed `view`; the advisory names the source it borrows. | Borrowed returns are easy to miss; surfacing the source (the parameter or value the view points into) makes the borrow visible without reading the signature. This is an inlay/advisory hint, on by default (D-REF3). | No action needed — it's informational. To return owned data instead, drop `view` and `.clone()` the value. |

## Library authoring diagnostics (E2-M6)

S61 (argument labels/defaults), S62 (trait delegation), D-LIB3 (Fallible `?`
conversion), and S77 (field punning) introduce these codes. E24xx is the
block reserved for M6.

| Code | What | Why | Fix |
|------|------|-----|-----|
| E2401 | The delegation target `{field}` doesn't implement `{trait}`, or the type has no field named `{field}`. | `impl Type: Trait using field` forwards every `Trait` method to the `field` field; if that field's type hasn't implemented `Trait`, there's nothing to forward to. | Implement `impl FieldType: Trait` on the field's type, or choose a different field that does implement `Trait`. If the field doesn't exist, add `{field}: FieldType` to the struct. |
| E2402 | `?` can't convert `{err}` into `Error` — `{err}` has no `Fallible` implementation. | `?` inside a `T ? Error` function can propagate errors whose type implements `Fallible`; the `to_error` method converts them. Without an impl, there's no path from `{err}` to `Error`. | Add `impl {err}: Fallible { fn to_error(self) -> Error { Error(str(self)) } }` (or a more descriptive conversion), or change the return type to `T ? {err}`. |
| E2403 | Field-pun name `{name}` is not in scope (or is not a field of `{type}`). | `Type { name }` is shorthand for `Type { name: name }` — it reads the local variable `name` and assigns it to the field of the same name. If no such local exists, or if `Type` has no field by that name, the shorthand is ambiguous. | Introduce a local `val name = …;` before the struct literal, or write the long form `Type { field_name: value }`. |
| E2404 | `` `?` can't turn a `{Source}` into a `{Target}` here ``. | `?` changes an error's type only when you've declared how via `impl Source -> Target { … }` (D-ERR-CONV); no such declaration exists for this pair. | Add `impl {Source} -> {Target} { … }` before the function that uses `?`. |
| E2405 | `impl {Source} -> {Target}` is already declared. | There can be at most one declared way to convert a `Source` error into a `Target`; the second block is rejected. | Remove one of the two `impl … -> …` blocks. |
| E2406 | Can't declare `impl {Source} -> {Target}` — neither type is defined in this program. | Error conversions obey the same orphan rule as trait impls (S28): at least one of `Source` or `Target` must be a type you defined, so conversions between two foreign types can't be added silently. | Define one of these types locally, or use `Fallible` (D-ERR2) if you don't own either type. |
| E2407 | `#[Rename(...)]` needs a string literal. | The wire key a `#[Codable]` field maps to is a constant string (D-SERDE5); a number or expression has no place on the wire. | Pass one quoted string — `#[Rename("wire_name")]`. |
| E2408 | `#[Flatten]` on `{field}` needs a struct-typed field. | Flatten splices another struct's keys into this object (D-SERDE5), so the field must itself be a `#[Codable]` struct — not a primitive, list, or map. | Give `{field}` a `#[Codable]` struct type, or drop `#[Flatten]`. |
| E2409 | `#[RenameAll({style})]` isn't a known casing style. | The wire-casing menu is the closed typed set `camel`/`snake`/`pascal`/`kebab`/`screaming` (D-SERDE3); anything else is rejected so a typo fails at compile time, not on the wire. | Pick one of `camel` / `snake` / `pascal` / `kebab` / `screaming`. |
| E2410 | `E2410: missing required field `{field}`` (runtime `DecodeError`). | Decoding into a `#[Codable]`/`#[Decode]` type found no wire value for a required field and the field has no `#[Default]` and isn't optional (D-SERDE5). | Mark the field optional (`T?`), give it `#[Default]`/`#[Default(value)]`, or fix the input so the key is present. Compose with `??` to supply a fallback. |
| E2411 | `{Type}` can't be serialized / decoded. | Only types that opt in with `#[Codable]`/`#[Encode]`/`#[Decode]` (and whose fields all have a wire form) can cross the wire; a field holding a non-Codable type, closure, or handle has nothing to (de)serialize (D-SERDE1). | Add `#[Codable]`/`#[Encode]`/`#[Decode]` to the offending type, or remove it from the encoded value (e.g. `#[Skip]`). |
| E2412 | `E2412: unknown field `{field}`` (runtime `DecodeError`). | The struct is marked `#[DenyUnknownFields]` (D-SERDE8) and the input carried a key the struct doesn't declare, so decoding fails fast instead of silently dropping it. | Remove `#[DenyUnknownFields]` to ignore extra keys (the lenient default), add the field, or fix the producer. |
| E2413 | `{Type}` is generic, so it can't derive `Encode`/`Decode` yet. | The built-in serde derive walks concrete fields; generic (de)serialization isn't built yet (D-SERDE1 scope). | Make `{Type}` concrete, or hand-write the (de)serialization for now. |
| L2401 | Public function `{fn}` has a positional `Bool` parameter `{param}`. | Positional booleans are easy to transpose: `connect(host, true, false)` is a guessing game. Labels (S61) make the intent clear at the call site. | Callers can use `{param}: true` to document intent; or give the parameter a default value so it can be omitted. No action required — this is advisory. |

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
| E2603 | Advisory `{advisory_id}` matches `{package}` `{version}`: {title}. | The advisory database flags this version as having a known vulnerability, exposed interface, or supply-chain risk. | Upgrade to `>= {fixed_version}` (or the version listed in the advisory). Run `jet audit --explain {advisory_id}` for details. |
| E2604 | Integrity check failed for `{package}` `{version}` — expected `{expected}`, got `{actual}`. | A fetched artifact's content hash differs from what the lockfile recorded. This means the artifact changed after it was locked — accidental or deliberate tampering. | Re-run `jet fetch` after removing the corrupt store entry (`jet gc --force`). If the problem persists, the upstream source may have been altered; audit the change before proceeding. |

## First-party ring library diagnostics (E2-M9, D-LR1–4)

Wave-1 ring packages (`jet.csv`, `jet.toml`, `jet.yaml`, `jet.log`, `jet.json`, `jet.time`, `jet.crypto`) are compiler-known modules — no external crates in `src/` (I6). E27xx is the block for M9.

| Code | What | Why | Fix |
|------|------|-----|-----|
| E2701 | `{parser}` found malformed input at row/line {n} — {detail}. | The ring library parse function encountered text it can't interpret: a missing delimiter, an unclosed quote, or an unexpected character. The row or line number points at the first offending record. | Fix the input at the location named, or validate it before parsing. |
| E2702 | Crypto API misuse: {detail}. | A `jet.crypto` call would use a key, nonce, or algorithm in an unsafe way — for example, an IV too short or a key length the algorithm doesn't accept. Reserved for future static checks. | Follow the fix named in the error; the `jet.crypto` API is intentionally restrictive to surface misuse. |
| L2701 | This regex pattern may catastrophically backtrack on certain inputs. | A regex with unbounded quantifiers nested inside another unbounded quantifier can run in exponential time on adversarial inputs, causing a denial-of-service. Reserved for future `jet.regex` patterns. | Anchor the pattern at the start (`^`) or end (`$`), or restructure it to avoid nested quantifiers. |

## Networking and services diagnostics (E2-M10, D-NET1–3)

`core.net` provides blocking TCP/UDP sockets; `jet.http` provides HTTP client and
server built on top. E28xx is the block for M10.

| Code | What | Why | Fix |
|------|------|-----|-----|
| E2801 | {operation} on `{address}` failed: {detail}. | A socket bind, listen, connect, or accept call returned an OS error. The address and operation are named so you can act on the specific failure. | Check that the address is reachable and the port is not already in use. For `bind`, try a different port. For `connect`, verify the server is running. |
| E2802 | TLS handshake with `{host}` failed: {detail}. | The TLS layer could not complete a secure handshake — the certificate may be invalid, expired, or untrusted by the system trust store. | Verify the server's certificate, ensure the system trust store is up-to-date, or use `jet.tls.insecure_skip_verify()` in a test environment only. |
| E2803 | Request body exceeds the {limit}-byte limit. | The server's configured `max_body` cap was reached. Allowing unbounded bodies risks memory exhaustion. | Raise the `max_body` option passed to `http.serve`, or reject the request in your handler before reading the body. |
| E2804 | Two routes both match `{METHOD} {pattern}`. | A duplicate route makes dispatch ambiguous — one handler would never be reached. Registered at start-up so the bug surfaces immediately. | Remove one of the duplicate registrations, or make the patterns distinct (e.g. add a static prefix to one). |
| L2801 | Blocking call inside the accept loop without a worker task. | A slow handler inside `http.serve` or a raw `net.tcp_accept` loop blocks all new connections until it returns. | Wrap the handler body in `tasks.spawn(() => …)` so each connection runs in its own task. |

## Testing and tooling diagnostics (E2-M11)

Quality workflows: doctests, snapshot testing, `todo` typed holes, `jet bench`, and capability summaries. E29xx is the block for M11.

| Code | What | Why | Fix |
|------|------|-----|-----|
| E2901 | Doctest output mismatch. Expected: `{expected}` Got: `{actual}` | The example in the doc comment claims a different result from what the code produces. Docs cannot lie (D-TEST4/I5 generalized to user code). | Run `jet test --update-snapshots` to update the golden output, or fix the code to match the claimed output. |
| E2902 | `#Todo` at `{file}:{line}` — expected `{type}` | A `#Todo` typed hole was reached at runtime. The hole compiles anywhere and type-checks, but panics when executed (D-TOOL2). | Replace `#Todo` with a real implementation. |
| L2901 | This `#Test` block has no assertions. | A test with no `require`, `require_eq`, or `expect(…).snapshot()` call cannot find bugs — it always passes. | Add at least one assertion, or remove the test if it only exercises compilation. |

## Debugging and observability diagnostics (E2-M12, D-OBS1–3)

Runtime panic reports and structured log shape. E30xx is the M12 block.
E3001/E3002 are **runtime** reports, not compile-time diagnostics — the
span is embedded in the message (Jet file + line + function name).
`jet explain E3001` explains the report format and D-OBS2 safe-locals policy.

| Code | What | Why | Fix |
|------|------|-----|-----|
| E3001 | `panic: {msg}` — with Jet file, line, function name, source-line context box, and (debug builds only) safe local variable values. | The program hit a `panic`, `require`, or `require_eq` call that failed, or a bounds/key check triggered at runtime. Jet file and line are shown in Jet terms — never generated-Rust terms (I2). | Fix the logic that led to the failure; the source line and locals show what values were in play. |
| E3002 | `error propagated from: {fn} ({file}:{line}) via ?` — an error-return trace entry appended when a `?` re-raises an error. | Each `?` that propagates an error adds a frame, making the full error path visible. | Follow the trace from the innermost `Err` origin to the outermost `?` to find where the error was created and which callers forwarded it. |

## Low-level tier diagnostics (E2-M13, S58)

The expert tier is gated twice: `use core.mem` unlocks the vocabulary, and an
`#Audit("…")` + `#Unsafe { … }` region (or an `#Unsafe fn` contract) opens the
operations that can violate memory safety. Ordinary Jet never reaches these.

| Code | What | Why | Fix |
|------|------|-----|-----|
| E3101 | `{op}` can only run inside an `#Unsafe` block. | This operation can violate memory safety, so it must sit in an audited region. | Wrap it: `#Unsafe("why this is safe") { … }`. |
| E3102 | `{item}` is part of the low-level tier. | Naming `Ptr`, `volatile_read`, or an allocator needs the discovery gate. | Add `use core.mem;` at the top of the file. |
| E3103 | `{fn}` is an `#Unsafe` function. | Its contract can't be checked by the compiler, so the caller must vouch for it. | Call it inside `#Audit("…") #Unsafe { … }`. |
| E3104 | `{arena}` was already {reset/freed}; this value lives in `{arena}` which is gone. | Calling `arena.reset()` or `arena.free()` invalidates all values allocated in it. | Move the `alloc` call before the `reset`/`free`, or create a new allocator. |
| L3101 | This `#Unsafe` block has no `#Audit` reason. | Every gated region records, in one line, why it can't break memory safety. | Add `#Audit("why this is safe")` on the line above. |

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

## Effect system diagnostics (D-EFF1, D-QUAL1)

Every function carries an inferred effect set (the ambient powers its body
reaches — `Net`, `Fs`, `Io`, `Db`, `Time`, …). A `#(…)` list on the signature
declares an upper bound; `#Pure` declares the empty set. The inferred set must
be a subset of the declared one. Effects are erased in codegen (I3), so these
are compile-time-only diagnostics. An unknown effect name in a `#(…)` list is
reported as **E0119** (unknown name).

| Code | What | Why | Fix |
|------|------|-----|-----|
| E0740 | `{fn}` uses the effect `{effect}`, which its signature doesn't allow. | A `#(…)` list is an upper bound on what the body may do; the inferred effects must be a subset. An effect the body reaches that the bound omits breaks that contract. | Add the named effect to the `#(…)` list, or stop using it (drop the Core call that introduces it, or move it out of this function). |
| E0741 | This `#Caps` region uses the effect `{effect}`, which it doesn't allow. | `#Caps(…)` restricts a region to a fixed set of effects; anything reached inside — even transitively through a call — must be in that set, so the region is a hard local ceiling. | Add the named effect to the `#Caps(…)` list, or move that work outside the region. |
| E0742 | This `{method}` impl uses the effect `{effect}`, which the trait doesn't allow. | A trait method may declare an effect upper bound (`#Pure fn hash(self)`, `fn render(self) #(Gpu)`); every implementation's inferred effects must fit inside it, so the bound holds for all impls (D-EFF3). | Remove the offending work from the impl, or widen the bound on the trait method. |
| E0711 | The capability `{handle}` can't escape its `#grant` block. | `#grant(…)` grants a capability into a lexical scope and revokes it at scope end (RAII, S63); returning, storing, or capturing the handle would let a revoked authority outlive the block (D-SCAP1). | Use the handle only inside the `#grant` block, or perform the work that needs it there. |
| E0712 | This `#grant` region uses the effect `{effect}`, which it has no capability for. | `#grant(…)` authorizes exactly the listed effects through its handle; the dual of `#Caps`, an effect reached inside — even transitively through a call — that the grant omits has no capability backing it (D-SCAP1). | Add the named effect to the `#grant(…)` list, or move that work outside the grant. |
| E0721 | Untrusted (`#Tainted`) data reaches `{api}` without being sanitized. | A `#Tainted` value is untrusted input; it spreads to anything derived from it. `{api}` is a sink effect (`Db`/`Exec`/`Net` — a database query, subprocess command, or network request), and an untrusted value used there unchecked is the classic injection bug (D-TAINT1). | Pass the value through a `#Sanitizer fn` first — its return value is trusted by contract, so it may reach the sink. |
| E0745 | `{fn}` is `#Pure` but also declares effects. | `#Pure` already means the empty effect set; a non-empty `#(…)` list on the same function asks for both empty and non-empty at once. | Drop the `#(…)` list to keep the function pure, or remove `#Pure` to allow the listed effects. |
| E0746 | `{api}` has the `{effect}` effect, which can't be rolled back inside a `#Transact` block. | A `#Transact` block undoes its work on a `?`-failure; a network, file, or subprocess effect (`Net`/`Fs`/`Exec`) leaves committed external state a rollback can't take back, so performing it on the block's direct path would break the all-or-nothing contract (D-TXN2). | Move the call after the block, or register it with `<handle>.on_commit(() => { … })` so it runs only after a clean commit. |
| E0747 | This callback uses the effect `{effect}`, which the parameter doesn't allow. | A `#Pure fn(…)` parameter demands a pure callback, and a `#(E) fn(…)` parameter bounds the callback to the listed effects; the actual callback's inferred effects must be a subset (D-EFF2). The bound is checked at the call site, so an impure callback is rejected before it runs. | Pass a callback within the bound (a `#Pure fn` for a pure parameter), or widen the parameter's effect bound. |
| E0748 | `#(via {param})` on `{fn}` names no such parameter / a parameter that isn't a callback. | `#(via f)` publishes a function's effects as a tight pass-through of its callback parameter `f` (D-EFF2); `f` must be a parameter of the function whose type is a `fn(…)`. | Point `via` at a function-typed parameter, or drop the `#(via …)` annotation. |

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

## C FFI diagnostics (E2-M14, S59)

| Code | What | Why | Fix |
|------|------|-----|-----|
| E3201 | C library `{lib}` was not found. | Jet looked for a `{lib}: c@…` dep in `pkg.jet`, then tried `pkg-config {lib}` on the system; neither provided include/link paths. | Install the system package (e.g. `pacman -S {lib}`), or declare it as `{lib}: c@system` in `deps:`. |
| E3202 | Type `{ty}` cannot cross the C boundary here. | C FFI allows by-value scalars and `String` in ordinary code; pointers and other gated types need `use core.mem` and an `#Unsafe { … }` region (S58). | Move the call inside `#Unsafe`, or change the type to a C-safe value type. |
| E3203 | `{ty}` is not a C-compatible type for a foreign function parameter or return. | `@extern` / `@bindgen` functions must use types with a stable C ABI at the edge. | Use scalars, `String`, or a struct with C layout; pointers only through the gated tier. |
| E3204 | Two different `use` forms refer to the same C library `{lib}`. | S59 allows one bring-in per C lib per file — either `use "{header}" as alias` or `use c.{lib} as alias`, not both. | Remove one line; keep the form that matches your workflow. |
| E3205 | Overlay `{name}` disagrees with the generated binding. | User `@extern module c.{lib}` may override bindgen symbols, but the Jet signature must stay compatible when replacing. | Match the generated signature, or rename your overlay function. |
| E3206 | Module path `{path}` uses the reserved segment `__bindgen__`. | Autogen lives in `c.{lib}.__bindgen__`; users declare overlays as `@extern module c.{lib}` only. | Drop `__bindgen__` from your module path, or use `@extern module c.{lib} { … }`. |
| E3207 | `@bindgen` is only allowed in generated cache files. | `.jet/bindings/c/{lib}.jet` is written by `jet bind`; hand-written sources use `@extern module`. | Edit your overlay file with `@extern module`, or regenerate the cache with `jet bind`. |
| E3208 | Could not generate bindings from `{header}`. | Header parsing or translation failed in the bind backend. | Fix the header path, install dev headers, run `jet bind` manually for details, or hand-write `@extern module c.{lib}`. |
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
| E3401 | `{pure_fn}` calls the impure function `{call}`. | A `#Pure fn` may only call other `#Pure fn`s and pure builtins. Impure calls make the result non-deterministic (D-PURE2). In `jet eval --pure` the whole call graph from `main` is checked transitively; the why-line shows the full chain (`main → a → b calls \`print\``) so the user can find the leak. | Mark `{call}` as `#Pure fn`, or remove the call from `{pure_fn}`. |
| E3402 | `{call}` is not allowed during a sandboxed package build. | Package builds run with ambient I/O and network access disabled (D-PURE2). | Compute this value at compile time or pass it in as a parameter. |
| E3403 | `{what}` is non-deterministic and cannot appear in a pure evaluation. | Pure evaluation must produce the same result on every machine (D-PURE2). | Remove this call, or do not mark the enclosing function `#Pure`. |

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
| E1215 | `jetpack.toml` {kind} `{name}` is not recognized. | `jetpack.toml` only accepts the tables `[repo]`, `[sources]`, and `[packages]`, and the keys listed for each. An unknown name is usually a typo. | Did you mean `{suggestion}`? Check the allowed names for this table. |

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
it is populated from the S14 teaching auto-corrects (e.g. E0037 "replace
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

`#PublishedSchema` pins a record's saved shape at release (D-MIGRATE1/2). A
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
no error. The runtime `from_vXXX` data conversion (reading old records through the
declared converter/default) is the Build-tier versioning library (follow-on #11);
E0910 checks intent only.

### E0911 — Unknown migration verb

| Case | What | Why | Fix |
|------|------|-----|-----|
| `drop` (D-MIGRATE2D) | `drop` isn't a migration verb — use `remove`. | A migration deletes a field with `remove`; `drop` is not a Jet keyword here. | Write `remove <field>`. |
| `reorder` (D-MIGRATE2F) | `reorder` isn't a migration verb — field order isn't a breaking change. | A `#PublishedSchema` record is keyed by field name, so reordering is safe. | Delete the `reorder` line; write the fields in any order. |
| other | `{op}` isn't a known migration verb. | A migration block contains `rename`, `add`, `remove`, or `change` operations. | Use one of those four verbs. |

## Process for a new diagnostic

1. Claim the next code here. 2. Write what/why/fix per the voice rules.
3. Add a tests/ui fixture + snapshot. 4. Ship. A diagnostic without a
snapshot test does not exist (invariant I4).
