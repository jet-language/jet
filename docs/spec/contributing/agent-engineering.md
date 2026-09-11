# Agent engineering

[AGENTS.md](../../../AGENTS.md) owns policy; Tower owns work. These are reusable
verification traps, not a capability inventory or a record of unfinished work.

## Exercise the compiled source

Prelude files are embedded in the compiler. Rebuild after changing them before
running a smoke test. Use the binary from that build and an isolated disk-backed
store; a wrapper or cached program may otherwise exercise a different revision.
See [`jet-env`](../../../scripts/agent/jet-env) and the
[test helpers](../../../tests/common/) for environment and isolation behavior.

A registry declaration alone does not prove dispatch. Exercise the real caller,
consume its result, and check the observable value or effect. A bind-and-discard
probe can pass while the operation does nothing. The
[Core conformance checker](../../../scripts/agent/core-conformance.mjs) and
[Core call tests](../../../tests/core_call_table.rs) expose the relevant seams.

Do not bless a backend rejection as a new user diagnostic. Trace it to the
front-end check or lowering defect; see the [diagnostic contract](../diagnostics.md).

## Preserve fixture meaning

Example paths and spans can be part of expected output, harness selection, or
generated artifacts. Before moving a fixture, trace every consumer. Preserve the
program and its expected meaning rather than updating snapshots to hide a
regression. Follow the [example procedure](examples.md).

An unused declaration is not automatically dead code. Trace callers, generated
uses, and approved Tower scope before deletion. Equally, a past plan is not a
reason to retain unused machinery indefinitely.

## Keep verification honest

Use the actual CLI or UI path, not only a helper that resembles it. Canvas draws
nodes on one canvas, so DOM queries alone do not establish what the user sees.
Use the [verification procedure](../../../.agents/skills/verify/SKILL.md) for the
changed behavior and required execution tiers.

Do not modify host-managed editor extensions or the owner's system configuration
to make a repository check pass. Report the unavailable environment and finish
independent work.
