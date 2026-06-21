# Language Spec (living document)

Behavior described here is authoritative when ratified in
docs/spec/syntax-decisions.md (enforced by `tests/decisions.rs` on every
`cargo test`). Open decisions in docs/spec/syntax-decisions.md are not implemented until
ratified. The examples/ directory is the executable form of this spec: if
the spec and a passing example disagree, the spec is wrong — fix the spec.

## M1 — what exists today (values, expressions, control flow)

### Lexical rules

- Source is UTF-8. Identifiers: a letter or `_`, then letters, digits, `_`.
- Source files use the `.jet` extension (N2). The path-accepting commands
  (`jet run`/`build`/`check`/`eval`) make the extension optional: `jet run
  examples/test` resolves to `examples/test.jet` when the literal path has no
  matching file. If neither the literal path nor `<path>.jet` exists, the
  original name is kept so the file-not-found diagnostic names what you typed.
- Line comments: `//` to end of line (S5). Block comments: `/* … */`, which
  nest (an unbalanced `/*` is E0002), so any region can be commented out (S5).
- String literals: `"..."` on a single line. Escapes (S20): `\n` `\t` `\"`
  `\\` only; anything else after `\` is E0001. Interpolation (S8): `{expr}`
  embeds any printable expression; `{{` and `}}` write literal braces; a
  lone `{` or `}` is E0001.
- Multi-line strings (S70): `"""…"""` span multiple lines with the same escapes
  and interpolation. The newline right after the opening `"""` and the one right
  before the closing `"""` are dropped, and the closing `"""`'s indentation is
  stripped from every line (Swift-style). An unterminated `"""` is E0002.
- Numbers (S67): decimal `Int` (64-bit signed, E0007 if too large) and `Float`
  (digits `.` digits, optional `e`/`E` exponent). `_` digit separators are
  allowed anywhere among the digits (`1_000_000`); base prefixes `0x`/`0o`/`0b`
  give an `Int` (`0xFF`, `0o755`, `0b1010`), and a prefix with no digits is
  E0001. Unary minus is an operator, not part of the literal.
- `true` and `false` are `Bool` literals.
- Statements end with `;` (S6 — required, including before `}`). Blocks
  (`}` of `if`/`loop`/`fn`) don't take one; `when` arms do.
- The lexer recovers from bad characters and keeps going; one run reports
  every lexical error it can.

### Grammar (EBNF)

```
program  = { func | struct | const } ;
func     = [ "pub" ] "fn" ident "(" [ params ] ")" [ "->" type ] block ;
params   = param { "," param } ;
param    = [ "mut" | "take" ] ident ":" type ;
block    = "{" { stmt } "}" ;            // S3: curly braces
// S6-R: no visible `;` — the lexer inserts a synthetic terminator (NL below)
// at each line end after a statement-ending token; the grammar stays
// terminator-based. A leading `.` or binary/logical operator on the next line
// suppresses insertion (continuation). `-> Type` / `{` must stay on the `)`
// line (E0986). `NL` below denotes that synthetic terminator.
stmt     = binding | assign | if | loop
         | "break" NL | "continue" NL | "return" [ expr ] NL
         | expr NL ;
binding  = ( ident [ ":" type ] | destructure ) ( "::" | ":=" ) expr NL ; // D-BIND1: `::` immutable, `:=` mutable (no keyword)
destructure = ident "{" ident { "," ident } "}"   // S74: struct fields
            | "[" [ ident { "," ident } ] "]" ;    // S74: list elements
assign   = ident ( "=" | "+=" | "-=" | "*=" | "/=" | "%="
                 | "&=" | "|=" | "^=" | "<<=" | ">>=" ) expr NL ;
// D-IF1: `if` is the one branching keyword. Two forms by body shape:
if       = "if" cond block { "else" "if" cond block } [ "else" block ]   // two-arm
         | "if" subject "{" { arm } [ "else" "->" arm-body ] "}" ;       // multi-arm dispatch
arm      = arm-head "->" arm-body NL ;
arm-head = value | condition ;   // bare value ⇒ `subject == value`; else a Bool condition (D-IF2 Q3)
arm-body = block | stmt ;        // `{ … }` block or one braceless statement (D-IF2 Q2)
loop     = [ "@" ident ] loop-body ;            // D-LABEL1: optional `@name` label
loop-body= "loop" block                                                  // infinite
         | "loop" cond block                                             // conditional (was `while`)
         | "loop" ident "in" expr [ ".." expr [ "step" expr ] ] block ; // iteration (was `for`)
         // S19-amend: `while` and `for` are teaching errors (E0050/E0051)
