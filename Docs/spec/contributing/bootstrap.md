# Rebuilding Jet from the repository

The Jet compiler under `Compiler/` is written in Jet, so building it needs an
existing `jet`: a *seed*. This page shows how to get a seed and build forward
from it when no working compiler is at hand. Every route ends in the same
self-host chain; they differ only in where the first seed comes from.

## What a seed is

A seed is one directory:

| Entry | Holds |
|---|---|
| `jet` | The compiler, a static x86-64 Linux executable |
| `jet-runtime/` | The prebuilt runtime pack; `jet` carries it built in, and only an unembedded `jet-bare` reads it |
| `jetpack` | The package-manager engine built by the same compiler (when the release has one) |
| `IDENTITY` | `jet@a0.0.N#<digest>`, the identity the compiler reports and stamps into builds, which `jet` also carries; the digest is the sha256 of `jet` with its `jet.identity` part zeroed ([`core-record --embed`](../reference/cli.md)) |
| `VERSION` | `a0.0.N <commit> <YYYY-MM-DD>`, the release stamp `jet --version` reports |
| `SHA256SUMS` | The digest of every file above |

`jet run Tools/release/Main.jet -- admit` assembles one for every `a0.0.N` release
only after the chain below reaches its fixed point with no Rust on `PATH`.
Its stage-2 compiler becomes the seed of the next release. The release tags
through `a0_042` are spelled `a0_NNN`; the release after it is `a0.0.43`.

## From a fresh clone

A clone and the latest published release are enough: no
`Tools/agent/JetEnv.jet`, nothing in `HOME` and no Jet environment variables,
only `git`, `curl`, `jq` and OpenSSL 3; `nix develop` supplies all of them. A
release tag whose own copy of this page also names a C compiler and archiver
(`cc` and `ar`), `nm` or static musl needs those too. This fetches the latest release, checks its signed
record, clones the release's tag, builds that compiler and its runtime pack
with the release's seed, runs one row
of the UI suite with it, runs an example, lands a patch on a scratch copy of
the checkout and validates a ballot. `PATCH` is a `git diff` of
your change, `MSG` a file with its commit message, `BALLOT` a ballot
payload, and `NAME` and `EMAIL` your git identity.

```sh
mkdir release && cd release
curl -fsSL https://api.github.com/repos/jet-language/jet/releases/latest | jq -r '.assets[].browser_download_url | select(test("/(jet-a0[.]0[.][0-9]+-linux-x64[.]tar[.]gz|CANDIDATE[.]json([.]sig)?|SHA256SUMS)$"))' | xargs -n1 curl -fsSLO
TAG=$(jq -r .name CANDIDATE.json)
git clone --branch "$TAG" https://github.com/jet-language/jet ../jet
test "sha256:$(jq -cS 'del(.content_digest)' CANDIDATE.json | tr -d '\n' | sha256sum | cut -d' ' -f1)" = "$(jq -r .content_digest CANDIDATE.json)"
jq -j .content_digest CANDIDATE.json > digest && openssl base64 -d -A -in CANDIDATE.json.sig -out signature
openssl pkeyutl -verify -pubin -inkey ../jet/Tools/release/jet-release.pub.pem -rawin -in digest -sigfile signature
jq -r '.artifacts.release[] | "\(.sha256)  \(.path)"' CANDIDATE.json | sha256sum -c --ignore-missing
mkdir ../seed && tar -xzf "jet-$TAG-linux-x64.tar.gz" -C ../seed && (cd ../seed && sha256sum -c --quiet SHA256SUMS)
cd ../jet && SEED=$(realpath ../seed)
"$SEED/jet" build Compiler                     # writes Compiler/JetCli/.jet/build/{jet,jet-runtime,IDENTITY}
export PATH=$PWD/Compiler/JetCli/.jet/build:$PATH
jet test Compiler --profile ui tests/ui/list_expected_type_mismatch.jet
jet run Examples/Basics/Hello.jet
git config user.name "$NAME" && git config user.email "$EMAIL"
jet Tools/agent/Land.jet land --scratch-copy ../dev-copy "$PATCH" "$MSG"
jet Tools/agent/ballots/Validate.jet "$BALLOT"
```

The `curl` line is the whole fetch: the GitHub API names the latest
release's assets, and it downloads the seed archive, `CANDIDATE.json`, its
signature and `SHA256SUMS`. `CANDIDATE.json` is the release's signed record
(`Tools/release/Record.jet`): its `content_digest` is the SHA-256 of the record
without that field, as sorted compact JSON, and `CANDIDATE.json.sig` is the
base64 Ed25519 signature of that digest by the release key, whose public half
is `Tools/release/jet-release.pub.pem`. The record lists every asset with its
SHA-256, and the archive's own `SHA256SUMS` covers each file of the seed. If a
check fails, stop: the downloaded `jet` is not what the release signed.
Releases tagged before the key was committed (a0_030 and earlier) don't carry
it; check those with the key file of a later checkout.

