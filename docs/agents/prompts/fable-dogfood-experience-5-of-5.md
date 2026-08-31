# Prompt: turn the Jetpack dogfood report into a 5/5 Jet experience slate

Exhaust `docs/audits/dogfood-jetpack-usage-experience-2026-08-30.md`. Turn every actionable finding into a complete, deduplicated Tower slate that, once implemented, earns 5/5 for reading, writing, reasoning, creating, modifying, diagnostics, and tooling/docs.

Use the report, frozen Jetpack source, current specs, current compiler and tooling, and live Tower state. Propose every semantic or structural improvement supported by actual Jet usage. Include ways to make idiomatic Jet obvious and enforceable, especially for agents: compiler or linter diagnostics, safe fixes, formatter or language-server actions, canonical executable examples, and guidance derived from one source of truth. Do not settle for a style guide where tooling can enforce the rule.

The target is earned preference, not coached praise. Define a neutral fresh-agent rerun on matched real tasks. It passes only when every category is 5/5, agents independently prefer Jet for the work, and all concrete negative feedback is resolved. Add a final hostile pass that fails the campaign if it finds any uncarded value left in the report or tested experience.

Tower behavior:

- Plan, card, and ballot only. Do not implement or close implementation cards.
- Do not edit `dogfood/jetpack/**` or resume Jetpack parity. The canary is evidence, not a second product.
- Deduplicate against current cards and decisions. Extend existing root-cause cards when they already own the work.
- Create one card per coherent root mechanism, with observable criteria, proof, dependencies, affected paths, I9 coverage where relevant, and a non-Jetpack generality witness.
- Create full ballots for every genuine owner choice, including semantic, syntax, default-behavior, public-tooling, or invariant changes. Do not create fake ballots for ordinary bugs or ratified work.
- Leave ungated cards ready and gated cards in decide. Run `tower lint` and fix findings caused by the slate.

Keep a complete finding-to-card disposition ledger in `docs/proposals/dogfood-jet-experience-5-of-5.md`. Finish with board counts, open decisions, dependencies, and the exact next burndown scope.
