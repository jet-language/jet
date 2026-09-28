# Adoption playbook contract

Version `1.0.0` defines the tier-neutral workflow. A tier supplies its source
language and migration mode from its own approved plan; the baseline registry
never guesses an importer or treats a generic Jet command as migration proof.
The machine-readable registry is
[`command-checks.json`](command-checks.json), validated by
[`../validate.py`](../validate.py).

## Required playbook sections

Every versioned tier playbook must cover these sections:

| Section | Required evidence |
| --- | --- |
| Prerequisites | Jet version, supported host, source-toolchain version, access, and rollback owner. |
| Inventory | Source revision, dependency graph, build entry points, data boundaries, and named owners. |
| Pilot | One representative path, baseline behavior, success threshold, and stop condition. |
| Build/import | Ratified tier operation, exact inputs, generated-output ownership, and loss report. |
| Test | Differential or golden behavior, failure injection, and clean-machine receipt. |
| Rollout | One bounded production slice, approval record, monitoring, and next checkpoint. |
| Rollback | Exact source/artifact revision, restore command, validation command, and recovery owner. |
| Ownership | Owners for source, generated Jet, dependencies, release, and incidents. |
| Known non-goals | Unsupported constructs and claims the playbook deliberately does not make. |

The baseline registry is tier-neutral and checks only the representative Jet
project plus clean execution discipline. A tier-specific importer or binder
belongs in that tier's evidence, not in the baseline file.

## Clean-project execution

The registry names [`fixtures/clean-project`](../fixtures/clean-project/run.jet)
as its representative project. Execute every command in a fresh copy of that
project. Record the exact argv, exit code, stdout, stderr, Jet version, source
revision, lock digest, and declared output digests. The executor rejects shell
command strings, unbounded working directories, and ambient secret variables;
network denial is supplied by the outer test environment.

The baseline commands are:

```sh
jet --version
jet check run.jet
jet build --small --sbom run.jet
jet run run.jet
```

Their registry entries require, respectively, a clean copy, denied network,
expected exit `0`, and stdout/stderr/exit-code capture. The SBOM build also
requires `build/run.spdx`. `jet build --small --sbom` proves the release path
can emit an SPDX sidecar; it does not prove that a tier migration succeeded.

## Evidence boundary

A playbook may claim only the tier, host, constructs, and outcomes present in
its receipts. Unsupported input remains in the original source. Generated Jet
is editable output and must not become a hidden second source of truth. Use
[`../validate.py`](../validate.py) for structural checks and
[`../compliance/README.md`](../compliance/README.md) for release evidence
requirements.
