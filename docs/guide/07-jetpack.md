# Jetpack — temporary & project environments

**Jetpack** is Jet's package manager. Phase 1 is a Nix-`shell`/`devenv`-class
**temporary environment**: ask for a package, get dropped into a beautiful
shell with it available, and `exit` leaves your machine untouched.

Jetpack is a separate binary from `jet` (you run `jetpack ...` directly). It
owns the package lifecycle; Nix is a *compatibility provider* it taps to reach
the huge nixpkgs collection.

## A first run

```
$ jetpack run nixpkgs:fastfetch

  jetpack  resolving nixpkgs:fastfetch …
  jetpack  fastfetch ready ✓
           ▸ /nix/store/…-fastfetch-2.21.0

  entering a temporary shell — type `exit` to leave, nothing is installed.

jetpack ~/work $ fastfetch --version
fastfetch 2.21.0
jetpack ~/work $ exit

  left the temporary shell. your machine is unchanged.
```

The prompt clearly shows `jetpack` so you always know you're inside a temporary
environment. bash, zsh, and fish are all supported — Jetpack decorates whichever
shell `$SHELL` names.

## Refs: `<source>:<package>`

Every package is named `<source>:<package/path>`. You never type Nix's `#`.

| Ref | Means |
|---|---|
| `nixpkgs:fastfetch` | the `fastfetch` package from nixpkgs |
| `github:owner/repo` | a Jet pack repo (or a translated `flake.nix` fallback) |
| `path:./my-env` | a local pack/flake directory |

## Commands

```
jetpack run   <source>:<package>        enter a temporary shell with that package
jetpack run   <source>:<package> -- cmd run a command in that env, then exit
jetpack run                            enter the shell described by ./env.jet
jetpack build [<source>:<package>]     realize a package/environment, don't enter
jetpack list                           show realized packages
jetpack clean                          drop unused store records
jetpack add    <source>:<package>      add a package to ./env.jet
jetpack remove <source>:<package>      remove a package from ./env.jet
```

`jetpack run <ref> -- cmd…` is the one-shot form: it runs `cmd` inside the
environment and exits with the command's status, never opening a shell.

## The env file (`env.jet`)

A project directory can describe its environment in `env.jet` — Jet's
equivalent of `flake.nix`. `jetpack run` with no ref enters the shell it
describes; `jetpack add/remove` edit it.

```jet
// env.jet — a Jetpack dev environment (Jet's flake equivalent)
import jetpack as pkg;

pub fn shell() -> [JSON] {
    return [
        pkg.source("nixpkgs");
        pkg.packages(["claude-code", "fd", "ripgrep"]);
        pkg.prompt("jetpack");
    ];
}
```

When `env.jet` exists it takes priority over a `flake.nix`; a `flake.nix`
fallback is translated by Jetpack.

### Named sources

A pack file can declare **named sources** and use them inline with the same
`source:package` syntax. This lets one project pull from, say, a pinned stable
nixpkgs and the unstable channel at once:

```jet
import jetpack as pkg;

pub fn shell() -> [JSON] {
    return [
        pkg.source("stable",   "github:NixOS/nixpkgs/nixos-24.05");
        pkg.source("unstable", "github:NixOS/nixpkgs/nixpkgs-unstable");
        pkg.packages([
            "stable:ripgrep",   // from the 24.05 pin
            "unstable:neovim",  // from unstable
        ]);
    ];
}
```

The built-in source names `nixpkgs`, `github`, and `path` always work without a
declaration. A one-argument `pkg.source("nixpkgs")` sets the default source for
bare (unprefixed) package entries. An unknown source name is a friendly error
that lists the sources the pack declares.

### First-party Jet packages (no Nix)

A named source can point at a **Jet package repo** built by Jetpack's own
first-party `core` provider — no Nix required. Add `"core"` as the third
argument to select it:

```jet
pkg.source("mine", "path:../jet-pkgs", "core");
pkg.packages(["mine:hello"]);
```

The repo at that path has its own `env.jet` declaring what it provides:

```jet
// jet-pkgs/env.jet
pkg.package("hello", "./pkgs/hello");   // name → source subpath
```

Jetpack fetches that source repo when needed, copies the package tree into the
store (content-addressed), and puts its `bin/` on PATH. This is the path Jet's
own package ecosystem grows on; the Nix provider stays available for nixpkgs in
the meantime. The `core` provider supports `path:`, `github:owner/repo[#ref]`,
and git URL sources.

## Where things live

Jetpack's store and state live under `/etc/jet/` and `/etc/jet/store/`. If those
aren't writable (the usual case for a normal user), Jetpack falls back to a
dev-mode root under `$XDG_STATE_HOME/jet` (or `~/.local/state/jet`) and says so.
Set `JETPACK_ROOT` to choose the root explicitly.

## Offline & color

- `--offline` (with `--fixtures <dir>` or `JETPACK_FIXTURES`) resolves from
  captured `nix build --json` fixtures instead of the network — this is how
  Jetpack's own tests run without Nix.
- `--no-color` (or `NO_COLOR`) turns off styling; output is also plain when not
  writing to a terminal.

## What needs Nix

Only the Nix provider needs `nix` installed. If it's missing when you ask for a
nixpkgs package, Jetpack says so and points you at the installer — it never
shows you a raw Nix error.

---

*Phase 1 scope and decisions: `docs/plans/jetpack-jetos/README.md` and the
`D-JPK*` rows in `docs/spec/syntax-decisions.md`. Phase 2 (jetos, the
declarative distro) builds on top of Jetpack.*
