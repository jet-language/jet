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
naming the canonical form. **D-S14-PAUSE**: this teaching layer is paused
until post-Epoch 6 — retired spellings currently get ordinary syntax errors;
stale teaching fixtures were deleted. **D-CAP10**: one definition per name
(E0105); no overloading — capability disambiguation is call-site sigils on a
single definition.

**D-CASING1 — Casing law + "Core"** *(with D-MARKER-CANON1, D-CONTRACTCASE1)*:
every `#`-marker and every `@`-marker is PascalCase (`#Test`, `#Unsafe`,
`#Grant`, `@Pure`, `@Pre`); traits are PascalCase. The standard library is
**"Core"** — never "std"/"stdlib" — in docs, identifiers, and error copy.

**D-CORENS1 — Single `core.*` namespace** *(D-CORENS-CANON1)*: every
first-party library (built-in module or ring package) is `core.<name>`. No
`jet.*`, `std.*`, or `jet.core` spellings (old ring spelling → E0341).

### Bindings & assignment

**S2 — Bindings** *(current law = D-BIND4; supersedes val/var keywords)*:

```jet
name :: expr            // immutable binding
name := expr            // mutable binding
name: Type :: expr      // explicit-typed immutable
name: Type := expr      // explicit-typed mutable
name = expr             // reassignment of an existing := binding
```

`val`/`var`/`let` are teaching errors; bindings use the sigils above.

**S4 — Type annotations**: `name: Type` after the name, everywhere (bindings,
params, fields). Never `Type name`.

**S17 — Compound assignment**: `+=` `-=` `*=` `/=` `%=` `&=` `|=` `^=` `<<=`
`>>=`. Arithmetic four on Int/Float; the rest Int-only. LHS must be a mutable
binding or `~` parameter.

**D-INCR1 — Increment/decrement**: `++x`, `x++`, `--x`, `x--` on mutable
integer lvalues; prefix yields the new value, postfix the old. Indexed slots
rejected; non-integer E0162; immutable E0161. Deliberate second spelling
beside S17 (owner-chosen I8 exception).

### Functions

**S1 — Function keyword**: `fn`.

**S12 — Entry point**: `fn main()`; no `pub` required. May be fallible:
`fn main() -> Unit ?` (S80). **D-CLIFLAG1**: a typed entry parameter opts into
CLI parsing — `fn run(args: ServeArgs)` derives `--flag` names/defaults/help
from the struct (`@Cli`, `@Doc` markers); enum param derives subcommands;
`cli.parse<Args>(…)` is the library floor. *(markers registered; feature
unbuilt — c7cliflag)*

**S27 — Methods**: `self` receiver with capability sigils (`~self`, `^self`,
`&self`; bare `self` = read). Call `value.method(args)`. Methods live in the
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
one-directional inference only.

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
`T?` — `if s == Rect(w, h)`, `x == null` — yields Bool. Patterns nest to any
depth (`r == ok(Rect(w, h))`). Guards are plain `&&`: a pattern-bound name is
in scope for the rest of the same condition. No `is`, no Rust `match`.

**D-ENUMDOT1 / D-ENUMDOT2 — Leading-dot variants**: match-arm patterns take a
leading dot (`.Circle(r)`, `.Empty`); value position too when the expected
type is known (`.Red`; E0330 fallback). `Color.Red` always valid.

**S74 — Standalone destructuring** *(with D-DESTRUCT1)*: bindings may
destructure structs, tuples, and lists:

```jet
.{ id, severity: sev } :: incident      // struct: bind id, rename severity
.{ kind, .. } :: event                  // partial needs mandatory `..` (E0326)
(x, y) :: point                         // named tuple, canonical order
[a, b] :: xs                            // list, runtime length check (E0315)
value(n) :: maybe_port() ?? return      // refutable bind needs ?? fallback
```

Redundant `..` on a full pattern is E0327. Nesting one level.
*Gap: the dispatch-arm struct-pattern head (`.{ kind: "page", target, .. } ->
…`) is ratified but unbuilt — no `Pattern::Struct` in arm heads yet.*

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
short-circuiting on null; non-optional left side E0047. `??` is the single
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

**S32 — Option**: `T?`; `value(expr)` present, `null` absent; no nullable
plain `T`. **D-RESULT-OPTION-CANON1**: `T?` always means Optional; fallible is
spaced `T ? E` / `T ?` (S34).

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

