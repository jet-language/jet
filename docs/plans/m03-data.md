# M3 — Data: structs, enums, Option, patterns

**Blocked on decisions:** none (Group 2 ratified: S29–S33).
S27 (methods) is already ratified.
**Error codes:** E0301+ / L0301+. Teaching codes E0020+ (E0019 is S16 staged import).
**Retires:** E0117 (fields/methods staged).

## Goal

User-defined value types. Structs hold named fields; enums are sum types
with optional payloads; `switch` becomes exhaustive over enums; absence
is `Option` (no null, ever). Methods per S27. Everything stays tier 1:
struct/enum values move like any other value; no stored references
(except the existing tier-2 `ref` field machinery), no user generics.

## Surface (examples use ballot recommendations — substitute ratified choices)

```jet
struct Point {
    x: Float;
    y: Float;

    fn dist_from_origin(self) -> Float {
        return (self.x * self.x + self.y * self.y).sqrt();
    }
}

enum Shape {
    Circle(Float);              // S30: one field → positional type only
    Rect(w: Float, h: Float);  // two or more → named fields
    Empty;
}

fn describe(s: Shape) -> String {
    switch s {
        s == Circle(r) -> { return "circle, radius {r}"; };
        s == Rect(w, h) -> { return "rect {w} by {h}"; };
        s == Empty -> { return "nothing"; };
    }
}

fn find_even(limit: Int) -> Int? {
    for i in 1..limit {
        if i % 2 == 0 { return value(i); };
    };
    return null;
}

fn main() {
    val p = Point { x: 3.0, y: 4.0 };
    print(p.dist_from_origin());
    val s = Shape.Circle(2.0);
    print(describe(s));
    if find_even(9) == value(n) { print(n); };
}
```

### Grammar additions (EBNF, extends docs/01)

```
program   = { func | structdef | enumdef | const } ;
structdef = [ "pub" ] "struct" ident "{" { field | func } "}" ;
field     = [ "pub" ] [ "ref" ] ident ":" type ";" ;
enumdef   = [ "pub" ] "enum" ident "{" { variant | func } "}" ;
variant   = ident [ "(" variantpayload ")" ] ";" ;
variantpayload = type                              // S30: single-field
               | ident ":" type { "," ident ":" type } ;  // multi-field
impldef   = "impl" ident "{" { func } "}" ;          // S27
type      = ident | type "?" ;                        // S32: T? is Option
expr      += structlit | enumlit | fieldaccess | methodcall
           | "value" "(" expr ")" | "null"
           | expr "==" pattern ;                     // S31: pattern == test
structlit = ident "{" ident ":" expr { "," ident ":" expr } "}" ;
enumlit   = ident "." ident [ "(" args ")" ] ;        // Shape.Circle(2.0)
pattern   = ident [ "(" ident { "," ident } ")" ]     // variant + bindings
          | "value" "(" ident ")" | "null" ;
```

Parser notes:
- `ident {` after `=`/`(`/`,`/`return` etc. is a struct literal; after
  `if`/`while`/`switch`/`for…in` heads it is NOT (the `{` opens the
  block). Same disambiguation Rust uses; implement as a "no struct
  literal in condition position" flag threaded through expression
  parsing. Parenthesized form `if (Point { … }).x > 0` always works.
- **`==` pattern tests (S31):** when the left operand is an enum or
  `T?` and the right is a pattern form, `==` is a variant/absence test
  (not value equality). Otherwise `==` is ordinary equality (S13).
- Methods inside the type body and `impl Type { }` blocks parse to the
  same AST node list (S27); sema treats them identically.

## Sema rules

1. **Registration pass.** Collect all struct/enum/impl names before
   checking bodies (forward references between types must work).
   Duplicate type names → E0105 (existing). Type/function name clash →
   E0105. `impl` for an unknown type → E0301.
2. **Fields.** Construction must supply every field exactly once, no
   extras, right types (E0302 unknown field, E0303 missing/duplicate
   field, with field names listed verbatim). Field access `v.f` on a
   non-struct or unknown field → E0302 with did-you-mean. Private
   fields: cross-file enforcement waits for M6; within one file all
   access is allowed (S18).
3. **Enums (S30).** Single-payload variants: positional type in the
   declaration, positional arg at `Type.Variant(expr)`. Multi-payload:
   named fields in the declaration and **required labels** at the call
   site (`Shape.Rect(w: 1.0, h: 2.0)`). `Shape.Circle(…)` checks
   variant exists (E0304) and payload arity/types. Enum values move;
   clonable iff all payloads clonable.
4. **`==` patterns (S31).** Left side must be an enum or Option value
   whose type owns the named variant (E0305). Payload binding names are
   fresh immutable bindings (`val`), scoped to: the arm block (in
   `switch`), the `if` body (in `if x == p { }`), or the right of `&&`
   in a condition. Binding count must match payload arity (E0306).
5. **Exhaustiveness.** A `switch` whose subject is an enum/Option and
   whose arms are all `subject == <pattern>` tests may omit `else` iff
   every variant is covered; otherwise E0307 lists the missing variants
   verbatim. Mixed condition/pattern arms keep S24's mandatory `else`.
   A variant arm after full coverage → lint L0301 (unreachable arm).
