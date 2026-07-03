# Error index

Stable diagnostic codes with examples from `tests/ui/`. Pages are generated —
run `./scripts/gen_errors.sh` after changing snapshots.

| Code | Topic |
|------|-------|
| [E0101](E0101.md) | no `run` function |
| [E0102](E0102.md) | unknown function |
| [E0103](E0103.md) | `print` arity |
| [E0104](E0104.md) | wrong argument count |
| [E0105](E0105.md) | duplicate definition |
| [E0107](E0107.md) | unknown name |
| [E0108](E0108.md) | binding type mismatch |
| [E0109](E0109.md) | operator type mismatch |
| [E0110](E0110.md) | condition not `Bool` |
| [E0111](E0111.md) | assign to an `::` binding |
| [E0119](E0119.md) | unknown type |
| [E0120](E0120.md) | returning a borrowed value |

Full registry: [04-diagnostics](../admin/04-diagnostics.md).