break    = "break" [ "@" ident ] NL ;           // D-LABEL1: `break @name` targets a label
continue = "continue" [ "@" ident ] NL ;        // D-LABEL1: `continue @name`
cond     = expr | "(" expr ")" ;                     // S68/D-SG2: optional parens, fmt strips them
if-expr  = "if" cond value-block "else" ( if-expr | value-block ) ;  // S68/D-SG2: value form
value-block = "{" { stmt } expr "}" ;                // trailing expr (no `;`) is the block's value
           // `when` is retired (D-IF1): a teaching error (E0984) pointing at the
           // multi-arm `if` form above.
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
- `name :: value` is immutable, `name := value` mutable (D-BIND1); assigning
  to an immutable binding is E0111. The retired `val`/`var` keywords are a
  teaching error (E0985). Names may not shadow an existing name in scope (E0118).
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
- `if`/`else if`/`else` (conditions must be `Bool`); `loop` in three forms:
  `loop { }` (infinite), `loop cond { }` (conditional), `loop x in a..b { }`
  (iterates a through b **inclusive**, S22; S19-amend); `break`/`continue`
  inside loops only (E0115, S23). `while`/`for` are teaching errors. A loop may
  carry an `@name` label (D-LABEL1) — `@outer loop … { }` — and `break @outer` /
  `continue @outer` target it from a nested loop (E0987 names an out-of-scope
  label; E0988 flags a `@name` not followed by `loop`).
- `when subject { cond -> { ... }; else -> { ... }; }` (S24): arms are
  arbitrary `Bool` conditions tried top to bottom; `else` is mandatory.
  Lowered to an if/else chain; rustc optimizes it.
- **Prelude (D-PRELUDE1 = B):** `print` and `input` are ambient — usable in
  any Jet file with no `use` line. `eprint`, `args`, and `read_all_input`
  stay qualified behind `use core.io`. A user-defined function named `print`
  or `input` shadows the prelude one in that scope (prelude is lowest-priority).
- `print(x)` is built in (S9); takes exactly one printable argument
  (E0103, E0112) and writes it with a trailing newline. `Float` always
  prints a decimal part (S21): `-5.0`, not `-5`.
- `input()` / `input(prompt)` is prelude (D-PRELUDE1); reads a line from
  stdin, strips the trailing newline, and returns `Result(String, IoError)`.
  Use `??` to unwrap or handle the error.
- Functions: multi-argument calls, checked arity (E0104) and argument
  types (E0112). A function with a return type must return on every path
  (E0114). Unknown names are E0102/E0107 with did-you-mean suggestions.
- **Named args and defaults (S61, D-NARG1):** parameters may carry a
  default value (`fn f(x: Int = 0)`); call sites may use a label to
  document intent (`f(x: 1)`). Labels must match the parameter name at
  that position — they never reorder arguments. Trailing defaults fill
  when omitted. Both rules apply equally to free functions **and methods**
  (D-NARG1). `jet fmt` preserves call-site labels as written (D-NARG2).
  A positional `Bool` parameter on a `pub` fn or `pub` method triggers the
  advisory L2401 lint.
- Definitions are unique (E0105), can't shadow built-ins (E0106), and
  unknown type names are E0119.

### Staged errors

Features that exist in the roadmap but not the language yet fail with an
error naming the milestone (see staged table in docs/spec/syntax-decisions.md).
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
on a clonable type inserts `.clone()` with lint **L0201** (fired only when
the cloned value is dead after the call — D-L0201); on a non-clonable type → **E0201**. Omitting `mut` on a mutable parameter →
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
surface (Group 2): struct literals **`Type{f: v}`** (S29; flush, S29-FLUSH); enums with
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
- **Named constructors (D-CTOR1):** multiple construction shapes = multiple
  distinctly-named no-`self` statics returning the type (`Point.cartesian`,
  `Point.polar`). Overloading is rejected; a duplicate name is E0105 with
  a teaching message pointing at constructor naming.
- Enum `when` arms must be exhaustive; missing cases are a compile error.
- **Traits (S28, M9):** `trait Name { fn sig(self) -> T; … }` — signatures
  only. Implement inside a type (`impl Trait { … }`) or outside as
  `impl Type: Trait { … }` (qualify foreign types: `impl other.Point: Shape`).
  A trait name in type position (`[Shape]`, `fn f(s: Shape)`) means
  dynamic dispatch with invisible boxing. Generic params: `fn f<T: Bound>(…)`
  and `struct Pair<T> { … }`. Built-in traits follow S55: auto
  `Printable`/`Equatable`; explicit `@Comparable` / `@Serialize` (S82).
- **Attributes (S82):** `@Marker` or `@[a, b]` on the line before a
  declaration; `@Marker { … }` for scoped effects (`@transact`, `@unsafe`) or
  in-body config (`@Serialize { rename …; }`). **`pure fn`** and **`comptime`**
  stay prefix keywords.

## M4 — errors as values (done)

Fallible functions return **`T ? E`** (S34): `T` is the success payload,
`E` is any enum, struct, `String`, or the default **`Error`** type. Omitting
the error side in a function return — **`T ?`** — means **`T ? Error`**.
Build outcomes with **`ok(v)`** and **`err(e)`**; test them with
**`== ok(n)`** / **`== err(e)`** (same pattern machinery as M3 optionals).
Cross-type **`?`** conversion supports two forms:
- **`Fallible`** trait (D-ERR2): `impl MyFail: Fallible { fn to_error(self) -> Error { … } }` — converts any typed error to the universal `Error`. Prelude types implement `Fallible` by default.
- **Declared typed conversion** (D-ERR-CONV): `impl Source -> Target { return Target.Variant(self) }` — converts a `Source` error into a typed `Target` error; `?` applies it automatically. Declared once per (Source, Target) pair; rejected unless declared (orphan rule S28 applies). `E2404` fires when `?` would need an undeclared conversion; `E2405` fires on duplicate declarations; `E2406` fires on orphan-rule violations.

