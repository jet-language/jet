# Design review: one source of truth for Core exports, canonical nominal identity, single-file Core check, monolith splits, and declaration poisoning

Card: #7056 (criteria 1–4). Compiler baseline: `068535bc409`. Every line count
below comes from `wc -l` on that tree in this session. Every source line cited was
read in this session.

## 1. Inventory of parallel Core export and metadata tables

| # | Location | Lines | What it encodes about Core | How it stays in sync today |
|---|---|---|---|---|
| T1 | `Compiler/Prelude/Core.rows` | 301 | `bootstrap` order (l.12–15), `root_types` (l.17), `namespace_only` (l.18), `error_types` (l.23), per-module `exports`/`types`/`depends_on`, `source_module … owns` (l.25–28+) | Hand-edited. Enum case lists are written by `Tools/Agent/CoreEnumCases.jet` (572 lines) with `--write` (header l.7–10). |
| T2 | `Compiler/JetFoundation/Source/Registry/CoreModules.jet` | 910 | `CoreModuleExportNames`, `CoreLeafKind`, `CoreModuleDeclaration` (l.1–35), lookups over `CORE_MODULE_EXPORTS`/`_KEYS`/`_KEY_ROWS` (l.37–45) | A compiled view of T1 (`line` field = Core.rows line, l.28) |
| T3 | `Compiler/JetFoundation/Source/Registry/CoreEnumPayloadRows.jet` | 54 | Payload slot types of compiler-owned Core enum families | Hand-authored (CoreModules.jet l.19–21) |
| T4 | `Compiler/JetSema/Source/Sema/Ownership/Copy.jet` | 1511 | `sema_copy_named_is_core` hand list of 11 names (l.403); `sema_atomic_type_name` (l.375); dynamic-loan heads `CellReadGuard`/`CellEditGuard` (l.390–395); compiler-owned family facts (l.733–788) | Hand-edited. The l.398–402 comment says these are "intentionally" outside T1. |
| T5 | `Compiler/JetDriver/Source/Driver/Loader.jet` | 7985 | `jet_driver_core_bare_aliases` maps `URL`, `Phase`, `CompilerEvent`, `Type`, and `SourceLocation` to demanded companions (l.4717–4723); Core demand scan, ordering, and private-unit rules (l.3840–6540) | Hand-edited |
| T6 | `Compiler/JetSema/Source/Sema/CoreUsage.jet` | 1878 | Core-module predicate (l.445); name tests for `Decimal`/`Fraction`/`Complex` (l.726, l.1148), `Atomic` with suffix match (l.736), `Duration` receiver (l.1237); `core.text.fmt` routing (l.480–483); UI capability rule E2937 (l.502) | Hand-edited |
| T7 | `Compiler/JetSema/Source/Sema/Expressions.jet` | 8459 | Precise-numeric names `Decimal`/`Fraction` (l.1483), `Decimal → Fraction` division result (l.1550) | Hand-edited |
| T8 | `Compiler/JetFoundation/Source/Types/Types.jet` | 2413 | Dual-spelled Core identity `name == "Tensor" \|\| name == "<corelib>/core.compute.Tensor"` (l.2150) | Hand-edited workaround for §2 |
| T9 | `Compiler/JetDriver/Source/Driver/CoreInterface.jet`, `CoreLayers.jet`, `CoreReach.jet` | 1922 / 153 / 2125 | Core interface units, runtime layering, reachability | Partly derived from T1. A per-row audit is owed (cutover item C9). |

**Duplication hazards (each one is a concrete failure mode):**
- **H1.** A Core type renamed or moved in `Core/` silently drops out of the name
  tests in T4, T6, T7, and T8, because they match by string and are not checked
  against T1. The checker then treats the type as user-defined: copy facts, numeric
  promotion, and Atomic sendability all change with no error.
- **H2.** T6 l.736 matches `name.ends_with(".Atomic")`, so a user type
  `mylib.Atomic` gets Core's Atomic rules. That is a nominal-identity leak.
- **H3.** T5's alias list encodes which Core declarations a bare name needs. It is
  a hidden dependency graph that `depends_on` in T1 should own.
- **H4.** T1's `error_types` (l.19–22) is "held to the sources" by a source check,
  so T1 is already partly derived. The rest of T1 is not, and the split policy is
  undocumented.

**Single source of truth.** `Core/**/*.jet` declarations are the only authored
truth. Their markers (`#Error`, `#MoveOnly`, precise-numeric, compiler-intrinsic
carrier) carry the facts T4, T6, T7, and T8 hand-code today. T1 becomes a
generated, checked-in twin, and T2 and T3 are compiled from it. A Core fact with no
declaration source goes into one table, `Compiler/Prelude/Intrinsics.rows`. That
covers the intrinsic carriers `Unit`, `Never`, `Err`, `U8`, `Range`, and the
process handles from T4 l.403. Every other table becomes a query over the derived
rows, keyed by canonical identity (§2), never by spelling.

