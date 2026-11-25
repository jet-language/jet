# Adversarial review: binding and function-body syntax ideas

Date: 2026-11-25. Scope: six owner ideas for Jet's binding and body syntax,
judged against the goal of being the best choice in every domain, scripting
included. For each idea: the gains if it had no downsides, the downsides and
their reach, a first comparison, the engineering that removes downsides, and
a final verdict. Nothing here is ratified; the verdicts are inputs to ballots.

## Evidence used

All counts are over the Jet sources in `Compiler`, `Core`, `Examples`,
`Tools`, `Jetpack` and `Tests` of the dev worktree at `70ff64f2288`
(2,032,956 lines). They are pattern counts and approximate.

| Fact | Count |
|---|---|
| Named function declarations (`name: fn(`) | 105,198 |
| Named functions whose body fits on the declaration line | 17,580 |
| Immutable bindings (`name: value`) | 294,339 |
| Mutable bindings (`name := value`) | 78,436 |
| ... of which start from a plain name | 7,936 |
| ... of which start from a field or index place | 1,782 |
| ... of which start from a literal | 45,154 |
| Named write windows (`name: ^place`) | 217 |
| One-line arms (`if`, `else`, `loop` with `->`) | 76,563 |
| Bodies that are only a bare record (`{ {…} }`), one-line and multi-line | about 130 (56 one-line, 76 multi-line) |
| Bodies that start with a typed literal (`{ T{…} }`), unaffected by idea 4 | about 2,000 |
| Two-statement blocks of functions and lambdas that would fit in 100 columns | 3,587 |
| Three-statement blocks that would fit in 100 columns | 1,263 |

Probes run with the installed compiler (`jet a0.0.46`) on scratch files:

| Source | Result |
|---|---|
| `sq: fn(n: Int) -> Int{n * n}` | Jet.ArrowFunctionBody |
| `scale: fn(n: Int) -> Float { n * 12 }` | runs, prints `24.0` |
| `half: fn(t: Float) -> Int { t * 2.0 }` | Jet.ReturnTypeMismatch |
| `half: fn(t: Float) -> Int { Int{t * 2.0} }`, called with `1.35` | compiles; stops at run time: "`Int{f}` needs a whole, finite Float" |
| `q: fn(n: Int) -> U8 { n }` | Jet.ReturnTypeMismatch, fix `checked(U8{value}) ?? fallback` |
| `q: fn(n: Int) -> U8 { U8{n} }`, called with `300` | compiles; stops at run time: "Value doesn't fit in destination type" |
| `one: fn(y: [Int]) -> [[Int]] { [[Int]]{y} }` | returns `[[1, 2]]` (the list is wrapped) |
| `mk: fn(w: Float, h: Float) -> Rect { width: w, height: h }` | Jet.InvalidStatement: "found `,`" |
| `mk: … -> Rect { {width: w, height: h} }` | runs |
| `span: fn() -> {min: Int, max: Int} { {min: 1, max: 2} }` | runs: record result types exist |
| `f: … { t: a + s.len(); print(t); t }` | Jet.ExplicitSemicolon |
| `g: fn(in: Int) -> Int { in * in }` | Jet.InvalidStatement: `in` is reserved |
| `jet fmt` on `sq: fn(n: Int) -> Int{n * n}` | rewrites it to `sq: fn(n: Int) { Int{n * n} }`, a unit function: **defect** (below) |

Registry and decisions read: `Spellings.jet` rows 13, 14, 17, 64, 76-78, 92;
`CoreSurface.jet:116, 360-371`; `MathLayout.jet:475-478, 520, 528`;
`Docs/Spec/spec.md:920-923, 2240-2241, 2793`; D-BINDSIG-COUNT1=A,
D-BIND-BARE1=A, D-BIND-TYPE2=A, D-MUT-NEVER1=A, D-DEFAULT-SHAPE1=B,
D-CHOOSE-FNBODY1=A, D-BODY-LAST1=B, D-BODY-ARROW1=B, D-ARRAY-CONVERT1=A,
D-LAMBDA-STORED1=A, D-FMTCOLLAPSE1=B.

Gleam, read from its parser (`compiler-core/src/parse.rs` on the
`gleam-lang/gleam` main branch): `next_token` skips newline tokens and keeps
them only for the formatter; `parse_statement_sequence` reads statements back
to back with no separator; `(` cannot start an expression (error
`ExprLparStart`, because Gleam groups with `{ }`). Gleam also starts every
binding with `let`, has no index syntax and writes tuples as `#(…)`. Those
restrictions are what make a separator unnecessary.

