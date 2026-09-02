# Records: one index, one identity, safe capture by default in dev

Status: proposal, 2026-09-01, revised after the review passes; element 6 of `whole-language-frame.md`. Independently adoptable; strongest with `rights-one-row.md`. Ballots: D-RECORD1 (recommended B), D-RECORD-SPELL1 (D), D-TIMETRAVEL2 (B). Nothing here is implemented; transcripts marked illustrative do not run today.

## Executive summary

Jet already ratified the artifacts that matter and their laws: `check`, `build`, `test`, `prove`, and `budget check` consult one content-addressed receipt store (`jet-receipt-v2`) before doing work, keyed by input bytes and invocation context (D-DEVR-TWICE1), and `jet status` shows what the project has proved from those receipts (D-DEVR-STATUS1); `jet prove --capture` writes a replay and `--replay` consumes it, with a privacy split between the safe form that records only the clock and the sensitive form that needs an explicit flag and typed consent (D-PROVE-REPLAY1=A, D-JREPLAY1); a proof artifact has its own codec and lifecycle (D-JPROOF1). The audit's first draft proposed one container and full capture by default; the rival-family review showed that this erased two ratified codecs and the privacy split. The design changed.

What is actually missing is an index and a default. Nothing lists the artifacts of a target in one place with one identity, so a receipt cannot say which replay it was proved against. And `jet dev` records nothing unless `--record=NAME` is passed, so the moment a beginner most needs a replay, after a crash in a live session, there is usually nothing to replay. And no command rebuilds from a receipt's inputs and compares bytes. The proposal adds one index under `.jet/records`, one identity key shared by every artifact, and safe capture on by default in `jet dev` within a project retention budget. Every ratified artifact keeps its bytes and codec; the sensitive form stays flag-gated; release never captures without the flag.

Score: mechanisms deleted 0; capabilities kept all; capabilities gained 3 (the index with one identity, dev-tier safe capture by default, `jet build --verify` on any indexed record).

| today | proposed | ballots |
|---|---|---|
| separate artifacts and no index; dev writes no record | one index and identity; safe capture by default in dev | D-RECORD1 |
| `jet prove --replay` consumes an artifact path | the ratified consumer accepts an index id and `--keep` | D-RECORD-SPELL1 |
| forward-only replay | backward stepping after a measured gate | D-TIMETRAVEL2 |

## The problem

| defect | evidence |
|---|---|
| no index | `.jet/build-receipts`, `.jet/replays`, `.jet/proofs`, the trust store, and `~/.cache/jet/runtime` are separate directories with separate identity keys (`syntax-decisions.md:6800-6850`; `BuildCache.rs:1-30`) |
| a receipt cannot name its replay | `Source/ProductionReceipt.rs:20-35` has no cross-link field |
| dev captures only by opt-in | `--record=NAME` on run, dev, and test (`crates/jet-cli/src/CLI.rs:1556`); `Source/CmdDevTools.rs:369-370` prepares the receipt context and captures only when a name is given |
| no verify | no command rebuilds from a receipt's inputs and compares bytes; `--verify` does not exist in `crates/jet-cli/src/CLI.rs` |
| the cache is a private receipt | `~/.cache/jet/runtime` entries keyed by generated-Rust hash, not by receipt identity (`RuntimeCache.rs`) |

Lane D: "no observed automatic release/promotion edge from proof artifact to `jet build`." Lane D also verified that generated Rust bytes are repeatable for the same source and that the runtime cache is bounded to 512 MiB.

## The proposal on the page

Real today (source-verified at `8b9933668` against the CLI registry and the receipt store; the verification lane did not execute these commands):

```text
$ jet check app.jet                      # consults the jet-receipt-v2 receipt store before doing work (D-DEVR-TWICE1; Source/ReceiptStore.rs:1-32)
$ jet status                             # shows what the project has proved by reading receipts (D-DEVR-STATUS1; crates/jet-cli/src/CLI.rs:832-836)
$ jet run --record=crash app.jet         # opt-in named replay capture on run, dev, and test (crates/jet-cli/src/CLI.rs:1556)
$ jet prove app.jet --capture            # ratified safe capture → .jet/replays/<id>.jetproof-replay (Time only; D-JREPLAY1)
$ jet prove app.jet --replay .jet/replays/<id>.jetproof-replay
```