- Postfix **`?`** (S7) propagates: unwraps `ok`, early-returns `err`. The
  enclosing function must return a compatible fallible type. On **`T?`**,
  `?` propagates `null` when the function returns an optional.
- In a function return type, **`T?`** parses as **`T ?`** and the formatter
  writes the space. A function that returns an optional writes
  **`-> (T?)`**.
- **`?? <expr>`** (S35/S71) is the fallback operator on a fallible value or
  optional: yields the success payload or evaluates the right side. Precedence is
  looser than **`&&`** / **`||`**, so `a? ?? b` and `x == 1 || y ?? 0`
  parse predictably. The right side may be a value, **`return`**, **`return expr`**,
  or **`panic(…)`**. The retired word **`or`** is a teaching error pointing at
  **`??`** (S71, D-SG6).
- **`panic("msg")`** and **`require(cond)`** / **`require(cond, "msg")`**
  (S36) stop the program with a friendly report on stderr and exit code 70.
- In **`when <fallible-expr> { … }`**, when the subject is not a plain
  name, **`it`** names the subject for pattern arms like **`it == ok(n)`**.
- **`main`** may not return a fallible type; handle errors with **`??`**, a
  full **`when`**, or **`panic`**.

Unchecked fallible values (**E0401**), ignored fallible calls (**E0402**),
bad propagation (**E0403**), `ok`/`err` outside a result context (**E0404**),
and fallback type mismatches (**E0405**) are compile errors with fixes that
name **`?`**, **`??`**, and pattern tests.

## M6 phase 1 — `jet fmt` (done)

**`jet fmt <file.jet>`** rewrites the file in place to canonical Jet style
(S44). **`jet fmt --check <file>`** prints a unified diff and exits **1**
when the file would change (CI mode). Formatting is lex → parse → print;
sema and rustc are not run.

Style (zero configuration): 4-space indent, `{` on the same line as its
header, one statement per line, at most one blank line between top-level
items, spaces around binary operators, no space before `;`/`,`/call `(`,
trailing `;` on statements (S6). **Line width is not enforced in v1.**

`//` and `/* … */` comments are preserved and re-attached by source span. When S14
teaching recovery has already lowered foreign spellings in the AST (`let` →
`val`, `def` → `fn`, …), fmt prints the canonical form. Real parse errors
still block fmt.

Idempotence: **`fmt(fmt(x)) == fmt(x)`** on every `examples/*.jet` and
`tests/ui/*.fixed.jet` (`tests/fmt.rs`).

## M6 phase 2 — `jet test` + `jet new` (done)

**`@test fn name { … }`** (S43, S82) — top-level blocks only. Bodies parse like a
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

Example: `examples/features/20_tests.jet`. Goldens: `examples/features/expected/20_tests.test.out`,
`tests/jet_test.rs`, `tests/fixtures/test_fail.jet` + `.fixed.jet`.

**NixOS / flake:** `nix develop` provides `cargo`, `rustc`, `gcc`, `nodejs`,
and a **`jet`** wrapper around `target/debug/jet`. **`cargo build`** once, then
`jet run …` / `jet lsp` / `cargo test --test lsp`. Editor setup:
`editors/vscode/README.md`. Release binary: `nix build .#jet`.

## M7 — Rust FFI (`extern rust`, done)

**`extern rust "crate@version" { … }`** (S50) declares foreign functions. Each
entry is a normal Jet signature plus **`= "rust::path"`** naming the target
item. This source-level declaration is sufficient even inside a project with
`pack.jet`; users do not need the package manager just to call a foreign
function. **`extern rust "std" { … }`** works for standard-library items with
no extra dependency. Non-`core` crates require an exact version pin (**E0701**).

Allowed boundary types pass **by value**: `Int`, `Float`, `Bool`, `String`,
`Char`, `List`/`Map`/`T?`/`T ? E` built from allowed types, and
structs/enums whose fields are allowed. No `mut`/`take`/`view` parameters, no
borrowed returns, no callbacks (**E0702**).

