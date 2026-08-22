# Epoch 4 — world-class Jetpack package manager

**Status:** executable master plan. **Audit date:** 2026-07-16.
**Scope:** Jetpack only. JetOS consumes this substrate in Epoch 7; OS activation,
desktop modules, installers, and system services remain Epoch 7.

This plan supersedes any Epoch 4 note that treats a schema, fixture, policy
model, or deferred protocol as a shipped package-manager feature. Existing
ratified decisions remain law unless a ballot below explicitly amends one.

## Exit claim

Epoch 4 exits when Jetpack has functional Nix package-manager support,
native Nix-package interoperability without an installed `nix` binary, and the
best compatible features from leading language and build package managers.
Per `D-JPK-EPOCHBOUNDARY1=B`, Epoch 4 proves the 20 functional lanes below and
reports the actual sandbox class. Epoch 8 card #398 alone owns hostile
Linux/macOS/Windows confinement and the full Nix-replacement claim.

The product may claim functional package-manager support only when all Epoch
4 acceptance lanes below pass against live stores, registries, caches,
builders, packages, and independent machines. Fixture-only tests support
development; they never close a product-claim card. Full parity remains reserved
until #398 also passes its hostile cross-platform lane.

## Current truth

Current Jetpack grade against the Nix package-manager bar: **C-**.

Strong foundations already exist:

- typed `package.jet` plus declaration-discovered Config surfaces;
- strict direct-dependency visibility and workspace catalogs;
- provider refs, channel intent, overlays, patches, semantic lock rationale;
- source-build recipes, toolchain records, offline mode, vendoring;
- services, secrets, discovery, migration import models, signing, SBOM;
- hangar metadata, basic GC/optimization, build logs, shell-on-fail;
- one-shot rootless default and Linux/macOS/Windows product intent.

The tier-1 platform gate drives a native package through the production
provider, lease, Hangar, offline, and clean paths in
`tests/jetpack_platform.rs`. The same focused test runs on Linux, macOS, and
Windows CI. Missing offline output fails with E1315 without leaving a partial
Hangar entry. Its success and failure rows run with an empty tool directory, so
an installed Nix or shell command cannot make the lane pass by accident. The
private lease snapshot is the non-Linux executable handoff; hostile child
confinement remains Epoch 8 card #398.

JP0 stop-line now enforces three truth boundaries:

- cache reuse verifies output existence, current canonical digest, platform,
  exact normalized source/manifest/recipe/toolchain policy, signed producer
  provenance, signature policy, and canonical closure reachability through one
  realization boundary used by CLI and JetOS. Shared cache roles pin their
  signing key, allowlist the provenance-bound builder, and reject revoked or
  changed identities. Invalid Jet-owned candidates are quarantined and E2604
  stops the command; repair is never silent. Unsigned reuse is limited to an
  exact Hangar-owned local output. Nix compatibility outputs need pinned output
  identity and verified store authority; Jetpack does not invoke an installed
  Nix executable as a cache or package fallback;
- direct `/nix/store` outputs fail closed until a native store authority proves
  and retains their closure. Jetpack does not create host GC roots through
  `nix-store`;
- canonical output archives hash node type, mode, bytes, symlink target, empty
  directories, and complete hardlink identity; reject outside aliases, escapes,
  cycles, concurrent mutation, and special files. Directory snapshots bind
  child names, types, metadata, and ctime before and after traversal. Local
  outputs are copied into per-realization sealed private snapshots. Executable
  files and executable symlink targets use lease-owned inherited file
  descriptors on Linux; macOS and Windows use the sealed private snapshot
  path. Linux lease-owned `PATH` wrappers live on a read-only private mount
  pinned by an inherited directory descriptor and are revalidated before
  handoff. Nested shell execution resolves to exact leased executable members,
  so wrapper chmod/replacement or a same-UID rename/symlink swap cannot
  redirect execution. Every realization
  carries a mandatory typed lease: missing outputs are explicitly
  non-consumable and cannot fall back to raw paths. Leases hold no object lock.
  JetOS retains original provider/output provenance alongside the snapshot,
  queries the original Nix output's full `nix-store -qR` closure, copies every
  member into generation-owned paths, and rewrites absolute store symlinks
  before lease drop; no `/proc/self/fd`, lease path, or source-store dependency
  enters the durable generation. Sandbox
  sandbox detection stays fallback until a child actually enters a jail.

Production blockers after that stop-line:

- recipes still execute as ordinary host processes; every platform reports
  fallback/unsandboxed until JP3 supplies an enforced jail;
- the native binary-cache slice provides signed NAR/narinfo publication,
  ordered host-owned mirror lookup, verified substitution, and resumable
  local/file transfer; cache repair remains a separate closure operation;
- registry dependencies have sparse signed metadata, checkpoint, and inclusion
  verification on the fetch path; complete consumer delivery still stops at
  E1207;
- the current package graph and semantic lock layers contain substantial data
  models, but several are not one live resolver/build/store path;
- source-backed package profile planning and immutable project-local
  generations now exist through the shared package/provider fact path;
  JetOS/user composition and dev-shell projection remain follow-on slices;
- direct Nix package refs need a pinned compatibility output or verified store
  authority; bounded foreign-shell projection is native;
- optional package-author TOFU remains separate from registry/cache authority;
  cache roles now fail closed on first-use key changes, rollback/freeze
  evidence, mix-and-match provenance, and revoked builders;
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

