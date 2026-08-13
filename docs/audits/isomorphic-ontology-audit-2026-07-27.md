# Isomorphic ontology audit — 2026-07-27

Vocabulary: [Jet vocabulary](../spec/vocabulary.md).

Prose in this note follows the `simple` skill. Code, decision IDs, paths, and
Jet spellings stay exact.

## Thesis

Jet teaches a few core ideas with shared form.

`if` is the one branch form. It covers tests, patterns, and ordered guards.
`=>` is the one callable arrow. It covers named functions, lambdas, methods, and
effect ceilings. `Type.{ … }` is the one construction head for every literal
kind. `loop` is the one iteration form for infinite, conditional, source, and
collecting loops. `#Rule` applies metadata. `@` marks locations and sources.

The main risks are not missing features. They are shared glyphs with more than
one job (`#`, `?`, `&`, `^`, `|`), and a few pairs that share meaning but not
spelling (multi-head functions vs `if` tables; `++`/`--` beside `+=`).

Clarity still wins. D-SHAPE-PIPE1 keeps `|` off general flow. Destination-owned
`from_*` and bare bindings (`name ::` / `name :=`) teach the ontology even when
they cost short Python scripts.

Authority for this run:

- `.agents/skills/isomorphic-ontology-audit/ontology.md`
- `docs/spec/philosophy.md`
- `docs/spec/syntax-decisions.md` (ratified)
- `docs/spec/spec.md` M1–M2
- `crates/jet-foundation/src/Syntax.rs` and `Syntax/*.rs` (I7)
- `examples/features/{basics,types,memory,effects,functions}/`

## Dual-facet scorecard

| Lens | Grade | Evidence |
| --- | --- | --- |
| Exploratory density vs Python | aligned (local drift) | Prelude `print`/`input`; inferred effects (D-EFFECT-OMIT1); bare-param lambdas; yielding `loop … ->`; methods plus `?.`/`??`; no general `\|>` (D-SHAPE-PIPE1); named-only tuples |
| Systems expressiveness vs Zig/Rust/Odin/C/C++ | aligned | Params `bare`/`&`/`^`; `~` copy; Views without lifetime syntax; `#Unsafe("reason")`; effect rows/`#Grant`; `#Transact`/`#Shield`; FFI/`#Layout`/`uninit`; comptime; generic modules |
| Clarity | aligned | `::`/`:=`; `=>` vs `->` (D-ARROW-CONTROL1); `Val`/`None` vs `Ok`/`Err`; bars mean alternatives |
| Isomorphic consistency | drift (local) | `#` used outside rules; `&`/`^` also bitwise; S83 multi-head vs `if` dispatch; owner I8 exception for `++`/`--` |

## Concept map (Jet → ontology)

Group rows by ontology family. Status values: teaches well / partial / broken /
false rhyme / absent.

### Meta slots (M0–M13)

| Jet form | Ids | Axes | What it is | Status |
| --- | --- | --- | --- | --- |
| Expression / statement / decl split | M4–M6 | X06 | One grammar; braces group; braces do not force a result | teaches well |
| `fn run` / typed CLI entry | M11, D20 | X19 | Program root; entry type owns argv when present | teaches well |
| `comptime` / `#Caller` / embed | M12, P04 | X03 | Same Jet at compile time | teaches well |
| Effect rows / `#Grant` / taint | M9, T10, E* | X04,X19 | World acts as type data | teaches well |
| Diagnostics + UI snapshots | H06, I4 | — | Compiler messages as product | teaches well |

### Values (V*)

