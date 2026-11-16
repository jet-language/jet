# Foundational programming ontology

Canonical primitive catalog for the isomorphic-ontology audit.
Language-agnostic. Not Jet-specific. Not a popularity contest.

**Honesty bound.** No finite list is the last word on “everything ever.”
This catalog aims to be *closed at the category layer* and *open at the
instance layer*: every known programming idea should land in exactly one
primary family (plus optional secondary tags). When something new appears,
extend an existing family or add a named family — do not invent a parallel
taxonomy.

**How to read entries**

- **Family** — irreducible kind of thing
- **Members** — common spellings / mechanisms across languages
- **Frontier** — rare, research, or not yet widely integrated
- **Orthogonal axes** — compose independently of the family
- **Isomorphism hints** — “same idea, different costume”

---

## 0. Meta: what a language is made of

These are the slots every surface construct fills. Map Jet spellings here first.

| Id | Primitive | One-line meaning |
| --- | --- | --- |
| M0 | **Value** | A runtime (or comptime) datum |
| M1 | **Type** | A classification / contract on values |
| M2 | **Name / symbol** | A handle that refers |
| M3 | **Binding** | Association of a name to a thing in a scope |
| M4 | **Expression** | Form that yields a value |
| M5 | **Statement / command** | Form done for effect (may also yield) |
| M6 | **Declaration** | Form that introduces bindings / types / modules |
| M7 | **Scope / environment** | Region where bindings are visible |
| M8 | **Module / unit** | Separately nameable, often separately compiled, namespace |
| M9 | **Effect** | Observable interaction beyond pure value production |
| M10 | **Evaluation strategy** | When/how subforms run (eager, lazy, staged, parallel…) |
| M11 | **Program / entry** | Root of evaluation / linking |
| M12 | **Metaprogram** | Program that constructs or transforms programs |
| M13 | **Proof / evidence** | Machine-checkable justification (types-as-proofs, contracts, SMT) |

Orthogonality baseline: **naming ∥ anonymity**, **declaration ∥ expression**,
**value ∥ type ∥ kind**, **pure ∥ effectful**, **static ∥ dynamic time**,
**beginner default ∥ expert opt-in**.

---

## 1. Values (what exists)

### 1.1 Scalar / atomic data

| Id | Concept | Notes / aliases |
| --- | --- | --- |
| V01 | Unit / void / nil-of-nothing | `()`, `void`, `Unit`, “no information” |
| V02 | Never / bottom / empty type | No values; divergence, abort, unreachable |
| V03 | Boolean | Truth values; may be enum of two |
| V04 | Bit | 0/1 as data (not just boolean logic) |
| V05 | Integer | Fixed-width, arbitrary precision, saturating, wrapping |
| V06 | Natural / unsigned | Non-negative integers as distinct concept |
| V07 | Floating point | Binary/decimal IEEE-ish; NaN policies |
| V08 | Fixed-point / decimal | Money, exact fractions of powers of 10 |
| V09 | Rational | Exact p/q |
| V10 | Complex / quaternion | Algebraic extensions |
| V11 | Character | Unicode scalar / code unit / grapheme (distinct!) |
| V12 | Text / string | Sequence of characters/bytes with encoding policy |
| V13 | Bytes / blob | Opaque octet sequence |
| V14 | Symbol / atom / interned name | Identity-by-name values (Lisp, Erlang, Ruby) |
| V15 | Enumerated tag | Named inhabitant without payload |
| V16 | Numeric tower / dynamic number | Scheme-style tower; units of measure (separate) |
| V17 | Instant / duration / calendar | Time as data |
| V18 | Identifier as data | Names reified (reflection, macros) |

**Frontier:** dual numbers / hyper-duals; tropical / interval arithmetic;
finite fields; algebraic numbers; soft floats; posits; unums.

### 1.2 Product / aggregate data

