# Name overlap: parameters and locals that reuse a module-level name

Date: 2026-10-30. Card #6316. Probes live in
`$JET_SCRATCH_ROOT/lanes/SurfaceDesign/probes/m6316` (120 programs) and
`probes/m6316b` (1 program), with results in `m6316.results/`. Each probe ran
with `jet run` at `--optimize=none`, `basic` and `full` using the dev compiler at
abab9666d8. Every probe printed the same output and exit status on all three
levels, and none printed a warning.

## Matrix

Each probe declares one outer name and reuses it in one inner position.

The outer names are:

- a file function (`helper`) and a sibling function that shares its name with
  a struct field (`size`);
- a module variable (`counter :=`) and a module constant (`LIMIT ::` and
  `limit ::`);
- an imported item (`use ... .[twice]`) and an imported module (`json`, `term`);
- a Prelude function (`max`), a Prelude alias (`read_file`), a Prelude
  build-time function (`find`), and the builtin `print`.

The inner positions are:

- a parameter, a closure parameter, and a function-typed parameter that is
  called or passed on to `map`;
- a local constant and a local variable;
- a local value or local closure inside a nested block;
- an `if x == Val(name)` pattern and a match-arm pattern.

Every probe then uses the outer name after the inner scope ends, so a silent
choice shows in the output.

## What happens today

| Outer name | Parameter, closure parameter, pattern, nested block | Local in the same scope as a later outer use |
| --- | --- | --- |
| file function | silent: the inner name wins inside, the function outside | E0803 "`helper` is Int, not a function" at the later call |
| imported item | silent | E0803 |
| sibling function | silent | E0803 |
| Prelude function `max` | silent | E0803 |
| builtin `print` | silent for parameters and closure parameters; E0803 for patterns and nested blocks | E0803 |
| Prelude alias `read_file` | silent | silent |
| Prelude build-time `find` | silent | silent |
| imported module `json` | silent | E0104 at `json.to_string(...)` |
| imported module `term` | silent | silent; see below |
| module variable `counter` | E0118 "The name `counter` is already taken here" | E0118 |
| module constant `LIMIT` | E0357: an inner `LIMIT` must be snake_case | E0357 |
| module constant `limit` | E0357 at the declaration: constants must be ALL_CAPS | E0357 |

Details:

- A local `term :: 10` beside `use term :: core.term` compiles. `term + 1`
  prints 11, and the next line, `term.print("t")`, prints `10` and then `t`.
  The call no longer reaches the module function as written
  (`import_module_term__local_const`).
- A local `read_file :: 10` or `find :: 10` compiles with no warning. The
  registered lint L0510 ("declaration replaces a Prelude alias") has no
  emitter, so `tests/ui_lint/prelude_shadow_warn.warn` and
  `prelude_assert_eq_shadow.warn` have no matching output.
- A module variable is the only outer name that is always refused. E0118 says
  "Inside one function, each name refers to exactly one thing" and fires for
  parameters as well as locals.
- An upper-case name in a pattern compares against the constant instead of
  binding: `if opt == Val(LIMIT)` with `opt = Val(10)` and `LIMIT :: 2` prints
  only the later `print(LIMIT)`. The same pattern as a match arm,
  `Val(LIMIT) -> ...`, is E0305 "Pattern `Val` doesn't match `Int?`", while a
  lower-case `Val(helper)` arm binds and runs.
- `math` without `use core.math` is E0107 "Nothing named `math` exists here"
  (`m6316b/bare_math_local`), so a local `math` hides nothing.

In short, an inner name silently wins over a file function, an imported item,
an imported module, a Prelude name or `print`. It stays silent whenever the
outer name is not used later in the same scope. Module variables and constants
never overlap, because E0118 and the casing rule E0357 refuse them.

## Prior art

- GCC and Clang `-Wshadow` warn when a local or parameter shadows a global
  variable or a built-in function. It is not part of `-Wall`.
  https://gcc.gnu.org/onlinedocs/gcc/Warning-Options.html
- Go has a separate `shadow` analyzer that is not in the default `go vet`.
  https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/shadow
- Rust allows shadowing. Clippy's `shadow_same`, `shadow_reuse` and
  `shadow_unrelated` lints are off by default.
  https://rust-lang.github.io/rust-clippy/master/
- ESLint `no-shadow` is off by default; its `builtinGlobals` option extends it
  to globals. https://eslint.org/docs/latest/rules/no-shadow
- Pylint warns by default: W0621 `redefined-outer-name` and W0622
  `redefined-builtin`.
  https://pylint.readthedocs.io/en/stable/user_guide/checkers/features.html
- Kotlin warns "Name shadowed" when a lambda parameter or local hides an outer
  local, but not a top-level declaration.
- C# makes a local that hides an enclosing local an error (CS0136), and lets a
  parameter hide a field without a warning.
