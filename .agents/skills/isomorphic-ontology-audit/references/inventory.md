# Ontology inventory and analysis

## Set the closure

At activation, name the target concept or area and record its finite reachable closure. Include forms, declarations, dependencies, and direct analogues that explain that concept. Do not inventory unrelated Jet. Account for every source in the closure, including unavailable sources as `unknown` with a reason.

Search the live target surfaces in specs, examples, stdlib, the Syntax registry, and CLI. Prefer `scripts/agent/jet-env` and repository search over memory. Source code shows implementation state; ratified decisions and domain specs show design law.

## Inventory and map

Inventory relevant keywords, sigils, declaration shapes, expressions, patterns, type syntax, attributes, module/import forms, and expert escapes. Cite each home. For every form, record:

- primary family and member ID from [`../ontology.md`](../ontology.md);
- applicable orthogonal X-axes;
- one sentence that says what the form is;
- whether the running parser accepts it, sema checks it, and code emits it;
- status such as teaches well, partial, broken, false rhyme, absent, or `unknown`.

Cluster rows by ontology family, not by glyph. Score each relevant family with `ontology.md` §16: clarity, isomorphism, exploratory density, systems expressiveness, ceremony tax, and tiering. Keep the dual Python and systems/safety facets separate in the evidence.

## Evidence and findings

Probe the running binary for every material executable claim. A spec paragraph, registry entry, or declaration is intent, not proof. If the parser accepts a form that sema ignores, or a declared form emits nothing, keep it in the map with that fact attached. State plainly when a false rhyme leaves Jet worse than a peer at the same concept.

Emit only the six finding kinds in [`report.md`](report.md). Recommend the smallest spelling or semantic move that creates the “ohhh,” or say `leave alone`. Do not add a parallel mechanism. Owner gates such as new syntax become ballot titles only unless the owner changes the report-only boundary.

## Inventory stop

The inventory phase is complete when every form in the finite closure has a mapping, probe result, or honest `unknown`, every relevant family has a score, and no required source row remains unaccounted for. A clean family is still a result.
