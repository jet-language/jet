# D-EMBED-WALK1: independent adversarial review (2026-11-10)

Author: BallotEmbedWalk, Claude Opus. Independent reviewer: EmbedWalkReview, GPT.
This replaces the author's self-review, not the ratified toolchain decision.

**Recommendation: A, conditionally.** It gives the existing operations one computed-argument rule.
**E is equally sufficient for c28 and is the smaller change.** Neither is implemented by the seed.
The ballot may ask for a direction; it must not claim a passing implementation, proved speedup,
cross-target parity, or completed sandbox/cache work.

## Authority and scope

Read `AGENTS.md`, including Owner gates, Adversarial design review and I1/I3/I8/I9.
Read #5072 and these decisions through the read-only Tower CLI:
D-EMBED-TOOLCHAIN1, D-CTIO1, D-CTFIND1, D-META-SPELL1 and D-PRELUDE-LAW1.
Their outcomes are A. D-CTIO1 requires literal paths; this ballot explicitly amends that requirement.
D-META-SPELL1 ratifies `#run`; its presence in examples does not mean the seed accepts it.
D-EMBED-TOOLCHAIN1=A remains in force. Its posted text does **not** ban a bulk read operation;
the old draft's claim that it ruled out B was not supported by that decision.

#5072 c28 names CoreSourceOwned, EnumRows, StructureRows, the EnumPayload tail and CoreCallRows.
It also requires one shared declaration parser and deletion of the generator copy.
The generator produces twelve views; the full-generator probe is not precisely the five-table workload.
No compiler, repository source, real board, or git state was written by this review.
Only the named draft directory and its probes were changed.

Source citations below refer to **origin/dev 53805444ba**, not the master worktree.
Executable observations use the **a0.0.43 admitted seed** (compiler source c460ba91e5).
A source claim and a seed observation are separate evidence; neither certifies a new compiler build.
Probe sources and receipts are under the ballot's `probes/` directory.

## What was independently exercised

The reviewer reran w01-w04, w06-w08, w05_glob_add.sh, census.sh,
cost/trace.sh, cost/sum.sh, cost/mk.sh, all c0-c4 programs, and three interleaved cost rounds.
Additional cases live in `review.sh`, `lifecycle.sh` and `empty_glob.sh`.

| Probe | Observed result |
|---|---|
| w01, w03 | E0957 for a path returned by find and for a named constant |
| w02 | `in/a.txt in/b.txt in/deep/c.txt`, sorted |
| w04 | Regex, map, named tuples and SHA-256 execute inside comptime; SHA-256(abc) is `ba7816bf…15ad` |
| w05 | Separate run invocations see 3, then 4, then 3 paths after add/remove |
| w06 | Computed b.embed builds; lock records generated asset.jet, not the original asset |
| w07 | E0957 quotes `../assets/message.txt` and refuses escape |
| w08 | Literal embed plus find builds; no .jet/lock exists |
| review: bytes / utf8 | A one-byte invalid-UTF-8 file embeds as bytes (length 1); text is E0955 |
| review: empty | A glob with no matches returns an empty list |
| review: symlink / symlink_dir | Final and intermediate symlink reads fail E0955 |
| review: parent / absolute | Literal parent and absolute paths fail E0957 |
| review: computed_bytes / computed_find | Both fail E0957 |
| review: toolchain / hash_run | Seed rejects the proposed prefix with E0955 and #run with E0733 |
| review: import_parent | A script importing above its authorized source root fails E0602 |
| review: build_computed_glob | A named String passed to b.find builds; b.embed of its result reads the expected text and selects the app output |
| lifecycle: native levels | Literal find/embed prints the same `count=1 text=alpha` under none/basic/full |
| lifecycle: build | Executables print counts 1,2,1 across add/remove, and changed text after an edit |
| lifecycle: dev | After watch readiness, neither the added .txt nor edited embedded file was observed during its 2-second window on this seed |
| empty_glob | A find-only program prints 0/0/0 under run after first addition/removal (stale); build prints the correct 0/1/0 |
| c0 | 284 inputs, 19,440,455 bytes, 9 existence-only paths |
| c1 | CoreCallRows is 895,745 bytes; SHA-256 `bb887113efd0972d18c274d3291356f3e9bc7a5d0122678a1670ed6c16041216`, match=true |
| c2, c3 | All twelve output digest pairs match, at compile time and at runtime |

