# Triage labels

Skills use five canonical triage roles. These are the Tower card tags for this repository:

| Role | Tower tag | Meaning |
|---|---|---|
| Needs triage | `needs-triage` | A maintainer must evaluate the issue. |
| Needs information | `needs-info` | The reporter must give more information. |
| Ready for an agent | `ready-for-agent` | The card is fully specified for an agent. |
| Ready for a human | `ready-for-human` | A human must implement the work. |
| Won't fix | `wontfix` | The work will not be actioned. |

Use the card tag when a skill names a triage role. Category roles use Tower `kind`, not tags: `bug` maps to `bug`, and `enhancement` maps to `feature`.

```sh
tower card update '#N' --add-tag needs-triage --by <me>
tower card update '#N' --remove-tag needs-triage --add-tag ready-for-agent --by <me>
```

Do not encode triage state in `phase`. Phases describe delivery (`planning` through `done`). Triage tags only decide whether work is ready for an agent; after `ready-for-agent`, normal Tower lanes take over.
