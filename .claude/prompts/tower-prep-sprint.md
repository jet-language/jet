# Tower prep sprint

Run the **tower-sweep** skill across the whole board. Goal: get every non-frozen
card to a clean state so I can burn them down later — **do not start any
implementation.**

When you're done, every card outside the **frozen** section must be exactly one of:
- **ready** — vetted plan in `tools/Tower/docs/sidequests/<slug>.md`, no open owner
  decision (implement on my "go"), or
- **deciding** — blocked on a house-format ballot card sitting in my queue.

Zero non-frozen cards left in an agent-action state (no "needs plan", no "decision
not drafted"). Leave **frozen** alone.

For each card:
- No vetted plan → write one (subagent) and have a *different* pass verify every
  claim against the codebase with `file:line`; refresh stale plans to the current
  ratifications.
- Reconcile board.json with reality: ratified+implemented → done (delete the plan);
  ratified+unbuilt → ready; lost/missing ballot linkages → fix; ratified decisions
  with no tracking card → create one; close stale-open `questions` whose decisions
  already ratified.
- Any genuine user-facing choice the ratified text didn't settle → develop a
  **house-format** ballot card (Gist / Story w/ American name / In the wild ```jet /
  Other languages / subagent-reviewed Tradeoffs / a worked example per option /
  Recommendation) with a rich original menu. **Never rank on effort/difficulty** —
  only safety, beginner UX, performance, one-path (I8), long-term correctness.
  Link it under `## <name> — board card cXX` and merge into
  `decision-ballots.md` yourself (single writer). Verify the ballot parses.

Honor invariants I1–I8 and every word in any owner ballot note. Use the Nix dev
shell for builds/tests; subagents stay read-only on the repo except their one plan
file and never run git-mutating commands. board.json is owner-owned — surgical
load-mutate-save only (`JSON.stringify(b, null, 2) + "\n"`).

Report the **ready vs deciding** split and the recommendation for each new ballot.
