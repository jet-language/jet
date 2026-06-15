# Functional Pack File Debrief

**Status:** research / implementation feedback from `examples/jetpack/functional-pack.jet`.

This note captures what it felt like to model a future `pack.jet` in a functional
style: immutable data, top-level transforms, and a final typed module value. The
goal is not to copy `flake.nix`; it is to keep Nix's best property, declarative
function-shaped configuration, while making the authoring experience smaller,
clearer, and harder to misuse.

## What Worked

The functional shape is a good fit for Jetpack. A pack can be plain data plus
pure transformations:

```jet
val pack = Pack {
    sources: [
        nix_source("stable", "github:NixOS/nixpkgs/nixos-24.05"),
        nix_source("unstable", "github:NixOS/nixpkgs/nixpkgs-unstable"),
        core_source("mine", "path:../jet-pkgs"),
    ],
    overlays: [
        overlay("stable", ["ripgrep", "fd"]),
        overlay("unstable", ["neovim"]),
        overlay("mine", ["hello"]),
    ],
    prompt: "jetpack-fp",
};
```

Rough Nix equivalent:

```nix
let
  sources = {
    stable = "github:NixOS/nixpkgs/nixos-24.05";
    unstable = "github:NixOS/nixpkgs/nixpkgs-unstable";
    mine = "path:../jet-pkgs";
  };

  overlays = {
    stable = [ "ripgrep" "fd" ];
    unstable = [ "neovim" ];
    mine = [ "hello" ];
  };
in
{
  inherit sources overlays;
  prompt = "jetpack-fp";
}
```

That is already easier to explain than a Nix flake attrset with `inputs`,
`outputs`, `system`, `legacyPackages`, and nested `devShells`. Jet can make the
pack file read like domain data first, language mechanism second.

## Friction Points

### 1. Collection Transforms Are Not Ergonomic Enough Yet

The first version wanted to say:

```jet
fn flatten_overlay(o: Overlay) -> List<Package> {
    return o.packages.map((name: String) => pack_item(o.source.clone(), name));
}
```

Nix equivalent:

```nix
flattenOverlay = o:
  map (name: {
    source = o.source;
    inherit name;
  }) o.packages;
```

and:

```jet
fn packages(pack: Pack) -> List<Package> {
    return pack.overlays.reduce([], (acc: List<Package>, o: Overlay) => append_packages(acc, o));
}
```

Nix equivalent:

```nix
packages = pack:
  builtins.foldl'
    (acc: o: acc ++ flattenOverlay o)
    []
    pack.overlays;
```

That exposed inference/type-checker weakness around `map` and `reduce` returning
custom types. The working version had to fall back to explicit loops and pushes.

**Improve it:** make `map`, `filter`, `flat_map`, and `reduce` first-class,
heavily-tested authoring tools for config code, especially when lambdas return
structs, enums, JSON, or generic lists.

**Why this beats Nix:** Nix has `map`, but errors often point at generic
evaluation machinery. Jet can say: "this `map` returns `Package`, so the result
is `[Package]`" and put diagnostics at the lambda body.

Real-world pack example:

```jet
fn package_refs(pack: Pack) -> List<String> {
    return pack.overlays
        .flat_map((o: Overlay) => o.packages.map((name) => "{o.source}@{name}"));
}
```

Nix equivalent:

```nix
packageRefs = pack:
  builtins.concatMap
    (o: map (name: "${o.source}@${name}") o.packages)
    pack.overlays;
```

Short explanation: this is the natural way to turn grouped package declarations
into canonical refs. It should be boring.

### 2. Empty List Type Inference Needs Context

This failed:

```jet
pack.overlays.reduce([], (acc: List<Package>, o: Overlay) => append_packages(acc, o))
```

Nix equivalent:

```nix
builtins.foldl' (acc: o: appendPackages acc o) [] pack.overlays
```

because `[]` had no type, even though the lambda accumulator annotated
`List<Package>`.

**Improve it:** infer empty list type from the expected argument type of generic
functions. If the method signature is `reduce<U>(init: U, f: fn(U, T) -> U)`,
then `[]` can be solved from either the annotated `acc` or the expected return.

Better Jet:

```jet
val refs = overlays.reduce([], append_refs);
```

Nix equivalent:

```nix
refs = builtins.foldl' appendRefs [] overlays;
```

Fallback diagnostic if inference is impossible:

```text
error: I can't infer what kind of list `[]` is
help: write `[]: List<Package>` or give the receiving value a type
```

**Why this beats Nix:** Nix lists are dynamically typed, so mixed package refs
and source records can get far before failing. Jet should keep static safety
without forcing noise into the common case.

### 3. Field Access In Loop Heads Parsed Poorly

This shape failed during parsing:

```jet
for name in o.packages {
    out.push(pack_item(o.source.clone(), take name));
}
```

Nix equivalent:

```nix
map (name: packItem o.source name) o.packages
```

The workaround was:

```jet
val names = o.packages;
val source = o.source;
for name in names {
    out.push(pack_item(source.clone(), take name));
}
```

Nix equivalent of the workaround:

```nix
let
  names = o.packages;
  source = o.source;
in
map (name: packItem source name) names
```

**Improve it:** allow any expression after `in`, including field access, method
calls, indexing, ranges, and pipeline expressions.

Better Jet:

```jet
for pkg in pack.packages() {
    print(pkg.ref());
}
```

Nix equivalent:

```nix
map (pkg: pkg.ref) (packages pack)
```

