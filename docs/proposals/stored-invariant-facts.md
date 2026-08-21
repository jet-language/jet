# Stored invariant facts

Status: design note for card #2140. The ballots below are drafts. No syntax or
semantic choice is ratified by this file. This card adds no compiler code.

## Result

Jet has the right substrate but not the storage attachment. `Type` already
projects compile-time knowledge into one `KnowledgeVector`; the vector carries
identity-bearing facts and recursively records facts at container paths. The
range implementation uses this path today. Typestate uses the same fact law,
but its value attachment lives only in `FlowFacts` and is intentionally lost
when a value enters a field, a non-local receiver, or a loop-carried position.

The missing middle is a registered predicate plane:

1. A nominal declaration owns one or more registered predicate members.
2. A checked constructor or transition is the only producer that may attach a
   predicate member to its result type.
3. The result's predicate member enters the type knowledge vector at the empty
   path, so fields and containers carry it through normal type composition.
4. Call-site checking compares registered facts by the plane's subsumption
   rule. It never compares prose, method names, or a constructor's reputation.
5. The knowledge vector erases at the typed-IR seam. No flag, wrapper, or
   discriminant reaches AOT, JIT, interpreter, or web codegen.

This makes state syntax and distinct wrappers possible surfaces over one
mechanism. It does not make either surface the mechanism.

## What exists

| Area | Current home | Gap |
| --- | --- | --- |
| Typestate vocabulary | `StateTable` and the shared fact registry | The live state is a `FlowFacts::states` row, not a type fact. |
| Numeric ranges | `KnowledgeVector` on `Type::IntN`, inline ranges, and distinct ranges | The projection is numeric-specific. |
| Nominal types | `TypeRegistry::TypeDef`; distinct types already retain a knowledge vector | Struct and ordinary nominal names do not project user predicate facts. |
| Plane registry | `Prelude/Facts.jet` through `jet_foundation::Registry` | It registers plane law and reflection shape, not user predicate producers. |
| Smart constructors | Private fields plus public fallible constructors | Privacy protects construction, but the return type carries no reusable fact. |

The important seam is already present. `KnowledgeVector::extend_at` is the
composition rule for `List`, `Map`, `Option`, `Result`, tuples, fixed lists,
and generic arguments. A new flow store or wrapper hierarchy would duplicate
that rule.

## Proposed semantic contract

### Predicate declaration

The registry gets one identity-bearing value plane for user predicates. User
predicate names are members of that plane; they are not new Rust enums, hidden
runtime tags, or arbitrary strings in reflection. The row records:

- the carrier or nominal owner;
- the predicate member and its canonical identity;
- the safe direction (`Gain`);
- the allowed producer forms;
- the plane's subsumption and composition rules;
- the typed reflection row.

Do not use `Type.Classification` for this. Classification is a flow tag and is
not identity-bearing. Do not use a free-form `#Post` message as the predicate.
The registry must own the fact that a call site can consume.

The existing `fact Name(@holds: …)` declaration is a registry-row declaration,
not yet a value-predicate declaration. Reusing that word is a surface ballot,
not an assumption in this note.

### Proof-producing operations

A constructor or transition may attach a predicate only when sema can point to
the registered proof producer. The first implementation should require the
producer to invoke the predicate's canonical checked validator, or use a
bounded sema proof already owned by the plane. An arbitrary pure Boolean,
message string, or author assertion is not enough.

`#Pre` and `#Post` stay ordinary contracts unless a clause names the registered
predicate in the registered form. A linked postcondition can establish the
result fact after its check. An unrelated postcondition containing the word
`unit` cannot. A producer that has no registered proof stays unrefined or fails
sema; it never receives a fact by declaration alone.

For a fallible constructor, success carries the fact and failure carries the
ordinary error. A transition that changes the predicate consumes the old
knowledge and must prove the new member. An in-place mutator must be rejected
unless its registered proof preserves the predicate. A raw projection is a
knowledge-loss boundary and uses the existing explicit gate law.

### Storage and calls

The fact is attached to the nominal result's type identity, not to one local.
The sema nominal-type projection supplies the registered entries to the
existing `KnowledgeVector`; it does not keep a second nominal-fact cache.

The ordinary composition law then gives these results:

- a field typed as the fact-bearing nominal type carries the fact when read;
- `[FactType]`, `[K: FactType]`, `FactType?`, `FactType ! E`, tuples, fixed
  lists, and generic arguments carry it at their existing structural paths;
- inserting or assigning an unproven carrier into a fact-bearing slot fails at
  the existing type/proof seam;
- a branch or loop can retain a stored fact because the fact is in the type,
  while flow facts still join conservatively for values whose type is plain;
- generic code receives only facts present in its type arguments. It cannot
  mint a predicate by calling a method whose registry row lacks a proof.

