# Build-time main: Jet's safe answer to Jai `#run`

Date: 2026-06-30

Purpose: propose the full-power compile-time/build system shape the owner asked
for: something with Jai `#run` energy, but enforceable for professional and
enterprise use.

## Summary

Jet should have two build-time tools, each with one job:

- `comptime { ... }`: local compile-time execution inside a source file.
- `#Build fn build(ctx: BuildContext) -> BuildPlan ?`: package/workspace build
  orchestration, selected explicitly by the build surface.

This is the build-time equivalent of `fn main`, but stricter:

- no hidden entrypoint in imported modules;
- no ambient host authority;
- no generated AST injection;
- generated Jet source re-enters lexer -> parser -> sema;
- effects are pure by default, reproducible when world-touching, audited when
  ambient;
- dependency build code gets less authority than root build code.

Tower ballots raised:

- `D-BUILDENTRY1`: entrypoint shape.
- `D-BUILDPOLICY1`: authority and enterprise defaults.

## Why Jai is attractive

Jai's model is fun because build-time code feels like normal code:

```jai
Texture_Info :: #run build_texture_table();

#run {
    build_game();
}
```

That unlocks fast game workflows: asset packing, shader compilation, generated
tables, reflection-driven editors, custom build steps, and no separate Make/CMake
language.

Jet should import that power. It should not import unrestricted build execution.
Build-time code is supply-chain code. It can read secrets, mutate source, hit the
network, or generate malicious code before the program exists.

## Jet equivalent

```jet
build: Build.{
    entry: build
    profiles: [
        .{ name: "debug", optimize: .none },
        .{ name: "release", optimize: .speed },
        .{ name: "ci", optimize: .speed, security: .locked },
    ]
}

#Build("generate assets and schema")
fn build(ctx: BuildContext) #(Fs, Net) -> BuildPlan ? {
    sprites #= ctx.find("assets/**/*.png")
    schema #= ctx.fetch(
        "https://example.com/schema.json",
        sha256: "..."
    )?

    source #= generate_schema_module(schema)
    ctx.generate("generated/schema.jet", source)?

    return ctx.plan(
        sources: ["src/main.jet", "generated/schema.jet"],
        assets: sprites,
    )
}
```

Commands:

```text
jet build build.jet
jet run --build build.jet
jet graph
jet explain-build
jet audit-effects
```

Optional later sugar:

```text
jet run build.jet
```

Only when the file has exactly one `#Build` entrypoint and no runtime `main`.

## Separation from `comptime`

`comptime {}` is for local computation:

```jet
comptime {
    table_size #= compute_table_size()
}

buffer := [U8](size: $table_size)
```

`#Build` is for graph construction:

```jet
#Build("pack assets")
fn build(ctx: BuildContext) -> BuildPlan ? {
    ctx.generate("generated/assets.jet", pack_assets(ctx)?)?
    return ctx.plan(sources: ["src/main.jet", "generated/assets.jet"])
}
```

Do not merge them. A local comptime block should not silently choose toolchains,
write generated source, or run dependency build steps. A build entrypoint should
not become a second spelling for local constant evaluation.

## Authority model

Use D-CTEFFECT1 tiers:

| Tier | Meaning | Default |
| --- | --- | --- |
| 0 | pure comptime, no host access | always allowed |
| 1 | reproducible effects recorded in `.jet/lock`: `find`, `embed`, fixed-output `fetch(url, sha256:)` | allowed |
| 2 | ambient effects: unpinned network, env, exec, filesystem mutation, real time/random | denied unless audited and allowed |

Tier 2 requires all of:

- source gate: `#Impure("reason")`;
- effect declaration: `#(Fs, Net, Exec)` or specific effect row;
- command/profile/project/org permission;
- lock/provenance entry.

Root package policy:

- developer default: Tier 0 + Tier 1 allowed; Tier 2 prompts/errors unless
  `--allow-impure` or policy grants it;
