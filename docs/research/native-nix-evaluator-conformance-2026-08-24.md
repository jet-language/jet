# Card #2162 — Native Nix evaluator conformance

Date: 2026-08-24. Card: #2162.

## Result

The current evaluator is a bounded flake projection. It is not a nixpkgs
evaluator. This report records the measured boundary, the reuse posture, and
the work needed for real conformance.

The current repository has no whole-nixpkgs attribute corpus, no published
per-attribute differential ledger, and no evaluated graph or closure result.
The five card criteria therefore remain open. The existing dogfood index path
must stay separate from this evaluator program.

## Plan of record

1. **Freeze reuse and licence policy.** Keep GPL Snix/Tvix evaluator code out
   of Jet's shipped crates until the owner and legal review approve a separate
   GPL distribution. Use MIT protocol definitions only when the exact files
   carry that licence. Use Snix as an external oracle and design reference.
2. **Define one corpus identity.** Pin Nix version and source commit, nixpkgs
   revision and NAR hash, system, attrpath, source digest, and oracle mode.
3. **Replace symbolic package lookup.** Evaluate the pinned nixpkgs source
   graph. `pkgs.<name>` must resolve through real Nix expressions, not become
   the string `<name>`.
4. **Return graph identity.** For every supported attr, record the derivation
   path, every named output, direct references, transitive closure digest, and
   unsupported effects. Do not admit an overlay or custom flake without these
   facts.
5. **Run differential batches.** Compare Jet and Nix for every inventory attr
   on every supported system. Classify each result as `match`, derivation
   mismatch, output mismatch, graph mismatch, unsupported, Nix error, or
   missing source.
6. **Publish and gate claims.** Publish one immutable report per
   `(nix_version, nixpkgs_revision, system)`. Remove an index-only limitation
   only after the matching graph and closure rows pass.

This is the derived plan because `docs/plans/jetpack-dogfood/` has plans for
cards #2155–#2160 and #2164, but no plan for #2162. The plan follows step 7 of
`docs/audits/jetpack-native-nixpkgs-2026-08-24.md`.

## Snix and Tvix reuse posture

### Proposed decision

Do not embed or link `snix-eval`, `tvix-eval`, `snix-glue`, `nix-compat`, or
other Snix/Tvix implementation crates into Jet's dependency-free evaluator or
Jetpack. Keep the native Jet implementation under its current no-`std`,
no-unsafe, no-external-dependency boundary.

Use these projects in three narrow ways:

- run a separately licensed Snix binary in an off-device conformance job;
- read Snix source and tests as implementation prior art;
- reuse only protocol-buffer definitions whose source file and licence grant
  are MIT.

