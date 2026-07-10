# Unicode text package

**Card:** c66 / cuiw349. **Decision:** D-GRAPHEME1=B. **Status:** ready to build.

## Goal

Ship `core.text.unicode` as an opt-in first-party package with grapheme-cluster
iteration and Unicode normalization (NFC/NFD). Core strings stay small; text-heavy
programs get correct user-visible text handling.

## Constraints

- UCD tables live in the package, not Core, so ordinary binaries do not carry them.
- Runtime/package-side dependencies or generated tables need owner-approved hash pins;
  no compiler `Source/` dependency.
- API must make codepoint-vs-grapheme difference visible enough for beginners.

## Build Plan

1. Add `core.text.unicode` package registration and lock-file table hash recording.
2. Implement or wrap UAX #29 grapheme segmentation and canonical normalization.
3. Expose `unicode.graphemes(s)`, `unicode.grapheme_count(s)`,
   `unicode.normalize(s, .NFC/.NFD)`, and equality helpers only if they do not create a
   second string-comparison path.
4. Add examples for emoji + combining accents, normalization-before-compare, and cursor
   movement by grapheme.
5. Document when to use package APIs versus ordinary string iteration.

## Verification

- Unicode conformance fixtures for grapheme breaks and normalization.
- Golden examples with combining marks and emoji.
- Size check: importing nothing from `core.text.unicode` must not emit UCD tables.