| Id | Concept | Notes |
| --- | --- | --- |
| V20 | Tuple / positional product | Anonymous fields by index |
| V21 | Record / struct / object fields | Named fields |
| V22 | Array / vector (dense sequence) | Fixed or dynamic length; contiguous |
| V23 | List / linked / persistent seq | Inductive sequences |
| V24 | String-like ropes / text buffers | Specialized sequences |
| V25 | Map / dictionary / association | Key → value |
| V26 | Set / multiset / bag | Membership collections |
| V27 | Matrix / tensor / ndarray | Ranked rectangular data (array languages) |
| V28 | Graph / tree / trie / heap | Explicit relational / hierarchical structures |
| V29 | Buffer / slice / view / span | Window onto contiguous memory |
| V30 | Stream / iterator / lazy seq | On-demand sequences |
| V31 | Table / relation / dataframe | Columnar / relational value |
| V32 | Heterogeneous list / HList | Type-level varying products |

**Frontier:** CRDTs as values; versioned/persistent everything; content-addressed
blobs; columnar batches; sparse tensors; meshes; images/audio/video as
first-class media values; geospatial geometries.

### 1.3 Sum / choice data

| Id | Concept | Notes |
| --- | --- | --- |
| V40 | Tagged union / variant / ADT | Sum of products |
| V41 | Enum with payloads | C-style enum upgraded |
| V42 | Optional / nullability | Presence/absence (`Option`, `?T`, null) |
| V43 | Result / either / error sum | Success vs failure as data |
| V44 | Open sum / polymorphic variant | Extensible cases (OCaml poly variants, Row) |
| V45 | Intersection / merge value | Value inhabiting multiple shapes at once |
| V46 | Nested / recursive ADT | Inductive data |

### 1.4 References, identity, and locations

| Id | Concept | Notes |
| --- | --- | --- |
| V50 | Reference / pointer / address | Indirection to a location |
| V51 | Handle / capability token | Unforgeable right to operate |
| V52 | Object identity | Identity distinct from structural equality |
| V53 | Weak / soft reference | Non-owning observability |
| V54 | Far / remote reference | Location may be another process/node |
| V55 | Interior pointer / field address | Pointer into aggregate |
| V56 | Function pointer / code address | Callable as data (unchecked) |
| V57 | Fat pointer / vtable/iface ptr | Data + metadata pointer pair |

### 1.5 Computational values

| Id | Concept | Notes |
| --- | --- | --- |
| V60 | Function / closure / lambda | Callable value; may capture |
| V61 | Procedure / routine | Callable primarily for effect (may be same as V60) |
| V62 | Method / receiver-bound function | Function with distinguished self |
| V63 | Continuation | Reified rest-of-computation |
| V64 | Coroutine / generator / async state machine | Suspendable computation value |
| V65 | Partial application / curried fn | Function waiting for more args |
| V66 | Type / class / trait as value | Reified type (dynamic or dependent) |
| V67 | Effect / handler as value | First-class algebraic effects |
| V68 | Channel / port / mailbox | Communication endpoint as value |
| V69 | Promise / future / task handle | Standing for a value-not-yet |
| V70 | Lens / optic / bidirectional transform | Get/set (or more) as data |
| V71 | Parser / grammar / regex as value | Recognizers as data |
| V72 | Probability / distribution | Random variables as values |
| V73 | Differentiable tape / dual computation | AD-carrying values |
| V74 | Quote / AST / code literal | Program fragments as data |
| V75 | Proof object / witness | Evidence as data |

**Isomorphism hints**

- Named function declaration ≈ binding a name to a function value (V60 + M3).
- Constants and named functions often share “named immutable binding” ontology.
- Methods ≈ functions with an extra implicit/explicit argument + dispatch rule.
- Async functions ≈ functions returning V64/V69, not a separate universe.

**Frontier:** quantum states/circuits as values; neural weights as values with
ops; world/ambient terms; session-typed endpoints; holographic/multiverse
speculative values; reversible computation fragments.

