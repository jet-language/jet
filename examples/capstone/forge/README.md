# Forge

Forge is a Nix-backed development environment and task runner written in Jet.
It reads `demo/forge.json`, resolves declared tools through Nix build JSON, and
runs task plans in dependency order.

`forge.env.jet` is the Jet-native environment spec. Edit that file the way you
would edit a small `flake.nix`, then run `./build/main sync` or
`./build/main shell` to regenerate `demo/forge.json` and the `[tool.forge]`
metadata in `jet.toml`.

## Verified Commands

From `examples/capstone/forge`:

```sh
nix develop -c env JET_ROOT=/Users/nathanbrown/Documents/GitHub/jet jet test packages/ansi/ansi.jet
nix develop -c env JET_ROOT=/Users/nathanbrown/Documents/GitHub/jet jet test packages/manifest/manifest.jet
nix develop -c env JET_ROOT=/Users/nathanbrown/Documents/GitHub/jet jet test packages/taskrunner/taskrunner.jet
nix develop -c env JET_ROOT=/Users/nathanbrown/Documents/GitHub/jet jet test packages/nixbridge/nixbridge.jet
nix develop -c env JET_ROOT=/Users/nathanbrown/Documents/GitHub/jet jet build main.jet
```

Run the built app for CLI flags such as `--no-color`; the `jet run` launcher
currently consumes `--...` flags before the program sees them.

```sh
nix develop -c ./build/main list --no-color
nix develop -c ./build/main plan build --no-color
nix develop -c ./build/main run build --no-color
nix develop -c env PATH=/usr/bin ./build/main env --no-color
nix develop -c env PATH=/usr/bin ./build/main doctor --no-color
```

## Ergonomic Use

Run one nixpkgs package without editing a Nix file:

```sh
./build/main use jq -- jq --version
./build/main use ripgrep -- rg --version
./build/main use nodejs_22 -- node --version
```

Create a project tool environment from `demo/forge.json`:

```sh
./build/main shell
source build/forge-env.sh
```

Or start a subshell from the generated wrapper:

```sh
bash build/forge-shell
```

`use` and `shell` resolve real nixpkgs outputs by default. The deterministic
`env` and dry-run commands still use offline fixtures unless `--online` is
passed, which keeps the capstone tests reproducible.

After editing `forge.env.jet`, sync the generated files directly:

```sh
./build/main sync
```

The deterministic expected outputs live in `demo/expected/`. `run` and `env`
default to offline fixture mode, so they do not need network access or a real
Nix build.
