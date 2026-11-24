# Adversarial review: D-RULE-ACROSS1 (how a check rule pairs two calls for E2804) and D-FORM-FIELD1 (what `.carrier` is in a typed form's `validate`)

Date: 2026-11-25
Reviewer: AcrossReview (Claude Opus, fresh agent)
Ballots reviewed:
- `$JET_SCRATCH_ROOT/tmp/orch/ballots/D-RULE-ACROSS1.json` (draft, author G1Generation-2; card #6851 c3, Tower card `c0e6znz4`). The author's text is kept as `D-RULE-ACROSS1.json.before-review`. Recommendation changed: the author's A (a list of earlier same-receiver calls) is now B, and a keyed pairing is the new A.
- `$JET_SCRATCH_ROOT/tmp/orch/ballots/D-FORM-FIELD1.json` (draft, author G1Generation-2; card #6851 c3, Tower card `c0e6znz4`). The author's text is kept as `D-FORM-FIELD1.json.before-review`. Recommendation A holds, repaired.

Compiler for probes: `$JET_SCRATCH_ROOT/tmp/orch/tip21-chain/s1/jet` (`jet a0.0.46-dev`, built from dev 491390130f).
Source read: dev worktree `Main-dev-collectall` at 2a09ee0e42. Line numbers below are that commit's.
Evidence: `$JET_SCRATCH_ROOT/AcrossReview/p1`-`p5` (ACROSS1) and `f1`-`f4` (FIELD1), each `main.jet` with `out.txt` (p3 also `timing.txt`). Every probe ran alone. Validator output: `$JET_SCRATCH_ROOT/tmp/orch/ballot/across1/validate/*.txt`.

Verdict scale: **holds**, **holds within limits** (the limits are stated), **fails**.
F1-F15 are ACROSS1 round 0 findings and F16-F30 are FIELD1 round 0 findings. R1-R5 (ACROSS1) and R6-R10 (FIELD1) are round 1 findings. Their state is in the last section.

## 0. Context the ballots depend on

- Card #6851 c3: the Http.jet and HttpRouting.jet checkers in Sema/Calls move to Core declarations and are deleted, every diagnostic code keeping its UI snapshot. So the E2804 table (HTTPRouting.jet, the route-table pairing) and the whole typed-form intercept (HTTP.jet:439-590) are both in scope. c22: rule dispatch at most 1 µs per heard call. c30: `CompilerArg.function` / FunctionInfo for function arguments, not landed.
- D-CORE-CHECK1=A: a rule hears one `CompilerCall` (CoreRecordRows.jet:250-269: caller, callee, module, span, name, args, receiver; `CompilerArg` label, type, value, function, span) and answers it; a rejection replaces the call's own errors. `sema_call_rule_stage` (CheckRules.jet:181-207) stages answers keyed by owner, rule span and site span.
- E2804 today: three compiler sites (HTTPRouting.jet table pairing; the mount path in HTTPMount.jet; the App graph in WebGraph.jet) plus the run-time `AmbiguousRoute` text in three copies (Diagnostics.jet:23257, HTTPRouting.jet:458, Server.jet:372). `jet explain --routes` renders the same table (WebGraph.jet:689-923).
- The StaticRoutes/E2808 move is on branches `land/6851-c3-static-routes-e2808-r2` and `rule-order-r2` (batch 23; r1 failed batch 21). It is not on dev.
- D-DX-FORM1=A (Tower, read into `AcrossReview/dxform1.json`): forms derive from input structs; "Config can rename, exclude, group, or replace fields"; ratified example `.validate(.carrier, on: .Blur, with: check_carrier)`.
- D-SUBJECT-CALL1=A (syntax-decisions.md:6123-6130): the subject shorthand is valid only where a callable is expected; a non-callable context needs the full lambda; "adds no ... second member mechanism".
- D-LOOP-SUBJECT1=A (syntax-decisions.md:9304-9311): subject-member resolution runs before the expected-type static shorthand. `sema_expr_check_bare_member_chain` (Expressions.jet:8230-8256) resolves a loop's `it` first; bindingless loops exist only in expression forms (`loop xs -> …`, `loop xs if …`; parser Loops.jet:364, Statements.jet:10190).
- Typed forms today: `sema_web_form_method_member` (HTTP.jet:439-464) maps 14 method names to `typed_*` free functions; `sema_call_try_web_form_method` (:469-529) lowers the call and makes the result infallible (:522); `sema_web_form_field_selector` (:566-590) turns `.carrier` or `.Carrier` into "carrier". `WebFormTyped` (Forms.jet:96-100) has no type parameter. `typed_validate_field` (:376-394) ignores an unknown name; `set`, `blur`, `typed_focus` and `typed_set_async_validator` (:168-198, :314-333, :444-453) return `WebFormError.Missing`. `web.form` skips computed fields (HTTP.jet:360). `forms.typed(input, action)` (:302) builds a typed form from any field list; `input_rename` changes only `wire_name` (:247-254).
- Prelude's `Type` (Prelude.jet:168) is a compiler-built value with a `priv` field: "Only the compiler builds one". Phantom generic parameters are allowed (syntax-decisions.md:2994).

## 1. Strongest existing solutions

### ACROSS1: one check about two calls

- **Roslyn analyzers**: `RegisterCompilationStartAction` + `RegisterCompilationEndAction` give an analyzer per-compilation state; a diagnostic carries `additionalLocations`. Closest to the author's C (rule memory), with the order problem solved by an end action.
- **Go `golang.org/x/tools/go/analysis`**: a pass sees a whole package; `Diagnostic.Related` points at the second place. That is B (`check_all`) plus `also:`.
- **ESLint**: rule state lives in the `create` closure for one file; `Program:exit` reports. Rule memory scoped to a unit.
- **rustc / Clippy / Rocket**: Clippy lints that pair items (e.g. `match_same_arms`) walk a whole body; Rocket detects route collisions at launch, run time only (D).
- **Go net/http (1.22) and axum**: both refuse conflicting patterns at registration, when run (D). Go's rule is exactly the shape-equality question F4 raises: two patterns conflict when one matches a request the other does and neither is more specific.
- **TypeScript Hono**: no build-time pairing at all; first match wins.

The strongest shape for a per-call rule system is a **key the compiler groups by**: the rule maps each call to a key, the compiler pairs equal keys in one table, and the rule answers each later call with its first partner. That keeps D-CORE-CHECK1's one-call answer, is linear, and is how SQL `UNIQUE` and Go's `ServeMux` registration think about duplicates. The author's draft did not have it; it is now A.

### FIELD1: naming a field of a model without a string

- **Swift `KeyPath<Root, Value>`** (`\.carrier` where the root is known): typed, unforgeable, carries the field. A's shape.
- **Kotlin `KProperty1<T, R>`** (`ShipForm::carrier`): typed, has `.name`. A's shape.
- **C# expression trees** (`x => x.Carrier`, FluentValidation) read when run: C's shape. **C# `nameof(ShipForm.Carrier)`**: text the build checks.
- **TypeScript `Path<T>` / `keyof T`** (react-hook-form): a string literal type checked against the model's keys; E with checking Jet's type system cannot give.
- **Elm** `.carrier` accessor functions: C's shape, and its gap (no name).
- **Diesel `table!`**: one generated column type per field: B's shape.
- **In Jet itself**: Prelude `Type`, a compiler-built value nobody can forge; `layout[.field]` in marker bodies, checked against the reflected type with Jet.UnknownField ("Typed selectors must name a field declared by the reflected type", Tests/UI/layout_unknown_selector); `devtools.publish(field: .x)`, checked against the package's `#DevPanel` state (Devtools.jet:35-75).

The strongest existing solution is the key-path family. A matches it, with Jet's leading dot in place of the backslash and Jet's `Type` as the in-repo precedent for a compiler-built Prelude value. The author's draft named Swift, Kotlin, C# and TypeScript but not `nameof`, Elm, Diesel, Prelude `Type` or `layout[.field]`.

## 2. Claims

### D-RULE-ACROSS1

- **C1. A keeps all six E2804 snapshots.** Holds within limits. The author's stand-ins were ordered by span, which breaks `http_route_ambiguous_overlap` where the convention file registers after the explicit route (F5). Registration order is needed. Repaired A states it.
- **C2. StaticRoutes/E2808 has already moved to a rule.** Fails on dev (F1). It is on two land branches in batch 23. Repaired A names it as a gate.
- **C3. CompilerCall rows at CoreRecordRows.jet:254.** Fails, minor: the rows are :250-269 (F2).
- **C4. "Same receiver" is "same table", and no program is newly accepted or refused.** Fails (F3). Probe p1: today E2804 wrongly refuses (a) exclusive if/else arms (21:16 labeled 19:16), (b) a reassigned router (30:12/28:12), (c) `pair.api`/`pair.admin` (37:16/36:14). It catches (h) a chain off a binding. It misses (d) a loop, (e) a helper parameter, (f) an alias and (g) `app: web.app().get(..)` then `app.get(..)`. Keying by root binding inherits a, b, c and still misses g. Repaired A defines "same table" as the same place (binding plus field path, no reassignment) or one builder chain, the earlier call dominating the later, and lists the unseen pairs as a loss.
- **C5. One definition of duplicate.** Fails (F4). Probe p4: the chain path refuses `/:a/b` against `/a/:b` while the same calls on a binding build and run ("binding registered all four"). The compiler has two definitions today. Repaired A uses run-time shape equality in one rule.
- **C6. Stand-ins need nothing new.** Fails (F6, F7). Probe p2: a double mount is reported at 7:5/7:5; stand-in staging keys (owner + rule span + site span) collide when one call registers twice. Convention rows have no `CompilerArg.function` until c30, so a stand-in needs `CompilerCall.source: String?`, and the site key must include the registering call.
- **C7. The inventory is complete.** Fails (F8, F12). `AmbiguousRoute` has no source variant and its text is copied three times; `jet explain --routes` reads the same table and was not listed.
- **C8. App and package routers are covered by one rule.** Fails (F9). The rule as drafted keyed off HTTPRouter; App (Core/Web) needs the same marker. Repaired A uses one public `#StaticRoutes` marker on HTTPRouter, App and package routers.
- **C9. Handing a rule its earlier calls "costs little".** Fails (F10). Probe p3: about 0.19 ms per heard call on a loaded host; with 500 routes the list form copies 124,750 calls, against c22's 1 µs per call. The keyed form is linear.
- **C10. "A rejection replaces the call's own errors" stays.** Fails (F11). Probe p5 prints E2804 and `Jet.UnknownName` (12:73) today; under D-CORE-CHECK1=A the second would vanish. Repaired A states a two-call finding (`also:`) is reported beside the call's errors, an exception named as a loss.
- **C11. Comparisons.** Holds within limits (F13): Go analysis `Related`, Clippy, axum, Spring, Rocket and Hono were missing.
- **C12. Other options are fairly stated.** Holds within limits (F14, F15). B at its strongest answers by position, but still makes every rule compute "same table" itself; pairing across check passes was unstated.

### D-FORM-FIELD1

- **D1. Today `.carrier` becomes unchecked text; a typo builds and renders `carier:invalid`.** Holds. Probe f1 (`check` rc 0; `run` prints `carrier:valid`, `carier:invalid`, `Carrier:invalid`, `false`).
- **D2. No private typing works: the no-argument `validate()` is a second meaning and Jet has no overloading.** Holds (HTTP.jet:439-464 maps `validate` to two functions by argument count).
- **D3. The form's type does not carry its model.** Holds (Forms.jet:96-100).
- **D4. A adds "one typing rule beside the subject shorthand, chosen by the expected type in the same way".** Fails (F16, F17). D-SUBJECT-CALL1=A sends every non-callable context to the full lambda and adds no second member mechanism; A adds a context, so it amends that decision. D-LOOP-SUBJECT1=A resolves the loop item first. Probe f3 (`out: loop ships -> "{.id} {typed_form.validate(.carrier, …).render()}"`) builds and prints `[1 carrier:valid]` today, because the intercept reads the written selector before typing. Under A with the ratified order, `.carrier` there is `it.carrier`, a String, so the program is newly refused. The draft named neither.
- **D5. A's run-time loop refuses an unknown name with `WebFormError.Missing`.** Fails (F18): A's signature `-> WebFormValidationChain E!` had no `WebFormError`. Repaired: `(E | WebFormError)!`, as `set_async_validator` has.
- **D6. The selector, its label binding and the `validate` branch leave the compiler (HTTP.jet:439-600).** Holds within limits (F22, F27). The file has 590 lines. Only `validate`'s branch left; 13 other methods stayed intercepted and `set`, `blur`, `focus` still named fields by text, two conventions on one type. Repaired A declares every typed-form method in Core and deletes HTTP.jet:439-590 whole, plus Items.jet:2370's compiler-owned entry.
- **D7. `validate()` becomes `validate_all()`; nothing outside the conformance corpus calls it.** Holds as a fact: no program calls the method (the corpus calls the free function `typed_validate`). But a whole-value `x.validate()` is a repo-wide convention (JetDevServer, Core/Encoding XML and others), and `validate_all` breaks it for one type (F23). Removing the method is smaller: `submit()` validates the whole form (Forms.jet:402) and `forms.typed_validate(form)` stays. Repaired A removes it.
- **D8. The check gets the field's value "as the wire carries it".** Holds within limits. `FieldRef` names `name`, and `input_rename` changes only `wire_name` (Forms.jet:247-254), so the name match is right; the wording is kept because the value is the wire text.
- **D9. Devtools' selector is checked.** Holds (Devtools.jet:40-75). The draft did not say why it stays; repaired A does (its call has no state type to fill a `FieldRef` from).
- **D10. A's `FieldRef` promise covers every typed form.** Fails (F20, F21). `forms.typed(input, action)` with `input_exclude`, `input_group` or `input_replace` makes forms whose fields differ from any model, and computed fields are skipped by `web.form`. Repaired A: stored fields only; `forms.typed<Model>` names the model for a hand-built form; the run-time Missing stays for that case.
- **D11. B, C as drafted.** Hold. B: `ShipFormField` cases are upper-case, so the spelling changes or every lower-case `.name` changes meaning. C: needs D-RULE-REWRITE1 A, c30 and D-RULE-BODYFACTS1, none landed.
- **D12. D "carries a hidden model tag through every binding".** Fails (F24). D at its strongest makes the same visible `WebFormTyped<Model>` change A needs and checks the name in `sema_web_form_field_selector`, about 20 lines. It is the smallest build-time catch, keeps f3 building and amends no decision. Its real cost is #6851's goal: Core keeps 14 methods no library can declare.
- **D13. E: "Making it generic brings back A's type change without A's checking."** Fails (F25). A call rule could check text against the model only if it could read the model's fields; `CompilerArg.type` is a TypeId row with identity, name and signature only (CoreRecordRows.jet:616-618). So E with build-time checking needs A's type change and a new reflection read.
- **D14. Comparisons.** Holds within limits (F26).
- **D15. The smallest fix for the `.carier` typo.** Not claimed by the draft; answered here (F19). The run-time refusal is a bug fix every option should include: `typed_validate_field` returns `WebFormError.Missing` for an unknown name like its four siblings. It needs no ballot. The smallest build-time fix is D as repaired.

### Checklist (ballot-review.md)

| Check | ACROSS1 | FIELD1 |
| --- | --- | --- |
| Every claim verified with code or a probe | p1-p5; C1-C12 | f1-f4; D1-D15 |
| Strongest existing solution named | keyed pairing, now A | key paths; A matches |
| Edge cases attacked | branches, reassignment, field paths, loop, alias, parameter, double mount, shape overlap | bindingless loop, hand-built forms, computed fields, generic helpers, upper-case selector, forged reference |
| Missing option added | keyed pairing (A) | D strengthened to the visible model; run-time baseline stated |
| Whole mechanism inventoried | three E2804 sites, AmbiguousRoute copies, `jet explain --routes`, App | 14 methods, Items.jet:2370, registry rows, three records, devtools, table |
| Ratified decisions checked | D-CORE-CHECK1=A exception named | D-SUBJECT-CALL1=A amended, D-LOOP-SUBJECT1=A kept, D-DX-FORM1=A spelling kept |
| One way (I8) | one marker for HTTPRouter, App, packages | one field-naming form on every typed-form method |
| Cost | linear keyed form vs quadratic list (p3) | negligible [INFERENCE] |
| Validate.jet | only the draft gap | only the draft gap |

## 3. Findings (round 0)

### D-RULE-ACROSS1

- **F1.** The draft said StaticRoutes/E2808 had moved to a rule. On dev it has not; it is on `land/6851-c3-static-routes-e2808-r2` and `rule-order-r2` (batch 23). A gate for A.
- **F2.** CompilerCall rows cited at :254; they are CoreRecordRows.jet:250-269.
- **F3.** "Same receiver" as "same table" by root binding inherits today's false refusals (p1 a, b, c) and misses p1 g. The claim "no program newly accepted or refused" was wrong.
- **F4.** Two definitions of duplicate exist today (p4): the chain path refuses an overlap the binding path accepts. One rule must use run-time shape equality.
- **F5.** Stand-ins ordered by span break `http_route_ambiguous_overlap`; registration order is needed.
- **F6.** Stand-in staging keys collide on a double mount (p2).
- **F7.** Convention rows need `source`, since `CompilerArg.function` is None until c30.
- **F8.** `AmbiguousRoute` has no source variant; its text is copied in three places.
- **F9.** App (Core/Web) was not covered; one public marker on HTTPRouter, App and package routers is needed (I8).
- **F10.** The earlier-calls list is quadratic, against c22's 1 µs per heard call (p3: about 0.19 ms per heard call; 500 routes copy 124,750 calls).
- **F11.** Under D-CORE-CHECK1=A a rejection replaces the call's own errors; p5 shows E2804 plus a second error today. A two-call finding needs an exception.
- **F12.** `jet explain --routes` (WebGraph.jet:689-923) was not inventoried.
- **F13.** Comparisons lacked Go analysis `Related`, Clippy, axum, Spring, Rocket and Hono.
- **F14.** B at its strongest answers by position, yet still makes each rule compute "same table".
- **F15.** Pairing across check passes was unstated.

### D-FORM-FIELD1

- **F16.** A amends D-SUBJECT-CALL1=A (a new non-callable context for `.name`) and did not say so.
- **F17.** Under D-LOOP-SUBJECT1=A, a selector inside a bindingless yielding loop reads the loop item; f3 builds today and A refuses it. The draft did not name the loss or the targeted error.
- **F18.** A's text promised a run-time `WebFormError.Missing` its signature could not return.
- **F19.** Today `typed_validate_field` is the only field-naming typed-form function silent on an unknown name; its siblings return `Missing`. This is a run-time bug fix every option needs, independent of the ballot.
- **F20.** Hand-built typed forms (`forms.typed`, `input_exclude`, `input_group`, `input_replace`) have no model the build can check; A did not say what their `Model` is.
- **F21.** Computed fields are skipped by `web.form` (HTTP.jet:360); A's rule must name stored fields only.
- **F22.** A left `set`, `blur` and `focus` taking text and 13 methods intercepted: two ways to name a field on one type (I8), and an incomplete inventory of the mechanism (registry rows WebCoreSignatures.jet:15, :60-72; Items.jet:2370; the three records holding a form, Forms.jet:114-128; the conformance corpus).
- **F23.** `validate_all()` breaks the repo-wide whole-value `validate()` convention; removing the uncalled method is smaller.
- **F24.** D was stated with a hidden tag; at its strongest it is the smallest build-time fix with a visible model.
- **F25.** E's claim about a generic call rule was wrong: a rule cannot read a TypeId's fields.
- **F26.** Prior art missing: C# `nameof`, Elm accessors, Diesel columns, Prelude `Type`, `layout[.field]` and its Jet.UnknownField wording.
- **F27.** Line references: HTTP.jet:439-600 runs past the 590-line file; checkInstructions cited 491390130f lines. Updated to 2a09ee0e42.
- **F28.** Aside, outside both ballots: in a top-level `CARRIER: #run ShipForm.reflect().layout[.carrier].name` the typed selector leaks its internal name: `Jet.UnknownName: Nothing named `\u{0000}jet.layout.selector.carrier` exists here` (f4).
- **F29.** Aside, outside both ballots: a failing validator propagates its error out of the caller (f2: "Error: carrier required ... run (main.jet:23)") rather than marking the field invalid. The JetWeb example relies on the intercept's `sema_owned_infallible` (HTTP.jet:522) hiding this; under A or E the method's error type is visible. [INFERENCE] fallibility is inferred, so Mia's spelling still compiles, as f2's `run` did.
- **F30.** Core's table library (`table.column("id", …)`, `sort_by(t, "id", …)`, JetWeb run.jet:201-205) names row fields by text, the same concept. Named in A as a later user of `FieldRef`; not in this ballot's scope.

## 4. Re-review log (round 1)

Round 1 re-attacked each repaired text after the round 0 fixes.

### D-RULE-ACROSS1

- **R1.** The repaired A still relied on D-CORE-CHECK1=A's "a rejection replaces the call's errors" without naming the exception for `also:` findings. Fixed: named in the technical text and as a loss.
- **R2.** Hearing every App method call to find route registrations costs every App call a dispatch. Fixed: named as a loss.
- **R3.** The marker was drafted `priv`, so App in Core/Web and package routers could not carry it. Fixed: public `#StaticRoutes`.
- **R4.** One registration call inside a loop registers many routes the build cannot see. Fixed: added to the not-seen loss.
- **R5.** If a later rule needs a non-key relation (one pattern shadowing another), the keyed form cannot express it. Fixed: B is kept as the hybrid for that case.

After round 1 ACROSS1's recommendation holds within its gates: the StaticRoutes landing (batch 23) and c22's cost bound.

### D-FORM-FIELD1

- **R6.** A built `FieldRef` as a plain struct with a public `name`, so any program could write `FieldRef<ShipForm, String>{name: "carier"}` and defeat the check. Fixed: `priv name` read through `name()`, built only by the compiler, as Prelude `Type` is.
- **R7.** A helper generic over the model (`fn<M>(form: WebFormTyped<M>)`) cannot write `.carrier`. Fixed: A says such a helper takes a `FieldRef` from its caller, which is also Leo's library pattern.
- **R8.** `forms.typed<Model>` cannot infer `Model` from its arguments, so every hand-built form, the conformance corpus's included, writes it once. Accepted: named in A's loss mitigation.
- **R9.** Extending `FieldRef` to `set`, `blur` and `focus` changes three `set("…", v)` lines in JetWeb, a spelling D-DX-FORM1 did not ratify either way. Accepted: named as a loss; the owner sees it.
- **R10.** Re-checked D after A's repair: D still keeps f3 building and amends no decision, which A cannot. Stated in D's text so the owner can weigh the smallest fix against #6851's goal.

After round 1 FIELD1's recommendation A holds within limits: it amends D-SUBJECT-CALL1=A by one context, refuses f3's loop form, and changes three `set` lines.

## 5. Overall verdict

**D-RULE-ACROSS1: recommendation changed, holds within limits.** The keyed pairing (A) replaces the author's quadratic earlier-calls list (now B). Gates: the StaticRoutes/E2808 landing (batch 23) and c22. The probes show today's E2804 is wrong in three ways and misses four, so the ballot no longer claims "no program changes".

**D-FORM-FIELD1: recommendation A holds within limits.** It now states the D-SUBJECT-CALL1=A amendment and D-LOOP-SUBJECT1=A's order (f3 refused), covers every field-naming method and hand-built forms, removes `validate()` instead of renaming it, and presents D as the smallest build-time fix. The run-time refusal (F19) is a bug fix no ballot needs.

| Claim | Round 0 | Round 1 |
| --- | --- | --- |
| C1 six snapshots | holds within limits | holds (registration order) |
| C2 StaticRoutes moved | fails | holds (gate named) |
| C3 row cite | fails | holds |
| C4 same table | fails | holds within limits (unseen pairs named) |
| C5 one duplicate | fails | holds |
| C6 stand-ins | fails | holds within limits (c30: `source`) |
| C7 inventory | fails | holds |
| C8 App coverage | fails | holds |
| C9 cost | fails | holds within limits (c22) |
| C10 rejection rule | fails | holds (exception named) |
| C11 comparisons | holds within limits | holds |
| C12 other options | holds within limits | holds |
| D1 silent typo | holds | holds |
| D2 no private typing | holds | holds |
| D3 no model on type | holds | holds |
| D4 typing rule beside shorthand | fails | holds within limits (amends D-SUBJECT-CALL1; f3 refused) |
| D5 run-time Missing | fails | holds |
| D6 intercept leaves | holds within limits | holds |
| D7 `validate()` rename | holds within limits | holds (removed) |
| D8 wire name | holds within limits | holds within limits |
| D9 devtools | holds | holds |
| D10 every typed form | fails | holds within limits (hand-built forms checked when run) |
| D11 B, C | holds | holds |
| D12 D | fails | holds |
| D13 E | fails | holds |
| D14 comparisons | holds within limits | holds |
| D15 smallest fix | (not claimed) | holds (F19 baseline; D smallest build-time) |
| Format | fails | holds (draft:false left) |

Proposed `reviewPasses.adversarial` lines, which pass `Validate.jet` with only the draft gap left (both are already in the ballot files):

> D-RULE-ACROSS1: Author model family: Claude Opus. Adversarial model family: Claude Opus. Fresh agent: AcrossReview. The adversarial review attacked the recommendation. Probes on tip 21 found three wrong refusals and a chain-only overlap test, and the drafted list was quadratic, so a keyed form now leads as A and the list is B.

> D-FORM-FIELD1: Author model family: Claude Opus. Adversarial model family: Claude Opus. Fresh agent: AcrossReview. The adversarial review attacked the recommendation. Probes on tip 21 showed A refused a selector in a loop with no binding, so A now names that and its D-SUBJECT-CALL1 change, covers every field method and drops `validate()`.

## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| F1 | card | #6851 |
| F2 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| F3 | card | #6851 |
| F4 | card | #6851 |
| F5 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| F6 | card | #6851 |
| F7 | card | #6851 |
| F8 | card | #6851 |
| F9 | card | #6851 |
| F10 | card | #6851 |
| F11 | card | #6851 |
| F12 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| F13 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| F14 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| F15 | card | #6851 |
| F16 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| F17 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| F18 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| F19 | card | #6851 |
| F20 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| F21 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| F22 | card | #6851 |
| F23 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| F24 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| F25 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| F26 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| F27 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| F28 | no-action | report-only run: outside both ballots; reported to Main for a card, no Tower writes authorized here |
| F29 | no-action | report-only run: outside both ballots; reported to Main for a card, no Tower writes authorized here |
| F30 | no-action | report-only run: named in D-FORM-FIELD1 A as a later user; no Tower writes authorized here |
| R1 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| R2 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| R3 | card | #6851 |
| R4 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| R5 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| R6 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| R7 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| R8 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| R9 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| R10 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
<!-- /audit-dispositions -->