- CI strict: `--locked`, no lock drift, Tier 2 denied unless explicitly granted;
- offline: no network except store-resolved fixed-output fetches;
- enterprise restricted: approved registries/tools/domains only.

Dependency package policy:

- Tier 0 + locked Tier 1 by default;
- no Tier 2 by default, even if root build allows Tier 2;
- sandboxed roots only;
- generated outputs hashed into package store fingerprint.

## BuildContext

All build authority should flow through `BuildContext`, not ambient `core.fs` or
raw process APIs.

Good:

```jet
fn build(ctx: BuildContext) #(Fs) -> BuildPlan ? {
    files #= ctx.find("src/**/*.jet")
    ctx.generate("generated/index.jet", make_index(files))?
    return ctx.plan(sources: files + ["generated/index.jet"])
}
```

Risky direct form:

```jet
fn build(ctx: BuildContext) -> BuildPlan ? {
    files #= fs.walk(".")       // authority not scoped by BuildContext
    process.run("curl ...")     // hard to audit/cache/replay
}
```

Recommendation: direct `core.fs`/`core.process` can exist in expert comptime
under `#Impure`, but build entrypoints should use `ctx` APIs so lock/provenance
can be complete.

## BuildPlan shape

MVP plan should support more than source lists:

- executable/library/test/plugin/custom-action targets;
- generated source under `build/generated/` or a canonical generated root;
- assets/resources;
- toolchain requirements;
- action graph with declared inputs/outputs;
- profile selection;
- package/workspace roots;
- local action cache keys;
- explain graph.

Illustrative:

```jet
return BuildPlan.{
    targets: [
        .Executable.{
            name: "game",
            sources: ["src/main.jet", "generated/assets.jet"],
            assets: sprites,
        },
        .Test.{
            name: "unit",
            sources: ["tests/**/*.jet"],
        },
    ],
}
```

## Generated code rule

All generated Jet source must re-enter the front end:

```text
generator output
  -> lexer
  -> parser
  -> sema
  -> TIR/codegen
```

Never:

- inject pre-checked AST;
- emit Rust as user truth;
- bypass diagnostics;
- let rustc explain generated-code mistakes.

Generated-code diagnostics pin to the trigger:

```jet
#Build("generate schema")
fn build(ctx: BuildContext) -> BuildPlan ? { ... }
```

The generated fragment can be optional context, not the primary user-facing
source of blame.

## Code in the wild

Game asset pack:

```jet
#Build("pack sprites and generate asset enum")
fn build(ctx: BuildContext) #(Fs) -> BuildPlan ? {
    sprites #= ctx.find("assets/sprites/*.png")
    atlas #= pack_sprites(sprites)?
    ctx.write_asset("build/assets/atlas.bin", atlas.bytes)?
    ctx.generate("generated/assets.jet", atlas.enum_source)?

    return ctx.plan(
        sources: ["src/main.jet", "generated/assets.jet"],
        assets: ["build/assets/atlas.bin"],
    )
}
```

C binding generation:

```jet
#Build("generate C bindings for local SDK")
fn build(ctx: BuildContext) #(Fs, Exec) -> BuildPlan ? {
    headers #= ctx.find("vendor/sdk/include/**/*.h")

    #Impure("run bindgen-compatible local SDK probe") {
        bindings #= ctx.bindgen(headers)?
        ctx.generate("generated/sdk.jet", bindings.source)?
    }

    return ctx.plan(sources: ["src/main.jet", "generated/sdk.jet"])
}
```

Enterprise locked build:

```text
jet build --profile=ci --locked --offline --deny-impure --sbom --provenance
jet audit-effects
jet explain-build generated/sdk.jet
```

## Other languages

Jai:

```jai
#run build_game();
```

Great flow; weak default audit boundary.

Zig:

```zig
pub fn build(b: *std.Build) void {
    const exe = b.addExecutable(.{ .name = "game" });
    b.installArtifact(exe);
}
```

Strong programmatic build file; separate build API and source file convention.

