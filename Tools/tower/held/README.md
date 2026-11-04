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

| Card | Directory | Items |
|---|---|---|
| #5910 | `5910/` | SP05b and SP05c: phase-arena work, with its proof logs (recovered 2026-11-04) |
| #6312 | `6312/` | D-CACHE-CONTROL1=A: the three surviving tests (the compiler half is lost) |
| #6414 | `6414/` | OptAccumulate: an unwired O2 accumulator pass (recovered 2026-11-04) |
| #6442 | `6442/` | BugSweep 15n, 19, 20, 21 and 23: Jetpack and driver fixes |
| #5971 | `5971/` | Main-13: checked fixed-width narrowing (parked: runtime recursion) |
| #6497 | `6497/` | FFIDesign-1: the generated #CLI parser (pulled: Exec effect on help/version) |

## Landed

No held item has landed yet.

## Held work that was lost

The 2026-11-04 scratch cleanup deleted these before they were copied here.
They could not be recovered. Each card keeps its description of the work, so
the work can be redone from that description. The SP05c patch (#5910) and
OptAccumulate.jet (#6414), first listed here, were later found in rescued
copies and are now held above.

| Card | What was held | Where the description is |
|---|---|---|
| #3595 | Fixes for three Jetpack/Foundation test roots: missing core.sys/core.process uses, the TreeHashPublication fs effects, and the ReportJSON Dyn assert (`found.patch`) | #3595 log, 2026-10-23 |
| #6312 | D-CACHE-CONTROL1=A, part-built: `mem.stream_store` (x64 `movnti`, IR StreamStore), `$build.cpu.cache_line`, docs rows, a conformance test and two examples. 31 files on base 67fd7fdc47 (`cache-control1-wip.patch`). The conformance test and the two examples survived and are held in `6312/` | #6312 logs 0 and 1 |

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
