# Adversarial review: D-RULE-TYPEARGS1 draft

Date: 2026-11-23. Reviewer: DerivReadReview, a fresh agent. The draft is
TypeIDSlice3's.

Subject: the draft ballot for card #6851 (c6, the move of E2521 into Core).
It asks how a marker's `check` rule reads the type arguments of the types a
call passes. The draft recommends A: TypeId carries `generic_bindings`, and
TypeInfo's field moves to `identity.generic_bindings`.

Trees. Most cites were checked on dev (Main-dev-collectall, ef3967732b).
`Compiler/JetSema/Source/Sema/Calls/CheckRules.jet` is not on dev or in any
dev commit. Its author pointed to batch tree e4d5e24c (orchestrator scratch
`batch12-tree`), and its cites were checked there. Reflection.jet,
FactReads.jet, BuildTypes.jet and IR/Host.jet read the same at the cited
lines in both trees.

Probes ran on the batch-11 s1 compiler, with the host's load average at about
50 on 32 threads. The probe files and a results table are in the
orchestrator's scratch probe directory (`RESULTS-typeargs.md`).

## Strength 1: a rule reads type arguments with no round trip (A)

Peers' best case. C# and Scala 3 hand an analyzer or macro a type that
carries its arguments:

- C#: Roslyn's `INamedTypeSymbol.TypeArguments` "returns the type arguments
  that have been substituted for the type parameters" (learn.microsoft.com,
  Microsoft.CodeAnalysis API).
- Scala 3: `TypeRepr.typeArgs` is "the applied type arguments (empty if there
  is no such arguments)", and `=:=` compares two types (scala/scala3,
  `library/src/scala/quoted/Quotes.scala`).

That is A's shape.

Attack. The draft says "the compiler holds them as structured types when it
builds the record". Only half of that holds.

- The TypeId builder is handed a `Type`, and a specialization is a
  `Type.Apply` with its arguments
  (`Compiler/JetSema/Source/Sema/Comptime/Reflection.jet:1519-1576`).
- The parameter *names* are not in the `Type`. TypeInfo's bindings get them
  by finding the struct or enum declaration in the registration graph
  (`Reflection.jet:3066-3072`) and pairing each type parameter with its
  argument (`Reflection.jet:2379-2389`).
- Three makers have no graph to look in:
  - `sema_comptime_reflection_written_type_id` and `_type_id_of` name a type
    as written (`Reflection.jet:1536-1557`);
  - derive templates build text-only TypeIds before the item is placed
    (`Compiler/JetSema/Source/Sema/Registration/Derives.jet:2198-2203`);
  - fn build's `add_struct` and `find_type` answer TypeIds from the build
    host (`Compiler/JetSema/Source/Sema/Calls/Build.jet:580, 609`;
    `Compiler/JetDriver/Source/Driver/BuildAdapter.jet:362-410, 631-633`).
  Each maker must name the parameters the same way. Otherwise two TypeIds
  of one type differ as records.

Verdict: holds, as repaired. The ballot names every maker, and adds a loss
with a parity case as the mitigation.

## Strength 2: one meaning everywhere, "nested as deep as the types are" (A)

Attack.

- Bindings exist only for specializations of declared generic structs and
  enums (`Reflection.jet:3066-3072`). A list, option, map, tuple or result
  type has no declared parameter names, so A gives it an empty list.
- In the probe's `Shelf<Pair<String, Int>>`, every generic part of every
  field and return type sits under a list or an option:
  - `[Pair<…>]`
  - `Pair<…>?`
  A gives none of them bindings, so a rule walking those TypeIds reads no
  argument at all.
- "Running code" holds: the probe reflects in `run` and prints at run time.
- On this compiler, a method's `self` ParamInfo reflects with an empty
  identity. That is a separate defect; this ballot doesn't depend on it.

Verdict: failed as written, and now repaired. A's detail and losses say
that built-in shapes list nothing, so a list's element type stays unread,
and the expert path says "nested through declared generic types".

## Strength 3: A keeps rules within D-CORE-CHECK1's 1 µs per call

Attack. The draft says "D-CORE-CHECK1=A measured rules within 1 µs per
call". The ratified ballot's technical text instead says "The budget is at
most 1 µs per rule call, checked by rerunning that probe with a rule in
place of the watch".

- Its review audit measured only the watch, at about 54 µs per heard call
  (`Docs/Audits/2026-11-23-d-core-check1-review.md:141-153`).
- The card's log repeats the same budget.
- No later measurement exists in dev or in e4d5e24c.

A's per-call cost is static data, not a round trip. That is the right side
of the budget, but its size in bytes and time is unmeasured.

Verdict: failed as written, and now repaired. The ballot calls it a budget
"not measured yet", and claims only "static data in each batch".

## Strength 4: C costs about twenty times the budget

The draft took 5.49 to 10.63 µs per round trip from
`Docs/Audits/2026-11-23-comptime-child-design.md:44-48`.

