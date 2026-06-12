# jetpack — the Jet package manager (start-to-finish guide)

> **STATUS (owner, 2026-06-12): NOT RATIFIED — exploration only.**
> Developed separately from the language roadmap. No decision in this
> file (D-JP1…D-JP5) has been made. It conflicts with
> docs/plans/m12-packages.md (manifest format, pin policy, resolver)
> and must be reconciled with M12 before any package-manager work
> starts. Agents: do not implement anything from this file.

Audience: the project owner (new to package-manager internals) and the agents
building it. Plain language first, spec second. jetos is a SEPARATE project
built on top of jetpack — see 07-jetos.md. Nothing in this file is about
operating systems.

Language status assumed: Jet frontend at milestone M4 (lexer, parser, sema,
codegen working). jetpack reuses that frontend; it does not fork it.

---

## 1. What jetpack is

jetpack is one tool that does two jobs:

1. **For Jet developers** — what Cargo is to Rust: create projects, add
   dependencies, build, run, test, publish. One command, one manifest file,
   zero configuration.
2. **For software in general** — what Nix is to Linux: build ANY software
   (C, Rust, Python, Jet) from recipes, with results that are reproducible,
   cached, shareable, and undoable.

Why one tool can do both: under the hood they are the same problem —
"turn a description of software into runnable files, without surprises."

**The kitchen analogy (used throughout this doc):**

| Thing | Analogy |
|---|---|
| Recipe (a `.jet` file) | A recipe card: ingredients + steps. Reading it cooks nothing. |
| Evaluation | Reading the card and writing a precise shopping-and-cooking PLAN |
| The engine | The kitchen: actually shops, cooks, stores dishes |
| The store | A pantry where every dish is labeled by fingerprint |
| Binary cache | A restaurant that already cooked the exact same dish — buy it instead |
| Lockfile | The receipt: exactly what was bought, so anyone can redo it |
| Generation | A save-slot of your whole setup you can reload anytime |

---

## 2. Vocabulary (read this once, everything else follows)

- **Package** — a folder of files (binaries, libraries, assets) that some
  recipe produced.
- **Recipe** — pure Jet code describing how to make a package: where the
  source comes from (URL + checksum), what it depends on, how to build it.
- **Plan (a.k.a. derivation in Nix)** — the evaluated recipe: a frozen,
  canonical data blob. Same recipe + same inputs ⇒ byte-identical plan.
- **Hash / fingerprint** — a short string computed from data; change one
  byte of the data and the hash changes completely.
- **Store** — a single directory (e.g. `/jetpack/store/`) holding every
  built package in a folder named by its hash. Nothing in the store is ever
  edited in place; folders are only added or garbage-collected.
- **Substitution** — instead of building, download a ready-made result for
  the same hash from a binary cache, verify its signature, done.
- **Sandbox** — a sealed box builds run in: no network, no clock, nothing
  visible except declared inputs. Stops builds from "cheating."
- **Resolver** — the part that picks versions when packages disagree
  ("A wants json 1.x, B wants json 0.9 — what now?").
- **Lockfile** — generated file pinning the exact chosen versions + hashes.
- **Profile / generation** — a numbered snapshot of "what's installed";
  switching or rolling back is instant because it's just re-pointing links.
- **GC (garbage collection)** — deleting store folders nothing points to.

---

## 3. The whole system on one screen

```
 package.jet / recipes (pure Jet)            ← humans write these
        │
        ▼  jet eval --pure        (reuses the M4 compiler frontend)
     PLAN  (canonical JSON, hashable)        ← machines own everything below
        │
        ▼  jetpack engine (separate Rust binary)
   ┌─ resolver ── picks versions, writes jetpack.lock
   ├─ fetcher ─── downloads sources, verifies checksums
   ├─ builder ─── sandbox-runs build steps          ┐ fills the
   ├─ substituter downloads from binary caches      ┘ STORE
   └─ linker ──── wires store folders into your project/profile
```

Two programs, on purpose:

- `jet eval` lives in the compiler repo. It gains a `--pure` mode (JP0).
- `jetpack` is a NEW standalone Rust binary. Compiler invariant **I6 (no
  external crates) does NOT apply to it** — it should use proven crates
  (tokio, reqwest, pubgrub-rs, sha2, zstd). It never links the compiler;
  it shells out to `jet eval --pure --json`.

---

## 4. Core concepts, one at a time

### 4.1 Recipes are pure — and why that's the keystone

Pure = vending machine: same coins + same button ⇒ same snack, every time,
no peeking at the weather. If a recipe could read the network, the clock, or
random files, then "same recipe" would not mean "same result," and every
caching trick below collapses.

