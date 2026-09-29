# #3159 — Type-indexed variant results on a concrete typed AST

Closer09, 2026-09-29. Binary: `~/.cache/jet-luna/safe-jet.sh` (jet-current =
`jet-debug-snapshot14`, moved to `snapshot16` during the session; the defect
repros were re-run on 16, see the end). Source head `e9c708fa7`.

## Question

Can current Jet express a typed expression AST (integer and boolean nodes
`Lit`, `Add`, `Eq`, `If`) whose evaluator's result type follows the node type,
and reject ill-typed nodes before runtime, without GADTs? If a real gap
survives, return one owner ballot.

## Method

Scratch package `~/.cache/jet-test-scratch/Closer09/free/` (no authority
budget). Files:

- `typed_ast.jet`: three encodings of the same two programs
  (`if 1 + 2 == 3 then 10 + 5 else 0`, `if 2 == 2 then 4 == 5 else true`):
  (a) one enum per result type (`IntExpr`, `BoolExpr`), total evaluators
  `eval_int`/`eval_bool`; (b) phantom `Expr<T>` wrapper over one untyped
  `Node` enum with smart constructors `int_lit`, `bool_lit`, `add`, `eq`,
  `cond<T>` and an untyped evaluator with a `Stuck` arm; (c) open trait
  encoding (`trait IntTerm { fn value(self) -[]> Int }`, `trait BoolTerm`,
  one struct per node with trait-typed children).
- `typed_ast_ab.jet`: the same file with (c) removed.
- `typed_ast_ill_typed.jet` (the planned `tests/ui/typed_ast_ill_typed.jet`):
  for (a) and (b), `Add` over `Bool`, `If` with an `Int` condition, and
  mismatched branches; plus one forged `Expr<Int>{node: Node.BLit(true), ..}`.

Commands: `run3.sh <file> check,run,interp,aot` (= `safe-jet.sh check`,
`safe-jet.sh run`, `safe-jet.sh run --interpret`, `safe-jet.sh build` then the
produced `.jet/build/<name>` binary).

## Evidence

### Execution (typed_ast_ab.jet, encodings a and b)

| tier | result |
|---|---|
| `jet check` | exit 1, E2104 cost projection: `IntExpr.compare`, `Node.compare`, `Node.equal` "sema-checked callable is outside typed TIR coverage" |
| `jet run` | exit 0, `a: 15 false` / `b: 15 false` (correct) |
| `jet run --interpret` | exit 0, same output |
| AOT `jet build` | exit 101: rustc E0308 x44 in generated Rust, e.g. `IntExpr::__jet_Add { __jet_a: … }` "expected `Box<IntExpr>`, found `IntExpr`" (recursive named-payload variants are not boxed at construction) |

Encoding (c) (`typed_ast.jet`): `run`, `--interpret` and AOT all exit 101 with
`internal compiler error: checked TIR cannot lower to MIR … missing checked
function target IntIf::value`. Reduced repros below (D1, D2, D5).

### Rejection before runtime (typed_ast_ill_typed.jet, `jet check`)

| node | (a) per-type enums | (b) phantom `Expr<T>` |
|---|---|---|
| `Add` over `Bool` | **accepted** (no diagnostic) | E0112 `add wants Expr<Int> … this is Expr<Bool>` (45:18) |
| `If` with `Int` condition | **accepted** | E0112 `cond wants Expr<Bool> … this is Expr<Int>` (46:18) |
| mismatched branches | **accepted** | E0904 `T is Int from argument 2 but Bool from argument 3` + E0112 (47:17, 47:50) |
| forged `Expr<Int>{node: Node.BLit(true)}` | n/a | accepted (by design: struct literal bypasses the smart constructors) |

`ill_a_only.jet` (the (a) rows alone) gives only E2104, no type error. The
cause is defect D3: named-payload variant construction does not check field
types at all (`Pair.Ints{a: "not an int", b: true}` passes `jet check`, and
`jet run` prints `33`).

### Encoding cost

| encoding | rejected before runtime (intended) | evaluator needs runtime arm / cast | shape |
|---|---|---|---|
| (a) per-type enums | all three (blocked today by D3) | no: `eval_int`, `eval_bool` total | 2 enums, 2 evaluators; polymorphic `If` duplicated per result type |
| (b) phantom `Expr<T>` | all three (observed) | yes: `Value.Stuck` arm and a fallback in `run_int`/`run_bool`; forge hole via struct literal | 1 node enum, 1 value enum, 5 smart constructors, 3 evaluators |
| (c) trait per result type | all by construction (field types are traits) | no | open; blocked today by D1/D2 |

