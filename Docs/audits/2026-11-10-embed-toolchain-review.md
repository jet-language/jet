# D-EMBED-TOOLCHAIN1: how compiler packages read Core sources while building (2026-11-10)

Ballot: D-EMBED-TOOLCHAIN1 (card #5072, goal G4). Author and reviewer: BallotLead-3 (Claude Opus).
Compiler: a0.0.43 (dev c460ba91e5), the published dev seed. Code citations are on dev c460ba91e5.
Probes: `$JET_SCRATCH_ROOT/lanes/BallotLead/out/D-EMBED-TOOLCHAIN1/probes`, run with `probe.sh` and
`timeit.sh` from `$JET_SCRATCH_ROOT/lanes/BallotLead/out/`.

## 1. What the ballot decides

Jet's compiler carries tables derived from Core's own source files (the effect list, module rows,
owned traits). A generator, `Tools/agent/CoreSourceRows.jet`, copies them into checked-in compiler
files with a SHA-256 header. The ballot picks how a compiler package reads those Core files itself
while it is built, so the copies and the regenerate step go away.

## 2. Measurements

### 2.1 Size of the generated tables and their churn (git census on dev c460ba91e5)

Scripts: `census.sh` and `census2.sh`, run from the dev worktree.

| Set | Files | Bytes | Commits since 2026-10-01 |
|---|---:|---:|---:|
| The ballot's five compiler files (CoreSources, CoreStructureRows, CoreSourceOwned, EffectVocabulary, OwnedTraitRows) | 5 | 279,898 | 341 |
| Four more derived tables the generator writes into compiler and Jetpack packages (CoreCallRows, CoreEnumRows, CoreEnumPayloadRows, ForeignLanguages) | 4 | 960,769 | 488 |
| Both sets together | 9 | 1,240,667 | 509 |
| The generator's other outputs (Core/EmbeddedSources.jet, Core/Compiler/Lang.jet, Compiler/Prelude/Core.jet, the core-library reference section) | 4 | 754,037 | 612 |

Of the 341 commits that touched the five files, 52 are titled as regenerating tables, 228 also change
Core or Prelude sources, and 2 name a generated-header conflict. Merges such as `911f354d07` and
`5b3cca254e` record resolving "generated Source SHA conflicts" by rerunning the generator.

Finding: the ballot counted only the five files. The generator's own output list
(`Tools/agent/CoreSourceRows.jet:2908-2921`) writes twelve files. Four of the other seven are compiler
or Jetpack tables that a `toolchain:` read could replace just as well: 960,769 more bytes, including
`CoreCallRows.jet` at 895,709 bytes.

### 2.2 What today's compiler does with each spelling (a0.0.43)

| Probe | Spelling | Result |
|---|---|---|
| `e01_beside.jet` | `comptime { embed_file("e01_data.txt") }` | runs: `beside: hello` |
| `pkg/` (src/module.jet) | `embed_file("../data.txt")`, a file inside the same package | E0957, by design |
| `e02_core_prefix.jet` | `embed_file("core:Effects.jet")` | E0955, read as a plain relative path |
| `e03_toolchain_prefix.jet` | `embed_file("toolchain:Compiler/Prelude/Effects.jet")` (A's spelling) | E0955: not built |
| `e04_run_spelling.jet` | `TEXT: #run embed_file(...)` (D-META-SPELL1=A) | E0733: `#run` is not built |
| `pkg_ok/` | a package embedding a file beside it, then `jet run` and `jet build` | runs; no `.jet/lock` is written |

### 2.3 What A's build-time derivation costs (a0.0.43, 4G cap, cold = `.jet/` removed before each run)

`cost/c1_derive.jet` embeds a copy of `Compiler/Prelude/Core.jet` (278,915 bytes) and counts its
`source_module` rows in a compile-time function; it prints `source_module rows: 116`. `c0_none.jet`
is the same program with the count written as a literal.

| Run | c0 (literal) | c1 (embed and derive) | Difference |
|---|---|---|---|
| `jet check`, cold, 5 runs, median | 0.070 s, 36 MB | 0.114 s, 57 MB | +44 ms, +21 MB |
| `jet build`, cold, 5 runs, median | 0.046 s, 50 MB | 0.078 s, 66 MB | +32 ms, +16 MB |
| `jet check`, warm (cached), 5 runs, median | 0.005 s | 0.006 s | about 1 ms |

The full generator, `jet Tools/agent/CoreSourceRows.jet --check` with the toolchain root set, takes
3.09 s wall and 414 MB peak for all twelve views (263 rows, 55 effects). That is an upper bound on
the derivation A would move into compiler builds, paid once per edit of a file that is read.

### 2.4 Is the cache keyed by content? (A's stale-table hazard)

`cost/stale.sh` runs c1 warm, appends one `source_module` line to the embedded file, runs again,
then restores the file:

```
warm:    source_module rows: 116
edited:  source_module rows: 117
reverted: source_module rows: 116
```

The cached result follows the file's bytes. The driver records each embed's SHA-256
(`Compiler/JetDriver/Source/Driver/ComptimeInputs.jet:54-57`) and the script cache keeps that log
(`Compiler/JetCli/Source/Cli/ScriptCache.jet:78,489,505`).

### 2.5 Is every embed recorded in `.jet/lock`?

The spec says it is (`Docs/spec/spec.md:539`). On a0.0.43, `pkg_ok/` builds and runs with an embed
and no `.jet/lock` appears. The lock writer supports `[[comptime_inputs]]` rows
(`Jetpack/PackageModel/Source/Lock/Write.jet:97-101`), but the CLI fills them only for generated
sources (`Compiler/JetCli/Source/Cli/InspectBuild.jet:267-275`). Embed digests reach the cache key,
not the lock. This is a defect against the spec, and A's "the lock lists every read" mitigation
depends on it.

### 2.6 The tree lookup A reuses

`jet_cli_toolchain_root` (`Compiler/JetCli/Source/Cli/Request.jet:295-303`) returns
`JET_TOOLCHAIN_ROOT`, else the nearest directory at or above the input that holds a toolchain. The
Core rows already resolve through it (`Request.jet:1049-1053`). The compiler workspace already names
paths above itself: `Compiler/package.jet:43-47` points its test suites at `../Examples` and
`../tests/...`.

## 3. Peer languages (sources read 2026-11-10)

| Language | Rule | Source |
|---|---|---|
| Go | `//go:embed` patterns "may not contain '.' or '..'"; they "must not match files outside the package's module". | https://github.com/golang/go/blob/master/src/embed/embed.go (lines 68, 93) |
| Go's own compiler | Keeps a checked-in generated file: `typecheck/builtin.go` starts "Code generated by mkbuiltin.go. DO NOT EDIT." This is option D's best case at scale. | https://github.com/golang/go/blob/master/src/cmd/compile/internal/typecheck/builtin.go |
| Rust | `include_str!` reads a path "relative to the current file", with no root limit, so `../` works. | https://doc.rust-lang.org/std/macro.include_str.html |
| Zig | `@embedFile` takes a path relative to the current file "just like @import"; outside the package it fails with "embed of file outside package path". | https://github.com/ziglang/zig/blob/master/src/Sema.zig (line 13920 on master) |

Fair best cases. Rust shows that an unrestricted relative read is simple and works, but any crate
can then read any file. Go and Zig show that a hard root keeps builds auditable; they reach files
outside it only through a build step (Zig's `addAnonymousImport`, Go's `go generate`). Go's compiler
shows that D, a generated file with a header, is a workable long-term choice when edits are rare.
Jet's case differs: 341 commits in six weeks touched the five files.

## 4. The same program in each option

The effect-source table, `JET_CLI_EFFECT_SOURCE` in `Compiler/JetCli/Source/Cli/CoreSources.jet`,
written each way (today's `comptime { }` spelling; `#run` is the ratified successor):

```
// A: prefix, allowed only for packages inside the toolchain tree.
JET_CLI_EFFECT_SOURCE: comptime { embed_file("toolchain:Compiler/Prelude/Effects.jet") }

// B: the same line, plus one grant in the workspace manifest Compiler/package.jet:
//   toolchain_reads: ["Compiler/Prelude/**", "Core/**"]
JET_CLI_EFFECT_SOURCE: comptime { embed_file("toolchain:Compiler/Prelude/Effects.jet") }

// C: Prelude becomes a package; compiler packages depend on it.
//   deps: { jet_prelude: ../Prelude }
JET_CLI_EFFECT_SOURCE: comptime { embed_file("jet_prelude/Effects.jet") }

// D: today. A generator writes the literal; a header pins its source.
// Source SHA-256: b4561341...; generated, do not edit.
JET_CLI_EFFECT_SOURCE: "// The machine effects are written here..."
```

## 5. Adversarial review, claim by claim

| # | Claim | Attack | Evidence | Verdict |
|---|---|---|---|---|
| 1 | A ends the regenerate step after Core edits. | The generator writes twelve files, not five. | `CoreSourceRows.jet:2908-2921`; census 2.1 | Failed as written. Redesigned: A's scope now covers all nine derived compiler and Jetpack tables (1.24 MB, 509 commits of churn). Four non-compiler outputs stay generated and are named. |
| 2 | Tables never go stale. | A cache keyed by the compiler identity would keep stale rows. | stale.sh: 116, 117, 116 | Pass. The cache follows content today. |
| 3 | The lock lists every toolchain read, so the reads are auditable. | Is the lock written at all? | 2.5: no `.jet/lock` on build or run | Failed today. The lock record is a precondition, named as A's first build criterion. The read sites stay greppable meanwhile. |
| 4 | Build-time derivation is cheap. | 280 KB of text scanned at compile time on every build. | 2.3: +44 ms cold check, about 1 ms warm; full generator 3.09 s | Pass, with a named cost: up to about 3 s of cold build work after an edit to a read file. |
| 5 | Ordinary packages never gain access. | Could `JET_TOOLCHAIN_ROOT` aim a user package at a checkout? | The gate compares the package directory with the tree `Request.jet:300` returns | Pass by design: a package outside that tree gets the new registered error. |
| 6 | No ambient name is added (D-PRELUDE-LAW1). | A prefix inside a string literal is not a name. | spec.md:536-541 | Pass. |
| 7 | B writes each path twice. | A grant can be one glob line in the workspace manifest. | `Compiler/package.jet:43-47` already names `../` paths | Failed as written. B is redesigned: one workspace grant, not per-path. Its remaining cost is a new manifest field that still needs A's location gate. |
| 8 | C conflicts with D-BUILD-COREUNIT1=A. | | The ratified rule keeps Core one compiler-known unit | Pass. |
| 9 | The `#run` spelling works. | | e04: E0733 | Failed today. The ballot code now uses `comptime { }` and says `#run` is its ratified successor. |

## 6. Each loss and the two attempts to remove it

1. Loss (A): derived rows cost build-time work.
   Attempt 1: cache by each read file's content; measured working (2.4).
   Attempt 2: split what is copied from what is derived. A pure copy, such as the effect source, is an
   embed with no evaluation. Only row tables run code.
   Remaining cost: up to about 3 s of cold work after an edit to a read file, against 3.09 s of manual
   generator time today plus a commit.
2. Loss (A): nothing in package.jet shows the permission.
   Attempt 1: list every read with its digest in the lock. Not built today (2.5), so it becomes the
   build's first criterion.
   Attempt 2: make each read self-describing. Every read site spells `toolchain:`, so one search
   lists them all, as Go's `//go:embed` lines do.
   Remaining cost: the permission itself is a location rule; the registered error states it.
3. Loss (A): D-CTIO1's "never escape the project" gains an exception.
   Attempt 1: confine it to packages inside the toolchain tree.
   Attempt 2: define the toolchain tree as these packages' project, as the compiler workspace already
   does for its test corpora (`Compiler/package.jet:43-47`).
   Remaining cost: D-CTIO1's text is amended by one sentence; no other package's rule changes.
4. Loss (B): a second gate, a new manifest field. Attempt 1: one workspace glob, not per file.
   Attempt 2: inherit the grant from the workspace to members. Remaining cost: the field is public
   manifest surface, and A's location check is still needed so user packages cannot write it.
5. Loss (C): Core becomes a dependency. Attempt 1: a private, unpublished Prelude package.
   Attempt 2: allow only declared embeddable files. Remaining cost: contradicts D-BUILD-COREUNIT1=A.
6. Loss (D): churn. Attempt 1: run `--write` in the land train. Attempt 2: drop the SHA headers so
   merges stop conflicting. Remaining cost: every Core edit still regenerates and commits up to 1.24 MB.

## 7. Verdict

A, with its scope widened to the nine derived compiler tables and with the lock record as its first
build criterion, passes. Claims 1, 3, 7 and 9 failed as first written and are corrected in the ballot.
No claim of A depends on unbuilt behavior without saying so.

## 8. Findings for cards

- `.jet/lock` records no embed digests on a0.0.43, against `Docs/spec/spec.md:539` (probe `pkg_ok/`).
  Card #5072 or the D-CTIO1 owner card.
- The ballot's scope missed four derived compiler tables (960,769 bytes); A's build card should list
  all nine.
