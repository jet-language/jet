# Pre-build adversarial review: large-scale effect ergonomics (card #7060)

Date: 2026-11-27. Scope: criterion 1 of #7060, the review that must land before
the build. The review was read-only. Sources read in this session:
`node Tools/Tower/tower.mjs card show 7060` (and `#7057`, `#7064`, plus the
card list filtered for effect cards); `Docs/Spec/spec.md` §"One rule on every
rung" (L3061–3096), §"Function and block ceilings" (L3571–3679) and
§"Higher-order and trait effects" (L3681–3752);
`Compiler/JetSema/Source/Sema/Effects/{Checks,TypeRows,PureRoot}.jet`;
`Compiler/JetSema/Source/Sema/Calls/Direct.jet`;
`Compiler/JetSema/Source/Sema/Registration/Items.jet`;
`Compiler/JetFoundation/Data/Syntax/{CoreSurface,PackageFiles}.jet`;
`Compiler/Prelude/Diagnostics.jet` (E3405);
`Examples/ErrorsAndEffects/{Effects/EffectHigherOrder.jet,Debugging/Dbg.jet}`;
`Tests/UI/{E3405_debug_trace_left_in_build,dbg_in_release_build}.*`.

No compiler was built or run for this review. Every behavioral claim below
comes from reading the code. Claims marked **unverified** need a test before
anyone relies on them.

---

## 0. Headline findings (read these first)

1. **Most of fix 3 already ships.** `debug(value)` is the ratified spelling.
   The registry says "owner: named `debug`, not `dbg`"
   (`CoreSurface.jet:237-242`, `BUILTIN_DEBUG`). It adds no effect: the call
   records an intrinsic fact with an empty row (`Calls/Direct.jet:318-322`).
   A shipped artifact, a `release` build and a `hardened` build all reject it
   with E3405 (`Registration/Items.jet:991-993`, `Direct.jet:296-301`). The
   Core seal also rejects it. `jet fix` unwraps it. A golden example covers
   `-[]>`, `#Memo` and `#run`
   (`Examples/ErrorsAndEffects/Debugging/Dbg.jet`, plus
   `Dbg.o1.err.out`/`Dbg.o2.err.out`), and two UI snapshots exist. **No new
   Prelude spelling is needed. A `dbg` ballot would re-ballot a settled
   choice and must not be raised.** What remains is a short list of gaps
   (§3).