Proposed (illustrative): the same commands, plus one index and one default.

```text
$ jet dev app.jet
live  app.jet  capture: safe (clock only)  budget 256 MB, 200 records
saved app.jet  swapped run  kept world
run   app.jet  R0802 division by zero at run.jet:12  replay recorded c10aee50
$ jet prove app.jet --replay c10aee50
… deterministic re-run of the crash …
$ jet inspect build app.jet
identity 9f3a…  (inputs sha256, tool jet 1.0.0, engine dev-tir-v1)
records
  c10aee50  replay   safe   dev-tir-v1   14 clock events   outcome R0802   2026-09-01 20:14
  4b21f0e3  proof    aot-native-v1      consumed replay c10aee50           2026-09-01 20:20
  a77d2c19  receipt  release            verified identical                 2026-09-01 20:25
$ jet build --verify a77d2c19
identical: 4 inputs, 1 output, sha256 match
```

A program that reaches randomness, raw input, or the network under `jet dev` is not captured by the safe form; the run line says `capture: skipped (Rand at run.jet:9); use --capture-sensitive` and the sensitive form stays exactly as ratified: explicit flag, typed consent, mode-0600 file.

### The index

| field | meaning |
|---|---|
| identity | target inputs sha256, tool version, engine name (one key for every kind) |
| kind | `receipt`, `replay`, `proof`, `evidence` (from element 2), `trust` |
| consumed | the identity of the record this one used, when any |
| produced | the identity this one yielded |
| capture | `safe` or `sensitive`; sensitive records are never listed by an unauthenticated `inspect` |
| budget | project retention: 256 MB and 200 records by default, oldest safe replays pruned first; sensitive records are never auto-pruned; settable in `package.jet` under `dev: .{ records: .{ budget: … } }` |

The trust store keeps its own file and law (D-AUTHORITY-MANIFEST1); the index lists it, and nothing else changes about it.

## Rungs

| rung | who | spelling | what they get |
|---|---|---|---|
| 0 | beginner | `jet dev app.jet` | a crash is replayable by id, from the safe form, with no flag |
| 1 | intermediate | `jet inspect build app.jet` | every record of the target, one identity, cross-linked |
| 2 | expert | `jet build --verify <id>` (proposed) | rebuilds from an indexed receipt's inputs and reports identical bytes or the first differing input |
| 3 | expert | `jet prove --capture-sensitive` | the ratified sensitive form with consent |
| 4 | expert | `dev: .{ records: .{ budget: .{ size: "256 MB", count: 200 } } }` in `package.jet` | the retention budget |
| 5 | expert | `dev: .{ records: .{ capture: off } }` | no dev capture at all |

Rung 0 is untouched by every rung above it.

## Three exits

| exit | spelling |
|---|---|
| see | `jet inspect build <target>` lists every record with its identity and links |
| write | `--record=NAME`, `--capture`, `--capture-sensitive`, `--replay <id>` as ratified |
| refuse | `dev: .{ records: .{ capture: off } }`; `jet dev --no-capture` for one session |

## Decisions

### D-RECORD1 — the record model

| option | what |
|---|---|
| A | one container format for every artifact: one versioned file format embeds the ratified proof JSON and the ratified replay frames as payloads; rejected: a new container reopens two ratified codecs and the closed extension family (D-ARTIFACT-EXT1) for no gain the index does not give |
| B (recommended) | every artifact keeps its ratified bytes and codec; one index under `.jet/records` with one identity key and cross-links; `jet dev` captures the safe form by default within a project retention budget and says why when it cannot; release never captures without the flag; a new `jet build --verify` rebuilds from the receipt inputs and reports identical bytes or the first differing input |
| C | keep the files as they are and add cross-references: each artifact gains a link to the others; nothing captures by default; rejected: links inside the files change their identity, and nothing is captured when it matters |