| Ability | Nix bar | Jetpack audited state | Epoch closure |
|---|---|---|---|
| Package language | lazy evaluator produces derivations | typed Jet build/env model; pinned compatibility outputs for Nix inputs | native compatibility evaluator plus one Jet action IR |
| Derivations | builder, args, env, inputs, platforms, named outputs | linear recipes plus separate build-plan models | every package path lowers to one finite action graph |
| Immutable store | objects, refs/referrers, atomic registration | metadata dirs plus some owned output trees | canonical Hangar v2 objects and closure DB |
| Addressing | derivation identity, fixed outputs, CA outputs | incomplete output hash; mixed meanings | separate complete action and canonical output digests |
| Verify/repair | verify, quarantine/repair through substituters | cache hit skips integrity proof; no repair | verify every trust boundary and repair/fallback |
| Substitution | ordered caches, miss builds locally | envelope only | native read/write cache plus NAR adapter |
| Cache trust | signed store metadata and trusted keys | empty cache signature slot; author TOFU | threshold metadata, builder provenance, rotation/revocation |
| Sandbox | enforced filesystem/process/network isolation | policy/status only; ordinary child execution | real Linux/macOS/Windows isolation backends |
| Remote builds | heterogeneous builders and distributed scheduling | host-bound capability model, deterministic scheduler, authenticated CAS exchange | verified remote action execution and failover |
| Flakes/locks | transitive inputs, follows, registries, selective update | native semantic locks; shallow foreign bridge | no-Nix flake evaluator/import and one semantic lock |
| Profiles | atomic per-user generations and rollback | project envs only | source-backed named package profiles |
| GC | closure roots and generations protect transitive objects | nearest project lock and age-based metadata GC | root/lease graph, why-live, crash-safe mark/sweep |
| Cross compilation | build/host/target roles | platform facts, host envelope | typed roles and variant-aware resolution/action keys |
| Explain | derivations, logs, why-depends, diff closures | good rationale/log substrate | file-edge closure, cache, rebuild, trust, GC explanations |
| Multi-user | secure shared store/build identities | per-user one-shot process | owner-selected optional shared-store architecture |
| Nixpkgs access | evaluator, `.drv`, NAR caches, local builds | pinned outputs and bounded evaluator; no installed-Nix shell-out | native compatibility pipeline and differential corpus |
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
- Create verified durable roots for every referenced compatibility closure.
  JP11 projects the verified bytes and references into native Hangar closure
  records; there is no raw unrooted host-store state.
- Stop reporting “strong sandbox” unless the child enters the jail.
- Inventory every E4 `done` claim as live, model-only, schema-only, fixture-only,
  or compatibility-only; reopen incomplete product-claim cards.
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
- Native per-user path resolution and reversible migration from the retired
  state/root-owned Hangar source; old state stays live until the synced native
  copy is atomically published.
- References/referrers, deriver/action identity, output digest, platform,
  provenance, signatures, and multiple named outputs.
- Path-independent digest; same bytes deduplicate below package trees.

### E4-JP2 — one derivation/action IR and complete cache identity

- Lower package recipes, `fn build`, adapters, toolchains, plugins, generated
  source, and legacy wrappers into one executable BuildPlan.
- Key includes the canonical plan, all imported/generated sources, dependency
  outputs, target/profile, build-host-target roles, exact toolchain/SDK/linker,
  environment allowlist, policy/rights, and helper versions.
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
- Hangar receipt substrate: immutable connected package-realization objects,
  lock digest projections, atomic publication, and fail-closed corruption/path
  repair that keeps the live closure intact.

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
  sandbox proof, toolchain, worker facts, and builder identity.
- Compromise, freeze, rollback, fast-forward, mix-and-match, first-root
  replacement, threshold-minus-one, and privacy-mode simulations.
- The simulation proof runs through the production trust engine and native
  cache transfer boundary: compromised signers, monotonic rollback, exact-expiry
  freeze, metadata mix-and-match, changed cache keys, and revoked builders fail
  closed with the existing signed provenance and recovery explanation.
- Native cache entries carry a signed, time-bounded admission receipt. The host
  pins the accepted receipt version and digest per cache role and output, so a
  replayed older receipt or same-version replacement cannot become usable. The
transfer report and explain JSON expose the accepted receipt version and
expiry alongside the provenance decision.
  `jet explain --json` reports this as `rebuild.cache_admissions`; the view is
  read-only and distinguishes an accepted, expired, missing, or invalid host
  admission pin.
- The signed cache action identity is derived from the exact producer facts and
  the canonical realized closure digest set. The signed cache deriver then
  commits the complete action-store key, producer record, output projection,
  platform, policy, and toolchain facts. A cache reader therefore cannot
  relabel an output with a different dependency closure or worker/build record.
- The read-only audit uses the checked Store projection and stops with a repair
  path when Hangar metadata or closure state is malformed; it never presents a
  partial inventory as a clean trust report.
- Trust material created for Hangar archives, native cache bindings, and the
  optional shared-store broker reads exactly 32 bytes from the platform OS
  CSPRNG. A failed or unsupported CSPRNG is a hard error with explicit-key
  recovery; process, path, and wall-clock values never become signing keys.

### E4-JP7 — remote builders and execution

- Builder facts: platform, features, resource pools, concurrency,
  priority, trust domain, cache access.
- Send missing CAS inputs, execute exact action, retrieve verified outputs.
- Cancellation, retry/failover, worker loss, duplicate result agreement,
  deterministic logs, metrics, and malicious-worker rejection.
- Remote cache and execution remain separate grants.
- Result statement binds action digest, named output digests, platform/worker
  rights, policy/sandbox class, stdout/stderr digests, exit status,
  provenance signer, and immutable execution identity.

Shipped slice evidence:

- `jetpack::Remote::RemoteBuildRequest` contains the requested maximum action
  capabilities, features, resource pools, platform, trust domain, separate
  cache/execution grants, and explicit local-fallback choice.