### Pinned primary sources (criterion 2)

- GHC 9.14.1 User's Guide §6.4.9 "Generalised Algebraic Data Types (GADTs)",
  <https://downloads.haskell.org/ghc/latest/docs/users_guide/exts/gadt.html>
  (read 2026-09-29): `data Term a where Lit :: Int -> Term Int; … If :: Term
  Bool -> Term a -> Term a -> Term a`, one `eval :: Term a -> a`; "pattern
  matching causes type refinement", refinement only from user-supplied
  signatures.
- OCaml 5.3 manual ch. 7 "Generalized algebraic datatypes",
  <https://ocaml.org/manual/5.3/gadts-tutorial.html> (read 2026-09-29):
  `type _ term = Int : int -> int term | …`, `let rec eval : type a. a term ->
  a`; needs explicit polymorphic recursion; exhaustiveness is GADT-aware.

Same job compared: both peers write one `Term a` with one polymorphic `If` and
one `eval`, and ill-typed nodes fail at construction. Jet encoding (a) gets the
same static rejection and total evaluators by splitting `Term a` into one enum
per index; the cost is one `If` constructor and one evaluator per result type.
Encoding (b) matches the peers' construction-time rejection but not their
evaluator (it needs a runtime arm). What Jet cannot express is a single
`fn eval<T>(e: Expr<T>) -> T` whose match refines `T`; that is the GADT
feature itself, not a missing check.

## Verdict

- Criterion 1 (execute best encoding; show which ill-typed nodes are rejected):
  met as evidence. Encoding (a)/(b) execute on default run and `--interpret`;
  AOT fails (D4). (b) rejects all three ill-typed nodes; (a) rejects none today
  because of D3.
- Criterion 2 (pinned primary sources): met, sources above.
- Criterion 3: recommend **close with existing mechanisms, no GADT ballot**.
  The closed typed-AST job is served by per-result-type enums (static
  rejection, total evaluators) once D3/D4 are fixed; the only surviving
  difference is duplication of result-polymorphic constructors, which is not a
  correctness gap and does not justify a type-refinement feature under this
  card. Met: the verdict is delivered; the defects that currently break
  encoding (a) are filed separately (D3, D4) and do not change it.
- Criterion 4 (golden + UI snapshot pass): not met. The golden would fail on AOT
  (D4) and `jet check` (E2104, D6); the UI snapshot would record acceptance of
  ill-typed (a) nodes (D3). Nothing was blessed; files stay in scratch.

## Defects

- D1 ICE: method call on a struct whose fields are trait-typed:
  `free/../gadt/ice_trait_field.jet` — `struct Plus { a: IntTerm b: IntTerm
  impl IntTerm { fn value(self) -[]> Int { int_of(self.a) + int_of(self.b) } } }`
  then `p.value()` → `internal compiler error: … missing checked function target
  Plus::value` on run/--interpret/AOT (same with a top-level `impl
  Plus.IntTerm`). Re-run on snapshot17: the ICE is gone and default run
  instead stops with `E3001 panic: panic … in int_of`.
- D2 wrong runtime result: same program calling `int_of(p)` (dynamic dispatch)
  → default run exit 70 `Stop [E3001]: panic: panic … in int_of`.
- D3 soundness: named-payload variant construction skips field type checks.
  `free/variant_field_type.jet`: `enum Pair { Ints(a: Int, b: Int) }`,
  `Pair.Ints{a: "not an int", b: true}` → `jet check` "no problems", `jet run`
  prints `33` (re-confirmed on snapshot17), `--interpret` E0956 "MIR enum argument type does not match its
  variant payload".
- D4 AOT: recursive named-payload variants are not boxed at construction
  (rustc E0308 `expected Box<IntExpr>`), `typed_ast_ab.jet`.
- D5 E0102 on a direct method call through a trait-typed field
  (`self.a.value()` / `p.a.value()` with `a: IntTerm`) while passing the same
  field to a `t: IntTerm` parameter and calling `t.value()` is accepted
  (`gadt/trait_field4.jet`).
- D6 `jet check` fails with E2104 on derived `compare`/`equal` of recursive
  enums while `jet run` runs the program (`typed_ast_ab.jet`).
- Observation: an unannotated trait method reached by dynamic dispatch is
  charged every effect root under a package budget (E1220 Browser/FFI/GPU/…,
  `gadt/trait_field.jet`) instead of the spec's annotate-the-method diagnostic
  (spec.md "Higher-order and trait effects").