**D-QUAL3 — Unit families**: `#UnitFamily(currency) { usd, eur, gbp }` mints
one distinct type per member (usd → `Usd`, erases to the base numeric);
cross-unit mixing reuses E0127. **D-UNITLIT1 — unit literals**: `500ms`,
`12.50usd` resolve against in-scope family members (E0134 unknown suffix); no
implicit cross-unit conversion; `e`+digits reserved for float exponents.
Dot-construction `px.{100}` also valid.

**D-TYPEALIAS1 — Aliases**: `alias X = Y` transparent aliases, scoped to
shortening generic spellings only — not primitive/unit newtypes (use
`distinct`). **D-TYPE-ALIAS-CANON1**: `[T]`, `[K, V]`, `*T` are the only
container/pointer spellings; `List<T>`/`Map<K,V>`/`Ptr<T>` are dead.

**D-BIGINT1**: Core `BigInt`, explicit construction `BigInt(…)`/`BigInt("…")`;
`Int` never auto-promotes (E0130–E0133). **D-DECIMAL1**: arbitrary-precision
base-10 `Decimal` in `core.numeric`; default-on lint L0504 fires when a
money-named field holds a float (`#[allow(float_money)]` suppresses).

**D-STATE1 — Typestate** *(D-STATE-REQ/TRANS/DECL)*: states declared in a
`state TypeName { A, B, C }` block; `#State(S) fn m(self)` requires state S;
`#Transition(From -> To) fn` advances it (`_` from-state = entry constructor).
Wrong-state call E0150; markers erase in codegen. Ordering falls out of the
transition graph.

**D-REFINE1 — Refinements**: extend `distinct` with `#Invariant` + a pure-Rust
linear-integer-arithmetic prover for bounds proofs; no new keyword.
*(ratified, unbuilt — c25)*

**D-PENDING1**: blessed loading-state enum `Loadable<T, E>`
(idle/loading/loaded/failed) in Core. **Declined (types)**: `newtype` keyword
(D-SUGAR4); tracked-uncertainty dimension (D-UNCERTAIN1, deferred);
content-addressed definitions (D-CADEFS1, frozen).

### Collections

**S37 — List literal**: `[a, b, c]`; empty `[]` needs a context type
(**S78**: `[]` infers from expected type; explicit `[]: [T]` always accepted).

**S38 — Map literal**: `["key": value, …]`; empty `[:]`.

**S65 — List type shorthand**: `[T]` is the canonical list-type spelling.

**S64 — Map shorthand & entry iteration**: `[K, V]` is the canonical map-type
spelling. One-binding map iteration yields `.key`/`.value` entries;
two-binding `loop name, amount in fruits` also supported.

**S39 — Indexing**: `xs[i]` / `m[k]` stop with a friendly report on
OOB/missing key; `xs.get(i) -> (T?)` safe access; `m[k] = v` inserts.

**S40 — Slicing**: `xs[a..b]` inclusive, copies (no exposed references);
`s.slice(a..b) -> String` on character positions; L0501 lints slice copies in
loops.

**D-ITER1 — Iterator adapters**: the full lazy family (enumerate, zip,
chunks, windows, take/skip(_while), flat_map, scan, group_by, dedup, step_by,
peekable, partition, find/position, fold/reduce, min/max_by, …) on the
iterator protocol; allocation-free until a terminal op.

**D-COLLBREADTH1**: `Set<T: [Hash, Eq]>` and ring-buffer `Deque<T>` in
`core.collections` (E0506). **D-ENC-DYN1**: `Data` is the single dynamic value
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
bindings without an expected type — **not yet implemented** (lexer prefix-
scan gap, no upstream gate; the no-prefix expected-type path is the common
case and covers it); user-defined prefixes deferred to E4.

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

**S80 — Error carrier & fallible main** *(D-ERR2)*: default `Error` carries
message + optional code + optional source (`Error.message("…")`,
`Error.code(n)`, `Error.with_source(e)`). `fn main() -> Unit ?` allowed;
returned errors print in the diagnostic voice, exit non-zero. Cross-type `?`
conversion is opt-in via the `Fallible` trait (`fn to_error(self) -> Error`);
prelude types implement it, unrelated enums never convert silently.

**D-ERRCTX1 — Error context**: automatic `?`-propagation trace in dev builds;
stdlib `.context("msg {var}")` (lazy) for human wording. No new grammar.

**S36 — Bug stops**: `panic("msg")` (friendly report, exit 70);
`require(cond[, "msg"])` for invariants/preconditions. Prelude builtins.

