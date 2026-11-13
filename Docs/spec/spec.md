# Jet language specification

This specification defines Jet's source syntax and durable language contracts. It
is for people writing Jet, reviewing a language change, or implementing another
execution tier. It describes the contract and its reasons; it is not a feature
inventory or a progress log.

[Ratified syntax decisions](syntax-decisions.md) are the rationale and decision
record for this contract. A decision ID is a citation, not an implementation
status: only ratified decisions define the language. The canonical lexical ledger
is the [syntax registry](../../Compiler/JetFoundation/Data/Syntax/), and
[`tests/decisions.rs`](../../tests/decisions.rs) checks that the ratified decision
surface and ledger agree. The parser and semantic checker are the executable
language rules; the implementation is in
[`Compiler/JetParser`](../../Compiler/JetParser/) and
[`Compiler/JetSema`](../../Compiler/JetSema/). The Prelude's Jet declarations are
under [`Compiler/Prelude`](../../Compiler/Prelude/) and
the core library under [`Core/`](../../Core/). This document states the
contract, not where it is implemented.

The [`Examples/`](../../Examples/) tree is the executable form of the examples in
this document. Prefer a feature example under `Examples/` when checking
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
  the registered forms. The number of a numbered selector (`Pad`, `PadLeft`,
  `Fixed`, `Grouped`, `Sci`, `Percent`, `Hex`) is any `Int` expression, such as
  `{name:Pad(width)}`; another type is E0112. It runs after the value, and a
  negative number acts as zero; an integer literal stays a constant
  (D-SCRIPT-LOCAL-CLOCK1=A). `{{` and `}}` produce literal braces; an unmatched
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
  D-UNIFYLIT1=A). Jet has no string prefixes: a name written against quoted
  text, such as the retired `sql"…"`, `html"…"` and `sh"…"`, is E0003, and
  the retired three name their `SQL{"…"}`, `HTML{"…"}` or `Sh{"…"}` head.
- A checked text type is any nominal with one `CheckedText` impl. A
  `distinct String` type's literal is the joined text, with each hole passed
  through `encode_hole`. A type that binds `Hole: T` and declares
  `build: fn(parts: [String], holes: [Hole]) -[]> Owner` keeps its holes as
  values: `Owner{"a {x} b"}` is `Owner.build(["a ", " b"],
  [Owner.encode_hole(x)])`, and `check` accepts the parts joined by `{}`
  before the program runs, or the literal is E2712. Such a literal adds no
  failure route to the function it is in: a failure of `encode_hole` or
  `build` stops the program at the literal, as a rejected run-time check
  does. `Hole` without `build` is E0907 (D-TEXTHEAD-HOLES1=A;
  Examples/Text/CheckedText/CheckedTextBoundHoles.jet).
- A marker is a struct whose fields are its arguments, with one
  `impl Marker { … }` block in its body. `Marker` is a Core trait
  (core.prelude) whose constants are the settings: `sites: [Site]`,
  `repeatable: Bool{false}`, `owns_menu: Bool{false}`,
  `inherits: Bool{false}`, `resolution: MarkerResolution{.SiteBound}` and
  `scopes: [MarkerScope]{[]}`. The block gives them as `name: value`
  (`sites: [...]`, `repeatable: true`, …), checked like any trait
  constants (E0933-E0937), and code reads them as `T.sites`
  (Examples/Metaprogramming/Attributes/MarkerSettings.jet). `group` and
  `companion` are declaration syntax, not constants, and a group implements
  no `Marker`. The template
  is `expand: fn(self, target: Target) { … }` (D-DECL-TYPES1=A,
  D-MARKER-SITES1, D-BOUND-SINK1=A). A trailing list field
  takes its items as separate arguments at the use site.
  `expand` runs at each site its `sites` names, and `target` is the record a
  derive reads there: a struct's or enum's TypeInfo, a field's or variant's
  FieldInfo, a method's MethodInfo, a function's FunctionInfo, an impl's
  TypeInfo, a code module's ModuleInfo (`name`, `identity`, `types`,
  `functions`, its written `markers` and its `comments`), the package's
  PackageInfo for a
  `.Package` marker, and a `.Block`'s text
  (Examples/Metaprogramming/Reflection/MarkerSites.jet). A marker on the
  package record is a header line above `package.jet`'s first field
  (`#Stamp("package")`, D-PACKAGE-MARKER1=A); it resolves in the entry
  module, is placed in `package.jet`, and what it generates joins the entry
  module (Examples/Metaprogramming/Reflection/PackageMarker). fn build's
  `b.program.packages()` holds one PackageInfo per package: its `name`,
  `identity` and `version`, the ModuleInfo of each of its file `modules`,
  all their `types` and `functions`, and its `settings`. Each SettingInfo
  names a declared setting, its `type` (a TypeId: the type's module-qualified
  `identity` and its printed `name`), its `default`, the value this build
  `selected`, and its `contributions`: one ContributionInfo per writer, least
  explicit first, with its `value`, its `source` (the SourceSpan of the
  manifest entry that writes it; a `build: fn` or `--set` writer's names no
  file and is empty), its
  `kind` (ContributionKind: `Declaration`, `Profile`, `Build` or
  `CommandLine`) and whether it is the `selected` writer. Every value is a
  PreparedValue of the setting's type: `.Int`, `.Float`, `.Bool`, `.Char`,
  `.String`, `.Enum` (a PreparedCase), `.List`, `.Struct` (a PreparedStruct)
  or `.Unit`, the cases of the compile-time value algebra. The build takes
  its settings from the built package alone, so any other package lists none
  (Examples/Metaprogramming/Reflection/PackageSettings).
  Comments, documentation and marker arguments stay distinct. A TypeInfo,
  FieldInfo, MethodInfo or FunctionInfo lists its declaration's
  `documentation`: the `///` lines written directly above it, read up past
  its markers, its `pub` or `priv` and any other comment, until a blank line
  or other code. A ModuleInfo lists every comment written in its file as
  `comments`. Each is a CommentInfo: its `text` without the `//`, `///` or
  `/* */` and one space after the opening, its `span` (a SourceSpan: the
  module's source `file` and the byte offsets `start` and `end` into its
  text), and its `kind` (CommentKind: `Line`, `Block` or `Documentation`).
  The lexer finds comments, so a `//` inside a string or a marker argument
  is text
  (Examples/Metaprogramming/Reflection/CommentsAndDocs).
  The template's row is `Meta.Generate`, which `expand` may write as
  `-[Meta.Generate]>`; a read-only lint writes `-[]>` and may reject or warn
  but writes no declaration (E0775). Reading the program needs no effect,
  and any other row entry is E0774 (D-META-LADDER1=A).
  The template holds the members it generates: on a type or impl site a
  `name: fn(…)` written directly in `expand` is a member of the marked
  type, with no nested `impl`, and on a function or block site it is a
  module item. A body-level `#if` expands only its chosen branch,
  and `#loop` writes its items once per value (D-META-BODY1=A).
  A package marker reads its target, adds checked items and rejects code;
  only Jet's Prelude markers set the checking settings `scopes`,
  `inherits`, `resolution`, `owns_menu` and `companion`, which in a
  package marker are E0936 (D-MARKER-LAW1=A).
  A fixed named group instead is a field-less marker struct whose block sets
  `group: #[Inline, Scalar]`; it has at least two members and no parameters,
  and expands through the same
  argument, site and duplicate checks as its written members. Gates, refusals,
  retired markers and other named groups cannot be hidden inside it
  (D-MARKER-GROUP1=A).
  Markers declared by the current module, or by another file of its package's
  namespace, may be bare; imported markers always
  name their local import alias in PascalCase, acronyms in caps, as in
  `#Web.Get(...)` through `use web: core.http.server` (D-MARKER-CASE1=A);
  a lowercase library part is E0087. Group members bind in the declaring
  module, not in whichever module applies the group. A public group cannot
  publish a private member (D-MARKER-MODULE1=A).
- Decimal `Int` and `Float` literals are exact arbitrary-precision values.
  A decimal `Float` has digits on both sides of `.`, with an optional `e` or
  `E` exponent. `_` may separate digits. `0x`, `0o`, and `0b` introduce
  integer literals. A base prefix without digits is E0001. Unary `-` is an
  operator, not part of a literal. A whole-number literal in an operator
  expression adopts a fixed-width peer when that peer can represent it;
  otherwise it remains `Int`. Ordinary numeric widening then applies. A typed
  or destination-owned literal is checked against that destination's range
  (D-INTLIT-WIDTH1, D-NUMLIT-PEER1).
- A local name for a number reads as its number would (D-LIT-ADAPT1=A). A
  `:` binding of a number literal with no width or type suffix, a negated
  one, or arithmetic on such literals and names keeps that literal; a use
  that refuses the name's own type reads it as the literal written there:
  `y: 2.5` passed to a `Float` parameter is the `Float` 2.5. A use its own
  type serves reads the name unchanged, and a number that does not fit the
  use is the error that number written there gets (`big: 300` read as a `U8`
  is E1003), pointing at the number. A `:=` binding takes the type of the
  first use that refuses its default, where the literal fits; a second use
  that needs another type is one E0112 naming both uses, and a later write of
  a value that type refuses is E0108 pointing at the use that decided it.
  A module constant bound to a number literal with no written type
  (`LIMIT: 200`) reads the same way at a use in its own module; a constant
  with a written type, or computed from other constants, keeps its type.
- Explicit conversion is destination-owned:
  `Target.from_source(value)`. Numeric narrowing is fallible; safe widening is
  implicit where the numeric decisions allow it. Numeric-backed distinct and
  unit types use the same source-kind names. Text interpretation is
  `Target.parse(text)`. Source-owned `to_*` conversions, casts, and a neutral
  `convert` helper are not Jet syntax (D-SHAPE-CONVERT1=A).
- Safe integer widening is one rule at every place a value meets a declared
  type: call and Core-call arguments, returns, local and place assignments,
  struct-literal fields, and list and map items. A whole number passes where
  the declared whole-number type (or the payload of a declared optional) holds
  all of its values: `Int(1..100)` and `U8` into `Int`, `I8` into `I16`,
  `Int(1..10)` into `Int(1..100)`. `U64` does not pass into `Int`, and
  narrowing is never implicit; that crossing is `Target.from_int(value)`.
- A `Duration` is a signed whole-nanosecond `i64` carrier. Construct one with
  `Duration.nanoseconds`, `.microseconds`, `.milliseconds`, `.seconds`,
  `.minutes`, or `.hours`; non-finite and out-of-range input produces
  `RangeError`. `duration.in(.Unit)` reads one whole unit, truncates toward
  zero, and cannot fail, so it is a plain `Int`; `total_in(unit)` preserves the
  fractional value. `round` supports the
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
  view without changing the instant, and `in_host_zone` views it in
  `time.host_zone()`: the zone `TZ` names, else /etc/localtime, else UTC; on
  the web target the host's `Intl` zone stands in for an unset `TZ`
  (D-SCRIPT-LOCAL-CLOCK1). The format patterns' no-pad flag `%-H %-I %-M %-S
  %-d %-m %-j` drops the leading zero. `ZonedDateTime` adds explicit DST
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
the parser and the [syntax registry](../../Compiler/JetFoundation/Data/Syntax/) own the
complete token ledger. `;` is shown only as the internal terminator `NL`, not as
source syntax.

```text
program  = { item | module-binding | script-stmt } ;
script-stmt = stmt ;
item     = func | struct | enum | trait | impl | alias | distinct | use | test
         | comptime ;
func     = "fn" ident [ type-params ] "(" [ params ] ")"
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
alias    = ident [ type-params ] "::" type NL ;
stmt     = binding | assign | if | loop | "break" | "next"
         | "return" [ expr ] | result-handler | expr ;
pattern  = ident | ".{" ident { "," ident } [ "," ".." ] "}"
         | "[" [ ident { "," ident } ] "]" ;
fence    = "<:" ( ident | expr ) { "," ( ident | expr ) } ":>" ;
assign   = place ( "=" | "+=" | "-=" | "*=" | "/=" | "/%=" | "%=" | "%%="
                 | "<<=" | ">>=" ) expr NL ;
```

A parameter's access marker is on its type (`x: &T` or `x: &&T`); receivers
write `&self` or `&&self`. A type is never written between a binding name and its
binding sigil. Types ride values (`Type{…}` or an expected-type-inferred
`{…}`), or appear on signatures and fields. The old `name: Type: value` and
`name: Type := value` forms are retired (D-BIND-BARE1=A).
View-typed parameters may add `from owner` or `from owner | other_owner`; the
names identify sibling owners in the published provenance relation.

An `alias` and a constant share the `name: …` spelling; meaning, never
casing, separates them. A declaration with type parameters, or whose right side
parses only as a type, is an alias. A bare path right side (`RNG:
random.RNG`, `MAX: other.MAX`) is resolved: it is an alias when the path
names a type and a constant otherwise, and ordinary scoping decides a name that
is both. `alias` is not a keyword (D-TYPEALIAS1, #6176).

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

The expression parser uses precedence climbing. From weakest to strongest,
the relevant operators are `||`, `&&`, comparisons, shifts, `+` and `-`,
`*`, `/`, `/%`, `%`, `%%`, unary `-` and Boolean `!`, then calls,
field/index access, identifiers, literals, and parenthesized expressions.
Bit arithmetic uses `bit_and`, `bit_or`, `bit_xor`, and `bit_not`; power
uses `pow` (D-SIGIL-ALGEBRA1=B). Method nesting states the grouping explicitly.

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

An executable entry is `run: fn`. An executable `run` has no parameters, or has
one parameter whose type is a CLI-derived program struct; it returns `()` or a
unit-fallible result. The checker gives an omitted entry
contract the default fallible `Result<(), Err>` carrier, so source may use a
plain `run: fn() { … }`. An explicit unit-fallible contract names its error,
for example `run: fn() IOError!`; a bare `!` is not a contract. Other output
kinds have their own entry contract. Notebook and REPL evaluation may construct
an explicit entry for the submitted fragment (S12, D-CLIFLAG1,
D-FAIL-EXIT1=A).

At ordinary file scope, `name: value` declares an immutable module global and
`name := value` declares a mutable module global. A mutable or `#Persist`
global's initializer is a tier-stable scalar literal; any other initializer is
E0622. An immutable global's initializer is any expression, computed while
building and stored in the program, and every read is an ordinary copy
(D-MODULE-VALUE1, D-MODULE-BUILD1). No code runs before `run: fn`: an
initializer that stops, reaches the outside world, or can't run while building
is a build error (E0643), the same on every tier, and so are initializers that
read each other in a cycle, one that reads itself included (E0338). A `name: Shared{value}`
global holds one process-wide cell: the build computes `value`, which must be
literal data, and every read shares the one cell. Only an explicit
`#run { … }` reads build
inputs. Both globals are visible to functions in the file; only the mutable
form may be assigned after initialization.
Typed scalar literals such as `I64{0}`, `I64{-1}`, and `F32{1.25}` retain their
checked scalar type in module storage (D-BIND-BARE1); the inner value must still
be literal data, not a call or arithmetic expression.
Inside a function, the same spellings are local bindings. A `#run { … }` inside a function runs
while building, before any call, so it reads the function's prepared `#run` locals but not its
run-time locals or parameters (E0640).
In an entry file whose top level holds
statements and no `run: fn`, every top-level line is the program's entry, in order: its statements
run, and its bindings other than ALL_CAPS constants are locals of that entry. A function declared
in the file may use them: the checker gives it one label-only parameter per local it names, `&`
when its body changes the local, whose default is the local itself, so each call in the file
passes the caller's binding and a call from elsewhere, such as a `#Test`, passes it by name
(`note("x", count: c)`). The function's own parameter or top-level local of the same spelling
hides the file's local. A method gets no such parameter and keeps E0107. A function also takes the
locals the functions it calls reach. A call made before a local it needs is set is E0107, naming
the function, the chain of calls and the line that sets the local. A function that changes a local
is E0227 when used as a function value and E1101 when called in a task, and an argument that
overlaps one of its extra parameters is E0204; each text names the extra parameter. Hover and
`jet inspect expand --facts callable-signature` show the extra parameters. A file with top-level
statements and a `run: fn` is E0621.
The `const` keyword is retired and is recognized only to teach its replacement
(E0146). A compile-time constant is an ordinary ALL_CAPS name, `NAME: value`,
described below.
The retired `#Known` constant, branch, and block forms teach E0377 and recover
their compile-time form. Their replacements use `NAME: #run value`,
`#if condition { … }`, and `#run { … }` (D-META-STAGE1=B,
D-PREP-SURFACE2=A). Constant and local-name repairs use the same canonical
identifier-casing algorithm as semantic diagnostics.

Names cannot shadow an existing name in the same scope (E0118), and definitions
are unique (E0105). A name that would shadow a built-in is rejected with E0106,
including a type named after a built-in type such as `Unit`, `Queue`, or `Set`;
unknown names and types are E0102/E0107 and E0119, with suggestions where the
checker has a useful candidate.
Text and byte pattern holes follow the same rule in every expression position:
`symbol == "{root}jet"` is refused if `root` already names a local, parameter,
or module constant. Use `symbol == ("{root}jet")` for interpolated equality,
or a new hole name to capture text (D-PATTERN-HOLE-NAME1=A). A reused Optional
name is not a flow refinement.

A statement fence expands one complete binding or expression statement per
entry. Multiple fences advance in lock-step. An ascending numbered range such
as `<: task1..task8 :>` creates or reuses the corresponding names. An expression
fence can also expand an ascending integer-literal range, so `<: 0..3 :>` has
four entries, while `<: (0..3) :>` keeps one range. Descending or non-literal
ranges remain one `Range` value. Expression fences accept expressions, as in
`print(<: "a", total(1, 2) :>)`; binding fences accept plain names. A fence is
not a list or destructure (D-EACH1=C, D-FENCE-GLYPH1=A, D-FENCE-RANGE1,
D-FENCE2=A).

`#Track name: value` and `#Track name := value` attach the origin fact. Read
that fact as `value.reflect().origin -> ?OriginInfo`; there is no runtime origin
projection (D-TRACK-ORIGIN1=A).

Arithmetic `+`, `-`, `*`, and the numeric division operations use the registered
numeric widening law. `%`, `%%`, `<<`, `>>` and the bit methods are integer-only.
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
the ambient stderr twin. Stdout is buffered the same way on every tier, for
`print` and for every `core.term.stdout()` write alike: a terminal stdout is
written at each line end, a pipe or file when its buffer fills, and an
explicit `flush()` writes what is pending at once (D-STDOUT-BUFFER1=A).
Stderr is never held. Pending stdout is written before any stderr write, before
every read of stdin (so all that was printed is out before the program waits
for input), before a child process starts, and when the process stops. Stdin
is read through one shared buffer (a read of up to 64 KiB), so lines,
delimiters, byte counts, keys and whole-input reads may be mixed and see each
byte once.
The explicit `core.term.print` and `core.term.eprint` operations each take one
`String`; use them for a qualified call rather than relying on ambient aliases.
`Float` prints a decimal part, so a negative whole-valued float is `-5.0`, not
`-5`. `input` is every line of the program's input, a lazy `Iter<String>`:
the lines of the files its arguments name, in order, else of standard input
(`loop line in input { … }`). `core.term.readline(prompt)` reads one line,
removes its trailing newline, and returns `String !IOError`; use `??` to
handle the error. `panic` is the
bug-stop builtin; `assert` and `assert_eq` are registered assertion builtins.
Other readable aliases are `Clock`, `Instant`, `Date`, `Duration`, `Path`,
`read_file`, `write_file`, `file_exists`, and `channel`. The comptime-gated
names are `embed_file`, `embed_bytes`, `find`, and `fetch`; `random` remains
qualified as `core.math.random` (D-NAME-ALIAS1=A, D-CORE-PRELUDE1=A).
A user declaration shadows a readable prelude alias; the language-owned built-in
names remain reserved. In member access, the nearest local value binding also
shadows an imported module alias or an implicit Core module name: `term.len()`,
`hex.bytes()`, and `time.contains(name)` use the local receiver when one exists.
An unshadowed module alias can qualify a declared enum and its variants while
retaining that enum's source-owned identity. `#NoPrelude` disables the readable
prelude aliases for that file, so use the qualified Core call instead
(D-NAME-ALIAS1=A, D-PRELUDEX1=A).

`min(a, b, ...rest)`, `max(a, b, ...rest)`, and `clamp(x, lo, hi)` are prelude
functions over one ordered type `T: Comparable`: Int and the sized integers,
Float and F32, Decimal, Duration, Instant, String, Char, or a type that
implements Comparable. `min` and `max` take two or more arguments and return
the first least or greatest one. All three order by `compare`, so a Float
follows IEEE 754 totalOrder (-NaN < -inf < ... < -0.0 < +0.0 < ... < +inf <
NaN): `max(Float.NAN, 1.0)` is NaN and `min(Float.NAN, 1.0)` is `1.0`;
`core.math.min_float` and `max_float` are the Float pair that skips a NaN
operand. `clamp` accepts reversed bounds, so
`clamp(x, 10, 0)` clamps to `0..=10`. They are ordinary Core generics
(`core.prelude`), so they evaluate at compile time (`LIMIT: max(4, CORES)`).

Automatic Core source loading and readable-prelude alias injection use identifiers
from the canonical lexer, including code inside string interpolation.
Text inside string literals, raw/foreign payloads, and comments is data, not
identifier demand; explicit imports and their dependency/private-part closure
are unchanged.

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
its type as `f: fn(x: Int{0})`. The braces hold one value of that type; an
empty `T{}`, as in `xs: [Int]{}`, is the empty value `T{}` names in an
expression, for parameter, field and trait constant defaults alike. A call-site
label binds by the public parameter
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
branches over an open subject require `else`, a closed subject's table names
every case instead (E0641), and all arm results unify (D-IF1, D-IF2, D-IF3,
D-IFGUARD1=A, D-IFDIST1=A).

`if subject == { head -> body }` compares a bare value head with the subject.
A range head `lo..hi` tests inclusive membership. The subject and both bounds
must be an ordered scalar type (`Int` or `Char`). Open `Int` and `Char` domains
still require `else`. `..=` and `step` are not arm-head syntax; an
inverted or empty band is E0316. Range heads are literals. A `distinct Int`
constraint likewise requires literal bounds because a runtime `Range` cannot
define a type (D-PATR, D-RANGE1, D-RANGE-VALUE1=A).

An arm table may be written on one line, with its arms separated by commas and
an optional trailing comma: `if lookup(k) == { .Val(n) -> n, .None -> 0,
.Err(e) -> -1 }`. The comma is layout only: compact and expanded tables share
heads, patterns, exhaustiveness, ownership checks and control flow, and the
subject is evaluated once. The formatter keeps a table on one line when it was
written on one line, holds no comment and fits the width; otherwise each arm
takes its own line and the commas go. A fallible result is handled by the same
table over its `.Ok` and `.Err` cases (`.Val`, `.None` and `.Err` for an
optional result); there is no result-only handler form (D-RESULT-BRANCH1=C).

When the subject of `if subject == { … }` is not a plain name — a call or a
field access, for example — the arms refer to it as `it`, as in
`it == .Ok(n)`.

`#Through` after a match arm's body runs the next arm's body (or the `else`
body) after it (D-JAI-SMALL1=A): `.Error -> page() #Through` then
`.Warn -> log()`. The next body runs in a scope of its own, without either
arm's pattern bindings. `#Through` on the last arm is E1126; a next body that
reads a name its own arm's pattern binds is E1127; and `#Through` on an arm
that always leaves, or in a table that gives a value, is E1128.

Inside a generic function the subject may be a type parameter, making a
type table: `if T == { Int -> … Point -> … else -> … }`. Each arm names one
type, a list type as `[U8]`, and is checked with `T` read as that type, including in its result and
its `return`s. `else` sees only `T`'s bounds and is required (E0003), because
types are an open set. Matching is exact identity (a bare `2.5` is a
`Decimal`, not a `Float`): a repeated type is an unreachable arm (E0365) and
an unknown or non-type head is E0119. `if T == Int { … }` is the one-arm test,
and `#run` on either form changes nothing. Each instance keeps only its
own arm, so no run-time test remains. Over a union value, an arm may name a
member without binding it, as in `.Int -> …` (the same test as `.Int(_)`); a
union table stays exhaustive and needs no `else` (D-TYPESWITCH1=A).

A value of a union passes where a union holding all its members is expected:
as an argument, a return value, a struct field, an assignment or a list
element, `keep: fn(cell: Int | String) { store(cell) }` hands its value to
`store: fn(cell: Int | String | Bool)`, and each member keeps its payload as
its own case of the wider union. A union with a member the expected one lacks
is E0112. Only the value widens: a `&` parameter takes exactly its union type,
and a list, optional or other container of the smaller union is not one of the
larger (D-ALT-SUBSET1=A).

A union member may be a named case, written exactly as inside an enum:
`temp: Celsius(Float) | Fahrenheit(Float)`. Inside a union, `Name(T)` is a
named case and a bare name is always a type, so named cases mix with type
members (`Kelvin(Int) | String`). `.Celsius(21.5)` builds a case where the
union is expected, and `.Celsius(c) ->` matches it and binds its payload. A
named case carries exactly one payload; it can't reuse the name of a type
member of the same union, and one name can't carry two payload types (E0363).
A repeated case with the same payload is one case. Serialization of named
cases is not promised (D-ALT-CASES1=A).

`loop` supports infinite, conditional, source, map-pair, and explicit-state
forms:

```jet
loop value in source, stride if keep(value) -> value
loop (key, value) in source -> key
loop i := init, i < limit {
    print(i)
    i += 1
}
```

The two-name form over a list or other indexed source yields the index and
the item, as in `loop (i, row) in rows`. When either name is a member of the
source's named-tuple items, the form destructures each item by name instead:
`loop (package, name) in [(package: "a", name: "b")]` binds `name` to `"b"`,
in either order. It is the tuple destructure a binding writes, so a name that
is no member is E0313 and a count that differs from the tuple's is E0315. A
map keeps its key-value pair.

A source loop may use `-> expression` or a braced value body. Each accepted
iteration yields one non-unit value. The eager result is a `List` in iteration
order; a guard or `next` omits an item. Multiple source clauses produce one flat
list. An explicitly nested collecting loop preserves nesting. Maps and sets use
explicit terminals; lazy work uses iterator adapters. In statement position,
a finite source, infinite, condition-only, or mutable-state loop may use
`-> statement`; its value is discarded. A discarded non-unit value earns the
registered lint. Bind a collecting loop with `:`, or use a write handle for an
in-place update. A value loop returns a final value only through `break value`
or `break(name, value)`; all exits unify. A collecting loop returns its partial
list and rejects payload breaks (D-COMPREHENSION1).
A collecting expression needs a finite source or explicit-state exhaustion
edge; bare infinite and condition-only headers are rejected with E0072.

`break` and `next` are legal only inside loops (E0115). A loop may have an
ordinary-name label, `outer: loop … { … }`; `break(outer)`,
`break(outer, value)`, and `next(outer)` target it from a nested loop. E0987
reports an out-of-scope label. E0988 teaches retired loop-name and `@` forms,
rejects `outer := loop`, and explains that a loop name is not a runtime value.
Normal explicit-state fallthrough and targeted `next` run the afterthought once
and retest. Source fallthrough and targeted `next` pull the stride and use the
final pull. `break`, `return`, propagated failure, and panic skip the target
afterthought; abandoning an inner loop runs no edge. Bare `next` is control only
as a complete statement or `??` fallback. `next()`, `.next()`, and `next: fn`
remain ordinary identifier uses; a value named `next` after `??` needs
parentheses: `value ?? (next)`.

In an `if` or `loop` header, `{` opens the body, so a typed literal can't
follow a comparison or `in` there: `if a > I32{0} ->` reads `{0}` as the body
and reports **E0637**. Compare with a plain number (`if a > 0 ->`), or group
the literal: `if a > (I32{0}) ->`.

## Compile-time names and staged syntax