---

## 2. Types and kinds (what classifies)

| Id | Concept | Notes |
| --- | --- | --- |
| T01 | Concrete type | Fully known type |
| T02 | Type variable / generic param | Placeholder |
| T03 | Higher-kinded type / type constructor | `List`, `Option` as constructors |
| T04 | Dependent type / value-indexed | Types depend on values |
| T05 | Refinement / predicate type | `{ n: Int \| n > 0 }` |
| T06 | Liquid / SMT-backed type | Solver-mediated refinements |
| T07 | Linear / affine / relevant / ordered | Resource usage disciplines |
| T08 | Ownership / uniqueness / borrowing type | Alias control |
| T09 | Region / lifetime / arena type | Extent of validity |
| T10 | Effect / IO / purity type | What a computation may do |
| T11 | Session / protocol type | Structured communication protocols |
| T12 | Gradual / dynamic / unknown type | `Any`, `dyn`, gradual |
| T13 | Structural vs nominal typing | Sameness by shape vs by name |
| T14 | Subtyping / variance | Inclusion relationships |
| T15 | Row / extensible record or variant type | Open products/sums |
| T16 | Intersection / union type (as types) | Type-level ∧ / ∨ |
| T17 | Existential / abstract type | Hidden representation |
| T18 | Universal quantification | ∀ polymorphism |
| T19 | Kind / sort | Type of types |
| T20 | Units of measure | Dimensional types |
| T21 | Information-flow / security label | Confidentiality/integrity types |
| T22 | Modal / temporal / spatial type | Necessity, eventually, locality |
| T23 | Graded / quantitative type | Usage counts, privacy budgets |
| T24 | Capability / authority type | Rights in the type |
| T25 | Fictional / phantom type | Compile-time only tags |
| T26 | Singleton / literal type | Type inhabited by one value |
| T27 | Negation / complement type | “Not T” |
| T28 | Recursive / mu type | Iso/equi recursive |
| T29 | Effect row / polymorphic effects | Extensible effect sets |
| T30 | Proof-relevant equality / path type | HoTT-style |

**Frontier:** homotopy type theory in practical langs; cost/complexity types;
energy types; approximate/probabilistic types; multi-stage types; choreographic
types; binary-level layout types as user surface.

---

## 3. Bindings, names, and namespaces

| Id | Concept | Notes |
| --- | --- | --- |
| B01 | Immutable binding | `let`/`const`/val |
| B02 | Mutable binding | `var`/`mut`/cell |
| B03 | Pattern binding / destructuring | Bind via structure |
| B04 | Wildcard / ignore binding | Deliberate non-name |
| B05 | Alias / rename | New name, same thing |
| B06 | Shadowing | Rebind name in inner scope |
| B07 | Forward declaration / hole | Name before body |
| B08 | Qualified / path name | `Mod::item` |
| B09 | Operator name | Symbolic callable names |
| B10 | Label (control name) | Loop/block labels, not values |
| B11 | Lifetime / region name | Names for extents |
| B12 | Type name / type alias | Names in type namespace |
| B13 | Namespace / module path | Hierarchical naming |
| B14 | Import / export / re-export | Binding transfer across modules |
| B15 | Visibility / access control | Public/private/friend/pub(crate) |
| B16 | Dynamic binding / special vars | Lisp special, thread-locals as names |
| B17 | Hygiene / macro-introduced names | Names that must not clash |
| B18 | Overloading / multiple bindings per name | Disambiguated by type/arity |
| B19 | Canonical / content-addressed name | Hash-as-name |

**Isomorphism hints**

- Parameters are bindings.
- Match arms introduce bindings.
- Import is binding transfer, not a fifth universe.
- Labels are names for control points, not values (unless reified).

---

## 4. Computation forms (what programs do)

### 4.1 Core calculi operations

