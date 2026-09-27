# Triage labels

Triage labels answer one question: who must act before delivery work can
proceed? This mapping is the repository contract used by
[the triage skill](../../../.agents/skills/triage/SKILL.md),
[Tower's CLI](../../../plugins/tower/skills/tower/SKILL.md), and
[`issue-tracker.md`](issue-tracker.md). Triage labels are tags; they are not
work phases.

## Canonical roles

Use these five roles and their Tower tags:

| Role | Tower tag | Meaning |
|---|---|---|
| Needs triage | `needs-triage` | A maintainer must evaluate the issue. |
| Needs information | `needs-info` | The reporter must give more information. |
| Ready for an agent | `ready-for-agent` | The card is fully specified for an agent. |
| Ready for a human | `ready-for-human` | A human must implement the work. |
| Won't fix | `wontfix` | The work will not be actioned. |

Use a tag when a skill names one of these triage roles. Category is different:
Tower's `kind` is `bug` for a bug and `feature` for an enhancement. Do not
encode a category as a triage tag. Every triaged card carries exactly one
category (`bug` or `feature`) and one state tag from the table.

## Change a triage tag

Add the first triage tag when an intake item becomes a card:

```sh
tower card update '#N' --add-tag needs-triage --by <agent>
```

When the card has enough information for an agent, replace the old tag in one
write:

```sh
tower card update '#N' --remove-tag needs-triage --add-tag ready-for-agent --by <agent>
```

The same `--add-tag` and `--remove-tag` options handle other transitions. Keep
the card's history in Tower rather than copying it into a process document.

## Keep triage separate from delivery

Tower stores delivery phases as `deciding`, `planning`, `ready`, `building`,
`review` (the phase id is `verify`), `done`, and `frozen`. A triage tag does
not replace a phase, and a phase does not express triage. After a card reaches
`ready-for-agent`, its normal Tower lane and card criteria govern execution.

The maintainer may override an unusual transition, but record the reason on
the card. An issue with a missing answer returns from `needs-info` to
`needs-triage` after the reporter replies. A `wontfix` card is frozen by the
owner; it is not an untracked deletion.
