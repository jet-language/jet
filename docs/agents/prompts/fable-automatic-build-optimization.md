# Prompt: automatic build optimization proposal

Write Jet’s definitive proposal for automatic compiler, package-manager, dependency-graph, build-action, cache, code-generation, and linker optimization.

Goal: beat Rust and Cargo on matched clean and incremental workloads without sacrificing safety, diagnostics, determinism, or execution-tier parity.

Hard rule: optimization must never change user-defined package, subpackage, or workspace boundaries. Preserve declared authority, policy, visibility, outputs, and dependency edges exactly. Optimize only within those boundaries and across their declared graph. A large package must gain fine-grained reuse without forcing the user to reorganize it.

Use current code, specs, ratified decisions, `docs/plans/compiler-speed.md`, the Jetpack metrics and audits, and relevant Tower records. Treat the existing generic draft at `docs/proposals/automatic-build-optimization.md` as untrusted input. Re-derive the design and replace that draft with your own proposal.

Decide the canonical work graph, identity and invalidation model, package and build-action integration, deterministic scheduling, cache integrity, failure behavior, beginner defaults, expert inspection, clean cutover, and matched Cargo benchmark method. Remove duplicate mechanisms rather than layering another graph or cache beside them. Separate current facts, proposed design, and proof still required.

Write the final proposal to `docs/proposals/automatic-build-optimization.md` using clear prose, concrete architecture, production seams, hostile invalidation cases, and measurable acceptance criteria.

Do not implement it, write Tower cards, modify `dogfood/jetpack/**`, or resume Jetpack parity.