- `RemoteBuilderCapabilities` records platform, features, pools, concurrency,
  priority, trust domain, and cache/execution access. `RemoteScheduler` orders
  eligible host-owned bindings by priority and builder name, reserves each
  builder's declared concurrency slot, then advances to the next candidate
  only for a retryable worker loss.
- `jet build --builder <bound-name>` enters the canonical build executor. It
  uses the named host binding as the primary candidate, then uploads missing
  action inputs to the authenticated CAS, submits the exact action identity
  (including argv, input snapshots, outputs, and effective resource pools),
  carries a deterministic policy/provenance digest in the worker proof,
  checks the returned execution identity against that exact request, and
  restores only digest- and length-verified outputs. Other registered bindings
  with matching platform and trust facts are deterministic failover candidates;
  local action publication re-hashes the restored outputs before recording or
  re-uploading them.
- The result statement is an HMAC-SHA256-authenticated envelope over the exact
  action, named output digests and lengths, stdout/stderr digests, worker proof,
  provenance signer, and execution identity. The transport rejects unsigned,
  corrupt, unauthenticated, malformed, mismatched, stale, replayed, or
  malicious-worker records before visibility. Cancellation and result
  publication share one commit lock; result publication is idempotent only when
  a duplicate statement agrees exactly with the existing record.
- A missing result is a retryable worker loss only after the host commits its
  authenticated cancellation tombstone; each retry receives a fresh attempt
  identity, and explicit local fallback runs only after remote attempts are
  exhausted. Remote output restoration rolls back staged files if later local
  cache or provenance publication fails, so a failed attempt cannot leave a
  partial output.
- If the selected binding has no eligible remote capacity for the action's
  declared resource pools, the same explicit local fallback is honored before
  any output is published; without that grant, the scheduler error remains
  terminal.

### E4-JP8 — Nix derivation compatibility

- ATerm `.drv` parse/encode, `hashDerivationModulo`, fixed flat/recursive,
  floating CA, text, self-referential and multiple outputs, output placeholders,
  Nix base32/store paths, reference scanning/discard rules, allowed substitutes,
  required system features, and derivation-closure copy semantics.
- Differential corpus over real store derivations and output paths.
- Compatibility types remain behind Jetpack internal interfaces.

### E4-JP9 — Nix evaluator stage A

- The first ordered slice is landed as a private `NixEval` boundary. Its strict,
  independently committed oracle manifest pins the ratified Nix and nixpkgs
  identities and fails closed unless every supported system records both
  required NAR hashes. The committed Stage A manifest now has those four
  identities and is `bit_exact`; partial-stage permits can be minted only by
  unit tests.
  Evaluator code lives in a dependency-free `no_std` crate where the compiler
  forbids unsafe code and Cargo disables build scripts. Full verification also
  applies resolved-symbol Clippy denials for host processes, TCP/UDP, Unix and
  Windows local sockets, and DNS; alias-based `extern crate std` escapes must
  fail that lane. Native linking and dynamic loading require forbidden unsafe
  code or a denied dependency. Filesystem authority is limited to the explicit
  project-root import authority below; time authority remains outside this
  stage. Jetpack's integration remains private, exposes no
  evaluation entry point, and partial-stage permits are minted inside seam
  tests only.
- Lazy thunks, attrsets, functions, string contexts, bounded project-relative
  path values, and read-only imports are now shipped in the native evaluator.
  Imports receive explicit private project-root authority from Jetpack;
  absolute paths, URI paths, escapes, symlink escapes, missing files, cycles,
  and over-budget sources fail closed. Windows has no pathname-based import
  fallback; imports remain unsupported there until handle-relative authority
  is available. The bounded derivation primitive now accepts one input-addressed
  or fixed-output result, records only canonical store-path inputs, and
  materializes its drvPath and every declared output path through Jetpack's
  existing NixDrv seam. Dynamic derivations remain on their own card.
- Required pure helpers now include attribute inspection, type predicates,
  bounded list/string operations, JSON conversion, currentSystem/storeDir, and
  storePath. Remote or unverified fetch authority, dynamic derivations, and
  non-canonical hashes fail closed; no helper shells out to Nix.
- Pure/restricted default, explicit URI/path authority, dirty-tree identity,
  native-code/plugin rejection, evaluator resource limits, and ballot-selected
  IFD behavior remain enforced. String contexts retain package/path provenance
  internally and never become executable shell behavior.
- No raw evaluator trace reaches users.
- The bounded devShell projection is now a product path under
  D-JPK-NIXPRODUCT1=A. It returns typed package facts, preserves unsupported
  hooks as explicit loss records, evaluates only bounded pure lazy expressions
  plus explicitly authorized project imports, records the native evaluator
  identity in `.jet/lock`, and grants no process, network, or derivation
  authority. Named devShell outputs use the same typed projection, package
  overlays are bounded and lazy, and every declared non-fixed derivation output
  keeps its exact lock identity. Unsupported evaluator stages remain private
  and are recorded as explicit inventory skips.
- Stage A flake projection consumes the locked input graph through the same
  semantic-lock path. It preserves every `flake.lock` node, including
  transitive input edges and array-shaped follows references, and records
  indirect registry resolutions from each node's `original` and `locked`
  objects. Ambient user/system registries are not consulted; a missing node or
  source-vs-lock revision drift fails closed.
