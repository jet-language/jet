# Jet enterprise adoption pack

This directory is the evidence contract for enterprise adoption. It contains
the tier-neutral migration workflow, release-calendar handoff, compliance
artifact schemas, and offline state fixtures.

The pack records the ratified D-ADOPT-LTS1 policy. The release calendar keeps
the first GA date, replacement line, and support matrix explicit until the GA
schedule is fixed. The current release compatibility rules remain in
[release-policy.md](../docs/spec/release-policy.md).

## Contents

- [Playbook contract](playbook/README.md) — common migration steps and clean-project command checks.
- [Compliance bundle](compliance/README.md) — release artifact, provenance, license, support, and air-gap evidence contract.
- [Release calendar](release/calendar.json) — ratified LTS policy plus pending GA schedule fields; no dates are invented here.
- [Case-study contract](case-studies/README.md) — evidence limits for UL14 outcomes.
- [Schemas](schemas/) — machine-readable contracts consumed by the validator.
- [Fixtures](fixtures/air-gap/README.md) — deterministic install, update, and revocation transitions.

## Checks

The checker uses only Python's standard library. It validates local links,
JSON shape, allowed owner tokens, command safety, artifact digests, SBOM
identity, provenance binding, license coverage, support status, and air-gap
state transitions. It does not claim that a fixture is a production release
or that a structural signature field is a cryptographic verification.

```sh
python3 adoption/validate.py
python3 adoption/tests/test_adoption.py
python3 adoption/validate.py --execute-playbook
```

The last command is the clean-project command pass. It is intentionally
separate from the structural check because it needs a built `jet` on `PATH`
and may invoke the host toolchain.
