# Jet Composition Laws

Status: target contract for card #7049. Each law gives the rule, its live
baseline (what the compiler at `068535bc409` does today, with sources read for
this document), and the adversarial gaps the build must close. When a law and the
live code differ, the code is the defect, unless this document marks the point as
an open owner question. Each open question is a ballot item and is not settled
here.

Sources read: `Compiler/JetFoundation/Data/Syntax/Spellings.jet` (306 lines),
`Compiler/JetSema/Source/Sema/MarkerRules.jet` (2639),
`Compiler/JetFoundation/Source/Types/Types.jet` (2413), and the
`Compiler/JetParser/Source/Parser/` tree (`Expressions.jet`, `Statements.jet`,
`Patterns.jet`, `DeclarationMarkers.jet`, `MarkerHooks.jet`,
`NamedMarkerGroups.jet`, `ValueTail.jet`, `ExpressionForms/`).

## Meta-law: composition without special cases

Every form below composes with every other form under the same rule (AGENTS.md
I8, pairwise orthogonality). If two forms need a compiler special case to
interact, one of the forms is defective. The test matrix for each law is
*(form × every context in Law 9)*, and it runs on O0, O1, and O2 (I9).

---

## Law 1: Blocks are expressions

**Rule.**
1. A `{ … }` block is an expression. Its value is its tail expression, and a block
   with no tail has type `Unit`.
2. `if` and `loop` take their bodies only as `{ … }`. The `cond -> expr` body
   short form is removed from both. One spelling covers statement and value
   position: `x: if c { a } else { b }`.
3. A block that ends in a diverging statement (`return`, `break`, `next`, a
   `Never` call) has type `Never` (Law 2).

**Live baseline.** The `->` body form is used throughout today's sources:
- `MarkerRules.jet:146`: `if ty == Type.String -> return …`
- `MarkerRules.jet:133`: `else -> "TypeUse"`
- `Types.jet:1756`: `loop part in name.split(".") -> leaf = part`
- `Types.jet:1733`: `loop arg in args -> type_nominals_unqualified(…)` (yielding)

`Spellings.jet:82` registers the yielding form
`loop x in xs -> item / Head{loop x in xs -> item}`. `Spellings.jet:81` defines
"value body" positions, including "an arm's arrow".

**Adversarial gaps.**
- **G1.1 (open owner question: scope of `->` removal).** `->` also has three other
  jobs: function signatures (`Spellings.jet:68`), match arms inside
  `if x == { .A -> … }` (`Types.jet:1732`), and `else ->` inside an arm block. The
  card text says "drop `->` from if and loop completely". It does not say whether
  arms keep `->`. Recommended reading: arms keep `->`, because an arm is a
  pattern-to-value mapping, not an `if`/`loop` body. Signatures keep `->`, because
  it is a type arrow. Both readings must go on the ballot, with the arm/`else`
  spelling named exactly.
- **G1.2.** A brace block in condition position is ambiguous: `if { … } { … }`
  could mean the multi-way `if { c -> v … }` form (`MarkerRules.jet:112`) or a
  block condition. The multi-way `if { … }` form must be re-specified under Law 1.
  The likely shape is `if { c { v } … }` or arms that keep `->` under G1.1. Without
  that, the formatter migration is not mechanical.
- **G1.3.** Removing `->` is a registered breaking spelling. It needs a
  `Spellings.jet` rewrite row (the rows at `Spellings.jet:104` onward) and a
  `jet fmt` rewrite. Legacy acceptance is deleted in the same batch (greenfield
  cutover).

## Law 2: Control-flow types

**Rule.**
1. **Collecting loops use container wrappers.**
   - `[loop x in xs { f(x) }]` builds a `List`.
   - `Set{ loop … { … } }` builds a set.
   - `[K: V]{ loop … { k: v } }` builds a map.

   A bare loop in value position is not a collection. It is a `break`-valued loop
   (point 4).
2. **Labels.** `outer: loop … { … }` names a loop. `break(outer)` and `next(outer)`
   target it.
3. **Label vs value.** `break(ident)` with no space means "break the loop labelled
   `ident`". `break (expr)` with a space, or `break expr`, means "break with this
   value". The lexer distinguishes the two by adjacency, the same rule already used
   for calls.
4. **Finite `break val` loops are `T?`.** A loop that can finish without a `break`
   (a `loop x in xs` or state loop) and that carries `break v: T` has type `T?`. A
   pass that exhausts the source gives `None`. An unbounded `loop { … }` with
   `break v` has type `T`.
5. **`Never` is first-class and lift-exempt.** `Never` is a subtype of every type.
   The implicit `T -> T?` lift never applies to `Never`: a `Never` expression in a
   `T?` slot stays `Never` and is never wrapped as `Val(never)`.

