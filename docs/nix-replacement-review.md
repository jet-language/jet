# Nix → Jet — review, pitfalls, and worked examples

> **STATUS (2026-06-13): DRAFT companion to docs/nix-replacement-plan.md.**
> Decides no syntax or semantics. Every Jet snippet below separates
> **ratified** pieces from **proposed** config surface; nothing here is
> spec until balloted (D-OS/D-NX/D-PM). Grounded against docs/02 (ratified
> syntax), docs/jetpack.md, docs/jetos.md, and docs/package-manager-decisions.md.

This file captures the review of the Nix-replacement direction: the
pitfalls in the umbrella plan, the reconciliation of "import anywhere"
with the liftability law, four worked nix↔jet examples (a devshell and a
NixOS system, each side by side), and the keep/lose/mitigate scorecard
for Nix's strengths.

---

## 1. Pitfalls in the plan

**1. The deepest gap: tapping nixpkgs gives you *binaries*, not
*configurability*.** D-NX1 (the existential one) solves packages — but
the thing that makes NixOS valuable isn't that it *has* nginx, it's
`services.nginx.virtualHosts.*`. Those ~10k typed options are nixlang
*module definitions*, and they cannot be substituted from a binary
cache. So "204 bootstrap packages substituted" gets you runnable
binaries and a **near-empty option tree**. The configurability — the
actual product — has to be hand-rebuilt as typed Jet option modules.
**This is a second existential decision the plan doesn't name: the
option-schema bootstrap.** It is bigger than D-NX1 because there is no
cache to tap. Recommend adding it as **D-NX6**.

**2. "Strict beats lazy" (§4) oversimplifies — and jetos already
concedes it.** Nix's laziness is load-bearing for the module system:
option bodies reference other options' *final merged values*
(`config.foo`), and laziness is what lets that fixpoint resolve.
jetos.md §5.2 already needs this — `cfg.…` reads are "resolved lazily;
cycles detected" (J-M030). So Jet does not ship pure strict eval; it
ships **strict module bodies + a demand-driven, cycle-checked
option-read layer.** The clean rule that makes this airtight:

> **Schema imports are strict and acyclic** (importing firefox's option
> *declarations* can't cycle — a schema doesn't depend on values).
> **Value reads** (`cfg.apps.firefox.enable`) **are lazy and
> cycle-checked.** Keep those two on opposite sides of a line and the
> strict/lazy tension dissolves.

**3. The no-escape-hatch rule (§7) collides with the long tail of real
machines.** Kernel params, udev rules, one-off `/etc/*` files,
state-migration at activation — NixOS users reach for
`extraConfig`/`environment.etc."x".text`/activation scripts constantly
*because the option tree never covers everything*. Two honest
mitigations:
- A **typed freeform file option** already exists in disguise: jetos.md's
  `user.me.files["~/..."] = …`. Generalize it to `sys.files["/etc/..."]`.
  That *is* the escape hatch, and because it is declarative+typed it does
  not violate the philosophy.
- You will still need a **typed, sandboxed activation-action** option for
  genuine state migration. Banning it outright loses a real class of
  systems. Ship one narrow, audited verb rather than pretend the need
  doesn't exist.

**4. Dynamic typing must survive at exactly one boundary: vendor
pass-through config.** Firefox's `policies.json`, a systemd unit's free
fields — open schemas you cannot statically model. jetos.md already
smuggles this in as `map<string, json>`. That instinct is right, but it
means **a first-class `Json` value type is non-negotiable**, and it is
the one place "everything is statically typed" yields. Frame it as a
feature (typed *up to* the pass-through boundary), not a leak. This is
the pressure-release valve that makes pitfall #3's escape-hatch ban
survivable.

**5. Syntax-protocol violations in the existing examples.** jetos.md uses
`map<string, json>` (ratified is `Map<String, …>` — S33 angle brackets,
S11 capitalized types; there is no `json`/`Json` ratified yet), and
`sys.desktop.environment = cinnamon` uses a bare identifier as a value
(ratified enum variants are `Type.Variant`, S30). The
`option`/`when`/`default`/`force` keywords are all unratified (D-OS1–4).
None of this blocks planning, but the worked examples must be rewritten
to ratified syntax or the gaps balloted before any of it is spec.

**6. The comptime fuel limit (S26) will bite config eval.** `jet eval
--pure` *is* the M9.5 comptime interpreter, which is fuel-limited with a
call-trace diagnostic. A realistic desktop config is far larger than any
comptime constant the limit was sized for. Without a separate fuel budget
for config evaluation, real configs hit "out of fuel" errors.