| Id | Concept | Notes |
| --- | --- | --- |
| C01 | Literal introduction | Construct a value in place |
| C02 | Variable reference | Read a binding |
| C03 | Function abstraction | Create callable (named or anonymous) |
| C04 | Application / call | Invoke callable with arguments |
| C05 | Named / labeled arguments | Application with explicit param names |
| C06 | Piping / threading | Application sugar (`|>`, method chains) |
| C07 | Composition | Make new function from functions |
| C08 | Projection / field access | Product elimination |
| C09 | Indexing / lookup | Sequence/map elimination |
| C10 | Update / functional record update | New product from old |
| C11 | Assignment / mutation | Change binding or location |
| C12 | Sequencing | Do A then B |
| C13 | Block / scope expression | Scoped sequence yielding value |
| C14 | Conditional / branch | Choose by proposition/pattern |
| C15 | Pattern match / case | Eliminate sums by structure |
| C16 | Loop / iteration | Repeated computation |
| C17 | Recursion / mutual recursion | Self-reference |
| C18 | Early exit | `return` / `break` / `continue` / labeled |
| C19 | Nonlocal exit | `goto`, longjmp, multi-level return |
| C20 | Exception throw/catch | Dynamic non-local control + value |
| C21 | Error propagate / `?` / `try` | Sugar for Result-like control |
| C22 | Defer / finally / RAII scope end | End-of-scope actions |
| C23 | Resource acquire/release | Bracketed lifetimes of resources |
| C24 | Cast / coerce / convert | Change type or representation |
| C25 | Assert / check / contract | Runtime or static obligation |
| C26 | Reflect / typecase / instanceof | Branch on runtime type |
| C27 | Print / format / interpolate | Textualization (often effectful) |
| C28 | Comprehension / query form | Set/list/dict builders; SQL-like |
| C29 | Operator section / infix form | Syntactic application variants |
| C30 | Macro invocation / annotation use | Metaprogram call sites |

### 4.2 Parallel, concurrent, distributed

| Id | Concept | Notes |
| --- | --- | --- |
| C40 | Spawn / fork task | Start concurrent work |
| C41 | Join / await / sync | Wait for completion |
| C42 | Send / receive message | Message passing |
| C43 | Channel ops / select | Multiplex communication |
| C44 | Lock / critical section | Mutual exclusion |
| C45 | Atomic RMW / CAS | Lock-free primitives |
| C46 | Barrier / latch / condition | Coordination |
| C47 | STM / transaction | Speculative shared memory |
| C48 | Data-parallel map/reduce/scan | Bulk parallelism |
| C49 | SIMD / GPU kernel launch | Wide/heterogeneous compute |
| C50 | Actor behavior change | Become / hot swap behavior |
| C51 | Cancel / timeout / deadline | Time-bounded control |
| C52 | Distributed consensus / election | Multi-node agreement |
| C53 | Remote call / RPC / capability invoke | Cross-address-space call |

### 4.3 Specialized computational models

| Id | Concept | Notes |
| --- | --- | --- |
| C60 | Unification / resolution | Logic programming |
| C61 | Constraint solve / propagate | CP / SMT in-language |
| C62 | Backtracking / choice points | Search |
| C63 | Relation / query evaluation | Datalog, SQL execution as lang |
| C64 | Array rank polymorphism | APL/J/Bqn-style |
| C65 | Spreadsheet / cell reactive eval | Demand-driven cells |
| C66 | Dataflow / stream graph fire | Token/actor dataflow |
| C67 | State machine / statechart step | Explicit states as program |
| C68 | Parser combinator / grammar run | Grammar-as-program |
| C69 | Differentiable transform | Gradients through code |
| C70 | Probabilistic sample/observe | PPLs |
| C71 | Bidirectional / put-get | Lenses, parsers+printers |
| C72 | Reversible step / uncompute | Bennett-style, Janus |
| C73 | Hardware signal / clocked process | HDL semantics |
| C74 | Quantum gate / measurement | Quantum PLs |
| C75 | Choreography / multiparty protocol run | Global vs local programs |
| C76 | Incremental / differential update | Propagate changes only |
| C77 | Live / hot code replacement | Change running program |
| C78 | Approximate / speculative execute | Trade accuracy/energy |

