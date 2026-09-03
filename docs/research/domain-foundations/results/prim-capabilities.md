# Capabilities, authority, and plugins

## What I built

I built a host package that reads one input file, attenuates an `FS.Read` authority, loads a Jet sandbox plugin, and calls its exports. The host also runs a short process under an exact `Exec` authority and exposes start, status, and stop actions. Separate scratch packages probe effectful guests, resource-scoped filesystem authority, rich exports, and ABI version changes.

Files:

- `pkg/package.jet` — host package authority: `Exec`, `FS`, `IO`, `Mem.Alloc`.
- `pkg/run.jet:1-46` — file read, authority attenuation, plugin load/calls, hostile paths, service call.
- `pkg/service.jet:1-25` — process lifecycle under `Authority.from_rights(...).under(...)`.
- `pkg/plugin_src/package.jet`, `pkg/plugin_src/run.jet` — pure guest package.
- `pkg/plugin_src/build/run.wasm`, `pkg/plugin_src/build/run_wit/world.wit` — generated sandbox artifact and WIT.
- `pkg/plugin_src/.jet/cache/api/plugin__capability_guest.api` — frozen plugin API.
- `pkg/fs_guest_pkg`, `pkg/net_guest_pkg`, `pkg/rich_guest_pkg` — hostile effect and rich-ABI guests.
- `pkg/scoped_fs_pkg`, `pkg/broad_fs_pkg` — filesystem authority comparison.
- `pkg/version_guest_pkg` — API removal probe; source was restored after the check.

## What worked

- Authority facts: `scripts/agent/jet-env jet inspect facts` reports `Rights D-AUTHORITY-MODEL1` and `PackagePolicy D-PACKAGE-POLICY-SCOPE1`.
- Pure plugin build: `jet build pkg/plugin_src/run.jet --target=sandbox` produced `build/run.wasm (sandbox)` with `required effects: none`.
- Versioned scalar ABI: `world.wit` declares `jet:capability-guest@0.1.0`, `scale(f64,f64)->f64`, and `greet(string)->string`; the API snapshot records `api_version = 3` and `published_version = 0.1.0`.
- Host integration: `jet run pkg/run.jet --allow-fs --allow-exec` printed `file:inside=inside-capability`, `plugin:greeting=hello, Ada!`, and `plugin:scale=42.0`.
- Authority attenuation: `pkg/run.jet:11-12` narrows `FS.Read:<root>` to `FS.Read:<root>/build`; `pkg/run.jet:20` loads the guest with that value.
- Malformed and out-of-root modules fail cleanly: the host printed `plugin:malformed-rejected no plugin loaded for this handle` and `plugin:outside-rejected no plugin loaded for this handle`.
- No-follow protection works: a temporary `link.wasm` symlink caused `E1334: Authority path .../link.wasm is a symlink`; the fixture was removed after the check.
- Service lifecycle works: the host printed `service:started`, `service:exited-before=false`, `service:stopped`, `service:exited-after=true`, and `service:exit-success=false`.
- ABI freeze works: `/home/nate/Projects/Github/jet/scripts/agent/jet-env jet build /home/nate/.cache/jet-luna/dx3/prim-capabilities/version_guest_pkg/run.jet --target=sandbox` after removing export `two` returned `E1257` and named `export two was removed`.

Important host run:

```text
$ /home/nate/Projects/Github/jet/scripts/agent/jet-env jet run /home/nate/.cache/jet-luna/dx3/prim-capabilities/pkg/run.jet --allow-fs --allow-exec
file:inside=inside-capability
plugin:greeting=hello, Ada!
plugin:scale=42.0
plugin:malformed-rejected no plugin loaded for this handle
plugin:outside-rejected no plugin loaded for this handle
service:started
service:exited-before=false
service:stopped
service:exited-after=true
service:exit-success=false
```

The implementation matches `crates/jet-pkg-model/src/Prelude/Plugin.rs:119-177` for `FS.Read` root checks and `:200-223` for a zero-import linker. The process contract is documented at `docs/reference/core-library.md:1556-1599` and `:1730-1740`.

## Gaps

### prim-capabilities-G1 — capability-scoped host imports

