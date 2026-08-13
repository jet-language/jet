# Isomorphic ontology audit — 2026-07-28

Prose in this note follows the `simple` skill. Code, decision IDs, paths, and
Jet spellings stay exact.

This run follows the 2026-07-27 audit. The stable map carries forward. The
focus is the surface that changed since that run: two rulings ratified today
and one spec canon fix.

## Thesis

Jet now teaches one construction law across the whole literal surface: **the
head names the type; the body is a recipe in that type's notation.**
`Type.{ fields }`, `SQL.{"…"}`, `HTML.{"…"}`, `Sh.{"…"}`, `[U8].{"…"}`, and
`Type.{ uninit }` are one head with different body notations (D-UNIFYLIT1=A).
The literal-prefix family (`sql"…"`, `html"…"`, `sh"…"`, `b"…"`) is gone, and a
bare `"…"` never changes meaning by context.

Code passed to a call is now spelled as what it is: a lambda argument inside
the parentheses (D-TRAILBLOCK2=A). The trailing `{ }` sugar — a second spelling
for passing a function value, and a false rhyme with a plain block — is
retired.

Yesterday's remaining risks stand: shared glyphs with more than one job
(`#`, `&`, `^`, `|`), and S83 multi-head functions beside `if` dispatch tables.
Today's rulings removed two of the drift sources, so overall isomorphic
consistency improved.

Authority for this run:

- `.agents/skills/isomorphic-ontology-audit/ontology.md`
- `docs/spec/philosophy.md`
- `docs/spec/syntax-decisions.md` (ratified; includes D-TRAILBLOCK2=A and
  D-UNIFYLIT1=A, both 2026-07-28)
- `docs/spec/spec.md` M1–M2 (typed text, `T?` return canon)
- `crates/jet-foundation/src/Syntax.rs` and `Syntax/*.rs` (I7)
- `examples/features/{basics,types,safety,parsing,syntax}/`
- `docs/audits/isomorphic-ontology-audit-2026-07-27.md` (baseline)

## Dual-facet scorecard

| Lens | Grade | Evidence |
| --- | --- | --- |
| Exploratory density vs Python | aligned (local drift) | Prelude `print`/`input`; inferred effects; bare-param lambdas; yielding `loop … ->`; `?.`/`??`. New small costs: `twice(() => { … })` replaces `twice { … }`; `SQL.{"…"}` replaces silent elaboration. Both buy clarity. |
| Systems expressiveness vs Zig/Rust/Odin/C/C++ | aligned | Params bare/`&`/`^`; `~` copy; `#Unsafe("reason")` + typed `assert` obligations; effect rows/`#Grant`; `[U8].{"…"}` bit-typed holes with `be`/`le`; `Reader.take_pattern`; FFI/`#Layout`/`Type.{ uninit }`; comptime. |
| Clarity | aligned (improved) | The silent expected-type rewrite of bare strings into `SQL`/`HTML` is gone — a literal's meaning no longer depends on distant context. Bare `{ }` after a call is E0335 with a fix showing `callee(() => { … })`. |
| Isomorphic consistency | aligned (improved from drift) | One literal head law (D-UNIFYLIT1=A); one code-argument spelling (D-TRAILBLOCK2=A); one `T?`/`T ? E` spacing law in every type position including returns. Remaining drift: `#` beyond rules; `&`/`^` bitwise; S83. |

## Concept map (Jet → ontology)

Status values: teaches well / partial / broken / false rhyme / absent.
Rows unchanged since 2026-07-27 are carried forward; changed rows are marked
**(changed)**.

### Meta slots (M0–M13)

| Jet form | Ids | Axes | What it is | Status |
| --- | --- | --- | --- | --- |
| Expression / statement / decl split | M4–M6 | X06 | One grammar; braces group; braces do not force a result | teaches well |
| `fn run` / typed CLI entry (`#CLI`, `#[Flag]`) | M11, D20 | X19 | Program root; entry type owns argv | teaches well |
| `comptime` / `#Caller` / embed / `build.os` fold | M12, P04 | X03 | Same Jet at compile time | teaches well |
| Effect rows / `#Grant` / taint | M9, T10 | X04,X19 | World acts as type data | teaches well |
| Diagnostics + UI snapshots (I4) | H06 | — | Compiler messages as product | teaches well |

