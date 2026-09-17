# Recovery after interruption

Use this reference before dispatching again after a crash, timeout, worker
failure, or tangled integration. The goal is to preserve owned work and restore
one truthful ledger, not to reset the checkout.

## Account first

Account for the current Tower cards and phases, claims and handoffs, worker
jobs, worktrees, branches, uncommitted paths, integrated commit, and pending
proofs. Use pid-aware process status plus `hub jobs` or `hub wait`; a missing
log marker alone is not proof of process failure. Keep the last integrated state
explicit in the recovery note or handoff.

## Salvage narrowly

- Checkpoint only the owned paths on a recovery branch, using explicit paths.
- Salvage a dead worker's coherent diff or commit and inspect it before
  integration. Rebrief only the missing slice from the last integrated state.
- Preserve disjoint sibling work. Never broad-restore, discard, overwrite, or
  reset another task's paths, worktree, or branch.
- After integration, remove only finished in-repository worktrees and temporary
  branches. Do not remove an active worktree or a pending proof's evidence.

If ownership, the integrated state, or a pending proof cannot be accounted for,
pause that slice, keep the card open, and resolve the ledger with the completion
owner before dispatching new work. Independent, accounted-for slices may
continue.