| Jet form | Ids | Axes | What it is | Status |
| --- | --- | --- | --- | --- |
| `Int`/`Float`/`Bool`/`String`; sized `I8`…`F64` | V05–V07,V12 | X19 | Default scalars plus expert widths | teaches well |
| `()` / Void callables | V01 | — | Unit | teaches well |
| Named tuples `(x: 1, y: 2)` | V20 | X01 | Product with required names | partial (clear; longer) |
| `struct` / `Type.{…}` / field punning | V21, C01 | — | Named product and literal | teaches well |
| `[T]`, `[T#N]`, `[K: V]`, `Set`/`Tally` | V22,V25,V26 | X03 (`#N`) | Collections; fixed length as refinement | partial (`#N` vs `#Rule`) |
| `enum` / leading-dot / nested groups | V40,V41,V46 | — | Tagged sums; group names a subtree | teaches well |
| `T?` / `Val`/`None`; `T ? E` / `Ok`/`Err` | V42,V43 | — | Optional and Result as separate spellings | teaches well |
| `A \| B` anonymous union | V40,T16 | — | Closed structural sum sugar | teaches well |
| `fn` values / `(p) =>` / bare `x =>` | V60 | X01 | Callable values | teaches well |
| Methods / `self`/`&self`/`^self` | V62 | X07,X16 | Function plus receiver plus access | teaches well |
| Tasks / channels / `#Shield` | V64,V68,V69 | X10 | Concurrent values plus cancel region | teaches well |
| `distinct` / quantities / unit lits | V05+T20 | X08 | Nominal and dimensional scalars | teaches well |
| Protocol handles / typestate tags | V51,T11 | X09 | Session values with authority | teaches well |

### Types (T*)

| Jet form | Ids | Axes | What it is | Status |
| --- | --- | --- | --- | --- |
| `name: Type` on sigs/fields only | T01, B01 | X15 | Types stay off local binding names | teaches well |
| `Type<Args>`, list bounds `[A,B]` | T02,T18,A01 | — | Generics; no `where` | teaches well |
| `trait` / `impl` / associated types | D06,D07,A02,A14 | — | Named contracts and witnesses | teaches well |
| Range/`refine` distinct types | T05 | X03 | Predicate types at declaration | teaches well |
| Effect rows in `=[…]=>` | T10,T29 | X04 | Effect types on callables | teaches well |
| Access on params (`T`/`&T`/`^T`) | T08,R06,R07 | X16,X19 | Ownership in the type; no lifetime syntax | teaches well |
| `View`/`ViewMut`/`str` | V29,T08 | X16 | Safe windows; facts, not surface lifetimes | teaches well |
| Higher-kinded / full dependent | T03,T04 | — | Out of scope for now | absent (deferred) |

### Bindings / names (B*)

| Jet form | Ids | Axes | What it is | Status |
| --- | --- | --- | --- | --- |
| `name :: expr` / `name := expr` / `=` | B01,B02,C11 | X02 | Immutable vs mutable binding | teaches well |
| Destructuring `.{…}` `(…)` `[…]` | B03 | X06 | Pattern binding | teaches well |
| `_` wildcard | B04 | — | Ignore | teaches well |
| `use` / `pub use` / `as` | B14,D10 | — | Binding transfer across modules | teaches well |
| `pub` / `priv` / `#PubFile` / `pub(package)` | B15 | X14 | Visibility | teaches well |
| Loop labels via `outer :: loop` | B10 | — | Control names, not values | teaches well |
| Path `Mod.item` / `Type.method` | B08 | X01 | Qualified names | teaches well |
| No overloading (D-CAP10); S83 multi-head | B18 vs C15 | — | One def per name; pattern heads dispatch | partial |

### Computation (C*)

