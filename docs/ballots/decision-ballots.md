# Decision ballots — open owner queue

Every open decision, and **nothing else**. The instant a decision is submitted it
leaves this file: it is recorded in the decision log in
[`syntax-decisions.md`](../../../docs/spec/syntax-decisions.md) and removed here.
No "recently ratified" section, no decided history — decided decisions never
reappear.

**House rule for whoever edits this file (enforced — a card missing any of these
is not ballot-ready; Tower v2 Focus Mode renders these as labeled facets, so use
the exact bold labels):** every full decision card carries `**Gist:**` (one VERY
short plain sentence — the headline), `**Learn this first:**` (a zero-context
mini lesson defining the concept, mechanics, unavoidable terms, stakes, and one
tiny example well enough that the owner could explain it to someone else),
`**Story.**` (a real person with an
American-traditional name and what they're doing), `**In the wild:**` (a fenced
```jet block of realistic project code where this bites), `**Other languages:**`
(short fenced blocks for Rust/TS/Swift/etc. when a cross-language compare helps),
`**Tradeoffs:**` (a compact plain-language table, one row per option, columns that actually
differ — subagent-reviewed), and a **worked example of every option** (each
`- **Option X — <name>.**` bullet with its own fenced ```jet/```shell block; mark
the recommended one `(recommended)`). Move exact protocol or compiler law into
an optional `**Technical details:**` appendix. After every independent option
is complete, add `**Hybrid pass:**`: harvest the strongest compatible idea from
every option into one resulting option, or name the exact conflict preventing a
combination. Only then close with `**Recommendation:**`
that explains why the winner wins, why every other option loses here, and which
downside the recommendation accepts. Define jargon, expand acronyms, and lead
with user impact. Put Owner Q&A in `**Owner Q …**` blocks — Tower routes those to a
separate Q&A facet, so keep them out of the recommendation. Decisions not yet
drafted to that bar belong on their Tower cards, not here.

---

## Open decisions

Open ballots live in Tower (`node Tower/tower.mjs status`); this file no
longer mirrors them. As of 2026-07-11 the open set is the polyglot binder
wave and the framework-lessons wave:

- card #502 — D-FFI-GO1, D-FFI-JVM1, D-FFI-DOTNET1, D-FFI-FORTRAN1
- card #503 — D-FFI-LUA1, D-FFI-RUBY1, D-FFI-PERL1, D-FFI-PHP1,
  D-FFI-R1, D-FFI-SH1
- card #504 — D-FFI-COBOL1, D-FFI-OCTAVE1, D-MIGRATE-SRC1
- card #505 — D-LIVEQUERY1, D-SCHEDULE1, D-LINTPOLICY1

Everything from the 2026-07-11 surface-condensation and Polyglot Phase 1
waves (cards #497, #498, #499, #501) was ratified same-day and is
recorded in `docs/spec/syntax-decisions.md`.