Attack. Those rows come from "an ephemeral two-process Linux
fork/pipe/`MAP_SHARED`/`process_vm_readv` microbenchmark (compiled via
`jet cc -O3` in scratch)" (same audit, line 42). It timed compiler-to-child
calls. It is not the host route that C would use, which runs from the child
to the compiler.

Probe. I timed real kind 1 host routes:

- `jet check` on a `#run` loop of 20,000 calls, minus the same file with 0
  calls;
- the minimum of five interleaved rounds.

| call | per call |
| --- | --- |
| `field.marker(Hidden)` on a FieldInfo | 43.4 µs |
| `info.has_method("first")` on `Pair<Int, String>`'s TypeInfo | 228.6 µs |

Both calls are answered by `jet_eval_ir_host_answer`
(`Compiler/JetEval/Source/IR/Host.jet:204-209`; `ReflectionMethods.jet:10-16`).
The second ships a whole TypeInfo across the route, as C's answer would.

The figures fit the 54 µs per heard call that D-CORE-CHECK1's review
measured for one exchange.

So E2521's two reflects cost about 0.1 to 0.5 ms per heard call, 90 to 460
times the budget. The draft understated C's cost by 4 to 40 times.

Fairness to C. A memo by identity makes the cost one round trip per
distinct type per batch. With few distinct transform types, a batch would
pay a few hundred µs once. The ballot keeps that remedy in C's technical
text and its why-not.

Verdict: the conclusion holds, and is stronger; the number was repaired.

## Strength 5: the fact moves rather than doubles; the cutover is small

Attack. The draft names one reader to move, `FactReads.jet:275`. It also
names `FactReads.jet:540` as TypeInfo's binding, but line 540 is
CallableSignature's, which A keeps.

The cutover is wider:

- Rows and records:
  - `Compiler/JetFoundation/Source/Registry/CoreRecordRows.jet:353` (TypeInfo) and 601-603 (TypeId);
  - `Compiler/JetIR/Source/CompilerOwned.jet:134` (TypeInfo) and 186 (TypeId).
