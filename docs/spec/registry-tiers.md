# Registry tiers

This page defines the trust and publish gates for Core and Community registry
packages. It covers receipts, names, signatures, maintainer liveness, and user
projections. The executable publish/index paths are `Source/Publish/Tier.rs`,
`Source/Publish/NamePolicy.rs`, `Source/Publish/Index.rs`, and
`Source/CmdSupply.rs`.

## Core tier

The Core tier contains packages reviewed by a registry maintainer. The receipt
is committed outside the package source tree at:

```text
reviews/<package>/<version>.review
```

It has this exact form:

```text
jet-registry-core-review-v1
package=<package>
version=<version>
reviewer=<maintainer-id>
decision=approved
```

`jet registry publish` refuses a Core release when the receipt is missing,
malformed, names a different package or version, has an empty reviewer, or does
not say `decision=approved`. `Source/Publish/Tier.rs::require_core_review`
reads and validates this file before the index can be changed.

## Community tier

`JET_REGISTRY_TIER=community` selects the Community channel; an invalid or
missing value defaults to Core. Community is open only when all four machine
gates pass:

- **#935 live signature chain** verifies the candidate and its index chain;
- **#431 advisory audit** uses a signed local feed, pinned trust root, freshness
  policy, and no advisory or maturity matches;
- **#1912 package-name policy** passes the confusable, suffix, and distance
  checks;
- **#1913 maintainer liveness** passes the required liveness status.

The immutable index records `tier` and a semicolon-delimited `gate_status` for
each version, with the fields `signature`, `audit`, `name`, `liveness`, and
`review`. Community acceptance requires all four Community fields to be
`passed`; `review` is `not-required` for Community. A blocked gate stops the
publish before artifact or index mutation. The gate computation is in
`Source/Publish/Tier.rs::community_gate_status`, and the publish ordering is
in `Source/CmdSupply.rs`.

D-REGCURATE1=C is the owner-ratified Community trust model. The gate is
fail-closed: a missing required feed, key, name result, or liveness result is
not a pass. The Core review receipt remains required for Core publications and
for signing-key takeovers.

## Package-name policy (#1912)

### Mechanical checks

At publish time, Jet compares the candidate with every existing index name.
`Source/Publish/NamePolicy.rs` first builds a case-folded confusable skeleton
for selected Latin, Greek, Cyrillic, and diacritic lookalikes, then computes
Levenshtein distance on that skeleton. The policy reserves names ending in:

- `-fixed`
- `-patched`
- `-bin`

An exact confusable match or reserved suffix blocks publish. Edit distance 1
blocks publish. Edit distance 2 emits `L2608` and allows the publish. A blocked
name emits `E2608`; `--force` does not bypass this policy.

### Ratified thresholds (D-1912-NAME1=A)

The same policy applies to Core and Community at publish time:

- `block=1`: block a candidate at distance 1 from an existing name;
- `warn=2`: warn at distance 2 and allow the publish;
- block confusable and homoglyph matches;
- block the three reserved suffixes;
- do not let `--force` bypass the result.

These thresholds are owner-ratified policy, not a suggestion to resolve a name
against only the candidate package's own index file.

## Maintainer liveness and takeover (#1913)

### Rights-first liveness policy

D-1913-LIVENESS1=C is owner-ratified with no forced reclaim:

- mark a package dormant after 365 days without a signed release or response;
- make three contact attempts and give 90 days' notice;
- allow three independent registry maintainers to approve a voluntary transfer
  only;
- never reclaim a package against an active maintainer objection;
- if a transfer fails, require the new maintainer to publish under a new name.

The package record exposes `liveness` as one of the required Community gate
fields. The policy's dormancy clock and contact process are not inferred from a
stale package name or silently converted into ownership; the registry must
carry an explicit passing liveness result before Community can open.

### Enforced signing-key takeover

A non-empty `public_key` different from the first non-empty key pinned for a
package is a maintainer takeover, not an ordinary rotation. The new key must
sign `content_hash`, and
`reviews/<package>/<version>.review` must contain the approved receipt above.
`Source/Publish/Index.rs::validate_takeover` enforces both requirements before
an immutable index line is written; fetch verification applies the same
signature and tier checks. A normal later release leaves `public_key` empty and
uses the pinned key.

A takeover review does not authorize forced reclaim or override an active
maintainer objection. The old warning-only key-rotation path is not a valid
substitute for the review receipt.

## User-visible projections

Registry resolution writes `tier` and `gate-status` into the lock. `jet fetch`
and `jet update` print the tier and gate status for every Jet registry package,
and `jet inspect info` prints the same fields from the package record. JSON
discovery carries `tier` and `gate_status`; package hovers display the tier and
the maintainer-liveness value from that local record. The lock projection is
implemented in the package-resolution path, and the discovery projection is
in `crates/jetpack/src/Discovery.rs`.
