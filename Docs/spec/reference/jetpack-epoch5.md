# Jetpack package and environment facts

Jetpack turns one typed project graph into package, environment, service, profile,
and image operations. This reference is for package and environment authors and
for tools that inspect or realize their declarations. The executable contracts
live in the [package model](../../../crates/jet-pkg-model/src/Package/mod.rs),
[environment model](../../../crates/jet-env-model/src/ModuleEval/Environment.rs),
and [Jetpack CLI help](../../../crates/jetpack/src/CLI/usage_tests.rs).

## One typed graph

`package.jet`, `env.jet`, workspace declarations, and foreign-source bindings
contribute typed facts. Jetpack resolves those facts once, then uses the same
plan for realization, trust, process control, managed files, locks, and image
projection. A service preset, language pack, or first-party integration is a
projection into this graph, not a second resolver.

A plan is data. Realization may write the managed `.jet` directory, Hangar
objects, process state, or a lock only at the operation that owns that write.
Read-only inspection does not realize packages, run jobs, start services, or
apply managed files.

## Package roots and Config facts

`package.jet` is the canonical package manifest. A migration-era `pkg.jet` can
be folded into it by the transition journal; it is not a second package model.
`jetpack.toml` is not a supported project input and is rejected with E1225.
Package identity, outputs, dependencies, policies, and root-only membership
belong to the Package root.

A minimal manifest uses the Jet grammar directly:

```jet
name: "demo"
version: "0.1.0"
outputs: {
    app: .Executable{ entry: run }
    check: .Check{ entry: check }
}
defaults: { run: app, test: check }
configs: ["config/dev.jet"]
members: find("./packages")
```

The output kinds are a closed vocabulary: `Library`, `Executable`, `Service`,
`Check`, `Environment`, `Image`, `Bundle`, `System`, `Fleet`, and `Model`.
Output records carry the kind, name, optional entry, and kind-specific fields.
A Package root may own `members`; a Config contribution may not. Equal facts
compose, conflicting facts fail before realization, and successful fields keep
ordered contributor provenance for inspection and lock projections.

A workspace or package may declare an explicit member list or a `find` root.
Discovery is deterministic: candidates are sorted, `package.jet` is reserved for
the manifest, and the resolver rejects absolute paths, `..` escapes, escaping
symlinks, duplicate physical directories, duplicate Package names, and nested
member roots. The package root owns membership even when a member is discovered
from a `find` expression.

### Import boundaries

The declaring Package can narrow its own module edges with `boundaries`:

```jet
boundaries: {
    deny: [
        { from: "app.ui", to: "app.db" },
        { from: "app.api.*", to: "app.db.*" },
    ],
}
```

Each side names an exact module or allows one trailing `*` subtree wildcard.
A denied edge is E0619. A rule matching no loaded edge produces L0619 and does
not block the build. The rule belongs to the declaring Package, records typed
import-edge facts for inspection, and narrows the graph before code generation;
it is not runtime code. Omitting `boundaries` preserves ordinary import
resolution.

`jet fmt` uses the typed Package and Config model for package files and declared
Config files. Comment placement is fail-closed until that formatter owns it;
Jet does not report a comment-bearing file clean after silently leaving it
unchanged. Ordinary Jet source uses the compiler formatter.

## Sources, providers, and the lock

The project lock is one file at `.jet/lock`. `jetpack add`, `jetpack remove`,
and `jetpack update` change the declared graph or its selected source pins;
`jetpack lock diff` compares lock changes with a revision. Dependency references
retain their provider, selector, and source authority rather than becoming an
unqualified package name.

A lock record can carry the exact resolved revision, source-tree content hash,
plan fingerprint, direct dependencies, effect provenance, a realized-output
envelope, a Hangar receipt, and provider provenance. Nix records additionally
carry a channel and revision, system, derivation and output identities, Nar
hash, references, cache identity, and project CAS bundle. Host paths are not
valid substitutes for these identities.

Offline operation reads only local fixtures, the project lock, and verified
local objects. A missing provider object or a lock/source mismatch is an error;
Jetpack does not silently consult the network or substitute a guessed provider.
Unsupported, ambiguous, or lossy provider fields remain explicit diagnostics or
loss records. A provider adapter must either preserve the fact or explain why
it cannot do so.

A foreign dependency that names a language and exact reference is accepted only
with a verified provider artifact containing the corresponding
`.jet/bindings/<language>/<library>.jet` binding. The binding, source authority,
language-qualified reference, and envelope are recorded with the lock. Missing
or unverified provider bytes fail closed; they do not produce an invented
binding.

