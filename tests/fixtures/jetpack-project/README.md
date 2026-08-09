# Jetpack example environment

A minimal Jetpack project: an `env.jet` that declares three **named sources**
(D-JPK17) and references packages from them inline, plus offline provider
fixtures so the commands run without Nix or a network.

## Files

- `env.jet` — the environment (Jet's `flake.nix` equivalent), using named
  sources `stable`, `unstable`, and a first-party `core` repo `mine`.
- `jetpack.toml` — repo metadata and source defaults. Package member indexes
  live in `workspace.jet` in monorepos, not here.
- `jet-pkgs/` — a first-party Jet package repo (no Nix) whose `package.jet`
  `packages:` block declares the `hello` package (an `executable`) that
  `hello@mine` resolves to; the package is the `module hello` discovered by name
  in its source tree (U10).
- `functional-env.jet` — an alternative, fully functional sketch of the same
  environment (a runnable Jet program that prints the directive list).
- `fixtures/*.json` — captured `nix build --json` output, used by `--offline`.
  Named-source fixtures are keyed by the source name, e.g. `stable-ripgrep.json`.

## Try it (offline, no Nix required)

```
$ cd tests/fixtures/jetpack-project
$ JETPACK_FIXTURES=fixtures jetpack build --offline

  jetpack  resolving ripgrep@stable …
  jetpack  ripgrep ready ✓
           ▸ /nix/store/…-ripgrep-14.1.0
  jetpack  resolving neovim@unstable …
  jetpack  neovim ready ✓
           ▸ /nix/store/…-neovim-0.10.2
  jetpack  resolving hello@mine …
  jetpack  hello ready ✓
           ▸ …/store/hello-…           (core � no Nix)
  jetpack  built 3 package(s).
```

`jetpack build`/`jetpack run` (no ref) read `env.jet` and resolve everything it
declares — `ripgrep@stable` against the `nixos-24.05` pin, `neovim@unstable`
against the unstable channel, and `hello@mine` through the first-party `core`
provider (no Nix). `jetpack add fd@unstable` / `jetpack remove fd@unstable`
edit `env.jet` in place, preserving the source declarations.

## Online

With Nix installed, drop `--offline`/`JETPACK_FIXTURES` and Jetpack resolves
through the real Nix provider:

```
$ jetpack run fastfetch@nixpkgs
```

See `docs/guide/07-jetpack.md` for the full command surface.
