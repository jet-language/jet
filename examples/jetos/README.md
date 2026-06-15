# jetos — a declarative system, configured in Jet

> Your whole machine in a few small files, a merge engine that resolves them
> the way Nix's module system does, and a CLI that feels like `nh`. Written
> entirely in Jet.

This is the Jet answer to **Nix flakes + the NixOS/home-manager module system**.
You describe *what* you want (programs, services, settings) in dendritic,
liftable feature files; jetpack merges them into one canonical system tree;
`jetos` shows you the diff and makes it live, with generations you can roll
back to. Nix stays the package *provider* (jetos shells `nix build` to realize
store paths); **jetos is the experience.**

```
config.jet ──(jet)──▶ merged system tree (canonical JSON) ──▶ jetos ──▶ nix build
   modules/**            jetpack merge engine                  diff /        store paths
   hosts/<name>          (lists combine, scalars               switch /      + generations
                          resolve by priority)                 rollback
```

## Try it (60 seconds)

From the repo root, inside the Nix dev shell:

```sh
nix develop -c jet build examples/jetos/config.jet   # the evaluator
nix develop -c jet build examples/jetos/jetos.jet    # the CLI
cd examples/jetos
JETOS_HOST=laptop ../../build/config > state/system.json   # evaluate the config

../../build/jetos list           # the merged system
../../build/jetos diff           # what would change vs the live generation
../../build/jetos switch         # build it + write a generation (offline)
../../build/jetos generations    # save-slots for your whole machine
```

Verify everything end to end (unit tests + evaluation + CLI golden output):

```sh
nix develop -c bash examples/jetos/demo/verify.sh
```

## What the config looks like (the whole point)

A feature lives in one liftable file. Copy it into any config repo and it just
works, because a module talks to the rest of the system **only** through the
directives it returns — never by importing other modules.

```jet
// modules/apps/desktop.jet — everything "desktop", in one file.
import jetpack as pkg;
import std.json as json;

pub fn contribute() -> List<Json> {
    return [
        pkg.installed(["firefox", "fastfetch", "btop", "vlc"]),
        pkg.services(["pipewire", "bluetooth"]),
        pkg.suggest("sys.desktop.environment", "gnome"),   // a polite default
    ];
}
```

A host is just a module that decides things for one machine:

```jet
// hosts/laptop.jet
pub fn contribute() -> List<Json> {
    return [
        pkg.option("sys.desktop.environment", "cinnamon"),  // normal > the suggestion
        pkg.option("sys.networking.hostName", "laptop"),
        pkg.installed(["tlp", "powertop"]),
    ];
}
```

`config.jet` itself stays tiny — it just picks the source and emits the tree:

```jet
import jetpack as pkg;
import "generated/tree" as tree;

fn main() {
    val source = "github:NixOS/nixpkgs/nixos-unstable";
    val host = pkg.target_host();
    pkg.emit(source, host, tree.directives(host));
}
```

## The merge rules (the whole referee)

| Kind | When two modules touch it |
|------|---------------------------|
| package / service lists | **combine** — concatenated, de-duplicated, sorted (so the result never depends on file-discovery order) |
| scalar option | resolved by **priority**: `suggest` (default) < `option` (normal) < `force`. One value per level — two distinct values at the same level is a **conflict**, reported with both files. |

A conflict is a feature, not a failure — it is the moment a lifted module and
your setup disagree, surfaced loudly in the Jet diagnostic voice:

```
conflict: option `sys.desktop.environment` is set to different values
  modules/apps/kde.jet = plasma
  hosts/laptop.jet = cinnamon
why: a scalar option takes one value per priority level
fix: mark one `default`, or `force` the one you want
```

## Dendritic & import-tree

* **Dendritic** — files are organized by *feature*, not by machine. One
  `firefox.jet` carries the package, the service, and the setting together.
* **Import-tree** — you never maintain an import list. `jetos sync` discovers
  every `.jet` under `modules/` and `hosts/` and regenerates `generated/tree.jet`.
  Prefix a file with `_` to park it (see `modules/apps/_draft.jet`).
* **Liftable** — a module's entire interface is the directives it emits, so
  copying one between repos is safe and statically checkable.

## CLI (the `nh`-style experience)

| Command | What it does |
|---------|--------------|
| `jetos check` | evaluate + validate the config; no side effects |
| `jetos list` | show the merged system tree |
| `jetos diff` | colored `+`/`-`/`~` diff vs the live generation |
| `jetos switch` / `build` | realize store paths and write a new generation |
| `jetos generations` | list save-slots for the whole machine |
| `jetos sync` | re-wire `modules/` into the import-tree |
| `jetos help` | the help screen |

Flags: `--no-color` (auto-off when piped), `--online` (hit real nixpkgs;
otherwise resolves from `nix/fixtures/` offline so the demo is deterministic).
`switch` degrades gracefully when `nix` is absent.

## Honest scope

* jetpack **orchestrates** Nix; it does not replace Nix's sandboxed builder.
  That's the realistic, shippable layer — and it's where the experience lives.
* The deterministic commands (`list`/`diff`/`check`/`generations`/`sync`) read
  JSON state in `state/` and need neither network nor Nix; they are golden-tested.
* `config.jet` is a real Jet program; `jet run config.jet` stands in for the
  future `jet eval --pure` (S60). The **fluent** `apps.installed.append([...])`
  surface needs first-party compiler support — see the design brief in
  [`docs/research/jetpack-config.md`](../../docs/research/jetpack-config.md),
  which compares the with- and without-pure-evaluation paths.

## Layout

```
examples/jetos/
  config.jet            tiny root: source + host + emit
  generated/tree.jet    GENERATED by `jetos sync` — the import-tree
  modules/**/*.jet      dendritic feature modules (auto-wired)
  hosts/<name>.jet      one per machine
  lib/jetpack.jet       directive API + merge engine + canonical render (unit-tested)
  lib/ansi.jet          terminal styling (unit-tested)
  jetos.jet             the CLI
  nix/fixtures/*.json   captured `nix build --json` for offline mode
  state/                evaluated tree + live generation + generations index
  demo/expected/*.out   golden CLI output (--no-color)
  demo/verify.sh        end-to-end verification
```