## Workspace and environment selection

A workspace declaration owns its member index. The member graph is checked
before realization, including path containment, duplicate physical roots,
Package identity, and nested membership. A package authority declaration is not
silently treated as a workspace index.

`--env <name>` selects one `env.<name>` module. `--preset <name>` selects one
declared environment preset. A selected environment is not merged with sibling
environment modules. Parent presets resolve before children, and conflicting
facts fail closed.

Language selections are typed catalog records. Enabled entries expand into
ordinary package references with host, platform, license, command, and required
tool facts. Disabled entries remain visible in the plan and trust fingerprint,
but missing tools for a disabled entry do not block the environment. Unsupported
hosts, incomplete records, duplicate registrations, malformed virtual
environments, invalid variable names, and required tools without commands fail
closed rather than creating a partial `PATH`.

`jetpack env info` reads the selected typed plan. It reports packages,
formatter, services, jobs, checks, variables by name, managed-file destinations,
Git hook path, and integration facts. `jetpack env info --json` emits the same
facts for tools. It does not realize packages, run jobs, start services, apply
files, or print variable values.

The environment entry also provides these operations:

```text
jetpack env test [-- command]
jetpack env sync
jetpack env hook <shell>
jetpack env export <shell>
```

`env test` runs lifecycle jobs and checks in a clean child environment. `env
sync` resolves the complete managed-file plan before writing and applies it with
rollback on failure. Hook and export operations use the selected composed
environment; they do not create a second environment definition.

## Profiles and immutable package generations

A `profile.<name>` declaration is a source-backed package generation, not an
environment preset. Parents resolve before children. The profile plan retains
raw references, source names, providers, channels, source modules, collision
choices, and a stable fingerprint:

```jet
module profile.base {
    packages: [default.ripgrep]
}

module profile.dev {
    extends: ["base"]
    packages: [default.fd]
    collisions: { "bin/editor": "fd@default" }
}
```

Use the profile verbs exposed by Jetpack:

```text
jetpack profile plan dev
jetpack profile build dev
jetpack profile switch dev
jetpack profile generations dev
jetpack profile rollback dev
jetpack profile rollback dev 3
```

`plan` is read-only. `build` records an immutable generation, `switch` updates
the checked current pointer atomically, `generations` lists retained history,
and `rollback` activates an exact retained generation. JSON planning includes
`fingerprint`, selected and applied profiles, source declarations, package
records, collision choices, and `provider_facts`.

A generation cannot discard provider identity or silently resolve a collision.
External references need exact version, revision, or digest information. The
resolver rejects missing parents, cycles, unsupported references, conflicting
facts, adapter packages, stale collision choices, file/directory mismatches,
and symlink-target mismatches. These failures use E1335 where the profile
lifecycle must preserve provider or output identity.

## Lifecycle, jobs, and managed files

Environment lifecycle facts include dotenv allowlists, unset names, enter and
check jobs, a project-relative `git_hooks_path`, reload policy, and an optional
formatter package. Bare lifecycle names resolve to checked `#Job` functions.
Explicit hook records require their command, working directory, and
`trusted: true` after review. Hook directories and working directories must
remain inside the project after symlink resolution. An untrusted hook is
rejected with E1329, and changes to hook or lifecycle facts change the
environment trust identity.

Managed files use project-relative destinations and one closed write policy:

| Mode | Meaning |
| --- | --- |
| `Symlink` | Point to an immutable Jet-owned content object (the default). |
| `Seed` | Preserve an existing destination. |
| `Copy` | Own the destination after the first successful application. |

An unmanaged destination is never overwritten. `jetpack env sync` resolves all
sources first, writes content objects, and applies destination changes as one
managed operation. Secret-bearing content is redacted from debug and plan
projections.

A formatter field names a package, not an arbitrary shell command. Native Jet
files use the compiler formatter. `jet fmt --lang <language>` realizes the
selected environment, stages the complete file batch, and delegates to the
formatter package; a formatter failure leaves source files unchanged.

## Hangar roots, leases, and quota

Jetpack stores realized objects in one per-user content-addressed Hangar. The
ordinary state roots are:

| Host | State root |
| --- | --- |
| Linux | `$XDG_DATA_HOME/jet` or `~/.local/share/jet` |
| macOS | `~/Library/Application Support/Jet` |
| Windows | `%LOCALAPPDATA%/Jet` |

