# Hand-off inventory: work that lives outside the repo (2026-11-04)

Cards: #6564 (a fresh clone is enough to continue), #6563 c3/c4, #5072 c9 (repo
tooling is Jet). This lists every file or path outside the repository that agent
work depends on, and every committed file that hard-codes a machine path.

Kinds: **tooling** (a program agents run), **config** (data a tool reads),
**seed** (a trusted compiler), **state** (queues, locks, logs; throwaway),
**output** (rebuildable from the repo and a seed), **evidence** (a record of a
past run; it keeps the path it recorded), **dead** (nothing live runs it).

## Root variables

Tools read these and nothing else; the machine defaults are set only in
`Tools/agent/jet-env` (`$JET_SCRATCH_ROOT` and the musl path apply on this machine
only, when they exist). Generic defaults derive from `$HOME`, so a fresh clone
needs only a seed.

| Variable | Meaning | Generic default | This machine |
|---|---|---|---|
| `JET_STATE_ROOT` | per-user dev state: agent scratch, caches, release work | `$XDG_CACHE_HOME/jet-dev` | `$JET_STATE_ROOT` |
| `JET_SCRATCH_ROOT` | large shared scratch: snapshots, stage store, suite output | `$JET_STATE_ROOT` | `$JET_SCRATCH_ROOT` |
| `JET_RUN_ROOT` | coordination state: land queue, locks, slot ledger | `$JET_SCRATCH_ROOT/run` | `$JET_RUN_ROOT` (kept: the live lander uses it) |
| `JET_BASE_ROOT` | published compilers, one dir per landed batch | `$JET_SCRATCH_ROOT/discovery/base` | same |
| `JET_TARGET_ROOT` | build target dirs | `$JET_SCRATCH_ROOT/targets` | same |
| `JET_SEEDS` | seed store (`current`, `a0_NNN`) | `$XDG_DATA_HOME/jet-seeds` | `~/.local/share/jet-seeds` |
| `JET_SEED` | the admitted seed dir (`jet`, `IDENTITY`, `jet-runtime/`) | `$JET_SEEDS/current` | same |
| `JET_DEV_WORKTREE` | the checkout the lander commits to | the repo jet-env runs from | `.agent-worktrees/Main-dev-collectall` |
| `JET_PACK_MUSL_LIB` | static musl libc dir for native packs | from the Nix shell | the pinned `/nix/store/...musl-1.2.5/lib` |

## External tooling, config and state

"Gap" names a Jet Core capability the Jet port needs and does not have yet.
"none found" means the port uses only existing Core surfaces (`core.process`
specs and `run_all`, `core.files` locks, `Path`, `core.crypto` hasher, JSON).

