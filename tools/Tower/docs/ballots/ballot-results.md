# Owner ballot results

Drop new owner decisions under `## Decisions` here; Claude processes them
**first** (ballot requests take precedence over all other work), ratifies into
`syntax-decisions.md`, strips the card, reconciles `board.json`, and implements
unblocked ones end-to-end. Processed batches move to `## Processed`.

## Decisions

**D-DBG2** — Policy for frames with no Jet source line
Decision: **C**
⚠️ **HELD — NOT ratified (conflicts with invariant I2).** Option C ("show the raw
Rust frame: file + line") is flagged in the ballot card itself as a **direct I2
violation** — I2 says rustc/Rust internals never speak to users. Ratifying C would
require amending a hard invariant, which is above an agent's authority, so this one
decision is deferred back to you. If you want raw Rust frames *anyway* (e.g. behind
a `jet debug --raw`/expert flag so the default stays I2-clean), say so and I'll
ratify that scoped form. Otherwise the recommended **A** (silent step-over) or **B**
(synthetic `[jet runtime]` frame) keep I2 intact — pick one and I'll ratify it.

## Processed

### Batch 2026-06-22 00:11 — owner decisions (7 ratified, 1 held)

Ratified into `syntax-decisions.md`, cards stripped from `decision-ballots.md`,
board reconciled:

- **D-STATE1** A (c71) — typestate via transitioning tags. D-QUAL2 (tag kind)
  ratified 2026-06-21 → **unblocked**; implementable (sequence after `#SingleUse`).
- **D-DET1** A (c99) — `pure` ⇒ reproducible; inject `Clock`/`Rng`;
  `assume_deterministic { }` escape. *Implementation gated on D-EFF1* (the
  effect-tracking pass is the enforcement engine).
- **D-TXN2** A (c100) — reject irreversible effects inside `#transact { }`.
  *Gated on D-EFF1* (effect classification) and ships with D-TXN1.
- **D-EXT1** A (c101) — extensibility ceiling: Tier 1 open to all, Tier 2
  stdlib-only, Tier 4 rejected; banks the local/global-footgun rule + the two
  principles (mark library syntax; diagnostics are the ceiling). Standing policy.
- **D-CTIO1** B (c-comptime-io) — ratify `embed_file`/`embed_bytes` + literal-path,
  no-`..`-escape rules; no broad build-time I/O. **Directly implementable.** Owner
  comment honored: option C (broad gated build I/O) recorded as a far-horizon idea
  card (`tools/Tower/docs/ideas/build-time-io-far-horizon.md`).
- **D-CTX1** G2 (c74) — Smart Context grammar `#context(field: value) { … }` (reuses
  Jet's one `name: value` spelling). Q1=A2 / Q2=Cβ already owner-set. Implementable.
- **D-ROUTE1** A (c83) — HTTP route registration & dispatch surface. Implementable.

### Batch 2026-06-21 12:09 — D-REGION1 ratified (A & B together)

- **D-REGION1** A+B (c05) — allocation regions: **implicit scope-inferred default (A, beginner)** + **explicit `region r { … }` expert tier (B)**, both ratified per owner ("A & B together"). This **unblocks D-ALLOC2** (the scope-bound arena `view` now has its region mechanism). c05 → `implementation`; card stripped; count 16/12.
