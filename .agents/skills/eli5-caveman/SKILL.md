---
name: eli5-caveman
description: >-
  Compress an ELI5 explanation for maximum brevity only when the user asks for
  ELI5 caveman or a brief beginner explanation. Keep meaning, comprehension,
  cause, caveats, negation, uncertainty, and safety.
---

# ELI5 caveman

This is a thin overlay. Read and apply `skill://eli5` for the beginner mental model and `skill://caveman` for compression. This file adds only the order and safety boundary.

## Persistence

Active for the current explanation unless the user asks to keep it active. If kept active, stop only on `stop ELI5 caveman`, `normal mode`, or a requested style change.

## Order

1. Build the accurate explanation with ELI5.
2. Keep the core sentence, necessary cause-and-effect chain, one concrete example, and every decision-changing caveat.
3. Define each unavoidable technical term in plain words at first use.
4. Apply caveman compression last. Cut filler, pleasantries, repetition, and needless detail.
5. Restore any word whose removal hides sequence, cause, negation, scope, uncertainty, or safety.

Compression comes last. It must not erase the bridge a beginner needs.

## Priority

When rules conflict, keep this order:

1. Correct facts, safety, uncertainty, and exact quoted or technical text.
2. Beginner comprehension without hidden prerequisites.
3. Clear cause, order, negation, and scope.
4. One concrete example.
5. Token reduction.

Keep `not`, `never`, `no`, `only`, `except`, and uncertainty words such as `may` and `usually`. Keep numbers, units, code, identifiers, paths, commands, API names, and error strings exact. Use full grammar for safety warnings, irreversible actions, ordered procedures, and subtle caveats. No baby talk, persona, style announcement, jargon pile, ornamental metaphor, history lesson, recap, or offer to explain more.

## Default shape

- Core: one short sentence.
- How: one to three short steps.
- Example: one short concrete case.
- Caveat: only when omission would teach a false model or cause a bad decision.

Skip labels when a plain paragraph is shorter. Do not add a recap. Every remaining word must earn its space without changing meaning.