**Why this beats Nix:** Nix commonly pushes users into nested `let` bindings to
name intermediate values. Jet should let users name values when it clarifies
intent, not because the grammar needs help.

### 4. `take` Is Correct But Verbose In Data Builders

Once the example moved owned strings into records and JSON values, helper
functions needed explicit `take`:

```jet
fn nix_source(take name: String, take upstream: String) -> Source {
    return Source {
        name: name,
        upstream: upstream,
        via: "nix",
    };
}
```

Nix equivalent:

```nix
nixSource = name: upstream: {
  inherit name upstream;
  via = "nix";
};
```

This is honest and safe, but pack files are mostly data constructors. Repeating
`take` everywhere makes small domain helpers look lower-level than they are.

**Improve it:** consider a concise value-constructor mode for pure functions, or
make by-value parameters the default for non-method top-level functions that
only consume arguments.

Possible option:

```jet
fn nix_source(name: owned String, upstream: owned String) -> Source {
    return Source { name, upstream, via: "nix" };
}
```

Nix equivalent:

```nix
nixSource = name: upstream: { inherit name upstream; via = "nix"; };
```

Possible shorthand:

```jet
fn nix_source(take name: String, take upstream: String) -> Source {
    return Source { name, upstream, via: "nix" };
}
```

Nix equivalent:

```nix
nixSource = name: upstream: { inherit name upstream; via = "nix"; };
```

The shorthand matters: field punning removes most of the visual tax.

**Why this beats Nix:** Nix hides sharing and laziness behind the evaluator. Jet
can be explicit about ownership while still giving config authors a clean,
data-first surface.

### 5. Struct Literal Repetition Is Noticeable In DSL Code

Current Jet:

```jet
return Source {
    name: name,
    upstream: upstream,
    via: "nix",
};
```

Nix equivalent:

```nix
{
  name = name;
  upstream = upstream;
  via = "nix";
}
```

Better Jet:

```jet
return Source { name, upstream, via: "nix" };
```

Nix equivalent:

```nix
{ inherit name upstream; via = "nix"; }
```

**Improve it:** add field punning for struct literals when a local binding has
the same name as the field.

**Why this beats Nix:** Nix attrsets are concise:

```nix
{ inherit name upstream; via = "nix"; }
```

Jet needs an equally concise spelling, with static field checking and better
errors when a field is missing or misspelled.

### 6. Pack Files Need Evaluation, Not Just Directive Scanning

Today Jetpack structurally scans directive calls like `pkg.source(...)` and
`pkg.packages(...)`. The functional example returns computed JSON from
functions, which works as a Jet program, but Jetpack itself does not yet
evaluate a typed module result.

**Improve it:** define the future pack ABI as a pure module declaration:

```jet
module root {
    sources: {
        default: github@NixOS/nixpkgs/nixos-24.05,
    },
    shells: {
        dev: Shell {
            packages: packages(system),
            env: env_vars(),
            prompt: "jetpack",
        },
    },
}
```

Conceptual lowered form:

```jet
pub pure fn module(system: System) -> PackModule {
    return PackModule {
        sources: sources(),
        shells: {
            "dev": Shell {
                packages: packages(system),
                env: env_vars(),
                prompt: "jetpack",
            },
        },
    };
}
```

Nix equivalent:

```nix
outputs = { nixpkgs, ... }:
let
  shellFor = system:
    let
      pkgs = import nixpkgs { inherit system; };
    in
    pkgs.mkShell {
      packages = packages system;
      shellHook = ''
        export PS1="jetpack $PS1"
      '';
    };
in
{
  devShells.x86_64-linux.default = shellFor "x86_64-linux";
  devShells.aarch64-darwin.default = shellFor "aarch64-darwin";
};
```

Jetpack should evaluate that function in a constrained mode:

- no network during evaluation
- no filesystem reads unless explicitly allowed
- deterministic output
- typed return value, not unstructured JSON as the long-term surface

**Why this beats Nix:** Nix flakes are powerful partly because `outputs` is a
function, but they are also hard to teach because that function returns a huge
open attrset. Jet can keep the function model and make the return type concrete.

## Ergonomic Targets For A Better-Than-Nix Pack File

### Typed Module Fields Instead Of Open Attrsets

Nix:

```nix
outputs = { nixpkgs, ... }: {
  devShells.x86_64-linux.default = pkgs.mkShell {
    packages = [ pkgs.ripgrep pkgs.fd ];
  };
};
```

Jet:

```jet
module root {
    shells: {
        dev: Shell {
            packages: [
                stable.ripgrep,
                stable.fd,
            ],
            prompt: "jetpack",
        },
    },
}
```

Short explanation: `Shell` is just a checked type inside a module field.
Unknown fields, wrong package ref shapes, and unsupported platforms become
local diagnostics instead of late evaluator failures.

### Plain Functions For Reuse

Nix:

```nix
commonPackages = pkgs: with pkgs; [ ripgrep fd jq ];
```

Jet:

```jet
fn common_tools(source: String) -> List<Package> {
    return ["ripgrep", "fd", "jq"].map((name) => pkg(source, name));
}
```

Expanded Nix equivalent:

```nix
commonPackages = pkgs:
  map (name: pkgs.${name}) [ "ripgrep" "fd" "jq" ];
```

Short explanation: reuse should be just a function. No special module language,
no overlays for simple composition.

### First-Class Platform Selection

Nix:

```nix
packages = with pkgs; [ ripgrep ] ++ lib.optionals stdenv.isDarwin [ cocoapods ];
```

Jet:

```jet
fn platform_tools(system: System) -> List<Package> {
    return match system.os {
        "darwin" => [pkg("stable", "cocoapods")],
        "linux" => [pkg("stable", "strace")],
        _ => [],
    };
}
```

Short explanation: platform branching should be explicit and typed. Jet can
teach exhaustiveness and show available platform fields.

### Better Package Ref Errors

Jet should reject bad refs before resolution:

```jet
pkg("stable", "rip grep")
```

Nix equivalent mistake:

```nix
pkgs."rip grep"
```

Diagnostic goal:

```text
error: package names cannot contain spaces
 --> pack.jet:12:20
fix: did you mean `ripgrep`?
```

Short explanation: Nix often fails deep inside an attr lookup or derivation.
Jetpack owns package refs, so it can validate them at the authoring boundary.

### Typed Environment Variables

Nix:

```nix
RUST_LOG = "debug";
DATABASE_URL = "postgres://localhost/app";
```

Jet:

```jet
env: [
    env("RUST_LOG", "debug"),
    secret("DATABASE_URL"),
],
```

Short explanation: Jetpack can distinguish literal env vars from required
secrets. That is more operationally useful than arbitrary attrsets.

## Recommended Language Work

1. Make `map`, `flat_map`, `filter`, and `reduce` robust with custom return
   types and contextual empty-list inference.
2. Allow arbitrary expressions in `for ... in ...` heads.
3. Add struct field punning: `Source { name, upstream, via: "nix" }`.
4. Improve ownership ergonomics for pure value constructors, without hiding
   moves in general application code.
5. Consider typed Jetpack return models (`Shell`, `Package`, `Source`) instead
   of JSON as the long-term pack ABI.
6. Keep directive scanning only as the bootstrap phase; move toward constrained
   pure evaluation of `module` declarations.

## Bottom Line

Jet can beat Nix for pack files if it stays typed, direct, and domain-shaped.
The biggest opportunity is not a clever DSL; it is making ordinary functional
Jet code feel excellent for small declarative transforms. If config authors can
write plain functions over typed records and lists, then Jetpack gets Nix's
composability without Nix's evaluator mystique.

## Syntax Decision Matrix

The examples above are still too verbose in a few places. The goal should be:
Jetpack pack files are at least as short as equivalent Nix for routine package
definitions, while staying typed and beginner-readable. These decisions are
ranked by payoff for `pack.jet`, with the smallest viable implementation path
called out.

| Decision | Current Jet / Problem | Nix Baseline | Proposed Jet | Payoff | Implementation Shape |
|---|---|---|---|---|---|
| **D-FP1: Package Ref Literal** | `pkg("default", "ripgrep")` repeats quotes and call syntax. | `pkgs.ripgrep` or a string package ref. | `default.ripgrep` as a typed `PackageRef` in pack contexts. | Very high: package lists become Nix-short or shorter. | Jetpack parser/sema: recognize `<source>.<package>` where a `PackageRef` is expected. |
| **D-FP2: Source-Scoped Package Lists** | `overlay("default", ["ripgrep", "fd"])` repeats grouping function names. | `with pkgs; [ ripgrep fd ]` | `default.[ripgrep, fd]`. | Very high: removes most package declaration noise. | Jetpack-specific syntax is smaller; general language syntax is riskier. Prefer pack-file sugar first. |
| **D-FP3: Field Punning** | `Source { name: name, upstream: upstream, via: "nix" }` is noisy. | `{ inherit name upstream; via = "nix"; }` | `Source { name, upstream, via: "nix" }` | High: broadly useful, matches Nix `inherit`. | Small parser/sema/codegen change for struct literals. |
| **D-FP4: Module Data Literals** | `pub fn module() -> PackModule { return jp.PackModule(...); }` repeats the output type and constructor. | `{ packages = ...; }` | `module root { shells: { dev: Shell { ... } } }` | High: uses one composable declaration form and ordinary types. | Core/top-level `module` declarations lower to typed pure fragments. |
| **D-FP5: Bare Package Names In Scoped Lists** | `["ripgrep", "fd"]` loses typed package identity; `pkg("default", "ripgrep")` is repetitive. | `[ ripgrep fd ]` | `packages: default.[ripgrep, fd]` | Very high for package lists. | Jetpack parser can desugar names to `PackageRef`s before normal Jet evaluation. |
| **D-FP6: List Spread / Concatenation** | Building package lists needs helper functions or loops. | `[ ripgrep ] ++ optionals isDarwin [ cocoapods ]` | `[default.[ripgrep], ...when(system.darwin, [default.cocoapods])]` | High: platform/env composition becomes compact. | General list spread is useful; `when(cond, list)` can be library. |
| **D-FP7: Contextual Empty Lists** | `reduce([]...)` fails without explicit type. | `[]` just works. | `val refs = overlays.reduce([], append_refs);` | Medium: removes type noise in transforms. | Type inference improvement, no syntax change. |
| **D-FP8: Expression Bodies** | Tiny constructors require `return ...;` blocks. | `name: upstream: { ... }` | `fn nix_source(name: String, upstream: String) -> Source = Source { name, upstream, via: "nix" };` | Medium-high: makes pure helpers Nix-compact. | Parser + formatter + sema; not a fundamental rewrite. |
| **D-FP9: Pipeline / Dot Transform Robustness** | Desired `pack.overlays.flat_map(...)` hit inference friction. | `concatMap (o: ...) overlays` | `pack.overlays.flat_map((o) => o.packages.map((name) => default.pkg(name)))` | Medium: makes functional style natural. | Typechecker/library hardening; syntax mostly exists. |
| **D-FP10: Pack ABI Types** | Current working example returns `List<JSON>`, which is verbose and weakly typed. | Open attrsets: flexible but opaque. | One `module` declaration containing typed fields: `sources`, `packages`, `shells`, `profiles`, `systems`, `images`. | High: better errors than Nix with less boilerplate. | Jetpack runtime/API work plus small core parse support for `module`. |