**D-IGNORERET1**: discarding a fallible/`#MustUse` result requires a visible
discard sigil at the call site; sema lints at the discard point. *(unbuilt)*

**Teaching & lint law**: `=` in a condition is E0322 with a "did you mean
`==`?" fix (D-ASSIGNCOND1). Homoglyph confusable names lint L0503 default-on
(D-CONFUSE1). Semantic-smell lints — float `==`, duplicate branches
default-on; always-true condition opt-in (D-SMELLLINT1, unbuilt).

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
namespaces: `env` (`Env`), `system` (`System`), `image` (`Image`),
`workspace`. **D-JPK-MODBODY1**: role namespaces live in the declaration
name — `module env.dev { packages: […] }`, `module system.laptop { … }`.

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

**S48 — Dynamic dispatch**: a trait name in type position (`List<Shape>`,
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
Snapshot; fn restore(self: ~Self, snap: Snapshot) }`; restore total;
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

**D-QUAL2 — Tag vs trait**: exactly two qualifier kinds — `trait` (has
methods, dispatches) and `tag` (no methods, erases). Methods on a tag E0732;
tag where dispatch expected E0731. **D-QUAL4**: type-position value tags are
prefix — `#Tainted String`.

**D-MATURITY1**: `@Experimental` / `@Tested` / `@Hardened` are doc-only
markers before `fn` — parsed, erased, zero semantic effect.

### Capabilities & memory

**S10 / D-CAP7 — Capability sigils** *(owner-frozen; supersedes all word
spellings)*:

```jet
T     // infer: starts at read/view, elevates only as the body requires
~T    // edit:  exclusive write/mutate access
^T    // take:  ownership moved/consumed
&T    // share: may escape the scope, be retained, cached, spawned, stored
*T    // raw:   unsafe pointer/address (#Unsafe-gated)
```

```jet
fn write(file: ~File, data: Bytes)     // read is the default → no sigil
fn equip(player: ~Player, item: ^Item)
```

Call sites mirror the type — `damage(~player, 10)`, `close(^file)`,
`cache(&texture)`; receivers carry it on self (`fn damage(~self)`,
`fn destroy(^self)`, `fn share(&self)`). Capability sits on the type side
(`name: ~Type`, D-CAP3). `copy x` stays a verb — no sixth sigil (D-CAP2).
Dereference is **postfix `p.*`** (composes: `p.*.field`); prefix `*x` is
raw-pointer-of only, `#Unsafe`-gated (D-CAP9). `mut`/`take`/`view` are not
keywords (E0056–E0058 point at the sigils).

**D-CAP8 — Unmarked default**: infer-in-bodies, freeze-at-API — an unmarked
param elevates by usage; at a `library { api: explicit }` boundary the
resolved capability freezes; later drift is E0912, never a silent flip.

**D-MUTSELF1 — Receiver mutation**: a `~self` method mutates in place —
`self.field = v`, compound ops, and whole-`self` reassignment all lower
through the deref'd receiver; the same write in a read method is E0205 with a
"write the receiver as `~self`" fix at the assignment.

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
`async`/`await` coloring (gated on scheduler work).

### Effects & safety

**D-EFF1 — Effect system**: inferred per-fn effect sets (Koka-style rows),
erased in codegen. Assert/restrict via `#(Net, Db)` on a signature and
`#Caps(Net) { … }` regions.

**S60 — Purity marking**: `@Pure fn` is a checked signature modifier — the
empty effect set; violations name the impure call path. Also valid as a
function-type bound (D-MARKERMOVE2).

**D-EFF4 / D-EFF5 — Vocabulary**: closed flat set of ten — `Net`, `Fs`, `Io`,
`Db`, `Time`, `Rand`, `Env`, `Exec`, `Log`, `Gpu`; unknown name E0119; no
subsumption (`Net` under `#(Io)` is E0740). `effect <Name>` user declarations
reserved, unminted.

**D-EFF2 — Polymorphism**: transparent flow-through by default; escaping
function values assume the maximal set. Expert levers: effect-bound function
types (`@Pure fn(T) -> U`, `#(Net) fn(T) -> U`; call-site check E0747) and
`#(via f)` pass-through publication (E0748).

