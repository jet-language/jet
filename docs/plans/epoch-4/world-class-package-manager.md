# Epoch 4 — world-class Jetpack package manager

**Status:** executable master plan. **Audit date:** 2026-07-09.
**Scope:** Jetpack only. JetOS consumes this substrate in Epoch 7; OS activation,
desktop modules, installers, and system services remain Epoch 7.

This plan supersedes any Epoch 4 note that treats a schema, fixture, policy
model, or deferred protocol as a shipped package-manager feature. Existing
ratified decisions remain law unless a ballot below explicitly amends one.

## Exit claim

Epoch 4 exits only when Jetpack has full Nix package-manager capability, native
Nix-package interoperability without an installed `nix` binary, and the best
compatible features from leading language and build package managers. “Full
parity” covers package management and build/store behavior, not NixOS module
breadth.

The product may claim parity only when all acceptance lanes in this document
pass against live stores, registries, caches, builders, hostile packages, and
independent machines. Fixture-only tests support development; they never close
a capability card.

## Current truth

Current Jetpack grade against the Nix package-manager bar: **C-**.

Strong foundations already exist:

- typed `pkg.jet`, `env.jet`, and `workspace.jet` surfaces;
- strict direct-dependency visibility and workspace catalogs;
- provider refs, channel intent, overlays, patches, semantic lock rationale;
- source-build recipes, toolchain records, offline mode, vendoring;
- services, secrets, discovery, migration import models, signing, SBOM;
- hangar metadata, basic GC/optimization, build logs, shell-on-fail;
- one-shot rootless default and Linux/macOS/Windows product intent.

JP0 stop-line now enforces three truth boundaries:

- cache reuse verifies output existence, current canonical digest, platform,
  exact normalized source/manifest/recipe/toolchain policy, signature policy,
  and canonical closure reachability through one realization boundary used by
  CLI and JetOS. Invalid Jet-owned candidates are quarantined and E2604 stops
  the command; repair is never silent. Unsigned reuse is limited to an exact
  Hangar-owned local output. Signed imports fail closed until an immutable
  in-process verifier ships. Nix compatibility outputs always re-enter Nix;
  Jetpack does not claim an early cache hit from spelling-only identity;
- every existing Nix compatibility output recorded in Hangar gets a durable
  `nix-store --add-root --indirect` root protecting its transitive closure;
- canonical output archives hash node type, mode, bytes, symlink target, empty
  directories, and complete hardlink identity; reject outside aliases, escapes,
  cycles, concurrent mutation, and special files. Local outputs are sealed
  read-only, then revalidated before and after child consumption. Sandbox
  capability detection stays fallback until a child actually enters a jail.

Production blockers after that stop-line:

- recipes still execute as ordinary host processes; every platform reports
  fallback/unsandboxed until JP3 supplies an enforced jail;
- native HTTP substitution/push, NAR/narinfo, mirrors, repair, remote builders,
  and remote execution do not exist;
- registry dependencies still stop at E1207; package authoring/publish metadata
  exists without complete consumer delivery;
- the current package graph and semantic lock layers contain substantial data
  models, but several are not one live resolver/build/store path;
- user package profiles/generations do not exist;
- Nix/flakes/nixpkgs still require installed Nix on product paths;
- current Git-index + author-TOFU trust does not defend first use, rollback,
  freeze, mix-and-match, or a compromised cache builder;
- the documented AST-only build-cache key is not a complete action identity.

These are integrity and isolation defects. They precede ecosystem breadth.

## Laws

1. **One graph.** Resolution, build, cache, audit, IDE, profiles, and JetOS
   consume the same typed dependency/action graph.
2. **Two identities.** Every build has a derivation/action digest over all
   declared inputs and a separate canonical output-content digest.
3. **No trust by location.** A path, mirror, registry, cache hit, or worker name
   never substitutes for verified bytes plus verified metadata.
4. **No host leakage.** Undeclared reads, writes, executables, environment,
   network, devices, and processes are denied by an OS boundary.
