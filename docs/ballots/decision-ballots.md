# Decision ballots — open owner queue

Every open decision, and **nothing else**. The instant a decision is submitted it
leaves this file: it is recorded in the decision log in
[`syntax-decisions.md`](../../../docs/spec/syntax-decisions.md) and removed here.
No "recently ratified" section, no decided history — decided decisions never
reappear.

**House rule for whoever edits this file (enforced — a card missing any of these
is not ballot-ready; Tower v2 Focus Mode renders these as labeled facets, so use
the exact bold labels):** every full decision card carries `**Gist:**` (one VERY
short plain sentence — the headline), `**Story.**` (a real person with an
American-traditional name and what they're doing), `**In the wild:**` (a fenced
```jet block of realistic project code where this bites), `**Other languages:**`
(short fenced blocks for Rust/TS/Swift/etc. when a cross-language compare helps),
`**Tradeoffs:**` (a compact table, one row per option, columns that actually
differ — subagent-reviewed), and a **worked example of every option** (each
`- **Option X — <name>.**` bullet with its own fenced ```jet/```shell block; mark
the recommended one `(recommended)`). Close with `**Recommendation:**` + a
one-line why. Put Owner Q&A in `**Owner Q …**` blocks — Tower routes those to a
separate Q&A facet, so keep them out of the recommendation. Decisions not yet
drafted to that bar belong on their Tower cards, not here.

---

## Open decisions

None. `node Tower/tower.mjs status` reported **0 decisions · 0 to
activate** on 2026-07-06.