A required call fact is satisfied by a supplied fact through the plane's
subsumption relation. A `#Pre` condition is discharged only when its condition
is the same registered predicate or a registered implication. Arbitrary
Boolean `#Pre` conditions remain runtime contracts. This preserves the
distinction between a proof and a test.

Nominal identity remains nominal. This proposal does not restore implicit
`distinct`-to-base conversion. Whether a fact-bearing value may satisfy a
plain-carrier parameter with a predicate precondition is a separate ballot;
the safe default is to require compatible carrier identity and preserve the
fact across the call.

### Erasure and reflection

The predicate plane follows D-TYPE2-FOUND1 and D-TYPE2-PLANE1. It contributes
to type identity where the row says it does, is readable through the existing
typed fact/reflection path, and disappears through `Type::erased_carrier` at
the typed-IR seam. If validation runs at construction, its meaning lives in
one Prelude validator symbol; every execution tier calls that symbol through
its normal adapter. No engine may reimplement predicate policy or error
meaning.

## Draft ballot slate

The IDs are labels for owner discussion only. They are not entries in
`docs/spec/syntax-decisions.md`.

### D-STORED-FACT-REP1 — where stored invariant facts live

- **A — state-carrying field types.** Extend typestate so declared states are
  part of field and container types. This keeps the current state vocabulary,
  but needs a stored-state type surface and a second set of rules for state
  joins, mutation, and generic storage. It does not naturally cover arbitrary
  predicates.
- **B — distinct-with-fact wrappers.** Generalize `distinct` from numeric
  ranges to nominal carriers with a declared predicate. Storage is easy, but
  every new wrapper becomes a new surface and the wrapper still needs a proof
  registry, composition rules, reflection, and generic propagation.
- **C — registered predicate plane.** Put user predicate members in the
  existing type knowledge vector and let state or distinct syntax, if later
  chosen, project onto it. This reuses identity, path composition, subsumption,
  reflection, and erasure.

**Recommendation: C.** A and B may become spellings over C. Neither should
become a second fact carrier.

### D-STORED-FACT-PROOF1 — what can mint a fact

- **A — declaration assertion.** A constructor or transition declares its
  result fact. Reject: this is the video author's false `cross(unit, unit)`
  pattern in another form.
- **B — canonical checked producer.** The registry names the validator or
  bounded prover. Sema accepts the fact only when the constructor or transition
  uses that producer and its success path returns the registered carrier.
- **C — unrestricted theorem proving.** Let sema prove arbitrary user
  predicates. Keep as a later research path; it creates solver, timeout, and
  proof-explanation scope before the storage model exists.

**Recommendation: B.** The producer relation is the proof obligation. Runtime
validation may establish a fact on constructor success, but the consumer never
trusts a name or comment.

### D-STORED-FACT-SUBSUME1 — how obligations compare

- **A — exact member only.** A required predicate matches only the same
  registered member. Small first slice; no mathematical implication.
- **B — registered bounded implications.** Each plane owns explicit, checked
  implication/subsumption edges. Interval arithmetic remains the range plane's
  algebra; user predicates do not pretend to be numeric intervals.
- **C — arbitrary predicate implication.** Ask a general solver to decide
  implication at every call. Reject for the first slice: cost and failure
  explanations are unbounded.

**Recommendation: B, with A as the initial implementation rung.** Add an edge
only when the plane can prove it. An author's claim that two unit vectors are
perpendicular is not such an edge.

### D-STORED-FACT-SURFACE1 — user-facing declaration

Any user-authored predicate or producer relation is new semantics and needs a
separate syntax ballot. Candidate homes are the existing `fact` declaration,
the nominal type declaration, or the constructor/transition declaration. This
card does not choose a spelling, add a keyword, or add a marker. The next card
must name the exact syntax, registry row, proof obligation, diagnostics, and
privacy rule before code starts.

## Worked meaning

Conceptually, a private `Vec3` carrier has a checked constructor that returns a
fact-bearing nominal `NormalizedVec3`. A record field and a list element typed
as `NormalizedVec3` retain the same registered fact after storage. A consumer
that requires the fact accepts those values without another length check. A
constructor for `cross(a, b)` returns the fact only when its registered proof
handles the actual angle; two `Unit` inputs alone are insufficient.

No source spelling in this paragraph is proposed. It describes the semantic
test that a future spelling must pass.

## Acceptance terms for the next implementation card

- one predicate plane and one registry source;
- one nominal projection into `KnowledgeVector`;
- field, container, branch, loop, generic, and return propagation tests;
- a negative test for an unproven constructor and for a false predicate
  implication;
- `#Pre` discharge tests for exact fact, registered implication, and arbitrary
  Boolean conditions;
- reflection tests from the same registry row;
- proof that the typed-IR carrier and all applicable execution tiers contain
  no predicate storage or engine-local predicate policy;
- registered diagnostics and `tests/ui/` snapshots for every new rejection;
- no `jit_gaps` entry and no AOT-only exception.