6. **Option (S32).** `T?` is the only optional type spelling.
   **`value(e)`** / **`null`** are the present/absent spellings. Bare
   `null` needs a context
   type or annotation (E0308). `T??` rejected (E0309). Using a `T?`
   where `T` is expected → E0310 with fix: test with `==`.
7. **Methods (S27).** `self` access prefixes mirror parameters and feed
   the existing M2 ownership checker: default→shared, `mut self`→mutable
   (call site needs a `var`/`mut` receiver, reuse E0202), `take self`→
   consumes the value (use-after-move reuses E0121). Methods without
   `self` are static: called `Type.name(…)`; calling a static through a
   value or an instance method through the type → E0311. Method name
   can't collide with a field (E0105).
8. **Recursive types.** A struct/enum that contains itself (directly or
   via a cycle) is legal: sema marks the minimal set of edges that break
   each cycle and codegen inserts `Box<…>` there invisibly. Users never
   see boxing; no error, no syntax.
9. **Printing & equality.** Every struct/enum is printable. `==`/`!=`
   on values work iff all fields support them; otherwise E0312. Pattern
   `==` is separate from value `==` (rule 4). No `<`/`>` on user types
   in M3.

## Codegen lowering

| Jet                          | Rust                                        |
|------------------------------|---------------------------------------------|
| `struct Point { x: Float; }` | `struct user_Point { user_x: f64 }` + generated `impl Display`, `impl Clone` (iff clonable), `impl PartialEq` (iff comparable) |
| `enum Shape { Circle(Float); Rect(w: Float, h: Float); }` | `enum user_Shape { Circle(f64), Rect { user_w: f64, user_h: f64 } }` (+ `Box` where needed) |
| `Point { x: 1.0 }`           | `user_Point { user_x: 1.0 }`                |
| `Shape.Circle(2.0)`          | `user_Shape::Circle(2.0)`                   |
| `Shape.Rect(w: 1.0, h: 2.0)` | `user_Shape::Rect { user_w: 1.0, user_h: 2.0 }` |
| `T?` / present / absent      | `Option<T>` / `Some` / `None` (spellings per S32) |
| `x == Circle(r) -> { … };`   | `match` arm `user_Shape::Circle(r) => { … }` |
| `if x == value(n) { … }`     | `if let Some(n) = x { … }`                  |
| `if x == null { … }`         | `if x.is_none()` / `matches!(x, None)`      |
| methods / `impl`             | one merged `impl user_Point { … }`; receivers `&self` / `&mut self` / `self` |

A `switch` that is fully patterns lowers to a native Rust `match` on the
subject. Mixed switches keep the M1 if/else-chain lowering with `==`
pattern tests becoming `matches!`/`if let` chains.

## Diagnostics to register (docs/04)

E0301 `impl` for unknown type · E0302 unknown field (suggestion) ·
E0303 construction missing/duplicate/extra fields (lists names) ·
E0304 unknown variant (suggestion) · E0305 pattern doesn't belong to
this value's type · E0306 pattern binding count mismatch ·
E0307 switch not exhaustive (lists missing variants) · E0308 absent
literal needs a known type · E0309 nested Option rejected · E0310 Option
used where plain value expected · E0311 static/instance method confusion ·
E0312 value `==` unsupported because of field X · L0301 unreachable switch arm.
Teaching: E0020 `nil`/`None`/`Some`/`none`/`some` → `null`/`value` · E0021
`class` → `struct` · E0022 `interface`/`trait` staged → M9 · E0023
`case`/`default` inside switch → arm syntax.

## Examples & tests

- `examples/10_structs.jet` — shapes with methods, static constructor,
  printing; expected output pinned.
- `examples/11_enums.jet` — traffic-light state machine driven by an
  exhaustive switch.
- `examples/12_option.jet` — search returning `Int?`, handled with `==`.
- ui fixtures for every E03xx/L0301 + the four teaching errors, each
  errorful fixture with a `.fixed.jet` companion (pattern from M2).
- Ownership interaction tests: struct moves, `take self` consume,
  L0201 implicit clone of a struct argument.
- Golden tests prove rustc accepts everything sema passes, including a
  recursive enum (auto-box) case.

## Out of scope (do not implement)

Traits (M9), user generics (M9), `List`/`Map` (M5), tuple types, struct
update syntax (`..base`), pattern guards beyond plain `&&` conditions,
nested patterns (`value(Circle(r))` — one level only in M3), visibility
enforcement across files (M6), operator overloading (never).

## Suggested implementation order

1. syntax.rs entries (`enum`, `value`/`null`, `?`-type) per S31/S32.
2. Parser: type defs + struct literal disambiguation + `==` pattern RHS.
3. Sema registration pass + field/variant checks (S30 rules) + Option typing.
4. `==` pattern binding scopes + exhaustiveness.
5. Methods/receivers wired into the M2 ownership checker.
6. Recursion detection + box marking.
7. Codegen + golden examples; derive Display/Clone/PartialEq.
8. Teaching errors; re-bless snapshots; update docs/01, docs/04, docs/05.
