# Card and milestone closeout

This is the sole detailed cadence for the orchestration, Tower, and verify
skills. `AGENTS.md` remains the policy authority. The board is the work ledger;
chat, worker receipts, and local notes do not replace it.

## Card loop

Repeat this sequence for each requested card or bounded slice:

1. Query Tower. If an actively claimed card has every criterion met and no open
   gate, close it and confirm a fresh query shows `done` before claiming or
   briefing anything else.
2. Brief and dispatch one bounded slice with one named implementer and a named
   close owner. Keep paths disjoint when independent slices are approved.
3. Harvest the worker. Accept code only with the exact `CHECK OK` receipt, or
   accept prose/static work with `DOCS ONLY`; otherwise return the same card
   with the exact missing/error line. Workers never write Tower, close cards,
   claim unrun proof, or spawn workers.
4. Inspect the owned diff and integrate the complete cutover into the intended
   tree. Do not lose a coherent patch, overwrite another task's paths, or leave
   callers, tests, tools, docs, generated uses, environment variables, or CLI
   spellings on the obsolete contract.
5. Run the one exact focused proof named by the card's observable criterion
   against the integrated tree. A nearby suite, type-check, worker receipt, or
   absence of an error is not a substitute.
6. Record the command and its result as criteria evidence. Close immediately
   when the criteria are met, then query Tower again for `done`.
7. Refill only after `done` is confirmed. Keep blockers, owner gates, and
   handoffs in Tower; do not maintain a competing task ledger.

A source-only result can satisfy only source/static criteria. It cannot mark an
unrun runtime, execution tier, diagnostic, snapshot, golden, or generated
artifact proof green. If a proof was not run, record it as unknown or pending.

## Linked-card milestone

After **every linked card** for a milestone is `done`, freeze the integrated
source in a commit and open the commit-bound token:

```sh
scripts/agent/closeout-gate.mjs open MILESTONE --by AGENT
```

The gate must select the frozen source commit, a qualified candidate at that
commit, and a review-ready milestone. Only that token authorizes **one composed
targeted sweep** and **one fresh-context integrated-diff review**, covering
every applicable I9 tier. Broad proof, an unfiltered conformance census,
`proof-parallel.sh`, and `verify-full.sh` do not substitute for card evidence
and refuse without the token. Do not reuse a token after the source changes.

The fresh reviewer receives the integrated milestone diff, acceptance criteria,
relevant authority and invariants, and implementation evidence. The review
checks concrete bugs, missed paths, false-green evidence, invariant breaks,
stale decisions, scope drift, duplicate mechanisms, orphaned work, and I9
drift. The reviewer does not implement. A finding reopens only its owning card
and affected criteria: fix it in the named implementation slice, integrate the
delta, run its affected focused proof, close the card, freeze the new source,
and open a new token before resuming the composed sweep.

## Conditional phase order and completion

The normal cadence proves each integrated card before closure. An explicit owner
request for implementation-before-validation overrides that ordering for the
requested implementation cards only. Do not activate that mode without the
request, and do not let it turn source receipts into runtime proof. Once the
owner-directed implementation phase ends, resume the exact criterion evidence
and milestone sequence above.

Successful completion requires every requested path integrated, every
applicable criterion evidenced, and Tower showing the true completed state.
An honest blocked return is allowed only after all reachable work is finished
and every remaining path has a concrete owner gate or external blocker,
responsible party, and continuation condition in Tower. Agent-fixable failures
are work, not external blockers. Leave unmet criteria open and say blocked,
not done. In either outcome, account for every worker, proof, worktree, branch,
and handoff; nothing may remain unowned or silently in flight.
