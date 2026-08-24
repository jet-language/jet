# Jetpack native nixpkgs ingestion: design and cost

Date: 2026-08-24

Scope: audit and plan only. This report does not change Jetpack behavior and does not create Tower cards.

## Executive conclusion

Jetpack should first add a native `cache.nixos.org` client and a Jet-signed channel index, then expand its native evaluator behind the same package identity. This is the shortest path to useful nixpkgs coverage without Nix on the user's machine. It is also the only staged path that does not pretend an index can handle overlays or that the current bounded evaluator can evaluate nixpkgs.

The two required operations are separate:

| Operation | Question | Immediate answer |
|---|---|---|
| Evaluation | Which exact `/nix/store/...` output implements `default.[ripgrep]` for this channel revision and system? | A signed off-device Hydra/channel index. Later, a full native evaluator. |
| Substitution | How does Jetpack get and trust that output and its complete runtime closure? | Parse standard `.narinfo`, verify the upstream Ed25519 signature, stream and verify the compressed NAR, fetch every reference, then admit the closure to the Hangar. |

Neither operation may invoke `nix` on the user's machine. The index producer may use upstream Hydra data and may run Nix off-device. That is build infrastructure, not a user dependency.

Current cold-machine coverage is **0/28 selections in `env.jet`** (27 plain
nixpkgs attrs plus nested `rPackages.jsonlite`), **0/22 direct nixpkgs
selections in `devShells.default`**, and **0/48 Nix-derived selections in
Linux `devShells.full`**. The two repo-local wrappers in each flake shell also
need Jet-native translation. A verified, already-present Hangar object can be
reused, but Jetpack has no cold path that discovers and downloads any of these
nixpkgs outputs.

## What exists today

Jetpack does not need a new store. It needs a standard Nix cache admission path and a package-to-store-path discovery feed wired into the store it already has.

### NAR, cache, closure, and projection inventory

| Capability | What exists | Exact gap |
|---|---|---|
| NAR codec | `write_nar` and `read_nar` implement a deterministic, bounded codec (`crates/jetpack/src/Store/Nar.rs:57-129`). Passing tests include `nar_roundtrip_is_deterministic` (`Nar.rs:1295`) and `binary_cache_nar_codec_rejects_noncanonical_and_malicious_input` (`tests/jetpack_engine.rs:1064`). | The `.narinfo` model accepts only `Compression: none`, requires `FileSize == NarSize`, and accepts Jet HMAC signatures only (`Nar.rs:137-187`). A real cache entry uses zstd and an upstream Ed25519 signature. The parser also rejects unknown fields and requires `FileHash == NarHash` (`Nar.rs:229-306`). The current byte-buffer API and 1 GiB object cap (`Nar.rs:18-24`) need a measured streaming policy for large SDK and browser outputs. |
| Local/Jet cache | Local publish, verify, corruption rejection, resumable transfer, mirror policy, and trust receipts exist. `binary_cache_local_publish_verify_and_reject_corruption` passes (`tests/jetpack_engine.rs:282`). | Remote lookup begins with an already-known Hangar `StoreEntry` (`Store/Cache.rs:548-555,661-675`) and constructs Jet's own object name and receipt paths (`Store/Cache.rs:968-1001`). It cannot discover or admit an unknown Nix store path. |
| HTTP and Nix endpoints | HTTP endpoints are read-capable (`Store/Cache.rs:93-169`). A separate Nix endpoint supports path-info, verify, and copy. | HTTP shells out to `curl` (`Store/Cache.rs:2491-2534`). The Nix endpoint shells out to `nix path-info`, `nix store verify`, and `nix copy` (`Store/Cache.rs:2303-2474`). The latter is expressly a non-answer for this requirement; the former also conflicts with “install Jet and it works” unless transport becomes native. |
| Closure graph | The Hangar has direct and transitive reference graphs, atomic registration, roots, leases, GC, and connected receipts (`Store/Closure.rs:430-657,921-996`). `store_validates_complete_closure` passes (`Store/Closure.rs:2186`). | Nix provider results always set `references: Vec::new()` (`Provider.rs:2551-2563`). No cache reference closure reaches the graph. |
| Nix output ingest | External Nix outputs are copied without following links into content-addressed Hangar objects (`Store.rs:537-799`). The record marks Hangar CAS as closure and projection authority (`Store.rs:591-594`). | A pinned result names an output; it does not fetch the output or its references. Copying one output is not a runnable dynamic closure. |
| Runtime projection | Verified leases map logical Nix output paths to Hangar snapshots (`Store.rs:1685-1764`). Linux enters a rootless mount namespace and bind-mounts those paths (`Shell.rs:191-294`). | The overlay explicitly uses the host `/nix/store` as its lower layer and defers an exact runtime closure (`Shell.rs:265-297`). A machine with no Nix store can therefore still miss loaders and libraries. This path also depends on host `unshare`, `mount`, and `sh` and has no equivalent implementation on other targets (`Shell.rs:216-238,423-437`). |
| Ed25519 | Jetpack already links `ed25519-dalek` (`crates/jetpack/Cargo.toml:17-30`) and verifies an Ed25519 cache receipt elsewhere (`Store.rs:2822-2843`). | The standard `.narinfo` verifier does not use it. Nix base64 signatures and their canonical fingerprint must be implemented and tested separately from Jet receipts. |

The nearest passing end-to-end Nix-provider test is `build_resolves_fixture_ref` (`tests/jetpack_engine.rs:2618`). It is not a substitution test. `realized_fixtures` creates a payload in a staging directory, seeds `hangar/objects/<digest>`, and rewrites the fixture JSON to that object (`tests/support/jetpack_fixtures.rs:241-301`). `write_fastfetch_fixture` does the same for an executable shell stub (`jetpack_fixtures.rs:420-436`). This proves the path from a prepared provider result through Hangar admission; it proves neither evaluation nor fetching from a Nix cache.

### What “pinned compatibility output” means

