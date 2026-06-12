# 01 — Language Spec (living document)

Behavior described here is authoritative when ratified in
docs/02-syntax-decisions.md (enforced by `tests/decisions.rs` on every
`cargo test`). Open decisions in docs/02 are not implemented until
ratified. The examples/ directory is the executable form of this spec: if
the spec and a passing example disagree, the spec is wrong — fix the spec.

## M1 — what exists today (values, expressions, control flow)

### Jetical rules

- Source is UTF-8. Identifiers: a letter or `_`, then letters, digits, `_`.
- Source files use the `.jet` extension (N2).
- Line comments: `//` to end of line (S5).
- String literals: `"..."` on a single line. Escapes (S20): `\n` `\t` `\"`
  `\\` only; anything else after `\` is E0001. Interpolation (S8): `{expr}`
  embeds any printable expression; `{{` and `}}` write literal braces; a
  lone `{` or `}` is E0001.
- Numbers: decimal `Int` (64-bit signed, E0007 if too large) and `Float`
  (digits `.` digits). Unary minus is an operator, not part of the literal.
- `true` and `false` are `Bool` literals.
- Statements end with `;` (S6 — required, including before `}`). Blocks
  (`}` of `if`/`while`/`for`/`fn`) don't take one; `switch` arms do.
- The lexer recovers from bad characters and keeps going; one run reports
  every lexical error it can.

### Grammar (EBNF)

```
program  = { func | struct | const } ;
func     = [ "pub" ] "fn" ident "(" [ params ] ")" [ "->" type ] block ;
params   = param { "," param } ;
param    = [ "mut" | "take" ] ident ":" type ;
block    = "{" { stmt } "}" ;            // S3: curly braces
stmt     = binding | assign | if | while | for | switch
         | "break" ";" | "continue" ";" | "return" [ expr ] ";"
         | expr ";" ;
binding  = ( "val" | "var" ) ident [ ":" type ] "=" expr ";" ;
assign   = ident ( "=" | "+=" | "-=" | "*=" | "/=" | "%="
                 | "&=" | "|=" | "^=" | "<<=" | ">>=" ) expr ";" ;
if       = "if" expr block { "else" "if" expr block } [ "else" block ] ;
while    = "while" expr block ;
for      = "for" ident "in" expr ".." expr block ;   // S22: inclusive
switch   = "switch" expr "{" { expr "->" block ";" }
           "else" "->" block ";" "}" ;               // S24
expr     = precedence climbing over:
           "||"  >  "&&"  >  "==" "!=" "<" ">" "<=" ">="
           >  "|"  >  "^"  >  "&"  >  "<<" ">>"
           >  "+" "-"  >  "*" "/" "%"  >  unary "-" "!"
           >  call | ident | literal | "(" expr ")" ;
