# Triage Labels

The skills speak in terms of five canonical triage roles. This file maps those roles to the actual label strings used on this repo's Tower board.

| Label in mattpocock/skills | Label in our tracker | Meaning                                  |
| -------------------------- | -------------------- | ---------------------------------------- |
| `needs-triage`             | `needs-triage`       | Maintainer needs to evaluate this issue  |
| `needs-info`               | `needs-info`         | Waiting on reporter for more information |
| `ready-for-agent`          | `ready-for-agent`    | Fully specified, ready for an AFK agent  |
| `ready-for-human`          | `ready-for-human`    | Requires human implementation            |
| `wontfix`                  | `wontfix`            | Will not be actioned                     |

When a skill mentions a role (e.g. "apply the AFK-ready triage label"), use the corresponding **card tag** from this table.

Category roles use Tower `kind`, not tags: `bug` → `bug`, `enhancement` → `feature`.

Apply or remove state roles with:

```sh
tower card update '#N' --add-tag needs-triage --by <me>
tower card update '#N' --remove-tag needs-triage --add-tag ready-for-agent --by <me>
```

Do **not** encode triage state in `phase` — phases are delivery (`planning` → `done`). Triage tags only gate whether work is agent-ready; once tagged `ready-for-agent`, normal Tower lanes take over.
