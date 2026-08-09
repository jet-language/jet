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
- Typed text (D-TYPEDTEXT1/2, D-FFI-SH1, D-UNIFYLIT1=A): `SQL.{"…"}`,
  `HTML.{"…"}`, and `Sh.{"…"}` use one checked interpolation engine. For `Sh`,
  literal words become argv items and each `{hole}` becomes exactly one argv
  item; neither word splitting, glob expansion, nor shell parsing touches a
  hole. Runtime `String` conversion is E0149; `Sh.raw(text)` is the audited
  escape. Bare `"…"` never elaborates into these types.
- Numbers (S67): decimal `Int` (64-bit signed, E0007 if too large) and `Float`
  (digits `.` digits, optional `e`/`E` exponent). `_` digit separators are
  allowed anywhere among the digits (`1_000_000`); base prefixes `0x`/`0o`/`0b`
  give an `Int` (`0xFF`, `0o755`, `0b1010`), and a prefix with no digits is
  E0001. Unary minus is an operator, not part of the literal. In an operator
  expression, a bare whole-number literal adopts a fixed-width peer when that
  type contains its exact value (D-INTLIT-WIDTH1=F); with no sized peer it stays
  `Int` (D-NUMLIT-PEER1=A). The operands then follow the ordinary numeric
  widening law. A typed or destination-owned literal keeps that destination's
  range check.
- Explicit conversion (D-SHAPE-CONVERT1=A) is destination-owned:
  `Target.from_source(value)`. Numeric narrowing returns a fallible result.
  Safe widening is implicit under D-INTLIT-WIDTH1, D-VERDICT-1304-1, and
  D-NUMWIDEN-CROSS1. Numeric-backed distinct and unit types use the same
  source-kind names. Text interpretation remains `Target.parse(text)`.
  Source-owned `to_*`, casts, and a neutral `convert` helper are absent.
- Runtime durations (D-SHAPE-DURATION1=A, D-SHAPE-DURATIONCONVERT1=A,
  D-TIMERES1=A) use
  `Duration.nanoseconds|microseconds|milliseconds|seconds|minutes|hours(number)?`;
  non-finite and
  out-of-range values fail with `RangeError`. A Duration stores a whole-nanosecond
  count. `duration.in(.Unit)?` reads a whole `Int` unit, truncating toward zero.
  `is_zero()`, `total_seconds()`, and `difference(other)` are Duration facts.
  Compile-time duration literals are unchanged.
- Civil time (D-TIMEDEPTH1 / D-TIME-CALENDAR1): `LocalDate` adds
  `quarter_of_year`, `days_in_month`, `is_leap_year`, and `replace(y, m, d)`.
  `DateTime` adds sub-second accessors (`millisecond` / `microsecond` /
  `nanosecond`), `floor` / `ceil` beside `truncate` / `round`, `replace(...)`,
  and `difference(other) => Duration`. `ZonedDateTime` adds `is_dst()`.
  `core.time.datetime(...)`, `core.time.time(...)` / `local_time(...)`,
  `days_in_month(y, m)`, and `is_leap_year(y)` construct or query the same
  values. `Instant.elapsed()` returns a `Duration`.
- `true` and `false` are `Bool` literals.
- Source has no visible statement separators. The lexer inserts internal
  terminators at line ends after statement-ending tokens (S6-R).
- The lexer recovers from bad characters and keeps going; one run reports
  every lexical error it can.

### Grammar (EBNF)

