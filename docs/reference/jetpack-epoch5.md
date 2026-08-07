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
Discovery follows the nearest `package.jet` and declared Config roots; a `.jet`
file whose name starts with `_` is skipped without changing its contents.
Scalar conflicts name both contributing source files and their values. Successful
fields retain ordered contributor provenance for lock and explain projections;
failed composition never mutates the Package facts.

`jet fmt` formats both `package.jet` and declared Config files through this
typed model. Ordinary source files continue through the compiler formatter.
Typed Package and Config files containing comments fail closed until the typed
formatter owns comment placement; Jet never reports them as clean after
silently leaving them unchanged. Running the command twice is a no-op after
the first pass for comment-free typed files.

## Registry delivery and provider facts

`jet registry publish` commits the immutable sparse index line and the matching
source tree under `artifacts/<name>/<version>` in one registry transaction.
The same commit carries signed per-package sparse metadata, an append-only
transparency log, and a signed checkpoint. The registry publisher key is
host-pinned under the Jet trust directory; fetch refuses a registry view whose
metadata is not signed by that pin or whose checkpoint rolls back or forks.
Fresh consumers may point `JET_REGISTRY_ROOT_KEY` at an administrator-provided
public root-key file; the repository cannot establish its own first trust.
The source tree hash is checked before the index changes. `jet fetch` selects
the highest non-yanked version that satisfies the declared requirement, checks
the publisher signature and source hash, then records the registry, exact
reference, source authority, and Hangar output in `.jet/lock`. Locked fetches
use only the local registry clone and fail if its artifact is missing.

Provider importers lower Jet registry, npm, Cargo, PyPI, SwiftPM, Maven,
NuGet, Conan, vcpkg, Homebrew, GitHub, and binary metadata into one fact
report. Unsupported, ambiguous, or missing identity facts remain explicit loss
records; they do not become invented defaults. Core, Nix, local path, Jet
registry, and verified binary paths expose byte/lock capabilities. The foreign
ecosystem importers expose metadata facts until a dedicated verified transport
adapter exists; they do not claim network fetch or offline substitution.

## Workspace membership

The root `workspace.jet` file can use an explicit list or `find`.

```text
module workspace {
    members: find("./packages")
}
```

Jet rejects absolute paths, `..`, escaping symlinks, duplicate physical
directories, duplicate Package names, and nested member roots.

## Environment presets and language packs

Presets resolve parents before children. `--preset` selects one named preset
for the command. The `env.<name>` modules are separate compositions:
`--env-profile full` selects one module when an environment declares more than
one. Without that flag, `dev`, then `default`, then lexical order chooses the
module. Conflicting facts fail closed. JetOS/tool generations keep their own
commands and state.

Language selections are typed records. Enabled records expand through the
closed catalog into ordinary package references. Disabled records remain in
the plan and in the trust fingerprint, but missing tools for a disabled pack do
not block the environment.

## Source-backed package generations

A package generation is separate from a shell environment preset. A
`profile.<name>` declaration names package refs, parent generations, and exact
path collision choices. `user.<name>` and JetOS use the same generation graph.

```text
module profile.base {
    packages: [default.ripgrep]
}

module profile.dev {
    extends: ["base"]
    packages: [default.fd]
    collisions: { "bin/editor": "fd@default" }
}
```

Run `jet profile plan dev` to inspect the resolved profile. The plan keeps the
raw package ref, source name, provider, channel, source module, and collision
map. `--json` gives the same facts for tools. The command does not realize or
change a generation.

The resolver applies parent profiles first. It rejects missing parents, cycles,
conflicting declarations, adapter packages, unsupported refs, and collision
choices that do not name a package in the resolved profile. It records these
facts in the environment trust identity so a source or collision change needs
a new trust decision.

The built-in catalog covers 58 language families: Ansible, C, Clojure,
Cplusplus, Crystal, Cue, Dart, Deno, Dotnet, Elixir, Elm, Erlang, Fortran,
Gawk, Gleam, Go, Hare, Haskell, Helm, Idris, Java, JavaScript, Jsonnet, Julia,
Kotlin, Lean4, Lobster, Lua, Nim, Nix, Ocaml, Odin, Opentofu, Pascal, Perl,
Php, Pkl, Purescript, Python, R, Racket, Raku, Robotframework, Ruby, Rust,
Scala, Shell, Solidity, Standardml, Swift, Terraform, Texlive, Typescript,
Typst, Unison, V, Vala, and Zig. Each pack discloses its host kind, supported
platform list, license summary, package facts, and required commands. An
unsupported host or an enabled catalog pack with a missing required command
fails during planning; it does not create a partial PATH or claim a tool that
is absent.

