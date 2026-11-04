# Held work

Each directory here holds the unlanded patches that one open card depends on.
The directory is named for the card's number. A held patch lives in the
repository, never only on a scratch disk, so it survives a machine change or a
scratch cleanup (#6563).

Each `<card>/README.md` lists its items. For each item it gives:

- the file;
- the base commit it was cut on;
- whether it applies to the dev commit named there;
- its state;
- the next step.

A patch uses repo-relative `a/` `b/` paths. Apply it with
`git apply -p1 <patch>`, or `git apply --3way` when the base has moved on.

Every held patch is integrated: it lands, it is not parked. An item leaves
this directory only after its landing commit. That commit deletes the item's
files (and the directory with its last item) and adds a row to "Landed"
below. The card then cites the landing commit, not this path.

Owner and status are as of 2026-11-04 (dev f32195ac8c). The owner is the
agent lane integrating the item. Update the row when either changes.

| Card | Directory | Items | Owner | Status |
|---|---|---|---|---|
| #6442 | `6442/` | BugSweep 15n, 19, 20, 21 and 23: Jetpack and driver fixes | Builder3 | All five are combined in Batch7, with the Jetpack compiler slice and the bootstrap check fix (#3590 c59), on a2f1ef4d3c. It reaches a fixed point; its case runs are being finished. Not landed. Batch7 deletes `6442/` |
| #5971 | `5971/` | Main-13: checked fixed-width narrowing (parked: runtime recursion) | Builder3 | Reworked: the runtime's 33 narrowing sites now use `wrapping(T{x})`, which removes the recursion. It reaches a fixed point on a2f1ef4d3c, and its suites are running. Not landed. The 10 Core sites follow in #5971 c34 |
| #6497 | `6497/` | FFIDesign-1: the generated #CLI parser (pulled: Exec effect on help/version) | Builder3, after #5971 c34 | Queued; no rework is on dev. Help/version must not need `Exec`; then rebase the three hand-written files onto dev and regenerate the four tables. A 3-way apply conflicts on 348c3649f1 |

## Landed

A landing commit cannot name itself; `git log --diff-filter=D -- Tools/tower/held/<card>`
finds the commit that landed a directory's items.

| Card | Item | Landed by |
|---|---|---|
| #5910 | SP05b and SP05c (#5910 c17): the recovered phase-arena work ported onto dev as one 13-file patch. Every SP05c hunk is ported, already on dev, or dropped with a reason. The patch removes the HTTP-routing environment copies, passes items rather than module rows to impl registration, adds the casing fast path and the scope-lookup counters, and has a same-run register + sema.check measurement | d248040524 |
| #6312 | D-CACHE-CONTROL1=A (#6312 c5): the three held tests, with the compiler half recovered from a rescued copy of MemDesign's `cache-control1-wip.patch` and integrated: `mem.stream_store` (IR StreamStore, x64 `movnti` plus `sfence`), `$build.cpu.cache_line` (also in `#Layout(c, align(…))`), E1105 text, docs rows, and golden output for both examples | the commit that deleted `6312/` |
| #6414 | OptAccumulate (#6414 c37): the recovered recognizer reworked into the general associative-operation tail-accumulation pass (`Compiler/JetIR/Source/OptAccumulate.jet`, pass `accumulate`, on at O1 and O2): exact Int `+`, `*`, `bit_and`, `bit_or` and `bit_xor`, wrapping word `+` and `*`, word `bit_or` and `bit_xor`, signed-word `bit_and`; either operand order, the other operand computed before or after the call; only a recursion whose head test its step provably reaches; a #Test per shape and refusal, the `math/accumulating_recursion` golden on every tier, and a same-run measurement against `JET_OPT_DISABLE=accumulate` | the commit that deleted `6414/` |

## Held work that was lost

The 2026-11-04 scratch cleanup deleted these before they were copied here.
They could not be recovered. Each card keeps its description of the work, so
the work can be redone from that description. The SP05c patch (#5910),
OptAccumulate.jet (#6414) and the #6312 compiler half, first listed here, were
later found in rescued copies, and all three have landed.

| Card | What was held | Where the description is |
|---|---|---|
| #3595 | Fixes for three Jetpack/Foundation test roots: missing core.sys/core.process uses, the TreeHashPublication fs effects, and the ReportJSON Dyn assert (`found.patch`) | #3595 log, 2026-10-23 |

## Superseded, not held

| Card | Item | Superseded by |
|---|---|---|
| #4599 | Main-6: a top-level fn passed as a parallel callback is not E1102 | e49bd059c4 (`sema_call_concurrency_names_module_function`) |
| #4717 | u7-secrets-cli, u7-hosted-blockers | 58fead1f16 (U7 step 3); 3aa6fe388c (U7 step 4) |
| #6328 | DeclTypes-32, DeclTypes-32b | on dev: both patches reverse-apply to 348c3649f1 |
| #6442 | BugSweep 18 (`exclusive:` header for fixed-port examples) | e6825281c6: the `host-port:` header serializes fixed-port examples |
| #6442 | BugSweep 22 (#6434, inline pair list as a `Dyn.Object` argument) | c81961f6fd |
| #6442 | BugSweep 15n, the ReportJSON.jet half | 6e912387a9 |
| #6442 | ToolsJet SHORTROOTS (short socket roots) | 7c67280960: unix socket paths of 108+ bytes bind through their directory |