The Nix provider admits exactly two states: an explicit fixture file or verified Hangar reuse (`crates/jetpack/src/Provider.rs:1246-1290`). Its fixture name is derived from the package reference (`Provider.rs:1214-1217`). The file contains the JSON shape emitted by `nix build --json`: exactly one result with `drvPath` and an `outputs` object. `out` or `bin` supplies the primary store path (`Provider.rs:2459-2503`).

Jetpack records the derivation and named output paths, labels the result as substituted, and records no references (`Provider.rs:2510-2563`). It does not evaluate the package, query a cache, fetch bytes, or prove the runtime closure. Therefore a pinned JSON file alone is insufficient; the named output must already exist as a real Nix store path or a pre-seeded Hangar object.

Without that state, the production path returns:

```text
Error [E1272]: 1 package lacks a supported Nix compatibility output
 Why: `ripgrep@default` need a pinned compatibility output. Jetpack does not
      invoke an installed Nix executable for package realization.
```

The diagnostic is emitted at `crates/jetpack/src/CLI/realize.rs:357-364`. `no_nix_nixpkgs_package_reports_e1272` locks this behavior in (`tests/jetpack_engine.rs:4066`). This is honest behavior, but it is the missing feature, not native nixpkgs support.

### The existing native evaluator is bounded projection

`jet-nix-eval` is a useful compatibility seed. It is `no_std`, forbids unsafe code, and has explicit resource budgets (`crates/jet-nix-eval/src/lib.rs:1-15,90-97`). It supports a bounded expression subset, a small derivation materializer, project-relative imports, selected fetch authorities, and a few dozen builtins (`crates/jet-nix-eval/src/Evaluator.rs:81-144,2028-2075`). `NixDrv` implements Nix32, store-path calculus, stable ATerm derivations, and derivation modulo hashing (`crates/jetpack/src/NixDrv.rs:1-8,50-190,396-749`).

It does **not** evaluate nixpkgs. The root environment injects `pkgs` as a symbolic package namespace (`crates/jet-nix-eval/src/Evaluator.rs:1154-1197`). Selecting any unknown attribute converts it into a package-name string (`Evaluator.rs:1878-1928`). The public contract says the dev-shell projection is deliberately smaller than Nix (`crates/jet-nix-eval/src/lib.rs:248-254`); dynamic derivations and import-from-derivation are explicitly skipped (`lib.rs:207-218`). The product boundary is described as non-executing and bounded (`crates/jetpack/src/NixEval/Boundary.rs:1-8`).

Tests named `bridge_flake_*_without_nix` therefore prove translation of supported surface shapes, not nixpkgs evaluation. For example, `pkgs.fd` becomes the identity `fd`; no nixpkgs function that defines `fd` runs.

## Coverage today

### Real repository demand

The historical `env.jet` baseline requested 20 packages from `nixos-unstable`.
Current `env.jet:8-18` requests 28 selections:

```text
cargo sccache clippy rustc rustfmt gcc clang lld nodejs_22 python3 nixfmt
ripgrep jq gh fd bashInteractive zsh fish util-linux wasm-tools tree-sitter
pkg-config tzdata vulkan-loader ruby php rWrapper rPackages.jsonlite
```

All 28 dispatch to `NixProvider`. There are no committed compatibility
fixtures or published index target for them. The index/provider substitution
consumer exists, but it has no generated target proving these selections. On a
cold no-Nix machine the honest count is **0/28**.

`flake.nix:110-137` adds `rustfmt` and `python3`, for **22 direct nixpkgs selections**, plus the repo-local `jetDev` and `jetpackDev` wrapper derivations. It also carries a shell hook, `TZDIR`, and a Linux library path (`flake.nix:139-156`). The direct-package count is **0/22** on the target machine. The wrappers and hook are translation work, not index entries.

Linux `devShells.full` contains **48 Nix-derived selections plus two repo-local wrappers** (`flake.nix:161-247`):

```text
cargo sccache clippy rustc rustup rustfmt gcc clang gnat fpc dart powershell
gfortran gnucobol go jdk dotnet-sdk_8 tcl lua5_4 lld ruby php jetR octave qemu
nodejs_22 python3 nixfmt ripgrep jq gh fd bashInteractive zsh fish util-linux
wasm-tools wasmtime tree-sitter emscripten lldb pkg-config raylib chromium firefox
geckodriver gtk4 bubblewrap
```

`jetR` is a composed `rWrapper.override` with `rPackages.jsonlite`, not a plain channel attr (`flake.nix:17-20`). `gnucobol` selects `lib.getBin`. Linux membership is conditional. `jetDev` and `jetpackDev` are generated scripts (`flake.nix:62-90`). The cold no-Nix count is **0/48**, and the two wrappers also cannot be reproduced from an attr index alone.

### Probe evidence and limitation

The owner's real repo-root probe reached the relevant production failure:

```text
$ ./target/debug/jetpack enter -- echo hi
Error [E1272]: 1 package lacks a supported Nix compatibility output
 Why: `ripgrep@default` need a pinned compatibility output. Jetpack does not
      invoke an installed Nix executable for package realization.
```

`default.[cargo]` currently stops earlier at E1317 because its name resembles a provider. Another worker owns that independent parser defect; this audit does not duplicate it.

I also attempted the package list as individual `target/debug/jetpack build <name>@nixpkgs` probes from `$HOME/.cache/jet-test-scratch`. The managed audit sandbox stopped every command before provider dispatch:

```text
Error [E2604]: Integrity check failed for Hangar path migration legacy — expected complete native per-user Hangar, got Read-only file system (os error 30).
```

That output is not package evidence and is not counted as E1272. The 0 count instead follows from the production dispatch above plus the exhaustive source fact that `NixProvider::realize` has no non-fixture online arm (`Provider.rs:1267-1290`). A future acceptance run must probe all names with a writable fresh Hangar and an empty `PATH`; the current sandbox could not supply that writable `$HOME` without violating the read-only audit constraint.

## How more than 100 closed Jetpack cards left this gap