### Highest-Value Recommendation

For package definitions specifically, add a Jetpack package-list shorthand before
attempting broader language redesign. The current verbose form:

```jet
val pack = Pack {
    sources: [
        nix_source("default", "github:NixOS/nixpkgs/nixos-24.05"),
        nix_source("unstable", "github:NixOS/nixpkgs/nixpkgs-unstable"),
    ],
    overlays: [
        overlay("default", ["ripgrep", "fd"]),
        overlay("unstable", ["neovim"]),
    ],
    prompt: "jetpack",
};
```

Nix baseline:

```nix
{
  packages = with pkgs; [ ripgrep fd neovim ];
}
```

Proposed Jetpack surface:

```jet
module root {
    sources: {
        default: github@NixOS/nixpkgs/nixos-24.05,
        unstable: github@NixOS/nixpkgs/nixpkgs-unstable,
    },
    shells: {
        dev: Shell {
            packages: [
                default.[ripgrep, fd],
                unstable.neovim,
            ],
            prompt: "jetpack",
        },
    },
}
```

This gives Nix-like brevity without Nix's untyped attrset behavior. `default`
is the fallback source for unqualified package refs, and all source declarations
live in one `sources:` shape.

### Package Shorthand Options

| Option | Example | Compared To Nix | Recommendation |
|---|---|---|---|
| String refs | `packages: ["default@ripgrep", "default@fd"]` | Similar length, but untyped strings. | Keep as compatibility, not the best authoring form. |
| Function calls | `packages: [pkg("default", "ripgrep"), pkg("default", "fd")]` | Much more verbose than `[ ripgrep fd ]`. | Useful in generated code; too noisy for humans. |
| Dot refs | `packages: [default.ripgrep, default.fd]` | Very close to `pkgs.ripgrep`; familiar to Nix users. | Best single-package spelling. |
| Scoped package list | `packages: [default.[ripgrep, fd], unstable.neovim]` | Same density as `with pkgs; [ ripgrep fd ]`, but multi-source. | Best overall shorthand for Jetpack. |
| Default source bare names | `packages: [ripgrep, fd, unstable.neovim]` | Shorter than Nix when the source is obvious. | Best default-source experience; must be limited to package-list context. |
| `from` helper | `packages: [from(default, [ripgrep, fd]), unstable.neovim]` | Longer than Nix, but implementable as a library helper. | Fallback only if scoped-list syntax is deferred. |

Recommended package syntax:

```jet
module root {
    sources: {
        default: github@NixOS/nixpkgs/nixos-24.05,
        unstable: github@NixOS/nixpkgs/nixpkgs-unstable,
    },
    shells: {
        dev: Shell {
            packages: [
                default.[ripgrep, fd],
                unstable.neovim,
            ],
        },
    },
}
```

Equivalent Nix:

```nix
{
  devShells.x86_64-linux.default = pkgs.mkShell {
    packages = with pkgs; [
      ripgrep
      fd
      unstable.neovim
    ];
  };
}
```

The Jet version is essentially the same verbosity, but it can make `ripgrep` a
typed `PackageRef`, validate that `unstable` is a declared source, and give a
local diagnostic if a package ref is malformed.

### Preferred Minimal Path

1. **Add typed Jetpack ABI types**: `Shell`, `Source`, `PackageRef`.
2. **Add Jetpack-context package refs** inside `Shell.packages`: `ripgrep`,
   `default.ripgrep`, and `default.[ripgrep, fd]`.
3. **Add core module declarations** so `module root { ... }` and
   `module vscode { ... }` lower to typed pure fragments and remain visible to
   Jet LSP.
4. **Add field punning** for all Jet struct literals: `Source { name, upstream }`.
5. **Harden collection transform inference** so advanced users can still build
   package lists functionally without dropping to loops.

This path keeps the main language coherent, avoids a full rewrite, and puts the
most domain-specific magic exactly where users expect it: inside `pack.jet`
package lists.

## Documentation Audit Of Proposed Core Features

This pass checks the syntax ideas above against existing decisions and plans.
The important pattern: several things that felt like "core language" work are
already planned as Jetpack/JetOS library or evaluator work. Keep them there
unless they clearly help ordinary Jet programs too.