2. **Most of the static half of fix 2 already ships.** A call through a type
   parameter's bound becomes a `SemaEffectParamDispatch`. That dispatch is
   charged to the caller that binds the parameter
   (`Checks.jet:398-407`, `4515-4546`). It is solved to a fixed point by
   `sema_effect_resolve_type_params` (`Checks.jet:4659-4744`), as the spec
   says at L3747-3752. Whole-program union pollution survives only in four
   fallbacks plus the `dyn` path (§2). The card text ("eliminating
   whole-program cross-package union pollution" for `<E: Trait>`) is
   therefore partly stale. The build must target the fallbacks, not rebuild
   what already works.
3. **Fix 1 overlaps `#7057`.** Criterion 1 of `#7057` (G39) already covers
   "effect-row polymorphism on higher-order callbacks/traits". Building both
   cards as written would make two owners for one mechanism, which breaks
   AGENTS "never duplicate a card" and I8. Ownership needs a ballot (B1).
4. **Fix 1 runs into `#7064` and I8.** `#7064` cuts effect spellings to 3. If
   pass-through is inferred automatically, `-[via f]>` becomes a second
   mechanism for the same meaning unless it is collapsed (B2).
5. **The registry text for `via` disagrees with the spec.**
   `PackageFiles.jet:239-242` documents the pass-through as "a `#(via f)`
   signature annotation". The spec (L3076, L3709) and the parser
   (`Parser/Items.jet:1100`) use `-[via f]>`. Under I4/I7, registry text must
   match the shipped rule. This goes on the cutover list whatever the
   ballots decide.

---

## 1. Fix 1: automatic multi-callback pass-through and per-instantiation rows on `fn` fields

### Current behavior (code)

- `via` names exactly one parameter: `effect_via: NamedSpan?` (`Checks.jet:177`
  and `533`; parser `Parser/Items.jet:1100, 1187`).
- `sema_effect_apply_via` (`Checks.jet:1487-1535`) copies the parameter's
  written row into the function's `direct` set. If the parameter type has
  **no row**, it sets `maximal = true` (`1525-1527`). So `-[via f]>` on an
  unrowed callback collapses to Maximal, which defeats the point of `via`.
- An unannotated HOF that calls a parameter has an indirect call with no
  callee key and no effect bound. That makes it Maximal
  (`sema_effect_record_fact`, `Checks.jet:3858-3861`). This matches spec
  L3690-3692: "A changeable local, a parameter, a returned, stored, or
  otherwise unknown function value contributes the maximal set". So the
  problem the card describes is real.
- A lambda written in the body and called through an immutable `:` local is
  exempt (`sema_effect_written_lambda`, `4218-4231`). A mutable local is
  never exempt (`4223`).
- Type rows take "the rows written on its function-typed fields"
  (`TypeRows.jet:1-6`). An unrowed field called through `self.cb(...)` hits
  the same Maximal path.

### Attack: termination

- **Recursive and mutually recursive HOFs.** Model the inferred summary of
  function `F` as `own(F) ∪ ⋃_{p ∈ P(F)} row(arg_p)`, where `P(F)` is a subset
  of `F`'s function-typed parameter positions. `P` only grows: a parameter
  joins when it is called, or when it is forwarded to a parameter position in
  `P(G)` of a callee `G`. The lattice is the powerset of a finite set of
  parameters, and the join is monotone. So the SCC fixed point terminates in
  at most Σ|params| rounds. This is the same shape as the existing
  `resolve_type_params` loop (`4671-4691`). **Sound to build.**
- **Forwarding with re-wrapping.** Example:
  `g(fn(x) { f(x) })` inside `F(f)`, where `g` is `F` itself (polymorphic
  recursion on rows). The lambda's row is `{via f}` and contains no new row
  variables. Rows range over effects plus the current function's own
  parameter variables, so nesting cannot create an unbounded number of rows.
  This is unlike E0909's type expansion (`4746-4833`). **Terminates.** It
  still needs a UI or golden case that proves it.
- **A callback that calls itself through a captured reference** (a Y-combinator
  shape through a `Shared` cell). The value passes through storage, so the
  escape rule below applies and it is not inferred.

### Attack: soundness of escaping and mutable values

| Case | Required rule | Why |
|---|---|---|
| `f` is stored in a struct field, global, `Shared`, channel, or task spawn | Do **not** add `f` to `P(F)`. The later call site pays the field's written row, or Maximal | The effect happens at another time and under other caps. Charging `F`'s caller is not enough: a scoped `FS`/`Net` cap is mediated at run time only while the cap is in force (spec L3645-3660) |
| `f` is returned | Do not infer, unless the return type carries the row (that is B4) | Same reason |
| `f` is captured by a lambda that escapes | Not inferred | Same reason |
| Parameter is `^fn` (mutable borrow) or rebound `^f` | Never inferred, as today (`4223`) | The callee can swap the caller's function |
| `f` is only called, or only forwarded to a `P` position | Inferred | Second-class use. This is exactly Effekt's sound fragment (§4) |

The design must decide "escapes" with the existing ownership/escape facts, not
with a separate syntactic scan (I3, I8).

### Attack: interaction with declared rows and `#FX`

- **A declared ceiling plus an unrowed callback parameter.** Example:
  `h: fn(f: fn(Int) -> Int) -[IO]> Int { f(1) }`. Today this is Maximal,
  which is E0740. Under inference there are two options. The definition
  check could treat `row(f)` as a variable and move the check to each call
  site: E0747 at the argument if `row(arg) ⊄ IO`. Or it could keep today's
  error. These give users visibly different behavior → **ballot B3**.
- **`#FX` caps above the HOF** (module, type or trait caps). A cap binds
  "including what each function reaches through a call" (L3608-3640). The
  inferred `via` part is only known at each call site, so the cap must be
  re-checked at every instantiation site. E0749 then has to name the
  instantiation, the way `SemaEffectInstanceReach` (`495-502`) already does
  for type parameters. Reuse that mechanism. Do not add a second report
  path (I8).
- **`#FX(...) { }` blocks inside the HOF** that call `f`. The region check
  (E0712) must also be deferred to the call site, and the generated region
  must still disappear when it has no scoped entry (L3605-3606).

### Attack: public API and SemVer (E1218, E3513)

- E1218 covers only *caps* on public types, traits and modules (L3638-3640).
  The lock records "each dependency's inferred effect row" as a **set**
  (L3527-3533). Once pass-through is inferred, a public function's effective
  contract is `(own, P)`.
- **Widening hazard.** A patch release changes a library body from
  "call `f`" to "store `f` and call it later". `f` leaves `P`, so every
  consumer's row jumps to Maximal. This is a widening, and the lock must
  record `P` by parameter **position** (not name, so a rename is not a
  break). `jet build --locked` must refuse it as E3513. Gaining a position in
  `P` (Maximal → pass-through) is a narrowing and is compatible.
- **Lock format change.** This is a greenfield cutover: migrate every lock
  file in the repo, and update Jetpack's reader and writer in the same
  batch.

### Per-instantiation rows on generic structs with `fn` fields (the second half of fix 1)

This is the riskiest part of the card.

- Jet's `Type.Fn` carries an optional row. A struct type carries no row
  parameter. "Per-instantiation callback rows" therefore needs a **hidden
  row parameter on the struct type**. That brings these problems:
  - Mixed instances in one `[Handler]` must join (union), so inference stops
    helping as soon as values are aggregated.
  - Assigning to a mutable field (`h.cb = other`) must widen the instance's
    row at every store, without regard to control flow.
  - Sharing (`Shared<Handler>`, globals) puts writers in other packages that
    separate compilation cannot see. That forces the written row or Maximal.
  - Converting to a `dyn` value erases the parameter, so the dyn ceiling
    from fix 2 is required anyway.
  - A recursive type (`Node{next: Node?, cb}`) shares one parameter across
    its cycle. It terminates on the finite effect lattice, as TypeRows
    already does.
  - A public constructor's return type now has an inferred hidden parameter
    that must be locked.
- Every peer that does this soundly makes the row **visible in the type**
  (Koka datatypes with effect parameters, Effekt boxes with capture sets).
  None of them hides it. A hidden type-level parameter is a new user-facing
  concept, so I8 applies (1-in, 1-out) → **ballot B4**.

**Verdict on fix 1.** HOF pass-through inference: **GO**, restricted to
second-class (non-escaping, immutable) parameters, with `P` locked by
position. Stored `fn` fields: **NO-GO as written**. Build only what B4
ratifies.

---

## 2. Fix 2: static trait resolution vs explicit ceilings on dynamic trait values

### Current behavior (code)

- **Static.** For a receiver of type `.Named(param)` where `param` is in the
  node's `type_params`, the call becomes a `param_dispatch` and is charged at
  the binding site (`4538-4544`, `4659-4744`). This works today. Report
  labels name the instantiation (`Traced<Loud>`).
- **The four fallbacks that still take the union of every implementation in
  the program (the real pollution):**
  1. The binding site does not resolve to an owner that has an impl key:
     `add_dispatch_union` at `4715`. This covers associated types,
     `Apply`-typed bindings, and a type with no impl of its own (only an
     inherited default).
  2. A call edge to a callee with open param dispatches that was recorded
     **without** its type arguments: union at `4725-4733`.
  3. A static trait call whose receiver is generic but is not one of this
     node's own type parameters: union at `4545`.
  4. Any implementer without a summary of the method makes the result
     **Maximal** (`4556-4568`).
- **Dynamic.** For a `TraitObject` receiver the precedence is: method row,
  then trait `#FX`, then the union of `dyn_lists` across every package
  (`4433-4468`, spec L3728-3734). E0743 fires only when no implementation
  exists **and** a ceiling encloses the call.

### Attack: separate compilation and action at a distance

- The dyn union depends on which packages are loaded. A downstream
  package's `impl` therefore widens an *upstream* function's inferred row. A
  dependency's recorded lock row then depends on its consumer, which breaks
  the "row only widens on update" rule at L3527-3533. This is the strongest
  argument for the card's direction: **require a ceiling at dyn dispatch.**
- **Third-party traits.** If the trait lives in package A (no row, no
  `#FX`) and the dyn call is in package B, B cannot edit A. "Require trait
  `#FX` or a method row" then leaves B with no fix except wrapping. A
  ceiling the *consumer* can write is necessary. The only sound and modular
  place for it is the **trait-object type**, checked when a concrete value
  is boxed (`TExprKind.TraitBox`), where the concrete impl's row must fit
  the bound. This is Effekt's box/capture-set rule (§4). It needs a new
  spelling, which must come from the syntax registry and be ratified (I7),
  so → **ballot B5**. This review does not invent one.
- **Generic through dynamic.** Example: `run<T: Worker>(w: T)` instantiated
  with a trait object. `name_find(... owner)` at `4704` fails for a
  trait-object type and falls into fallback 1. The rule must send
  `T := dyn Worker` to the dyn ceiling. It must not union.
- **Dynamic through generic.** Example: `Traced<T>` holding a trait-object
  field. The type row (`TypeRows.jet`) currently takes "a trait's row is the
  union of its implementing types' rows" (L3665-3666). That union must be
  replaced by the ceiling too, or the type row brings the pollution back.
- **Empty-impl traits (E0743).** Once ceilings are required on every
  row-less dyn call, today's narrow E0743 condition (no impl and an enclosing
  ceiling) is subsumed. The new rule has a different meaning. Under I4, either
  reword the E0743 registry entry, its what/why/fix text and the
  `Docs/Spec/diagnostics.md:310` witness text to the new single meaning, or
  register a new code and retire the old meaning. Never keep both.