When any crate dependency is needed, the driver builds a hidden cached cargo
bridge under `~/.cache/jet/ffi/` and links it into the generated program (R9:
the user's folder never grows a manifest). Missing **`cargo`** → **E0703**;
fetch/build failures → **E0704** (cargo output in an indented block); a wrong
foreign path or signature → **E0705**. Panics inside foreign code are caught
at the boundary and become the M4 runtime report (exit 70).

Teaching: **`unsafe`** / C-style FFI spellings → **`extern rust`** (**E0031**).

Example: `examples/features/22_ffi.jet` (`base64@0.22`). Ui: `tests/ui/ffi_*.jet`.
Integration: `tests/ffi.rs` (gated on `cargo`).

## E2-M14 — C FFI (implemented: overlay + merge + link; bind backend deferred)

**S59** — C import with auto-generated bindings (default) and optional user
overlay. (Full spec follows in this section.)

| Layer | Shape |
|---|---|
| Autogen | `@bindgen module c.<lib>.__bindgen__ { … }` in `.jet/bindings/c/<lib>.jet` |
| Overlay | `@extern module c.<lib> { … }` — empty `{ }` = no overrides |
| Call site | `use "header.h" as alias` or `use c.<lib> as alias` (one per lib per file) |

Function bodies mirror Rust FFI: `fn init_window(w: Int, h: Int, t: String) =
"InitWindow";` (the string is the C linker symbol). On any C `use`, the compiler
loads the bindgen cache at `.jet/bindings/c/<lib>.jet` (when present), merges the
user overlay over it (**effective module = bindgen ∪ overlay; overlay wins**;
incompatible re-declaration → **E3205**), and materializes one synthetic module
so calls resolve like any namespaced module call. Codegen emits an `extern "C"`
block plus small per-function wrappers (the only place compiler-vetted `unsafe`
is emitted, S58); `String`↔`*const c_char` and `Char`↔`u32` convert at the edge.

Link key = last segment `<lib>`: hangar dep (`[dependencies:c]` in `pkg.jet`)
if declared → else `pkg-config <lib>` → **E3201**. Link flags (`-L native=…`,
`-l <lib>`) are resolved at **build time** (not during front-end checking, I3) and
threaded into the `rustc` link line. By-value scalars/`String`/C-layout
structs+enums at the edge; aggregates (`[T]`, maps, `T?`, tuples, …) → **E3203**;
pointers require `use core.mem` + `@unsafe` (E2-M13) → **E3202** (registered;
unreachable until the pointer tier lands). `@bindgen` is legal only inside a
generated cache file (**E3207**); users may not name the reserved `__bindgen__`
segment (**E3206**); two `use` forms for one lib in one file → **E3204**.

`jet bind <header.h> --pkg <lib>` is the manual cache-refresh entry point and
shares the compile-time auto-bind backend. The header→Jet translator (D-CBIND3
bindgen helper) is not built into this binary yet, so both auto-bind and
`jet bind` report **E3208** with the hand-written-overlay workaround; ship a
hand-written `@bindgen` cache or `@extern module` in the meantime. Rust FFI
(S50) is unchanged. Diagnostics: **E3201–E3208** in diagnostics.md with
snapshots (front-end ones under `tests/ui/cffi_*`; link-time/gated ones pinned in
`tests/cffi.rs`).

## E2-M13 — Expert low-level tier (S58, implemented)

C/Zig-class control behind two explicit gates; ordinary Jet never reaches it and
emits **zero** `unsafe` (the I1 amendment, D-LL1, recorded in `architecture.md`).

- **Discovery gate** — `use core.mem;` unlocks the low-level vocabulary (`Ptr<T>`,
  `mem.volatile_read`, `mem.address_of`, allocators). Naming one of these without
  the import → **E3102**.
- **Audit gate** — `@audit("reason")` on the line above an `@unsafe { … }` block
  opens the operations that can violate memory safety (pointer build/deref,
  volatile access). A missing audit is lint **L3101**. `@unsafe` on the line
  before `fn` marks a whole-function contract; its body is itself an audited
  region, and calling it requires an enclosing `@unsafe` block → **E3103**.
- **Operations** — `mem.Ptr<T>.from_addr(addr)` builds a typed pointer from an
  `Int` address (`Ptr<T>` lowers to a Rust `*mut T`); `mem.volatile_read(p)`
  reads through it (lowers to `std::ptr::read_volatile`); `mem.address_of(x)` is
  inert (a plain address as `Int`) and legal outside a gate. Using a low-level op
  outside `@unsafe` → **E3101**.

Codegen stays dumb (I3): an `@unsafe { … }` region lowers straight to a Rust
`unsafe { … }`, an `@unsafe fn` to a Rust `unsafe fn`. All gating is decided in
sema. Diagnostics **E3101–E3104 + L3101** in diagnostics.md with snapshots
(`tests/ui/lowlevel_e310*`, `tests/ui/mem_arena_gate`, `tests/ui/mem_use_after_free`,
`tests/ui_lint/unsafe_missing_audit`); the audited end-to-end example is
`examples/features/48_lowlevel.jet`.

### Allocators (D-ALLOC1, D-ALLOC-C, D-ALLOC-D; ratified 2026-06-19, implemented)

Four allocators ship under `core.mem` — `Arena`, `Bump`, `Pool`, `Fixed` — all namespaced
under `core.mem.alloc` (D-ALLOC-C). No `#unsafe` needed; `use core.mem` is the discovery
gate (E3102). Constructors: `mem.Arena.new()` / `mem.Arena.new(capacity: N)` (D-ALLOC1);
allocate with `arena.alloc(value)` which returns the value. Two lifecycle verbs (D-ALLOC-D):
`reset()` keeps the backing buffer (cheap, arena is reusable), `free()` returns memory to
the OS. Use-after-reset/free → **E3104** (names the site). Example: `70_arena.jet`.

## M6 phase 3 — multi-file imports (done)

Two use forms (S16): **quotes = file path, no quotes = module.**
**`use "path/to/file";`** — quoted path to a `.jet` file, relative to
the using file's directory (`use "./lib";` for a sibling file;
default namespace = last path segment). **`use name;`** or
**`use core.fs;`** — unquoted module name (searches recursively from
the project root for `name.jet` or `name/{name,main}.jet`; `core` is a
compiler-exported module per S51). Optional **`as alias`** in both forms.

