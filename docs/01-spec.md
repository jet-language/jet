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
error naming the milestone (E0006 `?` → M4).
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
**`Type[T]`** (S33). `null` is only legal for `T?`, never plain `T`.

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
- **Traits/interfaces (S28)** are deferred; no `trait` / `implements` syntax yet.

## Deliberately absent

See non-goals in docs/00-philosophy.md. The parser should produce staged
or guiding errors for the ones users will reach for (e.g. `and` → teaching
error naming `&&`, per S14).
