# Tower Prep Sprint

Run the `$tower` skill across the whole board. Goal: get every non-frozen card to
a clean state so the owner can burn them down later. Do not start implementation.

When done, every card outside `frozen` must be exactly one of:

- `ready` - vetted plan in `docs/sidequests/<slug>.md`, no open owner
  decision, ready to implement on the owner's go.
- `deciding` - blocked on a house-format ballot card in the owner's queue.

Leave zero non-frozen cards in an agent-action state: no needs-plan, no undrafted
decision, no stale question. Leave `frozen` alone.

For each card:

- If no vetted plan exists, write one and have a different review pass verify
  every claim against the codebase with `file:line`.
- Refresh stale plans to current ratifications.
- Reconcile board JSON with reality: ratified and implemented means done; ratified
  and unbuilt means ready; missing ballot linkages get fixed; ratified decisions
  with no tracking card get one; stale open questions whose decisions already
  ratified get closed.
- Any genuine user-facing choice not settled by ratified text becomes a
  house-format ballot: Gist, Story with an American first name, In the wild Jet
  code, Other languages, reviewed Tradeoffs, worked example per option, and
  Recommendation.
- Never rank on effort or difficulty. Rank only safety, beginner UX, performance,
  one-path design, and long-term correctness.
- Link ballots under `## <name> - board card cXX` and merge into
  `docs/ballots/decision-ballots.md`. Verify the ballot parses.

Honor invariants I1-I8 and every owner ballot note. Use the Nix dev shell for
builds/tests. Board JSON is owner-owned: mutate surgically and keep
`JSON.stringify(b, null, 2) + "\n"` formatting.

Report the ready vs deciding split and the recommendation for each new ballot.
