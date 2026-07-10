# Structural Merge

## Goal

Card #143 captures structural merge by semantic identity rather than text. Current owner law says to capture the research item, do not build it yet, and revisit after content-addressed definitions and the semantic index exist.

The eventual product is a Jet-aware merge tool that resolves moves, renames, signature edits, and meaning-preserving refactors by comparing typed program identity instead of raw lines.

## Current law

- D-STRUCTMERGE1=A: far-horizon capture; prerequisites are content-addressed definitions and semantic index.
- D-STRUCTMERGE2=A folds "merge by meaning" into the same card; identity-based and meaning-based merge are one research track.
- This is tooling, not language syntax. It must not change parser, sema, or source truth.
- The visual editor roadmap depends on semantic merge because graph edits still persist as plain `.jet` source.
- I8 applies to tooling shape: one structural merge product, not separate AST-merge and meaning-merge tools.

No command spelling, file format, merge algorithm, or conflict UI is ratified.

## Vertical slices

1. Prerequisite audit: list exact semantic-index facts and content-addressed definition facts needed for merge.
2. Stable identity prototype: define identity for modules, functions, types, fields, variants, methods, and scene/editor nodes using existing front-end facts only.
3. Structural diff first: show AST/semantic changes without editing files, so identity mistakes are visible before merge writes.
4. Three-way merge: merge independent changes by identity, preserve formatting through the formatter, and surface semantic conflicts in Jet terms.
5. Meaning equivalence research: detect safe refactors only where sema can prove equivalence; otherwise report a conflict.
6. VCS integration: integrate with Git after the command/config surface is ratified.
7. Visual editor integration: graph edits and text edits use the same semantic identity and conflict model.

## Acceptance tests

- No implementation tests now: prerequisites are not met.
- Future structural diff tests: rename only, move only, add parameter, extract helper, reorder definitions, and edit same function body.
- Future merge tests: two independent edits merge; conflicting signature/body edits produce a Jet conflict report.
- Formatter round-trip test: merge output parses, formats, and preserves semantic identity.
- Visual-editor fixture: graph-originated change and text-originated change resolve through the same merge path.
- Safety test: merge never invents code that did not parse and sema-check.

## Dependency order

1. Land semantic index API with stable facts for definitions and references.
2. Land content-addressed definitions or equivalent stable definition identity.
3. Define structural identity and conflict taxonomy.
4. Build read-only structural diff.
5. Build three-way structural merge.
6. Add Git/editor integration.
7. Add meaning-equivalence rules only where sema proof is strong.

## Owner ballots needed

- D-MERGE-CMD1: command/config surface for structural diff and merge.
- D-MERGE-ID1: public identity model and what appears in conflict reports.
- D-MERGE-CONFLICT1: conflict UI and whether reports are text, JSON, editor protocol, or all three.
- D-MERGE-VCS1: Git integration policy.

## Adversarial tradeoffs

- Safety first: merge output must re-enter parser and sema before being accepted; text conflicts are better than unsound auto-merges.
- Beginner experience: default Git workflow should remain usable. Structural merge becomes a helpful tool, not required source control ceremony.
- Runtime performance: irrelevant to user programs, but tool latency matters enough that semantic-index reuse is required.
- One mechanical path: identity merge and meaning merge are one product. Diff, merge, editor, and VCS integrations must share one identity model.
- Ecosystem breadth: this supports text-first Jet, visual editing, refactors, package evolution, and future self-hosting without creating a second source of truth.
