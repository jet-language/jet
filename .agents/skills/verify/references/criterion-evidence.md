# Criterion evidence

Use this reference only when a Tower card names an observable criterion. The
card, not this skill, supplies the command and acceptance boundary.

1. Read the exact criterion, target, expected output/state, applicable tier, and
   owner gate. Confirm the tree is the integrated tree for that card.
2. Run the one exact focused command named by the criterion through
   `scripts/agent/jet-env` where it is a repository command. Exercise the real
   CLI or UI path, not only a helper that resembles it. Do not add a nearby
   suite, duplicate proof, or unfiltered census for reassurance.
3. For a runtime criterion, build a fresh current-source binary first with
   `scripts/agent/jet-env cargo build`, then run the actual path. For a tier or
   generated-artifact criterion, name and run that criterion's producer; a
   source check cannot stand in for it.
4. Capture the complete observable evidence: command, target, exit/status,
   relevant output or artifact identity, and any applicable tier. A command not
   run is `unknown` or `pending`, never `pass`.
5. Give the command and result to the closeout cadence in
   [`../../orchestration/references/closeout.md`](../../orchestration/references/closeout.md)
   for the orchestrator to record and close. A worker receipt does not close a
   card.

Use `scripts/agent/tmp-guard.sh` and the shared resource rules before trusting a
resource-related failure. Keep broad proof and milestone review behind the
commit-bound token described by closeout.
