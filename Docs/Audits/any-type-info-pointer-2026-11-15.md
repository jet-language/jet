# Adversarial review: a Jai-style `Any` for Jet (2026-11-15)

This review attacks the options of ballot D-PRIM-ANY1, "should Jet add `Any`".
Option E adds an `Any` every value converts to implicitly. Option B adds
nothing: a named enum or union holds a known set, a user trait holds an open
set, and a trait method gives a stored type back. Option C adds no `Any`
either: a type arm, `.Point(p)`, tests a trait value's stored type and binds
it. An earlier option A, `Any` with a written box, is kept as a variant of E
(claim 13). The review follows the AGENTS.md adversarial-review order. For each
claimed strength it names the strongest existing solution with sources,
attacks the claim, checks it against code or a probe, and gives a verdict.
Every gap ends on a card or on the ballot.

Ten fresh reviews failed earlier revisions. GoalsA-1-4 failed the first
draft, which recommended E. AnyReview2 failed the second, which recommended B:
the law list was incomplete, the shared prerequisites were wrong, B's
compile-time claim was overstated, and the float defect was misdescribed.
AnyReview3 failed the third, also recommending B: the float story was wrong
under D-LITCARRIER1=D, facts were stale once trait values printed,
`reflect.of` and D-DYNAMIC-TYPE1 were missing, and no option offered type arms
over a trait value. AnyReview4 failed the fourth, which recommended C: it gave
no arm spelling for a type from another module, no privacy rule, six open
forks, an unwritable expert example, and a surface twice the target length.
AnyReview5 failed the fifth, also recommending C: it claimed a type arm could
only copy, gave two conflicting rules for a bare arm, left a union with a trait
member unruled, cited a stale filing state, and missed a defect for another
file's types in a user trait. AnyReview6 failed the sixth, also recommending
C: it cited a filed defect as new, said `recover` does not copy where the
machine still copies, and missed a named union case sharing a member's leaf.
AnyReview7 failed the seventh, also recommending C: it called the copy
unobservable where its address shows, called the true loan one lowering
change, missed the landed fix for same-named members, and left a named case
beside a same-named type unruled. AnyReview8 failed the eighth, also
recommending C: its amendment of `spec.md:641-646` left a package's own
same-named member with no legal table, and it called the view checked as
Rust's borrow is, where a lambda capture and a Shared owner escape the check.
AnyReview9 failed the ninth, also recommending C: its union rule
left a package's own same-named member with no readable arm and kept a silent
`else`, it left two instances of one generic with no arm, and it counted two
escapes from the view check where every carrier escapes. AnyReview10 failed
the tenth, also recommending C: it quoted two Tower criteria as they stood
before they changed, said one arm text names one type per file, left a bare
arm over a union's one generic instance unruled, missed views picked by a
value-producing `if` or table and views in tuples and maps, called a lending
trait method's loan untracked after dev had tracked it, and cited spec lines
from two trees. This eleventh revision answers all ten reviews and recommends
**C**.

## Sources, compiler and probes

- **Mining run** `any-NXt-20261115` (mine-for-jet-v3): the 7.5-minute clip
  "Jai is already better than C" and its 117 comments. The coverage, ledger
  and claims files sit in `$JET_SCRATCH_ROOT/mine/any-NXt-20261115`.
