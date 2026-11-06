# Examples audit: shrink and PascalCase layout (2026-11-06)

This audit is step 1 of the owner-approved plan of 2026-11-06:
1. this audit;
2. the owner reviews the keep/merge/move/delete list;
3. one cutover, now unblocked: #6586 (`name: value`) landed on dev as 5ef77a52ad.

It is read-only. No repository file, Tower card or suite was touched.

Inputs:
- worktree `Main-dev-collectall` at c413fee823, checked against dev at 5ef77a52ad (see below);
- golden costs from vb4 at 152c15aaf6 (`candidate-golden-0.json`), measured before #6586;
- board snapshot `obe-cl2.json`.

The plan holds on dev at 5ef77a52ad. Between c413fee823 and 5ef77a52ad:
- #6586 changed 404 files under `Examples/` and added, deleted or renamed none, so every entry and path in `plan.tsv` is unchanged;
- `tests/run` has no added, deleted or renamed files;
- 80 test files were deleted, and none of them is a covering test named in `plan.tsv`.

The change is spelling only, so overlap judged by meaning still stands. The CPU figures predate #6586; a new golden census on dev would refresh them.

Companion files (working files, not committed):
- `plan.tsv`: one row per entry.
- `coverage.tsv`: each feature, the example that keeps it, and its tiers.
- `consumers.tsv`: every file or card that cites a path, and the path that replaces it.
- `owner-review.md`: the list to review, grouped by folder.

## Summary

| | now | after |
|---|---:|---:|
| Example entries | 1340 | 789 kept (−41%) |
| Examples golden CPU | 4.78 h | 2.61 h to 2.90 h |
| Whole golden suite (Examples plus `tests/run`) | 5.06 h, 1602 entries | 4.32 h to 4.61 h |
| `tests/run` golden CPU | 0.28 h, 321 entries | 1.71 h, 697 entries |

Where the 1340 entries go:

| action | entries | CPU today |
|---|---:|---:|
| keep | 736 | 2.61 h together with rename-only |
| rename-only (stays, changes folder) | 53 | |
| merge into a kept example | 132 (into 103 keepers) | 0.29 h |
| move to `tests/run` | 376 | 1.43 h |
| delete (an existing test already runs a copy) | 43 | 0.45 h |

The merge range is the gap between two bounds. If a merge adds nothing to its keeper's run time, the merged cost is all saved. If it keeps all of its cost, the upper figure applies.

Moved entries keep their cost; they only change suites. So the whole golden suite saves only the deletes and the merges.

Timeouts dominate what remains:
- **Kept:** 51.3 of the kept 156.5 minutes are TIMEOUT rows. They come from five examples:
  - `concurrency/job_queue`, 744 s;
  - `lowlevel/ffi`, 661 s;
  - `tooling/service_authority`, 622 s;
  - `tooling/service_partition_reconcile`, 622 s;
  - `packages/build_contribution`, 431 s.

  Fixing those five would bring kept Examples CPU to about 1.75 h.
- **Moved:** another 33.9 minutes of timeouts move to `tests/run`:
  - `crypto/file_seal_jetc`;
  - `foundations/storage`;
  - `game/raylib_indexed`;
  - `web/battery`.

Per current folder (folders under 1 CPU-minute are pooled):

