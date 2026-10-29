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
| `jet-runtime/` | The prebuilt runtime pack the compiler links programs against; it must sit beside `jet` |
| `jetpack` | The package-manager engine built by the same compiler (when the release has one) |
| `IDENTITY` | `jet@<version>#<sha256 of jet>`, the identity the compiler reports and stamps into builds |
| `SHA256SUMS` | The digest of every file above |

`Tools/agent/release-compiler.mjs` assembles one for every `a0_NNN` release
only after the chain below reaches its fixed point with no Rust on `PATH`.
Its stage-2 compiler becomes the seed of the next release.

## The chain

Run it in the Nix development environment (`Tools/agent/jet-env` or
`nix develop`) from a checkout of the commit you want to build. `SEED` is the
seed directory, `WORK` a scratch directory outside the checkout, and
`JET_PACK_MUSL_LIB` the `lib` directory of a static musl
(`$(nix build --no-link --print-out-paths nixpkgs#pkgsStatic.musl)/lib`).

```sh
export JET_TOOLCHAIN_ROOT=$PWD JET_COMPILER_IDENTITY=$(cat "$SEED/IDENTITY")
"$SEED/jet" build Compiler                 # s1, written to Compiler/JetCli/.jet/build/jet
mkdir -p "$WORK/s1" && cp Compiler/JetCli/.jet/build/jet "$WORK/s1/"
node Tools/packaging/prebuild-runtime.mjs "$WORK/s1/jet" "$WORK/s1/jet-runtime" --repo "$PWD"
"$WORK/s1/jet" build Compiler              # s2, built by s1 with s1's own runtime pack
```

Copy `s2` out and give it a runtime pack the same way, then build `s3` with
it, setting `JET_COMPILER_IDENTITY` to `jet@<version>#<sha256 of s2>`. The
build is trusted when `s2` and `s3` are byte-identical. Every release proves
that fixed point. The trusting-trust check compares the `s2` of two chains
that start from different seeds, for example a published seed and one rebuilt
from the `rust` branch.

The jump from a seed to the commit you want must stay small, because a seed
only builds sources its own language level accepts. To cross many releases,
walk the release tags forward. Check out each `a0_NNN` tag in order, and build
it with the previous tag's `s2` as its seed.

## Where the first seed comes from

1. **A local release.** `~/.local/share/jet-seeds/<a0_NNN>` holds every release
   cut on this machine; `current` points at the admitted one.
2. **A published release.** Each `a0_NNN` tag's GitHub release carries
   `jet-a0_NNN-<platform>-<arch>.tar.gz` and its `.sha256`. The archive is the
   seed directory:

   ```sh
   gh release download a0_NNN -R jet-language/jet -p 'jet-a0_NNN-linux-x64.tar.gz*'
   sha256sum -c jet-a0_NNN-linux-x64.tar.gz.sha256
   mkdir seed && tar -xzf jet-a0_NNN-linux-x64.tar.gz -C seed && (cd seed && sha256sum -c SHA256SUMS)
   ```

   Then check out the `a0_NNN` tag and run the chain with `SEED=seed`.
3. **The `rust` branch.** The last commit that still contains the Rust
   implementation (`6083377777`) is kept as the frozen `rust` branch (see
   `AGENTS.md`). It carries the Rust `jet` (`cargo build --release --bin jet`)
   and the stage-zero tooling (`Tools/stage0-repack/`) that produced the
   self-hosted compilers before the release train existed. It needs a Rust
   toolchain and no Jet binary. Use it only when no release seed survives.
   Then walk the release tags forward from the first `a0_NNN` after the branch
   point.
