# Explainable semantic lockfiles and lock merge

**Card:** Tower #232. **Epoch 4.** **Scope:** planning slice for `D-WD4`.

## Goal

`.jet/lock` is both exact machine identity and a readable explanation of why
each identity was chosen. Merging two lockfiles preserves provenance,
package ownership, platform intent, policy grants, provider facts, and semantic
conflicts instead of treating the lock as opaque text.

## Current Ratified Law

- `D-WD4`: `.jet/lock` records exact identity plus rationale, provenance, owner
  package, policy, platform, and semantic merge support.
- `D-JPK-CACHE1=A`: hangar and lock schema include `output_hash`, platform,
  signature slot, and provenance.
- `D-JPK-OFFLINE1=A`: realize-class verbs must run network-free when the lock is
  satisfied.
- `D-JPK-CHANNEL1=A`: channel refs resolve only on first add/update; lock stays
  exact.
- `D-JPK-TOOLCHAIN1=A`: `[toolchain]` lock records resolved toolchain identity.
- `D-WD1`: grants and policy facts need one audit substrate.

## Vertical Slices

### T1. Lock Record Inventory

Enumerate every lock record shape: package, source ref, monorepo member,
adapter output, source build, cache object, toolchain, secret reference metadata,
service package, image input, fleet input, and future jetos activation closure.

Exit: a test fixture can serialize one record per kind and round-trip without
dropping unknown future fields.

### T2. Rationale Fields

Add rationale fields to records without changing their exact identity:

- who owns this entry;
- why it was selected;
- source ref and provider;
- channel input and exact output;
- platform;
- policy/grant fingerprint;
- build recipe id or adapter id;
- signature and cache provenance.

Exit: lock writer keeps exact machine fields stable while adding readable
rationale beside them.

### T3. Semantic Merge Engine

Parse both sides plus base into typed records and merge by semantic key, not
line position. Identical identities merge silently. Same key with different
compatible rationale keeps both owners. Same owner with conflicting exact
identity produces a lock conflict diagnostic with both rationales.

Exit: merge tests cover independent package additions, shared package same
version, shared package divergent exact version, platform-specific records, and
toolchain divergence.

### T4. Explain View Producer

Produce a stable facts view used by `jet explain`, future dossier sections, and
CI JSON. It must answer: why is this package here, who requested it, what exact
artifact is used, what grants/policies apply, what would update it, and whether
offline realization is satisfied.

Exit: fixture output names exact owner/rationale for a package, a toolchain, and
an adapter output.

### T5. Lock Update Discipline

Restrict lock mutation to network/update-class verbs or first realization.
Read-only explain, audit, env entry with a satisfied lock, and build with a
satisfied lock must not rewrite lock order or rationale text.

Exit: read-only verbs leave lock bytes unchanged.

## Acceptance Tests

- `lock_record_kinds_roundtrip_unknown_future_fields`.
- `lock_rationale_preserves_exact_identity`.
- `lock_merge_independent_additions`.
- `lock_merge_same_identity_two_owners`.
- `lock_merge_conflicting_identity_diagnostic`.
- `lock_merge_platform_specific_records`.
- `lock_explain_names_owner_policy_provider_platform`.
- `lock_satisfied_offline_verbs_do_not_touch_network`.
- `read_only_verbs_do_not_rewrite_lock`.

## Dependencies

- Phase A hangar/lock envelope.
- Signed package cache and package signing, because signatures and cache objects
  are lock rationale.
- Toolchain-as-dependency, because toolchain records must merge with package
  records.
- Strict graph/catalogs, because package ownership and catalog rationale are
  merge inputs.
- Native replacement overlays, because compatibility proofs must be lockable
  replacement rationale.

## Ballots Needed

- none. This plan uses existing `.jet/lock` law and internal schema changes. A
  new ballot is required only if implementation adds a new user command, manifest
  field, or lock-edit syntax beyond existing update/explain flows.

