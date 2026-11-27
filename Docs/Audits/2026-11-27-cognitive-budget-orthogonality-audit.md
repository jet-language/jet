# Cognitive-Budget, 3-Ring Critique, and Pairwise-Orthogonality Audit

Date: 2026-11-27. Scope: read-only audit of the live `master` checkout. It
covers the syntax registries, AST, Prelude, spec, Sema special cases, the
authoring-friction research doc, and Tower cards #7045–#7059 (read-only
`card show`).

Baseline: this audit evaluates the **post-authoring-friction surface**. It
assumes everything ratified in #7042–#7059 lands as written, and it does not
re-recommend any of it:

- `@T` retirement and direct-field `Shared`/`Weak`
- `CheckedText`→`DSL`
- no `->` on `if`/`loop`
- zero postfix markers
- deleted `f`/`i`/`j`/`k` suffixes
- `!`/`!!` vs `?`/`??`
- `next`→`skip`
- structural Copy
- effect bundles
- COW/SSO `String`
- honest `Pool`/`ID`
- the Core-table collapse

Every remaining finding is something those cards leave in place.

Method notes:

- Counts are taken from the live enums and registries cited.
- Peer numbers are approximate and drawn from the public language references.
  They are marked `~`.
- Line numbers refer to the files as read in this session.
- Five constructs named in the brief **do not exist in live code**: `#EveryField`,
  `KeyPath<T,V>`, `#Dir`, `#Embed` and `Link<T>`. Each scored 0 hits across
  `Compiler/JetFoundation/Data/Syntax/`, `Compiler/Prelude/` and
  `Docs/Spec/spec.md`. They are not audited. The live analogue of `KeyPath` is
  the prelude `FieldRef<Model, T>` (`Docs/Spec/spec.md:4371-4385`). The live
  analogue of `#Embed` is `embed_file`/`embed_bytes` (`Docs/Spec/spec.md:1572`).

---

## Section 1 — Exact live surface census

### 1.1 Grammar and AST

| Construct | Live count | Source |
|---|---|---|
| Keywords (`KEYWORD_NAMES`) | **50** | `Compiler/JetFoundation/Source/Syntax/Spellings.jet:53-104` |
| Built-in type names (`TYPE_NAMES`) | **47** | same file |
| Lexical ledger rows (`LEXICAL_LEDGER`) | **~90** | `Compiler/JetFoundation/Data/Syntax/Spellings.jet:11-102` |
| Language rewrites (`LANGUAGE_REWRITES`) | **11** | `Compiler/JetFoundation/Data/Syntax/Spellings.jet:114-126` |
| `OP_*` / `SIGIL_*` constants | **~49 / 8** | `Compiler/JetFoundation/Data/Syntax/MathLayout.jet` |
| Binary operators (`BinOp`) | **22** | `Compiler/JetFoundation/Source/AST/Expressions.jet:197-220` |
| Unary operators (`UnOp`) | **3** (Neg, Not, Complement) | same |
| Top-level `Item` kinds | **24** (+ `StateDecl`, parsed but not an Item) | `Compiler/JetFoundation/Source/AST/Items.jet:3-28`, `:690` |
| `Expr` kinds | **51** (mirrored in `ASTExprKind`) | `Expressions.jet:11-63`, `Nodes.jet:43-95` |
| `Stmt` kinds | **36**, including 7 loop-control jumps and ~15 marked-block kinds | `Statements.jet:3-40` |
| `Pattern` kinds | **12** + 7 `PatSlot` + 3 `StructPatField` | `Patterns.jet:4-17` |
| Semantic `Type` kinds | **24** | `Compiler/JetFoundation/Source/Types/Types.jet:6-31` |

Syntax registry constants per file under `Compiler/JetFoundation/Data/Syntax/`:

| File | Constants |
|---|---|
| CoreSurface | 282 |
| MathLayout | 270 |
| EffectsSurface | 198 |
| JetpackConfig | 193 |
| PackageFiles | 139 |
| Predicates | 48 |
| Spellings | 43 |
| Acronyms | 3 |
| Highlights | 3 |
| Markers | 2 |
| CoreCalls | 1 |
| Dictionary | 1 |

Other surface counts:

- **Markers:** **83** built-in `#Markers`. Each is a struct with
  `impl Marker{sites:…}` in `Compiler/Prelude/Markers.jet`. 4 of them are
  TypeUse value tags: `Input`, `PII`, `Secret`, `Credential`. Users can define
  more markers (struct + `impl Marker`, lowered to `Item.MarkerDecl`,
  `Items.jet:309-334`).