**7. The perf claims (§4) are about the wrong layer.** "Evaluate a
desktop config in under 1s" is trivially true for a small typed option
tree — but the moment D-NX1-A taps nixpkgs for 200+ packages, you
re-inherit Nix-eval cost *at the substituter boundary*. Be explicit that
the sub-second claim is the **config/option** layer; the package graph is
a separate, cache-bound number.

**8. `jetos add` auto-commit (D-NX2) co-owns the user's git history.**
Signing, commit conventions, rebase/merge workflows all collide with a
tool that commits on every `add`. Consider staging-without-commit as the
default and `--commit` as opt-in.

---

## 2. "Import anywhere" vs. the liftability law (OS-I2)

The `import jetpack.firefox as firefox;` model **directly contradicts
jetos.md's OS-I2** ("modules communicate *only* through declared options;
modules may not import each other"). OS-I2 exists for a real reason — if
`firefox.jet` can `import "../desktop_helpers.jet"`, copying it to a
stranger's repo breaks.

The two ideas are reconcilable, and the reconciliation is *better* than
either alone. Distinguish two kinds of import:

| Import kind | Example | Verdict |
|---|---|---|
| **Sibling config file** in your own repo | `import "../desktop_helpers.jet"` | **Forbidden** (OS-I2) — drags hidden local deps, breaks lifting |
| **Published capability schema** from the registry | `import jetpack.firefox as firefox;` | **Allowed and encouraged** — explicit, checkable, the schema is shared, not private |

The semantic that makes it sound: **`import jetpack.firefox as firefox`
binds a local alias to the *global* `apps.firefox` option subtree.**
`firefox.enable = true` is sugar for `apps.firefox.enable = true`. So
underneath it is still *one merged option tree* — lists concatenate
across files, scalars conflict-or-priority, exactly as jetos.md §5.4
specifies. The import is a local name, not a private channel.

This buys three things the auto-import-tree model doesn't have:

1. **The import line *is* the file's declared interface** — no static
   inference needed. `jetos lift` just reads the imports.
2. **Explicit beats magic.** One import line per capability is a readable
   manifest of what each file does.
3. **It is already ratified syntax** — S16 form-2 + S51 (module import,
   optional `as`). The only new decision is registering `jetpack`/`jetos`
   as reserved module roots alongside `std` (S51): a one-line ballot, not
   new grammar.

The cost to be honest about: a file with no imports contributes nothing
(you lose "drop a file in the folder and it's live"). Fair trade for
explicitness, but a real D-OS decision — and it nudges toward **D-NX2
Option B** (per-feature module files), not the recommended Option A.

---

## 3. Worked examples

> **Syntax honesty:** the Jet reuses **ratified** pieces — imports
> (S16/S51), list/map literals (S37/S38), index-assign `m[k]=v` (S39),
> string interpolation (S8). Everything that is the *config surface
> itself* — top-level option assignment (`firefox.enable = true`),
> capability-schema imports binding into the option tree, bare
> package/enum identifiers as values — is **proposed and unratified**
> (D-OS1–7 plus a new "reserved roots" row).

### A. Nix — dev shell flake

```nix
{
  description = "rust dev shell";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.flake-utils.url = "github:numtide/flake-utils";

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let pkgs = import nixpkgs { inherit system; };
      in {
        devShells.default = pkgs.mkShell {
          packages = [ pkgs.rustc pkgs.cargo pkgs.rust-analyzer
                       pkgs.pkg-config pkgs.openssl ];
          env.RUST_LOG = "debug";
          shellHook = ''echo "rust dev shell ready"'';
        };
      });
}
```

### B. Jet — dev shell (`shell.jet`, evaluated `jet eval --pure`)

```jet
// A devshell is a project-level capability, not an OS config.
import jetpack.shell as shell;
import jetpack.pkgs;                 // the registry's package namespace

shell.packages = [
    pkgs.rustc, pkgs.cargo, pkgs.rust_analyzer,
    pkgs.pkg_config, pkgs.openssl,
];
shell.env["RUST_LOG"] = "debug";
shell.greeting = "rust dev shell ready";
```

What vanished and why it is an improvement:
- **No `inputs`/`outputs`/`system` boilerplate.** `jetpack.pkgs` is
  resolved by the lockfile (`jet.lock`, S52); per-system fan-out is the
  resolver's job, not yours to wire by hand.
- **No `import nixpkgs { inherit system; }` ritual.** The dependency is
  one pinned line in `jet.toml`.
