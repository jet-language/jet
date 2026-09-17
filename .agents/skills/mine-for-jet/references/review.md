# Mine for Jet: review and claims

Load this reference after capture when the selected outcome needs interpretation,
Jet cross-checks, or recommendations. Do not run unrelated evidence lanes.

## Read the named source

Process the body in bounded chunks: timestamp ranges for video, file or
 directory groups for a repository, sections for an article, or post ranges for
a thread. Cover the declared resource before stopping.

Reconstruct thesis, causal chain, measurements, proposed fixes, caveats, and
unresolved questions. For a repository, treat design stance as the thesis: what
it makes easy, refuses to do, and what its issue tracker shows users hit.

Use the micro sweep only for categories relevant to the requested subject. A
broad comparative outcome loads every category from the standing lens. Each
concrete item gets its own ledger row and Jet cross-check. An empty relevant
category is a valid result; skipping an applicable category is not.

Demos are primary surface evidence. When a source shows a command, snippet,
error, or editor session, record exact spelling, flags, output shape, and
message wording. In a repository, examples, test names, and error strings are
demos. If README claims contradict code, report the contradiction.

Distinguish author commentary from quoted material. Preserve locators for
important claims: timestamp, `file:line`, section, issue, or comment ID.
Paraphrase in the final report unless a short quote is needed.

## Audience evidence

Load this section only when audience evidence is requested or materially bears
on the named question. Sample top-liked or top-reaction roots, recent roots,
low-visibility technical items found by keywords, substantive replies, and
corrections or disagreements (`actually`, `wrong`, `missing`, `what about`,
tool names).

Keep participants anonymous by default. Name an author only when identity
materially affects credibility, such as a maintainer answering in its tracker,
and explain why. Group themes after reading representative items.

Use keyword counts, likes, reactions, and stars only as discovery aids. Separate
repeated user pain, factual correction, alternative explanation, workaround,
ecosystem preference, and noise. Verify technical corrections against primary
sources or local evidence. Report contradictions between source and audience;
do not choose by confidence or popularity.

## Claim ledger

Before Jet recommendations, write a compact JSON claim list. Give semantically
equivalent claims the same `topic` so multi-resource synthesis can group them.
Every claim keeps source identity and an exact locator:

```json
[
  {
    "topic": "incremental-cache-identity",
    "claim": "A cache must include backend and profile inputs.",
    "source_id": "ID",
    "source_kind": "primary",
    "source_ref": "https://github.com/org/repo",
    "source_identity": "repo:org/repo",
    "locator": "docs/design/cache.md §3",
    "confidence": "high",
    "stance": "supports",
    "correction": null,
    "jet_evidence": "docs/spec/reference/compiler-speed.md",
    "classification": "ratified-in-progress",
    "owner": "#666",
    "action": "Add hostile invalidation cases."
  }
]
```

Allowed `source_kind`: `primary` (resource body), `audience`, `linked-source`,
`local-evidence`, or `inference`. Confidence is `low`, `medium`, or `high`.
Stance is `supports`, `disputes`, or `neutral`. Classification is one of
`already-implemented`, `ratified-in-progress`, `real-gap`,
`rejected-conflict`, `needs-measurement`, or `owner-gate`.

Set `source_identity` to the shared upstream source when several resources
repeat one article, paper, benchmark, or speaker. This prevents false
independent corroboration. Write the ledger to `target-mine-ID/claims.json`.
Parse it as JSON, reject unknown enum values, and make each distinct claim's
`topic` intentional: merge duplicate claims or sharpen their topics.

## Jet cross-check

Cross-check only claims relevant to the named outcome against the smallest
authoritative Jet slices. Use `scripts/agent/jet-env` for project commands.
Classify each item:

- `already implemented`: cite executable proof or code;
- `ratified/in progress`: cite decision, plan, or card;
- `real gap`: cite missing or contradictory behavior;
- `rejected/conflicts with law`: name the governing invariant or decision;
- `needs measurement`: the claim lacks Jet-specific evidence;
- `owner gate`: syntax, Core dependency, invariant carve-out, or other owner-only
  choice.

Check actual code, not plans alone. Flag plan/implementation drift. Do not
invent syntax or duplicate an existing card.

Probe a running binary only when the claim concerns executable behavior or the
selected outcome asks for a live contrast. Build the smallest representative
input, run it through `scripts/agent/jet-env`, and record output, exit code, and
emitted paths. If the subject is runnable, run its equivalent probe too. A
working and a failing case beats a paraphrase.

Use beginner and expert facets only when relevant:

- beginner: magic defaults, policy ceremony, and direct diagnostics;
- expert: exact backend, target, cache, generated-code, performance, and audit
  control through the same mechanism.

## Recommendations

For each validated gap, state evidence, Jet impact, exact action, acceptance
proof, pitfall avoided, and owner gate. Keep measurement fixes separate from
product choices. Prefer internal instrumentation before public syntax. Never
trade safety, beginner experience, runtime performance, or one mechanical path
for implementation ease.

Answer the relevant standing-lens questions in the report. For comparative or
broad outcomes, include:

- **Beat vectors:** source evidence, mechanism, shipped or ratified state, and
  what the subject must change to match Jet;
- **Avoid list:** mistake, evidence, and Jet exposure, including structural
  immunity once;
- **Agent optimality:** which of the five quantities the source moves and which
  one Jet is weakest on;
- **Surface coverage:** exact types, methods, APIs, defaults, and commands that
  are covered, worth checking, or missing.

Do not manufacture a competitive comparison for a focused source question.

## Multiple resources

Load this section only when multiple resources are in scope. Keep one ledger per
resource. Group claims by exact `topic` into a matrix marked `repeated`,
`conflict`, or `single`.

`repeated` requires at least two independent `source_identity` values, not two
resources repeating one upstream source. `conflict` has both supporting and
disputing claims. Read every conflicting claim and its primary source. Never
resolve a conflict by count, likes, or confidence labels. Merge recommendations
only when Jet impact and acceptance proof match; preserve distinct mechanisms or
contexts.