- `tests/fixtures/nix-compat/stage-a.json` records the pinned literal,
  lazy-binding, function, and attrset projections, native rejection,
  normalized lock, and four output identities. Real nixpkgs derivation
  bit-match remains a separate derivation proof; the broader mutation corpus
  and evaluator budget proof belong to E4-JP10.
  Regenerate and verify it with the exact oracle executable:
  `JET_NIX_BIN=/nix/store/<pinned-nix-output>/bin/nix node
  scripts/agent/verify-nix-eval-fixture.js`. The authority corpus is checked
  with `JET_NIX_BIN=/nix/store/<pinned-nix-output>/bin/nix node
  scripts/agent/verify-nix-eval-authority-fixture.js`.

  The derivation corpus is checked with the exact oracle executable through
  scripts/agent/verify-nix-eval-derivation-fixture.js; it records both the
  pure request and exact Nix 2.34.8 drvPath/outPath values.

### E4-JP10 — Nix evaluator breadth and performance

- A pinned Nix version, nixpkgs commit, tier-platform attribute inventory,
  expected evaluable/buildable/skipped counts, and named reason for every skip.
- Full pinned inventory, overlays, dev shells, multi-output packages,
  fixed-output fetchers, cross packages, selected external flakes. The latter
  three use one explicit read-only authority callback: verified local `file:`
  fetches, pinned target selection, and bounded local `path:` flake sources;
  remote provider access remains unsupported.
- The pinned inventory is committed in
  `tests/fixtures/nix-compat/pinned-inventory.json` and checked against the
  dependency-free evaluator table. A native `jetpack bridge flake` run writes
  one `flake-evaluator-inventory:<surface>` record for every row into the
  existing semantic lock. Covered rows record their evaluable/buildable class;
  skipped rows record `status=skipped` and their explicit authority or
  compatibility reason. The ledger is produced without an installed Nix
  executable and is discarded with the lock transaction when evaluation
  exceeds its input/resource budget.
- `tests/fixtures/nix-compat/breadth.json` records exact values, errors, the
  pinned lock, and output identities for the covered evaluator surface,
  including authority-backed cross, fetch, and local-flake cases, with seven
  fixed syntax-preserving seeds.
- `scripts/agent/verify-nix-eval-breadth.js` runs every seed against the exact
  Nix 2.34.8 oracle. A value mismatch is a hard proof failure.
- The native evaluator enforces pinned input, token, expression, import,
  string, memory, and JSON-depth limits. The host proof keeps a 1 second
  latency budget for the bounded corpus.
- No silent path divergence: the production bridge uses the same native seam
  with Nix absent and `PATH` empty.

### E4-JP11 — permanent no-installed-Nix product gate

- Replace every Jetpack package/env/build `nix`/`nix-store` shell-out with
  JP5/JP8–JP10. Epoch 7 NixOS import/real-tier migration remains separate until
  its own replacement card.
- Project canonical `/nix/store` paths inside build/run sandboxes so unmodified,
  non-relocatable Nix binaries execute. Prove Linux rootless and macOS behavior;
  no path rewriting without equivalence proof. Match Nix build environment facts
  such as `/build`, `/homeless-shelter`, HOME, UID, time, and locale policy.
- Static gate and PATH-stripped integration lane forbid regression.

The shipped gate has both proof layers. `tests/jetpack_engine.rs` checks the
package, environment, profile, tool, build, and Store entry paths for direct
installed-Nix commands, then builds a pinned compatibility fixture with an
empty `PATH`. `tests/jetpack_dispatch.rs` repeats the unsupported-package
failure through the public `jet` front door. Native flake success and bounded
failure remain covered by the same empty-`PATH` production bridge tests. The
compatibility fixture is accepted only through the real Provider/Store path;
it does not replace the native bridge or unsupported-input product checks.
The Store registration boundary copies external Nix outputs through the
no-follow Hangar ingest, re-hashes them, and records the original canonical
path only as producer provenance. Linux command consumers receive the verified
snapshot through a rootless `/nix/store` namespace projection.
The projection supports both directory and flat-file output nodes. The helper
uses absolute host tool paths, so an empty `PATH` cannot hide a shell-out
dependency. The producer record also carries the fixed Nix build facts
(`/build`, `/homeless-shelter`, store path, unprivileged UID policy,
deterministic time, and `C` locale) under one content digest. Jetpack rejects a
missing or changed fact before it projects the runtime environment, which
never replaces Jetpack's composed `PATH`.

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

The registry delivery slice now publishes the index line, source artifact,
sparse package metadata, signed checkpoint, and transparency log as one git
transaction. Initial and refreshed local clones and artifact trees are built
in private staging paths and installed by rename; duplicate versions remain
reserved after a yank. Locked resolution verifies the recorded registry source
and exact artifact, including an exact yanked version, while fresh resolution
excludes yanked entries. A verified source is ingested into the canonical
Jetpack Hangar before project linking, and the lock keeps the registry identity
while the resolved output points at that immutable Hangar object. Registry git
transport rejects embedded credentials and URL parameters, then delegates
authentication to the host Git credential provider with path-scoped helper
requests. Secrets never enter Jet argv, environment, locks, provenance, or
diagnostics; endpoint details are redacted before errors are rendered.
The resolver loads the verified registry graph, backtracks incompatible
candidate branches, and records an E2602 PubGrub proof tree with smallest
fixes. `jet update <pkg>` moves only that package's locked dependency closure;
unrelated machine and semantic lock records stay byte-stable, and the update
rationale is committed with the new lock in one atomic write.