| Proposal | Existing Decision / Plan | Status | Reason / Boundary |
|---|---|---|---|
| Field punning: `Source { name, upstream }` | S29 ratifies `Type { field: expr }`; no explicit punning decision found. | **New small core proposal.** | Good candidate because it helps all struct-heavy Jet code, not only Jetpack. It extends S29 without replacing it. |
| Expression-body functions: `fn f(...) -> T = expr;` | S1 ratifies `fn`; no expression-body function decision found. Lambdas already allow expression bodies (S46). | **New core proposal, optional.** | Useful, but less urgent if Jetpack authoring can avoid tiny helper functions through a library surface. |
| Better `map`/`reduce` inference | M8 planned `map`, `filter`, `reduce`; lambda types are inferred from expected function type. Current implementation has gaps. | **Already planned/partially implemented; improve quality.** | This is not new syntax. It is typechecker/library hardening. |
| Newlines in dot chains | S69 ratified, implementation pending. | **Already planned.** | Supports readable fluent/library APIs without a pipe operator. |
| Pipe operator `|>` | Syntax gallery says undecided; S69 intentionally keeps pipe open. | **Not rejected, but not needed for Jetpack now.** | A Jetpack library can be clean with dot chains and typed records. Avoid adding pipe just for config. |
| Pure pack evaluation | S60 ratifies `pure fn`; Jetpack README says pure-eval enforcement of `pack.jet` is Phase 2/nice-to-have. | **Already planned, later.** | This is the right long-term flake-equivalent guarantee, but not required for the first ergonomic library. |
| Typed Jetpack ABI (`Shell`, `PackageRef`, `Source`) | D-JPK3 says Phase 1 directives now, intended evolution is a first-party fluent Jetpack module. Jetpack config brief Path A recommends first-party `jetpack` module. | **Already planned direction.** | This is the best boundary: library/module types, not global syntax. |
| Bare package identifiers (`ripgrep`) | Jetpack config brief D-JP5 recommended **strings in v1**, revisit typed package names with Path A. | **Deferred for Phase 1, viable in pack contexts.** | Do not make bare package names normal Jet identifiers. Allow them only where the expected type is `PackageRef` inside Jetpack pack literals. |
| Contextual `packages: [ripgrep]` magic | S14 says one canonical spelling; D-JP5 warns bare package names need a generated namespace/index. | **Acceptable only as Jetpack-context syntax.** | The boundary must be explicit: inside `packages:` lists, names are package refs; outside, normal Jet name rules apply. |
| `<source>:<package>` refs | D-JPK7/D-JPK15 ratify CLI/package refs as `<source>:<package/path>`; D-JPK17 ratifies named sources used inline in `pkg.packages([...])`. | **Already ratified for Phase 1; revise for next surface.** | Prefer `source@package` for the new authoring and CLI model to avoid colon ambiguity with source/provider refs. Keep colon strings as a compatibility parser only. |
| `default.[ripgrep, fd]` scoped list syntax | D-JPK17 ratified inline named-source refs for Phase 1; grouped source syntax was not the Phase 1 surface. | **Owner-preferred next Jetpack syntax.** | Keep it Jetpack-specific and type-directed; do not generalize `x.[...]` to all Jet expressions unless separately justified. |
| Core `module` declaration | Existing docs have no top-level module declaration for composable config fragments. S14 discourages duplicate constructor forms, and LSP should parse pack files without Jetpack-only syntax injection. | **New small core proposal.** | `module vscode { ... }` is a typed declaration that lowers to a public pure fragment. Shells/profiles/systems are fields/types, not keywords. |
| Single `sources:` map with `default` key | D-JPK17 already supports a default source and named sources, but Phase 1 spells them as directive calls. | **Preferred next Jetpack syntax.** | Put all source declarations in one typed `sources:` map; `default` is the fallback for unqualified package refs. |
| List spread `...xs` | No matching decision found; S14/smallness cautions apply. | **New core proposal; not necessary now.** | Useful generally, but package composition can be handled by `jp.packages(...)`, `jp.when(...)`, and list methods until broader evidence exists. |
| JetOS option syntax (`option`, `default`, `force`) | JetOS design D-OS1/D-OS2 recommend it, but Jetpack config brief says Path B is later and gated on pure eval/M12 layer 3. | **Planned for JetOS, not Jetpack Phase 1.** | Do not pull option syntax into ordinary Jetpack package files yet. |

**Recommendation from the audit:** make a first-party `jetpack` library/module
plus one core-parseable `module` declaration the ergonomic layer. Keep shells,
profiles, systems, and images as ordinary typed fields/values. Jet LSP parses
the same shape everywhere; Jetpack supplies schemas and merge behavior.

## Cleaner Jetpack Library Boundary

The target surface should be less repetitive than `jp.pkg("default", "ripgrep")`
and should also avoid repeating `shell` / `jp.shell`. A clean compromise is:

- `jetpack` provides typed values: `Shell`, `Env`, `Source`, `PackageRef`,
  `System`, and later JetOS `Module`/`Option`.
- Jet core has one typed declaration form for config fragments:
  `module vscode { ... }`.
- In fields whose expected type is `List<PackageRef>`, Jetpack interprets
  `default.[ripgrep, fastfetch]`, `unstable.neovim`, and default-source bare
  names as package refs.
- Strings remain valid for dynamic or unknown packages, with diagnostics.

Cleaner target with a Jetpack pack prelude:

```jet
module root {
    sources: {
        default: github@NixOS/nixpkgs/nixos-24.05,
        unstable: github@NixOS/nixpkgs/nixpkgs-unstable,
    },
    shells: {
        dev: Shell {
            packages: [
                default.[ripgrep, fastfetch],
                unstable.neovim,
                "mine@hello",
            ],
            env: [
                env("RUST_LOG", "debug"),
                secret("DATABASE_URL"),
            ],
            prompt: "jetpack",
        },
    },
}
```

Equivalent lowered shape, for implementation:

```jet
import jetpack as jp;

pub pure fn module() -> jp.PackModule {
    return jp.PackModule {
        sources: [
            jp.source("default", "github:NixOS/nixpkgs/nixos-24.05"),
            jp.source("unstable", "github:NixOS/nixpkgs/nixpkgs-unstable"),
        ],
        shells: {
            "dev": jp.Shell {
                packages: [
                    jp.pkg("default@ripgrep"),
                    jp.pkg("default@fastfetch"),
                    jp.pkg("unstable@neovim"),
                    jp.pkg("mine@hello"),
                ],
                env: [
                    jp.env("RUST_LOG", "debug"),
                    jp.secret("DATABASE_URL"),
                ],
                prompt: "jetpack",
            },
        },
    };
}
```

Why this is cleaner than the earlier target:

- no raw `JSON`
- no required `import jetpack as jp` in ordinary pack files
- no repeated `shell` / `jp.shell` constructor call in the authoring form
- `Shell`, `Profile`, `System`, and `Image` are ordinary typed values, not new
  keywords
- no `Package { source: ..., name: ... }` boilerplate
- package groups are concise like Nix, but still typed by context
- string refs such as `"mine@hello"` are the escape hatch for refs not covered
  by shorthand syntax
- it keeps the special interpretation inside typed `pack.jet` declarations
- it can lower to ordinary Jet structs, preserving the core language model

### Type-Directed Pack Data

This is the underlying cleanup: pack files should not force authors to name the
same shape three times. When the expected type is known, Jetpack can interpret a
plain data literal.

Verbose shape:

```jet
pub pure fn module() -> jp.PackModule {
    return jp.PackModule {
        shells: {
            "dev": jp.Shell {
                packages: [
                    jp.pkg("stable@ripgrep"),
                    jp.pkg("stable@fastfetch"),
                ],
            },
        },
    };
}
```

Type-directed module shape:

```jet
module devShell {
    shells: {
        dev: Shell {
            packages: [default.[ripgrep, fastfetch]],
        },
    },
}
```

Rules:

- `module vscode { ... }` gives the body an expected type of `PackModule`.
- `shells.dev: Shell` or `dev: Shell { ... }` gives the body an expected type
  of `Shell`.
- declarations are implicitly public in `pack.jet` and lower to pure exported
  fragments.
- `sources:` is the only source declaration shape. It is a typed source map.
- `default` is the special source key used for packages without an explicit
  source. In these examples it maps to `github@NixOS/nixpkgs/nixos-24.05`.
- `packages:` gives the list an expected item type of `PackageRef`.
- `default.[ripgrep, fastfetch]` expands to two `PackageRef` values.
- `unstable.neovim` expands to one `PackageRef`.
- `sources: { unstable: github@NixOS/nixpkgs/nixpkgs-unstable }` is a typed
  source map, not a general Jet anonymous object.
- The package/source shorthand only applies inside fields whose expected types
  are Jetpack types. The declaration syntax itself is core Jet syntax.

This gets Nix-level concision without adding anonymous objects or package-name
magic to the whole Jet language.

### Source Ref Spelling Options

The helper-call shape is too noisy:

```jet
default: github("NixOS/nixpkgs", "nixos-24.05")
```

Preferred source refs should be data, not function calls. The source provider
is the left side, and the provider-specific path is the right side.

| Option | Example | Pros | Cons | Recommendation |
|---|---|---|---|---|
| At ref | `github@NixOS/nixpkgs/nixos-24.05` | Avoids `:` collision; reads like provider-at-target; works cleanly with typed `sources:` maps. | `@` must be reserved for source refs in pack contexts. | **Recommended.** |
| Colon URI | `github:NixOS/nixpkgs/nixos-24.05` | Short and URL-like. | `:` is already busy in Jet syntax, and package refs already moved away from colon. | Reject for authoring; keep string compatibility only. |
| Arrow | `github -> NixOS/nixpkgs/nixos-24.05` | Visually separates provider and target. | Too much punctuation; `->` already means return/arms. | Reject. |
| Double colon | `github::NixOS/nixpkgs/nixos-24.05` | Namespacing feel. | `::` was rejected/reserved around paths/enums; looks Rusty. | Reject. |
| String only | `"github:NixOS/nixpkgs/nixos-24.05"` | Easy to implement. | Loses typed source shape and validation until runtime. | Keep as compatibility/escape hatch. |
| Function call | `github("NixOS/nixpkgs", "nixos-24.05")` | Normal Jet, no special parser. | Verbose and repeats quotes/commas for the common case. | Use only in lowered/internal examples. |

Recommended authoring form:

```jet
module root {
    sources: {
        default: github@NixOS/nixpkgs/nixos-24.05,
        unstable: github@NixOS/nixpkgs/nixpkgs-unstable,
        mine: path@../jet-pkgs,
    },
    shells: {
        dev: Shell {
            packages: [
                default.[ripgrep, fastfetch],
                unstable.neovim,
                mine.hello,
            ],
        },
    },
}
```

Cleanest common-case surface:

```jet
module root {
    sources: {
        default: github@NixOS/nixpkgs/nixos-24.05,
    },
    shells: {
        dev: Shell {
            packages: [default.[ripgrep, fd, jq]],
        },
    },
}
```

This is shorter than a flake dev shell because the library provides sane
defaults: source = `nixpkgs`, prompt = directory/project name, no named sources,
no env. The fuller form only appears when the user actually needs those knobs:

```jet
module root {
    sources: {
        default: github@NixOS/nixpkgs/nixos-24.05,
        unstable: github@NixOS/nixpkgs/nixpkgs-unstable,
    },
    shells: {
        dev: Shell {
            packages: [
                default.[ripgrep, fd],
                unstable.neovim,
            ],
            prompt: "jetpack",
        },
    },
}
```

This relies on a first-party Jetpack module, not normal user-defined cross-file
methods. That matches the existing Path A recommendation in
`jetpack-config-brief.md`.

### Naming Options For Pack Composition

Keep the syntax surface small: use one declaration keyword, `module`.
Everything else should be an ordinary type or field.

| Concept | Options | Recommendation | Reason |
|---|---|---|---|
| Composable file/unit | `module vscode { ... }` or implicit file module | **`module vscode { ... }`** | Best match for NixOS/flake-parts mental model and JetOS later. |
| Root file identity | `module root { ... }` or implicit root module | **`module root { ... }`** | Avoids a second declaration keyword. Root `pack.jet` is just the root module. |
| Dev environment | `shells: { dev: Shell { ... } }` | **`shells: { dev: Shell { ... } }`** | `Shell` is a type, not syntax. |
| User package set | `profiles: { user: Profile { ... } }` | **`profiles: { user: Profile { ... } }`** | Keeps profile as data. |
| Whole-machine target | `systems: { laptop: System { ... } }` | **`systems: { laptop: System { ... } }`** | Keeps JetOS targets as typed data. |
| ISO / image target | `images: { installer: Image { ... } }` | **`images: { installer: Image { ... } }`** | Generalizes beyond ISO later without new syntax. |

Recommended authoring vocabulary:

```text
module root { ... }       # root aggregate
module vscode { ... }     # imported contribution
Shell { ... }             # dev environment value
Profile { ... }           # user profile value
System { ... }            # JetOS whole-machine value
Image { ... }             # ISO/VM/image value
```

This keeps `module` for composability/importability and avoids extra accordion
syntax like `pack`, `shell`, `profile`, `system`, and `image`.

### Dispersed Pack Files

Jetpack should support flake-parts-style composition without making authors
maintain one giant file. A fragment should be self-contained: it can add its
own source, packages, shells, settings, or later JetOS options. The root imports
the tree and merges typed contributions.

Recommended project shape:

```text
my-project/
  pack.jet
  pack.lock
  apps/
    vscode.jet
    ai.jet
    media.jet
  shells/
    dev.jet
  systems/
    laptop.jet        # later JetOS
```

Root `pack.jet`:

```jet
module root {
    sources: {
        default: github@NixOS/nixpkgs/nixos-24.05,
    },
    imports: tree([
        "./apps",
        "./shells",
    ]),
}
```

`apps/vscode.jet` is self-contained:

```jet
module vscode {
    sources: {
        vscode-source: github@microsoft/vscode/main,
    },
    packages: [
        vscode-source.vscode,
    ],
    settings: {
        "editor.formatOnSave": true,
        "terminal.integrated.defaultProfile.linux": "bash",
    },
}
```

That means a GUI, CLI, or user can add `apps/vscode.jet` without editing the
root `pack.jet`. The import tree finds the file, Jetpack merges the declared
source into the source table, then resolves `vscode-source.vscode`.

`shells/dev.jet`:

```jet
module devShell {
    shells: {
        dev: Shell {
            packages: [
                default.[ripgrep, fd, jq],
            ],
        },
    },
}
```

Merge rules should be typed, not implicit text concatenation:

| Field | Merge Rule |
|---|---|
| `sources` | merge by key; duplicate source names with different refs are conflicts unless explicitly overridden |
| `packages` | concatenate, de-duplicate, preserve source identity |
| `shells` | merge by shell key; package lists combine, scalar fields conflict unless priority-marked |
| `profiles` | merge by profile key; package lists combine |
| `systems` | merge by system key; scalar conflicts are diagnostics unless explicitly overridden |
| `images` | merge by image key; scalar conflicts are diagnostics unless explicitly overridden |
| scalar settings | one value wins only by explicit priority such as `default`/`force` in JetOS-style layers |

This gives Jetpack the flake-parts/import-tree model and gives JetOS a direct
foundation. A future JetOS feature file can be the same shape:

```jet
module desktop.vscode {
    sources: {
        vscode-source: github@microsoft/vscode/main,
    },
    packages: [
        vscode-source.vscode,
    ],
    options: [
        set("apps.vscode.enable", true),
    ],
}
```

Why this should be core-parseable:

- `module` can be an ordinary typed top-level declaration in Jet syntax.
- `Shell`, `Profile`, `System`, and `Image` remain normal types.
- LSP can parse, rename, outline, and type-check them without Jetpack injecting
  a separate grammar.
- Jetpack supplies schemas and merge semantics for the declaration types.
- `pack.jet` remains a Jet file, not a second DSL with separate tooling.

## Three Use Cases

These should stay conceptually separate. They can share the Jetpack resolver,
store, lock model, and `jetpack` module, but the files mean different things.

### Category 1: Jet Language Project Package Manager

Purpose: dependencies for a project written in Jet. This is Cargo-like, not
NixOS-like. It should keep using `jet.toml` / `jet.lock` per S52 and M12.
`pack.jet` is optional and only describes a dev shell or external tools.

Project structure:

```text
wordstats/
  jet.toml          # Jet package identity + Jet library dependencies
  jet.lock          # tool-owned exact Jet dependency graph
  main.jet
  src/
    parser.jet
    report.jet
  tests/
    wordstats_test.jet
  pack.jet          # optional dev shell: ripgrep, jq, sqlite, etc.
  pack.lock         # optional Jetpack lock for external tools/env
```