Contributors add a pack to the same typed `LanguagePackCatalog` used by the
built-in entries. A contribution must declare its package and optional venv
package references, command mappings, required commands, host/platform facts,
and license. Expansion validates the required-command mapping before it adds
packages, carries variables and commands into the ordinary environment plan,
and includes the complete pack fingerprint in trust identity. Registration
rejects duplicate names, empty package/host/platform/license/tool facts, empty
command mappings, malformed venv package entries, invalid environment-variable
names, and required tools without a command. Unsupported platforms and
conflicting facts fail closed during expansion; a contribution does not create
a second resolver or an untracked PATH shortcut.

```text
module env.dev {
    presets: {
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

Lifecycle facts include dotenv allowlists, unset names, enter and check tasks,
and reload policy. Secret values never enter the plan or information output.

Managed files use project-relative destinations. `Symlink` points to an
immutable content object. `Seed` keeps an existing file. `Copy` owns the file
after the first write. Jet refuses to replace an unmanaged destination.

```text
module env.dev {
    dotenv: [Dotenv.{ file: ".env", allow: ["PORT"], secrets: ["TOKEN"] }]
    unset: ["RUST_LOG"]
    on_enter: [prepare]
    checks: [smoke]
    reload: .Watch.{ paths: ["env.jet"], debounce_ms: 250 }
    files: {
        "config/generated.txt": File.{ content: "generated\n", mode: .Copy }
    }
}
```

Lifecycle tasks run only after the environment is composed. Bare names resolve
to declared `#Job fn`s and use the normal task metadata, trust, and clean-shell
path. The explicit record form remains the one expert hook escape:
`command`, `cwd`, and `trusted: true` are required after review. `jet env test`
runs enter tasks, checks, and any explicit hook in a clean declared environment
and rejects an untrusted hook with `E1329`. Hook working directories must stay
inside the project, including after symlink resolution. A changed hook or
lifecycle policy changes the environment trust identity, so the next entry
needs a new trust decision.

Task metadata stays on the `#Job` marker. Bare tasks use the current project
directory and remain uncached. Typed fields can add task-local packages, a
project-relative `cwd`, declared `inputs` and `outputs`, a typed skip reason,
cache policy, authority, and limits. Platform skips use `Linux`, `MacOS`,
`Windows`, or `FreeBSD` and report why the task did not run. Direct task runs
and scheduled `jet dev` runs apply the same skip rule.

Cached tasks require declared inputs and outputs. Their identity includes task
arguments, locked package facts, policy, platform, compiler bytes, declared
project inputs, and the composed values of allowed non-secret environment
variables. `.env` files and secret-bearing paths are never cache inputs.
Strict cached runs trace project file access and refuse to record a cache
result when the task reads an undeclared project path. Tasks with declared
secrets or secret-bearing environment variables fail closed; use
`cache: .Uncached` when the task is intentionally dynamic.

`jet env sync` resolves all sources first, prints the plan, writes content
objects, and applies destination changes with rollback on failure.

## Hangar path

Jetpack uses one per-user Hangar. On Linux its path is
XDG_DATA_HOME/jet/hangar or ~/.local/share/jet/hangar. On macOS it is
~/Library/Application Support/Jet/Hangar. On Windows it is
%LOCALAPPDATA%/Jet/Hangar. The resolved path is printed by:

jet hangar path

An old state-directory Hangar, or the retired root-owned Hangar, is copied
through an atomic staging directory on first use. The old tree stays in place
so the migration is reversible. An incomplete staging tree stops the command
and must be inspected before retrying.

## Hangar external roots

The Hangar keeps automatic roots for packages, profiles, processes, builds,
toolchains, Systems, and Generations. Use a manual external root only when an
external consumer needs to retain an existing closure. The command never
realizes or downloads the reference.

```text
jet hangar register-external-root backup-sdk ripgrep#2.0.17@nixpkgs \
    --expires-in 12w --yes
jet hangar list-external-roots
jet hangar unregister-external-root backup-sdk --etag 1.1 --yes
```