- **Default methods.** A trait default body that calls another trait method
  on `self` dispatches through `Self`. This needs a test showing it goes
  through `param_dispatch` and not through fallback 3. **Unverified.**
- **Comptime guards.** `SemaEffectGuard` arms (`425-433`) must keep pruning
  per instance after the fallbacks are removed. Regression test required.

### Migration

The rule is stricter. Before it lands, run a collect-all sweep over every Jet
source in the repo: Compiler, Core, Jetpack, Examples, Tests and Tools. It
must list each trait-object call of a row-less method on a trait without
`#FX`, and the whole batch must be fixed together (AGENTS "Collect-all").
**I did not take the count in this review.** It is a binding pre-build step,
and its exact number goes on the card.

**Verdict on fix 2.** **GO** on removing fallbacks 1–3 and narrowing 4.
**GO** on requiring a ceiling at dyn dispatch, provided B5 ratifies a
consumer-writable spelling. Without one, third-party traits leave consumers
stuck. Criterion 3's wording ("resolve static/generic trait dispatch against
concrete monomorphized impl effects") is mostly met by existing code. Its
evidence must name the fallbacks it removes. Re-showing the already-working
`param_dispatch` path is not evidence.

---

## 3. Fix 3: dev-only zero-effect probe

### Current behavior (code): already shipped

