---
name: mine-for-jet
description: Mine any external resource for Jet — YouTube videos, git repositories, articles, papers, docs sites, and discussion threads — using resumable captured evidence, claim ledgers, linked-source verification, and cross-resource contradiction synthesis; mine macro lessons and micro details (syntax, ergonomics, surfaces, APIs, types, defaults, error text, UX/DX, tooling) with equal weight; run the standing competitive lens — how Jet beats the subject on a level playing field, what to avoid, and what makes Jet the best language an agent could use; cross-check every finding against Jet's running binary, code, specs, tests, and Tower; and, by default, log every actionable finding as deduplicated Tower cards and ballot-ready decisions. Use for "mine this repo/video/article for Jet," "extract lessons and pitfalls," "cross-check these sources," or "make Tower cards/ballots from this."
---

# Mine for Jet

Extract evidence first. Persist progress. Separate the source's claims, audience signals, verified facts, Jet state, and recommendations. Never turn popularity into truth.

A resource is anything with an argument or a surface worth mining: a YouTube video, a git repository, an article or blog post, a paper, a docs site, a talk, a discussion thread. The spine below is identical for all of them; only step 2's capture playbook changes per kind.

## The standing lens

Apply `.agents/skills/_shared/standing-lens.md` in full: the four questions, the
five agent-optimality quantities, the micro sweep, probe the running binary, and
the honesty rules. The owner never has to ask for any of it, and a request for
"lessons" never narrows it.

Read that page before starting. The rest of this skill covers what is specific
to mining an external resource: getting the evidence out, judging its quality,
and not mistaking popularity for truth.

Two places the lens bites hardest here. A resource's own conclusion is evidence,
never the finding — state the real mechanism when the two differ. And the
subject is usually a language or tool that works, so question 2 always has
material: whatever it got wrong is on display next to whatever it got right.

## Workflow

### 1. Establish scope