The Snix repository states that its crates are GPL-3.0 and that direct use,
including linking or embedding the evaluator, falls under GPL-3.0. It gives the
MIT exception to protocol-buffer definitions for independent implementations.
[Snix licence structure](https://git.snix.dev/snix/snix) records this split.
Tvix records the same split in its repository README.
[Tvix licence structure](https://github.com/tvlfyi/tvix/)

The owner/legal gate is still open. This report records a recommendation, not
owner ratification. A direct-reuse option needs a separate GPL package boundary,
licence review, source notices, and distribution decision. It must not enter
`Cargo.toml` by implication.

Snix itself reports that its APIs are unstable and that it lacks tooling to
diff all of nixpkgs against Nix/Lix. Its builtin table also marks store,
context, and unfinished builtin work. Snix is useful prior art and an oracle,
not evidence that Jet can ship a drop-in evaluator today.
[Snix eval-store issue](https://git.snix.dev/snix/snix/issues/162),
[Snix builtin status](https://snix.dev/docs/components/eval/builtins/)

## Measured coverage

### Pinned evaluator anchor

| Field | Current value | Meaning |
|---|---|---|
| Nix version | `2.34.8` | Pinned reference identity |
| Nix source commit | `f3f1c3c5b8ad91850e0f7c590cf177f7ab022024` | Pinned reference source |
| nixpkgs revision | `b5aa0fbd538984f6e3d201be0005b4463d8b09f8` | Pinned fixture source |
| nixpkgs NAR hash | `sha256-oPXCU/SSUokcGaJREHibG1CBX3+s/W7orDWQOZDsEeQ=` | Pinned source identity |
| systems | 4 | `aarch64-darwin`, `aarch64-linux`, `x86_64-darwin`, `x86_64-linux` |
| whole-nixpkgs attr rows | **0** | No attr inventory is in `oracle.json` |

Source: `tests/fixtures/nix-compat/oracle.json:1-35`.

### Bounded fixture corpus

| Corpus | Rows | Native checks | What it proves |
|---|---:|---:|---|
| `stage-a.json` | 9 | 9 | Pinned values, one error, and one lock projection |
| `stage-a-authority.json` | 1 | 1 | One explicit project-import authority case |
| `stage-a-derivation.json` | 4 | 4 | Pure derivation requests and rejection |
| `breadth.json` | 14 | 14 × 7 = 98 seeded variants | Bounded projection stability against committed expected values |
| inventory | 17 semantic rows | 15 covered, 2 skipped | Evaluator feature inventory, not nixpkgs attr coverage |

The inventory classifies 10 rows as `evaluable`, 5 as `buildable`, and 2 as
skipped. The two explicit skips are dynamic derivations and import-from-
derivation (`tests/fixtures/nix-compat/pinned-inventory.json:6-24`).

The 44 passing `jet-nix-eval` unit tests validate this fixture contract. They
do not run a whole-nixpkgs source graph. `breadth.json` stores Nix expressions
and expected values; the Rust test compares Jet with those committed values.
It is not a fresh Nix run and must not be reported as nixpkgs coverage.

### Per-revision differential ledger

The only pinned revision currently has no whole-nixpkgs differential run:

| Nix version | nixpkgs revision | system | inventory attrs | Jet evaluated | exact matches | mismatches | unsupported | status |
|---|---|---|---:|---:|---:|---:|---:|---|
| `2.34.8` | `b5aa0fbd538984f6e3d201be0005b4463d8b09f8` | all 4 pinned systems | 0 | 0 | 0 | 0 | 0 | `not-measured: inventory absent` |

This row is an honest zero-denominator baseline, not a pass. The next report
must replace it with one row per supported system and non-zero inventory
counts. A zero count must mean an empty evaluated corpus only when the report
also names the source inventory digest.

### Repository demand

`env.jet` declares 20 nixpkgs package attributes
(`env.jet:7-15`). The current `.jet/lock` contains one `cargo` record and no
whole-repository nixpkgs index (`.jet/lock:1-14`). The signed index client and
producer exist, but no committed index target proves these 20 attrs, and the
provider consumer is not yet wired. Therefore current evaluator identity
coverage for this repository is **0/20**, not 20/20.

The index producer's differential command compares staged index records with a
staged Nix oracle and writes only `records_compared`, `mismatches`, and
`status` (`crates/jetpack/src/bin/jetpack-nix-index.rs:167-227`). That is useful
for index publication, but it does not compare Jet evaluation with Nix and does
not prove overlays, custom flakes, or closure identity.

## Unsupported semantics

### Covered with proof

- bounded lexer/parser forms used by the fixture corpus;
- lazy `let` bindings, functions, attrsets, lists, equality, merge, and
  bounded string contexts;
- project-relative imports through explicit authority;
- synthetic package overlays with bounded `final`/`prev` behavior;
- explicit fixed-output fetch authority;
- explicit cross-system and external-flake authority;
- pure derivation request extraction and Nix store-path calculus;
- input, token, expression, import, string, memory, and latency limits.

These are the current bounded surfaces. The root environment injects `pkgs` and
`legacyPackages` as `PackageNamespace` values
(`crates/jet-nix-eval/src/Evaluator.rs:1154-1197`).

### Worth checking next

- real nixpkgs `default.nix` and `lib` import graph;
- `callPackage`, overlays, overrides, `rec`, recursive attrsets, and
  `__functionArgs` behavior;
- string contexts, `getContext`, path contexts, `toString`, and output
  dependencies;
- full builtin set, especially `derivation`, `derivationStrict`, fetchers,
  store reads, and evaluator/store effects;
- flake locks, input following, `getFlake`, local inputs, registries, and
  system selection;
- multi-output derivation graphs, direct references, transitive closure, and
  canonical closure digest;
- error parity, laziness parity, traces, source positions, and resource-limit
  behavior.

### Missing

- real nixpkgs evaluation;
- arbitrary overlays and custom flakes;
- import-from-derivation;
- dynamic derivations;
- builder execution for uncached outputs;
- a native evaluated graph and closure result;
- a whole-nixpkgs attr inventory joined to Jet results;
- per-revision differential reports with mismatch lists;
- a product gate that prevents identity claims before graph and closure parity.

Nix evaluation is lazy and call-by-need, so a corpus must compare forced
results and errors, not only parse success.
[Nix evaluation](https://nix.dev/manual/nix/2.32/language/evaluation.html)
Derivation evaluation can create `.drv` and output identities as part of
evaluation, so a package row must compare those identities.
[Nix derivations](https://nix.dev/manual/nix/2.29/language/derivations)
Import-from-derivation pauses evaluation to realize a store object before
reading it, so it needs a separate effect and closure gate.
[Nix IFD](https://nix.dev/manual/nix/2.19/language/import-from-derivation)

## Required differential artifact

The producer should emit one canonical JSON report per revision and system:

```text
schema
nix_version
nix_source_commit
nixpkgs_revision
nixpkgs_nar_hash
system
inventory_digest
records_total
matched
drv_mismatch
output_set_mismatch
output_path_mismatch
graph_mismatch
unsupported
nix_error
missing_source
rows[]
```

Each `rows[]` item must contain the exact attrpath, Jet result or error class,
Nix result or error class, derivation path, named outputs, graph digest, and
closure digest. Hash the canonical report. Keep the full row list beside the
summary; counts without rows cannot debug a regression.

For repository acceptance, the corpus must include all 20 `env.jet` attrs for
the locked system before the first dogfood claim. The row gate is:

```text
Jet evaluation == Nix evaluation
Jet drvPath == Nix drvPath
Jet named outputs == Nix named outputs
Jet direct references == Nix direct references
Jet transitive closure digest == Nix closure digest
```

An unsupported result is not a match. An index record is not a match. A store
path string extracted from `pkgs.<name>` is not a match.

Proposed future implementation names, not shipped API: `NixEvalCase`,
`NixEvaluationIdentity`, `NixEvaluatedGraph`, and `DifferentialCounts`. Add no
Rust type until the owner approves the graph and closure contract.

## Overlays and custom flakes

The existing overlay and external-flake tests prove that the bounded evaluator
can project selected shapes. They do not prove nixpkgs evaluation or closure
identity. The evaluator must not admit either surface until it can return the
same graph and closure facts as Nix for a pinned input set.

Admission rule:

1. Pin every flake input and overlay source.
2. Evaluate the same source graph with Nix and Jet.
3. Compare every selected derivation, named output, direct reference, and
   transitive closure digest.
4. Reject the selection on any mismatch or unsupported effect.

This keeps the current synthetic tests useful without treating them as
production support.

## User-facing claim policy

Remove no index-only limitation claim in this pass. The current code and audit
correctly describe a bounded, non-executing projection. The `overlays` and
`external-flakes` inventory rows mean bounded fixtures with explicit authority;
they do not mean arbitrary nixpkgs overlays or custom flakes.

Change user-facing text only after a differential report has non-zero matching
rows for that surface and the graph/closure gate passes. Until then, say
“bounded native projection” and name the unsupported surface.

## Standing lens

### How Jet can win

Jet can make evaluator results safe, bounded, signed, and auditable in the
client. A no-`std`, no-unsafe evaluator with explicit authority can reject
ambient store, network, and process access before evaluation. Nix remains
stronger on mature nixpkgs reach until Jet completes the corpus.

### What Jet must avoid

| Mistake | Evidence | Jet exposure |
|---|---|---|
| Treating `pkgs.fd` as evaluation | `PackageNamespace` returns a package identity | False nixpkgs coverage and wrong derivation identity |
| Counting fixtures as nixpkgs attrs | 17 semantic rows but 0 attr rows | Misleading percentages |
| Treating index equality as evaluator equality | Index producer compares staged records only | Overlays and custom flakes remain uncovered |
| Embedding GPL code casually | Snix/Tvix licence split | Distribution and legal risk |
| Removing limitation text early | No graph or closure rows | User sees support that cannot work |

Jet is structurally safer when it keeps authority explicit and fails closed.

### AI-driven development

The useful unit is one canonical attr row. A row gives an agent an exact
source revision, input, mismatch class, and repair target. This improves
verdict fidelity, actionability, context economy, and repair determinism. The
current fixture suite has good local latency but no whole-tree verdict.

### Strongest unverified assumption

The largest assumption is that a bounded evaluator can grow into nixpkgs
fidelity without changing the current package identity and authority seams.
The differential graph prototype must test this before more surface work.

## Criterion status

1. **NOT MET** — recommendation recorded; owner/legal decision on GPL reuse and
   MIT protocol use remains open. Evidence: Snix/Tvix licence links above.
2. **NOT MET** — no whole-nixpkgs per-revision differential counts exist.
   Evidence: `oracle.json` has no attr records; the published baseline is
   explicitly `not-measured`.
3. **NOT MET** — 0/20 repository attrs have native evaluated derivation/output
   identity rows. Evidence: `env.jet:7-15`, `.jet/lock:1-14`, and no committed
   index target.
4. **NOT MET** — overlay and custom-flake tests are bounded shape projections;
   no evaluated graph or closure agreement exists.
5. **NOT MET** — no limitation claim was removed because no matching graph and
   closure corpus supports removal.

This report changes no evaluator, provider, index, Tower, or user-facing
product code. It publishes the measured boundary and the closeout contract.