- **Compiler.** Every probe cited here ran on the p210 batch compiler, dev
  `7f902dd915`, identity
  `jet@a0.0.45#8e7e97c2b52745d37c75a7c6a78688ef33bc908c74df3d3499e197dea8dea508`,
  on O0 (`jet run -O0`), O1 (`jet run -O1`) and O2 (`jet build -O2`, then the
  binary); `--optimize=basic` does not exist (E2102). Every probe gave the
  same output on all three tiers. Every probe also gave its p209 output (dev
  `a1c3325a65`) but five: `w03b`, `w03c`, `y01` and `y02` are E0212 on p210,
  since dev c1fca7426c tracks a loan through trait dispatch (#6839 c42's
  check), and `n05`'s `reflect.of` names the private type `Secret` where p209
  gave `M3LibSecret`. Every probe older than review 8 also gave its p207
  output (dev `0e93e25032`), with two changes: `o8b`, `o8f` and `vf/f` are
  E0305 from p209, since a1c3325a65 fixed #6839 c36 (now met), and `y09b`
  gains a cascade E0305 (#6839 c47). The p207 outputs record the old defect.
  Earlier revisions ran on p199, p201, p202, p204, p205, p207 and p209.
- **Snapshot.** Every repository line number cited here (`spec.md`,
  `core-library.md`, `syntax-decisions.md`, the compiler sources) is at dev
  `7f902dd915`. Tower criteria are quoted as they stood on 2026-11-16.
- **Probe directory.** Every probe this review cites, the eleven reviews'
  probes included, is in `$JET_SCRATCH_ROOT/lanes/AnyBallot/probes`. Run
  `./run.sh NAME.jet`; it writes `NAME.p210.out`, `NAME.p210.O1.out` and
  `NAME.p210.O2.out`, each ending in its exit code. `MODE=emit FLAGS=--ir`
  writes O0 IR as `NAME.p210.ir.out`. A file-module probe is run by its
  folder path (`o7_file_module/main.jet`, `o8_file_modules/NAME.jet`,
  `vf/NAME.jet`, `fm/NAME.jet`); its outputs are `o7_file_module`,
  `o8X_file_NAME`, `vf_NAME` and `fm_NAME`. Probe families: `b*`, `c*`, `d1`,
  `e*`, `a1`, `f*`, `l1`, `o*`, `w*` (this ballot); `g*`, `q*` (review 1);
  `r*` (review 2); `x*` (review 3); `n*` (review 4); `v*`, `vf/*` (review 5);
  `y*` (review 6); `z*` (review 7); `a0*`, `fm/a*` (review 8); `e01`-`e11`,
  `fm/f*` (review 9); `h*`, `t*`, `fm/k*` (review 10); `m*`, `fm/m*` (review
  11).

## Recall audits

Audits 1 and 2 failed and the run was re-mined after each; audits 3-11
passed. All eleven are kept in the run directory.

| Audit | Reviewer | Seed | Sample | Result | File |
|---|---|---|---|---|---|
| 1 | GoalsA-1-4 | 4417 | V1, V2, C1, C3 | fail: 7 misses, 4 disputes | `recall-audit-1.json` |
| 2 | AnyReview2 | 90211 | V1, V2, C3, C2 | fail: 5 misses, no disputes | `recall-audit-2.json` |
| 3 | AnyReview3 | 2026 | V1, V2, C1, C2 | pass: 72 concepts, every one matched | `recall-audit.json` |
| 4 | AnyReview4 | 1116 | V2, V1, C2, C3 | pass: 58 concepts (V2 8, V1 14, C2 21, C3 15), 0 misses | `recall-audit-4.json` |
| 5 | AnyReview5 | 7351 | V2, V1, C3, C1 | pass: 71 concepts (V2 11, V1 17, C3 19, C1 24), 0 misses, 0 disputes | `recall-audit-5.json` |
| 6 | AnyReview6 | 27295 | V1, V2, C3, C2 | pass: 73 concepts (V1 19, V2 11, C3 19, C2 24), 0 misses, 0 disputes | `recall-audit-6.json` |
| 7 | AnyReview7 | 52011 | V2, V1, C1, C3 | pass: 86 concepts (V2 11, V1 23, C1 30, C3 22), 0 misses, 0 disputes | `recall-audit-7.json` |
| 8 | AnyReview8 | 9545 | V1, V2, C1, C2 | pass: 97 concepts (V1 24, V2 12, C1 34, C2 27), 0 misses, 0 disputes | `recall-audit-8.json` |
| 9 | AnyReview9 | 11815 | V1, V2, C1, C3 | pass: 92 concepts (V1 25, V2 11, C1 33, C3 23), 0 misses, 0 disputes | `recall-audit-9.json` |
| 10 | AnyReview10 | 28892 | V1, V2, C2, C1 | pass: 97 concepts (V1 27, V2 12, C1 33, C2 25), 0 misses, 0 disputes | `recall-audit-10.json` |
| 11 | AnyReview11 | 14666 | V2, V1, C3, C1 | pass: 125 concepts (V2 14, V1 29, C3 37, C1 45), 0 misses, 0 disputes | `recall-audit-11.json` |

**Audit 1 misses**, re-mined in the second revision:

| Miss | Now held by |
|---|---|
| V1 00:03:02: one-step build and run with explicit imports | ledger `v1-13`, claim K13 |
| V2 00:05:30: implicit `it` and `it_index` | ledger `v2-6`, claim K14 |
| C1 #3: undefined order as a feature | ledger `c1-13`, claim A13 |
| C1 #22: Java and Smalltalk `Object` | ledger `c1-10`, claim A12 |
| C1 #34, #40-#43: Jai availability | ledger `c1-16`, out of scope with a reason |
| C1 #44-#49: the C ceiling replies | ledgers `c1-17`, `c1-23`, `c1-24`, out of scope with reasons |
| C3 #98, #102: the price of ordered tables | ledgers `c3-11`, `c3-12`, claim A15 (a measurement gap) |

**Audit 2 misses.** Every miss was a second idea inside a comment whose first
idea the ledger already held. All five are off-topic for `Any`; each now has
its own ledger item and claim:

| Miss | Now held by |
|---|---|
| C2 #55: C predates today's word sizes and multicore | ledger `c2-14`, claim A20 (rejected: Jet already models both) |
| C2 #58: "defer in gcc when" | ledger `c2-15`, claim A21 (evidence: D-JAI-DEFER1) |
| C2 #70: compilers can produce useful AST and debug views | ledger `c2-19`, claim A22 (evidence: `jet explain`, `jet inspect`) |
| C2 #72: LLMs "unroll the loop" | ledger `c2-20`, claim A23 (rejected: no Jet change) |
| C3 #86: a linear scan can beat hashing on small tables | ledger `c3-14`, claim A24 (evidence: `Dyn` objects scan pairs) |

After audit 2 the same class of miss was sought in every segment, and compound
comments in C1 and C2 were split into their own ledger items (C1 #25, #26,
#27, #30, #35, #46, #49; C2 #59, #63, #65, #66, #69, #72). Audit 3 drew C1,
which no audit had re-checked since audit 1 failed it, and found no miss.
Audit 4 found no miss either. Its notes moved claim K10 and ledger `c3-1` off
`encoding-decisions.md:35` onto D-LITCARRIER1=D, and recorded `c3-8` as
covered by claim A1. Audit 5 re-mined V2, V1, C3 and C1 from the transcript
and the ordered comments before opening the ledger, and found no miss. Audit 6
did the same for V1, V2, C3 and C2 and found no miss; its one precision note,
that frame `f_000615` reverses only `foo`, is now in ledger `v2-3`. Audit 7
did the same for V2, V1, C1 and C3 and found no miss. Its two precision notes
are now in the ledger: `c1-10` no longer says comment #22 names `instanceof`,
and `c1-12` puts the INI use at 02:41, after the clip explains `Any`'s type
info and pointer. Audit 8 did the same for V1, V2, C1 and C2 and found no
miss. Its precision note is now in the ledger: `v1-14` names float, string
and bool, and adds that the file then holds the integers 1337 and 80085
(`v1-8`). Audit 9 did the same for V1, V2, C1 and C3 and found no miss. Its
three precision notes are now in the ledger: `v2-7` adds the link time, the
compiler time and the 27538 lines with blanks and comments; `c1-10` quotes
comment #22's question about Java and Smalltalk, without the Object gloss;
and `c3-5` records the #115-#116 dispute over Rust's purpose. Audit 10 did
the same for V1, V2, C2 and C1 and found no miss. Its precision note is now in
the ledger: `c1-11` adds comment #27's closing claim, that `Any` exists only
to silence the checker, giving a weaker type system and buggier code, which
`c1-1` and `c1-18` already carried.

## Ratified law

The no-top-type rule is a frozen wall: `Docs/audits/whole-language-frame.md:32`
lists "no top type" among the walls that stand. The rule for challenging one
is `Docs/audits/whole-language-audit-prompt.md:17`. A challenge "must carry
worked code, the measured cost of the wall today, and a named amendment in its
ballot." The worked code is each option's program below. E challenges the
wall; B and C keep it.

**D-ANY-JAI1** (`Docs/spec/syntax-decisions.md:751-754`, card #169
`c7jaiany`, ratified 2026-07-01) has three parts:

1. Heterogeneous varargs are trait-bounded, `parts: ...Renderable`, with trait
   sets as list bounds (D-VARARGBOUND1).
2. "No top type; general `Any` rejected."
3. A read-only reflection floor, `reflect.of`
   (`Docs/spec/reference/core-library.md:2550-2593`). The ratified story is
   an inspector panel for game entities whose gameplay APIs still say
   `Health` or `Position`, not `Any`. The floor "has no runtime type registry",
   and `get`, `set`, `clear`, `copy`, `equal`, `getfile`, `getmodule` and
   `loadfile` "remain declined" (`core-library.md:2589-2593`).

**D-DYNAMIC-TYPE1** (Tower card `c3gdynamic`, ratified 2026-06-30, option A):
"No general `Any` — existing mechanisms (enums, traits, generics, T?, data
values) cover all dynamic cases without a top type." Its rejected option C was
a boxed `Any` gated to interop edges with an explicit `downcast<T>()`. The
retired diagnostic E0350 once cited it (`Compiler/Prelude/Diagnostics.jet:5563`;
`Docs/audits/diagnostic-actionability-census.json:5164`).

Eleven places state or rely on the rule. Under E each changes as follows:

| Place | Text today | Amendment under E |
|---|---|---|
| D-ANY-JAI1, `syntax-decisions.md:751-754` | "No top type; general `Any` rejected." | part 2 superseded by D-PRIM-ANY1; varargs and `reflect.of` stay |
| D-DYNAMIC-TYPE1, Tower `c3gdynamic` | "No general `Any`" | superseded on Tower |
| D-UNIONTYPE1, `:765` | "Does not reopen D-ANY-JAI1." | the sentence is struck; `Any`'s arm reuses the union member test |
| D-TYPESWITCH1, `:796-797` | "Run-time dispatch over mixed values uses unions; no `Any` type (D-ANY-JAI1 stands)." | "uses unions, or `Any` for an open set" |
| D-INFER-PRIVATE1 `Dyn` note, `:5355` | "(D-ANY-JAI1 stands)" | the parenthesis is dropped; `Dyn` still holds data only |
| `Docs/spec/spec.md:7235-7236`, "Deliberately absent" | "Jet does not provide a general `Any` top type." | the bullet leaves the list; `Any` gets a spec section |
| Typed selection, `syntax-decisions.md:1166-1169` | "A checked downcast of a foreign value is not Jet syntax" | an exception for the checked case over an `Any`; foreign values still arrive typed |
| Frozen wall, `whole-language-frame.md:32` | "no top type" | the wall is removed from the list |
| Card #744 c10 | "D-ANY-JAI1 remains intact: no Any/top type or unchecked cast path is added." | recorded as superseded; the union itself is unchanged |
| `Docs/proposals/reflection-2026-11-04.md:66` | "A dynamic `Any`: it was rejected when `reflect.of` was ratified." | replaced by a pointer to D-PRIM-ANY1 |
| `Examples/Types/NoAnyAlternatives.jet` | "Jet has no top type like Any." | rewritten to say when `Any` beats an enum, a generic, `?T` or `Dyn` |

**Under C** the wall, D-ANY-JAI1 (all three parts), D-DYNAMIC-TYPE1, the
`Dyn` note, `spec.md:7235-7236`, card #744 c10, the reflection proposal and
`NoAnyAlternatives.jet` stay as written. C changes these places:

| Place | Text today | Amendment under C |
|---|---|---|
| D-TYPESWITCH1, `:796` | "Run-time dispatch over mixed values uses unions" | "uses unions, or type arms over a trait value" |
| Typed selection, `:1169` | "a value whose shape varies is declared as an enum and selected as above" | adds "or held as a trait value and selected with a type arm"; the foreign-value clause (`:1166-1168`) is unchanged, since a trait value is a Jet value |
| Typed selection, `:1163-1166` | a payload bound from a borrowed list is a loan of its element | applies to a type arm's binding as written; no change |
| D-UNIONTYPE1, `:760-762` | member arms `.Int(n)` over a union | the same arm over a trait subject, with the spelling of claim 19 for both; "Does not reopen D-ANY-JAI1" stays true |
| `spec.md:945-948`, trait values | dynamic dispatch with invisible boxing; prints as the concrete value | gains the type arm |
| `spec.md:641-646`, same-named union members (landed with dev a1c3325a65) | first sentence: "A union member is the type itself, never its name … holds two members, and each value keeps the member it was built as"; second: "a `.Point` arm over that union names neither member (E0305); only an `else` reaches them, and a table over such a union keeps its `else` without E0641"; third: "To match each, make them named cases" | the first sentence stays (the #6839 c36 fix); the second and third become "A bare `.Point` arm names the member `Point` names in type position there; a package's own `Point` is `.Point(p)` beside `geo.Point`. If `Point` names no type there, it names the one member whose leaf is `Point`. A generic's one instance takes its generic's name and leaf; two instances spell their arguments, `.Box<Int>(b)`, as do generic arms over a trait value. Any other bare arm is E0305, offering `.geo.Point(p)`. So every member has an arm; an `else` in a table is E0641." |
| `spec.md:903-905`, an `else` over a closed set is E0641 | applies to a union, with `spec.md:644-646`'s exemption for same-named members | holds with no exemption |
| E0641's exemption, `Compiler/JetSema/Source/Sema/Coverage.jet:988-991` | exempts two module members of one leaf (identity tags, `:933-953`), but not two instances of one generic (`fm/f01b`) | removed |
| E0641's fix | offers the internal tags `.M_Box_Int_ \| .M_Box_String_`, which compile and bind (`fm/f01c`, `fm/f01d`, #6839 c55), and `.M_Box_Int_` for one instance (`m33`) | offers each member's arm, `.Box(b)` for one instance and `.Box<Int>(b)` for two; internal tags are refused as arms (E0305) |
| E0307's fix, `Coverage.jet:521-523` | "add `else -> { … }` after the last arm: no arm can name geo2.Point, map2.Point, which share a type name" (`fm/a09`); a package's own member spelled `filestem.Point` (`fm/f06`, #6839 c52) | names the arm each missing member takes: `.Point(p)`, `.geo2.Point(p)` |
| E0305, `Compiler/JetSema/Source/Sema/Patterns.jet:294`, `:338` | "variant patterns only work on enum values" | names unions and trait values too |
| E0305's same-named-member branch, `Patterns.jet:152-169` | the fix names only named cases, and spells a package's own member as `filestem.Point`, which fails (`fm/a04c`, #6839 c52); over `Box<Int> \| Box<String>` the text says "doesn't belong" (`fm/f01`, #6839 c55) | fires only for a non-member type of that name or a leaf two out-of-scope members share, and offers the qualified arm; over two instances it says "names more than one member" and offers `.Box<Int>(b)` |
| E0305 Prelude entry, `Compiler/Prelude/Diagnostics.jet:4702-4716` | safety reason "A pattern of another type would read the value's bytes as that type."; expert path "none: an explicit conversion reaches the program" | the expert path names the checked type arm; the safety reason stays true of unchecked reads |
| `Docs/audits/2026-10-07-owner-notes-digest.md:113` (agent digest, not law) | "A trait value (`fn f(s: Shape)`) is dynamic dispatch with no downcast." | corrected |
| `Docs/research/evidence-2026-09-29/3389-typed-selection.md:100-101` | "any foreign downcast helper needs its own ballot" | C is not foreign; this ballot records it anyway |

C is not D-DYNAMIC-TYPE1's rejected option C. That option was an `Any` any
value could become, with an explicit downcast at interop edges. Under option C
no type exists that every value fits: a value enters a trait value only if its
type opted into that trait, and an arm may name only such a type. Card #3389
c3 (archived, a duplicate of #3132) says no type-erasure feature is added
"solely to imitate a query method name"; C answers a recovery job, not a name.

D-ALT-SUBSET1 (unions widen into larger unions), D-ALT-CASES1 (a named case
cannot reuse a type member's name, E0363), S48 (a trait name in type position
boxes invisibly) and the closed-set ruling on #6281 (an `else` over a closed
set is E0641) bear on the options but need no amendment. A qualified member's
name is its declared name, the last segment, so `Point(Int) | geo.Point`
already breaks D-ALT-CASES1; the compiler accepts it today (`y09`, `y09d`,
#6839 c43). The landed `spec.md:641-643` reads names the same way: it calls
`geo.Point` and `map.Point` "two types named `Point`". A trait value is an
open set, so a table over one needs `else`, as D-TYPESWITCH1 already says of
types. Visibility law bears on C: "Declarations are public by default", and
"`priv` is the only hiding form" (`spec.md:3509-3512`, `3568-3569`).

**The wall's measured cost**, counting each program's body. E's is five lines.
B's open-set form is eleven (review probe `x7`): a three-line trait, three impl
lines, one carrying a one-line override, and the five-line loop. C's is nine:
a one-line empty trait, three empty impl lines and E's five lines (`c1`). Each
structural type, such as a list, needs a one-field wrapper struct under B or C
(`b7`). B's known-set form adds a five-line enum, wraps each value as
`.Num(14)`, and reads with an eight-line table (`r8`).

## The jobs `Any` would serve

The clip uses `Any` for a config file. Jet already has a ratified answer for
each nearby job. Outcomes are on p210, the same on O0, O1 and O2.

| Job | Jet's answer | Evidence |
|---|---|---|
| A value whose type the caller picks | generic type table (D-TYPESWITCH1) | `g1`: `15`, `hi`. A bare `2.5` takes the `Decimal` arm and `Float{2.5}` the `Float` arm (`x4`) |
| A known set of types | named enum, D-UNIONTYPE1's "documenting form", or a union | `r8`, `b13`: the enum list prints the four lines; `f1` adds `2.5` as a `Float` payload. One union value runs (`b3`, `f2`), but not as a generic function's parameter, an ICE (`a07c`, #6839 c49). A union list is an ICE (#6839 c26, `r6`) |
| An open set of types | a user trait (B, C), or `Any` (E) | `x7`, `b13`: the `[Shown]` list prints with `print(item)` and gives the Point back through `point()`. That holds for local and inline-module types (`v01e`); a file-module type in the user's trait is an ICE (`v01`, `v01b`) and its direct call E0102 (`v01c`), #6839 c39 |
| A call with mixed arguments, such as print or format (comment #111) | D-ANY-JAI1's trait-bounded variadic; no box | `x5`: `inspect(parts: ...Printable)` prints 14, `Float{69.420}`, hi and the Point |
| Inspecting a concrete value, the ratified inspector panel | `reflect.of` (D-ANY-JAI1 part 3) | `x10`: a list of `reflect.of` values prints `Int: 14`, `String: hi`, `Point: Point { x: 1, y: 2 }` with fields, and `Decimal: 69.420`, with no opt-in. Over a trait value it reports the internal key (`n09`, #6845 c16). It cannot give a value back |
| Loose data such as a config file | `Dyn` and `decode<T>` | `d1`: `[Dyn]` prints and `decode<Point>()` gives the Point back |
| Typed lookup by a type parameter (the clip's `get_ini_field($T)`) | `Dyn`'s `decode<T>`, or `Core/Encoding/Ini.jet:125-140`'s typed getters | `d1`; `get_int`, `get_bool`, `get_float`. No option changes this job |
| A registry keyed by type | `[Type:Int]` | `g3`; `counts[Int] = 1` runs for a builtin (`g3b`, #6845 c14) |
| A value crossing a loaded `.jetlib` | undecided; see claim 15 | D-LIB-DYNTRUST1=A; `BuildTraits.jet` stops on an unknown key |

Reflection already does the inspector's printing. The open job is taking one
stored value back as its own type.

## The three options, one program

Each option runs the same program: hold 14, `"hi"` and `Point{x: 1, y: 2}`,
print every item, then print the Point's `x`. The intended output is `14`,
`hi`, `Point { x: 1, y: 2 }`, `1`. The program leaves out the clip's `69.420`,
a `Decimal`, because a `Decimal` cannot enter a trait value today (claim 14).

```jet
// E: implicit conversion. Today E0112 at the first item (probe e1).
items: [Any]{14, "hi", Point{x: 1, y: 2}}
loop item in items {
    print(item)
    if item == .Point(p) -> print(p.x)
}

// A, E's variant with a written box (claim 13). Today E0119 (probe a1).
items: [Any]{Any{14}, Any{"hi"}, Any{Point{x: 1, y: 2}}}
```

```jet
// B: runs today on O0, O1 and O2 (probe b13; the Point struct and the run
// function are omitted here).
// Open set: each stored type opts into a user trait.
Shown: trait {
    point: fn(self) -> Point? { None }
}
impl Int.Shown {}
impl String.Shown {}
impl Point.Shown { point: fn(self) -> Point? { self.copy() } }

// Known set: a named enum lists the types, and the table must handle each one.
Item: enum {
    Num(Int)
    Text(String)
    At(Point)
}

items: [Shown]{14, "hi", Point{x: 1, y: 2}}
loop item in items {
    print(item)
    if item.point() == Val(p) -> print(p.x)
}
known: [Item]{.Num(14), .Text("hi"), .At(Point{x: 1, y: 2})}
loop item in known {
    if item == {
        .Num(n) -> print(n)
        .Text(t) -> print(t)
        .At(p) -> {
            print(p)
            print(p.x)
        }
    }
}
```

```jet
// C: today E0305 at the arm on O0, O1 and O2 (probe c1).
Shown: trait {}
impl Int.Shown {}
impl String.Shown {}
impl Point.Shown {}

items: [Shown]{14, "hi", Point{x: 1, y: 2}}
loop item in items {
    print(item)
    if item == .Point(p) -> print(p.x)
}
```

## Claim 1: `Any` adds no storage

**Strongest prior art.** Rust's `dyn Any` is an ordinary trait object with one
blanket impl and a `TypeId` constant (doc.rust-lang.org/std/any). Go's `any`
is the empty interface (go.dev/ref/spec, "Type switches"). Swift's `Any` is an
ordinary existential (Swift ABI `TypeLayout.rst`).

**Attack.** Jet's dynamic call is a switch over the implementers the build
knows (`ir_trait_implementers` in `Compiler/JetIR/Source/BuildTraits.jet`),
not a vtable. A trait every type has would make that switch range over every
type. `ir_trait_implementers` also skips generic functions, so a trait value
of a generic type is an internal compiler error today (review `q6`, #6727 c6).

**Check.** A trait value is a counted box with the key at offset 0 and the
payload at offset 8 (`ir_trait_box`, `BuildTraits.jet:332-354`, `:457`;
`q1` IR: `jet_rt_alloc(40, 8)`, key stored at 0, `copy_bytes 32` to offset
8). `Any` would add no field, and neither would C's type arm.

**Verdict: holds within limits.** The switch for `Any` must cover the types
converted to `Any`, every implementer of a trait value converted to `Any`, and
every generic instance converted inside generic code. The generic case waits
on #6727 c6.

## Claim 2: the box key is a type identity reflection can use

**Strongest prior art.** Rust's `TypeId` is a 128-bit hash of the type, the
same value from `TypeId::of::<T>()` and from `dyn Any::type_id()`.

**Attack.** The first draft said the key is "the same identity reflection's
TypeId names". It is not. `ir_trait_identity` hashes `ir_render_type_key`,
for example `module:14:source-root:0::q6_generic_trait_value.jet::Pair<A, B>`.
The key is the first 56 bits of that string's SHA-256, plus one
(`BuildTraits.jet:9-28`), so it depends on the identity alone, not on build
order. Each IR module records the keys it gives. Today a collision panics
(`:21-24`), an internal-error path; #6845 c18 makes it a registered
diagnostic naming both types, as the collect-all rule requires. C needs c18,
since the key is what an arm tests. A TypeId's identity is the
module-qualified name such as `shapes.Point`
(`Sema/Comptime/Reflection.jet`, about lines 1475-1491).
`reflect.of` on a trait value shows the render key today
(`module:14:source-root:0::FILE::Shown`, `n09`, `n13`; #6845 c16).

**Verdict: fails as first worded; fixed wording holds once built.** The box
stores a key that reflection maps to the type's TypeId. #6845 c14 requires
that `ValueInfo.type` of an erased value equals `T.reflect()`'s TypeId through
one key-to-TypeId mapping, stable across builds that add a module. Whether the
module row number in the render key is stable is unverified. C's arm compares
keys inside one build and does not need the mapping.

## Claim 3: converting a trait value to `Any` keeps its inner key

**Strongest prior art.** Rust's trap: calling `type_id()` on a `Box<dyn Any>`
reports the box's TypeId, not the payload's. Rust 1.86 made trait upcasting
stable, so a trait object reaches `dyn Any` as a supertrait object.

**Check.** `ir_trait_box` returns an existing trait object unchanged, so a box
never holds a box in IR. At the user level, passing a `Shape` value where
`Named` is expected is an internal compiler error (review `q2`). Passing a
union value where a trait value is expected is one too (`n01`, #6839 c34).

**Verdict: holds in IR, fails at the user level today.** The fix is #6727 c7.
E needs it, because a trait value converts to `Any` implicitly. C needs it
only to take a type back after a trait-to-trait conversion, and needs #6839
c34 for a union value (claim 21).

## Claim 4: recovery is safe, total and familiar

**Strongest prior art.**
- **Java:** `if (obj instanceof Point p)` tests and binds `p` only where the
  test matched (JEP 394). A Java 21 pattern switch must be exhaustive: over
  `Object` it needs a `default` label or an unconditional pattern, and over a
  sealed interface it needs neither (JEP 441). The audience named Java's
  `Object` (comment #22).
- **Go:** `switch v := x.(type)` binds `v` per case. "x must be of interface
  type", and "each non-interface type T listed in a case must implement the
  type of x" (go.dev/ref/spec, "Type switches"). A case may name another
  interface, `default` is optional, and cases can name list and function
  types.
- **C#:** a type pattern, `obj is Point p`, tests and binds; a `switch`
  expression over `object` needs a discard arm to be exhaustive
  (learn.microsoft.com, "Pattern matching - the is and switch expressions").
- **Kotlin:** `is` tests a type and smart-casts the value (kotlinlang.org,
  "Type checks and casts"); a `when` over a sealed class needs no `else`
  (kotlinlang.org, "Sealed classes").
- **Rust:** `downcast_ref::<T>()` returns `Option<&T>`, a reference, and works
  only for `'static` types (doc.rust-lang.org/std/any/trait.Any.html). Since
  1.86, upcasting lets a trait object call `dyn Any`'s downcast methods
  "without adding any trait methods or using external crates", when the trait
  names `Any` as a supertrait, as in `trait MyAny: Any {}`
  (blog.rust-lang.org/2025/04/03/Rust-1.86.0).
- **Swift:** "The conditional form, `as?`, returns an optional value of the
  type you are trying to downcast to" (TSPL, Type Casting). C++ `any_cast`
  returns null or throws.
- **Jai, the source:** `get_ini_field` compares `field.type` with
  `cast(*Type_Info) T` and returns `*T` or null after logging (clip, 01:20).

Java's and Kotlin's split is Jet's split. A union or enum is the sealed set,
matched with no `else`; D-TYPESWITCH1 requires `else` over the open set of
types.

**Attack.**
- **The parent mechanism.** Recovery under E is D-UNIONTYPE1's member test
  (`.Int(n)`, `.Point(p)`; `Examples/Types/AnonymousUnions.jet`), extended to
  an open set. Under C it is the same test over a trait value. The `else` rule
  is D-TYPESWITCH1's open-set rule. An `Any` holding a `Dyn` matches
  `.Int(n)` as the type `Int`, never as `Dyn`'s `Int` case.
- **Names.** An arm can name a named type: a builtin, struct, enum or distinct
  type. `.Pair<Int, String>(p)` does not parse today (review `g6`, E0003).
  `[Int]`, `Int?`, function types and closures have no spelling after the dot,
  so under E their values reach `else`. Go's cases name such types, so this is
  a gap in Jet's spelling, not a property of `Any`. Under C the gap cannot
  arise: only named types implement traits (`impl [Int].Shown` is E0003,
  `b6`). Names from other modules are claim 19.
- **Trait values today.** `.Point(p)` over a `Shown` value is E0305, "Variant
  patterns only work on enum values" (`b8`, `c1`; `c3` for a table). Recovery
  from a trait value goes through a method the trait declares (`x7`,
  `point() -> Point?`).
- **Typed selection.** "A checked downcast of a foreign value is not Jet
  syntax" (`syntax-decisions.md:1166-1169`). E and C need the amendments in
  the law tables.

**Verdict: holds within limits for E and C.** No option adds an unchecked
cast. Each needs the extended member test in the spec, a diagnostic snapshot
for it (#6833 c4 under E; #6839 c35, c38 and c40 under C), and generic
arguments in member arms (#6833 c3). E also needs unnameable types reaching
`else`.

## Claim 5: an `Any` value prints with no per-type code (clip, 05:53)

**Strongest prior art.** Jai and Odin print an `Any` through its type info;
Go's `fmt` prints an `interface{}` by reflection. Rust's `dyn Any` prints only
`Any { .. }`.

**Check.** Trait values print as their concrete value (#6845 c13, met): `x7`,
`r9`, `q9` and `q10` print every item on O0, O1 and O2. A value of a trait
with no methods prints as an empty line, directly, interpolated
(`"bare: {s}"` prints `bare: `) and in a list (`c2`, `c2b`, `n02`). A trait
with one default method prints correctly in the same file. `spec.md:946-948`
says a trait value "prints, interpolates and debug-prints as the concrete
value it holds", and c13 says "for every trait". A struct declared in an
inline module prints mangled, `M3GeoPoint { x: 1, y: 2 }` (`n16`, #6845 c17).

**Verdict: holds, with two filed defects.** `print(item)` works for every
option whose trait has a method. C's program uses an empty trait, so it needs
#6845 c15. `reflect.of` prints a concrete value with no opt-in (`x10`).

## Claim 6: cost

**Strongest prior art.** Swift's `Any` has a three-word inline buffer, so a
small value never allocates. C++ `std::any` encourages small-object storage.
C# boxing "wraps the value inside a System.Object instance and stores it on
the managed heap"; boxing is implicit, unboxing explicit (learn.microsoft.com,
"Boxing and Unboxing"). Jai stores two words that point at the caller's value.
Odin's `any` is a data pointer and a `typeid`, and "passing a literal to an any
will allocate the literal in the current stack frame" (odin-lang.org,
overview).

**Check (O0 IR).** Every conversion to a trait value allocates a heap box
and copies the payload, read arguments included: a 32-byte struct passed to
`fn(s: Shape)` emits `jet_rt_alloc(40, 8)` and `copy_bytes 32` (`q1`). The `b5`
list boxes `14` in 16 bytes. A union argument allocates
`jet_rt_alloc_value(24, 8)` per call (`b3`). A named-enum list allocates per
element (`b4`). O1 and O2 IR could not be read: `jet emit` refuses both with
E2102.

**Verdict: holds for today's implementation only.** Today every option
allocates per stored value. A union or enum has a size known at compile time,
so it can be stored inline; a trait value or an `Any` needs a heap box or a
bounded inline buffer. A written box, A's variant, would show the allocation
that E hides. C's type arm adds no allocation: it loads the key, compares it
with a constant and views the payload. `recover`'s `T?` result allocates its
own cell today, as the copy form's does (`y12`, claim 22). A table of N type
arms costs up to N compares, where a union's tag can switch. Inline storage
for small values is G11 work for every trait value, proved by a paired cell.
Comment #83 raised the layout fear.

## Claim 7: `Any` will not become TypeScript's `any`

**Strongest counter-evidence.** Viewers call TypeScript `any` the "make the
compiler stop throwing errors" switch and predict overuse (comments #4, #32,
#35, #37). TypeScript 3.0 added `unknown`, "the type-safe counterpart of
`any`", on which "no operations are permitted ... without first asserting or
narrowing" (TypeScript 3.0 release notes). Comment #25 puts the dilemma: `Any`
loses every guarantee unless the compiler lets you do nothing with it. Comment
#27 adds that a run-time tag check loses local reasoning. Comment #4 also
calls Jai's `Any` a fantastic feature where sum types would be clumsy.

**Check.** An `Any` under E would be TypeScript's `unknown`, not `any`: no
members, calls or operators until an arm narrows it. Under E a wrong type
entering an `Any` is not an error anywhere; it reaches the reader's `else`.
A's written box would refuse it at a call unless written as `Any{...}` (claim
13). Under B and C a type that never opted in is refused where it enters
(`b9e`).

**Verdict: holds within limits for E; B and C avoid it.** Comment #4's
clumsiness is real for an open set, where B and C cost one impl line per type
and a wrapper struct for structural types (`b6`, `b7`).

## Claim 8: type information is paid for only when needed (clip, 02:07)

**Prior art.** Jai bakes `Type_Info` into the executable only for types used
at run time. Rust's `TypeId` is a constant with no metadata. Zig's `anytype`
is resolved at compile time: "the parameter types will be inferred when the
function is called" (ziglang.org, Function Parameter Type Inference), so it
needs no run-time type information; Jet's generics and trait-bounded varargs
are the same tool.

**Check.** The box key costs 8 bytes in the box and no table. Full metadata
(`ValueInfo`) is ratified under D-META-REFLECT2 for requested roots only and
is unbuilt (#6845). `reflect.of` already reads type names and fields (`x10`).

**Verdict: holds** for the key, and **holds within limits** for metadata,
once #6845 builds the retention rule. This applies under every option.

## Claim 9: `Any` is the right tool for the clip's own job

**Prior art.** The clip parses INI into `Table(string, Table(string, Any))`.
Python's configparser keeps text; TOML and JSON libraries return a dynamic
tree.

**Check.** Jet's `Dyn` is that tree: objects keep arrival order
(D-DATATREE-ORDER1) and numbers keep their source text (first-draft probe
`p6`). Jet's INI codec is text with typed getters (`Ini.jet:125-140`). The
clip's `Ini.jai` header admits a private dialect. The clip's `69.420` is, read
as Jet reads it, a `Decimal` (claim 14); `Dyn` carries it.

**Verdict: the clip's job does not justify `Any`.** INI joins the data tree
under #6883 (one strict profile per format).

## Claim 10: safer than Jai's and Odin's `Any`

**Prior art.** Jai's `Any` and Odin's `any` are views of the caller's value;
Odin says an `any` is valid only while its data lives.

**Check.** `ir_trait_box` lends a read argument's bits into the box
(`ir_store_lent`). No escape probe for `Any` exists, because `Any` does not.
Today a read trait-value parameter returned as-is is E0120, "`s` was not moved
here, so it cannot be returned as-is" (`l1`); its fix text names
`-> View<T> from s` (`n15`).

**Verdict: holds within limits for C; unverified for E's `Any`.** C's binding
follows typed selection: a view leaves only under a `from` clause naming the
source, a view of a temporary is E2305 (`y06b`, `y06c`), a view of a named
owner is E0212 when the owner moves (`w01b`, `y06`), and moving out takes the
source (claim 22). That check covers a view only while a plain local holds
it: a view picked by a value-producing `if` or table, held by a lambda, a
carrier (tuples and maps included), a defer, a method value or a Shared cell,
or read from a Shared owner, escapes it today (`a01`, `a03b`, `e01`-`e09`,
`h02`, `h05`, `h06`, `h08`-`h10`; #6839 c50, c51, c54, c56), safe only while
the view is a copy. Under E it is a required build criterion:
#6833 c3 makes an `Any` value holding a view of a local, returned from its
function, fail with the provenance diagnostic.

## Claim 11: option B serves the open set today

**Strongest prior art.** Go and Rust programmers reach for `any` and `dyn Any`
because declaring an interface per use is friction; Java uses `Object`.

**Check.** `x7` and `b13` run the open-set program on O0, O1 and O2 with
`print(item)`. Default methods make `impl Int.Shown {}` and
`impl String.Shown {}` one empty line each, and `Point` overrides `point()`. A
generic helper with a type table makes `Point`'s line empty too (`r3b`), but
the trait still declares one method per target type; a `Self` type table in
the default is E0107 (`r3`). A structural type cannot take an impl:
`impl [Int].Shown {}` is E0003 (`b6`); a one-field wrapper struct works
(`b7`). A Core trait cannot serve as the element type: `[Printable]{14}`,
`fn(value: Printable)` and `[Debug]{14}` are E0112 (`c4`, `c4b`, #6839 c32).

**Attack.** Taking a type back needs a method on the trait. For the user's
own trait that is one default method and one override (`b13`). A user of a
library trait cannot add one: the trait belongs to its module, and over a
trait value `.Point(p)` is E0305 (`b8`). The in-language workarounds fail: a
generic trait method `pick<T>()` is refused, "Trait method declarations do not
carry independent type parameters" (`x9`, #6839 c31), and a type parameter as
a type table arm is E0119, "`S` isn't a known type" (`x8`). `reflect.of`
inspects but cannot give the value back (`x10`).

Review 11 tried every route from a user to a library trait's type, with the
library in a file module (`fm/lib8`), on O0, O1 and O2. A free generic
function's type table sees only the trait, so a library `Point` takes `else`
(`fm/m41`). Adding a method to `lib8.Shown` from outside is E0003 (`fm/m42`),
and a second impl of it for `lib8.Point` is E0902 OrphanImpl (`fm/m45`). A
trait of the user's, `Back`, implemented for `lib8.Point`, reaches a `Shown`
value only if the value converts to `Back` keeping its inner key; that
conversion is an internal compiler error today, "checked incomplete without a
report" (`fm/m43`, `fm/m46`; #6727 c7). Review 11 asks c7 to refuse it under
B and C. `impl lib8.Shown.Back {}` is accepted but inert: a `Shown` value has
no `point()` (E0311, `fm/m44`, `fm/m44b`), and at a `T: Back` bound it gets a
wrong E0904 (`fm/m47`; #6839 c58).

**Verdict: holds within limits.** B serves the open set for printing and for
the types its trait declares a method for. Its distinct loss bites only for
values a library hands over already boxed as its own trait, where the user
cannot add the method. Its matching gain is control: a library decides which
of its types callers take back (claim 20).

## Claim 12: B checks a wrong type at compile time

**Strongest prior art.** A Java sealed interface with an exhaustive switch
checks recovery at compile time; a Go interface with methods refuses a type
that lacks them.

**Check.** B's open set checks only that a type opted in: a `String` with no
impl is E0905, "`String` isn't `Shown`" (`b9e`). Probe `r7` stores a `Point3`
with an empty `impl Point3.Shown {}`; the reader's `point()` returns `None`,
printing `found a Point: false` on O0, O1 and O2. B's closed set checks
recovery itself: a table over the named enum that forgets `.At` is E0307,
"This `if` doesn't cover every case — missing: At" (`b12`).

**Verdict: holds as two statements, and the second is shared.** A type that
never opted in is refused where it enters, under B and C. A stored type the
reader did not plan for reaches `else` under E and C, and has no method under
B: that constraint holds for every option and is not B's loss. Full
compile-time checking of recovery comes only from a closed set.

## Claim 13: A, the written box, is a separate option

**Strongest prior art.** `Docs/research/2026-11-09-any-type.md:253-256`: A's
only difference from E is the explicit call. A trait bound limits what
converts, but "`Any` has no bound, so implicit conversion would accept every
wrong argument." That research design also stored small values inline.

**Check.** The review rules allow at most three real alternatives
(`.agents/skills/mine-for-jet-v3/references/review.md:204-207`). A shares
E's law cost, carrier, dispatch and arm; it differs in one conversion rule.

**Verdict: fails as an option of its own; kept as E's variant.** A's merit is
that no value becomes `Any` unless written, so an argument not written
`Any{...}` is refused at compile time. It keeps E's run-time fall-through for
stored values, any value can still be written as `Any{...}`, and its box is
per value where B's and C's opt-in is per type. It also makes `Any` the one
trait whose conversion is written, while S48 keeps every other trait implicit.
The ballot records it as an attempt on E's "wrong value" loss.

## Claim 14: every form takes the clip's `69.420`

**Law.** D-LITCARRIER1=D (`syntax-decisions.md:2107-2113`): "`3.4` defaults to
`Decimal`", an expected type settles a literal, `3.4f` is `Float`, and
`Float{3.4}` stays valid. `Examples/Metaprogramming/Generics/TypeTables.jet`
documents the same. So the clip's `69.420` is a `Decimal`.

**Check.**
- A type table sends a bare `2.5` and `f := 2.5` to the `Decimal` arm, and
  `Float{2.5}` to the `Float` arm (`x4`, `c6`). That is the law working; the
  earlier "#6839 c29 wrong arm" reading was withdrawn.
- `2.5f` gives E0134, "`f` isn't a unit in scope" (`x2`): the suffix is
  unbuilt (#4730 c6).
- `Float{69.420}` enters a `[Shown]` list and prints `69.42` (`x6`), and enters
  `...Printable` varargs (`x5`). A returned `Float` enters too (`r4e`), and a
  wrapper works (`r4d`).
- A bare `Decimal` entering a trait list with no impl is an internal compiler
  error, "checked incomplete without a report" (`x3`, `b9`, `r4`-`r4c`), where
  a `Bool` gets E0112 (`x12`). #6727 c9 asks for E0112.
- `impl Decimal.Shown {}` is E0301, "needs a user-defined type" (`x1`, `x1b`),
  though `spec.md:927-929` lets the trait's module implement it for a Core
  math type (#6839 c30). `impl math.Decimal.Shown {}` passes checking and
  crashes `ir.build` with "unlowered … type math.Decimal" (`x1c`, #6727 c9).
- A `Decimal` at a `...Printable` bound gets E0904, "No trait named
  `Printable` is in scope" (`x5b`, `x5c`, `f3`, `f5`; #6839 c30).
- The enum list (`f1`) and a union value (`f2`) print `2.5` as a `Float`
  payload.

**Verdict: fails today for a bare decimal under B and C.** Their programs box
a `Decimal` into a user trait, which needs #6727 c9 and #6839 c30;
`Float{69.420}` works now. Under E an `Any` holding `69.420` holds a
`Decimal`, so `.Decimal(d)` matches it and `.Float(f)` does not. Whether boxing
a `Decimal` into `Any` would share `x1c`'s crash cannot be probed.

## Claim 15: no option serves a plugin

**Attack.** Dispatch is closed-world today: `BuildTraits.jet` stops on an
unknown key at `jet_rt_unreachable`. That is today's implementation, not law.
D-LIB-DYNTRUST1=A ratifies loadable `.jetlib` libraries, pinned to one
compiler identity, with a checked top-level function export table, loaded
through `core.mod` (`Docs/spec/reference/library-reuse-and-linking.md:116-139`).

**Check.** An `Any` from a loaded library (E) and a type arm on a loaded
trait value (C) need type identity shared across libraries; today's render
key carries a module row number (claim 2). A trait value (B) needs the same
cross-library dispatch. No option supplies either today.

**Verdict: out of this ballot's scope, stated to the owner.** Cross-library
type identity and dispatch is #3102 c17.

## Claim 16: today's carrier gives one answer on every tier

**Check.** Every probe in the directory gives the same output on O0, O1 and O2
on p210, the defects included. The web build succeeds but was not executed.

**Verdict: holds for trait values on native tiers.** `Any` and type arms
cannot be probed.

## Claim 17: reflection already serves the inspector

**Strongest prior art.** Go's `fmt` and Jai's `print` show any value through
type info; Rust's `{:?}` needs `Debug` per type.

**Check.** `x10` holds `reflect.of(14)`, `reflect.of("hi")`,
`reflect.of(Point{x: 1, y: 2})` and `reflect.of(69.420)` in one list and
prints each type name, value and field on O0, O1 and O2, with no opt-in. A
`reflect.Value` cannot give its value back: the floor declines `get` and
`copy`, and has no runtime type registry (`core-library.md:2589-2593`). Over a
trait value, `reflect.of(item).type_name()` gives the internal key,
`module:14:source-root:0::FILE::Shown`, not the stored type (`n09`, `n13`,
#6845 c16).

**Verdict: holds for concrete values.** D-ANY-JAI1's ratified story is served
today for values whose type is known where `reflect.of` is called. Every
option keeps `reflect.of`; none of them is needed for printing. What remains
open is taking a stored value back as its own type.

## Claim 18: a type arm over a trait value recovers any opted-in type (option C)

**Strongest prior art.** Go's type switch over any interface value, with each
case type required to implement it and no opt-in by the interface's author.
Rust 1.86 upcasting to `dyn Any` and `downcast_ref::<T>()`, with no added
trait method but an `Any` supertrait the trait's author declares. Swift's
`as?` on an existential, C#'s type pattern and Kotlin's `is`, each over any
reference value.

**Design.** In `if item == .Point(p) -> ...`, `item` is a `Shown` value. The
arm runs only when the stored type is `Point`, and binds it as `p`.
- The arm must name a type that implements `Shown`; any other type is a
  compile error, as Go requires.
- A table needs `else`, because another module may add an implementer. A
  single test needs none.
- The arm's spelling is claim 19, privacy claim 20, the edge cases claim 21,
  and the generic and lending forms claim 22.
- IR: load the key at offset 0, compare it with the arm type's key, view the
  payload at offset 8. No storage or allocation is added.

**Check.** `c1` (the option's program) and `c3` (a table) are E0305 on O0, O1
and O2; `c1` adds a cascade E0107 for the arm's binding `p` (#6839 c53).
Without the arm, `c2` compiles and runs but prints three empty lines (claim 5).
The workarounds inside today's language fail (`x8`, `x9`).

**Verdict: holds as a design; unbuilt.** C removes B's distinct loss: a user
recovers any implementer of any trait, a library's included, without editing
the trait. It keeps the wall and every no-`Any` ruling. Its costs are compiler
work, two amended ratified sentences (D-TYPESWITCH1 `:796`, typed selection
`:1169`) and the last two sentences of one spec paragraph (`spec.md:641-646`,
claim 19), and callers
branching on a library's public types (claim 20).

## Claim 19: an arm can name any implementer, wherever it is declared

**Strongest prior art.** Go's case names a type as type position does,
`case geo.Point:`. Java and C# name a type by its qualified or imported name.
Jet's impl head already writes a path after a dot: `impl Local.types.Named`
implements `types.Named` for `Local` (`spec.md:3619-3622`, D-IMPLDOT1).

**Design.** Over a trait value or a union, the name after the dot resolves as
type position resolves it at that spot.
- A local type, or one imported with `use geo.[Point]` after `use geo`, is
  `.Point(p)`.
- A type from another module is `.geo.Point(p)`.
- An arm through an alias names its target, as D-TYPESWITCH1 says of type
  tables.
- The name resolves by scope, never by the implementers the build holds, so
  adding a module never changes an arm.

Over a union one fallback is added: a name that type position does not know
names the one member with that leaf, as `o7`'s arm does today. So a package's
own `Point` is `.Point(p)` beside `geo2.Point`; today only an `else` reaches
it, and that `else` binds nothing (`fm/f05`: `item.z` is E0302) and takes a
member added later (`fm/a04b`). A name for a non-member type (`v03`), or a leaf
two members share with neither in scope (`fm/a09`), is E0305 naming both, with
the fix `.geo.Point(p)`. With `use geo2.[Point]`, `.Point(p)` names
`geo2.Point` (`fm/f03` is E0305 today). Today E0305's fix offers named cases
and spells a local member `filestem.Point`, which fails (`fm/a04c`, #6839
c52); named cases written by hand run (`fm/f04`), but change the type and
every construction site. So where `Point` names a type in scope, `.Point(p)`
names it throughout the file, over a union or a trait value: what a type name
names is file-wide, since a type cannot be declared in a function body (E0003,
`fm/k02`) and an import beside a package's own `Point` is E0105 (`fm/k03`).
Where it names none, each union's arm names that union's own `Point` member,
as `.Red` names each enum's own case: over `Int | geo.Point` and
`Int | geo2.Point` the same text binds `geo.Point` and `geo2.Point`
(`fm/k01`).

A named case is never ambiguous: its name is the union's own, as an enum
case's is, so a type of that name in scope does not change its arm (`z02`
binds the case today and under C; `fm/a06` with an imported `Point`; `a05`
with a case carrying the same-named struct), and a member may not share its
leaf (E0363).

**Rules weighed.** Two other union rules were rejected:
- *A package's own member has no arm, and its table keeps an `else` exempt
  from E0641* (the previous revision): that `else` binds nothing (`fm/f05`)
  and silently takes any member added later (`fm/a04b`).
- *Type position alone*: it refuses today's working bare arm `o7`, since only
  `geo` is in scope there.

The adopted rule costs a two-step lookup, and `v03`'s working bare arm becomes
E0305. It gives every member an arm, removes the exemption, and changes no
working arm's meaning: `fm/a04` and `fm/f03` are E0305 today.

**Generics.** Over a union with one instance of a generic, `Int | Box<Int>`,
the bare `.Box(b)` names `Box<Int>`, whether `Box` is local or imported;
today both are E0305 "doesn't belong" (`v05`, `fm/k04`). Over a trait value a
generic arm always spells its arguments. Two instances, `Box<Int> | Box<String>`,
share the leaf `Box`. Each arm spells its type arguments as type position does,
`.Box<Int>(b)` (`.geo.Box<Int>(b)` when qualified), the arm syntax #6833 c3
builds; the bare `.Box(b)` is E0305 naming both, and a table names both with
no `else`. Today `.Box(b)` is E0305 saying it "doesn't belong" (`fm/f01`), the
`else` is E0641 (`fm/f01b`), and E0641's fix offers the internal tags
`.M_Box_Int_ | .M_Box_String_`, which compile and bind (`fm/f01c`,
`fm/f01d`). C refuses internal tags as arms with E0305, as it refuses the
identity tag (`fm/a08`; #6839 c55). In an arm, `.Name<` opens type arguments;
today `.Box<Int>(b)` is E0003 (`t01`), and `>>` already closes nested
arguments in type position (`t02`). As an expression, `a < b > (c)` is a
direction-changing chain, E0333 (`t04`), so the arm form takes nothing from
expressions. A bare generic name in type position is E0104, with a cascade
E0112 at the call (`t03`, #6839 c57). A nested pair is coherent: over
`Pair(Point | geo2.Point) | Int` the outer `else` is E0641 with the fix
`.Pair ->` (`fm/f02`), and the inner table names `.Point(p)` and
`.geo2.Point(q)`.

Two same-named members, as in `geo.Point | map.Point`, are what
`spec.md:641-646` rules on. It landed with dev a1c3325a65, the #6839 c36 fix.
Its first sentence keeps the two members apart: "A union member is the type
itself, never its name … holds two members, and each value keeps the member it
was built as." C keeps that sentence. Its second and third say a `.Point` arm
"names neither member (E0305); only an `else` reaches them, and a table over
such a union keeps its `else` without E0641. To match each, make them named
cases." C replaces those two with:

> A bare `.Point` arm names the member `Point` names in type position there;
> a package's own `Point` is `.Point(p)` beside `geo.Point`. If `Point` names
> no type there, it names the one member whose leaf is `Point`. A generic's
> one instance takes its generic's name and leaf; two instances spell their
> arguments, `.Box<Int>(b)`, as do generic arms over a trait value. Any other
> bare arm is E0305, offering `.geo.Point(p)`. So every member has an arm; an
> `else` in a table is E0641.

Over `Int | Box<Int>` the bare `.Box(b)` names `Box<Int>`. Over
`boxes.Box<Int> | kit.Box`, with no `Box` in scope, two members have the leaf
`Box`, so the bare `.Box(b)` is E0305 naming both. Today it binds `kit.Box`,
and `boxes.Box<Int>` takes only the internal tag `.M_Box_Int_` (`fm/m31`,
`fm/m31b`; #6833 c5). Two imported instances, `boxes.Box<Int> |
boxes2.Box<Int>`, are E0305 "doesn't belong" today (`fm/m32`).

So `spec.md:903-905`'s rule, an `else` over a union is E0641, holds with no
exemption. `Coverage.jet:988-991` exempts two module members of one leaf today
(identity tags, `:933-953`), but not two instances of one generic
(`fm/f01b`); C removes it. E0307's fix at `Coverage.jet:521-523`, "no arm can
name geo2.Point, map2.Point, which share a type name" (`fm/a09`), names the
arm each missing member takes instead; today it also spells a package's own
member `filestem.Point` (`fm/f06`, #6839 c52). E0641's fix offers
`.Box<Int>(b)`, never the internal tags (#6839 c55). E0305's same-named branch
(`Patterns.jet:152-169`) fires only where the rule above leaves a bare arm
ambiguous, and offers the qualified arm. The internal identity tag does not
leak: `.I_geo2_2ePoint(p)` is E0305 "doesn't belong" (`fm/a08`).

Edge cases:
- A member imported with `use geo.[Point]`, or an alias naming it, is the same
  type, so it is not ambiguous.
- An alias named `Point` for another type is E0305, as `v03`'s local `Point`
  is.
- A qualified arm, `.geo.Point(p)` or `.outer.inner.Point(p)`, is never
  ambiguous; today both fail (#6839 c35).
- A generic member such as `.Box(b)` needs #6833 c3 (`v05`, `fm/k04`), which
  names that spelling over an `Any`, a union and a trait value, including a
  union's one instance and two instances of one generic. #6833 c5 holds the
  bare-arm rule above if C wins.
- A named case whose name is a member type's leaf, as in
  `Point(Int) | geo.Point`, is refused with E0363, as D-ALT-CASES1 refuses
  `Point(Int) | Point` (`y09b`). So a named case never competes for a bare
  leaf. Today that union is accepted and the bare `.Point(n)` binds the case
  (`y09d`, #6839 c43). On p210, `y09b`'s E0363 is followed by a cascade
  E0305 whose suggested fix, `Point1(Point(Int)) | …`, fails with E0003 when
  applied (`z07`, #6839 c47).
- A named case keeps its bare arm even when a type of its name is in scope:
  `Point(Int) | Int` beside a local non-member `Point` struct binds the case
  (`z02`), as an enum case named like a type keeps its arm (`n02`).
- **Migration.** Two kinds of program change, both at compile time with a
  written fix. `v03` has a local `Point`, and its bare arm over
  `Int | geo.Point` binds `geo.Point` today; under C it becomes E0305. A union
  whose named case shares a member's leaf (`y09d`) becomes E0363. No program
  changes meaning silently, and `z02` does not change. `fm/a04b`'s `else`
  becomes E0641, with the fix `.Point(p) | .geo2.Point(q)`.

**Check.** None of this spelling works everywhere today.

| Probe | Form | Today on p210 (O0, O1, O2 alike) |
|---|---|---|
| `o7` | file module: `.Point(p)` over `Int \| geo.Point`, alias parameter, `geo.Point ->` table arm | runs: `7`, `geo.Point`, `2` |
| `n03a`, `w07` | inline module: `.Point(p)` over `Int \| geo.Point` | E0305 |
| `n03b`, `o8a` | `.geo.Point(p)`, inline and file module | E0107 "Nothing named `` exists here" (#6839 c35) |
| `v11`, `v11b` | inline: `.outer.inner.Point(p)`; the bare leaf of a nested member | E0003 at `->` (#6839 c35); E0305 |
| `v03` | file module, a local `Point` in scope: bare `.Point(p)` over `Int \| geo.Point` | binds `geo.Point` (migrates to E0305) |
| `y09`, `y09d`, `y09b` | `Point(Int) \| geo.Point`; the same with a local `Point` | accepted, and bare `.Point(n)` binds the case; a table is E0307 "missing: geo.Point" (#6839 c43); the local form is E0363, then a cascade E0305 (#6839 c47) |
| `z02` | a named case `Point(Int)` beside a local non-member `Point` | accepted; `.Point(n)` binds the case (unchanged under C) |
| `z07` | E0305's suggested fix for `y09b`, applied verbatim | E0003 (#6839 c47) |
| `n04` | inline: `.Point(p)` over `geo.Point \| map.Point` | E0305 |
| `o8b`, `o8f` | file modules: `.Point(p)` over `geo2.Point \| map2.Point` | p207: accepted as one member, printing another type's bytes; p209 and p210: E0305 "names more than one member" (#6839 c36, met) |
| `fm/a04`, `fm/a04c` | a package's own `Point` beside `geo2.Point`: bare `.Point(p)`; E0305's suggested fix applied | E0305 offering `Point1(a04_local_vs_module_same_leaf.Point) \| …`, then a cascade E0107 on `p` (#6839 c53); applied, E0119 then a self-contradicting E0108 (#6839 c52) |
| `fm/a04b` | the same union in a table with `.Int(n)` and `else` | runs: `else` takes both `Point`s, `.Int(n)` takes 4 |
| `fm/f05` | the same union: `.geo2.Point(q)` and an `else` reading the local `Point`'s field | E0107 for the dotted arm (#6839 c35); `item.z` in the `else` is E0302 |
| `fm/f06` | the same union with `Int`, a table with only `.Int(n)` | E0307 "missing: f06_local_member_table_no_else.Point, geo2.Point", spelling the local member `filestem.Point` (#6839 c52) |
| `fm/f04` | named cases `Mine(Point) \| Theirs(geo2.Point)` | runs: `local Point { z: 5 }`, `geo2 Point { x: 1, y: 2 }` |
| `fm/f03` | `.Point(p)` over `geo2.Point \| map2.Point` with `use geo2.[Point]` | E0305 "names more than one member" (C: binds `geo2.Point`) |
| `fm/a09` | a table over `geo2.Point \| map2.Point` with no `else` | E0307 "no arm can name geo2.Point, map2.Point" (`Coverage.jet:521-523`) |
| `fm/f01`, `fm/f01b` | `Box<Int> \| Box<String>`: bare `.Box(b)`; a table with `else` | E0305 "doesn't belong"; E0641 with the fix `.M_Box_Int_ \| .M_Box_String_` (#6839 c55) |
| `fm/f01c`, `fm/f01d` | E0641's fix applied; `.M_Box_Int_(b)` reading `b.value + 1` | both run: the internal tags bind, `int box 2` (#6839 c55) |
| `fm/f02` | `else` over `Pair(Point \| geo2.Point) \| Int` | E0641 with the fix `.Pair ->` |
| `v05`, `fm/k04` | `Int \| Box<Int>`: bare `.Box(b)`, with `Box` local; with `Box` imported | E0305 "doesn't belong", then a cascade E0107 on `b` (C: binds `Box<Int>`) |
| `fm/k01` | one `.Point(p)` text over `Int \| geo.Point` and over `Int \| geo2.Point`, no `Point` in scope | runs: binds `geo.Point`, then `geo2.Point` |
| `fm/k02`, `fm/k03` | a struct declared in a function body; `use geo2.[Point]` beside a package's own `Point` | E0003; E0105 |
| `t01`, `t02`, `t03`, `t04` | `.Box<Int>(b)` as an arm; `>>` closing nested type arguments; a bare generic `Box` in type position; `a < b > (c)` | E0003 (#6833 c3); runs; E0104, then a cascade E0112 (#6839 c57); E0333 |
| `a05`, `fm/a06`, `fm/a08` | `Point(Point) \| Int`; `Point(Int) \| Int` after `use geo.[Point]`; the identity tag as an arm | bind the case; bind the case; E0305 "doesn't belong", then a cascade E0107 (#6839 c53) |
| `n14`, `n14b` | inline: `use geo.[Point]` | E0611 (#4723 c12) |
| `o8c`, `o8h` | file module: `use geo.[Point]` alone, then after `use geo` | E0610; after `use geo`, runs: `3` |
| `o1`, `o8e` | alias `GeoPoint: geo.Point` as a union arm | E0305 (#6839 c38) |
| `o8i` | file module: alias as a type-table arm | runs |
| `o5`, `o5c` | inline module: alias as a table arm; alias as a parameter | takes `else`; E0112 "wants `geo.Point`, but this is `geo.Point`" (#6839 c37) |
| `o5b` | inline module: `geo.Point ->` as a table arm | E0608 (#6839 c37) |

**Verdict: holds as a design; each row is a filed build criterion.** The
dotted and nested arms are #6839 c35, the alias forms c37 and c38, the
named-case collision c43 and its cascade c47, the package-member fix texts
c52, the refused arm's cascade E0107 c53, two instances of one generic and the
leaked internal tags c55, and the inline import #4723 c12. `o8f` read one
same-named member's bytes as the other on p207 (#6839 c36, now met). Dev
a1c3325a65 fixed it, and `spec.md:641-646` now refuses the bare arm with
E0305; C keeps its first sentence and replaces its second and third with one
rule, type position first and then the member leaf, so every member has an
arm and the `else` exemption goes (`Coverage.jet:521-523`, `:988-991`).

## Claim 20: C keeps a library's encapsulation

**Strongest prior art.** Go lets any caller type-switch on a library's
exported types. Go's errors blog names the cost: "wrapping an error makes that
error part of your API" (go.dev/blog/go1.13-errors). Its `errors.As` still
"behaves like a type assertion", the mechanism C adds. Rust asks the trait's
author to opt in with an `Any` supertrait. B, Jet today, lets a library decide
which types callers take back, through the methods its trait declares.

**Attack.** Jet declarations are public by default, and "`priv` is the only
hiding form" (`spec.md:3509-3512`, `3568-3569`). Under C a caller can arm any
public type a library stores behind its trait values, and the library then
cannot change that type without breaking those arms.

**Remedies weighed.**
- **A signed marker** (`#!X`, `spec.md:953-954`) opts a type out of a
  derivable trait. Using one to seal type arms would add a second hiding form
  beside `priv`, against `spec.md:3568`. Rejected.
- **An author opt-in**, as Rust's `Any` supertrait: brings back B's loss for
  every library trait whose author did not opt in. Rejected.
- **`priv`.** A `priv` type cannot be named outside its package, or outside
  its module when declared inside one (S18, `syntax-decisions.md:2531-2538`).
  So an arm cannot reach it, and it takes `else`. Kept.

**Check.** From a file module a `priv` struct is refused today, though with
E0119 where E0605 belongs (`o8d`, #4723 c13). Inside an inline module a
`priv` struct is usable from outside with no diagnostic, while a `priv`
function beside it is E0605 (`n12`, `n12b`, #4723 c11). `print` and
`reflect.of` already show a library's concrete type and name (`n05`: over a
`priv` type behind a trait value, `type_name()` gives `Secret` and `path()`
`n05_private_type_trait_value.lib.Secret`).

**Verdict: holds within limits.** C adds recovery, not disclosure, and `priv`
stays Jet's one hiding form. The cost is real and stated on the surface:
callers can come to depend on a library's public types, as Go's did. B's
matching gain, a library deciding what callers take back, is stated beside
it. C needs #4723 c11 and c13.

## Claim 21: C settles every edge of the arm

**Design and check**, p210, O0, O1 and O2 alike:
- **A stored enum whose case shares a type's name.** Over a `Shown` value,
  `.Point(p)` names the type `Point`; a stored `Item.Point(…)` is an `Item` and
  takes an `.Item(i)` arm or `else`. A union already behaves so: over
  `Item | Point`, `Item.Point(…)` takes `.Item` (`o6`). No lint warns, since
  implementers change with the build and such a warning would come and go.
  Union clashes stay E0363, because a union is closed. Today over an enum
  subject `.Point(p)` binds the case (`n02`), as D-ENUMDOT1 says.
- **A union value passed as a trait value.** It boxes the member it holds,
  with that member's own key, so `.Int(n)` matches a boxed `Int`. Every member
  must implement the trait; otherwise E0905. Boxing the union's own identity
  was rejected, since every member arm would then miss. Today it is an ICE
  (`n01`, #6839 c34).
- **An arm naming a trait.** Refused with E0305 and a fix pointing to the
  trait's methods. A trait-to-trait test needs #6727 c7. Type tables refuse a
  trait arm too (`n11`, E0119), so both forms keep one rule. Go, Swift, Java,
  C# and Kotlin allow such a test; C can add it after #6727 c7.
- **A union with a trait member.** `Shown | Int` is refused at the type with
  E0363, which already says an anonymous union holds concrete closed types,
  not trait objects (`Compiler/Prelude/Diagnostics.jet:5866-5880`; sema
  `FailureLaw.jet:1391-1400`, `1538-1561`). A trait written by its name slips
  past that check today: the type is accepted, `.Shown(s)` is E0305 and a
  `Point` argument E0112 (`v04`, #6839 c40). The fix is a named enum, or a
  `Shown` value with `Int` implementing `Shown`. Accepting `.Shown(s)` instead
  would put an open set inside a closed union, and `.Point(p)` over it would
  gain a second meaning, a reach into the box (`w06`, E0305 today). So an arm
  naming a trait has no union exception. A type parameter is refused the same
  way (`w05`, E0363). E0363's check looks inside member types
  (`FailureLaw.jet:1540-1561`), including lists, maps, `Shared`, optionals,
  results, fixed lists, tuples, named cases, nested unions and generic
  arguments. So c40's fix refuses `[Shown] | Int` and `S(Shown) | Int` too, as
  it already refuses a trait object or a function type there: `[fn(Int) ->
  Int] | Int` is E0363 today (`z03`), followed by a cascade E0641 (#6839 c48)
  whose fix leaks `.M__fn_Int_____Int_` (#6839 c27). One rule covers the trait
  at any depth. `[Shown] | Int` is accepted today (`y10`, whose E0641 fix text
  also leaks the mangled trait name, #6839 c27). The fix is a named struct
  holding the list, a closed member.
- **Identity.** D-TYPESWITCH1's rules apply: an alias is its target; a
  distinct type, `F32` or `Int?` is not `Int`. A stored `UserID` takes `else`
  (`o5`).
- **The bare arm.** `.Point ->` runs the same test as `.Point(_)`, as over a
  union (`o3` runs; `n10` shows it accepted beside E0641's `else` refusal).
- **An optional subject.** A `Shown?` subject takes no type arm, as
  `(Int | Point)?` takes none today (`o4`, E0305); unwrap with `Val(item)`
  first (`o4b` runs).

**Verdict: holds as a design.** Each edge has one rule, and each refusal has a
registered diagnostic to snapshot.

## Claim 22: an expert can write one recovery function per library

**Strongest prior art.** Rust's `downcast_ref::<T>()`, which returns `&T`, a
pointer with no allocation; C++'s `std::any_cast<T>(&a)`, which returns a
pointer or null, while `any_cast<T>(a)` copies or throws; Swift's `as? T`.

**Design.**

```jet
recover: fn<T: Shown>(item: Shown) -> T? from item {
    if item == .T(p) -> return Val(p)
    None
}
```

The arm `.T(p)` names the type parameter, so each instance compares against its
own type's key. `recover` returns a read view, carried today in a new optional
cell holding a byte copy (`spec.md:1906-1909`). The checker tracks the result
only while a plain local holds it: while that local lives the owner cannot
move or change, and the result cannot be written. Once a value-producing `if`
or table picks the view, anything else holds it, or its owner is Shared, the
check loses it (see the check below; #6839 c50, c51, c54, c56). A plain arm
binding escapes the same way (`e10`, `h11`), so the hole is the loan
checker's, not C's. A copy the caller owns is written
`Val(p.copy())`, with no `from` clause. A `Val(p)` under a `from` clause naming
its owner stays a view; #6839 c41 refuses only the unmarked copy with no
`from`. `take` is reserved (`n07b`, E0003); `recover` is an ordinary name
(`o2`).

Other subjects follow the enum rules:
- A temporary subject's view cannot leave its statement (E2305: `y06b`,
  `y06c`). A named value converted at the call ties the loan to that name
  (`y06`).
- An owned `&&` subject moves the payload out with no `from` (`y07`). A `from`
  clause naming a `&&` parameter is accepted today, and its call then gets a
  misleading E0209 (`y07b`, #6839 c45).
- Over a `&` subject the arm binds read-only, as an enum's arm does (`y08`), so
  C has no `downcast_mut` analogue. E0205 there blames `item`, not the binding
  (#6839 c44).

**Check.**
- Today a type parameter in a type-table arm is E0119 (`x8`), and a trait
  method cannot take its own type parameter (`x9`, #6839 c31).
- The checker already tracks a case payload's loan. `-> Point? from item`
  over one enum value (`w01`) and one union value (`w02`) runs; moving the
  owner while the result lives is E0212 (`w01b`, `w02b`, `y11`), and a write
  through the result is refused (`y11b`). `v07`, `v07b` and `v07c` show the
  same for a list, and the loan survives generic instantiation (`y04`, `y05`).
- What runs is not yet a loan. At O0, `pick` under `from item` and `keep`
  without it emit identical IR: `jet_rt_alloc_value` for a new 24-byte
  `?[Int]` cell, `copy_bytes` of the payload, and `jet_rt_block_retain` of its
  shared heap block (`y12`; O1 and O2 IR cannot be emitted). `jet audit`
  counts only `keep`'s copy. A struct payload is byte-copied the same way
  (`y01` IR on p209; on p210 `y01` is E0212).
- Where the check holds, the copy's value cannot be told from the payload's:
  the owner cannot change, the result cannot be written, and a `Close` payload
  lent out under `from` closes exactly once, also under the checked allocator
  (`z06`). Through an escape it can: `e11` rewrites the owner while a struct
  holds the view, and the view reads `41` while the owner is
  `At(Point { x: 99, y: 0 })`; `a03b` shows the same through Shared. Its
  address always can. `mem.address_of` needs no audit gate
  (`spec.md:3164-3166`). An arm binding over one owner has one address both
  times, and arms over two owners have two, so the binding is the owner's
  payload (`z01`, `z01b`). Two `from` results over one owner sit at two
  addresses, neither the payload's, on O0, O1 and O2 (`z01b`).
- Two legal programs have no payload that outlives the result. A `Point`
  passed to `item: Int | Point` is copied into a new 24-byte cell, which is
  released right after the call (op 16, `jet_rt_block_release`), before the
  result is read; the loan is tied to the named `pt` (`z04` IR, `y06`). A
  body under `from item` may return a fresh `Val(Point{x: n, y: n})`, or
  `spare.copy()` under `-> Point from item`; both run (`z05`), and that value
  lives in no owner.
- The written copy runs, and the owner then moves freely (`w04`).
- Today an unmarked `Val(p)` of a read value copies (`n06`, `n15b`, `v08`),
  while a bare return is E0120 and a take is E0201 (`v08`, `v08b`). #6839 c41
  refuses that hidden copy, since "Jet never hides a clone"
  (`spec.md:1892-1895`).
- B's trait method can lend the same way, `-> Point? from self` (`w03`).
  On p209 the loan was not tracked through dispatch: the list moved or
  changed while the result lived (`w03b`, `w03c`), and `y01` showed the
  result was copied, reading `41` after the stored `Point` became `141`
  (#6839 c42). Dev c1fca7426c, in batch p210, tracks it: `w03b`, `w03c`,
  `y01` and `y02` are E0212 on O0, O1 and O2, as a struct method's identical
  loan is (`w03d`, `y01d`), and `spec.md:1909-1912` now says so. The result
  stays a copy until #6839 c46. C needs no c42: a trait value's loan with no
  dispatch is tracked (`y03`, `y05`).
- The check covers a view only while a plain local holds it; every other
  holder escapes it today.
  **A lambda**, local, stored or returned: `got: pick(item)`, a lambda reads
  `got`, `consume(item)` moves the owner into a `&&` parameter, then the lambda
  runs and prints `closure reads 41` (`a01`, `e03`, `e02`); without the lambda
  the move is E0212 (`a01b`). `spec.md:2058-2059` says "Captures and field
  projections preserve the fact", and `spec.md:2066-2068` carries a view fact
  through "function values, lambdas" (#6839 c50). **A Shared owner:** `got:
  pick(cell.item)` with `cell` a `Shared<Holder>`, then `cell.item =
  Item.Num(7)` through the same handle (`a03b`) or a second one (`a03`). Both
  compile; the view prints `41 42` and the cell `Num(7)`. The IR reads the
  field under the lock into a retained block (op 59), calls `pick` on it (op
  69) and drops it right after the call (op 70); the write is op 92. A field
  read takes its lock for one statement (`spec.md:2128-2130`), so a second
  handle's write is never seen statically (#6839 c51). **A carrier:** the
  view in a struct field (`e01`; control `e01b` is E0212), an enum payload
  unwrapped twice (`e07`; control `e07b` is E0212), views collected into a
  list in a loop (`e05`), a `defer` (`e06`) or a method value `p.sum`
  (`e08`, printing `83`) all outlive a move of the owner, and a Shared cell
  carries the view into a task (`e09`). `spec.md:1963-1965` says carriers
  "carry the same hidden relation" (#6839 c54). A plain arm binding in a
  struct escapes too (`e10`). Resizing a list while a view of its element
  lives is still E0212 (`e04`). All of these ran alike on O0, O1 and O2.
- **A branch join.** A view picked by a value-producing `if` or table escapes
  the check: the owner moves into a `&&` call and the view still reads `41`
  (`h06`, `h08`, `h10`), even with `pick` called inside the branch (`h09`).
  An arm binding picked the same way escapes too (`h11`), and with the owner
  rewritten the view reads the old `41` (`h12`). A plain local alias of the
  view, or a view relayed through a second `from` call, stays E0212 (`h01`,
  `h03`). So does every move or write of the owner while a plain local holds
  the view: reassigned, a field rewritten, the struct moved, moved in a
  statement `if`, in a lambda or by a binding, or written through a `&`
  parameter (`m01`-`m03`, `m05`-`m08`); reassigning a view variable to a
  view of another owner is E0205, "Cannot edit through `other`" (`m04`), and
  passing view and owner to one call is E0204 (`m09`).
  `spec.md:2058` keeps invalidation conservative at control-flow
  joins, and `spec.md:2069` unions a branch's sources (#6839 c56). **Tuples
  and maps:** a view in a tuple field, `(view: got, tag: 1)` (`h02`), or a
  map value, `m["k"] = got` (`h05`), outlives a move of the owner (#6839
  c54). All ran alike on O0, O1 and O2.
- A task refuses a captured view: E1102 (`a02`, `spec.md:2059-2060`). Inside
  a Shared cell it reaches the task today (`e09`, #6839 c54).

**What makes it a checked loan.** A lowering change and a complete loan
checker, all #6839 c46, which lands after c50, c51, c54 and c56. The change: a
`T? from x` result carries a pointer into the owner's payload, with no new
cell, copy or retain; `z01b` is its acceptance test. With it:
- An argument converted at the call, or read through a Shared handle, lives
  in a cell released right after the call (`z04` op 16; `a03b` op 70). That
  cell must live as long as the result, or the call is E2305, as for a
  temporary. A trait-value subject boxes the same way at the call (`q1`: a
  40-byte box).
- A path that returns a value no owner holds (`z05`) keeps its own cell and a
  drop the caller runs, or is refused. `spec.md:1906-1909` says a view result
  "closes nothing, and its owner closes it", so a pointer alone would leak it.
- Every holder keeps the owner borrowed: a lambda, local, stored or returned,
  keeps its loan or is refused, and never clones the view (#6839 c50); a
  Shared read keeps the owner borrowed (c51); a struct field, enum payload,
  list element, tuple field, map value, `defer` or method value keeps the
  loan as a local name does (c54), and so does a value-producing `if` or
  table (c56). Moving or rewriting the owner while a holder lives is E0212.
- No task receives the view, also inside a Shared cell (E1102; c54).

Without these, a pointer view would read a payload the callee freed (`a01`,
`e01`-`e08`, `h02`, `h05`, `h06`, `h08`-`h12`), or a block freed when the write dropped its last reference
(`a03`, `a03b`), or one read on another thread (`e09`): the type confusion
that c36 was. Today all are safe only because the view is a copy (`z01`).

This serves every `from` result and every arm binding, enum, union and list
payloads included, so it is not C's alone, and C's gain does not wait for it.

**Verdict: holds as a design for ownership while a plain local holds the view;
at run time today's analogue copies the payload's bytes into the optional
(`y12`).** The copy is gone from direct use, not from the machine. Its address
shows it (`z01`, `z01b`), and through an escape so does its value (`e11`,
`h12`, `a03b`). It costs one cell and one byte copy per call, heap blocks
shared, where `downcast_ref` costs nothing. C's gain is the view tracked while
a plain local holds it, not the absence of a copy; it is checked as Rust's
`downcast_ref` borrow is only after #6839 c46, with its prerequisites c50,
c51, c54 and c56. This does not change the ranking, which rests on `b8`: a
library trait's user cannot add B's method (`b8`, `fm/m41`-`fm/m46`). B's program copies too (`b13`),
and its lending method's loan, untracked through dispatch on p209 (`w03b`), is
E0212 on p210 and a copy until c46. The earlier claim that a
type arm could only copy, and the `-> View<T> from item` form (a window of
many values, not one payload), stay withdrawn.

## Verdicts

| # | Claim | Verdict |
|---|---|---|
| 1 | `Any` adds no storage | holds within limits; switch set widened; #6727 c6 |
| 2 | key is a TypeId | fails as first worded; "a key reflection maps to the TypeId", #6845 c14, c16 |
| 3 | inner key kept | holds in IR; user-level ICEs, #6727 c7, #6839 c34 |
| 4 | recovery | holds within limits for E and C; the union member test extended; #6833 c3 (arms over an `Any`, a union and a trait value) and c5 (C's bare-arm rule) |
| 5 | printing | holds (#6845 c13 met); an empty trait prints blank (#6845 c15) |
| 6 | cost | today every option allocates; a union can be inline by design; C's arm adds none |
| 7 | not TypeScript `any` | holds within limits for E; B and C refuse types that never opted in |
| 8 | type info on demand | holds; metadata on #6845 |
| 9 | the clip's INI job | not an `Any` job; #6883 |
| 10 | loans | holds within limits for C, while a plain local holds the view: a view of a temporary is E2305 (`y06b`, `y06c`), a view of a named owner is E0212 (`w01b`, `y06`, `h01`, `h03`, `m01`-`m03`, `m05`-`m08`); a view picked by a value-producing `if` or table, or held by anything else, escapes the check today (`a01`, `a03b`, `e01`-`e10`, `h02`, `h05`, `h06`, `h08`-`h11`; #6839 c50, c51, c54, c56); unverified for E's `Any` (#6833 c3) |
| 11 | B serves the open set | holds within limits; recovery needs a trait method per type, and a library trait's user has no route to add one (`b8`, `fm/m41`-`fm/m47`; #6727 c7 refuses trait-to-trait conversion under B and C; #6839 c58) |
| 12 | B's compile-time check | entry by type only; the unplanned type is shared by every option |
| 13 | A as its own option | fails; kept as E's variant |
| 14 | every form takes `69.420` | a `Decimal`; fails under B and C today; #6727 c9, #6839 c30, #4730 c6 |
| 15 | no option serves a plugin | out of scope, stated; #3102 c17 |
| 16 | one answer on every tier | holds for trait values; web not executed |
| 17 | reflection serves the inspector | holds for concrete values; trait values report the key (#6845 c16) |
| 18 | type arms over a trait value | holds as a design; unbuilt (E0305 today) |
| 19 | arm spelling | holds as a design; one bare-arm rule over unions and trait values (type position first, then the member leaf; E0305 for a non-member type or a leaf two out-of-scope members share, `boxes.Box<Int> \| kit.Box` included (`fm/m31b`); every member has an arm, so no `else` is exempt from E0641; a generic's one instance takes its generic's leaf (`v05`, `fm/k04`), and two instances spell their arguments, `.Box<Int>(b)`; a named case never ambiguous, and a member may not share its leaf; `v03`, `fm/m31b` and `y09d` migrate, `z02` does not, and `fm/a04b`'s `else` becomes E0641); C keeps `spec.md:641-646`'s first sentence and replaces the other two, with `Coverage.jet:521-523`, `:988-991`; filed criteria #6833 c3, c5, #6839 c35, c37, c38, c43, c47, c52, c53, c55, c57, #4723 c12 |
| 20 | encapsulation | holds within limits; `priv` the one hiding form; needs #4723 c11, c13 |
| 21 | edge cases | holds as a design; one rule each; a trait at any depth of a union member refused (E0363, #6839 c40; cascade #6839 c48) |
| 22 | expert recovery | holds as a design for ownership while a plain local holds the view: a read view under `from item`, carried today in a new optional cell holding a byte copy at another address (`y12`, `z01`); every other holder and every branch join escapes the check, and the copy then shows its old value (`a01`, `a03b`, `e01`-`e11`, `h02`, `h05`, `h06`, `h08`-`h12`; #6839 c50, c51, c54, c56); a checked loan is #6839 c46 after them; `Val(p.copy())` copies (#6839 c41) |

## Recommendation: C

C answers the clip's real need, taking the Point back, for every opted-in type
with one checked arm. It keeps the no-top-type wall, D-ANY-JAI1 and
D-DYNAMIC-TYPE1, and it is how Go takes a type back from an interface value;
Rust, Swift, C# and Kotlin do the same from their trait objects and
references. B runs today and keeps every rule, but taking a type back needs a
trait method per type, which a library trait's user cannot add (`b8`, `fm/m41`-`fm/m47`).
`recover<T>` returns the payload as a read view under `from item`, tracked
today only while a plain local holds it. It is checked as Rust's
`downcast_ref` borrow is only after #6839 c46, with its prerequisites c50,
c51, c54 and c56. Until then it is a byte copy in a new optional cell (`y12`),
seen by address (`z01`) and, through a carrier or a branch, by value (`e11`,
`h12`; claim 22). The ranking rests on `b8`, not on the copy: B's program
copies too (`b13`).

**What C gives up.**
1. Compiler work before it runs: today the arm is E0305 (`c1`), and the
   spelling, privacy, copy and printing fixes (claims 19-22, 5) come with it,
   each on a filed criterion. Mitigation: B's program runs today (`b13`) and
   stays valid after C ships.
2. Callers can branch on a library's public types, so changing one can break
   them. Mitigation: tables require `else`, and a library hides a type with
   `priv`, Jet's one hiding form.

Also: two amended ratified sentences, the last two sentences of one spec
paragraph (`spec.md:641-646`, removing the E0641 exemption at
`Coverage.jet:988-991` and changing E0307's fix at `:521-523`), and no arm
for a second trait until #6727 c7.

**What `Any` would gain over C.** No stored type needs an impl line, so the
clip's `Table(string, Any)` works with no declarations. A list, an optional or
a closure goes in without a wrapper struct, though taking one back needs an
arm spelling (#6833 c3). Comment #4
calls this the case where sum types are clumsy. B and C pay one impl line per
stored type.

**What B would give up, if chosen.** Recovery of any type the trait does not
declare a method for; for a library trait, recovery the author did not plan
(`b8`). B keeps a library's control over which types callers take back. Its
shared program copies (`self.copy()`, `b13`); a lending method's loan through
dispatch is E0212 from dev c1fca7426c (`w03b`, `y01` on p210) and a copy
until #6839 c46.

**What E would give up, if chosen.** D-ANY-JAI1's no-top-type part,
D-DYNAMIC-TYPE1, and the nine more places that rely on them; any compile-time
rejection of a wrong value, which A's written box only narrows. It needs
#6727 c6 and c7 first.

## Gaps and where they live

Prerequisites by option:

| Prerequisite | E | B open set | B closed set | C |
|---|---|---|---|---|
| #6845 c13: print a trait value | met | met | not needed | met, but see the next row |
| #6845 c15: an empty trait's value prints (`c2b`) | not needed | only for a method-less trait | not needed | needed |
| #6845 c18: a key collision is a diagnostic, not a panic | needed | needed | not needed | needed |
| #6839 c39: a user trait on a file-module type (`v01`, `v01b`, `v01c`) | not known | needed across files | not needed | needed across files |
| Type arm over a trait value (`c1`, `c3`) | not needed | not needed | not needed | needed |
| Arm spelling: #6833 c3, c5; #6839 c35, c37, c38, c43, c47, c52, c53, c55; #4723 c12 (claim 19; `Coverage.jet:521-523`, `:988-991` and the type-position-then-leaf rule) | needed for its arms | not needed | not needed | needed |
| #6839 c36: same-named union members stay apart (`o8f`); met (dev a1c3325a65) | met | met | met | met |
| #6839 c40, c48: a union with a trait member refused, with one report (`v04`, `z03`) | not needed | not needed | not needed | needed |
| #6839 c41: no hidden copy in `Val` with no `from` (`v08`, `n15b`) | not needed | not needed | not needed | needed for the copy form |
| #6839 c42: a lent result through dispatch (`w03b`, `y01`); its check landed in p210 (dev c1fca7426c, E0212), and the copy is c46's | not needed | check met on p210; the copy waits on c46 | not needed | not needed (`y03`, `y05`: a trait value's loan without dispatch is tracked) |
| #6839 c50, c51, c54, c56: a view keeps its loan through lambdas, Shared reads, every carrier and every branch join (`a01`, `a03`, `a03b`, `e01`-`e11`, `h02`, `h05`, `h06`, `h08`-`h12`) | not needed | for a lending method | not needed | needed for the checked view |
| #6839 c46: a `from` result is a pointer, not a copy (`y12`, `z01b`, `z04`, `z05`); lands after c50, c51, c54 and c56 | not needed | not needed | not needed | not needed: C's gain is the view tracked while a plain local holds it; c46 removes the copy for every `from` result |
| #6839 c49: a union parameter of a generic function (`a07c`) | not needed | not needed | if a generic function takes a union | not needed |
| #4723 c11, c13: a `priv` struct is private (`n12b`, `o8d`) | not needed | not needed | not needed | needed |
| #6839 c34: a union value into a trait value (`n01`) | needed | if a union is passed | not needed | needed |
| #6727 c6: generic trait values (`q6`) | needed | only if a generic type opts in | not needed | only if a generic type opts in |
| #6727 c7: trait-to-trait conversion (`q2`) | needed | not needed | not needed | only to recover after a conversion |
| #6839 c26: lists of a union (`b2`, `r6`) | not needed | not needed | union form only; the enum list runs (`r8`) | not needed |
| #6727 c9 and #6839 c30: a `Decimal` in a trait value | unknown: `Any` cannot be probed | needed for a bare decimal | not needed (`f1`) | needed for a bare decimal |

Filed defects:

- **#6727 c7, c8:** trait-to-trait conversion and a closure in a trait list
  are ICEs (review `q2`, `q5`).
- **#6727 c9, #6839 c30, #4730 c6:** the `Decimal` defects and the `2.5f`
  suffix (claim 14).
- **#6839 c27:** E0641 fix text leaks `.M__Int_` (review `g7`).
- **#6839 c28:** `[Nope]{14}` reports E0112 instead of E0119 (`e0`).
- **#6839 c31:** a trait method with its own type parameter (`x9`).
- **#6839 c32:** a Core trait as a trait value, `[Printable]{14}` (`c4`,
  `c4b`); whether it should be is not settled here.
- **#6839 c33:** E0119 on the name `Any` points to an enum, a trait or
  `reflect.of` (`x11`).
- **#6839 c34, c35:** a union value into a trait value is an ICE (`n01`); the
  dotted arm's empty-name E0107 (`n03b`) and the nested arm's E0003 (`v11`).
- **#6839 c36 (met):** with file modules, `geo2.Point | map2.Point` was
  treated as one member on p207: `print(map2.Point{name: "harbor"})` through
  the union printed `Point { x: 35184415600640, y: 2 }`, and the arm bound the
  wrong type (`o8f`, `o8b`; O0, O1 and O2). Fixed on dev by a1c3325a65 (E0305
  on p209).
- **#6839 c37:** an alias of an inline-module struct is not its target: a call
  says "wants `geo.Point`, but this is `geo.Point`" (`o5c`), and a table arm
  through it takes `else` (`o5`). A table arm `geo.Point ->` is E0608
  (`o5b`). The same forms work with a file module (`o7`, `o8i`).
- **#6839 c38:** an alias as a union arm is E0305 with file or inline modules
  (`o1`, `o8e`).
- **#6839 c39:** a trait's owner implementing it for a file-module type passes
  checking, then boxing is an ICE and a direct call E0102 (`v01`, `v01b`,
  `v01c`; controls `v01d`, `v01e`).
- **#6839 c40:** a union with a trait member is accepted as a type, though
  E0363 refuses trait objects (`v04`); its fix also refuses `[Shown] | Int`
  (`y10`).
- **#6839 c41:** `Val(s)` of a read value with no `from` clause copies with no
  mark (`v08`, `v08b`, `n15b`).
- **#6839 c42:** a trait method's `from self` result through dispatch was
  copied with no loan check on p209: moving or changing the list while it
  lived compiled (`w03b`, `w03c`, `y01`); the struct form is E0212 (`w03d`,
  `y01d`). Dev c1fca7426c landed the check (E0212 on p210); the copy remains
  c46's, and the card's text still describes p208.
- **#6839 c43:** `Point(Int) | geo.Point` is accepted, and the bare
  `.Point(n)` binds the case (`y09`, `y09d`; control `y09b`).
- **#6839 c44:** a write to an arm binding over a `&Item` parameter gets an
  E0205 that blames `item`, not the binding (`y08`).
- **#6839 c45:** a `from` clause naming a `&&` parameter is accepted at the
  declaration, and the call gets a misleading E0209 (`y07b`).
- **#6839 c46:** a `T? from x` result is a byte copy in a new cell (`y12`); a
  pointer into the owner's payload makes it a checked loan, given rules for a
  converted or Shared-read argument's cell (`z04`, `a03b`) and a path
  returning an owned value (`z05`). `z01b` is its acceptance test. It lands
  after c50, c51, c54 and c56.
- **#6839 c47:** the same-named-member E0305 branch treats a named case as a
  type: `y09b` gets a cascade E0305 after E0363, and its suggested fix fails
  with E0003 when applied (`z07`).
- **#6839 c48:** a union refused by E0363 gets a follow-up E0641 (`z03`).
- **#6839 c49:** a generic function with a union parameter is an ICE at the
  call, "ir.build unlowered … argument type String | Int" (`a07c`; named cases
  `a07`, `a07b`). A single union value runs outside generics (`b3`, `f2`).
- **#6839 c50:** a `from` view captured by a lambda, local, stored or
  returned, loses its loan: the owner moves while the lambda lives, with no
  E0212 (`a01`, `e03`, `e02`; control `a01b`). A stored lambda clones its
  captures; the loan must be kept or the capture refused.
- **#6839 c51:** a `from` view of a Shared field survives a write to that
  field through the same handle or a second one (`a03b`, `a03`; IR op 70).
- **#6839 c52:** E0305's fix spells a package's own member `filestem.Point`;
  applied, it is E0119 and a self-contradicting E0108 (`fm/a04`, `fm/a04c`).
  E0307's text spells it the same way (`fm/f06`).
- **#6839 c53:** an arm refused with E0305 leaves its binding undeclared, so
  each use adds a cascade E0107 (`c1`, `fm/a04`, `fm/a08`, `o8f`).
- **#6839 c54:** a view, a `from` result or an arm binding, loses its loan in
  a carrier: a struct field (`e01`, `e10`, `e11`; control `e01b`), an enum
  payload (`e07`; control `e07b`), a list built in a loop (`e05`), a `defer`
  (`e06`), a method value (`e08`), a tuple field (`h02`), a map value
  (`h05`), or a Shared cell handed to a task (`e09`, past E1102). c46 lands
  after it.
- **#6839 c55:** over `Box<Int> | Box<String>`, the bare `.Box(b)` is E0305
  "doesn't belong", the `else` is E0641, and E0641's fix offers the internal
  tags `.M_Box_Int_ | .M_Box_String_`, which compile and bind (`fm/f01`-`f01d`).
- **#6839 c56:** a view picked by a value-producing `if` or table loses its
  loan, even with the call inside a branch, and an arm binding picked so does
  too (`h06`, `h08`-`h12`; controls `h01`, `h03`). c46 lands after it.
- **#6839 c57:** a bare generic name in type position, E0104, adds a cascade
  E0112 at the call, whose type text puts the type arguments outside the
  backticks (`t03`).
- **#4723 c11, c12, c13:** a `priv` struct usable from outside an inline module
  (`n12b`); `use geo.[Point]` refused for an inline-module type (`n14`); a
  `priv` file-module struct refused with E0119 where E0605 belongs (`o8d`).
- **#6845 c15, c16, c17, c18:** an empty trait prints blank (`c2`, `c2b`);
  `reflect.of` on a trait value gives the key (`n09`, `n13`); an inline-module
  struct prints mangled (`n16`); a key collision panics where a diagnostic
  belongs (`BuildTraits.jet:21-24`, source reading).
- **#3102 c17:** type identity and dispatch for values crossing a `.jetlib`
  boundary (claim 15).

No defect found in this run is unfiled.

Other gaps:

- **#6845 c14:** builtins as `Type` values work (`g3b`); one key-to-TypeId
  mapping, stable across builds, remains.
- **#6883:** INI over the data tree with `decode<T>`.
- **#6833 c3:** generic arguments in member arms over an `Any`, a union and a
  trait value, a union's one instance and two instances of one generic
  included, and the loan escape diagnostic (E, C); unnameable types reaching
  `else` (E).
- **#6833 c5:** if C wins, the bare-arm rule over unions and trait values,
  with the E0641 exemption removed and E0307's fix naming each member's arm.
- **#6833 c4:** if E is adopted, a diagnostic snapshot for the extended member
  test.
- **D-PRIM-ANY1:** options E, B and C as above, recommending C.

## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| trait-to-trait conversion and a closure in a trait list are ICEs (`q2`, `q5`); generic trait values (`q6`) | card | #6727 |
| a `Decimal` in a trait value (claim 14) | card | #6839 |
| the `2.5f` suffix (claim 14) | card | #4730 |
| union, arm, view and loan defects (`#6839` c27-c57) | card | #6839 |
| `priv` struct privacy and inline-module imports (`n12b`, `n14`, `o8d`) | card | #4723 |
| an empty trait prints blank, `reflect.of` on a trait value, mangled inline-module names, a key collision panics | card | #6845 |
| one key-to-TypeId mapping stable across builds | card | #6845 |
| type identity across a `.jetlib` boundary (claim 15) | card | #3102 |
| INI over the data tree with `decode<T>` | card | #6883 |
| member arms over `Any`, unions and trait values; the bare-arm rule; the extended member test | card | #6833 |
| the choice among options E, B and C (D-PRIM-ANY1, recommending C) | card | #3838 |
<!-- /audit-dispositions -->

## Unprobed edges

A comptime value as an `Any`; a move-only resource in an `Any` or a trait
value (`z06` lends a `Close` enum payload under `from`, and it closes once);
executing the web build; O1 and O2 IR (`jet emit` refuses both,
E2102); key stability across builds that add a module; a key collision, which
no probe can force (#6845 c18 rests on source reading); a type arm itself,
including its `from item` loan, which waits on the compiler (claim 22 rests on
the enum and union analogues `w01`-`w02b`). The run makes no compile-speed
comparison with Jai: the clip's 0.449 s build is one sample on unknown
hardware (claim K15).
