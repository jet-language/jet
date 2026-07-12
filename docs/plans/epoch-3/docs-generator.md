# Docs Generator

Card: #156 / c39sekg. Status: research plan.

## Goal

Create Jet's equivalent of API documentation generation without copying
rustdoc's Rust-shaped assumptions. The generator reads Jet semantic facts,
doc comments, examples, doctests, maturity tags, and package metadata, then
emits stable HTML and machine-readable docs data.

## Beginner/Expert/Hybrid Pass

- Beginner: `jet doc` should produce useful local HTML from a package or file
  with no configuration.
- Expert: JSON output, hidden/private controls, maturity filters, provenance,
  doctest status, API diff links, generated-source links, and package graph docs
  are available explicitly.
- Hybrid: one semantic-index-backed docs model powers HTML, CLI queries, LSP
  hovers, package pages, and CI checks.

## Current Anchors

- S49/D-TEST4: `///` docs and doctests are ratified.
- D-MARK-META1=B: `#Meta(maturity: .Experimental | .Tested | .Hardened)` is doc-only API metadata.
- D-SEMINDEX1: semantic index owns symbols, references, types, calls, effects.
- D-WD2: dossier is an umbrella over facts, not a second checker.
- `docs/reference/**` is current hand-written reference output.

## Implementation Slices

1. Fact schema: define `DocItem` over semantic index facts: module, type, trait,
   function, method, field, marker list, signature, effects, visibility, source
   span, doc text, examples, doctest status.
2. CLI surface after ballot: generate HTML and JSON from a file or package.
3. Markdown/doc-comment parser: std-only subset covering paragraphs, code fences,
   inline code, links, and headings.
4. Doctest integration: run fenced `jet` snippets through `jet test`; attach
   pass/fail and output expectations to `DocItem`.
5. Maturity metadata: render `.Experimental`, `.Tested`, `.Hardened` as badges with
   copy that says "documentation tag only".
6. HTML renderer: deterministic static site, no remote assets, search index
   generated locally, accessible navigation.
7. Package docs: include public API, examples, dependency/effect summary, version
   info, and links to `jet inspect dossier` lenses.
8. CI gate: optional docs check verifies links, doctests, stale examples, and
   missing public docs according to package policy.

## Test Strategy

- Golden JSON snapshots for `DocItem` schema.
- HTML snapshots for a tiny package with functions, traits, docs, doctests,
  maturity tags, private items, and links.
- Doctest mismatch uses E2901 and existing doctest runner.
- Link checker test over generated local pages.
- No host `cargo doc` dependency in the generator path; comparison to rustdoc is
  design input only.

## Ratified Surface

`D-DOC-GEN1=A`: `jet doc` is the documentation generator command.

- `jet doc` generates deterministic local HTML by default.
- `jet doc --json` emits the stable machine-readable docs schema.
- `jet doc --check` runs doc link, doctest, and stale-example checks for CI.
- `jet inspect dossier` may link to generated docs facts, but it is not the primary docs
  generation command.
