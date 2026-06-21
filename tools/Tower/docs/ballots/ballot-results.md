# Owner ballot results

Drop new owner decisions under `## Decisions` here; Claude processes them
**first** (ballot requests take precedence over all other work), ratifies into
`syntax-decisions.md`, strips the card, reconciles `board.json`, and implements
unblocked ones end-to-end. Processed batches move to `## Processed`.

## Decisions

_(none pending)_

## Processed

### Batch 2026-06-21 12:09 — D-REGION1 ratified (A & B together)

- **D-REGION1** A+B (c05) — allocation regions: **implicit scope-inferred default (A, beginner)** + **explicit `region r { … }` expert tier (B)**, both ratified per owner ("A & B together"). This **unblocks D-ALLOC2** (the scope-bound arena `view` now has its region mechanism). c05 → `implementation`; card stripped; count 16/12.