Rust:

```rust
fn main() {
    println!("cargo:rerun-if-changed=schema.json");
}
```

Simple and ecosystem-proven; stringly communication and historically broad host
access.

Bazel:

```python
genrule(
    name = "schema",
    srcs = ["schema.json"],
    outs = ["schema.jet"],
    cmd = "tool < $< > $@",
)
```

Excellent caching/action model; separate language and steep conceptual wall.

Nix:

```nix
derivation {
  name = "game";
  src = ./src;
  builder = ./builder.sh;
}
```

Excellent reproducibility; not a general app-language experience.

Jet target: Jai-level flow with Bazel/Nix-grade explainability.

## Creative extensions

1. **Build preview mode**

```text
jet explain-build --dry-run
```

Runs the build entrypoint in planning mode, records planned effects/actions, but
does not execute Tier 2 actions.

2. **Capability receipts**

Every `ctx` action returns a receipt handle:

```jet
schema_receipt #= ctx.fetch(url, sha256: "...")?
ctx.provenance.attach(schema_receipt)
```

3. **Generated-source dossiers**

```text
jet explain-build generated/assets.jet
```

Shows:

- build entrypoint;
- source inputs;
- generator function;
- action key;
- lock entries;
- owning package.

4. **Fun local mode, strict CI mode**

```text
jet build --dev-trust-root
jet build --profile=ci --locked --deny-impure
```

Same language, different policy. No source changes required.

5. **Dependency build quarantine**

Dependency build entrypoints run in a restricted sandbox. They can generate
package-local outputs but cannot read the root repo, home directory, env, or
network unless policy grants exact handles.

6. **Build graph as queryable data**

```text
jet query build "why generated/assets.jet"
jet query build "actions with Net"
jet query build "dependency builds requiring Exec"
```

This connects the build system to the semantic-index/query roadmap.

## Adversarial pass

Objection: this is too enterprise-heavy for game developers.

Answer: the default local path can be one file and one command. The policy model
is mostly invisible until world-touching effects appear. Game developers get
asset packing and generated code; studios get reproducibility.

Objection: `#Build` duplicates `comptime`.

Answer: no. `comptime` computes local values; `#Build` constructs package build
graphs. Merging them would hide authority and violate I8.

Objection: dependency builds will be annoying if Tier 2 is denied.

Answer: good. Dependencies should not read host env or execute arbitrary tools by
default. Fixed-output fetches, vendored sources, and declared toolchains cover
the common safe path.

Objection: generated source re-entry is weaker than AST injection.

Answer: yes by design. It preserves diagnostics, sema ownership, swappable
backend, and rustc-never-speaks. Full Jai message loop remains a future
sandboxed research track, not v1.

Objection: `BuildContext` could become an ambient authority bag.

Answer: only if implemented loosely. Capabilities must be exact handles with
roots/domains/tools/env vars named and recorded.

## Next decisions

Already raised:

- `D-BUILDENTRY1`
- `D-BUILDPOLICY1`

Likely follow-ons:

- `D-BUILDACTION1`: action graph shape and declared inputs/outputs.
- `D-BUILDTARGET1`: target kinds and lifecycle verbs.
- `D-BUILDCACHE1`: action-cache key contents.
- `D-BUILDLOCK1`: unified `.jet/lock` / provenance ownership.
- `D-BUILDTOOLCHAIN1`: toolchain packages and legacy probes.
- `D-BUILDPLUGIN1`: sandboxed build plugins.

Recommended build order:

1. Finish D-CTEFFECT1 fixed-output `fetch(url, sha256:)`.
2. Decide `D-BUILDENTRY1` and `D-BUILDPOLICY1`.
3. Implement `BuildContext` + minimal `BuildPlan`.
4. Add generated-source re-check and lock/provenance entries.
5. Add `jet graph`, `jet explain-build`, and `jet audit-effects`.
6. Integrate workspace members, profiles, package targets, and source packages.
