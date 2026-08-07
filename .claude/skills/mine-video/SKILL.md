---
name: mine-video-for-jet
description: Mine one or more YouTube videos using resumable transcript/comment evidence, caption-quality checks, stratified anonymous comment samples, claim ledgers, linked sources, and cross-video contradiction synthesis; mine both macro lessons and micro details (APIs, features, syntax, ergonomics); cross-check findings against Jet's specs, code, tests, plans, and Tower; and, by default, log every actionable finding as deduplicated Tower cards and ballot-ready decisions. Use for "analyze this video for Jet," "extract lessons and pitfalls," "cross-check these videos," or "make Tower cards/ballots from this video."
---

# Mine Video for Jet

Extract evidence first. Persist progress. Separate video claims, comment signals, verified facts, Jet state, and recommendations. Never turn popularity into truth.

## Workflow

### 1. Establish scope

- Confirm the YouTube URL and any outcome the owner named.
- Tower logging is the default outcome (owner directive 2026-08-06): every actionable finding — gap, deferred consideration, measurement, owner gate — becomes a Tower card or ballot in the same run, never a "revisit later" line in the report. Skip Tower writes only when the owner explicitly says report-only.
- Repo edits remain unauthorized unless requested explicitly.
- Read `AGENTS.md`, then its required spec files in order. Read `.agents/skills/tower/SKILL.md` and `.agents/skills/tower-ballot/SKILL.md` before any Tower work.
- Preserve unrelated dirty-tree changes. Do not checkpoint or delegate when doing so would commit another worker's changes.

### 2. Capture source material

Use browser tools for metadata and linked primary sources. When YouTube does not expose transcript/comments through browser tools, use `yt-dlp`:

```sh
nix shell nixpkgs#yt-dlp --command yt-dlp \
  --skip-download \
  --write-subs --write-auto-subs --sub-langs 'en.*' --sub-format json3 \
  --write-comments --write-info-json \
  -o '/tmp/jet-youtube-%(id)s.%(ext)s' \
  'VIDEO_URL'
```

- Record title, channel, publication date, duration, description, linked sources, retrieved comment count, and retrieval date.
- Prefer creator subtitles over auto-captions. Label auto-caption uncertainty.
- If captions are absent, use an available transcription path or report the gap. Never infer the video's argument from title/description alone.
- Retrieve comments broadly. Keep root/reply counts and note incomplete threads or API warnings.
- Inspect linked articles, papers, repositories, or measurements. Prefer those primary sources when checking technical claims.

Inspect captures with short ad-hoc Python (the host has no bare `python3`; run `nix shell nixpkgs#python3 --command python3 …`). Never dump a whole capture file into context: read metadata first, then transcript in bounded slices, then stratified comment samples. A convenient pattern is one pass that parses the json3 subtitles into `/tmp/jet-youtube-ID.transcript.txt` (`[mm:ss] text` lines), then reading that file in chunks.

For long or multi-session work, keep a small progress manifest (`/tmp/jet-youtube-ID.manifest.json`) you update by hand: transcript ranges already read, caption source used (creator vs auto), comment sample IDs seen, linked-source statuses (`pending`, `retrieved`, `verified`, `unavailable`), and any retrieval warnings.

- Read the manifest before resuming. Continue missing transcript ranges and pending linked sources; do not restart completed chunks.
- Record capture/API problems (comment-count mismatches, missing threads, blocked downloads) in the manifest and in the final report.
- Treat capture completion as coverage of retrieval, not of human review.

### 3. Read the full argument

- Process transcript in bounded timestamp chunks until the full runtime is covered.
- Reconstruct thesis, causal chain, measurements, proposed fixes, caveats, and unresolved questions.
- Mine micro alongside macro. Architectural lessons are half the value; also harvest every concrete small thing the video or comments praise, demo, or complain about: a nice API shape, a pleasant syntax form, a single well-designed feature, an ergonomic default, a naming choice, an error-message style, a tooling nicety. Each micro item gets its own ledger row and Jet cross-check; do not fold micro items into a macro theme.
- Distinguish host commentary from material being quoted or read.
- Preserve timestamps for important claims, but paraphrase in final output unless a short quote is necessary.
- Note caption quality. Creator captions (`subtitles` in the info JSON) are high quality; auto/unknown captions are never high-confidence evidence without corroboration.
- Clean rows when parsing json3: drop empty events, deduplicate, and merge progressive auto-caption updates while retaining start times.