---

## 5. Declarations (what is introduced)

| Id | Concept | Notes |
| --- | --- | --- |
| D01 | Value / constant declaration | Named value binding |
| D02 | Variable declaration | Named mutable cell |
| D03 | Function / procedure declaration | Named callable |
| D04 | Type declaration | New type name/shape |
| D05 | Alias declaration | Type or value synonym |
| D06 | Interface / trait / protocol / class | Behavioral contract |
| D07 | Implementation / instance / impl | Witness that type satisfies contract |
| D08 | Extension / orphan enrichment | Add behavior outside original def |
| D09 | Module / package declaration | New namespace unit |
| D10 | Import / use / include | Bring names in |
| D11 | Export / pub / API surface | Expose names out |
| D12 | Macro / syntax rule / template decl | Metaprogram definition |
| D13 | Foreign / extern declaration | Bind to other ABI/language |
| D14 | Test / benchmark / example decl | First-class verification artifacts |
| D15 | Operator declaration | Introduce symbolic ops |
| D16 | Effect / handler declaration | Introduce effect & default handlers |
| D17 | Capability / permission declaration | Authority introduction |
| D18 | Static / thread-local / global storage | Program-lifetime cells |
| D19 | Init / constructor / destructor | Lifecycle declarations |
| D20 | Main / entrypoint declaration | Program start |
| D21 | Conditional compilation / feature decl | Build-time presence |
| D22 | Documentation / contract attachment | Spec bound to declaration |

**Isomorphism hints**

- D01 and D03 are often the same mechanism with different RHS kinds.
- D06 vs D04: contract vs data shape — keep distinct unless language unifies.
- Attributes/annotations are usually *modifiers on declarations*, not a new
  declaration universe.

---

## 6. Effects and world interaction

| Id | Concept | Notes |
| --- | --- | --- |
| E01 | Pure computation | No world effects |
| E02 | Console / logging IO | Human-facing streams |
| E03 | Filesystem | Paths, files, dirs |
| E04 | Network | Sockets, HTTP, etc. |
| E05 | Process / OS / env | Env vars, spawn OS process |
| E06 | Time / clock / timer | Read or wait on time |
| E07 | Randomness / entropy | Non-deterministic bits |
| E08 | Allocation / deallocation | Memory effects |
| E09 | Mutation of shared state | Heap/global mutation |
| E10 | Concurrency schedule nondeterminism | Interleaving |
| E11 | UI / rendering / input devices | Interactive surfaces |
| E12 | Hardware / MMIO / interrupts | Bare metal |
| E13 | Database / durable store | Persistence services |
| E14 | Crypto / key material | Special secrecy effects |
| E15 | Foreign call | Leave the language’s semantic model |
| E16 | Panic / abort / fatal | Unrecoverable termination |
| E17 | Debug / trace / breakpoint | Observability effects |
| E18 | Ambient authority / ambient effects | Capability-less world access |

**Frontier:** privacy budgets; energy budgets; policy/intent engines; sensor
fusion; robot actuation; biological wet-lab devices.

---

## 7. Memory, resources, and lifetimes