An immutable `:` binding asks for build-time evaluation as an optimization,
and `#run` forces it (#6125). A module-level `name: value`, such as
`MAX_RETRIES: 3` or `LIMIT: 6 * 7 + 1`, is always evaluated while building
and read as `MAX_RETRIES` like a literal (D-MODULE-BUILD1). A value that reads
the outside world, or stops while building, is a build error (E0643). In a
block, an immutable `name: value` of Int arithmetic (`+`, `-`, `*`,
negation) over literals and other folded bindings holds its value from the
build on, at every optimization level; any other block value computes at run
time, where O1 and O2 may still fold it. They never fold a value whose
computation calls something the runtime answers from the running program
(a clock, the task, the process, its environment or its live and auth
stores): a build would bake its own process's answer. Nor do they fold a call
of a `#Memo` function, whose result store and `name.cache()` counts belong
to the running program. A folded value is laid
out once as read-only data, as a module constant is, rather than built part by
part at each use. `NAME: #run value` at file
scope, and `lanes: #run calculate()` in a block, where names stay
snake_case (D-SHAPE-CASE1), force evaluation of the single final expression
while building: a failure stops the build with E3010 (D-PREP-SURFACE2=A).
Names bound inside a `#run { … }` statement block stay readable after it.
A `#run` inside a larger value, such as a field of a literal or an argument,
is E0391: bind it as `name: #run value` first and use the name.
At file scope, `name := value` bindings remain runtime module globals; they do
not require `#Persist`. `#Static NAME: value` requests a
stable-address Rust static when the contract needs one, and `#Inline NAME:
value` copies the value into use sites (D-CONSTMARK1). Without a marker, an
immutable constant whose value is a number, text, or a small record or enum
case of those is copied into its use sites; any other value (a list, map, or
larger record) is built once, every read borrows that one value in place, and
a use that must own it copies only the part it takes. The retired `@NAME`
spelling teaches E0388. `#Persist name := value` additionally marks hot-reload state on a bare
binding (D-VERDICT-1308-1, D-PERSIST1).

`embed_file("path") -> String` embeds UTF-8 text, `embed_bytes("path") -> [U8]` embeds raw
bytes, and `find("glob") -> [String]` returns sorted relative paths. `find` accepts the
std-only glob forms `*`, `**`, `?`, `{a,b}`, and `[a-z]`. They are the sanctioned build-time
I/O operations inside a compile-time binding; other comptime evaluation is pure.
A path or glob is any `String` the comptime code computes: a literal, or a value such
as a path an earlier `find` returned (D-EMBED-WALK1). It resolves relative to the
embedding file, never absolute and never escaping the project with `..` (E0957, which
quotes a computed value that breaks the law). Sema judges a literal; the compiler judges
a computed value when the evaluation reaches the call, before it reads anything. Missing or unreadable
input, and non-UTF-8 input for `embed_file`, are E0955. Every embedded or matched file
records its SHA-256 in `.jet/lock` (D-CTIO1, D-CTFIND1/2, D-META-EFFECT1), and every `find`
its glob with the SHA-256 of its sorted matches, an empty match included. Each is an input of
the build identity: editing a file, or adding a file to or removing one from a glob, makes
a build that would reuse a stored binary rebuild instead, and `jet dev` rerun;
`jet build` stores no binary, as compile caches are off by default.
A Core library file embeds files beside it in the toolchain's Core tree. They
are part of the compiler, not of the program, so `.jet/lock` doesn't record them.
A path or glob spelled `toolchain:<path>` resolves against the toolchain tree the
compile takes Core from (`JET_TOOLCHAIN_ROOT`, else the nearest toolchain checkout),
held beneath that tree's root, with no built-in copy to fall back on (D-EMBED-TOOLCHAIN1).
Only the toolchain's own packages read it: the compiler workspace `Compiler/`, its
members, and the packages their path dependencies reach inside the tree. A file in any
other package, a file outside the tree, a Core file and a compile against the built-in
Core are E0945. Its `.jet/lock` row keeps the `toolchain:` name.

When a ratified feature has a staged diagnostic, unsupported syntax receives that
registered error and its milestone rather than a generic syntax failure. A spelling
with no ratified language rule is not silently accepted as a new feature: the
parser or sema may recognize a retired spelling to emit a teaching diagnostic and
its canonical fix; otherwise it is an ordinary parse or type error. Old Jet and
foreign-syntax teaching remains paused until post-Epoch 6 (D-S14-PAUSE); active
docs and fixtures use canonical syntax only.

## Data types and methods

Struct fields, enum cases, and the leaves of an enum variant group share one
separator rule: a `,` or a line break ends each entry, so `Point: struct { x:
Int, y: Int }` and `Mode: enum { Fast, Safe }` are the one-line forms of the
one-per-line bodies `jet fmt` prints. An authored `;` is E0373 there as
everywhere.

Structs and enums carry data; methods attach behavior. A struct literal is
`Type{field: value}` or an expected-type-inferred `{field: value}`. Enum values
use `Type.Variant`; labeled payloads use `Type.Variant{label: value}` rather
than call parentheses. Every labeled field appears exactly once, in any order.
Pattern tests use `==`. Optional values use `T?`,
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

Struct and enum bodies may declare methods with `fn` or `pub fn`; `pub`
exports the method, while an unqualified method remains private. A build-time
method places `#run` directly before `fn`, including `pub #run fn`.
Methods declared inside a generic struct share that struct's type-parameter
scope; Core's generic record methods use this form rather than generic `impl`
heads.

```jet
Circle: struct {
    radius: Float

    area: fn(self) -> Float {
        3.14159 * self.radius * self.radius
    }
}

impl Circle {
    unit: fn() -> Circle {
        Circle{radius: 1.0}
    }
}
```

`Type{ body }` is the one typed-literal head for every type (D-DOTCTOR3): the
body uses the type's literal notation, such as elements for `[U8]{42, 0}` and
entries for `[String:Int]{"a": 1}`. A body of exactly one expression is decided
by its type. When the expression is a value of the head type, or one that a
parameter of that type accepts, the result is that whole value:
`[Int]{[1, 2, 3].map(fn(n) { n * 2 })}` is a three-element list, `[String:Int]{m}`
is the map `m`, and `Float{rows()}` widens an `Int` with the checked rule a
`Float` parameter applies, stopping with E3010 when the number has no exact
Float. Otherwise the head's notation applies, so `[[Int]]{xs}` is a one-element
list when `xs: [Int]` (D-DOTCTOR3 amendment, 2026-09-28, #3739).

`self` is the receiver and follows the same access law as any parameter. In an
`&self` method, field assignment, compound assignment, and whole-receiver
replacement are legal. The same write in a read method is E0205. Calling a
write-receiver method needs a changeable receiver: on a `:` binding it is E0111,
on a read parameter E0205, and the error quotes the method's signature line.
Invoke an instance method as `circle.area()`, not `area(circle)`. Methods may
live in the type, in `impl Type { … }`, or as a top-level external inherent
method `Type.method: fn(self, …) { … }`; the type must be defined in the current
source module. Static methods omit `self`, as in `Circle.unit()`. A function
outside a type body or `impl`, an inline module's included, can't take `self`
(E0639): it has no receiver, so `self` would silently be an ordinary parameter.

Multiple construction shapes use distinctly named no-`self` statics such as
`Point.cartesian` and `Point.polar`. Overloading is rejected; a duplicate name
is E0105.

Fields and methods have separate namespaces (D-MEMBER-NAMESPACE1=A): a struct
may declare a field and a method with the same name, while two fields or two
methods with one name are still E0105. A member read without parentheses,
`err.cause`, is the field. A member call, `err.cause()`, selects the method,
whether it is declared in the type, in an `impl`, by a trait or as an operator
hook; only a type with no such method lets the call reach a callable field.
A method the caller cannot see (E0605) or a call whose arguments do not fit it
(E0766 and the other argument errors) reports that method's error; it never
falls through to the field. Invoke a callable field that shares a method's name
explicitly as `value.field.call(args)`. Visibility, receiver access, effects
and ownership are checked on the member the call selects, on every tier.

```jet
Counter: struct {
    value: Int
    step: fn(Int) -> Int
    step: fn(self, amount: Int) -> Int Never! { self.value + amount }
}

run: fn() {
    counter: Counter{value: 10, step: fn(n) { n * 2 }}
    print(counter.step(4))      // 14: the method
    print(counter.step.call(4)) // 8: the field
}
```

Enum `if subject == { … }` tables must be exhaustive, and they name
every case: an `else` over a closed set (an enum, `T?`, `T E!`, or a union) is
E0641, whose edit writes the cases the `else` stood for, or deletes an `else`
that can never run. A table whose arms are not all pattern tests of the subject
(a Bool or comparison arm), and a table over an open set (numbers, text,
structs), keep `else`.

Traits contain signatures and implementations contain behavior. Implement a trait
inside its type or with `impl Type.Trait { … }`; qualify a foreign type when the
module owns the implementation:

```jet
Shape: trait {
    area: fn(self) -> Float
}

impl Circle.Shape {
    area: fn(self) -> Float { 3.14159 * self.radius * self.radius }
}
```

Outside a type body, `impl Shape { … }` on a trait names no type to implement
it for and is E0301.

A module that declares a trait may implement it for a built-in scalar or Core
math type (`impl Int.Named { … }`). The module owns the trait, and under the
orphan rule the owner of either the trait or the type may write the impl. An
`impl Int.Trait` for a trait the module does not declare is E0301; wrap the
built-in type in a type the module declares instead.

An impl of a generic type may apply only under conditions on the type's
parameters (D-IMPL-COND1=A): `impl Pair<T: Priced>.Describe { … }` makes a
`Pair<T>` `Describe` only where `T` is `Priced`, and inside its methods `T` is
`Priced`. The head lists the type's parameters in their declared order, each
with its bounds (E0003 otherwise); a list with no bound at all is the plain
`impl Pair.Describe` spelled with parameters and is E0003 too. A call on a
`Pair<Int>`, or a `Pair<Int>` where a `Describe` bound is wanted, is E0905.
Inside a function with `U: Priced`, a `Pair<U>` meets the condition, because
the check reads `U`'s declared bounds. Two impls of one trait for one type
overlap and are E0908, conditions or not. A conditional impl stays outside
its type's body: a nested impl cannot state the bounds.

A trait name in type position, such as `[Shape]` or `f: fn(shape: Shape)`, means
dynamic dispatch with invisible boxing. Generic bounds use `f: fn<T: Bound>(…)`
and `Pair: struct<T> { … }`. Built-in `Printable`, `Equatable`, `Debug`,
`Comparable`, `Encode`, and `Decode` derive when every field qualifies. The
package default can deny `auto_derive` through
`policy: { lints: { deny: [auto_derive] } }`. A signed type marker opts one
trait in or out (`#Debug`, `#!Debug`), while a hand-written implementation wins.
A `Display` implementation also satisfies a `Printable` bound, and a list,
fixed list or option is `Printable` when its element is.
`#Codable` requests both codec directions; `#Encode` and `#Decode` request one
(D-AUTODERIVE1=E, D-AUTODERIVE-SYNTAX1=D).

Float and F32 are Comparable by IEEE 754 totalOrder (-NaN < -inf < ... <
-0.0 < +0.0 < ... < +inf < NaN): `<=>`, `.compare`, sorting, a list's `max` and
`min`, and a derived `compare` over a Float field all use it. The operators
`<`, `<=`, `>` and `>=` stay the IEEE comparisons, so `NaN < 1.0` is false. A
map or a function has no order, so an explicit `#Comparable` over a type that
holds one is E0905 at the marker.

A distinct type forwards the traits it names from its base (D-CAPBUNDLE1):
`#[Comparable, Printable, CodableAsBase] Score: distinct Int` orders, prints,
and encodes a `Score` exactly as its `Int`. Equality comes with every distinct
type whose base has it, and a distinct type is a map key or set element exactly
when its base is. A base without the named trait (`#Numeric` over a `String`)
is E0905 at the marker, naming the trait and the base. `jet inspect expand
--facts derive` shows the generated members as Jet source.

`#As` on one stored field lets the struct pass where that field's type is
expected, read as that field (D-JAI-SMALL1=A): with `Labeled: struct { #As at:
Point, label: String }`, a `Labeled` passes to a `Point` parameter as its `at`.
A value that already fits the expected type passes as itself. A second `#As`
field in one struct is E1129, and `#As` on a computed field is E1131.

### Encoding and validation

`Encode.encode(self) -> Dyn` and
`Decode.decode(tree: Dyn) -> Self ![FieldError]` are ordinary trait methods.
`Dyn.decode<T>()` is the public typed-dispatch path for primitive,
container, generated, and hand-written implementations. A built-in derive adds
typed Jet items beside the marked type and sends them through the same sema, IR builder,
and codegen path as hand-written members. JSON therefore compares the same
encoded bytes and decoded values for generated and hand-written codecs. A
package provider is a marker whose `expand` template holds the generated
members; the built-in `Equatable` and `Comparable` providers are Prelude markers
of the same shape. A marker from another module is written through its import
alias (`#Labels.RemoteLabel`), so no separate orphan rule applies and E2711 is
retired. The old `derive T.X { … }` declaration is not Jet syntax (D-SERDE2,
D-SERDE16, D-META-CODE1, D-META-BODY1, D-MARKER-LAW1).

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
See [`computed_field.jet`](../../Examples/Memory/ComputedField.jet)
(D-FIELDPOL1, D-FIELDMEMO1).

A pure function marked `#Memo` retains completed results by its argument tuple.
Its default bound is 128 entries; `#Memo(bound: none)` makes it unbounded.
`name.cache()` reads hits, misses, size, and bound from that same result store,
including before the first call. The shared Prelude owns eviction and counters;
emitters only marshal the checked function identity, argument key, and bound.

`Name: tag { deny: [Net] }` declares an erased dataflow fact and its policy.
`deny` is required and nonempty; `from` is optional. Direct `#Name` tags attach
to values, fields, parameters, and returns. `#Scrub(Name)` removes exactly that
tag. Tags have no methods: a method in a tag body is E0732, and using a tag
where dispatch or method attachment is expected is E0731. Tag names are
PascalCase. Prelude declares `Input`, `PII`, `Secret`, and `Credential`
(D-QUAL2, D-TAG-SURFACE1, D-CASING1).

A tag body may also write `timing: .Free | .Fixed` and `protects: .Secrecy |
.Integrity` (core.compiler.lang `TagTiming`, `TagProtects`). Left out, they are
`.Free` and `.Secrecy`; `Secret` is `.Fixed` and `Input` is `.Integrity`. A
`.Fixed` value must not decide a branch, a loop, a `&&` or `||` right side, an
index, a slice bound, a division, a shift amount, an exponent, an allocation
size, an indirect call target, a `?` or a `??` (E0737); must not be held in an
`Int` or a `Float` (E0738); and must not reach a checked `+`, `-` or `*` outside
`wrapping(...)`, `saturating(...)` or a wrapping or saturating arithmetic mode
(E0739); `wrapping_add`, `wrapping_sub` and `wrapping_mul` never stop. A
list's or a string's length is public whatever its contents carry
(D-SECRET-CT1).

A branch on a `.Fixed` value whose arms are total is masked, not refused. A
total arm only binds names, assigns to local names, and reads literals, names
and fields. It may use operators that cannot stop (bitwise operators,
comparisons, a shift by a literal, wrapping arithmetic), the bit methods, the
wrapping methods, `len()` and `is_empty()`, and nested total branches. No
arm breaks, calls, stops or writes memory, and an arm returns only as an
early return (below). Both arms run. Each local
an arm assigns keeps its old value where the arm's test fails, and an `if`
value keeps the chosen arm's value. Both are chosen with a masked select and
no jump, and the result carries the test's tag. Only fixed-width integers and
`Bool` are selected. `jet inspect decisions` lists each masked branch. A
branch with an arm that is not total stays E0737 (D-SECRET-CT1).

An early return from a branch on a `.Fixed` value is masked too. This is an
`if` with no else whose arms are total except for a last `return` of a total
value. It becomes a secret flag, and the code after it still runs. From that
return on, and through a loop that holds one, each local assignment keeps its
old value where the flag is set. Every later return gives the value of the
first return taken. So `check_fast(expected: #Secret [U8], got: [U8]) ->
#Secret Bool` reads every byte with no jump on the secret. The code an early
return skips must be total: total statements, `if`s over total code, counted
loops `for i in a..<b` with total bounds, and returns of total values. There
`xs[i]` is total when `i` counts a loop to `xs.len()`. It is also total when
`i` counts a loop to `ys.len()` and both lists are read parameters that an
earlier `if xs.len() != ys.len() -> return …` proved the same length. If the
skipped code is not total (it calls, can stop, writes memory or loops on its
own), the early return stays E0737. So does an early `break` or `continue`.

A tag inside a type counts as the value's own. The elements' tag of
`[#Secret U8]` or `[#Secret U8#4]`, an `Option`'s or `Result`'s payload tag,
a Map's value tag and a tuple field's tag reach every read out of the value,
a slice of it included. So `Option<#Secret U8>` keeps whether it holds a
value secret too: `opt ?? d` on it is E0737. A struct field's tag reaches
every read of that field. A tagged struct value (`x: #Secret Limbs`) reads
and writes the struct's declared fields, and each read carries the value's
tag. Comparing two struct values (`==`, `!=`, `<`, `>`,
`<=`, `>=` or `<=>`) runs the struct's `equal` or `compare`, which leaves at
the first field that differs. So it is E0737 when either value carries a
`.Fixed` tag or its struct type declares one on a field. The derived
`equal` and `compare` bodies are not checked themselves, and a call of a
derived method that passes a `.Fixed` value is E0737.

A `.Fixed` value keeps its meaning across calls, so the check gives the same
answer at every optimization level, before any inlining. A call that passes
one to a parameter with no `.Fixed` tag is checked as if the parameter had
the tag: the callee's body is walked with it, and through every call that
body makes. If the body would decide a branch or an early return on it,
divide by it, index with it, or do anything else above, the call is E0737 at
the argument. The report's why gives the chain of callees down to the code
that steers. A total helper, one that only combines the value with
operators that cannot stop, takes it with no tag and no report. A parameter
written with a `.Fixed` tag is checked in its own body instead. A call
through a value with no known body cannot be checked, so a `.Fixed` argument
to one is E0737. That covers a local function value and any other callee
expression. A call resolves to the function its name binds there: the
module's own, an import's or the package's, or a method of the receiver's
type. A same-named function elsewhere never stands in, so a local function
value or another type's method neither reveals like a `#Scrub` function nor
returns that function's tags. A foreign import keeps the C rules
(D-SECRET-CT1).

A call's result carries the tags of the arguments it was computed from, so a
`??`, `?` or match on the result of a call that took a `.Fixed` value decides
on that value, even when only a public length picks the outcome. Code that
refuses on public lengths checks them before the value enters, then calls a
function that cannot fail.

Core's constant-time code is written under this check. The constant-time
kit, the symmetric ciphers, MACs and KDFs, the bignum and elliptic-curve limb
operations, the TLS key schedule and the CBC record MAC take their keys and
secret operands as `#Secret` parameters. A verdict that must become public,
such as a MAC check, a discarded random draw or an authentic record's length,
leaves only through a `#Scrub(Secret)` function (D-SECRET-CT1).

The compiler keeps this check after its own lowering too. In the IR, a
fixed-width integer, `Bool` or `Char` whose type carries a `.Fixed` tag holds a
fixed bit. So does every value computed from one without a call. Before code
selection, and with the optimizer's verify mode after each of its passes, IR
Lint finds any fixed value that decides a branch, divides, sets a shift amount,
an address or an indirect call target, or meets arithmetic that can stop. Each
finding is a compiler bug (an ICE), never a user diagnostic. A routine that
reveals a fixed value (`#Scrub`) is not inlined where its result is public
(D-SECRET-CT1).

The selected instructions are checked too. For each function that holds a
fixed value, a validator reads the code selected for it on every target,
instruction by instruction. Each instruction a fixed value meets must be in
that target's table. x86-64's table holds the general-purpose instructions
on Intel's list of data operand independent timing instructions. It has no
division, no branch or stop the value decides, no memory access at a fixed
address, no string operation and no inline-asm template given the value.
wasm32's table is CT-Wasm's: integer constants, local and global access,
arithmetic without division or remainder, bit operations, shifts, rotates,
comparisons and width changes, but no branch, select, call target or
address. A finding is an ICE that names the function, the instruction and
its line (D-SECRET-CT1).

A host runs that code in fixed time only if its processor's vendor says so.
Intel's guidance does for a processor without `IA32_ARCH_CAPABILITIES`; one
with it holds only while the operating system sets DOITM, which a program
cannot read. AMD and other vendors publish no such statement, and a
WebAssembly engine picks its own instructions. So a program with fixed values
runs on any host by default: `core.sys.timing_host()` says whether the host
it runs on is confirmed, and `jet inspect decisions` prints the build's
`timing host:` row. A package that sets `policy.harden` admits only a
confirmed host: its native entry checks the host before `run` and refuses to
start anywhere else, giving the reason. A build of such a package for web or
another wasm32 target is E0744 (D-SECRET-CT1).

`#Rule` or `#[A, B]` applies a rule on the line before a declaration. Block
markers use PascalCase and parenthesized arguments when needed. An explicit empty
effect row is `-[]>`. Compile-time demand uses the prefix `@`; the retired `$`
spelling is not an alias (D-ONCE-AT1=D).

A declaration may instead write its markers inside its body, one per line, as
`#X: value`: anywhere directly in the body of a struct, enum, trait, impl,
module, or function. `#X: true` applies `#X` with no arguments, `#X: false`
is `#!X`, `#X: v` passes one name path, string, or number, and
`#X: (args)` passes any other argument list; any other value is E0943, and a
required Bool argument is written `#X: (true)` (E0943). The line is the same
marker as `#X(args)` above the declaration, checked at the same site; type
facts such as `reflect().layout` apply once the whole body is read, and a function's
value line may come before its marker lines. One declaration writes its whole
set in one place: markers above and inside together are E0942, whose fix
moves the set above. A line inside a nested block or a lambda body, and a
marker that decides how a function body is read (`#FFI`, `#Import(c)`), are
E0355. `jet fix --markers inside FILE` and `jet fix --markers above FILE` move
every declaration's set, and an editor offers both moves on the declaration
under the cursor. `jet fmt` keeps the placement and prints one simple argument
without parentheses; a module's markers above it go on its `module` line,
since a lone `#Policy(…)` line is the file's policy (D-MARKER-INBODY1,
D-MARKER-MOVE1). Markers above or inside a trait, impl, or module attach to
that declaration's own site (`.Trait`, `.Impl`, `.Module`); no Prelude marker
accepts `.Trait`, so a marker on a trait is E0355.

`#Off <stmt>` parses and type-checks one statement but emits no code in any
build. `#Off(.Release) <stmt>` type-checks in every build and is erased only in
release builds (the `release` and `hardened` profiles); the condition menu is
closed (D-OFF-RELEASE1). Two `#Off` forms on one statement are E0344. Names
introduced inside either form are scoped to that marker body.
The registered build-profile fact is `#build.profile`, read through the compiler
value `#build` (D-CANVASSTATE1, D-META-HASH1, D-META-HASH2).

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
#if #build.target.os == {
    .Linux -> …
    .MacOS -> …
    .Windows -> …
    else -> …
}
```

The switch folds before target-gating checks. Arms must cover each OS or provide
`else` (E-OSTARGET-DISPATCH-EXHAUSTIVE); the subject must be `#build.target.os`
(E-OSTARGET-BUILD-CONTEXT), and arm heads must be OS variants
(E-OSTARGET-DISPATCH-ARM) (D-OSTARGET1, D-OSTARGET2).

Inside an `@` binding, `embed_file`, `embed_bytes`, and `find` are the only
sanctioned build-time file operations. Their path, hash, and purity rules are
defined in the compile-time section above.

### Published schemas and migrations

`#PublishedSchema Name: struct { … }` marks a public record whose field layout is
snapshotted under `.jet/cache/schema/`. A later project build compares the
current shape with that snapshot by field name, ignoring order. A breaking shape
change is E0910 unless a `migration` block declares it. The four operations are:

```jet
migration UserRecord {
    rename name -> display_name
    remove legacy_id
    add verified: Bool = false
    change price: Int -> Usd via { fn(c) { Usd.from_int(c) } }
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
wire key set (after any field `#Rename` and type `#Rename(.Case)` treatment) with historical shapes
from newest to oldest, rewrites the first match forward in source order, and
decodes the current shape. The `rename` operation moves a
key, `remove` drops it, `add` evaluates its default, and `change` decodes,
converts, and re-encodes the old field; an old value that does not decode
fails the call with that field's errors (D-VALIDATE-DECODE1=B). A converter
and an `add` default are ordinary Jet expressions checked and lowered through
the normal pipeline. If no shape matches, the original decode error is
returned. Types with no migration blocks pay no chain cost, and CSV applies the
chain per row (D-MIGRATE4=A).

### Struct layout

A struct without `#Layout` is laid out by the compiler: its stored fields take
offsets by descending alignment (declaration order among equals), each at its
own alignment, and the struct rounds up to a multiple of 8 bytes, so
`{U8, Int, U8, Int, U8}` takes 24 bytes. `T.reflect().layout` reports the size,
alignment and offsets the compiler chose, with guarantee `optimized`. A C
declaration that follows them, with explicit padding fields, takes such a
struct by reference: an `#Import` C function with a `&T` parameter receives the
address of the value's own bytes when every field is a machine integer, float,
`Bool`, or such a struct.

`#Layout(c)` before a struct requests C layout and preserves field order for
foreign sharing. Growable fields (`[T]`, `[K:V]`, and `String`) are rejected with
E1104 because they have no stable C layout; fixed arrays `[T#N]` are allowed.
Reserved `packed`, `align(N)`, and `columnar` forms parse but report E1105 until
their contracts are defined (D-REPRC1).

### Enum backing

`#Layout(U8)` before a payload-free enum names the integer type that backs its
case numbers: `U8`, `I8`, `U16`, `I16`, `U32`, `I32`, `U64`, or `I64`
(D-ENUM-BACKING1=A). A case's number is its written integer literal
(`Load = 16`, `Low = -1`), or one more than the case before it; the first
case counts from 0. The enum gains `case.to_int() -> Int` and
`E.from_int(value: Int) -> E Err!`, which returns the case with that number
or an error naming the number (D-ENUM-INGRESS1=A). An enum that declares its
own `to_int` or `from_int` keeps it.

```jet
#Layout(U8)
Opcode: enum {
    Nop
    Load = 16
    Store
}

op: Opcode.from_int(17) ?? return
print(op.to_int())
```

A case with a payload is E1120, a number the backing type can't hold is E1121,
two cases with one number are E1122, and a case number that is not an integer
literal is E0035. `#Layout(U8)` on a struct is E1105; `#Layout(c, U8)` remains
the C ABI tag width of a `#Layout(c)` enum.

`#Specified` on a numbered enum (`#Layout(U8)` … or `#Layout(c)`) makes every
case state its number, so adding or moving a case renumbers no other
(D-JAI-SMALL1=A): `#[Specified, Layout(U8)] Op: enum { Add = 1, Sub = 2 }`. A
case without a number is E1124, and `#Specified` on a struct or on an enum
whose cases have no numbers is E1125.

A `Set<E>` over an enum of 1 to 64 cases with no payloads and no type
parameters is stored as one `U64` word (D-JAI-SMALL1=A). Case i is bit i in
declaration order; a `#Specified` enum's cases are their own values, so C
flags cross unchanged, and a value that is not one bit is E1132.
`perms.bits()` reads the word (`Set.from([Perm.Read, Perm.Write]).bits() ==
3`), and `Set<Perm>.from_bits(n)` gives `Set<Perm>?`, None when `n` has a bit
no case stands for. Either on a Set that is not stored as bits is E1133.
Set's other methods are the same on every element type.

## Fixed-size lists

`[T#N]` refines a list to exactly `N` elements of type `T`. Destructuring must
use exactly `N` names. When `N` is a literal and `T` is plain data (`Bool`,
`Char`, `Float`, `Float32`, a fixed-width integer, or a struct, tuple or fixed
list of those), code generation stores the `N` elements inline, with no heap
storage; any other `[T#N]` has the list representation of `[T]` (S76). Either
way a fixed list is a value: assignment and a by-value parameter copy its
elements, and a `&` parameter writes the caller's.

```ebnf
type_fixed_list = "[" type "#" int_literal "]" ;
```

```jet
result: [Int#3]{2, 4, 6}
[a, b, c]: result
```

`[T#N].filled(value)` is a fixed list of `N` copies of `value`, which is
evaluated once: `slots: [Int?#3].filled(None)`. A list type with no length
has no static methods. `[Int?].filled(3, None)` is **E0311**, and the fix is
the list literal `[Int?]{…}` or the fixed list `[Int?#N].filled(value)`.

A wrong destructuring count is **E0963**. `push`, `pop`, `insert`, `remove`,
and `clear` on a fixed-size list are **E0964**. A literal index outside
`0..N-1` is **E0965**. A proven `distinct Int(lo..hi)` or sized-integer
interval may index without a runtime bounds check when `lo >= 0` and `hi < N`
(D-TYPE2-REFINE1).
An immutable integer name whose interval is known keeps, after a guard of the
form `if i >= N -> return` (also `>`, `<`, `<=`, the mirrored forms and
`a || b`, with an integer literal or a fixed-size list's `.len()` as the
bound, and any exit: `return`, `break`, `next`, a diverging call), only the
part of its interval that the guard lets through, for the rest of the block:

```jet
channel: Int.from_u8(byte)
if channel >= channels.len() -> return Err("bad channel")
channels[channel] += U64{1}
```
Finally, `[T#N]` widens to `[T]`; the length fact is erased at that coercion.

### Number parameters

A type or function may take a compile-time `Int` as a generic parameter,
declared `<N: Int>`: a parameter bounded by exactly the core `Int` is a
number, not a type. Each number is its own instance: `Packet<4>` and
`Packet<2>` are two types, laid out and named apart. An argument is an
integer literal or a constant (`Packet<SIZE>`), or a number parameter in
scope (`Packet<N>`). Inside the declaration, `[T#N]` takes the instance's
length and `N` reads as an `Int`; an index into such a list is
bounds-checked at run time, per instance. Methods and trait impls of the
type resolve on every instance (`Inline<2>` meets a `Store` bound its
`impl Store` declares).
A call infers `N` from its arguments' types (D-CONSTGEN1, D-CONSTGEN2,
D-META-SPELL1).

```jet
Packet: struct<N: Int> {
    data: [U8#N]
}

size_of: fn<N: Int>(packet: Packet<N>) -> Int { N }
```

Only `Int` makes a number parameter; any other bound declares a type
parameter.

## Closures and function values

A lambda is `fn(params) { body }`: the `fn` keyword, its parameters and a
braced body whose last value is the result. Parameter annotations may be
omitted when an expected function type supplies them; with no expected type,
omitted annotations report **E0801** (S46).
A locally inferred value lambda keeps the failure contract proved by its own
body; a body with no failure has a `Never` failure rail. A same-named file
function cannot supply the local binding's call or failure contract.
Name resolution is lexical for calls as well as value reads: a non-callable
local also shadows a same-named file function and calling it reports **E0803**.
Use an explicit module-qualified call to select a declared function while
keeping a same-named local value in scope.

```jet
square: fn(n: Int) { n * n }
increment: fn(n: Int) {
    n + 1
}
```

`->` is the callable and control arrow. A named function or method has exactly
one arrow after its inputs, `->` or an effect ceiling `-[Effect]>` / `-[]>`,
followed by its optional result type and then a braced body:
`f: fn() -> T { expr }`. A unit or unit-fallible callable keeps a bare braced
body. A second body arrow (`f: fn() -> T -> expr`) or an unbraced body after the
arrow reports **E0080**; `jet fmt` rewrites it to `{ expr }` (D-SIG-AFTER1=A).

Function values use a function type with parameter types and a result:
`fn(T1, T2) -> R`. A result may be omitted for a unit callback. A pure
function-value effect bound is written `fn(T1, T2) -[]> R`; other effect sets
replace the empty set. Unmarked parameters have plain read access. A named
function becomes a value only when every parameter can be called with that read
access; parameters requiring `&` or `&&` keep the function direct-call-only
(**E0227**; S47, D-MEM-PARAM1).

The same structural callable-value comparison applies inside collection and
aggregate slots: a named function's declaration call metadata does not make
`[String:fn(String) -> String]` a different value type. Required public labels,
parameter zones, effects, and returned-view provenance remain directional
obligations at every container depth; storage never erases those requirements.

