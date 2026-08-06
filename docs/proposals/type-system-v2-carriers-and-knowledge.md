# Type system v2 — carriers and knowledge

Status: proposal, 2026-08-06. Owner decisions: eight ballots on card #1497.
Scope: types, numbers, units, refinements, measures, fact planes, reflection — the whole compile-time
knowledge surface. Sources: six research passes over spec, sema, prelude, Tower, and the 2026-07/08 audits.

## Executive summary

**The finding.** Six research passes over the whole corpus found the same pattern everywhere:
Jet keeps proving facts about values, and it has built a separate private machine for each kind
of fact. Ranges, units, list lengths, matrix shapes, SIMD lanes, states, tags, effects,
exactness, and function obligations — about ten machines, none sharing code, errors, or
reflection. Some store their facts as strings inside hidden type names. Two of them prove the
same thing (value ranges) with two different spellings. Three of them handle time without
talking to each other. This is not a broken design; it is ten good local designs that never met.

**The idea.** All ten are the same thing: **a type is a carrier plus knowledge.** The carrier is
the runtime shape — the bits. Knowledge is everything the compiler can prove about the value —
its range, its unit, its length, its state, how exactly the bits capture it. Knowledge lives in
planes, each with small combination rules (multiplying quantities adds dimension exponents;
adding two 0..10 values gives 0..20; matrix multiply composes shapes). The checker becomes one
engine folding knowledge through the program. Knowledge erases before codegen, so runtime cost
stays zero.

**Why now.** Twelve number decisions were ratified in the last 72 hours — bigint `Int`,
`Complex`, `Fraction`, `<=>`, nanosecond `Duration`, the operator slate — and almost none have
code yet. They all land naturally on this foundation. Building them on today's machinery means
building them twice.

**The payoffs, concretely.**
- One number model: `U8` is revealed to be `Int` plus "range 0..255, one byte" — so the sized
  widths, range types, and index proofs collapse into one prover. `BigInt` becomes redundant
  (bigint `Int` already holds its values) and retires.
- One law, already half-ratified in pieces: **knowledge is never lost silently** — every
  precision loss, unit rounding, or fact-stripping step must be spelled. Magic for beginners,
  audit trail for experts.
- Time becomes one system instead of three: `500ms` means the same thing in user code,
  `.timeout(...)`, and `#Every(...)`.