Registry artifacts may also carry one content-bound `registry.json` record. Its
existing provider-shaped fields (`dependencies`, build/tool/dev/test,
optional, peer/plugin, and target dependency maps; `features`; and
`constraints`) are validated before publish. Resolution enables only the
production roles and the `default` feature closure, while `require`, `prefer`,
`reject`, and `strict` rules are applied to the same verified candidate set.
The exact record is retained in the semantic lock, so metadata, resolution,
Hangar install, and the immutable source hash describe one package identity.

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
- Profiles lower through the shared provider-fact carrier. Raw refs, provider
  identity, provenance, native documents, stable fingerprints, and explicit
  loss/conflict reports survive plan, lock, explain, and generated output.
- The carrier accepts the ratified direct-root forms (`#version=<exact>` and
  its bare-version shorthand), canonicalizes lock refs, and retains resolved
  source selectors separately from an intentionally unpinned source spelling.
  Unknown, mutable, duplicate, or mismatched selector facts are explicit loss
  or conflict records.
- Unsupported, lossy, ambiguous, or conflicting provider facts fail with an
  explicit diagnostic; planning never supplies a silent provider default.
- Power-loss tests permit old or new only; GC protects retained generations.

The source-backed package-generation slice now implements the package view of
this law. `profile.<name>` resolves through `PackageProfilePlan`, and
`jet profile build|switch|rollback|generations` records or activates the same
project-local generation history. Each generation build, switch, rollback, and
history read holds the project profile lock. Builds write metadata, the
provider-fact lock, and the Store lifecycle root in a hidden generation stage,
then publish the complete stage by rename; switching replaces the current
pointer atomically. History and rollback revalidate the generation witness,
projected-root digest, and Store lifecycle root before activation. The
generation lock preserves raw refs,
provider facts, output digests, and exact-path collision contenders. A
switched `profile.dev` generation projects its immutable `root/bin` into
`jet enter`, `jet dev`, and shell-hook activation. The shell does not add
source outputs directly or rebuild a profile. JetOS/user composition remains a
separate delivery slice.

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
  rights, and provider authority.
- Wrong-source mapping, transitive Git/tarball injection, credential leaks, and
  trust-evidence downgrade are rejected.

### E4-JP17 — provider conformance and lossless migration

- Contract suites for Jet registry, npm, Cargo, PyPI, SwiftPM, Maven, NuGet,
  Conan/vcpkg, Homebrew, GitHub, binary, and Nix providers.
- Normalize dependency kinds, yanks, licenses, hooks, variants, signatures,
  platforms, advisories, and source ownership without erasing provider facts.
- Each importer lowers its native bytes and typed projection into the shared
  provider-fact carrier. A missing exact identity remains a source-linked
  migration finding; it never becomes a generated mutable provider ref.
- Importers prove behavior on real representative projects; TODOs are explicit
  source-linked migration findings, never fake generated implementations.

The production provider boundary now embeds the validated shared carrier in
each producer record. Store registration and Nix lock refreshes recompute its
digest after cache and lock facts are added; explain reads that carrier and
reports a loss or conflict when it is absent, malformed, or changed.

Shipped slice evidence:

- `normalize_provider_document` preserves native JSON/TOML bytes and lowers
  npm, Cargo, and Jet registry dependency kinds, hooks, variants, platforms,
  source ownership, lock checksums, signatures, yanks, and advisory facts into
  the shared typed carrier.
- The Nix provider lowers realization/package JSON and flake-lock source facts
  into the same carrier. Derivation paths, output hashes, dependency roles,
  hooks, platforms, signatures, advisories, licenses, and source ownership stay
  typed beside the retained native bytes. Missing exact selectors, malformed
  fields, multiple identities, and conflicting records remain explicit loss or
  conflict findings.
- NuGet XML/package metadata and lock JSON retain package identities, target
  framework groups, dependency requests and resolutions, licenses, repository
  ownership, content hashes, signatures, deprecation, and advisory fields.
  Multiple package identities, unresolved ranges, malformed groups, and mutable
  versions remain explicit loss findings.
- Conan recipes and graph-lock JSON retain recipe/package refs, runtime/tool/
  build dependencies, settings, options, generators, revisions, package IDs,
  and native hook lines. Missing refs, malformed dependency shapes, graph nodes,
  and executable recipe hooks remain explicit loss or conflict findings.
- vcpkg manifests retain version variants, baseline, overrides, features,
  supports, license, dependency constraints, host/platform facts, and the full
  native JSON projection. Malformed dependency entries, feature/override shapes,
  missing names, and conflicting version fields are explicit findings.
- PyPI JSON and Core Metadata retain native bytes while lowering distribution
  identities, requirements, classifiers, artifacts, hashes, yanks, signatures,
  vulnerabilities, and source fields. Missing or dynamic identity remains an
  explicit loss.
- SwiftPM Package.resolved v1/v2/v3 pins retain the lock document and lower
  revisions, versions, branches, locations, and pin state. Branch-only pins,
  malformed locks, and ambiguous multi-pin inputs remain explicit loss or
  conflict findings.
- Maven POMs retain XML and lower GAV coordinates, dependency scopes, licenses,
  build goals, profiles, repositories, SCM, and audit fields. Missing
  coordinates, unsupported namespaces, and conflicting declarations refuse
  losslessly with a finding.
- Homebrew imports retain formula versions, dependency kinds, tap/source
  ownership, bottle platform hashes, relocatability, hooks, and deprecation
  facts. GitHub imports retain release/tag and commit identity, source URLs,
  release status, signatures, advisories, and per-asset platform/digest facts.
  Binary imports require an exact content digest and target platform while
  retaining signature, provenance, SBOM, variant, and source facts.
- `import_npm` and `import_cargo` emit exact provider refs only when the source
  contains an exact identity. Mutable requests, missing locks, ambiguous locks,
  malformed fields, and conflicting native records remain explicit loss,
  conflict, or migration findings.