### Values (V*)

| Jet form | Ids | Axes | What it is | Status |
| --- | --- | --- | --- | --- |
| `Int`/`Float`/`Bool`/`String`; sized `I8`…`F64`; `BigInt`/`Decimal` | V05–V08,V12 | X19 | Default scalars plus expert widths and exact numbers | teaches well |
| `()` / Void callables | V01 | — | Unit | teaches well |
| Named tuples `(x: 1, y: 2)` | V20 | X01 | Product with required names | partial (clear; longer) |
| `struct` / `Type.{…}` / field punning | V21, C01 | — | Named product and literal | teaches well |
| `[T]`, `[T#N]`, `[K: V]`, `Set`/`Tally` | V22,V25,V26 | X03 | Collections; fixed length as refinement | partial (`#N` vs `#Rule`) |
| `enum` / leading-dot / nested groups | V40,V41,V46 | — | Tagged sums; group names a subtree | teaches well |
| `T?` / `Val`/`None`; `T ? E` / `Ok`/`Err` | V42,V43 | — | Optional and Result as separate spellings | teaches well |
| `A \| B` anonymous union (D-UNIONTYPE1) | V40,T16 | — | Closed structural sum sugar | teaches well |
| `fn` values / `(p) =>` / bare `x =>` | V60 | X01 | Callable values | teaches well |
| Code arguments `f(() => { … })` **(changed)** | V60,C04 | X01,X20 | A code argument is an ordinary lambda argument (D-TRAILBLOCK2=A) | teaches well |
| Methods / `self`/`&self`/`^self` | V62 | X07,X16 | Function plus receiver plus access | teaches well |
| Tasks / channels / `#Shield` / `Stream<T>` + `yield` | V64,V68,V69 | X10 | Concurrent and suspended values | teaches well |
| `distinct` / quantities / unit lits / `Duration` | V05+T20 | X08 | Nominal and dimensional scalars | teaches well |
| Typed text values `SQL`/`HTML`/`Sh` **(changed)** | V12+T24,V71 | X09,X15 | Checked domain text; holes become bound params / escaped insertions / argv items | teaches well |
| Byte patterns `[U8].{"…"}` **(changed)** | V71,V13 | X08 | Byte-mode recipe in the ONE pattern engine (D-BINPAT1 as amended) | teaches well |
| Protocol handles / typestate tags | V51,T11 | X09 | Session values with authority | teaches well |

### Types (T*)

| Jet form | Ids | Axes | What it is | Status |
| --- | --- | --- | --- | --- |
| `name: Type` on sigs/fields only (D-BIND-BARE1) | T01,B01 | X15 | Types stay off local binding names | teaches well |
| `Type<Args>`, list bounds `[A,B]` | T02,T18,A01 | — | Generics; no `where` | teaches well |
| `trait` / `impl` / associated types / `impl Type.Add` | D06,D07,A02,A14 | — | Named contracts and witnesses | teaches well |
| Range/`refine` distinct types | T05 | X03 | Predicate types at declaration | teaches well |
| Effect rows in `=[…]=>` | T10,T29 | X04 | Effect types on callables | teaches well |
| Access on params (`T`/`&T`/`^T`); `~` copy | T08,R06,R07 | X16,X19 | Ownership in the type; no lifetime syntax | teaches well |
| `View`/`ViewMut`/`str` | V29,T08 | X16 | Safe windows; facts, not surface lifetimes | teaches well |
| `T?` vs `T ? E` spacing in **all** positions, incl. returns **(changed)** | V42,V43 | X15 | One spacing law; `=> (T?)` parens now optional grouping | teaches well |
| Higher-kinded / full dependent | T03,T04 | — | Out of scope | absent (deferred) |

### Bindings / names (B*)