**D-EFF3 — Traits**: a trait method may declare an effect upper bound — both
the impl obligation (E0710) and the dispatch contract for trait objects.

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
not routed through a mockable capability. *(unbuilt)*

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
*(ratified, unbuilt — c157)*

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
(allocators, `*T`, layout/repr, volatile). `#Unsafe("reason") { … }` /
`#Unsafe("reason") fn` is the audit gate (**D-UNSAFE2** — the reason is the
gate's argument; missing reason L3101; whole-fn form requires an enclosing
`#Unsafe` at call sites). Gated ops: deref `p.*`, raw-pointer-of `*x`,
pointer math, transmute-class casts, FFI pointer crossings (outside the gate:
E0208). Address-of is `mem.address_of(x)`. `mem.cast_ptr<T>(p)` is the cast
primitive (D-CASTPTR1); no compact pointer-chain syntax (D-POINTERCHAIN1).
Generated `unsafe` appears only inside user-gated regions + vetted internals
(I1). Onboarding never mentions any of it.

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
*(reserved keyword today; unbuilt)* **D-NOSTD1**: no `no_std` flag — the std
baseline follows the typed platform `target:` (bare-metal ⇒ no-std).
**D-OOBPROOF1**: bounds-check elision must be proof-carrying (rides
D-REFINE1).

### Testing & benchmarks

**S43 — Tests** *(D-TESTPAREN1, D-TGT5)*: `#Test("name") { … }` blocks with
`require`/`require_eq`; `jet test` auto-collects every `#Test` in the
package; optional `test { entry: … }` target adds an out-of-tree file.
**D-TEST1**: a parameterized `#Test fn name(p: T)` is a property test —
~200 generated cases (`JET_PROP_SEED`), automatic shrinking; ungeneratable
param type E0613. **D-TEST4**: fenced ```jet blocks in `///` docs run as
doctests; `EXPR // => VALUE` compares JetShow output (E2901).

**D-BENCH1**: `#Bench "name" { … }` region benchmarks, run by `jet bench`
(ops/sec + ns/iter); the `benchmark` manifest target points `jet bench` at a
package entry.

**D-COV1**: `jet test --coverage` — per-function HIT/MISS table; probes only
in this mode, normal codegen byte-identical. **D-TOOL4**: snapshot testing
with `-u`/`--update-snapshots`. **D-A11YGATE1**: accessibility issues are
`jet lint --a11y` lints (E2930/E2931), opt-in CI gate.

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
Per-language binder depth (typed projection / runtime broker / shallow
decls) is a follow-up ballot per language (D-FFI-PY1, D-FFI-JS1,
D-FFI-SWIFT1 honoring D-JSWIFTFFI1 sequencing).

**D-DEP1 — Dependency law**: the compiler stays zero-external-crate (I6).
Any crate-backed capability ships as a Jet package wrapping the crate via
`extern rust`, source vendored + hash-pinned (D-BFS1). Owner-sanctioned
bootstrap wraps (all carry a native-ize obligation): `regex` (D-REGEX1,
`core.regex`), rustls (`core.tls`, D-NET1/D-HTTPLIB4 — embedded webpki
roots, no system TLS), zip/tar/flate2 (`core.archive`, D-DEP-ARCHIVE1),
rusqlite-bundled (`core.db`, D-DEP-DB1), ureq/hyper/tungstenite
(`core.http`, D-NETDEP1/D-HTTPLIB3), Cranelift (`jet-jit`, D-JITDEP1),
wasmtime (plugins, D-DEP-WASM1), age-style crypto bridge
(D-JPK-SECRETCRYPTO1). `jet repl` stays std-only (D-REPL18). Raylib ships as
first-party `core.raylib` bridge package (D-RAYLIB1). npm interop = typed
first-party stub packages, no `.d.ts` parsing (D-NPMTYPE1); Swift interop
waits on native-UI/C-ABI work (D-JSWIFTFFI1).

### Core library

**S9 — Print**: `print` (adds newline).

**S51 — Core library**: exported as the `core` module — `use core.fs`,
`use core.io as io`; dot paths select submodules; never quoted paths. `core`
is compiler-reserved (see D-CORENS1).

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

**Filesystem & time**: typed `Path` (`from`/`join`/`parent`/`extension`/
`stem`), `write_atomic()`, lazy cycle-safe `walk()` (D-PATHFS1, unbuilt);
`fs.list_dir -> [DirEntry]` (D-LSDIR1). Full civil time — Date/DateTime/
Duration/Zone over IANA tz, layered on the injectable `Clock`
(D-TIMEDEPTH1, unbuilt). PRNG `core.random` (SplitMix64, seedable) vs CSPRNG
`core.crypto.random` (D-RANDSPLIT1); both carry `Rand`.