- Builders: `Reflection.jet:2814` (the empty list every TypeInfo carries), 3099 (the specialization's list) and 1567-1576 (the TypeId record).
- Field tables: `FactReads.jet:275` (TypeInfo) and 655-660 (TypeId).
- Makers without a graph: `Derives.jet:2198-2203`, and the build host's `add_struct` and `find_type`.
- User-visible readers:
  - `Examples/Metaprogramming/Reflection/CallableCatalog/run.jet:143`;
  - `Examples/Metaprogramming/Reflection/TypeFacts/run.jet:54-55`;
  - `Tests/Cases/FactReads/TypeInfoListsTraitsConstantsAndGenericBindings.fixture/main.jet:54`;
  - `Docs/Spec/Reference/metaprogramming.md:686-695` (TypeInfo) and 888-897 (TypeId).

`CallableSignature.generic_bindings` stays: it binds a function's own
parameters (metaprogramming.md:561-581). So the moved field never doubles.

Verdict: holds, as repaired. The cutover list is in A's technical text.
It is still a greenfield cutover with no alias.

## Cost of A: growth and recursion

Main asked whether a binding list on every TypeId grows every reflected
record, and whether recursive generics stay bounded.

- Recursion. A TypeId lists only its own arguments, never their fields or
  methods. Its tree is therefore the written type term, which is finite.
  - The record is already recursive today through `signature`
    (TypeId → CallableSignature → ParamInfo → TypeId), so the layout has
    precedent.
  - A type nested d deep repeats each inner identity in every enclosing
    record, about d² characters [INFERENCE from the record shape].
- Growth, measured by counting. The probe reflected an `Xform<Float, Screen, World>`
  with Transform2's shape: 8 fields and 5 methods. Its TypeInfo holds 28
  TypeIds in field, parameter and return types, and A adds 21 binding
  records under them, 75% more TypeId records.
- Per heard `then` call, the receiver and argument TypeIds become about 8
  records instead of 2.
- Static data per batch and time per call under A: not built, so not
  measured.

## Strength 6: B is the wrong middle

Attack. B's figures hold: Transform2 has 8 fields and 8 methods
(`Core/Math/Math.jet:44604-44695`). The c3-c6 ruling deletes
`__then_float` and `__inverse_float`, which leaves 6 methods. B's flaw
remains: a TypeId reached by walking can't reflect in a rule, though the
same spelling works in marker bodies and fn build.

Verdict: holds.

## Missing option added: D, the signature states the check

Core already declares `then: fn<Next>(self, next: Transform<To, Next>)`
(`Core/Math/Math.jet:44538`). So a chain whose middle spaces differ fails
to type-check with no rule. The Geometry checker only words the failure as
E2521, before inference
(`Compiler/JetSema/Source/Sema/Expressions.jet:5001-5008`;
`Calls/Geometry.jet:220-237`).

This is Rust's shape. A procedural macro reads `syn` paths as written
tokens: `PathArguments::AngleBracketed` is "the `<'a, T>` in
`core::slice::iter<'a, T>`" (docs.rs, syn 2). Checks on resolved types are
trait bounds, which `#[diagnostic::on_unimplemented]` words.

D lets a parameter's marker name the registered error for its type mismatch.
Its fields are filled from the compared types.

Why it loses:

- it covers only what a signature can state;
- it is a second way to raise a registered error, beside the rules
  D-CORE-CHECK1 made the one mechanism.

Mia's units check would fit D. A "this unit must be a length" check would
not.

## Prior art

- C++26, P2996 (isocpp.org, P2996R13, §4.4.14 `template_of`,
  `template_arguments_of`). This is the strongest prior art.
  - A reflection `^^T` is an opaque handle like a TypeId.
  - `template_arguments_of` lists a specialization's arguments, each a
    reflection that can be queried in turn.
  - The query runs in the compiler, so it is C's surface with A's cost.
  - Jet can't copy that cost model, because rules run in an isolated child.
- C# Roslyn and Scala 3: sourced above; A's shape.
- Zig (ziglang.org, language reference, master):
  - "Generic Data Structures": a generic type is "a function that returns
    an anonymous struct", named `List(i32)` from the call.
  - `@typeInfo` lists fields and declarations, not arguments. That is
    Jet's position today: a name as text and no arguments.
- Rust `syn`: under option D above.
- Swift: SE-0382 (swiftlang/swift-evolution, `0382-expression-macros.md`).
  - A macro receives the syntax tree.
  - Type information is deferred to "a follow-on proposal", because it
    "would need to remain stable".
- Kotlin was dropped. The draft cited an FIR checker API without a source,
  and FIR is an internal compiler API.

## Other draft claims

- The call record's argument types come from
  `sema_comptime_reflection_type_id_in`: holds (CheckRules.jet:275 at
  e4d5e24c).
- Rule calls go to the child as one batch: holds (CheckRules.jet:1-10 at
  e4d5e24c).
- Host routes and their kind 1 answers: hold
  (`Compiler/JetIR/Source/BuildTypes.jet:919-960`; `IR/Host.jet:204-225`).
- The E2521 snapshot exists and reads `World` does not match `View`: holds
  (`Tests/UI/coordinate_transform_mismatch.stderr`).
  - Whether a Core rule reproduces it byte for byte depends on TypeId.name
    printing `Transform<Screen, World>` unqualified. That is unverified, and
    the same for A, B and C.
- `b.program.types()` lists declarations, not specializations: not checked
  [unverified]. If it lists specializations, C's fn build path is cheaper,
  but rules are unaffected.
- The orchestrator's earlier c3-c6 ruling said "TypeId gets no args field".
  Its later correction sends the question to this ballot, so A doesn't
  contradict a standing ruling.

## Verdict

Holds within limits, once repaired. A is still the right pick:

- C's measured round trip is 40 to 230 times the budget per reflect.
- B gives one spelling three answers.
- D covers only equality-shaped checks, with a second mechanism.

The draft could not have gone out as written. Its problems were:

- a budget presented as a measurement;
- C's cost understated 4 to 40 times;
- bindings claimed "nested as deep as the types are", while lists, options
  and maps carry none;
- a one-line cutover that is really about fifteen sites and three makers
  without a graph;
- a missing option;
- an unsourced peer.

The repaired ballot fixes each one and validates.

## Open gaps (unverified, with the cost if they fail)

1. A's static data per batch and time per heard call are unmeasured.
   - If it fails: rule batches outgrow the 1 µs budget.
   - Mitigation: rerun D-CORE-CHECK1's probe with the E2521 rule after the
     cutover.
2. Makers with no registration graph must name parameters identically.
   - If it fails: two TypeIds of one type compare unequal as records.
   - Mitigation: the parity case named in the ballot.
3. Element types of lists, options and maps stay unread.
   - If it fails: a package check over `[Quantity<U>]` can't move to a rule.
   - Mitigation: a later TypeId field.
4. The E2521 snapshot's exact text from a Core rule is unverified, and the
   same for every option.
   - If it fails: c6's byte-for-byte proof fails until the error's fields
     print as today.
5. The host route timings were taken under a load average near 50.
   - If they fail: C's gap shrinks, but even the floor of 5.49 µs per
     round trip is over the budget.

## Finding dispositions

<!-- audit-dispositions:v1 -->
|finding|disposition|target or reason|
|---|---|---|
|S1 rule reads type arguments with no round trip|card|#6851 (superseded by the interned-table redraft; see the 2026-11-24 second review)|
|S2 one meaning everywhere|card|#6851|
|S3 A within the 1 µs budget|card|#6851 (c22 measures rule dispatch at about 46.5 µs per heard call)|
|S4 C costs twenty times the budget|card|#6851|
|S5 the fact moves rather than doubles|card|#6851 (the redraft's cutover inventory)|
|S6 B is the wrong middle|card|#6851|
|Missing option D|card|#6851 (D is on the ballot)|
|Open gaps|card|#6851 (answered or carried by the second review)|
<!-- /audit-dispositions -->