| Jet form | Ids | Axes | What it is | Status |
| --- | --- | --- | --- | --- |
| `name :: expr` / `name := expr` / `=` reassign | B01,B02,C11 | X02 | Immutable vs mutable binding | teaches well |
| Destructuring `.{…}` `(…)` `[…]` + `..` rest | B03 | X06 | Pattern binding | teaches well |
| `_` wildcard; `_name` soft-public; `__` reserved | B04,B15 | — | Ignore and tier prefixes | teaches well |
| `use` / `pub use` / `as` | B14,D10 | — | Binding transfer across modules | teaches well |
| `pub` / `priv` / `#PubFile` / `pub(package)` | B15 | X14 | Visibility | teaches well |
| Loop labels `outer :: loop`; `break(name, value)` | B10 | — | Control names, not values | teaches well |
| Path `Mod.item` / `Type.method` | B08 | X01 | Qualified names | teaches well |
| Casing law Pascal/snake (`NAME_CASE_CATEGORIES`) | H13 | — | Name shape teaches kind | teaches well |
| No overloading (D-CAP10); S83 multi-head | B18 vs C15 | — | One def per name; pattern heads dispatch | partial |

### Computation (C*)

| Jet form | Ids | Axes | What it is | Status |
| --- | --- | --- | --- | --- |
| Literals / interpolation `{x}` / `{x#Debug}` | C01,C27 | — | Value intro plus format | partial (`#` selector) |
| Typed-literal heads `Type.{ body }` **(changed)** | C01,C24 | X15 | One elaboration head; body notation chosen by the head (fields, quoted recipe, `uninit`) | celebrated |
| Calls / labels / defaults / `...` spread | C04,C05 | — | Application | teaches well |
| Code argument `(() => { … })` **(changed)** | C04,V60 | X20 | No trailing-block sugar; bare `{ }` after call is E0335 | teaches well |
| `.` field / `[]` index / swizzle | C08,C09 | — | Product and sequence elim | teaches well |
| `f.[a,b,c]` fan-out | C04,C48 | — | One call mapped over a list | teaches well |
| No general pipe; methods instead (D-SHAPE-PIPE1) | C06 | — | Chosen on purpose | leave alone |
| `if` effect / value / `== {` table / subjectless | C14,C15 | — | One branch mechanism | celebrated |
| Pattern `==` tests / guards `&&` / flow narrowing (D-FLOWTYPE1) | C15 | — | Sum elim as Bool; proven unwraps | teaches well |
| String patterns `"…{hole}…"` / `String.{"…"}`; byte patterns `[U8].{"…"}` **(changed)** | C15,C68 | X08 | ONE pattern engine, text and byte modes; `take_pattern` is consume mode | teaches well |
| `loop` family + `->` yield + `break`/`next` | C16,C18,C28 | — | Iteration and list build | celebrated |
| `return` / `?? return\|next\|break` | C18,C21 | — | Early exit family | teaches well |
| `?` / `??` / `?.` | C21 | — | Error and optional control | partial (many `?` forms) |
| `++`/`--` vs `+=` | C11 | X20 | Second mutation spelling | ceremony / I8 exception |
| `Target.from_source` / `Target.parse` | C24 | — | Explicit conversion | teaches well |
| `require` / `panic` / `#Pre`/`#Post` / unsafe `assert` obligations | C25 | — | Contracts | teaches well |
| `defer close(^r)` | C22,R13 | X19 | Narrow end-of-scope close | teaches well |
| `para_*` / `taskgroup` / `#Transact` | C40–C48 | X05 | Explicit parallelism | teaches well |

### Declarations (D*)

| Jet form | Ids | Axes | What it is | Status |
| --- | --- | --- | --- | --- |
| `fn` / expression body `=> T = e` | D03,V60 | X01 | Named callable = binding of a function | celebrated |
| `struct`/`enum`/`alias`/`distinct` | D04,D05 | — | Type intro | teaches well |
| `trait`/`impl` / `fn Type.method` | D06–D08 | — | Contract plus orphan extension | teaches well |
| `module` / generic modules / `module _name` | D09,M8 | X03,X14 | Namespace unit; comptime args; discovery opt-out | teaches well |
| `#Test`/`#Bench` | D14 | — | Verification decls | teaches well |
| `extern` / `#FFI` / `#Bindgen` / `#ABI` | D13 | X09 | Foreign | teaches well |
| `protocol` / `state` / `migration` / `validate` | D16,D19,D22 | — | Contextual declaration family | teaches well |
| `#` applied rules / `#[A,B]` (`Policy::APPLIED_RULES`) | P09 | X19 | One metadata mechanism | teaches well (plane) |

### Effects / memory / safety (E*, R*, S*)