- **`shellHook` (arbitrary bash) becomes a typed field**
  (`shell.greeting`). Anything that needs to *run* goes through a typed,
  sandboxed shell action — not an unfenced string of bash (pitfall #3).

### C. Nix — NixOS system flake

```nix
{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.11";
  outputs = { self, nixpkgs }: {
    nixosConfigurations.laptop = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ./hardware-configuration.nix
        ({ config, pkgs, ... }: {
          networking.hostName = "laptop";
          time.timeZone = "America/Chicago";
          services.xserver.enable = true;
          services.xserver.desktopManager.cinnamon.enable = true;
          programs.firefox = {
            enable = true;
            policies.DisableTelemetry = true;
          };
          environment.systemPackages = [ pkgs.ripgrep pkgs.vlc ];
          users.users.nate = {
            isNormalUser = true;
            extraGroups = [ "wheel" "networkmanager" ];
          };
          system.stateVersion = "24.11";
        })
      ];
    };
  };
}
```

### D. Jet — the same system, dendritic (the import model)

Files are auto-discovered under `modules/` and `hosts/` (jetos.md §4),
but each file declares what it touches via imports — that declaration
*is* its liftable interface.

`hosts/laptop.jet`:
```jet
import jetos.sys as sys;

sys.hostname = "laptop";
sys.timezone = "America/Chicago";
sys.state_version = "1.0";
```

`modules/desktop.jet`:
```jet
import jetos.sys as sys;
import jetpack.cinnamon as cinnamon;

cinnamon.enable = true;
sys.desktop = cinnamon;          // scalar: conflicts loudly if another file disagrees
```

`modules/apps/firefox.jet`:
```jet
import jetpack.firefox as firefox;

firefox.enable = true;
firefox.policies["DisableTelemetry"] = true;     // typed up to here…
firefox.policies["Homepage"] = json({            // …Json past the pass-through boundary (pitfall #4)
    "URL": "https://start.example",
    "Locked": true,
});
firefox.extensions = [ublock_origin, dark_reader];  // list: concatenates across files
```

`users/nate.jet`:
```jet
import jetos.user as user;
import jetpack.pkgs;

val nate = user.named("nate");
nate.normal = true;
nate.groups = [Group.wheel, Group.network_manager];   // enum variants, not bare strings (S30)
nate.packages = [pkgs.ripgrep, pkgs.vlc];
```

The merge story, made explicit (this replaces Nix's `mkMerge`/`mkForce`
magic-number ladder):
- `firefox.extensions` and `nate.packages` are **lists → concatenate**,
  sorted by source path so discovery order can't change the result
  (jetos.md §5.4).
- `sys.desktop` is a **scalar → one value per priority**; conflicting
  values produce `J-M021` naming both files and lines, with the fix
  (`default`/`force`) spelled out — instead of Nix silently letting the
  last `mkDefault` win.
- `firefox` configurability across both files works **because `import
  jetpack.firefox` aliases the same global `apps.firefox` subtree** — the
  import is a local name, not a private copy.

---

## 4. Strengths-of-Nix scorecard (keep / lose / mitigate)

| Nix strength | Status in Jet | Mitigation if lost |
|---|---|---|
| Content-addressed store, rollback, atomic switch | **Kept identically** (PM layer) | — |
| Laziness → option fixpoint & `mkIf` | **Partially lost** (strict bodies) | `when` guards + lazy *value*-reads with cycle detection; schema imports stay strict/acyclic (pitfall #2) |
| Laziness → "only build what's referenced" | **Kept** | packages stay as plan-hash references, evaluated at the build layer, never strictly forced by config eval |
| 100k packages | **Lost initially** | D-NX1-A: tap cache.nixos.org, greenfield the spine |
| 10k *typed options* (configurability) | **Lost initially, under-addressed** | **Needs D-NX6**: hand-write the spine option schema; `Json` pass-through for the tail (pitfalls #1, #4) |
| Escape hatches (`extraConfig`, activation scripts) | **Banned by §7** | typed `sys.files[path]` + one narrow sandboxed activation verb (pitfall #3) |
| `imports = [...]` flexibility | **Replaced** | import-anywhere of *published capabilities* — better, because it's also the liftability manifest |

One-line summary: the package half of "make Nix obsolete" has a credible
path (D-NX1). The **configurability half does not yet** — and that, not
packages, is where Nix's real moat is. Naming **D-NX6** (option-schema
bootstrap) and committing to the **`Json` pass-through type** are the two
changes that would make the plan structurally complete.
