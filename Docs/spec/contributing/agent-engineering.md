# Agent engineering

This page collects reusable verification traps for contributors and agents. It
is not a capability inventory or a work queue. [`AGENTS.md`](../../../AGENTS.md)
owns project policy, Tower owns work state, and the relevant source, executable
example, or test owns behavior. Use [`jet-env`](../../../Tools/agent/jet-env)
for isolated repository commands and the
[verification skill](../../../.agents/skills/verify/SKILL.md) for the required
proof tier.

## Exercise the compiled source

Prelude files are embedded in the compiler. After changing one, rebuild before
running a smoke test, then invoke the binary from that build. Keep the program
and its stores on an isolated disk-backed path; a wrapper or cached executable
can otherwise exercise a different revision. The
[`tests/common`](../../../tests/common/) helpers show the repository's
isolation conventions.

A registry declaration does not prove dispatch. Exercise the real caller,
consume its result, and check the observable value or effect. A bind-and-
discard probe can pass while the operation does nothing. The
[Core call tests](../../../tests/core_call_table.rs) expose the relevant seams.

A backend rejection is not automatically a new user diagnostic. Trace it back
to the front-end check or lowering defect and use the
[diagnostic contract](../diagnostics.md) for the user-facing rule.

## Preserve fixture meaning

Example paths and source spans can be part of expected output, harness
selection, or generated artifacts. Before moving a fixture, search every
consumer, including expected output and harness lists. Preserve the program
and its expected meaning instead of updating snapshots to hide a regression.
Follow the [example procedure](examples.md).

An unused declaration is not automatically dead code. Trace callers, generated
uses, and approved Tower scope before deleting it. Conversely, a historical
plan is not a reason to retain machinery that has no caller or contract.

## Keep verification honest

Use the actual CLI or UI path, not only a helper that resembles it. Canvas
renders nodes on one canvas, so DOM queries alone do not establish what a user
sees. Run the smallest real command that exercises the changed behavior, then
perform the execution tiers and artifact checks named by its criteria.

Do not modify host-managed editor extensions or the owner's system
configuration to make a repository check pass. Report an unavailable
environment and finish independent work instead.
