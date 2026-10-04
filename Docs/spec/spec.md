# Jet language specification

This specification defines Jet's source syntax and durable language contracts. It
is for people writing Jet, reviewing a language change, or implementing another
execution tier. It describes the contract and its reasons; it is not a feature
inventory or a progress log.

[Ratified syntax decisions](syntax-decisions.md) are the rationale and decision
record for this contract. A decision ID is a citation, not an implementation
status: only ratified decisions define the language. The canonical lexical ledger
is [`Syntax.rs`](../../crates/jet-foundation/src/Syntax.rs), and
[`tests/decisions.rs`](../../tests/decisions.rs) checks that the ratified decision
surface and ledger agree. The parser and semantic checker are the executable
language rules; the reference implementation is in
[`crates/jet-parser`](../../crates/jet-parser/) and
[`crates/jet-sema`](../../crates/jet-sema/). The Prelude is under
[`crates/jet-codegen/src/Prelude`](../../crates/jet-codegen/src/Prelude/) and
the core library under [`Core/`](../../Core/). The Rust-hosted compiler is the
reference implementation while the compiler is ported to Jet; this document
states the contract, not where it is implemented.

The [`Examples/`](../../Examples/) tree is the executable form of the examples in
this document. Prefer a feature example under `Examples/features/` when checking
a claim. If this document disagrees with a checked example or with the parser,
sema, or Prelude source, the executable truth wins and the prose must be fixed.
Run `jet check` on a feature file, or `jet run` when the claim has runtime
behavior. `jet eval <file.jet|expression>` evaluates pure Jet and prints the
value; add `--json` for JSON output. Vocabulary is defined in [Jet
vocabulary](vocabulary.md).

## Contents