| Id | Concept | Notes |
| --- | --- | --- |
| R01 | Stack allocation | Scoped automatic storage |
| R02 | Heap allocation | Dynamic storage |
| R03 | Manual free | Explicit release |
| R04 | Garbage collection | Tracing / automatic reclaim |
| R05 | Reference counting / ARC | Count-based reclaim |
| R06 | Ownership move | Unique transfer |
| R07 | Borrow / shared XOR mutable | Alias-controlled temporary access |
| R08 | Interior mutability | Mut behind shared facade |
| R09 | Arena / region / bump | Bulk lifetime |
| R10 | Pool / slab / custom allocator | Allocation policy |
| R11 | Pinning / immovable | Address stability |
| R12 | RAII / destructor / Drop | End-of-life hooks |
| R13 | Defer / scope guard | Ad-hoc end-of-scope |
| R14 | Finalizer / phantom cleanup | GC-associated cleanup |
| R15 | Memory layout / packing / ABI | Representation control |
| R16 | Alignment / padding | Hardware constraints |
| R17 | Volatile / atomics / fences | Concurrency & hardware visibility |
| R18 | Virtual memory / mmap / pages | OS-backed memory |
| R19 | Stackful vs stackless coroutines | Resource model of suspension |
| R20 | Capability-delimited resources | Resource = unforgeable right |

---

## 8. Abstraction, reuse, and polymorphism

| Id | Concept | Notes |
| --- | --- | --- |
| A01 | Parametric polymorphism | Generics independent of type |
| A02 | Ad-hoc polymorphism | Overloading, typeclasses, traits |
| A03 | Subtype polymorphism | Inclusion + dynamic dispatch |
| A04 | Row / structural polymorphism | Open shapes |
| A05 | Higher-rank / higher-kinded polymorphism | ∀ inside; type constructors |
| A06 | Inheritance / embedding | Reuse via hierarchy or include |
| A07 | Delegation / composition | Forward to components |
| A08 | Mixin / trait alias / role | Composable behavior fragments |
| A09 | Prototype delegation | Runtime parent objects |
| A10 | Interface segregation / facades | Narrow contracts |
| A11 | Specialization / monomorphization | Per-type codegen |
| A12 | Conditional conformance | Impls gated on bounds |
| A13 | Default methods / mixin bodies | Reusable impl fragments |
| A14 | Associated types / type members | Type-level outputs of traits |
| A15 | Implicit parameters / typeclass dictionaries | Passed evidence |
| A16 | Value dependency / const generics | Values in the type/abstraction |

---

## 9. Metaprogramming and staging

| Id | Concept | Notes |
| --- | --- | --- |
| P01 | Textual / syntactic macro | Token/AST rewrite |
| P02 | Procedural / API macro | Host-language macro programs |
| P03 | Template / generic instantiation | Compile-time substitution |
| P04 | Compile-time execution / comptime | Run code while compiling |
| P05 | Partial evaluation / staging | Multi-stage programming |
| P06 | Quasiquote / splice | Code templates with holes |
| P07 | Reflection / introspection | Read program structure at run/compile |
| P08 | Code generation / emit | Write sources or IR |
| P09 | Annotation / attribute / decorator | Structured metadata on forms |
| P10 | Aspect / advice | Cross-cutting injection |
| P11 | Term rewriting / tactic | Proof and AST strategies |
| P12 | Dependent elimination as metaprogram | Proof-relevant computation |
| P13 | Feature flags / cfg | Conditional presence |
| P14 | Derive / schema → impl | Automatic witness synthesis |
| P15 | Homoiconicity | Code ≡ data uniformly |

---

## 10. Modules, builds, and packaging (language-adjacent but foundational)

| Id | Concept | Notes |
| --- | --- | --- |
| U01 | Compilation unit | What the compiler bites |
| U02 | Package / crate / library | Distributable unit |
| U03 | Dependency edge | Requires another unit |
| U04 | Version / compatibility range | Evolution contract |
| U05 | Feature / optional dependency | Conditional graph |
| U06 | Linkage / ABI / dylib | How binaries meet |
| U07 | Plugin / dynamic load | Runtime extension |
| U08 | Workspace / monorepo graph | Multi-package projects |
| U09 | Build task / recipe | How artifacts are produced |
| U10 | Configuration as code | Settings with language semantics |

---

## 11. Equality, ordering, and identity theories