Each root has a compare-and-swap etag. A changed root is not overwritten or
removed without the current etag. A changed root is not overwritten or removed
after a stale etag. Inspect the current root, then retry with the current
`--if-etag` value only when that state is intended. Expiry ends retention; it
does not delete the Hangar object.

## Signed Hangar archives

Hangar export, import, dump, restore, copy, sign, verify, and repair use one
canonical archive format. An archive contains the selected output closure and
portable package records. Export sorts records and signs the exact bytes with
the user-owned Hangar trust key at `$JETPACK_ROOT/trust/hangar.key` (or the
path passed to `--key`). Import authenticates and re-hashes every object in a private staging
directory before it changes the closure database. Existing objects are reused
only when their digest matches.

```text
jet hangar export app --to app.hangar
jet hangar verify app.hangar
jet hangar import app.hangar
jet hangar repair app --from app.hangar
```

Unsigned archives are refused by default. `--allow-unsigned` is an explicit
local migration escape and never becomes the default. Remote `ssh://` and
`https://` copy destinations fail with a transport error; Jet does not claim
to have transferred bytes when no verified transport is configured.

## Host-owned binary caches

Workspace policy may request cache roles. The host binds those roles to an
ordered mirror list; endpoints and credentials do not come from a repository,
flag, or environment variable. Local paths and `file://` mirrors use the
canonical signed NAR path. HTTP(S), SSH/ssh-ng, S3-compatible, Hangar, and
Nix-store endpoints use host-owned adapters and preserve the same narinfo,
NAR digest, and output identity. A missing host adapter is an explicit
capability error; it never pretends to transfer bytes.

```text
jet cache bind public file:///srv/jet-cache --credential keychain:jet/public --yes
jet cache bind release /srv/jet-release --write --yes
jet cache list
jet cache publish app --role release --yes
jet cache verify app --role public
jet cache substitute app --role public --to /tmp/app-output --yes
```

The first mirror with a valid signature, NAR digest, and decoded output hash
wins. Publishing requires the separate binding write grant. A substitution
never overwrites an existing destination.

Binary-cache substitution uses the canonical NAR codec. NAR bytes are hashed
before admission, and a narinfo must carry a matching signed hash, size,
reference set, and store identity. Substitution stages the decoded tree and
refuses conflicting existing objects. Local and host-adapter transfers resume
through a verified .partial prefix before publication. A missing or corrupt
mirror falls back to source realization; it never installs an unsigned or
replayed result.

`jet shared-store install` creates the optional administrator-installed shared
Hangar configuration and socket-activation units. Each request runs as a
short-lived non-root `DynamicUser` with a private state directory. The socket
is public by mode only; Linux peer credentials and a root-owned per-uid grant
authorize each read or write.

A read grant is persistent. A write grant contains a short-lived credential,
an expiry, and exact allowlists for `source=`, `builder=`, `action=`,
`output=`, `platform=`, `sandbox=`, and `policy=`. The command creates the
credential and expiry; an administrator must add every approved binding fact
before the pending write grant can write. Until then, reads work and writes
stay on the ordinary per-user Hangar path. The client sends
an unsigned closure plus the binding facts for source, builder, action,
output, platform, sandbox, and policy. The broker verifies those facts against
the archived metadata, content-checks the closure in an ephemeral `.incoming`
stage, signs it only after admission, and removes stale stages before
accepting new work. It never receives source or build commands. If the socket
or a write grant is absent or expired, realization also stays private.

## Services

Services run as direct argument vectors under the platform-owned supervisor.
On Linux this is a transient systemd user scope with a delegated cgroup; on
Windows it is a project-local guardian with a Job Object. macOS rejects a
service before spawn when that authority is unavailable (E1332). Readiness is
separate from process start. A service can use `exec`, `http`, `notify`, or
`tcp` readiness, and every probe has a bounded per-attempt time limit.

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

`after` names a declared service dependency. It is the only dependency spelling;
the retired `depends_on` spelling is rejected. Jetpack validates names, disabled
dependencies, and cycles before spawning a process, starts dependencies before
dependents, and stops the selected graph in reverse order. A failed task,
startup, readiness gate, or dependency stops the affected graph without
leaving a dependent alive against a failed prerequisite.

Jet reserves ports and socket paths before start. It checks process start
identity before it sends a signal. It bounds restart count and backoff, and it
stops dependent services before their dependencies. Each service directory also
persists the authority backend, generation, phase, containment, dependency
list, and recovery reason in `lifecycle`; a post-Ready crash records its
recovery generation before restart.

