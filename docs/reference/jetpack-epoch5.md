# Jetpack environment and package facts

Epoch 5 uses one typed graph for packages, environments, services, and foreign
flake inputs. The graph is data. Jetpack performs realization, process control,
file writes, and lock updates from that data.

## Package and Config

`package.jet` is the canonical Package file. `pkg.jet` remains a migration
input. A Package can declare outputs, environments, services, dependencies,
defaults, and root-only members.

```text
name: "demo"
version: "0.1.0"
outputs: .{
    app: .Executable.{ entry: run }
    check: .Check.{ entry: check }
}
defaults: .{ run: app, test: check }
configs: ["config/dev.jet"]
members: find("./packages")
```

`Config` files add typed facts to the Package. Equal facts merge. Conflicting
facts fail before realization. A Config cannot declare `members`.

## Workspace membership

The root `workspace.jet` file can use an explicit list or `find`.

```text
module workspace {
    members: find("./packages")
}
```

Jet rejects absolute paths, `..`, escaping symlinks, duplicate physical
directories, duplicate Package names, and nested member roots.

## Environment profiles and language packs

Profiles resolve parents before children. `--profile` selects one profile for
the command. Without a flag, hostname, user, and then `default` choose a
profile.

Language selections are typed records. Enabled records expand through the
closed catalog into ordinary package references. Disabled records remain in
the plan and in the trust fingerprint.

```text
module env.dev {
    profiles: {
        base: .{ packages: ["git@nixpkgs"] }
        work: .{ extends: ["base"], hostname: "build-01" }
    }
    languages: {
        rust: Lang.{ enable: true, channel: .Stable }
        python: Lang.{ enable: true, version: "3.12", venv: true }
    }
}
```

## Lifecycle and managed files

Lifecycle facts include dotenv allowlists, unset names, enter and check hooks,
and reload policy. Secret values never enter the plan or information output.

Managed files use project-relative destinations. `Symlink` points to an
immutable content object. `Seed` keeps an existing file. `Copy` owns the file
after the first write. Jet refuses to replace an unmanaged destination.

```text
module env.dev {
    dotenv: [Dotenv.{ file: ".env", allow: ["PORT"], secrets: ["TOKEN"] }]
    unset: ["RUST_LOG"]
    reload: .Watch.{ paths: ["env.jet"], debounce_ms: 250 }
    files: {
        "config/generated.txt": File.{ content: "generated\n", mode: .Copy }
    }
}
```

`jet env sync` resolves all sources first, prints the plan, writes content
objects, and applies destination changes with rollback on failure.

## Services

Services run as direct argument vectors. Readiness is separate from process
start. A service can use `exec`, `http`, `notify`, or `tcp` readiness.

```text
module env.dev {
    services: {
        api: Service.{
            enable: true,
            run: ["./bin/api", "--port", "8080"],
            ready: .http("http://127.0.0.1:8080/health", 200),
            ports: [8080],
            restart: .OnFailure.{ max: 3, backoff_ms: 250 },
            after: ["database"]
        }
    }
}
```

Jet reserves ports and socket paths before start. It checks process start
identity before it sends a signal. It bounds restart count and backoff, and it
stops dependent services before their dependencies.

## Flake-class graph

Foreign flakes and flake-parts modules feed the same graph as native sources.
Exact input revisions, `follows` edges, output mappings, provenance, and
declarative flake-parts modules round-trip through `.jet/lock`.

```nix
{
  inputs = {
    flake-parts.url = "github:hercules-ci/flake-parts?rev=0123456789abcdef0123456789abcdef01234567";
    nixpkgs.url = "github:NixOS/nixpkgs?rev=89abcdef0123456789abcdef0123456789abcdef";
  };
  outputs = inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [ ./parts/dev.nix ];
      systems = [ "x86_64-linux" ];
      perSystem = { pkgs, ... }: {
        devShells.default = pkgs.mkShell { };
      };
    };
}
```

Use `jet bridge flake` to review an `env.*` shim. Every field without a
lossless Jet meaning produces L0204. Missing Nix for a foreign-flake path
produces E1256. Arbitrary evaluator functions never become Jet values.

## Build hooks and images

Build hooks lower to a finite action graph. Fetches need exact hashes. Exec
steps use declared tool paths. Install paths stay under the output root.
Successful outputs publish atomically and failed stages are removed.

Environment images project the same package and service facts into OCI
metadata. Secret values and dotenv contents do not enter the image projection.
