# Jetpack package-manager rationale

Jetpack uses one typed graph for resolution, build, cache, audit, profiles,
and environment/package projections. Jet remains the canonical authoring
language; Nix and other ecosystems are compatibility and prior-art inputs.

The laws, Nix comparisons, ecosystem references, and rejected anti-patterns
below explain the design. Current implementation status, sequencing, and
acceptance obligations belong in Tower.


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

## Nix comparison

| Nix concern | Jetpack design response |
|---|---|
| Package language and derivations | Jet is the authoring language; typed package/build inputs lower to one finite action graph. |
| Immutable store and addressing | Hangar stores canonical objects and closure roots; action identity and output-content identity stay separate. |
| Verification and repair | Every trust boundary verifies bytes and metadata; invalid objects are quarantined or rebuilt. |
| Substitution and cache trust | Native cache reads/writes use ordered mirrors, signed metadata, builder provenance, and revocation. |
| Sandbox | Package actions run behind enforced filesystem, process, network, and device boundaries. |
| Remote builds | Typed builder facts, grants, authenticated content exchange, and explicit fallback preserve the same action meaning. |
| Flakes and locks | Semantic locks retain transitive inputs and selective updates; foreign flakes are compatibility inputs, not a second authoring language. |
| Profiles and GC | Source-backed profiles, generations, leases, closure roots, and crash-safe collection explain why an object remains live. |
| Cross compilation and variants | Build/host/target roles and typed platform/linkage/ABI axes enter resolution and action identity. |
| Explainability | File-edge closure, cache decisions, rebuild reasons, trust facts, and GC roots are inspectable. |
| Multi-user stores | Shared-store authority is optional; per-user operation remains a valid default. |
| Nixpkgs access | A bounded evaluator, derivation/NAR compatibility, and differential evidence avoid an installed-Nix dependency. |
| Dynamic planning | Dynamic derivations are either finite typed staging or an explicit rejection, never hidden evaluation. |

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

## Ecosystem lessons and alternatives

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

## Ratified owner decisions

These decision identifiers point to the ratified package-manager law:
The full wording is recorded in `docs/spec/syntax-decisions.md`. Changes to
syntax, provider roots, dependencies, or invariants require the normal ballot
protocol.

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

