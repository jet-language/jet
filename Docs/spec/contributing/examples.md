# Example authoring

Examples are executable specifications under I5 in
[`AGENTS.md`](../../../AGENTS.md). The source file and its expected output are
the behavior contract; the learning order is maintained in
[`Examples/README.md`](../../../Examples/README.md). This page explains how to
add or rewrite an example without losing a consumer such as a golden harness,
editorial link, or generated artifact. Name streams, readers, and events as the
[Jet vocabulary](../vocabulary.md) defines them.

## Lead with the safe path

1. Put the short path first. Teach the safe default, such as `para_map` /
   `task`, streaming readers, or `ui.mount`.
2. Put expert control beside it. Keep the long manual form in a sibling
   `*_expert.jet` file with a one-line header naming it as the expert variant.
3. Match both execution lenses. For a runnable example, prove both commands:

   ```sh
   jet run path/to/example.jet
   jet run --release path/to/example.jet
   ```

   Update only the matching file under `Examples/features/expected/` after the
   output agrees on both paths.
4. Do not hide a floor. If a topic has a directory such as `math/` or
   `lowlevel/`, put a beginner flagship there instead of leaving the only proof
   in tests.

The short example should make the common operation obvious. The expert
variant should expose control that a reader can choose deliberately, not a
second mechanism with a different meaning.

## Keep every consumer aligned

Before moving or renaming an example, search for its path in expected output,
golden discovery, corpus manifests, harness lists, decision records, and docs.
Then update every consumer in the same cutover. A path can be part of expected
output even when the program's values are unchanged.

Use [`agent-engineering.md`](agent-engineering.md) for fixture and generated-
artifact traps. Preserve the example's meaning; do not rewrite a snapshot only
to hide a compiler or runtime regression.

## Core conformance witnesses

Ordinary witnesses under `tests/conformance/corpus` use the public module
function's path and start with `// core-conformance: <module>.<function>`.
Check the declaration in `Core/` before choosing the target: a public receiver
method is not a free module function, and a private helper does not create a
denominator row. Exercise those paths through the public constructor or
operation's witness instead. Keep regression cases with that witness rather
than inventing a function name from a test filename. If an approved contract
requires a missing export, report the source/contract mismatch; do not expand
the Core API just to make the denominator pass.

Run `Tools/agent/jet-env node Tools/agent/core-conformance.mjs --check` before
the filtered `dev_corpus_gate` three-tier proof.
