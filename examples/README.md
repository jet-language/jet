# Examples

`canon.jet` is the compiling syntax showcase. `features/` groups every feature
example by topic (D-REPO-EXAMPLES1); `features/expected/` mirrors the tree with
each example's golden output. Run any example directly:

```
jet run examples/features/basics/hello.jet
```

## Short path first (D-EXAMPLES-SHORTPATH1=A)

Flagship examples teach the **magic default** first. Manual mechanics stay in a
clearly labelled expert sibling (`*_expert.jet`) beside the flagship — never as
the only path.

| Teach first | Keep beside it as expert |
|---|---|
| `#CLI` typed entry args | raw `io.args()` walks |
| `para_map` / `task.group` | hand-rolled channels + `task` + join |
| streaming `files.open(…).lines()` | materializing `String.lines()` for file-scale work |

New examples should land the same way: beginner default in the named flagship,
long path only as an expert variant when the manual form is still worth teaching.

## Genre marker (one rule)

Every example under `features/` is a teaching example first — it explains a
mechanism, not just a fact that a card closed. An example whose header is
nothing but a bare `#NNNN: <what shipped>` note, with no ratified decision ID
and no explanation of the mechanism, is a closure ledger entry wearing a
teaching example's clothes. Mark it: prefix the card number with the word
`ledger` (`// ledger #1477: remaining List surface after #1410/#1479.`). A
reader then knows at a glance this file exists to prove a ledger item closed,
not to teach — citing a card number *alongside* a decision ID or real
explanation (the norm across this corpus) needs no marker; only the bare,
unexplained case does.

## Auxiliary golden stream suffixes (one rule per meaning)

`features/expected/<stem>.out` always holds the plain `jet run` stdout. Every
other suffix under `features/expected/` names a distinct proof, never an ad
hoc pick:

- `<stem>.err.out` — the example is expected to fail (panic, exit 70, or an
  uncaught `Err`, exit 1). Its presence tells `tests/golden.rs` to require a
  non-zero exit.
- `<stem>.stderr.out` — the example is expected to **succeed** (exit 0) but
  still writes incidental stderr (warnings, progress). Optional; add it only
  when a passing example's stderr must stay pinned.
- `<stem>.web.out` — stdout captured running the example under the web/wasm
  target instead of native. Read by `tests/web_build.rs` and
  `tests/web_examples_doc.rs`.
- `<stem>.harness.out` — output from the DOM click/interaction test harness
  for a web example, not plain stdout. Read by `tests/web_build.rs` and
  `tests/web_examples_doc.rs`.
- `<stem>.seed.out` / `<stem>.greet.out` — an example with more than one
  named `#Job` task (D-JPK-TASKRUN1) uses the task name as the suffix,
  keyed per task rather than per stream. Read by `tests/golden.rs` and
  `tests/dev.rs` for `devloop/task_runner`.
- `<stem>.test.out` — the pinned report from running `jet test` on the
  example itself, not the example's own stdout. Read by `tests/jet_test.rs`.
- `<stem>.fuzz.out` — the pinned report from running `jet fuzz` on the
  example. Read by `tests/jet_test.rs`.

Never add a suffix ad hoc: reuse one of the meanings above, or extend this
list in the same commit that adds a new one.

Suggested learning order:

| Topic | What lives there |
|---|---|
| `basics/` | hello, functions, values, branches, loops, closures, pattern matching |
| `types/` | structs, enums, traits, generics, distinct types, typestate, tuples |
| `errors/` | error families, `?` propagation, panic, rollback, discard rules |
| `collections/` | lists, maps, sets, deques, iter adapters, parallel iteration |
| `text/` | strings, regex, unicode, hex/base64, streaming file parse |
| `math/` | numeric floor — libm, checked/saturating/wrapping integer families |
| `modules/` | imports, module files/dirs, packages, visibility, re-export |
| `comptime/` | comptime blocks, splice, reflect, embed, doctests |
| `effects/` | capability sigils, taint, pure, effect prohibition, grants |
| `memory/` | ownership, arenas, stored refs, rawptr, uninit, zero-copy, GC |
| `serde/` | json/csv/toml/yaml, derives, schema migrations, fidelity |
| `io/` | cli, files, stdin, paths, logging, terminal |
| `net/` | http client/server, routes |
| `concurrency/` | tasks, channels, select, race/cancel, deadlines, scheduler |
| `crypto/` | envelope, signing, key migration |
| `ui/` | view tree, styles, component kit, motion, a11y, reactive TUI |
| `web/` | hybrid JS DOM + Wasm compute — see `docs/sidequests/web-backend-wasm.md` for the full example index, build commands, and unsupported-breadth list |
| `lowlevel/` | ffi, c layout, simd, freestanding, MMIO board writes, cross-compile |
| `tooling/` | tests, bench, debug, property tests, build profiles |