### 4. Mine comments without polling by applause

- Sample strata yourself from the info JSON `comments` array: top-liked roots, recent roots, low-liked technical comments (keyword-filtered), substantive replies, and corrections/disagreements (`actually`, `wrong`, `missing`, `what about`, tool names).
- Comments are anonymous by default. Surface an author only when identity materially affects credibility, then explain why.
- Group themes only after reading representative comments.
- Treat keyword counts and likes as discovery aids, never sentiment science.
- Separate:
  - repeated user pain;
  - factual correction;
  - alternative explanation;
  - workaround;
  - ecosystem preference;
  - joke/noise.
- Verify technical corrections against primary sources or local evidence before adopting them.
- Call out contradictions between video and comments rather than choosing whichever side sounds confident.

### 5. Build a claim ledger

Before Jet recommendations, write a compact JSON claim list. Give semantically equivalent claims the same `topic`; cross-video synthesis groups by that exact key.

```json
[
  {
    "topic": "incremental-cache-identity",
    "claim": "A cache must include backend and profile inputs.",
    "video_id": "ID",
    "source_kind": "transcript",
    "source_ref": "https://youtube.com/watch?v=ID",
    "source_identity": "video:ID",
    "timestamp": "12:34",
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

Allowed source kinds: `transcript`, `comment`, `linked-source`, `local-evidence`, `inference`. Confidence: `low`, `medium`, `high`. Stance: `supports`, `disputes`, `neutral`. Classifications: `already-implemented`, `ratified-in-progress`, `real-gap`, `rejected-conflict`, `needs-measurement`, `owner-gate`. Set `source_identity` to the shared upstream source when several videos repeat one article, paper, benchmark, or speaker; this prevents false independent corroboration.

Write the ledger to `/tmp/ID.claims.json` and validate it yourself before use: parse it as JSON, check every claim uses only the allowed enum values above, and check `topic` keys are unique per distinct claim (duplicates mean two claims should merge or one needs a sharper topic).

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

### 8. Synthesize multiple videos

Build one ledger per video, then group all claims by exact `topic` key (a short Python pass over the ledger files) into a topic matrix marking each topic `repeated`, `conflict`, or `single`.

- `repeated` means at least two independent source identities, not two videos repeating one upstream source. Review or set `source_identity` when provenance overlaps.
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
- manifest completion, caption quality, retrieval warnings, and unresolved linked sources;
- strongest lessons;
- corrections and disputed claims;
- Jet alignment and gaps;
- prioritized implement/avoid list;
- owner gates;
- Tower card/decision IDs when created;
- exact local file links and primary-source links.

Keep comments anonymous unless identity materially affects credibility. Avoid long transcript/comment quotations.

## Failure guards

- Do not claim complete comment coverage when retrieval warned otherwise.
- Do not expose commenter identities by default.
- Do not treat auto-caption cleanup as factual verification.
- Do not merge cross-video claims only because their wording resembles each other; use an explicit shared topic after semantic review.
- Do not attribute compiler cost to type checking without phase data.
- Do not compare clean and incremental builds, CPU-summed and wall time, or debug and release profiles as equivalents.
- Do not solve backend cost through hidden allocation, silent de-optimization, or new surface syntax.
- Do not expose backend diagnostics as Jet user errors.
- Do not mutate the repo merely because the report recommends work; Tower logging, by contrast, is mandatory for every actionable finding unless the owner said report-only.
- Do not report a finding as "revisit later" without a card that carries it.
- Do not skip micro findings (APIs, syntax, features, ergonomics) because they look too small to matter.