`jet.toml` remains useful only for the current Jet package manager boundary:

```toml
[package]
name = "wordstats"
version = "0.1.0"
jet = ">=0.1.0"
description = "Count words in plain text."
license = "MIT OR Apache-2.0"

[dependencies]
textkit = "1.2.0"
helpers = { path = "../helpers" }
```

Optional `pack.jet` for project tools:

```jet
module root {
    sources: {
        default: github@NixOS/nixpkgs/nixos-24.05,
    },
    shells: {
        dev: Shell {
            packages: [default.[ripgrep, fd, jq, sqlite]],
            env: [
                env("RUST_LOG", "debug"),
            ],
            prompt: "wordstats",
        },
    },
}
```

Boundary: `jet.toml` answers "what Jet code does this project depend on?"
`pack.jet` answers "what external tools should be in this project's shell?"

### Category 2: Jetpack As A System Package Manager, Not JetOS

Purpose: a user wants Jetpack as a package/environment manager on an existing
Linux/macOS system. This is closer to `nix profile`, `nix shell`, `home-manager`
lite, or `devenv`, but it does not own the OS.

Project structure:

```text
~/.config/jetpack/
  pack.jet          # user's packages, shells, and app environments
  pack.lock         # exact resolved package/source graph
  shells/
    ai.jet
    media.jet
  overlays/
    local-tools.jet
  state/            # tool-owned generations/profile metadata
```

Root `pack.jet`:

```jet
module root {
    sources: {
        default: github@NixOS/nixpkgs/nixos-24.05,
        unstable: github@NixOS/nixpkgs/nixpkgs-unstable,
    },
    profiles: {
        user: Profile {
            packages: [
                default.[ripgrep, fd, bat, eza, git],
                unstable.neovim,
            ],
        },
    },
    shells: {
        ai: Shell {
            packages: [default.[claude-code, nodejs]],
        },
        media: Shell {
            packages: [default.[ffmpeg, yt-dlp]],
        },
    },
}
```

Example command model:

```text
jetpack profile switch
jetpack profile rollback
jetpack enter ai
jetpack run default@fastfetch
```

Boundary: Jetpack can manage packages, shells, profiles, generations, and
rollback. It should not configure bootloaders, system services, users, kernel
modules, or declarative OS state. That is Category 3.

### Category 3: JetOS

Purpose: declarative whole-machine configuration with eventual NixOS parity.
This needs options, modules, host outputs, activation, generations, ISO/VM
outputs, and pure evaluation. It is Phase 2 and should build on Jetpack.

Project structure:

```text
jetos-config/
  pack.jet              # inputs, hosts, ISO outputs, provider selection
  pack.lock             # exact source/package/module graph
  hosts/
    laptop.jet
    workstation.jet
    iso.jet
  modules/
    core/
      boot.jet
      networking.jet
      users.jet
    apps/
      desktop.jet
      developer.jet
      gaming.jet
    desktop/
      kde.jet
      gnome.jet
    services/
      pipewire.jet
      bluetooth.jet
  overlays/
    packages.jet
    patches/
      firefox.patch
  assets/
    wallpapers/
      default.png
  generated/
    tree.jet            # generated import tree, or replaced by pure module discovery
```

Root `pack.jet`:

```jet
import jetos;

module root {
    sources: {
        default: github@NixOS/nixpkgs/nixos-24.05,
        unstable: github@NixOS/nixpkgs/nixpkgs-unstable,
    },
    systems: {
        laptop: System {
            target: "x86_64-linux",
            module: "./hosts/laptop.jet",
        },
        workstation: System {
            target: "x86_64-linux",
            module: "./hosts/workstation.jet",
        },
    },
    images: {
        installer: Image {
            target: "x86_64-linux",
            module: "./hosts/iso.jet",
        },
    },
    modules: jetos.discover("./modules"),
    overlays: ["./overlays/packages.jet"],
}
```

Lowered conceptually to outputs like:

```jet
systems: [
        jetos.host("laptop", system: "x86_64-linux", module: "./hosts/laptop.jet"),
        jetos.host("workstation", system: "x86_64-linux", module: "./hosts/workstation.jet"),
        jetos.iso("installer", system: "x86_64-linux", module: "./hosts/iso.jet"),
]
```

Example host module:

```jet
import jetos;

module laptop {
    packages: [
        default.[firefox, fastfetch, btop],
    ],
    services: [
        jetos.service("pipewire").enable(),
        jetos.service("bluetooth").enable(),
    ],
    options: [
        jetos.set("sys.networking.hostName", "laptop"),
        jetos.set("sys.desktop.environment", "kde"),
        jetos.default("sys.locale.timeZone", "America/New_York"),
    ],
}
```

Long-term JetOS can still become more declarative without new declaration
keywords by keeping options as typed fields inside modules:

```jet
module laptop {
    options: [
        set("sys.networking.hostName", "laptop"),
        set("sys.desktop.environment", "kde"),
    ],
    packages: [default.[firefox, fastfetch, btop]],
    services: {
        pipewire: Service { enable: true },
        bluetooth: Service { enable: true },
    },
}
```

Boundary: JetOS owns OS/module semantics. Jetpack owns package resolution,
store realization, profiles, and provider integration. Jet language core should
only grow features that help ordinary programs too.