- `import_nix_facts` emits a Nix provider ref only when a package entry carries
  an exact version, revision, or digest. Mutable package names remain source-
  linked findings and never enter generated Jet source.
- Real-project import checks retain Cargo manifest/lock bytes, target and dev
  dependency roles, npm dependency-kind facts, and exact source identities.
  Every refused generated ref carries a source-linked migration finding;
  unresolved or bundled npm inputs never become silent defaults.
- Provider graph unit tests and package importer tests exercise both
  lossless lock/explain projection and the failure path that refuses a mutable
  generated ref.
- NuGet, Conan, and vcpkg conformance tests exercise the production normalizer,
  shared-carrier export, exact lock identity, and explicit malformed/conflicting
  provider findings.
- PyPI, SwiftPM, and Maven conformance tests exercise the same production
  normalizer path, retained native documents, explain output, exact lock
  identity, and explicit malformed/conflicting provider findings.

### E4-JP18 — reproducibility certification

- Build the same action on independent roots/builders with different cwd, UID,
  locale, timezone, clock, and scheduling.
- Structured output diff names the first differing path and producer action.
- Unreproducible outputs never enter shared trusted cache. If ratified, they may
  use a visibly untrusted namespace that cannot satisfy trusted policy.
- The shared closure-registration gate reuses the canonical Hangar digest and
  action key. A divergent registration writes deterministic evidence to
  `private/unreproducible/<action-key>.json` and does not commit a closure fact;
  trusted cache publication, verification, and substitution reject that action.
- The production source-build path certifies uncached candidates in two
  fresh Hangar subroots. It promotes the first result only after the action,
  output tree, named outputs, and producer facts agree; retries and
  cancellation discard both private roots, and a mismatch remains untrusted
  evidence until a later fresh agreeing certification replaces it.

Shipped slice evidence:

- `Store::realize_verified` sends uncached source candidates through two
  private Hangar roots before closure or cache publication. The same gate
  checks output bytes, named outputs, action identity, capabilities, and
  producer facts.
- `jet-reproducibility-report-v1` stores deterministic first-difference JSON
  under `private/unreproducible/<action-key>.json`; its producer-action record
  carries both action keys and source digests. E1315 points to that report and
  trusted cache operations reject its action until a fresh agreeing build.

### E4-JP19 — explain and store-operation parity-plus

- `jet explain`/`jet inspect dossier` lenses expose why/why-not version, why-depends,
  what-depends, closure, referrers, why-live,
  cache decision, rebuild reason, action/derivation, environment origin,
  overlay winner, trust chain, repair source.
- Card #430 ships the package slice through `jet explain`: the default view
  joins Store identity, provider facts, dependency/closure edges, liveness
  roots, and rebuild checks. `why-depends`, `what-depends`, `closure`,
  `why-live`, and `rebuild` select one causal view. JSON keeps the same fact
  model and reports loss or conflict instead of filling missing facts.
- Package explain also reads each matching profile-generation fact carrier and
  verifies the realized output digest against its Store identity. Missing,
  malformed, stale, or conflicting provider/profile facts remain explicit
  reports in text and JSON; they are never replaced with defaults.
- Provider-qualified refs use the shared typed target identity for explain and
  persisted build-attempt lookup, so selector and source-authority spellings
  do not hide the same package's failure record.
- Import/export/copy/dump/restore/sign/verify/repair/optimize operations.
- `jet hangar copy` locks and snapshots the source closure, verifies its signed
  archive with the source Hangar key, then imports it through the locked
  destination staging and closure-registration path. A fresh destination does
  not need an unrelated local signer key.
- Repair is one locked Hangar transaction: a signed archive is staged and
  re-hashed, a corrupt object is quarantined, and failed import restores the
  prior object; crash leftovers are recoverable through `hangar recover`.
- Failed-build shell recreates the exact sandbox and declared closure.
- Stable JSON and LSP use the same fact engine.

### E4-JP20 — advisory, license, SBOM, provenance policy

- OSV-compatible advisory feeds with signed offline bundles, monotonic sequence
  and expiry checks, 24-hour third-party release maturity, exact
  `package#version` exceptions, and trust-evidence no-downgrade.
- `jet inspect audit` reads only the project `.jet/lock`, signed
  `.jet/advisories.db`, and pinned `.jet/advisory-trust`; it prints the verified
  feed receipt and fails with E2611 when the lock or advisory database is
  absent. Malformed locked package versions fail closed instead of becoming
  unaudited results. A configured feed is verified before a new registry
  candidate can be installed; existing exact locks remain unchanged by
  freshness policy. The
  accepted sequence, digest, key, and maturity window are carried into the
  Hangar provenance for explain/audit output.
- SPDX license expression policy and package source mapping use the ratified
  `policy:` namespace. `policy.licenses: .Allow([...])` limits candidate
  identifiers. `policy.sources` maps exact package names or trailing `*`
  patterns to source authorities. Jet requires a concrete SPDX expression,
  checks both fields after registry identity, signature, and artifact checks,
  and rejects the candidate before Hangar ingest when a rule fails.
- Source-owned freshness exceptions use the same namespace:
  `policy.exceptions: [PolicyException.{ id: "JSA-…", scope: "package#version",
  reason: "…", expires: 9999999999 }]`. The scope is exact, the record is
  expiring, and an active exception can waive only release maturity. Signed
  trust-root failures and matching advisories still deny the candidate.
- The allow or deny result carries the matched source rule and policy
  fingerprint into Hangar provenance, cache identity, and semantic-lock
  explanation. Only active exact exceptions carry their id, scope, reason, and
  expiry as applied evidence; expired declarations cannot authorize a result.
  Locked and offline resolution repeat the metadata check.