| Jet form | Ids | Axes | What it is | Status |
| --- | --- | --- | --- | --- |
| Inferred effects; `=[]=>` purity | E01,T10 | X04,X15 | Pure when the row is empty | teaches well |
| Ten roots + dotted leaves | E02–E12 | — | Closed effect vocabulary | teaches well |
| `#Grant` / `=[!E]=>` / `#Caps` | E18,S03 | X19 | Capability and prohibition | teaches well |
| `#Tainted` / `#Sanitizer`; E0149 typed-text boundary **(changed)** | T21,S03 | — | Information flow; a runtime `String` cannot enter `SQL`/`HTML`/`Sh` without `.raw` audit | teaches well |
| `#Unsafe("reason")` + `assert valid_ptr, aligned, no_alias` | S09,S11 | X09,X19 | Audited escape with typed obligations | teaches well |
| `~x` copy; call-site `&`/`^` | R06,C10 | X16 | Explicit copy and access | teaches well |
| Arena/Bump/Pool/Fixed; `#Region`; `#Context(allocator:)` | R09,R10 | X19 | Allocation policy | teaches well |
| `Type.{ uninit }` (D-UNINIT-SENTINEL2) | R01,S09 | X19 | Uninitialized storage as a literal-head body | teaches well |
| Opt-in GC policy | R04 | X19 | Expert reclaim | teaches well |

### Human surface (H*)

| Jet form | Ids | Axes | What it is | Status |
| --- | --- | --- | --- | --- |
| Keywords `fn if loop …` (no `for`/`while`/`match`) | H01 | X19 | Small beginner set | teaches well |
| Sigils `:: := => -> ? # & ^ ~` | H02 | X20 | Dense; some glyphs do two jobs | partial |
| `#` rules vs `@` locations | H02,P09 | — | Rule vs location planes | celebrated |
| Literal prefixes (`sql"`, `b"`, …) **(changed)** | H03 | — | Retired — no prefix namespace to learn or extend | teaches well (by absence) |
| Synthetic `;`; nesting block comments; `"""` | H04,H05 | — | Spatial grammar | teaches well |
| `jet repl` / `jet dev` / `#Persist` | H08 | X18 | Exploratory loop | teaches well |

## Concept families

Families A–L from the 2026-07-27 run are unchanged unless listed. Changed and
new families:

### Family I′ — The typed-literal head (was Family I, widened)

- Members: `Type.{ fields }`, collection literals, `.{}` expected-type form,
  `SQL.{"…"}`, `HTML.{"…"}`, `Sh.{"…"}`, `[U8].{"…"}`, `String.{"…"}` pattern
  head, `Type.{ uninit }`, `.new(...)`, `.from_*`, `.parse`, unit literals.
- Shared ontology: C01 introduction under a named classification (M1), plus
  C24 at conversion edges. One law: the head names the type; the body is a
  recipe in that type's notation.
- Spellings today: one head after D-UNIFYLIT1=A. The prefix costumes
  (`sql"…"`, `html"…"`, `sh"…"`, `b"…"`) and the silent expected-type rewrite
  are retired (E0149 guards the boundary; `.raw` is the audited escape).
- Score: clarity high — the reader can always answer "what is this literal?"
  by reading the head. Isomorphism high — domain text, byte patterns, structs,
  and `uninit` share the intro form. Exploratory cost is a few tokens per
  literal; systems power grows (bit-typed holes, endian suffixes, argv-safe
  `Sh`).
- Move: leave alone. This is the "ohhh" this audit exists to find:
  `SQL.{"…"}` ≈ `Point.{x: 1}` — both elaborate a body under a head. Teach it
  with that one phrase.

### Family M — Code as argument (new; replaces the trailing-block row)

- Members: lambda args `f(() => { … })`, bare-param lambdas `x => e`,
  callbacks, `taskgroup` child bodies.
- Shared ontology: V60 + C04. Passing code is passing a function value.
- Spellings: one, after D-TRAILBLOCK2=A. A bare `{ }` after a call is E0335
  with the fix `callee(() => { … })`.
- Score: isomorphism improved — the sugar was a second spelling for one job
  (I8 pressure) and rhymed falsely with a plain scope block (C13). The cost is
  four tokens per call site; DSL-style builders read slightly heavier.
