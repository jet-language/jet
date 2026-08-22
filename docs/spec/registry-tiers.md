# Registry tiers

Status: current for D-REGCURATE1=C, owner ratified 2026-08-12.

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

The code stores `warn=2` and `block=1` in
`Source/Publish/NamePolicy.rs`. These values are provisional. No owner-ratified
threshold decision is recorded in this checkout. Ratify the two values, then
update this section and the constants before closing #1912.

## User surfaces

Registry resolution writes `tier` and `gate-status` into the lock. `jet fetch`
and `jet update` print the tier and gate status for every Jet registry package.
`jet inspect info` prints the same fields for a package record. JSON discovery
records carry `tier` and `gate_status` as well.