```

### Semantics

- Types: `Int`, `Float`, `Bool`, `String`. Local inference: annotations on
  bindings are optional; mismatched annotations are E0108.
- A program must define `fn main` with no parameters and no return type
  (E0101, E0122); execution starts there. `main` never takes `pub` (S12).
- `val` is immutable, `var` mutable; assigning to a `val` is E0111. Names
  may not shadow an existing name in scope (E0118).
- Arithmetic: `+ - * /` on `Int` and `Float` (never mixed — E0109);
  `% & | ^ << >>` on `Int` only. `+` on `String` is a teaching error
  pointing at interpolation. Compound assignment (S17) mirrors the binary
  operators.
- Comparisons (`== != < > <= >=`) need matching operand types and yield
  `Bool`; `&& || !` operate on `Bool` (E0110).
- **S25 comparison distribution**: in a `&&`/`||` chain, a plain value on
  the right re-applies the nearest comparison to its left:
  `day == "sat" || "sun"` means `day == "sat" || day == "sun"`. The
  value's type must match what was compared; a plain value with no
  comparison to its left is an error.
- `if`/`else if`/`else` (conditions must be `Bool`); `while`; `for x in
  a..b` iterates a through b **inclusive** (S22); `break`/`continue`
  inside loops only (E0115, S23).
- `switch subject { cond -> { ... }; else -> { ... }; }` (S24): arms are
  arbitrary `Bool` conditions tried top to bottom; `else` is mandatory.
  Lowered to an if/else chain; rustc optimizes it.
- `print(x)` is built in (S9); takes exactly one printable argument
  (E0103, E0112) and writes it with a trailing newline. `Float` always
  prints a decimal part (S21): `-5.0`, not `-5`.
- Functions: multi-argument calls, checked arity (E0104) and argument
  types (E0112). A function with a return type must return on every path
  (E0114). Unknown names are E0102/E0107 with did-you-mean suggestions.
- Definitions are unique (E0105), can't shadow built-ins (E0106), and
  unknown type names are E0119.

### Staged errors

Features that exist in the roadmap but not the language yet fail with an
error naming the milestone (see staged table in docs/02).
A future feature must never die as a generic syntax error. Teaching
errors (S14, E0008–E0016) recognize foreign spellings — `def`, `let`,
`set`, `println`, `and`/`or`/`not`, `Text`, `try`, `use`, `match` — and
name the Jet form.

## M2 — ownership (done)

Borrow-checker mechanics live in the transpiler; tier-1 users never write
`&`, `&mut`, `*`, or lifetime parameters.

| You write              | It means                          | Compiles to Rust |
|------------------------|-----------------------------------|------------------|
| `fn f(x: T)`           | shared read (default)             | `x: &T`          |
| `fn f(mut x: T)`       | mutable borrow                    | `x: &mut T`      |
| `fn f(take x: T)`      | move; caller must write `take`    | `x: T`           |
| `fn f() -> view T`     | borrow return (elided lifetime)   | `-> &T`          |
| `ref field: T` (tier 2)| stored reference in a struct      | `field: &'a T`   |

Call-site rules: `mut` and `take` must match the parameter; omitting `take`
on a clonable type inserts `.clone()` with lint **L0201**; on a
non-clonable type → **E0201**. Omitting `mut` on a mutable parameter →
**E0202**. Using the same name twice in one call while `mut` is active →
**E0204**. `*` outside `unsafe` → **E0208**.

`const NAME = value` always looks the same; the transpiler emits Rust
`const` or `static` when the address is taken or the type needs it.

