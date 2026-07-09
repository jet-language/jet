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

## Ratified

### Names & conventions

**N1 — Language name**: **Jet**. Binary `jet`.

**N2 — File extension**: `.jet`.

**S54 — Naming convention**: PascalCase for types/traits/enums/constants;
snake_case for functions, module path segments, locals. No naming lint in v1;
`jet fmt` is layout-only.

**S66 — Standard acronyms fully capitalized** *(D-ACRONYM-CANON1)*: `JSON`,
`TOML`, `YAML`, `CSV`, `IOError`, `UTF8Error`, `U8`. No PascalCase aliases.

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
every `#`-marker and every `@`-marker is PascalCase (`#Test`, `#Unsafe`,
`#Grant`, `@Pure`, `@Pre`); traits are PascalCase. The standard library is
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

**S2 — Bindings** *(current law = D-BIND4)*:

```jet
name :: expr            // immutable binding
name := expr            // mutable binding
name: Type :: expr      // explicit-typed immutable
name: Type := expr      // explicit-typed mutable
name = expr             // reassignment of an existing := binding
```

**S4 — Type annotations**: `name: Type` after the name, everywhere (bindings,
params, fields). Never `Type name`.

**S17 — Compound assignment**: `+=` `-=` `*=` `/=` `%=` `&=` `|=` `^=` `<<=`
`>>=`. Arithmetic four on Int/Float; the rest Int-only. LHS must be a mutable
binding or `&` parameter.

**D-INCR1 — Increment/decrement**: `++x`, `x++`, `--x`, `x--` on mutable
integer lvalues; prefix yields the new value, postfix the old. Indexed slots
rejected; non-integer E0162; immutable E0161. Deliberate second spelling
beside S17 (owner-chosen I8 exception).

### Functions

**S1 — Function keyword**: `fn`.

**S12 — Entry point**: `fn run()`; no `pub` required. May be fallible:
`fn run() -> Void ?` (S80, D-S80-RUN1). **D-CLIFLAG1** (implemented, c7cliflag): a
typed entry parameter opts into CLI parsing — `fn run(args: ServeArgs)`
derives `--flag` names/defaults/help from the struct's fields
(`@[Cli]`/`@[Doc("...")]` markers, bracket form matching `@[Codable]`); an
`enum` param derives subcommands. There is no Jet `main` entry and no
variadic entry signature. Raw argv access stays explicit inside `fn run()`
via `core.args`/`core.io.args`. See docs/spec/spec.md
"Typed entry-signature CLI parsing" for the full field-mapping rule. The
existing `core.args` `ArgsSpec` builder (D-ARGS1) remains the library floor
for non-entry parsing; the typed layer generates onto it rather than adding
a second parser.

**S27 — Methods**: `self` receiver with capability sigils (`^self`,
`&self`; bare `self` = read, D-MEM1). Call `value.method(args)`. Methods live in the
type body, in `impl Type { }`, or top-level `fn Type.method(self)`
(**D-EXTMETH1** — `.` connector, orphan rule: same source module; `~~` retired
→ E0325). No-`self` fn in a type = static method (`Circle.unit()`).

**D-CTOR1 — Named constructors only**: many ways to build a type = many
named statics (`Point.cartesian(…)`, `Point.polar(…)`); duplicate name E0105.
No marker keyword — return-type-is-the-type identifies a constructor (D-CTOR2).

**S46 — Lambda syntax**: `(params) => expr` / `(params) => { … }`. `=>` is
the lambda arrow; `->` is return types and dispatch arms. **D-LAMBDAINFER1**:
a lambda param type may be omitted where the expected type fixes it
(`xs.filter((i) => i.state == .Open)`); required elsewhere (E0801);
one-directional inference only. **D-LAMBDA-INFER1** *(ratified 2026-07-04)*:
a single bare param may ALSO drop the parens — `xs.filter(m => m.hp > 0)` —
wherever the expected type fixes it (same E0801/one-directional rule); no
`take` prefix on the bare form (write `(take x) (x) => …`). The
parenthesized/typed forms stay available for anywhere the type can't be
inferred.

**S47 — Function types & captures**: fn type `fn(T1, T2) -> R`. Named `fn`s
coerce to function values. Captures follow M2: shared read for read-only
names, mutable borrow for written names. Escaping closures own captures:
clonable auto-clone (L0801); non-clonable need `take(name)` prefix.

**S61 — Argument labels & defaults** *(D-NARG1, D-NARG2)*: optional call-site
labels, positional order fixed — labels never reorder; wrong label = compile
error showing the order. Trailing defaulted params omittable
(`fn f(x: Int, urgent: Bool = false)`). Methods/constructors behave the same.
fmt never adds nor strips labels.

**S83 — Multi-head functions**: same name, different parameter patterns, each
head its own body; dispatch by argument shape; heads must be exhaustive.

```jet
fn area(Circle(r: Float)) -> Float { return 3.14 * r * r }
fn area(Rect(w: Float, h: Float)) -> Float { return w * h }
```

**D-VARIADIC1 — Variadics & spread**: `...` everywhere — param `name: ...T`
(final position), call spread `f(...xs)`, list spread `[...a, x, ...b]`
(E1310–E1312). **D-ANY-JAI1**: heterogeneous varargs are trait-bounded —
`parts: ...Renderable`; trait sets use list bounds
(`fn f<T: [Renderable, Serializable]>(parts: ...T)`, **D-VARARGBOUND1**).
No top type; general `Any` rejected.

**D-TRAILBLOCK1 — Trailing block argument**: when a call's final param is a
function type, a bare `{ }` after `)` stands in for that lambda —
`ui.button("Save") { prefs.save() }`. Zero-parameter blocks only in v1
(E0334/E0335).

**Declined (functions)**: UFCS (D-UFCS1); call-site macro-method expansion —
inlining via `@Inline`/`@InlineAlways` contracts instead (D-METHODMACRO1);
expression-body `fn f() = expr` (D-FP2); pipe `|>` (D-SUGAR2).

### Control flow

**S3 — Blocks**: curly braces `{ }`, always required.

**S19 — Loops** *(one keyword; header picks the mode; D-LOOP-SEMICOLON1)*:

```jet
loop { … }                        // infinite
loop n > 0 { … }                  // conditional (no `while`)
loop i in 1..5 { … }              // iteration (no `for`)
loop i := 0; i < n; i++ { … }     // counted (C-style header, semicolons kept)
```

`while`/`for` are not keywords. Iteration head may be any iterable expression
(`loop p in shape.points()`), evaluated once (**S79**).

**S22 / S72 — Ranges**: `1..10` is **inclusive**. Optional `step n`
(`0..10 step 2`); `step` is contextual; non-positive literal step E0123.

**S23 — Loop control**: `break`, `continue`. Labels are **suffix `@`**
(**D-LABEL1 + D-LOOPLABEL2**): `outer@ loop { … break outer@ }` /
`continue outer@`; prefix `@outer` → E0988; unknown label E_UNDEFINED_LABEL.

**D-ORRETURN-CANON1 — Early-exit fallbacks**: `expr ?? return`,
`expr ?? continue`, `expr ?? break` are the only spellings (`?return` etc.
removed; E0115).

**S68 — `if`: two-arm, expression, and dispatch** *(D-IF1 + D-IF3 +
D-MATCHARM1/2)*: `if` is the only branching keyword.

- Statement form `if cond { } else { }`; parens optional, fmt strips them.
- Expression form `m :: if a > b { a } else { b }` — `else` required (E0003),
  branch types must match (E0124).
- **Dispatch form** — `==` between subject and `{` is required (bare
  `if subject { arm -> … }` is E0992, auto-fixed):

```jet
if code == {
    200 -> print("ok")
    301 | 302 -> redirect()          // `|` alternates values
    .Error(e) && e.fatal -> die(e)   // pattern + boolean guard
    code >= 500 -> retry()           // predicate arm
    else -> log(code)
}
```

Arms: bare values (compared to the subject), leading-dot enum patterns,
predicates, guards via `&&`/`||` (booleans only — no comparison
distribution); `|` binds tighter than `&&`/`||`, mixing without parens is
E0328. Catch-all is `else ->`. Braceless single-expression bodies allowed.
Exhaustive pattern arms may omit `else`.

**D-PROTO1 / D-PROTO2 — Protocol blocks**: `protocol Name { client ->
server: Msg(...) }` generates `.Client`/`.Server` handle types over
linear+typestate machinery (E0153/E0154). Temporal ordering comes from the
typestate transition graph itself — no separate surface (D-ROLE1).

### Patterns & matching

**S31 — Pattern tests**: `==` with a pattern RHS when the LHS is an enum or
`T?` — `if s == Rect(w, h)`, `x == None` — yields Bool. Patterns nest to any
depth (`r == ok(Rect(w, h))`). Guards are plain `&&`: a pattern-bound name is
in scope for the rest of the same condition. No `is`, no Rust `match`.

**D-ENUMDOT1 / D-ENUMDOT2 — Leading-dot variants**: match-arm patterns take a
leading dot (`.Circle(r)`, `.Empty`); value position too when the expected
type is known (`.Red`; E0330 fallback). `Color.Red` always valid.

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

**S75 — Fan-out**: `f.[a, b, c]` ≡ `[f(a), f(b), f(c)]`; `f` must be a
one-argument callable; items typed by f's param; result `[T#N]`. Flattens
inside an enclosing list literal; no spread `f.[*xs]` (E0961/E0962).
**D-FANOUT2**: no second fan-out axis (`s.{…}`) without real-use evidence.

**D-SWIZZLE1 — Vector swizzles**: `v.xyz`, `v.wzyx`, lvalue `v.xy = .{…}` on
lane/vector types; overlapping writes diagnosed (E3110/E3111).
**D-VECARITH1**: element-wise `+ - * /` closed to compiler-provided lane and
linalg types; user structs use methods.

### Types

**S11 — Built-in type names**: `Int`, `Float`, `Bool`, `String` (capitalized).

**S42 — Numeric types & conversions**: `Int` (i64) / `Float` (f64) are the
defaults; sized menu `I8 I16 I32 I64 U8 U16 U32 U64 F32 F64` for experts/FFI.
Conversions are named methods only (`n.to_float()`, `.to_u8()?` fallible
narrowing, `Int.parse(s) -> Int ? ParseError`); no `as`, no cast punctuation.
**D-NUMOPS1/2**: plain integer arithmetic **traps on overflow** at every
width; opt in per-op with `wrapping(…)` / `saturating(…)` /
`checked(…) -> T?`. Per-type `MIN`/`MAX`, float `INFINITY`/`NAN`/`EPSILON`,
bit ops. **D-FLOATW1**: `core.math` is width-generic; mixing F32 and Float is
a compile error with a convert fix-it.

**S21 — Float display**: a `Float` always prints a decimal part (`-5.0`).

**S32 / D-OPT-SPELL1 — Option** *(ratified 2026-07-04)*: `T?`; `Val(expr)`
present, `None` absent; no nullable plain `T`. `Val` is a PascalCase
constructor call (not a keyword — same non-keyword-identifier mechanism the
old `value` spelling used); `None` is a real keyword. Old `value`/`null` are
retired (greenfield: an ordinary unknown-identifier/parse error, no teaching
text). `Some`/`nil`/`none`/`some` remain foreign guesses for E0020, now
pointing at `Val`/`None`. **D-RESULT-OPTION-CANON1**: `T?` always means
Optional; fallible is spaced `T ? E` / `T ?` (S34).

