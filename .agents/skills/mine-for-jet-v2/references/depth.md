# Depth contract: segments, ledger, lane matrix, gate

"Exhaustive" is enforced by files, not prose. A run directory under the capture
root holds `coverage.json`, `ledger.json` and `claims.json` (and, after review,
`recall-audit.json`). The gate is:

```sh
Tools/agent/jet-env node .agents/skills/mine-for-jet-v2/scripts/check_coverage.mjs "$CAPTURE_ROOT"
```

Hand-in requires exit 0. Publication additionally requires `--audit` exit 0.
Exit 1 lists every gap by rule (`--summary` lists three per rule, counts stay
complete); exit 2 is a usage error. The output also prints per-source and
per-segment ledger and claim densities; keep it with the report.

## 1. Bounded segments (`coverage.json`)

Split every declared source into fixed segments before extraction. Each segment
has a globally unique `id`, a `title` saying what it covers, its locator, and
`retrieval: "captured"` plus `body_review: "full"`; anything else is a coverage
gap and fails.

| Kind | Bound | Locator and completeness |
| --- | --- | --- |
| `video`, `audio` | at most 5 minutes | `start`/`end` (`[h:]mm:ss` or seconds); segments are contiguous from 0 to the source `duration_s` |
| `text`, `discussion` | at most 1,500 words, never spanning sections | `section` (heading, post range) and `words`; segment words sum to the source `words` |
| `repo` | one module or directory | `path`; the source `modules` inventory is fully split into segments or `excluded` entries with specific reasons |

Sponsor reads, banter and intros stay segments: their ledger says why they are
out of scope. Nothing is skipped by leaving a hole.

```json
{
  "schema": "mine-for-jet-v2/coverage@1",
  "run_id": "RUN_ID",
  "sources": [{
    "source_id": "VIDEO_ID", "kind": "video", "title": "TITLE", "ref": "URL", "duration_s": 2379,
    "lanes_not_run": { "audience-corrections": "Owner did not request comments; none captured for VIDEO_ID." },
    "segments": [{
      "id": "VIDEO_ID/01", "title": "Stack versus heap; allocator calls per Vec push",
      "start": "00:00", "end": "05:00", "retrieval": "captured", "body_review": "full",
      "lanes": {
        "mechanism-and-architecture": { "claims": ["VIDEO_ID-03"] },
        "interop-and-portability": { "reason": "Allocator calls stay inside one Rust process; no foreign heap or platform ABI is shown." }
      }
    }]
  }]
}
```

Only `audience-corrections` may be switched off source-wide in `lanes_not_run`;
every other lane needs a row in every segment.

