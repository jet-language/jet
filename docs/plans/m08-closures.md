# M8 — Functions as values (closures)

**Blocked on decisions:** S46 (lambda syntax), S47 (function types &
capture rules). Depends on M5 (the collection methods this unlocks).
**Error codes:** E0801+ / L0801+.

## Goal

Functions become values: lambdas, function-typed parameters, and the
collection methods everyone expects (`map`, `filter`, `each`, …).
Capture follows tier-1 ownership — no new mental model, the M2 rules
applied to a new construct.

## Surface (uses ballot recommendations — substitute ratified choices)

```jet
fn apply_twice(f: fn(Int) -> Int, x: Int) -> Int {
    return f(f(x));
}

fn main() {
    val double = (x: Int) => x * 2;          // lambda (S46)
    print(apply_twice(double, 5));           // 20
    print(apply_twice((x) => x + 1, 5));     // 7 — param type inferred

    val nums = [1, 2, 3, 4];
    print(nums.map((n) => n * n).filter((n) => n > 5));
    nums.each((n) => { print(n); });         // block body form

    var total = 0;
    nums.each((n) => { total += n; });       // mutable capture
    print(total);
}
```

- Lambda: `(params) => expr` or `(params) => { stmts }`. Single
  expression form returns it; block form follows normal return rules.
  Parameter types optional when inferable from context (E0801 when not).
- Function type: `fn(T1, T2) -> R` (no parameter names); `-> ()` omitted
  like regular functions. Named `fn`s coerce to function values when
  referenced without calling (`apply_twice(double_fn, 5)`).
- **Capture rules (S47):** a lambda captures by the M2 defaults —
  shared read for names it only reads, mutable for names it writes
  (the enclosing binding must be `var`, else E0111). A lambda that
  **escapes** (is returned, stored in a struct/list, or passed to a
  `take` parameter) must own its captures: clonable captures are cloned
  with lint L0801 (mirror of L0201), non-clonable ones require
  `take name` in a capture list prefix: `take(name) (x) => …` — exact
  spelling per S47 ballot; non-clonable + no take → E0802.
- New `List[T]` methods (the v1 closure set, nothing more):
  `map(f) -> List[U]`, `filter(f) -> List[T]`, `each(f)`,
  `find(f) -> T?`, `any(f)`, `all(f)`, `sort_by(f)` (f returns the
  comparison key), `reduce(init, f)`. `Map`: `each(f)` over `(k, v)`.

### Grammar additions

```
type   += "fn" "(" [ type { "," type } ] ")" [ "->" type ] ;
expr   += lambda | expr "(" args ")" ;        // call-position on any expr
lambda  = [ capturespec ] "(" [ lparams ] ")" "=>" ( expr | block ) ;
lparams = ident [ ":" type ] { "," ident [ ":" type ] } ;
```

`=>` is a new token (S46); keep `->` for return types and switch arms —
the two never overlap, document the distinction in docs/01.

## Sema rules

1. Lambda parameter/return types unify with the expected function type
   at the use site; unconstrained and unannotated → E0801 ("tell me the
   type of `x`: write `(x: Int) => …`").
2. Capture analysis runs as part of the M2 checker: each captured name
   gets a borrow (shared or mut) spanning the lambda's *lifetime of use*.
   Non-escaping lambdas (called only within the statement/expression
   they're passed to — true for all M8 collection methods and
   function-typed params without `take`) borrow exactly like a nested
   block; conflicts reuse E0204/E0507 wording.
3. Escape analysis: returned/stored/`take`-passed lambdas trigger the
   ownership-capture path (L0801 clone / `take(...)` list / E0802).
4. Calling a non-function value → E0803 (with the value's type).
   Arity/type errors at call sites reuse E0104/E0112.
5. Recursion through a lambda binding is rejected in v1 (E0804 — "a
   lambda can't call itself; write a named `fn`").
6. Function values are not comparable or printable (E0312/E0112 paths).

## Codegen lowering

| Jet                          | Rust                                   |
|------------------------------|----------------------------------------|
| `fn(Int) -> Int` (param type)| generic `F: Fn(i64) -> i64` / `FnMut` when captures are mut — sema records which |
| `fn(Int) -> Int` (stored/returned/field) | `Box<dyn Fn…>` (boxing invisible, like M3 recursion boxes) |
| `(x) => x * 2`               | `move |x| x * 2` (always `move`; sema already inserted clones for shared captures so `move` is safe) |
| named fn as value            | path to the mangled fn item            |
| `nums.map(f)`                | prelude helper `jet_list_map(nums, f)` returning a fresh Vec (eager, no iterators exposed) |

Always emitting `move` + explicit clones keeps codegen dumb (R1): sema
decides what's cloned/taken; codegen never reasons about lifetimes.

## Diagnostics to register

E0801 lambda parameter type unknown · E0802 escaping lambda captures a
non-clonable value without `take` · E0803 calling something that isn't a
function · E0804 self-recursive lambda · L0801 escaping lambda silently
cloned a capture.
Teaching: E0032 `lambda`/`fn(x) {…}` anonymous-fn spellings → `(x) => …` ·
E0033 `|x| …` Rust pipes → `(x) => …`.

## Examples & tests

- `examples/19_closures.jet` — map/filter/reduce pipeline + sort_by.
- `examples/20_callbacks.jet` — function-typed params, a stored callback
  in a struct field (exercises boxing + capture cloning).
- ui fixtures for all E08xx/L0801 + teaching errors with fixes.
- Ownership torture fixtures: mut capture conflicting with outer use
  (E0204 wording), lambda mutating the list it's mapping (E0507).
- Golden: rustc accepts every passing fixture (Fn/FnMut/Box inference is
  where sema soundness bugs will hide — be generous with cases).

## Out of scope

Currying/partial application, generic lambdas, `FnOnce`-style one-shot
semantics surfaced to users, method references (`xs.map(Point.dist)` —
write a lambda), lazy iterator chains, async. Custom comparator beyond
`sort_by` key extraction.