| Id | Concept | Notes |
| --- | --- | --- |
| Q01 | Structural equality | Same shape/contents |
| Q02 | Referential / identity equality | Same location/object |
| Q03 | Observational / extensional equality | Same behavior |
| Q04 | Partial equality / NaN policies | Equality that may fail |
| Q05 | Hashing / content address | Map to digest |
| Q06 | Ordering / comparison | Total/partial orders |
| Q07 | Coherence / lawfulness | Eq/Ord/Hash agreements |
| Q08 | Homotopy / path equality | Higher equalities |

---

## 12. Errors, safety, and undefinedness

| Id | Concept | Notes |
| --- | --- | --- |
| S01 | Type error (static) | Rejected before run |
| S02 | Dynamic type/tag error | Runtime class failure |
| S03 | Effect / capability violation | Unauthorized world act |
| S04 | Resource exhaustion | OOM, stack overflow |
| S05 | Invariant / contract break | Programmer falsehood |
| S06 | Panic / abort | Unwinding or hard stop |
| S07 | Undefined behavior | Semantically unconstrained |
| S08 | Unspecified / implementation-defined | Allowed latitude |
| S09 | Safe subset vs unsafe escape | Audited opt-in danger |
| S10 | Sandbox / isolation boundary | Contained authority |
| S11 | Formal proof obligation | Machine-checked safety |

---

## 13. Human-facing surface dimensions (not semantics, but ontology of UX)

These are not “runtime things,” but they are fundamental *language design
objects* an isomorphism audit must treat.

| Id | Concept | Notes |
| --- | --- | --- |
| H01 | Keyword | Reserved word |
| H02 | Sigil / operator glyph | Symbolic syntax |
| H03 | Literal syntax | How values are written |
| H04 | Layout / indentation / separators | Spatial grammar |
| H05 | Comment / doc comment | Non-executive prose |
| H06 | Diagnostic / error message | Compiler-to-human channel |
| H07 | Formatter / canonical style | Mechanical arrangement |
| H08 | REPL / notebook / explorative loop | Interactive program surface |
| H09 | Debugger / inspector affordance | Runtime comprehension |
| H10 | Beginner default path | Progressive disclosure tier |
| H11 | Expert opt-in path | Explicit power tier |
| H12 | Boilerplate / ceremony | Tokens that don’t teach ontology |
| H13 | Naming convention space | Case, prefix, suffix systems |

---

## 14. Orthogonal axes (compose across families)

Apply these as tags on any member above. Do **not** mint a new family when an
axis explains the difference.

| Axis | Poles / spectrum |
| --- | --- |
| X01 Naming | anonymous ↔ named ↔ path-qualified |
| X02 Mutability | immutable ↔ mutable ↔ interior-mutable |
| X03 Time | runtime ↔ compile-time ↔ both (cross-stage) |
| X04 Purity | pure ↔ effect-delimited ↔ ambient-effectful |
| X05 Evaluation | eager ↔ lazy ↔ non-strict ↔ parallel ↔ speculative |
| X06 Binding site | expression ↔ statement ↔ declaration ↔ pattern |
| X07 Dispatch | static ↔ dynamic ↔ multi ↔ dependent |
| X08 Representation | abstract ↔ layout-explicit |
| X09 Safety | total-safe ↔ partial ↔ unsafe-audited ↔ UB-prone |
| X10 Sync | sync ↔ async/await ↔ callback ↔ evented ↔ blocking |
| X11 Locality | local ↔ shared-memory ↔ distributed |
| X12 Exactness | exact ↔ approximate ↔ probabilistic |
| X13 Specificity | monomorphic ↔ polymorphic ↔ dynamic |
| X14 Visibility | private ↔ module ↔ package ↔ public |
| X15 Explicitness | inferred ↔ annotated ↔ fully explicit |
| X16 Ownership | copied ↔ moved ↔ shared ↔ borrowed |
| X17 Idempotence / replay | once ↔ replayable ↔ reversible |
| X18 Interactivity | batch ↔ REPL ↔ live ↔ hot-swap |
| X19 Audience tier | beginner-default ↔ expert-opt-in |
| X20 Verbosity budget | golf ↔ clear ↔ ceremonious |