- Confirm the resource URL (or path), its kind, and any outcome the owner named.
- Reject already-mined sources before expensive capture: run
  `scripts/check_sources.py <url>…` (from this skill's directory) against the
  registry `docs/reference/prior-art.md`. A tracked source needs an explicit
  `--allow-rerun` approval from the owner; a duplicate in one batch is an error.
- Tower logging is the default outcome (owner directive 2026-08-06): every actionable finding — gap, deferred consideration, measurement, owner gate — becomes a Tower card or ballot in the same run, never a "revisit later" line in the report. Skip Tower writes only when the owner explicitly says report-only.
- Repo edits remain unauthorized unless requested explicitly.
- Read `AGENTS.md`, then its required spec files in order. Read `.agents/skills/tower/SKILL.md` and `.agents/skills/tower-ballot/SKILL.md` before any Tower work.
- Preserve unrelated dirty-tree changes. Do not checkpoint or delegate when doing so would commit another worker's changes.

### 2. Capture source material

Universal rules, all kinds:

- Assign each resource a short stable `ID` and keep captures under `/tmp/jet-mine-ID.*`. `/tmp` is RAM-backed: never put multi-GB captures or clones there; use a gitignored disk path (e.g. `<repo>/target-mine-ID`) for anything large, and delete captures when the task closes.
- Record: title, author/channel/org, publication or last-activity date, size (duration, page count, commit/star/issue counts), description, linked sources, and retrieval date.
- Inspect linked articles, papers, repositories, or measurements. Prefer those primary sources when checking technical claims.
- Never dump a whole capture into context: read metadata first, then the body in bounded slices, then stratified audience samples.
- For long or multi-session work, keep a progress manifest (`/tmp/jet-mine-ID.manifest.json`) you update by hand: body ranges already read, capture quality notes, audience sample IDs seen, linked-source statuses (`pending`, `retrieved`, `verified`, `unavailable`), and retrieval warnings. Read it before resuming; continue missing ranges, never restart completed chunks.
- Record capture/API problems (count mismatches, missing threads, blocked downloads, truncated pages) in the manifest and in the final report.
- Treat capture completion as coverage of retrieval, not of human review.
- Inspect captures with short ad-hoc Python (the host has no bare `python3`; run `nix shell nixpkgs#python3 --command python3 …`).

**Video.** Use browser tools for metadata and linked primary sources. When YouTube does not expose transcript/comments through browser tools, use `yt-dlp`:

```sh
nix shell nixpkgs#yt-dlp --command yt-dlp \
  --skip-download \
  --write-subs --write-auto-subs --sub-langs 'en.*' --sub-format json3 \
  --write-comments --write-info-json \
  -o '/tmp/jet-mine-%(id)s.%(ext)s' \
  'VIDEO_URL'
```

- Prefer creator subtitles over auto-captions; label auto-caption uncertainty. Creator captions (`subtitles` in the info JSON) are high quality; auto/unknown captions are never high-confidence evidence without corroboration.
- If captions are absent, use an available transcription path or report the gap. Never infer the video's argument from title/description alone.
- Parse json3 into `/tmp/jet-mine-ID.transcript.txt` (`[mm:ss] text` lines): drop empty events, deduplicate, merge progressive auto-caption updates while retaining start times. Read it in chunks.
- Retrieve comments broadly. Keep root/reply counts and note incomplete threads or API warnings.

**Repository.** Clone shallow and blobless (`git clone --depth 50 --filter=blob:none`) to a disk path, or read hosted files directly.

- The body is layered; capture each layer: README and docs (what it claims), the code (what it does), CHANGELOG/releases (what changed and why), design docs/RFCs/ADRs (what was decided and what was rejected).
- The audience layer is the issue tracker and discussions: top-reaction issues, recently active issues, closed-as-wontfix decisions, and long-thread debates are the comment section. Capture via `gh`/`issue://`/`pr://` or the forum the project uses.
- Note repo vital signs (stars, contributor count, commit cadence, open/closed issue ratio) as context, never as truth.

**Article / paper / docs page.** Fetch the page or PDF (`read` on the URL first; browser only when that fails).

- Capture the canonical version; note publication date and any updates/errata.
- The audience layer is wherever the piece was discussed: HN, lobste.rs, Reddit, the blog's own comments. Capture the largest thread or two; record which threads you did not read.
- For a paper, capture the artifact/benchmark repo when one is linked; it outranks the prose.

**Discussion thread.** The thread is both body and audience. Capture it whole when bounded; otherwise top-level posts plus the highest-signal subthreads, and record what was skipped.

### 3. Read the full argument

- Process the body in bounded chunks — timestamp ranges for video, file/directory groups for a repo, sections for an article — until the whole resource is covered.
- Reconstruct thesis, causal chain, measurements, proposed fixes, caveats, and unresolved questions. For a repo the thesis is its design stance: what it makes easy, what it refuses to do, and what its issue tracker proves users actually hit.
- Run the **micro sweep** from the shared lens over the body, every
  category, not only the ones the resource dwells on. Each micro item gets its
  own ledger row and its own Jet cross-check. Never fold one into a macro theme,
  never drop one for looking too small. A category with nothing in it is a valid
  result; skipping it is not.
- Demos are a micro goldmine and easy to skim past. When the resource shows a
  command, a snippet, an error, or an editor session, read what is actually on
  screen: the exact spelling, the flags, the output shape, the message wording.
  That is primary evidence about a surface, unlike the narration over it. In a
  repo, the examples directory, test names, and error-message strings are the
  demo; README claims that the code contradicts are a finding, not a footnote.
- Distinguish the author's commentary from material being quoted or read.
- Preserve locators for important claims (timestamp, `file:line`, section, comment ID), but paraphrase in final output unless a short quote is necessary.

### 4. Mine the audience without polling by applause

The audience layer: video comments, repo issues/discussions, article threads.

- Sample strata yourself: top-liked/top-reaction roots, recent roots, low-visibility technical items (keyword-filtered), substantive replies, and corrections/disagreements (`actually`, `wrong`, `missing`, `what about`, tool names).
- Participants are anonymous by default. Surface an author only when identity materially affects credibility (e.g. a maintainer answering in their own tracker), then explain why.
- Group themes only after reading representative items.
- Treat keyword counts, likes, reactions, and stars as discovery aids, never sentiment science.
- Separate:
  - repeated user pain;
  - factual correction;
  - alternative explanation;
  - workaround;
  - ecosystem preference;
  - joke/noise.
- Verify technical corrections against primary sources or local evidence before adopting them.
- Call out contradictions between the resource and its audience rather than choosing whichever side sounds confident.

### 5. Build a claim ledger

Before Jet recommendations, write a compact JSON claim list. Give semantically equivalent claims the same `topic`; cross-resource synthesis groups by that exact key.

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
    "jet_evidence": "docs/plans/compiler-speed.md",
    "classification": "ratified-in-progress",
    "owner": "#666",
    "action": "Add hostile invalidation cases."
  }
]
```

Allowed source kinds: `primary` (the resource body: transcript, code, docs, article text), `audience` (comments, issues, discussion threads), `linked-source`, `local-evidence`, `inference`. Confidence: `low`, `medium`, `high`. Stance: `supports`, `disputes`, `neutral`. Classifications: `already-implemented`, `ratified-in-progress`, `real-gap`, `rejected-conflict`, `needs-measurement`, `owner-gate`. Set `source_identity` to the shared upstream source when several resources repeat one article, paper, benchmark, or speaker; this prevents false independent corroboration.

Write the ledger to `/tmp/jet-mine-ID.claims.json` and validate it yourself before use: parse it as JSON, check every claim uses only the allowed enum values above, and check `topic` keys are unique per distinct claim (duplicates mean two claims should merge or one needs a sharper topic).

### 6. Cross-check Jet

Search Jet specs, plans, code, tests, and Tower for each candidate lesson. Use `scripts/agent/jet-env` for project commands.

Classify every ledger item:

- `already implemented` — cite executable proof or code;
- `ratified/in progress` — cite decision, plan, or card;
- `real gap` — cite missing or contradictory behavior;
- `rejected/conflicts with law` — name governing invariant or decision;
- `needs measurement` — claim lacks Jet-specific evidence;
- `owner gate` — syntax, Core external dependency, invariant carve-out, or other owner-only choice.

Check actual code, not plans alone. Flag plan/implementation drift. Do not invent syntax or duplicate an existing card.

**Probe the running binary**, per the shared lens. The highest-value findings
come from taking the resource's central mechanism and running Jet's version of
it, rather than reading what Jet says it does. Build the smallest input that
should exercise the surface, run it through `scripts/agent/jet-env`, and read
the real output, exit code, and emitted paths. A live two-case contrast — one
input that works and one that should and does not — is the finding, in one
paste. When the subject is itself runnable (a repo, a tool), run its version of
the same probe too; a side-by-side of real outputs beats any paraphrase.

Run two-facet review:

- Beginner: magic defaults, no policy ceremony, direct diagnostics.
- Expert: exact backend, target, cache, generated-code, performance, and audit control through the same mechanism.

### 7. Form recommendations

For each validated gap, state:

1. evidence;
2. Jet impact;
3. exact action;
4. acceptance proof;
5. pitfall avoided;
6. owner gate, if any.

Keep measurement fixes separate from product choices. Prefer internal instrumentation before public syntax. Never trade safety, beginner experience, runtime performance, or one mechanical path for implementation ease.

Then answer the standing lens directly, as its own part of the report:

- **Beat vectors.** Rank by how categorical the win is, not by effort. For each:
  the source evidence, the mechanism, whether Jet's version is shipped or only
  ratified, and what a competitor would have to change to match it. A vector the
  subject cannot adopt without breaking its own model is worth more than a
  vector it could copy next release.
- **Avoid list.** One row per mistake: the mistake, its evidence, and Jet's
  exposure. Include mistakes Jet is structurally immune to; immunity is a design
  asset worth stating once.
- **Agent-optimality.** Walk the five quantities. Say which one this resource
  moves and which one Jet is weakest on right now.
- **Surface coverage.** Name types, methods, APIs, defaults, and commands in
  three groups: already covered with proof, worth checking, and missing. Use
  exact names so the list is actionable without re-derivation.

### 8. Synthesize multiple resources

Build one ledger per resource, then group all claims by exact `topic` key (a short Python pass over the ledger files) into a topic matrix marking each topic `repeated`, `conflict`, or `single`.

- `repeated` means at least two independent source identities, not two resources repeating one upstream source. Review or set `source_identity` when provenance overlaps.
- `conflict` means a topic has both supporting and disputing claims.
- Read every conflicting claim and primary source. Never resolve contradiction by count, likes, or confidence labels alone.
- Merge recommendations only when Jet impact and acceptance proof match. Preserve distinct mechanisms or contexts.

### 9. Write Tower (default)

- Log everything now. Any finding whose action is not "none" gets a Tower card (or attaches to an existing card) in this run. Never end with a deferred list; "revisit at 1.0" still means a card exists today, homed appropriately.
- Read Tower status and search for duplicates first.
- Create one card per coherent deliverable, not one card per bullet.
- Add one ballot per independently decidable owner choice. Use full tower-ballot fields and worked options.
- Link cards through `blockedBy` when order matters.
- Freeze cards only when owner asks. Record the owner's exact instruction through supported CLI attribution; never hand-edit `.tower/*.json`.
- Do not create ballots for choices already ratified. Attach work to the existing decision/card instead.
- Run `tower lint` after writes and read cards/decisions back.

## Output contract

Lead with verdict. Include:

- source coverage and limitations;
- manifest completion, capture quality (caption source, clone depth, truncated pages), retrieval warnings, and unresolved linked sources;
- strongest lessons;
- **verified defects, with the live contrast that proves each one**;
- corrections and disputed claims;
- Jet alignment and gaps, stating shipped versus ratified-and-unbuilt;
- **the avoid list** — mistake, evidence, Jet's exposure;
- **the beat vectors** — ranked, with shipped/unbuilt marked;
- **agent-optimality** — which of the five quantities this moves;
- **surface coverage** — covered, worth checking, missing, by exact name;
- prioritized implement/avoid list;
- owner gates;
- Tower card/decision IDs when created;
- exact local file links and primary-source links.

Follow the owner's report format: visual-first, tables over prose, example-led,
no hard wrapping, no stuffiness. Lead with the reframe when the popular reading
of the resource is wrong — say so plainly and give the real mechanism.

Keep audience members anonymous unless identity materially affects credibility. Avoid long body/audience quotations.

A one-line note about the strongest unverified assumption in the report is worth
more than another paragraph of confirmed findings. End with it when one exists.

## Failure guards

- Do not claim complete audience coverage when retrieval warned otherwise.
- Do not expose audience identities by default.
- Do not treat auto-caption cleanup, OCR, or scraped-page cleanup as factual verification.
- Do not treat a repo's README or a paper's abstract as its behavior; the code and the artifact outrank the prose.
- Do not merge cross-resource claims only because their wording resembles each other; use an explicit shared topic after semantic review.
- Do not attribute compiler cost to type checking without phase data.
- Do not compare clean and incremental builds, CPU-summed and wall time, or debug and release profiles as equivalents.
- Do not solve backend cost through hidden allocation, silent de-optimization, or new surface syntax.
- Do not expose backend diagnostics as Jet user errors.
- Do not mutate the repo merely because the report recommends work; Tower logging, by contrast, is mandatory for every actionable finding unless the owner said report-only.
- Do not report a finding as "revisit later" without a card that carries it.
- Do not wait to be asked for the competitive lens, the micro sweep, or the surface list. They are standing requirements of every run.

The shared lens carries the rest: probe before believing a spec, name where Jet
is behind, mark shipped versus designed, do not quote a metric from a column you
have not checked, confirm a surprising source-level finding with a second
reader, and never restate the subject's own conclusion as the finding.
