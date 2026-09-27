# Jet enterprise adoption pack

The adoption pack is a machine-checkable evidence contract for a Jet release.
It defines a tier-neutral migration workflow, release-calendar inputs,
compliance artifact schemas, case-study evidence boundaries, and deterministic
offline state transitions. It is not a migration importer and it does not
turn a fixture into a production release claim.

The release policy outside this pack is
[`docs/spec/release-policy.md`](../docs/spec/release-policy.md). The pack's
calendar records the ratified `D-ADOPT-LTS1` policy while leaving the first-LTS
schedule and support-matrix fields as explicit owner tokens. No dates or
support commitments are inferred from the tokens.

## Contents

- [`playbook/`](playbook/README.md) — required migration phases and the
  clean-project command registry.
- [`compliance/`](compliance/README.md) — release-scoped artifact, provenance,
  license, security, support, reproducibility, and air-gap evidence.
- [`release/calendar.json`](release/calendar.json) — the policy values and
  owner-token fields consumed by validation.
- [`case-studies/`](case-studies/README.md) — bounded evidence shape for
  capstone outcomes.
- [`schemas/`](schemas/) — JSON Schemas used by the pack and release tooling.
- [`fixtures/air-gap/`](fixtures/air-gap/README.md) — deterministic local-byte
  install, replacement, and revocation transitions.

## Validate the pack

The default check covers schema inventory, local links, calendar values and
owner tokens, playbook argv safety, secret-like material, and air-gap
transitions. `--bundle` additionally checks release artifact relationships,
license coverage, provenance, support, and reproducibility evidence. The
validator does not implement Ed25519 or replace an approved cryptographic
verifier.

```sh
python3 adoption/validate.py
python3 adoption/tests/test_adoption.py
```

For a concrete release evidence directory, add the bundle checks:

```sh
python3 adoption/validate.py --bundle <evidence-dir>
python3 adoption/validate.py --bundle <evidence-dir> --publishable
```

`--publishable` requires a rendered support policy and a ratified calendar. To
exercise the declared commands, provide a built executable (the default is
`jet` on `PATH`):

```sh
python3 adoption/validate.py --execute-playbook --jet target/debug/jet
```

The executor copies the clean project to a temporary directory for every
command, denies network through its test environment, captures stdout/stderr
and exit code, and checks declared output files. The command registry and its
exact argv are the contract; a successful structural check alone is not a
migration result.