---

## 15. Classic isomorphisms (calibration set)

Use these as gold “ohhh” patterns. Prefer discovering new ones over restating.

1. **Named function ≈ named binding of a function value** (Odin procs/consts; Jet fn vs lambda).
2. **Method ≈ function with distinguished receiver** (+ optional dispatch).
3. **Module ≈ record of bindings** (ML structures; JS namespace objects).
4. **Class ≈ type + dictionary of methods** (typeclass/impl bundles).
5. **Interface/trait ≈ set of required operations** (not data layout).
6. **Async fn ≈ sync fn returning a suspended computation value**.
7. **Iterator / stream / generator ≈ suspended producer of a sequence**.
8. **Null / Option / nullable type ≈ sum with a missing case**.
9. **Exceptions ≈ effect or sum + non-local control** (don’t pretend otherwise).
10. **Array indexing ≈ function from index to element** (with effects/partiality).
11. **Map/filter/reduce ≈ (parallelizable) loops with structured intent**.
12. **String interpolation ≈ formatting function application sugar**.
13. **List comprehension ≈ nested loops + filter + collect**.
14. **Pipe `|>` ≈ reverse application**.
15. **Defer ≈ scope-limited resource/effect registration**.
16. **RAII type ≈ value whose type carries end-of-life effect**.
17. **Reference ≈ value that denotes a location** (capability-ish).
18. **Enum without payload ≈ union of unit types**.
19. **Tuple ≈ anonymous struct**; **struct ≈ named tuple**.
20. **Pattern match ≈ nested conditionals on tags + bindings**.
21. **Import ≈ binding introduction from another module**.
22. **Macro ≈ function on syntax** (staged).
23. **Type annotation ≈ proof/evidence attachment to an expression**.
24. **Cast ≈ explicit assertion of representation/type change**.
25. **Channel ≈ queue value with send/receive effects**.
26. **Actor ≈ state machine + mailbox**.
27. **Object with identity ≈ record + stable location**.
28. **Prototype ≈ dynamic delegation chain on records**.
29. **Regex ≈ specialized parser value**.
30. **SQL query embed ≈ comprehension over relations**.
31. **GPU kernel ≈ data-parallel function with memory hierarchy effects**.
32. **Comptime code ≈ same language at X03=compile-time**.
33. **Test declaration ≈ function + runner metadata**.
34. **Feature flag ≈ conditional presence in the module graph**.
35. **Unsafe block ≈ locally weakened S09 with audit obligation**.

---

## 16. Dual-facet length & power targets (audit scoring aids)

When mapping Jet forms to this ontology, score each relevant family against:

| Lens | Pass condition |
| --- | --- |
| **Clarity** | A careful newcomer can say what the form *is* in one sentence |
| **Isomorphism** | Shared ontology ⇒ shared spelling family; no false rhyme |
| **Exploratory density** | Typical Python analysis/scripting tasks are ≈ as short or shorter in Jet (stdlib power allowed) |
| **Systems expressiveness** | Zig/Rust/Odin/C/C++-class control remains expressible without a second language |
| **Ceremony tax** | Extra tokens buy safety, clarity, or real distinction — not historical accident |
| **Tiering** | Beginner default hides footguns; expert opt-in exposes full control (philosophy C1/I1) |

Conflict rule for this audit: **clarity beats mere consistency**. Consistency
wins only when it also teaches the ontology (creates the “ohhh”). Compression
wins only when it preserves or improves clarity.

---

## 17. Extension protocol

When you find a concept that doesn’t fit:

1. Try tagging an existing member with an **X-axis**.
2. Else add a member under the closest family with a new `Id`.
3. Else add a family in this file and cite why families 0–13 failed.
4. Never create a Jet-only parallel ontology; extend this one.
