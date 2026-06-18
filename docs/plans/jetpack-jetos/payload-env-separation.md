# Payload.jet vs. Env.jet: Design Rationale for Separation

**Status:** Design codification (ratifies existing practice from U1–U10)

## Core Principle

`payload.jet` and `env.jet` are **different artifacts with different audiences and lifecycles**. Merging them would violate the separation of concerns and create coupling between package definition and local development.

---

## What Each File Owns

### `payload.jet` — Package Definition (Immutable, Publishable)

**What it is:** The canonical declaration of what packages this repository ships.

**Contents:**
- `payload:` block with `name`, `version`, and `packages: { name: kind, … }`
- Kind is either `executable` or `library`
- Future: `[profile]`, `[features]` (post-v1)

**Lifecycle:**
- Checked into git; versioned with releases
- Consumed by the `core` provider when someone does `jetpack run <repo>`
- Part of the published artifact (in phase 2, it's the registry entry)
- Never changes between commits unless the actual packages change

**Audience:**
- End users who want to *use* a package from this repo
- Registry resolvers (phase 2) that index what's available
- Build systems that consume the packages

**Why immutable:** If `payload.jet` changes between commits, version pinning breaks. A user with lock entry `repo#abc123` must always get the same packages at that commit.

---

### `env.jet` — Development Environment (Mutable, Local)

**What it is:** The local developer's shell configuration for working on this project.

**Contents:**
- `use jetpack as pkg;` + function that returns package sources and references
- Named sources (e.g., `stable:`, `unstable:`, `mine:`)
- Packages to install (e.g., `["stable:ripgrep", "unstable:neovim", "mine:hello"]`)
- Typed `module {}` surface (D-JPK3) for declaring typed dev dependencies

**Lifecycle:**
- Local to the project; can differ per developer/branch
- Not part of the published package
- Changes frequently as developer workflow needs shift
- Can reference `payload.jet` packages, but doesn't mandate them

**Audience:**
- Local developers: `jetpack enter` or `jet dev` reads this to set up their shell
- IDE/tools: LSP, debugger, formatter discovery
- CI: implicit environment when running tests

**Why mutable:** A developer might pin unstable nixpkgs for debugging, add a temporary tool, or fork a provider without changing the package definition. The environment is *local state*, not *package definition*.

---

## Why Not Merge Them?

### 1. Concern Separation (Architectural)
Merging forces one file to answer two questions:
- **Package question:** "What does this repo publish?"
- **Environment question:** "What tools does a developer need?"

These are answered by different people, at different times, with different constraints:
- A package maintainer cares about semver and reproducibility.
- A developer cares about convenience and local iteration.

Conflating them makes both harder.

### 2. Publishing Complexity
If `env.jet` is merged into `payload.jet`:
- Publishing a package requires editing the dev section (noise in the diff)
- Users installing a package get the dev dependencies in the manifest (ceremony)
- The package manifest becomes bloated and specific to one team's workflow

Example (merged, bad):
```jet
payload: {
    name: "mylib",
    version: "0.2.0",
}
packages: {
    core: library,
}
dev_sources: {                          // ← Pollution
    unstable: "github:NixOS/nixpkgs/unstable",
}
dev_packages: ["unstable:cargo-nextest"],  // ← Not part of the package
```

### 3. Versioning Tight Coupling
If the package definition and dev environment are the same file:
- A developer makes a local change to `env.jet` (e.g., adds a tool)
- They accidentally commit it or merge it with a package version bump
- The next user sees a different dev environment when they checkout that tag

Keeping them separate means the version tag locks only the package definition.

### 4. Provider Integration
The `core` and `nix` providers (phase 1) read `payload.jet` to discover what's available. They never read `env.jet`. If they're merged:
- Providers need to parse dev-specific sections and ignore them
- Future providers must know about all dev-tool conventions
- The interface becomes unstable

### 5. Monorepo Clarity
In a monorepo with multiple packages, each member has its own `payload.jet`:

```
repo/
  lib-a/
    payload.jet (name: "lib-a", packages: { core: library })
  lib-b/
    payload.jet (name: "lib-b", packages: { core: library })
  root/
    payload.jet (name: "monorepo-core", packages: { a: library, b: library })
  env.jet (dev: [my tools])
```

The monorepo has *one* `env.jet` at the root because developers have *one* dev environment. Multiple `payload.jet` files (one per publishable unit) because packages are independent. Merging them would require one giant file or create ambiguity about which `payload` each `env.jet` refers to.

---

## Trade-off: Complexity vs. Purity

| Aspect | Merged | Separate |
|--------|--------|----------|
| **Files to manage** | 1 | 2 (but in different dirs) |
| **Conceptual clarity** | Muddied | Clear |
| **Publisher ceremony** | High (edit env section) | Low (edit `payload.jet` only) |
| **Version stability** | Risky (dev changes affect tag) | Safe (tag locks package only) |
| **Provider interface** | Coupled | Decoupled |
| **Monorepo structure** | Ambiguous | Clear |

**Verdict:** The extra file is worth it. Two simple files beat one complex file.

---

## Decision (Ratified)

- **`payload.jet`** is the package definition: immutable per version, publishable, provider-friendly.
- **`env.jet`** is the dev shell: mutable, local, decoupled from versioning.
- Never merge them.
- A repo may have multiple `payload.jet` files (monorepo members); it has one `env.jet` at the project root.