- Matrix shapes (card #1437), uncertainty propagation (deferred D-UNCERTAIN1), and full
  reflection stop being new features — they are new planes on the same substrate.

**Precise by default.** The model's own conclusion, taken all the way: the default numeric world
is fully exact — whole numbers, decimals, and fractions never lose a digit, so math, money,
science, analytics, and modeling are correct out of the box. Approximation becomes the
*restriction*: `Float`, `F32`, `U8` are expert opt-ins that read as restrictions, chosen for
memory, speed, or hardware — never suffered by default. `0.1 + 0.2 == 0.3` is true in Jet.

**What the ballots ask.** Eleven direction-level choices on card #1497: adopt the foundation
(FOUND1); adopt the number grid and retire `BigInt` (NUM1); one refinement spelling (REFINE1);
time joins the unit plane (TIME1); one substrate for compile-time numbers (MEASURE1); ratify the
conservation law (EXACT1); opt-in uncertainty propagation (UNCERT1); extend marker law zero to
every plane (PLANE1); make exact the default numeric world (DEFAULT1); inline refinements in
type position (SPELL1); imaginary literals via the unit-literal path (IMAG1). Each ballot stands
alone — any subset can be adopted, though FOUND1 is the foundation the others build on.

**See it, not just read it.** The "What it looks like" section near the end is three complete
Jet programs — beginner analytics, a measured simulation, and expert systems code — showing the
whole model in working syntax.

**What does not change.** All ratified surface spellings stay unless a ballot says otherwise.
The walls stay: no top type, no HKT, no macros, no dependent types, comptime never creates
types. Zero-cost stays. The sections below give the evidence, the model, and worked examples
for each ballot.

## Glossary

- **Carrier** — the runtime shape of a value: its bits, layout, and operations. At runtime only carriers exist.
- **Knowledge** — everything the compiler can prove about a value beyond its carrier: its range, unit,
  state, taint, exactness, length, shape, origin. Knowledge erases before codegen.
- **Plane** — one family of knowledge with its own combination rules. Dimensions are a plane.
  Ranges are a plane. Taint is a plane.
- **Algebra** — a plane's combination rules: what the fact becomes under `+`, `*`, a call, a branch.
- **Measure** — a compile-time number attached to a type: a list length, a matrix side, a lane count,
  a dimension exponent.
- **Exact / approximate / measured** — three grades of numeric knowledge: the value is perfectly known;
  known to a stated precision; known with a stated uncertainty.
- **Point / delta** — an absolute position on an axis versus a distance along it. 3 pm is a point;
  20 minutes is a delta. 40 °C is a point; a rise of 3 °C is a delta.

## The one idea

**A type is a carrier plus knowledge. Every type feature Jet has — and every one it is missing — is
one plane of knowledge over one carrier.**

`Meters` is `Float` plus the knowledge "measures Length, scale 1". `U8` is `Int` plus the knowledge
"value in 0..255, stored in one byte". `[T#4]` is `[T]` plus the knowledge "length 4".
`#PII String` is `String` plus the knowledge "classified PII". A `Reservation` in state `Confirmed`
is a `Reservation` plus the knowledge "the automaton is at Confirmed". A `Float` is a real number
plus the knowledge "approximate, 53 bits". A measured constant is a number plus the knowledge
"uncertain, σ = 5.2e-37".

Today each of those is a separate mechanism with separate code, separate errors, and separate holes.
V2 makes them the same mechanism. The checker becomes one thing: an engine that folds knowledge
through the program, plane by plane, using each plane's algebra — while the carriers compile to the
same zero-cost code as today.

Beginners never see this vocabulary. They see the magic it buys: meters times meters gives square
meters; a proven index skips its bounds check; money never silently loses a cent; `1/3` as a
`Fraction` stays exactly one third. Experts get the whole model made visible: every plane is
nameable, reflectable, and open by one law, and every default has a spelled override.

## The evidence: ten type systems in one compiler

Every row below is a compile-time fact attached to a carrier. No two rows share machinery today.

| # | Shadow system | Today's home | Defect |
|---|---|---|---|
| 1 | Dimensions | exponent map serialized into a string inside `\0Quantity` (`types.rs:621`) | type-level data smuggled as text |
| 2 | Ranges | `distinct Int(0..10)` (D-RANGETYPE1) | overlaps 3 and 4 |
| 3 | Invariants | `#Invariant("value >= 0 && value < 4")` string parser (D-REFINE1) | second spelling of 2; prover is interval-only anyway |
| 4 | Fixed widths | `IntN{signed,bits}` + hand-written `int_range` containment | secretly a range fact plus layout |
| 5 | Lengths | `FixedList{len: u64, len_symbol}` with sentinel `0` | one of four compile-time-number encodings |
| 6 | Shapes | `\0compute.dimension.N` string-encoded `Named` for `Vec<N>`/`Matrix<M,N>` | second encoding |
| 7 | Lanes | `"F32x4" => 4` string match | third encoding |
| 8 | States/tags/effects/taint | `FactRegistry` (D-FACTMODEL1, already unified internally) | dimensions were never invited |
| 9 | Exactness | widening law + `approx()` in one place, unit rounding + `from_*_rounded` in another | one concept, two vocabularies |
| 10 | Fn obligations | `effect_bound`, `param_contract`, `return_view_provenance` with three different identity rules | `Type` equality is non-transitive |

Add the three unconnected duration systems (`Duration` the type, `#UnitFamily` literals, two hardcoded
suffix tables for `.timeout` and `#Every`), and the picture is complete: the language keeps proving
the same kind of thing ten different ways.

The July audit already ratified the direction — "types-as-facts", one fact registry, open dimensions,
marker law zero, the compiler's vocabulary as readable prelude source. V2 is that direction taken to
its conclusion: **one registry of planes is the type system's second half.**

## The number tower: one lattice, two axes

Numbers are the deepest case, so they get their own section.

**Axis 1 — which mathematical world the value lives in:**

```
Whole (ℤ)  ⊂  Ratio (ℚ)  ⊂  Real (ℝ)  ⊂  Complex (ℂ)
```

**Axis 2 — what the bits know about the value:**

- **exact, unbounded** — the bits are the value (`Int` after D-INTBIG1, `Fraction`, `Decimal`)
- **exact, bounded** — the value plus a proven range and a layout (`I8`..`U64`)
- **approximate** — the value to a stated precision (`Float` = 53 bits, `F32` = 24 bits)
- **measured** — the value with a stated uncertainty (today only unit scales; see the uncertainty plane)

Every numeric type is one cell of this grid:

| | exact unbounded | exact bounded | approximate | measured |
|---|---|---|---|---|
| Whole ℤ | `Int` | `I8`..`U64` | — | — |
| Ratio ℚ | `Fraction`, `Decimal` (base-10) | — | — | — |
| Real ℝ | — | — | `Float`, `F32` | `Measurement` |
| Complex ℂ | — | — | `Complex` | — |

Three consequences, each an "of course" once seen:

**1. The sized integers are range types.** `U8` *is* `Int` plus the knowledge "0..255, one byte".
The ratified widening law — value-set containment — is not a table; it is the trivial theorem
"a subset needs no conversion". `sensor: U8 = 300` fails for exactly the same reason
`severity: Severity = 300` fails on `distinct Int(0..10)`. One prover: interval facts. It replaces
`numeric_widening_to`, `int_range`, the D-RANGETYPE1 checks, the `#Invariant` string parser, and
the fixed-list index proof — five mechanisms today, one in v2. Surface unchanged: `U8` still reads `U8`.

**2. The operator slate was one decision made nine times.** Every rule ratified on 2026-08-05 is
the tower answering "which world does the exact answer live in, and what does the approximation
policy say":
- `7 / 2` — the exact answer leaves ℤ and lives in ℚ; the beginner default approximates ℚ to `Float`,
  so `3.5`. `Fraction` users keep exactness by asking for it.
- `2 ^ -3` — a written negative exponent leaves ℤ; same landing, `Float` (D-EXPSEM1, D-EXPNEG1).
- `factorial(25)`, `2 ^ 200` — never leave ℤ, so they are exact, because `Int` *is* ℤ (D-INTBIG1).
- `sqrt(2.0)` — leaves ℚ for ℝ. `sqrt(-1.0)` leaves ℝ; it is a domain fault unless you asked for `Complex`.
- `<=>` — `Ordering` is the knowledge "how two values compare", reified as a value (D-CMP3WAY1).

**3. `BigInt` is now a duplicate mechanism.** D-INTBIG1 makes `Int` arbitrary-precision with a
machine-word fast path — exactly what `BigInt` was for. Greenfield law says one canonical form:
retire `BigInt` into `Int`, and E0130–E0133 (anti-promotion errors) retire with it. `Decimal`
stays: base-10 exactness is genuinely different knowledge.

The eleven ratified-but-unbuilt number decisions (bigint `Int`, `Complex`, `Fraction` polish,
`<=>`/`Ordering`, ns `Duration`, the operator slate remnants) all land *on* this grid. Building
them as grid instances means building them once.

## Precise by default: approximation is the restriction

The grid has a second conclusion the first draft stopped short of. D-INTBIG1 already made the
*whole-number* default exact — overflow stopped existing for beginners. The same move is
available for the rest of the tower: **make the whole default numeric world exact, and make
approximation an expert restriction you opt into, exactly like `U8`.**

- `0.1` is an exact `Decimal`, so `0.1 + 0.2 == 0.3` is **true**. The single most famous
  beginner footgun in programming dies.
- `7 / 2` is exactly `3.5`; `1 / 3` is exactly one third, printed `1/3` (a value with no finite
  decimal prints as a fraction; a finite one prints as a decimal). `third * 3 == 1` is true.
- Money, statistics, unit conversions, and long simulations accumulate **zero** representation
  error by default. Precision loss happens only where the conservation law demands a spelling.
- Approximation enters in exactly two ways, both visible: an expert writes a restricted type
  (`Float`, `F32`) for speed, memory, or hardware; or a function that mathematically leaves ℚ
  (`sqrt`, `sin`, `pi`) answers approximate — documented in its signature, the one honest
  boundary no exact system can cross.
- Performance keeps the D-INTBIG1 playbook: machine-word fast paths for small values, and the
  expert escape is one word at the declaration site. Hot loops that want raw floats say `Float`
  and get exactly today's machine arithmetic.

This amends three ratified rules, and the ballot names all three explicitly: D-INTDIV1's
`/`-lands-in-`Float` (the landing world becomes exact ℚ), the D-EXPSEM1/D-EXPNEG1 rule that a
written negative exponent lands in `Float` (same amendment, same reason), and D-NUMTYPE1's
"Fraction is opt-in by naming it" clause (an exact ratio can now arrive from plain division).
The worlds don't move — `/` and `2 ^ -3` still leave ℤ for ℚ exactly as ratified; only the ℚ
default flips from approximate to exact. "Which world does the answer live in" was always the
real decision, and it stands.

## One law: knowledge is conserved

Every exactness rule Jet has ratified is an instance of one unstated law. V2 states it:

1. **Knowledge grows silently.** Widening (subset), flow narrowing (`x != None`), exact unit
   conversion, literal adoption — all free, because nothing is lost.
2. **Knowledge is never lost silently.** Any step that discards range, exactness, unit, state,
   taint, or provenance must be spelled: `approx(x)` for precision, `Celsius.from_kelvin_rounded(k, .NearestEven, digits: 1)`
   for inexact unit crossings, `.raw()` for unit stripping, `#Scrub(Tag)` for taint,
   `#Transition` for state, `checked()/wrapping()/saturating()` for bounded escape.
3. **Knowledge erases.** At runtime only carriers remain. Facts cost nothing.

This is the philosophy's "footguns are opt-in" made precise for types: the compiler carries
knowledge for you (magic) and refuses to drop it behind your back (safety). It also exposes today's
one true inconsistency: float-precision loss and unit-rounding loss are the same event with two
unrelated vocabularies (`approx(v)` vs `from_*_rounded(...)`). V2 keeps destination-owned
conversion names and ratifies the law; whether the two spellings merge is a ballot below.

## The planes

Each plane: what it knows, its algebra, and what unifying it buys. All planes share four laws,
extending marker law zero (ratified inside D-VERDICT-1455-1): **a plane exists iff registered;
its facts are nameable; its facts are reflectable; its declarations ship as readable prelude source.**

### Nominal plane — algebra: identity only
`UserId :: distinct Int`. The degenerate plane: one fact, "this is a UserId", no combination rules.
Today's `distinct` is unchanged. The insight: `distinct`, unit families, and dimensions are the
*same declaration family at three algebra strengths* — none, discrete, group.

### Unit plane — algebra: free abelian group on dimension axes, rational scales, torsors for affine
Already Jet's crown jewel (open dimensions, exact rational scales, provenance, point/delta). V2
changes its *representation* (a real fact on the type, not a string in `\0Quantity`) and finishes
three things:
- **Duration joins the plane it invented.** `Duration` becomes the canonical `Time` delta quantity
  with the D-TIMERES1 i64-nanosecond carrier; `Instant` is the `Time` point. `500ms` in `.timeout(...)`,
  `#Every(5min)`, and user code become one literal resolved one way; both hardcoded suffix tables die.
  Three duration systems become zero special cases.
- **The affine pattern gets named.** `CelsiusPoint`/`CelsiusDelta`, `Instant`/`Duration`,
  and index/length are the same shape: a point on an axis versus a distance along it. Ratified
  D-QUANTITY-POINT1 already built the machinery; v2 reuses it instead of re-deriving it per domain.
- **Quantities reach reflection** everywhere: `TypeInfo.dimensions` becomes a real typed field,
  completing what D-QUANTITY-PRINT1 already did for printing.

### Interval plane — algebra: interval arithmetic on measures
One refinement story: `Severity :: distinct Int(0..10)`. The `#Invariant("...")` string form retires —
the prover is interval-only today, so the string spelling provably adds nothing but a parser.
Sized widths sit here too (above) for *checking and conversion*: the same interval facts drive
containment widening and fit errors. Their *arithmetic* contracts do not change: a `distinct`
refinement widens to its base carrier exactly as ratified (D-RANGETYPE1), while the sized widths
keep trap-on-overflow plus `wrapping`/`saturating`/`checked`, exactly as D-INTBIG1 reaffirmed.
Arithmetic folds intervals — `[0,10] + [0,10] = [0,20]` — which is how the widening result and
the index proofs (D-OOBPROOF1) fall out of one prover.

### Measure plane — algebra: naturals with per-operation rules
One substrate for every compile-time number in a type: `[T#N]` lengths, `Vec<N>`/`Matrix<M,N>`
shapes, SIMD lanes, dimension exponents. Concatenation adds lengths; `matmul` composes
`(M×K)·(K×N) → M×N`; `.len` on `[T#N]` is the fact, free. This deletes all four encodings
(sentinel `u64`, `\0compute.dimension.N` strings, lane string-match, `CtValue::Int`) and gives
card #1437 (matrix surface) its foundation without deciding its syntax. Measures are declared
literals or module value-parameters — never computed by user code, so S26's wall
("comptime never creates types") stands untouched.

### Classification plane — algebra: lattices and automata
Tags (powerset + deny), taint (closed kinds now, user kinds post-E3), effects (tree subsumption),
states (automaton). Already one registry internally (D-FACTMODEL1) — v2 keeps the ratified
surfaces (`tag`, `effect`, `state`) and finishes the missing halves: every fact nameable
(`Type.State.Name` shipped; tags and effect leaves join), every fact reflectable as typed values
(the `Reflect.rs:151` string fallback dies), one subsumption engine.

### Exactness plane — algebra: worst-grade-wins with spelled demotions
Exact / approximate(precision) / measured(σ). Today this knowledge exists in three places that
cannot see each other: the widening exactness check, unit scale provenance
(`Rational | SymbolicPi | Conventional | Measured`), and `core.science.measurement`. As one plane:
an exact `Int` times a measured constant is measured; the compiler can *tell you* your simulation's
uncertainty instead of you deriving it by hand. This revives deferred D-UNCERTAIN1 as a plane
instance, not a feature — opt-in, invisible until a measured value enters.

### Obligation plane — algebra: subsumption on function types
`effect_bound`, `param_contract`, `return_view_provenance`, plus ownership conventions. V2 states
one identity law and fixes the real defect found in review: **carrier determines type identity;
obligations compare by subsumption; equality is transitive again.** (Today `fn(Bool)` equals two
contracted types that are unequal to each other — `types.rs:305-391`.)

## What this unlocks

- **Simulation and science** — units + exactness + measures: dimensioned, uncertainty-carrying,
  shape-checked math with zero runtime cost. The Python/MATLAB pitch the owner asked for, checked
  at compile time.
- **Money** — `Decimal` + currency families + conservation law: no silent cent ever.
- **Indexing and systems code** — interval facts erase bounds checks; fixed widths are honest
  refinements; FFI boundaries become systematic range crossings.
- **Graphics/ML** — measure plane: shapes, lanes, and (post-#1437) matrix operators on one substrate.
- **Protocols and security** — classification plane: states, linearity, taint — unchanged surface,
  finished internals.
- **Tooling** — `T.reflect()` finally shows the same model the checker uses. `jet explain` can
  answer "what does the compiler know about this value, and where did it learn it" — the expert
  audit story, and the best teaching tool a beginner error can have.

## What does not change

- All ratified surface spellings stay unless a ballot below says otherwise. `U8`, `distinct`,
  `#UnitFamily`, `tag`, `state`, `effect`, `T?`, `T ? E`, unions, `Type.{ }` — untouched.
- The walls stay: no top type (D-ANY-JAI1), no HKT (D-LIB2), no macros, no dependent types —
  measures are declared, never computed. Comptime never creates types (S26). Facts classify and
  erase; they never dispatch (D-FACTMODEL1).
- Zero-cost stays: knowledge erases; carriers compile exactly as today. I9 parity is unaffected
  because planes live entirely in sema.
- The marker registry, law zero, and the mid-flight #1455–#1461 rebuild are the pattern v2
  extends, not competes with.

## The surface: spellings from first principles

Three spelling principles fall out of the model, and each yields concrete surface proposals:

**1. The default spelling is the mathematical name; restrictions read as restrictions.**
`Int` means ℤ. The exact ℚ world needs no name in daily code — it is just what numbers do.
`Float`, `F32`, `U8`, `I16` are restrictions and *look* like restrictions: terse, technical,
machine-flavored. A beginner who never needs them never sees them; an expert scanning code sees
every restriction at a glance. No spelling changes needed — the existing names already obey the
principle once the defaults flip.

**2. Knowledge you can state inline, you should be able to state inline.** Today a checked range
requires minting a named type first. The general form lets the type position carry the fact
directly:

```jet
fn set_brightness(level: Int(0..100)) { ... }     // proposed: inline refinement
volume: Int(0..11) = dial.read()?                  // fallible where unproven
UserId :: distinct Int                             // naming is still there when identity matters
```

`U8` is then revealed as exactly `Int(0..255)` plus a one-byte layout — the alias teaches the
model. The same inline position accepts unit and exactness knowledge later without new grammar.

**3. Literals reuse one literal machinery.** The lexer already turns `500ms` and `12.5usd` into
unit literals. Two more number worlds fit the identical path, with no new grammar:

```jet
z :: 3 + 4i                       // proposed: imaginary literal — `i` rides the unit-suffix path
g :: 9.80665 ± 0.00001            // proposed: measured literal (plain form: measurement(...))
```

`4i` is a unit literal whose family is ℂ's imaginary axis — the "same underlying thing" made
literal. `±` makes a measured value cost one keystroke more than a guess, which is the whole
adoption battle for honest numbers.

Worth future ballots, listed but not minted now: compound unit suffixes (`9.8m/s^2` instead of
declaring a derived family member), and exponent-aware unit printing. Both are surface sugar
over machinery this proposal already builds.

## What it looks like

Three complete programs. Everything not marked *proposed* is ratified syntax; the semantic
differences from today are the ballots of this slate in action.

### P1 — a beginner's first analytics script (nothing opted in, everything exact)

```jet
fn main() {
    price :: 19.99                    // exact Decimal — not a Float
    total :: price * 3
    print("total: {total}")           // total: 59.97   — exactly

    print(0.1 + 0.2 == 0.3)          // true — the classic footgun is gone

    share :: 7 / 2
    print(share)                      // 3.5    (finite decimal prints as a decimal)
    third :: 1 / 3
    print(third)                      // 1/3    (no finite decimal — prints exactly)
    print(third * 3 == 1)            // true

    n :: 2 ^ 200
    print(n)                          // 1606938044258990275541962092341162602522202993782792835301376
    print(factorial(25))              // 15511210043330985984000000 — Int is ℤ, full stop
}
```

### P2 — a measured simulation (units + uncertainty + shapes, all checked, all zero-cost)

```jet
fn main() {
    h :: 100meter                             // a Length quantity
    g :: 9.80665 ± 0.00001                    // proposed ± literal: a measured value

    t :: sqrt(2 * h.raw() / g)                // sqrt leaves ℚ — result is approximate,
    print("fall time: {t}")                   // and carries the propagated uncertainty:
                                              // fall time: 4.51600 ± 0.0000023

    later :: now() + 5min                     // point + delta = point (Time joins units)
    task.timeout(500ms)                       // same literal, same meaning, everywhere

    a :: Matrix<3, 4>.{ ... }                 // proposed surface — matrix design is card #1437;
    b :: Matrix<4, 2>.{ ... }                 // the measure plane is what makes it checkable
    c :: a * b                                // Matrix<3, 2> — shapes compose at compile time
    // a * a                                  // error: inner sides 4 and 3 do not match

    k :: 293.15kelvin
    c2 :: Celsius.from_kelvin(k)              // exact conversion: silent and free
    f :: Fahrenheit.from_celsius_rounded(c2, .NearestEven, digits: 1)
                                              // inexact: the loss is spelled, per the law
}
```

### P3 — expert systems code (restrictions where they pay, each one visible)

```jet
struct Packet {
    kind: U8                                  // one byte on the wire — a restriction, spelled as one
    len: U16
    body: [U8#1024]                           // length is a compile-time measure
}

fn checksum(bytes: [U8]) => U8 {
    sum: U8 = 0
    for b in bytes { sum = wrapping(sum + b) }   // overflow behavior chosen, not suffered
    return sum
}

#Kernel fn blend(a: F32x4, b: F32x4) => F32x4 =
    a * 0.5 + b * 0.5                         // approximate and fast — on purpose, and it shows

fn set_brightness(level: Int(0..100)) { ... }  // proposed inline refinement
fn on_dial(raw: Int) {
    level :: Int(0..100).from_int(raw) ?? return   // unproven → fallible, same as U8 today
    set_brightness(level)
}

fn to_wire(reading: Float) => F32 {
    return approx(reading)                    // precision loss exists — so it is spelled
}
```

The through-line: the beginner program contains zero annotations and zero surprises; the expert
program contains only visible, chosen restrictions; and every line in between is the
conservation law doing its job.

## Decisions for the owner

Direction-level; each gets a full ballot on the card. Worked examples live in the sections above.

| ID | Question | Recommendation |
|---|---|---|
| D-TYPE2-FOUND1 | Adopt the carrier+knowledge foundation, the plane registry law, and the one identity law for `Type` equality | adopt |
| D-TYPE2-NUM1 | Adopt the two-axis number tower; sized widths become interval+layout facts (surface unchanged); retire `BigInt` into `Int` | adopt |
| D-TYPE2-REFINE1 | One refinement spelling: keep `distinct Int(lo..hi)`, retire `#Invariant("...")` | retire the string form |
| D-TYPE2-TIME1 | `Duration`/`Instant` become the `Time` quantity pair on the unit plane; delete both hardcoded suffix tables | adopt |
| D-TYPE2-MEASURE1 | One measure substrate for lengths, shapes, lanes, and dimension exponents | adopt |
| D-TYPE2-EXACT1 | Ratify the conservation law; keep `approx` and `from_*_rounded` as its two spelled demotions, or merge them into one word | ratify law, keep both spellings |
| D-TYPE2-UNCERT1 | Uncertainty as an exactness-plane grade (revives D-UNCERTAIN1), opt-in | adopt as opt-in |
| D-TYPE2-PLANE1 | All planes nameable + reflectable + prelude-source by law (extends the D-VERDICT-1455-1 registration law); `TypeInfo` gains `dimensions` and typed marker args at every level | adopt |
| D-TYPE2-DEFAULT1 | The default numeric world is exact: decimal literals are `Decimal`, `/` lands in exact ℚ (amends D-INTDIV1's landing type), approximation is opt-in | adopt |
| D-TYPE2-SPELL1 | Inline refinements in type position: `Int(0..100)` wherever a type is written; sized widths become teachable aliases | adopt |
| D-TYPE2-IMAG1 | Imaginary literals `4i` ride the existing unit-literal path | adopt |

## Implementation shape

Effort is expendable; the sequence is what matters.

- **Phase A — re-found internally, no surface change.** One knowledge representation on `Type`;
  one interval prover; one measure substrate; one fact registry covering dimensions; transitive
  equality. Every existing test keeps passing.
- **Phase B — land the owed ratifications on the new substrate.** Bigint `Int` (#1436), `Complex`,
  `Fraction`, `<=>`/`Ordering` (#1435), ns `Duration` (#1466) — built once, as grid and plane
  instances, full I9 parity.
- **Phase C — the balloted surface unifications.** Refinement spelling, Duration→Time, `BigInt`
  retirement, uncertainty opt-in, reflection completion. Each is a coherent in-repo migration with
  the replaced form deleted, per greenfield law.

Phase A is pure consolidation and can start on ratification of D-TYPE2-FOUND1 alone; B and C
follow their own ballots and the existing cards they absorb.