**S33 — Generic type arguments**: `Type<Args>` angle brackets; `[]` is
reserved for collections/indexing/shorthands. No call-site type args in
general positions (exception: `decode<T>` turbofish blessed as general
grammar by D-SERDE6).

**S45 — Generic functions & types**: `fn largest<T: Comparable>(…)`,
`struct Pair<T> { }`; multi-trait bounds are lists `<T: [A, B]>`
(D-VARARGBOUND1). No `where`. **D-LIB2**: associated types + default method
bodies; no higher-kinded types.

**S73 — Tuples**: named members only — `p :: (x: 1, y: 2)`, `p.x`, type
position `(min: Int, max: Int)`. No positional tuples, no `.0`.

**S76 / D-FIXARR1 — Fixed-size lists**: `[T#N]` is a compile-time-length
refinement of `[T]`, lowered to a **real stack array**. `::` + literal/fan-out
⇒ `[T#N]`; widens one-way to `[T]` (by copy); `.map` preserves N; `.len` is a
compile-time constant; length-changing ops rejected (E0963–E0965).

**S29 / D-DOTCTOR1 / D-DOTCTOR2 — Construction**: the only struct-literal
spellings are **`Type.{ field: expr, … }`** (named) and **`.{ … }`**
(type inferred from expected type — the U18 expected-type elaboration, now
dot-spelled). Dotless `Type { … }` is E0320. Every field exactly once, any
order; flush style `Point.{x: 3.0, y: 4.0}` (S29-FLUSH). `.{}` constructs an
empty/unit value.

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
nominal type over a base; no inherited operators. **D-CAPBUNDLE1**: capability
bundles re-expose curated slices, stackable — `@Numeric` (`+ - * /`, ordering,
same-type only; E0138), `@Comparable`, `@Printable`, `@CodableAsBase`.
`Usd + Eur` stays a type error; `.raw()` strips. **D-RANGETYPE1 — range-constrained
types**: `distinct Int(0..10)` is an `Int` provably within bounds; literal
construction checks at compile time (E0135 out of bounds), runtime construction
is fallible (`Severity(raw)?`, else E0136); an empty/reversed range is E0137;
arithmetic widens to the base type.

**D-FIELDPOL1 — Computed fields** *(ratified 2026-07-03, card #181)*: a struct
field `name: T => expr` is never stored — every read recomputes `expr` against
the struct's current field values (siblings, data or computed, are readable by
bare name inside `expr`). Unsettable: absent from a `Type.{ … }` literal's
required/allowed field list (E0339 if provided), and direct assignment
(`s.field = v`, `s.field++`) is also E0339. A cycle among computed-field
formulas, including self-reference, is E0338. Codegen: not a Rust struct
member — a synthesized inherent getter method instead; every read routes to a
call of it. `@[Patchable]` (D-PATCH1) excludes a computed field from `T.Patch`
and from `apply`/`diff`/`merge`. `@[Codable]` encode calls the getter (the
field appears in the wire output); decode never reads into it.

**D-QUAL3 — Unit families**: `#UnitFamily(currency) { usd, eur, gbp }` mints
one distinct type per member (usd → `Usd`, erases to the base numeric);
cross-unit mixing reuses E0127. **D-UNITLIT1 — unit literals**: `500ms`,
`12.50usd` resolve against in-scope family members (E0134 unknown suffix); no
implicit cross-unit conversion; `e`+digits reserved for float exponents.
Dot-construction `px.{100}` also valid.

**D-TYPEALIAS1 — Aliases**: `alias X = Y` transparent aliases, scoped to
shortening generic spellings only — not primitive/unit newtypes (use
`distinct`). **D-TYPE-ALIAS-CANON1** + **D-LISTMAP-CANON1=A**: `[T]`, `[K: V]`, `*T`
are the only default container/pointer spellings; `List<T>`/`Map<K,V>`/`Ptr<T>`
are dead. Named specific collection spellings stay named rather than short
bracket forms; shipped today: `Set<T>`, `SortedSet<T>`, `Deque<T>`,
`PriorityQueue<T>`, `Lru<K,V>`, `Bag<T>`, `BitSet`, and `ByteBuffer`.
`HashMap<K,V>` and `BTreeMap<K,V>` are reserved names for specialized map
implementations.

**D-BIGINT1**: Core `BigInt`, explicit construction `BigInt(…)`/`BigInt("…")`;
`Int` never auto-promotes (E0130–E0133). **D-DECIMAL1**: arbitrary-precision
base-10 `Decimal` in `core.numeric`; default-on lint L0504 fires when a
money-named field holds a float (`#[allow(float_money)]` suppresses).

**D-STATE1 — Typestate** *(D-STATE-REQ/TRANS/DECL)*: states declared in a
`state TypeName { A, B, C }` block; `#State(S) fn m(self)` requires state S;
`#Transition(From -> To) fn` advances it (`_` from-state = entry constructor).
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

**S65 — List type shorthand**: `[T]` is the canonical list-type spelling.

**S64 — Map shorthand & entry iteration**: `[K: V]` is the canonical map-type
spelling. One-binding map iteration yields `.key`/`.value` entries;
two-binding `loop name, amount in fruits` also supported.

**S39 — Indexing**: `xs[i]` / `m[k]` stop with a friendly report on
OOB/missing key; `xs.get(i) -> (T?)` safe access; `m[k] = v` inserts.

**S40 — Slicing**: `xs[a..b]` inclusive, copies (no exposed references);
`s.slice(a..b) -> String` on character positions; L0501 lints slice copies in
loops.

**D-ITER1 / D-ITERTOOLS1=A — Iterator adapters**: `map`, `filter`, `each`,
`find`, `any`, `all`, `sort_by`, `reduce`, `take`, `skip`, `step_by`, `dedup`,
`chunks`, `windows`, `enumerate`, `zip`, `unzip`, `take_while`, `skip_while`,
`flat_map`, `filter_map`, `scan`, `fold`, `sum`, `product`, `min`, `max`,
`min_by`, `max_by`, `group_by`, `count_by`, `partition`, `flatten`, and
`intersperse` use one iterator model. Methods return materialized collections
until the lazy protocol lands; no second adapter spelling is introduced.

**D-COLLBREADTH1 / D-ITERTOOLS1=A**: `Set<T: [Hash, Eq]>`,
`SortedSet<T>`, ring-buffer `Deque<T>`, `PriorityQueue<T>`, `Lru<K,V>`,
`Bag<T>`, `BitSet`, and `ByteBuffer` in Core (E0506). `[K: V]` is the default
ordered map spelling; specialized map names stay reserved. **D-ENC-DYN1**:
`Data` is the single dynamic value
(`.Object/.Array/.Int/.Float/.Text/.Bool/.Null`); `Json`/`Toml`/`Yaml`/`Csv`
are aliases over it. **Declined**: `[..]T` spelling — zero-copy comes as
`View<T>` library type (D-DYNARRAY1).

### Strings & literals

**S8 — Interpolation**: `"hi {name}"`; no `+` concatenation.
`{value@Debug}` selects the Debug rendering (D-DISPLAYDBG1).

**S20 — Escapes**: `\n` `\t` `\"` `\\`; literal braces `{{` `}}`.

**S41 — Char & string length**: `Char` built-in, `'a'` literals; `s.len()`
counts Unicode scalars; `loop c in s.chars()`; no `s[i]` (E0503).
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
is plain text equality, not a pattern (I8). **D-TYPEDTEXT1 — Typed text**: a
string literal (with or without interpolation) in a position whose expected
type is `Sql`/`Html` elaborates to that checked value instead of `String` —
each `{hole}` becomes a bound parameter (Sql) or an HTML-escaped insertion
(Html); a runtime `String` reaching the position directly is E0149.
`Sql.raw("…")`/`Html.raw("…")` is the sole audited escape. Implemented for
the expected-type path (function params, bindings); `.template()`/
`.params()` (Sql) and `.text()` (Html) read the checked value back.
**D-TYPEDTEXT2 — Typed text amendment**: hole-free string literals also
elaborate (not just interpolated ones); `sql"…"`/`html"…"` prefixes for
bindings without an expected type use the same typed-text rewrite as
expected-type literals; user-defined prefixes deferred to E4.

**D-SHIFT1 — Shift-style stream parsing (ratified 2026-07-01, c7shift)**: the
Jai `shift` idiom lands as a core cursor surface, not an operator (option C —
`r >> U32` punctuation — rejected). `Reader.over(bytes)` wraps a `[U8]` with a
position: `read_u8`/`read_u16_le|be`/`read_u32_le|be`/`read_u64_le|be`,
`take(n: Int)`, `remaining()`, `at_end()`; every read advances and is
fallible (`T ? String`) — a bounds miss is an ordinary error value.
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

**S34 — Fallible return**: `T ? E`; bare `T ?` means `T ? Error`. Lowers to
Rust `Result` (not surface syntax).

**S80 — Error carrier & fallible `run`** *(D-ERR2, D-S80-RUN1)*: default `Error` carries
message + optional code + optional source (`Error.message("…")`,
`Error.code(n)`, `Error.with_source(e)`). `fn run() -> Void ?` allowed;
returned errors print in the diagnostic voice, exit non-zero. Cross-type `?`
conversion is opt-in via the `Fallible` trait (`fn to_error(self) -> Error`);
prelude types implement it, unrelated enums never convert silently.

**D-ERRCTX1 — Error context**: automatic `?`-propagation trace in dev builds;
stdlib `.context("msg {var}")` (lazy) for human wording. No new grammar.

**S36 — Bug stops**: `panic("msg")` (friendly report, exit 70);
`require(cond[, "msg"])` for invariants/preconditions. Prelude builtins.

**D-IGNORERET1 / D-IGNORERET2**: discarding a fallible/`#MustUse` result
requires visible intent. Shipped spelling is `.drop("reason")`; sema lints at
the discard point and examples cover the fallible path.

**Teaching & lint law**: `=` in a condition is E0322 with a "did you mean
`==`?" fix (D-ASSIGNCOND1). Homoglyph confusable names lint L0503 default-on
(D-CONFUSE1). Semantic-smell lints — float `==`, duplicate branches
default-on; full always-true condition coverage is tracked by #343.

### Modules, visibility & imports

**S16 — `use`** *(D-S16-USE, D-MOD1/2, D-MOD-DIR, D-SELIMPORT1)*: quotes mean
a file path, no quotes mean a module; `as alias` optional in both.

```jet
use "./lib"                      // file path, namespace lib
use "grades/scoring" as g        // file path, alias
module math                      // finds math.jet, then math/module.jet (E0607)
use math.clamp                   // selective import
use math.{sin, cos as c}         // grouped + aliased selective import
```

Two-step dot access; `use math.*` wildcard rejected (E0612, D-GLOBIMPORT1).
Re-export is `pub use` (D-MOD4); a directory module's summary file is
`module.jet`. Ambiguous module resolution E0606/E0607. `import` is not a
keyword.

**S18 — Visibility** *(D-MOD3, D-VISDEFAULT2, D-PUBPKG1)*: private by
default; `pub` exports. `#PubFile` flips a file to public-by-default with
`priv` marking exceptions (E0412–E0418). `pub(package)` exposes to the same
package/workspace only (other `pub(...)` forms E0411). Cross-file private
access E0605/E0609.