| folder | entries now → kept | CPU min now → kept |
|---|---:|---:|
| tooling | 86 → 64 | 39.4 → 26.6 |
| serde | 76 → 46 | 35.9 → 25.4 |
| io | 96 → 57 | 35.6 → 15.4 |
| foundations | 13 → 0 | 22.3 → 0.0 |
| concurrency | 50 → 31 | 19.6 → 17.3 |
| crypto | 19 → 13 | 16.7 → 3.1 |
| net | 47 → 32 | 15.0 → 10.9 |
| lowlevel | 49 → 31 | 14.4 → 13.6 |
| collections | 84 → 35 | 13.7 → 4.3 |
| game | 11 → 7 | 12.1 → 1.2 |
| packages | 15 → 13 | 8.0 → 7.7 |
| basics | 84 → 38 | 5.9 → 3.6 |
| effects | 37 → 25 | 5.4 → 2.9 |
| modules | 31 → 20 | 4.9 → 2.1 |
| text | 52 → 34 | 4.3 → 3.1 |
| errors | 60 → 28 | 4.0 → 1.5 |
| cli | 6 → 5 | 3.7 → 1.9 |
| web | 27 → 19 | 3.6 → 2.4 |
| ui | 20 → 18 | 2.8 → 1.4 |
| parsing | 9 → 5 | 2.5 → 2.0 |
| types | 90 → 50 | 2.4 → 1.7 |
| dyn | 9 → 8 | 2.3 → 2.1 |
| memory | 89 → 51 | 2.3 → 1.2 |
| patterns | 20 → 10 | 1.8 → 1.0 |
| time | 12 → 6 | 1.7 → 0.5 |
| math | 42 → 18 | 1.3 → 0.5 |
| comptime | 47 → 25 | 1.2 → 0.8 |
| traits | 10 → 3 | 1.2 → 0.3 |
| 26 smaller folders, and the trees outside `features/` | 149 → 97 | 3.2 → 1.9 |

The trees outside `features/` (FFI, interop, embedded, Jetpack, performance, suites and continuity) have no golden rows today. They are kept whole and only respelled.

Coverage:
- `coverage.tsv` lists 702 features.
- None is lost.
- 673 keep an example:
  - 593 directly;
  - 42 through a merge keeper;
  - 38 whose example is not PASS on o0/o1/o2 today. 28 of these are real I5 gaps; the other 10 run once through a tierless `.case` sidecar.
- 29 are proved only by the test they move to. These are:
  - the Foundations batteries' rulings (five D-FOUND-*, D-FILES-SCOPE1, D-DIMENSION-OPEN1, D-STDOUT-BUFFER1);
  - the ICE and failure-case diagnostics;
  - the runtime-stop codes E3011/E3012;
  - five witness rulings (D-ASYNCRT1, D-ATTR4, D-BRANCH-CODEGEN1, D-MATH-CORE-CALLS1, D-TEXT-CORE-CALLS1);
  - `core.build`, `core.compare`, `core.rt` and `core.units`.

## Method

### Inventory

An entry is anything the golden harness runs, or would run, as one example:
- a `.jet` file;
- a `<dir>/run.jet` project;
- a package directory.

Every `.jet` under `Examples/features`, and every entry tree outside it, was listed from `git ls-files`.

Each entry was joined to:
- its expected streams (`expected/<stem>.*`);
- its `.case` sidecar;
- its golden rows: tier, status, `wall_ms`, `peak_kib`.

The result is 1340 entries, 1281 of them with golden rows.

### Features

Features are read from each source:
- decision IDs (`D-…`);
- `use core.x` and `core.x` references;
- `#Marker` uses checked against `Markers.jet`, including grouped `#[A(…), B]` forms;
- diagnostic codes in expected error streams;
- the topic.

Features are matched by meaning, because #6586 respells nearly every line.

### How overlap was judged

Overlap was judged in four passes.

1. **Read.** I read a digest of every entry: its header comment, its first lines and its size. Each entry was classed as one of:
   - a teaching example;
   - an expert second rung;
   - a regression or card witness;
   - an integration battery.

   The defaults were:
   - `ice/` → move;
   - card or ledger headers (`#NNNN`, `Card #`, `ledger`) → move;
   - card-numbered file names → move;
   - everything else → keep.

   685 hand decisions in `decisions.txt` override these defaults.
2. **Overlap between examples.** Pairs were compared on normalized token shingles. Two examples count as overlapping when they teach the same rule at the same rung of the "Lead with the safe path" ladder (`Docs/spec/contributing/examples.md`). High textual similarity alone was not enough.

   The smaller lesson merges into the clearer one. A second example stays only when it is the expert rung of a ladder the guide names, for example `first_hour` and `first_hour_expert`.