Cross-file access uses **`namespace.item`**; only **`pub`** items are visible from
other files (S18), including **`pub`** struct fields. The driver loads the import
graph, sema checks the whole program, codegen emits one Rust file with **`mod`**
blocks and `user_<module>_<name>` mangling (`main` stays `main`).

Diagnostics: **E0602** path escapes the project · **E0603** missing import ·
**E0604** import cycle · **E0605** private item · **E0606** ambiguous module.
Example: `examples/features/21_imports/` (three files; file import + `as alias`). UI
fixtures under `tests/ui/import_{escape,missing,cycle,private,private_field,ambiguous}/`.

**Library packages resolve through the same `use` (U17, D-LIB-USE A).** One
import concept covers files, modules, **and `library` packages**: once a
`library` package (U10) is realized — its source staged in the shared hangar
store by the `core` provider — `use <pkg>;` resolves to that staged tree and its
`pub` items are usable as `pkg.item` (S18). A realized library is simply found by
the same module resolver, with the hangar staging dir added as an extra search
root (the staged tree is searched exactly like the project tree or a path dep).
No new keyword, no `..` import, no special call form — it is an ordinary module
on the search path. An **`executable`** package goes on PATH, not `use`: naming
one in `use` is **E0982**. A package's **`kind` is inferred when omitted**
(D-ILE1): in a `pkg.jet` `packages:` block a bare `name` (no `: kind`), or a
package with no `pkg.jet` at all, resolves to `executable` when its source stages
a `bin/` or declares a top-level `fn main`, otherwise `library`; an explicit
`library`/`executable` always wins. Single-file `jet run`/`build file.jet` stays
executable-requiring (R9; E0101 if it has no `main`). A `library` dependency the project declares but hasn't
realized yet is **E0983** (run `jetpack build`) — `jet build`/`run` never realize
on demand, keeping them offline and deterministic, the same flow as pre-fetched
deps. Resolver: `Source/Loader.rs` (`collect_pkg_resolution`). Tests: `tests/lib_use.rs`
(offline realize → `use` → call) and `tests/ui/use_unrealized_library/`.

## Code module system (D-MOD1–4, done 2026-06-18)

Jet's module system is **Rust's, with two surface swaps**: the keyword is
`module` (not `mod`) and scoping uses `.` (not `::`). The `use "path" as alias`
form above stays as the ceremony-free single-file entry point.

**Declaration forms (D-MOD1).** `module math;` declares a file module — the
loader searches the using file's directory for `math.jet`, then `math/module.jet`;
neither found is **E0607**, both found is **E0606** (ambiguous). `module math
{ … }` is an **inline module** — its items live in the `math` namespace of the
containing file, no file lookup. `module` is shared with the JetOS declaration
(U3); the parser disambiguates by peeking past `{` (a code module body opens with
`fn`/`struct`/`pub`/… or `}`, a JetOS body with `sources`/`imports`/a
contribution path) and by the `;` form, which is always a code module.

**Access (D-MOD2).** Qualified `math.clamp(…)` always works. Optionally bring
items unqualified with `use math.clamp;` or a group `use math.{clamp, lerp};`.
Wildcards (`use math.*`) are rejected — **E0612**. Unqualified import of an
undefined item is **E0611**; of an item in a module not in scope, **E0610**.

**Visibility (D-MOD3).** Private by default; `pub` exports. A non-`pub` item is
unreachable from outside its file or inline module: `math.helper()` where
`helper` is private is **E0609** (inline) / **E0605** (cross-file). Inline-module
function bodies are fully type-checked, and a sibling call (`area` → `square`)
lowers to the module-mangled name (`geo__square`), so private siblings never
leak into the file's namespace or to rustc.

**Re-export (D-MOD4 — Rust-exact `pub use`).** A directory module's `module.jet`
exposes a submodule item only by re-exporting it: `pub use wrap.wrap;`. Nothing
auto-surfaces — a `pub`-but-not-re-exported item stays internal to the directory.
`text.wrap(…)` then resolves through the re-export to the defining module, with
the real function's borrow/move conventions preserved.

Examples: `examples/features/42_inline_module`, `43_module_file`,
`44_module_dir`, `45_module_use_unqualified`, `46_module_use_group`,
`47_module_reexport`, `48_module_file_use`, `49_module_inline_sibling`. UI
fixtures: `tests/ui/module_{missing,private,unknown_namespace,wildcard,
inline_private,inline_type_error}`.

## M6 phase 4 — `--small` + LSP v0 (done)

**`jet build --small`** (S15): `opt-level=z`, fat LTO, `panic=abort`, stripped symbols.
Smaller binaries than the default speed-oriented profile (`tests/small.rs` on
`examples/features/16_wordcount.jet`).

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

## M8 — Functions as values (closures, done)