The short answer is that the cards built horizontal package-manager machinery, while the Nix compatibility provider remained a fixture boundary. Most evidence began after a package identity or local source was already available. No milestone gate required a cold `default.[ripgrep]` realization on a machine with no Nix, no fixture, and no prior Hangar object.

This sample is enough to locate the miss:

| Card | What it actually delivered | Why its evidence could not catch E1272 |
|---|---|---|
| #99, legacy source-build helpers | Historical source-build substrate, later reclassified as non-production-hermetic. | It has no exit criteria. Its fixture realization starts from prepared local content. |
| #179, toolchain as dependency | Package/toolchain pinning design and adjacent reuse machinery. | It has no exit criteria. A prebuilt toolchain object is downstream of nixpkgs evaluation and substitution. |
| #418, truth and integrity stop-line | Cache-hit, closure, GC-root, migration, and reproducibility hardening. | It has no retained criteria. It improved validation after a result exists; it did not create a result discovery path. |
| #419, one action IR | One action graph and complete action/cache identity. | Its tests cover action keys and `Recipe` lowering. Criterion 4 cites the truth matrix, not a cold external package realization. |
| #431, advisory feeds | Signed advisory policy, freshness, and audit failure behavior. | Correct but orthogonal: every criterion begins with known package/lock identities. |
| #477, on-demand tools | Tool verbs, global projection, collision diagnostics. | Criterion 2 promises realization across nixpkgs/GitHub/path, but its evidence says only “install projects ~/.jet/bin + generations meta.” That evidence does not prove the criterion and could not catch E1272. |
| #479, doctor | Real health probes and deterministic healthy/degraded/broken output. | It correctly reports this checkout as degraded. Its criteria never require doctor to repair or realize nixpkgs. |
| #641, provider noise parser | Robust extraction of one JSON payload from noisy `nix build --json` output. | It hardened the pinned/legacy producer document. Its “real noisy-host” proof assumes Nix produced the JSON; it does not remove Nix from the user machine. |
| #650, Hangar-seeded fixtures | Replaced fake scratch output paths with content-addressed test objects. | This is the mechanism that makes Nix-provider tests green without evaluation or cache substitution: the fixture helper seeds the output first (`tests/support/jetpack_fixtures.rs:241-301`). It has no criteria. |
| #955, Epoch 4 dogfood portfolio | A real cold/cache/offline/rebuild cycle for the local Core provider package `hello@mine`. | Criterion 1 calls this the package-manager dogfood portfolio, but the test states that it uses only the local Core provider (`tests/jetpack_engine.rs:7122-7143`). Criteria 2-4 generalize that proof to the integrated portfolio and “no hidden dependency.” No nixpkgs package appears, so the evidence was too broad for what ran. |

The straight accounting is:

- The store, action graph, policy, receipts, diagnostics, task/tool UX, source adapters, and Core provider are substantive deliveries.
- Nix tests mostly use a synthetic JSON result plus pre-seeded bytes. They test the downstream pipeline and should remain, but their names must not be read as native nixpkgs coverage.
- #477 criterion 2 is a clear evidence mismatch. #955 criteria 1-4 are a scope inflation from one Core-provider portfolio test. #99, #179, #418, and #650 closed with no retained exit criteria.
- `no_nix_nixpkgs_package_reports_e1272` is an excellent negative test. The board lacked its positive complement.

The missing acceptance criterion was one sentence: “With a fresh writable Hangar, empty ambient tool path, no `/nix/store`, no fixture directory, and no `nix` executable, `jetpack enter` resolves `default.[ripgrep]`, verifies and stores its complete signed closure, executes it, restarts offline, and reports exact disk use.”

## External evidence: native nixpkgs ingestion

This section records primary-source evidence gathered on 2026-08-24. It separates Nix evaluation from binary-cache substitution.

### External result

Index-driven substitution is a valid first native slice. The NixOS channel and Hydra publish enough data to build an index off-device.

They do not publish a ready `attrpath + version + system -> store path` index. Jet must publish and sign that derived index.

A native evaluator remains the path to overlays, arbitrary flakes, uncached derivations, and full nixpkgs fidelity. Snix proves feasibility and cost.

### Live channel and cache evidence

The current `nixpkgs-unstable` channel redirects to an immutable release name that contains the nixpkgs revision. The release publishes these artifacts:

- `git-revision`: the exact nixpkgs commit.
- `nixexprs.tar.xz`: the Nix source tree.
- `store-paths.xz`: store paths in the channel release closure.

The old `binary-cache-url` artifact is not present on this channel. The checked URL returned HTTP 404, so Jet must set cache endpoints independently.