So: when jetpack evaluates a `.jet` recipe/manifest, sema allows **only**
pure builtins (math, strings, lists, records) plus *declarations* like
`fetch(url, sha256)` — which records an intent, downloads nothing. All
real-world actions live behind the `env` value that ONLY the engine hands
to a build function, inside the sandbox.

Sample diagnostic (snapshot-pinned, docs/04 voice):

```
error[J-P001]: `read_file` is not allowed in a package file
  package files must produce the same answer on every machine
  fix: declare data with fetch(url:, sha256:), or do file work
       inside build(env) using env.*
```

### 4.2 A recipe, line by line

```jet
// recipes/ripgrep.jet  (PROPOSED syntax — see Decisions D-JP1..3)
package {
    name: "ripgrep",
    version: "14.1.0",
    src: fetch(
        url: "https://github.com/BurntSushi/ripgrep/archive/14.1.0.tar.gz",
        sha256: "k7KqJ3…",        // pinned: wrong download = hard error
    ),
    deps: [pcre2],                 // other recipes, by name
    fn build(env) {                // env = the ONLY door to the world
        env.run("cargo build --release")
        env.bin("target/release/rg")   // declare what to keep
    }
}
```

Evaluating this file performs zero downloads and zero builds. It produces a
plan: `{name, version, src{url,sha256}, deps:[<hash of pcre2's plan>],
build: <the function, serialized>}`.

### 4.3 The store and the family-tree fingerprint

The store path of a package is the hash **of its plan** — and a plan embeds
the hashes of its dependencies' plans, which embed theirs, and so on. So one
fingerprint covers the package's entire ancestry.

```
pcre2 plan ──hash──▶ 9f3a…   → /jetpack/store/9f3a…-pcre2-10.43/
ripgrep plan (contains "deps:[9f3a…]")
            ──hash──▶ b2c4…  → /jetpack/store/b2c4…-ripgrep-14.1.0/
```

Consequences, all free:
- Patch pcre2 → its hash changes → ripgrep's plan text changes → NEW
  ripgrep path. Old and new coexist; nothing is ever overwritten, so
  "upgrade broke an unrelated app" cannot happen.
- The fingerprint is a worldwide cache key: any machine that computes
  `b2c4…` may safely download someone else's build of it.

### 4.4 How `jetpack build` actually proceeds

```
for each plan, in dependency order:
  1. /jetpack/store/<hash>… already exists?      → done, 0 seconds
  2. a configured binary cache has <hash>?        → download, verify
                                                    signature, unpack
  3. otherwise → sandbox build:
       visible: declared inputs only (deps' store paths, fetched src)
       blocked: network, clock, $HOME, everything else
       output → /jetpack/store/<hash>-name-version/
```

The sandbox is not paranoia; it is what makes step 1 and 2 SOUND. A build
that secretly read `/usr/lib` would produce different bytes on different
machines while claiming the same fingerprint.

### 4.5 Installing without copying (the library card)

The store has one real copy of everything. "Installing" into a project or a
user profile means creating hardlinks/symlinks — catalog cards pointing at
the one true book — never photocopying the book.

```
$ jetpack add http json
  resolved 0.2s · fetched 14 KB of metadata
  linked 2 packages from store (0 bytes copied)
  wrote jetpack.lock
```

Ten projects using the same 500 MB toolchain cost ~500 MB, not 5 GB, and
"installs" complete in milliseconds.

### 4.6 The resolver and the lockfile

When dependencies disagree on versions, jetpack uses the PubGrub algorithm
(same family as Cargo and uv): it tries versions, and when it hits a dead
end it records WHY, never repeats that class of mistake, and can therefore
explain failures in plain language:

```
error[J-R012]: version conflict
  weather → http 2.3      needs json ^1
  legacy-soap 1.4         needs json ^0.9
  no json version satisfies both
  fix: `jetpack add legacy-soap@2` (that line uses json ^1)
```

Whatever the resolver decides is frozen into `jetpack.lock` (generated,
never hand-edited): exact versions + plan hashes. A teammate running
`jetpack build` gets your bytes, not "whatever was newest that day."

To pick versions FAST, jetpack never downloads packages during resolution —
only tiny metadata records from the registry index (kilobytes, fetched in
parallel). Downloading happens once, after the choice is made.

### 4.7 Profiles, generations, rollback, GC

A profile is "the set of packages this user wants." Every change creates
generation N+1 (a new set of links — cheap); the old one remains.

```
$ jetpack upgrade
  generation 7 → 8     Δ ripgrep 14.1.0 → 14.1.1
$ jetpack rollback     # instant: re-point one link to generation 7
$ jetpack gc           # delete store folders no generation references
```

Generations are the save-slots; GC roots are "every save-slot still on the
shelf"; everything unreachable is litter.