## Flake-class graph

Foreign flakes and flake-parts modules feed the same graph as native sources.
Exact input revisions, `follows` edges, output mappings, provenance, and
declarative flake-parts modules round-trip through `.jet/lock`. Loaded locks
also record each imported module's content fingerprint, so editing a module
invalidates the graph before bridge output is reused.

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
lossless Jet meaning produces L0204. Unsupported or over-budget foreign-flake
input produces E1256. Arbitrary evaluator functions never become Jet values.

The private native evaluator is bounded before parsing: source input is capped
at 1 MiB, token and expression budgets are finite, imports require explicit
project-root authority, and only the pinned Stage A systems are accepted.
Unsupported or over-budget expressions fail with a typed evaluator error. The
production boundary does not invoke `nix` or require it on `PATH`.

The evaluator's pinned breadth inventory records overlays, dev shells, and
multi-output package projections as covered. Fixed-output fetchers, cross-system
packages, and external flakes remain explicit skips: they require fetch,
target/provider, or remote-source authority that this no-std boundary does not
own. Differential fuzzing belongs to the host proof harness. These skips are
reported as unsupported evaluator input; they are not empty or guessed values.

## package.jet transitions

The typed Package model owns one reversible transition journal. `jet init
--check` previews migration from retired `pkg.jet`, `env.jet`, `workspace.jet`,
or `config.jet` files; `jet init` applies only closed facts and refuses unknown
or open fields before writing. An `env.<name>` module becomes a typed
`environments.<name>` Config contribution; it is not copied as an unrelated
top-level field. `jet init --restore-role-files` reverses the last migration
and restores the original bytes.

Growth uses the same journal: `jet split env`, `jet split package <name>`, and
`jet split hosts <name>` preview by default when `--check` is present. A split
extracts a closed Config or Fleet contribution, records the pre-change bytes
and fact fingerprint, and applies all files atomically. `jet fold <generated
file>` reverses the matching journal only when every recorded file still has
the expected bytes. User-authored files added after a split are not consumed.

## Build hooks and images

Build hooks lower to a finite action graph. Fetches need exact hashes. Exec
steps use declared tool paths. Install paths stay under the output root.
Successful outputs publish atomically and failed stages are removed. An
approval binds the package, provider/source, staged source digest, platform,
exact recipe digest, and declared capabilities. A change in any of these facts
needs a new approval; `--trust` is one-shot and CI accepts only an exact
repository grant. Hook processes receive no caller environment or credentials,
only the declared deterministic build values and the private output channel.

Environment images project the same package and service facts into OCI
metadata. Secret values and dotenv contents do not enter the image projection.

## First-party integrations

Environment modules may import typed first-party integrations such as
`env.platform.android()`, `env.platform.apple()`,
`env.security.certificates([...])`, `env.network.hosts({...})`, and
`env.agent.codex(...)`, `env.cloud.credentials([...])`, and
`env.security.vault([...])`. Each import lowers into the same package, file, secret,
host-check, provider, and grant facts used by the rest of Jetpack. SDK imports
carry deterministic safe defaults and preserve expert options in the plan. The
Apple preset includes `apple-sdk@nixpkgs`, the `apple-sdk-check` task, the
`nixpkgs` provider, an explicit `target:darwin-or-macos` host check, and the
`policy-required` license fact. Realization rejects an unsupported `JET_TARGET`
before package activation; the integration options and host meaning remain in
the environment fingerprint. Certificate imports lower named secret references
to the `vault` provider, the `certificate-store-check` task, and the separate
`certificate.read` grant. Host mappings lower to `host-binding` and
`host-binding-check`; host values stay ordinary plan options. The Codex agent
preset uses the `mcp` provider, `mcp-agent-check`, and the separate `mcp.read`
grant. The VS Code editor preset uses the ordinary `vscode@nixpkgs` package and
the `nixpkgs` provider. Cloud credential names lower to the
`credential-store-check` task, `credential-store` provider, and separate
`credential.read` grant. Vault names lower to the `vault-check` task, `vault`
provider, and separate `vault.read` grant.
Secret values are never stored in the plan or its fingerprint. Secret names
enter the ordinary environment secret check. Provider facts are checked against
the closed preset mapping, and sensitive integration grants are separate
persisted trust records: for example,
`jet trust grant integration:certificates:certificate.read --scope user`.
The one-shot `--trust` flag does not manufacture an integration authority.