The binary cache publishes `nix-cache-info`, one `<store-hash>.narinfo` file per object, and the NAR named by that file. The [Nix binary-cache specification](https://nix.dev/manual/nix/2.35/protocols/binary-cache/index.html) defines this HTTP(S) interface.

These commands fetched the current official artifacts:

```text
$ curl -fsSL -o /dev/null -w 'effective_url=%{url_effective}\nhttp_code=%{http_code}\nsize_download=%{size_download}\n' https://channels.nixos.org/nixpkgs-unstable/store-paths.xz
effective_url=https://releases.nixos.org/nixpkgs/nixpkgs-26.11pre1059707.c8f90650c152/store-paths.xz
http_code=200
size_download=7929940

$ curl -fsSL -o /dev/null -w 'effective_url=%{url_effective}\nhttp_code=%{http_code}\nsize_download=%{size_download}\n' https://channels.nixos.org/nixpkgs-unstable/nixexprs.tar.xz
effective_url=https://releases.nixos.org/nixpkgs/nixpkgs-26.11pre1059707.c8f90650c152/nixexprs.tar.xz
http_code=200
size_download=38189700

$ curl -fsSL https://channels.nixos.org/nixpkgs-unstable/git-revision
c8f90650c15282fa8656a041bfbbd2403997a9a7

$ curl -fsSL https://channels.nixos.org/nixpkgs-unstable/binary-cache-url
curl: (22) The requested URL returned error: 404

$ curl -fsSL https://cache.nixos.org/nix-cache-info
StoreDir: /nix/store
WantMassQuery: 1
Priority: 40
```

The downloaded path list has 306,392 lines. It is 7,929,940 bytes compressed and 22,154,544 bytes uncompressed:

```text
$ xz --robot -l store-paths.xz
name    store-paths.xz
file    1       1       7929940 22154544        0.358   CRC64   0
totals  1       1       7929940 22154544        0.358   CRC64   0       1

$ wc -l < <(xz -dc store-paths.xz)
306392
```

The line count is not a package count. It includes outputs and dependency closure objects, and it contains no attrpaths or systems.

The release revision was `c8f90650c15282fa8656a041bfbbd2403997a9a7`. Hydra evaluation `1828345` names the same revision. Its JSON is 2,147,334 bytes and lists 214,696 build IDs. Hydra reports 904 seconds of evaluation time.

The official [Hydra API](https://github.com/NixOS/hydra/blob/master/hydra-api.yaml) exposes both `/eval/{eval-id}/builds` and individual build records. A build record supplies job, system, output name, and output path.

One live record maps the normal package job for ripgrep:

```text
$ curl -fsSL -H 'Accept: application/json' https://hydra.nixos.org/build/341884274 | perl -0777 -ne 'for $k (qw(job jobset system nixname)) { print "$k=$1\n" if /"$k":"([^"]+)"/ } print "out=$1\n" if /"buildoutputs":\{"out":\{"path":"([^"]+)"/'
job=ripgrep.x86_64-linux
jobset=unstable
system=x86_64-linux
nixname=ripgrep-15.2.0
out=/nix/store/axp6zlky4x2v3jwcbq24a2cz25hzlw9b-ripgrep-15.2.0
```

The same output appears in `store-paths.xz`. This proves that an off-device index producer can join Hydra jobs to channel paths.

Hydra job names are not a complete nixpkgs attrpath contract. The producer must normalize job names and retain exact nixpkgs attrpaths as separate fields.

#### Actual `.narinfo` and NAR

The output hash above resolves without a Nix daemon or Nix-specific transport:

```text
$ curl -fsSL https://cache.nixos.org/axp6zlky4x2v3jwcbq24a2cz25hzlw9b.narinfo
StorePath: /nix/store/axp6zlky4x2v3jwcbq24a2cz25hzlw9b-ripgrep-15.2.0
URL: nar/19yag7za8bz38dzxd7g20p8738bmb80n4ci9y3hfaxhy15rxxxyh.nar.zst
Compression: zstd
FileHash: sha256:1lhgjf25d2ca7sx1ka4g5lsskicr484vqi7cbndzhz598hbr18zy
FileSize: 2133450
NarHash: sha256:19yag7za8bz38dzxd7g20p8738bmb80n4ci9y3hfaxhy15rxxxyh
NarSize: 7088584
References: 0d8g8n0a11v6f5m2h416ajyxmnkwc3md-glibc-2.42-67 dsn500c5j62qz9f49mi3nhx74jbkf6xq-pcre2-10.47 r48746qznwqxxl9qzd8f08ny8mg1dg2y-gcc-15.3.0-lib
Deriver: iv6j10qg3d5j5m2nija24gzvph451r7a-ripgrep-15.2.0.drv
Sig: cache.nixos.org-1:u47N81GjFd/qpAQ8bRz3Ve584pYwp/gWswtHa6PwWSzhfYvw7oTBW0DThOzapKGuxqqnvw9HfKRnggOniyPBDw==
```

The NAR URL returned HTTP 200 with `Content-Type: application/x-nix-nar` and `Content-Length: 2133450`. That length matches `FileSize`.

The [`.narinfo` specification](https://nix.dev/manual/nix/2.35/protocols/binary-cache/narinfo.html) defines each field. The [Nix binary-cache manual](https://releases.nixos.org/nix/nix-1.11.9/manual/index.html) defines the Ed25519 signature fields.

Nix trusts `cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY=` by default. The [current trust configuration](https://releases.nixos.org/nix/nix-2.34.0/manual/command-ref/conf-file.html#conf-trusted-public-keys) publishes that key.

A native client must do these checks in this order:

1. Check the signed store path, NAR hash, NAR size, and references with the published Ed25519 key.
2. Fetch the relative NAR URL and check `FileHash` and `FileSize` before decompression.
3. Decompress with the declared codec and check `NarHash` and `NarSize`.
4. Repeat the process for every reference before it admits the closure.
5. Check a separate Jet signature on the attrpath index before package selection.

The cache signature proves that bytes belong to a genuine signed store object. It does not prove that an index selected the intended package.

Jet must sign the index because a stale or changed index can select another valid, signed object. The index also needs the channel revision and system.

#### Index coverage and limits

The index can cover successful Hydra jobs whose outputs remain in the configured caches. It cannot cover these cases:

- User overlays or package overrides.
- Arbitrary flake functions or nonstandard arguments.
- Attributes outside the Hydra release job set.
- Builds that Hydra did not schedule or complete.
- Outputs absent from all configured caches.
- Restricted or unfree artifacts that the public cache does not distribute.
- Local source inputs and impure evaluation.

No public artifact supports an honest percentage of all nixpkgs attributes. The 306,392 paths and 214,696 Hydra jobs use different denominators.

The first index release must report its own coverage. It should count indexed attrpaths, successful outputs per system, cache misses, and exclusions.

#### Refresh and disk cost

Use the channel revision as the refresh trigger. Poll the small `git-revision` object and publish a new immutable index only when it changes.

The public sources do not promise a fixed channel cadence. A daily poll is a product policy, not an upstream guarantee.

Measured metadata for this release is small enough for a native client:

- Path membership list: 7.93 MB compressed; 22.15 MB raw.
- Nix source tarball: 38.19 MB compressed. The index path does not need it on client machines.
- Hydra evaluation ID list: 2.15 MB raw JSON for 214,696 jobs.
- One ripgrep `.narinfo`: 657 bytes.
- Ripgrep root output: 2.13 MB compressed and 7.09 MB as NAR, before its three references.

Budget 15-40 MB compressed for a normalized multi-system attr index. This is a planning estimate, not a measured artifact.

Measure the first generator before setting a permanent budget. Package closures, not the index, will dominate local disk use.

After the signed index and full selected closure are local, the package works offline. A missing closure object causes an honest offline miss.

### Tvix and Snix: native evaluator prior art

Tvix became Snix in March 2025 when its maintainers moved the project to dedicated infrastructure. The [announcement](https://snix.dev/blog/announcing-snix/) describes the split and rename.

Snix has achieved substantial native Nix support:

- `snix-eval` parses Nix, compiles it to custom bytecode, and runs a lazy VM.
- `snix-glue` joins evaluator builtins to builder and store services.
- `snix-store` tracks store paths, NAR hashes, references, and signatures.
- `nar-bridge` implements a read-write Nix HTTP binary-cache endpoint.
- `snix-cli` evaluates selected nixpkgs attrpaths and can dispatch substitutions or builds.

The [component overview](https://snix.dev/docs/components/overview/) states these capabilities. It also says that `snix-cli` is not a `nix-build` replacement.

Snix has produced matching derivation paths for selected nixpkgs jobs. In issue [#164](https://git.snix.dev/snix/snix/issues/164), maintainers show matching `zzuf` derivation hashes for two systems.

That issue also records the current validation limit. Snix compares only a few attrpaths and lacks a whole-nixpkgs differential evaluator.

The [Snix status page](https://snix.dev/about/) is explicit: APIs remain unstable, and no full-featured drop-in Nix replacement exists.

#### What took time

Primary sources do not rank work by person-hours. They do show the long pole.

In September 2022, Tvix reported more than one year of rewrite work. It called the evaluator the largest area of progress during the prior six months.

That [status report](https://tvl.fyi/blog/tvix-status-september-22) said code-review capacity slowed progress. It still lacked most builtins, `rec`, and assembled store and builder implementations.

Four years later, Snix can evaluate important nixpkgs slices. It still lacks whole-tree differential proof and a complete replacement CLI.

The current [builtin status table](https://snix.dev/docs/components/eval/builtins/) marks 19 entries incomplete. Eleven await evaluator-to-store APIs, two await context support, and six remain `todo`.

This supports one planning inference: evaluator semantics alone are not the main finish line. Nixpkgs fidelity, string contexts, derivation-store effects, fetchers, and differential testing are the long pole.

#### Licence and reuse posture

Snix uses a deliberate licence split. Its [repository licence statement](https://git.snix.dev/snix/snix) says:

- All Snix crates use GPL-3.0.
- Protocol Buffer definitions use MIT.
- Linking or embedding the evaluator falls under GPL-3.0.
- Independent implementations can use the MIT protocols.

Jet cannot treat `snix-eval` or `nix-compat` as a permissive Rust library. Direct code reuse needs an owner and legal decision on GPL-3.0 compliance.

Jet can use Snix as executable prior art, a differential oracle, and a protocol reference. It can also reuse the MIT protocol definitions.

#### Cost implication

Snix proves that a Rust evaluator is feasible. It also sets a floor for cost: a volunteer team has worked on the rewrite since at least 2021.

For planning, allow 36-72 engineer-months for broad nixpkgs evaluation, derivation fidelity, fetchers, store effects, and differential proof. This excludes a general builder.

Allow 12-24 engineer-months for a narrower evaluator that resolves common nixpkgs attrs and this repository's flakes. These are estimates, not Snix measurements.

A stripped evaluator addition will likely cost tens of megabytes, not hundreds. Use 15-40 MB as a binary budget until a linked prototype gives a measurement.

Do not promise full fidelity from the narrow estimate. A production evaluator must track Nix language changes and nixpkgs behavior continuously.

### Strategy comparison from external evidence

| Strategy | Reach | Trust | Offline behavior | Refresh | Client disk and build size | Main failure |
|---|---|---|---|---|---|---|
| Native evaluator | Arbitrary nixpkgs expressions, overlays, and flakes after full fidelity | Pin nixpkgs input; verify fetched sources and substituted NARs | Can evaluate cached source; builds or downloads still need all inputs | Track Nix and nixpkgs continuously | Estimated 15-40 MB binary plus sources and closures | Multi-year semantic and differential-test cost |
| Signed index plus substitution | Successful indexed Hydra outputs in public caches | Verify Jet index signature and upstream Ed25519 NAR signatures | Full selected closure works; any missing object fails | Rebuild on channel revision; daily poll is sufficient policy | Measured upstream metadata is tens of MB; proposed index budget 15-40 MB | No overlays, arbitrary flakes, uncached, or restricted outputs |
| Ahead-of-time native definitions | Only translated definitions and supported recipe forms | Sign definitions; verify every source or NAR | Works when definitions and source or closure bytes are local | Regenerate for each channel revision | At least the index size; full definitions can approach source-tar size | General translation converges on implementing Nix evaluation |
| Hybrid | Indexed cache coverage first; evaluator expands fidelity later | Same signed index and NAR chain; later pin evaluator inputs | Immediate cached closure support; later native build support | One channel feed, then evaluator conformance updates | Index first; evaluator cost arrives in later phases | Must keep one package identity model across both paths |

External evidence supports the hybrid order. It does not support an evaluator-first dogfood date.

An ahead-of-time output translation is useful only when it means a signed index of realized outputs. Translating general Nix functions without evaluation cannot preserve overlays or arguments.

## Strategy assessment and cost

The estimates below use one engineer-month as one experienced Rust/Nix engineer working full time. They include implementation and focused production-path tests, but not elapsed queue time, legal review, or a general cross-platform build farm. Binary-size figures are incremental stripped-release planning budgets. The current debug `target/debug/jetpack` is 622,769,232 bytes and is not a useful release baseline.

### 1. Full native Nix evaluator in Rust

**Reach.** A genuinely complete evaluator, derivation model, fetch layer, and builder can approach all pure nixpkgs expressions, arbitrary overlays, overrides, and flakes. No honest percentage applies before a whole-nixpkgs differential corpus exists. Evaluation alone does not realize uncached outputs: a native builder is also required for packages absent from substituters.

**Cannot do initially.** The narrow 12-24 engineer-month slice cannot claim general nixpkgs fidelity. It will still miss evaluator/store builtins, import-from-derivation, dynamic derivations, impure inputs, some string-context behavior, and unsupported builders. Restricted sources remain subject to their licence and authentication rules.

**Trust.** Pin the nixpkgs source revision and content hash. Treat evaluation as local computation. For substituted outputs, verify the same upstream cache signatures and NAR hashes as the index path. For local builds, Jet's recipe, sandbox, provenance, receipt, and trust policies become authoritative.

**Offline.** Evaluation works offline when the complete source graph is present. Execution works only when every selected closure object or every build input and builder is present. “Can evaluate” is not “can realize.”

**Refresh.** Track Nix language and builtin behavior continuously and run differential tests against every supported nixpkgs revision. This is a permanent compatibility program, not a one-time parser project.

**Disk and binary.** Budget 15-40 MB of stripped binary growth for a broad evaluator and compatibility runtime, plus the 38.19 MB compressed nixpkgs source archive when local evaluation is used, plus package closures. A builder adds tools and sandbox assets beyond that budget.

**Cost and risk.** Budget 12-24 engineer-months for a deliberately narrow repo/common-attr evaluator and 36-72 engineer-months for broad evaluator, derivation, fetcher, store-effect, and differential fidelity. A general builder is additional. Snix is the serious prior art, but its unstable APIs and GPL-3.0 crate licence prevent casual embedding. Reuse needs an owner/legal decision; the MIT protocol definitions and Snix as an external oracle are safer immediate uses.

### 2. Signed index-driven substitution without evaluation

**Reach.** This reaches successful indexed channel jobs whose complete outputs remain in configured substituters. The current public data contains 214,696 Hydra build IDs and 306,392 channel closure paths, but neither is a count of nixpkgs attributes. Therefore a percentage would be fabricated. The first index must publish per-system counts for evaluated attrs, successful builds, cached outputs, exclusions, and misses.

For this repository, the first acceptance target is 28/28 current `env.jet`
selections (with the original 20-item list retained as a regression slice),
then 22/22 direct default-shell attrs, then every cacheable direct attr in the
48-item full shell. The audit does not assume those counts: the generator must
prove them against the pinned revision.

**Cannot do.** User overlays, overrides, arbitrary flake functions, local source inputs, unbuilt attrs, outputs evicted from all caches, and public-cache exclusions such as restricted artifacts. `jetR`, repo-local wrappers, shell hooks, and conditional environment composition are outside a plain attr index.

**Trust.** A Jet release key signs the immutable mapping `{channel revision, system, attrpath, version, output name, store path}`. The upstream cache key separately signs `{store path, NarHash, NarSize, references}`. Jetpack must verify both; either one alone is insufficient. Receipt admission must retain the index revision, key, upstream signature, and closure hashes.

**Offline.** After the signed index and full selected closure are local, selection and execution are offline. A missing reference is an explicit offline miss. Never silently fall through to the host Nix store.

**Refresh.** Poll `git-revision` daily, generate once per new immutable revision, verify output availability, sign, and publish atomically. Keep older indexes addressable while locks refer to them. Expiry/rollback policy should use Jet's existing receipt and trust machinery.

**Disk and binary.** Budget 15-40 MB compressed per multi-system index until measured. Do not ship `nixexprs.tar.xz` to index-only clients. The client needs native HTTPS, Nix base32/hash handling, Ed25519 fingerprint verification, and at least zstd decompression. Ed25519 is already linked; budget 3-10 MB of incremental stripped binary for native transport/compression and under 2 MB for index logic. Package closures dominate disk.

**Cost and risk.** Budget 4-8 engineer-months: 2-3 for standard cache protocol and recursive closure admission, 1-3 for the generator/feed/signing/observability, and 1-2 for provider integration and dogfood hardening. The main risks are attrpath/Hydra-job normalization, cache retention, platform variance, and a signed but semantically wrong index. Differential comparison with Nix in the off-device generator is mandatory.

### 3. Ahead-of-time translation

There are two distinct meanings:

1. Translating evaluated outputs to a signed store-path index is strategy 2 and scales well for cached channel packages.
2. Translating Nix source expressions into Jet-native recipes attempts to preserve functions, overlays, builders, and fetchers. At nixpkgs scale it converges on implementing a Nix evaluator plus a Nix-to-Jet semantic compiler.

**Reach.** A curated translator can target all 28 current `env.jet`
selections and selected common recipe families. It has no honest global
percentage until every skipped construct is counted. General nixpkgs
definitions use enough library indirection, overrides, platform policy, and
generated derivations that coverage will plateau without evaluator semantics.

**Cannot do.** Unknown functions, dynamic imports, arbitrary overlays, import-from-derivation, impure evaluation, bespoke builders, and semantics added upstream after the translator's last revision. Generated definitions also cannot manufacture a restricted source or a cache-missing output.

**Trust and offline.** Sign the source revision, translation tool identity, generated definition set, and every fetched source or NAR. Offline use needs definitions plus complete sources or output closures. A signature proves provenance, not semantic equivalence; the generator must compare selected derivation/output identities with Nix.

**Refresh, disk, and binary.** Regenerate at every channel revision. Client binary cost can stay under 5 MB if generated data drives existing Jet recipes. Budget 40-200 MB compressed for a broad generated definition corpus until measured; a curated repo bootstrap is far smaller. Stale generated definitions are a correctness bug, not merely old metadata.

**Cost and risk.** Budget 3-6 engineer-months for a deliberately curated repo bootstrap after native substitution exists. Budget 12-24 engineer-months for a broad recipe translator, followed by permanent maintenance. Do not hand-pin thousands of packages. Use this only for repo-local composition that the index cannot express, or as generated evidence feeding the index.

### 4. Hybrid, piecemeal transition

This is the recommendation:

1. Use a signed index for discovery of channel-built outputs.
2. Use one native standard-cache path for recursive substitution into the Hangar.
3. Use small Jet-native definitions for repo-local wrappers and environment composition.
4. Expand the existing evaluator behind the same locked package identity.
5. Add a native builder only when uncached/overlay demand justifies it.

**Reach.** The first release reaches exactly the measured index fraction. AOT definitions cover explicit composition gaps. Evaluator coverage grows without invalidating locks or adding another store. Eventual fidelity is the evaluator/builder target; the immediate release remains honest about index-only limits.

**Trust, offline, refresh, and size.** These are the union of the index and evaluator paths, but not duplicate mechanisms: one lock schema, one closure graph, one Hangar object format, one receipt, and one projection path. Initial client cost is the index/substitution 3-10 MB budget; evaluator size arrives only later. Index refresh follows the channel; evaluator conformance follows Nix and nixpkgs changes.

**Cost and risk.** The first useful cold realization is 4-8 engineer-months. Default-shell dogfood is roughly 7-12 engineer-months including integration and composition gaps. Full evaluator fidelity remains 36-72 engineer-months plus builder work. The architectural risk is allowing index, AOT, and evaluator results to become three identities. Prevent that by locking the same nixpkgs revision, system, attrpath, derivation path when known, named outputs, and upstream closure proof.

## Where Jetpack can beat Nix

Native nixpkgs ingestion should preserve Jetpack's stronger product surfaces instead of exposing Nix internals.

| Advantage | Existing evidence | Native nixpkgs application |
|---|---|---|
| Content-addressed, race-safe Hangar admission | Ingest is staged, no-follow, rehashed, and atomically published (`Store/Ingest.rs:491-585`). External Nix outputs are rehashed before projection (`Store.rs:713-799`). | NAR signatures authorize upstream bytes; the Hangar digest remains the local object identity. Do not make `/nix/store` the writable source of truth. |
| Typed environment surface | Packages are typed `Pkg` values (`crates/jet-env-model/src/ModuleEval/Types.rs:297-298`), as shown by the compact `env.jet:7-16`. | Users write `default.[ripgrep]`; index revision, output names, closure, and policy remain expert/audit facts rather than beginner ceremony. |
| Trust gating and rollback resistance | Cache receipts are admitted only after transport, NAR, and output identity checks (`Store/Cache.rs:619-649`); `binary_cache_trust_receipt_rejects_rollback_freeze_and_mix_and_match` passes (`tests/jetpack_engine.rs:645`). | Add index-selection and upstream-cache proofs to the receipt. Reject stale indexes, valid-object substitution attacks, and closure mix-and-match with specific recovery text. |
| Honest disk accounting | `hangar du` walks realized objects and distinguishes unique/shared bytes (`Store/Closure.rs:25-84`); the production test asserts honest source-built accounting (`tests/jetpack_engine.rs:7837-7868`). | Report index bytes, compressed transfer bytes, unpacked unique/shared closure bytes, and roots. Nix users should not need `nix-store --query --requisites` plus separate disk tools to understand cost. |
| Connected receipts and provenance | Immutable receipts record inputs, action, outputs, and closure (`Store/Closure.rs:1232-1404`); `connected_receipt_reaches_lock_and_fails_closed_on_corruption` passes (`tests/jetpack_engine.rs:2869`). | Preserve channel revision, index signature, attrpath, Hydra/build identity, `.narinfo` signatures, and each closure edge in one inspectable chain. |
| Product diagnostics | E1272 already says what is absent and why (`CLI/realize.rs:357-364`); diagnostics are snapshot-tested. | Replace E1272 with precise stages: index has no attr, output not cached, index signature invalid, narinfo signature invalid, compressed hash mismatch, closure reference missing, or platform unavailable. Never surface a Nix trace dump. |

## Ordered plan to dogfooding

The plan keeps the existing store and provider seams. Every proof uses a fresh writable Hangar, no fixture directory, an empty or controlled `PATH`, no `nix` executable, and no usable host `/nix/store`. Networked proofs run once; the same closure must then run offline.

| Step | What exists | Missing work and rough size | Closing proof |
|---|---|---|---|
| 1. Native standard-cache closure admission | Deterministic NAR codec, Ed25519 dependency, HTTP endpoint concept, Hangar CAS, closure graph, receipts. | Parse standard `.narinfo`; canonical Nix signature fingerprint; base64/Ed25519; separate compressed `FileHash` from unpacked `NarHash`; streaming native HTTPS; zstd first, then codecs observed in supported caches; recursive `References`; bounded parallel fetch; atomic closure admission. 2-3 engineer-months, 3-8 MB stripped. | A recorded local HTTP cache fixture and a live `cache.nixos.org` ripgrep smoke both fetch the exact signed root plus all references with `PATH` containing no `nix` or `curl`. Wrong key, changed signed field, compressed corruption, NAR corruption, missing reference, traversal, duplicate reference, and interrupted transfer fail closed. A second offline execution passes. |
| 2. Signed nixpkgs index producer and client | Flake/channel revision locks, trust keys, cache receipts, Hydra and channel public data. | Off-device join of exact channel revision, system, attrpath/job, version, named outputs, derivation and store paths; compare with Nix; availability check; deterministic format; Jet signature; immutable publication; rollback/expiry; delta or whole-index refresh; coverage report. 1-3 engineer-months, 15-40 MB compressed feed, under 2 MB client logic. | For revision `c8f90650…`, the generated record for `ripgrep.x86_64-linux` selects `/nix/store/axp6…-ripgrep-15.2.0`, and the output exists in the channel list and cache. Reordered input generates identical bytes. Forged, stale, cross-system, duplicate, ambiguous, and valid-signature/wrong-attr cases fail. Published metrics reconcile every Hydra build ID to indexed, failed, skipped, or missing-cache state. |
| 3. Wire `NixProvider` to index plus substitution | Ref parsing, source table, producer records, named outputs, Hangar reuse, E1272, leases and Linux projection. | Resolve locked source/channel + attr + system through the index; substitute complete closure; populate `Realized.references`; record upstream and index proof; use only Hangar snapshots at runtime; make projection independent of an existing host store. 1-2 engineer-months. | `default.[ripgrep]` in a minimal `env.jet` enters from a cold root and runs `rg --version` with no Nix and no fixture. Delete network access and restart: it runs offline. Delete one transitive object: offline fails by exact reference; online repairs only that object. |
| 4. Dogfood this repository's real `env.jet` | The 28-selection typed manifest already exists and the provider can batch holes. The E1317 provider-name bug is owned separately. | Make sure all 28 selections are indexed for the pinned system, fetch deduplicated closures, project loaders/libraries from the Hangar, preserve package order and PATH collision rules, and expose exact disk accounting. 1-3 engineer-months plus transfer time; closure disk must be measured, not guessed. | In a clean user namespace with no Nix store, `jetpack enter -- cargo build` proves the real compiler build, then runs a targeted test and all 28 command/version probes. A fresh process repeats offline. `hangar du` reconciles unique/shared bytes with filesystem usage. No fixtures, hand pins, or host tools satisfy a package. |
| 5. Reach `devShells.default` parity | `jetpack bridge flake` can project bounded package lists and record unsupported facts. Repo wrappers and hook behavior are explicit in `flake.nix`. | Add Jet-native definitions for `jetDev`/`jetpackDev`; model `JET_ROOT`, `TZDIR`, `JET_ENV_DISABLE`, the Linux loader path, and the one-time cleanup behavior through ratified typed environment mechanisms. This may need an owner syntax decision; do not smuggle shell-hook execution back in. 1-2 engineer-months after decisions. | Compare a declared environment manifest against the Nix oracle in CI, then run the same build/test probes with Nix absent. Unsupported hook facts are zero or explicitly owner-ratified replacements. |
| 6. Dogfood `.#full` | Index substitution can cover ordinary cached attrs. The bounded bridge understands named dev-shell outputs, simple overlays, `getBin`, and derivation identities as projections. | Cover all 48 Nix-derived selections; translate `jetR`, `getBin`, Linux conditionals, `makeLibraryPath`, wrappers, and hook state into canonical Jet mechanisms. Generate, do not hand-pin, store paths. Restricted/cache-missing items need a declared native-source or builder path. 2-4 engineer-months after step 4; closure size likely dominates and must be measured per system. | On Linux with no Nix/store, enter the full environment, enumerate every expected executable/library, and run the existing full FFI/browser/graphics/VM targeted sweep. Restart offline. Compare command inventory, environment, and selected output identities against `nix develop .#full` in an off-device oracle job. Unsupported platforms fail with named exclusions, never silent skips. |
| 7. Expand toward full evaluator fidelity | Bounded lazy evaluator, derivation path calculus, project import authority, differential fixtures, Snix/Nix prior art. | Owner/legal decision on Snix GPL reuse; full grammar and value semantics; builtins and string contexts; real nixpkgs source graph; fetch/store effects; derivation graph; whole-corpus differential harness; later native builder for uncached outputs. 12-24 engineer-months narrow, 36-72 broad, builder extra; 15-40 MB stripped evaluator budget. | Publish per-revision differential counts. Every indexed repo attr evaluates to the same derivation and output identities as Nix. Overlays and custom flakes enter only when their evaluated graph and closure agree. Remove index-only limitation claims only when the measured corpus supports it. |

Step 4 is the first requested dogfood target: the repository's actual `env.jet`. Step 6 is the second: `.#full`. Step 5 is separated because `env.jet` intentionally omits behavior that the default flake shell still supplies (`env.jet:1-6`); package realization alone cannot claim shell parity.

## Proposed Tower cards

Do not mint these from this report. Proposed order and one-line scopes:

1. **Jetpack native Nix binary-cache admission** — Verify standard signed `.narinfo`, stream/decompress/hash NARs, fetch transitive references, and atomically admit a complete closure without `nix` or `curl`.
2. **Signed nixpkgs channel index pipeline** — Generate, differentially verify, measure, sign, publish, refresh, and retain immutable attr/system/version/output-to-store-path indexes from channel and Hydra data.
3. **Index-backed NixProvider realization** — Resolve locked nixpkgs refs through the signed index, substitute into Hangar, record closure/provenance, and reuse offline through the existing provider contract.
4. **Host-store-independent Nix closure projection** — Run Hangar-backed Nix outputs with their exact loaders and libraries when `/nix/store` is absent, with Linux proof and explicit unsupported-platform behavior.
5. **Jet repository `env.jet` no-Nix dogfood gate** — Cold-realize all 28 current selections, build/test Jet, restart offline, and reconcile exact closure disk use on a machine with no Nix.
6. **Jet default/full shell semantic translation** — Replace repo-local wrappers, `jetR`, hooks, output selection, platform conditionals, and library-path composition with ratified Jet-native environment mechanisms; prove default then `.#full` parity.
7. **Native Nix evaluator conformance program** — Decide Snix reuse/licence posture, expand the bounded evaluator against a whole-nixpkgs differential corpus, and publish measured coverage and unsupported semantics.
8. **Native Nix derivation builder** — After evaluator demand is measured, realize uncached derivations and overlays inside Jet's sandbox with the same Hangar identity, receipts, and diagnostics.

Cards 1-5 form the immediate dogfood milestone. Cards 7-8 are the long fidelity program. Card 6 may require owner ballots for user-visible environment syntax; the cache/index cards do not authorize new Jet syntax.

## Decision

Proceed with the hybrid in this order: standard-cache substitution, signed index, provider wiring, real `env.jet` dogfood, default/full shell composition, then evaluator and builder fidelity. Reject any proposed close that uses an installed `nix`, a pre-seeded fixture, a host `/nix/store` lower layer, or per-package hand pins as the production proof.
