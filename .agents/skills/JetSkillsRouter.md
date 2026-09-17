# Jet skill routing

Choose the method that matches the requested outcome. Its owner completes that outcome; bounded helpers return evidence to it. A clear request does not need a routing interview. Use `jet-router` only when the method is unclear.

The request defines scope and permissions. A full audit covers its declared corpus and required categories; a focused question does not expand into unrelated work. Report-only and proposal-only requests do not authorize implementation or new Tower cards. A request for an ordinary report or review does not activate HTML.

## Routes

| Requested outcome | Skill |
| --- | --- |
| Current run progress, completion estimate, or task status | `pulse` |
| Language shape, uniformity, or syntax outliers | `surface-audit` |
| Concept unity, isomorphisms, or false rhymes | `isomorphic-ontology-audit` |
| Persona experience, practical use, or push/pull | `persona-audit` |
| Ratified requirements versus current behavior | `spec-compliance-audit` |
| Philosophy or mission alignment | `mission-audit` |
| Workload friction, useful defaults, and expert override | `pragmatism-audit` |
| Types versus markers, tags, or control constructs | `type-unification-audit` |
| First-principles rethink of a named domain | `first-principles-audit` |
| Run the competitive corpus or change its matrix/entries | `gauntlet` |
| Learnability findings from a newcomer lens | `rli5` |
| Research a named language/API gap | `surface-research` |
| Peer-language lineage, regrets, and lessons | `lessons-learned` |
| Measured public-code frequency and Jet friction | `surface-frequency-audit` |
| Mine specified external sources for Jet lessons | `mine-for-jet` |
| One source-backed question | `research` |
| One-question or frontier-round decision interview | `grilling` or `batch-grill-me` |
| Interview plus agreed glossary/ADR record | `grill-with-docs` |
| Domain terms or agreed model records | `domain-modeling` |
| Map unresolved decisions to a destination | `wayfinder` |
| Slice settled work into dependency-linked tickets | `to-tickets` |
| Synthesize the conversation into a specification | `to-spec` |
| Triage an issue or external PR | `triage` |
| Design a module boundary, or survey deepening candidates | `codebase-design` or `improve-codebase-architecture` |
| Reorganize structure without changing behavior | `structure-cleanup` |
| Remove proven dead code or authorized stale artifacts | `garbage-collection` |
| Dispatch workers, integrate, recover, or close delivery | `orchestration` |
| Obtain evidence for a code change or milestone | `verify` |
| Explicit owner order to implement before validation | `implementation-before-validation`, only for that phase order |
| General board operation or single-card read/write | `tower` |
| Prepare plans and decisions without implementation | `tower-prep` |
| Rank the dependency-safe workOrder queue | `tower-rank` |
| Execute and close the requested Tower scope | `tower-burndown` |
| Prepare a genuine unresolved owner-only choice | `tower-ballot` |
| Initialize/import Tower or inspect configuration | `tower-setup` |
| Explicitly requested HTML report or interactive page | `html` |
| Beginner explanation, optionally compressed | `eli5` or `eli5-caveman` |
| Human-facing Jet prose or explicit STE request | `simple` |

Repository skill entrypoints are `.agents/skills/<name>/SKILL.md`; Tower entrypoints are `plugins/tower/skills/<name>/SKILL.md`. The conditional phase-order skill is a managed installation, not an implementation default. Imported Matt Pocock methods retain their own contracts; this index does not rewrite them.

## Shared references

- [Standing lens](_shared/standing-lens.md): apply the evidence sections relevant to the declared audit. Internal spec/ontology/type audits use probe and honesty sections, not a forced competitive analysis.
- [Audit dispositions](_shared/audit-dispositions.md): choose the output/write boundary; load publication details when producing a retained report. Mining, gauntlet, and first-principles methods retain their declared Tower obligations unless the owner requests report-only.
- [Orchestration](orchestration/SKILL.md): canonical dispatch and closeout mechanics. `AGENTS.md` remains the shared authority for invariants, owner decisions, models, and resources.
- [Skill sources and dispositions](_shared/skill-sources.md): read only for discovery, installation, duplicate-source, or retirement work. Metadata and source checks do not prove host enforcement.

## Completion

The selected method stops at its observable result: report, proposal, board operation, or integrated proven implementation. Account for blocked evidence and owned work; never substitute a first draft or unrelated follow-on agenda for the requested outcome. Return findings to a caller when used as bounded support rather than creating an unrequested second artifact.