The first reviewer build-glob fixture used `require`, which the build evaluator rejected with E0102.
It was repaired to select an executable name from the actual read content. Only the repaired fixture's
successful build supports the computed b.find claim. w06 alone did not use the returned text.

The import result proves a root restriction, **not** that every Jet script is forbidden to use every package.
The new embed rule alone does not solve parser sharing. c28 still needs a dependency-safe parser package
and a packaged tool/import arrangement; it does not authorize weakening E0602 or copying the parser again.

## Code facts and corrections

- `Compiler/JetSema/Source/Sema/Calls/ComptimeInputs.jet:22-28,51-67` accepts a literal,
  validates the spelling, and constructs `THostCall.ComptimeInput` with a stored String.
  `Compiler/JetSema/Source/Sema/Tree.jet:428-431` likewise stores `path: String`.
  `Compiler/JetIR/Source/BuildExpr.jet:1768-1778` emits **ConstText** for that stored path.
  The host adapter takes String values, but the checked expression representation does not yet
  carry a dynamic argument. A is not just deletion of one check.
- `Compiler/JetDriver/Source/Driver/BuildAdapter.jet:189-192,344-348` extracts a Text value
  from evaluated arguments. The error text at 346 says literal, but tests a value's type, not its AST.
  Both b.find and b.embed already take computed String values. The contrary draft claim fails.
- `BuildAdapter.jet:379-383` reads each find match to check readability. The comptime wrapper
  `Compiler/JetDriver/Source/Driver/ComptimeInputs.jet:123-127` reads it again to hash and record it.
  A later embed reads it again at 75-79: **three reads**, not two, before any proposed reuse.
  The current record at 27-35 holds path, file and SHA-256, **not bytes or glob membership**.
  A bulk operation can share these reads, but that is an implementation opportunity, not a measured win.
- `Compiler/JetCli/Source/Cli/Request.jet:1237-1262` rejects absolute/parent/NUL paths, opens
  intermediate directories and reads through held-directory authority with change checks.
  The final and intermediate symlink probes exercise the path. This is not a proof of every host,
  race schedule, or toolchain-prefix implementation.
- Not every read uses that adapter: `ComptimeInputs.jet:83-94` gives Core its own reader,
  refuses find in Core and omits ordinary program input logging there. Do not claim every existing
  read is rooted and logged by the ordinary path. Preserve this authority boundary unless separately amended.
- `Compiler/JetCli/Source/Cli/ScriptCache.jet:511-530` suppresses kept-script reuse only when
  the file input log is nonempty. An empty find has no file records and escapes that condition.
  `BuildCache.jet:29-36,141-175` uses a completed compile's file digests for the stored-binary key.
  These are different caches. Neither establishes a persistent table-value memo or universal package recomputation.
- `Compiler/JetCli/Source/Cli/Host.jet:3718-3759` watches package .jet files, explicit watch paths,
  and already-recorded input files. It has no glob-membership record. A new file not otherwise
  watched is absent from those lists. Existing explicitly watched toolchain directories are a limit
  on the old blanket claim that all toolchain additions are missed.
  The seed lifecycle test missed both addition and editing during the observed windows; origin/dev's
  recorded-file watcher is newer source behavior, not a seed capability certified by that test.
- `Jetpack/Bootstrap/Assemble.jet:685-720` scans literal embed_file text and copies assets under
  a new .embed suffix. It does not parse dynamic paths, embed_bytes or find.
  `Jetpack/Bootstrap/Check.jet:199-216` passes JET_TOOLCHAIN_ROOT, but that does **not** prove an
  assembled package satisfies c26's membership/location gate. That authorization needs a real proof.

## Read set and cost

The rerun census at 53805444ba found 42 runtime units (7,110,204 bytes), 13 syntax files
(199,547 bytes), 116 Core.jet source_module paths (4,424,072 bytes), and 267 Core .jet files
(18,411,878 bytes). The largest runtime unit is UnicodeNameIndex.jet (5,950,768 bytes).
The five full files total 1,214,454 bytes; c28 replaces only the generated EnumPayload tail, not
necessarily its whole 8,620-byte file. The other four sizes are 114,472, 54,160, 141,457 and 895,745 bytes.

The trace's 293 repository files include the tool and env.jet, and seven comparison-only outputs.
Removing those nine yields 284 input files. It is a full generator trace, not a precise c28 dependency set.
`CoreSourceRows.jet:1600-1602,2558-2562,2755-2756` reads names taken from declarations and embedded
asset names. A trace of opens alone does not establish the old exact attribution of **217** paths to data.
That unproved count was removed from the ballot. Data-derived paths are real; a content glob can still
cover them through a literal superset. Computed scalar reads are a convenience and tighter read set,
not the only possible solution.