`jet` reads the Core library from the checkout that holds its input (outside
a checkout, from the Core record built into it), and its identity from the
one built into it; `JET_TOOLCHAIN_ROOT` and
`JET_COMPILER_IDENTITY` override both ([Environment
variables](../reference/environment.md)). `jet build Compiler` builds
`jet-bare`, with it the runtime pack it lowers (`jet-runtime/`), and then
`jet`: `jet-bare` with the Core record, the runtime and its identity built
in, and that identity in `IDENTITY` beside it. The pack holds no C, and the
build compiles no C. The testkit
keeps its scratch in `$JET_SCRATCH_ROOT/testkit`, else `~/.cache/jet/testkit`.

A seed released before these defaults finds neither; run its build with
`JET_TOOLCHAIN_ROOT=$PWD JET_COMPILER_IDENTITY=$(cat "$SEED/IDENTITY")`.

`Land.jet land --scratch-copy DIR` clones the checkout into `DIR`, which must
not exist yet, with the checkout's git identity. It applies the patch, runs
the lane check (`Tools/agent/LaneCheck.jet`: the whole compiler and Jetpack's
assembly when the patch touches `Compiler/`, `Core/` or `Jetpack/`, else each
Jet file it touches) and commits it on `DIR`'s branch. A patch
that does not apply or does not check leaves `DIR` as it was cloned. Several
`PATCH MSG` pairs land in order, one commit each. `Validate.jet` reads only the
payload, with the rules Tower applies when a ballot is posted
([Tools/agent/ballots/README.md](../../../Tools/agent/ballots/README.md)).

## The chain

Run it from a checkout of the commit you want to build, in the Nix
development environment (`Tools/agent/JetEnv.jet` or `nix develop`): each
`jet build Compiler` also makes the runtime pack the new compiler lowers. Each
stage builds at `--optimize=basic`, the level the release stages use. `SEED` is
the seed directory and `WORK` a
scratch directory outside the checkout.

```sh
"$SEED/jet" build --optimize=basic Compiler     # s1: Compiler/JetCli/.jet/build/{jet,jet-runtime,IDENTITY}
mkdir -p "$WORK" && cp -r Compiler/JetCli/.jet/build "$WORK/s1"
"$WORK/s1/jet" build --optimize=basic Compiler  # s2, built by s1 with s1's own runtime pack
```

The build writes `IDENTITY` (`jet@a0.0.N#<digest>`) beside the
compiler it makes, so each stage runs under its own identity. Copy `s2` out
the same way and build `s3` with it. The build is trusted when `s2` and `s3`
are byte-identical, and the runtime pack `s2` builds is byte-identical to the
one `s1` built for it. Every release proves that fixed point. The
trusting-trust check compares the `s2` of two chains that start from
different seeds, for example a published seed and one rebuilt from the
`rust` branch.

The jump from a seed to the commit you want must stay small, because a seed
only builds sources its own language level accepts. To cross many releases,
walk the release tags forward. Check out each release tag in order, and build
it with the previous tag's `s2` as its seed.

## Where the first seed comes from

1. **A local release.** `~/.local/share/jet-seeds/<release>` holds every release
   cut on this machine, named by its tag; `current` points at the admitted one.
2. **A published release.** Each `a0.0.N` tag's GitHub release carries the
   seed archive `jet-a0.0.N-<platform>-<arch>.tar.gz` and its `.sha256`, the
   seed's `jet` and `IDENTITY`, `SHA256SUMS`, and the signed record
   `CANDIDATE.json` with its signature `CANDIDATE.json.sig`. The archive is the
   seed directory. [From a fresh clone](#from-a-fresh-clone) fetches and checks
   the latest; for an older one, download the same assets from
   `https://github.com/jet-language/jet/releases/download/<tag>/` and run the
   same checks, then check out that tag and run the chain with `SEED=seed`.
   `jet run Tools/release/Main.jet -- admit` publishes every release it admits: it
   tags the commit, then runs `jet run Tools/release/Main.jet -- candidate DIR`,
   which records and signs `CANDIDATE.json` with the key
   `jet registry keygen --registry jet-release` made, and
   `jet run Tools/release/Main.jet -- publish DIR`, which uploads the assets.
3. **The `rust` branch.** The last commit that still contains the Rust
   implementation (`6083377777`) is kept as the frozen `rust` branch (see
   `AGENTS.md`). It carries the Rust `jet` (`cargo build --release --bin jet`)
   and the stage-zero tooling (`Tools/stage0-repack/`) that produced the
   self-hosted compilers before the release train existed. It needs a Rust
   toolchain and no Jet binary. Use it only when no release seed survives.
   Then walk the release tags forward from the first release after the branch
   point.
