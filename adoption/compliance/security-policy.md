# Security policy for the adoption bundle

## Reporting

Report a suspected vulnerability through the repository's private security
reporting channel. Do not put exploit details, credentials, private keys, or
customer data in a public issue, pull request, playbook, fixture, or release
artifact.

## Response

The incident owner preserves the original report, identifies affected release
artifacts by digest, records the decision and scope, and publishes only the
minimum required advisory evidence. A release is not described as fixed until
the replacement artifact, provenance, and verification receipt are bound to
the affected subject.

## Bundle handling

Release operators verify hashes and provenance before copying a bundle into an
air-gapped environment. A failed hash, signature, support, license, advisory,
or revocation check stops the operation. Operators retain the failed receipt;
they do not repair a release by editing an artifact in place.

The package trust and offline behavior that this policy relies on are described
in the [Jetpack package plan](../../docs/plans/epoch-4/world-class-package-manager.md)
and [trust-root procedure](../../docs/infra/trust-root.md).
