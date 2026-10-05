# Capture once, review deeply

Load only the source-kind branch in scope. Give resources stable IDs and record
canonical URL/path, upstream identity, version/revision/subresource, retrieval
and publication dates, title/author, size and linked primary sources. Metadata,
retrieval, body review and extraction are distinct coverage fields; none implies
the others. Popularity is context, never truth.

`JET_SCRATCH_ROOT` defaults to the approved on-disk cache scratch location:

```sh
export JET_SCRATCH_ROOT="${JET_SCRATCH_ROOT:-$HOME/.cache/jet-dev/scratch}"
```

Run commands through jet-env. Set and pass an explicit capture root:

```sh
CAPTURE_ROOT="$JET_SCRATCH_ROOT/mine/$RUN_ID"
mkdir -p "$CAPTURE_ROOT"
```

Use a unique safe run ID. No repo target directories or `/tmp`. Commands below
assume the root and ID are supplied, not guessed. Text/transcripts first;
frames only for load-bearing on-screen code/diagrams/results. Read in bounded
chunks and reuse captures. No per-run ad-hoc Python; use reusable repository
scripts (the source checker is the existing Python helper) or host read tools.

Capture ends with a **segment index**: `coverage.json` listing each source's
bounded segments (IDs, locators, provisional titles) per the
[depth contract](depth.md). Legwork agents may produce captures and this index;
reading, ledgers and claims belong to the thinking model.

Bound linked-primary corroboration to load-bearing claims. A missing body,
truncated page, API warning, count mismatch or inaccessible thread is an explicit
coverage gap, not a reason to expand scope. Preserve enough exact evidence in
the retained result to understand and reproduce important claims before deleting
disposable captures. Scratch-only normalized JSON is not durable evidence.

Branches: [video](capture-video.md), [repository](capture-repository.md),
[reading](capture-reading.md), [discussion](capture-discussion.md).