| Jet form | Ids | Axes | What it is | Status |
| --- | --- | --- | --- | --- |
| Literals / interpolation `{x}` / `{x#Debug}` | C01,C27 | — | Value intro plus format | partial (`#` selector) |
| Calls / labels / defaults / `...` spread | C04,C05 | — | Application | teaches well |
| Trailing `{ }` block arg | C04 | X20 | Final lambda sugar | teaches well |
| `.` field / `[]` index / swizzle | C08,C09 | — | Product and sequence elim | teaches well |
| `f.[a,b,c]` fan-out | C04,C48′ | — | One call mapped over a list | teaches well |
| No general pipe; methods instead | C06 | — | Chosen on purpose | leave alone |
| `if` effect / value / `== {` table / subjectless | C14,C15 | — | One branch mechanism | celebrated |
| Pattern `==` tests / guards `&&` | C15 | — | Sum elim as Bool | teaches well |
| `loop` family + `->` yield + `break`/`next` | C16,C18,C28 | — | Iteration and list build | celebrated |
| `return` / `?? return\|next\|break` | C18,C21 | — | Early exit family | teaches well |
| `?` / `??` / `?.` | C21 | — | Error and optional control | partial (many `?` forms) |
| `++`/`--` vs `+=` | C11 | X20 | Second mutation spelling | ceremony / I8 exception |
| `Target.from_source` | C24 | — | Explicit conversion | teaches well |
| `require` / `panic` / `#Pre`/`#Post` | C25 | — | Contracts | teaches well |
| `defer close(^r)` | C22,R13 | X19 | Narrow end-of-scope close | teaches well |
| `para_*` / `#Transact` / tasks | C40–C48 | X05 | Explicit parallelism | teaches well |

### Declarations (D*)

| Jet form | Ids | Axes | What it is | Status |
| --- | --- | --- | --- | --- |
| `fn` / expression body `=> T = e` | D03,V60 | X01 | Named callable = binding of a function | celebrated |
| `struct`/`enum`/`alias`/`distinct` | D04,D05 | — | Type intro | teaches well |
| `trait`/`impl` / `fn Type.method` | D06–D08 | — | Contract plus orphan extension | teaches well |
| `module` / generic modules | D09,M8 | X03 | Namespace unit; optional comptime args | teaches well |
| `#Test`/`#Bench` | D14 | — | Verification decls | teaches well |
| `extern` / `#FFI` / `#Bindgen` | D13 | X09 | Foreign | teaches well |
| `protocol` / `state` / `migration` / `validate` | D16,D19,D22 | — | Contextual declaration family | teaches well |
| `#` applied rules / `#[A,B]` | P09 | X19 | One metadata mechanism | teaches well (plane) |

### Effects / memory / safety (E*, R*, S*)

| Jet form | Ids | Axes | What it is | Status |
| --- | --- | --- | --- | --- |
| Inferred effects; `=[]=>` purity | E01,T10 | X04,X15 | Pure when the row is empty | teaches well |
| Ten roots + dotted leaves | E02–E12 | — | Closed effect vocabulary | teaches well |
| `#Grant` / `=[!E]=>` / `#Caps` | E18,S03 | X19 | Capability and prohibition | teaches well |
| `#Tainted` / `#Sanitizer` | T21,S03 | — | Light information-flow tags | teaches well |
| `#Unsafe("reason")` | S09 | X09,X19 | Audited escape | teaches well |
| `~x` copy; call-site `&`/`^` | R06,C10 | X16 | Explicit copy and access | teaches well |
| Shared/Pool/Id / `#SingleUse` | R05,T07 | — | Sharing and linearity | teaches well |
| Opt-in GC policy | R04 | X19 | Expert reclaim | teaches well |

### Human surface (H*)

| Jet form | Ids | Axes | What it is | Status |
| --- | --- | --- | --- | --- |
| Keywords `fn if loop …` | H01 | H10 | Small beginner set; no `for`/`while`/`match` | teaches well |
| Sigils `:: := => -> ? # & ^ ~` | H02 | X20 | Dense; some glyphs do two jobs | partial |
| `#` vs `@` planes | H02,P09 | — | Rule vs location | celebrated |
| Casing law Pascal/snake | H13 | — | Name shape teaches kind | teaches well |
| `jet repl` | H08 | X18 | Exploratory loop | teaches well |

## Concept families

### Family A — Callable results (`=>` / `=[E]=>`)

- Members: named `fn`, lambdas, methods, fn types, computed fields
  (`name: T => expr`), conversions, migration converters.
- Shared ontology: V60 + C03 + optional T10. A value that yields a result under
  an effect ceiling.
