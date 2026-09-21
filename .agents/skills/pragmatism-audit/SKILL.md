---
name: pragmatism-audit
description: >-
  Audit Jet's ability to ship useful work across a declared domain set. Use when
  checking pragmatic friction, defaults, and expert escape paths.
---

# Pragmatism audit

Score Jet on shipping useful work, not elegance alone. Ask whether a competent person finishing a real job meets friction Jet could absorb by default. This is a draft method; keep its findings concrete and record method improvements in the run report.

Do not edit this skill during an audit. Apply method changes only in a separately authorized skill-maintenance task.

Before running, read [`_shared/audit-dispositions.md`](../_shared/audit-dispositions.md) and [`_shared/standing-lens.md`](../_shared/standing-lens.md). They own shared permissions, scope depth, evidence rules, publication, and finding dispositions. This method owns domain selection, live job probes, friction taxonomy, dual-facet findings, and finite closeout.

## Declare breadth

At activation, freeze the target outcome, risks, and finite domain/workload rows. A named narrower slice from the owner is allowed. Without one, the broad default covers at least six distinct workloads from the full pool: CLI tools, web/UI, games, scientific/numeric work, networking, embedded/systems, data/serde, packaging/build, scripting/automation, and text/parsing. A narrower request may reduce that breadth only explicitly; never silently reduce or expand the declared set.

Read [`references/method.md`](references/method.md) for the evidence obligations, friction taxonomy, calibration cases, and report shape. Select domains that cover the requested risks, then keep the frozen set through the run.

## Three facets on every finding

Every finding carries all three paths:

1. **Beginner/default.** The common useful behavior works without ceremony. Footguns stay opt-in. Printing, derives, conversions, units, formats, and obvious APIs work by default.
2. **Expert/control.** The same mechanism lets an expert reject the default, override it, or take a fully manual path. Do not add a parallel mechanism, hidden rustc, or safety carve-out without `#Unsafe`.
3. **Agent.** An unattended agent can finish from compiler output: the checker catches the mistake, the verdict arrives quickly, the report is actionable, the loop does not burn tokens on repeated error reading, and one error has one obvious repair.

Grade the five agent quantities per finding: verdict fidelity, verdict latency, verdict actionability, context economy, and repair determinism. Getting the job done beats theoretical purity; ceremony is justified only when it buys safety, clarity, or expert control that cannot live behind opt-in.

## Evidence and standing lens

Search live specs, examples, stdlib, CLI, and package surfaces. Use the standing-lens sections relevant to the frozen domain set, including runtime probes and the micro sweep where they bear on the target. Do not force unrelated comparative work. Use `scripts/agent/jet-env` for representative jobs; cite paths, examples, commands, and output. Missing evidence is `unknown`, not permission to widen the run.

## Attribute the complete Jet-owned path

When a concrete job appears to require new language machinery, a new battery,
or repeated hand-written plumbing, freeze the job, its observable success
contract, the normal Jet package boundary, current APIs and decisions, and the
evidence rows to inspect. Start with bundled CoreLib and Jet-owned first-party
libraries. A foreign package, handwritten binding, or FFI wrapper is not the
routine out-of-the-box solution; explicit external integration remains
available when the job actually needs it.

Trace each failure to its owning layer. Distinguish a missing Jet-owned
offering, a broken compiler or adapter, a missing foundation mechanism, poor
discoverability, and an existing feature that the canonical program simply
does not use. If a common capability is absent, a Jet-owned library or CoreLib
addition may be the right answer; adequate compiler mechanisms do not prove
that the standard offering is adequate.

Repeated patterns, boilerplate, or awkward code are reasons to compare
credible alternatives, not proof that a compiler feature is needed. Compare
the best Jet-library path with the compiler-supported path on the same
complete job, including measured performance when applicable, setup,
readability, writing effort, reasoning, error behavior, safety, and expert
control. A combined library/compiler solution is allowed. Keep unmeasured
benefits `unknown`; performance-motivated proposals still need the standing
matched-program gate, and every compiler, dependency, or API proposal keeps
its ordinary owner approval. This report recommends; it does not implement.

The bounded method exercise uses the existing native JSON shape through the
default `jet run` path with a fresh binary:

```jet
use core.encoding.json as json

#Codable
struct Settings { port: Int }

fn run() ![FieldError] {
    cfg :: json.decode<Settings>("{{\"port\":8080}}")
    print(cfg.port)
}
```

The valid input requires `8080`. For the wrong-type row, use
`{{"port":"bad"}}` and require a type/error refusal rather than coercing it or
silently producing a value. Record which
Jet-owned support is available and whether a further library or compiler
improvement is justified; do not substitute a foreign library when setup
fails. The existing sources
[`json_typed.jet`](../../../examples/features/serde/json_typed.jet) and
[`json.jet`](../../../examples/features/serde/json.jet) establish the
first-party example pattern, not a fresh execution result. Stop after this
job's success and wrong-type dispositions, and preserve any missing execution
or comparison as `unknown`.

Keep the existing report-only workflow and friction taxonomy. Do not add a
domain skill, runner, dependency, schema, ledger, blanket lint, or automatic
API/compiler change. Show the beginner default and the expert reject/override
path, then identify the earliest prevention point. A new product or
performance claim remains outside this method.

## Completion and output

Stop when every frozen domain row has a concrete job and evidence or an honest `unknown`; every finding has beginner, expert, and agent paths; the friction taxonomy, defaults map, celebrated pragmatism, and required report sections are complete; and the disposition marker is ready. A clean domain or zero losses is valid when declared coverage is complete.

This is report-only by default. Write one report under `docs/audits/` through the project-approved non-serve CLI, cite Tower read-only, and create no cards, decisions, ballots, or implementation edits unless the owner explicitly changes the boundary. Report completion does not implement a proposed fix.
