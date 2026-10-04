# Ballot validation

[ELI5-STANDARD.md](ELI5-STANDARD.md) defines the owner's plain-language ballot
standard. `validate.mjs` reads cards through the non-serve Tower CLI, checks
payload gaps and tries add/update against a detached in-memory card snapshot.
It does not save decisions or mutate the board.

```sh
node Tools/agent/ballots/validate.mjs "$HOME/.cache/jet-dev/ballots/READY/D-EXAMPLE.json"
```

Pass explicit payload files for READY reviews. With no arguments, the validator
uses the existing gate queue in `$HOME/.cache/jet-dev/ballots/gates` and its three
named core payloads. `JET_REPO` overrides the repository derived from the tool
location. Exit status is nonzero if any payload has gaps; JSON lists the gaps
and dry-run operation. Review/owner approval remains separate from validation.
Do not move mutable payloads or review receipts into this directory.