- Spellings: one arrow family after D-ARROW-CONTROL1.
- Score: clarity high; isomorphism high; scripts shorter than Rust; systems keep
  explicit ceilings.
- Move: leave alone. Do not put returns back on `->`.

### Family B — Control selection (`->` / `if` / `loop`)

- Members: `if` arms, subjectless guards, collecting loop items, ordinary break
  payloads.
- Shared ontology: C14/C15/C16. Choose or produce the next control value.
- Spellings: `->` marks selected or yielded values. Effect-only bodies omit the
  arrow.
- Score: isomorphism high after the arrow split. Clarity is good after one
  teach pass.
- Move: leave alone. Teach `=>` = define callable, `->` = select or yield.

### Family C — One branch keyword

- Members: two-arm `if`, value `if`, `if subject == {…}`, subjectless `if {…}`,
  pattern `==` tests.
- Shared ontology: C14 covers C15. Match is branch on structure.
- Spellings: only `if` (`KW_SWITCH = "if"` in Syntax).
- Score: strong once `== {` is learned. Risk: `==` looks like overloaded
  equality.
- Move: leave alone. Docs and diagnostics should say `== {` opens a dispatch
  table. Do not rename the marker now.

### Family D — Bindings and mutability

- Members: `::`, `:=`, `=`, `#Track`, destructuring.
- Shared ontology: B01/B02/C11.
- Score: mutability is clear. Fine for scripts and systems.
- Move: leave alone.

### Family E — Access / ownership sigils

- Members: bare read, `&` write, `^` take, `~` copy, Views.
- Shared ontology: T08/R06/R07/V50. Location plus authority.
- Spellings: type position and call site. Bitwise `&`/`^` stay expression ops.
- Score: strong for systems. False rhyme with bitwise ops. Position separates
  the jobs.
- Move: leave alone for v1. A later capability-only glyph set needs a ballot.

### Family F — Optional / fallible control (`?` cluster)

- Members: `T?`, `T ? E`, postfix `?`, `??`, `?.`, `Val`/`None`, `Ok`/`Err`.
- Shared ontology: V42/V43 + C21. Absence and failure as data, plus sugar.
- Score: good isomorphism. Spaced fallible vs glued optional teaches well. Many
  `?` forms still load newcomers.
- Move: leave alone. Teach the spacing law. Do not add a second Result sugar.

### Family G — Alternatives bar (`|`)

- Members: or-patterns, anonymous unions, choice arms; also bitwise OR / `|=`.
- Shared ontology (intended): alternatives (D-SHAPE-PIPE1). Bitwise OR is the
  arithmetic case.
- Score: good refusal of bar-as-pipe. Mild false rhyme with BitOr.
- Move: leave alone. Current law is correct.

### Family H — Applied rules (`#`)

- Members: `#Unsafe`, `#Grant`, `#Test`, `#Cli`, derives, contracts, block
  regions; also non-rule `#` in `[T#N]`, `pkg#ver`, `{x#Debug}`.
- Shared ontology: P09 for rules. Size, version, and format selectors are other
  jobs that share the glyph.
- Score: the rule plane teaches well. Non-rule uses are a false rhyme.
- Move: teach the three `#` grammars in docs and `jet explain`. Respell
  `[T#N]` only if real confusion shows up. That respell is owner-gated.

### Family I — Construction and conversion

- Members: `Type.{…}`, `.{}`, `.new`, `.from_*`, `.parse`, unit lits.
- Shared ontology: C01/C24/D19. Intro vs change of type.
- Score: destination-owned conversion is clearer than `as` or `to_*`.
- Move: leave alone.

### Family J — Modules as records of bindings