- Yanks/retractions, release maturity, and trust-evidence no-downgrade remain
  part of the same fail-closed policy path.
- OCI referrers bind SBOM, signature, provenance, and reproducibility proof.
- The git registry stores one immutable referrer set at
  referrers/<content-hash>/: a subject-bound index.json and four
  content-addressed blobs. The SBOM uses canonical lock bytes when a lock is
  present, while the signature, provenance, and reproducibility blobs bind the
  exact index entry. Signed sparse metadata also binds the referrer index
  digest, so replacing an index and its SBOM together cannot create a mixed
  evidence set.
- Publication stages the referrer set in the same explicit git transaction as
  the artifact, index line, sparse metadata, checkpoint, and transparency log.
  Fetch verifies every descriptor, blob digest, subject, and bound fact before
  a candidate is usable; missing, stale, mixed, or tampered evidence fails
  closed and asks for republish or restoration of the immutable evidence set.
- Policy failures explain the exact source owner and dependency edge, retain
  the policy evidence, and give the smallest source fix. Freshness exceptions
  are also printed by `jet inspect audit` when they actually apply.

### E4-JP21 — explicit finite staged planning

- A sandboxed plan action emits a typed `BuildPlan` fragment through the
  production `jetpack::Recipe::run_staged_plan_action` seam.
- The package model binds the finite stage, exact input digests, realized tools,
  effects, platform, outputs, dependencies, canonical fragment digest, and lock
  identity before the fragment enters the ordinary `BuildPlan` graph.
- `PlanSandbox` exposes only declared source inputs. Store reads and package
  resolution are denied. Cycles, undeclared inputs, unauthorized tools/effects,
  platform mismatches, overlapping/escaping outputs, cancellation, and failed
  callbacks publish no artifact.
- Publication writes the canonical fragment, lock, and plan fingerprint to a
  scratch directory, then renames it atomically. Repeating the same action
  returns the same identity and artifact directory.
- Production-path proof: `tests/jetpack_engine.rs` runs the real
  `jetpack::Recipe` seam for deterministic success, undeclared access, failed
  stages, cancellation, cycles, and invalid outputs.

### E4-JP22 — world-class acceptance and scale gate

- Runs every binding lane below across all tier-1 platforms.
- Performance gates: million-object store metadata, 100k-action graph, large
  workspaces, bounded evaluator/resolver memory, cache lookup and scheduling.
- Dogfood: CLI, full-stack server, native app, plugin, cross build, monorepo,
  public registry package, Nix flake import, remote build, offline rebuild.
- Epoch remains open until all evidence is attached and independently verified.

Card #954 ships the store, action-graph, resolver, and evaluator budget seams
used by this gate. Checked Hangar listing fails closed above one million
objects or one MiB of metadata, closure journals bound objects, records,
deletions, and transactions, and read-only model listing remains a separate
infallible observation view. Build plans admit at most 100,000 actions and
construct execution stages with a dependency-indexed topological walk. The
source-backed package-profile resolver admits and resolves 100,000 profiles
iteratively, with bounded package, source, collision, and inheritance output.
The native evaluator keeps its existing input, token, expression, import,
string, memory, and JSON-depth budgets. Focused proof is
`tests/jetpack_engine.rs::checked_hangar_listing_rejects_oversized_metadata`,
`jet_comptime::Comptime::Build::execution_helpers::tests::action_admission_budget_accepts_the_limit_and_rejects_the_next_action`,
and `jet_env_model::ModuleEval::Environment::tests::package_profile_resolver_handles_its_full_depth_budget_iteratively`.

Card #955 makes the dogfood claim auditable as one portfolio. Each row needs a
real production entry point, a success check, and a failure check. A fixture
may supply deterministic bytes, but it cannot replace the engine, provider,
store, lock, or front-door dispatch being tested.

| Workload | Production entry | Success proof | Failure proof |
|---|---|---|---|
| CLI | `jet` → `jetpack` | `tests/jetpack_dispatch.rs::jet_env_delegates_to_jetpack_enter` | `tests/jetpack_dispatch.rs::jet_run_without_nix_compatibility_output_reports_e1272` |
| Full-stack server | Jetpack service supervisor | `tests/jetpack_services.rs::services_up_health_logs_down_roundtrip` | `tests/jetpack_services.rs::dev_service_never_healthy_is_e1261` |
| Native app | Core provider → Hangar executable | `tests/jetpack_engine.rs::epoch4_dogfood_portfolio_rebuilds_offline_after_component_loss` | same test's missing-output/source-component run |
| Plugin | BuildPlan packaged-component seam | `tests/build_graph.rs::packaged_build_plugins_verify_bytes_and_roll_back_rejected_contributions` | same test's rejected contribution path |
| Cross build | typed target/action identity | `tests/build_graph.rs::compiler_package_identity_target_and_profile_force_rebuild_keys` | `tests/jetpack_engine.rs::bridge_flake_projects_fetchers_cross_packages_and_external_flakes_without_nix` |
| Monorepo | workspace/member resolver | `tests/jetpack_engine.rs::two_process_reverse_package_order_does_not_deadlock` | `tests/jetpack_engine.rs::mono_example_has_two_package_jet_members` structural guard |
| Public registry package | `jet registry publish` → fetch → Hangar | `tests/pkg.rs::registry_fetch_installs_verified_artifact_in_hangar_and_locked_reuses_it` | `tests/pkg.rs::registry_fetch_rejects_tampered_referrer_index_before_hangar_ingest` |
| Nix flake import | native `jetpack bridge flake` evaluator | `tests/jetpack_engine.rs::bridge_flake_native_evaluator_without_nix` | `tests/jetpack_engine.rs::bridge_flake_rejects_dynamic_native_evaluator_input` |
| Remote build | authenticated BuildPlan remote seam | `tests/build_graph.rs::remote_driver_consumes_authenticated_worker_result` | `tests/jetpack_engine.rs::remote_ineligible_builder_honors_local_fallback` |
| Offline rebuild | Hangar verification + local provider | `tests/jetpack_engine.rs::committed_example_builds_offline_end_to_end` and the card #955 gate above | `tests/jetpack_engine.rs::offline_without_fixtures_errors` |