- `debug(value)` adds no effect, so `-[]>` and `#Memo` keep their rows
  (`Direct.jet:318-322`). It lowers to the source-owned `core.term.eprint`
  row (`323-331`).
- E3405 fires when `graph.mode == .Program && (core_seal || artifact_build ||
  profile ∈ {release, hardened})` (`Direct.jet:296`, `Items.jet:991-993`).
  `artifact_build` is set for `jet build` and for the `jet fix` pass
  (`Driver/Identity.jet:456-459`).
- The `#run` comptime trace prints during compilation. Release rejects it
  (`Tests/UI/dbg_in_release_build.*`, `Dbg.o1/o2.err.out`).

### Gaps (attacks on the criterion-4 text)

1. **`--deny-warnings` does not exist.** No match for
   `deny.?warn|warnings_as_errors` under `Compiler/JetCLI/Source` or
   `Compiler/JetDriver/Source`. The flag is delivered by `#7057` G43. Either
   criterion 4 must be `blockedBy #7057`, or the owner rules (B6) whether
   `debug` is classed as a dev warning under G43. Today it is not a warning:
   it is silent in dev and E3405 in release.
2. **A trace in a published dependency breaks consumers.** A dependency that
   ships a `debug` trace makes every consumer's `jet build` fail with E3405 in
   a file the consumer cannot edit. Publishing (Jetpack) must refuse a
   package that contains a trace, and a consumer's report must name the
   dependency. **Unverified** whether publish runs a check with
   `artifact_build`.
