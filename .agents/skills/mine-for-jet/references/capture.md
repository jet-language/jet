# Mine for Jet: capture

Load this reference for every mining run, then load only the source-kind branch
that matches the named resource.

## Universal capture contract

Assign each resource a short stable `ID`. Keep captures under a gitignored path
such as `<repo>/target-mine-ID/`; never use `/tmp`. Delete the capture directory
when the task closes.

Record title, author/channel/org, publication or last-activity date, size
(duration, page count, commit/star/issue counts), description, linked sources,
and retrieval date. Inspect linked articles, papers, repositories, or
measurements. Prefer those primary sources when checking technical claims.

Read metadata first. Read the body in bounded slices. Read audience material in
strata only when the selected outcome needs it. For long or multi-session work,
resume from captured source files and completed claim records; do not create a
second run-tracking artifact.

Record capture/API problems—count mismatches, missing threads, blocked
downloads, and truncated pages—in the dated report and relevant claim records.
Capture completion proves retrieval coverage, not human-review completion.
Inspect captures with short ad-hoc Python. This host has no bare `python3`; use
`nix shell nixpkgs#python3 --command python3 …`.

## Source-kind branches

- [Video or talk](capture-video.md): metadata, subtitles, transcript, comments,
and linked sources.
- [Repository](capture-repository.md): docs, code, releases, design records,
and issue/discussion audience.
- [Article, paper, or docs page](capture-reading.md): canonical text, errata,
linked artifacts, and selected discussion.
- [Discussion thread](capture-discussion.md): bounded whole thread or explicit
sample of roots and high-signal replies.

Do not load branches for source kinds that are not in scope.