The card gate also runs with an empty tool directory, exercises `--offline`,
removes a realized component before retry, and runs `clean` against stale
Hangar state. Platform coverage remains the native Linux/macOS/Windows lane
in `tests/jetpack_platform.rs`; an unsupported host is a failed lane, never a
skip.

## Dependency order

```text
JP0
├─ JP1 store
├─ JP2 action IR
├─ JP6A trust primitives       after JP1 + JP2
├─ JP15 variants/domains       after JP2
├─ JP3 hostile sandbox         E8 #398, after JP2
├─ JP8 Nix derivations         after JP1 + JP2
├─ JP13 semantic lock          after JP1 + JP2 + JP15
├─ JP4 closure/roots           after JP1 + JP2 + JP13
├─ JP5 cache/NAR               after JP1 + JP4 + JP6A + JP13
├─ JP16 hook authority         after JP2 + JP6A
├─ JP12 registry/solver        after JP1 + JP6A + JP13 + JP15 + JP16
├─ JP6B trust integration      after JP5 + JP12
├─ JP7 remote execution        after JP1–JP6B + JP13 + JP15
├─ JP14 profiles               after JP4 + JP13
├─ JP17 providers              after JP5 + JP6B + JP12 + JP13 + JP15 + JP16
├─ JP18 reproducibility        after JP2 + JP7
├─ JP20 policy                 after JP6B + JP13 + JP17 + JP18
├─ JP19 explain/store ops      after JP4 + JP5 + JP6B + JP7 + JP13 + JP14 + JP17 + JP18 + JP20
├─ JP21 staging               after JP2 + JP6B + JP16 + ballot
└─ JP22                        after every card

JP9 follows JP8; JP10 follows JP9; JP11 follows JP4–JP5 + JP8–JP10 + JP13.
```

## Binding acceptance lanes

1. **Truth lane:** tamper/delete outputs, locks, cache metadata, and roots;
   cached/sandboxed/reproducible labels remain impossible without proof.
2. **No-installed-Nix lane:** PATH and filesystem expose no `nix` binary;
   pinned package fixtures and bounded flakes/devShells resolve natively;
   unsupported package refs fail with E1272.
3. **Canonical-store lane:** modes, symlinks, empty dirs, Unicode, hardlinks,
   xattrs, deep trees, corruption, partial ingest, and concurrent registration.
4. **Cache adversary lane:** wrong hashes/platform/refs, stale/forged metadata,
   truncated chunks, decompression bombs, mirror split-brain, concurrent push.
5. **Trust compromise lane:** stolen publisher/builder/timestamp/snapshot key,
   rollback/freeze/mix-and-match, rotation/revocation/recovery.
6. **Remote adversary lane:** worker lies, disappears, replays, returns wrong
   platform, loses logs, or disagrees with another builder.
7. **GC/power-loss lane:** kill during ingest, root/profile switch, repair, GC,
   cache publish, and lock write; restart proves atomic invariants.
8. **Independent reproducibility lane:** distinct builders, roots, UIDs, paths,
   locales, clocks, and schedules produce identical canonical objects.
9. **Resolver lane:** targeted/lowest/latest/multi-platform graphs; minimal
    conflict explanations; unrelated lock records remain byte-stable.
10. **Provider lane:** live provider projects import, lock, build, run, audit,
    update, vendor, migrate, and work offline.
11. **Cross lane:** build/host/target permutations, remote target, emulator,
    and platform-specific cache identities.
12. **Profile lane:** repeated kill injection shows old/new only; rollback and
    retained-generation GC proof work offline.
13. **Scale lane:** million objects, 100k actions, large workspace/provider
    metadata, bounded memory, stable latency budgets.
14. **Authority lane:** metadata probes execute nothing; unapproved hooks and
    transitive exotic sources fail; approvals invalidate on digest/cap changes;
    credential exfiltration attempts fail.
15. **Registry lane:** concurrent same-version publish yields one immutable
    success; signed yank never frees version; namespace transfer preserves
    history; quarantine blocks resolution; stale metadata cannot hide snapshot.
16. **Universal-lock lane:** one platform's lock realizes declared other
    domains offline; unsupported domains fail before fetch; build/target and
    duplicate-version domains never cross accidentally.
17. **Policy lane:** maturity applies direct/transitive, audited exception is
    exact, trust evidence cannot downgrade, advisory freshness/expiry works.
18. **Diagnostic lane:** builder/linker/evaluator/worker/registry/sandbox failures
    have Jet codes with what/why/fix; raw tool output is optional log detail.
19. **Protocol lane:** engine dispatch preserves exit codes, deterministic plain
    output, and versioned JSON schemas for every new surface.
20. **R9 lane:** `jet run file.jet` remains rootless and manifest/profile/
    registry/daemon-free; inline dependencies are the only package opt-in.

Epoch 8 #398 adds the hostile sandbox lane: host read/write, symlink escape,
network, process, ptrace, device, daemon, and undeclared executable attacks fail
at the OS boundary on Linux, macOS, and Windows.

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
