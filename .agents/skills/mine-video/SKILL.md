---
name: mine-video-for-jet
description: Mine one or more YouTube videos using duplicate-safe source tracking, resumable transcript/comment evidence, caption-quality checks, stratified anonymous comment samples, claim ledgers, linked sources, and cross-video contradiction synthesis; update Jet's prior-art registry for every completed mine; cross-check findings against Jet's specs, code, tests, plans, and Tower; and, only when explicitly requested, create deduplicated Tower cards and ballot-ready decisions. Use for "analyze this video for Jet," "extract lessons and pitfalls," "cross-check these videos," or "make frozen Tower cards/ballots from this video."
---

# Mine Video for Jet

Extract evidence first. Persist progress. Separate video claims, comment signals, verified facts, Jet state, and recommendations. Never turn popularity into truth.

## Workflow

### 1. Establish scope

- Confirm the YouTube URL and requested outcome: report only, repo changes, Tower cards, ballots, or some combination.
- Treat Tower writes and repo edits as unauthorized unless requested explicitly. The mandatory `docs/reference/prior-art.md` source-registry update is the sole exception.
- Read `AGENTS.md`, then its required spec files in order. Read `plugins/tower/skills/tower/SKILL.md` and `plugins/tower/skills/tower-ballot/SKILL.md` before any Tower work.
- Preserve unrelated dirty-tree changes. Do not checkpoint or delegate when doing so would commit another worker's changes.
- Extract each stable YouTube video ID and run the duplicate preflight before capture:

```sh
scripts/agent/jet-env python3 \
  .agents/skills/mine-video/scripts/check_sources.py VIDEO_URL...
```

- Stop before transcript, comment, or linked-source retrieval when any ID is already tracked. Show the existing registry line and ask for explicit rerun confirmation.
- Continue a duplicate only after the owner clearly says to rerun that exact video. Then pass `--allow-rerun VIDEO_ID` for that ID and record the confirmation in the report. Repeat the flag for each separately confirmed ID; never approve a whole batch.
- Do not treat a different playlist index, query string, shortened URL, or timestamp as a different video.
- Treat the same ID repeated inside one input batch as an input error. Remove the repeated input; `--allow-rerun` does not mine it twice.

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

Run a cut-level polish pass after the mechanism pass. Check small improvements across:

- syntax and API shape;
- names, defaults, and discoverability;
- diagnostics, help, examples, and fixes;
- microcopy, information density, progress, latency feedback, copy/paste, and recovery affordances;
- terminal, editor, LSP, semantic-index, and Canvas parity;
- empty, loading, invalid, offline, permission, stale, interrupted, resume, and rollback states;
- keyboard, focus, accessibility, ANSI, and `NO_COLOR`;
- performance attribution, profile/target disclosure, and audit receipts;
- install, migration, maintenance, recovery, and expert override paths.

For every praised or lauded shape, capture the exact syntax, API, interaction, or presentation quality that earned praise. Then test whether Jet already proves it or can adopt its useful property. Mine small moments of clarity and delight, not only failures and large mechanisms.

Do not call a cut covered because an umbrella card has a similar title. Map it to exact open criteria, a ratified decision, or executable proof.

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

For a multi-video mine, a zero-new-work result needs a written proof. For every actionable cut, cite one of:

- exact existing Tower criterion that owns it;
- executable code/test proof that it already works;
- governing law that rejects it;
- measurement needed before product work.

If no exact criterion or proof owns the action, classify it as a real gap. Propose one coherent card. If the gap needs an owner choice, make the options ballot-ready. Do not use broad alignment as a substitute for this audit.

### 8. Synthesize multiple videos

Build one ledger per video, then group all claims by exact `topic` key (a short Python pass over the ledger files) into a topic matrix marking each topic `repeated`, `conflict`, or `single`.

- `repeated` means at least two independent source identities, not two videos repeating one upstream source. Review or set `source_identity` when provenance overlaps.
- `conflict` means a topic has both supporting and disputing claims.
- Read every conflicting claim and primary source. Never resolve contradiction by count, likes, or confidence labels alone.
- Merge recommendations only when Jet impact and acceptance proof match. Preserve distinct mechanisms or contexts.

### 9. Persist source provenance

Update `docs/reference/prior-art.md` for every completed mine, including report-only runs and runs with no Jet recommendation.

- Add each video once under **Videos, talks & podcasts**.
- Record title, creator/channel, canonical `https://www.youtube.com/watch?v=ID` URL, mine date, and one concise adopted/rejected lesson.
- Link the durable report, Tower cards, or decisions when they exist.
- Keep the canonical video ID searchable as plain text.
- Re-run `.agents/skills/mine-video/scripts/check_sources.py --verify-tracked VIDEO_URL...` after the edit. Pass every completed video URL; each ID must report `tracked`.
- Never leave provenance only in `/tmp`, a chat response, a Tower log, or a playlist URL.

Example, first mine:

```text
input:  https://youtu.be/abc123DEF45?t=90
check:  new
action: mine video, then add https://www.youtube.com/watch?v=abc123DEF45 to prior-art.md
```

Example, duplicate:

```text
input:  https://www.youtube.com/watch?v=abc123DEF45&list=PL...
check:  tracked at docs/reference/prior-art.md:52
action: stop and ask; rerun only after explicit confirmation
rerun:  --allow-rerun abc123DEF45
```

### 10. Write Tower only when requested

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
- source-registry additions and any explicitly confirmed reruns;
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
- Do not mutate Tower or repo merely because report recommends work.
- Do not start capture before duplicate preflight.
- Do not finish a completed mine before `docs/reference/prior-art.md` tracks every video ID.
- Do not claim zero new work from broad architectural alignment or umbrella-card titles.
