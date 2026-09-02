# Probe prim-text — Text: Unicode, shaping, tokenizers, search

Read `docs/research/domain-foundations/probes/COMMON.md` first; it holds the rule, the gap rubric, the working method, and the output contract. This file adds only what is specific to this probe.

## Kind

Primitive probe: cross-cutting; you test whether library code can do this and what primitive is missing; no batteries.json. Areas this probe informs: cli, gui, ai-ml, web.

## Build this

Build, as library Jet: a BPE tokenizer over a 1 MB corpus; grapheme, word, and sentence segmentation for mixed scripts; a regex-driven log scanner over 100 MB; and a text-shaping call (HarfBuzz through a bridge, or report why not) that positions glyphs for Arabic and Devanagari. Time the tokenizer and the scanner against Python/Rust equivalents. Use examples/features/strings/**, text/**, io/grep_scan.jet.

## Answer these

1. What does the string/Unicode core lack for these authors: segmentation tables, normalization, regex features, byte/char views?

## Research to mine

Domain census and reports: `~/.cache/jet-luna/dx2/nlp-text/`, `~/.cache/jet-luna/dx2/typography-publishing/`, `~/.cache/jet-luna/dx2/text-processing-parsing/`, `~/.cache/jet-luna/dx2/documentation-static-sites/` (report.md, census.json, claims.json). Deleted ballots with worked code: `~/.cache/jet-luna/dx2/ballots/` (grep the mechanism name). Family syntheses: `~/.cache/jet-luna/dx2/_families/*/synthesis.md`.

## Output

`~/.cache/jet-luna/dx3/prim-text/probe.md`, `gaps.json`, and the code under `~/.cache/jet-luna/dx3/prim-text/pkg/`. Gap ids start with `prim-text-G`.
