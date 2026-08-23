# Registry tiers

Status: current for D-REGCURATE1=C and D-1913-LIVENESS1=C, owner ratified.

## Plan of record

No plan of record was stored on card #1911. This implementation derives the
plan from the card body and its exit criteria:

1. record the tier and gate result in registry metadata and the lock;
2. show that data on fetch, install, and `jet inspect info`;
3. enforce the core review receipt at publish;
4. refuse community publish until every named gate is open.

## Core tier

The core tier contains packages reviewed by a registry maintainer. The review
receipt is committed to the registry at:

```text
reviews/<package>/<version>.review
```

The receipt uses this exact form:

```text
jet-registry-core-review-v1
package=<package>
version=<version>
reviewer=<maintainer-id>
decision=approved
```

`jet registry publish` refuses a core release when the receipt is missing,
malformed, or does not name the package and version being published.

## Community tier

`JET_REGISTRY_TIER=community` selects the community channel for a publish.
The channel is closed unless all four machine gates pass:

- #935 live signature chain;
- #431 advisory audit with a signed local feed, pinned trust root, freshness
  policy, and no matches;
- #1912 package name policy;
- #1913 maintainer liveness.

The index records each result in `gate_status`. A blocked gate stops publish
before the artifact or index changes. The current implementation keeps the
community channel closed while #1912, #1913, #431, or the live #935 chain is
not available.

The community trust model is owner-ratified by D-REGCURATE1=C. This document
records the ratified tier rules; it does not open the channel while a gate is
closed.

## Package name policy (#1912)

### Plan of record

No plan was recorded on card #1912. This plan comes from the card body and its
exit criteria:

1. read every existing package name from the registry index;
2. compare the candidate with a case-folded confusable skeleton;
3. check the reserved suffix list;
4. warn for the warning distance and block for the block distance;
5. emit a teaching diagnostic before any artifact or index write;
6. test the rule and record its thresholds here.

### Mechanical rule

`jet registry publish` checks the candidate against all existing index names.
The skeleton maps common Latin, Greek, and Cyrillic lookalikes to one ASCII
form. The check uses Levenshtein edit distance on that form.

These suffixes are reserved:

- `-fixed`
- `-patched`
- `-bin`

An exact confusable match or a reserved suffix blocks publish. Edit distance 1
blocks publish. Edit distance 2 emits `L2608` and allows the publish. A blocked
name emits `E2608`. `--force` does not bypass this name policy.

### Owner-ratified policy (D-1912-NAME1=A)

The same rule applies to both registry tiers. Curated core and machine-gated
community use it at publish time.

- `block=1`: block a candidate at edit distance 1 from an existing name.
- `warn=2`: warn at edit distance 2 and allow the publish.
- Block confusable and homoglyph matches.
- Block names ending in `-fixed`, `-patched`, or `-bin`.
- `--force` does not bypass this policy.

These thresholds and rules are owner-ratified by D-1912-NAME1=A.

## Maintainer liveness and takeover (#1913)

### Derived plan

No plan was recorded on card #1913. This plan comes from the card body and its
exit criteria:

1. expose the signed maintainer liveness state in package detail projections;
2. treat a changed package signing key as a takeover;
3. require the registry maintainer review receipt before the takeover enters
   the index;
4. verify the new maintainer's signature over the release content hash;
5. test both refused and accepted takeover paths;
6. record the owner-ratified liveness and takeover rules.

### Owner-ratified liveness rule

`D-1913-LIVENESS1=C` is owner-ratified with a rights-first policy and no forced
reclaim:

- A package is marked **dormant after 365 days** without a signed release or a
  response.
- **Three contact attempts** are required, and **90 days notice** is required.
- **Three independent registry maintainers** may approve a **voluntary transfer
  only**.
- The registry must **never reclaim a package against an active maintainer
  objection**.
- When a transfer fails, a new maintainer **must publish under a new name**.

### Enforced takeover gate

An index entry with a non-empty `public_key` different from the package's first
pinned key is a takeover. The new key must sign `content_hash`, and
`reviews/<package>/<version>.review` must contain an approved registry review
receipt. The rule applies on publish and on fetch. The old warning-only key
rotation path is retired.

The takeover gate is separate from dormant-package transfer. It does not permit
forced reclaim or override an active maintainer objection.

## User surfaces

Registry resolution writes `tier` and `gate-status` into the lock. `jet fetch`
and `jet update` print the tier and gate status for every Jet registry package.
`jet inspect info` prints the same fields for a package record. JSON discovery
records carry `tier` and `gate_status` as well. Package hovers show the tier
and maintainer liveness state from the local discovery record.