## 2. Canonical nominal type identity

**Defect.** `type_identity_equal` (`Types.jet:1861`) compares `.Named` heads by raw
string (`l.1917–1918`: `.Named(other) -> name == other`) and `.Apply` heads the same
way (`l.1922`). A Core type reaches the checker under two spellings: the bare name
(`Tensor`) and the module identity (`<corelib>/core.compute.Tensor`). Helpers
`type_identity_unqualified_nominals` and `type_unqualified_text`
(`Types.jet:1724–1759`) strip identities for display only. Equality never
normalizes, so callers add dual-spelling workarounds (T8, H2). About 38 files call
`type_identity_equal`: 20 on the first grep page, with more paged.
`TypeIDs.jet:743` says its O(1) path has the "same semantics", so both paths must
change together.

**Design.**
1. **One canonical form.** Every `.Named`/`.Apply` head in a checked `Type` carries
   a resolved identity. Core identity is `<corelib>/<module>.<Name>`, user identity
   is `module:…`. The conversion happens once, at name resolution, and a bare
   spelling never survives past resolution.
2. **Equality stays string-equal on canonical heads.** No normalization runs inside
   `type_identity_equal`. That keeps it allocation-free, as its l.1859 comment
   requires.
3. **Debug assertion.** A bare head reaching `type_identity_equal` is an ICE (exit
   101, I2) in debug builds, which finds every unresolved producer.
4. **Delete every dual-spelling workaround.** That covers T8 l.2150 and every
   `ends_with(".X")` Core test (H2). The derived Core rows (§1) answer "is this
   Core's X" by identity.
5. **Display goes only through `type_identity_unqualified_nominals`.** Diagnostics
   text and UI snapshots must not change.

**Risk.** Producers that synthesize `Type.Named{name: "Duration"}` literally, such
as `MarkerRules.jet:168–170`, must use an identity constructor
(`core_type("Duration")`). A sweep for `Type.Named{name: "` across `Compiler/` is
part of the cutover.

## 3. Single-file `jet check Core/<file>.jet`

**Requirement.** `jet check Core/X/X.jet` gives the same package context as
`jet test` on the same file:
- `Core/package.jet` is recognized as the root.
- The file loads as Core module `X`, with Core privileges and Core.rows ownership.
- It is not loaded as a user script that imports Core.

**Design.**
- Both commands resolve the target through one function: the nearest `package.jet`
  at or above the file (`Front.jet:873–890` already implements the walk).
- If that root is the Core package, the file maps to its `source_module` row in T1
  (`source_module app path Core/App/App.jet …`, Core.rows l.26).
- The file is then checked as that module inside the Core load graph.
- A file under Core with no `source_module` row is a user error with a registered
  code. It is never a silent script fallback.

**Proof.** For every `source_module` path in T1, the diagnostics from `jet check`
must be byte-identical to the check phase of `jet test` on the same path. The
script lives under `Tools/Agent/` and is written in Jet.

The baseline divergence was not reproduced in this session. Before building, the
implementer records one failing pair as the regression case.

## 4. Monolith split plan

The rule for every split: one concern per file. Split by moving code without
changing it, one commit per file group, with an S2==S3 fixed point after each
group. No behavior change is allowed in a split commit.

**`Core/Math/Math.jet` (46,217 lines).** Split into a `Core/Math/` module family
by concern:

| File | Contents |
|---|---|
| `Scalar.jet` | Int/Float elementary functions |
| `Precise.jet` | `Decimal`, `Fraction` |
| `Complex.jet` | `Complex` |
| `Linear.jet` | vectors and matrices |
| `Stats.jet` | statistics |
| `Special.jet` | special functions |
| `Random.jet` | random number generation |
| `Bits.jet` | bit operations |
| `Units.jet` | `Quantity`/`Measure` support |

The public surface stays the one `core.math` module, with T1 `owns` lists per file.
The final cut follows the declaration-group boundaries in the file. Before cutting,
the implementer emits the group histogram (declarations per prefix) and attaches it
to the card.

**`Compiler/JetSema/Source/Sema/Expressions.jet` (8,459 lines).** An
`Expressions/` directory already exists (`Control.jet`, `Literals.jet`,
`Lambdas.jet`, `FixedArithmetic.jet`, `LayoutValues.jet`, …). Move the remaining
groups into it:
- `Conversions.jet`: numeric widening and casts, around l.267–497
- `Comparisons.jet`: equality and ordering, around l.816–1094
- `Precise.jet`: precise numerics, around l.1483–1550, which then reads the derived
  Core rows instead of name strings
- a dispatcher file of at most 1,000 lines

**`Compiler/JetDriver/Source/Driver/Loader.jet` (7,985 lines).** Split into:

| File | Contents |
|---|---|
| `CoreDemand.jet` | l.4509–4991: identifier demand, bare aliases (to be deleted per §1) |
| `CoreParse.jet` | l.5307–5541: Core source parsing and interface units |
| `CoreOrder.jet` | l.6213–6540: Core unit placement and ordering |
| `Loader.jet` | the user-package load graph only |