`cost/mk.sh` substitutes a TREE map and literal embeds for the generator's filesystem calls.
Its c1/c2 digest equality establishes correctness of this **adaptation on this input set**.
It does not establish dynamic-read integration, package sharing, bootstrap success, or absence of cycles.
The inputs include docs and in-place rewrite files not needed by every table. The TREE representation
also affects allocation, reachability and compile work. Use these figures as scale evidence, not option pricing.

Independent three-round run, same seed, shared machine; all exits zero. CPU is user plus system;
RSS is the largest observed peak, in MiB as the script prints it. No quiet-machine speed ranking is claimed.

| Program | CPU median | Wall median (min) | Peak RSS |
|---|---:|---:|---:|
| c4 empty TREE, salted compile | 3.84 s | 3.18 s (3.08) | 419 MiB |
| c0 embed inputs | 7.39 s | 11.35 s (6.30) | 483 MiB |
| c1 CoreCallRows at comptime | 9.71 s | 16.14 s (7.42) | 522 MiB |
| c2 twelve views at comptime | 10.00 s | 11.85 s (10.55) | 523 MiB |
| c3 twelve views at runtime | 8.84 s | 10.12 s (7.58) | 510 MiB |
| generator --check, salted cold | 5.39 s | 5.26 s (3.90) | 429 MiB |
| generator --check, warm | 1.00 s | 1.00 s (0.99) | 51 MiB |

Differences of noisy medians are not direct isolated measurements. Here c1 minus c0 is 2.32 CPU seconds,
and c2 minus c0 is 2.61; c3 minus c0 is 1.45. The earlier 2.5/3.3/1.0 figures and 3.5x ratio are not stable
prices. The old 55 MB CoreCallRows delta is 39 MiB in this run. No option has been benchmarked end-to-end.
B's superset work, C's bound, D's policy check and A/E's dynamic lowering need not have identical costs.
The generated baseline can amortize derivation between edits; this cold experiment does not model package reuse.

## Same program, paths, and strongest alternatives

The ballot gives A-E sketches of **the same CoreCallRows derivation**, using ratified #run syntax.
Helpers in those sketches stand for the existing parser/renderer work; they are not claimed to be APIs.
The complete runnable adapted derivation is c1; all-twelve correctness is c2. A-E cannot be executed on
this seed, and no proposed spelling is marked as working today.

| Option | Beginner path | Expert path / strongest case | Cost and limit |
|---|---|---|---|
| A | Existing embed literal, then variable path | Derive both paths and globs from recorded data | Uniform arguments; no source-only ceiling beyond existing file authority |
| B | Scalar embed, then content glob | A source-visible bound and bulk content set | Two names; extra files may be read/decoded; byte result must carry bytes, not text |
| C | Existing literal embed | First execute a literal find that authorizes later reads | A real bound, but ambient authority state and order dependence; use full file identity |
| D | Literal embeds need no field | Manifest declares a stable upper bound on computed reads | Useful separate authority ceiling, not redundant actual-input data; another public policy surface |
| E (added) | Exactly A for embeds | Compute embeds; retain static globs and filter when needed | Meets c28 with less change; data-derived walks need a superset |

E is the missing smaller alternative. The displayed c28 program contains no computed glob, so A has
no demonstrated workload advantage over E. I still prefer A for one argument rule, not for necessity,
performance or automatic safety. B and D remain meaningful choices when a static ceiling is more valuable.
C could use a typed bound instead of mutable ambient membership, but that would need a separately specified
value/API and is not silently included in C.

Keeping generated tables with --check is the current operational baseline, not a claim that c28 is done.
It stays until the cutover passes. Permanently abandoning the ratified direction would require an explicit
owner amendment; it is not smuggled into this ballot as the default.

### Existing solutions, fairly stated

Sources were independently read. Local Zig, Go and Rust compilers are unavailable, so these are source
comparisons, not executed peer results or peer performance evidence.

- Zig's docs give `@embedFile(comptime path: []const u8)`; its implementation calls resolveConstString
  and rejects ImportOutsideModulePath. This supports the constant-value read comparison. No blanket claim
  about every Zig comptime filesystem capability is needed for this ballot.
  Sources: https://ziglang.org/documentation/master/#embedFile and
  https://github.com/ziglang/zig/blob/master/src/Sema.zig (zirEmbedFile).
