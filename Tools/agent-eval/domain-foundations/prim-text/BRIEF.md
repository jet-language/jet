# Probe prim-text — Text: Unicode, shaping, tokenizers, search

Read `$JET_STATE_ROOT/dx3/COMMON.md` first; it holds the rule, the gap rubric, the working method, and the output contract. This file adds only what is specific to this probe.

## Kind

Primitive probe: cross-cutting; you test whether library code can do this and what primitive is missing; no batteries.json. Areas this probe informs: cli, gui, ai-ml, web.

## Build this

Build, as library Jet: a BPE tokenizer over a 1 MB corpus; grapheme, word, and sentence segmentation for mixed scripts; a regex-driven log scanner over 100 MB; and a text-shaping call (HarfBuzz through a bridge, or report why not) that positions glyphs for Arabic and Devanagari. Time the tokenizer and the scanner against Python/Rust equivalents. Use Examples/features/strings/**, text/**, io/grep_scan.jet.

## Answer these

1. What does the string/Unicode core lack for these authors: segmentation tables, normalization, regex features, byte/char views?

## Research to mine

Domain census and reports: `$JET_STATE_ROOT/dx2/nlp-text/`, `$JET_STATE_ROOT/dx2/typography-publishing/`, `$JET_STATE_ROOT/dx2/text-processing-parsing/`, `$JET_STATE_ROOT/dx2/documentation-static-sites/` (report.md, census.json, claims.json). Deleted ballots with worked code: `$JET_STATE_ROOT/dx2/ballots/` (grep the mechanism name). Family syntheses: `$JET_STATE_ROOT/dx2/_families/*/synthesis.md`.

## Output

`$JET_STATE_ROOT/dx3/prim-text/probe.md`, `gaps.json`, and the code under `$JET_STATE_ROOT/dx3/prim-text/pkg/`. Gap ids start with `prim-text-G`.