Amends: D-DEVR-TWICE1 (identity key and `consumed` link on the receipt), D-JREPLAY1 (dev-tier default for the safe form only; the E3627 refusal stays), D-DEV4 (dev prints the capture line). D-PROVE-REPLAY1's safe and sensitive forms are unchanged.

### D-RECORD-SPELL1 — the consumer spelling

| option | what |
|---|---|
| A | `.jetrec` files with `jet test --replay` and `jet test --keep`; rejected: a new extension reopens a closed artifact family and moves the consumer off the ratified command |
| B | a new top-level command replays; files unchanged; rejected: a second word for one job |
| C | `jet run --replay <file>`; rejected: the ratified consumer is `jet prove --replay` |
| D (recommended) | `jet prove TARGET --replay ARTIFACT` stays the only consumer and accepts an index id; `jet prove TARGET --keep ARTIFACT` copies the replay under `.jet/records/kept/<id>` and lists it in the index as a claim named `replay <id>`; every `jet prove` of that target replays kept claims first; when the source or build identity no longer matches, the claim reports `stale` with E3621 and the fix names `--unkeep <id>` or a fresh capture; two keeps of one artifact are one claim |

Amends: D-JREPLAY1 additively (id lookup, kept claims); D-PROVE-SEM1 (kept replays run first). No extension changes.

### D-TIMETRAVEL2 — stepping backward

| option | what |
|---|---|
| A | `jet debug --replay` steps forward and backward over recorded clock events through the existing debug adapter, re-executing deterministically between them |
| B (recommended) | replay stays forward-only through `jet prove --replay`; backward stepping is carded when three measured facts exist from one shipped release: safe replays capture by default in dev, replay divergence (E3623) stays under one in a thousand recorded runs, and `jet debug` passes its adapter conformance suite |
| C | replay stays forward-only forever |

Amends: nothing; D-TIMETRAVEL1=C is restated with its gate made measurable.

## What stays

| kept | why |
|---|---|
| the `jet-receipt-v2` receipt store and `jet status` | one content-addressed receipt store and status view |
| `--record=NAME` on run, dev, and test | opt-in named replay capture |
| `jet prove --capture` | ratified safe form |
| `jet prove --capture-sensitive` | explicit flag and typed consent |
| `jet prove --replay` | only consumer |
| `.jetproof` | proof artifact codec |
| `.jetproof-replay` | replay frames |
| the build receipt codec | ratified bytes and codec |
| the runtime cache and its bound | bounded to 512 MiB |
| the trust store | its own file and law |
| release-tier privacy | release never captures without the flag |
| mode-0600 sensitive files | sensitive records |

## Implementation shape

| phase | work |
|---|---|
| A | the index with the shared identity key; `consumed` and `produced` links on the receipt and proof codecs under a version bump; `jet inspect build` lists the index |
| B | a new `jet build --verify <id>` that rebuilds from an indexed receipt's inputs and compares bytes |
| C | after D-RECORD1: dev-tier safe capture with the budget and the run line; after D-RECORD-SPELL1: id lookup, `--keep`, `--unkeep`, and the E3621 stale row; after D-TIMETRAVEL2: nothing until the three measured facts exist |

## Where Jet loses today

| peer | what it does better | Jet today |
|---|---|---|
| Nix | one content-addressed store with one identity | receipts and replays with no store between them |
| Bazel | one content-addressed store with one identity | receipts and replays with no store between them |
| rr | records a crash by default in its dev workflow | receipts and replays with no store between them |

## Strongest unverified assumption

That the safe replay form (clock only) is enough to reproduce most dev-tier crashes. The ratified privacy split is deliberate; a crash that depends on randomness or input will show as `capture: skipped` with its reason, and the card measures how often that happens on the example corpus before the default is judged.

| assumption | how it is proved | where |
|---|---|---|
| the safe replay form (clock only) is enough to reproduce most dev-tier crashes | the card measures `capture: skipped` and its reason on the example corpus | card #2506 |
