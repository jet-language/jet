# Adoption playbook contract

Version `1.0.0` defines one workflow for every adoption tier. A tier supplies
its source language and migration mode from the ratified tier map; the
workflow does not guess either value.

## Required sections

Every versioned tier playbook must contain these sections:

| Section | Required evidence |
| --- | --- |
| Prerequisites | Jet version, supported host, source-toolchain version, access and rollback owner. |
| Inventory | Source revision, dependency graph, build entry points, data boundaries, and named owners. |
| Pilot | One representative path, baseline behavior, success threshold, and stop condition. |
| Build/import | The ratified tier operation, exact inputs, generated-output ownership, and loss report. |
| Test | Differential or golden behavior, failure injection, and clean-machine receipt. |
| Rollout | One bounded production slice, approval record, monitoring, and next checkpoint. |
| Rollback | Exact source/artifact revision, restore command, validation command, and recovery owner. |
| Ownership | Team responsible for source, generated Jet, dependencies, release, and incident response. |
| Known non-goals | Unsupported constructs and claims the playbook deliberately does not make. |

The baseline command registry is [command-checks.json](command-checks.json).
It is tier-neutral: it checks the Jet project path and the clean execution
discipline, not a language-specific importer. A tier-specific playbook adds its
own importer or binder command only after the tier map and implementation are
available. No baseline check is evidence for an unshipped importer.

## Clean-project rule

Run each command in a fresh copy of
[`fixtures/clean-project`](../fixtures/clean-project/run.jet). Record the
exact `argv`, exit code, stdout, stderr, Jet version, source revision, lock
digest, and output digests. The checker rejects shell command strings,
unbounded working directories, and ambient secret variables. Network denial
is an outer test-environment property; a command record cannot turn it into a
claim by setting a label.

`jet build --small --sbom` proves the current release path can emit an SPDX
sidecar. The command is not a substitute for a tier's migration proof.

## Evidence boundary

A playbook may claim only the tier, host, constructs, and outcomes present in
its receipts. Unsupported input keeps the original source. Generated Jet is
editable output and must not become a hidden second source of truth.