Aliasing rule, stated for humans: *while something is being changed,
nobody else may be looking at it.* Foreign `read`/`write` spellings get
teaching errors **E0017**/**E0018** (S14). A `view` return may only hand
back a parameter, a scalar local, or a const — not fresh text (**E0206**).

## M3 — data & methods (done)

Structs and enums carry fields; methods attach behavior (S27). Ratified
surface (Group 2): struct literals **`Type { f: v }`** (S29); enums with
**`Type.Variant`** (S30); **`==` pattern tests** (S31); optional
**`T?`** with **`value(v)`** / **`null`** (S32); generic args
**`Type<Args>`** (S33). `null` is only legal for `T?`, never plain `T`.

```
struct Circle {
    radius: Float;

    fn area(self) -> Float {
        return 3.14159 * radius * radius;
    }
}

impl Circle {
    fn unit() -> Circle {
        return Circle { radius: 1.0 };
    }
}
```

- **`self`** is the receiver; prefix with `mut` or `take` like any parameter.
- Invoke with **`c.area()`** (not `area(c)`).
- Methods may live **inside** the type **or** in **`impl Type { }`** — same rules either way.
- Static methods omit `self` (e.g. `Circle.unit()`).
- Enum `switch` arms must be exhaustive; missing cases are a compile error.
- **Traits (S28)** arrive in M9: `trait` declarations, `impl` inside a
  type or as `impl Type: Trait { }` (e.g. `impl other.Point: Serialize`).
  `.` for modules and methods; `:` separates type from trait in `impl`.

## M4 — errors as values (done)

Fallible functions return **`Result<T, E>`** (S34): `T` is the success
payload, `E` is any enum, struct, or `String`. Build outcomes with
**`ok(v)`** and **`err(e)`**; test them with **`== ok(n)`** / **`== err(e)`**
(same pattern machinery as M3 optionals).

- Postfix **`?`** (S7) propagates: unwraps `ok`, early-returns `err`. The
  enclosing function must return a compatible `Result<…, E>` (same error
  type in v1). On **`T?`**, `?` propagates `null` when the function returns
  an optional.
- **`or <expr>`** (S35) is the fallback operator on a fallible value:
  yields the success payload or evaluates the right side. Precedence is
  looser than **`&&`** / **`||`**, so `a? or b` and `x == 1 || y or 0`
  parse predictably. The right side may be a value, **`return`**, **`return expr`**,
  or **`panic(…)`**.
- **`panic("msg")`** and **`require(cond)`** / **`require(cond, "msg")`**
  (S36) stop the program with a friendly report on stderr and exit code 70.
- In **`switch <fallible-expr> { … }`**, when the subject is not a plain
  name, **`it`** names the subject for pattern arms like **`it == ok(n)`**.
- **`main`** may not return a fallible type; handle errors with **`or`**, a
  full **`switch`**, or **`panic`**.

Unchecked fallible values (**E0401**), ignored fallible calls (**E0402**),
bad propagation (**E0403**), `ok`/`err` outside a result context (**E0404**),
and fallback type mismatches (**E0405**) are compile errors with fixes that
name **`?`**, **`or`**, and pattern tests.

## M6 phase 1 — `jet fmt` (done)

**`jet fmt <file.jet>`** rewrites the file in place to canonical Jet style
(S44). **`jet fmt --check <file>`** prints a unified diff and exits **1**
when the file would change (CI mode). Formatting is lex → parse → print;
sema and rustc are not run.

Style (zero configuration): 4-space indent, `{` on the same line as its
header, one statement per line, at most one blank line between top-level
items, spaces around binary operators, no space before `;`/`,`/call `(`,
trailing `;` on statements (S6). **Line width is not enforced in v1.**

`//` comments are preserved and re-attached by source span. When S14
teaching recovery has already lowered foreign spellings in the AST (`let` →
`val`, `def` → `fn`, …), fmt prints the canonical form. Real parse errors
still block fmt.

Idempotence: **`fmt(fmt(x)) == fmt(x)`** on every `examples/*.jet` and
`tests/ui/*.fixed.jet` (`tests/fmt.rs`).

## M6 phase 2 — `jet test` + `jet new` (done)

**`test "name" { … }`** (S43) — top-level blocks only. Bodies parse like a
parameterless function; use **`require(cond)`** / **`require(cond, "msg")`**
and **`require_eq(a, b)`** (S36) for checks. Duplicate test names → **E0105**;
a nested `test` block → **E0601**. **`jet run`** / **`jet build`** ignore test
blocks; only **`jet test`** compiles and runs them.

**`jet test <file.jet>`** (or a directory of `*.jet` files) builds one harness
binary per file (no cargo project; R9). Each test runs in isolation; failures
use a generated unwind boundary (not observable in user code). Output is one
line per test (`name: pass` / `name: FAIL`), a summary (`N passed, M failed`),
and exit **1** when any test fails. **`require_eq`** failures print
`left: …, right: …` on stderr.

**`jet new <name>`** creates `<name>/main.jet` (hello world) and
`<name>/.gitignore` (`build/`). No manifest (M12; opt-in).

Example: `examples/20_tests.jet`. Goldens: `examples/expected/20_tests.test.out`,
`tests/jet_test.rs`, `tests/fixtures/test_fail.jet` + `.fixed.jet`.

**NixOS / flake:** `nix develop` provides `cargo`, `rustc`, `gcc`, `nodejs`,
and a **`jet`** wrapper around `target/debug/jet`. **`cargo build`** once, then
`jet run …` / `jet lsp` / `cargo test --test lsp`. Editor setup:
`editors/vscode/README.md`. Release binary: `nix build .#jet`.

## M7 — Rust FFI (`extern rust`, done)

**`extern rust "crate@version" { … }`** (S50) declares foreign functions. Each
entry is a normal Jet signature plus **`= "rust::path"`** naming the target
item. **`extern rust "std" { … }`** works for standard-library items with no
extra dependency. Non-`std` crates require an exact version pin (**E0701**).

Allowed boundary types pass **by value**: `Int`, `Float`, `Bool`, `String`,
`Char`, `List`/`Map`/`T?`/`Result<…>` built from allowed types, and
structs/enums whose fields are allowed. No `mut`/`take`/`view` parameters, no
borrowed returns, no callbacks (**E0702**).

When any crate dependency is needed, the driver builds a hidden cached cargo
bridge under `~/.cache/jet/ffi/` and links it into the generated program (R9:
the user's folder never grows a manifest). Missing **`cargo`** → **E0703**;
fetch/build failures → **E0704** (cargo output in an indented block); a wrong
foreign path or signature → **E0705**. Panics inside foreign code are caught
at the boundary and become the M4 runtime report (exit 70).

Teaching: **`unsafe`** / C-style FFI spellings → **`extern rust`** (**E0031**).

Example: `examples/22_ffi.jet` (`base64@0.22`). Ui: `tests/ui/ffi_*.jet`.
Integration: `tests/ffi.rs` (gated on `cargo`).

## M6 phase 3 — multi-file imports (done)

Two import forms (S16): **`import "path/to/file";`** (relative to the importing
file's directory; default namespace = last path segment) and **`import name;`**
(searches recursively from the project root for `name.jet` or `name/{name,main}.jet`;
skips `build/`, `target/`, dot-dirs). Optional **`as alias`** in both forms.

Cross-file access uses **`namespace.item`**; only **`pub`** items are visible from
other files (S18), including **`pub`** struct fields. The driver loads the import
graph, sema checks the whole program, codegen emits one Rust file with **`mod`**
blocks and `user_<module>_<name>` mangling (`main` stays `main`).

Diagnostics: **E0602** path escapes the project · **E0603** missing import ·
**E0604** import cycle · **E0605** private item · **E0606** ambiguous module.
Example: `examples/21_imports/` (three files; file import + `as alias`). UI
fixtures under `tests/ui/import_{escape,missing,cycle,private,private_field,ambiguous}/`.

## M6 phase 4 — `--small` + LSP v0 (done)

**`jet build --small`** (S15): `opt-level=z`, fat LTO, `panic=abort`, stripped symbols.
Smaller binaries than the default speed-oriented profile (`tests/small.rs` on
`examples/16_wordcount.jet`).

**`jet lsp`**: stdio JSON-RPC language server (hand-rolled JSON, invariant I6).
Capabilities: full-document diagnostics on open/change (real front end, including
import graph from disk with an in-memory overlay for the open buffer), S14
teaching-error quick-fixes (`Diagnostic.edit`), and formatting via `jet fmt`.
Scripted tests: `tests/lsp.rs`.

**VS Code / Cursor**: `editors/vscode/` — TextMate grammar + LSP client (plain
JS, no compile step; `install.sh` packs and installs the vsix). The client
auto-discovers the server: `jet.languageServerPath` setting, then
`<workspaceFolder>/target/debug/jet`, then `jet` on PATH. `jet lsp` never
invokes rustc, so the cargo debug binary is sufficient.

## Deliberately absent

See non-goals in docs/00-philosophy.md. The parser should produce staged
or guiding errors for the ones users will reach for (e.g. `and` → teaching
error naming `&&`, per S14).