3. **Taint and sink bypass.** A call "writes it out" when its row names `Log`
   or `IO` (spec L1372). `debug` has an empty row, so a tagged secret value
   (`#FX(!…)` kinds, credential taint) could leak through stderr during dev
   runs. Dev runs still handle real credentials.
   `Tests/Taint/credential_is_denied_at_every_log_level` covers only
   `log.debug`. Required: `debug(...)` counts as a sink for tag and taint
   checks even though it adds no effect row. **Unverified, high priority.**
   (`Secret`-typed values are already refused by E0112 because they are not
   `Debug`.)
4. **Pure root (E3401).** `PureRoot.jet:46-64` resolves callees from call
   facts and DeclIDs. The `debug` lowering emits a real `Call` to
   `core.term.eprint`. A pure-root walk that reaches the lowered body could
   report a false E3401. **Unverified.** Required: a golden or UI case with
   `debug` inside a pure program root.
5. **Run-time deny of `IO`.** Under `--deny=IO`, does the stderr write trip
   the E1803 run stop (L3501-3502)? A dev probe must not be stopped by
   authority, or the spec must say that it is. **Unverified.**
6. **`#Test`.** Tests run on the dev profile, so traces print. A test under
   `--profile release` fails with E3405. That is consistent, but no snapshot
   shows it.
7. **Memo determinism.** The probe traces only on a cache miss (stated in
   `Dbg.jet:4`). This is acceptable and already documented.

**Verdict on fix 3.** **No new spelling and no `dbg` ballot.** Rescope
criterion 4 to gaps 1–5. Gap 1 is gated on `#7057`.

---

## 4. Strongest peers

