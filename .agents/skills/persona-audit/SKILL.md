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

For every row, define a concrete project and core loop, run representative examples with `Tools/agent/jet-env`, and record evidence, push factors, pull factors, and one verdict. Keep the first useful visual check separate from the later project loop. Window checks are conditional on the row's declared window target; their exact gates are in [`references/first-session.md`](references/first-session.md).

## Coding-agent facet

The unattended agent is always present because it is one of Jet's three readers. Give it a real project and the same loop as any other row: read context, edit, run the checker, read the verdict, repeat, and stop when clean. Grade its push and pull factors with the five quantities: verdict fidelity, verdict latency, verdict actionability, context economy, and repair determinism. Do not soften `blocked` because the surrounding tooling is young.

Walk the relevant UX/DX slice for every row: where it waited, what surprised it, what it had to say twice, what it had to know before starting, and which error text left it stuck. Record one verbatim reaction such as “this reads nicely” or “this made me sigh.” A preference remark is evidence about the surface even when it is not evidence about the technology.

## Optional controlled first-session branch

Use this branch only when the requested outcome is a controlled cold-session,
repair, assistance, or preference study. Ordinary persona work keeps its
existing project loops and does not collect controlled-trial paperwork.

The retained T1 fixture at
[`Docs/audits/raw/2393-r2/fixtures.json`](../../Docs/audits/raw/2393-r2/fixtures.json)
is task data, not participant evidence. Freeze its bytes, argument vectors,
the fresh Jet binary identity, prompt, allowlisted current documentation,
tools, time limits, and expected results before launch. The initial pilot is
one fresh OMP `@implementation` agent on the configured Linux CLI host, with
no model substitution; record the resolved provider, model, and settings.
Capture or content-address every harness-delivered role, system, project,
skill, and injected context instruction before launch. Later unapproved
assistance changes controlled status to unavailable. This controls observable
harness inputs, not opaque provider internals or model training.

For T1, the success input is:

```text
[base]
name = "Ada"
role = "admin"
[dev]
inherits = "base"
role = "reviewer"
```

The controller invokes the argv vectors `["records.cfg"]` and
`["records-failure.cfg"]`; the successful stdout contract includes its final
newline.

The program receives `records.cfg` and must emit exactly:

```text
base.name=Ada
base.role=admin
dev.name=Ada
dev.role=reviewer
```

The failure input contains `name = "Ada"` followed by `name = "Grace"` in
`[base]`. It must exit non-zero, emit no stdout, and identify the duplicate
field name and section `base`. Preserve this stronger field-name requirement
even if the retained checker only searches for `duplicate` and `base`.

Use a fresh task workspace and program artifacts under
`~/.cache/jet-test-scratch`. The agent submits its first complete program
before receiving compiler or run feedback; preserve the exact source and
first result. Permit at most one repair submission, with only raw
compiler/run results and expected-versus-actual differences as feedback, not
suggested code. Give each submission a 15-minute limit. Keep first-attempt
success, checker result, independent semantic output, refusal, first failure,
repair, and post-task comments in separate records. A hardcoded fixture
answer is not success; a green checker with wrong output or an unnamed
duplicate is a semantic failure. A program that passes both cases needs no
forced failure or repair.

Missing profile, fresh session, required tool, or verifiable help boundary
makes this controlled branch unavailable rather than silently changing it.
A refusal, timeout, or unavailable run is a non-result, not a language score.
One agent's post-task comment is a usability observation, not earned
cross-language preference. Do not recruit people, resume the full `#2393`
campaign, create a runner or ledger, edit product code, or issue a release
verdict. Missing controlled setup blocks only this branch and claims that
require it; ordinary persona observations remain available.

## Standing lens

Apply only the standing-lens sections relevant to the declared matrix. Use runtime probes and the UX/DX micro-sweep slice where the row needs them; do not force unrelated comparisons or window work. Report missing or unavailable evidence as `not-proven` or `blocked` with the reason, never as an invented success.

## Completion and output

Stop when every frozen row has a project loop, representative evidence, push/pull factors, and a verdict; every row has a first-session result or an honest conditional `not-applicable`, `not-proven`, or `blocked`; and the report's disposition marker is complete. A report-only run writes one report under `Docs/audits/` through the project-approved non-serve CLI, cites Tower read-only, and creates no board or implementation work unless the owner explicitly changes the boundary. Report completion and implementation completion remain separate.
