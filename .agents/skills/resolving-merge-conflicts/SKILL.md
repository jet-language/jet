---
name: resolving-merge-conflicts
description: "Use when you need to resolve an in-progress git merge/rebase conflict."
---

1. **See the current state** of the merge/rebase. Check git history, and the conflicting files.

2. **Find the primary sources** for each conflict. Understand deeply why each change was made, and what the original intent was. Read the commit messages, check the PRs, check original issues/tickets.

3. **Resolve each hunk.** Preserve both intents where possible. Where incompatible, pick the one matching the merge's stated goal and note the trade-off. Do **not** invent new behaviour. Always resolve; never `--abort`.

4. Run only the smallest checks authorized by the current brief and
   `docs/agents/owner-guidance.md`. A worker runs the lane check; the
   orchestrator owns broader tests and formatting. Fix anything the merge
   broke within the owned slice.

5. **Finish the merge/rebase.** Leave staging, commits, and board updates to
   the owning orchestrator unless the current brief explicitly assigns them.
   If rebasing, continue the rebase process until all commits are rebased.
