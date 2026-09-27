# Compliance bundle contract

A compliance bundle is release-scoped. Every artifact must bind the same
release version, source commit, lock digest, platform, and binary subject. The
[`artifact-manifest.schema.json`](../schemas/artifact-manifest.schema.json)
is the index; [`../validate.py`](../validate.py) checks relationships and file
contents rather than trusting names.

## Required evidence

| Artifact kind | Required evidence |
| --- | --- |
| `binary` | Release artifact and SHA-256 digest. |
| `sbom-spdx` | SPDX 2.3 document whose root name/version and lock-derived namespace match the release. |
| `provenance` | In-toto Statement v1 with SLSA provenance, builder, source, inputs, and binary subject. |
| `signature` | Detached signature for the provenance and its key identity. |
| `licenses` | Concrete SPDX expression and notice/source record for every SBOM component. |
| `security-policy` | Reporting, response, and secret-handling rules. |
| `support-policy` | Release support facts rendered from the canonical calendar. |
| `reproducibility` | Exact commands, inputs, toolchain, and independently verified rebuild. |
| `air-gap-bundle` | Offline install, replacement, and revocation evidence. |

A CycloneDX view is optional (`sbom-cyclonedx`). If included, it must bind the
same root and lock digest and cannot disagree with SPDX evidence. The air-gap
fixture contract is [`air-gap/README.md`](air-gap/README.md).

## Verification order

1. Hash every manifest artifact and reject missing, changed, symlinked, or
   path-escaping entries.
2. Match binary, SBOM, provenance, and signature subjects; bind detached
   signature bytes to the provenance digest reference.
3. Match the SBOM root version and lock-derived namespace to the release.
4. Match the license inventory to every SBOM package and reject blank,
   `NOASSERTION`, `NONE`, or unknown expressions.
5. Require the approved release verifier to verify the detached provenance
   signature. The stdlib checker validates shape and binding; it does not
   implement Ed25519.
6. Require support data rendered from the calendar. Unresolved owner tokens
   make a bundle non-publishable.
7. Verify the air-gap receipt with network disabled and retain both allow and
   deny transitions; copying an archive is not revocation evidence.

Run structural checks with:

```sh
python3 adoption/validate.py --bundle <evidence-dir>
```

Add `--publishable` only when the release pipeline has rendered the schedule
fields and the canonical calendar is ratified. The default pack check is
`python3 adoption/validate.py`.

## Secret boundary

Private signing keys, bearer tokens, cloud credentials, and credential-bearing
URLs never enter this directory or a release archive. Public trust keys may be
recorded in a verifier receipt. A signature string without a separately
verified key proves presence, not authenticity.