## Defect found

`jet fmt`'s fix for Jet.ArrowFunctionBody turns
`sq: fn(n: Int) -> Int{n * n}` into `sq: fn(n: Int) { Int{n * n} }`. The
declared result type is dropped and the function becomes a unit function;
the diagnostic's "Fix: Replace `-> expr` with `{ expr }`" says the same. The
right fix when the text after the arrow is `Type{…}` is
`-> Type { … }`. It belongs on the card that owns Jet.ArrowFunctionBody.

## Idea 1: mutability on the value with `^` (`count: ^0`)

**Gains if free.** One binding symbol: `:=` retires and every declaration is
found by `name:`. `^` means "writable" everywhere: parameters (`p: ^T`),
`^self`, and now locals. Mutability sits in the same column as parameter
marks. The never-changed fix (D-MUT-NEVER1) becomes "delete `^`".

**Downsides.** `name: ^place` is already a named write window
(`spec.md:2241`, 217 uses): `first: ^slots[0]` then `first = 8` writes into
`slots`. Under the idea, `best: ^items[0]` written to mean "a changeable copy
of the first item" would instead write into the list. Reach: 9,718 of
today's mutable bindings start from a place, and every future one written by
people or agents who expect a copy, as `let mut best = items[0]` gives in
Rust.

**First comparison.** The gain is conceptual; the downside is silent data
corruption. Not worth it as stated.

**Engineering.**
1. Rule A, "`^` of a place is that place": consistent with parameters, but it
   keeps the hazard for new code. Rejected.
2. Rule B: a `^` binding always makes new storage (a copy of a copyable
   value, a move of a whole local, and an error that teaches `.copy()` for
   moving out of an element). Named windows become a compiler-owned method in
   the `.copy()`/`.move()` family: `edit: values[2..3].in_place()`, and two at
   once are two such bindings, checked by the disjointness proof Jet already
   has (D-MEMDISJOINT1). Passing `values[0..2]` to a `^` parameter stays a
   window by itself. The 217 named windows migrate.

**Final verdict.** Rule B removes the hazard. What remains: the cutover of
78,436 sites (mechanical), the 217 window rewrites, a new window API, and
reopening D-BINDSIG-COUNT1 and the window exception in D-CALLSITE-MARK2.
`^0` and `^""` read oddly at first. The gain is one less binding symbol and
one meaning for `^`; it does not make programs shorter or safer. **Juice:
small. Squeeze: moderate. Engineerable: yes. Net: marginal; the better of the
two one-symbol designs (it beats idea 2).**

## Idea 2: split sigils (`count: = 0`)

**Gains if free.** Every declaration is found by `name:`. One story: `:`
names, `=` assigns, and a name you will assign again is introduced with `=`.

**Downsides.** The empty gap between `:` and `=` invites a type: TypeScript,
Kotlin, Swift, Python and Rust all write `x: Int = 5`, and that form is one
word away. Jet removed typed bindings (D-BIND-BARE1=A) and teaches the type
onto the value (D-BIND-TYPE2=A). `count: = 0` reads as a typo. Labels
(`f(x: = 3)`, `{a: = 1}`, `#Context(k: = v)`) and struct bodies
(`port: = 3000` reads as a default) need errors.

**Engineering.** Accept `x: Int = 5`: then the type sits on the name for
changeable bindings and on the value for fixed ones, which reopens
D-BIND-BARE1 and needs a typed fixed form. Or reject it with a fix: the pull
toward the rejected form never goes away. Neither removes the downside.

**Final verdict.** **Juice: small. Squeeze: permanent. Not worth it.** Idea 1
with rule B gives the same search gain without the gap.

## Idea 3: `=` one-line bodies (`sq: fn(n: Int) -> Int = n * n`)

**Gains if free.** Brace-free one-liners, as in Kotlin, Scala, F# and
Haskell. Struct results lose the double brace: `= {x: 1, y: 2}`. Two
characters saved on each of 17,580 one-line functions.