**Crypto**: misuse-resistant `seal`/`open` + `sign`/`verify` defaults; raw
primitives require `core.crypto.expert` behind `#Unsafe` (D-CRYPTOENV1,
E0510/E0511). Versioned `JETC` envelope header gives algorithm agility;
PQ algorithms later (D-PQCRYPTO1). `core.encoding` hex/base64 + `core.uuid`
v4/v7 (D-UUIDENC1).

**Numerics & data**: `core.linalg` ring package — `Vec2/3/4`, `Mat3/Mat4`,
`.dot()`/`.cross()`/`.matmul()` as aliases over a generic `Vec<N>`/
`Matrix<M,N>` substrate (const-generic substrate unbuilt) (D-MATHLIB1,
D-LINALG1). `core.db`: backend-neutral `Driver` trait, parameterized-only
API, SQLite first; explicit `.begin/.commit/.rollback` distinct from
`#Transact` (D-DBDRIVER1). `core.http`: client+server submodules; server is
plain `fn(req: Request) -> Response` on a `mux` (`mux.get("/path", handler)`,
`req.params["id"]`, `http.serve(addr, mux)?`); HTTP/1.1+2+WebSocket
(D-HTTPLIB1–3, D-ROUTE1; unbuilt — c164). Compression
`core.compress.{gzip,zstd}` (D-CODECS1). Measurement-with-uncertainty in
`core.science.measurement` (D-HONESTNUM1, unbuilt). Opt-in `Gc<T>` module
for cyclic data (D-OPTGC1, gated on its I6 ballot). Approximate/sketch algos
are libraries (D-APPROX1); parallelism stays explicit `par_*`
(D-AUTOPAR1); adaptive fidelity is a manual knob (D-ADAPTFID1).

**Reactive & UI stack** *(D-REACT1, D-REACTCORE1, D-SIGNAL1, D-RENDERTGT1/2,
D-UITREE1, D-STYLESHAPE1, D-MOTIONTIME1, D-LAYOUT1, D-OWNCOMP1, D-A11Y1,
D-NATIVEUI1/2)*: reactivity is a library + explicit `#Reactive` scope marker
(E2914) lowering onto `core.reactive` — `Signal<T>` (`.get()/.set(v)`),
`Computed<T>`, `Effect`; explicit-by-read subscription; pure std runtime
(E2910–E2913). Render backends implement measure/layout/paint (`JetBackend`;
`NullBackend`/`TuiBackend` shipped). UI trees are typed dot-construction
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
error). *(formatter emission for `#Target`/`#Html` still owed)*

**D-OBS1 / D-OBS3 — Observability**: source maps + Jet-line panic reports;
OTel-aligned std-only structured logs/metrics; exporters are FFI-wrapped
packages, never compiler deps.

### Manifest, packages & jetpack

**S52 — Files** *(D-JPK-FILES, D-JPK-FILENAME2)*: per-package manifest
is **`pkg.jet`** (`payload: { name, version }` identity + `packages:` +
`deps:` + `targets:` + `effects:`); dev shell is **`env.jet`**; monorepo
index is **`module workspace` in `workspace.jet`** (`members:` may run
comptime; D-WORKSPACE1/2 — the root `jetpack.toml` index is retired);
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
omitted targets infer from `fn main()` (executable else library; two mains
E_DUPMAIN).

**D-CAP4/5/6 + c129 — API freeze**: `library { api: stable | explicit }`;
inference is the default forever. `jet publish` freezes public capability
signatures into `.jet/cache/api/<package>.api`; drift is E0912; digest folds
into the lock fingerprint.

**Publishing & supply chain**: `jet publish` (version from `pkg.jet`;
refuses dirty tree/failing tests, `--allow-dirty`; errors E1219+)
(D-PUBLISH1A). Published versions permanent; `jet yank --undo` hides from new
resolution only (D-VERSION1). Ranges `textkit#^1.2` freeze in `.jet/lock`
(D-RESOLVE1); `jet new` commits the lock for executables, ignores it for
libraries (D-LOCK1). SHA-256 verification always-on (E1204); Ed25519 signing
opt-in (D-PKGSIGN1). `jet vendor`, `jet audit`, `jet build --sbom`
(D-SUPPLY1; E1217/E1218). Store is content-addressed (D-CASTORE1).