**Lambdas (S46):** `(params) => expr` or `(params) => { … }`. Parameter types
may be omitted when the expected function type is known (**E0801** when not).
The lambda arrow is **`=>`**; **`->`** stays for return types and `when` arms.

**Function types (S47):** `fn(T1, T2) -> R` (no parameter names; `-> R` may be
omitted for no-return callbacks). Named `fn`s coerce to function values when
referenced without a call.

**Capture rules (S47):** shared read for names only read; mutable borrow for
names written (`var` required, else **E0111**). Escaping lambdas (stored in a
binding, returned, in a struct field, or passed to a `take` parameter) must own
captures: clonable values are copied (**L0801**); non-clonable values need an
explicit prefix **`take(name)`** on the lambda (**E0802**). Self-recursion through
the binding is rejected (**E0804**). Calling a non-function → **E0803**.

**Collection methods:** `map`, `filter`, `each`, `find`, `any`, `all`,
`sort_by`, `reduce` on `[T]`; `each` on `[K, V]` (two parameters).

Teaching: **`lambda`** / anonymous-fn spellings → `(x) => …` (**E0032**);
**`|x|`** pipes → `(x) => …` (**E0033**).

Examples: `examples/features/23_closures.jet`, `examples/features/24_callbacks.jet`. Ui:
`tests/ui/lambda_*.jet` (E0801–E0804, E0204 mut-capture conflict,
E0507 collection change inside a `for` loop), `tests/ui/not_a_function.jet`,
`tests/ui/foreign_{lambda,pipe}.jet`; lint: `tests/ui_lint/lambda_escape_clone.jet`
(L0801). Integration: `tests/closures.rs`.

## M10 — Standard library (done)

Full user-facing reference: **docs/reference/stdlib.md**.

M10 standard library modules are compiler-known namespaces backed by Rust std
helpers in the generated prelude. Import the short `core` spelling or the
canonical `jet.core` spelling:

```
use core.fs as fs;
use jet.core.json as json;
```

Implemented modules: `std.fs`, `std.io`, `std.env`, `std.process`,
`std.math`, `std.random`, `std.time`, and `std.json`. Unknown std modules are
**E1001**; local modules/import aliases may not shadow reserved first-party
roots (`core`, `jet`, `http`, `regex`, `csv`, `toml`, `crypto`, `archive`) —
**E1002**. Selective imports are rejected; keep qualified access through an
alias.

Fallible std functions return `T ? E` and must be handled with `?`,
`??`, or pattern tests like any M4 result. File APIs use whole-file helpers
only; file handles and streaming are out of scope. Paths are `String` in M10.
Binary APIs use `U8` and `[U8]`; integer literals for `U8` must be in
0..255 (**E1003**). Unknown items in a std module are **E1004** with a
did-you-mean suggestion when possible.

Receiver additions: `String.bytes() -> [U8]`,
`String.from_bytes([U8]) -> String ? UTF8Error`, `n.to_u8()`, and
`b.to_int()`. Time stays unix milliseconds (`time.now()`); random is
deterministic after `random.seed(n)`. JSON is dynamic (`JSON`) with
`json.parse`, `json.render`, and `json.render_pretty`. `jet.json` also exposes
`json.decode` (D-JSON1-decode + D-JSON3=B): a lenient variant that coerces
string values that look like numbers or booleans (`"8080"` → `8080`,
`"true"` → `true`) and emits one structured log line per coercion to stderr
naming the field and the from→to types. The decoded value comes back plain —
no wrapper type. Use `jet.json.decode` when consuming externally-produced JSON
that may encode numbers or booleans as strings.

Codegen invariant: importing std modules is free; sema records reachable std
calls and codegen emits only those helpers (R10).

Program arguments: `jet run file.jet -- arg1 arg2` forwards everything after `--`
verbatim to the program; `io.args()` sees them as argv[1..]. An unknown `--`-flag
before `--` is an error (E2102) that teaches the `--` form (D-CLI1=A). Plain
positional words with no separator still work (`jet run greet.jet Ada`). `jet test`
also accepts `--`; `jet build` does not (no running process).

Examples: `examples/features/29_files.jet`, `examples/features/30_json.jet`,
`examples/features/31_cli.jet`, `examples/features/64_cli_args.jet`. UI: `tests/ui/std_*`,
`tests/ui/u8_out_of_range.jet`, and M10 teaching errors **E0037**–**E0039**.

## E2-M1 — Concurrency (tasks and channels, verified 2026-06-14)

`std.tasks` provides blocking tasks and typed channels. Import it as a normal
std module:

```jet
use core.tasks as tasks;
```

`tasks.spawn(() => work()) -> Task<T>` starts a task from a zero-parameter
lambda. The lambda must own every captured value: shared mutable captures are
**E1101**; use `take(name)` to hand a value to the task, or use a channel to
send results back. Values crossing the task boundary must be sendable
(**E1102**): no `view` borrows, no structs that contain `ref` fields, no trait
values, and no closures unless handed over with `take`.