- Members: `module`, `use`, re-export, generic modules, `#PubFile`.
- Shared ontology: M8 is like a record of bindings (calibration #3).
- Score: strong. Generic modules add comptime value args with care.
- Move: leave alone.

### Family K — Multi-head functions vs dispatch tables

- Members: S83 `fn area(Circle(…))` / `fn area(Rect(…))` vs
  `if s == { .Circle -> … }`.
- Shared ontology: C15 pattern dispatch on a sum.
- Spellings: declaration-site multi-def vs expression-site table.
- Score: missed unification. Two costumes for one elim.
- Move: teach them as a pair that share one matcher. Prefer `if` tables in
  examples. A ballot may narrow S83 later. Do not add a third match keyword.

### Family L — Increment spellings

- Members: `++`/`--` vs `+= 1` (D-INCR1, explicit I8 exception).
- Shared ontology: C11 mutation of an integer place.
- Score: ceremony without a new idea. Familiar to C users. Shorter in places.
- Move: leave alone (owner law). Do not add more duplicate mutation glyphs.

## Findings

1. Keep / celebrate — Universal `if` (C14 covers C15)

   Kind: celebrate. Evidence: `if_universal.jet`, `pattern_matching.jet`,
   Syntax `KW_SWITCH="if"`. Dual-facet: guard tables like Python; exhaustiveness
   like Rust; no `match`. Rec: keep. Do not bring back `match` or `switch`.
   Owner-gate: no.

2. Keep / celebrate — Callable vs control arrows

   Kind: celebrate. Evidence: D-ARROW-CONTROL1,
   `arrow-syntax-consistency-2026-07-27` audit. Rec: keep. Owner-gate: no.

3. Keep / celebrate — `#` rules vs `@` locations

   Kind: celebrate. Evidence: D-VERDICT-732-1. Rec: keep the plane split.
   Owner-gate: no.

4. Keep / celebrate — Named fn ≈ function value; method ≈ fn + receiver

   Kind: celebrate. Evidence: S46/S47, same `=>`, coercion rules. Rec: keep.
   Owner-gate: no.

5. Keep / celebrate — `loop` covers for / while / comprehension

   Kind: celebrate. Evidence: S19, `loop_values.jet`, D-COMPREHENSION1. Rec:
   keep. Owner-gate: no.

6. False rhyme — `#` beyond applied rules

   Kind: false rhyme. Ids: P09 vs V22 size vs U04 version vs C27 format.
   Evidence: D-SHAPE2 allows non-rule `#`. Impact: a beginner may read
   `[U8#3]` as a marker. Rec: teach three `#` grammars. Ballot a fixed-length
   respell only if explain data shows real confusion. Owner-gate: yes — ballot
   title *“Fixed-length list spelling: keep `[T#N]` or replace non-rule `#`?”*
   only if metrics demand it.

7. False rhyme — `&` / `^` as capability and bitwise operators

   Kind: false rhyme. Ids: T08 vs C29 bitwise. Evidence: `SIGIL_WRITE` /
   `SIGIL_MOVE` vs `OP_AMP` / `OP_CARET`. Impact: systems readers cope;
   script readers may stumble in mixed expressions. Rec: leave alone. Make
   sure diagnostics say “write capability” vs “bitwise and”. Owner-gate: no
   unless a capability respell wave starts.

8. Missed unification — S83 multi-head vs `if` dispatch

   Kind: missed unification. Ids: C15 for both. Evidence: syntax-decisions S83
   vs S68. Impact: two ways to elim a sum; I8 pressure. Rec: document as a
   pair; prefer `if` tables in examples; ballot only to narrow S83.
   Owner-gate: yes — *“Multi-head functions: keep, soft-deprecate to sugar, or
   restrict to public API?”*

9. Ceremony without teaching — `++`/`--` beside `+=`

   Kind: ceremony. Ids: C11 twice. Evidence: D-INCR1 I8 exception. Impact:
   shorter code; weaker ontology teaching. Rec: leave (owner law).
   Owner-gate: no.

10. Clarity risk — `if subject == {` marker

    Kind: clarity (mild). Ids: C14/C15 using the Q01 glyph. Evidence: E0992
    requires `==` before `{`. Impact: looks like overloaded equality. Rec: keep
    the mechanism; use one teach phrase (“dispatch table”). Owner-gate: no.

11. Facet — exploratory density without `|>`

    Kind: facet (acceptable drift). Ids: C06 declined. Evidence: D-SHAPE-PIPE1.
    Impact: pipe fans write names or methods instead. Clarity stays. Rec: leave
    alone. Keep iterator methods short (`map`/`filter`/`has`). Owner-gate: no.

12. Facet — named-only tuples

    Kind: facet. Ids: V20. Evidence: S73. Impact: Python `(1,2)` is longer in
    Jet; `.0` muddle is gone. Rec: leave alone. Clarity beats golf.
    Owner-gate: no.

13. Missed unification (soft) — computed field `=>` vs method

    Kind: partial isomorphism. Ids: V60 vs field sugar. Evidence: D-FIELDPOL1.
    Impact: teaches “field is a pure getter”. Unsettable field vs `fn` may
    confuse. Rec: leave. Docs should say: computed field ≡ zero-arg pure method
    without call parens. Owner-gate: no.

14. Keep — destination-owned conversion

    Kind: celebrate. Ids: C24. Evidence: D-SHAPE-CONVERT1. Rec: keep. Do not
    revive `as`. Owner-gate: no.

15. Keep — effect omission + empty row purity

    Kind: celebrate / dual-facet. Ids: T10,X04,X15. Evidence: D-EFFECT-OMIT1,
    D-SHAPE8. Impact: Python-like scripts stay quiet; systems pin `=[]=>` /
    `=[Fs]=>`. Rec: keep. Owner-gate: no.

## Celebrated isomorphisms

- Named function ≈ binding of a function value (same `=>`).
- Lambda ≈ anonymous function (same arrow; optional parens).
- Method ≈ function + `self` + access sigil.
- Pattern match ⊂ `if` (no second keyword).
- `for` / `while` / list-comp ⊂ `loop`.
- Optional / Result as sums (`Val`/`None`, `Ok`/`Err`) with control sugar
  (`?`/`??`).
- Module ≈ namespace record; `use` ≈ binding transfer.
- Trait ≈ required operations; `impl` ≈ witness.
- Concurrent work as tasks/channels, not a second language.
- Comptime ≈ same Jet at X03=compile-time.
- `#Unsafe` ≈ locally weakened safety with an audit string.
- Bars ≈ alternatives (patterns/unions), not flow.
- `Type.{…}` ≈ one elaboration head for every literal kind.
- `#` applied rule ≈ one metadata mechanism (in rule grammar).
- Copy `~x` ≈ explicit duplicate; unmarked read ≈ default access.

## Ontology gaps / extensions

| Ontology id | Jet landing | Notes |
| --- | --- | --- |
| T03 HKT | absent | Declined D-LIB2 |
| T04 dependent | absent / deferred | |
| T09 lifetimes as syntax | deliberately absent | Views + facts instead (philosophy) |
| C06 general pipe | deliberately absent | D-SHAPE-PIPE1 |
| C19 goto | absent | |
| C60–C63 logic/SQL-as-lang | library (`core.solve`), not surface | |
| V63 continuations | absent | |
| P01 user macros | ceiling Tier 3 rejected | |
| A09 prototypes | absent | |
| Q08 HoTT | out of scope | |

Extensions to `ontology.md` this run: none. Every Jet form landed in an existing
family plus X-axes. Glyph overload is an H02/X20 concern, not a new family.

## Next actions

Ballot titles only. Do not create cards unless asked.

1. *Multi-head functions: keep, document-as-dual-of-`if`, soft-deprecate, or restrict to public API?*
2. *Fixed-length list spelling: keep `[T#N]` or replace the non-rule `#`?*
   Raise only if explain data shows marker confusion.
3. Non-ballot docs pass: one page “Jet’s isomorphisms” for `=>`/`->`, `if`
   tables, `loop` yields, `#` vs `@`, and the `?` cluster. No new syntax.

No `ontology.md` edits. No syntax proposals beyond the titles above.