The Hangar is the `hangar` child of the Linux state root and the `Hangar`
child on macOS and Windows. An explicit `JETPACK_ROOT` uses its lower-case
`hangar` child. The project-local `.jet/` folder contains `.jet/lock`, caches,
lifecycle records, and roots; realized package bytes remain in Hangar.
`jetpack hangar path` prints the resolved Hangar path. Nix paths are admitted
as content and retained only as producer provenance.

Hangar migration uses a private staging tree and leaves the old tree in place
until the new projection is complete. A symlink or non-directory at a managed
boundary stops the operation instead of redirecting writes. `jetpack hangar
recover` is the recovery boundary for interrupted publication, archive staging,
repair quarantine, and abandoned leases.

An executable lease carries the private snapshot, complete generations, an
authenticated owner lock, and an inheritable live lock. Descendants keep the
live lock, so liveness does not depend on a PID, timestamp, or marker file. A
replacement or rollback publishes only after a complete generation exists and
retains the last complete generation if setup fails. Recovery reclaims a lease
only when owner and lifetime locks are idle; it preserves snapshots protected by
running descendants. `jetpack audit` and `jetpack doctor` inspect this state
without publishing or deleting it.

Hangar admission counts logical file bytes and defaults to 128 GiB. The
`JETPACK_HANGAR_MAX_BYTES` environment variable overrides
`$JETPACK_ROOT/config/hangar-max-bytes`; the configured value must be one
positive byte count. Each admission also reserves 1 MiB for journal and
metadata. Jetpack removes build scratch and orphaned objects first, then
unreferenced objects from oldest known use; live roots and malformed records
are not eviction candidates.

Manual external roots retain an existing closure without realizing or
re-downloading it:

```text
jetpack hangar register-external-root backup-sdk ripgrep@nixpkgs#2.0.17 \
    --expires-in 12w --yes
jetpack hangar list-external-roots
jetpack hangar unregister-external-root backup-sdk --etag 1.1 --yes
```

Each manual root has a compare-and-swap etag. Registration and removal require
the expected current etag; expiry ends retention but does not delete the
object.

## Archives and host-owned caches

Hangar `export`, `import`, `dump`, `restore`, `copy`, `sign`, `verify`, and
`repair` use one canonical archive format. Export signs the exact sorted bytes;
import verifies signatures and object digests in private staging before changing
the closure database. Unsigned archives require the explicit local
`--allow-unsigned` escape. Repair accepts replacement bytes only from a verified
archive and preserves the old object in quarantine if publication fails.
The default archive signer is `$JETPACK_ROOT/trust/hangar.key`; `--key` selects
an explicit host-owned key. `jetpack hangar copy` verifies the source-signed
archive before importing it into a local destination, and does not infer SSH or
HTTP transport. Those remote archive destinations fail before any bytes move.

Workspace policy may request named cache roles, but the host binds each role to
ordered mirrors, credentials, trust keys, and write authority. Repository text,
flags, and environment variables do not supply those credentials. A cache hit
is admitted only after signature, NAR, output, platform, and producer identity
checks. A missing or corrupt mirror falls back to source realization rather
than installing an unsigned or mismatched result.

```text
jetpack hangar cache bind public file:///srv/jet-cache --yes
jetpack hangar cache list
jetpack hangar cache verify app --role public
jetpack hangar cache substitute app --role public --to ./out --yes
```

An uncached source-only build can be independently certified in two fresh
private Hangar roots. Jetpack compares action identity, output trees, named
outputs, and producer facts before publication. Divergence is written under the
Hangar `unreproducible` report area, publishes no trusted cache fact, and leaves
the private roots for recovery cleanup.

An optional administrator-installed shared-store broker accepts one
provenance-bound archive per request, verifies it, and signs it after admission.
It never receives source, build commands, or evaluator input. Read grants can
persist; write grants are short-lived and constrain source, builder, action,
output, platform, sandbox, and policy facts. Without the broker, callers keep
using their per-user Hangar.

## Services and readiness

Development services run as direct argument vectors under a platform-owned
supervisor. Linux uses a transient systemd user scope with a delegated cgroup;
Windows uses a project-local guardian Job Object. macOS and other unsupported
platforms refuse a service before spawn with E1332 when safe authority is not
available. Service lifecycle records retain the authority backend, generation,
phase, containment, dependencies, and recovery reason.

