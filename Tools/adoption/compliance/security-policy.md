# Security policy for the adoption bundle

This policy governs security evidence included with an adoption bundle. The
repository reporting channel is documented in [`SECURITY.md`](../../../SECURITY.md);
use it instead of a public issue or pull request.

## Reporting

Report a suspected vulnerability through the repository's [GitHub private
vulnerability reporting](https://github.com/jet-language/jet/security/advisories/new)
channel. Do not put exploit details, credentials, private keys, or customer
data in a public issue, pull request, playbook, fixture, or release artifact.

## Response

The incident owner preserves the original report, identifies affected release
artifacts by digest, records the decision and scope, and publishes only the
minimum required advisory evidence. Do not describe a release as fixed until
the replacement artifact, provenance, and verification receipt are bound to the
affected subject.

## Bundle handling

Release operators verify hashes and provenance before copying a bundle into an
air-gapped environment. A failed hash, signature, support, license, advisory,
or revocation check stops the operation. Retain the failed receipt; never repair
a release by editing an artifact in place.

The package trust and offline behavior used by this policy are described in the
[Jetpack package plan](../../../Docs/proposals/jetpack/world-class-package-manager.md)
and [trust-root procedure](../../../Docs/spec/packaging/trust-root.md).
