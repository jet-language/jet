# Package manager — unified vision & decision file

> **STATUS (2026-06-13): MANIFEST RATIFIED; architecture recommendations
> active.** Manifest layout is ratified in docs/manifest-design-research.md
> (S52 amended). Implementation plan: docs/plans/m12-packages.md. D-PM1…
> D-PM8 below are **recommendations** — treat as ratified defaults for
> implementation unless the owner overrides before M12.1 starts.
> docs/jetpack.md is historical (layer 3 only).

**How to read this file:** plain language first, spec second. §1 defines
every term once. Each decision in §5 shows what you would actually type
and see under each option, then the strengths and weaknesses, then a
recommendation. You don't need to know how any package manager works
internally to decide — that's what the examples are for.

---

## 1. The words, in plain language (read once)

- **Package** — a folder of code someone else wrote that your project
  wants to use.
- **Dependency** — a package your project needs. Dependencies have
  their own dependencies; the whole family is the **dependency tree**.
- **Manifest** (`jet.toml`) — the file where you write down what your
  project needs: "I want textkit, version 1.2." You (or `jet add` on
  your behalf) edit this file. It's the only package file a human
  touches.
- **Lockfile** (`jet.lock`) — the receipt the tool writes after it
  figures everything out: exactly which version of every package in the
  tree was used, plus a fingerprint of each. You never edit it; you
  commit it to git so a teammate's build uses *exactly* your versions —
  not "whatever was newest that day."
- **Fingerprint (hash)** — a short string computed from a pile of
  bytes. Change one byte anywhere and the fingerprint changes
  completely. If two folders have the same fingerprint, they are the
  same bytes. This is the load-bearing trick of the whole design.
- **Store** — one folder on your machine
  (`~/.jet/store/`) that holds every package version ever downloaded,
  each in a subfolder named by its fingerprint. Nothing inside is ever
  edited — subfolders are only added, or deleted when nothing uses them
  anymore. Because folders are named by fingerprint, two different
  versions of the same package simply sit side by side and can never
  collide or overwrite each other.
- **Hardlink** — a filesystem trick that makes one file appear in many
  places while existing on disk once. Think: library catalog cards all
  pointing at the one real book. "Installing" a package into a project
  means writing catalog cards, never photocopying the book.
- **Semver (semantic versioning)** — the `2.3.1` convention:
  major.minor.patch. Bumping *major* (3.0.0) is allowed to break you;
  minor/patch are not. A **version range** like `^2` means "any 2.x —
  I trust them not to break me before 3.0."