**U3 — Module declarations**: `module name { … }` is the single outermost
construct; multiple per file; leading `_` disables a module. Modules never
import each other — they contribute to the merged whole. Reserved
namespaces currently live for Jetpack/jetos: `env` (`Env`), `system`
(`System`), OCI/jetos `image` (`Image`), `workspace`. **D-JPK-MODBODY1**:
role namespaces live in the declaration
name — `module env.dev { packages: […] }`, `module image.server { … }`.
`system.*` is the jetos host declaration surface.

**U8 — Manifest fields nest in the module body**: a module's `sources:`
(`name: provider@target` entries, merged by key) and `imports:` are fields
inside `module name { … }`, never file top-level.

**U4 — Import-tree discovery**: `imports: find("./modules")` auto-discovers
`.jet` files and merges typed contributions; no manual lists.

**D-GENMOD1 — Generic modules**: ML-functor style — a module parameterized by
type/value; instantiation yields a specialized normal module (E0850–E0854).

**U17 — Library packages**: consumed with ordinary `use <pkg>`; executables
go on PATH, never `use`. **D-PRELUDEX1**: prelude opt-out exists; no library
may inject into the no-prefix surface. **Declined**: `namespace { }` keyword
(D-NAMESPACE1).

### Traits, generics & derives

**S28 — Traits** *(D-IMPLDOT1)*: explicit named capabilities, never
structural. `trait Shape { fn area(self) -> Float }`. Two equivalent impl
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

**S55 — Built-in derive policy** *(D-SERDE-CANON1 vocabulary)*: silent
auto-derive for `Printable` and `Equatable` whenever every field qualifies; a
hand-written impl overrides. Explicit opt-in markers for the rest —
`@Comparable`, `@Debug`, `@Summarize`, and the codability family `@Codable`
(≡ `@[Encode, Decode]`), `@Encode`, `@Decode` (D-SERDE4, D-MARKERMOVE3).
`Serialize`/`Deserialize` are not Jet words. Field-level wire markers stay on
the `#` plane (see Serde under Core library).

**D-DISPLAYDBG1 / D-DISPLAY-SHAPE — Display & Debug**: `Display` is
user-facing — a single explicit method `fn display(self) -> String`, no
default (E0915, L0520); interpolation `{}` calls it. `Debug` is dev-facing and
auto-derived; `{value@Debug}` selects it; `#[Redact]` on a field renders
`"[redacted]"` (D-DEBUG-REDACT).

**D-ITER-HOOK / D-INDEX-HOOK — Extensibility hooks**: beginners use
`.each`/`.to_list()` and `.get`/`.set`; experts implement
`Iterable`+`Iterator` for `loop x in mytype` and `Index`/`IndexMut` for
bracket syntax. Built-in `[T]`/`Map` keep native paths.

**D-ROLLBACK-TRAIT**: `trait Rollback { type Snapshot; fn snapshot(self) ->
Snapshot; fn restore(&self, snap: Snapshot) }`; restore total;
`derive Rollback` = field-wise clone impl.

**D-EXT1 — Extensibility ceiling**: Tier 0/1 (methods, traits, operators on
own types via bundles) open to all; Tier 2 DSL blocks stdlib-only; Tier 3
proc macros and Tier 4 grammar/sigil changes rejected — even for experts.

### Markers & attributes

**D-MARKER-FAMILY1 — Two-plane sigil law**: **`@` states a checkable
contract** about the declaration it precedes (`@Pure`, `@MustUse`, `@Codable`,
`@Pre`, `@Persist`); **`#` is a directive** — changes what compiles, when code
runs, what's legal in a region, or supplies a compile-time value (`#Unsafe`,
`#Test`, `#(Fs)`, `[T#N]`, `pkg#1.2.3`, `#Caller()`) and may appear inside
types/expressions where `@` never does. `$` is splice-only. Loop-label suffix
`@` is a different slot.

**D-MARKERMOVE1/2/3 — Plane assignments**: on `@`: `Pure`, `MustUse`,
`Codable`, `Encode`, `Decode`, `Experimental`, `Tested`, `Hardened`,
`PublishedSchema`, `Redact`, `Numeric`, `Debug`, `Summarize`, `Comparable`
(user derives of the same names stay `#`). `@Pure` also valid as a
function-type bound (`f: @Pure fn(Int) -> Int`). Field-level wire markers
stay `#`: `Rename`, `Skip`, `Default`, `Flatten`, `RenameAll`,
`DenyUnknownFields`, `Tag`, `Untagged`.

**S82 — Marker grammar shapes** *(sigil per plane above; D-ATTR2)*:
`@Marker` / `#Marker` single, line before the declaration;
`#[A, B]` / `@[A, B]` comma lists (no Rust `#[derive(…)]` wrapper);
`#Marker { … }` scoped region statement (`#Unsafe { }`, `#Transact { }`) or
in-body config as a type body's first statements. `comptime` stays a prefix
keyword. LSP surfaces applicable markers per item.

**D-CANVASSTATE1=D — Statement switch attributes**: `#Off <stmt>` parses and
type-checks the statement, then emits no code in every build. `#DebugOnly <stmt>`
parses and type-checks the statement in every build, emits in debug/dev builds,
and strips from release output. Both attach to statements only; item position is
E0342, expression position is E0343, and doubled switch attributes are E0344.
Names introduced inside the marker body do not escape. `build.profile` is not a
user-typeable comptime value.

**D-DOTSCOPE1 — Scope members**: inside a `#Marker { }` block body, a
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
Current implementation records Float local origins; `speed.origin() -> String`
returns the tracked source note, and untracked Floats return `"untracked"`.
No `Tracked<T>` wrapper exists and no general value-history type is introduced.

**D-QUAL2 — Tag vs trait**: exactly two qualifier kinds — `trait` (has
methods, dispatches) and `tag` (no methods, erases). Methods on a tag E0732;
tag where dispatch expected E0731. **D-QUAL4**: type-position value tags are
prefix — `#Tainted String`.

**D-MATURITY1**: `@Experimental` / `@Tested` / `@Hardened` are doc-only
markers before `fn` — parsed, erased, zero semantic effect.