5. **Offline means syscalls.** Offline tests prove network denial at the OS
   boundary, not only absence of calls in a mocked transport.
6. **Source-backed mutation.** Adds, profiles, policies, overrides, and grants
   write reviewable Jet source or exact locks; no hidden dependency truth.
7. **Compatibility is not canon.** Jetpack may evaluate Nix/flakes internally,
   but Jet remains the only canonical package/build authoring language.
8. **No fixture closure.** Live protocol and hostile-system acceptance is part
   of each feature, never a later hardening pass.
9. **One beginner/expert mechanism.** Defaults are automatic and safe; expert
   controls expose graph, authority, platform, cache, scheduler, and proof facts
   without changing semantics.
10. **No deferred core protocol.** A cache envelope without substitution, a
    registry index without consumption, or sandbox status without confinement
    is incomplete.

## Nix parity matrix

| Capability | Nix bar | Jetpack audited state | Epoch closure |
|---|---|---|---|
| Package language | lazy evaluator produces derivations | typed Jet build/env model; installed Nix for Nix inputs | native compatibility evaluator plus one Jet action IR |
| Derivations | builder, args, env, inputs, platforms, named outputs | linear recipes plus separate build-plan models | every package path lowers to one finite action graph |
| Immutable store | objects, refs/referrers, atomic registration | metadata dirs plus some owned output trees | canonical Hangar v2 objects and closure DB |
| Addressing | derivation identity, fixed outputs, CA outputs | incomplete output hash; mixed meanings | separate complete action and canonical output digests |
| Verify/repair | verify, quarantine/repair through substituters | cache hit skips integrity proof; no repair | verify every trust boundary and repair/fallback |
| Substitution | ordered caches, miss builds locally | envelope only | native read/write cache plus NAR adapter |
| Cache trust | signed store metadata and trusted keys | empty cache signature slot; author TOFU | threshold metadata, builder provenance, rotation/revocation |
| Sandbox | enforced filesystem/process/network isolation | policy/status only; ordinary child execution | real Linux/macOS/Windows isolation backends |
| Remote builds | heterogeneous builders and distributed scheduling | scheduler model only | verified remote action execution and failover |
| Flakes/locks | transitive inputs, follows, registries, selective update | native semantic locks; shallow foreign bridge | no-Nix flake evaluator/import and one semantic lock |
| Profiles | atomic per-user generations and rollback | project envs only | source-backed named package profiles |
| GC | closure roots and generations protect transitive objects | nearest project lock and age-based metadata GC | root/lease graph, why-live, crash-safe mark/sweep |
| Cross compilation | build/host/target roles | platform facts, host envelope | typed roles and variant-aware resolution/action keys |
| Explain | derivations, logs, why-depends, diff closures | good rationale/log substrate | file-edge closure, cache, rebuild, trust, GC explanations |
| Multi-user | secure shared store/build identities | per-user one-shot process | owner-selected optional shared-store architecture |
| Nixpkgs access | evaluator, `.drv`, NAR caches, local builds | installed-Nix shell-outs | native compatibility pipeline and differential corpus |
| Dynamic planning | IFD/dynamic derivations | absent | owner-selected finite typed staged planning or explicit rejection |

Primary Nix references:

- [Nix store and package-manager model](https://nix.dev/manual/nix/stable/)
- [Derivations](https://nix.dev/manual/nix/2.28/store/derivation/)
- [Store object information and cache protocol](https://nix.dev/manual/nix/2.34/protocols/json/store-object-info.html)
- [Profiles and generations](https://nix.dev/manual/nix/2.32/package-management/profiles)
- [GC roots](https://nix.dev/manual/nix/latest/package-management/garbage-collector-roots)
- [Distributed builds](https://nix.dev/tutorials/nixos/distributed-builds-setup.html)
- [Flakes](https://nix.dev/concepts/flakes.html)
- [Nixpkgs and cross compilation](https://nixos.org/manual/nixpkgs/stable/)
- [`why-depends`](https://nix.dev/manual/nix/2.34/command-ref/new-cli/nix3-why-depends)

## Features to transplant, not imitate blindly

### Resolution and workspaces

- PubGrub causal conflict proofs and smallest fixes.
- Cargo resolver separation for build/dev/target features.
- Gradle-style producer/consumer variants, constrained to typed axes.
- Conan/vcpkg build, host, target, compiler, runtime, linkage, and ABI identity.
- uv/Bundler/Conan multi-platform locks and conservative targeted updates.
- uv `latest`, `lowest`, and `lowest-direct` verification matrices.
- Go graph pruning, lazy metadata loading, checksum transparency, and `why`.
- pnpm strict dependency visibility, content dedup, release-age safety, and
  exact install-script approval.
- Yarn source constraints and safe autofix.
- Cargo sparse metadata with conditional requests.
- coherent provider baselines so one update sees one package universe.

### Store, build, and cache

- Nix/Guix complete derivation identity and closure roots.
- Bazel hermetic per-action sandboxes, local/remote cache symmetry, and remote
  execution protocol discipline.
- trusted-CI shared-cache writes; developer writes stay private.
- lazy intermediate materialization from remote CAS.
- Guix-style independent rebuild challenge.
- Homebrew relocatability facts and source fallback.
- OCI subject/referrers for signature, SBOM, provenance, and reproducibility
  proof artifacts.

### Supply-chain security

- TUF offline threshold root, delegated targets, snapshot/timestamp freshness,
  consistent snapshots, rollback/freeze/mix-and-match defenses.
- Sigstore identity-bound ephemeral signing and transparency bundles where
  public identity is appropriate; Ed25519/KMS/HSM for private/offline flows.
- SLSA provenance policy over builder identity, source, inputs, and process.
- NuGet package-source mapping against dependency confusion.
- pnpm trust-evidence no-downgrade, transitive exotic-source blocking, and
  maturity windows.
- typed credential providers; secrets never enter URLs, logs, or argv.

Primary ecosystem references:

- [Cargo resolver](https://doc.rust-lang.org/nightly/cargo/reference/resolver.html)
- [pnpm security settings](https://pnpm.io/settings)
- [pnpm build-script approval](https://pnpm.io/cli/approve-builds)
- [Yarn strict dependency graph](https://yarnpkg.com/features/pnp)
- [Go modules](https://go.dev/ref/mod)
- [Gradle variants](https://docs.gradle.org/current/userguide/variant_aware_resolution.html)
- [uv resolution](https://docs.astral.sh/uv/concepts/resolution/)
- [Conan package identity](https://docs.conan.io/2/reference/conanfile/methods/package_id.html)
- [Maven dependency mechanism](https://maven.apache.org/guides/introduction/introduction-to-dependency-mechanism.html)
- [NuGet central package management](https://learn.microsoft.com/en-us/nuget/consume-packages/central-package-management)
- [SwiftPM package security](https://docs.swift.org/swiftpm/documentation/packagemanagerdocs/packagesecurity/)
- [Bundler lock and platforms](https://bundler.io/man/bundle-lock.1.html)
- [vcpkg manifest mode](https://learn.microsoft.com/en-us/vcpkg/concepts/manifest-mode)
- [Homebrew bottles](https://docs.brew.sh/Bottles)
- [Guix substitutes and reproducibility](https://guix.gnu.org/manual/en/guix.pdf)
- [Bazel hermeticity](https://bazel.build/concepts/hermeticity)
- [OCI artifact manifests](https://github.com/opencontainers/image-spec/blob/main/manifest.md)
- [TUF specification](https://theupdateframework.github.io/specification/latest/)
- [Sigstore](https://docs.sigstore.dev/about/overview/)
- [SLSA build track](https://slsa.dev/spec/v1.2/build-track-basics)

Rejected anti-patterns: npm ambient lifecycle scripts and hoisting; Maven
nearest/declaration-order selection; arbitrary executable package manifests;
Gradle open-ended untyped attributes; Cargo feature-unification surprises and
ambient `build.rs`; Homebrew mutable global tap/install truth; recipe-controlled
weakening of binary identity; cache trust without sandbox/provenance proof;
Nix IFD's hidden store dependency and sequential evaluator/build coupling.

## Card program

Each card is a full vertical slice with spec, diagnostics, examples, tests,
live acceptance, and documentation. Work order is binding.

### E4-JP0 — truth and integrity stop-line

- Stop reporting a cache hit until output existence, canonical digest,
  platform, policy, signature, and closure are verified.
- Create verified durable Nix GC roots for every referenced compatibility
  closure. JP11 later replaces those roots with native Hangar closure import;
  there is no unrooted transitional state.
- Stop reporting “strong sandbox” unless the child enters the jail.
- Inventory every E4 `done` claim as live, model-only, schema-only, fixture-only,
  or compatibility-only; reopen incomplete capability cards.
- Exit: deletion/tamper never returns cached; hostile child never gets a
  sandboxed label; truth matrix is test-enforced.

### E4-JP1 — Hangar Store v2

- Canonical tree/archive format covers bytes, file type, executable bit,
  symlink target, directory/empty-directory records, sparse-file policy, and
  normalized hardlinks. Canonical digest is over uncompressed logical bytes.
- Path law distinguishes POSIX byte names from Windows WTF-16 names and rejects
  case-fold collisions, reserved names, trailing-dot/space aliases, and
  unrepresentable cross-platform names. Unicode normalization is never implicit.
- Security/quarantine xattrs are excluded; semantic xattrs require an explicit
  platform artifact kind. Unsupported special objects are rejected.
- Race-safe no-follow ingest re-stats open handles and aborts if source mutates.
- Atomic staged ingest, fsync/rename, crash recovery, quarantine.
- References/referrers, deriver/action identity, output digest, platform,
  provenance, signatures, and multiple named outputs.
- Path-independent digest; same bytes deduplicate below package trees.

### E4-JP2 — one derivation/action IR and complete cache identity

- Lower package recipes, `fn build`, adapters, toolchains, plugins, generated
  source, and legacy wrappers into one executable BuildPlan.
- Key includes the canonical plan, all imported/generated sources, dependency
  outputs, target/profile, build-host-target roles, exact toolchain/SDK/linker,
  environment allowlist, policy/capabilities, and helper versions.
- Exact source bytes remain inputs wherever docs, doctests, diagnostics, line
  maps, embedded source, debug info, or publication can observe them. Compile,
  documentation, debug, and source-archive actions have distinct identities.
- No cache lookup may bypass parsing, sema, policy, or diagnostics.

### E4-JP3 — real cross-platform sandbox

- Linux namespaces, private proc/dev, read-only closure, writable scratch/output,
  network only for fixed-output fetch actions, no-new-privileges.
- macOS native sandbox profile plus filesystem/network/process restrictions.
- Windows restricted token, Job Object, ACL projection, and network denial.
- Hostile corpus covers path/symlink escape, host reads, sibling writes,
  undeclared executables, process/ptrace/device access, network, and daemon leak.

### E4-JP4 — closure DB, roots, leases, GC, verify, repair

- Transitive references/referrers and action/output relations.
- Roots: project locks, profiles/generations, running-process leases, build
  leases, toolchains, generic external consumers, and manual roots. Epoch 7
  registers JetOS generations through the generic external-root API.
- Atomic root update, the ratified plan-before-apply mutation UX for `jet clean`,
  why-live/why-dead, and stale-lease recovery.
- Verify, quarantine, repair from ordered caches, then deterministic rebuild.

### E4-JP5 — native binary cache and Nix cache interoperability

- Native content lookup, manifest, chunk/range resume, compression negotiation,
  ordered mirrors, negative-cache TTL, idempotent concurrent publication.
- Read/write policy and credentials remain separate.
- NAR and `.narinfo` read/write adapter pins canonical NAR bytes, NarHash,
  NarSize, compressed file hash/size, References, Deriver, CA field, store-dir
  compatibility, and the exact signed fingerprint.
- Interoperable store endpoints: local, Nix daemon/SSH, file cache, HTTP cache,
  and owner-selected S3-compatible cache scope.
- Local execution disabled acceptance proves full substitution; miss falls back
  to source build.
- Negative-cache results are mirror-local advisory hints; they never suppress
  another mirror or deterministic source fallback.

### E4-JP6A — trust primitives and root bootstrap

- Separate publisher, registry, cache builder, and remote executor identities.
- Trusted-root bootstrap distribution, offline threshold root, delegation path
  bounds, consistent snapshots, monotonic versions, metadata size limits,
  trusted-time/bad-clock rules, and signature-stripping rejection.
- Hybrid public Sigstore bundles and private/offline Ed25519/KMS/HSM signing.
  D-PKGSIGN1 author signatures remain opt-in unless the ballot amends it.
- Key rotation/revocation and root/publisher recovery drills.

### E4-JP6B — trust integration and compromise resistance

- Apply JP6A to live registry, cache, remote execution, locks, and offline
  bundles after JP5 and JP12 exist.
- SLSA provenance binds action digest, closure, output digest, platform,
  sandbox proof, toolchain, worker capabilities, and builder identity.
- Compromise, freeze, rollback, fast-forward, mix-and-match, first-root
  replacement, threshold-minus-one, and privacy-mode simulations.

### E4-JP7 — remote builders and execution

- Builder capabilities: platform, features, resource pools, concurrency,
  priority, trust domain, cache access.
- Send missing CAS inputs, execute exact action, retrieve verified outputs.
- Cancellation, retry/failover, worker loss, duplicate result agreement,
  deterministic logs, metrics, and malicious-worker rejection.
- Remote cache and execution remain separate grants.
- Result statement binds action digest, named output digests, platform/worker
  capabilities, policy/sandbox class, stdout/stderr digests, exit status,
  provenance signer, and immutable execution identity.

### E4-JP8 — Nix derivation compatibility

- ATerm `.drv` parse/encode, `hashDerivationModulo`, fixed flat/recursive,
  floating CA, text, self-referential and multiple outputs, output placeholders,
  Nix base32/store paths, reference scanning/discard rules, allowed substitutes,
  required system features, and derivation-closure copy semantics.
- Differential corpus over real store derivations and output paths.
- Compatibility types remain behind Jetpack internal interfaces.

### E4-JP9 — Nix evaluator stage A

- Lazy thunks, attrsets, functions, string contexts, path values, import,
  derivation primitive, required builtins, flake inputs/locks/registries.
- Pure/restricted default, explicit URI/path authority, dirty-tree identity,
  native-code/plugin rejection, evaluator resource limits, and ballot-selected
  IFD behavior.
- No raw evaluator trace reaches users.
- Representative nixpkgs derivations bit-match reference Nix.
- Partial evaluator stages are internal test surfaces only; no provider/product
  path enables them before JP11.

### E4-JP10 — Nix evaluator breadth and performance

- A pinned Nix version, nixpkgs commit, tier-platform attribute inventory,
  expected evaluable/buildable/skipped counts, and named reason for every skip.
- Full pinned inventory, overlays, dev shells, multi-output packages,
  fixed-output fetchers, cross packages, selected external flakes.
- Differential fuzzing and pinned memory/latency budgets.
- No silent path divergence: mismatch is a hard internal compatibility failure.

### E4-JP11 — permanent no-installed-Nix product gate

- Replace every Jetpack package/env/build `nix`/`nix-store` shell-out with
  JP5/JP8–JP10. Epoch 7 NixOS import/real-tier migration remains separate until
  its own replacement card.
- Project canonical `/nix/store` paths inside build/run sandboxes so unmodified,
  non-relocatable Nix binaries execute. Prove Linux rootless and macOS behavior;
  no path rewriting without equivalence proof. Match Nix build environment facts
  such as `/build`, `/homeless-shelter`, HOME, UID, time, and locale policy.
- Static gate and PATH-stripped integration lane forbid regression.

### E4-JP12 — live registry, solver, and package delivery

- Immutable sparse metadata and content-addressed source/binary blobs.
- Atomic publish, namespace ownership/transfer/quarantine, signed yanks, mirror
  consistency, conditional requests, offline snapshots.
- Registry dependencies resolve, fetch, verify, build/substitute, and run.
- PubGrub proof tree, selective conservative updates, lazy graph loading,
  lowest/latest verification modes.
- Dependency roles: normal, build, tool, dev, test, optional, peer/plugin, and
  target; default/disabled/mutually-exclusive features; rich require/prefer/
  reject/strict constraints; prerelease/yank rules; source/binary/no-build
  policy; deploy-closure pruning; typed-domain collision diagnostics.
- Typed credential providers use OS keychains or a scoped external pipe
  protocol. Credentials never enter dynamic repo URLs, argv, environment, logs,
  locks, or provenance.

### E4-JP13 — one semantic lock, catalogs, overlays, and source maps

- Fold machine lock and semantic rationale into one forward-compatible schema.
- Exact graph and input/follows relations; platform domains; signatures,
  provenance, policy, cache facts, owner/reason/update command.
- Three-way semantic merge, byte-stable read-only commands, selective update.
- Overlay/patch changes invalidate exactly affected actions and explain why.
- Every merge revalidates graph satisfiability, signatures, domain consistency,
  source authority, and offline completeness before atomic write.

### E4-JP14 — source-backed package profiles and generations

- Owner ballot reconciles package-profile ownership with ratified JetOS
  `user.<name>`/`jetos user` law. No second hidden per-user mechanism.
- Atomic generation switch, history, rollback, collision policy, clean env.
- Dev shells are exact generated profiles, not host PATH overlays.
- Power-loss tests permit old or new only; GC protects retained generations.

### E4-JP15 — typed variants and cross compilation

- Build/host/target dependency roles; OS, architecture, runtime, linkage, ABI,
  artifact kind, feature set.
- Universal lock covers declared supported domains without evaluating every
  theoretical machine.
- Toolchain/SDK/sysroot/linker/signing identities enter action keys.
- Native, cross, toolchain-building, remote-target, and emulator tests.

### E4-JP16 — install/build authority and upstream hooks

- Metadata probing never executes upstream code.
- Hook approval binds package identity, exact script/recipe digest, requested
  capabilities, and provider authority.
- Wrong-source mapping, transitive Git/tarball injection, credential leaks, and
  trust-evidence downgrade are rejected.

### E4-JP17 — provider conformance and lossless migration

- Contract suites for Jet registry, npm, Cargo, PyPI, SwiftPM, Maven, NuGet,
  Conan/vcpkg, Homebrew, GitHub, binary, and Nix providers.
- Normalize dependency kinds, yanks, licenses, hooks, variants, signatures,
  platforms, advisories, and source ownership without erasing provider facts.
- Importers prove behavior on real representative projects; TODOs are explicit
  source-linked migration findings, never fake generated implementations.

### E4-JP18 — reproducibility certification

- Build the same action on independent roots/builders with different cwd, UID,
  locale, timezone, clock, and scheduling.
- Structured output diff names the first differing path and producer action.
- Unreproducible outputs never enter shared trusted cache. If ratified, they may
  use a visibly untrusted namespace that cannot satisfy trusted policy.

### E4-JP19 — explain and store-operation parity-plus

- `jet explain`/`jet dossier` lenses expose why/why-not version, why-depends,
  what-depends, closure, referrers, why-live,
  cache decision, rebuild reason, action/derivation, environment origin,
  overlay winner, trust chain, repair source.
- Import/export/copy/dump/restore/sign/verify/repair/optimize operations.
- Failed-build shell recreates the exact sandbox and declared closure.
- Stable JSON and LSP use the same fact engine.

### E4-JP20 — advisory, license, SBOM, provenance policy

- OSV-compatible advisory feeds with freshness/offline bundles and exceptions.
- SPDX license expression policy, source mapping, yanks/retractions, release
  maturity, and trust-evidence no-downgrade.
- OCI referrers bind SBOM, signature, provenance, and reproducibility proof.
- Policy failures explain exact owner, edge, evidence, and smallest source fix.

### E4-JP21 — explicit finite staged planning

- If ratified, a sandboxed plan action emits a typed BuildPlan fragment.
- Canonical hash, sema, authority, acyclicity, and finite-stage checks run before
  merging stage two.
- No arbitrary evaluator filesystem read or recursive package-manager call.

### E4-JP22 — world-class acceptance and scale gate

- Runs every binding lane below across all tier-1 platforms.
- Performance gates: million-object store metadata, 100k-action graph, large
  workspaces, bounded evaluator/resolver memory, cache lookup and scheduling.
- Dogfood: CLI, full-stack server, native app, plugin, cross build, monorepo,
  public registry package, Nix flake import, remote build, offline rebuild.
- Epoch remains open until all evidence is attached and independently verified.

## Dependency order

```text
JP0
├─ JP1 store
├─ JP2 action IR
├─ JP6A trust primitives       after JP1 + JP2
├─ JP15 variants/domains       after JP2
├─ JP3 sandbox                 after JP2
├─ JP8 Nix derivations         after JP1 + JP2
├─ JP13 semantic lock          after JP1 + JP2 + JP15
├─ JP4 closure/roots           after JP1 + JP2 + JP13
├─ JP5 cache/NAR               after JP1 + JP3 + JP4 + JP6A + JP13
├─ JP16 hook authority         after JP2 + JP3 + JP6A
├─ JP12 registry/solver        after JP1 + JP6A + JP13 + JP15 + JP16
├─ JP6B trust integration      after JP5 + JP12
├─ JP7 remote execution        after JP1–JP6B + JP13 + JP15
├─ JP14 profiles               after JP4 + JP13
├─ JP17 providers              after JP5 + JP6B + JP12 + JP13 + JP15 + JP16
├─ JP18 reproducibility        after JP2 + JP3 + JP7
├─ JP20 policy                 after JP6B + JP13 + JP17 + JP18
├─ JP19 explain/store ops      after JP4 + JP5 + JP6B + JP7 + JP13 + JP14 + JP17 + JP18 + JP20
├─ JP21 staging               after JP2 + JP3 + JP6B + JP16 + ballot
└─ JP22                        after every card

JP9 follows JP8; JP10 follows JP9; JP11 follows JP3–JP5 + JP8–JP10 + JP13.
```

## Binding acceptance lanes

1. **Truth lane:** tamper/delete outputs, locks, cache metadata, and roots;
   cached/sandboxed/reproducible labels remain impossible without proof.
2. **No-installed-Nix lane:** PATH and filesystem expose no `nix` binary;
   representative nixpkgs packages/flakes/devShells resolve and realize.
3. **Hostile sandbox lane:** host read/write, symlink escape, network, process,
   ptrace, device, daemon, and undeclared executable attacks fail at OS boundary.
4. **Canonical-store lane:** modes, symlinks, empty dirs, Unicode, hardlinks,
   xattrs, deep trees, corruption, partial ingest, and concurrent registration.
5. **Cache adversary lane:** wrong hashes/platform/refs, stale/forged metadata,
   truncated chunks, decompression bombs, mirror split-brain, concurrent push.
6. **Trust compromise lane:** stolen publisher/builder/timestamp/snapshot key,
   rollback/freeze/mix-and-match, rotation/revocation/recovery.
7. **Remote adversary lane:** worker lies, disappears, replays, returns wrong
   platform, loses logs, or disagrees with another builder.
8. **GC/power-loss lane:** kill during ingest, root/profile switch, repair, GC,
   cache publish, and lock write; restart proves atomic invariants.
9. **Independent reproducibility lane:** distinct builders, roots, UIDs, paths,
   locales, clocks, and schedules produce identical canonical objects.
10. **Resolver lane:** targeted/lowest/latest/multi-platform graphs; minimal
    conflict explanations; unrelated lock records remain byte-stable.
11. **Provider lane:** live provider projects import, lock, build, run, audit,
    update, vendor, migrate, and work offline.
12. **Cross lane:** build/host/target permutations, remote target, emulator,
    and platform-specific cache identities.
13. **Profile lane:** repeated kill injection shows old/new only; rollback and
    retained-generation GC proof work offline.
14. **Scale lane:** million objects, 100k actions, large workspace/provider
    metadata, bounded memory, stable latency budgets.
15. **Authority lane:** metadata probes execute nothing; unapproved hooks and
    transitive exotic sources fail; approvals invalidate on digest/cap changes;
    credential exfiltration attempts fail.
16. **Registry lane:** concurrent same-version publish yields one immutable
    success; signed yank never frees version; namespace transfer preserves
    history; quarantine blocks resolution; stale metadata cannot hide snapshot.
17. **Universal-lock lane:** one platform's lock realizes declared other
    domains offline; unsupported domains fail before fetch; build/target and
    duplicate-version domains never cross accidentally.
18. **Policy lane:** maturity applies direct/transitive, audited exception is
    exact, trust evidence cannot downgrade, advisory freshness/expiry works.
19. **Diagnostic lane:** builder/linker/evaluator/worker/registry/sandbox failures
    have Jet codes with what/why/fix; raw tool output is optional log detail.
20. **Protocol lane:** engine dispatch preserves exit codes, deterministic plain
    output, and versioned JSON schemas for every new surface.
21. **R9 lane:** `jet run file.jet` remains rootless and manifest/profile/
    registry/daemon-free; inline dependencies are the only package opt-in.

## Ratified owner decisions (2026-07-10)

All package-manager gates below are ratified as their hybrid option D and their
implementation cards are ready:

- `D-JPK-NIXENGINE1`: native implementation versus external Nix compatibility
  engine dependency (I6).
- `D-JPK-SANDBOX2`: strong sandbox default and audited fallback semantics.
- `D-JPK-MULTIUSER1`: optional secure shared-store broker versus per-user only.
- `D-JPK-DYNAMICPLAN1`: reject dynamic graphs or allow finite typed staging.
- `D-JPK-PROFILE1`: source-backed user profile surface.
- `D-JPK-TRUSTROOT1`: TUF/Sigstore/Ed25519 trust architecture.
- `D-JPK-RESOLUTIONDOMAIN1`: global, unrestricted duplicate, or typed-domain
  version multiplicity.
- `D-JPK-VARIANT1`: variant vocabulary.
- `D-JPK-FRESHNESS1`: 24-hour release maturity default with exact audited
  exceptions spelled `package#version`, never `package@version`.
- `D-JPK-BUILDSCRIPT1`: upstream hook approval law.
- `D-JPK-CACHEAUTH1`: shared-cache writer authority.
- `D-JPK-NIXBASELINE1`: pinned Nix version and experimental-feature parity set.
- `D-JPK-NIXSTORE1`: canonical `/nix/store` execution without installed Nix.
- `D-JPK-REMOTE1`: remote builder declaration, grants, credentials, fallback.
- `D-JPK-STORECLI1`: store operation surface through existing Jet intents.
- `D-JPK-PROVIDERS2`: new provider prefixes and refs.
- `D-JPK-REGISTRY1`: immutable registry transport/governance/transactions.
- `D-JPK-POLICYSURFACE1`: policy and exception source surface.
- `D-JPK-CACHECONFIG1`: mirror/substituter and credential configuration.
- `D-JPK-STOREBACKEND1`: native/Nix store endpoint interoperability.
- `D-JPK-RESOLVEMODE1`: conservative/latest/lowest/platform verification surface.
- `D-JPK-REPROCACHE1`: treatment of unreproducible outputs.

The full current law is recorded in `docs/spec/syntax-decisions.md`. New syntax,
provider roots, dependencies, or invariant amendments discovered during
implementation still require the normal ballot protocol.