**Downsides.** Two body forms for one meaning. A body that grows a second
statement must be respelled, which is the reason D-BODY-ARROW1 recorded
against `=` bodies ("blocks the formatter from resizing a body without
respelling it"). Combined with idea 2, `=` would mean both "changeable" and
"body". Lexing: functions that can fail end in `!` and generic results in
`>`, so `-> String IOError!= x` and `-> List<T>= xs` lex as `!=` and `>=`
(`MathLayout.jet:520, 528`). Jet has tried and retired three one-line body
forms for functions (`=`, then `::`, then `->`; today "the body is always braced",
`Spellings.jet:64`).

**Engineering.** The formatter picks the form from the body, so there is one
canonical form per program; the parser splits `!=` and `>=` after a result
type. The respelling on growth cannot be removed.

**Final verdict.** Once idea 4 removes the double brace, the remaining gain
is two characters against an existing one-line form
(`sq: fn(n: Int) -> Int { n * n }`). **Juice: small. Squeeze: moderate. Not
worth it.**

## Idea 4: the result type shapes the body braces

The owner's position: where the result type is known, the braces should take
it, so `-> Rect { width: w, height: h }` should build a `Rect`.

**Gains if free.**
- No double brace: `new: fn(w: Float, h: Float) -> Rect { width: w, height: h }`.
  About 130 bodies today; bodies that name the type (`{ T{…} }`) keep it.
- The same in lambdas and branches, the bigger win for data work and
  scripting: `rows.map(fn(r) { name: r.name, total: r.qty * r.price })`.
  JavaScript's version of this, an arrow function returning an object, needs
  `({…})` and is a well-known trap.
- `-> Int{n * n}`, which people and agents will write, is accepted and
  formatted as `-> Int { n * n }`, instead of today's error and broken fix.

**Downsides.**
1. Reading the body as a `T{…}` literal is wrong in Jet, because `T{…}`
   converts: the probes show `-> U8{n}` and `-> Int{t * 2.0}` would turn
   compile-time errors into run-time stops, and `-> [[Int]]{y}` would wrap a
   list silently (D-ARRAY-CONVERT1). Reach: every function with a number,
   list or checked-text result.
2. Whether braces hold a record or code must be decided before types exist.
3. A block made only of bindings looks like a record.
4. `{x}` is a block, so a one-field struct written `{ value }` does not
   build.
5. `{}` is a block.

**Engineering.**
1. The braces are never a conversion. They are code, as today, unless a
   comma separates field or pun members at their top level, the rule arm
   bodies already use (`parser_brace_record_value_at`,
   `Compiler/JetParser/Source/Parser/Expressions.jet:1553-1601`). Then they
   are a record, which builds the expected struct as a record does anywhere
   else. The rule is syntactic, so the parser needs no types, and it deletes
   the exception "braces after a signature … hold code" for function and
   lambda bodies.
2. A block never holds a top-level comma, so every body the rule reads as a
   record is a syntax error today: no valid program changes meaning. (An
   earlier draft keyed the reading on whether the value is used; the comma
   rule is simpler and already proven on arms.)
3. `-> T{` after a signature parses as `-> T {`; `jet fmt` adds the space.
   The number cases then stay compile errors, as today.
4. A one-field body (`-> Box { value: v }`), `{ value }` and `{}` stay
   blocks; Jet.MissingBlockValue's fix shows `Box{value: v}`.
5. Lists keep `[…]`, so they gain nothing and lose nothing.

**Final verdict.** All five downsides are removed, and the change deletes an
exception instead of adding a form. **Juice: moderate (about 130 bodies,
record-returning lambdas, a beginner trap). Squeeze: small. Worth it.**

## Idea 5: `;` between statements on one line

**Gains if free.**
- One grammar for files and one-liners: `jet eval` already separates
  statements with `;` (`Spellings.jet:92`), and source files reject it
  (Jet.ExplicitSemicolon), so code cannot move between them unchanged.
- Short lambdas and functions on one line:
  `xs.each(fn(x) { seen.add(x); total += x })`. About 4,850 blocks could
  shrink, about 16,000 lines (0.8% of the corpus).
- Code that has to live on one line works: a shebang `-e` script, a command
  flag, a config value, a chat message.
- Programmers and agents from C-family languages stop failing builds on `;`.

**Downsides.**
1. Arms. `if done -> save(); exit()` and `loop x in xs -> a(x); b(x)`: does
   the second call belong to the arm? Python says yes, C says no, and braceless C bodies are the
   class behind Apple's 2014 "goto fail" bug. Reach: 76,563 one-line arms.
2. Two separators and a formatter policy. D-FMTCOLLAPSE1 collapses every
   fitting braced block; with `;`, multi-statement blocks would collapse too,
   making code denser and diffs coarser.
3. Line-based tools: `jet debug` breakpoints and steps, `jet perf` line
   attribution, coverage, blame and diffs all key on lines.
4. A trailing `;` habit (`a();`).
5. `;` inside records and argument lists.

**Gleam.** Gleam has no separator at all, not even a line break, because its
grammar never lets the next statement be read as a continuation. Jet's
grammar does in seven places: `(` (call or grouping), `[` (index or list),
prefix `-`, `{` (struct head, record or block), a leading `.` (enum case),
`^place` and prefix `*`. Copying Gleam would mean giving those up. Jet
already matches Gleam where it counts: multi-line code has no separators.
What Gleam adds is one-line juxtaposition, which its formatter immediately
splits onto lines. The lesson to take is that the formatter, not the
separator, decides layout.

**Engineering.**
1. `;` means exactly what a line break means, everywhere statements are
   separated.
2. A one-line arm holds one statement; `;` right after an arm on the same
   line is an error: brace the arm or break the line. That removes the
   ambiguity instead of picking a side.
3. A trailing `;` before a line break is accepted and removed by `jet fmt`,
   as Go does.
4. `;` in a record, argument list or struct body is an error with a fix.
5. Formatter, tier 1: files are printed one statement per line, so stored
   code stays line-addressable. Tier 2, only after `jet debug`, `jet perf`
   and coverage address statements by column: `jet fmt` keeps a block of at
   most two short statements on one line.

**Final verdict.** Tier 1 removes every downside and keeps the
one-grammar, paste and agent gains; files look as they do today. Tier 2
gives the visible one-liners at the cost of column-addressed tooling.
**Juice: moderate for scripting and agents, small for files. Squeeze: small
for tier 1. Worth it.**

## Idea 6: no result type (`-> {…}`) or no braces (`-> a: …; print(a)`)

**Gains if free.** Less ceremony in scripts: no result type to write, and no
braces.

**Downsides.**
- `-> {…}` is already a record result type (`-> {min: Int, max: Int}` runs),
  so `-> {total: a + b}` is either a type or a body.
- Functions that return nothing already omit the arrow.
- Stored functions state their results (D-LAMBDA-STORED1=A). That keeps
  checking per function, which serves the compile-speed target, and keeps
  public interfaces visible.
- Without braces, the body sits where the type sits: `-> Point` could be the
  type or a value named `Point`, because types are values (`Spellings.jet:55`).
  The body would end at the line break, and arm arrows would stack
  (`-> if c -> a else -> b`). Jet retired arrow bodies for functions
  (Jet.ArrowFunctionBody).

**Engineering.** The type/value ambiguity and the record-type clash cannot be
removed without a new marker, which brings back a second body form. Inferring
an omitted result type is a separate idea (inferred results for private
helpers), with its own costs to compile speed and to API stability.

**Final verdict.** **Not worth it; not engineerable without new syntax.**

## Idea 1b: the mark on the name (`^count: 0`)

The owner asked whether another placement of the mark works. The candidates:

| Form | Verdict | Reason |
|---|---|---|
| `count: ^0` (on the value) | works only with windows respelled | `best: ^items[0]` is a window today |
| `^count: 0` (before the name) | **works**, with the engineering below | the name is what becomes writable |
| `count^: 0` (after the name) | works, but worse than `^count` | `count:` no longer finds it; reads as Pascal's pointer `p^`; the mark sits mid-line |
| `count ^: 0` (a new sigil) | no gain | `:=` with another glyph |
| no mark; writable if written anywhere | no | loses E0111 for accidental writes to a fixed name, and the reader's signal |
| a word (`var`, `mut`) | no | a new keyword |

**Gains of `^count: 0`.**
- One binding symbol: every declaration is found by `name:`.
- Changeable state is marked at the start of the line, the most scannable
  place, as Rust's `let mut` is; `:=` sits mid-line.
- One rule for `^`: it sits on what can be written. On a name, the name's
  own value; on a type (`p: ^T`), the caller's storage, as Rust splits
  `mut x` from `x: &mut T`. `^self` already puts the mark before a name.
- Per-name choice in destructuring: `{^lo, hi}: bounds()`. `:=` makes every
  name changeable or none (7 destructuring `:=` sites, 56 with `:`).
- The never-changed fix (D-MUT-NEVER1) becomes "delete the `^`".
- The three jobs of D-BINDSIG-COUNT1 stay distinct (fixed `x:`, changing
  `^x:`, write `x =`), so a misspelt assignment is still an error, and a
  second `^x:` is still a redeclaration (E0118).

**Downsides.**
1. The dev parser still dual-reads a mark before a parameter name
   (`fill: fn(^xs: [Int])`, `Compiler/JetParser/Tests/CaretMarks.jet:20-23`,
   parsed at `Compiler/JetParser/Source/Parser/Items.jet:869-874`) and at the
   start of a line (`^buf.push(total)`, `CaretMarks.jet:50-52`), from the
   D-CALLSITE-MARK2 transition (commit `4be611db588`). There `^name` means
   the caller's storage. Two sites use the parameter form and three the
   line-start form.
2. Named windows: `best: ^items[0]` (a window) and `^best: items[0]` (a copy)
   differ by where one character sits, and both compile. The owner's first
   instinct put the mark on the value, which is evidence that people will
   write the window form when they mean a copy.
3. Cutover of 78,436 `:=` sites; the `:=` token (`CoreSurface.jet:116`)
   retires.

**Engineering.**
1. Retire the dual-read forms, as the greenfield cutover rule already
   requires. In a parameter list, `^name:` becomes an error whose fix moves
   the mark onto the type. Parameters do not get Rust's writable-copy
   meaning: a learner who wrote `fn(^n: Int)` to change the caller would
   otherwise see no error and no change.
2. Named windows become a compiler-owned method (`edit: values[2..3].in_place()`,
   with two windows at once checked by the disjointness proof of
   D-MEMDISJOINT1). Then `^` never sits on a value in a binding, and
   `best: ^items[0]` is simply not Jet: by the owner's greenfield order of
   2026-11-06 it gets no teaching diagnostic. Every misplaced mark fails to
   compile. 217 sites migrate. Passing a part of a list to a `^` parameter
   stays a window by itself.
3. `^` is refused before a struct field, a pattern binding and a loop
   binding (a writing match already binds windows into its subject), and
   `^^` is refused before any binding name.
4. The `:=` cutover is mechanical and keeps meaning: `x := v` becomes
   `^x: v`.

**Final verdict.** Every downside is removed. What remains is the cost: one
window API, 217 window rewrites, the 78,436-site cutover, and reopening
D-BINDSIG-COUNT1 (spelling only) and the window exception of
D-CALLSITE-MARK2. **Juice: modest (scannable state, one rule for `^`,
per-name destructuring, one binding symbol). Squeeze: moderate, mostly the
window respelling. Net positive; the form of the idea that works.**

## Summary

| Idea | Juice | Squeeze after engineering | Verdict |
|---|---|---|---|
| 1 `count: ^0` | small | moderate: new window API, two reopenings, 78k-site cutover | marginal; superseded by 1b |
| 1b `^count: 0` | modest | moderate: window API, 217 windows, 78k-site cutover | **yes, net positive** |
| 2 `count: = 0` | small | permanent: the gap invites `x: Int = 5` | no |
| 3 `= expr` bodies | small once 4 lands | moderate: respelling on growth | no |
| 4 result type shapes the braces | moderate | small: deletes an exception | **yes** |
| 5 `;` as a line break | moderate for scripts and agents | small (tier 1) | **yes** |
| 6 `-> {…}` / no braces | small | not removable | no |

## Ballot review notes (2026-11-25)

Each recommended idea went to a ballot, and a fresh Claude Opus reviewer
attacked each draft against code until it passed. Changes the reviews forced:

- **1b (D-BIND-MARK1).** The `#Policy(writes: .Explicit)` call marks stay
  (D-CALLSITE-MARK2); only the leading parameter mark retires. Every window
  maker (named windows, `name := ^place`, writing matches including the
  one-arm form, writing loop sources: 419 sites) moves to one parser-lowered
  method, `place.in_place()`. `edit`, `window`, `lend` and `mut` were
  rejected as the name for collisions or read-loan wording; the one static
  `in_place` function is renamed and E0436 reserves the name. `_ := v`
  migrates to `^_: v` to keep its owned copy. The search pattern for
  changeable names is `(^\s*|[{,]\s*|loop |#Persist )\^\w+[:,}]`.
- **4 (D-BODY-RECORD1).** The comma rule covers every caller of
  `parser_parse_callable_body` and `#run`. A one-field body builds a record
  only when the declared result carries a value and the value does not start
  with `loop` (labeled loops). The shared helper must skip `<…>` under the
  generic-head test. Two defects stand regardless of the vote: `jet fmt`
  applies E0080 and makes a unit function, and a lambda ending in a record
  where no value is expected crashes at run time.
- **5 (D-SEMI-LINE1).** "`;` means a line break everywhere" was wrong: a
  `;` is accepted only where the lexer's line-break decision would end the
  statement, and is refused after a statement arm. Inside a decision table
  it ends a row. Today's eval rewrite gets this wrong
  (`jet eval 'x := 1; -1; print(x)'` prints 0).