- **Resolver** — the part that picks concrete versions when ranges are
  involved and packages disagree ("A wants json ^1, B wants json ^0.9 —
  now what?"). **PubGrub** is the best-known resolving algorithm (used
  by Cargo's cousins and uv); its claim to fame is *explaining* dead
  ends in plain language instead of dumping them on you.
- **Registry** — the public phone book of published packages: name →
  available versions → where to download → fingerprint.
- **Generations** — numbered save-slots of "what's installed," like
  game saves. Rolling back = loading an old save, instantly.
- **Binary cache** — a server that holds already-built results, so your
  machine can download a finished build instead of building it.
- **Sandbox** — a sealed box a build runs in: no network, no access to
  the rest of your machine, only its declared ingredients. It's what
  makes "same fingerprint" trustworthy *across* machines.

## 2. Vision (one paragraph)

**Nix's engine under Cargo's steering wheel.** Nix is the best system
ever built for *storing and versioning* software: every version of
everything coexists peacefully, upgrades can't break unrelated things,
and you can roll back anything — but it's famously hard to learn. Cargo
(Rust's tool) has the friendliest *daily workflow* ever shipped: one
manifest, one lockfile, `add`/`build`/`run`, done — but its storage
model wastes disk and re-builds the world per project. Jet takes both
halves: the commands you type and the files you touch are Cargo-simple
(`jet add`, `jet.toml`, `jet.lock`), while the machinery underneath
stores packages the Nix way (fingerprint-named, immutable, every version
side by side). And the place Nix hurts most — incomprehensible errors —
is exactly Jet's superpower: every package error gets the docs/04
what/why/fix treatment. One sacred rule on top: single-file
`jet run file.jet` never needs any of this (R9).

## 3. What we take from each existing tool (the survey)

| Tool | What it does best (we take it) | What we refuse |
|---|---|---|
| **Nix** (primary inspiration) | The store: packages live in fingerprint-named, never-edited folders, so every version coexists and an upgrade can never break an unrelated project. Rollback as a first-class idea. Downloading pre-built results by fingerprint. Pinning the whole world to a date and reproducing it later | The Nix *language* (Jet itself is the replacement — S60 `pure fn`); the learning cliff; cryptic errors; a root-owned `/nix/store` + daemon that makes installation painful |
| **Cargo** (Rust) | The daily workflow: one manifest, one lockfile, `add`/`build`/`run`/`test`/`publish` built into the compiler's own CLI; builds obey the lockfile; `--locked` for CI | `build.rs` — dependencies running arbitrary code on your machine at build time (our supply-chain rule forbids this); each project keeping its own bloated build folder |
| **pnpm** (JS) | Hardlinking from one global store into projects: ten projects share one copy of everything; "installing" takes milliseconds and ~0 bytes | the node_modules layout it has to emulate |
| **uv** (Python) | Speed as a feature: pick versions from tiny metadata files *before* downloading any package; fetch in parallel; PubGrub's readable conflict explanations | — |
| **Go modules** | Paranoia done right: record every dependency's fingerprint and re-verify it on every build (tamper detection — M12 already adopted this as E1204); easy vendoring (copy deps into the repo for air-gapped builds) | its unusual version-picking rule (MVS) that surprises newcomers; `/v2` import-path versioning |
| **Elm** | The registry *enforces* semver: it diffs your package's public API against the last release and refuses a "minor" bump that actually breaks people. Jet's sema already knows every `pub` signature, so this flagship feature is nearly free | — |
| **Deno** (JS) | Capability honesty: tell the user what a dependency tree can touch (files? network? processes?) based on what it imports | URL imports |
| **npm** (anti-lessons) | — | install scripts (a dependency running code on your machine the moment you download it — the #1 supply-chain attack vector; already ratified out of M12); silently bundling duplicate versions; a registry where published code could vanish (left-pad). Ours: registry entries are immutable; yanking flags a version, never deletes it |
| **apt / Homebrew** (anti-lessons) | — | one global folder where installing X can overwrite a file Y needed. The store model makes this *entire category* of breakage impossible |

## 4. The shape of the system (three layers, one store)

```
LAYER 1 — what you touch daily (Cargo-shaped; ships first, M12 phase 1)
  jet.toml · jet.lock · jet add/remove/fetch/build/run/test
        │
LAYER 2 — versions & publishing (uv/Elm-shaped; M12 phase 2)
  registry · version ranges + resolver · "--as-of <date>" time travel
  · semver-enforcing publish · jet audit / SBOM for enterprises
        │
LAYER 3 — the engine room (Nix-shaped; underneath from day one)
  the store: ~/.jet/store/<fingerprint>-name-version/
  immutable · hardlinked into projects · cleaned by `jet gc`
```

The key call (this is decision D-PM1): **layer 3 is not a future phase —
it is how phase 1 stores things from the very first release.** What
changes per phase is only *what kinds of things* go into the store:

- **Phase 1:** verified source code of git/path dependencies. Each
  project still compiles its deps (simple, matches current M12), but
  the source lives in the store once, hardlinked everywhere.
- **Phase 2:** registry packages, plus *compiled* results keyed by
  fingerprint — so a given version of a dependency compiles **once per
  machine, ever**, instead of once per project (Nix's idea pointed at
  Cargo's biggest pain).
- **Phase 3 (post-v1):** the full Nix tier — sandboxed builds of
  recipes written in pure Jet (`jet eval --pure`, S60), downloading
  signed pre-built results from team/public caches, save-slot
  generations for installed tools, packaging non-Jet software. This is
  everything docs/jetpack.md dreamed, re-homed onto the *same* store
  and lockfile instead of a parallel system. jetos waits on this layer.

Why the fingerprint covers more than you'd think: a package's
fingerprint is computed from its *plan* — name, version, source
fingerprint, **and the fingerprints of its dependencies' plans**. So one
short string covers the package's entire family tree. Patch a deep
dependency and every package above it automatically gets a new identity;
old and new coexist in the store; nothing is ever mutated in place.

## 5. The decisions (D-PM1…D-PM8), each with worked examples

At-a-glance:

| ID | Question | Recommendation |
|----|----------|----------------|
| D-PM1 | How Nix-like is the core? | A — Nix store underneath, Cargo surface, from day one |
| D-PM2 | What is the manifest written in? | A — keep `jet.toml` (S52); Jet-code recipes only at layer 3 |
| D-PM3 | How are versions picked? | A — exact pins first, ranges+resolver at phase 2 |
| D-PM4 | One binary or two? | A — everything in `jet`; no-crates rule (I6) holds |
| D-PM5 | Where does the store live? | A — `~/.jet/store`, no root, no daemon |
| D-PM6 | What is the registry, physically? | A — an append-only git repo |
| D-PM7 | When do save-slot generations arrive? | A — layer 3, for installed tools |
| D-PM8 | When can machines share built results? | A — layer 3; phase 2 cache is local-only |

---

### D-PM1 — How Nix-like is the core architecture?

*The question in plain words: when `jet add` downloads a package, where
do the bytes go, and what are they named? This one choice decides
whether Nix's superpowers (versions coexisting, upgrades never breaking
neighbors, future build-sharing) come for free later or have to be
retrofitted.*

**Option A — Nix store underneath, Cargo surface on top (Rec).**
Downloads land in one global store, in folders named by fingerprint.
Projects get hardlinks, never copies.

What you'd see — two projects using the same dependency:

```
$ cd ~/code/weather
$ jet add textkit --git https://github.com/someone/textkit --tag v1.2.0
  fetched textkit v1.2.0 (84 KB)
  store + a7f3d2…-textkit-1.2.0          ← first time on this machine
  linked into weather (0 bytes copied)
  wrote jet.lock

$ cd ~/code/blog
$ jet add textkit --git https://github.com/someone/textkit --tag v1.2.0
  linked textkit v1.2.0 from store       ← no download, no copy, instant
  wrote jet.lock
```

Upgrading one project cannot touch the other, because nothing is ever
overwritten — the new version just gets its own fingerprint-named folder:

```
$ cd ~/code/blog
$ jet add textkit --tag v2.0.1
  store + c91d08…-textkit-2.0.1          ← v1.2.0 still there, untouched
$ cd ~/code/weather && jet test
  all tests pass                          ← weather still on v1.2.0
```

What's on disk:

```
~/.jet/store/
  a7f3d2…-textkit-1.2.0/      ← both versions, side by side, forever
  c91d08…-textkit-2.0.1/         (until `jet gc` finds them unused)
  e22b41…-helpers-0.4.2/
```

- **Strengths:** "upgrading here broke something there" is structurally
  impossible — the bug *category* is gone, not patched. Ten projects
  share one copy of everything (installs in milliseconds, near-zero
  disk). And the fingerprint is exactly the key that phase 2's
  compile-once cache and layer 3's build-sharing need — so the Nix
  future plugs in with no migration and no second naming scheme.
- **Weaknesses:** phase 1 has more to build: fingerprint computing, the
  hardlink linker, `jet gc`, a `jet store verify` command, and file
  locking so two `jet` processes don't fight. Hardlinks also require
  the store and project to be on the same disk partition (fallback:
  copy — still correct, just loses the sharing).

**Option B — Full Nix now (build docs/jetpack.md as written).** Before
(or instead of) the Cargo-style workflow, build the whole Nix machine:
recipes written in Jet, sandboxed builds, a tool that can package *any*
software (C, Rust, Python…), not just Jet libraries.

What you'd see — packaging arbitrary software is the first deliverable:

```jet
// recipes/ripgrep.jet — a recipe: ingredients + steps, in pure Jet
package {
    name: "ripgrep",
    version: "14.1.0",
    src: fetch(url: "https://…/14.1.0.tar.gz", sha256: "k7KqJ3…"),
    deps: [pcre2],
    fn build(env) {
        env.run("cargo build --release")
        env.bin("target/release/rg")
    }
}
```

```
$ jetpack build recipes/ripgrep.jet
  sandbox: network blocked, only declared inputs visible
  → /jetpack/store/b2c4…-ripgrep-14.1.0/
```

- **Strengths:** the most powerful endpoint — build anything,
  reproducibly; sharing built results between machines is sound from
  day one (the sandbox guarantees a fingerprint means the same bytes
  everywhere); jetos gets its foundation directly.
- **Weaknesses:** Jet developers wait a very long time for `jet add` —
  purity checking, the sandbox, and the store all come first. The
  sandbox is Linux-first tech (the macOS story is much weaker). It
  requires un-ratifying S52 (`jet.toml`). And it means building two
  ambitious products at once — a language *and* a Nix replacement —
  before either is proven.

**Option C — Cargo-style cache now, store later (current M12 plan as
written).** Downloads go to a simple cache folder keyed by package name
and version; no fingerprint naming, no store.

What you'd see — almost the same, which is exactly the trap:

```
$ jet add textkit --git https://… --tag v1.2.0
  fetched textkit v1.2.0 → ~/.cache/jet/pkg/textkit/9f3a…/
  wrote jet.lock
```

The invisible difference: the cache's name for a package doesn't include
what *its* dependencies were. So "textkit 1.2 built against helpers
0.4" and "textkit 1.2 built against helpers 0.5" look like the same
thing — meaning compiled results can never be safely shared from this
layout. When the store is added later, the lockfile format changes and
every existing user re-downloads everything: a breaking migration.

- **Strengths:** the smallest possible first release — it's the plan
  already written; no garbage collection, linker, or locking code.
- **Weaknesses:** bakes the wrong naming scheme into the first release.
  Phase 2 and layer 3 need fingerprint-naming anyway, so the work is
  deferred, not avoided — and a user-facing migration is added on top.

---

### D-PM2 — What language is the manifest written in?

*The question in plain words: the one file users edit to declare
dependencies — is it simple data (like a settings file) or actual Jet
code? S52 already ratified data (`jet.toml`); the jetpack exploration
proposed code. This decides what beginners see in their first minute.*

**Option A — keep `jet.toml`: plain data (Rec).** Jet-code recipe files
exist only at layer 3, for packaging non-Jet software where real logic
is genuinely needed.

What you'd see — `jet add` edits the file for you, mechanically:

```toml
# jet.toml, before
[package]
name = "weather"
version = "0.1.0"

[dependencies]
helpers = { path = "../helpers" }
```

```
$ jet add textkit --git https://github.com/someone/textkit --tag v1.2.0
```

```toml
# jet.toml, after — one line appended; your comments and order untouched
[dependencies]
helpers = { path = "../helpers" }
textkit = { git = "https://github.com/someone/textkit", tag = "v1.2.0" }
```

- **Strengths:** a beginner reads it in ten seconds with zero Jet
  knowledge. Tools can edit it safely (append a line) and read it
  cheaply (CI, registries, editors — no evaluator needed). S52 stays
  frozen — no re-ratification churn.
- **Weaknesses:** it can't express logic (conditional dependencies,
  computed versions) — which is deliberate. And at layer 3 a second
  file kind (recipes, in Jet) will exist beside it. That split is
  principled, though: *data for projects, code for build recipes* —
  the same split Cargo uses, minus the dangerous part (build scripts).

**Option B — manifests are Jet code (`package.jet`).**

What you'd see:

```jet
// package.jet — evaluated by the compiler in pure mode
package {
    name: "weather",
    version: "0.1.0",
    deps: {
        helpers: path("../helpers"),
        textkit: git("https://github.com/someone/textkit", tag: "v1.2.0"),
    },
}
```

Looks fine — until someone uses the power code grants, which breaks the
tooling around it:

```jet
package {
    name: "weather",
    deps: base_deps + extra_deps,   // ← computed. Where should
}                                    //   `jet add` insert a new dep?
```

Now `jet add` must parse, evaluate, and *rewrite your source code*
without mangling comments or formatting — or refuse manifests that
aren't simple literals, which reinvents "plain data" with extra steps.

- **Strengths:** one language for everything; exercises S60 purity;
  manifests and layer-3 recipes become a single file kind.
- **Weaknesses:** machine-editing code is fragile (above); even
  *reading* the dependency list requires running the evaluator;
  beginners meet code before their first dependency; requires
  un-ratifying S52 and updating the enforcement tests.

**Option C — TOML now, switch to Jet manifests at layer 3.**

What you'd see, years in: half the tutorials and repos show `jet.toml`,
half show `package.jet`, and every tool supports both forever.

- **Strengths:** defers the question.
- **Weaknesses:** two manifest formats in the wild — a standing
  violation of "one way to do things" (PM-I6) that every user pays for.

---

### D-PM3 — How are versions picked?

*The question in plain words: when you say you want textkit, do you name
an exact version ("v1.2.0, that one"), or a range ("any 1.x") that the
tool resolves? And may two versions of the same package ever appear in
one program?*

**Option A — exact pins in phase 1; ranges + resolver in phase 2; always
one version per package name (Rec).**

What you'd see in phase 1 — everything is pinned, so there is nothing to
"resolve" and no resolver code exists at all:

```toml
[dependencies]
textkit = { git = "https://…/textkit", tag = "v1.2.0" }   # exact
```

```
$ jet update textkit          # phase-1 "update" = move the pin forward
  textkit v1.2.0 → v1.3.0 (latest tag); jet.toml and jet.lock rewritten
```

If two of your dependencies demand different versions of the same thing,
that's a clear error showing both chains of blame (E1201):

```
error[E1201]: two versions of `textkit` are required
  weather → markdown v1.1 → textkit v1.2.0
  weather → slides v0.3   → textkit v2.0.1
  one project can only use one version of a package
  fix: update `markdown` — its v1.2 release uses textkit v2
```

What you'd see in phase 2 — the registry arrives, ranges arrive with it,
and the PubGrub resolver explains dead ends instead of dumping them:

```toml
[dependencies]
http = "^2"        # any 2.x — the resolver picks; jet.lock freezes it
```

```
error[E1207]: no version of `json` satisfies this project
  http 2.3 (needed by weather)        wants json ^1
  legacy-soap 1.4 (needed by weather) wants json ^0.9
  fix: `jet add legacy-soap@^2` — that release moved to json ^1
```

- **Strengths:** phase 1 ships with zero resolver code and is
  reproducible by construction. (Ranges barely make sense in phase 1
  anyway: against a git URL, "any 1.x" has no cheap answer — there's no
  index to consult.) Ranges arrive exactly when the registry exists to
  make them fast and meaningful. One-version-per-name keeps the
  beginner's mental model simple — "this project uses *the* json" —
  and keeps binaries small.
- **Weaknesses:** phase-1 upgrades are manual pin-moves; library
  authors can't declare compatibility ranges until phase 2; very large
  dependency trees can hit E1201 walls that ranges would have dodged —
  accepted until evidence says otherwise (and the store already
  supports relaxing the rule).

**Option B — ranges + resolver from day one.** The phase-2 experience
above, just immediately.

- **Strengths:** instantly familiar to Cargo/npm users; libraries can
  declare compatibility from their first release.
- **Weaknesses:** the resolver becomes the critical path of the *first*
  release, and it would be resolving against git tags — every "any
  2.x?" question is a slow network call to list a repo's tags, because
  the registry metadata that makes resolving fast doesn't exist yet.

**Option C — allow two major versions of one package to coexist (what
Cargo does).** The store makes this mechanically nearly free.

What you'd see — the conflict above just… dissolves:

```
weather → http 2.3        → json 1.2.0   ┐ both in the store,
weather → legacy-soap 1.4 → json 0.9.6   ┘ both inside your binary
```

…until the two worlds touch, producing the classic confusing error:

```
error: type mismatch
  expected json.Value   (from json 1.2.0, via http)
  found    json.Value   (from json 0.9.6, via legacy-soap)
  these are different types: two versions of `json` are in this program
```

- **Strengths:** version conflicts essentially vanish; huge ecosystems
  scale without coordination between library authors.
- **Weaknesses:** that error is the most beginner-hostile message in
  package management ("expected json.Value, found json.Value" —
  *what?*); duplicate code bloats binaries; two copies of a package
  means two copies of its internal state, causing bugs no error message
  can catch. Wrong default for Jet; revisit only with evidence.

---

### D-PM4 — One binary or two? (and does I6, the no-external-crates
rule, hold?)

*The question in plain words: is the package manager part of the `jet`
command, or a second program? Tied to it: invariant I6 says the compiler
uses no third-party Rust libraries — networking and hashing are exactly
where that rule starts to pinch.*

**Option A — everything inside `jet`; I6 holds (Rec).** Instead of
linking networking libraries, `jet` runs the `git` command the user
already has, and carries a small (~100-line) hashing routine in-tree.

What you'd see:

```
$ jet fetch
  textkit v1.2.0: downloading (via git)…
  fingerprint verified against jet.lock ✓
```

```
error[E1203]: `git` is not installed
  jet uses git to download dependencies
  fix: install git (https://git-scm.com), then re-run `jet fetch`
```

A tidy bonus if D-PM6 also picks A: the registry *is* a git repository —
so `git` is the **only** network tool the package manager ever needs,
through all of phase 1 and 2. No HTTP code in the compiler at all.

- **Strengths:** one tool to install, teach, and version; no carve-out
  of an invariant (carve-outs have been declined before — see the
  C-FFI/I1 history); the resolver is just an algorithm, ~1–2k lines of
  plain Rust we control and test ourselves.
- **Weaknesses:** we maintain that resolver ourselves rather than using
  the community's; parallel downloads use plain threads (fine at this
  scale); and *if* layer 3 arrives, sandboxing/compression/signing in
  pure std Rust is a real cost — deferred, not denied.

