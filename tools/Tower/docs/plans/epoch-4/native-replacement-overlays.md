# Compatibility-proved native replacement overlays

**Card:** Tower #242. **Epoch 4.** **Scope:** implementation slice for
`D-WD15`, `D-JPK-REPLACEPOLICY1`, and `D-JPK-REPLACEPROOF1`.

## Goal

A native Jet package may replace a foreign package only after compatibility is
proved across public types, effects, errors, examples, and golden fixtures. Once
proved, call sites keep their foreign surface while Jetpack routes to the native
implementation through one authority graph and one lock rationale.

## Current Ratified Law

- `D-WD15`: native replacement overlays require compatibility proof across
  public types, effects, errors, examples, and golden fixtures before replacing
  a foreign surface without call-site rewrites.
- `D-WD5`: importers track native migration progress.
- `D-WD6`: foreign ecosystems are federated providers under Jetpack authority.
- `D-CFFI-CANON1` and related FFI law: foreign interop surfaces remain explicit
  and typed.
- `D-WD4`: lock records policy, owner, provenance, and semantic rationale.
- `D-WD1`: replacement authority is a grant/policy fact.
- `I8`: replacement is one semantic mechanism, not parallel ad-hoc shims.

## Vertical Slices

### T1. Replacement Candidate Facts

Provider metadata and importers emit candidate facts: foreign package identity,
native package identity, covered public symbols, unsupported symbols, license,
platforms, and proof status.

Exit: `jet info`/explain can show a candidate without enabling it.

### T2. Compatibility Model

Define internal compatibility records:

- public types and constructors;
- callable signatures and labels;
- effects and authority requirements;
- error shapes and diagnostic mapping;
- examples and expected output;
- golden behavior fixtures;
- platform constraints.

Exit: a fixture native package can declare a compatibility record as data in the
test harness without adding user syntax.

### T3. Proof Runner

Run compatibility proofs by compiling both surfaces against shared examples and
golden fixtures. Type/effect mismatches fail before runtime. Runtime fixtures
compare stdout, stderr, exit code, files, and declared side effects.

Exit: tests prove mismatched effects, missing public symbols, and divergent
golden output block replacement.

### T4. Overlay Resolution

When proof passes and policy allows replacement, the resolver substitutes the
native package for the foreign identity while preserving the foreign owner ref
in the lock. Call sites and imports do not change.

Exit: a package depending on a foreign fixture resolves to the native fixture
only when proof status is valid and policy grants replacement.

### T5. Lock And Audit

Lock records include foreign identity, native replacement identity, proof digest,
proof inputs, platform, owner package, policy grant, and rollback path. Audit
can explain why replacement happened and how to disable it.

Exit: lock merge keeps replacement rationale and conflicts when two owners pick
different replacements for the same foreign identity.

### T6. Importer Progress

Migration importers write replacement progress facts: no candidate, candidate
found, proof failed, proof passed, replacement active. This is status until
policy enables replacement.

Exit: importer fixture emits progress facts and explain output tracks them.

## Acceptance Tests

- `replacement_candidate_visible_but_inactive`.
- `compat_proof_fails_on_missing_symbol`.
- `compat_proof_fails_on_effect_mismatch`.
- `compat_proof_fails_on_error_shape_mismatch`.
- `compat_proof_fails_on_golden_output_diff`.
- `compat_proof_pass_enables_policy_replacement`.
- `replacement_preserves_foreign_call_site`.
- `lock_records_foreign_native_proof_and_policy`.
- `lock_merge_replacement_conflict_names_owners`.
- `importer_reports_replacement_progress`.

## Dependencies

- Federated providers, because foreign identity and metadata are provider facts.
- Migration importers, because they surface candidates and progress.
- Explainable lockfiles, because replacement rationale must be durable.
- Universal trust grants, because replacement is policy authority.
- Strict package graph/catalogs, because visibility still applies after
  substitution.
- Build/test/golden infrastructure for both foreign bridge and native package
  behavior.

## Ratified Decisions

- `D-JPK-REPLACEPOLICY1 (=A)` — native replacements are controlled by
  `policy.replacements`; compatibility proof is mandatory, and policy only
  chooses allow/deny/prefer.
- `D-JPK-REPLACEPROOF1 (=A)` — native replacement packages publish
  `replacementProof:` metadata naming the foreign package, public surface,
  effects, errors, examples, and goldens proved.
- `D-WD15 (=B)` — replacement overlays are allowed only after compatibility is
  proved across public types, effects, errors, examples, and golden fixtures.