- **Koka.** Row-polymorphic effect inference with Hindley–Milner
  unification. Higher-order functions are polymorphic in callback effects
  without annotation, and datatypes holding functions take explicit effect
  parameters (Leijen, "Koka: Programming with Row Polymorphic Effect
  Types", MSFP 2014). Lesson: HOF inference without annotations is mature.
  Rows on *stored* functions are written on the type.
- **Flix.** Polymorphic effects with Boolean unification (Madsen & van de
  Pol, OOPSLA 2020). *Associated effects* attach an effect to a type-class
  instance and resolve it at the instance (Lutze & Madsen, PLDI 2024). That
  is exactly fix 2's static half, which Jet already has through
  `param_dispatch`.
- **Effekt.** Second-class functions need no effect variables. A first-class
  value must be **boxed** with an explicit capture set (Brachthäuser et al.,
  "Effects as Capabilities", OOPSLA 2020; "Effects, Capabilities, and
  Boxes", OOPSLA 2022). Lesson: infer for non-escaping parameters, and
  require the row at the escape or box point. That is the rule proposed for
  fix 1 and for the dyn ceiling in fix 2.
- **Swift `rethrows`.** Automatic pass-through over *all* function
  parameters, without naming them. It is precedent for multi-callback
  pass-through without a per-parameter marker. Its known limit is that it is
  only a single bit, a gap typed throws later addressed.
- **Rust `dbg!` / `#[cfg(debug_assertions)]`.** `dbg!` compiles in release.
  Clippy's `dbg_macro` lint is opt-in. `cfg(debug_assertions)` changes the
  program's meaning per profile. Jet's E3405 is stricter and keeps one
  meaning across tiers (I9): release refuses the trace instead of compiling
  it differently. Jet is already ahead here.
- **Roc `dbg`.** Allowed inside otherwise-pure code, like Jet's zero-effect
  `debug`. I did not verify in this session how Roc treats `dbg` in
  optimized builds. Do not cite Roc's release behavior until it is checked
  against Roc's documentation.

---

## 5. Verdict

**Conditional GO.** The build may start only after B1, B2 and B4 are decided.
B5 must be decided before the dyn-ceiling rule lands. B3 and B6 must be
decided before criteria 2 and 4 close.

- Fix 1 HOF inference: GO, limited to the second-class fragment, with
  position-keyed `P` in the lock.
- Fix 1 stored fields: NO-GO pending B4.
- Fix 2: GO on the fallbacks and the dyn ceiling. The criterion 3 evidence
  must target fallbacks 1–4.
- Fix 3: already shipped. Rescope to the gaps; there is nothing to ballot
  for spelling.

Card-scope corrections need an owner ballot because AGENTS forbids
rewording criteria unilaterally. The criteria needing it are criterion 2
(the overlap with `#7057`), criterion 3 (the stale "eliminating ..." wording)
and criterion 4 (the `--deny-warnings` dependency).

## 6. Binding cutover list

Implementation (sema, one mechanism):
- [ ] `Checks.jet` `sema_effect_apply_via` (1487-1535): unrowed `via`
  parameter must stop collapsing to Maximal, or `via` is removed (B2).
- [ ] `Checks.jet` `sema_effect_record_fact` (3858-3861) and
  `sema_effect_record_unresolved_call` (4260-4264): calls to second-class
  parameters record pass-through, not Maximal.
- [ ] `Checks.jet` `sema_effect_merge_lambda` (4190-4203): carry lambda
  `via` rows instead of Maximal.
- [ ] `Checks.jet` fallbacks at 4545, 4556-4568, 4715 and 4725-4733 removed
  or narrowed. Generic-through-dyn is routed to the dyn ceiling.
- [ ] `Checks.jet` `sema_effect_record_dispatch` (4433-4468) and
  `sema_effect_record_value_reads` dyn branch (4498-4510): no union across
  `dyn_lists`; the ceiling is required.
- [ ] `TypeRows.jet`: the trait row is no longer a union of implementers;
  stored-field rows follow B4.
- [ ] Parser `Parser/Items.jet:1100-1187` and Formatter `Items.jet:1944-1960`,
  if B2 changes the `via` grammar.
- [ ] Jetpack lock: record `P` by parameter position; E3513 covers losing a
  `P` position. Migrate every repo lock file in the same batch.

Registry, diagnostics and docs:
- [ ] `PackageFiles.jet:239-242` `KW_VIA` doc: fix the stale `#(via f)` text,
  or delete the row if B2 picks A.
- [ ] `Diagnostics.jet` and `JetFoundation/Source/Registry/DiagnosticRows.jet`:
  E0743 (rewritten to one meaning or retired), E0747/E0748 (via wording),
  any new code for a missing dyn ceiling (what/why/fix and UI snapshot), and
  E3405 if B6 changes it.
- [ ] `Docs/Spec/spec.md` L3076, L3662-3668, L3681-3752; L1372 (`debug` as a
  sink); L3527-3533 (lock records pass-through).
  `Docs/Spec/diagnostics.md:310`.
- [ ] `CoreSurface.jet:237-242` `BUILTIN_DEBUG` doc, if B6 or the sink rule
  changes its contract.
- [ ] `jet explain effects` and the LSP hover show inferred pass-through and
  instance rows. Share this with `#7057` G39; do not build it twice.

Tests and examples (O0, O1 and O2 each; the web tier only where `debug` and
effects apply):
- [ ] `Examples/ErrorsAndEffects/Effects/EffectHigherOrder.jet`: a golden
  that shows a bounded caller passing through two callbacks; the output must
  be the report, not "ok".
- [ ] `TraitCap.jet` plus a new multi-module example: an unrelated
  downstream impl does not widen an upstream row; a dyn call without a
  ceiling produces the diagnostic.
- [ ] UI snapshots: recursive and mutually recursive HOFs; re-wrapped
  forwarding; escape to field, global, return or task (still Maximal);
  `^fn` parameter; `#FX` block inside a HOF; generic-through-dyn;
  dyn-through-generic; default method dispatch through `Self`; comptime
  guard arms.
- [ ] `debug` cases: pure root (E3401 must not fire), tagged/taint value,
  `--deny=IO` run, `#Test` under release, and publishing a package that
  contains a trace.
- [ ] A collect-all sweep of row-less dyn calls across all repo Jet sources,
  with the exact count recorded and every hit fixed in the same batch.
- [ ] A cold-compile `jet perf` before and after on the compiler build. The
  extra fixed point must not regress the cold-compile target, and the
  result is recorded.
- [ ] A full suite before criteria 2 and 3 close, because the change touches
  sema rules for every module.

## 7. Ballot questions

**B1: who owns callback and trait row polymorphism.** `#7057` G39 criterion 1
and `#7060` criteria 2 and 3 overlap.
- (A) `#7060` owns the inference engine; G39 is reworded to cover bundles,
  aliases and hover only.
- (B) Move the HOF part of `#7060` criterion 2 onto `#7057`.
- (C) Keep both, with `#7057` blocked by `#7060`.

Trade-off: A gives the cleanest single owner. B delays the P0 card. C leaves
duplicate acceptance terms.

**B2: what happens to `-[via f]>` once inference exists (I8 1-in/1-out).**
Coordinate with `#7064`'s 3-spelling cut.
- (A) Delete `via`. Pass-through is always inferred and published through
  the lock.
- (B) Keep `via` as an optional asserted contract and extend it to several
  parameters (new grammar, registered in `Syntax/` and ratified).
- (C) Keep single-parameter `via` beside inference.

C gives two mechanisms for one meaning; this review advises against it.

**B3: a declared row plus an unrowed callback parameter.**
- (A) Status quo: the parameter is Maximal, giving E0740 at the definition.
- (B) The row means "declared ∪ inferred pass-through"; the ceiling is
  checked at each call site (E0747 on the argument).

B is more permissive, but the error moves away from the definition.

**B4: rows on stored `fn` fields.**
- (A) A hidden per-instance row parameter, limited to immutable, unshared,
  non-dyn instances.
- (B) Require a written row on `fn` fields; `jet fix` inserts the inferred
  row.
- (C) An explicit effect parameter on the struct (new syntax, Koka-style).

B adds no new concept. A brings in hidden generics. C adds surface.

**B5: a ceiling the consumer can write on dyn dispatch.**
- (A) Only a method row or the trait's `#FX`. A consumer of a third-party
  unbounded trait has no fix except wrapping.
- (B) A row on the trait-object type, checked at boxing. The spelling must
  be proposed from the syntax registry; this review invents none.
- (C) Keep the union, but only over the impls the compiling package can see.

C is unsound under separate compilation; this review advises against it.
The same ballot must also settle the diagnostic code: reword E0743 or
register a new code.

**B6: `debug` and `--deny-warnings`.** Under G43:
- (A) `debug` also emits a dev warning, so `--deny-warnings` refuses it,
  and E3405 stays for artifact and release builds.
- (B) `--deny-warnings` is unrelated; only artifact and release builds
  refuse traces. Criterion 4's text is then reworded.

In both cases, criterion 4 is blocked by `#7057` until the flag exists.

No ballot is needed for a probe spelling: `debug` is already the ratified
name, and the owner rejected `dbg`.