**Language core:** [Lexical structure](#lexical-structure) · [Core grammar](#core-grammar) · [Values, expressions, and calls](#values-expressions-and-calls) · [Branching, results, and loops](#branching-results-and-loops) · [Compile-time names and staged syntax](#compile-time-names-and-staged-syntax) · [Data types and methods](#data-types-and-methods) · [Fixed-size lists](#fixed-size-lists) · [Closures and function values](#closures-and-function-values) · [Errors as values](#errors-as-values) · [Physical dimensions](#physical-dimensions)

**Ownership, effects, and safety:** [Ownership and borrowing](#ownership-and-borrowing) · [Access sigils](#access-sigils) · [Boundary crossings](#boundary-crossings) · [Effect system](#effect-system) · [Expert memory tier](#expert-memory-tier)

**Programs and modules:** [Imports and visibility](#imports-and-visibility) · [Composable configuration modules](#composable-configuration-modules) · [Concurrency](#concurrency) · [Core library](#core-library) · [Foreign-function interfaces](#foreign-function-interfaces) · [Browser effects and web values](#browser-effects-and-web-values) · [Terminal direct input](#terminal-direct-input)

**Tools:** [Formatting Jet source](#formatting-jet-source) · [Writing and running tests](#writing-and-running-tests) · [Size-oriented builds and language tooling](#size-oriented-builds-and-language-tooling) · [REPL state, authority, and editing](#repl-state-authority-and-editing) · [jet inspect expand](#jet-inspect-expand) · [Semantic index and codemods](#semantic-index-and-codemods) · [Semantic source import](#semantic-source-import) · [Web development dashboard](#web-development-dashboard) · [Canvas visual editor](#canvas-visual-editor) · [Public front-end toolkit](#public-front-end-toolkit)

**Packages, builds, and releases:** [Command registry and typed inputs](#command-registry-and-typed-inputs) · [Editions and compatibility](#editions-and-compatibility) · [Toolchain pins and source channels](#toolchain-pins-and-source-channels) · [Inline script dependencies](#inline-script-dependencies) · [Sandboxed WASM packages](#sandboxed-wasm-packages) · [Programmable builds](#programmable-builds) · [JetOS plans and proofs](#jetos-plans-and-proofs)

**Scope:** [Deliberately absent](#deliberately-absent)

## Lexical structure

- Source is UTF-8. An identifier starts with a letter or `_` and continues with
  letters, digits, or `_`.
- Source files use the `.jet` extension. `jet run`, `jet build`,
  `jet check`, and `jet eval` accept a file path without the suffix and try the
  corresponding `.jet` path when the literal path does not exist. `jet eval` also
  accepts an expression directly. If neither path exists, the original spelling
  is retained for the file-not-found diagnostic (N2).
- Jet's closed artifact suffix family is `.jetmap` (source maps), `.jetnb`
  (notebooks), `.jetproof` (proof evidence), `.jettrace` (performance traces),
  `.jetreplay` (game-input replays), and `.jetproof-replay` (proof replays).
  Consumers reject a different family member by artifact kind; retired suffixes
  have no compatibility aliases (D-ARTIFACT-EXT1=A).
- A line comment starts with `//` and ends at the line ending. A block comment
  starts with `/*` and ends with the matching `*/`; block comments nest. An
  unbalanced block comment is E0002 (S5).
- A single-line string uses `"..."`. Its only escapes are `\n`, `\t`, `\"`,
  and `\\`; another escape is E0001. Interpolation uses `{expr}`. The format
  selector rail is closed: `{value:Debug}`, `{value:Pretty}`,
  `{value:Fixed(2)}`, `{value:Grouped(2)}`, and `{value:Unit(name|bare)}` are
  the registered forms. `{{` and `}}` produce literal braces; an unmatched
  brace is E0001 (S8, S20).
- A run of N backticks opens a raw ordinary `String`; the next maximal run of
  exactly N backticks closes it. Other runs remain text. Raw text has no
  escapes or interpolation, preserves line-ending bytes, and removes one edge
  space from each end only when the raw text contains a non-space character
  (D-RAWSTR1).
- A triple-quoted string, `"""…"""`, may span lines and has the ordinary
  escape and interpolation rules. The newline immediately after the opening
  delimiter and the newline immediately before the closing delimiter are
  removed. The closing delimiter's indentation is removed from every line. An
  unterminated triple-quoted string is E0002 (S70).
- A checked text head, such as `SQL{"select {table}"}`, is a nominal typed
  value. Its body is a raw boundary for that head's grammar: backslashes remain
  literal, while quote, brace, delimiter, and interpolation-hole rules still
  apply. Plain strings retain the four-entry escape table
  (D-BOUND-RAW1=A, D-TEXTHEAD-TYPE1=A).
- `SQL{"…"}`, `HTML{"…"}`, and `Sh{"…"}` use the checked interpolation
  engine. In `Sh`, literal words become argv items and each hole becomes one
  argv item; no word splitting, glob expansion, or shell parsing is applied to
  a hole. Converting a runtime `String` to checked text is E0149; the audited
  `Sh.raw(text)` operation is the escape hatch. A bare string never silently
  changes into one of these types (D-TYPEDTEXT1/2, D-FFI-SH1,
  D-UNIFYLIT1=A).
- Marker declarations use one named parameter list:
  `marker Name(args..., $sites: [...], $repeatable: ..., ...)`. Ordinary
  arguments and `@`-marked metadata share that list; checked-text marker
  declarations are retired, so typed text heads do not introduce a second
  marker form (D-META-FORM1, D-MARKER-SITES1, D-BOUND-SINK1=A).
- Decimal `Int` and `Float` literals are exact arbitrary-precision values.
  A decimal `Float` has digits on both sides of `.`, with an optional `e` or
  `E` exponent. `_` may separate digits. `0x`, `0o`, and `0b` introduce
  integer literals. A base prefix without digits is E0001. Unary `-` is an
  operator, not part of a literal. A whole-number literal in an operator
  expression adopts a fixed-width peer when that peer can represent it;
  otherwise it remains `Int`. Ordinary numeric widening then applies. A typed
  or destination-owned literal is checked against that destination's range
  (D-INTLIT-WIDTH1, D-NUMLIT-PEER1).
- Explicit conversion is destination-owned:
  `Target.from_source(value)`. Numeric narrowing is fallible; safe widening is
  implicit where the numeric decisions allow it. Numeric-backed distinct and
  unit types use the same source-kind names. Text interpretation is
  `Target.parse(text)`. Source-owned `to_*` conversions, casts, and a neutral
  `convert` helper are not Jet syntax (D-SHAPE-CONVERT1=A).
- A `Duration` is a signed whole-nanosecond `i64` carrier. Construct one with
  `Duration.nanoseconds`, `.microseconds`, `.milliseconds`, `.seconds`,
  `.minutes`, or `.hours`; non-finite and out-of-range input produces
  `RangeError`. `duration.in(.Unit)` reads one whole unit and truncates toward
  zero; `total_in(unit)` preserves the fractional value. `round` supports the
  nine canonical rounding modes. `abs`, `negated`, `sign`, `is_zero`, and
  `difference` operate on the exact carrier. The `ns`, `us`, `ms`, `s`, `min`,
  `h`, and `d` suffixes resolve through the canonical Time family
  (D-SHAPE-DURATION1=A, D-SHAPE-DURATIONCONVERT1=A, D-TIMERES1=A).
- Civil time keeps calendar and elapsed time separate. `LocalDate` provides ISO
  week fields, construction, calendar replacement, period arithmetic, checked
  formatting, and `until`/`since` with explicit rounding. `LocalTime` keeps
  nanoseconds and supports `Duration` arithmetic, rounding, differences, and
  checked formatting. `DateTime` provides Unix second/microsecond/nanosecond
  views, sub-second fields, `Duration` and `Period` arithmetic, rounding,
  replacement, checked formatting, and `until`/`since`; `in_zone` changes its
  view without changing the instant. `ZonedDateTime` adds explicit DST
  disambiguation, RFC 9557 round trips, transition queries, `start_of_day`,
  `hours_in_day`, `with_time`, and `with_zone`. `Period` exposes its parts,
  sign, absolute/negated/add/sub operations, and anchor-dependent totals.
  `Instant.elapsed()` returns a `Duration`. `largest_unit` is accepted by the
  Temporal-shaped difference calls, but Jet's canonical return remains an
  exact `Duration`; calendar totals need an explicit date anchor. Jet has no
  separate `YearMonth` or `MonthDay` types without a later owner-ratified type
  decision (D-TIMEDEPTH1, D-TIME-CALENDAR1).
- `true` and `false` are `Bool` literals. Source has no visible statement
  separator. The lexer inserts an internal terminator after a line ending a
  statement; a leading `.`, binary operator, or logical operator continues the
  expression (S6-R). It recovers from bad characters and reports every lexical
  error found in one run.

## Core grammar

The following compact grammar shows the surface that the rules below rely on;
the parser and [`Syntax.rs`](../../crates/jet-foundation/src/Syntax.rs) own the
complete token ledger. `;` is shown only as the internal terminator `NL`, not as
source syntax.

```text
program  = { item | module-binding | script-stmt } ;
script-stmt = stmt ;
item     = func | struct | enum | trait | impl | alias | distinct | use | test
         | comptime ;
func     = [ "pub" ] "fn" ident [ type-params ] "(" [ params ] ")"
           [ result ] body ;
result   = ( "->" type [ "from" ident { "|" ident } ]
           | effect-row [ "->" type [ "from" ident { "|" ident } ] ]
           | "!" error-type )
           [ effect-row ] ;
effect-row = "-[" [ effect { "," effect } | ".." ident ] "]>" ;
body     = "{" { stmt } [ expr ] "}" | "->" expr ;
params   = param-or-zone { "," param-or-zone } ;
param-or-zone = param | "/" | "*" ;
param    = [ "#Root" ] ( [ "&" | "^" ] "self"
           | [ "&" | "^" ] ident [ ident ] ":" [ "&" | "^" ]
             [ "..." ] type [ "from" ident { "|" ident } ]
             [ "{" expr "}" ] ) ;
module-binding = [ "#Track" ] ident ( "::" | ":=" ) expr NL ;
comptime = "@" ident "::" expr NL ;
stmt     = binding | assign | if | loop | "break" | "next"
         | "return" [ expr ] | result-handler | expr ;
pattern  = ident | ".{" ident { "," ident } [ "," ".." ] "}"
         | "[" [ ident { "," ident } ] "]" ;
fence    = "<:" ( ident | expr ) { "," ( ident | expr ) } ":>" ;
assign   = place ( "=" | "+=" | "-=" | "*=" | "/=" | "%="
                 | "&=" | "|=" | "^=" | "<<=" | ">>=" ) expr NL ;
```

A parameter's access marker is on its type (`x: &T` or `x: ^T`); receivers
write `&self` or `^self`. A type is never written between a binding name and its
binding sigil. Types ride values (`Type{…}` or an expected-type-inferred
`{…}`), or appear on signatures and fields. The old `name: Type :: value` and
`name: Type := value` forms are retired (D-BIND-BARE1=A).
View-typed parameters may add `from owner` or `from owner | other_owner`; the
names identify sibling owners in the published provenance relation.

`if` and `loop` also have expression forms. The essential productions are:

```text
if       = "if" cond effect-body
           { "else" "if" cond effect-body } [ "else" effect-body ]
         | "if" [ subject comparison ] "{" arm { arm }
           [ "else" "->" arm-body ] "}" ;
arm      = arm-head "->" arm-body NL ;
loop     = [ ident "::" ] "loop" loop-head loop-body ;
loop-head= [ cond ]
         | source-clauses [ "if" cond ]
         | ident ":=" expr "," cond [ "," expr ] ;
source-clauses = source-clause { "," source-clause } ;
source-clause = ( ident | "(" ident "," ident ")" ) "in" expr
                [ "," expr ] ;
loop-body= effect-body | "->" value-arm-body ;
effect-body = block | "->" non-if-stmt ;
```

The expression parser uses precedence climbing. From weakest to strongest, the
relevant operators are `||`, `&&`, comparisons, `|`, `~|`, `&`, shifts, `+` and
`-`, `*`, `/`, `/%`, `%`, `%%`, unary `-` and `!`, power `^`, then calls,
field/index access, identifiers, literals, and parenthesized expressions.
`|` is bitwise OR, `&` is bitwise AND, and `~|` is bitwise XOR
(D-BITOREXPR1, D-XORSPELL1, D-EXPSEM1).

## Values, expressions, and calls

Jet's primitive value types include `Int`, `Float`, `Bool`, and `String`, as well
as the fixed-width numeric, character, collection, option, result, tuple, and
user-defined types described below. Local inference keeps the type on a value
head when needed; a headed literal whose fields or elements do not match its
head is an ordinary type error.

An expected tuple type flows to its fields by position or by field name.
For example, a declared `(lines: [String], at: Int, message: String)` return
type gives an empty `lines: []` field the element type `String`; it does not
default that field to `[Int]`. The same contextual rule applies at call arguments.

An executable entry is `fn run`. An executable `run` has no parameters, or has
one parameter whose type is a CLI-derived program struct; it returns `()` or a
unit-fallible result. `run` is not `pub`. The checker gives an omitted entry
contract the default fallible `Result<(), Err>` carrier, so source may use a
plain `fn run() { … }`. An explicit unit-fallible contract names its error,
for example `fn run() !IOError`; a bare `!` is not a contract. Other output
kinds have their own entry contract. Notebook and REPL evaluation may construct
an explicit entry for the submitted fragment (S12, D-CLIFLAG1,
D-FAIL-EXIT1=A).

At ordinary file scope, `name :: value` declares an immutable module global and
`name := value` declares a mutable module global. A mutable global's initializer
is a tier-stable scalar literal; an immutable global's initializer is literal
data (numbers, text, struct, enum-case, list, and map literals, nested freely),
which the compile-time evaluator folds once, and every read is an ordinary
copy (D-MODULE-VALUE1). Both are visible to functions in the file; only the
mutable form may be assigned after initialization. Calls, operators, and name
reads in an initializer are E0622; computation belongs in an explicit function.
Inside a function, the same spellings are local bindings. A top-level executable statement is
rejected with E0621; Jet does not synthesize an implicit runtime function.
The `const` keyword is retired and is recognized only to teach its replacement
(E0146). A compile-time constant is an ordinary ALL_CAPS name, `NAME :: value`,
described below.

Names cannot shadow an existing name in the same scope (E0118), and definitions
are unique (E0105). A name that would shadow a built-in is rejected with E0106,
including a type named after a built-in type such as `Unit`, `Queue`, or `Set`;
unknown names and types are E0102/E0107 and E0119, with suggestions where the
checker has a useful candidate.

A statement fence expands one complete binding or expression statement per
entry. Multiple fences advance in lock-step. An ascending numbered range such
as `<: task1..task8 :>` creates or reuses the corresponding names. An expression
fence can also expand an ascending integer-literal range, so `<: 0..3 :>` has
four entries, while `<: (0..3) :>` keeps one range. Descending or non-literal
ranges remain one `Range` value. Expression fences accept expressions, as in
`print(<: "a", total(1, 2) :>)`; binding fences accept plain names. A fence is
not a list or destructure (D-EACH1=C, D-FENCE-GLYPH1=A, D-FENCE-RANGE1,
D-FENCE2=A).

`#Track name :: value` and `#Track name := value` attach the origin fact. Read
that fact as `value.$origin -> ?OriginInfo`; there is no runtime origin
projection (D-TRACK-ORIGIN1=A).

Arithmetic `+`, `-`, `*`, and the numeric division operations use the registered
numeric widening law. `%`, `&`, `|`, `~|`, `<<`, and `>>` are integer-only.
`+` on `String` is a teaching error that points to interpolation. Compound
assignment follows the corresponding binary operator. Comparisons use the same
numeric widening law and return `Bool`; nonnumeric operands must have matching
types. `&&`, `||`, and `!` operate on `Bool` (E0110). `&&` and `||` are boolean
operators; a value alternative in an arm head uses `|`.

A control construct is an expression when it produces a value. Its result has a
type, but `If` and `Loop` are not types: deferred control uses lambdas, and
inventing construct types would duplicate that mechanism (I8). The syntax and
semantics of a value-producing `if` or `loop` are therefore the same at every
execution tier.

`print` is an ambient prelude operation. It accepts one or more printable
arguments and writes one line per argument with a trailing newline. `eprint` is
the ambient stderr twin.
The explicit `core.term.print` and `core.term.eprint` operations each take one
`String`; use them for a qualified call rather than relying on ambient aliases.
`Float` prints a decimal part, so a negative whole-valued float is `-5.0`, not
`-5`. `input()` or `input(prompt)` reads one line, removes its trailing newline,
and returns `String !IOError`; use `??` to handle the error. `panic` is the
bug-stop builtin; `assert` and `assert_eq` are registered assertion builtins.
Other readable aliases are `Clock`, `Instant`, `Date`, `Duration`, `Path`,
`read_file`, `write_file`, `file_exists`, and `channel`. The comptime-gated
names are `embed_file`, `embed_bytes`, `find`, and `fetch`; `random` remains
qualified as `core.math.random` (D-NAME-ALIAS1=A, D-CORE-PRELUDE1=A).
A user declaration shadows a readable prelude alias; the language-owned built-in
names remain reserved. `#NoPrelude` disables the readable prelude aliases for that
file, so use the qualified Core call instead (D-NAME-ALIAS1=A, D-PRELUDEX1=A).
The readable prelude is a closed set. A candidate must satisfy its seven
membership tests: measured frequency; total and safe behavior; names that never
carry semantics; no better home; first-hour coverage; one fixed set for every
file; and collision-conscious naming. A user declaration that replaces an alias
earns L0510; adding a name uses the edition migration lint L2001. Prelude entries
may be total or return a `Result`, but may not add an implicit conversion
(D-CORE-PRELUDE1=A).

Functions support multiple arguments, checked arity (E0104), and checked
argument types (E0112). A function with a non-unit success result must return
that result on every path (E0114). A parameter may have a default, written on
its type as `fn f(x: Int{0})`. A call-site label binds by the public parameter
name, so `f(x: 1)` may omit a default and labelled arguments may be supplied in
any order. `/` closes the positional-only zone; `*` opens the label-only zone;
`timeout seconds: Int` publishes `timeout` while the body reads `seconds`.
Expressions supplied by the caller run left to right; unbound defaults run in
declaration order. The same mapping applies to free functions, methods,
constructors, generic calls, and function values. `jet fmt` preserves labels as
written. A positional `Bool` in a public function or method earns advisory lint
L2401 (S61, D-NARG1, D-NARG2, D-APILABEL1=A).

Bare arguments fill parameter slots left to right. Defaults then fill unbound
slots, and a final `...T` parameter packs the remaining tail. Types never reroute
a bound value. An imported candidate set must have exactly one successful
binding; two successes produce E0772 and require labels to disambiguate
(D-CALLPOS1=A).

Ranges are values. `a..b` is inclusive and `a..<b` is half-open; both are
`Range` values over `Int` that can be stored, passed, returned, looped, or used
as slice bounds. A range exposes `.start`, `.end`, and `.contains(value)`.
Literal range loops can lower directly to jumps without allocation. Bounds and
stride expressions evaluate once from left to right. A stride is a positive
`Int` and is checked before the first pull (S22, D-RANGE-VALUE1=A,
D-RANGE-EXCL1=C).

## Branching, results, and loops

`if` is Jet's one branching form. Its canonical multi-branch form is an ordered
arm table: `if subject == { … }` names a subject, while `if { … }` uses the
implicit subject for guard arms. A head can be a value, a structural pattern,
or any `Bool` expression; unrelated head forms may coexist. The first match or
true guard wins. Chained `else if` remains legal, but an arm table is the
preferred teaching form. `->` introduces a one-statement arm or value; braces
hold multiple statements and make a scope. Arm tables can yield `()`; value
branches require `else` unless a closed subject is exhaustive, and all arm
results unify (D-IF1, D-IF2, D-IF3, D-IFGUARD1=A, D-IFDIST1=A).

`if subject == { head -> body }` compares a bare value head with the subject.
A range head `lo..hi` tests inclusive membership. The subject and both bounds
must be an ordered scalar type (`Int` or `Char`). Open `Int` and `Char` domains
still require `else`. `..=` in an arm head is E0318 because `..` is already
inclusive; `step` in an arm head is E0319 because stride belongs to loops; an
inverted or empty band is E0316. Range heads are literals. A `distinct Int`
constraint likewise requires literal bounds because a runtime `Range` cannot
define a type (D-PATR, D-RANGE1, D-RANGE-VALUE1=A).

A fallible result can use the fixed exhaustive handler
`result ? ok -> success ! error -> failure`. `ok` and `error` are branch-local
payload bindings. The parser lowers this form to the ordinary `.Ok` and `.Err`
pattern tests, so normal typing, effects, divergence, ownership, and execution
tiers apply. The `?` here is contextual; postfix propagation, `?.`, unary `!`,
and `??` keep their ordinary meanings (D-RESULT-DECON2=B).

When the subject of `if subject == { … }` is not a plain name — a call or a
field access, for example — the arms refer to it as `it`, as in
`it == .Ok(n)`.

`loop` supports infinite, conditional, source, map-pair, and explicit-state
forms:

```jet
loop value in source, stride if keep(value) -> value
loop (key, value) in source -> key
loop i := init, i < limit, i + 1 -> print(i)
```

A source loop may use `-> expression` or a braced value body. Each accepted
iteration yields one non-unit value. The eager result is a `List` in iteration
order; a guard or `next` omits an item. Multiple source clauses produce one flat
list. An explicitly nested collecting loop preserves nesting. Maps and sets use
explicit terminals; lazy work uses iterator adapters. In statement position,
a finite source, infinite, condition-only, or mutable-state loop may use
`-> statement`; its value is discarded. A discarded non-unit value earns the
registered lint. Bind a collecting loop with `::`, or use a write handle for an
in-place update. A value loop returns a final value only through `break value`
or `break(name, value)`; all exits unify. A collecting loop returns its partial
list and rejects payload breaks (D-COMPREHENSION1).

`break` and `next` are legal only inside loops (E0115). A loop may have an
ordinary-name label, `outer :: loop … { … }`; `break(outer)`,
`break(outer, value)`, and `next(outer)` target it from a nested loop. E0987
reports an out-of-scope label. E0988 teaches retired loop-name and `@` forms,
rejects `outer := loop`, and explains that a loop name is not a runtime value.
Normal explicit-state fallthrough and targeted `next` run the afterthought once
and retest. Source fallthrough and targeted `next` pull the stride and use the
final pull. `break`, `return`, propagated failure, and panic skip the target
afterthought; abandoning an inner loop runs no edge. Bare `next` is control only
as a complete statement or `??` fallback. `next()`, `.next()`, and `fn next`
remain ordinary identifier uses; a value named `next` after `??` needs
parentheses: `value ?? (next)`.

## Compile-time names and staged syntax

A compile-time constant is an ordinary ALL_CAPS name: an immutable module-level
`::` binding such as `MAX_RETRIES :: 3` is evaluated while building and read as
`MAX_RETRIES`; failure to compute it stops the build (D-PREP-SURFACE2=A). In a
block, names stay snake_case (D-SHAPE-CASE1): `lanes :: prep { calculate() }`
evaluates its single final expression while building, and names bound inside a
`prep { … }` statement block stay readable after it. Ordinary foldable expressions need no constant. At file
scope, other `name :: value` and `name := value` bindings remain runtime module
globals; they do not require `#Persist`. `#Static NAME :: value` requests a
stable-address Rust static when the contract needs one, and `#Inline NAME ::
value` copies the value into use sites (D-CONSTMARK1). Without a marker, an
immutable constant whose value is a number, text, or a small record or enum
case of those is copied into its use sites; a list or map is read through one
shared item instead, and a large one is built once. The retired `@NAME`
spelling teaches E0388. `#Persist name := value` additionally marks hot-reload state on a bare
binding (D-VERDICT-1308-1, D-PERSIST1).

`embed_file("path") -> String` embeds UTF-8 text, `embed_bytes("path") -> [U8]`
embeds raw bytes, and `find("glob") -> [String]` returns sorted relative paths.
`find` accepts the std-only glob forms `*`, `**`, `?`, `{a,b}`, and `[a-z]`.
They are the sanctioned build-time I/O operations inside a compile-time binding;
other comptime evaluation is pure.
Paths and globs are string literals resolved relative to the embedding file, never
absolute and never escaping the project with `..` (E0957). Missing or unreadable
input, and non-UTF-8 input for `embed_file`, are E0955. Every embedded or matched
file records its SHA-256 in `.jet/lock` (D-CTIO1, D-CTFIND1/2, D-META-EFFECT1).

When a ratified feature has a staged diagnostic, unsupported syntax receives that
registered error and its milestone rather than a generic syntax failure. A spelling
with no ratified language rule is not silently accepted as a new feature: the
parser or sema may recognize a retired spelling to emit a teaching diagnostic and
its canonical fix; otherwise it is an ordinary parse or type error. Old Jet and
foreign-syntax teaching remains paused until post-Epoch 6 (D-S14-PAUSE); active
docs and fixtures use canonical syntax only.

## Data types and methods

Structs and enums carry data; methods attach behavior. A struct literal is
`Type{field: value}` or an expected-type-inferred `{field: value}`. Enum values
use `Type.Variant`, and pattern tests use `==`. Optional values use `T?`,
`Val(value)`, and `None`. `None` needs an expected optional type, such as a
return, field, or argument type; `Val(value)` uses that type to check its
payload, or infers the optional type from its payload when no type is expected.
`None` is not legal for a non-optional `T`. Generic
arguments use `Type<Args>`. Fresh hidden-state construction uses `Type.new(…)`.
Expected-type elaboration permits `.new(…)` when a binding, return, field, or
call argument determines exactly one receiver. Generic receiver arguments may
also be omitted from `Type.new(…)` when constructor inputs and expected type
force one answer; otherwise write `Type<Args>.new(…)` (S27, S29–S33,
D-SHAPE3a, D-SHAPE-OPAQUE-INFER1).

```jet
struct Circle {
    radius: Float

    fn area(self) -> Float {
        3.14159 * self.radius * self.radius
    }
}

impl Circle {
    fn unit() -> Circle {
        Circle{radius: 1.0}
    }
}
```

`Type{ body }` is the one typed-literal head for every type (D-DOTCTOR3): the
body uses the type's literal notation, such as elements for `[U8]{42, 0}` and
entries for `[String:Int]{"a": 1}`. A body of exactly one expression is decided
by its type. When the expression is a value of the head type, or one that a
parameter of that type accepts, the result is that whole value:
`[Int]{[1, 2, 3].map(n -> n * 2)}` is a three-element list, `[String:Int]{m}`
is the map `m`, and `Float{rows()}` widens an `Int` with the checked rule a
`Float` parameter applies, stopping with E3010 when the number has no exact
Float. Otherwise the head's notation applies, so `[[Int]]{xs}` is a one-element
list when `xs: [Int]` (D-DOTCTOR3 amendment, 2026-09-28, #3739).

`self` is the receiver and follows the same access law as any parameter. In an
`&self` method, field assignment, compound assignment, and whole-receiver
replacement are legal. The same write in a read method is E0205. Calling a
write-receiver method requires a changeable receiver binding and is E0202.
Invoke an instance method as `circle.area()`, not `area(circle)`. Methods may
live in the type, in `impl Type { … }`, or as a top-level external inherent
method `fn Type.method(self, …) { … }`; the type must be defined in the current
source module. Static methods omit `self`, as in `Circle.unit()`.

Multiple construction shapes use distinctly named no-`self` statics such as
`Point.cartesian` and `Point.polar`. Overloading is rejected; a duplicate name
is E0105. Enum `if subject == { … }` tables must be exhaustive.

Traits contain signatures and implementations contain behavior. Implement a trait
inside its type or with `impl Type.Trait { … }`; qualify a foreign type when the
module owns the implementation:

```jet
trait Shape {
    fn area(self) -> Float
}

impl Circle.Shape {
    fn area(self) -> Float { 3.14159 * self.radius * self.radius }
}
```

A trait name in type position, such as `[Shape]` or `fn f(shape: Shape)`, means
dynamic dispatch with invisible boxing. Generic bounds use `fn f<T: Bound>(…)`
and `struct Pair<T> { … }`. Built-in `Printable`, `Equatable`, `Debug`,
`Comparable`, `Encode`, and `Decode` derive when every field qualifies. The
package default can deny `auto_derive` through
`policy: .{ lints: .{ deny: [auto_derive] } }`. A signed type marker opts one
trait in or out (`#Debug`, `#!Debug`), while a hand-written implementation wins.
`#Codable` requests both codec directions; `#Encode` and `#Decode` request one
(D-AUTODERIVE1=E, D-AUTODERIVE-SYNTAX1=D).

### Encoding and validation

`Encode.encode(self) -> DataTree` and
`Decode.decode(tree: DataTree) -> Self ![FieldError]` are ordinary trait methods.
`DataTree.decode<T>()` is the public typed-dispatch path for primitive,
container, generated, and hand-written implementations. A built-in derive adds
typed Jet items beside the marked type and sends them through the same sema, TIR,
and codegen path as hand-written members. JSON therefore compares the same
encoded bytes and decoded values for generated and hand-written codecs. A
user-defined derive expands only when its provider or target is entry-local;
otherwise E2711 points at the derive marker (D-SERDE2, D-SERDE16,
D-META-CODE1, D-META-BODY1).

A struct's `validate { … }` block declares checks in the exact form
`check(condition, at: field, "message")`. `field` names a sibling field. Every
failed check accumulates a `FieldError` with its `path` and `reason` in
`[FieldError]` rather than failing fast. Sema requires this statement shape
(E0353), a real field (E0354), and pure references (S60/E3401).
`Type.validate(value)` returns `value ![FieldError]`. A derived decoder validates
through the same list; hand-written codecs opt in explicitly. `Validate.over(s)`
builds outside-context checks and `finish()` returns `T ![FieldError]`
(D-VALIDATE1, D-VALIDATE-DECODE1=B).

### Computed fields, tags, and applied rules

`name: T -> expr` is an unmarked read-time formula over sibling fields. Put bare
`#Memo` immediately before the field to retain the result after its first read;
writes to stored dependencies invalidate it. A memoized field is read-only and
is not supplied in a `Type{ … }` literal. Arguments to bare `#Memo` are E0382.
See [`computed_field.jet`](../../Examples/features/memory/computed_field.jet)
(D-FIELDPOL1, D-FIELDMEMO1).

`tag Name { deny: [Net] }` declares an erased dataflow fact and its policy.
`deny` is required and nonempty; `from` is optional. Direct `#Name` tags attach
to values, fields, parameters, and returns. `#Scrub(Name)` removes exactly that
tag. Tags have no methods: a method in a tag body is E0732, and using a tag
where dispatch or method attachment is expected is E0731. Tag names are
PascalCase. Prelude declares `Input`, `PII`, `Secret`, and `Credential`
(D-QUAL2, D-TAG-SURFACE1, D-CASING1).

`#Rule` or `#[A, B]` applies a rule on the line before a declaration. Block
markers use PascalCase and parenthesized arguments when needed. An explicit empty
effect row is `-[]>`. Compile-time demand uses the prefix `@`; the retired `$`
spelling is not an alias (D-ONCE-AT1=D).

`#Off <stmt>` parses and type-checks one statement but emits no code in any
build. `#DebugOnly <stmt>` type-checks in every build and emits only in debug or
dev builds. Names introduced inside either marker are scoped to that marker body.
The registered build-profile fact is `$build.profile`; bare `build.profile` is
not a user-typeable comptime value (D-CANVASSTATE1).

`#Meta(category: "Movement", tunable)` attaches checked tooling facts to a
binding, top-level const, or function. `category` is a non-empty plain string
literal and `tunable` is a bare flag. The marker emits no code and changes no
runtime behavior (D-CANVASMETA1).

### Target selection and build-time embedding

`#Target(OS.Linux | MacOS | Windows)` gates an `impl` to a native operating
system. `jet build --target=<triple>` emits only matching implementations and
uses the host OS by default. Ungated code can select the surviving implementation
with the compiler-known switch

```jet
prep if $build.os == {
    .Linux -> …
    .MacOS -> …
    .Windows -> …
    else -> …
}
```

The switch folds before target-gating checks. Arms must cover each OS or provide
`else` (E-OSTARGET-DISPATCH-EXHAUSTIVE); the subject must be `$build.os`
(E-OSTARGET-BUILD-CONTEXT), and arm heads must be OS variants
(E-OSTARGET-DISPATCH-ARM) (D-OSTARGET1, D-OSTARGET2).

Inside an `@` binding, `embed_file`, `embed_bytes`, and `find` are the only
sanctioned build-time file operations. Their path, hash, and purity rules are
defined in the compile-time section above.

### Published schemas and migrations

`#PublishedSchema struct Name { … }` marks a public record whose field layout is
snapshotted under `.jet/cache/schema/`. A later project build compares the
current shape with that snapshot by field name, ignoring order. A breaking shape
change is E0910 unless a `migration` block declares it. The four operations are:

```jet
migration UserRecord {
    rename name -> display_name
    remove legacy_id
    add verified: Bool = false
    change price: Int -> Usd via { c -> Usd.from_int(c) }
}
```

`rename` targets an existing field of the same type. `change` resolves its
converter in order: inline `via { … }`, an in-scope `impl Old -> New`, then an
E0910 asking for one. `add` supplies the value for old records. There is no
`reorder` operation because field order is not a breaking shape change. `drop`
and `reorder`, and other unknown verbs, teach E0911. An operation that contradicts
the real shape is itself an E0910-family error. A single-file run accepts the
marker but enforces the comparison only when a project snapshot exists
(D-MIGRATE1, D-MIGRATE2A/B/D/E/F).

`jet inspect schema status` lists snapshotted published types, their pinned
version, and fields, and flags pending E0910 changes. `jet inspect schema squash
--before <ver>` rewrites snapshots to the current struct shape, records the
squash boundary, and changes only `.jet/cache/schema/`. There is no
`jet inspect schema check`; the E0910 from `jet build` is the CI gate
(D-MIGRATE2C).

Typed `decode<T>` is the one codec contract and returns `T ![FieldError]` (or
`[T] ![FieldError]` for CSV). Schema migration is silent inside that call; it
has no second decoder or migration-report result (D-MIGRATE3=A as amended by
D-VALIDATE-DECODE1=B).

For a concrete `#PublishedSchema` type that derives `Decode` and has migration
blocks, decode first tries the current shape. If that fails, it compares the
wire key set (after any `#Rename`/`#RenameAll` treatment) with historical shapes
from newest to oldest, rewrites the first match forward in source order, and
decodes the current shape. The `rename` operation moves a
key, `remove` drops it, `add` evaluates its default, and `change` decodes,
converts, and re-encodes the old field. A converter and an `add` default are
ordinary Jet expressions checked and lowered through the normal pipeline. If no
shape matches, the original decode error is returned. Types with no migration
blocks pay no chain cost, and CSV applies the chain per row (D-MIGRATE4=A).

### Struct layout

`#Layout(c)` before a struct requests C layout and preserves field order for
foreign sharing. Growable fields (`[T]`, `[K:V]`, and `String`) are rejected with
E1104 because they have no stable C layout; fixed arrays `[T#N]` are allowed.
Reserved `packed`, `align(N)`, and `columnar` forms parse but report E1105 until
their contracts are defined (D-REPRC1).

## Fixed-size lists

`[T#N]` refines a list to exactly `N` elements of type `T`. Destructuring must
use exactly `N` names. At code generation the representation is `Vec<T>`, as
for `[T]` (S76).

```ebnf
type_fixed_list = "[" type "#" int_literal "]" ;
```

```jet
result :: [Int#3]{2, 4, 6}
[a, b, c] :: result
```

A wrong destructuring count is **E0963**. `push`, `pop`, `insert`, `remove`,
and `clear` on a fixed-size list are **E0964**. A literal index outside
`0..N-1` is **E0965**. A proven `distinct Int(lo..hi)` or sized-integer
interval may index without a runtime bounds check when `lo >= 0` and `hi < N`
(D-TYPE2-REFINE1).
Finally, `[T#N]` widens to `[T]`; the length fact is erased at that coercion.

## Closures and function values

A lambda is `(params) -> expression` or `(params) -> { ... }`. A single
assignment or a unit call needs no extra braces after the arrow. Parameter
annotations may be omitted when an expected function type supplies them; with
no expected type, omitted annotations report **E0801** (S46).

```jet
square :: (n: Int) -> n * n
increment :: (n: Int) -> {
    n + 1
}
```

`->` is the callable and control arrow. A named function or method has exactly
one arrow after its inputs, `->` or an effect ceiling `-[Effect]>` / `-[]>`,
followed by its optional result type and then a braced body:
`fn f() -> T { expr }`. A unit or unit-fallible callable keeps a bare braced
body. A second body arrow (`fn f() -> T -> expr`) or an unbraced body after the
arrow reports **E0080**; `jet fmt` rewrites it to `{ expr }` (D-SIG-AFTER1=A).

Function values use a function type with parameter types and a result:
`fn(T1, T2) -> R`. A result may be omitted for a unit callback. A pure
function-value effect bound is written `fn(T1, T2) -[]> R`; other effect sets
replace the empty set. Unmarked parameters have plain read access. A named
function becomes a value only when every parameter can be called with that read
access; parameters requiring `&` or `^` keep the function direct-call-only
(S47, D-MEM-PARAM1).

Direct named and method calls keep their ordinary syntax. A function-valued
expression is invoked with `.call(...)`; a struct field or method actually
named `call` shadows that built-in projection. Direct calls on names, fields,
indexes, and lambdas remain valid. The adjacent `)(` function-value spelling
is rejected with **E-CALL-VALUE** (D-CALLVALUE1=B).

```jet
fn apply(f: fn(Int) -> Int, n: Int) -> Int {
    f.call(n)
}

fn make_adder(base: Int) -> fn(Int) -[]> Int {
    (n: Int) -> base + n
}
```

### Captures and escaping lambdas

A lambda captures a name for shared read access when the body only reads it.
Writing a captured name requires a mutable `:=` binding; otherwise the compiler
reports **E0111**. A lambda bound to a local that is only ever called directly
in that scope does not escape: it borrows each `:=` local it writes, so the
write lands on the owner (`count := 0; bump :: (n: Int) -> { count += n };
bump(2)` leaves `count` at 2). An escaping lambda—one stored in a binding that
is passed, stored, or returned, returned itself, stored in a struct field, or
passed to a `^T` parameter—must own its captures.
Copy values are copied at closure creation; other clonable values are cloned;
an owned non-clonable value moves. A borrowed non-clonable parameter cannot
escape (**E0120**). The old `take(...)` prefix is rejected with **E0057**.
Self-recursion through the lambda's own binding is **E0804**; calling a
non-function is **E0803** (S47).

Function values expose only read parameters when coerced from a named
function. This rule prevents the value conversion from erasing a write or
move requirement. The same capture and call checks apply to interpretation,
JIT, and generated programs.

### Collection adapters

Concrete lists provide eager `map`, `filter`, `each`, `find`, `any`, `all`,
`sort_by`, and `reduce`; maps provide `each` with key and value parameters.
Call `.lazy()` to enter the deferred `Iter` vocabulary. The lazy adapter set
includes `take`, `skip`, `step_by`, `dedup`, `chunks`, `windows`,
`take_while`, `skip_while`, `flat_map`, `scan`, `fold`, `position`, `min_by`,
`max_by`, `group_by`, and `partition`. `indexed()` yields `(idx: Int, item: T)`
(D-ITER1, D-EXT1 Tier 1, D-CORE-EAGER1).

The zip family is variadic and named. Strict `zip` requires equal lengths and
reports **E0128**; `zip_short` stops at the shortest input; `zip_pad` reaches
the longest input and uses `None`, one typed `fill:`, or typed per-column
`fills:` values. Free calls preserve labels; method calls use `a`, `b`, `c`,
and so on. Zero free inputs produce an empty `Iter<Unit>`; one input is the
identity. `partition(f)` returns `(false_: [T], true_: [T])`. These adapters
are lazy on `Iter`, while concrete list, map, and set `map` and `filter` are
eager.

`first()` is the consuming positional terminal. Use `skip(n).first()` for a
zero-based selection; an out-of-range selection returns `None`. There is no
`nth` adapter. `min_by` and `max_by` retain the last source item when keys
compare equal. The examples are `Examples/features/basics/closures.jet`,
`Examples/features/basics/callbacks.jet`, and
`Examples/features/collections/iter_adapters.jet`
(D-S14-PAUSE, D-SHAPE-PIPE1=C, D-BITOREXPR1=A).

The lambda diagnostics are covered by `tests/ui/lambda_*.jet` and
`tests/ui/not_a_function.jet`; integration coverage is in `tests/closures.rs`.

## Errors as values

### Failure contracts and propagation

A fallible value has one shared result carrier. A success type `T` uses the
implicit default error family `Err` when it is used in a fallible context. Add
`?` before the success type when success may be absent, and add `!` before a
named error domain. The supported forms are:

- `T` — a non-optional success with the implicit `Err` failure route;
- `?T !E` — an optional success with error `E`;
- `T !E` — a non-optional success with error `E`;
- `!E` — unit success with error `E`.
- `T !(E1 | E2)` — an explicit error union; prefix the success type for
  `?T !(E1 | E2)`. (D-UNIONTYPE1=A)

The explicit `!` must name an error domain; ordinary failure leaves the error
contract implicit. The entry function `fn run()` has the default fallible
`Result<(), Err>` carrier, so it may propagate a standard-library failure
without writing a return annotation. Pin an entry to an application error with
`fn run() !StoreError { ... }` when that distinction is part of the interface.
(D-FAILURE-FOUNDATION1, D-FAIL-EXIT1)

Construct the two sides with `Ok(value)` and `Err(error)`. These are contextual
calls: a user declaration named `Ok` or `Err` takes precedence. `Ok()` supplies
the unit success value when the expected result has unit success. Leading-dot
`.Ok(value)` and `.Err(error)` values use the expected result type; in pattern
tests, they match the corresponding side, for example `result == .Ok(n)`. A value of an
optional-success type such as `User? DBError!` has exactly three states, and
its patterns name them directly: `.Val(user)`, `.None`, and `.Err(e)`. An
else-less table over it must cover all three (E0307 names the missing state).
The nested `.Ok(.Val(user))` / `.Ok(.None)` spelling is retired and refused
with E0392, whose edit writes the flat state. A table with an `.Err` arm
handles the failure itself; without one, the failure passes up and the
`.Val`/`.None` arms test the optional. (D-OUTCOME-SHAPE1=A)

A failure conversion is one declared rail:

```jet
impl DiskError -> StoreError {
    fn convert(error: DiskError) -> StoreError {
        StoreError.IO
    }
}
```

The conversion applies when a fallible call propagates. A conversion into the
default `Err` may name a foreign source type. A typed target must obey the
orphan rule: the declaration must be owned by the source or target side. The
compiler reports an undeclared conversion as E2402, a duplicate as E2405, and a
violation of the typed-target rule as E2406. (D-ERR-CONV, D-FAIL-CONV1,
D-FAIL-CONV2)

A fallible call propagates its failure automatically: success continues with
its payload and failure returns from the current fallible context. A missing
value never leaves a function by itself: it stays an ordinary `T?` value until
the code handles it with `?.`, `??`, or a `.Val`/`.None` pattern, and reading a
field of it directly is E0310 (D-OUTCOME-SHAPE1=A). The postfix form `?(text)`
is not a second propagation operator; it adds one failure-context frame to the
report. Use it when a boundary needs a local explanation. (S7, D-FAIL-CTX1)

`??` is the fallback operator for an optional or fallible value. It yields the
success payload or evaluates its right side. Its precedence is looser than
`&&` and `||`, and the right side may be a value, `return`, `return expr`, or
`panic(...)`. `??` is the only fallback spelling; it must produce the
enclosing success type, otherwise the compiler reports E0405. (S35, S71)

`panic(message)`, failed `assert`/`assert_eq`, `#Todo`, a raw Prelude panic, and
scheduler, stream, or foreign-boundary stops enter the shared runtime report
boundary. Program-side runtime breaches use E3001 and the process-stop boundary
uses exit code 70; an explicitly requested process exit keeps its requested
code. An unhandled error returned by the entry function is rendered as a full
report and exits 1. E3002 records the trail of `?(text)` frames, while E3003
reports an expired wait or I/O deadline. See [diagnostics.md](diagnostics.md)
for the complete report shape and fixes. (S36)

### Cleanup at a process stop

An explicit stop runs active cleanup before the process ends. `defer close(...)`
actions run in reverse declaration order, `scope.guard` closures run in reverse
registration order, and `os.atexit` handlers run in registration order after
scope cleanup. Work registered after the stop does not run. A host kill or an
abort does not promise these finalizers.

### Handling and discarding failure values

The compiler rejects an unchecked fallible value (E0401), a fallible call used
as a bare statement (E0402), a discarded `#MustUse` result (E0419), a failure
that does not fit the enclosing domain (E0403), `Ok`/`Err` outside a result
context (E0404), and a bad fallback (E0405). Handle the value, propagate it,
or bind it. The sole intentional-discard spelling is
`.drop("reason")`; the reason is part of the source-level audit trail.
(D-IGNORERET2, D-MARK-DISCARD1)

Two other lost values are errors (E0433, D-DISCARD1=A). A statement that
calls a pure function and ignores a non-Unit result does nothing: the call is
pure when its solved effect row is empty, it cannot fail, and it passes
nothing with `&`. Calls with effects or `&` arguments may still drop their
result. The last line of a named function with no written return type is
the other case: the function returns nothing, so a non-Unit value there is
lost, and the fix is the missing `-> T` or `.drop("reason")`. Lambdas are
unaffected, because their result type comes from the body. When a return type
is written and the last line does not match it, E0113 reports the value's
type or shape and names the conversion or wrap; the declared return type is
the contract and is never the suggested fix.

## Physical dimensions

The compiler knows the dimension identities of `Length`, `Time`, `Speed`,
`Area`, and `Temperature`. Addition, subtraction, and comparison require equal
dimensions; a mismatch is E0359 before code generation. Multiplication adds
normalized exponents and division subtracts them, so `Length / Time` is
`Speed`, `Length * Length` is `Area`, and `Speed * Time` is `Length`. Semantic
index and API snapshots serialize normalized identity and numeric base; runtime
values carry only the numeric base (D-SHAPE-QUANTITY1).

Currency remains nominal. A user quantity family declares a base and exact
rational scale/offset with stable package identity. A nonzero offset mints
separate `Point` and `Delta` types for each member. Sema owns closed affine
algebra and exactness: implicit conversion is value-aware and never rounds,
destination-owned exact conversion returns `Result`, and
`_rounded(value, mode, digits: n)` is the explicit fallible rounding path.
Modes are `TowardZero`, `Floor`, `Ceiling`, and `NearestEven`; `digits` is
nonnegative destination decimal places, and the rounded rational must be
exactly representable by the destination. Imported `Quantity<Dimension, Kind>`
retains its concrete unit through checking, API freeze, semantic inspection,
Codable, AOT, and JIT lowering (D-QUAL3, D-QUANTITY-DECL1).

Dimension identities retain raw UTF-8 axis names. Their percent escapes protect
only the separators `%`, `;`, and `:`; decoding does not reinterpret non-ASCII
UTF-8 bytes as separate characters.

## Ownership and borrowing

Jet has one explicit ownership contract. A parameter without an access marker is
always a read parameter; sema decides this from the signature and never raises
it to write or take access from body usage. The public surface is:

| Source type | Access | Required call-site marker |
| --- | --- | --- |
| `T` | read; the callee cannot mutate the caller's value | none |
| `&T` | exclusive write/edit access | `&` |
| `^T` | take; ownership moves to the callee | `^` |

For non-scalar values, a read call borrows the existing value without an
allocation. A body write through an unmarked parameter, or passing it to a write
or take position, is a hard error at the definition or call site; add `&` or `^`
to the contract and mirror it where the call transfers access. A cloneable read
value entering an owning destination is materialized automatically. A bare
`::` binding of a place remains a read window; non-cloneable values and
`#Policy(copies: .Explicit)` require an explicit `~` copy or an owning `^`
contract (D-MEM1, D-MEM-COPYSEM1).

```jet
fn bump(n: &Int) { n += 1 }
fn archive(name: ^String) -> String { name }

fn run() {
    score := 41
    bump(&score)
    saved :: archive(^"vault")
    print(saved)
}
```

The sigil on a method receiver is written at its definition. Plain `self` is a
read receiver; `&self` is a write receiver; `^self` consumes the receiver.

```jet
struct Player {
    hp: Int
}

impl Player {
    fn show(self) -> Int { self.hp }
    fn heal(&self, amount: Int) { self.hp = self.hp + amount }
}

fn run() {
    player := Player{hp: 10}
    &player.heal(2)
    print(player.show())
}
```

Writing through a read receiver is E0205 and points to `&self`. Calling a
`&self` method requires a changeable receiver binding and is checked with E0202.
Using the same name twice while a write access is live is E0204: pass `&x` once,
or materialize `~x` first.

### Copies, moves, and owned destinations

A named binding passed to a take parameter without `^` is E0209. Jet never hides a
clone to make that call work. A literal, `~x`, or call result is a temporary and
may enter a take parameter without another marker. `~x` is the one copy spelling;
it creates an independent owned value, including when `x` is a Bool or integer:
it does not negate the value. `.clone()` is not user-typable Jet syntax,
and the retired `copy x` word teaches E0991. Copying a value that Jet cannot
duplicate, such as a function or trait value, is E0211. Copying a scalar is
legal but redundant.

```jet
name :: "vault"
saved :: ~name
print(name)
print(saved)
```
See [`copy_verb.jet`](../../Examples/features/memory/copy_verb.jet) for the
explicit-copy form.

The compiler chooses when a name is unmarked, and a mark gives exact control
(D-COPY-DEFAULT1=A). `ys :: xs` reads a place through a view, `ys := ~xs` makes
an independent copy, and `ys := ^xs` moves `xs` exactly: `^place` is accepted
in every value position (binding, assignment, field initializer, collection
element, result), and any later use of `xs` is E0121 naming the move. `^`
before a literal or call result, or before a collection element, is E0225; on a
read or write parameter it is E0201. An unmarked use that is not the last one
shares the value instead of moving it: the source stays usable and each holder
has its own value, so a write through one never shows in the other. A use in
`return`, or in a loop's `break` value, is the last use for the loops it
leaves. Values that cannot be copied (resources, one-pass iterators, consume
duties, tasks) keep E0121 on the later use. A taking receiver call on a whole
local at its last use needs no `^`; a take elsewhere still does, and a writing
call always needs `&`. Under `#Policy(copies: .Explicit)`, every implicit copy
that `jet audit copies` reports is a diagnostic with a Safe `~` edit, and an
unmarked move that a later use reaches stays E0121.
See [`view_move_copy.jet`](../../Examples/features/memory/view_move_copy.jet)
and [`share_on_reuse.jet`](../../Examples/features/memory/share_on_reuse.jet).

### Named views and places

Raw reference syntax is not a first-class Jet value: `-> &T` return types,
`&T` fields, and `#Ref` provenance are not in the grammar. A named `View<T>` or
`ViewMut<T>` can cross a return or aggregate boundary only when sema proves a
bounded set of receiver, parameter, or static owner paths for each output slot.
Every possible owner remains live while the view is live. Lists, tuples, options,
results, enum payloads, named aggregates, callbacks, and closed trait dispatch
carry the same hidden relation. Temporary owners, unbounded dynamic dispatch,
and incompatible read/write paths are E2305, or E2307 for string views
(D-MEM-VIEWRET1, D-MEMPROVENANCE2=A).
At a string boundary, fill `View<str>` only from `.trim()`, `.after()`,
`.before()`, or a tracked string-view binding; a plain owned `String` is not a
borrowed window.

An ordinary owned field owns its value:

```jet
struct Span {
    text: String
    meta: String
}

fn describe(source: String, kind: String) {
    value :: Span{text: source, meta: kind}
    print(value.text)
}
```

Use `Shared<T>` or `Pool<T>`/`ID<T>` when many owners need one value. A plain
owned `String` is not a borrowed window. A read-only view entering an owning
slot is materialized as an owned copy, as though `~` had been written. This
covers bindings, returns, fields, collection elements, enum payloads, fallback
values, and stored read-only captures. Declared `View<T>` and `ViewMut<T>`
boundaries retain their provenance; `ViewMut<T>` is never copied implicitly.
`#Policy(copies: .Explicit)` makes read-only materialization explicit. The same
rule is shared by AOT, JIT, interpreter, comptime, and web lowering; each engine
marshals the sema-approved result.

A place is a name followed by its maximal field, index, or range projection. A
bare place is a checked read window; `&place` is the exclusive write window; and
`~place` creates independent owned storage.

```jet
values := [10, 20, 30, 40]
read :: values[0..1]
edit :: &values[2..3]
copy :: ~values[0..1]
```

The windows above are disjoint. A bare read window remains a view in a non-owning
place and materializes when it enters an owning slot. Constant disjoint indexes
and ranges lower through a safe structural split; known fields are disjoint;
dynamic projections conservatively overlap. Jet's sema, not rustc, validates
these facts. A call or temporary is not a place: bind it first (E0213). The
retired `values.view(0..1)` spelling is E0214 and points to `values[0..1]`.
Method calls do not extend a place, so `&values[0..1].sort()` writes the maximal
range and then calls the method on that window (D-SHAPE-PLACE1=A).

Sema keeps one provenance and alias graph for every borrowed window, regardless
of whether the runtime representation is a string slice, `View<T>`, arena
window, buffer, or matrix window. An owner is identified by declaration, not by
spelling: a local uses its definition identity, a public function parameter its
zero-based position, and static storage its declaration. Shadowing therefore
creates a different owner. A place is an owner plus ordered field, index, or
range projections; reborrowing preserves that owner.

Each view fact records place, read/write access, lexical extent, source kind, and
invalidation state. Read views may overlap. A write view is unique and cannot
overlap any live read or write view. Different known fields are disjoint; ranges
and indexes overlap unless sema proves otherwise. Moving or replacing an owner,
writing an overlapping place, or resizing or relocating its storage is E0212
while a view is live. Arena reset or close invalidates its views; a later read is
E0632. Facts end after the last use or at lexical scope, whichever comes first;
control-flow joins and loops keep invalidation conservatively. Captures and field
projections preserve the fact. A captured or returned view crossing a task or
channel boundary is rejected once as E1102.

D-MEMPROVENANCE2 carries a view fact through public calls, returns, aggregate
fields and elements, generic instantiations, methods, function values, lambdas,
and trait dispatch. Each output slot has a bounded deterministic source set;
branches and compatible trait implementations union their sources. All paths
for one output slot must agree on access. Open dynamic dispatch without a
proven contract, temporary owners, captured mutable views, and incompatible
access paths are E2305/E2307. Function types carry the same hidden relation;
a generic callback without a narrower declaration conservatively keeps every
compatible non-scalar argument live.

Public API snapshots publish each relation canonically. One source is
`source;access:...;path:...`; a union is
`one_of(source;path:...,source;path:...);access:...`, sorted by stable source
identity. Adding, removing, or changing a possible source changes the API digest
and is a breaking provenance change. TIR receives only sema-approved provenance
and lowering flags; it does not infer owners, overlaps, lifetimes, or escapes.

### Zero-copy string views

Binding `String.trim()`, `.after(sep)`, or `.before(sep)` to a local creates a
zero-copy view into the receiver's buffer when sema proves the binding cannot
outlive its owner. The Jet-level type remains `String`.

```jet
padded := "  nate@jet-lang.dev  "
email :: padded.trim()
domain :: email.after("@")
print("padded still readable: {padded}")
```

A local view may chain another string-view operation, be interpolated, be placed
in a view-typed aggregate, or be copied with `~`. An owning destination copies it
by default; returning that owned copy does not move the original owner. A
returned window likewise retains its owner's provenance rather than consuming
the owner's storage. At a named boundary, `View<str>` states the owner-tied contract;
E2307 rejects a temporary or unstable owner, or an explicit-copy policy that was
not satisfied. See
[`string_view.jet`](../../Examples/features/memory/string_view.jet) and
[`returned_views.jet`](../../Examples/features/memory/returned_views.jet).

### Shared state, local cells, and pools

`Shared<T>` is a lock-guarded, copyable handle. `shared value` constructs it and
infers `T` from the value. A field read takes one read lock and a field write one
write lock; a read-modify-write such as `config.hits += 1` holds one lock across
both halves. Each statement is one atomic step. Cloning a `Shared<T>` clones the
cheap handle, not the payload, so it can cross a task boundary without `^`
(D-SHARED-API1, D-CONC-SHARE1=A).

```jet
struct AppConfig {
    name: String
    hits: Int
}

fn handle(id: Int, config: Shared<AppConfig>) -> String {
    label :: config.name
    "request {id} on {label}"
}

fn run() {
    config :: shared AppConfig{name: "jet-server", hits: 0}
    t1 :: task handle(1, config)
    print(t1.join() ?? panic("task failed"))
    config.hits += 1
}
```

The lock is per statement. When one statement touches a second shared value,
the ordered shared engine avoids nesting locks and therefore avoids the plain
access deadlock class. The closure spellings `Shared.read(f)` and `Shared.edit(f)`
are retired (E1116); read fields directly. `Shared.new(x)` is retired (E1115);
use `shared x`.

Expert code can hold a lock across helper calls with `guard_read()` or
`guard_edit()`. Each returns an owned `SharedGuard<T>` and releases it on every
exit. `.map(value -> value.field)` narrows one guard to a field. `.split(first,
second)` is legal only when sema proves the stored field paths disjoint. Guards
are task-local and cannot be copied or sent. A public guard parameter reads by
default; `&guard: SharedGuard<T>` requires and preserves edit access
(D-SHAREDGUARD1=A, D-SHAREDGUARD2=A).

`Condition.new()` creates a wait set. `guard.wait(condition, predicate)` requires
an edit guard, registers before releasing the lock, reacquires the same lock,
and checks the predicate again. Cancellation unregisters the waiter before final
release. `notify_one()` wakes one waiter and `notify_all()` wakes all waiters.
See [`shared_guard_queue.jet`](../../Examples/features/memory/shared_guard_queue.jet)
for the bounded-queue shape.

Inside `#Transact`, writes to `Shared` values are buffered and commit atomically.
The body runs exactly once. At commit, touched values are write-locked together
in stable-address order, buffered writes are applied, and the locks are released.
Contention waits; it does not retry the body. A `?` failure or early return
before commit drops buffered writes. Irreversible `Net`, `FS`, or `Exec` effects
inside the body are E0746; move them after the transaction or register them with
`on_commit` on a named transaction. A transaction name is optional; the named
form exposes `on_commit` and `on_rollback`. `name.on_rollback(() -> { … })`
hooks run in reverse registration order after a `?` failure or early return and
are dropped on a clean commit. Registering one takes control of undo for the
handled value, so that value is not automatically snapshotted (D-STM1,
D-CONC-STM1=A, D-TXN-ROLLBACK).

```jet
fn transfer(from: Shared<Account>, to: Shared<Account>, amount: Int) {
    #Transact {
        from.balance -= amount
        to.balance += amount
    }
}
```

`Cell<T>` is the local interior-mutation path. `Cell.new(value)` infers `T`.
`get`, `set`, `replace`, and `get_or_set` are value methods; `read` and `edit`
closure methods keep a dynamic loan inside one call. `get` and `get_or_set` copy
their result, so the result type must satisfy the copy law. `guard_read()` and
`guard_edit()` keep a loan across calls. Read guards may coexist; an edit guard
conflicts with every other guard and reports a `Cell borrow conflict` panic.
Dropping a guard releases its loan on normal return, early return, and panic
unwind. `map` projects one field and `split` creates two disjoint projected
edit guards. A cell guard can cross a direct helper or named tuple but cannot be
stored in a user struct, enum, list, fixed list, map, option, result, union, or
lambda. `Cell`, `CellReadGuard`, and `CellEditGuard` cannot cross a task,
channel, `Shared`, task-group, or parallel-adapter boundary; use `Shared<T>` for
synchronized cross-boundary state (D-LOCALCELL1=A).

`Pool<T>` is a generational arena and `ID<T>` is copyable index-plus-generation
data. The pool owns each `T`; an `ID<T>` never accesses `T` by itself. `add`
returns an ID, `pool[id]` indexes for read or write, and `ids()` walks live
entries. Removing an entry bumps its generation and returns `?T`. Indexing a
stale ID panics, like an array bounds failure, rather than silently reading old
storage.

```jet
struct Player {
    name: String
    hp: Int
    attack: Int
    target: ID<Player>?

fn run() {
    world := Pool<Player>.new()
    kai :: world.add(Player{name: "Kai", hp: 100, attack: 15, target: None})
    world[kai].hp += 1
    fallen :: world.remove(kai)
    _ :: fallen
}
```

See [`entity_world.jet`](../../Examples/features/memory/entity_world.jet) and
[`entity_tree.jet`](../../Examples/features/memory/entity_tree.jet) for pool
links and nested writes (D-POOLID-API1).

### Transitive memory facts

Memory floors are effect-row prohibitions, not `#Policy` settings. Sema checks
every reachable call, including dependency calls, against the function's
`!Mem.*` denial. The unbounded forms are `!Mem.Alloc` and `!Mem.Rc`; the bounded
form is `!Mem.Alloc(above: N)`. E0921 identifies the incompatible operation,
prints the full call path, and names the denial and its provenance. An open-world
dispatch needs a sealed target set or signed dependency summary; otherwise the
strict fact is unprovable and rejected (D-AUTHORITY-MEM1, D-MEM-FACTS1).

```jet
fn integrate(e: &Entity, dt: Float) -[!Mem.Alloc]> {
    e.pos += e.vel * dt
}
```

The human aliasing rule is: while something is being changed, nobody else may be
looking at it. Foreign `read` and `write` spellings are not Jet access syntax;
under D-S14-PAUSE they receive ordinary parse errors, apart from a separately
ratified narrow teaching diagnostic.

## Access sigils

Access markers are prefix sigils on the type, not on the binding name. The
unmarked type is the enforced read default; the only explicit access sigils are
`&` for exclusive write and `^` for take. They appear on parameters, and the
call site mirrors them when a caller supplies a write window or transfers
ownership.

| Written type | Meaning | Native lowering of a non-scalar parameter |
| --- | --- | --- |
| `T` | read; the callee cannot elevate it | shared borrow |
| `&T` | exclusive edit access | mutable borrow |
| `^T` | ownership moves to the callee | owned value |

```jet
struct Player {
    hp: Int
}

fn damage(player: &Player, amount: Int) {
    player.hp = player.hp - amount
}

fn consume(resource: ^String) { print(resource) }

fn run() {
    player := Player{hp: 10}
    damage(&player, 3)
    consume(^"resource")
}
```

An access marker composes with the optional prefix: `&?User` is write access to
an optional `User`, and `^?Texture` takes an optional `Texture`. More than one
access marker on a parameter is E0029:

```text
error[E0029]: two access markers on one parameter
  --> file.jet:3:12
   |
 3 | fn bad(p: &^Player) { … }
   |           ^^ remove one access marker
```

`~` is the copy sigil, not an access marker. Raw-pointer access (`p.*` or
prefix `*x`) is a separate `#Unsafe`-gated mechanism. It does not change the
parameter-access table (D-MEM1, D-SHAPE-COPY1=A, D-CAP9).

## Boundary crossings

Nothing foreign becomes a Jet value silently. Every crossing names its schema and
leaves its fact. A boundary feature must occupy one row and state its time,
schema, checker, evolution, and fact. A proposal that needs a new row or column
returns to design review instead of creating a second boundary mechanism
(D-BOUND-LAW1=A).

| Time | Schema named by | Checked by | Evolves by | Fact left |
| --- | --- | --- | --- | --- |
| comptime — literal | the checked text type from D-TEXTHEAD-TYPE1 | the type checker and E2712 | source edits | the typed source expression; no runtime origin is needed |
| build — manifest | the closed manifest vocabulary from D-CONF | manifest validation and registered errors | editions and explicit manifest changes | parsed manifest identity and its provenance |
| build — dependency | the lockfile entry and content hash | E1204 hash verification and the trust gate | re-resolution, lockfile update, or edition change | locked bytes, hash, and trust decision |
| link — foreign signature | the binder descriptor from D-FFI-UNIFY1 | the language binder and link checks | explicit rebind or descriptor change | the foreign effect leaf and link provenance |
| run — wire value | the target type with D-SERDE1 / D-ENC1 | typed decode and D-VALIDATE-DECODE1 `FieldError` values | named migration steps from D-MIGRATE1 and D-MIGRATE4 | the origin fact until successful typed decode |

The ratified instances use the same grid:

| Instance | Grid cell | Existing rule |
| --- | --- | --- |
| literal | comptime / typed literal | D-TEXTHEAD-TYPE1 and E2712 reject unchecked checked-text bodies |
| manifest | build / manifest | D-CONF names accepted fields and manifest diagnostics reject other shapes |
| dependency | build / dependency | E1204 binds resolved bytes to the lockfile hash; trust commands record the grant decision |
| link | link / foreign signature | D-FFI-UNIFY1 gives a foreign declaration one binder descriptor and one effect leaf |
| schema binder | link / foreign signature | D-BOUND-BIND1 turns a JSON, CSV, SQL DDL, XML, or proto schema into visible Jet source, hashed and stamped in its header |
| wire | run / wire value | D-SERDE1 and D-ENC1 use `DataTree` and one typed codec path |
| validation | run / wire value | D-VALIDATE1 accumulates `FieldError` values; D-VALIDATE-DECODE1 gives decode failures one shape |
| migration | run / wire value | D-MIGRATE1 and D-MIGRATE4 apply named schema operations transparently through typed decode |
| fact | all rows / fact | D-FACT-LAW1 and D-FACT-FLOW1 keep dataflow facts in the shared sema ledger |
| trust | build / dependency | D-JPK-GRANTCMD1 and D-JPK-GRANTSCHEMA1 make authority explicit, reviewable, and revocable |
| error | checker column of every row | registered E-codes, `FieldError` values, and typed foreign errors name what failed and its repair path |

The grid is a planning and proof obligation, not a runtime layer. Sema owns
checks and facts; the Prelude owns runtime boundary meaning. Rust emission,
Cranelift, and interpreter paths marshal the same typed result and fact
semantics. A boundary with a runtime path must preserve the same cell through
AOT, the default `jet run`, and interpreter execution.

## Effect system

This section defines effect inference, authority, and the checked tooling that
exposes those facts. It is for authors of functions and packages, and for tools
that consume checked programs. Executable truth lives in the [effect
table](../../crates/jet-codegen/src/Prelude/Effects.jet), [syntax
tables](../../crates/jet-foundation/src/Syntax/), [sema effect
checker](../../crates/jet-sema/src/Sema/Effects.rs), [inspect
projections](../../Source/CmdExpand.rs), and [expand
tests](../../tests/cli_parts/expand.rs).

Every function has an **effect set**: the ambient powers exercised by its body,
such as filesystem, network, clock, or process access. Jet infers the set,
propagates it through calls, and erases it before code generation. An empty set
is purity; effects are not runtime values, handlers, or monads
(D-EFF1, D-QUAL1, I3).

### Effect names and declarations

The thirteen grantable roots are `Net`, `FS`, `IO`, `DB`, `Time`, `Rand`,
`Env`, `Exec`, `Log`, `GPU`, `FFI`, `Browser`, and `Secret`. A path may add
user-chosen dotted leaves, for example `FS.Read` or `Net.HTTP.Get`. The root
must be one of those names; descendants are checked by tree ancestry, so a
bound on `FS` covers `FS.Read`. Foreign-language names such as `FFI.Go` and
`FFI.Py` are leaves beneath `FFI`; the parent covers the whole foreign-call
tree. The Prelude also registers common leaves such as `DB.Read`, `DB.Write`,
`FS.Read`, `FS.Write`, and `Time.Wait` (D-CASING1, D-EFFTREE1).

A package may declare a leaf at compile time:

```jet
effect Log.Audit
```

Declarations from the package, its dependencies, and the Prelude form one
package view. If a root has declared leaves, a dotted use under that root must
name one of the view's declared leaves. A bare root remains valid. A root with
no declared leaves remains open. The same rule applies to function effect
rows, `#FX`, and package effect budgets. Declarations have no runtime
representation. An unknown root is a language error, not a new effect.

`Panic` and `Mem` are deny-only rows. `Panic` can be named in a prohibition but
cannot be granted by an authority, positive effect ceiling, or package budget.
Memory events such as `Mem.Alloc` and `Mem.Rc` remain visible to diagnostics
and denial facts; they do not have to be listed in a positive effect ceiling.
`Mem.Alloc(above: N)` is the parameterized memory-denial spelling. An explicit
memory or panic denial still rejects a reachable operation (D-EFFTREE1,
D-PANICROOT1, D-AUTHORITY-MEM1).

Core operations contribute the following broad categories. More precise leaves
are retained when the operation supplies them.

| Root | Typical operations |
| --- | --- |
| `IO` | `print`, `eprint`, input, and terminal operations |
| `FS` | `core.files` and file watchers |
| `Net` | `core.net`, `core.http`, and port watchers |
| `Time` | ambient clock, zone, sleep, and timer operations |
| `Rand` | ambient random operations |
| `Env` | `core.sys` environment operations |
| `Exec` | argv, process, command, pipeline, and process watchers |
| `DB` | database operations; query and execute refine to `DB.Read` and `DB.Write` |
| `Log` | `core.log` operations |
| `GPU` | graphics and game operations |
| `FFI` | calls through a foreign-language boundary |
| `Browser` | browser or DOM provider operations |
| `Secret` | decrypted repository-secret reads |

A call to an opaque `extern rust` or C function contributes the maximal effect
set because the checker cannot inspect its body. This keeps inference sound
without attempting to read foreign code. Deterministic constructors such as a
seeded `Clock` or `RNG` carry no ambient effect; reading ambient time or
randomness still does.

For example, interpolated `print` in
`Examples/features/basics/first_hour_expert.jet` records both `IO` and the
`Mem.Alloc` needed for its fresh `String`; `core.process.argv()` records the
`Exec.Args` leaf, while starting a process records the `Exec` root. A Core
call with a precise leaf under its own root records only that leaf, so
`files.read` requires `FS.Read` and `files.write` requires `FS.Write`; the root
still covers both. The manifest-less authority supplies only the beginner
default and does not alter these sema facts.

### Application authority

A manifest-less program, and a `package.jet` that writes no
`authority.holds`, receive the beginner grant `IO`, `Mem.Alloc`, and
`Exec.Args`. This covers output, ordinary allocation, and argument reads. It
never covers the `Exec` root, which also starts processes. Written
`authority.holds` replace that default with exactly the written policy. A deny
wins over a grant, and the floor does not silently grant filesystem, network,
process, or other roots. An undecided or denied required effect stops before
host state changes with E1803, whose Why is one plain sentence naming what the
program does; the required, granted, denied, and policy-identity facts stay
separate in its structured detail for `--json` (D-AUTH-AMBIENT1). In a
terminal, a run with undecided authority first lists each one in plain words
and asks `Run it? [Y/n/always]`: Enter runs once, `n` stops with exit code 1,
and `always` writes the complete grant into the script's package block or
`package.jet` (D-SCRIPT-CONFIRM1). Without a terminal the run stops with
E1803 and its Fix names the exact `--allow` command. A program without
`package.jet` has no manifest to edit, so its E1803 names the complete
required row and carries the leading inline `package { … }` block
(D-ECO-INLINEPACKAGE1) that declares it; because the block replaces the
beginner default, the row lists every required effect, not only the
undecided ones. Rights widen only by a written word, so the edit is graded
`needs-review` and `jet fix --all` inserts it (D-RIGHTS-DIAG1).

Command-line authority uses one rights surface: `--allow=<RIGHTS>` and
`--deny=<RIGHTS>`, where the value is a comma-separated list of canonical roots
or leaves, such as `--allow=FS.Read,Time`. The spaced form is also accepted.
The CLI has no per-effect flag family: all authority uses this one rights
surface (D-RIGHTS-CLI1).

### Function and block ceilings

A function may omit an effect row; sema still infers its complete transitive
row. A declared row is an upper bound, not a claim that the function uses every
listed effect. The unannotated form uses `-> Type`; an effect ceiling is written
before the return type, as in:

```jet
fn load(path: String) -[FS]> String {
    core.files.read(path)
}
```

The body must use a subset of the declared set. An omitted effect is E0740 and
names the introducing call and the declared set. `-[]>` is an empty upper bound;
any effect in the body is rejected as a purity violation. Effect annotations
are erased, so an annotated and an unannotated function with the same body
produce the same runtime code.

`#FX(...) { ... }` applies the same idea to one block:

```jet
fn run() {
    #FX(FS, IO) {
        text :: core.files.read("x") ?? ""
        print(text)
    }
}
```

The region permits only the listed effects, including effects reached through a
call, or reports E0712. It is a ceiling, not a grant: the operations still
happen and contribute to the enclosing function's inferred row. `#FX` is a
lexical block and disappears in generated code.

### Higher-order and trait effects

A higher-order function's row includes its own body and the effects of function
values passed at each call. A lambda is walked inline. A directly named
function contributes its known row. A local, returned, stored, or otherwise
unknown function value contributes the maximal set, which is conservative and
sound.

```jet
fn apply(f: fn(Int) -> Int, x: Int) -> Int { f(x) }

fn run() -[IO]> Unit {
    apply(log_it, 1)
}
```

If `log_it` reaches `Net`, the call in `run` violates the `IO` ceiling. A
function-typed parameter can carry its own bound, such as `fn(Int) -[Net]> Int`;
passing a callback outside that bound is E0747. `-[via f]>` publishes a tight
pass-through for a named function-valued parameter, including when the value
escapes.

A trait method may declare an upper bound:

```jet
trait Shape {
    fn area(self) -[]> Int
}

impl Square.Shape {
    fn area(self) -[]> Int { self.side * self.side }
}
```

Every implementation must fit the bound or E0742 is reported. A dynamic call
uses the trait's declared bound because its concrete implementation is unknown
at the call site. An unannotated method is inferred per implementation under
static dispatch; a dynamic-dispatch call that needs a bound receives the
annotate-the-method diagnostic (D-EFF2, D-EFF3).

### Opaque cryptographic values

`core.crypto` exposes opaque, move-only `Secret`, `SigningKey`,
`X25519SecretKey`, and `SharedSecret` values. They cannot be compared with
ordinary equality, cloned, hashed, printed, reflected, or serialized. Constant-
time operations are the comparison API. Raw bytes leave an opaque value only
through explicitly named expert exposure functions.

The expert surface names `xchacha20poly1305_seal/open`,
`aes256gcm_seal/open`, `ed25519_sign`, `ed25519_verify_strict`, `x25519_raw`,
`hkdf_sha256_raw`, `argon2id`, `secret_bytes`, `signing_key_bytes`,
`x25519_secret_bytes`, and `shared_secret_bytes`. Every expert call is
lexically inside `#Unsafe("reason")`; importing the module does not open that
gate. AEAD authentication failures use `CryptoError.OpenFailed`; X25519 rejects
an all-zero shared secret. HKDF output is at most 8160 bytes. Argon2id accepts
8192–262144 KiB, 1–10 iterations, 1–8 lanes, `memory >= 8 * lanes`, and
`memory * iterations <= 1048576`; salts are 8–64 bytes and output is 16–64
bytes. Invalid expert parameters use the `CryptoError` family.

`crypto.file_seal(recipients, source, destination)` and
`crypto.file_open(identity, source, destination)` use the recipient-based JETC
v2 envelope. The fixed prefix is `JETC`, version 2, kind 1, suite 1, flags 0,
followed by little-endian header and body lengths. The authenticated header
contains a 16-byte file id, an ephemeral X25519 public key, a 16-byte nonce
prefix, a fixed 1 MiB chunk size, 1–256 canonical recipient stanzas, no
metadata, and its tag. Body records contain little-endian length, final flag,
ciphertext, and tag. Non-final records are exactly 1 MiB, and an exact multiple
has one empty final record. Readers bound declared sizes before allocation and
accept only safe-open v2.

Sealing snapshots and revalidates a no-follow regular source before requesting
randomness. Seal and open stream one authenticated chunk at a time, poll
cancellation between chunks, zeroize secret and plaintext buffers on every
exit, and publish with atomic no-overwrite semantics only after authentication
and durable staging. Safe-open identity, framing, recipient, and authentication
failures collapse to `FileCryptoError.OpenFailed`; no partial destination is
published. A target without the required native bridge fails closed rather than
claiming filesystem JETC support (D-CRYPTO-API1, D-CRYPTO-ENVELOPE2).

### HTTPS and graphics

`core.net.fetch` and `core.http.client` support `https://` through the rustls
bridge and system certificate roots. Plain `http://` remains available for
loopback and existing endpoints. Handshake, trust, and missing-root failures
are E4201, E4202, and E4203. Advanced client configuration belongs in
`core.net.tls`. HTTPS serving is an explicit labeled argument:
`Server.serve(addr, mux, tls: Server.tls(cert, key))`; an unlabeled TLS value is
rejected so the transport choice is visible at the call site (D-TLS1,
D-TLSSERVE1).

`core.game.raylib` provides the typed window, drawing, input, sound, and texture
operations. Its default path is headless; `JET_RAYLIB_DISPLAY=1` enables the
native display bridge, and a missing raylib library uses the headless path.
`core.game` is scene-first: `game.Scene.new`, `scene.assets.image` and
`sound`, `scene.input.bind`, `scene.component<T>()`, `scene.query<T...>()`,
`game.Replay.record`, `game.Backend.headless`, and `game.run` provide
deterministic headless replay with a frame hook. Effects and authority still
apply to the bridge (D-RAYLIB1, D-GAME1-3).

## Expert memory tier

The low-level tier keeps ordinary Jet free of raw pointer syntax. `use core.mem`
is the discovery gate for pointer types and operations; `#Unsafe("reason")`
is the audit gate. A low-level operation outside the corresponding gate is a
compile error, not an implicit capability escalation. (D-LL1, D-UNSAFE2)

### Pointer operations and lifetime sentries

`*x` takes a raw pointer to `x`, and `p.*` dereferences it. These operations
require `use core.mem` and an audited region. `mem.address_of(x)` produces an
inert address value and can be named outside the audit gate, but using that
address as a pointer still requires the gate. `mem.volatile_read(p)` and
`mem.volatile_write(p, value)` provide explicit typed volatile/MMIO access.

An address into a current stack frame has a runtime sentry whose registration
expires when that Jet frame ends. Heap, static, and foreign storage retain their
own ownership rules. Code generation lowers an audited region to the native
unsafe block; it does not invent a safety proof. E3101 reports a low-level
operation outside `#Unsafe`, E3102 reports a missing `core.mem` discovery
import, E3103 reports an unsafe function call without an enclosing gate, and
E3112 requires a nonempty reason. The audited example is
[`Examples/features/lowlevel/lowlevel.jet`](../../Examples/features/lowlevel/lowlevel.jet).

A function marker `#Unsafe("reason") fn` makes the entire function body an
audited contract. Its caller must itself be inside an enclosing unsafe region.
The reason belongs to `#Unsafe`; there is no separate audit marker in the
contract. (D-UNSAFE-REASON1=A)

### Unsafe obligations

The obligation policy adds evidence without weakening either gate. With no
policy or `.GateOnly`, the mandatory reason and gate rules remain. An
`.Obligations` policy requires operation-specific typed assertions immediately
after each low-level operation using the closed facts `valid_ptr`, `aligned`,
and `no_alias`, for example `assert valid_ptr, aligned`. `.PerSite` additionally
requires every gate to select `obligations: .Track` or `.Skip`; an organization
floor may reject `.Skip`.

CI or an administrator supplies the floor through
`JET_ORG_UNSAFE_POLICY=<path>`. Its package-policy shape is
`policy: .{ unsafe: .Obligations, impure: .GateOnly, nondeterministic: .GateOnly }`.
The path is retained as provenance, and an unreadable or malformed configured
file fails closed. `jet inspect gates --kind unsafe FILE` reports each gate,
operation, discharge state, and effective policy. Assertions erase in sema
before the shared AOT
or development TIR boundary. See
[`Examples/features/lowlevel/unsafe_obligations.jet`](../../Examples/features/lowlevel/unsafe_obligations.jet). (D-UNSAFE-OBLIG1)

### Explicit pointer casts

Jet has no compact cast-and-dereference operator. To reinterpret an address,
first construct `mem.Ptr<T>.from_addr(addr)`, then use postfix `p.*` in the same
audited region. The two operations make the cast and dereference obligations
separate. The Jai comparison is illustrated by
[`Examples/features/lowlevel/pointer_cast_deref.jet`](../../Examples/features/lowlevel/pointer_cast_deref.jet). (D-POINTERCHAIN1)

### Allocator families

`core.mem` exposes `Arena`, `Bump`, `Pool`, and `Fixed`; constructing and using
these allocators does not require `#Unsafe`. `Arena.new()` or
`Arena.new(capacity: N)` grows aligned heterogeneous chunks; `Bump` places
monotonically in one caller-capacity buffer; `Pool` owns a caller-bounded slot
count and reuses compatible size/alignment classes after reset. Values are
dropped in reverse allocation order before storage is reused. Allocator handles
are thread-confined, so move plain owned data across a task or channel rather
than moving the allocator itself.

`arena.reset()` retains backing storage for reuse. Terminal release uses the
universal resource operation `close(^allocator)`; `.free()` is not the
allocator release spelling. Using an allocator after its move is E0121. The
walkthrough is [`Examples/features/memory/arena.jet`](../../Examples/features/memory/arena.jet). (D-ALLOC1, D-ALLOC-C, D-ALLOC-D)

The fallible allocation family is separate from plain abort-on-failure calls:
`List.try_new`, `List.try_with_capacity`, `try_push`, `try_reserve`,
`Map.try_insert`, `String.try_push`, and each allocator's `try_alloc` return
`AllocError { requested_bytes, allocator }`. A plain `new`, `alloc`, or
mutation retains its abort-on-failure behavior. The AOT emitter, Cranelift
host, and interpreter carry the same fallible result; they do not choose a
different allocation policy. See
[`Examples/features/memory/try_allocation.jet`](../../Examples/features/memory/try_allocation.jet). (D-ALLOCFAIL1=A)

### Arena regions and scope-bound views

`arena.alloc(value)` places a value in retained arena storage and returns a
scope-bound view, not an owned copy. `reset()` mutably borrows the arena and
`close(^arena)` consumes it, so the compiler rejects either operation while a
view is live. The runtime has one narrowly contained lifetime-extension helper
inside the memory implementation; that lifetime never appears in user syntax.

A view must stay inside its region and before the arena is reset or closed.
E0631 rejects a view returned, stored in another owner, passed to a `&` or `^`
parameter, or captured by an escaping closure. E0632 rejects a read after the
arena reset. Regions are implicit and scope-inferred from the lexical scope of
the arena binding. Expert code may use `#Region(r) { ... }` for a named or
narrower region, or one spanning more than one allocator; the same escape rule
applies. Views are non-reassignable, non-escaping locals, and analysis that
cannot prove those properties is rejected. The example and diagnostics are
[`Examples/features/memory/arena_regions.jet`](../../Examples/features/memory/arena_regions.jet),
[`tests/ui/arena_view_escape.stderr`](../../tests/ui/arena_view_escape.stderr),
and [`tests/ui/arena_view_after_reset.stderr`](../../tests/ui/arena_view_after_reset.stderr). (D-ALLOC2, D-REGION1)

## Imports and visibility

### Importing files, modules, and packages

Jet has two import forms: a quoted path names a file, and an unquoted name
names a module.

```jet
use "scoring";
use util as text;
use core.files;
```

A quoted path is relative to the directory of the file containing the `use`.
The `.jet` suffix is implicit. A path cannot escape its project with `..`.
The last path segment supplies the default namespace; `as` gives either form
an explicit alias (S16).

An unquoted module name is resolved by recursively searching from the project
root for `name.jet` and for `name/run.jet`; `core` is a compiler-provided
namespace (S51).
A realized `library` package is another module-search root, so its public
items are used with the same `use package;` and `package.item` syntax. The
package must already be realized: compilation does not realize dependencies on
demand. An `executable` package belongs on `PATH`, not in `use`; naming one in
an import is **E0982**. A declared library that has not been realized is
**E0983**. If a package omits `kind`, its staged `bin/` output or top-level
`run` function makes it an executable; otherwise it is a library. An explicit
kind takes precedence. A single-file `jet run` or `jet build` still requires
an executable entry point and reports **E0101** when it has no `run` function
(U10, U17, D-LIB-USE, D-ILE1).

A dependency package is checked as its own package. An importing check never
repeats a dependency's reports; when the dependency has errors, the importer
reports one **E0626** naming the dependency and the `jet check` that owns
them. A path dependency declared by a dependency must stay below that
dependency's directory (**E1206**) unless the root package declares the same
directory itself, which lets sibling packages such as `Compiler/JetLexer` and
`Compiler/JetFoundation` share a dependency (D-MOD-CYCLE1). The UI fixture is
`tests/ui/dependency_package_errors/`.

A package is one namespace (D-MOD-CYCLE1=A). A package is a directory with a
`package.jet`, holding every `.jet` file below it except the files of nested
packages, or a single file that begins with a `package { }` header. Its
files see each other's top-level names, private ones included, without any
import; a file of a package that imports another file by path is **E0623**,
and a top-level name declared in two files of one package is **E0624**.
Only loose files, which have no header and no `package.jet` above them,
import files by relative path. Files of one package may refer to each other
in a cycle; packages themselves are acyclic, and one package uses another
with `use package.[names]`, which reaches the dependency's `pub` names in
any of its files. An executable package has one `fn run`, except that each
file an `outputs:` entry names keeps its own `fn run` (owner ruling B); a
library package has none (**E0625**). An `outputs:` entry `file.callable`
names the member file whose stem is `file`, at any depth, and a callable it
declares. The UI fixtures are `tests/ui/package_file_import/`,
`package_duplicate_name/`, and `library_package_run/`.

Cross-file access is qualified:

```jet
score :: scoring.score(91);
```

Only `pub` declarations and `pub` struct fields cross a file boundary. A
file-wide `#PubFile` marker changes the default for that file: top-level
items are public unless they carry `priv` (S18, D-VISDEFAULT1=C,
D-VISDEFAULT2=A).

```jet
#PubFile

fn greet() -> String {
    "hello"
}

priv fn secret() -> Int {
    42
}
```

The import graph is loaded before semantic checking, so checks cover the whole
program. The relevant diagnostics are **E0602** (path escapes the project),
**E0603** (missing import), **E0604** (import cycle), **E0605** (private item),
and **E0606** (ambiguous module). The executable import example is
`Examples/features/modules/imports/`; the corresponding UI fixtures are under
`tests/ui/import_escape/`, `import_missing/`, `import_cycle/`,
`import_private/`, `import_private_field/`, and `import_ambiguous/`.

### Code modules

Code modules use `module` where Rust uses `mod`, and `.` where Rust uses `::`
(D-MOD1). The path-import form above remains the short form for importing one
file.

```jet
module math {
    pub fn clamp(value: Int, low: Int, high: Int) -> Int {
        if value < low -> low
        else if value > high -> high
        else -> value
    }
}

fn run() {
    math.clamp(3, 0, 2)
}
```

`module math;` declares a file module. The loader searches beside the file for
`math.jet`, then for `math/module.jet`. Neither file is **E0607**; finding both
is **E0606**. `module math { ... }` is inline and adds the `math` namespace to
the containing file; it performs no file lookup. The same `module` keyword is
used by JetOS declarations. The semicolon form always declares a code module,
and the parser distinguishes braced code and JetOS contributions from their
contents.

Qualified access always works. `use math.clamp;` imports one member, and
`use math.[clamp, lerp];` imports a member group. The `.[ ]` member-list form
has the same meaning after `use` and in an expression such as
`point.[x, y]`; use entries may also have aliases and dotted paths. Wildcard
imports are rejected with **E0612**. An unqualified import of an undefined
member is **E0611**, and importing an item from a module that is not in scope is
**E0610** (D-MOD2).

Declarations are private by default. `pub` exports to every consumer;
`pub(package)` exports only within the same payload or workspace package and
not to downstream packages. Access to a private inline item reports **E0609**;
access across files reports **E0605**. An unknown `pub(...)` qualifier is
**E0411**. Inline-module bodies are type-checked in their defining scope, so a
private sibling can call another sibling without exporting it
(D-MOD3, D-PUBPKG1).

A member import names any public top-level declaration: a function, type,
enum, trait, tag, protocol, or constant. A constant is exported the same way
as any other item, `pub MAX_ITEMS :: 8`, and is read qualified
(`limits.MAX_ITEMS`) or by name after `use limits.[MAX_ITEMS]` (3A). A `use`
inside an inline-module body follows the same rule.

An `impl` head that names a declaration through a module alias binds the alias
to the segment after it: `impl types.Span { ... }` extends `types.Span`, and
`impl Local.types.Named { ... }` implements `types.Named` for `Local`
(D-IMPLDOT1).

A directory module exposes a child only through an explicit Rust-style
re-export:

```jet
pub use wrap.wrap;
```

A public declaration that is not re-exported does not become part of the
parent directory's surface. Re-exported calls retain the defining function's
borrow and move rules. Examples covering file modules, inline modules,
qualified and grouped imports, and re-exports are in
`Examples/features/modules/` (D-MOD4).

### Generic modules

A generic module is a module template with type parameters and compile-time
value parameters. Applying it creates a specialized ordinary module
(D-GENMOD1, D-GENMOD2, D-CONF-GENSPELL1, D-GENMOD-VALUE1, D-GENMOD-BODY1,
D-GENMOD-IDENTITY1).

```jet
module cache<K: Hash>(capacity: Int) {
    pub fn key_of(k: K) -> String {
        "key"
    }
}

module int_cache :: cache<Int>(64)
```

Type parameters and bounds use the angle list (`<K>` or `<K: Hash>`). Value
parameters use the typed parenthesis list (`(capacity: Int)`); the two lists
are separate. A value argument must be an immutable Tier-0 comptime `Bool`,
`Int`, `Char`, `String`, or fieldless-enum value. The compiler closes and
normalizes each argument before specialization. Registered build facts may be
leaves in such an expression and use the same fuel-limited evaluator as other
Tier-0 values. An `Int` value parameter may also supply the generic-module
fixed-list layout form `[T#capacity]`. A value type mismatch is **E0853**.

The body has ordinary-module parity: it may contain functions, structs, enums,
tags, module-global values, traits and impls, tests and benches, nested modules,
generic modules, aliases, and other legal markers. Definition-site lexical
scope and all ordinary visibility rules remain in force.

An instance is identified by the resolved template definition and normalized
arguments. Repeating the same application shares nominal member types and one
specialization; different arguments or a different template definition produce
different instances. Semantic checks reject invalid targets, arity, bounds,
value kinds and types, scope, and cycles with **E0850**–**E0853** and
**E0855**–**E0857**; a specialization identity collision is **E0859**. The
value-argument example is `Examples/features/modules/fact_value_arguments.jet`.

## Composable configuration modules

A `module` declaration can contribute typed values to reserved configuration
namespaces. Several modules may share a file (U3).

```ebnf
module       = "module" dashed-name "{" contribution* "}" ;
contribution = namespace "." dashed-name ":" expr [","] ;
namespace    = "env" | "image" ;
dashed-name  = ident { "-" ident } ;
```

Package, module, image, and environment names may use kebab case, for example
`module web-app`, `env.web-tools`, and `image.halcyon-oci`. A hyphen joins
segments only when it is adjacent to both; `a - b` remains subtraction. Code
identifiers—variables, fields, types, and functions—remain plain identifiers.
Leading, trailing, and doubled hyphens are invalid (S84).

Automatic discovery skips a declaration whose declared name begins with `_`:
`module _name { ... }`. An explicit `use project._name` is still allowed under
ordinary visibility rules. The name, not the filename or scan order, controls
discovery; `jet project parts --skipped` lists omitted declarations. Duplicate
declared names conflict even when one is omitted from discovery
(D-SHAPE-MODULEINTERNAL1=A).

The reserved role namespaces are `env` (an `Env` development environment),
`system` (a `System` JetOS host), and `image` (an OCI image or JetOS installer
input). An `env.<name>:` contribution uses the ordinary expression parser,
usually an `Env{ ... }` struct literal. A string prompt is shorthand; the
structured `Prompt` value selects label/path and stripping modes. The shell
hook command is `jet env hook <shell>` for `bash`, `zsh`, or `fish`; the hook
is opt-in, restores the prior shell outside an environment tree, and honors a
non-empty `JET_ENV_DISABLE`. Activation of a trust-sensitive environment uses
the normal trust gate (D-ENVHOOK1=A).

### Images and package adapters

An `image.<name>:` contribution describes a Jetpack OCI image. `from: env.<name>`
projects a typed environment into the image. Image records may carry `services`,
`target`, `user`, `entrypoint`, `health`, `expose`, `env_vars`, `files`, and
`base`. `files` explicitly layers project-relative non-secret paths; managed
environment files and dotenv inputs are omitted. `.Iso`, `.Qcow`, and `.Raw`,
and `from: system.<name>`, are JetOS installer inputs handled by `jet os image`.

An environment package list may contain `Pkg.adapt(name:, source:, deps:,
recipe:)`. Dependencies are realized first, and only their verified executable
members are available to a `Recipe.build` `.exec` step. `Recipe.copy()`,
`Recipe.prebuilt(bin:, as:)`, and finite `Recipe.build` steps (`.fetch`,
`.exec`, `.install`, and `.install_tree`) produce ordinary hangar packages
through the same store and lock path as other packages. `jetpack add <ref>
--adapt` writes a draft adapter and does not execute upstream code (U20).

Direct providers verify their source and closure before projection. For example,
the LuaRocks reference has the exact `<name>#version=<version>@luarocks` shape;
source SHA-256 values, dependency closure, and the qualified reference are
recorded in `.jet/lock`. Mutable refs, unsupported native-build metadata,
cycles, unsafe archive paths, source drift, and cache tampering fail closed.
Offline reuse verifies sealed Hangar output without contacting the repository
(D-JPK-PROVIDERS2).

Core and adapted packages can be realized without an installed Nix executable.
A reference without a pinned compatibility output reports **E1272**; foreign
flake projection uses the bounded native evaluator and reports **E1256** for
unsupported expressions. Package search and information commands read local
`.jet/discovery/index.jsonl`, provider fixtures, and Hangar metadata only;
they do not fetch. `jet env info [--env <name>] [--preset <name>] [--json]` is a
read-only report of one selected environment plan: package references, typed
service facts, jobs and checks, variables with source labels, managed files,
and integrations. It does not realize, start, or apply anything (U23, U26/#789).

The selected environment facts are also available through the existing LSP
resource bridge as `jet://environment` via `resources/list` and
`resources/read`; the bridge remains read-only and does not enter the lifecycle.

### Workspace overlays

`workspace.jet` may carry reviewed overlay policy inside `module workspace`.
An `overlay <name> { ... }` records provider and channel swaps, package-local
source/version/flag/patch changes, and `allowUnfree` decisions. For example,
`Provider.nixpkgs(channel: "plasma-beta")` selects a provider channel and
`package("foo").patches += [patch("patches/foo.patch")]` records a deterministic
source patch. Workspace-wide unfree review uses `policy.allowUnfree`.
`jetpack override draft <ref> --patch <file>` writes source policy only; it
does not create hidden state. Patch application is deterministic unified-diff
application. `jetpack explain package-overlay:<overlay>:<package>` reports the
provider, channel, policy fingerprint, and update command from that source
policy (D-JPK-OVERLAY1).

### Explanations, provenance, and offline operation

Recipe failures retain per-step logs and scratch in Hangar-managed storage.
`jet logs <pkg>` and `--shell-on-fail` identify the failure surface.
`jet explain <ref>` reports store identity, provider facts, dependency and
closure edges, liveness roots, and rebuild checks. The causal forms
`jet explain why-depends|what-depends|closure|why-live|rebuild <ref>` select
one view, and `jet explain <ref> --json` emits the stable `jet.report/v1`
projection. `jet inspect provenance` reads the one lock-backed provenance
record for each dependency and shows integrity, transparency, publisher, and
build-attestation facts; present evidence is labeled `verified` or `recorded`
(U27).

Mutation output plans before applying. `-y` and `--yes` are equivalent; a
non-interactive mutation without either prints the plan and makes no change.
The plan rows use `+`, `-`, and `~` in colored and plain output (D-FE-CLI1).
One-shot Jetpack commands are user-owned and coordinate with file locks; they
do not require a resident daemon, a root-owned Hangar, or a privileged helper.
Linux recipe execution uses the Bubblewrap boundary and refuses an executable
action with **L0205** when that backend is unavailable; requiring the backend
through configuration reports **E1275** before launch (U28).

`jet trust list`, `jet trust explain [<grant>]`, `jet trust grant <grant>
[--scope user|repo]`, and `jet trust revoke <grant>` operate on package, build,
environment, service, image, fleet, and JetOS authority grants. Trust decisions
are `allow`, `prompt`, or `deny`; provenance requirements are `none`, `logged`,
or `attested`. The grant store accepts the existing `hash:` and `pattern:`
records (U19). A `package.jet` authority policy may set `authority.trust`
defaults and per-profile, service, provider, and `require` decisions. An absent
`require` means `none`; unknown authority fields are manifest errors
(D-JPK-GRANTCMD1/SCHEMA1).
Lock-backed provenance reports integrity, transparency, publisher, and
build-attestation facts; present evidence is labeled `verified` or `recorded`,
and integrity failures remain **E1204**. Lints warn by default, while
`package.jet` policy can deny named lints and produce **E1293**. Host policy
can narrow but not widen that rule
(I1, I8, D-BOUND-PROV1, D-LINTPOLICY1=A, D-JPK-POLICYSURFACE1).

Once a package is realized, realize-class operations can reuse it with
`--offline`. Network-class operations reject `--offline`; a missing local
object reports **E1276** rather than fetching (U29).

### Services, secrets, and vault keys

An environment may declare project-local development services:

```jet
services: {
    database: { enable: true }
}
```

They are managed with `jetpack services up`, `down`, `health`, `logs`, and
`wait`. They are not system services and do not activate JetOS
(D-JPK-SERVICE1).

A `secrets` map contains secret metadata or a read-time composition. The
compiler checks declaration types, duplicate names, environment labels,
policy/generator shapes, placeholders, input names, and composition cycles. A
known profile checks allowed environments; a dynamic profile defers that check
to activation. A missing required entry reports **E1263** with its name and
environment. Compositions resolve on each `core.crypto.vault.get` read and
derive the result in memory. Plans, diagnostics, logs, fixtures, snapshots,
and audit events contain names and sorted input names, never secret values
(D-JPK-SECRETMETA1=B, D-JPK-SECRETCOMPOSE1=D).

`core.crypto.vault` persists `SigningKey` and `X25519SecretKey` behind immutable
`KeyRef<T>` handles. Reads, preparation, authorization, and commits require
`Secret`. Mutation is a compare-and-swap sequence: prepare a five-minute,
move-only `MutationPlan<T>`, authorize its exact native preview into a one-use
`VaultWrite<T>`, then consume the write and plan in order. Rotation creates a
new generation and retires the previous active generation; an exact retired
reference still loads, while a revoked reference fails before key bytes are
copied (D-CRYPTO-VAULT1=A).

String secrets and typed keys share the age-encrypted `.jet/secrets.age`
artifact but use separate namespaces. Its authenticated plaintext is the
bounded `JVLT` version-2 format. The canonical `JVKW` version-1 envelope is the
portable backup format: recipient mode wraps an independent backup key for
1–16 sorted X25519 recipients, while passphrase mode uses fixed Argon2id
parameters. Both modes authenticate source repository, name, generation, record
hash, and concrete key type. Import decrypts into a short-lived
`WrappedImportPlan<T>` and then reuses native authorization and compare-and-swap
commit; same-origin imports are idempotent and revoked origins cannot reactivate.
Secret-dependent open failures collapse to `KeyWrapError.OpenFailed`. Raw
32-byte import is available only in an audited `#Unsafe` region. Headless
mutation requires `jet trust grant vault.write:<repository_uuid>`; source,
workspace, environment, DAP, and stdin are not write authority.

## Concurrency

`task` is the reserved concurrency word. `core.tasks` supplies task control and
timer helpers, and typed channels use `channel<T>()`.

### Tasks, ownership, and crossing boundaries

A task starts zero-parameter work captured from its surrounding scope:

```jet
task work()
task { work() }
```

Copyable captures are copied at closure creation; owned non-copyable captures
move. A task-boundary value must be sendable (**E1102**). Mutable or otherwise
non-sendable captures report **E1101** or **E1102**. A `view` borrow, a struct
containing `ref` fields, a trait value, or a closure with non-sendable captures
cannot cross. Give ownership to a child with `^`, make an immutable owned
snapshot with `freeze`, or use `Shared` and a lock for deliberate shared
mutation (D-CONC-CROSS1).

### Frozen snapshots and consuming captures

`freeze(x)` creates one deeply immutable, owned snapshot. A task may read it
after the lexical group ends because no task can write through it. Writing the
root, a field, or an index is **E1113**. Freezing a frozen value is the
identity; freezing a value containing a lock-backed `Shared`, a resource, or
another non-clonable value is **E1114**. Other crossing failures remain
**E1102** (D-CONC-FREEZE1=A).

`task ^name { ... }` consumes `name` into the child. Using `name` after child
creation is the ordinary **E0121** use-after-move. This `^` capture spelling is
specific to tasks; `^` on an ordinary call argument retains its move meaning.
`Shared` and `Cell` retain their synchronized and local-only semantics.

The example and golden output are `Examples/features/concurrency/freeze_capture.jet`
and `freeze_capture.out`; UI fixtures are
`tests/ui/frozen_write.jet`, `frozen_capture_use_after_move.jet`, and
`freeze_shared_source.jet`.

### Task completion and cancellation

`handle.join()` consumes a task handle and returns `T !TaskFailure`. Calling
`join()` twice is **E0121**. Dropping a bound `Task` without joining, using its
result, or detaching it is the compile error **L1101**. A failure is `.Panicked(reason)`,
`.Cancelled`, or `.DeadlineBlown`.

`handle.detach()` consumes the handle and lets the task run in the background;
it suppresses **L1101**. Detach is sound only when the child owns all data it
can use. A detached child that captures or returns a `view` borrow reports
**E1106**; another sendability failure reports **E1103** (D-DETACH1).

Task handles expose `pause()`, `resume()`, and `cancel()`. `tasks.yield_now()`
cooperatively yields at a wait point, `tasks.current_task()` returns the
running task's control trace (and an idle value outside a task), and
`sender.close()`/`receiver.close()` close channel ends. Pause holds a task at
its next wait point until `resume()`; it is scheduler state, not a flag the
application must poll. Task failure uses `TaskFailure` (D-COROUTINE1).

Cancellation is preemptive at wait points: channel send and receive,
`time.sleep`, task join, a select arm, and I/O. A cancelled task unwinds at the
next such point and runs Drop-backed cleanup. A cancelled receive unwinds
instead of returning `Closed`; a race loser stops at its next wait point; and
a cancelled `task.all` member reports `Cancelled`.

`#Shield { ... }` defers cancellation or a blown deadline until the lexical
critical section exits. It takes no arguments, nests, and always restores its
state through an RAII guard on return, error propagation, or unwinding. An
expired deadline is delivered before a pending cancellation at the outermost
normal exit. Outside a task it is a transparent block; at comptime it has no
scheduler effect. Cleanup reached during an unwind completes its wait normally
rather than starting a second unwind; a failure already propagating is not
replaced. Generator completion notification still runs while its producer is
cancelled (D-CANCELMODEL1=C, D-SHIELDNAME1=A).

### Channels

`channel<T>()` returns a linked sender and receiver and is normally destructured
at the call site:

```jet
(tx, rx) :: channel<Int>()
tx.send(7)
value :: rx.receive() ?? panic("channel closed")
```

A second sender is made with `~tx`; there is no combined channel value from
which to fetch one. `send` moves its value into the channel, and
`receive() -> T !Closed` waits until a value arrives or all senders close.
Channel payloads must be sendable (**E1102**). `channel<T>(capacity: N)` adds a
bounded buffer. Seed a channel with `N` tokens when at most `N` workers may be
active; `Examples/features/concurrency/bounded_workers.jet` demonstrates the
pattern (D-CONC-CHAN1).

### Bounded buffering law

Jet bounded buffers preserve accepted values and apply backpressure by default.
Only `AsyncEvent` may discard a payload, and only when its immutable
`AsyncPolicy` explicitly chooses `DropNewest` or `DropOldest`. `Block` waits
and preserves the payload. A channel is typed work transfer; its capacity
bounds queued memory and producer pressure. An `AsyncEvent` is an asynchronous
many-subscriber occurrence stream, so its explicit pressure choice is visible
in its dispatch report. No other primitive gains an overflow option (D-EVENT2,
D-TASKRUNTIME1).

Both queue APIs call the numeric bound `capacity`:
`channel<T>(capacity: N)` and `AsyncPolicy{ capacity: N, overflow: ... }`.
Channel capacity supplies backpressure only; channels have no drop policy.

| Primitive | Full behavior | Buffering law |
| --- | --- | --- |
| `channel<T>(capacity: N)` | `send` waits for receiver space; a deadline or cancellation wakes the wait. | Preserve work-queue values in FIFO order; capacity bounds queued memory and producer pressure. |
| `AsyncEvent<T, E>` | `Block` waits; `DropNewest` drops the new attempt; `DropOldest` drops the oldest queued attempt. | The only explicit loss path; the report exposes acceptance and terminal state. |
| A service worker mailbox | Full delivery waits under a deadline or returns `Full`. | At-most-once, per-sender FIFO; no silent drop. |
| Buffered file handles | Read and write calls block or flush; there is no Jet queue or drop policy. | Bounded-memory stream; caller pace controls progress. |
| Terminal streams and HTTP `Body` | OS or socket backpressure controls blocking reads and writes; body limits reject over-limit input. | Accepted transport bytes are preserved; overflow is not dropped. |
| Encoding readers/writers and `DataStream` | `next`, `write`, and `flush` block; `EncodingLimits` and `DataLimits` bound retained work. | A bounded pull/push stream, not lossy delivery. |
| Log sinks | No public bounded queue, capacity, or overflow policy; writes and `flush` are explicit. | Sampling and disabling are explicit emission controls, not silent buffer loss. |

`Queue`, `PriorityQueue`, `Cache`, and `Bytes` capacity fields describe storage,
not concurrent producer/consumer buffering. Host-internal queues for browser
events, HTTP admission, observation, and tooling are implementation limits, not
Jet primitives. A future lossy log sink requires an explicit owner decision.

### Deadlines and teaching diagnostics

`#Context(deadline: <Int epoch_ms>) { ... }` supplies an ambient deadline.
Wait and I/O points inherit it, including task joins, channel receive,
`time.sleep`, and network reads and writes. Exceeding the budget emits **E3003**
and exits with the runtime error code (D-DEADLINE1).

The teaching diagnostic **E0040** directs `async`/`await` users to `task`;
**E0041** directs `Mutex`/`lock` users to channels.

### Parallel collection adapters

Lists expose `para_map`, `para_filter`, `para_partition`, and `para_fold`.
The older `par_*` spellings are not aliases. Map and filter preserve source
order. `para_partition` returns `(false_: [T], true_: [T])` with source order
within each side.

All four use one bounded indexed chunk engine. Chunk boundaries are stable;
worker count does not exceed available host parallelism; scheduling cannot
change result order. `para_fold(seed_factory, step, merge)` creates a fresh
accumulator per chunk, steps each chunk in source order, and combines partial
results with a deterministic adjacent-pair tree. Empty input calls the seed
factory once. The seed must be an identity for `merge`, and `merge` must be
associative; without those laws the tree remains deterministic but is not a
portable parallel reduction. A one-chunk plan runs on the caller thread.

If multiple callbacks fail, each chunk stops at its first failure, all started
chunks are joined, and the operation reports the failure at the lowest source
index, independent of completion order. It returns no partial collection or
fold accumulator. Effects already performed outside the returned collection
are not rolled back. Ordinary mutable captures, hidden callback capture facts,
non-sendable values, and function-typed worker values are rejected with
**E1101** or **E1102** before code generation; there is no implicit
serialization or capture merge (D-PARCAPTURE1=D).

### Structured tasks and combinators

`all`, `race`, `any`, and `group` become concurrency combinators only after
`task.`. Each branch starts one child
(D-CONC-SPAWN1=D, D-CONC-FAIL1=A, D-CONC-JOIN1).

```jet
one :: task work()
two :: task { work_again() }
results :: (task.all { work_a(), work_b() }) ?? []
winner :: (task.race { fast(), slow() }) ?? 0
first :: (task.any { read_cache(), read_network() }) ?? fallback
```

`task.all` waits for every branch and fail-fast cancels remaining children.
`task.race` returns the first successful result and cancels losers.
`task.any` returns the first completed result and cancels the remaining
children. The combinators consume their children and have no handle-list twin.

`task.all` may name all branches:

```jet
results :: (task.all {
    text: load_text(),
    count: count_items()
}) ?? panic("task.all failed")
print(results.text, results.count)
```

The named form returns an anonymous named tuple or record. Named and positional
branches cannot be mixed; that is **E1117**. Child evaluation and assembly
retain source order. `race` and `any` remain positional.

`task.group name { ... }` creates a lexical group. `task.group name(limit: n)
{ ... }` limits active children; a limit below one is clamped to one before
admission. The group owns and joins every child created in its body. A `Group`
may be passed directly as a free-function or method parameter:

```jet
fn add_work(group: Group, value: Int) {
    task value + 1
}

struct Crawler {
    step: Int

    fn add_stepped(self, group: Group, value: Int) {
        step :: self.step
        task value + step
    }
}
```

`Group` is not allowed in a struct field, return type, local annotation,
lambda parameter, alias, or aggregate. A lambda cannot capture the group
handle. A method receiver does not retain the group, and a spawn through a
`Group` parameter still owns its captures (D-CONC-GROUP1=A).

`join()` is `Task<T>.join() -> T !TaskFailure`; its failures are `.Cancelled`,
`.DeadlineBlown`, and `.Panicked(reason)`. `TaskOutcome`, `TaskStatus`,
`.trace()`, and `.exception()` are not part of this interface.

#### Borrowed captures in a group child

A lexical group joins every child before its block returns. A child may borrow
an owner that remains accessible through that join. Reads are unrestricted;
write borrows require a proof of non-overlap. Distinct fields and distinct
constant indexes are disjoint, while dynamic places are overlapping. Two
children reaching one place report **E1101**.

An owner declared inside the group drops before the group can join it, and a
`Group` passed as a parameter joins in another frame; both cases report
**E1102**. Channels and task bodies still require sendable owned values
(D-TASKBORROW1=A).

### Select and scheduling

A select is a subjectless `if` table. Each arm head binds a value and names a
source; the comma marks the wait. A bare `Bool` arm is rejected rather than
being interpreted as a source. `after` takes a duration literal and fires
when no source is ready by that duration. An optional `else` makes the wait
non-blocking.

```jet
if {
    job, jobs    -> handle(job)
    msg, control -> obey(msg)
    after 100ms  -> retry()
}
```

The table compiles to one wait, so testing a source and then reading it cannot
race. Cancellation at the wait follows the cancellation rules above
(D-CONC-CHAN2=D, D-CONCSELECT1=A).

The scheduler is M:N: tasks park at channel, timer, and I/O waits instead of
blocking OS threads. Native pollers are `epoll` on Linux, `kqueue` on macOS and
BSD, and IOCP on Windows. Task-local failures unwind into the scheduler so a
sibling combinator can report a task panic without exiting the process early
(D-ASYNCRT1=A).

### Deadlock stance

Jet guarantees deadlock-free lock acquisition for one narrow path: a
`#Transact` commit collects its touched `Shared<T>` values and acquires write
locks in stable address order. Plain shared access that reads a second shared
value uses the same ordered commit engine, so it does not nest locks in an
arbitrary order.

Jet does not guarantee deadlock freedom for arbitrary structured-concurrency
programs and does not detect arbitrary deadlocks at runtime. Tasks, groups,
join duties, and channels specify ownership, lifetime, and wait behavior; they
do not prove progress. Tasks can wait for one another through bounded channels,
or a task can wait for a result that no task sends. The scheduler wakes tasks
when a source, cancellation, or deadline changes but does not construct a
global wait-for graph. A deadline or cancellation can bound a wait; neither
makes a deadlock successful.

See [channel buffering](#bounded-buffering-law) and
[concurrency boundary safety](architecture.md#concurrency-boundary-safety).

## Core library

The [core-library reference](reference/core-library.md) is the complete
user-facing module and signature index. Compiler-known `core.*` namespaces
cover file, terminal, environment, process, math, random, time, arguments,
numeric, and encoding operations. `core.encoding` uses one `DataTree` value for
JSON, CSV, TOML, and YAML and supports `#Codable`. Every fallible call returns
`T !E`; handle it with `??`, a pattern test, or contextual `?(text)`. Importing
a core namespace does not by itself emit every helper: code generation keeps
the helpers a program calls. Teaching errors are **E0037**–**E0039**
(D-CORENS1, D-CORENS-CANON1).

Core surface diagnostics and examples are exercised by the `tests/ui/core_*`
fixtures.

### `Bytes`

`Bytes` is a growable byte builder with one read cursor. End of input is
`position == len`.

```jet
buffer :: Bytes.with_capacity(128)
buffer.write([65, 66])
print(buffer.string())
```

Constructors are `Bytes.new()`, `Bytes.with_capacity(n)`, and
`Bytes.from(bytes)`. The write family includes `write_u8`/`write_byte`,
width-specific `write_u16_*`, `write_u32_*`, and `write_u64_*`,
`write_bytes`/`write`, and `write_to`. Cursor operations include `position`,
`eof`, `seek`, `rewind`, `read`, `read_byte`/`next`, `read_bytes`,
`read_string`, `get`, and `first`.

String-like operations decode UTF-8 lossily and then use String behavior:
`contains`, `starts_with`, `ends_with`, `trim`, `trim_start`, `trim_end`,
`to_lower`, `to_upper`, `to_title`, `title`, `replace`, `split`, `join`,
`lines`, `index_of`, `last_index_of`, `is_ascii`, `to_string`/`string`, and
`parse`. Lifecycle and inspection operations include `flush`, `close`,
`shutdown`, `copy`/`clone`, `copy_to`, `equal`, `compare`, `capacity`,
`get_buffer`/`buffer`, `to_bytes`, `len`, `is_empty`, and `clear`.

Consuming typed reads remain on `core.encoding.Reader`; terminal output remains
on the `core.term` writer. `Bytes` does not introduce a second reader or
writer hierarchy. See `Examples/features/io/byte_buffer.jet`
(D-ITERTOOLS1=A, #1467).

### `core.math`

`core.math` supplies floating-point and whole-number helpers: the base libm
family, inverse hyperbolics and accurate near-zero forms, classification and
neighbor operations, decomposition pairs such as `sin_cos` and `frexp`,
special functions, checked/saturating/wrapping integer families, and exact
ratios. Exact ratio values expose numerator, denominator, string, float, zero,
and arithmetic operations.

`round` returns the nearest integer with an exact half away from zero
(`-2.5` becomes `-3`, and `2.5` becomes `3`). For `min` and `max`, one NaN
operand yields the non-NaN operand; two NaN operands yield NaN. Examples are
`Examples/features/math/math_audit.jet`, `more_math.jet`, and `fraction.jet`
(D-MATHLIB2, D-CORESURFACE1, D-NUMTYPE1).

### `core.sys` and process boundaries

System facts and process identity live in `core.sys`; environment variables and
cwd/home are also `core.sys`. Subprocess execution and exit status live in
`core.process`. Safe facts include OS name and family, architecture, CPU count,
temporary directory, executable, `pid`/`getpid`, hostname, username, release,
version, `getppid`, `getuid`, `geteuid`, `getgid`, `getegid`, `getgroups`,
`getpgid`, `getpgrp`, `getsid`, `expand`, `uptime`, `loadavg`, `times`,
`exitcode`, `success`, `sync`, `set_current_dir`, and interrupt registration
(D-OSFACTS1).
POSIX process and session control requires an audited `#Unsafe("...")` region
and a host-OS target gate. The gated set includes `fork`, `setuid`, `setgid`,
`setpgid`, `setpgrp`, `setsid`, `initgroups`, `kill`, `wait`, `waitpid`, `pipe`,
`close_fd`, `mkfifo`, `umask`, `getpriority`, `setpriority`, `utime`, `atexit`,
and `stop`.

These helpers do not emulate POSIX semantics on Windows. Examples are
`Examples/features/io/os_facts.jet` and `os_process_control.jet`.

### Other core contracts

`core.archive.gzip` and `core.archive.zstd` are the byte-stream codec homes;
`core.archive` owns ZIP and tar container operations and does not re-export a
codec (D-CORE-COMPRESS1=A). `core.email` uses typed `Message` values with a
separate envelope, so Bcc is never serialized. Mail transport verifies TLS,
authenticates only after verified TLS and post-upgrade EHLO, never retries, and
reports `DeliveryUnknown` when cancellation or a deadline interrupts after
DATA.
Optional DKIM configuration signs final MIME bytes with one Ed25519 identity;
invalid or missing requested headers fail before connecting, with no unsigned
fallback. Passwords use move-only `Secret` values and are zeroized on failure
and drop (D-EMAIL1, D-EMAIL-SMTP-SURFACE1, D-EMAIL-SMTP-CONFIG1,
D-EMAIL-DKIM-CONFIG1).

`Query<T>` is the typed carrier for ordinary list results, checked readers, and
checked SQL. `data.query(rows)` and the SQL form produce the same carrier;
`collect` materializes an ordinary `[T]`. Query operations retain typed
callbacks and plan order. `inner_join` preserves duplicate-key multiplicity;
`left_join` preserves every left row and uses `?R`; `group_by` produces
`Group<K, V>`. `DataStream<T>` is one-shot and fallible, and `DataLoader<T>` is
stateful and typed. There is no public `Table<T>`, `Series<T>`, `LazyFrame<T>`,
or `DataGroup` carrier (D-QUERY-RETAIN1=A).

## Foreign-function interfaces

### One model for foreign boundaries

A project binder mounts a language root and library name as `<language>.<lib>`.
Generated Jet modules and their provenance live below
`.jet/bindings/<language>/`; generated Jet is ordinary source and can be
inspected. Import a whole library or a member list with
`use <lang>.<lib> as alias` or
`use <lang>.[lib as alias, other]`. A generated descriptor records the ABI
contract, ownership/layout rules, callback and error model, effect leaf,
provider, cache suffix, and capability set. Every generated artifact carries
the `jet-ffi-descriptor-v1` stamp, and a stale stamp is rejected before a
foreign call. Foreign-interface routing and cache validation read the same
descriptor table. (D-FFI-UNIFY1)

A binder is not an unchecked symbol lookup. It must reject an unsupported
signature before emitting a callable surface, record the declaration and tool
identities, and keep foreign diagnostics behind Jet's boundary error (usually
E3208). Generated scalar sidecars use content-addressed bridge identities that
include the descriptor, source bytes, worker, runtime, native toolchain, target,
and function list. An input or toolchain change therefore selects a new
artifact rather than silently reusing an old archive.

### Inline native functions

The inline tier accepts `#FFI(c)`, `#FFI(cpp)`, and `#FFI(asm)` functions whose
body is exactly one triple-quoted raw foreign-source string. The Jet signature
is the checked ABI contract. C, C++, and assembly require an enclosing
`#Unsafe("reason")` gate; assembly also requires `use core.mem`. The checker
validates scalar signatures, named operands, the `; -> return` anchor, clobbers,
and the selected target before lowering. Native code is linked by the native
build path; resident JIT execution reports the boundary by name rather than
pretending that the raw body is portable JIT code. See
[`Examples/features/lowlevel/inline_c.jet`](../../Examples/features/lowlevel/inline_c.jet)
and [`Examples/features/lowlevel/inline_asm.jet`](../../Examples/features/lowlevel/inline_asm.jet).
(D-FFI-INLINE1, D-FFI-RAWBODY1, D-FFI-ASM1, D-FFI-CPP1)

### Rust declarations

The existing direct declaration surface is
`extern rust "crate@version" { ... }`. Each entry has a normal Jet signature
and `= "rust::path"`; `extern rust "std"` needs no extra dependency. A
non-`core` crate requires an exact version pin (E0701). By-value boundary types
include scalars, `String`, `Char`, lists/maps/options/results built from allowed
types, and structs or enums whose fields obey the same rule.
Capability parameters use the `&` and `^` conventions; a raw foreign call that
carries one requires an audited `#Unsafe` boundary (E0702).

A returned resource may declare `#Close(close)`. Its close function must consume
exactly `^` of the handle and return no value, so `close(^handle)` owns one
release. When a dependency is needed, Jet builds a hidden cached cargo bridge
under `~/.cache/jet/ffi/` or the directory selected by `JET_FFI_CACHE_DIR`; it
does not add a manifest to the user's project. Missing cargo is E0703, a
fetch/build failure is E0704, and an invalid foreign path or signature is E0705.
A panic at the foreign boundary becomes the shared runtime report. The worked
surface is [`Examples/features/lowlevel/ffi.jet`](../../Examples/features/lowlevel/ffi.jet);
Rust FFI integration assertions live in [`tests/ffi_rust.rs`](../../tests/ffi_rust.rs).
The descriptor table also records the namespaced `rust.*` and `swift.*`
bridge roots; this direct block is the Rust declaration surface described here.
(S50, D-FFI-CAP1)

### C ABI

`jet inspect bind <header.h> [--pkg <lib>] [--overlay <path>] [--link <lib>] [-o <out.jet>]`
reads bindable C prototypes and writes the cache
`.jet/bindings/c/<lib>.jet` by default. The generated module is marked
`#Bindgen module c.<lib>.__bindgen__`. A source overlay uses
`#Import module c.<lib> { ... }`; the effective module is the generated
binding union, with the overlay winning an incompatible redeclaration. A
header declaration that the generator cannot map is skipped and reported,
not guessed. E3208 means the header cannot be read or contains no bindable
prototype; write an explicit overlay for a declaration outside the generated
subset. (S58, S59)

`#Bindgen` is compiler-generated and legal only in
`.jet/bindings/c/<lib>.jet`; a user overlay uses `#Import module`.
The reserved `__bindgen__` path is E3206. A second C `use` form for the same
library is E3204; an incompatible overlay is E3205, and handwritten
`#Bindgen` is E3207.

A C declaration binds by-value scalars, `String`, `Char`, and C-layout
aggregates. Lists, maps, tuples, options, results, and other Jet aggregates
are E3203 at this boundary. Pointer returns are E3202. The status-plus-out
pattern is the deliberate exception: a pointer parameter is allowed only when
its pointee is C-safe, the wrapper is marked as requiring an audited unsafe
call, and the caller creates and initializes the slot through `core.mem`,
checks the returned status, and reads the slot only when the ABI promises it
was initialized. Raw pointer work still requires `use core.mem` and
`#Unsafe("reason")`. (D-CABI-RESULT1)

A C `String` return must be non-null, NUL-terminated, and valid UTF-8. Jet
copies it immediately and never frees the C pointer. Owned buffers, nullable
strings, other encodings, and library-specific free functions remain raw until
a user-written audited wrapper supplies their ownership contract. C library
linking uses the last `<lib>` segment as its key. A `c@system` dependency uses
pkg-config with a bare `-l` fallback; a `c@"path"` dependency supplies local
include/library/link information. Otherwise pkg-config is queried and E3201
reports a missing link identity. Link flags are resolved at build time.

The end-to-end C binding checks are in [`tests/cffi.rs`](../../tests/cffi.rs).

### C++ ABI

`jet inspect bind cpp <header.hpp> --target <triple> --clang <absolute-path> --ar <absolute-path> [--pkg <lib>] [--namespace <name>] [--instantiate <qualified=type:jet-name>] [-I <dir>] [-L <dir>] [-l <lib>] [-o <out.jet>]`
uses a clang AST and a content-addressed C-ABI shim. The target, clang path,
and archiver path are required and absolute. Namespace selection is explicit;
public scalar classes become owned opaque handles, exceptions become checked
`T !CppError` results, and pure named callbacks retain a checked C ABI. Template
instantiations are emitted only for explicit `--instantiate` requests. Include
paths, library paths, and link libraries are part of binding provenance and are
reused at final link. Unsupported layouts or callbacks fail binding rather than
being approximated. (D-FFI-CPP1)

### Python

**Binds.** `jet inspect bind py <script.py> [--pkg <lib>] [-o <out.jet>]`
accepts top-level functions whose parameters and result use the scalar
annotations `int`, `float`, or `bool`. Defaults, variadic parameters, async
functions, malformed signatures, and other annotations are rejected with the
binder error E3208.

**Boundary.** The generated `.jet/bindings/py/<lib>.jet` wrapper, supervised
worker, C archive, and provenance record form one checked scalar surface. Calls
carry `-[FFI.Py]>`; worker stdout/stderr, exception text, tracebacks, command
strings, and raw symbols do not cross into Jet. Non-finite values and malformed
worker results fail closed. Python package installation remains the PyPI
provider's responsibility; the binder consumes the provisioned `python3` and
records its identity.

**Evidence.** The scalar archive is under
`.jet/bindings/py/.bridges/<identity>/`, with a stable `lib<abi>.a` projection.
The identity includes the descriptor, source and worker bytes, runtime,
native toolchain, and typed function list. The source fixture is
[`Examples/interop/python`](../../Examples/interop/python); the generated
surface and effect spelling are asserted in [`tests/ffi_python.rs`](../../tests/ffi_python.rs). (D-FFI-PY1)

### JavaScript and TypeScript

**Binds.** `jet inspect bind js <module.d.ts> --runtime <module.js> [--pkg <lib>] [-o <out.jet>]`
uses the declaration file as the ABI source and checks the runtime module with
Node. Exported declarations may use only `number`, `bigint`, and `boolean`
parameters and results. Optional, default, rest, dynamic, callback, and
asynchronous result shapes are rejected instead of being guessed.

**Boundary.** The generated wrapper uses scalar conversion and a supervised
Node worker for the bind-time/native sidecar. It rejects non-finite numbers,
wrong scalar results, missing exports, and an async result where the declaration
promises a scalar. The same `<lang>.<lib>` surface is target-dispatched: web
builds use the browser JavaScript engine, while native builds use the native
JS-on-WASM host. Npm realization remains provider work.

**Evidence.** The cache stores `<lib>.jet` and declaration provenance below
`.jet/bindings/js/`, including the runtime and declaration identities. The
language descriptor supplies the `FFI` effect root and the target capability
row. (D-FFI-JS1)

### Go

**Binds.** `jet inspect bind go <source.go> [--pkg <lib>] [-o <out.jet>]`
selects cgo `//export Name` functions whose parameters and optional result are
`int64`, `float64`, or `uintptr`. The provisioned compiler builds a
`-buildmode=c-archive` archive and writes a typed `go.<lib>` cache.

**Boundary.** Calls execute in-process through the C archive; the Go runtime is
part of the native program, not a sidecar. `uintptr` is a private, move-only
`go.<lib>.Handle`; passing it to a foreign function consumes the handle so a
released `runtime/cgo.Handle` cannot be reused. Handles require a 64-bit host
ABI. Generated calls carry `FFI.Go`; unsupported signatures and tool failures
are rejected or laundered through E3208. Compilation has a 60-second deadline
and bounded diagnostics.

**Evidence.** Use [`Examples/features/lowlevel/polyglot_go`](../../Examples/features/lowlevel/polyglot_go)
for the archive, handle, and close path. (D-FFI-GO1)

### Fortran ISO C binding

**Binds.** `jet inspect bind fortran <source.f90> [--pkg <lib>] [-o <out.jet>]`
selects explicit `bind(C, name="...")` functions. Scalar
`integer(c_int64_t)` and `real(c_double)` inputs require `value`. Fixed-shape
input arrays use `intent(in)` and map to flat `[Int]` or `[Float]` values.

**Boundary.** The generated wrapper records every array extent and rejects a
list whose length differs from the declared shape before passing its pointer
through the private C ABI. The order is Fortran column-major order. Unsupported
shapes and gfortran failures use E3208, and calls carry `FFI.Fortran`.

**Evidence.** The checked matrix fixture is
[`Examples/features/lowlevel/polyglot_fortran`](../../Examples/features/lowlevel/polyglot_fortran). (D-FFI-FORTRAN1)

### COBOL and copybooks

**Binds.** `jet inspect bind cobol <program.cob> --copybook <record.cpy> [--pkg <lib>] [-o <out.jet>]`
compiles one GnuCOBOL linkage program. The copybook subset is one level-01
record with level-05 fixed text, `COMP-5` integers, and `COMP-3` packed
decimals. The binder records offsets and widths and emits an editable
`#Codable` Jet record.

**Boundary.** `COMP-3` maps to `Decimal`, never `Float`; the C bridge accepts
packed decimal values as scaled minor-unit `Int` values. It initializes libcob
once and calls the exported `int PROGRAM(cob_u8_t*)` entry in-process. Range
and foreign-program failures become `CobolError`, and calls carry
`-[FFI.Cobol]>`. Undefined-link checks, the descriptor, source/copybook
hashes, runtime, and archive hash are recorded in `.provenance`. Unknown
layouts and ABI-proof failures use E3208; bridge tools have 60-second and
64-KiB bounds.

**Evidence.** The payroll fixture is
[`Examples/interop/cobol`](../../Examples/interop/cobol). (D-FFI-COBOL1)

### Java and the JVM

**Binds.** `jet inspect bind java <source.java> [--pkg <lib>] [-o <out.jet>]`
compiles the source and discovers public descriptors with `javap -s`. One
constructor and non-overloaded methods using `long` and `double` form the
supported surface.

**Boundary.** A generated JNI bridge links the provisioned `libjvm`, starts one
JVM lazily in the Jet process, attaches calling threads, and destroys the JVM
at process teardown. Java objects are opaque `java.<lib>.Handle` values in a
bounded 1,024-slot global-reference table. Calls borrow a handle;
`close(^handle)` consumes it and releases the global reference. Constructors
and value-returning methods return `JavaError.Exception` on a Java exception;
Java stack text and foreign frames remain inside the bridge. Calls carry
`FFI.Java`, and javac/javap/cc/ar are bounded to 60 seconds with 64-KiB
capture.

**Evidence.** Provenance joins the source, reflected bytecode surface, class
cache, and schema. See
[`Examples/features/lowlevel/polyglot_java`](../../Examples/features/lowlevel/polyglot_java). (D-FFI-JVM1)

### .NET

**Binds.** `jet inspect bind cs <source.cs> [--pkg <lib>] [-o <out.jet>]`
compiles with the provisioned .NET 8 SDK and uses reflection. One public class,
one constructor, and non-overloaded `long`/`double` methods project into a
typed `cs.<lib>` module.

**Boundary.** The native archive embeds CoreCLR through `hostfxr` and
`load_assembly_and_get_function_pointer`; managed entry points use
`[UnmanagedCallersOnly]`. No worker or file protocol participates in calls.
Instances are opaque move-only handles in a 1,024-slot generation-checked
`GCHandle` table. `close(^handle)` releases the managed root. Exhaustion,
managed exceptions, and invalid/stale handles become `DotNetError.ResourceLimit`,
`DotNetError.Exception`, and `DotNetError.InvalidHandle`; foreign exception
text does not cross. Calls carry `FFI.DotNet`, with 60-second tool deadlines
and 64-KiB diagnostics.

**Evidence.** Provenance binds the source, reflected surface, hostfxr identity,
and schema. See
[`Examples/features/lowlevel/polyglot_dotnet`](../../Examples/features/lowlevel/polyglot_dotnet). (D-FFI-DOTNET1)

### Tcl

**Binds.** `jet inspect bind tcl <script.tcl> [--pkg <lib>] [-o <out.jet>]`
compiles a standard-only C bridge against the provisioned Tcl headers and
runtime. `open()` creates an in-process interpreter and evaluates the script
once; `eval`, `eval_int`, and `eval_float` share that session. `eval_once` uses
a fresh interpreter for one call.

**Boundary.** `Session` is opaque and thread-affine. A bounded 64-slot table owns
interpreters; `close(^session)` consumes the Jet handle, and process teardown
cleans remaining interpreters before Tcl finalization. String results are
copied through a 64-KiB boundary and reject embedded NUL and oversized values.
Typed Tcl parsing supplies integer and float entrypoints. Tcl failures become
`TclError.Eval`; raw Tcl result text and frames stay inside the bridge. Calls
carry `FFI.Tcl`. Evaluation is synchronous: this surface makes no
cancellation claim for a long-running command.

**Evidence.** Binding tools use a 60-second deadline and 64-KiB capture; source,
runtime, and schema are hashed in provenance. See
[`Examples/features/lowlevel/polyglot_tcl`](../../Examples/features/lowlevel/polyglot_tcl). (D-FFI-TCL1)

### Lua

**Binds.** `jet inspect bind lua <script.lua> [--pkg <lib>] [-o <out.jet>]`
discovers direct top-level `function name(input)` declarations without running
the script and compiles against provisioned Lua 5.4. Each `open()` owns an
independent in-process `lua_State`; the script runs once for session
initialization and its mutable state persists within that session.

**Boundary.** Generated functions use `DataTree`; sibling
`<name>_typed<T>` adapters require `T: [Encode, Decode]` and validate the
decoded result. Null, booleans, integers, floats, strings, lists, and
string-keyed maps retain their data meaning. Cycles, unsupported keys/values,
depth over 64, and input/output at least 1 MiB fail at the boundary. Lua
failures become closed `LuaError` variants and do not expose stack text.
`<name>_view(session, deadline_ms)` can pin a returned table in the session;
`TableView` reads and writes the live table without JSON serialization and
`close` releases its registry reference. Session close invalidates stale views
with `LuaError.NotRunning`.

**Evidence.** VM hooks enforce call deadlines and observe cancellation without
destroying a healthy session. A generation-tagged 32-slot state table owns
sessions; tools have 60-second and 64-KiB bounds, and provenance joins source,
runtime, and schema. LuaRocks realization remains provider work. See
[`Examples/interop/lua`](../../Examples/interop/lua). (D-FFI-LUA1)

### Ada

**Binds.** `jet inspect bind ada <package.ads> [--pkg <lib>] [-o <out.jet>]`
reads exported functions from an Ada package specification and compiles its
sibling body with GNAT. Supported exports use `Export`, `Convention => C`, and
`External_Name`; scalar arguments and results use
`Interfaces.C.long_long`/`Long_Long_Integer` or
`Interfaces.C.double`/`Long_Float`.

**Boundary.** A scalar subtype with `range LOW .. HIGH` becomes a pre-call check;
an out-of-range value returns `AdaError.Constraint` before the export runs.
GNAT elaboration runs once and finalization runs at process exit. Calls carry
`FFI.Ada`; GNAT, binder, C compiler, and archiver tools have 60-second and
64-KiB bounds, and missing or non-absolute runtime identity is rejected.

**Evidence.** Provenance hashes the specification, body, GNAT identity, and
schema. `jet import ada <dir>` preserves the Ada sources and emits an editable
binder stub with JT0101 for unsupported semantics; it does not translate
ranges, exceptions, tasking, representation clauses, or ownership. The native
fixture is [`Examples/features/lowlevel/polyglot_ada`](../../Examples/features/lowlevel/polyglot_ada). (D-FFI-ADA1)

### Object Pascal

**Binds.** `jet inspect bind pascal <library.pas> [--pkg <lib>] [-o <out.jet>]`
compiles FreePascal `cdecl` exports. `Int64` and `Double` cross as Jet `Int`
and `Float`. A declared class uses exported `<class>_new`, pointer-first scalar
methods, and `<class>_free` wrappers. `jet import pascal <dir>` preserves the
source and emits a binder stub with JT0101 rather than inventing class or
ownership semantics.

**Boundary.** Class pointers never reach Jet. The C bridge owns them in a
bounded 64-slot table and returns opaque move-only identities;
`<class>_close(^handle)` consumes one identity. Stale or double close fails
before the Pascal destructor runs, and process teardown destroys remaining
objects before runtime finalization. Calls carry `FFI.Pascal`; compiler and
archiver tools have 60-second and 64-KiB bounds, and native links pin the
Pascal runtime search path.

**Evidence.** See [`Examples/features/lowlevel/polyglot_pascal`](../../Examples/features/lowlevel/polyglot_pascal). (D-FFI-PASCAL1)

### Dart and Flutter

**Binds.** `jet inspect bind dart <contract.dart> --jet <compute.jet> [--pkg <lib>] [-o <out.jet>]`
builds one bidirectional, in-process FFI surface. The generated
`<lib>_host.dart` loads the native Jet compute library with `dart:ffi`,
initializes `dart_api_dl` from `NativeApi.initializeApiDLData`, and registers
isolate-local callbacks. The Dart or Flutter application owns the isolate; no
helper process, shell, environment transport, or file protocol participates in
a call.

**Boundary.** `shutdownJetDart()` unregisters every native callback before
closing pinned `NativeCallable` values. Callback functions are top-level
`@pragma('vm:entry-point')` functions with positional `int`/`double` inputs and
an `int`/`double` result. Optional, named, generic, object, string, async, and
overloaded shapes are rejected. Generated wrappers return
`DartError.NotInitialized` before API-DL initialization and
`DartError.CallbackUnavailable` before registration. Calls carry `FFI.Dart`
and are synchronous and isolate-thread-affine.

**Evidence.** Flutter deploys the generated host and platform library through
ordinary native-library packaging; Jet does not embed or launch a Flutter
engine. SDK, C, archiver, and native compilation are bounded to 60 seconds and
64-KiB capture, with E3208 for tool failures. See
[`Examples/features/lowlevel/polyglot_dart`](../../Examples/features/lowlevel/polyglot_dart). (D-FFI-DART1)

### PowerShell

**Binds.** `jet inspect bind pwsh <script.ps1> [--pkg <lib>] [-o <out.jet>]`
parses named functions with PowerShell 7, maps the conventional `-` separator
to `_` in Jet names, and writes a typed cache. `open()` starts one supervised
`pwsh` worker, waits for its startup handshake, and loads the script once. The
worker accepts only binder-approved function identities; Jet never sends source
or an arbitrary command string. The shipped supervisor is POSIX-only.

**Boundary.** Each call accepts and returns one `DataTree` through a
length-framed structured JSON protocol. Objects, lists, integers, floats,
booleans, text, and null retain their data meaning. Requests and responses are
capped at 1 MiB and depth 64. Exceptions become
`PowerShellError.CommandFailed`; error records, paths, stderr, and stack traces
remain in the worker. Calls carry `FFI.PowerShell`.

**Evidence.** A call has a 1–300000 ms deadline. Expiry or cancellation kills
and reaps the complete process group and invalidates its generation-tagged
session; `close(^session)` consumes the handle. At most 32 workers exist per
process. Binding-time tools have 60-second and 64-KiB bounds and E3208
laundered diagnostics. See
[`Examples/interop/powershell`](../../Examples/interop/powershell). (D-FFI-PWSH1)

### Perl

**Binds.** `jet inspect bind perl <script.pl> [--pkg <lib>] [-o <out.jet>]`
uses compiler metadata to discover named main-package `sub` declarations without
running the top-level body. Foreign names project to Jet `snake_case`, while
the worker calls the exact Perl name. `open()` starts one supervised POSIX
worker, loads the script once, and retains package and lexical state. Generated
entries carry a fixed allowlist.

**Boundary.** Each entry accepts and returns one `DataTree` through Perl's core
`JSON::PP`. Requests and responses are length-framed and capped at 1 MiB;
stdout is not a result channel. Exceptions become
`PerlError.CommandFailed`; stderr, paths, stack traces, and exception text stay
inside the worker. Calls carry `FFI.Perl` and do not accept runtime source.

**Evidence.** Calls use a 1–300000 ms deadline. Timeout or cancellation kills
the process group; generation-tagged handles reject stale identities and
`close(^session)` consumes a session. At most 32 workers exist. CPAN
realization is provider work. Binding tools have 60-second and 64-KiB bounds
and E3208 diagnostics. See
[`Examples/interop/perl`](../../Examples/interop/perl). (D-FFI-PERL1)

### Ruby

**Binds.** `jet inspect bind ruby <script.rb> [--pkg <lib>] [-o <out.jet>]`
uses Ruby's `Ripper` parser to find direct top-level methods without executing
the file. A bindable method has one required positional argument and a
Jet-compatible name. The generated allowlist fixes the callable identity;
source and arbitrary commands never cross the API.

**Boundary.** `open()` starts one supervised worker and loads the script once,
retaining its state. Methods accept and return `DataTree` through Ruby's
standard JSON library. Length-framed messages are capped at 1 MiB and stdout
is not the result channel. Ruby exceptions become
`RubyError.CommandFailed`; exception text, traces, stderr, and paths stay
inside the worker. Calls carry `FFI.Ruby`.

**Evidence.** Timeout and cancellation use a 1–300000 ms deadline and kill the
process group; `close(^session)` consumes a generation-tagged session, with at
most 32 workers. Compiler and archive tools have 60-second and 64-KiB bounds.
RubyGems installation remains provider work. See
[`Examples/interop/ruby`](../../Examples/interop/ruby). (D-FFI-RUBY1)

### PHP

**Binds.** `jet inspect bind php <script.php> [--pkg <lib>] [-o <out.jet>]`
requires a POSIX supervisor, lints the script, and discovers top-level named
PHP functions. A bindable function takes exactly one required positional
argument by value; references, defaults, variadics, nested functions, and
unsupported generated names are rejected. The generated cache uses a native C
pool bridge.

**Boundary.** PHP calls accept and return one `DataTree` over a length-framed
JSON protocol. The pool has four workers per pool and eight generation-tagged
pool slots. A worker allowlist fixes the function identity; stdout is not the
result channel. `PhpError` distinguishes not-running, timeout, cancellation,
protocol, command, and resource-limit failures. Calls carry `FFI.Php` and do
not expose PHP exception text or source paths.

**Evidence.** Worker frames and responses are capped at 1 MiB. A call deadline
or cancellation replaces the affected workers; `close(^pool)` consumes the
pool handle. Binding lint, C compilation, and archiving use bounded diagnostics
and the descriptor/provenance record. See
[`Examples/interop/php`](../../Examples/interop/php). (D-FFI-PHP1)

### R

**Binds.** `jet inspect bind r <script.R> [--pkg <lib>] [-o <out.jet>]`
parses the script without running its top-level body and binds direct named
functions with one required argument. `open()` starts a supervised worker,
loads the script once, and retains its state. A normal function round-trips
`DataTree`; `<name>_table<T>` maps ordinary `[T]` rows to a data frame and back
through the same framed channel.

**Boundary.** `<name>_plot` runs on an isolated SVG device and returns a
`String`. The bridge parses the XML structurally and emits deterministic
canonical XML. It rejects scripts, event handlers, `foreignObject`, external
references, active CSS, declarations, entities, malformed XML, and input or
output above 512 KiB. R failures and rejected SVG content become
`RError.CommandFailed`; calls carry `FFI.R`. Each worker receives a private
temporary directory, which is removed on success, failure, cancellation, and
close.

**Evidence.** Calls use a 1–300000 ms deadline, process-group cancellation,
generation-tagged handles, 1-MiB frames, and at most 32 workers. CRAN
realization remains provider work. See
[`Examples/interop/r`](../../Examples/interop/r). (D-FFI-R1)

### Octave

**Binds.** `jet inspect bind octave <script.m> [--pkg <lib>] [-o <out.jet>]`
requires a POSIX supervisor and `octave-cli` (or `octave`). It discovers
functions of the form `name = function(input)` with exactly one matrix input
and one matrix output. Multiple outputs, `varargin`, duplicate names, and
non-identifiers are rejected before a bridge is emitted.

**Boundary.** The generated `octave.<lib>` surface accepts a rank-two `Tensor`
and returns a rank-two `Tensor !OctaveError -[FFI.Octave, GPU]>`. The wire value
carries exact shape and column-major flat data. Jet checks rank, dimensions,
width, finiteness, and real numeric output; frames and responses are capped at
1 MiB. A generation-tagged 32-session supervisor enforces deadlines and
cancellation, and errors become `OctaveError` variants without foreign text.

**Evidence.** Provenance records the script, Octave identity, worker, JSON
transport, column-major order, rank-two shape, and session bound. Binding tools
use 60-second and 64-KiB capture. See
[`Examples/interop/octave`](../../Examples/interop/octave). (D-FFI-OCTAVE1)

### Windows COM automation

**Binds.** `com.*` exists only on Windows. On another host, importing it or
running `jet inspect bind com` fails with E3260 before reading a type library or
looking for a cache. On Windows,
`jet inspect bind com <library.tlb> [--pkg <lib>] [-o <out.jet>]` reads a
file-backed type library; the registered form is
`--registered <guid> --major <n> --minor <n> [--lcid <n>]`.

**Boundary.** The inspector uses `ITypeLib` and `ITypeInfo` and rejects hidden,
restricted, out-parameter, or unrepresentable members. Primitive VARIANT types
become Jet scalars, BSTR becomes `String`, dispatch interfaces become
move-only opaque `Object` values, and VARIANT/SAFEARRAY values cross as bounded
`DataTree`. Dynamic name-based `IDispatch` is available only inside explicit
`#Unsafe`; generated safe calls use fixed DISPIDs. Each live object owns a
single-threaded apartment and a generation-tagged handle tied to its creating
thread. `close(^object)` releases it and balances `CoUninitialize`; stale,
cross-thread, and double-close uses fail before invocation. HRESULT and
EXCEPINFO become `ComError` variants without vendor text. Calls carry
`FFI.Com`; frames and DataTree depth are capped at 1 MiB and 64.

**Evidence.** Provenance hashes the extracted type-library schema and generated
surface. A released file-backed binding may omit the original `.tlb`, but when
the input remains available its bytes must match the recorded hash. See
[`Examples/features/lowlevel/polyglot_com`](../../Examples/features/lowlevel/polyglot_com). (D-FFI-COM1)

### Data-schema binders

`jet inspect bind json|csv|sql|xml|proto <input> [--type <Type>] [-o <output>]`
reads a data schema or sample and writes ordinary Jet source. The output uses
one `#Codable` struct per record and adds `#Rename` when a wire key is not a
valid Jet name. The default destination is
`.jet/bindings/<sanitized-input-stem>.jet`; `-o` selects another path. The
binder does not import the file for the author: read it, commit it, and own it
as source, including any hand edits.

Each generated file begins with provenance containing the exact command, input
path, input `sha256`, format, and one `inference` line for every rule applied.
Command and path fields are escaped so a hostile file name cannot inject Jet
source. Regeneration is explicit. Writing the default destination with a
different recorded command is E2104, an unreadable input or unwritable output
is E2105, and an invalid schema or input with no record is E3208. The generic
binder implementation and CLI dispatch live in
[`Source/CmdDevTools.rs`](../../Source/CmdDevTools.rs). (D-BOUND-BIND1, D-NAME-FILES1)

## Browser effects and web values

`core.web` is the server-rendered web application surface over `core.http`; its
`App`, pages, sessions, forms, and live records are Jet values. Browser-owned
operations carry the `Browser` effect and are lowered by the web target. The
source authority is [`Core/web/web.jet`](../../Core/web/web.jet), while the
browser test controller is [`Core/web/browser.jet`](../../Core/web/browser.jet). (D-FLAGSHIP-WEBAPI1)

### Browser events, values, and storage

Use `core.web` for the checked browser operations:

- `web.on(target, event, handler)` registers a DOM event handler. The handler
  receives `WebEvent`; a handler that does not need it can ignore the argument.
- `web.value(target) -> String` reads the selected input or element value.
- `core.web.storage.local.get(key) -> ?String` and
  `core.web.storage.session.get(key) -> ?String` read origin-local and
  tab-scoped storage. `set`, `remove`, and `clear` mutate the corresponding
  namespace, and `get_or` composes a missing key with a fallback.

For example, the browser conformance surface uses
`web.on("#new-task", "input", handler)` and
`web.storage.local.set("draft", web.value("#new-task"))`. The generated web
runtime uses `addEventListener`, DOM selection, `localStorage`, and
`sessionStorage`; native codegen provides inert checked routes rather than
asking rustc to type-check browser APIs. The round-trip is exercised by
[`tests/web_build.rs`](../../tests/web_build.rs).

### Web queries

A `WebQuery` has one explicit cache key. The key is the record identity; the
current Core query source does not make a separate public footprint parameter.
`query.live(key, data, url, loader)` creates an initial fresh query and calls
`loader` when `url` is nonempty. `query.new(key, seed)` creates a seeded query,
and `query.subscribe(key)` creates a stale subscription record.

The lifecycle states are `Pending`, `Fresh`, `Stale`, `Fetching`, `Error`, and
`Offline`. Mutation states are `Idle`, `Pending`, `Success`, `Error`, and
`Settled`; network modes are `Online`, `Always`, and `OfflineFirst`. `get`,
`show`, `state`, `state_signal`, `mutation_state`, `mutation_signal`, and
`facts` read values and bounded lifecycle facts. `invalidate(&q)` marks a
query stale and advances its generation. `refresh(&q)`, `queue(&q, data)`, and
`retry(&q, replay)` are fallible operations; `cancel(&q)` returns whether an
in-flight operation was cancelled. `mutate` checks the key and optional
expected value, applies the mutation, and returns a `WebMutationState`;
`mutate_with_invalidations` additionally validates target keys.

Keys are at most 256 bytes, payloads at most 1 MiB, and subscribers and
invalidation targets are bounded by the Core limit of 1,000,000. Reusing a key
with a different dependency footprint is E2473: a key must have one
invalidation declaration. Offline mode reports `WebQueryError.Offline` rather
than pretending that a mutation was durable. The offline-first runtime persists
queued payloads and invalidation targets under
`$JET_WEB_QUERY_QUEUE_PATH`, `$XDG_STATE_HOME/jet/web-query-queue.v1`, or
`$HOME/.local/state/jet/web-query-queue.v1`; it uses a temporary file and atomic
rename and reports a durability error when no usable state directory or queue
file is available. Query state and queue transitions are defined by
[`Core/web/query.jet`](../../Core/web/query.jet) and
[`CoreLib/Top/WebQuery.rs`](../../crates/jet-codegen/src/Prelude/CoreLib/Top/WebQuery.rs). (D-WEBQUERY1)

### Events and hooks

`use core.event as event` exposes typed compiler-known event values; it does
not add an event declaration syntax. `event.new<T>()` creates an infallible
synchronous `Event<T>`. `event.scope()` creates an owner for subscriptions;
`scope.cancel()` is idempotent, unsubscribes every owned listener, and makes
later subscriptions through that scope inactive. `scope.active_count()` counts
active subscriptions.

`Event<T>.on(scope, handler)`, `.once(scope, handler)`, and
`.on_priority(scope, priority, handler)` return a `Subscription`. Priority sorts
higher values first and registration order breaks ties. `once` deactivates its
subscription before invoking the handler. `emit(payload)` returns an
`EventTrace` with delivered, queued, dropped, and summary accessors.

Synchronous emission snapshots the active listeners at dispatch start, sorts by
priority and registration order, and dispatches that snapshot depth-first.
Unsubscribing before a listener's turn skips it; subscribing during delivery
affects a later or nested emission. A `once` listener cannot run twice through
reentrant emission. The beginner event path is infallible; typed failure
aggregation belongs to the asynchronous event surface. (D-EVENT1,
D-EVENT2)

`event.async_result<T, E>(policy, failures)` returns
`AsyncEvent<T, E> !EventConfigError`. Its policy has a positive queue capacity
and one overflow mode: `Block`, `DropNewest`, or `DropOldest`. Its failure
policy is `StopFirst`, `Collect`, `Log`, or `Ignore`. `on`, `once`, and
`on_priority` handlers may return unit or `Result<(), E>`; `emit_async` returns
a task whose `DispatchReport<E>` records acceptance, terminal state, delivered
handlers, failures, and ordered trace. `queued_count`, `running_count`, and
`blocked_count` expose bounded lifecycle facts. Close and scope cancellation
make queued, blocked, or running entries terminate with explicit states rather
than silently dropping the report. (D-EVENT-CONTINUE1)

`event.hook<T, R>(fallback)` creates an ordered intervention point. Its
`.run(payload, fallback)` returns the last active handler result, or the
call-site fallback when no handler is active. `event.decision_hook<T, E>(HookPolicy.FirstCancelElseTransform)`
creates a typed fold. Handlers return `HookDecision.Continue`,
`.Transform(value)`, `.Cancel`, or `.Fail(error)`; `run(payload)` returns
`HookOutcome.Continue(value)`, `.Cancel`, or `.Fail(error)`. Hooks use the same
scope, priority, and once rules as events.

```jet
use core.event as event

fn run() {
    scope :: event.scope()
    clicked :: event.new<Int>()
    clicked.on(scope, n -> print("clicked {n}"))
    clicked.once(scope, n -> print("once {n}"))
    print(&clicked.emit(1).summary())
    scope.cancel()
}
```

With `JET_OBSERVE=1`, the runtime publishes one bounded, payload-free sequence
for Event, AsyncEvent, and DecisionHook transitions. `jet inspect live` and a
Canvas session attached to the live Jet identity consume that validated source;
a source-call match alone is not a runtime observation. The observation writer
bounds event history and does not include channel values, task locals,
environment, or credentials. The executable event example is covered by
[`tests/canvas.rs`](../../tests/canvas.rs) and the public API by
[`Core/event/event.jet`](../../Core/event/event.jet). (D-OBSERVE-LIVE1, D-OBSERVE-TASK1)

## Terminal direct input

A `#Live { ... }` block enters unbuffered, no-echo terminal input for its body.
An RAII guard restores the terminal on normal return, `?` propagation, and panic
unwind. The marker itself does not require an import; `term.read_key()` does.

```jet
use core.term as term

#Live {
    key :: term.read_key()
    if key == Enter { return }
    print("got: {key}")
}
```

`Key` is a prelude enum exposed by `core.term` with these variants:

| Variant | Payload | Meaning |
| --- | --- | --- |
| `Char(c)` | `Char` | Printable character |
| `Enter` | — | Enter or Return |
| `Escape` | — | Escape |
| `Backspace` | — | Backspace |
| `Tab` | — | Tab |
| `Delete` | — | Forward delete |
| `Up`, `Down`, `Left`, `Right` | — | Arrow keys |
| `F(n)` | `Int` | Function key F1–F12 |
| `Ctrl(c)` | `Char` | Ctrl plus a character |
| `Unknown` | — | Unrecognized byte sequence |

Pattern tests can bind payloads with `==`, and enum literals may use the
qualified form such as `Key.Char('a')` or `Key.Enter`.

A live block is impure: E3401 rejects it in a `-[]>` function. E3301 rejects it
for a target without an OS terminal device, and the interactive REPL rejects
it. The implementation uses inline `extern "C"` for POSIX termios and
`extern "system"` for the Windows console API; it does not require an external
crate (D-TERM1, D-DEFER1).

## Formatting Jet source

`jet fmt <file.jet>` formats source in place. `jet fmt --check <file.jet>`
reports files that would change and exits nonzero without writing them; add
`--diff` to the check for unified diffs. `jet fmt -` reads source from stdin
and writes formatted source to stdout; `--stdin-path=<label>` supplies the
reported source name. These commands run the lexer, parser, and printer, not
sema or rustc. (S44)

The formatter also accepts `--lang`, `--simplify`, `--changed`, and
`--explicit-copies`. `--simplify` opts into the ratified simplest spellings;
the default output does not depend on it. `--changed` selects changed project
files. `--explicit-copies` keeps copy operations explicit where that mode
requires them. For a write-free preview, use `--check` and optionally
`--diff`; the public help advertises `--check`.

Canonical style uses four-space indentation, an opening brace on the header
line, one statement per line, at most one blank line between top-level items,
spaces around binary operators, no space before `;`, `,`, or a call `(`, and
trailing commas only on multiline comma lists. Explicit semicolons are not
canonical: E0373 supplies the behavior-preserving line-break or end-of-line
fix. Comments are retained and reattached by source span. A real parse error
blocks formatting. (D-TRAILCOMMA1, D-SEMI1)

`package.jet` is formatted by its typed closed-record formatter rather than the
ordinary Jet formatter. If that path cannot place an authored comment safely,
it fails closed instead of declaring the file clean. A leading inline package
carrier is preserved byte-for-byte, including its source spans; only the Jet
source after the carrier is formatted. The driver preflights every selected
file before writing any file, so one read or parse failure leaves the batch
unchanged. The formatter contract is exercised by
[`tests/fmt.rs`](../../tests/fmt.rs); its idempotence law is
`fmt(fmt(source)) == fmt(source)`.

## Writing and running tests

A top-level `#Test("name") { ... }` block is a test claim. Its body has ordinary
function statements and uses `assert`, `assert_eq`, snapshots, or another
checked assertion. Duplicate names report E0105; a nested `#Test` reports
E0601. `jet run` and `jet build` ignore test claims, while `jet test` compiles
and runs them. (S43, D-CASING1)

For an explicit file, `jet test <file.jet>` builds one generated AOT harness for
that file. Each claim runs behind a process-local unwind boundary, and the
harness emits one result per claim plus a summary. Human output names `pass`,
`FAIL`, `skip`, expected-fail, and unexpected-pass outcomes; JSON mode emits the
same result categories. `jet test` exits with status 1 when any selected claim
fails. A failing assertion carries the E3001 report and Jet
source location. A test target with no tests or doctests reports E0601; bare
package testing skips source members with neither, then reports the resolved
package contains no testable member at all. (R9)

Bare `jet test` in a package discovers every source member in the package's
checked project set, not only the entry file. A directory without a package
manifest is walked recursively for `*.jet` files. A package-level `fn test`
override owns the command unless `--show-default` requests the stock harness.
The default child-output policy is `--capture=failed`; choose `all` or `none`
explicitly. `--fresh` bypasses only the persisted test-result cache; the build
cache remains reusable. `--docs` selects checked documentation examples.
`--watch`, `--where=<expr>`, `--filter=<substring>`, `--shuffle[=<seed>]`,
`--serial`, `--coverage`, `--update-snapshots` (or `-u`), `--release`, and
`--profile=<name>` select the corresponding test-run behavior. Browser claims
use `--browser=<engines>`, `--browser-retries=<n>`,
`--browser-reporter=<text|json|html>`, `--browser-ui`, `--browser-visual`, and
`--browser-trace`.

### Test scope members

A dot-prefixed scope member is a statement directly inside a marker that
publishes that vocabulary. `#Test` declares `.setup`, `.expect_fail`,
`.timeout`, `.skip`, and `.measure`; members cannot occur in an ordinary
function, inside a control block, or inside another member. E0614 reports an
unknown member, E0615 reports a member outside a vocabulary, E0616 requires
`.setup` to be first, E0617 reports the wrong argument shape, and E0618
reports nesting. The checker validates malformed members in every command mode,
but only `jet test` executes them. (D-DOTSCOPE1)

- `.setup { ... }` is first, runs inline, and leaves its bindings visible to
  the rest of the test. It does not create a separate scope.
- `.expect_fail { ... }` requires a runtime stop. It may name one registered
  E30xx code and a non-empty string literal message, as in
  `.expect_fail(E3001, message: "fingerprint collision") { ... }` or
  `.expect_fail(message: "fingerprint collision") { ... }`. The message is a
  plain, case-sensitive substring of the raw stop text, not a pattern or a
  match on the rendered frame or `panic:` prefix (D-TEST-STOPMSG1=A). A wrong
  code, wrong message, or clean return fails the claim; a mismatch reports
  the expected text and actual code and message. Empty or non-literal text is
  E0617. A matched stop is consumed and execution continues after the region.
  This does not change the whole-test `expected_fail` known-bug mark.
- `.timeout(duration) { ... }` takes one canonical duration value. Version 1
  compares elapsed time after the region completes; it does not interrupt a
  hung body.
- `.skip { ... }` or `.skip("reason") { ... }` type-checks but does not execute
  the region. A first `.skip` skips the entire claim; a later one skips only
  its region.
- `.measure { ... }` marks the containing claim for the measurement harness.

`jet new <name>` creates a new simple project directory and refuses an existing
path. It writes `package.jet`, a print-only `run.jet` with one test claim, a
`.gitignore`, and a toolchain lock record. `--template cli|ui|web|overrides`
selects a richer starter: a typed CLI entry, a native UI tree, the browser app,
or the print-only entry plus the commented command homes
`@run.jet`/`@build.jet`/`@dev.jet`/`@test.jet` (D-NEW-TEMPLATE1); an unknown
name is E2104. `--annotated` includes commented example dependencies.
The same command family also provides `jet new service|route|job|migration` for
backend source scaffolds; those subcommands have their own `--path`, `--route`,
`--model`, SQL, risk, lock, and preview/apply options. The scaffold source is
the command implementation in [`Source/CmdCompile.rs`](../../Source/CmdCompile.rs).
(D-CLI-RECIPE1, D-ILE1, D-CLI-BARE1, D-VERDICT-678-1)

Inside a package, bare `jet run`, `jet dev`, `jet check`, `jet build`, and
`jet test` share one entry resolver: a selected command home first, then
`run.jet`, then `src/run.jet`, then `<package>.jet`. An explicit file or
directory stays explicit. In an ambiguous workspace, name the member with `-p`
or pass a path. A lone `main.jet` is moved to `run.jet` with a notice; a layout
that has both is ambiguous and fails with E2105 (D-ILE1, D-ROLEFILE1).

### Measured test claims

`#Test("name") { .measure { ... } }` is the one claim form that supplies both
correctness and performance evidence. Plain `jet test` runs every claim once;
`jet test --measure` selects only measured claims. The measurement harness
builds an optimized AOT executable, performs five warmups, calibrates the
iteration count, and records twenty serial samples. Its records identify the
execution tier, profile, warmups, iterations, and serial mode. `--filter` still
selects measured claims by name. A standalone benchmark marker and command
are not a second test surface. (D-CLAIM-BENCH1=A)

The harness sends the region's result through `black_box`, but that sink does
not keep dead intermediate work alive. Put the operation's observable result or
the approved identity sink `keep(value)` inside the measured loop.
(D-BENCH-KEEP1). The following traps can otherwise produce a measurement
of the optimizer rather than the intended work:

- a loop whose result is dropped can be eliminated;
- a lazy resource such as an mmap may not fault pages inside the region;
- loop-invariant work can be folded or hoisted; and
- the harness sink cannot make a value live after the region or force a lazy
  operation that occurs later.

The language example is
[`Examples/features/tooling/bench.jet`](../../Examples/features/tooling/bench.jet),
and the test command contract is covered by
[`tests/measurement_tiers.rs`](../../tests/measurement_tiers.rs). The separate
compiler-speed dashboard at [`Tools/perf/dashboard.sh`](../../Tools/perf/dashboard.sh)
uses its own one-warmup, twenty-sample policy and six clean/no-change/edit
production rows; [`Tools/perf/ci-perf-check.sh`](../../Tools/perf/ci-perf-check.sh)
rejects missing or mismatched corpus, identity, parity, variance, and budget
evidence. Do not use dashboard numbers as a substitute for the `.measure`
claim's execution record.

## Size-oriented builds and language tooling

### Size-oriented builds

`jet build --small` selects the size-oriented build profile. The profile uses
`opt-level=z`, fat LTO, `panic=abort`, and stripped symbols rather than the
speed-oriented defaults. The command-line help describes the flag as
“Favor a smaller binary”; it does not promise a particular byte-size ratio
(S15).

### Language server

`jet self lsp` speaks JSON-RPC over standard input and output. It uses the same
front end as checking, reads the import graph from disk, and overlays the open
buffer over those files. Diagnostics cover the complete document, and teaching
quick-fixes use `Diagnostic.edit`. Formatting is available through `jet fmt`
(I6, S14).
The server's protocol loop accepts JSON-RPC requests and emits framed
`Content-Length` responses; clients do not need a second compiler process.
The LSP integration coverage is in `tests/lsp.rs`.

The VS Code/Cursor client in `Tools/editors/vscode/` provides the TextMate grammar
and LSP transport. `install.sh` packages the extension. Server discovery uses
`jet.languageServerPath`, then `<workspaceFolder>/target/debug/jet`, then
`jet` on `PATH`; `jet self lsp` does not invoke `rustc`.

### Hand-rolled parser contracts

The standard-library-only parsers at compiler and package boundaries accept
only the published grammars. They reject unsupported or ambiguous input rather
than guessing at a partial value.

| Boundary | Accepted input | Rejection rules |
| --- | --- | --- |
| LSP/DAP JSON and framing | UTF-8 RFC 8259 null, booleans, finite numbers, strings (including valid `\\u` surrogate pairs), arrays, and objects to depth 64. Object names are unique. LSP `Content-Length` bodies are capped at 1 MiB before allocation; DAP bodies at 16 MiB; framing headers at 8 KiB and 64 fields. JSON-RPC uses `jsonrpc: "2.0"`, a string `method`, object or array `params`, and a string or signed 64-bit `id`. DAP requests use `type: "request"`, a positive `u32` `seq`, a nonempty `command`, and optional object `arguments`; breakpoint lines are positive `u32` integers. LSP positions are nonnegative `u32` integers; `jet.impact` depth is limited to 1–64. | Oversized messages or headers, duplicate `Content-Length` headers, non-UTF-8 frames, duplicate JSON names, raw string control characters, malformed or overflowing numbers, lone surrogates, deeper nesting, non-object requests, fractional IDs/positions/sequences/lines, and scalar parameters. JSON-RPC syntax errors return `-32700`; invalid envelopes return `-32600` with a null id. Malformed DAP envelopes are not dispatched, and unknown string commands receive an unsuccessful response. |
| Project configuration | `package.jet` supplies package identity, `workspace.jet` supplies workspace membership, and `env.jet` supplies named source aliases and environment facts. Each uses Jet grammar and resolves from the nearest applicable project root. | `jetpack.toml` is not a second configuration grammar; it is rejected with **E1225** before dependency resolution. |
| SemVer and dependency ranges | SemVer 2.0.0 versions and the documented comparator, caret, tilde, `x`, hyphen, whitespace-AND, and `||`-OR forms. A leading `v` is accepted for tag compatibility; an empty requirement means `*`. | Numeric overflow and leading zeroes, empty identifiers, invalid characters, wildcard-before-number forms, empty `||` alternatives, and ranges whose exclusive upper bound overflows `u64`. Pre-release numeric identifiers remain spec-unbounded and compare without integer conversion. |
| C bind prototypes | Top-level `return_type name(parameters);` declarations for the documented scalar, `char*`, and `void` subset. Empty lists and `(void)` are accepted; scalar parameters may be unnamed. Unsupported but structurally valid types are reported as skipped. | Bodies, pointers, variadics, unbalanced lists, empty comma fields, trailing declarators, non-ASCII identifiers, and declarations without a return type. No guessed binding is emitted. |
| Registry and advisory feeds | Each registry line is one UTF-8 JSON object with nonempty string `name`, `version`, `tier`, and `gate_status`, optional string `content_hash`, `fingerprint`, `public_key`, and `signature`, and optional boolean `yanked`; `tier` is `core` or `community`, and `gate_status` records the five named gate states. The offline advisory file starts with `jet-advisory-feed-v1`, then a signed `feed|sequence|issued-unix|expires-unix|maturity-seconds|key-id|public-key|signature` header, exact `release|package#version|first-seen-unix|source-class` records, `advisory|id|package|affected|fixed-or-empty|title|severity` records, and exact `exception|package#version|reason|reviewer|expires-unix` records. Package source policy may add exact, expiring `PolicyException` records under `policy.exceptions`, with `id`, `scope`, `reason`, and `expires`. `key-id` is `sha256-` plus the hash of the decoded public-key bytes. `.jet/advisory-trust` pins the publisher key, minimum sequence, accepted digest, and revoked keys. The default third-party maturity window is 24 hours; first-party and workspace releases default to zero. See [registry tiers](registry-tiers.md). | Malformed or duplicate or nested-fake JSON fields, unknown registry keys, wrong field types, partial registry records, invalid tier or gate status, unsigned, untrusted, stale, expired, rolled-back, or forked advisory feeds, compromised keys, missing remote release records, invalid exact targets, advisory field-count errors, empty required fields, invalid affected or fixed versions, malformed or expired source exceptions, or `|` inside fields. Reads fail closed with **E2607**, **E2609**, **E2610**, or **E2611** rather than skipping security metadata. |

## REPL state, authority, and editing

The REPL retains accepted statement ASTs and live `CtValue`s between turns.
Lists, maps, options, results, structs, enums, and closures remain values rather
than being rebuilt from display text. Explicit binding annotations remain
available to `:type`.

Pure Core calls run directly. Ambient calls use the same authority model as a
program: the call must be inside a matching `#FX` boundary, and the REPL must
authorize the exact operation and resource before host state changes. A
TTY prompts for once, session, or deny. A session decision is an exact tuple
and offers continue or revoke on reuse. Piped and transcript sessions never
prompt; an unflagged effect is E1803. Filesystem access stays below the project
root descriptor opened at session start. Later components open descriptor-
relative without following symlinks; a platform that cannot enforce this
confinement fails closed.

Ambient random draws require `Rand`; an explicitly seeded `RNG` is input data.
REPL-owned `print` and `eprint` capture does not need an `IO` grant. Process
execution opens the canonical executable before authorization and launches that
exact descriptor. Stdin is closed unless a separately authorized stream
surface supplies it. The child starts in the verified project directory with an
empty environment; stdout and stderr are captured. Interrupts target the
process group, which the REPL kills and reaps after 30 seconds. Native-only
features report E1802.

### Multiline input

In a raw-terminal `jet repl`, Enter submits when the parser accepts the current
item, statement, or expression. If parsing stops at the end of the buffer,
Enter inserts a newline and draws each continuation with `· `. Invalid input
that already contains a parser problem submits immediately so the compiler can
report it.

Escape followed by Enter always inserts a newline, even for a complete input.
Enter on an empty continuation line force-submits. The editor repaints the
logical buffer after insertion, deletion, history, or cursor movement and
restores the source cursor position. Cooked and non-TTY sessions use bracket
balance and the `...  ` prompt; they do not claim raw parser-aware editing
(D-FE-REPL-MULTILINE1=A).

### Interrupts

Ctrl-C during raw REPL evaluation cancels the current turn and restores the
prompt within 100 ms for Jet-controlled work. The interpreter polls before
each instruction and around runtime calls. A blocking external call follows
its own cancellation behavior and the REPL warns while it waits.

Cancellation is transactional for session state: bindings, moves, and statement
history from the interrupted turn do not commit. Host effects already completed
cannot be undone, so the REPL reports that external effects were performed.
`:turns` records the turn as `interrupted`, and the normal rerun mechanism can
replay it. A second Ctrl-C while stopping exits the REPL. Outside evaluation,
Ctrl-C clears nonempty editor input and exits from an empty prompt
(D-FE-REPL-INTERRUPT1=A).

### Semantic assistance and history

REPL documentation and completion, LSP hover and completion, and `jet ?` help
read the shared `jet-semindex` facts. A fact carries stable module/member
identity, kind, signature, summary, examples, provenance, and an optional
source span. Definitions, members, parameters, locals, imports, and aliases
retain identity, so equal spellings from different owners remain distinct.
Builtins use the same index. Command help adds search categories, flags, and
cross-links as presentation metadata.

A raw-terminal completion inserts a unique match. Multiple matches open a
selectable list: Up and Down select, Tab advances, Enter inserts, and Escape
closes. Cooked terminals and `NO_COLOR` use the same candidates with a textual
selection marker.

The REPL retains the latest 2,000 successful submissions between sessions.
Failed turns and meta-commands are not stored. History is at
`$XDG_STATE_HOME/jet/repl-history` on XDG systems or the platform state
directory elsewhere. The directory and file are owner-only. Inputs are stored
losslessly, including multiline, effectful, and secret-bearing text; Jet cannot
identify every secret.

State-path traversal rejects symlink or reparse components and keeps the opened
history directory as the authority for later reads, replacement, and erasure.
Writes and clear operations take a bounded cross-process lock, reread current
history, and atomically replace durable data where supported. Concurrent
sessions therefore merge successful submissions, and a stale session cannot
resurrect an entry removed by `:history clear`.

F3 opens interactive history search. `:history search <text>` is the textual
path, and `:history clear` erases the file. `JET_REPL_HISTORY=off` makes history
session-only; `JET_REPL_HISTORY_LIMIT=N` changes the retained bound. A corrupt
or incomplete final record is discarded while the valid prefix is preserved
with a warning. If private storage cannot be opened or written, the REPL warns
and continues with session-only history (D-FE-REPL-HISTORY1=A).

## `jet inspect expand`

`jet inspect expand` is the single transparency command for compiler-inferred
facts. It reads the ordinary checked bundle; it does not run a second analysis
or ask rustc.

```text
jet inspect expand --facts <lens> <file.jet>
jet inspect expand <file.jet>
jet inspect expand --facts inline --json <file.jet>
```

The registered lenses are `inline`, `memory`, `web`, `effects`, `layout`,
`origin`, `derive`, `templates`, and `callable-signature`.

- `inline` reports `#Inline` and `#Inline(Always)` contracts and their emitted
  Rust attributes.
- `memory` reports transitive `Mem.*` denial facts, including bounded
  `Mem.Alloc(above: N)` facts.
- `web` reports checked routes, actions, mounts, and policy.
- `effects` reports resolved function rows, direct effects, callees, and
  provenance.
- `layout` reports target-aware size, alignment, stride, and field offsets when
  the compiler has a physical layout; absent optional facts retain their reason.
- `origin` reports typed `#Track` provenance from the checked `?OriginInfo`
  projection; it adds no runtime metadata channel.
- `derive` reports behavior already attached to checked types and spans.
- `templates` reports checked `@loop` marker expansions, including `impl`,
  `#Test`, and `.measure` rows.
- `callable-signature` reports labels, local names, defaults, access modes,
  zones, types, variadics, effects, errors, returned-view provenance, identity,
  and policy chain.

The removed stored-reference field mechanism has no `refs` lens. Asking for it
is an unknown lens, not a compatibility path.

An unknown lens is E2941, lists the registered choices, and exits nonzero. A
missing lens or entry path is E2104. A source that fails the ordinary check
prints its normal diagnostics and exits nonzero. A clean program with no facts
for a requested lens exits successfully.

`--json` keeps the canonical semantic-index document and adds one `expand`
projection. The projection records the selection (`all` for the bare form),
lens name and summary, structured facts, and source locations where available.
The checked bundle and sema facts are shared with human output. Human output
retains the stable `effect roles` header and grouped lens lines including
`inline —`, `effects —`, `layout —`, and `callable-signature —`; empty lenses
are skipped in the bare form. In JSON, a usage error starts with the
`jet.status/v1` envelope and an unknown lens carries `"code":"E2941"`.

Lenses live in one static registry in `Source/CmdExpand.rs`. A future ratified
projection adds a row rather than a new subcommand or mechanism-specific flag
(D-EXPANDCLI1).

## Semantic index and codemods

`jet inspect semindex --json <file.jet>` emits semantic-index schema v20:
definitions, references, call edges, effects, member facts, and typed package
and workspace-overlay facts. Member facts unify fields, variants, inline
methods, external inherent methods, trait methods, and trait requirements under
stable owner order. Resolved references carry definition identity; unresolved
or ambiguous references carry no target. Structural `expr`, `stmt`, `item`,
and written `type` boundaries support refactoring without spelling guesses
(D-SEMINDEX1, D-WD2).

The registry exposes semantic projections through `semindex`, `output`, and
related inspect actions; it has no dossier route. Tools consume the checked
index and command/output schemas rather than reconstructing field mappings. LSP
scattered-method breadcrumbs are editor overlays with source links and do not
edit source.

The codemod commands use one replay engine:

```text
jet inspect codemod <plan.json> --dry-run
jet inspect codemod apply <plan.json> [--yes]
jet inspect codemod undo <log.json>
```

A missing `version` or `version: 1` is the semantic rename form:

```json
{"name":"RenameReport","entry":"main.jet","operation":"rename","from":"report","to":"summarize"}
```

Schema 2 is an ordered batch over typed Jet templates. `project` is relative to
the plan object. Each root is one `.jet` file or directory below `Examples/` or
`tests/ui/`; absolute, parent, and symlink escapes fail. Rules are
`symbol_rename` or `ast_rewrite` over `expr`, `stmt`, `item`, or `type` nodes.
`@value` captures one subtree and `@values...` captures a list. Matching uses
compiler-owned AST boundaries and resolved definition anchors, not token
spelling.

Every rule declares its exact match count. Duplicate IDs, unknown fields,
ambiguous names, unused captures, unresolved replacement names, zero matches,
and overlapping edits fail before writes. Rule N+1 sees a compiler-reindexed
overlay containing rule N's output.

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
their paired `.stderr` exactly. An intentional snapshot change names a
non-symlink project file in `snapshot_after`; the engine verifies the compiler-
rendered result and includes the paired stderr in the same transaction. A
code-only fixture change is refused.

Dry-run holds the codemod lock through discovery, staged compilation,
validation, input rehash, and diff output, but writes no source, snapshot, log,
temporary file, or journal. Apply requires `--yes`. Same-directory temporary
files are fsynced, parent directories are fsynced, and a recovery journal wraps
each rename. Replacement reopens parents without following links and renames
relative to the checked handle. Unix uses an OS advisory lock; Windows uses a
delete-on-close exclusive file. Concurrent byte changes stop and preserve the
journal. Schema-2 logs contain byte-exact before/after images. Undo verifies
every after-hash before writing and uses the same journal protocol. Schema-1
inverse-edit logs remain readable. Unified dry-run output marks non-newline-
terminated sides explicitly (D-CODEMOD1, D-CODEMOD-BATCH1).

## Semantic source import

`jet import LANG DIR` parses foreign constructs it can prove, emits ordinary
editable Jet, and records every other construct as a structured JT01xx omission
in `import-report.json`. Unsupported code is neither guessed nor silently
dropped.

The initial `py` subset covers annotated top-level functions over `int`,
`float`, `str`, `bool`, and `None`; straight-line local assignment, return,
calls, arithmetic/comparison/boolean expressions, and equality asserts.
Parameterless Python `test_` functions become Jet Test functions. Unsupported
imports, signatures, expressions, and nested control flow stay out of callable
Jet and appear in the omissions report with construct, reason, fix, source,
generated target, and migration status (D-MIGRATE-SRC1).

Dry-run prints the same plan without writing. A rerun is byte-idempotent.
`--update` uses the last generated baseline: untouched generated files advance,
owner-edited files remain when foreign source is unchanged, and simultaneous
edits conflict before any conflicted file is written. Directory walks are
deterministic and do not follow symlinks.

The fixed corpus under `tests/fixtures/source_import` and
`tests/source_import.rs` runs imported Jet against Python 3, compares oracle
output, pins generated bytes, proves a second import is byte-identical, and
proves that a three-way conflict writes no conflicted file. The language
boundary is published in the [migration tier map](reference/migration-tier-map.md):
Python, Java, C#, TypeScript/JavaScript, and Go use the scalar-function subset;
C and C++ use the explicit binder-plus-overlay boundary. Foreign source remains
authoritative until each JT0101 omission is resolved
(D-MIGRATE-SRC1, D-ADOPT-TIER1).

## Web development dashboard

`jet dev <file.jet> --target=web` exposes one status snapshot at
`/__jet_dev_status`. The terminal dashboard and browser corner strip render the
same status words, client count, build time, and diagnostic. Browser clients use
tab-scoped identities with a short polling lease, so the count represents live
tabs rather than transient HTTP connections.

In a TTY, a two-row header stays pinned above the scrolling log. `v` toggles
request and rebuild detail; `--verbose` starts with detail open. The scroll
region is installed only after raw input and its cleanup guard are active.
`NO_COLOR` uses a bracketed state word while retaining controls. Non-TTY output
is plain and append-only.
During a rebuild the browser dims the last good page. A failed build expands the
strip into an overlay with the verbatim front-end diagnostic while serving the
last good artifacts. Escape collapses the overlay without hiding error status;
the next clean build clears it and reloads. Failed polls and expired leases
produce a shared reconnecting state that overrides ready, building, and error
while retaining the last build time and diagnostic. A renewed lease reveals
that retained state, and recovery reloads even if a restarted server reuses a
previous numeric version (D-FE-DEVSRV1).

Revisions are numbered: `/__jet_dev_version` serves the last accepted build.
The watcher takes its baseline before the first build, so a save made the
moment the server reports ready still rebuilds, and a save is compiled only
after its content stops changing (an in-place save that briefly reads empty is
not compiled as an empty file). While a build is in flight or rejected,
`/__jet_dev_status` reports `candidate`, the revision that build would publish.
A rejected build's diagnostic carries `diagnostic_revision` (that candidate),
and `accepted_revision` and `last_good_revision` equal the revision still
served. Recovery publishes the candidate. With `--json`, every watch-cycle build
emits one `jet.status/v1` record with action `dev.rebuild`, its `jet.report/v3`
diagnostics and the same revision fields.

## Canvas visual editor

The web development server serves Canvas at `/canvas`; its versioned JSON
endpoints are also available under `/__jet_canvas`. Canvas is a projection of
checked Jet source, not a graph asset. `/__jet_canvas/graph` emits
`jet.canvas.graph` schema v1 with one function graph per checked function,
deterministic source-order layout, structural nodes, typed pins, data and
fallible wires, inline pure expressions, source byte spans, and semantic-index
handles.

`POST /__jet_canvas/transaction` accepts `jet.canvas.edit` schema v1. A
transaction carries the current source revision; stale revisions fail with a
conflict. Supported edits include rename, inline expression edit, binding
promotion, call insertion, function/signature edits, trait implementation
creation, wire break/move, source replacement, structural rail insertion,
comment/collapse regions, action preview, and no-op/reprojection. Successful
writes go through `jet fmt`, re-check through the front end, replace ordinary
`.jet` source, and reproject.

The Code lens is read-only by default. `Edit Source` enables an explicit source
editor, and `Apply Source` sends a `replace_source` transaction through the
same format/check/reproject path. Canvas never writes a graph asset and never
owns a second parser or checker.

`POST /__jet_canvas/query` accepts read-only query schema v1 for find,
references, source-to-graph, rename preview, action palette, and Core catalog
browsing. `GET /canvas/core-catalog` exposes the read-only `core.*` catalog
from the canonical Core library reference. Entries carry
`canvas.catalog:core.read` authority and `writes:"none"`; browsing never claims
that a Core call ran.

`GET /canvas/proof` reports the selected revision's front-end check, Git text
state, local debug persistence, and command receipt status. Missing or stale
build/run receipts stay missing or stale; a graph projection is never proof
that code ran. Run opens the real `jet run <source>` authority card. The
whitelisted command endpoint runs only check, run, or build cards for the
current revision and records the receipt; build output requires confirmation.
The public field contract is pinned in
[`reference/canvas-protocol.md`](reference/canvas-protocol.md), and the AST
coverage ratchet in [`reference/canvas-parity.md`](reference/canvas-parity.md).
Unknown request fields are ignored by v1; unknown operations fail as edit
errors, and unknown future graph fields cannot carry hidden semantics
(D-BPE-*).

## Public front-end toolkit

The compiler toolkit is a read-only value facade over the front end. Rust tools
use `jet::Compiler`; the public values are stable data, not AST handles or
mutable compiler state. Version 1 provides:

- `lex_source(src)`: token views with stable kinds, byte ranges, and
  line/column positions;
- `parse_source(src)`: top-level syntax summaries and diagnostics;
- `check_file(path)`: diagnostics, syntax summaries, and a semantic-index
  snapshot when checking succeeds;
- `source_map_from_generated_rust(rust)`: generated-Rust line markers mapped to
  Jet source lines.

A compile-time `CompilerChecked` value retains source text, checked function and
effect facts, optional semantic-index data, syntax, and diagnostics. The
`inspect compiler` command exposes the same operations as deterministic JSON
with `schema_version: 1` and `api_version: 1`; its operations are `lex`,
`parse`, `check`, and `source-map`. Runtime calls to `core.compiler` are E0956.

Diagnostics are value records with code, severity, message, why, fix, and span.
Semantic facts reuse the existing index schema. The API does not return
`Program`, `Item`, `Expr`, `Token`, mutable caches, parser state, or sema
internals, and modified syntax cannot be fed back into compilation
(D-FRONTENDAPI1).

## Command registry and typed inputs

The command registry is the one source for dispatch, help, manual pages,
completion, and typo suggestions. Daily commands such as `jet run`, `jet build`,
`jet test`, `jet fmt`, and `jet search` remain flat. Registry actions such as
`publish`, `keygen`, `key`, `yank`, and `vendor` use `jet registry`. Inspection
actions use `jet inspect`; among them are `semindex`, `output`, `expand`,
`schema`, `codemod`, `compiler`, `graph`, and `outdated`. A bare moved word is
E2101 and names its canonical route; it is not a compatibility alias. The
registry source is `crates/jet-cli/src/CLI.rs` and the runtime dispatch is in
`Source/` (D-SHAPE6, D-CLI-ONE1).

### Typed entry signatures

A typed entry's resolved parameter type is its CLI specification; no separate
flag DSL is required. Plain `fn run()` is the zero-input entry. A program opts
into typed input with one parameter whose type is a `#CLI` struct:

```jet
#CLI
struct ServeArgs {
    #[Doc("port to listen on"), Env("PORT")] port: Int{3000}
    #Short("v") verbose: Bool
    config: String?
}

fn run(args: ServeArgs) {
    http.serve(routes(), port: args.port)
}
```

`#CLI` is a sibling derive of `#Codable`. `#Doc` on a struct describes the
program, on a field describes its option, and on a callable member describes
its command. Descriptions preserve embedded line breaks. `#Short` and `#Env`
are field markers. `#CLI(Standard)` additionally registers
`--verbose`/`-v`, `--quiet`/`-q`, `--color=auto|always|never`, and `--version`.
Plain `#CLI` always registers `--help`.

`run` is the reserved program entry name. A typed `fn run(args: T)` is an
explicit request for command input; a `main` function has no entry meaning.
Raw argv access remains explicit through `core.args` or `core.term.args`. An
`Output.Executable` entry uses the same contract. There is no variadic typed
entry. An App-returning typed entry decodes the one argument, calls `run(args)`,
and serves the returned App; it does not change the CLI schema
(D-CLIFLAG1, D-SHAPE-CLI1).

### Field mapping

Each supported `#CLI` field maps to exactly one long flag. Required scalar
fields also fill bare positional values in declaration order unless `#Flag`
opts that field out. Named input wins over a positional value. The supported
shapes are:

| Field shape | Named form | Bare form | Missing value |
| --- | --- | --- | --- |
| `Bool` | `--name` | none | `false` |
| `?T` for a supported scalar | `--name VALUE` | none | `None` |
| scalar with `{expr}` | `--name VALUE` | none | `expr` |
| required scalar with `#Flag` | `--name VALUE` | rejected | runtime `core.args` error |
| other supported scalar | `--name VALUE` | declaration-order positional | runtime `core.args` error |

Supported scalar types are `Int`, including inline ranges, `Float`, `Bool`,
`String`, and `Path`. A map, closure, list, or nested `#CLI` struct is E1305.
Field names convert underscores to dashes. Every field accepts its named form.
`#Flag` on a bool, optional, or defaulted field is E1309. Declaration order is
part of the command interface and is represented in the checked `CLISchema`.
A field named `help` collides with generated `--help` and is E1306.

`#Short("n")` adds `-n` to the long form and must be one unique ASCII letter.
`#Env("PORT")` is read only when command input is absent. The precedence is
command input, then environment, then field default. Invalid or duplicate
short names are E1318; field markers outside a `#CLI` struct and `#Env` on a
presence-only bool are E1319. Nested `#CLI` structs are deliberately not
supported in this contract; grouped prefix decoding would require its own
shape and diagnostics (D-CLI-GLOBAL1, D-CLI-POS1).

### Program commands and Output

One `#CLI` program struct owns root flags. The same root flags are legal before
or after a command. Callable members are commands; a method receives a
read-only `self` containing parsed shared fields. A binding `name = function`
can bind an existing function whose first parameter is the program struct. The
member name is lowercased for the command word, and `#Doc` supplies its summary.
Callable members are behavior, not data: they are not serialized or exposed as
positional fields. A callable member on a `#Codable` struct is E1346. Invalid
bindings, parameters, mutable receivers, duplicate command words, and
root-command collisions use E1345, E1347, and E1344 as applicable. A program
with commands and no command token prints root help; an unknown command is an
error.

An `Output` is a closed sum with `Library`, `Executable`, `Service`, `Check`,
`Environment`, `Image`, `Bundle`, `System`, and `Fleet`. Every value has a
text `name:`. Executable, Service, and Check also have an `entry:` reference.
The reference follows normal scope, import, visibility, rename, and editor
navigation rules; it is never a string lookup and a lock cannot rescue a stale
reference:

```jet
CLI :: Output.Executable{name: "todo", entry: launch}
API :: Output.Service{name: "todo-api", entry: serve}
RELEASE :: Output.Check{name: "release", entry: verify_release}

fn launch() {}
fn serve() {}
fn verify_release() {}
```

An Executable takes zero or one supported typed CLI parameter; a Service or
Check takes no parameters. Each returns `Unit` or `Unit ?`. Sema resolves and
validates the reference before TIR or Rust emission and publishes its definition
and effect row to semantic tooling.
With no explicit selection, a legacy `fn run` wins; otherwise one compatible
Executable is selected. Multiple candidates are E1321 with a sorted list
(D-SHAPE-OUTPUT-CALLABLE1).

Code generation builds on the existing `core.args` `ArgsSpec`/`ParsedArgs`
parser. Hand-written builder use remains valid; typed CLI is a checked layer
on that one parser, not a second implementation. Runtime bad-flag messages
remain the `core.args` voice. Compile-time shape diagnostics include E1305,
E1306, E1308, E1309, and E1344–E1347.

Each compiled program carries a versioned `JetCommandSchema` in its artifact:
`.jet_command` in ELF, `.jetcmd` in PE, `__jetcmd` in Mach-O, and
`jet.command` in Wasm. Universal Mach-O slices each carry the record. The
record is part of artifact identity and is emitted before caching, packaging,
or signing. Readers use bounded section offsets and lengths; missing,
malformed, duplicate, unsupported-version, or disagreeing records fail closed.
External discovery opens one regular file and reads at most 512 MiB plus one
byte without executing it.

The external completion form is
`jet self completions <bash|zsh|fish|powershell> [--for PROGRAM]`. Without
`--for`, Jet's own completions are unchanged. With it, the reader uses only the
checked schema: root help, shared flags, command words, and the selected
command's local flags. It does not query application values and registers the
executable basename, rejecting control characters. A plain `fn run()` still
has a valid built-in-only schema. Metadata failures are E2103
(D-SHAPE-CLI-CARRIER1, D-SHAPE-CLI-COMPLETE1).

## Editions and compatibility

A package may pin an edition with `edition: "2026"` in `package.jet`. Supported
editions are `2026`, `2027`, and `2028`; a missing field uses the newest stable
edition supported by the running toolchain. A single-file `jet run file.jet`
has no edition marker and uses that newest edition. An unsupported future
edition is E2001. The durable compatibility contract for patch, minor, major,
epoch, edition, migration, deprecation, and generated-code licensing lives in
[the release policy](release-policy.md) (D-REL3, E2-V4).

## Toolchain pins and source channels

The top-level `jet:` field pins the Jet compiler channel used for a package:

```jet
name: "wordstats"
version: "0.3.1"
jet: "0.4"
```

A pin is a `MAJOR.MINOR` series, an exact `MAJOR.MINOR.PATCH`, or a named
channel such as `main`, `stable`, or `nightly`. Comparison and wildcard forms
such as `>=1.0.0` remain legacy minimum-version constraints and are not channel
pins. A malformed channel pin is E1249. If `jet:` is absent, the running
compiler is used without a fetch.

The resolved exact version is recorded in `.jet/lock` as a `[[toolchain]]`
record containing channel, version, and envelope. `jet update jet [<channel>]`
is the only operation that moves that pin; ordinary runs read the lock. A
compiler from another series realizes the pinned compiler as a prebuilt object
and re-executes it. It never source-builds a compiler for this dispatch. A
platform cache miss is E1251. Under `--offline` or CI, an unlocked channel is
E1250. `jet self toolchain` reports the selected pin and lock facts, while
`jet self update` updates a signed toolchain installation. `jet init` writes a
pin for the running channel when creating a package. This compiler pin is
separate from the native build toolchain used to compile an `extern rust`
bridge crate (D-JPK-TOOLCHAIN1, D-JPK-CACHE1, D-JPK-DISPATCH1).

Source references may carry `#latest`, `#main`, or a major-series selector such
as `#v0.x`. A missing marker is pinned; `#latest` moves only through the
package-manager update operation; `#auto` opts into automatic movement. The
lock records the exact resolved source:

```jet
rustc@nixpkgs
jq@nixpkgs#latest
omp@releases#auto
```

```toml
[[source_channel]]
name = "default"
channel = "latest"
exact = "acme/tool@github#v1.2.0"
```

`jetpack update [<source>]` is the operation that moves a source-channel
record. `jetpack outdated` compares lock facts with channel metadata without
writing. `jet build`, `jet run`, the environment view, and `jet dev` read the
exact lock entry; an unlocked channel is E1271, including in CI and `--offline`
mode (D-JPK-CHANNEL1).

### Package identity

The canonical Package identity consists of top-level `name:`, `version:`, and
`jet:` entries. A frozen-forward identity reader extracts those simple,
trimmed values before parsing the rest of the manifest. Unknown surrounding
keys and nested blocks are ignored by that identity reader so a later manifest
feature cannot prevent toolchain dispatch. The full manifest parser still
validates the complete manifest vocabulary; identity stays at the top level,
not inside a wrapper (D-JPK-TOOLCHAIN1).

### Inline package context

A single-entry `.jet` file may carry package context in one leading `package`
block:

```jet
package {
    name: "inline-demo"
    version: "0.1.0"
}

pub(package) fn helper() -> String {
    "same package"
}

fn run() {
    print(helper())
}
```

The block is structural context, not a second Package grammar. The loader
extracts its body bytes and spans and passes the body to the same
`PackageFacts` parser used for `package.jet`. The ordinary parser, formatter,
and LSP mask only those bytes, preserving offsets; they do not reimplement
Package fields. `pub(package)` retains package-scoped visibility.

Only one inline block may appear, and it must precede every other top-level
declaration. A byte-zero `#!/...` launch header may precede it. A malformed
or unbalanced block is E1362, a non-leading block is E1360, and a duplicate is
E1361. Coexistence with a project `package.jet` is E1363. The file otherwise
has normal single-file package context and does not create a synthetic
manifest (D-ECO-INLINEPACKAGE1).

### Manifest import boundaries

A manifest can narrow the declaring package's resolved import graph:

```text
boundaries: {
    deny: [{ from: "app.ui", to: "app.db" }]
}
```

`from` and `to` are quoted exact module names or names with one trailing `*`
subtree wildcard. A matching denial is E0619; a rule that matches no loaded edge
is the non-blocking L0619 warning. With no `boundaries` key, ordinary import
behavior remains. Checked edges enter the `Structure.ImportEdge` and GateLedger
facts, while manifest policy is erased before native, interpreter, and web
lowering (D-STRUCT-EDGE1).

## Inline script dependencies

A manifest-less `.jet` script may begin with an inline dependency:

```jet
// stats.jet
use textkit#1.4

fn run() {
    print(textkit.wrap(input(), width: 72))
}
```

`pkg#version` is the `#` directive-plane selector. The selector is dotted
numeric text such as `1.4` or `1.4.2`, not an operator range, and applies only
to a single-segment package name. `core.files#1.0` is not a package dependency.

`jet run stats.jet` collects inline references from the entry file and resolves
them into the module search. Resolution is local: a matching copy under
`.jet/inline-deps/<name>/<version>/` or the documented fixture override. It
never executes dependency code during resolution and does not fetch from the
public registry by name. An unresolved reference is E1253.

An exact `major.minor.patch` selector is pinned. A looser selector such as
`1.4` is accepted but emits L0203 until
`jet fetch --lock stats.jet` writes `stats.jet.lock`. The sidecar records the
script content hash, resolved versions, and content hashes; editing the script
makes the lock stale. `jet init stats.jet` lifts inline references into the
manifest's `deps` block without discarding their declarations
(D-JPK-SCRIPTDEP1).

## Sandboxed WASM packages

A package with `target: sandbox` compiles to an isolated `wasm32` Component
Model module. A native host loads and calls it across the Component boundary;
the sandbox does not require a `#Unsafe` gate. This application-sandbox world
is distinct from PATH helpers and from the compiler-extension world.

```jet
// package.jet
name: "mathkit"
version: "0.1.0"
```

```jet
// sandbox source; it is loaded, not a run entry
pub fn scale(a: Float, b: Float) -> Float {
    a * b
}
```

The sandbox build derives a WIT world from the entry file's top-level `pub fn`
surface, emits guest code, and uses the Component Model toolchain to produce a
`.wasm` component. A host uses a typed plugin handle:

```jet
use core.plugin as plugin

fn run() {
    policy :: Authority.from_rights(["FS.Read:repo"])
    mathkit :: plugin.load("mathkit.wasm", policy)
    area :: mathkit.scale(6.0, 7.0) ?? panic("scale failed")
    print("scale(6, 7) = {area}")
}
```

`plugin.load(path, authority)` exposes only frozen named exports. Sema checks
parameter and result contracts before code generation. There is no dynamic
export lookup or missing-export fallback. Exported functions use a homogeneous
`Int`, `Float`, `Bool`, or `Text` shape, or a recursively closed Component
Model shape (E1260).
The `packages/sandbox_mathkit` golden exercises `Bool` results and UTF-8 text
round trips, including a non-ASCII name; its checked-in interface snapshot
lists every named export used by the host.

The loader reads the component under explicit resource-scoped `FS.Read`
authority and preflights every declared WIT import. The guest's
`authority.needs` and the narrowed host authority must cover each import before
linking or instantiation. Unknown, wrong-shape, missing, or denied imports fail
closed before guest code runs. A guest with no declared imports has an explicit
empty capability set, never an ambient fallback. Registered imports re-check
the same typed decision at the call edge and retain resource scope. Failure is a
clean `Err`, not a host crash. Guest-local `Mem` effects support Component text
ABI values but do not grant host capabilities.

The manifest `export:` target names the interface and defaults to the package
name. The frozen public interface is keyed as `plugin__<export>` in
`.jet/cache/api/`; unchanged interfaces freeze silently, while removing or
changing an export is E1257. E1258 reports authority denial, E1259 a component
build/toolchain failure, and E1260 an unsupported export shape
(D-PLUGIN1, D-PLUGIN-EXPORT1, D-PLUGIN-VERSION1, D-DEP-WASM1, D-EMBED2).

### Embedding artifacts

Native and sandbox artifacts use one top-level `pub fn` export list and one
scalar lowering table. Each exported function uses one homogeneous `Int`,
`Float`, `Bool`, or `Text` shape; no separate `#Export` marker is required.

`jet build --lib` emits a linkable native static or shared library and a C
header. It needs no Component runtime, but the native artifact is trusted code:
the C host owns its ABI and capabilities. Text results are released with the
generated `jet_text_free` function.

`jet build --target=sandbox` emits the Component from the same list. A sandbox
host supplies a Component runtime; the guest has no ambient Jet authority, and
future host capabilities must be explicit WIT imports. The native and Component
artifacts therefore share source semantics while offering different trust and
runtime guarantees.

## Programmable builds

`jet build` checks the root program and then evaluates at most one package-local
`fn build(b: BuildContext) -> BuildPlan` through the comptime interpreter. The
entry may live in any package source file or in `package.jet`; two candidates
name both sites and fail. An explicit Output `entry:` can select a rare layout.
For a workspace, member entries run in deterministic dependency order with
separate read-only plans; the workspace entry runs last with a fresh context
and can add only workspace-owned targets. Dependency entries are checked but
not run. With no package entry, the ordinary zero-configuration battery remains
the build path.

Build code registers typed values. Targets include
`b.add_executable`, `b.add_library`, `b.add_test`, `b.add_asset_bundle`,
`b.add_doc`, `b.add_install`, `b.add_package`, and `b.add_publish`. Each returns
a `BuildTarget`. `b.action(name, inputs, outputs, argv, caps)` returns a
`BuildAction`; optional typed `BuildToolchain` and `BuildProbe` arguments add
toolchain and probe identity. `b.plan()` or `b.plan(default)` hands one
canonical graph to scheduling, caching, execution, queries, and LSP.
`b.toolchain(name, target_triple)` records target identity. `b.probe` supports
`find_program`, `pkg_config`, and `header` probes.

```jet
fn build(b: BuildContext) -[Exec, FS]> BuildPlan {
    #Impure("run declared toolchain probe and action") {
        shell :: b.probe("shell", "find_program", "sh")
        native :: b.toolchain("native", "x86_64-linux")
        stamp :: b.action(
            "stamp",
            ["assets/version.txt"],
            ["build/version.txt"],
            ["sh", "-c", "cp assets/version.txt build/version.txt"],
            ["Exec", "FS"],
            native,
            [shell]
        )
        app :: b.add_executable("app", ["main.jet"], [stamp])
        return b.plan(app)
    }
}

fn run() { print("hello") }
```

Action dependencies come from target dependencies and declared producer and
consumer edges. Ready nodes run concurrently in deterministic stages; `linker`,
`console`, `gpu`, and named resource pools serialize. Cached actions include
declared input content, argv, environment, authority, toolchain, probes,
resource pools, plugins, and generated-source hashes. A cache hit restores only
declared outputs from the local CAS.

There is no ambient execution fallback. On Linux each action runs under
bubblewrap with private mount, PID, IPC, UTS, and network namespaces. Only
declared inputs enter its writable tree and only declared outputs leave it.
Network is unshared unless both source and policy grant `Net`. A missing
sandbox is E3505, not an unsandboxed run. Build authority uses the same
`--allow=<RIGHTS>` and `--deny=<RIGHTS>` surface as other commands. Every
effectful action or probe is inside an active `#Impure("reason")` region;
signature declaration and effective grant are checked before execution.

`b.generate(name) { ... }` materializes
`.jet/generated/<package>/<name>.jet`. Action outputs ending in `.jet` follow
the same path. Built-in derives, user derives, and build entries share one
checked expansion pass. Build-only entries and imported build entries are
removed before runtime code generation, so build handles never reach rustc.

Generation is additive with one owner per managed path. Generated modules are
ordered in dependency rounds using ordinary quoted-file imports; a later round
can observe an earlier one. The round count is bounded by generated-module
count, and a cycle is E3511 before any generated file is written. An action and
generated source cannot own the same path; existing source collisions are
E3510. `--locked` checks generated input and output hashes before materializing
and records provenance after the complete runtime bundle is checked
(D-BUILDENTRY1).

### Remote execution and plugins

The remote cache and execution seam is transport-only. Cache and remote
execution require explicit policy and a complete sandbox proof binding action
key, toolchain digest, outputs, provenance, authorized builder, trust domain,
worker identity, platform, ABI, and a credential-bound worker receipt. Missing
worker or remote records are errors. `fallback_local` explicitly resumes the
same sandboxed local executor after remote failure. Timeouts write an
authenticated cancellation tombstone; late results cannot become cache records.

`jet remote bind`, `jet remote list`, and `jet remote remove` manage host
bindings; `jet build --builder <name>` selects one. Source text, ordinary flags,
and environment variables cannot create an endpoint, credential, or trust root.
Request, result, cache-record, and CAS-blob envelopes use authenticated
HMAC-SHA256 records. Blobs exist before request, result, or cache publication.
Every execution has a unique attempt ID bound into request, result, receipt,
and cancellation marker. Cancellation, publication, and reads share a
cross-process commit lock; an in-process mutex is not a correctness boundary.
Cache-only reads may use authenticated transport, but cache writes and
execution blob exchange require the host-bound worker identity.

WASM build plugins enter through the packaged manifest/component loader or the
typed in-memory test seam. The loader verifies regular non-symlink files, the
Component envelope, API version, and SHA-256 digest before application. It
checks authority per plugin and rolls back rejected contributions, so a plugin
cannot leave partial actions, targets, or generated modules. Packaged manifests
are bounded to 64 KiB and components to 64 MiB; request and response pipes are
bounded too. Production loaders reject symlinks and non-regular package files.

### Legacy wrappers

CMake, Make, Gradle, npm, and Cargo remain explicit Tier-2 wrappers. A
`LegacyWrapperSpec` parses a canonical root file into typed command, paths,
authority, environment, cache, kind, pools, and provenance. It records that
file and its bounded non-symlink source closure as typed inputs, rejects links,
oversized or non-UTF-8 files, and refuses unsupported constructs. CMake and
Gradle imports require an exact `jet: output=...` directive. npm imports require
an exact `main` or `module` entry; dependency-bearing imports fail because the
hermetic sandbox does not copy `node_modules`. No undeclared install or network
fallback is attempted. Make recipes, Gradle task bodies, unpinned Cargo
sources, non-registry Cargo sources, and unmodeled fields fail closed.
Production policy denies wrappers in CI unless a stronger host policy replaces
that default.

Fleet host overrides are typed values evaluated by the same pure comptime
engine and cycle checks as computed module fields. Results retain exact source,
dependency, and purity provenance.

### Compiler and package model APIs

`core.compiler` is a typed, read-only compiler API. `lex`, `parse`, `check`, and
`source_map` are compile-time-only and retain source, spans, diagnostics,
semantic facts, and generated-line mappings. `jet inspect compiler` mirrors
these operations as deterministic JSON with schema and API version 1. Runtime
calls are E0956.

Package-model operations use separate schema version 1 views:

| Operation | Value | Ordering and composition |
| --- | --- | --- |
| `manifest()` | `CompilerManifest` with schema, file, optional `jet`, `edition`, description, license, repository, layer, target, and dependency/package/output/profile lists | Uncomposed `package.jet`; map keys sorted, declaration lists retain model order |
| `package()` | `CompilerPackage` with the same fields and optionality | Composed package facts after Config and authority validation; it does not add current-package identity |
| `lock()` | `CompilerLock` with schema, file, version, root dependencies, and locked packages | Lock model order; package records retain source and digest facts |
| `profiles()` | `CompilerProfileSet` with schema, file, and profiles | Profile and collision keys sorted; declaration-order extends, packages, and sources retained |

Nested records are fixed: `CompilerDependency` requires `name` and `source`;
`CompilerPackageTarget` requires `name` and `targets`;
`CompilerPackageOutput` requires `name` and `kind` and may have `entry`; and
`CompilerBuildProfile` requires `name`, `optimize`, `debug_info`, and `small`
and may have `panic`. A locked package requires `name`, `version`, `source_kind`,
`fingerprint`, and `dependencies`, with optional `source`, `revision`,
`content_hash`, `layer`, and `inferred_layer`. Empty collections remain empty
lists. Optional source fields remain optional strings. Git sources redact
credentials, URL queries, and fragments. Current package identity remains
available through `$package.name` and `$package.version`, not as a
repeated field in each view.

Each operation returns a compile-time `Result`. The Rust facade reports
`PackageReadError { code, message, file, cause }`; the Jet carrier is
`CompilerPackageError` with the same four fields. Missing, malformed, changed,
or escaping files keep their logical file and typed cause; the cause carries
actionability without absolute authority paths or secret material. Reads are
restricted to the pinned package root and never become empty views.
Every consumed manifest, Config, lock, or profile is a relative hashed build
input and contributes to the cache key. The model does not invent source
positions. Runtime calls remain E0956.

The selected target and dependency closure plus generated modules form one
fresh checked runtime bundle for native, cross, web, sandbox, and freestanding
lowering. `--locked` compares generated hashes before committing provenance;
drift is E3512 and action outputs roll back. `jet inspect graph`, `jet inspect
query build`, and `jet inspect explain-build` expose the same typed graph and
cache provenance without running actions. LSP checking uses the same root
signature and static graph facts.

## JetOS plans and proofs

The `jet os` command family is:

```text
jet os check|init|plan|proof|build|switch|rollback|generations|lift|import|image|vm
```

A host name selects `system.<name>` in `./config.jet`; `host@../machines` selects
an exact external root. Builds create named generations, `generations` lists
them newest first, `switch --name <name>` selects a name explicitly, and
`rollback` activates a prior generation. `plan` prints the checked plan without
building. `proof` reads the selected generation's plan, proof, provenance,
health, boot, init, secrets, VM, and rollback facts.

A system plan can carry a package closure, services and target wants,
users/groups, filesystems and swap, network/firewall/wireless facts, boot and
kernel facts, terminal and desktop facts, storage and persistence plans,
workloads, hardware facts, profile and specialization entries, theme
projections, fleet plans, lifecycle policy, typed option records, and image
variant records. These facts are written into generation artifacts and are
available to proof and explain commands; secrets remain ciphertext or metadata,
never plaintext in generated reports.

Generated terminal profiles set `JETOS_BRAND=JetOS` and a `JetOS <host>`
prompt for login and VM run-mode shells; `/etc/issue` and `/etc/motd` carry
the same host branding. Compatibility escape hatches such as overlays and
`specialArgs` are allowed only as explicit `packages.*` options, and each is
recorded in generation compatibility audit and provenance facts.

`jet os import <flake-or-dir>` consumes semantic
`jetos-import-facts.json` input; the facts-only fallback is audited. Storage
application is safe by default and requires its explicit apply command. Fleet
deploy scripts stage over SSH, request remote proof before switching, health
check the result, and roll back on failure. Lifecycle garbage collection
explains before deletion and needs `--apply` to delete old generation
directories.

### VM proof and installed media

`jet os vm prove <host> --disk <path>` is the install and reboot proof
entrypoint. `--real` upgrades it to replacement acceptance and rejects script or
fake VM tools. Missing pinned QEMU or media tools report **E1279**; a prepared
harness without guest proof reports **E1285**. The harness records QEMU create,
install, reboot, and verifier phases and requires the installed guest to emit
a matching `JETOS_GUEST_PROOF:` marker. The installer boots with
`console=ttyS0`; its Limine entry supplies `rdinit=/jetos/init`,
`jetos.mode=install`, and the target disk. The installed-disk verifier uses
`jetos.mode=verify` and the installed root label.

The installer writes a GPT disk with a FAT ESP, an ext4 `jetos-root`,
`EFI/BOOT/BOOTX64.EFI`, the kernel and initrd, and an installed Limine config.
The proof also checks terminal-login, desktop-session, graphical-console, and
launcher readiness, including the generation's terminal facts, shell profile,
serial getty, desktop facts, display-manager unit, fallback launcher, and
installed Studio app. `jet os vm run <host> --disk <path>` opens only a disk
linked to the latest `guest-passed` proof; otherwise it reports **E1287**.

The default kernel is a first-party `cachyos-kernel` package with recipe,
configuration, patch, and initrd-input hashes beside its artifacts. Missing
kernel provenance is **E1280**; missing bootable artifact headers are **E1282**;
missing source or builder provenance is **E1284**; and a failing package
`source/build.sh` is **E1286**. When present, the package-internal build script
runs before boot validation and writes the kernel and initrd that the
installer and proof boot.

The installer initrd contains `/jetos/init`, `/jetos/install.sh`, and
`/jetos/guest-verify.sh`. `/jetos/init` dispatches `install`, `verify`, and
`desktop-verify` modes after mounting proc, dev, and sys and probing the root
label and installer disks. The hybrid ISO carries BIOS Limine files and a FAT
UEFI image so QEMU/OVMF and physical UEFI use the same installer artifact.

### JetOS Studio and VM scenarios

`jetos studio` launches the installed Studio app. `jetos studio --headless`
prints its installed path; `--json` prints root, app, metadata, and data paths;
`--serve <loopback:port>` serves the local browser fallback. `GET /studio/source`
returns the selected `config.jet`. `POST /studio/transaction` accepts source
transactions such as `set-option` and writes only with `write: true`, returning
an exact diff. `POST /studio/run` executes the selected
`jet os check|plan|build|proof|generations` action and returns captured output;
Studio does not replace CLI proof with hidden state. It remains a separate
JetOS system app from Canvas, may fall back to the browser over the same local
projection service, and may deep-link to Canvas for generic source-graph
editing without storing Canvas semantic state.

`module vmtest.<name>` declares a VM scenario. Its `hosts:` map names scenario
handles bound to `system.<host>` declarations, and `run: test { ... }` contains
assertions such as `wait_for_boot`, `assert_unit_active`, and
`assert_port_open`. `jet os vm test <name> --disk <path>` uses the same
installer/reboot proof harness as `vm prove` and writes a proof per host plus a
scenario proof artifact containing the source test, assertion method, host
generations, disks, and artifact paths.

`jetos user plan|build|switch|rollback|prove <name>` selects a `user.<name>` or
`users.<name>` generation from `config.jet` and uses the same named-generation
model. `jetos-user-apply <name>` projects declared files, links package binaries
into `.jetos/profile/bin`, writes user service units, and records a user proof
under `.jetos/proof/`.

## Deliberately absent

These are intentional boundaries, not implied promises:

- Jet does not provide a general `Any` top type. Use a precise type, a generic,
  a trait, or a closed data carrier; E0350 explains the boundary.
- Stored-borrow reference fields and their `refs` expansion lens are absent.
  Borrowed views may still be used where their checked callable contract allows.
- Nested `#CLI` structs and implicit grouped flag prefixes are not part of the
  typed CLI shape.
- The registry is not an inline-script source resolver. Manifest-less inline
  dependencies resolve from the local managed cache or fixture and fail with
  E1253 when absent.
- A sandbox guest has no ambient host capability, dynamic export lookup, or
  missing-export fallback. New host imports require an explicit contract.
- Canvas owns no graph asset and no second parser or checker; its projection is
  not execution proof.
- Build actions have no unsandboxed fallback. Remote failure can use only the
  explicit sandboxed local binding.
- Runtime effect handlers, monads, and effect values are not part of the
  language; effects are checked and erased.
- Importers never infer unsupported foreign behavior, and the command does not
  silently discard omissions. Foreign-language support outside the published
  tier boundary requires the binder or an explicit importer contract.
- Retired spellings and routes are not compatibility aliases. The command
  registry and its canonical grouped routes are the only CLI contract.
- Jet is not described here as self-hosted. The Rust-hosted compiler remains
  the reference execution path while the staged `Compiler/` work proceeds under
  its separate bootstrap gates.

For teaching diagnostics, staged parser errors should point toward the current
operator spelling—for example, `and` should teach `&&`—rather than accepting a
retired form (S14).