Readiness is separate from process start. The closed probe types are `exec`,
`http`, `notify`, and `tcp`, each with a bounded attempt:

```jet
module env.dev {
    services: {
        api: Service{
            enable: true,
            run: ["./bin/api", "--port", "8080"],
            ready: .http("http://127.0.0.1:8080/health", 200),
            ports: [8080],
            restart: .OnFailure{ max: 3, backoff_ms: 250 },
            after: ["database"]
        }
    }
}
```

`after` names a declared service dependency. Jetpack validates names, disabled
dependencies, and cycles before spawning; it starts dependencies before
dependents and stops the selected graph in reverse order. It reserves ports and
socket paths, checks process identity before signalling, and bounds restart
count and backoff. A failed pre-start job, process, readiness gate, or
prerequisite stops the affected dependent graph.

Built-in presets use one constructor registry for package reference,
executable, port, argument vector, readiness, and state setup. For example,
`postgres` uses `postgresql@nixpkgs`, `.jet/services/postgres/data`, loopback
port 5432, and `pg_isready`; `redis` uses `redis@nixpkgs`,
`.jet/services/redis/data`, loopback port 6379, and `redis-cli -p 6379 ping`.
Explicit `ports`, `run`, or `ready` values override only the corresponding
preset fact.

The service commands operate on that same supervised graph:

```text
jetpack services up [name]
jetpack services down [name]
jetpack services health [name]
jetpack services logs <name>
jetpack services wait [name]
```

## Foreign flakes and bounded projection

`jetpack bridge flake` reads a local `flake.nix` or `devenv.nix`, evaluates the
bounded supported surface with project-root authority, records foreign graph
facts in the lock, and prints a generated `env.dev` package projection. It does
not invoke `nix` as an unbounded subprocess or require `nix` on `PATH`.

A foreign field without a lossless `env.*` meaning is retained as an unmapped
fact and produces L0204. An unsupported, unsafe, or over-budget evaluator
request produces E1256. Imports stay below the flake project root; symlink
escapes, remote authority that is not configured, dynamic derivations, and
other unsupported evaluation surfaces fail rather than becoming empty or
invented Jet values. Exact input revisions, `follows` edges, output mappings,
and content fingerprints remain lock facts.

`jetpack import <flake.nix|shell.nix|default.nix>` uses the same bounded
projection for a migration into `env.jet`. Inputs without a proved catalog
mapping remain explicit gaps instead of becoming an arbitrary package.

## Build actions

Build recipes lower to one finite acyclic action graph. Fetches need an exact
source hash and cannot carry URL credentials (E1236); build tools are realized
package dependencies rather than host `/usr/bin` (E1238); install paths stay
below the package output root (E1237). The planner records declared inputs,
realized tools, effects, platform facts, and a plan fingerprint. Changing a
source, provider, recipe, tool, effect, or platform fact changes that identity,
so an old approval or output is not reused. Failed stages publish no output.

## Images and first-party integrations

`jetpack image <name>` builds a declared `.Oci` image from verified Hangar
outputs into a native OCI layout. An image may copy only regular,
project-relative, non-secret extra files. Managed environment files, dotenv
inputs, service state, and secret values are not image inputs. A selected
service set contributes the generated `/jet/supervise` entrypoint and the
projected argument vectors in dependency order. Missing verified executable
outputs, conflicting image paths, unsupported service projections, and invalid
extra files produce E1336.

Image output uses deterministic OCI metadata and records a projection report.
`jetpack image <name> --push <http-or-https-reference>` uses the native OCI
Distribution transport; a local path is an immutable copy target. Image
transport credentials remain host-owned.

The first-party integration vocabulary is closed:

- `android`
- `apple`
- `certificates`
- `hosts`
- `codex-agent`
- `editor`
- `cloud-credentials`
- `vault`

An integration lowers to ordinary package, file, provider, host-check, secret,
and grant facts. Secret-bearing inputs are represented by names only. Secret
values never enter plans, fingerprints, logs, Hangar metadata, or image
layers. Integrations are projections, not a package resolver or a separate
activation engine. A requested integration authority must have a persisted
trust grant; one-shot `--trust` does not invent that authority.

## Command and schema authority

Use `jetpack --help` for the command surface and the linked source modules for
field schemas. The package model owns manifest and lock facts, the environment
model owns typed environment projections, and Jetpack owns realization,
process supervision, Hangar lifecycle, and transport boundaries. A tool should
consume those facts rather than parse a second package or environment grammar.
