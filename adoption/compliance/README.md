# Compliance bundle contract

The compliance bundle is release-scoped. It is publishable only when every
file names the same release version, source revision, lock digest, platform,
and binary subject. The [artifact manifest schema](../schemas/artifact-manifest.schema.json)
is the index; the validator checks the relationships rather than trusting file
names.

## Required evidence

| File kind | Required content | Current producer or source |
| --- | --- | --- |
| Binary | Release artifact and SHA-256 digest. | Release workflow. |
| SPDX SBOM | SPDX 2.3 document with root name/version and lock-derived namespace. | `jet build --sbom`; sidecar is next to the binary. |
| Provenance | In-toto Statement v1 with SLSA provenance predicate, builder, source, inputs, and binary subject. | Release signing pipeline. |
| Signature | Detached signature for the provenance and its key identity. | Offline or approved release signer. |
| License inventory | One concrete SPDX expression and notice/source record for every SBOM component. | Release evidence collection. |
| Security policy | Reporting, response, and secret-handling rules. | This pack plus the repository policy. |
| Support policy | Rendered release support facts linked to the calendar. | Generated after the GA schedule is fixed. |
| Reproducibility receipt | Exact commands, inputs, toolchain, and independent rebuild result. | Release verification. |
| Air-gap fixture/receipt | Offline install, replacement, and revocation transitions. | [fixture contract](air-gap/README.md). |

`jet inspect sbom --cyclonedx` can produce the optional CycloneDX view. If it
is published, the artifact manifest and validator bind it to the same root
and release; it is never allowed to silently disagree with the SPDX view.

## Verification order

1. Hash every file named by `artifact-manifest.json`; reject missing, changed,
   symlinked, or path-escaping entries.
2. Match binary, SBOM, provenance, and signature subjects and bind the
   detached-signature bytes to the provenance digest reference.
3. Match SBOM root version and lock-derived namespace to the release record.
4. Match the license inventory to every SBOM package; reject `NOASSERTION`,
   `NONE`, blank, or unknown expressions.
5. Require a verified detached provenance signature from the release verifier.
   The stdlib checker validates the evidence shape and binding. It does not
   implement Ed25519 or replace the approved cryptographic verifier.
6. Require the support artifact to be rendered from the calendar. An
   unresolved owner token makes a bundle non-publishable.
7. Verify the air-gap receipt with network disabled and retain both allow and
   deny transitions. A copied archive is not proof of revocation handling.

Run the structural checker with `python3 adoption/validate.py --bundle <dir>`.
Add `--publishable` only after the release pipeline has rendered the pending GA
schedule fields and ratified support policy.

## Secret boundary

Private signing keys, bearer tokens, cloud credentials, and credential-bearing
URLs never enter this directory or a release archive. Public trust keys may be
recorded in a verifier receipt. A signature string without a separately
verified key is evidence of presence, not evidence of authenticity.