**D-PATCH1 — Typed patches** *(ratified 2026-07-03, card #181)*: `@[Patchable]`
on a struct `T` synthesizes `T.Patch` — every field wrapped `T?` (Option),
absent field = no change. Generated methods: `t.apply(patch) -> T` (apply
onto a base), `T.diff(new, old) -> T.Patch` (static; fields that changed,
`None` where equal), `patch.merge(other) -> T.Patch` (`other` wins on
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

*History:* D-CAP7's original text (pre-2026-07-03) had a third visible
parameter sigil `~T` (edit/mutate), a fourth `*T` in parameter position, and
an `Infer` tier where a bare `T` param elevated by body usage. D-MEM1
superseded all of that: unmarked is always read (no elevation, ever), `~` is
gone from the grammar entirely (ordinary syntax error — no compat, per the
rule at the top of this file), and `*T` never shipped further than this doc
as a *parameter* sigil (raw-pointer access stays the separate `p.*`/`*x`
expression mechanism, D-CAP9, untouched by the migration).
`AccessConvention::Share`/`::Raw` remain dead enum variants in the compiler,
inert until a future tier reactivates them.

**D-CAP8 — Unmarked default (retired 2026-07-04 by D-MEM1/S2)**: originally,
an unmarked param elevated by body usage and froze its resolved capability
at a `library { api: explicit }` boundary (drift = E0912, see D-CAP4/5/6).
D-MEM1 deleted elevation and the freeze tier outright: unmarked is always
read, no inference, no `api:` manifest field (an ordinary unknown-key error,
E1216) — see D-MEM1 below.

**D-MEM1 — Memory model v5, "the borrow checker, humanized"** *(ratified
2026-07-03, migration in progress — card #187; plan
tools/Tower/docs/plans/memory-v5-migration.md)*: supersedes the D-CAP7
spelling assignments and D-CAP8 when the migration lands. Three sigils:
unmarked = read (enforced — no elevation, no freeze; no `api:` manifest field),
`&T` = exclusive write, `^T` = take; `&`/`^` mirrored at call sites;
`&self`/`^self` receivers. `~` is not part of the v5 grammar (ordinary syntax
error — no compat, ever, per the rule at the top of this file). Borrows are
second-class — no `-> &T` returns, no `&T` fields (D-REF-SHORTHAND1/2 and
E0207/E0427 deleted); string/list slices are counted view values. L0201
deleted — moves of named bindings are always written `^`; temporaries pass
freely; `copy x` (D-CAP2) is the one copy spelling. Named escape hatches
`Shared<T>`, `Pool<T>`/`Id<T>`; module `policy` floors (`no_alloc` first).
**S1 shipped (2026-07-04)**: `&` is the write sigil, `~` is gone from the
grammar, call sites/receivers/formatter speak v5 spelling. **S2 shipped
(2026-07-04)**: unmarked param is `Read`, decided at parse time — `Infer` and
body-usage elevation are gone; a body write or an escape/consume of an
unmarked param is a hard error (fix-it: add `&`, or `^`/copy it); L0201 is
gone (E0209 hard error, no silent clone ever); `CapabilityFreeze`/E0912 are
gone and the `api:` manifest field no longer exists (ordinary unknown-field
error) — `ApiFreeze`'s snapshot mechanism remains, now unconditional pub-fn
semver diffing (E1218/E2601), not a capability-tier freeze. **S3 shipped
(2026-07-04)**: `-> &T` returns and `&T` fields are gone from the grammar
(ordinary syntax errors); `#Ref`/E0207/E0427 deleted outright. **S4 shipped
(2026-07-04)**: `copy x` (D-CAP2) is the one copy verb — a real prefix-verb
expression (`Expr::Copy`), parses on any expression, formatter round-trips
it. `.clone()` is not user-typable Jet syntax — `clone` falls through to the
ordinary "no such method" path (I8). `copy x` on a non-cloneable type is
E0211; on a scalar it's legal but redundant (already `Copy`). Every fix-it
that used to suggest `.clone()` now suggests `copy name`. **S5 shipped
(2026-07-04)**: `[T]` slice views were already live (`View<T>`, D-DYNARRAY1,
predates this migration — nothing to build). `String.trim()`/`.after(sep)`/
`.before(sep)` bound to a local now return a zero-copy `&str` view instead of
an owned `String` when sema proves it can't outlive its owner (no distinct
Jet-level view type — `String` stays one type; view-ness lives on the binding,
codegen-invisible to the user); escape (return/rebind/field/call-arg/any other
method) is **E2307**. `split` stays eager (`Vec<String>`) — a view-of-views
list needs S6-scale representation work, named as a deferred gap, not built
here. **S6 shipped (2026-07-04)**: `Shared<T>` (D-SHARED-API1=A) is a
lock-guarded shared handle — `Shared.new(x)` constructs (bare type-name call,
`T` inferred from `x`); `.read(f)`/`.edit(f)` run a closure against a read- or
write-locked view, the lock scoped to the call only; cloning is always a
cheap handle clone (never a deep copy), so it crosses a `tasks.spawn` boundary
with no `take`. (`Shared<T>`'s *type* predates this stage as inert signature
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
code. **S7 shipped (2026-07-04, D-NOALLOC-SEM1=A)**: `policy no_alloc` is a
bare module-level item (parses like `use`, no manifest/`pkg.jet` field — it's
ordinary module-file syntax, nothing else reads it). The check is **local
only**: it walks only the policy'd module's own function bodies and never
follows a call into another function (unlike `@Pure fn`'s whole-program
call-graph fixpoint) — a denylist of four allocation-shaped expressions,
each **E0921**: string interpolation with a `{…}` hole (a hole-less literal
isn't flagged); any `.push`/`.insert` call (method-name match only, no
receiver-type check — capacity headroom isn't provable statically); a
struct/enum literal for a type that owns heap data directly or transitively
(`String`/`[T]`/`[K,V]`/`Shared<T>`/`Pool<T>`, walked through struct fields
and enum variant payloads — `Id<T>` is plain `Copy` data, never flagged);
`copy` of a heap-owning type. A bare list/map literal outside those four
shapes (`xs := [1, 2, 3]`) is NOT checked — a deliberate, ratified-text-exact
scope cut, not silently expanded. **S8 shipped (2026-07-04)**: docs sweep —
diagnostics.md retired-code stubs for every deleted S1-S7 mechanism,
spec.md's memory chapter rewritten to v5 end to end, this file's
D-CAP7/D-CAP8/D-CAP4-5-6/D-REF-SHORTHAND1/2 supersession notes, stale
`~`/`.clone()`/`api:` sweep across docs/reference. S9 (final verification
gate) remains.

**D-MUTSELF1 — Receiver mutation**: a `&self` method mutates in place —
`self.field = v`, compound ops, and whole-`self` reassignment all lower
through the deref'd receiver; the same write in a read method is E0205 with a
"write the receiver as `&self`" fix at the assignment.

**S63 — Resource cleanup**: automatic scope-end cleanup (RAII) is the single
story — backed by Rust `Drop`, every exit path. No `defer` keyword
(D-DEFERKW1/D-SUGAR5); `core.scope.guard` is the scope-exit hook.

**S53 — Concurrency** *(deferred core; combinators live)*: tasks/channels
deferred past v1.0 (planned: `tasks.spawn(closure) -> Task<T>`, `t.join()`,
`tasks.channel<T>()`; no shared mutable state). Already ratified around it:
**D-CONCCOMB1** structured combinators `g.all` / `g.race` / `g.any`
(race cancels losers; all fails fast; any takes first Ok, D-RACEWIN1);
**D-DETACH1** `task.detach()` consumes the handle, detached capture of a
borrowed view is a compile error; **D-ASYNCRT1** M:N green threads, no
`async`/`await` coloring (gated on scheduler work). **D-TUPLE-DESTRUCT1**
*(ratified/implemented 2026-07-04)*: `tasks.channel<T>()` returns
`(Sender<T>, Receiver<T>)` directly — no combined "Channel" handle, no
`.sender()` method. Destructure with the existing S74 tuple form:
`(tx, rx) := tasks.channel<T>()`; a second sender is `copy tx`. A
`Receiver<T>` is what `g.select().recv(rx)` takes.

### Effects & safety

**D-EFF1 — Effect system**: inferred per-fn effect sets (Koka-style rows),
erased in codegen. Assert/restrict via `#(Net, Db)` on a signature and
`#Caps(Net) { … }` regions.

**S60 — Purity marking**: `@Pure fn` is a checked signature modifier — the
empty effect set; violations name the impure call path. Also valid as a
function-type bound (D-MARKERMOVE2).

**D-EFF4 / D-EFF5 — Vocabulary**: closed set of ten tree ROOTS — `Net`, `Fs`,
`Io`, `Db`, `Time`, `Rand`, `Env`, `Exec`, `Log`, `Gpu`; unknown root E0119.
Amended by D-EFFTREE1: a root may be dotted into an open leaf path (`Fs.Read`)
and ancestor matching is subsumption. `effect <Name>` user declarations
reserved, unminted.

**D-EFFTREE1 — Effect tree** *(ratified 2026-07-03, card #181)*: the ten
D-EFF4/5 names are tree roots; a signature/`#Caps`/`#Grant`/`#(!…)` entry may
be a dotted path rooted at one (`Fs.Read`, `Net.Http.Get`) — root closed
(E0119), leaf open/user-chosen, no fixed vocabulary or depth limit. Ancestor
matching is subsumption, the same rule as D-TAG1's tag-tree subtree matching
learned once and reused: `#(Fs)` accepts any `Fs.*` callee; `#(Fs.Read)`
rejects a sibling `Fs.Write` callee; `#Grant(Fs.Read)` doesn't authorize
`Fs.Write`; `#(!Fs)` prohibits the whole `Fs.*` subtree. Reverses E0740 for
the ancestor case, keeps it for out-of-tree/sibling cases. Flat root names
stay valid (no migration break) — Core stdlib calls are still tagged with a
bare root; leaf precision is a user-declared-contract concept.

**D-EFF2 — Polymorphism**: transparent flow-through by default; escaping
function values assume the maximal set. Expert levers: effect-bound function
types (`@Pure fn(T) -> U`, `#(Net) fn(T) -> U`; call-site check E0747) and
`#(via f)` pass-through publication (E0748).

**D-EFF3 — Traits**: a trait method may declare an effect upper bound — both
the impl obligation (E0742) and the dispatch contract for trait objects.

**D-PROP1 / D-PROP2 — Prohibition**: `#(!Net)` — the fn and every reachable
callee must not use the effect (E0749).

**D-SCAP1 — Scoped capabilities**: `#Grant(Fs) { caps -> … }` authorizes
effects into a lexical scope, binding an erased first-class handle; effect
without backing grant E0712; handle escape E0711.

**D-TAINT1 — Taint** *(D-TAINT-SAN, D-IFC1)*: `#Tainted expr` marks untrusted
values (closed kinds `.Input`/`.PII`/`.Secret`/`.Credential`; bare = `.Input`);
taint spreads by dataflow; `#Sanitizer fn` strips by contract (bare
`sanitizer` E0059); tainted value reaching a `Db`/`Exec`/`Net` sink is E0721.
Full IFC deferred post-Epoch 3.

**D-DET1 — Determinism** *(D-DET-CAPAPI)*: `#Pure` implies reproducible —
wall-clock/OS-rng/fs/net rejected (E3401/E3403); injectable `Clock`
(`now/tick/advance/wait`) and `Rng` (`int/float/bool/pick/shuffle`) are the
pure-callable capabilities; `assume_deterministic { }` expert escape.

**D-REPLAY1**: `#Replayable` rejects any reachable `Time`/`Rand`/`Net`/`Io`
not routed through a deterministic/mockable capability. Implemented by the
effect fixpoint as E0725; deterministic `Clock`/`Rng` handles remain pure.

**D-TXN1–4, D-TXN-ROLLBACK — Transactions**: `#Transact(name) { … }` — on a
`?`-failure, mutated locals restore LIFO from auto-snapshots (layer 1);
`Rollback` trait for custom snapshots (layer 2); `name.on_rollback(() => …)`
and `name.on_commit(() => …)` explicit hooks (layer 3, Drop-backed).
Irreversible effects (`Net`/`Fs`/`Exec`) inside the block are E0746 — move
after the block or register via `on_commit`.

**D-LIN1 — Single-use values** *(D-LIN1-DROP)*: `#SingleUse` (implies
`#NoCopy`) must be consumed exactly once on every path — `^` param, return,
or `drop(x)` inside `#Unsafe("reason")` (else E0143). Unconsumed E0140;
one-branch-only E0141; lending instead E0142.

**D-PREPOST1 — Contracts**: `@Pre(cond, "msg")` / `@Post(cond, "msg")` on a
signature (`result` in Post); conditions pure; checked in every build;
per-module build-policy strip is an explicit opt-out. Violation quotes the
clause at the call site.

**D-METHODMACRO1=A — Checked inline contracts**: `@Inline`/`@InlineAlways` on
a `fn` or `fn Type.method`; methods stay ordinary functions, no macro-rewrite
hooks. `@Inline` is a soft hint (`#[inline]`; never rejected). `@InlineAlways`
is a checked promise (`#[inline(always)]`): sema proves the call can actually
inline or fails the build naming why — self-recursive (E0917), address-taken
(E0918), or over the statement ceiling (E0919). Both markers on one
declaration is E0920 (pick one). PascalCase per D-CONTRACTCASE1.

**D-PERSIST1**: `@Persist` module binding survives `jet dev` hot reload;
identity = module path + name; layout change re-decodes Codable-style, falls
back to reinit + warning. Dev-tier only. *(rides JIT hot-reload runtime)*

**D-EFFBUDGET1 — Package effect budget**: every build prints a one-line
effect summary and records per-dependency provenance in the lock.
`effects: { allow: […], deny: […] }` in `pkg.jet` enforces the whole graph
(E1220 names the dependency); `grants: { "dep": [Effect] }` per-dep escape;
malformed block E1221.

**D-STREAMYIELD1 — Generators**: `fn f() -> Stream<T>` uses `yield expr` to
hand a value to the consumer and suspend until the next pull; falling off
the end (or a bare `return;`) ends the stream; `return value;` is E0806.
Consumers are ordinary `loop x in f() { }` loops — one keyword, one type, no
async/await coloring. Implemented on a real OS thread + a rendezvous
channel (`std::sync::mpsc::sync_channel(0)`): `yield` blocks the producer
thread until the consumer's loop pulls, exactly reproducing suspend/resume
with zero coroutine machinery.

### Comptime & metaprogramming

**S26 — Comptime, value-level**: layered, value-only. **One law: comptime
never creates, parameterizes, or selects a type, and never affects dispatch**
— polymorphism is traits-only. Pure Jet is comptime-callable with no
annotation; evaluation is a sema tree-walking interpreter, type-checked
first, fuel-limited; comptime `panic` is a user-authored compile error;
results lower to constant data. Permanent differential CI: interpreter and
compiled runtime must agree bit-for-bit. **Rejected forever**: token/AST/
attribute macros, custom syntax, comptime types, const generics in v1.

**S57 — Comptime bindings**: `comptime x = f()` — `comptime` is the binding
keyword, always immutable. **D-CTBLOCKEXPOSE1**: `comptime { … }` execution
block (Jai `#run` analog); bindings inside leak to the enclosing scope as
`$name` (E2713). **D-CTMARKER1**: `$name` is reserved **only** for comptime
splices into generated code.

**D-CTCORE1 / D-CTIO1 / D-PURE2 — Comptime I/O**: only a curated whitelist of
pure Core functions evaluates at comptime; the sole I/O escapes are
`embed_file(path) -> String` and `embed_bytes(path) -> [U8]` (string-literal
path, no escaping the project root). **D-STRPARSE1**: comptime evaluation may
pass through `Result`/`Option` for pure parse paths.

**D-CTEFFECT1 — Comptime effect tiers**: Tier 0 pure always-on; Tier 1
hashed-reproducible recorded into `.jet/lock` (`@embed`, `find`,
`fetch(url, sha256:)`); Tier 2 ambient requires `#Impure("reason")` **and**
`--allow-impure`. **D-CTFIND1/2**: `find(glob) -> [String]` builtin, sorted,
hash-recorded; hand-rolled std-only glob (`*`, `**`, `?`, `{a,b}`, `[a-z]`).
Shipped by #350.

**D-METADEPTH1/2 — Metaprogramming ceiling**: read-only reflection + derives.
Rung B granted: whole-program read-only reflection + structured diagnostics
from the build entry (post-sema snapshot; diagnostics must carry
code+what/why/fix). No mutation, no AST injection, no macros
(D-PROCMACRO1/D-READERMACRO1 deferred/rejected; libraries can never mutate
grammar).

**D-METAREFLECT1 — Reflection API**: `T.reflect()` returns a `Type` handle —
`.name`, `.fields`; each field carries `.name`/`.ty`/`.markers`/
`.has_marker("…")`.

**D-METADERIVE1 — User derives**: `derive T.Wire { … }` (old
`derive Wire for T` is E2714); body uses `T.reflect()` + `$name` splices,
emits Jet source text that re-enters lexer→parser→sema like hand-written code
(**D-CTCODEGEN1** — never inject pre-parsed AST). Local-only orphan rule.
Routed through the existing marker system (D-USERDERIVE1).

**S56** stays open for Epoch 3 (typed-reflection hardening; see Open
decisions).

### Low-level tier

**S58 — Two gates, one keyword**: `use core.mem` is the discovery gate
(allocators, `*T`, layout/repr, volatile read/write). `#Unsafe("reason") { … }` /
`#Unsafe("reason") fn` is the audit gate (**D-UNSAFE2** — the reason is the
gate's argument; **D-UNSAFE-REASON1=B** — bare `#Unsafe { … }` / `#Unsafe fn`
compile and emit L3101; whole-fn form requires an enclosing `#Unsafe` at call
sites). Gated ops: deref `p.*`, raw-pointer-of `*x`,
volatile `mem.volatile_read(p)` / `mem.volatile_write(p, value)`, pointer math,
transmute-class casts, FFI pointer crossings (outside the gate: E0208).
Address-of is `mem.address_of(x)`. `mem.cast_ptr<T>(p)` is the cast primitive
(D-CASTPTR1); no compact pointer-chain syntax (D-POINTERCHAIN1).
Generated `unsafe` appears only inside user-gated regions + vetted internals
(I1). Onboarding never mentions any of it.

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
2026-07-02)**: replaces D-UNINIT1's marker spelling. `buffer: [U8#4096] :=
uninit` — `uninit` is legal only as the RHS of `:=` on a binding with an
explicit type annotation; the flow-analysis engine (E0420/E0423/E0424) is
unchanged, only the trigger moved. The old `#Uninit buffer: [U8#4096]`
spelling is retired: a hard parse error (E0426) teaches the new form.

**D-REF-SHORTHAND1 — Stored-ref field shorthand (retired 2026-07-04 by
D-MEM1/S3)**: originally, a stored-reference field spelled its type `&T` —
the borrow sigil already used at call sites — instead of a bare `T` plus a
separate marker; the owner was *inferred* at each construction site (one
candidate → it, two or more → **E0207**), disambiguated with `#Ref(label)`
when needed; the retired `#Ref(owner) name: T` form (plain type, no `&`) was
a hard teaching error (**E0427**). D-MEM1/S3 deleted the whole mechanism
outright — `&T` struct fields are not in the v5 grammar (ordinary syntax
error), and E0207/E0427 are gone (retired stub rows in
docs/spec/diagnostics.md). The "how do I store a reference?" answer is now
an owned field, `Shared<T>`, or `Pool<T>`/`Id<T>` (see D-MEM1) — forward
guidance only, this mechanism is not coming back.

**D-REF-SHORTHAND2 — `#Ref(label)` disambiguator (retired 2026-07-04 by
D-MEM1/S3)**: originally the owner label stayed on the `#` directive plane,
spelled `#Ref(label)` — *not* `@Ref`, resolving the sigil clash with
D-MARKERMOVE1. Deleted along with D-REF-SHORTHAND1's `&T` fields; a
`#Ref(label)` naming no candidate owner used to be **E2306** — also gone
(retired stub row in docs/spec/diagnostics.md). `jet expand --facts refs`
(the lens that reported these owners) is gone with it.

**D-REGION1 / D-ALLOC1 / D-ALLOC2 — Arenas & regions**: regions are implicit
and scope-inferred by default (the region is the arena binding's lexical
scope); explicit `region r { … }` for the expert tier. `arena ::
mem.Arena.new(capacity: 4096)`; `arena.alloc(value)` returns a scope-bound
view — escape E0631, use-after-`reset`/`free` E0632. Arenas live flat in
`core.mem` (D-REF2); arena values are not `#Unsafe`.

**D-SOA1 / D-SOA2A–D — Columnar layout**: `#Layout(columnar)` on a struct;
a `[S]` of it lowers to a struct-of-arrays with a logical-Vec API
(index-read gathers, field-read hits the column). Whole-struct only (partial
E1109); `columnar [T]` type-position reserved (E1107); deferred surface ops
E1108; serialization-transparent. **D-REPRC1**: `#Layout(c)` = C repr in the
same family (growable field under it = compile error).

**D-SIMD1 / D-SIMD2 — SIMD**: portable lane types `F32x4`/`F64x2` —
`F32x4(…)`, `.splat(x)`, `v[i]`, element-wise ops, `v.sum()` /
`v.reduce(#Add)`; `[F32#4]` bridges via `from_array`/`to_array`
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
compatibility checks, and Jet-owned diagnostics E1257-E1260. **D-NOSTD1**: no
`no_std` flag — the std baseline follows the typed platform `target:`
(bare-metal ⇒ no-std).
**D-OOBPROOF1**: bounds-check elision is proof-carrying: a fixed-list index
whose distinct-`Int` invariant fits `0..N-1` lowers without the runtime bounds
helper; other dynamic indexes keep the check.

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

**D-COV1**: `jet test --coverage` — per-function HIT/MISS table; probes only
in this mode, normal codegen byte-identical. **D-TOOL4**: snapshot testing
with `-u`/`--update-snapshots`. **D-A11YGATE1**: accessibility issues are
`jet lint --a11y` lints (E2930/E2931), opt-in CI gate.

**D-TESTKIT1=A** *(ratified 2026-07-07, card #308)*: `#Test` remains the only
test syntax. `core.testing` adds snapshots, fixtures, corpora, temp dirs, fake
clocks/random, HTTP servers, golden files, and benchmark budgets as library
helpers. Helpers emit structured test metadata so reports and CI can render
categories without adding markers for every feature. Epoch 3 ships `snap`,
`golden`, `fixture`, `temp_dir`, `corpus`, `fake_clock`, `fake_rng`, and
`bench_budget`; the existing `expect(...).snapshot()` remains the canonical
assertion snapshot path.

### Formatting & comments

**S5 — Comments**: `//` line; `/* … */` **nesting** block comments
(unbalanced E0002).

**S49 — Doc comments**: `///` summary lines above items, plain text v1;
doctest-runnable (D-TEST4); no `/** … */`.

**S6 — No visible semicolons** *(S6-R)*: statements end at line end; the
lexer inserts Go-style terminators after any line whose last token can end a
statement. Layout rules: `-> Type` and `{` stay on the closing-`)` line; a
terminator is suppressed when the next non-blank line starts with `.` (chain)
or a binary/logical operator. Counted-loop headers keep their internal `;`
(D-LOOP-SEMICOLON1). New terminal tokens must be added to `ends_statement`.

**S44 — Formatter** *(D-FMT1, D-FMTPARENS1, D-NARG2)*: one style, zero
config — 4-space indent, same-line `{`, width 100, spaced binary operators.
Author intent preserved: a single-simple-statement one-line body stays
one-line (fits, no inner comment); author grouping parens preserved even when
redundant; call-site labels never added/stripped; dot-chain breaks preserved
(**S69** — break before `.`, optional trailing comment per step). Idempotent,
not AST-canonical. **Every new syntax needs formatter emission + a fmt
STABILITY test** (idempotence alone misses dropped tokens).

### FFI & external dependencies

**S50 — Rust FFI**: `extern rust "crate@version" { fn name(args) -> T =
"rust::path" }`. Version pins required; by-value boundary only — no borrows,
callbacks, or trait objects across the edge.

**S59 — C FFI** *(D-CFFI2, D-CFFI-CANON1, D-CBIND2/3/5/6)*: auto-generated
bindings + optional user overlay; by-value first, pointers only inside S58.

| Layer | Shape |
|---|---|
| Autogen | `#Bindgen module c.<lib>.__bindgen__ { … }` in `.jet/bindings/c/<lib>.jet` |
| Overlay | `#Extern module c.<lib> { … }` — merged bindgen ∪ overlay, overlay wins |
| Script | `use "raylib.h" as rl` — compile-time bind on cache miss |
| Project | `use c.raylib as rl` — one form per lib per file |

Link resolution: declared `<lib>: c@system` / `c@"vendor/path"` in `pkg.jet`
`deps:` → pkg-config fallback → E3201. C deps are link deps, never packages.
`jet bind` uses a native std-only C-prototype parser (`Source/CBind.rs`);
binds scalars and `char*`↔String; `#define` constants only. Old
`@extern`/`#extern` spellings E0060. `#Bindgen`/`#Extern` PascalCase.

**D-FFI-UNIFY1 — FFI structure law**: every foreign language mounts as a
namespace `<lang>.<lib>` with the same three tiers (S59 generalized): script
tier (`use "xxhash.h" as xx` — bind on first compile), project tier
(`use py.h5instrument as h5`, dep pinned in `pkg.jet` as
`<lib>: <lang>@"ref"`), overlay tier (`#Extern module <lang>.<lib> { … }`,
overlay wins). `jet bind <lang>` is a per-language binder emitting
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
worker (typed message boundary, crash-isolated, `#(Py)` effect added to the
D-EFF4 set); opt-in `py@embed` switches to in-process libpython for
zero-copy buffer-protocol arrays. One `use py.X` surface; the tier never
moves call sites. **D-FFI-JS1 (=A)**: one `use js.X` surface, host chosen by
compile target — browser JS engine on the web target, QuickJS/componentize-js
WASM component on wasmtime for native targets. `jet bind js` generates
committable typed stubs from a package's `.d.ts` — this AMENDS D-NPMTYPE1's
hand-authored-only floor; no-`.d.ts` packages get a `#Unsafe`-gated dynamic
surface; Node-subprocess broker is an opt-in tier. **D-FFI-SWIFT1 (=A)**:
swift-bridge-style generated projection over the fixed C-ABI transport
(D-JSWIFTFFI1) — `jet bind swift` runs swiftc to emit `@_cdecl` shims +
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
client TLS config), zip/tar/flate2 (`core.archive`, D-DEP-ARCHIVE1),
rusqlite-bundled (`core.db`, D-DEP-DB1), ureq/hyper/tungstenite
(`core.http`, D-NETDEP1/D-HTTPLIB3), Cranelift (`jet-jit`, D-JITDEP1),
wasmtime (plugins, D-DEP-WASM1), age-style crypto bridge
(D-JPK-SECRETCRYPTO1). `jet repl` stays std-only (D-REPL18). Raylib ships as
first-party `core.raylib` bridge package (D-RAYLIB1); `core.game` is the
scene-first game engine layered above it (D-GAME1=B, D-GAME2=A, D-GAME3=C).
npm interop = typed first-party stub packages, no `.d.ts` parsing
(D-NPMTYPE1); Swift interop waits on native-UI/C-ABI work (D-JSWIFTFFI1).

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
`frame.input.pressed`), `game.Replay.record(".jreplay")`, an explicit
`game.Backend.headless()` default, and scene budgets via `scene.budgets.set`.
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
and `with_policy<T>(policy_async(n))` gives an explicit queued/backpressure
entrypoint. Hooks combine by "last active handler result wins" in this first
slice, with the call-site fallback used when no handler is active.

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
D-ENC-YAML1)*: one format-agnostic data model. `@Codable` (≡
`@[Encode, Decode]`), `@Encode`, `@Decode` derives — a built-in compiler
field-walk, not S56 reflection. Formats are adapters in **`core.encoding`**
(`core.encoding.{json,csv,toml,yaml}`); encode verbs `to_string` /
`to_string_pretty`; typed decode `decode<T>` (target inferable from the
binding type; bare `decode(s)` yields dynamic `Data`). Hand-impl surface:
`encode`/`decode` verbs over `DataTree`
(`.Null/.Bool/.Int/.Float/.Text/.Bytes/.Array/.Object`); `DecodeError
{ path, reason }`; encode infallible. Field markers (`#` plane):
`#[Rename("x")]`, `#[Skip]`, `#[Default]`/`#[Default(expr)]`, `#[Flatten]`,
`#[RenameAll(camel|snake|pascal|kebab|screaming)]` (E2409). Enum wire:
externally tagged default, single-value variants bare; `#[Tag("type")]`
internal, `#[Untagged]`. Unknown wire keys ignored by default;
`#[DenyUnknownFields]` errors (E2412). Generic `@Codable` auto-adds
`Encode`/`Decode` bounds to wire-reaching type params only. Dynamic trees get
`?`-chaining accessors (`.field(name)`, `.at(i)`, `.int()`, `.text()`, …).
YAML parser is std-only, YAML 1.2 core incl. anchors.

**CLI & IO**: builder-spec arg parsing `args.spec().flag(…).option(…)
.positional(…)` with generated `--help` (D-ARGS1). `io.stdin()` handle with
`.lines()`/`.read_line()` (D-STDIN1). Scoped `live { … }` raw-terminal block
with guaranteed restore (D-TERM1). `core.log` auto-detects TTY (text) vs
piped (JSON); `log.setup(format:)` overrides (D-LOGFMT1).

**Core library audit ratifications** *(ratified 2026-07-07, cards #289-#308,
#310)*: the Epoch 3 Core expansion follows these owner picks.

- **D-COREIO1=A**: `core.io` owns stdout/stderr/stdin streams, flush, raw
  bytes, TTY facts, and terminal capabilities. Style/progress/raw mode/key
  events live under `io.terminal` or stream methods, honor TTY/NO_COLOR by
  default, and expose explicit force/raw controls for experts.
  Implemented Epoch 3 stream surface: `io.stdout()` / `io.stderr()` handles with
  `.write`, `.write_line`, `.write_bytes`, `.flush`, `.is_tty`,
  `io.terminal_width/height`, `io.style`, `io.style_force`, and `io.progress`;
  D-TERM1's `core.term` remains the direct raw-key bridge.
- **D-COREARGS1=A**: `ArgsSpec` is the one CLI parsing model. Typed
  `fn run(args: T)` derives an `ArgsSpec`; library/tooling code may build the
  same spec dynamically for subcommands, env fallbacks, completions, and tests.
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
- **D-ENCSTREAM1=A**: each `core.encoding` codec has one adapter identity with
  whole-value and reader/writer stream modes over the shared `Data`/`DataTree`
  and `Codable` machinery. XML, JSONL, canonical JSON, and CBOR follow this
  model. Epoch 3 ships JSON canonical output/events, JSON Lines, namespace-
  preserving XML tree parse/render, CBOR, base32, and URL-safe base64 beside
  existing JSON/CSV/TOML/YAML/hex/base64.
- **D-TEXTUNICODE1=A**: `core.text` owns Unicode algorithms: normalization,
  case folding, segmentation, width, classification, and UTF-8/scalar helpers.
  `String` stays small; tooling may insert `core.text` calls from String
  contexts. Epoch 3 ships `core.text` helpers for NFC/NFD/NFKC/NFKD, casefold
  and caseless compare, grapheme/word/sentence slices, terminal display width,
  Unicode classification, scalar/byte counts, split/trim/pad/center,
  prefix/suffix combinators, and char-index views. Locale collation is an i18n
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
  socket model. String entrypoints remain the beginner path; `IpAddr` and
  `SocketAddr` are the expert/control path over the same semantics.
- **D-HTTPDEPTH1=A**: `core.http` owns Client, Server, Router, middleware,
  streaming bodies, forms/multipart, cookies, redirects, timeouts, TLS policy,
  and SSE, built on `core.url`, `core.mime`, and `core.net`. WebSocket support
  has its own top-level home, `core.ws`, per D-WS1=B.
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
- **D-ITERTOOLS1=A**: one lazy `Iterable`/`Iterator` model powers collection
  adapters. Collections expose beginner-friendly methods returning lazy views;
  materialization is explicit via `collect`, `to_list`, or reducers.
- **D-TASKRUNTIME1=A**: task groups remain the structured lifetime boundary.
  Channels, timers, deadlines, cancellation, and select produce typed event
  values; scheduler budgets, tracing, and deterministic tests are expert hooks.
  Implemented Epoch 3 surface: `tasks.channel<T>()`, bounded
  `tasks.channel<T>(capacity: N)`, `tasks.after(ms: N)`,
  `tasks.after(ms: N, value: fallback)`, `tasks.interval(ms: N)`, and
  `g.select().recv(rx).after(ms: N, value: fallback).wait()` over one return
  type.
- **D-DATAFRAME1=A**: `core.data` exposes typed `Table`/`Series<T>`, schema,
  typed rows, lazy query plans, joins, windows, missing values, and plotting.
  Eager helpers and lazy plans share the same operations. Current shipped floor:
  typed CSV rows, `Table<T>`/`Series<T>` wrappers, `LazyFrame<T>` plans with
  typed lazy filter/sort/collect and plan audit output, optional-series missing
  counts, typed-lambda eager `filter`/`sort_by`, group stats, inner/left key-join
  summaries, pivot sums, rolling means, distribution summaries, and deterministic
  text/SVG plots.
- **D-STDLIBLEDGER1=C**: Core docs track built modules only. Missing domains
  are implicit; Jet does not maintain a have/have-not ledger of unbuilt or
  declined stdlib domains.

**Filesystem & time**: typed `Path` (`from`/`join`/`parent`/`extension`/
`stem`), `write_atomic()`, lazy cycle-safe `walk()` (D-PATHFS1 shipped).
`core.files` now ships D-FSOPS1 depth: `Stat`, `WalkEntry`, `TempDir`,
`TempFile`, `FileLock`, recursive `walk`/`glob`, metadata, create/remove tree,
copy tree, symlink/readlink/hardlink, temp handles, advisory lock files,
canonical/absolute, offset bytes, fsync, and atomic writes (#288). Watch APIs
follow the owner's D-WATCH-SCOPE1 comment: `core.watcher` owns file, process,
and port watch handles with `WatchEvent` values plus callback methods through
`core.event`. `fs.list_dir -> [DirEntry]` (D-LSDIR1). Civil time uses
Instant/DateTime/LocalDate/LocalTime/Duration/Period/Zone/ZonedDateTime over
IANA TZif zoneinfo, layered on the injectable `Clock` (D-TIMEDEPTH1 +
D-TIME-CALENDAR1; #295). PRNG
`core.random` (SplitMix64, seedable) vs CSPRNG `core.crypto.random`
(D-RANDSPLIT1); both carry `Rand`.

**Crypto**: misuse-resistant `seal`/`open` + `sign`/`verify` defaults; raw
primitives require `core.crypto.expert` behind `#Unsafe` (D-CRYPTOENV1,
E0510/E0511). Versioned `JETC` envelope header gives algorithm agility;
PQ algorithms later (D-PQCRYPTO1). `core.encoding` hex/base64 + `core.uuid`
v4/v7 (D-UUIDENC1).

**Numerics & data**: `core.linalg` ring package — `Vec2/3/4`, `Mat3/Mat4`,
`.dot()`/`.cross()`/`.matmul()` as aliases over a generic `Vec<N>`/
`Matrix<M,N>` substrate (const-generic substrate tracked by #293) (D-MATHLIB1,
D-LINALG1). `core.db`: backend-neutral `Driver` trait, parameterized-only
API, SQLite first; explicit `.begin/.commit/.rollback` distinct from
`#Transact` (D-DBDRIVER1). D-DBMIGRATE1 ships the hybrid database floor:
checked `Sql` literals feed `db.params(sql)`, rows stay inspectable maps with
typed `db.row_*` reads, and `db.transaction`/`db.migrate` provide rollback and
checksum-recorded migration helpers over the same parameterized path. `core.http`: client+server submodules; client
supports HTTPS by default via rustls + system roots (D-TLS1=A); server is
plain `fn(req: Request) -> Response` on a `mux` (`mux.get("/path", handler)`,
`req.params["id"]`, `Server.serve(addr, mux)?`) with HTTPS enabled by the named
option `Server.serve(addr, mux, tls: Server.tls(cert, key))` (D-TLSSERVE1=A).
HTTP/1.1 depth is tracked by #301; HTTP/2 remains a separate transport upgrade,
and WebSocket belongs to `core.ws` per D-WS1=B (D-HTTPLIB1–3, D-ROUTE1).
Compression
`core.compress.{gzip,zstd}` (D-CODECS1). Measurement-with-uncertainty in
`core.science.measurement` (D-HONESTNUM1 shipped/partial; #310 verifies module
map truth). Opt-in `Gc<T>` module
for cyclic data (D-OPTGC1, gated on its I6 ballot). Approximate/sketch algos
are libraries (D-APPROX1); parallelism stays explicit `par_*`
(D-AUTOPAR1); adaptive fidelity is a manual runtime-global knob:
`core.perf.Perf.fidelity()`, `default_fidelity()`, `override_fidelity(v)?`,
and `reset_fidelity()` (D-FIDELITY-API1=A). No automatic adaptive scheduler or
platform-signal providers ship in Epoch 3 (D-ADAPTRT1=C,
D-ADAPT-PROVIDER1=A).

**Reactive, events & UI stack** *(D-REACT1, D-REACTCORE1, D-SIGNAL1, D-EVENT1,
D-RENDERTGT1/2, D-UITREE1, D-STYLESHAPE1, D-MOTIONTIME1, D-LAYOUT1,
D-OWNCOMP1, D-A11Y1, D-NATIVEUI1/2)*: reactivity is a library + explicit
`#Reactive` scope marker (E2914) lowering onto `core.reactive` — `Signal<T>`
(`.get()/.set(v)`), `Computed<T>`, `Effect`; explicit-by-read subscription;
pure std runtime (E2910–E2913). Events and hooks are compiler-known Core values
in `core.event`: `Event<T>`, `Hook<T, R>`, `Subscription`, `EventScope`,
`EventPolicy`, and `EventTrace`. Render backends implement measure/layout/paint
(`JetBackend`; `NullBackend`/`TuiBackend` shipped). UI trees are typed dot-construction
(`.Button.{ label: "OK" }`); `Style` is one flat record; motion uses the
injectable `Clock`; constraint layout is a `layout { }` block over
`Constraint` handles with a first-party simplex solver (E2932–E2934).
Components distribute copy-in-and-own: `jetpack add <Component>` copies
source into `./components/` (no version lock). Native UI wraps platform
widget FFI, all three desktop platforms against one trait seam *(gated)*.

**Web target** *(D-WEBKIND1, D-DOMGEN1, D-WEBBACKEND1, D-OSTARGET1,
D-WEBDEFAULT1, D-HTMLPAIR1)*: browser target is `wasm32-unknown-unknown` +
generated JS loader; DOM work goes through a tiny first-party `JetDom` shim
(no vdom); hybrid: view emits JS, compute may compile to WASM. `#Target(…)`
takes `Web`/`Browser`/`Wasm`/`Js` and `Os.Linux`/`Os.Macos`/`Os.Windows`
(mixing web+OS on one item rejected). Default target: CLI `--target` >
`pkg.jet` `target:` > file marker. `#Html("path.html")` names a companion
page (explicit > sibling `<stem>.html` > generated; missing path = build
error). `Os.*` gates a single `impl` block (item-scoped), not a file/module —
`E-OSTARGET-MIXED-AXIS`/`E-OSTARGET-UNMATCHED-CALL` enforce it.
**D-OSTARGET2 (=B, ratified 2026-07-03, c2qj06uq)**: ungated code reaches
the surviving OS-gated impl through a comptime dispatch on `build.os` — a
compiler-known comptime value matched with `.Linux`/`.Macos`/`.Windows`
arms; non-matching arms are discarded before OS-gating checks run.
fn-level `#Target(Os.*)` gating (option A) rejected.
*Shipped spelling (2026-07-03):* the ballot wrote the dispatch loosely as `match
build.os { … }`; reconciled to Jet's one canonical branching form (D-IF1/D-IF3
`if subject == { }` if-table) with the existing `comptime if` lead (D-WHEN1) —
**no `match` keyword was added** (I8). Statement-position dispatch:

```jet
fn run() {
    comptime if build.os == {
        .Linux   -> { b :: LinuxBackend.{ name: "gtk" }    print(b.label()) }
        .Macos   -> { b :: MacosBackend.{ name: "appkit" } print(b.label()) }
        .Windows -> { b :: WinBackend.{ name: "win32" }    print(b.label()) }
    }
}
```

`build.os` resolves to `ProgramBundle.active_os` (the `--target=<triple>` OS
bucket, host OS when omitted; a web/wasm target falls back to the host per
`OsTarget::active`). Sema desugars the dispatch into a `comptime if` chain
(D-WHEN1/D-WHEN2 machinery) as the *first* step of `check_bundle`, folding to
the arm matching `active_os` and discarding the rest before any OS-gating
check, type-check, or codegen sees a body — so constructing an OS-gated type
inside the taken arm is legal and dead arms never trip
`E-OSTARGET-UNMATCHED-CALL`. **Exhaustiveness** is build-independent: the arm
set must cover `.Linux`, `.Macos`, and `.Windows`, or carry an `else` — missing
an OS with no `else` is `E-OSTARGET-DISPATCH-EXHAUSTIVE` (so the same source
compiles or fails identically on every platform). A non-`build.os` subject is
`E-OSTARGET-BUILD-CONTEXT`; a non-OS arm head is `E-OSTARGET-DISPATCH-ARM`.
`build.os` is meaningful *only* as this dispatch's subject — `build` is not a
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
in place (the shipped reactive core, I8). Selected by the shipped `comptime if
build.os` dispatch (D-OSTARGET2=B) and emitted only on a Linux target; all gtk
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
unit-family literals — `core.ui.style` declares `#UnitFamily(length) { px }`
(D-QUAL3), so `width: 320px` is a compile-checked `Px` value via the one
ratified unit mechanism (D-UNITLIT1); no second style-only unit system (I8).
Supersedes Phase 3's interim `Length` struct pair. *Shipped* (Tower c134):
`examples/features/ui/ui_typed_style.jet` declares `#UnitFamily(length) { px }`
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

**S52 — Files** *(D-JPK-FILES, D-JPK-FILENAME2)*: per-package manifest
is **`pkg.jet`** (`payload: { name, version }` identity + `packages:` +
`deps:` + `targets:` + `effects:`); dev shell is **`env.jet`**; monorepo
index is **`module workspace` in `workspace.jet`** (`members:` may run
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

**U10 — payload → packages → modules**: a payload (one `pkg.jet`) lists its
packages in `packages: { name: … }`; a package **is** a top-level `module` —
its module name is its identity, its file is discovered by walking the tree
(exactly-one-match required). `env.jet` is never a package index.

**U7 — Zero-ceremony single files**: `jet run file.jet` never needs a
manifest, `.jet/`, or any ecosystem file — forever.

**U6 / D-JPK7 / D-JPK15 / VERSION-# — Refs & pins**: manifest source refs are
`provider@target` (`github@owner/repo`, `path@../local`, `nixpkgs@…`); CLI
refs are `<source>:<package>` (`jetpack run nixpkgs:fastfetch`; never Nix's
`#` selector). Versions pin with `#`: `textkit#1.2.0` (`#` = "a pinned
number", shared with `[T#N]`). Channel refs (`#latest`, `#main`) resolve only
in network-class verbs; the lock stays exact (D-JPK-CHANNEL1). Git deps
needing selectors use inline structs (D-JPK23):

```jet
deps: {
    textkit:  "1.2.0",
    helpers:  path@../helpers,
    parsekit: { git: "https://github.com/acme/parsekit", tag: "v0.4.1" },
}
```

**U30 / D-JPK-TOOLCHAIN1 — Toolchain pin**: `pkg.jet` may carry a top-level
`jet:` field pinning the toolchain (channel semantics per D-JPK-CHANNEL1;
`.jet/lock` records the exact version). A jet whose version differs realizes
the pinned toolchain into the hangar (prebuilt objects via D-JPK-CACHE1,
offline thereafter per D-JPK-OFFLINE1, GC per D-JPK-GC1, no Nix required per
D-JPK-NONIX1, no daemon/root per D-JPK-NODAEMON1) and execs it
(D-JPK-DISPATCH1). Frozen-forward identity block: the `payload:` block and
`jet:` line stay parseable by every future jet, so an old jet can always
read enough of any manifest to fetch the right toolchain. `jet toolchain`
shows the pin; `jet update jet` moves it deliberately.

**U9 — Provider inference**: a source is always `name: provider@target`; core
vs nix is inferred by probing the target for `pkg.jet` (cheap manifest-only
probe; `nixpkgs@…` never probed). No `via:` marker.

**D-TGT1–4 — Targets**: packages declare `targets:` (no `kind:`); shipped:
`library`, `executable`, `test`, `example`, `benchmark`; `plugin` reserved.
Bare keyword or block (`executable { entry: "src/cli.jet" }`); bare
`executable` searches `src/main.jet` then `<package>.jet`. **D-ILE1**:
omitted targets infer from `fn run()` (executable else library; two entries
E_DUPMAIN).

**D-CAP4/5/6 + c129 — API freeze (retired 2026-07-04 by D-MEM1/S2)**:
originally, `library { api: stable | explicit }` froze public capability
signatures into `.jet/cache/api/<package>.api` at `jet publish`, drift was
E0912, digest folded into the lock fingerprint. D-MEM1/S2 deleted the
mechanism outright: the `api:` field no longer exists (an ordinary
unknown-key error, E1216, like any typo'd key); `ApiFreeze`'s snapshot
machinery survives, re-grounded as unconditional pub-fn semver diffing
(E1218/E2601) — same intent (breaking-change detection at publish), no
capability-tier freeze.

**Publishing & supply chain**: `jet publish` (version from `pkg.jet`;
refuses dirty tree/failing tests, `--allow-dirty`; errors E1219+)
(D-PUBLISH1A). Published versions permanent; `jet yank --undo` hides from new
resolution only (D-VERSION1). Ranges `textkit#^1.2` freeze in `.jet/lock`
(D-RESOLVE1); `jet new` commits the lock for executables, ignores it for
libraries (D-LOCK1). SHA-256 verification always-on (E1204); Ed25519 signing
opt-in (D-PKGSIGN1). `jet vendor`, `jet audit`, `jet build --sbom`
(D-SUPPLY1; E1217/E1218). Store is content-addressed (D-CASTORE1).

**Build system** *(D-BUILDENTRY1, D-BUILDPOLICY1, D-BUILDSCOPE1, D-BUILDGEN1,
D-BUILDPROFILE1, D-BUILDNORM1, D-BUILDTARGET1, D-BUILDACTION1,
D-BUILDTOOLCHAIN1, D-BUILDPROBE1, D-BUILDCACHE1, D-BUILDREMOTE1,
D-BUILDSCHED1, D-BUILDQUERY1, D-BUILDLEGACY1, D-BUILDPLUGIN1,
D-FRONTENDAPI1, D-DSLBLOCK1, D-METAMUTATE1)*: compile-time build entry is
`fn build(b: BuildContext)`, living in the unit's own definition file (beside
`fn run` / in `pkg.jet` / in `workspace.jet`); `jet build` runs it when
defined, else the batteries pipeline. Build code is tiered: Tier 1
pure+locked by default; Tier 2 needs `#Impure("reason")` + explicit
permission + provenance; deps never get Tier 2 implicitly. Generated source
lands under `.jet/generated/`, never committed; lock records source+output
hashes. Profiles: `Build.{optimize, debug_info, small, panic, features,
env}`, selected by explicit flag (`--release`/`--profile=<name>`), never
ambient env.

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
`jet graph`, `jet query build`, and `jet explain-build <target/file/action>`,
with the LSP using the same graph/provenance model.

D-BUILDLEGACY1=A: legacy CMake/Make/Gradle/npm/cargo builds are Tier-2
wrappers with declared inputs, outputs, and caps; optional graph import lives
inside the same wrapper and CI can ban it. D-BUILDPLUGIN1=A: one build-plugin
contract covers first-party Jet build libraries and packaged/third-party WASM
component plugins under policy; both emit the same BuildPlan graph. D-FRONTENDAPI1=A:
`core.compiler` exposes stable read-only lexer/parser/check/semindex/source-map
value APIs plus a CLI JSON mirror; internal compiler crates stay private and no
AST mutation enters compilation. D-DSLBLOCK1=A: stdlib-only PascalCase
directive DSL blocks such as `#Sql<Row> { ... }` and `#Html { ... }` are a
fixed whitelist in `Syntax.rs`; third-party grammar mutation is rejected.
D-METAMUTATE1=A: Jai-style AST mutation/message loop/user macros are rejected;
the power surface is additive generated modules/overlays, registered
targets/actions, read-only program/build graph enforcement, DSL blocks, and
front-end APIs.

**Migrations** *(D-MIGRATE1, D-MIGRATE2A–F)*: `@PublishedSchema` types
snapshot field layout; a breaking change without a migration is E0910.
Verbs: `add f: T = val`; `remove f`; `change f: Old -> New via { (old) =>
expr }` (converter: inline `via` → `impl Old -> New` in scope → E0910); no
`reorder`. CLI: `jet schema squash --before <ver>`, `jet schema status`.

**Decode-time migration transparency** *(D-MIGRATE3=A)*: `decode_traced<T>(raw)
-> DecodeResult<T> ?` beside `decode<T>` on every codec sharing the decode
machinery (json/csv/toml/yaml); `DecodeResult<T> = { value: T, migration:
MigrationStatus }`, `MigrationStatus = { migrated: Bool, from: String, steps:
[String] }`. `decode` unchanged (I8, zero cost for callers not asking).
`.migrated` is `false` for a plain type and for a `@PublishedSchema` type
decoding fresh (current-shape) data; the migrated cases are D-MIGRATE4.
**D-MIGRATE4 (=A, ratified 2026-07-03, c105migrate4; shipped)**: the runtime
half — codegen lowers each `migration { }` block to a step function; decoding
a `@PublishedSchema` type first tries the current shape (prefer-newest
ambiguity rule), on mismatch detects the source shape by field-name set
(newest matching historical shape wins) and walks the chain oldest→current
applying steps. Plain `decode` applies silently; `decode_traced` records
`from` + `steps` (positional labels: `v1` oldest, steps `"v1->v2"`). No
matching shape → the ordinary decode error. Zero cost for types without
migrations. Runtime semantics: spec.md "Runtime migration chain".

**D-EXPANDCLI1 (=A, ratified 2026-07-03, c183expand)**: the transparency
command is `jet expand --facts <lens> <file>`; bare `jet expand <file>`
runs every lens, grouped (magic default). Lens floor: `inline`
(D-METHODMACRO1); other ratified surfaces add lenses under the same flag,
never new commands (I8). A `refs` lens (D-REF-SHORTHAND1) shipped alongside
`inline` at c183expand; D-MEM1/S3 deleted it along with the `&T`
stored-ref-field mechanism it reported on (`--facts refs` is an
unknown-lens usage error today).

**Jetpack engine** *(D-JPK1/2/5/9/16, D-JPK-ADAPTER1, D-JPK-GC1,
D-JPK-NONIX1, D-JPK-CACHE1, D-JPK-PLATFORM1, D-JPK-NODAEMON1,
D-JPK-OFFLINE1, U5, D-MONOREF1)*: `jetpack` is its own binary
(`run/build/list/clean/add/remove` + `enter`); Jetpack owns the user model,
refs, lock, shells — Nix is one provider behind the `core`-first resolver
trait (tvix shim scoped I6 waiver for the no-installed-nix goal). Ad-hoc
adapters are `Pkg.adapt(name:, source:, recipe:)` with curated recipes
(`prebuilt`, `copy`, `cargo`, `go`, `node`, `cmake`/`make`). Hangar GC by age
(default 30 days), `jet clean` (one verb: garbage-collect + optimize the
hangar via hardlink/dedup, `nix store optimise` equivalent; owner amendment
2026-07-03 — there is no `jet gc`), `jet hangar du`; no daemon, no root
(transient sudo only for jetos activation). No-Nix machines degrade gracefully (E12xx
names fixes). Binary cache = output-hash-addressed HTTP(S) protocol with
signed objects; miss never errors. Linux+macOS+Windows tier-1 native.
Offline is a tested guarantee: realize-class verbs never touch the network
when the lock is satisfied. One canonical merge table (unified-ecosystem §6)
across env/system/image. Monorepo addressing: `source.package` dot form +
in-repo path-style + bare-name sugar when unambiguous.

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
  policy lives under `policy.trust` in `pkg.jet`, e.g.
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
- **D-WD2**: `jet dossier` is the umbrella explain view over named existing fact
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
    is the D-WD dossier lens `jet dossier data`.
  - **D-DATA-PLOT1 (=A, ratified 2026-07-06, #237)**: Core plotting starts with
    first-party deterministic SVG plus a text backend; bitmap export may layer
    on the same model later.
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
  - **D-TARGET-AUDIT1 (=A, ratified 2026-07-06)**: `jet dossier target` is the
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
spellings confirmed as `Pkg.adapt(name:, source:, recipe:)` + the `Recipe.*`
family (`Recipe.prebuilt/copy/cargo/go/node/cmake/make`, expert
`Recipe.build(fn(b: BuildContext))`) — the vision-doc spelling is now law;
`jet add <ref> --adapt` drafts one.

**U1 — manifest history**: superseded — see D-JPK-FILES above (`pkg.jet`).

### Jetpack Images

**D-JPK-IMAGE1 (=A, ratified 2026-07-01, c9jetpackgates)**: active `image.*`
syntax is OCI-only: `from: packages.<name>` (a package this project's `pkg.jet`
declares `executable`) + optional `kind: .Oci`, `expose: [Int]`, `env_vars:
[KEY: "value"]` (map keys must be quoted strings — no bare-ident sugar),
`files: [String]`, and `base: oci("<ref>")` (captured but not yet realized; no
native registry-pull client exists). `jet image <name>` builds a deterministic
OCI layout (`oci-layout`/`index.json`/`blobs/sha256/<digest>`) with an
uncompressed tar layer. `--push` is honestly gated on TLS (E1268), never a fake
push.

### jetos Runtime Slice

**D-JPK-OSVERB1=A / D-JOS-PROOFAPI1=B**: the public CLI is `jet os
check|init|plan|proof|build|switch|rollback|generations|lift|import|image`. `jet os
plan` prints the canonical checked plan without building. `jet os proof` reads
the latest generation's proof, provenance, health, boot, init, secrets, VM, and
rollback artifacts. `jetpack` remains the engine process behind the dispatch
seam; users type `jet os`, not `jetpack os`.

**D-JPK-OSHOST1=C**: a bare host name discovers `system.<host>` in `./config.jet`;
`path@host` selects an exact external root (directory roots load
`path/config.jet`; file roots load that file).

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
  environment source. A profile can apply standalone or attach to
  `system.<host>`; host-specific overrides live where the host composes the
  profile.
- **D-JOS-USERAPPLY1=A**: `jetos user plan|build|switch|rollback|prove` is
  the standalone user-profile path, and `jet os switch` invokes the same
  user-generation engine when a host imports user profiles.
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
- **D-JOS-NIXBACKEND1=C**: `jet os vm prove <host> --disk <path> --real`
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

### CLI & tooling

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

**D-SEMINDEX1**: versioned semantic-index query API (symbols/refs/types/
call-graph/effects/member facts; `jet semindex --json`, schema v3) —
foundation for dossier views, breadcrumb hints, impact analysis, and codemods
(D-DOSSIER1/D-BREADCRUMB1/D-IMPACT1/D-CODEMOD1). `jet dossier <file> [Symbol]`
is the D-WD2 umbrella over those facts; `jet codemod` starts with named JSON
rename objects (`dry-run`/`apply`/`undo`) and replay logs. **D-DX5**: PATH `jet-*` plugin
discovery. **D-REF3**: borrowed-return + cleanup-scope inlay hints on by
default. **D-JPK-DISCOVER1**: `jet search`/`jet info` + LSP completions from
a local offline index. **D-JPK-BUILDDBG1**: failed builds keep the scratch
dir; `--shell-on-fail`; `jet explain <ref>`; `jet logs <pkg>`.

**D-DOC-GEN1=A**: the documentation generator command is `jet doc`. Default
output is deterministic local HTML; `--json` emits the stable docs schema;
`--check` runs doc link, doctest, and stale-example checks. Implementation is
deferred until the owner explicitly reopens documentation build work.

**D-PROVE-REPLAY1=A**: `jet prove` is the umbrella proof/replay command. It
accepts `--replay`, `--lens`, and `--json`, with typed `.jreplay` and `.jproof`
artifacts. Raw solver/runtime text must be laundered into Jet diagnostics.

**D-PERFBUDGET-SURFACE1=A / D-PERFBUDGET-BASELINE1=A**: performance budgets
are declared in role modules such as `module perf.server { budgets: ... }`.
Statistical budgets use pinned baseline artifacts with hardware/toolchain
identity, trend window, confidence policy, and explicit `jet budget update
--baseline <name>` / `jet bench --budget <name>` commands.

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

**D-LSP1 / D-LSP2**: LSP v2 uses one incremental compiler-service query cache
(`crates/jet-queries`) shared by editor requests, with full applicable LSP
3.17 coverage. Every advertised capability must have a named test in
`tests/lsp.rs`.

**D-HL1**: highlighting is generated lexical base plus semantic overlay.
`Syntax.rs` owns all user-typeable tokens; `jet devtools grammars` regenerates
VS Code/TextMate, tree-sitter, and Zed generated sections, and
`tests/grammar.rs` fails on drift. LSP semantic tokens refine live editors for
ownership (`copy`, `^`, `&`) and markers; retired/foreign spellings are not
colored as live syntax.

**D-RECONCILE-SCOPE1 / D-CANON-SOURCE1**: syntax reconciliation is a strict
repo-wide purge of stale spellings; canonical truth is `Syntax.rs` + this
file, CI-checked.

### Superseded & deferred IDs (tombstones)

**S6 — semicolons**: superseded by S6-R (see Formatting).
**S10 — ownership keywords**: superseded by D-CAP7 sigils (see Capabilities).
**S24 — `when` dispatch**: superseded by D-IF1/D-IF3 `if … == { }` (see
Control flow).
**S25 — comparison distribution**: retired by D-S25-RETIRE1; use `|`.
**S29 — dotless struct literal**: superseded by D-DOTCTOR2 `T.{ }` (E0320).
**S35 — `or` fallback**: superseded by `??` (S71).
**S43 — `test` blocks**: superseded by `#Test("name")` (see Testing).
**S53 — concurrency**: deferred past v1.0 (see Capabilities & memory).
**S81 — `?continue`**: superseded by `expr ?? continue` (D-ORRETURN-CANON1).
**U1 / U10 filenames, D-JPK3/8/13, D-BIND1/2, D-ATTR1/3, D-CAP1/2-words,
D-JSONOUT1, D-LITSUFFIX-SCOPE, D-UNIT1-spelling, U18-bare-braces**: all
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
| D-CANVASMETA1 = B (2026-07-09) | Epoch 6 (card #386) | `#Meta(category: "…", tunable)` attribute on bindings/functions, sema-checked fields (unknown field = teaching error), scoped to `category` + `tunable` for now; grows only by ballot. | — |


## Provisional — currently in the code


| ID  | Choice in code                         | Where |
| --- | -------------------------------------- | ----- |
| —   | *(none — Group 1 ratified 2026-06-11)* |       |


## Open decisions — owner input needed

> New decisions are ballot cards in Tower (tools/Tower/tower.json); this table
> is the registry of open language-surface questions.

### Registered for M3–M14 (see tools/Tower/docs/ballots/decision-ballots.md for options)


| ID   | Question                                   | Needed by |
| ---- | ------------------------------------------ | --------- |
| S56  | typed reflection / user derives | **Epoch 3** — [`tools/Tower/docs/plans/epoch-3/user-derives-reflection.md`](../../tools/Tower/docs/plans/epoch-3/user-derives-reflection.md) |


## Decision log

The dated per-decision log (2026-06-10 → 2026-07-02, ~350 rows) was folded
into the current-law entries above on 2026-07-02. The full history — every
amendment chain, ballot narrative, and superseded spelling — lives in the git
history of this file (`git log -p docs/spec/syntax-decisions.md`, up to
commit bfe18d43 and its ancestors). New ratifications append their law to the
topical sections above; they do not restart a log here.
