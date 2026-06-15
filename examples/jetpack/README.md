# Jetpack example environment

A minimal Jetpack project: a `pack.jet` that declares two **named sources**
(D-JPK17) and references packages from them inline, plus offline provider
fixtures so the commands run without Nix or a network.

## Files

- `pack.jet` — the environment (Jet's `flake.nix` equivalent), using named
  sources `stable` and `unstable`.
- `fixtures/*.json` — captured `nix build --json` output, used by `--offline`.
  Named-source fixtures are keyed by the source name, e.g. `stable-ripgrep.json`.

## Try it (offline, no Nix required)

```
$ cd examples/jetpack
$ JETPACK_FIXTURES=fixtures jetpack build --offline

  jetpack  resolving stable:ripgrep …
  jetpack  ripgrep ready ✓
           ▸ /nix/store/…-ripgrep-14.1.0
  jetpack  resolving unstable:neovim …
  jetpack  neovim ready ✓
           ▸ /nix/store/…-neovim-0.10.2
  jetpack  built 2 package(s).
```

`jetpack build`/`jetpack run` (no ref) read `pack.jet` and resolve everything it
declares — `stable:ripgrep` against the `nixos-24.05` pin, `unstable:neovim`
against the unstable channel. `jetpack add unstable:fd` /
`jetpack remove unstable:fd` edit `pack.jet` in place, preserving the source
declarations.

## Online

With Nix installed, drop `--offline`/`JETPACK_FIXTURES` and Jetpack resolves
through the real Nix provider:

```
$ jetpack run nixpkgs:fastfetch
```

See `docs/guide/07-jetpack.md` for the full command surface.