**Sharded video/audio.** When a long source is split across miners, each shard's
run directory declares the slice it owns on the source: `"range": { "start_s":
3600, "end_s": 5400 }` (absolute seconds, `end_s` ≤ `duration_s`; `duration_s`
stays the whole source's). Segments must then tile exactly `start_s`..`end_s`,
and every time is absolute source time: segment `start`/`end`, ledger `at`, and
timestamps in claim `locator`s (a shard starting at 1:00:00 never writes
`00:20`). Densities use the range length. Segment, ledger-item and claim IDs
stay unique across shards. Join and re-check before hand-in:

```sh
Tools/agent/jet-env node .agents/skills/mine-for-jet-v2/scripts/check_coverage.mjs \
  --merge "$CAPTURE_ROOT/shard-a" "$CAPTURE_ROOT/shard-b" --out "$CAPTURE_ROOT/merged"
```

Merge requires a `range` on every shard of a shared source and identical
`kind`/`title`/`ref`/`duration_s`, sorts segments, drops `range` once the shards
cover the whole duration, never overwrites `--out`, and exits by the re-check of
the merged run. The recall audit runs on the merged run.

## 2. Concept ledger before claims (`ledger.json`)

While reading each segment, enumerate **every** concept, technique, mechanism,
claim, demo, measurement and caveat the source presents, micro and macro, in
source order. Write the inventory first; judge relevance after. Then map each
item to exactly one of:

- `claims`: one or more claim IDs from the same segment;
- `covered`: `{ "ref", "reason" }` when the same concept and witness is already
  retained on a cited Jet record or earlier claim and this segment adds no new
  mechanism, measurement, variant or counterexample;
- `out_of_scope`: a reason naming the item and why it cannot improve, enhance,
  fill a gap in, fix, confirm or refute Jet.

A concept that confirms or refutes a Jet choice with a concrete mechanism is a
claim, not `covered`. Unmapped items, claims not traced from any ledger item, and
items whose `at` falls outside their segment fail.

```json
{
  "schema": "mine-for-jet-v2/ledger@1",
  "segments": {
    "VIDEO_ID/01": [
      { "id": "VIDEO_ID/01.L1", "at": "01:12", "kind": "mechanism",
        "concept": "Vec growth doubles capacity and reallocates", "claims": ["VIDEO_ID-03"] },
      { "id": "VIDEO_ID/01.L2", "at": "04:40", "kind": "claim",
        "concept": "Channel sponsor read", "out_of_scope": "Sponsor read for a course platform; no mechanism or Jet consequence." }
    ]
  }
}
```

`kind` is one of `concept`, `technique`, `mechanism`, `claim`, `demo`,
`measurement`, `caveat`. Text and repository items locate `at` by paragraph,
file or post.

## 3. Lane matrix

Every segment has a row for every lane in [review](review.md): non-empty
`claims` (from that segment) or a `reason`. Every claim appears in its own lane's
row. Reasons are rejected when they are generic (`not applicable`, `nothing
relevant`, `no finding`, `no delta`, `context only`), shorter than six words,
name nothing from the segment's title or ledger, or repeat another segment's
reason verbatim. A reason states what the segment actually shows and why that
lane gains nothing from it.

## 4. Claims, scale and density

`claims.json` is an array or `{ "claims": [...] }` of the normalized records in
[review](review.md); `segment` and `scale` (`micro`/`macro`) are required for
primary, audience and linked-source claims, and enums are strict. Each source
needs both micro and macro claims or a specific `scale_justification`.

A segment is low-density when its ledger has fewer than one item per minute
(video/audio), one per 250 words (text) or three per module (repository), or it
yields no claim. Low-density segments need a specific `density_justification`
(for example, a long live-coding stretch repeating one compile error). Thin is
the failure being guarded against; a justification is evidence, not an escape.

## 5. Recall audit (`recall-audit.json`)

After the gate passes, the fresh-context reviewer picks a seed and runs
`check_coverage.mjs RUN --sample SEED`, which selects at least 2 segments or 10%
of segments per source (all when fewer). For each sampled segment the reviewer
re-mines the raw transcript/body **without reading the ledger first**, lists
every concept, then matches each to a ledger item ID or `null`, and records
mapping disputes (an item that should have been a claim, a wrong `covered`).

```json
{
  "schema": "mine-for-jet-v2/recall-audit@1", "reviewer": "REVIEWER_ID", "seed": "SEED",
  "segments": [{ "segment": "VIDEO_ID/01",
    "concepts": [{ "at": "02:10", "concept": "Small-vector inline capacity", "ledger": null }],
    "disputes": [{ "ledger": "VIDEO_ID/01.L2", "reason": "Confirms a Jet choice; should be a claim" }] }]
}
```

Any `null` match or dispute fails `--audit`: the whole batch returns to its miner
for re-mining of every segment, not just the sampled ones, then a new audit with a
new seed. Keep failed audits as `recall-audit-N.json` and record every audit's
seed, sample, misses and outcome in the report.

## 6. Orchestration and batch size

- **Legwork agents** (cheap, fast helpers) only capture: transcripts, cleaned
  text, clones, metadata and the segment index (`coverage.json` with IDs,
  locators, provisional titles and no lane rows). They never write ledgers,
  claims, lane reasons or dispositions.
- **The thinking model** reads every segment body, writes the ledger while
  reading, then claims, lane rows and alignment. Extraction is never delegated
  to a legwork agent or summarized from someone else's notes.
- **Batch size** is set in hours of source per miner: give each miner only as
  many hours as it can read in full, segment by segment, with the ledger written
  during reading, inside one working context. Split long sessions across miners
  by segment range rather than skim; a miner that cannot finish hands back the
  remaining segments explicitly. When a source is split, whoever writes its
  macro claims reads every range's ledger first. Densities in the checker output
  show whether a batch was too large.