`task.join() -> T` waits for the task and consumes the `Task<T>` handle. Calling
`.join()` twice is ordinary use-after-move (**E0121**). Dropping a `Task`
without joining emits **L1101** because the program may end before the task
finishes. A panic inside a task is reported when joined and exits with the
runtime panic code.

`tasks.channel<T>() -> Channel<T>` creates a receive half. `ch.sender() ->
Sender<T>` creates a clonable send half. `sender.send(value)` moves a `T` into
the channel (`take` semantics for non-copy values), and `ch.receive() -> T or
Closed` blocks until a value arrives or all senders are gone. Channel payloads
must be sendable (**E1102**).

Teaching errors: **E0040** points `async`/`await` users at `tasks.spawn`;
**E0041** points `Mutex`/`lock` users at channels.

## Modules — `module name { … }` (U3, unified-ecosystem §4–5; parser, Stage 1a)

A module is a named, composable top-level declaration that contributes typed
values to reserved namespaces. Many modules may share a file.

```ebnf
module      = "module" dashed-name "{" contribution* "}" ;
contribution = namespace "." dashed-name ":" expr [","] ;
namespace   = "env" | "system" | "image" ;
dashed-name = ident { "-" ident } ;                (* S84: kebab-case names *)
```

- **Dashed names (S84):** package / module / system / image / env **names** may
  be kebab-case — `module web-app`, `system.my-host`, `image.halcyon-iso` —
  matching nixpkgs/npm convention. A `-` joins two segments only when it is
  *span-adjacent* to both (no surrounding whitespace), so a spaced `a - b` stays
  subtraction; this is a parser rule (`expect_dashed_name`), not a lexer or
  expression-grammar change. Code identifiers (variables, fields, types,
  functions) stay plain `ident`. No leading, trailing, or doubled hyphen.
- **Disable with a leading underscore:** `module _name { … }` parses with
  `disabled = true` (the name begins with `_`); it is not discovered or merged
  (U3, one-character reversible toggle).
- **Reserved namespaces** are `env` → `Env` (dev environment), `system` →
  `System` (whole machine), `image` → `Image` (disk image). Any other namespace
  is **E0960** (parse).
- **`env.<name>:` values reuse the ordinary expression parser** — typically a
  struct literal (`Env { packages: […], prompt: "…" }`), so lists and strings
  work with no new grammar.
- **`system.<name>:` and `image.<name>:` values use dedicated typed parsers**
  (U11/U13/U14) — the `options` list (`net.hostName: laptop`), the typed
  `target` value (`linux.x64`), and the `Service` map don't fit the ordinary
  expression grammar.

Stage 1a is parser-only for the AST shape; the jetpack module evaluator
(`Source/Jetpack/ModuleEval.rs`) gives these contributions meaning (field-checking +
capture into a plan model). The U5 merge engine consumes `env` contributions.

### `System` / `Service` / `Image` (U11–U14, U18; modeval field-check + capture)

```ebnf
system_lit  = [ "System" ] "{" system_field { "," system_field } [ "," ] "}" ;
system_field = "target"   ":" platform
             | "packages" ":" list
             | "services" ":" service_map
             | "options"  ":" option_list ;
platform    = ident "." ident ;                    (* U13: linux.x64 / linux.arm64 *)
service_map = "{" { ident ":" service_rec [ "," ] } "}" ;
service_rec = [ "Service" ] "{" { ident ":" expr [ "," ] } "}" ;
option_list = "[" { dotted_key ":" expr [ "," ] } "]" ;
dotted_key  = ident { "." ident } ;
image_lit   = [ "Image" ] "{" image_field { "," image_field } [ "," ] "}" ;
image_field = "from"   ":" "system" "." dashed-name  (* U14: required; S84 name *)
            | "format" ":" ident                   (* U14: iso | qcow | raw, default iso *)
            | "target" ":" platform ;              (* U14: cross-compile only *)
```

- **U11 — `System` fields.** A `System` has exactly four fields: `target` (a
  typed platform value, required), `packages` (a `Pkg` list, U6 sugar applies),
  `services` (a keyed `Service` map), and `options` (an ordered key/value list).
  Any other field is **E0972**; a missing `target` is **E0974**.
- **U13 — `target` & `options`.** `target` is a typed platform value
  (`linux.x64` / `linux.arm64`), never a quoted string — an unknown platform is
  **E0973**. `options:` is an ordered **list** of dotted-key `key: value` entries
  (`net.hostName: laptop`, `time.timeZone: "Europe/London"`) — no `set(…)`
  wrapper. Values that are jet identifiers or typed values are written bare; only
  free-form strings (timezones, paths) keep quotes.
- **U12 — `Service` is an open record.** Each service under `services:` is a bare
  `{ … }` (type inferred, U18) whose first field is `enable: Bool` (required —
  missing is **E0975**, non-Bool is **E0975**); any further fields are allowed.
- **U14 — `Image` derives from a `System`.** An `Image` has `from: system.<name>`
  (required — missing is **E0977**; an unknown system is **E0978**) and an
  optional `format:` ∈ {`iso`, `qcow`, `raw`}, default `iso` (anything else is
  **E0976**). `packages`/`services`/`options` are inherited from the system and
  must not be restated (**E0977**); only `target:` may be restated, for
  cross-compiling.