## 5. I47 declaration poisoning

**Defect (card criterion 4).** One syntax or type error in a declaration cascades
into 20+ downstream type, sendability, and unused-parameter diagnostics.

**Design.**
1. **Poisoned symbols.** When parsing or resolving a declaration fails, its symbol
   is entered as poisoned, carrying the root diagnostic's id. It is not omitted.
   - A poisoned function keeps its name and arity if they parsed, and its type
     becomes `Poison`.
   - A poisoned type keeps its name and has no fields.
2. **`Poison` absorbs checks.** `Poison` unifies with every type silently. Any
   check whose subject or operand is `Poison` emits nothing: type mismatch,
   sendability, copy/move, unused parameter or binding, and exhaustiveness. A
   use-site of a poisoned symbol emits nothing.
3. **Scope is the declaration.** Bodies of other declarations are checked normally,
   so real independent errors still surface (collect-all).
4. **Never reaches codegen.** A program with any poisoned symbol never reaches
   codegen. The root diagnostic already failed the build, so I2 holds.
5. **Proof.** UI snapshots for:
   - a typo in a signature
   - a typo in a struct field type
   - a missing closing brace in a function body
   - a bad marker argument

   Each snapshot must show exactly the root diagnostic plus any genuinely
   independent ones. This needs a new snapshot set, and no existing diagnostic code
   is reused (I4).

**Attack.** Silent unification could hide a real second error in the same
declaration. That trade-off is accepted only within the poisoned declaration. The
snapshot set must include one independent error elsewhere to prove collect-all
still reports it.

## 6. Binding cutover list

The card cannot close while any item is open.

| ID | Item |
|---|---|
| C1 | Generate `Compiler/Prelude/Core.rows` from `Core/**/*.jet`. The source check fails on any hand edit. Fold `Tools/Agent/CoreEnumCases.jet` into the generator, or delete it. |
| C2 | Create `Compiler/Prelude/Intrinsics.rows` for the intrinsic carriers. Delete the hand list at `Copy.jet:403` and `sema_atomic_type_name` (`Copy.jet:375`). |
| C3 | Delete the name tests at `CoreUsage.jet:726`, `:736`, `:1148`, `:1237` and `Expressions.jet:1483`, `:1550`. Replace them with identity or marker queries over the derived rows. |
| C4 | Delete `jet_driver_core_bare_aliases` (`Loader.jet:4717–4724`). Express those dependencies as `depends_on` or declaration references in the Core sources. |
| C5 | Canonical heads at resolution. Delete the `Types.jet:2150` dual spelling. Keep `TypeIDs.jet:743–757` on the same semantics. Sweep `Type.Named{name: "` across `Compiler/`. |
| C6 | Shared single-file package resolution for `jet check` and `jet test`, with a parity script. |
| C7 | Splits of `Core/Math/Math.jet`, `Sema/Expressions.jet`, and `Loader.jet` per §4, with S2==S3 after each group. |
| C8 | I47 `Poison` symbol kind, absorption rules, and UI snapshots per §5. |
| C9 | Per-row audit of `CoreInterface.jet`, `CoreLayers.jet`, and `CoreReach.jet`. Every hand-coded Core fact moves to the derived rows. |
| C10 | Update the `Docs/Spec/` text that names Core.rows as hand-authored. Update `hot-text.list` entries for renamed or moved functions (`JetCLI/hot-text.list:2202`, `:2705`). |

## 7. Verification plan

1. **Build.** `jet build --profile dev Compiler`, then a self-host S2==S3 fixed
   point after each split group and after C5.
2. **Drift guard.** Rename one Core type in a scratch copy. The build must fail at
   the generator or source check, not silently change copy or numeric facts. This
   proves H1 is closed.
3. **Identity.** A test where user type `mylib.Atomic` gets user semantics (H2),
   and a test where bare and qualified Core spellings compare equal after
   resolution.
4. **Single-file parity.** Run the C6 script over every T1 `source_module` path.
5. **I47.** Snapshots per §5.
6. **Per tier.** O0, O1, and O2 goldens for the affected Core math and precise
   numeric examples.
7. **Full suite.** Run `Tools/Agent/VerifyFull.jet` (collect-all) on the batch
   compiler and compare against the last run. This is required because C3 and C5
   are cross-cutting sema changes.

## Verdict

The parallel tables are real and measurable:
- T4, T5, T6, T7, and T8 hold hand-written Core name strings outside T1.
- `type_identity_equal` compares raw strings while Core types reach it under two
  spellings.

The single-source design (derive from `Core/` and resolve identity once) removes
both problems at the root. Open risks:
- the §3 baseline divergence, which was not reproduced here
- the final Math.jet group boundaries, which need the histogram

Both are criteria on #7056, not ballots: they are implementation choices within
the approved scope.