- **Prelude traits:** **26** owned in `Compiler/Prelude/Traits.jet`, plus
  `Effect`, `Marker` and `Neg` (from #7044), plus `Describe` from the Core
  prelude.
- **Trait metadata flags:** 16 (`local`, `bare_bound`, `raw_return`,
  `any_receiver`, `commutative`, `codec`, `conversion`, `derived`,
  `checked_text`, `auto_derive`, `auto_derive_family`, `bundle`,
  `through_elements`, `requires`, `shares`, `declared_in`). These are per-trait
  compiler switches, so each one is a special case by construction.

The **24 `Item` kinds** are:

- Func, Struct, Enum, Distinct, TypeAlias, UnitFamily
- Trait, Tag, EffectDecl, Impl, Const, Test
- ExternRust, Module, CModule, CodeModule, ErrorConv, Migration
- ProtocolDecl, TemplateLoop, GenericModule, ModuleAlias, MarkerDecl, FactDecl

The **36 `Stmt` kinds** fall into these groups:

- Plain statements: Expr, Val, Assign, Return.
- Loops: While, For, Loop, CountedLoop.
- Control: Switch.
- Jumps: Break, BreakValue, Continue, BreakLabel, BreakLabelValue,
  ContinueLabel.
- Bespoke marked blocks: Unsafe, Impure, Reactive, Shield, Region, Policy,
  Layout, ContextBlock, Live, AssumeDet, Transact.
- Generic marked block: Switched.
- Others: TaskGroup, AuthorityScope, ComptimeIf, ComptimeSwitch, ComptimeBlock,
  Yield, ScopeMember, DeferClose, Defer.

The **24 semantic `Type` kinds** include:

- `Shared` as a built-in type constructor rather than a library type.
- Separate `IntN` and `Float32`.
- `InlineRange`, `Tagged`, `Union`, `Case`, `Quantity`, `Measure`.

### 1.2 Domain surfaces (post #7042–#7059)

**Operators.** The division family has **6 spellings**:

- `/`
- `/%` (floor division)
- `%` (floored modulo)
- `%%` (truncated remainder)
- `div_euclid` and `rem_euclid`

Evidence: `MathLayout.jet:409-421` and `Spellings.jet:244-248`. In addition,
`~` is both XOR and complement (`MathLayout.jet:437`), and `&~` clears bits.

**Ownership and memory: ~26 user concepts** after #7053 and #7059.

- Access marks: `^`, `^^`, the changeable-binding `^name`.
- Copy and move: `.copy()`, `.move()`, `#MoveOnly`, `#SingleUse`.
- Cleanup: `Close` trait, `#Close` marker, `defer`, `DeferClose`, `.drop("…")`.
- Sharing: `Shared<T>`, `Shared.Weak`, `SharedGuard` (`guard_read`/`guard_edit`
  plus `try_*` and `*_within`), `Condition`.
- Cells: `Cell<T>` (get/set/replace/get_or_set/read/edit/guard_read/guard_edit),
  `CellReadGuard`, `CellEditGuard`, `Atomic<T>`.
- Views: `View`, `ViewMut`, `View<str>`, `.in_place()`, `from` provenance.
- Handles: `Pool<T>`/`ID<T>` (an arena), `Rows<T>`/`Row<T>`.
- Allocators: `core.mem` `Pool`/`Arena`/`Bump`/`Fixed`, `#Region`.
- Other: `#Local`, `freeze`, `#Transact` + `Rollback`, `#Policy(copies:)`,
  `#Policy(gc)`/`Mem.Gc`.

**Effects: ~22 machine effects** in `Compiler/Prelude/Effects.jet`, the largest
of which is `FFI` with 24 languages. Beyond the effects themselves:

- 3 Core service effects: DB, Log, Secret.
- 5 `Meta.*` effects.
- 4 deny-only roots.
- 4 effect kinds: machine, service, library, package-kind (`parent:`).
- Effect values (`-[Bucket{…}]>`) and effect scopes (`FS.Read("templates/")`).
- 8 rungs: block/lambda, function, trait, type, module, file head, package
  `authority`, run flags.
- 7 spellings: `-[…]>`, `-[]>`, `-[via f]>`, `#FX(...)`, `#FX: (...)`,
  `#FX(...) {}` and bare `#FX`.
- `Stmt.AuthorityScope`.
- 4 operation-fact axes: `EffectAccess{Reads,Adds,Replaces}`, `irreversible`,
  `#Repeatable`, `#Replayable` (`Docs/Spec/spec.md:3125-3158`).

**Concurrency: ~24 concepts.**

- Spawning: `task f()`, `task {}`, `task ^^name {}`, `task.group`, `Group`
  parameters.
- Combinators: `task.all`, `task.race`, `task.any`, `task.timeout`.
- Communication: `Task<T>`, channel `Sender`/`Receiver`, select-as-`if`-table
  with `after`, `Shared`/`Condition`.
- Parallel collections: `para_map`, `para_filter`, `para_partition`,
  `para_fold`.
- Streams: `yield`/`Stream`.
- Events: `Event`, `Hook`, `DecisionHook`, `AsyncEvent`, `Subscription`,
  `EventScope`, `EventTrace`, `DispatchReport`.
- Scope markers: `#Deadline`, `#Shield`, `#Frame`, `#Context`/`context.*`,
  `#ContextKey`.
- Jobs: `#Job`, `TaskFailure`.

**Metaprogramming and build: ~30 forms.**

- Comptime blocks: `#run`, `#run {}`, `#run fn`, `#run use`, `#if`, `#loop`.
- Code generation: `#{{name}}` paste, `#code {}`, `<: :>` fence, `meta.add`,
  `add_struct`, `find_type`, `rewrite_body`, `allow_rewrite`, `events`,
  `stages`.
- Compiler access: `b.compiler(output)`, `TypeId{T}`, `Type{T}`, `type_of`,
  `compiler.query`, `x.reflect()`.
- Fact roots: `#build`, `#package`, `#phase`, `#program`, `#here`, `#caller`.
- Embedding: `embed_file`, `embed_bytes`, `find`.
- Other: computed type aliases, `Grammar{"""…"""}` heads,
  `FieldRef<M,T>`, `check` rules, `FactDecl`, `#Recorded`.
- Module forms: generic modules, code modules.

### 1.3 Peer comparison (Jet live vs approximate peers)

| Axis | Jet (live) | Go | Rust | Swift |
|---|---|---|---|---|
| Reserved keywords | 50 | 25 | ~38 strict + ~14 reserved | ~95 incl. contextual |
| Attribute/marker vocabulary | **83** built-in + user markers | ~10 `//go:` directives, struct tags | ~50 built-in attrs + derive/proc macros | ~45 `@` attributes |
| Top-level declaration kinds | **24** | 5 (`import`, `const`, `var`, `type`, `func`) | ~17 `ItemKind` | ~14 |
| Expression node kinds | **51** | ~19 | ~44 `ExprKind` | ~40 |
| Statement node kinds | **36** | ~21 | ~6 (expression-oriented) | ~20 |
| Pattern kinds | 12 (+7 slot kinds) | 0 | ~17 | ~8 |
| Division/remainder spellings | **6** | 2 | 2 ops + 4 methods | 2 ops + methods |
| Ownership/memory concepts a reader meets | **~26** | ~4 | ~15 (`&`, `&mut`, lifetimes, Box/Rc/Arc/Weak/Cell/RefCell/Mutex/RwLock/Pin, Copy/Clone/Drop) | ~8 |
| Checked effects | ~22 effects × 8 rungs × 7 spellings | none | ad-hoc (`unsafe`, `async`, `const`) | `throws`, `async`, isolation, Sendable |
| Concurrency concepts | **~24** | ~5 (`go`, `chan`, `select`, `sync`, `context`) | ~12 (async/await, Future, Pin, Send/Sync, executors) | ~12 (async/await, Task, TaskGroup, actors, Sendable, isolation) |
| Metaprogramming forms | **~30** | ~3 (`go generate`, build tags, reflect) | ~8 (macro_rules, 3 proc-macro kinds, const fn, build.rs, cfg) | ~10 (macro roles, result builders, property wrappers) |
| Spec length | 8550 lines (`Docs/Spec/spec.md`) | ~8k lines incl. examples | Reference ~30k lines | ~TSPL book scale |

**Honest reading of the table:**

- **Where Jet is simpler than Rust:**
  - No lifetime parameters in signatures.
  - No `Pin`.
  - No trait-object vs generic duplication at the surface.
  - No macro_rules/proc-macro split; `#run` and `DSL` are one tier.
  - Inferred structural Copy (#7053).
  - Effects replace Rust's ad-hoc `unsafe`/`async`/`const` colors with one row.
- **Where Jet already equals or exceeds Rust/Swift:**
  - Declaration kinds (24 vs ~17).
  - Statement kinds (36 vs ~6).
  - Attribute vocabulary (83 vs ~50).
  - Memory-sharing concepts.
  - Concurrency vocabulary.
  - Effect granularity.
- **Scala/C++ territory:** the per-package dialect switches (§2.3 D-leaks) and
  the implicit conversion web (§3 O13).
- **Go-level simplicity:** out of reach on raw counts. It is reachable on the
  *Ring-1 path*, but only if Rings are enforced by the compiler, not by docs
  (§2).

The **compiler-side** surface is larger than the user-side surface:

- **Item dispatch:** 75 files under `Compiler/JetSema/Source` match `.ErrorConv(`.
  That is a proxy for exhaustive 24-way `Item` matches, for example
  `Sema/CheckProgram.jet:67,136`, `Sema/Comparability.jet:178-570` and
  `Effects/Checks.jet:2967`.
- **Hardcoded type names:** Sema hardcodes Core type names by string. Counts of
  `== "<Name>"` across the 236 Sema files (221,084 lines):

| Name | Matches |
|---|---|
| `Err` | 29 |
| `View` | 28 |
| `ViewMut` | 28 |
| `String` | 15 |
| `Iter` | 15 |
| `Dyn` | 11 |
| `Cell` | 10 |
| `Pool` | 9 |
| `SharedGuard` | 7 |
| `ID` | 5 |
| `Shared` | 4 |
| `Task` | 4 |
| `Group` | 3 |

---

## Section 2 — Adversarial critique of the 3-ring progressive-disclosure model

The intended model, per `Docs/Spec/philosophy.md`:

- Pillar 1: implicit beginner magic and explicit expert control.
- Pillar 4: ladders, not cliffs.
- Design priority 2: reveal complexity when the program needs it.

In ring terms: Ring 1 is the defaults, Ring 2 is opt-in markers and explicit
controls, and Ring 3 is `#Unsafe`, comptime/meta, policy and FFI.

### 2.1 History: progressive disclosure has failed exactly where rings leak

**C++ ("you don't pay for what you don't use").**

- Ring separation lived in style guides (Google C++ style, Embedded C++,
  `-fno-exceptions`, `-fno-rtti`), not in the compiler.
- The result was mutually incompatible dialects. Libraries written for one
  could not be used in another.
- Template error novels leaked Ring-3 machinery into Ring-1 code: one wrong
  `std::map` key type yields a page of allocator/comparator instantiation.
- Concepts (C++20) were the admission that diagnostics must speak the
  vocabulary of the caller.

**Scala 2.**

- `List.map` had the signature `map[B, That](f: A => B)(implicit bf:
  CanBuildFrom[List[A], B, That]): That`.
- Scaladoc had to invent "use case" fake signatures to hide it. The ring
  leaked through *library signatures*.
- 2.13 deleted `CanBuildFrom` outright: the fix was deletion, not better
  hiding.
- Implicit conversions produced action-at-a-distance that no ring discipline
  contained.

**Scala 3.**

- Split one `implicit` into `given`/`using`/`extension`/`Conversion`. That is
  more orthogonal, but more words.
- Shipped two surface syntaxes (braces and significant indentation).
- The 2→3 ecosystem split showed that dialect switches fragment communities
  even when each dialect is coherent.

**Swift** (progressive disclosure is an explicit Swift design goal).

- `some`/`any` existential errors, `where`-clause-heavy stdlib signatures, and
  result-builder "expression too complex to type-check in reasonable time"
  reach beginners writing SwiftUI.
- Swift 6 strict concurrency made `Sendable`/isolation diagnostics appear in
  apps that never wrote `async`. A Ring-3 concept surfaced through
  *errors in Ring-1 code*.

**Rust.**

- Lifetimes are nominally advanced, but E0106/borrowck errors are among the
  first a beginner sees.
- Async leaks `Send` bounds and `Pin` into ordinary handlers ("future cannot be
  sent between threads safely").
- Editions are the one partly successful dialect mechanism, because they are
  *per-crate, mechanically migratable, and interoperable*.

**Lesson:** rings hold only where the compiler guarantees four properties:

- Ring-1 code never *sees* Ring-3 vocabulary, in errors or signatures.
- Moving up a ring is mechanical (no rewrite cliff).
- Ring switches never change the meaning of existing code (no dialects).
- Redundant mechanisms are deleted rather than relegated to a higher ring.

### 2.2 Where Jet's rings leak today (live evidence)

**E-leaks: errors expose Ring-2/3 concepts in Ring-1 code.**

- **E1. Closure captures.** A beginner closure that reads a local after the
  local changes reports E0204, because non-escaping lambdas copy read
  captures at creation but borrow written ones (`Docs/Spec/spec.md:1953-1962`).
  The fix text requires understanding capture modes.
- **E2. Fallibility in function types.** A function value that fails differently
  inside a list reports E0638 ("`[fn(Int) -> Int]` and `[fn(Int) -> Int Err!]`
  are different value types", `spec.md:1916-1929`). Meanwhile, the same
  function type as a parameter silently widens. The beginner meets
  failure-rail variance.
- **E3. Function values and access marks.** Passing a named function with a
  `^`/`^^` parameter as a value reports E0227 (`spec.md:1884-1890`).
- **E4. Pure rows reject non-effects.** `-[]>` rejects module-state reads with
  E3401 even though such reads "are not effects" (`spec.md:3264-3284`). The
  beginner learns an exception to the effect model before learning the model.
- **E5. Sendability.** E1101/E1102 and the group-borrow rules surface in the
  first `task.group` program (`spec.md:5186-5204`). This is the Swift-6 failure
  mode.

**S-leaks: Core signatures expose Ring-2/3 types.**

- **S1. Constructor types.** Enum payload constructors take types like
  `fn(*, left: Int, right: Int) -[]> Message` and `fn(^^FileWriter) -> Sink`
  (`spec.md:1892-1900`).
- **S2. Ring-2 machinery in common call sites.** `FieldRef<M, T>`, `ViewMut`,
  `-[via f]>`, `![FieldError]`, `SharedGuard` and `*_within` appear on
  everyday Core entry points: forms validation (`spec.md:4371-4385`),
  `Shared` access (`spec.md:2675-2752`), callbacks.
- **S3. Sema privileges Core types.** Sema string-matches Core type names to
  grant powers that user types cannot earn. Examples:
  - `Sema/Calls/SharedOwner.jet:524`: `name == "Pool" … (method == "add" || "remove" || "ids")`.
  - `Sema/Ownership/Crossings.jet:522`: `.ThreadConfined(name) -> name == "Cell" || …`.
  - `Sema/Scopes.jet:3491`: `"Cell" || "Atom" || "Mutex"`.
  - `Sema/Ownership/Copy.jet:1396`: `"SharedGuard" || "ProcessChild" || "TempDir"`.
  - `Sema/Calls/Plugin.jet:97` and `Sema/Expressions/Dyn.jet:141`: `"Dyn" || "JSON" || "TOML" || "YAML" || "CSV"`.

  This violates philosophy Pillar 5 ("no names only Core may call, no
  diagnostics or optimizations that only Core can earn"). A third-party library
  that wraps these types cannot present a Ring-1 signature with the same
  guarantees.

**C-leaks: refactoring cliffs.**

- **C1. Lambda escape flips capture semantics.** Moving a lambda from local
  direct-call to "stored or passed" flips read-capture from copy-at-creation
  to clone/move, and write-capture from borrow to "own copy". The same body
  changes meaning (`spec.md:1955-1974`).
- **C2. Named functions become non-values.** Adding a `^` parameter to a named
  function silently turns every `xs.map(f)` use into E0227.
- **C3. Struct to module.** Promoting a static-only struct to a module is
  pushed by lint L0309 (`spec.md:1058-1062`). It is a different declaration
  kind with a different value story: a module is a `Module` record value
  (`spec.md:4336-4348`), and Core modules have none (E0629).
- **C4. Generic types to generic modules.** Generic type parameters use
  `<N: Int>`. Generic modules take value parameters in a *separate* `(capacity:
  Int)` list (`spec.md:4435-4442`). Lifting a generic struct into a generic
  module rewrites the parameter list.
- **C5. Error conversion: two forms.** The `impl A -> B` arrow
  (`Item.ErrorConv`, `Compiler/JetParser/Source/Parser/Items.jet:3303-3319`)
  vs `impl B.From(A)`. Code written against one form does not compose with
  generic code bounded on `From`.

**D-leaks: dialects.** Package and file policy switches change the meaning of
unchanged code:

- **D1. Collector tier.** `#Policy(gc)` / `Mem.Gc` enable an opt-in tracing
  collector (`spec.md:2873`, `:4145-4148`; `Compiler/Prelude/Effects.jet:130-138`).
- **D2. Copy law.** `#Policy(copies: .Explicit)` changes the copy law.
- **D3. Overflow.** `#Overflow(.Wrapping)` changes `+`.
- **D4. Auto-derive.** `policy: { lints: { deny: [auto_derive] } }`, `#!Debug`
  and `#Debug: false` change which traits exist.
- **D5. Prelude.** `#NoPrelude` changes the name environment.
- **D6. Failure policy.** The spec still teaches `#Policy(failures: .Explicit)`
  (`spec.md:2148`), stale against #7047.
- **D7. Policy keys.** 17 package policy keys (`spec.md:3897-3926`).

Copy-pasted code between two packages can change behavior (D2, D3) or stop
compiling (D4, D5). This is the C++/Scala dialect failure.

**V-leaks: vocabulary.**

- `it` names the `if` subject and the loop item. A bare `.name` inside a loop
  reads `it.name`, but the same spelling where a `FieldRef` is expected names a
  field. The clash is resolved by a special diagnostic, E-SUBJECT-CALL
  (`spec.md:4381-4384`).
- `<>` is both the pipe value and the hole-table hole
  (`Compiler/JetFoundation/Data/Syntax/Spellings.jet:73`).

### 2.3 Verdict

Progressive disclosure is **sound as a presentation strategy and unsound as a
complexity budget**. It works in Jet only where four invariants hold. Adopt
them as checkable laws (proposed below and on #7049):

1. **Ring-closed diagnostics.** Every diagnostic reachable from code that uses
   no Ring-2/3 spelling must state what, why and fix in Ring-1 vocabulary. This
   can be gated: tag each diagnostic code with the ring of the concepts its
   text names, and fail the snapshot suite if a Ring-1 corpus triggers a
   higher-ring code.
2. **Signature monotonicity.** A Core public function callable from Ring 1 may
   expose only Ring-1 types in its signature. Higher-ring parameters must be
   defaulted, labeled and optional. And no Core type may gain a capability by
   name in Sema (Pillar 5).
3. **Tighten-only policies.** A package, file or block switch may add errors or
   restrict authority. It may never change the meaning of code that compiles
   both ways. This rules out D1, D2 and D3 as switches. Express them as
   explicit types or ops instead, e.g. `Wrapping<Int>` or `wrapping(...)`.
4. **No-cliff rungs.** Every rung transition is either meaning-preserving or
   offered by `jet fix` as a mechanical rewrite. C1–C4 fail this today.

What **must be deleted or unified rather than swept into Ring 2/3**:

- Duplicate spellings for one meaning:
  - error conversion
  - 4 method sites
  - 6 division spellings
  - 7 effect spellings
  - text traits
- Compiler-only Item kinds that the surface already expresses as `struct + impl
  Role`.
- Semantics-changing dialect switches.
- Capture-mode and failure-variance special cases.

Relegating any of these to a higher ring keeps their full cost: users still
meet them through errors, docs and other people's code.

---

## Section 3 — Catalog of overlapping, duplicate and bloated mechanisms

Each entry follows the same format: ID & Name, then **Evidence**, **Cost**,
**Proposal** and **Trade-offs**. "Post-card" means what remains after
#7042–#7059.

### A. Declarations (`Item` kinds)

Classification of the 24 `Item` kinds:

- **Irreducible core (7):** Func, Struct, Enum, Trait, Impl, Const, Module.
- **Reducible to `struct`/`impl Role`, comptime or library (13):** Distinct,
  TypeAlias, UnitFamily, Tag, EffectDecl, MarkerDecl, FactDecl, ErrorConv,
  Migration, ProtocolDecl, TemplateLoop, Test, ModuleAlias.
- **Foreign-boundary (2):** ExternRust, CModule.
- **Module variants (2):** CodeModule, GenericModule.

**O1 — Role-declaration Items re-split from one surface form**

- **Evidence:** The surface already declares markers, effects, tags and facts
  as a `struct` plus `impl Marker`/`impl Effect`/a tag struct/`value.fact`. The
  spec states that "`effect` is not a declaration word" (`spec.md:3170`). The
  parser then re-emits separate Items:
  - `Item.EffectDecl` companions (`Compiler/JetParser/Source/Parser/Declarations/Effects.jet:35-42`)
  - `Item.Tag` via `parser_tag_struct` (`Parser/Items.jet:325`, `Declarations/Markers.jet:538`)
  - `Item.FactDecl` from `value.fact` (`Parser/Items.jet:322`)
  - `Item.MarkerDecl` with bespoke group/derive/check/pairs fields (`Items.jet:309-334`)
  - `Item.UnitFamily`
- **Cost:** The user cost is low; the surface is already unified. The compiler
  cost is high. Every Sema pass must handle 24 Item kinds (75 files match
  `.ErrorConv(`), and each role carries its own field layout. Adding a new
  role means adding a new Item kind, which is the opposite of 1-in-1-out.
- **Proposal:** Make the AST match the surface. Keep one `Item.Struct` plus
  `Item.Impl`, and derive "is a marker/effect/tag/fact/unit family" as a
  *query* over `impl Role` facts. Role-specific data (sites, parent, access)
  comes from the `impl` body's typed fields, which is how `impl Marker{sites:…}`
  already reads in `Compiler/Prelude/Markers.jet`. This collapses 5 Item kinds.
- **Trade-offs:** Role facts must be resolved before passes that need them,
  which means one early registration pass. No user-visible power changes.
  Internal only, so no ballot. It refines #7049 (composition laws) and #7056
  (parallel tables).

**O2 — Error conversion: `impl A -> B` vs `From`**

- **Evidence:**
  - `Item.ErrorConv` (parser `Parser/Items.jet:3303-3319`) is used widely:
    `Compiler/Prelude/Errors.jet:99-143` (`impl AllocError -> Err`),
    `Core/Crypto/*.jet`, `Core/DB/DB.jet:206`,
    `Examples/Apps/Crewboard/Logic/model.jet:62`.
  - The spec calls `impl Target.From(Source)` "the one declared rail"
    (`spec.md:2082-2102`).
  - Two more conversion paths exist: `#Error` auto-lift (`spec.md:2104-2118`)
    and union widening (`spec.md:2133-2138`).
- **Cost:** Two ways to declare the same conversion, plus two implicit ones. A
  reader cannot tell which one fires at a `!` site. Generic code bounded on
  `From` misses `->` declarations.
- **Proposal:** Delete `Item.ErrorConv` and the `impl A -> B` spelling. Have
  `jet fix` rewrite each one to `impl B.From(A)`. `!` propagation consults
  `From` only. Keep auto-lift as the derived `From` impl that `#Error`
  generates, so it is one mechanism with a generated instance.
- **Trade-offs:** `impl A -> B` is terser. That is a loss of ~10 characters
  per declaration, not of power. **Ballot** (syntax deletion); adjacent to
  #7047.

**O3 — `migration` item with 5 dedicated verbs**

- **Evidence:** `Item.Migration`. The keywords `migration`, `rename`, `add`,
  `remove`, `change` and `via` are in `KEYWORD_NAMES` (`Spellings.jet:53-104`)
  and the syntax is in `spec.md:1576-1624`.
- **Cost:** 6 of the 50 reserved keywords serve one niche (schema evolution).
  `add`/`remove`/`change` are common identifiers, and reserving them taxes
  every program.
- **Proposal:** Express migrations as a `DSL` (`#7046` unification) or as typed
  data, e.g. `migrate: Migration<V1, V2>{ rename: [.old -> .new], via: fn … }`,
  checked by `FieldRef` (`spec.md:4371`). Unreserve the 5 verbs.
- **Trade-offs:** Error messages for migrations become DSL diagnostics, which
  #7046 already requires to be first-class. **Ballot**; refines #7046.

**O4 — `protocol` with `client`/`server`, plus typestate `state {}`**

- **Evidence:**
  - `Item.ProtocolDecl` (`Items.jet:651-671`) and the `protocol`, `client` and
    `server` keywords.
  - Typestate `state { A -> B }` inside struct/enum (`Spellings.jet:20`); it is
    `Stmt`-adjacent, and `StateDecl` is parsed but not an Item (`Items.jet:690`,
    `Parser/Items.jet:246-256`).
  - The `#State` marker is still in `Compiler/Prelude/Markers.jet`, and the
    `State` keyword is still in `KEYWORD_NAMES`.
- **Cost:** Three spellings for "an object with allowed state transitions": the
  protocol, the typestate block and the marker. Three more reserved words.
- **Proposal:** One typestate mechanism. A protocol is a pair of typestates
  generated by a library `DSL`/`#run`. Delete the `client`/`server` keywords
  and the residual `#State` marker or keyword, whichever is not the survivor.
- **Trade-offs:** Session-type checking quality must be preserved in the
  library. That requires #7046's diagnostic parity. **Ballot**.

**O5 — Module zoo**

- **Evidence:**
  - `Item.Module` (configuration modules with contributions, `spec.md:4470+`)
    vs `Item.CodeModule` (`spec.md:4280-4298`). Both are spelled `module`.
  - `Item.GenericModule` (`spec.md:4418-4468`), `Item.ModuleAlias`,
    `Item.CModule`, `Item.ExternRust`.
  - A static-only struct is pushed to a module by L0309 (`spec.md:1058-1062`).
  - A module is a `Module` record *value*, except Core modules (E0629,
    `spec.md:4336-4348`).
- **Cost:** One keyword covers 2 meanings and 4 Item kinds. Struct-vs-module is
  a judgement call that a lint polices. Generic modules duplicate generic types
  with a different parameter grammar (T10).
- **Proposal:**
  - One `Item.Module` with optional type/value parameters, so `GenericModule`
    is a module with parameters.
  - `ModuleAlias` is an ordinary binding of a `Module` value, since modules are
    already values.
  - Configuration contributions are typed fields of the `Module` record, which
    the spec already supports with field lines at `spec.md:4340-4343`.
  - Fold `CModule`/`ExternRust` into one `#Import(lang)` foreign-module form.
- **Trade-offs:** Core modules must gain a `Module` record to remove E0629.
  Mostly internal; a **ballot** only for any changed spelling. Refines #7056.

**O6 — `#loop` (TemplateLoop) vs `#run` + `meta.add` vs `DSL`**

- **Evidence:** `Item.TemplateLoop`; `#loop`, `#run`, `meta.add`, `#code {}`,
  `#{{name}}` paste (`Spellings.jet:33-65`).
- **Cost:** Three code-generation tiers that reach the same result.
- **Proposal:** Make `#loop` sugar that lowers to `#run { for … meta.add(#code{…}) }`
  in the parser, or delete it. Delete `Item.TemplateLoop`.
- **Trade-offs:** A template loop is more readable for the simple case. Keep
  the sugar only if a two-program cell shows a real readability win.
  **Ballot**.

**O7 — Type-naming forms**

- **Evidence:**
  - `Item.Distinct` with range/quantity (`DistinctDef.quantity/range`).
  - `Item.TypeAlias`, computed type aliases (`Spellings.jet:33-65`),
    `Item.UnitFamily` with the `#UnitFamily` marker.
  - Semantic `Quantity`/`Measure` and `IntN`/`InlineRange` types
    (`Types.jet:6-31`).
- **Cost:** Five ways to name a type, and a unit family is spelled both as an
  Item and as a marker.
- **Proposal:** Keep `distinct` (new nominal type) and alias (same type).
  Express unit families as `distinct` plus a library `Unit` trait, and delete
  `Item.UnitFamily` and either the marker or the item. Computed aliases stay
  as comptime aliases (one alias form).
- **Trade-offs:** Unit algebra is checked by a trait instead of a bespoke
  checker, which needs comptime evaluation of dimensions. That is already
  available. **Ballot**.

**O8 — Four method declaration sites**

- **Evidence:** Methods can be declared in the type body, in `impl Type {}`, as
  `Type.method: fn`, or in a trait impl in the body or `impl Type.Trait`
  (`spec.md:1031-1083`).
- **Cost:** "Where is this method?" has 4 answers. Formatters and agents must
  support all of them.
- **Proposal:** Two sites. Inherent methods go in the type body or in
  `impl Type {}` (needed for cross-file extension). Trait methods go in
  `impl Type.Trait {}`. Delete the `Type.method: fn` free form via `jet fix`.
- **Trade-offs:** One-liners for small types get one brace level deeper.
  **Ballot**.

**O9 — Construction forms**

- **Evidence:** `Type{…}`, inferred `{…}`, `Type.new(…)`, `.new(…)`,
  `Type.Variant{label:}`, `Val(x)`, `Type{expr}` conversion/ascription (#7051),
  and `From`'s `Target{value}` (`Compiler/Prelude/Traits.jet:174`).
- **Cost:** `Type{x}` means literal, conversion and ascription. A reader must
  know whether `Type` has a single field or a `From` impl to parse the meaning.
- **Proposal:** Keep 2 meanings: literal `Type{field: …}` and conversion
  `Type{expr}`, with conversion only via `From`. Make `.new` an ordinary static
  method with no compiler meaning. #7051 is already mid-flight; this sharpens
  its acceptance terms.
- **Trade-offs:** None in power. Refines #7051.

### B. Traits, derive, validation, conversion

**O10 — Text and presentation traits**

- **Evidence:** `Printable`, `Display`, `Debug`, `Renderable`, `Serialize`,
  `Encode`, `Decode` (`Compiler/Prelude/Traits.jet`), and `Describe`
  (public/detail, `spec.md:2195`).
- **Cost:** Seven traits for "turn a value into text or bytes". Users must
  pick among `Printable` vs `Display` vs `Describe`.
- **Proposal:** Keep three:
  - `Display` (user-facing text, used by `print`)
  - `Debug` (developer text)
  - `Encode`/`Decode` (format-parametric codec)

  `Serialize` becomes `Encode` and `Renderable` becomes `Display` into a
  target. `Describe`'s public/detail split becomes `Display` vs `Debug`.
  `Printable` is deleted, and `print` bounds on `Display`.
- **Trade-offs:** Renderers that need structured output (HTML) use `Encode` to
  a document codec. **Ballot**.

**O11 — Derive markers mirror traits**

- **Evidence:**
  - Markers `#Equatable`, `#Comparable`, `#Debug`, `#Printable`, `#Numeric`,
    `#Codable` and `#Error` sit alongside traits of the same names.
  - Auto-derive is on by default; there are `#!Debug` and `#Debug: false`
    opt-outs, a lint switch `deny: [auto_derive]`, `impl Trait.Derive { expand }`
    (`spec.md:1261-1280`), and distinct forwarding
    `#[Comparable, Printable] Score: distinct Int` (`spec.md:1240`).
- **Cost:** Each derivable trait has a trait name, a marker name, 2 opt-out
  spellings and a policy lint. That is 5 handles for one fact: "this type
  implements T structurally".
- **Proposal:** A derive is an `impl T` with a default body, and auto-derive
  is the default. One opt-out spelling: `#!T`. Delete `#T: false` and the
  derive markers that duplicate trait names. Distinct forwarding becomes
  `impl Score.Comparable` with the default (forwarded) body.
- **Trade-offs:** The `#[A, B]` stacked marker shorthand for derives
  disappears, which costs one line per trait. **Ballot**; refines #7053
  (structural Copy uses the same law).

**O12 — Validation trio**

- **Evidence:**
  - `validate { check(cond, at:, "msg") }` blocks (keywords `validate`, `check`;
    `spec.md:1300-1308`).
  - `#Pre`/`#Post` contracts.
  - Trait laws as `#Test` with `#Waive`.
  - Decode `![FieldError]` and `Validate.over(s)`.
- **Cost:** Two reserved keywords for what is a function returning field
  errors. "Contract or validator?" paralysis.
- **Proposal:** A validator is a method `validate: fn(self) -> Unit
  ![FieldError]` on a library `Validate` trait. `check(...)` is an ordinary
  function, and `#Pre`/`#Post` remain the only *language* contracts. Unreserve
  `validate` and `check`.
- **Trade-offs:** None in power; the field path comes from `FieldRef`.
  **Ballot**.

**O13 — Implicit conversion web**

- **Evidence:**
  - `#As` field forwarding (`spec.md:1253-1257`).
  - Union widening (`spec.md:2133-2138`).
  - Distinct trait forwarding (`spec.md:1240`).
  - `From`, ErrorConv (O2), `#Error` auto-lift (`spec.md:2104-2118`).
  - Function failure-rail widening (`spec.md:1916-1923`).
- **Cost:** Six implicit or semi-implicit coercion paths. This is the Scala-2
  implicit-conversion failure in miniature: a reader cannot see which edge
  fired.
- **Proposal:** One conversion lattice. Implicit coercion is allowed only along
  (a) subtyping (union widening, `Never`→`E` failure widening, applied
  uniformly; see T5), and (b) explicit `Type{expr}` via `From`. `#As`
  forwarding becomes a declared `impl T.Deref`-like trait, or is deleted.
- **Trade-offs:** Some call sites need `Type{x}`. **Ballot**.

### C. Ownership and memory

**O14 — Interior-mutability family**

- **Evidence:**
  - `Shared<T>`, `Shared.Weak`, `SharedGuard` with
    `guard_read`/`guard_edit`/`try_*`/`*_within`, and `Condition`
    (`spec.md:2675-2752`).
  - `Cell<T>` with 8 methods plus `CellReadGuard`/`CellEditGuard`
    (`spec.md:2794-2807`).
  - `Atomic<T>` (`Spellings.jet:31`).
  - Sema also knows `Atom` and `Mutex` by name (`Sema/Scopes.jet:3491`).
- **Cost:** Two guard families with parallel method names, two "shared mutable"
  types split by thread-confinement, and atomics on the side. This is Rust's
  `Cell`/`RefCell`/`Mutex`/`RwLock`/`Atomic*` split, which is Rust's most-cited
  learning cliff.
- **Proposal:** Remove the separate `Cell` API. `Cell<T>` is the
  thread-confined instantiation of `Shared<T>` (same `read`/`edit`/guard
  methods, same guard type), and confinement is a type fact the crossing
  checker already computes (`Sema/Ownership/Crossings.jet:522`). `Atomic<T>`
  is `Shared<T>` specialized for word-sized `T` by the optimizer, with explicit
  orderings only in Ring 3.
- **Trade-offs:** `Cell` today avoids lock cost by construction. The unified
  type must lower confined instances to plain loads/stores, which is required
  by the peer floor. **Ballot**; refines #7059 (which already reshapes
  `Shared`/`Weak`).

**O15 — `Pool` name collision, `Rows`, allocators**

- **Evidence:**
  - `Pool<T>`/`ID<T>` generational arena (`spec.md:2809-2837`) vs the
    `core.mem` `Pool` allocator (`spec.md:4052-4062`).
  - `Rows<T>`/`Row<T>` (`spec.md:2839-2849`).
  - Allocators `Arena`/`Bump`/`Fixed`, `#Region`.
  - Sema treats both meanings by string (`Sema/Calls/Memory.jet:96`
    `family == "Arena" || "Bump" || "Pool" || "Fixed"` vs
    `Sema/Calls/SharedOwner.jet:524`).
- **Cost:** One name, two unrelated mechanisms, plus a third
  handle-collection type.
- **Proposal:**
  - Rename the allocator, e.g. `FreeList`, or fold it into `Arena` options.
  - Make `Rows<T>` a `Pool<T>` iteration view or delete it.
  - One handle type, `ID<T>`.
- **Trade-offs:** None in power. Refines #7059 ("honest Pool/ID"); a **ballot**
  for the rename.

**O16 — Cleanup mechanisms**

- **Evidence:**
  - `Close` trait and `#Close` marker (site `.Function`).
  - `defer` and `Stmt.DeferClose`.
  - `scope.guard`, `os.atexit`, `.drop("reason")`.
  - `#Transact` with `on_commit`/`on_rollback` plus the `Rollback` trait.
- **Cost:** Seven ways to say "run this at end of scope or on failure".
- **Proposal:**
  - One destructor trait, `Close`, invoked automatically for owned values.
  - One scope hook, `defer`, with `defer on_err` covering rollback.
  - `Stmt.DeferClose` becomes the lowering of `Close`, not a user form.
  - `#Close` the marker is deleted (the trait suffices).
  - `scope.guard` is deleted (`defer` suffices).
  - `Rollback` is implemented via `defer on_err` in `#Transact`'s library.
- **Trade-offs:** `os.atexit` stays as a library call (process scope).
  **Ballot**.

**O17 — Move and use-count modifiers**

- **Evidence:** `#MoveOnly`, `#SingleUse`, `.copy()`, `.move()`, `^^`.
  Structural Copy lands via #7053.
- **Cost:** After #7053, `#MoveOnly` means "opt out of structural Copy" and
  `#SingleUse` means "linear". Two markers on one axis (number of uses).
- **Proposal:** One marker with a value, `#Uses(.Many | .Once)` or `#!Copy`
  plus `#Linear`, and fold it into the derive-opt-out law of O11 (`#!Copy` =
  move-only).
- **Trade-offs:** None. Refines #7053.

**O18 — Opt-in tracing collector**

- **Evidence:** `#Policy(gc)`, effect `Mem.Gc` (`spec.md:2873`, `:4145-4148`;
  `Compiler/Prelude/Effects.jet:130-138`). This conflicts with the brief's
  zero-GC power claim and with Design priority 3.
- **Cost:** A second memory model. Code that leaks cycles is fine under the
  collector and leaks without it (dialect D1). Every Core API must be correct
  under both.
- **Proposal:** Delete the collector tier, or quarantine it as a Ring-3 library
  type (`Gc<T>`) that is explicit at every use and never a policy switch.
- **Trade-offs:** Cyclic graphs need `Pool`/`ID` or `Weak`, which already
  exist. **Ballot**.

**O19 — Call-site `^` contract: three-way contradiction**

- **Evidence:**
  - Spec Ownership: "Calls carry no marks … the signature decides"
    (`spec.md:2333`, `:2402`; access-sigil section `:2953`).
  - `spec.md:2996` teaches `^buf.push(x)` and `fill(^xs)`.
  - Registry `Spellings.jet:75-76`: `^` before each changed named argument,
    receivers unmarked.
  - E0228 `CallMarkRequired` (`Compiler/Prelude/Diagnostics.jet:4329-4352`):
    `^` required, `^^` optional.
  - Examples use the marks (`Examples/Tooling/PowerAssert/PowerAssert.jet:41`);
    rewrite `Compiler/Migrations/MarkCaret1.jet`.
- **Cost:** The spec contradicts itself and the registry. Writes via receivers
  are invisible, and moves (`^^`) are optional while writes are mandatory: the
  more dangerous operation is the less marked one.
- **Proposal:** Pick one law and cut over. Recommended: **mark every place a
  call may change or take, receivers included** (`^xs.push(4)`,
  `grow(^xs)`, `send(^^msg)`). `jet fmt` inserts the marks, so writing cost is
  near zero and reading gain is total. The alternative law, "no marks; the
  signature decides", is coherent too. The current hybrid is not.
- **Trade-offs:** The receiver mark adds noise on builder chains.
  **Ballot**; refines #7053.

**O20 — Views and windows**

- **Evidence:** `View<T>`, `ViewMut<T>`, `View<str>`, `.in_place()` windows,
  `from` provenance clauses. Sema matches `"View"`/`"ViewMut"` 28 times each
  (e.g. `Sema/Expressions/Literals.jet:1075`, `Sema/Statements/ForIn.jet:1016`).
- **Cost:** Borrowed slices have a type pair plus a method plus a clause
  grammar.
- **Proposal:** Keep `View`/`ViewMut` (irreducible for zero-copy). Make
  `.in_place()` return `ViewMut`, and make `from` provenance inferable except
  at public signatures. Move all View knowledge in Sema behind a `Borrowed`
  role trait (O1 law).
- **Trade-offs:** None in power. Refines #7056.

### D. Effects

**O21 — Effect spellings and rungs**

- **Evidence:**
  - 7 spellings (`-[…]>`, `-[]>`, `-[via f]>`, `#FX(...)`, `#FX: (...)`,
    `#FX(...) {}`, bare `#FX`) and 8 rungs (`spec.md:3061-3082`).
  - `Stmt.AuthorityScope`.
  - Package `authority:{allow,deny,grants}`, run flags `--allow/--deny/-A`.
  - Tags carry `#FX` denials.
- **Cost:** The ladder itself is good (Pillar 4). The cost is the **two
  syntaxes on the function rung**: arrow-row `-[E]>` vs the marker
  `#FX(...)`/`#FX: (...)` in the body. Bare `#FX` means "pure" and `-[]>`
  means "pure" too.
- **Proposal:** Arrow-row on signatures and function types only. `#FX(...)`
  everywhere else (block, type, module, file). Delete in-body `#FX: (...)` on
  functions and bare `#FX` (use `-[]>`). That leaves one spelling per rung.
- **Trade-offs:** None. Refines #7057.

**O22 — Operation-fact axes**

- **Evidence:** `EffectAccess{Reads,Adds,Replaces}`, `irreversible: fn(self) ->
  Bool`, `#Repeatable`, `#Repeatable(key:)`, `#Replayable`
  (`spec.md:3125-3158`).
- **Cost:** Four overlapping idempotence/reversibility facts. The checks
  (E0778, retry-guard rules) are expert-only, but `#Repeatable` reaches app
  code through job retries.
- **Proposal:** One operation-fact record per effect operation:
  `{access, reversible, idempotent_by: .None | .Key(…)}`. `#Replayable` and
  `#Repeatable` become one marker that asserts `idempotent_by`.
- **Trade-offs:** None in power. **Ballot**; refines #7057.

**O23 — Effect kinds, values and scopes**

- **Evidence:**
  - Kinds: machine, service, library, package-kind (`parent:`).
  - Values: `-[Bucket{name:"photos"}]>` (`spec.md:3239-3262`).
  - Scopes: `FS.Read("templates/")` (`spec.md:3376`).
  - 5 `Meta.*` effects.
  - 24 FFI languages as effect cases.
- **Cost:** Effect *values* and effect *scopes* are two ways to parameterize
  an effect.
- **Proposal:** One parameterized form. An effect case may carry a value
  (scope = value of type `Path`, bucket = value of type `Bucket`). The four
  kinds stay as documentation categories, not compiler distinctions. FFI
  languages become a value parameter (`FFI("c")`), not 24 enum cases.
- **Trade-offs:** None. Refines #7057.

**O24 — What `-[]>` means**

See T6, T7 and T8. `-[]>` admits allocation and panic, forbids module-state
reads (which are not effects), and ignores `context.*` reads
(`spec.md:3264-3284`).

- **Proposal:** Define `-[]>` as the empty set *relative to a named default
  set* `Ambient = {Mem.Alloc, Panic}`, and make module-state reads a real
  effect (`State.Read`).
- Refines #7057; **ballot** for `State.Read`.

### E. Concurrency

**O25 — Combinator naming inverted against peers**

- **Evidence:** `task.race` returns the first *successful* result and `task.any`
  the first *completed* (`spec.md:5150-5152`). JavaScript's `Promise.race` is
  first settled and `Promise.any` is first fulfilled: exactly inverted.
- **Cost:** A guaranteed transfer bug for every JS/TS developer.
- **Proposal:** Rename to `task.first_ok` and `task.first_done`, or swap the
  meanings to match JS.
- **Trade-offs:** None. **Ballot** (public API).

**O26 — Messaging families**

- **Evidence:** Channel `Sender`/`Receiver` vs `Event`, `Hook`, `DecisionHook`,
  `AsyncEvent`, `Subscription`, `EventScope`, `EventTrace` and
  `DispatchReport`, all in `TYPE_NAMES`. Plus `Shared` + `Condition`.
- **Cost:** Eight built-in event types beside channels. These are built-in
  *type names*, so they are reserved for every program.
- **Proposal:**
  - Channels are the one built-in message primitive.
  - Events/hooks move to a Core library module (`core.events`) built on
    channels and closures.
  - Remove them from `TYPE_NAMES`.
- **Trade-offs:** Event tracing must be implementable by libraries, which
  Pillar 5 requires anyway. **Ballot**.

**O27 — Deadlines and cancellation**

- **Evidence:**
  - `#Deadline(limit)` and `task.timeout` (`spec.md:4772`, `:4875`).
  - Select `after` (`spec.md:5256`).
  - `*_within` guard variants on `Shared`.
  - `#Shield` (cancellation shielding).
- **Cost:** Four time-limit mechanisms.
- **Proposal:** `#Deadline` is the one mechanism, block or function rung.
  `task.timeout(d, f)` is library sugar for `#Deadline(d) { f() }`.
  `*_within` is deleted: a guard inside `#Deadline` respects it. `after` in
  select stays, because it is a selectable event rather than a limit.
- **Trade-offs:** None in power. **Ballot**.

**O28 — Ambient context**

- **Evidence:** `#Context`, `context.*`, `#ContextKey`, `#Frame`. Reading
  `context.*` is not an effect (`spec.md:3264-3284`).
- **Cost:** A hidden dependency channel beside effects and parameters.
- **Proposal:** Context reads become an effect (`Context.Read`) so they appear
  in rows. `#Frame` folds into `#Context`.
- **Trade-offs:** More effect rows. Mitigate with effect bundles (#7057).
  **Ballot**.

**O29 — Data parallelism vs groups vs streams**

- **Evidence:** `para_map`/`para_filter`/`para_partition`/`para_fold`,
  `task.group`, `yield`/`Stream`.
- **Cost:** `para_*` is a second parallel API beside groups.
- **Proposal:** `xs.lazy().par().map(f)`: one `par()` adapter over the existing
  `Iter` vocabulary (`spec.md:2001-2031`). Delete the four `para_*` names.
- **Trade-offs:** None. **Ballot** (public API).

### F. Control flow, patterns, operators

**O30 — `if` family and implicit subjects**

- **Evidence:** Seven forms (`spec.md:765-874`):
  - plain if/else-if
  - subject table `if x == {}`
  - guard table `if {}`
  - hole table with `<>`
  - type table `if T == {}`
  - select table
  - union-member arms

  Plus `it`, `#Through`, and comptime `#if`/`Stmt.ComptimeIf`/`ComptimeSwitch`.
  `<>` is also the pipe value (`Spellings.jet:73`).
- **Cost:** The *table* idea is one good mechanism. The cost is the implicit
  subjects (`it`, `<>`, bare `.name`), which change meaning by context.
- **Proposal:** Keep one table form, `if subject == { arms }`, with the
  subjectless guard as the `subject = true` case. The hole table becomes a
  subject table with `_` placeholders. `<>` keeps only the pipe meaning. `it`
  is deleted in favour of explicit loop/if bindings (V-leak).
- **Trade-offs:** Slightly longer arms in hole tables. Refines #7049 ("all 6 if
  forms"); **ballot** for `it`/`<>`.

**O31 — Loop AST and jumps**

- **Evidence:** `Stmt.While`, `For`, `Loop`, `CountedLoop` and 7 jump variants
  (`Statements.jet:3-40`). The surface is already one `loop` keyword.
- **Cost:** The cost is in the compiler only.
- **Proposal:** One `Stmt.Loop{head: .Forever | .While(c) | .Over(p, src) |
  .Counted(...), label}` and one `Stmt.Jump{kind, label, value}`. That removes
  9 variants.
- **Trade-offs:** None. Internal; refines #7049.

**O32 — Bespoke marked-block statements**

- **Evidence:** `Stmt.Unsafe`, `Impure`, `Reactive`, `Shield`, `Region`,
  `Policy`, `Layout`, `ContextBlock`, `Live`, `AssumeDet` and `Transact`, while
  a generic `Stmt.Switched` exists.
- **Cost:** The cost is in the compiler. Every marker block needs a new Stmt
  variant and touches every pass, which violates I8's pairwise law ("features
  that need a compiler special case to interact").
- **Proposal:** Lower all marker blocks to `Stmt.Marked{marker: MarkerUse,
  body}`. Marker semantics come from the marker's `impl Marker` facts (sites,
  scope effects). Delete 11 variants.
- **Trade-offs:** None. Internal; refines #7049.

**O33 — Division family**

- **Evidence:** `/`, `/%`, `%`, `%%`, `div_euclid`, `rem_euclid`
  (`MathLayout.jet:409-421`, `Spellings.jet:244-248`).
- **Cost:** Two dedicated operators (`/%`, `%%`) for rounding modes that most
  programmers never choose deliberately. `/%` reads as divmod to Python users.
- **Proposal:** `/` and `%` (floored) as the only operators. Truncated and
  Euclidean variants become methods (`a.rem_trunc(b)`, `a.div_euclid(b)`).
  Unreserve `/%` and `%%`.
- **Trade-offs:** Integer floor division needs a spelling. Make `/` on `Int`
  floor-divide (consistent with floored `%`), or use `a.div_floor(b)`.
  **Ballot**.

**O34 — Bit-op and overflow spellings**

- **Evidence:** `~` is both XOR and complement (`MathLayout.jet:437`). `&~`.
  `#Overflow(.Wrapping)` and `wrapping(...)`/`saturating(...)`.
- **Cost:** `~` overloads on arity; overflow behavior has a policy-switch form
  (D3) and an expression form.
- **Proposal:** Keep `wrapping(...)`/`saturating(...)` and delete the
  `#Overflow` switch (tighten-only law). `~` stays if a two-program cell
  justifies it; otherwise use `xor` as a method.
- **Trade-offs:** None. **Ballot**.

### G. Metaprogramming, policy, specialized sub-languages

**O35 — Comptime/meta API breadth**

- **Evidence:** ~30 forms (§1.2), including 6 fact roots (`#build`,
  `#package`, `#phase`, `#program`, `#here`, `#caller`), three type-as-value
  spellings (`TypeId{T}`, `Type{T}`, `type_of`; `spec.md:4350-4369`), and
  `x.reflect()`.
- **Cost:** `TypeId{T}` vs `Type{T}` is a direct duplicate. Six fact roots is
  a namespace zoo.
- **Proposal:**
  - Use `Type{T}` and delete `TypeId`.
  - One fact root, `#facts.build/.package/.phase/.program/.here/.caller`, or
    library functions.
  - `#run fn` and `#run use` fold into `#run {}`.
- **Trade-offs:** Slightly longer fact access. **Ballot**.

**O36 — Constant-time secret sub-language**

- **Evidence:** `.Fixed` tags drive masked branches, an IR lint and
  instruction validation (`spec.md:1378-1497`). Also `#Scrub`, `Expr.Tainted`
  and the value tags `Secret`/`Credential`/`PII`/`Input`.
- **Cost:** A full sub-language inside the core spec and the core compiler.
  It is valuable for crypto authors and invisible to everyone else, until a
  Core signature carries a tag (S-leak).
- **Proposal:** Keep taint tags (Ring 2, broadly useful). Move constant-time
  checking to a compiler-library plugin that `Core/Crypto` depends on, via the
  public compiler API that Pillar 5 already promises.
- **Trade-offs:** The plugin API must expose IR validation hooks. **Ballot**.

**O37 — Policy and lint system**

- **Evidence:** 17 package policy keys (`spec.md:3897-3926`),
  `#Allow(Jet.Lint, "why")`, `#Policy`, the `policy` keyword.
- **Cost:** Dialects (§2.2 D-leaks).
- **Proposal:** Apply the tighten-only law. Every key that changes semantics
  (gc, copies, failures, overflow) is deleted or becomes an explicit type/op.
- **Trade-offs:** Fewer global switches. **Ballot**; refines #7057 (G43
  warnings policy).

**O38 — Spec drift (cost: agents and humans learn dead forms)**

- **Evidence:**
  - Old lambda shape `(n: Int) -> base + n` (`spec.md:1947`) and
    `bump: (n: Int) -> {…}` (`:1957`), vs the canonical `fn(params) { body }`
    (`:1824`).
  - Comptime `@` prefix (`spec.md:1501`, `:1572`).
  - `?`/`?(text)` propagation and `#Policy(failures: .Explicit)`
    (`spec.md:2140-2155`, `:2148`, `:4726`).
  - Call-mark contradiction (O19).
  - Function values with `^` parameters "direct-call-only" (`spec.md:1888`)
    vs `fn(^T)` types in the registry.
- **Cost:** Every stale form doubles the apparent surface.
- **Proposal:** A spec sweep against `LEXICAL_LEDGER`/`LANGUAGE_REWRITES`,
  plus a guard that every fenced `jet` block in `spec.md` parses with the
  admitted compiler.
- **Trade-offs:** None. No ballot; refines #7047 and #7049.

**O39 — Foreign-language breadth**

- **Evidence:** FFI effect cases for 24 languages
  (`Compiler/Prelude/Effects.jet`). The FFI spec section runs
  `spec.md:5461-6030`.
- **Cost:** ~570 spec lines and 24 effect cases in the core.
- **Proposal:** Core ships C and Rust. Other languages are packages through
  one plugin interface (`FFI(lang)` value; O23).
- **Trade-offs:** First-party breadth moves out of the compiler. **Ballot**.

---

## Section 4 — Pairwise orthogonality traps and compiler special cases

Each entry follows the same format: the trap and its evidence, a **Code
example**, the **Root cause**, and the **Law** that removes it. Code examples
are minimal sketches in live spellings; `…` elides bodies. Where an example
sketches a *proposed* spelling, the text says so.

**T1 — Ownership × Methods: receiver writes are invisible, argument writes are
mandatory**

- **Evidence:** `Spellings.jet:75-76`, E0228 (`Diagnostics.jet:4329-4352`) vs
  `spec.md:2333`.
- **Code example:**
  ```jet
  ^xs: [1, 2]
  xs.push(3)      // writes xs, no mark
  grow(^xs)       // writes xs, mark required (E0228 without it)
  send(msg)       // moves msg via ^^ parameter, mark optional
  ```
- **Root cause:** The mark was attached to the argument syntax, not to the
  place being changed.
- **Law:** *Every place a call may change or take carries its access mark, in
  every position*, inserted by `jet fmt`. Or, alternatively: no position
  carries one.

**T2 — Ownership × Closures: capture mode depends on escape**

- **Evidence:** `spec.md:1953-1974`.
- **Code example:**
  ```jet
  ^v: [1]
  f: fn() -> Int { v.len() }   // local: copies v now
  v.push(2)
  f()                          // E0204
  ```
  The same body passed to `xs.map(...)` clones; one that writes a captured
  local in an escaping lambda writes "its own copy".
- **Root cause:** Captures are modeled as copies taken at creation, not as
  borrows with checked lifetimes.
- **Law:** *A capture is a parameter.* Reads capture as read borrows, writes as
  `^` borrows, moves as `^^`. Escape only decides whether the borrow must be
  converted to ownership. That conversion is visible (`.copy()` or `^^`) and
  diagnosed at the escape point.

**T3 — Closures × Concurrency: hidden invocation lock**

- **Evidence:** `spec.md:1974-1977`: "`task.group` children and their owner
  that call the same function value take turns under its invocation lock".
- **Code example:**
  ```jet
  ^n: 0
  bump: fn() { n += 1 }
  task.group { task { bump() }; task { bump() } }
  // serialized by an invisible lock
  ```
- **Root cause:** Writing closures are made shareable by inserting
  synchronization instead of being rejected as non-sendable.
- **Law:** *No hidden locks.* A closure that writes captured state is not
  sendable. Sharing requires `Shared<T>`, whose cost is visible (Design
  priority 3: no hidden runtime overhead).

**T4 — Closures × Ownership × Function types: marked parameters**

- **Evidence:** `spec.md:1884-1890` (E0227: such functions are direct-call
  only) vs `Spellings.jet` `fn(^T)` function types; payload constructors
  `fn(^^FileWriter) -> Sink` (`spec.md:1897-1900`).
- **Code example:**
  ```jet
  grow: fn(^xs: [Int]) { … }
  apply_all: fn(fs: [fn(^[Int])]) { … }
  apply_all([grow])
  // spec.md:1888 says E0227 (direct-call only);
  // the registry's fn(^T) row says it fits
  ```
- **Root cause:** Access marks were added to signatures before function
  *types* could carry them.
- **Law:** *Parameter access marks are part of the function type*, and
  coercion matches marks exactly.

**T5 — Errors × Function values × Containers: failure-rail variance**

- **Evidence:** `spec.md:1916-1929` (E0638).
- **Code example:**
  ```jet
  ok: fn(n: Int) -> Int { n }
  run_one: fn(f: fn(Int) -> Int) { … }        // parameter: implicit Err contract
  run_all: fn(fs: [fn(Int) -> Int Err!]) { … }
  run_one(ok)     // accepted: the single value widens
  run_all([ok])   // a list keeps the form it was made with: E0638
  ```
- **Root cause:** Widening is implemented as a call-site coercion, not as
  subtyping.
- **Law:** *`Never` failure ≤ any failure* is subtyping. It applies through
  covariant positions (results, read-only containers) and is rejected through
  invariant ones (`^` containers). This is the one rule Swift and Kotlin use
  for `throws` and nullability.

**T6 — Effects × Module state**

- **Evidence:** `spec.md:3264-3284`: module-state reads are not effects, but
  `-[]>` forbids them (E3401).
- **Code example:**
  ```jet
  LIMIT: 10
  ^hits: 0
  f: fn() -[]> Int { hits }   // E3401 although no effect is read
  ```
- **Root cause:** A purity check is layered on top of the effect row.
- **Law:** *Purity = empty effect row.* Make mutable module-state reads a
  declared effect (`State.Read`) or permit them under `-[]>`.

**T7 — Effects × Context**

- **Evidence:** `context.*` reads are not effects (`spec.md:3264-3284`).
- **Code example:**
  ```jet
  f: fn() -[]> String { context.user }
  // a "pure" function depends on ambient task context
  ```
- **Root cause:** Context is an implicit parameter that has neither parameter
  nor effect status.
- **Law:** *Every ambient input is a parameter or an effect.* Use
  `Context.Read`.

**T8 — Effects × Memory/Panic: `-[]>` is not empty**

- **Evidence:** `spec.md:3264-3284` (`-[]>` admits allocation and panic), vs
  `Mem`/`Panic` as deny-only effects in `Compiler/Prelude/Effects.jet`.
- **Code example:**
  ```jet
  f: fn() -[]> [Int] { [1, 2, 3] }   // allocates and may panic under the "empty" row
  // excluding Mem.Alloc needs the deny-only root at another rung
  // (package `authority: deny` or `--deny`)
  ```
- **Root cause:** There are two classes of effect (positive and deny-only)
  with different default membership.
- **Law:** *One effect lattice with a named ambient default* (`Ambient =
  {Mem.Alloc, Panic}`). `-[]>` means `Ambient`, and a separate, proposed
  spelling (to be balloted) means truly empty. Each root's default is
  documented in one place.

**T9 — Types × Library (Pillar 5): Sema grants powers by Core type name**

- **Evidence:**
  - `Sema/Calls/SharedOwner.jet:411,524` (`Cell`, `Pool` add/remove/ids)
  - `Sema/Ownership/Crossings.jet:353,522` (thread confinement for `Cell*`)
  - `Sema/Scopes.jet:3491,3518` (`Cell`/`Atom`/`Mutex`)
  - `Sema/Ownership/Copy.jet:1396` (`SharedGuard`/`ProcessChild`/`TempDir`)
  - `Sema/Calls/Memory.jet:96` (allocator families)
  - `Sema/Comparability.jet:1023` (`Task`/`Sender`/`Receiver`/`SharedGuard`/`Pool`/`Set`)
  - `Sema/Expressions/Dyn.jet:45,141`, `Sema/Calls/Plugin.jet:97`,
    `Sema/Registration/Derives.jet:2669`,
    `Sema/Registration/OwnedTraitContracts.jet:294`
    (`Dyn`/`JSON`/`TOML`/`YAML`/`CSV`/date types)
  - `Sema/Registration/KeyBy.jet:554` (`Cell`/`Weak`/`Atomic*`)
- **Code example:**
  ```jet
  MyCell: struct<T> { … }   // cannot be thread-confined
  // and cannot be excluded from KeyBy or comparisons
  ```
  The string match fires only for the name `Cell`.
- **Root cause:** Capabilities are keyed on names instead of declared facts.
- **Law:** *Every compiler capability of a Core type is a role trait or marker
  that any package can declare* (`impl ThreadConfined`, `impl Guard`,
  `impl Handle<T>`, `impl Allocator`, `impl DynamicData`), and Sema queries the
  role. This is the O1 law applied to types.

**T10 — Generics × Modules: two comptime-parameter grammars**

- **Evidence:** `module<K: Hash>(capacity: Int)` (`spec.md:4426-4442`) vs
  number parameters `<N: Int>` on types (fixed lists, `spec.md:1698-1822`).
- **Code example:**
  ```jet
  Ring: struct<T, N: Int> { … }
  cache: module<K: Hash>(capacity: Int) { … }   // same idea, different list
  ```
- **Root cause:** Generic modules were added as a separate feature (ML-functor
  style).
- **Law:** *One comptime parameter list `<…>` for types and values*,
  everywhere generics appear.

**T11 — Patterns/Control × Implicit subjects**

- **Evidence:** `it` and bare `.name` inside loops (`spec.md:4381-4384`,
  E-SUBJECT-CALL); `<>` pipe vs hole (`Spellings.jet:73`).
- **Code example:**
  ```jet
  loop ships { form.validate(.carrier, …) }
  // .carrier reads it.carrier, not the FieldRef: E-SUBJECT-CALL
  ```
- **Root cause:** Several features each claimed the "implicit current value"
  slot.
- **Law:** *An implicit subject exists only where it is the sole candidate*.
  In practice: delete `it`, keep `<>` for pipes only, and keep bare `.name`
  for expected-type resolution only.

**T12 — Traits × Derive × Distinct × Policy**

- **Evidence:** Auto-derive by default, `#!Debug`, `#Debug: false`,
  `deny: [auto_derive]`, `#[Comparable, Printable] Score: distinct Int`,
  `impl Trait.Derive` (`spec.md:1240`, `:1261-1280`).
- **Code example:**
  ```jet
  Score: distinct Int        // derives? forwards? depends on package policy
  #[Comparable] Score: distinct Int
  #Debug: false
  ```
- **Root cause:** Derive is modeled as markers instead of as default trait
  implementations.
- **Law:** *A derive is an `impl` with a default body; auto-derive inserts it;
  `#!T` removes it.* No policy switch.

**T13 — Errors × Conversions: which edge fired?**

- **Evidence:** O2 and O13.
- **Code example:**
  ```jet
  impl IoError -> AppError { … }
  impl AppError.From(ParseError) { … }
  #Error DbError: …          // auto-lifts
  load: fn() -> T AppError! { read()!; parse()!; query()! }
  // three different conversion mechanisms
  ```
- **Root cause:** Each conversion mechanism was added for one error ergonomics
  case.
- **Law:** *`!` converts only via `From` or subtyping.*

**T14 — Concurrency × Ownership: spawn forms differ in capture law**

- **Evidence:** `task f()` takes captured function values (E0121,
  `spec.md:1977-1978`); group children borrow (`spec.md:5186-5204`);
  `task ^^name {}`.
- **Code example:**
  ```jet
  task.group {
      task { use(log) }     // borrows log
  }
  task { use(log) }         // detached: must own
  task ^^log { use(log) }   // explicit move list
  ```
- **Root cause:** Spawn is a statement form with its own capture rules, not a
  call.
- **Law:** *Spawn is a call of a closure.* Captures follow T2's law, and group
  scope merely extends the borrow lifetime. #7053's "auto-move last use into
  `^^`" should be the *same* rule as ordinary calls.

**T15 — Markers × Statements × Sites**

- **Evidence:**
  - 11 bespoke block `Stmt`s (O32).
  - A marker above and inside the same item is E0942.
  - The TypeUse site fix (`MarkerRules.jet:140`) is named on #7049.
- **Code example:**
  ```jet
  #Transact { … }    // Stmt.Transact
  #Shield { … }      // Stmt.Shield
  #Deadline(1s) { … }   // Stmt.Switched? (generic path)
  // three marker blocks take two compiler paths
  ```
- **Root cause:** Each marker block was implemented as its own statement.
- **Law:** *A marker is a value of a Marker type applied at a site.* One
  `Stmt.Marked` lowering, with behavior from the marker's declared facts.

**T16 — Operators × Numbers: arity-overloaded `~`, policy-overloaded `+`**

- **Evidence:** `MathLayout.jet:437`; `#Overflow(.Wrapping)`.
- **Code example:**
  ```jet
  a ~ b      // xor
  ~a         // complement
  #Overflow(.Wrapping)
  total + 1  // meaning set by a marker possibly far above
  ```
- **Root cause:** Behavior is chosen by context rather than by type or
  operation.
- **Law:** *An operator's meaning is a function of operand types only.*

**T17 — Fields × Methods: two member namespaces**

- **Evidence:** `value.step(…)` selects the method and `value.step.call(…)`
  the field (`spec.md:1931-1937`); E0802/E0950 (`spec.md:1091-1137`).
- **Code example:**
  ```jet
  Job: struct { step: fn(Int) -> Int }
  impl Job { step: fn(self, n: Int) -> Int { … } }
  j.step(1)        // the method
  j.step.call(1)   // the field: the only place .call survives
  ```
- **Root cause:** Fields and methods were given separate namespaces, which
  allows the collision.
- **Law:** *One member namespace.* A field and a method with the same name is
  a declaration error, so `.call` disappears entirely.

**T18 — Policy × Semantics: identical code, different meaning per package**

- **Evidence:** §2.2 D1–D7.
- **Code example:** In package A (default), a non-Copy value assigned twice is
  copied by inference. In package B (`#Policy(copies: .Explicit)`), the same
  lines are an error, or a move.
- **Root cause:** Policies are allowed to choose semantics.
- **Law:** *Tighten-only policies* (§2.3 law 3).

**T19 — Concurrency names × Peer knowledge**

- **Evidence:** O25 (`spec.md:5150-5152`).
- **Code example:**
  ```jet
  task.race([a, b])   // JS devs expect first settled
  // Jet returns first successful
  ```
- **Root cause:** The names were chosen without the peer-convention check.
- **Law:** *Names of borrowed concepts keep the dominant peer meaning, or use a
  name no peer uses.*

**T20 — Bodies × Records: braces mean block or record by content**

- **Evidence:** `spec.md:1861-1873`. After a declared result, one `name: value`
  line is a record "unless its value starts with `loop`".
- **Code example:**
  ```jet
  f: fn() -> Box { value: v }        // record
  g: fn() -> Int { total: loop … }   // labeled loop
  h: fn() { x: 1 }                   // binding
  ```
- **Root cause:** Record literals reuse block braces without a distinguishing
  token.
- **Law:** *A body is always a block; a record is an expression inside it.*
  Keep the brace-drop only as a `jet fmt` *display* convention. Alternatively,
  require `,` or a type for records. Single-field records then need
  `Box{value: v}`.

**T21 — Modules × Values: Core modules are not values**

- **Evidence:** `m: math` after `use core.math` reports E0629
  (`spec.md:4344-4347`).
- **Code example:**
  ```jet
  m: text      // ok (user module)
  m: math      // E0629 (Core module)
  ```
- **Root cause:** Core modules are compiled differently, so they have no
  record.
- **Law:** *Core is ordinary Jet* (Pillar 5). Every module has a `Module`
  record.

**T22 — Comptime × Types: three type-value spellings**

- **Evidence:** `TypeId{T}`, `Type{T}`, `type_of(x)`, plus bare type names
  where a `Type` is expected and E0636 elsewhere (`spec.md:4350-4369`).
- **Code example:**
  ```jet
  counts[Position] = 2        // ok: Type expected
  t: Position                 // E0636
  k: TypeId{Position}         // a different type value?
  ```
- **Root cause:** Reflection grew in two passes.
- **Law:** *One type-value type, `Type`.* `TypeId` is deleted.

---

## Section 5 — Pareto-ranked simplification plan

Ranking is by concepts, AST nodes and special cases eliminated, weighted by
how often everyday code meets them. "Card" means the work refines an open card
without new owner choices. "Ballot" means a new owner decision is required
(syntax, public API or semantics) before any build.

### Tier 1 — architectural collapses that delete whole categories

| # | Collapse | Deletes | Gate |
|---|---|---|---|
| 1 | **Tighten-only policy law** (T18, O18, O34, O37): delete semantic switches `#Policy(gc)`/`Mem.Gc`, `#Policy(copies:)`, `#Overflow`, residual `failures:`; one derive opt-out | 4+ dialect axes; every Core API's dual-semantics testing | **Ballot** (semantics). Refines #7057 (G43 policy) and #7053 |
| 2 | **Role-trait law for types** (T9, O20, O15): replace Sema string matches on Core names (~150 sites across Cell/Pool/View/ViewMut/SharedGuard/Dyn/Err/Iter) with declared roles (`ThreadConfined`, `Guard`, `Handle`, `Allocator`, `Borrowed`, `DynamicData`) | Name-keyed special cases; restores Pillar 5 | Card: refines #7056 (parallel Core tables) and #7059 (honest Pool/ID). **Ballot** only for public role-trait names |
| 3 | **AST follows surface** (O1, O31, O32): `struct + impl Role` for Marker/Effect/Tag/Fact/UnitFamily; one `Stmt.Loop` + `Stmt.Jump`; one `Stmt.Marked` | ~5 Item kinds, ~20 Stmt variants; 24-way matches across ~75 Sema files | Card: refines #7049 (composition laws, "all 16+ marked blocks") and #7056. No ballot (internal) |
| 4 | **Uniform access-mark and capture law** (T1, T2, T3, T4, T14, O19) | E0228 hybrid, spec contradiction, capture-mode zoo, hidden invocation lock, E0227 split | **Ballot** (call-site marks; capture semantics). Refines #7053 (auto-move into `^^`) |
| 5 | **One conversion lattice** (O2, O13, T5, T13): delete `impl A -> B` (`Item.ErrorConv`); `!` via `From` + subtyping; failure widening as subtyping | 1 Item kind, 3 implicit paths, E0638 asymmetry | **Ballot**. Adjacent to #7047 |
| 6 | **Ring-closed diagnostics gate** (§2.3 law 1): tag diagnostics by ring; fail snapshots when a Ring-1 corpus triggers higher-ring vocabulary | Makes progressive disclosure enforceable instead of aspirational | Card: refines #7049. **Ballot** for adopting the gate as an invariant |

### Tier 2 — high-value unifications and orthogonality fixes

| # | Unification | Gate |
|---|---|---|
| 7 | Effects: one spelling per rung (O21); named ambient default for `-[]>` (T8); `State.Read` and `Context.Read` effects (T6, T7, O28); one operation-fact record (O22); parameterized effect values replace effect scopes and the 24 FFI cases (O23) | Refines #7057; **Ballot** for new effect names |
| 8 | Interior mutability: `Cell` = confined `Shared`, one guard family; `Atomic` as specialization (O14) | **Ballot**; refines #7059 |
| 9 | Concurrency: rename `race`/`any` (O25, T19); `#Deadline` as the single limit, delete `*_within`, `task.timeout` = sugar (O27); `par()` adapter replaces `para_*` (O29); events to `core.events` library (O26) | **Ballot** (public API) |
| 10 | Text traits to `Display`/`Debug`/`Encode`/`Decode` (O10); derive = default impl (O11, T12); `#MoveOnly`/`#SingleUse` onto the same law (O17) | **Ballot**; refines #7053 and #7046 |
| 11 | One comptime parameter list (T10); unified `Item.Module` with parameters, alias as value, Core `Module` records (O5, T21) | **Ballot**; refines #7056 |
| 12 | Methods at 2 sites (O8); one member namespace (T17) | **Ballot** |
| 13 | `if` family: one table form; delete `it`; `<>` pipe-only (O30, T11) | Refines #7049 ("all 6 if forms"); **Ballot** for `it`/`<>` |
| 14 | Division to `/`, `%` plus methods; `~` and overflow by type/op only (O33, O34, T16) | **Ballot** |
| 15 | Body/record brace rule (T20); construction meanings narrowed (O9) | Refines #7051; **Ballot** for T20 |

### Tier 3 — clean deletions and deferrals of premature surface

| # | Deletion / deferral | Gate |
|---|---|---|
| 16 | `migration` + 5 verbs to a DSL/typed data; unreserve 6 keywords (O3) | **Ballot**; refines #7046 |
| 17 | `protocol`/`client`/`server` + residual `#State`/`State` to one typestate library mechanism (O4) | **Ballot**; refines #7046 |
| 18 | `validate`/`check` keywords to a library `Validate` trait (O12) | **Ballot** |
| 19 | `#loop` TemplateLoop to `#run` sugar or delete; `TypeId` deleted; one fact root; `#run fn`/`#run use` folded (O6, O35, T22) | **Ballot** |
| 20 | Cleanup: delete `#Close` marker, `scope.guard`, `DeferClose` as user form; rollback via `defer on_err` (O16) | **Ballot** |
| 21 | `UnitFamily` item to `distinct` + `Unit` trait (O7) | **Ballot** |
| 22 | Constant-time checking to a compiler plugin used by `Core/Crypto` (O36) | **Ballot** |
| 23 | FFI beyond C/Rust to packages (O39) | **Ballot** |
| 24 | Spec drift sweep plus a "spec code blocks parse" guard: lambda shape (`spec.md:1947`, `:1957`), comptime `@` (`:1501`, `:1572`), `?` propagation (`:2140-2155`, `:2148`, `:4726`), call marks (`:2333`/`:2402`/`:2953` vs `:2996`), marked function values (`:1888`) | No ballot except O19's law; refines #7047 and #7049 |

### Expected effect (estimate, if Tiers 1–3 land on top of #7042–#7059)

| Measure | Today | After | Notes |
|---|---|---|---|
| Item kinds | 24 | ~12 | |
| Stmt kinds | 36 | ~18 | |
| Reserved keywords | 50 | ~38 | migration verbs, `protocol`/`client`/`server`, `validate`/`check`, `it`, `State` |
| Built-in type names | 47 | ~37 | events, `Cell` guards |
| Ownership/memory concepts | ~26 | ~16 | |
| Concurrency concepts | ~24 | ~14 | |
| Effect spellings | 7 | 3 | |
| Name-keyed Sema special cases | ~150 | ~0 | |

The headline numbers would then sit near Rust on declarations and below Rust
on ownership, while keeping the stated core power:

- memory safety without a collector
- structured concurrency
- checked effects
- comptime/DSL

Go-level simplicity remains out of reach on raw counts. It becomes reachable
on the Ring-1 path, but only if laws 1–4 of §2.3 are enforced by the compiler.