- Move: leave alone. Watch UI-heavy code for ergonomic pressure; any revival
  of block sugar is owner-gated and should reuse the lambda ontology, not a
  parallel form.

### Family F′ — Optional / fallible spacing law (was Family F, completed)

- Members: `T?`, `T ? E`, `T ? (E1 | E2)`, postfix `?`, `??`, `?.`,
  `Val`/`None`, `Ok`/`Err`, D-FLOWTYPE1 narrowing.
- Change: return position now follows the same tight/spaced law as every
  other type position; `=> (T?)` parens are ordinary grouping, not required.
- Score: the last positional exception is gone. One law everywhere.
- Move: leave alone. Teach the spacing law once.

### Family N — Pattern-head symmetry (new observation)

- Members: text patterns bare `"…{hole}…"` (head optional: `String.{"…"}`);
  byte patterns mandatory `[U8].{"…"}`.
- Shared ontology: C15/C68 — the ONE pattern engine in two modes; the head
  selects the mode.
- Score: partial isomorphism, deliberately tiered. Text matching is the
  beginner default and stays bare; byte matching is expert and names its type.
  The asymmetry is a teaching choice, not drift: the head is exactly the
  information the byte mode needs.
- Move: leave alone. Docs should present `[U8].{"…"}` as "the pattern engine
  wearing the byte head," next to `String.{"…"}`.

Families A (callable `=>`), B (`->` selection), C (universal `if`),
D (bindings), E (access sigils), G (bars), H (`#` rules), J (modules),
K (multi-head), L (increment) are unchanged from 2026-07-27.

## Findings

