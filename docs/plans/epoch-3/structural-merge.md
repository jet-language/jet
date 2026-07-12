# Structural Merge

## Goal

Card #143 ships structural merge by semantic identity rather than text. The
semantic index now exposes compiler-owned structural nodes plus stable and
content-addressed definition facts, satisfying the original prerequisites.

The product resolves moves, renames, signature edits, and independent body
edits by comparing typed program identity instead of raw lines. Meaning
equivalence remains conservative: without a sema proof, Jet reports a conflict.

## Current law

- D-STRUCTMERGE1=A established the prerequisite order. Both foundations now
  exist in `jet-semindex`; D-MERGE-CMD1, D-MERGE-ID1,
  D-MERGE-CONFLICT1, and D-MERGE-VCS1 ratify the product surface.
- D-STRUCTMERGE2=A folds "merge by meaning" into the same card; identity-based and meaning-based merge are one research track.
- This is tooling, not language syntax. It must not change parser, sema, or source truth.
- The visual editor roadmap depends on semantic merge because graph edits still persist as plain `.jet` source.
- I8 applies to tooling shape: one structural merge product, not separate AST-merge and meaning-merge tools.

Commands are `jet diff --structural` and `jet merge --structural`. Reports use
one versioned conflict schema with text, JSON, and editor renderers. Human
identity appears first and stable internal ID remains present. `jet merge
install-driver --repo <path>` installs the opt-in Git driver.

## Shipped contract

`jet-semindex` emits one `DefinitionFact` per compiler-owned top-level item.
`stable_id` hashes typed signature and AST ownership/slot shape. It survives
formatting, source moves, symbol renames, and body-literal edits. `content_id`
hashes normalized definition source and
changes for semantic edits. Human spelling and paths stay separate from both
machine identities.

Structural diff matches stable identity first, then an unambiguous human
identity from checked sema facts. Structural merge uses the base program as its
ancestry map. One-sided edits and edits to different definitions compose;
overlapping edits, delete/edit pairs, competing additions, and ambiguous
identity matches stop as typed conflicts. Merge output is formatted, reparsed,
and sema-checked before any output file is written.

## Vertical slices

1. Prerequisite audit: list exact semantic-index facts and content-addressed definition facts needed for merge.
2. Stable identity prototype: define identity for modules, functions, types, fields, variants, methods, and scene/editor nodes using existing front-end facts only.
3. Structural diff first: show AST/semantic changes without editing files, so identity mistakes are visible before merge writes.
4. Three-way merge: merge independent changes by identity, preserve formatting through the formatter, and surface semantic conflicts in Jet terms.
5. Meaning equivalence research: detect safe refactors only where sema can prove equivalence; otherwise report a conflict.
6. VCS integration: integrate with Git after the command/config surface is ratified.
7. Visual editor integration: graph edits and text edits use the same semantic identity and conflict model.

## Acceptance tests

- Structural diff tests cover rename, body changes, typed IDs, and JSON output.
- Merge tests cover independent edits, overlapping edit conflicts, malformed
  input, parser/sema recheck, no-write failure, and idempotent Git installation.
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

## Ratified owner ballots

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
