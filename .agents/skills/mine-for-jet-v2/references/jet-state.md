# Read-only Jet state digest (v1)

```sh
Tools/agent/jet-env node Tools/agent/jet-state.mjs --subject 'memory model' --format json
Tools/agent/jet-env node Tools/agent/jet-state.mjs --subject 'build system' --format markdown
```

Default root is the tool's checkout. `--root PATH` selects another checkout;
`--dev-ref` and `--base-ref` default to dev/master. Missing refs are unknown,
never substituted with another branch. `--format text` and `markdown` are the
same compact bounded preview; JSON retains all candidate rows. Subject terms
are OR retrieval leads, not proof of semantic equivalence or completeness.
Compact decision/card previews prioritize subject/title and phrase matches;
the labeled broader term and exact-link leads remain in JSON. This is retrieval
ordering, not recommendation value ranking or a semantic-equivalence verdict.

The tool reads the canonical live main-worktree board via Tower's existing path
resolver, starting from `--root` and its vendored `Tools/tower` CLI home rather
than assuming the tool module and selected checkout share a root. It never uses
a stale tracked linked-board fallback. It reuses Tower's
normalization/lane rules in memory, not openStore (which may repair on read).
Pending pair transactions are unavailable/inconsistent; the tool never repairs.
One board/history read and one indexed record projection per invocation, with
metadata checks around collection. All commands are local/read-only, with git
optional locks disabled. No fetch, service, build, test, Tower mutation or
committed generated status. Unknown/partial providers are explicit.
Identical committed heads share one diff within the invocation; clean local
lanes need no patch reads. Tower ledger changed paths are retained separately
as work-state changes, not repeatedly decoded as enormous code hunks. Oversized
or unsupported source patches retain paths with unknown hunk coverage; unknown
is never rendered as no semantic change.

JSON `jet-state:v1` contains generated date, checkout HEAD/dev/base/dirty
fingerprints, Tower revision, root-relative input identities, decisions with
separate intended/observed/work axes, open/draft ballots, matching cards/phases,
owner comments, explicit supersession and retained historical deletion/rejection/
merge context, dev and named-worktree committed/staged/unstaged/untracked changed
files and candidate hunks, dated research/unratified proposals with exact line
locators/source URLs, and active lexical registry spellings. The existing
lexical-ledger reader fails closed on unknown source rows. Syntax registry
presence is not runtime proof. Branch presence/diffs are not assignments.

Ratified-unbuilt-or-partial means ratified work is still open and actual
implementation completeness is unknown. Board done **never** becomes built
without a matching exercised receipt and implementation witness. Proposal-only
completed work stays proposal-only. Supersession follows explicit Tower verdict
edges (its `supersededBy` field names the replaced decision); dates do not resolve
unlinked conflicting decisions. Inspect direct records and focused code yourself.

## Optional exercised receipts

`--receipts PATH` supplies a JSON array of existing exercised evidence, not
permission for the digest to run it. Each usable record requires:

```json
[
  {
    "decision_id": "D-EXAMPLE",
    "scope": "decision-acceptance",
    "commit": "40-or-64-character-exact-checkout-HEAD",
    "exercised": true,
    "command": "exact previously exercised acceptance command",
    "witnesses": [
      { "path": "root-relative implementation path", "sha256": "exact source content hash" }
    ]
  }
]
```

The supplier must honestly establish **the entire stated decision acceptance
scope**, not relabel a narrow successful probe. The digest checks identity,
clean checkout and witness bytes; it cannot independently authenticate a
supplier's runtime claim. Historical/dirty/mismatched/partial receipts remain
unknown. Source path witnesses are portable and may not escape the root.
Missing receipts, optional pending caches, unrecorded owner chat and unparsed
semantic changes stay unknown. This portable v1 core has no harness/home/cache
prerequisite; it does not claim optional cache adapters are implemented.

## Disposable output and freshness

Stdout is default. `--output PATH` explicitly saves one new artifact (exclusive
creation, no overwriting). Use `$JET_SCRATCH_ROOT/mine/<run-id>/digest.json`, never
a committed status file. Exit 0 means collection saw no identity change, not
provider completeness; exit 2 marks inconsistent collection and requires a fresh
invocation before publication; exit 1 is invocation/collection failure. Dirty
files/index and Tower/source changes invalidate identity. No unbounded retry.

An optional checker `--evidence-index digest.json` joins this digest's retained
report source URLs to source inputs. Research/source URL occurrence is a prior-
coverage lead, never full body review or a complete historical inventory. The
source checker preserves historical `source_id` and reports a separate
`content_id` retaining resource-defining query/fragment selectors while removing
known tracking parameters. Approval applies to that exact content identity;
unknown historical selectors do not silently cover a new edition. Keep pinned
repository revision and subresource separately in capture evidence.

Focused fixture checks: `Tools/agent/jet-env node --test Tools/agent/jet-state.test.mjs`
and `Tools/agent/jet-env node --test .agents/skills/mine-for-jet-v2/scripts/check_coverage.test.mjs`.
The source-registry checker tests can separately be run with unittest discovery
in its scripts directory by the verification owner when Python use is authorized.
These are fixture proofs of projection/capture/coverage helpers, not compiler or
mined-source proof.
