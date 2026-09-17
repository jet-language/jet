---
name: mine-for-jet
description: >-
  Mine a named external source for Jet evidence and surface lessons. Use when
  the owner names a repository, video, article, paper, docs page, or discussion.
---

# Mine for Jet

Extract evidence first. Keep source claims, audience signals, verified facts,
Jet state, and recommendations separate. Popularity is context, not truth.

## Scope and permission

Name the source URL or path, source kind, Jet subject, workload, and requested
outcome before capture. Set a finite source and subject boundary. A request for
“lessons” does not widen that boundary. Run only the evidence lanes relevant to
the outcome: source capture is universal; audience review, the micro sweep,
live probes, and cross-resource synthesis are conditional. Record an evidence
gap instead of expanding scope.

Read `.agents/skills/_shared/audit-dispositions.md` before running. It owns
shared permission and publication mechanics. Normal mining keeps this method's
Tower logging obligation. An explicit `report-only` request overrides it:
produce the report and read existing Tower records only; create no cards,
decisions, ballots, or implementation edits. Repo edits are unauthorized unless
the owner explicitly asks.

Apply `.agents/skills/_shared/standing-lens.md` contextually. Its comparative,
micro, live-probe, and honesty sections apply only when the declared outcome
needs them; a broad comparison expands the relevant sweep.

Use `AGENTS.md` task-triggered lookup. Read only relevant authoritative slices:

- semantics or syntax: `docs/spec/philosophy.md`,
  `docs/spec/syntax-decisions.md`, `docs/spec/architecture.md`;
- diagnostics: `docs/spec/diagnostics.md` and matching snapshots;
- Tower mechanics or an owner-gated choice:
  `plugins/tower/skills/tower/SKILL.md`, plus tower-ballot only for that choice.

## Check the source registry

From the repository root, run the checker with an explicit repository-root
script and registry path:

```sh
scripts/agent/jet-env python3 \
  .agents/skills/mine-for-jet/scripts/check_sources.py \
  --registry docs/spec/reference/prior-art.md \
  'SOURCE_URL'
```

Replace `SOURCE_URL` with the requested source; pass additional URLs as separate arguments.

The checker reports `new`, `tracked`, or `duplicate-input`. A tracked source
needs explicit owner approval in `--allow-rerun SOURCE` before recapture. A
duplicate in one batch is an error. `--verify-tracked` returns failure for an
untracked input. Exit status `3` means duplicate input or an unapproved tracked
source; status `4` applies to `--verify-tracked` with an untracked source.

For an approved rerun, add `--allow-rerun SOURCE_ID` to the same command. Use
the source ID reported by the checker, not an unchecked URL spelling. The
checker does not authenticate owner approval; the caller supplies that fact.
Preserve source identity and rerun approval in the report.

## Load only the needed branch

1. Read [`references/capture.md`](references/capture.md).
2. Load the source-kind branch for the named resource:
   video/talk, repository, article/paper/docs, or discussion thread. If a linked
   source becomes explicitly in scope, load only its matching branch too.
3. Capture under a gitignored `target-mine-ID/` path, never `/tmp`. Resume from
   captured files and claim records. Delete the capture directory at close.
4. Load [`references/review.md`](references/review.md) only when the declared
   outcome needs source interpretation, a claim ledger, Jet cross-checks, or
   recommendations. It selects audience, micro, live, and multi-source checks
   by relevance.
5. Load [`references/output.md`](references/output.md) for the retained report
   and the normal Tower-enabled or explicit report-only close.

## Integrity and completion

Every important claim keeps exact source provenance: source ID, source kind,
canonical URL or path, shared `source_identity`, version or commit, retrieval
date, and locator such as timestamp, section, `file:line`, issue, or comment
ID. Linked sources remain distinct from the primary source. When
recommendations are in scope, write and validate `target-mine-ID/claims.json`
with the allowed enums and reviewed topic keys before forming recommendations.

Cross-check actual Jet code and relevant executable behavior, not plans alone.
Separate inference from evidence. Stop when the declared source, subject,
outcome, and relevant evidence lanes are covered; claims have evidence or an
explicit unknown; and the report plus any authorized Tower obligations are
complete. Do not start an unrelated audit or implementation workflow.
