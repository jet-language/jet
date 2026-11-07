# Tooling

Tests, debugging, the dev loop, builds, performance and developer tools.

Run a lesson from the repository root, for example `jet run Examples/Tooling/<Lesson>.jet`. Each lesson's expected output sits beside it as `<Lesson>.out`.

## Lessons

- [CanvasBlueprintDemo](CanvasBlueprintDemo.jet) — Canvas onboarding: read the graph, edit the value in Inspector, Check, Run, then Undo.
- [DevTools](DevTools/)
- [Fidelity](Fidelity.jet) — Perf fidelity signal — explicit app-read quality/perf knob.
- [InternalProjectParts](InternalProjectParts.jet) — This declaration is omitted from automatic project discovery until another source writes `use project._bench`.
- [OutputCallable](OutputCallable.jet) — A Package executable can name ordinary checked code.
- [SolvePuzzle](SolvePuzzle.jet)
- [StructureLiveness](StructureLiveness.jet) — The four liveness verdicts are warnings, not runtime work.
- [StructurePlane](StructurePlane/) — `jet inspect structure Examples/Tooling/StructurePlane/run.jet` exposes the registered structure rows without changing this program.

## Build

- [BuildLinkInputs](Build/BuildLinkInputs/) — The link and target rows: jet build                              writes dist/run (`b.output_dir("dist")`) jet build --output-dir=out             writes…
- [BuildOptions](Build/BuildOptions/) — Build options never change what a program prints, only how hard it is optimized: jet run --profile=fast run.jet                  optimize: full, from the…
- [BuildProfiles](Build/BuildProfiles/) — Named build profiles — an example package that declares custom profiles in its `build {}` block.
- [EmitTiers](Build/EmitTiers.jet) — `jet emit` prints one compiler tier of a file, the same tree as text or, with `--json`, as the `emit` status; `--fn NAME` keeps one function: jet emit --ast…
- [RunDefault](Build/RunDefault/) — The package default: bare `jet run` selects this file.

## Debugging

- [Debug](Debugging/Debug.jet) — A small program you can step through with `jet debug`.
- [DebugNative](Debugging/DebugNative.jet)
- [PanicReport](Debugging/PanicReport.jet)
- [ProvenanceTrack](Debugging/ProvenanceTrack.jet)
- [TodoHole](Debugging/TodoHole.jet) — `#Todo` typed goal.

## DevLoop

- [AppLive](DevLoop/AppLive.jet) — Live bindings: `app.live` binds a database query to one argument list, `app.state` reads the binding's current Loadable snapshot, `app.invalidate` re-runs…
- [BenchOverride](DevLoop/BenchOverride.jet) — Measurement is selected by `jet test --measure`; the command override remains the ordinary `test: fn` path.
- [DevContext](DevLoop/DevContext/) — `jet run` runs this; `jet dev` reruns it on every change, configured by dev.jet.
- [JobEnv](DevLoop/JobEnv.jet) — A job sets an environment variable on each command it launches with `ProcessSpec.env`, not on its own process.
- [JobRunner](DevLoop/JobRunner.jet) — Project jobs are Jet `#Job fn`s with typed visibility.
- [Persist](DevLoop/Persist.jet) — `#Persist` pins live dev-loop state.
- [ScheduleEvery](DevLoop/ScheduleEvery.jet) — Schedule-as-code.
- [ScriptJob](DevLoop/ScriptJob.jet) — Short path first: this is a direct script entry.
- [ScriptJobExpert](DevLoop/ScriptJobExpert.jet)
- [ScriptToSystem](DevLoop/ScriptToSystem/)
- [StaticSiteGenerator](DevLoop/StaticSiteGenerator/) — `jet dev` runs a generator, serves its output, and reloads it when the generator or one of its filesystem inputs changes.
- [TestOverride](DevLoop/TestOverride.jet) — `test: fn` owns command policy while the compiler-owned Test remains an ordinary value.
- [TopLevel](DevLoop/TopLevel/) — In this directory, run `jet deploy --region eu` or `jet --release deploy --region eu`.

## Performance

- [Bench](Performance/Bench.jet) — `.measure` marks a `#Test` claim for `jet test --measure`; plain `jet test` still checks the claim once.
- [Hello](Performance/Hello/) — Package used by the lessons beside it.
- [HTTPReady](Performance/HTTPReady/) — Package used by the lessons beside it.

## Testing

- [Doctests](Testing/Doctests.jet) — doctests.
- [ExpectedFail](Testing/ExpectedFail.jet) — A marked test runs, stays green while failing, and turns red when the known bug is fixed but the marker remains.
- [PropertyTests](Testing/PropertyTests.jet) — property tests.
- [TestCompare](Testing/TestCompare/) — Equal finite samples are an honest matched result, not a universal proof.
- [TestHistory](Testing/TestHistory/) — One ordinary public strategy owns resource-aware generation, rebuilding, validity, bounds, and weighted distributions.
- [TestingFakeData](Testing/TestingFakeData.jet)
- [TestingHelpers](Testing/TestingHelpers.jet)
- [TestingProjection](Testing/TestingProjection.jet) — Corpus, fixture, and golden helpers all read explicit paths, here relative to the example's own folder (the directory it runs from).
- [TestMembers](Testing/TestMembers.jet) — Contextual scope members inside a `#Test` block.
- [TestOutcomes](Testing/TestOutcomes/) — Only a matched comparison passes.
- [TestPackageModules](Testing/TestPackageModules/)
- [Tests](Testing/Tests.jet)
- [TestTarget](Testing/TestTarget/) — Measurement is a mode of `#Test`, not a second claim family.