- **U18 — inferred constructors.** Under a typed namespace (`system.<name>:`,
  `image.<name>:`, `env.<name>:`) or a typed field (`services:` holds `Service`s)
  the type name is optional: a bare `{ … }` elaborates to it. The explicit
  `System { … }` / `Image { … }` / `Service { … }` / `Env { … }` form stays legal.

The evaluator captures each `system.<name>:` into a `SystemPlan` and each
`image.<name>:` into an `ImagePlan` (`Source/Jetpack/ModuleEval.rs`), carried on
`EnvPlan` so the jetos realize tier can consume them; the dev-shell path ignores
them.

### `jetpack os <verb> [<config-path>]@<host>` — the jetos tier (U15/U16)

Whole-machine management (the jetos tier) is a **subcommand group of `jetpack`**,
not a separate `jetos` binary and not part of the `jet` tool (**U15**). Two verbs,
mirroring `nixos-rebuild`:

- `jetpack os build [<config-path>]@<host>` — realize the system into a generation.
- `jetpack os switch [<config-path>]@<host>` — build, then **activate** it.

```ebnf
os_command = "jetpack" "os" os_verb os_target ;
os_verb    = "switch" | "build" ;
os_target  = [ config_path ] "@" host ;            (* U16; split on the FINAL @ *)
```

- **U16 — the `@host` target.** The target is `[<config-path>]@<host>`. Everything
  after the **final** `@` is `<host>` (so a path may itself contain `@`); the
  optional prefix names the config file. The `@host` selector picks which captured
  `System` to apply and is **required** (no selector → **E0979**). An empty path
  defaults to `~/.jet/config.jet`. A config file that doesn't exist → **E0981**; a
  `<host>` no `system.<name>:` contribution defines → **E0980** (the error lists
  the systems the config does define).

The config is loaded through the **same** typed-module path as `env.jet`
(`modeval::evaluate_env`), so `System` field-checking + capture is reused verbatim.

**Activation model (internal mechanics, not user-facing syntax).** Each build
realizes the selected system's packages through the existing provider boundary
into the shared hangar (codegen/realize stays dumb, I3), then assembles a
content-addressed **generation directory** under `<root>/systems/<host>-<fp>/`
holding a `manifest.json` (target, realized packages, services, options).
Services/options are recorded as **intent** — never started, there is no daemon
yet (D-OS2..D-OS6 remain open). `switch` additionally flips two pointers under
`<root>/systems/`: `current` (the active generation) and `default` (the boot
default). Store layout and symlinks are internal mechanics, so they need no
syntax ratification; boot/test verbs are a later chunk.

## Fan-out operator `f.[a, b, c]` (S75) and fixed-size list `[T#N]` (S76)

### Fan-out `f.[a, b, c]`

`f.[a, b, c]` is syntactic sugar that expands to `[f(a), f(b), f(c)]`. The
callee `f` must be a one-argument function; each item is type-checked against
`f`'s parameter type. The result type is `[R#N]` where `R` is `f`'s return
type and `N` is the number of items.

```ebnf
fan_out = expr ".[" [ expr { "," expr } [ "," ] ] "]" ;
```

```jet
fn double(n: Int) -> Int { return n * 2; }

val doubled = double.[1, 2, 3];  // : [Int#3]  →  [2, 4, 6]
```

Errors: **E0961** if the callee is not a one-argument function; **E0962** if an
item's type doesn't match the parameter type.

### Fixed-size list `[T#N]`

`[T#N]` is a type refinement meaning "a list of exactly N elements of type T."
It is produced by fan-out and can be destructured with an exact-count pattern.
At codegen it erases to `Vec<T>` (same as plain `[T]`).

```ebnf
type_fixed_list = "[" type "#" int_literal "]" ;
```

```jet
val result: [Int#3] = double.[1, 2, 3];
val [a, b, c] = result;   // OK — 3 names for 3 elements
```

- Destructuring a `[T#N]` with the wrong number of names is **E0963**.
- Calling `push`, `pop`, `insert`, `remove`, or `clear` on a `[T#N]` is **E0964**.
- A literal index outside `0..N-1` on a `[T#N]` is **E0965** (compile-time check).
- `[T#N]` is accepted wherever `[T]` is expected (widening coercion); the
  length information is erased at that point.

## Editions & release policy (E2-M2)

A project pins an **edition** with `edition: "2026"` in its `pkg.jet`
(D-REL3). An edition opts the project into a specific era of Jet syntax; the
toolchain advertises the editions it supports in `jet --version` and rejects a
future edition it can't provide (E2001). Single-file `jet run file.jet` carries
no edition marker and always uses the newest stable edition (E2-V4). The full
compatibility contract — patch/minor/major/epoch/edition definitions, the
backward-compatibility guarantee, the deprecation window (L2001 → E2002), the
migration authority (only `jet fix` + edition upgrade, D-REL5), and the
generated-code license statement — lives in docs/spec/release-policy.md.

## Deliberately absent

See non-goals in docs/spec/philosophy.md. The parser should produce staged
or guiding errors for the ones users will reach for (e.g. `and` → teaching
error naming `&&`, per S14).
