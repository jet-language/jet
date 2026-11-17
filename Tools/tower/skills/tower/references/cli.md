# Tower CLI and board operations

Read this reference only when the requested board operation needs command detail.
The root skill is the router; `Tools/tower/AGENTS.md` remains the policy authority.

## Find the CLI

Prefer the vendored executable so the board stays in this checkout:

```sh
node Tools/tower/tower.mjs help
```

For an installed plugin, use the directory that contains `tower.mjs` and `app/`:

```sh
node ${CLAUDE_PLUGIN_ROOT}/tower.mjs help
```

An alias is optional. Do not assume that an installed copy owns this project's
`.tower` directory.

## Read the board

Use the smallest read that answers the question:

```sh
tower status
tower state --json
tower card show '#12' --json
tower question list --open
tower decision list --json
tower ballots --json
tower brief --no-claim
```

Read an unclaimed packet first and inspect its computed `who`; a next-card
picker can return owner-acceptance `verify` work. Skip owner work before taking
a lease. Only after that check, `tower brief '#N' --agent <me>` takes a renewable 24-hour lease.
`E_CLAIMED` means another agent owns the card. `E_CLOSE_READY` means an
actively claimed card already has every criterion met and no open gate. Close,
reopen, or block that card before any other brief or claim; the barrier also
applies to `--no-claim` reads.

## Write and read back

Every mutation goes through the non-serve CLI. Pass `--by <agent>` and, for a
read-modify-write, `--expect-rev REV`. A revision conflict means re-read the
card and retry from the new revision. Never edit
`Tools/tower/.tower/*.json` by hand.

After a requested write, read the affected card, decision, question, or board
state back. Confirm the phase, criteria, log attribution, revision, and linked
state that the write was meant to change. A successful command without a
read-back is not completion.

With `--json`, every verb prints one JSON document; a refusal is
`{"error":{"code":"E_…","message":"…"}}` with a non-zero exit. A mistyped
command, verb or flag names the nearest real one, and a missing argument or
flag is named. `tower help --json` lists every command's argument, required
flags and flags. `tower card history '#N'` shows everything that happened to a
card, newest first. `tower lint --evidence` warns on met rows whose evidence
cites only scratch paths, wave trees or Rust-era commands.

Workers do not write Tower. In an orchestrated run, they return `CHECK OK` for
code or `DOCS ONLY` for prose/static data. The orchestrator attributes phase,
criteria, proof, and handoff changes on the board.

## State and closure

Cards compute to one lane: `decide` is owner-only; `plan`, `implement`,
`building`, and `verify` are agent work; `blocked`, `frozen`, and `done` are
inert. Epochs are the board's one planning axis: `tower epoch list` shows them
in `order` (then by id number) with their exit criteria, labelled `E18 <name>`.
Set the order and the exit criteria with `tower epoch add|update e18 --order N
--exit-criteria "…" --by <agent>`; repeat `--exit-criteria` for each row, or
pass `{"order": N, "exitCriteria": ["…"]}` through `--file`/`--stdin`. An empty
list clears them. `tower epoch remove e18 --by <agent>` is refused while any
live or archived card, or any milestone, still points at the epoch; the refusal
names the counts. Milestones group targets inside an epoch, worked in `order`,
and derive progress from linked cards. `tower milestone update <id> --epoch E`
moves a milestone with its live cards; `tower milestone retire <id> --reason
"…" --by <agent>` keeps it for the record but takes it out of lists and briefs.

A card needs a non-empty criteria checklist. Each row moves from `open` to
`met` to `verified`. Integrated focused proof supplies `met`; a different
reviewer may supply `verified`. Close a card as soon as every row has concrete
evidence and no contradictory blocker remains. Do not add a per-card review,
duplicate proof, broad suite, or technical `verify` step. Use `verify` only for
an explicit owner acceptance or closeout follow-up.

Change a checklist with `tower card criteria '#N' --add "…"`, `--edit n --text
"…" --reason "…"` or `--remove n --reason "…"` (`tower milestone criteria`
takes the same flags for a milestone's `exitCriteria`, and its meets pass the
same evidence gate). Rewording a met or verified row reopens it, because its
evidence proved the old wording. Removed numbers are never reused, and the event
log keeps the old text and the reason.

`--move n --to '#M' [--reason "…"]` moves an open row to card #M: the text
becomes a new open row there, and the old row is met as kind `duplicate` with
evidence `Moved to #M cK`. Both rows keep the link, shown by `--list`. A row
meets only on durable evidence (a commit on `dev` or `master`, or a `--kind`
with its evidence); a refused meet changes nothing. A refused close lists every
failing row with its reason, one per line.

A card holds at most `criteriaSoftLimit` live rows (config, default 6). A
write that grows it past that is refused (`E_OVER_LIMIT`) unless it carries
`--over-limit "reason"`, which the card's log keeps. Prefer `tower card split
'#N' --criteria 5,6,7 --title "…" --by <agent>`: it moves those open rows onto
a new card in the same epoch, linked as `--move` links them. `tower lint
--limits` lists the cards past the limit with their reasons.

`--proof "jet test Compiler --profile P --filter F"` on `--add` or `--edit n`
sets the row's proof (`P` is `cases`, `ui`, `golden`, `core` or `packages`; any
other `jet test …` command is kept as given); `--proof ""` clears it. A row text
ending in ``Proof: `jet test …`.`` has that sentence moved into the field on load.
`tower prove --card '#N'` or `--epoch E` prints which runs cover which open rows.
With `--yes --jet PATH --tree DIR --commit SHA --by <agent>` it runs them (one run
per corpus profile, with the filters combined into a `--cases` list) and meets
each passing row with evidence `<commit>: <proof command> PASS n/n`. The commit
must be on `dev` or `master`, and a git `--tree` must be a clean checkout of it;
`--dry-run` runs and reports without meeting. `tower lint --proofs --tree DIR`
lists proofs whose filter matches no file under `Tests` or `Examples`.

`--blocked-by '#M'|'#M cK'|D-ID` on `--add` or `--edit n` makes a row wait on a
card, a criterion or a decision; `--unblock n [--reason "…"]` ends the wait by
hand. When the blocker is done, settled or decided, the next load moves the ref
to `unblockedFrom` (`--list` shows `unblocked: REF resolved`). `tower prove`
skips blocked rows.

Epoch `order` is the chain: the first epoch not `done` or `arrived` is active
(three agents), the one after it next (one agent). `tower next` picks the
active epoch's cards first. `tower chain [--json]` lists every epoch with its
role, open cards and open criteria, each with today's change; `tower status`
prints the board-wide `OPEN TODAY` line. `tower closest [--epoch E] [--limit N]
[--json]` ranks open cards by open criteria, fewest first, with blocked rows
counted apart. `--sort KEY[:asc|desc]` on `tower closest` and `tower card list`
orders cards by a board sort key: `workflow`, `workOrder`, `priority`, `open`,
`updated`, `created`, `num`, `title`, `milestone` or `lane`.

Only the owner starts `tower serve`. Agents use the non-serve CLI. Use HTTP
only when the owner has already started the server and the task explicitly
requires it; never start a server or open a browser for the owner.

For dispatch, worker receipts, integration, focused proof, and milestone
cadence, read the canonical orchestration references:

- [dispatch and receipts](../../../../../.agents/skills/orchestration/references/dispatch.md)
- [milestone closeout](../../../../../.agents/skills/orchestration/references/closeout.md)
- [owner policy](../../../AGENTS.md)