**Option B — a separate `jetpack` program, exempt from I6 (the
jetpack.md proposal).** Free to use the Rust ecosystem's networking,
compression, and crypto libraries.

What you'd see — two tools, and the failure mode two tools always have:

```
$ jet --version        → jet 1.2.0
$ jetpack --version    → jetpack 1.1.3
$ jetpack build
error: jetpack 1.1.3 cannot read a jet.lock written by jet 1.2.0
```

- **Strengths:** battle-tested libraries make the hard parts cheap;
  the compiler itself stays pure-std.
- **Weaknesses:** an explicit invariant carve-out; a second install
  step in every getting-started guide; version skew between the two
  tools is a brand-new failure class; and the boundary between them
  becomes an interface to design, version, and keep stable forever.

**Option C — inside `jet` for now; split a *hidden* engine binary at
layer 3 only if truly needed.** Identical to A through all of M12. If
layer 3's sandbox work genuinely demands heavy libraries, an internal
helper binary appears that users never type — the way rustc is present
but invisible today.

- **Strengths:** all of A's simplicity now, plus a named escape route
  that doesn't pre-decide the I6 debate.
- **Weaknesses:** escape routes have a way of becoming assumptions —
  though the debate would land exactly when layer 3 gets planned
  anyway, which is also the argument *for* C.

