# Foundations probe — common brief (read fully before starting)

You are one prober in a wave that answers one question for Jet's owner: **can a library author build this with today's Jet, and if not, which primitive is missing?** You build real code with the real `jet` binary, you record what happened, and you never propose a niche library. The orchestrator writes every ballot; you produce evidence.

## The rule you are testing (owner, 2026-09-02)

Core keeps four things: (1) language primitives a library cannot add for itself: syntax, the type system, memory and effects, execution tiers, the runtime; (2) ecosystem and developer-experience tools: build, packages, devtools, live loop, tests, receipts, diagnostics, editor support; (3) today's core library; (4) shared data types two libraries must agree on: one table, one unit, one receipt. Everything else (an acoustics library, a ledger, a media pipeline) is a package a library author writes in Jet. Your job is to find what that author cannot do.

## What counts as a gap (the rubric)

Tag every finding with one or more of these; anything else is not a gap:

- `impossible` — cannot be expressed or run at all (compile error, missing capability, missing tier).
- `unsafe` — cannot be made memory- or type-safe without `#Unsafe`.
- `slow` — cannot reach the incumbent's speed band from library code because it needs the compiler (no SIMD, no layout control, no fusion, allocation you cannot avoid). Measure before you claim it: time your Jet program and the incumbent on the same input.
- `call-site` — every USER of the library must write ceremony the incumbent's users do not (extra wrappers, unwraps, conversions at each call).
- `boilerplate` — the library AUTHOR must repeat heavy code for a common action. Count the repetitions and name the action. This is a gap only when the same boilerplate is very common or likely shared across domains; say which.
- `defect` — a bug in current Jet (panic, ICE, wrong answer, diagnostic that lies). Give the exact repro.

Ugliness inside the library that a user never sees is not a gap; note it under "friction" if it is heavy, tagged `boilerplate`, with counts.

"Very good, not perfect": an awkward but working, safe, fast-enough build is **buildable**. Say so plainly.

## How to work

1. Read your probe brief (`docs/research/domain-foundations/probes/<probe>.md`; the wave runner copies it to `~/.cache/jet-luna/dx3/<probe>/BRIEF.md`). It names the program to build and the research to mine.
2. Learn today's Jet from the source of truth, not memory: `docs/spec/spec.md` (search it), `docs/spec/stdlib-api-laws.md`, `examples/features/**` (every feature ships an example; read the closest ones), `scripts/agent/jet-env jet --help`, `scripts/agent/jet-env jet inspect facts`, and `crates/jet-codegen/src/Prelude/**` for the shipped core API. When something looks missing, search before you conclude.
3. Build in `~/.cache/jet-luna/dx3/<probe>/pkg/` as a real Jet package where a package is the natural shape (a `package.jet` and modules), otherwise as files. Run with `scripts/agent/jet-env jet run <file>` (default tier), then `scripts/agent/jet-env jet build` when the brief asks. Keep every run's exact command and output; put the important ones in the report verbatim.
4. Mine the archived research for your area or mechanism: `~/.cache/jet-luna/dx2/<domain>/report.md` and `census.json` per domain, `~/.cache/jet-luna/dx2/_families/<family>/synthesis.md`, and the deleted ballots in `~/.cache/jet-luna/dx2/ballots/<ID>.json` (options, code, comparisons). They tell you what the best tool in the field does; you test whether a Jet library could do it.
5. Never edit the repository. Never run `cargo` (another orchestrator is building in the shared target; the binary at `target/debug/jet` is fresh). Never write under `/tmp`. Never run `tower` write commands. If the binary is missing or broken, report it and stop.
6. Time box: 60 minutes of work. Breadth beats polish: it is better to hit eight parts of the program shallowly than to perfect one.

## What you return

Write three files in `~/.cache/jet-luna/dx3/<probe>/`:

`probe.md` — the report, in this order, plain prose, under 400 lines:
1. **What I built** — the program, in three sentences, and the file list.
2. **What worked** — the parts a library author can build today, each with the one-line proof (command + result). Be concrete: "typed HTTP routes with query params: works, `jet run server.jet` served 200 on /orders?limit=3".
3. **Gaps** — one entry per gap: tag(s), the primitive that is missing named as a capability ("no way to declare a fixed-timestep loop with a deadline"; never "no game engine"), the exact evidence (command, error text or timing), and which other areas you believe share it. Order by how much it blocked you.
4. **Friction** — heavy boilerplate with counts, and call-site ceremony, even where a workaround exists.
5. **Defects** — exact repros, one per bug.
6. **Battery notes** (critical-area probes only) — what a first-party battery package for this area should contain to get a builder to a working first program, as a list of parts with one line each, and which of them are pure library code.
7. **Verdict** — buildable today / buildable with listed gaps fixed / not buildable, and why, in five lines.

`gaps.json` — an array of objects: `{ "id": "<probe>-G<n>", "tags": ["slow"], "primitive": "one sentence naming the missing capability", "evidence": "exact command and output or timing", "areas": ["games", "embedded"], "severity": "blocks | hurts | annoys", "workaround": "what a library author can do today, or null" }`. Every entry appears in probe.md.

`batteries.json` (critical-area probes only) — `[{ "part": "...", "why": "...", "libraryOnly": true }]`.

Final message under 15 lines: verdict, gap count by severity, the top three gaps with their tags, anything you could not run.