Whether a function type's functions can fail is part of its value type wherever
it appears inside another type. A single function value converts between its
failing and non-failing forms where it is passed, stored or returned: a
function-typed parameter `f: fn(Int) -> Int` takes the implicit `Err`
contract, and a named function of either form fills it. A converted value
passes each argument on the same way, so a non-failing function argument
widens into the failing form the value's parameter takes: `read: fn(f: fn(Int)
-> Int) -> Int` fills a `fn(fn(Int) -> Int) -> Int` field. Nothing narrows a
failing argument for a parameter that cannot fail. The functions a list, map,
optional or tuple holds, or a function returns, keep the form they were made
with, so `[fn(Int) -> Int]` and `[fn(Int) -> Int Err!]` are different value
types, and passing one where the other is required reports **E0638**. A
function type nested in a parameter's type keeps its written contract, as the
same type does on a binding or a typed literal (#6281 c123).

Every call uses plain parentheses: named functions, methods, fields, indexes,
lambdas and function-valued expressions alike, so `make_adder(1)(2)` calls the
function `make_adder(1)` returns. Calling a function value through `.call(…)`
is **E0802**, and its safe fix removes `.call`. The one place `.call(…)`
remains is a function-typed field that shares its name with a method of the
same type: `value.step(…)` selects the method and `value.step.call(…)` the
field (D-CALLVALUE2=A, scoped by D-MEMBER-NAMESPACE1=A). The adjacent `)(`
spelling once reported as E-CALL-VALUE is the ordinary call; that code is
retired.

```jet
apply: fn(f: fn(Int) -> Int, n: Int) -> Int {
    f(n)
}

make_adder: fn(base: Int) -> fn(Int) -[]> Int {
    (n: Int) -> base + n
}
```

### Captures and escaping lambdas

A lambda captures a name for shared read access when the body only reads it.
Writing a captured name requires a mutable `:=` binding; otherwise the compiler
reports **E0111**. A lambda bound to a local that is only ever called directly
in that scope does not escape: it borrows each `:=` local it writes, so the
write lands on the owner (`count := 0; bump: (n: Int) -> { count += n };
bump(2)` leaves `count` at 2). An escaping lambda—one stored in a binding that
is passed, stored, or returned, returned itself, stored in a struct field, or
passed to a `&&T` parameter—must own its captures.
Copy values are copied at closure creation; other clonable values are cloned;
an owned non-clonable value moves. A borrowed non-clonable parameter cannot
escape (**E0120**), and a lambda a function returns cannot write that
function's write parameter (**E0120**): the caller lent it only until the call
returns. A lambda that runs inside the call, or is stored and passed down to a
callee, may write it. The old `take(...)` prefix is rejected with
**E0057**. Self-recursion through the lambda's own binding is **E0804**;
calling a non-function is **E0803** (S47).
[`ClosureCaptureModes`](../../Examples/Basics/Functions/ClosureCaptureModes.jet)
shows each capture mode with its output.

A lambda whose body moves a captured owner (`name.move()`, or an argument to
a `&&T` parameter) gives that owner away when it runs, so it may run only
once. A `task` body, a lambda called where it is written, and a lambda bound
to a local that only direct calls reach run once per evaluation, and a second
direct call of that binding is **E0121**. Any other such lambda—passed to a
call, stored, or returned—may be called again, so the move is **E0121** at
the move: pass a copy (`name.copy()`), or take the value as a parameter of the
lambda instead of capturing it. An unmarked move of a clonable name there
copies instead, as it does in a loop body.

Function values expose only read parameters when coerced from a named
function. This rule prevents the value conversion from erasing a write or
move requirement. The same capture and call checks apply to interpretation
and generated programs.

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

Collection result builders cross a native kernel as host callbacks while
retaining the checked Jet parameter conventions. A split builder receives
owned halves; map conflict builders borrow the key and receive owned values.
Static and receiver routes use the same adapter.

`first()` is the consuming positional terminal. Use `skip(n).first()` for a
zero-based selection; an out-of-range selection returns `None`. There is no
`nth` adapter. `min_by` and `max_by` retain the last source item when keys
compare equal. The examples are `Examples/Basics/Functions/Closures.jet`,
`Examples/Basics/Functions/Callbacks.jet`, and
`Examples/Collections/Iterators/IterAdapters.jet`
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
contract implicit. The entry function `run: fn()` has the default fallible
`Result<(), Err>` carrier, so it may propagate a standard-library failure
without writing a return annotation. Pin an entry to an application error with
`run: fn() !StoreError { ... }` when that distinction is part of the interface.
(D-FAILURE-FOUNDATION1, D-FAIL-EXIT1)

Construct the two sides with `Ok(value)` and `Err(error)`. These are contextual
calls: a user declaration named `Ok` or `Err` takes precedence. `Ok()` is
the unit success value when the expected result has unit success, and its only
spelling: there `Ok` and `.Ok` take no argument (E0104). Leading-dot
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
    convert: fn(error: DiskError) -> StoreError {
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

A conversion from a generic error type names the type bare, as an impl of a
generic type does (`impl Unsent -> Err`), and applies to every instantiation:
an `Unsent<Int>` and an `Unsent<String>` failure both convert through it. It is
declared in the file that declares the type.

A failure whose error type is a member of the enclosing error union widens
into that union without a declared conversion. So does a failure whose error
type is itself a union, when every one of its members is a member of the
enclosing union: each member keeps its own variant in the wider union. A
failure with any member outside the enclosing union needs a conversion.
(D-UNIONTYPE1=A)

A fallible call propagates its failure automatically: success continues with
its payload and failure returns from the current fallible context. A missing
value never leaves a function by itself: it stays an ordinary `T?` value until
the code handles it with `?.`, `??`, or a `.Val`/`.None` pattern, and reading a
field of it directly is E0310 (D-OUTCOME-SHAPE1=A). The postfix form `?(text)`
is not a second propagation operator; it adds one failure-context frame to the
report. Use it when a boundary needs a local explanation. (S7, D-FAIL-CTX1)

A `T Never!` call supplies `T` in an ordinary value position, including a
source field default evaluated by a generated codec. The codec checks the
default's payload type, not `Result<T, Never>`; an explicit result slot still
retains the carrier. A value arm's expected return type does not apply to
a loop source or an indexed assignment's owner and key.

`??` is the fallback operator for an optional or fallible value. It yields the
success payload or evaluates its right side. Its precedence is looser than
`&&` and `||`, and the right side may be a value, `return`, `return expr`, or
`panic(...)`. `??` is the only fallback spelling; it must produce the
enclosing success type, otherwise the compiler reports E0405. (S35, S71)

A chain of fallbacks groups from the right: `a ?? b ?? c` means
`a ?? (b ?? c)`. Each fallback is tried in turn and the last one ends the
chain; parentheses override the grouping, and the formatter writes chains
without them. (D-FALLBACK-CHAIN1=A)

`panic(message)`, failed `assert`/`assert_eq`, `#Todo`, a raw Prelude panic, and
scheduler, stream, or foreign-boundary stops enter the shared runtime report
boundary. Program-side runtime breaches use E3001 and the process-stop boundary
uses exit code 70; an explicitly requested process exit keeps its requested
code. An unhandled error returned by the entry function is rendered as a full
report and exits 1. E3002 records the trail of `?(text)` frames, while E3003
reports an expired wait or I/O deadline. See [diagnostics.md](diagnostics.md)
for the complete report shape and fixes. (S36)

`#Todo` reports E3011 with its source file, source line, and the checked Jet
type expected at that expression. Native adapters pass those facts to the
shared stop renderer; backend carrier names are not source type names.

A function diverges when no path of its body returns to its caller: every
path ends in `panic`, `#Todo`, a `loop` with no `break`, or a call of a
function that diverges. The compiler decides this once per bundle as a least
fixed point, so a public function that only calls a private helper that
panics diverges too, while functions that only call each other do not. A
public diverging function whose return type does not say `Never` gets the
advisory **L2421** (D-NEVER1=C, D-NEVER2=B).

### Cleanup at a process stop

An explicit stop runs active cleanup before the process ends. `defer close(...)`
actions run in reverse declaration order, `scope.guard` closures run in reverse
registration order, and `os.atexit` handlers run in registration order after
scope cleanup. Work registered after the stop does not run. A host kill or an
abort does not promise these finalizers.
The ordinary process root follows the same rule for a runtime stop: lexical
`Close` and deferred cleanup run before the report is delivered with exit 70.
A non-unwinding native call boundary transports the stop to its caller for
cleanup, without exposing a successful return value.
If the first stop happens in an exit handler, the remaining handlers already
registered before that stop are still owed cleanup, in registration order.
Handlers registered after the stop do not run; their adopted captures are
released inside stopped cleanup. A second panic during stopped cleanup is
process-fatal: its own Stop report prints, no further cleanup runs, and the
process exits 70.

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

An infallible unit call written with no return arrow and `Never!` also has
no result to lose: its checked `Result<Unit, Never>` carrier is classified by
its `Unit` success value. This does not expose effects intentionally hidden
by an audited `#Unsafe` block or change the callee's public effect row.

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
Codable, AOT, and interpreter lowering (D-QUAL3, D-QUANTITY-DECL1).

Dimension identities retain raw UTF-8 axis names. Their percent escapes protect
only the separators `%`, `;`, and `:`; decoding does not reinterpret non-ASCII
UTF-8 bytes as separate characters.

## Ownership and borrowing

Jet has one explicit ownership contract. A parameter without an access marker is
always a read parameter; sema decides this from the signature and never raises
it to write or take access from body usage. The public surface is:

| Source type | Access | Call site |
| --- | --- | --- |
| `T` | read; the callee cannot mutate the caller's value | plain |
| `&T` | exclusive write/edit access | plain; needs a `:=` binding |
| `&&T` | take; ownership moves to the callee | plain; the name is gone after the call |

Calls carry no marks (D-CALLSITE-MARK2=A): the signature decides. For
non-scalar values, a read call borrows the existing value without an
allocation. A body write through an unmarked parameter, or passing it to a
write or take position, is a hard error at the definition or call site; add
`&` or `&&` to the contract. A cloneable read value entering an owning
destination is materialized automatically. A bare `:` binding of a place
remains a read window; non-cloneable values and `#Policy(copies: .Explicit)`
require an explicit `.copy()` or an owning `&&` contract (D-MEM1,
D-MEM-COPYSEM1).

Move facts follow executable control-flow paths. A loop repeats a move only
when that path reaches its back edge (including `next`); a `return` or `break`
after a move ends that path instead. Assigning a fresh whole value clears the
old move fact before another iteration. A move completed before the loop, or
while evaluating its one-time collection header, is not repeated by the loop
body. An owned pattern table such as
`if owned.move() == { .Object(entries) -> … }` consumes its subject once and binds
each arm's payload from that already-checked subject, not by moving it again.
This includes tables used as value tails: testing the next arm does not
consume the original subject a second time.
Owning `Dyn` and `DBValue` constructors accept explicit payload transfers,
for example `Dyn.Text(text.move())`; the payload stays moved and is not copied.

Public Jet APIs return newly computed answers (D-OUTPUT-RETURNS1=A). Use one
value, tuple, or named struct for multiple results, including trait requirements
and callback contracts. Do not make callers initialize a dummy `&T` merely to
receive an answer. This is an API-review rule, not a syntax prohibition.
Keep `&` for genuine edits, stateful handles, accumulation, reusable capacity,
checked byte readers and fixed storage, streams/writers, and raw foreign
signatures. A buffer's capacity can be an input even when its prior bytes are
not. Private helpers and compiler-generated machine destinations remain internal
choices; source returns do not guarantee copy elision or allocation avoidance.
Those performance claims require exercised proof.

```jet
bump: fn(n: &Int) { n += 1 }
archive: fn(name: &&String) -> String { name }

run: fn() {
    score := 41
    bump(score)
    saved: archive("vault")
    print(saved)
}
```

The sigil on a method receiver is written at its definition. Plain `self` is a
read receiver; `&self` is a write receiver; `&&self` consumes the receiver.

```jet
Player: struct {
    hp: Int
}

impl Player {
    show: fn(self) -> Int { self.hp }
    heal: fn(&self, amount: Int) { self.hp = self.hp + amount }
}

run: fn() {
    player := Player{hp: 10}
    player.heal(2)
    print(player.show())
}
```

Writing through a read receiver is E0205 and points to `&self`. Calling a
`&self` method needs a changeable receiver: a `:` binding is E0111 and a read
parameter is E0205, each quoting the method's signature line.
Calls carry no marks (D-CALLSITE-MARK2=A): the signature says what a call does
to each input, so `bump(score)`, `items.push(3)` and `buf.seal()` are written
plain. Using the same name twice while a write access is live is E0204: pass
`x` once, or materialize `x.copy()` first. A write argument writes only once
the call runs, so a later argument may read `x` when that read ends with its
own evaluation (`grow(xs, xs.len())`, like `xs.push(xs.len())`, or a copied
scalar in `add(n, n)`); a later argument the call keeps holding, moves, or
writes is still E0204, and so is `x` passed on to a nested call
(`grow(xs, f(xs))`), whose parameter may keep or take it. A part of a list
passed to a writing parameter is a write window by itself
(`sort_part(values[0..2])`). A write window bound to a name keeps its mark:
`edit: &values[2..3]`.

### Copies, moves, and owned destinations

A named binding passed to a take parameter moves into the call
(D-COPY-DEFAULT1=A, D-CALLSITE-MARK2=A); any later use on any path, or a later
iteration of a loop the name outlives, is E0209, which names the call and
teaches `x.copy()` or a reorder. Jet never hides a clone to make that call
work. A name in two argument slots of one call has no last use in that call,
so a take slot is E0209 there. A field passed to a take parameter is written
`owner.field.move()`, so a partial move is always visible. One call that reads
a name through one argument and takes it through another is E0204. A captured
name passed to a take parameter inside a lambda body moves `x` into the
lambda: `x` is gone after the lambda is made, and a second call of the lambda
binding on any path is E0121.
A literal, `x.copy()`, or call result is a temporary and
may enter a take parameter without another marker. A temporary with cleanup
lent to a read parameter (`show(open_log("x"))`) closes exactly once, when its
statement ends; one made in a branch or a loop condition of the statement
closes right after the call that reads it. The result of a call that returns a
view (`-> Directory from self | picked`) is borrowed, not owned: passed on,
bound with `:`, wrapped in a carrier, or taken apart, it closes nothing, and
its owner closes it. `x.copy()` is the one copy spelling
(D-COPY-WORD1=A); it is a compiler-owned method that no type can redefine, and
it creates an independent owned value. `.clone()` is not user-typable Jet
syntax. Copying a value that Jet cannot
duplicate, such as a function or trait value, is E0211. Copying a scalar is
legal but redundant.
The Core-only `IOOperation` carrier is a payload-free runtime `Copy` enum;
its explicit `.copy()` has no allocation and does not make the type a user export.

```jet
name: "vault"
saved: name.copy()
print(name)
print(saved)
```
See [`copy_verb.jet`](../../Examples/Memory/CopyVerb.jet) for the
explicit-copy form.

The compiler chooses when a name is unmarked, and `.move()` and `.copy()` give
exact control (D-COPY-DEFAULT1=A, D-COPY-MOVE-SPELL1=A). `ys: xs` reads a place
through a view, `ys := xs.copy()` makes an independent copy, and
`ys := xs.move()` moves `xs` exactly: `.move()` is accepted in every value
position (binding, assignment, field initializer, collection element, result),
and any later use of `xs` is E0121 naming the move. `.move()` below a collection
element (`items[i].tags.move()`, whose copy is spelled `items[i].tags.copy()`)
is E0225; on a read or write parameter it is E0201. A call argument that takes a
resource-holding field out of a write parameter or window (`next(self.r.move())`)
is E0211, as the unmarked `next(self.r)` is: a stop inside the callee before the
field is refilled would close the resource twice. An unmarked use that is not
the last one shares the value instead of moving it: the source stays usable and
each holder has its own value, so a write through one never shows in the other.
A use in `return`, or in a loop's `break` value, is the last use for the loops
it leaves. Values that cannot be copied (resources, one-pass iterators, consume
duties, tasks) keep E0121 on the later use. A call is always written plain: its
signature's `&` or `&&` decides whether it writes or takes. Under
`#Policy(copies: .Explicit)`, every implicit copy that `jet audit copies`
reports is a diagnostic with a Safe `.copy()` edit, and an unmarked move that a
later use reaches stays E0121.
See [`view_move_copy.jet`](../../Examples/Memory/Views/ViewMoveCopy.jet)
and [`share_on_reuse.jet`](../../Examples/Memory/Shared/ShareOnReuse.jet).

After a partial move from an owned local, assigning a replacement to that
field initializes that field and its descendants again; it does not restore a
moved ancestor or an unrelated moved field.

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
Constructing a struct prefixes each contained view's output path with its field
name, including nested structs and views already bound to local names. This
does not turn a local owner into a caller-owned one: returning such an aggregate
still rejects views into values made here or consumed by a `&&` parameter.
A positional enum constructor preserves the payload's access: with
`edit: &values[0..1]`, `Choice.First(edit)` fills a declared `ViewMut<Int>`
payload with that write window. It cannot grant write access to a read-only
parameter or turn an unmarked owned slice into a write view.
A name bound to one map value (`w: &m[key]`) reads and writes as that value.
Where a `ViewMut<V>` of the value's type is expected (a return, a parameter),
it is a window over that one value, used as `w[0]` like the one-element list
window `&xs[i..i]`. A `slot: fn(m: &[String: [Int]], key: String) ->
ViewMut<[Int]> from m` whose body binds `w: &m[key]` and returns `w` lets the
caller edit the entry by `s[0].push(1)`. A missing key stops as `m[key]` does,
and while the window lives the map cannot gain or lose entries (E0220, E0212).
At a string boundary, fill `View<str>` only from `.trim()`, `.after()`,
`.before()`, or a tracked string-view binding; a plain owned `String` is not a
borrowed window.

An ordinary owned field owns its value:

```jet
Span: struct {
    text: String
    meta: String
}

describe: fn(source: String, kind: String) {
    value: Span{text: source, meta: kind}
    print(value.text)
}
```

Use `Shared<T>` or `Pool<T>`/`ID<T>` when many owners need one value. A plain
owned `String` is not a borrowed window. A read-only view entering an owning
slot is materialized as an owned copy, as though `.copy()` had been written. This
covers bindings, returns, fields, collection elements, enum payloads, fallback
values, and stored read-only captures. Declared `View<T>` and `ViewMut<T>`
boundaries retain their provenance; `ViewMut<T>` is never copied implicitly.
An imported function's declared `from` contract follows its module-alias call
too: an eager comparator may read that returned window without taking it away
from the caller.
`#Policy(copies: .Explicit)` makes read-only materialization explicit. The same
rule is shared by AOT, interpreter, comptime, and web lowering; each engine
marshals the sema-approved result.

A place is a name followed by its maximal field, index, or range projection. A
bare place is a checked read window; `&place` is the exclusive write window; and
`place.copy()` creates independent owned storage.

```jet
values := [10, 20, 30, 40]
read: values[0..1]
edit: &values[2..3]
copy: values[0..1].copy()
```

The windows above are disjoint. A bare read window remains a view in a non-owning
place and materializes when it enters an owning slot. Constant disjoint indexes
and ranges lower through a safe structural split; known fields are disjoint;
dynamic projections conservatively overlap. Jet's sema validates these facts.
A call or temporary is not a place: bind it first (E0213). The
retired `values.view(0..1)` spelling is E0214 and points to `values[0..1]`.
Method calls do not extend a place, so `values[0..1].sort()` calls `sort` on
the maximal range, which `sort`'s `&self` makes a write window
(D-SHAPE-PLACE1=A, D-CALLSITE-MARK2=A).

Sema keeps one provenance and alias graph for every borrowed window, regardless
of whether the runtime representation is a string slice, `View<T>`, arena
window, buffer, or matrix window. An owner is identified by declaration, not by
spelling: a local uses its definition identity, a public function parameter its
zero-based position, and static storage its declaration. Shadowing therefore
creates a different owner. A place is an owner plus ordered field, index, or
range projections; reborrowing preserves that owner.

Pattern payload bindings retain the matched place's owner. A payload borrowed
from a parameter may therefore supply a returned view `from` that parameter;
a payload taken from a local or consumed parameter cannot outlive its owner.
A nested write-pattern binding reborrows its parent's write access. While the
child is live, overlapping accesses through the parent are suspended; accesses
through the child do not create an independent competing loan.

Each view fact records place, read/write access, lexical extent, source kind, and
invalidation state. Read views may overlap. A write view is unique and cannot
overlap any live read or write view. Different known fields are disjoint; ranges
and indexes overlap unless sema proves otherwise. Moving or replacing an owner,
writing an overlapping place, or resizing or relocating its storage is E0212
while a view is live; a call of a lambda that writes a captured owner writes
that owner. Arena reset or close invalidates its views; a later read is
E0632. Facts end after the last use or at lexical scope, whichever comes first;
control-flow joins and loops keep invalidation conservatively. Captures and field
projections preserve the fact. A captured or returned view crossing a task or
channel boundary is rejected once as E1102.
Access through a stored field such as `parts.left[0]` uses that field's view
fact, not another window stored in `parts.right`. Only its own loan and its
reborrow ancestors are exempt; a live descendant still suspends overlapping
accesses through the parent field.

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
and is a breaking provenance change. The IR builder receives only sema-approved provenance
and lowering flags; it does not infer owners, overlaps, lifetimes, or escapes.

A kept link into a growable container follows D-LINK-ITEM1=A. A brief link
into a list item is a view: it pauses growth until its last use. A link the
checker cannot bound to a brief range must point at an object, so a list
whose items are kept is declared `[@T]`; a stored link into an item of `[T]`
is rejected with a fix that rewrites the element type. This rule takes effect
with the D-MEMREF1 link build.

A method's field footprint (D-METHOD-FOOTPRINT1=A) is the set of receiver
fields its body reads or changes through `self`, including the footprints of
sibling methods it calls on `self`; any other use of the whole `self` makes it
the whole record. A receiver may state it, `&self.log` or `&self.[log, hits]`;
the stated set must name stored fields and cover the body (E0231). Public
methods publish the footprint (stated, else inferred) in the API snapshot as
`; footprint = self.[a, b]`. Calls still count as touching the whole receiver
until the native emitter keeps disjoint field borrows of one local across
statements.

### Zero-copy string views

Binding `String.trim()`, `.after(sep)`, or `.before(sep)` to a local creates a
zero-copy view into the receiver's buffer when sema proves the binding cannot
outlive its owner. The Jet-level type remains `String`.

```jet
padded := "  user@jet-lang.dev  "
email: padded.trim()
domain: email.after("@")
print("padded still readable: {padded}")
```

A local view may chain another string-view operation, be interpolated, be placed
in a view-typed aggregate, or be copied with `.copy()`. An owning destination copies it
by default; returning that owned copy does not move the original owner. A
returned window likewise retains its owner's provenance rather than consuming
the owner's storage. At a named boundary, `View<str>` states the owner-tied contract;
E2307 rejects a temporary or unstable owner, or an explicit-copy policy that was
not satisfied. See
[`string_view.jet`](../../Examples/Memory/Views/StringView.jet) and
[`returned_views.jet`](../../Examples/Memory/Views/ReturnedViews.jet).

### Shared state, local cells, and pools

`Shared<T>` is a lock-guarded, copyable handle. `Shared{value}` constructs it and
infers `T` from the value; `Shared<T>{value}` names `T`, and the value adapts to
it like any typed literal's field (#6281 c130). `shared` is an ordinary name,
not a construction word. A field read takes one read lock and a field write one
write lock; a read-modify-write such as `config.hits += 1` holds one lock across
both halves. Each statement is one atomic step. Cloning a `Shared<T>` clones the
cheap handle, not the payload, so it can cross a task boundary without `&&`
(D-SHARED-API1, D-CONC-SHARE1=A).
A type may declare strong `Shared` edges back to itself (a linked list, a tree);
the cycle check runs at the write. A write into a shared cell, its edit guard's
`.value`, or `try_replace`, whose place can hold a strong edge back to the cell's
type, is rejected with E0221 unless the stored value is built in place: a
literal, a new cell (`Shared{T{…}}`) whose fields are built the same way, a value
whose type has no strong path back, or a `:` name bound to a new cell in the
same run of its scope and read nowhere else since. A writing call
(`cell.kids.push(kid)`) is checked the same way for each argument. Building a
list by prepending or adding new children never closes a cycle; storing an
existing handle does, so use a `Shared.Weak<T>` back-edge (`.downgrade()`) or an
id. Weak back-edges do not keep a cycle alive; computed fields are not stored
edges. A function value keeps alive what it captures, so a place of function
type (or a list, option or map of them) can reach a cell of any type: a
function stored there must be a named function or a lambda whose captured
locals and parameters are themselves built in place, and a stored callback
that needs its cell captures a weak handle (`back: cell.downgrade()`, then
`back.upgrade()` inside it). A writing call on a place inside a cell
(`cell.items.push(x)`) edits the stored value under the cell's write lock, like
a field write.

The complete assignment place stays under that lock: `store.items[key] = value`
updates the stored map, and nested field or indexed-record writes update the
payload rather than a copied read. This also holds inside `#Transact` and through
an expert edit guard. Direct indexed `+=`/`-=` still follows the ordinary E0164
rule: use a total update with an explicit missing-key fallback instead. See the
[compound-place test](../../tests/cases/shared_transaction/shared_compound_places_match_o0_o1_o2_and_interpreter.fixture/run.jet).

```jet
AppConfig: struct {
    name: String
    hits: Int
}

handle: fn(id: Int, config: Shared<AppConfig>) -> String {
    label: config.name
    "request {id} on {label}"
}

run: fn() {
    config: Shared{AppConfig{name: "jet-server", hits: 0}}
    t1: task handle(1, config)
    print(t1.join() ?? panic("task failed"))
    config.hits += 1
}
```

The lock is per statement. When one statement touches a second shared value,
the ordered shared engine avoids nesting locks and therefore avoids the plain
access deadlock class. The closure spellings `Shared.read(f)` and `Shared.edit(f)`
are retired (E1116); read fields directly. `Shared.new(x)` is retired (E1115);
use `Shared{x}`. The braces hold any expression, so a generic optional payload
is `Shared<T?>{Val(x.move())}`.

Expert code can hold a lock across helper calls with `guard_read()` or
`guard_edit()`. Each returns an owned `SharedGuard<T>` and releases it on every
exit. `.map(fn(value) { value.field })` narrows one guard to a field. `.split(first,
second)` is legal only when sema proves the stored field paths disjoint. Guards
are task-local and cannot be copied or sent. A public guard parameter reads by
default; `&guard: SharedGuard<T>` requires and preserves edit access
(D-SHAREDGUARD1=A, D-SHAREDGUARD2=A).

`Condition.new()` creates a wait set. `guard.wait(condition, predicate)` requires
an edit guard, registers before releasing the lock, reacquires the same lock,
and checks the predicate again. Cancellation unregisters the waiter before final
release. `notify_one()` wakes one waiter and `notify_all()` wakes all waiters.
See [`shared_guard_queue.jet`](../../Examples/Memory/Shared/SharedGuardQueue.jet)
for the bounded-queue shape.

Inside `#Transact`, writes to `Shared` values are buffered and commit atomically.
The body runs exactly once. At commit, touched values are write-locked together
in stable-address order, buffered writes are applied, and the locks are released.
Contention waits; it does not retry the body. Irreversible `Net`, `FS`, or `Exec`
effects inside the body are E0746; move them after the transaction or register
them with `on_commit` on a named transaction. A transaction name is optional;
the named form exposes `on_commit` and `on_rollback`. Registering an
`on_rollback` hook takes control of undo for the handled value, so that value is
not automatically snapshotted (D-STM1, D-CONC-STM1=A, D-TXN-ROLLBACK).

Every exit other than the body's clean end rolls back, the same way on `jet run`,
`jet build` and `--release`: a failure that propagates out of the block, an
early `return`, a `break` or `next` out of the block, a panic or trap, a
cancellation or deadline, `process.exit` and `os.stop`. Rollback restores the
snapshotted values (latest first), drops the buffered `Shared` writes, then runs
the `on_rollback` hooks in reverse registration order, in the same cleanup that
runs the frame's closes and guards (D-FAIL-EXIT1). A clean commit drops the
rollback hooks unrun. Each hook runs with the stop that is unwinding the frame
masked (a cancellation or deadline does not reach it) and under its own
deadline of 5 seconds, observed at the hook's wait points like any `#Context`
deadline. A failure inside a hook (a panic, a trap, its deadline) is never
fatal: it stops only that hook, the remaining hooks and the frame's cleanup
still run, and it is reported as a secondary failure after `while rolling back,
this also failed:`. It is appended to the report of the panic or stop being
unwound, or written to stderr when the rollback came from a propagated failure
or an early exit, and the program goes on. Rollback never turns a panic into a
recoverable error: after the hooks the panic keeps propagating (D-NOPANIC1). A
hard kill (SIGKILL, power loss) runs nothing; the buffered writes were never
applied, so no half transaction is visible (#5477).

```jet
transfer: fn(from: Shared<Account>, to: Shared<Account>, amount: Int) {
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
escaping lambda; a lambda that does not escape borrows the guard inside its
scope and may read it. `Cell`, `CellReadGuard`, and `CellEditGuard` cannot cross a task,
channel, `Shared`, task-group, or parallel-adapter boundary; use `Shared<T>` for
synchronized cross-boundary state (D-LOCALCELL1=A).

`Pool<T>` is a generational arena and `ID<T>` is copyable index-plus-generation
data. The pool owns each `T`; an `ID<T>` never accesses `T` by itself. `add`
returns an ID, `pool[id]` indexes for read or write, and `ids()` walks live
entries. Removing an entry bumps its generation and returns `?T`. Indexing a
stale ID panics, like an array bounds failure, rather than silently reading old
storage. `add`, `remove` and `pool[id] = v` change the pool, so they need a
writable pool: a `:=` binding or a `&` parameter. Through an unmarked
parameter, `add` and `remove` are **E0205**.

```jet
Player: struct {
    name: String
    hp: Int
    attack: Int
    target: ID<Player>?
}

run: fn() {
    world := Pool<Player>.new()
    kai: world.add(Player{name: "Kai", hp: 100, attack: 15, target: None})
    world[kai].hp += 1
    fallen: world.remove(kai)
    _: fallen
}
```

See [`entity_world.jet`](../../Examples/Memory/EntityWorld.jet) and
[`entity_tree.jet`](../../Examples/Memory/EntityTree.jet) for pool
links and nested writes (D-POOLID-API1).

`Rows<T>` (`core.collections`) is an add-only table and `Row<T>` its 4-byte
row handle. `rows.add(v)` appends `v` and returns its `Row<T>`; `rows[row]`
reads or writes that row (`rows[row].f = v` too); `rows.f` is a read-only
`[F]` of field `f` of every row, in row order; `row.index()` is the row's
position as an `Int`; `freeze(rows)` snapshots the table. There is no remove,
insert, or sort, so a handle never goes stale and needs no generation. A handle
indexes only a table of its own row type: `Row<Node>` indexing a `Rows<Ty>` is
E0112 at compile time. `add` panics past 4,294,967,295 rows. Row fields are
meant to be scalars or `Row` handles, which a later column layout keeps one
array per field (D-COLUMNS1). See
[`rows.jet`](../../Examples/Collections/Rows.jet).

### Transitive memory facts

Memory floors are effect-row prohibitions, not `#Policy` settings. Sema checks
every reachable call, including dependency calls, against the function's
`!Mem.*` denial. The unbounded forms are `!Mem.Alloc` and `!Mem.Rc`; the bounded
forms are `!Mem.Alloc(above: N)` and `!Mem.Stack(above: N)`. E0921 identifies
the incompatible operation, prints the full call path, and names the denial and
its provenance. An open-world dispatch needs a sealed target set or signed
dependency summary; otherwise the strict fact is unprovable and rejected
(D-AUTHORITY-MEM1, D-MEM-FACTS1). For `!Mem.Stack(above: N)`, an executable
build (one with a `run: fn` in its entry file or an Output entry) seals its own
dispatch: a call through a trait value may run any implementation the program
holds of the method, and a call through a function value may run any function
the program takes as a value or lambda it writes that takes the call's
arguments. A library build leaves both open, since its callers may pass any
value or add implementations.
`!Mem.Gc` forbids collection by the opt-in tracing collector (D-DENY-COST1); a
row, a `#FX(…)` cap or an authority `deny:` names it like any other deny-only
leaf, and the denial follows every reachable call.

```jet
integrate: fn(e: &Entity, dt: Float) -[!Mem.Alloc]> {
    e.pos += e.vel * dt
}
```

`!Mem.Stack(above: N)` bounds the stack every call path may take, in bytes
(D-STACKBOUND1). Sema bounds each frame by a fixed allowance for call linkage
and saved registers plus two slots for every parameter and checked value, each
slot sized by the value's layout, and adds the deepest callee's path. In an
executable build a call through a function or trait value, or a function
value handed to other code, counts each target of its sealed set, and a
lambda's frame on top of the caller's. A path through a call cycle with no
`#MaxDepth(n)` function on it, a function or trait value in a library build, a
function value with no checked body, a trait value with no implementation, an
open dispatch or a value of unknown size, such as an unbound generic
parameter, has no bound, so the denial is E0921. Adding a function the program
takes as a value, or an implementation, can change a bound. `Mem.Stack` has no
unbounded form: `!Mem.Stack` alone is E0119. The backend checks every frame it
lays out for a function a `!Mem.Stack(above: N)` denial reaches against sema's
bound for that function, with the bounds of the bodies inlined into it added;
a frame over it is a compiler fault, an ICE (exit 101). Frames no stack denial
reaches are not checked.

`#MaxDepth(n)` on a function or method bounds its recursion: at most n calls of
it are live at once on one thread, the first call included
(D-RESOURCE-LIMIT1). Call n + 1 stops with the stack overflow report, E3012,
exit 70, before its body runs. Calls that return or stop count off, so the
function may be called again. `#MaxDepth(0)` is E0930. In mutual recursion
only the marked functions are counted. For `!Mem.Stack(above: N)`, every call
cycle needs a marked function on it. Sema then counts the cycle's deepest pass
between marked calls once per call the marks allow: the sum of their n, plus
one pass when the path enters the cycle at an unmarked function.

```jet
#MaxDepth(64)
descend: fn(level: Int) -[!Mem.Stack(above: 65536)]> Int {
    if level == 0 -> return 0
    descend(level - 1) + 1
}
```

The human aliasing rule is: while something is being changed, nobody else may be
looking at it. Foreign `read` and `write` spellings are not Jet access syntax;
under D-S14-PAUSE they receive ordinary parse errors, apart from a separately
ratified narrow teaching diagnostic.

## Access sigils

Access markers are prefix sigils on the type, not on the binding name. The
unmarked type is the enforced read default; the only explicit access sigils are
`&` for exclusive write and `&&` for take (D-MOVE-SIGIL1=A). They appear on
parameters; calls stay plain, because the signature decides
(D-CALLSITE-MARK2=A).

| Written type | Meaning | Native lowering of a non-scalar parameter |
| --- | --- | --- |
| `T` | read; the callee cannot elevate it | shared borrow |
| `&T` | exclusive edit access | mutable borrow |
| `&&T` | ownership moves to the callee | owned value |

```jet
Player: struct {
    hp: Int
}

damage: fn(player: &Player, amount: Int) {
    player.hp = player.hp - amount
}

consume: fn(resource: &&String) { print(resource) }

run: fn() {
    player := Player{hp: 10}
    damage(player, 3)
    consume("resource")
}
```

An access marker composes with the optional prefix: `&?User` is write access to
an optional `User`, and `&&?Texture` takes an optional `Texture`. A marker both
before the parameter name and on its type is E0029:

```text
Error [jet.DuplicateAccessMarker]: `p` has two access markers
  --> file.jet:4:10
    |
  4 | bad: fn(&p: &&Player) { print(p.hp) }
    |          ^
 Why: The write-access marker `&` before the parameter name conflicts with the take marker `&&` on its type; a parameter can declare only one access marker
 Fix: Keep one access marker on the type or receiver and remove the other
```

The three marks outside signatures are a write window bound to a name
(`edit: &values[2..3]`), a match that writes into its subject
(`if &shape == { … }`), and `task &&name`. `^` has no meaning in the grammar.

`.copy()` is a method call, not an access marker. Raw-pointer access (`p.*` or
prefix `*x`) is a separate `#Unsafe`-gated mechanism. It does not change the
parameter-access table (D-MEM1, D-COPY-WORD1=A, D-CAP9).

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
| wire | run / wire value | D-SERDE1 and D-ENC1 use `Dyn` and one typed codec path |
| validation | run / wire value | D-VALIDATE1 accumulates `FieldError` values; D-VALIDATE-DECODE1 gives decode failures one shape |
| migration | run / wire value | D-MIGRATE1 and D-MIGRATE4 apply named schema operations transparently through typed decode |
| fact | all rows / fact | D-FACT-LAW1 and D-FACT-FLOW1 keep dataflow facts in the shared sema ledger |
| trust | build / dependency | D-JPK-GRANTCMD1 and D-JPK-GRANTSCHEMA1 make authority explicit, reviewable, and revocable |
| error | checker column of every row | registered E-codes, `FieldError` values, and typed foreign errors name what failed and its repair path |

The grid is a planning and proof obligation, not a runtime layer. Sema owns
checks and facts; the Prelude owns runtime boundary meaning. Rust emission
and interpreter paths marshal the same typed result and fact semantics. A
boundary with a runtime path must preserve the same cell through AOT, the
default `jet run`, and interpreter execution.

## Effect system

This section defines effect inference, authority, and the checked tooling that
exposes those facts. It is for authors of functions and packages, and for tools
that consume checked programs. Executable truth lives in the [effect
table](../../Compiler/Prelude/Effects.jet), [syntax
tables](../../Compiler/JetFoundation/Data/Syntax/), [sema effect
checker](../../Compiler/JetSema/Source/Effects/), [inspect
projections](../../Compiler/JetCli/Source/Cli/Expand.jet), and [expand
tests](../../tests/cli_parts/expand.rs).

Every function has an **effect set**: the ambient powers exercised by its body,
such as filesystem, network, clock, or process access. Jet infers the set,
propagates it through calls, and erases it before code generation. An empty set
is purity; effects are not runtime values, handlers, or monads
(D-EFF1, D-QUAL1, I3).

### One rule on every rung

Every place that holds code is a *rung* with an effect row: a `#FX` block or
lambda, a function, a trait, a type, a module (an inline module, a file head,
and the file where the module is declared), the package and the run. A
written row is a ceiling (`FS, IO`: at most these) plus denials (`!Net`:
never this, including through calls). The checker proves that what code
reaches fits every ceiling above it and meets no denial above it. Rungs
compose from the outermost inward: the nearest rung is checked first, a
denial anywhere above always wins, and a row written inside a ceiling may
only narrow it. Only the package and the run grant; every other rung only
restricts.

The spellings are one list syntax in two places. A function and a function
type carry the row in the arrow: `-[FS.Read]> String`, `-[]>` for pure,
`fn(T) -[Net]> U`, and `-[via f]>` to pass a function parameter's row
through. Every other rung writes `#FX(...)`: above a module, a struct, an
enum, a trait or a file's `use` lines, as an `#FX: (...)` line in a body, or
around a block as `#FX(...) { ... }`. Inside the list, `!E` denies and
`Mem.Alloc(above: N)` bounds a denial; a bare `#FX` is pure. The package
writes `authority: { allow, deny, grants }`, and the run takes `--allow`,
`--deny` and `-A`.

A row nobody writes is inferred: a function's from everything its body
reaches, a type's from its methods, function fields and held types, a
trait's from its implementations, and a dependency's from its code, recorded
in the lock. So a program with no rows checks and runs, and an error appears
only where someone wrote a limit or a grant is exceeded. Each such error has
one registered code, with its what, why and fix text and a worked example,
in the [diagnostics registry](../../Compiler/Prelude/Diagnostics.jet)
(`jet explain E0749`). The examples in
[`Examples/ErrorsAndEffects/Effects`](../../Examples/ErrorsAndEffects/Effects/)
show every rung, each with golden output: `Effects.jet` (no rows),
`Pure.jet` (functions), `EffectHigherOrder.jet` (function types),
`TraitCap.jet`, `TypeCap.jet`, `ModuleCap.jet`, `FileScope.jet`,
`LibraryEffect.jet` and `EffectAuthority.jet` (the package).

### Effect names and declarations

The thirteen grantable roots are `Net`, `FS`, `IO`, `DB`, `Time`, `Rand`,
`Env`, `Exec`, `Log`, `GPU`, `FFI`, `Browser`, and `Secret`. A path may add
user-chosen dotted leaves, for example `FS.Read` or `Net.HTTP.Get`. The root
must be one of those names; descendants are checked by tree ancestry, so a
bound on `FS` covers `FS.Read`. Foreign-language names such as `FFI.Go` and
`FFI.Py` are leaves beneath `FFI`; the parent covers the whole foreign-call
tree. The Prelude also registers common leaves such as `FS.Read`,
`FS.Write`, and `Time.Wait` (D-CASING1, D-EFFTREE1).

The compiler builds in the **machine effects**: `Net`, `FS`, `IO`, `Time`,
`Rand`, `Env`, `Exec`, `GPU`, `FFI`, `Browser`, `Panic`, and `Mem`
([Compiler/Prelude/Effects.jet](../../Compiler/Prelude/Effects.jet)). `DB`,
`Log`, and `Secret` are Core's **service effects**, declared in
[Core/Effects/Effects.jet](../../Core/Effects/Effects.jet) with `#FX(…)` above
each one naming the machine effects it relies on: `DB` (with its cases
`DB.Read` and `DB.Write`) on `Net` and `FS`, `Log` on `IO`, `FS`, and `Net`,
and `Secret` on `Env`, `FS`, and `Net` (D-EFFTYPE-KERNEL1). A row that names
a service effect may also do what its list names, so a `-[DB]>` function may
read a file; a body that reaches anything else is E0740. A service effect is
a bare name everywhere. Denials keep checking what a body really reaches, so
a `-[!Net]>` function may call a `-[DB]>` function over a local SQLite file,
while one that calls a driver that reaches the network is E0749.

The Prelude writes each effect as a type implementing `Effect`, recognized by
its in-body `impl Effect { … }` block. A struct is a root with no members; an
enum is a closed family whose cases are its leaves. `irreversible: fn(self) ->
Bool` says which effects a `#Transact` rollback can't take back, per case
(D-DECL-TYPES1):

```jet
FS: enum {
    Read
    Write

    impl Effect {
        irreversible: fn(self) -> Bool { self == .Write }
    }
}
```

An `FS.Read` call inside `#Transact` is allowed; an `FS.Write` call is E0746.
An effect is a type with an `impl Effect` block; `effect` is not a declaration
word. A `Log.Audit: effect` line is E0003, and its fix declares
`Audit: struct { impl Effect { parent: Log } }`, a package effect that is a
kind of `Log` (see the package-effect paragraph after the E0757 entries).

A root that has declared leaves is closed: the Prelude's own leaves
(`FS.Read`) and an effect family's cases (`Query.Read`) are the only dotted
names under it. A dotted use under a closed root must name one of them
(E0750); a bare root remains valid, and a root with no declared leaves
remains open. The same rule applies to function effect rows, `#FX`, and
package effect budgets. Declarations have no runtime representation. An
unknown root is a language error, not a new effect.

A package writes its own effect the same way, as a *library effect*: a
struct or enum that implements `Effect`, with `#FX(…)` above it naming the
kernel effects it relies on (D-EFFTYPE-USES1, D-DECL-TYPES1):

```jet
#FX(Net)
Query: enum {
    Read
    Write

    impl Effect {}
}

count: fn(sql: String) -[Query.Read]> Int { sql.len() }
```

A row that names a library effect, or one of its cases, may do what its
list names and nothing else: a body that reaches another kernel effect is
E0756. Inside its package the name is bare; elsewhere it is qualified by the
name the package is imported under (`graph.Query`), and the bare name there
is E0119. Denials keep checking what a body really reaches (D-PROP1), so
`-[!Net]>` blocks a call into `Query`'s network work. A function uses a
library effect when its row names it or one of its cases, so
`-[!graph.Query]>` blocks every call that reaches such a function, and
`-[!graph.Query.Write]>` only those that reach `Query.Write` or `Query`
itself; either is E0749. The list above a library effect holds no `!`
entries (E0757).

A *package effect* is a library effect that is a kind of another effect. Its
`impl Effect` block names the parent with `parent:` (D-EFFECT-PKGLEAF1):

```jet
Audit: struct {
    impl Effect {
        parent: Log
    }
}

record: fn(event: String) -[Audit]> { log.info(event) }
```

The parent is an open Prelude effect (a struct such as `Log`, not an enum
family such as `FS` and not a deny-only row) or another library effect
without cases that the module can name, and following parents ends without a
cycle; otherwise the declaration is E0758. A row that names the kind may do
what its parent may; a `#FX(…)` above it may narrow that, and one that widens
it is E0758. The kind takes the parent's irreversibility unless its own
`impl Effect` block gives one. Denying the parent denies every kind of it:
`-[!Log]>` blocks a call that reaches a function whose row names `Audit`, and
`-[!audit.Audit]>` one whose row names a kind of `audit.Audit`; both are
E0749. A row that names one kind allows only that kind and the kinds of it:
two packages may each declare an `Audit` kind of `Log` without clashing, and
a function whose row names `audit.Audit` and that reaches `billing.Audit` is
E0759 unless its row also names `billing.Audit` or `Log`.

`Panic` and `Mem` are deny-only rows. `Panic` can be named in a prohibition but
cannot be granted by an authority, positive effect ceiling, or package budget.
Memory events such as `Mem.Alloc` and `Mem.Rc` remain visible to diagnostics
and denial facts; they do not have to be listed in a positive effect ceiling.
The same exclusion applies to inferred purity: an explicit empty effect row
`-[]>` admits allocation, heap copies, and panic facts without requesting a
positive right. Ambient effects such as `IO` still violate that empty ceiling.
Writing module state is not an effect, but a `-[]>` function may not do it,
itself or through any function it calls (E3401): a module `Shared` holder's
value, a module `Cell`, and a `#Persist` or `#Unsafe` module `:=` binding all
outlive the call, so a later call could see the write. A cell reached through
a read parameter, `self`, an alias or a lambda capture may be such a cell, so
writing it, taking its `guard_edit()` lease included, is E3401 too. Its own
locals, a cell it builds (`Shared{…}`, `Cell.new(…)`), and a `&` parameter
its caller lends for writing stay free to change.
`Mem.Alloc(above: N)` and `Mem.Stack(above: N)` are the parameterized
memory-denial spellings. An explicit memory, waiting, or panic denial still
rejects a reachable operation
(D-EFFTREE1, D-PANICROOT1, D-AUTHORITY-MEM1, D-AUTH-AMBIENT2).

`Unsafe` is a deny-only row too: it is the fact that a function reaches
`#Unsafe` code. A function that opens an `#Unsafe` gate itself (an
`#Unsafe` block, an `#Unsafe` function, or a foreign function's gate) records
it, and every function that can call it inherits it; an open-world call
outside Core reaches it as well, so an opaque callee fails closed. Core's own
gates are vetted and sealed, so calling Core never reaches `Unsafe`.
`-[!Unsafe]>`, a cap's `!Unsafe`, `authority: { deny: [Unsafe] }` and
`--deny=Unsafe` each refuse every such path with E0749, which prints the call
path to the function that opens the gate and the gate's `file:line`. `Unsafe`
never has to be granted, and naming it in a positive row grants nothing
(D-MEMRAW-EFFECT1).

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
randomness still does, except where the runtime or Core reads the host under
another effect's row.
[Host reads under another effect](reference/host-reads.md) lists each such
read. One example is the resolver's `/etc/hosts` read under `Net`. The page
also says how each read is classified, and a row or a denial covers a read
only as that page says.
The opaque Rust bridge example `Examples/Interop/FFI/FFI.jet` therefore
declares the complete maximal grant, including `Secret` and `Time`; its base64
operation is unchanged, and this grant does not assert that the foreign body
actually reads either capability.

A read that only makes a collision-avoiding name, finds the program an `Exec`
call starts, or inspects the running process belongs to the calling
operation's effect (D-HOST-NONCE1=A). The temporary-name and atomic-write
reads are part of `FS.Write`, the `PATH` lookup is part of `Exec`, and
`/proc/self` reads are part of whatever call makes them. A name such a read
makes is unspecified, so it differs from run to run. Reading the `/proc` entry
of a child that an `Exec` call started, to enforce that call's memory or
open-file limit, is part of `Exec` too, and the typed limit errors stay
(D-HOST-NONCE2=A). The vault write lock reads no process entry: it is a
kernel lock, released when its writer exits. Any other host read either fits
these rules or declares its own effect.

For example, interpolated `print` in
`Examples/Basics/FirstHourExpert.jet` records both `IO` and the
`Mem.Alloc` needed for its fresh `String`; `core.process.args()` records the
`Exec.Args` leaf, while starting a process records the `Exec` root. A Core
call with a precise leaf under its own root records only that leaf, so
`files.read` requires `FS.Read` and `files.write` requires `FS.Write`; the root
still covers both. Application authority reads these sema facts and never
alters them.

### Effect scopes

An effect argument names a scope from Core's scope kinds: a folder
(`FS.Read("templates/")`), a host (`Net("api.example.com")`), a device
(`GPU("npu")`) or a C library (`FFI.C("z")`, see "C ABI"). The argument
is one plain string, with no interpolation,
quote or backslash; `Mem` and `Panic` take none. One covers rule
(`Compiler/JetFoundation/Source/Policy/EffectScope.jet`) decides coverage for
effect rows, denials and run grants, from three fixed laws
(D-EFFECT-ARGS1):

- A path covers by whole components after normalization: `templates/` covers
  `templates/home.html` but not `templates2/`, and `templates/../secret.txt`
  is `secret.txt`. A `..` kept above a relative start never climbs into a
  scope: `..` does not cover `../..`, nor `.` cover `../a`.
- A host covers by exact match after lower-casing and dropping one trailing
  `.`: `api.example.com` never covers `evilapi.example.com`. Every other byte,
  whitespace too, is part of the host. Internationalized names are compared
  as written.
- Any other argument covers by equality.

These laws have one implementation, the runtime's (`jet.runtime.authority`'s
`covers`): the checker's rows, grants and denials, `--allow` and `--deny`, the
runtime's Authority values (`with`, `without`) and its effect-scope checks all
answer by it, and one normalization table pins its answers.

A bare effect covers every scoped instance of itself and its leaves, so `FS`
and `FS.Read` both cover `FS.Read("assets/")`. A scoped effect never covers
the bare one.

A call names its scope through a constant argument. The `core.files`
functions that read or write the one path their first argument names, such
as `read`, `read_bytes`, `open`, `write` and `create`, need `FS.Read` or
`FS.Write` scoped to that path when it is a string literal. The compiler
reads this off each declaration: a top-level `core.files` function whose
first parameter is `path` and whose row declares one `FS` leaf publishes it.
`files.read("assets/logo.txt")` needs `FS.Read("assets/logo.txt")`. An
interpolated literal names the folder its constant text ends in, so
`files.read("assets/{name}")` needs `FS.Read("assets/")`. `http.get` with a
constant URL host needs `Net` scoped to that host. A path known only at run
time names no scope and needs the bare effect.

```jet
load: fn() -[FS.Read("assets/")]> String {
    files.read("assets/logo.txt") ?? ""
}
```

A row with a scope is checked at build time: a constant path outside it is
E0740, and so is a run-time path, since the bare need is outside every scope.
A denial (`-[!FS.Read("secrets/")]>`) refuses each need it covers with E0749;
a need that only overlaps it, such as `FS.Read("assets/")` under a denied
`assets/private/`, builds. Run grants (`--allow`, `--deny`) use the same
spelling and rule, and a mediated call checks its run-time path against them
(see "Application authority").

A constant prefix proves where a call starts, not where it lands:
`files.read("data/{name}")` with `name` set to `../secret.txt` leaves
`data/`. So a closed written row with a scoped `FS` or `Net` entry is also
mediated at run time, at every optimization level. While the function runs,
its row is in force, for every callee and every task it spawns. Each file a
`core.files` path call reaches, and each host an `http.get` reaches,
including every redirect hop, is checked before the system call or the
lookup against every row in force. A file is resolved as the kernel resolves
it: from the working directory, with each link of its existing part followed
(a call that acts on a link itself resolves only its folder) and `..`
applied. A scope's folder is resolved the same way and covers by whole
components. A row that allows some effect of the resource's root (`FS` or
`Net`) must allow one covering it, else the call fails with E0740. A denial
in the row that covers it fails the call with E0749. The failure is the
call's own error, `IOError.PermissionDenied` with the refusal as its cause,
or `HTTPError.Policy` with the refusal as its reason, and the call does
nothing. A path replaced between the check and the system call is the
kernel's to confine (`--allow`, Landlock). An open row (`..E`), the listed
scopes of a library effect and value scopes (`GPU("npu")`) stay build-time
only. An inline loop body runs under its function's row.

### Application authority

Every effect is allowed unless code denies it (D-AUTH-DEFAULT1). With no
written authority, `jet run` grants every effect. Before the program's first
instruction it prints one notice on stderr that names the program's inferred
effects in plain words:

```text
note: tool.jet reads files and uses the network (-A hides this)
```

The list is sema's row for the entry: it follows calls and every
function the program takes as a value, so a function reached only through a
value still lists its effects. An open-world call (a C binding no descriptor
owns, an unannotated foreign call) lists `FFI`, whose words say the program
"calls native code, which can do anything". `Exec` reads "starts other
programs, which this list does not limit". The floor, `IO`, `Mem.Alloc` and
`Exec.Args`, is never listed, and neither are the deny-only rights (`Panic`,
`Mem`, `Time.Wait`, `Unsafe`) or `Context`, the task context every program reads
without a grant. The floor covers output, ordinary allocation and argument
reads, and carries no power the shell has not already given the process. It
never covers the `Exec` root, which also starts processes.

The notice repeats whenever the effect set changes, and only then. Jet
records the last effect set it named for each entry under
`$XDG_STATE_HOME/jet/effects` (or `$HOME/.local/state/jet/effects`), keyed by
the canonical entry path. A rerun with the same set prints nothing; a rerun
whose set changed, from the program's own code or from a dependency, prints the
notice again. When standard input is not a terminal (a pipe, CI) or the state
store can't be written, the notice prints on every run. `jet run --json`
carries the same effect list as structured detail: one `run.effects` status
on stderr whose `effects` field lists the noticed rights, beside
`required_effects`, on every run with an effect to name.
`-A` (`--allow-all`) acknowledges every effect and hides
the notice. `--allow=R` acknowledges `R`, leaving it out of the notice; a
scoped right (`--allow='FS.Read("data/")'`) still confines the run through the host sandbox,
and so does the same right in the package's `authority.allow` row.
`--deny=R` wins over every grant: a run that reaches a denied effect stops
with E1803 before host state changes. A host read that runs under another
effect's row is denied only as [host-reads.md](reference/host-reads.md)
classifies it. `--deny=FS.Read`, for example, does not stop the reads that
page places under the operation making them: the resolver's `/etc/hosts` and
`/etc/resolv.conf` under `Net`, the `/proc`, `/sys` and `/etc/group` facts
`core.sys` reads under `Env`, the running process's own `/proc/self` entries,
and the `/proc` entry of a child an `Exec` call started, read to enforce that
call's memory or open-file limit.

A written authority row is a ceiling. This holds for an `authority` row in
`package.jet` or in a script's inline `package: { … }` block, a module's
`#FX`, a type's `#FX`, and a function's `-[…]>` row. Reaching an effect
outside a function row is E0740. Reaching an effect outside a package row or
a module's or type's `#FX` is E0749. All are reported by `jet check`, `jet build` and
`jet run` with the call chain, before anything runs. The floor stays inside
every ceiling, and a floor right leaves only through an explicit `deny`. The
effects the entry package's
rows list are acknowledged and leave the notice, so a program whose package
row covers what it reaches prints no notice. A `deny`, whether on a package,
module, type or function, prohibits the denied effects for everything inside
it, transitively, exactly as if every function inside wrote `-[!E]>`.
Reaching one is E0749 with the call chain (D-AUTHORITY-DENYROW1). `jet new`
writes no authority row. A ceiling is something a team chooses to write; it
is not the price of running.

Jetpack records each dependency's inferred effect row in the lock: the first
build or run after `jet fetch` records it, and a refetch or update keeps it.
When an update or an edit widens a dependency's row, the next build or run
prints which package gained which effect, with the call path to it, and
records the wider row; a row only widens. `jet build --locked` refuses each
widened effect (E3513) at the function that performs it, with the same call
path, and writes no lock. A dependency's line under `authority: { grants: { … } }` is its whole
limit, in place of `allow`: `grants: { json: [], dice: [Rand] }` lets `dice`
use only `Rand` and `json` nothing, even when `allow` lists more, and what a
line grants passes `deny` for that dependency (`Panic` is deny-only, so no
line grants it). A dependency that reaches an effect outside its limit fails
the build with E1220, naming the dependency, the effect and the call path
(D-AUTH-DEFAULT1). A grant line is one the user writes; `jet add` writes
none. See
[`Examples/Modules/Packages/GrantLine`](../../Examples/Modules/Packages/GrantLine/package.jet).
Code run straight from a URL or the registry
is pinned by content hash and asks before its first run in a terminal; its
own `allow` rows acknowledge nothing. A yes is kept with the pinned bytes, and
changed bytes or new effects ask again. Without a terminal it stops before
running, and the Fix names the `-A` command.

Command-line authority uses one rights surface: `--allow=<RIGHTS>` and
`--deny=<RIGHTS>`, where the value is a comma-separated list of canonical roots
or leaves, such as `--allow=FS.Read,Time`. The spaced form is also accepted.
`-A` (`--allow-all`) is `--allow` with every right; a deny still wins over it.
The CLI has no per-effect flag family: all authority uses this one rights
surface (D-RIGHTS-CLI1).

A file whose first line is a `#!/...` launch line, such as
`#!/usr/bin/env jet`, is a script. A script follows the same rules as any
program: it runs, prints the same notice when its effect set changes, and is
fully checked. A script is a standalone program wherever it lives, never a
member of the package around it. `jet <file> [args...]` runs a file, and every word after the
file is the program's, so an executable script runs as `./tool.jet args`.
An unchanged script reruns its kept build without compiling.

### Function and block ceilings

A function may omit an effect row; sema still infers its complete transitive
row. A declared row is an upper bound, not a claim that the function uses every
listed effect. The unannotated form uses `-> Type`; an effect ceiling is written
before the return type, as in:

```jet
load: fn(path: String) -[FS]> String {
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
run: fn() {
    #FX(FS, IO) {
        text: core.files.read("x") ?? ""
        print(text)
    }
}
```

The region permits only the listed effects, including effects reached through a
call, or reports E0712. It is a ceiling, not a grant: the operations still
happen and contribute to the enclosing function's inferred row. A block whose
list has no scoped `FS` or `Net` entry disappears in generated code.

Written above a module, a struct or an enum, `#FX(...)` caps every function
inside it with the same list, including what each function reaches through a
call:

```jet
#FX(IO, !FS)
Report: struct {
    title: String

    show: fn(self) { print(self.title) }
}
```

A type's cap binds the methods in its body, the trait impls written in it, and
`impl` blocks written apart from it. A module's cap binds every function in
the module, including the methods of the types declared in it and the default
bodies of its traits, which keep the cap in every impl that inherits them; a
generic module's cap binds the functions of every instance. Written above a
file's `use` lines, `#FX(...)` caps every function in the file. A plain entry
is a ceiling and a `!` entry is a denial; a bare `#FX` is the empty ceiling,
so every function under it is pure. Reaching outside a ceiling or a denial is
E0749 at the offending function, with the cap as the secondary label. A row
written inside a cap may only narrow it: a declared effect outside the ceiling,
or one the cap denies, is E0749 even when the body never uses it. A type's
cap also binds the lambdas written in its methods, the trait default bodies it
inherits, its `Close` impl, its function-typed fields and calls through its
trait-value fields. A function-typed field whose row reaches outside the cap
is E0755 at the field, and each method that calls it is E0749. An
`#FX: (...)` line in the body is the same cap. A declaration takes one cap:
a second `#FX` marker on it, above it or in its body, is E0999. A type without
`#FX` keeps its inferred rows unchanged. A public type's, trait's or module's
cap is part of its package's published API: widening or removing it under a
non-major release is E1218, and narrowing or adding one is compatible.

An entry of a cap or a block is spelled as a row entry: a dotted effect, one
with a plain string scope (`FS.Read("data/")`, `Net("api.example.com")`), or
a bounded denial `!Mem.Alloc(above: N)` or `!Mem.Stack(above: N)`. Any other
argument is E0119, as is either without `!`. A scoped `FS` or `Net` entry is
mediated at run time as a written row's is (see "Effect scopes"): while a
capped function runs, its cap is in force before its own row, and while a
block runs, its list is in force, for every callee and every task it spawns.
So

```jet
#FX(FS.Read("data/"))
Reader: struct {
    read: fn(self, name: String) -> String { files.read("data/{name}") ?? "refused" }
}
```

refuses `name` set to `../secret.txt` with E0740 at every optimization level.
A block inside a lambda, and a lambda that escapes its capped function, are
checked at build time only.

A type's inferred row is everything a value of it can reach: its methods'
solved rows, the rows written on its function-typed fields, and the inferred
rows of the types its fields and enum payloads hold, through lists, maps,
optionals, results, tuples and type arguments. A trait's row is the union of
its implementing types' rows. The rows are solved to a fixed point, so a
recursive type, or a cycle of types that hold each other, settles on the
union of every member's own effects.

`jet explain effects <file>:<line>` prints every cap in force at a site,
nearest first: its `#FX` blocks and lambdas, the enclosing function's row, the
cap of the trait the method implements, the type rung, the module rungs (the
inline module caps, a generic module's cap on each function of its
instances, the file-head cap and the caps where the file's module is
declared), the
package `authority` rows, and `run`, each with the `file:line` that writes
it, then what the site reaches, deny-only effects such as `Panic` included,
and the call chain to each effect, every name as the source spells it.
`--caps` lists every file-head, module, type and trait cap under a path.

### Higher-order and trait effects

A higher-order function's row includes its own body and the effects of function
values passed at each call. A lambda is walked inline, so its effects are the
function's where it is written. Calling a lambda written in the function's
body, through a `:` local bound to it, adds nothing more: a `-[IO]>` function
may call a local lambda that prints. The same call from inside another
lambda's body adds the called lambda's effects to that lambda's own row, so a
callback that calls a local lambda that prints fits a `-[IO]>` parameter. A
directly named function contributes its known row. A `:=` local, a parameter,
a returned, stored, or otherwise unknown function value contributes the
maximal set, which is conservative and sound.

```jet
apply: fn(f: fn(Int) -> Int, x: Int) -> Int { f(x) }

run: fn() -[IO]> Unit {
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
Shape: trait {
    area: fn(self) -[]> Int
}

impl Square.Shape {
    area: fn(self) -[]> Int { self.side * self.side }
}
```

Every implementation must fit the bound or E0742 is reported. An impl method
that writes no row takes the trait's bound as its ceiling, so
`area: fn(self) -> Int { … }` is pure here; this holds for the Prelude's
`CheckedText.check` and `encode_hole` too. A call through a trait value counts
with the trait method's declared row. With no row, it counts with the ceiling
of the trait's `#FX(...)` cap, and with neither, with the union of every
implementation of the method in the program, in any package. A dynamic call
of a row-less method that has no implementation to bound it, under an
enclosing ceiling, receives the annotate-the-method diagnostic E0743 (D-EFF2,
D-EFF3).

Written above a trait, `#FX(...)` caps every implementation of the trait's
methods, in any package, with the same list, including what each reaches
through a call. An `impl Trait` block written in a struct's body and one
written apart from the type are both bound. Reaching outside the cap is
E0754 at the offending method, with the cap as the secondary label:

```jet
#FX(!FS)
Source: trait { text: fn(self) -> String }
```

A call through a type parameter belongs to the caller that binds it. Inside
`Traced: struct<T: Worker>`, `self.inner.work()` is not part of `Traced`'s
rows or its cap; `Traced<Loud>` does `Traced`'s work plus `Loud`'s, charged
where `Traced<Loud>` is written. A report there names the instantiation and
the implementation it runs. A binding the compiler cannot see takes every
implementation, as an unbound generic call does.

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

A bound the compiler can see is checked at the call (D-CRYPTO-DIAG1=A): an
argument outside one of the bounds above, or outside `hkdf_sha256`'s 0–8160-byte
length or `pbkdf2_hmac`'s 1–67108864-byte key length and at-least-one
iteration count, is E2702 (`Crypto API misuse`, with the operation, the bound
and the value), not a `CryptoError` at run time. The compiler sees a written
Int literal, and a byte argument's length when it is a list literal or a
fixed-length `[U8#N]` value; expert keys, secrets and public keys are 32
bytes, Ed25519 signatures 64, AES-GCM nonces 12 and XChaCha20 nonces 24.
E2702 is considered only once every other check passes: the argument's own
type error, an expert call's missing `#Unsafe` gate (E0510), an effect error,
or any other error is reported instead, and the call's own failure
propagation is not reported beside its E2702. A value only known at run time
still reaches the operation's own check.

`crypto.file_seal(recipients, source, destination)` and
`crypto.file_open(identity, source, destination)` have unit success and the
explicit `FileCryptoError!` contract. The identity is read-borrowed rather than
moved. Both use the recipient-based JETC v2 envelope. The fixed prefix is `JETC`,
version 2, kind 1, suite 1, flags 0,
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

`core.ui` windows draw through the `Frame` of each loop turn, rasterized in Jet
the same way on every backend.
`clear_background`, the 2D shapes (pixels, rectangles with outlines, gradients
and rounded corners, lines, circles, ellipses, rings and sectors, triangles and
regular polygons) and `draw_text` (anti-aliased Atkinson Hyperlegible Next at
any pixel size, whose width `measure_text` reports without an effect) paint a
premultiplied RGBA canvas the size of the window by core.ui's one blend rule,
source-over on the sRGB bytes (each byte becomes `s + (d * (255 - a) + 127) / 255`
for the premultiplied source byte `s`); the frame presents the
canvas when the next frame begins unless the program presented its own pixels.
`load_texture_atlas` decodes the atlas's PNG texture in Jet, and `draw_sprite`
blits a named region of it onto the canvas. The collision checks
(`check_collision_*` and `get_collision_rec`) are pure `Float` functions that
need no effect, as are the CPU image functions (`gen_image_*`,
`get_image_color` and `image_*`), which change an `Image` in place through `&`;
`window.load_image_from_screen()` copies the canvas. `core.game` is
scene-first: `game.Scene.new`, `scene.assets.image` and `sound`,
`scene.input.bind`, `scene.component<T>()`, `scene.query<T...>()`,
`game.Replay.record`, `game.Backend.headless`, and `game.run` provide
deterministic headless replay with a frame hook (D-GAME1-3, D-RAY-API1).

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
[`Examples/LowLevel/LowLevel.jet`](../../Examples/LowLevel/LowLevel.jet).

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
`policy: { unsafe: .Obligations, impure: .GateOnly, nondeterministic: .GateOnly }`.
The path is retained as provenance, and an unreadable or malformed configured
file fails closed. `jet inspect gates --kind unsafe FILE` reports each gate,
operation, discharge state, and effective policy. Assertions erase in sema
before the shared IR
boundary. See
[`Examples/LowLevel/UnsafeObligations.jet`](../../Examples/LowLevel/UnsafeObligations.jet). (D-UNSAFE-OBLIG1)

### One audited-gate law

Every deliberate gate uses the same organization → package → module →
function → block tightening ladder. The policy keys are `unsafe`, `impure`,
`nondeterministic`, `dependency_grant`, `build_flag`, `session_flag`,
`trust_grant`, `force_pin`, `taint_scrub`, `duty_drop`, `state_transition`,
`precision_demotion`, `structure`, `lint_allow`, `check_removal`, and `link_override`. The first three retain
their existing modes; every added key accepts `.GateOnly` (the default) or
`.Forbid`. An organization's `.Forbid` cannot be widened by a package
`.GateOnly` or a command-line allowance. Refusal reports the gate kind, its
use site, and the owning policy file.

These keys are package/organization settings, not source settings:
`#Policy(lint_allow)` is E0355. Source writes the concrete gate instead. A
lint exemption is `#Allow(jet.LintName, "why this lint does not apply")`,
naming the lint by its diagnostic type path (`jet.UnitScalarRewrap`); its
reason slot is optional, but omission emits `jet.AllowReason` (L3103).
`#Allow(jet.AllowReason)` cannot suppress that audit and is E0355. Reviewed
corpus exemptions keep reasons at these source sites, not in manifest rows.
`jet fix` rewrites the retired `#allow(lint_name, …)` spelling to `#Allow`.

`jet inspect gates` reports every kind, including lint allowances and their
written reasons. Structure gates record deliberate suppressions such as
`_name`, not ordinary unused names for which `_name` is merely a suggested
fix. Durable trust grants, dependency grants and `.Force` pins are checked
before their writer publishes state; invocation choices are checked before
dispatch. An ordinary deliberate `jet update jet` is not a forced pin.
See [`Examples/ErrorsAndEffects/Effects/AuditedGateLadder.jet`](../../Examples/ErrorsAndEffects/Effects/AuditedGateLadder.jet).
(D-GATE-LAW1=A)


### Explicit pointer casts

Jet has no compact cast-and-dereference operator. To reinterpret an address,
first construct `mem.Ptr<T>.from_addr(addr)`, then use postfix `p.*` in the same
audited region. The two operations make the cast and dereference obligations
separate. The Jai comparison is illustrated by
[`Examples/LowLevel/PointerCastDeref.jet`](../../Examples/LowLevel/PointerCastDeref.jet). (D-POINTERCHAIN1)

### Raw pointer tools

`mem.Ptr<T>`, spelled through an import of `core.mem`, names the same type
as `*T`. An optional pointer is spelled `mem.Ptr<T>?`, since Jet's one
optional spelling is a postfix `?` (the ballot's `?mem.Ptr<T>` names this
type; `*T?` is a pointer to a `T?`). It is one machine word. The null
address is its `None`, so `Val(mem.Ptr<T>.from_addr(0))` is `None`, and a
present pointer is never null. Matching `.Val(p)` hands back the pointer
itself, with no box to unwrap. See
`tests/cases/unsafe_and_runtime/an_optional_raw_pointer_is_none_when_null.case`.
(D-RAWPTR2=A)

`p.offset(n)` is the pointer `n` elements of `T` past `p`, at `T`'s List
stride, and `p.byte_offset(n)` the pointer `n` bytes past it; `n` may be
negative, and either way the result is a `*T`. Both are E3101 outside
`#Unsafe`, since a step can leave the storage `p` names. A step is not checked
where it is made. Under the memory sentry (D-MEM-SENTRY1), a read or write
through the stepped pointer is checked like any raw access, so a read that
runs past the end of a live local is a located R0801 fault. See
`tests/cases/unsafe_and_runtime/a_raw_pointer_steps_by_elements_or_bytes.case`.
(D-RAWPTR2=A)

`p.cast<U>()` is the same address as `p`, typed as a `*U`. It takes one type
argument and no value arguments, and it is E3101 outside `#Unsafe`. A cast
moves no bytes and checks nothing where it is made. A read or write through
the `*U` is a raw access at `U`'s size and alignment, so under the memory
sentry a cast pointer that is not aligned for `U` is a located R0803 fault
where it is read. See
`tests/cases/unsafe_and_runtime/a_raw_pointer_cast_reads_the_same_address.case`.
(D-RAWPTR2=A)

`mem.copy(to, from, count)` moves the bytes of `count` `T`s from `from` to
`to` as one memmove, so the two ranges may overlap and the result is as if
the bytes went through a buffer. The copy is bitwise: a `T` that owns a
resource is not cloned, and the places it lands on are not dropped first.
`mem.fill(to, value, count)` stores a copy of `value` in each of the `count`
places at `to`, without dropping what they held, and drops `value` after.
A `count` of zero or less touches nothing. Both are E3101 outside `#Unsafe`.
Under the memory sentry, each whole range (`count` times `T`'s List stride
bytes) is checked before any byte moves, so a range that runs past a live
local is a located R0801 fault. See
`tests/cases/unsafe_and_runtime/a_raw_block_copy_and_fill_move_whole_ranges.case`.
(D-RAWPTR2=A)

`mem.alloc_raw(size, align)` takes a byte size and a byte alignment and gives
`mem.Ptr<U8>?`: `size` fresh, uncleared heap bytes at an `align` boundary, or
`None` when `size` is not above zero, `align` is not a power of two, or the
heap refuses. It is not generic; `p.cast<U>()` gives the typed view, and
`mem.copy` over the bytes is overlap-safe (a memmove). `mem.free_raw(p)`
releases the block. Its parameter takes the pointer (`&&mem.Ptr<U8>`), so the
binding is gone after the call and a second release of it is E0209. Both are
E3101 outside `#Unsafe`. Under the memory sentry the bytes are a live
allocation until their release and are quarantined after it, so a read
through a kept copy of the pointer is a located R0802 fault. See
`tests/cases/unsafe_and_runtime/raw_heap_storage_is_taken_and_released.case`.
(D-RAWPTR2=A)

`mem.window(p, len)` gives a `mem.Window<T>`, a checked window over the `len`
`T`s at `p`; a `len` below zero gives an empty window. `w.len()` is its
length. `w[i]` reads element `i` and `w[i] = v` writes it, through the
`Index` and `IndexMut` hooks, and `w.get(i)` is `None` past either end. An
`i` outside `0..<w.len()` stops the run before any byte is touched: a read
with the hook's index miss, a write with a panic naming the index. The
window keeps `p` unchanged, so its accesses are raw accesses that the memory
sentry still checks for liveness and alignment. `mem.window` is E3101
outside `#Unsafe`, while using the window it gives is not. See
`tests/cases/unsafe_and_runtime/a_raw_window_checks_each_index.case`.
(D-RAWPTR2=A)

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
universal resource operation `close(allocator)`; `.free()` is not the
allocator release spelling. Using an allocator after its move is E0121. The
walkthrough is [`Examples/Memory/Allocation/Arena.jet`](../../Examples/Memory/Allocation/Arena.jet). (D-ALLOC1, D-ALLOC-C, D-ALLOC-D)

The fallible allocation family is separate from plain abort-on-failure calls:
`List.try_new`, `List.try_with_capacity`, `try_push`, `try_reserve`,
`Map.try_insert`, `String.try_push`, and each allocator's `try_alloc` return
`AllocError { requested_bytes, allocator }`. A plain `new`, `alloc`, or
mutation retains its abort-on-failure behavior. The AOT emitter and the
interpreter carry the same fallible result; they do not choose a
different allocation policy. A value `try_alloc` accepts belongs to its
allocator as an `alloc` value does: it closes and drops once, at reset or
close, and the Result carrying its view owns none of it. `AllocError` is a
member of the Core error family, so a refused allocation propagates into the
default `Err` (in `run`, for example) without a declared conversion. The
Prelude's `impl AllocError -> Err` allocates nothing: its message is the
static text `memory allocation refused`. A handler that needs the requested
bytes handles the `AllocError` before it converts. A `Never!` function can't
propagate it (**E2404**). See
[`Examples/Memory/Allocation/TryAllocation.jet`](../../Examples/Memory/Allocation/TryAllocation.jet). (D-ALLOCFAIL1=A, D-ALLOC-FAIL1)

### Arena regions and scope-bound views

`arena.alloc(value)` places a value in retained arena storage and returns a
scope-bound view, not an owned copy. `reset()` mutably borrows the arena and
`close(arena)` consumes it, so the compiler rejects either operation while a
view is live. The runtime has one narrowly contained lifetime-extension helper
inside the memory implementation; that lifetime never appears in user syntax.

A view must stay inside its region and before the arena is reset or closed.
E0631 rejects a view returned, stored in another owner, passed to a `&` or `&&`
parameter, or captured by an escaping closure. E0632 rejects a read after the
arena reset. Regions are implicit and scope-inferred from the lexical scope of
the arena binding. Expert code may use `#Region(r) { ... }` for a named or
narrower region, or one spanning more than one allocator; the same escape rule
applies. Views are non-reassignable, non-escaping locals, and analysis that
cannot prove those properties is rejected. The example and diagnostics are
[`Examples/Memory/Allocation/ArenaRegions.jet`](../../Examples/Memory/Allocation/ArenaRegions.jet),
[`tests/ui/arena_view_escape.stderr`](../../tests/ui/arena_view_escape.stderr),
and [`tests/ui/arena_view_after_reset.stderr`](../../tests/ui/arena_view_after_reset.stderr). (D-ALLOC2, D-REGION1)

## Imports and visibility

### Importing files, modules, and packages

Jet has two import forms: a quoted path names a file, and an unquoted name
names a module.

```jet
use {
    "scoring"
    text: util
    core.files
}
```

A quoted path is relative to the directory of the file containing the `use`.
The `.jet` suffix is implicit. A path cannot escape its project with `..`.
The last path segment supplies the default namespace; `alias: path` gives
either form an explicit alias (S16).

Two or more imports are written as one `use { … }` block with one row per
line, and `pub use { … }` re-exports every row (#6542). A row is any import:
a module path, `path#version`, a member list `path.[a, b]`, a quoted file,
each optionally after `alias:`. A file may hold several blocks, and an
inline module body takes its own. `jet fmt` writes a lone import on one line
and two or more as a block, with re-exports in a separate `pub use { … }`
block.

```jet
use {
    "scoring"
    text: util
    core.files
    biggest: numeric.max
    core.math.[min]
}
```

`alias: path` binds `alias` to whatever `path` names. A path that names a
module (a Core, local, package or foreign language module) is that module
import. A path `owner.name` that names no module, under an owner that declares
the member `name` (a Core module, an inline or local module, the file a
body-less `owner: module` declares, a file import's alias, or a package
dependency), binds that member; any other path keeps its module error. A
member list names its members unrenamed: a renamed member is its own
`alias: owner.name` row. A row takes one alias, so `alias:` with a member
list is an error, and an empty block is an error.

An unquoted module name is resolved by recursively searching from the project
root for `name.jet` and for `name/run.jet`; `core` is a compiler-provided
namespace (S51).
A realized `library` package is another module-search root, so its public
items are used with the same `use package;` and `package.item` syntax. The
package must already be realized: compilation does not realize dependencies on
demand. An `executable` package belongs on `PATH`, not in `use`; refusing one
named in an import is card #6040's work. A declared library that has not been realized is
**E0983**. If a package omits `kind`, its staged `bin/` output or top-level
`run` function makes it an executable; otherwise it is a library. An explicit
kind takes precedence. A single-file `jet run` or `jet build` still requires
an executable entry point and reports **E0101** when it has no `run` function
(U10, U17, D-LIB-USE, D-ILE1).

A dependency package is checked as its own package. When the dependency has
errors, the importer reports them inline, followed by one **E0626** naming the
dependency and the `jet check` that checks it on its own; the dependency's
warnings stay with its own check. A path dependency declared by a dependency
must stay below that dependency's directory (**E1223**) unless the root
package declares the same directory itself, or reaches its sibling directory
through an explicit out-of-tree path dependency. This lets sibling packages such as
`Compiler/JetLexer` and `Compiler/JetFoundation` share a dependency
(D-MOD-CYCLE1). The host reads only the declared, reachable path-dependency
closure; opening that closure never fetches a source or contacts the network.
The UI fixture is
`tests/ui/dependency_package_errors/`.

A package is one namespace (D-MOD-CYCLE1=A). A package is a directory with a
`package.jet`, holding every `.jet` file below it except the files of nested
packages, or a single file that begins with a `package: { }` header. Its
files see each other's top-level names without any
import; a file of a package that imports another file by path is **E0623**,
and a top-level name declared in two files of one package is **E0624**.
Only loose files, which have no header and no `package.jet` above them,
import files by relative path. Files of one package may refer to each other
in a cycle; packages themselves are acyclic, and one package uses another
with `use package.[names]`, which reaches every name of the dependency outside its `priv` and `priv(package)` modules, in
any of its files. An executable package has one `run: fn`, except that each
file an `outputs:` entry names keeps its own `run: fn` (owner ruling B); a
library package has none (**E0625**). An `outputs:` entry `file.callable`
names the member file whose stem is `file`, at any depth, and a callable it
declares. The UI fixtures are `tests/ui/package_file_import/`,
`package_duplicate_name/`, and `library_package_run/`.
Library helpers consumed as `package.helper(...)` are declared at package
scope. Wrapping those helpers in an inline module adds another namespace; a
library output name does not flatten that module's members.

An explicit local import alias selects its bound module, even when a public
Core module has the same short name. Struct and enum identity comes from the
declaring module, not the leaf spelling: two modules' `Limits` records retain
their own fields and are different types, even if their field shapes agree.

Cross-file access is qualified:

```jet
score: scoring.score(91);
```

Every declaration and struct field is public by default and crosses a file
boundary (S18, D-VIS-MODULE1). Privacy belongs to a `priv module`, whose
contents only its enclosing module uses, and to a `priv` struct field, which
only its type's module reads and writes.

```jet
greet: fn() -> String {
    "hello"
}

secret: fn() -> Int {
    42
}
```

The import graph is loaded before semantic checking, so checks cover the whole
program. The relevant diagnostics are **E0602** (path escapes the project),
**E0603** (missing import), **E0604** (import cycle), **E0605** (private item),
and **E0606** (ambiguous module). The executable import example is
`Examples/Modules/Imports/`; the corresponding UI fixtures are under
`tests/ui/import_escape/`, `import_missing/`, `import_cycle/`,
`import_private/`, `import_private_field/`, and `import_ambiguous/`.

### Code modules

Code modules use `module` where Rust uses `mod`, and `.` where Rust uses `::`
(D-MOD1). The path-import form above remains the short form for importing one
file.

```jet
math: module {
    clamp: fn(value: Int, low: Int, high: Int) -> Int {
        if value < low -> low
        else if value > high -> high
        else -> value
    }
}

run: fn() {
    math.clamp(3, 0, 2)
}
```

`math: module;` declares a file module. The loader searches beside the file for
`math.jet`, then for `math/module.jet`. Neither file is **E0607**; finding both
is **E0606**. `math: module { ... }` is inline and adds the `math` namespace to
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

Declarations are public by default (S18, D-VIS-MODULE1, D-VIS-ITEM1). `priv`
is the only hiding form: a `priv` declaration at the top level of a package is
usable by every file of that package, and inside a module by that module and
its children (`priv(package)` there widens it to the package). `priv name:
module` keeps a module's contents to its enclosing module, and `priv(package)
module name` keeps them inside the package; a `priv` struct field is usable
only by its type's module. Reaching a private declaration, module or field from
outside reports **E0605**, importing a member of a private module **E0609**.
These markers are errors whose fix deletes them: an unknown `priv(...)`
qualifier or `priv(package)` on a top-level declaration other than a module
(**E0414**), section labels (**E0415**) and `priv(package)` on a field
(**E0417**). Inline-module bodies are type-checked in their defining scope, so
siblings call each other freely.

A member import names any top-level declaration: a function, type, enum,
trait, tag, protocol, or constant. A constant is declared like any other item,
`MAX_ITEMS: 8`, and is read qualified (`limits.MAX_ITEMS`) or by name after
`use limits.[MAX_ITEMS]` (3A). A `use` inside an inline-module body follows the
same rule.

A module is a value: its `core.meta` `Module` record. `m: text` binds the
`Module` of `text` and `show(lib.inner)` passes that of `lib.inner`. The
compiler fills `name`, `file` (from the package root), `doc` (the `///`
comment at the top of `module.jet`, or above an inline module) and `package`
(its package's `Package`). A module body may open with field lines that set
the rest, `maturity`, `category` and `deprecated`, before any binding:
`category: "billing"`. A field line is checked against its field's type, and
one that sets a compiler-filled field reports **E0111**. `pricing.category`
reads a field, and a module argument to a generic module, as in
`billing<pricing>`, reads as that module's `Module` in the instance. A Core
module has no record, so binding or passing one, such as `m: math` after
`use core.math`, reports **E0629**. A function the module offers is a value:
`f: text.shout` is the function, as `shout` is after `use text.shout`.

A type is not a value: binding, passing or printing a type, such as
`t: Rock` or `print(Rock)`, reports **E0636**. Build a value of the type, or
read a compile-time fact of it such as `Rock.reflect().fields`.

An `impl` head that names a declaration through a module alias binds the alias
to the segment after it: `impl types.Span { ... }` extends `types.Span`, and
`impl Local.types.Named { ... }` implements `types.Named` for `Local`
(D-IMPLDOT1).

A directory module exposes a child only through an explicit Rust-style
re-export:

```jet
pub use wrap.wrap
```

A public declaration that is not re-exported does not become part of the
parent directory's surface, and neither does the child module itself: when
`text/module.jet` re-exports only `shout: wrap.wrap`, reaching the file
from outside, as `text.wrap.wrap("hi")`, reports **E0605**.

A file and the item it re-exports may share a name, as `wrap` does above. The
re-exported item then owns the name in the parent (D-MOD-NAMESPACE1):
`text.wrap("hi")` calls the function, and inside `text/module.jet` the plain
name `wrap` is the function too. The file is still reached where a module is
expected, at the start of a `use` path such as `use wrap.decorate`. Any other
`wrap.member` in the parent reports **E0628**, whose fix brings the member in
with its own `use` line. This holds for `use` and `pub use` alike; every other
pair of equal names stays **E0105**.

Re-exported calls retain the defining function's borrow and move rules.
Examples covering file modules, inline modules,
qualified and grouped imports, and re-exports are in
`Examples/Modules/` (D-MOD4).

### Generic modules

A generic module is a module template with type parameters and compile-time
value parameters. Applying it creates a specialized ordinary module
(D-GENMOD1, D-GENMOD2, D-CONF-GENSPELL1, D-GENMOD-VALUE1, D-GENMOD-BODY1,
D-GENMOD-IDENTITY1).

```jet
cache: module<K: Hash>(capacity: Int) {
    key_of: fn(k: K) -> String {
        "key"
    }
}

int_cache: cache<Int>(64)
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
**E0855**–**E0857**; a specialization identity collision is an internal
compiler error (exit 101; card #521). The
value-argument example is `Examples/Modules/FactValueArguments.jet`.

An instance's member bodies are its template's text, checked once per
instance. An error in one is reported at the instance line
(`int_cache: cache<Int>(64)`), with the template text it came from as a
label; inline-module and instance members are named by their source paths
(`int_cache.key_of`), never by their generated identities. A type parameter
bound to a built-in scalar names that scalar as a static-call owner, so
`K.parse(text)` in an `Int` instance reads as `Int.parse(text)`.

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
`web-app: module`, `env.web-tools`, and `image.devhost-oci`. A hyphen joins
segments only when it is adjacent to both; `a - b` remains subtraction. Code
identifiers—variables, fields, types, and functions—remain plain identifiers.
Leading, trailing, and doubled hyphens are invalid (S84).

Automatic discovery skips a declaration whose declared name begins with `_`:
`_name: module { ... }`. An explicit `use project._name` is still allowed under
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
the LuaRocks reference has the exact `<name>@luarocks#version=<version>` shape;
source SHA-256 values, dependency closure, and the qualified reference are
recorded in `.jet/lock`. Mutable refs, unsupported native-build metadata,
cycles, unsafe archive paths, source drift, and cache tampering fail closed.
Offline reuse verifies sealed Hangar output without contacting the repository
(D-JPK-PROVIDERS2).
Every provider whose metadata Jetpack federates is also a `@source` word. A
direct root without a native realization path, such as `@maven` or
`@homebrew`, fails closed instead of falling through to Nix.

Core and adapted packages can be realized without an installed Nix executable.
A reference without a pinned compatibility output reports **E1272**; foreign
flake projection uses the bounded native evaluator and reports **E1256** for
unsupported expressions. Package search and information commands read local
`.jet/discovery/index.jsonl`, provider fixtures, and Hangar metadata only;
they do not fetch. `jet env info [--env <name>] [--preset <name>] [--json]` is a
read-only report of one selected environment plan: package references, typed
service facts, jobs and checks, variables with source labels, managed files,
and integrations. It does not realize, start, or apply anything (U23, U26/#789).
`jet env fetch-toolchain` downloads this jet's C toolchain pack into the Hangar
ahead of the first `jet cc`, so later C builds work offline
([`reference/cc-driver.md`](reference/cc-driver.md)).

The selected environment facts are also available through the existing LSP
resource bridge as `jet://environment` via `resources/list` and
`resources/read`; the bridge remains read-only and does not enter the lifecycle.

### Workspace overlays

`workspace.jet` may carry reviewed overlay policy inside `workspace: module`.
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
cannot cross. Give ownership to a child with `task &&name`, make an immutable owned
snapshot with `freeze`, or use `Shared` and a lock for deliberate shared
mutation (D-CONC-CROSS1).

Generic code is held to the same rule through its type arguments. A generic
function that moves a value of its type parameter into a task, out of one,
through a channel, into a parallel worker, or into an HTTP handler, directly
or by passing it on to another generic function that does, crosses that
boundary with each call's type argument. A call whose argument can't cross
there is **E1102** at the call, naming the function and the parameter: a
thread-confined value such as `Cell<T>`, or a closure the generic code
captures without giving it away.

### Frozen snapshots and consuming captures

`freeze(x)` creates one deeply immutable, owned snapshot. A task may read it
after the lexical group ends because no task can write through it. Writing the
root, a field, or an index is **E1113**. Freezing a frozen value is the
identity; freezing a value containing a lock-backed `Shared`, a resource, or
another non-clonable value is **E1114**. Other crossing failures remain
**E1102** (D-CONC-FREEZE1=A).
The freeze site follows bindings and place projections, including captured
values. Mutating methods and write windows report the same E1113 with the
original freeze location; a control-flow merge preserves the proof only when
every reaching path carries a frozen value.

`task &&name { ... }` consumes `name` into the child. Using `name` after child
creation is the ordinary **E0121** use-after-move. This capture mark is one of
the three marks written outside a signature (D-CALLSITE-MARK2=A).
`Shared` and `Cell` retain their synchronized and local-only semantics.

The example and golden output are `Examples/Concurrency/FreezeCapture.jet`
and `freeze_capture.out`; UI fixtures are
`tests/ui/frozen_write.jet`, `frozen_capture_use_after_move.jet`, and
`freeze_shared_source.jet`.

### Task completion and cancellation

`handle.join()` consumes a task handle and returns `T !TaskFailure`. Calling
`join()` twice is **E0121**. Dropping a bound `Task` without joining, using its
result, or detaching it is the compile error **L1101**. A failure is `.Panicked(reason)`,
`.Cancelled`, or `.DeadlineBlown`. An error that leaves a task body whose element
is not a `Result` or `Option` stops the task as a panic does: its join reports
`.Panicked(reason)` with the error's text.

`handle.detach()` consumes the handle and lets the task run in the background;
it suppresses **L1101**. Detach is sound only when the child owns all data it
can use. A detached child that captures or returns a `view` borrow reports
**E1106**; another sendability failure reports **E1103** (D-DETACH1).

Task handles expose `pause()`, `resume()`, and `cancel()`. `tasks.yield_now()`
cooperatively yields at a wait point, `tasks.current_task()` returns the
running task's opaque `TaskID` (comparable, printable, unique among running
tasks), `tasks.cpu_ns()` reads the running task's own CPU clock, and
`sender.close()`/`receiver.close()` close channel ends. Pause holds a task at
its next wait point until `resume()`; it is scheduler state, not a flag the
application must poll. A task that reaches no wait point, such as a loop
that only computes, keeps running under the default pause. Task failure uses
`TaskFailure` (D-COROUTINE1).

`pause(mode: .CheckLoops)` is the strong pause (D-TASK-PAUSE-TIER1). Its
argument is the compiler-owned enum `TaskPauseMode`: `.WaitPoints`, the
default, or `.CheckLoops`. A strong pause also holds the task at its next
check. Every loop iteration starts with a check, and so does every call of a
function that can call itself, directly, through other functions or through
a function value. The return from a call that can stop is also a check. Every
engine and optimization level lowers the check from the same rule. Until
some task is stopped from outside, interrupted, cancelled, given a deadline,
strongly paused or asked for its turn, the native check that starts an
iteration or a call costs one load and one branch.

Cancellation is preemptive at wait points and at checks (D-CANCELMODEL1=C,
amended by D-TASK-SLICE1=A). The wait points are channel send and receive,
`time.sleep`, task join, a select arm, and I/O. A cancelled task unwinds at
the next wait point or check, whichever comes first, and runs Drop-backed
cleanup, so a loop or a recursion that only computes stops at its next
iteration or call. A cancelled
receive unwinds instead of returning `Closed`. A busy race loser stops at its
next iteration, so `task.race` returns once its winner does. A cancelled
`task.all` member reports `Cancelled`. Cancelling a task also cancels every
unfinished child of each `task.group` it has open outside `#Shield`, and
through their own groups every unfinished descendant, even when the cancel
arrives while the task waits at a group's closing brace (D-GROUP-CANCEL1=A).
A deadline (`#Context(deadline:)`, `task.timeout`) that passes is delivered
the same way: at the next wait point or check, as E3003, so a handler busy in
a loop stops at its deadline. A task or root that has set a deadline reads
the clock at each check.

When `run` returns, the runtime cancels every live task, however many there
are, and keeps running them until none is queued, ready or running, or until
1 s of monotonic time has passed; the process then exits with `run`'s status.
A task waiting at that point unwinds at its wait and runs its cleanup. A task
that reaches no wait point within the second is abandoned, and the process
exits around it. Leaving tasks parked at exit is not an error. An explicit
`process.exit` or a fatal stop neither cancels nor waits (D-EXIT-TASKS1=A).

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
{ sender: tx, receiver: rx }: channel<Int>()
tx.send(7)
value: rx.receive() ?? panic("channel closed")
```

A second sender is made with `tx.copy()`; there is no combined channel value from
which to fetch one. `send` moves its value into the channel, and
`receive() -> T !Closed` waits until a value arrives or all senders close.
Channel payloads must be sendable (**E1102**). `channel<T>(capacity: N)` adds a
bounded buffer of `N >= 1` values; a literal capacity below 1 is **E0395** and a
run-time capacity below 1 stops the program, never clamping. Seed a channel with
`N` tokens when at most `N` workers may be
active; `Examples/Concurrency/Parallel/BoundedWorkers.jet` demonstrates the
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
For a relative wall-clock budget, derive that absolute millisecond value from
`time.unix_ns() /% 1_000_000 + budget_ms`; `time.now()` returns a civil
`DateTime`, not the deadline's integer epoch.

Code reads the task context as `context.<field>` (D-CONTEXT1).
`context.deadline` is the installed absolute wall-millisecond deadline as an
`Int?`, `None` outside every `#Context(deadline:)` block and `task.timeout`;
a task reads the deadline it was spawned under. `context.thread` is the
running thread's index as an `Int`: 0 on the main thread, and a fiber reads
its carrier thread's index. Each read adds the `Context` effect to the
function's row, so a `-[]>` function or a written row that leaves `Context`
out cannot read the context. Every program holds `Context`: no policy has to
grant it, and `authority: { deny: [Context] }` refuses it. Any other field is
**E0302**. The name `context` reads the
task context only where nothing in scope is named `context`.

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
one: task work()
two: task { work_again() }
results: (task.all { work_a(), work_b() }) ?? []
winner: (task.race { fast(), slow() }) ?? 0
first: (task.any { read_cache(), read_network() }) ?? fallback
```

`task.all` waits for every branch and fail-fast cancels remaining children.
`task.race` returns the first successful result and cancels losers.
`task.any` returns the first completed result and cancels the remaining
children. The combinators consume their children and have no handle-list twin.

`task.all` may name all branches:

```jet
results: (task.all {
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
admission. `task.group name(stack: bytes) { ... }` gives each child at least
`bytes` of stack, and the groups and tasks a child opens inherit it unless
they name their own; every other task gets the system's stack size (the soft
`RLIMIT_STACK`), at least 8 MiB. The runtime caps a request at 8 GiB, and
under a finite address-space limit at a hundredth of it; a constant `stack:`
outside 1 byte to 8 GiB is **E0112**, and any `stack:` on a web target is
**E-WEB-UNSUPPORTED**, since a browser task has no stack of its own. Both
arguments may be given, in either order, at most once each
(D-THREAD-STACK1=A). The group owns and joins every child created in its
body. When the task that opened a group is cancelled outside `#Shield`, the
closing brace cancels the group's unfinished children, waits for all of them,
and then the task unwinds as `Cancelled`; a group opened inside `#Shield` waits
for its children as before (D-GROUP-CANCEL1=A). A `Group`
may be passed directly as a free-function or method parameter:

```jet
add_work: fn(group: Group, value: Int) {
    task value + 1
}

Crawler: struct {
    step: Int

    add_stepped: fn(self, group: Group, value: Int) {
        step: self.step
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

A view or write window whose owner outlives the group is borrowed the same
way: a child that captures it holds a loan on the place it looks into until
the join, and its elements must be sendable. The halves of `split_write` and
windows over constant ranges that do not meet are disjoint. Two children
holding one written window, or a parent read of the lent place inside the
group, report **E1101** (D-TASK-WINDOW1=A).

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
(D-ASYNCRT1=A). A task that has run a whole time slice on its thread while
other tasks wait for one gives its thread to them at its next check, a loop
iteration or a call of a function that can call itself, and continues later,
so busy tasks cannot hold a runnable task back beyond a bounded number of
slices (D-TASK-SLICE1=A).

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
numeric, and encoding operations. `core.encoding` uses one `Dyn` value for
JSON, CSV, TOML, and YAML and supports `#Codable`. Every fallible call returns
`T !E`; handle it with `??`, a pattern test, or contextual `?(text)`. Importing
a core namespace does not by itself emit every helper: code generation keeps
the helpers a program calls (D-CORENS1, D-CORENS-CANON1).

Core surface diagnostics and examples are exercised by the `tests/ui/core_*`
fixtures.

### `Bytes`

`Bytes` is a growable byte builder with one read cursor. End of input is
`position == len`.

```jet
buffer: Bytes.with_capacity(128)
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
writer hierarchy. See `Examples/Systems/Bytes.jet`
(D-ITERTOOLS1=A, #1467).

### `core.math`

`core.math` supplies floating-point and whole-number helpers: the base libm
family, inverse hyperbolics and accurate near-zero forms, classification and
neighbor operations, decomposition pairs such as `sin_cos` and `frexp`,
special functions, checked/saturating/wrapping integer families, and exact
ratios. Exact ratio values expose numerator, denominator, string, float, zero,
and arithmetic operations.

The base libm family's transcendental functions (trigonometric, inverse
trigonometric, hyperbolic, inverse hyperbolic, exponential and logarithmic
functions, `cbrt`, `hypot` and Float `pow`) are correctly rounded Jet source in
Core, so each result has the same bits on every build level and on the web.
The runtime computes only the exactly rounded `floor`, `ceil`, `trunc`,
`round`, `fmod`, `fma` and `sqrt`, in Jet ported from musl 1.2.5, and imports
no C math symbol.

`round` returns the nearest integer with an exact half away from zero
(`-2.5` becomes `-3`, and `2.5` becomes `3`). For `min` and `max`, one NaN
operand yields the non-NaN operand; two NaN operands yield NaN. Examples are
`Examples/Math/MathAudit.jet`, `more_math.jet`, and `fraction.jet`
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
`set_user_ids`, `set_group_ids`, `setpgid`, `setpgrp`, `setsid`, `initgroups`,
`kill`, `reset_signal`, `wait`, `waitpid`, `pipe`, `close_fd`, `mkfifo`,
`mknod`, `umask`, `getpriority`, `setpriority`, `set_scheduling`,
`set_wall_clock`, `utime`, `atexit`, and `stop`. `scheduling`,
`round_robin_interval`, `cpu_time` and `tree_cpu_time` read a process's CPU policy and time
without the gate, as `user_ids` and `group_ids` read its IDs and `users`,
`user`, `user_by_id`, `groups`, `group` and `group_by_id` read the user and
group databases.

These helpers do not emulate POSIX semantics on Windows. Examples are
`Examples/Systems/Process/OSFacts.jet` and `os_process_control.jet`.

### Other core contracts

`core.archive.gzip` and `core.archive.zstd` are the byte-stream codec homes;
`core.archive` owns ZIP and tar container operations and does not re-export a
codec (D-CORE-COMPRESS1=A).

Core source boundaries preserve their ownership and failure contracts:
HTTP response construction moves each selected `Body` and `Headers` owner
once, without flattening streaming bodies. The module loader
catches provider refusal text at each loading stage and maps it to `ModError`;
its lexical context still owns cleanup on every early return. Cryptographic
envelopes reuse the private fixed-limb primitives without exposing new expert
exports, and data value serialization sorts owned object entries without a
closure that consumes the object it must subsequently render.

`Query<T>` is the typed carrier for ordinary list results, checked readers, and
checked SQL. `data.query(rows)` and the SQL form produce the same carrier;
`collect` materializes an ordinary `[T]`. Query operations retain typed
callbacks and plan order. `inner_join` preserves duplicate-key multiplicity;
`left_join` preserves every left row and uses `?R`; `group_by` produces one
`Group<K, V>` per key, in ascending key order, so `K` must be Comparable.
`DataStream<T>` is one-shot and fallible, and `DataLoader<T>` is
stateful and typed. There is no public `Table<T>`, `Series<T>`, `LazyFrame<T>`,
or `DataGroup` carrier (D-QUERY-RETAIN1=A).

## Foreign-function interfaces

### One model for foreign boundaries

A project binder mounts a language root and library name as `<language>.<lib>`.
Generated Jet modules and their provenance live below
`.jet/bindings/<language>/`; generated Jet is ordinary source and can be
inspected. Import a whole library or a member list with
`use alias: <lang>.<lib>` or `use <lang>.[lib, other]`. A generated
descriptor records the ABI
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
and the selected target before lowering. On x86-64, every instruction must name
a mnemonic Jet's template assembler encodes; any other mnemonic is E3221 at
that line. Native code is linked by the native build path. Compile-time code
links no foreign functions, so a raw body never runs at compile time. See
[`Examples/Interop/FFI/InlineC.jet`](../../Examples/Interop/FFI/InlineC.jet)
and [`Examples/LowLevel/InlineAsm.jet`](../../Examples/LowLevel/InlineAsm.jet).
(D-FFI-INLINE1, D-FFI-RAWBODY1, D-FFI-ASM1, D-FFI-CPP1)

### Rust declarations

The existing direct declaration surface is
`extern rust "crate@version" { ... }`. Each entry has a normal Jet signature
and `= "rust::path"`; `extern rust "std"` needs no extra dependency. A
non-`core` crate requires an exact version pin (E0701). By-value boundary types
include scalars, `String`, `Char`, lists/maps/options/results built from allowed
types, and structs or enums whose fields obey the same rule.
Capability parameters use the `&` and `&&` conventions; a raw foreign call that
carries one requires an audited `#Unsafe` boundary (E0702).

A returned resource may declare `#Close(close)`. Its close function must consume
exactly `&&` of the handle and return no value, so `close(handle)` owns one
release. When a dependency is needed, `jet run`, `jet build` and `jet test` build
a hidden cached cargo bridge under `~/.cache/jet/ffi/` or the directory selected
by `JET_FFI_CACHE_DIR`; it does not add a manifest to the user's project.
`jet check` reuses a cached bridge, or type-checks the bridge with `cargo check`
in a private workspace and publishes nothing. Missing cargo is E0703, a
fetch/build failure is E0704, and an invalid foreign path or signature is E0705.
A panic at the foreign boundary becomes the shared runtime report. The worked
surface is [`Examples/Interop/FFI/FFI.jet`](../../Examples/Interop/FFI/FFI.jet);
Rust FFI integration assertions live in [`tests/ffi_rust.rs`](../../tests/ffi_rust.rs).
The descriptor table also records the namespaced `rust.*` and `swift.*`
bridge roots; this direct block is the Rust declaration surface described here.
(S50, D-FFI-CAP1)

### C ABI

`jet inspect bind <header.h> [--pkg <lib>] [--overlay <path>] [--link <lib>] [-o <out.jet>]`
reads bindable C prototypes and writes the cache
`.jet/bindings/c/<lib>.jet` by default. The generated module is marked
`#Bindgen c.<lib>.__bindgen__: module`. A source overlay uses
`#Import c.<lib>: module { ... }`; the effective module is the generated
binding union, with the overlay winning an incompatible redeclaration. A
header declaration that the generator cannot map is skipped and reported,
not guessed. E3208 means the header cannot be read or contains no bindable
prototype; write an explicit overlay for a declaration outside the generated
subset. (S58, S59)
Foreign-module head markers belong to that module's declaration and are parsed
before ordinary item or file-rule replay; they are not unattached file rules.

`#Bindgen` is compiler-generated and legal only in
`.jet/bindings/c/<lib>.jet`; a user overlay uses `#Import c.<lib>: module`.
The reserved `__bindgen__` path is E3206. A second C `use` form for the same
library is E3204; an incompatible overlay is E3205, and handwritten
`#Bindgen` is E3207.

A C call records `FFI.C("<lib>")`, the C leaf scoped to the library key the
call enters: `#Import c.z: module` and the bindings a `z: c@…` dependency
generates both name `z`. When no binding descriptor owns the declaration's
effect row (a written `#Import c.<lib>: module` with no `c@` dependency
descriptor), the checker cannot see what the C body does, so the call also
reaches every effect: written ceilings (E0740) and `deny` rows (E0749) apply
to it, and C `system` under `deny: [Exec]` is refused at check time. A package
`authority: { allow: […] }` row holds such a call by a library grant,
`FFI.C("<lib>")`, the explicit opt-in to that library's code doing anything,
or by the whole-family `FFI.C` or `FFI`: a row without one is E0749 naming
`FFI.C("<lib>")`. A library grant covers its own library only, so
`allow: [IO, FFI.C("c")]` refuses a call into `m`; `deny: [FFI.C("c")]`
refuses calls into `c` under a broad grant. The key belongs to the package
that writes it: a root's `FFI.C("c")` lends a dependency nothing, and a
dependency's library calls are granted by its `authority.grants` line, such as
`grants: { "helper": [FFI.C("c")] }`, or by a whole-family grant (E1220
otherwise). A grant names a language leaf: `FFI("c")` is E1221. An inline
`#FFI(c)` body enters no library and records the bare `FFI.C`, which only a
whole-family grant covers. A call whose descriptor declares its row is held
to that row, and `FFI.C` covers neither another `FFI` leaf nor a Jet-side
effect. Core's own C bindings are toolchain code whose public functions
declare their rows. (D-FFI-EFFECTS1)

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

The end-to-end C binding checks are in
[`tests/cases/cffi/`](../../tests/cases/cffi/).

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
[`Examples/Interop/Languages/Python`](../../Examples/Interop/Languages/Python); the generated
surface and effect spelling are asserted in [`tests/cases/ffi_python`](../../tests/cases/ffi_python/). (D-FFI-PY1)

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

**Evidence.** Use [`Examples/Interop/Polyglot/PolyglotGo`](../../Examples/Interop/Polyglot/PolyglotGo)
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
[`Examples/Interop/Polyglot/PolyglotFortran`](../../Examples/Interop/Polyglot/PolyglotFortran). (D-FFI-FORTRAN1)

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
[`Examples/Interop/Languages/Cobol`](../../Examples/Interop/Languages/Cobol). (D-FFI-COBOL1)

### Java and the JVM

**Binds.** `jet inspect bind java <source.java> [--pkg <lib>] [-o <out.jet>]`
compiles the source and discovers public descriptors with `javap -s`. One
constructor and non-overloaded methods using `long` and `double` form the
supported surface.

**Boundary.** A generated JNI bridge links the provisioned `libjvm`, starts one
JVM lazily in the Jet process, attaches calling threads, and destroys the JVM
at process teardown. Java objects are opaque `java.<lib>.Handle` values in a
bounded 1,024-slot global-reference table. Calls borrow a handle;
`close(handle)` consumes it and releases the global reference. Constructors
and value-returning methods return `JavaError.Exception` on a Java exception;
Java stack text and foreign frames remain inside the bridge. Calls carry
`FFI.Java`, and javac/javap/cc/ar are bounded to 60 seconds with 64-KiB
capture.

**Evidence.** Provenance joins the source, reflected bytecode surface, class
cache, and schema. See
[`Examples/Interop/Polyglot/PolyglotJava`](../../Examples/Interop/Polyglot/PolyglotJava). (D-FFI-JVM1)

### .NET

**Binds.** `jet inspect bind cs <source.cs> [--pkg <lib>] [-o <out.jet>]`
compiles with the provisioned .NET 8 SDK and uses reflection. One public class,
one constructor, and non-overloaded `long`/`double` methods project into a
typed `cs.<lib>` module.

**Boundary.** The native archive embeds CoreCLR through `hostfxr` and
`load_assembly_and_get_function_pointer`; managed entry points use
`[UnmanagedCallersOnly]`. No worker or file protocol participates in calls.
Instances are opaque move-only handles in a 1,024-slot generation-checked
`GCHandle` table. `close(handle)` releases the managed root. Exhaustion,
managed exceptions, and invalid/stale handles become `DotNetError.ResourceLimit`,
`DotNetError.Exception`, and `DotNetError.InvalidHandle`; foreign exception
text does not cross. Calls carry `FFI.DotNet`, with 60-second tool deadlines
and 64-KiB diagnostics.

**Evidence.** Provenance binds the source, reflected surface, hostfxr identity,
and schema. See
[`Examples/Interop/Polyglot/PolyglotDotNet`](../../Examples/Interop/Polyglot/PolyglotDotNet). (D-FFI-DOTNET1)

### Tcl

**Binds.** `jet inspect bind tcl <script.tcl> [--pkg <lib>] [-o <out.jet>]`
compiles a standard-only C bridge against the provisioned Tcl headers and
runtime. `open()` creates an in-process interpreter and evaluates the script
once; `eval`, `eval_int`, and `eval_float` share that session. `eval_once` uses
a fresh interpreter for one call.

**Boundary.** `Session` is opaque and thread-affine. A bounded 64-slot table owns
interpreters; `close(session)` consumes the Jet handle, and process teardown
cleans remaining interpreters before Tcl finalization. String results are
copied through a 64-KiB boundary and reject embedded NUL and oversized values.
Typed Tcl parsing supplies integer and float entrypoints. Tcl failures become
`TclError.Eval`; raw Tcl result text and frames stay inside the bridge. Calls
carry `FFI.Tcl`. Evaluation is synchronous: this surface makes no
cancellation claim for a long-running command.

**Evidence.** Binding tools use a 60-second deadline and 64-KiB capture; source,
runtime, and schema are hashed in provenance. See
[`Examples/Interop/Polyglot/PolyglotTcl`](../../Examples/Interop/Polyglot/PolyglotTcl). (D-FFI-TCL1)

### Lua

**Binds.** `jet inspect bind lua <script.lua> [--pkg <lib>] [-o <out.jet>]`
discovers direct top-level `function name(input)` declarations without running
the script and compiles against provisioned Lua 5.4. Each `open()` owns an
independent in-process `lua_State`; the script runs once for session
initialization and its mutable state persists within that session.

**Boundary.** Generated functions use `Dyn`; sibling
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
[`Examples/Interop/Languages/Lua`](../../Examples/Interop/Languages/Lua). (D-FFI-LUA1)

### Ada

**Binds.** `jet inspect bind ada <package.ads> [--pkg <lib>] [-o <out.jet>]`
reads exported functions from an Ada package specification and compiles its
sibling body with GNAT. Supported Ada exports use `Export`, `Convention => C`, and
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
fixture is [`Examples/Interop/Polyglot/PolyglotAda`](../../Examples/Interop/Polyglot/PolyglotAda). (D-FFI-ADA1)

### Object Pascal

**Binds.** `jet inspect bind pascal <library.pas> [--pkg <lib>] [-o <out.jet>]`
compiles FreePascal `cdecl` exports. `Int64` and `Double` cross as Jet `Int`
and `Float`. A declared class uses exported `<class>_new`, pointer-first scalar
methods, and `<class>_free` wrappers. `jet import pascal <dir>` preserves the
source and emits a binder stub with JT0101 rather than inventing class or
ownership semantics.

**Boundary.** Class pointers never reach Jet. The C bridge owns them in a
bounded 64-slot table and returns opaque move-only identities;
`<class>_close(handle)` consumes one identity. Stale or double close fails
before the Pascal destructor runs, and process teardown destroys remaining
objects before runtime finalization. Calls carry `FFI.Pascal`; compiler and
archiver tools have 60-second and 64-KiB bounds, and native links pin the
Pascal runtime search path.

**Evidence.** See [`Examples/Interop/Polyglot/PolyglotPascal`](../../Examples/Interop/Polyglot/PolyglotPascal). (D-FFI-PASCAL1)

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
[`Examples/Interop/Polyglot/PolyglotDart`](../../Examples/Interop/Polyglot/PolyglotDart). (D-FFI-DART1)

### PowerShell

**Binds.** `jet inspect bind pwsh <script.ps1> [--pkg <lib>] [-o <out.jet>]`
parses named functions with PowerShell 7, maps the conventional `-` separator
to `_` in Jet names, and writes a typed cache. `open()` starts one supervised
`pwsh` worker, waits for its startup handshake, and loads the script once. The
worker accepts only binder-approved function identities; Jet never sends source
or an arbitrary command string. The shipped supervisor is POSIX-only.

**Boundary.** Each call accepts and returns one `Dyn` through a
length-framed structured JSON protocol. Objects, lists, integers, floats,
booleans, text, and null retain their data meaning. Requests and responses are
capped at 1 MiB and depth 64. Exceptions become
`PowerShellError.CommandFailed`; error records, paths, stderr, and stack traces
remain in the worker. Calls carry `FFI.PowerShell`.

**Evidence.** A call has a 1–300000 ms deadline. Expiry or cancellation kills
and reaps the complete process group and invalidates its generation-tagged
session; `close(session)` consumes the handle. At most 32 workers exist per
process. Binding-time tools have 60-second and 64-KiB bounds and E3208
laundered diagnostics. See
[`Examples/Interop/Languages/PowerShell`](../../Examples/Interop/Languages/PowerShell). (D-FFI-PWSH1)

### Perl

**Binds.** `jet inspect bind perl <script.pl> [--pkg <lib>] [-o <out.jet>]`
uses compiler metadata to discover named main-package `sub` declarations without
running the top-level body. Foreign names project to Jet `snake_case`, while
the worker calls the exact Perl name. `open()` starts one supervised POSIX
worker, loads the script once, and retains package and lexical state. Generated
entries carry a fixed allowlist.

**Boundary.** Each entry accepts and returns one `Dyn` through Perl's core
`JSON::PP`. Requests and responses are length-framed and capped at 1 MiB;
stdout is not a result channel. Exceptions become
`PerlError.CommandFailed`; stderr, paths, stack traces, and exception text stay
inside the worker. Calls carry `FFI.Perl` and do not accept runtime source.

**Evidence.** Calls use a 1–300000 ms deadline. Timeout or cancellation kills
the process group; generation-tagged handles reject stale identities and
`close(session)` consumes a session. At most 32 workers exist. CPAN
realization is provider work. Binding tools have 60-second and 64-KiB bounds
and E3208 diagnostics. See
[`Examples/Interop/Languages/Perl`](../../Examples/Interop/Languages/Perl). (D-FFI-PERL1)

### Ruby

**Binds.** `jet inspect bind ruby <script.rb> [--pkg <lib>] [-o <out.jet>]`
uses Ruby's `Ripper` parser to find direct top-level methods without executing
the file. A bindable method has one required positional argument and a
Jet-compatible name. The generated allowlist fixes the callable identity;
source and arbitrary commands never cross the API.

**Boundary.** `open()` starts one supervised worker and loads the script once,
retaining its state. Methods accept and return `Dyn` through Ruby's
standard JSON library. Length-framed messages are capped at 1 MiB and stdout
is not the result channel. Ruby exceptions become
`RubyError.CommandFailed`; exception text, traces, stderr, and paths stay
inside the worker. Calls carry `FFI.Ruby`.

**Evidence.** Timeout and cancellation use a 1–300000 ms deadline and kill the
process group; `close(session)` consumes a generation-tagged session, with at
most 32 workers. Compiler and archive tools have 60-second and 64-KiB bounds.
RubyGems installation remains provider work. See
[`Examples/Interop/Languages/Ruby`](../../Examples/Interop/Languages/Ruby). (D-FFI-RUBY1)

### PHP

**Binds.** `jet inspect bind php <script.php> [--pkg <lib>] [-o <out.jet>]`
requires a POSIX supervisor, lints the script, and discovers top-level named
PHP functions. A bindable function takes exactly one required positional
argument by value; references, defaults, variadics, nested functions, and
unsupported generated names are rejected. The generated cache uses a native C
pool bridge.

**Boundary.** PHP calls accept and return one `Dyn` over a length-framed
JSON protocol. The pool has four workers per pool and eight generation-tagged
pool slots. A worker allowlist fixes the function identity; stdout is not the
result channel. `PhpError` distinguishes not-running, timeout, cancellation,
protocol, command, and resource-limit failures. Calls carry `FFI.Php` and do
not expose PHP exception text or source paths.

**Evidence.** Worker frames and responses are capped at 1 MiB. A call deadline
or cancellation replaces the affected workers; `close(pool)` consumes the
pool handle. Binding lint, C compilation, and archiving use bounded diagnostics
and the descriptor/provenance record. See
[`Examples/Interop/Languages/PHP`](../../Examples/Interop/Languages/PHP). (D-FFI-PHP1)

### R

**Binds.** `jet inspect bind r <script.R> [--pkg <lib>] [-o <out.jet>]`
parses the script without running its top-level body and binds direct named
functions with one required argument. `open()` starts a supervised worker,
loads the script once, and retains its state. A normal function round-trips
`Dyn`; `<name>_table<T>` maps ordinary `[T]` rows to a data frame and back
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
[`Examples/Interop/Languages/R`](../../Examples/Interop/Languages/R). (D-FFI-R1)

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
[`Examples/Interop/Languages/Octave`](../../Examples/Interop/Languages/Octave). (D-FFI-OCTAVE1)

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
`Dyn`. Dynamic name-based `IDispatch` is available only inside explicit
`#Unsafe`; generated safe calls use fixed DISPIDs. Each live object owns a
single-threaded apartment and a generation-tagged handle tied to its creating
thread. `close(object)` releases it and balances `CoUninitialize`; stale,
cross-thread, and double-close uses fail before invocation. HRESULT and
EXCEPINFO become `ComError` variants without vendor text. Calls carry
`FFI.Com`; frames and Dyn depth are capped at 1 MiB and 64.

**Evidence.** Provenance hashes the extracted type-library schema and generated
surface. A released file-backed binding may omit the original `.tlb`, but when
the input remains available its bytes must match the recorded hash. See
[`Examples/Interop/Polyglot/PolyglotCOM`](../../Examples/Interop/Polyglot/PolyglotCOM). (D-FFI-COM1)

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
[`Bindings.jet`](../../Compiler/JetCli/Source/Cli/Bindings.jet) and
[`InspectB.jet`](../../Compiler/JetCli/Source/Cli/InspectB.jet). (D-BOUND-BIND1, D-NAME-FILES1)

## Browser effects and web values

`core.web` is the server-rendered web application surface over `core.http`; its
`App`, pages, sessions, forms, and live records are Jet values. Browser-owned
operations carry the `Browser` effect and are lowered by the web target. The
source authority is [`Core/Web/Web.jet`](../../Core/Web/Web.jet), while the
browser test controller is [`Core/Browser/Browser.jet`](../../Core/Browser/Browser.jet). (D-FLAGSHIP-WEBAPI1)

### Browser events, values, and storage

Use `core.browser.host` and `core.browser.storage` for the checked browser
operations:

- `host.on(target, event, handler)` registers a DOM event handler. The handler
  receives `WebEvent`; a handler that does not need it can ignore the argument.
- `host.value(target) -> String` reads the selected input or element value.
- `core.browser.storage.local.get(key) -> ?String` and
  `core.browser.storage.session.get(key) -> ?String` read origin-local and
  tab-scoped storage. `set`, `remove`, and `clear` mutate the corresponding
  namespace, and `get_or` composes a missing key with a fallback.

For example, the browser conformance surface imports `host: core.browser.host`
and `core.browser.storage.local`, then uses
`host.on("#new-task", "input", handler)` and
`local.set("draft", host.value("#new-task"))`. The generated web
runtime uses `addEventListener`, DOM selection, `localStorage`, and
`sessionStorage`; native codegen provides inert checked routes for the browser
APIs. The round-trip is exercised by
[`tests/cases/web_build`](../../tests/cases/web_build).

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
`facts` read values and bounded lifecycle facts. `invalidate(q)` marks a
query stale and advances its generation. `refresh(q)`, `queue(q, data)`, and
`retry(q, replay)` are fallible operations; `cancel(q)` returns whether an
in-flight operation was cancelled. `mutate` checks the key and optional
expected value, applies the mutation, and returns a `WebMutationState`;
`mutate_with_invalidations` additionally validates target keys.

Keys are at most 256 bytes, payloads at most 1 MiB, and subscribers and
invalidation targets are bounded by the Core limit of 1,000,000. Reusing a key
with a different dependency footprint is a runtime contract error (card
#3709): a key must have one invalidation declaration. Offline mode reports
`WebQueryError.Offline` rather
than pretending that a mutation was durable. The offline-first runtime persists
queued payloads and invalidation targets under
`$JET_WEB_QUERY_QUEUE_PATH`, `$XDG_STATE_HOME/jet/web-query-queue.v1`, or
`$HOME/.local/state/jet/web-query-queue.v1`; it uses a temporary file and atomic
rename and reports a durability error when no usable state directory or queue
file is available. Query state and queue transitions are defined by
[`Core/Web/Query.jet`](../../Core/Web/Query.jet). (D-WEBQUERY1)

### Events and hooks

`use event: core.event` exposes typed compiler-known event values; it does
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
use event: core.event

run: fn() {
    scope: event.scope()
    clicked: event.new<Int>()
    clicked.on(scope, fn(n) { print("clicked {n}") })
    clicked.once(scope, fn(n) { print("once {n}") })
    print(clicked.emit(1).summary())
    scope.cancel()
}
```

With `JET_OBSERVE=1`, the runtime publishes one bounded, payload-free sequence
for Event, AsyncEvent, and DecisionHook transitions. `jet inspect live` and a
Canvas session attached to the live Jet identity consume that validated source;
a source-call match alone is not a runtime observation. The observation writer
bounds event history and does not include channel values, task locals,
environment, or credentials. Observation is pay-for-use (D-SIZE-PAYUSE1): a
program built without tasks, channels, live values or events links no writer,
so `JET_OBSERVE=1` publishes nothing for it; `jet run --observe` links the
writer into any program and runs it with `JET_OBSERVE=1`. The public API is
[`Core/Event/Event.jet`](../../Core/Event/Event.jet); the executable event
example has no running test since the Rust-era `Canvas*.jet` transliterations
were removed (#6650). (D-OBSERVE-LIVE1, D-OBSERVE-TASK1)

## Terminal direct input

A `#Live { ... }` block enters unbuffered, no-echo terminal input for its body.
An RAII guard restores the terminal on normal return, `?` propagation, and panic
unwind. The marker itself does not require an import; `term.read_key()` does.

```jet
use term: core.term

#Live {
    key: term.read_key()
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
reports files that would change and exits nonzero without writing them;
`--diff` adds unified diffs and is itself a check, so it never writes either.
`jet fmt -` reads source from stdin
and writes formatted source to stdout; `--stdin-path=<label>` supplies the
reported source name. These commands run the lexer, parser, and printer, not
sema. (S44)

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

Source-edit migrations reject out-of-range, overlapping, or reversed spans and
spans that split a UTF-8 scalar. No edit is applied when the edit set is invalid.

`package.jet` is formatted by its typed closed-record formatter rather than the
ordinary Jet formatter. If that path cannot place an authored comment safely,
it fails closed instead of declaring the file clean. A leading inline package
carrier is preserved byte-for-byte, including its source spans; only the Jet
source after the carrier is formatted. The driver preflights every selected
file before writing any file, so one read or parse failure leaves the batch
unchanged. The formatter contract is exercised by
[`tests/cases/fmt/`](../../tests/cases/fmt/); its idempotence law is
`fmt(fmt(source)) == fmt(source)`.

## Writing and running tests

A top-level `#Test("name") { ... }` block is a test claim. Its body has ordinary
function statements and uses `assert`, `assert_eq`, snapshots, or another
checked assertion. Duplicate names report E0105; a nested `#Test` reports
E0601. `jet run` and `jet build` ignore test claims, while `jet test` compiles
and runs them. (S43, D-CASING1)

A top-level `#Test name: fn(p: T, ...) { ... }` is a property test. Each
parameter type must be generatable: `Int`, a sized integer, an inline range,
`Float`, `Float32`, `Bool`, `Char`, `String`, or a list, fixed list, or
optional of a generatable type; any other type reports E0613. `jet test` runs
the body on 200 generated cases, each behind the same unwind boundary as a
claim; the generated grade, `jet test --grade=generated`, runs each property
on `--iterations=<n>` cases instead (1000 when unset) from the base seed
`--seed=<n>` (the fixed 0 when unset, unless `JET_PROP_SEED` names one), so a
graded run is reproducible; `iterations` or `seed` without `grade` is refused.
Every generated value comes from one recorded sequence of bounded
choices. The first failing case is shrunk: spans of choices are deleted and
single choices lowered while the case still fails, so integers move toward
zero and lists toward fewer, simpler elements. The failure report names the
failing case, the run seed, the minimized inputs (`minimized_inputs: n = 50`,
each parameter as `name = value` in Debug form), and the minimized case's own
report; only that case's output is
captured. Outside the generated grade a seed is fresh entropy unless
`JET_PROP_SEED` (an integer from 0 to 2^63-1) names one; rerunning with the
printed seed reproduces the run.
A `#Pre(condition, "reason")` marker on a property test is its precondition: a
generated case that fails it is rejected before the body runs, never counts
toward the case budget, and is never a shrink target; a failure report names
how many cases the precondition rejected, and a property whose rejections reach ten
times its budget fails with a fix instead of passing on too few cases. Each
property's evidence record counts only the cases that ran and attaches its
law: `precondition`, `partitions` (each parameter as `name: Type`),
`relation` (the body's assertions), `surfaces` (the functions and methods the
body calls), `sink` (a runtime stop of the body), `shrink_rule`, `seed` and
`rejected`.
(D-TEST1)

For an explicit file, `jet test <file.jet>` builds one generated AOT harness for
that file. Each claim runs behind a process-local unwind boundary, and the
harness emits one result per claim plus a summary. Human output names `pass`,
`FAIL`, `skip`, expected-fail, and unexpected-pass outcomes; JSON mode emits the
same result categories. `jet test` exits with status 1 when any selected claim
fails. A failing assertion carries the E3001 report and Jet
source location. A test target with no tests or doctests reports E0601; bare
package testing skips source members with neither, then reports the resolved
package contains no testable member at all. (R9)

A package's `policy: { claims: { min: <rung> } }` sets its claims floor
(D-GRADE-POLICY1). The rungs, weakest first, are `.Unchecked`, `.Checked`,
`examples(n)`, `generated(n)` and `.Proved`; a count is part of the rung. When
nothing failed but the run's grade is below the floor, `jet test` and
`jet prove` report E2942 and exit 1. Its fix names the evidence to add: more
passing `#Test` examples, property tests (200 generated cases each), or
`jet prove FILE --lens solver`. A run with a failure gets no E2942, because the
failure already fails it and names its own fix. `jet prove` reaches `.Proved`
only when the solver proves every obligation it found; its ProofReport carries
`summaries.claims.floor` and `floorMet` when a floor is declared. `jet inspect
claims` shows the grade of the newest `.jet/evidence` report for the target's
current source, the floor, and whether it is met.

Like `contain` and `harden`, the floor is a package-only key with no
`#Policy` mirror (`#Policy(claims)` is E0355), and it only tightens. A member
of a Package root with `members:` inherits the root's floor when it sets none
and may raise it; a member whose `claims.min` is below the root's is E1215 in
`jet test`, `jet prove` and `jet inspect claims`. No organization scope holds
a floor.

A fenced ```` ```jet ```` block in a `///` doc comment is a doctest: a claim
named `doctest <n> of <item> in <file>` in the same harness, where `<item>` is
the item the comment documents (`Outer.name` for a member) and `<n>` counts
that item's doctests from 1; a comment that documents no named item gives
`doctest <n> of <file>`. An edit elsewhere in the file keeps the name. The
claim is compiled after the file's own items so it can call them. Its lines
run in order; each `EXPR // => VALUE` line compares the Display rendering of
`EXPR` with `VALUE` exactly, and a mismatch fails the doctest with E2901 at
that line. With
`--update-snapshots` a mismatched one-line rendering rewrites the claimed
`VALUE` in the file instead. (D-TEST4)

`expect(value).snapshot()` in a `#Test` asserts that the Display text of
`value` equals the snapshot `__snapshots__/<file>_<line>.snap` beside the test
(`run.jet` line 8 is `run_jet_8.snap`). It is
`assert(core.testing.snap(name, "{value}"))` and needs no import: the first
run writes a missing snapshot and passes, a different value fails with
`snapshot differs: <name>` and a unified diff while the file stays as it was,
and `jet test -u` (`--update-snapshots`) rewrites the snapshot with the new
value. A local or function named `expect` shadows the builtin. (D-TOOL4,
D-TESTKIT1)

Bare `jet test` and `jet test <package dir>` discover every source member in the
package's checked project set, not only the entry file, and run the aggregate
`#Test` and contract harness exactly once. Documentation examples remain
file-local and run once per member. A directory without a package manifest is
walked recursively for `*.jet` files. A package-level `test: fn` override owns
the command unless `--show-default` requests the stock harness.
The default child-output policy is `--capture=failed`; choose `all` or `none`
explicitly. Every `jet test` runs its tests: no result is stored, and no
stored result stands in for a run. `--docs` selects checked documentation examples.
`--watch`, `--where=<expr>`, `--filter=<substring>`, `--shuffle[=<seed>]`,
`--serial`, `--coverage`, `--update-snapshots` (or `-u`), `--release`, and
`--profile=<name>` select the corresponding test-run behavior. Browser claims
use `--browser=<engines>`, `--browser-retries=<n>`,
`--browser-reporter=<text|json|html>`, `--browser-ui`, `--browser-visual`, and
`--browser-trace`.

A package names test profiles in `package.jet`, and `--profile=<name>` or
`defaults: { test: <name> }` selects one. Test and build profiles share one
namespace, so a test profile can't take the name of a stock or declared build
profile. `tests` is the built-in suite, every `#Test` of the package; stock
suites are declared in `suites:`, and code suites by `t.suite("<name>", body)`
in `test.jet`'s `test: fn`. A profile naming a suite none of these declares, or
a name declared twice, is a manifest error (E1215) that `jet check` and every
command loading the package report, as is a `defaults.test` that names neither
a test profile nor a Check output.

```text
test: {
    quick: TestProfile{ suites: [tests], time_limit: 60s },
    full: TestProfile{ suites: [tests, golden], optimize: [none, full] },
}
suites: { golden: TestSuite{ golden_corpus: "tests/golden" } }
defaults: { test: quick }
```

`time_limit:` (`ms`, `s` or `m`) stops and fails each test that runs longer.
`optimize:` runs the `tests` suite once per listed level, and runs a golden
corpus at the matching tiers (none is `jet run`, basic `jet build`, full
`jet build --release`). Each run's rows are its baseline in
`.jet/test/<profile>/`. A failing test fails the run whatever the baseline
holds; the baseline names the rows that moved, a test that passed in it and
now fails or is gone is lost and also fails the run, and `-u` records the run
as the new baseline. A suite with
`golden_corpus:`, `ui_corpus:` or `cases_corpus:` runs in the `jet` process
itself, every entry of the corpus a row; `jobs:`, `memory:`, `filter:`,
`shard: I/N`, `opt_in:` and `baseline:` (a ledger of the rows accepted as
failing) shape that run, a filter argument after the package replaces
`filter:`, and `-u` rewrites the selected rows' expected output
(`reference/cli.md`).

A code suite checks what no corpus folder describes, such as a binary-size
budget (D-TEST-SUITE2=A). Its name is a string literal (E0635), so a profile
run lists the suites without running code. `body` takes a `TestRows` and
reports each row as its check ends: `rows.pass(id)`, `rows.fail(id, detail)`
or `rows.skip(id, reason)`. A profile runs the suite in its own child, which
runs no `#Test`, so a package with no `#Test` still runs its code suites
(no E0601); a plain `jet test` skips it. The child's environment names
the `jet` running the profile in `JET` and the package's directory in
`JET_TEST_SUITE_ROOT`, so the suite drives the compiler under test. A run's
`filter` reaches the suite as `rows.selects(id)`, which holds only for an id
containing the filter. `pass` and `skip` drop every other id, and a suite asks
`selects` before doing a row's work; a failing row is always kept. The rows
are compared with the suite's baseline like a run's, a repeated id fails, and
a suite that panics keeps the rows it reported and adds the failing row
`stopped`.

```text
test: fn(t: Test) -> Int {
    t.suite("perf", perf_rows)
    t.run()
}

perf_rows: fn(rows: TestRows) {
    size: built_size("hello")
    if size <= 24_000 -> rows.pass("size hello") else -> rows.fail("size hello", "{size} B > 24000 B")
}
```

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
  The harness runs the region many times, so it is checked as a loop body:
  moving a binding declared outside it is E0121 at the move, as a move a
  `loop` body repeats is.

`jet new <name>` creates a new simple project directory and refuses an existing
path. It writes `package.jet`, a print-only `run.jet` with one test claim, a
`.gitignore`, and a toolchain lock record. `--template cli|ui|web|overrides`
selects a richer starter: a typed CLI entry, a native UI tree, the browser app,
or the print-only entry plus the commented command homes
`build.jet`/`dev.jet`/`test.jet` beside it, `run.jet` being the `jet run`
home itself (D-NEW-TEMPLATE1); an unknown
name is E2104. `--annotated` includes commented example dependencies.
The same command family also provides `jet new service|route|job|migration` for
backend source scaffolds; those subcommands have their own `--path`, `--route`,
`--model`, SQL, risk, lock, and preview/apply options. The `new` command
implementation is in [`Front.jet`](../../Compiler/JetCli/Source/Cli/Front.jet);
the backend scaffold subcommands are not yet ported and report that.
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

Each measured claim yields one record: its name, whether it passed, the tier,
profile, warmups, iterations, serial mode, sample count, and the minimum,
median, mean, and maximum time per iteration in nanoseconds. Text prints one
`measure NAME:` line per record, and `--json` prints the same fields in the
same order as one JSON object per line. The record is also the evidence for
the file's typed budgets whose scope is `.Test(name)` (metric `BenchTime`,
provider `BenchMeasurement`), and they are evaluated in the same report frame.
For an `.AbsoluteFrom` `.AtMost` budget, the named percentile of the sample
times is compared with the limit. A miss is E2907, which fails the claim under
`.Fail` and is only reported under `.Warn`. A `.RelativeTo` budget needs a
recorded baseline, and `jet test` keeps none, so it reports E2906 and fails
nothing. Measurement writes no evidence store and feeds no other budget store.
`--measure` builds with the release profile unless `--profile` selects another.

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
[`Examples/Tooling/Performance/Bench.jet`](../../Examples/Tooling/Performance/Bench.jet),
and the test command contract is covered by
[`tests/measurement_tiers.rs`](../../tests/measurement_tiers.rs). Compiler
build speed is measured separately, by the throughput lanes in
[Compile throughput](reference/compiler-speed.md#compile-throughput), with
their own warmup and sampling policy. Do not use those numbers as a substitute
for the `.measure` claim's execution record.

## Size-oriented builds and language tooling

### Build options

Build options come from one table (`JET_BUILD_OPTIONS`, #5005). Each row is
settable in three places that spell it the same way: a profile field in
`package.jet` (`build: { release: Build{ optimize: full } }`), a call in
`build: fn` (`b.optimize(.Full)`), and a flag (`--optimize=full`; the `=` is
required). Layers apply in order over the command's stock values (`jet build`
optimizes `basic`; `jet run`, `jet dev` and `jet test` optimize `none`):
profile, then `build: fn`, then the flag. A flag that replaces a profile or
`build: fn` value prints one `note:` line naming the replaced value and where it
was written. `b.pin(.optimize, .Full)` fixes the value for every build; a flag
with another value is E2113. `jet run` reads profiles and flags, not
`build: fn`. Example:
[`Examples/Tooling/Build/BuildOptions`](../../Examples/Tooling/Build/BuildOptions/run.jet).

The `bounds` row says what a failed list bounds check does: `fatal` (the
stock everywhere) stops the program, `report` writes the failure to stderr and
goes on with the access unchecked, `off` drops the check. It applies only to
the built package's own code; dependencies and Core keep their fatal checks.
`report` and `off` remove a memory-safety check, so each is a `check_removal`
gate (see One audited-gate law): the command prints a `note:` naming it
(`--quiet` drops it), `jet inspect gates FILE --bounds=off` lists it, and a
package or organization `policy: { check_removal: .Forbid }` refuses it with
E3415 before the native program is built. Jai's `null` and `casts` checks have no row: a
reference is never null and a raw pointer's validity is the `#Unsafe` author's
obligation (I1), and a conversion is total or returns an Optional
(D-SHAPE-CONVERT1), so there is no check to switch. A profile field, flag or
`build: fn` call of either name is refused with that law. Example:
[`tests/run/Tooling/BuildBounds`](../../tests/run/Tooling/BuildBounds/run.jet).

The `debug_info` and `small` rows hold a Bool: a profile field writes
`debug_info: false`, `build: fn` writes `b.debug_info(false)`, and the flag
writes `--debug-info=false`; a bare `--small` or `--debug-info` means `true`.
`debug_info` (stock `true`) says whether the image carries DWARF debug info
and the location table crash reports read (`.debug_jetline`). Both describe
only the functions the image holds: the link drops the debug records of the
functions nothing reaches.
With `false` the image has no `.debug_*` section; `.eh_frame` stays, and a
crash report prints the faulting address and the return addresses of its
frames instead of function names and lines.

`debug_info` also takes `split`, the value of the stock `release` profile
(`--release` is `optimize: full` and `debug_info: split`; a `release` profile
in `package.jet` replaces both). Under `split`, `jet build` writes the DWARF,
the location table, the symbol table and the section headers to a side file,
`<image>.debug` beside the image, and files the same bytes in the package's
symbol store as
`.jet/symbols/.build-id/<first two hex digits>/<rest>.debug`. The image keeps
its loaded segments and a GNU build-id note under one `PT_NOTE` header, with
section headers naming only that note; the build id hashes both files, so
debuggers and profilers given the store (`gdb`'s
`debug-file-directory`, `perf`'s build-id lookup) find the side file. An
image that links no unwinder (no code of it unwinds the stack; a crash report
walks frame pointers) moves its call frame information to the side file too,
as `.debug_frame`, so its loaded segments hold no `.eh_frame`; with an
unwinder `.eh_frame` stays loaded. A crash report of the image prints
addresses: its first line, then `Build <id> (names and lines: jet crash)`
with the image's build id, then `Backtrace:` with the faulting address and,
under `backtrace: full` or for a stack overflow, the return addresses
(`backtrace: off` prints no frame). `jet crash < report.txt`, run in the
package that built the image or a directory below it, reads that report,
finds the side file in the symbol store and prints the report a
`debug_info: true` build of the program prints: the same functions, lines and
source. A report with no `Build` line is E2114, and a build id the store
doesn't hold is E2115.
`jet run`, `jet dev` and `jet test` keep the debug info in the image under
`split`, as under `true`, so their crash reports name functions and lines.
`backtrace: full` stays legal with `split`. A user who wants the debug info
in a release image writes `--debug-info=true` or `debug_info: true` in a
`release` profile.

### Deep checks

Every deep check is off by default, in every profile (D-DEEP-CHECK1). `jet
run`, `jet dev`, `jet test` and `jet build` take `--deep-checks`: the bare
flag keeps every deep check of the program and its Jet libraries, and
`--deep-checks=<scope>` keeps those of one declaration and everything under
it. A scope is a package name, then module, type and function names, joined
by `.` (`--deep-checks=tree.run`); a file outside a package starts at its
module. The flag repeats, and the scopes add up. An empty or malformed scope
is E2119 before the compile starts. After checking, a scope that names no
declaration is E2120, and one whose declarations hold no deep check is
E2121, even beside a bare flag or another scope that keeps one; their fix
offers the scopes that do. The selection is part of the compile's identity,
so a cached image built with one selection never serves another; equal
selections written in any order share it. Example:
[`tests/ui/deep_checks_unmatched_scope`](../../tests/ui/deep_checks_unmatched_scope.jet).

### Size-oriented builds

`small` (stock `false`) favors a smaller binary: with `true` the optimizer
inlines only within the O1 limits (callee size and per-caller budget, and no
once-called callees) at any `optimize` level, so inlining grows the code no
more than `optimize: basic` does. `jet build --small` is `--small=true`. Jai's
`min_size` has no row, since the native backend has no separate size pass. The
command-line help describes the flag as “Favor a smaller binary”; it does not
promise a particular byte-size ratio (S15). Example:
[`tests/run/Tooling/BuildDebugInfo`](../../tests/run/Tooling/BuildDebugInfo/run.jet).

`inline` says which callees the optimizer inlines at `optimize: basic` and
`full` (`none` inlines none). `auto` (the stock) picks them by size, within the
level's limits. `always_marked` takes only the functions marked
`#Inline(Always)`, at every direct call the inliner can splice, whatever their
size and with no per-caller budget (`small` does not limit them). The inliner
cannot splice a recursive callee, a callee that writes a parameter, an
`#FFI(asm)` leaf, or a callee from another file whose arithmetic can stop.
`off` inlines nothing. `build: fn` writes `b.inline(.AlwaysMarked)`, and the
flag writes `--inline=always_marked`. Example:
[`tests/run/Tooling/BuildInline`](../../tests/run/Tooling/BuildInline/run.jet).

### Fault reports and unreached functions

`backtrace` (stock `full`) says what a native fault report prints after its
first line. `full` prints every Jet frame with its file and line, read from the
image's debug info. `brief` prints only the faulting frame. A stack-overflow
report names the innermost Jet function and its line under both `full` and
`brief`. `off` prints no frame: the report is the stop line alone, for example
`Stop [E3012]: Stack overflow`. Naming frames needs `debug_info`, so writing
`backtrace: full` (in a profile, in `build: fn` or as `--backtrace=full`)
together with `debug_info: false` is refused with E2104. The stock `full` under
`debug_info: false` prints the return addresses instead.

`dead_code` (stock `remove`) says whether the image keeps functions the entry
never reaches. `remove` builds only the reached ones, and for `jet build`,
`jet run`, a script and `jet dev` it lowers only those of the package's own
modules: the ones its roots reach. The roots are the entry, its jobs, every
function under a marker (`#Export(c)` among them) or that the compiler
generates, and a foreign call's callback, `#Close` or `undo` function; a
function passed as a value is reached through the body that names it. `keep`
also builds every non-generic function of the package's own modules, so the
image carries them (a generic function is still built only for the instances
a call reaches). A dependency package's function is lowered only when a
lowered body reaches it, under either setting and in every command but
`jet test`, as a Core function is: an unreached one costs the build nothing
past its check.
`build: fn` writes `b.backtrace(.Brief)` and `b.dead_code(.Keep)`, and the
flags write `--backtrace=brief` and `--dead-code=keep`. Example:
[`tests/run/Tooling/BuildBacktrace`](../../tests/run/Tooling/BuildBacktrace/run.jet).

`checked_alloc` (stock `false`) holds a Bool. With `true` the program links
the runtime's checked memory allocator and runs with it from its first
allocation: every heap block carries guard words and the call chain that
allocated it, and a double free, a free of a pointer the heap never handed
out, a free with another size or alignment than the allocation's, a write
past a block's end or a write into a freed block (freed blocks wait in a
quarantine and are checked as they leave it and when `main` returns) stops
the program with a report that names the block and the call sites, then
aborts. With `false` the image holds none of the checked allocator, and the
program reads nothing at start to decide it. `jet build --checked-alloc`,
`jet run --checked-alloc` and `jet test --checked-alloc` are `true`, and
`build: fn` writes `b.checked_alloc(true)`. The row is part of every build
key, so a checked build never reuses an unchecked one's outputs. A web build
has no checked allocator: `true` with `os: web` is refused (E2104).

### Output places, link inputs and targets

The `output_dir`, `libs`, `link_args`, `os` and `min_os` rows place the
executable, add native link inputs and name the target. `output_dir` and
`min_os` hold a string (`output_dir: "dist"`, `b.output_dir("dist")`,
`--output-dir=dist`); `libs` and `link_args` hold a list of strings
(`libs: ["z"]`, `b.libs(["z"])`), and each flag adds one item
(`--libs=z --libs=bz2`). `build: fn` writes a string or list literal.

`output_dir` (stock `""`, which is `.jet/build`) is the directory `jet build`
writes its executables to, under the package root unless it is absolute. It
places a product without changing its bytes, so no cache key names it.
`jet run`, `jet dev` and `jet test` write no executable for it to place, and
a value written for them is refused with E2104.

`libs` names more C libraries the native link resolves foreign symbols
against. Each is a static archive, found like an undeclared `#Link` library:
in a `link_args` `-L` directory, then through `pkg-config`, then on the C
compiler's search path. `link_args` adds `-L<dir>` (a directory searched
first), `-l<name>` (a library, as in `libs`) and `.a` archive paths, under the
package root unless absolute. The native linker takes no flags and reads `ar`
archives only, so any other word is refused with E2104. Link inputs the
program's own declarations don't name are a `link_override` gate (see One
audited-gate law): `jet inspect gates FILE --link-args=-lz` lists it, and a
package or organization `policy: { link_override: .Forbid }` refuses it with
E3415 before the program is linked.

`os` (stock `linux`; also `macos`, `windows`, `web` and `freestanding`) is the
operating system the image runs on. The native backend writes Linux x86-64 ELF
images, so every other value is refused with E2104. `min_os` (stock `""`) is
the oldest OS version the image runs on; a Linux ELF image has no versioned
ABI floor, so any value is refused with E2104. Example:
[`Examples/Tooling/Build/BuildLinkInputs`](../../Examples/Tooling/Build/BuildLinkInputs/run.jet).

### Language server

`jet self lsp` speaks JSON-RPC over standard input and output. It uses the same
front end as checking, reads the import graph from disk, and overlays the open
buffer over those files. Diagnostics cover the complete document, and teaching
quick-fixes use `Diagnostic.edit`. Formatting is available through `jet fmt`
(I6, S14).
The server's protocol loop accepts JSON-RPC requests and emits framed
`Content-Length` responses; clients do not need a second compiler process.
The LSP coverage is in `Compiler/JetLSP/Tests/LSP/` and
`tests/cases/lsp/`.

The VS Code/Cursor client in `Tools/editors/vscode/` provides the TextMate grammar
and LSP transport. `install.sh` packages the extension. Server discovery uses
`jet.executablePath`, then the legacy `jet.languageServerPath`, then `jet` on
`PATH`; `jet self lsp` does not invoke `rustc`.

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
| Registry API snapshots | `apis/<name>/<version>.api`, written once beside each published artifact: `api_version = 4`, `package = <name>`, `published_version = <version>`, then one sema-produced public API row per line, each starting with its kind (`cap`, `const`, `distinct`, `enum`, `field`, `fn`, `impl`, `require`, `struct`, `trait`, `type`, `unit`, `variant`) and its dotted name. A `cap` row is a written `#FX(…)` cap of a public type, trait or module (`cap Receipt #FX(IO, !FS)`); a file's head cap names its module, the entry file's the package. | A missing snapshot for an earlier published version, another `api_version`, a package or version that differs from the path, and a row with no kind are malformed (**E2607**); the publish stops before it mutates the registry. |

## REPL state, authority, and editing

The REPL retains accepted statement ASTs and live `CtValue`s between turns.
Lists, maps, options, results, structs, enums, and closures remain values rather
than being rebuilt from display text. Explicit binding annotations remain
available to `:type`.

Pure Core calls run directly. A REPL session follows the program model of
"Application authority": every effect is allowed, and `--deny` wins. A turn
that reaches an effect the session has not used before first prints the
notice on stderr naming the new effects, so the session's effect set never
widens silently. A turn that reaches a `--deny`ed effect stops with E1803
before host state changes, and a piped session that was refused exits
nonzero. Calls inside a written `#FX` boundary or effect row are checked
against it as ceilings. Filesystem access stays below the project
root descriptor opened at session start. Later components open descriptor-
relative without following symlinks; a platform that cannot enforce this
confinement fails closed.

Ambient random draws require `Rand`; an explicitly seeded `RNG` is input data.
REPL-owned `print` and `eprint` capture does not need an `IO` grant. Process
execution opens the canonical executable before authorization and launches that
exact descriptor. Stdin is closed unless a separately authorized stream
surface supplies it. The child starts in the verified project directory with an
empty environment; stdout and stderr are captured. Interrupts target the
process group, which the REPL kills and reaps after 30 seconds. Every accepted
input runs on the shared checked path; the REPL refuses no feature by name.

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

REPL documentation and completion, and LSP hover and completion
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
facts. It reads the ordinary checked bundle; it does not run a second
analysis.

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
  provenance. Every function is named by its dotted source path
  (`pricing.rates`, `Receipt.render`), and a Core callee by its Core path
  (`core.files.exists`), so a provenance path steps into a Core call the way it
  steps into a user call.
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
retains the stable `roles: effect` header and grouped lens lines including
`inline —`, `effects —`, `layout —`, and `callable-signature —`; empty lenses
are skipped in the bare form. In JSON, a usage error starts with the
`jet.status/v1` envelope and an unknown lens carries `"code":"E2941"`.

Lenses live in one static registry in `Compiler/JetCli/Source/Cli/Expand.jet`. A future ratified
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
`$value` captures one subtree and `$values...` captures a list. Matching uses
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
    {"path":"Examples/Tooling","validate":"clean"},
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
`parse`, `check`, and `source-map`. `inspect facts` likewise renders the fact
query `core.compiler.query` (see Compiler and package model APIs).
Runtime calls to `core.compiler` are E0956.

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
registry source is `Compiler/JetFoundation/Source/Registry/CLICommands.jet` and the runtime
dispatch is in `Compiler/JetCli/Source/Cli/` (D-SHAPE6, D-CLI-ONE1).

### Typed entry signatures

A typed entry's resolved parameter type is its CLI specification; no separate
flag DSL is required. Plain `run: fn()` is the zero-input entry. A program opts
into typed input with one parameter whose type is a `#CLI` struct:

```jet
#CLI
ServeArgs: struct {
    #[Doc("port to listen on"), Arg(env: "PORT")] port: Int{3000}
    #Arg(short: "v") verbose: Bool
    config: String?
}

run: fn(args: ServeArgs) {
    routes().bind("127.0.0.1:{args.port}") ?? return
}
```

`#CLI` is a sibling derive of `#Codable`. `#Doc` on a struct describes the
program, on a field describes its option, and on a callable member describes
its command. Descriptions preserve embedded line breaks. `#Arg(flag_only:,
short:, env:)` is the one command-input field marker (D-CLI-ARG1). `#CLI(Standard)` additionally registers
`--verbose`/`-v`, `--quiet`/`-q`, `--color=auto|always|never`, and `--version`.
Plain `#CLI` always registers `--help`.

`run` is the reserved program entry name. A typed `run: fn(args: T)` is an
explicit request for command input; a `main` function has no entry meaning.
Raw argument access remains explicit through `core.process.args()`. An
`Output.Executable` entry uses the same contract. There is no variadic typed
entry. An App-returning typed entry decodes the one argument, calls `run(args)`,
and serves the returned App; it does not change the CLI schema
(D-CLIFLAG1, D-SHAPE-CLI1).

### Field mapping

Each supported `#CLI` field maps to exactly one long flag. Required scalar
fields also fill bare positional values in declaration order unless
`#Arg(flag_only: true)` opts that field out. Named input wins over a positional value. The supported
shapes are:

| Field shape | Named form | Bare form | Missing value |
| --- | --- | --- | --- |
| `Bool` | `--name` | none | `false` |
| `?T` for a supported scalar | `--name VALUE` | none | `None` |
| scalar with `{expr}` | `--name VALUE` | none | `expr` |
| required scalar with `#Arg(flag_only: true)` | `--name VALUE` | rejected | runtime `core.args` error |
| other supported scalar | `--name VALUE` | declaration-order positional | runtime `core.args` error |

Supported scalar types are `Int`, including inline ranges, `Float`, `Bool`,
`String`, and `Path`. A map, closure, list, or nested `#CLI` struct is E1305.
Field names convert underscores to dashes. Every field accepts its named form.
`#Arg(flag_only: true)` on a bool, optional, or defaulted field is E1309. Declaration order is
part of the command interface and is represented in the checked `CLISchema`.
A field named `help` collides with generated `--help` and is E1306.

`#Arg(short: "n")` adds `-n` to the long form and must be one unique ASCII
letter. `#Arg(env: "PORT")` is read only when command input is absent. The
precedence is command input, then environment, then field default. Invalid or
duplicate short names are E1318; `#Arg` outside a `#CLI` struct and
`env:` on a presence-only bool are E1319. Nested `#CLI` structs are deliberately not
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
CLI: Output.Executable{name: "todo", entry: launch}
API: Output.Service{name: "todo-api", entry: serve}
RELEASE: Output.Check{name: "release", entry: verify_release}

launch: fn() {}
serve: fn() {}
verify_release: fn() {}
```

An Executable takes zero or one supported typed CLI parameter; a Service or
Check takes no parameters. Each returns `Unit` or `Unit ?`. Sema resolves and
validates the reference before the IR is built and publishes its definition
and effect row to semantic tooling.
With no explicit selection, a legacy `run: fn` wins; otherwise one compatible
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
executable basename, rejecting control characters. A plain `run: fn()` still
has a valid built-in-only schema. Metadata failures are E2103
(D-SHAPE-CLI-CARRIER1, D-SHAPE-CLI-COMPLETE1).

## Editions and compatibility

A package may pin an edition with `edition: "2026"` in `package.jet`. The one
supported edition is `2026`, and it carries no edition-gated behaviour; a
missing field uses that edition, as does a single-file `jet run file.jet`,
which has no edition marker. Any other edition is E2001 (7bef65ccd4). The
durable compatibility contract for patch, minor, major,
epoch, edition, migration, deprecation, and generated-code licensing lives in
[the release policy](release-policy.md) (D-REL3, E2-V4).

## Toolchain pins and source channels

The top-level `jet:` field pins the Jet compiler channel used for a package:

```jet
name: "wordstats"
version: "0.3.1"
jet: "a0.0"
```

A pin is in Jet's own version spelling, `a0.0.N` (the leading letter is the
release stage, `a` for alpha): an `aMAJOR.MINOR` series such as `a0.0`, an exact
`aMAJOR.MINOR.PATCH` such as `a0.0.43`, or a named channel such as `main`,
`stable`, or `nightly`. Comparison and wildcard forms such as `>=a0.0.43` remain
minimum-version constraints and are not channel pins. A malformed channel pin,
including an unstaged `0.4`, is E1249. If `jet:` is absent, the running
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
record. Neither it nor an automatic refresh rewrites the source declaration:
the lock is the one home of the exact version (D-CHANNEL-AUTO2=A).
`jetpack outdated` compares lock facts with channel metadata without writing;
each row names the locked exact version, and a row whose metadata lookup fails
says `unknown`, never `current`. `jet build`, `jet run`, the environment view,
and `jet dev` read the exact lock entry; an unlocked channel is E1271,
including in CI and `--offline` mode (D-JPK-CHANNEL1).

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
package: {
    name: "inline-demo"
    version: "0.1.0"
}

helper: fn() -> String {
    "same package"
}

run: fn() {
    print(helper())
}
```

The block is structural context, not a second Package grammar. The loader
extracts its body bytes and spans and passes the body to the same
`PackageFacts` parser used for `package.jet`. The ordinary parser, formatter,
and LSP mask only those bytes, preserving offsets; they do not reimplement
Package fields. `priv(package) module` keeps package-scoped privacy.

Only one inline block may appear, and it must precede every other top-level
declaration. A byte-zero `#!/...` launch header may precede it. An
unbalanced block is E1362, a non-leading block is E1360, and a duplicate is
E1361. A balanced block's field errors are the diagnostics `package.jet`
reports for the same body (an `authority:` field error is E1221 at the
carrier file), never a generic wrapper. Coexistence with a project
`package.jet` is E1363. The file otherwise
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

### Core layers

`Core/package.jet` orders the Core library's modules into layers, lowest first:

```text
layers: {
    prelude: ["core.prelude", "core.mem", "core.sync"]
    data: ["core.text", "core.collections"]
    system: ["core.files", "core.net"]
}
```

Each key names a layer and lists quoted module names. A listed name also covers
the modules under it (`core.net` covers `core.net.tls`); the longest listed name
decides a module's layer, and a module is listed in at most one layer. A Core
module names only modules of its own layer or a lower one.

`jet check Core` reports each name of a higher layer's module where it is
written (E1012): a `core.<area>` path, in a `use` or in code, or a private part
of a higher area's module. Comments and quoted text name nothing. A program's
check reads no layer table.

## Inline script dependencies

A manifest-less `.jet` script may begin with an inline dependency:

```jet
// stats.jet
use textkit#1.4

run: fn() {
    print(textkit.wrap(input.join("\n"), width: 72))
}
```

`pkg#version` is the `#` directive-plane selector. The selector is dotted
numeric text such as `1.4` or `1.4.2`, not an operator range, and applies only
to a single-segment package name. `core.files#1.0` is not a package dependency.

`jet run stats.jet` collects inline references from the entry file and resolves
them into the module search from a matching copy under
`.jet/inline-deps/<name>/<version>/` or the documented fixture override, and
last from the toolchain's first-party battery of that name
(`Jetpack/Provider/Batteries/<name>`), when its package.jet declares a
version the selector matches, as a version dependency in package.jet resolves.
A loose file therefore uses a battery with `use web#0.1.0`. On
its first run, a reference to a package no local copy knows is fetched through
the configured package registry on the same path as `jet fetch --lock`: the
artifact is verified, ingested into the Hangar, copied to
`.jet/inline-deps/<name>/<version>/` and locked in `stats.jet.lock`, with no
`jet add` step. Later runs resolve from that copy offline. A local package
stays authoritative: when it has no matching version, the registry is not
asked. Resolution never executes dependency code. A reference that still
doesn't resolve, or one `jet check` or `jet build` meets without a local copy,
is E1253.

An exact `major.minor.patch` selector is pinned. A looser selector such as
`1.4` is accepted but emits L0203 until
`jet fetch --lock stats.jet` writes `stats.jet.lock`. The sidecar records the
script content hash, resolved versions, and content hashes; editing the script
makes the lock stale. `jet init stats.jet` lifts inline references into the
manifest's `deps` block without discarding their declarations
(D-JPK-SCRIPTDEP1).

## Sandboxed WASM packages

A sandbox is an isolated `wasm32` Component Model module. A native host loads
and calls it across the Component boundary; the sandbox does not require a
`#Unsafe` gate. This application-sandbox world is distinct from PATH helpers
and from the compiler-extension world. The compiler does not build sandbox
Components: `jet build` has no sandbox target, and a host loads a Component
that was built ahead of time.

```jet
// package.jet
name: "mathkit"
version: "0.1.0"
```

```jet
// sandbox source; it is loaded, not a run entry
scale: fn(a: Float, b: Float) -> Float {
    a * b
}
```

A sandbox's WIT world lists the entry file's top-level functions. A host uses a
typed plugin handle:

```jet
use plugin: core.plugin

run: fn() {
    policy: Authority.from_rights(["FS.Read(\"repo\")"])
    mathkit: plugin.load("mathkit.wasm", policy)
    area: mathkit.scale(6.0, 7.0) ?? panic("scale failed")
    print("scale(6, 7) = {area}")
}
```

`plugin.load(path, authority)` exposes only its Component's named exports. Sema checks
parameter and result contracts before code generation. There is no dynamic
export lookup or missing-export fallback. Each parameter and the result of an
export is `Int`, `Float`, `Bool`, or `Text`, mixed freely across positions, or
a recursively closed `#Codable` record, list, `Option`, or `Result` of those
(E1260). A result may also be a Component tuple of those scalars, such as
`tuple<s64, s64>`; the host reads it from the export's return area as the Jet
tuple `(part0: Int, part1: Int)`, the member `partN` holding the field at
position N (D-COMPONENT-TUPLE1=A). A
sandbox export must return a value; fixed widths, maps, tuple parameters,
unions, handles, and generic applications do not cross the Component boundary.
The `packages/sandbox_mathkit` golden exercises `Bool` results and UTF-8 text
round trips, including a non-ASCII name; every named export the host uses comes
from the guest Component it loads.

The path is a literal. The compiler reads a relative path under the package
root, takes the guest's interface from that Component on every compile, and
builds the guest into the host; no cached snapshot stands in for the
Component. At load the
host reads the same path from its working directory, so it runs from the
package root. The loader reads the component under explicit resource-scoped
`FS.Read` authority, checks that it matches the built guest, and preflights
every declared WIT import. Each import's right must be declared in the loading
package's `authority.needs` and held by the narrowed host authority before
linking or instantiation. Otherwise the load fails as `PluginError.Denied`
with export `load`, which every export call reports, and no guest code runs,
on every tier. The loading package is the entry package whose root the
artifact path is read under; a package with no `needs` loads only import-free
components. Unknown, wrong-shape, or missing imports fail closed before guest
code runs too. A guest with no declared imports has an explicit empty
capability set, never an ambient fallback. Registered imports re-check
the same typed decision at the call edge and retain resource scope. Failure is a
clean `Err`, not a host crash. Guest-local `Mem` effects support Component text
ABI values but do not grant host capabilities.
The narrowed authority crosses the host boundary as its wire text (the sorted
rights, one per line). The compiler inserts that conversion once at the
`plugin.load` call; no source, Core source included, may name it (E0510).
Each export call returns `T PluginError!` (D-PLUGIN-FAILURE1=A). Match `Guest`,
`Denied`, `Budget(limit, fault)`, or `Defect` rather than reading the display text.
The `PluginFault` retains the export, message, and bounded guest frames, each a
`PluginFrame { function, component, offset }`; a propagated failure retains the
host call line through the ordinary failure journey. Loading a
malformed Component does not stop the host: its first call reports `Guest` with
export `load` and no frames.

Every call runs under fixed budgets, and running past one fails as
`Budget(limit, fault)`:

- `.Fuel`: 10,000,000 units per call, spent at each function entry and loop
  iteration.
- `.Memory`: 256 pages of 64 KiB for each linear memory.
- `.Table`: 10,000 elements for each table.
- `.Wire`: 1,048,576 bytes for each string argument or result crossing the
  boundary. An argument over it fails before the guest's `realloc` is asked
  for room, and a result over it fails before the host copies it out. Neither
  failure has guest frames.

A fault keeps at most 32 guest frames.

The manifest `export:` target names the interface and defaults to the package
name. The frozen public interface is keyed as `plugin__<export>` in
`.jet/cache/api/`. Sema resolves a host's calls against the interface it reads
from the loaded Component, and a call to an export the Component does not list,
or through a path that names no readable Component, is E1257. E1260 reports an unsupported
export shape on
either the sandbox Component boundary or the native `#Export(c)` C boundary
(D-PLUGIN1, D-PLUGIN-EXPORT1, D-PLUGIN-VERSION1, D-EMBED2).

### Embedding artifacts

A native artifact exports the entry module's `#Export(c)` functions; a sandbox
artifact exports its top-level `pub fn` items. Each parameter and the result of
a native `#Export(c)` function is, independently, one C primitive: `Int`
(`int64_t`), `Float` (`double`), `Bool`, `Text` (the JetText pointer/length
pair), `I8`–`I64` and `U8`–`U64` (`int8_t`–`uint64_t`), or `F32` (`float`); a
unit result is `void`. A sandbox export uses the Component shapes above. Both
boundaries publish the success value of a function's failure contract.

`jet build --lib` emits a linkable native static or shared library and a C
header. It needs no Component runtime, but the native artifact is trusted code:
the C host owns its ABI and capabilities. Text results are released with the
generated `jet_text_free` function.

A sandbox host supplies a Component runtime; the guest has no ambient Jet
authority, and future host capabilities must be explicit WIT imports. The
native and Component artifacts therefore offer different trust and runtime
guarantees.

## Programmable builds

`jet build` checks the root program and then evaluates at most one package-local
`build: fn(b: Build) -> BuildPlan` through the comptime interpreter. The
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
a `BuildTarget`. `b.plan()` or `b.plan(default)` hands one
canonical graph to scheduling, caching, execution, queries, and LSP.
`b.toolchain(name, target_triple)` records target identity. `b.probe` supports
`find_program`, `find_folder`, `pkg_config`, and `header` probes.
`b.args()` returns the words after `--` on the command line as a `[String]`
(`jet build -- release-notes` gives `["release-notes"]`; no `--` gives
`[]`), so a build entry can choose its default target or options from them.
A `jet build` that runs no `build: fn`, such as `jet build main.jet -- a`,
refuses the words with E2104 instead of dropping them.

`b.compiler(target)` returns the `Compiler` value that acts on one of the
build's targets. Only the root package's own build source can make it: a
`b.compiler` call written in any other package is E0776, even inside a
function the root hands its `Build` to. The value expires with the build that
made it. `#code { items }` is a `Code` value holding the items written in its
block, and `meta.add(code, scope: "name")` adds them to the value's target as
the generated module `name`, which the program checks like hand-written code.
A `#{name}` in the items that names a String build local pastes the name or
type it spells, through the same template paste a `b.generate` template uses
(`#{name}: fn(who: #{kind}) -> #{kind}`). A paste of any other build local
is refused where the code is written, and the build host refuses text that is
not one name or type's spelling (E0357).
`meta.add` writes declarations, so it uses the `Meta.Generate` effect: allowed
unless the calling function's row denies it (`-[!Meta.Generate]>`).
`meta.rewrite_body(f, code)` replaces the body of the top-level function `f`
with the statements of a `#code { statements }` block. The function keeps its
own declaration up to its body and becomes the generated module
`rewrite_body/f` of its package; its original text is withdrawn from its file,
whose other code keeps its place. The new body is checked like hand-written
code, in that module, which sees its package's declarations and opens with
its file's module imports, so `f`'s markers, signature and body name what
they named there. `f` is named
directly or through its package's import (`orm.query`); a local value is
refused, and a function its package doesn't declare with a braced body fails
the build (E3502). One build rewrites a function's body once. A line
`#{f.body}` of the new body pastes the function's original body there, at
that line's indentation, so a rewrite can wrap the body it replaces. Replacing
a body uses the `Meta.Rewrite` effect, denied the same way
(`-[!Meta.Rewrite]>`). The root package's own functions need no consent; a
dependency's function is rewritten only after the root writes
`meta.allow_rewrite(package: "orm")` on its value, and otherwise the call
fails the build (E3514). The consent names the dependency the build loads at
its locked version, and the rewrite changes only this build's copy of it.

`b.run(name, tool, caps)` declares one step and returns its `BuildAction`.
The tool is a `BuildFile`. `b.found(probe)` is the program a `find_program`
probe finds, and a step's `.output(path)` is a file that step makes, so one
step can build the tool the next one runs. `b.host(target)` is the executable
`target` built for the machine running the build, at
`.jet/build/host/<name>` and never installed; a step that runs it waits for it
to compile, and the one graph builds it whether or not it is selected. Only an
executable target with a source can be built so (E3502), and on a machine the
native backend doesn't write images for, a step that runs one is refused
before anything runs. `b.image(target)` is the same target built for the
build's selected platform, the image a step packs or ships, copied to
`.jet/build/image/<name>` once it compiles; a step that reads or runs it
waits for it, the one graph builds it whether or not it is selected, and only
an executable target with a source has one (E3502). When the build selects
the machine's own platform, a target's image and host build are one file,
built once. A step runs an image only where a sandbox can (Linux x86-64);
elsewhere it is refused before anything runs. Example:
[`tests/cases/build_entry/run_steps_read_an_image.case`](../../tests/cases/build_entry/run_steps_read_an_image.case).
`b.jet()` is the compiler running the build; its step
runs it from its own folder, mounted read-only, so a step can check, run or
build Jet code. `b.dependency(name)` is the folder of the package package.jet
lists as `name` (an unlisted name is E3502), and `.file(path)` on it is a
file there: before any step runs, the build copies it to
`.jet/deps/<name>/<path>`, the path the step gets, so a step reads another
package's file without reaching outside its own folder. Example:
[`tests/cases/build_entry/run_steps_read_a_dependency_file.case`](../../tests/cases/build_entry/run_steps_read_a_dependency_file.case).

A step can also read a whole folder. `b.folder(path)` is the folder at `path`
in the build's package (`.` is the package itself), `.folder(path)` on
`b.dependency(name)` is one in that package, at `.jet/deps/<name>/<path>`,
and `b.dependency(name)` itself is all of it. `.file(dir)` passes the
folder's path and binds the folder read-only at that path in the step's
sandbox; nothing is copied. The step's cache key covers every file in it, so
adding, removing or editing one runs the step again. A folder's `.jet/` and
`.git/` folders and what its `.gitignore` files exclude are not part of it,
and a link inside it that leads out of it fails the step. A folder is not a
tool, and no step's output may lie inside a folder a step reads (E3502).
A folder path is written plainly, as `dir/sub`: an empty part, a `.` part or
a trailing `/` is refused (E3502), as a path that leaves the package is.
Example:
[`tests/cases/build_entry/run_steps_read_a_folder.case`](../../tests/cases/build_entry/run_steps_read_a_folder.case).

A `find_folder` probe finds a folder on the machine:
`b.probe("musl", "find_folder", "libc.a")` looks for the file `libc.a` in
each absolute LIBRARY_PATH entry, then `/usr/local/lib`, `/usr/lib`, `/lib`,
`/usr/lib/musl/lib` and `/usr/local/musl/lib`, and chooses the first folder
where it fits. Any file fits, except that a `libc.a` fits only when it is
musl's: an ar archive holding musl's `__libc_start_main` and `__init_tls`
objects, with `crt1.o` beside it, so a glibc `libc.a` is passed over.
The probe names a file alone, not a path (E3502). `b.found(probe)` is the
folder it chose, read as `b.folder` is, at `.jet/found/<probe>`; a step that
reads it runs the probe first. A probe that finds no folder fails the build
before any step runs (E3505), naming each folder it looked in and why it
passed it over. Examples:
[`tests/cases/build_entry/run_steps_read_a_found_folder.case`](../../tests/cases/build_entry/run_steps_read_a_found_folder.case),
[`tests/ui/build_find_folder_missing.jet`](../../tests/ui/build_find_folder_missing.jet).

Calls on the step build its
command line in order. `.input(path)` and `.output(path)` declare a file and
pass its path as the next argument. `.reads(path)` and `.writes(path)`
declare a file without passing it. `.file(f)` passes another step's output
and orders the step after that one, and `.arg(text)` passes literal text.
`.input`, `.output` and `.file` take `flag:`, written before the path in the
same argument: `.output("gen.c", flag: "--out=")` passes `--out=gen.c`.
`.output` returns the new `BuildFile`, and `f.path()` is a made, host-built,
copied or folder file's package-relative path (a package's is
`.jet/deps/<name>`); a found program and `b.jet()`
have no path until the build runs, so their `.path()` is refused. `.file` on a
found program passes the path its probe finds when the step starts, the path
the program runs at in the step's sandbox, which shows it as it shows a step's
tool; on Linux that is a program in the Nix store (`/nix/store` is bound
read-only), so a compiler there finds its own `cc1`, `as`, `ld` and libraries.
The step's cache key covers the program's bytes and the path its probe found,
so another compiler at that name runs the step again. Example:
[`tests/cases/build_entry/run_steps_pass_a_found_program.case`](../../tests/cases/build_entry/run_steps_pass_a_found_program.case).
An input a step reads reaches its sandbox runnable when its owner may run it,
so a step can run a host build it is passed. Example:
[`tests/cases/build_entry/run_steps_pass_a_runnable_input.case`](../../tests/cases/build_entry/run_steps_pass_a_runnable_input.case).
`.toolchain(t)` and
`.sign(identity)` add toolchain and signing identity; a `b.jet()` step runs on
the toolchain its compiler was built with, so `.toolchain` on it is refused. A
step's result is cached. A step that
declares no output is refused at its `b.run` call (E3502), since only declared
outputs leave its sandbox, and `.arg` text that names a package file is
refused with the fix `.input(path)`, so the step can't read an undeclared file.
An output that would overwrite a file no build made, such as a committed one,
is refused before anything runs (E3502), as generated sources are: a step
replaces only what the last build's receipt lists as a step's output. Example:
[`tests/cases/build_entry/run_steps_refuse_to_overwrite_a_file.case`](../../tests/cases/build_entry/run_steps_refuse_to_overwrite_a_file.case).
`.output` and `.writes` take `kept: true` for a file that stays in git, such
as a generated table: the step may replace it though no build made it, and
the build writes it only when the step makes other bytes than it holds. When
it holds bytes that are neither those nor the ones the last build left there,
it was edited by hand: the step publishes none of its outputs and the build
fails with E3502, so the fix goes into the step's inputs instead. A fresh
checkout, with no receipt yet, writes a kept file only when its bytes differ.
A kept path under `.jet/`, which is never committed, is refused (E3502).
Example:
[`tests/cases/build_entry/run_steps_keep_a_committed_output.case`](../../tests/cases/build_entry/run_steps_keep_a_committed_output.case).
`.timeout(10min)` bounds how long the step may run. The `action_timeout` build
option (stock `30s`; the profile field `action_timeout: 10min`, the call
`b.action_timeout(10min)` and the flag `--action-timeout=10min`) bounds every
step that sets no limit of its own. Both take a whole number and one unit
(`ns`, `us`, `ms`, `s`, `min` or `h`) that comes to at least 1ms, and no cache
key names the limit. On Linux the limit is CPU time: the step's process tree
may use that much on each of the host's cores, which other load on the host
does not raise, so a loaded host slows a step without failing it. Its wall
clock is only a hang guard at ten times the limit. Elsewhere the limit is wall
time. A step that runs past either is stopped and reported with E3505, which
names the limit it hit. Examples:
[`tests/cases/build_entry/run_steps_chain_a_found_tool_and_a_made_file.case`](../../tests/cases/build_entry/run_steps_chain_a_found_tool_and_a_made_file.case),
[`tests/cases/build_entry/run_steps_stop_a_looping_step_at_its_cpu_limit.case`](../../tests/cases/build_entry/run_steps_stop_a_looping_step_at_its_cpu_limit.case);
a generator built and run in one build, whose output another target compiles:
[`tests/cases/build_entry/run_steps_run_a_host_tool.case`](../../tests/cases/build_entry/run_steps_run_a_host_tool.case).

A target's sources are package-relative paths, and the first is its entry.
Every source a target lists is part of it: the others share the entry's
namespace, as the modules `b.generate` produces do, so
`b.add_executable("app", ["main.jet", "helper.jet"], [])` lets `main.jet` call
what `helper.jet` declares by name. Before any action runs or the lock is
written, `jet build` refuses a selected target that lists a source which is
neither a file, a generated module, nor an output a build action declares
(E2105). An action's output is the action's to produce: the target compiles
it after the graph runs.

```jet
build: fn(b: Build) -[Exec, FS]> {
    #Impure("run declared toolchain probe and step") {
        shell: b.probe("shell", "find_program", "sh")
        native: b.toolchain("native", "x86_64-linux")
        stamp: b.run("stamp", b.found(shell), ["Exec", "FS"])
        stamp.arg("-c")
        stamp.arg("cp assets/version.txt build/version.txt")
        stamp.reads("assets/version.txt")
        stamp.writes("build/version.txt")
        stamp.toolchain(native)
        app: b.add_executable("app", ["main.jet"], [stamp])
        b.default(app)
    }
}

run: fn() { print("hello") }
```

`jet build` runs steps and targets as one graph. A step waits for the steps
that make its inputs, the targets it runs as `b.host` tools, and its target's
dependencies' steps; a target waits for its own and its dependencies' steps
and the steps that make its sources. Every step's grant is checked and every
probe runs before anything else does. Then whatever is ready runs: the ready
steps together, up to `--threads` at once, then a ready target compiles. A
failure stops only what waits on it, the rest of the graph still runs, and
every failure is reported. Steps and targets that wait on each other are
refused (E3502) before anything runs. A step's key covers its declared input
content, argv, authority, toolchain, probes, signing identity, and
generated-source hashes.

No step is reused: every `jet build` runs every step, whatever its effects, and
opens no store, so a step's outputs are always the ones this build made. Reusing
a step's stored record is off, and no flag, variable or setting turns it on.
`FS`, `Exec` and `Env` are not ambient effects: the sandbox lets the step read
only its declared inputs, write only its tree, run only its tool and what is
mounted (each identified by content in the key), and see only its declared
variables (their values in the key). `Net`, `Time`, `Rand`, `IO`, `DB`, `Log`
and `GPU` are. A step must write an output. No step runs outside the sandbox: a
read of an undeclared file fails the step on every build. Example:
[`tests/cases/build_entry/run_steps_run_again_on_every_build.case`](../../tests/cases/build_entry/run_steps_run_again_on_every_build.case).

When `jet build` runs the graph of a `build: fn`, it leaves two records under
`.jet/build/` beside the build entry, whether or not the build succeeded. `receipt.json` is
the `core.build` `BuildGraph` it ran: its targets, steps with their keys, files
and the compiler's own nodes, with each step that ran marked a cache miss in
`cache_hit` (`cache_hits` lists none, since no step's outputs come from the
store). `times.json` is its `BuildTimes`: one `BuildTime` row (`name`, `kind`
`step`, `host`, `image` or `library`, `wall_ms`, `reused`) per step and target,
in the order each finished. A step's `wall_ms` is the time of the steps that ran
with it, and `reused` is false, since every image is built. Both are
JSON objects whose fields are the value's own, in declaration order; after
them `receipt.json` has `kept`, one `path` and `digest` (`sha256:<hex>`) per
kept output, of the bytes the build left there, then `generated`, one row per
module `build: fn` generated, as `jet inspect graph` shows it (`id`, `name`,
`path`, `digest`, `made_by`, `handoffs`, `authority`, `consent`). A hover in
`jet self lsp` anywhere in a module under `.jet/generated/` that the last
build's receipt lists names what made it: the call and function, each
handoff of the `Compiler` value that led there, root first, the `Meta` effect
and consent it used, and its identity.
`jet inspect build --times` shows the last build's rows (`kind`, `name`,
`wall_ms`, and `made` or `reused`), or the record itself under `--json`;
before any build has recorded one it fails with E2104. Examples:
[`tests/cases/build_entry/run_steps_record_the_graph_and_times.case`](../../tests/cases/build_entry/run_steps_record_the_graph_and_times.case),
[`tests/cases/lsp/lsp_hover_in_a_generated_module_names_what_made_it.case`](../../tests/cases/lsp/lsp_hover_in_a_generated_module_names_what_made_it.case).

There is no ambient execution fallback. On Linux each action runs under
bubblewrap with private mount, PID, IPC, UTS, and network namespaces. Only
declared inputs enter its writable tree and only declared outputs leave it.
Network is unshared unless both source and policy grant `Net`. One resolver
(`foundation_linux_bwrap`) picks the bubblewrap for every sandboxed action,
`jet build`, `jet cc` and the Rust bridge alike: the `bwrap` on PATH first,
because a host that lets only profiled programs create user namespaces
(Ubuntu's AppArmor profile for `/usr/bin/bwrap`) allows only its own copy;
then Jet's own hash-pinned static bubblewrap at
`<hangar>/tools/<sha256>/bwrap`, installed from the `jet-bwrap-<arch>-linux`
beside jet (a release archive carries it there) or, by a command that runs
build actions, fetched on first need from `JET_BWRAP_URL` or the jet's
release. Each must pass its probe. With neither usable the build is E3505,
whose message names why each failed, not an unsandboxed run. Example:
[`tests/cases/build_entry/build_step_without_any_usable_bubblewrap_names_both.case`](../../tests/cases/build_entry/build_step_without_any_usable_bubblewrap_names_both.case).
Build authority uses the same
`--allow=<RIGHTS>` and `--deny=<RIGHTS>` surface as other commands. Every
effectful action or probe is inside an active `#Impure("reason")` region;
signature declaration and effective grant are checked before execution.

`b.generate(name) { ... }` produces the module
`.jet/generated/<package>/<name>.jet`. Action outputs ending in `.jet` follow
the same path. Built-in derives, user derives, and build entries share one
checked expansion pass. Build-only entries and imported build entries are
removed before runtime code generation, so build handles never reach the built program.
The `write_generated` build option (stock `off`) says whether `jet build`
writes each generated module to that path under the package root for
inspection: `on` writes them, `off` writes none. It is the profile field
`write_generated: on`, the flag `--write-generated=on` (which replaces the
retired `--emit-generated`) and the `build: fn` call `b.write_generated(.On)`.
Example:
[`Examples/Metaprogramming/BuildTime/BuildWriteGenerated`](../../Examples/Metaprogramming/BuildTime/BuildWriteGenerated/run.jet).

A problem in a generated module is reported against that module's own text,
under its virtual path `<build-generated>/<package identity>/.jet/generated/…`,
with a note naming the `b.generate` call that produced it.

`b.contribute(name, value)` gives a value to a setting the package declares in
`package.jet` (`settings: { cache_slots: Int = 1 }`). The manifest lists every
setting and its type, and `build: fn` only computes a value (D-CONF-SPLIT1).
Before a package that declares settings compiles (`jet build`, `jet run`,
`jet test`), its `build: fn` runs, and the project lock records each
contribution as a `[[build.contribution]]` row. Every compile of the package
reads those rows: `#build.settings.cache_slots` folds to the contributed value
over the declared default and the selected profile's value. Example:
[`Examples/Metaprogramming/BuildTime/BuildContribution`](../../Examples/Metaprogramming/BuildTime/BuildContribution/run.jet).

Generation is additive with one owner per managed path.
`b.generate(name) { items }` makes one module at
`.jet/generated/<package>/<name>.jet`; a name keeps
only letters, digits, `_` and `-` (every other character becomes `_`), so no
name leaves that folder. Every generated module joins the package's one checked
compile beside its hand-written sources, so a generated module can use another
in any order and generation has no rounds to bound. A module generated twice,
two names that share one path, and a step that writes a generated module's path
are E3511, reported at the call that makes the second claim, before any step
runs or any generated file is written; an empty name or item block is E3502.
Steps that wait on each other are E3502 at the `b.run` call of the step the
cycle starts from. A target source or an input under `.jet/generated/` that no
`b.generate`, `meta.add` or step output makes is E3515, at the call that
declared the target or step that uses it. A generated module that would shadow
a hand-written one is E3510. When `run` or another hand-written function uses
a module the build never made because it refused a call, the refusal is
reported first and the check report follows. `--locked` checks generated input
and output hashes before materializing and records provenance after the
complete runtime bundle is checked (D-BUILDENTRY1).

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

Fleet host overrides are typed values evaluated by the same pure comptime
engine and cycle checks as computed module fields. Results retain exact source,
dependency, and purity provenance.
Host-local generic fields take precedence over enclosing comptime values and
resolve before service and option expressions, independently of declaration
order. Option values that are unresolved names, dotted symbolic atoms, or
string literals retain their written spelling; computed option expressions
retain their evaluated display value. The referenced System plan is unchanged:
the host keeps its typed override fields alongside the System reference.

### Compiler and package model APIs

`core.compiler` is a typed, read-only compiler API. `lex`, `parse`, `check`, and
`source_map` are compile-time-only and retain source, spans, diagnostics,
semantic facts, and generated-line mappings. `jet inspect compiler` mirrors
these operations as deterministic JSON with schema and API version 1. Runtime
calls are E0956.

Facts have one reader. `query(source: FactSource, kind: FactKind)` answers the
executable schema (`schema()`), a checked program (`b.program`) and a recorded
run (`receipt(path)`) as typed rows, and `jet inspect facts
--store=<schema|program|run>` renders that same call: the command checks a
program that calls `query` and prints the rows it returns, so the command and a
build read the same rows, with the same identities and errors, on every
optimization level. No tool or command answers a fact question another way
(core-library.md, "Fact queries").

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
available through `#package.name` and `#package.version`, not as a
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

`vmtest.<name>: module` declares a VM scenario. Its `hosts:` map names scenario
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
  a trait, or a closed data carrier.
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
- The compiler is written in Jet and builds programs through its own back end;
  no Rust-hosted compiler or Rust emission path is part of the toolchain.

For teaching diagnostics, staged parser errors should point toward the current
operator spelling—for example, `and` should teach `&&`—rather than accepting a
retired form (S14).
