---
name: research
description: Investigate a question against high-trust primary sources and capture the findings as a Markdown file in the repo. Use when the user wants a topic researched, docs or API facts gathered, or reading legwork delegated to a background OMP task.
---

## Contract

- **Requested outcome:** In a standalone run, one cited Markdown result that answers the supplied research question; in a bounded caller handoff, cited findings returned to the caller's owned result.
- **Supplied inputs:** The question, source boundaries, repository conventions, and any existing report location; a bounded caller also supplies its output boundary.
- **Allowed child result:** One bounded OMP evidence-gathering task may return primary-source claims and links. It cannot open an undeclared agenda, change the question, or start implementation.
- **Completion owner:** `research` owns source checking and standalone synthesis; the caller owns synthesis and output when research is a bounded support handoff.
- **Return point:** Evidence returns to research synthesis before a standalone result is saved, or directly to the bounded caller before it completes its owned result.
- **Stopping condition:** A standalone run stops when every reported claim is traced to its source and the single result is saved; a bounded handoff stops when cited findings return to the caller without a required second artifact or follow-on workflow.

For a standalone run, if the host supports child tasks and delegation helps,
submit one **background OMP task** under `AGENTS.md`;
otherwise do the research in the current context. For a bounded caller
handoff, return cited findings to the caller's output boundary instead of
requiring a separate research artifact. The child returns evidence only; keep
the research workflow and the applicable output contract below.

Its job:

1. Investigate the question against **primary sources** — official docs, source code, specs, first-party APIs — not a secondary write-up of them. Follow every claim back to the source that owns it.
2. For a standalone run, write the findings to a single Markdown file, citing
   each claim's source. For a bounded caller handoff, return those cited
   findings to the caller instead of creating a required second file.
3. For a standalone run, save the file where the repo already keeps such notes;
   match the existing convention, and if there is none, put it somewhere
   sensible and say where.