3. **Overlap with tests.** Each example was compared with every program under `tests/cases` fixtures and `tests/run`, by containment: the share of the example's shingles found in the test program.

   A regression-class example is deleted only when both hold:
   - containment is at least 0.9, or at least 0.75 when the test copies it at the same `Examples` path;
   - the test asserts the output itself.

   `tests/fuzz` copies never count, because they prove no output. Teaching examples that also have a fixture copy stay, and the duplicate run is noted below.
4. **Coverage check.** The plan is re-derived from the rules and the overrides, then checked:
   - every feature an entry covers today maps to a kept example or to the test that receives it;
   - every merge target is itself kept;
   - every delete names its covering test.

   An unsure overlap stays as keep or merge and is marked `review`. There are 192 such rows:
   - 104 keep;
   - 68 merge;
   - 10 move;
   - 5 rename-only;
   - 5 delete.

   The 13 examples that `Examples/README.md` cites in its learning order stay regardless of overlap, for example `first_hour`, `loop_forms`, `errors`, `scope_guard` and `view_return_lifetimes`.

### Cost

Cost is `wall_ms` summed over all tiers from the vb4 candidate run. Statuses that are not PASS are reported as they are. A TIMEOUT row costs its full wait.

## Proposed layout

The layout follows D-PATH-CASE2=A (#4613):
- every path segment is PascalCase;
- acronyms are taken from `Compiler/JetFoundation/Data/Syntax/Acronyms.jet` (`IO`, `JSON`, `CLI`, `UI`, `HTTP`, `FFI`, `UTF8`, …);
- adjacent acronyms are joined with `_` (`UI_TUIReactive`, `URL_MIME`);
- `run.jet` and `package.jet` stay lowercase.

```
Examples/
  README.md
  Features/                          golden corpus (was features/)
    <Topic>/<Name>.jet               e.g. Basics/FirstHour.jet, IO/ProcessRunAll.jet
    <Topic>/<Name>/run.jet           projects, e.g. Tooling/ProgrammableBuild/run.jet
    Expected/<Topic>/<Name>.<suffix> e.g. Expected/Basics/FirstHour.out
  FFI/ Interop/ Embedded/ Jetpack/ Performance/ Suites/ Continuity/
tests/run/<TestTopic>/<Name>.jet     moved programs; .out/.err.out/.case beside them
tests/run/<TestTopic>/<Name>/run.jet moved projects; <Name>.out beside the folder
```

I propose keeping `Features/` rather than flattening it. It keeps the golden discovery root (`Examples/Features` plus `Expected/`) separate from the trees that run under other harnesses (FFI, interop, embedded), so discovery stays one directory walk.

There are 28 topics:

| | | | | | | |
|---|---|---|---|---|---|---|
| Basics | CLI | Collections | Comptime | Concurrency | Crypto | Devloop |
| Dyn | Effects | Errors | Functions | Game | IO | Lowlevel |
| Math | Memory | Modules | Net | Packages | Patterns | Serde |
| Text | Time | Tooling | Traits | Types | UI | Web |

16 small folders fold away:

| old folder | new topic | exceptions |
|---|---|---|
| callable, contracts | Functions | |
| devtools | Tooling | |
| generics | Types | |
| loops | Basics | `read_sentinel` → IO |
| script_mode | Basics | |
| syntax | Basics | `byte_literals` → Text, `maturity_tags` → Tooling, `optional_method_chain` → Types, `trailing_block` → Functions |
| loose_files | Modules | |
| operators | Math | `chained_comparison` → Basics, `user_defined` → Traits |
| parsing | Serde | |
| ports | Lowlevel | |
| reflection | Comptime | |
| safety, strings | Text | |
| script_job | Devloop | |
| streams | Concurrency | |

Two more groups move without a folder of their own:
- From `text`: date and duration files go to Time; `decimal` and `int_exact` go to Math.
- `foundations/` (13 integration batteries) and `ice/` (14) leave Examples for `tests/run/Foundations` and `tests/run/IceRegressions`.

`tests/run` topics reuse today's folders, respelled (`control_and_data` → `ControlAndData`, and so on). The new ones are:

| | | | | |
|---|---|---|---|---|
| IceRegressions | CoreIO | CoreDB | Encoding | Concurrency |
| Tooling | Services | Net | UI | WebBuild |
| Game | Foundations | TraitsAndImpls | CLI | Contracts |
| VFailureCases | Dyn | Devloop | Packages | |

## Cutover shape (for step 3)

The cutover uses only Jet programs, `.out`/`.err.out` goldens and `.case` sidecars. It adds no node script, shell runner, Python helper or other non-Jet runner or fixture. Moved entries keep the golden form `tests/run` already supports: a source with its streams beside it.

Two existing non-Jet files inside `Examples/` are handled as follows:
- `foundations/tooling/rename.mjs` is a peer program that the battery's `run.jet` never calls, so it is dropped, not carried into `tests/`. Its maintained copy lives in `Tools/agent-eval`.
- The foreign-language peers under `FFI/`, `Interop/` and the polyglot examples stay where they are and are only respelled.

The 16 `.sh`/`.mjs`/`.py` files there are already listed in `Tools/script-census.tsv`.

Order of work:

1. Rewrite each merge keeper so it carries the merged lesson, and regenerate its golden on o0/o1/o2 (and web where the merged entry had it).
2. For the seven deletes whose covering test reads the example path live (`{root}/Examples/…` or `../../../Examples/…`), first copy the source and `.out` into that test's fixture. These are:
   - `impl_shape_matrix`, read by `cli_compiler_speed/parallel_check`;
   - `find_empty`;
   - `host_rights`;
   - `refutable_variant_owner`;
   - `resource_close_containers`;
   - `service_runtime`;
   - `shared_compound_places`.
3. Move each entry and its streams with `git mv` to the target in `plan.tsv`. `Expected/<Topic>/<Name>.*` goes beside a moved source.
4. Rename everything else to the new spelling.
5. Update every consumer in `consumers.tsv` (see below). Code consumers that change:
   - `Compiler/JetTestKit/Source/Golden.jet`:
     - the `"expected"` folder name;
     - the `Examples/features/expected/` prefix;
     - the hard-coded exit-1 stem list. It names five stems that move to `tests/run`: `propagation_journey_direct`, `_one_hop`, `_core`, `or_err_journey` and `log_fatal`. The exit oracle must follow them, or it silently returns `None` there.
   - the golden corpora list (`Examples/features` → `Examples/Features`);
   - `Compiler/JetCli/Source/Cli/DevtoolsExample.jet` scaffolds;
   - `Compiler/JetLSP` and `Compiler/JetSemIndex` fixtures;
   - `Tools/agent/run-feature-examples.mjs` (its constants only);
   - `Tools/testkit/known-failing.tsv` and `user-rust-ffi.allow`;
   - `tests/corpus_policy.tsv`;
   - `tests/guards`.
6. Update `Examples/README.md` (the learning order) and `Docs/spec/contributing/examples.md` paths.

### Consumers

`consumers.tsv` has 2186 rows over 687 files and 59 Tower criterion citations.

- **Dated records.** 125 files are dated records under `Docs/audits`, `Docs/proposals` and `Docs/research`. I propose leaving them unchanged, as history.
- **Live files.** 158 rows in 96 live files cite a path that is merged, moved or deleted. These need a real edit, not only a respelling. They sit in:
  - `tests/cases` (55);
  - `Docs/spec` (37);
  - `Examples` cross-references (36);
  - small counts in `Tools`, `Compiler` and `tests/guards`.
- **Case files.** 268 rows are `.case` files that read an example path live. 93 more mention one only in a comment, or as a fixture-relative path, which is test data and needs no change.
- **Tower.** 52 open criteria on 39 non-done cards cite 55 paths.
  - 31 of those paths do not exist today; they are planned examples. `consumers.tsv` gives their new spelling.
  - 10 citations on nine cards point at a merged or moved path: #2279 (two), #3173, #3262, #3661, #4599 (two), #4629, #4631 and #6566.

  The brief expected 43 criteria on 33 cards. My count includes every criterion not marked `met` on every card not `done`, including frozen cards.

## Risks

- **#6586 landed.** Its spelling now governs. Each merge keeper is rewritten in the new spelling, and its golden is regenerated during the cutover.
- **Some deletes rest on narrower tests.** `tests/run` golden runs the same o0/o1/o2 tiers as Examples, so moves keep their tiers. A few deletes rest on tests with fewer tiers:
  - `dyn_exact_int` and the `web_wasm_*` witnesses are proved on web only;
  - `resource_close_containers` is proved on AOT and the interpreter;
  - `traits/display_read_receiver` is proved on native tiers.

  `plan.tsv` names the covering test, and the reviewer should accept that narrowing.
- **Fixture drift.** #6586 converted the `tests/cases` fixture copies along with the examples. Even so, containment was measured on the old spelling, and the 0.75 to 0.9 rows (runtime stops, `todo_stop`, `stack_overflow`) are worth a glance during review.
- **Duplicate cost.** Teaching examples that also have a fixture copy (for example `list_bounds` and `panic` in `runtime_stop_goldens`) are kept, so they run twice. Dropping the fixture copy is a tests change, outside this plan.
- **Timeouts and failures.**
  - Five kept examples time out, four of them on every tier.
  - Two service examples ERROR: `service_projection` and `service_workflow_outcome`.
  - Several are KNOWN-FAILING: `compute_autodiff`, `polyglot_go` and `opaque_handles`.

  They stay because each is the only example of its ruling. They still need repair to satisfy I5.
- **No golden today (I5 gaps).** None of these has a golden row:
  - the trees outside `features/`: FFI 18, interop 10, suites 8, embedded 6, Jetpack 2 and performance 2;
  - packages: `library_loadable`, `monorepo` and `outputs_build`.
- **Tracked run caches.** 106 `.jet/last-run/*` files under Examples are tracked even though `.gitignore` lists `**/.jet/last-run/`. They came in with the WIP rescue commit 4ab6a903d1. The cutover should drop them rather than respell them.
- **`fixture-state/` trees** inside some examples (locks, generated bindings, cache snapshots) move with their entry. Their inner names are tool-generated, and respelling them may break the tools that write them.
- **Similar projections.** The seven `ffi/projections` hosts are 0.93 similar to each other. They are kept and marked for review, because each is a different host language.

## Owner decisions

1. **`tests/run` casing.** Its folders are lowercase today (`control_and_data`). This plan respells them in PascalCase under #4613's scope for the insides of `tests/`. The alternative is to keep lowercase topic folders in `tests/run` and PascalCase only the moved names.
2. **Topic names.** `Devloop` and `Lowlevel` are the mechanical word-key spellings of `devloop` and `lowlevel`. `DevLoop` and `LowLevel` read better but need a two-word judgment. The same question applies to `Fizzbuzz`, `Datetime` and `Javascript`.
3. **Folder folds.** The 16 folds listed above. In particular:
   - `operators` → Math;
   - `parsing` → Serde;
   - `reflection` → Comptime;
   - text's date files → Time.
4. **Foundations batteries.** Moving them to `tests/run/Foundations` leaves eight rulings with a test but no example: five D-FOUND-* rulings, D-FILES-SCOPE1, D-DIMENSION-OPEN1 and D-STDOUT-BUFFER1. The alternative is to keep one short example per ruling.
5. **Single-example rulings that are broken today.** These are kept, marked ⚑, as the only example of their ruling:
   - `job_queue` (TIMEOUT);
   - `service_authority` and `service_partition_reconcile` (TIMEOUT);
   - `service_projection` and `service_workflow_outcome` (ERROR);

   Each needs either a repair card or acceptance as a test-only ruling. `files_store_primitives` (266 s, eight D-CORE-FILE* rulings) and `arrow_ownership` are also kept as single examples of their rulings, though a case runs a copy of the first.
6. **The 192 review rows** in `owner-review.md`, marked ⚑.
7. **Foreign-language peer files** under `FFI/`, `Interop/` and the polyglot examples. Should they take PascalCase names too, or keep the names their own toolchains expect (`go.mod`, `host.py`, `build.sh`)? This plan respells directories and Jet files only.

## Outcome

The owner approved the recommendations and the concept-area top level. The cutover applied `plan.tsv`, including the flagged rows. Results:
- 43 deletes;
- 132 merges into 103 keepers;
- 376 moves to `tests/run`;
- every other entry renamed into the area tree.

`Examples/` now has 18 areas, in learning order. Each area has a navigation-only `README.md`:

| # | Area | Sub-folders |
|---|---|---|
| 1 | Basics | Functions |
| 2 | Types | Optionals, Traits, Patterns, Units |
| 3 | Collections | Iterators |
| 4 | Text | Unicode, CheckedText |
| 5 | Math | Compute |
| 6 | Memory | Views, Shared, Allocation, Unsafe |
| 7 | ErrorsAndEffects | Errors, Effects, Contracts |
| 8 | Metaprogramming | Comptime, Reflection, Generics, Derives, Attributes, BuildTime |
| 9 | Modules | Packages, Jetpack |
| 10 | Concurrency | Parallel, Services |
| 11 | Systems | Files, Process, Terminal, Time, CLI |
| 12 | LowLevel | Embedded |
| 13 | Data | JSON, Formats, Binary, Dyn, Database, Analysis |
| 14 | Network | HTTP, Browser |
| 15 | Crypto | none |
| 16 | Interop | FFI, Polyglot, Languages |
| 17 | Apps | UI, Web, Game, Programs |
| 18 | Tooling | Testing, Debugging, DevLoop, Build, Performance |

Layout rules:
- Expected streams sit beside their sources. The `Expected/` tree and the `features/` level are gone.
- Golden discovery walks all of `Examples/` and `tests/run` with no prefix logic.
- Entries that need a host tool carry a `.case` with `requires: tool:<name>`.
- The Foundations batteries moved to `tests/run/Foundations`.
- Two new examples were added: `ErrorsAndEffects/Effects/FileScope.jet` and `Systems/Terminal/StdoutFlush.jet`.
- The 106 tracked `.jet/last-run/` run caches in the old example folders were dropped with those folders.
- The unsafe-region baseline in `Docs/spec/safety.md` follows the moved files.

Entries that can't pass yet have rows in `Tools/testkit/known-failing.tsv` that give the reason:
- `Interop/Languages/*`;
- `Interop/FFI/Callbacks`;
- `LowLevel/Embedded/{AllocatorFirmware,Battery}`;
- `Modules/Jetpack/Epoch5`;
- `ErrorsAndEffects/Effects/FileScope`;
- `tests/run/Foundations/{Capabilities,Exact,Units}`.

## Open items

Scope was cut on 2026-11-06 to meet the deadline. Still to do:
- an o1/o2 sweep across all 103 merge keepers (each passed `jet run` against its stream; Migrations passed all tiers);
- regenerate StructurePlane/StructureLiveness `.structure.out` if the rebless did not cover them;
- fix stale lowercase prose names (`metaprogramming.md:93,96`, `OSProcessControl.jet:2`);
- check whether known-failing row 419 (web_examples_doc, #4599 state_surface) is stale.
