# Syntax Decisions (the owner's control surface)

**The owner has final say on all user-facing syntax.** Agents implement only
what is Ratified here and must never invent surface syntax. To propose
something new: develop a decision card (options, worked examples, rec), queue
it in Tower, and stop work on that feature until the owner decides.

**Ratify = then build it, end to end.** An owner answer on a decision with no
open upstream gate is the "go": parser → sema → codegen, a `tests/ui` snapshot
for every diagnostic (I4), a golden-tested example where user-visible (I5),
all tests green. A ratified entry may sit unbuilt **only** when gated on
another unratified decision (name the gate). After ratifying: update
`crates/jet-foundation/src/Syntax.rs`, re-bless snapshots, log it here.

This file records **current law only** — one entry per decision, final form.
History (superseded spellings, ballot narratives, amendment chains) lives in
git history of this file. Canonical truth = `Syntax.rs` + this file
(D-CANON-SOURCE1); old-spelling teaching is paused until post-Epoch 6
(D-S14-PAUSE) — retired forms get ordinary syntax errors.

Vocabulary: [Jet vocabulary](vocabulary.md).

## Ratified

### Names & conventions

**N1 — Language name**: **Jet**. Binary `jet`.

**N2 — File extension**: `.jet`.

**S54 / D-SHAPE-CASE1=C — One identifier-casing law, machine-enforced**
*(ratified 2026-07-16, card #665; amends S54's PascalCase constants and its
no-lint rule)*: one rule per grammatical category, zero exceptions. Type-like
names are PascalCase: types, traits, enum variants, markers, unit-family names
and their generated unit types, Config block types; effect and capability
keywords stay PascalCase. Value-like names are snake_case: functions, methods,
fields, locals, module paths, generic module templates, unit members, and
constants. The compiler enforces the law; casing drift is a coded diagnostic,
not a convention. Foreign names in FFI bindings are exempt inside binding
modules per D-SHAPE-CASE2=A (FFI section).

**S66 — Standard acronyms fully capitalized** *(D-ACRONYM-CANON1; applied by
D-ACRO-CASE1=A + D-ACRO-LEX1=A)*: initials-formed names stay all-caps inside
PascalCase — `JSON`, `TOML`, `YAML`, `CSV`, `HTTP`, `CLI`, `SQL`, `IOError`,
`UTF8Error`, `U8`, `MacOS`, and the closed lexicon in
`Syntax::ACRONYM_RESPILLS`. Glued compounds (`HTTPHeader`); underscore only
between touching capital runs (`HTTP_API`). Coined contractions stay words:
`Wasm`, `WasmExport`, `Bindgen`. No PascalCase aliases; retired spellings get
one teaching fix (E0358 / E0927).

**S84 — Hyphens in package/module/system/image/env names**: kebab-case allowed
in these *name* positions (`image.halcyon-iso`, `module web-app`). Grammar:
dashed name `ident (-ident)*`, `-` joins only when span-adjacent on both sides,
so spaced `a - b` stays subtraction — parser-level (`expect_dashed_name`), no
lexer change. No leading/trailing/doubled hyphen. Code identifiers stay plain
`ident`.

**S14 — Alias policy**: one canonical spelling per construct; **no aliases,
ever**. The compiler may recognize foreign syntax to emit a teaching error
naming the canonical form. **D-S14-PAUSE / D-TEACHING-LAYER1=A**: this
teaching layer is paused until post-Epoch 6 — retired spellings currently get
ordinary syntax errors; stale teaching fixtures were deleted. **D-CAP10**: one
definition per name (E0105); no overloading — capability disambiguation is
call-site sigils on a single definition.

**D-CASING1 — Casing law + "Core"** *(with D-MARKER-CANON1, D-CONTRACTCASE1)*:
every `#` applied rule is PascalCase (`#Test`, `#Unsafe`, `#Grant`, `#Pre`);
lowercase `#[allow(...)]` is the registered lint-policy exception. Traits are
PascalCase. The standard library is
**"Core"** — never "std"/"stdlib" — in docs, identifiers, and error copy.

**D-CORENS1 — Single `core.*` namespace** *(D-CORENS-CANON1)*: every
first-party library (built-in module or ring package) is `core.<name>`. No
`jet.*`, `std.*`, or `jet.core` spellings (old ring spelling → E0341).

**D-SOLVER-LIB1 — Explicit solver library**: `core.solve` ships a finite
solver API with explicit `Solver` state, deterministic insertion-order
constraint checks, ordinary `Bool` values, and no language-level
backtracking/unification. Initial Core surface: `solve.Solver.new(seed)`,
`solver.require(ok)`, `solver.failure_count()`, `solver.status()`.

### Bindings & assignment

**S2 — Bindings** *(current law = D-BIND-BARE1)*:

```jet
name :: expr            // immutable binding
name := expr            // mutable binding
name = expr             // reassignment of an existing := binding
```

Types never ride the binding name. Put the type on the value
(`U8.{ 250 }`, `Color.{ r: 64, g: 128, b: 200 }`) or on signatures and fields
(`name: Type`). The retired forms `name: Type :: expr` and `name: Type := expr`
are ordinary parse errors (D-BIND-BARE1; amends D-BIND4).

**S4 — Type annotations**: `name: Type` after the name on signatures and fields.
Never on a local binding. Never `Type name`.

**S17 — Compound assignment**: `+=` `-=` `*=` `/=` `%=` `&=` `|=` `^=` `<<=`
`>>=`. Arithmetic four on Int/Float; the rest Int-only. LHS must be a mutable
binding or `&` parameter.

**D-INCR1 — Increment/decrement**: `++x`, `x++`, `--x`, `x--` on mutable
integer lvalues; prefix yields the new value, postfix the old. Indexed slots
rejected; non-integer E0162; immutable E0161. Deliberate second spelling
beside S17 (owner-chosen I8 exception).

### Functions

**S1 — Function keyword**: `fn`.

**D-ARROW-CONTROL1=A — Callable and control arrows** *(ratified 2026-07-26,
card #1209)*:

- `=>` defines a callable result.
- `=[Effects]=>` adds an explicit effect ceiling to that callable result.
- `=[]=>` states an empty effect ceiling. `=[..E]=>` keeps an open row.
- `->` selects a control-flow arm or yields one finite-loop item.
- effect-only `if` and `loop` bodies have no arrow.
- braces group multiline bodies. They do not imply a result.

Named functions, methods, function types, lambdas, computed fields, conversion
implementations, and migration converters use the callable arrow. A concise
named callable uses `= expression` after its result type:

```jet
fn double(value: Int) => Int = value * 2

fn load(path: String) =[FS]=> String {
    text :: core.files.read(path)?
    text.trim()
}
```

A returned block uses its final value. `return` remains the explicit early-exit
form. A unit-returning callable with no explicit effect ceiling needs no arrow.
`->` is not a general operator.

**D-VOID1=A — one no-information result spelling** *(ratified 2026-08-04,
card #1411)*: user-facing no-information results use `()`. The compiler keeps
the internal `Unit` value, but `Void` is retired and produces E0431 with a fix
to `()`. This decision does not add or change tuple syntax. D-NEVER1=C keeps
non-returning paths as a compiler-only bottom fact; Jet has no public `Never`
type.

**S12 — Entry point** *(amended by D-FAIL-EXIT1/D-FAIL-UNIT1)*: `fn run()`;
no `pub` required; it is fallible by default. An expert may pin
`fn run() ? E`. **D-CLIFLAG1** (implemented, c7cliflag): a
typed entry parameter optionally opts into CLI parsing — `fn run(args: ServeArgs)`
derives `--flag` names/defaults/help from the struct's fields
(`#CLI`/field-level `#Doc("...")` markers, bracket form matching `#Codable`);
an entry `enum` may use `#Doc("...")` on the enum and its variants for the
program description and subcommand summaries. There is no Jet `main` entry and no
variadic entry signature. Raw argv access stays explicit inside `fn run()`
via `core.args`/`core.io.args`. See docs/spec/spec.md
"Typed entry-signature CLI parsing" for the full field-mapping rule. The
existing `core.args` `ArgsSpec` builder (D-ARGS1) remains the library floor
for non-entry parsing; the typed layer generates onto it rather than adding
a second parser.

**D-ENTRY-SCRIPT1=B / D-ENTRY-ORDER1=B — script entry sugar** *(ratified
2026-08-08, card #1808)*: bare top-level statements in a direct entry file
are the body of one implicit fallible `fn run()`. Top-level declarations stay
file-wide declarations; loose statements keep written execution order and
their bindings are local to that body. `jet run`, `jet dev`, and `jet test`
use the same entry law. An explicit `fn run` alongside loose statements is
E0621 with the `jet fix` auto-wrap; an imported file with loose statements is
E0620 and never executes as an import. `fn dev`, `fn build`, and `#Test`
declarations remain legal beside script statements.

**D-CLI-POS1=A — positional by default, `#Flag` to opt out** *(ratified
2026-07-23, card #748)*: on a `#CLI` struct, required value fields fill from
bare argv in declaration order. Boolean flags and optional/defaulted fields stay
flag-only. Every field still accepts its named `--field` spelling; when both the
named form and a bare value appear for the same field, the named value wins.
`#Flag` on a required value field removes it from positional filling (expert
opt-out for secrets and similar). `#Flag` on a Bool / optional / defaulted
field is E1309. Declaration order of required value fields is
part of the command interface; the always-available named spelling keeps scripts
stable across reorders. Help lists positionals before flags. Marker constant:
`MARKER_FLAG` (`Flag`).

**D-CLI-FIELD-MARKERS1=A — per-field short names and environment fallbacks**
*(ratified 2026-07-28, card #1277)*: a `#CLI` field can use `#Short("n")`
for a one-letter alias and `#Env("PORT")` for an environment fallback. Both
rules use the existing field marker list and lower to the existing `core.args`
builder. Explicit command input wins over the environment, and the environment
wins over `#Default`. Help shows the short name, fallback, default, and
precedence. A short name is one ASCII letter and cannot collide (E1318).
Typed-CLI markers with no builder mapping are E1319. Marker constants:
`MARKER_SHORT` (`Short`) and `MARKER_ENV` (`Env`).

**D-SHAPE-CLI1=A — entry type owns command inputs** *(ratified 2026-07-14,
card #541)*: when present, the resolved parameter type of `fn run(args: T)` is
the single source for shell input names, types, defaults, parsing, help,
completion, validation, and audit facts. This adds no required ceremony:
plain `fn run()` is the canonical zero-ceremony entry and may read raw arguments
through `core.args` when typed derivation is not wanted. Jet never requires an
entry parameter; the author adds one only when external command input belongs in
the function signature. The public CLI type may live in the entry file or one
directly imported module.

**S27 — Methods**: `self` receiver with capability sigils (`^self`,
`&self`; bare `self` = read, D-MEM1). Call `value.method(args)`. Methods live in the
type body, in `impl Type { }`, or top-level `fn Type.method(self)`
(**D-EXTMETH1** — `.` connector, orphan rule: same source module; `~~` retired
→ E0325). No-`self` fn in a type = static method (`Circle.unit()`).

**D-CALLDUAL1=E — marked receiver for reversible calls** *(ratified
2026-08-03, card #1401)*: a top-level free function may mark its first
bare-read parameter with `#Root`. The function then accepts both `f(value, …)`
and `value.f(…)`. Dot-call lookup is limited to functions visible through the
current module's imports; it is never a global search. A real method with the
same name is an error, and more than one matching imported function is an
ambiguity error. Resolution never uses the return type. `#Root` is not valid on
a write or move parameter, on a later parameter, or on a method. Foreign
top-level declarations preserve the marker. Normal argument compatibility,
including numeric widening, applies to the hidden receiver. Inspect and LSP
show the module that supplied an imported `#Root` function. Formatting preserves
the declaration and never rewrites either call spelling.

**D-CTOR1 — Named constructors only**: many ways to build a type = many
named statics (`Point.cartesian(…)`, `Point.polar(…)`); duplicate name E0105.
No marker keyword — return-type-is-the-type identifies a constructor (D-CTOR2).

**S46 — Lambda syntax**: `(params) => expr` / `(params) => { … }`. `=>` is
the callable arrow; `->` is reserved for selected or yielded control values.
**D-LAMBDAINFER1**:
a lambda param type may be omitted where the expected type fixes it
(`xs.filter((i) => i.state == .Open)`); required elsewhere (E0801);
one-directional inference only. **D-LAMBDA-INFER1** *(ratified 2026-07-04)*:
a single bare param may ALSO drop the parens — `xs.filter(m => m.hp > 0)` —
wherever the expected type fixes it (same E0801/one-directional rule); no
explicit capture prefix exists. The parenthesized and typed forms stay
available where the type cannot be inferred.

**S47 — Function types & captures**: fn type `fn(T1, T2) => R`; each unmarked
parameter has plain read access (D-MEM-PARAM1). Named `fn`s coerce to function
values only when every parameter also has plain read access. A named function
with a write (`&`) or move (`^`) parameter stays direct-call-only because S47
has no function-type spelling that could preserve that requirement. Captures follow M2: shared read for read-only
names, mutable borrow for written names. Escaping closures own captures.
Values accepted by Jet's ordinary copy law are copied at closure creation, so
the source binding remains available. Owned values that cannot be copied move.
Mutable and borrowed captures remain subject to sema's ownership checks. No
`take(...)` capture prefix or preparatory copy binding is required.

**S61 — Argument labels & parameter contracts** *(D-NARG1, D-NARG2,
D-APILABEL1=A ratified 2026-08-05, card #1393)*: a written label binds by
**name**. A call may skip a default and write its labelled arguments in any
order. Two zone separators split the parameter list: `/` closes the
positional-only zone (a label there is E0767) and `*` opens the label-only
zone (a positional argument there is E0769); parameters between them take
either form. A parameter may publish a label distinct from its local name —
`timeout seconds: Int` binds callers with `timeout:` while the body reads
`seconds`. Public labels and zones are callable type identity; local names
and default bodies are not.

Supplied expressions run left to right in the order they were **written**;
unbound defaults then run in declaration order and may read an earlier
parameter. Diagnostics: E0763 misplaced zone separator, E0764 unknown label,
E0765 repeated label, E0766 missing argument, E0767 label forbidden, E0768
bare argument after a labelled one, E0769 label required. Methods,
constructors, generic calls, variadics, and function values all bind through
the one binder. fmt never adds nor strips labels.

```jet
fn connect(host: String, /, *, timeout seconds: Int = 30, tls: Bool = true) => Client ? ConnectError
client :: connect("db.internal", tls: true, timeout: 5)?
```

Retires the fixed-position rule: labels used to be spelling checks at one
position and never reordered (E0125).

**S83 — Multi-head functions**: same name, different parameter patterns, each
head its own body; dispatch by argument shape; heads must be exhaustive.

```jet
fn area(Circle(r: Float)) => Float = 3.14 * r * r
fn area(Rect(w: Float, h: Float)) => Float = w * h
```

**D-VARIADIC1 — Variadics & spread**: `...` everywhere — param `name: ...T`
(final position), call spread `f(...xs)`, list spread `[...a, x, ...b]`
(E1310–E1312). **D-ANY-JAI1**: heterogeneous varargs are trait-bounded —
`parts: ...Renderable`; trait sets use list bounds
(`fn f<T: [Renderable, Serializable]>(parts: ...T)`, **D-VARARGBOUND1**).
No top type; general `Any` rejected.

**D-UNIONTYPE1=A — Anonymous union types** *(ratified 2026-07-24, card #744)*:
`A | B` is closed structural enum sugar. Nested unions flatten, duplicates
disappear, and identity is order-insensitive. `T ? E1 | E2` places the union on
the error side. Members widen into the union only at binding, argument, return,
Codable field, and `?` error boundaries. Match arms name member types; Codable
decode rejects ambiguous wire shapes (E2415). Named enums remain the documenting
form; crossings use ordinary match conversion. Does not reopen D-ANY-JAI1.

**D-TRAILBLOCK1 — Trailing block argument** *(superseded by D-TRAILBLOCK2=A)*:
when a call's final param was a function type, a bare `{ }` after `)` stood in
for that lambda — `ui.button("Save") { prefs.save() }`. Zero-parameter blocks
only in v1 (E0334/E0335).

**D-TRAILBLOCK2=A — Explicit `() =>` code arguments** *(ratified 2026-07-28,
card #1266)*: pass multiline code into a call as an ordinary lambda argument
inside the parentheses — `twice(() => { print("HI"); print("Hello") })` (each
statement on its own line). Multiple code arguments are ordinary
comma-separated args. Retires D-TRAILBLOCK1 trailing `{ }` sugar; a bare `{ }`
after a call is E0335 with a fix that shows `callee(() => { … })`.

**Declined (functions)**: UFCS (D-UFCS1); call-site macro-method expansion —
inlining via `#Inline`/`#Inline(Always)` contracts instead (D-METHODMACRO1);
the earlier untyped expression-body `fn f() = expr` (D-FP2), superseded by
D-ARROW-CONTROL1's typed `fn f() => T = expr`; the earlier general-pipe
proposal (D-SUGAR2), superseded by D-SHAPE-PIPE1=C.

**D-SHAPE-PIPE1=C — Bars mean alternatives, not general flow** *(ratified
2026-07-15, card #552)*: single `|` is legal only in alternative-list grammar,
including structural or-patterns and choice arms. Jet assigns no general flow
operator or other bar operator. Reusable flows use ordinary calls, named
intermediate values, and ordinary named composition helpers. `||` and the
separately ratified `|=` compound assignment keep their existing meanings.

### Control flow

**S3 — Blocks** *(amended by D-ARROW-CONTROL1 and D-BRACE1=A, ratified
2026-07-30, card #1335)*: curly braces `{ }` give every effect `if`, `else`,
and `loop` body a visible parse boundary. Arrow arm bodies remain expressions
and retain their existing braceless form. Braces do not determine whether a
construct returns a value.

**S19 — Loops** *(D-LOOP-HEADER2=A, D-LOOP-ADVANCE2=A,
D-LOOPEVAL1=A, D-COMPREHENSION1=A, D-LOOP-COMMA1=A, ratified
2026-07-30, card #1336)*:

```jet
loop { … }                        // infinite
loop n > 0 { tick() }              // conditional effect loop (no `while`)
loop i, 1..5 { print(i) }          // source effect loop (no `for`)
loop (key, value), map { audit(key) } // map-pair effect loop
loop i, 0..10, 2 { audit(i) }      // positive source stride
loop i := 0, i < n { draw(i); i += 1 } // mutable state (two-slot)
values :: loop item, items -> item.value
```

`while`/`for` are not keywords. A source and optional stride evaluate once,
left to right; a dynamic nonpositive stride raises E0123 before the first pull.
Stride N visits source indexes 0, N, 2N; exhaustion while advancing is normal.
Commas separate loop header clauses. Semicolons remain statement boundaries only.
**D-LOOP-HEADER3=D**: the old three-slot C-style counter (`loop i := 0, i < n, i += 1`)
is retired with teaching diagnostic E0376; prefer `loop i, 0..<n` or a range step rule.

A finite source loop may use `-> expression` or `-> { ... }`.
Each accepted iteration yields one non-unit item. The result is an eager
`List<T>` in iteration order. A header guard filters items. Multiple source
clauses nest left to right and yield one flat List. An inner collecting loop
preserves nesting. `next` omits the current item. Maps, Sets, and lazy
iteration use ordinary Map construction, `Set.from(...)`, and the existing
iterator adapters.
Bare infinite and condition-only loops cannot use a yield arrow.

**S22 / S72 — Ranges**: `1..10` is **inclusive**. Source-loop stride is the
optional third clause (`loop i, 0..10, 2`); `step` has no loop role.
**D-RANGE-EXCL1=C**: `0..<n` is half-open (stops before `n`, empty when
`a >= b`). Prefer `loop i, item; xs` or `loop i; xs.indexes` for index loops;
the compiler teaches when inclusive `0..xs.len()` indexes the same `xs`.
D-ITER1's pipeline adapter is `indexed()` (renamed from `enumerate`).
**D-RANGE-VALUE1=A** *(ratified 2026-07-28, card #1298)*: both spellings
construct one nominal `Range` over `Int`. The value carries whether its end is
exclusive and exposes `.start`, `.end`, and `.contains(value)`. A Range may be
stored, passed, returned, looped, or used as a slice bound. Literal loop ranges
still lower directly to jumps; no iterator object or allocation is added.
Range arm heads and `distinct Int(0..10)` constraints stay literal-only.
D-SHAPE-PLACE1's retired `.view(range)` method remains retired; bracket slicing
is the one live range-place surface. A bare place projection such as
`window :: values[range]` is a no-copy `View`; `&values[range]` is a
write-through `ViewMut`. Write `~values[range]` when an independent list copy
must outlive or detach from the owner.

**D-RANGE-EXCL1=C — Exclusive range + index idioms**: half-open `a..<b` runs
`a` through `b-1` and is empty when `a >= b`. Inclusive `..` (S22) is unchanged.
Sequence two-binding `loop i, item; xs` yields index then item; one binding stays
item-only. `xs.indexes` yields every valid `Int` index. An inclusive loop ending
at `xs.len()` that indexes that same `xs` is E0364 (teach two-binding, `indexes`,
then `0..<xs.len()`). D-ITER1's pipeline adapter is `indexed()` (same named tuple).

**S23 — Loop control** *(D-LOOP-CONTROLWORD1=B, D-LOOP-CONTINUE2=A,
D-LOOPLABEL3=A, D-LOOPSTATE1=A)*: `break` and contextual `next`. A named loop
uses declaration syntax (`outer :: loop`). The complete exit family is
`break`, `break value`, `break(outer)`, `break(outer, value)`, `next`, and
`next(outer)`. Bare exits target the innermost loop. Parentheses immediately
after the keyword select a loop target. A bare break operand is the innermost
loop's payload. Loop names share the ordinary namespace but are not runtime
values. Dot exits and the older `@` spellings are retired.

An ordinary loop returns one final value only through break payloads. Every
payload exit must unify. A collecting loop's bare or targeted `break` returns
the accumulated List, and its `next` omits the current item. Payload breaks
are rejected in collecting loops because their result is already `[T]`.
**D-CHOOSE-FIND1=A**: a finite source loop in value position uses a braced
body with `break value` exits and must be followed by a written exhaustion
route: `loop item, source { if found(item) { break item } } ?? fallback`.
The route is part of the expression. `?? next` and `?? break` immediately
after that closing brace are rejected; use the labeled loop form when the
exhaustion path must steer another loop. An explicit target,
`?? next(name)` or `?? break(name)`, remains a named route to an enclosing
loop.
`next` enters the target's continuation edge: explicit state runs its
afterthought then retests; source advances; condition retests; infinite restarts.
`next()`, `.next()`, and `fn next` remain ordinary uses. After `??`, bare `next`
is control; write `?? (next)` for a same-named fallback value.

**D-ORRETURN-CANON1 — Early-exit fallbacks**: `expr ?? return`,
`expr ?? next`, `expr ?? break` are the only spellings (`?return` etc.
removed; E0115).

**S68 — `if`: effect, value, and ordered arm tables** *(D-IF1 +
D-IF3 + D-MATCHARM1/2 + D-IFGUARD1=A + D-ARROW-CONTROL1=A + D-IFDIST1=A)*: `if` is the only
branching keyword. The canonical multi-branch surface is one ordered arm-table
model. Write `if subject OP { head -> body }` when a named subject improves
clarity (`OP` is any comparison), or `if { head -> body }` without one. A head may be a value or
structural pattern against the subject, or any `Bool` expression evaluated as
written; one table may mix unrelated expressions. The first matching or true
head wins. Chained `else if` remains legal, but there should rarely be a reason
to prefer it and docs and diagnostics do not teach it as Jet's normal form.

**D-BRANCH-PREF1=A / D-BRANCH-ONELINE1=A / D-BRANCH-ELSEIF1=A /
D-BRANCH-LINT1=A / D-BRANCH-VALUE1=A / D-BRANCH-FMT1=C /
D-BRANCH-TEACH1=A** *(ratified 2026-07-28, card #1259)*: L0507 points
multi-line braced branches and all `else if` chains at ordered arm tables.
One-line effect and value forms stay quiet. Fmt does not change branch shape.

- Effect form has no arrow: `if ready run() else wait()`. Braces group
  multiline bodies. Parentheses around the condition are optional and fmt
  strips them.
- Value form marks each selected value: `m :: if a > b -> a else -> b`.
  `else` is required (E0003), and branch types must match (E0124). A returned
  multiline arm uses `-> { ... }`. The same arm-table spelling works in
  expression position and yields `()` or one unified value type (D-IFDIST1).
- **Dispatch form** — a comparison between subject and `{` is required (bare
  `if subject { arm -> … }` is E0992, auto-fixed). Any of `== != < > <= >=`
  distributes over bare arm atoms (D-IFDIST1=A):

```jet
if code == {
    200 -> print("ok")
    301 | 302 -> redirect()          // `|` unions distributed atoms
    .Error(e) && e.fatal -> die(e)   // pattern + boolean guard (== only)
    code >= 500 -> retry()           // predicate arm
    else -> log(code)
}

return if n < {
    16 -> 0
    48 | 45 && ready -> 2            // `((n < 48 || n < 45) && ready)`
    else -> 15
}
```

Arms: bare values (compared with the table's `OP`), leading-dot enum patterns
(only with `==`), and any `Bool` expression evaluated as written. `|` binds
tighter than `&&`/`||` and mixes without requiring parens (D-IFDIST1 amends
D-MATCHARM2). Catch-all is `else ->`. Braceless single-expression bodies
are allowed. Exhaustive pattern arms may omit `else`.

**D-IFDIST1=A — distributed compare markers + ()-or-value tables** *(ratified
2026-07-28, card #1305)*: amends D-IF3 / D-MATCHARM2 / S68. The dispatch marker
is any comparison operator. Each bare atom desugars to `subject OP atom`; `|`
is OR of those atoms (including for `!=`); `&&`/`||` combine further Bool
heads. Pattern heads remain `==`-only. Statement and expression position share
one table spelling; expression tables unify arm values or yield `()`.

**D-IFGUARD1=A — ordered subjectless guards** *(ratified 2026-07-18, card
#680; amended by D-ARROW-CONTROL1)*: `if cond statement` is the one-line
effect guard. A direct adjacent nested `if` requires braces when its boundary
would be ambiguous. The subjectless spelling is the same ordered arm-table
model without a named subject, not a separate or lesser branching mechanism.
It keeps `->` because each arrow selects one arm, including an arm yielding `()`. Each
head is an arbitrary `Bool` expression evaluated in order; the first true head
wins. A value table requires a final `else` and all result types unify. A
pattern binding under `&&` reaches the rest of that head and its body;
a binding under `||` never reaches the body. A table whose every head starts
with a pattern test of the same closed enum emits L0302 recommending exhaustive
subject dispatch. Fmt keeps a fitting inline guard on one line, widens an
overlong one once to a one-arm table, and keeps value-table outer braces.

**D-PROTO1 / D-PROTO2 — Protocol blocks** *(amended by
D-ARROW-CONTROL1)*: a two-endpoint protocol names the sender:

```jet
protocol Payment {
    client: Pay(amount: Int)
    server: Receipt(id: String)
}
```

`client:` means the client sends, and `server:` means the server sends. The
receiver is implied. The block generates `.Client` and `.Server` handle types
over linear typestate machinery. Temporal ordering comes from the transition
graph itself. Protocol transport arrows are retired.

### Patterns & matching

**S31 — Pattern tests**: `==` with a pattern RHS when the LHS is an enum or
`T?` — `if s == .Rect(w, h)`, `x == .None` — yields Bool. Patterns nest to any
depth (`r == .Ok(.Rect(w, h))`). Guards are plain `&&`: a pattern-bound name is
in scope for the rest of the same condition. No `is`, no Rust `match`. Bare
variant names without a leading `.` are E0367 (D-ENUMDOT1).

**D-FLOWTYPE1=A — Optional narrowing after presence checks** *(ratified
2026-07-24, card #746)*: for a direct immutable local or parameter of type
`T?`, `x != None` refines `x` to `T` in the true branch; `x == None` refines
`x` to `T` in the false/`else` branch. The fact reaches the right side of
short-circuit `&&`, not `||`, and ends at the branch boundary. Mutable locals,
field paths, indexes, aliases, and calls never narrow — bind with
`x == Val(v)` (S31) instead. Sema records each proven unwrap as an S31 Present
binding for typed IR; codegen stays mechanical. Documentation teaches the
`None` check for a direct stable name and the binding pattern for every other
case.

**D-ENUMDOT1 / D-ENUMDOT2 — Leading-dot variants**: match-arm patterns require a
leading dot (`.Circle(r)`, `.Empty`); bare `Circle(r)` / `Empty` is E0367.
Value position too when the expected type is known (`.Red`; E0330 fallback).
`Color.Red` always valid.

**D-TAG1 — Nested variant groups** *(ratified 2026-07-03, card #181)*: a
variant may enclose sub-variants in `{ }` to any depth (`enum Damage {
Physical { Blunt, Pierce } Fire { Burn, Scald } Cold }`). A group name
matches its whole subtree in `==` pattern tests and dispatch arms (`d == .Fire`
is true for `.Fire.Burn`); exhaustiveness is checked at the group level;
payloads live on leaves only (E0331); a value is always a leaf — a group name
is not a value (E0332). Ships with core `Bag<T>` counted multiset
(`Bag.new()`, `add`, `remove`, `has`, `count`; subtree queries stay an explicit
`any` closure). No new keyword — reuses `{ }`, dot paths, and D-ENUMDOT1
leading-dot patterns.

**S74 — Standalone destructuring** *(with D-DESTRUCT1)*: bindings may
destructure structs, tuples, and lists:

```jet
.{ id, severity: sev } :: incident      // struct: bind id, rename severity
.{ kind, .. } :: event                  // partial needs mandatory `..` (E0326)
(x, y) :: point                         // named tuple, canonical order
[a, b] :: xs                            // list, runtime length check (E0315)
Val(n) :: maybe_port() ?? return      // refutable bind needs ?? fallback
```

Redundant `..` on a full pattern is E0327. Nesting one level. Dispatch-arm
struct-pattern heads (`.{ kind: "page", target, .. } -> …`) are source-shipped;
#341 owns the remaining user-facing dispatch/pattern wording audit.

**D-BINPAT1=A — binary patterns** *(ratified by owner 2026-07-12, card
#506; spelling amended by D-UNIFYLIT1=A 2026-07-28)*: `[U8].{"…"}` binary
pattern literals join the ONE pattern engine (D-PARSESTR1's grammar and
matcher, byte mode). Bit-typed holes —
`[U8].{"{version:U4}{ihl:U4}{len:U16be}{rest:...}"}` — with widths U1–U64,
`le`/`be` suffixes on multi-byte reads, and a final `{name:...}` rest
capture. Valid wherever string patterns are: `==` pattern tests,
if-table arms (refutable — table needs `else`), and consume mode via
`Reader.take_pattern([U8].{"…"})` (D-SHIFT1, prefix match + advance). Same
non-greedy anchoring and E0147-class ambiguity law as text holes. The
retired `b"…"` lexer prefix is gone.

**S77 — Field punning**: in a struct literal, bare `name` ≡ `name: name` when
a binding of that name is in scope; mixes freely with explicit fields.

**Declined (patterns)**: parameter destructuring (D-PAT6 — unpack on first
body line); Zig `.{ .field = value }` (D-DOTFIELD1); comprehension syntax —
use `filter_map`/`try_collect` (D-FAILCOMP1). S25 comparison distribution is
**retired** (D-S25-RETIRE1): `||`/`&&` never distribute; use `|`.

### Operators

**S13 — Logic & comparison**: `&&` `||` `!`; `==` `!=` `<` `>` `<=` `>=`.
Word forms are not operators.

**D-CHAINCMP1 — Chained comparisons**: `0 <= sev < 10` desugars to
`0 <= sev && sev < 10`, middle operand evaluated once. Same-direction chains
of `<`/`<=`/`>`/`>=` only; mixed direction is a compile error (E0333);
`==`/`!=` chains excluded.

**S71 / S35 — Optional chaining & fallback**: `?.` chains fields and methods
(`user?.address?.city`, `user?.display_name()`) yielding `T?`,
short-circuiting on None; non-optional left side E0047. `??` is the single
fallback for both `T?` and `T ? E`: `x ?? default`, `x ?? return`,
`x ?? panic("…")`. `or` is not an operator.

**S75 — Fan-out: REMOVED** (D-VERDICT-1324-1, owner ruling 2026-07-30).
Call fan-out `f.[a, b, c]` → `[f(a), f(b), f(c)]` is gone. The `.[` sigil is
reused by **D-SPREAD1=A** (card #1341, ratified 2026-07-31) for **member
spread**: `prefix.[a, b, c]` means `[prefix.a, prefix.b, prefix.c]`. Entries
are bare member names only (E0961 if not — E0963 remains S76 fixed-list
destructure). In list position the fields splice into the surrounding list;
elsewhere the result is an ordinary list of that length. Sema desugars before
inference; no TIR node. S76 `[T#N]` fixed-size lists are unaffected.
**D-FANOUT2** (no `s.{…}` second axis) is moot.

**D-SWIZZLE1 — Vector swizzles**: `v.xyz`, `v.wzyx`, lvalue `v.xy = .{…}` on
lane/vector types; overlapping writes diagnosed (E3110/E3111).
**D-VECARITH1**: element-wise `+ - * /` closed to compiler-provided lane and
linalg types; user structs use methods.

### Types

**S11 — Built-in type names**: `Int`, `Float`, `Bool`, `String` (capitalized).

**S42 — Numeric types & conversions**: `Int` (i64) / `Float` (f64) are the
defaults; sized menu `I8 I16 I32 I64 U8 U16 U32 U64 F32 F64` for experts/FFI.
Conversions are destination-owned named methods only (`Float.from_int(n)`,
`U8.from_int(n)?` for fallible narrowing, `Int.parse(s) => Int ? ParseError`);
no `as`, cast punctuation, or source-owned `to_*` aliases.
**D-NUMOPS1/2**: plain integer arithmetic **traps on overflow** at every
width; opt in per-op with `wrapping(…)` / `saturating(…)` /
`checked(…) => T?`. Per-type `MIN`/`MAX`, float `INFINITY`/`NAN`/`EPSILON`,
bit ops. **D-FLOATW1**: `core.math` is width-generic.

**D-INTLIT-WIDTH1=F / D-VERDICT-1304-1 / D-NUMWIDEN-CROSS1=E** *(ratified
2026-07-28)*: one numeric widening law applies to operators, arguments,
returns, and assignments. One operand may widen to the other; Jet never
searches for a third type and never narrows implicitly. Integer widening
requires full value-set containment. `F32` widens to `Float`. Integers cross
freely to `Float` from `I8 I16 I32 U8 U16 U32`, and to `F32` from
`I8 I16 U8 U16`. Other integer-to-float crossings use a runtime exactness
check and trap before rounding. A numeral is checked exactly at compile time.
Experts accept possible precision loss for one crossing with `approx(value)`.
**D-NUMLIT-PEER1=A** *(ratified 2026-07-30)*: a bare numeral adopts a
fixed-width peer only when a sized neighbour asks for it; with no sized peer
the numeral stays `Int`. Peerless `1000000 * 1000000` and `0 - 17` are therefore
`Int` arithmetic. Destination annotations and parameters still push width
through the expected-type channel (`take_u8(1 + 2)`).

**D-SHAPE-CONVERT1=A — destination type owns explicit conversion** *(ratified
2026-07-14, card #566)*: typed conversion is always
`Target.from_source(value)`. The source-kind suffix is canonical and bounded
(`from_int`, `from_u8`, `from_float`, or a source type's snake-case name), so
completion stays on the promised result type. Checked narrowing returns the
ordinary fallible result and is handled with `?`/`??`; safe numeric widening
is implicit.
Text interpretation remains `Target.parse(text)`; non-parsing materialization
such as `Secret.from_text` remains an ordinary destination-owned conversion. Numeric,
distinct/unit, enum, FFI, and user-defined static conversions share this one
call direction and the normal method coherence law. Source-owned `to_*`, casts,
and neutral `convert(value)` aliases are not accepted.

```jet
id :: UserId.from_int(raw_id)
byte :: U8.from_int(count) ?? return
ratio :: byte + 0.0
parsed :: Int.parse(text) ?? return
```

**S21 — Float display**: a `Float` always prints a decimal part (`-5.0`).

**S32 / D-OPT-SPELL1 / D-SHAPE3b — Optional and Result variants** *(D-SHAPE3b
ratified 2026-07-14 with owner substitution `Val`, not `Some`)*: `T?` uses
`Val(expr)` / `None`; `T ? E` uses `Ok(expr)` / `Err(expr)`. When the wrapper
type is known, `.Val` / `.None` / `.Ok` / `.Err` are the contextual forms,
including patterns. `Some` is never a spelling or alias. Old lowercase result
forms and foreign optional spellings receive ordinary current name/parse
errors; E0020's teaching path is retired. **D-RESULT-OPTION-CANON1**: `T?`
always means Optional; fallible is spaced `T ? E` / `T ?` (S34).

**S33 — Generic type arguments**: `Type<Args>` angle brackets; `[]` is
reserved for collections/indexing/shorthands. Calls infer type arguments by
default. **D-GENERIC-CALL1=A** *(ratified 2026-08-03)* allows explicit
`call<T>(…)` arguments on every generic free, namespaced, and method call.
The callee, `<…>`, and `(...)` stay adjacent, so spaced `a < B > (c)` remains
a comparison. The formatter writes `call<T>(…)`. Jet does not use Rust's
`::<T>` separator. This final uniform law supersedes S45's ban,
D-SERDE6's `decode<T>` exception, and D-SHAPE-CONVERT1's preserved general
ban.

**S45 — Generic functions & types**: `fn largest<T: Comparable>(…)`,
`struct Pair<T> { }`; multi-trait bounds are lists `<T: [A, B]>`
(D-VARARGBOUND1). No `where`. **D-LIB2**: associated types + default method
bodies; no higher-kinded types.

**S73 — Tuples**: named members only — `p :: (x: 1, y: 2)`, `p.x`, type
position `(min: Int, max: Int)`. No positional tuples, no `.0`.

**S76 / D-FIXARR1 — Fixed-size lists**: `[T#N]` is a compile-time-length
refinement of `[T]`, lowered to a **real stack array**. `::` + literal
⇒ `[T#N]`; widens one-way to `[T]` (by copy); `.map` preserves N; `.len` is a
compile-time constant; length-changing ops rejected (E0963–E0965).

**S29 / D-DOTCTOR1 / D-DOTCTOR2 / D-DOTCTOR3 — Construction**: the only
struct-literal spellings are **`Type.{ field: expr, … }`** (named) and
**`.{ … }`** (type inferred from expected type — the D-DOTCTOR2 expected-type
elaboration, now dot-spelled). Dotless `Type { … }` is E0320. Every field
exactly once, any order; flush style `Point.{x: 3.0, y: 4.0}` (S29-FLUSH).
`.{}` constructs an empty/unit value.
**D-DOTCTOR3=A** *(ratified 2026-07-24)*: the same `Type.{ body }` head is
universal for every type. The body uses that type's own literal notation —
elements for lists, entries for maps, one value for scalars — and elaborates
against the head exactly like an expected-type position. It never converts; a
mismatched body is the ordinary type error (teach `from_*` when a conversion
was intended). Scalar literal bodies keep comptime range checks in the E0135 /
E1003 family (D-RANGETYPE1 / D-SG9). Nested list bodies may be bare `.{ … }` or
plain `[ … ]`; both elaborate against the element type. Examples:
`U8.{ 250 }`, `[U8].{ 42, 0, 0 }`, `[U8#3].{ 255, 128, 0 }`,
`[String: Int].{}`, `Int.{ fetch_rows() }`, `[[U8]].{ .{ 1, 0 }, [0, 1] }`.
Amends D-EMPTYLIT1: `[T].{}` / `[K: V].{}` is the explicit empty collection;
bare `[]` stays contextual.

**S30 — Enums**:

```jet
enum Shape {
    Circle(Float)               // one payload: positional
    Rect(w: Float, h: Float)    // two+: named fields
    Empty
}
```

Value spelling `Shape.Circle(2.0)` or `.Circle(2.0)` where the type is known;
patterns take the leading dot (D-ENUMDOT1).

**D-DIST1 / D-DIST3 — Distinct types**: `Usd :: distinct Decimal` mints a
nominal type over a base; no inherited operators. **D-CAPBUNDLE1 /
D-CAPPLANE1=A** *(ratified 2026-07-23, card #762)*: capability rules expose
one concept on every type. `#Comparable` and `#Printable` derive from fields
on structs and enums, and re-expose the base capability on distinct types.
Distinct-only `#Numeric` exposes `+ - * /` and ordering for the same type
(E0138); `#CodableAsBase` exposes base coding.
`Usd + Eur` stays a type error; `.raw()` strips. **D-RANGETYPE1 — range-constrained
types**: `distinct Int(0..10)` is an `Int` provably within bounds; literal
conversion checks at compile time (E0135 out of bounds), runtime conversion
is fallible (`Severity.from_int(raw)?`, else E0136); an empty/reversed range is E0137;
arithmetic widens to the base type. The constraint bounds are integer literals,
not Range values; accepting a runtime value here would make the declaration
value-dependent (D-RANGE-VALUE1=A).

**D-FIELDPOL1 — Computed fields** *(ratified 2026-07-03, card #181; amended by
D-FIELDMEMO1=A)*: a struct field `name: T => expr` is an unmarked read-time
formula over the struct's current field values (siblings, data or computed,
are readable by bare name inside `expr`). `#Memo` is the explicit retained
form: the first read keeps its answer, and a write to any stored sibling in
its dependency graph drops that answer. Both forms are unsettable: the field
is absent from a `Type.{ … }` literal's required/allowed field list (E0339 if
provided), and direct assignment (`s.field = v`, `s.field++`) is also E0339. A
cycle among computed-field formulas, including self-reference, is E0338.
Codegen: not a Rust struct member — a synthesized inherent getter method
instead; every read routes to a call of it. `#Patchable` (D-PATCH1) excludes a
computed field from `T.Patch` and from `apply`/`diff`/`merge`. `#Codable` encode
calls the getter (the field appears in the wire output); decode never reads
into it.

**D-QUAL3 — Unit families**: `#UnitFamily(Currency) { usd, eur, gbp }` mints
one distinct type per member (usd → `Usd`, erases to the base numeric);
inexact, noncommensurable, or explicit-only cross-unit mixing reuses E0127;
exact same-dimension conversion follows D-QUANTITY-CONVERT1. **D-UNITLIT1 —
unit literals**: `500ms`, `12.50usd` resolve against in-scope family members
(E0134 unknown suffix); `e`+digits reserved for float exponents.
Dot-construction `px.{100}` also valid.

**D-SHAPE-QUANTITY1=A — Jet understands physical dimensions** *(ratified
2026-07-15; amended by D-DIMENSION-OPEN1=D)*: length divided by time is speed,
and length plus time is a Jet error. Unit information costs nothing at runtime.
Currency remains nominal and does not take part in dimension algebra.

Implementation identity is a sorted exponent map over nominal base dimensions.
Zero exponents are removed. API facts escape and serialize each sorted key with
its signed exponent. Sema erases the map before backend emission.

**D-DIMENSION-OPEN1=D — Open dimensions with the standard-unit prelude**
*(ratified 2026-07-28, card #1292)*: `#UnitFamily(Name, dimension, base: unit)`
declares a new base dimension. The seven SI base dimensions and the standard
unit catalog use this same surface in `Prelude/Units.jet`. The compiler has no
dimension-name table. A third-party dimension is nominal to its declaring
package, so packages share one by importing a common declaration.

**D-DERIVED-DIMENSION-CLAIM1=A — Derived dimension in the family header**
*(ratified 2026-07-30, card #1292)*:
`#UnitFamily(Force, dimension: Mass * Length / Time / Time, base: newton)`
claims an existing structural dimension. Sema normalizes the expression to the
same exponent map that value arithmetic uses. The coherent base member has
scale one.

**D-UNIT-SCALE-PROVENANCE1=A — Unit scales keep their source truth**
*(ratified 2026-07-30, card #1292)*: a unit scale is `Rational`,
`SymbolicPi`, `Conventional(value, source)`, or
`Measured(central_value, standard_uncertainty, source)`. Degree uses
`pi / 180`. Dalton records the BIPM/CODATA value and uncertainty. `mmHg`
records the NIST SP 811 convention. Rational and symbolic conversions use the
ordinary path. A measured crossing requires an explicit rounded conversion and
is never reported as exact. API facts and diagnostics retain the provenance.

**D-QUANTITY-DECL1=A — scaled and affine units extend `#UnitFamily`**
*(ratified 2026-07-16, card #603)*: the post-D-SHAPE2 `#UnitFamily` typed
rule gains one canonical `base` and exact rational `scale`/`offset`
metadata per member; D-QUAL3's family and generated-type semantics are
unchanged. Conversion is `canonical = stored * scale + offset`, inverse
`(canonical - offset) / scale`; metadata normalizes to arbitrary-precision
rationals at compile time. A family is closed in its declaring package — no
downstream reopening syntax; duplicate bases or members are errors, and a
same-spelled member in another family is a distinct type, never silently
unified. API snapshots record dimension, base, scale, offset, and package
provenance so conflicts fail and changes are semver-visible.

Implementation checkpoint (2026-07-19): the compiler normalizes exact ratios,
validates the canonical base, and keeps one package-owned unit fact registry for
dimension, kind, scale, and offset. Scaled and affine families implement the
closed Point/Delta algebra, value-aware exact implicit conversion, fallible
destination-owned exact conversion, the four explicit rounded-conversion modes,
`explicit_units`, imported `Quantity<Dimension, Kind>` generics, concrete
Codable identity, semantic inspection, API/SemVer identity, and AOT/JIT parity.

```jet
#UnitFamily(Length, base: meter) {
    meter
    millimeter(scale: 1/1000)
    foot(scale: 381/1250)
}

#UnitFamily(Temperature, base: kelvin) {
    kelvin
    celsius(scale: 1, offset: 27315/100)
}
// kelvin = celsius * 1 + 27315/100
```

**D-QUANTITY-POINT1=A — affine units generate concrete Point and Delta
types** *(ratified 2026-07-16, card #603)*: each affine unit generates two
concrete named types (e.g. `CelsiusPoint`, `CelsiusDelta`) via the existing
named-constructor (D-API-CTOR1) and destination-owned source-named
conversion (D-SHAPE-CONVERT1) laws — neither law is amended. They share an
erased numeric representation but carry distinct nominal/API/Codable
identity. Sema closes the algebra: Point plus Delta yields Point, Point
minus Point yields Delta, Delta plus Delta yields Delta; Point plus Point
and Delta minus Point are rejected. D-QUANTITY-DECL1 family metadata applies
scale+offset to Point conversions and scale-only to Delta conversions.

```jet
target :: CelsiusPoint.from_float(200.0)?
tolerance :: CelsiusDelta.from_float(5.0)?
next :: target + tolerance
drift :: next - target

FahrenheitPoint.from_celsius_point(target)?
FahrenheitDelta.from_celsius_delta(tolerance)?

target + target
// error: two Temperature points cannot be added
// fix: subtract them for a delta, or add a CelsiusDelta
```

**D-QUANTITY-TYPE1=A — generic APIs bound dimension and kind via
`Quantity<Dimension, Kind>`** *(ratified 2026-07-16, card #603)*: concrete
unit APIs keep their D-QUAL3 named types and D-QUANTITY-POINT1's Point/Delta
split. A reusable generic function instead names a compiler-known
`Quantity<Dimension, Kind>` bound — a compile-time bound, not a runtime
wrapper. Monomorphization retains the concrete unit and Point/Delta
representation selected at each call site; Codable uses that concrete
instantiation's wire rule, and API snapshots record the normalized
dimension, kind, and input/output relations. Every input/output must
determine one concrete unit and kind; an undetermined result is rejected.

```jet
fn mean<Q: Quantity<Length, .Linear>>(xs: [Q]) => Q { xs.mean() }
fn shift<P: Quantity<Temperature, .Point>, D: Quantity<Temperature, .Delta>>(p: P, d: D) => P { p + d }

fn mystery<Q: Quantity<Length, .Linear>>() => Q { Meter.from_int(1)? }
// error: return unit is not determined by the signature
// fix: accept a unit-bearing input or return Meter
```

**D-QUANTITY-CONVERT1=B — implicit exact conversion by default; scoped
explicit-only opt-in** *(ratified 2026-07-16, card #603)*: same-dimension units
convert automatically in mixed arithmetic, argument passing, and binding when
the conversion is exact. Mixed arithmetic uses the finer operand's unit so
whole-number storage remains exact; equal scales keep the left operand's unit.
Point conversion applies scale plus offset; Delta conversion, including the
delta in Point-plus-Delta, applies scale only. Point plus Point remains
rejected. The compiler never rounds silently: a conversion that cannot be
represented exactly is a compile error naming the destination-owned
`from_*_rounded` fix. This amends E0127 for exact commensurable-unit mixing;
inexact mixing remains rejected.

Conversion checks use the exact rational value represented by the source
`Float`, scale, and point offset. `Destination.from_source(value)?` succeeds
only when that rational is an integer that the destination `Float` can
represent exactly. When precision loss is intentional,
`Destination.from_source_rounded(value, mode, digits: n)?` first computes the
same exact rational and then rounds to `n` nonnegative destination decimal
places. `.TowardZero` truncates, `.Floor` rounds toward negative infinity,
`.Ceiling` toward positive infinity, and `.NearestEven` chooses the even result
at an exact tie (so `-2.5` becomes `-2`, while `-3.5` becomes `-4`). Point
conversion includes the offset; Delta conversion never does. Integer-backed
destinations require `digits: 0`; current physical-unit storage is `Float`. The
rounded rational must itself be exactly representable by the destination, so
binary storage never adds precision loss beyond the requested decimal rounding.
A negative `digits`, non-finite input, or unrepresentable result is a conversion
error carried by `Result`, never `NaN`, infinity, or a trap. AOT
and resident execution use the same conversion mechanism. For an unknown
unrestricted `Float`-backed value, only an identity scale and offset proves
exact finite conversion over the complete source domain; integral coefficients
alone are not proof.

Per D-PACKAGE-POLICY-SCOPE1, `policy: .{ explicit_units: true }` in
`package.jet` restores explicit-only conversion at package scope, and
`#Policy(explicit_units)` does the same at module, function, or block scope.
The normal D-MARK-SCOPE1 inheritance/provenance law applies.

```jet
inner_diameter :: 42millimeter
length :: 3meter
total :: length + inner_diameter
// 3042millimeter — finer unit wins, exactly

fn fits(depth: Meter) => Bool { depth > 0meter }
fits(3000millimeter)
// argument converts exactly to 3meter

alt_km: Kilometer = 1500meter
// error: 1500 meter is not an exact number of kilometer
// fix: Kilometer.from_meter_rounded(1500meter, .NearestEven, digits: 0)?

# package.jet
policy: .{ explicit_units: true }

#Policy(explicit_units)
module dosing

length + inner_diameter
// error[E0127]: explicit_units requires a written conversion
// fix: Millimeter.from_meter(length)?
```

**D-TYPEALIAS1 — Aliases**: `alias X :: Y` transparent aliases, scoped to
shortening generic spellings only — not primitive/unit newtypes (use
`distinct`). **D-TYPE-ALIAS-CANON1** + **D-LISTMAP-CANON1=A**: `[T]`, `[K: V]`, `*T`
are the only default container/pointer spellings; `List<T>`/`Map<K,V>`/`Ptr<T>`
are dead. Named specific collection spellings stay named rather than short
bracket forms; shipped today: `Set<T>`, `SortedSet<T>`, `Deque<T>`,
`PriorityQueue<T>`, `Cache<K,V>`, `Bag<T>`, `BitSet`, and `ByteBuffer`.
`HashMap<K,V>` and `BTreeMap<K,V>` are reserved names for specialized map
implementations.

**D-ALIAS-OP1=B** *(ratified 2026-08-06, card #1513; amends D-TYPEALIAS1)*:
Alias declarations bind with `::`; the retired `=` spelling emits E0378.

**D-BIGINT1** *(home moved to `core.math` by D-CORE-NUMERIC1=A, 2026-07-12)*: Core `BigInt`, explicit construction `BigInt(…)`/`BigInt("…")`;
`Int` never auto-promotes (E0130–E0133). **D-DECIMAL1**: arbitrary-precision
base-10 `Decimal` in `core.math`; default-on lint L0504 fires when a
money-named field holds a float (`#[allow(float_money)]` suppresses).

**D-STATE1 — Typestate** *(D-STATE-REQ/TRANS/DECL)*: states declared in a
`state TypeName { A, B, C }` block; `#State(S) fn m(self)` requires state S;
`#Transition(From, To) fn` advances it (`_` from-state = entry constructor).
Wrong-state call E0150; markers erase in codegen. Ordering falls out of the
transition graph.

**D-REFINE1 — Refinements**: `#Invariant("value >= lo && value < hi")` before
a `distinct Int` declaration records a pure linear integer bound. The first
shipped prover uses that bound to prove fixed-list indexes in-bounds; no new
keyword.

**D-PENDING1**: blessed loading-state enum `Loadable<T, E>`
(idle/loading/loaded/failed) in Core. **Declined (types)**: `newtype` keyword
(D-SUGAR4); tracked-uncertainty dimension (D-UNCERTAIN1, deferred);
content-addressed definitions (D-CADEFS1, frozen).

### Collections

**S37 — List literal**: `[a, b, c]`; empty `[]` needs a context type
(**S78**: `[]` infers from expected type; explicit `[]: [T]` always accepted).

**S38 / D-EMPTYLIT1 — Map literal**: `["key": value, …]`. **D-EMPTYLIT1**
*(ratified 2026-07-04)*: `[]` is the ONE empty-collection spelling for both
list and map — type-directed from the expected-type context (a `[K: V]`
binding/field/return/arg makes empty `[]` a map, same as `[T]` makes it a
list). `[:]` is retired; `[` immediately followed by `:` is an ordinary
parse error (E0003), no special-cased teaching text.
**D-DOTCTOR3=A** *(ratified 2026-07-24)* amends this: `[T].{}` and
`[K: V].{}` are the explicit empty forms that name the collection type on the
value; bare `[]` keeps working wherever context already supplies the type.

**S65 — List type shorthand**: `[T]` is the canonical list-type spelling.

**S64 — Map shorthand & entry iteration**: `[K: V]` is the canonical map-type
spelling. One-binding map iteration yields `.key`/`.value` entries;
two-binding `loop name, amount; fruits` also supported.
**D-RANGE-EXCL1=C** extends two-binding to sequences: `loop i, item; xs` yields
index then item; one-binding stays item-only. `xs.indexes()` yields every valid
`Int` index.

**S39 — Indexing**: `xs[i]` / `m[k]` stop with a friendly report on
OOB/missing key; `xs.get(i) => (T?)` safe access; `m[k] = v` inserts.

**S40 — Slicing**: `xs[a..b]` inclusive, copies (no exposed references);
`s.slice(a..b) => String` on character positions; L0501 lints slice copies in
loops.

**D-ITER1 / D-ITERTOOLS1=A — Iterator adapters**: `map`, `filter`, `each`,
`find`, `any`, `all`, `sort_by`, `reduce`, `take`, `skip`, `step_by`, `dedup`,
`chunks`, `windows`, `indexed`, `indexes`, `zip`, `unzip`, `take_while`, `skip_while`,
`flat_map`, `filter_map`, `scan`, `fold`, `sum`, `product`, `min`, `max`,
`min_by`, `max_by`, `group_by`, `count_by`, `partition`, `flatten`, and
`intersperse` use one iterator model. Adapters return lazy `Iter<T>` views;
materialize with `to_list`, `collect`, or terminal reducers. String `.split`
returns the same `Iter<String>`. Unconsumed chains are `#MustUse` (E0419);
driving a consumed `Iter` twice is use-after-move (E0121).
`indexed()` (D-RANGE-EXCL1=C amend of D-ITER1) yields `(idx: Int, item: T)`;
there is no public `enumerate` adapter.

**D-CORE-EAGER1=A / D-LOOPMAP1=B amends the default above for concrete
collections**: `map` and `filter` on a real `List`, `Map`, or `Set` execute
now and return a plain collection. Write `.lazy()` first to enter the same
deferred `Iter<T>` vocabulary. Sources that already arrive over time, such as
file lines, streams, channels, and `String.split`, remain deferred. The rest
of the D-ITERTOOLS1 adapter ledger stays on `Iter`; the #1479 surface is
covered by `examples/features/collections/iter_adapters.jet` under this
split. Adjacent eager adapters may fuse when their intermediate is not
observable, without changing callback order or count.

**D-ZIPPAD1 (ratified on card #1400):** the zip family is one lazy iterator
mechanism. Free calls and methods accept any number of sequence inputs and
preserve every input type in named row fields. `zip` is strict and reports
E0128 when lengths differ; `zip_short` stops at the shortest input; and
`zip_pad` continues to the longest input. Zero free inputs produce an empty
`Iter<Unit>` and one input is the identity sequence. Free row labels are
preserved; methods use `a`, `b`, `c`, and so on. `zip_pad` without a fill uses
`None` for each missing value, `fill: value` supplies one typed value for every
column, and `fills: (field: value, ...)` supplies typed per-column values.
Every form stays lazy and has the same AOT, dev/JIT, and interpreter meaning.

**D-COLLBREADTH1 / D-ITERTOOLS1=A**: `Set<T: [Hash, Eq]>`,
`SortedSet<T>`, ring-buffer `Deque<T>`, `PriorityQueue<T>`, `Cache<K,V>`,
`Bag<T>`, `BitSet`, and `ByteBuffer` in Core (E0506). `[K: V]` is the default
ordered map spelling; specialized map names stay reserved. **D-ENC-DYN1**:
`DataTree` is the single dynamic value
(`.Object/.Array/.Int/.Float/.Text/.Bool/.Null`); `JSON`/`TOML`/`YAML`/`CSV`
are aliases over it. **Declined**: `[..]T` spelling — zero-copy comes as
`View<T>` library type (D-DYNARRAY1).

### Strings & literals

**S8 — Interpolation**: `"hi {name}"`; no `+` concatenation.
`{value:Debug}` selects the Debug rendering (D-DISPLAYDBG1, amended by
D-ONCE-HASH1=B).

**S20 — Escapes**: `\n` `\t` `\"` `\\`; literal braces `{{` `}}`.

**S41 — Char & string length**: `Char` built-in, `'a'` literals; `s.len()`
counts Unicode scalars; `loop c; s.chars()`; no `s[i]` (E0503).
Grapheme clusters + NFC/NFD live in opt-in `core.text.unicode` (D-GRAPHEME1).

**D-STR-AFTER1 — `String.after`/`.before`** *(ratified/implemented
2026-07-04)*: `s.after(sep)` returns the substring strictly after the first
`sep`; `s.before(sep)` the substring strictly before it. `sep` absent -> the
whole original string on both sides (symmetric identity fallback, matching
`.replace`'s no-match-is-unchanged convention — no `Option`/error to
unwrap; I8, one way to mean it). No `.after_last`/`.before_last`: the
ratified set covers only the first-occurrence case actually needed
(email/path-prefix splitting); a last-occurrence sibling is unrequested
surface growth without a driving example.

**S67 — Numeric literals**: `_` separators (`1_000_000`); `0x`/`0o`/`0b`
prefixes (E0001 if empty); float exponent `6.022e23`; `1..10` still lexes as
a range.

**S70 — Multi-line strings**: `"""…"""`, Swift-style trimming (newline after
opening and before closing dropped; closing-quote column sets stripped
indent); escapes and `{interp}` stay active; unterminated is E0002.

**D-PARSESTR1 — Interpolation literal as pattern**: the same `"…{hole}…"`
literal that formats a string may sit in pattern position (if-table arm
head, `subject == pattern`) and match instead: it matches the fixed text and
binds each `{hole}` to a name (untyped binds `String`). A typed hole
(`{id:Int}`; `Int`/`Float`/`Bool`/`String`) is a fallible read — it binds
only if the matched text reads as that type. Holes are non-greedy, anchored
by the literal text between them. Always refutable (a typed hole can fail to
read, and the literal text might not match), so an `if == {}` table needs an
`else` arm (E0148). **D-PARSESTR2 — ambiguity rule**: two interpolation
holes with no literal text between them is E0147 (add an anchor, or type
them so the boundary is unambiguous); a hole-free string in pattern position
is plain text equality, not a pattern (I8). **D-TYPEDTEXT1 — Typed text** *(amended by D-UNIFYLIT1=A)*: a typed-literal
head `SQL.{"…"}` / `HTML.{"…"}` elaborates to that checked value — each
`{hole}` becomes a bound parameter (SQL) or an HTML-escaped insertion
(HTML); a runtime `String` reaching the position directly is E0149.
`SQL.raw("…")`/`HTML.raw("…")` is the sole audited escape. Implemented for
typed-literal heads (and annotated bindings that still use a typed head);
`.template()`/`.params()` (SQL) and `.text()` (HTML) read the checked value
back. Bare `"…"` never elaborates into these types.
**D-TYPEDTEXT2 — Typed text amendment** *(amended by D-UNIFYLIT1=A)*:
hole-free bodies also elaborate under a typed head; the former `sql"…"` /
`html"…"` prefixes are retired — use `SQL.{"…"}` / `HTML.{"…"}`.
User-defined prefixes remain deferred (and D-LITERAL-PREFIX1's prefix
surface is superseded by D-UNIFYLIT1=A).

**D-UNIFYLIT1=A — unify domain text + pattern modes** *(ratified 2026-07-28,
card #1265)*: one surface law — the head names the language; the body is that
language's quoted recipe. Domain text: `SQL`/`HTML`/`Sh.{"…"}` only (plus
`.raw`). Byte patterns: `[U8].{"…"}` in pattern / `Reader.take_pattern`
position (retires `b"…"`). Text patterns keep plain `"…"` convenience and
optional `String.{"…"}`. Amends D-TYPEDTEXT1/2 and D-BINPAT1; supersedes
D-LITERAL-PREFIX1's prefix-first surface.

**D-REGEX-LIT1=D — checked Regex literals** *(ratified 2026-07-28, card
#1283)*: a regex pattern is always a typed literal. `Regex.{"…"}` names a
reusable pattern value. The head can be inferred as `.{"…"}` at a
Regex-typed positional or named argument. Sema validates the complete literal
with the linear engine grammar, and reports E0152 with the pattern offset.
One-shot `core.regex` calls take `Regex` and return their match value directly.
A bare `String` is E0152. Runtime text remains fallible through
`core.regex.compile` or `compile_with`.

**D-BOUND-HEAD1=A — checked URL, Path, and DateTime heads** *(ratified
2026-08-07)*: `URL.{"…"}`, `Path.{"…"}`, and `DateTime.{"…"}` are checked
typed values, not fallible runtime constructors. Sema validates the complete
literal skeleton and emits E0155 for an invalid head. A URL hole is percent
encoded; a Path hole is one encoded component, including `.` and `..`; a
DateTime head cannot contain holes. Runtime strings keep the existing
fallible `parse`/`from` constructors. The forms ride D-UNIFYLIT1, D-DOTCTOR3,
and D-CORE-PATH1; no compatibility spelling is retained.

**D-BOUND-LAW1=A — one crossing law** (ratified 2026-08-07, card #1813):
nothing foreign becomes Jet silently. Every comptime, build, link, and runtime
crossing names its schema and leaves its fact. Boundary planning uses the
five-column grid in spec.md: time, schema, checker, evolution, and fact. The
literal, manifest, dependency, link, wire, validation, migration, fact, trust,
and error rules are instances of the same grid, not separate mechanisms.

**D-BOUND-TAINT1=A — origin facts clear on typed decode** (ratified
2026-08-07, card #1817): external net, file, environment, process, and FFI
returns seed compiler-owned origin facts in the existing taint flow store.
Facts propagate through ordinary expressions and clear only after successful
typed validation, including typed codec decode and typed construction. The
existing SQL.raw, HTML.raw, and Sh.raw gates reject origin-marked text.
Existing #Scrub functions and the jet inspect gates remain the explicit
audited escapes. Grid cell: run / wire value. The fact is sema-only and is
erased before every execution tier.

**D-BOUND-EVOLVE1=A — published schemas preserve unknown wire fields**
(ratified 2026-08-07, card #1818): a PublishedSchema codec retains unknown
wire fields and re-emits them in their original order. Plain Codable keeps its
drop behavior. DenyUnknownFields still refuses unknown fields with E2412.
The holder extends D-MIGRATE1/D-MIGRATE4 bookkeeping and does not change
known-field decoding, marker behavior, or FieldError diagnostics. Grid cell:
run / wire value.

**D-SHIFT1 — Shift-style stream parsing (ratified 2026-07-01, c7shift)**: the
Jai `shift` idiom lands as a core cursor surface, not an operator (option C —
`r >> U32` punctuation — rejected). `Reader.over(bytes)` wraps a `[U8]` with a
position: `read_u8`/`read_u16_le|be`/`read_u32_le|be`/`read_u64_le|be`,
`take(n: Int)`, `remaining()`, `is_at_end()`; every read advances and is
fallible (`T ? String`) — a bounds miss is an ordinary error value.
**D-BINREAD-LEN1=A** narrowly extends the `take` length slot to accept
`U8`/`U16`/`U32`, which widen internally to `Int`; `U64` remains an explicit
conversion, and S42 sized-int separation is unchanged everywhere else.
`Cursor.over(s)` is the text sibling: `take_until(delim)`, `skip_ws()`, and
`take_pattern("…{hole:Type}…")`, which reuses the D-PARSESTR1 pattern grammar
and matcher engine (I8 — one engine) in consume mode: it matches a *prefix*
of the remaining text, advances past it, and returns the typed holes. The
pattern must be a literal string (E0003 otherwise); this one call position is
the only place the literal is parsed as a pattern rather than interpolation.
`Reader`/`Cursor` are reserved core names with the user-type-wins guard: a
user type of the same name shadows the core surface entirely.

### Errors

**S7 — Propagation**: postfix `?` on a fallible call.

**S34 — Failure-returning return**: `T ? E`; bare `T ?` means `T ? Err`. A `? E`
clause after a parameter list is a full unit-fallible return annotation per
D-FAIL-UNIT1; value-returning signatures keep `=> T ? E`. Lowers to Rust
`Result` (not surface syntax).

**S80 — Default error value** *(D-ERR2, D-S80-RUN1 and D-FAIL-ERROR1=A,
amended by D-FAIL-EXIT1, 2026-08-06, card #1528)*: `Err` is both the default
error type and its constructor. `Err("msg")`, `Err("msg", code: "CFG404")`, and
`Err("msg", cause: e)` build one Prelude-owned value with readable `message`,
`code`, and `cause` fields. `Error` is deleted, not aliased. Bare `T ?` means
`T ? Err`; `T ? Err` is the explicit form. An `Err(e)` in a result-arm context
still wraps the typed error value `e`. Argument shape distinguishes the two
readings. `fn run()` is fallible by default; an expert may pin `fn run() ? E`.
Returned default errors print one report frame and exit non-zero. Cross-type
`?` conversion uses declared `impl Source => Target`; the retired
default-conversion trait, helper, and compiler arm are deleted by D-FAIL-CONV1.

**D-ERRCTX1 — Error context** *(amended by D-FAIL-CTX1)*: a postfix `?` may
carry a lazy string note. Every `?` hop joins the failure journey on every
tier. The `.context` method and the internal error-context lowering sentinel
are deleted. No new binder grammar is added.

**D-FAIL-MODEL1=A — one report, three routes** *(ratified 2026-08-06, card
#1507)*: every failure is one product: a report with a code, message, why,
fix, and source location. Attribution picks the route. A broken world promise
returns a value in the signature. A broken code promise stops the program with
attribution. A substrate failure stops and is contained at the nearest
boundary. Routes change only at spelled boundaries, such as a task join or the
process edge. No failure is lost or reworded.

E0401/E0402 and `.drop("reason")` remain delivery-exactly-once rules on the
value route. I2 and exit 101 remain substrate attribution. S36 owns the
process-edge conversion. D-VALIDATE1's three layers are the attribution split.

**D-FAIL-CARRIER1=A — one outcome carrier** *(ratified 2026-08-06, card
#1527)*: `T?` and `T ? E` are two views of one carrier, not two types. The
two type spellings are unchanged (D-RESULT-OPTION-CANON1 stands). An outcome
has three facts: a payload, a verdict, and the reports it collected. Three
methods reach those facts, and no new grammar is added.

`.or_err("why")` lifts a clean absence into a failure: the payload rides
through untouched and only the report changes. `.partial()` reads the payload
a failure kept and answers `T?`. `.notes()` reads what a failure had to say
and answers `[String]`.

Both middle states live on the outcome value. An error type opts in by
carrying them on its report — the surviving payload under the field name
`partial`, at the same type the outcome carries, and the notes under `notes`.
An error type that declined either one reports the ordinary E0302. Because
nothing is stored beside the value, two outcomes alive at once never share a
fact, reading one twice answers the same thing both times, and an outcome that
crosses a thread takes its facts with it. A success has no report, so
`.partial()` on one answers a clean absence and `.notes()` answers an empty
list. Writing a note as a `?` hop, and the journey it travels on, belong to
D-FAIL-CTX1.

The verdict and the notes erase from the happy path: the clean report is
zero-sized, so `T?` keeps the payload's own size, and an error type that opts
into neither middle state pays for neither. Constants:
`METHOD_OUTCOME_OR_ERR`, `METHOD_OUTCOME_PARTIAL`, `METHOD_OUTCOME_NOTES`,
`FIELD_OUTCOME_PARTIAL`, `FIELD_OUTCOME_NOTES` in
`crates/jet-foundation/src/Syntax/package_files.rs`. The one carrier lives in
`crates/jet-foundation/src/Outcome.rs`, embedded as the first prelude part,
so every tier names one type. Examples:
`examples/features/errors/or_err.jet` and
`examples/features/errors/partial_and_notes.jet`.

**D-FAIL-ERROR1=A — `Err` is the default error value** *(ratified 2026-08-06,
card #1528)*: `Err("msg")` builds the default error. Labels add `code` and
`cause`, and programs read `.message`, `.code`, and `.cause`. The default
error type is `Err`; bare `T ?` implies it. This amends S80's builder spelling
and deletes `Error` as a type name, not an alias.

**D-FAIL-CONV1=A — one conversion rail** *(ratified 2026-08-06, card #1529)*:
the retired default-conversion trait, helper, and compiler arm are deleted.
Error conversion uses the declared `impl Source => Target` form, including a
conversion into the default error. The default-error target may name a foreign
source type; typed targets keep E2406's orphan rule. This amends D-ERR2 and
D-LIB3. E2402 and E2406 teach the one rail and their registered UI snapshots
follow it.

**D-FAIL-CTX1=A — notes ride `?` and the journey is automatic** *(ratified
2026-08-06, card #1532)*: a postfix `?` may carry a string note. Each `?` hop
joins the failure journey on every tier, whether it has a note or not. Notes
use D-ERRCTX1's lazy interpolation rule. The `.context` method and the
internal error-context lowering sentinel are deleted.

**D-FAIL-BREACH1=A — one runtime report family and one renderer** *(ratified
2026-08-06, card #1530)*: arithmetic traps, bounds traps, contract failures,
`require`, `panic`, and `#Todo` carry registered E30xx codes. One Prelude
renderer owns the what/why/fix text and source arrow on every tier. `jet
explain` answers runtime codes, `.expect_fail(E3010)` names an expected stop,
and web throws carry the same report. `#Todo` is a real stop at exit 70, not
exit 101. I4 and I9 apply to runtime failures.

**D-FAIL-TIER1=A — contracts check on every tier and erase under proof**
*(ratified 2026-08-06, card #1531)*: contracts have a TIR node and one
Prelude check, so AOT, JIT, and the interpreter run the same check. A `#Pre`
breach points at the call site. A `#Post` or `require` breach points at the
body. A range proof removes a check; the unproven case keeps it. This amends
D-PREPOST1 with proof-keyed erasure while keeping its explicit strip opt-out.

**D-FAIL-EXIT1=A — fallible entry and one exit law** *(ratified 2026-08-06,
card #1533)*: `fn run()` is fallible by default. An unhandled error reaches the
process edge as a full report and exits 1. A breach or program-side fault exits
70. Exit 101 belongs to Jet defects. `#Todo`, raw Prelude panics, and direct
program-side exit sites route through the report boundary; the `CryptoError`
entry arm is deleted. This amends S80's entry clause, retires E0122 and its
allow-list, and updates S36, the architecture exit table's producer column,
and E3001's entry text.

**D-FAIL-UNIT1=A — unit-fallible signatures omit arrow and unit** *(ratified
2026-08-06, card #1534)*: `fn save(path: String) ? IOError` has no arrow and no
unit. Bare `fn sync() ?` means the default error. A value-returning signature
keeps the arrow: `fn load() => Config ? E`. This retires S80's arrow-and-unit
form and extends S34 so a `?` clause after the parameter list is a full
return annotation.

**D-FAIL-BIND1=A — `err` is ambient inside a fallible fallback** *(ratified
2026-08-06, card #1535)*: inside a `??` fallback for a fallible value, `err`
names the report. It is in scope only in that fallback. Optional fallbacks
reject `err`. No lambda, binder, or parentheses are added. The
`?? (next)` value form in D-ORRETURN-CANON1 is unchanged.

**D-FAIL-EDGE1=A — the program edge uses target-native delivery** *(ratified
2026-08-06, card #1536)*: an unhandled error crosses the final boundary in the
target's native shape while carrying the same report. A CLI prints the frame
and exits 1. A web app raises a typed error object. A wasm module returns a
host-visible error value. A service emits a structured report, and a test
fails with the report. Program source is unchanged; the target kind selects
the boundary. D-FAIL-BREACH1 owns report words. Raw JavaScript throws and raw
wasm panic paths are replaced.

**S36 — Bug stops** *(amended by D-FAIL-BREACH1 and D-FAIL-EXIT1)*:
`panic("msg")` and `require(cond[, "msg"])` are Prelude builtins. Program-side
stops use the registered E30xx report family and exit 70. Exit 101 is reserved
for Jet defects.

**D-IGNORERET1 / D-IGNORERET2** *(as amended by D-MARK-DISCARD1=A,
2026-07-11, card #498)*: discarding a fallible/`#MustUse` result requires
visible intent. `.drop("reason")` is the ONLY discard spelling — the
per-value reason keeps every discard auditable at its site. The
`#Suppress(MustUse) { … }` region form is removed from the grammar
(ordinary unknown-marker error): a block that silently swallowed every
result would also swallow the fallible call someone adds later.

**Teaching & lint law**: `=` in a condition is E0322 with a "did you mean
`==`?" fix (D-ASSIGNCOND1). Identifier shapes use D-SHAPE-CASE1=C's
zero-exception errors, not a parallel lint. Semantic-smell lints — float `==`,
duplicate branches default-on; full always-true condition coverage is tracked
by #343.

### Modules, visibility & imports

**S16 — `use`** *(D-S16-USE, D-MOD1/2, D-MOD-DIR, D-SELIMPORT1)*: quotes mean
a file path, no quotes mean a module; `as alias` optional in both.

```jet
use "./lib"                      // file path, namespace lib
use "grades/scoring" as g        // file path, alias
module math                      // finds math.jet, then math/module.jet (E0607)
use math.clamp                   // selective import
use math.[sin, cos as c]         // grouped + aliased selective import
```

Two-step dot access; `use math.*` wildcard rejected (E0612, D-GLOBIMPORT1).
Re-export is `pub use` (D-MOD4); a directory module's summary file is
`module.jet`. Ambiguous module resolution E0606/E0607. `import` is not a
keyword.

`.[ ]` has one prefix-member law. In expression position, entries remain
members and produce their values (`point.[x, y]`); after `use`, entries produce
aliases (`use math.[sin, cos as c]`). The expression grammar keeps member
entries, while the import grammar additionally permits `as` aliases and dotted
paths. The prefix itself may be dotted at any depth, so
`use core.encoding.[json, csv]` walks the same members as
`use core.[encoding.json, encoding.csv]`.

**D-NAME-WALK1=A** *(ratified 2026-08-07, card #1803)*: a module body is a
namespace like any other, so `use` and `pub use` are legal inside
`module name { }`. They resolve in the enclosing file's scope and bind only
inside that body.

**S18 — Visibility** *(D-MOD3, D-VISDEFAULT2, D-PUBPKG1)*: private by
default; `pub` exports. `#PubFile` flips a file to public-by-default with
`priv` marking exceptions (E0412–E0418). `pub(package)` exposes to the same
package/workspace only (other `pub(...)` forms E0411). Cross-file private
access E0605/E0609.

**U3 / D-SHAPE-MODULEINTERNAL1=A — Module declarations and internal discovery**
*(ratified 2026-07-15, card #602)*: `module name { … }` is the single outermost
construct; multiple may live in one file. Automatic discovery skips a module
whose declared name begins with one underscore. An explicit
`use project._name` remains allowed under ordinary visibility rules: the
underscore changes discovery, not access, and tools report it as skipped rather
than disabled. `jet project parts` is the authoritative listing; `--skipped`
shows only omitted declarations. LSP and Canvas omit closed skipped-only source
files from semantic indexes, keep open files visible, and index an internal
module after an exact `use project._name`. Duplicate declarations remain a
conflict even while skipped. Renaming `_name` to `name` opts it back into
discovery. Modules never import each other — they contribute to the merged whole. Reserved
namespaces currently live for Jetpack/jetos: `env` (`Env`), `system`
(`System`), OCI/jetos `image` (`Image`), `workspace`. **D-JPK-MODBODY1**:
role namespaces live in the declaration
name — `module env.dev { packages: […] }`, `module image.server { … }`.
`system.*` is the jetos host declaration surface.

**U8 — Manifest fields nest in the module body**: a module's `sources:`
(`name: target@provider` or bare-path entries, merged by key) and `imports:` are fields
inside `module name { … }`, never file top-level.

**D-ENV-FACET1=A — environment modules use the environment noun** *(ratified by
owner 2026-08-03, card #1104; selector amended by D-ENVFLAG1=A, card #1619)*:
a typed environment may declare multiple `env.<name>` contributions, but one
contribution is active at a time. The beginner default is `dev`, then `default`,
then lexical order. Experts select one explicitly with `jet env info --env
<name>`; the selector applies to all Jetpack commands that load an environment
plan. `--preset` selects a named environment composition. The build
`--profile` flag remains the optimize bundle; JetOS generation commands keep
their own namespace and state.

**U4 — Import-tree discovery**: `imports: find("./modules")` auto-discovers
`.jet` files and merges typed contributions; no manual lists.

**D-GENMOD1 / D-GENMOD2 — Generic modules**: ML-functor style — a module
parameterized by types and values; instantiation yields a specialized normal
module. Type parameters (`K: Hash`) use `<…>`, value parameters (`capacity: Int`)
use `(…)`, and application mirrors it: `module cache64 :: cache<String>(64)`.

**D-GENMOD-VALUE1=A — Closed value specialization**: value parameters are
immutable Tier-0 comptime values of type `Bool`, `Int`, `Char`, `String`, or a
fieldless enum. Arguments are evaluated and normalized before specialization;
they do not convert between value types. An `Int` value parameter may also fill
the narrowly approved generic-module layout slot `[T#capacity]` under S26,
S76, and D-FIXARR1.

Parameter kind comes from its declaration list, never casing. An angle-list
parameter is a type parameter (optionally bounded); a parenthesis-list
parameter is a typed value parameter. Alias arguments remain unresolved syntax
until the target resolves, then each slot contextualizes one ordinary type or
expression. There are no defaults,
named arguments, inference, packs, implicit conversions, or module-valued
parameters.

A value expression must type-check exactly and finish in the fuel-limited
Tier-0 pure comptime interpreter before expansion. Literals, fieldless enum
cases, pure arithmetic/comparison/Boolean/string/`if` expressions, earlier
immutable `const`/`comptime` bindings, and wholly Tier-0 pure call graphs are
admitted. Runtime or sibling/template references, ambient state, mutation,
effects, `find`/`fetch`/embedding/reflection, panic, exhaustion, overflow,
division by zero, and non-values reject. After validation, results bind as
immutable comptime constants in ordinary value positions, marker values, and
`[T#capacity]`; they cannot manufacture names, paths, fields, markers, traits,
types, overloads, dispatch targets, or enum discriminants. No other type
position and no general const-generic/type-computation surface is opened.

Canonical value bytes are: Bool `0x01 || 0x00/0x01`; Int `0x02 ||` signed i64
two's-complement BE; Char `0x03 ||` Unicode scalar as u32-BE; String `0x04 ||`
u64-BE UTF-8 length `||` exact UTF-8; enum `0x05 ||` u64-length-framed resolved
package/module/type identity `||` u64-length-framed variant. Strings receive no
Unicode/case/path normalization. Arguments concatenate in declared parameter
order. Type plus Jet value defines equality; spelling, span, binding name,
expression tree, and evaluation route never contribute.

**D-GENMOD-BODY1=A — Full module bodies, definition-site scope**: a generic
module admits every declaration and legal marker admitted by an ordinary
module: functions, structs, enums, tags, runtime and comptime constants,
traits, trait/error-conversion/OS-gated impls, tests, benches, ordinary and
generic nested modules, and aliases. Names outside the template resolve in the
template's definition-site lexical scope. A specialization gains no additional
authority from its application site.

Inline `use`/`pub use` is admitted, on the same terms ordinary module bodies
admit it (D-NAME-WALK1=A).
File/package/build/FFI/C-module/generated-binding/role-module/policy/protocol/
state/migration/user-derive/generic-package declarations remain in their
existing homes. Existing markers apply only to their already-legal declaration
kinds; tests and benches instantiate only in their existing modes. Expanded
impls obey ordinary orphan/coherence law, so supplied or captured external
types/traits do not become local. Public specialized APIs cannot expose private
captures. Template/alias lookup is order-independent: sema builds one dependency
graph, expands in deterministic topological order, and reports direct or
indirect cycles as E0855 with the complete chain; it never expands until stable.

**D-GENMOD-IDENTITY1=A — Applicative instance identity**: one resolved template
DefinitionId plus one normalized argument tuple identifies one module instance.
Repeated applications of that pair project the same nominal member types,
InstanceFingerprint, sema result, TIR/codegen specialization, cache entry, and
LSP references. A different normalized argument tuple or resolved template
definition is a different instance.

`frame(B)` is u64-BE byte length plus `B`; `text(S)` frames exact UTF-8.
Resolved package identity frames `jet.package.v1`, canonical package name and
SemVer, plus locked workspace/registry/git/path source identity. Credentials,
absolute host paths, spans, aliases, display/Rust names, content/body/interface
hashes, map order, and compile order never enter identity. DefinitionFullKey
frames `jet.genmod.definition.v1`, package identity, defining workspace path,
lexical module path, kind `generic-module`, and template name; DefinitionId is
the 64-lowercase-hex SHA-256. Stores retain schema, id, and full key and treat an
id/full-key collision as ICE 101.

ParameterBytes records count, kind, name, resolved bound identity, and value
annotation TypeFullKey. ArgumentBytes records count and either the resolved
canonical sema TypeFullKey or the complete D-GENMOD-VALUE1 bytes.
ApplicationFullKey frames `jet.genmod.application.v1`, DefinitionFullKey,
ParameterBytes, and ArgumentBytes. Under applicative option A,
InstanceFullKey equals ApplicationFullKey and InstanceFingerprint is its
64-lowercase-hex SHA-256. Specialized nominal struct/enum/tag/trait keys frame
that identity basis, member kind/path/name, and member generic arguments; shape
never replaces nominal identity, and impls mint no type identity.

Resolution occurs before instantiation, so imports/re-exports do not mint
identity; locked package version/source differences do. Public TypeFingerprints
persist in semantic-index, interface, and cache artifacts with schema/full-key
verification. Sema checks each reachable instance once; TIR/codegen emit one
specialization and aliases are source-name projections. Cache keys additionally
include compiler ABI/schema, checked body semantic hash, target/profile/effects/
layout, and dependency interfaces. The semantic index records definitions,
applications, aliases, arguments, exported members, and their shared identity;
go-to-definition reaches alias and template application, and references join
all applicative aliases. AOT, JIT, dev, and comptime consume the same checked
instance law.

**U17 — Library packages**: consumed with ordinary `use <pkg>`; executables
go on PATH, never `use`. **D-PRELUDEX1**: `#NoPrelude` opts a file out of
ambient `print`/`input`; no library may inject into the no-prefix surface.
**Declined**: `namespace { }` keyword (D-NAMESPACE1).

### Traits, generics & derives

**S28 — Traits** *(D-IMPLDOT1)*: explicit named capabilities, never
structural. `trait Shape { fn area(self) => Float }`. Two equivalent impl
spellings — inside the type body (`impl Serialize { … }`) or top-level
**`impl Type.Trait { … }`** ("Type's Trait"). `.` walks namespaces and
attaches traits; `::` exists only inside `extern rust` path strings. Orphan
rule applies. v1: signatures plus D-LIB2's associated types and default
bodies.

**S48 — Dynamic dispatch**: a trait name in type position (`[Shape]`,
`fn f(s: Shape)`) means automatic boxing + dynamic dispatch; `<T: Shape>`
means monomorphization. No user-facing `dyn`.

**S62 — Delegation**: `impl Trait using field` — compiler-written forwarding
for one trait to one field; `impl App.Logger using logger` top-level form.
All-or-nothing in v1.

**S55 — Built-in derive policy** *(D-SERDE-CANON1 vocabulary; amended by
D-AUTODERIVE1=E, D-AUTODERIVE-SYNTAX1=D, and D-META-AUTO1=A, 2026-08-06,
cards #1267/#1545)*: every structurally derivable built-in capability
(`Printable`, `Equatable`, `Debug`, `Comparable`, `Encode`, and `Decode`)
auto-derives whenever every field qualifies. `Encode` + `Decode` is the
`Codable` family. The package default is on. `policy: .{ auto_derive: false }`
turns silent generation off for that package. At a type site, a bare trait
marker opts in and `!` refuses compiler generation: `#!Printable` or
`#[!Debug, !Equatable, Printable]`; `#!Codable` refuses both wire directions.
A missing trait follows the package default. Positive markers generate only
when fields qualify. Negative markers affect only compiler generation. A
hand-written implementation wins over the package default and either sign.
Traits that are not structurally derivable remain explicit-only.

`Debug` remains the dev-facing lens; `#Redact` carries the secrets story.
Explicit opt-in markers for the other derive families remain
`#Comparable` and the codability family `#Codable`
(≡ `#[Encode, Decode]`), `#Encode`, `#Decode` (D-SERDE4, D-MARKERMOVE3).
`Serialize`/`Deserialize` are not Jet words. Field-level wire markers stay on
the `#` plane (see Serde under Core library).

**D-DISPLAYDBG1 / D-DISPLAY-SHAPE — Display & Debug**: `Display` is
user-facing — a single explicit method `fn display(self) => String`, no
default (E0915, L0520); interpolation `{}` calls it. `Debug` is dev-facing and
auto-derived; `{value:Debug}` selects it; `#Redact` on a field renders
`"[redacted]"` (D-DEBUG-REDACT).

**D-ITER-HOOK / D-INDEX-HOOK — Extensibility hooks**: beginners use
`.each`/`.to_list()` and `.get`/`.set`; experts implement
`Iterable`+`Iterator` for `loop x; mytype` and `Index`/`IndexMut` for
bracket syntax. Built-in `[T]`/`Map` keep native paths.

**D-ROLLBACK-TRAIT**: `trait Rollback { type Snapshot; fn snapshot(self) =>
Snapshot; fn restore(&self, snap: Snapshot) }`; restore total; the
compiler-generated field-wise Rollback implementation is the canonical
implementation.

**D-EXT1 — Extensibility ceiling**: Tier 0/1 (methods, traits, operators on
own types via bundles) open to all; Tier 2 DSL blocks stdlib-only; Tier 3
proc macros and Tier 4 grammar/sigil changes rejected — even for experts.

### Markers & attributes

**D-VERDICT-732-1 — Marker plane reconciliation** *(ratified 2026-07-23,
card #732; supersedes the sigil assignment in D-SHAPE2 and
D-MARKER-FAMILY1; its former `@` reservation is amended by D-ONCE-AT1=D)*:
`#` is the sole prefix for attributes, instructions, and properties. Prefix
`@` is now the compile-time and fact-read mark; infix `@` still separates a
package target from its source. Thus
`#Rule` is the target syntax for applying a typed rule to the next
declaration, expression, or brace scope. Braces show extent; the rule name
states behavior; each rule declares its legal attachment targets. Authority-
bearing rules require a visible brace scope, reason, and audit treatment, and
`#Unsafe` remains the sole user-written unsafe gate. D-JPK-REF1 puts the thing
before `@` and its source after it. A leading `@Rule` in marker position produces E0063
with the canonical `#Rule` fix.

**`#` disambiguation** *(D-ONCE-HASH1=B, ratified 2026-08-07)*: `#` lexes to
one `TokKind::Hash` and carries three jobs, told apart by parser position
alone, never lookahead heuristics. At a declaration, statement, or expression
target it is the rule prefix (D-VERDICT-732-1): `#Rule`, `#[A, B]`. Inside a
bracketed type, `[T#N]`, it is the fixed-size-list separator (S76/D-FIXARR1).
After a package name, `pkg#1.2.0`, it is the version-pin separator (same
constant as the fixed-size form). Interpolation selectors do not use `#`; they
respell to colon (`{x:Fixed(2)}`) so they never collide with any of the three
jobs above.

**D-SHAPE2=A — One applied-rule marker** *(ratified 2026-07-14, card #534;
sigil assignment superseded by D-VERDICT-732-1)*: every typed rule uses the
single `#Rule` mechanism. Non-rule `#` constructs remain unchanged: effect sets
`#(FS)`, fixed lists
`[T#N]`, package selectors `pkg#1.2.3`, and the compile-time value `#Caller()`.
Later rulings assign `@name` to known-value splice, `@[ … ]@` to statement
expansion, and `T.@layout` to the compiler-known layout projection (D-ONCE-AT1=D
and D-FENCE-GLYPH1=A supersede the former `$` spellings).

**D-MARKERMOVE1/2/3 — Plane assignments** *(spelling reconciled by
D-VERDICT-732-1)*: on `#`: `MustUse`,
`Codable`, `Encode`, `Decode`,
`PublishedSchema`, `Redact`, `Numeric`, `Debug`, `Comparable`
(user derives of the same names also use `#`). D-SHAPE8 later moved explicit
purity to the empty function-effect row (`f: fn(Int) =[]=> Int`). Field-level wire markers
use `#`: `Rename`, `Skip`, `Default`, `Flatten`, `RenameAll`,
`DenyUnknownFields`, `Discriminant`, `Untagged`.

**S82 — One placement law** *(D-SHAPE2/D-ATTR2/D-VERDICT-732-1; rewritten by
D-MARK-FORM1=A, ratified 2026-08-05, card #1455)*: a marker, or one `#[A, B]`
bracket group, is written immediately before its target. The registry says
which targets each rule accepts. Parentheses appear exactly when arguments are
written; empty parentheses are an error the formatter fixes.

That one sentence covers every position. `#Codable struct …`, `@limit ::
32`, `#Inline fn hot(…)`, and `#Unsafe("reason") { … }` are the same shape —
marker, then target — differing only in what the target is. The five written
forms (`Bare`, `Call`, `BareOrCall`, `Block`, `Prefix`) are retired; they
described one shape and the registry held contradictory filings for it. The
row schema is name, signature, legal targets, repeatable, status, and
inheritance: the signature says whether arguments may and must be written, and
the target list says where the rule may sit. LSP surfaces applicable markers
per item.

Layout is the formatter's job, one rule: a marker on a type, function, or
method goes on its own line above it; a marker on a field, parameter, binding,
statement, or block stays inline.

**D-MARK-REPEAT1=A — One rule once per target** *(ratified 2026-08-05, card
#1455)*: writing the same rule twice on one target is an E0999 error with a
remove-the-repeat fix. Rows whose repetition carries meaning declare
`repeatable` in the registry: `#Pre` and `#Post` (several contracts, each with
its own message) and `#allow` (several lints). Every other row rejects the
second copy, because a silent duplicate is a typo or a merge artifact and two
copies can only disagree once one of them gains arguments.

**D-MARKSIG1=A — Marker signatures and one call grammar** *(ratified
2026-07-23, card #759; the `form` column retired by D-MARK-FORM1=A)*: one
declarative registry row owns each rule's name,
typed signature, legal sites, and active or retired status. Marker
arguments use the ordinary Jet call grammar: positional arguments first, then
named arguments. One E0930 family reports a mismatch and prints the declared
signature. Parser, sema, formatter, LSP, explain, and retirement handling read
the same row.

**D-MARK-STACK1=A — Bare single, one bracket list for several** *(ratified
2026-07-23, card #761; amends S82)*: one attached rule is `#Rule`; two or more
share one `#[A, B]` list. A one-item list or adjacent bare stack is E0999 with
the canonical rewrite. `jet fmt` performs that rewrite and preserves order.
Scoped block rules do not stack.

**D-INLINE-PARAM1=A — One inline rule** *(ratified 2026-07-23, card #762)*:
`#Inline` is the soft function hint and `#Inline(Always)` is the checked
promise. The retired `#InlineAlways` spelling fixes to `#Inline(Always)`.
E0917, E0918, and E0919 retain their checked semantics; E0920 is retired
because one rule cannot conflict with itself.

**D-CONSTMARK1=A — PascalCase constant rules** *(ratified 2026-07-23,
card #762)*: `#Static const` gives the constant a stable address; `#Inline
const` copies the value into use sites. Retired lowercase `#static` and
`#inline` fix to those registry-backed forms.

**D-CANVASSTATE1=D — Statement switch attributes**: `#Off <stmt>` parses and
type-checks the statement, then emits no code in every build. `#DebugOnly <stmt>`
parses and type-checks the statement in every build, emits in debug/dev builds,
and strips from release output. Both attach to statements only; item position is
E0342, expression position is E0343, and doubled switch attributes are E0344.
Names introduced inside the marker body do not escape. Bare `build.profile` is
not a user-typeable comptime value; the registered fact spelling is
`@build.profile`.

**D-CANVASMETA1=B — Canvas metadata attribute**: `#Meta(category: "Movement",
tunable)` attaches to bindings and functions; at top level it may also attach to
`const`/`comptime` bindings. `category` is a non-empty plain string literal and
`tunable` is a bare flag. Unknown fields are E0345 with did-you-mean help,
duplicate fields are E0346, wrong `category` value type is E0347, empty category
is E0348, and expression-position use is E0349. `#Meta` has no runtime
semantics and emits no code; it is checked source data for Canvas/tooling only.
New fields require a future ballot.

**D-ATTR4=A — Interpolation selectors** *(ratified 2026-07-23, card #732)*:
debug interpolation is `{value#Debug}`. The retired `{value@Debug}` spelling
teaches `{value#Debug}` because `@` belongs to the location/address/source plane.

**Current spelling amendment (D-ONCE-HASH1=B, ratified 2026-08-07):** the
selector is now `{value:Debug}`; the old `#` selector rail is a pure-rename
input for the D-ONCE-RETIRE1=C formatter/fix rewrite.

**D-FMT-INTERP1=A — Fixed-decimal interpolation selector** *(ratified
2026-07-29, card #1278)*: `{value#Fixed(n)}` formats a `Float` with `n` decimal
places through `core.fmt.decimal`. It uses the existing `#` selector rail.
Unknown selectors report E0914 with the valid set. No `#Percent` sibling is
defined.

**Current spelling amendment (D-ONCE-HASH1=B):** write `{value:Fixed(n)}`.

**D-FMT-INTERP2=A — Interpolation debug-label shorthand** *(ratified
2026-07-31, card #1351)*: trailing `=` in an interpolation hole prints the
expression's own source text, then ` = `, then the value. Example:
`print("{count=}")` → `count = 3`. The label cannot drift from the expression.
Composes with the current `:` selector rail: `{count=:Debug}`. Empty `{=}` is
an error.

**D-QUANTITY-PRINT1=A+D — Quantity and unit display** *(ratified 2026-07-29,
card #1268)*: `print` and bare interpolation show a quantity's magnitude and
declared unit symbol by default. Derived dimensions use a normalized product
or quotient, such as `4 meter/second`. An explicit `Display` implementation
replaces this default for its concrete unit type.

`{value:Unit(name)}` uses the generated unit type name.
`{value:Unit(bare)}` shows only the magnitude. The default symbol form uses
bare interpolation, so there is no second symbol selector. All three styles
use the same Display path. `.raw()` still extracts the unchanged numeric value.

**D-MARK-TARGET1=A — one target-marker family** *(ratified 2026-07-11, card
#498)*: `#Target(…)` is the only target-partition spelling, for every axis —
`#Target(Wasm)`, `#Target(JS)`, `#Target(Web)`, `#Target(OS.Linux)`. The
bare `#Wasm` and `#JS` markers are removed from the grammar (ordinary
unknown-marker errors, no teaching residue). `#WasmExport` is a different
job (export surface) and is untouched.

**D-BLOCKPLANE1=A — expert regions are `#` blocks** *(ratified by owner
2026-07-12, card #512)*: the three keyword regions join the marker
family. `#Region(r) { }` has D-REGION1 semantics; `#Live { }` has D-TERM1
semantics and no reason argument; `#Nondeterministic("reason") { }` has
D-DET1 semantics, now
reason-gated like `#Unsafe`/`#Impure`). The three keywords leave the
grammar as ordinary syntax errors. The rule is now universal: an expert
scoped region is a `#` block.

**D-POLICY-WORD1=A — one meaning for `policy`** *(ratified by owner
2026-07-12, card #512; amended by D-MARK-SCOPE1)*: source policy uses
`#Policy(…)`; future floors arrive as arguments, never new keywords. Package-
wide policy integrates with `package.jet`'s existing `policy:` namespace rather
than copying source-marker placement into the Package record. The bare `policy`
keyword leaves the grammar; the word means the Package governance namespace
alone.

**D-MARK-SCOPE1=A — common scope ladder for eligible settings** *(qualified
owner ratification 2026-07-15, card #657: “A but at the package level syntax
needs to be consistent and coherent within the package syntax”)*: eligible settings share block, function, module, and
package scope. The nearest declaration of a key wins, unmentioned keys inherit,
and `jet explain` reports the effective value plus every declaration it
overrode. A compiler-owned applicability matrix decides which levels each
setting may use and whether it may tighten, override, or merge. Site-specific
proof and authority stay site-bound: `#Unsafe` authorization, `#Grant`,
direct fact tags, `#Scrub(Tag)`, and field wire attributes do not widen through this
ladder. At package scope, each setting uses the coherent Package `policy:`
surface owned by its policy decision; the common ladder does not mint a second
manifest spelling.

The compiler registry is also the source of semantic-index/explain provenance:
it returns one effective value and the complete outer-to-inner declaration
chain. The shared memory fields are `no_alloc: true`, `zero_rc: true`,
`arena_bounded: <positive bytes>`, and `gc: true`. Site-bound
`#Unsafe`, `#Grant`, direct fact tags, `#Scrub(Tag)`, wire, and authority rows have
explicit applicability but never inherit.
The concrete terminal view uses the ratified existing route:
`jet explain marker Source/sensor.jet:9 arena_bounded`.

**D-PACKAGE-POLICY-SCOPE1=A — package `policy:` holds a typed field value**
*(ratified 2026-07-16, card #657)*: the package-echelon `policy:` field
settled by D-POLICY-WORD1 holds a typed `.{ ... }` value whose governance
keys are ordinary fields, written like every other Package role field, with
bare top-level `name:`/`version:` identity — not the `#Policy(...)` marker
call. Each package-field key maps to the identical key the source-scope
`#Policy(...)` marker uses, so `no_alloc: true` in `policy:` and
`#Policy(no_alloc)` on a block/function/module are the same key in two
echelon-appropriate spellings, and `jet explain` unifies provenance across
the whole D-MARK-SCOPE1 ladder. Package policy may only tighten safety — it
can forbid unsafe code but can never authorize an unsafe operation; that
still requires a written `#Unsafe("reason")` block or function. Reuse across
a monorepo is reached through the ratified `jet split` (D-ECO-SPLITPOLICY1),
which extracts a shared `policy:` value into a named `Config` when needed;
`policy:` itself does not compose a `use:` list of named profiles.

```jet
# package.jet — policy: reads beside bare Package identity
name:     "meter"
version:  "1.0.0"
policy:   .{
    no_alloc: true
    zero_rc: true
    arena_bounded: 65536
    unsafe: .Forbid
}

# Source/sensor.jet — a module tightens one key with the source marker
#Policy(arena_bounded(2048))
module sensor

# Package policy may only tighten safety, never authorize it:
policy: .{ unsafe: .Allow }
// error: package policy cannot authorize unsafe — write #Unsafe("reason")
// at the exact block or function instead
```

**D-MEM-FACTS1=B — transitive memory facts** *(ratified 2026-07-15, card
#644)*: `no_alloc`, `zero_rc`, and `arena_bounded(N)` are explicit memory facts
on the D-MARK-SCOPE1 ladder. Each fact checks every reachable call, including
dependencies, and a violation reports its source, full call path, effective
declaration, and declaration provenance. Open-world dispatch cannot prove a
strict fact: the program must seal the target set or consume a signed dependency
summary, otherwise the compiler rejects the unprovable contract. This
supersedes D-NOALLOC-SEM1=A's local-only denylist scope.

**D-DROP-WORD1=A — one meaning for `drop`** *(ratified by owner
2026-07-12, card #512)*: the linear finisher for `#SingleUse` values
uses `consume(x)` (still `#Unsafe`-gated, D-LIN1 semantics unchanged).
`.drop("reason")` keeps sole ownership of the
discard meaning.

**D-DOTSCOPE1 — Scope members**: inside an applied-rule block body, a
statement-position `.name { … }` / `.name(args) { … }` resolves against that
marker's declared scope members (`#Test`: `.expect_fail`, `.setup`,
`.timeout`, `.skip`); this is the ONLY spelling for scope vocabulary (I8 —
no nested per-scope markers, no block-valued args for the same job). Unknown
member is a teaching error listing the scope's vocabulary. Typing `.` in
statement position inside a marker block completes members. Disambiguation:
the required trailing block separates it from leading-dot enum values
(D-ENUMDOT1); the identifier after the dot separates it from `.{ }`
construction and S74 destructuring. Other block markers may declare members
under the same law — each addition is an API decision, not a syntax one.

**D-PROVENANCE1=B — Binding-level provenance tracking**: `#Track` may prefix
a sigil binding:

```jet
#Track speed :: compute_speed()
#Track correction: Float := 0.0
```

The marker records provenance for that binding without changing its type.
Current implementation records Float local origins; `speed.origin() => String`
returns the tracked source note, and untracked Floats return `"untracked"`.
No `Tracked<T>` wrapper exists and no general value-history type is introduced.

**D-QUAL2 — Tag vs trait**: exactly two qualifier kinds — `trait` (has
methods and dispatches) and `tag` (declares an erased dataflow fact and its
policy). A tag requires a nonempty `deny` list and may declare `from`. Methods
on a tag are E0732; tag where dispatch is expected is E0731. **D-QUAL4**:
type-position value tags use a direct prefix, such as `#PII String`.

**D-MATURITY1 (superseded by D-MARK-META1=B, 2026-07-12)**: the maturity
trio `#Experimental`/`#Tested`/`#Hardened` leaves the grammar — maturity
is a `#Meta` field: `#Meta(maturity: .Experimental | .Tested |
.Hardened)`. Same semantics (doc-only, parsed onto `Func.maturity`,
formatter-preserved, zero sema/codegen effect). Retired `@` spellings get
ordinary unknown-marker errors (greenfield, no teaching residue; the old
E0062 row retires with them).

**D-MARK-META1=B — doc-metadata growth law + trio fold** *(ratified by
owner 2026-07-12, card #509)*: every tool-facing, behavior-free
annotation is a `#Meta(…)` field — ballot-ed as a field, never a new
marker (extends D-CANVASMETA1's field-ballot hook). Applied immediately
to the shipped maturity trio (above). `#Doc` stays: it is the CLI
help-text carrier (D-CLIFLAG1), not free metadata.

**D-PATCH1 — Typed patches** *(ratified 2026-07-03, card #181)*: `#Patchable`
on a struct `T` synthesizes `T.Patch` — every field wrapped `T?` (Option),
absent field = no change. Generated methods: `t.apply(patch) => T` (apply
onto a base), `T.diff(new, old) => T.Patch` (static; fields that changed,
`None` where equal), `patch.merge(other) => T.Patch` (`other` wins on
conflicting `Some`s). No type parameters (E0336 — concrete field list only);
no stored-reference or function-typed fields (E0337 — a patch holds owned
optional values). `Patch` Encode/Decode is deferred to the prelude serde
path, not yet generated.

### Capabilities & memory

**S10 / D-CAP7 — Capability sigils, memory v5** *(owner-frozen; migration to
D-MEM1 complete 2026-07-04)*: two sigils ship; unmarked is always read,
enforced (no elevation, no inference tier):

```jet
T     // read: default, enforced — never elevates
&T    // write: exclusive write access
^T    // take: ownership moved/consumed
```

```jet
fn write(file: &File, data: Bytes)     // read is the default → no sigil
fn equip(player: &Player, item: ^Item)
```

Call sites mirror the type — `write(&file, data)`, `close(^file)`; receivers
carry it on self (`fn write(&self)`, `fn destroy(^self)`, bare `self` =
read). Capability sits on the type side (`name: &Type`, D-CAP3). `copy x`
stays a verb — no third sigil (D-CAP2). Dereference is **postfix `p.*`**
(composes: `p.*.field`); prefix `*x` is raw-pointer-of only, `#Unsafe`-gated
(D-CAP9), and is not a parameter capability. `mut`/`take`/`view` are not
keywords (E0056/E0057 retired by D-S14-PAUSE; E0058 retired earlier by
D-MEM1/S3).

*Supersession (D-SHAPE-COPY1=A, ratified 2026-07-15):* `copy x` is
historical text in this frozen entry, not current syntax. The current copy
form is `~x`; the retired `copy` word produces E0991.

*History:* D-CAP7's original text (pre-2026-07-03) had a third visible
parameter sigil `~T` (edit/mutate), a fourth `*T` in parameter position, and
an `Infer` tier where a bare `T` param elevated by body usage. D-MEM1
superseded all of that: unmarked is always read (no elevation, ever), `~` is
gone from the grammar entirely (ordinary syntax error — no compat, per the
rule at the top of this file), and `*T` never shipped further than this doc
as a *parameter* sigil (raw-pointer access stays the separate `p.*`/`*x`
expression mechanism, D-CAP9, untouched by the migration).
The dead internal `AccessConvention::Share`/`::Raw` placeholders were removed;
a future ratified tier can add the exact representation it needs.

**D-CAP8 — Unmarked default (retired 2026-07-04 by D-MEM1/S2)**: originally,
an unmarked param elevated by body usage and froze its resolved capability
at a `library { api: explicit }` boundary (drift = E0912, see D-CAP4/5/6).
D-MEM1 deleted elevation and the freeze tier outright: unmarked is always
read, no inference, no `api:` manifest field (an ordinary unknown-key error,
E1216) — see D-MEM1 below.

**D-MEM1 / D-MEM-PARAM1=A — Memory model v5, "the borrow checker,
humanized"** *(ratified 2026-07-03; unmarked-read law reconfirmed 2026-07-15,
card #642)*: supersedes the D-CAP7
spelling assignments and D-CAP8 when the migration lands. Three sigils:
unmarked = read (enforced — no elevation, no freeze; no `api:` manifest field),
`&T` = exclusive write, `^T` = take; `&`/`^` mirrored at call sites;
`&self`/`^self` receivers. Passing an unmarked parameter is allocation-free and
never elevates; a body write requires `&`, while consuming ownership requires
`^`. `~` has only the copy meaning assigned by D-SHAPE-COPY1. Raw `&T`
reference returns and fields remain deleted (D-REF-SHORTHAND1/2 and
E0207/E0427); safe stored and returned views use D-MEM-VIEWRET1's named
`View<T>`/`ViewMut<T>` boundary instead. L0201
deleted — moves of named bindings are always written `^`; temporaries pass
freely; `~x` (D-SHAPE-COPY1) is the one copy spelling. Named escape hatches
`Shared<T>`, `Pool<T>`/`Id<T>`; scoped memory-policy facts (`no_alloc` first).
**S1 shipped (2026-07-04)**: `&` is the write sigil, `~` is gone from the
grammar, call sites/receivers/formatter speak v5 spelling. **S2 shipped
(2026-07-04)**: unmarked param is `Read`, decided at parse time — `Infer` and
body-usage elevation are gone; a body write or an escape/consume of an
unmarked param is a hard error (fix-it: add `&`, or `^`/copy it); L0201 is
gone (E0209 hard error, no silent clone ever); `CapabilityFreeze`/E0912 are
gone and the `api:` manifest field no longer exists (ordinary unknown-field
error) — `ApiFreeze`'s snapshot mechanism remains, now unconditional pub-fn
semver diffing (E1218/E2601), not a capability-tier freeze. **Card #642
reconciliation shipped (2026-07-16)**: concrete, generic, callback, and
receiver parameters now share the same unmarked-read lowering; no sema or
codegen path may upgrade them to move or insert a hidden copy. Direct `&`
calls write back identically in dev, TIR, and native builds. **S3 shipped
(2026-07-04)**: `-> &T` returns and `&T` fields are gone from the grammar
(ordinary syntax errors); `#Ref`/E0207/E0427 deleted outright. **S4 shipped
(2026-07-04)**: `copy x` (D-CAP2) is the one copy verb — a real prefix-verb
expression (`Expr::Copy`), parses on any expression, formatter round-trips
it. `.clone()` is not user-typable Jet syntax — `clone` falls through to the
ordinary "no such method" path (I8). `copy x` on a non-cloneable type is
E0211; on a scalar it's legal but redundant (already `Copy`). Every fix-it
that used to suggest `.clone()` now suggests `copy name`.
*Supersession:* D-SHAPE-COPY1=A replaces this S4 spelling with `~name`; the
retired word produces E0991. **S5 shipped
(2026-07-04)**: `[T]` slice views were already live (`View<T>`, D-DYNARRAY1,
predates this migration — nothing to build). `String.trim()`/`.after(sep)`/
`.before(sep)` bound to a local return a zero-copy string view instead of an
owned `String`. Local use remains codegen-invisible; crossing a return or field
boundary uses D-MEM-VIEWRET1's named, provenance-carrying `View<str>` contract.
`split` later joined the lazy `Iter` model under D-ITERTOOLS1=A (was eager
`Vec<String>` through S5). **S6 shipped (2026-07-04), amended by
D-CONC-SHARE1=A**: `Shared<T>` (D-SHARED-API1=A) is a lock-guarded shared
value — `shared expr` constructs the cell; plain field reads and writes take
one statement step; several steps commit under `#Transact`; expert guards and
`Condition` remain. Cloning is always a cheap handle clone (never a deep copy),
so it crosses a task boundary with no `take`. (`Shared<T>`'s *type* predates
this stage as inert signature
plumbing over a plain `Arc<T>` — never actually constructible, see
`tests/ownership.rs`'s pre-existing param-signature tests; S6 gives it a real
constructor and upgrades the representation to `Arc<RwLock<T>>`.) `Pool<T>`/
`Id<T>` (D-POOLID-API1=A) is a generational arena: `Pool<T>.new()` constructs;
`.add(val)` inserts and returns an `Id<T>` (plain, `Copy`, comparable —
index+generation, never touches `T`); `pool[id]` indexes for read AND write
(including a nested `pool[id].field = v`, a genuine mutable place, not a
value round-trip); `.ids()` snapshots every live id; `.remove(id)` removes,
bumping the slot's generation, returning `T?` (mirrors `Map.remove`'s
`Option` convention). A stale `Id<T>` (removed/reused slot) panics at
runtime, mirroring the array-out-of-bounds precedent — not a new diagnostic
code. **S7 shipped (2026-07-04, D-NOALLOC-SEM1=A; superseded by
D-MEM-FACTS1=B)**: the original module-local allocation denylist shipped as
E0921. Current law follows reachable calls at every eligible scope and checks
the transitive `no_alloc`, `zero_rc`, and `arena_bounded(N)` facts above.
**S8 shipped (2026-07-04)**: docs sweep —
diagnostics.md retired-code stubs for every deleted S1-S7 mechanism,
spec.md's memory chapter rewritten to v5 end to end, this file's
D-CAP7/D-CAP8/D-CAP4-5-6/D-REF-SHORTHAND1/2 supersession notes, stale
`~`/`.clone()`/`api:` sweep across docs/reference. S9 (final verification
gate) remains.

**D-SHAPE-COPY1=A — the one copy sigil, `~x`** *(ratified 2026-07-15,
card #535)*: supersedes D-CAP2/S4's `copy x` word. `~x` is a prefix-verb
expression producing an independent duplicate, legal in any position;
chained on a method call it needs parentheses (`(~input).rotate()`); on a
non-cloneable type it is still E0211. The `copy` keyword is retired to a
teaching error, E0991, pointing at `~`, mirroring how D-MEM1/S10 retired
`mut`/`take` to E0056/E0057. This also reopens D-MEM1's "`~` is not part of
the v5 grammar" decree for the copy sigil specifically — `~` still has no
role as a parameter-position capability (that stays `&`/`^` only).
D-SHAPE-LIFECYCLE's `^^` (never implemented — zero code hits) is superseded
and retired outright, never shipping.

**D-SHAPE-PLACE1=A — one rule for reading, editing, and copying a place**
*(ratified 2026-07-15, card #613)*: a place is a name plus its maximal field,
index, or range projection. Bare place access creates a checked read window,
`&place` creates the exclusive write window, and `~place` creates independent
owned storage. Many read windows may overlap; a write window must be exclusive;
moving or resizing an owner is rejected while it could invalidate a live
window. Method calls are not part of a place, so calling a method on a copy
still needs `(~input).method()`. This supersedes D-SHAPE-VIEW1's `.view()`
spelling and dissolves the separate D-SHAPE-VIEWMUT1 question.

**D-MEM-VIEWRET1=B — stored and returned safe views** *(ratified 2026-07-15,
card #643)*: `View<T>` and `ViewMut<T>` may cross a return or field boundary.
This explicitly supersedes D-MEM1/S3's blanket ban on returned and stored
borrows; it does not revive raw `&T` returns or fields.
Each carries public, queryable source provenance in the API snapshot; changing
that provenance is a breaking API change under E2601. Sema proves the owner
outlives every view and keeps at most one mutable view live. Ordinary in-body
place access remains D-SHAPE-PLACE1's bare/`&`/`~` rule; the named view types
appear only where the return or storage boundary needs to state the contract.
No lifetime syntax is added.

**D-MEMPROVENANCE3=A — expert declared view provenance** *(ratified 2026-07-31,
card #1340)*: opt-in trailing `from` after a return type names which inputs a
view may borrow from (`from packet`, `from left | right`, `from self`,
`from static.MOTD`, or per-slot `from (text: primary | fallback)`).
`VIEW_FROM` / `VIEW_FROM_STATIC` are contextual identifiers, not lexer
keywords. Function types may name parameters so a clause can resolve them
(`fn(line: String, noise: String) => View<str> from line`); names exist only
for that resolution and are not part of type identity. Inference
(D-MEMPROVENANCE2=A) remains the beginner default; undeclared APIs behave as
today. Sema requires every return path's inferred owners ⊆ the declaration (a
bare source covers its field/index/range projections) and publishes the
declared set to callers. Parameter-requirement clauses (`View<T> from corpus`)
and aggregate per-slot `from (text: primary | fallback)` ship in the same ruling.

**D-MUTSELF1 — Receiver mutation**: a `&self` method mutates in place —
`self.field = v`, compound ops, and whole-`self` reassignment all lower
through the deref'd receiver; the same write in a read method is E0205 with a
"write the receiver as `&self`" fix at the assignment.

**S63 / D-SHAPE-RESOURCE1=A / D-SHAPE-RESOURCE2=A — resource cleanup**
*(amended 2026-07-15, cards #557/#647; supersedes D-DEFERKW1/D-SUGAR5's
no-`defer` ruling)*: automatic scope-end cleanup remains
the safety net on success, error, panic, and cancellation; a small block still
ends ownership early. `close(^resource)` is the one consuming close operation
for files, locks, connections, and other resources. `defer close(^resource)`
schedules it beside acquisition and runs deferred closes in reverse order on
every scope exit. An immediate close consumes at the call; a deferred close
consumes when its scheduled action runs. `.drop("reason")` remains deliberate
value discard, while protocol
success such as `finish`, `commit`, `flush`, and `shutdown` remains an ordinary
fallible method rather than pretending automatic cleanup succeeded.
There is no general deferred statement or block: `defer action()` and
`defer { ... }` are E0003. The resource expression is one directly moved local;
ordinary close-call resolution proves that `close(^resource)` is valid, and the
normal move checker rejects any later use, immediate close, or second defer.

**S53 — Concurrency** *(historical record, superseded by D-CONC-SPAWN1=D)*:
tasks/channels were once planned around `tasks.spawn` and handle-list
combinators. The current surface is one `task` word with nested
`task.all`/`task.race`/`task.any`/`task.group`; old spellings have no alias.
**D-CONC-FAIL1=A** makes `join()` fallible with `TaskFailure`.
**D-DETACH1** `task.detach()` consumes the handle, detached capture of a
borrowed view is a compile error; **D-ASYNCRT1** M:N green threads, no
`async`/`await` coloring (gated on scheduler work). **D-TUPLE-DESTRUCT1**
*(ratified/implemented 2026-07-04)*: `tasks.channel<T>()` returns
`(Sender<T>, Receiver<T>)` directly — no combined "Channel" handle, no
`.sender()` method. Destructure with the existing S74 tuple form:
`(tx, rx) := tasks.channel<T>()`; a second sender is `~tx`. A
`Receiver<T>` is what `g.select().recv(rx)` takes.

**D-CONC-UNIT1=A — one substrate for state, duty, and reach** *(ratified
2026-08-06, card #1505)*: task lifecycles are typestate rows; the join duty
uses the same single-use obligation as every other single-use value; and the
five separate crossing checks become one registered fact plane. Every
concurrency error uses one vocabulary. The facts erase before codegen, so this
decision changes no user-facing surface and amends no prior semantics.

**D-CONC-JOIN1=A — dropping a bound task handle is an error** *(ratified
2026-08-06, card #1505)*: a bound `Task<T>` must be joined, used through its
result, or detached. Dropping it without discharge is a compile error, not a
warning or an implicit join. The rule extends D-LIN1 to task handles and the
task-list forms from D-VERDICT-1323-1, and amends L1101. Unbound spawns still
join at scope end under D-CONC-SPAWN1.

**D-CONC-GROUP1=A — group borrows work in any parameter position** *(ratified
2026-08-06, card #1505)*: a group may be a direct parameter of a free function
or method. A group may not be stored, returned, captured, or placed in a field.
The storage ban and E1110 rationale remain. This amends
D-TASKGROUP-PARAM1=A only at its parameter-position boundary.

**D-CONC-OUTCOME1=A — typed outcome and status** *(ratified 2026-08-06,
card #1505; retired by D-CONC-FAIL1=A)*: the ballot first ratified consuming
`outcome()` with `TaskOutcome` and borrowing `status()` with `TaskStatus`,
covering `Finished`, `Panicked`, `Cancelled`, and `DeadlineBlown`; `status()`
covered running, paused, and cancel-requested states. The later
D-CONC-FAIL1=A ruling retires that separate surface. `TaskOutcome`,
`TaskStatus`, `trace()`, and `exception()` do not ship. It amends
D-COROUTINE1's handle surface; D-CONC-FAIL1=A is the later retirement ruling.

**D-CONC-CROSS1=A — one crossing plane and one error family** *(ratified
2026-08-06, card #1505)*: sendability is a registered fact about a type. One
prover handles task captures, parallel adapters, kernels, cells, and fixed
backings. Each rejection names what cannot cross, the boundary, and the fix.
E1111 and the kernel wording join the E1101/E1102 family. The semantics
ratified by D-PARCAPTURE1 and the kernel decisions stay unchanged; this
decision amends their prover and diagnostic implementation only.

**D-CONC-STM1=A — one-run, ordered transaction commit** *(ratified
2026-08-06, card #1505; amends D-STM1)*: a transaction body runs exactly once.
Participants acquire write locks in stable order, buffered changes apply, and
locks release. Contention waits instead of retrying. Log lines and other body
effects do not run twice. Irreversible-effect checks remain unchanged.

**D-CONC-SCHED1=A — typed schedule data and one job word** *(ratified
2026-08-06, card #1505)*: the value behind the unchanged schedule marker is
typed `Duration` or wall-clock time on the D-TYPE2-TIME1 rail. `jet dev`,
services, and jetos timers read that value. A job is a task the runtime starts.
The service plane uses supervisor tasks, groups, and restart data. This amends
the value grammar of D-SCHEDULE1=A, aligns D-TAG-SURFACE1 and D-TASK-META1,
and does not change schedule placement rules.

**D-CONC-STREAM1=A — a stream is a task** *(ratified 2026-08-06, card #1505;
amends D-STREAMYIELD1)*: dropping a stream iterator cancels its producer. The
producer unwinds at its next wait point and runs its cleanups. `yield` is a
wait point and drop-close is cancellation. The generator delegates to
D-CANCELMODEL1=C and subsumes the separate generator-lifecycle work; it has no
separate shutdown law.

**D-CONC-CHAN1=A — builtin channels and arm-table draining** *(ratified
2026-08-06, card #1505; its wait spelling is amended by D-CONC-CHAN2=D)*:
`channel<T>()` is builtin and needs no import. `loop value, receiver` drains
until close. `Receiver<T>` and `Sender<T>` are nameable in signatures. The
wait works on plain endpoints in any task. The fluent builder, dead `Channel`
entry, and silently dropped `.read` arm are deleted. `after` takes a duration
on the D-TYPE2-TIME1 rail. This amends D-CONCSELECT1=A and narrows
D-TASKRUNTIME1's module surface; cancellation remains D-CANCELMODEL1=C.

**D-CONC-SHARE1=A — shared values use plain access** *(ratified 2026-08-06,
card #1505; amends D-SHARED-API1 and D-TXN2)*: `shared expr` builds the cell;
field reads and writes use ordinary access; each statement is one atomic step;
and several steps commit under `#Transact`. The `read` and `edit` closure
forms are deleted. Expert guards and `Condition` remain. Lock order and
crossing safety stay checked. It amends D-SHARED-API1=A, D-TXN2, and uses the
ordered commit law of D-CONC-STM1=A.

**D-CONC-SPAWN1=D — one nested task keyword family** *(ratified 2026-08-06,
card #1505)*: the surface is `task f()`, `task.all { … }`, `task.race { … }`,
`task.any { … }`, and `task.group g(limit: n) { … }`. Only `task` is reserved;
`all`, `race`, `any`, and `group` remain free identifiers. Scope-end joins,
fail-fast, cancel-losers, first-`Ok`, and same-completion source-order
tie-break laws stay. It respells the surfaces of D-TASKSCOPE1, D-NURSERY1,
D-CONCCOMB1, and D-TASKGROUP-PARAM1 without changing their lifetime laws. The
parser recognizes a spawn only at the complete `task` surface shape; otherwise
a local named `task` keeps ordinary identifier syntax. In particular, `task`
followed by `(`, `[`, `.`, or an operator is a local call, index, member, or
ordinary expression; the bare-spawn operand form starts with an atom or control
expression instead. A parenthesized or list operand can use `task { … }` or a
named function when a local named `task` must remain visible. This is the final
spelling for the spawn surface.

**D-CONC-FAIL1=A — task failure uses the one `?` rail** *(ratified
2026-08-06, card #1505; amends D-COROUTINE1 and retires D-CONC-OUTCOME1)*:
`join()` returns `T ? TaskFailure`. `TaskFailure` is a normal enum with
`.Cancelled`, `.DeadlineBlown`, and `.Panicked(reason)`. `task.all` returns
its tuple on the same rail. A joined child panic becomes `.Panicked(reason)`;
it does not kill the process. `trace()`, `exception()`, `TaskOutcome`, and
`TaskStatus` are deleted. It amends D-COROUTINE1 and retires
D-CONC-OUTCOME1=A; conversion uses D-FAIL-CONV1.

**D-CONC-CHAN2=D — the readiness wait rides subjectless `if`** *(ratified
2026-08-07, card #1505; amends D-IFGUARD1, D-CONCSELECT1, and the wait
spelling in D-CONC-CHAN1)*: a subjectless `if` table whose every arm head is a
binding/source pair is one atomic readiness wait. `after` takes a Time delta;
an optional `else` arm makes the wait non-blocking; mixed readiness and Bool
heads are a registered diagnostic; a closed source stops matching. Expression
form follows the `if` value-table law. `select` stays a free identifier
everywhere. It amends D-IFGUARD1, D-CONCSELECT1, and D-CONC-CHAN1's spelling;
`after` uses D-TYPE2-TIME1=A and cancellation follows D-CANCELMODEL1=C.

**D-CANCELMODEL1 = C** *(ratified 2026-07-11, card #126)*: cancellation is
preemptive at wait points. A cancelled task (race loser, fail-fast sibling,
explicit `handle.cancel()`) unwinds at its next wait point — channel
recv/send, sleep, join, select, I/O — running Drop-backed cleanup, exactly as
a blown deadline (E3003) already does; a cancelled `task.all` member reports
`Cancelled`, not a completed `Value`. A scoped shielded region defers (never
discards) the unwind until a critical section finishes. **D-SHIELDNAME1 = A**
*(ratified 2026-07-11)*: the shielded region is spelled `#Shield { … }`,
joining the `#Unsafe` / `#Context` sigil family. A cancellation (or blown
deadline) pending against a task inside `#Shield` lands the moment the block
exits — deadline first, then cancel.

### Effects & safety

**D-EFF1 — Effect system**: inferred per-fn effect sets (Koka-style rows),
erased in codegen. Assert or restrict via `=[Net, DB]=>` on a signature and
`#Caps(Net) { … }` regions.

**D-SHAPE8=A — Effects inside the arrow** *(ratified 2026-07-14,
owner-amended by D-ARROW-CONTROL1 on 2026-07-26; card #543)*: every explicit
function effect row uses exactly `=[Effects]=>` in declarations, trait
methods, function values, and callback types. `=[]=>` explicitly bounds the
row empty. Open rows stay inside the brackets (`=[Log, ..E]=>`). The older
`--[Effects]->`, `-[Effects]->`, `#(Effects)`, `#(via f)`, and `#Pure fn`
spellings are retired; no alias exists.

**S60 — Purity marking** *(surface superseded by D-SHAPE8=A)*: an explicit
empty effect row `=[]=>` is the checked purity signature; violations name the
impure call path. The same empty row works in function-type bounds.

**D-EFF4 / D-EFF5 — Vocabulary**: closed set of ten tree ROOTS — `Net`, `FS`,
`IO`, `DB`, `Time`, `Rand`, `Env`, `Exec`, `Log`, `GPU`; unknown root E0119.
Amended by D-EFFTREE1: a root may be dotted into an open leaf path (`FS.Read`)
and ancestor matching is subsumption. `effect <Name>` user declarations
reserved, unminted.

**D-EFFTREE1 — Effect tree** *(ratified 2026-07-03, card #181)*: the ten
D-EFF4/5 names are tree roots; a signature/`#Caps`/`#Grant`/`=[!…]=>` entry may
be a dotted path rooted at one (`FS.Read`, `Net.HTTP.Get`) — root closed
(E0119), leaf open/user-chosen, no fixed vocabulary or depth limit. Ancestor
matching is subsumption, the same rule as D-TAG1's tag-tree subtree matching
learned once and reused: `=[FS]=>` accepts any `FS.*` callee; `=[FS.Read]=>`
rejects a sibling `FS.Write` callee; `#Grant(FS.Read)` doesn't authorize
`FS.Write`; `=[!FS]=>` prohibits the whole `FS.*` subtree. Reverses E0740 for
the ancestor case, keeps it for out-of-tree/sibling cases. Flat root names
stay valid (no migration break) — Core stdlib calls are still tagged with a
bare root; leaf precision is a user-declared-contract concept.

**D-EFFECT-DECL1=A — Effect-leaf declarations** *(ratified 2026-07-28,
card #1299)*: `effect FS.Read` is a compile-time package declaration. A package
view contains its own declarations, declarations from loaded dependencies, and
Prelude leaves. Once a root has one declared leaf, every dotted use under that
root in effect rows, `#Caps`, `#Grant`, and package budgets must name a declared
leaf exactly; E0750 suggests the nearest declaration. Bare roots stay valid.
A root with no declared leaves stays open for gradual adoption. Declarations
erase before TIR.

**D-EFF2 — Polymorphism**: transparent flow-through by default; escaping
function values assume the maximal set. Expert levers: effect-bound function
types (`fn(T) =[]=> U`, `fn(T) =[Net]=> U`; call-site check E0747) and
`=[via f]=>` pass-through publication (E0748).

**D-EFF3 — Traits**: a trait method may declare an effect upper bound — both
the impl obligation (E0742) and the dispatch contract for trait objects.

**D-EFFECT-OMIT1=A — inferred effects may stay unwritten** *(ratified
2026-07-16; implemented 2026-07-17, card #570)*: private and public ordinary
functions may omit an effect bound. `=>` defines the result and never asserts
purity. A function is pure when its inferred row is empty, or when `=[]=>`
bounds it empty. Public API snapshots store the inferred normalized
row and provenance, and semver rejects row changes; an explicit row is
always available as an upper bound (`=[FS.Read]=>`). D-EFF3 is unchanged:
static calls use each implementation's inferred row, while a trait method
used through dynamic dispatch keeps its declared upper-bound contract.

```jet
fn twice(n: Int) => Int = n * 2
// inferred []: pure

pub fn load(path: String) => String { core.files.read(path)? }
// API snapshot: load =[FS.Read]=> String

pub fn bounded(path: String) =[FS.Read]=> String { core.files.read(path)? }
fn hash(text: String) =[]=> Int { text.length() }

trait Renderer { fn render(self) =[GPU]=> Image }
```

**D-PROP1 / D-PROP2 — Prohibition**: `=[!Net]=>` — the fn and every reachable
callee must not use the effect (E0749).

**D-SCAP1 — Scoped capabilities** *(amended by D-ARROW-CONTROL1)*:
`#Grant(caps: FS) { … }` authorizes effects in a lexical scope and binds an
erased first-class handle in the marker head. An effect
without backing grant E0712; handle escape E0711.

**D-TAINT1 — Fact-tag dataflow** *(amended by D-TAG-SURFACE1=A)*:
`#Input expr` attaches the Prelude `Input` tag. Direct tags spread through
dataflow. A value reaching a destination named by its tag declaration is E0721.
`#Scrub(Tag)` removes exactly the named tag when its signature consumes that
tag and returns an untagged value. Bare `sanitizer` remains E0059 with this fix.

**D-TAINT2=A — Credential taint** *(ratified 2026-07-13)*:
`#Credential T` attaches the Prelude credential tag through the shared fact
lattice. Credential facts spread by dataflow and may not reach `print`, `log`,
or serialization sinks (E0722); `#Scrub(Credential)` is the exact audited strip
point. Other tags follow their declared `deny` and optional `from` policy.

**D-DET1 — Determinism** *(D-DET-CAPAPI)*: an explicit `=[]=>` bound implies reproducible —
wall-clock/OS-rng/fs/net rejected (E3401/E3403); injectable `Clock`
(`now/tick/advance/wait`) and `Rng` (`int/float/bool/pick/shuffle`) are the
pure-callable capabilities; `#Nondeterministic("reason") { }` expert escape (respelled by D-BLOCKPLANE1, 2026-07-12).

**D-REPLAY1**: `#Replayable` rejects any reachable `Time`/`Rand`/`Net`/`IO`
not routed through a deterministic/mockable capability. Implemented by the
effect fixpoint as E0725; deterministic `Clock`/`Rng` handles remain pure.

**D-TXN1–4, D-TXN-ROLLBACK — Transactions**: `#Transact(name) { … }` — on a
`?`-failure, mutated locals restore LIFO from auto-snapshots (layer 1);
`Rollback` trait for custom snapshots (layer 2); `name.on_rollback(() => …)`
and `name.on_commit(() => …)` explicit hooks (layer 3, Drop-backed).
Irreversible effects (`Net`/`FS`/`Exec`) inside the block are E0746 — move
after the block or register via `on_commit`.

**D-BOUND-UNDO1=A — Foreign undo contracts** *(ratified; amends the
D-TXN2 irreversible set and rides D-ROLLBACK-TRAIT)*: an FFI call is
irreversible inside `#Transact` because the compiler cannot inspect the
foreign body. Such a call is E0746 unless its binding declares
`#Undo(inverse)`. The declared inverse is registered on the same rollback
path as `on_rollback` hooks, so multiple inverses run in reverse call order.
Hoisting the foreign call out of the transaction remains the beginner fix.

**D-LIN1 — Single-use values** *(D-LIN1-DROP)*: `#SingleUse` values must be
consumed exactly once on every path — `^` param, return,
or `consume(x)` inside `#Unsafe("reason")` (respelled by D-DROP-WORD1, 2026-07-12; else E0143). Unconsumed E0140;
one-branch-only E0141; lending instead E0142.

**D-PREPOST1 — Contracts** *(amended by D-FAIL-TIER1)*:
`#Pre(cond, "msg")` / `#Post(cond, "msg")` on a signature (`result` in Post);
conditions pure; checked in every build; per-module build-policy strip is an
explicit opt-out. A proof of the condition is a second, proof-keyed erasure
disposition. A `#Pre` violation quotes the clause at the call site; a `#Post`
violation points at the body.

**D-METHODMACRO1=A / D-INLINE-PARAM1=A — Checked inline contracts**:
`#Inline`/`#Inline(Always)` on
a `fn` or `fn Type.method`; methods stay ordinary functions, no macro-rewrite
hooks. `#Inline` is a soft hint (`#[inline]`; never rejected). `#Inline(Always)`
is a checked promise (`#[inline(always)]`): sema proves the call can actually
inline or fails the build naming why — self-recursive (E0917), address-taken
(E0918), or over the statement ceiling (E0919). PascalCase per
D-CONTRACTCASE1.

**D-PERSIST1**: `#Persist` module binding survives `jet dev` hot reload;
identity = module path + name; layout change re-decodes Codable-style, falls
back to reinit + warning. Dev-tier only. Values live in the shared
`jet_foundation::Persist` store at the runtime-heap boundary (tier-0 and
tier-1); not inside one JIT module. *(rides JIT hot-reload runtime)*

**D-EFFBUDGET1 — Package effect budget**: every build prints a one-line
effect summary and records per-dependency provenance in the lock.
`effects: { allow: […], deny: […] }` in `package.jet` enforces the whole graph
(E1220 names the dependency); `grants: { "dep": [Effect] }` per-dep escape;
malformed block E1221.

**D-STREAMYIELD1 — Generators** *(amended by D-CONC-STREAM1=A)*:
`fn f() => Stream<T>` uses `yield expr` to hand a value to the consumer and
suspend until the next pull; falling off the end (or a bare `return;`) ends the
stream; `return value;` is E0806. Consumers are ordinary `loop x; f() { }`
loops — one keyword, one type, no async/await coloring. The producer is a task:
`yield` is a wait point, dropping the iterator is cancellation, and the
producer unwinds at its next wait point with normal cleanup. This is the task
cancellation law on AOT, JIT, and the interpreter; it does not require
coroutine syntax.

### Comptime & metaprogramming

**S26 — Comptime, value-level**: layered, value-only. **One law: comptime
never creates, parameterizes, or selects a type, and never affects dispatch**
— polymorphism is traits-only. Pure Jet is comptime-callable with no
annotation; evaluation is a sema tree-walking interpreter, type-checked
first, fuel-limited; comptime `panic` is a user-authored compile error;
results lower to constant data. Permanent differential CI: interpreter and
compiled runtime must agree bit-for-bit. **Rejected forever**: token/AST/
attribute macros, custom syntax, comptime types, const generics in v1.

**S57 — Compile-time demand** *(as amended by D-META-STAGE1=B and
D-ONCE-AT1=D, ratified 2026-08-06, card #1537)*: ordinary foldable expressions
fold implicitly. Compile time has one mark, `@`, and **the mark belongs to the
name**, so it is written at every mention:

```jet
@limit :: 1000

fn run() {
    @ {
        @ratio :: @limit /% 10
    }
    @if debug { … }
    print("limit: {@limit}")
    print("ratio: {@ratio}")
}
```

`@name :: expr` demands an immutable value now and fails the build if
evaluation cannot complete. A bare mark opens the execution block (`@ { … }`,
the Jai `#run` analog) and precedes the compile-time verbs `@if` and
`@loop`. Sema treats the mark as part of the identifier, so a plain name and
a marked name can never denote the same binding, and codegen keeps them apart:
a marked name spells `__jet_ct_<name>` in generated Rust where a plain name
spells `__jet_<name>`.

Because the name is the same name inside and outside a compile-time block,
there is nothing to carry out. **D-CTMARKER1 retires outright** — the splice
concept, its carry-out rule, and E2713 are deleted rather than generalized; an
unresolved marked name now reports the ordinary E0107. `#Known`, `#Known if`
and the `#Known` block retire together and teach E0377, and `#Known` leaves
the marker registry because a stage is not a rule about a target. The
`comptime` keyword stays retired (E0374).

Constants: `COMPTIME_MARK` and `is_comptime_name` in
`crates/jet-foundation/src/Syntax/core_surface.rs`. Examples:
`examples/features/comptime/comptime_block.jet` and `comptime_if.jet`.

**D-LAYOUT-FACTS1=B — Compiler-owned type facts** *(spelling amended by
D-ONCE-AT1=D)*: `T.@layout`, `T.@name` and `T.@fields` are one closed spelling,
each projecting the matching
`TypeInfo` member. The mark after `.` is not a general user-member escape:
any other marked member reports E0302 and names the three facts.

**D-CTCORE1 / D-CTIO1 / D-PURE2 — Comptime I/O**, amended by
**D-META-EFFECT1**: comptime and runtime read the same Core effect fact. An
empty effect set is Tier 0 and may evaluate without a gate. Recorded,
hash-checked inputs such as `embed_file(path) => String`,
`embed_bytes(path) => [U8]`, `find(glob)`, and pinned `fetch` are Tier 1; they
enter `.jet/lock`. Ambient effects are Tier 2 and keep the explicit
`#Impure("reason")` plus the `--gate impure=allow` gate. **D-STRPARSE1**:
comptime evaluation may pass through `Result`/`Option` for pure parse paths.

**D-CTEFFECT1 — Comptime effect tiers**: Tier 0 pure always-on; Tier 1
hashed-reproducible recorded into `.jet/lock` (`@embed`, `find`,
`fetch(url, sha256:)`); Tier 2 ambient requires `#Impure("reason")` **and**
`--gate impure=allow`. **D-CTFIND1/2**: `find(glob) => [String]` builtin, sorted,
hash-recorded; hand-rolled std-only glob (`*`, `**`, `?`, `{a,b}`, `[a-z]`).
Shipped by #350.

**D-MODCOMPUTE1=A — Computed module fields: pure dependency graph** *(ratified
2026-07-16, card #673)*: a module field may use any Tier-0 pure expression,
top-level immutable `comptime` values, pure helper call graphs, and sibling
fields of the same module. Fields evaluate once, in deterministic dependency
order; source order breaks independent ties. A cycle fails before any plan
exists and prints the complete chain. Tier-1 and Tier-2 reads are rejected
inside fields: locked external input reaches a field only through a top-level
`comptime` binding (D-CTIO1) or the `fn build` input surface. No field gains
`BuildContext`, `#Impure`, filesystem, network, environment, clock, or
randomness authority.

**D-METADEPTH1/2 — Metaprogramming ceiling**: read-only reflection + derives.
Rung B granted: whole-program read-only reflection + structured diagnostics
from the build entry (post-sema snapshot; diagnostics must carry
code+what/why/fix). No mutation, no AST injection, no macros
(D-PROCMACRO1/D-READERMACRO1 deferred/rejected; libraries can never mutate
grammar).

**D-METAREFLECT1 — Reflection API**: `T.reflect()` returns a `Type` handle —
`.name` is the leaf, `.path` is the canonical typeable path, and `.fields` is
the field list; each field carries `.name`/`.ty`/`.markers`/
`.has_marker("…")`. Runtime `reflect.of(x)` uses the same rule: `.type_name()`
is the leaf and `.path()` is the canonical path.

**D-LAYOUT-FACTS1=B — Focused compiler facts use `@`** *(ratified
2026-08-03, spelling amended by D-ONCE-AT1=D)*: `T.@layout` is exactly the
focused projection of `T.reflect().layout`, not a second metadata model. Users
cannot declare `@` members.
members. Layout exposes kind, optional size/alignment/stride, target,
guarantee, source, fields, and typed `[.field]` selection. Field byte facts are
optional when physical layout is not guaranteed. Completion visibly groups
`@layout` as a compiler fact; hover, rename, navigation, diagnostics,
`jet inspect expand --facts layout`, and JSON project the same model.

**D-METADERIVE1 — User derives**: `derive T.Wire { … }` (old
`derive Wire for T` is E2714); the header names the provider and target, and
the body contains typed Jet members. `@` holes are filled at expansion and the
resulting items enter ordinary sema like hand-written code (**D-META-CODE1** —
never inject unchecked AST or re-lex emitted text). `@loop` over
`T.@fields` contributes one checked member per field. Local-only orphan rule.
Routed through the existing marker system (D-USERDERIVE1).

**S56** stays open for Epoch 3 (typed-reflection hardening; see Open
decisions).

### Low-level tier

**S58 — Two gates, one keyword**: `use core.mem` is the discovery gate
(allocators, `*T`, layout/repr, volatile read/write). `#Unsafe("reason") { … }` /
`#Unsafe("reason") fn` is the audit gate (**D-UNSAFE2** — the reason is the
gate's argument; **D-UNSAFE-REASON1=A** *(ratified 2026-07-23, card #762)* —
bare `#Unsafe { … }` / `#Unsafe fn` are hard E3112 errors; whole-fn form
requires an enclosing `#Unsafe` at call
sites). Gated ops: deref `p.*`, raw-pointer-of `*x`,
volatile `mem.volatile_read(p)` / `mem.volatile_write(p, value)`, pointer math,
transmute-class casts, FFI pointer crossings (outside the gate: E0208).
Address-of is `mem.address_of(x)`. `mem.cast_ptr<T>(p)` is the cast primitive
(D-CASTPTR1); no compact pointer-chain syntax (D-POINTERCHAIN1).
Generated `unsafe` appears only inside user-gated regions + vetted internals
(I1). Onboarding never mentions any of it.

The tier rule is simple: use the import gate when sema can prove that an item
stays within Jet's safe ownership and value model; use the audit gate when use
can create, dereference, or access raw or foreign memory. Owning allocators and
address-stable pins are import-gated because they cannot corrupt memory by
existing. Raw-pointer construction, raw-pointer dereference, and MMIO are
audit-gated because their use can. The canonical named-item table is
`CORE_MEM_GATE_TIERS` in `crates/jet-foundation/src/Syntax/core_surface.rs`.
The `jet-sema` gate test requires every current `core.mem` export to have one
tier.

**D-UNSAFE-OBLIG1=A — gate-only default with optional typed obligations and
per-site control**
*(qualified owner ratification 2026-07-15, card #645: A with ballot C's
per-site flexibility)*: absent policy keeps the existing `#Unsafe` gate with no
per-operation obligation records. Package or organization policy may require
typed `valid_ptr`, `aligned`, and `no_alias` obligations; an undischarged
required obligation is an error. A typed assertion is a postfix record on the
immediately preceding operation statement (`mem.volatile_read(p)` followed by
`assert valid_ptr, aligned`); it cannot discharge a later operation or cross a
control-flow boundary. `.Relaxed` relaxes typed per-operation obligations but
never the required reason. `.PerSite` is also available: each gate selects
`obligations: .Track` or `.Skip`, and organization policy may reject `.Skip`.
Every mode still requires a lexical `#Unsafe` block or function for every
low-level operation; none permits generated Rust `unsafe` outside I1's audited
regions. `jet inspect unsafe` reports the effective mode, its policy source,
each gate, and tracked operation state.

**D-FLAGSHIP-MMIO1 — MMIO writes**: volatile writes use the Core helper
`mem.volatile_write(ptr, value)`, paired with `mem.volatile_read(ptr)`. No
pointer-assignment lvalue spelling is added.

**D-FLAGSHIP-WEBAPI1 — Browser events and storage**: web flagship slices use
`core.web` for browser-owned state and DOM events. `web.on(selector, event,
handler)` binds an event listener, `web.value(selector)` reads an element's
current value/text, and `web.storage.local` / `web.storage.session` expose
`get`, `set`, `remove`, and `clear`. `get(key)` returns `String?`, so missing
storage reads use the standard fallback operator: `web.storage.local.get("tasks")
?? "[]"`. Component-level events are the same mechanism at the compiler level:
components emit stable selectors/ids, then bind handlers through `web.on(...)`;
there is no second component-only event language.

**D-UNINIT1 — Visible uninitialization**: skips zero-fill for a binding, gated
behind `use core.mem` (E0424); sema proves write-before-read on all paths
(E0420; POD-only E0423); lowers to `MaybeUninit` after the proof. *(sema
green; codegen rides D-FIXARR1 stack arrays)*

**D-UNINIT-SENTINEL1 — `uninit` contextual keyword (opt D, ratified
2026-07-02; amended by D-UNINIT-SENTINEL2)**: replaced D-UNINIT1's marker
spelling with `uninit` as a contextual keyword. The original trigger was
`buffer: [U8#4096] := uninit` (RHS of `:=` on an annotated binding). The old
`#Uninit buffer: [U8#4096]` marker is a hard parse error (E0426).

**D-UNINIT-SENTINEL2=A — body sentinel `Type.{ uninit }` (ratified
2026-07-24)**: amends D-UNINIT-SENTINEL1 after D-BIND-BARE1 and D-DOTCTOR3.
`uninit` is legal only as the whole body of a typed-literal head:
`bytes := [U8#128].{ uninit }` (scalars too: `n := Int.{ uninit }`). The
annotated form `name: Type := uninit` is gone with typed bindings. Flow proof
E0420 / E0423 / E0424, POD-only, MaybeUninit lowering, and `use core.mem` stay
the same — only the spelling moves. Anywhere else, `uninit` is an ordinary
identifier (or bare `:= uninit` is E0421).

**D-REF-SHORTHAND1 — Stored-ref field shorthand (retired 2026-07-04 by
D-MEM1/S3)**: originally, a stored-reference field spelled its type `&T` —
the borrow sigil already used at call sites — instead of a bare `T` plus a
separate marker; the owner was *inferred* at each construction site (one
candidate → it, two or more → **E0207**), disambiguated with `#Ref(label)`
when needed; the retired `#Ref(owner) name: T` form (plain type, no `&`) was
a hard teaching error (**E0427**). D-MEM1/S3 deleted the whole mechanism
outright — raw `&T` struct fields remain outside the grammar and E0207/E0427
remain retired. D-MEM-VIEWRET1=B later superseded only the blanket return/store
ban: the selected safe boundary is a named, provenance-carrying `View<T>` or
`ViewMut<T>`, while `#Ref` and raw `&T` storage remain retired. Owned fields,
`Shared<T>`, and `Pool<T>`/`Id<T>` remain the non-view alternatives.

**D-REF-SHORTHAND2 — `#Ref(label)` disambiguator (retired 2026-07-04 by
D-MEM1/S3)**: originally the owner label stayed on the `#` directive plane,
spelled `#Ref(label)` — *not* `#Ref`, resolving the sigil clash with
D-MARKERMOVE1. Deleted along with D-REF-SHORTHAND1's `&T` fields; a
`#Ref(label)` naming no candidate owner used to be **E2306** — also gone
(retired stub row in docs/spec/diagnostics.md). `jet inspect expand --facts refs`
(the lens that reported these owners) is gone with it.

**D-REGION1 / D-ALLOC1 / D-ALLOC2 — Arenas & regions**: regions are implicit
and scope-inferred by default (the region is the arena binding's lexical
scope); explicit `#Region(r) { … }` for the expert tier (respelled by D-BLOCKPLANE1, 2026-07-12). `arena ::
mem.Arena.new(capacity: 4096)`; `arena.alloc(value)` returns a scope-bound
view — escape E0631, use-after-reset E0632. D-SHAPE-RESOURCE2 later supersedes
terminal `free()` with universal `close(^allocator)`. Arenas live flat in
`core.mem` (D-REF2); arena values are not `#Unsafe`.

**D-FIXED-BACKING1=A — Fixed allocator backing**: `fixed ::
mem.Fixed.new(size: N)` requires a positive comptime `N` and synthesizes one
inline `[Byte#N]` in that lexical frame; `mem.Fixed.over(&storage)` borrows one
mutable fixed-size byte array instead. Both constructors must directly
initialize a lexical binding. Payload plus alignment grows from the buffer
start, reverse-drop metadata reserves from its end, and allocation fails
atomically before the cursors collide; there is no heap fallback. The backing
borrow is exclusive until consuming `close(^fixed)` or scope exit. Fixed
handles and allocation views cannot escape, be stored/captured, or cross a
task/join boundary, and `reset()` is rejected while any allocation view lives.

**D-ALLOCFAIL1=A — Fallible allocation family** *(ratified 2026-08-12)*:
`List.try_new`, `List.try_with_capacity`, `try_push`, `try_reserve`,
`Map.try_insert`, `String.try_push`, and
`mem.Arena.try_alloc`/`mem.Bump.try_alloc`/`mem.Pool.try_alloc`/
`mem.Fixed.try_alloc` return `T ? AllocError`. `AllocError` is a Core value
with `requested_bytes: Int` and `allocator: String`; a failed allocation is
an ordinary error value and never calls the hosted abort hook. One Prelude
implementation owns the fallible allocation semantics for AOT, JIT, and
interpreter tiers. Plain allocation APIs keep their existing hosted abort
default. A fallible result remains must-use, so E0402 rejects an ignored
`try_` call. This decision supersedes no earlier allocation surface.

**D-SOA1 / D-SOA2A–D — Columnar layout**: `#Layout(columnar)` on a struct;
a `[S]` of it lowers to a struct-of-arrays with a logical-Vec API
(index-read gathers, field-read hits the column). Whole-struct only (partial
E1109); `columnar [T]` type-position reserved (E1107); deferred surface ops
E1108; serialization-transparent. **D-REPRC1**: `#Layout(c)` = C repr in the
same family (growable field under it = compile error).

**D-SIMD1 / D-SIMD2 — SIMD**: portable lane types `F32x4`/`F64x2` —
`F32x4(…)`, `.splat(x)`, `v[i]`, element-wise ops, `v.sum()` /
`v.reduce(.Add)`; **D-REDUCE-VALUE1=A** *(ratified 2026-07-23, card #762)*:
the typed `ReduceOp` values are `.Add`, `.Mul`, `.Min`, `.Max`, and `.Avg`;
the old argument-position markers fix to dot values. `[F32#4]` bridges via
`from_array`/`to_array`
(E2510/E2511). Raw intrinsics behind `#Unsafe`. Operator overloading exists
**only** on built-in lane/linalg types.

**D-JIT1 / D-JIT2 / D-JITDEP1 — JIT tier**: production is AOT; the Cranelift
JIT is the dev-loop tier-1 over the interpreter tier-0, behind the
`JitBackend` seam, in its own `jet-jit/` crate (`Source/` stays std-only).
**D-DEVMODE1 hard rule**: dev-runtime output must be byte-identical to the
release build — divergence is a release blocker.

**D-PLUGIN1 / D-DEP-WASM1**: `target: plugin` compiles to a sandboxed WASM
module (wasmtime + Component Model, typed `.wit` contract), safe by default.
Plugin target support is shipped for the v1 scope: all-`Int` or all-`Float`
exported functions, deny-by-default plugin effects, `.wit` emission, component
lifting through `wasm-tools`, host loading through `core.plugin`, version
compatibility checks, and Jet-owned diagnostics E1257-E1260. This is the
**application-plugin** substrate (fixed WIT world `jetplugin`) — not the
compiler-extension API (D-DX5-HOOK1) and not PATH `jet-*` helpers (D-DX5).
**D-NOSTD1**: no `no_std` flag — the std baseline follows the typed platform
`target:` (bare-metal ⇒ no-std).
**D-OOBPROOF1**: bounds-check elision is proof-carrying: a fixed-list index
whose distinct-`Int` invariant fits `0..N-1` lowers without the runtime bounds
helper; other dynamic indexes keep the check.

**D-DX5-HOOK1=A — compiler-extension v1 boundary** *(ratified 2026-07-14,
card #549)*: the optional formal compiler-extension API's first hook is a
**typed read-only post-sema snapshot** delivered to an **isolated WASM
Component Model** guest. The host reuses jet-pkg-model's shipped wasmtime
Component Model substrate (same `WASMTIME_CRATE_SPEC` as `core.plugin`) with a
**compiler-specific** WIT world `compiler-extension-v1` (`package
jet:compiler-extension@0.1.0`, export `analyze`). Plugins inspect typed
symbols, types, effects, spans, and provenance; they return findings and
proposed edits. The host validates every response and remains the only
semantic authority (I2/I3). Later parse or codegen capabilities extend the
same negotiated protocol — they do not invent a second plugin system (I8).
Exact user-facing registration spelling remains a later ballot if new syntax
is needed. Distinct from PATH `jet-*` helpers (D-DX5) and application
`target: plugin` / `core.plugin` (D-PLUGIN1).
**Wire contract (Tower #549 C3):** deterministic JSON snapshot/response
(`protocol=1`, `stage=typed`, capability negotiation, exact
types/symbols/effects/spans/provenance fields, validated findings/edits,
resource limits, `trust=untrusted`, Idle→Loaded→Closed lifecycle with
rollback of uncommitted stages) lives in
`crates/jet-pkg-model/src/CompilerExtension.rs` and is documented under
architecture.md "Compiler-extension plugins".

### Testing & benchmarks

**S43 — Tests** *(D-TESTPAREN1, D-TGT5)*: `#Test("name") { … }` blocks with
`require`/`require_eq`; `jet test` auto-collects every `#Test` in the
package; optional `test { entry: … }` target adds an out-of-tree file.
**D-TEST1**: a parameterized `#Test fn name(p: T)` is a property test —
~200 generated cases (`JET_PROP_SEED`), automatic shrinking; ungeneratable
param type E0613. **D-TEST4**: fenced ```jet blocks in `///` docs run as
doctests; `EXPR // => VALUE` compares JetShow output (E2901).

**D-BENCH1 / D-BENCH-MARKER1=A**: `#Bench("name") { … }` region benchmarks, run by `jet bench`
(ops/sec + ns/iter); the `benchmark` manifest target points `jet bench` at a
package entry.

**D-BENCH-PARITY1=B** *(ratified 2026-08-05, card #1452)*: `jet bench` and
`jet test` share file, recursive-directory, and project targets. `jet bench`
selects region names with `--filter=<substr>` and always runs serially for
timing integrity. Test-only flags (`--shuffle`, `--coverage`, and
`--update-snapshots`) stay off the bench surface. A multi-file human report
uses one heading per file and path-qualified region names; `--json` emits one
record per region. Future runner-surface changes apply to both commands or
name an explicit carve-out.

**D-COV1**: `jet test --coverage` — per-function HIT/MISS table; probes only
in this mode, normal codegen byte-identical. **D-TOOL4**: snapshot testing
with `-u`/`--update-snapshots`. **D-A11YGATE1**: accessibility issues are
`jet lint --a11y` lints (E2930/E2931), opt-in CI gate.

**D-TESTKIT1=A** *(ratified 2026-07-07, card #308)*: `#Test` remains the only
test syntax. `core.testing` adds snapshots, fixtures, corpora, temp dirs, fake
clocks/random, HTTP servers, and golden files as library
helpers. Helpers emit structured test metadata so reports and CI can render
categories without adding markers for every feature. Epoch 3 ships `snap`,
`golden`, `fixture`, `temp_dir`, `corpus`, `fake_clock`, and `fake_rng`;
the existing `expect(...).snapshot()` remains the canonical
assertion snapshot path.

### Formatting & comments

**S5 — Comments**: `//` line; `/* … */` **nesting** block comments
(unbalanced E0002).

**S49 — Doc comments**: `///` summary lines above items, plain text v1;
doctest-runnable (D-TEST4); no `/** … */`.

**S6 — No visible semicolons** *(S6-R, amended by D-LOOP-COMMA1=A)*: statements end at line end; the
lexer inserts Go-style terminators after any line whose last token can end a
statement. Layout rules: `-> Type` and `{` stay on the closing-`)` line; a
terminator is suppressed when the next non-blank line starts with `.` (chain)
or a binary/logical operator. Loop header clauses use commas; the retired
semicolon spelling is E0373. New terminal tokens must be added to `ends_statement`.

**S44 — Formatter** *(D-FMT1, D-FMTPARENS1, D-NARG2, D-BRACE1=A, D-FMTCOLLAPSE1=B)*: one style, zero
config — 4-space indent, same-line `{`, width 100, spaced binary operators.
Every fitting, comment-free braced block collapses to one line (controls, arms,
function bodies, marker regions) regardless of authored layout; author grouping parens are preserved even when
redundant; call-site labels never added/stripped; dot-chain breaks preserved
(**S69** — break before `.`, optional trailing comment per step). Idempotent,
not AST-canonical. **Every new syntax needs formatter emission + a fmt
STABILITY test** (idempotence alone misses dropped tokens).

### FFI & external dependencies

**S50 — Rust FFI**: `extern rust "crate@version" { fn name(args) => T =
"rust::path" }`. Version pins required; by-value boundary only — no borrows,
callbacks, or trait objects across the edge.

**S59 — C FFI** *(D-CFFI2, D-CFFI-CANON1, D-CBIND2/3/5/6)*: auto-generated
bindings + optional user overlay; by-value first, pointers only inside S58.

| Layer | Shape |
|---|---|
| Autogen | `#Bindgen module c.<lib>.__bindgen__ { … }` in `.jet/bindings/c/<lib>.jet` |
| Overlay | `#Extern module c.<lib> { … }` — merged bindgen ∪ overlay, overlay wins |
| Script | `use "raylib.h" as rl` — compile-time bind on cache miss |
| Project | `use c.raylib as rl` or `use c.[raylib as rl, sqlite3]` — one form per lib per file |

All foreign-language namespaces use the same `.[…]` member-list form as ordinary
namespaces. Single-library `use <lang>.<lib> as alias` remains valid; there is no
FFI-only import grammar (D-VERDICT-1867-1).

**D-SHAPE-CASE2=A — FFI casing escape: binding modules are exempt zones**
*(ratified 2026-07-16, card #665)*: casing diagnostics (S54/D-SHAPE-CASE1)
skip declarations inside `#Bindgen` modules, `#Extern` modules, and
`extern rust` blocks. Call sites use the foreign spelling verbatim, so a name
in Jet code matches the C header and the library's documentation. Ordinary
Jet modules stay fully enforced; the exemption boundary is a module kind sema
already tracks, so no new syntax exists.

Link resolution: declared `<lib>: c@system` / `c@"vendor/path"` in `package.jet`
`deps:` → pkg-config fallback → E3201. C deps are link deps, never packages.
`jet inspect bind` uses a native std-only C-prototype parser (`Source/CBind.rs`);
binds scalars and `char*`↔String; `#define` constants only. Old
`@extern`/`#extern` spellings E0060. `#Bindgen`/`#Extern` PascalCase.

**D-CABI-CALLBACK1=A / D-CABI-RESULT1=C / D-CABI-PLATFORM1=A** *(ratified
2026-07-11, card #436)*: C callbacks accept only C-convention function values
whose arguments and return are C-safe. A callback is non-null, monomorphic,
explicitly bounded `=[]=>` or a capture-free lambda with an inferred empty row, and safe for foreign threads:
no heap allocation, mutable static or thread-local state, scheduler access, or
panic-capable path. Its pointer is stable for the program lifetime and may be
called concurrently or reentrantly. There is no hidden context pointer,
nullable callback, or alternate callback ABI. Unsupported cases are E3203.
Generic `Result<T, E>` remains illegal in C declarations: expose a raw C status
plus out-pointer function and write an ordinary Jet wrapper that initializes
the out value and maps the status. The compiler invents no error adapter.
`#ABI(name)` is a per-function marker with no module inheritance. Omission means
C. `system` selects the target-native convention; `cdecl`, `stdcall`, and
`fastcall` exist only on Windows x86, `win64` only on Windows x86_64, and
`sysv64` only on non-Windows x86_64. ARM/AArch64 accept only C/system.
Unknown, target-invalid, and variadic-invalid choices are E3212/E3213/E3214;
variadics allow only C and Windows-x86 cdecl. Symbols are used exactly as
written. ABI participates in compatibility, overlay merge, caching, wrappers,
and bindgen. Alternate-ABI functions are direct-call-only and are E3203 when
taken as values. Bindgen records both portable declared `system` and the
resolved target ABI.

**D-FFI-INLINE1=A — inline foreign tier** *(ratified 2026-07-11, card
#501; owner ratification comment renames the marker: **`#FFI(<lang>)`**,
not `#Foreign` — FFI fully capitalized per S66)*: the fourth D-FFI-UNIFY1
tier. `#FFI(<lang>) fn` declares an ordinary Jet signature whose body is one
triple-quoted raw foreign-source string. Per D-FFI-RAWBODY1, the bytes between
the delimiters are passed to the binder exactly as written: no escapes,
interpolation, indentation stripping, or newline normalization. Sema checks
every call site against the signature; the language's
binder compiles the body on cache miss through the same machinery as the
script tier; a body/signature mismatch is a Jet diagnostic naming both
sides (I2). Effects declare like any extern; unsafe-language bodies
(c, cpp, asm) additionally require the enclosing `#Unsafe("reason")` gate
(S58). One shape for every current and future language.

**D-FFI-ASM1=A — inline assembly** *(ratified 2026-07-11, card #501; the
`asm` instance of the D-FFI-INLINE1 tier, whose ratification cleared this
entry's gate; owner comment: spelled **`#FFI(asm)`**)*: `#FFI(asm) fn`
bodies are per-target assembly with the Jet
signature as the operand contract. Integer parameters are named register
inputs (`{parameter}`), the single integer return is anchored by
`; -> return`, and audited registers are declared by `; clobbers ...`.
Every declared parameter must be referenced and exactly one return anchor is
required. Requires `use core.mem`
plus an enclosing `#Unsafe("reason")` (S58); outside the gate is E0208-class.
Target variants select via the existing `@if @build.os ==` /
`#Target` machinery. Lowering emits Rust `asm!` so rustc verifies
register/clobber facts per target; every user-facing error stays a Jet
diagnostic (I2). `core.mem.intrinsics` may wrap popular cases as named
functions on top — beginners meet only the named functions.

**D-FFI-CPP1=A — C++ binder depth** *(ratified 2026-07-11, card #501)*:
`cpp.*` binds at full depth via a clang-based binder emitting a generated,
cached C shim archive per library. Classes become opaque `#SingleUse` owned
handles with consuming cleanup (S63); methods become ordinary Jet methods;
exceptions are caught at the shim and surface as `T ? CppError` (fallible
at every call site); templates instantiate on demand (`cpp.vector<Int>`);
overloads collapse to argument labels (S61); operator overloads become
named methods. The overlay tier corrects wrong guesses. Internal staging
may land C-linkage first, then classes/exceptions, then templates — the
ratified surface is full depth, so no intermediate stage becomes law.

**Polyglot binder wave (all =A, ratified by owner 2026-07-11, cards
#502–#504; per-language depth under D-FFI-UNIFY1, host models following
the D-FFI-PY1 precedent):**

- **D-FFI-GO1=A**: `go.*` — in-process `go build -buildmode=c-archive`
  static shims; Go runtime rides in-process; blocking calls carry
  effects; handle pinning bridges Go GC and Jet ownership.
- **D-FFI-JVM1=A**: `java.*` (Kotlin/Scala ride the same bytecode
  surface) — embedded JVM via the JNI invocation API, created lazily on
  first `java.*` call; classes are opaque handles; checked exceptions
  surface as `T ? JavaError`; JVM provisioned by jetpack (I6).
- **D-FFI-DOTNET1=A**: `cs.*` (C#/F#) — hostfxr/hostpolicy embed; .NET
  Tasks bridge to Jet's concurrency runtime at the boundary; NuGet as jetpack
  provider.
- **D-FFI-FORTRAN1=A**: `fortran.*` — ISO_C_BINDING bridge via gfortran;
  arrays cross as `[T]`/`Tensor<T>` with explicit column-major facts
  recorded in the binding (order mismatch is a checked error, never a
  silent transposition).
- **D-FFI-LUA1=A**: `lua.*` — in-process VM (embedding is Lua's design
  point); tables ↔ `[K: V]` zero-copy views; effect root `=[Lua]=>`.
- **D-FFI-RUBY1=A**: `ruby.*` — sidecar worker (GVL + interpreter global
  state make embedding hostile); RubyGems as jetpack provider.
- **D-FFI-PERL1=A**: `perl.*` — sidecar worker; CPAN provider; legacy
  scripts callable as-is.
- **D-FFI-PHP1=A**: `php.*` — sidecar fpm-style worker pool; Packagist
  provider.
- **D-FFI-R1=A**: `r.*` (root reserved by D-DATA-BRIDGE1) — sidecar
  Rserve-style worker; `data.frame` ↔ `core.data.Table` typed round-trip;
  CRAN provider; plots return as SVG values (D-DATA-PLOT1-compatible).
- **D-FFI-COBOL1=A**: `cobol.*` — GnuCOBOL C-ABI binder; copybooks import
  as `#Codable` structs with fixed-width/packed-decimal wire facts
  (COMP-3 money maps to `Decimal`, never `Float`); enables strangler-fig
  migration of the COBOL estate.
- **D-FFI-OCTAVE1=A**: `octave.*` — sidecar Octave worker
  (MATLAB-compatible); matrices ↔ `Matrix<M,N>`/`Tensor<T>`; `.m`
  scripts callable; jetpack-provisioned.
- **D-FFI-SH1=A**: `Sh` typed text — the third D-TYPEDTEXT1 instance
  (same engine, I8): each `{hole}` becomes exactly one argv item, never
  word-split or glob-expanded; `core.process.run(cmd: Sh)` executes
  without a shell parsing user data; `Sh.raw("…")` is the sole audited
  escape; spelling `Sh.{"…"}` per D-UNIFYLIT1=A (former `sh"…"` prefix
  retired).
- **Phase 5 (ratified by owner 2026-07-12, card #507)**:
  **D-FFI-COM1=A** — `com.*` Windows COM/IDispatch automation root,
  Windows-gated (honest error elsewhere); typed stubs generated from
  type libraries via `jet inspect bind com` (committable); dynamic
  IDispatch fallback behind `#Unsafe`; the Office/VBA estate becomes
  automatable and migratable. **D-FFI-PWSH1=A** — `pwsh.*` sidecar
  PowerShell 7+ worker; cmdlet objects cross as `DataTree`; pipelines
  callable. **D-FFI-DART1=A** — dual surface: `dart.*` library binder
  (dart_api_dl) plus the Flutter embedding path (Jet compute compiled to
  C-ABI, callable from Flutter apps); interop floor for the mobile
  strategy (#480). **D-FFI-TCL1=A** — `tcl.*` in-process interpreter;
  live tool sessions for the EDA estate with typed result parsing.
  **D-FFI-ADA1=A** — `ada.*` GNAT C-ABI binder; Ada range/constraint
  facts recorded in the binding become checked boundary errors; pairs
  with `jet prove`. **D-FFI-PASCAL1=A** — `pascal.*` FreePascal cdecl
  binder; classes as opaque handles (cpp precedent, no templates); the
  Delphi estate gets call-in-place plus D-MIGRATE-SRC1 migration.
- **D-MIGRATE-SRC1=A**: source-importer framework law — `jet import
  <lang> <dir>` gains per-language semantic source importers; output is
  editable canonical Jet (D-WD5), every untranslatable construct is a
  TODO diagnostic (D-JPK-IMPORTTODO1 family), omissions are reported
  never dropped (D-JOS-NIXIMPORT1 discipline), detectable tests carry
  over, import is idempotent with update/dry-run/conflict policy; a body
  the importer cannot translate becomes a binder-backed FFI stub + TODO.
  Per-language importers ship separately under this law.

**D-FFI-UNIFY1 — FFI structure law**: every foreign language mounts as a
namespace `<lang>.<lib>` with the same three tiers (S59 generalized): script
tier (`use "xxhash.h" as xx` — bind on first compile), project tier
(`use py.h5instrument as h5`, dep pinned in `package.jet` as
`<lib>: <lang>@"ref"`), overlay tier (`#Extern module <lang>.<lib> { … }`,
overlay wins). `jet inspect bind <lang>` is a per-language binder emitting
inspectable bindings in `.jet/bindings/<lang>/<lib>.jet`. Generated bindings
are safe wrappers by construction (marshaling internals compiler-vetted like
std internals — I1); calling a foreign symbol outside a binding requires
`#Unsafe("reason")`. In-situ replacement: any `<lang>.<lib>` can be shadowed
by a Jet package exporting the same surface — call sites never change.
Binder diagnostics are Jet diagnostics with codes and snapshots (I2/I4); no
foreign toolchain error reaches the user unlaundered. One structure for all
languages (I8) — S59 is the C instance; S50's block becomes the rust
binder's declaration format inside `rust.*`; D-NPMTYPE1 stubs are the js
binder's v1; D-DEP1 vendoring/hash-pinning extends to every language's refs.
Per-language binder depth, all ratified 2026-07-03:
**D-FFI-PY1 (=A)**: Python's default host is a supervised sidecar CPython
worker (typed message boundary, crash-isolated, `=[Py]=>` effect added to the
D-EFF4 set); opt-in `py@embed` switches to in-process libpython for
zero-copy buffer-protocol arrays. One `use py.X` or `use py.[X, Y]` surface; the
tier never moves call sites. **D-FFI-JS1 (=A)**: one `use js.X` or
`use js.[X, Y]` surface, host chosen by
compile target — browser JS engine on the web target, QuickJS/componentize-js
WASM component on wasmtime for native targets. `jet inspect bind js` generates
committable typed stubs from a package's `.d.ts` — this AMENDS D-NPMTYPE1's
hand-authored-only floor; no-`.d.ts` packages get a `#Unsafe`-gated dynamic
surface; Node-subprocess broker is an opt-in tier. **D-FFI-SWIFT1 (=A)**:
swift-bridge-style generated projection over the fixed C-ABI transport
(D-JSWIFTFFI1) — `jet inspect bind swift` runs swiftc to emit `@_cdecl` shims +
typed Jet wrappers; classes/actors are opaque ref-counted handles;
throws→Result, async→Jet async; macOS/iOS + Linux-Swift only, honest gated
error elsewhere. **D-DEP-PY1 (=A)**: CPython approved as a runtime-side
dependency (libpython for the embed tier + interpreter for the broker) —
never in Source/ (I6), provisioned nixpkgs-interim/jetpack-long-term,
hash-pinned, native-ize obligation recorded, acquired only when a project
uses `py.*`. **D-JPK-EXTPROV1 (=A)**: npm/PyPI/SwiftPM become first-class
jetpack providers resolved by `<lib>: <lang>@"ref"` — fetched into the
hangar, vendored + hash-pinned, obeying U29 offline, U21 channels, U28
no-daemon, U24 provenance, U23 honest fallback; hash-verified on arrival.
**D-PLUGIN-EXPORT1 (=A, shipped c81)**: a `plugin` target's exported surface
is the top-level `pub fn` items of its entry file, all-`Int` or all-`Float`
(E1260) in v1, named by the manifest `export:` field (defaults to the package
name) and frozen via `Sema::ApiFreeze`'s pub-metadata semver snapshot
(re-grounded off the retired D-CAP4 `api: stable` machinery, which D-MEM1/S2
deleted — same intent, the still-live mechanism); the `.wit` world is
generated from those signatures — no new in-source keyword (I8).
**D-PLUGIN-VERSION1 (=A, shipped c81)**: plugin load-time compatibility is
structural — the `ApiFreeze` snapshot of the exported interface (keyed
`plugin__<export>` in `.jet/cache/api/`, diffed at the plugin's own build
time) is the contract; a plugin rebuilt with an unchanged interface still
loads; an incompatible change is E1257, naming the interface delta, never a
loader crash (I2).

**D-DEP1 — Dependency law**: the compiler stays zero-external-crate (I6).
Any crate-backed capability ships as a Jet package wrapping the crate via
`extern rust`, source vendored + hash-pinned (D-BFS1). Owner-sanctioned
bootstrap wraps (all carry a native-ize obligation): rustls bridge
(D-NET1/D-HTTPLIB4; `core.http` client default
HTTPS via rustls + system roots, D-TLS1=A; `core.tls` reserved for advanced
client TLS config), zip/tar (`core.archive`, D-DEP-ARCHIVE1), flate2/zstd
(`core.compress`, D-CORE-COMPRESS1/D-CODECS1),
rusqlite-bundled (`core.db`, D-DEP-DB1), ureq/hyper/tungstenite
(`core.http`, D-NETDEP1/D-HTTPLIB3), Cranelift (`jet-jit`, D-JITDEP1),
wasmtime (application plugins D-DEP-WASM1 and compiler-extension host
D-DX5-HOOK1), age-style crypto bridge
(D-JPK-SECRETCRYPTO1). `jet repl` stays std-only (D-REPL18). Raylib ships as
first-party `core.raylib` bridge package (D-RAYLIB1); `core.game` is the
scene-first game engine layered above it (D-GAME1=B, D-GAME2=A, D-GAME3=C).
npm interop = typed first-party stub packages, no `.d.ts` parsing
(D-NPMTYPE1); Swift interop waits on native-UI/C-ABI work (D-JSWIFTFFI1).

**D-REPLCOREEFFECT1=A (ratified 2026-07-11)**: `jet repl` uses the existing
effect model for ambient Core calls. An enclosing `#Grant(root)` supplies
lexical authority. Interactive sessions then authorize the exact
`(root, operation, resource)` tuple once or for the in-memory session;
reusing session authority offers continue or revoke before execution.
`--allow-{root}` skips ordinary prompts, while `--deny-{root}` always wins.
Non-TTY and transcript sessions never prompt and deny effects without the
matching allow flag. Filesystem operations stay within the REPL project root
and reject absolute paths, parent traversal, and symlinks. `Exec.Exit` always
gets its own consequence prompt interactively and needs both `#Grant(Exec)`
and `--allow-exec` outside a TTY.

**D-FE-REPL-HISTORY1=A (ratified 2026-07-11)**: `jet repl` persists the
latest 2,000 successful submissions per user in the platform state directory.
F3 opens interactive search; `:history search <text>` is the textual path and
`:history clear` erases persisted history. `JET_REPL_HISTORY=off` selects
session-only history; `JET_REPL_HISTORY_LIMIT=N` changes retention. Storage is
owner-only. Corrupt tails are discarded with a visible warning; unavailable
private storage visibly falls back to session-only history. Inputs are not
secret-filtered because Jet cannot reliably identify every secret.

**D-FE-REPL-MULTILINE1=A (ratified 2026-07-11)**: raw-terminal Enter submits
parser-complete input and inserts a newline when parsing needs more source.
Escape then Enter always inserts a newline. Enter on an empty continuation
line force-submits. One-line Enter stays unchanged. Cooked and non-TTY input
keeps bracket-balance continuation.

**D-FE-REPL-INTERRUPT1=A (ratified 2026-07-11)**: Ctrl-C during raw REPL
evaluation interrupts the current turn while keeping prior session state.
Jet-controlled execution polls every interpreter instruction and before and
after runtime calls, returning to a restored prompt within 100 ms. Blocking
external calls follow their cancellation contracts and produce a visible
still-stopping warning. No bindings from the interrupted turn commit; prior
external effects remain and the REPL says so. The turn is recorded as
`interrupted` and remains rerunnable. A second Ctrl-C during the active turn
exits the REPL; outside evaluation, Ctrl-C clears nonempty input first and
exits from an empty prompt.

### Core library

**S9 — Print**: `print` (adds newline).

**S51 — Core library**: exported as the `core` module — `use core.files`,
`use core.io as io`; dot paths select submodules; never quoted paths. `core`
is compiler-reserved (see D-CORENS1).

**D-GAME1 / D-GAME2 / D-GAME3 — Game stack** *(ratified 2026-07-05/06,
card #212)*: Jet ships a first-party game stack: public primitives plus a
batteries engine. The engine name is `core.game` (D-GAME2=A). The beginner
API is scene-first with a frame hook (D-GAME3=C): a `Scene` owns sprites,
shapes, sounds, camera, and input bindings as durable editable data, and
`scene.on_frame((frame) => { ... })` attaches small game logic to that scene.
The frame hook is not a second engine model; it is script on the scene. The
already-ratified `core.raylib` bridge package (D-RAYLIB1=A) remains the
interim compatibility floor beneath the native-shaped stack.

**D-FLAGSHIP-RAYLIB1 — Native raylib bridge** *(ratified 2026-07-07,
card #9)*: the flagship raylib slice uses the already-ratified `core.raylib`
surface. Generated code is headless by default; with `JET_RAYLIB_DISPLAY=1` it
dynamically loads native raylib and calls the real C window, draw, keyboard, and
FPS APIs. Missing raylib degrades to the same headless path so CI and ordinary
test runs do not link raylib or require a display server.

**D-GAME-ASSET1 / D-GAME-ECS1 / D-GAME-INPUT1 / D-GAME-REPLAY1 /
D-GAME-BACKEND1 / D-GAME-BUDGET1 — Stable `core.game` substrate** *(ratified
2026-07-06, card #238)*: the Epoch 3 headless Core floor ships scene-owned
asset registries (`scene.assets.image`, `scene.assets.sound`), struct-marker
components (`scene.component<T>()`) plus typed queries (`scene.query<T...>()`),
scene input bindings with per-frame snapshots (`scene.input.bind`,
`frame.input.pressed`), `game.Replay.record(".jetreplay")`, an explicit
`game.Backend.headless()` default. D-PERFBUDGET-GAMEMIGRATE1 supersedes the
former scene-budget value/setter with typed `perf` role declarations.
`game.run(scene, replay: replay)` produces a deterministic transcript without
renderer/audio/editor dependencies. Renderer, audio, editor, native asset I/O,
and richer replay files remain replaceable-package layers over this substrate.

**D-EVENT1 — First-party typed Event/Hook family** *(ratified 2026-07-07,
card #286)*: Jet ships one event semantic family as ordinary Core values, not
new event syntax. `core.event` exposes `Event<T>` for many-subscriber typed
occurrences, `Hook<T, R>` for ordered intervention points, `Subscription` for
explicit unsubscribe, `EventScope` for owned lifetime cleanup, `EventPolicy`
for sync/queued dispatch policy, and `EventTrace` for delivered/queued/dropped
debug facts. The compiler knows these types for checking, TIR lowering, docs,
debugger/Canvas projection, and editor highlighting; source sugar remains
reserved until examples prove the library spelling is too noisy. Default
dispatch is synchronous and deterministic: priority first, then subscription
order. `once` auto-unsubscribes, `scope.cancel()` drops all owned subscriptions,
and `with_policy<T>(policy_async(n))` gave the original queued entrypoint;
D-EVENT2=A supersedes it with `AsyncEvent<T,E>`. Ordinary hooks combine by
"last active handler result wins", with the call-site fallback used when no
handler is active.

**D-EVENT2=A — Typed async events (scheduler tranche)** *(ratified 2026-07-11,
card #286)*: `Event<T>` and `Hook<T,R>` stay synchronous. `core.event`
`async_result<T,E>(AsyncPolicy.{ capacity, overflow }, FailurePolicy)` creates
one `AsyncEvent<T,E>` queue; capacity must be positive. Overflow is exactly
`Block`, `DropNewest`, or `DropOldest`. `emit_async` returns
`Task<DispatchReport<E>>`. Capacity counts Queued only; Running and Pending do
not consume a slot. `queued_count`, `running_count`, and `blocked_count` expose
those states. Each payload uses stable priority then subscription order, with
once reserved atomically before invocation. Reports expose
`state`, `accepted`, `delivered_handlers`, `failures`, and ordered `EventTrace`.
`close()` rejects new and Pending producers as Closed while draining accepted
Queued and Running work. `EventScope.cancel()` and last-owner teardown hard-cancel
Pending and Queued work, request structured cancellation for Running handlers,
and publish exactly one terminal report. Inherited deadlines publish
DeadlineExceeded through that same single-winner transition. StopFirst stops
after the first `E`; Collect preserves
all `E` values in order; Log records failure facts in trace without storing `E`;
Ignore stores neither. A panic always stops that payload as HandlerFailed with
`DispatchFailure.Panic`, independent of failure policy. DecisionHook remains
in the same subscription and owner-lifetime family. The cross-primitive summary
is the [Bounded buffering law](spec.md#bounded-buffering-law).

**D-EVENT-CONTINUE1=C — Decision input, outcome output** *(ratified
2026-07-14, card #286)*: `DecisionHook<T,E>` handlers return
`HookDecision<T,E>`. `Continue` preserves the current value, `Transform(T)`
replaces it, and `Cancel`/`Fail(E)` stop the ordered fold. `run(T)` returns
`HookOutcome<T,E>`: `Continue(final T)`, `Cancel`, or `Fail(E)`. Zero handlers
returns `Continue(original)`. This keeps handler approval payload-free without
creating a second transform spelling or nullable run record.

**D-FILES-WRITE1 — `core.fs`/`core.files` merge** *(ratified/shipped
2026-07-04, cv5syntaxdecrees)*: one `core.files` module for both whole-file
convenience helpers (`read`/`read_bytes`/`write`/`append_all`/`exists`/
`remove`/`list_dir`/`create_dir`/`is_dir`/`copy`/`rename`) and streaming
handle constructors (`open`/`create`/`append` → `FileReader`/`FileWriter`,
D-IO2); `core.fs` no longer exists (greenfield — `use core.fs` is an ordinary
unknown-module error). **D-FILES-APPEND1 = A** resolved the merge collision:
the whole-file one-shot is `append_all(path, text)`; the streaming handle's
`.append(…)` is untouched.

**Serde & encoding** *(D-SERDE1–12, D-ENC1, D-JSONVERB1, D-SERDE-ACCESS,
D-ENC-YAML1)*: one format-agnostic data model. `#Codable` (≡
`#[Encode, Decode]`), `#Encode`, `#Decode` derives — a built-in compiler
field-walk, not S56 reflection. Formats are adapters in **`core.encoding`**
(`core.encoding.{json,csv,toml,yaml}`); encode verbs `to_string` /
`to_string_pretty`; typed decode `decode<T>` (target inferable from the
binding type; bare `decode(s)` yields dynamic `DataTree`). Hand-impl surface:
`encode`/`decode` verbs over `DataTree`
(`.Null/.Bool/.Int/.Float/.Text/.Array/.Object`); accumulated `[FieldError]`
values with `{ path, reason }`; encode infallible. Field markers (`#` plane):
`#Rename("x")`, `#Skip`, `#Default`/`#Default(expr)`, `#Flatten`,
`#RenameAll(camel|snake|pascal|kebab|screaming)` (E2409). Enum wire:
externally tagged default, single-value variants bare; `#Discriminant("type")`
internal (single unnamed payload under `"value"`), `#Untagged`. Unknown wire
keys ignored by default;
`#DenyUnknownFields` errors (E2412). Generic `#Codable` auto-adds
`Encode`/`Decode` bounds to wire-reaching type params only. Dynamic trees get
`?`-chaining accessors (`.field(name)`, `.at(i)`, `.int()`, `.text()`, …).
YAML parser is std-only, YAML 1.2 core incl. anchors.

**D-SERDE2 = A** *(ratified 2026-07-11, card #131; error contract amended by
D-VALIDATE-DECODE1)*: the hand-writable codec
surface is a first-class `Encode`/`Decode` protocol — a type implements
`encode(self) => DataTree` and `decode(tree: DataTree) => Result<T,
[FieldError]>` to own its wire form (e.g. a validated newtype serializing as a
bare string). The built-in `#Codable`/`#Encode`/`#Decode` derives become
ordinary derives that contribute the same checked Jet items and enter sema
(R11, D-META-CODE1) — no compiler-synthesized Rust, no R11 carve-out.
Ratifying this also fixes cross-module `decode<T>` (derive output
previously referenced entry-file-local paths).

**D-SERDE13 = B / D-SERDE14 = A / D-SERDE15 = A** *(ratified 2026-07-11, card
#131)*: the value tree's one user-facing name is **`DataTree`** — the retired `Data`
spelling becomes a teaching error pointing at `DataTree` (no alias,
I8); tree accessors (`.field`/`.at`/`.int`/`.text`/…) return `T ? [FieldError]`
everywhere. `.field` and `.at` fill `path` from the segment they read; scalar
accessors leave it empty and a containing decoder uses `FieldError.under`, so
`?` chains inside a hand `decode` remain direct; hand-built object
nodes take the map literal — `DataTree.Object({ "name": v, … })` —
insertion-ordered, and the pair-list form is not accepted.

**D-SERDE16 = A** *(ratified 2026-07-11, card #131)*: decode an arbitrary
subtree through its target's public protocol with `tree.decode<T>()`. The
spelling works uniformly for primitives, user types, `List`, `Option`, and
`Map`; generated derives emit it as ordinary Jet source. A target without a
`Decode` implementation is E0905 before codegen. No compiler-only helper,
hidden alias, alternate codec, or fallback exists.

**CLI & IO**: builder-spec arg parsing `args.spec().flag(…).option(…)
.positional(…)` with generated `--help` (D-ARGS1). `io.stdin()` handle with
`.lines()`/`.read_line()` (D-STDIN1). Scoped `#Live { … }` raw-terminal block (respelled by D-BLOCKPLANE1, 2026-07-12)
with guaranteed restore (D-TERM1). `core.log` auto-detects TTY (text) vs
piped (JSON); `log.setup(format:)` overrides (D-LOGFMT1).

**Core library audit ratifications** *(ratified 2026-07-07, cards #289-#308,
#310)*: the Epoch 3 Core expansion follows these owner picks.

The complete normative encoding contracts, including exact public types,
limits, counters, event/node keys, error projection, byte laws, conformance
vectors, lifecycle state, and edition migrations, are preserved in
[`encoding-decisions.md`](encoding-decisions.md). The entries below are an
index, not a substitute for that law.

- **D-COREIO1=A**: `core.io` owns stdout/stderr/stdin streams, flush, raw
  bytes, TTY facts, and terminal capabilities. Style/progress/raw mode/key
  events live under `io.terminal` or stream methods, honor TTY/NO_COLOR by
  default, and expose explicit force/raw controls for experts.
  Implemented Epoch 3 stream surface: `io.stdout()` / `io.stderr()` handles with
  `.write`, `.write_line`, `.write_bytes`, `.flush`, `.is_tty`,
  `io.terminal_width/height`, `io.style`, `io.style_force`, and `io.progress`;
  `io.progress(source[, description[, format]])` wraps `List<T>` or `Iter<T>` and
  reports percent, count, elapsed time, remaining estimate, and rate. TTY output
  redraws one line; non-TTY output appends one line per update. `NO_COLOR` removes
  ANSI sequences from custom formats.
  D-TERM1's `core.term` remains the direct raw-key bridge.
- **D-COREARGS1=A**: `ArgsSpec` is the one CLI parsing model. Typed
  `fn run(args: T)` derives an `ArgsSpec`; library/tooling code may build the
  same spec dynamically for subcommands, env fallbacks, completions, and tests.
- **D-ENV-MUTATE1=A**: `core.env` uses one process-global, raw-preserving
  logical environment. `unset(name) => Bool ? EnvError` removes a key and
  `vars() => [String] ? EnvError` returns a deterministic, owned names-only
  snapshot. Unix identity and ordering use exact bytes; Windows identity uses
  `CompareStringOrdinal` ignoring case while preserving the last spelling and
  exact UTF-16 value. `get`, `home_dir`, mutations, and child launches share
  this table. Child launch clones it atomically, composes `env_clear`, `env`,
  and `env_remove`, then passes raw entries to the OS. Jet mutations never
  mutate libc `environ` or the Windows process environment block. Invalid
  names and values fail without revealing inputs; `vars` fails as a whole on
  any non-Unicode entry. Existing editions keep `set => ()` and report
  invalid input through E3001; its fallible `() ? EnvError` signature waits
  for a major release plus edition opt-in.
- **D-PROCESS-SESSION1=A**: terminal-backed children use the existing
  `core.process` mechanism. `ProcessSpec.terminal()` is an explicit opt-in;
  ordinary argv execution stays pipe-backed and never inherits a terminal by
  accident. The same spec owns cwd, environment, stream, timeout, and child
  lifecycle policy. A requested terminal that the host cannot provide fails
  closed instead of silently running on pipes.
- **D-PROCESS-SESSION2=D**: the expert surface is
  `terminal(TerminalPolicy)`, with `TerminalSize` and `.Raw`/`.Cooked`
  `TerminalMode`; `ProcessChild.terminal` is `TerminalSession?`, present only
  when that child was launched with a terminal.
  `ProcessSpec.capabilities()` returns an open keyed report. Stable keys use
  `TerminalFact` constants; string keys allow preview facts without a parallel
  report type, and close literal typos suggest the nearest stable key.
- **D-MATHLIB2=A**: `core.math` is the canonical callable surface for libm and
  explicit checked/saturating/wrapping integer families. Value-context docs,
  LSP completion, and snippets may discover helpers, but emitted code uses the
  same `core.math` names.
- **D-RANDOMDIST1=A**: `core.random` owns deterministic PRNGs,
  distributions, shuffling, sampling, seed splitting, and test fixtures:
  `bool(p)`, `float_range`, `normal`, `exponential`, `weighted_pick`,
  `sample`, `bytes`, `split`, and matching `Rng` draws. Secret randomness
  remains in `core.crypto`.
- **D-TIME-CALENDAR1=A**: time uses distinct `Instant`, `DateTime`,
  `LocalDate`, `LocalTime`, `Duration`, and `Zone` types, with easy beginner
  constructors plus expert control over timezone data, monotonic clocks, fake
  clocks, and schedulers.
- **D-URL1=A**: `core.url` and `core.mime` are separate typed modules. `Url`
  owns parse/build/join/normalize, typed repeated query pairs, component
  percent-encoding, IDNA host handling, and `file:`/`data:` URLs. `Mime` owns
  extension mapping plus `type/subtype; param=value` parsing. `core.http` and
  `core.web` consume typed values instead of re-solving string escaping.
- **D-EMAIL1=A**: `core.email` is the one provider-neutral email mechanism.
  `Address`, `Message`, `Attachment`, `Envelope`, `Mailer`, `SMTPConfig`,
  `SMTPSecurity`, `SMTPAuth`, `DkimConfig`, `SendReport`, `RecipientReport`,
  and `EmailError` cover construction, transport, signing, and honest relay
  acceptance. `smtp_from_env` supplies verified STARTTLS beginner defaults;
  `smtp(config)` exposes the same Mailer with expert policy. Port 587 requires
  verified STARTTLS; port 465 TLS is explicit; STARTTLS never downgrades and
  password AUTH never crosses plaintext. MIME uses CRLF, bounded safe folding,
  Unicode encodings, content-derived collision-resistant boundaries, bounded
  attachments, and Bcc only in the SMTP envelope. RequireAll rejects before
  DATA when any recipient is rejected; DeliverAccepted is explicit. SendReport
  means relay acceptance, never inbox delivery. EmailError distinguishes
  configuration, DNS, connect, TLS, auth, protocol, rejection, transient,
  timeout, cancellation, and DeliveryUnknown. No hidden retry, thread-per-send,
  raw transport error, credential disclosure, external dependency, or duplicate
  async API exists. DKIM signs final bytes; SPF and DMARC remain DNS policy.
- **D-EMAIL-SMTP-SURFACE1=A**: SMTP uses closed records and enums plus one
  `Mailer.send(message)` mechanism. `Envelope` contains `from:Address` and
  `recipients:[Address]`; `email.envelope` validates it, and
  `message.with_envelope(envelope)` replaces only SMTP routing. MIME headers
  remain unchanged, so Bcc stays envelope-only. `SMTPConfig` contains host,
  port, `.StartTls`/`.TLS` security, `.None`/`.Password` auth,
  `.RequireAll`/`.DeliverAccepted` recipient policy, verified system or
  system-plus-CA trust, and bounded `Limits`. Ambient `#Context` alone owns
  deadline and cancellation. `SendReport` records server, accepted and rejected
  recipient reports, final response, and acceptance time; acceptance never
  claims inbox delivery. `EmailError` is the closed Configuration, DNS, Connect,
  TLS, Auth, Protocol, Rejected, Transient, TimedOut, Cancelled, and
  DeliveryUnknown set, each with operation, optional server/code, and reason.
  No trust-all mode, TLS downgrade, plaintext password auth, or automatic retry
  exists.
- **D-EMAIL-SMTP-CONFIG1=A**: SMTP passwords use the existing move-only,
  redacted `Secret`. `Limits` has `max_reply_line_bytes`, `max_reply_lines`,
  `max_capabilities`, `max_recipients`, `max_message_bytes`, and
  `max_auth_challenge_bytes`; `Limits.safe()` returns 512, 100, 100, 100,
  33554432, and 4096. Valid ranges, checked in that field order, are
  64..65536, 1..1000, 1..1000, 1..10000, 1..1073741824, and 1..65536; the
  first failure is `EmailError.Configuration`. `.SystemPlusCa(pem:[U8])`
  extends system roots with parsed PEM certificates while retaining DNS-name
  verification. Empty, malformed, or certificate-free PEM is rejected before
  credentials can be sent. There is no trust-all mode. SMTP interprets secret
  bytes as UTF-8 only inside authentication and rejects invalid UTF-8 as
  configuration.
- **D-EMAIL-DKIM-CONFIG1=A**: optional DKIM policy is the
  `dkim:DkimConfig?` field on `SMTPConfig`; `None` sends unsigned and `Val(dkim)`
  signs every message through that `Mailer`. `DkimConfig` contains exactly
  `domain:String`, `selector:String`, `private_key:Secret`, and
  `signed_headers:[String]`. Signing is fixed to `ed25519-sha256` with
  relaxed/relaxed canonicalization over final MIME bytes. `from` is mandatory;
  names match case-insensitively; duplicates, absent requested headers, hop
  headers, invalid DNS names/selectors, and non-32-byte Ed25519 seeds fail as
  `EmailError.Configuration` before connecting. Mailer owns and zeroizes the
  extracted key; no signing failure falls back to unsigned delivery.
  `smtp_from_env` accepts DKIM only when `SMTP_DKIM_DOMAIN`,
  `SMTP_DKIM_SELECTOR`, and `SMTP_DKIM_PRIVATE_KEY_BASE64` are all present;
  `SMTP_DKIM_SIGNED_HEADERS` optionally replaces the safe header set. DNS must
  publish `v=DKIM1; k=ed25519; p=<base64 public key>` at
  `<selector>._domainkey.<domain>`. SPF and DMARC remain DNS policy. Multiple
  identities use separate named Mailers; there is no per-message override.
- **D-ENCSTREAM1=A**: each `core.encoding` codec has one adapter identity with
  whole-value and reader/writer modes over the shared `DataTree`
  and `Codable` machinery. Reader/writer access is a mode of that adapter, never a second
  codec library.

  **D-ENCSTREAM-SURFACE1=A** fixes codec-native, synchronous pull handles.
  JSON/JSONL/CSV/CBOR expose opaque non-`Codable`, non-`Copy`, non-`Clone`
  Reader/Writer types. Constructors consume `^files.FileReader` or
  `^files.FileWriter`, take shared `EncodingLimits.safe()`, and return
  `EncodingError`; readers provide `next(&self)`, writers `write(&self)`,
  `flush(&self)`, and required idempotent `finish(&self)`. Items are `DataEvent`
  for JSON/CBOR, `DataTree` for JSONL, `[String]` for CSV, and D-ENCXML1's exact
  tagged `DataTree` event algebra for XML. Blocking calls provide backpressure; no
  hidden task, queue, partial-success state, or `WouldBlock` exists. Clean EOF
  follows complete structural/trailing-input validation; the first terminal
  error is cloned forever. `EncodingLimits` owns buffer/depth/item/total/entity
  expansion bounds and all constructors validate fields before IO. Shared
  `EncodingError` records format, kind, zero-based byte offset, optional
  one-based line/column, DataTree path, reason, and handle-free IO cause. Whole and
  stream paths share parser, value tree, errors, limits, canonical bytes, and
  bounded-memory law, summarized across primitives in the [Bounded buffering law](spec.md#bounded-buffering-law).
  Shipped `json.events(DataTree) => String` is unchanged;
  pull events exist only through `json.reader` until an edition migration.

  **D-ENCXML1=A** selects one lossless namespace-aware ordinary-`DataTree` XML
  algebra, not an `XmlDocument` or `XMLEvent` type. XML 1.0 Fifth Edition plus
  Namespaces 1.0 is the floor. `parse` accepts Unicode/UTF-8 declarations;
  `parse_bytes` additionally detects UTF-8 BOM and UTF-16 LE/BE BOM. The closed
  tagged node/event objects preserve document order, expanded `XMLName`, ordered
  namespace/attribute lists, text, CDATA, comments, processing instructions,
  declarations, doctypes, entity references, empty-element style, encoding/BOM,
  and token-local `XMLLexical` evidence. Untouched compatible tokens reuse exact
  text/bytes; edits invalidate only that token; false lexical snapshots are
  ignored. Entity default is Preserve; Reject and explicit in-memory Resolve
  are available. No files, URLs, catalogs, external entities, parameter
  entities, or replacement markup execute. Exact XML/expansion limits reject,
  never truncate. Codable uses Clark keys, `@` attributes, `$text` for simple
  content, and ordered `$content` for mixed content. Canonicalization is whole-
  document W3C Inclusive 1.1 or Exclusive 1.0, optional comments, UTF-8/LF/no
  BOM/declaration/doctype, over the semantic infoset. `XMLError` has the closed
  reason/location/path law and projects field-for-field into stream
  `EncodingError`. Folding/unfolding the exact tagged stream and tree algebras
  is lossless. The prior `{name, attrs, children, text}` tree is unratified and
  receives no compatibility alias.

  **D-JSONCANON1=A** makes `json.canonical(data, limits:) => String ?
  EncodingError` strict RFC 8785 JCS in edition 2027. It recursively emits UTF-8
  without BOM/LF/whitespace, preserves array order and Unicode scalars, sorts
  keys by unsigned UTF-16 code units, rejects duplicate keys/Bytes/nonfinite
  numbers, and uses the RFC-frozen ECMAScript binary64 serializer (`-0.0` ->
  `0`; Int must be exactly representable). Whole and `json.writer(canonical:
  true)` output/errors/bounds are byte-identical; nested canonical-object
  workspace is aggregate-bounded. Edition 2026 retains the old infallible
  prototype bytes; `jet fix` plus explicit edition upgrade inserts fallibility
  and audits hashing/signing fixtures. No 2027 legacy branch exists.

  **D-ENCBIN1=A / D-ENC-CBOR-SURFACE1=A** select RFC 8949 CBOR. The only whole
  surface is `parse([U8], options) => DataTree`, `decode<T: Codable>`, `to_bytes`,
  and `to_bytes_canonical`; `parse` and byte verbs return closed `CBORError`,
  while typed `decode<T>` returns the ratified `[FieldError]` list. `[U8]` uses native
  byte strings through typed Codable; untyped `DataTree` rejects byte strings,
  tags, bignums, non-text/duplicate map keys, and unsupported values rather than
  coercing. `CBOROptions` bounds depth/items/input and live allocation and may
  require canonical input. Canonical output is RFC 8949 section 4.2.1 Core:
  definite/shortest forms, preferred Float, canonical NaN, preserved signed
  zero, and pure bytewise lexicographic complete encoded-key order. Canonical
  validation checks original bytes. Ordinary output is preferred interoperable
  CBOR, not a cross-version hash promise. Edition 2026 keeps shipped
  `encode(DataTree)`/`DataTree`-returning `decode`; edition 2027 migrates to the ratified
  names, edition 2028 removes forwarding entries, and `jet fix` owns rewrites.

  **D-ENCBASE-STRICT1=A** makes edition-2027 base64/base64url/base32 decoding
  strict RFC 4648 by default: canonical alphabet/case/padding/length, no
  whitespace, and zero unused bits. Named false-by-default allowances cover
  ASCII whitespace and missing standard/base32 padding, URL padding, and
  lowercase base32 only; they never admit wrong alphabets, interior/excess
  padding, impossible lengths, `0`/`1` aliases, or nonzero unused bits. Errors
  are `invalid <codec> at byte <N>: <reason>` at original UTF-8 byte offsets
  under one fixed validation order. Edition 2026 uses one parity parser for the
  union of historically accepted AOT/comptime inputs; `jet fix --edition 2027`
  adds allowances and audits irreconcilable legacy data.
- **D-TEXTUNICODE1=A**: `core.text` owns Unicode algorithms: normalization,
  case folding, segmentation, width, classification, and UTF-8/scalar helpers.
  `String` stays small; tooling may insert `core.text` calls from String
  contexts. Epoch 3 ships `core.text` helpers for NFC/NFD/NFKC/NFKD, casefold
  and caseless compare, grapheme/word/sentence slices, terminal display width,
  Unicode classification, scalar/byte counts, split/trim/pad/center,
  prefix/suffix combinators, char-index views, and named scalar inspection.
  Locale collation is an i18n
  data problem and is not part of v1 core.
- **D-HUMANFMT1=A**: `core.fmt` owns human-readable formatting as ordinary
  library calls, with no interpolation sublanguage. Epoch 3 ships thousands
  numbers, fixed decimals, percents, SI bytes, compact durations, ordinals,
  plural phrases, and width padding helpers.
- **D-REGEXENGINE1=A**: `core.regex` is RE2-class and linear by default,
  including captures, named groups, replace, split, flags, and Unicode
  classes. It is std-only in the generated prelude; the old bootstrap `regex`
  crate bridge is retired. Any PCRE/backtracking compatibility is explicit and
  never the default.
- **D-NETSOCKET1=A**: `core.net` exposes typed blocking-looking
  TCP/UDP/Unix/DNS/TLS APIs over handles compatible with the task runtime, so
  deadlines, cancellation, readiness, and high-concurrency serving stay one
  socket model. String entrypoints remain the beginner path; `IPAddr` and
  `SocketAddr` are the expert/control path over the same semantics.
- **D-NETDNS2=A**: ordinary IP resolution delegates to the host resolver and
  therefore preserves hosts files, search domains, VPNs, and enterprise
  policy. `DNSResolver.at` is the one expert wire-resolver escape hatch. It
  uses unpredictable transaction IDs, validates sender/header/question and
  every packet bound, follows bounded compression and CNAME chains, retries a
  truncated UDP answer over bounded TCP, and never invents a public resolver.
- **D-NETIO1=D**: TCP, Unix, and TLS streams are byte-canonical and conform to
  the same reader/writer operation contract as file and codec streams; checked
  UTF-8 helpers project over those bytes. UDP remains packet-oriented and
  reports source, original length, and truncation. Half-close is explicit;
  close is idempotent; later misuse returns `.Closed`.
- **D-NETIO-CONTRACT1=A / D-NETIO-CONTRACT2=B**: `core.io.Reader` and
  `core.io.Writer` are the one nominal byte-stream contract. Both use write
  receivers and return `IOError`; `read(limit)` requires a positive limit,
  returns at most that many bytes, and reserves an empty success for clean EOF.
  `write(bytes)` may report a positive prefix; zero for nonempty input is an
  error; `write_all` is the one looping implementation. Close stays inherent
  and idempotent, never a trait member. File, TCP, Unix, TLS, and explicit codec
  adapters convert their native failures into the closed `IOError` tree. TLS
  byte and lifecycle methods expose that shared tree directly; other native
  byte methods keep their network-specific result types.
- **D-IOERROR-TREE1=A**: every `core.io.Reader`/`Writer` adapter returns one
  closed `IOError` tree: `InvalidInput(IOContext)`, `NotFound(IOContext)`,
  `PermissionDenied(IOContext)`, `TimedOut(IOContext)`,
  `Cancelled(IOContext)`, `Closed(IOContext)`, `Protocol(IOContext)`, or
  `Other(IOContext)`.
  `IOContext` has `operation: IOOperation`, `resource: String?`,
  `os_code: Int?`, and `cause: String?`. `IOOperation` is exactly `Read`,
  `Write`, `Flush`, `Connect`, `Accept`, `Close`, `Resolve`, or `Codec`.
  Clean EOF remains an empty successful read; zero limits are
  `InvalidInput(.Read)`. Native file/network errors preserve stable kind,
  operation, resource, OS code, and owned cause. Display stays concise; no
  compatibility constructor or flat string adapter survives.
- **D-NETERROR1=A**: every fallible network operation returns the structured
  `NetError` family. Stable variants cover invalid input, permissions,
  address/connection state, closed handles, timeout, cancellation,
  unsupported operations, DNS, TLS, protocol errors, and other OS failures.
  Operation/address/name are stable data; an OS code is optional audit data.
- **D-NETTASK1=A**: blocking-looking calls on the one socket-handle family
  observe the current `#Context`, yield through the shared runtime where
  available, and obey the earliest context, persistent socket, or explicit
  per-call deadline. The same handles expose readiness; cancellation and
  expiry are distinct `.Cancelled` and `.Timeout` values. **Implementation
  status:** TCP reads and writes use nonblocking handles plus the shared AOT
  readiness backend. On Unix, UDP send/receive and Unix-socket accept/read/write
  use the same scheduler park slots; blocked operations return typed
  `.Cancelled` or `.Timeout`, UDP preserves datagram atomicity, and Unix streams
  preserve the shared byte/half-close/idempotent-close contract. TLS consumes
  that same nonblocking TCP handle: handshake, read, write, write_all, and
  close-notify park through the shared readiness slots, preserve inherited
  socket deadlines, and return typed cancellation or timeout. Same-handle TCP
  readiness and deadline bounding exist. Per-call deadline coverage, JIT parity,
  Windows IOCP, and remaining platform proof stay tracked by #300;
  #306's shared cancellation/runtime prerequisite is complete.
- **D-NETTLSSTREAM1=A**: `core.tls.client` consumes a connected `TcpStream` and
  returns a `TLSStream` with the same byte and close law. Safe defaults verify the server name
  with system roots. The stream uses shared socket readiness, deadlines, cancellation,
  explicit close-notify, underlying write half-close, and idempotent close.
  `ClientConfig.default().with_alpn(protocols)` and the ratified labeled
  `client(..., server_name:, config:, deadline:)` path are implemented. ALPN
  validation is fallible at config construction, before the stream is consumed.
- **D-NETTLSCONFIG1=A**: validated opaque `RootCertificates` and secret
  `ClientIdentity` values configure `.System`, `.SystemPlus(roots)`, or
  `.CustomOnly(roots)` trust, optional mTLS identity, and inclusive `.Tls12` /
  `.Tls13` bounds on immutable `ClientConfig` values. Invalid PEM, mismatched
  keys, reversed bounds, and invalid ALPN fail before network use; private key
  storage is wiped on drop and never appears in errors.
- **D-NETTLSPEER1=A**: a verified handshake retains immutable peer identity.
  `stream.peer_identity()` exposes the verified server name, the complete
  certificate chain as exact DER in wire order, leaf certificate/SPKI SHA-256,
  DNS SANs, validity bounds, subject, and issuer without re-handshaking.
- **D-NETTLSCLOSE1=A**: `stream.close_write(deadline:)` sends and flushes
  close-notify, then closes only the transport write side. Repetition is
  harmless, later writes return `.Closed`, reads continue, empty bytes mean a
  verified peer close-notify, and raw EOF is `IOError.Protocol` with operation
  `.Read` and the TLS truncation cause.
- **D-HTTPDEPTH1=A**: `core.http` owns Client, Server, Router, middleware,
  streaming bodies, forms/multipart, cookies, redirects, timeouts, TLS policy,
  and SSE, built on `core.url`, `core.mime`, and `core.net`. WebSocket support
  has its own top-level home, `core.ws`, per D-WS1=B.
- **D-HTTP-ROUTE-SYNTAX2=A**: route-pattern Strings use `:name` for one
  decoded path segment and a final `*name` for zero or more remaining decoded
  segments. Names match `[A-Za-z_][A-Za-z0-9_]*` and are unique. Static
  segments beginning with either marker use URL percent encoding. Bare `*`,
  brace forms, duplicate names, encoded slashes, dot traversal, and malformed
  percent/UTF-8 input are rejected; no compatibility alias remains.
- **D-CRYPTO-SUITE1=A**: beginner crypto APIs are safe envelopes
  (`seal`/`open`, `sign`/`verify`, password hashing, key agreement, file
  envelope, `Secret`/`Key` types). Expert primitives live under
  `crypto.expert` with explicit algorithm choice and audit surface.
- **D-DBMIGRATE1=A**: the canonical database path is parameterized SQL with
  typed row decoding, transactions, prepared statements, migrations, and
  checksums. Query builders may generate the same inspectable SQL/parameter
  plan.
- **D-LOGTRACE1=A**: `core.log` records typed events, spans, and fields as
  source truth; text, JSON, and OTel are output sinks. Expert controls cover
  propagation, sampling, redaction, trace IDs, and export policy. Epoch 3
  ships typed `LogField` builders, `LogSpan` enter/close, stderr/text/JSON,
  JSONL file sinks, OTLP-file export, sampling, redaction, and counter fields.
  Sink buffering is classified by the [Bounded buffering law](spec.md#bounded-buffering-law).
- **D-ITERTOOLS1=A**: one lazy `Iterable`/`Iterator` model powers the deferred
  collection adapter plane. Concrete `List`/`Map`/`Set` `map` and `filter`
  are eager under the later D-CORE-EAGER1 amendment; `.lazy()` enters this
  plane, and materialization is explicit via `collect`, `to_list`, or reducers.
- **D-TASKRUNTIME1=A** *(amended by D-CONC-SPAWN1=D,
  D-CONC-CHAN1=A, and D-CONC-CHAN2=D)*: structured task scopes remain the
  lifetime boundary. The current surface is the nested `task` family,
  builtin `channel<T>()`, `loop value, receiver`, and a subjectless `if`
  readiness table on plain endpoints. Timers, deadlines, cancellation, and
  readiness waits use typed values and the shared scheduler laws. Bounded
  channel pressure is classified by the [Bounded buffering law](spec.md#bounded-buffering-law).
- **D-DATAFRAME1=A**: `core.data` exposes typed `Table`/`Series<T>`, schema,
  typed rows, lazy query plans, joins, windows, missing values, and plotting.
  Eager helpers and lazy plans share the same operations. Current shipped floor:
  typed CSV rows, typed JSON array-of-object rows (`data.json<T>`),
  `Table<T>`/`Series<T>` wrappers, type-driven `data.schema`
  (empty tables still report the row model; series always expose one `value`
  column), `LazyFrame<T>` plans with deferred typed filter/sort plans with
  explicit collect and plan audit output, optional-series missing counts,
  typed-lambda eager `filter`/`sort_by`, group stats, stable typed inner/left
  joined rows, pivot sums, rolling means, distribution summaries, and
  deterministic text/SVG plots.
- **D-STDLIBLEDGER1=C**: Core docs track built modules only. Missing domains
  are implicit; Jet does not maintain a have/have-not ledger of unbuilt or
  declined stdlib domains.

**Framework-lessons Core wave (ratified by owner 2026-07-12, card #506):**

- **D-AUTH1=A**: `core.auth` batteries — sessions (signed rotating
  cookies; httponly/secure/samesite defaults), password login (argon2id
  via the crypto suite), OAuth/OIDC client, email magic links, JWT/PASETO
  verification. `app.auth(users: db)` is the magic default; every knob
  expert-overridable; secrets carry `#Credential`; policy
  may require stronger factors.
- **D-AUTH2=A / D-AUTH-TOKENPOLICY1=A** *(ratified 2026-07-18)*: token
  verification ships as strict standalone typed `core.auth` functions before
  the application graph. `verify_jwt(token, key:, audience:, [issuer:,
  clock_skew:])` accepts HS256 only and requires valid `exp` and `aud` claims.
  `verify_paseto(token, key:, audience:, [issuer:, clock_skew:, footer:,
  implicit:])` accepts PASETO v4.public only, verifies Ed25519 over PAE, and
  applies the same claims policy. Optional `nbf` is checked with the same
  signed NumericDate policy; `iat`, `nbf`, and `exp` remain exact integer
  claims, while `clock_skew` keeps exact nanoseconds. Unknown algorithms,
  versions, and purposes fail closed. Both return `Result<Claims, AuthError>`;
  `Claims.audience` preserves the validated audience for downstream
  authorization. Future `app.auth` reuses these functions rather than
  creating another mechanism.
- **D-SYNC1=A**: `core.sync` CRDT value types — `SyncText`,
  `SyncMap<K,V>`, `SyncList<T>`, `SyncCounter`; `#Codable`,
  deterministic merge, ride the live-query channel via
  `app.sync(doc, over: session)`; offline edits merge conflict-free on
  reconnect; expert access to merge metadata.
- **D-DBPOLICY1=A**: typed row policies —
  `db.policy<Ticket>((user, row) => …)`; enforced below app code on
  every query/mutation/live-query path: provable satisfaction at compile
  time where the effect machinery can see it, generated runtime filter
  otherwise; active policies appear in audit output.
- **D-ENVHOOK1=A**: `jet env hook <shell>` prints an opt-in shell hook;
  entering a directory with `env.jet` activates (first activation of an
  untrusted env prompts per the D-JPK-GRANTCMD1 trust law), leaving
  deactivates; `JET_ENV_DISABLE` escape.
- **D-OBSERVE-LIVE1=A**: `jet inspect live <target>` — live task tree,
  channel depths, deadlines, effect activity, arena/GC stats; attaches
  to a `jet dev` session or an `--observe`-enabled process; a viewer
  over the existing observability rails (no new fact producer); the same
  facts feed Canvas's proof rail.

**Filesystem & time**: typed `Path` (`from`/`join`/`parent`/`extension`/
`stem`), `write_atomic()`, lazy cycle-safe `walk()` (D-PATHFS1 shipped).
`core.files` now ships D-FSOPS1 depth: `Stat`, `WalkEntry`, `TempDir`,
`TempFile`, `FileLock`, recursive `walk`/`glob`, metadata, create/remove tree,
copy tree, symlink/readlink/hardlink, temp handles, advisory lock files,
canonical/absolute, offset bytes, fsync, and atomic writes (#288). Watch APIs
follow the owner's D-WATCH-SCOPE1 comment: `core.watcher` owns file, process,
and port watch handles with `WatchEvent` values plus callback methods through
`core.event`. `fs.list_dir => [DirEntry]` (D-LSDIR1). Civil time uses
Instant/DateTime/LocalDate/LocalTime/Duration/Period/Zone/ZonedDateTime over
IANA TZif zoneinfo, layered on the injectable `Clock` (D-TIMEDEPTH1 +
D-TIME-CALENDAR1; #295). PRNG
`core.random` (SplitMix64, seedable) vs CSPRNG `core.crypto.random`
(D-RANDSPLIT1); both carry `Rand`.

**Crypto**: misuse-resistant `seal`/`open` + `sign`/`verify` defaults. The
ratified raw surface is `core.crypto.expert.{xchacha20poly1305_seal,
xchacha20poly1305_open,aes256gcm_seal,aes256gcm_open,ed25519_sign,
ed25519_verify_strict,x25519_raw,hkdf_sha256_raw,argon2id}` plus the explicit
`secret_bytes`, `signing_key_bytes`, `x25519_secret_bytes`, and
`shared_secret_bytes` exposure functions (D-CRYPTO-API1). Every call requires
an audited `#Unsafe` region (D-CRYPTOENV1, E0510/E0511). Secret-bearing values
are move-only and cannot use ordinary equality, printing, interpolation,
reflection, hashing, or serialization; use constant-time operations or an
explicit expert exposure instead. Versioned `JETC` envelope headers give
algorithm agility; PQ algorithms later (D-PQCRYPTO1). `core.encoding`
hex/base64 + `core.uuid` v4/v7 (D-UUIDENC1).

**D-CRYPTO-ENVELOPE2=A — recipient JETC v2 files** *(ratified by owner,
card #302)*: safe file crypto is
`crypto.file_seal([X25519PublicKey], Path, Path)` and
`crypto.file_open(&X25519SecretKey, Path, Path)`. Both stream bounded,
authenticated 1 MiB chunks through the canonical recipient JETC v2 format;
sealing snapshots and revalidates the source before its four independent RNG
requests, and neither operation overwrites or exposes partial output. Safe open
accepts v2 only and collapses attacker-controlled parse, recipient, and auth
failures to `FileCryptoError.OpenFailed`. Legacy v1 is confined to the ratified
expert `open_v1`/`migrate_v1` path under `#Unsafe`; no v1 writer exists.
`crypto.file_inspect` is parse-only and never authenticates. Secret material
and plaintext staging buffers are zeroized on every exit. The exact format,
nonce/AAD domains, parser caps, atomic publication rules, cancellation points,
and platform primitives are normative parts of the decision, not replaceable
aliases or whole-buffer facades.

**D-CORE-NUMERIC1=A — one math home** *(ratified by owner 2026-07-12, card #512)*: `BigInt` and `Decimal` move into `core.math`; `core.numeric` leaves the registry (ordinary unknown-module error). Construction spellings, the no-auto-promotion law (E0130–E0133), and lint L0504 are unchanged.

**D-API-LEN1=A — Law 1 blessed vocabulary** *(ratified by owner 2026-07-12, card #513)*: the API rubric keeps its plain-English rule; `len` joins a closed blessed-abbreviation list (with the module names `fmt`, `args`, `env`, `mem`); extensions to the list need a ballot. The shipped `len()`/`.len` surface is untouched.

**D-API-CONTAINS1=B — membership is `has`** *(ratified by owner 2026-07-12, card #513; owner picked B over the rec)*: the membership word is `has` everywhere — `Set`/`SortedSet`/`BitSet` `contains` respells to `has(value)`, map/`Cache` `contains_key` respells to `has_key(key)`, `Bag.has` is already law. `contains`/`contains_key` leave the surface as ordinary no-such-method errors. Amends the D-COLLBREADTH1/D-ITER method lists.

**D-API-CTOR1=A — constructor-idiom law** *(ratified by owner 2026-07-12, card #513)*: the four shipped idioms become written rubric law — bare `Type(…)` when the arguments ARE the value's components (fallible where narrowing); `.new(…)` for fresh stateful containers; `.over(…)` for non-owning views over existing data; `.from_*(…)` for conversions. `Type.{ }` stays the literal for plain data records. Nothing shipped changes; new construction shapes need a ballot.

**D-SHAPE-CTORVERB1=C — one fresh-value completion stem** *(ratified by
owner 2026-07-23, card #736)*: deterministic fresh values use type-owned
`Type.new(…)`; constructors that draw entropy use `Type.new_random(…)`.
`Clock.new`, `ExpiringValue.new`, `SigningKey.new_random`, and
`X25519SecretKey.new_random` are the canonical spellings. The former
`time.clock`, `expiring.new`, and key-type `generate` spellings retire with
teaching errors. The plain `random` suffix does not extend the closed
abbreviation list.

**D-SHAPE3a=A — inferred fresh construction** *(ratified by owner 2026-07-14,
card #536)*: `.new(…)` may omit the receiver only when the surrounding expected
type plus its arguments determine one receiver type. `Type.new(…)` always remains
available. Elaboration reuses ordinary expected-type inference and the existing
static-call path; there is no constructor registry or global search.

**D-SHAPE-DURATION1=A / D-SHAPE-DURATIONCONVERT1=A — checked runtime
durations** *(ratified by owner 2026-07-14, cards #558/#575)*: a runtime `Int`
or `Float` becomes a duration only through the type-owned closed family
`Duration.milliseconds/seconds/minutes/hours(value)?`. Scaling rejects overflow
and non-finite floats; fractional milliseconds truncate toward zero. Whole-unit
reads use only `duration.in(.Milliseconds/.Seconds/.Minutes/.Hours)?`, return
`Int ? RangeError`, and truncate toward zero. The former `core.time` free
constructors and per-unit readers leave the surface without aliases. Static
unit literals remain unchanged.

**D-SHAPE-OPAQUE-INFER1=A — hidden generic constructor arguments** *(ratified by
owner 2026-07-14, card #568)*: `Type.new(…)` may omit the receiver's generic
arguments when its inputs and surrounding expected type determine exactly one
substitution. Empty or conflicting evidence is an error; write the full
`Type<Args>.new(…)` receiver to pin it. This is the ordinary one-way generic
solver, including nested types, bounds, and privacy—not a constructor registry,
alias, or priority rule.

**D-PRELUDE-LAW1=A — ambient-surface registry** *(ratified by owner 2026-07-12, card #514)*: the no-prefix surface is one closed list — always ambient: `print`, `input`, `panic`, `require`; comptime-gated ambient: `embed_file`, `embed_bytes`, `find`, `fetch`. User shadowing wins; libraries never inject (D-PRELUDEX1). Any addition or removal is a ballot.

**D-ARTIFACT-EXT1=A — one artifact-extension family** *(ratified by owner 2026-07-12, card #514)*: every Jet tool artifact is `.jet<kind>`: `.jetmap`, `.jetnb`, `.jetproof`, `.jettrace`, `.jetreplay` (game input replays), and `.jetproof-replay` (proof replays). The former short-prefix family and replay collision are retired without aliases. Closed family; new artifact kinds need a ballot. Amends D-JPROOF1/D-JREPLAY1/D-PERFSESSION1/D-GAME-REPLAY1 spellings.

**D-API-STORE1=A — one storage verb: add / add_new** *(ratified 2026-07-12, card #513; shape set by owner question q2zvcuql)*: `insert` and `put` die. Keyed containers: `add(key, value) => T?` upserts and returns the displaced old value (`None` = fresh key); `add_new(key, value) => Bool` stores only if absent — `false` means the key existed and the value is untouched (the race-safe claim). Element containers: `add(value) => Bool` (`Set`/`SortedSet`: true if newly added; `Bag`: always true). `m[k] = v` index-write stays the literal upsert (S39). Enters Law 1; amends the map/`Cache`/D-COLLBREADTH1 method lists.

**D-CACHENAME1=A — bounded cache is `Cache<K,V>`** *(ratified 2026-07-31, card #1356)*: rename the Core type formerly spelled `Lru<K,V>` to `Cache<K,V>`. Eviction remains least-recently-used when full; method law unchanged (`has_key`, `add`, `add_new`, …). Amends D-COLLBREADTH1 / D-ITERTOOLS1 naming.

**D-MAP-MERGE1=E — `Map.merge`** *(ratified 2026-07-31, card #1354)*: one method `merge(other, conflict: ((K, V, V) => V)? = None)`. Omit `conflict` → right wins on shared keys (beginner default). Pass `conflict:` → callback result per shared key. Distinct from `Set.union` and struct Patch `merge`. Semantics live in Prelude (`jet_map_merge` / `jet_map_merge_with`); engines marshall only (I9).

**D-LISTREMOVE1=F — value-first list removal** *(ratified by owner, card #1410)*:
`List.remove(value)` removes the first equal item and returns `T?`; explicit
`.Val` spells the same behavior. `List.remove(index, .Slot)` removes by
position and retains the existing bounds diagnostic. The old one-argument index
meaning is retired, and every in-repo caller uses `.Slot` where positional
removal is intended. `RemoveBy.{Val, Slot}` is the closed selector type; no
parallel `remove_value` or `remove_at` names ship. Map removal is unchanged.

**D-FIELDDEF1=C — field defaults use `=`** *(ratified 2026-07-31, card #1367)*: `field: T = expr` covers wire/CLI absence and omitted `Type.{ … }` construction fields (same spelling as parameter defaults, S61). `#Default(expr)` is retired (E0375); defaults must be compile-time constants (E2414). Required fields are those without `=`.

**D-FMTCOLLAPSE1=B — collapse every fitting braced block** *(ratified 2026-07-31, card #1368)*: any `{ … }` whose contents fit the width budget becomes one line — control bodies, arms, function bodies, and marker regions. Comments or over-width contents stay multiline. Amends S44 / #1335.

**D-LOOPMAP1=B — eager in-memory adapters; `.lazy()` for pipelines** *(ratified 2026-07-31, card #1325)*: `List`/`Set`/`Map` `.map`/`.filter` return collections. Opt into the lazy Iter plane with `.lazy()` before chaining adapters that should not build middle lists. Iterator sources (files, channels, string split) stay lazy.

**D-LOOP-HEADER3=D — one three-slot header meaning** *(ratified 2026-07-31, card #1325)*: slots are binding; source; step rule. Int strides and range+step-rule share that dialect. The C-style counter form retires with a teaching diagnostic.

**D-LOOPLABEL4=A — binding-as-label stays** *(ratified 2026-07-31, card #1325)*: labeled loops keep binding-as-label for both value and effect loops.

**D-TASK-PAUSE-TIER1=E — wait-point pause; optional strong mode** *(ratified 2026-07-31, card #1359)*: default pause is cooperative wait-points on every engine. Experts may pass `mode: .CheckLoops` for strong pause. Trace `paused=`/`cancel=` text comes from one Prelude formatter (`jet_task_control_trace`).

**D-VALIDATE1=A — validation in the struct definition** *(ratified 2026-07-12, cards #506/#513; shape set by owner direction)*: a `validate { … }` section in the struct body (S82 in-body grammar) declares rules as dot-chains on bare field names (D-FIELDPOL1 sibling access); cross-field rules use `check(cond, at: field, "msg")` in the same block. All rules ACCUMULATE into `[FieldError]` (`{ path, reason }`). `decode<T>()` runs the block automatically; `Type.validate(value)` runs it standalone. `Validate.over(s)` is the sole use-site escape, same rule vocabulary and engine (I8), only for rules needing context the definition cannot see. Type-level constraints (D-RANGETYPE1, D-REFINE1) remain layer zero. `#Pre`/`#Post` stay call-site contracts, outside the validation story.

**D-VALIDATE-DECODE1=B — one accumulated typed-decode error contract** *(ratified 2026-08-03, owner ballot; cards #1158/#1161)*: every `Decode` implementation and every format adapter returns `Result<T, [FieldError]>`. A `FieldError` is `{ path, reason }`; nested decoders prefix every error in the list with their field or index segment. This is the same list used by `validate` blocks. The retired single `DecodeError` envelope has no alias, bridge, fallback, compatibility flag, or second decoder API. XML/CBOR/native adapters may project the list into their format-specific error type only at that boundary; typed Jet callers always receive `[FieldError]`.

**D-CORE-SECRETS1=A — one secrets home** *(ratified by owner
2026-07-12, card #509)*: `core.vault` owns secret storage AND lifecycle
(rotation schedules, expiry, audit facts); `core.secrets` leaves the
registry (ordinary unknown-module error). Generic TTL wrapping stays in
`core.time.expiring`; `core.crypto` stays primitives and envelopes. The
teachable rule: crypto moves bytes, vault keeps secrets.

**D-CRYPTO-VAULT1=A — typed persistent key lifecycle** *(ratified by owner
2026-07-11, card #302)*: `core.vault` stores sealed `SigningKey` and
`X25519SecretKey` generations in the existing recipient-encrypted repository
vault. Immutable `KeyRef<T>` values identify exact versions. Mutation requires
prepared plans, exact-preview one-use authority, and consuming commits;
rotation, retirement, and logical revocation never retarget an old ref. The
  canonical `JVLT` v2 payload, redacted closed `VaultError`, and native atomic
durability law are part of the API contract. Portable backup remains solely
owned by D-CRYPTO-KEYWRAP1 and is absent here.

**D-CRYPTO-KEYWRAP1=A — authenticated portable vault backup** *(ratified by
owner 2026-07-20, card #302)*: `core.vault` exports type-distinct `SigningKey`
and `X25519SecretKey` generations as canonical `JVKW` v1 envelopes, either to
1–16 X25519 recipients or a fixed-policy Argon2id passphrase. The authenticated
header binds repository, name, generation, opaque identity, record hash,
public key, and concrete key type. Import is safe only through
`prepare_import_wrapped<T>` → `authorize_wrapped_import<T>` →
`commit_import_wrapped<T>`; same-origin exact imports are idempotent, revoked
origins never reactivate, cross-repository copies receive a new identity, and
secret-dependent unlock/tamper/type failures collapse to `OpenFailed`.

**D-CORENS2=A — core namespace admission law** *(ratified by owner
2026-07-12, card #509)*: a new top-level `core.<name>` requires a
domain — a coherent problem area with a plausible member family —
ratified by ballot; features join an existing domain. Applied today:
`core.devserver` → `core.web.devserver`, `core.async.loadable` →
`core.reactive.loadable` (clean breaks, ordinary unknown-module
errors).

**D-CORE-COMPRESS1=A — compression split by job** *(ratified 2026-07-11,
card #499)*: `core.compress` owns stream codecs (`gzip`, `zstd`, future
additions); `core.archive` owns container formats (`zip`, `tar`; `tar.gz`
composes archive over compress). `core.archive`'s standalone gzip helpers
move to `core.compress` — clean break, no re-export. Supersedes the
overlapping gzip rows of D-DEP-ARCHIVE1/D-CODECS1.

**Numerics & data**: `core.linalg` ring package — `Vec2/3/4`, `Mat3/Mat4`,
`.dot()`/`.cross()`/`.matmul()` as aliases over a generic `Vec<N>`/
`Matrix<M,N>` substrate (const-generic substrate tracked by #293) (D-MATHLIB1,
D-LINALG1). `core.db`: backend-neutral `Driver` trait, parameterized-only
API, SQLite first; explicit `.begin/.commit/.rollback` distinct from
`#Transact` (D-DBDRIVER1). D-DBMIGRATE1 ships the hybrid database floor:
checked `SQL` literals feed `db.params(sql)`, rows stay inspectable maps with
typed `db.row_*` reads, and `db.transaction`/`db.migrate` provide rollback and
checksum-recorded migration helpers over the same parameterized path. `core.http`: client+server submodules; client
supports HTTPS by default via rustls + system roots (D-TLS1=A); server is
plain `fn(req: Request) => Response` on a `mux` (`mux.get("/users/:id", handler)`,
`req.params["id"]`, `Server.serve(addr, mux)?`) with HTTPS enabled by the named
option `Server.serve(addr, mux, tls: Server.tls(cert, key))` (D-TLSSERVE1=A).
HTTP route parameters use `:name` and final catch-alls use `*name`
(D-HTTP-ROUTE-SYNTAX2=A). HTTP/1.1 depth is tracked by #301; HTTP/2 remains a
separate transport upgrade, and WebSocket belongs to `core.ws` per D-WS1=B
(D-HTTPLIB1–3, D-ROUTE1).
Compression
`core.compress.{gzip,zstd}` (D-CODECS1). Measurement-with-uncertainty in
`core.science.measurement` (D-HONESTNUM1 shipped/partial; #310 verifies module
map truth). **D-OPTGC1=A — scoped opt-in tracing GC** *(ratified 2026-07-15,
card #646)*: GC is magic inside an opted scope, using D-MARK-SCOPE1 from
package through module, function, and block. Heap-owning values gain traced
identity without `Gc<T>` wrappers; a bare store creates a traced edge, `&`
still gates mutation, `^` transfers a root, and `~` deep-copies. An unproved
escape into ownership-only code is rejected. `jet gc report` lists each
allocation ownership could not prove, with source, reason, and a concrete path
back to owned values or `Pool<T>`/`Id<T>`; removing the opt-in is the supported
end state. The collector is D-DEP-GC1=A's pure-Rust, std-only mark-sweep engine;
it is an internal substrate, not a second source mechanism. Option C's explicit
`core.gc` / `Gc<T>` handle surface is retired; scoped automatic promotion is
the one GC path (I8). Nested stores and later mutations update deterministic
collector edges, including cycles, while source values remain bare. An external
collector still needs a separate I6 ballot.
Approximate/sketch algos
are libraries (D-APPROX1). **D-PARCAPTURE1=D** *(ratified 2026-07-20,
card #392)* supersedes D-AUTOPAR1's provisional spelling: the sole explicit
parallel collection family is the clean-break `para_map`, `para_filter`,
`para_partition`, and `para_fold`; no `par_*` aliases remain. All four share
one bounded indexed chunk engine. Map/filter preserve source order; partition
returns source-ordered `(false_, true_)`; fold takes
`(seed_factory, step, merge)` and uses stable chunks plus a deterministic merge
tree. The seed is the two-sided merge identity and merge is associative. If
callbacks fail concurrently, the original failure at the lowest source index
wins; no partial result or accumulator escapes, while external effects already
performed are not rolled back. Sema rejects mutable captures, function-typed
items/results/accumulators, and other non-shareable/non-transferable worker
values as E1111; stored/imported callbacks are rejected when their capture facts
are unavailable, rather than silently serializing or inventing a capture merge.
Adaptive fidelity is a manual runtime-global knob:
`core.perf.Perf.fidelity()`, `default_fidelity()`, `override_fidelity(v)?`,
and `reset_fidelity()` (D-FIDELITY-API1=A). No automatic adaptive scheduler or
platform-signal providers ship in Epoch 3 (D-ADAPTRT1=C,
D-ADAPT-PROVIDER1=A).

**Reactive, events & UI stack** *(D-REACT1, D-REACTCORE1, D-SIGNAL1, D-EVENT1,
D-DATARACE1=C, D-RENDERTGT1/2, D-UITREE1, D-STYLESHAPE1, D-MOTIONTIME1, D-LAYOUT1,
D-LAYOUT-CTOR1, D-OWNCOMP1, D-A11Y1, D-NATIVEUI1/2)*: reactivity is a library + explicit
`#Reactive` scope marker (E2914) lowering onto `core.reactive` — `Signal<T>`
(`.get()/.set(v)`), `Computed<T>`, `Effect`; explicit-by-read subscription;
pure std runtime (E2910–E2913). D-DATARACE1=C: reactive boxes use lock-ordered
`Arc` storage so task/channel crossings cannot data-race or lean on rustc
`Send`. Expert pins `#Local` / `#Shared` are registered (MARKER_LOCAL /
MARKER_SHARED): `#Local` rejects task/channel/parallel crossings (E1102);
`#Shared` and automatic boundary crossings list synchronized-form upgrades
in generated Rust (`/* jet-reactive-upgrade: … */`) and `jet report`. Events and hooks are compiler-known Core values
in `core.event`: `Event<T>`, `Hook<T, R>`, `Subscription`, `EventScope`,
`EventPolicy`, and `EventTrace`. Render backends implement measure/layout/paint
(`JetBackend`; `NullBackend`/`TuiBackend` shipped). UI trees are typed dot-construction
(`.Button.{ label: "OK" }`); `Style` is one flat record; motion uses the
injectable `Clock`; constraint layout is `name :: Layout.{ … }` (D-DOTCTOR3
element body of `Constraint` handles; D-LAYOUT-CTOR1) with a first-party
simplex solver (E2932–E2936).
Components distribute copy-in-and-own: `jetpack add <Component>` copies
source into `./components/` (no version lock). Native UI wraps platform
widget FFI, all three desktop platforms against one trait seam *(gated)*.

**D-LIVEQUERY1=A — live queries** *(ratified by owner 2026-07-11, card
#505)*: `app.live(query, args)` accepts only a function whose effect row
is inside `DB.Read` and whose body has no effects beyond those reads;
anything else is a compile error naming the offending effect. Sema
records the query's read footprint; a committed `#Transact` whose write
set intersects a live footprint invalidates exactly those subscriptions,
re-runs them, and pushes results over `core.ws` into a client
`Signal<T>` (D-REACT1). No invalidation keys exist. Runs in `jet dev`
and any self-hosted server — no platform dependency. Expert floor stays
public: `app.subscribe`/`app.invalidate` for sources outside the
tracker, and an `every:` interval option for untracked queries.

**D-EFFDBREAD1=A — how a live query proves it only reads** *(ratified by
owner 2026-07-12, card #505)*: the compiler's own closed `core.db` method
table infers effect **leaves** — `conn.query`/`conn.query_one` carry
`DB.Read`, `conn.execute` carries `DB.Write` (arbitrary DDL/DML), and the
transaction-control/`close` calls (`begin`/`commit`/`rollback`/`close`)
keep the plain `DB` root, since they neither read nor write rows
themselves. This is the one exception to D-EFFTREE1's rule that a real
Core call is only ever tagged with a bare root (leaf precision otherwise
being a user-declared-contract concept): the shape is rustc special-casing
a small closed list of known intrinsics, and it touches only the finite
table the compiler already keeps for its own stdlib signatures — inference
through ordinary user calls stays bare-root, exactly as D-EFFTREE1
decided. A read-only query function can therefore *prove* `=[DB.Read]=>` —
the read-footprint qualification `app.live` demands — and a write hiding
inside such a function is caught by the existing `E0740` check (no new
diagnostic code). *Reconciliation:* D-LIVEQUERY1's `inWild` stacked
`#Pure` on top of `#(DB.Read)`; D-SHAPE8 later retired both spellings, so a live
query now qualifies by its `=[DB.Read]=>` bound alone. `DBConnection` (and
`DBError`) are now nameable types so a query function can annotate its
connection parameter.
*Shipped 2026-07-12 (card #505, slice 4 — leaf-inference layer only)*: the
`DB.Read`/`DB.Write` leaf inference above, the `=[DB.Read]=>` qualification
proof, the `E0740` hidden-write reject (`tests/ui/db_read_query_hidden_write`),
and `DBConnection`/`DBError` nameability, with a runnable example
(`examples/features/io/db_read_footprint.jet`). The remaining `app.live`
legs — the `app` namespace / app graph (D-WEBAPP1, card #438), the
`core.ws` push transport (D-WS1; a native std-only WebSocket, I6 forbids a
crate), the client `Signal<T>` binding, and the `#Transact` write-set
publication + invalidation scheduler — ride unbuilt infrastructure owned
by cards #438 and #134 and are not yet implemented.

**Web target** *(D-WEBKIND1, D-DOMGEN1, D-WEBBACKEND1, D-OSTARGET1,
D-WEBDEFAULT1, D-HTMLPAIR1)*: browser target is `wasm32-unknown-unknown` +
generated JS loader; DOM work goes through a tiny first-party `JetDom` shim
(no vdom); hybrid: view emits JS, compute may compile to WASM. `#Target(…)`
takes `Web`/`Browser`/`Wasm`/`JS` and `OS.Linux`/`OS.MacOS`/`OS.Windows`
(mixing web+OS on one item rejected). Default target: CLI `--target` >
`package.jet` `target:` > file marker. `#HTML("path.html")` names a companion
page (explicit > sibling `<stem>.html` > generated; missing path = build
error). `OS.*` gates a single `impl` block (item-scoped), not a file/module —
`E-OSTARGET-MIXED-AXIS`/`E-OSTARGET-UNMATCHED-CALL` enforce it.
**D-OSTARGET2 (=B, ratified 2026-07-03, c2qj06uq)**: ungated code reaches
the surviving OS-gated impl through a comptime dispatch on `@build.os` — a
compiler-known comptime value matched with `.Linux`/`.MacOS`/`.Windows`
arms; non-matching arms are discarded before OS-gating checks run.
fn-level `#Target(OS.*)` gating (option A) rejected.
*Shipped spelling (2026-07-03):* the ballot wrote the dispatch loosely as `match
@build.os { … }`; reconciled to Jet's one canonical branching form (D-IF1/D-IF3
`if subject == { }` if-table) with the existing `@if` lead (D-WHEN1) —
**no `match` keyword was added** (I8). Statement-position dispatch:

```jet
fn run() {
    @if @build.os == {
        .Linux   -> { b :: LinuxBackend.{ name: "gtk" }    print(b.label()) }
        .MacOS   -> { b :: MacOSBackend.{ name: "appkit" } print(b.label()) }
        .Windows -> { b :: WinBackend.{ name: "win32" }    print(b.label()) }
    }
}
```

`@build.os` resolves to `ProgramBundle.active_os` (the `--target=<triple>` OS
bucket, host OS when omitted; a web/wasm target falls back to the host per
`OSTarget::active`). Sema desugars the dispatch into an `@if` chain
(D-WHEN1/D-WHEN2 machinery) as the *first* step of `check_bundle`, folding to
the arm matching `active_os` and discarding the rest before any OS-gating
check, type-check, or codegen sees a body — so constructing an OS-gated type
inside the taken arm is legal and dead arms never trip
`E-OSTARGET-UNMATCHED-CALL`. **Exhaustiveness** is build-independent: the arm
set must cover `.Linux`, `.MacOS`, and `.Windows`, or carry an `else` — missing
an OS with no `else` is `E-OSTARGET-DISPATCH-EXHAUSTIVE` (so the same source
compiles or fails identically on every platform). A non-`@build.os` subject is
`E-OSTARGET-BUILD-CONTEXT`; a non-OS arm head is `E-OSTARGET-DISPATCH-ARM`.
`@build.os` is meaningful *only* as this dispatch's subject — `build` is not a
reserved word, so anywhere else it is an ordinary identifier (undefined at
runtime → E0107), never a magic runtime value.
**D-UIDEVSHELL1 (=A, ratified 2026-07-03, c2qj06uq)**: Phase 8 native
backend toolchain deps enter via nixpkgs devShell (`gtk4` + `pkg-config`,
Linux first) per the standing native-deps stopgap; jetpack core provider
owns it long-term; non-Nix users get a clear install message. *Shipped*
(Tower c134 Phase 8): `flake.nix`'s devShell gains `gtk4` + `pkg-config`;
`core.ui.gtk_backend()` is a real, working `JetBackend` over libgtk-4 — a
retained-mode widget API: `label`/`button` create real widgets and return a
handle, `set_text`/`set_size`/`set_color` mutate a live widget, `on_click(id,
handler)` wires a button, `present(title)` opens the window and runs the GLib
loop. The flagship example is a live counter — a button click sets a
`reactive.signal` and the `ui.reactive_render` effect updates the label text
in place (the shipped reactive core, I8). Selected by the shipped `@if
@build.os` dispatch (D-OSTARGET2=B) and emitted only on a Linux target; all gtk
C-ABI calls are confined to the vetted `jet_gtk` prelude module (I1 — user
code writes no low-level tier). The native link (`-lgtk-4 …`) is named by `use
c.gtk4` through the S59 `pkg-config gtk4` path; a missing gtk4 at build time is
the existing **E3201** (names the fix — install gtk4 / add the `c@system` dep
/ enter `nix develop`). `jet run` opens the window; `JET_UI_HEADLESS=1` skips
it so tests/CI terminate. Example:
`examples/features/ui/ui_native_linux.jet`; structural + link proof in
`tests/cross.rs` (`gtk_backend_*`). macOS/Windows native backends stay out of
scope (their `comptime` arms degrade honestly).
**D-STYLEUNIT1 (=A, ratified 2026-07-03, c2qj06uq)**: UI style lengths are
unit-family literals — `core.ui.style` declares `#UnitFamily(Length) { px }`
(D-QUAL3), so `width: 320px` is a compile-checked `Px` value via the one
ratified unit mechanism (D-UNITLIT1); no second style-only unit system (I8).
Supersedes Phase 3's interim `Length` struct pair. *Shipped* (Tower c134):
`examples/features/ui/ui_typed_style.jet` declares `#UnitFamily(Length) { px }`
and its `Style` record carries `width: Px`/`height: Px`; the interim `Length`
struct/enum pair is deleted. Landing this required closing a standing typed-IR
gap — a distinct-typed struct/enum field (`width: Px`, `Length(Px)`) was not
admitted by the TIR subset (`field_ty_covered`/`enum_payload_ty_covered`), so
any struct/enum carrying a unit-family field ICEd once the TIR became codegen's
sole path; both predicates now admit distinct types, and the distinct newtype
emits a `JetDebug` impl so a container's derived debug covers the field.
Cross-family mixing (`320px + 500ms`) is E0127, unchanged from D-DIST3.

**D-OBS1 / D-OBS3 — Observability**: source maps + Jet-line panic reports;
OTel-aligned std-only structured logs/metrics; exporters are FFI-wrapped
packages, never compiler deps.

### Manifest, packages & jetpack

**D-SHAPE5a=A — Package roles are typed fields**: each role is a named field
whose package schema fixes one role type. Its value uses the existing
expected-type record form `.{ ... }`; writing the role type explicitly produces
the same typed value:

```jet
greeter: Package :: .{
    name: "greeter"
    version: "1.0.0"
    sources: .{ roots: ["Source"] }
}
```

The exact view may pin those same role types without changing meaning:

```jet
greeter: Package :: .{
    name: "greeter"
    version: "1.0.0"
    sources: Package.Sources.{ roots: ["Source"] }
}
```

There is no package-only identity wrapper and no untyped, dotless identity
record. This decision reuses ordinary `.{ ... }` and
`Type.{ ... }` construction; it adds no `Syntax.rs` entry, token, parser
production, formatter form, editor grammar, snapshot, or executable example.

#560 owns all runtime enforcement and acceptance work: parser, sema, TIR,
formatter, hover, inspect, Canvas, templates, migration, and rejection of
unknown role fields. D-SHAPE5a does not choose file placement; the final role
inventory or its fields; merge, composition, or override law; provenance
(#578); outputs (#540/#587); or callable entry linking (#544).

**D-SHAPE5b=A — One package output is an `Output` variant**: an executable,
library, service, image, or bundle is one case of the closed `Output` sum. Each
case carries a checked named record payload. An expected `Output` type may
supply the omitted qualifier, so the compact and explicit forms have the same
meaning:

```jet
command: Output :: .Executable.{ name: "greeter", entry: run }
library: Output :: .Library.{ name: "greeter_core", modules: [Greeter] }
service: Output :: .Service.{ name: "greeter_api", entry: serve }
```

This decision reuses the existing named-payload enum construction
`.Variant.{ field: value }`. It adds no token, keyword, parser production,
formatter form, or editor grammar, so it requires no parser, grammar, or
snapshot change of its own. The package output collection and capability
inventory are implemented by #587; language-wide shape enforcement is owned by
#560. Aliases, default selection, and callable entry linking remain separate
choices and are not implied by D-SHAPE5b.

**D-ECO-DECL1=A — Ecosystem entries are normal named typed values**
*(ratified 2026-07-15, card #615)*: each package, environment, check, service,
image, fleet, and system is an ordinary named field whose value uses the
existing D-DOTCTOR1 `Type.{ field: value }` constructor. The root value has
typed sections; section-qualified names such as `packages.api`,
`services.web`, and `images.server` are stable references within that root.
The root type is `Package` (D-ECO-ROOTNAME1=I).

```text
root: <Root> :: <Root>.{
    packages: {
        api: Package.{ source: "apps/api" }
    }
    checks: {
        unit: Check.{ run: packages.api.tests }
    }
    services: {
        web: Service.{ run: packages.api }
    }
    images: {
        server: Image.{ services: [services.web] }
    }
    systems: {
        home: System.{ image: images.server }
    }
}
```

This is normative future source behavior, not an executable spelling today.
There is no per-kind parser or evaluator. Package and role declarations are
replaced only after #560 lands the source gate; ordinary Jet modules remain
ordinary modules and are unchanged. #560 owns parser, sema, TIR, formatter,
hover, inspection, Canvas, templates, migration, editor support, and acceptance
for this shape. D-ECO-DECL1 adds no keyword, sigil, `Syntax.rs` constant,
parser production, grammar form, diagnostic, snapshot, or executable example
by itself.

**D-ECO-EXTENSION1=A — Extensions are ordinary typed Jet functions**: an
extension accepts typed settings and returns a closed typed graph value. The
returned value follows the same validation, authority checks, composition law,
and provenance retention as a built-in value.

```text
conceptual flow only — not Jet syntax
typed settings => ordinary Jet function => closed typed graph value
```

There is no separate plugin language and no callback with authority to mutate
the whole graph. Extension code constructs and returns values; normal graph
composition decides how those values join other contributions.

This decision does not choose the aggregate graph's name or boundary, any
project-part type name, declaration shape, file path, or source spelling; #532
and #615 remain gates for those questions. Executable enforcement and
conformance tests remain downstream under #560 or a dedicated graph
implementation card. #611 itself adds no `Syntax.rs` entry, parser or runtime
behavior, diagnostic, grammar, snapshot, or executable example.

**D-ECO-COMPOSE2=A — Safe additions combine; disagreements stop**: composition
is order-independent and follows the field type. Equal single facts coalesce.
Unequal single facts conflict, and the diagnostic identifies both origins.
Named collections combine by key; sets union their members; ordered steps
combine only when their type defines an order. Every successful result retains
the provenance of all contributions.

Experts resolve a disagreement by using ordinary Jet functions to construct one
final typed value before contributing it. There is no last-file-wins rule and
this decision creates no override operator. Ballot examples involving
`ProjectPart`, particular paths, or `project/backend.jet` are explanatory only;
D-ECO-COMPOSE2 ratifies no source type, file layout, or source spelling.

Executable implementation and conformance tests remain downstream. #560 owns
language-wide shape enforcement; if project-graph composition is outside that
program, a dedicated implementation card must own it before this law ships.
#605 itself adds no `Syntax.rs` entry, grammar, snapshot, parser, sema, runtime
behavior, diagnostic, or executable example.

**D-SHAPE-MERGEPROVENANCE1=A — Complete successful merge history lives in
`.jet/lock`**: the unified lock is the sole primary copy of that history,
stored beside the resolved graph. For each semantic field path it retains the
final value and an ordered edge for every successful contributor and deliberate
replacement. Every edge carries its source span, operation, input value hash,
and final value hash. A failed conflict produces a Jet diagnostic and no lock
artifact.

Human explanations, signed receipts, and audit streams derive from those same
locked edges. They may render, sign, hash, or stream the history, but none is a
second authority.

The following is a conceptual generated view only. It is neither `.jet/lock`
serialization nor command output:

```text
semantic path  policy.optimization
final value    .Speed
successful edges, in order
  workspace.jet:18:5  default  input sha256:21…  final sha256:74…
  release.jet:4:9      replace  input sha256:9c…  final sha256:74…
```

This decision does not choose composition or override behavior, the source-file
model, exact lock serialization, retention or compaction beyond complete
successful inputs, an artifact for failed conflicts, signature/receipt/
generation schemas, audit transport, or inspection CLI spelling. #560 owns
implementation and every generated view. The graph, composition, and receipt
decisions on #532, #605, and #608 remain gates. D-SHAPE-MERGEPROVENANCE1 adds
no `Syntax.rs` entry, parser or runtime behavior, CLI, grammar, snapshot, or
executable example by itself.

**D-ECO-RECEIPT2=A — One connected record spans realization through
rollback**: the record connects exact inputs, planned actions, produced output
digests, activation proof, and the parent generation. A planned action is not
the bytes it produces; keeping them distinct lets Jet detect when identical
planned work yields unexpected bytes.

```text
conceptual relationships only — not a schema, file format, or CLI
exact inputs => planned action
planned action != produced bytes => output digest => activation proof
activation proof => parent generation
```

D-SHAPE-MERGEPROVENANCE1 remains unchanged: `.jet/lock` is the sole primary
copy of complete successful merge history. The connected receipt may refer to
locked inputs and their digests, but it neither replaces nor duplicates that
merge-history authority.

D-ECO-RECEIPT2 does not choose schema, serialization, file placement,
retention, a normalized-DAG shape, a freeze algorithm, or CLI spelling.
Implementation is currently fragmented across #420, #422, #424, #425, and
#431; those cards or a dedicated receipt-integration card must connect the
record end to end before this law ships. #608 itself adds no `Syntax.rs` entry,
parser or runtime behavior, diagnostic, grammar, snapshot, or executable
example.

**S52 — Files** *(D-JPK-FILES, D-JPK-FILENAME2)*: per-package manifest
is **`package.jet`** (bare `name:`/`version:` identity + `packages:` +
`deps:` + `targets:` + `effects:`); other `.jet` files contribute Configs;
`module workspace` is discovered by declaration (`members:` may run
comptime — inline lists, `find("./dir")`, or an expression referencing a
sibling `comptime`/`fn`; D-WORKSPACE1/2 — the root `jetpack.toml` index is
retired). A member is addressed three ways (D-MONOREF1=A, implemented):
dot form `source.package`, path form `infra/logging`, or the bare member
name `logging`; ambiguous/unknown bare-or-path refs are E1230/E1231. A remote
monorepo resolves **index-first**: only the addressed member's subtree (plus
its in-repo deps) is sparsely fetched (E1232/E1233 on fetch/index errors).
lockfile is the single **`.jet/lock`** (U2); `.jet/` holds only generated
state; shared store is the hangar at `/etc/jet/hangar/`. `pack.jet`,
`payload.jet`, `jet.toml` are dead names.

**D-ECO-ENV1=A — One typed environment output is the source of truth**: an
`Environment` is a package output. `jet dev`, tasks, editors, and CI are
projections of that same checked value rather than separate setup recipes.
Imperative environment actions remain possible only when capability-scoped and
audited inside the same graph.

The structural `env.jet` model described above is a stopgap. Its current
existence does not mean the environment is already unified with the typed
package graph. #587 owns the typed output collection and capability mapping;
#560 owns language-wide enforcement. Their implementation remains gated by the
project graph, composition, and receipt decisions on #532, #605, and #608.

D-ECO-ENV1 does not choose the tool inventory or `Environment` field schema,
service/task/CI mappings, hook or shell spelling, source placement or
composition, receipts, or lifecycle behavior. It adds no token, `Syntax.rs`
entry, parser or runtime behavior, grammar rule, diagnostic, snapshot, or
executable example by itself.

**U10 — Package → packages → modules**: a Package (one `package.jet`) lists its
packages in `packages: { name: … }`; a package **is** a top-level `module` —
its module name is its identity, its file is discovered by walking the tree
(exactly-one-match required). No other filename is a Package root.

**U7 — Zero-ceremony single files**: `jet run file.jet` never needs a
manifest, `.jet/`, or any ecosystem file — forever.

**D-JPK-REF1=A / U6 / U9 / D-JPK-OSHOST1 — Email-order refs**
*(ratified 2026-07-23, card #733; amends U6, U9, and D-JPK-OSHOST1)*:
package refs use `name['#'version-or-channel]['@'source]`. Thus the CLI uses
`fastfetch@nixpkgs`, a named source uses `textkit#1.2.0@vendor`, and a source
value uses `target@provider`, such as `acme/helpers@github`. Local `./`, `../`,
and `/` paths are bare; the `path` provider word retires. An old
`provider@target` ref is a teaching error with the flipped spelling. The parser
never silently treats a built-in provider name as a package name.

Versions still pin with `#`: `textkit#1.2.0` (`#` = "a pinned number", shared
with `[T#N]`). Channel refs (`#latest`, `#main`) resolve only in network-class
verbs; the lock stays exact (D-JPK-CHANNEL1). Git deps that need selectors use
inline structs (D-JPK23):

```jet
deps: {
    textkit:  textkit#1.2.0,
    helpers:  ../helpers,
    parsekit: { git: "https://github.com/acme/parsekit", tag: "v0.4.1" },
}
```

**U30 / D-JPK-TOOLCHAIN1 — Toolchain pin**: `package.jet` carries a top-level
`jet:` field pinning the toolchain (channel semantics per D-JPK-CHANNEL1;
`.jet/lock` records the exact version). A jet whose version differs realizes
the pinned toolchain into the hangar (prebuilt objects via D-JPK-CACHE1,
offline thereafter per D-JPK-OFFLINE1, GC per D-JPK-GC1, no Nix required per
D-JPK-NONIX1, no daemon/root per D-JPK-NODAEMON1) and execs it
(D-JPK-DISPATCH1). Frozen-forward identity block: the bare top-level identity
fields stay parseable by every future jet, so an old jet can always read enough
of any manifest to fetch the right toolchain. The retired `payload:` wrapper is
an ordinary E1206 unknown-field error. `jet self toolchain`
shows the pin; `jet update jet` moves it deliberately.

**U9 — Provider inference**: a source is `name: target@provider`, or a bare
local path. Core vs nix is inferred by probing the target for `package.jet`
(cheap manifest-only probe; `…@nixpkgs` is never probed). No `via:` marker.

**D-TGT1–4 — Targets**: packages declare `targets:` (no `kind:`); shipped:
`library`, `executable`, `test`, `example`, `benchmark`; `plugin` reserved.
Bare keyword or block (`executable { entry: "src/cli.jet" }`); bare
`executable` searches `run.jet`, `src/run.jet`, then `<package>.jet`; legacy
`main.jet` locations remain compatibility fallbacks. **D-ILE1**:
omitted targets infer from `fn run()` (executable else library; two entries
E_DUPMAIN).

**D-CAP4/5/6 + c129 — API freeze (retired 2026-07-04 by D-MEM1/S2)**:
originally, `library { api: stable | explicit }` froze public capability
signatures into `.jet/cache/api/<package>.api` at `jet registry publish`, drift was
E0912, digest folded into the lock fingerprint. D-MEM1/S2 deleted the
mechanism outright: the `api:` field no longer exists (an ordinary
unknown-key error, E1216, like any typo'd key); `ApiFreeze`'s snapshot
machinery survives, re-grounded as unconditional pub-fn semver diffing
(E1218/E2601) — same intent (breaking-change detection at publish), no
capability-tier freeze.

**Publishing & supply chain**: `jet registry publish` (version from `package.jet`;
refuses dirty tree/failing tests, `--allow-dirty`; errors E1219+)
(D-PUBLISH1A). Published versions permanent; `jet registry yank --undo` hides from new
resolution only (D-VERSION1). Ranges `textkit#^1.2` freeze in `.jet/lock`
(D-RESOLVE1); `jet new` commits the lock for executables, ignores it for
libraries (D-LOCK1). SHA-256 verification always-on (E1204); Ed25519 signing
opt-in (D-PKGSIGN1). `jet registry vendor`, `jet inspect audit`, `jet build --sbom`
(D-SUPPLY1; E1217/E1218). Store is content-addressed (D-CASTORE1).

**Cryptographic entropy** *(D-CRYPTO-RNG1, D-CRYPTO-WASI-ALLOC2)*: one
fail-closed operating-system provider supplies `random.bytes`, envelope
nonces, signing keys, password salts, and file envelopes. WASI retries
`random_get` interruption at most sixteen times after the first call. Every
generation owns a new exact-count zeroed `Vec`; a failed generation is fully
volatile-zeroized and dropped before the next is created. A later allocation
may reuse the same numeric address. No bytes, capacity object, provider state,
or reference crosses ownership lifetimes, and only a successful generation
can escape.

**Package-key entropy failure** *(D-CRYPTO-KEYGEN-DIAG1=A)*: explicit
`jet registry keygen` and automatic first-publish key creation fail as a tool
error when the operating system cannot provide cryptographic randomness.
Stdout stays empty; no provider/helper/dependency text escapes; no key,
package, index, or temporary artifact is created; secret temporaries are
volatile-zeroized; an existing valid key bypasses key generation. The selected
E1275 spelling conflicted with D-JPK-NODAEMON1's existing sandbox assignment.
D-CRYPTO-KEYGEN-CODE2=A therefore assigns this command failure E1292 and leaves
E1275 unchanged. E1292 exits 1 and renders the exact headline plus What/Why/Fix
frame; `jet explain E1292` is generated from the same diagnostics ledger.

**Build system** *(D-BUILDENTRY1, D-BUILDPOLICY1, D-BUILDSCOPE1, D-BUILDGEN1,
D-BUILDPROFILE1, D-BUILDNORM1, D-BUILDTARGET1, D-BUILDACTION1,
D-BUILDTOOLCHAIN1, D-BUILDPROBE1, D-BUILDCACHE1, D-BUILDREMOTE1,
D-BUILDSCHED1, D-BUILDQUERY1, D-BUILDLEGACY1, D-BUILDPLUGIN1,
D-FRONTENDAPI1, D-DSLBLOCK1, D-METAMUTATE1)*: compile-time build entry is
`fn build(b: BuildContext)`, living in the unit's own definition file (beside
`fn run` / in `package.jet` / in a discovered Config); `jet build` runs it when
defined, else the batteries pipeline. Build code is tiered: Tier 1
pure+locked by default; Tier 2 needs `#Impure("reason")` + explicit
permission + provenance; deps never get Tier 2 implicitly. Generated source
lands under `.jet/generated/`, never committed; lock records source+output
hashes. Profiles: `Build.{optimize, debug_info, small, panic, settings}`,
selected by explicit flag (`--release`/`--profile=<name>`), never ambient env.

D-BUILDTARGET1=A: build targets are registered once with `b.add_executable`,
`b.add_library`, `b.add_test`, `b.add_bench`, `b.add_asset_bundle`,
`b.add_doc`, `b.add_install`, `b.add_package`, and `b.add_publish`; each call
returns a typed handle and `b.plan()` / `b.plan(default: target)` returns the
registered graph. D-BUILDACTION1=A: `b.action(name, inputs, outputs, run,
caps)` declares cached actions; side-effect-only commands are explicit,
uncached, visible, and capability-gated. D-BUILDTOOLCHAIN1=A: default host
toolchain is inferred; non-default builds use typed toolchain handles from
jetpack/toolchain deps, with host/target triples, SDKs, and signing identities
recorded. D-BUILDPROBE1=A: configure checks are typed probes, each classified
as reproducible or ambient.

D-BUILDCACHE1=A: local action cache is automatic; the key includes inputs,
outputs, argv, env, caps, tool digest, target, policy, toolchain, compiler
version, and generated source hashes. D-BUILDREMOTE1=A: local is default;
remote cache and remote execution are separate policy grants, and remote
execution waits on sandbox/provenance proof. D-BUILDSCHED1=A: the scheduler is
deterministic with automatic parallelism and named resource pools (`cpu`,
`memory`, `linker`, `console`, `gpu`). D-BUILDQUERY1=A: graph inspection is
`jet inspect graph`, `jet inspect query build`, and `jet inspect explain-build <target/file/action>`,
with the LSP using the same graph/provenance model.

D-BUILDLEGACY1=A: legacy CMake/Make/Gradle/npm/cargo builds are Tier-2
wrappers with declared inputs, outputs, and caps; optional graph import lives
inside the same wrapper and CI can ban it. D-BUILDPLUGIN1=A: one build-plugin
contract covers first-party Jet build libraries and packaged/third-party WASM
component plugins under policy; both emit the same BuildPlan graph. D-FRONTENDAPI1=A:
`core.compiler` exposes stable read-only lexer/parser/check/semindex/source-map
value APIs plus a CLI JSON mirror; internal compiler crates stay private and no
AST mutation enters compilation. D-DSLBLOCK1=A: stdlib-only PascalCase
directive DSL blocks such as `#SQL(Row) { ... }` and `#HTML { ... }` are a
fixed whitelist in `Syntax.rs`; third-party grammar mutation is rejected.
D-METAMUTATE1=A: Jai-style AST mutation/message loop/user macros are rejected;
the power surface is additive generated modules/overlays, registered
targets/actions, read-only program/build graph enforcement, DSL blocks, and
front-end APIs.

**D-BUILD-DEFAULT1=B — default profile split by command** *(ratified
2026-07-16, card #666)*: `jet run` and `jet dev` compile at the fast profile
(opt-level 0, codegen-units 256, no LTO); `jet build` keeps the optimized
profile (opt-level 2, thin-LTO, strip) unchanged from D-BUILDPROFILE1. An
explicit `--profile=<name>`/`--release` flag overrides either command's
default; D-BUILDPROFILE1's "never ambient env" rule is unchanged.

**D-VERDICT-666-1 — one compiler, two lenses** *(owner verdict 2026-07-16,
card #666)*: “One compiler core, two lenses. JIT lens = rapid dev work people
love in python/typescript; AOT lens = highly optimized ship binary at the cost
of longer build time. Same compiler core prevents drift: there should NEVER be
a difference in supported features/functionality between JIT and AOT.” D-AOT-CRANELIFT1
(below) decided the remaining AOT mechanism; it does not create a third
product lens or a feature/functionality split.

**D-AOT-CRANELIFT1=B — Cranelift emits the fast AOT debug profile** *(ratified
2026-07-16, card #666)*: on supported targets, `jet build --profile=debug`
lowers the same sema-approved executable TIR through Cranelift to object code
and links, with no generated Rust and no rustc. Unsupported targets fall back
to rustc opt-level 0 and report that fallback in `jet explain build`; they
never lose features or silently change optimization intent. Optimized AOT
(`jet build`, `--release`) keeps the rustc backend. Both backends face R12
differential tests; behavior and diagnostics are identical, only wait time
differs.

```text
$ jet run hello.jet    # fast: opt-level 0         ~150 ms (illustrative)
$ jet dev  hello.jet   # fast: opt-level 0         ~150 ms (illustrative)
$ jet build hello.jet  # optimized: -O + thin-LTO  ~450 ms (illustrative)

$ jet build --profile=debug hello.jet   # fast build of a build artifact
$ jet run   --release       hello.jet   # optimized one-off run
```

**Migrations** *(D-MIGRATE1, D-MIGRATE2A–F)*: `#PublishedSchema` types
snapshot field layout; a breaking change without a migration is E0910.
Verbs: `add f: T = val`; `remove f`; `change f: Old => New via { old =>
expr }` (converter: inline `via` → `impl Old => New` in scope → E0910); no
`reorder`. CLI: `jet inspect schema squash --before <ver>`, `jet inspect schema status`.

**Decode-time migration transparency** *(D-MIGRATE3=A)*: `decode_traced<T>(raw)
=> DecodeResult<T> ?` beside `decode<T>` on every codec sharing the decode
machinery (json/csv/toml/yaml); `DecodeResult<T> = { value: T, migration:
MigrationStatus }`, `MigrationStatus = { migrated: Bool, from: String, steps:
[String] }`. `decode` unchanged (I8, zero cost for callers not asking).
`.migrated` is `false` for a plain type and for a `#PublishedSchema` type
decoding fresh (current-shape) data; the migrated cases are D-MIGRATE4.
**D-MIGRATE4 (=A, ratified 2026-07-03, c105migrate4; shipped)**: the runtime
half — codegen lowers each `migration { }` block to a step function; decoding
a `#PublishedSchema` type first tries the current shape (prefer-newest
ambiguity rule), on mismatch detects the source shape by field-name set
(newest matching historical shape wins) and walks the chain oldest→current
applying steps. Plain `decode` applies silently; `decode_traced` records
`from` + `steps` (positional labels: `v1` oldest, steps `"v1->v2"`). No
matching shape → the ordinary decode error. Zero cost for types without
migrations. Runtime semantics: spec.md "Runtime migration chain".

**D-EXPANDCLI1 (=A, ratified 2026-07-03, c183expand)**: the transparency
command is `jet inspect expand --facts <lens> <file>`; bare `jet inspect expand <file>`
runs every lens, grouped (magic default). Lens floor: `inline`
(D-METHODMACRO1); other ratified surfaces add lenses under the same flag,
never new commands (I8). A `refs` lens (D-REF-SHORTHAND1) shipped alongside
`inline` at c183expand; D-MEM1/S3 deleted it along with the `&T`
stored-ref-field mechanism it reported on (`--facts refs` is an
unknown-lens usage error today).

**Jetpack engine** *(D-JPK1/2/5/9/16, D-JPK-ADAPTER1, D-JPK-BUILDRECIPE1, D-JPK-GC1,
D-JPK-NONIX1, D-JPK-CACHE1, D-JPK-PLATFORM1, D-JPK-NODAEMON1,
D-JPK-OFFLINE1, U5, D-MONOREF1)*: `jetpack` is its own binary
(`run/build/list/clean/add/remove` + `enter`); Jetpack owns the user model,
refs, lock, shells — Nix is one provider behind the `core`-first resolver
trait (tvix shim scoped I6 waiver for the no-installed-nix goal). Ad-hoc
adapters are `Pkg.adapt(name:, source:, recipe:)` with curated recipes
(`prebuilt`, `copy`, `cargo`, `go`, `node`, `cmake`/`make`) and finite executable
`Recipe.build(steps: […])` actions. Hangar GC by age
(default 30 days), `jet clean` (one verb: garbage-collect + optimize the
hangar via hardlink/dedup, `nix store optimise` equivalent; owner amendment
2026-07-03 — there is no `jet store gc`), `jet hangar du`; no daemon, no root
(transient sudo only for jetos activation). No-Nix machines degrade gracefully (E12xx
names fixes). Binary cache = output-hash-addressed HTTP(S) protocol with
signed objects; miss never errors. Linux+macOS+Windows tier-1 native.
Offline is a tested guarantee: realize-class verbs never touch the network
when the lock is satisfied. One canonical merge table (unified-ecosystem §6)
across env/system/image. Monorepo addressing: `source.package` dot form +
in-repo path-style + bare-name sugar when unambiguous.

**D-JPK-SERVICEAUTH1=A — safe authority for dev-service trees** *(ratified
2026-07-15, narrowly amends D-JPK-NODAEMON1)*: background dev services
activate per proved platform. Linux uses a transient systemd user unit with a
delegated control group; Windows uses a project-local guardian and Job Object
alive only for the service's lifetime; macOS fails before spawn with E1332
(`Safe service authority is unavailable for {name}.`). Root, a global Jetpack
daemon, package background work, new syntax, new manifest fields, and new CLI
remain forbidden. Structured output exposes backend, generation, phase,
containment, and recovery facts; a post-Ready crash recovers the Ready
generation.

**D-JPK-EXECLEASE1=A — Protected native executable lease service**: Linux
keeps its unprivileged private read-only mount. Official macOS and Windows
installers, after one explicit administrator approval, register a narrowly
scoped root-owned LaunchDaemon or LocalSystem service. The service accepts only
authenticated local lease requests, independently re-verifies the locked
digest, copies executable bytes into a caller-unwritable protected directory,
and launches under the caller UID/session/token with no root/SYSTEM privileges,
service handles, privilege-sensitive environment, or network access. Protected
lease bins precede only the caller's approved inherited/system PATH. The
service never resolves packages, evaluates source, builds arbitrary requests,
or accesses the network; mismatch, stopped service, unverifiable bytes, or
failed isolation rejects before any child starts. `jet env` keeps the lease
through the shell and every descendant and deletes it after the process tree
exits. Doctor/audit output names service/toolchain version, caller and child
identity, stripped authority, PATH, network denial, verified handoff, and
same-user redirect protection. This is the ratified narrow amendment to the
otherwise no-root/no-daemon product law; ordinary run/env remains offline and
ceremony-free after installation.

**D-FE-PROMPT1 (=D) + D-FE-PROMPT-STRIP1 (=B, ratified 2026-07-08, #359)**:
`jet env` uses one hybrid prompt engine. Default prompt shows the env label and
compact path; `Ctrl-G` pulls the same status words the optional always-on strip
shows. Shorthand remains `prompt: "web-api"`. Expert config is
`prompt: Prompt.{ label: "web-api", path: .Short, strip: .On }`
(`path: .Full` and `strip: .Off` also valid). Shell state is only a renderer;
source truth stays in `env.jet`.

**D-FE-CLI1 (=D, ratified 2026-07-08, #361)**: jetpack/jetos CLI output is
hybrid and consequence-scaled. Trivial reads stay quiet. Long realization/build
work uses the shared dependency-chain progress renderer, with deterministic
plain ledger fallback. Mutations print a plan first with the same `+`, `-`, `~`
marks in color and plain logs; `-y` and `--yes` are equivalent confirmation
bypasses. Non-interactive mutation without either flag prints the plan and
does not apply it. Diagnostics remain verbatim and JSON schemas are unchanged.

**D-JPK-RINGSHIP1 (=C, ratified 2026-07-03, c1rixz5d)**: first-party
`core.*` ring libraries ship as prebuilt per-platform artifacts riding the
pinned toolchain object (D-JPK-TOOLCHAIN1) — realizing the toolchain stages
them into the hangar (offline forever, D-JPK-OFFLINE1);
`is_ring_module_staged` flips true when present; the compiler-embedded
bridge templates remain the zero-config fallback for a dev-built jet. Ring
version equals toolchain version by construction; one resolution path.

**D-JPK-BUILDTOOL1 (=A, ratified 2026-07-03, c1rixz5d)**: bridge crates
build with a pinned, realized Rust toolchain (hash-pinned hangar object,
substituted via D-JPK-CACHE1 or nixpkgs on Nix machines; D-JPK-NONIX1
honest error otherwise) — never the host cargo/rustc. Same source + same
pinned toolchain → same output hash → portable cache hits; the toolchain id
enters output provenance.

**D-JPK-OVERLAY1=A** *(ratified 2026-07-07, card #330)*: Jetpack package
overrides live as reviewed source truth in `workspace.jet`: typed workspace
policy, named overlay sets, provider/channel swaps, per-package patches, and
unfree policy. Reusable overlay modules may package override logic only when
they materialize as typed workspace policy facts. CLI override commands are
drafting tools that write source patches/policy, not hidden state. Shipped
surface: `overlay <name> { provider: Provider.nixpkgs(channel: "...");
package("pkg").patches += [patch("path.patch")] }`,
`policy.allowUnfree: [...]`, `jetpack override draft`, unified-diff patch
application, and `jetpack explain package-overlay:<overlay>:<package>`.

**World-domination ratifications (D-WD1–12, D-WD14–15 = B, ratified
2026-07-06, c07589v1)**: these decisions set product law and planning
direction, not a blanket approval of every illustrative syntax snippet in the
ballot. Any new user-typeable keyword, marker, manifest field, provider prefix,
command spelling, or diagnostic code still needs its own Tower ballot before
implementation.

- **D-WD1**: one grant graph spans code effects, packages, builds, envs,
  services, images, fleets, and jetos activation. Beginner UX summarizes by
  intent; expert UX exposes exact authority, provenance, cache keys, and
  revocation.
- **D-JPK-GRANTCMD1 (=A, ratified 2026-07-06, #229)**: trust authority is
  controlled through `jet trust`: `list`, `explain`, `grant`, and `revoke`.
  Older `jetpack config trust` remains a storage-management compatibility path;
  the product concept is `jet trust`.
- **D-JPK-GRANTSCHEMA1 (=A, ratified 2026-07-06, #229)**: reviewed source
  policy lives under `policy.trust` in `package.jet`, e.g.
  `policy: { trust: { default: prompt, ci: { prompt: deny }, services: { postgres: prompt } } }`.
  Trust policy feeds the same grant graph used by prompts, CLI, locks, and
  audit.
- **D-JPK-CATALOG1 (=A, ratified 2026-07-06, #231)**: shared dependency
  versions live in `workspace.jet` under `catalog:`. Packages still opt in
  through ordinary visible deps, e.g. `deps: { http: catalog.http }`; the
  catalog is not a hidden dependency leak.
- **D-JPK-STRICTVIS1 (=A, ratified 2026-07-06, #231)**: strict package
  visibility failures get dedicated diagnostics that name the requesting
  package, hidden dependency, reason, and smallest valid direct-dep or catalog
  fix.
- **D-JPK-IMPORTCMD1 (=A, ratified 2026-07-06, #233)**: foreign metadata is
  imported with `jet import <ecosystem> <path>` plus update/dry-run/conflict
  policy. Generated Jet source is canonical, editable output.
- **D-JPK-IMPORTTODO1 (=A, ratified 2026-07-06, #233)**: importer gaps use a
  dedicated TODO diagnostic family carrying what/why/fix, source path,
  generated target, and migration status; generated comments are secondary.
- **D-JPK-PROVIDERAUTH1 (=A, ratified 2026-07-06, #234)**: provider trust
  roots live in reviewable source policy at `policy.providers`, feeding fetch
  grants, cache, signatures, lock rationale, and audit.
- **D-JPK-REPLACEPOLICY1 (=A, ratified 2026-07-06, #234)**: native
  replacements are controlled by `policy.replacements`; compatibility proof is
  mandatory, and policy only chooses allow/deny/prefer.
- **D-JPK-REPLACEPROOF1 (=A, ratified 2026-07-06, #234)**: native replacement
  packages publish `replacementProof:` metadata naming the foreign package,
  public surface, effects, errors, examples, and goldens proved.
- **D-WD2**: `jet inspect dossier` is the umbrella explain view over named existing fact
  lenses. Each section must be owned by a real fact producer; experts get stable
  lenses and JSON schemas.
- **D-WD3**: Jetpack package visibility is strict by default; workspace catalogs
  centralize shared versions. Missing-dependency diagnostics should offer the
  right `jet add`/catalog fix.
- **D-WD4**: `.jet/lock` is exact machine identity plus explainable rationale,
  provenance, owner package, policy, platform, and semantic merge support.
- **D-WD5**: migration importers generate editable canonical Jet source, role
  modules, deps, adapters, FFI stubs, and TODO diagnostics; native migration
  progress is tracked.
- **D-WD6**: npm, PyPI, Cargo, SwiftPM, Nix, GitHub, and binary sources are
  federated metadata providers under Jetpack's fetch, lock, sandbox, audit,
  signature, and replacement-overlay authority.
- **D-WD7**: jetos Studio is a GUI/source editor over canonical Jet modules,
  with diff preview and expert provenance; no GUI-owned split-brain state.
- **D-WD8**: jetos activation always has plan/diff, with VM proof and rollback
  proof required for risk classes such as boot, kernel, filesystem, and service
  changes.
- **D-WD9**: Core gets a typed data floor (tables, series, stats, plotting
  basics) while Python/R/GPU bridges cover gaps and expose native replacement
  status.
  - **D-DATA-SURFACE1 (=A, ratified 2026-07-06, #237)**: tables, series,
    stats, CSV, and plot builders live behind one beginner import,
    `core.data`; expert modules may sit below it with the same operation names.
  - **D-DATA-BRIDGE1 (=A, ratified 2026-07-06, #237)**: accepted bridge
    providers use direct roots such as `py.*`, `r.*`, and `gpu.*`; data APIs
    accept typed bridge results and report status instead of nesting providers
    under `core.data`.
  - **D-DATA-STATUS1 (=A, ratified 2026-07-06, #237)**: data workflows expose
    machine-readable Core status through an API, and the canonical human view
    is the D-WD dossier lens `jet inspect dossier data`.
  - **D-DATA-PLOT1 (=A, ratified 2026-07-06, #237)**: Core plotting starts with
    first-party deterministic SVG plus a text backend; bitmap export may layer
    on the same model later. The shipped line family is `line_text(groups,
    options)` / `line_svg(groups, options)` over `DataGroup`; `DataLineOptions`
    carries title, x/y labels, point markers, an optional horizontal reference,
    line style, color, and legend. Bar and line renderers use deterministic
    output and the same checked geometry rules on every execution tier.
- **D-WD10**: `core.game` is a stable game substrate: assets, ECS, input,
  fixed-step timing, deterministic replay, editor hooks, and budgets; renderer,
  audio, and editor backends remain replaceable packages.
- **D-WD11**: embedded/freestanding work uses typed target profiles that expose
  memory, linker, allocator, panic, volatile/MMIO, and audit controls only when
  such targets are selected.
  - **D-TARGET-SURFACE1 (=A, ratified 2026-07-06)**: embedded/freestanding
    profiles are typed Jet profile modules selected through `targets:`; hosted
    single-file programs never mention them.
  - **D-TARGET-MEMORY1 (=A, ratified 2026-07-06)**: memory uses named regions
    with origin, size, access, and kind; sizes use typed units, addresses stay
    numeric, and validation catches overflow/overlap/MMIO mistakes before
    codegen.
  - **D-TARGET-LINKER1 (=A, ratified 2026-07-06)**: Jet generates linker input
    from typed profile facts by default; expert file overrides require explicit
    hashed provenance and appear in audit output.
  - **D-TARGET-ALLOC1 (=A, ratified 2026-07-06)**: freestanding `allocator` and
    `panic` are required typed profile facts; hosted defaults stay hidden, and
    sema reads these facts to reject unavailable Core APIs.
  - **D-TARGET-AUDIT1 (=A, ratified 2026-07-06)**: `jet inspect dossier target` is the
    canonical human/machine audit view, and builds also write the same stable
    JSON artifact for CI archives.
- **D-WD12**: `jet prove` becomes a progressive proof/replay product over
  contracts, refinements, effects, budgets, property tests, and replay facts;
  solvers are opt-in lenses with Jet diagnostics.
- **D-WD14**: performance budgets attach to packages/envs/services and feed
  build, bench, dev, dossier, and CI; deterministic budgets are hard gates,
  statistical budgets use pinned baselines and trend policy.
- **D-WD15**: native replacement overlays require compatibility proof across
  public types, effects, errors, examples, and golden fixtures before a native
  Jet package can replace a foreign surface without call-site rewrites.

**D-JPK-ADAPTNAME1 (=A, ratified 2026-07-03, c9jetpackgates)**: adapter
spellings confirmed as `Pkg.adapt(name:, source:, recipe:)` + the curated
`Recipe.*` family (`Recipe.prebuilt/copy/cargo/go/node/cmake/make`).
`jet add <ref> --adapt` drafts one.

**D-JPK-BUILDRECIPE1 (=A, ratified 2026-08-03)**: an executable adapter uses
the finite public form `Recipe.build(steps: […])`. Its steps are `.fetch(url:,
sha256:)`, `.exec(tool:, args:)`, `.install(src:, dest:)`, and
`.install_tree(src:, dest:)`. The parser lowers these values into the existing
`BuildStep` action graph. Unknown fields, missing fields, unlocked fetches,
invalid paths, cycles, and partial outputs remain errors; the recipe does not
add ambient authority or a second build mechanism.

**U1 — manifest history**: superseded — see D-ECO-FILEROOT1 above (`package.jet`).

### Jetpack Images

**D-JPK-IMAGE1 (=A, ratified 2026-07-01, c9jetpackgates)**: active `image.*`
syntax is OCI-only: `from: packages.<name>` (a package this project's `package.jet`
declares `executable`) + optional `kind: .Oci`, `expose: [Int]`, `env_vars:
[KEY: "value"]` (map keys must be quoted strings — no bare-ident sugar),
`files: [String]`, and `base: oci("<ref>")`. A local `file://` OCI layout is
validated and its layers are preserved before Jet appends the new deterministic
layer. `jet image <name>` builds a deterministic OCI layout
(`oci-layout`/`index.json`/`blobs/sha256/<digest>`) with an uncompressed tar
layer. Remote base pulls and pushes remain E1268 until a verified registry
transport is configured; Jet never silently builds from scratch or claims a
remote transfer.

**D-ENV-INTEGRATIONS1=A**: first-party environment integrations are typed
imports in the existing module `imports:` field. The closed v1 names are
`env.platform.android()`, `env.platform.apple()`,
`env.security.certificates(...)`, `env.network.hosts(...)`,
`env.agent.codex(...)`, `env.editor.vscode(...)`,
`env.cloud.credentials(...)`, and `env.security.vault(...)`. Each lowers into
the existing package, file, task, provider, host-check, secret, and grant
facts. Safe presets are deterministic; expert arguments preserve exact SDK,
target, path, license, certificate, host, tool, and policy facts. Secret values
are names-only in plans and receipts. There is no integration-owned resolver,
lock, effect, or activation engine.

### jetos Runtime Slice

**D-JPK-OSVERB1=A / D-JOS-PROOFAPI1=B**: the public CLI is `jet os
check|init|plan|proof|build|switch|rollback|generations|lift|import|image`. `jet os
plan` prints the canonical checked plan without building. `jet os proof` reads
the latest generation's proof, provenance, health, boot, init, secrets, VM, and
rollback artifacts. `jetpack` remains the engine process behind the dispatch
seam; users type `jet os`, not `jetpack os`.

**D-JPK-OSHOST1=C**, as amended by **D-JPK-REF1=A**: a bare host name discovers
`system.<host>` in `./config.jet`; `host@root` selects an exact external root
(directory roots load `root/config.jet`; file roots load that file).

**D-JPK-OSGEN1=C**: every build gets an automatic generation name; `jet os
switch --name <name>` overrides it. `jet os generations` lists newest first.

**D-JPK-OSNS1=B / D-JOS-SYSTEMTREE1=A**: jetos option keys use full-word
namespaces: `filesystem.*`, `network.*`, `packages.*`, `services.*`,
`users.*`, `user.*`, `apps.*`, `performance.*`, `storage.*`, `theme.*`,
`workload.*`, `hardware.*`, `groups.*`, `secrets.*`, `boot.*`, `kernel.*`,
`init.*`, `health.*`, and `deploy.*`. The current generation projection covers package closure/cache
facts, users/groups, filesystems and swap, network/firewall/wireless facts,
systemd services/timers/sockets plus target wants, kernel firmware/driver
facts, desktop display manager facts under `services.*`, secrets, health checks,
and explicit audited compatibility escape hatches under `packages.*`
(overlay/specialArgs/nixModule).

**D-JOS-BOOTKERNEL1=A / D-JPK-OSINIT1=A / D-JPK-OSSECRET1=A /
D-JPK-OSBRAND1=A / D-JPK-OSDISK1=C / D-JPK-OSDISABLE1=C**: the active runtime
slice defaults to the Limine bootloader, CachyOS kernel, and systemd init,
generates systemd unit files in generations, requires a first-party `systemd`
package for the default `/sbin/init` projection, records repo-ciphertext plus
host-key tmpfs-only secret activation proof, brands installer artifacts as
`jetos`, defaults guided disk setup to ext4 with a manual path override, and
keeps discovery aligned with the existing module/import skip rules.

**D-JOS-KERNELSRC1=A**: `.CachyOS` resolves to a first-party
`cachyos-kernel` package. Generation proof records that package's reference,
output hash, provenance, and boot artifacts; jetos does not silently substitute a
generic kernel.

**D-JOS-KERNELBOOTSTRAP1=A**: the first-party `cachyos-kernel` package is
source-built. Its output must carry the source recipe, kernel config, patch
manifest, initrd-input manifest, kernel image, and initrd. Generation, installer,
and VM proof all boot the same recorded artifacts; no generic proof-only kernel
may stand in for `.CachyOS`.

**D-JOS-INSTALLUX1=A / D-JOS-INSTALLMEDIA1=A / D-JOS-VMCOMMAND1=A /
D-JOS-VMDEPS1=A**: the installer surface offers guided and scripted modes,
builds a hybrid ISO first, proves install/reboot flows through `jet os vm
prove <host> --disk <path>`, and uses pinned tool packages for QEMU/media work.

**D-JOS-STUDIO-HOST1=A**: jetos Studio runs through one local Jet-owned
projection/edit service. It is a separate jetos application, not Canvas. By
default `jetos studio` opens the installed first-party jetos Studio app from the
jetos system profile when available. The browser UI is the fallback, review, and
headless screenshot path against the same protocol. The GUI never owns semantic
configuration state outside Jet source, lock/proof artifacts, and the generated
local UI cache ratified by D-JOS-STUDIO-STATE1.

**jetos post-runtime surface ratifications** *(ratified 2026-07-07, cards
#320-#336)*:

- **D-JOS-VMTEST1=A**: `vmtest.<name>` declarations are the canonical VM
  scenario surface and are also normal test targets for CI, filtering,
  artifacts, and package integration. Single-host smoke shorthand expands to
  `vmtest`; multi-host topology stays explicit.
- **D-JOS-VMASSERT1=A**: VM tests use typed host handles and assertion
  methods. Those methods also produce declarative check values/proof facts for
  Studio and CI replay. String shell commands are explicit fallback assertions,
  never the default.
- **D-JOS-USERENV1=A**: `user.<name>` declarations are the canonical per-user
  environment source. A user generation can apply standalone or attach to
  `system.<host>`; host-specific overrides live where the host composes the
  generation.
- **D-JOS-USERAPPLY1=A**: `jetos user plan|build|switch|rollback|prove` is
  the standalone user-generation path, and `jet os switch` invokes the same
  user-generation engine when a host imports user generations.
- **D-JOS-CONTAINER1=A**: isolated workloads use one `workload.<name>`
  mechanism. The backend enum selects Container or MicroVM; shared fields cover
  image/package, ports, mounts, secrets, health, resources, proof, and
  rollback. Backend-specific knobs live under nested profiles.
- **D-JOS-HARDWARE1=A**: hardware scans emit `hardware.<host>` source.
  Systems import that source and may apply first-party or community hardware
  profiles. Specialisations are named boot variants over the same host and
  generation, with explain output showing what changed.
- **D-JOS-DISK1=A**: one storage tree declares disks, partitions, encryption,
  filesystems, mounts, ephemeral roots, and persistence. The installer consumes
  it for destructive actions; activation consumes it for mounts and persistence
  proof. Guided install may draft this source only.
- **D-JOS-PRIORITY1=A**: option conflicts use named tiers (`Default`,
  ordinary, `Force`) backed by explicit expert `Priority(n)` weights inside the
  same mechanism. Explain output shows all contenders. Module disabling uses
  stable module IDs.
- **D-JOS-PRIORITY-SURFACE2=A**: an ordinary option contribution stays a plain
  value. Expert precedence wraps only that contribution as
  `OptionValue.{ value, priority }`, where priority is `.Default`, `.Force`, or
  `.Priority(n)`. The distinct record cannot collide with a real dotted option
  key such as `filesystem.swap.fast.priority`. Explain output retains wrapper
  metadata and every contender; the option consumer receives only `value`.
- **D-JOS-THEME1=A**: `theme.<name>` modules are reusable theme profiles. A
  system references one and may override specific targets inline; the theme
  engine projects GTK, Qt, terminals, editors, display manager, and Studio
  preview from one source.
- **D-JOS-FLATPAK1=A**: `apps.flatpak` declares remotes, refs, pins/tracking,
  reconcile mode, and permission policy. User environment app modules may
  reference those apps for per-user install intent; activation computes one
  plan/diff/proof and rollback path.
- **D-JOS-KERNELTUNE1=A**: beginners choose safe/lts/performance profile enums.
  Experts override typed boot/performance families for kernel params, sysctl,
  zram, sched-ext, initrd, and bootloader. Overrides carry risk/proof
  classification and explain output.
- **D-JOS-FLEETTARGET1=A**: each fleet host carries typed target/authority
  fields beside its system. Friendly labels can be safe SSH defaults; reusable
  `deploy.target` refs cover bastions/CI. Host key policy, privilege boundary,
  and identity are proof-visible.
- **D-JOS-FLEETROLLOUT1=A**: default fleet push is staged, proof-gated, and
  rollback-and-stop. Experts can choose batch/canary/dependency order, health
  timeout, and stage-only/continue flows through the same rollout object.
  All-at-once is explicit policy, never default.
- **D-JOS-NIXIMPORT1=C**: `jet os import <flake-or-dir>` is a semantic
  NixOS/flake-parts/Home Manager importer. Default import evaluates the module
  graph into editable JetOS source, lock/provenance, and an omissions report.
  Facts-only import is a fallback for constructs JetOS cannot represent yet;
  every fallback is explicit and audited.
- **D-JOS-REALGUEST1=C**: a JetOS replacement claim requires real installed
  guest proof: QEMU boots installer media, installs to disk, reboots from that
  disk, logs in, launches Studio/desktop paths, checks packages/services/network
  and rollback, and records guest-bound artifacts. Fake-QEMU or host-only
  harness artifacts may test plumbing, but may not close replacement acceptance.
- **D-JOS-NIXBACKEND1=C** *(product/replacement use superseded; migration-only
  under D-JOS-NATIVE1/D-JOS-NIXEVAL1/D-JOS-NIXBACKEND2)*:
  `jet os vm prove <host> --disk <path> --real`
  realizes the system through a hidden NixOS backend — a generated
  `flake.nix`/`configuration.nix` under the Jetpack root, `nix build` for a
  bootable qcow2, and QEMU for the guest boot. The user only ever writes
  `.jet`; the backend is a build artifact, never a user-facing surface (I2).
  Every `SystemPlan` option/service/package this backend cannot map to a
  NixOS setting is collected and reported in one diagnostic (E1291) before
  `nix` ever runs — no silent omissions, mirroring D-JOS-NIXIMPORT1=C's
  discipline for the import direction.
- **D-JOS-DESKTOPPROOF1=C**: desktop proof means a live guest display-manager
  and session path, Studio/app presence, terminal fallback, and user-home facts.
  File presence and launcher `--jetos-proof` checks are preflight only.
- **D-JOS-IMAGEPROOF1=C**: image variants are `built` only when they are real,
  bootable artifacts with format-specific smoke proof. Sparse marker or deferred
  artifacts must report `staged`, not `built`.
- **D-JOS-FIRSTBOOT1=D**: first boot opens Studio as the OS control center:
  current host, generation, source, proof, update, rollback, and health are
  visible. Canvas is a deep-link/source graph editor from Studio source spans,
  not the first OS control surface.
- **D-STUDIO-CHANGESET1=D**: every Studio mutation stages through one changeset
  gate. Low-risk edits may use a compact review sheet, but widgets, alert fixes,
  rollback, and source edits all produce a diff/impact/proof requirement before
  apply.
- **D-STUDIO-WIDGETS1=D**: Studio derives controls from typed JetOS option
  shapes where safe. Unknown or expert-only constructs remain source-visible
  with an explicit reason and exact source span, never hidden GUI state.
- **D-STUDIO-SECRETS1=D**: Studio never renders plaintext secrets in DOM, JSON,
  screenshots, or logs. Add/rekey/rotate flows use audited secret transactions
  with recipient/proof diffs.
- **D-STUDIO-FLEET1=C**: Studio is single-host quiet by default. Fleet strips,
  staged rollout, and fleet proof controls appear only when the workspace
  declares multiple systems or fleet targets.
- **D-STUDIO-CANVASBRIDGE1=C**: Studio and Canvas are separate products sharing
  protocol components, source spans, proof widgets, and source transactions.
  Studio owns OS workflows; Canvas owns general source graph editing. Neither
  stores semantic state outside Jet source/proof artifacts.

**jetos native decrees** *(ratified 2026-07-09, card #363)*:

- **D-JOS-NATIVE1=A**: jetos is a from-scratch standalone OS. Building the
  product through the NixOS module system is forbidden; the earlier `--real`
  tier produced a reskinned NixOS and is not native jetos.
- **D-JOS-STORE1=A**: Hangar is the on-disk store. During stage 1, nixpkgs
  closures live under a Hangar-managed compatibility root; they re-root into
  native Hangar layout later. D-ECO-HANGARPATH1 governs the current default
  per-user platform path without changing this store-ownership law.
- **D-JOS-NIXEVAL1=C**: no `nix` binary may appear anywhere in the product
  path. Jetpack owns the nixpkgs evaluation, fetch, and build pipeline.
- **D-JOS-PARITYBAR1=A**: native exit requires NixOS-class typed modules and
  options, immutable generations, atomic activation, live switch, rollback,
  and a fully declarative system; terminal and graphical baselines; one-line
  GNOME, KDE, Hyprland, and Niri swaps across Wayland/X11; and the owner's
  complete `~/nixos` configuration running natively. The whole system remains
  user-config-driven, with defaults and Studio authoring available; module
  breadth beyond that bar grows by demand.
- **D-JOS-NIXBACKEND2=C**: the Jet-to-NixOS realizer survives only as a
  clearly labeled migration tool.

D-JOS-NATIVE1, D-JOS-NIXEVAL1, and D-JOS-NIXBACKEND2 explicitly supersede
D-JOS-NIXBACKEND1=C as a product and replacement-proof backend. Its hidden
NixOS realization may support migration only; it cannot build native jetos or
close D-JOS-PARITYBAR1.

**D-JOS-MIGRATIONVERB1=A** *(ratified 2026-07-16, card #415)*: `jet os migrate
compare-nixos <host> --out <dir>` is the one command that may reach the
migration-only NixOS backend. It builds the NixOS guest, boots it, verifies
guest identity, and only then publishes artifacts (system image, boot proof,
receipt). There is no successful unproved comparison state.

**D-JOS-IMAGEFORMAT2=A** *(ratified 2026-07-16, card #325)*: `jet os image`
takes `--format iso | qcow | raw | sd | netboot`; a bare `jet os image`
defaults to `iso`. No other spelling is accepted. Every image publishes its
proof artifact alongside the file.

**D-JOS-LIFECYCLE1=A** *(ratified 2026-07-16, card #327)*: visible retention,
automatic upgrade off. `jet os init` writes all lifecycle fields into the
system module: `packages.generations.keep`, `packages.generations.keepYoungerThan`,
`deploy.autoUpgrade.enable: false`, `deploy.autoUpgrade.schedule`, and
`deploy.autoUpgrade.healthFor`. Experts flip `enable` on the same schedule;
upgrades continue while logged out.

**D-JOS-SERVICELOG1=A** *(ratified 2026-07-16, card #329)*: runtime journals
read via `jet os logs <unit>`. The positional unit normalizes to a systemd
unit (`web` resolves to `web.service`). Expert filters (`--generation`,
`--host`, `--follow`) extend the same local or authorized remote query;
denials and host loss are explicit errors, with a resume cursor on `--follow`.

**D-JOS-USERREMOVAL1=A** *(ratified 2026-07-16, card #404)*: absence locks and
preserves. Removing `users.<name>` revokes login but preserves the home and
UID reservation. `users.removals["name"]` is required only to explicitly
preserve, archive, or destroy, and carries audit facts (expectedUid,
expectedHome, action, destination, backupProof, reason).

**D-JOS-ETCMANAGE1=A** *(ratified 2026-07-16, card #401)*: declared `/etc`
entries live under the `filesystem.etc` map as named entries (path, source,
mode, user, group). Managed is the default; a mutable entry requires
`mutable: true` plus a seed, and live drift from the seed fingerprint blocks
the plan until reviewed. One owner per path: directory/file overlaps are plan
errors.

**D-WD7-WELCOME1=B** *(ratified 2026-07-16, card #474)*: the first-boot
driver/codec offer is an inline state on the ratified Studio control center,
not a dedicated welcome view. A dismissible "Detected hardware" panel sits at
the top of the normal host/generation/proof home until its offers are
installed or dismissed; a later boot with unchanged hardware shows no panel.

### CLI & tooling

**D-SHAPE-EXPOSE1=A — Every interface lens preserves the exact callable
contract**: CLI, HTTP, GUI, and tool lenses use the same application input,
output, declared failure, inferred or pinned effects, and function identity. A
lens may parse wire data into the application input and render the application
result back to its transport. It may not replace or alter the callable.

Access policy may narrow who can reach the function, never change what function
is called or what contract it has. Authentication, cancellation, streaming,
protocol, and version failures are typed boundary layers around the application
contract; they do not become alternate application failures.

The arrows below form an architecture diagram, not Jet source:

```text
CLI wire  -> GreetingRequest -> same greet -> Greeting | GreetError -> CLI wire
HTTP wire -> GreetingRequest -> same greet -> Greeting | GreetError -> HTTP wire
GUI event -> GreetingRequest -> same greet -> Greeting | GreetError -> GUI state
Tool wire -> GreetingRequest -> same greet -> Greeting | GreetError -> Tool wire
```

These diagram glyphs do not change any token's existing Jet meaning and
authorize no `|>` pipe, marker, or exposure declaration. D-SHAPE-EXPOSE1 does
not choose exposure spelling, transport mapping, wire types, authentication/
streaming/cancellation/version policy, or access-policy spelling. Each new
user-facing surface still requires its own owner ballot. #560 owns enforcement
of the shared law.

Because this decision adds no user-typeable token or form, it adds no
`Syntax.rs` entry, parser or runtime behavior, grammar rule, diagnostic,
snapshot, or executable example.

**D-CI1=A — Full change gate, slow science nightly**: every proposed change
runs the complete test suite on the exact candidate revision, sharded only to
reduce wall time, plus grammar-drift and documentation-build checks. No test
family is demoted to nightly. Nightly adds the supported Linux/macOS matrix,
the performance-regression harness and ratchets, a bounded fuzzing cadence, and
coverage reporting. CI uses the live Tower path. A green change therefore means
the whole executable spec passed; nightly evidence covers slower platform and
measurement work rather than repairing a knowingly partial merge gate.

**D-CI2=A — Warning-free compiler gate with expiring exceptions**: CI denies
Rust warnings, checks rustfmt, and runs the curated correctness/suspicious
clippy set without imposing style lints that conflict with house conventions.
Intentional lint exceptions use `#[expect(..., reason = "#card: ...")]`; the
card reference explains unfinished work and the expectation fails when its lint
stops firing. Unexplained dead code is not protected by convention or guesswork.

**S15 — Binary profile**: default build unwinds; `jet build --small` =
`opt-level="z"` + LTO + `panic=abort`.

**D-DEV4 / D-DEVMODE1 — Dev loop**: `jet dev <entry>` is the watch loop
(auto-detects rerun vs resident hot-swap; `--restart`/`--swap`/`--watch=off`
overrides); `jet env` enters the `env.jet` shell (delegates to
`jetpack enter`). **D-HOTSWAP1**: reload unit is a module; type-stable edits
swap in place, layout changes trigger a clean announced restart.

**D-DBG2/3 — Debugger**: `jet debug` shows only Jet frames by default
(`--raw-frames` expert opt-in); `(jet)` prompt, `step/next/continue/finish`
(+`s/n/c/f`), `<- here` caret, `locals:` dump (E2203/E2204). **D-OBS2**: the
Jet→Rust line table is a sidecar `<file>.jetmap` JSON (versioned, std-only).

**D-DBG-DAP1=A — Full local daily-driver DAP profile**: editors start
`jet debug --dap <file.jet>` over stdio. DAP framing uses strict byte-counted
`Content-Length` messages, positive correlated sequence IDs, bounded JSON
objects, one response per accepted request, structured Jet-owned errors, and no
TCP, reverse-request, telemetry, or remote transport. The adapter implements the
Start → Initialized → Configuring → Stopped/Running → Terminated state machine;
`initialized` follows a successful single launch or attach, and user code never
runs before `configurationDone`. Advertised capabilities must equal implemented
behavior.

The full profile includes launch with explicit args/cwd/env, verified true local
same-user attach, replacement source breakpoints with conditions, hit counts,
and logpoints, all-stop Jet-task threads, Jet-only stacks/scopes/nested variables,
read-only bounded Jet expression evaluation, panic/error/signal filters, pause,
launch restart, cancellation, progress, timeouts, ordered output, and exact exit
or signal status. Attach verifies process identity, executable build ID, source
hashes, architecture, process image, and matching `.jetmap` before changing the
target; disconnect defaults to terminating launched targets and continuing
attached targets. Restart rebuilds through the same pipeline and preserves
source breakpoints while invalidating every object reference.

The default projection never exposes generated Rust names, paths, helper
threads, backend values, addresses, secrets, or raw backend text. Explicit
`showRawFrames` appends clearly marked raw frames/scopes without changing
stepping or evaluation semantics. Mutation, memory read/write, reverse
execution, disassembly, and other unimplemented advanced requests are
truthfully unsupported. Conformance vendors a pinned Microsoft DAP schema,
model-tests every legal and illegal transition/reference lifetime, and drives
current VS Code and Zed clients plus native backend tests without skip on
supported hosts. Terminal and editor debugging remain projections of one
breakpoint, task, evaluator, exception, mapping, diagnostic, and process model.

**D-SEMINDEX1**: versioned semantic-index query API (symbols/refs/types/
call-graph/effects/member facts plus typed Package and workspace-overlay facts;
`jet inspect semindex --json`, schema v12) —
foundation for dossier views, breadcrumb hints, impact analysis, and codemods
(D-DOSSIER1/D-BREADCRUMB1/D-IMPACT1/D-CODEMOD1). `jet inspect dossier <file> [Symbol]`
is the D-WD2 umbrella over those facts; `jet inspect codemod` starts with named JSON
rename objects (`<plan.json> --dry-run`, `apply <plan.json>`, `undo <log.json>`) and replay logs. **D-DX5**: PATH `jet-*`
helper discovery (cargo/git-style external commands). **D-DX5-HOOK1=A**:
compiler-extension WASM components (typed post-sema snapshot; see above) —
not PATH helpers and not `target: plugin`. **D-REF3**: borrowed-return +
cleanup-scope inlay hints on by default. **D-JPK-DISCOVER1**: `jet search`/`jet info` + LSP completions from
a local offline index. **D-JPK-BUILDDBG1**: failed builds keep the scratch
dir; `--shell-on-fail`; `jet explain <ref>`; `jet logs <pkg>`.

**D-DOC-GEN1=A**: the documentation generator command is `jet doc`. Default
output is deterministic local HTML; `--json` emits the stable docs schema;
`--check` runs doc link, doctest, and stale-example checks. Implementation is
deferred until the owner explicitly reopens documentation build work.

**D-PROVE-REPLAY1=A / D-PROVE-SEM1=A / D-JPROOF1=A / D-JREPLAY1=A /
D-PROVE-SOLVER1=A / D-PROVE-LENS1=A**: `jet prove` is the single progressive
proof/replay command. It owns deterministic target resolution and producer
order, evidence policy, results/exits, stable complete JSON, typed versioned
`.jetproof`/`.jetproof-replay` artifacts, opt-in deterministic native Presburger proof,
and presentation-only evidence lenses. Raw solver/runtime text never reaches
users. Exact resources, canonical hashes/bytes, errors, privacy/security,
artifact lifecycle, migration/version law, fixtures, and failure precedence are
normative in [`proof-replay-decisions.md`](proof-replay-decisions.md); later
specialized decisions there override umbrella examples.

**D-PERFBUDGET-SURFACE1=A / D-PERFBUDGET-BASELINE1=A /
D-PERFBUDGET-GRAMMAR1=A / D-PERFBUDGET-REPORT1=A /
D-PERFBUDGET-OUTPUT1=A / D-PERFBUDGET-BENCHMIGRATE1=B /
D-PERFBUDGET-GAMEMIGRATE1=A / D-PERFBUDGET-PROVIDER1=A /
D-PERFBUDGET-INTEGRATION1=A / D-PERFBUDGET-COMPILE1=C**: performance budgets use typed
`module perf.<role> { budgets: [Budget.{ ... }] }` declarations, pinned
statistical baselines, one canonical `BudgetReport`, and exact `jet budget
check` / plan-first `jet budget update` projections. Full closed grammar,
inference, arithmetic, collision, report, baseline, storage, output,
diagnostic, fixture, migration, and prototype-retirement law is normative in
[`performance-budget-decisions.md`](performance-budget-decisions.md).
Compile budgets use the same surface with the closed
`CompileTime`/`CompilerProbe` workload values and typed `compile_workloads`
entries defined by D-PERFBUDGET-COMPILE1.

**D-BPE-NAME1=A / D-BPE-HOST1=B / D-BPE-LAYOUT1=A / D-BPE-ALTITUDE1=A /
D-BPE-TAXONOMY1=A / D-BPE-EDITSCOPE1=A / D-BPE-PROTOCOL1=C**: Jet's visual
code editor product is **Canvas** (owner rename 2026-07-07; this supersedes the
earlier Canopy name under the same decision). First host is a `jet dev` browser panel.
Layout is deterministic from source. Structural constructs are nodes while
pure leaves stay inline. The node vocabulary uses restrained semantic badges,
typed pins, and distinct control/error/proof rails. V1 write scope is insert
call, rewire, edit inline expression, add fallback rail, extract/collapse,
rename binding, and create test. The graph protocol may stay internal for the
read-only Reader, but must become public and versioned before write flows ship.

**D-CANVAS-PARITY1=C**: Canvas coverage is enforced by an AST-derived semantic
parity ratchet plus a manual UX matrix. Every shipped language form must have a
specific projection/edit/support status and either tests for real behavior or a
Jet diagnostic naming the unsupported boundary.

**D-CANVAS-CORECATALOG1=C**: Canvas exposes a typed Core action catalog. Core
functions and methods known to sema appear as compatible actions from pins and
search, with docs, imports, effects, fallibility, authority, examples, and
source-backed insertion.

**D-CANVAS-ACTIONAUTH1=C**: Canvas command/action execution uses one authority
model. Beginner intent cards expose Run/Check/Build and similar actions; expert
drawers show exact command, grants, touched files, outputs, diagnostics, and
diff/provenance before mutations.

**D-CANVAS-CODELENS1=C**: Canvas Code view is read-only until the user enters
explicit Edit Source mode. Edit Source uses the same source transaction,
formatter, check, and reproject pipeline as graph edits; Canvas may not claim
full edit parity without this path.

**D-CANVAS-PROOFLENS1=C**: Canvas has an always-visible proof/debug rail with
drilldown to diagnostics, commands, source revisions, build/run/proof state,
provenance, and stale-proof reasons. Badges alone are insufficient.

**D-VERIFY-SCHED1=C / D-VERIFY-CACHE1=C / D-VERIFY-TIER1=C**: `scripts/agent/verify-full.sh`
is a repo scheduler, not plain `cargo test`: it emits timings, uses bounded
parallelism/serial groups, uses `cargo-nextest` when available with a cargo
fallback, and targets a 1-3 minute default full suite. Generated-Rust test
artifacts may share a repo-local cache keyed by rustc version, generated source,
flags, and linked inputs. Slow/real tiers are separate but binding for claims
that require them; no card may close a real-VM or replacement claim on default
full verification alone.

**D-PRODUCT-SPLIT1=C**: canonical product ownership is split: `jet` owns the
language/compiler/dev loop, `jetpack` owns packages/env/build substrate, and
`jetos` owns OS workflows. Compatibility shims such as `jet os ...` may route
to the owning binary with a clear teaching/provenance message until release
policy retires them.

**D-ARCH-SOURCE1=A — dissolve the Source/ monolith** *(ratified by
owner 2026-07-12, card #508)*: the root crate reduces to `main.rs` plus
`CmdCompile.rs` (the R5 rustc/ICE edge). New seam crates under the same
I6/path-dependency law: `jet-cli` (verb registry, dispatch, help,
completions, greeting), `jet-repl`, `jet-debug`; dev-server glue joins
`jet-driver`/`jet-cli`. tests/workspace_crates.rs and truthfulness.rs
extend to the new members. Landing order: interactive tiers first, CLI
next, architecture.md updated last, when it is true.

**D-LINTPOLICY1=A — the override law** *(ratified by owner 2026-07-11,
card #505)*: binding on every current and future expert gate.
(1) Warnings and lints never fail a build by default — errors are
reserved for programs Jet cannot compile safely or unambiguously.
(2) Every bypass is spelled at the site or on the command line, never in
hidden config, and lands in the audit record (`jet inspect dossier`,
effect-budget provenance, build facts). (3) Walls are team policy only:
the `policy:` namespace (D-JPK-POLICYSURFACE1) gains
`lints: { deny: [...] }`, joining effect budgets and trust; the list takes
stable snake_case lint names, never diagnostic codes. Host/org policy narrows,
never widens. Memory/type safety (I1) has no override
and is outside this law. Existing gates keep their spellings; behavior
and audit become uniform.

**D-CLI-EMIT1=A — one generated-Rust spelling** *(ratified by owner 2026-07-12, card #512)*: `jet emit --rust <file>` is the sole spelling; the global `--emit-rust` flag leaves the table (unknown-flag teaching error naming the verb).

**D-LSP1 / D-LSP2**: LSP v2 uses one incremental compiler-service query cache
(`crates/jet-queries`) shared by editor requests, with full applicable LSP
3.17 coverage. Every advertised capability must have a named test in
`tests/lsp.rs`.

**D-HL1**: highlighting is generated lexical base plus semantic overlay.
`Syntax.rs` owns all user-typeable tokens; `jet self devtools grammars` regenerates
VS Code/TextMate, tree-sitter, and Zed generated sections, and
`tests/grammar.rs` fails on drift. LSP semantic tokens refine live editors for
ownership (`copy`, `^`, `&`) and markers; retired/foreign spellings are not
colored as live syntax.

**D-RECONCILE-SCOPE1 / D-CANON-SOURCE1**: syntax reconciliation is a strict
repo-wide purge of stale spellings; canonical truth is `Syntax.rs` + this
file, CI-checked.

### Ratified product and runtime architecture — 2026-07-10

These decisions freeze the implementation contracts for their Tower cards.
They do not waive I1–I8, create unlisted syntax, or lower any card's proof bar.

#### jetos baselines, desktop, and identity

**B-E7-DESKTOPNS1=E — one-line desktop swaps**: desktop selection lives under
the ratified full-word tree. `services.desktop.environment` accepts
`.Gnome/.Kde/.Hyprland/.Niri`; `services.desktop.session` accepts
`.Auto/.Wayland/.X11`; `services.desktop.displayManager` accepts
`.Auto/.Gdm/.Sddm/.Greetd`. Session and display manager default to `.Auto`.
The derived display manager is Gnome→Gdm, Kde→Sddm, and Hyprland/Niri→Greetd;
`jet os explain` shows the derivation and winning priority. Invalid combinations
are typed assertions naming both source lines. Combinatorial DE/session enum
names are rejected; every fact keeps one option home.

**B-E7-BASELINE1=D — two fully materialized baselines**: init and the installer
write either a terminal or graphical source template directly into `config.jet`.
There is no hidden baseline import, installed default, or silent reinsertion.
Terminal declares the complete boot/kernel/init/filesystem/user/logging/time/
locale/certificate/network/firewall/package substrate, compatibility utilities,
Fish, Helix, Git, OpenSSH client, curl, rsync, ripgrep, fd, bat, eza, fzf,
zoxide, jq, and btop; no remote server is enabled by default. Graphical adds
GNOME Wayland/GDM, NetworkManager UI, PipeWire/WirePlumber, Bluetooth,
printing/scanning, firmware and power management, portals, polkit, keyring,
fonts/input/automount/notifications, jetos Studio, Firefox, Nautilus, and one
maintained GNOME app for terminal, text, documents, images, media, archives,
calculator, calendar, clocks, disks, screenshots, settings, and Jetpack-backed
software management. Flatpak and alternate stores are opt-in. Every realized
package/unit traces to a `config.jet` source span. Expert removal or replacement
is authoritative: jetos reports obligations and proof failure but never heals
the source behind the user's back.

**B-E7-IDENTITY1=E — calver plus ordered navigation codenames**: jetos releases
use `YY.MM` and an alphabetically ordered aviation-navigation codename, vetted
against existing software names and recorded in a committed canon. First entry:
`26.10 "Apex"`; later names advance alphabetically. `os-release` identifies
only jetos (`NAME=jetos`, `ID=jetos`, `VERSION_ID=26.10`,
`VERSION_CODENAME=apex`, `PRETTY_NAME="jetos 26.10 (Apex)"`); prereleases append
`-pre`. CI forbids upstream NixOS identity strings in `os-release`, boot menus,
and welcome surfaces.

#### Jetpack package-manager law

**D-JPK-NIXENGINE1=D — native compatibility engine**: Jetpack implements Nix
formats and reference behavior natively and ships no Tvix code. Reference Nix
and Tvix are dev/CI differential oracles only. Product paths verify computed
`drvPath`, `outPath`, and `NarHash` against cache metadata and fail closed on
divergence. No evaluator stage reaches a provider path before the pinned corpus
is bit-exact.

**D-JPK-NIXPIN1=A — hermetic reference oracle**: compatibility fixtures use
Nix 2.34.8 at annotated tag object
`b6769c588f60b3e762f73d3a8cf60294df078ccd`, peeled source commit
`f3f1c3c5b8ad91850e0f7c590cf177f7ab022024`, and nixpkgs revision
`b5aa0fbd538984f6e3d201be0005b4463d8b09f8` with `lastModified = 1782723713`
and NAR hash `sha256-oPXCU/SSUokcGaJREHibG1CBX3+s/W7orDWQOZDsEeQ=`. The
supported oracle systems are `x86_64-linux`, `aarch64-linux`,
`x86_64-darwin`, and `aarch64-darwin`. Each system records both the complete
Nix installation NAR hash and exact evaluator executable NAR hash. Missing,
unknown, or mismatched identity blocks fixture generation and acceptance.
Root `flake.lock` updates cannot change this independent manifest.

**D-JPK-SANDBOX2=D — sandbox or substitute**: non-executing copy and prebuilt
verification may proceed directly. Fetched/transitive executable actions require
the strong sandbox; unavailable backends try a trusted substitute or approved
remote builder, then fail. A first-party local action may receive an exact,
digest/capability/reviewer/expiry-bound `jet trust` grant; its outputs remain
private and untrusted and never enter shared publication. CI denies by default.

**D-JPK-EPOCHBOUNDARY1=B — functional package management in Epoch 4,
cross-platform sandbox proof in Epoch 8** *(ratified 2026-07-16)*: #398 remains
in Epoch 8 and is the sole owner of hostile Linux, macOS, and Windows child
confinement proof. #395, #399, #422, #427, #429, #432, and #656 remain Epoch 4
functional work; their sandbox-dependent hostile proof moves to #398 without
duplicate implementation or a #398 blocker. Every Epoch 4 action reports the
actual sandbox class it enforced, and a fallback never reports `sandboxed`.

Epoch 4 exits on 20 functional lanes and may claim functional Jetpack package
management. It may not claim full hostile isolation or complete Nix replacement
parity. Those product claims remain unavailable until #398 passes in Epoch 8.
This boundary changes proof ownership and claim scope only; it does not weaken
D-JPK-SANDBOX2's safe defaults or permit an unproved isolation label.

**D-JPK-MULTIUSER1=D — optional verifying shared-store broker**: per-user,
rootless operation remains the default. `jetpack shared-store install` opts an
administrator into a socket-activated transient broker. The broker re-verifies
digest, signature, and provenance in its own privilege domain, never evaluates
user source, builds under ephemeral sandboxed identities, and promotes objects
only under cache-writer law. Missing broker means transparent per-user operation.

**D-JPK-DYNAMICPLAN1=D — finite staged planning**: a sandboxed plan action may
emit a typed `BuildPlan` fragment only within a declared finite stage bound.
Each fragment is sema-checked, authority-checked, acyclic, canonically hashed,
and locked with its exact inputs for deterministic offline replay. Build steps
cannot read the store or invoke resolution. Imported Nix IFD stays isolated in
the compatibility engine under the same limits.

**D-JPK-PROFILE1=D — one package/user generation engine**: `profile.<name>` is
the single package-generation declaration; `user.<name>` composes package
generations by reference.
`jet profile plan/build/switch/rollback/generations` and `jetos user` share one
identity, atomic-switch, history, collision, and GC-root engine. Composed generations
have one history across both product views; non-jetos platforms retain parity.

**D-JPK-PROFILECOLLISION1=A — exact-path provider map** *(ratified 2026-07-16,
card #425)*: when composed packages provide different files at the same path,
the plan fails and names every contender with its content digest; the
generation's `collisions:` map selects one provider per exact path. A selection
is recorded with all contender digests in the lock; if a contender's file
changes, the pick is stale and refused (exit 2) until re-reviewed.
Byte-identical files deduplicate, directories merge recursively, and
file/directory or symlink-target mismatches always fail — a provider selection
cannot change a path's type. The rule applies identically to `jet profile`,
JetOS systems, tools, and `user.<name>`.

**D-JPK-TRUSTROOT1=D — TUF root plus hybrid publisher identity**: registry
authority uses a toolchain-pinned TUF root with offline threshold keys,
delegations, snapshot/timestamp freshness, consistent snapshots, monotonic
versions, and fail-closed expiry/rollback/bad-clock handling. Public releases
accept a Sigstore identity bundle against a pinned transparency checkpoint or an
offline Ed25519/KMS/HSM publisher signature. Cache builders and executors hold
separate identities with digest-bound SLSA provenance; algorithms use the
crypto-agility seam.

**D-JPK-CACHEAUTH1=D — provenance-bound cache writers**: developers write only
private namespaces with short-lived write credentials. Shared namespaces accept
source-allowlisted builders and signed provenance binding action, output,
platform, sandbox, and policy digests. Consumers verify on every read. Revocation
quarantines objects signed by that builder; promotion requires an approved
rebuild, never relocation or relabeling.

**D-JPK-RESOLUTIONDOMAIN1=D — typed version domains**: package identity is
provider + namespace + name. One version unifies inside each typed domain;
automatic duplication requires a proven build/host/target, platform, runtime,
linkage, or ABI boundary. Distinct majors may coexist only when no type/value
crosses versions. Otherwise E1201 carries the causal proof and smallest fix.
Expert duplication is named, reasoned source policy and appears in lock/audit.

**D-JPK-VARIANT1=D — closed, total variant axes**: native variants use only
role, OS, architecture, runtime/libc, linkage, ABI, artifact kind, and feature
set. Every axis has a context-derived default; matching is exact-then-compatible
under one total order. Ambiguity is an error naming the first distinguishing
axis. Provider facts affect selection only through explicit source mappings;
new native axes require another decision.

**D-JPK-FRESHNESS1=D — 24-hour maturity default**: new third-party versions wait
24 hours from first immutable inclusion in trusted monotonic signed metadata.
Existing exact locks and realized environments do not move. Policy may change
the window per source class; first-party/workspace sources default to zero.
Advisory fixes may receive an audited exact exception bound to
`package#version`, evidence, reviewer, and expiry. The owner's amendment is
absolute: Jet package versions use `package#version`, never `package@version`.

**D-JPK-BUILDSCRIPT1=D — reviewed hooks, always contained**: metadata probing
never executes upstream code. An upstream hook is held until an exact grant binds
package identity, provider/source, script digest, and capabilities; any change
re-prompts with a diff. Approval never removes sandboxing. CI fails with the
exact source-policy fix. Curated feeds are signed and digest-bound, never name
wildcards.

**D-JPK-NIXBASELINE1=D — pinned parity baseline**: Nix 2.34 plus one nixpkgs
commit is the sole oracle. Stable language/store/protocol, flakes, nix-command,
and content-addressed derivations require bit parity. IFD and dynamic/recursive
derivations lower only through finite typed staging. Impure evaluation requires
an explicit capability grant and yields private, non-promotable outputs. Re-pins
require a green differential corpus and decision amendment.

**D-JPK-NIXSTORE1=D — canonical Nix paths without installed Nix**: Hangar owns
bytes/references while Linux projects `/nix/store` through rootless namespaces.
Fallback order is verified read-only host store, audited userspace translation
with degraded non-promotable provenance, then fail closed. macOS uses the narrow
approved helper/broker or fails; Windows uses declared WSL2/VM/remote execution.
Jetpack never creates a root-owned host store. Relocation creates a new identity.

**D-JPK-REMOTE1=D — source grants, host bindings**: reviewable `build.remote`
declares requested maximum capability, trust domain, and fallback; stricter
user/org policy can only narrow it. `jet remote bind/list/remove` owns host
endpoint and typed credential-provider mappings. Repositories and flags cannot
introduce endpoints, keys, or trust roots. `--builder` selects only a previously
bound and granted name; absent bindings mean silent local operation.

**D-JPK-STORECLI1=D — physical verbs under Hangar**: `jet hangar` owns verify,
repair, copy, import, export, dump/restore, and sign. Causal questions stay
`jet explain`/`jet inspect dossier`; `jet clean` remains the sole GC+optimize intent.
Mutations use plan-before-apply and stable JSON. Imports are verified and
quarantined before registration.

**D-JPK-PROVIDERS2=D — direct ecosystem roots**: external ecosystems use
readable direct roots under the one dependency/action/lock model. Selectors
retain version/revision/baseline/features/digest/platform facts; mutable
unhashed refs are rejected. `#version=` is canonical and bare `#2.0.17` is its
shorthand. Each root binds source authority in `policy.providers`; workspaces
may remap roots without changing dependency spellings. Locks record the fully
qualified ref and resolved source.

**D-JPK-REGISTRY1=D — TUF sparse registry and witnessed log**: per-package
sparse signed metadata points to content-addressed blobs. Publish is
transactional with one immutable winner; versions are never reused; signed
yanks stop new selection while exact locks remain resolvable. An append-only
transparency log covers publish, yank, ownership/recovery, and checkpoints.
Clients verify online inclusion, pin offline checkpoints, and reject forks.

**D-JPK-POLICYSURFACE1=D — one source policy namespace**: sources, licenses,
advisories, maturity, hooks, cache roles, trust, providers, replacements, and
exceptions live under the one workspace `policy:` namespace. Safe defaults
apply when absent. Effective policy is the intersection with stronger host/org
policy, which can never be weakened by source. Exceptions require id, exact
package-edge scope, reason, and expiry. `jet policy draft` writes reviewable
source diffs only.

**D-JPK-CACHECONFIG1=D — role-bound cache configuration**: workspace policy
requests cache roles/trust, while `jet cache bind` maps roles to ordered host
mirrors and typed credential providers. Repositories, flags, and environment
cannot introduce endpoints or secrets. The first verifying digest/signature hit
wins; misses build from source; offline mode wins over all. Writes remain a
separate grant.

**D-JPK-STOREBACKEND1=D — native and Nix endpoint adapters**: supported
endpoints are local Hangar, native HTTP, SSH, file, S3-compatible, and Nix
daemon/SSH/file/HTTP. Each declares read/write/remote-execute/trust/credential
capabilities. All transfers lower to verified Jetpack object APIs; Nix crossings
record Jetpack digest plus NarHash/signed fingerprint. Nix URI spellings are
endpoint addresses, not canonical Jetpack vocabulary.

**D-JPK-RESOLVEMODE1=D — one resolution strategy vocabulary**: resolver modes
are `conservative`, `latest`, `lowest`, and `lowest-direct`; named resolver
configurations may bundle these with platform matrices. `jet update <pkg>` defaults
conservative, moves only the named subtree, and records rationale. Realize verbs
never resolve. `jet prove --lens dependencies` emits non-mutating `.jetproof`
matrix artifacts; unrelated lock records remain byte-identical.

**D-JPK-REPROCACHE1=D — preserve divergence as untrusted evidence**: trusted
shared caches accept only policy-reproducible outputs. Divergent bytes,
provenance, and first-difference facts live in `private/unreproducible` and never
satisfy trusted policy. Explicit consumption taints every downstream result.
Promotion requires fresh independent agreeing rebuilds after the fix, never
relabeling stored bytes.

#### Full-stack web, compute, and services

**D-APP-UNIFY1=B — one `App` type across targets**: `fn run() => App` is
the single application return surface for native, web, Wasm, JS, and
freestanding builds. Platform selection comes from the existing `#Target`
marker or the package manifest `target:` field, never from the type name.
Target-sensitive builder capabilities are checked in sema against that
resolved target and report the capability, required target, and resolved
target; all execution tiers then consume the same checked App/TIR contract.

**D-WEBAPP1=D — one sema-known application graph**: sema evaluates the
statically evaluable App-returning `fn run` builder chain into one typed graph. Runtime
registration outside a declared typed `.mount` is a compile diagnostic. Mounts
keep prefix, effects, and security policy static. Browser/server partition,
hydration mismatch, executable TIR, and generated-source re-entry laws apply;
`jet inspect expand --facts web` and `jet explain --web-graph` expose stable JSON facts.

**D-ENTRY-VALUE1=B — the entry returns the program** *(card #1446)*: `fn run`
is the one entry. Its return type says what the program is. No return means a
script that executes and exits. Returning the App-family value, today spelled
`App`, serves that value at the runtime edge; `jet dev` serves the same value
with watch, reload, and hot swap. An explicit `fn dev()` remains the expert
override. The hidden app-name convention, synthesized `run` overlay, and
run/app/dev precedence table are deleted.

**D-APP-UNIFY1=B — one App type, target by marker** *(card #1447)*: every
target uses the same `App` value and builder surface. The resolved `#Target`
marker or manifest target selects platform behavior; the type name never
selects a platform-specific application type. A capability unavailable on the
resolved target is a compile-time error naming the target and the capability.

**D-APP-ARGS1=A — typed CLI inputs reach an App entry** *(card #1450)*:
`fn run(args: T) => App` uses the same checked `T` schema, `core.args` parser,
help text, defaults, and value errors as a typed script entry. Jet constructs
`T`, invokes `run(args)`, and serves the returned `App`; the App return does
not create a second CLI mechanism or change the `#CLI` field mapping. A typed
`Result` return keeps the same entry error boundary before serving its `App`.

**D-WEBAUTHOR1=D — explicit builder with opt-in conventions**: the builder is
always canonical and one file remains complete. File routing activates only
through `.routes(from: "routes")` written in that builder. Every file under the
root maps once, is excluded by leading `_`, or is diagnosed. Explicit and
convention routes collide loudly with both spans. Scaffolds write the opt-in line
up front; directory presence never changes behavior.

**D-COMPUTE1=D — one Core compute family**: `core.compute` owns eager,
fusion, differentiation, placement, and expert device/buffer/stream/kernel work
under one operation family. Graph/kernel forms are internal executable-TIR
stages. External accelerator providers are explicit bridges and must
differentially match Core semantics.

**D-COMPUTE-TYPE1=D — Tensor owner, unified View**: `Tensor<T>` owns ranked
multidimensional storage; sema checks static shapes and runtime shapes use typed
fallible operations. Existing `View<T>` is the sole borrowed strided projection
for host/device memory. `Vec<N>` and `Matrix<M,N>` share the substrate and cross
zero-copy. Differentiability is a transform property, not a second type;
relational tables convert explicitly through audited `to_tensor`.

**D-COMPUTE-PLACE1=D — automatic placement with receipts**: `.Auto` is the
beginner default and carries `GPU` because an accelerator may be selected.
Experts pin device, memory, precision, and transfer policy. Project/deployment
policy may only narrow call-site authority. Transfers, excess allocations, and
fallbacks emit stable receipts; fallback must be named and cannot change
precision, effects, failure shape, or observable results.

**D-COMPUTE-KERNEL1=D — proved safe kernels plus audited raw tier**: sema proves
bounds, alias/race freedom, captures, barrier uniformity, and control flow before
an ordinary Jet function becomes a kernel. Failure names the unmet obligation;
there is no unproved fallback. Atomics/reorderable reductions require recorded
policy. Raw device code is confined to `#Unsafe("reason")` with typed boundary
contracts and differential-test requirements.

**D-COMPUTE-KERNEL-SURFACE1=B — explicit safe-kernel marker**: an ordinary
function becomes a safe kernel only with `#Kernel(.parallel)`. Sema attaches
one proof record covering bounds, aliasing, captures, races, barrier
uniformity, and control flow before TIR. This epoch ships the conservative
read-only, effect-free expression subset; indexed writes, loops, captures, and
opaque/provider calls remain rejected until their proof facts exist. Raw
vendor/device code stays in the typed `#Unsafe` tier.

**D-COMPUTE-AUTODIFF1=D — reverse default, composable transforms**:
`compute.grad`/`value_and_grad` are scalar-loss defaults; `jvp`/`vjp` compose for
mixed and higher-order work. Unsupported mutation/control flow fails at its
source span. Custom derivatives live once at the operation definition, are
type/shape/effect checked, and may carry numerical validation evidence.
Gradient execution inherits primal placement and determinism.

**D-COMPUTE-GRAD1=E — current gradient surface** *(ratified 2026-08-08;
supersedes the public spellings in D-COMPUTE-AUTODIFF1)*: `compute.gradient` is
one name with two arities. With a function and Tensor values it returns the
selected named gradient tuple; with only the function and optional `wrt:` it
returns a derivative function that must be bound before it is called.
`compute.value_and_gradient` follows the same direct and transform arities.
Both forms lower to the same VJP core, and `wrt:` names are checked against the
differentiated function's parameters.
JVP retains the technical transform surface from D-COMPUTE-AUTODIFF1: bind
`d_f :: compute.jvp(f)` before calling it with primal and tangent values.
The differentiated function must be pure over its Tensor arguments; its effect
row carries through the bound transform unchanged.

**D-COMPUTE-VJP1=A — VJP run value and pull** *(ratified 2026-08-08)*:
`compute.vjp(f, args...)` returns a `VjpRun` with `.value`, `.pull(seed)`, and
`.grads`. `.grads` is the unit-seed pull for scalar output; non-scalar output
requires an explicit seed. A `VjpRun` also destructures positionally as
`(value, pull)`.

**D-COMPUTE-BACKEND1=D — portable policies and CPU oracle**: default compute
policy is F32Strict + Reproducible. Fast math, reassociation, and nondeterministic
reductions require named recorded policies. Typed capability negotiation fails
before launch. Every tier backend differentially conforms to the CPU oracle;
dev and AOT use the same backend, policy, and cache identity.

**D-COMPUTE-RAWBOUNDARY1=A — provider-issued opaque raw-kernel contract**
*(ratified 2026-08-03)*: a raw-device contract is minted only by a trusted
provider after it checks the kernel metadata and differential reference. The
contract is opaque to safe Jet, is accepted only inside `#Unsafe`, and is
carried through TIR into the launch receipt. A reason string, arity, or ordinary
record is never a proof. With no raw-device provider in the Epoch 3 CPU
policy, the built-in constructor fails closed; it must not fabricate a token.

**D-SERVICE1=D — sema-known structured service tree**: typed builders promote
ordinary functions into named workers/groups; sema validates topology, endpoint
types, effects, cycles, and lifetimes. Each group is a supervisor-owned child
taskgroup. Beginner default is bounded OneForOne restart with parent escalation.
Deployment may place/scale the declared graph but never invent children.

**D-SERVICE-DELIVERY1=D — at-most-once default, proved durable retry**: live
calls are at-most-once with per-sender FIFO. Full mailboxes wait under deadline
or return `Full`; timeout after send returns `Ambiguous`, never retries silently.
DurableAtLeastOnce requires typed idempotency key, dedup/transaction contract,
retention, typed receipt, and handled dead-letter endpoint. Exactly-once is never
claimed. Mailbox pressure is classified by the [Bounded buffering law](spec.md#bounded-buffering-law).

**D-SERVICE-STATE1=D — explicit state adapters and one commit point**: services
restart empty unless they declare `.Snapshot` or `.EventLog`; both reuse schema
migrations and decode provenance. Durability occurs only at explicit snapshot
commit or event append. Corrupt/newer state refuses start with recovery guidance;
restart-empty is explicit policy only. Storage authority is injected.

**D-SERVICE-WORKFLOW1=D — effect-checked deterministic workflows**: workflow
bodies reject ambient time, randomness, I/O, channels, and free task spawn and
name recorded equivalents. Activities own effects, idempotency, and retry.
Histories are workflow id + run id and bounded through explicit continuation.
Version markers gate branch changes; incompatible deployment is refused against
the affected live histories.

**D-SERVICE-IDENTITY1=D — signed generational directories**: callers receive
typed endpoint capabilities from signed, generation-versioned directory
snapshots projected from source trust policy. Resolution carries generation and
staleness bounds; partitions, revocation, and expiry are typed results with no
ambient DNS fallback. Rotation overlaps generations. Explicit outside-graph
`service.connect` requires an audited trust grant.

**D-SERVICE-UPGRADE1=D — shard-scoped proved handoff**: deployment starts and
checks the new generation, drains, and switches atomically per routing shard;
global convergence is observed, not called atomic. Migrations are
Reversible/DualWrite/ForwardOnly with explicit rollback facts. Drain overruns use
declared PinShard or Cancel policy. Partitioned shards retain their last allowed
generation and reconcile through the proof-gated rollout object.

#### Formatter, profiler, and notebooks

**D-FMTPROJECT1=D — project formatter contract**: `jet fmt` discovers
workspace/package/cwd scope, accepts explicit paths, `--dry-run`, `--check --diff`,
`--changed`, and stdin. Exit 0 means clean/formatted, 1 means check differences,
and 2 means usage/parse/I/O failure. Preflight finds all failures before a
zero-write abort. `jet fmt - --stdin-path=...` gives editor-equivalent stdin
diagnostics. CLI, LSP, Canvas, and CI output is one byte-identical fixpoint.

**D-PERFSESSION1=D / D-ARTIFACT-EXT1=A — one `.jettrace` truth**: `jet perf run/test/bench` preserves
the exact base-command argument surface and driver path; `attach/view/compare/
export` complete the family. `.jettrace` embeds schema/toolchain/source/source-map
identity and capture policy. Compare enforces hardware/toolchain baseline
identity. pprof/OTel/Chrome and profile maps are projections only; generated
Rust frames stay hidden unless explicitly requested.

**D-NOTEBOOK-SURFACE1=D — one first-party client plus enforcing Jupyter
adapter**: `jet notebook PATH` is loopback-only with fragment bearer token;
non-loopback needs explicit bind/auth. Shared Canvas/Studio renderer, sandbox,
receipt, and proof components back the client, and Canvas notebook view opens
the same session. Jupyter projection recomputes stale turns, confirms effects,
and never displays output the first-party client would reject.

**D-NOTEBOOK-DOC1=D — mergeable `.jetnb` source truth**: v1 stores environment
identity, CSPRNG cell IDs, merge-by-ID facts, and an output cache keyed by cell
source + environment + transitive dependency closure. Upstream edits invalidate
all downstream outputs. Paste/duplicate mints IDs; ipynb 4.5+ IDs round-trip
deterministically with exact loss reports. Git merge/textconv is opt-in;
`.jet` export is a stated-loss projection.

**D-NOTEBOOK-TRUST1=D — passive-equivalent sandbox plus unified trust**:
sanitized output and zero-capability opaque-origin widgets render without a
prompt. Capability widgets bind grants to source, payload, renderer, locked
environment, and policy version in the unified `jet trust` graph. Beginner
intent cards summarize; expert drawers show exact cell hashes/origins/messages.
Imported ipynb output is quarantined, grants stay local, and any relevant change
revokes to safe text fallback.

### Ratified product and runtime architecture — 2026-07-10 (batch 2)

**D-CLI-SURFACE1=B — frequency-ringed jet command surface** *(as amended by
D-CLI-STORE2, D-CLI-DEVSERVE1, D-CLI-SURFACE3, 2026-07-11)*: the daily
dev-loop verbs (`run`, `build`, `test`, `check`, `fix`, `new`, `init`, `add`,
`remove`, `update`, `fmt`, `lint`, `dev`, `repl`, `debug`, `bench`,
`eval`, `emit`, `explain`, `help`, `version`, plus D-CLI-SURFACE3's `env`,
`fetch`, `search`, `info`, `outdated`, `clean`) stay flat and top-level. The
long tail lives under groups on the jet binary: `jet registry`
(publish, yank, keygen, key backup, vendor), `jet inspect` (graph, query,
explain-build, impact, dossier, semindex, expand, schema, codemod, audit,
sbom, bind, plus D-CLI-SURFACE3's logs), `jet hangar` (physical store verbs
per D-CLI-STORE2), `jet self` (toolchain, upgrade, doctor, completions, man,
devtools). The bare ungrouped spelling of a moved verb is a teaching error
naming the grouped form, never a silent alias (I8).

**D-SHAPE6=A — one grouped command grammar** *(ratified 2026-07-14, card
#541)*: tool families use noun then verb, including `jet inspect dossier` and
`jet registry publish`; daily lifecycle verbs remain flat. Bare moved actions
are teaching errors naming the grouped route, never aliases. Help, completions,
man pages, typo suggestions, and dispatch consume one command registry.

**D-CLI-STORE2=A — hangar is the store noun** *(ratified 2026-07-11, card
#497)*: `jet hangar` owns every physical store verb — verify, repair, copy,
import, export, dump/restore, sign, rollback, generations, du. `jet clean`
stays the sole GC+optimize intent. The `jet store` group is dissolved:
`jet fetch` is a flat daily verb, script locking is `jet fetch --lock
<script.jet>`, and `jet store …` / bare `jet gc` are teaching errors naming
the real spelling. Supersedes D-CLI-SURFACE1's `jet store` rows.

**D-CLI-HANGAR1=B — one Hangar archive format, seven operational views**
*(ratified 2026-07-13, card #517)*: `export` writes one canonical, signed,
self-describing `.hangar` archive, including the closure by default and only
the selected object with `--no-deps`; `import` verifies it in quarantine before
registration. `dump` and `restore` stream those same archive bytes through
stdout/stdin. `copy` fuses export, transport, and import over an endpoint.
`repair` verifies, then re-realizes or re-fetches corrupt objects. `sign`
(re)signs an object or archive. The shared archive format is a stability
commitment: it carries a version field and has a compatibility rule from day
one. This refines D-JPK-STORECLI1=D and
D-CLI-STORE2=A while explicitly superseding any interpretation of
`dump`/`restore` as a second raw single-object serialization format: Hangar has
one archive backbone, not separate dump and export formats.

**D-CLI-DEVSERVE1=A — `serve` deleted** *(ratified 2026-07-11, card #497)*:
`jet dev` is the only dev loop (auto-detects rerun vs resident hot-swap;
`--restart`/`--swap` overrides, per D-DEV4). `jet serve` is a teaching error
naming `jet dev` (hot-swap: `--swap`). The word stays unclaimed for a future
ratified job.

**D-CLI-SURFACE3=B — every verb stays on jet, grouped** *(ratified
2026-07-11, card #497)*: no verb leaves the jet binary. The four silent
aliases die (`doctor`/`devtools`/`toolchain` → teaching errors naming
`jet self …`; `gc` → teaching error naming `jet clean`). `env`, `fetch`,
`clean` join the flat ring; `search`, `info`, `logs`, `outdated` group under
`jet inspect`; `push`, `bridge`, `services`, `image`, `config` group under
`jet os`. `jet trust` (D-JPK-GRANTCMD1) and `jet os` (D-JPK-OSVERB1) stay
as ratified.

**D-CLI-BARE1=A — bare project verbs** *(ratified 2026-07-11, card #497)*:
one shared entry-resolution rule makes `run`, `dev`, `debug`, `bench`,
`check`, and `build` bare-capable inside a package: the entry resolves via
`targets:`/D-ILE1; ambiguity is an error listing the targets (pick with
`-p <member>` or an explicit file); outside a package the bare form stays
the current usage error. An explicit file argument always wins.

**D-CLI-SURFACE2=A**: `jet fuzz` remains flat beside testing. The language
server is canonically `jet self lsp`; first-party editors launch that argv.
Bare `jet self lsp` is E2101 before external-command discovery, preserves following
argv in its replacement, exits 2, and never starts a server. Help, palette,
man pages, and completions advertise only canonical grouped spellings.

**D-JPK-TASKRUN1=A — named entries are `#Job fn`**: a job is an ordinary Jet
function marked `#Job`, living beside `fn run()`. Reuses typed-argument CLI
parsing (D-CLIFLAG1) and `?` fallibility; a cross-job dependency is a plain
function call, no separate DAG syntax. Invoked canonically with
`jet run <entry> -- <name>`; `jetpack run <name>` is the Jetpack engine bridge.
`run`, `dev`, `build`, `test` remain reserved lifecycle verb names a job
cannot reuse.

**D-JOB-NAME1=A — one word for named auxiliary entries** *(ratified
2026-08-05, card #1448)*: the marker and every command use `job`. The
canonical CLI is `jet run <entry> -- <name>` and `jet jobs`; retired spellings
have no alias or fallback. Help, completions, diagnostics, docs, examples, and
tests use the same vocabulary.

**D-JOB-SUBCMD1=C — jobs are argv subcommands**: every built binary exposes
its `#Job` functions through one canonical subcommand table. Bare `#Job` and
`#Job(.Dev)` jobs are available in the dev tiers; `#Job(.Ship)` jobs are
available in dev and release binaries; `#Job(.Internal)` jobs are dev-only.
The first program word selects a job before the ordinary `fn run` CLI parser;
`jet run <entry> -- <name> [args…]` uses the same selection. Job names are
reserved against built-in lifecycle/CLI names and collisions at one scope.
`.Internal` jobs remain callable from code and schedulers, never from argv.
There are no flag aliases, and a release binary reports non-shipped jobs as
unknown commands.

**D-SCHEDULE1=A — schedule-as-code** *(ratified by owner 2026-07-11, card
#505)*: `#Every(…)` is a directive marker on a `#Job fn`
(D-JPK-TASKRUN1). `#Every(5min)` takes a duration literal (D-UNITLIT1);
`#Every("03:00")` takes a daily wall-clock time; both are
compile-checked. One declaration feeds every consumer: `jet dev` runs
due jobs in the dev loop, the service runtime (D-SERVICE1) schedules
them in production, a jetos generation projects them as timer units.
Complex calendars (cron expressions, timezones, jitter) stay with the
runtime API or jetos timers; operator-side cadence overrides live at the
jetos/service layer with explain provenance.

*Shipped 2026-07-12 (card #505, slice 2)*: `#Job fn` (D-JPK-TASKRUN1) and
`#Every(…)` parse, placement-check (E0925), and value-check (E0926); `jet
dev`'s watch loop runs due jobs on their own schedule (UTC for
`#Every("HH:MM")` — timezone-aware calendars stay the jetos/service tier's
job per this same law).

*Shipped 2026-07-12 (card #476)*: reserved-lifecycle reject on `#Job fn
run|dev|build|test` (E0928); `jet run <entry> -- <name>` dispatches an
`#Job fn`, and the Jetpack engine bridges `jetpack run <name>` to that path
(D-JPK-DISPATCH1); unknown names list declared jobs (E1294). Typed job
args reuse D-CLIFLAG1 once the job is selected. The one dispatch table keeps
the selected function's source name, so a sibling's plain-call dependency
(ballot: dependency = plain function call) does not die with E0102.
D-SERVICE1 still has no typed builder/worker/group to carry a schedule into a
service runtime — that remains a future card, not a corner cut here.

**D-JPK-TOOLRUN1=A — unified `jetpack tool` noun**: `jetpack tool run <ref>`
executes a package binary ephemerally across all providers (generalizing the
nix-only `jetpack run pkg@nixpkgs` bridge); `jetpack tool install <ref>`
adds it to the user's default generation (D-JPK-PROFILE1) and projects its
bins onto PATH as its own generation; `jetpack tool list`/`uninstall`
manage them. A name collision with a project-local job (D-JPK-TASKRUN1)
is a checked error naming both.

*Shipped 2026-07-12 (card #477)*: `jetpack tool run|install|list|uninstall`
CLI surface. Built-in providers (`nixpkgs`/`github`/bare paths) realize through
the existing hangar path; recognized external prefixes (`npm`/`pypi`/`cargo`/…)
emit E1298 (JPK-TOOL-PROVIDER) instead of silent skip. `tool install` writes
real symlinks under `~/.jet/bin` plus generation metadata at
`~/.jet/tools/generations/<n>/` (profile `"tools"`) — a minimal isolated
install until the shared D-JPK-PROFILE1 `jet profile` front door is the
caller. Bin/`#Job fn` collision is E1297 (JPK-TOOL-COLLIDE).

**D-JPK-PKGOVERRIDE1=B — keyed override record inside `overlay`**: a
package's version/flags/env/patch overrides live under one
`overrides: { <pkg>: .{ … } }` typed record per overlay (D-JPK-OVERLAY1),
using the standard `.{ }` construction. Patches are file references
(`patch("...")`), never inline diffs. Conflicting overlays resolve through
the same named-tier/`Priority(n)` mechanism ratified for jetos
(D-JOS-PRIORITY1) — one conflict model shared by jetpack and jetos.

**D-JPK-SELECTOR1=C — explicit + computed workspace selection, no pattern
DSL**: `jet build/test/run -p <member>` (repeatable, exact name, cargo-style)
selects workspace members explicitly; `--affected[-since <ref>]` computes
the changed-member set (plus dependents, always included) from the action
cache's existing input-hash keys (D-BUILDCACHE1). No glob/dependency-modifier
pattern language.

**D-JOS-NETWORK1=B — inline `net.*` in `system.<host>`, reusable named
tunnels/printers**: host-bound network state (`net.hostname`, `net.wifi`,
`net.firewall`, `net.bluetooth`) stays inline in `system.<host>`, matching
existing convention. Portable pieces are name-referenced modules —
`vpn.<name>`, `printing.<name>` — reused across hosts, matching the
`user.<name>`/`theme.<name>` convention. `net.firewall` accepts a typed
`Firewall.nftables` family for expert rule sets. Zero-declaration default:
NM+DHCP, deny-inbound firewall, CUPS+Avahi discovery, bluetooth on.

**D-JOS-OPTIONSVERB1=B — reuse `jet search`/`jet info`, no new verb**:
jetos option search/browse (search.nixos.org parity) rides the existing
ratified `jet search`/`jet info` discovery verbs (D-JPK-DISCOVER1) rather
than minting a dedicated `jet os options` verb.

**D-JOS-BACKUP1=C — backup as a `workload.<name>` backend**: user-data
backup/snapshot reuses the `workload.<name>` mechanism ratified for
Container/MicroVM (D-JOS-CONTAINER1); the backend enum gains `.Snapshot`
and `.EncryptedRemote`. Restore goes through the existing `jet os rollback`
verb — no new CLI surface. Shares mounts/secrets/health/resources/proof
fields with every other workload kind.

**D-JOS-APPSTORE1=D — hybrid storefront inside the one Studio shell**: the
beginner app-store view is a first-class Apps view inside Studio (not a
second GUI app, not Studio-external): full storefront browse (featured,
categories, screenshots), one-click Install writes the declarative
`user.<name>.packages` diff and applies through the generation engine
(D-JOS-USERAPPLY1) — never an imperative side-channel — with the exact
generated source diff visible on demand ("View source"), never forced.

**D-JOS-APPMODULE1=B — one `apps` record, one field per app**: `user.<name>`
carries a single `apps: .{ … }` record; each known app gets one lowercase
field, its config type inferred from the field (`.{ }`, D-DOTCTOR2) — the
type name never appears. Every app's config supports standard `package:`,
`extraConfig:` (verbatim passthrough), and `files:` fields. Presence in the
record means installed and configured; no `enable:` ceremony.

**D-ECO1=A — one typed Jet project graph, source to machine** *(ratified
2026-07-15)*: packages, development setups, checks, services, images, and
machines are parts of one typed Jet value; a machine points directly at the
package it runs, and every link is checked before building. The same graph
powers run, test, build, explain, image, and OS commands (`jet explain
systems.<host>` answers from machine back to source). The owner's
ratification comment asked for a better root noun than "project" — honored by
D-ECO-ROOTNAME1=I below: the root is `Package`.

**D-ECO-ROOTNAME1=I — the ecosystem root is `Package`** *(ratified
2026-07-15)*: `Package` is the one noun for the complete graph from a single
package through a monorepo and fleet. A root may list flat `members:` by
reference, member depth is capped at one, and members cannot list members.

**D-ECO-SLICENAME1=G — typed contributions are `Config` values** *(ratified
2026-07-15)*: `Config` names one layout-neutral typed contribution of Package
facts. Modules hold code, Packages ship things, and Configs merge settings.

**D-ECO-FILEROOT1=A — `package.jet` is the single reserved ecosystem file**
*(ratified 2026-07-15)*: bare top-level fields in `package.jet` construct the
Package; no wrapper binding is required. `pkg.jet`, `env.jet`, `workspace.jet`,
and JetOS `config.jet` fold into it through one teaching diagnostic and one
migration epoch; a leading `_` disables a discovered `.jet` file.

**D-ECO-SPLITPOLICY1=A — `jet split` extracts and moves closed facts**
*(ratified 2026-07-15)*: splitting inline Package facts creates the equivalent
Config, previews its binding before writes, and records enough provenance for
exact reversal. A non-closed extraction refuses before changing files.

**D-ECO-TRANSITION1=A — growth uses `jet split`; reversal uses `jet fold`**
*(ratified 2026-07-15)*: both commands preserve graph identity through one
reversible provenance ledger. `jet split env`, `jet split package <name>`, and
`jet split hosts <name>` use the same transition law.

**D-ECO-OUTPUT-PAYLOAD1=A — Outputs are thin projections** *(ratified
2026-07-15)*: an Output payload stores only its name and kind-specific facts;
sources, dependencies, actions, effects, policy, target facts, and provenance
remain single-copy graph facts. `jet inspect output <address>` reconstructs the
complete path.

**D-ECO-OUTPUT-KINDS1=A — Output has nine closed kinds** *(ratified
2026-07-15)*: the exact set is `Library`, `Executable`, `Service`, `Check`,
`Environment`, `Image`, `Bundle`, `System`, and `Fleet`. Arbitrary text kinds
are rejected; another kind requires ratification.

**D-SHAPE-OUTPUT-CALLABLE1=A — runnable Outputs hold checked function
references** *(ratified 2026-07-15)*: `entry: run` refers to ordinary Jet code
through normal name resolution, visibility, rename, provenance, and role
validation. Text entry names and mandatory wrapper modules are not alternate
paths.

**D-ECO-OUTPUT-CALLCONTRACT1=A — runnable roles use distinct ordinary-function
contracts** *(ratified 2026-07-15)*: Executable parameters derive typed command
flags; Service and Check entries accept no ad hoc invocation flags. Normal
return means success, and `?` carries failure without lifecycle result types.

**D-ECO-OUTPUT-DEFAULT1=A — plural runs all; singular uses a five-step rule**
*(ratified 2026-07-15)*: `jet test` runs every Check Output. Singular intents
select in order: explicit address, legacy zero-config entry, sole compatible
Output, checked `defaults:` entry, then an ambiguity error listing choices.
`jet run --output api package.jet` is the explicit form; `defaults: .{ run: app };`
is checked against the Package's Executable Output addresses. Explicit
selection wins even when a legacy `fn run` exists, but accepts only an
Executable or Service; Check Outputs remain plural under `jet test`. Defaults
are validated even when a sole Output or legacy `fn run` would otherwise win.

**D-SHAPE-INTERNAL1=A — `pub _name` is soft-public** *(ratified 2026-07-15)*:
outside use is allowed with one unsuppressible warning, but the name is omitted
from beginner discovery and carries no supported-API or semver promise.

**D-SHAPE-DUNDER2=A — `__name` belongs to Jet** *(ratified 2026-07-15)*:
every source-written double-underscore identifier is rejected. The namespace is
reserved for compiler-generated binders, debugger and serializer metadata, and
tools; user code has no escape spelling.

**D-NAME-SIGIL1=A — one underscore ladder and `__jet` machine names**
*(ratified 2026-08-07)*: zero leading underscores is an ordinary name; one
leading underscore is the human-facing discard/internal rung; two leading
underscores are machine space and have no user meaning. Dunder and sunder
shapes are intentionally meaningless. Every compiler-generated symbol visible
to generated-code review, diagnostics, traces, debuggers, serializers, or other
tools begins with the single reserved `__jet_` prefix (for example,
`__jet_lambda_7`). There is no source escape for that namespace.

**D-ECO-JETOS2=A — Systems and Fleets are Outputs of the Package graph**
*(ratified 2026-07-15)*: Package, environment, image, System, and Fleet share
locked identity, policy, cache, explanation, and receipts. JetOS consumes that
graph while retaining its separate assembly and activation engine.

**D-ECO-JETOS-PREVIEW1=A — plan predicts; proof confirms against a baseline**
*(ratified 2026-07-15)*: `jet os plan <host>` previews the candidate delta.
`jet os proof <host> --name <generation>` preserves the exact built delta from
its captured parent generation with output digests, readiness, activation,
provenance, and rollback evidence.

**D-ECO-RECEIPTSTORE1=A — receipts are immutable Hangar objects** *(ratified
2026-07-15)*: `.jet/lock` or a generation references each receipt by digest;
the receipt points back to locked inputs without duplicating merge history.
Inspection and export load or copy the receipt closure from the Hangar.

**D-ECO-FLEETVERB1=A — fleet rollout is `jet deploy <fleet>`** *(ratified
2026-07-15)*: `deploy` owns plan, staged rollout, observation, and rollback for
Fleet Outputs. It supersedes D-CLI-SURFACE3's `jet os push` grouping and leaves
`push` unclaimed.

**D-JPK-MANUALROOT1=B — external retention uses explicit root verbs**
*(ratified 2026-07-15)*: the exact commands are
`jet hangar register-external-root`, `jet hangar unregister-external-root`, and
`jet hangar list-external-roots`. They retain closures lacking any automatic
Package, package/user generation, process, build, toolchain, System, or Generation owner.

**D-ECO-HANGARPATH1=A — Hangar defaults to native per-user data paths**
*(ratified 2026-07-15)*: Linux uses `$XDG_DATA_HOME/jet/hangar` or
`~/.local/share/jet/hangar`, macOS uses
`~/Library/Application Support/Jet/Hangar`, and Windows uses
`%LOCALAPPDATA%\\Jet\\Hangar`. `jet hangar path` reports the resolved path;
shared storage remains an administrator opt-in.

**D-ECO-BROKERBOUNDARY1=A — shared storage uses a transient verifier**
*(ratified 2026-07-15)*: the optional administrator-installed broker is
socket-activated, exits when idle, never evaluates user source, rebuilds only
under ephemeral sandbox identities, and re-verifies bytes, signatures,
provenance, and writer authority before promotion.

**D-ECO-MEMBERS1=A — Package membership is flat** *(ratified 2026-07-15)*: a
monorepo root's `members:` field contains references to independent Packages;
members cannot have members. A single Package needs no `packages:` or
`members:` field.

### Superseded & deferred IDs (tombstones)

**S6 — semicolons**: superseded by S6-R (see Formatting).
**S10 — ownership keywords**: superseded by D-CAP7 sigils (see Capabilities).
**S24 — `when` dispatch**: superseded by D-IF1/D-IF3 `if … == { }` (see
Control flow).
**S25 — comparison distribution**: retired by D-S25-RETIRE1; use `|`.
**S29 — dotless struct literal**: superseded by D-DOTCTOR2 `T.{ }` (E0320).
**S35 — `or` fallback**: superseded by `??` (S71).
**S43 — `test` blocks**: superseded by `#Test("name")` (see Testing).
**S53 — concurrency**: superseded as a deferral. Concurrency is live ratified
law; see the D-CONC entries above. Implementation remains on its own cards.
**S81 — `?continue`**: superseded by `expr ?? next` (D-LOOP-CONTROLWORD1=B).
**D-MARKER-FAMILY1 / D-MARKERMOVE1/2/3 — two-plane `@`/`#` sigil law**:
superseded by D-VERDICT-732-1; every marker is back on `#`, `@` is reserved
for locations/sources.
**D-CTMARKER1 — `$name` splice marker**: retired outright by D-META-STAGE1=B;
`$` is the one compile-time mark, not a splice-only sigil.
**U1 / U10 filenames, D-JPK3/8/13, D-BIND1/2, D-ATTR1/3, D-CAP1/2-words,
D-JSONOUT1, D-LITSUFFIX-SCOPE, D-UNIT1-spelling, the bare-brace constructor
spelling superseded by D-DOTCTOR2**: all
superseded by the entries above; law as written in this file is final.

## Enforcement

Ratified decisions are **frozen**. `cargo test` runs `tests/decisions.rs`,
which fails if:

- any `Syntax.rs` entry is `(provisional)` while ratified in this file;
- any open or deferred decision ID appears in `Syntax.rs`;
- the Provisional table below lists a real decision ID;
- a staged decision loses its pinned error code in docs/spec/diagnostics.md.

Agents: after ratifying a row, update `Syntax.rs` to `(ratified)`, clear the
Provisional table row, and add a ui snapshot if behavior changes.

## Staged implementation (ratified syntax, milestone pending)

Syntax and semantics below are **decided** — do not re-litigate. Only the
implementation milestone is pending.


| ID  | Milestone | Enforcement today                                                | Code  |
| --- | --------- | ---------------------------------------------------------------- | ----- |
| S15 | M6        | default unwind in `Source/main.rs`; `--small` + `panic=abort` in M6 | —     |


## Provisional — currently in the code


| ID  | Choice in code                         | Where |
| --- | -------------------------------------- | ----- |
| —   | *(none — Group 1 ratified 2026-06-11)* |       |


## Open decisions — owner input needed

> New decisions are ballot cards in Tower (.tower/tower.json); this table
> is the registry of open language-surface questions.

### Registered for M3–M14

#### Ready for one decision now

*(none)*

Blocked follow-ups stay on Tower planning cards and remain outside the owner
queue. They enter this table only after their blockers resolve and the ballot
passes independent review.


## Decision log

The dated per-decision log (2026-06-10 → 2026-07-02, ~350 rows) was folded
into the current-law entries above on 2026-07-02. The full history — every
amendment chain, ballot narrative, and superseded spelling — lives in the git
history of this file (`git log -p docs/spec/syntax-decisions.md`, up to
commit bfe18d43 and its ancestors). New ratifications append their law to the
topical sections above; they do not restart a log here.
**D-OPDEF1 — User-defined operators**: option A. Existing arithmetic and
comparison symbols may dispatch through ordinary hook traits: `Add.add`,
`Sub.sub`, `Mul.mul`, `Div.div`, `Equatable.equal`, and
`Comparable.compare => Ordering`. Implementations use canonical
`impl Type.Trait { ... }` syntax, keep the same operand type, and cannot add
symbols, precedence, overload sets, or side-effect meanings. `+=` reuses `Add`.
Beginner `#Numeric`/`#Comparable` and auto equality remain front doors to the
same capabilities; built-in numeric, lane, and linalg behavior is unchanged.

**D-CMP3WAY1=B — Three-way comparison** *(ratified 2026-08-13)*: ship the
spaceship operator `<=>` as sugar over `Comparable.compare`, returning the core
`Ordering` type with `Less`, `Equal`, and `Greater` variants. `Ordering.then`
keeps the first non-`Equal` result; `Ordering.reverse` swaps `Less` and
`Greater`. A user type that defines `compare(self, other)` derives `<`, `<=`,
`==`, `!=`, `>=`, and `>`. The lexer chooses `<=>` before `<=` by longest
match.

**2026-07-17 — D-LOOP-HEADER2=A, D-LOOP-ADVANCE2=A,
D-LOOP-CONTINUE2=A, D-LOOP-CONTROLWORD1=B**: loop-only `in`, loop-only `step`,
and source `continue` retired. Semicolon clauses now name every multi-part loop
header; controller-specific advancement is the third clause; `next` enters that
edge before retesting. Implemented end to end on card #681.

**2026-07-24 — D-DOTCTOR3=A**: universal typed-literal head `Type.{ body }` for
every type. Body elaborates against the head with no silent conversion;
`[T].{}` is the explicit empty; bare `[]` stays contextual. Amends S29 /
D-DOTCTOR1 / D-DOTCTOR2 and D-EMPTYLIT1. Implemented on card #780.

**2026-07-24 — D-BIND-BARE1=A**: bindings are always bare — only
`name :: value`, `name := value`, and reassignment `name = value`. Retires
`name: Type :: expr` and `name: Type := expr` as an immediate clean break
(ordinary parse error, no teaching window). Types ride the value
(`Type.{ … }` from D-DOTCTOR3) or live on signatures and fields. Amends S2 /
D-BIND4. Implemented on card #781.

**2026-07-25 — D-FLOWTYPE1=A**: a direct immutable local or parameter of type
`T?` narrows to `T` after `x != None` (true branch) or `x == None` (false
branch). Facts reach `&&` tails, not `||`, and end at the branch boundary.
Mutable storage, fields, indexes, aliases, and calls stay un-narrowed; bind
with `x == Val(v)` there. Sema desugars to S31 Present/`IfLet` so codegen stays
proof-free. Implemented on card #746.

**2026-07-25 — D-UNIONTYPE1=A**: `A | B` is closed structural enum sugar, not a
second sum mechanism. Nested unions flatten, duplicates drop, and identity is
order-insensitive with a deterministic canonical spelling and codegen enum.
`T ? E1 | E2` parses as `T ? (E1 | E2)`. A member widens into its union only at
binding, argument, return, Codable field, and `?` error-propagation boundaries.
Match arms name member types (`.Int(n)`, `.String(s)`). Codable decode dispatches
by primary wire shape and rejects ambiguous shapes (E2415). Named enums stay the
documenting form; union↔named-enum crossings stay explicit match conversions.
D-ANY-JAI1 stays intact — no Any or cast escape. Implemented on card #744.

**2026-07-24 — D-UNINIT-SENTINEL2=A**: `uninit` is legal only as the whole
body of a typed literal — `name := Type.{ uninit }`. Amends D-UNINIT-SENTINEL1
(retires `name: Type := uninit`). Flow proof E0420/E0423/E0424 and
`use core.mem` unchanged. Implemented on card #782.

**2026-07-19 — D-SHAPE-OUTPUT-CALLABLE1=A,
D-ECO-OUTPUT-CALLCONTRACT1=A**: typed runnable Outputs now parse and format as
closed Output values, sema resolves `entry:` through ordinary function
resolution and exact role contracts, and the resolved callable identity feeds
AOT, dev/JIT, semantic navigation, solved effects, safe authority, inspection,
and Canvas. Checked defaults and explicit addresses select Executable or
Service entries; `jet test` runs every Check. E1321 owns stale, mismatched, and
ambiguous links. Implemented end to end on card #544.

**2026-07-28 — D-TRAILBLOCK2=A**: code-as-argument uses explicit `() => { … }` inside call parentheses (multiline bodies and multiple code args allowed); retires D-TRAILBLOCK1 trailing `{ }` sugar (`twice { … }` / `f() { … }`). E0335 teaches the `() =>` form. Card #1266.

**2026-07-28 — D-UNIFYLIT1=A**: typed heads only for domain text (`SQL`/`HTML`/`Sh.{"…"}` + `.raw`); byte patterns `[U8].{"…"}`; retires `sql"`/`html"`/`sh"` prefixes, silent expected-type rewrite of bare quotes, and `b"…"`; text patterns keep plain `"…"` convenience. Amends D-TYPEDTEXT1/2 and D-BINPAT1; supersedes D-LITERAL-PREFIX1 prefix surface. Card #1265.

**2026-07-29 — D-FACTMODEL1=A**: tags, states, taint kinds, and effect leaves
are compile-time enum facts. They share one registry, segment-aware
subsumption, and TypeInfo reflection. Facts classify and erase. They never
select runtime types or dispatch. Card #1294.

**2026-07-29 — D-TAG-SURFACE1=A** *(amends D-TAG-UNIFY1)*: a tag declaration
has a required non-empty `deny` list and optional `from` list:
`tag PII { deny: [Log, Net], from: [DB.read] }`. Direct `#PII` prefixes apply
the fact to types, fields, returns, and expressions. `#Scrub(PII)` removes
exactly that tag and its function signature must consume a tagged value and
return an untagged value. `Input`, `PII`, `Secret`, and `Credential` are Prelude
tag declarations. Bare `tag Name`, strict tag modes, `#Tainted`, and
`#Sanitizer` are retired. Cards #1294 and #1295.

**2026-07-29 — D-STATE-NS1=A**: each state declaration reserves
`Type.State.Name`. Bare `Name` remains sugar inside `#State` and `#Transition`;
qualified spelling is accepted there and is required for unambiguous
reflection. A state path in value position is an error because state facts
erase. Card #1296.

**2026-07-29 — D-RULEARG-TYPES1=A, D-LANGNS-NAME1=A**: every nonprimitive
marker argument type is a generated ordinary enum under `core.lang`. The
marker registry generates exactly fourteen declarations and supplies parser,
E0930, explain, hover, completion, documentation, and reflection. Dot literals
use the expected enum type; importing `core.lang` is optional for them. Card
#1293.

**2026-07-29 — D-MARKER-NAME-HYGIENE1=A**: PascalCase type-site markers share
the trait/derive namespace. Serde internal tagging is
`#Discriminant("field")`, and a scheduled project entry is `#Job fn`.
`#Tag` and `#Task` are retired with fixes. D-TAG1 prose calls nested enum
families “variant groups.” Card #1300.

**2026-07-29 — D-EACH1=C / D-FANOUT3=C**: `<: a, b :>` expands one complete
binding or expression statement per name. Multiple fences on the same statement
advance in lock-step. An ascending numbered-name range such as
`<: task1..task8 :>` may bind names and reuse them in a later expression fence.
The fence is statement expansion, not a list,
destructure. `<:` and `:>` are longest-match digraphs; `:>`
suppresses line termination and neither digraph ends a statement. The formatter
retains one authored fence and wraps wide explicit fences one name per line.
The former `tasks.join_all([Task<T>]) => [T]` list operation was folded into
`task.all { … }`; task combinators now own the structured surface. Card #1239,
superseded by D-CONC-SPAWN1=D.

**2026-07-30 — D-VERDICT-1320-1 (amends D-EACH1)**: the fence respells as
symmetric `$[ a, b ]$`. `$[` and `]$` are longest-match digraphs (`$[` before
the D-CTMARKER1 `$name` splice, `]$` before plain `]`); `]$` suppresses line
termination exactly as `:>` did. An expression-position fence now accepts
arbitrary expression entries separated by top-level commas — each entry
substitutes token-for-token into its statement copy. A binding-target fence
still requires plain names or one ascending numbered-name range
(`$[ t1..t8 ]$`). E0369 duplicate checking applies to binding fences only;
repeated expression entries are legal and expand as written. Semantics are
unchanged from D-EACH1: one statement copy per entry, lock-step across fences,
formatter keeps one authored fence. Fences inside string interpolation remain
unsupported (owner-accepted tradeoff, card #1320). Card #1320.

**2026-07-30 — D-VERDICT-1321-1 (amends S9 print arity)**: `print`, `io.print`,
and `io.eprint` are variadic — they accept one or more arguments and write each
argument on its own line, in order, with a trailing newline after the last.
Zero arguments stays rejected (E0103, "needs at least one thing to print").
Every argument passes the same printable checks as before (E0112). Single-line
composition stays explicit via string interpolation or concatenation in one
argument. Lowering joins the arguments into one newline-separated value, so all
execution tiers (AOT, JIT/dev, interpreter, comptime sink) render identically.
No new syntax is minted. Card #1321.

**2026-07-31 — D-SPREAD1=A**: `prefix.[a, b, c]` is member spread —
`[prefix.a, prefix.b, prefix.c]`. Entries are bare identifiers only (E0961).
In list position the fields splice into the surrounding list; elsewhere the
result is an ordinary list. Sema desugars before inference; no TIR node.
Reuses the `.[` sigil freed by D-VERDICT-1324-1 (call fan-out stays removed).
Card #1341.

**2026-07-31 — D-WEB-CLICK-PORT1=D / D-UI-NODE-ID1=C / D-UI-EVT-DISP1=E /
D-UI-EVT-SET1=D**: portable `ui.button(label, on_click: handler)` carries a
click handler on the node. Identity is the render path by default; an optional
author `key` overrides (lists/reorders). Dispatch is O(1) slot lookup by that
identity (`jet_ui_dispatch` / DomRuntime `jetUiDispatch`) — never CSS-selector
identity. The portable event core is click/activate only; richer events need an
explicit capability module (for example `ui.web.hover`). Unsupported backends
may register the handler without firing it. Spelling uses the existing labeled
argument surface (S61); no new Syntax.rs keyword. Flagship:
`examples/features/web/ui_web_click.jet`. GTK folds into the same node-carried
handler (I8). Card #1311.

**2026-07-31 — D-SHARED-CYCLE1=C**: strong `Shared<T>` fields that can form a
reference cycle are rejected at sema (E0221). Expert intentional cycles use
`Shared.Weak<T>` — a weak handle with `.downgrade()` / `.upgrade()` /
`.strong_count()` — and free when strong roots drop. One Shared mechanism (I8);
full I9; no follow-on card. Flagship: `examples/features/memory/shared_weak_cycle.jet`.
Card #1372.

**2026-08-04 — D-ZIPPAD1:** `zip`, `zip_short`, and `zip_pad` share one
variadic lazy sequence family with strict, shortest, and longest-length
policies. Padding supports default `None`, one common `fill:`, or named
per-column `fills:`. Implemented end to end on card #1400.

**2026-07-26 — D-PIN1=A / D-PIN2=A / D-PIN3=A**: `mem.pin(&place) -> Pin<T>` is
the reusable address-stability contract. `Pin<T>` is a tracked write window on
one place; safe code may read and edit through it but must not move, replace, or
resize the pinned storage while any pin is live (E0218/E0219). Pin reuses the
owner/provenance graph — no second memory model. A field declared `Pin<U>` is
the structural projection mark (D-PIN3=A): reaching it through a pinned value
projects to `Pin<U>`; every other field projects as an ordinary view (D-PIN2=A).
Dropping the final pin ends the no-move loan. Unsafe construction stays inside
a library (`#Unsafe("…") { … }` in a safe function body); callers use the safe
`Pin<T>` API with no audited region. Spelling lives in Syntax.rs (`MEM_PIN`,
`TYPE_PIN`). Flagship: `examples/features/memory/pin.jet`. Card #1200.

**2026-08-05 — D-EXPOP1=A / D-EXPSEM1=A / D-XORSPELL1=A**: infix `^` raises a
number to a power, and `^=` raises in place — the spelling mathematics, MATLAB,
Julia, R, and Excel all use. Power groups to the right (`2 ^ 3 ^ 2` is 512) and
binds tighter than a leading minus (`-3 ^ 2` is -9), so it sits between the
unary and postfix levels. A whole base with a non-negative whole exponent stays
an exact whole number; a written negative exponent gives a Float; any Float
operand makes the result a Float. An exponent whose sign the checker cannot
read keeps the whole-number result and traps in the Prelude if it turns out
negative. Bitwise exclusive-or gives up the caret and becomes infix `~|` with
compound `~|=` — the exclusive variant of `|` — in the precedence slot the old
`^` held, under the same D-SG9 same-width rule. Prefix `^` (take, D-MEM1) and
prefix `~` (copy, D-SHAPE-COPY1) are untouched; parser position tells each from
its infix neighbour. Semantics live in one Prelude file
(`crates/jet-codegen/src/Prelude/Core/Power.rs`), shared verbatim by the native
and wasm Rust tiers; the Cranelift host and the interpreter run the same rule.
Spellings live in Syntax.rs (`OP_CARET`, `OP_CARET_EQ`, `OP_TILDE_PIPE`,
`OP_TILDE_PIPE_EQ`). Flagships: `examples/features/math/power_operator.jet`,
`examples/features/math/power_semantics.jet`,
`examples/features/math/xor_operator.jet`. Cards #1428, #1429, #1430.

**2026-08-05 — D-FLOORDIV1=A**: infix `/%` divides and rounds the answer down,
toward negative infinity, with compound `/%=`. It rounds down on whole numbers
and on floats alike, so `7 /% 2` is 3, `-7 /% 2` is -4, and `7.5 /% 2.0` is
3.0. Rounding down rather than toward zero means the answer does not jump up as
it crosses zero, which is what wrap-around and bucketing arithmetic wants. `/%`
sits with the other division-family operators, so a chain groups left to right
and `*` does not bind tighter. Dividing by zero traps, exactly as `/` does, and
reports "divided by zero". `/%` is the whole-number companion of `/`, which
gives a Float (D-INTDIV1). Semantics live in one Prelude file
(`crates/jet-codegen/src/Prelude/Core/Division.rs`), shared verbatim by the
native and wasm Rust tiers; the comptime interpreter and the Cranelift host call
one shared rounding rule in `MathLayout`, and the JS tier calls a preamble
helper carrying the same rule. Spellings live in Syntax.rs
(`OP_SLASH_PERCENT`, `OP_SLASH_PERCENT_EQ`). Flagship:
`examples/features/math/floor_div.jet`. Card #1431.

**2026-08-05 — D-MODSEM1=A**: `%` is the floored modulo and `%%` is the
truncated remainder, with compounds `%=` and `%%=`. The two differ only when
the operands straddle zero, and that difference is why both exist. `%` takes
the DIVISOR's sign, so `-7 % 2` is 1 — the answer stays in range for a positive
divisor, which is what clock and ring-buffer arithmetic wants, with no separate
"if it went negative" branch. `%%` takes the DIVIDEND's sign, so `-7 %% 2` is
-1, which is what "what is left over" wants when the original sign matters.
`%` pairs with `/%` so that `a == b * (a /% b) + a % b`. The two remainders
are always exactly one divisor apart when they disagree, and equal when they
agree.
Both are whole-number only under the D-SG9 same-width rule, and both trap on a
zero divisor rather than letting a Rust panic reach the user (I2) — so neither
uses the bare Rust operator, and their traps do not depend on whether the
checker could see through the operands. Semantics live in the same one Prelude
file as `/%` (`Prelude/Core/Division.rs`), shared verbatim by the native and
wasm Rust tiers; the comptime interpreter and the Cranelift host call one
shared rule in `MathLayout`, and the JS tier calls preamble helpers carrying it.
No in-repo use of `%` relied on truncation — every operand was already
non-negative, where the two agree. Spellings live in Syntax.rs (`OP_PERCENT`,
`OP_PERCENT_EQ`, `OP_PERCENT_PERCENT`, `OP_PERCENT_PERCENT_EQ`). Flagship:
`examples/features/math/modulo.jet`. Card #1432.

**2026-08-05 — D-BITNOT1=A**: prefix `!` turns over every bit it is given. On a
`Bool` there is one bit, so a yes becomes a no and that meaning is unchanged.
On a whole number it is the bitwise complement — the missing partner of `&`,
`|`, and `~|` — and the width comes back unchanged. On the default `Int`, which
carries no fixed width, turning over every bit is the same as `-x - 1`; on a
sized type the exact bits of that width flip, so `!U8.{5}` is 250. Clearing
bits is the everyday use: `flags & !mask` keeps everything except the bits the
mask names. No new sigil — `!` already existed, and this widens what it accepts,
so E0109 now says `!` needs a Bool or a whole number
(`tests/ui/bit_not_wrong_type.stderr`). Rust's `!` is already both operations,
so the native and wasm tiers need no helper; the Cranelift host uses `bnot` for
`Int` and reuses the fixed-width exclusive-or table for a sized type, and the
interpreter complements the value directly. The JS tier is the one that splits
them: JavaScript's `!` is logical only, so a whole number there lowers to `~`.
Spelling lives in Syntax.rs (`OP_BIT_NOT`, the same `!` as `OP_NOT`). Flagship:
`examples/features/math/bit_not.jet`. Card #1434.

**2026-08-05 — D-INTDIV1=A**: `/` answers the true quotient, so two whole
numbers give a Float: `7 / 2` is 3.5, the Python 3 model. This removes the
oldest arithmetic surprise in programming — an average that silently loses its
fraction. `/%` (D-FLOORDIV1) is the whole-number path, and it is the operator to
reach for when rounding down is what was meant. Because the result no longer
fits back into a whole-number binding, `n /= 2` on an `Int` is a type error;
compound assignment is desugared to `n = n / 2` before checking, so both
spellings report the same E0108 and its fix names `/%`
(`tests/ui/int_div_compound_on_int.stderr`). Sized widths are not touched: they
keep the D-SG9 same-width rule, so `/` on `U8` still answers a `U8`. Both
operands widen to Float in sema, so every tier runs one ordinary float
division and no engine needed a new rule. In-repo migration: the compound-assignment example and its two fuzz twins moved
to `/%=`; the comptime ratio in `comptime_block.jet` and its differential twin
moved to `/%`; the divide-by-zero probe in `user_defined.jet` moved to `/%`,
which still traps where `/` would now answer a Float infinity.
`all_failfast.jet` panics outright instead: `return 1 / zero` in a `=> Int`
function is a type error under this decision, and `/%` was not a drop-in there
because the comptime evaluator folds a divisor it can see and stops the build.
The example is about sibling cancellation, so it says so directly. Spelling is unchanged
(`OP_SLASH`). Flagship: `examples/features/math/int_div.jet`. Card #1433.

**2026-08-06 — D-TYPE2-FOUND1 = A — Adopt the carrier plus knowledge foundation**
*(card #1497, implementation card #1546)*. Every compile-time fact joins one
substrate with one registry. Ranges, units, lengths, states, and obligations
share registration, checking, and reflection. Type equality becomes transitive
again. Shipped programs keep their exact behavior; this changes how the
compiler holds what it already knows.

Type carries a knowledge vector. Identity is carrier plus identity-bearing facts,
declared per plane. Obligations compare by subsumption, which restores transitive
equality. All knowledge erases before the typed IR, so codegen and every
execution tier are untouched.

Amends: none.

**2026-08-06 — D-TYPE2-NUM1 = A — The number tower: one grid, and BigInt folds into Int**
*(card #1497, implementation card #1550)*. Every number type becomes one cell
of the grid. A sized width like U8 becomes Int plus a proven range and a one-byte
layout; its spelling and behavior do not change. BigInt retires, because the
arbitrary-precision Int already holds every value it held. The four errors that
police mixing Int with BigInt retire with it.

Axis one is the value world: whole, ratio, real, complex. Axis two is the
knowledge grade: exact unbounded, exact bounded, approximate, measured. The
ratified widening law, value-set containment, becomes the theorem that a subset
needs no conversion. One interval prover replaces the width table, the range-type
checks, and the fixed-list index proof.

Amends: D-INTBIG1. E0130–E0133 retire with BigInt.

The existing `BigInt` Syntax and editor-highlight rows remain as migration
anchors until #1550 removes the implementation; retirement snapshots are owed
to #1550.

**2026-08-06 — D-TYPE2-REFINE1 = A — One spelling for value-range rules**
*(card #1497, implementation card #1548)*. `distinct Int(1..6)` becomes the
one spelling. The `#Invariant` marker retires, and the index prover reads range
facts directly. The same range facts describe the sized widths, so U8 and Die
fail with the same error shape.

The range becomes an interval fact on the plane registry. The prover for
fixed-list indexing consumes interval facts instead of parsed strings. The marker
row retires under law zero with a replacement note.

The `#Invariant` row remains only as a migration anchor until implementation
card #1548 removes its parser and sema references. Its removal and any UI or
formatter snapshot updates are owed to #1548.

Amends: D-RANGETYPE1, D-REFINE1, and D-VERDICT-1455-1. Replacement: use
`distinct Int(lo..hi)` for a named range and `Int(lo..hi)` inline.

**2026-08-06 — D-TYPE2-TIME1 = A — Time joins the unit plane**
*(card #1497, implementation card #1552)*. Duration becomes the delta quantity
of a canonical Time unit family, stored as a 64-bit nanosecond count per the
ratified resolution decision. Instant becomes the matching point quantity.
Every 500ms resolves through the one unit plane, in user code, timeouts, and
schedules alike. Both hidden suffix tables are deleted.

The Time family is the one unit family with a fixed integer carrier, per the
ratified nanosecond decision. Point and delta reuse the affine machinery
temperatures already use. Timeout and schedule sites accept the Time delta type
instead of reading raw suffix tables.

The `Instant` Syntax row is registered now for I7 and editor-grammar coverage;
its parser/sema behavior and UI or formatter snapshots are owed to #1552.

Amends: D-TIMERES1 and D-QUANTITY-POINT1. Replacement: resolve Duration and
Instant through the canonical Time quantity family.

**2026-08-06 — D-TYPE2-MEASURE1 = A — One home for compile-time numbers in types**
*(card #1497, implementation card #1553)*. Lengths, shapes, lanes, and exponents
become measures: compile-time numbers attached to types through the plane
registry. Each use declares its combination rules. Joining two lists adds
lengths. Matrix multiply matches inner sides and composes outer sides.
Multiplying quantities adds exponents. Surfaces do not change; the encodings
underneath become one.

Measures are declared literals or module value parameters, never computed by user
code, so the ratified wall that compile-time evaluation cannot create types
stands. Symbolic lengths from generic modules resolve at specialization exactly
as today, without the placeholder-zero sentinel.

Amends: D-COMPUTE-TYPE1's stored encoding; its semantics stay unchanged.

**2026-08-06 — D-TYPE2-EXACT1 = A — The conservation law for precision**
*(card #1497, implementation card #1554)*. The law enters the spec: knowledge
grows silently, is lost only at a spelled step, and erases at runtime. The two
existing spellings stay, because each names its own operation well. Approx marks
accepting a representation's precision; rounded conversions name a mode because
rounding direction matters there.

The law binds every future surface: any operation that discards range, exactness,
unit, state, or classification must require a spelled step, checked in sema.
Existing spellings are grandfathered as the law's two instances.

Amends: none.

**2026-08-06 — D-TYPE2-UNCERT1 = A — Uncertainty as a knowledge grade**
*(card #1497, implementation card #1555)*. Measurement becomes the measured
grade of numeric knowledge. Arithmetic and the math functions propagate
uncertainty by the standard first-order rules. Exact values are
zero-uncertainty, so mixing works without ceremony. Nothing changes for any
program until a measurement enters it, and printing shows the value with its
uncertainty.

Propagation uses first-order linear approximation with uncorrelated inputs,
stated plainly in the docs. Correlated errors are out of scope and documented as
such. Measured unit scales produce measured conversion results, connecting the
two existing homes of uncertainty. A companion literal spelling, value ±
uncertainty, is proposed in the surface section of the proposal; the plain
measurement call is the canonical form either way. The spelling is not ratified.

Amends: D-UNCERTAIN1.

**2026-08-06 — D-TYPE2-PLANE1 = A — The plane law: registered, nameable, reflectable, readable**
*(card #1497, implementation card #1547)*. Every fact plane lives in one
registry. A plane exists exactly when registered, its facts are nameable,
reflection reports them as typed values, and the compiler's planes ship as
readable prelude source. Reflection gains dimensions, and marker arguments
become typed records on fields, methods, and variants, matching the type level.

This extends the marker registration law to dimensions, measures, exactness, and
classifications. Reflection's remaining string fallbacks are removed. Drift
guards walk every registered plane end to end, the same pattern the marker
rebuild uses.

Amends: D-VERDICT-1455-1.

**2026-08-06 — D-TYPE2-DEFAULT1 = A — Precise by default: the exact numeric world**
*(card #1497, implementation card #1551)*. Decimal literals are exact Decimal
values. Division of exact values lands in exact rational territory: a result with
a finite decimal prints as a decimal, otherwise it prints as a fraction and
stays exact. Functions that truly leave the rationals, like square root and sine,
answer approximate and say so. Experts opt into Float or F32 at the declaration,
and hot code gets today's machine arithmetic unchanged.

This amends three ratified rules by name. D-INTDIV1: division still maps whole to
rational, but the landing default is exact instead of Float. D-EXPSEM1 and
D-EXPNEG1: a written negative exponent gets the same exact landing. D-NUMTYPE1:
its Fraction-is-opt-in-by-name clause relaxes, because plain division can now
answer an exact ratio. Exact values use machine-word fast paths with spill, the
bigint Int playbook. Sized widths and Float keep every ratified behavior once
named, mixing with an approximate operand answers approximate under the one
widening law, and narrowing still requires approx.

Amends: D-INTDIV1, D-EXPSEM1, D-EXPNEG1, and D-NUMTYPE1.

**2026-08-06 — D-TYPE2-SPELL1 = A — Inline refinements in type position**
*(card #1497, implementation card #1549)*. `Int(0..100)` is legal in any type
position: parameters, returns, fields, bindings. Literals check at compile time;
unproven values convert fallibly, exactly as U8 works today. Naming stays
available through distinct when identity matters. Two inline types with the same
base and range are the same type.

The inline form is structural: identity is carrier plus interval fact. distinct
stays the nominal wrapper. The same checking already ratified for range types
applies unchanged, including the literal-only bounds rule from D-RANGE-VALUE1;
only the requirement to pre-declare a name is removed.

`Int(0..100)` is registered now, but is not parseable in every type position
until #1549. Its UI and formatter snapshots are owed to #1549.

Amends: D-RANGE-VALUE1.

**2026-08-06 — D-TYPE2-IMAG1 = A — Imaginary literals ride the unit-literal path**
*(card #1497, implementation card #1556)*. A numeric literal with the suffix i
is an imaginary number. Real plus imaginary reads exactly like the textbook:
0.5 + 0.8i. The lexer path is the shipped unit-literal path, so no new grammar
exists, and the identifier i alone remains an ordinary variable name.

The suffix resolves through the same table-free literal machinery as unit suffixes,
minting the imaginary component of the ratified Complex type. Only the suffix form
is special: bare i is untouched, and shadowing risks are the same as any unit
suffix in scope.

The `Complex` and `i` Syntax rows are registered now; imaginary semantics and
their UI or formatter snapshots are owed to #1556. No standalone `i` keyword row
is added because it remains an ordinary identifier.

Amends: none.

**2026-08-06 — D-AUTHORITY-MODEL1=A / D-AUTHORITY-ROOTS1=A /
D-AUTHORITY-MEM1=B / D-AUTHORITY-NAME1=A / D-AUTHORITY-SCOPE1=A /
D-AUTHORITY-MANIFEST1=A / D-AUTHORITY-GATE1=A / D-AUTHORITY-WORD1=A /
D-AUTHORITY-MEM2=A** *(card #1500, proposal
`docs/proposals/authority-one-model.md`)*. The authority slate records one
relation: a scope holds rights, nested scopes only shrink them, and every
widening is written and audited.

- **D-AUTHORITY-MODEL1=A**: effects, caps, policy, budgets, trust, sandbox,
  REPL, build and boundary checks read one rights tree, holds relation,
  tighten rule and gate record. No user-facing spelling changes in the
  substrate slice. Implementation: #1566.
- **D-AUTHORITY-ROOTS1=A**: the closed table has thirteen roots — the ratified
  ten plus `FFI`, `Browser` and `Secret`. FFI languages are leaves such as
  `FFI.Go`; flat language roots are deleted. This amends D-EFF4/D-EFF5 and the
  FFI effect clauses. Implementation: #1567.
- **D-AUTHORITY-MEM1=B**: memory floors leave `#Policy` and become effect
  denials such as `=[!Mem.Alloc]=>` and manifest `deny: [Mem.Alloc]`.
  `#Policy` keeps non-memory arguments. This amends D-MEM-FACTS1 and
  D-POLICY-WORD1. Implementation: #1568.
- **D-AUTHORITY-MEM2=A**: denial rows accept an optional `above: Bytes`
  argument, for example `=[!Mem.Alloc(above: 65536)]=>` and manifest
  `deny: [Mem.Alloc(above: 65536)]`. Record-only outcome; it closes the
  bounded-arena gate in #1568 criterion 4.
- **D-AUTHORITY-NAME1=A**: `Authority` is the one nameable rights value at
  process, plugin and session boundaries; `ProcessAuthority` becomes
  `Authority`, while `ProcessPlan` and `ProcessReceipt` stay. Implementation:
  #1569.
- **D-AUTHORITY-SCOPE1=A**: `#Caps` is the one block marker. A bare list
  narrows (`#Caps(FS, Net)`); a name-before-list head binds the handle
  (`#Caps(g: FS, Net)`). `#Grant` is deleted, and its error points to
  `#Caps`. This amends D-EFF1, D-SCAP1 and D-ARROW-CONTROL1. Implementation:
  #1573.
- **D-AUTHORITY-MANIFEST1=A**: one `authority:` block holds package bounds,
  dependency grants, trust defaults and provider bounds; replaced keys are
  migrated and deleted. This amends D-EFFBUDGET1,
  D-JPK-POLICYSURFACE1 and D-JPK-GRANTSCHEMA1. Implementation: #1570.
- **D-AUTHORITY-GATE1=A**: every authority gate records in one provenance-
  carrying ledger; `jet inspect authority` reads the rights view and existing
  views remain filters. Implementation: #1571.
- **D-AUTHORITY-WORD1=A**: `capability` leaves user-facing surfaces; borrow
  diagnostics say `write access`, the rights value is `Authority`, and the
  product claim surface is `feature claims`. Implementation: #1572.

Outcome-to-card map:

| Decision | Outcome | Delivery |
| --- | --- | --- |
| D-AUTHORITY-MODEL1 | A | #1566 |
| D-AUTHORITY-ROOTS1 | A | #1567 |
| D-AUTHORITY-MEM1 | B | #1568 |
| D-AUTHORITY-MEM2 | A | record-only; #1568 criterion 4 |
| D-AUTHORITY-NAME1 | A | #1569 |
| D-AUTHORITY-MANIFEST1 | A | #1570 |
| D-AUTHORITY-GATE1 | A | #1571 |
| D-AUTHORITY-WORD1 | A | #1572 |
| D-AUTHORITY-SCOPE1 | A | #1573 |

**D-VERDICT-1455-1 — Mandatory registration (law zero)** *(ratified
2026-08-05, card #1455)*: a marker exists if and only if it is a row in
`Policy::APPLIED_RULES`. No marker may be parsed, checked, formatted,
highlighted, or reflected outside that registry, and every active row must be
implemented end to end or retired. Drift in either direction is a defect.

Two rows were resolved under this law. `#Authority` was registered but no
parser ever read it, so it is removed; the name now reports the ordinary
E0927 unknown-marker family. `#Summarize` was registered as a compiler derive,
but no `Summarize` trait exists anywhere in the compiler or Core library —
it only ever worked through an ordinary `derive T.Summarize { … }` provider
(D-METADERIVE1=A). The compiler row is removed and the user-derive spelling is
unchanged, so `#Summarize` on a type with no visible provider now reports
E0927 instead of silently doing nothing.

Retired rows also stop applying effects. `#Pure` set `is_pure` and
`#InlineAlways` set `is_inline_always` *after* diagnosing, so both retired
spellings kept working and the registry's status column lied about them. They
now teach their replacement and apply nothing.

**D-CONF-PLANE1=A — one fact plane, one parser** *(ratified 2026-08-06,
card #1517)*: configuration is the program's knowledge about itself. Every
build fact is a typed value on one registered plane, with one contribution law
and one audit command. The role-typed `Package` shape is the only manifest
vocabulary. The legacy flat parser is deleted. `@build.os` is the plane's first
row. The fact registry follows D-VERDICT-1455-1 and D-TYPE2-PLANE1: every fact
is nameable, reflectable, and registered. Facts are comptime values that erase
at codegen; S26 stands. This implements D-SHAPE5a/b and D-ECO-DECL1 on the
compile path.

**D-CONF-NAME1=A — bare identity, ratified nouns** *(ratified 2026-08-06,
card #1517)*: `name:` and `version:` sit at the top level with no wrapper.
The block nouns are `deps:`, `outputs:`, `settings:`, `build:`, `policy:`,
and `members:`. The `identity:` wrapper is deleted. This amends D-SHAPE5a/b;
all other ratified block nouns keep their decision IDs.

Every block noun's grammar accepts nested records with or without the
dot-construction prefix (`deps: .{ … }` and `deps: { … }` both parse; new
manifests are written with the dot, matching every other typed literal in
Jet). Dependency values accept both the legacy bare token (`helpers:
../helpers`, `textkit: textkit#1.2.0`) and the ratified quoted spelling
(`httpkit: "^2"`) — the same `DepSource` classification either way.
D-BUILDSCOPE1's package-scoped `fn build` may still live beside the manifest
fields in the same file; the one parser steps over that one declaration
rather than reporting it as a malformed field.

Every in-repo `package.jet`/`pkg.jet` (examples, tests, fixtures) uses the
bare-identity spelling; no fallback parse path remains. Flagship:
`examples/features/packages/outputs_build/`. Card #1517.

**D-CONF-READ1=A — `@build.*` fact reads** *(ratified 2026-08-06, card
#1518; spelling amended by D-ONCE-AT1=D)*: facts are read with the
metaprogramming mark: `@build.package.version`, `@build.os`, and
`@build.settings.tls`. `@` carries
the compile-time meaning, so `build` remains an ordinary identifier. One
spelling covers compiler facts, identity, settings, and stamps. Reads fold to
constants before codegen; bare scripts use fallback identity from the file
name and `0.0.0`. The `@build.os` dispatch subject becomes
`@if @build.os == { ... }`, and value-position fact reads are legal.
This amends D-OSTARGET2 and D-CANVASSTATE1 and rides D-META-ONE1/S4. All I9
tiers see one folded program.

**D-CONF-KEY1=A — declared typed settings** *(ratified 2026-08-06, card
#1519)*: a package declares each setting with a Tier-0 type and a default in a
`settings:` block. Optimization bundles and the command line contribute
values by the contribution law. `@if` can fold on a setting, and a fact read is a
constant. Undeclared settings are compile errors. `--set key=value` is the CLI
spelling. `features:` and `env:` are deleted from `Build.{}`. This amends
D-BUILDPROFILE1. The setting value set is Bool, Int, Char, String, and
fieldless enum, as ratified by D-GENMOD-VALUE1.

**2026-08-13 — card #1519 implementation log.** The package parser, build-fact
snapshot, CLI/profile contribution path, `@build.settings.*` folding, and
execution-tier callers now use the typed settings plane. Retired `features:`
and `env:` profile fields teach the declaration-plus-`--set` form and do not
reach rustc configuration or process environment.

**D-CONF-MERGE1=A — nearest source, most explicit layer, `.Force` pins**
*(ratified 2026-08-06, card #1520)*: one contribution law has two halves. In
source scopes, the nearest scope wins across item, block, function, module,
file, and package. Across layers, the most explicit writer wins:
declaration, optimization bundle, workspace, environment, system, fleet, and
CLI. Two writers at one layer with different values remain a hard error that names both
sources. At system and fleet layers, `.Force` pins a value against later
layers, including the CLI, and `jet explain` names the pin. This extends
D-MARK-SCOPE1 and amends the Config composition conflict rule so cross-layer
overrides are legal while same-layer conflicts still error. Safety facts
tighten only. Ambient contributions remain forbidden. The
D-JOS-PRIORITY-SURFACE2 surface is unchanged.

**D-CONF-SPLIT1=A — facts in text, actions in `fn build`** *(ratified
2026-08-06, card #1522)*: manifest text is the complete index of configurable
facts. Each fact has a type and default. `fn build` performs probes, steps, and
code generation. It may contribute a value to a declared fact. Every computed
contribution is recorded in `.jet/lock` and named by `jet explain`. `fn build`
cannot mint an undeclared fact. D-BUILDCTX-FLAGS1 survives only as
`b.contribute(fact, value)` against a declared fact. Computed contributions
follow D-MODCOMPUTE1. Build actions keep the D-BUILDTARGET1/D-BUILDACTION1
family.

**D-CONF-ENTRY1=A — one build entry per package** *(ratified 2026-08-06,
card #1521)*: `fn build` follows `fn run` discovery. A package has at most one
`fn build`, in any package file; the compiler discovers it. Duplicate
candidates are a compile error that names both sites. A workspace runs member
builds in dependency order, root last. `jet build <name>` resolves a package
through the members graph and runs its build. No pointer field is required;
an Output-level `entry:` override remains available. No `fn build` means the
batteries pipeline. This extends D-BUILDENTRY1, uses D-ECO-MEMBERS1, and keeps
the D-SHAPE-OUTPUT-CALLABLE1 override.

**D-CONF-MODULE1=A — one value substrate** *(ratified 2026-08-06, card
#1524)*: settings and module value parameters share one evaluator, one fuel
budget, and one diagnostic family. A build fact is a legal module value
argument, so the final module spelling can read
`module tuned :: cache<Int>(@build.settings.cache_slots)`. The closed-
expression rule in D-GENMOD-VALUE1 admits facts as value arguments. The
ratified `[T#capacity]` layout carve-out admits fact-fed `Int` parameters. A
change to the optimization bundle produces a different cached specialization.
Defaults and named arguments remain forbidden in the parameter list. S26 still says values
specialize bodies and the one layout slot, never type identity or dispatch.
D-CONF-GENSPELL1 owns the punctuation; this decision owns the shared
substrate.

**D-CONF-GENSPELL1=A — generic modules mirror functions** *(ratified
2026-08-06, card #1523)*: generic modules use angle brackets for types and
bounds, parentheses for typed value parameters, and `::` for the module alias.
The canonical form is:

```jet
module cache<K>(capacity: Int) { ... }
module int_cache :: cache<Int>(64)
module tuned :: cache<Int>(@build.settings.cache_slots)
```

A module with no value parameters has no parentheses. The value-parameter
rules remain Tier-0, with no defaults or named arguments and the
`[T#capacity]` slot. This amends D-GENMOD2. Migration is mechanical:
`<K, capacity: Int>` becomes `<K>(capacity: Int)`, and `=` becomes `::`.
E0851/E0853 keep their arity and type meanings. D-CONF-MODULE1 facts use the
value position.

**D-CONF-STAMP1=B — stamps plus a lock-pinned timestamp** *(ratified
2026-08-06, card #1525; spelling amended by D-ONCE-AT1=D)*: `@build.stamp.git`
is the commit hash, with a
`-dirty` suffix for an unclean tree and no value outside a repository.
`@build.stamp.dirty` is the explicit working-tree state.
`@build.stamp.toolchain` is the Jet version. `@build.stamp.at` is captured once
when the lock is written and replayed from the lock. All stamps are recorded
in `.jet/lock`; a locked rebuild does not read the wall clock. Repository state
is a Tier-1 locked input under D-CTEFFECT1. The timestamp describes lock
history, not source.

**D-CONF-WORD1=A — one meaning for the word `profile`** *(ratified 2026-08-06,
card #1526)*:

`profile` names exactly one thing: the optimize bundle selected by
`--profile` and its ratified `--release` sugar. `release`, `debug`, and `ci`
keep the word, and D-BUILDPROFILE1 is unchanged.

The board identity is a machine description and uses the machine vocabulary.
`--target=<triple|board>` selects it. The `targets:` manifest field
(D-TGT1-4) and build-graph targets (D-BUILDTARGET1) keep their own words, and
E1216 is unchanged.

Under D-ENV-PROFILE1, the retired environment `profiles:` spelling becomes
`presets:` and `--preset` selects that composition. Under D-ENV-FACET1,
`--env <name>` selects the declared `env.<name>` module. Under
D-JPK-PROFILE1, package and user prose uses `generation`. These renames do
not change behavior.

**D-META-ONE1=A — compile time is one Jet program** *(ratified 2026-08-06,
card #1539)*: Compiler rules are Jet declarations in the Prelude program. Marker
rows, effect roots, and the compile-time dimension vocabulary are not separate
Rust-only language mechanisms. User rules use the same declaration and checking
path. This fulfills D-VERDICT-1455-1: one law-zero registry and one declaration
model govern built-in and user-defined compile-time behavior.

**D-META-REG1=A — one registration table** *(ratified 2026-08-06, card
#1538)*: A marker, knowledge-plane entry, right, or build fact is one row in the
shared registration table. The row records its name, target, law columns,
publisher where needed, and marker signature where code is attached. Reflection,
`jet explain`, and drift guards read that table. No category gets a second table,
lookup, or guard.

**D-META-NAME1=A — the declaring word is `marker`** *(ratified 2026-08-06, card
#1538)*: `marker` is the contextual keyword for a compile-time rule declaration.
`rule`, `derive`, and a clause-only declaration are not language spellings.

**2026-08-06 — D-META-EFFECT1=A** *(card #1543, same proposal)*. One effect
model applies at both stages. Tier 0 is an empty effect set; Tier 1 is recorded
and hashed in `.jet/lock`; Tier 2 requires `#Impure("reason")` and
`--gate impure=allow`. The curated pure-Core eligibility list retires:
the shared foundation effect fact decides eligibility, and the shared
`PurityStage` walk gives both stages one E3401 family (`E0951` redirects to
E3401). `D-ONCE-GATE1` retires the old `--allow-impure` spelling.

**D-META-REG1 = A — one table, four uses.** A marker rule, a knowledge plane, a
right, and a build fact are rows of the same table, separated only by what they
attach to. The table is `crates/jet-foundation/src/Registry.rs`. A row states its
name, its target, the two law columns below, the prover that publishes it where
one does, and — for a row whose target is written code — its marker signature.
Reflection, `jet explain`, and the drift guards read that one table and are
written once: no kind gets a second table, a second lookup, or a guard of its
own. Marker rows come from the marker registry, so law zero (`D-VERDICT-1455-1`)
is unchanged and every marker is still a row.
**D-META-FORM1=A — one named-parameter list** *(ratified 2026-08-06, card
#1538)*: A marker declaration uses one ordinary named parameter list. Plain
parameters are use-site arguments; `@`-marked parameters are facts about the
rule, such as `@sites` and `@repeatable`. There is no second parameter list,
trailing clause, or scope block. A new rule fact is a new named parameter. Typed
rule arguments use D-RULEARG-TYPES1.

**D-META-USER1=A — user rules record facts or add code** *(ratified 2026-08-06,
card #1540)*: A bodyless rule records its facts. A rule body adds code through the
same checked path. The rule law is additive: it does not mutate or shadow an
existing declaration. This amends D-METADEPTH1: additive user rules are inside
the compile-time ceiling, while mutation and grammar changes remain outside it.

**D-META-CODE1=A — generated code is real Jet code** *(ratified 2026-08-06, card
#1541)*: `derive` and `b.generate` bodies are Jet code with typed holes. A filled
template enters ordinary semantic checking; D-META-CODE1 is the typed
code-generation boundary.

**D-META-BODY1=A — derive bodies are implementations** *(ratified 2026-08-06,
card #1541)*: A derive body is a real implementation body. Compile-time flow may
add members through that body, and member collisions are errors under the normal
declaration rules.

**D-META-DSL1=A — libraries may declare checked text blocks** *(ratified
2026-08-06, card #1542)*: A library can declare a checked text block. Jet owns the
boundary and grammar contract; the library supplies the domain content. This
amends D-DSLBLOCK1 and retires the marker-only `STDLIB_DSL_BLOCK_MARKERS`
mechanism.

**D-META-EFFECT1=A — one effect model at both stages** *(ratified 2026-08-06,
card #1543)*: Compile-time and runtime use the same effect syntax and checker.
D-CTCORE1's special allow-list is retired. The effect tiers remain: Tier 0 is
empty, Tier 1 records an effect, and Tier 2 requires `#Impure` plus permission.

**D-META-CONST1=A — comptime values are constants** *(ratified 2026-08-06, card
#1544)*: Any computable compile-time value is legal wherever a constant is legal.
The literal-only E0963 and E0035 restrictions are retired; S26 remains the
constant-evaluation law.

**2026-08-13 — cards #1543 and #1544 implementation log.** Compile-time build
entries accept the runtime effect row syntax. The foundation `Effects` table
and shared `PurityStage` walker decide both stages; Tier 2 uses
`--gate impure=allow`. `#Impure` reasons remain the recording point in the AST
and sema gate handler until the shared gate ledger from #1571 lands. Fixed-list
lengths and enum discriminants retain their source expressions and use the
ordinary comptime evaluator before type registration; E0963 and E0035 report
unknown, noninteger, and range failures.

**D-META-MODNAME1=A — instance members use the instance path** *(ratified
2026-08-06, card #1541)*: Generated members are reached through the module
instance path, for example `three_ints.Buffer`. Any mangled implementation name
is internal and is not a second user-facing naming form.

**D-META-STAGE1=B — `@` marks every compile-time mention** *(ratified
2026-08-06, card #1537; spelling amended by D-ONCE-AT1=D)*: `@` belongs to the
name at every compile-time mention.
It marks a declaration, use, and fact read; `#Known`, `#Known if`, and the
`#Known { ... }` form retire from the marker plane. This amends
D-VERDICT-1308-1 and D-VERDICT-1308-2 and retires D-CTMARKER1.

**D-META-AUTO1=A — derive all eligible structure unless refused** *(ratified
2026-08-06, card #1545)*: Every structurally derivable capability is derived by
default unless the user refuses it. The existing refusal spelling and eligibility
gates remain. This amends D-AUTODERIVE1=E and D-AUTODERIVE-SYNTAX1.

**2026-08-07 — the compiler-facts slate: D-FACT-LAW1=B, D-FACT-WORD1=A,
D-FACT-GATE1=A, D-FACT-READ1=A, D-FACT-HOME1=A, D-FACT-OWN1=A,
D-FACT-FLOW1=A** *(card #1620, proposal `docs/proposals/compiler-facts-one-law.md`)*.
The capstone over four ratified rethinks — type v2 knowledge, authority rights,
concurrency state/duty/reach, and memory v5 ownership. It amends none of their
decisions; it names the one law they each drew a corner of. The law itself is
stated in `docs/spec/philosophy.md`.

**D-FACT-LAW1 = B — one law, plus a guarded registry.** A fact moves toward
safety silently; every move away is one written word, at the site, on the
record; at runtime no fact remains. `D-TYPE2-EXACT1`, `D-AUTHORITY-MODEL1`,
`D-PACKAGE-POLICY-SCOPE1`, `D-CONF-MERGE1`, `D-FLOWTYPE1`, and `D-CONC-JOIN1`
are recorded as named instances, not as separate rules. The law is enforced by
the registry: every row on the `D-META-REG1` table states its safe direction and
its gate words, a row with no meaningful direction states `none`, and a row that
states neither fails the build. The guard is the marker drift-guard, extended —
one implementation, not a second. Existing rows are backfilled in the same
change. Cards #1538 and #1460.

The named instances, as rows of the one table
(`crates/jet-foundation/src/Registry.rs`):

| Instance | Row | Target | Safe direction | Gate words |
| --- | --- | --- | --- | --- |
| Marker law zero (`D-VERDICT-1455-1`) | every marker row | written code | `none` | none |
| The plane law (`D-TYPE2-EXACT1`) | `Exactness` | a value | `gain` | `approx`, `raw`, `wrapping` |
| Flow narrowing (`D-FLOWTYPE1`) | `Flow` | a value | `gain` | none |
| Taint (`D-TAG-SURFACE1`) | `Taint` | a value | `gain` | `Scrub` |
| Duty (`D-CONC-JOIN1`) | `Duty` | a value | `discharge` | `detach`, `drop` |
| The rights law (`D-AUTHORITY-MODEL1`) | `Rights` | a scope | `shrink` | `Unsafe`, `Impure`, `Grant` |
| Package policy (`D-PACKAGE-POLICY-SCOPE1`) | `PackagePolicy` | a scope | `shrink` | `Unsafe`, `Grant` |
| The fact plane (`D-CONF-KEY1`) | `Build.Settings` | the build | `shrink` | `Force` |

Marker law zero is the instance with no meaningful direction. A rule on written
code says what a writer may attach and where; it holds no fact that moves, so
every marker row states `none` and names no gate. The moving facts a marker
*writes* belong to the row that holds them — `#Grant` is a gate word on the
`Rights` row, not a direction of its own. That is stated once, for every marker
row at once, so no row can drift from it.

A row states both columns because both are fields of the row, so a row that
states neither cannot be written. The guard reads what the field types cannot:
a gate word named with no direction to loosen, a gate word nothing spells
(a gate is a registered marker or one of the Prelude gates `approx`, `raw`,
`wrapping`, `detach`, `drop`, `Force`), a prover row that claims plane algebra,
and a name registered twice. `Flow` shows the honest empty gate list: a narrowed
optional gains certainty for free and the fact ends at the branch boundary, so
nothing loosens it.

**D-FACT-WORD1 = A — tighten and loosen.** One word pair names the law in
diagnostics and docs. `attenuate`, `conserve`, and law-level `narrow` retire as
law words. Operation names are untouched: flow narrowing keeps its name,
widening conversions keep theirs, and no user-typed spelling changes. Duty
diagnostics use the *owes* phrasing, which the ballot records as the one plane
where `tighten` reads poorly: "this value still owes `join` — join it, or write
`detach`". The E0140 family and the `#SingleUse` message speak with that one
voice. Card #1624.

**D-FACT-GATE1 = A — one full ledger, grouped by kind.** Every gate lands in one
ledger: unsafe regions with obligation status, `#Impure` marks, dependency
grants, trust grants, build and session flags, `.Force` pins, taint scrubs, duty
drops, and precision demotions. `jet inspect gates` reads it, groups by kind with
the security kinds first, and summarizes a large kind as a count so numeric code
cannot bury the security rows. This generalizes `D-AUTHORITY-GATE1`:
`jet inspect authority` stays as the rights-kind filter of the same ledger, and
`jet unsafe` as another filter. Compile-time only, no runtime cost, no new write
surface, and no gate gains new ceremony — reasons stay exactly where ratified law
already puts them. Card #1571.

**D-FACT-READ1 = A — the `@` mark reads every plane** *(spelling amended by
D-ONCE-AT1=D)*. Reading a compiler-held fact is one act with one mark:
`Severity.@range`, `send_report.@effects`, `Order.@states`, `Meters.@dimension`,
and `@build.profile` as the same act with
the build as its subject. This extends `D-META-STAGE1` from build facts and
splices to member position on types and functions. Facts answer typed values
from the registry. `reflect()` remains the aggregate view and loses its string
fallback; the `"\0state:"` encoding is deleted. Comptime only: S26 holds, a fact
is a value and never mints a type, and `D-FACTMODEL1` holds, facts classify and
never dispatch. Card #1623.

**D-FACT-HOME1 = A — home the user-facing orphans.** Send-safety, failure
attribution, `#Track` origin (`D-PROVENANCE1`), view provenance
(`D-MEMPROVENANCE3`), unit-scale provenance, and maturity become typed registry
rows, readable like every other fact. Prover internals — uninit tracking and
exhaustiveness — stay engine-side and gain no row, matching the wall in
`D-FACT-OWN1`; the line is written down per fact rather than left to folklore.
Phantom fact-menu names such as `Capability` are refused at the signature
instead of compiling to a dead end at every call, and each refusal names its
living counterpart (`Authority`, or a rights list) or says plainly that none
exists. The census is about eighteen names, not the rule-arg seven. Cards #1622
and #1501.

**D-FACT-OWN1 = A — the ownership wall: prover, not plane.** The borrow checker
is a prover and never a registered plane. Alias and flow analysis over places
does not decompose into per-operation rules, so no card may re-express `&`, `^`,
or `~` windows as plane algebra. Ownership checking continues to follow `D-MEM1`,
`D-SHAPE-PLACE1`, and `D-MEM-VIEWRET1` as its own analysis. The facts it
publishes — sendability, view provenance, moved-ness — enter the registry as
read-only prover-supplied rows (`Sendability`, `ViewProvenance`, `Movedness`),
so tools and other planes consume them like any other fact. Each names the prover
that publishes it and states safe direction `none` with no gate: a window is
closed by the prover, never loosened by a written word. The memory sigils obey the law's spirit but are the prover's surface,
not gates, and they never enter the gate ledger. Cards #1538 and #1622.

**D-FACT-FLOW1 = A — one flow-fact store, planes supply joins.** One store on the
checker, keyed by binding and plane, replaces the six hand-written stores (moved,
uninit, typestate, taint, narrowing, view facts) and the twelve one-off fact
columns on `LocalInfo`. Each plane supplies only its join rule, and the shared
walker applies joins at every `if`, loop, and arm-table merge point, so a
forgotten join becomes impossible by construction. This closes the two dead joins
— `State.rs` `join_after` and `UninitState::merge_paths`, both
`#[allow(dead_code)]` — which today let typestate and partial-init keep whatever
branch the walker saw last. The walker states its loop fixpoint rule once. Sema
only: no TIR or codegen change, and generated Rust stays byte-identical. View
facts are stored here while their invalidation logic stays with the ownership
prover. Card #1621.

**Sequencing (owner directive, 2026-08-07).** The substrate cards outrank every
per-plane implementation card. Ranked order: #1538 (the guarded registry), #1621
(the flow-fact store), #1539 (rows in readable Prelude source), #1547 (fact
planes on the one registry), #1622 (home the orphans), #1501 (phantom-name
refusal), #1623 (the `$` read surface), #1571 (the gate ledger), #1624 (the one
law voice). No per-plane card in #1517–#1579 closes before its substrate
prerequisite closes.

**2026-08-06 — D-CONC-SPAWN1 = D / D-CONC-FAIL1 = A / D-CONC-JOIN1** *(card #1685)*. The spawn surface has one reserved word: `task`. A plain `task f()` or `task { … }` starts one child. Nested selectors stay free identifiers: `task.all { … }` waits for every branch and fail-fast cancels siblings, `task.race { … }` returns the first successful branch and cancels losers, and `task.any { … }` returns the first completed branch and cancels the rest. `task.group g { … }` owns a lexical scope; `task.group g(limit: n) { … }` bounds active children and the group joins at close. `TaskGroup` parameters reuse the caller's lexical group. `join()` is the fallible `T ? TaskFailure` rail with `.Cancelled`, `.DeadlineBlown`, and `.Panicked(reason)`. `TaskOutcome`, `TaskStatus`, `trace()`, and `exception()` are retired. The old `taskgroup`, `g.task =>`, `g.all([…])`, `g.race([…])`, `g.any([…])`, `tasks.spawn`, `tasks.spawn_group`, `tasks.join_all`, and `tasks.wait_any` spellings have no compatibility path.
**2026-08-08 — D-LIB-EXPORT1=C / D-LIB-DYNTRUST1=A / D-LIB-NAME1=A /
D-LIB-CALLGRANT1=A** *(card #1421)*: `Library` keeps one output kind. Its
`loadable: true` field requests a `.jetlib` artifact. `Mod.load(path,
grant: .{ ... })?` is the load form, and `mod.on_tick(dt)` is the typed member
call form. The grant maps declared effects to normalized path sets and is
checked before mapping. Native export produces static and shared libraries,
the C header, and generated bindings for named languages. No new lexer token,
keyword, or Jet-to-Jet ABI promise enters the surface.

**2026-08-13 — D-MATRIX-MUL1=F / D-MATRIX-LIT1=E /
D-MATRIX-BCAST1=E / D-MATRIX-INDEX1=E / D-MATRIX-SOLVE1=B /
D-MATRIX-HOME1=D** *(card #1437)*: `Matrix<M,N>` and `Vec<N>` use the
ratified D-TYPE2-MEASURE1 substrate. `Matrix * Matrix` composes maps, and a
leading dot selects cell arithmetic only on `Matrix`; `Grid` and lane vectors
already use `*` cell by cell. A matrix row ends at a source line break or `;`.
Extent one broadcasts along its stated axis, and vectors use `.as_row()` or
`.as_column()` before a matrix operation. Brackets read cells, bands, and
windows; `.row(i)` and `.col(j)` name whole axes, a scalar index drops its axis,
and a range keeps it. `.solve(y)` answers square systems and
`.least_squares(y)` answers tall systems. Types, literals, indexing, transpose,
`+ - *`, and reductions need no import; solving, determinants, decompositions,
and transforms use `core.linalg`. The proposal and all illustrative examples
live in `docs/proposals/matrix-surface.md`; implementation is a later card.

## The say-it-once slate (D-ONCE-*, ratified 2026-08-07, card #1656)

The corpus-wide first-principles audit's rulings. Tower is the decision home (D-ONCE-LEDGER1=A); this block is the spec render.

- **D-ONCE-LAW1=A** — the corpus law: every truth has one home with named renderers and a guard; a guardless law row fails lint; a hand-written second copy fails the build.
- **D-ONCE-TIER1=A** — full tier parity; the interpreter is a first-class tier; supersedes D-VERDICT-1254-1's interpreter carve-out.
- **D-ONCE-RETIRE1=C** — retirements ship as code, split by category: pure renames auto-rewrite with a notice; semantic changes hard-error with a fix line; every retirement carries an adoption ratchet that ends at zero.
- **D-ONCE-WORD1=A** — `Stream<T>` owns the word stream; codec modes are readers and writers; events are events; eager loop comprehensions are collecting loops. One vocabulary page; the retired senses are doc-lint errors.
- **D-ONCE-DERIVE1=A** — the marker (`#Comparable`) is the one spelling to request a derived capability; the body-line `derive X` retires; the `derive` keyword keeps only its provider-definition meaning (amends D-USERDERIVE1; keeps D-METADERIVE1).
- **D-ONCE-SANDBOX1=A** — the isolated package kind is `target: sandbox`; `target: plugin` retires as a rename (amends D-PLUGIN1 naming).
- **D-ONCE-GATE1=A** — one org-policy ladder governs every audited escape (`#Unsafe`, `#Impure`, `#Nondeterministic`); `--gate <name>=allow` is the per-invocation escape the org file can refuse; `--allow-impure` retires. The shared ledger is D-FACT-GATE1's.
- **D-ONCE-LEDGER1=A** — Tower is the one decision home; spec text renders it; `tower lint --docs` walks `docs/spec/**`; supersession links are mandatory on verdicts.
- **D-ONCE-VERB1=A** — one verb per job across collections: `pop` is remove-and-return everywhere; `replace` keeps only the List swap meaning; bare `.from()` is the blessed conversion constructor, `from_x()` for source-qualified variants (amends the D-API-STORE1 table and D-STDRUBRIC1 idioms).
- **D-ONCE-AT1=D** — `@` is the word *at*, in space and time: infix `@` stays the package-source separator (D-JPK-REF1, unchanged); prefix `@` becomes the compile-time mark and the fact-read glyph (`@config`, `T.@range`, `@build.profile`), amending D-FACT-READ1's spelling half, superseding D-VERDICT-732-1's former prefix reservation, and replacing the `$` comptime-mention mark. The location reservation rides the same word, minted by a future ballot.
- **D-FENCE-GLYPH1=A** *(card #1516)* — expression fences are spelled `@[ … ]@`, superseding `$[ … ]$` under the D-ONCE-AT1 prefix migration. The open and close digraphs are longest-match lexer forms. The retired `$` spelling has no compatibility path; old prefix `$` uses teach the `@` form through E0003.
- **D-FENCE-RANGE1=A** *(card #1516)* — inside an expression fence, an ascending integer-literal range `0..3` expands to the four entries `0`, `1`, `2`, and `3`. Descending ranges and ranges with non-integer-literal endpoints remain one ordinary Range value and add no diagnostic. Binding-name ranges keep their existing numbered-name rule.
- **D-ONCE-DOLLAR1=B** — the freed `$` becomes typed environment access in config surfaces (`$HOME` in `env.jet` and deploy files, listed by `jet inspect env`); outside config surfaces it is a teaching error.
- **D-ONCE-UITREE1=C** — the ratified-but-unbuilt `.Button.{ }` UI-tree spelling is marked unbuilt in the spec; the spelling decision reopens with card #1588's architecture result.
- **D-ONCE-CASE1=A** — one naming lexicon (plain words, the blessed-abbreviation list, fixed acronym casing) governs every surface: source, CLI verbs and flags, manifest keys, and file names. New abbreviations earn a row by ballot.
- **D-ONCE-HASH1=B** — interpolation format selectors respell to colon (`{x:Fixed(2)}`, `{user:Debug}`); `#` keeps its three non-colliding jobs: applied rules, fixed-size lists (`[T#4]`), and package version pins.

## Implementation note — interpolation selector registry

`crates/jet-foundation/src/Syntax/effects_surface.rs` owns one
`INTERPOLATION_SELECTORS` table for the selector names and their argument
shapes. Law: each selector vocabulary name appears in that table exactly once,
and parser/formatter code reads the row rather than copying the name. Adding a
selector of an existing shape adds one row there. This surface is separate from
the applied-rule registry because a selector chooses how a value is rendered
inside one string hole; it does not attach a rule, inherit policy, or select a
target.

## Implementation note — marker rows are Prelude declarations

The marker vocabulary is written as ordinary Jet source, not as a Rust table.

- `crates/jet-codegen/src/Prelude/Markers.jet` holds one `marker Name(params…)`
  declaration per rule, in the ratified D-META-FORM1=A form. A plain parameter
  is an argument the use site writes; an `@`-marked parameter is a fact about
  the rule: `@sites`, `@repeatable`, `@owns_menu`, `@inherits`, `@resolution`,
  `@scopes`, `@companion`, `@retired`.
- `crates/jet-codegen/src/Prelude/Effects.jet` holds one `effect Name`
  declaration per effect root.
- The Rust constants `Policy::APPLIED_RULES` and `Facts::EFFECT_ROOTS` no longer
  hold the vocabulary; both are read from those two files.
- D-VARIADIC1: a marker that takes a list of arguments writes it the same way a
  function does — `capabilities: ...Capability`. This is the only new spelling
  the card adds, and it reuses the ratified rest-parameter form.
- D-MARK-VOCAB1: `Policy::MarkerVocabulary` is the one vocabulary value. It
  holds the registry rows plus the `derive T.Name { … }` providers a build can
  see, and it answers both "may this name be written?" and "did you mean?". The
  separate `known_derive_names` set is deleted.
- `tests/marker_declarations.rs` is the drift guard: the real Jet parser reads
  both files and must agree with the registry on every row, and no Rust file may
  build a marker row or keep a copy of the effect roots.

## Memory audit slate (ratified 2026-08-12, card #1889 — directions ratified, all unbuilt)

Owner-commissioned first-principles memory audit; proposal at `docs/proposals/memory-one-question.md`; evidence dossiers under `docs/audits/memory-*-2026-08-12.md`. Five decisions ratified =A with the owner's checkpoint quotes on the ballots, plus one owner-designed tooling element. **Every entry below is a ratified direction with zero implementation**: the named implementation card owns the I7 Syntax.rs rows, I4 diagnostic registration and snapshots, and I9 tier proof before any entry may be read as shipped law. Fresh-context review findings are folded in below; the ballot texts remain the historical record and this section is the operative wording where they differ.

**D-MEM-COPYSEM1=A — a read-only view flowing into a non-view slot means a copy** *(ratified 2026-08-12, card #1889; implementation #1890)*: wherever today's checker refuses a read-only view entering a non-view destination — a `String` binding or field, a collection element, a stored capture (string views: E2307; generic views: E2305) — the store now means an owned copy, with the same lowering as an explicit `~` store, on every tier. Declared `View<T>`/`ViewMut<T>` fields and returns are untouched: they keep D-MEM-VIEWRET1=B provenance semantics exactly as ratified; this decision governs only the flows those boundaries already refuse. `ViewMut` flowing into a non-view slot stays an error under its existing codes (E2305/E0212 for generic views) — copying a write-through window would break write-back; nothing about legal ViewMut boundaries changes. `~x` remains the one copy spelling (D-SHAPE-COPY1 untouched); nothing is ever injected into user source. Visibility ladder: `jet audit copies` (site + size per copy), editor ghost hints, `jet fmt --explicit-copies` (invited materializer), and per D-PACKAGE-POLICY-SCOPE1's mirror law the refusal exists at both scopes — `#Policy(copies: .Explicit)` in source and `policy: .{ copies: .Explicit }` in `package.jet` — restoring today's errors with their exact fix text. Amends D-MEM-VIEWRET1's non-view-flow behavior only.

**D-CONC-FREEZE1=A — `freeze` and `^`-captures** *(ratified 2026-08-12, card #1889; implementation #1891)*: `freeze(x)` is a prefix verb returning a deeply immutable value; bare task captures of frozen values are legal because no race can exist; writing to a frozen value is a compile error naming the freeze site (code assigned, registered, and snapshotted by #1891 per I4); freezing twice is the identity. `^x` in a group-child capture consumes the value into the task (existing use-after-move E0121 applies afterward). Baseline corrected per review: D-TASKBORROW1=A already lets lexical `task.group` children borrow readable places where non-overlap is provable, and D-CONC-SHARE1=A already replaces `Shared.read/.edit` closures with `shared expr` plus ordinary field access — `freeze` covers what neither does: sharing past provable non-overlap and past the lexical group, without a lock. E1101's fix menu becomes: give it (`^`), freeze it (`freeze`), or share it (`shared`). Extends D-TASKBORROW1; composes with D-CONC-SHARE1; `Shared`/`Cell` semantics unchanged.

**D-MEM-SENTRY1=A — dev-tier sentries for `#Unsafe`** *(ratified 2026-08-12, card #1889; implementation #1892)*: default `jet run` guards raw operations inside `#Unsafe` gates — freed storage quarantined and poisoned, writes checked against allocation provenance the runtime already tracks (no new storage declaration exists or is added). A violation is a located runtime fault (proposed R08xx report family — registered and snapshot-tested by #1892 before any code ships, per I4) naming the gate and the unmet obligation, and it feeds the `jet audit memory` ledger. Interaction with D-UNSAFE-OBLIG1, stated precisely: required obligations keep their compile-time law — a runtime witness never substitutes for a required static discharge; sentries watch the operations whose obligations were asserted or `.Skip`-ped, and a fault is evidence the assertion was false on that run. **This decision is an owner-ratified I9 instrumentation carve-out, named here**: the dev and hardened tiers report a fault for operations whose release meaning is undefined behavior inside a gate; release without `harden` runs unwatched. Sentry logic lives in the Prelude; engines marshal (AGENTS.md I9). Refusal exit exists at both scopes: `#Policy(sentries: .Off)` and `policy: .{ sentries: .Off }` keep the dev tier but drop the instrumentation. The hardened shipping profile is card #1888 (open — a dependency, not shipped).

**D-FFI-CAP1=A — capability sigils across the FFI boundary** *(ratified 2026-08-12, card #1889; implementation #1893)*: extern signatures may accept `&` and `^` and a returned foreign handle may declare `#Close(fn)`. Corrected per review — `&` keeps its exact D-MEM1 meaning: **exclusive access for exactly this call** (the foreign side may read and write through it, must not retain it; no copy). `^` gives: the foreign side owns and frees; the Jet name dies. `#Close(fn)` joins the `close(^)` protocol — the named function runs exactly once or the program does not compile (S63 handle precedent). The `extern c "lib" { … }` block in the proposal is **proposed grammar**, not current surface; #1893 integrates the tier with the ratified D-FFI-UNIFY1 structure (`<lang>.<lib>` mounts, `#FFI` bodies, `extern rust "crate@version"` + `= "rust::path"`) rather than minting a parallel declaration form. Capability-tier functions are generated bindings, so the existing D-FFI-UNIFY1 rule stands: bindings are callable directly; raw symbols outside bindings still require `#Unsafe`. Tier behavior follows the existing FFI law (JIT/dev reports the native boundary; native build executes it); the web tier is out of scope for native FFI per that same law. Amends the D-FFI-UNIFY1 by-value-only rule; E0702 narrows from a ban to a gate; the by-value floor stays the default tier.

**D-MEM-GUARANTEE1=A — the guarantee dial** *(ratified 2026-08-12, card #1889; implementation #1894)*: `package.jet` policy gains `contain: [deps]` — the named dependencies compile with tracked-handle pointers inside a fence, so a wild write there faults with a report instead of corrupting the program's heap, at a cost confined to the fence — and `harden: true`, which keeps D-MEM-SENTRY1's sentries in release **and contains every foreign dependency** (with `harden: true` no dependency may report TRUSTED; the earlier sample showing one was wrong and is corrected in the proposal). `jet inspect guarantees` (proposed command, #1894) prints the per-component truth: proven, watched, fenced, or TRUSTED. **Named amendment to D-PACKAGE-POLICY-SCOPE1**: `contain` and `harden` are the first package-only policy keys — they govern dependencies and the build profile, not lexical source scopes, so they deliberately have no `#Policy` mirror; the mirror law continues to hold for every scope-governing key (`copies`, `sentries`). Both keys only tighten. Explicitly decoupled from gc (owner ruling): gc is a lifetime convenience and never a safety mechanism. R9 preserved: single files need no manifest and cannot use these keys; their guarantee line reads "your code: proven; externs: trusted". Freestanding targets report "prover + audit only" honestly.

**Tooling element (no ballot; owner-designed; implementation #1895)**: `jet audit memory` + `jet fix memory` — every runtime-witness step-in (gc activity in `#Policy(gc)` scopes, sentry faults in gates) writes one ledger row; the fix command applies the static repair where exactly one exists and names the options where several do. This **composes with** the ratified D-OPTGC1 receipt (`jet run --gc-trace` + `jet gc report`, which already exists): `jet gc report` stays the per-run gc detail; `jet audit memory` is the cross-witness ledger above it. The ledger covers exercised runs only, and its output says so.

Owner rulings recorded without ballots: the strategy-attachment unification was withdrawn on the owner's pushback (allocator, container, type, and scope-attribute are different things on purpose; `#Policy(gc)` keeps its one-line shape). The 2026-08-12 probed defect set stays ordinary repair cards under I9, outside this slate.

**2026-08-07 — the one-tree name slate: D-NAME-TREE1=A, D-NAME-FILES1=C,
D-NAME-ALIAS1=A, D-NAME-FENCE1=A, D-NAME-WALK1=A, D-NAME-ROLEMOD1=A,
D-NAME-REFLECT1=A, D-NAME-SIGIL1=A** *(card #1625, proposal
`docs/proposals/names-one-tree.md`)*.

- **D-NAME-FILES1=C** — manual named imports stay: every cross-file name keeps
  an explicit import line, and no invisible auto-import of project files
  ships. Owner's words: "Let's just stick with manual named imports like we
  used to have rather than the magic auto imports that are invisible."
  D-NAME-AUDIT1 (the refuse-the-magic switch) is withdrawn as moot.

- **D-NAME-TREE1=A** — a name is a member at one point in one tree. A
  declaration attaches it, and an alias only points at it. Sema writes one name
  ledger that every engine and tool reads.
- **D-NAME-ALIAS1=A** — Core ships one short prelude module. It declares the
  ambient basics and uses `pub use` aliases for the rest of the ratified Core
  tree. The compiler opens it for every file, and `#NoPrelude` still opts out.
- **D-NAME-FENCE1=A** — `pub` opens an edge, `pub(package)` fences at the
  package, and `_name` means internal everywhere. `pub module` makes a
  public-by-default subtree and replaces `#PubFile`; `priv` opts a member of
  that subtree back to private.
- **D-NAME-WALK1=A** — `use` and `pub use` work in module bodies with the same
  meaning as at file top. `.[ ]` always means "these members of that prefix":
  values in expression position and aliases after `use`.
- **D-NAME-ROLEMOD1=A** — `module` means code. Ecosystem entries are typed
  values in `package.jet`, and role declarations such as `module env.dev`
  respell as those values.
- **D-NAME-REFLECT1=A** — `T.reflect()` gains `.path`, while `.name` stays the
  leaf; runtime `reflect.of` follows. Diagnostics use the path when the leaf is
  ambiguous and the leaf otherwise. Hover, `jet project parts`, and JSON use
  the same spelling.
- **D-NAME-SIGIL1=A** — zero underscores is a name, one underscore is a
  message to humans, and two underscores is the machine's space. Dunder and
  sunder shapes have no meaning. Every generated symbol that a tool can show
  starts with `__jet`.

**2026-08-08 — the choosing slate: D-CHOOSE-PAT1=A, D-CHOOSE-TEST1=A,
D-CHOOSE-HEADS1=A, D-CHOOSE-FNBODY1=A** *(card #1651, proposal
`docs/proposals/choosing-one-table.md`)*. D-CHOOSE-FIND1=A already records the
finite-source finding form above. This section records the other outcomes.

- **D-CHOOSE-PAT1=A** — arm heads and `==` tests accept list and tuple shapes.
  The rest capture is `...rest`; two dots stays discard-only. Binds, loop
  heads, and the exact-length list bind do not change.
- **D-CHOOSE-TEST1=A** — the statement gives the subject, its expected shape,
  and the route for a miss. The route must leave the surrounding flow, so the
  bound names are safe afterward. A test with no route binds nothing.
- **D-CHOOSE-HEADS1=A** — the table proofs check multi-head coverage and
  overlap: E0307 reports a hole, and the unreachable-arm lint reports a
  shadowed head. The surface does not change. A named follow-up owns any later
  table-sugar choice.
- **D-CHOOSE-FNBODY1=A** — a one-line function body uses `::`. Defining a
  named thing uses `::`; filling a slot inside a definition uses `=`.
  Reassignment, parameter defaults, field defaults, and enum discriminants
  keep `=`.

**2026-08-13 — D-MARK-BLOCK1=A** *(card #1606)*. Dedicated block-construct
nodes keep their typed fields. Tools recover the registered marker row and its
argument values through one accessor instead of storing a second marker node.

**2026-08-13 — D-FAIL-EXIT1=A / D-FAIL-EDGE1=A implementation log**
*(cards #1533/#1536)*. The default-fallible `fn run()` uses the generic entry
report and exit-1 path. Program-side stops and explicit process termination
cross the shared Prelude boundary; target adapters carry that report in their
native shape. No source spelling changed.

**M03 marker ballot migration — D-MARK-FORM1=A, D-MARK-REPEAT1=A,
D-IMPURE-REASON1=B, D-SQL-ARG1=B, and D-HTML-NAME1=B** *(cards #1457 and
#1461)*: marker declarations use one ordinary named parameter list. `#Impure`
keeps an optional `reason: String`; a bare `#Impure { … }` is valid. The SQL
row type is an ordinary argument: `#SQL(Row) { … }`; the retired angle form
is rejected with a teaching fix. `#HTML` keeps one name with a site-dependent
registry signature: `#HTML(path)` at file sites and bare `#HTML { … }` at block
sites. Duplicate applications remain E0999 unless the registry row is
repeatable. These rows are declared in `Prelude/Markers.jet` and read through
the shared registry.

**2026-08-13 — D-CALLPOLICY1=E and D-CALLPOLICY2=C** *(card #1396)*.
`#Policy(retry(3), trace("users.load"))` is the one typed callable-wrapper
marker. Its chain is the declaration default. `apply(retry(3), fn)` replaces
that default with exactly the named chain; `apply(fn)` selects the bare
function.
The replacement copies only policy metadata and preserves the complete
checked `fn` contract: labels, defaults, access, zones, effects, errors,
variadics, and returned-view provenance. The old scoped `PolicyKey` entries
move to their ratified homes: memory keys are rights-tree denials, unsafe is
an authority-manifest permission, and `explicit_units` is a typed fact-plane
setting. They are not aliases or fallback spellings.

**2026-08-13 — D-META-USER1=A / D-META-DSL1=A implementation log** *(cards
#1540, #1542)*: a `marker` declaration may have an ordinary checked body;
without one it records facts, and with one it may reject or emit additive
items. A checked text block uses the closed `#Name { … }` boundary and resolves
the name through the declaration registry; `#SQL<Row>` and `#HTML { … }` now
use their declared `.Block` rows. The old marker-only DSL whitelist is retired
by the syntax-chore lane.

**2026-08-13 — D-CONF-MODULE1=A implementation log** *(card #1524)*.
Declared profile settings now seed the shared typed build-fact snapshot. A
closed `@build.settings.<name>` expression is folded by the same Tier-0
evaluator used for generic-module value arguments, and its normalized value is
part of the existing instance fingerprint. Semantic instance facts retain the
profile and declaration provenance; no execution tier reads settings.

**2026-08-13 — D-MEMO1=A / D-FIELDMEMO1=A** *(cards #1871/#1872)*: `#Memo`
is one marker family. A pure function uses its shared bounded store by default
(`128` entries), with `#Memo(bound: none)` for an unbounded store; lazy
`Iter<T>` parameters or results are refused. `name.cache()` reads hits,
misses, current size, and the written bound. A computed field uses the same
bare `#Memo` spelling before `name: T => expr`. Its first valid read is
retained, and a write to any stored sibling in the formula's dependency graph
drops that field's result. Without `#Memo`, D-FIELDPOL1 remains
recompute-on-read. The field form has no cache arguments and is a compile-time
error when attached to an ordinary field (E0382). The marker registry and one
Prelude cache implementation own the spelling and execution-tier behavior.

## The one-report slate (D-REPORT-*, ratified 2026-08-07, card #1626)

All six D-REPORT decisions are ratified as option A. This slate extends
D-FAIL-BREACH1's one runtime report family and one renderer to every report
moment and surface. D-REPORT-HOME1 amends the I4 `Adding a diagnostic` process;
D-REPORT-MACHINE1 amends D-DX1. The card numbers below record the current
implementation handoff. They do not claim that any card is complete.

- **D-REPORT-LAW1=A — one law for every report (implementation #1712).** Every error, warning, stop, test failure, and tool failure across compile, run, test, tool, web, and editor is a registered report with one code, What, Why, and Fix, rendered by one renderer per surface. Status, progress, success, ledger, and usage lines stay plain. Amendment: D-FAIL-BREACH1's one-renderer rule now covers every report moment. Required guards: one UI snapshot for each owned report surface, and I9 proof that one condition has one report with the same meaning and text under AOT, `jet run`, and the interpreter; the registry or Prelude owns the text, and engines only marshal it.
- **D-REPORT-HOME1=A — typed rows are the one text home (implementation #1806).** A typed row owns code, severity, moment, What/Why/Fix templates, named holes, and structured fixes. The registry generates the spec, error pages, and `jet explain`; no renderer or emitter keeps a second copy. I4 `Adding a diagnostic` now requires the row, its snapshot, and both-direction coverage. Required guards: the card's UI snapshot proves the complete row-backed report, and I9 proves the same row-backed meaning under AOT, `jet run`, and the interpreter.
- **D-REPORT-SEV1=A — two severities and the ratified words (implementation #1678).** Blocking and advisory are the only levels. `Error`, `Stop`, and `Warning` remain the frame words; a contained failed assertion is `Stop` in the E3001 family, not a new `Fail` word or code block. Moment is row metadata, not a new severity. The duplicate severity enum is deleted; E and L prefixes stay, and W0410 stays the frozen exception. Required guards: UI snapshots cover each changed severity form, and I9 proves identical severity meaning and frame text under AOT, `jet run`, and the interpreter.
- **D-REPORT-TEST1=A — a failed test shows the full report (implementation #1684).** A failed assertion or golden check prints the full report frame under the test ledger, through the same renderer used by the stop path, with registered code, location, and caret. A golden failure states whether the file is missing or the output changed and includes its diff. Passing output stays unchanged. `JETTEST2` proof records stay separate from JSON reports. Required guards: one UI snapshot covers the failed-test frame, and I9 proves the E3001 report is the same under AOT, `jet run`, and the interpreter.
- **D-REPORT-MACHINE1=A — one JSON line for every report (implementation #1713).** Each report is one complete `jet.report/v1` JSON object per line, with schema, moment, severity, code, What, Why, Fix, detail, location, span, `fix_edits`, cause, and clears fields when applicable. Command status envelopes remain status data; every report inside them uses this object. Amendment: D-DX1 changes `message` to `what`, `suggestions` to `fix_edits`, and `schema_version` to `schema`. The failed-test example uses registered code **E3001**, never provisional E3020. Required guards: UI/machine snapshots cover the exact emitted object, and I9 proves one condition emits the same report meaning under AOT, `jet run`, and the interpreter; no engine invents a second JSON shape or report field meaning.
- **D-REPORT-EDITOR1=A — the editor gets the whole report (implementation #1807).** LSP uses What as `message` and carries Why, Fix, `fix_edits`, cause, the `jet://explain/<CODE>` link, and related locations in structured fields. Code actions reuse the row-owned edit used by `jet fix`; no editor path parses prose or keeps editor-only text. Required guards: LSP/UI snapshots cover the full payload and action, and I9 proves the same report meaning and recovery data under AOT, `jet run`, and the interpreter.

These six cards share exact truthfulness guards. Every report code is registered;
every row has What, Why, Fix, and a snapshot; every emitted code has one row;
every row is emitted, retired, or reserved; locations name the source that the
consumer can open; and `fix_edits` apply to the named span. No status line is
promoted to a report, no report falls back to a bare string, and no engine,
editor, JSON writer, or web host re-creates registry meaning. A missing tier
proof is an open criterion, not a `tests/jit_gaps.txt` entry.