```
program  = { func | struct | const } ;
func     = [ "pub" ] "fn" ident "(" [ params ] ")"
           [ "=>" [ type ] | "=[" [ effect-row ] "]=>" type ]
           ( block | "=" expr NL ) ;
params   = param { "," param } ;
param    = ident ":" [ "^" | "&" ] type ;
effect-row = effect { "," effect } | ".." ident ;
block    = "{" { stmt } [ expr ] "}" ;   // S3: multiline grouping
// S6-R: no visible `;` — the lexer inserts a synthetic terminator (NL below)
// at each line end after a statement-ending token; the grammar stays
// terminator-based. A leading `.` or binary/logical operator on the next line
// suppresses insertion (continuation). A callable arrow, `=`, or `{` stays
// attached to the declaration head. `NL` denotes that synthetic terminator.
stmt     = binding | assign | if | loop | fenced-stmt
         | break | next | "return" [ expr ] NL
         | expr NL ;
binding  = [ "#Track" ] ( ident "::" expr     // immutable
         | ident ":=" expr ) NL               // mutable
         | destructure ( "::" | ":=" ) expr NL ;
// Types ride the value (D-DOTCTOR3 `Type.{ … }`) or live on signatures/fields.
// Retired: ident ":" type ("::" | ":=") expr  (D-BIND-BARE1).
destructure = ".{" ident { "," ident } [ ", .." ] "}"   // S74: struct fields
            | "[" [ ident { "," ident } ] "]" ;    // S74: list elements
fenced-stmt = fence ( "::" | ":=" ) expr NL | expr-with-fence NL ; // D-EACH1=C / D-VERDICT-1320-1
fence    = "$[" fence-entry { "," fence-entry } "]$"
         | "$[" numbered-name ".." numbered-name "]$" ;
// binding fences: entries are plain names; expression fences: any expression
assign   = ident ( "=" | "+=" | "-=" | "*=" | "/=" | "%="
                 | "&=" | "|=" | "^=" | "<<=" | ">>=" ) expr NL ;
// D-IF1/D-ARROW-CONTROL1: `if` is the one branching keyword.
if       = "if" cond block
           { "else" "if" cond block } [ "else" block ]
         | "if" subject "==" "{" { arm } [ "else" "->" arm-body ] "}"    // ordered arm table with named subject
         | "if" "{" guard-arm { guard-arm } [ "else" "->" guard-stmt-body ] "}" ; // ordered arm table
arm      = arm-head "->" arm-body NL ;
guard-arm = cond "->" guard-stmt-body NL ;
effect-body = block ;
guard-stmt-body = block | non-if-stmt ;
arm-head = value | range | condition ; // bare value ⇒ `subject == value`; range `lo..hi` ⇒ membership (D-PATR/D-RANGE1); else a Bool condition (D-IF2 Q3)
range    = expr ".." expr ;            // inclusive (S22); no `..=` (E0318), no `step` in arm head (E0319)
arm-body = block | stmt ;        // `{ … }` block or one braceless statement (D-IF2 Q2)
loop     = [ ident "::" ] loop-body ;            // D-LOOPLABEL3: optional ordinary-name label
loop-body= "loop" block
         | "loop" cond block
         | "loop" source-clauses [ "if" cond ] loop-result-body
         | "loop" ident ":=" expr "," cond [ "," expr ] loop-result-body ;
source-clauses = source-clause { "," source-clause } ;
source-clause = ( ident | "(" ident "," ident ")" ) "," source [ "," expr ] ;
loop-result-body = effect-body | "->" value-arm-body ;
source   = expr ;                              // a range literal is one Range expression (D-RANGE-VALUE1)
break    = "break" [ expr | "(" ident [ "," expr ] ")" ] NL ;
next     = "next" [ "(" ident ")" ] NL ;
cond     = expr | "(" expr ")" ;                     // S68/D-SG2: optional parens, fmt strips them
if-expr  = "if" cond "->" value-arm-body
           "else" ( "->" value-arm-body | if-expr )
         | "if" "{" value-guard-arm { value-guard-arm } "else" "->" value-arm-body "}"
         | "if" subject cmp-op "{" value-dispatch-arm { value-dispatch-arm }
           "else" "->" value-arm-body "}" ;   // D-IFDIST1
value-dispatch-arm = arm-head "->" value-arm-body NL ;
cmp-op   = "==" | "!=" | "<" | ">" | "<=" | ">=" ;
value-guard-arm = cond "->" value-arm-body NL ;
value-arm-body = expr | value-block ;
value-block = "{" { stmt } expr "}" ;
expr     = precedence climbing over:
           "||"  >  "&&"  >  "==" "!=" "<" ">" "<=" ">="
           >  "|"  >  "~|"  >  "&"  >  "<<" ">>"      // D-XORSPELL1: "~|" is xor
           >  "+" "-"  >  "*" "/" "/%" "%" "%%"  >  unary "-" "!"
           >  "^"                                     // D-EXPSEM1: power, groups right
           >  call | ident | literal | "(" expr ")" ;
```

### Semantics

- Types: `Int`, `Float`, `Bool`, `String`. Local inference: types ride the
  value (`Type.{ … }`) when needed; mismatched headed literals are ordinary
  type errors.
- A program must define `fn run` with no parameters and no return type,
  `fn run() => () ?` for top-level error propagation, or a single typed CLI
  parameter as described by D-CLIFLAG1 (E0101, E0122, E1308). Execution starts
  there. `run` never takes `pub` (S12).
- `name :: value` is immutable; `name := value` is mutable (D-BIND-BARE1).
  Assigning to an immutable binding is E0111.
  Names may not shadow an existing name in scope (E0118).
  Types never annotate the binding name — use `Type.{ … }` or a signature/field.
- `$[ a, b ]$` expands one complete binding or expression statement per entry.
  Multiple fences advance in lock-step. `$[ task1..task8 ]$` generates or
  reuses the ascending numbered names. Expression-position fences accept
  expression entries (`print($[ "a", total(1, 2) ]$)`); binding fences need
  plain names. A fence is not a list or destructure (D-VERDICT-1320-1).
- `#Track name :: value` / `#Track name := value` opt a binding into
  D-PROVENANCE1 provenance. Today this records Float binding origins for
  `value.origin() => String`; untracked Floats return `"untracked"`.
- Arithmetic: `+ - * /` widen one numeric operand to the other when the ruled
  numeric widening law permits it; `% & | ^ << >>` remain integer-only.
  `+` on `String` is a teaching error pointing at interpolation. Compound
  assignment (S17) mirrors the binary operators.
- Comparisons (`== != < > <= >=`) use the same numeric widening law and yield
  `Bool`; other operand types must match. `&& || !` operate on `Bool` (E0110).
- `&&` and `||` combine `Bool` expressions only (D-S25-RETIRE1). Value
  alternatives in arm heads use single `|`.

A control construct is an expression wherever it produces a value; its runtime
artifacts are types; the construct itself never is. Jet already uses lambdas
for deferred control, so `Loop` and `If` types would duplicate the lambda
mechanism and violate I8. Value-producing cases are already expressions. Typed
artifacts hold reusable values, while constructs stay zero-cost keywords and
keep code readable from top to bottom. See
[type-unification audit F11](../audits/type-unification-audit-2026-07-28.md#f11--spec-law-constructs-are-never-types-their-artifacts-always-are).

- `if` is Jet's one branching form. Its preferred multi-branch surface is an
  ordered arm table: `if subject == { head -> body }` when naming a subject
  improves clarity, or `if { head -> body }` without one. A head may be a value
  or structural pattern against the subject, or any `Bool` expression evaluated
  as written; unrelated expressions may appear in the same table. The first
  matching or true head wins. Chained `else if` remains legal, but there should
  rarely be a reason to prefer it and it is not a canonical teaching form.
  Conventional effect-only branches have no arrow. Arm-table arrows select an
  arm, including an arm yielding `()`. Value branches require `else` unless a closed
  subject is exhaustive; result types unify. Braces group multiline bodies.
- `loop` has infinite,
  conditional, source (`loop x, source [, stride]`), map-pair
  (`loop (key, value), source`), and explicit-state
  (`loop i := init, cond [, afterthought]`) headers. `a..b` and `a..<b`
  construct one `Range` value over `Int`; the first includes `b` and the second
  excludes it. A Range may be stored, passed, returned, and used as a loop
  source or slice bound. It exposes `.start`, `.end`, and `.contains(value)`.
  Literal range loops still compile directly to jumps without allocation.
  Source/bounds/stride evaluate once left-to-right; stride must be positive `Int`
  and is checked before the first pull. `break`/`next`
  inside loops only (E0115, S23). A loop may carry an ordinary-name label
  (D-LOOPLABEL3) — `outer :: loop … { }`. `break(outer)`,
  `break(outer, value)`, and `next(outer)` target it from a nested loop.
  E0987 names an out-of-scope label. E0988 teaches retired dot and `@` forms,
  rejects `outer := loop`, and explains that a loop name is not a runtime
  value.
  Normal explicit-state fallthrough and targeted `next` run the afterthought
  exactly once, then retest; normal source fallthrough and targeted `next` pull
  stride items and use the final pull. `break`, `return`, propagated failure,
  and panic skip the target afterthought. Abandoned inner loops run no edge.
  Bare `next` is control only as a complete statement or `??` fallback;
  `next()`, `.next()`, and `fn next` are ordinary identifier uses, while a value
  named `next` after `??` needs parentheses: `value ?? (next)`.
- A finite source or C-style loop may use `-> expression` or `-> { ... }`.
  Each accepted iteration yields one non-unit value. The result is an eager
  List in iteration order. A header guard or `next` omits items. Multiple
  source clauses yield one flat List; an explicitly nested yielding loop
  preserves nesting. Maps and Sets use explicit terminals. Lazy work uses the
  existing iterator adapters.
- A bare or condition-only loop does not accept `->`. It returns one final
  value only through `break value` or `break(name, value)`. All payload exits
  unify. In a yielding loop, `break` returns the partial List and payload
  breaks are rejected.
- `if subject == { head -> { ... } else -> { ... } }` (D-IF1/D-IF3) tests arm
  heads top to bottom. Bare values and ranges compare against the subject;
  predicate heads are `Bool`; `else` is mandatory unless enum/option
  exhaustiveness proves coverage.
- **Range arms (D-RANGE1/D-PATR, c25):** in multi-arm `if`, an arm head that is
  a range `lo..hi` fires when the subject is in that inclusive band (S22) —
  `90..100 -> "A"` desugars to `subject >= 90 && subject <= 100`. The subject
  and bounds must share an ordered scalar type (`Int`/`Char`); the open
  `Int`/`Char` domain always still needs a trailing `else` (D-PATR). c25 adds
  the porting-hazard teaching errors: `..=` in an arm head is **E0318** (Jet's
  `..` is already inclusive — write `lo..hi`), `step` in an arm head is
  **E0319** (`step` is a loop modifier, not a band), and an inverted/empty band
  `hi..lo` is **E0316**. Arm heads accept range literals only. A
  `distinct Int(0..10)` constraint also stays literal-only because a runtime
  Range cannot determine a type declaration (D-RANGE-VALUE1=A).
- **Ambient surface (D-NAME-ALIAS1=A, D-CORE-PRELUDE1/2):** one readable
  `core/prelude.jet` module declares the closed no-prefix surface. Functions
  are `print`, `input`, `panic`, `require`, `assert`, and `assert_eq`.
  `pub use` aliases add `eprint`, `Clock`, `Instant`, `Date`, `Duration`,
  `Path`, `read_file`, `write_file`, and `file_exists`. The comptime-gated
  names `embed_file`, `embed_bytes`, `find`, and `fetch` stay gated at their
  existing declarations. `random` stays qualified as `core.math.random`.
  User declarations replace a prelude alias and produce the ratified shadow
  lint; libraries cannot inject names; additions and removals need an owner
  ballot. Core meaning stays in Prelude/CoreLib (I9).
  **`#NoPrelude` (D-PRELUDEX1=A)** opts a file out of every readable prelude
  name. Use a qualified Core call, or remove the marker.
- **Tool artifact extensions (D-ARTIFACT-EXT1=A):** the closed family is
  `.jetmap` (source maps), `.jetnb` (notebooks), `.jetproof` (proof evidence),
  `.jettrace` (performance traces), `.jetreplay` (game input replays), and
  `.jetproof-replay` (proof replays). Consumers reject a different family
  member by artifact kind; retired suffixes have no compatibility aliases.
- `print(x)` is prelude-declared (S9); takes one or more printable arguments
  (E0103, E0112) and writes each on its own line with a trailing newline
  (D-VERDICT-1321-1). `io.print`/`io.eprint` accept the same variadic
  form. `Float` always prints a decimal part (S21): `-5.0`, not `-5`.
- `input()` / `input(prompt)` is prelude (D-NAME-ALIAS1); reads a line from
  stdin, strips the trailing newline, and returns `String ? IOError`.
  Use `??` to unwrap or handle the error.
- Functions: multi-argument calls, checked arity (E0104) and argument
  types (E0112). A function with a return type must return on every path
  (E0114). Unknown names are E0102/E0107 with did-you-mean suggestions.
- **Named args and defaults (S61, D-NARG1):** parameters may carry a
  default value (`fn f(x: Int =  0)`). A call-site label binds by NAME, so a
  call may skip a default and write its labelled arguments in any order
  (`f(x: 1)`). `/` closes the positional-only zone and `*` opens the
  label-only zone; `timeout seconds: Int` publishes `timeout` while the body
  reads `seconds`. Supplied expressions run left to right as written; unbound
  defaults then run in declaration order. The same law covers free functions,
  methods, constructors, generic calls and function values (D-APILABEL1=A).
  `jet fmt` preserves call-site labels as written (D-NARG2).
  A positional `Bool` parameter on a `pub` fn or `pub` method triggers the
  advisory L2401 lint.
- Definitions are unique (E0105), can't shadow built-ins (E0106), and
  unknown type names are E0119.

### Staged errors

Features that exist in the roadmap but not the language yet fail with an
error naming the milestone (see staged table in docs/spec/syntax-decisions.md).
A future feature must never die as a generic syntax error. Old Jet and foreign
syntax teaching is paused until post-Epoch 6 (D-S14-PAUSE); active docs and
fixtures use canonical syntax only.

## M2 — ownership (memory model v5, D-MEM1, done 2026-07-04)

Borrow-checker mechanics live in the transpiler; tier-1 users never write
Rust's `&`, `&mut`, `*`, or lifetime parameters. Two sigils, enforced (no
inference, no elevation):

| You write     | It means                       | Compiles to Rust |
|----------------|--------------------------------|-------------------|
| `fn f(x: T)`   | read (default; the only unmarked meaning) | `x: &T`   |
| `fn f(x: &T)`  | write — exclusive edit access   | `x: &mut T`       |
| `fn f(x: ^T)`  | take — ownership moves to callee | `x: T`          |

An unmarked parameter is **always** read — a body write to it, or handing it
to a `&`/`^` position, is a hard error at the definition (fix-it: add `&` at
the parameter and every call site). This is allocation-free for every
non-scalar shape, including strings, collections, structs, generic values, and
callbacks: the compiler borrows the existing value and never inserts a copy,
reference count, or allocation. Binding, returning, or storing an owned value
from that borrow requires an explicit copy or a `^` parameter. Call sites
mirror the parameter's sigil:

```jet
fn bump(n: &Int) { n += 1 }
fn archive(name: ^String) => String { return name }

fn run() {
    score: Int := 41
    bump(&score)                 // & mirrors &Int
    saved :: archive(^"vault")   // ^ mirrors ^String
}
```

(examples/features/memory/ownership.jet) Method receivers carry the sigil on
`self`; plain `self` is read; the sigil lives on the definition, not the call
site:

```jet
impl Player {
    fn show(self) => Int { return self.hp }                     // read receiver
    fn heal(&self, amount: Int) { self.hp = self.hp + amount }  // write receiver
}
```

```jet
p.heal(10)    // clean — the &self is on the method definition, not here
p.show()      // plain read receiver
```

A write through a read receiver is **E0205** ("write the receiver as
`&self`"); calling a `&self` method needs a changeable binding at the call
site (**E0202**, "does not have edit access (`&`)"). Using the same name
twice in one call while a `&` on it is active is **E0204** ("while something
is being changed, nobody else may be looking at it") — pass `&x` once, or
`~x` first.

**Named binding vs. temporary.** Passing a *named binding* to a `^` (take)
parameter without `^` is **E0209** — a hard error, never a silent clone (the
old `L0201` lint that auto-cloned is gone). A *temporary* — a literal,
`~x`, or a call result — passes freely with no `^`, since nothing survives
to be used after. `~x` (D-SHAPE-COPY1=A, supersedes D-CAP2) is the one copy
spelling — a real prefix expression, not a method: `.clone()` is not
user-typable Jet syntax (`clone` falls through to the ordinary "no such
method" error). The retired `copy x` word teaches **E0991**, pointing at
`~x`. `~` on a value Jet can't duplicate — a function, a trait value — is
**E0211**; on a scalar it's legal but redundant (already trivially
copyable).

```jet
name: String :: "vault"
saved :: ~name    // fresh, independent value; `name` still usable after
```

(examples/features/memory/copy_verb.jet)

### Named views, not raw references

Raw reference syntax is not first-class: `-> &T` return types, `&T` struct
fields, and `#Ref` provenance are not in the grammar. D-MEMPROVENANCE2=A
extends D-MEM-VIEWRET1: named `View<T>` / `ViewMut<T>` values can cross
returns and aggregates when sema proves a bounded set of receiver, parameter,
or static owner paths for each output slot. Every possible owner stays live
while the view is live. Lists, tuples, options, results, enums, named
aggregates, callbacks, and closed trait dispatch carry the same hidden
relation (see `examples/features/memory/returned_views.jet` and
`examples/features/memory/owner_backed_views.jet`). Temporary owners,
unbounded dynamic dispatch, and incompatible read/write paths remain
**E2305** (or **E2307** for string views). An ordinary owned field still owns
its value:

```jet
struct Span { text: String, meta: String }

fn describe(source: String, kind: String) {
    s: Span :: Span.{text: source, meta: kind}   // fields own their data
    print(s.text)
}
```

(examples/features/memory/ref_field.jet) When a program genuinely needs
"many owners, one value," reach for `Shared<T>` or `Pool<T>`/`Id<T>` (below)
instead of a raw stored reference. Fill `View<str>` only from
`.trim()` / `.after()` / `.before()` or a tracked string-view binding. A plain
owned `String` is not a borrowed window.

#### Place access (D-SHAPE-PLACE1=A)

A place is a name followed by its maximal field, index, or range projection.
Binding a bare place creates a checked read window; prefixing it with `&`
creates the exclusive write window; prefixing it with `~` makes independent
owned storage:

```jet
values := [10, 20, 30, 40]
read :: values[0..1]
edit :: &values[2..3]
copy :: ~values[0..1]
```

The two windows above are disjoint. Constant disjoint ranges and indexes lower
through a safe structural split, while different fields use Rust's native
field disjointness. Dynamic projections stay conservatively overlapping. Jet
never asks rustc to validate Jet semantics. A call or temporary is not a
place: bind it first (**E0213**). The retired `values.view(0..1)` spelling is
**E0214** and points at `values[0..1]`. Method calls never extend a place, so
`&values[0..1].sort()` applies write access to the maximal range and then calls
the method on that window.

#### Unified provenance and alias model (D-MEM1/S9, #649)

Sema keeps one fact graph for every borrowed window, independent of its runtime
representation. String windows, list `View<T>`, arena allocations, and existing
buffer or matrix window APIs enter the same graph. A type-specific side table
must not decide lifetime or alias safety.

An **owner** is identified by its declaration, not its spelling. A local owner
uses its definition identity, a public function owner uses its zero-based
parameter position, and a static owner uses its static declaration. Shadowing a
name therefore creates a different owner. A **place** is an owner plus an
ordered field, index, or range projection. A **window** names the part of that
place a view can observe. Reborrowing a view preserves the original owner and
appends projections; it never invents a new owner.

Each view fact records its place, read or write access, lexical extent, source
kind, and invalidation state. Read views may overlap. A write view is unique:
it may not overlap any live read or write view. Different known fields are
disjoint; ranges and indices overlap unless sema can prove otherwise. When in
doubt, sema treats places as overlapping.

Moving or replacing an owner, writing an overlapping place, or calling an
operation that may resize or relocate its storage is rejected while a view is
live (**E0212**). Arena reset or close invalidates its views; a later read is
**E0632**. A local fact ends after its last use or at lexical scope, whichever
comes first. At control-flow joins, invalidation on any reachable branch
survives; loops use the same conservative rule across iterations. Captures and
field projections preserve the fact rather than rebuilding it from a type name.
Tasks and channels reject a captured or returned view once as **E1102**.

D-MEMPROVENANCE2=A carries the same fact through public calls, returns,
aggregate fields and elements, generic instantiation, methods, function values,
lambdas, and trait dispatch. Each returned view slot is keyed by its full
output path. Its source relation is a bounded, deterministic set. Each member
names the receiver, a zero-based parameter, or static storage, followed by
field/index/range projections. Branches and compatible trait implementations
union their possible sources. Sema computes these maps to a deterministic fixed
point, so declaration and implementation order do not change the result.

All paths for one output slot must agree on read or write access. Open dynamic
dispatch without a proven contract, temporary owners, captured local owners,
and incompatible access paths are rejected as **E2305** (or **E2307** for
string views). Rebinding a stored view cannot replace its proven source
relation. Function types carry the same hidden relation; a generic callback
without a narrower declaration conservatively keeps every compatible
non-scalar argument live.

Public API snapshots publish each relation in canonical form. A single source
uses the compatibility-preserving `source;access:...;path:...`. A source union uses
`one_of(source;path:...,source;path:...);access:...`, sorted by stable source
identity. Adding, removing, or changing a possible source changes the API
digest and is reported as a breaking provenance change.

TIR receives only sema-approved provenance and lowering flags. It does not infer
owners, overlap, lifetimes, or escape safety. Codegen uses the approved relation to
emit a hidden Rust lifetime for `View<T>`/`ViewMut<T>` returns and containing
aggregates. Generated references are a representation of sema facts, never
their definition or a validation mechanism.

### Zero-copy string views

`String.trim()`/`.after(sep)`/`.before(sep)` bound to a local return a
zero-copy view into the receiver's own buffer, invisible in the local type
(`String` stays one Jet-level type end to end), whenever sema can prove the
binding can't outlive its owner:

```jet
padded := "  nate@jet.dev  "
email :: padded.trim()
domain :: email.after("@")
print("padded still readable: {padded}")   // reading the owner still works
```

(examples/features/memory/string_view.jet) A local view may chain another
`.trim()/.after()/.before()`, be interpolated (`"{domain}"`), be carried in a
view-typed aggregate, or be copied into an owned `String` with `~`. At a named
boundary, `View<str>` states the same owner-tied contract as `View<T>`: a
parameter- or receiver-rooted view may be returned or stored, with public
provenance inferred by sema. **E2307** reports a local or temporary owner that
cannot outlive the view, an unstable public source, or a use that requires an
owned `String`. See `examples/features/memory/returned_views.jet` for a
runtime-selected source, a multi-buffer parser, and a borrowing deserializer.
Either kind of view crossing a `tasks.spawn`/
`Sender.send` boundary is reported once, as **E1102** (unsendable value) —
a task or channel moves owned data between threads, and a view can't cross
without ownership.

### Escape hatches — `Shared<T>` and `Pool<T>`/`Id<T>`

`Shared<T>` and `Pool<T>`/`Id<T>` solve cross-scope and many-owner ownership.
They are distinct from provenance-carrying `View<T>`/`ViewMut<T>`, which model
owner-tied borrowed access.

**`Shared<T>`** (D-SHARED-API1) is a lock-guarded shared handle — "a
copyable door":

```jet
config :: Shared.new(AppConfig.{ name: "jet-server", hits: 0 })
t1 :: tasks.spawn(() => handle(1, config))   // no `take` needed
label :: config.read(c => c.name)
config.edit(c => { c.hits += 1 })
```

(examples/features/memory/shared_config.jet) `Shared.new(x)` infers `T` from
`x`; `.read(f)`/`.edit(f)` run a closure against a read- or write-locked view,
the lock scoped to the call only. Cloning `Shared<T>` is always a cheap
handle clone, never a deep copy of `T` — so it crosses a `tasks.spawn`
boundary with no `^`.

Expert code can hold the same lock across helper calls (D-SHAREDGUARD1=A,
D-SHAREDGUARD2=A):

```jet
space_ready :: Condition.new()
guard :: queue.guard_edit()
guard.wait(space_ready, q => q.jobs.len() < q.capacity) ?? panic("wait failed")
guard.value.jobs.push(job)
space_ready.notify_one()
```

`guard_read()` and `guard_edit()` return an owned `SharedGuard<T>`. The guard
releases on every exit. `.map(value => value.field)` narrows one guard to a
field. `.split(first, second)` creates two guards only when sema proves the
field paths are disjoint; both guards retain the original lock and provenance.
Guards are task-local and cannot be copied or sent.
The public `SharedGuard<T>` name is safe at helper boundaries: a normal
parameter reads it, while `&guard: SharedGuard<T>` requires and preserves edit
access. A returned or stored public guard keeps read access; perform edits at
the acquisition site or through an explicit write helper.

`Condition.new()` creates a wait set. `guard.wait(condition, predicate)`
requires an edit guard. It registers before release, reacquires the same lock,
and checks the predicate again. Cancellation unregisters the waiter before the
guard's final release. `notify_one()` wakes one waiter; `notify_all()` wakes
all waiters. Short `.read` and `.edit` closures remain the default.

See `examples/features/memory/shared_guard_queue.jet` for a bounded queue that
covers the notify-before-park race.

Inside a `#Transact` block (D-STM1), a `Shared<T>.edit` joins the block's
atomic commit instead of locking on its own line: every touched handle changes
together or not at all, and no other task ever sees a half-applied change. The
runtime defers each edit and, at the block's end, takes all the touched
handles' locks at once in a fixed order that cannot deadlock — the deadlock
class hand-ordered locking is famous for simply disappears. One marker, one
meaning (I8): the same `#Transact` that gives single-task rollback now spans
shared state.

```jet
fn transfer(from: Shared<Account>, to: Shared<Account>, amount: Int) {
    #Transact(tx) {
        from.edit(a => a.balance -= amount)  // both land, or neither
        to.edit(a => a.balance += amount)    // no lock order to get wrong
    }
}
```

(examples/features/memory/shared_transact.jet) A `Shared.edit` here yields
nothing — the write happens at commit — so its closure ends in a statement.
An irreversible effect (`Net`/`FS`/`Exec`) directly in the block is still
E0746: move it after the block or register it with `tx.on_commit(…)`.

**`Cell<T>`** (D-LOCALCELL1=A) is the local interior-mutation path. It lets a
read receiver update private state without an `Arc` or an operating-system
lock. `Cell.new(value)` infers `T`. Value methods are `get`, `set`, `replace`,
and `get_or_set` for `Cell<T?>`. Closure methods `read` and `edit` keep the
dynamic loan inside one call. `get` and `get_or_set` copy their result, so the
stored result type must support Jet's copy law. Use `read` when it does not.

`guard_read()` and `guard_edit()` keep a dynamic loan across calls. Any number
of read guards can coexist. An edit guard conflicts with every other guard.
A conflict stops at runtime with a `Cell borrow conflict` panic. Dropping a
guard releases its loan on normal return, early return, and panic unwind.
`guard.map(project)` keeps the same loan for one projected field.
`guard.split(first, second)` returns two projected guards that share the
original loan. Sema accepts direct field paths and proves the two edit paths
disjoint. The loan ends only after both guards drop.

Cell guards are temporary loan handles. A function can pass or return one
directly, and named tuples can contain guards recursively. This keeps mapped
and split guards useful across named helpers. A guard cannot be stored in a
user struct, enum, list, fixed list, map, `Option`, `Result`, `Shared`, another
`Cell`, a union, or a lambda. Keep it in a local name or tuple and use
`map` or `split` to project it.

`Cell<T>`, `CellReadGuard<T>`, and `CellEditGuard<T>` are local types. Sema
rejects them across task, task-group, channel, `Shared<T>`, and parallel
adapter boundaries. Use `Shared<T>` when state must cross one of these
boundaries.

**`Pool<T>`/`Id<T>`** (D-POOLID-API1) is a generational arena: every value
lives in one shared table, and other values point at it by `Id<T>` — plain
copyable, comparable index+generation data, never touching `T` itself:

```jet
world := Pool<Player>.new()
kai :: world.add(Player.{ name: "Kai", hp: 100, attack: 15, target: None })
world[kai].target = Val(rem)          // nested write through a real place
fallen :: world.remove(kai)           // T?, mirrors Map.remove
```

(examples/features/memory/entity_world.jet, entity_tree.jet) `pool[id]`
indexes for read and write; `.ids()` walks every live entry. A stale `Id<T>`
(its slot was removed) panics at runtime, mirroring the array-out-of-bounds
precedent (examples/features/memory/pool_stale_id.jet) — not a new
diagnostic code.

### Transitive memory facts

`no_alloc`, `zero_rc`, and `arena_bounded(N)` are explicit memory facts on the
D-MARK-SCOPE1 package/module/function/block ladder (D-MEM-FACTS1). Sema checks
every reachable call, including dependencies, against the effective inherited
facts. **E0921** identifies the incompatible source operation, prints the full
call path, and names the effective declaration plus its provenance. An
open-world dispatch must have a sealed target set or a signed dependency
summary; otherwise the strict fact is unprovable and rejected.

```jet
#Policy(no_alloc)

fn integrate(e: &Entity, dt: Float) { e.pos += e.vel * dt }
```

(examples/features/memory/no_alloc_policy.jet)

Card #644 owns the implementation migration from the shipped module-local
`no_alloc` denylist to this transitive contract.

`$name :: value` is the explicit compile-time-demand binding
(S57 / D-VERDICT-1308-1); ordinary foldable expressions need no marker.
`#Static $` emits a Rust `static`
when a stable address is required. `#Persist name := value` marks hot-reload
state on a bare binding (D-PERSIST1).

Aliasing rule, stated for humans: *while something is being changed, nobody
else may be looking at it.* Foreign `read`/`write` spellings are paused under
D-S14-PAUSE and get ordinary parse errors.

## Access capability sigils (D-MEM1)

The capability is a prefix sigil on the **type**, not the name. Two sigils
ship in v1 (unmarked read is the default, not a sigil):

| Sigil | Capability | Compiles to Rust |
|-------|-----------|-------------------|
| `T` (bare) | read — callee only reads; enforced, never elevated | `x: &T` |
| `&T` | write — exclusive edit access | `x: &mut T` |
| `^T` | take — ownership moves to callee | `x: T` |

`~` is the copy sigil (D-SHAPE-COPY1=A, below), not a parameter capability —
it has no arm in this table. Raw-pointer access (`p.*` postfix deref, prefix
`*x`) is a separate, `#Unsafe`-gated mechanism (D-CAP9) — also not a
parameter capability; the compiler's `AccessConvention` enum keeps dead
`Share`/`Raw` variants internally, inert until a future tier reactivates
them.

### Placement

Capability rides the type on the parameter:

```jet
fn damage(p: &Player, amount: Int) {   // &Player: write; Int: read (bare)
    p.hp = p.hp - amount
}
```

The call site mirrors the sigil — the capability is always visible where
mutation or movement happens:

```jet
damage(&p, 30)    // & mirrors the parameter's &Player
close(^file)      // ^ mirrors ^File — file is consumed
```

### Optional composition

A capability sigil composes with `?` (optional presence) directly: `&User?`
means "write access over an optional User", `^Texture?` means "take an
optional Texture". The sigil and `?` follow the same type-side grammar as
any other type annotation — the sigil is the parameter prefix, `?` is the
type suffix.

### E0029 — two capability markers

Placing more than one capability sigil on a single parameter is a parse error:

```
error[E0029]: two capability markers on one parameter
  --> file.jet:3:12
   |
 3 | fn bad(p: &^Player) { … }
   |           ^^ remove one capability marker
```

Access capabilities use sigils only: bare `T` (read), `&T` (write), `^T`
(take) — no fourth spelling.

## M3 — data & methods (done)

Structs and enums carry fields; methods attach behavior (S27). Ratified
surface (Group 2): struct literals **`Type.{f: v}`** (S29; flush, S29-FLUSH; dot-prefixed by D-DOTCTOR2); enums with
**`Type.Variant`** (S30); **`==` pattern tests** (S31); optional
**`T?`** with **`Val(v)`** / **`None`** (S32); generic args
**`Type<Args>`** (S33). `None` is only legal for `T?`, never plain `T`.
Fresh hidden-state construction uses `Type.new(…)`. Under D-SHAPE3a, the
receiver may be omitted as `.new(…)` when an expected type from a binding,
return, field, or call argument determines exactly one receiver. This is
ordinary expected-type elaboration, not a global constructor search.
Under D-SHAPE-OPAQUE-INFER1, `Type.new(…)` may likewise omit generic receiver
arguments when constructor inputs and the surrounding expected type force one
answer; otherwise write `Type<Args>.new(…)` explicitly.

```
struct Circle {
    radius: Float;

    fn area(self) => Float {
        return 3.14159 * radius * radius;
    }
}

impl Circle {
    fn unit() => Circle {
        return Circle.{ radius: 1.0 };
    }
}
```

- **`self`** is the receiver; prefix the type sigil (`^self`, `&self`) like any parameter (D-MEM1) — bare `self` is read.
- **Self-mutation (D-MUTSELF1):** inside a **`&self`** method the receiver may be
  changed in place — assign a field (`self.field = v`), update one (`self.field += v`,
  S17), or reassign the whole receiver (`self = New.{…}`). No new syntax (a `&`
  parameter is already a valid assignment LHS). The same write in a non-`&self`
  method (a read receiver) is **E0205**, pointed at the assignment with a "write
  the receiver as `&self`" fix. Calling a `&self` method needs a changeable
  receiver binding (`:=`), enforced at the call site by E0202.
- Invoke with **`c.area()`** (not `area(c)`).
- Methods may live **inside** the type, in **`impl Type { }`**, or as a top-level
  external inherent method **`fn Type.method(self, ...) { }`** (D-EXTMETH1) —
  same rules either way. The type must be defined in the current source module.
- Static methods omit `self` (e.g. `Circle.unit()`).
- **Named constructors (D-CTOR1):** multiple construction shapes = multiple
  distinctly-named no-`self` statics returning the type (`Point.cartesian`,
  `Point.polar`). Overloading is rejected; a duplicate name is E0105 with
  a teaching message pointing at constructor naming.
- Enum `if subject == { … }` arms must be exhaustive; missing cases are a compile error.
- **Traits (S28, M9):** `trait Name { fn sig(self) => T; … }` — signatures
  only. Implement inside a type (`impl Trait { … }`) or outside as
  `impl Type.Trait { … }` (qualify foreign types: `impl other.Point.Shape`).
  A trait name in type position (`[Shape]`, `fn f(s: Shape)`) means
  dynamic dispatch with invisible boxing. Generic params: `fn f<T: Bound>(…)`
  and `struct Pair<T> { … }`. Built-in traits follow S55:
  `Printable`/`Equatable`/`Debug` auto-derive whenever every field qualifies.
  The package default is on; `policy: .{ auto_derive: false }` disables silent
  generation. A signed type marker opts one trait in or out (`#Debug`,
  `#!Debug`), and a hand-written implementation wins (D-AUTODERIVE1=E,
  D-AUTODERIVE-SYNTAX1=D). Other explicit derives are `#Comparable`, `#Codable`,
  `#Encode`, `#Decode`.
- **Encoding traits (D-SERDE2/D-SERDE16):** `Encode.encode(self) => DataTree`
  and `Decode.decode(tree: DataTree) => Self ? [FieldError]` are ordinary Jet
  trait methods. `DataTree.decode<T>()` is the one public typed-dispatch path;
  primitive, container, generated, and hand-written implementations all use it.
  Built-in derives generate Jet source fragments beside the marked type, then
  run those fragments through the normal parser, sema, TIR, and codegen pipeline.
  A user-defined derive may expand only when its provider or target type is
  entry-local; otherwise E2711 points at the derive marker.
- **Accumulated validation (D-VALIDATE1, card #506):** a `validate { … }`
  section in a struct body declares rules as `check(cond, at: field, "msg")`
  statements; `field` is a bare sibling-field reference (D-FIELDPOL1). Every
  failing `check` accumulates into `[FieldError]` (`{ path, reason }`) instead
  of failing fast. Sema requires each rule
  statement be exactly this shape (E0353), `at:` to name a real field
  (E0354), and purity-checks the whole synthesized function (S60/E3401) —
  a rule may reference only sibling fields and pure calls. `Type.validate(value)`
  runs the block standalone, returning `value ? [FieldError]`. Derived struct
  decoders now pass a successfully shaped value through that validator, so
  shape and rule failures share one list. Hand-written codecs still opt into
  validation explicitly. The `Validate.over(s)` use-site escape for rules
  needing outside context remains a separate framework slice. The contract
  ruling is recorded as `D-VALIDATE-DECODE1=B`.
- **Tags (D-QUAL2, D-TAG-SURFACE1):** `tag Name { deny: [Net] }` declares an
  erased dataflow fact and its policy. `deny` is required and nonempty; `from`
  is optional. Direct `#Name` tags attach to values, fields, parameters, and
  returns. `#Scrub(Name)` removes exactly that tag. A tag carries no methods, so
  declaring one in a tag body is **E0732**, and using a tag where dispatch or
  method attachment is expected — `derive`d, or implemented/used as a trait —
  is **E0731** (fix-it: declare it as a `trait`). All tags are PascalCase
  (D-CASING1). Prelude declares `Input`, `PII`, `Secret`, and `Credential`.
- **Applied rules (D-SHAPE2/D-ATTR2):** `#Rule` or `#[A, B]` on the
  line before a declaration. Block markers use PascalCase and parenthesized
  arguments when arguments exist. An explicit empty effect row is `=[]=>`;
  compile-time demand is the prefix marker `$`.
- **Statement switch attributes (D-CANVASSTATE1):** `#Off <stmt>` parses and
  type-checks one statement, including block-shaped statements, then emits no
  code in every build. `#DebugOnly <stmt>` parses and type-checks the statement
  in every build, emits only in debug/dev builds, and strips from release output.
  Names introduced inside either marker are scoped to that marker body.
  `build.profile` is not a user-typeable comptime value.
- **Canvas metadata (D-CANVASMETA1):** `#Meta(category: "Movement", tunable)`
  attaches checked tooling facts to bindings, top-level consts, and functions.
  `category` must be a non-empty plain string literal; `tunable` is a bare flag.
  The marker emits no code and changes no runtime behavior.
- **OS-target gating & dispatch (D-OSTARGET1/D-OSTARGET2):** `#Target(OS.Linux
  |MacOS|Windows)` gates one `impl` block to a native OS; `jet build
  --target=<triple>` emits only the matching build's impls (host OS by default).
  Ungated code reaches the surviving impl through the compile-time switch
  **`$if build.os == { .Linux -> … .MacOS -> … .Windows -> … [else -> …]
  }`** — `build.os` is a compiler-known comptime value, the switch folds to the
  arm matching the build's target OS and discards the rest before any gating
  check runs. Arms must cover every OS or carry an `else`
  (**E-OSTARGET-DISPATCH-EXHAUSTIVE**); the subject must be `build.os`
  (**E-OSTARGET-BUILD-CONTEXT**); arm heads are bare OS variants
  (**E-OSTARGET-DISPATCH-ARM**). See syntax-decisions.md → D-OSTARGET2 for the
  full rules.
- **Build-time embedding (D-CTIO1/D-CTFIND1/2):** inside a `$` binding,
  **`embed_file("path") => String`** bakes a file's UTF-8 text into the binary
  and **`embed_bytes("path") => [U8]`** bakes its raw bytes (binary-safe, no
  UTF-8 requirement — images, fonts, any blob). **`find("glob") => [String]`**
  returns sorted relative file paths for a std-only glob (`*`, `**`, `?`,
  `{a,b}`, `[a-z]`). These are the *only* sanctioned build-time I/O; comptime is
  otherwise pure (**E3401** — D-META-EFFECT1 c3: one call-graph purity walk
  shared with the run-time `=[]=>` check; retires the former E0951). Paths/globs must be string literals resolved
  relative to the embedding file's directory, never absolute and never escaping
  the project via `..` (**E0957**). A missing or unreadable embedded file is
  **E0955**; for `embed_file`, a non-UTF-8 file is also **E0955**, with a fix
  pointing at `embed_bytes`. Every embedded file and every file matched by
  `find` records its sha256 in `.jet/lock`.
- **Published schema migrations (D-MIGRATE1/D-MIGRATE2):** `#PublishedSchema struct
  Name { ... }` marks a public record whose field layout is snapshotted at release
  under `.jet/cache/schema/`. On later project builds, sema compares the current
  shape to the saved snapshot (keyed by field name, so order is ignored). A
  breaking data-shape change is refused — **E0910** — unless a `migration` op
  declares the intent. The four ops:

  ```jet
  migration UserRecord {
      rename name => display_name              // D-MIGRATE1: field renamed (same type)
      remove legacy_id                         // D-MIGRATE2D: field deleted
      add verified: Bool =  false               // D-MIGRATE2A: new field + default for old data
      change price: Int => Usd via { c => Usd.from_int(c) } // D-MIGRATE2E: type change + converter
  }
  ```

  - `rename` must target an existing field with the same type.
  - `change f: Old => New` resolves its converter in order (D-MIGRATE2B): the inline
    `via { … }`, else an `impl Old => New` in scope (the D-ERR-CONV surface), else
    E0910 asking for one. The `via` body is single- or multi-line and reuses the
    callable arrow and lambda grammar.
  - `add f: T =  default` supplies the value old records (written before the field
    existed) are read with. A field is only "added" if absent from the snapshot.
  - There is **no `reorder` verb** (D-MIGRATE2F): reordering is never a breaking
    change and needs no op (writing `reorder` teaches E0911).
  - `drop` is not a verb (use `remove`); both `drop` and `reorder` are taught back
    via **E0911**, as is any other unknown verb.

  A declared op that contradicts the real shape (e.g. `remove f` where `f` still
  exists, `add f` where `f` already existed, a `change` whose from/to types don't
  match) is itself an E0910-family teaching error. E0910 checks *intent*; the
  runtime data conversion is the D-MIGRATE4 chain below.
  Single-file runs accept the marker but only enforce the check when a project
  snapshot exists.

  **`jet inspect schema` (D-MIGRATE2C):** `jet inspect schema status` lists every snapshotted
  `#PublishedSchema` type with its pinned published version and fields, flagging any
  type that has a pending breaking change vs its snapshot (reusing the E0910 diff).
  `jet inspect schema squash --before <ver>` re-baselines: it rewrites each snapshot to the
  *current* struct shape and records `squashed_before = <ver>`, so future builds
  treat the current shape as the authoritative baseline and migration blocks for
  versions before `<ver>` are no longer required (delete the now-stale blocks). It
  edits only `.jet/cache/schema/`, never user source. There is **no `jet inspect schema
  check` verb** — `jet build`'s E0910 is already the CI gate.

  **Decode-time migration transparency (D-MIGRATE3=A):** `decode_traced<T>(raw)
  => DecodeResult<T> ?` sits beside `decode<T>` on every codec that shares this
  decode machinery (json/csv/toml/yaml, D-ENC1). `DecodeResult<T>` is `{ value:
  T, migration: MigrationStatus }`; `MigrationStatus` carries `.migrated: Bool`,
  `.from` (the source shape's version label), and `.steps` (one entry per
  migration step applied, `"v1->v2"` style). `decode` itself is unchanged —
  same call, same cost, for anyone not asking (I8). `.migrated` is `false` and
  `.from`/`.steps` are empty for a plain type and for a `#PublishedSchema`
  type decoding data already shaped like the current struct.

  ```jet
  r    :: json.decode_traced<UserRecord>(raw)?
  user :: r.value
  if r.migration.migrated {
      log.info("record {user.id} arrived as schema {r.migration.from}")
  }
  ```

  **Runtime migration chain (D-MIGRATE4=A):** decoding a concrete
  `#PublishedSchema` type that derives `Decode` and has `migration { }` blocks
  runs the chain. The blocks, in source order, are the steps: with `K` blocks
  the historical shapes are `v1` (oldest) … `vK`, and the current struct is
  `v(K+1)`; each historical shape's field set is derived at compile time by
  inverting the ops (`add` ⇒ absent before, `remove` ⇒ present before,
  `rename a => b` means `a` before, while `change` means no field-set difference). At decode
  time:

  1. **Current shape first** — the ordinary decode is tried as-is. Success is
     the fresh case (`migrated: false`). This is also the ambiguity rule:
     *prefer the newest matching version*, so data that satisfies the current
     shape never migrates.
  2. **Shape detection** — on failure, the data's top-level field-name set
     (wire keys, after any `#Rename`/`#RenameAll` treatment) is compared
     against the historical shapes, newest (`vK`) to oldest (`v1`); the first
     match wins.
  3. **Walk forward** — the matched shape's data is rewritten step by step,
     oldest-matching → current: `rename` moves a key, `remove` drops one,
     `add` evaluates its default expression and fills the field, `change`
     decodes the old field type, runs the `via { … }` converter (or the
     `impl Old => New` conversion, D-MIGRATE2B), and re-encodes the result.
     Converter bodies and `add` defaults are ordinary Jet expressions,
     type-checked and lowered through the normal pipeline. The rewritten data
     then decodes as the current shape.
  4. **No match** — the ordinary decode error is returned unchanged (garbage
     stays garbage).

  Plain `decode` applies the same chain silently; `decode_traced` reports it
  (`from: "v1"`, `steps: ["v1->v2", "v2->v3"]`, …). Version labels are
  positional — `v1` is the oldest shape the blocks describe. Types without
  migration blocks pay nothing: no step functions and no per-type chain code
  are emitted, and their decode path is unchanged. CSV applies the chain per
  row (an old-header file migrates every row; the batch-level status reports
  the first migrated row).

- **Struct layout control (D-REPRC1):** `#Layout(c)` before a struct stamps
  `#[repr(C)]` on the generated Rust struct, enabling direct C-FFI pointer
  sharing. Field order is preserved as written. Growable fields (`[T]`, `[K: V]`,
  `String`) are rejected with **E1104** because they lack a stable C layout;
  fixed-size arrays `[T#N]` are allowed. Reserved variants (`packed`, `align(N)`,
  `columnar`) parse but error with **E1105** until their milestones ship.

## M4 — errors as values (done)

Fallible functions return **`T ? E`** (S34): `T` is the success payload,
`E` is any enum, struct, `String`, or the default **`Err`** type. Omitting
the error side in a function return — **`T ?`** — means **`T ? Err`**.
Build outcomes with **`Ok(v)`** and **`Err(e)`**; test them with
**`== .Ok(n)`** / **`== .Err(e)`** (same pattern machinery as M3 optionals).
Cross-type **`?`** conversion supports two forms:
- **`Fallible`** trait (D-ERR2): `impl MyFail.Fallible { fn to_error(self) => Err { Err(str(self)) } }` — converts a typed error to the default `Err`. Prelude types implement `Fallible` by default.
- **Declared typed conversion** (D-ERR-CONV): `impl Source => Target { Target.Variant(self) }` — converts a `Source` error into a typed `Target` error; `?` applies it automatically. Declared once per (Source, Target) pair; rejected unless declared (orphan rule S28 applies). `E2404` fires when `?` would need an undeclared conversion; `E2405` fires on duplicate declarations; `E2406` fires on orphan-rule violations.

- Postfix **`?`** (S7) propagates: unwraps `ok`, early-returns `err`. The
  enclosing function must return a compatible fallible type. On **`T?`**,
  `?` propagates `None` when the function returns an optional.
- Return types follow **D-RESULT-OPTION-CANON1** like every other type
  position: tight **`T?`** is Optional; spaced **`T ?`** / **`T ? E`** is
  fallible. Parentheses (`=> (T?)`) remain legal grouping, not required.
- **`?? <expr>`** (S35/S71) is the fallback operator on a fallible value or
  optional: yields the success payload or evaluates the right side. Precedence is
  looser than **`&&`** / **`||`**, so `a? ?? b` and `x == 1 || y ?? 0`
  parse predictably. The right side may be a value, **`return`**, **`return expr`**,
  or **`panic(…)`**. The retired word **`or`** is paused under D-S14-PAUSE and
  gets an ordinary parse error.
- **`panic("msg")`** and **`require(cond)`** / **`require(cond, "msg")`**
  (S36) stop the program with a friendly report on stderr and exit code 70.
- In **`if <fallible-expr> { … }`**, when the subject is not a plain
  name, **`it`** names the subject for pattern arms like **`it == .Ok(n)`**.
- **`fn run()`** may stay bare for beginner programs. Use
  **`fn run() => () ?`** only when the entry itself propagates errors with
  **`?`**; returned errors print and exit non-zero.

Unchecked fallible values (**E0401**), ignored fallible calls (**E0402**),
ignored **`#MustUse`** results (**E0419**), bad propagation (**E0403**),
`ok`/`err` outside a result context (**E0404**), and fallback type mismatches
(**E0405**) are compile errors with fixes that name **`?`**, **`??`**, pattern
tests, binding, and **`.drop("reason")`** — the sole intentional-discard
spelling (D-IGNORERET2, amended by D-MARK-DISCARD1=A: the `#Suppress(MustUse)
{ … }` lexical-scope form is retired).

## M6 phase 1 — `jet fmt` (done)

**`jet fmt <file.jet>`** rewrites the file in place to canonical Jet style
(S44). **`jet fmt --dry-run <file>`** prints a unified diff and writes nothing.
**`jet fmt --check <file>`** reports changed files and exits **1** when the
file would change (CI mode). Formatting is lex → parse → print;
sema and rustc are not run.

Style (zero configuration): 4-space indent, `{` on the same line as its
header, one statement per line, at most one blank line between top-level
items, spaces around binary operators, no space before `;`/`,`/call `(`,
trailing `;` on statements (S6). General line width is not enforced in v1;
long multi-clause loop headers wrap only after their canonical semicolons.

`//` and `/* … */` comments are preserved and re-attached by source span. Real
parse errors still block fmt. The typed `package.jet`/Config formatter is a
separate closed-record path: when it sees comments, it fails closed until it
owns their placement rather than reporting the source as clean unchanged.

Idempotence: **`fmt(fmt(x)) == fmt(x)`** on every `examples/*.jet` and
`tests/ui/*.fixed.jet` (`tests/fmt.rs`).

## M6 phase 2 — `jet test` + `jet new` (done)

**`#Test("name") { … }`** (S43, D-CASING1 follow-on) — top-level blocks only.
Bodies parse like a parameterless function; use **`require(cond)`** /
**`require(cond, "msg")`** and **`require_eq(a, b)`** (S36) for checks. Duplicate
test names → **E0105**; a nested `#Test` block → **E0601**; bare `test "name"` is
paused under D-S14-PAUSE and gets an ordinary parse error. **`jet run`** / **`jet build`** ignore test
blocks; only **`jet test`** compiles and runs them.

**`jet test <file.jet>`** (or a directory of `*.jet` files) builds one harness
binary per file (no cargo project; R9). Each test runs in isolation; failures
use a generated unwind boundary (not observable in user code). Output is one
line per test (`name: pass` / `name: FAIL`), a shared summary (`N passed, M
failed, K skipped`), and exit **1** when any test fails. Failed assertions print
the registered `Stop [E3001]` report with the Jet source location; equality
checks say `expected …, got …`.

**Scope members (D-DOTSCOPE1)** — inside a `#Test { … }` body, a
statement-position `.name { … }` / `.name(args) { … }` resolves against the
marker's declared vocabulary (`Syntax::scope_members`). This is the one spelling
for scope vocabulary (I8); the parser/sema shape is generic (a marker→members
table), so other markers can grow members later without new grammar. `#Test`
declares four; `#Bench` (and every other block marker) declares none, so a member
there is **E0614**. A member outside any member-declaring marker is **E0615**;
an unknown member **E0614** (lists the vocabulary); a wrong argument shape
**E0617**; a member nested instead of a top-level statement of the block
**E0618**. Members only *run* under **`jet test`** — `jet run`/`jet build` ignore
`#Test` blocks entirely (unchanged); a malformed member is still reported in any
mode (structural check).

- **`.setup { … }`** — must be the first statement (**E0616** otherwise). Its
  statements are spliced inline and run first; a failure inside fails the test on
  the normal path. It does **not** open a new scope — bindings made in `.setup`
  are visible to the rest of the test body (init sugar).
- **`.expect_fail { … }`** — the region *must* fail (a `require` failure or a
  panic). It runs under the harness's panic-catching boundary with a silenced
  hook; if it completes cleanly the **test** fails with `expected this region to
  fail, but it passed`. If it fails, execution continues after the region and the
  test can still pass.
- **`.timeout(<dur>) { … }`** — the region must complete within the duration or
  the test fails with a `timeout: region took …` message. v1 ships **post-hoc**
  semantics: the region runs to completion, then its elapsed time is compared
  against the budget (it does not interrupt a hang — out of scope). `<dur>` is a
  bare duration literal (`ns`/`us`/`ms`/`s`); it needs no `#UnitFamily` in scope.
- **`.skip { … }` / `.skip("reason") { … }`** — the region is **not executed**
  (emitted as a dead `if false` block, so it still type-checks). When `.skip` is
  the **first** statement the whole test is skipped: it reports `name: skip` and
  the shared summary reports its skipped count. A `.skip` later in the body skips
  only that region; the rest of the test still runs.

**`jet new <name>`** creates `<name>/run.jet` with a zero-argument `fn run()`
(hello world), plus `<name>/package.jet` and `<name>/.gitignore` (`build/`).

Example: `examples/features/tooling/tests.jet`; scope members in
`examples/features/tooling/test_members.jet`. Goldens: `examples/features/expected/20_tests.test.out`,
`tests/jet_test.rs`, `tests/fixtures/test_fail.jet` + `.fixed.jet`, and the
`scope_*` fixtures for the member fail paths.

## `#Bench` region benchmarks + perf timing (c121, D-BENCH1) — done

**`#Bench("name") { … }`** (D-BENCH1, D-BENCH-MARKER1) is the exact sibling of `#Test`: a
top-level block whose body parses like a parameterless function (and may use
`require`/`require_eq`). **`jet run`** / **`jet build`** ignore bench blocks. A
file with `#Bench` blocks runs per-region under **`jet bench`** — each region's
body is warmed, its iteration count auto-scaled to ≥1ms, sampled, and reported
as `name  <ns> ns/iter (±sd)  <ops> ops/sec`. A file with no `#Bench` blocks
keeps whole-program `jet bench` timing (5 warmup + 20 trials). The body call is
`black_box`'d so the optimizer can't elide it. Example:
`examples/features/tooling/bench.jet`; golden `examples/features/expected/105_bench.out`
(the `jet run` `main` output) + structural check in `tests/jet_test.rs`.

**Compiler phase timing** — set **`JET_TIMING=1`** and any build writes
`jet-timing.json` (load/sema/ffi/codegen µs + generated-Rust bytes), prints
`jet-timing binary_bytes=…` after link, and the LSP appends per-request latency
to `jet-lsp-timing.json`. All gated by the env var (zero cost otherwise; I6
hand-rolled JSON, no external crate). **`tools/perf/dashboard.sh`** aggregates a
table across representative programs; **`tools/perf/ci-perf-check.sh`** gates
against the committed **`tools/perf/baseline.json`** (sema time + binary size,
15% threshold); **`tools/perf/update-baseline.sh`** refreshes it.

**NixOS / flake:** `nix develop` provides `cargo`, `rustc`, `gcc`, `nodejs`,
and a **`jet`** wrapper around `target/debug/jet`. **`cargo build`** once, then
`jet run …` / `jet self lsp` / `cargo test --test lsp`. Editor setup:
`editors/vscode/README.md`. Release binary: `nix build .#jet`.

## Unified FFI frame (D-FFI-UNIFY1)

Every foreign ecosystem mounts through one model: a language root plus library
name, `<lang>.<lib>`, with generated bindings under `.jet/bindings/<lang>/`.
C, C++, and JS are active namespace binders. C uses the namespace surface
(`use c.<lib>` / `#Extern module c.<lib>`). C++ uses `use cpp.<lib>` over a
clang-AST-derived, content-addressed C-ABI shim: namespaces are selected
explicitly, public scalar classes become owned opaque handles, exceptions become
`T ? CppError`, pure named callbacks keep their checked C ABI, and template
instantiations are requested on demand. `jet inspect bind cpp` requires the
selected target and absolute clang/archiver paths; include/library search paths
and link libraries are audited in binding provenance and reused at final link.
JS uses one `use js.<lib>` surface;
the host is target-dispatched, with browser JS on web targets and the native
JS-on-WASM host on native targets. Generated JS binding caches live under
`.jet/bindings/js/`: `<lib>.jet` carries the callable Jet surface and
`<lib>.d.ts` records the TypeScript declaration provenance. Rust keeps the shipped
`extern rust "crate@version" { ... }` declaration block as its active binder
surface until the `rust.*` namespace migrates. Python and Swift roots are
registered for their ratified binders; Swift's planned route is a typed bridge
over generated C-ABI shims.

The inline fourth tier is also implemented. `#FFI(c|cpp) fn` carries one exact
triple-quoted raw body whose Jet signature remains the checked contract.
`#FFI(asm) fn` is available only inside an audited `#Unsafe("reason")` region
with `use core.mem`; its named operands, return anchor, clobbers, and selected
target are checked before lowering. These native boundaries are not resident-JIT
code: the JIT reports the foreign boundary by name, while native build/run owns
execution and link proof.

## M7 — Rust FFI (`extern rust`, done)

**`extern rust "crate@version" { … }`** (S50) declares foreign functions. Each
entry is a normal Jet signature plus **`= "rust::path"`** naming the target
item. This source-level declaration is sufficient even inside a project with
`package.jet`; users do not need the package manager just to call a foreign
function. **`extern rust "std" { … }`** works for Rust standard-library items with
no extra dependency. Non-`core` crates require an exact version pin (**E0701**).

Allowed boundary types pass **by value**: `Int`, `Float`, `Bool`, `String`,
`Char`, `[T]`/`[K: V]`/`T?`/`T ? E` built from allowed types, and
structs/enums whose fields are allowed. No borrowed parameters or returns, no
callbacks (**E0702**).

When any crate dependency is needed, the driver builds a hidden cached cargo
bridge under `~/.cache/jet/ffi/` and links it into the generated program (R9:
the user's folder never grows a manifest). Missing **`cargo`** → **E0703**;
fetch/build failures → **E0704** (cargo output in an indented block); a wrong
foreign path or signature → **E0705**. Panics inside foreign code are caught
at the boundary and become the M4 runtime report (exit 70).

Teaching: **`unsafe`** / C-style FFI spellings → **`extern rust`** (**E0031**).

Example: `examples/features/lowlevel/ffi.jet` (`base64@0.22`). Ui: `tests/ui/ffi_*.jet`.
Integration: `tests/ffi.rs` (gated on `cargo`).

## E2-M14 — C FFI (implemented: overlay + merge + link + bind backend)

**S59** — C import with auto-generated bindings (default) and optional user
overlay. (Full spec follows in this section.)

| Layer | Shape |
|---|---|
| Autogen | `#Bindgen module c.<lib>.__bindgen__ { … }` in `.jet/bindings/c/<lib>.jet` |
| Overlay | `#Extern module c.<lib> { … }` — empty `{ }` = no overrides |
| Call site | `use "header.h" as alias` or `use c.<lib> as alias` (one per lib per file) |

Function bodies mirror Rust FFI: `fn init_window(w: Int, h: Int, t: String) = 
"InitWindow";` (the string is the C linker symbol). On any C `use`, the compiler
loads the bindgen cache at `.jet/bindings/c/<lib>.jet` (when present), merges the
user overlay over it (**effective module = bindgen ∪ overlay; overlay wins**;
incompatible re-declaration → **E3205**), and materializes one synthetic module
so calls resolve like any namespaced module call. Codegen emits an `extern "C"`
block plus small per-function wrappers (the only place compiler-vetted `unsafe`
is emitted, S58); `String`↔`*const c_char` and `Char`↔`u32` convert at the edge.
For a C function declared to return `String`, the pointer is borrowed from C:
it must be non-null, NUL-terminated, and valid UTF-8. Jet copies it immediately
into an owned `String` and never frees the C pointer. Null and invalid UTF-8 are
runtime boundary failures; neither becomes an empty or lossy string. APIs with
owned buffers, nullable strings, another encoding, or a library-specific free
function stay raw and need an audited wrapper.

Link key = last segment `<lib>`: a declared `<lib>: c@…` dep in the `deps:`
block of `package.jet` (`c@system` → pkg-config with a bare `-l <lib>` fallback;
`c@"path"` → local `-L`/`-I`/`-l`) → else `pkg-config <lib>` → **E3201**. Link flags (`-L native=…`,
`-l <lib>`) are resolved at **build time** (not during front-end checking, I3) and
threaded into the `rustc` link line. By-value scalars/`String`/C-layout
structs+enums at the edge; aggregates (`[T]`, maps, `T?`, tuples, …) → **E3203**.
D-CABI-RESULT1 keeps status-plus-out APIs raw: a parameter may be `*T` only
when `T` is C-safe, and every call is an unsafe-function call requiring an
audited `#Unsafe("reason")` region. The caller creates the non-null pointer
through `core.mem`, initializes its slot, checks the raw status, and reads the
slot only on a status the wrapper knows initialized it. Pointer returns remain
**E3202**; direct `Result` declarations remain **E3203**. `#Bindgen` is legal only inside a
generated cache file (**E3207**); users may not name the reserved `__bindgen__`
segment (**E3206**); two `use` forms for one lib in one file → **E3204**.

`jet inspect bind <header.h> --pkg <lib>` is the manual cache-refresh entry point and
shares the compile-time auto-bind backend (owner 2026-06-18: native std-only
implementation, D-CBIND3 superseded). It parses C function prototypes over the
bindable type subset (scalars, `char*` strings, `void`) and emits a `#Bindgen`
cache; declarations it cannot map are skipped and reported rather than faked
(I3). **E3208** fires only when the header cannot be read or contains no
bindable prototypes — the fix is a hand-written `#Extern module c.<lib>` overlay
for those declarations. Rust FFI (S50) is unchanged. Diagnostics:
**E3201–E3208** in diagnostics.md with snapshots (front-end ones under
`tests/ui/cffi_*`; link-time/gated ones pinned in `tests/cffi.rs`).

## E3 — Go project binder (D-FFI-GO1=A, scalar + handle surface implemented)

`jet inspect bind go <source.go> --pkg <lib>` finds cgo `//export Name`
functions whose parameters and optional result are `int64`, `float64`, or
`uintptr`, runs
the provisioned Go compiler with `go build -buildmode=c-archive`, and writes
the archive plus a typed `.jet/bindings/go/<lib>.jet` cache. Programs import it
with `use go.<lib> as alias`; calls execute in-process through the shared C ABI
linker, so the Go runtime is part of the native program rather than a sidecar.
`uintptr` maps to a private-field, move-only `go.<lib>.Handle`; passing it to a
foreign function consumes it, preventing Jet from reusing a released
`runtime/cgo.Handle`. The binder accepts handles only on a 64-bit host ABI and
supervises compilation with a 60-second deadline plus bounded diagnostic
capture. Calls through generated `go.*` caches contribute the `Go` effect root;
ordinary C externs remain maximally effectful. Unsupported signatures fail before compilation. Go compiler failures
are laundered through **E3208** and never expose raw foreign source frames
(I2/I4).

Example: `examples/features/lowlevel/polyglot_go/`.

## E3 — Fortran project binder (D-FFI-FORTRAN1=A, checked ISO_C_BINDING vertical)

`jet inspect bind fortran <source.f90> --pkg <lib>` discovers explicit
`bind(C, name="...")` functions and compiles them with the provisioned
`gfortran` toolchain. Scalar `integer(c_int64_t)` and `real(c_double)` inputs
must use `value`. Fixed-shape input arrays of those elements must use
`intent(in)` and map to flat `[Int]` or `[Float]` values in Fortran
column-major order. The generated public wrapper records every extent and
rejects a list whose length does not exactly match the shape before passing its
pointer across the private C ABI seam. Generated `fortran.*` calls contribute
the `Fortran` effect root. Unsupported declarations and compiler failures are
laundered through **E3208** rather than exposing `gfortran` diagnostics.

Example: `examples/features/lowlevel/polyglot_fortran/`.

## E3 — COBOL project binder (D-FFI-COBOL1=A, GnuCOBOL C-ABI vertical)

`jet inspect bind cobol <program.cob> --copybook <record.cpy> --pkg <lib>`
compiles one linkage program with provisioned GnuCOBOL and writes a
`cobol.<lib>` cache. The copybook subset is closed: one level-01 record with
level-05 fixed text, COMP-5 integers, and COMP-3 packed decimals. The binder
records exact offsets and widths, emits an `#Codable` Jet record, and maps every
COMP-3 field to `Decimal`, never `Float`. Its callable C bridge accepts packed
decimal values as scaled minor-unit `Int` values, initializes `libcob` once,
and invokes the exported `int PROGRAM(cob_u8_t*)` entry in-process. Generated
tools have 60-second deadlines and 64 KiB capture ceilings. Unknown layouts and
laundered foreign-tool failures use **E3208**.

## E3 — JVM project binder (D-FFI-JVM1=A, embedded class vertical)

`jet inspect bind java <source.java> --pkg <lib>` uses the provisioned OpenJDK
toolchain to compile bytecode and discovers public JVM descriptors through
`javap -s`. Supported constructors and non-overloaded methods use `long` and
`double`; unsupported descriptors fail binding rather than guessing an ABI.
The generated cache links a std-only JNI bridge against the provisioned
`libjvm`. It creates one JVM lazily inside the native Jet process, attaches
calling threads, and destroys the JVM at process teardown.

Java objects cross as opaque `java.<lib>.Handle` values backed by a bounded
1,024-slot global-reference table. Calls borrow the handle; `close(^handle)`
consumes Jet ownership and releases the global reference. Remaining references
are released during JVM teardown. Constructors and value-returning methods are
fallible with `JavaError.Exception`; the bridge clears the Java exception and
returns only the typed Jet error, never a Java stack or foreign source frame.
Generated calls carry the `Java` effect root. `javac`, `javap`, `cc`, and `ar`
run under a 60-second deadline with 64-KiB diagnostic capture. Cache provenance
binds the source, discovered bytecode surface, class cache path, and schema with
SHA-256. Tool failures use **E3208** what/why/fix copy.

Example: `examples/features/lowlevel/polyglot_java/`.

## E3 — .NET project binder (D-FFI-DOTNET1=A, embedded class vertical)

`jet inspect bind cs <source.cs> --pkg <lib>` compiles the source with the
provisioned .NET 8 SDK and discovers its public API through managed reflection.
One public class, one constructor, and non-overloaded methods using `long` and
`double` project into a typed `cs.<lib>` module. Unsupported types and overloads
fail binding rather than guessing an ABI.

The generated native archive embeds CoreCLR through
`hostfxr_initialize_for_runtime_config` and
`load_assembly_and_get_function_pointer`. Generated managed entry points use
`[UnmanagedCallersOnly]`; no worker process, file protocol, or environment
transport participates in calls. Instances cross as opaque move-only `Handle`
values backed by a 1,024-slot generation-checked `GCHandle` table.
`close(^handle)` deterministically releases the managed root. Exhaustion becomes
`DotNetError.ResourceLimit`; managed exceptions become
`DotNetError.Exception`; invalid, stale, or released handles become
`DotNetError.InvalidHandle`, with foreign exception text never exposed. Calls
carry the `DotNet` effect root. SDK, C compiler, and
archive tools run under a 60-second deadline with 64-KiB output capture.
Provenance binds source, reflected surface, hostfxr identity, and schema with
SHA-256. Tool failures use the snapshotted **E3208** diagnostic.

Example: `examples/features/lowlevel/polyglot_dotnet/`.

## E3 — Tcl project binder (D-FFI-TCL1=A, live-session vertical)

`jet inspect bind tcl <script.tcl> --pkg <lib>` compiles a std-only C bridge
against the Nix-provisioned Tcl headers and shared runtime, then writes a typed
`tcl.<lib>` cache. `open()` creates an in-process interpreter and evaluates the
script once as session initialization. Later `eval`, `eval_int`, and
`eval_float` calls share its variables and procedures. `eval_once` uses a fresh
interpreter and destroys it after one call.

`Session` is opaque and thread-affine. A bounded 64-slot table owns every live
interpreter; `close(^session)` consumes the Jet handle, and process teardown
deletes any remaining interpreters before Tcl finalization. String results are
copied through a 64-KiB thread-local boundary and reject embedded NUL or
oversize values. Integer and float entrypoints use Tcl's typed object parsers.
Tcl failures become `TclError.Eval`; raw Tcl result text and stack frames never
cross the boundary. Calls carry the `Tcl` effect root.

Evaluation is synchronous. A long-running Tcl command blocks its calling Jet
thread until Tcl returns. This vertical exposes no cancellation claim and does
not kill or corrupt the in-process interpreter on timeout; cancellation needs a
future Tcl event-limit contract. Bridge tools run under a 60-second deadline
with 64-KiB diagnostic capture. Binding provenance hashes the initialization
script, Tcl runtime identity, and schema. Bind failures use **E3208**.

## E3 — Lua project binder (D-FFI-LUA1=A)

`jet inspect bind lua <script.lua> --pkg <lib>` validates the script with the
Jetpack-provisioned Lua compiler, discovers direct top-level
`function name(input)` declarations without executing the script, and compiles
a native archive against the provisioned Lua 5.4 headers. Each `open()` owns an
independent in-process `lua_State` and evaluates the script once. Mutable module
state persists within one session and remains isolated between sessions. No
subprocess or raw Lua handle is part of the public API.

Generated functions accept `DataTree`; sibling `<name>_typed<T>` adapters require
`T: [Encode, Decode]` and validate the decoded result before returning it. Null,
booleans, integers, floats, text, lists, and string-keyed maps retain their data
meaning. Cyclic tables, unsupported keys and values, nesting beyond 64 levels,
and input or output at least 1 MiB fail at the boundary. Lua errors become the
closed `LuaError` variants; exception text, paths, and stack frames never cross.
Calls carry the `Lua` effect root.

Sibling `<name>_view(session, deadline_ms)` adapters require the Lua function to
return a table and pin that table in the owning session's registry. `TableView`
integer reads and writes address the live table directly: they do not serialize
the table through JSON or `DataTree`, and Lua-side mutations are visible through
the same view. `TableView.Close` releases the registry reference at scope exit.
Session close releases every remaining table and invalidates all copied or stale
view handles; post-close access returns `LuaError.NotRunning`.

The VM instruction hook enforces each call deadline and observes concurrent
`cancel(session)` requests without destroying the session. A caught timeout,
cancellation, Lua exception, or protocol error leaves the VM available for the
next call. A bounded 32-slot generation table owns states. `close(^session)`
deterministically calls `lua_close`; stale and post-close calls return
`LuaError.NotRunning`. Provenance binds source, runtime identity, and schema.
Binding tools have a 60-second deadline and 64-KiB output cap; parse and tool
failures use laundered **E3208** copy. LuaRocks realization remains Jetpack
provider work and is not claimed by this binder.

Example: `examples/interop/lua/`.

## E3 — Ada project binder (D-FFI-ADA1=A, GNAT C-ABI vertical)

`jet inspect bind ada <package.ads> --pkg <lib>` reads exported functions from
an Ada package spec, compiles its sibling body with Nix-provisioned GNAT, and
writes a typed `ada.<lib>` binding cache. Supported exports use `Export`,
`Convention => C`, and `External_Name`; inputs and results are
`Interfaces.C.long_long`/`Long_Long_Integer` or
`Interfaces.C.double`/`Long_Float`. Unsupported ABI shapes fail binding rather
than being guessed.

Scalar subtypes with `range LOW .. HIGH` become pre-call checks in generated
Jet wrappers. A value outside the Ada range returns `AdaError.Constraint`
before the C-ABI export executes. Calls carry the `Ada` effect. Generated
bridges run GNAT elaboration once and finalization at process exit.

GNAT, binder, C compiler, and archiver processes have a 60-second deadline and
64-KiB output bounds. Raw GNAT locations are laundered behind **E3208**.
Provenance hashes the spec, package body, GNAT runtime identity, and binding
schema. The native link records the exact GNAT runtime directory and rejects a
missing or non-absolute runtime identity.

## E3 — Object Pascal project binder (D-FFI-PASCAL1=A)

`jet inspect bind pascal <library.pas> --pkg <lib>` compiles a FreePascal
library's exported `cdecl` routines and writes a typed `pascal.<lib>` cache.
`Int64` and `Double` cross as Jet `Int` and `Float`. A declared class binds
through exported `<class>_new`, pointer-first method, and `<class>_free`
wrappers. Unsupported ABI shapes fail binding instead of being guessed.

Class pointers never reach Jet. A generated C bridge owns them in a bounded
64-slot table and returns opaque integer identities wrapped in a move-only Jet
type. Methods borrow that type. `<class>_close(^handle)` consumes it. Closing a
stale identity is rejected by the table before the Pascal destructor runs;
process teardown destroys any remaining owned objects before FreePascal library
finalization. Calls carry the `Pascal` effect.

FreePascal, C compiler, and archiver processes have a 60-second deadline and
64-KiB output bounds. Compiler failures use laundered **E3208** what/why/fix
copy. Provenance hashes source, canonical compiler identity, and binder schema.
Native links pin the generated static bridge and shared Pascal runtime cache,
including its runtime search path.

## E3 — Dart and Flutter host FFI (D-FFI-DART1=A)

`jet inspect bind dart <contract.dart> --jet <compute.jet> --pkg <lib>` builds
one in-process, bidirectional FFI estate. The Dart or Flutter application owns
the isolate. The generated `<lib>_host.dart` loads the native Jet compute
library with `dart:ffi`, initializes `dart_api_dl` from
`NativeApi.initializeApiDLData`, pins isolate-local callbacks, and registers
their native function pointers. Jet compute exports use the existing plugin
C-ABI surface, so the same library can call Dart callbacks and be called from
Dart. No helper process, command shell, environment variable, or file protocol
participates in a call.

`shutdownJetDart()` unregisters every native callback pointer before closing
the pinned `NativeCallable` values. The isolate can then terminate without
leaving native code a callable address whose Dart owner has been released.

Dart callbacks are top-level `@pragma('vm:entry-point')` functions with
positional `int`/`double` inputs and an `int`/`double` result. Unsupported,
optional, named, generic, object, string, async, or overloaded shapes fail
binding rather than being guessed. Generated Jet wrappers return
`DartError.NotInitialized` until the Dart host initializes API DL and
`DartError.CallbackUnavailable` until a callback is registered. Calls carry
the `Dart` effect.

`NativeCallable.isolateLocal` makes this vertical synchronous and
isolate-thread-affine. Flutter uses the same generated Dart host and deploys
the produced platform library through its ordinary native-library packaging;
Jet does not claim to embed or launch a Flutter engine. Dart SDK discovery,
C compilation, archiving, and native Rust compilation are bounded to 60
seconds and 64 KiB of captured output. Tool failures are laundered behind
**E3208**. Provenance hashes the contract, Jet compute source, both canonical
source paths, canonical Dart SDK tool identity, and binder schema.

## E3 — Persistent PowerShell object pipeline (D-FFI-PWSH1=A)

`jet inspect bind pwsh <script.ps1> --pkg <lib>` validates the script with
PowerShell 7, binds its named `function` declarations, projects the conventional
PowerShell `-` separator to `_` in Jet names, and writes a typed
`pwsh.<lib>` cache. `open()` starts one supervised `pwsh` worker, waits at most
five seconds for its fixed startup handshake, and loads the
script once. Calls on that session retain script/module state and run the
named function's pipeline. Jet never accepts runtime PowerShell source or a
command string: generated entrypoints carry a binder-approved function
identity, and the worker checks the same allowlist before invocation.
The shipped process supervisor is POSIX-only; other hosts reject binding
generation instead of emitting a bridge they cannot supervise truthfully.

Every function accepts one canonical `DataTree` input and returns one
`DataTree`. Nested objects, lists, integers, floats, booleans, text, and null
cross through a length-framed structured JSON protocol; stdout text is not the
result channel. Requests and responses are capped at 1 MiB and JSON depth 64.
The bridge validates frame lengths and response envelopes before the generated
Jet wrapper exposes a value. PowerShell exceptions become
`PowerShellError.CommandFailed`; raw error records, script paths, stderr, and
stack traces never cross the boundary. Calls carry the `PowerShell` effect.

Each call declares a 1–300000 ms deadline. Expiry kills and reaps the whole
worker process group and invalidates its session. `cancel(session)` performs
the same group cancellation for an in-flight call; `close(^session)` consumes
the handle, and process teardown reaps remaining workers. Handles contain a
generation so stale identities cannot address a reused slot. At most 32
workers exist per process. Binding-time `pwsh`, C compiler, and archiver runs
have a 60-second deadline and 64-KiB output capture. Failures use laundered
**E3208** copy. Provenance hashes the script, canonical script and PowerShell
identities, worker protocol, and binder schema.

## E3 — Persistent Perl worker (D-FFI-PERL1=A)

`jet inspect bind perl <script.pl> --pkg <lib>` validates the script with the
provisioned Perl compiler, discovers named main-package `sub` declarations from
compiler metadata without running the top-level runtime body, and writes a
typed `perl.<lib>` cache. Foreign function names project to Jet `snake_case`
while the worker retains and invokes the exact Perl name. Perl's normal
compile-time blocks still obey `perl -c`.
`open()` starts one supervised Perl process, loads the script once, and retains
its lexical and package state across calls. Generated entrypoints carry fixed
function identities; the worker rejects names outside the binder-generated
allowlist. Runtime source and arbitrary command strings never cross the API.
The process supervisor is POSIX-only; unsupported hosts reject binding
generation instead of emitting an unusable bridge.

Every bound function accepts one `DataTree` and returns one `DataTree` through
Perl's core `JSON::PP`. Null, booleans, integers, floats, text, arrays, and
objects retain their JSON data meaning. The binary protocol length-frames each
request and response, checks response identities, limits frames to 1 MiB, and
never treats stdout text as a result. Perl exceptions become
`PerlError.CommandFailed`; stderr, script paths, stack traces, and exception
text stay inside the worker. Calls carry the `Perl` effect.

Calls require a 1–300000 ms deadline. Expiry or `cancel(session)` kills and
reaps the worker process group and invalidates the generation-tagged handle.
`close(^session)` consumes the session. At most 32 workers exist per process;
process teardown reaps all survivors. Binding-time Perl, C compiler, and
archiver processes have 60-second deadlines and 64-KiB output capture. Their
failures use laundered **E3208** copy. Provenance hashes source, canonical
script and Perl identities, worker protocol, and binder schema. CPAN package
realization remains the Jetpack provider's responsibility; the binder consumes
the Perl executable and installed modules exposed by that realized environment.

## E3 — Persistent Ruby worker (D-FFI-RUBY1=A)

`jet inspect bind ruby <script.rb> --pkg <lib>` uses the provisioned Ruby
runtime's `Ripper` parser to discover direct top-level method declarations
without executing the script. Bindable methods have one required positional
argument and a Jet-compatible name. Generated entrypoints are a fixed allowlist;
runtime source, method names, and arbitrary commands never cross the API.

`open()` starts one supervised Ruby process and loads the script once, retaining
global and object state across calls. Each method accepts and returns `DataTree`
through Ruby's standard `JSON` library. The binary protocol length-frames every
request and response, verifies response identities, limits frames to 1 MiB, and
never treats stdout text as a result. Ruby exceptions become
`RubyError.CommandFailed`; exception text, stack traces, stderr, and paths stay
inside the worker. Calls carry the `Ruby` effect.

Calls require a 1–300000 ms deadline. Expiry or `cancel(session)` kills and
reaps the worker process group and invalidates the generation-tagged handle.
`close(^session)` consumes the session. At most 32 workers exist per process;
process teardown reaps survivors. Binding-time Ruby, C compiler, and archiver
processes have 60-second deadlines and 64-KiB output capture. Failures use
laundered **E3208** copy. Provenance hashes source, canonical script and Ruby
identities, worker protocol, and binder schema. RubyGems resolution and install
are not implemented by this binder and remain unclaimed Jetpack provider work.

## E3 — Persistent R worker (D-FFI-R1=A)

`jet inspect bind r <script.R> --pkg <lib>` parses the script without running
its top-level body and binds direct named functions with one required argument.
`open()` starts one supervised R worker, loads the script once, and retains its
state. A normal `<name>` adapter round-trips `DataTree`; `<name>_table<T>` maps
`Table<T>` to `data.frame` and back through the same framed JSON channel.

`<name>_plot` runs the function on an isolated SVG graphics device and returns
the plot as `String`. The worker parses resulting XML structurally, permits only
an inert SVG element, attribute, local-fragment, and presentation-style
vocabulary, and emits deterministic canonical XML. It rejects scripts, event
handlers, `foreignObject`, external references, active CSS, declarations,
entities, malformed XML, and input or canonical output above 512 KiB. Plot
failures become `RError.CommandFailed`; R errors and rejected SVG content never
cross the boundary. Each worker gets a supervisor-created private temporary
directory. Success, error, deadline, cancellation, close, and process teardown
all remove its SVG and directory.

All calls require a 1–300000 ms deadline. Expiry or `cancel(session)` kills and
reaps the process group and invalidates that handle; a new session starts a
clean worker. Frames remain capped at 1 MiB, handles are generation-tagged, and
at most 32 workers exist per process. CRAN realization belongs to Jetpack; the
binder consumes the provisioned R runtime and installed modules.

## E3 — Windows COM automation (D-FFI-COM1=A)

`com.*` exists only on a Windows host. Elsewhere, importing it or running
`jet inspect bind com` emits **E3260** before reading a type library or looking
for a generated cache. Jet does not route COM through PowerShell or scripts.

`jet inspect bind com <library.tlb> --pkg <lib>` reads a file-backed type
library. `--registered <guid> --major <n> --minor <n> [--lcid <n>]` reads the
Windows type-library registry through `LoadRegTypeLib`. The inspector uses
`ITypeLib` and `ITypeInfo`, rejects hidden, restricted, out-parameter, and
unrepresentable members, and emits committable typed stubs. Primitive VARIANT
types become Jet scalars, BSTR becomes `String`, dispatch interfaces become a
move-only `Object`, and VARIANT/SAFEARRAY values become `DataTree` through a
bounded JSON boundary. Dynamic name-based IDispatch remains available only in
an explicit `#Unsafe` region; the safe generated surface carries fixed DISPIDs.

The Windows bridge initializes a single-threaded COM apartment per live
object, pins each generation-tagged handle to its creating thread, invokes
members through `IDispatch::Invoke`, and launders HRESULT/EXCEPINFO into
`ComError` variants without exposing vendor text. `close(^object)` consumes the
handle, calls `Release`, and balances `CoUninitialize`; stale, cross-thread,
and double-close handles fail before invocation. Frames and DataTree recursion
are capped at 1 MiB and depth 64. Provenance hashes the extracted type-library
schema and generated surface.

## E2-M13 — Expert low-level tier (S58, implemented)

C/Zig-class control behind two explicit gates; ordinary Jet never reaches it and
emits **zero** `unsafe` (the I1 amendment, D-LL1, recorded in `architecture.md`).

- **Discovery gate** — `use core.mem;` unlocks the low-level vocabulary (`*T`,
  `mem.volatile_read`, `mem.volatile_write`, `mem.address_of`, allocators).
  Naming one of these without the import → **E3102**.
- **Audit gate** — `#Unsafe("reason") { … }` opens the operations that can
  violate memory safety (pointer build/deref, volatile access). The reason
  string is the argument to `#Unsafe` itself (D-UNSAFE2; the former separate
  `#Audit("…")` line is retired → **E0055**). Under **D-UNSAFE-REASON1=A**,
  bare `#Unsafe { … }` and bare `#Unsafe fn` are hard errors (**E3112**).
  `#Unsafe("reason") fn` marks a whole-function contract; its body is itself
  an audited region, and calling it requires an enclosing `#Unsafe` block →
  **E3103**.
- **Operations** — prefix `*x` takes a raw pointer to `x`; postfix `p.*`
  dereferences it. `mem.address_of(x)` is inert (a plain address as `Int`) and
  legal outside a gate. `mem.volatile_read(p)` and
  `mem.volatile_write(p, value)` perform explicit volatile/MMIO access through a
  typed pointer. Using a low-level op outside `#Unsafe` → **E3101**.

Codegen stays dumb (I3): an `#Unsafe { … }` region lowers straight to a Rust
`unsafe { … }`, an `#Unsafe fn` to a Rust `unsafe fn`. All gating is decided in
sema. Diagnostics **E3101–E3104 + E3112** in diagnostics.md with snapshots
(`tests/ui/lowlevel_e310*`, `tests/ui/mem_arena_gate`, `tests/ui/mem_use_after_free`,
`tests/ui/unsafe_missing_reason`, `tests/ui/unsafe_fn_missing_reason`); the audited end-to-end example is
`examples/features/lowlevel/lowlevel.jet`.

D-UNSAFE-OBLIG1 adds a policy layer without weakening either gate. Absent policy
and `.GateOnly` retain the behavior above. Policy never suppresses the mandatory
reason.
`.Obligations` requires an operation-specific typed assertion immediately after
each low-level operation, using the closed facts `valid_ptr`, `aligned`, and
`no_alias`, for example `assert valid_ptr, aligned`. `.PerSite` requires each
gate to add `obligations: .Track` or `.Skip`; an organization `.Obligations`
floor rejects `.Skip`. CI/admins provide that floor explicitly through
`JET_ORG_UNSAFE_POLICY=<path>`; the file uses the package-policy shape
`policy: .{ unsafe: .Obligations }`, its path is retained as provenance, and a
configured unreadable or malformed file fails closed. `jet inspect unsafe FILE`
reports every gate, operation, discharge state, and effective-policy provenance
in stable human or `--json` form. Human rows use the source file's
`file:line:column` location; JSON keeps the byte span and adds matching 1-based
start/end line and column objects. Loader failures use the ordinary diagnostic
renderer, including the source frame, Why, Fix, and `NO_COLOR` behavior.
Assertions erase in sema before the shared AOT/dev TIR boundary.

## Web browser API (D-FLAGSHIP-WEBAPI1, implemented)

`use core.web as web` exposes the browser-owned pieces that a web flagship slice
needs outside the retained `core.ui` paint surface:

- `web.on(selector, event, handler)` binds a DOM event listener. The handler gets
  a `WebEvent` value; handlers that do not need the event may ignore it.
- `web.value(selector) => String` reads an input value or element text.
- `web.storage.local.get(key) => String?` and
  `web.storage.session.get(key) => String?` read browser storage. Missing keys
  compose with the normal `??` fallback: `web.storage.local.get("tasks") ?? "[]"`.
- `set(key, value)`, `remove(key)`, and `clear()` mutate local/session storage.

`core.web` carries the `Browser` effect. The web JS backend emits real
`addEventListener`, `querySelector`, `localStorage`, and `sessionStorage` calls;
native codegen lowers the same checked calls to inert stubs so rustc never
becomes the browser API checker.

## First-party events and hooks (D-EVENT1, implemented)

`use core.event as event` exposes the first compiler-known event family as
ordinary Core values. There is no `event` declaration syntax in this slice.

- `event.new<T>() => Event<T>` creates a typed many-subscriber occurrence stream.
- `event.async_result<T, E>(policy, failures) => AsyncEvent<T, E> ? String`
  creates one scheduler-backed bounded queue; see [Bounded buffering law](#bounded-buffering-law)
  for its pressure behavior. `emit_async` returns `Task<DispatchReport<E>>`;
  queue, running, blocked, failure, cancellation, deadline, close, and overflow
  outcomes are explicit.
- `event.hook<T, R>(fallback) => Hook<T, R>` creates an ordered intervention
  point. `.run(payload, fallback)` returns the last active handler result, or
  the call-site fallback when no handler is active.
- `event.decision_hook<T, E>(HookPolicy.FirstCancelElseTransform)` creates a
  typed fold. Handlers return `HookDecision.Continue`, `.Transform(value)`,
  `.Cancel`, or `.Fail(error)`; `run` returns `HookOutcome.Continue(final)`,
  `.Cancel`, or `.Fail(error)`.
- `event.scope() => EventScope` owns subscriptions. `scope.cancel()` unsubscribes
  all owned subscriptions and permanently closes that owner. Cancellation is
  idempotent; a later subscription attempt through the cancelled scope returns
  an inactive `Subscription` and installs no listener. `scope.active_count()`
  reports currently active subscriptions.
- `Event<T>.on(scope, handler)`, `.once(scope, handler)`, and
  `.on_priority(scope, priority, handler)` return `Subscription`. Priority sorts
  before source order; `once` auto-unsubscribes after first delivery.
- `Event<T>.emit(payload)` returns `EventTrace`; `AsyncEvent.emit_async(payload)`
  returns a task whose report records the accepted payload's terminal state and
  ordered trace.

Synchronous emission snapshots active listeners at dispatch start, sorts by
priority descending then registration order, and invokes that snapshot
depth-first. Unsubscribing before a listener's turn skips it; subscribing during
delivery affects only a later or explicitly nested emission. A `once` listener
is deactivated before its handler runs, so reentrant emission cannot deliver it
twice. D-EVENT2=A keeps this beginner `Event<T>` handler path infallible; typed
failure aggregation is outside this synchronous API.

With `JET_OBSERVE=1`, the runtime publishes one bounded, payload-free sequence
for executed Event, AsyncEvent, and DecisionHook transitions. `jet inspect live`
and Canvas opened with `?pid=<live Jet pid>` consume that same validated source;
Canvas reports `runtime_events: null` when no live process is attached and never
turns source-call matches into runtime facts.

```jet
use core.event as event

fn run() {
    scope :: event.scope()
    clicked :: event.new<Int>()

    sub :: clicked.on(scope, (n) => { print("clicked {n}") })
    clicked.once(scope, (n) => { print("once {n}") })

    print(clicked.emit(1).summary())
    sub.unsubscribe()
    scope.cancel()
}
```

### Jai transliteration: compact cast/deref chains (D-POINTERCHAIN1=A, docs-only)

Jai allows a single compact expression that casts and dereferences a raw pointer
in one chain, e.g. `slot.value_pointer.(*Bool).* = true`. Jet rejects that
compact form outright — there is no cast-and-deref operator. The equivalent is
two explicit, audited lines: reinterpret an address through
`mem.Ptr<T>.from_addr(addr)` (the cast step), then read or write through it
with postfix `p.*` (the deref step), both inside `#Unsafe`:

```jet
use core.mem

flag: Bool :: true
#Unsafe("flag is live on this stack frame and the pointer never escapes") {
    addr: Int :: mem.address_of(flag)
    p :: mem.Ptr<Bool>.from_addr(addr)
    print(p.*)
}
```

No new syntax, sema, or codegen — this section only names the existing
`mem.address_of` / `mem.Ptr<T>.from_addr` / postfix `.*` vocabulary (§E2-M13
above) as the answer to "what does Jai's chain do in Jet." Example:
`examples/features/lowlevel/pointer_cast_deref.jet`.

### Allocators (D-ALLOC1, D-ALLOC-C, D-ALLOC-D; ratified 2026-06-19)

Four allocators ship under `core.mem` — `Arena`, `Bump`, `Pool`, `Fixed` — all namespaced
under `core.mem.alloc` (D-ALLOC-C). No `#Unsafe` needed; `use core.mem` is the discovery
gate (E3102). Constructors: `mem.Arena.new()` / `mem.Arena.new(capacity: N)` (D-ALLOC1);
allocate with `arena.alloc(value)`. `reset()` keeps the backing storage (cheap, allocator is
reusable). Terminal release uses the universal resource operation `close(^allocator)`; the
retired `.free()` spelling is rejected with a fix to `close`. A later use is the ordinary
**E0121** use-after-move error. Example:
`examples/features/memory/arena.jet`.

The runtime families are not aliases. `Arena` grows through aligned heterogeneous chunks and
reuses every retained chunk after reset. `Bump` is one contiguous caller-capacity buffer with
monotonic placement and explicit exhaustion. `Pool` has a caller-bounded slot count; reset bumps
its generation and reuses compatible retained size/alignment classes, while incompatible classes
are replaced without imposing a secret maximum value size. Values are dropped in reverse
allocation order before storage is reused. Allocator handles are thread-confined; move plain owned
data across task/channel boundaries instead.

`Fixed` retains the ratified no-hidden-heap law, but that law is not implementation-complete. The
front end currently erases `size: N` to monomorphic `Type::Named("Fixed")`, and emitted signatures
name monomorphic `jet_mem::JetFixed`; therefore an arbitrary runtime size cannot become owned
stack/static storage without either hidden heap allocation or an invented maximum. The existing
heap-backed compatibility runtime is not acceptance evidence for `Fixed`. Completion requires the
compiler to preserve a compile-time capacity (or an owner-ratified caller-buffer representation);
#648 must not disguise that gate with a heap facade or silent cap.

### Arena regions and scope-bound views (D-ALLOC2, D-REGION1; ratified 2026-06-21, implemented)

The c05 upgrade makes the arena *real*: `arena.alloc(value)` places a value in retained
allocator storage and returns a **scope-bound `view`** — Rust `&'arena mut T`
— not an owned copy. The runtime (`Source/Prelude/Mem.rs`, `mod jet_mem`) carries the one
vetted lifetime-extension internal (D-LL1, inside the helper only; never leaks to user code,
golden-test enforced); reset borrows the arena mutably and close consumes it, so rustc itself
forbids reset/close while a view is live — the I2 backstop.

A view is sound only inside its **region** and only until the arena is reset or closed. Two
sema checks (`Source/Sema/CheckerOwnership.rs`), both at least as strict as rustc's borrow
checker so Jet always rejects first (I2):

- **E0631** — the view escapes its region: returned, stored in another binding
  or struct field, passed to a `&`/`^` parameter, or captured by an escaping
  closure.
- **E0632** — the view is read after its arena was reset.

Regions (D-REGION1): **implicit and scope-inferred by default** — the region is the lexical
scope of the `arena` binding; the beginner never types a lifetime. **Plus an explicit
`#Region(r) { … }` block** for expert cases
inference can't give: a region spanning two allocators, narrower than the enclosing function,
or named. The escape rule is enforced against the inferred scope or the named region
identically. v1 restriction (I8): views are non-reassignable, non-escaping locals; anything
the analysis can't prove is rejected with a teaching error. Example: `75_arena_regions.jet`;
UI snapshots `tests/ui/arena_view_escape` (E0631), `tests/ui/arena_view_after_reset` (E0632);
unit tests `tests/arena.rs`.

## M6 phase 3 — multi-file imports (done)

Two use forms (S16): **quotes = file path, no quotes = module.**
**`use "path/to/file";`** — quoted path to a `.jet` file, relative to
the using file's directory (`use "./lib";` for a sibling file;
default namespace = last path segment). **`use name;`** or
**`use core.files;`** — unquoted module name (searches recursively from
the project root for `name.jet` or `name/{name,main}.jet`; `core` is a
compiler-exported module per S51). Optional **`as alias`** in both forms.

Cross-file access uses **`namespace.item`**; only **`pub`** items are visible from
other files (S18), including **`pub`** struct fields. A file may opt into
public-by-default with a single **`#PubFile`** marker (D-VISDEFAULT1=C /
D-VISDEFAULT2=A); inside such a file, top-level items export unless marked
**`priv`**. The driver loads the import
graph, sema checks the whole program, codegen emits one Rust file with **`mod`**
blocks and `user_<module>_<name>` mangling (`main` stays `main`).

Diagnostics: **E0602** path escapes the project · **E0603** missing import ·
**E0604** import cycle · **E0605** private item · **E0606** ambiguous module.
Example: `examples/features/modules/imports/` (three files; file import + `as alias`). UI
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
(D-ILE1): in a `package.jet` `packages:` block a bare `name` (no `: kind`), or a
package with no `package.jet` at all, resolves to `executable` when its source stages
a `bin/` or declares a top-level `fn run`, otherwise `library`; an explicit
`library`/`executable` always wins. Single-file `jet run`/`build file.jet` stays
executable-requiring (R9; E0101 if it has no `run`). A `library` dependency the project declares but hasn't
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

**Visibility (D-MOD3, D-PUBPKG1).** Private by default; `pub` exports to every
consumer; `pub(package)` exports only inside the same payload/workspace package
boundary and stays hidden from downstream package consumers. A private item is
unreachable from outside its file or inline module: `math.helper()` where
`helper` is private is **E0609** (inline) / **E0605** (cross-file). An unknown
`pub(…)` qualifier is **E0411**. Inline-module function bodies are fully
type-checked, and a sibling call (`area` → `square`) lowers to the
module-mangled name (`geo__square`), so private siblings never leak into the
file's namespace or to rustc.

**Re-export (D-MOD4 — Rust-exact `pub use`).** A directory module's `module.jet`
exposes a submodule item only by re-exporting it: `pub use wrap.wrap;`. Nothing
auto-surfaces — a `pub`-but-not-re-exported item stays internal to the directory.
`text.wrap(…)` then resolves through the re-export to the defining module, with
the real function's borrow/move conventions preserved.

Examples: `examples/features/modules/inline_module`, `43_module_file`,
`44_module_dir`, `45_module_use_unqualified`, `46_module_use_group`,
`47_module_reexport`, `48_module_file_use`, `49_module_inline_sibling`,
`170_generic_modules`. UI
fixtures: `tests/ui/module_{missing,private,unknown_namespace,wildcard,
inline_private,inline_type_error}`, `genmod_{unknown_target,wrong_arg_count,
value_wrong_type,value_not_comptime,disallowed_value_type,trait_bound_unsatisfied,
cycle_direct,cycle_indirect}`.

### Generic modules (D-GENMOD1, D-GENMOD2, D-GENMOD-VALUE1,
D-GENMOD-BODY1, D-GENMOD-IDENTITY1)

A **generic module** is a module template parameterized by types and compile-time
values. Instantiating it produces a specialized ordinary module.

**Template form (D-GENMOD2=A):**

```jet
module cache<K> {
    pub fn key_of(k: K) => String { … }
}
```

Type parameters use PascalCase names with an optional bound (`K: Hash`).
Value parameters use lowercase names with a type annotation (`capacity: Int`).
Both live in one `<…>` list.

**Instantiation alias:**

```jet
module int_cache = cache<Int>
```

Value parameters are immutable Tier-0 comptime `Bool`, `Int`, `Char`, `String`,
or fieldless-enum values (D-GENMOD-VALUE1=A). The compiler evaluates and
normalizes each value before specialization. `Int` parameters may appear in the
generic-module-only fixed-list layout form `[T#capacity]`. Value argument types
must match exactly; E0853 reports a mismatch.

The template body has ordinary-module parity and definition-site lexical scope
(D-GENMOD-BODY1=A). It may contain functions, structs, enums, tags, constants,
traits and impls, tests and benches, nested modules and generic modules,
aliases, and existing legal markers. Applying a template does not change what
its body can see or authorize.

Instantiation is applicative (D-GENMOD-IDENTITY1=A). The resolved template
DefinitionId and normalized arguments form the instance identity. Repeating
the same application shares nominal member types and one checked/code-generated
specialization; different arguments or a different template definition produce
a different instance.

**Implementation status:** parser, sema, and codegen specialize full module
bodies across same-file and imported templates. Type/value substitution,
definition-site capture, bounds, cycles, applicative identity, stable instance
fingerprints, semindex/LSP identity, and fail-closed E0859 collision handling
are implemented. E0850–E0853 and E0855–E0857 reject invalid targets, arity,
bounds, value kinds/types, scope, and cycles in sema before codegen. Remaining
card work is the final executable acceptance matrix and documentation/example
closure; package-cache and cross-toolchain proof is tracked separately by the
card's later criteria.

## M6 phase 4 — `--small` + LSP v0 (done)

**`jet build --small`** (S15): `opt-level=z`, fat LTO, `panic=abort`, stripped symbols.
Smaller binaries than the default speed-oriented profile (`tests/release_gates.rs` on
`examples/features/collections/wordcount.jet`).

**`jet self lsp`**: stdio JSON-RPC language server (hand-rolled JSON, invariant I6).
Capabilities: full-document diagnostics on open/change (real front end, including
import graph from disk with an in-memory overlay for the open buffer), S14
teaching-error quick-fixes (`Diagnostic.edit`), and formatting via `jet fmt`.
Scripted tests: `tests/lsp.rs`.

### Hand-rolled parser contracts

The std-only parsers at compiler and package boundaries accept only these
published grammars. Unsupported or ambiguous input is rejected; it is never
partially guessed.

| Boundary | Accepted input | Deliberate rejection |
|---|---|---|
| LSP/DAP JSON, framing, and request envelopes | UTF-8 RFC 8259 null/booleans, finite numbers, strings (including `\\u` surrogate pairs), arrays, and objects through depth 64. Object names are unique. `Content-Length` bodies are capped at 1 MiB before allocation; framing headers are capped at 8 KiB and 64 fields. JSON-RPC requests use `jsonrpc: "2.0"`, a string `method`, object/array `params`, and a string or signed-64-bit-integer `id`. DAP requests use `type: "request"`, a positive `u32` `seq`, a nonempty string `command`, and optional object `arguments`; breakpoint lines are positive `u32` integers. LSP positions are nonnegative `u32` integers; `jet.impact` depth is clamped to 1–64. | Oversized headers/bodies, duplicate `Content-Length` headers, or non-UTF-8 frames; duplicate JSON names; raw string control characters; malformed/overflowing numbers; lone surrogates; deeper nesting; non-object requests; fractional IDs/positions/sequences/lines; and scalar parameters. JSON-RPC syntax errors return `-32700`; invalid JSON-RPC envelopes return `-32600` with a null id. Malformed DAP envelopes are not dispatched, and unknown string commands receive an unsuccessful response. |
| `jetpack.toml` manifest | TOML syntax followed by the closed `[repo]` (`name`, `version`) and `[sources]` string-valued schema. Dotted top-level keys are allowed. Syntax recovery continues at the next statement so independent errors are reported together. | Duplicate assignments or table declarations, dotted-key/table collisions, invalid escapes/numeric lexemes or overflow, non-string schema values, array tables, unknown tables/keys, and retired `[packages]`. These are Jet-owned E1214/E1215/E1225 diagnostics. |
| SemVer and dependency ranges | SemVer 2.0.0 `major.minor.patch`, optional pre-release/build identifiers, and the documented node-semver comparator, caret, tilde, x, hyphen, whitespace-AND, and `||`-OR forms. A leading version `v` is accepted for tag compatibility; an empty requirement means `*`. | Core numeric overflow/leading zeroes, empty identifiers, invalid characters, wildcard-before-number forms, empty `||` alternatives, and any range whose exclusive upper bound would overflow `u64`. Pre-release numeric identifiers remain spec-unbounded and compare without integer conversion. |
| C bind prototypes | Top-level `return_type name(parameters);` declarations for the documented scalar, `char*`, and `void` subset. `(void)`/empty lists and unnamed scalar parameters are accepted. Unsupported but structurally valid types are reported in the skipped list. | Function bodies/pointers, variadics, unbalanced lists, empty comma fields, trailing declarator text, non-ASCII C identifiers, and declarations with no return type. No guessed binding is emitted. |
| Registry JSONL and advisory database | One UTF-8 JSON object per registry line with nonempty string `name`/`version`, optional string `content_hash`, `fingerprint`, `public_key`, and `signature`, plus optional boolean `yanked`; older lines may omit signing fields. Advisory lines are `id|package|affected|fixed-or-empty|title[|severity]`; blank lines and `#` comments are allowed, and unknown/missing severity remains `medium` for compatibility. | Malformed/duplicate/nested-fake JSON fields, unknown registry keys, wrong field types, partial registry records, advisory field-count errors, empty required fields, invalid affected/fixed versions, or `|` inside fields. Reads fail closed with E2607 rather than skipping security metadata. |

**VS Code / Cursor**: `editors/vscode/` — TextMate grammar + LSP client (plain
JS, no compile step; `install.sh` packs and installs the vsix). The client
auto-discovers the server: `jet.languageServerPath` setting, then
`<workspaceFolder>/target/debug/jet`, then `jet` on PATH. `jet self lsp` never
invokes rustc, so the cargo debug binary is sufficient.

## M8 — Functions as values (closures, done)

**Lambdas (S46):** `(params) => expr` or `(params) => { … }`. A single
assignment or void call after `=>` needs no braces (`a => a.n += 1`,
`() => work()`). Parameter types
may be omitted when the expected function type is known (**E0801** when not).
The lambda arrow is **`=>`**. **`->`** selects dispatch-arm values and finite-loop
items.

**Function types (S47):** `fn(T1, T2) => R` (no parameter names; the result may be
omitted for `()` callbacks). Their unmarked parameters always have plain
read access (D-MEM-PARAM1). Named `fn`s coerce to function values when referenced
without a call only if every parameter also has plain read access. Functions with
write (`&`) or move (`^`) parameters remain direct-call-only; coercion cannot erase
those requirements.

**Capture rules (S47):** shared read for names only read; mutable borrow for
names written (a `:=` binding required, else **E0111**). Escaping lambdas (stored in a
binding, returned, in a struct field, or passed to a `^T` parameter) must own
captures. Copy values copy at closure creation. Other clonable values clone at
closure creation. Owned non-clonable values move. A borrowed non-clonable
parameter cannot escape (**E0120**). The retired `take(...)` prefix is **E0057**.
Self-recursion through the binding is rejected (**E0804**). Calling a
non-function → **E0803**.

**Collection methods:** `map`, `filter`, `each`, `find`, `any`, `all`,
`sort_by`, `reduce` on `[T]`; `each` on `[K: V]` (two parameters).

**D-ITER1 — lazy iterator adapter set (c105):** `take(n)`, `skip(n)`, `step_by(n)`,
`dedup()`, `chunks(n)`, `windows(n)`, `take_while(f)`, `skip_while(f)`, `flat_map(f)`,
`scan(init, f)`, `fold(init, f)`, `position(f)`, `min_by(f)`, `max_by(f)`, `group_by(f)`,
`partition(f)` on `[T]`. No new grammar — all are library methods on the iterator
protocol (D-EXT1 Tier 1). `take` is accepted in dot-method position even though `take`
is also the lambda-capture keyword. `indexed()` returns `(idx: Int, item: T)`.
The zip family is variadic and named: strict `zip` requires equal lengths and
reports E0128, `zip_short` stops at the shortest input, and `zip_pad` reaches the
longest input. `zip_pad` uses `None` for omitted fills, one typed `fill:` value
for all columns, or typed `fills: (field: value, ...)` per column. Free calls
preserve labels; methods use `a`, `b`, `c`, and so on. Zero free inputs are an
empty `Iter<Unit>` and one input is identity. `partition(f)` returns
`(false_: [T], true_: [T])`. All are lazy (evaluated at call site, allocation
deferred to result use).

D-S14-PAUSE: retired `lambda` / anonymous-function spellings get ordinary
parse errors. Current lambda syntax is `(x) => …`. D-SHAPE-PIPE1=C assigns a
single bar only to pattern and choice alternatives; it has no lambda or flow
alias.

Examples: `examples/features/basics/closures.jet`, `examples/features/basics/callbacks.jet`,
`examples/features/collections/iter_adapters.jet`. Ui:
`tests/ui/lambda_*.jet` (E0801–E0804, E0204 mut-capture conflict,
E0507 collection change inside a `for` loop), `tests/ui/not_a_function.jet`.
Integration: `tests/closures.rs`.

## M10 — Core library (done)

Full user-facing reference: **docs/reference/core-library.md**.

Compiler-known `core.<name>` namespaces backed by Rust std helpers in the
generated prelude (D-CORENS1/D-CORENS-CANON1): file/terminal/env/process I/O,
math, random, time, args, sized numeric types with checked-by-default
overflow, and unified `core.encoding` serialization (JSON/CSV/TOML/YAML over
one `DataTree` value, plus `#Codable` derive). Every fallible call returns
`T ? E`, handled with `?`/`??`/a pattern test like any M4 result. Importing a
module is free (R10) — codegen only emits the helpers a program actually
calls. See core-library.md for the full module list, signatures, and
examples; UI snapshots: `tests/ui/core_*`, teaching errors **E0037**–**E0039**.

### `ByteBuffer`

Growable byte builder with one read cursor (D-ITERTOOLS1=A / #1467). EOF is
`position == len`. Construct with `ByteBuffer.new()`, `ByteBuffer.with_capacity(n)`,
or `ByteBuffer.from(bytes)`.

Write path: `write_u8` / `write_byte`, width-specific `write_u16_*` / `write_u32_*` /
`write_u64_*`, `write_bytes` / `write`, `write_to`.

Cursor: `position`, `eof`, `seek`, `rewind`, `read`, `read_byte` / `next`,
`read_bytes`, `read_string`, `get`, `first`.

String-like ops decode UTF-8 (lossy) then reuse String behavior: `contains`,
`starts_with`, `ends_with`, `trim` / `trim_start` / `trim_end`, `to_lower` /
`to_upper` / `to_title` / `title`, `replace`, `split`, `join`, `lines`,
`index_of` / `last_index_of`, `is_ascii`, `to_string` / `string`, `parse`.

Lifecycle: `flush`, `close`, `shutdown`, `copy` / `clone`, `copy_to`, `equal`,
`compare`, `capacity`, `get_buffer` / `buffer`, `to_bytes`, `len`, `is_empty`,
`clear`.

Consuming typed reads stay on `core.binary.Reader` / `core.io` Writer handles —
ByteBuffer does not grow a second Reader/Writer type (I8).

Example: `examples/features/io/byte_buffer.jet`.

### `core.math`

Callable surface for floating-point and whole-number helpers (D-MATHLIB2,
D-CORESURFACE1, D-NUMTYPE1). Beyond the base libm set (`sin`/`cos`/`exp`/`ln`/…),
Jet ships:

- inverse hyperbolics and accurate near-zero forms: `acosh`, `asinh`, `atanh`,
  `cbrt`, `exp2`, `exp_m1`, `ln_1p`, `log(x, base)`, `signum`, `fma`, `copysign`
- float classification and neighbors: `is_normal`, `is_subnormal`,
  `is_canonical`, `is_signed`, `is_zero`, `is_integer`, `sign_bit`, `next_up`,
  `next_down`, `next_after`, `ldexp`, `scaleb`, `logb`, `ilogb`, `significand`,
  `ulp`, `radix`, `zero`
- decomposition pairs (named tuples): `sin_cos`, `modf`, `frexp`, `div_mod`,
  `div_rem`
- specials: `erf`, `erfc`, `gamma`, `lgamma`, `inv`, `cot`, `copy`, `cmp`
- whole-number helpers: `is_even`, `is_odd`, `isqrt`, `factorial`, `binomial`,
  `digits`, `leading_ones`, `trailing_ones`, plus checked/saturating/wrapping
  integer families
- exact ratios: `fraction(n, d) => Fraction?` with `.numerator()`,
  `.denominator()`, `.to_string()`, `.to_float()`, `.is_zero()`, and arithmetic

Examples: `examples/features/math/math_audit.jet`,
`examples/features/math/more_math.jet`, `examples/features/math/fraction.jet`.

### `core.os` (D-OSFACTS1, ledger #1465)

System facts and process identity live in `core.os`. Environment variables and
cwd/home stay in `core.env`. Subprocess run/exit stay in `core.process`.

Safe facts: `name`, `family`, `arch`, `cpu_count`, `temp_dir`, `executable`,
`pid`/`getpid`, `hostname`, `username`, `release`, `version`, `getppid`,
`getuid`, `geteuid`, `getgid`, `getegid`, `getgroups`, `getpgid`, `getpgrp`,
`getsid`, `expand`, `uptime`, `loadavg`, `times`, `exitcode`, `success`,
`sync`, `set_current_dir`, `on_interrupt`.

POSIX process/session control requires an audited `#Unsafe("…")` region and a
host-OS gate (`$if build.os` / `#Target(OS.*)`): `fork`, `setuid`,
`setgid`, `setpgid`, `setpgrp`, `setsid`, `initgroups`, `kill`, `wait`,
`waitpid`, `pipe`, `close_fd`, `mkfifo`, `umask`, `getpriority`,
`setpriority`, `utime`, `atexit`, `stop`. Those helpers do not fake POSIX
semantics on Windows.

Examples: `examples/features/io/os_facts.jet`,
`examples/features/io/os_process_control.jet`.

D-CORE-COMPRESS1=A splits compression by job. `core.compress.gzip` and
`core.compress.zstd` are the only byte-stream codec homes; both expose
`compress` and fallible `decompress`. `core.archive` exposes zip/tar container
operations only (`zip_compress`, `zip_decompress`, `tar_add`, `tar_get`,
`tar_names_json`). It has no gzip re-export or compatibility alias.

D-EMAIL1/D-EMAIL-SMTP-SURFACE1/D-EMAIL-SMTP-CONFIG1/
D-EMAIL-DKIM-CONFIG1 define one native
`core.email` path. Typed `Message` values retain a separate envelope so Bcc is
never serialized. `smtp_from_env()` and `smtp(config)` construct the same
`Mailer`; `Mailer.send(message)` performs verified TLS-from-connect or mandatory
STARTTLS, authenticates only after verified TLS and post-upgrade EHLO, returns
relay-acceptance `SendReport`, and never retries. `SystemPlusCa` extends system
roots while retaining hostname verification. Passwords use the existing
move-only `Secret`, cross one private extraction boundary, and are zeroized on
failure and drop. Ambient task cancellation and `#Context` deadlines govern
every transport wait; interruption after DATA is `DeliveryUnknown`.
Optional `SMTPConfig.dkim:DkimConfig?` binds one Ed25519 signing identity to
every send through that Mailer. The signer uses relaxed/relaxed DKIM over final
MIME bytes, requires `from`, rejects invalid or absent requested headers before
connecting, and never falls back to unsigned mail. Environment configuration
requires the domain, selector, and base64 32-byte seed together. Separate
identities use separate Mailers.

D-DATAFRAME1/D-DATA-SURFACE1 define one typed `core.data` path. `Table<T>` and
`Series<T>` own rows and values. `LazyFrame<T>` owns a source plus deferred
filter/sort operations; `plan` inspects them without running selectors, while
`collect` and reducers materialize them in order. `inner_join` returns stable
`DataJoin<L, R>` row pairs with full duplicate-key multiplicity. `left_join`
returns `DataJoin<L, R?>`, preserving every left row and representing an
unmatched right row as `None`.

## E2-M1 — Concurrency (tasks and channels, verified 2026-06-14)

`core.tasks` provides blocking tasks and typed channels. Import it as a normal
core module:

```jet
use core.tasks as tasks;
```

`tasks.spawn(() => work()) => Task<T>` starts a task from a zero-parameter
lambda. Copyable captures are copied at closure creation. Owned non-copyable
captures move. Shared mutable captures are **E1101**; use task-local state or a
channel to send results back. Values crossing the task boundary must be
sendable (**E1102**): no `view` borrows, no structs that contain `ref` fields,
no trait values, and no closures with non-sendable captures.

`task.join() => T` waits for the task and consumes the `Task<T>` handle. Calling
`.join()` twice is ordinary use-after-move (**E0121**). Dropping a `Task`
without joining or detaching emits **L1101** because the program may end before
the task finishes. A panic inside a task is reported when joined and exits with
the runtime panic code.

`task.detach()` (D-DETACH1) fire-and-forgets the task — it consumes the
`Task<T>` handle so **L1101** is suppressed, and the task continues running in
the background. Detach is sound only when the spawned lambda holds fully-owned
data. Two detach-site diagnostics guard unsound cases:

- **E1106**: the lambda returned or captured a `view` borrow — a detached task
  may outlive the borrow's source, so the `view` would dangle. Fix: pass an
  owned `~` copy or `share` instead.
- **E1103**: the lambda had a different sendability failure at spawn (E1102
  already fired); detaching an unsound task is doubly dangerous.

D-COROUTINE1 keeps coroutine machinery internal and exposes expert control via
task handles instead of new `coroutine` syntax. `task.wait()` aliases
`task.join()`. `task.pause()`, `task.resume()`, and `task.cancel()` set
control-plane state on the handle; `task.trace() => String` reports
`paused=...,cancel=...`. `task.exception() => String` reports `"cancelled"`
after cancel (otherwise `""`). `tasks.yield_now()` cooperatively yields at a wait
point; `tasks.current_task() => String` returns the running task's control
trace (idle defaults outside a task). `tasks.wait_any(^handles) => T` (and
`handles.wait_any()`) waits for the first finished task. `sender.close()` /
`receiver.close()` close a channel end explicitly. Pause holds a running task
at its next wait point until `resume()`; these are enforced by the M:N
scheduler, not mere flags.

D-CANCELMODEL1=C (ratified 2026-07-11): cancellation is **preemptive at wait
points**. A cancelled task — a race loser, a fail-fast sibling, or an explicit
`handle.cancel()` — unwinds at its next wait point (channel receive/send,
`time.sleep`, task join, a `select` arm, I/O), running Drop-backed (RAII)
cleanup on the way out, exactly as a blown deadline (E3003) already does. There
is one unwind mechanism with two triggers (deadline, cancel). A cancelled task
does not fall through to the code after the wait: a cancelled `receive()` unwinds
instead of returning `Closed`, and a race loser stops at its next wait point and
releases resources via Drop rather than running to completion. A cancelled
`g.all` member reports `Cancelled` rather than a completed value. A scoped
shielded region defers (never discards) the unwind until a critical section
finishes — its wait points complete normally and the deferred cancel/deadline
lands when the region exits. D-SHIELDNAME1=A spells that lexical region
`#Shield { … }`. It takes no arguments, nests by depth, and always leaves through
an RAII guard, so return, error propagation, and unwinding cannot strand a task
in the shielded state. At the outermost normal exit, an expired deadline lands
before a pending cancellation. Outside a task, the region is a transparent
block; at comptime it has no scheduler effect.

`tasks.channel<T>() => (Sender<T>, Receiver<T>)` (D-TUPLE-DESTRUCT1) creates a
linked send/receive pair, destructured at the call site: `(tx, rx) :=
tasks.channel<T>()`. A second sender is `~tx` — there's no combined
"channel" value to fetch one off of. `sender.send(value)` moves a `T` into the
channel (ownership semantics for non-copy values), and
`receiver.receive() => T ? Closed` blocks until a value arrives or all senders
are gone. Channel payloads
must be sendable (**E1102**).

`tasks.channel<T>(capacity: N)` creates the same pair with a bounded buffer.
Its full-buffer behavior is defined by the [Bounded buffering law](#bounded-buffering-law).
To limit active work, seed the channel with `N`
tokens; each worker receives one before work and sends it back afterward. Token
ownership then admits at most `N` active workers. Both patterns are demonstrated
in `examples/features/concurrency/bounded_workers.jet`.

### Bounded buffering law

Jet bounded buffers preserve accepted values and apply backpressure by default.
Only `AsyncEvent` may discard a payload, and only when its immutable
`AsyncPolicy` explicitly selects `DropNewest` or `DropOldest`; `Block` preserves
it by waiting. This split follows ratified roles: `tasks.channel` is typed work
transfer, where capacity bounds queued memory and active work, while `AsyncEvent`
is an asynchronous many-subscriber occurrence stream whose pressure choice is
explicit and observable in its dispatch report. No primitive gains a new
overflow knob here. See [D-EVENT2=A](syntax-decisions.md) and
[D-TASKRUNTIME1=A](syntax-decisions.md).

Both queue APIs use `capacity` for the numeric bound:
`tasks.channel<T>(capacity: N)` and
`AsyncPolicy.{ capacity: N, overflow: ... }`. Channel capacity applies
backpressure only; channels have no drop policy.

| Primitive | Full behavior | Buffering law |
|---|---|---|
| `tasks.channel<T>(capacity: N)` | `send` waits for receiver space; deadline or cancellation can wake the wait | Preserve work-queue values and FIFO; capacity bounds queued memory and producer pressure |
| `AsyncEvent<T, E>` | `Block` waits; `DropNewest` drops the new attempt; `DropOldest` drops the oldest queued attempt | Only explicit loss path; report exposes acceptance and terminal state |
| `core.services` worker mailbox | Full delivery waits under deadline or returns `Full` | At-most-once, per-sender FIFO; no silent drop |
| `core.files` buffered handles | Reader/writer calls block or flush; no Jet queue or drop policy | Bounded-memory stream; caller pace controls progress |
| `core.io` stream handles and `core.http` `Body` | Blocking reads/writes use OS or socket backpressure; body limits reject over-limit input | Transport streaming preserves accepted bytes; no overflow drop |
| `core.encoding` readers/writers and `core.data.DataStream` | Blocking `next`/`write`/`flush`; `EncodingLimits`/`DataLimits` bound retained work; no hidden queue or drop | Bounded pull/push stream, not lossy delivery |
| `core.log` sinks | No public bounded queue, capacity, or overflow policy; sink writes and `flush` are explicit | No buffering-loss rule today; sampling/disable are explicit emission controls |

`Deque`, `PriorityQueue`, `Cache`, and `ByteBuffer` capacity fields are storage
capacity, not concurrent producer/consumer buffering. Host-internal queues for
browser events, HTTP admission, observation, and tooling are implementation
limits, not Jet primitives. Any future buffered log sink or other lossy surface
needs an owner decision before adding a policy knob.

D-DEADLINE1 (ratified 2026-06-28): an ambient deadline can be set with
`#Context(deadline: <Int epoch_ms>) { … }`. Inside that scope, wait/IO points
observe the inherited budget (task joins, channel receive, `time.sleep`, TCP
read/write stubs). When the budget is exceeded, runtime report **E3003** is
emitted in Jet terms and execution exits with the runtime error code.

Teaching errors: **E0040** points `async`/`await` users at `tasks.spawn`;
**E0041** points `Mutex`/`lock` users at channels.

### Parallel collection adapters (D-PARCAPTURE1=D)

Lists expose one explicit parallel family: `para_map`, `para_filter`,
`para_partition`, and `para_fold`. The old provisional `par_*` spellings are
removed rather than aliased. Map and filter preserve source order. Partition
returns `(false_: [T], true_: [T])`, preserving source order within each side.

All four operations use one bounded indexed chunk engine. Chunk boundaries are
stable, worker count never exceeds available host parallelism, and scheduling
does not change result order. `para_fold(seed_factory, step, merge)` creates a
fresh accumulator for each chunk, steps through each chunk in source order, and
combines partial results with a deterministic adjacent-pair tree. An empty input
calls the seed factory once. The seed must be an identity for `merge`
(`merge(seed, x) == x == merge(x, seed)`), and `merge` must be associative;
otherwise the deterministic tree is still stable, but it does not define a
portable parallel reduction.

When the plan has one chunk, the engine runs that chunk on the caller thread.
This keeps small inputs serial and avoids paying for worker setup; the crossover
to a useful `para_map` speedup depends on item count, callback cost, and the host.
Run `jet bench examples/features/tooling/para_map_crossover_bench.jet` on the
same machine as the workload before choosing between `map` and `para_map`.
`jet bench` owns the optimized benchmark profile; do not compare its numbers
with debug or release builds.

The checked-in reference run (Linux x86_64, Ryzen 9 7950X3D, 32 logical CPUs,
three invocations) first favored `para_map` at 256 items with callback cost 256;
costs 1 and 32 did not cross within the matrix. This is a teaching example, not
a portable threshold.

If callbacks fail at more than one item, each chunk stops at its first failure,
all started chunks are joined, and the operation reports the original Jet
failure belonging to the lowest source index, independent of worker completion
order. It returns no
partial map, filter, partition, or fold accumulator. Effects already performed
outside the returned collection are not rolled back, so callbacks should keep
external effects explicit and synchronization-safe.

Sema rejects ordinary mutable captures, stored/imported callbacks whose capture
facts are hidden, and values that cannot be safely shared or transferred between
workers as **E1111**. Inline lambdas and top-level functions expose the required
facts. Function-typed items, results, and fold accumulators are not transferable
worker values and are rejected before code generation. There is no hidden
serialization or implicit capture merge; callers
return data, use `para_partition` or `para_fold`, or choose explicit synchronized
state.

### Task groups without a loop (D-VERDICT-1323-1)

`tasks.spawn_group(n, body) => [Task<T>]` starts `n` tasks from one callable.
Every single-handle method has a list twin on `[Task<T>]` that means the same
thing applied in list order:

| single | group | ownership |
| --- | --- | --- |
| `.join()` / `.wait()` | `.join_all()` / `.wait_all()` | consumes |
| `.detach()` | `.detach_all()` | consumes |
| `.cancel()` | `.cancel_all()` | borrows |
| `.pause()` | `.pause_all()` | borrows |
| `.resume()` | `.resume_all()` | borrows |
| `.trace()` | `.trace_all()` | borrows |

```jet
workers :: tasks.spawn_group(3, () => 7)
workers.cancel_all()
print(workers.wait_all())
```

`handles.join_all()` is the method spelling of `tasks.join_all(^handles)` — one
mechanism, two spellings, not two mechanisms. Example:
`examples/features/concurrency/task_group_helpers.jet`.

`.trace()` and `.trace_all()` render `paused=<bool>,cancel=<bool>` per handle.
Pause is cooperative on every tier: a paused task stops at its next wait point,
not mid-statement. Tier parity for the whole control plane — singles and twins,
AOT, `jet run`, and the interpreter — is held by `tests/task_control_tiers.rs`.

Iterating the handles yourself instead is a different thing: `loop h, hs { … }`
hands you each handle, which takes the list, so the loop must own it. A borrowed
list is **E0120** and points back at these methods.

### Taskgroups and structured combinators (D-TASKSCOPE1, D-TASKGROUP-PARAM1, D-CONCCOMB1, D-RACEWIN1, D-CONCSELECT1; verified 2026-07-29)

Structured concurrency uses a scoped `taskgroup` (D-TASKSCOPE1=A). Inside
`taskgroup g { … }`, `g.task => expression` or `g.task => { … }` spawns a
child owned by the
group. Unjoined handles at scope exit are cancelled and joined before the block
returns.

`TaskGroup` may also be written as a direct parameter of a named function
(D-TASKGROUP-PARAM1=A). This lets a lexical group flow down the call stack:

```jet
fn add_work(group: TaskGroup, value: Int) {
    task :: group.task => value + 1
    print(task.join())
}
```

A spawn through a `TaskGroup` parameter may capture only copied or moved owned
values. It may not capture a `view`: the group joins in the caller's frame, so
this frame cannot prove a borrowed owner outlives the loan. `TaskGroup` remains a scoped authority, not
a general value: it is illegal in fields, returns, local declarations, lambda
parameters, and escaping closures. The parameter carries the lexical group's
internal collector. A task spawned by a helper therefore remains owned by the
caller's group and is cancelled and joined at that outer scope's exit.

#### Borrowed captures in a group child (D-TASKBORROW1=A)

A lexical group joins every child before its block returns, so a child may
borrow places the owner still gives access to — the loan opens before the child launches
and closes at the join. Reads are borrowed freely. A write borrow is admitted
only where the compiler proves the places never overlap; distinct fields and
distinct constant indexes are disjoint, and anything dynamic is treated as
overlapping. Two children reaching one place is **E1101**.

```jet
taskgroup g {
    left :: &particles[0]
    right :: &particles[2]
    a :: g.task => { left.position += left.velocity; left.position }
    b :: g.task => { right.position += right.velocity; right.position }
    print(g.all([a, b]))
}
```

A group lends only what outlives its own join. An owner declared inside the
group's own block drops before the group joins, and a group reached through a
`TaskGroup` parameter joins in another frame; both stay **E1102**. Detached
tasks, channels, and `tasks.spawn` are unchanged — they still require ownership.

Combinators are methods on the group handle only (no detached work):

| Operation | Completion and cancellation |
| --- | --- |
| `g.all([t1, t2, …]) => [Task]` | Every task must succeed. Fail-fast cancels siblings and exits with `panic: a task panicked` (example `169_all_failfast.jet`). |
| `g.race([t1, t2, …]) => T` | The first **successful** result wins. Losers are cancelled (D-RACEWIN1; example `167_race_cancel.jet`). |
| `g.any([t1, t2, …]) => T` | The first **completion** wins, including errors. |
| `[Task<T>].join_all()` / `.wait_all()` | Both methods consume the list and return results in list order. They use the same fail-fast rule as `g.all`: a failure cancels remaining siblings. |
| `[Task<T>].cancel_all()` | The method borrows the list and requests cancellation for every task. It does not select a winner or loser and does not wait. Each task unwinds at its next wait point under D-CANCELMODEL1. |

`.join_all()` and `.wait_all()` therefore cancel remaining siblings and fail
fast like `g.all`; `.cancel_all()` is explicit cancellation of every task, not
loser selection.

- Waiting on several sources at once — a select — is a subjectless `if` table
  whose arm heads are a binding and a source (D-CONC-CHAN2=D; amends
  D-CONCSELECT1=A's fluent builder and D-CONC-CHAN1's spelling of it). The
  comma head marks the wait; a Bool head in the same table is a registered
  diagnostic. `after` takes a Time delta and fires when no source is ready by
  that deadline; an optional `else` arm makes the wait non-blocking. The whole
  table compiles to one wait, so there is no test-then-read race:

```jet
if {
    job, jobs    -> handle(job)
    msg, control -> obey(msg)
    after 100ms  -> retry()
}
```

Cancellation at the wait follows D-CANCELMODEL1=C. Example: `select_channel.jet`.

The M:N scheduler (D-ASYNCRT1=A) parks tasks at channel/timer/IO waits instead
of blocking OS threads. Native I/O pollers: Linux `epoll`, macOS/BSD `kqueue`,
and Windows IOCP. The Windows backend registers sockets with a completion port,
handles completion, cancellation, deadlines, stale packets, scale, cleanup, and
terminal poller failure without a portable-poll fallback. Task-local Jet traps
unwind into the scheduler so sibling combinators can
report `panic: a task panicked` instead of exiting the whole process early.

Scale tests: `scheduler_spawn_10000_tasks` in CI; `scheduler_spawn_100000_tasks_bench`
is `#[ignore]` for local 100k stress.

## Modules — `module name { … }` (U3, unified-ecosystem §4–5; parser, Stage 1a)

A module is a named, composable top-level declaration that contributes typed
values to reserved namespaces. Many modules may share a file.

```ebnf
module      = "module" dashed-name "{" contribution* "}" ;
contribution = namespace "." dashed-name ":" expr [","] ;
namespace   = "env" | "image" ;
dashed-name = ident { "-" ident } ;                (* S84: kebab-case names *)
```

- **Dashed names (S84):** package / module / image / env **names** may
  be kebab-case — `module web-app`, `env.web-tools`, `image.halcyon-oci` —
  matching nixpkgs/npm convention. A `-` joins two segments only when it is
  *span-adjacent* to both (no surrounding whitespace), so a spaced `a - b` stays
  subtraction; this is a parser rule (`expect_dashed_name`), not a lexer or
  expression-grammar change. Code identifiers (variables, fields, types,
  functions) stay plain `ident`. No leading, trailing, or doubled hyphen.
- **Internal with a leading underscore:** automatic discovery skips
  `module _name { … }` based on the declared name, not its filename or scan
  order. An explicit `use project._name` remains allowed under ordinary
  visibility rules and resolves the declaration name regardless of filename.
  The underscore changes discovery, not access; rename it to `module name` to
  opt back in. `jet project parts --skipped` lists omitted declarations;
  duplicate declared names are conflicts even when omitted
  (D-SHAPE-MODULEINTERNAL1=A).
- **Active reserved namespaces** are `env` → `Env` (dev environment),
  `system` → `System` (jetos host), and `image` → `Image` (OCI container image
  or jetos installer input).
- **`env.<name>:` values reuse the ordinary expression parser** — typically a
  struct literal (`Env.{ packages: […], prompt: "…" }`), so lists and strings
  work with no new grammar. `prompt: "name"` is the beginner shorthand. For
  prompt depth, write `prompt: Prompt.{ label: "name", path: .Short, strip: .On }`
  (`path: .Full` and `strip: .Off` are the other modes). `jet env` renders that
  as one hybrid prompt: label plus path by default, `Ctrl-G` status glance on
  demand, and the optional strip showing the same status words.
- **Auto-activation hook (D-ENVHOOK1=A):** `jet env hook <shell>`
  (`bash`/`zsh`/`fish`) prints a one-line, opt-in shell hook the user installs
  once (`jet env hook fish | source`, or a line in the shell config). After
  that, entering any directory whose tree carries an `env.jet` activates that
  env — the same packages, `PATH`, and prompt as `jet env` — and leaving the
  tree restores the shell exactly as it was. The first activation of an
  untrusted, trust-sensitive env prompts through the ordinary D-JPK-GRANTCMD1
  trust gate (never on `cd` into a project you already trust); explicit
  `jet env` stays available for one-off shells and anyone who declines the hook.
  Set `JET_ENV_DISABLE` to any non-empty value to suppress activation (and drop
  any active env) in the current shell. The hook re-checks on each prompt via a
  private `jet env export <shell>` callback that emits nothing until the current
  directory crosses an env boundary, so it is a no-op on the vast majority of
  prompts.
- **`image.<name>:` values are Jetpack OCI images.** Active fields are
  `kind: .Oci` (optional when `from: packages.<name>` makes it clear),
  `from: packages.<name>`, `expose: [Int]`, `env_vars: ["KEY": "value"]`,
  `files: [String]`, and `base: oci("<ref>")`. `base:` is captured but not yet
  realized because registry-pull is gated on TLS/native-client work. `.Iso`,
  `.Qcow`, `.Raw`, and `from: system.<name>` are jetos installer inputs handled
  by `jet os image`, not by `jet image`.
- **Ad-hoc adapters (U20):** an `env.<name>.packages` list may contain
  `Pkg.adapt(name:, source:, deps:, recipe:)`. `source:` is a provider ref such as
  `"./vendor/tool"`; each `deps:` package is realized and its verified executable
  members are the only tools available to a `Recipe.build` `.exec` step. Jetpack realizes `Recipe.copy()`,
  `Recipe.prebuilt(bin:, as:)`, and finite `Recipe.build(steps: […])` actions
  (`.fetch`, `.exec`, `.install`, and `.install_tree`) into ordinary hangar
  packages, with the same store/lock path as any other package. `jetpack add
  <ref> --adapt` prints a draft adapter and does not run upstream code.
- **Direct ecosystem providers (D-JPK-PROVIDERS2):** LuaRocks uses the exact
  `<name>#version=<version>@luarocks` root. Jetpack resolves the repository
  manifest and rockspec dependency closure, verifies every source SHA-256,
  records the qualified ref and closure in `.jet/lock`, and projects the
  realized `LUA_PATH`/`LUA_CPATH` into the environment. Mutable refs, unsupported
  platform/native build metadata, dependency cycles, unsafe archive paths,
  source drift, and cache tampering fail closed. Offline reuse re-verifies the
  sealed Hangar output without contacting the repository.
- **No-Nix machines (U23):** core packages and adapted packages realize without
  Nix. Package refs that still route through the Nix compatibility provider are
  reported together as E1272, naming only those holes and suggesting either
  installing Nix or drafting an adapter with `jetpack add <ref> --adapt`.
  Foreign-flake projection uses the bounded native evaluator. Unsupported
  expressions remain E1256; Jetpack does not delegate this path to an
  installed `nix` binary.
- **Offline discovery (U26):** `jet search <query>` and
  `jet info <source>.<package>` read only `.jet/discovery/index.jsonl`, local
  provider fixtures, and hangar metadata. They never fetch. `--json` emits the
  same package records the editor discovery hooks consume: source, name,
  resolved ref, version, platforms, docs, provenance, and typed service option
  fields.
- **Failed-build debugging (U27):** recipe-backed builds persist per-step logs
  under the hangar. On failure, scratch is preserved in hangar-managed storage
  and the diagnostic names `jet logs <pkg>` plus `--shell-on-fail`. Package-form
  `jet explain <ref>` prints the latest resolution/build record; code-form
  `jet explain E1234` keeps the existing diagnostic essay behavior.
- **Hybrid CLI output (D-FE-CLI1):** trivial reads stay quiet, multi-package
  realization/build work reports dependency-chain progress, and mutations plan
  before applying. Plan rows use `+`, `-`, and `~` in both colored and plain
  output. `-y` and `--yes` are the same confirmation bypass; non-interactive
  mutation without either prints the plan and changes nothing. Diagnostic text
  and JSON output remain unchanged.
- **Package overlays and overrides (D-JPK-OVERLAY1):** `workspace.jet` may carry
  reviewed overlay policy inside `module workspace`. An `overlay <name> { ... }`
  block records provider/channel swaps (`provider: Provider.nixpkgs(channel:
  "plasma-beta")`), per-package source/version/flag/patch changes
  (`package("foo").patches += [patch("patches/foo.patch")]`), and package-local
  `allowUnfree`. Workspace-wide unfree review uses
  `policy.allowUnfree: ["discord"]`. `jetpack override draft <ref> --patch
  <file>` only writes that source policy; it never creates hidden state. Patch
  application is deterministic unified-diff application against the source tree,
  and `jetpack explain package-overlay:<overlay>:<package>` prints provider,
  channel, policy fingerprint, and update command from the same source policy.
- **No daemon / no root (U28):** jetpack is a one-shot, user-owned process:
  no resident daemon, no root-owned default hangar, no privileged sandbox
  helper. Concurrent commands coordinate through file locks. If unprivileged
  sandboxing is unavailable, Jetpack emits L0205; `jetpack config sandbox
  require` makes that condition E1275 instead.
- **Universal trust grants (D-JPK-GRANTCMD1/SCHEMA1):** `jet trust` is the
  public command family for the unified grant graph. `jet trust list` shows
  package, build, env, service, image, fleet, and jetos authority grants;
  `jet trust explain [<grant>]` expands exact authority and revocation keys;
  `jet trust grant <grant> [--scope user|repo]` records a reviewed local grant;
  `jet trust revoke <grant>` removes it so the next risky action asks again.
  The store remains backward-compatible with U19 `hash:` and `pattern:` lines.
  `package.jet` may carry reviewed source policy as
  `policy: { trust: { default: prompt, ci: { prompt: deny }, services: { postgres: prompt } } }`.
  Policy decisions are `allow`, `prompt`, or `deny`; unknown fields are a
  manifest error.
- **The override law (D-LINTPOLICY1=A):** warnings and lints never fail a
  build by default — errors stay reserved for programs Jet cannot compile
  safely or unambiguously (I1 memory/type safety has no override and is
  outside this law). Every bypass is spelled at the site or on the command
  line, never in hidden config, and lands in the audit record (`jet inspect
  dossier`, effect-budget provenance, build facts). Walls are team policy
  only: `package.jet`'s `policy: { lints: { deny: […] } }` joins `policy.trust`
  under the one `policy:` namespace (D-JPK-POLICYSURFACE1) — `deny:` lists
  lint codes (e.g. `L0504`), and a lint that fires while its code is listed
  fails the build with E1293 instead of only warning. Absent entirely, every
  lint stays a warning (I1/D-LINTPOLICY1 default); host/org policy narrows,
  never widens (already law). This is the one policy surface for lint walls
  — no per-call flag or attribute may duplicate it (I8).
- **Offline guarantee (U29):** once a package ref is realized into the hangar,
  realize-class verbs can use it again with `--offline` and no provider
  metadata refresh. Network-class verbs (`add`, `update`, `outdated`,
  publish/cache sync) refuse `--offline`, and a missing local object reports
  E1276 instead of fetching or timing out.
Stage 1a is parser-only for the AST shape; the jetpack module evaluator
(`Source/Jetpack/ModuleEval.rs`) gives these contributions meaning (field-checking +
capture into a plan model). The U5 merge engine consumes `env` contributions.

### Jetpack Services And Secrets

- **Dev services (D-JPK-SERVICE1):** an `env.<name>` role-module may carry
  `services: { name: { enable: Bool, ... } }`. These are project-local
  processes managed by Jetpack for the dev loop (`jetpack services
  up/down/health/logs`). They are not system services and do not imply jetos
  activation.
- **Secrets (D-JPK-SECRETCRYPTO1):** an `env.<name>` role-module may
  declare `secrets: ["name", …]` — a plain `[String]` list, no dedicated
  grammar. Each name is one this env expects to find in the project's
  encrypted repo store (`.jet/secrets.age`, managed by `jetpack secrets
  set/get/recipients/keygen`); reading one at runtime is `core.vault.get`,
  gated by the `Secret` effect (**E1264** if ungranted) and unconditionally
  denied at build/comptime time (**E1265**, no `#Impure` escape hatch).
  `jetpack secrets get <name>` on a name absent from the store is **E1263**.
- **Typed vault keys (D-CRYPTO-VAULT1=A):** `core.vault` persists only
  `SigningKey` and `X25519SecretKey` behind immutable `KeyRef<T>` handles.
  Reads, preparation, authorization, and commits all require `Secret`.
  Mutation is a three-step compare-and-swap: prepare a five-minute move-only
  `MutationPlan<T>`, authorize its exact native preview into a one-use
  `VaultWrite<T>`, then consume write and plan in that order. Rotation creates
  the next generation and retires the prior active generation; exact retired
  refs still load, while revoked refs fail before key bytes are copied.
  String secrets and typed keys share the age-encrypted `.jet/secrets.age`
  artifact but occupy disjoint namespaces. Its authenticated plaintext is the
  canonical bounded `JVLT` version-2 format; the first authorized mutation
  migrates historical String rows without inferring keys from them. There is
  Safe portable backup uses the canonical `JVKW` v1 envelope: recipient mode
  wraps an independent backup key for 1–16 sorted X25519 recipients, while
  passphrase mode uses fixed Argon2id parameters. Both modes authenticate the
  source repository/name/generation/record hash and the concrete key type.
  Import decrypts into a short-lived `WrappedImportPlan<T>`, then reuses native
  authorization and consuming compare-and-swap commit; same-origin imports are
  idempotent and revoked origins never reactivate. All secret-dependent open
  failures collapse to `KeyWrapError.OpenFailed`. Raw 32-byte import is available only
  through `core.vault.expert` inside an audited `#Unsafe` region. Headless
  mutation requires `jet trust grant vault.write:<repository_uuid>`; source,
  workspace, environment, DAP, and stdin are never write authority.

### jetos Runtime Slice

`jet os check|init|plan|proof|build|switch|rollback|generations|lift|import|image|vm`
is active. A bare host (`jet os switch laptop`) selects `system.laptop` in
`./config.jet`; `laptop@../machines` selects an exact external root. Builds create named
generations; `generations` lists newest first; `switch --name <name>` overrides
the automatic name; `rollback` activates a prior generation. `plan` prints the
checked system plan without building. `proof` reads the latest generation's
plan, proof, provenance, health, boot, init, secrets, VM, and rollback facts.
The current slice records a root `sw/bin` package closure, hangar/cache facts,
systemd service/timer/socket units plus target wants, users/groups, filesystems and swap,
networkd/firewall/wireless facts, Limine + CachyOS boot facts, first-party
systemd init closure with `/sbin/init`, kernel firmware/driver facts, desktop/display-manager facts,
terminal login facts with serial/virtual getty units and user home/profile
projection,
NixOS/flake-parts/Home Manager import through `jet os import <flake-or-dir>`
with semantic `jetos-import-facts.json` input and audited facts-only fallback,
per-user generation profiles under `users/`, Flatpak exact reconcile plans,
permission overrides, undeclared-app removal, and AppImage runtime integration under
`flatpak/`/`appimage/`, performance profile, sysctl, zram, sched-ext scheduler, initrd, and
bootloader tuning proof under `performance/`, option priority
explain output under `module-system/`, storage plans, fstab projections,
safe-by-default `jetos-storage-apply`, and `jetos-persist-activate`
impermanence proof under `storage/`, container/microVM workload plans with
mounts, secrets, resources, health, and rollback proof under `workloads/`, hardware scan source,
profile manifests, boot specialisation entries, and `jetos-hardware-scan` /
`jetos-hardware-doctor` commands under `hardware/`, reusable theme projections under `theme/` and concrete GTK,
Qt, terminal, editor, display-manager, and Studio preview files, and
`jetos-service-logs` journal/fallback log query support under
`service-manager/`,
fleet deploy plans plus generated `jetos-fleet-deploy` host scripts under
`fleet/`, runnable workload launchers under `workloads/`, a generated
`jetos-flatpak-reconcile` command for remotes/apps/permissions/removals,
`jetos-appimage-run` for AppImage desktop integration, lifecycle channel policy,
proof-gated auto-upgrade service/timer, rollback-on-health-fail proof, and
explainable `jetos-lifecycle-gc` retention scripts under
`lifecycle/`, option priority/explain output under `module-system/` with
winner/loser contenders and disabled-module manifests, typed option
reference/search artifacts under `options/` including type, default, example,
doc, tier, priority, and provenance plus exact/explain search modes, and image
variant artifacts under `systems/images/`: qcow2, raw, SD-card image, and a
netboot bundle with kernel/initrd/iPXE config plus
`jetos-image-variants-<host>.proof.json`,
first-wave `apps.program.*` modules under `apps/programs/` for git, ssh, fish,
starship, ghostty, helix, yazi, btop, bat, eza, fzf, zoxide, ripgrep, tealdeer,
fastfetch, VS Code, Cursor, Discord, Spicetify, and browser policy projection,
plus `jetos-app-module-apply`,
desktop breadth projections for PipeWire/rtkit, GNOME and Plasma Wayland
sessions, libvirt/SPICE/swtpm/USB redirection, GameMode/Steam/Proton policy,
locales/keymaps, XDG mime defaults, fonts, smartcard pcscd, and AppImage binfmt,
owner-`~/nixos` acceptance coverage matrix, VM gate list, no-omission diff, and
`jetos-acceptance-prove` proof under `acceptance/`,
repo-ciphertext/host-key tmpfs-only secret activation proof, guided-ext4 disk
intent with `--manual`, and `jetos` hybrid ISO media staging/proof. When pinned
xorriso, Limine, zstd, QEMU, and filesystem tools are present, `jet os image`
writes the ISO artifact, the qcow2/SD/netboot variant proof, and records the
exact tool paths in proof JSON.
Generated terminal profiles set `JETOS_BRAND=JetOS` and a clean `JetOS <host>`
prompt for login shells and VM run-mode shells; `/etc/issue` and `/etc/motd`
carry the same light host branding.
Fleet deploy scripts default to tar-over-SSH staging, remote proof before
switch, health check, and rollback-on-fail; tests may replace those commands
with local hooks, but the generated default is a real push path, not a proof
label. Lifecycle GC is explain-before-delete by default and deletes old
generation directories only when invoked with `--apply`.
Installer media copies the generation as a self-contained tree; host-root
symlinks are dereferenced before the ISO is built so the guest install does not
depend on the build machine's paths.
Compatibility escape
hatches such as overlays and specialArgs are allowed only as explicit
`packages.*` options; each one is written to the generation's compat audit file
and provenance so Studio can show it and native replacement work can track it.
`jet os vm prove <host> --disk <path>` is the install/reboot proof entrypoint;
`--real` upgrades it to replacement acceptance and rejects script/fake VM tools.
it fails with E1279 rather than faking boot media when pinned QEMU/media tools
are missing, and it fails with E1285 rather than treating a prepared QEMU harness
as a passed guest proof. It writes the VM proof harness, runs the recorded QEMU
create/install/reboot phases, captures a `JETOS_GUEST_PROOF:` marker from the
installed guest's serial output, and writes the guest proof artifact. The QEMU
installer phase boots the hybrid ISO with `console=ttyS0`; the ISO's Limine
entry carries `rdinit=/jetos/init`, `jetos.mode=install`, and the target
disk. The installer writes a GPT disk with a FAT ESP, ext4 `jetos-root`,
`EFI/BOOT/BOOTX64.EFI`, the kernel/initrd, and an installed Limine config. The
installed-disk verifier phase boots that disk through firmware/Limine with
`rdinit=/jetos/init`, `jetos.mode=verify`, and the installed root label. A rerun
may promote the harness to `guest-passed` only when the guest proof records the same
host, generation, disk, media proof, tool hashes, and required guest assertions;
harness JSON names the expected guest proof path, command argv, and per-phase
run logs. The proof then boots a graphical desktop verifier with QEMU VNC,
stdvga, and the packaged `bochs` DRM module; promotion requires the guest to see
a framebuffer (`fb0`) and execute the generated display-manager,
desktop-session, and terminal-fallback launchers in proof mode before it emits
`graphical-console-ready` and `desktop-launchers-run`. The required guest
assertion set includes terminal-login readiness, desktop-session readiness,
graphical-console readiness, and launcher readiness:
the installed generation must carry `terminal/facts.json`, `/etc/profile`,
`/etc/shells`, enabled `serial-getty@ttyS0.service`, and the user home profile
inside the root projection, plus `desktop/facts.json`, the GNOME Wayland session
launcher, the display-manager unit, the terminal fallback launcher, the installed
jetos Studio app, a guest-visible graphical framebuffer, and launchers that
resolve GNOME/GDM commands from the installed system closure before VM proof can
pass. The
ratified interactive launch surface is `jet os vm run <host> --disk <path>`;
it opens only a disk already tied to the latest generation by a `guest-passed`
VM proof, exposes a graphical VNC console, and attaches serial output to the
current process. It fails with E1287 rather than launching an unproven qcow2. The
default CachyOS kernel is a first-party `cachyos-kernel`
source-built package carrying recipe/config/patch/initrd-input hashes beside the
kernel and initrd artifacts; missing kernel package provenance is E1280, missing
bootable kernel/initrd artifact headers are E1282, missing source recipe
or builder provenance is E1284, and a failing `source/build.sh` bootstrap build
is E1286.
The build script is package-internal and authoritative: when the source recipe
is present, `source/build.sh` runs before boot validation even if stale boot
files already exist. `JETOS_KERNEL_SOURCE`, `JETOS_KERNEL_OUT`, and
`JETOS_KERNEL_PACKAGE` point at the realized first-party package, and the script
must write the kernel and initrd artifacts that the generation, installer, and
VM proof will boot. Installer media appends a JetOS initrd overlay containing
`/jetos/init`, `/jetos/install.sh`, and `/jetos/guest-verify.sh`; `/jetos/init` dispatches
`jetos.mode=install`, `jetos.mode=verify`, and `jetos.mode=desktop-verify`,
mounts proc/dev/sys before reading cmdline, probes `LABEL=jetos-root`,
`/dev/vda2`, and `/dev/sda2` before falling back to install mode, and the ISO
Limine config enters this dispatcher. The hybrid ISO
carries both BIOS Limine boot files and a FAT `boot/efiboot.img` ESP with
`EFI/BOOT/BOOTX64.EFI`, so QEMU/OVMF and physical UEFI firmware boot the same
installer artifact. The graphical verifier phase still direct-boots the same
kernel/initrd with `rdinit=/jetos/init` so it can force a VNC/stdvga display for
desktop proof; the installed-disk verifier uses firmware disk boot. The verifier
emits the serial guest-proof marker. The default systemd
init path requires a first-party `systemd` package; missing init provenance is
E1281. Each generation defaults to the ratified GNOME-on-Wayland desktop profile
with terminal login as the secondary fallback. The display-manager service
launches the system session when GNOME/GDM are present and falls back to the
terminal launcher instead of blocking boot. Each generation also installs the
first-party jetos Studio app projection under `sw/bin/jetos-studio`,
`share/applications`, and `studio/`;
`studio/data.json` carries the read-only host/package/service/option projection
artifact paths, no-plaintext secret policy, adaptive fleet surface, separate-app
Canvas deep-link metadata, and changeset apply gates. The root projection carries these files into
`/run/current-system`. Studio remains a separate jetos system app from Canvas
and may fall back to the browser over the same local projection service. It may
deep-link to Canvas for generic source graph editing, but it stores no Canvas
semantic state. The
direct launch command is `jetos studio`; `jetos studio --headless` prints the
installed app path for CI/review without opening a browser, and `--json` prints
the root, app, metadata, and data paths without opening a browser.
`jetos studio --serve <loopback:port>` serves `index.html`, `app.json`, and
`data.json` over the local browser fallback. `GET /studio/source` serves the
selected `config.jet` for the adjacent source pane. The same local service
accepts source transactions at `POST /studio/transaction`; the first implemented
transaction is `set-option`, which returns an exact source diff and writes
`config.jet` only when `write:true`. `POST /studio/run` executes the matching
`jet os check|plan|build|proof|generations` action from the selected
project/host and returns captured output, so Studio never substitutes hidden
state for CLI proof. The build action writes a named Studio candidate generation
before proof.

`module vmtest.<name>` declares a JetOS VM scenario. Its `hosts:` map names
scenario handles bound to `system.<host>` declarations, and `run: test { ... }`
captures typed host-handle assertions such as `wait_for_boot`,
`assert_unit_active`, and `assert_port_open`. `jet os vm test <name> --disk
<path>` evaluates that scenario through the same installer/reboot proof harness
as `jet os vm prove`, writes one host proof per scenario host, then records
`systems/vm-tests/<name>-vmtest-proof.json` with the source test body,
assertion method facts, host generations, disks, and proof artifact paths.

`jetos user plan|build|switch|rollback|prove <name>` is the standalone
per-user path. It selects a `user.<name>` or `users.<name>` profile from
`config.jet`, renders the same `users/<name>/profile.json` artifact used by
`jet os switch`, and builds/activates/proves it through normal named
generations rather than a separate hidden state store. The generated
`jetos-user-apply <name>` command applies that profile to a home directory:
projects declared files, links package binaries into `.jetos/profile/bin`,
writes user service units under `.config/systemd/user`, and records
`.jetos/proof/user-<name>.json`.

## Fixed-size list `[T#N]` (S76)

`[T#N]` is a type refinement meaning "a list of exactly N elements of type T."
It can be destructured with an exact-count pattern.
At codegen it erases to `Vec<T>` (same as plain `[T]`).

```ebnf
type_fixed_list = "[" type "#" int_literal "]" ;
```

```jet
result :: [Int#3].{2, 4, 6};
[a, b, c] :: result;   // OK — 3 names for 3 elements
```

- Destructuring a `[T#N]` with the wrong number of names is **E0963**.
- Calling `push`, `pop`, `insert`, `remove`, or `clear` on a `[T#N]` is **E0964**.
- A literal index outside `0..N-1` on a `[T#N]` is **E0965** (compile-time check).
- A `distinct Int` with `#Invariant("value >= lo && value < hi")` may index a
  `[T#N]` without a runtime bounds check when `lo >= 0` and `hi < N`.
- `[T#N]` is accepted wherever `[T]` is expected (widening coercion); the
  length information is erased at that point.

## Effect system (D-EFF1, D-QUAL1, D-EFF2, D-EFF3)

Every function carries an **effect set**: the categories of ambient power its
body exercises — touching the network, the filesystem, the clock, and so on.
The set is **inferred**, never declared by default, **propagated along calls**
(a caller's set includes every callee's set), and **fully erased in codegen**
(I3) — effects are a compile-time proof, with no runtime value, handler, or
monad. A `fn … =[]=>` is exactly the function whose inferred set is empty.

### The effect vocabulary

Effects are a closed, compiler-known set of PascalCase tags (D-CASING1). Each
primitive Core operation contributes one effect; an effect appears in a
function's set when the function reaches an operation that carries it.

Packages can name precise leaves with a top-level compile-time declaration:

```jet
effect Log.Audit
```

The package view merges its declarations with loaded dependency and Prelude
declarations. After a root has any declared leaves, dotted uses under that root
must match a declaration exactly. Bare roots remain valid, and a root with no
declared leaves remains open. The same check applies to function effect rows,
`#Caps`, `#Grant`, and package effect budgets. Declarations have no runtime
representation.

| Effect  | Carried by |
|---------|-----------|
| `IO`    | `print`, `eprint`, `input`, `read_all_input`, `core.io.*` |
| `FS`    | `core.files.*` (whole-file helpers and streaming handles), `core.watcher.files` |
| `Net`   | `core.net.*`, `core.http.*`, `core.watcher.port` |
| `Time`  | ambient `core.time` clock/zone reads (`now`, `now_utc`, `today`, `instant`, `zone`, `sleep`, `start`) |
| `Rand`  | `core.random.*` |
| `Env`   | `core.env.*` |
| `Exec`  | `core.process.run`/`exit`/`cmd`/`pipeline`, `ProcessSpec.run`/`spawn`, `ProcessChild` wait/control/stream calls, `core.watcher.process_pid` |
| `DB`    | `core.db.*`; leaves (D-EFFDBREAD1): `conn.query`/`conn.query_one` carry `DB.Read`, `conn.execute` carries `DB.Write`, `begin`/`commit`/`rollback`/`close` and `open`/`open_memory` keep the bare `DB` root |
| `Log`   | `core.log.*` |
| `GPU`   | `core.raylib.*`, future `core.gpu.*` / `core.game.*` |

A call to an `extern rust`/C foreign function, whose body the compiler can't
inspect, contributes the **maximal** set (every effect) — it is assumed to do
anything. This keeps inference sound without reading foreign code.

### Cryptography (D-CRYPTO-API1)

`core.crypto` owns opaque, move-only `Secret`, `SigningKey`,
`X25519SecretKey`, and `SharedSecret` values. They do not support ordinary
equality, cloning, hashing, printing, Display/Debug interpolation, reflection,
or serialization. Compare with constant-time operations. Raw bytes leave an
opaque value only through the explicitly named expert exposure functions.

The complete expert surface is `xchacha20poly1305_seal/open` (32-byte key,
24-byte nonce), `aes256gcm_seal/open` (32-byte key, 12-byte nonce),
`ed25519_sign`, `ed25519_verify_strict`, `x25519`, `hkdf_sha256`, `argon2id`,
`secret_bytes`, `signing_key_bytes`, `x25519_secret_bytes`, and
`shared_secret_bytes`. Every call is lexical `#Unsafe`; importing the module
does not weaken the gate. AEAD authentication failures collapse to
`CryptoError.OpenFailed`. X25519 rejects all-zero shared secrets by default.
HKDF-SHA256 output is at most 8160 bytes. Expert Argon2id accepts 8192–262144
KiB, 1–10 iterations, 1–8 lanes, `memory >= 8 * lanes`, and
`memory * iterations <= 1048576`; salts are 8–64 bytes and outputs 16–64
bytes. These failures use the same `CryptoError` family as the safe API.

`crypto.file_seal(recipients, source, destination)` and
`crypto.file_open(&recipient, source, destination)` use the recipient-based
JETC v2 envelope ratified by D-CRYPTO-ENVELOPE2. The fixed prefix is `JETC`,
version 2, kind 1, suite 1, flags 0, followed by little-endian header and body
lengths. The authenticated header carries a 16-byte file id, ephemeral X25519
public key, 16-byte nonce prefix, the fixed 1 MiB chunk size, 1–256 canonical
recipient stanzas, no metadata, and its tag. Body records carry a little-endian
length, final flag, ciphertext, and tag. Non-final records are exactly 1 MiB;
there is exactly one final record, including an empty final record after an
exact multiple. Readers cap all declared sizes before allocation and accept
safe-open v2 only.

Sealing snapshots and revalidates a no-follow regular source before requesting
envelope randomness. Seal and open stream one authenticated chunk at a time,
poll cancellation between chunks, zeroize secret and plaintext buffers on every
exit, and publish with atomic no-overwrite semantics only after authentication
and durable staging. Identity, framing, recipient, and authentication failures
from safe open collapse to `FileCryptoError.OpenFailed`; no failure publishes a
partial destination. The current native bridge supplies this runtime on Linux.
Other targets fail closed and do not claim JETC filesystem support. The
ratified Windows delete-on-close and rename implementation remains required
before any future cross-platform completion claim; it is not part of #526's
entropy-adapter work.

### HTTPS client default (D-TLS1)

`core.net.fetch` and `core.http.client` support `https://` in the default build
through the rustls bridge and system certificate roots. Plain `http://` remains
available for loopback fixtures and old endpoints. HTTPS client failures are
reported in Jet terms: E4201 for handshake failure, E4202 for certificate trust
failure, and E4203 when the host image has no usable certificate roots.
Advanced client TLS configuration belongs in `core.tls` (custom roots,
pinning, client certificates). D-TLSSERVE1=A adds HTTPS serving as a named
option on the same server entry point: `Server.serve(addr, mux, tls:
Server.tls(cert, key))?`. The third argument must be labeled `tls:`; unlabeled
TLS config is rejected so the transport switch is visible at the call site.

### Graphics and games (D-RAYLIB1, D-GAME1-3)

`core.raylib` is the first-party graphics bridge package. The typed surface is
`window_open`, `window_should_close`, `window_ready`, `begin_drawing`,
`clear_background`, `draw_rectangle`, `draw_text`, `end_drawing`,
`close_window`, `key_down`, `set_target_fps`, and `color`. By default the
bridge runs headless so CI does not need a display server. With
`JET_RAYLIB_DISPLAY=1`, generated code
dynamically loads the native raylib shared library and calls the real C API; if
the library is absent, it degrades to the same headless path.

`core.game` is the flagship engine name (D-GAME2=A). Its public beginner API is
scene-first with a frame hook (D-GAME3=C): a `Scene` owns durable editable game
data, while `scene.on_frame((frame) => { ... })` attaches per-frame logic.
The current Core floor is headless and deterministic: `game.Scene.new`,
`scene.assets.image`/`sound`, `scene.input.bind`, `scene.component<T>()`,
`scene.query<T...>()`,
`game.Replay.record`, `game.Backend.headless`, and
`game.run(scene, replay: replay)` produce a stable transcript without renderer,
audio, editor, or file-backend dependencies.

### Declaring a boundary — effects inside the arrow

A function may omit an effect row. Sema still infers its complete transitive
row. Ordinary `=>` defines the callable result and never claims purity. Public API
metadata stores that normalized inferred row, so publishing rejects effect drift.

A function may pin an **upper bound** on its effects by writing
`=[E1, E2, …]=>` between its parameter list and return type:

```ebnf
fn_effects = "fn" ident "(" params ")"
             [ ( "=[" [ effect { "," effect } ] "]=>" | "=>" ) [ type ] ] block ;
```

```jet
fn load(path: String) =[FS]=> String {
    core.files.read(path)?     // OK: FS ⊆ {FS}
}
```

The compiler infers the body's real effect set and checks it is a **subset** of
the declared bound. An effect the body uses that the bound omits is **E0740**,
naming the effect, the call that introduced it, and the declared set. The row is
an assertion the author makes a contract — the inferred set may be *smaller*
than the bound (the bound is a ceiling, not an exact set), but never larger.

`=[]=>` is the same contract with an empty bound: any effect at all is a
purity violation (reported as **E3401**, the established purity diagnostic).

Effects are erased: `=[FS]=>`, `=[]=>`, and an unannotated function with the same
body all generate byte-identical Rust.

### Restricting a region — `#Caps(…) { … }`

Where `=[…]=>` bounds a whole function, `#Caps(…) { … }` restricts a **block**.
Inside the region, the only effects allowed — directly or through any call it
reaches — are the ones listed; anything else is **E0741**. It is a hard local
ceiling, not a grant: the effects still happen and still count toward the
enclosing function's set.

```ebnf
caps_region = "#Caps" "(" effect { "," effect } ")" block ;
```

```jet
fn run() {
    #Caps(FS, IO) {
        text :: core.files.read("x") ?? "";   // FS — allowed
        print(text);                            // IO — allowed
    }
}
```

A call inside the region that transitively touches `Net` would be E0741 even
though no `Net` call appears literally in the block. Like every effect
construct, `#Caps` is a plain lexical block in codegen — it erases.

### Higher-order effects — transparent flow-through (D-EFF2)

A higher-order function's effect set is **its own body plus, at each call, the
effects of the function values passed to it** — so a callback's effects surface
at the *call site*, not buried inside the higher-order callee. This is the
zero-syntax default:

```jet
fn apply(f: fn(Int) => Int, x: Int) => Int { return f(x); }

fn run() =[IO]=> {
    apply(log_it, 1);   // if `log_it` uses Net, this line is E0740 — Net ⊄ {IO}
}
```

- A **lambda** argument's body is walked inline, so its effects already belong
  to the enclosing function.
- A **directly-named function** argument flows its effects through precisely.
- Any **other** function value (a local binding, a parameter passed onward, a
  returned or stored callback) has an origin that isn't statically known at the
  call, so it defaults to the **maximal** effect set — sound, conservative.

Two expert levers refine this (ratified D-EFF2, additive to the default above):
`fn(…) =[]=>` / `fn(…) =[Net]=>` **parameter types** demand/bound a callback
(passing one with effects outside the bound is **E0747**), and `=[via f]=>` on a
signature publishes a tight pass-through that holds even when the value escapes.
The conservative default is correct without them; they trade syntax for
precision.

### Effects on trait methods (D-EFF3)

A trait method may declare an effect upper bound — `fn hash(self) =[]=>` (the
empty set) or `fn render(self) =[GPU]=>`. The bound is two things at once:

- **The impl obligation.** Every implementation's inferred effects must fit
  inside the bound, or it is **E0742**. So a trait can promise "all `hash`
  implementations are pure" and the compiler holds every impl to it.
- **The dispatch contract.** A call through a trait object (`Box<dyn Trait>`)
  sees the declared bound as its effect, because the concrete impl is unknown at
  the call site — so safe-by-default survives dynamic dispatch.

```jet
trait Shape {
    fn area(self) =[]=> Int;   // every impl must be pure
}
impl Square.Shape {
    fn area(self) => Int { return self.side * self.side; }   // OK — pure
}
```

An un-annotated trait method is inferred per-impl under static dispatch; the
dynamic-dispatch fix-it (annotate the method when it's called through an object
under an effect ceiling, E0743) is the remaining surface here.

## Terminal direct-input (D-TERM1, ratified 2026-06-22)

`live { … }` enters un-buffered, no-echo terminal input mode for its body and
restores the terminal on every exit path (normal return, `?` propagation, panic
unwind) via a RAII scope guard (D-DEFER1).

```jet
use core.term as term

#Live {
    k :: term.read_key()
    if k == Enter { return }
    print("got: {k}")
}
```

`use core.term as term` is required for `term.read_key() => Key`. The `live`
keyword itself does not require the import — the block's syntactic gate is
sufficient.

**`Key` enum** (prelude type, `core.term`):

| Variant | Payload | Description |
|---------|---------|-------------|
| `Char(c)` | `Char` | Printable character |
| `Enter` | — | Enter / Return |
| `Escape` | — | Escape |
| `Backspace` | — | Backspace |
| `Tab` | — | Tab |
| `Delete` | — | Forward delete |
| `Up` / `Down` / `Left` / `Right` | — | Arrow keys |
| `F(n)` | `Int` | Function key F1–F12 |
| `Ctrl(c)` | `Char` | Ctrl + character |
| `Unknown` | — | Unrecognised byte sequence |

Pattern matching uses `== Variant(binding)` (PatternTest form):

```jet
if k == Char(c) { print("char: {c}") }
if k == Enter   { break }
if k == F(n)    { print("F{n}") }
```

Enum literals use the qualified form: `Key.Char('a')`, `Key.Enter`, etc.

**Restrictions:**
- E3401: `live { … }` is impure — rejected in a `fn … =[]=>`.
- E3301: rejected in `--freestanding` builds (no OS terminal device).
- REPL: rejected in interactive mode.

**Platform FFI:** I6-compliant; uses inline `extern "C"` (POSIX termios) and
`extern "system"` (Windows console API) — no external crates.

## REPL Core effects

The REPL keeps accepted statement ASTs and live `CtValue`s across turns.
Lists, maps, options, results, structs, enums, and closures are not rebuilt
from display text; explicit binding annotations remain available to `:type`.

Pure Core calls run directly. Ambient Core calls use normal Jet authority:
the call must be inside `#Grant(root)`, and the REPL must authorize the exact
operation and resource before it touches host state. A TTY prompts for once,
session, or deny. A session allowance is an exact tuple and offers continue
or revoke on reuse. `--allow-fs`, `--allow-env`, `--allow-exec`,
`--allow-net`, and `--allow-io` skip ordinary prompts for their roots;
matching `--deny-*` flags override them. Piped and transcript sessions never
prompt and deny unflagged effects with E1803. Filesystem access is confined to
the project root descriptor fixed at session start; every later component is
opened descriptor-relative without following symlinks. Platforms unable to
enforce that confinement fail closed. Ambient random draws require `Rand`, but
explicitly seeded `Rng` values are injected data. REPL-owned `print`/`eprint`
capture is inherent and needs no `IO` grant.

Process execution opens the canonical executable before authorization and
launches that exact descriptor without resolving its pathname again. Stdin is
closed unless a future separately authorized stream surface supplies it. The
child starts in the verified project directory with an empty environment;
stdout and stderr are captured. Interrupts forward to its process group, and
the REPL kills and reaps that group after 30 seconds.
Native-only modules still report E1802.

## REPL multiline editing

Raw-terminal `jet repl` uses syntax-aware Enter
(D-FE-REPL-MULTILINE1=A). Enter submits input when the REPL parser accepts its
item, statement, or expression shape. When parsing instead stops at the end of
the current input, Enter inserts a newline and redraws each continuation with
the `· ` prompt. Invalid input that already contains the parser's problem
submits immediately so the normal compiler-owned diagnostic can explain it.

Escape then Enter always inserts a newline, including when the current input
is already complete. Enter on an empty continuation line force-submits. The
editor repaints the whole logical buffer after insertion, deletion, history,
or cursor movement, then restores the cursor to its source position. Cooked
and non-TTY sessions keep D-REPL9's bracket-balance continuation and `...  `
prompt; they do not claim parser-aware raw editing.

## REPL evaluation interruption

In a raw interactive REPL, Ctrl-C during interpreter execution cancels the
current turn and restores the prompt within 100 ms for Jet-controlled work
(D-FE-REPL-INTERRUPT1=A). The interpreter polls before every instruction and
before and after each runtime call. A blocking external call follows that
call's cancellation behavior; while it remains active the REPL prints
`warning: interrupt requested; waiting for active external I/O to stop`.

Cancellation is transactional for session state. Bindings, moves, and
statement history from the interrupted turn do not commit; earlier session
state remains. Host effects completed before cancellation cannot be undone,
so the REPL prints `Interrupted. External effects already performed were not
rolled back.` The interrupted turn remains visible as `interrupted` in
`:turns` and can be replayed with the ordinary rerun mechanism. A second
Ctrl-C received while that turn is still stopping exits the REPL. Outside
evaluation, Ctrl-C keeps its editor behavior: clear nonempty input first;
exit from an empty prompt.

## Semantic assistance

REPL documentation and completion, LSP hover and completion, and `jet ?` help
project their facts from `jet-semindex`'s shared semantic symbol index. A
symbol fact carries stable module/member identity, kind, signature, summary,
examples, provenance, and source span where one exists. Checked definitions,
members, parameters, locals, imports, and aliases retain their semantic
identity; equal spellings in different modules or on different owners remain
distinct. Language builtins live in the same index rather than consumer-local
tables. `jet ?` command facts use the same model, with search categories,
flags, and cross-links kept as presentation metadata.

Raw-terminal REPL completion inserts a unique match immediately. Multiple
matches open a selectable list: Up and Down change selection, Tab advances,
Enter inserts, and Escape closes the list. Cooked terminals and `NO_COLOR`
use the same candidates with a textual selection marker; ANSI styling is not
required to discover or choose an item.

## REPL history

The REPL keeps the latest 2,000 successful submissions between sessions
(D-FE-REPL-HISTORY1=A). Failed turns and meta-commands are not stored. History
lives at `$XDG_STATE_HOME/jet/repl-history` on XDG systems or the platform
state-directory equivalent. Its directory and file are owner-only. Each input
is stored losslessly, including multiline, effectful, or secret-bearing text;
Jet cannot truthfully identify every secret.

State-path traversal rejects symlink/reparse components and holds the opened
history directory as the authority for later reads, replacements, and erasure.
Each write and clear takes a bounded, crash-released cross-process lock, then
re-reads current history before changing it. Concurrent sessions therefore
merge successful submissions, and a stale session cannot resurrect entries
removed by `:history clear`. Replacement is atomic and durable on supported
platforms.

F3 opens interactive history search. `:history search <text>` is the textual
path and `:history clear` erases the whole file. `JET_REPL_HISTORY=off` keeps
history in memory for the current session only. `JET_REPL_HISTORY_LIMIT=N`
changes the retained-entry bound. If the file ends in a corrupt or incomplete
record, the REPL discards that tail, preserves the valid prefix, and warns. If
private storage cannot be opened or written, the REPL warns and continues with
session-only history.

## Editions & release policy (E2-M2)

A project pins an **edition** with `edition: "2026"` in its `package.jet`
(D-REL3). An edition opts the project into a specific era of Jet syntax; the
toolchain advertises the editions it supports in `jet --version` and rejects a
future edition it can't provide (E2001). Single-file `jet run file.jet` carries
no edition marker and always uses the newest stable edition (E2-V4). The full
compatibility contract — patch/minor/major/epoch/edition definitions, the
backward-compatibility guarantee, the deprecation window (L2001 → E2002), the
migration authority (only `jet fix` + edition upgrade, D-REL5), and the
generated-code license statement — lives in docs/spec/release-policy.md.

## Toolchain as a dependency — the `jet:` pin (D-JPK-TOOLCHAIN1=A, #179, U30)

A `package.jet` pins **which Jet compiler** builds the project with a top-level
`jet:` field, whose value is a **channel ref** (D-JPK-CHANNEL1 semantics):

```jet
name:    "wordstats"
version: "0.3.1"
jet:     0.4              // track the 0.4 series
```

Accepted forms: a `MAJOR.MINOR` series (`0.4`), a `MAJOR.MINOR.PATCH` exact
(`0.4.2`), or a named channel (`main`). A range/operator form (`>=1.0.0`) is
**not** a pin — it is the legacy compatibility constraint (E1208) and stays a
minimum-version gate; a channel-form pin is owned by version dispatch instead
(E1249 rejects a malformed pin). Absent `jet:` = unpinned: the running `jet`
builds it with no fetch (rung-0/1 stays frictionless).

The channel resolves to an exact version recorded in the `.jet/lock`
`[[toolchain]]` block (channel + version + envelope). The channel re-resolves
only on `jet update jet` and first realization; every other run reads the lock.
A running `jet` in the pinned channel builds the project natively. A `jet` from
a different series **realizes the pinned compiler as a prebuilt hangar object**
(D-JPK-CACHE1 substitution) and **re-execs into it** (D-JPK-DISPATCH1) — never a
source build of the compiler; a platform cache miss is E1251, never a silent
wrong `jet`. A `JET_TOOLCHAIN_EXEC=<version>` env marker guards the re-exec so
the pinned child runs natively without looping. Under `--offline`/CI an unlocked
channel is E1250 (run `jet update jet`, commit the lock). Verbs: `jet self toolchain`
(read-only pin/version/status), `jet update jet [<channel>]` (the only place the
pin moves), `jet init` (writes a `jet:` pin for the running channel by default).

This is a *different* toolchain from the Rust/native **build** toolchain that
compiles a user's `extern rust` bridge crates (D-JPK-BUILDTOOL1, E1240): that
one builds bridge dependencies; this one pins the Jet compiler itself.

## Source channels and outdated (D-JPK-CHANNEL1=A, U21)

Source refs may carry channel selectors: `#latest`, `#main`, or a major-series
mask such as `#v0.x`. The channel is tracking intent; `.jet/lock` records the
exact source that intent resolved to:

```toml
[[source_channel]]
name = "default"
channel = "latest"
exact = "acme/tool#v1.2.0@github"
```

`jetpack update [source]` is the only verb that moves `[[source_channel]]`.
`jetpack outdated` compares the lock to channel metadata and writes nothing.
`jetpack build`, `jetpack run`, `jetpack enter`, and `jetpack dev` read only the
exact lock entry; an unlocked channel source is E1271, including under CI or
`--offline`.

### Frozen-forward identity block

The Package root's `name`, `version`, and `jet` fields form the project's
**identity block**, read by the single `Package` parser before the rest of the
manifest facts. Identity is bare top-level syntax. The canonical grammar is
**contract-frozen** and must never be narrowed, so version dispatch can never be
wedged by later manifest evolution (the Go `go.mod` contract):

- The reader extracts top-level `name:`, `version:`, and `jet:` as simple
  `key: value` entries, unquoted and trimmed. There is no `payload:` or
  `identity:` wrapper.
- Any other top-level key, any unknown nested block inside or outside the Package,
  and any surrounding syntax the running `jet` doesn't recognise is tolerated
  and skipped — it never blocks the identity read.

Guarantee: **every past and future `jet` can read the identity block of any
`package.jet`.** New manifest features may only *add* fields/blocks the identity
reader ignores; the three identity fields keep this exact `key: value` shape.

## Command grouping and typed inputs (D-SHAPE6, D-SHAPE-CLI1)

Tool families use one noun-then-verb grammar. D-SHAPE6 moved
`dossier`, `schema`, `expand`, `live`, and `semindex` under `jet inspect`.
It moved `publish`, `keygen`, `key`, and `yank` under `jet registry`.
Other commands in these groups keep their existing grouped routes.
The daily commands `jet run`, `jet build`, `jet test`, and `jet fmt` stay flat.
A bare moved action is E2101 and names its canonical grouped route. It is never
a compatibility alias. Help, completion, manual, typo suggestion, and dispatch
views use the same command registry.

### Typed entry-signature CLI parsing (D-CLIFLAG1, D-SHAPE-CLI1, c7cliflag)

When present, the entry function's resolved parameter type IS the CLI spec —
no separate flag DSL to learn. `fn run()` (S12, zero-arg) is the simple program
entry; a program opts into CLI parsing by defining `fn run` with one parameter:

```jet
#CLI
struct ServeArgs {
    #[Doc("port to listen on"), Env("PORT"), Default(3000)] port: Int
    #Short("v") verbose: Bool
    config: String?
}

fn run(args: ServeArgs) {
    http.serve(routes(), port: args.port)
}
```

`#CLI` is a sibling derive of `#Codable` on the same marker/derive
machinery (D-MARKERMOVE1). `#Doc("...")` is a field-level marker giving
that flag's `--help` line; a field with no `#Doc(...)` gets a generic
"value for --name" line instead.

**Entry semantics.** `run` is the only reserved program entry name (S12). Plain
`fn run()` is the default and never requires arguments. `fn run(args: T)` is an
explicit opt-in used only when the program wants external command input in its
signature, where `T` is a CLI spec shape below. A Package may instead declare a
typed Executable `Output` whose checked `entry:` function has the same CLI
contract; this does not reserve another function name.
No variadic entry signature exists; raw argv access stays explicit inside
`fn run()` via `core.args`/`core.io.args`. `main` has no entry meaning in Jet.
Bad typed-entry shapes are diagnosed (E1308 below), not silently ignored.

### Checked Output callable references (D-SHAPE-OUTPUT-CALLABLE1)

A runnable Package `Output` links to ordinary Jet code with a function
reference. The reference uses normal scope, import, visibility, rename, and
editor-navigation rules; it is never a string lookup and `.jet/lock` cannot
rescue a stale source reference.

```jet
cli: Output :: .Executable.{ name: "todo", entry: launch };
api: Output :: .Service.{ name: "todo-api", entry: serve };
release: Output :: .Check.{ name: "release", entry: verify_release };

fn launch() {}
fn serve() => () ? {}
fn verify_release() => () ? {}
```

`Output` is a closed sum with exactly `Library`, `Executable`, `Service`,
`Check`, `Environment`, `Image`, `Bundle`, `System`, and `Fleet`. Every Output
has fixed text `name:`. Executable, Service, and Check also require `entry:`.
An Executable takes zero or one `#CLI`-derived parameter; Service and Check
take none. All three return `()` or `() ?`. Sema resolves and validates the
callable before TIR or Rust emission, and publishes its definition and solved
effect row to semantic tooling.

For a singular run, explicit selection is handled by the command layer. With
no explicit address, legacy `fn run` wins; otherwise a sole compatible
Executable is selected. Multiple candidates produce E1321 with a sorted list.

**Pinned field-mapping rule** — every `#CLI` struct field maps to exactly
one named `--flag`, by this rule (checked top to bottom, first match wins).
D-CLI-POS1=A adds positional filling for required value fields:

| Field shape | Named form | Bare form | Absent at runtime |
|---|---|---|---|
| `Bool` | `--name` (boolean flag) | — | `false` |
| `T?` (`T` a supported scalar) | `--name VALUE` (optional) | — | `None` |
| scalar with `#Default(expr)` | `--name VALUE` (optional) | — | `expr` |
| required scalar with `#Flag` | `--name VALUE` only | rejected on purpose | runtime error, `core.args` voice |
| any other supported scalar | `--name VALUE` | fills by declaration order | runtime error, `core.args` voice — no new diagnostic code |

Supported scalars: `Int`, `Float`, `Bool`, `String`, `Path`. Any other field
type (a `[K: V]`, a closure, a `[T]`, a nested struct that isn't itself
`#CLI`, …) is **E1305** — there is no flag shape for it. Field defaults
use the *existing* `#Default(expr)` marker (D-SERDE5) — not a second,
inline `= expr` mechanism (that syntax is reserved for function-parameter
defaults, S61, a different grammar slot; reusing `#Default(...)` here is
I8: one mechanism for "this field has a default", not two). Field name
`snake_case` → flag `--snake-case` (underscores become dashes); no
casing-style menu (that's a wire-format concern, D-SERDE3, not a CLI-flag
one). Every field always accepts its named `--field` spelling; when both a
named value and a bare positional appear for the same field, the named value
wins. `#Flag` on a Bool / optional / defaulted field is **E1309** (nothing
to opt out of). Declaration order of required value fields is part of the
command interface; reordering them is a breaking shape change reported through
the checked `CLISchema` / dossier / embedded command metadata.
Every generated CLI spec also registers `--help` automatically (rendering
the struct's fields/types/`#Doc` text); a field named `help` collides
with it and is **E1306**.

`#Short("n")` adds the one-ASCII-letter `-n` form to the field's existing
long form. `#Env("PORT")` reads `PORT` only when command input is absent.
Explicit command input wins over the environment, and the environment wins
over `#Default`. Generated help shows this precedence. The checked
`CLISchema`, dossier, embedded metadata, and shell completion keep both marker
values. An invalid or duplicate short name is **E1318**. These markers outside
a `#CLI` struct, and `#Env` on a presence-only `Bool` flag, are **E1319**.

**Nested `#CLI` structs are not supported in v1** — a field whose type is
itself a `#CLI`-derived struct is E1305, same as any other unmapped type.
(Grouped `--outer-inner` flag prefixing was scoped out rather than bolted
onto the decode machinery under time pressure that would otherwise force a
second, prefix-threaded code path — a real feature, not a punt: it needs
its own worked design before it rides this derive.)

**Subcommands** — an `enum` parameter dispatches by variant:

```jet
enum Cmd {
    Serve(ServeArgs)
    Import(ImportArgs)
}
fn run(cmd: Cmd) { ... }   // $ myapp import data.csv
```

The first positional token picks the variant by its **lowercased** name;
the rest of argv re-parses against that variant's own `#CLI` spec (its
own `--help`, its own flags — no flag namespace is shared across
variants). Every variant's payload must be a single `#CLI`-derived
struct — any other payload shape is **E1307**. Given **zero** arguments (no
subcommand token at all), or given root `--help`, the generated entry prints
the lowercased command list to stdout and exits 0 — an invocation asking "what
can this program do" is treated as a request for orientation, not a mistake;
an unrecognized subcommand name is still a real error (nonzero exit, stderr).

**Codegen** generates directly onto `core.args`'s existing `ArgsSpec`/
`ParsedArgs` builder (D-ARGS1) — the same `.flag`/`.option`/`.parse`
surface a hand-written call chain uses, so there is exactly one parser
(I8), not two. A bad flag at runtime (unknown flag, bad `--port` value, a
missing required flag) is the same `core.args` runtime-error voice as
`ArgsSpec.parse`'s own messages — no new diagnostic codes for that path,
only for the compile-time shape checks above (E1305–E1308). `88_args_spec`/
`64_cli_args`-style direct builder use is untouched; this feature is a
layer generated on top of it, not a replacement.

`jet inspect dossier <entry.jet> run --json` projects that same checked command
schema as `command_schema`: shell flag, value type, required/default state,
help text, subcommands, and completion words. The human dossier prints the same
facts. Tools consume this projection instead of reconstructing field-to-shell
mapping.

**Executable command metadata (D-SHAPE-CLI-CARRIER1=A).** Every compiled
program carries one versioned `JetCommandSchema` record inside its executable:
`.jet_command` in ELF, `.jetcmd` in PE, `__jetcmd` in Mach-O, and the
`jet.command` custom section in Wasm. Universal Mach-O files carry the same
record in every architecture slice. The record is emitted before the artifact
is cached, packaged, or signed, so it is part of the artifact identity. Readers
parse the format section tables with bounded offsets and lengths; missing,
malformed, duplicate, unsupported-version, or disagreeing universal-slice
records fail closed. Universal slices cannot contain another universal
container. External discovery opens the artifact once, requires a regular
file, then reads at most the 512 MiB limit plus one byte from that same handle.
It never executes the target program.

`jet self completions SHELL --for PROGRAM` (D-SHAPE-CLI-COMPLETE1=A) reads that
record and writes a bash, zsh, fish, or PowerShell script to stdout. Without
`--for`, Jet's own completion output is unchanged. External scripts contain
only checked schema candidates: root `--help` and lowercased enum subcommands,
then only the selected subcommand's `--help` and derived flags. They never
query live application values. Scripts register the executable's basename,
not its supplied path; a basename containing control characters is rejected.
Plain `fn run()` embeds an empty application schema and therefore still
produces a valid built-in-only `--help` script. Metadata failures are E2103 on
stderr.

**Diagnostics:** E1305 (unmappable field type), E1306 (flag-name collision,
including the reserved `--help`), E1307 (subcommand payload isn't
`#CLI`), E1308 (`run`'s one parameter isn't a `#CLI` struct or an enum
of `#CLI` payloads), E1309 (`#Flag` on a field that is already flag-only).
See docs/spec/diagnostics.md.

The public `#CLI` struct or subcommand enum may be declared in the entry file
or in one directly imported module. Its generated parser/decode helpers remain
internal projections over the same `ArgsSpec` engine.

## `jet inspect expand` — transparency command (D-EXPANDCLI1, card #183)

Every "the compiler inferred this for you" mechanism (I8: magic default,
expert opt-in) needs a way to ask the compiler what it decided. `jet inspect expand`
is that one command for all of them — never a second, mechanism-specific
CLI flag per feature.

```
jet inspect expand --facts <lens> <file.jet>   # one lens's facts
jet inspect expand <file.jet>                  # every lens, grouped, empty ones skipped
jet inspect expand --facts inline --json <file.jet>  # canonical semindex + inline projection
```

Facts are read straight off the ordinary check pass — never a second
analysis, never rustc (I2/I3). A lens renders fields already sitting on the
checked AST (e.g. `Func::is_inline`/`is_inline_always`, validated by the
time the bundle compiled at all) — the same side-channel `jet inspect semindex`/
`jet inspect impact` already read, not a parallel pipeline.

**Shipped lenses:**

- `inline` (D-INLINE-PARAM1) — every fn/method carrying `#Inline` or
  `#Inline(Always)`: the contract and the Rust attribute codegen emits
  (`#[inline]` / `#[inline(always)]`). Functions with neither marker produce
  no line — the lens reports contracts, not every function in the program.

- `memory` (D-MEM-FACTS1) — declared and projected `no_alloc`, `zero_rc`, and
  `arena_bounded` facts.
- `web` (D-WEBAPP1) — the checked application graph: routes, actions, mounts,
  and policy.
- `effects` (D-EFF1 / D-SEMINDEX1) — each checked function's resolved effect
  row, including direct effects, callees, and provenance.
- `layout` (D-LAYOUT-FACTS1=B) — compiler-owned type layout facts. Physical
  byte facts remain optional; when absent, the lens names the registered
  diagnostic and the reason.

A `refs` lens (D-REF-SHORTHAND1) once reported resolved owners for `&T`
stored-reference struct fields; D-MEM1/S3 deleted that mechanism outright
(no stored-borrow fields in v1), and the lens went with it — `jet inspect expand
--facts refs` is an unknown-lens usage error today, like any retired name.

Unknown `--facts <lens>` lists the registered lenses and exits nonzero
(usage error, not an E-code — it never reaches the diagnostic renderer). A
file that fails to compile prints the ordinary front-end diagnostics and
exits nonzero: facts require a clean check, same as `jet inspect semindex`/
`jet inspect impact`. A clean program with no facts for a lens (or for every lens,
bare form) exits 0 — absence of facts is not a failure.

`--json` keeps the canonical semantic-index document and adds one additive
`expand` projection. The projection records the requested selection (`all` for
the bare form), the registered lens name and summary, and structured facts with
source paths, byte spans, and line/column positions where a lens has a source
location. The checked bundle and sema facts are shared with human output; JSON
does not create a second schema or analysis path.
Usage errors such as an unknown lens stay outside the diagnostic-code table and
use a small versioned `error.kind = "usage"` object; a source that fails the
ordinary check uses one versioned document whose `diagnostics` entries are
serialized by the shared machine-diagnostic renderer.

**Extensibility:** lenses live in one static table in `Source/CmdExpand.rs`
(name, one-line summary, renderer) — adding a lens for a future ratified
mechanism (effects, layout, derive expansion) is one row, never a new
subcommand or a new flag (I8).

## Semantic index, dossier, and codemods (D-SEMINDEX1, D-WD2, D-CODEMOD1)

`jet inspect semindex --json <file.jet>` emits schema v12: definitions, references,
call edges, effects, member facts, and typed Package/workspace-overlay facts. Member facts stitch fields, variants,
inline methods, external inherent impl methods, trait impl methods, and trait
requirements into one stable owner-ordered view. Every resolved reference also
carries its definition identity; unresolved or ambiguous references carry no
target and semantic edits must not fall back to spelling. Compiler-internal
structural facts expose checked `expr`, `stmt`, `item`, and written `type` node
boundaries for refactoring tools.

`jet inspect dossier <file.jet> [Symbol]` renders those facts as a human report;
`--json` emits the same lens data. The first shipped lens is the type/member
dossier. It never re-checks by another path and never invents facts missing
from semindex.

The LSP exposes scattered-method breadcrumbs as inlay hints at the owning type
declaration. These are editor-only overlays: they do not edit source and carry
source links to the real impl method spans.

`jet inspect codemod <plan.json> --dry-run`, `jet inspect codemod apply <plan.json>`, and
`jet inspect codemod undo <log.json>` use one replay engine for both schema versions.
A missing version or `version: 1` is the original semantic rename:

```json
{"name":"RenameReport","entry":"main.jet","operation":"rename","from":"report","to":"summarize"}
```

Schema 2 (D-CODEMOD-BATCH1=A) is an ordered batch over typed Jet templates.
`project` is relative to the object. Each root is one `.jet` file or directory
beneath `examples/` or `tests/ui/`; absolute, parent, and symlink escapes fail.
Directory discovery is recursive and byte-path ordered. Rules are either a
semantic `symbol_rename` or an `ast_rewrite` whose `node` is `expr`, `stmt`,
`item`, or `type`. `$value` captures one subtree and `$values...` captures a
list. Matching is confined to the requested compiler-owned AST boundaries;
same token bytes in another node class are not candidates. Symbol definitions
and references are selected by their resolved definition anchors, never by
spelling. Every rule declares its exact match count; duplicate ids, unknown fields,
ambiguous names, unused captures, unresolved replacement names, zero matches,
and overlapping edits fail before any write. Rule N+1 sees a compiler-reindexed
overlay containing rule N, so a later semantic rule may target an earlier
rule's output.

```json
{
  "version": 2,
  "name": "ReportV2",
  "project": "..",
  "roots": [
    {"path":"examples/report.jet","validate":"clean"},
    {"path":"tests/ui/report_type.jet","validate":"fixture"}
  ],
  "rules": [
    {"id":"rename","kind":"symbol_rename","from":{"name":"report","symbol_kind":"function"},"to":"summarize","matches":4},
    {"id":"call","kind":"ast_rewrite","node":"expr","match":"legacy_parse($input)","replace":"parse_int($input, base: 10)","matches":2}
  ],
  "snapshot_after": {"tests/ui/report_type.jet":"migrations/report_type.after.stderr"}
}
```

Clean roots must finish without front-end errors. Fixture roots must reproduce
their complete paired `.stderr` exactly. An intentional snapshot change names
a non-symlink project file in `snapshot_after`; the engine first proves those
bytes equal the compiler-rendered result, then includes the paired `.stderr` in
the same plan, transaction, and undo log. Code-only fixture changes are refused.

Dry-run holds the codemod lock through discovery, staged compilation,
validation, input rehash, and diff output but writes no source, snapshot, log,
temporary file, or journal. Apply requires `--yes` after warning that an editor
which ignores the codemod lock can still race the final rename. Apply writes
same-directory temporary files, fsyncs contents and parents, and advances a
fsynced recovery journal around each rename. Replacement reopens each parent
without following links, verifies the destination through that handle, and
renames relative to the same handle (`openat`/`renameat` on Unix; directory and
file handles plus `SetFileInformationByHandle` on Windows). The process lock is
an OS-owned advisory lock on Unix and a delete-on-close exclusive file on
Windows, so crash recovery does not depend on `/proc`. A later codemod recovers a crash
before planning; unexpected concurrent bytes preserve the journal and stop.
Schema-2 logs contain byte-exact before/after images. Undo verifies every
after-hash before making any write and uses the same journal protocol to restore
source and snapshots. Existing schema-1 inverse-edit logs remain readable.
Unified dry-run output emits the standard no-newline marker for each side whose
last line lacks a terminating newline.

## Web dev-server dashboard (D-FE-DEVSRV1)

`jet dev <file.jet> --target=web` exposes one shared status snapshot at
`/__jet_dev_status`. The terminal dashboard and browser corner strip render the
same status words, client count, build time, and diagnostic. Browser clients
have tab-scoped identities with a short polling lease, so the count represents
live tabs rather than transient HTTP connections.

In a TTY, the terminal keeps a two-row header pinned above the scrolling log.
Pressing `v` toggles request and rebuild detail without changing the shared
status; `--verbose` starts with that detail open. The scroll region is installed
only after raw input and its cleanup guard are active. `NO_COLOR` replaces the
status dot with a bracketed state word while retaining TTY pinning and controls.
Non-TTY output is plain and append-only.

While rebuilding, the browser dims the last good page. A failed build expands
the strip into an overlay containing the front end's verbatim diagnostic and
keeps serving the last good artifacts. `Esc` collapses that diagnostic without
hiding the error status. The next clean build clears it and reloads. A failed
status poll shows reconnecting in the browser; expiry of its server lease puts
the terminal on the same reconnecting state. The renewed lease returns both to
the underlying build state. Reconnecting overrides ready, building, and error
on both surfaces while retaining the last build time and any diagnostic in the
shared snapshot. The renewed lease reveals that retained state again. Recovery
reloads even when a restarted server reuses the previous process's numeric
version.

## Canvas visual editor prototype (D-BPE-*)

`jet dev <file.jet> --target=web` serves Canvas at `/canvas` (with the same
versioned JSON endpoints also reachable under `/__jet_canvas`). Canvas is a
projection of checked Jet source, not a graph asset. `/__jet_canvas/graph`
emits `jet.canvas.graph` schema v1 with one function graph per checked function:
deterministic source-order layout, structural nodes, typed pins, data/fallible
wires, inline pure expressions, source byte spans, and semindex fact handles.

`POST /__jet_canvas/transaction` accepts `jet.canvas.edit` schema v1.
Transactions include source no-op/reprojection, rename, inline expression edit,
binding promotion, call insertion, function/signature edits, trait impl creation,
wire break/move, source replace, structural rail inserts, comment/collapse
regions, and action preview. Each transaction must carry the current source
`revision`; stale revisions fail with a conflict. Successful writes go through
`jet fmt`, re-check through the front end, replace ordinary `.jet` source, and
then reproject.

The Code lens is read-only by default. `Edit Source` switches to an explicit
source editor, and `Apply Source` sends a `replace_source` transaction through
the same format/check/reproject path. Canvas never writes a graph asset or owns a
second parser/checker.

`POST /__jet_canvas/query` accepts read-only query schema v1 for find,
references, source-to-graph, rename preview, action palette, and Core catalog
browsing. `GET /canvas/core-catalog` exposes the same read-only `core.*`
catalog from the canonical Core library reference. Catalog entries carry
`canvas.catalog:core.read` authority and `writes:"none"`; browsing never claims
that a Core call executed.

`GET /canvas/proof` reports the selected source revision's current proof state:
front-end check result, Git text state, local debug persistence, and command
receipt status. Missing build/run authority receipts are reported as missing and
stale; Canvas never converts a graph projection into proof that code ran.
The Run button opens the real `jet run <source>` command authority card; it does
not simulate output. `POST /canvas/command` executes only whitelisted
run/check/build authority cards for the current revision, records the receipt,
and lets the proof rail mark that exact revision current. Build output commands
require explicit confirmation.

The public v1 graph/edit field contract is pinned in
[`docs/reference/canvas-protocol.md`](../reference/canvas-protocol.md), and the
AST-derived Canvas coverage ratchet is pinned in
[`docs/reference/canvas-parity.md`](../reference/canvas-parity.md).
Unknown request fields are ignored by v1; unknown operations fail as Canvas edit
errors, and unknown future graph fields may never carry hidden semantics.

## Public front-end toolkit API (D-FRONTENDAPI1=A, card #227)

The public compiler toolkit is a read-only value facade over the front end.
Rust dogfood tools use `jet::Compiler`; the exposed shapes are stable data,
not AST handles or mutable compiler state.

Version 1 exports:

- `lex_source(src)` → token views with stable kind strings, byte ranges, and
  line/column positions.
- `parse_source(src)` → top-level syntax summaries plus diagnostics.
- `check_file(path)` → diagnostics, syntax summaries, and a semantic-index
  snapshot when the file checks cleanly.
- `source_map_from_generated_rust(rust)` → generated Rust line markers mapped
  back to Jet source lines.

The compile-time `CompilerChecked` value preserves the source text, checked
function/effect facts, and optional structured semantic index alongside its
syntax and diagnostics. The CLI `check` operation serializes that same value
inside its file-addressed JSON envelope; it does not replace structured facts
with a JSON string or a second partial shape.

Diagnostics are cloned into value records (`code`, severity, message, why,
fix, span). Semantic facts are cloned from the existing semindex schema. No
API returns `Program`, `Item`, `Expr`, `Token`, mutable caches, parser state,
or sema internals, and no API can feed modified syntax back into compilation.

## Inline script dependencies — `use pkg#version` (D-JPK-SCRIPTDEP1=A)

A bare `.jet` script — no `package.jet` — may open with an inline dependency
instead of a manifest:

```jet
// stats.jet
use textkit#1.4

fn run() {
    print(textkit.wrap(input(), width: 72))
}
```

`pkg#version` is the `#` directive plane's version-selector form
(D-MARKER-FAMILY1); the version is a dotted numeric selector (`1.4`, `1.4.2`),
never a range operator. Only a single-segment module name takes one — `use
core.files#1.0` is nonsensical and isn't accepted.

`jet run stats.jet` collects every inline ref from the entry file, resolves
each, and wires the resolved directory into module search exactly like a
hangar-realized `library` (U17) — the rest of import resolution is
unchanged. Resolution is a local directory lookup only (no network, no
code execution): a script's own `.jet/inline-deps/<name>/<version>/` copy
(the `.jet/` managed-folder convention), or a version matching one there by
dotted prefix (`1.4` matches `1.4.2`). Consuming the public Jet package
registry by name isn't wired yet (E1207/M12.2 — publishing today writes only
the sparse index line, never a fetchable source tree); an inline ref that
resolves nowhere is E1253.

A loose selector (`1.4` rather than an exact `1.4.2`) is fine to write —
rung 0 stays magic — but is L0203: nothing pins it until `jet store lock stats.jet`
writes a `stats.jet.lock` sidecar (`script_hash` + each dep's resolved
version and content hash, keyed by the script's own file-content hash so an
edit goes stale). `jet init stats.jet` lifts the inline refs into a freshly
written `package.jet`'s `deps: {}` block, growing the script from rung 0 to rung 1
(vision.md's ladder) without discarding what it already declared.

## `target: plugin` — sandboxed WASM Component Model plugins (c81, D-PLUGIN1=B, D-DEP-WASM1=A)

A package built `target: plugin` compiles to a sandboxed `wasm32` Component
Model module instead of a native binary. A host program loads and calls it —
safe by default, **no `#Unsafe` gate anywhere in the story** (I1): the
sandbox is the safety boundary, by construction. This is a general
application-plugin substrate (WIT world `jetplugin`), distinct from PATH
`jet-*` helpers (D-DX5) and from the compiler-extension API (Tower #549,
D-DX5-HOOK1=A: typed read-only post-sema snapshot in world
`compiler-extension-v1`, same wasmtime substrate, separate host) — don't
conflate them (I8).

```jet
// package.jet
name: "mathkit"
version: "0.1.0"
```

```jet
// main.jet — the plugin's own source, no `fn run()` (it's loaded, not run)
pub fn scale(a: Float, b: Float) => Float {
    return a * b
}
```

`jet build main.jet --target=plugin` writes a `.wit` world (generated from
the entry file's top-level `pub fn` surface) plus the wasm32 guest Rust, then
shells out to `rustc --target wasm32-unknown-unknown --crate-type cdylib`
followed by `wasm-tools component embed`/`component new` (D-DEP-WASM1=A) —
external CLI tools, never linked into the compiler (I6) — producing a
`.wasm` Component Model binary.

A host is an ordinary native Jet program:

```jet
use core.plugin as plugin

fn run() {
    mathkit :: plugin.load("mathkit.wasm")
    area :: mathkit.call("scale", [6.0, 7.0]) ?? panic("scale failed")
    print("scale(6, 7) = {area}")
}
```

`Plugin.load(path) => Plugin` produces a handle (mirrors `core.db`'s
`open`/`open_memory`); `.call(name, [Float]) => Float ? String` and
`.call_int(name, [Int]) => Int ? String` are the only instance methods (v1
scope — every parameter and the return type must be all-`Int` or all-`Float`,
E1260; Bool is a trivial follow-on, Text needs the Component Model's
memory-based string ABI, a real future increment). The wasmtime host embedded
via the FFI-bridge pattern (`crates/jet-driver/src/Prelude/Plugin.rs`,
runtime-side only, I6) registers **zero host imports** — deny-by-default
capabilities: a plugin that tried to touch the filesystem, network, or clock
simply fails to instantiate at load time, reported as a clean `Err`, never a
crash (I2). A plugin's own code may not use any effect either — caught at
build time as E1258, not deferred to that runtime failure.

D-PLUGIN-EXPORT1=A: the exported surface is named by the manifest `export:`
target field (`plugin { export: "mathkit" }`), defaulting to the package
name. D-PLUGIN-VERSION1=A: the exported interface is frozen via
`Sema::ApiFreeze`'s pub-metadata semver-snapshot mechanism (the same one an
ordinary library's public API uses, E1218/E2601) — keyed `plugin__<export>`
in `.jet/cache/api/` so it never collides with a library's own frozen API in
the same project. Rebuilding a plugin with an unchanged interface freezes
silently; removing or changing an export is E1257, naming the exact delta.

Full worked example: `examples/features/packages/plugin_mathkit/` (a
`plugin_src/` package + a host `main.jet`; golden-enforced, I5). New
diagnostics: E1257 (interface changed incompatibly), E1258 (capability
denied), E1259 (wasm build/toolchain failure), E1260 (unsupported export
shape) — see docs/spec/diagnostics.md.

## Programmable builds as Jet (D-BUILDENTRY1 and build-graph decisions)

`jet build` checks the root program, then runs one optional unit-local
`fn build(b: BuildContext) => BuildPlan ?` through the same interpreter used by
comptime. The entry may live in the source file, in a managed package's
`package.jet`, or in `workspace.jet`. For a workspace entry, member entries run in
deterministic dependency order with separate read-only plans; the workspace
entry runs last with a fresh `BuildContext` and can add only workspace-owned
targets. Imported dependency entries are checked but never run. With no
unit-local entry, the existing zero-configuration pipeline is unchanged.

Build code registers ordinary typed values. Targets are declared once with
`b.add_executable`, `b.add_library`, `b.add_test`, `b.add_bench`,
`b.add_asset_bundle`, `b.add_doc`, `b.add_install`, `b.add_package`, or
`b.add_publish`; each returns a `BuildTarget`. `b.action(name, inputs, outputs,
argv, caps)` returns a `BuildAction`. `b.plan()` or `b.plan(default)` hands the
same canonical graph to scheduling, caching, execution, query tools, and the
LSP. Expert actions may additionally pass a typed `BuildToolchain` and a list
of typed `BuildProbe` handles as arguments six and seven. `b.toolchain(name,
target_triple)` records the target identity; `b.probe(name, kind, value)`
supports `find_program`, `pkg_config`, and `header` probe kinds.

```jet
fn build(b: BuildContext) =[Exec, FS]=> BuildPlan ? {
    #Impure("run declared toolchain probe and action") {
    shell :: b.probe("shell", "find_program", "sh")?
    native :: b.toolchain("native", "x86_64-linux")?
    stamp :: b.action(
        "stamp",
        ["assets/version.txt"],
        ["build/version.txt"],
        ["sh", "-c", "cp assets/version.txt build/version.txt"],
        ["Exec", "FS"],
        native,
        [shell]
    )?
    app :: b.add_executable("app", ["main.jet"], [stamp])?
    return b.plan(app)
    }
    return b.plan()
}

fn run() { print("hello") }
```

Action dependencies come from target dependencies and declared file
producer/consumer edges. Ready nodes run concurrently in deterministic stages;
`linker`, `console`, `gpu`, and named resource pools serialize. Cached actions
key declared input content, argv, environment, capabilities, toolchain, probes,
resource pools, plugins, and generated-source hashes. Cache hits restore only
declared outputs from the local CAS.

Execution has no ambient fallback. On Linux each action runs under bubblewrap
with private mount, PID, IPC, UTS, and network namespaces. Only declared inputs
enter its writable work tree; only declared outputs return. Network remains
unshared unless both source and policy grant `Net`. A missing sandbox is E3505,
not an unsandboxed run. Single-file authority uses the ratified per-effect
flags (`--allow-exec`, `--allow-fs`, `--allow-net`, and the remaining D-EFF4
names). Package/workspace policy can grant or cap the same capability set.
The vocabulary is one closed typed ten-effect enum shared by sema, policy,
CLI, graph, cache, and executor. Every effectful `b.action`/`b.probe` call must
be inside its active `#Impure("reason")` region. Signature declaration and
effective grant are checked before any probe or process runs.

`b.generate(name, source)` materializes `.jet/generated/<package>/<name>.jet`.
Action outputs ending in `.jet` follow the same path. Both re-enter lexer,
parser, and sema before runtime codegen; malformed generated source is a Jet
diagnostic with generator provenance. The build-only entry and imported build
entries are removed before codegen, so rustc never sees build handles.

Generation is additive and has one owner per managed path. Generated modules
are ordered in deterministic dependency rounds using ordinary quoted-file
imports; a later round may observe an earlier round, the number of rounds is
bounded by the number of generated modules, and a cycle is E3511 before any
generated file is written. A selected action may not also own a generated
`.jet` path. Existing source collisions are E3510. `--locked` checks the
generated input and output hashes before materialization and records the same
provenance after the complete runtime bundle is checked.

The remote cache/execution seam is transport-only. Cache reads, writes, and
remote execution require an explicit policy grant plus a complete sandbox
proof whose action key, toolchain digest, output paths, and provenance are
checked for parity. A proof also binds the authorized builder, trust domain,
worker identity, platform, ABI, and credential-bound worker receipt. A missing
worker or remote record is an error; `fallback_local` is an explicit host
binding choice that resumes the same sandboxed local executor after a remote
failure. Timeouts write an authenticated cancellation tombstone; late worker
results are rejected and cannot become cache records. The host creates and
removes bindings with `jet remote bind`, `jet remote list`, and `jet remote
remove`; `jet build --builder <name>` selects one. Source text, ordinary CLI
flags, and environment variables cannot create an endpoint, credential, or
trust root. Request, result, cache-record, and CAS-blob envelopes use
authenticated HMAC-SHA256 transport records. Input/output blobs must exist
before a request/result or cache record is published, and a new submission
removes any older result for the same action key before a worker can answer.
Every remote execution request carries a unique attempt ID. The ID is bound
into the worker receipt, request, authenticated cancellation marker, and
result; resubmitting an already-cancelled attempt or publishing a result for
an older attempt fails even when the action key is unchanged. Execution
cancellation, result publication, and result reads share a cross-process
kernel/file commit lock over their final visibility transition; an in-process
mutex is not a correctness boundary. Authenticated cache-only transports may
read, but cache writes and execution blob exchange require the host-bound
worker identity.

WASM build plugins enter through the packaged manifest/component loader or the
typed in-memory test seam. The loader verifies a regular non-symlink file, the
Component Model binary envelope, the fixed API version, and its SHA-256 digest
before application. Capability grants are checked per plugin and the graph is
rolled back on every rejected contribution, so a hostile plugin cannot leave
partial actions, targets, or generated modules behind. Packaged manifests are
bounded to 64 KiB and components to 64 MiB; request and response pipes are
bounded too, and both the compiler loader and the production `jetpack` host
reject symlinks and non-regular package files.

Legacy CMake, Make, Gradle, npm, and Cargo support remains an explicit Tier-2
wrapper. `LegacyWrapperSpec::from_project_file` parses the wrapper's canonical
root file into the typed command, paths, capabilities, environment, cache,
kind, pools, and provenance fields. It records the file as a typed action
input, rejects links, oversized/non-UTF-8 files, and unsupported constructs,
and records the bounded non-symlink project source closure as typed inputs;
the action key therefore includes auxiliary scripts, headers, and lockfiles.
Imported CMake and Gradle projects must declare each exact output with a
`jet: output=...` directive; the importer never guesses a target artifact.
An imported npm package must declare its exact entry output in `main` or
`module`. Dependency-bearing npm imports fail before execution because the
hermetic sandbox does not copy `node_modules`; no undeclared install or
network fallback is attempted. Make recipes, Gradle task bodies, unpinned
Cargo dependencies, non-registry Cargo sources, and unmodeled package fields
fail closed rather than being dropped. The production `b.legacy` bridge fails
if its declared graph does not match the import. The production build policy
denies these wrappers in CI unless a stronger host policy explicitly replaces
that default.

Fleet host overrides are typed values, not deferred source snippets. Their
fields use the same pure comptime evaluator and dependency-cycle checks as
computed module fields; each result retains exact source, dependency, and
purity provenance for inspection.

`core.compiler` is the typed read-only compiler API. `lex`, `parse`, `check`,
and `source_map` are compile-time-only and preserve source, spans, diagnostics,
semantic facts, and generated-line mappings. `jet inspect compiler` mirrors
these operations in deterministic JSON with `schema_version: 1` and
`api_version: 1`; runtime calls are E0956.

The selected target source/dependency closure plus generated modules becomes a
fresh runtime bundle. Native, cross, web, plugin, and freestanding lowering all
consume that same checked bundle. `--locked` compares generated input/output
hashes before committing provenance; drift is E3512 and action outputs roll
back.

`jet inspect graph <file> --json` and `jet inspect query build <file> --json` return the same
typed graph without executing actions. `jet inspect explain-build <target|action|file>
<file>` reports graph and cache provenance. LSP checking uses the same selected
root signature validation and the same static graph facts, including E3501.

## Physical dimension algebra

D-SHAPE-QUANTITY1 makes the `Length`, `Time`, `Speed`, `Area`, and
`Temperature` unit-family
identities compiler-known. Addition, subtraction, and comparison require equal
dimensions; E0359 reports a mismatch before codegen. Multiplication adds and
division subtracts normalized exponents, so `Length / Time` is `Speed`,
`Length * Length` is `Area`, and `Speed * Time` is `Length`. The semantic index
and API snapshots serialize the normalized identity and numeric base. Backends
receive only that numeric base: dimensions have no runtime representation.

Currency remains nominal D-QUAL3 behavior. D-QUANTITY-DECL1 extends a closed
family with `base`, exact rational `scale`/`offset`, and stable package-owned API
identity. A family with any nonzero offset mints separate `Point` and `Delta`
named types for every member. Sema owns the closed affine algebra and exactness:
implicit conversion is value-aware and never rounds, destination-owned exact
conversion returns `Result`, and `_rounded(value, mode, digits: n)` is the
fallible explicit rounding path. Its ratified modes are `.TowardZero`,
`.Floor`, `.Ceiling`, and `.NearestEven`; `n` is a nonnegative count of
destination decimal places, and the rounded rational must be exactly
representable by the destination so binary storage adds no further loss.
Imported `Quantity<Dimension, Kind>` bounds preserve their
concrete unit through checking, API freeze, semantic inspection, Codable, AOT,
and resident JIT lowering.

## Semantic source import

D-MIGRATE-SRC1 extends the canonical import command to foreign source:
jet import LANG DIR. A language importer parses constructs it can prove,
emits ordinary editable Jet, and records every other construct as a structured
JT01xx TODO in import-report.json. Unsupported code never becomes guessed
behavior and never disappears silently.

The first importer is py. Its initial proven subset is annotated top-level
functions over int, float, str, bool, and None; straight-line local assignment,
return, calls, arithmetic/comparison/boolean expressions, and equality asserts.
Python test_ functions with no parameters become Jet Test functions. Unsupported
imports, signatures, expressions, and nested control flow stay absent from
callable Jet and appear in the omissions report with what, why, fix, source,
generated target, and migration status.

Dry-run computes and prints the same plan without writing. A plain rerun is
byte-idempotent. Update uses the last generated baseline: untouched generated
files advance, owner-edited files remain when foreign source is unchanged, and
simultaneous edits conflict before any conflicted file is written. Directory
walks are deterministic and do not follow symlinks.

## Deliberately absent

See non-goals in docs/spec/philosophy.md. The parser should produce staged
or guiding errors for the ones users will reach for (e.g. `and` → teaching
error naming `&&`, per S14).