---

## 5. The user-facing surface (complete)

Files a human touches:

```jet
// package.jet — the project manifest (data-shaped so `jetpack add`
// can edit it mechanically, the way cargo edits Cargo.toml)
package {
    name: "weather",
    version: "0.1.0",
    deps: { http: "^2", json: "^1" },
}
```

`jetpack.lock` — generated receipt. `recipes/*.jet` — only for people
packaging non-Jet software (most users never write one).

Commands (and nothing else — one way to do things):

```
jetpack new <name>        create project
jetpack add/remove <dep>  edit manifest + re-lock + link
jetpack build|run|test    everyday loop
jetpack shell             subshell where deps exist on PATH
jetpack update            re-resolve within manifest ranges, new lock
jetpack publish           push package to the registry
jetpack upgrade|rollback  profile generations
jetpack gc                sweep unreferenced store paths
jetpack cache add <url>   trust a binary cache (key required)
```

Single-file scripts (no project at all):

```jet
#!/usr/bin/env jetpack run
//! deps: { http: "^2" }
fn main() { print(http.get("https://wttr.in?format=3").text) }
```

---

## 6. Milestones JP0–JP6 (each ends with something usable)

| MS | Build | You can now… | Exit criteria (golden/snapshot tests) |
|----|-------|--------------|----------------------------------------|
| JP0 | `jet eval --pure --json` in the compiler: purity check in sema (allowlist), canonical JSON writer (sorted keys, fixed number/string forms), J-P0xx diagnostics | evaluate any pure `.jet` to stable JSON | same file ⇒ byte-identical JSON across runs/machines; J-P001 snapshot |
| JP1 | engine skeleton: plan hashing, store layout, sandbox builder (Linux namespaces), `fetch` with sha256 verify | `jetpack build recipes/hello.jet` → store path; rebuild = instant no-op | tamper with one dep byte ⇒ new hash; build with network blocked succeeds; checksum mismatch = hard error |
| JP2 | linker + profiles + `shell`/`run` | use built packages in projects | 2 projects share store bytes (verify inode count); `shell` exposes deps |
| JP3 | registry index format + resolver (pubgrub-rs) + lockfile + `new/add/remove/update/publish` | full Cargo-style workflow for Jet code | J-R012 conflict snapshot; lock replay reproduces hashes; metadata-only resolution (measure: no package downloads during resolve) |
| JP4 | binary cache: HTTP/S3 layout, zstd, ed25519 signing, substitution step | fast installs; team/CI share builds | cold vs warm timing test; bad signature = refusal snapshot |
| JP5 | generations, rollback, GC with roots | safe upgrades | upgrade→rollback round-trip; gc never deletes a rooted path (property test) |
| JP6 | polish: inline script deps, `--as-of <date>` (registry snapshots), optional Nix-cache substituter (read NAR/narinfo from cache.nixos.org) | scripts run themselves; time-machine; tap nixpkgs binaries | script demo; `--as-of` reproduces an old lock |

Order matters: JP0 unblocks everything; JP1–2 need no registry; JP3+ are
where network services appear. jetos (other doc) needs JP0–JP5.

---

## 7. Agent guardrails

- The engine is a separate crate/binary; never import compiler internals —
  the boundary is `jet eval --pure --json`.
- Determinism is tested, not assumed: shuffle inputs / parallelism in tests;
  hashes and JSON must be byte-identical.
- Every diagnostic: code (J-P pure, J-R resolver, J-B build, J-S store) +
  what/why/fix + snapshot, per compiler invariant I4.
- No second mechanism for anything: one manifest, one lockfile, one store
  layout, one cache protocol.
- Error text is product copy. Write it like docs/04 or don't ship it.

---

## 8. Decisions for the owner (jetpack only — answer "all A" or per-ID)

| ID | Question | A | B | Rec |
|----|----------|---|---|-----|
| D-JP1 | Where recipes live | `recipes/` directory; pure mode applies to `package.jet` + everything under it | filename suffix `*.pkg.jet` anywhere | A — one obvious place, no naming rule to teach |
| D-JP2 | Manifest block keyword | `package { … }` | `pkg { … }` | A — beginner-first, no abbreviation |
| D-JP3 | Dep spec shape | map: `deps: { http: "^2" }` | list: `deps: ["http@^2"]` | A — machine-editable, mirrors lockfile |
| D-JP4 | Lockfile | `jetpack.lock`, canonical JSON | TOML-style, human-tweakable | A — locks are generated artifacts; humans edit the manifest |
| D-JP5 | Store location | `/jetpack/store` system-wide, `~/.jetpack/store` rootless fallback | XDG paths only | A — short stable paths matter (they appear inside binaries) |