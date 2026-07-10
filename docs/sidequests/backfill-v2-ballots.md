# Plan: Back-fill open decisions to Tower v2 ballot schema

**Status:** planned. No language decision required.

## Goal

Upgrade remaining open ballot cards to the richer Tower v2 shape without changing their
substantive recommendation.

## Required Card Shape

Each full decision card should have:

- `Gist`
- `Story`
- `In the wild`
- `Other languages`
- `Tradeoffs`
- worked examples for each option
- `Recommendation`

## Implementation Steps

1. Parse the current open decisions list with `node Tower/tower.mjs status`.
2. For each v1-shaped card, add the missing facets.
3. Preserve existing recommendation and option semantics.
4. Keep owner Q&A in `Owner Q` blocks so Focus Mode routes it separately.
5. Run Tower status after each batch.

## Verification

- `node Tower/tower.mjs status`
- Manual Focus Mode spot-check for a representative card.