**Build system** *(D-BUILDENTRY1, D-BUILDPOLICY1, D-BUILDSCOPE1, D-BUILDGEN1,
D-BUILDPROFILE1, D-BUILDNORM1)*: compile-time build entry is
`fn build(b: BuildContext)`, living in the unit's own definition file (beside
`fn main` / in `pkg.jet` / in `workspace.jet`); `jet build` runs it when
defined, else the batteries pipeline. Build code is tiered: Tier 1
pure+locked by default; Tier 2 needs `#Impure("reason")` + explicit
permission + provenance; deps never get Tier 2 implicitly. Generated source
lands under `.jet/generated/`, never committed; lock records source+output
hashes. Profiles: `Build.{optimize, debug_info, small, panic, features,
env}`, selected by explicit flag (`--release`/`--profile=<name>`), never
ambient env. Build cache hashes at AST level.

**Migrations** *(D-MIGRATE1, D-MIGRATE2A–F)*: `@PublishedSchema` types
snapshot field layout; a breaking change without a migration is E0910.
Verbs: `add f: T = val`; `remove f`; `change f: Old -> New via { (old) =>
expr }` (converter: inline `via` → `impl Old -> New` in scope → E0910); no
`reorder`. CLI: `jet schema squash --before <ver>`, `jet schema status`.

**Jetpack engine** *(D-JPK1/2/5/9/16, D-JPK-ADAPTER1, D-JPK-GC1,
D-JPK-NONIX1, D-JPK-CACHE1, D-JPK-PLATFORM1, D-JPK-NODAEMON1,
D-JPK-OFFLINE1, U5, D-MONOREF1)*: `jetpack` is its own binary
(`run/build/list/clean/add/remove` + `enter`); Jetpack owns the user model,
refs, lock, shells — Nix is one provider behind the `core`-first resolver
trait (tvix shim scoped I6 waiver for the no-installed-nix goal). Ad-hoc
adapters are `Pkg.adapt(name:, source:, recipe:)` with curated recipes
(`prebuilt`, `copy`, `cargo`, `go`, `node`, `cmake`/`make`). Hangar GC by age
(default 14 days), `jet gc`, `jet hangar du`; no daemon, no root (transient
sudo only for jetos activation). No-Nix machines degrade gracefully (E12xx
names fixes). Binary cache = output-hash-addressed HTTP(S) protocol with
signed objects; miss never errors. Linux+macOS+Windows tier-1 native.
Offline is a tested guarantee: realize-class verbs never touch the network
when the lock is satisfied. One canonical merge table (unified-ecosystem §6)
across env/system/image. Monorepo addressing: `source.package` dot form +
in-repo path-style + bare-name sugar when unambiguous.

**U1 — manifest history**: superseded — see D-JPK-FILES above (`pkg.jet`).

### jetos

**U11 — `System`**: fields `target` (typed platform value, `linux.x64`,
never a string), `packages`, `services`, `options`.

**U12 — `Service`**: open record, first field `enable: Bool`
(`openssh: { enable: true, ports: [22] }`); bare `{ … }` under `services:`.

**U13 — `options:`**: ordered list of dotted `key: value` pairs; Jet values
bare, free-form strings quoted. **D-OS4**: priorities are a map
`[default: x, force: y]`; bare assignment = `default`.

**U14 — `Image`**: `from: system.<name>` + `format: iso|qcow|raw`; inherits
everything from its System (explicit `target:` only for cross-compiling).

**U15 — Verbs**: whole-machine management is `jetpack os switch|build`.

**U16 — Target selector**: `[<config-path>]@<host>`, default path
`~/.jet/config.jet`; `@host` picks the System.

**D-OS6**: user scope `user.<name>.*` with `user.me` alias.
**D-JPK-OSNAME1**: the OS is named **jetos**. **D-JPK-DISPATCH1**: verbs
dispatch by executable name (`jetpack`, `jetos`), never linked into the
compiler process.

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
call-graph/effects; `jet semindex --json`) — foundation for dossier views,
breadcrumb hints, impact analysis, and codemods (D-DOSSIER1/D-BREADCRUMB1/
D-IMPACT1/D-CODEMOD1, all gated on it). **D-DX5**: PATH `jet-*` plugin
discovery. **D-REF3**: borrowed-return + cleanup-scope inlay hints on by
default. **D-JPK-DISCOVER1**: `jet search`/`jet info` + LSP completions from
a local offline index. **D-JPK-BUILDDBG1**: failed builds keep the scratch
dir; `--shell-on-fail`; `jet explain <ref>`; `jet logs <pkg>`.

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