**Tags:** `impossible`  **Severity:** blocks

A sandbox guest cannot receive a checked FS, Net, or Exec host import. This blocks an untrusted plugin that must open a host-granted directory or call a host service. It affects backend, CLI, GUI, and games plugins.

```text
$ /home/nate/Projects/Github/jet/scripts/agent/jet-env jet build /home/nate/.cache/jet-luna/dx3/prim-capabilities/fs_guest_pkg/run.jet --target=sandbox
Error [E1258]: A sandbox can't use any effect
Why: ... it uses: FS. Sandboxes run with zero host authority ...

$ /home/nate/Projects/Github/jet/scripts/agent/jet-env jet build /home/nate/.cache/jet-luna/dx3/prim-capabilities/net_guest_pkg/run.jet --target=sandbox
Error [E3304]: `core.net` is not available for target `sandbox`.
```

`core.plugin` accepts an `Authority` for module loading, but the generated `world.wit` has no imports. The host must do the effectful work and pass scalar data, or use trusted native code.

### prim-capabilities-G2 — resource-scoped authority for core.files

**Tags:** `impossible`  **Severity:** hurts

A package cannot bind a directory authority to ordinary `core.files` calls. `pkg/scoped_fs_pkg/package.jet:4` grants `IO`, `Mem.Alloc`, and `FS.Read:<root>`, but `fs.read(path)` still requires the bare `FS` effect and the package fails:

```text
$ /home/nate/Projects/Github/jet/scripts/agent/jet-env jet run /home/nate/.cache/jet-luna/dx3/prim-capabilities/scoped_fs_pkg/run.jet --allow-fs
Error [E1803]: Application authority is undecided for `FS`
Why: required_effects=FS, IO, Mem.Alloc; granted_effects=FS.Read:/home/nate/.cache/jet-luna/dx3/prim-capabilities/inputs, IO, Mem.Alloc; denied_effects=; denied_required_effects=none; undecided_effects=FS; authority=package.jet authority.holds
```

With broad `[FS, IO, Mem.Alloc]`, the same path-only program reads both `inside-capability` and `outside-capability`. `crates/jet-codegen/src/Prelude/CoreLib/Top/Text.rs:835-844` confirms `jet_std_fs_read` takes only a path. A library can manually validate paths; the shipped no-follow root boundary is only on `plugin.load` for WASM artifacts.

### prim-capabilities-G3 — rich versioned plugin values

**Tags:** `impossible`, `call-site`  **Severity:** hurts

The sandbox ABI cannot export records, lists, resources, callbacks, or async values. A structured export fails:

```text
$ /home/nate/Projects/Github/jet/scripts/agent/jet-env jet build /home/nate/.cache/jet-luna/dx3/prim-capabilities/rich_guest_pkg/run.jet --target=sandbox
Error [E1260]: A sandbox's exported function has an unsupported signature
Why: `pub fn sum` isn't one homogeneous `Int`, `Float`, `Bool`, or `Text` shape
```

The generated WIT is limited to homogeneous scalar functions. A library author must encode structured data as `Text`, split it into scalar calls, and maintain conversion code at both boundaries.

## Friction

- Every plugin call needs a typed method or scalar inference plus `??`; the host repeats this for two successful exports and three rejection paths.
- The authority must carry the canonical resolved executable path. `/run/current-system/sw/bin/sleep` was rejected until the policy named its resolved `/nix/store/.../coreutils` identity.
- `Plugin.load` returns a handle sentinel. Malformed and out-of-root loads both become `no plugin loaded for this handle` at the first call, so the host loses the load reason.

## Defects

None observed. E1257, E1258, E1260, E1334, E1803, and E3304 were expected diagnostics for the hostile probes.

## Battery notes

Not applicable. This is a primitive probe, not a critical-area probe.

## Verdict

Buildable today for pure, versioned, scalar, zero-import sandbox plugins.
The host can attenuate an `FS.Read` root for plugin artifact loading.
The host can expose start, status, and stop for a process under exact `Exec` authority.
Untrusted guests that need host FS, Net, or Exec are not buildable today.
Rich plugin values and resource-scoped ordinary file authority need the listed primitives.
