---
name: persona-audit
description: >-
  Measure Jet through a finite matrix of fresh users and jobs. Use when the owner
  needs practical status, push/pull factors, or first-session evidence.
---

# Persona audit

Run fresh personas through real Jet project loops and report whether they could finish, what pulled them forward, and what pushed them away. Use the same verdicts for every row: `ship-ready`, `usable-with-friction`, or `blocked`. This is not a substitute for a focused implementation or spec audit.

Before running, read [`_shared/audit-dispositions.md`](../_shared/audit-dispositions.md) and [`_shared/standing-lens.md`](../_shared/standing-lens.md). They own shared permissions, scope depth, evidence rules, publication, and finding dispositions. This method owns the persona matrix, live project loops, first-session measurements, push/pull evidence, and verdicts.

## Declare the matrix

At activation, freeze a finite matrix with `persona × domain × job × window target`. Include fresh beginner-through-expert personas in distinct domains, plus an unattended coding agent. If the owner declares one domain, record that narrower scope. Do not add rows during the run; record a needed row as an explicit coverage gap for the owner.

For every row, define a concrete project and core loop, run representative examples with `scripts/agent/jet-env`, and record evidence, push factors, pull factors, and one verdict. Keep the first useful visual check separate from the later project loop. Window checks are conditional on the row's declared window target; their exact gates are in [`references/first-session.md`](references/first-session.md).

## Coding-agent facet

The unattended agent is always present because it is one of Jet's three readers. Give it a real project and the same loop as any other row: read context, edit, run the checker, read the verdict, repeat, and stop when clean. Grade its push and pull factors with the five quantities: verdict fidelity, verdict latency, verdict actionability, context economy, and repair determinism. Do not soften `blocked` because the surrounding tooling is young.

Walk the relevant UX/DX slice for every row: where it waited, what surprised it, what it had to say twice, what it had to know before starting, and which error text left it stuck. Record one verbatim reaction such as “this reads nicely” or “this made me sigh.” A preference remark is evidence about the surface even when it is not evidence about the technology.

## Standing lens

Apply only the standing-lens sections relevant to the declared matrix. Use runtime probes and the UX/DX micro-sweep slice where the row needs them; do not force unrelated comparisons or window work. Report missing or unavailable evidence as `not-proven` or `blocked` with the reason, never as an invented success.

## Completion and output

Stop when every frozen row has a project loop, representative evidence, push/pull factors, and a verdict; every row has a first-session result or an honest conditional `not-applicable`, `not-proven`, or `blocked`; and the report's disposition marker is complete. A report-only run writes one report under `docs/audits/` through the project-approved non-serve CLI, cites Tower read-only, and creates no board or implementation work unless the owner explicitly changes the boundary. Report completion and implementation completion remain separate.