**Live baseline.**
- Bare value-position loops build a `List` today (`Spellings.jet:82`: "a bare one
  builds a List and ignores the expected type"). Point 1 reverses that. The
  `Head{loop …}` form becomes the only collecting form.
- `Never` is not a variant of `Type`. The variant list at `Types.jet:1744` and the
  exhaustive arms at `Types.jet:1872–1927` have no `.Never`. `Never` is spelled
  `Type.Named{name: "Never"}` (see `Expressions/Control.jet:1385`) and is also used
  as an error-contract head (`-> String Never!`, `Types.jet:1861`).
- Loop labels are registered as snake-case names (`Spellings.jet:292`).

**Adversarial gaps.**
- **G2.1.** A first-class `Never` needs a `Type.Never` variant. Adding it touches
  every exhaustive match over `Type`; `type_identity_equal` alone has about 20 arm
  lists. That migration cost is real but mechanical. While `Never` stays
  `Named("Never")`, a user type named `Never` in a module would alias it. That is a
  soundness hole, and it must be a criterion.
- **G2.2.** `break(x)` vs `break (x)` is whitespace-sensitive. A formatter that
  normalizes spacing must never rewrite one into the other. `jet fmt` needs a
  round-trip test for both spellings, and E-diagnostics are needed for
  `break(nonlabel)`.
- **G2.3.** Under point 4, `[loop … { break v }]` mixes yield and break. Rule: a
  `break` inside a collecting wrapper ends collection early with the items
  collected so far. `break v` inside a wrapper is a build error, because the
  wrapper's value is the collection.

## Law 3: Flow-binding pattern algebra

**Rule.**
1. A pattern test that binds names (`x == .Some(v)`, `x == .Named(name)`) may only
   appear in a control context: an `if` header, a `loop` header, or a guard.
   Bindings flow into the branch the test makes true.
2. A binding test is never a standalone `Bool` value. `ok: x == .Val(v)` is a
   build error when the test binds.
3. Connectives:
   - `a && b`: bindings from `a` are visible in `b` and in the true branch.
   - `a || b`: allowed only when both sides bind the same names at the same types,
     or when neither side binds.
   - `!p`: binds nothing in the true branch. Its bindings flow into the false
     branch or after an early-exit guard (`if !(x == .Val(v)) { return }` makes `v`
     live afterward).

**Live baseline.** `MarkerRules.jet:149` (`if ty == .Named(name) { … name … }`) and
`:158`/`:161` show header binding. The `!`-guard flow and the `||` same-binding rule
were not verified in this session. The build must add negative UI snapshots for
each connective.

**Adversarial gaps.**
- **G3.1.** Name the scope of a guard binding for `if !p { return }` exactly:
  bindings live from the end of the `if` statement to the end of the enclosing
  block.
- **G3.2.** If `||` with mismatched bindings gets a precise registered code, the
  code must be new, not a reuse (I4).

## Law 4: Top-down expected-type propagation

**Rule.** The expected type flows from the slot (binding annotation, parameter,
field, return, `Type{…}` head, arm result) into the value:
- `[]` and `{}` take their element and record types from the slot.
- `None` takes `T?` from the slot.
- A closure takes its parameter and effect types from the slot.
- `Type{expr}` converts from or to `Type`.
- `From` conversions resolve against the expected type, never by inspecting the
  value.

Propagation goes through every composite in Law 9: arms, block tails, pipes,
`??`, and wrappers.

**Live baseline.** `Spellings.jet:81`: "where a struct type is expected the same
braces build it". Yielding loops ignore the expected type today
(`Spellings.jet:82`), which is a Law 4 violation that Law 2.1 removes.

**Adversarial gap.**
- **G4.1.** `{}` is a block (`Spellings.jet:81`). With an expected record or map
  type, `{}` must still mean an empty block value, unless the slot is a collection
  head. State the rule plainly: an empty collection is written `Head{}`, and a bare
  `{}` never changes meaning based on the slot.

## Law 5: `Type{…}` in `if`/`loop` headers

**Rule.** `if Type{…} == x { … }` and `loop … in Type{…} { … }` parse the `Type{`
as a typed literal, never as the start of the body.

The header lexer decides in three stages:
1. A PascalCase head immediately followed by `{` (no space) is a literal opener.
2. A space before `{` after a complete header expression opens the body.
3. Grammar heads (`Spellings.jet:80`) defer to their DSL lexer.

**Adversarial gaps.**
- **G5.1.** Stage 1 is adjacency-based, the same as Law 2.3. The build must prove
  no false E0637 on the corpus sweep, and the formatter must never insert a space
  between `Type` and `{`.
- **G5.2.** `if x == Point {…}` (spaced, a unit-like named value followed by the
  body) must keep working under stage 2. This needs a snapshot.

## Law 6: Uniform postfix chains

**Rule.** Postfix operators bind tighter than every prefix and binary operator and
chain left to right with no exceptions:
- `?` propagates an `Option`/`Result` failure to the enclosing contract.
- `?.` is an optional member or call.
- `??` is a default. It is binary, at the lowest precedence of this group, with the
  right side typed by Law 4.
- `!` propagates an error.
- `!(ctx)` propagates with added context.
- `!!` asserts success; failure is an explicit trap.
- Indexing `[i]`, calls `(…)`, and `.member` all chain the same way.

**Pipes.** `a |> f(<>)` passes `a` into the `<>` hole. Without `<>`, `a |> f`
passes it as the first argument. `<>` may appear once per stage. A pipe stage is a
postfix-chain context, so `a |> f(<>)?.g()` chains.

**Live baseline.** `??` with `return` on the right is live
(`MarkerRules.jet:138`: `… ?? return None`).

**Adversarial gap.**
- **G6.1.** `!` is both prefix negation and postfix propagation. `!x!` must parse
  as `!(x!)`. A snapshot must pin this.

## Law 7: Markers are prefix or in-body, never postfix; stacked `#[A, B]` everywhere

**Rule.**
1. A marker is written before what it marks (`#A fn …`, `#A { … }`) or inside a
   type or declaration body.
2. No marker follows its subject. `#Through` and `#CatchAll` are prefix-only.
3. A marker list `#[A, B]` is accepted at every marker slot. It is identical to
   `#A #B` in order.
4. Validation is per slot through one site table.

**Live baseline.**
- `Spellings.jet:25` registers `#[module.Marker, Other]`.
- `MarkerRules.jet:111–135` maps `RuleSite` to a name and falls through
  `else -> "TypeUse"`.
- `MarkerRules.jet:137–143` parses a site name from source and accepts 20 names.
  `TypeUse` is not one of them, so a rule can never name the `TypeUse` site that
  `site_name` reports. The two functions are hand-written mirrors.

**Adversarial gaps.**
- **G7.1.** Replace both functions with one generated `RuleSite`↔name table that
  includes `TypeUse`, so the two directions cannot drift (card criterion 3 cites
  line 140; the live list is lines 139–140).
- **G7.2.** A postfix-marker removal sweep must find every postfix use in
  `Compiler/`, `Core/`, `Examples/`, `Tests/`, and `Docs/`. The fix is a
  formatter rewrite to prefix form plus a parse error that has a registered
  fix-it.
- **G7.3.** Stacked lists nested inside named marker groups
  (`NamedMarkerGroups.jet`) must flatten under one rule. A group used inside
  `#[…]` expands in place.

## Law 8: Three-class lexically scoped jumps

**Rule.** `return`, `break`, `next`, and `?`/`!` propagation are jumps. Each
enclosing construct falls into one of three classes:

| Class | Constructs | Rule |
|---|---|---|
| Sealed | `defer { … }`, `#Defer`, `#Off`, `#On(.Deep)`, `#run { … }`, `#Pre(…)`, `#Post(…)` | No jump may leave the body. A jump that targets outside it is a build error. Propagation inside must be handled inside. |
| Unwinding | `task.group`, `#Task`, `#Transact`, `#Context`, `#Frame` | Jumps may leave. Every exited scope runs its cleanup in LIFO order before control transfers. `#Transact` rolls back on any non-normal exit. |
| Transparent | plain blocks, `if`, `loop`, arms, other marked blocks | Jumps pass through freely. |

**Adversarial gaps.**
- **G8.1.** `#run` is sealed because it runs at build time. A `return` there would
  return from a build-time evaluation into run-time code. State this as the reason.
- **G8.2.** `#Task` and `task.group` unwinding must define what a `break` out of a
  group does to sibling tasks. Recommended: cancel the siblings, join them, then
  jump. This needs an O0/O1/O2 test with observable cleanup order.
- **G8.3.** Every marked block in Law 9 must be assigned a class in the registry
  row, not in code, so that adding a new marker forces a classification.

## Law 9: Uniform coverage

Each law above holds in every one of these contexts, and each context gets a
golden example (I5) run on O0, O1, and O2 (I9):
- **Pipes:** `|>` stages and `<>` holes.
- **All six `if` forms:** boolean, `else`, pattern-binding, multi-way `if { … }`,
  match `if x == { … }`, and value position.
- **All marked blocks:** the 16+ value-giving and statement marker blocks, each
  classified under Law 8.
- **Staged code:** `#run`, `#if`, and the build-time loop (`Spellings.jet:35`,
  `PREP_LOOP`). Laws 1–6 hold in staged code unchanged.
- **Ownership and places:** the `^` binding (`Spellings.jet:13`),
  `place.in_place()` windows (`Spellings.jet:15`), and `fn(^T)`/`fn(^^T)`
  (`Spellings.jet:76`) compose with blocks, loops, and postfix chains with no
  extra rule.

**Adversarial gap.**
- **G9.1.** The "six `if` forms" and "16+ marked blocks" lists must be derived from
  the registry (`Spellings.jet` plus the marker table), not hand-listed in tests,
  or the coverage claim drifts.

## Verdict

Laws 3, 4, 6, and 9 are mostly live and need proof work, meaning snapshots and
per-tier goldens.

Laws 1, 2, 5, 7, and 8 change shipped behavior or close real holes:
- `->` removal
- collecting-wrapper reversal
- `Type.Never` as a variant
- the `TypeUse` site mirror
- postfix-marker removal
- sealed-jump enforcement

One item needs a ballot before build: G1.1/G1.2, which decides whether match arms,
`else` inside arm blocks, and the multi-way `if { … }` keep `->`. Every other gap
above is a criterion on #7049.