- Go's embed package defines literal package-relative patterns, disallows parent components and symlinks,
  requires nonempty matches, and exposes a read-only FS supporting computed names. It also exposes patterns
  to go list tools. Its strongest case is a tool-visible static bound and a reusable filesystem abstraction,
  not merely a second spelling of a scalar read. Jet's empty find behavior differs.
  Source: https://github.com/golang/go/blob/master/src/embed/embed.go.
- Cargo caches a host build script, supports dependency libraries, OUT_DIR outputs and rerun-if-changed.
  A script can derive table literals directly: it need not generate include_str! calls. Directory invalidation
  must be declared; host/target confusion is explicitly documented. The old framing understated this option's
  amortization and overstated how much generated source Rust requires.
  Source: https://doc.rust-lang.org/cargo/reference/build-scripts.html#rerun-if-changed.
- Jai's closed-beta sandbox/cache behavior was not verifiable here. The unsupported claim that nothing is
  sandboxed or recorded was removed. Buck was also removed rather than asserted from a Bazel-only citation.

## Claim verdicts

The numbers preserve the author's original fifteen claims; additions cover missed assertions.

| # | Claim | Verdict and repair |
|---|---|---|
| 1 | Five tables need the walk | **Holds within limits.** Source and trace show walked/listed and data-named inputs. A literal content superset also solves the read problem; 284 is the full generator, not just five tables. |
| 2 | Compiler cannot tell which computed file it reads | **Fails.** The computed build-host probe and value adapter show otherwise. Source-only pre-execution audit is a different, valid concern. |
| 3 | Computed paths necessarily weaken the sandbox | **Fails as a necessity claim.** Existing rooted authority can be retained. **Holds within limits** that A can preserve it; proposed dynamic and toolchain routes are not tested. |
| 4 | B gives a static bound | **Holds.** Even a broad bound is enforceable and reviewable; c28 does not make this benefit meaningless. |
| 5 | B necessarily costs more I/O | **Fails.** Current find already rereads files, and implementations can share bytes. No implemented A/B measurement exists. |
| 6 | A is one mechanism | **Holds within limits.** Shared sema checks and reader can give one meaning; current typed IR needs changes. D-CTIO1 must be amended. B is not automatically forbidden by I8 or the toolchain ballot. |
| 7 | Comptime derivation is affordable | **Holds within limits.** Host adapted probes complete with the stated costs. No fixed speed ratio, package-cache price or equal-option cost is established. |
| 8 | Output equals the generator | **Holds within limits.** c1/c2 matching digests on the pinned tree, not every input or the unbuilt dynamic integration. |
| 9 | Lock identity covers every read | **Fails today.** w06/w08 omit original inputs; Core has a distinct recording rule. c25 is a prerequisite, not a shipped guarantee. |
| 10 | Glob add/remove invalidates | **Holds within limits.** Nonempty run and separate build cases pass; empty find-only run is stale after its first addition. Dev also has watch gaps. No toolchain glob is executable on the seed. |
| 11 | Assemble copies the inputs correctly | **Fails as a general guarantee.** Literal text scanning cannot stage computed assets. Passing a root is not proof of membership authorization. |
| 12 | C gives A's read power with B's bound | **Holds within limits.** Needs prior finds and mutable membership state; extra reads can be reused. |
| 13 | D merely duplicates one fact | **Fails.** An authority ceiling and observed inputs are different facts. Manifest cost is real but its security-review benefit is real too. |
| 14 | Zig already does all of A | **Holds within limits.** Constant-value embed supported by source; Jet's proposed dynamic find and toolchain authority are not thereby proven. |
| 15 | Any option can land c28 now | **Fails.** Seed rejects computed embeds, toolchain prefix and #run. Parser sharing and bootstrap remain separate obligations. |
| 16 | b.find refuses a computed glob | **Fails.** Repaired review fixture successfully builds using a named String glob. |
| 17 | Everything below sema already supports dynamic paths | **Fails.** Typed host call stores a String; IR emits ConstText. |
| 18 | Input log already caches bytes; costs are removed | **Fails.** Records contain identities/digests, not reusable bytes. Reuse requires a bounded, consistent snapshot design. |
| 19 | All comptime code is hermetic Tier 1 | **Fails as a blanket claim.** This ballot concerns sanctioned recorded inputs; it must not promise arbitrary phase code has no ambient authority. |
| 20 | I1/I3/I9 pass automatically | **Fails as a sign-off.** Sema ownership, memory/resource bounds and target/tier equality need implementation proof. The required contract is below. |
| 21 | A is necessary for computed globs in c28 | **Fails.** All shown c28 globs are literal. E covers that same program. |
| 22 | An empty find still disables stale run-cache reuse | **Fails today.** probes/empty_glob.sh prints run 0/0/0 versus build 0/1/0 across addition/removal. Shared blocker under every option. |