---

### D-PM5 — Where does the store live?

*The question in plain words: one folder will hold every package on the
machine. Is it inside your home folder, or a system-wide location like
Nix's `/nix/store`?*

**Option A — `~/.jet/store`: per-user, no root, no daemon (Rec).**

```
~/.jet/store/a7f3d2…-textkit-1.2.0/src/words.jet
```

- **Strengths:** `jet` works the moment it's on your PATH — no
  installer, no sudo, no background service. Works in CI containers and
  on locked-down machines. (Nix's single hardest adoption problem is
  exactly its root-owned store + daemon; we skip it.)
- **Weaknesses:** on a shared machine each user has their own store
  (duplicate disk — rare for our audience). Nix's short stable system
  path matters when store paths get embedded inside built binaries —
  but in phases 1–2 Jet dependencies are compiled from source and no
  store path ever enters a binary, so the benefit doesn't apply yet.
  Revisit at layer 3.

**Option B — `/jet/store`, system-wide (Nix's choice).**

- **Strengths:** one store per machine, shared by all users; short
  stable paths if layer 3 ever embeds them in artifacts.
- **Weaknesses:** creating it needs root or a privileged background
  service; the #1 source of Nix install friction, imported on day one
  for zero phase-1 benefit.

---

### D-PM6 — What is the registry, physically? (phase 2)

*The question in plain words: when `jet add http` looks up what versions
of `http` exist, what is it consulting — a git repository of small text
files, or a web service we run?*

**Option A — an append-only git repository (Rec).** The whole registry
is a repo of one-line-per-version entries:

```
registry/t/textkit:
{"name":"textkit","version":"1.2.0","source":"https://github.com/someone/textkit","tree":"sha256:9f3a…"}
{"name":"textkit","version":"2.0.1","source":"https://github.com/someone/textkit","tree":"sha256:b2c4…"}
```

What you'd see:

```
$ jet add textkit              # consults a local clone of the index
$ jet fetch --as-of 2026-03-01
  index @ a91c3f (2026-03-01) — resolving against March's world
```

That `--as-of` time-travel costs nothing to build: an append-only git
repo's history *is* a complete archive of every past state — "the
registry as of March 1st" is just an older commit. A company's private
registry, or an offline mirror, is just another git remote.

- **Strengths:** no server to build, run, secure, or fund; audit
  history, mirroring, air-gapped use, and time-travel all inherited
  from git for free; publishing-by-pull-request gives human review
  while the ecosystem is tiny (honest and cheap).
- **Weaknesses:** no realtime search API (a generated website covers
  browsing); the index clone grows with the ecosystem — git's sparse
  checkout delays that problem for years, and Cargo's own later
  migration (git index → simple HTTP files) is a proven escape path if
  we ever outgrow it.

**Option B — a hosted web service from the start.**

- **Strengths:** instant search, instant publish, per-user access
  tokens.
- **Weaknesses:** a production service to build, operate, secure, and
  pay for *before there are users to justify it*; a single point of
  failure for every build in the ecosystem; snapshots and mirrors
  become features to engineer instead of properties inherited for free.

---

### D-PM7 — When do save-slot generations arrive?

*The question in plain words: Nix lets you roll back "what's installed"
like loading a game save. Do projects need that, or only globally
installed tools — and when?*

**Option A — layer 3, for installed tools only; projects already have
it via git (Rec).**

What you'd see — the split that matches where the value really is:

```
# A PROJECT's history is its lockfile's git history — works today:
$ git log --oneline jet.lock
$ git checkout HEAD~3 -- jet.lock && jet fetch    # project "rollback"

# A TOOL you install globally gets real save-slots — at layer 3:
$ jet install ripgrep         # generation 4 → 5
$ jet rollback                # instant: back to generation 4
```

- **Strengths:** projects need nothing built — git already version-
  controls the lockfile better than any save-slot system could;
  generations arrive exactly when `jet install <tool>` exists to need
  them; M12 stays small.
- **Weaknesses:** no rollback for globally installed tools until
  layer 3 — acceptable because v1 has no global tool installs at all.

**Option B — build generations into M12.**

- **Strengths:** the full Nix save-slot experience ships sooner.
- **Weaknesses:** M12 grows profiles, generation storage, rollback
  bookkeeping, and a `jet install` command that nothing in v1
  otherwise needs — real scope, no v1 consumer.

---

### D-PM8 — When can machines share built results?

*The question in plain words: compiling dependencies takes time. When
may your machine reuse work — first its own past work, then builds done
by teammates' machines or a public cache?*

**Option A — your own machine reuses everything from phase 2;
*cross-machine* sharing waits for layer 3's sandbox (Rec).**

What you'd see in phase 2 (local reuse — the big everyday win):

```
$ jet build
  3 dependencies already compiled for these exact versions — reused
  compiled: weather (your code only)
```

What you'd see at layer 3 (cross-machine, same fingerprints, now signed):

```
$ jet fetch
  12 deps: 12 downloaded pre-built from cache.jet-lang.org (3.1 MB)
  signatures verified ✓ · 0 compiled locally
```

- **Strengths:** the dangerous step is deferred for a principled
  reason: trusting another machine's build is only sound if the
  fingerprint *provably* determines the bytes — which is what layer 3's
  sandbox guarantees. Until then, "same fingerprint" on two unsandboxed
  machines might not mean "same bytes," and sharing would quietly
  poison the well. Signing and key distribution get designed once,
  deliberately.
- **Weaknesses:** CI fleets recompile shared dependencies per machine
  until layer 3 (mitigated by caching the store directory between CI
  runs — standard practice).

**Option B — networked sharing in M12 phase 2.**

- **Strengths:** team/CI build-sharing arrives a phase earlier.
- **Weaknesses:** exactly the unsoundness above, plus signing
  infrastructure and trust policy land on v1's critical path.

## 6. Phasing (replaces both M12's two phases and jetpack's JP0–JP6)

| Phase | Milestone | Ships | Exit criteria (tests) |
|-------|-----------|-------|----------------------|
| 1 | **M12.1** | `jet.toml` + path/git deps, exact pins; `jet add/fetch`; `jet.lock` with plan fingerprints; **the store + hardlink linker**; E1201–E1206; `--locked` | current m12-packages.md battery **plus**: two fixture projects sharing one dep have identical store inodes; tampered store path detected (E1204); lock replay reproduces identical fingerprints |
| 2 | **M12.2** | git-index registry; semver ranges + PubGrub resolver; `jet publish` with **enforced semver API diff**; `--as-of`; compile-once-per-machine artifact cache (local); `jet audit` + SBOM emission (enterprise.md); `jet vendor` for air-gapped builds | resolver-conflict snapshot (E1207, docs/04 voice); publish-refusal snapshot on a breaking "minor" bump; `--as-of` reproduces an old lock byte-for-byte; air-gapped build from vendor dir |
| 3 | **post-v1** (needs its own plan + ballots; subsumes jetpack JP0–JP6 and unblocks jetos) | `jet eval --pure` (S60) over recipe files; sandboxed builds; signed cross-machine caches; generations/rollback/GC for installed tools; packaging non-Jet software | jetpack.md §6 JP-row criteria, re-homed onto this store/lockfile |

## 7. Invariants (extend I1–I8)

- **PM-I1** No code execution at dependency install/fetch time, ever.
  No install hooks, no build scripts in phase 1–2. (Already ratified in
  M12; npm's hardest-learned lesson.)
- **PM-I2** The store is append-only: folders are added or
  garbage-collected, never edited. Every store path is verified against
  its fingerprint on creation and re-verifiable on demand.
- **PM-I3** The lockfile is generated, authoritative, and sufficient:
  `jet.lock` + the store (or network) reproduces the exact dependency
  tree, byte-for-byte, on any machine.
- **PM-I4** Version-picking never downloads packages — only small
  metadata records (uv's rule). Downloads happen once, after choosing.
- **PM-I5** Registry entries are immutable. Yanking flags a version; it
  never deletes bytes a lockfile may point to.
- **PM-I6** One mechanism per job: one manifest, one lockfile, one
  store layout, one registry protocol. No alternates, no escape
  hatches.
- **PM-I7** Every package-manager diagnostic carries an E12xx code,
  what/why/fix, and a ui snapshot (compiler I4 applies unchanged).
- **PM-I8** R9 stands forever: `jet run file.jet` with no manifest
  works exactly as today, touching none of this machinery.

## 8. Conflicts this file resolves (the reconciliation ledger)

| Conflict | jetpack.md said | m12-packages.md said | Resolution here |
|----------|-----------------|----------------------|-----------------|
| Manifest | pure-Jet `package { }` | `jet.toml` (S52 ratified) | jet.toml (D-PM2); .jet recipes only at layer 3 |
| Pin policy | semver ranges + resolver now | exact pins only | pins phase 1 → ranges phase 2 (D-PM3) |
| Resolver | pubgrub-rs crate | n/a (no ranges) | PubGrub algorithm, written in-tree, std-only (D-PM4) |
| Tool | separate `jetpack` binary, I6-exempt | `jet` subcommands | `jet` subcommands, I6 holds (D-PM4) |
| Store | `/jetpack/store`, own track | none (plain cache dir) | `~/.jet/store`, fingerprint-named, in M12.1 (D-PM1/5) |
| Lockfile | `jetpack.lock`, canonical JSON | `jet.lock` | `jet.lock`, carrying plan fingerprints (generated, never hand-edited) |
| Registry | full registry + cache service | static git index | append-only git index; history = snapshots (D-PM6) |
| Generations/GC | core feature | out of scope | layer 3, installed tools only (D-PM7) |

Open jetpack/jetos decision IDs (D-JP1–5, D-OS1–7) are superseded or
deferred to layer 3's future plan; none remain open against M12.

## 9. Ratification status (2026-06-13)

| Item | Status |
|---|---|
| Manifest layout (D-MF1…5, lock graph, `@latest`) | **Ratified** — manifest-design-research.md, S52 |
| docs/plans/m12-packages.md | **Updated** — M12.1/M12.2 phases, implementation order |
| docs/05-roadmap.md M12 entry | **Updated** |
| D-PM1…D-PM8 architecture | **Recommendations** — implement as written unless owner overrides |
| Layer 3 recipe syntax | **Deferred** — separate ballot before code |

**Agent checklist before M12.1:**

1. Read manifest-design-research.md, this file §6–7, m12-packages.md.
2. Claim E1201–E1209 in docs/04 as diagnostics land.
3. Layer-3 Jet-code manifests / recipes still need ballots — do not implement.