1. Keep / celebrate — one literal head law (D-UNIFYLIT1=A)

   Kind: celebrate (resolved missed unification + removed false rhyme).
   Ids: C01, V71, T24; H03. Evidence: `docs/spec/syntax-decisions.md`
   D-UNIFYLIT1=A (card #1265); `Syntax/package_files.rs` (retired `b` prefix);
   `Syntax/core_surface.rs` (retired `$typed_text_*` sentinels);
   `examples/features/safety/typed_sql.jet`;
   `tests/ui/take_pattern_string_typed_bad_hole.jet`. Dual-facet: scripts pay
   a head per domain literal and gain a visible boundary; systems gain
   `[U8].{"…"}` bit-typed holes under the same head. Rec: keep; teach the one
   phrase "the head names the language; the body is its quoted recipe." Do
   not add new literal prefixes — that surface is superseded. Owner-gate: no.

2. Keep / celebrate — code arguments are lambdas (D-TRAILBLOCK2=A)

   Kind: celebrate (resolved I8 pressure + false rhyme). Ids: V60, C04 vs
   C13. Evidence: syntax-decisions D-TRAILBLOCK2=A (card #1266); E0335
   diagnostics row; `examples/features/syntax/trailing_block.jet`;
   `tests/ui/trailing_block_on_index.jet`. Dual-facet: small token cost in
   callback-heavy code; no systems impact. Rec: keep. If builder DSL pressure
   returns, any sugar must desugar to the same lambda, not a parallel
   mechanism. Owner-gate: no.

3. Keep / celebrate — one `T?`/`T ? E` spacing law in all type positions

   Kind: celebrate (removed positional exception). Ids: V42, V43, X15.
   Evidence: `docs/spec/spec.md` return-type section now cites
   D-RESULT-OPTION-CANON1; `=> (T?)` demoted to optional grouping. Rec: keep.
   Owner-gate: no.

4. Clarity — E0149 boundary replaces silent elaboration

   Kind: clarity win worth naming separately. Ids: T21, S03, X15. Evidence:
   spec.md "Bare `\"…\"` never elaborates into these types"; E0149;
   `tests/ui/typed_text_bare_string_into_sql.jet` and siblings. Impact: a
   string literal's meaning no longer depends on the expected type of a
   distant position — reading order is local again. Rec: keep. Owner-gate: no.

5. False rhyme — `#` beyond applied rules (carried)

   Kind: false rhyme. Ids: P09 vs V22 size (`[T#N]`) vs U04 version
   (`pkg#ver`) vs C27 format (`{x#Debug}`). Unchanged since 2026-07-27; note
   D-UNIFYLIT1=A did not touch `#`. Rec: teach the three `#` grammars; ballot
   a fixed-length respell only if explain data shows confusion. Owner-gate:
   yes — ballot title unchanged from the 07-27 run.

6. False rhyme — `&` / `^` as capability and bitwise operators (carried)

   Kind: false rhyme. Ids: T08 vs C29. Unchanged. Rec: leave alone;
   diagnostics must name "write capability" vs "bitwise and". Owner-gate: no.

7. Missed unification — S83 multi-head vs `if` dispatch (carried)

   Kind: missed unification. Ids: C15 twice. Unchanged. Rec: document as a
   pair; prefer `if` tables in examples; ballot only to narrow S83.
   Owner-gate: yes — ballot title unchanged from the 07-27 run.

8. Ceremony without teaching — `++`/`--` beside `+=` (carried)

   Kind: ceremony; owner I8 exception (D-INCR1). Unchanged. Rec: leave.
   Owner-gate: no.

9. Facet — token cost of the new explicitness

   Kind: facet (acceptable drift). Ids: X20 vs X15. Evidence: `twice(() => {`
   vs `twice {`; `SQL.{"…"}` vs bare string. Impact: exploratory scripts grow
   by a few tokens exactly where a value crosses a trust or code boundary.
   That is the right place to spend tokens (ceremony that teaches). Rec: no
   action. Owner-gate: no.

## Celebrated isomorphisms

Carried from 2026-07-27, plus new entries marked •new:

- Named function ≈ binding of a function value (same `=>`).
- Lambda ≈ anonymous function; method ≈ function + `self` + access sigil.
- •new Code argument ≈ lambda argument — no special trailing form.
- Pattern match ⊂ `if`; `for`/`while`/comprehension ⊂ `loop`.
- Optional / Result as sums with control sugar (`?`/`??`); •new one spacing
  law in every type position.
- •new `SQL.{"…"}` ≈ `Point.{x: 1}` ≈ `[U8].{"…"}` ≈ `Type.{ uninit }` — one
  head, many body notations; the head names the type, the body is its recipe.
- •new Byte pattern ≈ string pattern wearing the byte head — one engine, two
  modes, mode chosen by the head.
- Module ≈ namespace record; `use` ≈ binding transfer.
- Trait ≈ required operations; `impl` ≈ witness.
- Comptime ≈ same Jet at compile time.
- `#Unsafe` ≈ locally weakened safety with an audit string and typed
  obligations.
- Bars ≈ alternatives; `#` rule ≈ one metadata mechanism; `@` ≈ location.
- Copy `~x` ≈ explicit duplicate; unmarked read ≈ default access.

## Ontology gaps / extensions

| Ontology id | Jet landing | Notes |
| --- | --- | --- |
| T03 HKT | absent | Declined D-LIB2 |
| T04 dependent | absent / deferred | |
| T09 lifetimes as syntax | deliberately absent | Views + facts instead |
| C06 general pipe | deliberately absent | D-SHAPE-PIPE1 |
| C19 goto | absent | |
| C60–C63 logic/SQL-as-lang | library (`core.solve`, `SQL` typed text), not surface | |
| V63 continuations | absent | |
| P01 user macros | ceiling Tier 3 rejected; user literal prefixes now also superseded (D-UNIFYLIT1=A) | |
| A09 prototypes | absent | |
| Q08 HoTT | out of scope | |

Extensions to `ontology.md` this run: none. D-UNIFYLIT1=A's typed-literal
head lands cleanly in C01 + M1 with X15; D-TRAILBLOCK2=A is V60 + C04. No new
family needed.

## Next actions

Ballot titles or docs work only — no cards created this run.

1. *Multi-head functions: keep, document-as-dual-of-`if`, soft-deprecate, or
   restrict to public API?* (carried from 07-27; still the largest open
   unification.)
2. *Fixed-length list spelling: keep `[T#N]` or replace the non-rule `#`?*
   Raise only if explain data shows marker confusion. (carried.)
3. Non-ballot docs pass: extend the planned "Jet's isomorphisms" page with the
   two new laws — "the head names the type; the body is its recipe" and "code
   you pass is a lambda." No new syntax.

No `ontology.md` edits. No syntax proposals beyond the carried titles.