## Shared blocker under every option

`probes/empty_glob.sh` is the reproducer; `probes/empty_glob.txt` is the independent receipt.
The program only evaluates `find("in/*.txt")`. It starts empty, then gains its first .txt file.
`jet run` incorrectly reuses count=0; `jet build` produces count=1. Removing the file restores count=0.
The file-only cache guard at ScriptCache.jet:530 has no record for the initial empty match.
**This blocks the cutover under A, B, C, D and E.** Record empty membership and validate it before
any cached result is reused. Merely changing the read spelling does not repair this shared cache hole.

## Required contract before the table cutover

These are acceptance requirements, **not observed capabilities or separate granted scope**.
They belong on c28's build work and c25/c26 where applicable; this reviewer never writes Tower.

1. **I3:** sema checks String type, phase and the common path policy, including evaluated arguments.
   If evaluation calls a policy checker, it is sema-owned; codegen does not introduce language checking.
   Rooted transport independently rejects unauthorized reads even from an invalid host request.
   Type errors, computed absolute/parent paths and forbidden toolchain authority need registered diagnostics
   and UI snapshots, with the evaluated path and original call span.
2. **I1 and sandbox:** retain per-file limits and safe cleanup on success, failure and cancellation.
   A byte snapshot needs a bounded lifetime and aggregate resource policy, not unbounded permanent retention.
   Reject symlinks and intermediate-link replacement, path escapes, embedded NUL, unsupported prefixes,
   Windows absolute/UNC/drive forms where applicable, and malformed UTF-8 for text. Bytes preserve raw data.
   Do not silently widen existing Core or ordinary-package authority.
3. **Determinism:** sorted normalized results, defined handling of duplicate patterns and prefix-preserving
   toolchain results, empty matches, missing/unreadable files and case-sensitive identity where applicable.
   Use root + embedding directory + normalized relative path for identity, not a bare spelling.
   Test two different source directories both embedding x.txt. Preserve the same result across repeated reads
   within an evaluation or diagnose concurrent mutation; revalidate snapshot inputs before publishing results.
4. **Caching and audit:** c25 must record file digests and each find's glob, authority and sorted membership,
   including an empty result. Track data that determines paths and target-dependent selection as well as leaves.
   The key includes code, options, target, selected toolchain and authority policy. Recheck old globs before cache
   reuse so additions and removals invalidate. Lock data is a receipt, not a pre-run sandbox permission.
   Audit negative probes: absent directory becoming present, no matches becoming one, additions/removals,
   edited bytes with unchanged size/timestamps, two roots with identical relative spellings and interrupted writes.
5. **Run/build/dev:** repeat the same add/remove/edit/empty cases for all three verbs, including additions outside
   the package's .jet snapshot and authorized toolchain membership. Explicit d.watch is not a required manual
   workaround for a sanctioned input. Check every watched member rather than only count and maximum timestamp.
6. **I9 and cross-target:** builds read the selected source tree on the build host; generated tables describe the
   chosen target, not whatever host APIs happen to report. Include target facts in cache identity and test different
   targets from one source tree, changed toolchains and source roots. Prove the same bytes through O0/O1/O2 and
   web where applicable. Do not invoke a foreign target binary to evaluate host-time work or add a second read policy.
   Native-level literal-read probes are not this cross-target proof.
7. **Bootstrap and sharing:** exercise assembled packages under c26's exact membership rule; merely setting
   JET_TOOLCHAIN_ROOT is insufficient. Stage package-relative assets from checked identities or retain their
   authorized source snapshot, not a guessed literal scan. Put the parser in one dependency-safe package; package
   the remaining tool appropriately rather than weakening import roots. Delete obsolete tables/parser copies
   only after a seed implementing the chosen surface can build the new dependency graph.
8. **Cost:** measure the actual five-table package, unchanged and edited builds, not only the full-generator TREE
   adaptation. Measure final binary size, peak memory and allocations as well as time. Compare with the generated
   baseline including its ordinary check gate. No performance-gate win is claimed by this ballot.

The original draft called several planned fixes “removed.” They are now open acceptance requirements.
A conditional direction recommendation is not a passing review of an unbuilt design.