| # | Path (outside the repo) | Used by | Kind | Repo home (Jet, via jet-env) | Root vars | Gap |
|---|---|---|---|---|---|---|
| 1 | `$JET_STATE_ROOT/scratch/Main/lander.sh` | Main's landing loop (queue, stacked prescreen, bisecting batches) | tooling | `Tools/agent/Land.jet serve` | RUN, DEV_WORKTREE | #5072 c2 (bounded worker pool) for parallel prescreens; serial works now |
| 2 | `…/Main/batch-land.sh` | lander: apply, generators, precommit, private-index commit chain, CAS update-ref | tooling | `Land.jet batch` | RUN, DEV_WORKTREE, SEED | none found |
| 3 | `…/Main/prescreen.sh` | lander admission (`jet check Compiler/JetCli` on an archived HEAD) | tooling | `Land.jet prescreen` | SCRATCH, SEED | none found |
| 4 | `…/Main/precommit.sh` | lander and manual commits: collect-all census | tooling | `Land.jet precommit` | SCRATCH, RUN | none found |
| 5 | `…/Main/census-snap.sh` | the census: check, build Compiler, runtime pack, selfcheck | tooling | `Land.jet census` (also the AGENTS.md preflight) | SCRATCH, SEED | none found |
| 6 | `…/Main/capped.sh` | memory-capped scope for census | tooling | folded into `Land.jet` (systemd-run spec) | — | none found |
| 7 | `…/Main/runtime-pack-check.sh` | census runtime pack (generates `prebuild-runtime-reuse.mjs`) | tooling | `Land.jet census` pack step (the chain's direct `prebuild-runtime.mjs` form) | MUSL | none found |
| 8 | `…/Main/census-errors.mjs`, `…/Main/native-preflight.mjs` | census error rows to source paths | tooling | `Land.jet census` report | — | none found |
| 9 | `…/Main/census-jetpack.sh` | nothing (code after `exit 0`) | dead | retire to held/ | — | — |
| 10 | `$JET_SCRATCH_ROOT/lanes/Main/unstick.sh` | lander cleanup after a batch | tooling | `Land.jet batch` (reverts only what the batch applied) | DEV_WORKTREE | none found |
| 11 | `…/Main/publish-latest.sh` | lander: `discovery/b2/install-latest` | tooling | `Land.jet publish` | BASE | none found |
| 12 | `…/Main/publish-base.sh` | lander: `base/<sha>`, goldens, jetpack, seed-published chain | tooling | `Land.jet publish` | BASE, SEED | none found |
| 13 | `…/Main/release-train.sh` | lander hook to `Tools/agent/release-compiler.mjs` | tooling | `Land.jet release-train` | STATE, SEEDS | none found |
| 14 | `$JET_SCRATCH_ROOT/lanes/Main/q.mjs` | every lane: enqueue with Pareto gate | tooling | `Land.jet queue` | RUN | none found |
| 15 | `…/lanes/Main/stackcheck.sh` | q.mjs: stack applies on HEAD (temp index under /tmp) | tooling | `Land.jet queue` | RUN | none found |
| 16 | `…/lanes/Main/pareto-approved.txt` | q.mjs speed-lane gate | config | `Tools/agent/land/pareto-approved.tsv` | — | — |
| 17 | `$JET_RUN_ROOT/{land-queue,land-results,lander.log,batch-land.log,lander.stop,commit.lock,precommit-census.lock}` | lander | state | `$JET_RUN_ROOT/…` | RUN | — |
| 18 | `$JET_RUN_ROOT/{slot-queue,build-slot-*.lock,slot-ledger.tsv}`, `…/Main/.slot-key` | build-slot | state | `$JET_RUN_ROOT/slots` | RUN | — |
| 19 | `$JET_SCRATCH_ROOT/discovery/r14/{snap,census}` | census and chain canonical build root | output | `$JET_SCRATCH_ROOT/discovery/r14` | SCRATCH | — |
| 20 | `$JET_SCRATCH_ROOT/discovery/b2/install-latest` | published install | output | `$JET_BASE_ROOT/../b2` | BASE | — |
| 21 | `$JET_SCRATCH_ROOT/discovery/base/<sha>` | published compilers; the fresh-clone seed today | output/seed | `$JET_BASE_ROOT` | BASE | — |
| 22 | `$JET_STATE_ROOT/release` | release train state | state | `$JET_STATE_ROOT/release` | STATE | — |
| 23 | `…/Main/chain.sh` | lanes: export / check / s1 / full / seed-published | tooling | `Tools/agent/Chain.jet` | SCRATCH, RUN, SEED, MUSL | none found |
| 24 | `…/Main/chain-lib.mjs` | chain: diff, plan, input hashing, eviction | tooling | `Chain.jet` | — | none found |
| 25 | `…/Main/trapscan.mjs` | chain: ELF scan for `todo` traps | tooling | `Chain.jet trapscan` | — | none found |
| 26 | `…/Main/build-slot.sh` | chain, release-compiler: CPU/memory slots and ledger | tooling | `Tools/agent/Slot.jet` | RUN | none found |
| 27 | `$JET_SCRATCH_ROOT/lanes/SeedRotate/norust.sh` | chain: PATH without Rust | tooling | `Chain.jet` (PATH filter) | — | none found |
| 28 | `…/SeedRotate/gsel.mjs` | publish-base, suite: golden selection | tooling | `Suite.jet` | — | none found |
| 29 | `…/SeedRotate/rotate.sh`, `cmp.mjs` | cited by chain.sh and CHAIN.md; missing | dead | none (release-compiler.mjs replaced them) | — | — |
| 30 | `…/lanes/Main/CHAIN.md` | lane guide for chain levels and bases | doc | the `Chain.jet` header and AGENTS.md | — | — |
| 31 | `$JET_SCRATCH_ROOT/stagestore` (fallback `$JET_RUN_ROOT/stagestore`) | chain stage store | output | `$JET_SCRATCH_ROOT/stagestore` | SCRATCH | — |
| 32 | `$JET_RUN_ROOT/{chain-ns,chain-work}` | chain mount namespace and work | state | `$JET_RUN_ROOT` | RUN | — |
| 33 | `~/.local/share/jet-seeds/{current,a0_NNN,current.env}` | the seed launcher, release-compiler | seed | outside by design; source decided by D-SEED-DIST1 (A: publish the proven compiler) | SEEDS, SEED | — |
| 34 | `~/.local/share/jet-seeds/bin/jet` | seed launcher: toolchain root, identity, musl, stack | tooling | `Tools/agent/jet-env` (sets the same from `$JET_SEED`) | SEED | — |
| 35 | `/nix/store/wrng0x…-musl-static-…-1.2.5/lib` | chain, census, launcher | config | `jet-env` (from the Nix shell) | MUSL | — |
| 36 | `$JET_SCRATCH_ROOT/lanes/Main/q/sg/suite.sh` | suite slices on a snapshot | tooling | `Tools/agent/Suite.jet` | SCRATCH, BASE | #5072 c2 for its parallel shards (it uses `--jobs`; serial works now) |
| 37 | `$JET_SCRATCH_ROOT/lanes/TestGreen/tk-build.sh` | suite: cached testkit build | tooling | `Suite.jet testkit` | STATE | none found |
| 38 | `$JET_STATE_ROOT/scratch/TestGreen/tk/<key>` | testkit cache | output | `$JET_STATE_ROOT/testkit` | STATE | — |
| 39 | `$JET_SCRATCH_ROOT/lanes/Main/q/sg/*.txt`, `$JET_STATE_ROOT/scratch/Main/sgfs` | suite output and snapshot | output | `$JET_SCRATCH_ROOT/suite` | SCRATCH | — |
| 40 | `$JET_SCRATCH_ROOT/lanes/Main/ballots/validate.mjs` | ballot gate before `decision add` | tooling | Tower CLI `decision validate` (one gate, in Tower) | — | — |
| 41 | `$JET_STATE_ROOT/ballots/{gates,READY,…}`, `lanes/Main/ballots/*` | ballot drafts | evidence | Tower decision drafts (#6563 c2) | — | — |
| 42 | `$JET_STATE_ROOT/scratch/Main/preflight.mjs`, `preflight-gate.mjs` | nothing live (Rust-era stage0; its compilers are gone; last receipt Oct 21) | dead | retire to held/; `Land.jet census` + `Chain.jet --dry-run` replace it | — | — |
| 43 | `$JET_STATE_ROOT/scratch/DevGreenLead/prepare-sharded-chain.mjs` | nothing live (Oct 11) | dead | retire to held/ | — | — |
| 44 | `$JET_STATE_ROOT/cheap.sh`, `cheap*.lock`, `cheap-heavy.pause` | check-runtime.mjs, safety-schedule.mjs: check slots | tooling | `Tools/agent/lane-check.sh` slots | STATE | — |
| 45 | `$JET_SCRATCH_ROOT/held/<date>/…`, `held/LOG.tsv` | AGENTS.md "Never delete work" | evidence | `$JET_SCRATCH_ROOT/held` | SCRATCH | — |
| 46 | `$JET_STATE_ROOT/proofq/{queue,PAUSE,bin/pq-preflight.mjs}` | safety-schedule, safety-gate-job, s5/make-jobs | tooling/state | `$JET_STATE_ROOT/proofq`; pq-preflight is the dead preflight family | STATE | — |
| 47 | `$JET_STATE_ROOT/scratch/RtCrypto1/…` frozen inputs | `Tools/agent/s5/stage-frozen-inputs.mjs` origins | evidence | recorded origins; the sha256 pins the bytes (the char Debug and formatter goldens are tracked under `Compiler/JetRuntime/Tests`) | — | — |

## Committed files that hard-code a machine path

Found with `git grep -E '$JET_SCRATCH_ROOT|\$JET_STATE_ROOT|$JET_RUN_ROOT'` outside
`Docs/audits` and `Docs/research` (about 270 files). The `lane-check.sh` guard
rejects new ones; the allowlist covers only `Tools/agent/jet-env` and the
evidence rows marked **keep**.

| # | Files | What the path is | Kind | Fix |
|---|---|---|---|---|
| 48 | `AGENTS.md` (held paths, preflight) | rules | doc | `$JET_SCRATCH_ROOT/held`, census preflight |
| 49 | `.agents/skills/{improve-codebase-architecture,mine-for-jet-v2,persona-audit}/…` (5) | scratch default | doc/tooling | `$JET_STATE_ROOT/scratch` |
| 50 | `Tools/tower/skills/tower-ballot/{SKILL.md,references/exemplar.md}`, `Tools/tower/test/hardening.test.mjs` | ballot draft path, test scratch | doc/test | `$JET_STATE_ROOT` |
| 51 | `Compiler/JetRuntime/Tests/check-runtime.mjs` | `cheap.sh` | tooling | `lane-check.sh` slots |
| 52 | `Compiler/JetRuntime/Tests/CryptoEd25519SignatureFixtures.json`, `tests/conformance/corpus/core/crypto/{aes,ct}/fixtures.json` | oracle rlib provenance | evidence | `$JET_TARGET_ROOT`-relative path; sha256 unchanged |
| 53 | `Tools/agent/{proof-source,verify-full,verify-full-collect-regression}.mjs`, `proof-source.test.mjs` | candidates/targets roots | tooling | `JET_SCRATCH_ROOT`, `JET_TARGET_ROOT` |
| 54 | `Tools/agent/release-compiler.mjs` | chain.sh, build-slot.sh, release work | tooling | Chain.jet, Slot.jet, `JET_STATE_ROOT` |
| 55 | `Tools/agent/{safety-schedule.mjs,safety-gate-job.sh,safety-gate.test.mjs}` | proofq, SafetyNet8 scratch, cheap.sh | tooling | root vars, lane-check slots |
| 56 | `Tools/agent/s5/{make-jobs.sh,stage-frozen-inputs.mjs,parked-modes.tsv}` | candidates, proofq, frozen origins | tooling/evidence | root vars; parked-modes.tsv **keep** (records) |
| 57 | `Tools/agent/cache/*` (6) | cache layout | tooling | `JET_STATE_ROOT` |
| 58 | `Tools/agent/{lane-check.sh,run-feature-examples,compiler-diff(.test),compiler-registries,core-coverage-census.test,gen-runtime-fault-report,goldens-census,jet-state.test}.mjs` | scratch default | tooling | `JET_STATE_ROOT` |
| 59 | `Tools/testkit/run.jet` and 5 `Tools/testkit/Tests/*.jet` | scratch default | tooling/test | `JET_STATE_ROOT` |
| 60 | `Jetpack/Bootstrap/{check.sh,assemble.mjs}` | jetpack-bootstrap cache | tooling | `JET_STATE_ROOT` |
| 61 | `Jetpack/*/Tests/*.jet` (10) | test scratch roots | test | `JET_STATE_ROOT` |
| 62 | `Tools/ci/{AgentWorkloads,CompiledWorkloadPeerLauncher}.jet`, 3 `Tools/ci/*.sh` | scratch default | tooling | `JET_STATE_ROOT` |
| 63 | `Tools/perf/**` (about 30) | scratch and targets | tooling/evidence | root vars; recorded results **keep** |
| 64 | `Tools/canvas-test/{native-lifecycle,scenario}.mjs`, `Tools/compiler-modules/{inventory,migrate-examples}.mjs`, `Tools/jetpack-infra/test-toolchain-update.sh`, `Tools/agent-eval/run-cold-context.mjs` | scratch/cache default | tooling | `JET_STATE_ROOT` |
| 65 | `Tools/agent-eval/ffi-assurance/manifest.json` | `resume-implementation/ffi-ratified.json` input | config | commit the input or cite the Tower decision |
| 66 | `Tools/tower/.tower/{tower.json,history.json}` | board text quoting paths | evidence | **keep** (board data; Tower-owned) |
| 67 | `Tools/agent-eval/domain-foundations/**` (about 170) | recorded probe transcripts | evidence | **keep** |
| 68 | `Tools/gauntlet/performance-surface-evidence.json` | recorded measurements | evidence | **keep** |
| 69 | `Docs/proposals/*` (7), `Docs/spec/contributing/examples.md`, `Examples/features/ports/murmur3_x86_32/provenance.md`, `Examples/ffi/workflows/driver-build/run.sh` | proposal evidence, instructions | doc/evidence | instructions to root vars; proposal quotes **keep** |
| 70 | `tests/agent_workloads/prompts/GENERATOR.mjs`, `tests/guards/agent_workloads_reviewed.jet`, one `tests/cases/web_dev/*.case` comment | output dir, scratch | tooling/test | `JET_STATE_ROOT` |

## Core gaps

| Gap | Card | Blocks |
|---|---|---|
| Bounded parallel worker pool: N jobs, per-job timeout, collected exit status, deterministic order, cancellation on parent exit | #5072 c2 | parallel prescreen in `Land.jet serve` (row 1); sharded `Suite.jet` (row 36). Both run serially until it lands. |

Further gaps go into this table as the ports find them.

## Open questions for the owner

1. Seed for a remote fresh clone. D-SEED-DIST1 chose A (publish the proven
   compiler as the seed), but nothing is published yet. Until then a fresh clone
   needs a copy of a `discovery/base/<sha>` dir supplied as `JET_SEED`. Where is
   the seed published (release asset, URL), so docs can name one fetch command?
2. Evidence that records a path (rows 56, 63, 66-69) stays as written; the guard
   allowlists those files rather than rewriting history.
