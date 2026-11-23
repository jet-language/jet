# Adversarial review: D-BUILD-TARGETFACTS1 draft

Date: 2026-11-23. Reviewer: Main (a fresh agent; the draft is G1Facts').
Subject: the draft ballot for card #5007 c6 to c9, which asks how `fn build`
gives each output its own setting values and which names the target hardware
facts keep. Recommendation under review: option A (per-output `options:`
settings, one set of target facts, `#build.cpu.cache_line` retired).

## Claim 1: A "reuses the `options:` value every output already takes"

Fails as worded. The draft's own detail says the labeled `options:` argument
on `b.add_*` is ratified (D-TARGET-ONE1=A, D-BUILD-CORE1=A) but not built:
`b.add_executable` takes its arguments by position today
(`Compiler/JetBuildSession/Source/Declarations.jet`). A therefore depends on
building `options:` first, and its cost includes that work. The ballot must
say so and name the card that builds `options:`.

## Claim 2: the two cache-line facts are "two names for one thing"

Fails. The two facts have different ratified meanings:

- `#build.cpu.cache_line` is the selected CPU's cache line
  (D-CACHE-CONTROL1=A, `Docs/Research/2026-10-30-cache-control-and-visibility.md`
  line 22: "with a default of 64 on generic targets"). It answers the
  constant `COMPILER_BUILD_CPU_CACHE_LINE_GENERIC`
  (`Compiler/JetSema/Source/Sema/Expressions/FactReads.jet:868`) only because
  no CPU can be selected yet.
- `#build.target.layout.cache_line` is the triple's baseline:
  `jet_target_layout_cache_line` answers 128 for `aarch64-apple-*` and 64
  for every other triple (`Compiler/JetFoundation/Source/Target/Layout.jet:249-253`).

The same split holds for `#build.target.cpu_features` (features every CPU of
the triple has, `Layout.jet:255-261`) against a selected CPU's features.
Once a CPU flag exists, the two answers differ by design. Retiring the CPU
fact loses the meaning D-CACHE-CONTROL1 ratified.

## Claim 3: the retirement is harmless ("the same 64 on every generic target")

Fails. A rewrite from `#build.cpu.cache_line` to
`#build.target.layout.cache_line` changes the value from 64 to 128 on
`aarch64-apple-*`, so `#Layout(c, align(…))` padding in
`Examples/Memory/Allocation/CacheLinePadding.jet` and in user code doubles there.
The "retired spelling with a fix" form also conflicts with the greenfield
cutover rule in AGENTS.md: an old spelling is removed and every in-repository
caller is migrated in one change (37 occurrences today, in 15 files), with no
kept retired spelling.

## Claim 4: A is the strongest option (option space)

Holds only after a missing option is added. The draft lacks the option that
keeps both families with distinct meanings:

- `#build.target.*` is the triple's guaranteed baseline.
- `#build.cpu.*` (`cache_line`, `features`) is the selected CPU's, which
  defaults to the triple's baseline (128 on Apple arm64, not a flat 64) until
  CPU selection exists.

This fixes today's wrong 64 on Apple arm64 with no rename, and gives a
future `--cpu` a home. The ballot should offer it, either as its own option
or as A's fact half, and compare it with A's merge.

## Claim 5: per-output settings are fully specified

Holds only with stated limits. The draft does not say:

- whether `options: .{settings: …}` can set a dependency package's
  settings, or only the root package's;
- what happens when two outputs share a library and give it different
  values (two compiled instances of the library, and the build-time cost of
  that);
- whether a setting may change a public type of a shared library across
  outputs.

Zig's per-artifact options module, which the draft cites, scopes options to
the module that imports them. The ballot must state Jet's scope rule with the
same precision, and add these cases to its edge cases.

## Verdict

The settings half of A is the right direction, but the draft cannot post.
Claims 1 to 3 fail and claims 4 and 5 hold only within the stated limits.
Before posting, the draft must:

1. State that `options:` is ratified but unbuilt, and name the card that
   builds it.
2. Withdraw "two names for one thing". Keep `#build.target.*` as the triple
   baseline and `#build.cpu.*` as the selected CPU, which defaults to the
   triple's value, or present the merge as an explicit option with the
   64-to-128 change on Apple arm64 named as its cost.
3. Drop the retired spelling. Any rename migrates every caller in one
   cutover.
4. State the scope of per-output settings (root package or dependencies),
   the shared-library case and its compile cost.
5. Decide whether `cpu_features` stays a `[String]` or becomes a typed set,
   and say why.
